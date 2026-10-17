//! A computer player's sites against the native routines (0x401b30,
//! 0x44ffc0, 0x469530, 0x40a990, 0x40b610, 0x40cec0, 0x40b9e0, 0x40bab0), in
//! the test world: kingdom 2 a European computer player with 10000 of
//! everything on a 64-cell map of grass.
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
fn script(sim: &mut Simulation, c: &Catalog, text: &str) {
    let lib = Library::new(
        [("data\\ai\\t.ais".to_owned(), text.as_bytes().to_vec())],
        c.ai.types.clone(),
    );
    sim.gameplay.ai.players.get_mut(&2).unwrap().program = Some(lib.compile("t.ais").unwrap());
}
fn player(sim: &Simulation) -> &Player {
    &sim.gameplay.ai.players[&2]
}
fn cost(c: &Catalog, name: &str) -> [u32; 4] {
    c.stats(&super::super::tree::probe(0, 1), name, &[]).cost
}

#[test]
fn rects_are_apart_by_the_sides_their_centres_face() {
    // 0x401b30: each axis from the side the other's centre lies on; a
    // rect inside or across another is at 0.
    assert_eq!(rect_gap([0, 0, 2, 2], [6, 1, 7, 1]), 3);
    assert_eq!(rect_gap([6, 1, 7, 1], [0, 0, 2, 2]), 3);
    assert_eq!(rect_gap([0, 0, 2, 2], [4, 9, 5, 10]), 6);
    assert_eq!(rect_gap([0, 0, 9, 9], [3, 3, 4, 4]), 0);
}

#[test]
fn the_flood_spreads_by_rings_inside_its_bounds() {
    // 0x44ffc0: the start, its eight neighbours in their fixed order, then
    // the next ring; the ring markers count in the budget.
    let mut f = Flood::new([2, 2], [0, 0, 4, 4], 16);
    let mut seen = Vec::new();
    let (more, found) = f.run(10, |p, _| {
        seen.push(p);
        true
    });
    assert!(more && found.is_none());
    assert_eq!(
        seen,
        [
            [2, 2],
            [1, 1],
            [2, 1],
            [3, 1],
            [1, 2],
            [3, 2],
            [1, 3],
            [2, 3],
            [3, 3]
        ]
    );
    // It goes on where it stopped and tests each cell of its bounds once.
    let (more, _) = f.run(1000, |p, _| {
        seen.push(p);
        true
    });
    assert!(!more);
    assert_eq!(seen.len(), 25);
    // A cell that does not spread blocks the flood but the start's.
    let mut f = Flood::new([2, 2], [0, 0, 4, 4], 16);
    let mut n = 0;
    f.run(1000, |_, _| {
        n += 1;
        false
    });
    assert_eq!(n, 9);
    // The stopping cell ends it.
    let mut f = Flood::new([2, 2], [0, 0, 4, 4], 16);
    let out = f.run(1000, |p, stop| {
        *stop = p == [3, 1];
        true
    });
    assert_eq!(out, (false, Some([3, 1])));
}

#[test]
#[ignore = "Requires installed original archives"]
fn free_land_wants_one_more_column_than_the_rect() {
    let (_, lib, _, g, _) = setup(1);
    let mut cells = crate::gameplay::pathing::Cells::new(&g, lib.trunks);
    // 0x469530 over the runs of 0x469230: the cell right of the rect counts.
    assert!(free_land(&cells, [10, 10, 12, 11]));
    cells.set([13, 11], crate::gameplay::pathing::STATIC, true);
    assert!(!free_land(&cells, [10, 10, 12, 11]));
    assert!(free_land(&cells, [10, 10, 11, 11]));
    // The map's last column has nothing after it.
    assert!(!free_land(&cells, [62, 0, 63, 0]));
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_building_goes_round_the_home_and_its_builder_goes_at_once() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    script(&mut sim, &c, "rule have 1 farm end");
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [30, 30]).unwrap();
    // Beside where the flood from the home first meets a site (42, 24).
    let peasant = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [44, 30]).unwrap();
    let before = m.kingdoms[1].resources;
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // 0x40a990 round the home, the castle there the reference: a farm
    // site 5 to 50 cells from it, paid by the kingdom, its builder sent.
    let site = (castle..m.objects.len())
        .find(|&j| m.objects[j].class == class::FARM)
        .expect("a farm site");
    assert_ne!(m.objects[site].flags & FLAG_CONSTRUCTION, 0);
    let r = movement::footprint_rect(&lib, &m.objects[site]);
    let d = rect_gap(movement::footprint_rect(&lib, &m.objects[castle]), r);
    assert!((5..=50).contains(&d), "{d}");
    assert_eq!(
        sim.gameplay.actors[peasant].intent,
        Some(Intent::Build(site))
    );
    let farm = cost(&c, "farm");
    assert_eq!(
        m.kingdoms[1].resources,
        std::array::from_fn(|r| before[r] - farm[r])
    );
    // 0x40b300: the farm's margin of 2 is free land.
    let cells = movement::cells_now(&sim, &m, &g, &lib);
    let around = [r[0] - 2, r[1] - 2, r[2] + 2, r[3] + 2];
    for y in around[1]..=around[3] {
        for x in around[0]..=around[2] {
            let inside = x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3];
            if !inside {
                assert_eq!(cells.at([x, y]) & crate::gameplay::pathing::STATIC, 0);
            }
        }
    }
    assert!(sim.gameplay.ai.virtuals.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_far_builder_walks_to_a_site_still_to_be_laid() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    script(&mut sim, &c, "rule have 1 farm end");
    // Economy only: at war the Special phase would take the other free
    // peasant for the towers of the home (0x4104f0).
    sim.gameplay.ai.players.get_mut(&2).unwrap().mode = 1;
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [10, 10]).unwrap();
    // Two peasants far away: at the arrival 0x40a990 still wants a free one
    // besides the builder.
    new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [60, 61]).unwrap();
    let peasant = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [60, 60]).unwrap();
    let before = m.kingdoms[1].resources;
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // 0x40b610: farther than 10 the site is still to be laid: its price
    // committed, not paid; it counts with the farms and the builder walks.
    let farm = cost(&c, "farm").map(|n| n as i32);
    let p = player(&sim);
    assert_eq!(m.kingdoms[1].resources, before);
    assert_eq!(p.committed, farm);
    let Some(Intent::FarBuild(id)) = sim.gameplay.actors[peasant].intent else {
        panic!("{:?}", sim.gameplay.actors[peasant].intent);
    };
    assert!(sim.gameplay.ai.virtuals.contains_key(&id));
    assert_eq!(
        rules::record(&sim, &m, &c.ai, 2, class::FARM as u16),
        (1, 1)
    );
    // At 10 cells it is laid near where it was meant and paid; the
    // commitment is given back once the builder works on it.
    for _ in 0..1200 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
        if matches!(sim.gameplay.actors[peasant].intent, Some(Intent::Build(_))) {
            break;
        }
    }
    let Some(Intent::Build(site)) = sim.gameplay.actors[peasant].intent else {
        panic!("{:?}", sim.gameplay.actors[peasant].intent);
    };
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert_eq!(m.objects[site].class, class::FARM);
    assert!(sim.gameplay.ai.virtuals.is_empty());
    assert_eq!(player(&sim).committed, [0; 4]);
    assert_eq!(
        m.kingdoms[1].resources,
        std::array::from_fn(|r| before[r] - farm[r] as u32)
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn an_abandoned_site_is_taken_up_again() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    script(&mut sim, &c, "rule have 1 farm end");
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [30, 30]).unwrap();
    let site = new_unit(&mut sim, &mut m, &lib, class::FARM, 2, 1, [30, 40]).unwrap();
    m.objects[site].flags |= FLAG_CONSTRUCTION;
    let a = &mut sim.gameplay.actors[site];
    (a.construction, a.construction_total) = (50, 100);
    let peasant = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [36, 40]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // The site began is listed when its last builder leaves; the rule is
    // in progress meanwhile (a farm counts, site or not).
    assert_eq!(player(&sim).lists.sites, Vec::<usize>::new());
    sim.gameplay
        .ai
        .players
        .get_mut(&2)
        .unwrap()
        .lists
        .sites
        .push(site);
    let mut w = rules::World {
        sim: &mut sim,
        m: &mut m,
        g: &g,
        lib: &lib,
        c: &c,
        cells: None,
    };
    let mut p = std::mem::take(w.sim.gameplay.ai.players.get_mut(&2).unwrap());
    let placed = p.place(
        &mut w,
        class::FARM as u16,
        p.home.unwrap(),
        5,
        50,
        None,
        false,
    );
    assert_eq!(placed, Some(Site::Real(site)));
    assert_eq!(
        w.sim.gameplay.actors[peasant].intent,
        Some(Intent::Build(site))
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn orders_start_only_on_the_units_island() {
    let (c, lib, mut m, _, mut sim) = setup(1);
    // Two islands across a strip of water.
    let side = 64;
    let ground = (1 << 4) | (1 << 7);
    let g = MapGrid {
        name: "islands".into(),
        side,
        cells: (0..side * side)
            .map(|i| {
                if (28..36).contains(&(i % side)) {
                    0
                } else {
                    ground
                }
            })
            .collect(),
    };
    sim.gameplay.islands = islands::Islands::from_grid(&g, lib.trunks);
    let peasant = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [10, 10]).unwrap();
    let mut w = rules::World {
        sim: &mut sim,
        m: &mut m,
        g: &g,
        lib: &lib,
        c: &c,
        cells: None,
    };
    // 0x40bab0 → 0x415f80: across the water nothing starts without a ship;
    // the order stays kept.
    let mut p = std::mem::take(w.sim.gameplay.ai.players.get_mut(&2).unwrap());
    assert!(!order(
        &mut w,
        &mut p,
        peasant,
        Intent::Move([50, 10]),
        [50, 10],
        None
    ));
    assert_eq!(w.sim.gameplay.actors[peasant].intent, None);
    assert_eq!(
        w.sim.gameplay.actors[peasant].resume,
        Some(Intent::Move([50, 10]))
    );
    assert!(order(
        &mut w,
        &mut p,
        peasant,
        Intent::Move([20, 10]),
        [20, 10],
        None
    ));
    assert_eq!(
        w.sim.gameplay.actors[peasant].intent,
        Some(Intent::Move([20, 10]))
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_search_without_reference_measures_from_its_start() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    script(&mut sim, &c, "rule have 1 farm end");
    new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [58, 50]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let mut w = rules::World {
        sim: &mut sim,
        m: &mut m,
        g: &g,
        lib: &lib,
        c: &c,
        cells: None,
    };
    let mut p = std::mem::take(w.sim.gameplay.ai.players.get_mut(&2).unwrap());
    // 0x40a360 with no reference (no castle at the home): the distance from
    // the flood's start, here far from the map's corner.
    let start = [58, 58];
    let size = [3, 3];
    let mut s = Search {
        class: class::FARM as u16,
        size,
        min: 5,
        max: 50,
        from: [58, 50],
        flood: Flood::new(start, [0, 0, 63, 63], 64),
        ..Default::default()
    };
    let (_, found) = p.run(&mut w, &mut s, 20_000);
    let at = found.expect("a site");
    let d = records::gap(start, [at[0], at[1], at[0] + 2, at[1] + 2]);
    assert!((5..=50).contains(&d), "{at:?} {d}");
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_player_locked_by_needs_locked_still_lays_its_sites() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    script(&mut sim, &c, "rule have 1 castle end");
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [30, 30]).unwrap();
    new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [44, 30]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let mut w = rules::World {
        sim: &mut sim,
        m: &mut m,
        g: &g,
        lib: &lib,
        c: &c,
        cells: None,
    };
    let mut p = std::mem::take(w.sim.gameplay.ai.players.get_mut(&2).unwrap());
    // NEEDS_LOCKED holds back ships and defences (0x40b186), but the site
    // checks its price with no part (0xff at 0x40b67d).
    p.locked = true;
    let placed = p.place(
        &mut w,
        class::FARM as u16,
        p.home.unwrap(),
        5,
        50,
        None,
        false,
    );
    assert!(matches!(placed, Some(Site::Real(_))), "{placed:?}");
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_lock_of_needs_locked_lasts_while_its_rule_is_reached() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [30, 30]).unwrap();
    script(&mut sim, &c, "rule have needs_locked end");
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let compile = |text: &str| {
        Library::new(
            [("data\\ai\\t.ais".to_owned(), text.as_bytes().to_vec())],
            c.ai.types.clone(),
        )
        .compile("t.ais")
        .unwrap()
    };
    let mut w = rules::World {
        sim: &mut sim,
        m: &mut m,
        g: &g,
        lib: &lib,
        c: &c,
        cells: None,
    };
    let mut p = std::mem::take(w.sim.gameplay.ai.players.get_mut(&2).unwrap());
    let others = BTreeMap::new();
    p.program = Some(compile("rule have needs_locked end"));
    rules::build(&mut w, &mut p, 2, &others);
    assert!(p.locked);
    // The next Build phase clears the script's part with the lock, its
    // fifth word (0x408a22); the rule no longer reached, it stays off.
    p.program = Some(compile("rule have 1 castle end"));
    rules::build(&mut w, &mut p, 2, &others);
    assert!(!p.locked);
}
