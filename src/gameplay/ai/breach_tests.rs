//! The computer's breaches against the native routines (0x4171e0,
//! 0x4dfb84, 0x417190), in the test world: kingdom 2 a computer player
//! without a home on a 64-cell map of grass, at war with the human
//! kingdom 1.
use super::*;
use crate::gameplay::ai::script::Library;
use crate::gameplay::tests::setup;

fn tick(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
) {
    advance(sim, m, g, lib, c, Duration::from_millis(TICK_MS), 10, false);
}
/// A script whose only rule holds: the Build phase does nothing.
fn quiet(sim: &mut Simulation, c: &Catalog) {
    let lib = Library::new(
        [(
            "data\\ai\\t.ais".to_owned(),
            b"rule have 1 castle end".to_vec(),
        )],
        c.ai.types.clone(),
    );
    sim.gameplay.ai.players.get_mut(&2).unwrap().program = Some(lib.compile("t.ais").unwrap());
}
/// The player's stone wall across the map at x = 30, but for the rows of
/// `open`, and a peasant of its behind it.
fn walled(
    open: std::ops::RangeInclusive<u16>,
) -> (
    Catalog,
    ObjectLibrary,
    MissionWorld,
    MapGrid,
    Simulation,
    Vec<Option<usize>>,
    usize,
) {
    let (c, lib, mut m, g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let walls = (0..64)
        .map(|y| {
            (!open.contains(&y)).then(|| {
                new_unit(&mut sim, &mut m, &lib, class::WALL_STONE, 1, 1, [30, y]).unwrap()
            })
        })
        .collect();
    let peasant = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 1, [45, 30]).unwrap();
    (c, lib, m, g, sim, walls, peasant)
}
fn marked(sim: &Simulation, m: &MissionWorld, walls: &[Option<usize>]) -> Vec<u16> {
    walls
        .iter()
        .flatten()
        .filter(|&&j| sim.gameplay.actors[j].marked)
        .map(|&j| m.objects[j].y)
        .collect()
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_wall_across_the_way_is_marked_and_attacked() {
    let (c, lib, mut m, mut g, mut sim, walls, peasant) = walled(1..=0);
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [20, 30]).unwrap();
    set_order(&mut sim, knight, Intent::Attack(peasant), false).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // No way round: the search through the walls crosses at the knight's
    // row; the hostile walls beside that cell are marked and the knight
    // attacks the one on it at priority 4000, its class the attack type.
    let a = &sim.gameplay.actors[knight];
    assert_eq!(a.intent, Some(Intent::Attack(walls[30].unwrap())));
    assert!(!a.automatic);
    assert_eq!(a.attack_type, Some(class::WALL_STONE));
    assert_eq!(marked(&sim, &m, &walls), [29, 30, 31]);
    // Another knight with a way blocked near the marks takes the marked
    // wall within 12 cells, marking nothing more.
    let other = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [20, 38]).unwrap();
    let far = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 1, [45, 38]).unwrap();
    set_order(&mut sim, other, Intent::Attack(far), false).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let Some(Intent::Attack(t)) = sim.gameplay.actors[other].intent else {
        panic!("{:?}", sim.gameplay.actors[other].intent);
    };
    assert!(sim.gameplay.actors[t].marked);
    assert_eq!(marked(&sim, &m, &walls), [29, 30, 31]);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_near_way_round_is_walked_and_marks_nothing() {
    let (c, lib, mut m, mut g, mut sim, walls, peasant) = walled(33..=35);
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [20, 30]).unwrap();
    set_order(&mut sim, knight, Intent::Attack(peasant), false).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // The way through the opening is shorter than twice the gap to the
    // target (0x417267): kept.
    assert_eq!(
        sim.gameplay.actors[knight].intent,
        Some(Intent::Attack(peasant))
    );
    assert!(marked(&sim, &m, &walls).is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_fallen_marked_wall_clears_the_marks_and_calms_the_attackers() {
    let (c, lib, mut m, mut g, mut sim, walls, peasant) = walled(1..=0);
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [20, 30]).unwrap();
    set_order(&mut sim, knight, Intent::Attack(peasant), false).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert_eq!(marked(&sim, &m, &walls), [29, 30, 31]);
    // Knights of the player ordered on its walls, one within 16 cells of
    // the marked wall, one beyond, in their sectors after a step.
    let near = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [24, 36]).unwrap();
    set_order(&mut sim, near, Intent::Attack(walls[36].unwrap()), false).unwrap();
    let beyond = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [24, 50]).unwrap();
    set_order(&mut sim, beyond, Intent::Attack(walls[50].unwrap()), false).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    kill(&mut sim, &mut m, &g, &lib, walls[30].unwrap());
    // 0x417190, as the wall leaves the map: every mark within 16 cells
    // goes, and the units there attacking a wall, whoever owns them, go
    // down to priority 3000.
    assert!(marked(&sim, &m, &walls).is_empty());
    assert!(sim.gameplay.actors[knight].automatic);
    assert!(sim.gameplay.actors[near].automatic);
    assert!(!sim.gameplay.actors[beyond].automatic);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_computer_striking_a_wall_calms_down_by_chance() {
    let (c, lib, mut m, mut g, mut sim, walls, _) = walled(1..=0);
    let wall = walls[30].unwrap();
    m.objects[wall].health = m.objects[wall].max_health;
    let mine = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [29, 30]).unwrap();
    let theirs = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [29, 40]).unwrap();
    set_order(&mut sim, mine, Intent::Attack(wall), false).unwrap();
    set_order(&mut sim, theirs, Intent::Attack(walls[40].unwrap()), false).unwrap();
    // 0x4dfc54: one chance in ten a blow; the player's knight is not the
    // computer's and keeps its order.
    let mut calm = None;
    for t in 0..4000 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
        if sim.gameplay.actors[mine].automatic {
            calm = Some(t);
            break;
        }
    }
    assert!(calm.is_some());
    assert_eq!(sim.gameplay.actors[mine].intent, Some(Intent::Attack(wall)));
    assert!(m.objects[wall].health < m.objects[wall].max_health);
    assert!(!sim.gameplay.actors[theirs].automatic);
    assert_eq!(
        sim.gameplay.actors[theirs].intent,
        Some(Intent::Attack(walls[40].unwrap()))
    );
}
