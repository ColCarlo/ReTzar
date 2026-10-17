//! Whether a computer player can get from one land region to another
//! (0x417470, 0x417900), or to a rect from a point (0x417640), and the ship
//! that would carry it (0x416ef0, 0x416d00). The map's caches and the
//! player's request for ships change with every question, as in the
//! original.
use super::*;
use crate::gameplay::islands::{LAND, NONE, WIPED, link};

/// A ship's water must have this many cells for the player to want one
/// there unasked (0x416d25).
const WATER_FOR_SHIPS: u32 = 60;
/// The most ships a water is asked for (0x416ebf).
const MOST_SHIPS: u16 = 4;

/// The ways to reach a region.
pub const NO: u8 = 0;
pub const BY_LAND: u8 = 1;
pub const BY_SEA: u8 = 2;

struct Ask<'a> {
    sim: &'a mut Simulation,
    m: &'a MissionWorld,
    lib: &'a ObjectLibrary,
    c: &'a Catalog,
    /// The player that asks, whose ships answer and whose wishes for
    /// ships grow (IA+0x1a24, IA+0x1a34).
    player: &'a mut Player,
}

/// 0x417470: from land region `a` to land region `b`, by land, by sea or
/// not at all. Only "by land" is remembered.
pub fn regions(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    player: &mut Player,
    a: u8,
    b: u8,
) -> u8 {
    Ask {
        sim,
        m,
        lib,
        c,
        player,
    }
    .regions(a, b)
}

/// 0x417640: from the point's region to the regions along the border of
/// `rect`, the first that answers. Each border region is asked from its
/// side (`regions(border, point)`).
pub fn point(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    player: &mut Player,
    p: [i32; 2],
    rect: [i32; 4],
) -> u8 {
    Ask {
        sim,
        m,
        lib,
        c,
        player,
    }
    .point(p, rect)
}

/// 0x416ef0: the ship of the player that could carry `passenger` (or any
/// unit) from land region `a` to land region `b`.
#[allow(clippy::too_many_arguments)]
pub fn ship(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    player: &mut Player,
    a: u8,
    b: u8,
    passenger: Option<usize>,
) -> Option<usize> {
    Ask {
        sim,
        m,
        lib,
        c,
        player,
    }
    .ship(a, b, passenger)
}

impl Ask<'_> {
    fn regions(&mut self, a: u8, b: u8) -> u8 {
        if matches!(a, NONE | WIPED) || matches!(b, NONE | WIPED) {
            return NO;
        }
        if a == b {
            return BY_LAND;
        }
        let islands = &mut self.sim.gameplay.islands;
        if islands.kind(a) != islands.kind(b) || islands.kind(a) != LAND {
            return NO;
        }
        if islands.pairs.get((a, b)) == Some(true) {
            return BY_LAND;
        }
        let mut seen = [false; 256];
        let way = self.search(&mut seen, a, b);
        if way != BY_SEA {
            self.sim.gameplay.islands.pairs.put((a, b), way != NO);
        }
        way
    }
    /// 0x417900: depth first through the regions joined to `a` by land or
    /// bridge; then, between two lands, through each land joined only by
    /// water that reaches `b` by land and has a ship for the crossing. The
    /// original looks for the ship from that land to `a`, the wrong way
    /// round; ReTzar looks from `a`.
    fn search(&mut self, seen: &mut [bool; 256], a: u8, b: u8) -> u8 {
        seen[a as usize] = true;
        let next = self.sim.gameplay.islands.next;
        for r in 1..next {
            let islands = &self.sim.gameplay.islands;
            if r == a || islands.links(r, a) & (link::ADJACENT | link::BRIDGE) == 0 {
                continue;
            }
            if r == b {
                return BY_LAND;
            }
            if !seen[r as usize] && islands.kind(r) == LAND {
                let way = self.search(seen, r, b);
                if way != NO {
                    return way;
                }
            }
        }
        let islands = &self.sim.gameplay.islands;
        if islands.kind(a) != LAND || islands.kind(b) != LAND {
            return NO;
        }
        for r in 1..next {
            let islands = &self.sim.gameplay.islands;
            let links = islands.links(r, a);
            if r == a
                || islands.kind(r) != LAND
                || links & (link::ADJACENT | link::BRIDGE) != 0
                || links & link::BY_WATER == 0
                || seen[r as usize]
            {
                continue;
            }
            if (r == b || self.search(seen, r, b) == BY_LAND) && self.ship(a, r, None).is_some() {
                return BY_SEA;
            }
        }
        NO
    }
    fn point(&mut self, p: [i32; 2], rect: [i32; 4]) -> u8 {
        let here = self.sim.gameplay.islands.region(p);
        if matches!(here, NONE | WIPED) {
            return NO;
        }
        if self.sim.gameplay.islands.rects.get((here, rect)) == Some(true) {
            return BY_LAND;
        }
        let side = self.sim.gameplay.islands.side as i32;
        // The last region asked on the top or left side, and on the bottom
        // or right side.
        let mut last = [NONE; 2];
        let mut ask = |me: &mut Self, side_of: usize, cell: [i32; 2]| {
            let r = me.sim.gameplay.islands.region(cell);
            if r == last[side_of] {
                return NO;
            }
            last[side_of] = r;
            me.regions(r, here)
        };
        let mut cells = Vec::new();
        for x in rect[0].max(0)..=rect[2].min(side - 1) {
            if rect[1] >= 0 {
                cells.push((0, [x, rect[1]]));
            }
            if rect[3] < side {
                cells.push((1, [x, rect[3]]));
            }
        }
        for y in (rect[1] + 1).max(0)..rect[3].min(side) {
            if rect[0] >= 0 {
                cells.push((0, [rect[0], y]));
            }
            if rect[2] < side {
                cells.push((1, [rect[2], y]));
            }
        }
        for (side_of, cell) in cells {
            let way = ask(self, side_of, cell);
            if way != NO {
                if way == BY_LAND {
                    self.sim.gameplay.islands.rects.put((here, rect), true);
                }
                return way;
            }
        }
        self.sim.gameplay.islands.rects.put((here, rect), false);
        NO
    }
    fn ship(&mut self, a: u8, b: u8, passenger: Option<usize>) -> Option<usize> {
        let islands = &self.sim.gameplay.islands;
        if a == b
            || islands.kind(a) != LAND
            || islands.kind(b) != LAND
            || islands.links(a, b) & link::BY_WATER == 0
        {
            return None;
        }
        let mut best: Option<(usize, u32)> = None;
        for w in 1..islands.next {
            let islands = &self.sim.gameplay.islands;
            if islands.links(w, a) & link::ADJACENT == 0
                || islands.links(w, b) & link::ADJACENT == 0
            {
                continue;
            }
            let Some(s) = self.water_ship(w, b, passenger) else {
                continue;
            };
            let Some(p) = passenger else {
                return Some(s);
            };
            let gap = combat::object_gap(self.lib, &self.m.objects[s], &self.m.objects[p]);
            if best.is_none_or(|(_, g)| gap < g) {
                best = Some((s, gap));
            }
        }
        best.map(|(s, _)| s)
    }
    /// 0x416d00: a ship of the player on water `w` that can take a
    /// passenger now, the nearest to it (the first without one). A water
    /// of 60 cells or more wants a ship; when none fits, it wants one more,
    /// up to 4 and never more than one beyond the ships there.
    fn water_ship(&mut self, w: u8, land: u8, passenger: Option<usize>) -> Option<usize> {
        let big = self.sim.gameplay.islands.regions[w as usize].cells >= WATER_FOR_SHIPS;
        let player = &mut *self.player;
        if big && player.wanted_ships.get(&w).copied().unwrap_or(0) == 0 {
            player.wanted_ships.insert(w, 1);
        }
        let ships = player.ships.get(&w)?.clone();
        let mut best: Option<(usize, u32)> = None;
        for &s in &ships {
            let Some(o) = self.m.objects.get(s) else {
                continue;
            };
            if o.flags & (FLAG_STORY | FLAG_DYING) != 0 {
                continue;
            }
            if passenger.is_some_and(|p| !accepts(self.lib, self.c, o, &self.m.objects[p])) {
                continue;
            }
            let room = holder_capacity(self.sim, self.m, self.lib, s);
            if self.sim.objects[s].contents.len() >= room {
                continue;
            }
            // Idle, or a Transport still loading for the island of `land`
            // with room for its passengers too.
            let idle = program(self.sim, self.m, s).kind == program::IDLE;
            if !idle && !super::transport::loading_for(self.sim, s, land, room) {
                continue;
            }
            let Some(p) = passenger else {
                return Some(s);
            };
            let gap = combat::object_gap(self.lib, o, &self.m.objects[p]);
            if best.is_none_or(|(_, g)| gap < g) {
                best = Some((s, gap));
            }
        }
        if best.is_none() {
            let player = &mut *self.player;
            let wanted = player.wanted_ships.get(&w).copied().unwrap_or(0);
            if ships.len() >= wanted as usize && wanted < MOST_SHIPS {
                player.wanted_ships.insert(w, wanted + 1);
            }
        }
        best.map(|(s, _)| s)
    }
}
