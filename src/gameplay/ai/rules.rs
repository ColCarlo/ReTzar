//! A computer player's script at work: its conditions (0x407e60), the
//! state of its rules (0x408420), the lists of rules that read each value
//! (0x41ab10), the Build phase that pursues their goals (0x408960) and the
//! goals carried out (0x409550), with the resources each part of the player
//! books (IA+0x914 to IA+0x988).
use super::script::{Condition, Program, flag, goal};
use super::*;

/// The parts that book resources (IA+0x93c + 0x14·i); each is served after
/// the ones before it (0x40b170).
pub mod bucket {
    /// Ships and harbors.
    pub const NAVY: usize = 0;
    /// Stores near the work.
    pub const STORES: usize = 1;
    /// Peasants, houses, fields and fishing boats.
    pub const WORKERS: usize = 2;
    /// The script's goals.
    pub const SCRIPT: usize = 3;
    /// Towers and defences.
    pub const DEFENCE: usize = 4;
    /// No part (0xff, 0x40b19c): nothing booked comes before, and the lock
    /// of `NEEDS_LOCKED` does not apply.
    pub const NONE: usize = 0xff;
}

/// The rules that read each value (0x41ab10), made from the program: a
/// rule is evaluated again when one of them changes.
#[derive(Clone, Debug, Default)]
pub struct Watch {
    pub built: bool,
    /// By rule (`RULE`), by switch (`TRIGGER`, and the switches a rule
    /// sets), by resource and by object type (0 is `UNITS`).
    pub rules: Vec<Vec<u32>>,
    pub triggers: Vec<Vec<u32>>,
    pub resources: [Vec<u32>; 4],
    pub types: BTreeMap<u16, Vec<u32>>,
    /// Time, `RANDOM`, `EXTERN`, `ENEMY`, `ALLY`, `CONVERT` and
    /// `NEEDS_LOCKED`: every Build phase (+0x83e).
    pub poll: Vec<u32>,
    /// The percentages of the population limit (+0x842).
    pub percent: Vec<u32>,
}
/// Made again from the program: not part of the state.
impl PartialEq for Watch {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for Watch {}
impl Watch {
    pub fn of(p: &Program) -> Self {
        let mut w = Self {
            built: true,
            rules: vec![Vec::new(); p.rules.len()],
            triggers: vec![Vec::new(); p.triggers.len()],
            ..Default::default()
        };
        fn push(list: &mut [Vec<u32>], at: u32, r: u32) {
            if let Some(l) = list.get_mut(at as usize) {
                l.push(r);
            }
        }
        for (r, rule) in p.rules.iter().enumerate() {
            let r = r as u32;
            let mut percent = false;
            for &n in p.entries(rule.when) {
                // A NOT leads to the condition it denies.
                let mut c = p.conditions.get(n as usize);
                while let Some(Condition::Not(inner)) = c {
                    c = p.conditions.get(*inner as usize);
                }
                match c {
                    Some(Condition::Rule(x)) => push(&mut w.rules, *x, r),
                    Some(Condition::Trigger(t)) => push(&mut w.triggers, t & 0x7fff_ffff, r),
                    Some(Condition::Resource { resource, .. }) => {
                        push(&mut w.resources, *resource as u32, r)
                    }
                    Some(Condition::Count { kind, count }) => {
                        percent |= count & script::PERCENT != 0;
                        w.types.entry(*kind).or_default().push(r);
                    }
                    Some(
                        Condition::Time(_)
                        | Condition::Random(_)
                        | Condition::Extern(_)
                        | Condition::Enemy { .. }
                        | Condition::Ally { .. },
                    ) => w.poll.push(r),
                    _ => {}
                }
            }
            for &t in p.entries(rule.triggers) {
                push(&mut w.triggers, t & 0x7fff_ffff, r);
            }
            if let Some(have) = rule.have {
                match have.kind {
                    0 | goal::RESIGN => {}
                    goal::NEEDS_LOCKED | goal::CONVERT => w.poll.push(r),
                    goal::GOLD => w.resources[0].push(r),
                    goal::STONE => w.resources[1].push(r),
                    goal::FOOD => w.resources[2].push(r),
                    goal::WOOD => w.resources[3].push(r),
                    kind => {
                        percent |= have.count as u32 & script::PERCENT != 0;
                        w.types.entry(kind).or_default().push(r);
                    }
                }
            }
            if percent {
                w.percent.push(r);
            }
        }
        w
    }
}

/// What a watched value names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watched {
    Rule(u32),
    Trigger(u32),
    Resource(usize),
    Type(u16),
    Poll,
    Percent,
}

impl Player {
    /// 0x407d00: the rules that read the value are evaluated again.
    pub fn touch(&mut self, what: Watched) {
        let Some(program) = self.program.as_mut() else {
            return;
        };
        if !self.watch.built {
            self.watch = Watch::of(program);
        }
        let w = &self.watch;
        let list: &[u32] = match what {
            Watched::Rule(r) => w.rules.get(r as usize).map_or(&[], Vec::as_slice),
            Watched::Trigger(t) => w.triggers.get(t as usize).map_or(&[], Vec::as_slice),
            Watched::Resource(r) => w.resources.get(r).map_or(&[], Vec::as_slice),
            Watched::Type(k) => w.types.get(&k).map_or(&[], Vec::as_slice),
            Watched::Poll => &w.poll,
            Watched::Percent => &w.percent,
        };
        for &r in list {
            if let Some(rule) = program.rules.get_mut(r as usize) {
                rule.flags |= flag::DIRTY;
            }
        }
    }
}

/// The world a computer player thinks in.
pub(crate) struct World<'a> {
    pub sim: &'a mut Simulation,
    pub m: &'a mut MissionWorld,
    pub g: &'a MapGrid,
    pub lib: &'a ObjectLibrary,
    pub c: &'a Catalog,
    /// The cells as the player last read them, built again after it lays an
    /// object.
    pub cells: Option<crate::gameplay::pathing::Cells>,
}

/// An object still counted in its kingdom's record of its type: the
/// constructor counts it (0x494da0) and its death or deletion takes it
/// off (0x494dc0); objects hidden by the map count.
pub(super) fn counted(o: &MapObject) -> bool {
    o.health > 0 && o.flags & FLAG_DYING == 0
}

/// The kingdom's record of a type (0x4976a0): how many there are
/// (+0x2c8: objects, sites included, or researches done) and how many are
/// still under way (+0x2cc: sites, and the units and researches whose step
/// has started).
pub fn record(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &script::Library,
    k: u8,
    kind: u16,
) -> (u32, u32) {
    let index = kind as usize;
    let mut made = 0u32;
    let mut under = 0u32;
    if index != 0 && index < CLASSES.len() {
        for o in &m.objects {
            if o.owner == k && o.class == index && counted(o) {
                made += 1;
                if o.building() && o.flags & FLAG_CONSTRUCTION != 0 {
                    under += 1;
                }
            }
        }
        for (i, a) in sim.gameplay.actors.iter().enumerate() {
            if m.objects.get(i).is_some_and(|o| o.owner == k && counted(o))
                && matches!(a.jobs.front(), Some(Job { kind: JobKind::Train(c), .. }) if *c == index)
            {
                under += 1;
            }
        }
        // The sites still to be laid count with their type (0x40b7f5).
        for v in sim.gameplay.ai.virtuals.values() {
            if v.owner == k && v.class == kind {
                made += 1;
                under += 1;
            }
        }
    } else if lib.category(kind) == 3 {
        let name = lib.name(kind).unwrap_or_default();
        made = crate::simulation::completions(researches(m, k), name);
        under = sim
            .gameplay
            .actors
            .iter()
            .enumerate()
            .filter(|(i, a)| {
                m.objects.get(*i).is_some_and(|o| o.owner == k && counted(o))
                    && matches!(a.jobs.front(), Some(Job { kind: JobKind::Research(n), .. }) if n.eq_ignore_ascii_case(name))
            })
            .count() as u32;
    }
    (made, under)
}

/// IA's 0x407da0: what the player has of a goal and how much of it is
/// under way. Resources are what it may spend, fields its list of them.
pub fn count(
    p: &Player,
    sim: &Simulation,
    m: &MissionWorld,
    lib: &script::Library,
    k: u8,
    kind: u16,
) -> (u32, u32) {
    match kind {
        goal::GOLD => (p.available[0] as u32, 0),
        goal::STONE => (p.available[1] as u32, 0),
        goal::FOOD => (p.available[2] as u32, 0),
        goal::WOOD => (p.available[3] as u32, 0),
        goal::CONVERT | goal::NEEDS_LOCKED | goal::RESIGN => (0, 0),
        k63 if k63 as usize == class::FIELD => (p.lists.fields.len() as u32, 0),
        _ => record(sim, m, lib, k, kind),
    }
}

/// K+0x5e and K+0x62: the kingdom's units and its population limit.
fn units(m: &MissionWorld, lib: &ObjectLibrary, k: u8) -> u32 {
    population_parts(m, lib, k).0 as u32
}
pub(super) fn limit(m: &MissionWorld, lib: &ObjectLibrary, k: u8) -> u32 {
    population_parts(m, lib, k).2 as u32
}
/// A count of a script: with `%`, that share of the population limit.
fn wanted(count: u32, limit: u32) -> u32 {
    if count & script::PERCENT != 0 {
        ((count & 0x7fff).wrapping_mul(limit) as u64 / 100) as u32
    } else {
        count
    }
}

/// What the other computer players spend and farm, as 0x407da0 of their
/// own objects gives it to `ENEMY` and `ALLY`.
#[derive(Clone, Copy, Default)]
pub(super) struct Other {
    pub available: [i32; 4],
    pub fields: u32,
}

/// 0x407e60: one condition of the script.
pub(super) fn holds(
    w: &mut World,
    p: &mut Player,
    k: u8,
    others: &BTreeMap<u8, Other>,
    n: u32,
) -> bool {
    let Some(c) = p
        .program
        .as_ref()
        .and_then(|x| x.conditions.get(n as usize))
        .copied()
    else {
        return false;
    };
    let lib = &w.c.ai;
    match c {
        Condition::Rule(r) => p
            .program
            .as_ref()
            .and_then(|x| x.rules.get(r as usize))
            .is_some_and(|x| x.flags & flag::SATISFIED != 0),
        Condition::Trigger(t) => p
            .program
            .as_ref()
            .and_then(|x| x.triggers.get(t as usize))
            .is_some_and(|x| x.on),
        Condition::Resource { resource, amount } => {
            p.available.get(resource as usize).copied().unwrap_or(0) >= amount as i32
        }
        Condition::Count { kind, count } => {
            let n = wanted(count, limit(w.m, w.lib, k));
            if kind == 0 {
                return units(w.m, w.lib, k) >= n;
            }
            let (made, under) = super::rules::count(p, w.sim, w.m, lib, k, kind);
            match lib.category(kind) {
                1 | 3 => made >= n,
                2 => made.wrapping_sub(under) >= n,
                _ => false,
            }
        }
        Condition::Time(ms) => w.sim.ticks * TICK_MS >= ms as u64,
        Condition::Random(chance) => {
            // Drawn once, R(0, 100), and kept (+6).
            *p.rolls.entry(n).or_insert_with(|| {
                let r = w.sim.gameplay.game_rng.range(0, 100);
                chance >= r as u32
            })
        }
        Condition::Extern(t) => {
            let text = p
                .program
                .as_ref()
                .and_then(|x| x.texts.get(t as usize))
                .cloned()
                .unwrap_or_default();
            scripting::extern_value(w.sim, w.m, w.g, w.lib, w.c, &text) != 0
        }
        Condition::Not(inner) => !holds(w, p, k, others, inner),
        Condition::Civ(civ) => p.civilization == civ,
        Condition::Difficulty(d) => w.m.game_rules.difficulty as u32 >= d as u32,
        Condition::Enemy { kind, count } | Condition::Ally { kind, count } => {
            let mask = if matches!(c, Condition::Enemy { .. }) {
                0x10
            } else {
                0x40
            };
            let n = wanted(count, limit(w.m, w.lib, k));
            let (mut made, mut under) = (0u32, 0u32);
            for b in 1..=8u8 {
                if relation(w.m, k, b) & mask == 0 {
                    continue;
                }
                if kind == 0 {
                    made = made.wrapping_add(units(w.m, w.lib, b));
                    continue;
                }
                // Another computer player answers with its own count.
                let (a, u) = if b == k {
                    super::rules::count(p, w.sim, w.m, lib, b, kind)
                } else if let Some(o) = others.get(&b) {
                    match kind {
                        goal::GOLD => (o.available[0] as u32, 0),
                        goal::STONE => (o.available[1] as u32, 0),
                        goal::FOOD => (o.available[2] as u32, 0),
                        goal::WOOD => (o.available[3] as u32, 0),
                        goal::CONVERT | goal::NEEDS_LOCKED | goal::RESIGN => (0, 0),
                        f if f as usize == class::FIELD => (o.fields, 0),
                        _ => record(w.sim, w.m, lib, b, kind),
                    }
                } else {
                    record(w.sim, w.m, lib, b, kind)
                };
                made = made.wrapping_add(a);
                under = under.wrapping_add(u);
            }
            match lib.category(kind) {
                1 | 3 => made >= n,
                2 => made.wrapping_sub(under) >= n,
                _ => false,
            }
        }
    }
}

/// 0x408270: the rule's goal is reached (no goal, or enough of it, sites
/// left out); the count is the one written, so a `%` never is.
fn satisfied(w: &World, p: &Player, k: u8, r: usize) -> bool {
    let Some(have) = p
        .program
        .as_ref()
        .and_then(|x| x.rules.get(r))
        .and_then(|x| x.have)
    else {
        return true;
    };
    if have.kind == 0 {
        return true;
    }
    let lib = &w.c.ai;
    let (mut made, under) = count(p, w.sim, w.m, lib, k, have.kind);
    if lib.category(have.kind) == 2 {
        made = made.saturating_sub(under);
    }
    made >= have.count as u32
}
/// 0x408220: the goal is under way (enough of it, sites included).
fn under_way(w: &World, p: &Player, k: u8, r: usize) -> bool {
    let Some(have) = p
        .program
        .as_ref()
        .and_then(|x| x.rules.get(r))
        .and_then(|x| x.have)
        .filter(|h| h.kind != 0)
    else {
        return false;
    };
    count(p, w.sim, w.m, &w.c.ai, k, have.kind).0 >= have.count as u32
}

/// 0x408420: the rule evaluated again.
pub(super) fn evaluate(
    w: &mut World,
    p: &mut Player,
    k: u8,
    others: &BTreeMap<u8, Other>,
    r: usize,
) {
    let Some(rule) = p.program.as_ref().and_then(|x| x.rules.get(r)).cloned() else {
        return;
    };
    let old = rule.flags;
    let mut flags = (old & !flag::DIRTY) | flag::TRUE;
    set_flags(p, r, flags);
    let when: Vec<u32> = p
        .program
        .as_ref()
        .map_or(Vec::new(), |x| x.entries(rule.when).to_vec());
    for n in when {
        if !holds(w, p, k, others, n) {
            flags &= !flag::TRUE;
            break;
        }
    }
    if flags & flag::TRUE != 0 {
        if satisfied(w, p, k, r) {
            flags = (flags | flag::SATISFIED) & !flag::IN_PROGRESS;
        } else {
            flags &= !flag::SATISFIED;
            if under_way(w, p, k, r) {
                flags |= flag::IN_PROGRESS;
            } else {
                flags &= !flag::IN_PROGRESS;
            }
        }
    } else {
        flags &= !(flag::SATISFIED | flag::IN_PROGRESS);
    }
    set_flags(p, r, flags);
    if flags & flag::SATISFIED != 0 {
        // The switches are set at every evaluation that finds it satisfied.
        let switches: Vec<u32> = p
            .program
            .as_ref()
            .map_or(Vec::new(), |x| x.entries(rule.triggers).to_vec());
        for t in switches {
            let on = t & 0x8000_0000 == 0;
            let t = t & 0x7fff_ffff;
            let now = p
                .program
                .as_ref()
                .and_then(|x| x.triggers.get(t as usize))
                .is_some_and(|x| x.on);
            if now != on {
                p.touch(Watched::Trigger(t));
                if let Some(x) = p
                    .program
                    .as_mut()
                    .and_then(|x| x.triggers.get_mut(t as usize))
                {
                    x.on = on;
                }
            }
        }
    }
    if (flags ^ old) & flag::SATISFIED == 0 {
        return;
    }
    p.touch(Watched::Rule(r as u32));
    if flags & flag::SATISFIED == 0 {
        return;
    }
    let forget: Vec<u32> = p
        .program
        .as_ref()
        .map_or(Vec::new(), |x| x.entries(rule.forget).to_vec());
    for f in forget {
        if let Some(x) = p.program.as_mut().and_then(|x| x.rules.get_mut(f as usize)) {
            x.flags |= flag::FORGOTTEN;
        }
    }
    let Some(text) = rule.say else {
        return;
    };
    if rule.have.is_some_and(|h| h.kind != 0) && old & flag::IN_PROGRESS == 0 {
        return;
    }
    if let Some(command) = text.strip_prefix("HMAI ") {
        // An AI command of the script (0x404460).
        let line = super::command(w.sim, w.m, k, &command.trim().to_ascii_lowercase());
        if let Some(show) = line {
            let outbox = &mut w.sim.gameplay.script.outbox;
            if outbox.len() < 1024 {
                outbox.push(show);
            }
        }
        return;
    }
    if p.mode & super::TALK != 0 {
        let s = w.sim.ticks * TICK_MS / 1000;
        let name =
            w.m.kingdoms
                .get((k as usize).wrapping_sub(1))
                .map_or(String::new(), |x| x.name.clone());
        let line = format!(
            "{name}: [{:02}:{:02}:{:02}]: {text}",
            s / 3600,
            s / 60 % 60,
            s % 60
        );
        let outbox = &mut w.sim.gameplay.script.outbox;
        if outbox.len() < 1024 {
            outbox.push(Show::Line(line));
        }
    }
}
fn set_flags(p: &mut Player, r: usize, flags: u32) {
    if let Some(x) = p.program.as_mut().and_then(|x| x.rules.get_mut(r)) {
        x.flags = flags;
    }
}

/// 0x40af70: what one of a goal costs: an object's or a research's price
/// for the player's civilization; one of a resource; ten food a unit of
/// `CONVERT`.
pub(super) fn cost(w: &World, p: &Player, kind: u16) -> [i32; 4] {
    match kind {
        goal::CONVERT => [0, 0, 10, 0],
        goal::GOLD => [1, 0, 0, 0],
        goal::STONE => [0, 1, 0, 0],
        goal::FOOD => [0, 0, 1, 0],
        goal::WOOD => [0, 0, 0, 1],
        goal::NEEDS_LOCKED | goal::RESIGN => [0; 4],
        _ => {
            let Some(name) = w.c.ai.name(kind) else {
                return [0; 4];
            };
            let probe = super::tree::probe(0, p.civilization);
            w.c.stats(&probe, name, researches(w.m, p.kingdom))
                .cost
                .map(|n| n.min(i32::MAX as u32) as i32)
        }
    }
}
/// 0x40b040: `n` of a cost booked by a part.
pub(super) fn book(p: &mut Player, part: usize, cost: [i32; 4], n: u32) {
    for (b, c) in p.buckets[part].iter_mut().zip(cost) {
        *b = b.wrapping_add(c.wrapping_mul(n as i32));
    }
}
/// 0x40b0f0 and 0x40b130: a cost committed (a site still to be laid, the
/// reserve of the start) or given back.
pub(super) fn commit(p: &mut Player, cost: [i32; 4]) {
    for (r, c) in cost.iter().enumerate() {
        p.available[r] = p.available[r].wrapping_sub(*c);
        p.committed[r] = p.committed[r].wrapping_add(*c);
    }
}
pub(super) fn release(p: &mut Player, cost: [i32; 4]) {
    for (r, c) in cost.iter().enumerate() {
        p.committed[r] = p.committed[r].wrapping_sub(*c);
        p.available[r] = p.available[r].wrapping_add(*c);
    }
}
/// 0x40b090: the kingdom's resources less what the player committed.
pub(super) fn spendable(m: &MissionWorld, p: &Player) -> [i32; 4] {
    let have = resources_of(m, p.kingdom);
    std::array::from_fn(|r| (have[r].min(i32::MAX as u32) as i32).wrapping_sub(p.committed[r]))
}
/// 0x40b170: `n` of a cost can be paid by a part, after what the parts
/// before it booked. The parts of ships and defences wait while
/// `NEEDS_LOCKED` holds. The first castle or lumber mill also spends what
/// the start committed for it.
pub(super) fn affordable(
    w: &World,
    p: &Player,
    cost: [i32; 4],
    part: usize,
    n: u32,
    first: bool,
) -> bool {
    if (part == bucket::NAVY || part == bucket::DEFENCE) && p.locked {
        return false;
    }
    let first = first
        && record(w.sim, w.m, &w.c.ai, p.kingdom, class::LUMBER as u16).0 == 0
        && record(w.sim, w.m, &w.c.ai, p.kingdom, class::CASTLE as u16).0 == 0;
    let mut before = [0i32; 4];
    let parts = if part == bucket::NONE { 0 } else { part.min(5) };
    for b in &p.buckets[..parts] {
        for r in 0..4 {
            before[r] = before[r].wrapping_add(b[r]);
        }
    }
    (0..4).all(|r| {
        if cost[r] == 0 {
            return true;
        }
        let need = cost[r].wrapping_mul(n as i32).wrapping_add(before[r]);
        let have = if first {
            p.committed[r].wrapping_add(p.available[r])
        } else {
            p.available[r]
        };
        have >= need
    })
}
/// 0x40fb00 and 0x40fb60: with neither lumber mill nor castle, the price of
/// a lumber mill stays committed for it.
pub(super) fn start_reserve(w: &World, p: &mut Player, on: bool) {
    let lib = &w.c.ai;
    if record(w.sim, w.m, lib, p.kingdom, class::LUMBER as u16).0 != 0
        || record(w.sim, w.m, lib, p.kingdom, class::CASTLE as u16).0 != 0
    {
        return;
    }
    let c = cost(w, p, class::LUMBER as u16);
    if on { commit(p, c) } else { release(p, c) }
}

/// 0x407d50: the goal can be pursued: a resource goal always; an object
/// type in the kingdom's tree with what it needs.
pub(super) fn available(w: &World, p: &Player, kind: u16) -> bool {
    if kind >= 0x8000 {
        return true;
    }
    if kind == 0 {
        return false;
    }
    match w.c.ai.name(kind) {
        Some(name) => {
            super::tree::available(w.sim, w.m, w.c, w.lib, p.kingdom, p.civilization, name)
        }
        None => false,
    }
}

/// 0x408960: the Build phase. The resources it may spend are read again,
/// the script's part of them cleared, and the rules run in order; the
/// first goal it can pay for is carried out, and a building ends the
/// phase. A critical rule that stops stops the phase, or its group.
pub(super) fn build(w: &mut World, p: &mut Player, k: u8, others: &BTreeMap<u8, Other>) {
    w.sim.gameplay.script.variables.set("$AIK", k as i32);
    let cap = limit(w.m, w.lib, k);
    if cap != p.cap as u32 {
        p.cap = cap as u16;
        p.touch(Watched::Percent);
    }
    let now = spendable(w.m, p);
    for (r, v) in now.iter().enumerate() {
        if *v != p.available[r] {
            p.touch(Watched::Resource(r));
        }
    }
    p.available = now;
    // 0x408a22 clears the script's part, 0x14 bytes: the lock of
    // `NEEDS_LOCKED` (IA+0x988) is its fifth word.
    p.buckets[bucket::SCRIPT] = [0; 4];
    p.locked = false;
    p.touch(Watched::Poll);
    if w.m.game_rules.ai_resign {
        resignation(p);
    }
    let Some(count) = p.program.as_ref().map(|x| x.rules.len()) else {
        return;
    };
    let mut groups: Vec<u32> = Vec::new();
    let mut r = 0usize;
    p.building = true;
    'rules: while r < count {
        let Some(rule) = p.program.as_ref().map(|x| x.rules[r].clone()) else {
            break;
        };
        let mut stop = false;
        if rule.flags & flag::GROUP_START != 0 {
            groups.push(rule.end);
        } else if rule.flags & flag::GROUP_END != 0 {
            groups.pop();
        } else if rule.flags & flag::FORGOTTEN == 0 {
            if rule.flags & (flag::DIRTY | flag::IN_PROGRESS) != 0 {
                evaluate(w, p, k, others, r);
            }
            let flags = p.program.as_ref().map_or(0, |x| x.rules[r].flags);
            if flags & flag::SATISFIED == 0 {
                stop = true;
                if flags & flag::TRUE != 0 && flags & flag::IN_PROGRESS == 0 {
                    let have = rule.have.unwrap_or_default();
                    if available(w, p, have.kind) {
                        let made = count_of(w, p, k, have.kind);
                        let want = wanted(have.count as u32, limit(w.m, w.lib, k));
                        let one = cost(w, p, have.kind);
                        let mut part = bucket::SCRIPT;
                        let skip = if have.kind as usize == class::PEON
                            || have.kind as usize == class::HOUSE
                        {
                            part = bucket::WORKERS;
                            want == 0 || want <= made
                        } else {
                            want <= made
                        };
                        if skip {
                            stop = false;
                        } else {
                            book(p, part, one, want - made);
                            let first = matches!(have.kind as usize, class::CASTLE | class::LUMBER);
                            if affordable(w, p, one, part, 1, first)
                                && execute(w, p, k, have.kind, want)
                            {
                                stop = false;
                                if let Some(x) = p.program.as_mut() {
                                    x.rules[r].flags |= flag::IN_PROGRESS;
                                }
                                if w.c.ai.category(have.kind) == 2 {
                                    break 'rules;
                                }
                            }
                        }
                    }
                }
            }
        }
        if stop && rule.flags & flag::CRITICAL != 0 {
            match groups.pop() {
                Some(end) if end != 0 => r = end as usize,
                _ => break,
            }
        }
        r += 1;
    }
    p.building = false;
}
/// The decided difference of the resignation (`resign.ai`): its switches
/// are reset by rules without conditions, which the original reads once at
/// the start, so a kingdom once with a castle never resigns. With the
/// computer allowed to resign, every rule of the group holding `HAVE
/// RESIGN` is read again at each Build phase, in order.
fn resignation(p: &mut Player) {
    let Some(x) = p.program.as_mut() else {
        return;
    };
    let mut open = Vec::new();
    let mut spans = Vec::new();
    for (r, rule) in x.rules.iter().enumerate() {
        if rule.flags & flag::GROUP_START != 0 {
            open.push(r);
        } else if rule.flags & flag::GROUP_END != 0 {
            open.pop();
        } else if rule.have.is_some_and(|h| h.kind == goal::RESIGN)
            && let Some(&start) = open.last()
        {
            spans.push((start, x.rules[start].end as usize));
        }
    }
    for (start, end) in spans {
        for rule in x.rules.iter_mut().take(end + 1).skip(start) {
            rule.flags |= flag::DIRTY;
        }
    }
}
/// The Build phase's count of a goal: 0x407da0 without what is under way.
fn count_of(w: &World, p: &Player, k: u8, kind: u16) -> u32 {
    count(p, w.sim, w.m, &w.c.ai, k, kind).0
}

/// 0x409550: a goal carried out; whether it was.
pub(super) fn execute(w: &mut World, p: &mut Player, k: u8, kind: u16, want: u32) -> bool {
    if !available(w, p, kind) {
        return false;
    }
    match kind {
        // 0x4096d1: when the game lets the computer resign (0x5cf6d4), the
        // command 0x43 (0x490e30) marks the kingdom resigned without a word
        // (0x48fc8d) and tests the end of the game (0x48fd06) with the
        // step's.
        goal::RESIGN => {
            if w.m.game_rules.ai_resign
                && let Some(s) = w.sim.gameplay.script.kingdoms.get_mut(k as usize)
            {
                s.resigned = true;
                w.sim.gameplay.script.end_due = true;
            }
            false
        }
        goal::NEEDS_LOCKED => {
            for part in [
                bucket::NAVY,
                bucket::STORES,
                bucket::WORKERS,
                bucket::DEFENCE,
            ] {
                p.buckets[part] = [0; 4];
            }
            p.locked = true;
            false
        }
        goal::GOLD | goal::STONE | goal::FOOD | goal::WOOD => false,
        goal::CONVERT => {
            convert(w, p, k, want);
            false
        }
        super::goals::JIHAD_WARRIOR => super::goals::jihad(w, p),
        super::goals::CRUSADE => super::goals::crusade(w, p),
        _ => match w.c.ai.category(kind) {
            1 => {
                let producer = super::tree::producer(w.c, w.lib, p.civilization, kind as usize);
                if producer == Some(class::MAGE) {
                    return super::goals::summon(w, p, kind);
                }
                let Some(at) = producer.and_then(|c| idle_producer(w, p, k, c)) else {
                    return false;
                };
                let one = cost(w, p, kind);
                pay(w, p, one);
                super::produce(w.sim, w.m, w.c, at, JobKind::Train(kind as usize), one);
                true
            }
            3 => {
                let producer = super::tree::producer(w.c, w.lib, p.civilization, kind as usize);
                let Some(at) = producer.and_then(|c| idle_producer(w, p, k, c)) else {
                    return false;
                };
                let one = cost(w, p, kind);
                pay(w, p, one);
                let name = w.c.ai.name(kind).unwrap_or_default().to_owned();
                super::produce(w.sim, w.m, w.c, at, JobKind::Research(name), one);
                true
            }
            2 => {
                let home = p.home.unwrap_or_default();
                let (center, min, max) = match kind as usize {
                    class::LUMBER => (home, 10, 50),
                    // Near the first mine of the list, else as a lumber mill.
                    class::STORAGE => match p.lists.mines.first() {
                        Some(e) => {
                            let o = &w.m.objects[e.object];
                            ([o.x as i32, o.y as i32], 1, 8)
                        }
                        None => (home, 10, 50),
                    },
                    class::FIELD => (home, 0, 50),
                    _ => (home, 5, 50),
                };
                p.place(w, kind, center, min, max, None, false).is_some()
            }
            _ => false,
        },
    }
}
/// 0x409941: a unit or research paid at once, from what the player may
/// spend and from the kingdom with what is shared with it (0x467790).
pub(super) fn pay(w: &mut World, p: &mut Player, cost: [i32; 4]) {
    for (a, c) in p.available.iter_mut().zip(cost) {
        *a = a.wrapping_sub(c);
    }
    crate::gameplay::spend(w.m, p.kingdom, cost.map(|c| c.max(0) as u32));
}
/// 0x40d560: of the kingdom's buildings of the producing type that are
/// finished and idle, the one whose places most lack defence (0x40d4a0),
/// the newest first on ties.
fn idle_producer(w: &World, p: &Player, k: u8, class: usize) -> Option<usize> {
    let mut best: Option<(u32, usize)> = None;
    for i in super::think::kingdom_list(w.sim, w.m, k, false) {
        let o = &w.m.objects[i];
        if o.class != class
            || o.flags & (FLAG_STORY | FLAG_DYING) != 0
            || o.flags & FLAG_CONSTRUCTION != 0
            || !idle_building(w.sim, i)
        {
            continue;
        }
        let score = uncovered(w, p, i);
        if best.is_none_or(|(s, _)| s < score) {
            best = Some((score, i));
        }
    }
    best.map(|(_, i)| i)
}
/// A building without a program (+0x82): nothing to make or exchange.
pub(super) fn idle_building(sim: &Simulation, i: usize) -> bool {
    let a = &sim.gameplay.actors[i];
    a.jobs.is_empty() && a.exchange.is_none() && a.intent.is_none()
}
/// 0x40d4a0: over the places of the building's sector, how far the
/// player's present fighters fall short of each minimum.
fn uncovered(w: &World, p: &Player, i: usize) -> u32 {
    let Some(mgr) = w.sim.gameplay.ai.manager.as_ref() else {
        return 0;
    };
    let Some(s) = w.sim.gameplay.actors[i].sector else {
        return 0;
    };
    let mut sum = 0u32;
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
                sum = sum.wrapping_add(r.minimum - have);
            }
        }
    }
    sum
}
/// 0x409771: `CONVERT` with no exchange under way: the first tavern of the
/// kingdom trades ten food for each unit wanted into the resource the
/// parts lack most, gold on ties.
fn convert(w: &mut World, p: &Player, k: u8, want: u32) {
    if exchanging(w.sim, w.m, k) {
        return;
    }
    let Some(tavern) = super::think::kingdom_list(w.sim, w.m, k, false)
        .into_iter()
        .find(|&i| w.m.objects[i].class == class::TAVERN)
    else {
        return;
    };
    let short = |r: usize| {
        let booked: i32 = p.buckets.iter().map(|b| b[r]).fold(0, i32::wrapping_add);
        if booked > p.available[r] {
            booked - p.available[r]
        } else {
            0
        }
    };
    let to = u8::from(short(1) > short(0));
    let paid = want.wrapping_mul(10);
    let mut cost = [0; 4];
    cost[2] = paid;
    if debit(w.c, w.m, k, cost).is_err() {
        return;
    }
    w.sim.gameplay.actors[tavern].exchange = Some(Exchange {
        from: 2,
        to,
        paid,
        total: want,
        delivered: 0,
        next: w.sim.ticks + 6,
    });
}
