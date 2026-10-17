//! A computer player's Special phase against the native routines
//! (0x409220, 0x4107d0, 0x410b60, 0x4104f0), in the test world: kingdom 2
//! a computer player at war with the human kingdom 1 on a 64-cell map of
//! grass.
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
fn on_player<R>(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    f: impl FnOnce(&mut Player, &mut World) -> R,
) -> R {
    let mut w = World {
        sim,
        m,
        g,
        lib,
        c,
        cells: None,
    };
    let mut p = std::mem::take(w.sim.gameplay.ai.players.get_mut(&2).unwrap());
    let out = f(&mut p, &mut w);
    *w.sim.gameplay.ai.players.get_mut(&2).unwrap() = p;
    out
}
fn upgrade(sim: &Simulation, i: usize) -> Option<String> {
    match sim.gameplay.actors[i].jobs.front() {
        Some(Job {
            kind: JobKind::Upgrade(n, _),
            ..
        }) => Some(n.clone()),
        _ => None,
    }
}
/// Sites of the class the player put up, laid or still to be laid.
fn sites(sim: &Simulation, m: &MissionWorld, class: usize) -> usize {
    let laid = m
        .objects
        .iter()
        .filter(|o| o.owner == 2 && o.class == class && o.flags & FLAG_CONSTRUCTION != 0)
        .count();
    let virtuals = sim
        .gameplay
        .ai
        .virtuals
        .values()
        .filter(|v| v.owner == 2 && v.class as usize == class)
        .count();
    laid + virtuals
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_wooden_tower_is_raised_one_a_pass() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 8]).unwrap();
    let towers: Vec<usize> = [[20, 8], [24, 8]]
        .into_iter()
        .map(|p| new_unit(&mut sim, &mut m, &lib, class::TOWER_WOOD, 2, 1, p).unwrap())
        .collect();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let before = m.kingdoms[1].resources;
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        assert_eq!(p.lists.towers.len(), 2);
        p.special(w)
    });
    // 0x4107d0: the first tower of the list is paid at once and the pass
    // ends.
    let raised: Vec<usize> = towers
        .iter()
        .copied()
        .filter(|&t| upgrade(&sim, t).as_deref() == Some("to_wtower2"))
        .collect();
    assert_eq!(raised.len(), 1);
    assert!(
        m.kingdoms[1]
            .resources
            .iter()
            .zip(before)
            .any(|(a, b)| *a < b)
    );
    // The next pass the other one, the first raising already.
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| p.special(w));
    for &t in &towers {
        assert_eq!(upgrade(&sim, t).as_deref(), Some("to_wtower2"));
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn an_upgrade_it_cannot_pay_is_booked_once() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 8]).unwrap();
    let towers: Vec<usize> = [[20, 8], [24, 8]]
        .into_iter()
        .map(|p| new_unit(&mut sim, &mut m, &lib, class::TOWER_WOOD, 2, 1, p).unwrap())
        .collect();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    m.kingdoms[1].resources = [0; 4];
    let booked = on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        p.special(w);
        let up = rules::cost(w, p, w.c.ai.kind("to_wtower2").unwrap());
        let tower = rules::cost(w, p, class::TOWER_WOOD as u16);
        let both: [i32; 4] = std::array::from_fn(|r| up[r] + tower[r]);
        (p.buckets[rules::bucket::DEFENCE], both, up)
    });
    // One kind of upgrade, looked at once a pass, booked once; and the
    // wooden tower the home asks for (0x4104f0) booked too.
    assert_eq!(booked.0, booked.1);
    assert!(booked.2.iter().any(|&n| n > 0));
    for &t in &towers {
        assert_eq!(upgrade(&sim, t), None);
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_home_asks_for_towers_then_barracks() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [20, 20]).unwrap();
    for x in [14, 15, 16, 17] {
        new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [x, 30]).unwrap();
    }
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    // 0x4104f0: the player's home (flags 7) has no tower: one goes up.
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| p.special(w));
    assert_eq!(sites(&sim, &m, class::TOWER_WOOD), 1);
    assert_eq!(sites(&sim, &m, class::BARRACK), 0);
    // With five towers there, and a barracks in the kingdom, a second
    // barracks.
    let (c2, lib2, mut m2, mut g2, mut sim2) = setup(1);
    quiet(&mut sim2, &c2);
    new_unit(&mut sim2, &mut m2, &lib2, class::CASTLE, 2, 1, [20, 20]).unwrap();
    for x in [14, 15, 16, 17] {
        new_unit(&mut sim2, &mut m2, &lib2, class::PEON, 2, 1, [x, 30]).unwrap();
    }
    for p in [[12, 12], [28, 12], [12, 28], [28, 28], [20, 12]] {
        new_unit(&mut sim2, &mut m2, &lib2, 39, 2, 1, p).unwrap();
    }
    new_unit(&mut sim2, &mut m2, &lib2, class::BARRACK, 2, 1, [30, 20]).unwrap();
    for _ in 0..2 {
        tick(&mut sim2, &mut m2, &mut g2, &lib2, &c2);
    }
    on_player(&mut sim2, &mut m2, &g2, &lib2, &c2, |p, w| p.special(w));
    assert_eq!(sites(&sim2, &m2, class::TOWER_WOOD), 0);
    assert_eq!(sites(&sim2, &m2, class::BARRACK), 1);
}
