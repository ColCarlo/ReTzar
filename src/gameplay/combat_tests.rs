//! Fights against the original data: target choice by the tables of
//! 0x4129d0, the next target at once, the receive path of every victim,
//! the calls for help and the peasant's flight.

use super::tests::{request, setup};
use super::*;

fn class_of(name: &str) -> usize {
    CLASSES.iter().position(|&s| s == name).unwrap()
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
/// A finished building of the original catalogue.
fn building(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    name: &str,
    owner: u8,
    at: [u16; 2],
) -> usize {
    let k = new_unit(sim, m, lib, class_of(name), owner, 1, at).unwrap();
    m.objects[k].flags &= !FLAG_CONSTRUCTION;
    let full = lib.attributes.get(&m.objects[k]).health.max(100);
    m.objects[k].max_health = full;
    m.objects[k].health = full;
    k
}
fn attack_of(sim: &Simulation, m: &MissionWorld, k: usize) -> u16 {
    sim.effective_attributes(k, m).unwrap().attack as u16
}
fn defence_of(sim: &Simulation, m: &MissionWorld, k: usize) -> u32 {
    sim.effective_attributes(k, m).unwrap().defence
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn targets_are_ranked_by_the_native_category_tables() {
    let (c, lib, mut m, _, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    // A soldier (category 5, 4000) before a peasant (3, 3000) at the same gap.
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
    let peon = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [33, 30]).unwrap();
    let archer = new_unit(&mut sim, &mut m, &lib, class_of("archer"), 2, 1, [30, 33]).unwrap();
    assert_eq!(combat::search(&sim, &m, &lib, &c, footman), Some(archer));
    // A peasant capturing (7, 7000) before the soldier.
    sim.gameplay.actors[peon].intent = Some(Intent::Capture(0));
    assert_eq!(combat::search(&sim, &m, &lib, &c, footman), Some(peon));
    // A wall (1) is worth nothing to a soldier...
    let (c, lib, mut m, _, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
    let wall = building(&mut sim, &mut m, &lib, "wall_stone", 2, [32, 30]);
    assert_eq!(combat::search(&sim, &m, &lib, &c, footman), None);
    // ...but 4000 to a catapult (0x58110c), which never aims within its
    // minimum range of 4 (`min_attack_range`).
    let catapult = new_unit(&mut sim, &mut m, &lib, class_of("catapult"), 1, 1, [30, 40]).unwrap();
    sim.objects[catapult].sight = 12;
    assert_eq!(combat::min_range(&lib, &m.objects[catapult]), 4);
    let near = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [31, 41]).unwrap();
    assert_eq!(combat::search(&sim, &m, &lib, &c, catapult), Some(wall));
    position(&mut m.objects[wall], [60, 60]);
    assert_eq!(combat::search(&sim, &m, &lib, &c, catapult), None);
    // A tower (4, 6000) before a soldier (5, 3000) for the catapult.
    position(&mut m.objects[near], [30, 47]);
    let tower = building(&mut sim, &mut m, &lib, "tower_stone", 2, [37, 40]);
    assert_eq!(combat::search(&sim, &m, &lib, &c, catapult), Some(tower));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn towers_spare_the_walls_of_nature_but_fight_its_towers() {
    // Owner 10 (Nature) is an enemy of every kingdom both ways (0x4405ca).
    // Its walls carry 0x1000 with +0x160 cleared by their constructor
    // (0x4fac10): category 1, worth 0 even to a computer's tower; its tower
    // is category 4 and scans the kingdoms in turn (0x44dc70 with 0x30).
    let (c, lib, mut m, _, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    m.kingdoms[1].computer = true;
    let tower = building(&mut sim, &mut m, &lib, "tower_stone", 2, [30, 30]);
    let wall = building(&mut sim, &mut m, &lib, "wall_stone", 10, [32, 30]);
    assert!(combat::computer(&sim, 2));
    assert!(combat::in_reach(&sim, &m, &lib, &c, tower, wall));
    assert_eq!(combat::search(&sim, &m, &lib, &c, tower), None);
    let nature = building(&mut sim, &mut m, &lib, "tower_stone", 10, [30, 34]);
    assert_eq!(combat::search(&sim, &m, &lib, &c, tower), Some(nature));
    assert_eq!(combat::search(&sim, &m, &lib, &c, nature), Some(tower));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_next_target_follows_a_kill_at_once() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let knight = new_unit(&mut sim, &mut m, &lib, class_of("knight"), 1, 1, [30, 30]).unwrap();
    let first = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [31, 30]).unwrap();
    let second = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [33, 32]).unwrap();
    for k in [first, second] {
        // Hold steady: they neither scan nor strike back.
        sim.gameplay.actors[k].stance = 2;
    }
    m.objects[first].health = 1;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("attack", vec![knight], [31, 30], Some(first)),
    )
    .unwrap();
    for _ in 0..200 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if !standing(&m, first) {
            break;
        }
    }
    assert!(!standing(&m, first));
    // Steps 9 → 4 of the Attack program: in the same tick, no idle scan.
    assert_eq!(
        sim.gameplay.actors[knight].intent,
        Some(Intent::Attack(second))
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn every_victim_adds_its_own_bonus_and_pikemen_strike_back() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let pikeman = new_unit(&mut sim, &mut m, &lib, class::PIKEMAN, 2, 1, [30, 30]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, class_of("knight"), 1, 1, [31, 30]).unwrap();
    for k in [pikeman, knight] {
        m.objects[k].max_health = 5000;
        m.objects[k].health = 5000;
        sim.gameplay.actors[k].stance = 2;
    }
    // 0x4ea0b0: a fifth of the pikeman's attack, at least 1, then BONUS.INI
    // `[pikeman] mounted = +600%` against the knight, then armour.
    let back = (attack_of(&sim, &m, pikeman) as u32 * 20 / 100).max(1) as u16;
    let expected = physical_damage(back * 7, defence_of(&sim, &m, knight), 0);
    apply_damage(&mut sim, &mut m, &g, &lib, &c, pikeman, 5, 0, Some(knight));
    assert_eq!(5000 - m.objects[knight].health, expected);
    // Arrows (category 1) are not answered.
    let before = m.objects[knight].health;
    apply_damage(&mut sim, &mut m, &g, &lib, &c, pikeman, 5, 1, Some(knight));
    assert_eq!(m.objects[knight].health, before);
    // A catapult shell: the target takes the raw attack, everything whose
    // cells meet the 5×5 square half of it, each with its own bonus. The
    // house is anchored outside the square; `[siege]
    // buildings_no_towers_walls = +400%` applies to it, nothing to the
    // footman.
    let catapult = new_unit(&mut sim, &mut m, &lib, class_of("catapult"), 1, 1, [10, 40]).unwrap();
    let target = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [30, 40]).unwrap();
    let beside = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [32, 41]).unwrap();
    let house = building(&mut sim, &mut m, &lib, "house", 2, [27, 41]);
    let [w, _] = footprint(&lib, &m.objects[house]);
    assert!(w >= 2 && 27 + w as u16 > 28);
    for k in [target, beside, house] {
        m.objects[k].max_health = 5000;
        m.objects[k].health = 5000;
        sim.gameplay.actors[k].stance = 2;
    }
    let raw = attack_of(&sim, &m, catapult);
    sim.gameplay.projectiles.push(Projectile {
        attacker: catapult,
        target,
        amount: raw,
        splash: raw >> 1,
        category: 2,
        cell: [30, 40],
        kind: missile::SHELL,
        lands: sim.ticks,
        ..Default::default()
    });
    land_projectiles(&mut sim, &mut m, &g, &lib, &c);
    assert_eq!(
        5000 - m.objects[target].health,
        physical_damage(raw, defence_of(&sim, &m, target), 2)
    );
    assert_eq!(
        5000 - m.objects[beside].health,
        physical_damage(raw >> 1, defence_of(&sim, &m, beside), 2)
    );
    let half = raw >> 1;
    assert_eq!(
        5000 - m.objects[house].health,
        physical_damage(half + half * 4, defence_of(&sim, &m, house), 2)
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn golems_break_buildings_and_the_messiah_ignores_magic() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    // 0x4ec830: 13/10 of the attack, then `[troll] buildings = +100%`.
    let troll = new_unit(&mut sim, &mut m, &lib, class_of("troll"), 1, 1, [30, 30]).unwrap();
    let house = building(&mut sim, &mut m, &lib, "house", 2, [31, 30]);
    m.objects[house].max_health = 5000;
    m.objects[house].health = 5000;
    let a = attack_of(&sim, &m, troll) as u32 * 13 / 10;
    let expected = physical_damage((a * 2) as u16, defence_of(&sim, &m, house), 0);
    blow(&mut sim, &mut m, &g, &lib, &c, troll, house);
    assert_eq!(5000 - m.objects[house].health, expected);
    // 0x4ef1d0: magic does not touch the messiah; a sword does.
    let messiah = new_unit(&mut sim, &mut m, &lib, class::MESSIAH, 2, 1, [40, 40]).unwrap();
    let before = m.objects[messiah].health;
    apply_damage(&mut sim, &mut m, &g, &lib, &c, messiah, 50, 3, Some(troll));
    assert_eq!(m.objects[messiah].health, before);
    apply_damage(&mut sim, &mut m, &g, &lib, &c, messiah, 50, 0, Some(troll));
    assert!(m.objects[messiah].health < before);
    // 0x4ef200: nor freeze, discord, hypnosis, stone or curse.
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [41, 40]).unwrap();
    freeze_impact(&mut sim, &mut m, &g, &lib, &c, None, [40, 40]);
    assert!(has_effect(&sim, footman, effect::FREEZE));
    assert!(!has_effect(&sim, messiah, effect::FREEZE));
    discord(&mut sim, &mut m, &lib, messiah);
    assert!(!has_effect(&sim, messiah, effect::DISCORD));
    assert_eq!(m.objects[messiah].owner, 2);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn buildings_call_for_help_and_peasants_run_away() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    // 0x4202d0 → 0x4dd960: a hit farm calls the kingdom's fighters within
    // 3 cells of its corner.
    let farm = building(&mut sim, &mut m, &lib, "farm", 1, [30, 30]);
    let helper = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [33, 27]).unwrap();
    let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [45, 45]).unwrap();
    sim.gameplay.actors[foe].stance = 2;
    apply_damage(&mut sim, &mut m, &g, &lib, &c, farm, 3, 0, Some(foe));
    assert_eq!(
        sim.gameplay.actors[helper].intent,
        Some(Intent::Attack(foe))
    );
    // 0x4ae640 → 0x430be0: a weaker peasant hit by a knight at (+3, +1)
    // runs to (x - 3·5/3, y - 1·5/3) = (x - 5, y - 1), rests 3000 ms and
    // goes back to what it did.
    let peon = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 1, [20, 50]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, class_of("knight"), 2, 1, [23, 51]).unwrap();
    sim.gameplay.actors[knight].stance = 2;
    apply_damage(&mut sim, &mut m, &g, &lib, &c, peon, 1, 0, Some(knight));
    assert_eq!(
        sim.gameplay.actors[peon].intent,
        Some(Intent::Flee([15, 49], 3000))
    );
    let mut arrived = None;
    for t in 0..400 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if arrived.is_none() && sim.gameplay.actors[peon].search_at.is_some() {
            arrived = Some(t);
            assert_eq!([m.objects[peon].x, m.objects[peon].y], [15, 49]);
        }
        if sim.gameplay.actors[peon].intent.is_none() {
            assert_eq!(t - arrived.unwrap(), 3000 / TICK_MS);
            return;
        }
    }
    panic!("the peasant never came back");
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn arrivals_strike_at_once_and_patrols_look_out() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    // Kingdom 2 without its computer player, whose peasant would go to work.
    sim.gameplay.ai.players.remove(&2);
    m.game_rules.no_fog = true;
    // The default program (0x4e4c10): a target already in reach is attacked
    // in the tick the move ends.
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [20, 30]).unwrap();
    let peon = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [31, 30]).unwrap();
    sim.gameplay.actors[peon].stance = 2;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("move", vec![footman], [30, 30], None),
    )
    .unwrap();
    for _ in 0..400 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if !matches!(sim.gameplay.actors[footman].intent, Some(Intent::Move(_))) {
            break;
        }
    }
    assert_eq!(
        sim.gameplay.actors[footman].intent,
        Some(Intent::Attack(peon))
    );
    // A patrol leg (Move argument 2) looks out every second and engages
    // what it finds, keeping the patrol to go back to.
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let walker = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [20, 40]).unwrap();
    let archer = new_unit(&mut sim, &mut m, &lib, class_of("archer"), 2, 1, [30, 45]).unwrap();
    sim.gameplay.actors[archer].stance = 2;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("patrol", vec![walker], [40, 40], None),
    )
    .unwrap();
    let mut engaged = false;
    for _ in 0..600 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if sim.gameplay.actors[walker].intent == Some(Intent::Attack(archer)) {
            engaged = true;
            break;
        }
    }
    assert!(engaged);
    assert!(matches!(
        sim.gameplay.actors[walker].resume,
        Some(Intent::Patrol(..))
    ));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn healers_go_on_to_the_next_wounded_friend() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    m.rules[0].initial_researches.push("te_healing".into());
    let priest = new_unit(&mut sim, &mut m, &lib, 18, 1, 1, [30, 30]).unwrap();
    sim.objects[priest].max_mana = 500;
    sim.objects[priest].mana = 500;
    let first = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [31, 30]).unwrap();
    let second = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [33, 33]).unwrap();
    m.objects[first].health = m.objects[first].max_health - 6;
    m.objects[second].health = m.objects[second].max_health - 12;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("spell_heal", vec![priest], [31, 30], Some(first)),
    )
    .unwrap();
    // Program 0x29: heal, then op 0x51 finds the next wounded friend.
    for _ in 0..400 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if m.objects[second].health == m.objects[second].max_health {
            break;
        }
    }
    assert_eq!(m.objects[first].health, m.objects[first].max_health);
    assert_eq!(m.objects[second].health, m.objects[second].max_health);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 40);
    assert_eq!(sim.gameplay.actors[priest].intent, None);
    // Short of mana the order waits (0x4dff44) instead of ending.
    m.objects[first].health -= 6;
    sim.objects[priest].mana = 0;
    sim.gameplay.actors[priest].intent = Some(Intent::Cast {
        key: "spell_heal".into(),
        cell: [31, 30],
        target: Some(first),
    });
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 10);
    assert!(matches!(
        sim.gameplay.actors[priest].intent,
        Some(Intent::Cast { .. })
    ));
    // Once the mana is back the heal goes on.
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 400);
    assert_eq!(m.objects[first].health, m.objects[first].max_health);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn automatic_drawbridges_rise_before_strangers() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let gate = building(&mut sim, &mut m, &lib, "gate_wood", 1, [30, 30]);
    m.objects[gate].data.resize(77, 0);
    m.objects[gate].data[56] = 0;
    m.objects[gate].data[65] = 1;
    // Passage [32..34, 30..32]; an own unit near it changes nothing.
    let own = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [33, 35]).unwrap();
    sim.gameplay.actors[own].stance = 2;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 20);
    assert_eq!(m.objects[gate].byte(57), 0);
    // A stranger within 4 cells of the passage raises it...
    let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [38, 32]).unwrap();
    sim.gameplay.actors[foe].stance = 2;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 20);
    assert_eq!(m.objects[gate].byte(57), 1);
    // ...and it comes down when the stranger is gone.
    position(&mut m.objects[foe], [60, 60]);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 20);
    assert_eq!(m.objects[gate].byte(57), 0);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_attack_order_reaches_allies_but_the_right_click_does_not() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    m.diplomacy[1] = 0x40;
    m.diplomacy[8] = 0x40;
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
    let friend = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [32, 30]).unwrap();
    // 0x4e43fb: the right click attacks only what is hostile (0x468190).
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![footman], [32, 30], Some(friend)),
    )
    .unwrap();
    assert_ne!(
        sim.gameplay.actors[footman].intent,
        Some(Intent::Attack(friend))
    );
    // 0x435500 / 0x4e3923: the Attack order takes the unit under the cursor.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("attack", vec![footman], [32, 30], Some(friend)),
    )
    .unwrap();
    assert_eq!(
        sim.gameplay.actors[footman].intent,
        Some(Intent::Attack(friend))
    );
    // The program does not drop it for being a friend (op 0xd, 0x4e2523).
    let health = m.objects[friend].health;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 100);
    assert!(m.objects[friend].health < health);
    // Never itself.
    assert!(
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("attack", vec![footman], [30, 30], Some(footman)),
        )
        .is_err()
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_right_click_on_a_dummy_trains_once_learning_allows_levels() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let dummy = building(&mut sim, &mut m, &lib, "dummy", 1, [34, 30]);
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
    let before = m.objects[dummy].health;
    // Without Learning or an academy the kingdom's cap is 0 (0x4df8a9):
    // no training.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![footman], [34, 30], Some(dummy)),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 200);
    assert_eq!(m.objects[dummy].health, before);
    // 0x4e43d9: with Learning the right click on the own dummy trains,
    // where any other own building would only be a destination.
    m.rules[0].initial_researches.push("te_learning".into());
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![footman], [34, 30], Some(dummy)),
    )
    .unwrap();
    assert!(matches!(sim.gameplay.actors[footman].intent, Some(Intent::Attack(j)) if j == dummy));
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 400);
    assert!(m.objects[dummy].health < before);
    // A peasant's own right click (0x4adc20) finds nothing to do with the
    // dummy and falls through 0x4e8220 to 0x4e4280: it trains too.
    let peon = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 1, [30, 32]).unwrap();
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![peon], [34, 30], Some(dummy)),
    )
    .unwrap();
    assert!(matches!(sim.gameplay.actors[peon].intent, Some(Intent::Attack(j)) if j == dummy));
    // What answers 1 to 0x2729 never trains (0x4e43e3, 0x5023e6): a
    // catapult's right click on the dummy is no attack.
    let catapult = new_unit(&mut sim, &mut m, &lib, class_of("catapult"), 1, 1, [30, 36]).unwrap();
    assert!(!combat::can_attack(
        &sim, &m, &lib, &c, catapult, dummy, false
    ));
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![catapult], [34, 30], Some(dummy)),
    )
    .unwrap();
    assert!(!matches!(
        sim.gameplay.actors[catapult].intent,
        Some(Intent::Attack(_))
    ));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn some_classes_choose_their_own_targets() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [30, 30]).unwrap();
    let skeleton = new_unit(&mut sim, &mut m, &lib, class::SKELETON, 2, 1, [30, 34]).unwrap();
    let golem = new_unit(&mut sim, &mut m, &lib, class_of("troll"), 2, 1, [34, 34]).unwrap();
    let barrack = building(&mut sim, &mut m, &lib, "barrack", 2, [40, 30]);
    let bat = new_unit(&mut sim, &mut m, &lib, class_of("bat"), 2, 1, [36, 30]).unwrap();
    // 0x4ea5a0: a priest with a RANGE hits only what leaves no body (0x271c).
    let priest = new_unit(&mut sim, &mut m, &lib, class_of("priest"), 1, 1, [30, 38]).unwrap();
    assert!(combat::max_range(&m, &c, priest) > 0);
    assert!(!combat::can_attack(
        &sim, &m, &lib, &c, priest, footman, false
    ));
    assert!(combat::can_attack(
        &sim, &m, &lib, &c, priest, skeleton, false
    ));
    // 0x4e43fb: its right click on a soldier is then a move.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![priest], [30, 30], Some(footman)),
    )
    .unwrap();
    assert!(matches!(
        sim.gameplay.actors[priest].intent,
        Some(Intent::Move(_))
    ));
    // 0x4ec6b0: a bat hits living objects, no building or golem.
    let own_bat = new_unit(&mut sim, &mut m, &lib, class_of("bat"), 1, 1, [32, 38]).unwrap();
    assert!(combat::can_attack(
        &sim, &m, &lib, &c, own_bat, footman, false
    ));
    assert!(!combat::can_attack(
        &sim, &m, &lib, &c, own_bat, barrack, false
    ));
    assert!(!combat::can_attack(
        &sim, &m, &lib, &c, own_bat, golem, false
    ));
    // 0x4eee30: the bear the same, on land only.
    let bear = new_unit(&mut sim, &mut m, &lib, class_of("bear"), 1, 1, [31, 31]).unwrap();
    assert!(combat::can_attack(&sim, &m, &lib, &c, bear, footman, false));
    assert!(!combat::can_attack(&sim, &m, &lib, &c, bear, bat, false));
    assert!(!combat::can_attack(
        &sim, &m, &lib, &c, bear, barrack, false
    ));
    // 0x4e7580: siege engines never hit the air, the ballista included.
    let ballista = new_unit(&mut sim, &mut m, &lib, class_of("ballista"), 1, 1, [36, 38]).unwrap();
    assert!(!combat::can_attack(
        &sim, &m, &lib, &c, ballista, bat, false
    ));
    assert!(combat::can_attack(
        &sim, &m, &lib, &c, ballista, footman, false
    ));
    // 0x4eeea0: a bear looks for a target only when R(0, 100) < 30.
    let mut found = 0;
    for _ in 0..200 {
        if find_target(&mut sim, &m, &lib, &c, bear).is_some() {
            found += 1;
        }
    }
    assert!((30..=90).contains(&found), "{found}");
    // 0x4e4339: a bridge is walked on, never attacked.
    let soldier = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 42]).unwrap();
    let bridge = building(&mut sim, &mut m, &lib, "bridge", 2, [30, 46]);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![soldier], [30, 46], Some(bridge)),
    )
    .unwrap();
    assert!(!matches!(
        sim.gameplay.actors[soldier].intent,
        Some(Intent::Attack(_))
    ));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_finished_academy_raises_the_cap_by_ten_and_soldiers_train_without_learning() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    // 0x401850: an academy adds 10 to the cap once finished (0x41e10c); one
    // under construction does not count.
    let academy = building(&mut sim, &mut m, &lib, "academy", 1, [20, 20]);
    m.objects[academy].flags |= FLAG_CONSTRUCTION;
    assert_eq!(experience_rules(&m, 1).cap, 0);
    m.objects[academy].flags &= !FLAG_CONSTRUCTION;
    assert_eq!(experience_rules(&m, 1).cap, 10);
    assert_eq!(experience_rules(&m, 2).cap, 0);
    m.rules[0].initial_researches.push("te_learning".into());
    assert_eq!(experience_rules(&m, 1).cap, 14);
    m.rules[0].initial_researches.clear();
    // With it a soldier trains on the dummy, no technology needed (as in
    // the original's «Ponte vicino»).
    let dummy = building(&mut sim, &mut m, &lib, "dummy", 1, [34, 30]);
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
    let before = m.objects[dummy].health;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![footman], [34, 30], Some(dummy)),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 400);
    assert!(m.objects[dummy].health < before);
    assert!(sim.objects[footman].level > 0);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_cannon_tower_loads_its_shells_from_the_class_record() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    m.rules[0].initial_researches.push("te_gunpowder".into());
    let tower = building(&mut sim, &mut m, &lib, "tower_stone2", 1, [30, 30]);
    // A blessing on the tower (+20) does not reach its shells.
    add_timed_effect(
        &mut sim.gameplay.actors[tower],
        Effect {
            until: u64::MAX,
            attack: 20,
            kind: effect::BLESS,
            ..Default::default()
        },
    );
    let enemy = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [36, 30]).unwrap();
    sim.gameplay.actors[enemy].stance = 2;
    m.objects[enemy].max_health = 60000;
    m.objects[enemy].health = 60000;
    let base = lib.attributes.get(&m.objects[tower]).attack;
    for _ in 0..200 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if let Some(p) = sim
            .gameplay
            .projectiles
            .iter()
            .find(|p| p.attacker == tower)
        {
            // 0x4dbd8e: the record's ATTACK with Gunpowder's +10, half of it
            // for the splinters.
            assert_eq!((p.amount, p.splash), (base + 10, (base + 10) / 2));
            return;
        }
    }
    panic!("the tower never fired");
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn archers_loose_from_the_point_of_their_attack_picture() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    m.game_rules.no_fog = true;
    for (class, table) in [
        (
            4,
            [
                [34, 36],
                [44, 35],
                [50, 30],
                [50, 22],
                [43, 16],
                [33, 16],
                [27, 22],
                [27, 30],
            ],
        ),
        (
            6,
            [
                [48, 45],
                [53, 51],
                [62, 58],
                [68, 49],
                [71, 43],
                [66, 37],
                [58, 39],
                [50, 39],
            ],
        ),
    ] {
        let archer = new_unit(&mut sim, &mut m, &lib, class, 1, 1, [30, 30]).unwrap();
        let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [30, 34]).unwrap();
        sim.gameplay.actors[foe].stance = 2;
        sim.gameplay.projectiles.clear();
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("attack", vec![archer], [30, 34], Some(foe)),
        )
        .unwrap();
        for _ in 0..200 {
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
            if !sim.gameplay.projectiles.is_empty() {
                break;
            }
        }
        let p = sim.gameplay.projectiles[0].clone();
        // 0x4e9b60, 0x4e9d90: the picture's corner plus the column's point.
        let column = sim.gameplay.actors[archer].direction as usize;
        let section = lib.property(&m.objects[archer], "image_attack").unwrap();
        let hot = lib.images.numbers(section, "object");
        assert_eq!(
            p.start,
            [
                30 * 32 - hot[0] + table[column][0],
                30 * 32 - hot[1] + table[column][1]
            ],
            "{}",
            CLASSES[class]
        );
        assert_eq!(p.end, [30 * 32 + 16, 34 * 32 + 16]);
        kill(&mut sim, &mut m, &g, &lib, archer);
        kill(&mut sim, &mut m, &g, &lib, foe);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 200);
    }
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn an_arrow_that_finds_nobody_flies_on_for_500_ms() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    m.game_rules.no_fog = true;
    let archer = new_unit(&mut sim, &mut m, &lib, 4, 1, 1, [30, 30]).unwrap();
    let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [30, 36]).unwrap();
    sim.gameplay.actors[foe].stance = 2;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("attack", vec![archer], [30, 36], Some(foe)),
    )
    .unwrap();
    for _ in 0..200 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if !sim.gameplay.projectiles.is_empty() {
            break;
        }
    }
    let p = sim.gameplay.projectiles[0].clone();
    // The target leaves the map before the arrow arrives.
    vanish(&mut sim, &mut m, foe);
    let health = m.objects[archer].health;
    while sim.ticks < p.lands {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    }
    // 0x432bac: nobody on the cell, so it goes on for another 500 ms.
    let spent = sim.gameplay.projectiles[0].clone();
    assert_eq!(spent.flown, p.lands);
    assert_eq!(spent.lands, p.lands + 10);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 10);
    assert!(
        sim.gameplay
            .projectiles
            .iter()
            .all(|x| x.launched != p.launched)
    );
    assert_eq!(m.objects[archer].health, health);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_dragon_breathes_fire_toward_its_target() {
    let (c, lib, mut m, mut g, mut sim) = setup(3);
    sim.fog.explore_all();
    m.game_rules.no_fog = true;
    let dragon = new_unit(&mut sim, &mut m, &lib, 32, 1, 3, [30, 30]).unwrap();
    let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [31, 30]).unwrap();
    sim.gameplay.actors[foe].stance = 2;
    let health = m.objects[foe].health;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("attack", vec![dragon], [31, 30], Some(foe)),
    )
    .unwrap();
    sim.flashes.clear();
    for _ in 0..200 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if m.objects[foe].health < health {
            break;
        }
    }
    // 0x4eea00: the blow at once, no missile, and the fire facing east.
    assert!(m.objects[foe].health < health);
    assert!(sim.gameplay.projectiles.is_empty());
    let fire = sim
        .flashes
        .iter()
        .find(|f| f.image == "dragon_fire")
        .unwrap();
    let column = sim.gameplay.actors[dragon].direction;
    assert_eq!(fire.column, column);
    let section = lib.property(&m.objects[dragon], "image_attack").unwrap();
    let hot = lib.images.numbers(section, "object");
    let sel = lib.images.numbers(section, "selection");
    let x1 = 30 * 32 - hot[0] + sel[0];
    let y1 = 30 * 32 - hot[1] + sel[1];
    let centre = [(2 * x1 + sel[2] - 1) / 2, (2 * y1 + sel[3] - 1) / 2];
    let toward = [
        [0, 90],
        [78, 63],
        [110, 0],
        [78, -63],
        [0, -90],
        [-63, -63],
        [-110, 0],
        [-78, 63],
    ];
    let [dx, dy] = toward[column as usize];
    assert_eq!(fire.at, [centre[0] + dx, centre[1] + dy]);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn attacks_on_the_player_ring_the_alarm_after_fifteen_quiet_seconds() {
    let (c, lib, mut m, _, mut sim) = setup(1);
    let player = m.player();
    let a = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, player, 1, [30, 30]).unwrap();
    let b = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, player, 1, [40, 30]).unwrap();
    let far = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, player, 1, [60, 30]).unwrap();
    let alarms = |sim: &Simulation| {
        sim.sounds
            .iter()
            .filter(|s| s.property == "@notify/attack.wav")
            .count()
    };
    event(&mut sim, &m, &lib, 1, a, a);
    assert_eq!(alarms(&sim), 1);
    // 0x4932cb: a blow within 24 cells renews the event silently, in place.
    sim.ticks += 100;
    event(&mut sim, &m, &lib, 1, b, b);
    assert_eq!((alarms(&sim), sim.gameplay.events.len()), (1, 1));
    assert_eq!(sim.gameplay.events[0].object, b);
    // Each blow restarts the 15 s of quiet (0x493317).
    sim.ticks += 299;
    event(&mut sim, &m, &lib, 1, a, a);
    assert_eq!(alarms(&sim), 1);
    sim.ticks += 300;
    event(&mut sim, &m, &lib, 1, a, a);
    assert_eq!(alarms(&sim), 2);
    // Farther away, a new event at the front.
    event(&mut sim, &m, &lib, 1, far, far);
    assert_eq!(alarms(&sim), 3);
    assert_eq!(sim.gameplay.events.len(), 2);
    assert_eq!(sim.gameplay.events[0].object, far);
    // The notice is a voice of units, category 24, where the blow fell.
    let notice = sim.sounds.last().unwrap();
    assert_eq!(notice.channel, crate::audio::Channel::Unit);
    assert_eq!(notice.category, crate::audio::category::UNIT_READY);
    assert_eq!(notice.cell, Some([60, 30]));
    // Strikes sound by class (vt+0x2ec): bows 8, longbows 9, blows 11.
    let archer = new_unit(&mut sim, &mut m, &lib, class_of("archer"), 2, 1, [9, 9]).unwrap();
    let longbow = new_unit(
        &mut sim,
        &mut m,
        &lib,
        class_of("longbowman"),
        2,
        1,
        [9, 11],
    )
    .unwrap();
    assert_eq!(
        strike_category(&m, &c, archer),
        crate::audio::category::SHOT
    );
    assert_eq!(
        strike_category(&m, &c, longbow),
        crate::audio::category::HEAVY_SHOT
    );
    assert_eq!(strike_category(&m, &c, a), crate::audio::category::BLOW);
}

#[test]
#[ignore = "Requires installed original archives"]
fn on_full_parity_the_sector_lists_decide_the_target() {
    let (c, lib, mut m, _g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [15, 15]).unwrap();
    // Two enemies at the same distance: the older east (sector column 2),
    // the newer south (column 1).
    let east = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [17, 15]).unwrap();
    let south = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [15, 17]).unwrap();
    arrive_in_sectors(&mut sim, &m);
    // 0x44e690 walks the sector columns from the west.
    assert_eq!(combat::search(&sim, &m, &lib, &c, footman), Some(south));
    // In one sector the list's head, the latest arrival, comes first
    // (0x5001a0, 0x500da0).
    position(&mut m.objects[footman], [12, 12]);
    position(&mut m.objects[east], [14, 12]);
    position(&mut m.objects[south], [12, 14]);
    arrive_in_sectors(&mut sim, &m);
    assert_eq!(sim.gameplay.actors[east].sector, Some([1, 1]));
    assert_eq!(sim.gameplay.actors[south].sector, Some([1, 1]));
    assert_eq!(combat::search(&sim, &m, &lib, &c, footman), Some(south));
    position(&mut m.objects[east], [14, 16]);
    arrive_in_sectors(&mut sim, &m);
    position(&mut m.objects[east], [14, 12]);
    arrive_in_sectors(&mut sim, &m);
    assert_eq!(combat::search(&sim, &m, &lib, &c, footman), Some(east));
    // A unit's step puts it in the list as it takes the cell: of two
    // arrivals the later in the game's time leads, whatever order the step
    // met them in.
    sim.gameplay.actors[east].sector_ms = 900;
    sim.gameplay.actors[south].sector_ms = 950;
    assert_eq!(combat::search(&sim, &m, &lib, &c, footman), Some(south));
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_queued_order_waits_for_find_enemy() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [20, 20]).unwrap();
    let first = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [21, 20]).unwrap();
    let second = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [21, 21]).unwrap();
    sim.gameplay.actors[second].hold_fire = true;
    sim.gameplay.actors[first].hold_fire = true;
    set_order(&mut sim, footman, Intent::Attack(first), false).unwrap();
    set_order(&mut sim, footman, Intent::Move([40, 20]), true).unwrap();
    m.objects[first].health = 1;
    // 0x42df96: the Attack program looks for the next enemy (op 0x23)
    // before it ends and lets the queued Move run.
    for _ in 0..200 {
        advance(
            &mut sim,
            &mut m,
            &mut g,
            &lib,
            &c,
            Duration::from_millis(TICK_MS),
            10,
            false,
        );
        if !alive(&m.objects[first]) {
            break;
        }
    }
    assert!(!alive(&m.objects[first]));
    advance(
        &mut sim,
        &mut m,
        &mut g,
        &lib,
        &c,
        Duration::from_millis(TICK_MS),
        10,
        false,
    );
    assert_eq!(
        sim.gameplay.actors[footman].intent,
        Some(Intent::Attack(second))
    );
    assert_eq!(sim.gameplay.actors[footman].queued.len(), 1);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_shift_list_of_targets_walks_up_to_each_and_strikes_none() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    m.diplomacy[1] = 0x40;
    m.diplomacy[8] = 0x40;
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
    let a = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [38, 30]).unwrap();
    let b = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [38, 38]).unwrap();
    let health = [m.objects[a].health, m.objects[b].health];
    // 0x4356c2 → 0x4e3979: a Move within reach of each, in the order
    // clicked, and Attacks given no object.
    let order = Request {
        targets: vec![a, b],
        ..request("attack", vec![footman], [38, 38], Some(b))
    };
    issue(&mut sim, &mut m, &g, &lib, &c, &order).unwrap();
    let actor = &sim.gameplay.actors[footman];
    assert_eq!(actor.intent, Some(Intent::Reach(a)));
    assert_eq!(actor.queued, [Intent::Reach(b)]);
    let mut steps = 0;
    while sim.gameplay.actors[footman].intent == Some(Intent::Reach(a)) && steps < 600 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        steps += 1;
    }
    assert_eq!(sim.gameplay.actors[footman].intent, Some(Intent::Reach(b)));
    assert!(combat::object_gap(&lib, &m.objects[footman], &m.objects[a]) <= 1);
    while sim.gameplay.actors[footman].intent.is_some() && steps < 1200 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        steps += 1;
    }
    assert!(combat::object_gap(&lib, &m.objects[footman], &m.objects[b]) <= 1);
    assert_eq!([m.objects[a].health, m.objects[b].health], health);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_messiah_casts_its_spells_by_itself_while_it_fights() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let messiah = new_unit(&mut sim, &mut m, &lib, class::MESSIAH, 1, 1, [30, 30]).unwrap();
    let full = sim.objects[messiah].mana;
    assert!(full >= 1000);
    let troll = new_unit(&mut sim, &mut m, &lib, class_of("troll"), 2, 1, [31, 30]).unwrap();
    sim.gameplay.actors[troll].stance = 2;
    m.objects[troll].max_health = 30000;
    m.objects[troll].health = 30000;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("attack", vec![messiah], [31, 30], Some(troll)),
    )
    .unwrap();
    // 0x4eefd0: 7 blows of 31 try a spell it has the mana for, with no
    // button: the mana goes down by itself.
    let mut cast = false;
    for _ in 0..3000 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if sim.objects[messiah].mana < full {
            cast = true;
            break;
        }
    }
    assert!(cast);
}
