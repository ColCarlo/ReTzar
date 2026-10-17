//! Production against the original data: steps in 666 ms ticks, housing
//! from 1, the exit cell, the rally point, refunds, fields and repairs.

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

#[test]
#[ignore = "Requires the installed original game archives"]
fn units_come_out_after_666_ms_ticks_next_to_the_building() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    let time = c.stats(&m.objects[0], "peon", &[]).time_ms;
    // 0x41fc29 / 0x41f0bf: first tick 50 ms after the order, then 666 ms
    // ticks of 666 ms of progress.
    let done = 50 + 666 * (time.div_ceil(666) - 1);
    let count = m.objects.len();
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("e_peon", vec![0]),
    )
    .unwrap();
    ticks(
        &mut sim,
        &mut m,
        &mut g,
        &lib,
        &c,
        done.div_ceil(TICK_MS) - 1,
    );
    assert_eq!(m.objects.len(), count);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(m.objects.len(), count + 1);
    // Without a rally point it stands on a ring around the castle.
    let peon = &m.objects[count];
    let r = movement::footprint_rect(&lib, &m.objects[0]);
    let gap = combat::gap(
        r,
        [peon.x as i32, peon.y as i32, peon.x as i32, peon.y as i32],
    );
    assert!(gap <= 5, "gap {gap}");
    position(&mut m.objects[count], [30, 30]);
    // With a rally point east of the castle, on the castle's east border
    // cell nearest to it (the rally clamped onto the ring, ring 0 first).
    let east = [r[2] as u16 + 6, r[1] as u16 + 1];
    sim.gameplay.actors[0].rally = Some(east);
    sim.gameplay.actors[0].rally_mode = 0;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("e_peon", vec![0]),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, done.div_ceil(TICK_MS));
    let second = &m.objects[count + 1];
    assert_eq!([second.x as i32, second.y as i32], [r[2] + 1, r[1] + 1]);
    // ...and walks to within two cells of the flag (0x41f5e5).
    let Some(Intent::Move(to)) = sim.gameplay.actors[count + 1].intent else {
        panic!("no walk to the rally point");
    };
    assert!(to[0].abs_diff(east[0]) <= 2 && to[1].abs_diff(east[1]) <= 2);
    // A click inside the building clears the rally point (0x41e877).
    let mut inside = request("move", vec![0], [r[0] as u16 + 1, r[1] as u16 + 1], None);
    inside.order = "context".into();
    issue(&mut sim, &mut m, &g, &lib, &c, &inside).unwrap();
    assert_eq!(sim.gameplay.actors[0].rally, None);
    // 0x41e760: a building that trains nothing (a house) ignores the click,
    // and so does an unfinished barracks; a finished one takes it.
    for (name, finished, kept) in [
        ("house", true, false),
        ("barrack", false, false),
        ("barrack", true, true),
    ] {
        let class = CLASSES.iter().position(|&s| s == name).unwrap();
        let k = new_unit(&mut sim, &mut m, &lib, class, 1, 1, [40, 40]).unwrap();
        if finished {
            m.objects[k].flags &= !FLAG_CONSTRUCTION;
        } else {
            m.objects[k].flags |= FLAG_CONSTRUCTION;
        }
        let mut click = request("move", vec![k], [48, 48], None);
        click.order = "context".into();
        issue(&mut sim, &mut m, &g, &lib, &c, &click).unwrap();
        assert_eq!(sim.gameplay.actors[k].rally.is_some(), kept, "{name}");
    }
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn housing_starts_at_one_and_cows_ignore_it() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    // 0x4671b0: 1, plus the castle's pop_count.
    let castle: usize = lib
        .property(&m.objects[0], "pop_count")
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(population(&m, &lib, 1).1, 1 + castle);
    // `@setpoplimit` replaces the housing; a new house adds to it.
    m.population_limit.insert(1, 3);
    m.population_base.insert(1, housing_sum(&m, &lib, 1));
    assert_eq!(population(&m, &lib, 1), (3, 3));
    // A full kingdom waits, with the original message, retrying every 2 s.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("e_peon", vec![0]),
    )
    .unwrap();
    let count = m.objects.len();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 600);
    assert_eq!(m.objects.len(), count);
    let message = sim.gameplay.actors[0].production_blocked.clone().unwrap();
    assert_eq!(message, c.poplimit[1]);
    let due = sim.gameplay.actors[0].jobs[0].due_ms.unwrap();
    for _ in 0..41 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if sim.gameplay.actors[0].jobs[0].due_ms != Some(due) {
            break;
        }
    }
    assert_eq!(sim.gameplay.actors[0].jobs[0].due_ms, Some(due + 2000));
    // A farm still gives cows (attribute 0x2725).
    let farm = new_unit(&mut sim, &mut m, &lib, class::FARM, 1, 1, [40, 40]).unwrap();
    m.objects[farm].flags &= !FLAG_CONSTRUCTION;
    let cows = |m: &MissionWorld| {
        m.objects
            .iter()
            .filter(|o| o.class == class::COW && alive(o))
            .count()
    };
    let before = cows(&m);
    sim.gameplay.actors[farm].jobs.push_back(Job {
        kind: JobKind::Train(class::COW),
        remaining: 1,
        total: 1,
        paid: [0; 4],
        due_ms: Some(0),
        waiting: false,
    });
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(cows(&m), before + 1);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn unfinished_sites_give_back_their_cost_and_fields_are_born_ripe() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    let wood = |m: &MissionWorld| m.kingdoms[0].resources[3];
    let start = wood(&m);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("e_house", vec![1], [20, 10], None),
    )
    .unwrap();
    let house = m.objects.len() - 1;
    let cost = start - wood(&m);
    assert!(cost > 0);
    // Deleted before any work: all of it (0x41d6a8).
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("die", vec![house]),
    )
    .unwrap();
    assert_eq!(wood(&m), start);
    // After some work: cost × f / 1000, f = (1000·T − 1000·p) / (2·T).
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("e_house", vec![1], [20, 14], None),
    )
    .unwrap();
    let house = m.objects.len() - 1;
    let a = &mut sim.gameplay.actors[house];
    let t = a.construction_total;
    a.construction = t - t / 4;
    let p = t / 4;
    let f = ((1000 * t - 1000 * p) / (2 * t)) as u32;
    let before = wood(&m);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("die", vec![house]),
    )
    .unwrap();
    assert_eq!(wood(&m), before + cost * f / 1000);
    // 0x44c7a0: a field is complete at once and the peasant digs it.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("field", vec![1], [24, 18], None),
    )
    .unwrap();
    let field = m.objects.len() - 1;
    assert_eq!(m.objects[field].class, class::FIELD);
    assert_eq!(m.objects[field].flags & FLAG_CONSTRUCTION, 0);
    assert_eq!(m.objects[field].health, m.objects[field].max_health);
    assert_eq!(sim.gameplay.actors[1].intent, Some(Intent::Harvest(field)));
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn peasants_repair_machines_five_points_a_cycle() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    let catapult = new_unit(&mut sim, &mut m, &lib, 19, 1, 1, [16, 6]).unwrap();
    let full = m.objects[catapult].max_health;
    m.objects[catapult].health = full - 20;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![1], [16, 6], Some(catapult)),
    )
    .unwrap();
    assert_eq!(
        sim.gameplay.actors[1].intent,
        Some(Intent::Repair(catapult))
    );
    for _ in 0..400 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if m.objects[catapult].health != full - 20 {
            break;
        }
    }
    // 0x4abd5d: +5 a cycle.
    assert_eq!(m.objects[catapult].health, full - 15);
}

/// A finished house of kingdom 1 with `missing` health to mend.
fn damaged_house(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    at: [u16; 2],
    missing: u16,
) -> usize {
    let k = new_unit(sim, m, lib, class::HOUSE, 1, 1, at).unwrap();
    m.objects[k].flags &= !FLAG_CONSTRUCTION;
    sim.gameplay.actors[k].construction = 0;
    m.objects[k].health = m.objects[k].max_health - missing;
    k
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_repair_goes_on_to_the_nearest_damaged_building_in_sight() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    sim.gameplay.ai.players.remove(&2);
    let first = damaged_house(&mut sim, &mut m, &lib, [20, 10], 4);
    let next = damaged_house(&mut sim, &mut m, &lib, [20, 16], 4);
    let far = damaged_house(&mut sim, &mut m, &lib, [55, 55], 4);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("repair", vec![1], [20, 10], Some(first)),
    )
    .unwrap();
    // The Repair program (0x42efe0): step 3 (op 0x36, 0x4ad3e9) takes the
    // nearest object to mend within twice the sight, then the program ends.
    let mut seen = Vec::new();
    for _ in 0..4000 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        match sim.gameplay.actors[1].intent {
            Some(Intent::Repair(k)) if seen.last() != Some(&k) => seen.push(k),
            None => break,
            _ => {}
        }
    }
    assert_eq!(seen, vec![first, next]);
    assert_eq!(sim.gameplay.actors[1].intent, None);
    for k in [first, next] {
        assert_eq!(m.objects[k].health, m.objects[k].max_health);
    }
    assert!(m.objects[far].health < m.objects[far].max_health);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_repair_walk_that_fails_waits_four_seconds_and_looks_again() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    sim.gameplay.ai.players.remove(&2);
    position(&mut m.objects[1], [30, 30]);
    // A pond round the peasant: nothing is nearer than its cell, so its
    // walk fails at once (0x4df364).
    for y in 28..=32 {
        for x in 28..=32 {
            if (x, y) != (30, 30) {
                g.cells[y * 64 + x] = 0;
            }
        }
    }
    movement::refresh(&mut sim, &m, &g, &lib);
    let far = damaged_house(&mut sim, &mut m, &lib, [44, 30], 4);
    let near = damaged_house(&mut sim, &mut m, &lib, [34, 30], 4);
    set_order(&mut sim, 1, Intent::Repair(far), false).unwrap();
    let mut failed = None;
    for _ in 0..100 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if let Some(due) = sim.gameplay.actors[1].search_at {
            failed = Some((sim.ticks, due));
            break;
        }
    }
    // Step 1's Move fails to step 5, a Delay of 4000 ms, then step 3.
    let (at, due) = failed.expect("the walk fails");
    assert_eq!(due, at + 4000 / TICK_MS);
    while sim.ticks + 1 < due {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        assert_eq!(sim.gameplay.actors[1].intent, Some(Intent::Repair(far)));
    }
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(sim.gameplay.actors[1].intent, Some(Intent::Repair(near)));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_builder_whose_site_another_finished_goes_on_to_the_next() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    sim.gameplay.ai.players.remove(&2);
    let site = |sim: &mut Simulation, m: &mut MissionWorld, at: [u16; 2], work: u64| {
        let k = new_unit(sim, m, &lib, class::HOUSE, 1, 1, at).unwrap();
        m.objects[k].flags |= FLAG_CONSTRUCTION;
        m.objects[k].health = 1;
        sim.gameplay.actors[k].construction_total = work;
        sim.gameplay.actors[k].construction = work;
        k
    };
    let first = site(&mut sim, &mut m, [18, 8], 100);
    let second = site(&mut sim, &mut m, [26, 8], 1_000_000);
    set_order(&mut sim, 1, Intent::Build(first), false).unwrap();
    set_order(&mut sim, 2, Intent::Build(first), false).unwrap();
    for _ in 0..4000 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if m.objects[first].flags & FLAG_CONSTRUCTION == 0 {
            break;
        }
    }
    assert_eq!(m.objects[first].flags & FLAG_CONSTRUCTION, 0);
    // 0x4acc5f: the Build op on a finished building succeeds, and step 4
    // (op 0x22) finds the next site, for the builder who finished it and
    // for the other alike.
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 400);
    for i in [1, 2] {
        assert_eq!(sim.gameplay.actors[i].intent, Some(Intent::Build(second)));
    }
}

#[test]
fn a_dragged_wall_is_an_l_with_the_longer_side_first() {
    // 0x4fb8f0: longer side first, Alt the other one unless straight.
    assert_eq!(
        wall_row([10, 10], [13, 11], false, None),
        vec![[10, 10], [11, 10], [12, 10], [13, 10], [13, 11]]
    );
    assert_eq!(
        wall_row([10, 10], [13, 11], true, None),
        vec![[10, 10], [10, 11], [11, 11], [12, 11], [13, 11]]
    );
    assert_eq!(
        wall_row([10, 10], [9, 13], false, None),
        vec![[10, 10], [10, 11], [10, 12], [10, 13], [9, 13]]
    );
    assert_eq!(
        wall_row([5, 5], [5, 3], true, None),
        vec![[5, 5], [5, 4], [5, 3]]
    );
    assert_eq!(wall_row([5, 5], [5, 5], false, None), vec![[5, 5]]);
    // 0x4fb19d: placed from the end nearer the builder.
    assert_eq!(
        wall_row([10, 10], [13, 11], false, Some([14, 12])),
        vec![[13, 11], [13, 10], [12, 10], [11, 10], [10, 10]]
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_builder_takes_new_sites_after_its_own_then_finds_the_nearest() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    let order = |sim: &mut Simulation, m: &mut MissionWorld, g: &MapGrid, at: [u16; 2]| {
        issue(
            sim,
            m,
            g,
            &lib,
            &c,
            &request("wall_wood", vec![1], at, None),
        )
        .unwrap();
        m.objects.len() - 1
    };
    let first = order(&mut sim, &mut m, &g, [20, 10]);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    assert_eq!(sim.gameplay.actors[1].intent, Some(Intent::Build(first)));
    // 0x48f160: while building, the next site waits its turn.
    let second = order(&mut sim, &mut m, &g, [24, 10]);
    assert_eq!(sim.gameplay.actors[1].intent, Some(Intent::Build(first)));
    assert_eq!(
        sim.gameplay.actors[1].queued.front(),
        Some(&Intent::Build(second))
    );
    // A third site no one was given: op 0x22 finds it after the second.
    let third = new_unit(&mut sim, &mut m, &lib, class::WALL_WOOD, 1, 1, [26, 12]).unwrap();
    m.objects[third].flags |= FLAG_CONSTRUCTION;
    m.objects[third].health = 1;
    sim.gameplay.actors[third].construction_total = 100;
    sim.gameplay.actors[third].construction = 100;
    let mut seen = Vec::new();
    for _ in 0..4000 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if let Some(Intent::Build(k)) = sim.gameplay.actors[1].intent
            && seen.last() != Some(&k)
        {
            seen.push(k);
        }
        if sim.gameplay.actors[1].intent.is_none() {
            break;
        }
    }
    assert_eq!(seen, vec![first, second, third]);
    assert!(
        [first, second, third]
            .iter()
            .all(|&k| m.objects[k].flags & FLAG_CONSTRUCTION == 0)
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_wooden_wall_row_turns_to_stone_together_for_the_price_of_each_segment() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    m.rules[0].initial_researches.push("te_architecture".into());
    let wall = |sim: &mut Simulation, m: &mut MissionWorld, at: [u16; 2]| {
        let k = new_unit(sim, m, &lib, class::WALL_WOOD, 1, 1, at).unwrap();
        m.objects[k].flags &= !FLAG_CONSTRUCTION;
        m.objects[k].health = m.objects[k].max_health;
        k
    };
    let row: Vec<usize> = (20..23).map(|x| wall(&mut sim, &mut m, [x, 20])).collect();
    let apart = wall(&mut sim, &mut m, [30, 20]);
    let one = c.stats(&m.objects[row[1]], "to_wall_stone", &[]).cost;
    let before = m.kingdoms[0].resources;
    let result = issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("to_swall", vec![row[1]]),
    );
    result.unwrap();
    // 0x4fab10: three segments, three times the price.
    let after = m.kingdoms[0].resources;
    assert_eq!(
        [0, 1, 2, 3].map(|r| before[r] - after[r]),
        one.map(|n| 3 * n)
    );
    // 0x4fa640: every segment shows the leader's queue.
    assert!(row.iter().all(|&k| producer(&sim, k) == row[1]));
    assert_eq!(producer(&sim, apart), apart);
    for _ in 0..20_000 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if sim.gameplay.actors[row[1]].jobs.is_empty() {
            break;
        }
    }
    assert!(row.iter().all(|&k| m.objects[k].class == class::WALL_STONE));
    assert_eq!(m.objects[apart].class, class::WALL_WOOD);
    assert!(sim.gameplay.wall_rows.is_empty());
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_posed_builder_moves_to_the_next_segment_without_turning_or_waiting_anew() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    position(&mut m.objects[1], [21, 11]);
    let order = |sim: &mut Simulation, m: &mut MissionWorld, g: &MapGrid, at: [u16; 2]| {
        issue(
            sim,
            m,
            g,
            &lib,
            &c,
            &request("wall_wood", vec![1], at, None),
        )
        .unwrap();
        m.objects.len() - 1
    };
    let first = order(&mut sim, &mut m, &g, [20, 10]);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    let second = order(&mut sim, &mut m, &g, [22, 10]);
    let mut last_blow = None;
    let mut turned = None;
    for _ in 0..4000 {
        let before = sim.gameplay.actors[first].construction;
        let facing = sim.gameplay.actors[1].direction;
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if sim.gameplay.actors[first].construction != before {
            last_blow = Some(sim.ticks * TICK_MS);
        }
        if sim.gameplay.actors[1].intent == Some(Intent::Build(second)) {
            turned = Some(sim.gameplay.actors[1].direction != facing);
            break;
        }
    }
    // 0x4abb7a: no turn, and the next blow is due a whole anim_time after
    // the last one rather than at the picture's actionrow.
    assert_eq!(turned, Some(false));
    let period = action_ms(&lib, &m.objects[1], "image_build");
    let due = sim.gameplay.actors[1].work_due_ms;
    assert_eq!(due, last_blow.unwrap() + period);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_harbour_sits_across_the_shore_never_on_plain_land() {
    let (_, lib, mut m, mut g, _) = setup(1);
    // Land up to x = 29, a shore column pair, then sea.
    for y in 0..g.side {
        for x in 30..g.side {
            g.cells[y * g.side + x] = if x < 32 { 0x10 } else { 0 };
        }
    }
    let mut probe = m.objects[0].clone();
    probe.class = class::HARBOR;
    probe.owner = 1;
    let [w, _] = footprint(&lib, &probe);
    assert!(w >= 3);
    let mut site = |m: &mut MissionWorld, x: u16| {
        probe.x = x;
        probe.y = 20;
        placement(&g, m, &lib, &probe, None, true)
    };
    // 0x45b070: across the shore into the sea.
    assert!(site(&mut m, 30));
    // Plain land under it, or nothing but sea along its border.
    assert!(!site(&mut m, 28));
    assert!(!site(&mut m, 40));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn cancelling_the_last_unit_under_way_drops_the_research_queued_after_it() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    for order in ["e_peon", "te_learning"] {
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command(order, vec![0]),
        )
        .unwrap();
    }
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    assert_eq!(sim.gameplay.actors[0].jobs.len(), 2);
    let before = m.kingdoms[0].resources;
    let peon = c.stats(&m.objects[0], "peon", &[]).cost;
    let mut cancel = Request::command("cancel_job", vec![0]);
    cancel.target = Some(0);
    issue(&mut sim, &mut m, &g, &lib, &c, &cancel).unwrap();
    // 0x41ea81: the program ends; Learning goes without its cost back.
    assert!(sim.gameplay.actors[0].jobs.is_empty());
    let after = m.kingdoms[0].resources;
    assert_eq!([0, 1, 2, 3].map(|r| after[r] - before[r]), peon);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn taverns_offer_mercenaries_and_heroes_like_the_original() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    for r in ["te_mercenaries", "te_heroesquest", "te_fame"] {
        m.rules[0].initial_researches.push(r.into());
    }
    let tavern = new_unit(&mut sim, &mut m, &lib, class::TAVERN, 1, 1, [30, 30]).unwrap();
    // 0x4d1e5f: the first offers a second after the tavern appears.
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 19);
    assert!(sim.gameplay.actors[tavern].hires.iter().all(Vec::is_empty));
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    let [mercs, heroes] = sim.gameplay.actors[tavern].hires.clone();
    assert_eq!((mercs.len(), heroes.len()), (1, 1));
    // 8–12 mercenaries at level 0, one hero at 10–30, Fame adding 4; both
    // from the barracks menu of their civilization.
    assert!((8..=12).contains(&mercs[0].count) && mercs[0].level == 4);
    assert!(heroes[0].count == 1 && (14..=34).contains(&heroes[0].level));
    for h in mercs.iter().chain(&heroes) {
        let mut barrack = m.objects[tavern].clone();
        barrack.class = class::BARRACK;
        barrack.civilization = h.civilization;
        let menu = lib.property(&barrack, "menu").unwrap();
        assert!(c.menus.entries(menu).iter().any(|(k, _)| *k == h.order));
        assert!(CLASSES[h.class].eq_ignore_ascii_case(&c.class_name(&h.order)));
    }
    // Then one more of each every 60 s, eight at most, the oldest leaving.
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1199);
    assert_eq!(sim.gameplay.actors[tavern].hires[0].len(), 1);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(sim.gameplay.actors[tavern].hires[0].len(), 2);
    for _ in 0..8 {
        sim.gameplay.actors[tavern].hire_due = Some(sim.ticks);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    }
    let mercs = &sim.gameplay.actors[tavern].hires[0];
    assert_eq!(mercs.len(), 8);
    assert!(mercs.iter().all(|h| h.id > heroes[0].id + 2));
    // vt+0xf8: cost × count × 1.6, plus 30 gold and 15 of each other
    // resource the class costs a level.
    assert_eq!(hire_price([50, 0, 20, 0], 10, 4), [920, 0, 380, 0]);
    assert_eq!(hire_price([0, 0, 0, 0], 1, 10), [300, 0, 0, 0]);
    // Hiring the hero: paid, out by a corner, promoted, the offer gone.
    m.population_limit.insert(1, 200);
    let hero = sim.gameplay.actors[tavern].hires[1][0].clone();
    let mut probe = m.objects[tavern].clone();
    probe.class = hero.class;
    probe.civilization = hero.civilization;
    let price = hire_price(
        c.stats(&probe, CLASSES[hero.class], researches(&m, 1)).cost,
        hero.count,
        hero.level,
    );
    let before = m.kingdoms[0].resources;
    let count = m.objects.len();
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command(&format!("hire:{}", hero.id), vec![tavern]),
    )
    .unwrap();
    assert_eq!(m.objects.len(), count + 1);
    let j = count;
    let o = &m.objects[j];
    assert_eq!(
        (o.class, o.civilization, o.owner),
        (hero.class, hero.civilization, 1)
    );
    assert_eq!(sim.objects[j].level, hero.level);
    assert!(super::hero(o) && !o.name.is_empty());
    assert!(o.x.abs_diff(30) <= 4 && o.y.abs_diff(30) <= 4);
    for r in 0..4 {
        assert_eq!(m.kingdoms[0].resources[r], before[r] - price[r]);
    }
    assert!(
        sim.gameplay.actors[tavern].hires[1]
            .iter()
            .all(|h| h.id != hero.id)
    );
    // A full population refuses with the original message.
    m.population_limit.insert(1, 1);
    m.population_base.insert(1, housing_sum(&m, &lib, 1));
    let merc = sim.gameplay.actors[tavern].hires[0][0].clone();
    let refused = issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command(&format!("hire:{}", merc.id), vec![tavern]),
    );
    assert!(refused.is_err());
    assert_eq!(m.objects.len(), count + 1);
    assert_eq!(sim.gameplay.actors[tavern].hires[0][0], merc);
    // The submenu lists the offers with the barracks picture, «Contratta
    // %s», the price and the number of units.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let assets = crate::assets::GameAssets::open(root).unwrap();
    let skin = crate::hud::HudSkin::load(&assets).unwrap();
    let locale = crate::assets::Ini::locale(&assets).unwrap();
    m.simulation = Some(sim.clone());
    let state = crate::hud::HudState {
        selected: vec![tavern],
        command_menu: Some("tavern_merc".into()),
        ..Default::default()
    };
    let list = crate::hud::commands(&skin, &locale, &state, &m, &lib);
    assert_eq!(list.len(), 8);
    let first = &list[0];
    assert_eq!(first.order, format!("hire:{}", merc.id));
    assert_eq!(
        first.icon,
        c.orders
            .get(&merc.order, "img")
            .unwrap()
            .trim()
            .parse::<usize>()
            .unwrap()
    );
    assert!(first.label.starts_with("Contratta "), "{}", first.label);
    let mut probe = m.objects[tavern].clone();
    probe.class = merc.class;
    probe.civilization = merc.civilization;
    let price = hire_price(
        c.stats(&probe, CLASSES[merc.class], researches(&m, 1)).cost,
        merc.count,
        merc.level,
    );
    let gold = locale.label("viewmsg", "Gold");
    assert!(
        first
            .cost
            .as_deref()
            .unwrap()
            .contains(&format!("{gold}:{}", price[0])),
        "{:?}",
        first.cost
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn asian_markets_sell_and_buy_items_like_the_original() {
    let (c, lib, mut m, mut g, mut sim) = setup(3);
    sim.fog.explore_all();
    m.rules[0].initial_researches.push("te_jewelry".into());
    let market = new_unit(&mut sim, &mut m, &lib, class::MARKET, 1, 3, [30, 30]).unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 20);
    let offers = |sim: &Simulation| sim.gameplay.jewels[&1].offers.clone();
    assert_eq!(offers(&sim).len(), 1);
    // A second market of the kingdom adds nothing while the first stands.
    let other = new_unit(&mut sim, &mut m, &lib, class::MARKET, 1, 3, [40, 40]).unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 40);
    assert_eq!(offers(&sim).len(), 1);
    assert_eq!(sim.gameplay.jewels[&1].market, Some(market));
    let _ = other;
    // Eight at most, the oldest leaving.
    for _ in 0..10 {
        sim.gameplay.actors[market].hire_due = Some(sim.ticks);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    }
    let list = offers(&sim);
    assert_eq!(list.len(), 8);
    assert!(list.windows(2).all(|w| w[0].id < w[1].id) && list[0].id >= 3);
    for jewel in &list {
        let item = jewel_object(&m.objects[market], jewel);
        match jewel.class {
            class::ITEM_MAGIC => {
                assert!(!matches!(item.byte(0x60), 8 | 13) && item.byte(0x60) <= 20);
                // One charge in the first ten minutes.
                assert_eq!(item.word(0x66), 1);
            }
            class::ITEM_PLUSPLUS => {
                let total = item.byte(0x16) as u16 + item.byte(0x17) as u16 + item.word(0x18);
                assert!((5..=20).contains(&total), "{:?}", jewel.fields);
            }
            class::ITEM_HEALTH => {
                let fields: Vec<u16> = [0x12, 0x16, 0x18, 0x1c, 0x24]
                    .iter()
                    .map(|&k| item.word(k))
                    .filter(|&n| n > 0)
                    .collect();
                assert_eq!(fields.len(), 1, "{:?}", jewel.fields);
            }
            k => panic!("class {k}"),
        }
    }
    // Values (vt+0x2c4).
    let probe = m.objects[market].clone();
    let gold = |k: usize| creature_gold(&c, &probe, k);
    let value = |fields: &[(usize, u16)], class: usize, spell: u8| {
        let jewel = Jewel {
            class,
            image: "item_potion1".into(),
            size: if class == class::ITEM_MAGIC {
                0x70
            } else {
                0x6a
            },
            fields: fields.to_vec(),
            spell,
            ..Default::default()
        };
        item_value(&jewel_object(&probe, &jewel), gold)
    };
    assert_eq!(value(&[(0x16, 10)], class::ITEM_PLUSPLUS, 0), 40);
    assert_eq!(value(&[(0x16, 10 << 8)], class::ITEM_PLUSPLUS, 0), 40);
    assert_eq!(value(&[(0x18, 10)], class::ITEM_PLUSPLUS, 0), 40);
    assert_eq!(value(&[(0x12, 50)], class::ITEM_HEALTH, 0), 50);
    assert_eq!(value(&[(0x1c, 10)], class::ITEM_HEALTH, 0), 50);
    assert_eq!(value(&[(0x16, 10)], class::ITEM_HEALTH, 0), 20);
    assert_eq!(value(&[(0x66, 2)], class::ITEM_MAGIC, 0), 150);
    assert_eq!(value(&[(0x66, 1)], class::ITEM_MAGIC, 3), gold(22) * 3 / 2);
    // Buying: four times the value in gold, the item by the market.
    let jewel = list[0].clone();
    let price = 4 * item_value(&jewel_object(&m.objects[market], &jewel), gold);
    let before = m.kingdoms[0].resources[0];
    let count = m.objects.len();
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command(&format!("buy:{}", jewel.id), vec![market]),
    )
    .unwrap();
    assert_eq!(m.objects.len(), count + 1);
    let item = count;
    assert_eq!(m.objects[item].class, jewel.class);
    assert_eq!(m.kingdoms[0].resources[0], before - price);
    assert!(m.objects[item].x.abs_diff(30) <= 4 && m.objects[item].y.abs_diff(30) <= 4);
    assert!(offers(&sim).iter().all(|x| x.id != jewel.id));
    // Selling what a unit of the kingdom holds: its value, the item gone.
    let peon = 1;
    sim.objects[item].holder = Some(peon);
    sim.objects[peon].contents.push(item);
    m.objects[item].flags |= crate::world::FLAG_HELD;
    assert_eq!(sellable_items(&sim, &m, 1), vec![item]);
    assert!(sellable_items(&sim, &m, 2).is_empty());
    let worth = item_value(&m.objects[item], gold);
    let before = m.kingdoms[0].resources[0];
    // The lists the command grid shows.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let assets = crate::assets::GameAssets::open(root).unwrap();
    let skin = crate::hud::HudSkin::load(&assets).unwrap();
    let locale = crate::assets::Ini::locale(&assets).unwrap();
    m.simulation = Some(sim.clone());
    let menu = |name: &str| crate::hud::HudState {
        selected: vec![market],
        command_menu: Some(name.into()),
        ..Default::default()
    };
    let buy = crate::hud::commands(&skin, &locale, &menu("market_items"), &m, &lib);
    assert_eq!(buy.len(), 7);
    assert!(buy[0].label.starts_with("Compra "), "{}", buy[0].label);
    let first = jewel_object(&m.objects[market], &offers(&sim)[0]);
    assert!(
        buy[0]
            .cost
            .as_deref()
            .unwrap()
            .ends_with(&format!(":{}", 4 * item_value(&first, gold)))
    );
    let sell = crate::hud::commands(&skin, &locale, &menu("market_sell"), &m, &lib);
    assert_eq!(sell.len(), 1);
    assert_eq!(sell[0].order, format!("sell:{item}"));
    assert!(
        sell[0]
            .cost
            .as_deref()
            .unwrap()
            .contains(&(4 * worth).to_string())
    );
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command(&format!("sell:{item}"), vec![market]),
    )
    .unwrap();
    assert_eq!(m.kingdoms[0].resources[0], before + worth);
    assert!(m.objects[item].flags & FLAG_INACTIVE != 0);
    assert!(sim.objects[peon].contents.is_empty());
    // Without the gold nothing is bought.
    m.kingdoms[0].resources[0] = 0;
    let jewel = offers(&sim)[0].clone();
    let count = m.objects.len();
    assert!(
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command(&format!("buy:{}", jewel.id), vec![market]),
        )
        .is_err()
    );
    assert_eq!(m.objects.len(), count);
    assert_eq!(offers(&sim)[0], jewel);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn monks_pay_for_the_priesthood_and_hypnotised_units_stay() {
    // An Asian monk of level 3 has «Converti in sacerdote» (0x4eb3c0).
    let (c, lib, mut m, g, mut sim) = setup(3);
    let me = m.player();
    let k = me as usize - 1;
    let monk = new_unit(&mut sim, &mut m, &lib, 12, me, 3, [20, 20]).unwrap();
    m.objects[monk].health = 100;
    add_levels(&mut sim, &mut m, &lib, monk, 3);
    let price = c.stats(&m.objects[monk], CLASSES[18], &[]).cost;
    assert!(price.iter().any(|&n| n > 0));
    // 0x4679b0: «Non c'è sufficiente …» names the first resource short, in
    // the order gold, stone, wood, food.
    m.kingdoms[k].resources = [0; 4];
    let order = Request::command("become_priest", vec![monk]);
    let first = [0, 1, 3, 2].into_iter().find(|&r| price[r] > 0).unwrap();
    assert_eq!(
        issue(&mut sim, &mut m, &g, &lib, &c, &order),
        Err(c.notres[first].clone())
    );
    assert!(c.notres[first].starts_with("Non c'è sufficiente "));
    assert_eq!(m.objects[monk].class, 12);
    // Paid: the monk becomes a priest (0x4eb4b3).
    m.kingdoms[k].resources = [1000; 4];
    issue(&mut sim, &mut m, &g, &lib, &c, &order).unwrap();
    assert_eq!(
        m.kingdoms[k].resources,
        [0, 1, 2, 3].map(|r| 1000 - price[r])
    );
    // 0x4ac74e: a hypnotised peasant does not become a jihad warrior.
    // An Arab peasant (`r_peon` menu).
    let peon = new_unit(&mut sim, &mut m, &lib, class::PEON, me, 2, [24, 20]).unwrap();
    m.objects[peon].health = 100;
    sim.gameplay.actors[peon].effects.push(Effect {
        until: sim.ticks + 600,
        kind: effect::HYPNOSIS,
        ..Default::default()
    });
    m.rules[k].initial_researches.push("te_jihad".into());
    let jihad = Request::command("become_jihadw", vec![peon]);
    assert_eq!(
        issue(&mut sim, &mut m, &g, &lib, &c, &jihad),
        Err("Un'unità ipnotizzata non può convertirsi".into())
    );
    assert_eq!(m.objects[peon].class, class::PEON);
    sim.gameplay.actors[peon].effects.clear();
    issue(&mut sim, &mut m, &g, &lib, &c, &jihad).unwrap();
    assert_eq!(m.objects[peon].class, 33);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_sell_list_follows_the_order_items_were_picked() {
    let (_, lib, mut m, _, mut sim) = setup(3);
    let me = m.player();
    let holder = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, me, 3, [30, 30]).unwrap();
    let a = new_unit(&mut sim, &mut m, &lib, class::ITEM_HEALTH, 0, 0, [31, 30]).unwrap();
    let b = new_unit(&mut sim, &mut m, &lib, class::ITEM_HEALTH, 0, 0, [32, 30]).unwrap();
    // 0x4629e3: each pick appends `sellitem_<id>` to `market_sell`.
    garrison(&mut sim, &mut m, holder, b);
    garrison(&mut sim, &mut m, holder, a);
    assert_eq!(sellable_items(&sim, &m, me), [b, a]);
    // Picked again, an item goes last.
    sim.objects[holder].contents.retain(|&k| k != b);
    sim.objects[b].holder = None;
    garrison(&mut sim, &mut m, holder, b);
    assert_eq!(sellable_items(&sim, &m, me), [a, b]);
}

#[test]
#[ignore = "Requires installed original archives"]
fn each_class_tests_its_own_site() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    sim.fog.explore_all();
    position(&mut m.objects[1], [20, 50]);
    let build = |sim: &mut Simulation, m: &mut MissionWorld, order: &str, at: [u16; 2]| {
        issue(sim, m, &g, &lib, &c, &request(order, vec![1], at, None))
    };
    let newest = |m: &MissionWorld, class: usize| {
        (0..m.objects.len())
            .rev()
            .find(|&k| m.objects[k].class == class)
            .unwrap()
    };
    // 0x456b50: a gate stands on walls, which it replaces (0x455eae).
    let walls: Vec<usize> = (14..34)
        .map(|x| {
            let k = new_unit(&mut sim, &mut m, &lib, class::WALL_WOOD, 1, 1, [x, 30]).unwrap();
            m.objects[k].flags &= !FLAG_CONSTRUCTION;
            k
        })
        .collect();
    build(&mut sim, &mut m, "gate_wood", [20, 30]).unwrap();
    let gate = newest(&m, class::GATE_WOOD);
    let r = movement::footprint_rect(&lib, &m.objects[gate]);
    for &k in &walls {
        let under = (r[0]..=r[2]).contains(&(m.objects[k].x as i32))
            && (r[1]..=r[3]).contains(&(m.objects[k].y as i32));
        assert_eq!(alive(&m.objects[k]), !under);
    }
    // A unit in a gate's passage forbids it.
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [42, 41]).unwrap();
    let mut probe = gameplay_probe(&lib, &c, &m.objects[1], "gate_wood", [40, 40]);
    assert!(!can_place(&g, &with_sim(&m, &sim), &lib, &probe));
    position(&mut m.objects[footman], [42, 50]);
    probe.x = 40;
    assert!(can_place(&g, &with_sim(&m, &sim), &lib, &probe));
    // 0x44ca70: a field takes cells where land units stand; a house does not.
    let field = gameplay_probe(&lib, &c, &m.objects[1], "field", [41, 49]);
    let house = gameplay_probe(&lib, &c, &m.objects[1], "house", [41, 49]);
    assert!(can_place(&g, &with_sim(&m, &sim), &lib, &field));
    assert!(!can_place(&g, &with_sim(&m, &sim), &lib, &house));
    // Fliers hold no land cell (0xc1200000 has bit 24 only).
    let bat = new_unit(&mut sim, &mut m, &lib, 15, 2, 1, [50, 20]).unwrap();
    let over = gameplay_probe(&lib, &c, &m.objects[1], "house", [49, 19]);
    assert_eq!([m.objects[bat].x, m.objects[bat].y], [50, 20]);
    assert!(can_place(&g, &with_sim(&m, &sim), &lib, &over));
    // 0x41c4e0: a bridge asks no terrain kind.
    let bridge = gameplay_probe(&lib, &c, &m.objects[1], "bridge", [36, 8]);
    assert!(can_place(&g, &with_sim(&m, &sim), &lib, &bridge));
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_placement_grid_refuses_the_cells_each_site_test_refuses() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    sim.fog.visible.fill(true);
    sim.fog.visible_vertices.fill(true);
    // A lake, a standing tree, a footman and a row of walls beside the
    // castle and the peons of `setup`.
    for y in 30..40 {
        for x in 40..50 {
            g.cells[y * 64 + x] = 0;
        }
    }
    let tree = [30, 20];
    g.cells[tree[1] * 64 + tree[0]] = 0x90 | ((lib.trunks as u16 + 1) << 11);
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [25, 20]).unwrap();
    for x in 20..26 {
        let k = new_unit(&mut sim, &mut m, &lib, class::WALL_WOOD, 1, 1, [x, 12]).unwrap();
        m.objects[k].flags &= !FLAG_CONSTRUCTION;
    }
    let all = [0, 0, 64, 64];
    let mw = with_sim(&m, &sim);
    for order in ["house", "field", "bridge", "gate_wood", "harbor"] {
        let probe = gameplay_probe(&lib, &c, &m.objects[1], order, [0, 0]);
        let refused = refused_cells(&g, &mw, &lib, &probe, all);
        for y in 0..60u16 {
            for x in 0..60u16 {
                let mut probe = probe.clone();
                (probe.x, probe.y) = (x, y);
                let [w, h] = footprint(&lib, &probe).map(|n| n as i32);
                let mut site = vec![[x as i32, y as i32, x as i32 + w - 1, y as i32 + h - 1]];
                if order == "gate_wood" {
                    site.push(gate_passage(&probe));
                }
                let clear = site.iter().all(|r| {
                    (r[1]..=r[3]).all(|y| {
                        (r[0]..=r[2]).all(|x| {
                            (0..64).contains(&x)
                                && (0..64).contains(&y)
                                && !refused[y as usize * 64 + x as usize]
                        })
                    })
                });
                let placed = can_place(&g, &mw, &lib, &probe);
                // The harbour's border rule goes beyond single cells.
                if order == "harbor" {
                    assert!(!placed || clear, "{order} at {x},{y}");
                } else {
                    assert_eq!(placed, clear, "{order} at {x},{y}");
                }
            }
        }
    }
    // Out of view the grid shows only what the player knows: their castle
    // and walls, the tree and the lake, not where the footman stands now.
    let house = gameplay_probe(&lib, &c, &m.objects[1], "house", [0, 0]);
    let at = |p: [usize; 2]| p[1] * 64 + p[0];
    let unit = [m.objects[footman].x as usize, m.objects[footman].y as usize];
    assert!(refused_cells(&g, &mw, &lib, &house, all)[at(unit)]);
    sim.fog.visible.fill(false);
    sim.fog.visible_vertices.fill(false);
    let fogged = refused_cells(&g, &with_sim(&m, &sim), &lib, &house, all);
    assert!(!fogged[at(unit)]);
    assert!(fogged[at([8, 8])] && fogged[at([20, 12])]);
    assert!(fogged[at(tree)] && fogged[at([45, 35])]);
    // Unexplored ground stays unmarked, black under the fog.
    sim.fog.explored.fill(false);
    sim.fog.explored_vertices.fill(false);
    let dark = refused_cells(&g, &with_sim(&m, &sim), &lib, &house, all);
    assert!(dark.iter().all(|&r| !r));
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_building_site_holds_the_cells_of_the_finished_building() {
    let (_, lib, mut m, g, mut sim) = setup(1);
    // `e_castle.dat`: the top row is `.#####.`, the bottom one `..##.##`.
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 1, 1, [30, 30]).unwrap();
    let held = |m: &MissionWorld, sim: &Simulation| {
        let cells = movement::cells_now(sim, m, &g, &lib);
        [[30, 30], [31, 30], [36, 30], [32, 36], [34, 36], [35, 36]]
            .map(|p| cells.at(p) & pathing::OCCUPIED[1] != 0)
    };
    let shape = [false, true, false, true, false, true];
    assert_eq!(held(&m, &sim), shape);
    // 0x41ddc0 reads the picture of `+0xe8`, which the site keeps while it
    // shows its `build<W>x<H>` picture (0x41d8f0).
    m.objects[castle].flags |= FLAG_CONSTRUCTION;
    assert!(
        lib.image_for(&m.objects[castle])
            .is_some_and(|s| s.starts_with("build"))
    );
    assert_eq!(held(&m, &sim), shape);
}

/// A building of `order` as the preview places it, its top left at `at`.
fn gameplay_probe(
    lib: &ObjectLibrary,
    c: &Catalog,
    worker: &MapObject,
    order: &str,
    at: [u16; 2],
) -> MapObject {
    let mut probe = building_at(lib, c, worker, order, [100, 100]).unwrap();
    probe.x = at[0];
    probe.y = at[1];
    probe
}
/// The mission with the simulation in it, as the frontend holds it.
fn with_sim(m: &MissionWorld, sim: &Simulation) -> MissionWorld {
    let mut copy = m.clone();
    copy.simulation = Some(sim.clone());
    copy
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn tributes_move_what_the_sender_holds() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    m.kingdoms[0].resources = [300, 0, 50, 1000];
    let tribute = |to: usize, resource: u8, amount: u32| Request {
        order: "tribute".into(),
        target: Some(to),
        resource,
        amount,
        ..Default::default()
    };
    // 0x48fbb6: the amount leaves the sender and reaches the recipient...
    issue(&mut sim, &mut m, &g, &lib, &c, &tribute(2, 0, 200)).unwrap();
    assert_eq!(
        (m.kingdoms[0].resources[0], m.kingdoms[1].resources[0]),
        (100, 10200)
    );
    // ...cut to what the sender holds, nothing from an empty stock.
    issue(&mut sim, &mut m, &g, &lib, &c, &tribute(2, 2, 500)).unwrap();
    assert_eq!(
        (m.kingdoms[0].resources[2], m.kingdoms[1].resources[2]),
        (0, 10050)
    );
    issue(&mut sim, &mut m, &g, &lib, &c, &tribute(2, 1, 100)).unwrap();
    assert_eq!(m.kingdoms[1].resources[1], 10000);
    // The player's own kingdom is no recipient, nor one past the eighth.
    for to in [1, 9] {
        assert!(issue(&mut sim, &mut m, &g, &lib, &c, &tribute(to, 3, 100)).is_err());
    }
    assert_eq!(m.kingdoms[0].resources[3], 1000);
    // The sender's line is silent; the local recipient of a transfer (the
    // dice's prize comes from nobody) reads «resadded» (0x48fc6a).
    assert!(sim.gameplay.notices.is_empty());
    assert_eq!(transfer(&mut sim, &mut m, &c, 0, 1, 500, 3), 500);
    assert_eq!(m.kingdoms[0].resources[3], 1500);
    assert_eq!(sim.gameplay.notices, [c.resadded.clone()]);
    assert_eq!(transfer(&mut sim, &mut m, &c, 1, 2, 100, 4), 0);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn king_of_the_hill_with_unlimited_resources_keeps_every_stock() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    // 0x467640 / 0x4676c0 return at once in King of the Hill with «molte»
    // resources: production, gains and tributes leave the stocks alone.
    m.game_rules.game_type = 1;
    m.game_rules.resource_level = 2;
    let before = [m.kingdoms[0].resources, m.kingdoms[1].resources];
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("e_peon", vec![0]),
    )
    .unwrap();
    assert_eq!(transfer(&mut sim, &mut m, &c, 1, 2, 500, 0), 500);
    assert_eq!([m.kingdoms[0].resources, m.kingdoms[1].resources], before);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn kingdoms_sharing_their_resources_pay_for_each_other() {
    let (c, _, mut m, _, _) = setup(1);
    m.kingdoms[0].resources = [100, 0, 0, 0];
    m.kingdoms[1].resources = [500, 50, 0, 0];
    m.diplomacy[1] = 0x40;
    m.diplomacy[8] = 0x40;
    // 0x4677d0: a kingdom may spend its own resources and those of whoever
    // shares theirs with it (bit 2 of the sharer's relation toward it).
    assert_eq!(stock(&m, 1), [100, 0, 0, 0]);
    m.diplomacy[8] |= 2;
    assert_eq!(stock(&m, 1), [600, 50, 0, 0]);
    assert_eq!(stock(&m, 2), [500, 50, 0, 0]);
    // 0x4676c0: its own first, then the rest from the sharer.
    debit(&c, &mut m, 1, [250, 20, 0, 0]).unwrap();
    assert_eq!(m.kingdoms[0].resources, [0, 0, 0, 0]);
    assert_eq!(m.kingdoms[1].resources, [350, 30, 0, 0]);
    // Short of the shared stock, nothing is paid (0x4679b0).
    assert!(debit(&c, &mut m, 1, [400, 0, 0, 0]).is_err());
    assert_eq!(m.kingdoms[1].resources[0], 350);
    // A kingdom out of the game shares nothing.
    m.kingdoms[1].active = false;
    assert_eq!(stock(&m, 1), [0, 0, 0, 0]);
}
