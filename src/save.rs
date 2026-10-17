//! Portable session snapshots. No engine pointers, GPU resources or asset atlases.
//! The envelope is independent of the versioned, compressed JSON schema: its
//! version 2 enciphers and authenticates metadata and session together, and
//! the map data kept beside the saves (`blobs`), with the keys of the
//! portable copy's pack ([`crate::pack::seal`]), so that a save is neither
//! read nor changed with ordinary tools. Version 1, checksummed only, is
//! still read.
use crate::{
    Result, bitmap::Bitmap, flight::Flyer, hud::HudState, map::MapGrid, menu::SavedSession,
    simulation::Simulation, world::MissionWorld,
};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

const MAGIC: &[u8; 8] = b"RTZSNAP\0";
/// The schema of [`Snapshot`].
const VERSION: u32 = 1;
/// The envelope written: enciphered.
const SEALED: u32 = 2;
const HEADER: usize = 64;
const TAG: usize = 16;
const MAX_RAW: u64 = 256 * 1024 * 1024;
const MAX_PACKED: u64 = 128 * 1024 * 1024;
const MAX_META: usize = 65536;
const MAX_BLOB: u64 = 64 * 1024 * 1024;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

mod source_bytes {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        entries: &BTreeMap<String, std::sync::Arc<Vec<u8>>>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        entries
            .iter()
            .map(|(name, bytes)| (name, bytes.as_ref()))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<BTreeMap<String, std::sync::Arc<Vec<u8>>>, D::Error> {
        Ok(BTreeMap::<String, Vec<u8>>::deserialize(deserializer)?
            .into_iter()
            .map(|(name, bytes)| (name, std::sync::Arc::new(bytes)))
            .collect())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema: u32,
    pub grid: MapGrid,
    pub biome: String,
    pub title: String,
    pub description: String,
    pub mission: MissionWorld,
    pub starting_positions: [Option<[u16; 2]>; 8],
    pub simulation: Simulation,
    /// All original container entries, including scripts and extensions not yet interpreted.
    #[serde(with = "source_bytes")]
    pub source_entries: BTreeMap<String, std::sync::Arc<Vec<u8>>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub external_entries: BTreeMap<String, String>,
    pub portraits: BTreeMap<String, Bitmap>,
    pub hud: HudState,
    pub groups: [Vec<usize>; 9],
    pub health_kept: bool,
    #[serde(default)]
    pub idle: usize,
    pub visual_time: Duration,
    pub flyers: Vec<Flyer>,
    /// The language the game was saved in, an index of
    /// [`crate::lang::LANGUAGES`] as it was then: the order changed when
    /// English became the default, so loading never relies on it.
    #[serde(default)]
    pub language: usize,
}
impl Snapshot {
    pub fn validate(&self) -> Result<()> {
        let side = self.grid.side;
        let n = self.mission.objects.len();
        if self.schema != VERSION {
            return Err("Versione della sessione non supportata".into());
        }
        if self.source_entries.len() + self.external_entries.len() > 4096
            || self
                .source_entries
                .values()
                .map(|v| v.len() as u64)
                .sum::<u64>()
                > MAX_RAW
            || self
                .external_entries
                .values()
                .any(|hash| hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()))
            || self
                .external_entries
                .keys()
                .any(|key| self.source_entries.contains_key(key))
        {
            return Err("Archivio della mappa non valido".into());
        }
        if !(32..=1024).contains(&side)
            || self.grid.cells.len() != side * side
            || self.simulation.fog.side != side
            || self.simulation.fog.visible.len() != side * side
            || self.simulation.fog.explored.len() != side * side
            || ![0, (side + 1) * (side + 1)].contains(&self.simulation.fog.explored_vertices.len())
            || self.simulation.fog.visible_vertices.len()
                != self.simulation.fog.explored_vertices.len()
            || self.simulation.memory.keys().any(|&i| i >= n)
            || self.simulation.memory.len() > n
            || self.simulation.objects.len() != n
            || !self.simulation.gameplay.validate(n, side)
            || !self.simulation.gameplay.script.validate(&self.mission)
            || n > 100_000
            || self.mission.kingdoms.len() > 8
            || (!self.mission.diplomacy.is_empty() && self.mission.diplomacy.len() != 64)
            || !self.simulation.precipitation.is_finite()
            || !(0. ..=1.).contains(&self.simulation.precipitation)
            || self.simulation.elapsed.as_secs() > 100 * 365 * 86400
            || self.visual_time.as_secs() > 100 * 365 * 86400
        {
            return Err("Dimensioni o stato della sessione non validi".into());
        }
        let mut incoming = vec![0u8; n];
        for (i, state) in self.simulation.objects.iter().enumerate() {
            if state.holder.is_some_and(|h| h >= n || h == i) || state.contents.len() > n {
                return Err("Riferimenti dell'inventario non validi".into());
            }
            for &j in &state.contents {
                if j >= n
                    || j == i
                    || incoming[j] != 0
                    || self.simulation.objects[j].holder != Some(i)
                {
                    return Err("Contenuto dell'inventario non valido".into());
                }
                incoming[j] = 1;
            }
        }
        if self
            .simulation
            .objects
            .iter()
            .enumerate()
            .any(|(i, s)| s.holder.is_some() != (incoming[i] == 1))
        {
            return Err("Collegamento dell'inventario non valido".into());
        }
        let mut colors = vec![0u8; n];
        for i in 0..n {
            let mut cursor = Some(i);
            let mut chain = Vec::new();
            while let Some(j) = cursor {
                match colors[j] {
                    2 => break,
                    1 => return Err("Ciclo nell'inventario".into()),
                    _ => {
                        colors[j] = 1;
                        chain.push(j);
                        cursor = self.simulation.objects[j].holder;
                    }
                }
            }
            for j in chain {
                colors[j] = 2;
            }
        }
        if self.hud.selected.len() > 50
            || self.hud.selected.iter().any(|&i| i >= n)
            || [
                self.hud.inspected,
                self.hud.contents_owner,
                self.hud.inspected_content,
            ]
            .iter()
            .flatten()
            .any(|&i| i >= n)
            || self
                .groups
                .iter()
                .any(|g| g.len() > 50 || g.iter().any(|&i| i >= n))
            || [
                self.hud.diplomacy.as_ref(),
                self.hud.diplomacy_draft.as_ref(),
            ]
            .iter()
            .flatten()
            .any(|d| d.len() != 64)
            || self
                .mission
                .objects
                .iter()
                .any(|o| o.class >= crate::world::CLASSES.len())
        {
            return Err("Riferimenti della mappa o dell'interfaccia non validi".into());
        }
        for picture in self.portraits.values() {
            if picture.width == 0
                || picture.height == 0
                || picture.width > 1024
                || picture.height > 1024
                || picture.rgba.len() != picture.width as usize * picture.height as usize * 4
            {
                return Err("Ritratto salvato non valido".into());
            }
        }
        Ok(())
    }
}

/// A save `saves/<file>.rtzs`. The file name is chosen by the game; the
/// player's name for the save lives in its metadata.
pub fn save_path(root: &Path, file: &str) -> Result<PathBuf> {
    if file.is_empty()
        || file.len() > 64
        || !file
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("Salvataggio non valido".into());
    }
    Ok(root.join(format!("saves/{file}.rtzs")))
}
/// The first unused file name: `save1`, `save2`, …
pub fn new_file(root: &Path) -> String {
    (1..)
        .map(|n| format!("save{n}"))
        .find(|f| {
            let path = root.join(format!("saves/{f}.rtzs"));
            !path.exists() && !backup(&path).exists()
        })
        .unwrap_or_default()
}
/// The saves in `saves/`, newest first. A damaged save is listed from its
/// recovery copy; the old `slotN.json` (versions 1 and 2) are still read.
pub fn list(root: &Path) -> Vec<SavedSession> {
    let dir = root.join("saves");
    let mut stems = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stem = name
            .strip_suffix(".rtzs")
            .or_else(|| name.strip_suffix(".rtzs.bak"))
            .or_else(|| name.strip_suffix(".json").filter(|s| legacy_slot(s)));
        if let Some(stem) = stem {
            stems.insert(stem.to_owned());
        }
    }
    let mut saves: Vec<_> = stems
        .into_iter()
        .filter_map(|stem| {
            let path = save_path(root, &stem).ok()?;
            let legacy = dir.join(format!("{stem}.json"));
            let (mut saved, file) = metadata(&path)
                .map(|m| (m, path.clone()))
                .or_else(|_| metadata(&backup(&path)).map(|m| (m, backup(&path))))
                .ok()
                .or_else(|| {
                    let bytes = std::fs::read(&legacy).ok()?;
                    Some((serde_json::from_slice(&bytes).ok()?, legacy.clone()))
                })?;
            saved.file = stem;
            let time = std::fs::metadata(file).and_then(|m| m.modified()).ok();
            Some((time, saved))
        })
        .collect();
    saves.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.file.cmp(&b.1.file)));
    saves.into_iter().map(|(_, s)| s).collect()
}
/// Saves of the first versions: `slot1` to `slot5`.
fn legacy_slot(stem: &str) -> bool {
    stem.strip_prefix("slot")
        .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5"))
}
/// Removes a save with its recovery copy and its old JSON form.
pub fn delete(root: &Path, file: &str) -> Result<()> {
    let path = save_path(root, file)?;
    let mut files = vec![backup(&path), path];
    if legacy_slot(file) {
        files.push(root.join(format!("saves/{file}.json")));
    }
    for file in files {
        if file.exists() {
            std::fs::remove_file(file)?;
        }
    }
    Ok(())
}
pub fn backup(path: &Path) -> PathBuf {
    path.with_extension("rtzs.bak")
}

/// The envelope's version, metadata length, session length and packed
/// session length.
fn header(bytes: &[u8]) -> Result<(u32, usize, u64, usize)> {
    if bytes.len() != HEADER || &bytes[..8] != MAGIC {
        return Err("Salvataggio non riconosciuto".into());
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into()?);
    if version != 1 && version != SEALED {
        return Err("Versione del salvataggio non supportata".into());
    }
    let meta = u32::from_le_bytes(bytes[12..16].try_into()?) as usize;
    let raw = u64::from_le_bytes(bytes[16..24].try_into()?);
    let packed = u64::from_le_bytes(bytes[24..32].try_into()?);
    if meta > MAX_META || raw > MAX_RAW || packed > MAX_PACKED {
        return Err("Salvataggio troppo grande o danneggiato".into());
    }
    Ok((version, meta, raw, packed as usize))
}
/// What binds a sealed save's contents to its header (sizes and nonce).
fn context(header: &[u8]) -> Vec<u8> {
    [b"save ".as_slice(), &header[..48]].concat()
}
/// The bytes after the header: metadata and packed session, then the tag of
/// a sealed save.
fn body_len(version: u32, meta: usize, packed: usize) -> usize {
    meta + packed + if version == SEALED { TAG } else { 0 }
}
fn metadata_valid(meta: &SavedSession) -> Result<()> {
    if meta.version != 3
        || !meta.snapshot
        || meta.camera.iter().any(|v| !v.is_finite())
        || meta.camera[2] <= 0.
    {
        return Err("Metadati del salvataggio non validi".into());
    }
    Ok(())
}
/// Listing reads only the bounded header, never deserializes the world.
pub fn metadata(path: &Path) -> Result<SavedSession> {
    let mut file = File::open(path)?;
    let mut h = [0; HEADER];
    file.read_exact(&mut h)?;
    let (version, m, _, p) = header(&h)?;
    if file.metadata()?.len() != (HEADER + body_len(version, m, p)) as u64 {
        return Err("Salvataggio troncato".into());
    }
    let mut bytes = vec![0; m];
    file.read_exact(&mut bytes)?;
    if version == SEALED {
        bytes = crate::pack::peek(&context(&h), &bytes);
    }
    let meta: SavedSession = serde_json::from_slice(&bytes)?;
    metadata_valid(&meta)?;
    Ok(meta)
}
pub fn encode(meta: &SavedSession, snapshot: &Snapshot) -> Result<Vec<u8>> {
    metadata_valid(meta)?;
    snapshot.validate()?;
    let metadata = serde_json::to_vec(meta)?;
    let raw = serde_json::to_vec(snapshot)?;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&raw)?;
    let packed = encoder.finish()?;
    let mut h = Vec::with_capacity(HEADER + metadata.len() + packed.len() + TAG);
    h.extend(MAGIC);
    h.extend(SEALED.to_le_bytes());
    h.extend((metadata.len() as u32).to_le_bytes());
    h.extend((raw.len() as u64).to_le_bytes());
    h.extend((packed.len() as u64).to_le_bytes());
    // A nonce of its own: no two saves share a keystream.
    let mut nonce = blake3::Hasher::new();
    nonce.update(&std::process::id().to_le_bytes());
    nonce.update(&TEMP_ID.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    nonce.update(
        &std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
            .to_le_bytes(),
    );
    nonce.update(&raw);
    h.extend(&nonce.finalize().as_bytes()[..16]);
    h.extend([0; 16]);
    header(&h)?;
    let body = [metadata.as_slice(), &packed].concat();
    let sealed = crate::pack::seal(&context(&h), &body);
    h.extend(sealed);
    Ok(h)
}
pub fn decode(bytes: &[u8]) -> Result<(SavedSession, Snapshot)> {
    let h = bytes.get(..HEADER).ok_or("Salvataggio troncato")?;
    let (version, m, raw_len, p) = header(h)?;
    if bytes.len() != HEADER + body_len(version, m, p) {
        return Err("Lunghezza del salvataggio non valida".into());
    }
    let body = if version == SEALED {
        crate::pack::unseal(&context(h), &bytes[HEADER..])
            .map_err(|_| "Controllo d'integrità del salvataggio fallito")?
    } else {
        let mut hash = blake3::Hasher::new();
        hash.update(&h[..32]);
        hash.update(&bytes[HEADER..]);
        if hash.finalize().as_bytes() != &h[32..64] {
            return Err("Controllo d'integrità del salvataggio fallito".into());
        }
        bytes[HEADER..].to_vec()
    };
    let meta: SavedSession = serde_json::from_slice(&body[..m])?;
    metadata_valid(&meta)?;
    let mut decoder = ZlibDecoder::new(&body[m..]);
    let mut raw = Vec::new();
    (&mut decoder).take(raw_len + 1).read_to_end(&mut raw)?;
    if raw.len() as u64 != raw_len || decoder.total_in() != p as u64 {
        return Err("Sessione compressa danneggiata".into());
    }
    let mut snapshot: Snapshot = match serde_json::from_slice(&raw) {
        Ok(snapshot) => snapshot,
        // A save of an earlier ReTzar whose units' orders no longer read
        // resumes without them, as a snapshot without gameplay does: the
        // world, the resources and the rest of the state stay.
        Err(_) => {
            const OLD: &str =
                "Salvataggio di una versione precedente di ReTzar: non si può più riprendere";
            let mut v: serde_json::Value = serde_json::from_slice(&raw).map_err(|_| OLD)?;
            v.pointer_mut("/simulation/gameplay")
                .and_then(|g| g.as_object_mut())
                .ok_or(OLD)?
                .remove("actors");
            serde_json::from_value(v).map_err(|_| OLD)?
        }
    };
    snapshot.simulation.gameplay.ai.upgrade(&snapshot.mission);
    snapshot.validate()?;
    // A game resumes in the language of the moment, whichever it was saved
    // in: texts already in it stay as they are. A random map's names come
    // from no catalog and stay as they are.
    let language = crate::lang::current();
    if !matches!(meta.selection, crate::menu::Selection::Random { .. }) {
        crate::lang::localize_mission(&mut snapshot.mission, language);
        let script = &mut snapshot.simulation.gameplay.script;
        if let Some(text) = crate::lang::translate(&script.end_text, language) {
            script.end_text = text.to_owned();
        }
        for text in [&mut snapshot.title, &mut snapshot.description] {
            if let Some(t) = crate::lang::translate(text, language) {
                *text = t.to_owned();
            }
        }
    }
    snapshot.language = language.index();
    Ok((meta, snapshot))
}
fn read_file(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_PACKED + MAX_META as u64 + HEADER as u64 {
        return Err("Salvataggio troppo grande".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_PACKED + MAX_META as u64 + HEADER as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}
pub fn read(path: &Path) -> Result<(SavedSession, Snapshot, bool)> {
    match read_snapshot(path) {
        Ok((m, s)) => Ok((m, s, false)),
        Err(main) => match read_snapshot(&backup(path)) {
            Ok((m, s)) => Ok((m, s, true)),
            Err(_) => Err(main),
        },
    }
}

fn blob_path(save: &Path, hash: &str) -> Result<PathBuf> {
    Ok(save
        .parent()
        .ok_or("Percorso del salvataggio non valido")?
        .join("blobs")
        .join(format!("{hash}.rtzb")))
}
/// A map datum kept beside the saves and whether it is enciphered
/// (`RTZBLOB2`; the plain `RTZBLOB\0` of earlier versions is still read).
fn read_blob(path: &Path, hash: &str) -> Result<(Vec<u8>, bool)> {
    let bytes = read_file(path)?;
    let sealed = match bytes.get(..8) {
        Some(b"RTZBLOB2") => true,
        Some(b"RTZBLOB\0") => false,
        _ => return Err("Dato della mappa non riconosciuto".into()),
    };
    if bytes.len() < 16 {
        return Err("Dato della mappa non riconosciuto".into());
    }
    let length = u64::from_le_bytes(bytes[8..16].try_into()?);
    if length > MAX_BLOB {
        return Err("Dato della mappa troppo grande".into());
    }
    let packed = if sealed {
        crate::pack::unseal(&blob_context(hash), &bytes[16..])
            .map_err(|_| "Dato della mappa danneggiato")?
    } else {
        bytes[16..].to_vec()
    };
    let mut decoder = ZlibDecoder::new(packed.as_slice());
    let mut raw = Vec::new();
    (&mut decoder).take(length + 1).read_to_end(&mut raw)?;
    if raw.len() as u64 != length
        || decoder.total_in() != packed.len() as u64
        || blake3::hash(&raw).to_hex().as_str() != hash
    {
        return Err("Dato della mappa danneggiato".into());
    }
    Ok((raw, sealed))
}
fn blob_context(hash: &str) -> Vec<u8> {
    [b"blob ".as_slice(), hash.as_bytes()].concat()
}
fn read_snapshot(path: &Path) -> Result<(SavedSession, Snapshot)> {
    let (meta, mut snapshot) = decode(&read_file(path)?)?;
    let mut total: u64 = snapshot
        .source_entries
        .values()
        .map(|v| v.len() as u64)
        .sum();
    for (name, hash) in std::mem::take(&mut snapshot.external_entries) {
        let (bytes, _) = read_blob(&blob_path(path, &hash)?, &hash)?;
        total += bytes.len() as u64;
        if total > MAX_RAW {
            return Err("Archivio della mappa troppo grande".into());
        }
        snapshot
            .source_entries
            .insert(name, std::sync::Arc::new(bytes));
    }
    snapshot.validate()?;
    Ok((meta, snapshot))
}
/// Flush a unique same-directory temporary file, then replace in one filesystem operation.
fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or("Percorso del salvataggio non valido")?;
    fs::create_dir_all(dir)?;
    let temp = dir.join(format!(
        ".retzar-{}-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos(),
        TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
            }
            let from: Vec<_> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            let to: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        #[cfg(not(windows))]
        {
            fs::rename(&temp, path)?;
            File::open(dir)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
pub fn write(path: &Path, meta: &SavedSession, snapshot: &Snapshot) -> Result<()> {
    snapshot.validate()?;
    if !snapshot.external_entries.is_empty() {
        return Err("Salvataggio con dati della mappa non risolti".into());
    }
    // Immutable scripts, portraits and speech are stored once, shared by slots
    // and their recovery copies. The slot stores only authenticated references.
    let mut compact = snapshot.clone();
    let large: Vec<_> = compact
        .source_entries
        .iter()
        .filter(|(_, b)| b.len() > 4096)
        .map(|(name, _)| name.clone())
        .collect();
    for name in large {
        let raw = compact.source_entries.remove(&name).unwrap();
        if raw.len() as u64 > MAX_BLOB {
            return Err("Dato della mappa troppo grande".into());
        }
        let hash = blake3::hash(&raw).to_hex().to_string();
        let blob = blob_path(path, &hash)?;
        // Written again when missing, damaged or of an earlier, plain form.
        if !matches!(read_blob(&blob, &hash), Ok((_, true))) {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
            encoder.write_all(&raw)?;
            let mut data = b"RTZBLOB2".to_vec();
            data.extend((raw.len() as u64).to_le_bytes());
            data.extend(crate::pack::seal(&blob_context(&hash), &encoder.finish()?));
            replace(&blob, &data)?;
        }
        compact.external_entries.insert(name, hash);
    }
    let bytes = encode(meta, &compact)?;
    // A damaged current file must never overwrite the last valid recovery copy.
    if let Ok(previous) = read_file(path)
        && read_snapshot(path).is_ok()
    {
        replace(&backup(path), &previous)?;
    }
    replace(path, &bytes)
}
