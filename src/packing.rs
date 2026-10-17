//! The portable copy's pack made from an installation of any edition
//! (`retzar-tools --crea-pacchetto`): its archives (or, for a digital
//! release that keeps its data loose, archives made of those folders under
//! the names the engine reads), maps, campaigns and `Tzar.ini`, the
//! American release's campaign, and the movies turned into MP4: the Italian
//! intro and the American DDV, decoded with the tables of an installed
//! `Tzar.exe` that is not encrypted, which stays out of the pack. The pack
//! is read back whole before it is kept: every file must give back its
//! bytes.
use crate::{
    Result,
    archive::Archive,
    ddv,
    hpfs::Container,
    invalid,
    media::{self, Kind, Source},
    pack::{self, Codec, Folder, FolderEntry, Item, Node, Pack, PackWriter},
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// The quality of the movies' pictures (of 100): their own blocks, no new
/// ones of the encoder.
const QUALITY: u32 = 90;

/// What went into a pack.
#[derive(Debug, Default)]
pub struct Summary {
    pub files: usize,
    pub resources: usize,
    pub original: u64,
    pub packed: u64,
    /// Bytes saved by deflate, PNG and FLAC.
    pub saved: [u64; 3],
}

/// Records for `items`, the new ones encoded on every core.
fn add_all(w: &mut PackWriter, items: &[(String, Vec<u8>)]) -> Result<Vec<u32>> {
    let mut todo: Vec<usize> = Vec::new();
    let mut fresh = std::collections::HashSet::new();
    for (i, (_, bytes)) in items.iter().enumerate() {
        if w.known(bytes).is_none() && fresh.insert(*blake3::hash(bytes).as_bytes()) {
            todo.push(i);
        }
    }
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut encoded: Vec<Option<(Codec, Vec<u8>)>> = vec![None; items.len()];
    std::thread::scope(|scope| {
        let chunks: Vec<_> = todo
            .chunks(todo.len().div_ceil(threads).max(1))
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|&i| (i, pack::encode(&items[i].0, &items[i].1)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for c in chunks {
            for (i, e) in c.join().expect("codifica") {
                encoded[i] = Some(e);
            }
        }
    });
    let mut ids = Vec::with_capacity(items.len());
    for ((_, bytes), e) in items.iter().zip(encoded) {
        ids.push(match e {
            Some((codec, payload)) => w.add_encoded(bytes, codec, payload)?,
            None => w.known(bytes).ok_or_else(|| invalid("Record perso"))?,
        });
    }
    Ok(ids)
}

fn folder(w: &mut PackWriter, c: &Container, summary: &mut Summary) -> Result<Folder> {
    // Each entry: a file, the next of `items`, or a container of maps.
    let mut items = Vec::new();
    let mut inner = Vec::with_capacity(c.entries.len());
    for e in &c.entries {
        let nested = if e.name.to_ascii_lowercase().ends_with(".wmp") {
            c.open_nested(&e.name).ok()
        } else {
            None
        };
        if nested.is_none() {
            items.push((e.name.clone(), c.read(e.inode)?));
        }
        inner.push(nested);
    }
    summary.resources += items.len();
    summary.original += items.iter().map(|(_, b)| b.len() as u64).sum::<u64>();
    let mut ids = add_all(w, &items)?.into_iter();
    let mut entries = Vec::with_capacity(c.entries.len());
    for (e, nested) in c.entries.iter().zip(inner) {
        let item = match nested {
            Some(n) => Item::Nested(folder(w, &n, summary)?),
            None => Item::Record(ids.next().ok_or_else(|| invalid("Ordine perso"))?),
        };
        entries.push(FolderEntry {
            name: e.name.clone(),
            inode: e.inode,
            size: e.size,
            item,
        });
    }
    Ok(Folder {
        block_size: c.header.block_size,
        declared_blocks: c.header.declared_blocks,
        bytes: c.header.bytes,
        entries,
    })
}

/// The files of `dir` with `extension`, sorted; none when it is missing.
fn files_in(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case(extension))
        })
        .collect();
    files.sort();
    files
}
fn name_of(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The folders a digital release keeps loose, under the names of the
/// archives that hold them in the other editions.
const LOOSE: [(&str, &[&str]); 5] = [
    ("data.wdt", &["DATA"]),
    ("help.wdt", &["HELP"]),
    ("images.wdt", &["IMAGES"]),
    ("local.wdt", &["LOCAL"]),
    ("sounds.wdt", &["SOUND", "MUSIC"]),
];

/// The archives of an installation, by their name in the pack: those of its
/// `Packs` but the movies' or, when it keeps its data loose, archives made
/// of those folders.
fn archives(source: &Path) -> Result<Vec<(String, Archive)>> {
    let mut out = Vec::new();
    for path in files_in(&source.join("Packs"), "wdt") {
        let name = name_of(&path);
        if !name.eq_ignore_ascii_case("movies.wdt") {
            let archive = Archive::open(&path).map_err(|e| format!("{name}: {e}"))?;
            out.push((name, archive));
        }
    }
    if !out
        .iter()
        .any(|(n, _)| n.eq_ignore_ascii_case("images.wdt"))
    {
        for (name, tops) in LOOSE {
            if out.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
                continue;
            }
            let archive = Archive::folder(source, tops)?;
            if !archive.entries.is_empty() {
                out.push((name.to_owned(), archive));
            }
        }
    }
    if !out
        .iter()
        .any(|(n, _)| n.eq_ignore_ascii_case("images.wdt"))
    {
        return Err(invalid(format!(
            "{}: Tzar non trovato (mancano Packs, o DATA e IMAGES)",
            source.display()
        )));
    }
    Ok(out)
}

/// Writes the pack at `out` from the installation `source`, of any edition,
/// and, when given, the American CD's `Tzar` folder `american` (its
/// campaign and its movies); `work` holds the movies while they are
/// converted.
pub fn build(
    source: &Path,
    american: Option<&Path>,
    out: &Path,
    work: &Path,
    log: &mut dyn FnMut(String),
) -> Result<Summary> {
    let mut summary = Summary::default();
    let partial = out.with_extension("parziale");
    let mut w = PackWriter::create(&partial)?;
    for (name, archive) in archives(source)? {
        log(format!("Archivio {name}"));
        let items: Vec<(String, Vec<u8>)> = archive
            .entries
            .iter()
            .map(|e| Ok((e.name.clone(), archive.read(e)?)))
            .collect::<Result<_>>()?;
        summary.resources += items.len();
        summary.original += items.iter().map(|(_, b)| b.len() as u64).sum::<u64>();
        let ids = add_all(&mut w, &items)?;
        let entries = items.into_iter().map(|(n, _)| n).zip(ids).collect();
        w.file(&format!("Packs/{name}"), Node::Archive(entries));
        summary.files += 1;
    }
    let mut containers: Vec<(String, PathBuf)> = Vec::new();
    for path in files_in(&source.join("Maps"), "wmp") {
        containers.push((format!("Maps/{}", name_of(&path)), path));
    }
    for path in files_in(&source.join("Campaign"), "wcm") {
        containers.push((format!("Campaign/{}", name_of(&path)), path));
    }
    if let Some(cd) = american {
        let campaign = cd.join("Campaign/Tzar.wcm");
        if campaign.is_file()
            && !containers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("Campaign/Tzar.wcm"))
        {
            containers.push(("Campaign/Tzar.wcm".into(), campaign));
        }
    }
    for (name, path) in containers {
        log(format!("Contenitore {name}"));
        let c = Container::open(&path)?;
        let f = folder(&mut w, &c, &mut summary)?;
        w.file(&name, Node::Container(f));
        summary.files += 1;
    }
    // `Tzar.ini`, which an edition may lack.
    if let Ok(ini) = std::fs::read(source.join("Tzar.ini")) {
        summary.original += ini.len() as u64;
        let id = w.add("Tzar.ini", &ini)?;
        w.file("Tzar.ini", Node::File(id));
        summary.files += 1;
    }
    // The movies, as MP4.
    std::fs::create_dir_all(work)?;
    let mut movies: Vec<(String, Source)> = Vec::new();
    if let Some(intro) = media::italian_intro(source) {
        movies.push((
            "intro".into(),
            Source::Media(std::fs::read(intro)?, Kind::Mpeg),
        ));
    }
    // The American release's movies, the installation's own (an archive, or
    // the loose folder of a digital release) or the American CD's, decoded
    // with the tables of an installed `Tzar.exe` that is not encrypted.
    let ddv = ddv::american_movies(source)
        .or_else(|| american.and_then(|cd| Archive::open(&cd.join("Packs/movies.wdt")).ok()));
    if let Some(archive) = ddv {
        let tables = std::iter::once(source)
            .chain(american)
            .find_map(|dir| ddv::Tables::of_installation(dir).ok());
        match tables {
            Some(tables) => {
                let tables = Arc::new(tables);
                for e in &archive.entries {
                    movies.push((
                        media::movie_name(&e.name),
                        Source::Ddv(archive.read(e)?, tables.clone()),
                    ));
                }
            }
            None => log(
                "Filmati americani esclusi: nessun Tzar.exe leggibile (quello del CD americano è cifrato)"
                    .into(),
            ),
        }
    }
    for (name, source) in movies {
        log(format!("Filmato {name}.mp4"));
        let file = work.join(format!("{name}.mp4"));
        #[cfg(windows)]
        media::to_mp4(source, &file, QUALITY)?;
        #[cfg(not(windows))]
        {
            let _ = source;
            return Err(invalid("La conversione dei filmati richiede Windows"));
        }
        let bytes = std::fs::read(&file)?;
        std::fs::remove_file(&file)?;
        let id = w.add("x.mp4", &bytes)?;
        w.file(&format!("{}/{name}.mp4", media::MOVIES), Node::File(id));
        summary.files += 1;
    }
    let _ = std::fs::remove_dir(work);
    summary.saved = [w.saved[1], w.saved[2], w.saved[3]];
    summary.packed = w.finish()?;
    log("Verifica del pacchetto".into());
    verify(&partial, source, american)?;
    if out.exists() {
        std::fs::remove_file(out)?;
    }
    std::fs::rename(&partial, out)?;
    Ok(summary)
}

fn same_folder(pack: &Pack, f: &Folder, c: &Container, name: &str) -> Result<()> {
    let differs = || invalid(format!("{name}: diverso dall'originale"));
    if f.entries.len() != c.entries.len()
        || (f.block_size, f.declared_blocks, f.bytes)
            != (
                c.header.block_size,
                c.header.declared_blocks,
                c.header.bytes,
            )
    {
        return Err(differs());
    }
    for (p, o) in f.entries.iter().zip(&c.entries) {
        if (p.name.as_str(), p.inode, p.size) != (o.name.as_str(), o.inode, o.size) {
            return Err(differs());
        }
        match &p.item {
            Item::Record(id) => {
                if pack.record(*id)? != c.read(o.inode)? {
                    return Err(invalid(format!("{name}/{}: byte diversi", o.name)));
                }
            }
            Item::Nested(n) => same_folder(pack, n, &c.open_nested(&o.name)?, &o.name)?,
        }
    }
    Ok(())
}

/// Every file of the pack against the original, and every movie decoded.
pub fn verify(path: &Path, source: &Path, american: Option<&Path>) -> Result<()> {
    let pack = Pack::open(path)?;
    let original = |name: &str| -> PathBuf {
        let at = source.join(name);
        match american {
            Some(cd) if !at.exists() => cd.join(name),
            _ => at,
        }
    };
    let archives = archives(source)?;
    for (name, node) in pack.files() {
        match node {
            Node::Archive(entries) => {
                let short = name.rsplit('/').next().unwrap_or(name);
                let a = archives
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(short))
                    .map(|(_, a)| a)
                    .ok_or_else(|| invalid(format!("{name}: archivio sconosciuto")))?;
                if entries.len() != a.entries.len() {
                    return Err(invalid(format!("{name}: voci diverse")));
                }
                for ((n, id), e) in entries.iter().zip(&a.entries) {
                    if *n != e.name || pack.record(*id)? != a.read(e)? {
                        return Err(invalid(format!("{name}/{n}: byte diversi")));
                    }
                }
            }
            Node::Container(f) => same_folder(&pack, f, &Container::open(&original(name))?, name)?,
            Node::File(id) if name.to_ascii_lowercase().ends_with(".mp4") => {
                let mut m = media::Media::open(&pack.record(*id)?, Kind::Mp4)?;
                if m.width == 0 || m.next_frame()?.is_none() || m.audio.is_none() {
                    return Err(invalid(format!("{name}: filmato illeggibile")));
                }
            }
            Node::File(id) => {
                if pack.record(*id)? != std::fs::read(original(name))? {
                    return Err(invalid(format!("{name}: byte diversi")));
                }
            }
        }
    }
    Ok(())
}
