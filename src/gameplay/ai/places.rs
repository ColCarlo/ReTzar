//! The places of the computer players' manager (0x5abb14): rects of the map
//! aligned to the 8×8 sectors, each with the forces every kingdom has there
//! and on its way there, and the kingdoms' totals. The original keeps them
//! with hooks as things happen (0x402a70, 0x402c50, 0x403840, 0x403be0,
//! 0x402ea0, 0x404140, 0x403e10); ReTzar runs the same hooks once a step,
//! after the sector lists, for what changed since the step before, object
//! by object. The manager reads the other objects as it last saw them.
use super::*;
use crate::gameplay::{auto_hostile, relation, researches, strength};

/// Kingdoms 0–12.
pub const KINGDOMS: usize = 13;
/// The kinds of places (+2).
pub mod kind {
    /// A kingdom's castle and what is around it (param: the kingdom).
    pub const HOME: u8 = 1;
    /// A gold or stone mine (param: 0 gold, 1 stone).
    pub const RESOURCE: u8 = 2;
    /// Where kingdoms lost something to an enemy (param: their mask).
    pub const LOSSES: u8 = 3;
    pub const GATEWAY: u8 = 4;
    /// A kingdom's buildings away from its castles.
    pub const BASE: u8 = 5;
    /// A kingdom's units away from everything else.
    pub const UNITS: u8 = 6;
}
/// The categories of forces (the 0x1e-byte record).
pub mod category {
    pub const MILITARY: usize = 0;
    pub const CIVIL: usize = 1;
    pub const TOWERS: usize = 2;
    pub const BUILDINGS: usize = 3;
    pub const BARRACKS: usize = 4;
}
/// Masks of categories (0x4022a0, 0x4022e0).
pub mod mask {
    pub const MILITARY: u8 = 1;
    pub const TOWERS: u8 = 2;
    pub const CIVIL: u8 = 4;
    pub const BUILDINGS: u8 = 8;
    pub const BARRACKS: u8 = 0x10;
}
const MASKS: [(u8, usize); 5] = [
    (mask::MILITARY, category::MILITARY),
    (mask::TOWERS, category::TOWERS),
    (mask::CIVIL, category::CIVIL),
    (mask::BUILDINGS, category::BUILDINGS),
    (mask::BARRACKS, category::BARRACKS),
];

/// How many of a category, and their strength.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Force {
    pub count: u16,
    pub strength: u32,
}
/// A kingdom's forces by category.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Forces(pub [Force; 5]);
impl Forces {
    /// 0x401f40.
    fn add(&mut self, cat: usize, strength: u32) {
        let f = &mut self.0[cat];
        f.count = f.count.wrapping_add(1);
        f.strength = f.strength.wrapping_add(strength);
    }
    /// 0x401f60: never below zero.
    fn remove(&mut self, cat: usize, strength: u32) {
        let f = &mut self.0[cat];
        f.count = f.count.saturating_sub(1);
        f.strength = f.strength.saturating_sub(strength);
    }
    /// 0x4022a0.
    pub fn strength(&self, mask: u8) -> u32 {
        MASKS
            .iter()
            .filter(|(m, _)| mask & m != 0)
            .fold(0u32, |s, &(_, c)| s.wrapping_add(self.0[c].strength))
    }
    /// 0x4022e0.
    pub fn count(&self, mask: u8) -> u32 {
        MASKS
            .iter()
            .filter(|(m, _)| mask & m != 0)
            .map(|&(_, c)| self.0[c].count as u32)
            .sum()
    }
    /// 0x402080: object `i` in its category, with its strength.
    pub(super) fn count_in(
        &mut self,
        sim: &Simulation,
        m: &MissionWorld,
        lib: &ObjectLibrary,
        c: &Catalog,
        i: usize,
    ) {
        let w = World { sim, m, lib, c };
        self.add(w.category(i), w.strength(i));
    }
}

/// A place (0x403210).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Place {
    /// Its slot + 1; 0 while it is being made.
    pub id: u16,
    pub kind: u8,
    pub param: u32,
    /// The manager's stamp at its last change (+8).
    pub stamp: u32,
    /// In cells, inside the map (+0xc).
    pub rect: [i32; 4],
    /// The anchor's rect grown by one, or the centre of the place (+0x14).
    pub anchor_rect: [i32; 4],
    /// The object it hangs on (+0x1c).
    pub anchor: Option<usize>,
    /// Forces standing in its sectors, by kingdom (+0x56).
    pub present: Vec<Forces>,
    /// Units whose destination is in it and that are not in it yet, and the
    /// buildings going up there (+0x1dc).
    pub incoming: Vec<Forces>,
    /// The places whose sectors overlap its own, the latest first (+0x362).
    pub overlaps: Vec<usize>,
    /// ReTzar's count of the places made, to tell a slot's places apart.
    pub serial: u64,
}

/// What the manager last saw of an object it counts.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Tracked {
    pub owner: u8,
    pub sector: [u16; 2],
    /// The sector its walk leads to (+0x11c); its own when still.
    pub destination: [u16; 2],
    pub unit: bool,
    pub built: bool,
    pub strength: u32,
    /// What its strength comes from, to know when to count it again.
    pub signature: [u32; 4],
}

/// The manager's places and totals.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Manager {
    /// The kingdoms' forces (+0x38).
    pub totals: Vec<Forces>,
    /// What each kingdom lost to an enemy (+0x1be), for the diagnostics.
    pub losses: Vec<Forces>,
    /// The places by slot (+0x344); ReTzar has no limit of 512.
    pub places: Vec<Option<Place>>,
    /// Bumped at every change of a place (+0xbee), never 0.
    pub stamp: u32,
    /// Places anchored per kingdom: all, then by kind (+0x19c).
    pub anchored: Vec<[u16; 7]>,
    /// The objects with the anchor flag (0x80000000).
    pub anchors: BTreeSet<usize>,
    pub side: usize,
    /// Sectors per row.
    pub sectors_side: usize,
    /// The places of each sector, the latest first (sector +0x10).
    pub sectors: Vec<Vec<usize>>,
    /// What the manager last saw of each object it counts.
    pub census: Vec<Option<Tracked>>,
    /// The researches each kingdom had then.
    pub researched: Vec<usize>,
    pub made: u64,
    /// The places made (true) and deleted since the players last heard,
    /// with their serials (0x40fc60, 0x40fcc0).
    #[serde(skip)]
    pub events: Vec<(bool, usize, u64)>,
}

/// The world the hooks read.
struct World<'a> {
    sim: &'a Simulation,
    m: &'a MissionWorld,
    lib: &'a ObjectLibrary,
    c: &'a Catalog,
}
impl World<'_> {
    fn object(&self, i: usize) -> &MapObject {
        &self.m.objects[i]
    }
    /// Units (flag 0x10).
    fn unit(&self, i: usize) -> bool {
        crate::selection::unit(self.object(i))
    }
    /// Buildings (flag 0x20).
    fn building(&self, i: usize) -> bool {
        self.object(i).building()
    }
    /// vt+0x2c0: a building that is finished.
    fn finished(&self, i: usize) -> bool {
        self.object(i).flags & FLAG_CONSTRUCTION == 0
    }
    /// Walls (0x1000) make no places.
    fn wall(&self, i: usize) -> bool {
        matches!(self.object(i).class, class::WALL_STONE | class::WALL_WOOD)
    }
    /// The classes 0x404b30 counts: up to 73, the cow, the dummy and the
    /// fishing boat.
    fn counted(&self, i: usize) -> bool {
        let c = self.object(i).class;
        c <= class::EVILCASTLE || matches!(c, class::COW | class::DUMMY | class::FISHING_BOAT)
    }
    /// The sector the lists give it now.
    fn sector_now(&self, i: usize) -> Option<[u16; 2]> {
        self.sim.gameplay.actors.get(i)?.sector
    }
    /// 0x402080: the category an object counts in.
    fn category(&self, i: usize) -> usize {
        let o = self.object(i);
        if self.unit(i) {
            if crate::selection::combat(o) {
                category::MILITARY
            } else {
                category::CIVIL
            }
        } else if o.class == class::BARRACK {
            category::BARRACKS
        } else if (39..=43).contains(&o.class) {
            category::TOWERS
        } else {
            category::BUILDINGS
        }
    }
    /// 0x401fa0.
    fn strength(&self, i: usize) -> u32 {
        strength(self.sim, self.m, self.c, i).min(u32::MAX as u64) as u32
    }
    /// What the strength reads, cheap to compare each step.
    fn signature(&self, i: usize) -> [u32; 4] {
        let o = self.object(i);
        let s = &self.sim.objects[i];
        let effects = self.sim.gameplay.actors.get(i).map_or(0, |a| {
            a.effects
                .iter()
                .filter(|e| e.until > self.sim.ticks)
                .fold(0u32, |n, e| {
                    n.wrapping_add(e.attack as u32)
                        .wrapping_add((e.defence as u32) << 16)
                })
        });
        [
            o.class as u32 | (o.max_health as u32) << 16,
            s.attack,
            s.defence,
            effects,
        ]
    }
    /// What the places count of an object now: a unit or building of a
    /// kingdom, on the map and alive; fields (0x40000) aside.
    fn tracked(&self, i: usize) -> Option<Tracked> {
        let o = self.object(i);
        if o.owner == 0
            || o.class == class::FIELD
            || o.flags & (FLAG_INACTIVE | FLAG_DYING | FLAG_HELD) != 0
            || !(self.unit(i) || self.building(i))
        {
            return None;
        }
        let sector = self.sector_now(i)?;
        let unit = self.unit(i);
        // 0x4df3b2: the first cell of the Move's rect, inside the map.
        let destination = if unit {
            super::war::heading(self.sim, i).unwrap_or(sector)
        } else {
            sector
        };
        Some(Tracked {
            owner: o.owner,
            sector,
            destination,
            unit,
            built: self.finished(i),
            strength: 0,
            signature: self.signature(i),
        })
    }
    fn researched(&self, k: usize) -> usize {
        researches(self.m, k as u8).len()
    }
}

fn sector_rect(r: [i32; 4]) -> [i32; 4] {
    r.map(|v| v.div_euclid(8))
}
fn inside(s: [u16; 2], r: [i32; 4]) -> bool {
    (r[0]..=r[2]).contains(&(s[0] as i32)) && (r[1]..=r[3]).contains(&(s[1] as i32))
}
/// 0x515d40: `outer` holds `inner` whole.
fn holds(outer: [i32; 4], inner: [i32; 4]) -> bool {
    outer[0] <= inner[0] && outer[2] >= inner[2] && outer[1] <= inner[1] && outer[3] >= inner[3]
}
fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
fn at(s: [u16; 2]) -> [i32; 2] {
    [s[0] as i32, s[1] as i32]
}

impl Manager {
    fn new(side: usize) -> Self {
        let sectors_side = side.div_ceil(8).max(1);
        Self {
            totals: vec![Forces::default(); KINGDOMS],
            losses: vec![Forces::default(); KINGDOMS],
            anchored: vec![[0; 7]; KINGDOMS],
            side,
            sectors_side,
            sectors: vec![Vec::new(); sectors_side * sectors_side],
            ..Default::default()
        }
    }
    fn bump(&mut self) -> u32 {
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.stamp = 1;
        }
        self.stamp
    }
    pub fn place(&self, slot: usize) -> Option<&Place> {
        self.places.get(slot)?.as_ref()
    }
    fn place_mut(&mut self, slot: usize) -> Option<&mut Place> {
        self.places.get_mut(slot)?.as_mut()
    }
    /// The places listed in a sector, clamped to the map's sectors.
    pub fn sector_places(&self, s: [i32; 2]) -> &[usize] {
        let n = self.sectors_side as i32;
        let [x, y] = s.map(|v| v.clamp(0, n - 1));
        &self.sectors[(y * n + x) as usize]
    }
    fn sector_places_mut(&mut self, s: [i32; 2]) -> &mut Vec<usize> {
        let n = self.sectors_side as i32;
        let [x, y] = s.map(|v| v.clamp(0, n - 1));
        &mut self.sectors[(y * n + x) as usize]
    }
    /// A sector's places as they are now, to walk while some may go.
    fn walk(&self, s: [i32; 2]) -> Vec<(usize, u64)> {
        self.sector_places(s)
            .iter()
            .filter_map(|&slot| Some((slot, self.place(slot)?.serial)))
            .collect()
    }
    fn still(&self, (slot, serial): (usize, u64)) -> bool {
        self.place(slot).is_some_and(|p| p.serial == serial)
    }
    fn sector_of(&self, slot: usize) -> [i32; 4] {
        self.place(slot).map_or([0; 4], |p| sector_rect(p.rect))
    }
    fn known(&self, i: usize) -> Option<&Tracked> {
        self.census.get(i)?.as_ref()
    }
    /// The sector of an object as the manager sees it: its last count, or
    /// the lists for what it does not count.
    fn sector(&self, w: &World, i: usize) -> Option<[u16; 2]> {
        match self.known(i) {
            Some(t) => Some(t.sector),
            None if self.census.get(i).is_some() || w.tracked(i).is_none() => w.sector_now(i),
            None => None,
        }
    }
    /// The objects the manager counts in a sector, the latest arrival
    /// first (sector +4).
    fn sector_objects(&self, w: &World, s: [i32; 2]) -> Vec<usize> {
        let mut out: Vec<usize> = (0..w.m.objects.len())
            .filter(|&i| self.known(i).is_some_and(|t| at(t.sector) == s))
            .collect();
        out.sort_by_key(|&i| std::cmp::Reverse(w.sim.gameplay.actors[i].sector_stamp));
        out
    }

    /// The manager made at the start (0x402760): every object it counts,
    /// the kingdoms' totals, then a place for every object in order
    /// (0x404990).
    fn start(w: &World) -> Self {
        let mut s = Self::new(w.sim.fog.side);
        s.census = (0..w.m.objects.len())
            .map(|i| {
                w.tracked(i).map(|mut t| {
                    t.strength = w.strength(i);
                    t
                })
            })
            .collect();
        for k in 1..12u8 {
            for i in 0..w.m.objects.len() {
                if let Some(t) = s.known(i).filter(|t| t.owner == k).copied() {
                    s.totals[k as usize].add(w.category(i), t.strength);
                }
            }
        }
        for i in 0..w.m.objects.len() {
            if w.object(i).flags & FLAG_INACTIVE == 0 {
                s.create_for(w, i);
            }
        }
        s.researched = (0..KINGDOMS).map(|k| w.researched(k)).collect();
        s
    }

    /// 0x404d00: the place an object makes.
    fn create_for(&mut self, w: &World, i: usize) {
        let o = w.object(i);
        if w.wall(i) || o.flags & FLAG_DYING != 0 {
            return;
        }
        let Some(sector) = self.sector(w, i) else {
            return;
        };
        let side = self.side as i32;
        let around = |n: i32| {
            let f = movement::footprint_rect(w.lib, o);
            let c = [(f[0] + f[2]) / 2, (f[1] + f[3]) / 2].map(|v| v.div_euclid(8));
            [
                ((c[0] - n) * 8).max(0),
                ((c[1] - n) * 8).max(0),
                ((c[0] + n) * 8 + 7).min(side - 1),
                ((c[1] + n) * 8 + 7).min(side - 1),
            ]
        };
        match o.class {
            class::RES_GOLD | class::RES_STONE => {
                let resource = u32::from(o.class == class::RES_STONE);
                self.create(w, kind::RESOURCE, resource, around(1), Some(i));
            }
            class::CASTLE => {
                let home = self.sector_places(at(sector)).iter().copied().find(|&p| {
                    self.place(p)
                        .is_some_and(|x| x.kind == kind::HOME && x.param == o.owner as u32)
                });
                match home {
                    Some(p) => {
                        if self.place(p).is_some_and(|x| x.anchor.is_none()) {
                            self.set_anchor(p, Some(i));
                        }
                    }
                    None => self.create(w, kind::HOME, o.owner as u32, around(2), Some(i)),
                }
            }
            class::MAGIC_GATEWAY if !self.anchors.contains(&i) => {
                self.create(w, kind::GATEWAY, 0, around(1), Some(i));
            }
            _ => {
                if !(1..=8).contains(&o.owner) || !self.sector_places(at(sector)).is_empty() {
                    return;
                }
                if w.unit(i) {
                    self.create(w, kind::UNITS, o.owner as u32, around(1), Some(i));
                } else if w.building(i) {
                    self.create(w, kind::BASE, o.owner as u32, around(1), Some(i));
                }
            }
        }
    }

    /// 0x403210.
    fn create(&mut self, w: &World, kind: u8, param: u32, rect: [i32; 4], anchor: Option<usize>) {
        let slot = match self.places.iter().position(Option::is_none) {
            Some(s) => s,
            None => {
                self.places.push(None);
                self.places.len() - 1
            }
        };
        let side = self.side as i32;
        let rect = [
            rect[0].max(0),
            rect[1].max(0),
            rect[2].min(side - 1),
            rect[3].min(side - 1),
        ];
        let stamp = self.bump();
        self.made += 1;
        self.places[slot] = Some(Place {
            kind,
            param,
            stamp,
            rect,
            anchor,
            present: vec![Forces::default(); KINGDOMS],
            incoming: vec![Forces::default(); KINGDOMS],
            serial: self.made,
            ..Default::default()
        });
        let sr = sector_rect(rect);
        for sy in sr[1]..=sr[3] {
            for sx in sr[0]..=sr[2] {
                self.sector_places_mut([sx, sy]).insert(0, slot);
                for i in self.sector_objects(w, [sx, sy]) {
                    self.present_add(w, i, slot, true);
                }
            }
        }
        self.overlaps(slot);
        let others: Vec<(usize, u64)> = self
            .place(slot)
            .map_or(Vec::new(), |p| p.overlaps.clone())
            .into_iter()
            .filter_map(|o| Some((o, self.place(o)?.serial)))
            .collect();
        for other in others {
            if self.still(other) && self.dead(w, other.0) {
                self.delete(w, other.0);
            }
        }
        if let Some(p) = self.place_mut(slot) {
            p.id = slot as u16 + 1;
            p.anchor = None;
        }
        self.set_anchor(slot, anchor);
        self.anchor_rect(w, slot);
        self.incoming_start(w, slot);
        if let Some(a) = anchor
            && let Some(n) = self.anchored.get_mut(w.object(a).owner as usize)
        {
            n[0] = n[0].wrapping_add(1);
            n[kind as usize] = n[kind as usize].wrapping_add(1);
        }
        self.events.push((true, slot, self.made));
    }

    /// 0x403120: the anchor flag moves with the anchor.
    fn set_anchor(&mut self, slot: usize, anchor: Option<usize>) {
        let Some(p) = self.place_mut(slot) else {
            return;
        };
        let old = std::mem::replace(&mut p.anchor, anchor);
        if let Some(o) = old {
            self.anchors.remove(&o);
        }
        if let Some(a) = anchor {
            self.anchors.insert(a);
        }
    }
    /// 0x403170.
    fn anchor_rect(&mut self, w: &World, slot: usize) {
        let Some(p) = self.place(slot) else {
            return;
        };
        let r = match p.anchor.filter(|&a| w.object(a).flags & FLAG_INACTIVE == 0) {
            Some(a) => {
                let f = movement::footprint_rect(w.lib, w.object(a));
                [f[0] - 1, f[1] - 1, f[2] + 1, f[3] + 1]
            }
            None => {
                let c = [(p.rect[0] + p.rect[2]) / 2, (p.rect[1] + p.rect[3]) / 2];
                [c[0], c[1], c[0], c[1]]
            }
        };
        let side = self.side as i32;
        let r = [r[0].max(0), r[1].max(0), r[2].min(side), r[3].min(side)];
        let losses = p.kind == kind::LOSSES;
        if let Some(p) = self.place_mut(slot) {
            p.anchor_rect = r;
        }
        if losses {
            self.set_anchor(slot, None);
        }
    }
    /// 0x405520: the units of kingdoms 1–11 on their way into the place,
    /// then the sites still to be laid of each computer player's builders,
    /// each once.
    fn incoming_start(&mut self, w: &World, slot: usize) {
        let sr = self.sector_of(slot);
        for i in 0..w.m.objects.len() {
            let Some(t) = self.known(i).copied() else {
                continue;
            };
            if (1..12).contains(&t.owner)
                && t.unit
                && t.destination != t.sector
                && inside(t.destination, sr)
                && !inside(t.sector, sr)
            {
                let (cat, s) = (w.category(i), w.strength(i));
                if let Some(p) = self.place_mut(slot) {
                    p.incoming[t.owner as usize].add(cat, s);
                }
            }
        }
        let ai = &w.sim.gameplay.ai;
        let mut counted = BTreeSet::new();
        for (&k, player) in &ai.players {
            for b in &player.lists.groups[think::group::BUILDERS as usize] {
                let Some(id) = player.lists.peons.get(b).and_then(|x| x.site) else {
                    continue;
                };
                let Some(v) = ai.virtuals.get(&id) else {
                    continue;
                };
                if counted.insert(id)
                    && inside(v.sector, sr)
                    && let Some(p) = self.place_mut(slot)
                {
                    p.incoming[k as usize].add(v.category as usize, v.strength);
                }
            }
        }
    }
    /// 0x405800: the places overlapping it, in slot order, each listed on
    /// both sides.
    fn overlaps(&mut self, slot: usize) {
        let sr = self.sector_of(slot);
        if let Some(p) = self.place_mut(slot) {
            p.overlaps.clear();
        }
        for other in 0..self.places.len() {
            if other == slot || self.place(other).is_none() {
                continue;
            }
            if overlap(sr, self.sector_of(other)) {
                if let Some(o) = self.place_mut(other) {
                    o.overlaps.insert(0, slot);
                }
                if let Some(p) = self.place_mut(slot) {
                    p.overlaps.insert(0, other);
                }
            }
        }
    }

    /// 0x403580: gone from its sectors; the objects of a sector it leaves
    /// without places make new ones.
    fn delete(&mut self, w: &World, slot: usize) {
        let Some(p) = self.place(slot).cloned() else {
            return;
        };
        if p.id == 0 {
            return;
        }
        if let Some(pl) = self.place_mut(slot) {
            pl.id = 0;
        }
        self.events.push((false, slot, p.serial));
        if let Some(a) = p.anchor.filter(|&a| w.object(a).flags & FLAG_INACTIVE == 0)
            && let Some(n) = self.anchored.get_mut(w.object(a).owner as usize)
        {
            n[0] = n[0].saturating_sub(1);
            n[p.kind as usize] = n[p.kind as usize].saturating_sub(1);
        }
        let sr = sector_rect(p.rect);
        for sy in sr[1]..=sr[3] {
            for sx in sr[0]..=sr[2] {
                let list = self.sector_places_mut([sx, sy]);
                if let Some(k) = list.iter().position(|&x| x == slot) {
                    list.remove(k);
                }
                if list.is_empty() {
                    for i in self.sector_objects(w, [sx, sy]) {
                        if !w.wall(i) && !self.anchors.contains(&i) {
                            self.create_for(w, i);
                        }
                    }
                }
            }
        }
        self.set_anchor(slot, None);
        for other in p.overlaps {
            if let Some(o) = self.place_mut(other) {
                o.overlaps.retain(|&x| x != slot);
            }
        }
        self.places[slot] = None;
    }

    /// 0x405070: whether the place has nothing left to stand for; a Home,
    /// Base or Units place may take a new anchor.
    fn dead(&mut self, w: &World, slot: usize) -> bool {
        let Some(p) = self.place(slot).cloned() else {
            return true;
        };
        let alive = |a: usize| {
            w.m.objects
                .get(a)
                .is_some_and(|o| o.flags & FLAG_INACTIVE == 0)
        };
        let k = p.param as usize;
        let structures = |pl: &Place| {
            pl.present.get(k).is_some_and(|f| f.count(0x1a) != 0)
                || pl.incoming.get(k).is_some_and(|f| f.count(0x1a) != 0)
        };
        match p.kind {
            kind::HOME | kind::BASE => {
                if !structures(&p) {
                    return true;
                }
                if p.kind == kind::BASE {
                    // A Home on its first sector holding the whole Base.
                    let sr = sector_rect(p.rect);
                    if self.sector_places([sr[0], sr[1]]).iter().any(|&h| {
                        self.place(h)
                            .is_some_and(|x| x.kind == kind::HOME && holds(sector_rect(x.rect), sr))
                    }) {
                        return true;
                    }
                }
                if p.anchor.is_some_and(|a| alive(a) && self.keeps(w, a, slot)) {
                    return false;
                }
                self.reanchor(w, slot, k)
            }
            kind::RESOURCE => !p.anchor.is_some_and(|a| {
                alive(a)
                    && matches!(w.object(a).class, class::RES_GOLD | class::RES_STONE)
                    && w.sim
                        .gameplay
                        .resources
                        .get(&a)
                        .is_none_or(|&left| left != 0)
            }),
            kind::LOSSES => {
                for a in 1..=12u8 {
                    if p.param & (1 << (a - 1)) == 0 {
                        continue;
                    }
                    for b in 1..=12u8 {
                        if b != a
                            && relation(w.m, b, a) & 0x40 == 0
                            && p.present[b as usize].count(mask::MILITARY | mask::TOWERS) != 0
                        {
                            return false;
                        }
                    }
                }
                true
            }
            kind::GATEWAY => !p.anchor.is_some_and(alive),
            kind::UNITS => {
                if let Some(a) = p.anchor.filter(|&a| alive(a))
                    && self.keeps(w, a, slot)
                    && self
                        .sector(w, a)
                        .is_some_and(|s| inside(s, sector_rect(p.rect)))
                {
                    return false;
                }
                self.reanchor(w, slot, k)
            }
            _ => true,
        }
    }
    /// 0x44e1a0 with 0x404ff0 or 0x404f90: a new anchor in the place, or
    /// none and the place dies.
    fn reanchor(&mut self, w: &World, slot: usize, k: usize) -> bool {
        let Some(rect) = self.place(slot).map(|p| p.rect) else {
            return true;
        };
        match self.search(w, rect, k, slot) {
            Some(a) => {
                self.set_anchor(slot, Some(a));
                false
            }
            None => true,
        }
    }
    /// 0x404ff0 (Homes and Bases) and 0x404f90 (Units): the object can
    /// hold the place: of its kingdom, anchoring no other place, a castle
    /// for a Home, else in a sector this place alone covers.
    fn keeps(&self, w: &World, i: usize, slot: usize) -> bool {
        let Some(p) = self.place(slot) else {
            return false;
        };
        let o = w.object(i);
        let fits = if p.kind == kind::UNITS {
            w.unit(i)
        } else {
            w.building(i) && !w.wall(i)
        };
        if !fits || o.owner as u32 != p.param {
            return false;
        }
        if self.anchors.contains(&i) && p.anchor != Some(i) {
            return false;
        }
        if p.kind == kind::HOME {
            return o.class == class::CASTLE;
        }
        self.sector(w, i)
            .is_some_and(|s| self.sector_places(at(s)).iter().all(|&x| x == slot))
    }
    /// 0x44e1a0: the first object of kingdom `k` in the rect that can hold
    /// the place: sector columns from the west, each from the north, each
    /// list from its latest arrival.
    fn search(&self, w: &World, rect: [i32; 4], k: usize, slot: usize) -> Option<usize> {
        let sr = sector_rect(rect);
        let mut found: Vec<(usize, [u16; 2])> = (0..w.m.objects.len())
            .filter_map(|i| {
                let s = self.sector(w, i)?;
                (w.object(i).owner as usize == k && inside(s, sr)).then_some((i, s))
            })
            .collect();
        found.sort_by_key(|&(i, s)| {
            (
                s[0],
                s[1],
                std::cmp::Reverse(w.sim.gameplay.actors[i].sector_stamp),
            )
        });
        found.into_iter().map(|(i, _)| i).find(|&i| {
            overlap(movement::footprint_rect(w.lib, w.object(i)), rect) && self.keeps(w, i, slot)
        })
    }

    /// 0x404b30: the object counts in the place; a finished building that
    /// is not counted for the first time leaves the place's incoming.
    fn present_add(&mut self, w: &World, i: usize, slot: usize, initial: bool) {
        if !w.counted(i) {
            return;
        }
        let stamp = self.bump();
        let (k, unit, finished) = match self.known(i) {
            Some(t) => (t.owner as usize, t.unit, t.built),
            None => (w.object(i).owner as usize, w.unit(i), w.finished(i)),
        };
        let (cat, s) = (w.category(i), w.strength(i));
        let Some(p) = self.place_mut(slot) else {
            return;
        };
        p.stamp = stamp;
        if unit || finished {
            p.present[k].add(cat, s);
            if !unit && !initial {
                p.incoming[k].remove(cat, s);
            }
        } else {
            p.incoming[k].add(cat, s);
        }
    }
    /// 0x404c10.
    fn present_remove(&mut self, w: &World, i: usize, t: &Tracked, slot: usize) {
        if !w.counted(i) {
            return;
        }
        let stamp = self.bump();
        let cat = w.category(i);
        if let Some(p) = self.place_mut(slot) {
            p.stamp = stamp;
            let k = t.owner as usize;
            if t.unit || t.built {
                p.present[k].remove(cat, t.strength);
            } else {
                p.incoming[k].remove(cat, t.strength);
            }
        }
        if self.dead(w, slot) {
            self.delete(w, slot);
        }
        if self.place(slot).is_some_and(|p| p.anchor == Some(i))
            && w.object(i).class != class::MAGIC_GATEWAY
        {
            self.set_anchor(slot, None);
        }
    }

    /// 0x402a70: an object comes into the places (`initial` after a change
    /// of owner).
    fn added(&mut self, w: &World, i: usize, t: &Tracked, initial: bool) {
        for slot in self.sector_places(at(t.sector)).to_vec() {
            self.present_add(w, i, slot, initial);
        }
        if t.unit || t.built {
            self.totals[t.owner as usize].add(w.category(i), t.strength);
        }
        if t.unit && t.destination != t.sector {
            self.incoming_at(w, i, t, t.destination, true);
        }
        self.create_for(w, i);
    }
    /// 0x402c50.
    fn removed(&mut self, w: &World, i: usize, t: &Tracked) {
        for place in self.walk(at(t.sector)) {
            if self.still(place) {
                self.present_remove(w, i, t, place.0);
            }
        }
        if t.unit && t.destination != t.sector {
            self.incoming_at(w, i, t, t.destination, false);
        }
        if t.unit || t.built {
            self.totals[t.owner as usize].remove(w.category(i), t.strength);
        }
    }
    /// The places of a destination sector that do not hold the object's own
    /// sector count it on its way, or no more.
    fn incoming_at(&mut self, w: &World, i: usize, t: &Tracked, to: [u16; 2], on: bool) {
        let cat = w.category(i);
        for slot in self.sector_places(at(to)).to_vec() {
            if inside(t.sector, self.sector_of(slot)) {
                continue;
            }
            let stamp = self.bump();
            if let Some(p) = self.place_mut(slot) {
                if on {
                    p.incoming[t.owner as usize].add(cat, t.strength);
                } else {
                    p.incoming[t.owner as usize].remove(cat, t.strength);
                }
                p.stamp = stamp;
            }
        }
    }
    /// 0x403be0.
    fn destination_changed(&mut self, w: &World, i: usize, t: &Tracked, before: [u16; 2]) {
        if before != t.sector {
            self.incoming_at(w, i, t, before, false);
        }
        if t.destination != t.sector {
            self.incoming_at(w, i, t, t.destination, true);
        }
    }
    /// 0x403840.
    fn sector_changed(&mut self, w: &World, i: usize, t: &Tracked, before: [u16; 2]) {
        let cat = w.category(i);
        let k = t.owner as usize;
        for place in self.walk(at(before)) {
            if !self.still(place) {
                continue;
            }
            let slot = place.0;
            let sr = self.sector_of(slot);
            if inside(t.sector, sr) {
                continue;
            }
            if inside(t.destination, sr) {
                let stamp = self.bump();
                if let Some(p) = self.place_mut(slot) {
                    p.incoming[k].add(cat, t.strength);
                    p.stamp = stamp;
                }
            }
            let mut was = *t;
            was.sector = before;
            self.present_remove(w, i, &was, slot);
        }
        for slot in self.sector_places(at(t.sector)).to_vec() {
            let sr = self.sector_of(slot);
            if inside(before, sr) {
                continue;
            }
            self.present_add(w, i, slot, false);
            if inside(t.destination, sr) {
                let stamp = self.bump();
                if let Some(p) = self.place_mut(slot) {
                    p.incoming[k].remove(cat, t.strength);
                    p.stamp = stamp;
                }
            }
        }
        if self.anchors.contains(&i)
            && let Some(slot) = self
                .sector_places(at(t.sector))
                .iter()
                .copied()
                .find(|&s| self.place(s).is_some_and(|p| p.anchor == Some(i)))
            && self.dead(w, slot)
        {
            self.delete(w, slot);
        }
        if self.sector_places(at(t.sector)).is_empty() {
            self.create_for(w, i);
        }
    }
    /// 0x402ea0: its strength moves by the difference everywhere it counts.
    fn strength_changed(&mut self, w: &World, i: usize, t: &Tracked, before: u32) {
        let delta = t.strength.wrapping_sub(before);
        if delta == 0 {
            return;
        }
        let (cat, k) = (w.category(i), t.owner as usize);
        for slot in self.sector_places(at(t.sector)).to_vec() {
            let stamp = self.bump();
            if let Some(p) = self.place_mut(slot) {
                let f = &mut p.present[k].0[cat];
                f.strength = f.strength.wrapping_add(delta);
                p.stamp = stamp;
            }
        }
        if t.unit && t.destination != t.sector {
            for slot in self.sector_places(at(t.destination)).to_vec() {
                if inside(t.sector, self.sector_of(slot)) {
                    continue;
                }
                let stamp = self.bump();
                if let Some(p) = self.place_mut(slot) {
                    let f = &mut p.incoming[k].0[cat];
                    f.strength = f.strength.wrapping_add(delta);
                    p.stamp = stamp;
                }
            }
        }
        let f = &mut self.totals[k].0[cat];
        f.strength = f.strength.wrapping_add(delta);
    }
    /// 0x404140, after a research of kingdom `k`: its forces counted anew,
    /// units first, then buildings.
    fn recount(&mut self, w: &World, k: usize) {
        self.totals[k] = Forces::default();
        for slot in 0..self.places.len() {
            if self.place(slot).is_none() {
                continue;
            }
            let stamp = self.bump();
            if let Some(p) = self.place_mut(slot) {
                p.stamp = stamp;
                p.present[k] = Forces::default();
                p.incoming[k] = Forces::default();
            }
        }
        for units in [true, false] {
            for i in 0..w.m.objects.len() {
                let Some(t) = self.known(i).copied() else {
                    continue;
                };
                if t.owner as usize != k || t.unit != units {
                    continue;
                }
                let strength = w.strength(i);
                if let Some(Some(x)) = self.census.get_mut(i) {
                    x.strength = strength;
                }
                if t.unit || t.built {
                    self.totals[k].add(w.category(i), strength);
                }
                for slot in self.sector_places(at(t.sector)).to_vec() {
                    self.present_add(w, i, slot, true);
                }
                if t.unit && t.destination != t.sector {
                    let cat = w.category(i);
                    for slot in self.sector_places(at(t.destination)).to_vec() {
                        if !inside(t.sector, self.sector_of(slot))
                            && let Some(p) = self.place_mut(slot)
                        {
                            p.incoming[k].add(cat, strength);
                        }
                    }
                }
            }
        }
    }
    /// 0x403e10, when `victim` dies by `killer`'s hand: its loss counted,
    /// and against an enemy a place of losses over the sectors of both,
    /// unless a place there already holds them.
    fn losses(&mut self, w: &World, victim: usize, killer: usize) {
        let v = w.object(victim);
        if v.owner == 0 {
            return;
        }
        self.losses[v.owner as usize].add(w.category(victim), w.strength(victim));
        if !auto_hostile(w.sim, w.m, killer, victim) {
            return;
        }
        let (Some(a), Some(b)) = (w.sector_now(victim), w.sector_now(killer)) else {
            return;
        };
        let mut bit = 1u32 << (v.owner - 1);
        let near = [
            a[0].min(b[0]) as i32,
            a[1].min(b[1]) as i32,
            a[0].max(b[0]) as i32,
            a[1].max(b[1]) as i32,
        ];
        let n = self.sectors_side as i32;
        let mut wide = near;
        for axis in 0..2 {
            if wide[axis] == wide[axis + 2] {
                if wide[axis] != 0 {
                    wide[axis] -= 1;
                }
                if wide[axis + 2] + 1 < n {
                    wide[axis + 2] += 1;
                }
            }
        }
        let mut create = true;
        for slot in self.sector_places(at(a)).to_vec() {
            let Some(p) = self.place(slot).cloned() else {
                continue;
            };
            let sr = sector_rect(p.rect);
            if p.kind == kind::LOSSES && holds(wide, sr) {
                if sr == wide {
                    if p.param & bit != bit {
                        let stamp = self.bump();
                        if let Some(pl) = self.place_mut(slot) {
                            pl.param |= bit;
                            pl.stamp = stamp;
                        }
                    }
                    return;
                }
                bit |= p.param;
                continue;
            }
            if holds(sr, near) {
                create = false;
                if p.kind == kind::LOSSES && p.param & bit == 0 {
                    let stamp = self.bump();
                    if let Some(pl) = self.place_mut(slot) {
                        pl.param |= bit;
                        pl.stamp = stamp;
                    }
                }
            }
        }
        if create {
            let rect = [wide[0] * 8, wide[1] * 8, wide[2] * 8 + 7, wide[3] * 8 + 7];
            self.create(w, kind::LOSSES, bit, rect, Some(killer));
        }
    }
}

/// Once a step, after the sector lists: the manager made when there are
/// computer players and none yet (0x402760), then every change since the
/// step before passed through the original's hooks.
pub(crate) fn watch(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, c: &Catalog) {
    let Some(mut mgr) = sim.gameplay.ai.manager.take() else {
        if !sim.gameplay.ai.players.is_empty() && !sim.gameplay.ai.starting {
            let manager = Manager::start(&World { sim, m, lib, c });
            sim.gameplay.ai.manager = Some(manager);
            records::sync(sim, m);
            think::schedule(sim, m);
        }
        return;
    };
    {
        let w = World { sim, m, lib, c };
        mgr.census.resize(m.objects.len(), None);
        for i in 0..m.objects.len() {
            let before = mgr.census[i];
            let mut now = w.tracked(i);
            if let (Some(b), Some(t)) = (before, now.as_mut()) {
                t.strength = if t.signature != b.signature || b.owner != t.owner {
                    w.strength(i)
                } else {
                    b.strength
                };
            } else if let Some(t) = now.as_mut() {
                t.strength = w.strength(i);
            }
            mgr.census[i] = now;
            match (before, now) {
                (None, None) => {}
                (Some(b), None) => mgr.removed(&w, i, &b),
                (None, Some(t)) => mgr.added(&w, i, &t, false),
                (Some(b), Some(t)) if b.owner != t.owner => {
                    mgr.removed(&w, i, &b);
                    mgr.added(&w, i, &t, true);
                }
                (Some(b), Some(t)) => {
                    if t.built && !b.built {
                        // 0x41e180: a finished site comes in again.
                        mgr.added(&w, i, &t, false);
                    }
                    if t.strength != b.strength {
                        let mut was = t;
                        was.sector = b.sector;
                        was.destination = b.destination;
                        mgr.strength_changed(&w, i, &was, b.strength);
                    }
                    if t.destination != b.destination {
                        let mut was = t;
                        was.sector = b.sector;
                        mgr.destination_changed(&w, i, &was, b.destination);
                    }
                    if t.sector != b.sector {
                        mgr.sector_changed(&w, i, &t, b.sector);
                    }
                }
            }
        }
        for k in 1..KINGDOMS {
            if mgr.researched.get(k).copied().unwrap_or(0) != w.researched(k) {
                mgr.recount(&w, k);
            }
        }
        mgr.researched = (0..KINGDOMS).map(|k| w.researched(k)).collect();
    }
    sim.gameplay.ai.manager = Some(mgr);
    records::sync(sim, m);
}

/// A computer player's site still to be laid comes into the places of its
/// sector on its way (0x40b890 → 0x402a70), or leaves them (0x40bd9e →
/// 0x402c50), where a place left without reason to be goes.
pub(crate) fn virtual_site(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    id: u32,
    on: bool,
) {
    let Some(v) = sim.gameplay.ai.virtuals.get(&id).copied() else {
        return;
    };
    let Some(mut mgr) = sim.gameplay.ai.manager.take() else {
        return;
    };
    {
        let w = World { sim, m, lib, c };
        let (k, cat) = (v.owner as usize, v.category as usize);
        for place in mgr.walk(at(v.sector)) {
            if !mgr.still(place) {
                continue;
            }
            let stamp = mgr.bump();
            if let Some(p) = mgr.place_mut(place.0) {
                p.stamp = stamp;
                if on {
                    p.incoming[k].add(cat, v.strength);
                } else {
                    p.incoming[k].remove(cat, v.strength);
                }
            }
            if !on && mgr.dead(&w, place.0) {
                mgr.delete(&w, place.0);
            }
        }
    }
    sim.gameplay.ai.manager = Some(mgr);
    records::sync(sim, m);
}

/// 0x403e10 from a death (0x4e22a9, 0x41db7b), before the step's watch
/// takes the victim off the places.
pub(crate) fn died(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    victim: usize,
    killer: usize,
) {
    let Some(mut mgr) = sim.gameplay.ai.manager.take() else {
        return;
    };
    mgr.losses(&World { sim, m, lib, c }, victim, killer);
    sim.gameplay.ai.manager = Some(mgr);
    records::sync(sim, m);
}

#[cfg(test)]
#[path = "places_tests.rs"]
mod tests;
