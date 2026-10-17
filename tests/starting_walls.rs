use retzar::{
    assets::GameAssets,
    map::MapGrid,
    scenario::ScenarioCatalog,
    world::{Layer, MissionWorld, ObjectLibrary, class},
};

fn source() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}
fn normalize(world: &mut MissionWorld, lib: &ObjectLibrary) {
    for o in &mut world.objects {
        if o.building() || retzar::selection::unit(o) {
            let hp = lib.attributes.get(o).health;
            if o.health == 0 || o.flags & 0x40 != 0 {
                o.health = hp;
            }
            if o.max_health == 0 || o.flags & 0x80 != 0 {
                o.max_health = hp;
            }
        }
    }
}

#[test]
#[ignore = "Requires original installed maps and artwork"]
fn all_custom_maps_reset_to_castle_and_three_workers_and_keep_neutral_objects() {
    let a = GameAssets::open(&source()).unwrap();
    let catalog = ScenarioCatalog::load(&source()).unwrap();
    let mut report = Vec::new();
    for scenario in &catalog.missions {
        let c = scenario.container().unwrap();
        let grid = MapGrid::read(&c, &scenario.header).unwrap();
        let header = c.get(&scenario.header).unwrap();
        let lib = ObjectLibrary::load(&a, &retzar::terrain::biome_of(&header).unwrap()).unwrap();
        let original = MissionWorld::read(&c, &grid).unwrap();
        let neutral: Vec<_> = original
            .objects
            .iter()
            .filter(|o| !(1..=8).contains(&o.owner))
            .map(|o| (o.id, o.class, o.owner, o.x, o.y))
            .collect();
        for civ in 1..=3 {
            let mut m = MissionWorld::read(&c, &grid).unwrap();
            for k in &mut m.kingdoms {
                k.civilization = civ;
            }
            m.reset_initial_forces(&grid, &lib)
                .unwrap_or_else(|e| panic!("{}: {e}", scenario.title));
            normalize(&mut m, &lib);
            m.refresh_walls(&lib, grid.side);
            assert_eq!(
                neutral,
                m.objects
                    .iter()
                    .filter(|o| !(1..=8).contains(&o.owner))
                    .map(|o| (o.id, o.class, o.owner, o.x, o.y))
                    .collect::<Vec<_>>()
            );
            let forces: Vec<_> = m
                .objects
                .iter()
                .filter(|o| {
                    (1..=8).contains(&o.owner) && (o.building() || retzar::selection::unit(o))
                })
                .collect();
            let ids: std::collections::BTreeSet<_> = forces.iter().map(|o| o.id).collect();
            assert_eq!(ids.len(), forces.len());
            assert!(
                forces
                    .iter()
                    .all(|o| o.id != 0 && !neutral.iter().any(|n| n.0 == o.id))
            );
            let highest_original = original
                .objects
                .iter()
                .map(|o| o.id)
                .filter(|&id| id < 0xfc17)
                .max()
                .unwrap();
            assert!(forces.iter().all(|o| o.id > highest_original));
            for (owner, _) in m.active_kingdoms() {
                let force: Vec<_> = m
                    .objects
                    .iter()
                    .filter(|o| o.owner == owner && (o.building() || retzar::selection::unit(o)))
                    .collect();
                assert_eq!(force.len(), 4, "{} owner {owner}", scenario.title);
                assert_eq!(force.iter().filter(|o| o.class == class::CASTLE).count(), 1);
                assert_eq!(force.iter().filter(|o| o.class == class::PEON).count(), 3);
                assert!(
                    force
                        .iter()
                        .all(|o| o.civilization == civ && o.health > 0 && o.health == o.max_health)
                );
                for o in &force {
                    let base = lib.attributes.get(o);
                    let state = retzar::simulation::ObjectState::read(o, &lib);
                    assert_eq!(state.sight, base.sight);
                    assert_eq!(state.attack, base.attack as u32);
                    assert_eq!(state.defence, base.defence as u32);
                }
                let castle = force.iter().find(|o| o.class == class::CASTLE).unwrap();
                let rect = lib
                    .images
                    .numbers(&lib.image_for(castle).unwrap(), "object");
                let size = [rect[2] as u16 / 32, rect[3] as u16 / 32];
                for peon in force.iter().filter(|o| o.class == class::PEON) {
                    assert!(
                        peon.x < castle.x
                            || peon.y < castle.y
                            || peon.x >= castle.x + size[0]
                            || peon.y >= castle.y + size[1]
                    );
                }
            }
            let art = lib.build(&a, &m, grid.side).unwrap();
            assert!(art.missing.is_empty());
            let mut replay = MissionWorld::read(&c, &grid).unwrap();
            for k in &mut replay.kingdoms {
                k.civilization = civ;
            }
            replay.reset_initial_forces(&grid, &lib).unwrap();
            normalize(&mut replay, &lib);
            replay.refresh_walls(&lib, grid.side);
            assert_eq!(
                serde_json::to_vec(&m).unwrap(),
                serde_json::to_vec(&replay).unwrap()
            );
            report.push(serde_json::json!({"mission":scenario.title,"civilization":civ,
                "kingdoms":m.active_kingdoms().count(),"starting_positions":m.starting_positions,
                "neutral_objects":neutral.len(),"objects":m.objects.len(),"missing_art":art.missing}));
        }
    }
    assert_eq!(report.len(), 18);
}

#[test]
#[ignore = "Requires original installed wall pictures"]
fn walls_reconnect_across_owners_and_gates_and_render_every_damage_stage() {
    let a = GameAssets::open(&source()).unwrap();
    let catalog = ScenarioCatalog::load(&source()).unwrap();
    let (_, index) = catalog.locate("mp4.wmp::maps/mp4").unwrap();
    let scenario = &catalog.missions[index];
    let c = scenario.container().unwrap();
    let grid = MapGrid::read(&c, &scenario.header).unwrap();
    let original = MissionWorld::read(&c, &grid).unwrap();
    let lib = ObjectLibrary::load(&a, "grass").unwrap();
    let template = original
        .objects
        .iter()
        .find(|o| o.class == class::WALL_STONE)
        .unwrap()
        .clone();
    let wall = |id, x, y, owner, class| {
        let mut o = template.clone();
        o.id = id;
        o.x = x;
        o.y = y;
        o.owner = owner;
        o.class = class;
        o.data[56..58].copy_from_slice(&28u16.to_le_bytes());
        o.health = 100;
        o.max_health = 100;
        o
    };
    let mut m = MissionWorld::default();
    m.objects = vec![
        wall(1, 10, 10, 1, class::WALL_STONE),
        wall(2, 9, 10, 2, class::WALL_STONE),
        wall(3, 11, 10, 10, class::WALL_STONE),
        wall(4, 10, 9, 1, class::WALL_WOOD),
    ];
    m.refresh_walls(&lib, 64);
    assert_eq!(m.objects[0].word(56), 16);
    m.objects[1].flags = 0x4000;
    m.refresh_walls(&lib, 64);
    assert_eq!(m.objects[0].word(56), 2);
    m.objects[3].class = class::WALL_STONE;
    m.refresh_walls(&lib, 64);
    assert_eq!(m.objects[0].word(56), 3);
    // Replacing the eastern wall with an original seven-cell horizontal gate
    // must also connect the wall at its far end, not just its anchor cell.
    m.objects[2].class = class::GATE_STONE;
    m.objects[2].data[56] = 0;
    m.objects[2].data[57] = 0;
    m.objects.push(wall(5, 18, 10, 1, class::WALL_STONE));
    m.refresh_walls(&lib, 64);
    assert_eq!(m.objects[4].word(56), 8);
    m.objects[2].class = class::GATE_WOOD;
    m.refresh_walls(&lib, 64);
    assert_eq!(m.objects[4].word(56), 0);
    // A saved connection value 28 must not select column 4 of a vertical
    // picture whose independent saved frame is column zero.
    let mut straight = wall(25, 25, 25, 1, class::WALL_STONE);
    straight.variant = 0;
    straight.data[56..58].copy_from_slice(&28u16.to_le_bytes());
    let mut normal = MissionWorld::default();
    normal.objects.push(straight);
    let art = lib.build(&a, &normal, 64).unwrap();
    let body = art.visuals.iter().find(|v| v.layer == Layer::Body).unwrap();
    let original_sheet = retzar::sprite::SpriteSheet::decode(
        &a.get("IMAGES/BUILDING/WALLS/STONE_VERT.RLE").unwrap(),
    )
    .unwrap();
    assert_eq!(
        art.images[body.image].rgba,
        original_sheet.frame(0, 0).unwrap().rgba
    );
    for material in [class::WALL_STONE, class::WALL_WOOD] {
        for mask in [0, 16, 24] {
            for (hp, row) in [(99, 0), (79, 1), (59, 2), (39, 3), (19, 4)] {
                let mut o = wall(20, 20, 20, 1, material);
                o.health = hp;
                o.data[56..58].copy_from_slice(&(mask as u16).to_le_bytes());
                let section = lib.image_for(&o).unwrap();
                assert!(section.ends_with("_dest"));
                let mut damaged = MissionWorld::default();
                damaged.kingdoms = original.kingdoms.clone();
                damaged.objects.push(o.clone());
                let visuals = lib.build(&a, &damaged, 64).unwrap();
                let body = visuals
                    .visuals
                    .iter()
                    .find(|v| v.layer == Layer::Body)
                    .unwrap();
                let file = lib
                    .images
                    .get(&section, "image")
                    .unwrap()
                    .replace('\\', "/");
                let packed = format!("IMAGES/{}.RLE", file.trim_end_matches(".bmp"));
                let image = retzar::sprite::SpriteSheet::decode(&a.get(&packed).unwrap()).unwrap();
                assert_eq!(
                    visuals.images[body.image].rgba,
                    image.frame(0, row as usize).unwrap().rgba
                );
                assert_eq!(
                    visuals.visuals.iter().any(|v| v.layer == Layer::Overlay),
                    mask == 0 && row == 0
                );
            }
        }
    }
}
