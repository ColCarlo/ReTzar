//! The spells of a computer player's units: what a unit casts before an
//! attack (0x414730, with 0x414cd0, 0x4145e0 and 0x4146e0), the priest at
//! its idle tick (0x414f10), the ninja setting off in advance (0x4ebe10)
//! and the flight of the kingdom's units from its own fire rain and freeze
//! (0x472760).
use super::breach;
use super::build;
use super::idle::to_centre;
use super::places::Forces;
use super::rules::World;
use super::*;

/// The native numbers of the spells (0x58a2f0).
mod spell {
    pub const FIRE_RAIN: u8 = 0;
    pub const LIGHTNING: u8 = 1;
    pub const HEAL: u8 = 2;
    pub const BLESS: u8 = 5;
    pub const FREEZE: u8 = 6;
    pub const BERSERK: u8 = 7;
    pub const WHIRLPOOL: u8 = 15;
    pub const FLAME_BOLT: u8 = 17;
    pub const RAISE_EXP: u8 = 18;
}

/// The square round the target that the area spells weigh (0x414a26), and
/// the hostile strength they need there when the target attacks nobody of
/// the kingdom (0x41469d).
const AREA: i32 = 3;
const AREA_STRENGTH: u32 = 20;
/// A target this sturdy is worth a bolt (0x414bd6).
const BOLT_HEALTH: u16 = 100;
/// The rest after the flight of the attacked mage (0x414c8b) and of the
/// units leaving the area of a spell (0x472746).
const FLIGHT_REST_MS: u32 = 100;
/// The priest heals a unit this near (0x4150a8) and teaches or blesses one
/// this near (0x41522e, 0x4152e9).
const HEAL_REACH: u32 = 64;
const PRIEST_REACH: i32 = 15;
/// A ninja hides setting off in advance with this much mana (0x4ebe89).
const HIDE_MANA: u16 = 90;
/// The civilization of the priests who teach (+0x3f).
const ASIAN: u8 = 3;

fn key(spell: u8) -> &'static str {
    world::spell_key(spell).unwrap_or_default()
}

/// How a unit casts a spell (0x5af424): from a charged book of it that it
/// holds, or with its own mana.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Means {
    Book(usize),
    Mana,
}

/// The first book of `spell` among the unit's things whose mana is full
/// (0x414ce5).
fn book(sim: &Simulation, m: &MissionWorld, i: usize, spell: u8) -> Option<usize> {
    sim.objects[i].contents.iter().copied().find(|&j| {
        let x = &m.objects[j];
        x.class == class::ITEM_MAGIC
            && x.byte(96) == spell
            && sim.objects[j].mana >= sim.objects[j].max_mana
    })
}
/// 0x414d3f: the unit has mana and learned the spell from a book
/// (vt+0x238, its own menu), or is a mage, or a priest for heal, bless and,
/// Asian, raise experience, and its kingdom has the spell's research
/// (0x60a850).
fn studied(sim: &Simulation, m: &MissionWorld, c: &Catalog, i: usize, spell: u8) -> bool {
    let o = &m.objects[i];
    if sim.objects[i].max_mana == 0 {
        return false;
    }
    if sim.gameplay.actors[i].spells.contains(&spell) {
        return true;
    }
    let taught = match o.class {
        class::MAGE => true,
        class::PRIEST => match spell {
            spell::HEAL | spell::BLESS => true,
            spell::RAISE_EXP => o.civilization == ASIAN,
            _ => false,
        },
        _ => false,
    };
    // «notech» does not ask the research (0x414897).
    taught
        && (m.cheats.no_tech
            || c.order_research(key(spell)).is_none_or(|dep| {
                researches(m, o.owner)
                    .iter()
                    .any(|r| r.eq_ignore_ascii_case(dep.trim()))
            }))
}
/// The mana a spell asks (0x4723e0); for bless, all the unit's mana
/// (vt+0x214, 0x414e19).
fn cost(sim: &Simulation, m: &MissionWorld, c: &Catalog, i: usize, spell: u8) -> u16 {
    if spell == spell::BLESS {
        sim.objects[i].max_mana
    } else {
        spell_parameters(c, m, &m.objects[i], key(spell)).1
    }
}
/// 0x414cd0: a charged book of the spell, else the unit's own mana when
/// it studied the spell and the mana reaches the cost.
fn means(sim: &Simulation, m: &MissionWorld, c: &Catalog, i: usize, spell: u8) -> Option<Means> {
    if let Some(j) = book(sim, m, i, spell) {
        return Some(Means::Book(j));
    }
    (studied(sim, m, c, i, spell) && sim.objects[i].mana >= cost(sim, m, c, i, spell))
        .then_some(Means::Mana)
}
/// The Cast program (0x12, 0x42ebc0) of `spell` at `p` on `t`.
fn cast(spell: u8, p: [u16; 2], t: usize) -> Intent {
    Intent::Cast {
        key: key(spell).into(),
        cell: p,
        target: Some(t),
    }
}
/// 0x4146e0: the use of the book (program 0x11, 0x42ea30) or the cast.
fn program(by: Means, spell: u8, p: [u16; 2], t: usize) -> Intent {
    match by {
        Means::Book(j) => Intent::UseItem {
            item: j,
            target: Some(t),
            cell: p,
        },
        Means::Mana => cast(spell, p, t),
    }
}

/// What the area test counts (0x414530, 0x414560, 0x4145b0): never a unit
/// on its way (op 9); for freeze only units, no machine (0x2719) nor one
/// of 1 health; for berserk nobody already berserk.
#[derive(Clone, Copy, Debug)]
enum Filter {
    Still,
    Freeze,
    Berserk,
}
impl Filter {
    fn passes(self, sim: &Simulation, m: &MissionWorld, j: usize) -> bool {
        let x = &m.objects[j];
        let unit = crate::selection::unit(x);
        if unit && sim.gameplay.actors[j].movement.is_some() {
            return false;
        }
        match self {
            Filter::Still => true,
            Filter::Freeze => unit && !machine(x) && x.health != 1,
            Filter::Berserk => !has_effect(sim, j, effect::DISCORD),
        }
    }
}
/// 0x4145e0: the objects in the cells within 3 of `p` that pass `filter`,
/// once for each of their cells there (0x44e060 reads the cells one by
/// one), split into those the unit's kingdom attacks on its own (0x468190)
/// and the others (0x414500, 0x402080). Worth a spell with more than one
/// hostile and, when the target attacks the unit or a mage of its kingdom,
/// hostile strength at least the others'; else 20 or more and at least
/// twice theirs.
#[allow(clippy::too_many_arguments)]
fn worth(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    u: usize,
    p: [u16; 2],
    filter: Filter,
    aimed: bool,
) -> bool {
    let k = m.objects[u].owner;
    let (x, y) = (p[0] as i32, p[1] as i32);
    let area = [x - AREA, y - AREA, x + AREA, y + AREA];
    let (mut hostile, mut others) = (Forces::default(), Forces::default());
    for j in 0..m.objects.len() {
        let o = &m.objects[j];
        if !alive(o)
            || sim.objects[j].holder.is_some()
            || !(crate::selection::unit(o) || o.building())
        {
            continue;
        }
        let r = movement::footprint_rect(lib, o);
        let w = r[2].min(area[2]) - r[0].max(area[0]) + 1;
        let h = r[3].min(area[3]) - r[1].max(area[1]) + 1;
        if w <= 0 || h <= 0 || !filter.passes(sim, m, j) {
            continue;
        }
        let forces = if hostile_to(sim, m, k, j) {
            &mut hostile
        } else {
            &mut others
        };
        for _ in 0..w * h {
            forces.count_in(sim, m, lib, c, j);
        }
    }
    if hostile.count(0xff) <= 1 {
        return false;
    }
    let (h, o) = (hostile.strength(0xff), others.strength(0xff));
    if aimed {
        h >= o
    } else {
        h >= AREA_STRENGTH && h as u64 >= 2 * o as u64
    }
}

/// 0x414730 (from 0x4dd8c0), before a computer player's unit `u` attacks
/// `t`: unless one blow (vt+0x1e0) would fell the target or it is the
/// dummy, a spell may come instead. Against a ship a whirlpool, cast with
/// mana even when a book was found; then at the target's centre fire rain,
/// freeze or berserk when the area round it is worth it (`worth`); then
/// lightning or a flame bolt when the target attacks the unit or a mage of
/// its kingdom (0x5ab8fc), or has 100 health or more. A mage the target
/// attacks takes refuge (0x413f50) or runs five cells away (0x430be0).
pub(crate) fn before_attack(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    u: usize,
    t: usize,
) -> Option<Intent> {
    let o = &m.objects[u];
    let x = &m.objects[t];
    let blow = sim.effective_attributes(u, m).map_or(0, |s| s.attack);
    if x.health as u32 <= blow || x.class == class::DUMMY {
        return None;
    }
    // +0x1c: whom the target attacks (program 3).
    let aim = match sim.gameplay.actors.get(t).and_then(|a| a.intent.as_ref()) {
        Some(&Intent::Attack(v)) if m.objects.get(v).is_some_and(alive) => Some(v),
        _ => None,
    };
    let aimed = aim.is_some_and(|v| {
        let y = &m.objects[v];
        v == u || (y.class == class::MAGE && y.owner == o.owner)
    });
    if matches!(x.class, 23..=25) && means(sim, m, c, u, spell::WHIRLPOOL).is_some() {
        return Some(cast(spell::WHIRLPOOL, [x.x, x.y], t));
    }
    let p = centre(lib, x);
    for (spell, filter) in [
        (spell::FIRE_RAIN, Filter::Still),
        (spell::FREEZE, Filter::Freeze),
        (spell::BERSERK, Filter::Berserk),
    ] {
        if let Some(by) = means(sim, m, c, u, spell)
            && worth(sim, m, lib, c, u, p, filter, aimed)
        {
            return Some(program(by, spell, p, t));
        }
    }
    for spell in [spell::LIGHTNING, spell::FLAME_BOLT] {
        if let Some(by) = means(sim, m, c, u, spell)
            && (aimed || x.health >= BOLT_HEALTH)
        {
            return Some(program(by, spell, p, t));
        }
    }
    if o.class != class::MAGE || aim != Some(u) {
        return None;
    }
    let refuge = sim
        .gameplay
        .ai
        .players
        .get(&o.owner)
        .and_then(|pl| idle::refuge(sim, m, lib, c, pl, u));
    Some(match refuge {
        Some(r) => Intent::Board(r),
        None => {
            let side = sim.gameplay.pathing.cells.side;
            let away = flight_point(sim, side, [o.x, o.y], [x.x, x.y]);
            Intent::Flee(away, FLIGHT_REST_MS)
        }
    })
}

/// 0x506c30 (vt+0x2a0): kingdom `k` commands unit `j`, neither frozen nor
/// stone ([`commanded_by`]).
fn commanded(sim: &Simulation, m: &MissionWorld, j: usize, k: u8) -> bool {
    !has_effect(sim, j, effect::FREEZE)
        && !has_effect(sim, j, effect::STONE)
        && commanded_by(m, j, k)
}
/// 0x472760 at the start of the spell op (0x4e1c74): a computer player's
/// fire rain or freeze sends out of the square of 3 round its point (the
/// target's centre, else the cell) the units its kingdom commands
/// (0x44dc70 with 4, 0x4726e0), but the caster, those it attacks on its
/// own and, for freeze, the machines; each runs five cells from the point
/// and rests 100 ms (0x413ae0).
#[allow(clippy::too_many_arguments)]
pub(crate) fn clear_area(
    sim: &mut Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    key: &str,
    cell: [u16; 2],
    target: Option<usize>,
) {
    let freeze = key == "spell_freeze";
    let o = &m.objects[i];
    if !(freeze || key == "spell_firerain")
        || !combat::computer(sim, o.owner)
        || o.flags & (FLAG_STORY | FLAG_DYING) != 0
    {
        return;
    }
    let p = match target {
        Some(t) => match m.objects.get(t).filter(|x| alive(x)) {
            Some(x) => centre(lib, x),
            None => return,
        },
        None => cell,
    };
    let k = o.owner;
    let (x, y) = (p[0] as i32, p[1] as i32);
    for j in breach::meeting(sim, m, lib, [x - AREA, y - AREA, x + AREA, y + AREA]) {
        let u = &m.objects[j];
        if j == i
            || !crate::selection::unit(u)
            || !standing(m, j)
            || sim.objects[j].holder.is_some()
            || (freeze && machine(u))
            || hostile_to(sim, m, k, j)
            || !commanded(sim, m, j, k)
        {
            continue;
        }
        run_away(sim, m, g, lib, c, j, p, FLIGHT_REST_MS);
    }
}

/// 0x4ebe10 when a leg of an advance starts (op 9 with argument 2): a
/// computer player's ninja with 90 mana that is not hidden hides at once.
pub(crate) fn set_off(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
) {
    let o = &m.objects[i];
    if o.class != class::NINJA
        || !combat::computer(sim, o.owner)
        || o.flags & (FLAG_STORY | FLAG_DYING) != 0
        || sim.objects[i].mana < HIDE_MANA
        || sim.gameplay.actors[i].effect_until > sim.ticks
    {
        return;
    }
    hide(sim, m, lib, c, i);
}

impl Player {
    /// 0x414f10: an idle priest of the player. One time in two (R(0, 1))
    /// it heals (spell 2: its book, or mana it studied the spell for; the
    /// mana must reach the cost either way) the nearest wounded unit of its
    /// kingdom, no machine, when it is within 64; with a book or the mana
    /// it goes no further. Otherwise, one time in two again, it teaches
    /// (spell 18) the nearest Asian peasant, monk, ninja or priest of a
    /// lower level within 15, no machine, of a kingdom allied both ways
    /// (0x44f170); unable to teach, it blesses (spell 5) the nearest
    /// fighter within 15 of such a kingdom, not blessed, on a cell the
    /// player deems safe (0x414e40, 0x411a30). Whether it was given work.
    pub(super) fn idle_priest(&mut self, w: &mut World, i: usize) -> bool {
        if w.sim.gameplay.game_rng.range(0, 1) == 0
            && (book(w.sim, w.m, i, spell::HEAL).is_some()
                || (studied(w.sim, w.m, w.c, i, spell::HEAL)
                    && w.sim.objects[i].mana >= cost(w.sim, w.m, w.c, i, spell::HEAL)))
        {
            return self.heal(w, i);
        }
        if w.sim.gameplay.game_rng.range(0, 1) != 0 {
            return false;
        }
        if means(w.sim, w.m, w.c, i, spell::RAISE_EXP).is_some() {
            return self.teach(w, i);
        }
        if means(w.sim, w.m, w.c, i, spell::BLESS).is_some() {
            return self.bless(w, i);
        }
        false
    }
    /// 0x41502d: the heal itself (program 0x430020, vt+0x160).
    fn heal(&mut self, w: &mut World, i: usize) -> bool {
        if w.sim.objects[i].mana < cost(w.sim, w.m, w.c, i, spell::HEAL) {
            return false;
        }
        let o = &w.m.objects[i];
        let at = [o.x as i32, o.y as i32];
        let mut best: Option<(i32, usize)> = None;
        for j in think::kingdom_list(w.sim, w.m, self.kingdom, true) {
            let x = &w.m.objects[j];
            if !standing(w.m, j)
                || w.sim.objects[j].holder.is_some()
                || x.health >= x.max_health
                || machine(x)
                || relation(w.m, x.owner, o.owner) & 0x40 == 0
            {
                continue;
            }
            let d = to_centre(w.lib, at, x);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, j));
            }
        }
        let Some((_, j)) = best else {
            return false;
        };
        let own = |j: usize| movement::footprint_rect(w.lib, &w.m.objects[j]);
        if combat::gap(own(j), own(i)) > HEAL_REACH {
            return false;
        }
        let x = &w.m.objects[j];
        let intent = Intent::Cast {
            key: key(spell::HEAL).into(),
            cell: [x.x, x.y],
            target: Some(j),
        };
        set_order(w.sim, i, intent, false).is_ok()
    }
    /// 0x4151fb: the nearest pupil (program 0x4300d0, vt+0x160).
    fn teach(&mut self, w: &mut World, i: usize) -> bool {
        let o = &w.m.objects[i];
        let level = w.sim.objects[i].level;
        let pupil = |w: &World, j: usize| {
            let x = &w.m.objects[j];
            crate::selection::unit(x)
                && standing(w.m, j)
                && w.sim.objects[j].holder.is_none()
                && x.civilization == ASIAN
                && matches!(x.class, class::PEON | 12 | class::NINJA | class::PRIEST)
                && w.sim.objects[j].level < level
                && !machine(x)
                && allied_both(w.m, o.owner, x.owner)
        };
        let Some(j) = self.nearest(w, i, |w, j| pupil(w, j)) else {
            return false;
        };
        let x = &w.m.objects[j];
        let intent = Intent::Cast {
            key: key(spell::RAISE_EXP).into(),
            cell: [x.x, x.y],
            target: Some(j),
        };
        set_order(w.sim, i, intent, false).is_ok()
    }
    /// 0x4152b7: the nearest fighter to bless on a safe cell (0x42ebc0,
    /// vt+0x160).
    fn bless(&mut self, w: &mut World, i: usize) -> bool {
        let owner = w.m.objects[i].owner;
        let blessed = |w: &World, j: usize| {
            let x = &w.m.objects[j];
            crate::selection::combat(x)
                && standing(w.m, j)
                && w.sim.objects[j].holder.is_none()
                && !has_effect(w.sim, j, effect::BLESS)
                && allied_both(w.m, owner, x.owner)
        };
        let Some(j) = self.nearest(w, i, |w, j| {
            blessed(w, j) && {
                let x = &w.m.objects[j];
                build::safe(w, self, x.x as i32, x.y as i32, [1, 1])
            }
        }) else {
            return false;
        };
        let x = &w.m.objects[j];
        set_order(w.sim, i, cast(spell::BLESS, [x.x, x.y], j), false).is_ok()
    }
    /// 0x44e690 over the square of 15 round the priest's cell: of the
    /// objects there, in the order of the sector lists, the first nearest
    /// to that cell by the centres (0x454d70) that `take` accepts, tested
    /// only when nearer than the best so far.
    fn nearest(&self, w: &World, i: usize, take: impl Fn(&World, usize) -> bool) -> Option<usize> {
        let o = &w.m.objects[i];
        let at = [o.x as i32, o.y as i32];
        let area = [
            at[0] - PRIEST_REACH,
            at[1] - PRIEST_REACH,
            at[0] + PRIEST_REACH,
            at[1] + PRIEST_REACH,
        ];
        let mut best: Option<(i32, usize)> = None;
        for j in breach::meeting(w.sim, w.m, w.lib, area) {
            let d = to_centre(w.lib, at, &w.m.objects[j]);
            if best.is_none_or(|(b, _)| d < b) && take(w, j) {
                best = Some((d, j));
            }
        }
        best.map(|(_, j)| j)
    }
}
/// A kingdom `b` allied to `a` both ways (0x44dc70 with 0x40, then
/// 0x467d10 the other way).
fn allied_both(m: &MissionWorld, a: u8, b: u8) -> bool {
    relation(m, a, b) & 0x40 != 0 && relation(m, b, a) & 0x40 != 0
}

#[cfg(test)]
#[path = "spells_tests.rs"]
mod tests;
