//! HD packs: folders of pictures that take the place of the original art,
//! at [`SCALE`] times its size, chosen in «Pacchetto HD» of the «GRAFICA»
//! page. A pack lives in `hd-packs/<folder>/` beside the settings, with a
//! `pack.ini` and its pictures named after the archive's, in capitals:
//! `IMAGES/BUILDING/EURO CASTLE.RLE.png` for the sheet `IMAGES/BUILDING/EURO
//! CASTLE.RLE`, twice its size; a sheet with kingdom colours in its light
//! greys, with `<NAME>.PLAYER.png`, the mask of those parts. See
//! `docs/hd-packs.md`.
//!
//! `RETZAR_HD` (a pack folder) and `RETZAR_HD_TERRAIN` (`0` or `1`) take
//! the place of the settings.
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// How many times larger a pack's pictures are than the originals.
pub const SCALE: u32 = 2;
/// The folder of the packs, beside the settings.
pub const FOLDER: &str = "hd-packs";

/// A pack's `pack.ini`, `[pack]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pack {
    /// Its folder in [`FOLDER`], the name the settings keep.
    pub folder: String,
    pub name: String,
    pub author: String,
    pub license: String,
    pub description: String,
    /// Whether it has the ground atlases.
    pub terrain: bool,
}

/// The packs in `root`'s [`FOLDER`] that ReTzar can draw, by name.
pub fn packs(root: &Path) -> Vec<Pack> {
    let mut packs: Vec<Pack> = std::fs::read_dir(root.join(FOLDER))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let folder = e.file_name().to_str()?.to_owned();
            let ini = crate::assets::Ini::parse(&std::fs::read(e.path().join("pack.ini")).ok()?);
            let get = |k: &str| ini.get("pack", k).unwrap_or("").trim().to_owned();
            let scale = get("scale").parse().unwrap_or(SCALE);
            (scale == SCALE).then(|| Pack {
                name: Some(get("name"))
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| folder.clone()),
                author: get("author"),
                license: get("license"),
                description: get("description"),
                terrain: get("terrain") == "1",
                folder,
            })
        })
        .collect();
    packs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    packs
}

/// The pack drawn by the next scene loaded, and whether its ground too.
static ACTIVE: RwLock<Option<(PathBuf, bool)>> = RwLock::new(None);

/// Chooses the pack for the scenes loaded from now on: `folder` in `root`'s
/// [`FOLDER`], none when empty; its ground atlases if `terrain`.
pub fn select(root: &Path, folder: &str, terrain: bool) {
    let chosen = (!folder.is_empty())
        .then(|| packs(root).into_iter().find(|p| p.folder == folder))
        .flatten()
        .map(|p| (root.join(FOLDER).join(&p.folder), terrain && p.terrain));
    if let Ok(mut active) = ACTIVE.write() {
        *active = chosen;
    }
}

/// The folder of the pack drawn.
pub fn dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("RETZAR_HD") {
        return Some(PathBuf::from(dir));
    }
    ACTIVE.read().ok()?.as_ref().map(|(dir, _)| dir.clone())
}

/// Whether the pack drawn takes the place of the ground too.
pub fn terrain() -> bool {
    match std::env::var_os("RETZAR_HD_TERRAIN") {
        Some(v) => v == "1",
        None => ACTIVE
            .read()
            .ok()
            .and_then(|a| a.as_ref().map(|(_, t)| *t))
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_are_read_from_their_manifests() {
        let root = std::env::temp_dir().join(format!("retzar-hd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let write = |folder: &str, ini: &str| {
            let dir = root.join(FOLDER).join(folder);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("pack.ini"), ini).unwrap();
        };
        write(
            "b",
            "[pack]\nname = Beta\nterrain = 1\nlicense = CC-BY-4.0\n",
        );
        write("a", "[pack]\nname = alpha\n");
        write("big", "[pack]\nname = Four times\nscale = 4\n");
        std::fs::create_dir_all(root.join(FOLDER).join("no manifest")).unwrap();
        let found = packs(&root);
        let names: Vec<_> = found.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Beta"]);
        assert!(found[1].terrain && found[1].license == "CC-BY-4.0");
        select(&root, "b", false);
        assert!(dir().is_some_and(|d| d.ends_with("b")) || std::env::var_os("RETZAR_HD").is_some());
        select(&root, "", true);
        let _ = std::fs::remove_dir_all(&root);
    }
}
