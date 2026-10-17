//! Researches against the original data: records changed in place, what a
//! change of owner keeps, holders sized when made, and the order rules.

use super::tests::{request, setup};
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
/// Completes `name` in the castle at once, through the queue.
fn research(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    name: &str,
) {
    sim.gameplay.actors[0].jobs.push_back(Job {
        kind: JobKind::Research(name.into()),
        remaining: 1,
        total: 1,
        paid: [0; 4],
        due_ms: Some(0),
        waiting: false,
    });
    ticks(sim, m, g, lib, c, 1);
    assert!(researches(m, 1).iter().any(|r| r == name));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn feudalism_and_slavery_cut_the_peasant_food_twice() {
    let (c, _, m, _, _) = setup(2);
    let peon = |known: &[&str]| {
        let known: Vec<String> = known.iter().map(|s| s.to_string()).collect();
        c.stats(&m.objects[1], "peon", &known).cost[2]
    };
    let base = peon(&[]);
    // 0x4d4159 multiplies the record by 80/100 for each, truncating.
    assert_eq!(peon(&["te_feudalism"]), base * 80 / 100);
    assert_eq!(
        peon(&["te_feudalism", "te_slavery"]),
        base * 80 / 100 * 80 / 100
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn holders_keep_the_room_they_were_made_with() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    let room = |sim: &Simulation, m: &MissionWorld, k: usize| holder_capacity(sim, m, &lib, k);
    let castle = room(&sim, &m, 0);
    let galleon = new_unit(&mut sim, &mut m, &lib, 23, 1, 1, [40, 40]).unwrap();
    let ship = room(&sim, &m, galleon);
    research(&mut sim, &mut m, &mut g, &lib, &c, "te_landingtactics");
    // 0x4d389f grows the ships already made; the castle keeps its room.
    assert_eq!(room(&sim, &m, galleon), ship * 150 / 100);
    // Completed a second time: the ships grow again, to × (100 + 100) / 100.
    let again = new_unit(&mut sim, &mut m, &lib, 23, 1, 1, [44, 40]).unwrap();
    research(&mut sim, &mut m, &mut g, &lib, &c, "te_landingtactics");
    assert_eq!(room(&sim, &m, galleon), ship * 150 / 100 * 200 / 100);
    assert_eq!(room(&sim, &m, again), ship * 150 / 100 * 200 / 100);
    assert_eq!(room(&sim, &m, 0), castle);
    // 0x4fffc4: everything made afterwards, any class, half again for each
    // completion (K+0x106 is now 100).
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [42, 42]).unwrap();
    let base: usize = lib
        .property(&m.objects[footman], "max_obj_in_holder")
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(room(&sim, &m, footman), base * 200 / 100);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_new_owner_keeps_the_bonuses_baked_into_the_object() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    // An Arab horse archer (the European tree has none).
    let rider = new_unit(&mut sim, &mut m, &lib, 10, 1, 2, [40, 40]).unwrap();
    let plain = (sim.objects[rider].attack, sim.objects[rider].defence);
    research(&mut sim, &mut m, &mut g, &lib, &c, "te_ridinghorses");
    research(&mut sim, &mut m, &mut g, &lib, &c, "te_crossbow");
    let trained = (sim.objects[rider].attack, sim.objects[rider].defence);
    assert_eq!(trained, (plain.0 + 3, plain.1 + 1));
    // 0x5050a0: Riding Horses stays with the horse archer; the kingdom's
    // crossbow bonus (0x4de7d0) does not.
    transfer_owner(&mut sim, &mut m, &lib, rider, 2);
    assert_eq!(
        (sim.objects[rider].attack, sim.objects[rider].defence),
        (plain.0 + 2, plain.1 + 1)
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn priests_heal_with_a_right_click_after_healing() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let priest = new_unit(&mut sim, &mut m, &lib, 18, 1, 1, [30, 30]).unwrap();
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [32, 30]).unwrap();
    m.objects[footman].health -= 10;
    let click = request("context", vec![priest], [32, 30], Some(footman));
    issue(&mut sim, &mut m, &g, &lib, &c, &click).unwrap();
    assert!(matches!(
        sim.gameplay.actors[priest].intent,
        Some(Intent::Move(_))
    ));
    m.rules[0].initial_researches.push("te_healing".into());
    issue(&mut sim, &mut m, &g, &lib, &c, &click).unwrap();
    assert!(matches!(
        sim.gameplay.actors[priest].intent,
        Some(Intent::Cast { ref key, target: Some(t), .. }) if key == "spell_heal" && t == footman
    ));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn order_rules_follow_the_shared_records() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    // `trade = te_traderoutes` is only in the Asian tree, but binds every
    // trader (0x496362).
    let trader = new_unit(&mut sim, &mut m, &lib, 2, 1, 1, [30, 30]).unwrap();
    assert_eq!(c.order_research("trade"), Some("te_traderoutes"));
    assert!(c.available("trade", &m.objects[trader], &m).is_err());
    // Any forbidden list forbids the name (0x440ef3).
    m.rules[0].forbidden_researches.push("footman".into());
    assert!(c.available("e_footman", &m.objects[0], &m).is_err());
    assert!(
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("e_footman", vec![0])
        )
        .is_err()
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_cancelled_step_gives_back_the_record_as_it_is_now() {
    let (c, lib, mut m, g, mut sim) = setup(2);
    sync(&mut sim, &mut m, &lib);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("r_peon", vec![0]),
    )
    .unwrap();
    let food = m.kingdoms[0].resources[2];
    // Feudalism completes elsewhere while the peasant is under way.
    m.rules[0].initial_researches.push("te_feudalism".into());
    let now = c.stats(&m.objects[0], "peon", researches(&m, 1)).cost[2];
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("cancel", vec![0]),
    )
    .unwrap();
    assert_eq!(m.kingdoms[0].resources[2], food + now);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn kung_fu_peasants_use_their_third_blow() {
    for (civ, kungfu, expected) in [(3, true, true), (3, false, false), (1, true, false)] {
        let (c, lib, mut m, mut g, mut sim) = setup(civ);
        m.game_rules.no_fog = true;
        if kungfu {
            m.rules[0].initial_researches.push("te_kungfu".into());
        }
        let peon = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, civ, [30, 30]).unwrap();
        let dummy = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [31, 30]).unwrap();
        m.objects[dummy].max_health = 60000;
        m.objects[dummy].health = 60000;
        sim.gameplay.actors[dummy].stance = 2;
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("attack", vec![peon], [31, 30], Some(dummy)),
        )
        .unwrap();
        // 0x4ae090: one blow in three with `image_attack3`.
        let mut seen = false;
        for _ in 0..1200 {
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
            seen |= sim.gameplay.actors[peon].action_image == "image_attack3";
        }
        assert_eq!(seen, expected, "civ {civ}, kung fu {kungfu}");
    }
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_research_completed_twice_counts_twice() {
    // F7 of the audit: queued in two busy buildings, a research completes
    // twice and its handler runs twice (0x41eed0 → 0x4d33d0): Gold Mining
    // adds 2 to the yield each time, 10 → 14.
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    research(&mut sim, &mut m, &mut g, &lib, &c, "te_goldplus");
    research(&mut sim, &mut m, &mut g, &lib, &c, "te_goldplus");
    assert_eq!(resource_yield(&m, 1, 0), 14);
    // Temper Iron twice: +2 attack to the swords (K+0xe0 += 1 each time).
    let before = StatsCheck::of(&sim, &m, &lib);
    research(&mut sim, &mut m, &mut g, &lib, &c, "te_temperiron");
    research(&mut sim, &mut m, &mut g, &lib, &c, "te_temperiron");
    let after = StatsCheck::of(&sim, &m, &lib);
    assert_eq!(after.0 - before.0, 2);
    // Fields set by a handler follow the order of completion: Warfare after
    // Sun Tzu leaves 70 (0x4d36dc), as the original.
    research(&mut sim, &mut m, &mut g, &lib, &c, "te_suntzubook");
    research(&mut sim, &mut m, &mut g, &lib, &c, "te_warfare");
    assert_eq!(experience_rules(&m, 1).speed, 70);
}
/// The live attack of a footman of kingdom 1 made for the check.
struct StatsCheck(u32);
impl StatsCheck {
    fn of(_sim: &Simulation, m: &MissionWorld, _lib: &ObjectLibrary) -> Self {
        let mut footman = m.objects[0].clone();
        footman.class = class::FOOTMAN;
        footman.owner = 1;
        Self(ObjectState::live_bonus(&footman, researches(m, 1))[0])
    }
}
