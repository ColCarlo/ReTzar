//! A computer player's work on its resources: the gatherers it wants of
//! each and where it sends them (Harvest, 0x40dd90 with 0x40e7a0, 0x40eab0,
//! 0x40f370 and 0x40f880), the stores it builds near the work (Storages,
//! 0x40dff0 with 0x40e150 and 0x40f0a0) and its search for a wood (message
//! 0x2202, 0x40d5c0 with the test 0x40d790).
use super::build::{self, Site};
use super::records::gap;
use super::rules::{self, World, bucket};
use super::think::{self, group};
use super::*;
use crate::gameplay::pathing::{self, Cells};

/// A mine takes this many gatherers before the next is looked for
/// (0x40df14, 0x40ebee).
const MINERS: u16 = 10;
/// Woodcutters sent to a tree at once (0x40df22); a tree with more aimed at
/// it is no wood for the search (0x40daea).
const CUTTERS: u16 = 4;
/// A store this near the work is enough (0x40e216); one is looked for this
/// far (0x40f3b2, 0x40e1c0).
const NEAR: u32 = 8;
const STORE_REACH: u32 = 16;
/// The lumber mills and castles keep their woodcutters on trees this near
/// (0x40ecd4); the wood is kept with a tree this near it (0x40ede0).
const MILL_TREES: i32 = 8;
const WOOD_TREES: i32 = 10;
/// The search for a wood: 1000 cells every 500 ms (0x40d74d, 0x40d59d); a
/// tree this far from the wood is the next one (0x40d634).
pub const FOREST_PERIOD: u64 = 500;
const FOREST_CELLS: u32 = 1000;
const NEXT_WOOD: u32 = 10;
/// Off the map (0xffff).
pub(super) const NOWHERE: [i32; 2] = [-1, -1];

/// The search for a wood (IA+0x9d6 to IA+0x9e2, IA+0xa09).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Forest {
    /// The ring round the home it is on (+0x9d6), and the first ring with a
    /// tree it turned down (+0x9d8), where it starts again past the map.
    pub radius: u16,
    pub saved: u16,
    /// The cell it looks at next (+0x9dc), `NOWHERE` past the map.
    pub at: [i32; 2],
    /// The wood the woodcutters go to (+0x9e0, +0x9e2).
    pub wood: Option<[i32; 2]>,
    /// While it runs (+0x9da): when it goes on and its place in the queue.
    pub due: Option<u64>,
    pub seq: u64,
    /// No wood on the whole map (+0xa09).
    pub none: bool,
}

/// 0x515bf0: the centre of a rect.
pub(super) fn centre(r: [i32; 4]) -> [i32; 2] {
    [(r[0] + r[2]) / 2, (r[1] + r[3]) / 2]
}
/// 0x401a70: the empty cells between two points.
fn apart(a: [i32; 2], b: [i32; 2]) -> u32 {
    gap(a, [b[0], b[1], b[0], b[1]])
}
fn cell(o: &MapObject) -> [i32; 2] {
    [o.x as i32, o.y as i32]
}
/// The resource of a mine (+0x144): gold 0, stone 1.
fn ore(o: &MapObject) -> usize {
    usize::from(o.class == class::RES_STONE)
}
/// What a mine has left (+0x146), `u32::MAX` without end.
fn left(w: &World, j: usize) -> u32 {
    let t = &w.m.objects[j];
    w.sim
        .gameplay
        .resources
        .get(&j)
        .copied()
        .unwrap_or(if t.byte(0x3f) != 0 {
            u32::MAX
        } else {
            t.dword(0x3a)
        })
}
/// A tree stands on the cell: its image above the trunks, not felled.
fn standing(cells: &Cells, p: [i32; 2]) -> bool {
    let word = cells.at(p);
    let image = (word >> 11) & 31;
    image > cells.trunks && word & pathing::FELLED == 0 && image != 0
}
fn around(p: [i32; 2]) -> [i32; 4] {
    [p[0] - 1, p[1] - 1, p[0] + 1, p[1] + 1]
}

/// 0x44d670: the first standing tree of the engine's flood from `p` within
/// `r`, the flood spreading over cells a land unit may stand on (all of
/// them until it meets one).
fn tree_near(w: &mut World, p: [i32; 2], r: i32) -> Option<[i32; 2]> {
    let last = w.g.side as i32 - 1;
    let bounds = [
        (p[0] - r).max(0),
        (p[1] - r).max(0),
        (p[0] + r).min(last),
        (p[1] + r).min(last),
    ];
    let side = w.g.side;
    let cells = build::cells(w);
    let mut flood = build::Flood::new(p, bounds, side);
    let mut ground = false;
    flood
        .run(u32::MAX, |q, stop| {
            *stop = standing(cells, q);
            if cells.free(1, q) {
                ground = true;
                true
            } else {
                !ground
            }
        })
        .1
}

/// 0x401be0: the first cell of a ring on the map. A ring goes clockwise
/// from its top left corner: the top row rightwards, the right column
/// down, the bottom row leftwards and the left column up.
pub(super) fn ring_first(lo: [i32; 2], hi: [i32; 2], side: i32) -> [i32; 2] {
    let ([x0, y0], [x1, y1]) = (lo, hi);
    if y0 >= side || y1 < 0 || x0 >= side || x1 < 0 {
        NOWHERE
    } else if y0 >= 0 {
        [x0.max(0), y0]
    } else if x1 < side {
        [x1, 0]
    } else if y1 < side {
        [side - 1, y1]
    } else if x0 >= 0 {
        [x0, side - 1]
    } else {
        NOWHERE
    }
}
/// 0x401c90: the cell after `p` on the ring, on the map.
pub(super) fn ring_next(lo: [i32; 2], hi: [i32; 2], p: [i32; 2], side: i32) -> [i32; 2] {
    let ([x0, y0], [x1, y1]) = (lo, hi);
    let [mut x, mut y] = p;
    enum Side {
        Right,
        Bottom,
        Left,
    }
    let mut at = if y == y0 {
        x += 1;
        if x <= x1 && x < side {
            return [x, y];
        }
        x = x1;
        Side::Right
    } else if x == x1 {
        Side::Right
    } else if y == y1 {
        Side::Bottom
    } else if x == x0 {
        Side::Left
    } else {
        return NOWHERE;
    };
    loop {
        match at {
            Side::Right => {
                if x < side {
                    y = (y + 1).max(0);
                    if y <= y1 && y < side {
                        return [x, y];
                    }
                }
                y = y1;
                at = Side::Bottom;
            }
            Side::Bottom => {
                if y0 == y1 {
                    if x != x0 {
                        return NOWHERE;
                    }
                } else if y < side {
                    x = (x - 1).min(side - 1);
                    if x >= x0 && x >= 0 {
                        return [x, y];
                    }
                    x = x0;
                } else {
                    x = x0;
                }
                at = Side::Left;
            }
            Side::Left => {
                if x0 == x1 || x < 0 {
                    return NOWHERE;
                }
                y = (y - 1).min(side - 1);
                return if y > y0 && y >= 0 { [x, y] } else { NOWHERE };
            }
        }
    }
}

fn site_rect(w: &World, s: Site) -> [i32; 4] {
    match s {
        Site::Real(j) => movement::footprint_rect(w.lib, &w.m.objects[j]),
        Site::Virtual(id) => w
            .sim
            .gameplay
            .ai
            .virtuals
            .get(&id)
            .map_or([0; 4], |v| v.rect()),
    }
}
fn site_class(w: &World, s: Site) -> usize {
    match s {
        Site::Real(j) => w.m.objects[j].class,
        Site::Virtual(id) => w
            .sim
            .gameplay
            .ai
            .virtuals
            .get(&id)
            .map_or(0, |v| v.class as usize),
    }
}
/// vt+0x2c0: the building's work is done (+0xde reached +0xe2); never for
/// a site still to be laid.
fn finished(w: &World, s: Site) -> bool {
    matches!(s, Site::Real(j) if w.m.objects[j].flags & FLAG_CONSTRUCTION == 0)
}
fn rect_of(w: &World, i: usize) -> [i32; 4] {
    movement::footprint_rect(w.lib, &w.m.objects[i])
}
fn grown(r: [i32; 4]) -> [i32; 4] {
    [r[0] - 1, r[1] - 1, r[2] + 1, r[3] + 1]
}

impl Player {
    /// The site a builder works on (+0x146), laid or still to be.
    fn builder_site(&self, w: &World, i: usize) -> Option<Site> {
        let p = self.lists.peons.get(&i)?;
        match (p.target, p.site) {
            (Some(j), _) if alive(&w.m.objects[j]) => Some(Site::Real(j)),
            (_, Some(id)) => Some(Site::Virtual(id)),
            _ => None,
        }
    }
    /// The gatherers of `r` working `j` (+0x146).
    fn working(&self, r: usize, j: usize) -> u16 {
        self.lists.groups[group::GOLD as usize + r]
            .iter()
            .filter(|i| self.lists.peons.get(i).and_then(|p| p.target) == Some(j))
            .count() as u16
    }

    /// 0x40e6c0: what the parts book of a resource the player may still
    /// gather.
    fn need(&self, r: usize) -> i32 {
        if self.blocked[r] {
            return 0;
        }
        self.buckets.iter().map(|b| b[r]).fold(0, i32::wrapping_add)
    }
    /// 0x40e7a0: the gatherers wanted of each resource. The resources are
    /// put in order of need (in hundreds, the later one first on ties); the
    /// parts' bookings are weighed (the script's half again, the defences'
    /// halved without barracks, gold's half again) and the peasants, idle
    /// or gathering, are shared by how far each resource the player may
    /// still gather falls short; the rest goes one more to the first in
    /// order. None in King of the Hill with unlimited resources.
    fn want(&mut self, w: &World) {
        if w.m.game_rules.game_type == 1 && w.m.game_rules.resource_level == 2 {
            self.wanted = [0; 4];
            return;
        }
        let mut order = self.order.0;
        order.sort_by(|&a, &b| {
            let (na, nb) = (self.need(a as usize) / 100, self.need(b as usize) / 100);
            nb.cmp(&na).then(b.cmp(&a))
        });
        self.order.0 = order;
        let barracks =
            rules::record(w.sim, w.m, &w.c.ai, self.kingdom, class::BARRACK as u16).0 != 0;
        let mut weight = [0i32; 4];
        let mut people = 0u16;
        for (r, total) in weight.iter_mut().enumerate() {
            people = people.wrapping_add(self.lists.groups[group::GOLD as usize + r].len() as u16);
            for (part, b) in self.buckets.iter().enumerate() {
                let (times, by) = match part {
                    bucket::SCRIPT => (3, 2),
                    bucket::DEFENCE => (1, if barracks { 1 } else { 2 }),
                    _ => (1, 1),
                };
                *total = total.wrapping_add(b[r].wrapping_mul(times) / by);
            }
        }
        weight[0] = weight[0].wrapping_mul(3) / 2;
        let people = people.wrapping_add(self.lists.groups[group::IDLE as usize].len() as u16);
        if people == 0 {
            self.wanted = [0; 4];
            return;
        }
        let short = |r: usize| {
            (!self.blocked[r] && weight[r] > self.available[r])
                .then(|| weight[r].wrapping_sub(self.available[r]) as u32)
        };
        let total = (0..4).filter_map(short).fold(0u32, u32::wrapping_add);
        let mut rest = people;
        for r in 0..4 {
            self.wanted[r] = match short(r).filter(|_| total != 0) {
                Some(s) => {
                    let n = s.wrapping_mul(people as u32) / total;
                    rest = rest.wrapping_sub(n as u16);
                    n as u16
                }
                None => 0,
            };
        }
        let open = self.blocked.iter().filter(|b| !**b).count() as u16;
        if open == 0 {
            return;
        }
        while rest != 0 {
            let (each, mut extra) = (rest / open, rest % open);
            let mut at = 0;
            while extra != 0 {
                let r = self.order.0[at] as usize;
                if !self.blocked[r] {
                    self.wanted[r] = self.wanted[r].wrapping_add(each + 1);
                    rest = rest.wrapping_sub(each + 1);
                    extra -= 1;
                }
                at += 1;
            }
            for &r in &self.order.0[at.min(4)..] {
                if !self.blocked[r as usize] {
                    self.wanted[r as usize] = self.wanted[r as usize].wrapping_add(each);
                    rest = rest.wrapping_sub(each);
                }
            }
        }
    }

    /// 0x40dd90: the Harvest phase. The gatherers wanted are worked out and
    /// the workers' part cleared; the gatherers over the number go back to
    /// idleness, and each resource short of them gets idle peasants at one
    /// work after the other. What a mine, field or herd cannot take goes to
    /// wood.
    pub(super) fn harvest(&mut self, w: &mut World) {
        self.want(w);
        self.buckets[bucket::WORKERS] = [0; 4];
        for r in 0..4 {
            let have = self.lists.groups[group::GOLD as usize + r].len() as u32;
            if have > self.wanted[r] as u32 {
                self.release(w, r, (have - self.wanted[r] as u32) as u16);
            }
        }
        for r in 0..4 {
            let have = self.lists.groups[group::GOLD as usize + r].len() as u16;
            let mut short = self.wanted[r].wrapping_sub(have) as i16;
            if short <= 0 {
                continue;
            }
            let mut spot = self.site_for(w, r, None);
            while let Some((at, site)) = spot {
                let n = match r {
                    2 => 1,
                    3 => short.min(CUTTERS as i16),
                    _ => {
                        let working = site.map_or(0, |j| self.working(r, j));
                        if working < MINERS {
                            short.min((MINERS - working) as i16)
                        } else {
                            short
                        }
                    }
                };
                let (all, rest) = self.assign(w, r, n as u16, at, site);
                if !all {
                    short = short.wrapping_add(rest as i16).wrapping_sub(n);
                    break;
                }
                short = short.wrapping_sub(n);
                if short == 0 {
                    break;
                }
                spot = self.site_for(w, r, site);
            }
            if short != 0 && r != 3 {
                self.wanted[r] = self.wanted[r].wrapping_sub(short as u16);
                self.wanted[3] = self.wanted[3].wrapping_add(short as u16);
            }
        }
    }

    /// 0x40f880: `n` gatherers of `r` the player may take go back to
    /// idleness, each time the farthest from its work (the first on ties).
    fn release(&mut self, w: &mut World, r: usize, mut n: u16) {
        while n != 0 {
            let mut best: Option<(u32, usize)> = None;
            for &i in &self.lists.groups[group::GOLD as usize + r] {
                if !build::free(w, self, i) {
                    continue;
                }
                let Some(peon) = self.lists.peons.get(&i) else {
                    continue;
                };
                let d = apart(centre(peon.rect), cell(&w.m.objects[i]));
                if best.is_none_or(|(b, _)| d > b) {
                    best = Some((d, i));
                }
            }
            let Some((_, i)) = best else {
                return;
            };
            think::restart(w.sim, i);
            self.hooked(w, i);
            n -= 1;
        }
    }

    /// 0x40eab0: the next work for gatherers of `r` and its cell, after
    /// `prev`.
    ///
    /// Gold and stone: of the mines not yet left, nearest the home first,
    /// the first safe one nobody works, else one with fewer than ten
    /// gatherers, else the last full one (`prev` at first); with no mine
    /// left of the resource it is blocked. Wood: a tree within 8 of a
    /// lumber mill, then of a castle (a mill or castle without one is not
    /// looked at again), then the wood or a tree within 10 of it, then the
    /// tree the search last found, the search starting again. Food: a cow
    /// while the milkers are not more than half the cows, else the safe
    /// field without a farmer least grown (the later on ties), else a new
    /// field the workers' part pays for (booked when it cannot).
    fn site_for(
        &mut self,
        w: &mut World,
        r: usize,
        prev: Option<usize>,
    ) -> Option<([i32; 2], Option<usize>)> {
        match r {
            0 | 1 => {
                let mut fallback = prev;
                let mut skip = prev;
                let mut blocked = true;
                for e in self.lists.mines.clone() {
                    if let Some(p) = skip {
                        if e.object == p {
                            skip = None;
                        }
                        continue;
                    }
                    let o = &w.m.objects[e.object];
                    if ore(o) != r || left(w, e.object) == 0 {
                        continue;
                    }
                    blocked = false;
                    let at = cell(o);
                    if !build::safe(w, self, at[0], at[1], [1, 1]) {
                        continue;
                    }
                    if e.count == 0 || self.working(r, e.object) < MINERS {
                        return Some((at, Some(e.object)));
                    }
                    fallback = Some(e.object);
                }
                self.blocked[r] = blocked;
                fallback.map(|j| (cell(&w.m.objects[j]), Some(j)))
            }
            3 => self.wood(w).map(|t| (t, None)),
            _ => self.food(w),
        }
    }
    fn wood(&mut self, w: &mut World) -> Option<[i32; 2]> {
        self.home?;
        for mills in [false, true] {
            let list = if mills {
                self.lists.castles.clone()
            } else {
                self.lists.lumber.clone()
            };
            for j in list {
                if self.lists.bare.contains(&j) {
                    continue;
                }
                let c = centre(rect_of(w, j));
                if let Some(t) = tree_near(w, c, MILL_TREES) {
                    return Some(t);
                }
                self.lists.bare.push(j);
            }
        }
        if let Some(f) = self.forest.wood {
            if self.tree_ok(w, f) {
                return Some(f);
            }
            if let Some(t) = tree_near(w, f, WOOD_TREES)
                && self.tree_ok(w, t)
            {
                return Some(t);
            }
        }
        self.forest.wood = None;
        if self.forest.due.is_some() {
            return None;
        }
        self.forest_start(w);
        let at = self.forest.at;
        if at[0] == NOWHERE[0] || !self.tree_ok(w, at) {
            return None;
        }
        self.forest.wood = Some(at);
        Some(at)
    }
    fn food(&mut self, w: &mut World) -> Option<([i32; 2], Option<usize>)> {
        let k = self.kingdom;
        let cows = rules::record(w.sim, w.m, &w.c.ai, k, class::COW as u16).0;
        if cows != 0 && (self.lists.groups[group::MILKERS as usize].len() as u32) < cows / 2 + 1 {
            let herd = think::kingdom_list(w.sim, w.m, k, true)
                .into_iter()
                .find(|&i| w.m.objects[i].class == class::COW);
            if let Some(cow) = herd {
                return Some((cell(&w.m.objects[cow]), Some(cow)));
            }
        }
        let mut best: Option<usize> = None;
        for e in self.lists.fields.clone() {
            if e.count >= 1 {
                continue;
            }
            let at = cell(&w.m.objects[e.object]);
            if !build::safe(w, self, at[0], at[1], [1, 1]) {
                continue;
            }
            if best.is_none_or(|b| w.m.objects[b].health >= w.m.objects[e.object].health) {
                best = Some(e.object);
            }
        }
        if let Some(f) = best {
            return Some((cell(&w.m.objects[f]), Some(f)));
        }
        // 0x40efeb: a new field (0x409550); booked by the workers' part
        // when it cannot pay for one.
        let cost = rules::cost(w, self, class::FIELD as u16);
        if !rules::affordable(w, self, cost, bucket::WORKERS, 1, false) {
            rules::book(self, bucket::WORKERS, cost, 1);
            return None;
        }
        if !rules::available(w, self, class::FIELD as u16) {
            return None;
        }
        let home = self.home.unwrap_or_default();
        match self.place(w, class::FIELD as u16, home, 0, 50, None, false) {
            Some(Site::Real(f)) => Some((cell(&w.m.objects[f]), Some(f))),
            _ => None,
        }
    }

    /// 0x40f370: up to `n` idle peasants the player may take, the nearest
    /// the work first, sent to it; each must reach it and not carry the
    /// resource already. The builders of an unfinished store near the work
    /// count among them. A mine is marked worked; mines, fields and herds
    /// want a store within 16, wood not. Whether all went, and what is left
    /// of `n`.
    fn assign(
        &mut self,
        w: &mut World,
        r: usize,
        mut n: u16,
        at: [i32; 2],
        site: Option<usize>,
    ) -> (bool, u16) {
        let store = self.store_near(w, r, at, site.map(Site::Real), STORE_REACH);
        if let Some(s) = store
            && !finished(w, s)
        {
            for &b in &self.lists.groups[group::BUILDERS as usize] {
                if n == 0 {
                    break;
                }
                if self.builder_site(w, b) == Some(s) {
                    n -= 1;
                }
            }
        }
        if r < 2 {
            let Some(e) = self.lists.mines.iter_mut().find(|e| Some(e.object) == site) else {
                return (false, n);
            };
            e.count |= 1;
        }
        if r != 3 && store.is_none() {
            return (false, n);
        }
        let target = match site {
            Some(j) => grown(rect_of(w, j)),
            None => around(at),
        };
        while n != 0 {
            let mut best: Option<(u32, usize)> = None;
            for i in self.lists.groups[group::IDLE as usize].clone() {
                if carry_type(&w.sim.gameplay.actors[i]) == Some(r as u8)
                    || !build::free(w, self, i)
                {
                    continue;
                }
                let from = cell(&w.m.objects[i]);
                if !build::reach(w, self, from, target) {
                    continue;
                }
                let d = gap(at, rect_of(w, i));
                if best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, i));
                }
            }
            let Some((_, b)) = best else {
                return (false, n);
            };
            let intent = match (r, site) {
                (3, _) | (_, None) => Intent::Chop([at[0] as u16, at[1] as u16]),
                (_, Some(j)) => Intent::Harvest(j),
            };
            self.command(w, b, intent, at, site);
            // 0x40f7b4: a field's Dig program looks for a field again after
            // each delivery (its flag 1).
            if site.is_some_and(|j| w.m.objects[j].class == class::FIELD) {
                w.sim.gameplay.actors[b].dig_again = true;
            }
            n -= 1;
        }
        (true, 0)
    }

    /// 0x40f0a0: the castle or store of `r` nearest the work (its cell, or
    /// the site), within `max` and on its island, the kingdom's newest first
    /// on ties; but a site of one of them that a builder works on there
    /// comes before.
    fn store_near(
        &self,
        w: &mut World,
        r: usize,
        at: [i32; 2],
        site: Option<Site>,
        max: u32,
    ) -> Option<Site> {
        let a = match site {
            Some(s) => grown(site_rect(w, s)),
            None => around(at),
        };
        let list = think::kingdom_list(w.sim, w.m, self.kingdom, false);
        if list.is_empty() {
            return None;
        }
        let kind = build::store_of(r);
        let mut best: Option<(u32, usize)> = None;
        for j in list {
            let c = w.m.objects[j].class;
            if c != class::CASTLE && c != kind {
                continue;
            }
            let b = grown(rect_of(w, j));
            let d = build::rect_gap(a, b);
            if d > max || !w.sim.gameplay.islands.rects_island(a, b) {
                continue;
            }
            if best.is_none_or(|(x, _)| d < x) {
                best = Some((d, j));
            }
        }
        for &i in &self.lists.groups[group::BUILDERS as usize] {
            let Some(s) = self.builder_site(w, i) else {
                continue;
            };
            let c = site_class(w, s);
            if c != class::CASTLE && c != kind {
                continue;
            }
            let b = grown(site_rect(w, s));
            if build::rect_gap(a, b) <= max && w.sim.gameplay.islands.rects_island(a, b) {
                return Some(s);
            }
        }
        best.map(|(_, j)| Site::Real(j))
    }

    /// 0x40dff0: the Storages phase: the stores' part cleared, each
    /// resource in order of need: the worked mines of gold or stone, the
    /// fields, the woodcutters; one place served each.
    pub(super) fn storages(&mut self, w: &mut World) {
        self.buckets[bucket::STORES] = [0; 4];
        for at in 0..4 {
            let r = self.order.0[at] as usize;
            match r {
                0 | 1 => {
                    for e in self.lists.mines.clone() {
                        let o = &w.m.objects[e.object];
                        if ore(o) != r || e.count & 1 == 0 {
                            continue;
                        }
                        let c = centre(rect_of(w, e.object));
                        if self.store_need(w, r, c, Some(e.object)) {
                            break;
                        }
                    }
                }
                2 => {
                    for e in self.lists.fields.clone() {
                        let c = centre(rect_of(w, e.object));
                        if self.store_need(w, 2, c, Some(e.object)) {
                            break;
                        }
                    }
                }
                _ => {
                    self.store_need(w, 3, NOWHERE, None);
                }
            }
        }
    }
    /// 0x40e150: a store for the work at `at`. A finished one within 8 is
    /// enough. Otherwise the stores' part must pay for one (booked when it
    /// cannot); with no gatherer there yet one is built near the work (8
    /// cells with a store farther, else 16). Each gatherer the player may
    /// take helps build an unfinished store within 8, or builds one near
    /// its work. For wood each woodcutter's tree is the work. Whether a
    /// store was begun.
    fn store_need(
        &mut self,
        w: &mut World,
        r: usize,
        mut at: [i32; 2],
        site: Option<usize>,
    ) -> bool {
        let kind = build::store_of(r);
        let cost = rules::cost(w, self, kind as u16);
        let (mut store, mut d) = (None, STORE_REACH);
        if r != 3 {
            store = self.store_near(w, r, at, site.map(Site::Real), STORE_REACH);
            if let (Some(s), Some(j)) = (store, site) {
                d = build::rect_gap(rect_of(w, j), site_rect(w, s));
                if finished(w, s) && d <= NEAR {
                    return false;
                }
            }
            let far = store.is_none() || d > NEAR;
            if far && !rules::affordable(w, self, cost, bucket::STORES, 1, false) {
                rules::book(self, bucket::STORES, cost, 1);
                return false;
            }
            if far && self.lists.groups[group::GOLD as usize + r].is_empty() {
                let max = if store.is_some() { NEAR } else { STORE_REACH };
                self.store_build(w, r, at, max, None);
                return true;
            }
        }
        let mut begun = false;
        let gatherers = self.lists.groups[group::GOLD as usize + r].clone();
        for g in gatherers {
            if !self.lists.groups[group::GOLD as usize + r].contains(&g) || !build::free(w, self, g)
            {
                continue;
            }
            let work = self.lists.peons.get(&g).map_or(NOWHERE, |p| centre(p.rect));
            if r == 3 {
                at = work;
                store = self.store_near(w, 3, work, None, STORE_REACH);
                d = store.map_or(STORE_REACH, |s| gap(work, site_rect(w, s)));
                if store.is_some_and(|s| finished(w, s)) && d <= NEAR {
                    continue;
                }
                let far = store.is_none() || d > NEAR;
                if far && !rules::affordable(w, self, cost, bucket::STORES, 1, true) {
                    rules::book(self, bucket::STORES, cost, 1);
                    return begun;
                }
            } else if work != at {
                continue;
            }
            if let Some(s) = store
                && d <= NEAR
            {
                // 0x40e535: it helps build the store.
                let (intent, point, target) = match s {
                    Site::Real(j) => (Intent::Build(j), cell(&w.m.objects[j]), Some(j)),
                    Site::Virtual(id) => {
                        let v = w
                            .sim
                            .gameplay
                            .ai
                            .virtuals
                            .get(&id)
                            .copied()
                            .unwrap_or_default();
                        (Intent::FarBuild(id), [v.at[0] as i32, v.at[1] as i32], None)
                    }
                };
                self.command(w, g, intent, point, target);
                continue;
            }
            begun = true;
            let Some(new) = self.store_build(w, r, at, NEAR, Some(g)) else {
                return begun;
            };
            store = Some(new);
            d = match site {
                Some(j) => build::rect_gap(rect_of(w, j), site_rect(w, new)),
                None => gap(at, site_rect(w, new)),
            };
        }
        begun
    }
    /// 0x40f300: a store of `r` between 1 and `max` of the work, the
    /// gatherer `by` building it.
    fn store_build(
        &mut self,
        w: &mut World,
        r: usize,
        at: [i32; 2],
        max: u32,
        by: Option<usize>,
    ) -> Option<Site> {
        self.place(w, build::store_of(r) as u16, at, 1, max, by, false)
    }

    /// 0x40d790: a standing tree woodcutters may go to: at least three of
    /// its eight neighbours open land (no static object, no unit, no
    /// standing tree), at most four woodcutters on it, reachable from the
    /// home and safe. A tree turned down keeps the search's ring to start
    /// again from.
    fn tree_ok(&mut self, w: &mut World, p: [i32; 2]) -> bool {
        let home = self.home.unwrap_or_default();
        let cells = build::cells(w);
        if !cells.inside(p) || !standing(cells, p) {
            return false;
        }
        let open = |q: [i32; 2]| {
            let word = cells.at(q);
            cells.inside(q)
                && word & (3 << 17) == pathing::LAND
                && word & (pathing::STATIC | pathing::OCCUPIED[1]) == 0
                && !((word >> 11) & 31 > cells.trunks && word & pathing::FELLED == 0)
        };
        let mut free = 0;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let q = [p[0] + dx, p[1] + dy];
                if (dx, dy) != (0, 0) && cells.inside(q) && open(q) {
                    free += 1;
                }
            }
        }
        let cutters = self.lists.groups[group::WOOD as usize]
            .iter()
            .filter(|i| self.lists.peons.get(i).is_some_and(|x| centre(x.rect) == p))
            .count() as u16;
        if free >= 3
            && cutters <= CUTTERS
            && build::reach(w, self, home, around(p))
            && build::safe(w, self, p[0], p[1], [1, 1])
        {
            return true;
        }
        if self.forest.saved == 0 {
            self.forest.saved = self.forest.radius;
        }
        false
    }
    /// 0x40d590: the search for a wood goes on in 500 ms.
    pub(super) fn forest_start(&mut self, w: &mut World) {
        self.forest.due = Some(w.sim.ticks * TICK_MS + FOREST_PERIOD);
        self.forest.seq = w.sim.gameplay.ai.next_seq();
    }
    /// Message 0x2202: the search goes on while it has not ended.
    pub(super) fn forest_on(&mut self, w: &mut World) {
        let Some(due) = self.forest.due else {
            return;
        };
        if self.forest_search(w) {
            self.forest.due = Some(due + FOREST_PERIOD);
            self.forest.seq = w.sim.gameplay.ai.next_seq();
        } else {
            self.forest.due = None;
        }
    }
    /// 0x40d5c0: up to 1000 cells of the rings round the home. The first
    /// tree woodcutters may go to is the wood; then a tree 10 or more from
    /// the wood ends the search while the wood still stands, else it is the
    /// wood. Past the map the search starts again from the first ring with
    /// a tree it turned down; with none, and no wood, the map has none.
    fn forest_search(&mut self, w: &mut World) -> bool {
        let home = self.home.unwrap_or_default();
        let side = w.g.side as i32;
        let ring = |r: i32| ([home[0] - r, home[1] - r], [home[0] + r, home[1] + r]);
        let (mut lo, mut hi) = ring(self.forest.radius as i32);
        for _ in 0..FOREST_CELLS {
            let p = self.forest.at;
            if self.tree_ok(w, p) {
                match self.forest.wood {
                    Some(f) if apart(p, f) < NEXT_WOOD => {}
                    Some(f) if standing(build::cells(w), f) => return false,
                    _ => self.forest.wood = Some(p),
                }
            }
            self.forest.at = ring_next(lo, hi, p, side);
            if self.forest.at[0] != NOWHERE[0] {
                continue;
            }
            self.forest.radius += 1;
            (lo, hi) = ring(self.forest.radius as i32);
            self.forest.at = ring_first(lo, hi, side);
            if self.forest.at[0] != NOWHERE[0] {
                continue;
            }
            if self.forest.saved == 0 {
                if self.forest.wood.is_none() {
                    self.forest.none = true;
                }
                return false;
            }
            self.forest.radius = std::mem::take(&mut self.forest.saved);
            (lo, hi) = ring(self.forest.radius as i32);
            self.forest.at = ring_first(lo, hi, side);
        }
        true
    }
}

#[cfg(test)]
#[path = "harvest_tests.rs"]
mod tests;
