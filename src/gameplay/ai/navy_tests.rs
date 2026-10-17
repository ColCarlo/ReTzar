//! A computer player's ships, fishing boats and harbors against the native
//! routines (0x46a730, 0x46a9b0, 0x4168d0, 0x4164c0, 0x415730, 0x40a710),
//! on an island of the test world: land at 20–29 in both directions, a ring
//! of shore round it whose edges turn round the island (the shapes of
//! 0x58a1f0), water elsewhere; kingdom 2 a computer player living there.
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
const RING: [i32; 2] = [19, 30];
/// Water, then the ring of shore with the shape of each edge (up 0, right
/// 1, down 3, left 7), then the land.
fn island(g: &mut MapGrid) {
    for y in 0..64 {
        for x in 0..64 {
            let on = |v: i32| v == RING[0] || v == RING[1];
            let inside = |v: i32| (RING[0]..=RING[1]).contains(&v);
            let tile: u16 = if (20..=29).contains(&x) && (20..=29).contains(&y) {
                0x90
            } else if inside(x) && inside(y) && (on(x) || on(y)) {
                let shape = if y == RING[0] && x < RING[1] {
                    1
                } else if x == RING[1] && y < RING[1] {
                    3
                } else if y == RING[1] && x > RING[0] {
                    7
                } else {
                    0
                };
                0x10 | shape
            } else {
                0
            };
            g.cells[y as usize * 64 + x as usize] = tile;
        }
    }
}
fn world() -> (Catalog, ObjectLibrary, MissionWorld, MapGrid, Simulation) {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    island(&mut g);
    // The player's own objects of the test world stand in the water.
    m.objects.clear();
    sim.objects.clear();
    sim.gameplay.actors.clear();
    quiet(&mut sim, &c);
    (c, lib, m, g, sim)
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_coast_of_an_island_is_walked_from_one_point() {
    let (_, lib, _, g, _) = world();
    let isl = islands::Islands::from_grid(&g, lib.trunks);
    // Regions in row order: the water from the corner (1), then the land
    // (2), which takes the ring its flood meets last.
    assert_eq!(isl.region([0, 0]), 1);
    assert_eq!(isl.region([25, 25]), 2);
    assert_eq!(isl.region([19, 19]), 2);
    // 0x46a730: the first shore cell the flood of the land met is the
    // corner up and left of where it started; the walk goes round from it.
    let first = isl.regions[2].coast.unwrap() as usize;
    assert_eq!(&isl.coasts[first..first + 2], &[[19, 19], [0xffff, 0xffff]]);
    let first = isl.regions[1].coast.unwrap() as usize;
    assert_eq!(isl.coasts[first + 1], [0xffff, 0xffff]);
    // 0x46a9b0 later: round the ring of 44 cells and back.
    let mut at = [19, 19];
    let mut steps = 0;
    while isl.coast_step(&g, 2, &mut at, false) {
        steps += 1;
        if at == [19, 19] {
            break;
        }
    }
    assert_eq!(steps, 44);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_harbor_goes_on_the_coast_facing_the_home() {
    let (c, lib, mut m, mut g, mut sim) = world();
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [24, 21]).unwrap();
    for x in [21, 22, 23] {
        new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [x, 28]).unwrap();
    }
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    let (spot, expected) = on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        assert_eq!(p.island, 2);
        w.sim.gameplay.ai.harbor_water = 1;
        let home = p.home.unwrap();
        let spot = p.harbor_spot(w, class::HARBOR as u16, home);
        // The ring in the order of its walk, from the point of the coast.
        let mut ring = Vec::new();
        ring.extend((19..30).map(|x| [x, 19]));
        ring.extend((19..30).map(|y| [30, y]));
        ring.extend((20..=30).rev().map(|x| [x, 30]));
        ring.extend((20..=30).rev().map(|y| [19, y]));
        let mut probe = tree::probe(class::HARBOR, p.civilization);
        probe.owner = 2;
        let [wd, ht] = footprint(w.lib, &probe).map(|n| n as i32);
        let mut best: Option<([i32; 2], u32)> = None;
        for at in ring {
            let score = records::gap(home, [at[0], at[1], at[0], at[1]]);
            for d in [[0, 0], [wd - 1, 0], [wd - 1, ht - 1], [0, ht - 1]] {
                let q = [at[0] + d[0], at[1] + d[1]];
                (probe.x, probe.y) = (q[0] as u16, q[1] as u16);
                if site_test(w.sim, w.m, w.g, w.lib, &probe) {
                    if build::safe(w, p, q[0], q[1], [wd, ht])
                        && best.is_none_or(|(_, b)| score < b)
                    {
                        best = Some((q, score));
                    }
                    break;
                }
            }
        }
        (spot, best.map(|(q, _)| q))
    });
    // 0x40a710: the water it was for is taken; the site is the first of
    // least gap to the home along the walk of the coast, the footprint
    // tried with its corners from the cell to the right and down.
    assert_eq!(sim.gameplay.ai.harbor_water, 0);
    assert!(expected.is_some());
    assert_eq!(spot, expected);
    let [x, y] = spot.unwrap();
    let mut probe = tree::probe(class::HARBOR, 1);
    (probe.x, probe.y, probe.owner) = (x as u16, y as u16, 2);
    assert!(site_test(&sim, &m, &g, &lib, &probe));
}

#[test]
#[ignore = "Requires installed original archives"]
fn ships_wanted_on_a_water_without_harbor_ask_for_one_and_it_is_built() {
    let (c, lib, mut m, mut g, mut sim) = world();
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [24, 21]).unwrap();
    for x in [21, 22, 23] {
        new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [x, 28]).unwrap();
    }
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        // The home on a small island already wants a ship on the water.
        assert_eq!(p.wanted_ships.get(&1), Some(&1));
        p.navy(w)
    });
    // 0x4168d0: no harbor there, so the water wants one; the harbor is
    // carried out for it at once, its site on the coast.
    let p = &sim.gameplay.ai.players[&2];
    assert_eq!(p.wanted_harbors.get(&1), Some(&1));
    let sites = m
        .objects
        .iter()
        .filter(|o| o.class == class::HARBOR && o.owner == 2)
        .count()
        + sim
            .gameplay
            .ai
            .virtuals
            .values()
            .filter(|v| v.class as usize == class::HARBOR)
            .count();
    assert_eq!(sites, 1);
    assert_eq!(sim.gameplay.ai.harbor_water, 0);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_harbor_makes_ships_and_fishing_boats_without_paying() {
    let (c, lib, mut m, mut g, mut sim) = world();
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [24, 21]).unwrap();
    for x in [21, 22, 23] {
        new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [x, 28]).unwrap();
    }
    let size = footprint(&lib, &tree::probe(class::HARBOR, 1)).map(|n| n as u16);
    let harbor = new_unit(
        &mut sim,
        &mut m,
        &lib,
        class::HARBOR,
        2,
        1,
        [22, 20 - size[1]],
    )
    .unwrap();
    let patch = new_unit(&mut sim, &mut m, &lib, class::FISHPATCH, 0, 1, [24, 10]).unwrap();
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    let before = m.kingdoms[1].resources;
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        assert_eq!(p.lists.harbors.get(&1).map(Vec::len), Some(1));
        p.navy(w)
    });
    // 0x416a62: the ship wanted is made by the idle harbor, with the
    // program Create alone: nothing is paid.
    let jobs: Vec<JobKind> = sim.gameplay.actors[harbor]
        .jobs
        .iter()
        .map(|j| j.kind.clone())
        .collect();
    assert!(matches!(jobs.first(), Some(JobKind::Train(c)) if [23, 24, 25].contains(c)));
    assert_eq!(m.kingdoms[1].resources, before);
    // The harbor busy, no fishing boat this pass; idle again, the patch
    // near it gets one (0x416b62), still unpaid.
    assert_eq!(jobs.len(), 1);
    sim.gameplay.actors[harbor].jobs.clear();
    sim.gameplay
        .ai
        .players
        .get_mut(&2)
        .unwrap()
        .wanted_ships
        .clear();
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| p.navy(w));
    assert!(matches!(
        sim.gameplay.actors[harbor].jobs.front(),
        Some(Job { kind: JobKind::Train(c), .. }) if *c == class::FISHING_BOAT
    ));
    assert_eq!(m.kingdoms[1].resources, before);
    // 0x415730: an idle boat of the player fishes that patch.
    sim.gameplay.actors[harbor].jobs.clear();
    let boat = new_unit(&mut sim, &mut m, &lib, class::FISHING_BOAT, 2, 1, [24, 12]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let fished = on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        p.idle_fishing(w, boat)
    });
    assert!(fished);
    assert_eq!(
        sim.gameplay.actors[boat].intent,
        Some(Intent::Harvest(patch))
    );
}
