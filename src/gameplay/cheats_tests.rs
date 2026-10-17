//! The cheat panel's commands against the original's handler (0x424140).

use super::cheats::{Cheat, Kind, next_map, pertinent, researches_left};
use super::tests::setup;
use super::*;

fn cheat(cheat: Cheat, selected: Vec<usize>) -> Request {
    Request {
        order: "cheat".into(),
        selected,
        cheat: Some(cheat),
        ..Default::default()
    }
}

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

#[test]
#[ignore = "Requires the installed original game archives"]
fn resources_population_fog_and_time_as_the_original_commands() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    assert!(!m.cheater);
    // 0x424277: «dvaleva» gives 10 000 of each resource, «petleva» the
    // same five times; either marks the player a cheater (0x4240e0).
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::Resources(1), vec![]),
    )
    .unwrap();
    assert!(m.cheater);
    assert_eq!(m.kingdoms[0].resources, [20000; 4]);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::Resources(5), vec![]),
    )
    .unwrap();
    assert_eq!(m.kingdoms[0].resources, [70000; 4]);
    assert_eq!(m.kingdoms[1].resources, [10000; 4]);
    // 0x424530: housing and cap at 1000; a later Supply Storage adds its
    // quarter of the game's limit to the cap (0x4d3819).
    assert!(pertinent(&m, &lib, Kind::NoPop));
    issue(&mut sim, &mut m, &g, &lib, &c, &cheat(Cheat::NoPop, vec![])).unwrap();
    let (_, housing, cap) = population_parts(&m, &lib, 1);
    assert_eq!((housing, cap), (1000, 1000));
    assert!(!pertinent(&m, &lib, Kind::NoPop));
    finish_research(&mut sim, &mut m, &lib, 1, "te_supplystorage");
    assert_eq!(population_parts(&m, &lib, 1).2, 1000 + 200 * 25 / 100);
    // 0x4424d0, 0x442420: the fog goes, the map is explored; done once.
    assert!(pertinent(&m, &lib, Kind::NoFog));
    issue(&mut sim, &mut m, &g, &lib, &c, &cheat(Cheat::NoFog, vec![])).unwrap();
    assert!(m.game_rules.no_fog && !pertinent(&m, &lib, Kind::NoFog));
    m.simulation = Some(sim.clone());
    assert!(pertinent(&m, &lib, Kind::Reveal));
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::Reveal, vec![]),
    )
    .unwrap();
    assert!(sim.fog.explored_vertices.iter().all(|&e| e));
    m.simulation = Some(sim.clone());
    assert!(!pertinent(&m, &lib, Kind::Reveal));
    m.simulation = None;
    // 0x42465a → 0x42b520: the day clock to the hour.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::Time(14), vec![]),
    )
    .unwrap();
    assert_eq!(sim.clock_minutes(), 14 * 60);
    // King of the Hill with «molte» resources keeps every stock: nothing
    // to give.
    m.game_rules.game_type = 1;
    m.game_rules.resource_level = 2;
    assert!(!pertinent(&m, &lib, Kind::Resources));
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::Resources(1), vec![]),
    )
    .unwrap();
    assert_eq!(m.kingdoms[0].resources, [70000; 4]);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_selection_turns_god_changes_kingdom_and_is_usurped_or_watched() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    let enemy = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [40, 40]).unwrap();
    sim.refresh_visibility(&m, None);
    // The objects' commands wait for the object clicked after them.
    assert!(pertinent(&m, &lib, Kind::God));
    // 0x4245e3: flag 0x200 on, then off, for each selected object.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::God, vec![1, 2]),
    )
    .unwrap();
    assert!(m.objects[1].flags & FLAG_GOD != 0 && m.objects[2].flags & FLAG_GOD != 0);
    issue(&mut sim, &mut m, &g, &lib, &c, &cheat(Cheat::God, vec![1])).unwrap();
    assert!(m.objects[1].flags & FLAG_GOD == 0 && m.objects[2].flags & FLAG_GOD != 0);
    // 0x424774: the player commands an enemy unit, then no more; its own
    // peasant it gives up.
    assert!(!m.commands(&m.objects[enemy]));
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::Usurp, vec![enemy]),
    )
    .unwrap();
    assert!(m.commands(&m.objects[enemy]));
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::Usurp, vec![enemy]),
    )
    .unwrap();
    assert!(!m.commands(&m.objects[enemy]));
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::Usurp, vec![3]),
    )
    .unwrap();
    assert_eq!(m.objects[3].control[0], 2);
    assert!(!m.commands(&m.objects[3]));
    // 0x4247fd: the player sees what the enemy unit sees (+0xa4 = 1).
    assert!(!sim.visible(&m.objects[enemy]));
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::Spy, vec![enemy]),
    )
    .unwrap();
    assert_eq!(m.objects[enemy].sight[0], 1);
    assert!(sim.visible(&m.objects[enemy]));
    // 0x42436e: the selected peasant goes to kingdom 2 (vt+0x274).
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::Kingdom(2), vec![2]),
    )
    .unwrap();
    assert_eq!(m.objects[2].owner, 2);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn instant_building_no_requirements_hunt_relations_and_discoveries() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    // 0x424404: a bit of kingdom 1's relation toward 2, set then cleared.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(
            Cheat::Relation {
                from: 1,
                to: 2,
                bit: 1,
                set: true,
            },
            vec![],
        ),
    )
    .unwrap();
    assert_eq!(m.diplomacy[1] & 2, 2);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(
            Cheat::Relation {
                from: 1,
                to: 2,
                bit: 1,
                set: false,
            },
            vec![],
        ),
    )
    .unwrap();
    assert_eq!(m.diplomacy[1] & 2, 0);
    // 0x494bbf: «notech» lets a forbidden unit be trained.
    m.rules[0].forbidden_units = vec!["peon".into()];
    assert!(
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("e_peon", vec![0])
        )
        .is_err()
    );
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::NoTech, vec![]),
    )
    .unwrap();
    assert!(m.cheats.no_tech);
    // 0x41f113: «buildozer» ends the step at its first tick.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::InstantBuild, vec![]),
    )
    .unwrap();
    let count = m.objects.len();
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("e_peon", vec![0]),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 3);
    assert_eq!(m.objects.len(), count + 1);
    // 0x4246ed: the units of kingdom 10 leave the map.
    let bear = new_unit(&mut sim, &mut m, &lib, 26, DISCORD_OWNER, 0, [50, 50]).unwrap();
    m.simulation = Some(sim.clone());
    assert!(pertinent(&m, &lib, Kind::Hunt));
    m.simulation = None;
    issue(&mut sim, &mut m, &g, &lib, &c, &cheat(Cheat::Hunt, vec![])).unwrap();
    assert!(m.objects[bear].flags & FLAG_INACTIVE != 0);
    // 0x424886: a research completed at once; an unknown name does nothing.
    let left = researches_left(&c, &lib, &m);
    let first = left.first().cloned().expect("a research to discover");
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &cheat(Cheat::Discover(first.clone()), vec![]),
    )
    .unwrap();
    assert!(m.rules[0].initial_researches.contains(&first));
    assert!(!researches_left(&c, &lib, &m).contains(&first));
    assert!(
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &cheat(Cheat::Discover("te_none".into()), vec![])
        )
        .is_err()
    );
    // «next» belongs to a campaign map whose scripts name one.
    assert!(next_map(&m).is_none() && !pertinent(&m, &lib, Kind::Next));
    m.campaign = true;
    m.script.actions.push(crate::script::Named {
        name: "fine".into(),
        value: crate::script::Action::LoadMap("CAMPAIGN\\NEXT.TZM".into()),
    });
    assert_eq!(next_map(&m), Some("CAMPAIGN\\NEXT.TZM"));
    assert!(pertinent(&m, &lib, Kind::Next));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn spawned_units_stand_where_clicked_and_buildings_come_built() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    let spawn_for = |class, civ, owner, cell| Request {
        cell: Some(cell),
        ..cheat(Cheat::Spawn { class, civ, owner }, vec![])
    };
    let spawn = |class, civ, cell, _: Vec<usize>| spawn_for(class, civ, 1, cell);
    // A unit on the cell clicked, of the player's kingdom.
    let count = m.objects.len();
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &spawn(3, 2, [30, 30], vec![]),
    )
    .unwrap();
    let footman = &m.objects[count];
    assert_eq!(
        (
            footman.class,
            footman.owner,
            footman.civilization,
            [footman.x, footman.y]
        ),
        (3, 1, 2, [30, 30])
    );
    // A building around the pointer, built at once at no price.
    let before = m.kingdoms[0].resources;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &spawn(45, 1, [40, 20], vec![]),
    )
    .unwrap();
    let barrack = &m.objects[count + 1];
    assert!(barrack.class == 45 && barrack.flags & FLAG_CONSTRUCTION == 0);
    assert_eq!(barrack.health, barrack.max_health);
    let [w, h] = footprint(&lib, barrack);
    assert_eq!(
        [barrack.x, barrack.y],
        [40 - (w / 2) as u16, 20 - (h / 2) as u16]
    );
    assert_eq!(m.kingdoms[0].resources, before);
    // The panel names the kingdom.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &spawn_for(4, 1, 2, [52, 52]),
    )
    .unwrap();
    assert_eq!(m.objects.last().unwrap().owner, 2);
    // A house counts for the population at once.
    let (_, housing, _) = population_parts(&m, &lib, 1);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &spawn(66, 1, [60, 20], vec![]),
    )
    .unwrap();
    assert!(population_parts(&m, &lib, 1).1 > housing);
    // A place its site test refuses puts nothing (on the castle).
    let count = m.objects.len();
    assert!(issue(&mut sim, &mut m, &g, &lib, &c, &spawn(3, 1, [6, 6], vec![])).is_err());
    assert_eq!(m.objects.len(), count);
}
