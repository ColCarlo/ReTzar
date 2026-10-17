//! The computer's breaches in the walls: the hook on its land units' path
//! searches that marks a stretch of wall and sends the attack on it
//! (0x4171e0), the blows on it (0x4dfb84) and the marks taken off when a
//! marked wall falls (0x402c50, 0x417190).
use super::rules::World;
use super::*;
use crate::gameplay::pathing::{self, Cost, Query, WALL};

/// A path this short is kept as it is (0x41723c).
const SHORT: usize = 8;
/// A target this far is not worth a breach (0x41725e).
const FAR: u32 = 64;
/// The marked walls looked for round the first wall cell (0x417326), and
/// the walls marked when there is none (0x417389).
const LOOK: i32 = 12;
const MARK: i32 = 1;
/// The marks taken off round a marked wall that falls (0x402ca5).
const CLEAR: i32 = 16;

/// Walls (the flag 0x1000).
pub(crate) fn wall(o: &MapObject) -> bool {
    matches!(o.class, class::WALL_STONE | class::WALL_WOOD)
}

/// 0x44e1a0: the objects whose cells meet the rect, by the engine's 8×8
/// sectors: columns from the west, each from the north, each list from its
/// latest arrival.
pub(super) fn meeting(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    rect: [i32; 4],
) -> Vec<usize> {
    let mut out: Vec<usize> = (0..m.objects.len())
        .filter(|&j| {
            let o = &m.objects[j];
            if !alive(o)
                || sim
                    .gameplay
                    .actors
                    .get(j)
                    .is_none_or(|a| a.sector.is_none())
            {
                return false;
            }
            let r = movement::footprint_rect(lib, o);
            r[0] <= rect[2] && r[2] >= rect[0] && r[1] <= rect[3] && r[3] >= rect[1]
        })
        .collect();
    out.sort_by_key(|&j| sector_order(sim, j));
    out
}
/// The walls round the cell that `k` attacks on its own (0x417110,
/// 0x468190: relation 0x10 with their kingdom).
fn hostile_walls(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    k: u8,
    c: [i32; 2],
    r: i32,
) -> Vec<usize> {
    meeting(sim, m, lib, [c[0] - r, c[1] - r, c[0] + r, c[1] + r])
        .into_iter()
        .filter(|&j| {
            let o = &m.objects[j];
            wall(o) && relation(m, k, o.owner) & 0x10 != 0
        })
        .collect()
}

/// 0x4171e0, after the path search of a computer player's land unit: the
/// path it gets. A path that reaches its goal (only the last node is
/// tested) is kept when short, when the target is 64 cells or more away,
/// or when shorter than twice that gap; otherwise a search that crosses the
/// walls for 1 a cell (0x4170d0, run on the request as the first search
/// left it: the range squared) is tried, and the first path kept when at
/// most half as long again. Past that a path that does not reach its goal
/// is none at all, unless the unit, attacking, is sent against a wall: a
/// hostile wall already marked within 12 cells of the first wall cell of
/// the new path, or, when none is marked, the wall on that cell, and the
/// hostile walls beside it are marked. The program (vt+0x2b0 at priority
/// 4000) is posted (vt+0x164) and the class of the wall becomes the unit's
/// attack type (vt+0x25c).
pub(crate) fn path(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    q: &Query,
    path: Option<Vec<[u16; 2]>>,
) -> Option<Vec<[u16; 2]>> {
    let side = sim.gameplay.pathing.cells.side;
    let at = |p: [u16; 2]| [p[0] as i32, p[1] as i32];
    let whole = path
        .as_ref()
        .is_some_and(|p| p.last().is_none_or(|&n| pathing::arrives(side, q, at(n))));
    let len = path.as_ref().map_or(0, Vec::len);
    if whole && len < SHORT {
        return path;
    }
    let gap = records::gap(q.start, pathing::clamped(side, q.rect));
    if gap >= FAR || whole && len < 2 * gap as usize {
        return path;
    }
    let through = Query {
        range: q.range * q.range,
        cost: Cost::Breach,
        ..*q
    };
    let p = &mut sim.gameplay.pathing;
    let Some(through) = pathing::search(&p.cells, &mut p.regions, &through) else {
        return path;
    };
    if whole && len <= through.len() * 3 / 2 {
        return path;
    }
    let none = if whole { path.clone() } else { None };
    let cells = &sim.gameplay.pathing.cells;
    let Some(c) = through
        .iter()
        .map(|&n| at(n))
        .find(|&n| cells.at(n) & WALL != 0)
    else {
        return none;
    };
    let k = m.objects[i].owner;
    let near = hostile_walls(sim, m, lib, k, c, LOOK);
    let marked = near
        .iter()
        .copied()
        .find(|&j| sim.gameplay.actors[j].marked);
    if marked.is_none() {
        for j in hostile_walls(sim, m, lib, k, c, MARK) {
            sim.gameplay.actors[j].marked = true;
        }
    }
    if near.is_empty() || program(sim, m, i).kind != program::ATTACK {
        return none;
    }
    // 0x44e400 with 0x44ea20: any wall on the cell.
    let target = marked.or_else(|| {
        (0..m.objects.len()).find(|&j| {
            let o = &m.objects[j];
            let r = movement::footprint_rect(lib, o);
            alive(o) && wall(o) && c[0] >= r[0] && c[0] <= r[2] && c[1] >= r[1] && c[1] <= r[3]
        })
    });
    let Some(target) = target else {
        return none;
    };
    sim.gameplay.ai.breaches.push((i, target));
    sim.gameplay.actors[i].attack_type = Some(m.objects[target].class);
    Some(path.unwrap_or_default())
}

/// The programs the breaches posted (message 0x4014), taken by their units
/// when the object's update ends: the unit's program against the wall
/// (vt+0x2b0, an attack or a spell) at priority 4000, with the hook on a
/// new program.
pub(crate) fn orders(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
) {
    for (i, target) in std::mem::take(&mut sim.gameplay.ai.breaches) {
        let o = &m.objects[i];
        if !alive(o) || o.flags & FLAG_DYING != 0 {
            continue;
        }
        let k = o.owner;
        let intent = units::target_program(sim, m, lib, c, i, target);
        if set_order(sim, i, intent, false).is_err() {
            continue;
        }
        let Some(mut p) = sim.gameplay.ai.players.get_mut(&k).map(std::mem::take) else {
            continue;
        };
        if let Some(slot) = sim.gameplay.ai.players.get_mut(&k) {
            slot.mode = p.mode;
        }
        let mut w = World {
            sim,
            m,
            g,
            lib,
            c,
            cells: None,
        };
        p.hooked(&mut w, i);
        if let Some(slot) = w.sim.gameplay.ai.players.get_mut(&k) {
            *slot = p;
        }
    }
}

/// 0x4dfb84: a computer's unit striking a wall under an attack of priority
/// 4000 looks for no better target before its blows.
pub(crate) fn on_wall(sim: &Simulation, m: &MissionWorld, i: usize, j: usize) -> bool {
    let o = &m.objects[i];
    o.flags & (FLAG_STORY | FLAG_DYING) == 0
        && controls(sim, o.owner)
        && m.objects.get(j).is_some_and(wall)
        && program(sim, m, i).priority == 4000
}

/// 0x402c50 → 0x417190: a marked wall leaves the map; the walls within 16
/// cells of it lose their marks, and the units there attacking a wall go
/// down to priority 3000.
pub(crate) fn fallen(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, i: usize) {
    let o = &m.objects[i];
    if !wall(o) || !std::mem::take(&mut sim.gameplay.actors[i].marked) {
        return;
    }
    let (x, y) = (o.x as i32, o.y as i32);
    for j in meeting(sim, m, lib, [x - CLEAR, y - CLEAR, x + CLEAR, y + CLEAR]) {
        let u = &m.objects[j];
        if wall(u) {
            sim.gameplay.actors[j].marked = false;
        } else if crate::selection::unit(u)
            && let Some(Intent::Attack(t)) = sim.gameplay.actors[j].intent
            && m.objects.get(t).is_some_and(wall)
        {
            sim.gameplay.actors[j].automatic = true;
        }
    }
}

#[cfg(test)]
#[path = "breach_tests.rs"]
mod tests;
