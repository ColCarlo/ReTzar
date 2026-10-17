//! A computer player's transports: an order across the water set up with a
//! ship (0x415f80, 0x415ec0), the ship's program Transport (0x2b, 0x430200,
//! 0x430400) and its passengers' Use transport (0x2c, 0x4306a0).
use super::rules::World;
use super::*;
use crate::gameplay::islands::{LAND, NONE, WIPED, link};

/// The rings round a point looked at for its land region (0x4303af).
const LANDING_RINGS: i32 = 10;
/// Passengers step off within this many rings of the ship (0x4e90e3).
const UNLOAD_RINGS: i32 = 4;
/// The ship and its passenger walk to within this of each other (0x430267,
/// 0x4306ea).
const MEET: u32 = 2;
/// The waits of the program, in ms (0x430241, 0x4302bb, 0x4303e9,
/// 0x430721).
const FIRST_WAIT: u64 = 1;
const WAIT: u64 = 1000;
/// The radius of a war transport round the place's anchor (0x415f3f).
const WAR_RADIUS: u16 = 0x20;
/// ReTzar's own: the waits or failed walks for one passenger after which
/// the ship gives it up (the original waits for ever).
const PATIENCE: u16 = 60;

/// A ship's program Transport: it fetches its passengers one at a time,
/// then sails to the point and lands them on its land region. Steps as the
/// program's ops: 1 a first wait, 2 Pick passenger, 3 the walk to it, 4
/// Wait passenger, 5 a wait, 6 the walk to the point, 7 Unload, 8 a wait;
/// for a group sent to a place, 9 Check approach distance, 10 the walk to
/// the place and 11 Wait others.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Transport {
    pub point: [u16; 2],
    /// The land region it lands on (the Unload op's argument, +0x11e).
    pub landing: u8,
    /// Its passengers, in the order they were given (+0x26).
    pub passengers: Vec<usize>,
    pub step: u8,
    /// The end of a wait, in game ms.
    pub due: u64,
    /// The passenger it goes to (the op's target).
    pub fetching: Option<usize>,
    pub patience: u16,
    /// The place of the group it carries (0x430400).
    pub war: Option<War>,
}
/// The place a war transport goes to, and its wait there.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct War {
    pub place: usize,
    pub serial: u64,
    pub sector: Option<[u16; 2]>,
}
/// A passenger's program Use transport: the walk to the ship, aboard, and,
/// when the order asks, the walk to its point once landed. Steps: 1 the
/// walk to the ship, 2 Check transporter, 3 a wait, 4 EnterHolder, 5 a
/// wait, 6 InHolder, 7 the walk to the point, 8 a wait.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Ride {
    pub ship: usize,
    pub point: [u16; 2],
    pub walk: bool,
    pub step: u8,
    pub due: u64,
}

fn valid(r: u8) -> bool {
    r != NONE && r != WIPED
}
/// The land region a ship lands on for a point: the point's, or the first
/// land within 10 rings (0x430323).
fn landing(sim: &Simulation, g: &MapGrid, p: [u16; 2]) -> u8 {
    let isl = &sim.gameplay.islands;
    let [x, y] = [p[0] as i32, p[1] as i32];
    let here = isl.region([x, y]);
    if valid(here) && isl.kind(here) == LAND {
        return here;
    }
    for r in 1..=LANDING_RINGS {
        let found = isl.nearest(g, [x - r, y - r, x + r, y + r], LAND);
        if found != NONE {
            return found;
        }
    }
    NONE
}

/// 0x415f80: the order of a computer player's unit reaches its point on
/// the unit's island (0), across the water with a ship of the player (1:
/// the unit and the ship given their programs) or not at all (2). The
/// unit's region is its cell's, or the first round it.
pub(super) fn organise(
    w: &mut World,
    p: &mut Player,
    i: usize,
    order: &Intent,
    point: [u16; 2],
    dest: u8,
    walk: bool,
) -> u8 {
    let o = &w.m.objects[i];
    let isl = &w.sim.gameplay.islands;
    let at = isl.region([o.x as i32, o.y as i32]);
    let here = if valid(at) {
        at
    } else {
        isl.nearest(w.g, movement::target_rect(w.sim, w.m, w.lib, i), 0)
    };
    let dest = if dest == NONE {
        isl.region([point[0] as i32, point[1] as i32])
    } else {
        dest
    };
    if w.sim.gameplay.islands.same_island(here, dest) {
        return 0;
    }
    let isl = &w.sim.gameplay.islands;
    if isl.kind(here) != LAND || isl.kind(dest) != LAND {
        return 2;
    }
    let links = isl.links(here, dest);
    if links & (link::ADJACENT | link::BRIDGE) != 0 || links & link::BY_WATER == 0 {
        return 2;
    }
    match reach::ship(w.sim, w.m, w.lib, w.c, p, here, dest, Some(i)) {
        Some(ship) => {
            board(w, i, ship, order, point, walk);
            1
        }
        None => 2,
    }
}
/// 0x415ec0: the unit takes Use transport at priority 4000, its order kept
/// for after; a ship not yet on a Transport takes one to the point, for
/// the place when the order is a group's Approach; the unit joins the
/// ship's passengers (0x415e20).
fn board(w: &mut World, i: usize, ship: usize, order: &Intent, point: [u16; 2], walk: bool) {
    let ride = Ride {
        ship,
        point,
        walk,
        step: 1,
        due: 0,
    };
    if set_order(w.sim, i, Intent::UseTransport(ride), false).is_err() {
        return;
    }
    w.sim.gameplay.actors[i].resume = Some(order.clone());
    if !matches!(
        w.sim.gameplay.actors[ship].intent,
        Some(Intent::Transport(_))
    ) {
        let war = match order {
            Intent::Approach(ap) => Some(War {
                place: ap.place,
                serial: ap.serial,
                sector: None,
            }),
            _ => None,
        };
        let t = Transport {
            point,
            landing: landing(w.sim, w.g, point),
            step: 1,
            war,
            ..Default::default()
        };
        let _ = set_order(w.sim, ship, Intent::Transport(Box::new(t)), false);
    }
    if let Some(Intent::Transport(t)) = w.sim.gameplay.actors[ship].intent.as_mut() {
        t.passengers.push(i);
    }
}

/// A Transport still loading (steps up to 5, or 8) for a landing on the
/// island of `land`, with room for its passengers as well (0x416dd4).
pub(super) fn loading_for(sim: &mut Simulation, ship: usize, land: u8, room: usize) -> bool {
    let Some(Intent::Transport(t)) = &sim.gameplay.actors[ship].intent else {
        return false;
    };
    if t.step > 5 && t.step != 8 {
        return false;
    }
    let (landing, waiting) = (t.landing, t.passengers.len());
    sim.gameplay.islands.same_island(landing, land)
        && waiting + sim.objects[ship].contents.len() < room
}

/// The passenger still comes to the ship: there, not aboard, its program
/// Use transport for it.
fn coming(sim: &Simulation, m: &MissionWorld, ship: usize, j: usize) -> bool {
    m.objects.get(j).is_some_and(alive)
        && m.objects[j].flags & FLAG_DYING == 0
        && sim.objects[j].holder.is_none()
        && matches!(sim.gameplay.actors[j].intent, Some(Intent::UseTransport(r)) if r.ship == ship)
}
fn now(sim: &Simulation) -> u64 {
    sim.ticks * TICK_MS
}
/// A wait of the program: true once over.
fn waited(sim: &Simulation, due: &mut u64, ms: u64) -> bool {
    if *due == 0 {
        *due = now(sim) + ms;
        return false;
    }
    if now(sim) < *due {
        return false;
    }
    *due = 0;
    true
}

/// The ship's Transport, one step.
pub(crate) fn transport(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    mut t: Transport,
) {
    match t.step {
        1 => {
            if waited(sim, &mut t.due, FIRST_WAIT) {
                t.step = 2;
            }
        }
        // Op 0x53: the first passenger listed that still comes.
        2 => match t.passengers.iter().copied().find(|&j| coming(sim, m, i, j)) {
            Some(j) => {
                if t.fetching != Some(j) {
                    t.patience = 0;
                }
                t.fetching = Some(j);
                t.step = 3;
            }
            None => t.step = if t.war.is_some() { 9 } else { 6 },
        },
        3 => {
            let Some(j) = t.fetching else {
                t.step = 8;
                return keep(sim, i, t);
            };
            let rect = movement::target_rect(sim, m, lib, j);
            match movement::walk(sim, m, g, lib, i, rect, MEET, Some(j)) {
                Some(true) => {
                    halt(&mut sim.gameplay.actors[i]);
                    t.step = 4;
                }
                Some(false) => {
                    t.step = 8;
                    give_up(&mut t, j);
                }
                _ if unreachable(sim, i) => {
                    t.step = 8;
                    give_up(&mut t, j);
                }
                _ => {}
            }
        }
        // Op 0x54: aboard or gone, the passenger leaves the list.
        4 => {
            let j = t.fetching.unwrap_or(usize::MAX);
            if coming(sim, m, i, j) && !give_up(&mut t, j) {
                t.step = 5;
            } else {
                t.passengers.retain(|&k| k != j);
                t.step = 8;
            }
        }
        5 => {
            if waited(sim, &mut t.due, WAIT) {
                t.step = 4;
            }
        }
        6 => {
            let p = t.point;
            let rect = [p[0] as i32, p[1] as i32, p[0] as i32, p[1] as i32];
            match movement::walk(sim, m, g, lib, i, rect, 0, None) {
                Some(_) => {
                    halt(&mut sim.gameplay.actors[i]);
                    t.step = 7;
                }
                _ if unreachable(sim, i) => t.step = 7,
                _ => {}
            }
        }
        7 => {
            unload(sim, m, lib, i, t.landing);
            finish(sim, i);
            return;
        }
        8 => {
            if waited(sim, &mut t.due, WAIT) {
                t.step = 2;
            }
        }
        9..=11 => {
            if !war_step(sim, m, g, lib, i, &mut t) {
                finish(sim, i);
                return;
            }
        }
        _ => {
            finish(sim, i);
            return;
        }
    }
    keep(sim, i, t);
}
fn keep(sim: &mut Simulation, i: usize, t: Transport) {
    if matches!(sim.gameplay.actors[i].intent, Some(Intent::Transport(_))) {
        sim.gameplay.actors[i].intent = Some(Intent::Transport(Box::new(t)));
    }
}
/// ReTzar's patience with one passenger; true once it is out.
fn give_up(t: &mut Transport, j: usize) -> bool {
    t.patience = t.patience.saturating_add(1);
    if t.patience < PATIENCE {
        return false;
    }
    t.passengers.retain(|&k| k != j);
    t.patience = 0;
    true
}
/// Steps 9 to 11 of a war transport, as a group's Approach (ops 0x4f, 9
/// and 0x50); false when the program ends.
fn war_step(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    t: &mut Transport,
) -> bool {
    let Some(war) = t.war else {
        return false;
    };
    let Some(rect) = sim
        .gameplay
        .ai
        .manager
        .as_ref()
        .and_then(|mgr| mgr.place(war.place))
        .filter(|p| p.serial == war.serial)
        .map(|p| p.anchor_rect)
    else {
        return false;
    };
    let ap = war::Approach {
        place: war.place,
        serial: war.serial,
        rect,
        point: t.point,
        radius: WAR_RADIUS,
        step: 2,
        sector: war.sector,
        due: 0,
    };
    match t.step {
        9 => {
            match war::approach_check(sim, m, i, &ap) {
                Some(sector) => {
                    if let Some(x) = t.war.as_mut() {
                        x.sector = sector;
                    }
                    t.step = 11;
                }
                None => t.step = 10,
            }
            true
        }
        10 => {
            match movement::walk(sim, m, g, lib, i, rect, WAR_RADIUS as u32, None) {
                Some(_) => {
                    halt(&mut sim.gameplay.actors[i]);
                    t.step = 11;
                }
                _ if unreachable(sim, i) => t.step = 11,
                _ => {}
            }
            true
        }
        _ => {
            if t.due != 0 && now(sim) < t.due {
                return true;
            }
            t.due = 0;
            match war::wait_others(sim, m, i, &ap) {
                0 => {
                    t.due = now(sim) + war::WAIT_MS;
                    true
                }
                1 => {
                    t.step = 6;
                    true
                }
                _ => false,
            }
        }
    }
}
/// Op 0x33 of a ship: its passengers step off within 4 rings of it, from
/// the cell each came in by, onto cells of the landing region (0xcd9f24);
/// from the first that finds none, they stay aboard.
fn unload(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    ship: usize,
    region: u8,
) {
    let side = sim.gameplay.pathing.cells.side as i32;
    while let Some(&j) = sim.objects[ship]
        .contents
        .iter()
        .find(|&&j| !carried(&m.objects[j]))
    {
        let (rect, start) = way_out(sim, m, lib, ship, j);
        let cell = ring_search(sim, rect, start, UNLOAD_RINGS, |sim, p| {
            p[0] >= 0
                && p[1] >= 0
                && p[0] <= side - 2
                && p[1] <= side - 2
                && (region == NONE || sim.gameplay.islands.region(p) == region)
                && sim.gameplay.pathing.cells.spawnable(p, 1)
        });
        let Some(q) = cell else {
            return;
        };
        leave(sim, m, ship, j, q);
    }
}

/// A passenger's Use transport, one step.
pub(crate) fn ride(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    mut r: Ride,
) {
    let ship = r.ship;
    match r.step {
        1 => {
            if !m.objects.get(ship).is_some_and(alive) {
                r.step = 2;
                return keep_ride(sim, i, r);
            }
            let rect = movement::target_rect(sim, m, lib, ship);
            match movement::walk(sim, m, g, lib, i, rect, MEET, Some(ship)) {
                Some(true) => {
                    halt(&mut sim.gameplay.actors[i]);
                    r.step = 4;
                }
                Some(false) => r.step = 2,
                _ if unreachable(sim, i) => r.step = 2,
                _ => {}
            }
        }
        // Op 0x55: the ship is on Transport with the unit among its
        // passengers.
        2 => {
            let listed = m.objects.get(ship).is_some_and(alive)
                && matches!(&sim.gameplay.actors[ship].intent,
                    Some(Intent::Transport(t)) if t.passengers.contains(&i));
            if !listed {
                finish(sim, i);
                return;
            }
            r.step = 3;
        }
        3 | 5 | 8 => {
            if waited(sim, &mut r.due, WAIT) {
                r.step = match r.step {
                    5 => 6,
                    _ => 1,
                };
            }
        }
        // Op 0xb (0x4e0ce8): the ship gone fails; a full ship ends the op
        // as done; else the unit goes in where it stands when the ship
        // takes it (0x455280, vt+0x1a8).
        4 => {
            if !m.objects.get(ship).is_some_and(alive) {
                r.step = 8;
            } else if sim.objects[ship].contents.len() >= holder_capacity(sim, m, lib, ship) {
                r.step = 5;
            } else if boardable(sim, m, lib, c, i, ship) {
                halt(&mut sim.gameplay.actors[i]);
                garrison(sim, m, ship, i);
                r.step = 5;
            } else {
                r.step = 8;
            }
        }
        // Op 0x56: still aboard, wait again; landed, walk to the point if
        // the order asks, else the program ends.
        6 => {
            if sim.objects[i].holder.is_some() {
                r.step = 5;
            } else if r.walk {
                r.step = 7;
            } else {
                finish(sim, i);
                return;
            }
        }
        7 => {
            let p = r.point;
            let rect = [p[0] as i32, p[1] as i32, p[0] as i32, p[1] as i32];
            match movement::walk(sim, m, g, lib, i, rect, 0, None) {
                Some(_) => {
                    finish(sim, i);
                    return;
                }
                _ if unreachable(sim, i) => {
                    finish(sim, i);
                    return;
                }
                _ => {}
            }
        }
        _ => {
            finish(sim, i);
            return;
        }
    }
    keep_ride(sim, i, r);
}
fn keep_ride(sim: &mut Simulation, i: usize, r: Ride) {
    if matches!(sim.gameplay.actors[i].intent, Some(Intent::UseTransport(_))) {
        sim.gameplay.actors[i].intent = Some(Intent::UseTransport(r));
    }
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
