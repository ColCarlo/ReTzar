//! The manager's places against its native routines (0x402760, 0x404d00,
//! 0x403210, 0x403840, 0x403be0, 0x405070, 0x403e10), in the test world:
//! kingdom 1 with a castle at (5, 5) and three peasants at (14–16, 5),
//! kingdom 2 a computer player with nothing.
use super::*;
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
fn manager(sim: &Simulation) -> &Manager {
    sim.gameplay.ai.manager.as_ref().unwrap()
}
fn places(sim: &Simulation) -> Vec<&Place> {
    manager(sim).places.iter().flatten().collect()
}
fn of_kind(sim: &Simulation, k: u8) -> Vec<&Place> {
    places(sim).into_iter().filter(|p| p.kind == k).collect()
}
/// 0x4048e0: the (2n + 1)×(2n + 1) sectors round the sector of the object's
/// centre (0x515bf0), cut at the edges of the 64-cell map.
fn around(lib: &ObjectLibrary, o: &MapObject, n: i32) -> [i32; 4] {
    let f = crate::gameplay::movement::footprint_rect(lib, o);
    let c = [(f[0] + f[2]) / 2 / 8, (f[1] + f[3]) / 2 / 8];
    [
        ((c[0] - n) * 8).max(0),
        ((c[1] - n) * 8).max(0),
        ((c[0] + n) * 8 + 7).min(63),
        ((c[1] + n) * 8 + 7).min(63),
    ]
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_start_makes_a_home_round_each_castle_and_a_place_for_each_mine() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let mine = new_unit(&mut sim, &mut m, &lib, class::RES_STONE, 0, 1, [40, 12]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // 0x404d00: the castle's Home spans 5 × 5 sectors round the sector of
    // its centre, cut at the map's edge; the peasants are inside.
    let homes = of_kind(&sim, kind::HOME);
    assert_eq!(homes.len(), 1);
    let home = homes[0];
    assert_eq!(
        (home.param, home.anchor, home.rect),
        (1, Some(0), around(&lib, &m.objects[0], 2))
    );
    assert_eq!(home.present[1].count(mask::CIVIL), 3);
    assert_eq!(home.present[1].count(mask::BUILDINGS), 1);
    // The mine: 3 × 3 sectors, its resource as param (1 stone).
    let res = of_kind(&sim, kind::RESOURCE);
    assert_eq!(res.len(), 1);
    assert_eq!(
        (res[0].param, res[0].anchor, res[0].rect),
        (1, Some(mine), around(&lib, &m.objects[mine], 1))
    );
    // The totals of kingdom 1 (0x402760).
    let t = manager(&sim).totals[1];
    assert_eq!((t.count(mask::CIVIL), t.count(mask::BUILDINGS)), (3, 1));
    // Every place's id is its slot + 1 and the anchors carry the flag.
    for (slot, p) in manager(&sim).places.iter().enumerate() {
        if let Some(p) = p {
            assert_eq!(p.id as usize, slot + 1);
        }
    }
    assert!(manager(&sim).anchors.contains(&0) && manager(&sim).anchors.contains(&mine));
}

#[test]
#[ignore = "Requires installed original archives"]
fn units_away_make_their_place_and_leave_it_for_the_home() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [50, 50]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let units = of_kind(&sim, kind::UNITS);
    assert_eq!(units.len(), 1);
    assert_eq!(
        (units[0].param, units[0].anchor, units[0].rect),
        (1, Some(footman), around(&lib, &m.objects[footman], 1))
    );
    assert_eq!(units[0].present[1].count(mask::MILITARY), 1);
    // On its way home it counts in the Home's incoming (0x403be0).
    sim.gameplay.actors[footman].intent = Some(Intent::Move([10, 10]));
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let home = of_kind(&sim, kind::HOME)[0];
    assert_eq!(home.incoming[1].count(mask::MILITARY), 1);
    // Once in the Home's sectors it stands there; the Units place, left
    // without a unit of its own, is gone (0x403840, 0x405070).
    m.objects[footman].x = 10;
    m.objects[footman].y = 10;
    sim.gameplay.actors[footman].intent = None;
    sim.gameplay.actors[footman].goal = None;
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert!(of_kind(&sim, kind::UNITS).is_empty());
    let home = of_kind(&sim, kind::HOME)[0];
    assert_eq!(home.present[1].count(mask::MILITARY), 1);
    assert_eq!(home.incoming[1].count(mask::MILITARY), 0);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_loss_where_no_place_stands_makes_a_place_of_losses() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let wall = new_unit(&mut sim, &mut m, &lib, class::WALL_STONE, 2, 1, [40, 8]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let killer = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [40, 26]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // Walls make no place of their own; the killer's Units place does not
    // reach the wall's sector.
    assert!(manager(&sim).sector_places([5, 1]).is_empty());
    died(&mut sim, &m, &lib, &c, wall, killer);
    // 0x403e10: the sectors of both, widened by one where one wide, with
    // kingdom 2's bit; such a place has no anchor.
    let losses = of_kind(&sim, kind::LOSSES);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        (losses[0].param, losses[0].anchor, losses[0].rect),
        (2, None, [32, 8, 55, 31])
    );
    assert_eq!(manager(&sim).losses[2].count(mask::BUILDINGS), 1);
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_manager_travels_with_the_save() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let saved = serde_json::to_string(&sim.gameplay.ai).unwrap();
    let back: crate::gameplay::ai::State = serde_json::from_str(&saved).unwrap();
    assert_eq!(back, sim.gameplay.ai);
    assert!(back.manager.is_some());
}

#[test]
#[ignore = "Requires installed original archives"]
fn every_player_keeps_a_record_of_every_place() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let mine = new_unit(&mut sim, &mut m, &lib, class::RES_GOLD, 0, 1, [40, 40]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // 0x40fc60: each new place at the head of the list; before the player
    // knows its home nothing is evaluated (0x40fdd0), so the order stays.
    let slots: Vec<usize> = manager(&sim)
        .places
        .iter()
        .enumerate()
        .filter_map(|(s, p)| p.as_ref().map(|_| s))
        .collect();
    let player = &sim.gameplay.ai.players[&2];
    let listed: Vec<usize> = player.records.iter().map(|r| r.place).collect();
    assert_eq!(listed, slots.iter().rev().copied().collect::<Vec<_>>());
    assert!(
        player
            .records
            .iter()
            .all(|r| r.priority == 0 && r.stamp == 0)
    );
    // With a home, the castle's Home of an enemy: priority (3 size − 3
    // distance) / 2; no threat, so the minimum is a fifth of its civil and
    // other buildings and the wanted twice them (0x40fdd0).
    let home = manager(&sim)
        .places
        .iter()
        .position(|p| p.as_ref().is_some_and(|p| p.kind == kind::HOME))
        .unwrap();
    let mgr = sim.gameplay.ai.manager.clone().unwrap();
    let player = sim.gameplay.ai.players.get_mut(&2).unwrap();
    player.home = Some([60, 60]);
    for at in 0..player.records.len() {
        crate::gameplay::ai::records::evaluate(player, 2, at, &mgr, &m, 64);
    }
    let r = *player.records.iter().find(|r| r.place == home).unwrap();
    let place = mgr.place(home).unwrap();
    let distance = (60 - place.rect[2] - 1).max(60 - place.rect[3] - 1) as u32;
    assert_eq!(
        (r.distance, r.priority),
        (distance, (3 * 64 - 3 * distance) / 2)
    );
    let rest = place.present[1].strength(mask::CIVIL | mask::BUILDINGS);
    assert_eq!((r.minimum, r.wanted, r.flags), (rest / 5, rest * 2, 0));
    assert_eq!(r.hostile, place.present[1]);
    // The mine, which the player does not work: size − distance.
    let res = manager(&sim)
        .places
        .iter()
        .position(|p| p.as_ref().is_some_and(|p| p.anchor == Some(mine)))
        .unwrap();
    let r = sim.gameplay.ai.players[&2]
        .records
        .iter()
        .find(|r| r.place == res)
        .copied()
        .unwrap();
    assert_eq!(r.priority, 64 - r.distance);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_place_gone_leaves_the_records() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [50, 50]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let before = sim.gameplay.ai.players[&2].records.len();
    m.objects[footman].x = 10;
    m.objects[footman].y = 10;
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // 0x40fcc0: the Units place died with its unit gone home.
    assert_eq!(sim.gameplay.ai.players[&2].records.len(), before - 1);
    let mgr = manager(&sim);
    assert!(
        sim.gameplay.ai.players[&2]
            .records
            .iter()
            .all(|r| mgr.place(r.place).is_some_and(|p| p.serial == r.serial))
    );
}
