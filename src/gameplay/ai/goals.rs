//! The goals of a script carried out by their own routine in 0x409550: a
//! creature a mage summons (0x409ab0), the crusade (0x409ed0, 0x409d30) and
//! a peasant become a jihad warrior (0x409500, 0x409290).
use super::harvest::{NOWHERE, ring_first, ring_next};
use super::rules::{self, World, bucket};
use super::*;

/// The jihad warrior (0x21) and the crusade's anchor (0x74), which 0x409550
/// sends to their routines.
pub const JIHAD_WARRIOR: u16 = 33;
pub const CRUSADE: u16 = class::CRUSADEANCHOR as u16;
/// The crusader (0xb).
const CRUSADER: u16 = 11;
/// A summoned creature's spot is looked for this many rings round its mage
/// (0x409ca2), the crusade's anchor this many round a cathedral (0x409e8f).
const SUMMON_RINGS: i32 = 6;
const ANCHOR_RINGS: u32 = 4;

/// The creature of a summoning goal: its spell, and whether it walks
/// (0x409b3f to 0x409b6c, the golem's flag for 0x5073f0).
fn summoning(kind: u16) -> Option<(&'static str, bool)> {
    match kind {
        15 => Some(("spell_spawnbat", false)),
        16 => Some(("spell_spawnstonegolem", true)),
        22 => Some(("spell_spawncarpet", false)),
        30 => Some(("spell_spawnjin", false)),
        32 => Some(("spell_spawndragon", false)),
        _ => None,
    }
}

/// 0x409ab0: a creature a mage makes. With a mage and the script's part
/// able to pay, the price is taken from what the player may spend at once;
/// the spell's research must be done. The caster is an idle mage of a
/// sector whose places, up to the first with hostile forces present or
/// coming, include the highest priority; it casts at a free cell within six
/// rings of it (for the air, or the land for the golem).
pub(super) fn summon(w: &mut World, p: &mut Player, kind: u16) -> bool {
    if p.lists.mages.is_empty() {
        return false;
    }
    let cost = rules::cost(w, p, kind);
    if !rules::affordable(w, p, cost, bucket::SCRIPT, 1, false) {
        return false;
    }
    for (a, c) in p.available.iter_mut().zip(cost) {
        *a = a.wrapping_sub(c);
    }
    let Some((key, walks)) = summoning(kind) else {
        return false;
    };
    // 0x60a850: the research of the spell, unless «notech» (0x409b74).
    if let Some(dep) = w.c.order_research(key)
        && !w.m.cheats.no_tech
        && !researches(w.m, p.kingdom)
            .iter()
            .any(|r| r.eq_ignore_ascii_case(dep.trim()))
    {
        return false;
    }
    let Some(mgr) = w.sim.gameplay.ai.manager.as_ref() else {
        return false;
    };
    let mut best: Option<(u32, usize)> = None;
    for &i in &p.lists.mages {
        if program(w.sim, w.m, i).kind != program::IDLE {
            continue;
        }
        let Some(s) = w.sim.gameplay.actors[i].sector else {
            continue;
        };
        for &slot in mgr.sector_places([s[0] as i32, s[1] as i32]) {
            let Some(place) = mgr.place(slot) else {
                continue;
            };
            let Some(r) = p
                .records
                .iter()
                .find(|r| r.place == slot && r.serial == place.serial)
            else {
                continue;
            };
            if r.hostile.count(0xff) != 0 || r.hostile_incoming.count(0xff) != 0 {
                break;
            }
            if best.is_none_or(|(b, _)| r.priority > b) {
                best = Some((r.priority, i));
            }
        }
    }
    let Some((_, mage)) = best else {
        return false;
    };
    let rect = movement::footprint_rect(w.lib, &w.m.objects[mage]);
    let Some(cell) = exit_cell(w.sim, rect, None, usize::from(walks), SUMMON_RINGS) else {
        return false;
    };
    // 0x42ebc0 at priority 4000, given straight to the mage (vt+0x160).
    let intent = Intent::Cast {
        key: key.into(),
        cell,
        target: None,
    };
    set_order(w.sim, mage, intent, false).is_ok()
}

/// 0x409ed0: a European player with a castle, a cathedral and no crusader,
/// the script's part able to pay and the cathedral's crusaders all gathered
/// (0x439640), lays the anchor (0x409d30) and pays it from what it may
/// spend too.
pub(super) fn crusade(w: &mut World, p: &mut Player) -> bool {
    let k = p.kingdom;
    if p.civilization != 1 {
        return false;
    }
    let count = |w: &World, kind: usize| rules::record(w.sim, w.m, &w.c.ai, k, kind as u16).0;
    if count(w, class::CASTLE) == 0
        || count(w, CRUSADER as usize) != 0
        || count(w, class::CATHEDRAL) == 0
    {
        return false;
    }
    let cost = rules::cost(w, p, CRUSADE);
    if !rules::affordable(w, p, cost, bucket::SCRIPT, 1, false) {
        return false;
    }
    let t = w.sim.gameplay.treasury.get(&k).cloned().unwrap_or_default();
    let Some(ready) = t.crusaders else {
        return false;
    };
    if ready < crusade_ceiling(&t) {
        return false;
    }
    let Some(at) = anchor_spot(w, k) else {
        return false;
    };
    let mut by = super::tree::probe(class::CATHEDRAL, p.civilization);
    by.owner = k;
    if begin_crusade(w.sim, w.m, w.g, w.lib, w.c, &by, ready, at).is_err() {
        return false;
    }
    w.cells = None;
    for (a, c) in p.available.iter_mut().zip(cost) {
        *a = a.wrapping_sub(c);
    }
    true
}
/// 0x409d30: of the kingdom's cathedrals, the newest first, the first cell
/// of the rings round each (from its rect grown by one, four rings) where a
/// land unit may stand on the cathedral's island.
fn anchor_spot(w: &mut World, k: u8) -> Option<[u16; 2]> {
    let side = w.g.side as i32;
    for b in super::think::kingdom_list(w.sim, w.m, k, false) {
        let o = &w.m.objects[b];
        if o.class != class::CATHEDRAL {
            continue;
        }
        let at = [o.x as i32, o.y as i32];
        let r = movement::footprint_rect(w.lib, o);
        let rect = [r[0] - 1, r[1] - 1, r[2] + 1, r[3] + 1];
        let isl = &w.sim.gameplay.islands;
        let here = isl.region(at);
        let cell = at[1] as usize * w.g.side + at[0] as usize;
        let region = if !matches!(here, islands::NONE | islands::WIPED)
            && islands::cell_kind(w.g, cell) == islands::LAND
        {
            here
        } else {
            isl.nearest(w.g, rect, islands::LAND)
        };
        let (mut lo, mut hi) = ([rect[0], rect[1]], [rect[2], rect[3]]);
        for _ in 0..ANCHOR_RINGS {
            let mut q = ring_first(lo, hi, side);
            while q != NOWHERE {
                if super::build::cells(w).free(1, q) {
                    let there = w.sim.gameplay.islands.region(q);
                    if w.sim.gameplay.islands.same_island(region, there) {
                        return Some([q[0] as u16, q[1] as u16]);
                    }
                }
                q = ring_next(lo, hi, q, side);
            }
            lo = [lo[0] - 1, lo[1] - 1];
            hi = [hi[0] + 1, hi[1] + 1];
        }
    }
    None
}

/// 0x409500: an Arabian player's peasant becomes a jihad warrior: of all
/// its peasants the one with the highest score (0x409290, the first on
/// ties), given the program at once (0x430860, op 0x5d); a hypnotised one
/// stays as it is.
pub(super) fn jihad(w: &mut World, p: &mut Player) -> bool {
    if p.civilization != 2 {
        return false;
    }
    let mut best: Option<(u32, usize)> = None;
    for &i in &p.lists.peasants {
        let s = score(w, p, i);
        if s != 0 && best.is_none_or(|(b, _)| s > b) {
            best = Some((s, i));
        }
    }
    let Some((_, i)) = best else {
        return false;
    };
    if !has_effect(w.sim, i, effect::HYPNOSIS) {
        let was = super::think::seen(w.sim, w.m, i);
        convert(w.sim, w.m, w.lib, i, JIHAD_WARRIOR as usize);
        if let Some(was) = was {
            p.replaced(w, i, was);
        }
    }
    true
}
/// 0x409290: a peasant the player may take scores one more than how far its
/// own fighters fall short of each minimum over the places of its sector,
/// times 3/2 idle, 2 attacking, 2/3 chopping, digging, mining or tending
/// cows; nothing while it builds.
fn score(w: &World, p: &Player, i: usize) -> u32 {
    if !super::build::free(w, p, i) {
        return 0;
    }
    let mut short = 0u32;
    if let (Some(mgr), Some(s)) = (
        w.sim.gameplay.ai.manager.as_ref(),
        w.sim.gameplay.actors[i].sector,
    ) {
        for &slot in mgr.sector_places([s[0] as i32, s[1] as i32]) {
            let Some(place) = mgr.place(slot) else {
                continue;
            };
            if let Some(r) = p
                .records
                .iter()
                .find(|r| r.place == slot && r.serial == place.serial)
            {
                let have = r.friendly.strength(places::mask::MILITARY);
                if have < r.minimum {
                    short = short.wrapping_add(r.minimum - have);
                }
            }
        }
    }
    let s = match program(w.sim, w.m, i).kind {
        program::CHOP | program::DIG | program::MINE | program::TEND_COWS => {
            short.wrapping_mul(2) / 3
        }
        program::ATTACK => short.wrapping_mul(2),
        program::IDLE => short.wrapping_mul(3) / 2,
        program::BUILD => return 0,
        _ => short,
    };
    s.wrapping_add(1)
}

#[cfg(test)]
#[path = "goals_tests.rs"]
mod tests;
