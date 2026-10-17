use retzar::{
    assets::GameAssets,
    bitmap::Bitmap,
    map::MapGrid,
    scenario::ScenarioCatalog,
    sprite::SHADOW,
    world::{Layer, MissionWorld, ObjectLibrary, add_trees, class, depth},
};

fn assets() -> GameAssets {
    GameAssets::open(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    )
    .unwrap()
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn painter_priorities_dominate_position_and_keep_shadow_body_overlay_order() {
    use retzar::world::{painter_depths, presentation_priority};
    let assets = assets();
    let library = ObjectLibrary::load(&assets, "grass").unwrap();
    let mut m = mission();
    let template = m
        .objects
        .iter()
        .find(|o| o.class == class::PEON)
        .unwrap()
        .clone();
    for (class, expected) in [(class::RES_GOLD, 100), (class::RES_STONE, 100), (63, 50)] {
        let mut o = template.clone();
        o.class = class;
        assert_eq!(presentation_priority(&o), expected);
    }
    m.objects = [3, 79, 15, 22, 32]
        .into_iter()
        .map(|class| {
            let mut o = template.clone();
            o.class = class;
            o.flags = 0;
            o.custom_image.clear();
            o.civilization = 1;
            o.x = 10;
            o.y = if class == 3 { 240 } else { 1 };
            o
        })
        .collect();
    let visuals = library.build(&assets, &m, 256).unwrap();
    let depths = painter_depths(&visuals.visuals, &m, 256);
    let body = |object| {
        depths[visuals
            .visuals
            .iter()
            .position(|v| v.object == object && v.layer == Layer::Body)
            .unwrap()]
    };
    assert!(body(1) < body(0));
    for i in 2..5 {
        assert_eq!(presentation_priority(&m.objects[i]), 300);
        assert!(body(i) > body(0));
    }
    for i in 0..m.objects.len() {
        for (v, &z) in visuals
            .visuals
            .iter()
            .zip(&depths)
            .filter(|(v, _)| v.object == i)
        {
            match v.layer {
                Layer::Shadow => assert!(z < body(i)),
                Layer::Overlay => assert!(z > body(i)),
                _ => {}
            }
        }
    }
}

fn mission() -> MissionWorld {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let catalog = ScenarioCatalog::load(source).unwrap();
    let (_, index) = catalog.locate("mp4.wmp::maps/mp4").unwrap();
    let scenario = &catalog.missions[index];
    let container = scenario.container().unwrap();
    let grid = MapGrid::read(&container, &scenario.header).unwrap();
    MissionWorld::read(&container, &grid).unwrap()
}

#[test]
#[ignore = "Requires installed original terrain atlases"]
fn all_124_tree_shadows_match_native_masks_and_anchors() {
    let a = assets();
    for biome in ["grass", "summer", "snow", "desert"] {
        let lib = ObjectLibrary::load(&a, biome).unwrap();
        let mut scene = lib.build(&a, &MissionWorld::default(), 32).unwrap();
        let mut grid = MapGrid {
            name: "trees".into(),
            side: 32,
            cells: vec![0; 1024],
        };
        for tree in 1..=31 {
            grid.cells[32 + tree] = (tree as u16) << 11;
        }
        assert_eq!(add_trees(&a, &grid, biome, &mut scene).unwrap(), 31);
        assert_eq!(
            scene.visuals.len(),
            62,
            "{biome}: one body and shadow per tree"
        );
        let atlas = Bitmap::decode(
            &a.get(&format!("IMAGES/TERRAIN/{biome}/trees_s.bmp"))
                .unwrap(),
            true,
        )
        .unwrap();
        for tree in 1..=31 {
            let shadow = &scene.visuals[(tree - 1) * 2];
            let body = &scene.visuals[(tree - 1) * 2 + 1];
            assert_eq!(shadow.layer, Layer::Shadow);
            assert_eq!(body.layer, Layer::Body);
            assert_eq!(shadow.top_left, [tree as f32 * 32., 0.]);
            assert_eq!(body.top_left, [tree as f32 * 32. - 32., 64.]);
            let mut mask = atlas.crop((tree as u32 - 1) * 128, 0, 128, 96).unwrap();
            for p in mask.rgba.chunks_exact_mut(4) {
                if p[3] != 0 {
                    p.copy_from_slice(&SHADOW);
                }
            }
            assert_eq!(scene.images[shadow.image].rgba, mask.rgba, "{biome}/{tree}");
            // Stumps/bare variants may deliberately have an empty mask.
        }
    }
}

#[test]
#[ignore = "Requires installed original icon definitions and item records"]
fn books_use_contained_spell_icons_and_weapons_keep_their_original_icons() {
    let a = assets();
    let lib = ObjectLibrary::load(&a, "grass").unwrap();
    let m = mission();
    let mut book = m
        .objects
        .iter()
        .find(|o| o.class == class::ITEM_MAGIC)
        .unwrap()
        .clone();
    for picture in ["item_book1", "item_book2", "item_book3"] {
        book.custom_image = picture.into();
        for spell in 0..21u8 {
            book.data[96] = spell;
            let icon = lib.icon_for(&book);
            // The installed native atlas has no icon for soulbind/hypnotize.
            if [8, 13].contains(&spell) {
                assert_eq!(icon, None);
            } else {
                let key = icon.unwrap();
                assert!(key.starts_with("spell_"));
                assert_eq!(lib.icons.numbers(key, "index"), [199 + spell as i32]);
            }
            assert_eq!(lib.image_for(&book).as_deref(), Some(picture));
        }
    }
    book.data[96] = 255;
    assert_eq!(lib.icon_for(&book), None);
    let mut weapon = m
        .objects
        .iter()
        .find(|o| o.class == class::ITEM_PLUSPLUS)
        .unwrap()
        .clone();
    for (key, index) in [
        ("item_sword1", 16),
        ("item_sword2", 17),
        ("item_sword3", 18),
        ("item_shield1", 2),
        ("item_shield2", 1),
        ("item_shield3", 3),
        ("item_bow1", 5),
        ("item_bow2", 6),
        ("item_bow3", 4),
    ] {
        weapon.custom_image = key.into();
        assert_eq!(lib.icon_for(&weapon), Some(key));
        assert_eq!(lib.icons.numbers(key, "index"), [index]);
    }
    weapon.custom_image = "item_unknown".into();
    assert_eq!(lib.icon_for(&weapon), None);
}

#[test]
#[ignore = "Requires original wall artwork"]
fn overlapping_walls_composite_in_native_order_without_repeated_caps() {
    let a = assets();
    let lib = ObjectLibrary::load(&a, "grass").unwrap();
    let original = mission();
    let template = original
        .objects
        .iter()
        .find(|o| o.class == class::WALL_STONE)
        .unwrap();
    for material in [class::WALL_STONE, class::WALL_WOOD] {
        for vertical in [false, true] {
            let mut m = MissionWorld::default();
            m.kingdoms = original.kingdoms.clone();
            for i in 0..8 {
                let mut o = template.clone();
                o.id = i + 1;
                o.class = material;
                o.x = 10 + if vertical { 0 } else { i };
                o.y = 10 + if vertical { i } else { 0 };
                o.health = 100;
                o.max_health = 100;
                m.objects.push(o);
            }
            m.refresh_walls(&lib, 64);
            let scene = lib.build(&a, &m, 64).unwrap();
            let mut painter = scene
                .visuals
                .iter()
                .filter(|v| v.layer == Layer::Body)
                .collect::<Vec<_>>();
            let mut native = painter.clone();
            painter.sort_by(|a, b| depth(a.footprint, 64).total_cmp(&depth(b.footprint, 64)));
            // Independent oracle: 0x4f1000, y ascending and x descending.
            native.sort_by_key(|v| {
                let o = &m.objects[v.object];
                (o.y, std::cmp::Reverse(o.x))
            });
            let paint = |order: Vec<&retzar::world::ObjectVisual>| {
                let mut canvas = vec![0u8; 352 * 352 * 4];
                for v in order {
                    let image = &scene.images[v.image];
                    let x = (v.top_left[0] - 256.) as usize;
                    let y = (-v.top_left[1] - 256.) as usize;
                    for sy in 0..image.height as usize {
                        for sx in 0..image.width as usize {
                            let src = (sy * image.width as usize + sx) * 4;
                            if image.rgba[src + 3] != 0 {
                                let dst = ((y + sy) * 352 + x + sx) * 4;
                                canvas[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
                            }
                        }
                    }
                }
                canvas
            };
            assert_eq!(
                paint(painter),
                paint(native),
                "material {material}, vertical {vertical}"
            );
        }
    }
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn units_cross_bridges_over_the_deck_and_behind_the_front_railing() {
    use retzar::world::painter_depths;
    let assets = assets();
    let library = ObjectLibrary::load(&assets, "grass").unwrap();
    let mut m = mission();
    let template = m
        .objects
        .iter()
        .find(|o| o.class == class::PEON)
        .unwrap()
        .clone();
    let at = |class: usize, x: u16, y: u16, way: u8| {
        let mut o = template.clone();
        o.class = class;
        o.flags = 0;
        o.owner = 1;
        o.custom_image.clear();
        o.civilization = 1;
        o.variant = 0;
        o.x = x;
        o.y = y;
        if o.data.len() < 128 {
            o.data.resize(128, 0);
        }
        o.data[56] = way;
        o
    };
    m.objects = vec![
        at(class::BRIDGE, 10, 10, 0),
        at(8, 15, 12, 0),
        at(8, 15, 16, 0),
        at(class::BRIDGE, 40, 10, 1),
        at(8, 43, 14, 0),
        at(class::CASTLE, 60, 10, 0),
        at(8, 64, 11, 0),
    ];
    let visuals = library.build(&assets, &m, 128).unwrap();
    assert!(visuals.missing.is_empty(), "{:?}", visuals.missing);
    let depths = painter_depths(&visuals.visuals, &m, 128);
    let bodies = |object: usize| -> Vec<f32> {
        visuals
            .visuals
            .iter()
            .zip(&depths)
            .filter(|(v, _)| v.object == object && v.layer == Layer::Body)
            .map(|(_, &z)| z)
            .collect()
    };
    // 0x4f0df0: the horizontal bridge's own node and its railing's, 178
    // pixels lower (0x41bfc0); a knight on the deck is between them, one
    // on its last row in front of both.
    let deck = bodies(0)[0];
    let railing: Vec<f32> = visuals
        .visuals
        .iter()
        .zip(&depths)
        .filter(|(v, _)| v.object == 0 && v.layer == Layer::Overlay)
        .map(|(_, &z)| z)
        .collect();
    assert_eq!(
        (bodies(0).len(), railing.len()),
        (1, 1),
        "body and front railing"
    );
    let railing = railing[0];
    let knight = bodies(1)[0];
    assert!(
        deck < knight && knight < railing,
        "{deck} {knight} {railing}"
    );
    assert!(bodies(2)[0] > railing);
    // A vertical bridge has no railing node: the knight is over it.
    assert!(
        !visuals
            .visuals
            .iter()
            .any(|v| v.object == 3 && v.layer == Layer::Overlay)
    );
    assert!(bodies(4)[0] > bodies(3)[0]);
    // 0x500950: nodes sort by the corner of their cell, so a knight beside
    // a castle, below its first row, is in front of it.
    assert!(bodies(6)[0] > bodies(5)[0]);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn clicks_take_the_unit_on_a_bridge_before_the_bridge() {
    let assets = assets();
    let library = ObjectLibrary::load(&assets, "grass").unwrap();
    let mut m = mission();
    let template = m
        .objects
        .iter()
        .find(|o| o.class == class::PEON)
        .unwrap()
        .clone();
    let at = |class: usize, x: u16, y: u16, way: u8| {
        let mut o = template.clone();
        o.class = class;
        o.flags = 0;
        o.owner = 1;
        o.custom_image.clear();
        o.civilization = 1;
        o.variant = 0;
        o.x = x;
        o.y = y;
        o.data.resize(o.data.len().max(128), 0);
        o.data[56] = way;
        o
    };
    m.objects = vec![at(class::BRIDGE, 40, 10, 1), at(8, 43, 14, 0)];
    let visuals = library.build(&assets, &m, 128).unwrap();
    let point = |x: f32, y: f32| [x * 32. + 16., -(y * 32. + 16.)];
    let pick = |p| retzar::selection::pointed(&m, &library, &visuals.visuals, p, |_, _| true);
    // 0x4e4280, 0x44e900: the knight's cell, and a click within 30 pixels
    // of its picture's middle, take the knight, whose picture lies over
    // the bridge's; a cell further on is the bridge's (0x44ea10).
    assert_eq!(pick(point(43., 14.)), Some(1));
    assert_eq!(pick(point(43. + 10. / 32., 14.)), Some(1));
    assert_eq!(pick(point(45., 14.)), Some(0));
    // 0x44e400: elsewhere on the deck, the bridge.
    assert_eq!(pick(point(43., 18.)), Some(0));
}
