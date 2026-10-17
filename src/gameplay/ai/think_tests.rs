//! The computer players' looks against their native routines (0x402760,
//! 0x407c20, 0x407690, 0x407110, 0x40be30, 0x408420, 0x408960, 0x409550,
//! 0x405a20), in the test world: kingdom 1 with a castle at (5, 5) and three
//! peasants, kingdom 2 a European computer player with 10000 of everything.
use super::*;
use crate::gameplay::ai::script::{Library, flag};
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
fn ticks(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    n: usize,
) {
    for _ in 0..n {
        tick(sim, m, g, lib, c);
    }
}
/// Kingdom 2 reads `text` instead of its default script.
fn script(sim: &mut Simulation, c: &Catalog, text: &str) {
    let lib = Library::new(
        [("data\\ai\\t.ais".to_owned(), text.as_bytes().to_vec())],
        c.ai.types.clone(),
    );
    let p = sim.gameplay.ai.players.get_mut(&2).unwrap();
    p.program = Some(lib.compile("t.ais").unwrap());
}
fn player(sim: &Simulation) -> &Player {
    &sim.gameplay.ai.players[&2]
}
fn kind(c: &Catalog, name: &str) -> u16 {
    c.ai.types.iter().position(|t| t == name).unwrap() as u16
}
fn rule_flags(sim: &Simulation, r: usize) -> u32 {
    player(sim).program.as_ref().unwrap().rules[r].flags
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_first_looks_come_one_period_apart_over_the_players() {
    let (c, lib, mut m, mut g, _) = setup(1);
    let mut third = m.kingdoms[1].clone();
    third.name = "Third".into();
    m.kingdoms.push(third);
    let mut sim = Simulation::new(&m, &lib, 64, "grass");
    script(&mut sim, &c, "");
    sim.gameplay.ai.players.get_mut(&3).unwrap().program = player(&sim).program.clone();
    // Kingdom 2 has a unit, so a home; kingdom 3 nothing, so no phase.
    new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [40, 40]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // 0x402760 at 50 ms: kingdom 2 at once, kingdom 3 1500 / 2 ms later
    // (normal); each looks again a period after its look (0x407c20).
    let due = |sim: &Simulation, k: u8| sim.gameplay.ai.players[&k].due;
    assert_eq!((due(&sim, 2), due(&sim, 3)), (Some(1550), Some(800)));
    assert_eq!(sim.gameplay.ai.players[&2].phase, 1);
    assert_eq!(sim.gameplay.ai.players[&3].phase, 0);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 15);
    assert_eq!((due(&sim, 2), due(&sim, 3)), (Some(1550), Some(2300)));
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_home_is_a_castle_or_what_stands_nearest_the_kingdom() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    script(&mut sim, &c, "");
    // Only units: the nearest to their centroid (20, 20), where it stands,
    // looked for again every period.
    for p in [[10, 20], [20, 19], [30, 21]] {
        new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, p).unwrap();
    }
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert_eq!(player(&sim).home, Some([20, 19]));
    assert!(player(&sim).homeless);
    // A building: the one nearest the centroid of all, at its corner.
    let farm = new_unit(&mut sim, &mut m, &lib, class::FARM, 2, 1, [40, 40]).unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 30);
    assert_eq!(player(&sim).home, Some([40, 40]));
    assert!(!player(&sim).homeless);
    // A castle: its centre. Two: the one nearest the buildings' centroid
    // (0x401ad0), the latest to join first on ties.
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [50, 10]).unwrap();
    m.objects[farm].health = 0;
    m.objects[farm].flags |= FLAG_INACTIVE;
    let [w, h] = footprint(&lib, &m.objects[castle]).map(|n| n as i32);
    // Homeless no more, the home is found again only when it was not.
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 30);
    assert_eq!(player(&sim).home, Some([40, 40]));
    sim.gameplay.ai.players.get_mut(&2).unwrap().homeless = true;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 30);
    assert_eq!(player(&sim).home, Some([50 + w / 2, 10 + h / 2]));
    let near = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [20, 40]).unwrap();
    new_unit(&mut sim, &mut m, &lib, class::FARM, 2, 1, [10, 50]).unwrap();
    sim.gameplay.ai.players.get_mut(&2).unwrap().homeless = true;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 30);
    let at = [m.objects[near].x as i32, m.objects[near].y as i32];
    assert_eq!(player(&sim).home, Some([at[0] + w / 2, at[1] + h / 2]));
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_kingdom_lists_run_from_the_latest_to_join() {
    let (_, lib, mut m, _, mut sim) = setup(1);
    let a = new_unit(&mut sim, &mut m, &lib, class::FARM, 2, 1, [20, 20]).unwrap();
    let b = new_unit(&mut sim, &mut m, &lib, class::FARM, 2, 1, [30, 20]).unwrap();
    let u = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [30, 30]).unwrap();
    assert_eq!(think::kingdom_list(&sim, &m, 2, false), [b, a]);
    assert_eq!(think::kingdom_list(&sim, &m, 2, true), [u]);
    // 0x5050a0: a building given away and back joins at the head.
    transfer_owner(&mut sim, &mut m, &lib, a, 1);
    transfer_owner(&mut sim, &mut m, &lib, a, 2);
    assert_eq!(think::kingdom_list(&sim, &m, 2, false), [a, b]);
    // The map's own objects joined in their order.
    assert_eq!(think::kingdom_list(&sim, &m, 1, true), [3, 2, 1]);
}

#[test]
#[ignore = "Requires installed original archives"]
fn peasants_are_listed_by_their_program() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    script(&mut sim, &c, "");
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [40, 40]).unwrap();
    let mine = new_unit(&mut sim, &mut m, &lib, class::RES_GOLD, 0, 1, [50, 30]).unwrap();
    let site = new_unit(&mut sim, &mut m, &lib, class::FARM, 2, 1, [30, 50]).unwrap();
    m.objects[site].flags |= FLAG_CONSTRUCTION;
    let a = &mut sim.gameplay.actors[site];
    (a.construction, a.construction_total) = (100, 100);
    let peasants: Vec<usize> = (0..4)
        .map(|n| new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [36 + n, 36]).unwrap())
        .collect();
    let [idle, miner, builder, chopper] = peasants[..] else {
        unreachable!()
    };
    sim.gameplay.actors[miner].intent = Some(Intent::Harvest(mine));
    sim.gameplay.actors[builder].intent = Some(Intent::Build(site));
    sim.gameplay.actors[chopper].intent = Some(Intent::Chop([60, 60]));
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // 0x40c720 at the take-over: every peasant, then each in its group, the
    // latest sorted first; the mine it works is marked in the list.
    let l = &player(&sim).lists;
    assert_eq!(l.peasants, [chopper, builder, miner, idle]);
    assert_eq!(l.groups[group::IDLE as usize], [idle]);
    assert_eq!(l.groups[group::GOLD as usize], [miner]);
    assert_eq!(l.groups[group::BUILDERS as usize], [builder]);
    assert_eq!(l.groups[group::WOOD as usize], [chopper]);
    assert_eq!(l.peons[&builder].target, Some(site));
    assert_eq!(l.peons[&chopper].rect, [60, 60, 60, 60]);
    assert_eq!(
        l.mines
            .iter()
            .map(|e| (e.object, e.count))
            .collect::<Vec<_>>(),
        [(mine, 1)]
    );
    // A new order is a new program (0x4ae5e0): the miner leaves its group.
    set_order(&mut sim, miner, Intent::Move([20, 20]), false).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let l = &player(&sim).lists;
    assert!(!l.peons.contains_key(&miner));
    assert!(l.groups.iter().all(|g| !g.contains(&miner)));
    // The builder's site is left by its last builder before any work: it
    // dies (0x42df20), its price given back.
    let before: u32 = m.kingdoms[1].resources.iter().sum();
    set_order(&mut sim, builder, Intent::Move([20, 21]), false).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert!(!alive(&m.objects[site]));
    assert!(m.kingdoms[1].resources.iter().sum::<u32>() > before);
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_build_phase_orders_a_unit_at_an_idle_producer_and_pays_for_it() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    script(&mut sim, &c, "rule have 3 peon end");
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [40, 40]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // 0x409550: the castle trains a peasant, paid from the kingdom and from
    // what the player may spend; three are booked by the workers' part.
    let cost = c
        .stats(&m.objects[castle], "peon", researches(&m, 2))
        .cost
        .map(|n| n as i32);
    assert!(matches!(
        sim.gameplay.actors[castle].jobs.front(),
        Some(Job {
            kind: JobKind::Train(class::PEON),
            ..
        })
    ));
    assert_eq!(
        m.kingdoms[1].resources,
        std::array::from_fn(|r| 10000 - cost[r] as u32)
    );
    let p = player(&sim);
    assert_eq!(p.available, std::array::from_fn(|r| 10000 - cost[r]));
    assert_eq!(p.buckets[rules::bucket::WORKERS], cost.map(|n| 3 * n));
    assert_ne!(rule_flags(&sim, 0) & flag::IN_PROGRESS, 0);
    // Five periods on, the castle is busy: nothing more is ordered.
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 150);
    assert_eq!(sim.gameplay.actors[castle].jobs.len(), 1);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_critical_rule_stops_the_phase_until_it_holds() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    script(
        &mut sim,
        &c,
        "critical rule when gold 30000 end\nrule have 1 peon end",
    );
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [40, 40]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert!(sim.gameplay.actors[castle].jobs.is_empty());
    // Inside a group it skips only to the group's end.
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    script(
        &mut sim,
        &c,
        "{ critical rule when gold 30000 end\nrule have 1 farm end }\nrule have 1 peon end",
    );
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [40, 40]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert_eq!(sim.gameplay.actors[castle].jobs.len(), 1);
}

#[test]
#[ignore = "Requires installed original archives"]
fn switches_forgotten_rules_and_percentages() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    script(
        &mut sim,
        &c,
        "rule First trigger Ready forget Second end\n\
         rule Second when trigger Ready have 1 peon end\n\
         rule Third have 1% peon trigger Never end\n\
         rule Fourth when random 50 end",
    );
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [40, 40]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let program = player(&sim).program.as_ref().unwrap();
    // 0x408420: the first rule has no goal, so it holds; it sets its switch
    // and forgets the second, which then never runs.
    assert_ne!(program.rules[0].flags & flag::SATISFIED, 0);
    assert!(
        program
            .triggers
            .iter()
            .find(|t| t.name == "READY")
            .unwrap()
            .on
    );
    assert_ne!(program.rules[1].flags & flag::FORGOTTEN, 0);
    // `HAVE 1%` keeps its bit: never satisfied, yet two peasants (1% of
    // 200) are pursued; its switch never turns on.
    assert_eq!(program.rules[2].flags & flag::SATISFIED, 0);
    assert!(
        !program
            .triggers
            .iter()
            .find(|t| t.name == "NEVER")
            .unwrap()
            .on
    );
    assert_eq!(sim.gameplay.actors[castle].jobs.len(), 1);
    // RANDOM is drawn once and kept.
    let roll = player(&sim).rolls.clone();
    assert_eq!(roll.len(), 1);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 160);
    assert_eq!(player(&sim).rolls, roll);
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_parts_book_their_needs_and_the_start_reserves_a_lumber_mill() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    script(&mut sim, &c, "rule have 2 barracks end");
    new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [40, 40]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // 0x40fb00: without castle nor lumber mill the mill's price stays
    // committed; the script's part books two barracks, and the peasant
    // lays one, paid by the kingdom and taken from what the player may
    // spend.
    let probe = tree::probe(0, 1);
    let lumber = c.stats(&probe, "lumber", &[]).cost.map(|n| n as i32);
    let barrack = c.stats(&probe, "barrack", &[]).cost.map(|n| n as i32);
    let p = player(&sim);
    assert_eq!(p.committed, lumber);
    assert_eq!(
        p.available,
        std::array::from_fn(|r| 10000 - lumber[r] - barrack[r])
    );
    assert_eq!(p.buckets[rules::bucket::SCRIPT], barrack.map(|n| 2 * n));
    assert_eq!(
        m.kingdoms[1].resources,
        std::array::from_fn(|r| 10000 - barrack[r] as u32)
    );
    let _ = kind(&c, "barrack");
}

#[test]
#[ignore = "Requires installed original archives"]
fn producers_and_availability_follow_the_trees() {
    let (c, lib, m, _, sim) = setup(1);
    // 0x405a20: the last class whose own menu trains or researches it; the
    // summoned creatures are the mage's.
    assert_eq!(
        tree::producer(&c, &lib, 1, class::PEON),
        Some(class::CASTLE)
    );
    assert_eq!(
        tree::producer(&c, &lib, 1, class::FOOTMAN),
        Some(class::BARRACK)
    );
    assert_eq!(tree::producer(&c, &lib, 1, 15), Some(class::MAGE));
    let feudalism = kind(&c, "te_feudalism") as usize;
    assert_eq!(tree::producer(&c, &lib, 1, feudalism), Some(class::CASTLE));
    // 0x494c90: a research missing (the pikeman's halberd), a building not
    // finished (the knight's stable) or not in the tree (the mosque for a
    // European) keeps a type out.
    let ok = |name: &str| tree::available(&sim, &m, &c, &lib, 2, 1, name);
    assert!(ok("footman") && ok("barrack") && ok("te_feudalism"));
    assert!(!ok("pikeman") && !ok("knight") && !ok("mosque"));
    let mut m = m;
    m.rules[1].initial_researches.push("te_feudalism".into());
    assert!(!tree::available(&sim, &m, &c, &lib, 2, 1, "te_feudalism"));
}
