//! A computer player's ships, fishing boats and harbors (0x4168d0, in the
//! Special phase), the fish patch it chooses (0x4164c0), its idle fishing
//! boats (0x415730) and the site of a harbor on the coast of its island
//! (0x40a710).
use super::rules::{self, World, bucket};
use super::*;

/// The fishing boats a player keeps (0x416aee).
const FISHERS: usize = 5;
/// A patch this near its harbor is fished (0x41684f); a water without
/// harbors asks for one for a patch this near the home (0x416863).
const NEAR_HARBOR: u32 = 20;
const NEAR_HOME: u32 = 64;
/// The ships, the first available taken (0x4168d9).
const SHIPS: [u16; 3] = [24, 25, 23];
const END: [u16; 2] = [0xffff; 2];

/// A finished building with nothing to do (vt+0x2c0, program +0x82 none).
fn ready(w: &World, i: usize) -> bool {
    let o = &w.m.objects[i];
    alive(o) && o.flags & FLAG_CONSTRUCTION == 0 && rules::idle_building(w.sim, i)
}
/// The region is a region of the map, not none nor wiped.
fn valid(r: u8) -> bool {
    r != islands::NONE && r != islands::WIPED
}
/// 0x401a70: the empty cells between two points.
fn apart(a: [i32; 2], b: [i32; 2]) -> u32 {
    records::gap(a, [b[0], b[1], b[0], b[1]])
}

impl Player {
    /// 0x4168d0: on each water that wants more ships than it has, the
    /// first finished idle harbor of that water makes one (the first ship
    /// available of hulk, xebec and galleon; a hulk it cannot pay tries the
    /// xebec), or the water wants a harbor; a ship it cannot pay is booked
    /// and the waters wait. Then up to five fishing boats, from the harbor
    /// the choice of a patch gives. Then, when no harbor goes up, a harbor
    /// for the first water that wants more, from the ships' part if ships
    /// are wanted there. Ships and boats are made with the program Create
    /// alone: unlike the goals (0x4099d0) nothing pays them.
    pub(super) fn navy(&mut self, w: &mut World) -> bool {
        let waters = w.sim.gameplay.islands.next;
        if let Some(mut ship) = SHIPS.into_iter().find(|&t| rules::available(w, self, t)) {
            for water in 1..waters {
                let wanted = self.wanted_ships.get(&water).copied().unwrap_or(0);
                if wanted == 0 || self.ships.get(&water).map_or(0, Vec::len) >= wanted as usize {
                    continue;
                }
                let cost = rules::cost(w, self, ship);
                if !rules::affordable(w, self, cost, bucket::NAVY, 1, false) {
                    let xebec = rules::cost(w, self, 25);
                    if ship == 24 && rules::affordable(w, self, xebec, bucket::NAVY, 1, false) {
                        ship = 25;
                    } else {
                        rules::book(self, bucket::NAVY, cost, 1);
                        break;
                    }
                }
                let harbors = self.lists.harbors.get(&water).cloned().unwrap_or_default();
                if harbors.is_empty() {
                    let want = self.wanted_harbors.entry(water).or_default();
                    if *want == 0 {
                        *want = 1;
                    }
                } else if let Some(h) = harbors.into_iter().find(|&h| ready(w, h)) {
                    super::produce(w.sim, w.m, w.c, h, JobKind::Train(ship as usize), [0; 4]);
                }
            }
        }
        let setup = &w.m.game_rules;
        let unlimited_hill = setup.game_type == 1 && setup.resource_level == 2;
        let boat = class::FISHING_BOAT as u16;
        if !unlimited_hill && self.lists.fishing.len() < FISHERS && rules::available(w, self, boat)
        {
            let (patch, harbor) = self.fish_patch(w, false, 0);
            if patch.is_some()
                && let Some(h) = harbor.filter(|&h| ready(w, h))
            {
                let cost = rules::cost(w, self, boat);
                if rules::affordable(w, self, cost, bucket::WORKERS, 1, false) {
                    super::produce(w.sim, w.m, w.c, h, JobKind::Train(boat as usize), [0; 4]);
                } else {
                    rules::book(self, bucket::WORKERS, cost, 1);
                }
            }
        }
        let harbor = class::HARBOR as u16;
        if !rules::available(w, self, harbor)
            || rules::record(w.sim, w.m, &w.c.ai, self.kingdom, harbor).1 != 0
        {
            return false;
        }
        let mut failed = false;
        for water in 1..waters {
            let wanted = self.wanted_harbors.get(&water).copied().unwrap_or(0);
            let have = self.lists.harbors.get(&water).map_or(0, Vec::len);
            if wanted == 0 || have >= wanted as usize {
                continue;
            }
            let cost = rules::cost(w, self, harbor);
            let part = if self.wanted_ships.get(&water).copied().unwrap_or(0) != 0 {
                bucket::NAVY
            } else {
                bucket::WORKERS
            };
            if !rules::affordable(w, self, cost, part, 1, false) {
                rules::book(self, part, cost, 1);
                return failed;
            }
            // 0x5af414: the water the harbor is for, read by its site.
            w.sim.gameplay.ai.harbor_water = water;
            if rules::execute(w, self, self.kingdom, harbor, 1) {
                return true;
            }
            failed = true;
        }
        failed
    }

    /// 0x4164c0: the fish patch for a boat, with a harbor: of the patches
    /// (the game's list, the newest first) that no boat of the player
    /// fishes, on `filter`'s water if given, the one nearest a finished
    /// harbor of its water if within 20 cells; else, unless `quiet`, the
    /// one nearest the home within 64 on a water that wants a harbor, else
    /// on another water, which then wants one with the patch as its mark.
    /// The harbor is the one nearest the last patch looked at whose water
    /// has harbors, whichever patch is chosen.
    pub(super) fn fish_patch(
        &mut self,
        w: &World,
        quiet: bool,
        filter: u8,
    ) -> (Option<usize>, Option<usize>) {
        let fished: Vec<usize> = self
            .lists
            .fishing
            .iter()
            .filter(|&&b| program(w.sim, w.m, b).kind == program::FISHING)
            .filter_map(|&b| match w.sim.gameplay.actors[b].intent {
                Some(Intent::Harvest(j)) => Some(j),
                _ => None,
            })
            .collect();
        let home = self.home.unwrap_or_default();
        let own = |i: usize| movement::footprint_rect(w.lib, &w.m.objects[i]);
        let mut harbor = None;
        let mut near: Option<(usize, u32)> = None;
        let mut asked: Option<(usize, u32)> = None;
        let mut other: Option<(usize, u32, u8)> = None;
        for i in (0..w.m.objects.len()).rev() {
            let o = &w.m.objects[i];
            if o.class != class::FISHPATCH || !alive(o) || fished.contains(&i) {
                continue;
            }
            let water = super::think::water_of(w, i);
            if !valid(water) || (filter != 0 && water != filter) {
                continue;
            }
            if self
                .lists
                .harbors
                .get(&water)
                .is_some_and(|l| !l.is_empty())
            {
                harbor = None;
                let mut best = 0u32;
                for &h in &self.lists.harbors[&water] {
                    let x = &w.m.objects[h];
                    if !alive(x) || x.flags & FLAG_CONSTRUCTION != 0 {
                        continue;
                    }
                    let d = combat::gap(own(h), own(i));
                    if harbor.is_none() || d < best {
                        (best, harbor) = (d, Some(h));
                    }
                }
                if harbor.is_some() {
                    if near.is_none_or(|(_, d)| best < d) {
                        near = Some((i, best));
                    }
                    continue;
                }
            }
            if quiet {
                continue;
            }
            let d = records::gap(home, own(i));
            if self.wanted_harbors.get(&water).copied().unwrap_or(0) != 0 {
                if asked.is_none_or(|(_, b)| d < b) {
                    asked = Some((i, d));
                }
            } else if other.is_none_or(|(_, b, _)| d < b) {
                other = Some((i, d, water));
            }
        }
        if let Some((i, d)) = near
            && d <= NEAR_HARBOR
        {
            return (Some(i), harbor);
        }
        if let Some((i, d)) = asked
            && d < NEAR_HOME
        {
            return (Some(i), harbor);
        }
        match other {
            Some((i, d, water)) if d < NEAR_HOME => {
                let want = self.wanted_harbors.entry(water).or_default();
                if *want == 0 {
                    let o = &w.m.objects[i];
                    self.harbor_targets.insert(water, [o.x, o.y]);
                    *want = 1;
                }
                (Some(i), harbor)
            }
            _ => (None, harbor),
        }
    }

    /// 0x415730: an idle fishing boat fishes the patch of its water that
    /// the choice gives with a harbor.
    pub(super) fn idle_fishing(&mut self, w: &mut World, boat: usize) -> bool {
        let water = super::think::water_of(w, boat);
        if !valid(water) {
            return false;
        }
        let (Some(patch), Some(_)) = self.fish_patch(w, true, water) else {
            return false;
        };
        set_order(w.sim, boat, Intent::Harvest(patch), false).is_ok()
    }

    /// 0x40a710: the site of a harbor on the coast of the home island,
    /// walked from each of its points (0x46a9b0): a cell counts when the
    /// first water round it (0x46aff0) is the one the harbor is for, if
    /// any; the score is its gap to `reference` and to the patch that asked
    /// for the harbor; the footprint is tried with each of its corners on
    /// the cell, then the site must be safe. The lowest score wins, the
    /// first on ties.
    pub(super) fn harbor_spot(
        &mut self,
        w: &mut World,
        kind: u16,
        reference: [i32; 2],
    ) -> Option<[i32; 2]> {
        let water = std::mem::take(&mut w.sim.gameplay.ai.harbor_water);
        let target = (water != 0)
            .then(|| self.harbor_targets.get(&water).copied())
            .flatten();
        let island = self.island;
        if !valid(island) {
            return None;
        }
        let first = w.sim.gameplay.islands.regions.get(island as usize)?.coast? as usize;
        let mut probe = super::tree::probe(kind as usize, self.civilization);
        probe.owner = self.kingdom;
        let [wd, ht] = footprint(w.lib, &probe).map(|n| n as i32);
        let fits = |w: &World, probe: &mut MapObject, p: [i32; 2]| {
            probe.x = p[0] as u16;
            probe.y = p[1] as u16;
            p[0] >= 0 && p[1] >= 0 && site_test(w.sim, w.m, w.g, w.lib, probe)
        };
        let mut best: Option<([i32; 2], u32)> = None;
        let mut at_point = first;
        loop {
            let start = match w.sim.gameplay.islands.coasts.get(at_point) {
                Some(&s) if s != END => [s[0] as i32, s[1] as i32],
                _ => break,
            };
            let mut at = start;
            loop {
                let isl = &w.sim.gameplay.islands;
                let round = isl.nearest(
                    w.g,
                    [at[0] - 1, at[1] - 1, at[0] + 1, at[1] + 1],
                    islands::WATER,
                );
                if valid(round) && (water == 0 || round == water) {
                    let mut score = apart(at, reference);
                    if let Some(t) = target {
                        score = score.wrapping_add(apart(at, [t[0] as i32, t[1] as i32]));
                    }
                    if best.is_none_or(|(_, b)| score < b) {
                        let mut p = at;
                        let mut ok = fits(w, &mut probe, p);
                        for d in [[wd - 1, 0], [0, ht - 1], [1 - wd, 0]] {
                            if ok {
                                break;
                            }
                            p = [p[0] + d[0], p[1] + d[1]];
                            ok = fits(w, &mut probe, p);
                        }
                        if ok && build::safe(w, self, p[0], p[1], [wd, ht]) {
                            best = Some((p, score));
                        }
                    }
                }
                let isl = &w.sim.gameplay.islands;
                if !isl.coast_step(w.g, island, &mut at, false) || at == start {
                    break;
                }
            }
            at_point += 1;
        }
        best.map(|(p, _)| p)
    }
}

#[cfg(test)]
#[path = "navy_tests.rs"]
mod tests;
