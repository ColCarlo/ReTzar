//! The goals with their own routines against 0x409ab0, 0x409ed0, 0x409d30,
//! 0x409500 and 0x409290, in the test world: kingdom 2 a computer player on
//! a 64-cell map of grass.
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

#[test]
#[ignore = "Requires installed original archives"]
fn an_idle_mage_near_home_summons_beside_itself() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [28, 28]).unwrap();
    let mage = new_unit(&mut sim, &mut m, &lib, class::MAGE, 2, 1, [36, 30]).unwrap();
    if let Some(dep) = c.order_research("spell_spawnbat") {
        m.rules[1].initial_researches.push(dep.trim().to_owned());
    }
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        assert_eq!(p.lists.mages, [mage]);
        p.available = [10000; 4];
        p.buckets = [[0; 4]; 5];
        let before = p.available;
        let cost = rules::cost(w, p, 15);
        // 0x409ab0: the bat's price taken at once, the spell cast within
        // six rings of the mage.
        assert!(summon(w, p, 15));
        assert_eq!(p.available, std::array::from_fn(|r| before[r] - cost[r]));
        let Some(Intent::Cast { key, cell, target }) = &w.sim.gameplay.actors[mage].intent else {
            panic!("{:?}", w.sim.gameplay.actors[mage].intent);
        };
        assert_eq!((key.as_str(), *target), ("spell_spawnbat", None));
        let r = movement::footprint_rect(w.lib, &w.m.objects[mage]);
        let d = super::super::records::gap([cell[0] as i32, cell[1] as i32], r);
        assert!(d <= 6, "{d}");
        // Not a creature of a mage, and no mage at all: nothing.
        assert!(!summon(w, p, 17));
        p.lists.mages.clear();
        assert!(!summon(w, p, 15));
    });
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_european_player_lays_the_crusade_beside_its_cathedral() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 50]).unwrap();
    let cathedral = new_unit(&mut sim, &mut m, &lib, class::CATHEDRAL, 2, 1, [30, 30]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        p.available = [10000; 4];
        p.buckets = [[0; 4]; 5];
        // 0x409d30: the first ring round the cathedral, from its top left.
        assert!(crusade(w, p));
        let t = &w.sim.gameplay.treasury[&2];
        let anchor = t.crusade.as_ref().expect("a crusade").anchor;
        let o = &w.m.objects[cathedral];
        assert_eq!(
            [w.m.objects[anchor].x, w.m.objects[anchor].y],
            [o.x - 1, o.y - 1]
        );
        assert_eq!(t.crusaders, None);
        // One crusade at a time: no crusaders gathered now.
        assert!(!crusade(w, p));
    });
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_idlest_peasant_where_fighters_are_short_becomes_a_jihad_warrior() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    m.kingdoms[1].civilization = 2;
    sim.gameplay.ai.players.get_mut(&2).unwrap().civilization = 2;
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 2, [28, 28]).unwrap();
    let mine = new_unit(&mut sim, &mut m, &lib, class::RES_GOLD, 0, 0, [36, 26]).unwrap();
    m.objects[mine].data.resize(64, 0);
    m.objects[mine].data[58..62].copy_from_slice(&5000u32.to_le_bytes());
    let miner = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 2, [36, 30]).unwrap();
    let idle = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 2, [37, 31]).unwrap();
    set_order(&mut sim, miner, Intent::Harvest(mine), false).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        // 0x409290: in the home's sector, short of its minimum, the idle
        // one scores 3/2 of it and the miner 2/3.
        assert!(score(w, p, idle) > score(w, p, miner));
        assert!(jihad(w, p));
        assert_eq!(w.m.objects[idle].class, JIHAD_WARRIOR as usize);
        assert_eq!(w.m.objects[miner].class, class::PEON);
        // The lists learn it at once: one peasant less, a fighter more.
        assert!(!p.lists.peasants.contains(&idle));
        assert!(p.lists.army.contains(&idle));
        // Only an Arabian player has jihad warriors.
        p.civilization = 1;
        assert!(!jihad(w, p));
    });
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_jihad_warrior_comes_new_without_the_peasants_work() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    m.kingdoms[1].civilization = 2;
    sim.gameplay.ai.players.get_mut(&2).unwrap().civilization = 2;
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 2, [28, 28]).unwrap();
    let mine = new_unit(&mut sim, &mut m, &lib, class::RES_GOLD, 0, 0, [36, 26]).unwrap();
    m.objects[mine].data.resize(64, 0);
    m.objects[mine].data[58..62].copy_from_slice(&5000u32.to_le_bytes());
    let miner = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 2, [36, 30]).unwrap();
    set_order(&mut sim, miner, Intent::Harvest(mine), false).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    sim.gameplay.actors[miner].cargo = [10, 0, 0, 0];
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| assert!(jihad(w, p)));
    // Op 0x5d (0x4ac74e): the peasant goes and a warrior comes from its
    // record (0x4fd4c0), with neither its program nor its load.
    let a = &sim.gameplay.actors[miner];
    assert_eq!(m.objects[miner].class, JIHAD_WARRIOR as usize);
    assert_eq!((a.intent.clone(), a.cargo), (None, [0; 4]));
    let gold = m.kingdoms[1].resources[0];
    for _ in 0..600 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    assert_eq!(m.kingdoms[1].resources[0], gold);
    assert!(!matches!(
        sim.gameplay.actors[miner].intent,
        Some(Intent::Harvest(_))
    ));
}
