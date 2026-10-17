//! Exhaustive checks of installed original artwork and class definitions.
//! These inspect presentation and imported attributes, without enabling RTS logic.
use retzar::{
    assets::{GameAssets, legacy_text},
    bitmap::Bitmap,
    simulation::ObjectState,
    sprite::SpriteSheet,
    world::{CLASSES, Layer, MapObject, MissionWorld, ObjectLibrary, civ_section, class},
};
use std::{collections::BTreeMap, path::Path};

fn assets() -> GameAssets {
    GameAssets::open(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap()
}
fn object(class: usize, civilization: u8, variant: usize) -> MapObject {
    let mut data = vec![0; 128];
    data[0x1a] = 255;
    MapObject {
        id: variant as u16 + 1,
        class,
        civilization,
        variant: variant as u16,
        owner: 1,
        x: 32,
        y: 32,
        flags: 0,
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
/// Evidence files only on request (`RETZAR_TEST_REPORTS=1`): a plain test run writes nothing.
fn report(name: &str, value: serde_json::Value) {
    if std::env::var_os("RETZAR_TEST_REPORTS").is_none() {
        return;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("reports/comprehensive-audit");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
fn word(d: &[u8], p: usize) -> usize {
    u16::from_le_bytes(d[p..p + 2].try_into().unwrap()) as usize
}
fn dword(d: &[u8], p: usize) -> usize {
    u32::from_le_bytes(d[p..p + 4].try_into().unwrap()) as usize
}

/// Independent scanline oracle: keep the original direction-major storage,
/// and compare each frame to the public decoder's row-major frame accessor.
fn original_rle(d: &[u8], slot: Option<usize>) -> (usize, usize, usize, usize, Vec<u8>) {
    let (cols, rows, width, height) = (dword(d, 4), dword(d, 8), word(d, 20), word(d, 22));
    let (format, colors) = (word(d, 24), dword(d, 30));
    let base = 38 + colors * 4;
    let mut rgba = vec![0; width * height * 4];
    for y in 0..height {
        let mut p = base + dword(d, base + y * 4);
        let mut x = 0;
        while x < width {
            x += d[p] as usize;
            let count = d[p + 1] as usize;
            p += 2;
            for _ in 0..count {
                let pixel = match format {
                    1 => {
                        let mut index = d[p] as usize;
                        p += 1;
                        if colors == 256
                            && index >= 240
                            && let Some(s) = slot
                        {
                            index = 112 + s * 16 + index - 240;
                        }
                        let at = 38 + index * 4;
                        [d[at + 2], d[at + 1], d[at], 255]
                    }
                    3 => {
                        let rgb = word(d, p);
                        p += 2;
                        let expand = |v: usize| ((v << 3) | (v >> 2)) as u8;
                        [
                            expand((rgb >> 10) & 31),
                            expand((rgb >> 5) & 31),
                            expand(rgb & 31),
                            255,
                        ]
                    }
                    7 => [0, 0, 0, 128],
                    _ => panic!("Unexpected format {format}"),
                };
                rgba[(y * width + x) * 4..(y * width + x + 1) * 4].copy_from_slice(&pixel);
                x += 1;
            }
        }
    }
    (cols, rows, width, height / (cols * rows), rgba)
}

#[test]
#[ignore = "Requires the installed original artwork"]
fn every_original_rle_frame_and_direction_matches_an_independent_pixel_oracle() {
    let a = assets();
    let mut entries = Vec::new();
    let mut frames = 0;
    for (i, name) in a.names().iter().filter(|n| n.ends_with(".RLE")).enumerate() {
        let d = a.get(name).unwrap();
        // All indexed sheets exercise recolouring; rotate through all eight slots.
        let slot = Some(i % 8);
        let decoded = SpriteSheet::decode_for(&d, slot).unwrap();
        let (cols, rows, width, height, raw) = original_rle(&d, slot);
        let len = width * height * 4;
        for c in 0..cols {
            for r in 0..rows {
                let start = (c * rows + r) * len;
                assert_eq!(
                    decoded.frame(c, r).unwrap().rgba,
                    raw[start..start + len],
                    "{name}: direction {c}, frame {r}"
                );
            }
        }
        frames += cols * rows;
        entries.push(serde_json::json!({"path":name,"directions":cols,"rows":rows,"frames":cols*rows,"color":slot}));
    }
    report(
        "rle-frames.json",
        serde_json::json!({"sheets":entries.len(),"frames":frames,"entries":entries}),
    );
    assert!(entries.len() >= 900 && frames >= 30000);
}

fn source_frame(
    a: &GameAssets,
    lib: &ObjectLibrary,
    section: &str,
    col: usize,
    row: usize,
) -> Bitmap {
    let file = format!(
        "IMAGES/{}",
        lib.images.get(section, "image").unwrap().replace('\\', "/")
    );
    if a.contains(&file) {
        let b = Bitmap::decode_for(&a.get(&file).unwrap(), true, Some(0)).unwrap();
        let n = |k| {
            lib.images
                .get(section, k)
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(1)
        };
        let (w, h) = (b.width / n("cols"), b.height / n("rows"));
        b.crop(col as u32 * w, row as u32 * h, w, h).unwrap()
    } else {
        let d = a.get(&format!("{}.RLE", &file[..file.len() - 4])).unwrap();
        let (_, rows, w, h, raw) = original_rle(&d, Some(0));
        let len = w * h * 4;
        let start = (col * rows + row) * len;
        Bitmap {
            width: w as u32,
            height: h as u32,
            rgba: raw[start..start + len].to_vec(),
        }
    }
}

#[test]
#[ignore = "Requires original class and image definitions"]
fn every_defined_unit_and_building_civilization_renders_all_idle_directions() {
    let a = assets();
    let lib = ObjectLibrary::load(&a, "grass").unwrap();
    let mut entries = Vec::new();
    for (class_index, name) in CLASSES.iter().enumerate() {
        if !(class::is_unit(class_index) || (39..=73).contains(&class_index)) {
            continue;
        }
        let Some(def) = lib.objects.get(*name) else {
            continue;
        };
        for civ in 1..=3 {
            let s = civ_section(civ);
            if !def.section_names().any(|x| x == s || x == "comm") {
                continue;
            }
            let first = object(class_index, civ, 0);
            let Some(section) = lib.image_for(&first) else {
                panic!("{name}/{s}: no picture")
            };
            // Gates have a dedicated compositor, exercised for both materials
            // and all orientations by starting_walls.rs.
            if [class::GATE_WOOD, class::GATE_STONE].contains(&class_index) {
                continue;
            }
            let cols = lib
                .images
                .get(&section, "cols")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1);
            let m = MissionWorld {
                objects: (0..cols).map(|c| object(class_index, civ, c)).collect(),
                ..Default::default()
            };
            let visuals = lib
                .build(&a, &m, 64)
                .unwrap_or_else(|e| panic!("{name}/{s}: {e}"));
            assert!(
                visuals.missing.is_empty(),
                "{name}/{s}: {:?}",
                visuals.missing
            );
            for (index, o) in m.objects.iter().enumerate() {
                let body = visuals
                    .visuals
                    .iter()
                    .find(|v| v.object == index && matches!(v.layer, Layer::Body | Layer::Air))
                    .unwrap();
                let startrow = if body.layer == Layer::Air {
                    0
                } else {
                    lib.images
                        .get(&section, "startrow")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0)
                };
                assert!(
                    visuals.images[body.image].rgba
                        == source_frame(&a, &lib, &section, index, startrow).rgba,
                    "{name}/{s}, direction {index}"
                );
                if let Some(id) = body.animation {
                    let anim = &visuals.animations[id];
                    // Independent integer oracle from native animation records:
                    // directional delays are padded to rounded anim_time;
                    // one-column sheets use a 1000/fps timer.
                    let count = anim.frames.len();
                    let mut delays: Vec<u64> = if cols > 1 {
                        let total = lib
                            .images
                            .get(&section, "anim_time")
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(1000)
                            / count as u64
                            * count as u64;
                        let mut ds: Vec<u64> = lib
                            .images
                            .get(&section, "delay")
                            .unwrap_or("")
                            .split(',')
                            .filter_map(|v| v.trim().parse().ok())
                            .collect();
                        if ds.is_empty() {
                            ds = vec![total / count as u64; count];
                        }
                        assert_eq!(ds.len(), count, "{section}: native delay count");
                        let padding = total.saturating_sub(ds.iter().sum());
                        for (i, d) in ds.iter_mut().enumerate() {
                            *d += padding / count as u64
                                + u64::from((i as u64) < padding % count as u64);
                        }
                        ds
                    } else {
                        let fps = lib
                            .images
                            .get(&section, "fps")
                            .unwrap()
                            .parse::<u64>()
                            .unwrap();
                        vec![1000 / fps; count]
                    };
                    let mut total = 0;
                    for d in &mut delays {
                        total += (*d).max(1);
                        *d = total;
                    }
                    assert_eq!(
                        anim.timeline.ends(),
                        delays,
                        "{name}/{s}: native idle timing"
                    );
                    for (row, &image) in anim.frames.iter().enumerate() {
                        assert!(
                            visuals.images[image].rgba
                                == source_frame(&a, &lib, &section, index, row).rgba,
                            "{name}/{s}: idle direction {index}, row {row}"
                        );
                        let start = if row == 0 {
                            0
                        } else {
                            anim.timeline.ends()[row - 1]
                        };
                        assert_eq!(anim.timeline.frame(start, 0), row);
                        assert_eq!(anim.timeline.frame(anim.timeline.ends()[row] - 1, 0), row);
                    }
                    assert_eq!(anim.timeline.frame(anim.timeline.duration_ms, 0), 0);
                }
                let shadow_section = format!("{section}_s");
                if lib.images.get(&shadow_section, "image").is_some() {
                    let shadow = visuals
                        .visuals
                        .iter()
                        .find(|v| v.object == index && v.layer == Layer::Shadow)
                        .unwrap();
                    let n = |s: &str, k| {
                        lib.images
                            .get(s, k)
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(1)
                    };
                    let row = if n(&section, "rows") == n(&shadow_section, "rows") {
                        startrow
                    } else {
                        0
                    };
                    // A flier's shadow falls aside of its body (0x4e76f0, 0x4e77a0).
                    let [dx, dy] = retzar::world::shadow_lift(class_index);
                    assert_eq!(
                        shadow.top_left,
                        [body.top_left[0] + dx, body.top_left[1] - dy],
                        "{name}/{s}: shadow anchor"
                    );
                    assert!(
                        visuals.images[shadow.image].rgba
                            == source_frame(
                                &a,
                                &lib,
                                &shadow_section,
                                index % n(&shadow_section, "cols"),
                                row
                            )
                            .rgba,
                        "{name}/{s}: shadow direction {index}"
                    );
                    if let Some(id) = shadow.animation {
                        let anim = &visuals.animations[id];
                        assert_eq!(shadow.phase_ms, body.phase_ms);
                        for (row, &image) in anim.frames.iter().enumerate() {
                            assert!(
                                visuals.images[image].rgba
                                    == source_frame(
                                        &a,
                                        &lib,
                                        &shadow_section,
                                        index % n(&shadow_section, "cols"),
                                        row
                                    )
                                    .rgba,
                                "{name}/{s}: shadow frame {row}"
                            );
                        }
                    }
                }
                if let Some(overlay_section) = lib.property(o, "image_c") {
                    let overlay = visuals
                        .visuals
                        .iter()
                        .find(|v| v.object == index && v.layer == Layer::Overlay)
                        .unwrap();
                    let col = lib
                        .images
                        .get(overlay_section, "cols")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1);
                    assert!(
                        visuals.images[overlay.image].rgba
                            == source_frame(&a, &lib, overlay_section, index % col, 0).rgba,
                        "{name}/{s}: banner"
                    );
                    let offset = lib.images.numbers(overlay_section, "object");
                    assert_eq!(
                        overlay.top_left,
                        [
                            body.top_left[0] + offset.first().copied().unwrap_or(0) as f32,
                            body.top_left[1] - offset.get(1).copied().unwrap_or(0) as f32
                        ],
                        "{name}/{s}: banner anchor"
                    );
                }
                assert!(lib.icon_for(o).is_some(), "{name}/{s}: no icon");
            }
            entries.push(serde_json::json!({"class":name,"civilization":s,"section":section,"directions":cols,"visuals":visuals.visuals.len(),"animations":visuals.animations.len()}));
        }
    }
    report(
        "class-presentation.json",
        serde_json::json!({"variants":entries.len(),"entries":entries}),
    );
    assert!(entries.len() > 100);
}

#[test]
#[ignore = "Requires the original civilization tables"]
fn civilization_attributes_follow_native_building_then_unit_table_precedence() {
    let a = assets();
    let lib = ObjectLibrary::load(&a, "grass").unwrap();
    let hud = retzar::hud::HudSkin::load(&a).unwrap();
    let mut reports = Vec::new();
    for civ in 1..=3 {
        let mut expected: BTreeMap<String, [u16; 6]> = BTreeMap::new();
        // Native 0x49751a–0x4975a8: civilization buildings, common
        // buildings, civilization units, common units. Numeric zero is data.
        for (kind, section) in [
            ("BUILDINGS", civ_section(civ)),
            ("BUILDINGS", "comm"),
            ("UNITS", civ_section(civ)),
            ("UNITS", "comm"),
        ] {
            let text = legacy_text(&a.get(&format!("DATA/{section} {kind}.TXT")).unwrap());
            for line in text.lines().skip(1) {
                let f: Vec<_> = line.split('\t').map(str::trim).collect();
                if f.len() < 9 || f[1].is_empty() {
                    continue;
                }
                let n = |i: usize| f.get(i).and_then(|s| s.parse::<u16>().ok()).unwrap_or(0);
                let record = expected.entry(f[1].to_ascii_lowercase()).or_default();
                if kind == "BUILDINGS" {
                    record[0] = n(6);
                    record[3] = n(4);
                    record[5] = n(5);
                } else {
                    record[0] = n(6);
                    record[1] = n(7);
                    record[2] = n(8);
                    record[3] = n(4);
                    record[4] = n(5);
                }
            }
        }
        for (name, values) in expected {
            let Some(index) = CLASSES.iter().position(|c| *c == name) else {
                panic!("Unknown table class {name}")
            };
            let o = object(index, civ, 0);
            let actual = lib.attributes.get(&o);
            assert_eq!(
                [
                    actual.health,
                    actual.attack,
                    actual.defence,
                    actual.sight,
                    actual.range,
                    actual.resistance
                ],
                values,
                "{name}/{}",
                civ_section(civ)
            );
            let state = ObjectState::read(&o, &lib);
            assert_eq!(
                (state.attack, state.defence, state.sight),
                (values[1] as u32, values[2] as u32, values[3]),
                "{name}/{}",
                civ_section(civ)
            );
            if let Some(stats) = hud.class_stats(&o)
                && (39..=43).contains(&index)
            {
                assert_eq!(stats.attack, Some(values[1] as u32), "tower HUD: {name}");
                assert_eq!(stats.resistance, Some(values[5] as u32));
            }
            reports.push(serde_json::json!({"class":name,"civilization":civ_section(civ),"health":values[0],"attack":values[1],"defence":values[2],"sight":values[3],"range":values[4],"resistance":values[5]}));
        }
    }
    report(
        "civilization-attributes.json",
        serde_json::json!({"records":reports.len(),"entries":reports}),
    );
}

#[test]
#[ignore = "Requires original terrain and scenery artwork"]
fn every_biome_tree_cliff_and_water_page_uses_its_original_pixels() {
    use retzar::{
        assets::Ini,
        map::MapGrid,
        terrain::Terrain,
        world::{add_cliffs, add_trees, cliff_file},
    };
    let a = assets();
    let mut reports = Vec::new();
    for biome in ["grass", "summer", "snow", "desert"] {
        let lib = ObjectLibrary::load(&a, biome).unwrap();
        let ini = Ini::parse(
            &a.get(&format!("IMAGES/TERRAIN/{biome}.TERRAIN.INI"))
                .unwrap(),
        );
        let dir = ini.get("terrain", "dir").unwrap_or(biome);
        let mut grid = MapGrid {
            name: biome.into(),
            side: 32,
            cells: vec![0; 1024],
        };
        for t in 1..=31 {
            grid.cells[32 + t] = (t as u16) << 11;
        }
        let mut scene = lib.build(&a, &MissionWorld::default(), 32).unwrap();
        assert_eq!(add_trees(&a, &grid, biome, &mut scene).unwrap(), 31);
        let trees = Bitmap::decode(
            &a.get(&format!(
                "IMAGES/TERRAIN/{dir}/{}",
                ini.get("trees", "img_trees").unwrap()
            ))
            .unwrap(),
            true,
        )
        .unwrap();
        for t in 0..31 {
            let body = &scene.visuals[t * 2 + 1];
            assert!(
                scene.images[body.image].rgba
                    == trees.crop(t as u32 * 96, 0, 96, 128).unwrap().rgba,
                "{biome}: tree {t}"
            );
        }
        let mut cliff_frames = 0;
        for kind in 0..8 {
            let file = format!("IMAGES/TERRAIN/{dir}/{}", cliff_file(&ini, kind));
            if !a.contains(&file) {
                continue;
            }
            let raw = Bitmap::decode(&a.get(&file).unwrap(), true).unwrap();
            for column in 0..raw.width / 32 {
                for row in 0..raw.height / 32 {
                    assert!(column < 8 && row < 16);
                    let grid = MapGrid {
                        name: biome.into(),
                        side: 1,
                        cells: vec![0x400 | (kind << 7) | ((column as u16) << 4) | row as u16],
                    };
                    let mut scene = lib.build(&a, &MissionWorld::default(), 1).unwrap();
                    assert_eq!(add_cliffs(&a, &grid, biome, &mut scene).unwrap(), 1);
                    assert!(
                        scene.images[scene.visuals[0].image].rgba
                            == raw.crop(column * 32, row * 32, 32, 32).unwrap().rgba,
                        "{biome}: cliff {kind}/{column}/{row}"
                    );
                    cliff_frames += 1;
                }
            }
        }
        let terrain = Terrain::load(&a, biome).unwrap();
        let mut pages = 0;
        for sheet in &terrain.sheets {
            let raw = Bitmap::decode(&a.get(&sheet.name).unwrap(), false).unwrap();
            let file = sheet.name.rsplit('/').next().unwrap();
            let dims = ini.numbers("terrainsize", file.trim_end_matches(".bmp"));
            let coast_frames = dims.get(2).copied().unwrap_or(0);
            for (phase, page) in std::iter::once(&sheet.image)
                .chain(&sheet.frames)
                .enumerate()
            {
                if coast_frames > 0 {
                    for col in 0..sheet.columns as u32 {
                        assert!(
                            page.crop(col * 32, 0, 32, page.height).unwrap().rgba
                                == raw
                                    .crop(
                                        (col * coast_frames as u32 + phase as u32) * 32,
                                        0,
                                        32,
                                        page.height
                                    )
                                    .unwrap()
                                    .rgba,
                            "{biome}/{file}: coast {phase}/{col}"
                        );
                    }
                } else {
                    assert!(
                        page.rgba
                            == raw
                                .crop(0, phase as u32 * page.height, page.width, page.height)
                                .unwrap()
                                .rgba,
                        "{biome}/{file}: page {phase}"
                    );
                }
                pages += 1;
            }
        }
        reports.push(serde_json::json!({"biome":biome,"trees":31,"cliff_frames":cliff_frames,"terrain_pages":pages,"clouds":terrain.clouds,"rain":terrain.rain,"snow":terrain.snow}));
    }
    // A map flag has eight temporal pictures, independent of capture gameplay.
    let lib = ObjectLibrary::load(&a, "grass").unwrap();
    let m = MissionWorld {
        objects: vec![object(class::ITEM_FLAG, 1, 0)],
        ..Default::default()
    };
    let scene = lib.build(&a, &m, 64).unwrap();
    let body = scene
        .visuals
        .iter()
        .find(|v| v.layer == Layer::Body)
        .unwrap();
    let anim = &scene.animations[body.animation.expect("native capture flag timer")];
    assert_eq!(anim.frames.len(), 8);
    assert_eq!(anim.timeline.duration_ms, 8 * (1000 / 35));
    for (row, &frame) in anim.frames.iter().enumerate() {
        assert!(scene.images[frame].rgba == source_frame(&a, &lib, "flag_capture", 0, row).rgba);
    }
    report(
        "nature.json",
        serde_json::json!({"biomes":reports,"capture_flag_frames":anim.frames.len()}),
    );
}
