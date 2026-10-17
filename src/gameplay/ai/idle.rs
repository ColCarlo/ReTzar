//! What a computer player's unit does at its idle tick (0x4ae4a5,
//! 0x4e4fc9 → 0x4140b0): inside a castle or an outpost it leaves once
//! healed; then by class (peasant 0x4153a0, fishing boat 0x415730), the
//! fighters' own rules beyond the easy level, and the refuge of the
//! wounded (0x413f50).
use super::build;
use super::records::gap;
use super::rules::World;
use super::think::{self, group};
use super::*;

/// A fighter picks up an item this near (0x414219), a wounded unit takes
/// refuge this near (0x413fc1).
const FIGHTER_ITEMS: u32 = 16;
const REFUGE_REACH: u32 = 24;
/// An abandoned site this near is taken up (0x4153d5).
const SITE_REACH: u32 = 21;
/// Resource items are picked up this near (0x415614), damaged objects
/// repaired this near (0x415698).
const ITEM_REACH: u32 = 16;
const REPAIR_REACH: i32 = 15;

/// 0x4140b0: unit `i` of a computer player idle for `idle` ms (+0xe2).
pub(crate) fn unit(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    idle: u32,
) {
    let k = m.objects[i].owner;
    let Some(mut p) = sim.gameplay.ai.players.get_mut(&k).map(std::mem::take) else {
        return;
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
    p.idle_unit(&mut w, i, idle);
    if let Some(slot) = w.sim.gameplay.ai.players.get_mut(&k) {
        *slot = p;
    }
}

fn rect_of(w: &World, i: usize) -> [i32; 4] {
    movement::footprint_rect(w.lib, &w.m.objects[i])
}
fn cell(o: &MapObject) -> [i32; 2] {
    [o.x as i32, o.y as i32]
}
/// 0x454d70: from a point to an object's centre.
pub(super) fn to_centre(lib: &ObjectLibrary, p: [i32; 2], o: &MapObject) -> i32 {
    let [w, h] = footprint(lib, o).map(|n| n as i32);
    let c = [o.x as i32 + w / 2, o.y as i32 + h / 2];
    (p[0] - c[0]).abs().max((p[1] - c[1]).abs())
}
/// The store of a load by its native kind (+0x120, 0x40fafc): milk and
/// picked food go only to a castle.
fn store_of_load(kind: u8) -> Option<usize> {
    match kind {
        0 | 1 => Some(class::STORAGE),
        2 => Some(class::FARM),
        3 => Some(class::LUMBER),
        _ => None,
    }
}

impl Player {
    /// 0x4153a0: an idle peasant, in turn: helps the nearest abandoned site
    /// within 21 with at most one builder (three once idle two periods);
    /// takes its load to the nearest castle or store of it it reaches;
    /// picks up the nearest resource item within 16; idle two periods,
    /// repairs the nearest damaged object of an ally within 15.
    fn idle_peasant(&mut self, w: &mut World, i: usize, idle: u32) -> bool {
        let long = idle as u64 >= 2 * think::period(w.m);
        let own = rect_of(w, i);
        let mut best = None;
        let mut limit = SITE_REACH;
        for s in self.lists.sites.clone() {
            let d = build::rect_gap(rect_of(w, s), own);
            if d >= limit {
                continue;
            }
            let builders = self.lists.groups[group::BUILDERS as usize]
                .iter()
                .filter(|b| self.lists.peons.get(b).and_then(|x| x.target) == Some(s))
                .count();
            if builders <= if long { 3 } else { 1 } {
                best = Some(s);
                limit = d;
            }
        }
        if let Some(s) = best {
            let at = cell(&w.m.objects[s]);
            self.command(w, i, Intent::Build(s), at, Some(s));
            return true;
        }
        let from = cell(&w.m.objects[i]);
        if let Some(load) = carry_type(&w.sim.gameplay.actors[i]) {
            let kind = store_of_load(load);
            let mut best: Option<(u32, usize)> = None;
            for j in think::kingdom_list(w.sim, w.m, self.kingdom, false) {
                let c = w.m.objects[j].class;
                if c != class::CASTLE && Some(c) != kind {
                    continue;
                }
                let r = rect_of(w, j);
                if !build::reach(w, self, from, [r[0] - 1, r[1] - 1, r[2] + 1, r[3] + 1]) {
                    continue;
                }
                let d = build::rect_gap(own, r);
                if best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, j));
                }
            }
            if let Some((_, j)) = best {
                let at = cell(&w.m.objects[j]);
                self.command(w, i, Intent::Deliver(j), at, Some(j));
                return true;
            }
        }
        // 0x413e00 over the items on the ground, with 0x44ec10 for
        // resource items.
        let mut best: Option<(u32, usize)> = None;
        for (j, o) in w.m.objects.iter().enumerate() {
            if o.class != class::RESITEM || !alive(o) || w.sim.objects[j].holder.is_some() {
                continue;
            }
            let d = gap(from, movement::footprint_rect(w.lib, o));
            if d <= ITEM_REACH && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, j));
            }
        }
        if let Some((_, j)) = best {
            let at = cell(&w.m.objects[j]);
            self.command(w, i, Intent::Harvest(j), at, Some(j));
            return true;
        }
        if !long {
            return false;
        }
        // 0x44e690 round the peasant with 0x44f2f0: the nearest to its cell
        // by the centres (0x454d70), the first on ties.
        let area = [
            from[0] - REPAIR_REACH,
            from[1] - REPAIR_REACH,
            from[0] + REPAIR_REACH,
            from[1] + REPAIR_REACH,
        ];
        let Some(j) = crate::gameplay::mend_near(w.sim, w.m, w.g, w.lib, self.kingdom, from, area)
        else {
            return false;
        };
        let at = cell(&w.m.objects[j]);
        self.command(w, i, Intent::Repair(j), at, Some(j));
        true
    }
}

/// The unit is wounded (vt+0x230): its health below the most.
fn wounded(o: &MapObject) -> bool {
    o.health < o.max_health
}
/// The classes a fighter picks up (0x413ee0): `item_plusplus`, the magic
/// item and the health item.
pub(super) fn fighter_item(class: usize) -> bool {
    matches!(
        class,
        class::ITEM_PLUSPLUS | class::ITEM_MAGIC | class::ITEM_HEALTH
    )
}

impl Player {
    /// 0x4140b0: a unit of the player at its idle tick. Inside a castle or
    /// an outpost it leaves when healed, and does nothing else inside a
    /// holder; then by class: the peasant and the fishing boat have their
    /// own, the priest its spells (`idle_priest`), going on as a fighter
    /// when it casts none, the mage none (0x4b01c0 is empty); a fighter,
    /// beyond the easy level, goes to a magic gateway of its sector of
    /// another or a hostile kingdom, else picks up the nearest item within
    /// 16, else advances to the next sector with the most hostile strength
    /// in the player's places; a wounded unit takes refuge.
    pub(super) fn idle_unit(&mut self, w: &mut World, i: usize, idle: u32) -> bool {
        if let Some(h) = w.sim.objects[i].holder {
            let class = w.m.objects[h].class;
            if matches!(class, class::CASTLE | class::OUTPOST) && !wounded(&w.m.objects[i]) {
                return step_off(w.sim, w.m, w.lib, h, i);
            }
            return false;
        }
        let class = w.m.objects[i].class;
        match class {
            class::PEON => return self.idle_peasant(w, i, idle),
            class::FISHING_BOAT => return self.idle_fishing(w, i),
            class::PRIEST if self.idle_priest(w, i) => return true,
            _ => {}
        }
        if military(w.sim, w.m, i) && w.m.game_rules.difficulty != 0 {
            if self.to_gateway(w, i) || self.to_item(w, i) {
                return true;
            }
            if let Some(Some(s)) = self.hostile_sector(w, i) {
                // A Move with advance to the sector's centre (0x4143dd).
                let p = [s[0] * 8 + 4, s[1] * 8 + 4];
                if set_order(w.sim, i, Intent::Move(p), false).is_ok() {
                    w.sim.gameplay.actors[i].move_mode = 1;
                    return true;
                }
            }
        }
        if wounded(&w.m.objects[i])
            && let Some(r) = self.refuge(w, i)
        {
            return set_order(w.sim, i, Intent::Board(r), false).is_ok();
        }
        false
    }
    /// 0x4141d2: a magic gateway place of the unit's sector whose gateway is
    /// of a kingdom beyond the 8 or not allied: the unit walks to it.
    fn to_gateway(&mut self, w: &mut World, i: usize) -> bool {
        let Some(s) = w.sim.gameplay.actors[i].sector else {
            return false;
        };
        let owner = w.m.objects[i].owner;
        let slots = match w.sim.gameplay.ai.manager.as_ref() {
            Some(mgr) => mgr.sector_places([s[0] as i32, s[1] as i32]).to_vec(),
            None => return false,
        };
        for slot in slots {
            let anchor = w
                .sim
                .gameplay
                .ai
                .manager
                .as_ref()
                .and_then(|mgr| mgr.place(slot))
                .filter(|p| p.kind == places::kind::GATEWAY)
                .and_then(|p| p.anchor);
            let Some(gw) = anchor else {
                continue;
            };
            let g = &w.m.objects[gw];
            if g.owner > 8 || relation(w.m, g.owner, owner) & 0x40 == 0 {
                let at = [g.x, g.y];
                self.command(
                    w,
                    i,
                    Intent::Move(at),
                    [at[0] as i32, at[1] as i32],
                    Some(gw),
                );
                return true;
            }
        }
        false
    }
    /// 0x413e00 with 0x413ee0: the nearest item on the ground within 16 of
    /// the unit's cell, the game's newest first on ties; the unit picks it
    /// up.
    fn to_item(&mut self, w: &mut World, i: usize) -> bool {
        let o = &w.m.objects[i];
        let at = [o.x as i32, o.y as i32];
        let mut best: Option<(u32, usize)> = None;
        for j in (0..w.m.objects.len()).rev() {
            let x = &w.m.objects[j];
            if !fighter_item(x.class)
                || !alive(x)
                || x.flags & FLAG_INACTIVE != 0
                || w.sim.objects[j].holder.is_some()
            {
                continue;
            }
            let d = gap(at, rect_of(w, j));
            if d <= FIGHTER_ITEMS && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, j));
            }
        }
        let Some((_, j)) = best else {
            return false;
        };
        let x = &w.m.objects[j];
        let p = [x.x as i32, x.y as i32];
        self.command(w, i, Intent::Pick(j), p, Some(j));
        true
    }
    /// 0x41429d: of the 3×3 sectors round the unit's, the one whose places
    /// have the most hostile strength in the player's records, the first
    /// on ties (columns from the west); none when the unit's own has some
    /// (`Some(None)`, the refuge comes next) or none has any.
    fn hostile_sector(&self, w: &World, i: usize) -> Option<Option<[u16; 2]>> {
        let own = w.sim.gameplay.actors[i].sector?;
        let mgr = w.sim.gameplay.ai.manager.as_ref()?;
        let n = mgr.sectors_side as i32;
        let mut best: Option<([u16; 2], u32)> = None;
        for sx in own[0] as i32 - 1..=own[0] as i32 + 1 {
            if sx < 0 || sx >= n {
                continue;
            }
            for sy in own[1] as i32 - 1..=own[1] as i32 + 1 {
                if sy < 0 || sy >= n {
                    continue;
                }
                let mut sum = 0u32;
                for &slot in mgr.sector_places([sx, sy]) {
                    let Some(p) = mgr.place(slot) else {
                        continue;
                    };
                    let Some(r) = self
                        .records
                        .iter()
                        .find(|r| r.place == slot && r.serial == p.serial)
                    else {
                        continue;
                    };
                    let s = r.hostile.strength(0xff);
                    if s != 0 {
                        if [sx as u16, sy as u16] == own {
                            return Some(None);
                        }
                        sum = sum.wrapping_add(s);
                    }
                }
                if sum > best.map_or(0, |(_, b)| b) {
                    best = Some(([sx as u16, sy as u16], sum));
                }
            }
        }
        best.map(|(s, _)| Some(s))
    }
    fn refuge(&self, w: &World, i: usize) -> Option<usize> {
        refuge(w.sim, w.m, w.lib, w.c, self, i)
    }
}

/// 0x413f50: the nearest castle of the player, and outpost, within 24 cells
/// with room that takes the unit; the outpost when it is no farther than
/// the castle.
pub(crate) fn refuge(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    p: &Player,
    i: usize,
) -> Option<usize> {
    let own = |j: usize| movement::footprint_rect(lib, &m.objects[j]);
    let nearest = |list: &[usize]| {
        let mut best: Option<(u32, usize)> = None;
        for &h in list {
            if sim.objects[h].contents.len() >= holder_capacity(sim, m, lib, h)
                || !boardable(sim, m, lib, c, i, h)
            {
                continue;
            }
            let d = combat::gap(own(h), own(i));
            if d <= REFUGE_REACH && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, h));
            }
        }
        best
    };
    match (nearest(&p.lists.castles), nearest(&p.lists.outposts)) {
        (None, outpost) => outpost.map(|(_, h)| h),
        (Some((c, _)), Some((o, outpost))) if c >= o => Some(outpost),
        (Some((_, castle)), _) => Some(castle),
    }
}

#[cfg(test)]
#[path = "idle_tests.rs"]
mod tests;
