//! The portable copy plays beside an original copy of Tzar. The folder the
//! player chooses must hold some of the original game's own files, known
//! here only by their size and a digest of their bytes: any edition passes,
//! with its archives or its loose files wherever they lie in the folder, and
//! a folder without the game does not.
use crate::{
    Result,
    archive::Archive,
    invalid,
    lang::{self, Language},
};
use std::{
    collections::{BTreeSet, VecDeque},
    path::{Path, PathBuf},
};

/// Size and digest ([`digest`]) of the files looked for.
const KNOWN: [(u64, [u8; 16]); 8] = [
    (
        594063,
        [
            34, 114, 95, 39, 197, 8, 91, 84, 141, 147, 146, 246, 32, 126, 244, 176,
        ],
    ),
    (
        143478,
        [
            242, 195, 241, 129, 168, 97, 21, 155, 130, 118, 163, 123, 17, 66, 169, 18,
        ],
    ),
    (
        227694,
        [
            181, 170, 40, 207, 62, 176, 157, 188, 235, 9, 104, 128, 241, 254, 251, 245,
        ],
    ),
    (
        525609,
        [
            131, 39, 227, 82, 213, 150, 8, 182, 43, 165, 149, 11, 34, 27, 20, 131,
        ],
    ),
    (
        671397,
        [
            233, 12, 108, 236, 220, 195, 137, 51, 94, 198, 179, 147, 177, 176, 183, 240,
        ],
    ),
    (
        116854,
        [
            162, 27, 131, 38, 112, 62, 236, 241, 16, 145, 194, 3, 216, 17, 35, 154,
        ],
    ),
    (
        231972,
        [
            152, 167, 252, 215, 111, 161, 222, 87, 141, 220, 237, 84, 31, 246, 168, 69,
        ],
    ),
    (
        339864,
        [
            26, 109, 4, 4, 149, 214, 186, 188, 50, 228, 218, 109, 211, 236, 230, 57,
        ],
    ),
];
/// How many of them the folder must hold.
const NEEDED: usize = 4;
/// How deep below the chosen folder, and through how many files, the search
/// goes before giving up.
const DEPTH: usize = 4;
const FILES: usize = 50_000;
/// Where the folder is kept, in `settings.json`.
const KEY: &str = "original_folder";

fn digest(bytes: &[u8]) -> [u8; 16] {
    let hash = blake3::Hasher::new_derive_key("ReTzar original copy")
        .update(bytes)
        .finalize();
    let mut out = [0; 16];
    out.copy_from_slice(&hash.as_bytes()[..16]);
    out
}

/// Notes the file of `size` among those found when its bytes, read only
/// when the size is one looked for, have a known digest.
fn note(found: &mut BTreeSet<usize>, size: u64, read: impl FnOnce() -> Option<Vec<u8>>) {
    if !KNOWN
        .iter()
        .enumerate()
        .any(|(i, (s, _))| *s == size && !found.contains(&i))
    {
        return;
    }
    let Some(bytes) = read() else {
        return;
    };
    let d = digest(&bytes);
    if let Some(i) = KNOWN.iter().position(|(s, k)| *s == size && *k == d) {
        found.insert(i);
    }
}

/// Whether `folder` holds an original copy of Tzar: the folders nearest to
/// it first, the archives opened and the loose files read only where a size
/// matches.
pub fn holds(folder: &Path) -> bool {
    let mut found = BTreeSet::new();
    let mut queue = VecDeque::from([(folder.to_path_buf(), 0)]);
    let mut seen = 0;
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                if depth < DEPTH {
                    queue.push_back((path, depth + 1));
                }
                continue;
            }
            seen += 1;
            if seen > FILES {
                return false;
            }
            let archive = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("wdt"));
            if archive && let Ok(a) = Archive::open(&path) {
                for e in &a.entries {
                    note(&mut found, e.size as u64, || a.read(e).ok());
                }
            } else {
                note(&mut found, meta.len(), || std::fs::read(&path).ok());
            }
            if found.len() >= NEEDED {
                return true;
            }
        }
    }
    false
}

fn settings(root: &Path) -> serde_json::Value {
    std::fs::read(root.join("settings.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}))
}
/// The folder chosen before.
fn saved(root: &Path) -> Option<PathBuf> {
    settings(root).get(KEY)?.as_str().map(PathBuf::from)
}
/// Keeps the chosen folder in `settings.json`, with the rest of it.
pub fn remember(root: &Path, folder: &Path) -> Result<()> {
    let mut s = settings(root);
    s[KEY] = folder.to_string_lossy().into_owned().into();
    crate::write_atomic(&root.join("settings.json"), &serde_json::to_vec_pretty(&s)?)
}
/// The questions in the player's language, before the game has started:
/// the one of `settings.json`, else the default.
pub(crate) fn speak(root: &Path) {
    let language = settings(root)
        .get("language")
        .and_then(serde_json::Value::as_u64)
        .map_or(crate::menu::Settings::default().language, |i| i as usize);
    lang::set(Language::from_index(language));
}
/// Why the game does not start when no folder is chosen.
pub(crate) fn refused() -> Box<dyn std::error::Error + Send + Sync> {
    invalid(lang::t(
        "Per giocare con ReTzar serve una copia originale di Tzar (dal CD, da GOG o da Steam): alla prossima partenza scegli la cartella in cui è installata.",
    ))
}

/// Lets the portable copy start only beside an original copy of Tzar: the
/// folder chosen before, or one the player chooses now and that is kept.
/// Closing the window ends the game with the reason.
pub fn require(root: &Path) -> Result<()> {
    let before = saved(root);
    if before.as_deref().is_some_and(holds) {
        return Ok(());
    }
    speak(root);
    let mut start = before;
    loop {
        let chosen = crate::picker::folder(
            lang::t("Scegli la cartella in cui è installato Tzar"),
            start.as_deref(),
        )
        .ok_or_else(refused)?;
        if holds(&chosen) {
            remember(root, &chosen)?;
            return Ok(());
        }
        if !crate::picker::confirm(&lang::fill(
            "In {} non c'è una copia originale di Tzar.\n\nScegli la cartella in cui è installato il gioco.",
            &[&chosen.display()],
        )) {
            return Err(refused());
        }
        start = Some(chosen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("retzar-original-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A folder without the game holds no copy, even with archives and
    /// files of the right sizes; the files known outnumber those asked for.
    #[test]
    fn a_folder_without_the_game_holds_no_copy() {
        assert!(NEEDED <= KNOWN.len());
        let dir = scratch("vuota");
        for (k, (size, _)) in KNOWN.iter().enumerate() {
            let folder = dir.join("a").join(k.to_string());
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("x.wdt"), vec![0u8; *size as usize]).unwrap();
            std::fs::write(dir.join(format!("{k}.bin")), vec![7u8; *size as usize]).unwrap();
        }
        assert!(!holds(&dir));
        assert!(!holds(&dir.join("assente")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The installation passes with its archives, and so do its files taken
    /// out loose, under any name and folder, as other editions keep them;
    /// fewer than those asked for do not.
    #[test]
    #[ignore = "Requires the original installation"]
    fn the_original_passes_with_archives_or_loose_files() {
        let installation = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        assert!(holds(&installation));
        let mut files = Vec::new();
        for entry in std::fs::read_dir(installation.join("Packs"))
            .unwrap()
            .flatten()
        {
            let Ok(a) = Archive::open(&entry.path()) else {
                continue;
            };
            for e in &a.entries {
                if KNOWN.iter().any(|(s, _)| *s == e.size as u64) {
                    let bytes = a.read(e).unwrap();
                    let d = digest(&bytes);
                    if KNOWN.iter().any(|(s, k)| *s == e.size as u64 && *k == d) {
                        files.push(bytes);
                    }
                }
            }
        }
        assert_eq!(files.len(), KNOWN.len());
        let dir = scratch("sciolti");
        for (k, bytes) in files.iter().enumerate() {
            let folder = dir.join("Gioco").join(format!("cartella {}", k % 3));
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(format!("{k}.dat")), bytes).unwrap();
            assert_eq!(holds(&dir), k + 1 >= NEEDED, "{} file", k + 1);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
