//! The first start of an executable without its data (`ReTzar.dat`)
//! beside it, as built from the sources: ReTzar asks for the player's Tzar
//! folder, of any edition, checks that it holds the original game and makes
//! its data pack from it, once, with that edition's content. The release
//! ships the whole pack instead ([`original::require`]).
use crate::{Result, invalid, lang, original, pack, packing, paths, picker};
use std::path::{Path, PathBuf};

/// Whether this start is the first of a portable copy: no game given on
/// the command line or in the environment, and no pack beside the
/// executable.
pub fn applies(args: &[String], exe_dir: &Path) -> bool {
    !args.iter().any(|a| a == "--source")
        && std::env::var_os("TZAR_DATA_ROOT").is_none()
        && !exe_dir.join(pack::FILE).exists()
}

/// The installation a chosen folder points to: the folder itself, one of
/// its own (a library of games), or the one above (its `Packs`).
fn installation_at(chosen: &Path) -> Option<PathBuf> {
    if paths::is_installation(chosen) {
        return Some(chosen.to_path_buf());
    }
    let mut inside: Vec<PathBuf> = std::fs::read_dir(chosen)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| paths::is_installation(p))
        .collect();
    inside.sort();
    inside.into_iter().next().or_else(|| {
        chosen
            .parent()
            .filter(|p| paths::is_installation(p))
            .map(Path::to_path_buf)
    })
}

/// Asks for the Tzar folder and makes `ReTzar.dat` beside the executable
/// in `exe_dir`, where the settings also go; the folder is kept as the
/// original copy the game plays beside ([`original::require`]).
pub fn prepare(exe_dir: &Path) -> Result<()> {
    original::speak(exe_dir);
    if !picker::welcome(lang::t(
        "Benvenuto in ReTzar! Il gioco usa la tua copia di Tzar (GOG, Steam o CD): scegli la cartella in cui è installato e ReTzar ne ricaverà i suoi dati. Serve solo la prima volta.",
    )) {
        return Err(original::refused());
    }
    // The pack and the settings go beside the executable.
    let probe = exe_dir.join("ReTzar.prova");
    if std::fs::write(&probe, b"").is_err() {
        return Err(invalid(lang::fill(
            "ReTzar non può scrivere nella sua cartella ({}): spostalo in una cartella tua, per esempio sul Desktop o in Documenti.",
            &[&exe_dir.display()],
        )));
    }
    let _ = std::fs::remove_file(&probe);
    let mut start = None;
    let installation = loop {
        let chosen = picker::folder(
            lang::t("Scegli la cartella in cui è installato Tzar"),
            start.as_deref(),
        )
        .ok_or_else(original::refused)?;
        if let Some(found) = installation_at(&chosen).filter(|i| original::holds(i)) {
            break found;
        }
        if !picker::confirm(&lang::fill(
            "In {} non c'è una copia originale di Tzar.\n\nScegli la cartella in cui è installato il gioco.",
            &[&chosen.display()],
        )) {
            return Err(original::refused());
        }
        start = Some(chosen);
    };
    let american = Some(installation.join("USA/CD/SETUP/Tzar")).filter(|p| p.is_dir());
    let work = std::env::temp_dir().join(format!("retzar-filmati-{}", std::process::id()));
    let progress = picker::Progress::start(
        "ReTzar",
        lang::t("ReTzar sta creando i dati del gioco dalla tua copia di Tzar…"),
    );
    let made = packing::build(
        &installation,
        american.as_deref(),
        None,
        &exe_dir.join(pack::FILE),
        &work,
        &mut |step| {
            // What is being packed: a file's name, or the final check.
            let detail = ["Archivio ", "Contenitore ", "Filmato "]
                .iter()
                .find_map(|p| step.strip_prefix(p))
                .map(str::to_owned)
                .or_else(|| {
                    step.starts_with("Verifica")
                        .then(|| lang::t("Controllo dei dati…").to_owned())
                });
            if let Some(detail) = detail {
                progress.detail(&detail);
            }
        },
    );
    drop(progress);
    made.map_err(|e| {
        invalid(lang::fill(
            "Non è stato possibile creare i dati del gioco: {}",
            &[&e],
        ))
    })?;
    original::remember(exe_dir, &installation)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The installation is found from the folder itself, from the one that
    /// holds it, or from its `Packs`.
    #[test]
    fn a_chosen_folder_points_to_its_installation() {
        let base = std::env::temp_dir().join(format!("retzar-primo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let game = base.join("Giochi").join("Tzar");
        for dir in ["Maps", "Packs"] {
            std::fs::create_dir_all(game.join(dir)).unwrap();
        }
        assert_eq!(installation_at(&game), Some(game.clone()));
        assert_eq!(installation_at(&base.join("Giochi")), Some(game.clone()));
        assert_eq!(installation_at(&game.join("Packs")), Some(game.clone()));
        assert_eq!(installation_at(&base), None);
        // A digital release keeps its data loose.
        let loose = base.join("GOG");
        for dir in ["Maps", "DATA", "IMAGES"] {
            std::fs::create_dir_all(loose.join(dir)).unwrap();
        }
        assert_eq!(installation_at(&loose), Some(loose.clone()));
        let mut args = vec!["--source".to_owned(), "x".to_owned()];
        assert!(!applies(&args, &base));
        args.clear();
        if std::env::var_os("TZAR_DATA_ROOT").is_none() {
            assert!(applies(&args, &base));
            std::fs::write(base.join(pack::FILE), b"").unwrap();
            assert!(!applies(&args, &base));
        }
        std::fs::remove_dir_all(&base).unwrap();
    }
}
