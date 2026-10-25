//! ReTzar ships the whole game, every edition's campaigns, voices and
//! languages, and starts beside any copy of Tzar. The folder the player
//! chooses must hold some of the original game's files, known here only by
//! a digest of their names: any edition passes, in any language and modded
//! or not, with its archives or its loose files wherever they lie in the
//! folder, and a folder without the game does not.
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

/// Digests ([`digest`]) of the names of the files looked for.
const KNOWN: [[u8; 16]; 8] = [
    [
        203, 101, 216, 159, 254, 96, 194, 121, 92, 104, 61, 84, 33, 136, 145, 185,
    ],
    [
        45, 57, 243, 83, 42, 134, 153, 242, 116, 122, 37, 212, 240, 123, 209, 233,
    ],
    [
        179, 5, 94, 189, 201, 134, 35, 151, 247, 11, 154, 17, 141, 223, 140, 190,
    ],
    [
        53, 15, 251, 85, 51, 242, 204, 184, 50, 146, 112, 49, 144, 63, 74, 142,
    ],
    [
        151, 30, 236, 161, 149, 85, 245, 78, 102, 29, 210, 75, 151, 75, 106, 81,
    ],
    [
        248, 250, 210, 51, 133, 14, 46, 243, 167, 7, 104, 19, 207, 244, 159, 101,
    ],
    [
        63, 72, 242, 115, 227, 231, 32, 100, 149, 48, 128, 200, 179, 202, 38, 18,
    ],
    [
        231, 165, 7, 73, 11, 138, 142, 10, 135, 57, 103, 144, 31, 206, 85, 127,
    ],
];
/// How many of them the folder must hold.
const NEEDED: usize = 4;
/// How deep below the chosen folder, and through how many files, the search
/// goes before giving up.
const DEPTH: usize = 4;
const FILES: usize = 50_000;
/// Where the folder is kept, in `settings.json`.
const KEY: &str = "original_folder";

/// The digest of a file's name, without its folders and in lower case.
fn digest(path: &str) -> [u8; 16] {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let hash = blake3::Hasher::new_derive_key("ReTzar original file name")
        .update(name.to_ascii_lowercase().as_bytes())
        .finalize();
    let mut out = [0; 16];
    out.copy_from_slice(&hash.as_bytes()[..16]);
    out
}

/// Notes the file named `path` among those found when its name is known.
fn note(found: &mut BTreeSet<usize>, path: &str) {
    let d = digest(path);
    if let Some(i) = KNOWN.iter().position(|k| *k == d) {
        found.insert(i);
    }
}

/// Whether `folder` holds a copy of Tzar: the folders nearest to it first,
/// the names inside its archives and those of its loose files.
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
                    note(&mut found, &e.name);
                }
            } else {
                note(&mut found, &entry.file_name().to_string_lossy());
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
pub(crate) fn saved(root: &Path) -> Option<PathBuf> {
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

/// Lets the game start only beside a copy of Tzar: the folder chosen
/// before, or one the player chooses now and that is kept. Closing a
/// window ends the game with the reason.
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

    /// A name is the same in any folder and in any case.
    #[test]
    fn a_name_is_known_whatever_its_folder_and_case() {
        assert_eq!(
            digest("IMAGES\\UNIT\\Some File.RLE"),
            digest("some file.rle")
        );
        assert_eq!(digest("a/b/SOME FILE.rle"), digest("Some File.rle"));
        assert_ne!(digest("some file.rle"), digest("some file.wav"));
    }

    /// A folder without the game holds no copy, archives or not; the files
    /// known outnumber those asked for.
    #[test]
    fn a_folder_without_the_game_holds_no_copy() {
        assert!(NEEDED <= KNOWN.len());
        let dir = scratch("vuota");
        for k in 0..KNOWN.len() {
            let folder = dir.join("a").join(k.to_string());
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("x.wdt"), vec![0u8; 64]).unwrap();
            std::fs::write(dir.join(format!("{k}.bin")), vec![7u8; 64]).unwrap();
        }
        assert!(!holds(&dir));
        assert!(!holds(&dir.join("assente")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Every edition at hand passes with its archives (the Italian one, the
    /// American CD, the Spanish Anthology with its mods), and so do the
    /// files taken out loose, empty and under any folder, as other editions
    /// and mods keep them; fewer than those asked for do not.
    #[test]
    #[ignore = "Requires the original installation"]
    fn any_edition_passes_with_archives_or_loose_files() {
        let tzar = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        assert!(holds(&tzar));
        for other in ["USA/CD/SETUP/Tzar", "Anthology"] {
            if tzar.join(other).is_dir() {
                assert!(holds(&tzar.join(other)), "{other}");
            }
        }
        let mut names = BTreeSet::new();
        for entry in std::fs::read_dir(tzar.join("Packs")).unwrap().flatten() {
            let Ok(a) = Archive::open(&entry.path()) else {
                continue;
            };
            for e in &a.entries {
                if KNOWN.contains(&digest(&e.name)) {
                    names.insert(e.name.rsplit(['/', '\\']).next().unwrap().to_owned());
                }
            }
        }
        assert_eq!(names.len(), KNOWN.len());
        let dir = scratch("sciolti");
        for (k, name) in names.iter().enumerate() {
            let folder = dir.join("Gioco").join(format!("cartella {}", k % 3));
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(name.to_ascii_lowercase()), b"").unwrap();
            assert_eq!(holds(&dir), k + 1 >= NEEDED, "{} file", k + 1);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
