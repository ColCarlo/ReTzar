//! Runtime locations. Nothing depends on the folder the program was built in,
//! so the project and the installation can be moved or renamed freely.
use crate::{Result, invalid};
use std::path::{Path, PathBuf};

/// Where the prototype keeps settings, slots and reports (`root`) and where
/// the original installation is read from (`source`).
#[derive(Clone, Debug)]
pub struct Locations {
    pub root: PathBuf,
    pub source: PathBuf,
}

fn argument(args: &[String], key: &str) -> Option<PathBuf> {
    args.windows(2)
        .find(|a| a[0] == key)
        .map(|a| PathBuf::from(&a[1]))
}

/// A folder is the prototype's home when it contains this project's manifest
/// (development layout, executable in `target/<profile>`), or when it is the
/// executable's own folder (distribution layout).
fn project_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().take(5).find_map(|dir| {
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
        manifest
            .contains("name = \"retzar\"")
            .then(|| dir.to_path_buf())
    })
}

/// The original game is recognized by its map folder and its data, on disk
/// or in a mounted pack: the archives of `Packs`, or the loose `DATA` and
/// `IMAGES` of the digital releases.
pub fn is_installation(dir: &Path) -> bool {
    let has = |name: &str| crate::vfs::is_dir(&dir.join(name));
    has("Maps") && (has("Packs") || has("DATA") && has("IMAGES"))
}

impl Locations {
    /// Priority: command line, the portable copy's own files, environment,
    /// then the folders around the executable. `--root` / `RETZAR_HOME` and
    /// `--source` / `TZAR_DATA_ROOT`. The portable copy (`release`) is the
    /// executable with its pack, `ReTzar.dat`, beside it: the pack stands
    /// for the installation ([`crate::vfs`]) and settings and saves stay
    /// next to the executable, wherever the copy is moved. A source that is
    /// a file is such a pack.
    pub fn resolve(args: &[String]) -> Result<Self> {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let found = Self::resolve_with(args, |key| std::env::var_os(key), &exe_dir)?;
        if found.source.is_file() {
            crate::vfs::mount(&found.source)?;
        }
        Ok(found)
    }
    fn resolve_with(
        args: &[String],
        env: impl Fn(&str) -> Option<std::ffi::OsString>,
        exe_dir: &Path,
    ) -> Result<Self> {
        let portable = Some(exe_dir.join(crate::pack::FILE)).filter(|p| p.is_file());
        let root = Self::root_with(args, &env, exe_dir);
        let source = match argument(args, "--source")
            .or(portable)
            .or_else(|| env("TZAR_DATA_ROOT").map(PathBuf::from))
            .or_else(|| {
                // macOS uses the original files directly, rather than building
                // a portable pack as part of first-run setup.
                #[cfg(target_os = "macos")]
                {
                    crate::original::saved(&root).filter(|p| is_installation(p))
                }
                #[cfg(not(target_os = "macos"))]
                {
                    None
                }
            }) {
            Some(source) => source,
            None => root
                .ancestors()
                .chain(exe_dir.ancestors())
                .find(|dir| is_installation(dir))
                .map(Path::to_path_buf)
                .ok_or_else(|| {
                    invalid(crate::lang::fill(
                        "Installazione di Tzar non trovata vicino a {}. Usa --source CARTELLA.",
                        &[&root.display()],
                    ))
                })?,
        };
        Ok(Self { root, source })
    }
    pub(crate) fn root_with(
        args: &[String],
        env: &impl Fn(&str) -> Option<std::ffi::OsString>,
        exe_dir: &Path,
    ) -> PathBuf {
        argument(args, "--root")
            .or_else(|| {
                exe_dir
                    .join(crate::pack::FILE)
                    .is_file()
                    .then(|| exe_dir.to_path_buf())
            })
            .or_else(|| env("RETZAR_HOME").map(PathBuf::from))
            .or_else(|| project_root(exe_dir))
            .unwrap_or_else(|| exe_dir.to_path_buf())
    }
    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn reports(&self) -> PathBuf {
        self.root.join("reports")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_remembers_the_selected_installation_and_honors_overrides() {
        let base = std::env::temp_dir().join(format!("retzar-mac-source-{}", std::process::id()));
        let root = base.join("settings");
        let source = base.join("Tzar GOG");
        let exe = base.join("bin");
        for dir in [
            &root,
            &exe,
            &source.join("Maps"),
            &source.join("DATA"),
            &source.join("IMAGES"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        crate::original::remember(&root, &source).unwrap();
        let args = vec!["--root".into(), root.to_string_lossy().into_owned()];
        let found = Locations::resolve_with(&args, |_| None, &exe).unwrap();
        assert_eq!(found.root, root);
        assert_eq!(found.source, source);
        // Explicit sources take priority over the remembered folder.
        let from_env = Locations::resolve_with(
            &args,
            |key| (key == "TZAR_DATA_ROOT").then(|| "other-source".into()),
            &exe,
        )
        .unwrap();
        assert_eq!(from_env.source, PathBuf::from("other-source"));
        let mut explicit = args.clone();
        explicit.extend(["--source".into(), "explicit-source".into()]);
        assert_eq!(
            Locations::resolve_with(&explicit, |_| Some("env-source".into()), &exe)
                .unwrap()
                .source,
            PathBuf::from("explicit-source")
        );
        // A moved/deleted installation must prompt again, not launch a stale path.
        std::fs::remove_dir_all(&source).unwrap();
        assert!(Locations::resolve_with(&args, |_| None, &exe).is_err());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn locations_follow_the_folders_not_the_build_machine() {
        let base = std::env::temp_dir().join(format!("retzar-paths-{}", std::process::id()));
        let project = base.join("Giochi").join("ReTzar");
        std::fs::create_dir_all(project.join("target/debug")).unwrap();
        std::fs::create_dir_all(base.join("Giochi/Packs")).unwrap();
        std::fs::create_dir_all(base.join("Giochi/Maps")).unwrap();
        std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"retzar\"\n").unwrap();
        let exe = project.join("target/debug");
        assert_eq!(project_root(&exe), Some(project.clone()));
        let no_env = |_: &str| None;
        // Executable inside the project: both folders are found from it.
        let found = Locations::resolve_with(&[], no_env, &exe).unwrap();
        assert_eq!(found.root, project);
        assert_eq!(found.source, base.join("Giochi"));
        // Distribution layout: no manifest, the executable's folder is the root.
        let loose = base.join("Giochi/ReTzar-1.0.0");
        std::fs::create_dir_all(&loose).unwrap();
        let found = Locations::resolve_with(&[], no_env, &loose).unwrap();
        assert_eq!((found.root, found.source), (loose, base.join("Giochi")));
        let explicit: Vec<String> = ["--root", project.to_str().unwrap(), "--source", "X:/Tzar"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            Locations::resolve_with(&explicit, no_env, &exe)
                .unwrap()
                .source,
            PathBuf::from("X:/Tzar")
        );
        let from_env = Locations::resolve_with(
            &[],
            |k| (k == "TZAR_DATA_ROOT").then(|| "Y:/Tzar".into()),
            &exe,
        )
        .unwrap();
        assert_eq!(from_env.source, PathBuf::from("Y:/Tzar"));
        // Portable copy, even inside the project and with the environment
        // set: its own pack and its own settings, only the command line wins.
        let copy = project.join("release");
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::write(copy.join(crate::pack::FILE), b"").unwrap();
        let everywhere = |k: &str| Some(format!("Z:/{k}").into());
        let found = Locations::resolve_with(&[], everywhere, &copy).unwrap();
        assert_eq!(
            (found.root, found.source),
            (copy.clone(), copy.join(crate::pack::FILE))
        );
        assert_eq!(
            Locations::resolve_with(&explicit, everywhere, &copy)
                .unwrap()
                .source,
            PathBuf::from("X:/Tzar")
        );
        let nowhere = base.join("Altro/Vuoto");
        std::fs::create_dir_all(&nowhere).unwrap();
        assert!(!is_installation(&base));
        assert!(Locations::resolve_with(&[], no_env, &nowhere).is_err());
        std::fs::remove_dir_all(base).unwrap();
    }
}
