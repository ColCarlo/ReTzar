//! Movement and pathfinding against the original data: step times from
//! `move_len`, occupancy by layer and picture masks, waits and ranges.

use super::movement::edge_cell;
use super::pathing::Cells;
use super::tests::{request, setup};
use super::*;

fn chebyshev(a: [i32; 2], b: [i32; 2]) -> i32 {
    (a[0] - b[0]).abs().max((a[1] - b[1]).abs())
}
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
fn cells(sim: &mut Simulation, m: &MissionWorld, g: &MapGrid, lib: &ObjectLibrary) -> Cells {
    movement::refresh(sim, m, g, lib);
    sim.gameplay.pathing.cells.clone()
}
/// Walk `unit` to `to` and return (first step start, last step end) in ms.
fn timed_walk(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    unit: usize,
    to: [u16; 2],
) -> (u64, u64) {
    issue(sim, m, g, lib, c, &request("move", vec![unit], to, None)).unwrap();
    let mut first = None;
    for _ in 0..400 {
        ticks(sim, m, g, lib, c, 1);
        if let Some(w) = &sim.gameplay.actors[unit].walk {
            first.get_or_insert(w.start_ms);
        }
        if sim.gameplay.actors[unit].intent.is_none() {
            break;
        }
    }
    assert_eq!([m.objects[unit].x, m.objects[unit].y], to);
    (
        first.unwrap(),
        sim.gameplay.actors[unit].walk.as_ref().unwrap().due_ms,
    )
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn steps_last_32_pixels_of_move_len_and_diagonals_as_long() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    // e_footman: move_len 50, (32000 / 50) >> 5 = 20 ms a pixel, 640 a cell.
    let section = lib.property(&m.objects[1], "image").unwrap().to_owned();
    let footman = new_unit(&mut sim, &mut m, &lib, class_of("footman"), 1, 1, [20, 20]).unwrap();
    let section_f = lib
        .property(&m.objects[footman], "image")
        .unwrap()
        .to_owned();
    assert_eq!(lib.images.get(&section_f, "move_len"), Some("50"));
    let (start, end) = timed_walk(&mut sim, &mut m, &mut g, &lib, &c, footman, [30, 20]);
    assert_eq!(end - start, 10 * 640);
    let (start, end) = timed_walk(&mut sim, &mut m, &mut g, &lib, &c, footman, [40, 30]);
    assert_eq!(end - start, 10 * 640);
    // The peasant walks with its own picture.
    let move_len: u64 = lib
        .images
        .get(&section, "move_len")
        .unwrap()
        .parse()
        .unwrap();
    let (start, end) = timed_walk(&mut sim, &mut m, &mut g, &lib, &c, 1, [14, 15]);
    assert_eq!(end - start, 10 * 32 * ((32000 / move_len) >> 5));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_castle_leaves_its_masked_cells_free() {
    let (_, lib, m, g, mut sim) = setup(1);
    // EURO CASTLE.DAT, 7×7 at (5, 5): o#####o / … / oo##### / oo##o##.
    let cells = cells(&mut sim, &m, &g, &lib);
    for p in [[5, 5], [11, 5], [5, 10], [6, 10], [5, 11], [6, 11], [9, 11]] {
        assert!(cells.free(1, p), "{p:?}");
    }
    for p in [[6, 5], [10, 5], [5, 6], [7, 10], [8, 11], [11, 11]] {
        assert!(!cells.free(1, p), "{p:?}");
    }
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn bridges_block_their_rails_and_gates_open_only_their_door() {
    let (_, lib, mut m, mut g, mut sim) = setup(1);
    // Water under a horizontal bridge at (30, 30).
    let mut ids = m.objects.iter().map(|o| o.id).collect();
    let mut bridge =
        world::starting::new_object(&mut ids, 10, class::BRIDGE, 1, 1, [30, 30], &lib).unwrap();
    bridge.data.resize(77, 0);
    bridge.data[56] = 0;
    let [w, h] = lib.cell_footprint(&bridge);
    for y in 30..30 + h {
        for x in 30..30 + w {
            g.cells[y * g.side + x] = 0;
        }
    }
    m.objects.push(bridge);
    let b = m.objects.len() - 1;
    let mid = 30 + (h / 2) as i32;
    let cells = cells(&mut sim, &m, &g, &lib);
    assert!(cells.free(1, [33, mid]));
    assert!(!cells.free(1, [33, 30]));
    assert!(!cells.free(1, [33, 30 + h as i32 - 1]));
    // Ships pass under it.
    assert!(cells.free(2, [33, 30]));
    m.objects[b].flags |= FLAG_CONSTRUCTION;
    let cells = super::movement_tests::cells(&mut sim, &m, &g, &lib);
    assert!(!cells.free(1, [33, mid]));

    let mut gate =
        world::starting::new_object(&mut ids, 10, class::GATE_STONE, 1, 1, [20, 40], &lib).unwrap();
    gate.data.resize(77, 0);
    gate.data[56] = 0;
    gate.data[57] = 1;
    m.objects.push(gate);
    let k = m.objects.len() - 1;
    let closed = super::movement_tests::cells(&mut sim, &m, &g, &lib);
    m.objects[k].data[57] = 0;
    let open = super::movement_tests::cells(&mut sim, &m, &g, &lib);
    for x in 20..27 {
        assert!(!closed.free(1, [x, 40]));
        assert_eq!(open.free(1, [x, 40]), (22..=24).contains(&x), "{x}");
    }
    // Bit 30: the door always (0x4562cb), the two rows beside it facing
    // down only while open (0x4577b4, 0x457ac0).
    let gateway = |c: &Cells, p: [i32; 2]| c.at(p) & pathing::GATEWAY != 0;
    for x in 20..27 {
        let door = (22..=24).contains(&x);
        assert_eq!(gateway(&closed, [x, 40]), door, "{x}");
        assert_eq!(gateway(&open, [x, 40]), door, "{x}");
        for y in [41, 42] {
            assert!(!gateway(&closed, [x, y]), "{x},{y}");
            assert_eq!(gateway(&open, [x, y]), door, "{x},{y}");
        }
        assert!(!gateway(&open, [x, 43]));
    }
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn layers_hold_their_own_units() {
    let (_, lib, mut m, mut g, mut sim) = setup(1);
    for y in 40..50 {
        for x in 40..50 {
            g.cells[y * g.side + x] = 0;
        }
    }
    let ship = new_unit(&mut sim, &mut m, &lib, class_of("galleon"), 1, 1, [45, 45]).unwrap();
    let dragon = new_unit(&mut sim, &mut m, &lib, class_of("dragon"), 1, 1, [20, 20]).unwrap();
    assert_eq!(lib.layer(&m.objects[ship]), 2);
    assert_eq!(lib.layer(&m.objects[dragon]), 0);
    // The jinn has no `teren`: it walks on land.
    let jinn = new_unit(&mut sim, &mut m, &lib, class_of("jinn"), 1, 1, [22, 20]).unwrap();
    assert_eq!(lib.layer(&m.objects[jinn]), 1);
    let cells = cells(&mut sim, &m, &g, &lib);
    assert!(!cells.free(2, [45, 45]));
    assert!(!cells.free(0, [20, 20]));
    assert!(cells.free(1, [20, 20]));
    assert!(!cells.free(1, [22, 20]));
    assert!(cells.free(0, [22, 20]));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn an_archer_stops_at_its_native_range() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    // A target that stays: its kingdom without a computer player, whose
    // idle would move it (0x4140b0).
    sim.gameplay.ai.players.remove(&2);
    let archer = new_unit(&mut sim, &mut m, &lib, class_of("archer"), 1, 1, [14, 20]).unwrap();
    let target = new_unit(&mut sim, &mut m, &lib, class_of("footman"), 2, 1, [34, 20]).unwrap();
    sim.gameplay.actors[target].stance = 1;
    let range = c.stats(&m.objects[archer], "archer", &[]).range;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("attack", vec![archer], [34, 20], Some(target)),
    )
    .unwrap();
    for _ in 0..400 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if sim.objects[archer].order == OrderState::Attacking {
            break;
        }
    }
    // Half cells from the archer's centre to the 3×3 rect: (2·(dx−1))² ≤ 4·r².
    assert_eq!(m.objects[target].x - m.objects[archer].x, range as u16 + 1);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_faster_unit_waits_behind_a_slower_one_in_a_corridor() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    // A corridor along row 30.
    for x in 8..45 {
        for y in [29, 31] {
            g.cells[y * g.side + x] = 0;
        }
    }
    let leader = new_unit(&mut sim, &mut m, &lib, class_of("footman"), 1, 1, [12, 30]).unwrap();
    // A faster knight catches up with the footman and waits behind it.
    let follower = new_unit(&mut sim, &mut m, &lib, class_of("knight"), 1, 1, [10, 30]).unwrap();
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("move", vec![leader], [40, 30], None),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    // The first search ignores the leader on the move.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("move", vec![follower], [38, 30], None),
    )
    .unwrap();
    let mut waited = false;
    for _ in 0..800 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        assert_eq!(m.objects[follower].y, 30);
        waited |= sim.gameplay.actors[follower].waiting_for.is_some();
        if [follower, leader]
            .iter()
            .all(|&k| sim.gameplay.actors[k].intent.is_none())
        {
            break;
        }
    }
    assert_eq!([m.objects[follower].x, m.objects[follower].y], [38, 30]);
    assert_eq!([m.objects[leader].x, m.objects[leader].y], [40, 30]);
    assert!(waited);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_group_keeps_its_formation_along_one_shifted_path() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let mut block = Vec::new();
    for y in 30..33 {
        for x in 20..23 {
            block
                .push(new_unit(&mut sim, &mut m, &lib, class_of("footman"), 1, 1, [x, y]).unwrap());
        }
    }
    let before: Vec<[i32; 2]> = block
        .iter()
        .map(|&k| [m.objects[k].x as i32, m.objects[k].y as i32])
        .collect();
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("move", block.clone(), [42, 31], None),
    )
    .unwrap();
    for _ in 0..800 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if block
            .iter()
            .all(|&k| sim.gameplay.actors[k].intent.is_none())
        {
            break;
        }
    }
    // The first unit searches from the block's centre (21, 31) while the
    // others still stand around it: that search fails (0x451edb) and it
    // walks to the point itself. The others share the path found next and
    // keep their places, shifted by the point less the centre (0x451f78).
    let after: Vec<[i32; 2]> = block
        .iter()
        .map(|&k| [m.objects[k].x as i32, m.objects[k].y as i32])
        .collect();
    // Where two make for one cell, the later stops beside it.
    let shift = [42 - 21, 31 - 31];
    let target = |k: usize| {
        if k == 0 {
            [42, 31]
        } else {
            [before[k][0] + shift[0], before[k][1] + shift[1]]
        }
    };
    assert!(
        (0..9).all(|k| chebyshev(after[k], target(k)) <= 1),
        "{after:?}"
    );
    assert!(
        (0..9).filter(|&k| after[k] == target(k)).count() >= 6,
        "{after:?}"
    );
    let distinct: BTreeSet<_> = after.iter().collect();
    assert_eq!(distinct.len(), 9);
    assert_eq!(sim.gameplay.groups.len(), 1);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_worker_that_stops_beside_its_mine_leaves_no_transit_behind() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let mine = new_unit(&mut sim, &mut m, &lib, class::RES_GOLD, 0, 0, [22, 8]).unwrap();
    m.objects[mine].data.resize(64, 0);
    m.objects[mine].data[58..62].copy_from_slice(&1000u32.to_le_bytes());
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("harvest", vec![1], [22, 8], Some(mine)),
    )
    .unwrap();
    let mut working = false;
    for _ in 0..400 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if sim.objects[1].order == OrderState::Working {
            working = true;
            break;
        }
    }
    assert!(working);
    // The Move op's cleanup (0x4e334c): once the program goes on to mine,
    // the cell stays occupied but no longer in transit, so a peasant behind
    // searches around it at once instead of waiting 2.5 s.
    assert!(sim.gameplay.actors[1].movement.is_none());
    let here = [m.objects[1].x as i32, m.objects[1].y as i32];
    let cells = cells(&mut sim, &m, &g, &lib);
    assert_ne!(cells.at(here) & pathing::OCCUPIED[1], 0);
    assert_eq!(cells.at(here) & pathing::TRANSIT[1], 0);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_mine_that_gives_its_last_load_dies_and_frees_its_cells() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let mine = new_unit(&mut sim, &mut m, &lib, class::RES_GOLD, 0, 0, [22, 8]).unwrap();
    m.objects[mine].data.resize(64, 0);
    m.objects[mine].data[58..62].copy_from_slice(&15u32.to_le_bytes());
    let inside = [23, 9];
    assert_ne!(
        cells(&mut sim, &m, &g, &lib).at(inside) & pathing::OCCUPIED[1],
        0
    );
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("harvest", vec![1], [22, 8], Some(mine)),
    )
    .unwrap();
    for _ in 0..6000 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if m.objects[mine].flags & crate::world::FLAG_DEAD != 0 {
            break;
        }
    }
    // 0x4affab: the pit stays, dead, on the ground and walkable.
    let o = &m.objects[mine];
    assert_ne!(o.flags & crate::world::FLAG_DEAD, 0);
    assert_eq!(o.flags & crate::world::FLAG_INACTIVE, 0);
    assert_eq!(o.dword(58), 0);
    assert!(!o.selectable());
    assert_eq!(crate::world::presentation_priority(o), 1);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(
        cells(&mut sim, &m, &g, &lib).at(inside) & pathing::OCCUPIED[1],
        0
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_ninja_jumps_one_or_two_cells_of_cliff_but_never_three() {
    for thick in 1..=3usize {
        let (c, lib, mut m, mut g, mut sim) = setup(3);
        // A cliff from edge to edge: only climbing crosses it.
        for y in 0..g.side {
            for x in 30..30 + thick {
                g.cells[y * g.side + x] |= 0x400;
            }
        }
        let ninja = new_unit(&mut sim, &mut m, &lib, class::NINJA, 1, 3, [27, 20]).unwrap();
        let to = [33 + thick as u16, 20];
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("move", vec![ninja], to, None),
        )
        .unwrap();
        let mut jumps = Vec::new();
        for _ in 0..600 {
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
            if let Some(w) = &sim.gameplay.actors[ninja].walk
                && w.jump > 0
                && jumps.last() != Some(&(w.from, w.start_ms))
            {
                // 0x4ebc00: the step skips the cliff; a_ninja_jump has 22
                // rows and move_len 44, so 22 steps of 22 ms, and the Move op
                // waits 32 of them.
                assert_eq!((w.jump, w.step_ms, w.due_ms - w.start_ms), (22, 22, 704));
                let here = [m.objects[ninja].x, m.objects[ninja].y];
                assert_eq!(chebyshev(at(w.from), at(here)), thick as i32 + 1);
                jumps.push((w.from, w.start_ms));
            }
            if sim.gameplay.actors[ninja].intent.is_none() {
                break;
            }
        }
        let here = [m.objects[ninja].x, m.objects[ninja].y];
        if thick < 3 {
            assert_eq!(here, to, "{thick}");
            assert_eq!(jumps.len(), 1, "{thick}");
        } else {
            // 0x4a99a0 refuses a third cell of cliff.
            assert!(here[0] < 30, "{thick}");
            assert!(jumps.is_empty());
        }
    }
}

fn at(p: [u16; 2]) -> [i32; 2] {
    [p[0] as i32, p[1] as i32]
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_group_sent_at_an_object_spreads_over_its_edge() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let mut squad = Vec::new();
    for y in 30..32 {
        for x in 20..22 {
            squad
                .push(new_unit(&mut sim, &mut m, &lib, class_of("footman"), 1, 1, [x, y]).unwrap());
        }
    }
    let enemy = new_unit(&mut sim, &mut m, &lib, class_of("footman"), 2, 1, [44, 30]).unwrap();
    sim.gameplay.actors[enemy].stance = 2;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("attack", squad.clone(), [44, 30], Some(enemy)),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    let id = sim.gameplay.actors[squad[0]].group.expect("one group");
    assert!(
        squad
            .iter()
            .all(|&k| sim.gameplay.actors[k].group == Some(id))
    );
    // 0x451cc0: one shared path toward the enemy, cut eight cells from it;
    // each soldier then takes a different free cell of the enemy's 3×3 edge.
    let paths = &sim.gameplay.groups[&id].paths;
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].target, Some(enemy));
    let taken: BTreeSet<[i32; 2]> = paths[0].taken.iter().map(|&(p, _)| p).collect();
    assert_eq!(taken.len(), 4, "{:?}", paths[0].taken);
    assert!(taken.iter().all(|p| chebyshev(*p, [44, 30]) == 1));
    for &k in &squad {
        let path = &sim.gameplay.actors[k].path;
        let end = path.back().map(|p| [p[0] as i32, p[1] as i32]).unwrap();
        assert!(taken.contains(&end));
        // The shared part ends before eight cells from the enemy.
        let joined = path
            .iter()
            .position(|p| chebyshev([p[0] as i32, p[1] as i32], [44, 30]) < 8)
            .unwrap();
        assert!(joined > 0);
    }
    // The edge cells are tried from where the shared path ends, alternately
    // ahead and behind (0x451b40).
    assert_eq!(edge_cell([1, 1, 3, 3], [1, 2], 0), [1, 2]);
    assert_eq!(edge_cell([1, 1, 3, 3], [1, 2], 1), [1, 1]);
    assert_eq!(edge_cell([1, 1, 3, 3], [1, 2], 2), [1, 3]);
    assert_eq!(edge_cell([1, 1, 3, 3], [1, 2], 3), [2, 1]);
    assert_eq!(edge_cell([1, 1, 3, 3], [1, 2], 4), [2, 3]);
}
