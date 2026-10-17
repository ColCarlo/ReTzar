//! Gates against the original: automatism from the constructor, the
//! drawbridge's rows and pace, the pause after an order and the checks of
//! the orders (0x455da0, 0x456080, 0x456fd0, 0x4571a0, 0x4573dc).

use super::tests::setup;
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
fn order(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    gate: usize,
    name: &str,
) {
    issue(sim, m, g, lib, c, &Request::command(name, vec![gate])).unwrap();
}
fn finished_gate(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    at: [u16; 2],
) -> usize {
    let k = new_unit(sim, m, lib, class::GATE_WOOD, 1, 1, at).unwrap();
    m.objects[k].flags &= !FLAG_CONSTRUCTION;
    m.objects[k].data[56] = 0;
    k
}

#[test]
fn a_still_drawbridge_rests_raised_on_row_zero_and_lowered_on_row_six() {
    // 0x456080: state 1 (raised, the passage closed) row 0, state 0 row 6.
    let mut o = MapObject {
        class: class::GATE_WOOD,
        owner: 1,
        civilization: 1,
        x: 0,
        y: 0,
        id: 1,
        variant: 0,
        flags: 0,
        health: 1,
        max_health: 1,
        name: String::new(),
        subtitle: String::new(),
        portrait: String::new(),
        portrait_large: String::new(),
        custom_image: String::new(),
        data: vec![0; 77],
        mole: 0,
        control: [0; 8],
        sight: [0; 8],
    };
    assert_eq!(drawbridge_row(None, 0, &o), 6);
    o.data[57] = 1;
    assert_eq!(drawbridge_row(None, 0, &o), 0);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn map_gates_start_automatic() {
    // The constructor sets +0x15e (0x455da0) and the map record leaves it.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let catalog = crate::scenario::ScenarioCatalog::load(root).unwrap();
    let (_, index) = catalog.locate("mp4.wmp::maps/mp4").unwrap();
    let scenario = &catalog.missions[index];
    let c = scenario.container().unwrap();
    let grid = MapGrid::read(&c, &scenario.header).unwrap();
    let m = MissionWorld::read(&c, &grid).unwrap();
    let gates: Vec<_> = m
        .objects
        .iter()
        .filter(|o| matches!(o.class, class::GATE_WOOD | class::GATE_STONE))
        .collect();
    assert_eq!(gates.len(), 8);
    assert!(gates.iter().all(|o| o.byte(65) == 1 && o.byte(57) == 0));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_drawbridge_moves_a_row_every_100_ms() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let gate = finished_gate(&mut sim, &mut m, &lib, [30, 30]);
    order(&mut sim, &mut m, &g, &lib, &c, gate, "gate_subai_off");
    // Raising: the passage closes at once, the bridge climbs 6 → 0.
    order(&mut sim, &mut m, &g, &lib, &c, gate, "gateopen");
    assert_eq!(m.objects[gate].byte(57), 1);
    assert_eq!(drawbridge_row(Some(&sim), gate, &m.objects[gate]), 6);
    assert!(
        sim.sounds
            .iter()
            .any(|e| e.property == "@gate/gate open.wav")
    );
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    assert_eq!(drawbridge_row(Some(&sim), gate, &m.objects[gate]), 5);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 10);
    assert_eq!(drawbridge_row(Some(&sim), gate, &m.objects[gate]), 0);
    assert!(sim.gameplay.actors[gate].gate_motion.is_some());
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    assert_eq!(sim.gameplay.actors[gate].gate_motion, None);
    assert_eq!(drawbridge_row(Some(&sim), gate, &m.objects[gate]), 0);
    // Lowering: 0 → 6 in the same pace, with its own sound.
    sim.sounds.clear();
    order(&mut sim, &mut m, &g, &lib, &c, gate, "gateclose");
    assert_eq!(m.objects[gate].byte(57), 0);
    assert!(
        sim.sounds
            .iter()
            .any(|e| e.property == "@gate/gate close.wav")
    );
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 6);
    assert_eq!(drawbridge_row(Some(&sim), gate, &m.objects[gate]), 3);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 8);
    assert_eq!(sim.gameplay.actors[gate].gate_motion, None);
    assert_eq!(drawbridge_row(Some(&sim), gate, &m.objects[gate]), 6);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn an_order_keeps_the_automatism_away_for_ten_seconds() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let gate = finished_gate(&mut sim, &mut m, &lib, [30, 30]);
    assert_eq!(m.objects[gate].byte(65), 1);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 10);
    // Raised by hand with nobody around: the automatism would lower it, but
    // waits 20 of its 500 ms steps (+0x166 = 20).
    order(&mut sim, &mut m, &g, &lib, &c, gate, "gateopen");
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 190);
    assert_eq!(m.objects[gate].byte(57), 1);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 20);
    assert_eq!(m.objects[gate].byte(57), 0);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn orders_wait_for_a_clear_passage() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let gate = finished_gate(&mut sim, &mut m, &lib, [30, 30]);
    order(&mut sim, &mut m, &g, &lib, &c, gate, "gate_subai_off");
    // Passage [32..34, 30..32]: a unit in front keeps it from rising
    // (0x4574e0) and, once raised, from coming down on it (0x457560).
    let unit = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [33, 32]).unwrap();
    sim.gameplay.actors[unit].stance = 2;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    order(&mut sim, &mut m, &g, &lib, &c, gate, "gateopen");
    assert_eq!(m.objects[gate].byte(57), 0);
    position(&mut m.objects[unit], [40, 40]);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    order(&mut sim, &mut m, &g, &lib, &c, gate, "gateopen");
    assert_eq!(m.objects[gate].byte(57), 1);
    position(&mut m.objects[unit], [33, 32]);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    order(&mut sim, &mut m, &g, &lib, &c, gate, "gateclose");
    assert_eq!(m.objects[gate].byte(57), 1);
    assert!(alive(&m.objects[unit]));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_automatism_lowers_the_bridge_on_what_it_does_not_see() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let gate = finished_gate(&mut sim, &mut m, &lib, [30, 30]);
    order(&mut sim, &mut m, &g, &lib, &c, gate, "gate_subai_off");
    order(&mut sim, &mut m, &g, &lib, &c, gate, "gateopen");
    // A friend standing in front of the raised gate keeps it up (0x4572ed).
    let friend = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [33, 32]).unwrap();
    sim.gameplay.actors[friend].stance = 2;
    order(&mut sim, &mut m, &g, &lib, &c, gate, "gate_subai_on");
    sim.gameplay.actors[gate].gate_hold = 0;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 20);
    assert_eq!(m.objects[gate].byte(57), 1);
    // An invisible stranger is not seen: once the friend leaves, the bridge
    // comes down and kills it (0x4575d0, program Die).
    position(&mut m.objects[friend], [60, 60]);
    let ghost = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [32, 32]).unwrap();
    sim.gameplay.actors[ghost].stance = 2;
    sim.gameplay.actors[ghost].effect_until = u64::MAX;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 20);
    assert_eq!(m.objects[gate].byte(57), 0);
    assert!(!alive(&m.objects[ghost]) || m.objects[ghost].flags & FLAG_DYING != 0);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_gate_counts_its_500_ms_from_when_it_came_into_service() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let gate = finished_gate(&mut sim, &mut m, &lib, [30, 30]);
    // Automatism off: the hold only counts down.
    m.objects[gate].data[65] = 0;
    // 0x456173: finished at tick 3, the timer runs at 13, 23, …
    sim.gameplay.actors[gate].gate_since = 3;
    sim.gameplay.actors[gate].gate_hold = 2;
    let hold = |sim: &Simulation| sim.gameplay.actors[gate].gate_hold;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 12);
    assert_eq!(hold(&sim), 2);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(hold(&sim), 1);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 9);
    assert_eq!(hold(&sim), 1);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(hold(&sim), 0);
}
