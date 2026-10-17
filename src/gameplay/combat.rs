//! The fight's own rules: reach (0x502550, 0x5024d0), who may be attacked
//! (0x502390), the target categories and priorities (0x412730, 0x4129d0)
//! and the choice between two targets (0x412bf0).

use super::*;

/// Priorities by target category 1–13 (0x4129d0): a human kingdom's
/// (0x5810a4) and a computer kingdom's (0x5810d8); catapults use their own
/// pair (0x58110c, 0x581140).
const PRIORITY: [u32; 13] = [
    0, 2000, 3000, 4000, 4000, 6000, 7000, 8000, 500, 4000, 2800, 2200, 0,
];
const PRIORITY_AI: [u32; 13] = [
    0, 2000, 4000, 4000, 4000, 6000, 7000, 8000, 500, 5000, 2800, 2800, 2700,
];
const PRIORITY_CATAPULT: [u32; 13] = [
    4000, 5000, 1000, 6000, 3000, 2000, 2500, 7000, 0, 3000, 5500, 5200, 4000,
];
const PRIORITY_CATAPULT_AI: [u32; 13] = [
    0, 5000, 1000, 6000, 3000, 2000, 2500, 7000, 0, 3000, 5500, 5200, 6000,
];

/// An object's own cells (vt+0x130).
fn cells(lib: &ObjectLibrary, o: &MapObject) -> [i32; 4] {
    movement::footprint_rect(lib, o)
}
/// 0x401b30: the empty cells between two rects along the axis where they
/// are farthest apart, the side chosen by their centres; 0 when touching.
pub(super) fn gap(a: [i32; 4], b: [i32; 4]) -> u32 {
    let centre = |r: [i32; 4]| [(r[0] + r[2]) / 2, (r[1] + r[3]) / 2];
    let (ca, cb) = (centre(a), centre(b));
    let dx = if ca[0] < cb[0] {
        b[0] - a[2] - 1
    } else {
        a[0] - b[2] - 1
    };
    let dy = if ca[1] < cb[1] {
        b[1] - a[3] - 1
    } else {
        a[1] - b[3] - 1
    };
    dx.max(dy).max(0) as u32
}
pub(super) fn object_gap(lib: &ObjectLibrary, a: &MapObject, b: &MapObject) -> u32 {
    gap(cells(lib, a), cells(lib, b))
}
/// The RANGE column (vt+0x1fc), 0 for melee.
pub(super) fn max_range(m: &MissionWorld, c: &Catalog, k: usize) -> u32 {
    let o = &m.objects[k];
    c.stats(o, o.class_name(), researches(m, o.owner)).range
}
/// `min_attack_range` of the object's INI (classdef +0x8e, vt+0x200).
pub(super) fn min_range(lib: &ObjectLibrary, o: &MapObject) -> u32 {
    lib.property(o, "min_attack_range")
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}
/// 0x502550 from the object's own footprint: its centre lies in `rect` or
/// within `n` cells of it.
pub(super) fn within(
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    rect: [i32; 4],
    n: u32,
) -> bool {
    let o = &m.objects[i];
    let [w, h] = footprint(lib, o);
    movement::reaches([o.x, o.y], [w as u16, h as u16], rect, n)
}
/// vt+0x224 = 0x5024d0: the target's rect grown by one within RANGE and
/// not within the minimum range.
pub(super) fn in_reach(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    j: usize,
) -> bool {
    let rect = movement::target_rect(sim, m, lib, j);
    if !within(m, lib, i, rect, max_range(m, c, i)) {
        return false;
    }
    let min = min_range(lib, &m.objects[i]);
    min == 0 || !within(m, lib, i, rect, min)
}
/// The target lies within the attacker's minimum range (0x4dfae9).
pub(super) fn too_close(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    j: usize,
) -> bool {
    let min = min_range(lib, &m.objects[i]);
    min != 0 && within(m, lib, i, movement::target_rect(sim, m, lib, j), min)
}
/// vt+0x228: the unit may leave its place (+0xd6); Stop's hold, stand
/// ground and hold steady keep it, and buildings never move.
pub(super) fn free(sim: &Simulation, m: &MissionWorld, i: usize) -> bool {
    mobile(&m.objects[i]) && sim.gameplay.actors[i].stance == 0
}
/// vt+0x21c = 0x502390: whether `i` may attack `j` at all. A unit needs an
/// attack picture; the magic gateway and the gold and stone mines are never
/// attacked, the dummy only by what answers 0 to 0x2729, below level 8 and
/// the cap; with `hold` a unit that keeps its place only hits what it
/// already reaches. Stone is never hit, a land fighter without RANGE never
/// hits water, and a cannon ball (+0x6b = 2) never hits the air.
///
/// Some classes have their own: the bat (0x4ec6b0) and the bear (0x4eee30,
/// on land only) hit any living object that is not a building (0x2719),
/// a priest with a RANGE only what leaves no body (0x4ea5a0, 0x271c),
/// siege engines never the air (0x4e7580); the melee priest and the siege
/// engines are always judged with `hold`.
pub(super) fn can_attack(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    j: usize,
    hold: bool,
) -> bool {
    let (Some(o), Some(x)) = (m.objects.get(i), m.objects.get(j)) else {
        return false;
    };
    let stone = has_effect(sim, j, effect::STONE);
    let mut hold = hold;
    match o.class {
        15 | 26 => {
            return !stone && !x.building() && !machine(x) && (o.class == 15 || lib.layer(x) == 1);
        }
        18 if max_range(m, c, i) > 0 => return !stone && bodiless(x),
        19..=21 if stone || lib.layer(x) == 0 => return false,
        18..=21 => hold = true,
        _ => {}
    }
    if i == j
        || (crate::selection::unit(o) && lib.property(o, "image_attack").is_none())
        || matches!(x.class, 72 | 76 | 78)
    {
        return false;
    }
    if x.class == class::DUMMY {
        let cap = experience_rules(m, o.owner).cap;
        let level = sim.objects[i].level;
        if untrainable(o) || level >= 8 || level >= cap {
            return false;
        }
    }
    if hold && !free(sim, m, i) && !in_reach(sim, m, lib, c, i, j) {
        return false;
    }
    if stone {
        return false;
    }
    if lib.layer(o) == 1 && max_range(m, c, i) == 0 && lib.layer(x) == 2 {
        return false;
    }
    !(crate::simulation::weapon_category(o.class) == 2 && lib.layer(x) == 0)
}
/// Whether the kingdom has a computer player (the controller of 0x5abb14).
pub(super) fn computer(sim: &Simulation, owner: u8) -> bool {
    super::ai::controls(sim, owner)
}
/// The peasant builds a tower (0x4125b0).
fn building_tower(sim: &Simulation, m: &MissionWorld, t: usize) -> bool {
    matches!(sim.gameplay.actors[t].intent, Some(Intent::Build(b))
        if m.objects.get(b).is_some_and(|x| x.building() && alive(x) && (39..=43).contains(&x.class)))
}
/// Target category of `t` for the scanner `u` (0x412730).
fn category(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    u: usize,
    t: usize,
    depth: u8,
) -> u8 {
    let x = &m.objects[t];
    match x.class {
        class::PEON => {
            if building_tower(sim, m, t) {
                let rect = movement::target_rect(sim, m, lib, t);
                return if within(m, lib, u, rect, max_range(m, c, u) + 2) {
                    6
                } else {
                    3
                };
            }
            match sim.gameplay.actors[t].intent {
                Some(Intent::Capture(_)) => 7,
                Some(Intent::Attack(_)) => 5,
                // The repaired object's own category, while it needs repair.
                Some(Intent::Repair(r))
                    if depth == 0
                        && m.objects.get(r).is_some_and(|y| {
                            alive(y) && y.flags & FLAG_DYING == 0 && y.health < y.max_health
                        }) =>
                {
                    category(sim, m, lib, c, u, r, depth + 1)
                }
                _ => 3,
            }
        }
        17 => 10,
        44 | 55 | 56 | 60 | 65 | 70 | 71 => 12,
        45 | 49 | 59 => 11,
        class::DUMMY => 9,
        _ if crate::selection::unit(x) => {
            if military(sim, m, t) {
                5
            } else {
                3
            }
        }
        _ if x.building() => {
            // A site not started yet, near enough to be reached soon.
            if x.flags & FLAG_CONSTRUCTION != 0
                && sim.gameplay.actors[t].construction == 0
                && within(
                    m,
                    lib,
                    u,
                    movement::target_rect(sim, m, lib, t),
                    max_range(m, c, u) + 4,
                )
            {
                8
            } else if (39..=43).contains(&x.class) {
                4
            } else if matches!(x.class, 61 | 62) {
                // Walls (0x1000): 13 once marked for a breach (+0x160).
                if sim.gameplay.actors[t].marked { 13 } else { 1 }
            } else {
                2
            }
        }
        _ => 0,
    }
}
/// 0x4129d0: the class chosen with "attack type" first, then the table of
/// the target's category.
fn priority(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    u: usize,
    t: usize,
) -> u32 {
    if sim.gameplay.actors[u].attack_type == Some(m.objects[t].class) {
        return 9000;
    }
    let cat = category(sim, m, lib, c, u, t, 0);
    if cat == 0 {
        return 0;
    }
    let ai = super::ai_managed(sim, m, u);
    let table = match (matches!(m.objects[u].class, 19 | 20), ai) {
        (true, true) => &PRIORITY_CATAPULT_AI,
        (true, false) => &PRIORITY_CATAPULT,
        (false, true) => &PRIORITY_AI,
        (false, false) => &PRIORITY,
    };
    table[cat as usize - 1]
}
/// 0x412aa0: `x` attacks what `u` guards.
fn attacks_guarded(sim: &Simulation, m: &MissionWorld, x: usize, u: usize) -> bool {
    let Some(Intent::Attack(t)) = sim.gameplay.actors[x].intent.as_ref() else {
        return false;
    };
    let guard = &sim.gameplay.actors[u];
    m.objects.get(*t).is_some_and(|o| o.flags & FLAG_DYING == 0)
        && if guard.guard_class != 0 {
            m.objects[*t].class as u16 == guard.guard_class
        } else {
            super::guarded(guard).any(|g| g == *t)
        }
}
/// 0x412b30: the BONUS.INI entry of `a` against `b`, read as a percentage
/// of `a`'s attack.
fn edge(sim: &Simulation, m: &MissionWorld, c: &Catalog, a: usize, b: usize) -> i32 {
    let attack = sim
        .effective_attributes(a, m)
        .map_or(sim.objects[a].attack, |s| s.attack) as i32
        & 0xffff;
    let value = c
        .bonus(&m.objects[a], &m.objects[b])
        .map_or(0, |(v, _)| v as i32);
    attack * value / 100
}
fn matchup(sim: &Simulation, m: &MissionWorld, c: &Catalog, u: usize, t: usize) -> i16 {
    (edge(sim, m, c, u, t) as i16).wrapping_sub(edge(sim, m, c, t, u) as i16)
}
/// Attack programs hooked on each object (+0x108, ops 0x6f/0x70) and the
/// kingdoms' units striking each building (+0x12c, op 0xd).
/// Who attacks whom, counted when a candidate is first judged.
pub(super) struct Hooks<'a> {
    sim: &'a Simulation,
    m: &'a MissionWorld,
    counted: std::cell::OnceCell<Counts>,
}
struct Counts {
    attackers: Vec<u16>,
    strikers: BTreeMap<(usize, u8), u16>,
}
impl<'a> Hooks<'a> {
    pub(super) fn new(sim: &'a Simulation, m: &'a MissionWorld) -> Self {
        Self {
            sim,
            m,
            counted: std::cell::OnceCell::new(),
        }
    }
    fn counts(&self) -> &Counts {
        self.counted.get_or_init(|| {
            let (sim, m) = (self.sim, self.m);
            let mut attackers = vec![0u16; m.objects.len()];
            let mut strikers = BTreeMap::new();
            for (k, a) in sim.gameplay.actors.iter().enumerate() {
                if let Some(Intent::Attack(t)) = a.intent
                    && t < attackers.len()
                {
                    attackers[t] = attackers[t].saturating_add(1);
                    if sim.objects[k].order == OrderState::Attacking {
                        let n = strikers.entry((t, m.objects[k].owner)).or_insert(0u16);
                        *n = n.saturating_add(1);
                    }
                }
            }
            Counts {
                attackers,
                strikers,
            }
        })
    }
}
/// 0x412bf0: whether `t` is a better target for `u` than `best`. A
/// candidate must be hostile, attackable, outside the minimum range and,
/// for a building, not already struck by the kingdom.
pub(super) fn better(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    hooks: &Hooks,
    u: usize,
    best: Option<usize>,
    t: usize,
) -> bool {
    let (o, x) = (&m.objects[u], &m.objects[t]);
    let ai = super::ai_managed(sim, m, u);
    if x.flags & FLAG_DYING != 0
        || !alive(x)
        || x.class == class::BRIDGE
        || (!ai && matches!(x.class, class::GATE_WOOD | class::GATE_STONE))
        || !auto_hostile(sim, m, u, t)
        || !can_attack(sim, m, lib, c, u, t, true)
    {
        return false;
    }
    if x.building() {
        let n = hooks
            .counts()
            .strikers
            .get(&(t, o.owner))
            .copied()
            .unwrap_or(0);
        if n > u16::from(ai) {
            return false;
        }
    }
    if too_close(sim, m, lib, u, t) {
        return false;
    }
    let prio = priority(sim, m, lib, c, u, t);
    let Some(b) = best else {
        return prio > 0;
    };
    let y = &m.objects[b];
    let prio_b = priority(sim, m, lib, c, u, b);
    let gap_b = object_gap(lib, o, y);
    let gap_t = object_gap(lib, o, x);
    let range = max_range(m, c, u) & 0xff;
    let (reach_t, reach_b) = (range >= gap_t, range >= gap_b);
    let unit = |k: usize| crate::selection::unit(&m.objects[k]);
    if ai && !(reach_t && reach_b) {
        // A computer kingdom spreads its fighters first.
        let count = |k: usize| {
            if unit(k) {
                hooks.counts().attackers[k] as u32
            } else {
                0
            }
        };
        let (ct, cb) = (count(t), count(b));
        if ct >= cb + 5 {
            return false;
        }
        if cb >= ct + 5 {
            return true;
        }
    }
    if prio == 0 || prio < prio_b {
        return false;
    }
    if prio > prio_b {
        return true;
    }
    match (attacks_guarded(sim, m, b, u), attacks_guarded(sim, m, t, u)) {
        (true, false) => return false,
        (false, true) => return true,
        _ => {}
    }
    match (reach_t, reach_b) {
        (true, false) => return true,
        (false, true) => return false,
        _ => {}
    }
    let apart = gap_t.abs_diff(gap_b);
    if apart < if ai { 4 } else { 2 } {
        let (mt, mb) = (matchup(sim, m, c, u, t), matchup(sim, m, c, u, b));
        if mt != mb {
            return mt > mb;
        }
    }
    if apart < 2 {
        if unit(t) && unit(b) {
            let near = |k: usize| {
                (hooks.counts().attackers[k] as i16)
                    .wrapping_sub(2)
                    .unsigned_abs()
            };
            let (nt, nb) = (near(t), near(b));
            if nb != nt {
                return nb > nt;
            }
        }
        let d = (x.health as i16).wrapping_sub(y.health as i16);
        if d.unsigned_abs() > 20 {
            return d < 0;
        }
    }
    gap_t < gap_b
}
/// vt+0x2d0 = 0x4e4d90: the best target around `u`. Units search their
/// sight + 1, towers their RANGE + 1 (0x4db8d0) among what they reach
/// (0x4db890); under Discord the nearest object of any kingdom (0x4e4cb0).
pub(super) fn search(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    u: usize,
) -> Option<usize> {
    let o = &m.objects[u];
    let tower = (39..=43).contains(&o.class);
    let reach = if tower {
        max_range(m, c, u) + 1
    } else {
        sim.objects[u].sight as u32 + 1
    } as i32;
    let own = cells(lib, o);
    let area = [
        own[0] - reach,
        own[1] - reach,
        own[2] + reach,
        own[3] + reach,
    ];
    let discord = has_effect(sim, u, effect::DISCORD);
    if !discord && !tower && super::ai_managed(sim, m, u) {
        return search_computer(sim, m, lib, c, u);
    }
    let hooks = Hooks::new(sim, m);
    let mut best: Option<usize> = None;
    let mut nearest = u32::MAX;
    for t in candidates(sim, m, lib, c, u, area) {
        let x = &m.objects[t];
        if discord {
            if x.flags & FLAG_DYING != 0
                || x.class == class::BRIDGE
                || !can_attack(sim, m, lib, c, u, t, true)
                || too_close(sim, m, lib, u, t)
            {
                continue;
            }
            let d = distance(lib, o, x);
            if d < nearest {
                nearest = d;
                best = Some(t);
            }
        } else if better(sim, m, lib, c, &hooks, u, best, t) {
            best = Some(t);
        }
    }
    best
}
/// 0x44e690 round `area`: what `u` could look at there, in the order of the
/// sector lists (on full parity the first one met wins).
fn candidates(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    u: usize,
    area: [i32; 4],
) -> Vec<usize> {
    let owner = side(sim, m, u);
    let watched = owner == m.player();
    let tower = (39..=43).contains(&m.objects[u].class);
    // The cheap tests first, the object's own cell before anything else (its
    // rect starts there): a building's footprint is looked up only for what
    // may lie in the area.
    let mut order: Vec<usize> = (0..m.objects.len())
        .filter(|&t| {
            let x = &m.objects[t];
            if x.x as i32 > area[2]
                || x.y as i32 > area[3]
                || t == u
                || !(crate::selection::unit(x) || x.building())
                || !alive(x)
                || sim.objects[t].holder.is_some()
            {
                return false;
            }
            let r = cells(lib, x);
            !(r[2] < area[0]
                || r[3] < area[1]
                || !target_known(sim, m, owner, t)
                || (watched && !m.game_rules.no_fog && !sim.visible(x))
                || (tower && !in_reach(sim, m, lib, c, u, t)))
        })
        .collect();
    order.sort_by_key(|&t| sector_order(sim, t));
    order
}
/// The best of `area` by 0x412bf0, from `best`.
fn best_in(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    hooks: &Hooks,
    u: usize,
    area: [i32; 4],
) -> Option<usize> {
    let mut best = None;
    for t in candidates(sim, m, lib, c, u, area) {
        if better(sim, m, lib, c, hooks, u, best, t) {
            best = Some(t);
        }
    }
    best
}
/// 0x4130a0, vt+0x2d0 of a computer player's unit: its sight round its
/// cells. A place of its sector that holds the area and has no hostiles
/// present nor hostile towers, buildings or barracks coming spares the
/// look; when one has, it looks in the sectors of each such place and keeps
/// the better; else in the area. With nothing found, one of its kingdom's
/// training dummies within 20 more, when the unit trains and is below level
/// 8 and the kingdom's cap.
fn search_computer(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    u: usize,
) -> Option<usize> {
    use super::ai::places::mask;
    let o = &m.objects[u];
    let own = cells(lib, o);
    let sight = sim.objects[u].sight as i32;
    let area = [
        own[0] - sight,
        own[1] - sight,
        own[2] + sight,
        own[3] + sight,
    ];
    let hooks = Hooks::new(sim, m);
    let player = sim.gameplay.ai.players.get(&o.owner);
    let mgr = sim.gameplay.ai.manager.as_ref();
    let places: Vec<usize> = match (mgr, sim.gameplay.actors[u].sector) {
        (Some(mgr), Some(s)) => mgr.sector_places([s[0] as i32, s[1] as i32]).to_vec(),
        _ => Vec::new(),
    };
    let record = |slot: usize| {
        let p = mgr?.place(slot)?;
        let r = player?
            .records
            .iter()
            .find(|r| r.place == slot && r.serial == p.serial)?;
        Some((p.rect, r))
    };
    let threat = |r: &super::ai::records::Record| {
        r.hostile.count(0xff) != 0
            || r.hostile_incoming
                .count(mask::TOWERS | mask::BUILDINGS | mask::BARRACKS)
                != 0
    };
    let mut threatened = false;
    let mut look = true;
    for &slot in &places {
        let Some((rect, r)) = record(slot) else {
            continue;
        };
        if threat(r) {
            threatened = true;
            break;
        }
        // 0x515d40: the place holds the area.
        if rect[0] <= area[0] && rect[2] >= area[2] && rect[1] <= area[1] && rect[3] >= area[3] {
            look = false;
            break;
        }
    }
    let mut best = None;
    if threatened {
        for &slot in &places {
            let Some((rect, r)) = record(slot) else {
                continue;
            };
            if !threat(r) {
                continue;
            }
            let s = rect.map(|v| v.div_euclid(8));
            let cells = [s[0] * 8, s[1] * 8, s[2] * 8 + 7, s[3] * 8 + 7];
            let found = best_in(sim, m, lib, c, &hooks, u, cells);
            match (best, found) {
                (None, _) => best = found,
                (Some(_), Some(t)) if better(sim, m, lib, c, &hooks, u, best, t) => best = found,
                _ => {}
            }
        }
    } else if look {
        best = best_in(sim, m, lib, c, &hooks, u, area);
    }
    if best.is_some() {
        return best;
    }
    let dummies = player.map(|p| p.lists.dummies.clone()).unwrap_or_default();
    if dummies.is_empty()
        || untrainable(o)
        || sim.objects[u].level >= 8
        || sim.objects[u].level >= experience_rules(m, o.owner).cap
    {
        return None;
    }
    let near = [area[0] - 20, area[1] - 20, area[2] + 20, area[3] + 20];
    for d in dummies {
        let x = &m.objects[d];
        let (px, py) = (x.x as i32, x.y as i32);
        if px >= near[0]
            && px <= near[2]
            && py >= near[1]
            && py <= near[3]
            && better(sim, m, lib, c, &hooks, u, best, d)
        {
            best = Some(d);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_counts_the_empty_cells_between_rects() {
        // Touching or overlapping rects have no gap.
        assert_eq!(gap([0, 0, 0, 0], [1, 1, 1, 1]), 0);
        assert_eq!(gap([0, 0, 1, 1], [1, 0, 2, 1]), 0);
        // Three empty cells to the east, two to the south: the larger.
        assert_eq!(gap([0, 0, 0, 0], [4, 3, 4, 3]), 3);
        assert_eq!(gap([4, 3, 4, 3], [0, 0, 0, 0]), 3);
        // A 2×2 building and a unit five cells to the west.
        assert_eq!(gap([10, 10, 11, 11], [4, 10, 4, 10]), 5);
    }

    #[test]
    fn priority_tables_match_the_executable() {
        // 0x5810a4: civilians 3000, soldiers and towers 4000, a site about
        // to rise 8000, walls never.
        assert_eq!(PRIORITY[2], 3000);
        assert_eq!(PRIORITY[4], 4000);
        assert_eq!(PRIORITY[7], 8000);
        assert_eq!(PRIORITY[12], 0);
        // 0x58110c: catapults prefer towers and walls to soldiers.
        assert_eq!(PRIORITY_CATAPULT[3], 6000);
        assert_eq!(PRIORITY_CATAPULT[4], 3000);
        assert_eq!(PRIORITY_CATAPULT[12], 4000);
    }
}
