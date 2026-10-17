//! Where ReTzar reads the files of the game: the installation's folders as
//! they are, or a pack ([`crate::pack`]) mounted in their place. A pack is
//! mounted at its own path, so `X\ReTzar.dat\Packs\data.wdt` is the archive
//! `Packs/data.wdt` inside `X\ReTzar.dat`; every other path is the disk's.
use crate::{
    Result, invalid,
    pack::{Folder, Node, Pack},
};
use std::{
    path::{Component, Path, PathBuf},
    sync::{Arc, RwLock},
};

static MOUNTED: RwLock<Vec<Arc<Pack>>> = RwLock::new(Vec::new());

/// Opens the pack at `path` and reads it in place of the folders from now
/// on; a pack already mounted stays as it is.
pub fn mount(path: &Path) -> Result<Arc<Pack>> {
    if let Some(p) = MOUNTED
        .read()
        .unwrap()
        .iter()
        .find(|p| same(p.path(), path))
    {
        return Ok(p.clone());
    }
    let pack = Arc::new(Pack::open(path)?);
    MOUNTED.write().unwrap().push(pack.clone());
    Ok(pack)
}
/// Forgets a pack mounted by a test, so its file can go.
#[cfg(test)]
pub(crate) fn unmount(path: &Path) {
    MOUNTED.write().unwrap().retain(|p| !same(p.path(), path));
}

fn names(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Prefix(p) => Some(p.as_os_str().to_string_lossy().to_ascii_uppercase()),
            Component::RootDir => Some(String::new()),
            Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            Component::CurDir => None,
            Component::ParentDir => Some("..".into()),
        })
        .collect()
}
fn same(a: &Path, b: &Path) -> bool {
    let (a, b) = (names(a), names(b));
    a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}
/// The mounted pack a path lies in, and the path inside it.
fn inside(path: &Path) -> Option<(Arc<Pack>, String)> {
    let mounted = MOUNTED.read().unwrap();
    if mounted.is_empty() {
        return None;
    }
    let full = names(path);
    mounted.iter().find_map(|pack| {
        let base = names(pack.path());
        (full.len() >= base.len()
            && full
                .iter()
                .zip(&base)
                .all(|(x, y)| x.eq_ignore_ascii_case(y)))
        .then(|| (pack.clone(), full[base.len()..].join("/")))
    })
}

/// A whole file.
pub fn read(path: &Path) -> Result<Vec<u8>> {
    match inside(path) {
        Some((pack, rest)) => match pack.node(&rest) {
            Some(Node::File(id)) => pack.record(*id),
            _ => Err(invalid(format!("File non trovato: {}", path.display()))),
        },
        None => Ok(std::fs::read(path)?),
    }
}
/// Whether a path is a file (an archive and a container are files too).
pub fn is_file(path: &Path) -> bool {
    match inside(path) {
        Some((pack, rest)) => !rest.is_empty() && pack.node(&rest).is_some(),
        None => path.is_file(),
    }
}
/// Whether a path is a folder: inside a pack, the pack itself or a path
/// that some file lies under.
pub fn is_dir(path: &Path) -> bool {
    match inside(path) {
        Some((pack, rest)) => {
            let prefix = crate::pack::key(&rest);
            prefix.is_empty()
                || pack
                    .files()
                    .iter()
                    .any(|(p, _)| crate::pack::key(p).starts_with(&format!("{prefix}/")))
        }
        None => path.is_dir(),
    }
}
/// What a folder holds, files and folders, as paths under it.
pub fn list(dir: &Path) -> Result<Vec<PathBuf>> {
    match inside(dir) {
        Some((pack, rest)) => {
            let prefix = crate::pack::key(&rest);
            let mut out: Vec<PathBuf> = Vec::new();
            for (p, _) in pack.files() {
                let parts: Vec<&str> = p.split('/').collect();
                let depth = if prefix.is_empty() {
                    0
                } else {
                    prefix.split('/').count()
                };
                let under = parts.len() > depth
                    && (depth == 0 || crate::pack::key(&parts[..depth].join("/")) == prefix);
                if under {
                    let child = dir.join(parts[depth]);
                    if !out.iter().any(|o| same(o, &child)) {
                        out.push(child);
                    }
                }
            }
            if out.is_empty() && !prefix.is_empty() {
                return Err(invalid(format!("Cartella non trovata: {}", dir.display())));
            }
            Ok(out)
        }
        None => Ok(std::fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect()),
    }
}
/// The entries of an archive of a mounted pack.
pub fn archive(path: &Path) -> Option<(Arc<Pack>, crate::pack::Entries)> {
    let (pack, rest) = inside(path)?;
    match pack.node(&rest)? {
        Node::Archive(entries) => {
            let entries = entries.clone();
            Some((pack, entries))
        }
        _ => None,
    }
}
/// A container of a mounted pack.
pub fn container(path: &Path) -> Option<(Arc<Pack>, Folder)> {
    let (pack, rest) = inside(path)?;
    match pack.node(&rest)? {
        Node::Container(folder) => {
            let folder = folder.clone();
            Some((pack, folder))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::PackWriter;

    #[test]
    fn a_mounted_pack_stands_for_its_folders() {
        let dir = std::env::temp_dir().join(format!("retzar-vfs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Prova.dat");
        let mut w = PackWriter::create(&path).unwrap();
        let ini = w.add("Tzar.ini", b"[SOUND]\n").unwrap();
        let map = w.add("x", b"mappa").unwrap();
        w.file("Tzar.ini", Node::File(ini));
        w.file("Maps/MP1.wmp", Node::File(map));
        w.file("Maps/MP2.wmp", Node::File(map));
        w.file("Campaign/Uno.wcm", Node::File(map));
        w.finish().unwrap();
        mount(&path).unwrap();
        // Another spelling of the same path.
        let root = PathBuf::from(path.to_string_lossy().to_ascii_uppercase());
        assert_eq!(read(&root.join("tzar.INI")).unwrap(), b"[SOUND]\n");
        assert!(is_dir(&root) && is_dir(&root.join("maps")) && !is_dir(&root.join("Avance")));
        assert!(is_file(&root.join("Maps/mp1.wmp")) && !is_file(&root.join("Maps")));
        let mut maps = list(&root.join("Maps")).unwrap();
        maps.sort();
        assert_eq!(
            maps,
            vec![
                root.join("Maps").join("MP1.wmp"),
                root.join("Maps").join("MP2.wmp")
            ]
        );
        let top = list(&root).unwrap();
        assert_eq!(top.len(), 3, "{top:?}");
        assert!(read(&root.join("Nessuno.txt")).is_err());
        assert!(list(&root.join("Avance")).is_err());
        // Paths outside stay the disk's.
        std::fs::write(dir.join("fuori.txt"), b"disco").unwrap();
        assert_eq!(read(&dir.join("fuori.txt")).unwrap(), b"disco");
        unmount(&path);
        assert!(!is_dir(&root));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
