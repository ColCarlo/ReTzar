//! Every sound and picture the original data names, looked up as the game
//! looks it up: a wave of `Packs/sounds.wdt` that decodes, a sheet of the
//! archives that decodes with the grid its section declares.
use retzar::{
    archive::{Archive, key},
    assets::{GameAssets, Ini},
    audio::{Catalog, Habitat, NatureContext, Pcm},
    bitmap::Bitmap,
    sprite::SpriteSheet,
    world::{CLASSES, MapObject, ObjectLibrary},
};
use std::{collections::BTreeSet, path::Path};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}
fn object(class: usize, civilization: u8) -> MapObject {
    let mut data = vec![0; 128];
    data[0x1a] = 255;
    MapObject {
        id: 1,
        class,
        flags: 0,
        x: 32,
        y: 32,
        owner: 1,
        civilization,
        variant: 0,
        health: 100,
        max_health: 100,
        name: String::new(),
        subtitle: String::new(),
        portrait: String::new(),
        portrait_large: String::new(),
        custom_image: String::new(),
        data,
        mole: 0,
        control: [0; 8],
        sight: [0; 8],
    }
}
/// The sounds of a class record (0x4e2163, 0x4535e0, 0x4df6e0, …); a
/// plain `sound` (the Arab mage's) is read by nothing.
const SOUND_KEYS: [&str; 5] = [
    "sound_create",
    "sound_select",
    "sound_command",
    "sound_attack",
    "sound_die",
];
/// The groups of SOUNDS.INI the engine names itself. The others are named
/// by the classes; the bear, catapult and galleon groups by nobody, and
/// their files are not in the archive.
const ENGINE_GROUPS: [&str; 6] = ["build", "chop", "dig", "fire", "mine", "summon_spell"];
/// The pictures a class record reads (0x4950d2–0x495267).
const IMAGE_KEYS: [&str; 17] = [
    "image",
    "image_idle",
    "image_idle2",
    "image_attack",
    "image_attack2",
    "image_spell",
    "image_capture",
    "image_build",
    "image_dig",
    "image_chop",
    "image_die",
    "image_murder",
    "image_mine",
    "image_milk",
    "image_carrymilk",
    "image_carrymeat",
    "image_c",
];
/// The pictures the engine names itself: missiles, trails, sparks and the
/// spells' effects, ruins and the weather (0x4ea280, 0x4ea4c0, 0x432dd9,
/// 0x473d80, 0x4481ac, 0x4a6db0, …).
const EFFECTS: [&str; 62] = [
    "arrow",
    "arrow_big",
    "arrow_ballista",
    "arrow_ballista_shadow",
    "arrow_rocket",
    "cb_fire",
    "cb_metal",
    "cb_metal_shadow",
    "cb_stone",
    "trail",
    "rocket_trail",
    "freeze_ball_trail",
    "flame_ball_trail",
    "flame_bolt",
    "freeze_bolt",
    "exorcism",
    "fire_rain",
    "fire_small",
    "fire_big",
    "dragon_fire",
    "explosion",
    "explosion_small",
    "expl64",
    "expl_small_hole",
    "expl_small_water",
    "flame_bolt_expl",
    "freeze_bolt_expl",
    "convert_exp",
    "magic_star",
    "turn_to_stone",
    "whirlpool",
    "bless",
    "curse",
    "healing",
    "hypnotise",
    "bribe",
    "impersonate",
    "spawn",
    "skeleton_rise",
    "pump",
    "a_ninja_jump",
    "cow_down",
    "cow_eat",
    "cow_idle",
    "field_grow",
    "debris",
    "debris3x3",
    "walls_stone_debris",
    "walls_wood_debris",
    "bridge_horz_demol",
    "bridge_vert_demol",
    "dummy1_demol",
    "dummy2_demol",
    "dummy1_build",
    "dummy2_build",
    "item_flag",
    "flag_capture",
    "raindrops",
    "rainfalls",
    "net1",
    "net2",
    "net3",
];

/// Every wave a list names, or why it cannot be played.
fn check_waves<'a>(
    archive: &Archive,
    names: impl IntoIterator<Item = (String, &'a str)>,
    bad: &mut Vec<String>,
    heard: &mut BTreeSet<String>,
) {
    for (name, why) in names {
        let k = key(&name);
        if !heard.insert(k.clone()) {
            continue;
        }
        match archive.get(&k).map(|d| Pcm::decode(&d)) {
            Ok(Ok(p)) if p.seconds() > 0. => {}
            Ok(Ok(_)) => bad.push(format!("{why}: {name} è vuoto")),
            Ok(Err(e)) => bad.push(format!("{why}: {name} non si decodifica ({e})")),
            Err(_) => bad.push(format!("{why}: {name} manca")),
        }
    }
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn every_sound_the_data_names_is_a_wave_that_plays() {
    let assets = GameAssets::open(root()).unwrap();
    let lib = ObjectLibrary::load(&assets, "grass").unwrap();
    let archive = Archive::open(&root().join("Packs/sounds.wdt")).unwrap();
    let mut bad = Vec::new();
    let mut heard = BTreeSet::new();
    // The voices and noises of every class, for each civilization.
    for (class, name) in CLASSES.iter().enumerate() {
        for civ in 1..=3 {
            let o = object(class, civ);
            for property in SOUND_KEYS {
                let Some(value) = lib.property(&o, property).map(str::trim) else {
                    continue;
                };
                if value.is_empty() {
                    continue;
                }
                let sounds = lib.sounds_for(&o, property);
                if sounds.is_empty() {
                    bad.push(format!("{name}/{civ} {property} = {value}: nessun suono"));
                }
                let why = format!("{name} {property}");
                let names: Vec<_> = sounds.into_iter().map(|(n, _)| (n, why.as_str())).collect();
                check_waves(&archive, names, &mut bad, &mut heard);
            }
        }
    }
    // The groups the engine plays by name, and its own waves: notices,
    // spells, gambling (0x4934f6, 0x450f25, …).
    for group in ENGINE_GROUPS.iter().chain(&[
        "notify/attack.wav",
        "notify/bldgdone.wav",
        "notify/unitdone.wav",
        "notify/techdone.wav",
        "notify/upgrdone.wav",
        "gamble/win.wav",
        "gamble/lose.wav",
    ]) {
        let sounds = lib.sound_group(group);
        if sounds.is_empty() {
            bad.push(format!("[{group}]: nessun suono"));
        }
        let names: Vec<_> = sounds.into_iter().map(|(n, _)| (n, *group)).collect();
        check_waves(&archive, names, &mut bad, &mut heard);
    }
    // The nature calls of the four biomes, by day and by night.
    let nature = Catalog::load(&assets).unwrap();
    for biome in ["grass", "summer", "snow", "desert"] {
        for habitat in [Habitat::Marine, Habitat::Forest, Habitat::Plains] {
            for night in [false, true] {
                let clips = nature.clips(biome, NatureContext { habitat, night });
                let names: Vec<_> = clips.iter().map(|c| (c.clone(), biome)).collect();
                check_waves(&archive, names, &mut bad, &mut heard);
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    // What no list names: printed for the record (stories, menus, the
    // engine's own notices and the taunts of network games).
    let unnamed: Vec<_> = archive
        .entries
        .iter()
        .map(|e| key(&e.name))
        .filter(|k| k.ends_with(".WAV") && !heard.contains(k))
        .collect();
    println!("{} onde nominate, {} no:", heard.len(), unnamed.len());
    for k in unnamed {
        println!("  {k}");
    }
}

/// A section's sheet as the game finds it (the BMP, or the RLE the archive
/// keeps in its place) and whether it holds the grid the section declares.
fn sheet(assets: &GameAssets, images: &Ini, section: &str) -> Result<(), String> {
    let file = images
        .get(section, "image")
        .ok_or("nessun «image»")?
        .replace('\\', "/");
    let file = format!("IMAGES/{file}");
    let rle = format!("{}.RLE", &file[..file.len().saturating_sub(4)]);
    let grid = |name| {
        images
            .get(section, name)
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(1)
    };
    let (cols, rows) = (grid("cols"), grid("rows"));
    if assets.contains(&file) {
        let data = assets.get(&file).map_err(|e| e.to_string())?;
        if data.starts_with(b"BM") {
            let b = Bitmap::decode(&data, true).map_err(|e| e.to_string())?;
            if cols == 0 || rows == 0 || cols > b.width as usize || rows > b.height as usize {
                return Err(format!(
                    "{file}: {}×{} per {cols}×{rows}",
                    b.width, b.height
                ));
            }
            return Ok(());
        }
    }
    let name = if assets.contains(&rle) { &rle } else { &file };
    let data = assets
        .get(name)
        .map_err(|_| format!("{file} manca (né {rle})"))?;
    let s = SpriteSheet::decode(&data).map_err(|e| format!("{name}: {e}"))?;
    if s.columns != cols || s.rows < rows {
        return Err(format!(
            "{name}: {}×{} contro {cols}×{rows} della sezione",
            s.columns, s.rows
        ));
    }
    Ok(())
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn every_picture_the_classes_and_the_engine_name_is_a_sheet() {
    let assets = GameAssets::open(root()).unwrap();
    let mut bad = Vec::new();
    let mut seen = BTreeSet::new();
    for biome in ["grass", "summer", "snow", "desert"] {
        let lib = ObjectLibrary::load(&assets, biome).unwrap();
        let mut sections: Vec<(String, String)> = Vec::new();
        for (class, name) in CLASSES.iter().enumerate() {
            for civ in 1..=3 {
                let o = object(class, civ);
                for property in IMAGE_KEYS {
                    if let Some(s) = lib.property(&o, property).map(str::trim)
                        && !s.is_empty()
                    {
                        sections.push((s.to_owned(), format!("{name} {property}")));
                    }
                }
            }
        }
        sections.extend(EFFECTS.iter().map(|s| (s.to_string(), "motore".into())));
        for (section, why) in sections {
            if !seen.insert((biome, section.to_ascii_lowercase())) {
                continue;
            }
            if lib.images.get(&section, "image").is_none() {
                bad.push(format!("{biome} {why}: [{section}] assente"));
                continue;
            }
            if let Err(e) = sheet(&assets, &lib.images, &section) {
                bad.push(format!("{biome} {why}: [{section}] {e}"));
            }
            // The shadow drawn under it, when the data has one (0x41d8f0).
            let shadow = format!("{section}_s");
            if lib.images.get(&shadow, "image").is_some()
                && let Err(e) = sheet(&assets, &lib.images, &shadow)
            {
                bad.push(format!("{biome} {why}: [{shadow}] {e}"));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn walls_bridges_gates_and_dummies_show_their_own_sites() {
    use retzar::world::{FLAG_CONSTRUCTION, class};
    let assets = GameAssets::open(root()).unwrap();
    let lib = ObjectLibrary::load(&assets, "grass").unwrap();
    let site = |class: usize, edit: &dyn Fn(&mut MapObject)| {
        let mut o = object(class, 1);
        o.flags |= FLAG_CONSTRUCTION;
        edit(&mut o);
        lib.image_for(&o).unwrap()
    };
    // 0x4fbce0, 0x41c800, 0x456e91, 0x4481a0.
    let mut cases = vec![
        (
            site(class::WALL_WOOD, &|_| {}),
            "walls_wood_build".to_owned(),
        ),
        (
            site(class::WALL_STONE, &|_| {}),
            "walls_stone_build".to_owned(),
        ),
        (
            site(class::BRIDGE, &|o| o.data[56] = 0),
            "bridge_horz_build".to_owned(),
        ),
        (
            site(class::BRIDGE, &|o| o.data[56] = 1),
            "bridge_vert_build".to_owned(),
        ),
        (
            site(class::DUMMY, &|o| o.variant = 1),
            "dummy1_build".to_owned(),
        ),
        (
            site(class::DUMMY, &|o| o.variant = 3),
            "dummy2_build".to_owned(),
        ),
    ];
    for way in 0..4u8 {
        for (c, letter) in [(class::GATE_WOOD, 'w'), (class::GATE_STONE, 's')] {
            cases.push((
                site(c, &|o| o.data[0x38] = way),
                format!("g{letter}{way}_build"),
            ));
        }
    }
    for (got, want) in cases {
        assert_eq!(got, want);
        sheet(&assets, &lib.images, &want).unwrap();
    }
    // Drawn as sites, a row of their sheet by their progress.
    let objects: Vec<MapObject> = [
        class::WALL_WOOD,
        class::WALL_STONE,
        class::BRIDGE,
        class::DUMMY,
        class::GATE_WOOD,
        class::GATE_STONE,
    ]
    .into_iter()
    .enumerate()
    .map(|(k, c)| {
        let mut o = object(c, 1);
        o.id = k as u16 + 1;
        o.x = 8 + 12 * k as u16;
        o.flags |= FLAG_CONSTRUCTION;
        o.health = 50;
        o
    })
    .collect();
    let m = retzar::world::MissionWorld {
        objects,
        ..Default::default()
    };
    let drawn = lib.build(&assets, &m, 96).unwrap();
    assert!(drawn.missing.is_empty(), "{:?}", drawn.missing);
    for k in 0..m.objects.len() {
        assert!(drawn.visuals.iter().any(|v| v.object == k), "{k}");
    }
}
