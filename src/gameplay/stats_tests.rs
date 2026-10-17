//! Class data against the original tables: EMIT auras, the easy campaign
//! scaling, record categories and sights, and the ground attack cycle.

use super::tests::{request, setup};
use super::*;

fn class_of(name: &str) -> usize {
    CLASSES.iter().position(|&s| s == name).unwrap()
}
fn finished(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    name: &str,
    owner: u8,
    civ: u8,
    at: [u16; 2],
) -> usize {
    let k = new_unit(sim, m, lib, class_of(name), owner, civ, at).unwrap();
    m.objects[k].flags &= !FLAG_CONSTRUCTION;
    k
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn castles_raise_friends_and_lower_strangers() {
    let (_c, lib, mut m, _g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    // EURO BUILDINGS.TXT: castle sight 12, EMIT "+400,-200"; the European
    // row binds every civilization (0x49751a).
    let euro = lib.attributes.emit(&m.objects[0]).unwrap();
    assert_eq!((euro.sight, euro.own, euro.other), (12, 400, -200));
    let mut arab = m.objects[0].clone();
    arab.civilization = 2;
    assert_eq!(lib.attributes.emit(&arab), Some(euro));
    let own = finished(&mut sim, &mut m, &lib, "house", 1, 1, [20, 6]);
    let foe = finished(&mut sim, &mut m, &lib, "house", 2, 1, [21, 12]);
    let far = finished(&mut sim, &mut m, &lib, "house", 1, 1, [50, 50]);
    let base = sim.objects[own].resistance;
    assert_eq!(base, 50);
    // 0x41f271: 10 a pulse toward the target, never below 0.
    restore_resistance(&mut sim, &m, &lib, 1);
    assert_eq!(sim.objects[own].resistance, base + 10);
    assert_eq!(sim.objects[foe].resistance, base - 10);
    assert_eq!(sim.objects[far].resistance, base);
    for _ in 0..60 {
        restore_resistance(&mut sim, &m, &lib, 1);
    }
    assert_eq!(sim.objects[own].resistance, base + 400);
    assert_eq!(sim.objects[foe].resistance, 0);
    // Without the castle the target is the house's own again.
    m.objects[0].flags |= FLAG_INACTIVE;
    restore_resistance(&mut sim, &m, &lib, 1);
    assert_eq!(sim.objects[own].resistance, base + 390);
    assert_eq!(sim.objects[foe].resistance, 10);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn easy_campaigns_spare_the_players_side() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    m.campaign = true;
    m.game_rules.difficulty = 0;
    let mine = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
    let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [31, 30]).unwrap();
    for k in [mine, foe] {
        m.objects[k].max_health = 5000;
        m.objects[k].health = 5000;
        sim.gameplay.actors[k].stance = 2;
    }
    let def = |sim: &Simulation, k: usize| {
        sim.effective_attributes(k, &m)
            .unwrap()
            .defence
    };
    let (dm, df) = (def(&sim, mine), def(&sim, foe));
    // 0x504990: ×80/100 (at least 1) to the player's side, ×120/100 from it.
    apply_damage(&mut sim, &mut m, &g, &lib, &c, mine, 20, 0, Some(foe));
    assert_eq!(5000 - m.objects[mine].health, physical_damage(16, dm, 0));
    apply_damage(&mut sim, &mut m, &g, &lib, &c, foe, 20, 0, Some(mine));
    assert_eq!(5000 - m.objects[foe].health, physical_damage(24, df, 0));
    // Not in a game started from the setup dialog.
    m.campaign = false;
    let before = m.objects[mine].health;
    apply_damage(&mut sim, &mut m, &g, &lib, &c, mine, 20, 0, Some(foe));
    assert_eq!(before - m.objects[mine].health, physical_damage(20, dm, 0));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn records_give_the_messiah_sword_bonuses_and_revealers_their_sight() {
    let (_c, lib, mut m, _g, mut sim) = setup(1);
    let messiah = new_unit(&mut sim, &mut m, &lib, class::MESSIAH, 1, 1, [30, 30]).unwrap();
    let o = &m.objects[messiah];
    let plain = ObjectState::read(o, &lib).attack;
    let mut trained = ObjectState::read(o, &lib);
    trained.apply_researches(o, &["te_temperiron".into(), "te_scimitar".into()]);
    // 0x4eef50 leaves +0x6b at 0: the melee bonuses (K+0xe0) apply.
    assert_eq!(trained.attack, plain + 2);
    // FOG_REVEALER.INI [comm] fog_range = 8 (0x494f66).
    let revealer = new_unit(&mut sim, &mut m, &lib, class::FOG_REVEALER, 1, 1, [40, 40]).unwrap();
    m.objects[revealer].data.resize(64, 0);
    m.objects[revealer].data[0x1a] = 255;
    assert_eq!(ObjectState::read(&m.objects[revealer], &lib).sight, 8);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn ground_attacks_fire_on_the_attack_cycle() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    let catapult = new_unit(&mut sim, &mut m, &lib, class_of("catapult"), 1, 1, [30, 30]).unwrap();
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("attack_ground", vec![catapult], [37, 30], None),
    )
    .unwrap();
    let (_, period) = animation_timing(&lib, &m.objects[catapult], "image_attack").unwrap();
    let mut shots = Vec::new();
    for _ in 0..600 {
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
        for p in &sim.gameplay.projectiles {
            if p.attacker == catapult && !shots.contains(&p.launched) {
                shots.push(p.launched);
            }
        }
        if shots.len() == 3 {
            break;
        }
    }
    assert_eq!(shots.len(), 3, "no shots at the ground");
    // The blows keep their own due time: two periods apart in all.
    let span = (shots[2] - shots[0]) * TICK_MS;
    assert!(span.abs_diff(2 * period) < TICK_MS, "{span} vs {period}");
}
