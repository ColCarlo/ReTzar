//! The English language (an addition of this version). The language is one
//! for the whole program, so every test of this file runs in English.
use retzar::{
    assets::{GameAssets, Ini},
    lang::{self, Language},
    scenario::ScenarioCatalog,
    world::MissionWorld,
};
use std::path::PathBuf;

fn source() -> PathBuf {
    std::env::var_os("TZAR_DATA_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .to_path_buf()
        })
}
fn english() {
    lang::set(Language::English);
}

#[test]
fn every_overlay_file_has_an_original_to_lay_over() {
    // Without the installation the names are still checked for shape.
    for name in lang::overlay_names(Language::English) {
        assert_eq!(name, retzar::archive::key(name), "{name}");
    }
    let Ok(assets) = GameAssets::open(&source()) else {
        return;
    };
    for name in lang::overlay_names(Language::English) {
        assert!(assets.contains(name), "{name} non è negli archivi");
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_interface_reads_the_american_release_texts() {
    english();
    let assets = GameAssets::open_in(&source(), Language::English).unwrap();
    let locale = Ini::locale(&assets).unwrap();
    assert_eq!(locale.label("options", "ok"), "OK");
    assert_eq!(locale.label("endgamedlg", "win2"), "VICTORIOUS");
    // The Italian edition's own names, merged by key.
    assert_eq!(
        locale.label("orders_names", "e_tower_stone2"),
        "Build Cannon Tower"
    );
    let grass = Ini::parse(&assets.get("IMAGES/TERRAIN/GRASS.TERRAIN.INI").unwrap());
    assert_eq!(grass.get("terrain", "name"), Some("Grassland"));
    // The rest of a merged file stays the Italian edition's.
    let italian = GameAssets::open(&source()).unwrap();
    let original = Ini::parse(&italian.get("IMAGES/TERRAIN/GRASS.TERRAIN.INI").unwrap());
    assert_eq!(grass.get("terrain", "dir"), original.get("terrain", "dir"));
    let help = Ini::parse(&assets.get("HELP/ITEMS.TXT").unwrap());
    let original_help = Ini::parse(&italian.get("HELP/ITEMS.TXT").unwrap());
    assert_eq!(help.get("U_E_PEASANT", "brief"), Some("Peasant"));
    assert_eq!(
        help.get("U_E_PEASANT", "icon"),
        original_help.get("U_E_PEASANT", "icon")
    );
    // The Italian edition's screens give way to the American ones.
    assert_ne!(
        assets.get("IMAGES/SCREENS/TITLE.BMP").unwrap(),
        italian.get("IMAGES/SCREENS/TITLE.BMP").unwrap()
    );
}

#[test]
fn own_texts_follow_the_language() {
    english();
    assert_eq!(lang::t("Lingua"), "Language");
    assert_eq!(lang::fill("Mappa casuale {}", &[&60]), "Random map 60");
    // A text without a translation stays as written.
    assert_eq!(lang::t("Testo senza traduzione"), "Testo senza traduzione");
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_missions_read_in_english_and_back() {
    english();
    let catalog = ScenarioCatalog::load(&source()).unwrap();
    let title = |id: &str| {
        let (c, m) = catalog.locate(id).unwrap();
        match c {
            Some(c) => catalog.campaigns[c].missions[m].title.clone(),
            None => catalog.missions[m].title.clone(),
        }
    };
    // The American release's own texts for the maps and the tutorial.
    assert_eq!(title("mp4.wmp::maps/mp4"), "Bridge too close");
    assert_eq!(title("tzartutorial.wcm::maps/intro"), "Introduction");
    // The campaigns of the Italian edition, translated.
    assert_eq!(
        title("excalibur e il re artu.wcm::maps/1_a merlín y el rey arturo"),
        "Merlin and King Arthur"
    );
    assert_eq!(
        title("impero di gengis khan.wcm::maps/gengis_khan02"),
        "The Conquest of Peking"
    );
    let scenario = {
        let (c, m) = catalog
            .locate("impero di gengis khan.wcm::maps/gengis_khan01")
            .unwrap();
        catalog.campaigns[c.unwrap()].missions[m].clone()
    };
    let mut world =
        MissionWorld::read(&scenario.container().unwrap(), &scenario.grid().unwrap()).unwrap();
    assert!(world.kingdoms.iter().any(|k| k.name == "Tatars"));
    assert!(
        world
            .conversations
            .iter()
            .flat_map(|c| &c.phrases)
            .any(|p| p.text == "Run, my children!")
    );
    // A game saved in English resumes in Italian.
    lang::localize_mission(&mut world, Language::Italian);
    assert!(world.kingdoms.iter().any(|k| k.name == "Tartari"));
    assert!(
        world
            .conversations
            .iter()
            .flat_map(|c| &c.phrases)
            .any(|p| p.text == "Correte figli miei!")
    );
}

#[test]
#[ignore = "Requires installed original archives and the American CD in USA"]
fn the_american_campaign_reads_its_own_english() {
    use retzar::script::Action;
    english();
    if retzar::scenario::american_campaign(&source()).is_none() {
        return;
    }
    let catalog = ScenarioCatalog::load(&source()).unwrap();
    let (c, m) = catalog.locate("tzar.wcm::maps/v_00").unwrap();
    let campaign = &catalog.campaigns[c.unwrap()];
    assert_eq!(campaign.title, "TZAR: The Burden Of The Crown");
    let scenario = &campaign.missions[m];
    assert_eq!(scenario.title, "The Beginning");
    let mut world =
        MissionWorld::read(&scenario.container().unwrap(), &scenario.grid().unwrap()).unwrap();
    let story = |world: &MissionWorld, start: &str| {
        world
            .script
            .actions
            .iter()
            .any(|a| matches!(&a.value, Action::Story { text, .. } if text.starts_with(start)))
    };
    assert!(story(&world, "Evil has always existed."));
    assert!(world.objects.iter().any(|o| o.name == "Uncle Brian"));
    // A game saved in English resumes in Italian.
    lang::localize_mission(&mut world, Language::Italian);
    assert!(story(&world, "Il Male è sempre esistito."));
    assert!(world.objects.iter().any(|o| o.name == "Zio Brian"));
}

#[test]
#[ignore = "Requires installed original archives (and the American CD in USA)"]
fn each_installation_is_told_by_its_own_texts() {
    let italian = GameAssets::open(&source()).unwrap();
    assert_eq!(italian.native(), Some(Language::Italian));
    assert_eq!(lang::installation(), Some(Language::Italian));
    // The American release read as an installation of its own.
    let american = source().join("USA/CD/SETUP/Tzar");
    if american.join("Packs").is_dir() {
        let english = GameAssets::open_in(&american, Language::English).unwrap();
        assert_eq!(english.native(), Some(Language::English));
        assert_eq!(lang::installation(), Some(Language::English));
        // Its own texts, not those this version lays over the Italian.
        let own = english.original("LOCAL/STRINGS.INI").unwrap();
        assert_eq!(english.get("LOCAL/STRINGS.INI").unwrap(), own);
        // Out of its language the translations of this version.
        let french = GameAssets::open_in(&american, Language::French).unwrap();
        assert_ne!(french.get("LOCAL/STRINGS.INI").unwrap(), own);
        digital_release(&american);
        GameAssets::open(&source()).unwrap();
    }
}

/// A release without archives, as GOG's: the American data as loose
/// folders beside `Maps`, read as they are.
fn digital_release(american: &std::path::Path) {
    use retzar::archive::Archive;
    let game = std::env::temp_dir().join(format!("retzar-digital-{}", std::process::id()));
    std::fs::create_dir_all(game.join("Maps")).unwrap();
    std::fs::create_dir_all(game.join("IMAGES")).unwrap();
    let mut files = Vec::new();
    let mut waves = 0;
    for pack in ["data.wdt", "local.wdt", "help.wdt", "sounds.wdt"] {
        let archive = Archive::open(&american.join("Packs").join(pack)).unwrap();
        for e in &archive.entries {
            let name = e.name.replace('\\', "/");
            if files.iter().any(|(n, _)| *n == name.to_ascii_uppercase()) {
                continue;
            }
            // A few sounds are enough, with every song.
            if name.to_ascii_uppercase().starts_with("SOUND/") {
                waves += 1;
                if waves > 4 {
                    continue;
                }
            }
            let path = game.join(&name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let bytes = archive.read(e).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            files.push((name.to_ascii_uppercase(), bytes));
        }
    }
    assert!(retzar::paths::is_installation(&game));
    assert!(!retzar::paths::is_installation(&game.join("Maps")));
    let loose = GameAssets::open(&game).unwrap();
    assert_eq!(loose.native(), Some(Language::English));
    let sounds = retzar::assets::sounds(&game).unwrap();
    for (name, bytes) in &files {
        if name.starts_with("SOUND/") || name.starts_with("MUSIC/") {
            assert_eq!(&sounds.get(name).unwrap(), bytes, "{name}");
        } else {
            assert_eq!(&loose.original(name).unwrap(), bytes, "{name}");
        }
    }
    assert!(
        sounds
            .entries
            .iter()
            .any(|e| e.name.to_ascii_uppercase().ends_with(".MID"))
    );
    // A movie of the loose `MOVIES`, as the start plays them.
    let pan = Archive::open(&american.join("Packs/movies.wdt"))
        .unwrap()
        .get("MOVIES/PAN.DDV")
        .unwrap();
    std::fs::create_dir_all(game.join("MOVIES")).unwrap();
    std::fs::write(game.join("MOVIES/PAN.DDV"), &pan).unwrap();
    let movies = retzar::ddv::american_movies(&game).unwrap();
    assert_eq!(movies.get(retzar::ddv::START[1]).unwrap(), pan);
    std::fs::remove_dir_all(game).unwrap();
}
