//! A computer player's Special phase (0x409220): what it may spend read
//! again, then, at war, the towers it raises (0x4107d0) and the defences
//! its places ask for (0x410b60, 0x4104f0), paid from the part of the
//! defences (`bucket::DEFENCE`).
use super::places::category;
use super::rules::{self, World, bucket};
use super::*;

/// The towers the player keeps where it lives, and beyond the easy level
/// once it has as many (0x41056d, 0x41057d); one elsewhere.
const HOME_TOWERS: [u16; 2] = [5, 10];

impl Player {
    /// 0x409220: the Special phase.
    pub(super) fn special(&mut self, w: &mut World) {
        self.available = rules::spendable(w.m, self);
        self.buckets[bucket::NAVY] = [0; 4];
        self.navy(w);
        if self.mode & 2 != 0 {
            self.buckets[bucket::DEFENCE] = [0; 4];
            self.towers(w);
            self.defences(w);
        }
    }

    /// 0x4107d0: each finished wooden tower not raising already becomes a
    /// wooden tower 2; beyond the easy level a wooden tower 2 becomes a
    /// stone tower 2 with one chance in three when it can, else a stone
    /// tower. Each kind is looked at once a pass, an upgrade the player
    /// cannot pay booked; the first it pays starts at once (queued behind
    /// what the tower does) and ends the pass.
    fn towers(&mut self, w: &mut World) -> bool {
        let mut seen = 0u8;
        for t in self.lists.towers.clone() {
            let o = &w.m.objects[t];
            if !alive(o) || o.flags & FLAG_CONSTRUCTION != 0 || matches!(o.class, 39 | 42 | 43) {
                continue;
            }
            // The tower's current op is already the Upgrade (0x2e).
            if matches!(
                w.sim.gameplay.actors[t].jobs.front(),
                Some(Job {
                    kind: JobKind::Upgrade(..),
                    ..
                })
            ) {
                continue;
            }
            let (name, to, bit) = if o.class == 40 {
                ("to_wtower2", 41, 1)
            } else {
                if w.m.game_rules.difficulty == 0 {
                    continue;
                }
                if self.can(w, "to_stower2_1") && w.sim.gameplay.game_rng.range(0, 2) == 0 {
                    ("to_stower2_1", 42, 4)
                } else {
                    ("to_stower", 39, 2)
                }
            };
            if seen & bit != 0 {
                continue;
            }
            seen |= bit;
            let Some(kind) = w.c.ai.kind(name).filter(|_| self.can(w, name)) else {
                continue;
            };
            let cost = rules::cost(w, self, kind);
            if rules::affordable(w, self, cost, bucket::DEFENCE, 1, false) {
                rules::pay(w, self, cost);
                super::produce(
                    w.sim,
                    w.m,
                    w.c,
                    t,
                    JobKind::Upgrade(name.into(), Some(to)),
                    cost,
                );
                return true;
            }
            rules::book(self, bucket::DEFENCE, cost, 1);
            if seen == 7 {
                break;
            }
        }
        false
    }
    /// 0x494c90 for a type named.
    fn can(&self, w: &World, name: &str) -> bool {
        w.c.ai
            .kind(name)
            .is_some_and(|kind| rules::available(w, self, kind))
    }

    /// 0x410b60: the places in the order of the records, each evaluated
    /// again (0x40fdd0, without sorting them again); the first that gets a
    /// defence built, the others book what theirs would cost.
    fn defences(&mut self, w: &mut World) -> bool {
        let k = self.kingdom;
        let mut first = true;
        for at in 0..self.records.len() {
            if let Some(mgr) = w.sim.gameplay.ai.manager.as_ref() {
                records::evaluate(self, k, at, mgr, w.m, mgr.side);
            }
            if self.defence(w, at, first) {
                // The place keeps its record: ReTzar's places change with the
                // step's events, never during a phase (0x410bc8).
                first = false;
            }
        }
        false
    }
    /// 0x4104f0: what a place asks for (record +0x92): with bit 0 a wooden
    /// tower while it has fewer, its own and those going up, than its
    /// limit; with bit 1 and a barracks in the kingdom, a barracks while it
    /// has fewer than 2, then on hard an outpost and a workshop. The site
    /// is within half the place's width of its centre.
    fn defence(&mut self, w: &mut World, at: usize, first: bool) -> bool {
        let r = self.records[at];
        if r.flags & 3 == 0 {
            return false;
        }
        let k = self.kingdom;
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
        let count = |cat: usize| {
            let of = |f: &Vec<places::Forces>| f.get(k as usize).map_or(0, |x| x.0[cat].count);
            of(&place.incoming).wrapping_add(of(&place.present))
        };
        let towers = count(category::TOWERS);
        let barracks = count(category::BARRACKS);
        let rect = place.rect;
        let half = |v: i32| (v - (v >> 31)) >> 1;
        let center = [half(rect[0] + rect[2]), half(rect[1] + rect[3])];
        let reach = (center[0] - rect[0]) as u32;
        let mut limit = 1;
        if place.kind == places::kind::HOME && place.param == k as u32 {
            limit = HOME_TOWERS[0];
            if w.m.game_rules.difficulty != 0 && towers >= HOME_TOWERS[0] {
                limit = HOME_TOWERS[1];
            }
        }
        let mut built = false;
        if r.flags & 1 != 0 && towers < limit {
            built = self.raise(w, class::TOWER_WOOD as u16, center, reach, first);
        }
        if r.flags & 2 == 0 || built {
            return built;
        }
        if rules::record(w.sim, w.m, &w.c.ai, k, class::BARRACK as u16).0 == 0 {
            return false;
        }
        if barracks < 2 {
            return self.raise(w, class::BARRACK as u16, center, reach, first);
        }
        if w.m.game_rules.difficulty != 2 {
            return false;
        }
        for (kind, list) in [
            (class::OUTPOST, self.lists.outposts.clone()),
            (class::WORKSHOP, self.lists.workshops.clone()),
        ] {
            if self.standing(w, kind, &list, rect) == 0 {
                return self.raise(w, kind as u16, center, reach, first);
            }
        }
        false
    }
    /// A building put up if the first place asking and the defences can pay
    /// it (0x40a990 from 1 cell to `reach`), else its cost booked.
    fn raise(
        &mut self,
        w: &mut World,
        kind: u16,
        center: [i32; 2],
        reach: u32,
        first: bool,
    ) -> bool {
        let cost = rules::cost(w, self, kind);
        if first && rules::affordable(w, self, cost, bucket::DEFENCE, 1, false) {
            return self.place(w, kind, center, 1, reach, None, false).is_some();
        }
        rules::book(self, bucket::DEFENCE, cost, 1);
        false
    }
    /// 0x4103d0: the objects of a list of the player standing in the
    /// place's sectors, and the builders walking to a site of the type
    /// still to be laid, wherever it is.
    fn standing(&self, w: &World, kind: usize, list: &[usize], rect: [i32; 4]) -> u32 {
        let sectors = rect.map(|v| v.div_euclid(8));
        let here = list
            .iter()
            .filter(|&&i| {
                w.sim.gameplay.actors[i].sector.is_some_and(|s| {
                    let s = [s[0] as i32, s[1] as i32];
                    s[0] >= sectors[0]
                        && s[0] <= sectors[2]
                        && s[1] >= sectors[1]
                        && s[1] <= sectors[3]
                })
            })
            .count() as u32;
        let walking = self.lists.groups[think::group::BUILDERS as usize]
            .iter()
            .filter(|i| {
                self.lists
                    .peons
                    .get(i)
                    .and_then(|p| p.site)
                    .and_then(|id| w.sim.gameplay.ai.virtuals.get(&id))
                    .is_some_and(|v| v.class as usize == kind)
            })
            .count() as u32;
        here + walking
    }
}

#[cfg(test)]
#[path = "defence_tests.rs"]
mod tests;
