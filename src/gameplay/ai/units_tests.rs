//! The engine's hooks on a computer player's units against the native
//! routines (0x4130a0, vt+0x2b0 0x4ae580 and 0x4ebeb0, 0x4e4e90,
//! 0x4dfb84), in the test world: kingdom 2 a computer player at war with
//! the human kingdom 1, whose castle and peasants stand at the top left.
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
    let p = sim.gameplay.ai.players.get_mut(&2).unwrap();
    p.program = Some(lib.compile("t.ais").unwrap());
    // Economy only: no group goes anywhere.
    p.mode = 1;
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_computer_unit_looks_over_the_threatened_place_of_its_sector() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [40, 40]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [42, 38]).unwrap();
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    // An enemy in the home place, far beyond the knight's sight.
    let rect = {
        let mgr = sim.gameplay.ai.manager.as_ref().unwrap();
        let slot = (0..mgr.places.len())
            .find(|&s| mgr.place(s).is_some_and(|p| p.anchor == Some(castle)))
            .unwrap();
        mgr.place(slot).unwrap().rect
    };
    let far = [rect[0] as u16 + 1, rect[3] as u16 - 1];
    let enemy = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, far).unwrap();
    let sight = sim.objects[knight].sight as i32;
    let gap = (far[0] as i32 - 42).abs().max((far[1] as i32 - 38).abs());
    assert!(gap > sight + 1);
    assert_eq!(find_target(&mut sim, &m, &lib, &c, knight), None);
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    // 0x4130a0: the place, threatened, is looked over whole.
    // The records evaluated again, as the phases of War and Special do
    // (0x40fdd0).
    let mut p = std::mem::take(sim.gameplay.ai.players.get_mut(&2).unwrap());
    let mgr = sim.gameplay.ai.manager.as_ref().unwrap();
    for at in 0..p.records.len() {
        records::evaluate(&mut p, 2, at, mgr, &m, mgr.side);
    }
    *sim.gameplay.ai.players.get_mut(&2).unwrap() = p;
    sim.gameplay.actors[knight].intent = None;
    assert_eq!(find_target(&mut sim, &m, &lib, &c, knight), Some(enemy));
}

#[test]
#[ignore = "Requires installed original archives"]
fn with_nobody_around_a_computer_unit_trains_on_its_dummy() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [40, 40]).unwrap();
    new_unit(&mut sim, &mut m, &lib, class::ACADEMY, 2, 1, [50, 50]).unwrap();
    let dummy = new_unit(&mut sim, &mut m, &lib, class::DUMMY, 2, 1, [42, 22]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [42, 36]).unwrap();
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    // Beyond its sight but within 20 more, its kingdom's dummy.
    let sight = sim.objects[knight].sight as i32;
    assert!(36 - 22 > sight + 1);
    assert_eq!(find_target(&mut sim, &m, &lib, &c, knight), Some(dummy));
    // At level 8 no more.
    sim.objects[knight].level = 8;
    assert_eq!(find_target(&mut sim, &m, &lib, &c, knight), None);
}

#[test]
#[ignore = "Requires installed original archives"]
fn computer_peasants_capture_and_ninjas_murder() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let peasant = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [30, 30]).unwrap();
    let ninja = new_unit(&mut sim, &mut m, &lib, class::NINJA, 2, 1, [32, 30]).unwrap();
    let tower = new_unit(&mut sim, &mut m, &lib, 40, 1, 1, [36, 30]).unwrap();
    let wall = new_unit(&mut sim, &mut m, &lib, class::WALL_STONE, 1, 1, [36, 34]).unwrap();
    let mage = new_unit(&mut sim, &mut m, &lib, class::MAGE, 1, 1, [36, 38]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // vt+0x2b0, 0x4ae580: a building that answers 0 to 0x271e is captured,
    // a wall attacked.
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, peasant, tower),
        Intent::Capture(tower)
    );
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, peasant, wall),
        Intent::Attack(wall)
    );
    // 0x4ebeb0: with 60 mana the ninja murders a mage, below it attacks.
    sim.objects[ninja].mana = 60;
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, ninja, mage),
        Intent::Murder(mage)
    );
    sim.objects[ninja].mana = 59;
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, ninja, mage),
        Intent::Attack(mage)
    );
    // The player's units attack.
    let human = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 1, [20, 20]).unwrap();
    assert_eq!(
        program_against(&mut sim, &m, &lib, &c, human, tower),
        Intent::Attack(tower)
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn after_an_order_a_computer_unit_waits_for_what_is_out_of_reach() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [52, 52]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [30, 40]).unwrap();
    let enemy = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [30, 45]).unwrap();
    sim.gameplay.actors[enemy].stance = 2;
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // 0x4e4fd5: a program of priority 4000 just ended, the enemy beyond
    // its reach: the computer's knight does not go for it.
    let a = &mut sim.gameplay.actors[knight];
    (a.intent, a.last_priority, a.idle_ms) = (None, 4000, 0);
    assert!(!scan(&mut sim, &m, &lib, &c, knight));
    assert_eq!(sim.gameplay.actors[knight].intent, None);
    // Idle 3000 ms on, it does.
    sim.gameplay.actors[knight].idle_ms = 3000;
    assert!(!scan(&mut sim, &m, &lib, &c, knight));
    assert_eq!(
        sim.gameplay.actors[knight].intent,
        Some(Intent::Attack(enemy))
    );
    // With nobody around the computer's idle takes over (0x4e4fc9).
    let a = &mut sim.gameplay.actors[knight];
    a.intent = None;
    m.objects[enemy].health = 0;
    assert!(scan(&mut sim, &m, &lib, &c, knight));
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_healed_unit_leaves_the_castle_a_wounded_one_stays() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [40, 40]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [38, 40]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    garrison(&mut sim, &mut m, castle, knight);
    m.objects[knight].health = m.objects[knight].max_health - 1;
    // 0x4140b0 inside a castle: wounded it stays.
    idle::unit(&mut sim, &mut m, &g, &lib, &c, knight, 0);
    assert_eq!(sim.objects[knight].holder, Some(castle));
    // Healed, it steps off at its next idle tick, which a unit inside a
    // holder still has.
    m.objects[knight].health = m.objects[knight].max_health;
    sim.gameplay.actors[knight].intent = None;
    for _ in 0..80 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    assert_eq!(sim.objects[knight].holder, None);
}

#[test]
#[ignore = "Requires installed original archives"]
fn an_idle_computer_fighter_picks_up_items_and_wounded_takes_refuge() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [40, 40]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [44, 30]).unwrap();
    let item = new_unit(&mut sim, &mut m, &lib, class::ITEM_HEALTH, 0, 1, [44, 18]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // 0x414211: beyond the easy level, the nearest item within 16 cells.
    sim.gameplay.actors[knight].intent = None;
    idle::unit(&mut sim, &mut m, &g, &lib, &c, knight, 0);
    assert_eq!(sim.gameplay.actors[knight].intent, Some(Intent::Pick(item)));
    // On the easy level, and wounded: the castle within 24 cells.
    m.game_rules.difficulty = 0;
    m.objects[knight].health -= 1;
    sim.gameplay.actors[knight].intent = None;
    idle::unit(&mut sim, &mut m, &g, &lib, &c, knight, 0);
    assert_eq!(
        sim.gameplay.actors[knight].intent,
        Some(Intent::Board(castle))
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_computer_fighter_walking_by_an_item_takes_it() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [50, 50]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [30, 30]).unwrap();
    let item = new_unit(&mut sim, &mut m, &lib, class::ITEM_HEALTH, 0, 1, [33, 31]).unwrap();
    set_order(&mut sim, knight, Intent::Move([36, 30]), false).unwrap();
    for _ in 0..200 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
        if m.objects[item].flags & FLAG_INACTIVE != 0 {
            break;
        }
    }
    // 0x4de310: the potion one cell off its way is taken up, and drunk at
    // once by the computer's unit (0x4629aa → 0x413f10).
    assert_ne!(m.objects[item].flags & FLAG_INACTIVE, 0);
    assert_eq!(sim.objects[item].holder, None);
    assert!(!sim.objects[knight].contents.contains(&item));
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_computer_peasant_flees_without_its_work_or_into_a_near_refuge() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [40, 40]).unwrap();
    let peasant = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [44, 48]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [46, 48]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // 0x413ae0: unhurt, it runs away and keeps no work.
    sim.gameplay.actors[peasant].intent = Some(Intent::Chop([45, 52]));
    react(&mut sim, &m, &g, &lib, &c, peasant, knight);
    let a = &sim.gameplay.actors[peasant];
    assert!(matches!(a.intent, Some(Intent::Flee(..))));
    assert_eq!(a.resume, None);
    // Wounded, the castle nearer than 12 cells takes it in.
    let a = &mut sim.gameplay.actors[peasant];
    (a.intent, a.automatic) = (None, false);
    m.objects[peasant].health -= 1;
    react(&mut sim, &m, &g, &lib, &c, peasant, knight);
    assert_eq!(
        sim.gameplay.actors[peasant].intent,
        Some(Intent::Board(castle))
    );
}
