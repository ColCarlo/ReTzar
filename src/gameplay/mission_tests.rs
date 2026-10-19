//! Mission objects against the original loader and their own programs:
//! prisoners of owner 9 (0x44092d, 0x502e18), ruins and magic gateways
//! taken by presence (0x4b6760, 0x420fd0), the score of a self-destroyed
//! object (0x5038df) and the objects the loader leaves out (0x4407a0).

use super::tests::{request, setup};
use super::*;

fn ticks(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    n: u64,
) {
    for _ in 0..n {
        advance(sim, m, g, lib, c, Duration::from_millis(TICK_MS), 10, false);
    }
}
fn class_of(name: &str) -> usize {
    CLASSES.iter().position(|&s| s == name).unwrap()
}
fn standing_unit(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    class: usize,
    owner: u8,
    at: [u16; 2],
) -> usize {
    let k = new_unit(sim, m, lib, class, owner, 1, at).unwrap();
    sim.gameplay.actors[k].stance = 2;
    k
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_prisoners_of_a_map_wait_to_be_freed_from_its_start() {
    // As `game::start` does: the map's objects, a new simulation that has
    // not stepped yet, then the loader's pass (0x44092d).
    let (_, lib, mut m, _, _) = setup(1);
    let mut used = m.objects.iter().map(|o| o.id).collect();
    m.objects.push(
        world::starting::new_object(&mut used, 300, class::PEON, 9, 1, [30, 30], &lib).unwrap(),
    );
    let prisoner = m.objects.len() - 1;
    let mut sim = Simulation::new(&m, &lib, 64, "grass");
    hold_prisoners(&mut sim, &mut m, &lib);
    assert_eq!(
        sim.gameplay.actors[prisoner].intent,
        Some(Intent::Rescue(0))
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn prisoners_join_the_kingdom_of_a_fighter_beside_them() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let prisoner = new_unit(&mut sim, &mut m, &lib, class::PEON, 9, 1, [30, 30]).unwrap();
    hold_prisoners(&mut sim, &mut m, &lib);
    assert_eq!(
        sim.gameplay.actors[prisoner].intent,
        Some(Intent::Rescue(0))
    );
    // A peasant is no fighter (flag 0x10000): nothing happens.
    standing_unit(&mut sim, &mut m, &lib, class::PEON, 1, [31, 30]);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 25);
    assert_eq!(m.objects[prisoner].owner, 9);
    // A fighter two cells away is outside the 3×3 (0x501a20).
    let soldier = standing_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, [32, 30]);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 25);
    assert_eq!(m.objects[prisoner].owner, 9);
    // A kingdom that may not rescue (@kingdomcanrescue 0) frees nobody.
    position(&mut m.objects[soldier], [31, 31]);
    sim.gameplay.no_rescue.insert(1);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 25);
    assert_eq!(m.objects[prisoner].owner, 9);
    sim.gameplay.no_rescue.clear();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 25);
    assert_eq!(m.objects[prisoner].owner, 1);
    assert_ne!(
        sim.gameplay.actors[prisoner].intent,
        Some(Intent::Rescue(0))
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn gateways_and_ruins_go_to_the_only_kingdom_standing_beside_them() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let gate = new_unit(
        &mut sim,
        &mut m,
        &lib,
        class::MAGIC_GATEWAY,
        10,
        1,
        [30, 30],
    )
    .unwrap();
    m.objects[gate].flags &= !FLAG_CONSTRUCTION;
    let r = movement::footprint_rect(&lib, &m.objects[gate]);
    // Never hurt (vt+0x1d0 = 0x4b6740).
    let health = m.objects[gate].health;
    apply_damage(&mut sim, &mut m, &g, &lib, &c, gate, 500, 0, None);
    assert_eq!(m.objects[gate].health, health);
    // Fighters of two kingdoms in the footprint grown by one: nobody takes it.
    let ours = standing_unit(
        &mut sim,
        &mut m,
        &lib,
        class::FOOTMAN,
        1,
        [r[2] as u16 + 1, r[1] as u16],
    );
    let theirs = standing_unit(
        &mut sim,
        &mut m,
        &lib,
        class::FOOTMAN,
        2,
        [r[0] as u16 - 1, r[1] as u16],
    );
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 25);
    assert_eq!(m.objects[gate].owner, 10);
    position(&mut m.objects[theirs], [60, 60]);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 25);
    assert_eq!(m.objects[gate].owner, 1);
    assert!(
        sim.gameplay
            .notices
            .iter()
            .any(|n| n.contains("porta magica"))
    );
    // Its owner cannot destroy it (0x421190).
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("die", vec![gate]),
    )
    .unwrap();
    assert!(alive(&m.objects[gate]));
    let _ = ours;
    // A ruin of kingdom 2 gives it its picture's `ruinpoints` each second.
    let ruin = new_unit(&mut sim, &mut m, &lib, class::RUIN, 2, 1, [10, 50]).unwrap();
    m.objects[ruin].flags &= !FLAG_CONSTRUCTION;
    m.objects[ruin].custom_image = "gr_statue_1".into();
    let before = sim.gameplay.score.get(&2).copied().unwrap_or(0);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 40);
    assert_eq!(
        sim.gameplay.score.get(&2).copied().unwrap_or(0),
        before + 10
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn destroying_an_own_object_costs_a_hundred_points() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let unit = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
    sim.gameplay.score.insert(1, 250);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("die", vec![unit]),
    )
    .unwrap();
    assert_eq!(sim.gameplay.score[&1], 150);
    // Never below zero (0x4434a2).
    let other = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [32, 30]).unwrap();
    sim.gameplay.score.insert(1, 40);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("die", vec![other], [32, 30], None),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(sim.gameplay.score[&1], 0);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_loader_leaves_out_absent_kingdoms_and_king_of_the_hill_mines() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let catalog = crate::scenario::ScenarioCatalog::load(root).unwrap();
    let read = |id: &str| {
        let (_, index) = catalog.locate(id).unwrap();
        let scenario = &catalog.missions[index];
        let c = scenario.container().unwrap();
        let grid = MapGrid::read(&c, &scenario.header).unwrap();
        MissionWorld::read(&c, &grid).unwrap()
    };
    // mp3 has objects of kingdoms 3 and 4, which do not take part.
    let mut m = read("mp3.wmp::maps/mp3");
    assert!(m.objects.iter().any(|o| matches!(o.owner, 3 | 4)));
    m.apply_loader_rules();
    assert!(
        m.objects
            .iter()
            .all(|o| o.flags & FLAG_INACTIVE != 0 || !matches!(o.owner, 3 | 4))
    );
    // King of the Hill: no mines with many resources, endless ones else.
    let mines = |m: &MissionWorld| {
        m.objects
            .iter()
            .filter(|o| matches!(o.class, class::RES_GOLD | class::RES_STONE))
            .count()
    };
    let mut m = read("mp4.wmp::maps/mp4");
    assert!(mines(&m) > 0);
    m.game_rules.game_type = 1;
    m.game_rules.resource_level = 1;
    m.apply_loader_rules();
    assert!(
        m.objects
            .iter()
            .filter(|o| matches!(o.class, class::RES_GOLD | class::RES_STONE))
            .all(|o| o.byte(0x3f) == 1)
    );
    m.game_rules.resource_level = 2;
    m.apply_loader_rules();
    assert_eq!(mines(&m), 0);
    let _ = class_of("ruin");
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn map_monks_and_priests_start_at_their_kingdoms_level() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let catalog = crate::scenario::ScenarioCatalog::load(root).unwrap();
    let scenario = catalog
        .campaigns
        .iter()
        .flat_map(|c| c.missions.iter())
        .find(|s| s.header.contains("Gengis_Khan01"))
        .unwrap();
    let c = scenario.container().unwrap();
    let grid = MapGrid::read(&c, &scenario.header).unwrap();
    let mut m = MissionWorld::read(&c, &grid).unwrap();
    let levels = |m: &MissionWorld| {
        m.objects
            .iter()
            .filter(|o| matches!(o.class, 12 | 18) && o.owner != 0)
            .map(|o| o.word(0x1c))
            .collect::<Vec<_>>()
    };
    // The records carry levels of their own (10 and 19)…
    assert!(levels(&m).iter().any(|&l| l > 0));
    // …which the kingdom's level, still 0 at loading, replaces (0x4eb340).
    m.apply_loader_rules();
    let after = levels(&m);
    assert!(!after.is_empty() && after.iter().all(|&l| l == 0));
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_campaign_map_starts_under_the_fog_whatever_the_setup_carries() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let assets = GameAssets::open(root).unwrap();
    let catalog = crate::scenario::ScenarioCatalog::load(root).unwrap();
    // Every map of every campaign there, those of the other editions too.
    let maps: Vec<(usize, usize)> = (0..catalog.campaigns.len())
        .flat_map(|c| (0..catalog.campaigns[c].missions.len()).map(move |m| (c, m)))
        .collect();
    assert!(maps.len() > 30);
    for (c, mission) in maps {
        let scenario = catalog.campaigns[c].missions[mission].clone();
        let id = scenario.title.clone();
        let header = scenario.container().unwrap().get(&scenario.header).unwrap();
        let campaign = Some(c);
        let selection = crate::menu::Selection::Scenario { campaign, mission };
        // The setup of an older save, with the fog off and the map explored.
        let setup = world::GameSetup {
            options: Some(world::GameRules {
                no_fog: true,
                explored: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let opened = crate::game::open(&assets, &selection, Some(scenario), Some(&setup)).unwrap();
        // 0x441b93: the fog and the exploration of the map's header (0x4e,
        // 0x4f), 0 in all but three maps of Arthur's that start explored
        // ('Merlin' also with no fog).
        let r = &opened.mission.game_rules;
        assert_eq!(
            (r.no_fog, r.explored),
            (header[0x4e] != 0, header[0x4f] != 0),
            "{id}"
        );
        if r.no_fog || r.explored {
            continue;
        }
        // Started, only what the player and the allies sharing their sight
        // see now is explored.
        let lib = ObjectLibrary::load(&assets, &opened.biome).unwrap();
        let mut m = opened.mission;
        crate::game::start(
            &mut m,
            &opened.grid,
            &lib,
            &opened.biome,
            &selection,
            Some(&setup),
            crate::terrain::Climate::default(),
        )
        .unwrap();
        let fog = &m.simulation.as_ref().unwrap().fog;
        let beyond = (0..fog.explored.len())
            .filter(|&k| fog.explored[k] && !fog.visible[k])
            .count();
        assert_eq!(beyond, 0, "{id}");
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn campaign_missions_follow_the_campaign_rules_and_test_the_end_at_events() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let assets = GameAssets::open(root).unwrap();
    let catalog = crate::scenario::ScenarioCatalog::load(root).unwrap();
    let c = Catalog::load(&assets).unwrap();
    let open = |id: &str| {
        let (campaign, mission) = catalog.locate(id).unwrap();
        let scenario = catalog.campaigns[campaign.unwrap()].missions[mission].clone();
        let header = scenario.container().unwrap().get(&scenario.header).unwrap();
        let selection = crate::menu::Selection::Scenario { campaign, mission };
        let opened = crate::game::open(&assets, &selection, Some(scenario), None).unwrap();
        let lib = ObjectLibrary::load(&assets, &opened.biome).unwrap();
        let mut m = opened.mission;
        crate::game::start(
            &mut m,
            &opened.grid,
            &lib,
            &opened.biome,
            &selection,
            None,
            crate::terrain::Climate::default(),
        )
        .unwrap();
        let sim = m.simulation.take().unwrap();
        (
            world::GameRules::from_header(&header),
            m,
            opened.grid,
            lib,
            sim,
        )
    };
    // 0x444890: a campaign plays a standard game to 200 of population,
    // whatever its header says («Il mito di Camelot» has «destroy the
    // castles», «La fata Morgana» a population of 0, the Khan's first map 50).
    for (id, victory, population) in [
        (
            "excalibur e il re artu.wcm::maps/8_a el mito de camelot",
            1,
            200,
        ),
        ("excalibur e il re artu.wcm::maps/5 morgana le fay", 0, 0),
        ("impero di gengis khan.wcm::maps/gengis_khan01", 0, 50),
    ] {
        let (header, m, ..) = open(id);
        assert_eq!((header.victory, header.population), (victory, population));
        let r = &m.game_rules;
        assert_eq!((r.game_type, r.victory, r.population), (0, 0, 200), "{id}");
    }
    // «Merlino e il Re Artù 2»: Merlin's kingdom has only hidden objects at
    // the start and its script keeps it in the game before anything dies;
    // tested at every step it fell at once, and the mission with it.
    let (_, mut m, mut g, lib, mut sim) =
        open("excalibur e il re artu.wcm::maps/1_b merlín y el rey arturo b");
    for _ in 0..30 {
        if sim.gameplay.script.modal {
            if sim.gameplay.script.conversation.is_some() {
                answer(&mut sim, &mut m, &g, &lib, &c, 0);
            } else {
                close_window(&mut sim, &mut m);
            }
        }
        sim.gameplay.script.outbox.0.clear();
        advance(
            &mut sim,
            &mut m,
            &mut g,
            &lib,
            &c,
            Duration::from_secs(1),
            10,
            false,
        );
    }
    assert!(!sim.gameplay.script.kingdoms[4].defeated);
    assert_eq!(sim.gameplay.script.over, None);
}
