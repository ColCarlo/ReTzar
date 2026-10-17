//! A computer player's war (0x410be0): its places evaluated in turn by
//! priority, and one group a phase sent to the first whose friendly
//! fighters fall short of its minimum (0x410340, 0x4113a0), the army's
//! candidates scored and sorted (0x4111b0, 0x410cd0, 0x410fe0) and given
//! Approach (program 0x28, 0x42dde0) through the orders with transport.
//! The program's own ops (0x4f, 0x50) are `approach_check` and
//! `wait_others`.
use super::records::{self, Record, gap};
use super::rules::World;
use super::*;
use places::mask;

/// A candidate this near the place's anchor is near (0x41127a).
const NEAR: u32 = 32;
/// Tries for a point of the place on the island of its anchor (0x41167a).
const POINT_TRIES: u32 = 100;
/// The range round the place's anchor a member of the group stops at
/// (0x41172c).
const RADIUS: (i32, i32) = (24, 32);
/// On easy another kingdom's home waits this long after a group went to
/// one (0x411792).
const HOME_PAUSE_MS: u64 = 900_000;
/// Wait others looks again after this (0x4e12ea).
pub const WAIT_MS: u64 = 3000;
/// Half the minimum is enough with this chance in a hundred (0x411171).
const BOLD: i32 = 10;

/// A unit of a group on its way to a place (Approach, 0x42dde0): the place
/// (slot and serial), the anchor's rect it walks to within `radius`, the
/// point it advances to at last, the step (1 Check approach distance, 2 the
/// walk, 3 Wait others, 4 the advance to the point), the sector it counts
/// as heading to while it waits (`+0x11c`), and when it looks again.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Approach {
    pub place: usize,
    pub serial: u64,
    pub rect: [i32; 4],
    pub point: [u16; 2],
    pub radius: u16,
    pub step: u8,
    pub sector: Option<[u16; 2]>,
    pub due: u64,
}

/// 0x52e320, the game's own sort: a quicksort whose pivot is followed by
/// its place, down to runs of seven, which a bubble sort ends (0x52e2b0).
/// Equal elements end where this order puts them.
pub(crate) fn native_sort<T>(v: &mut [T], cmp: &mut impl FnMut(&T, &T) -> i16) {
    let mut base = 0usize;
    let mut num = v.len();
    while num > 7 {
        let mut lo = base;
        let mut hi = base + num - 1;
        let mut mid = base + num / 2;
        loop {
            while lo != mid && cmp(&v[lo], &v[mid]) <= 0 {
                lo += 1;
            }
            while hi != mid && cmp(&v[mid], &v[hi]) <= 0 {
                hi -= 1;
            }
            if lo >= hi {
                break;
            }
            if mid == lo {
                v.swap(lo, hi);
                mid = hi;
            } else {
                if mid == hi {
                    mid = lo;
                }
                v.swap(lo, hi);
            }
        }
        let left = mid - base;
        let right = num - left - 1;
        if left > right {
            native_sort(&mut v[mid + 1..mid + 1 + right], cmp);
            num = left;
        } else {
            native_sort(&mut v[base..base + left], cmp);
            base = mid + 1;
            num = right;
        }
    }
    // 0x52e2b0: passes until one swaps nothing.
    let run = &mut v[base..base + num];
    let mut passes = run.len().saturating_sub(1);
    let mut swapped = true;
    while passes != 0 && swapped {
        swapped = false;
        for j in 0..passes {
            if cmp(&run[j], &run[j + 1]) > 0 {
                run.swap(j, j + 1);
                swapped = true;
            }
        }
        passes -= 1;
    }
}

fn cell(o: &MapObject) -> [i32; 2] {
    [o.x as i32, o.y as i32]
}
/// A place's rect in sectors (the cells divided by 8).
fn sectors(r: [i32; 4]) -> [i32; 4] {
    r.map(|v| v / 8)
}
fn inside(s: [u16; 2], r: [i32; 4]) -> bool {
    let (x, y) = (s[0] as i32, s[1] as i32);
    x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3]
}
fn valid(region: u8) -> bool {
    !matches!(region, islands::NONE | islands::WIPED)
}

/// The units of kingdom `k` still on their walk to the place (step 2 of
/// Approach): the record's groups on their way (`+0x84`), counted up by op
/// 0x4f and down when that walk ends (0x4e340b), however it ends.
pub(crate) fn on_their_way(
    sim: &Simulation,
    m: &MissionWorld,
    k: u8,
    place: usize,
    serial: u64,
) -> u16 {
    sim.gameplay
        .actors
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            m.objects.get(*i).is_some_and(|o| o.owner == k && alive(o))
                && match &a.intent {
                    Some(Intent::Approach(ap)) => {
                        ap.step == 2 && ap.place == place && ap.serial == serial
                    }
                    // A ship carrying a group, on its way to the place (op 10).
                    Some(Intent::Transport(t)) => {
                        t.step == 10
                            && t.war
                                .is_some_and(|x| x.place == place && x.serial == serial)
                    }
                    _ => false,
                }
        })
        .count() as u16
}

/// The record of kingdom `k`'s player for the place of `ap`, if both still
/// stand.
fn record_for<'a>(sim: &'a Simulation, k: u8, place: usize, serial: u64) -> Option<&'a Record> {
    let mgr = sim.gameplay.ai.manager.as_ref()?;
    mgr.place(place).filter(|p| p.serial == serial)?;
    sim.gameplay
        .ai
        .players
        .get(&k)?
        .records
        .iter()
        .find(|r| r.place == place && r.serial == serial)
}

/// Op 0x4f, Check approach distance (0x4e2a11): within the place's anchor
/// rect grown by the range, the unit counts as heading to the anchor's
/// sector and waits (`Some`); else it walks there (`None`). Without its
/// place or record it waits too.
pub(crate) fn approach_check(
    sim: &Simulation,
    m: &MissionWorld,
    i: usize,
    ap: &Approach,
) -> Option<Option<[u16; 2]>> {
    let k = m.objects[i].owner;
    let mgr = sim.gameplay.ai.manager.as_ref()?;
    if record_for(sim, k, ap.place, ap.serial).is_none() {
        return Some(None);
    }
    let place = mgr.place(ap.place)?;
    let r = ap.radius as i32;
    let a = place.anchor_rect;
    let rect = [a[0] - r, a[1] - r, a[2] + r, a[3] + r];
    let p = cell(&m.objects[i]);
    if p[0] < rect[0] || p[0] > rect[2] || p[1] < rect[1] || p[1] > rect[3] {
        return None;
    }
    let last = mgr.side as i32 - 1;
    let s = [a[0].clamp(0, last) / 8, a[1].clamp(0, last) / 8];
    Some(Some([s[0] as u16, s[1] as u16]))
}

/// Op 0x50, Wait others (0x413c80) after its look for a target: while the
/// place is threatened and its friendly fighters and towers fall short of
/// half its minimum, wait (0) as long as a record of the place's own or
/// allied players has groups on their way; then a ship goes (1), anyone
/// else goes only with the friendly forces, those on their way included, at
/// half the minimum, otherwise gives up (2). Without its place it gives up
/// (the original reads past the step's table there).
pub(crate) fn wait_others(sim: &mut Simulation, m: &MissionWorld, i: usize, ap: &Approach) -> u8 {
    let k = m.objects[i].owner;
    if record_for(sim, k, ap.place, ap.serial).is_none() {
        return 2;
    }
    let Some(mut p) = sim.gameplay.ai.players.get_mut(&k).map(std::mem::take) else {
        return 2;
    };
    let at = p
        .records
        .iter()
        .position(|r| r.place == ap.place && r.serial == ap.serial)
        .unwrap_or_default();
    if let Some(mgr) = sim.gameplay.ai.manager.as_ref() {
        records::evaluate(&mut p, k, at, mgr, m, mgr.side);
    }
    let r = p.records[at];
    if let Some(slot) = sim.gameplay.ai.players.get_mut(&k) {
        *slot = p;
    }
    let half = r.minimum >> 1;
    if r.hostile.strength(mask::MILITARY | mask::TOWERS) == 0
        || r.friendly.strength(mask::MILITARY | mask::TOWERS) >= half
    {
        return 1;
    }
    for b in 1..=12u8 {
        if (b == k || relation(m, k, b) & 0x40 != 0)
            && record_for(sim, b, ap.place, ap.serial).is_some()
            && on_their_way(sim, m, b, ap.place, ap.serial) != 0
        {
            return 0;
        }
    }
    if matches!(m.objects[i].class, 23..=25) {
        return 1;
    }
    let ready = r
        .friendly_incoming
        .strength(mask::MILITARY)
        .wrapping_add(r.friendly.strength(mask::MILITARY | mask::TOWERS));
    if ready >= half { 1 } else { 2 }
}

/// The sector a unit heads to (`+0x11c`, 0x4df3b2): the first cell of its
/// walk's rect, the one Approach counts while waiting, else its own.
pub(crate) fn heading(sim: &Simulation, i: usize) -> Option<[u16; 2]> {
    let a = sim.gameplay.actors.get(i)?;
    if let Some(mv) = a.movement.as_ref() {
        let last = sim.fog.side.max(1) as i32 - 1;
        return Some([mv.rect[0], mv.rect[1]].map(|v| (v.clamp(0, last) / 8) as u16));
    }
    if let Some(Intent::Approach(ap)) = &a.intent
        && ap.step == 3
        && let Some(s) = ap.sector
    {
        return Some(s);
    }
    a.sector
}

impl Player {
    /// 0x410be0: the War phase. Every record is evaluated again in turn and
    /// the first short of its minimum sends a group; the list is sorted
    /// again when a priority changed (0x40fbf0).
    pub(super) fn war(&mut self, w: &mut World) {
        let k = self.kingdom;
        let mut moved = false;
        let mut at = 0;
        while at < self.records.len() {
            let before = self.records[at].priority;
            if let Some(mgr) = w.sim.gameplay.ai.manager.as_ref() {
                records::evaluate(self, k, at, mgr, w.m, mgr.side);
            }
            moved |= self.records[at].priority != before;
            if self.short(w, at) {
                break;
            }
            at += 1;
        }
        if moved {
            self.records.sort_by_key(|r| std::cmp::Reverse(r.priority));
        }
    }

    /// 0x410340: a place whose friendly fighters, present and coming, fall
    /// short of its minimum gets a group.
    fn short(&mut self, w: &mut World, at: usize) -> bool {
        let r = self.records[at];
        let friendly = r
            .friendly_incoming
            .strength(mask::MILITARY)
            .wrapping_add(r.friendly.strength(mask::MILITARY));
        friendly < r.minimum && self.send(w, at)
    }

    /// 0x4113a0: a group for the place. Another kingdom's home (one not
    /// sharing control) or a place with only hostile forces waits for the
    /// player's pause; an overlapping place of higher priority threatened
    /// keeps the army. The candidates are taken by score until their
    /// strength reaches the wanted one (or, when the near ones suffice, the
    /// near strength); those already heading there or inside count without
    /// orders, the others form a group and get Approach to a random point of
    /// the place on its anchor's island, each with its own range.
    fn send(&mut self, w: &mut World, at: usize) -> bool {
        let k = self.kingdom;
        let r = self.records[at];
        let Some(place) = w
            .sim
            .gameplay
            .ai
            .manager
            .as_ref()
            .and_then(|mgr| mgr.place(r.place))
            .filter(|p| p.serial == r.serial)
            .cloned()
        else {
            return false;
        };
        let others_home =
            place.kind == places::kind::HOME && relation(w.m, place.param as u8, k) & 4 == 0;
        let hostile_only = r.hostile.count(0xff) != 0 && r.friendly.count(0xff) == 0;
        let now = w.sim.ticks * TICK_MS;
        if (others_home || hostile_only) && self.peace_until != 0 && self.peace_until > now {
            return false;
        }
        for &o in &place.overlaps {
            let Some(other) = self.record_at(w, o) else {
                continue;
            };
            if other.priority > r.priority
                && (other.hostile.strength(mask::MILITARY | mask::TOWERS) != 0
                    || other.hostile_incoming.strength(mask::MILITARY) != 0)
            {
                return false;
            }
        }
        let (mode, near, pool) = self.candidates(w, at, &place);
        if mode == 0 {
            return false;
        }
        let mut members = Vec::new();
        let mut total = 0u32;
        let rect = sectors(place.rect);
        for (i, score) in pool {
            if score == 0 {
                continue;
            }
            if !self.there(w, i, rect) {
                members.push(i);
            }
            total = total.wrapping_add(strength(w.sim, w.m, w.c, i).min(u32::MAX as u64) as u32);
            if total >= r.wanted || (mode == 1 && total >= near) {
                break;
            }
        }
        if members.is_empty() {
            return false;
        }
        // The anchor's island, of its cell or round it.
        let isl = &w.sim.gameplay.islands;
        let region = match place.anchor {
            Some(a) => {
                let here = isl.region(cell(&w.m.objects[a]));
                if valid(here) {
                    here
                } else {
                    let r = movement::footprint_rect(w.lib, &w.m.objects[a]);
                    isl.nearest(w.g, [r[0] - 1, r[1] - 1, r[2] + 1, r[3] + 1], 0)
                }
            }
            None => isl.nearest(w.g, place.anchor_rect, 0),
        };
        let mut point = [place.anchor_rect[0], place.anchor_rect[1]];
        if valid(region) {
            for _ in 0..POINT_TRIES {
                let rng = &mut w.sim.gameplay.game_rng;
                let x = rng.range(place.rect[0], place.rect[2]);
                let y = rng.range(place.rect[1], place.rect[3]);
                point = [x, y];
                if w.sim.gameplay.islands.region(point) == region {
                    break;
                }
            }
        }
        let target = [point[0] as u16, point[1] as u16];
        // 0x507940 puts the latest first: the orders go that way.
        for &i in members.iter().rev() {
            let radius = w.sim.gameplay.game_rng.range(RADIUS.0, RADIUS.1) as u16;
            let ap = Approach {
                place: r.place,
                serial: r.serial,
                rect: place.anchor_rect,
                point: target,
                radius,
                step: 1,
                sector: None,
                due: 0,
            };
            self.command(w, i, Intent::Approach(ap), point, None);
        }
        // 0x4518f0: they share their paths.
        let id = w
            .sim
            .gameplay
            .groups
            .keys()
            .next_back()
            .map_or(1, |g| g + 1);
        for &i in &members {
            w.sim.gameplay.actors[i].group = Some(id);
        }
        w.sim.gameplay.groups.insert(
            id,
            movement::Group {
                members,
                paths: Vec::new(),
            },
        );
        let used: BTreeSet<u32> = w
            .sim
            .gameplay
            .actors
            .iter()
            .filter_map(|a| a.group)
            .collect();
        w.sim.gameplay.groups.retain(|g, _| used.contains(g));
        if others_home && self.peace_until != 0 && self.mode & 0x80 == 0 {
            self.peace_until = now + HOME_PAUSE_MS;
        }
        true
    }

    /// The player's record of a place by its slot (`place + 0x22`).
    fn record_at(&self, w: &World, slot: usize) -> Option<Record> {
        let mgr = w.sim.gameplay.ai.manager.as_ref()?;
        let serial = mgr.place(slot)?.serial;
        self.records
            .iter()
            .find(|r| r.place == slot && r.serial == serial)
            .copied()
    }

    /// 0x4111b0: the army's candidates scored (0x410fe0), their strength
    /// near the place's anchor and farther; whether the near ones suffice
    /// (1), or all (2), else none (0), but a friendly home invaded takes
    /// them all. The army is then sorted by score (0x52e320 with 0x4110d0)
    /// and keeps that order.
    fn candidates(
        &mut self,
        w: &mut World,
        at: usize,
        place: &places::Place,
    ) -> (u8, u32, Vec<(usize, u16)>) {
        let k = self.kingdom;
        if self.lists.army.is_empty() {
            return (0, 0, Vec::new());
        }
        let (mut near, mut far) = (0u32, 0u32);
        let mut pool = Vec::with_capacity(self.lists.army.len());
        for i in self.lists.army.clone() {
            let score = if alive(&w.m.objects[i]) {
                self.score(w, i, at, place)
            } else {
                0
            };
            if score != 0 {
                let s = strength(w.sim, w.m, w.c, i).min(u32::MAX as u64) as u32;
                if gap(cell(&w.m.objects[i]), place.anchor_rect) < NEAR {
                    near = near.wrapping_add(s);
                } else {
                    far = far.wrapping_add(s);
                }
            }
            pool.push((i, score));
        }
        if near == 0 && far == 0 {
            return (0, near, Vec::new());
        }
        let mut mode = if self.enough(w, at, place, near) {
            1
        } else if self.enough(w, at, place, near.wrapping_add(far)) {
            2
        } else {
            0
        };
        let r = self.records[at];
        let invaded = place.kind == places::kind::HOME
            && relation(w.m, k, place.param as u8) & 0x40 != 0
            && r.hostile.count(mask::MILITARY | mask::TOWERS) != 0;
        if mode == 0 {
            if !invaded {
                return (0, near, Vec::new());
            }
            mode = 2;
        }
        native_sort(&mut pool, &mut |a, b| {
            if a.1 < b.1 {
                1
            } else if a.1 == b.1 {
                0
            } else {
                -1
            }
        });
        self.lists.army = pool.iter().map(|e| e.0).collect();
        (mode, near, pool)
    }

    /// 0x411100: the friendly fighters of the other kingdoms, present and
    /// coming, with `s` reach the minimum (half of it with one chance in
    /// ten, drawn every time).
    fn enough(&self, w: &mut World, at: usize, place: &places::Place, s: u32) -> bool {
        let k = self.kingdom as usize;
        let r = self.records[at];
        let others = r
            .friendly
            .strength(mask::MILITARY)
            .wrapping_sub(place.present[k].strength(mask::MILITARY))
            .wrapping_sub(place.incoming[k].strength(mask::MILITARY))
            .wrapping_add(r.friendly_incoming.strength(mask::MILITARY));
        let roll = w.sim.gameplay.game_rng.range(1, 100);
        let need = if roll <= BOLD {
            r.minimum >> 1
        } else {
            r.minimum
        };
        others.wrapping_add(s) >= need
    }

    /// 0x410fe0: a candidate (0x410cd0) scores the map's side in sectors
    /// less its sector's distance from the place's, two thirds of it while
    /// attacking; anyone else 0.
    fn score(&mut self, w: &mut World, i: usize, at: usize, place: &places::Place) -> u16 {
        if !self.candidate(w, i, at, place) {
            return 0;
        }
        let Some(s) = w.sim.gameplay.actors[i].sector else {
            return 0;
        };
        let d = gap([s[0] as i32, s[1] as i32], sectors(place.rect));
        let mut score = ((w.g.side / 8) as u32).wrapping_sub(d) as u16;
        if program(w.sim, w.m, i).kind == program::ATTACK {
            score = ((score as u32 * 2) / 3) as u16;
        }
        score
    }

    /// 0x410cd0: a fighter the player may send: idle, walking or
    /// approaching, or fighting a wall, a training dummy or nothing left;
    /// able to reach the place unless it flies; and not kept by the places
    /// of the sector it heads to: one threatened while the place is calm, or
    /// one as important that cannot spare it (0x410c70). Heading to a calm
    /// place, only units of the barracks are taken.
    fn candidate(&mut self, w: &mut World, i: usize, at: usize, place: &places::Place) -> bool {
        let o = &w.m.objects[i];
        if o.flags & (FLAG_STORY | FLAG_DYING) != 0 || w.sim.objects[i].holder.is_some() {
            return false;
        }
        match program(w.sim, w.m, i).kind {
            program::IDLE | program::MOVE | program::APPROACH => {}
            program::ATTACK => {
                // At its Attack op (0xd), on nothing, a wall or a dummy.
                let fighting = w.sim.objects[i].order == OrderState::Attacking;
                let free = match w.sim.gameplay.actors[i].intent {
                    Some(Intent::Attack(t)) => w.m.objects.get(t).is_none_or(|x| {
                        !alive(x)
                            || matches!(
                                x.class,
                                class::WALL_STONE
                                    | class::WALL_WOOD
                                    | class::GATE_WOOD
                                    | class::GATE_STONE
                                    | class::DUMMY
                            )
                    }),
                    _ => true,
                };
                if !(fighting && free) {
                    return false;
                }
            }
            _ => return false,
        }
        if w.lib.layer(&w.m.objects[i]) != 0 {
            let from = cell(&w.m.objects[i]);
            let rect = match place.anchor {
                Some(a) => {
                    let r = movement::footprint_rect(w.lib, &w.m.objects[a]);
                    [r[0] - 1, r[1] - 1, r[2] + 1, r[3] + 1]
                }
                None => place.anchor_rect,
            };
            if !super::build::reach(w, self, from, rect) {
                return false;
            }
        }
        let Some(dest) = heading(w.sim, i) else {
            return true;
        };
        let r = self.records[at];
        let calm = r.hostile.strength(mask::MILITARY | mask::TOWERS) == 0;
        if calm {
            let civ = w.m.objects[i].civilization;
            let class = w.m.objects[i].class;
            if super::tree::producer(w.c, w.lib, civ, class) != Some(class::BARRACK) {
                return false;
            }
        }
        let slots: Vec<usize> = w
            .sim
            .gameplay
            .ai
            .manager
            .as_ref()
            .map_or(Vec::new(), |mgr| {
                mgr.sector_places([dest[0] as i32, dest[1] as i32]).to_vec()
            });
        for slot in slots {
            let Some(other) = self.records.iter().position(|x| {
                x.place == slot
                    && w.sim
                        .gameplay
                        .ai
                        .manager
                        .as_ref()
                        .and_then(|mgr| mgr.place(slot))
                        .is_some_and(|p| p.serial == x.serial)
            }) else {
                continue;
            };
            if other == at {
                return true;
            }
            if calm && self.records[other].hostile.count(0xff) != 0 {
                return false;
            }
            if self.records[other].priority >= r.priority {
                if let Some(mgr) = w.sim.gameplay.ai.manager.as_ref() {
                    records::evaluate(self, self.kingdom, other, mgr, w.m, mgr.side);
                }
                if !spare(w, i, &self.records[other]) {
                    return false;
                }
            }
        }
        true
    }

    /// 0x4118e0: a fighter already heading to the place's sectors, or in
    /// them (back to its default program unless idle or fighting).
    fn there(&mut self, w: &mut World, i: usize, rect: [i32; 4]) -> bool {
        if heading(w.sim, i).is_some_and(|d| inside(d, rect)) {
            return true;
        }
        let Some(s) = w.sim.gameplay.actors[i].sector else {
            return false;
        };
        if !inside(s, rect) {
            return false;
        }
        let kind = program(w.sim, w.m, i).kind;
        if kind != program::IDLE && kind != program::ATTACK {
            think::restart(w.sim, i);
            self.hooked(w, i);
        }
        true
    }
}

/// 0x410c70: the place keeps more friendly fighters, present and coming,
/// than its minimum by at least the unit's strength.
fn spare(w: &World, i: usize, r: &Record) -> bool {
    let unit = strength(w.sim, w.m, w.c, i).min(u32::MAX as u64) as u32;
    let have = r
        .friendly
        .strength(mask::MILITARY)
        .wrapping_add(r.friendly_incoming.strength(mask::MILITARY));
    have > r.minimum && have - r.minimum >= unit
}

/// Query 0x271e: what a peasant does not capture (walls and gates, the
/// field, the magic gateway, the mines, the training dummy).
pub(super) fn uncaptured(class: usize) -> bool {
    matches!(
        class,
        class::WALL_STONE
            | class::WALL_WOOD
            | class::FIELD
            | class::GATE_WOOD
            | class::GATE_STONE
            | 72
            | class::RES_GOLD
            | class::RES_STONE
            | class::DUMMY
    )
}

/// The militias called during the step (0x413a99), each by its player.
pub(crate) fn alarms(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
) {
    for (k, threat) in std::mem::take(&mut sim.gameplay.ai.alarms) {
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
        if w.m.objects.get(threat).is_some_and(alive) {
            p.militia(&mut w, threat);
        }
        if let Some(slot) = w.sim.gameplay.ai.players.get_mut(&k) {
            *slot = p;
        }
    }
}

impl Player {
    /// 0x411ed0, from the hook on a new object: one the kingdom could
    /// capture and not allied calls the militia.
    pub(super) fn alarm(&mut self, w: &mut World, i: usize) {
        let k = self.kingdom;
        if capturable(w.sim, w.m, w.lib, k, i) && relation(w.m, k, w.m.objects[i].owner) & 0x40 == 0
        {
            self.militia(w, i);
        }
    }

    /// 0x4120d0: the peasants against a threat. Of the places of its sector
    /// with civilians of the kingdom, a threat it could capture takes the
    /// one with most of them; otherwise none is needed while the friendly
    /// fighters exceed 1.5 times the hostile fighters and towers, and the
    /// first where fighters and civilians reach three times those is taken.
    /// Outside its home the kingdom's fighters and civilians there must
    /// match the hostile ones. The peasants are sorted by their distance
    /// from the threat (0x411f70, the ineligible last); a first count adds
    /// the nearest up to N1, then from the nearest again, the count going
    /// on, each is sent until it reaches N2 (a capture counting one each).
    fn militia(&mut self, w: &mut World, threat: usize) {
        let k = self.kingdom;
        let ku = k as usize;
        let capture = capturable(w.sim, w.m, w.lib, k, threat);
        let Some(sector) = w.sim.gameplay.actors[threat].sector else {
            return;
        };
        let Some(slots) = w.sim.gameplay.ai.manager.as_ref().map(|mgr| {
            mgr.sector_places([sector[0] as i32, sector[1] as i32])
                .to_vec()
        }) else {
            return;
        };
        let mut home = false;
        let mut best: Option<places::Place> = None;
        for slot in slots {
            let Some(place) = w
                .sim
                .gameplay
                .ai
                .manager
                .as_ref()
                .and_then(|mgr| mgr.place(slot))
                .cloned()
            else {
                continue;
            };
            if place.kind == places::kind::HOME && place.param == k as u32 {
                home = true;
            }
            let civil = place.present[ku].count(mask::CIVIL);
            if civil == 0 {
                continue;
            }
            if capture {
                if best
                    .as_ref()
                    .is_none_or(|b| b.present[ku].count(mask::CIVIL) < civil)
                {
                    best = Some(place);
                }
                continue;
            }
            let Some(at) = self
                .records
                .iter()
                .position(|r| r.place == slot && r.serial == place.serial)
            else {
                continue;
            };
            if let Some(mgr) = w.sim.gameplay.ai.manager.as_ref() {
                records::evaluate(self, k, at, mgr, w.m, mgr.side);
            }
            let r = self.records[at];
            let hostile = r
                .hostile
                .strength(mask::MILITARY | mask::TOWERS)
                .wrapping_mul(3);
            let friendly = r.friendly.strength(mask::MILITARY);
            if friendly > hostile >> 1 {
                return;
            }
            if friendly.wrapping_add(place.present[ku].strength(mask::CIVIL)) >= hostile {
                best = Some(place);
                break;
            }
        }
        let Some(place) = best else {
            return;
        };
        if !home {
            let Some(r) = self.record_at(w, place.id.saturating_sub(1) as usize) else {
                return;
            };
            if r.friendly.strength(mask::MILITARY | mask::CIVIL)
                < r.hostile.strength(mask::MILITARY | mask::TOWERS)
            {
                return;
            }
        }
        // 0x411f70 with 0x412070, through the list's sort (0x52ea20).
        let rect = sectors(place.rect);
        let threat_rect = movement::footprint_rect(w.lib, &w.m.objects[threat]);
        let key = |w: &World, i: usize| -> u32 {
            let o = &w.m.objects[i];
            if !alive(o)
                || o.flags & (FLAG_STORY | FLAG_DYING) != 0
                || w.sim.objects[i].holder.is_some()
                || !w.sim.gameplay.actors[i]
                    .sector
                    .is_some_and(|s| inside(s, rect))
                || matches!(
                    program(w.sim, w.m, i).kind,
                    program::ATTACK | program::CAPTURE
                )
            {
                return u32::MAX;
            }
            super::build::rect_gap(movement::footprint_rect(w.lib, o), threat_rect)
        };
        let mut keyed: Vec<(usize, u32)> = self
            .lists
            .peasants
            .iter()
            .map(|&i| (i, key(w, i)))
            .collect();
        native_sort(&mut keyed, &mut |a, b| {
            if a.1 < b.1 {
                -1
            } else if a.1 == b.1 {
                0
            } else {
                1
            }
        });
        self.lists.peasants = keyed.iter().map(|e| e.0).collect();
        let resistance = |w: &World| {
            resistance_target(w.sim, w.m, w.lib, threat) + w.sim.objects[threat].resistance as i32
        };
        let worth = |w: &World, i: usize| -> u32 {
            if capture {
                1
            } else {
                strength(w.sim, w.m, w.c, i).min(u32::MAX as u64) as u32
            }
        };
        let threat_strength = strength(w.sim, w.m, w.c, threat).min(u32::MAX as u64) as u32;
        let first = if capture {
            (resistance(w) / 15 + 1) as u32
        } else {
            threat_strength.wrapping_mul(3) >> 1
        };
        let eligible = |w: &World, i: usize| -> bool {
            let o = &w.m.objects[i];
            alive(o)
                && o.flags & (FLAG_STORY | FLAG_DYING) == 0
                && w.sim.objects[i].holder.is_none()
                && w.sim.gameplay.actors[i]
                    .sector
                    .is_some_and(|s| inside(s, rect))
                && !matches!(
                    program(w.sim, w.m, i).kind,
                    program::ATTACK | program::CAPTURE
                )
        };
        let peasants = self.lists.peasants.clone();
        let mut sum = 0u32;
        for &i in &peasants {
            // The first count also stops at a frozen or stone peasant.
            if !eligible(w, i)
                || has_effect(w.sim, i, effect::FREEZE)
                || has_effect(w.sim, i, effect::STONE)
            {
                break;
            }
            sum = sum.wrapping_add(worth(w, i));
            if sum >= first {
                break;
            }
        }
        let second = if capture {
            (resistance(w) / 10 + 1) as u32
        } else {
            threat_strength.wrapping_mul(2)
        };
        let o = &w.m.objects[threat];
        let at = [o.x as i32, o.y as i32];
        for &i in &peasants {
            if !eligible(w, i) {
                break;
            }
            // 0x412540: each peasant's program against the threat (vt+0x2b0).
            let intent = super::units::target_program(w.sim, w.m, w.lib, w.c, i, threat);
            self.command(w, i, intent, at, Some(threat));
            sum = sum.wrapping_add(worth(w, i));
            if sum >= second {
                break;
            }
        }
    }
}

#[cfg(test)]
#[path = "war_tests.rs"]
mod tests;
