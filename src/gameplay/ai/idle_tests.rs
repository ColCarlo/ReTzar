//! A computer player's idle peasants and full cows against the native
//! routines (0x4153a0, 0x44f2f0, 0x4ecf6e), in the test world: kingdom 2 a
//! European computer player on a 64-cell map of grass.
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
/// `f` on kingdom 2's player and the world.
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
/// Twice the period, the idle time from which the peasant does more.
fn long(m: &MissionWorld) -> u32 {
    2 * think::period(m) as u32
}

#[test]
#[ignore = "Requires installed original archives"]
fn an_idle_peasant_takes_up_a_site_left_with_few_builders() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 50]).unwrap();
    let site = new_unit(&mut sim, &mut m, &lib, class::FARM, 2, 1, [30, 30]).unwrap();
    m.objects[site].flags |= FLAG_CONSTRUCTION;
    let a = &mut sim.gameplay.actors[site];
    (a.construction, a.construction_total) = (50, 100);
    let idle = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [40, 30]).unwrap();
    let busy: Vec<usize> = [[30, 36], [32, 36]]
        .into_iter()
        .map(|p| new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, p).unwrap())
        .collect();
    for &b in &busy {
        sim.gameplay.actors[b].intent = Some(Intent::Build(site));
    }
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        p.lists.sites = vec![site];
        assert_eq!(p.lists.groups[group::BUILDERS as usize].len(), 2);
        // 0x4153a0: two builders there are too many at first...
        assert!(!p.idle_peasant(w, idle, 0));
        assert_eq!(w.sim.gameplay.actors[idle].intent, None);
        // ...not after two periods idle.
        assert!(p.idle_peasant(w, idle, long(w.m)));
        assert_eq!(
            w.sim.gameplay.actors[idle].intent,
            Some(Intent::Build(site))
        );
        assert_eq!(p.lists.groups[group::BUILDERS as usize].len(), 3);
    });
}

#[test]
#[ignore = "Requires installed original archives"]
fn an_idle_peasant_takes_its_load_home_then_picks_up_items() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 50]).unwrap();
    let mill = new_unit(&mut sim, &mut m, &lib, class::LUMBER, 2, 1, [40, 20]).unwrap();
    let farm = new_unit(&mut sim, &mut m, &lib, class::FARM, 2, 1, [44, 34]).unwrap();
    let peasant = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [40, 30]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        // Wood goes to the mill, the nearer store of it.
        w.sim.gameplay.actors[peasant].cargo = [0, 0, 0, 10];
        assert!(p.idle_peasant(w, peasant, 0));
        assert_eq!(
            w.sim.gameplay.actors[peasant].intent,
            Some(Intent::Deliver(mill))
        );
        // Milk has no store but the castle, however near the farm is.
        think::restart(w.sim, peasant);
        let a = &mut w.sim.gameplay.actors[peasant];
        (a.cargo, a.cargo_source) = ([0, 0, 10, 0], class::COW);
        assert!(p.idle_peasant(w, peasant, 0));
        assert_eq!(
            w.sim.gameplay.actors[peasant].intent,
            Some(Intent::Deliver(castle))
        );
        let _ = farm;
        // Empty handed, a resource item within 16 is picked up.
        think::restart(w.sim, peasant);
        w.sim.gameplay.actors[peasant].cargo = [0; 4];
        let item = new_unit(w.sim, w.m, w.lib, class::RESITEM, 0, 0, [50, 30]).unwrap();
        assert!(p.idle_peasant(w, peasant, 0));
        assert_eq!(
            w.sim.gameplay.actors[peasant].intent,
            Some(Intent::Harvest(item))
        );
    });
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_long_idle_peasant_repairs_the_nearest_damaged_building() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 50]).unwrap();
    let far = new_unit(&mut sim, &mut m, &lib, class::FARM, 2, 1, [48, 30]).unwrap();
    let near = new_unit(&mut sim, &mut m, &lib, class::FARM, 2, 1, [30, 24]).unwrap();
    let enemy = new_unit(&mut sim, &mut m, &lib, class::FARM, 1, 1, [36, 34]).unwrap();
    for j in [far, near, enemy] {
        m.objects[j].health = m.objects[j].max_health / 2;
    }
    let peasant = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [36, 30]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        // Not before two periods idle.
        assert!(!p.idle_peasant(w, peasant, 0));
        // 0x44f2f0: the enemy's farm is not mended; of its own, the nearer
        // by the centres.
        assert!(p.idle_peasant(w, peasant, long(w.m)));
        assert_eq!(
            w.sim.gameplay.actors[peasant].intent,
            Some(Intent::Repair(near))
        );
    });
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_computers_full_cows_are_slaughtered() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 50]).unwrap();
    let cow = new_unit(&mut sim, &mut m, &lib, class::COW, 2, 1, [30, 30]).unwrap();
    let theirs = new_unit(&mut sim, &mut m, &lib, class::COW, 1, 1, [30, 40]).unwrap();
    for j in [cow, theirs] {
        sim.gameplay.meat.insert(j, 598);
    }
    for _ in 0..20 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    // 0x4ecf6e: at 600 the computer's cow dies and leaves its meat; the
    // human's keeps it.
    assert!(!alive(&m.objects[cow]));
    assert!(alive(&m.objects[theirs]));
    assert_eq!(sim.gameplay.meat[&theirs], 600);
}
