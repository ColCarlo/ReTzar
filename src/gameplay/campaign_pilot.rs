//! The campaign pilot: every mission of a campaign played to its victory by
//! working back from the rules that win it or open the next map. A rule's
//! condition is probed ([`scripting::probe`]) for the fewest changes that
//! would make it hold: events true or false, variables, resources. A
//! variable is brought by the rule, or the line of a conversation, that
//! sets it; an event by the orders a player gives: heroes walked into an
//! area or a building, the army sent at a target, a tribute. What the
//! orders do not bring in a while is forced and listed: a walk set down (by
//! ship where the place is sea), a target struck down, a building raised, a
//! wound, a kingdom swept. The player's units and buildings cannot be hurt,
//! so the losses a script counts on are forced from the expendable ones,
//! and the enemies about to bring a defeat are stopped. The pilot shows
//! that the scripts lead to victory on the real maps, not that the battles
//! can be won.
//!
//! With `RETZAR_PILOT_SAVES` naming the folder of a portable copy, a game
//! is saved shortly before each victory, to watch its end there.

use super::*;
use crate::gameplay::scripting::{Suppose, probe};
use crate::scenario::{Scenario, ScenarioCatalog};
use crate::script::{Action, Event, Selector, expr::GLOBALS};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A change the pilot may bring about.
#[derive(Clone, Debug, PartialEq)]
enum Atom {
    /// A named event of the map holds.
    Event(String),
    /// A named event the condition negates stops holding.
    Not(String),
    /// A variable takes a value.
    Var(String, i32),
    /// The local kingdom has plenty of every resource.
    Rich,
    /// Another kingdom has plenty of a resource (index, kingdom): a
    /// tribute of the player's.
    Tribute(u8, u8),
}

/// A source of a variable's value.
enum Source {
    /// A rule's actions set it.
    Rule(usize),
    /// A line of a conversation sets it: the conversation and the line.
    Line(usize, usize),
}

struct Pilot {
    p: Play,
    scenario: Scenario,
    campaign: usize,
    mission: usize,
    /// What was forced, in order.
    forced: Vec<String>,
    /// When the orders toward an event give up, by its name.
    tries: BTreeMap<String, u64>,
    /// When orders were last given toward an event.
    ordered: BTreeMap<String, u64>,
    /// Copies of the game every ten seconds, the last few.
    ring: VecDeque<(u64, Play)>,
    rich: bool,
    /// Why the pilot gave up.
    stuck: Option<String>,
    /// The heroes the pilot made unhurtable, which a save gives back.
    godded: BTreeSet<u16>,
    /// The answers of `spot`, by object, expression, answer wanted and sea,
    /// with where the object stood.
    spots: BTreeMap<(usize, String, bool, bool), ([u16; 2], Option<[u16; 2]>)>,
    /// The tributes sent: resource and kingdom.
    tributes: BTreeSet<(u8, u8)>,
}

/// The longest a mission may take, in seconds of play.
const BUDGET: u64 = 2 * 3600;
/// A mission where no rule fires for this long has stalled.
const STALL: u64 = 900;

impl Pilot {
    fn new(catalog: &ScenarioCatalog, campaign: usize, mission: usize, p: Play) -> Pilot {
        Pilot {
            p,
            scenario: catalog.campaigns[campaign].missions[mission].clone(),
            campaign,
            mission,
            forced: Vec::new(),
            tries: BTreeMap::new(),
            ordered: BTreeMap::new(),
            ring: VecDeque::new(),
            rich: false,
            stuck: None,
            godded: BTreeSet::new(),
            spots: BTreeMap::new(),
            tributes: BTreeSet::new(),
        }
    }
    fn now(&self) -> u64 {
        self.p.sim.ticks * TICK_MS / 1000
    }
    fn player(&self) -> u8 {
        self.p.m.player()
    }
    fn outcome(&self) -> Option<bool> {
        self.p.sim.gameplay.script.over.map(|o| o.0)
    }
    fn trace_len(&self) -> usize {
        self.p
            .sim
            .gameplay
            .script
            .trace
            .0
            .as_ref()
            .map_or(0, |t| t.len())
    }
    fn probe(&mut self, text: &str, suppose: &Suppose) -> i32 {
        let p = &mut self.p;
        probe(&mut p.sim, &mut p.m, &p.g, &p.lib, &p.c, text, suppose)
    }
    fn holds(&mut self, text: &str) -> bool {
        self.probe(text, &Suppose::default()) != 0
    }

    /// Plays the mission to its end; the victory, or what went wrong.
    fn fly(&mut self) -> std::result::Result<(), String> {
        let start = self.now();
        let mut progress = (self.trace_len() + self.forced.len(), start);
        let mut doing = String::new();
        loop {
            if let Some(won) = self.outcome() {
                // The end window opens 5 s later (0x442af0): a map a script
                // loads meanwhile gives a defeat «CONTINUA» too (0x442861).
                if !won {
                    let at = self.p.sim.gameplay.script.over.map_or(0, |o| o.1);
                    while self.p.sim.ticks * TICK_MS < at + END_DELAY_MS {
                        self.p.second();
                    }
                }
                return if won || self.p.sim.gameplay.script.next_map.is_some() {
                    Ok(())
                } else {
                    Err(format!("persa ({doing}){}", self.p.trace_tail()))
                };
            }
            let now = self.now();
            if now - start > BUDGET {
                return Err(format!("tempo scaduto ({doing}){}", self.p.trace_tail()));
            }
            if now - progress.1 > STALL {
                return Err(format!("ferma ({doing}){}", self.p.trace_tail()));
            }
            self.protect();
            self.guard();
            if self.ring.back().is_none_or(|(t, _)| now >= t + 10) {
                self.ring.push_back((now, self.p.clone()));
                if self.ring.len() > 6 {
                    self.ring.pop_front();
                }
            }
            doing = self.work();
            if let Some(why) = self.stuck.take() {
                return Err(format!("{why}{}", self.p.trace_tail()));
            }
            for _ in 0..2 {
                self.p.second();
                self.spare();
                if self.outcome().is_some() {
                    break;
                }
            }
            let n = self.trace_len() + self.forced.len();
            if n != progress.0 {
                progress = (n, self.now());
            }
        }
    }

    /// The player's units and buildings cannot be hurt (the «god» cheat's
    /// flag) and the heroes stay whole, so that the pilot's walks and the
    /// battles it does not fight do not end the mission; a defeat the
    /// script wants is forced.
    fn protect(&mut self) {
        // The player's own and those of the kingdoms that share their units.
        let mine: Vec<bool> = (0..=8u8).map(|k| k != 0 && self.p.m.controls(k)).collect();
        for o in &mut self.p.m.objects {
            if mine.get(o.owner as usize) == Some(&true)
                && alive(o)
                && (crate::selection::unit(o) || o.building())
            {
                if o.flags & FLAG_GOD == 0 {
                    o.flags |= FLAG_GOD;
                    self.godded.insert(o.id);
                }
                if !o.name.is_empty() {
                    o.health = o.health.max(o.max_health);
                }
            }
        }
    }
    /// A wound the script waits for ends the fight with its object, as a
    /// player stops striking someone the story still needs: the attacks on
    /// it called off, and it kept from further harm.
    fn spare(&mut self) {
        let events: Vec<(Selector, String)> = self
            .p
            .m
            .script
            .events
            .iter()
            .filter_map(|e| match &e.value {
                Event::Expression(s, x) if wounds(x) => Some((s.clone(), e.name.clone())),
                _ => None,
            })
            .collect();
        for (s, name) in events {
            if !self.holds(&name) {
                continue;
            }
            let spared: Vec<usize> = self
                .listed(&s)
                .into_iter()
                .filter(|&i| !self.p.m.controls(self.p.m.objects[i].owner))
                .collect();
            if spared.is_empty() {
                continue;
            }
            for &i in &spared {
                let o = &mut self.p.m.objects[i];
                if o.flags & FLAG_GOD == 0 {
                    o.flags |= FLAG_GOD;
                    self.godded.insert(o.id);
                }
            }
            for i in self.army() {
                if matches!(self.p.sim.gameplay.actors[i].intent, Some(Intent::Attack(t)) if spared.contains(&t))
                {
                    let p = &mut self.p;
                    issue(
                        &mut p.sim,
                        &mut p.m,
                        &p.g,
                        &p.lib,
                        &p.c,
                        &Request::command("stop", vec![i]),
                    )
                    .ok();
                }
            }
        }
    }
    /// The enemies about to bring about a rule that loses the mission, as
    /// a player would stop them: those within a few cells of the area an
    /// event of the rule waits for them in, struck down.
    fn guard(&mut self) {
        let s = self.p.m.script.clone();
        let bit = 1u16 << (self.player() - 1);
        let loses = |r: usize| {
            s.rules[r]
                .actions
                .iter()
                .map_while(|a| s.action(a))
                .any(|i| match &s.actions[i].value {
                    Action::Lose(mask, _) => mask & bit != 0,
                    Action::Win(mask, _) => *mask != 0 && mask & bit == 0,
                    _ => false,
                })
        };
        let rules: Vec<usize> = self
            .p
            .sim
            .gameplay
            .script
            .rules
            .iter()
            .filter(|run| run.cursor.is_none() && loses(run.rule))
            .map(|run| run.rule)
            .collect();
        for r in rules {
            let text = s.rules[r].condition.clone();
            for name in crate::script::audit::symbols(&text) {
                let Some(Event::InArea(sel, a)) = s
                    .events
                    .iter()
                    .find(|e| e.name == name)
                    .map(|e| e.value.clone())
                else {
                    continue;
                };
                let one = suppose(&[&Atom::Event(name.clone())]);
                if self.holds(&name) || self.probe(&text, &one) == 0 {
                    continue;
                }
                let Some(r) = self.rect(&a) else { continue };
                let near: Vec<usize> = self
                    .listed(&sel)
                    .into_iter()
                    .filter(|&i| {
                        let o = &self.p.m.objects[i];
                        let (x, y) = (o.x as i32, o.y as i32);
                        !self.p.m.controls(o.owner)
                            && self.present(i)
                            && x >= r[0] - 12
                            && x <= r[2] + 12
                            && y >= r[1] - 12
                            && y <= r[3] + 12
                    })
                    .collect();
                if !near.is_empty() {
                    self.strike(&near);
                    self.forced
                        .push(format!("fermati {} («{name}»)", near.len()));
                }
            }
        }
    }

    /// The rules still to run that win the mission or open the next map.
    fn goals(&self) -> (Vec<usize>, bool) {
        let s = &self.p.m.script;
        let st = &self.p.sim.gameplay.script;
        let bit = 1u16 << (self.player() - 1);
        let wins = |r: usize| {
            s.rules[r]
                .actions
                .iter()
                .map_while(|a| s.action(a))
                .any(|i| match &s.actions[i].value {
                    Action::LoadMap(_) => true,
                    Action::Win(mask, _) => mask & bit != 0,
                    Action::Lose(mask, _) => *mask != 0 && mask & bit == 0,
                    _ => false,
                })
        };
        let running = st
            .rules
            .iter()
            .any(|run| run.cursor.is_some() && wins(run.rule));
        let mut goals: Vec<usize> = st
            .rules
            .iter()
            .filter(|run| run.cursor.is_none())
            .map(|run| run.rule)
            .filter(|&r| wins(r))
            .collect();
        // The rules that open the next map first: the campaign goes on
        // by them.
        let loads = |r: usize| {
            s.rules[r]
                .actions
                .iter()
                .map_while(|a| s.action(a))
                .any(|i| matches!(s.actions[i].value, Action::LoadMap(_)))
        };
        goals.sort_by_key(|&r| (!loads(r), r));
        goals.dedup();
        (goals, running)
    }

    /// One step toward the victory; what it is doing.
    fn work(&mut self) -> String {
        let (goals, running) = self.goals();
        if running {
            return "una regola di vittoria è in corso".into();
        }
        if goals.is_empty() {
            return self.sweep();
        }
        for r in goals {
            let rule = self.p.m.script.rules[r].clone();
            match self.plan(&rule.condition) {
                Some(plan) if plan.is_empty() => {
                    return format!("regola «{}» pronta", rule.name);
                }
                Some(plan) => {
                    let mut seen = BTreeSet::from([r]);
                    let step = self.achieve(&plan, 0, &mut seen);
                    return format!("regola «{}»: {step}", rule.name);
                }
                None => {}
            }
        }
        "nessuna regola di vittoria raggiungibile".into()
    }

    /// The fewest changes, at most three, that make `text` hold; none when
    /// it holds already.
    fn plan(&mut self, text: &str) -> Option<Vec<Atom>> {
        if self.holds(text) {
            return Some(Vec::new());
        }
        let atoms = self.atoms(text);
        for size in 1..=3.min(atoms.len()) {
            let mut pick: Vec<usize> = (0..size).collect();
            loop {
                let combo: Vec<&Atom> = pick.iter().map(|&i| &atoms[i]).collect();
                let names: BTreeSet<&str> = combo
                    .iter()
                    .filter_map(|a| match a {
                        Atom::Var(n, _) => Some(n.as_str()),
                        _ => None,
                    })
                    .collect();
                let vars = combo.iter().filter(|a| matches!(a, Atom::Var(..))).count();
                if names.len() == vars {
                    let suppose = suppose(&combo);
                    if self.probe(text, &suppose) != 0 {
                        return Some(combo.into_iter().cloned().collect());
                    }
                }
                // The next combination of `size` among the atoms.
                let mut k = size;
                loop {
                    if k == 0 {
                        break;
                    }
                    k -= 1;
                    if pick[k] < atoms.len() - size + k {
                        pick[k] += 1;
                        for j in k + 1..size {
                            pick[j] = pick[j - 1] + 1;
                        }
                        k = usize::MAX;
                        break;
                    }
                }
                if k != usize::MAX {
                    break;
                }
            }
        }
        self.greedy(text, &atoms)
    }
    /// More than three changes: every event and one value of each variable,
    /// tried value by value, then each change left out while the rest still
    /// makes `text` hold.
    fn greedy(&mut self, text: &str, atoms: &[Atom]) -> Option<Vec<Atom>> {
        let mut values: BTreeMap<&str, Vec<i32>> = BTreeMap::new();
        for a in atoms {
            if let Atom::Var(n, v) = a {
                values.entry(n).or_default().push(*v);
            }
        }
        let fixed: Vec<Atom> = atoms
            .iter()
            .filter(|a| !matches!(a, Atom::Var(..)))
            .cloned()
            .collect();
        let pick: BTreeMap<&str, i32> = values.iter().map(|(&n, v)| (n, v[0])).collect();
        let with = |pick: &BTreeMap<&str, i32>| -> Vec<Atom> {
            let mut out = fixed.clone();
            out.extend(pick.iter().map(|(&n, &v)| Atom::Var(n.to_owned(), v)));
            out
        };
        let holds = |me: &mut Pilot, set: &[Atom]| {
            let s = suppose(&set.iter().collect::<Vec<_>>());
            me.probe(text, &s) != 0
        };
        let mut set = with(&pick);
        if !holds(self, &set) {
            let found = values.iter().find_map(|(&n, vs)| {
                vs.iter().find_map(|&v| {
                    let mut p = pick.clone();
                    p.insert(n, v);
                    let s = with(&p);
                    holds(self, &s).then_some(s)
                })
            });
            set = found?;
        }
        let mut k = 0;
        while k < set.len() {
            let mut fewer = set.clone();
            fewer.remove(k);
            if holds(self, &fewer) {
                set = fewer;
            } else {
                k += 1;
            }
        }
        Some(set)
    }

    /// The changes a condition may want: its events that do not hold, its
    /// variables at the values the script gives them, the resources.
    fn atoms(&mut self, text: &str) -> Vec<Atom> {
        let symbols = crate::script::audit::symbols(text);
        let literals: BTreeSet<i32> = symbols
            .iter()
            .filter_map(|s| s.parse::<i32>().ok())
            .collect();
        let mut out = Vec::new();
        let player = self.player();
        for (r, k) in stocks(text) {
            if k != player && (1..=8).contains(&k) && !out.contains(&Atom::Tribute(r, k)) {
                out.push(Atom::Tribute(r, k));
            }
        }
        for s in &symbols {
            let lower = s.to_ascii_lowercase();
            if s.as_bytes()[0].is_ascii_digit() {
                continue;
            }
            if s.starts_with('@') {
                if ["@gold", "@stone", "@food", "@wood"]
                    .iter()
                    .any(|f| lower.starts_with(f))
                    && !out.contains(&Atom::Rich)
                {
                    out.push(Atom::Rich);
                }
                continue;
            }
            if GLOBALS.iter().any(|(n, _)| n.eq_ignore_ascii_case(s)) {
                if !out.contains(&Atom::Rich) {
                    out.push(Atom::Rich);
                }
                continue;
            }
            if self.p.m.script.events.iter().any(|e| e.name == *s) {
                let atom = if !self.holds(s) {
                    Atom::Event(s.clone())
                } else if negated(text, s) {
                    Atom::Not(s.clone())
                } else {
                    continue;
                };
                if !out.contains(&atom) {
                    out.push(atom);
                }
                continue;
            }
            let now = self.p.sim.gameplay.script.variables.get(s);
            let mut values: BTreeSet<i32> = literals.clone();
            values.extend(self.assigned(s));
            values.insert(1);
            for v in values {
                let atom = Atom::Var(s.clone(), v);
                if v != now && !out.contains(&atom) {
                    out.push(atom);
                }
            }
        }
        out.truncate(24);
        out
    }

    /// The constant values the script and its conversations give `name`.
    fn assigned(&self, name: &str) -> Vec<i32> {
        let mut out = Vec::new();
        for text in self.statements() {
            for (lhs, rhs) in assignments(&text) {
                if lhs == name
                    && let Ok(v) = rhs.parse::<i32>()
                {
                    out.push(v);
                }
            }
        }
        out
    }
    /// The statements of the script's expressions and of its lines.
    fn statements(&self) -> Vec<String> {
        let s = &self.p.m.script;
        s.actions
            .iter()
            .filter_map(|a| match &a.value {
                Action::Expression(t) => Some(t.clone()),
                _ => None,
            })
            .chain(
                self.p
                    .m
                    .conversations
                    .iter()
                    .flat_map(|c| c.phrases.iter().map(|ph| ph.action.clone())),
            )
            .collect()
    }

    /// Where `name` gets `v`: the rules that set it, then the lines.
    fn sources(&self, name: &str, v: i32) -> Vec<Source> {
        let s = &self.p.m.script;
        let gives = |text: &str| {
            assignments(text).iter().any(|(lhs, rhs)| {
                lhs == name
                    && (rhs.parse::<i32>() == Ok(v)
                        || rhs.replace(' ', "").starts_with(&format!("{name}+")))
            })
        };
        let mut out: Vec<Source> = (0..s.rules.len())
            .filter(|&r| {
                s.rules[r]
                    .actions
                    .iter()
                    .map_while(|a| s.action(a))
                    .any(|i| match &s.actions[i].value {
                        Action::Expression(t) => gives(t),
                        Action::SetTimer { .. } => s.actions[i].name == name,
                        _ => false,
                    })
            })
            .map(Source::Rule)
            .collect();
        for (c, talk) in self.p.m.conversations.iter().enumerate() {
            for (k, ph) in talk.phrases.iter().enumerate() {
                if gives(&ph.action) {
                    out.push(Source::Line(c, k));
                }
            }
        }
        out
    }
    /// The rules still to run that start the conversation `c`.
    fn starters(&self, c: usize) -> Vec<usize> {
        let s = &self.p.m.script;
        let name = &self.p.m.conversations[c].name;
        (0..s.rules.len())
            .filter(|&r| {
                s.rules[r]
                    .actions
                    .iter()
                    .map_while(|a| s.action(a))
                    .any(|i| matches!(&s.actions[i].value, Action::Conversation(n) if n.eq_ignore_ascii_case(name)))
            })
            .collect()
    }
    fn scheduled(&self, r: usize) -> bool {
        self.p
            .sim
            .gameplay
            .script
            .rules
            .iter()
            .any(|run| run.rule == r)
    }
    /// The answers that lead a conversation to its line `k`: each line on
    /// the way from a line that opens it.
    fn steer(&mut self, c: usize, k: usize) {
        let talk = self.p.m.conversations[c].clone();
        let index = |n: u16| talk.phrases.iter().position(|ph| ph.index == n);
        let mut back: BTreeMap<usize, usize> = BTreeMap::new();
        let mut queue: VecDeque<usize> = talk
            .phrases
            .iter()
            .enumerate()
            .filter(|(_, ph)| ph.start)
            .map(|(i, _)| i)
            .collect();
        let mut seen: BTreeSet<usize> = queue.iter().copied().collect();
        while let Some(i) = queue.pop_front() {
            if i == k {
                break;
            }
            for &n in &talk.phrases[i].next {
                if let Some(j) = index(n)
                    && seen.insert(j)
                {
                    back.insert(j, i);
                    queue.push_back(j);
                }
            }
        }
        let name: &'static str = Box::leak(talk.name.clone().into_boxed_str());
        let mut at = k;
        loop {
            let entry = (name, talk.phrases[at].index);
            if !self.p.answers.contains(&entry) {
                self.p.answers.push(entry);
            }
            match back.get(&at) {
                Some(&i) => at = i,
                None => break,
            }
        }
    }

    /// Works on the first change of `plan` not yet there.
    fn achieve(&mut self, plan: &[Atom], depth: usize, seen: &mut BTreeSet<usize>) -> String {
        // The variables first: they mostly say which stage of the story an
        // event counts in (deaths counted once the attack has begun).
        // Being somewhere comes last: a hero waits for the rest where he
        // is, rather than at the place he would then leave.
        let mut plan = plan.to_vec();
        // The player's own losses the script counts on come right after:
        // unhurtable, the player's units do not suffer them by themselves.
        plan.sort_by_key(|a| match a {
            Atom::Event(name) | Atom::Not(name) => {
                match self.p.m.script.events.iter().find(|e| e.name == *name) {
                    Some(e) if self.own_loss(&e.value) => 1,
                    Some(e) if matches!(e.value, Event::InArea(..) | Event::Entered(..)) => 3,
                    _ => 2,
                }
            }
            _ => 0,
        });
        for atom in &plan {
            match atom {
                Atom::Rich => {
                    if !self.rich {
                        self.rich = true;
                        let player = self.player();
                        if let Some(k) = self.p.m.kingdoms.get_mut(player as usize - 1) {
                            k.resources = [1_000_000; 4];
                        }
                        self.forced.push("risorse".into());
                    }
                }
                Atom::Tribute(r, k) => {
                    if self.tributes.contains(&(*r, *k)) {
                        continue;
                    }
                    self.tributes.insert((*r, *k));
                    let player = self.player();
                    if let Some(own) = self.p.m.kingdoms.get_mut(player as usize - 1) {
                        own.resources[*r as usize] = own.resources[*r as usize].max(100_000);
                    }
                    // The tribute window's send (0x447bd1).
                    let p = &mut self.p;
                    let sent = issue(
                        &mut p.sim,
                        &mut p.m,
                        &p.g,
                        &p.lib,
                        &p.c,
                        &Request {
                            order: "tribute".into(),
                            target: Some(*k as usize),
                            resource: *r,
                            amount: 100_000,
                            ..Default::default()
                        },
                    );
                    self.forced
                        .push(format!("tributo di 100000 ({r}) al regno {k}"));
                    return format!("tributo al regno {k}: {sent:?}");
                }
                Atom::Event(name) => {
                    if self.holds(name) {
                        continue;
                    }
                    return self.event(name, depth, seen);
                }
                Atom::Not(name) => {
                    if !self.holds(name) {
                        continue;
                    }
                    return self.unevent(name);
                }
                Atom::Var(name, v) => {
                    if self.p.sim.gameplay.script.variables.get(name) == *v {
                        continue;
                    }
                    // A running timer comes by itself.
                    if self
                        .p
                        .sim
                        .gameplay
                        .script
                        .timers
                        .iter()
                        .any(|t| t.name == *name)
                    {
                        return format!("{name}: attesa del timer");
                    }
                    let mut waiting = false;
                    for source in self.sources(name, *v) {
                        let rules = match source {
                            Source::Rule(r) => vec![r],
                            Source::Line(c, k) => {
                                self.steer(c, k);
                                self.starters(c)
                            }
                        };
                        for r in rules {
                            if seen.contains(&r) || !self.scheduled(r) {
                                continue;
                            }
                            waiting = true;
                            let rule = self.p.m.script.rules[r].clone();
                            match self.plan(&rule.condition) {
                                Some(sub) if sub.is_empty() => {
                                    return format!("{name}: regola «{}» pronta", rule.name);
                                }
                                Some(sub) if depth < 8 => {
                                    seen.insert(r);
                                    let step = self.achieve(&sub, depth + 1, seen);
                                    return format!("{name} ← «{}»: {step}", rule.name);
                                }
                                _ => {}
                            }
                        }
                    }
                    if waiting {
                        return format!("{name}: attesa delle regole che lo danno");
                    }
                    // Nothing in the script gives it any more.
                    self.p.sim.gameplay.script.variables.set(name, *v);
                    self.forced.push(format!("variabile {name}={v}"));
                    return format!("{name}={v} forzata");
                }
            }
        }
        "attesa".into()
    }

    /// Works on the event `name`: the orders toward it, else forced.
    fn event(&mut self, name: &str, depth: usize, seen: &mut BTreeSet<usize>) -> String {
        let event = self
            .p
            .m
            .script
            .events
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.value.clone())
            .unwrap();
        // Hidden objects come with the rules that bring them.
        let listed = match &event {
            Event::Exists(s)
            | Event::InArea(s, _)
            | Event::Entered(s, _)
            | Event::Expression(s, _) => Some(s),
            _ => None,
        };
        if let Some(s) = listed
            && !s.by_class
        {
            let hidden: Vec<u16> = s
                .ids
                .iter()
                .copied()
                .filter(|&id| {
                    self.index(id)
                        .is_some_and(|i| self.p.sim.gameplay.script.hidden.contains(&i))
                })
                .collect();
            if !hidden.is_empty() {
                return self.appear(name, &hidden, depth, seen);
            }
        }
        // The player wins by the game's rules: no kingdom at war left.
        if let Event::Won(mask) = &event
            && mask & 1 << (self.player() - 1) != 0
        {
            return format!("evento «{name}» (vittoria): {}", self.sweep());
        }
        let now = self.now();
        let patience = if self.own_loss(&event) {
            0
        } else {
            self.patience(&event)
        };
        let until = *self.tries.entry(name.into()).or_insert(now + patience);
        if now < until {
            if self.ordered.get(name).is_none_or(|&t| now >= t + 20) {
                self.ordered.insert(name.into(), now);
                self.toward(&event);
            }
            return format!("evento «{name}»: ordini");
        }
        self.tries.insert(name.into(), now + 60);
        let times = self
            .forced
            .iter()
            .filter(|f| f.ends_with(&format!("(«{name}»)")))
            .count();
        if times >= 3 {
            self.stuck = Some(format!(
                "evento «{name}» forzato {times} volte senza effetto"
            ));
            return format!("evento «{name}»: forzatura senza effetto");
        }
        match self.force(&event) {
            Some(how) => {
                self.forced.push(format!("{how} («{name}»)"));
                format!("evento «{name}» forzato: {how}")
            }
            None => format!("evento «{name}»: attesa"),
        }
    }
    /// Works on an event the condition negates: what makes it hold sent
    /// where it no longer does, else carried there or struck down.
    fn unevent(&mut self, name: &str) -> String {
        let event = self
            .p
            .m
            .script
            .events
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.value.clone())
            .unwrap();
        // The objects that make it hold, each with a place where it would
        // not; none for those only a death takes away.
        let mut away: Vec<(usize, Option<[u16; 2]>)> = Vec::new();
        match &event {
            Event::InArea(s, a) => {
                let Some(r) = self.rect(a) else {
                    return format!("evento «{name}» da togliere: area sconosciuta");
                };
                let side = self.p.g.side as i32;
                for i in self.listed(s) {
                    if !self.inside(i, r) {
                        continue;
                    }
                    let (x, y) = (self.p.m.objects[i].x as i32, self.p.m.objects[i].y as i32);
                    let to = [[r[0] - 3, y], [r[2] + 3, y], [x, r[1] - 3], [x, r[3] + 3]]
                        .into_iter()
                        .filter(|c| c[0] >= 0 && c[1] >= 0 && c[0] < side && c[1] < side)
                        .min_by_key(|c| (c[0] - x).abs() + (c[1] - y).abs())
                        .map(|c| [c[0] as u16, c[1] as u16]);
                    away.push((i, to));
                }
            }
            Event::Expression(s, e) => {
                let suppose = |i| Suppose {
                    object: Some(i),
                    ..Suppose::default()
                };
                for i in self.listed(s) {
                    if self.present(i) && self.probe(e, &suppose(i)) != 0 {
                        let to = if placed(e) {
                            self.spot(i, e, false, false)
                        } else {
                            None
                        };
                        away.push((i, to));
                    }
                }
            }
            Event::Exists(s) if !s.at_most => {
                away = self.listed(s).into_iter().map(|i| (i, None)).collect();
            }
            _ => return format!("evento «{name}» da togliere: non so come"),
        }
        let key = format!("!{name}");
        let now = self.now();
        let until = *self.tries.entry(key.clone()).or_insert(now + 180);
        if now < until {
            if self.ordered.get(&key).is_none_or(|&t| now >= t + 20) {
                self.ordered.insert(key, now);
                for &(i, to) in &away {
                    if let Some(to) = to
                        && self.p.m.commands(&self.p.m.objects[i])
                    {
                        let id = self.p.m.objects[i].id;
                        self.p.try_order("move", &[id], to, None);
                    }
                }
            }
            return format!("evento «{name}» da togliere: ordini");
        }
        self.tries.insert(key, now + 60);
        let tag = format!("(«!{name}»)");
        let times = self.forced.iter().filter(|f| f.ends_with(&tag)).count();
        if times >= 3 {
            self.stuck = Some(format!("evento «{name}» tolto {times} volte senza effetto"));
            return format!("evento «{name}» da togliere: forzatura senza effetto");
        }
        // What can move set down elsewhere, by sea too; the rest struck down,
        // the player's left behind as a player would, but for the heroes.
        let mut carried = Vec::new();
        let mut left = Vec::new();
        if let Event::Expression(_, e) = &event
            && placed(e)
        {
            let objs: Vec<usize> = away.iter().map(|&(i, _)| i).collect();
            (carried, left) = self.relocate(&objs, e, false);
        } else {
            for (i, to) in away {
                match to {
                    Some(to) => {
                        let id = self.p.m.objects[i].id;
                        self.p.carry(&[id], to);
                        carried.push(id);
                    }
                    None => left.push(i),
                }
            }
        }
        let named = self.named();
        let struck: Vec<usize> = left
            .into_iter()
            .filter(|&i| {
                !self.p.m.controls(self.p.m.objects[i].owner) || self.expendable(i, &named)
            })
            .collect();
        self.strike(&struck);
        let how = format!("tolti: portati {carried:?}, abbattuti {}", struck.len());
        self.forced.push(format!("{how} {tag}"));
        format!("evento «{name}» tolto: {how}")
    }
    /// Hidden objects that a rule's `AppearObj` brings.
    fn appear(
        &mut self,
        name: &str,
        ids: &[u16],
        depth: usize,
        seen: &mut BTreeSet<usize>,
    ) -> String {
        let s = self.p.m.script.clone();
        for r in 0..s.rules.len() {
            if seen.contains(&r) || !self.scheduled(r) {
                continue;
            }
            let brings = s.rules[r]
                .actions
                .iter()
                .map_while(|a| s.action(a))
                .any(|i| matches!(&s.actions[i].value, Action::Appear(sel) if sel.ids.iter().any(|id| ids.contains(id))));
            if !brings {
                continue;
            }
            match self.plan(&s.rules[r].condition) {
                Some(sub) if sub.is_empty() => {
                    return format!("evento «{name}»: regola «{}» pronta", s.rules[r].name);
                }
                Some(sub) if depth < 8 => {
                    seen.insert(r);
                    let step = self.achieve(&sub, depth + 1, seen);
                    return format!("evento «{name}» ← «{}»: {step}", s.rules[r].name);
                }
                _ => {}
            }
        }
        format!("evento «{name}»: oggetti nascosti in attesa")
    }

    fn index(&self, id: u16) -> Option<usize> {
        self.p.m.objects.iter().position(|o| o.id == id)
    }
    fn present(&self, i: usize) -> bool {
        let o = &self.p.m.objects[i];
        alive(o) && o.flags & (FLAG_DYING | FLAG_INACTIVE | 4) == 0
    }
    /// The objects a selector lists, or those of its classes and kingdoms.
    fn listed(&self, s: &Selector) -> Vec<usize> {
        if s.by_class {
            (0..self.p.m.objects.len())
                .filter(|&i| {
                    let o = &self.p.m.objects[i];
                    (1..=8).contains(&o.owner)
                        && s.kingdoms & 1 << (o.owner - 1) != 0
                        && class_code(s.class, o)
                        && self.present(i)
                })
                .collect()
        } else {
            s.ids.iter().filter_map(|&id| self.index(id)).collect()
        }
    }
    /// How many of a selector's objects must match.
    fn wanted(&mut self, s: &Selector, listed: usize) -> usize {
        if s.by_class {
            if s.at_most {
                return 0;
            }
            self.probe(&s.count, &Suppose::default()).max(0) as usize
        } else if s.all {
            listed
        } else {
            1.min(listed)
        }
    }
    /// The cells of an area.
    fn rect(&mut self, a: &crate::script::Area) -> Option<[i32; 4]> {
        if !a.around {
            let [x1, y1, x2, y2] = a.rect.map(i32::from);
            return Some([x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2)]);
        }
        let i = self.index(a.object)?;
        let o = self.p.m.objects[i].clone();
        let [w, h] = self.p.lib.cell_footprint(&o);
        let d = self.probe(&a.radius, &Suppose::default());
        let (x, y) = (o.x as i32, o.y as i32);
        Some([x - d, y - d, x + w as i32 - 1 + d, y + h as i32 - 1 + d])
    }
    fn inside(&self, i: usize, r: [i32; 4]) -> bool {
        let o = &self.p.m.objects[i];
        (r[0]..=r[2]).contains(&(o.x as i32)) && (r[1]..=r[3]).contains(&(o.y as i32))
    }
    /// The player's fighters and heroes.
    fn army(&self) -> Vec<usize> {
        let player = self.player();
        (0..self.p.m.objects.len())
            .filter(|&i| {
                let o = &self.p.m.objects[i];
                (o.owner == player || ((1..=8).contains(&o.owner) && self.p.m.controls(o.owner)))
                    && self.present(i)
                    && crate::selection::unit(o)
                    && ((3..=38).contains(&o.class) || !o.name.is_empty())
            })
            .collect()
    }

    /// Whether the event counts deaths of the player's own objects.
    fn own_loss(&self, event: &Event) -> bool {
        let Event::Destroyed(s) = event else {
            return false;
        };
        if s.by_class {
            s.kingdoms != 0
                && (1..=8u8)
                    .filter(|&k| s.kingdoms & 1 << (k - 1) != 0)
                    .all(|k| self.p.m.controls(k))
        } else {
            !s.ids.is_empty()
                && s.ids.iter().all(|&id| {
                    self.index(id)
                        .is_some_and(|i| self.p.m.controls(self.p.m.objects[i].owner))
                })
        }
    }
    /// Seconds of orders before an event is forced.
    fn patience(&mut self, event: &Event) -> u64 {
        match event {
            Event::InArea(s, a) => {
                let Some(r) = self.rect(a) else { return 60 };
                let far = self
                    .listed(s)
                    .iter()
                    .map(|&i| {
                        let o = &self.p.m.objects[i];
                        (o.x as i32 - (r[0] + r[2]) / 2)
                            .abs()
                            .max((o.y as i32 - (r[1] + r[3]) / 2).abs())
                    })
                    .max()
                    .unwrap_or(0);
                (60 + 3 * far as u64).min(400)
            }
            Event::Entered(..) => 180,
            Event::Destroyed(_) | Event::Expression(..) => 180,
            Event::Defeated(_) => 120,
            Event::Exists(s) if s.at_most => 180,
            Event::Exists(_) => 30,
            _ => 600,
        }
    }

    /// The orders a player would give toward the event.
    fn toward(&mut self, event: &Event) {
        match event {
            Event::InArea(s, a) => {
                let Some(r) = self.rect(a) else { return };
                let to = [
                    ((r[0] + r[2]) / 2).max(0) as u16,
                    ((r[1] + r[3]) / 2).max(0) as u16,
                ];
                let movers = self.movers(s, |p, i| !p.inside(i, r), to);
                for i in movers {
                    let id = self.p.m.objects[i].id;
                    if !self.p.m.objects[i].item() {
                        self.p.try_order("move", &[id], to, None);
                        continue;
                    }
                    // A thing is carried there: by who holds it, else
                    // picked up by the nearest hero.
                    if let Some(h) = self.p.sim.objects[i].holder {
                        let hid = self.p.m.objects[h].id;
                        self.p.try_order("move", &[hid], to, None);
                        continue;
                    }
                    let at = [self.p.m.objects[i].x, self.p.m.objects[i].y];
                    let hero = self
                        .army()
                        .into_iter()
                        .filter(|&h| !self.p.m.objects[h].name.is_empty())
                        .min_by_key(|&h| gap(at, [self.p.m.objects[h].x, self.p.m.objects[h].y]));
                    if let Some(h) = hero {
                        let hid = self.p.m.objects[h].id;
                        self.p.try_order("pick_item", &[hid], at, Some(id));
                    }
                }
            }
            Event::Entered(s, h) => {
                let Some(holder) = self.holder(h) else { return };
                let hid = self.p.m.objects[holder].id;
                let at = [self.p.m.objects[holder].x, self.p.m.objects[holder].y];
                let movers = self.movers(s, |p, i| p.p.sim.objects[i].holder != Some(holder), at);
                for i in movers {
                    let id = self.p.m.objects[i].id;
                    if self.p.m.objects[i].item() {
                        // A thing goes to a unit that picks it up.
                        let spot = [self.p.m.objects[i].x, self.p.m.objects[i].y];
                        self.p.try_order("pick_item", &[hid], spot, Some(id));
                    } else {
                        self.p.try_order("context", &[id], at, Some(hid));
                    }
                }
            }
            // A place: each object sent where the expression holds for it.
            Event::Expression(s, e) if placed(e) => {
                for i in self.listed(s) {
                    if !self.present(i) || !self.p.m.commands(&self.p.m.objects[i]) {
                        continue;
                    }
                    if let Some(to) = self.spot(i, e, true, false)
                        && to != [self.p.m.objects[i].x, self.p.m.objects[i].y]
                    {
                        let id = self.p.m.objects[i].id;
                        self.p.try_order("move", &[id], to, None);
                    }
                }
            }
            // A wound comes from the script's own fights, else is forced.
            Event::Expression(_, e) if wounds(e) => {}
            Event::Destroyed(s) | Event::Expression(s, _) => self.assail(s),
            Event::Exists(s) if s.at_most => self.assail(s),
            _ => {}
        }
    }
    /// The army sent at the nearest of a selector's objects.
    fn assail(&mut self, s: &Selector) {
        let player = self.player();
        let targets: Vec<usize> = self
            .listed(s)
            .into_iter()
            .filter(|&i| self.present(i) && self.p.m.objects[i].owner != player)
            .collect();
        if targets.is_empty() {
            return;
        }
        for i in self.army() {
            let here = [self.p.m.objects[i].x, self.p.m.objects[i].y];
            let &t = targets
                .iter()
                .min_by_key(|&&t| gap(here, [self.p.m.objects[t].x, self.p.m.objects[t].y]))
                .unwrap();
            let id = self.p.m.objects[i].id;
            let tid = self.p.m.objects[t].id;
            self.p.close_in(&[id], tid);
        }
    }
    /// The nearest cell, of a coarse grid, where `e` read for the object `i`
    /// standing there gives `want`: on its own ground, or on the sea for
    /// a walker that goes there aboard a ship.
    fn spot(&mut self, i: usize, e: &str, want: bool, sea: bool) -> Option<[u16; 2]> {
        let o = self.p.m.objects[i].clone();
        let side = self.p.g.side as i32;
        let at = [o.x as i32, o.y as i32];
        // The last answer stands while the object stays near where it was.
        let key = (i, e.to_owned(), want, sea);
        if let Some(&(from, found)) = self.spots.get(&key)
            && gap(from, [o.x, o.y]) <= 8
        {
            return found;
        }
        let suppose = Suppose {
            object: Some(i),
            ..Suppose::default()
        };
        let mut found = None;
        'rings: for r in (0..side).step_by(8) {
            for dy in (-r..=r).step_by(8) {
                for dx in (-r..=r).step_by(8) {
                    if dx.abs().max(dy.abs()) != r {
                        continue;
                    }
                    // Beyond the map, its edge: a thin strip there is not missed.
                    let c = [
                        (at[0] + dx).clamp(0, side - 1),
                        (at[1] + dy).clamp(0, side - 1),
                    ];
                    let cell = [c[0] as u16, c[1] as u16];
                    let water = water_cell(&self.p.g, cell);
                    if water != (naval(&o) || sea)
                        || !(water || self.p.sim.gameplay.pathing.cells.passable(c))
                    {
                        continue;
                    }
                    position(&mut self.p.m.objects[i], cell);
                    let v = self.probe(e, &suppose) != 0;
                    if v == want {
                        found = Some(cell);
                        break 'rings;
                    }
                }
            }
        }
        self.p.m.objects[i] = o.clone();
        self.spots.insert(key, ([o.x, o.y], found));
        found
    }
    /// Each of `objs` set down where `e` read for it gives `want`: on the
    /// ground, else on the sea aboard the player's ships; the ids carried
    /// and the objects left where they were.
    fn relocate(&mut self, objs: &[usize], e: &str, want: bool) -> (Vec<u16>, Vec<usize>) {
        let mut moved = Vec::new();
        let mut aboard = Vec::new();
        for &i in objs {
            let id = self.p.m.objects[i].id;
            let here = [self.p.m.objects[i].x, self.p.m.objects[i].y];
            match self.spot(i, e, want, false) {
                Some(to) if to == here => {}
                Some(to) => {
                    self.p.carry(&[id], to);
                    moved.push(id);
                }
                None if self.p.sim.objects[i].holder.is_none() => aboard.push(i),
                None => {}
            }
        }
        if let Some(to) = aboard.first().and_then(|&i| self.spot(i, e, want, true)) {
            moved.extend(self.embark(&mut aboard, to));
        }
        (moved, aboard)
    }
    /// `aboard` taken onto the player's ships, the heroes first, each ship
    /// then set down at `to`; the ships' ids. What no ship takes stays in
    /// `aboard`.
    fn embark(&mut self, aboard: &mut Vec<usize>, to: [u16; 2]) -> Vec<u16> {
        aboard.sort_by_key(|&i| self.p.m.objects[i].name.is_empty());
        let mut ships = Vec::new();
        while let Some(ship) = self.ship(aboard) {
            let mut room = holder_capacity(&self.p.sim, &self.p.m, &self.p.lib, ship)
                - self.p.sim.objects[ship].contents.len();
            let at = [self.p.m.objects[ship].x, self.p.m.objects[ship].y];
            let mut k = 0;
            while k < aboard.len() && room > 0 {
                let i = aboard[k];
                if !accepts(
                    &self.p.lib,
                    &self.p.c,
                    &self.p.m.objects[ship],
                    &self.p.m.objects[i],
                ) {
                    k += 1;
                    continue;
                }
                aboard.remove(k);
                position(&mut self.p.m.objects[i], at);
                garrison(&mut self.p.sim, &mut self.p.m, ship, i);
                room -= 1;
            }
            let sid = self.p.m.objects[ship].id;
            self.p.carry(&[sid], to);
            ships.push(sid);
        }
        ships
    }
    /// The free cell of `r` nearest its middle where `i` may stand: its
    /// own ground, or the sea for a walker going aboard.
    fn cell_in(&self, i: usize, r: [i32; 4], sea: bool) -> Option<[u16; 2]> {
        let o = &self.p.m.objects[i];
        let side = self.p.g.side as i32;
        let mid = [(r[0] + r[2]) / 2, (r[1] + r[3]) / 2];
        let mut cells: Vec<[i32; 2]> = (r[1].max(0)..=r[3].min(side - 1))
            .flat_map(|y| (r[0].max(0)..=r[2].min(side - 1)).map(move |x| [x, y]))
            .collect();
        cells.sort_by_key(|c| (c[0] - mid[0]).abs().max((c[1] - mid[1]).abs()));
        cells
            .into_iter()
            .map(|c| [c[0] as u16, c[1] as u16])
            .find(|&cell| {
                let water = water_cell(&self.p.g, cell);
                water == (naval(o) || sea)
                    && (water
                        || self
                            .p
                            .sim
                            .gameplay
                            .pathing
                            .cells
                            .passable([cell[0] as i32, cell[1] as i32]))
                    && !self
                        .p
                        .m
                        .objects
                        .iter()
                        .any(|x| alive(x) && crate::selection::unit(x) && [x.x, x.y] == cell)
            })
    }
    /// The player's ship with room nearest the first of `who` that takes
    /// one of them.
    fn ship(&self, who: &[usize]) -> Option<usize> {
        let first = &self.p.m.objects[*who.first()?];
        let at = [first.x, first.y];
        (0..self.p.m.objects.len())
            .filter(|&k| {
                let o = &self.p.m.objects[k];
                naval(o)
                    && (1..=8).contains(&o.owner)
                    && self.p.m.controls(o.owner)
                    && self.present(k)
                    && !who.contains(&k)
                    && holder_capacity(&self.p.sim, &self.p.m, &self.p.lib, k)
                        > self.p.sim.objects[k].contents.len()
                    && who
                        .iter()
                        .any(|&i| accepts(&self.p.lib, &self.p.c, o, &self.p.m.objects[i]))
            })
            .min_by_key(|&k| gap(at, [self.p.m.objects[k].x, self.p.m.objects[k].y]))
    }
    /// A building of `class` raised whole for `owner` with its corner in
    /// `r`, nearest the middle; its id.
    fn raise(&mut self, class: usize, owner: u8, r: [i32; 4]) -> Option<u16> {
        let civ = self
            .p
            .m
            .kingdoms
            .get(owner as usize - 1)
            .map_or(1, |k| k.civilization.max(1));
        let side = self.p.g.side as i32;
        let mid = [(r[0] + r[2]) / 2, (r[1] + r[3]) / 2];
        let mut cells: Vec<[i32; 2]> = (r[1].max(0)..=r[3].min(side - 1))
            .flat_map(|y| (r[0].max(0)..=r[2].min(side - 1)).map(move |x| [x, y]))
            .collect();
        cells.sort_by_key(|c| (c[0] - mid[0]).abs().max((c[1] - mid[1]).abs()));
        for c in cells {
            let cell = [c[0] as u16, c[1] as u16];
            let o = crate::gameplay::cheats::spawn_probe(&self.p.lib, class, civ, owner, cell)?;
            let corner = [o.x as i32, o.y as i32];
            if !(r[0]..=r[2]).contains(&corner[0])
                || !(r[1]..=r[3]).contains(&corner[1])
                || !crate::gameplay::cheats::spawn_site(
                    &self.p.sim,
                    &self.p.m,
                    &self.p.g,
                    &self.p.lib,
                    &o,
                )
            {
                continue;
            }
            let p = &mut self.p;
            let j = new_unit(&mut p.sim, &mut p.m, &p.lib, class, owner, civ, [o.x, o.y]).ok()?;
            found_site(&mut p.sim, &mut p.m, &p.lib, &p.c, j);
            if p.m.objects[j].flags & FLAG_CONSTRUCTION != 0 {
                complete_site(&mut p.sim, &mut p.m, &p.lib, j);
            }
            return Some(p.m.objects[j].id);
        }
        None
    }
    /// The player's objects to send for a selector: those it lists, or, by
    /// class, the nearest of the classes to `to`.
    fn movers(
        &mut self,
        s: &Selector,
        needs: impl Fn(&Pilot, usize) -> bool,
        to: [u16; 2],
    ) -> Vec<usize> {
        let mut listed: Vec<usize> = self
            .listed(s)
            .into_iter()
            .filter(|&i| self.present(i) && self.p.m.commands(&self.p.m.objects[i]))
            .collect();
        listed.sort_by_key(|&i| gap([self.p.m.objects[i].x, self.p.m.objects[i].y], to));
        let n = self.wanted(s, listed.len()).max(1);
        listed
            .into_iter()
            .filter(|&i| needs(self, i))
            .take(n)
            .collect()
    }
    fn holder(&self, h: &crate::script::Holder) -> Option<usize> {
        if h.by_class {
            (0..self.p.m.objects.len()).find(|&i| {
                let o = &self.p.m.objects[i];
                (1..=8).contains(&o.owner)
                    && h.kingdoms & 1 << (o.owner - 1) != 0
                    && class_code(h.class, o)
                    && self.present(i)
            })
        } else {
            self.index(h.object)
        }
    }

    /// Brings the event about at once; how, or none when it cannot.
    fn force(&mut self, event: &Event) -> Option<String> {
        match event {
            Event::InArea(s, a) => {
                let r = self.rect(a)?;
                let to = [
                    ((r[0] + r[2]) / 2).max(0) as u16,
                    ((r[1] + r[3]) / 2).max(0) as u16,
                ];
                let listed = self.listed(s);
                let n = self.wanted(s, listed.len()).max(1);
                // A building is not carried: another is raised there.
                let mut out: Vec<usize> = listed
                    .into_iter()
                    .filter(|&i| !self.inside(i, r) && !self.p.m.objects[i].building())
                    .collect();
                out.sort_by_key(|&i| gap([self.p.m.objects[i].x, self.p.m.objects[i].y], to));
                let ids: Vec<u16> = out
                    .iter()
                    .take(n)
                    .map(|&i| self.p.m.objects[i].id)
                    .collect();
                // A building wanted there that nobody has built.
                if ids.is_empty() && s.by_class && (39..=73).contains(&s.class) {
                    let owner = (1..=8u8).find(|&p| s.kingdoms & 1 << (p - 1) != 0)?;
                    if let Some(id) = self.raise(s.class as usize, owner, r) {
                        return Some(format!("eretto #{id} di classe {}", s.class));
                    }
                    // No room: the enemies' objects there struck down first,
                    // as the player takes the place.
                    let player = self.player();
                    let foes: Vec<usize> = (0..self.p.m.objects.len())
                        .filter(|&i| {
                            let o = &self.p.m.objects[i];
                            (1..=8).contains(&o.owner)
                                && !self.p.m.controls(o.owner)
                                && relation(&self.p.m, player, o.owner) & 0x40 == 0
                                && self.present(i)
                                && self.inside(i, r)
                                && (crate::selection::unit(o) || o.building())
                        })
                        .collect();
                    if foes.is_empty() {
                        return None;
                    }
                    self.strike(&foes);
                    return Some(format!("sgomberati {} oggetti", foes.len()));
                }
                // Each set down in the area, on its ground; a walker where the
                // area is sea, aboard the player's ships.
                let mut moved = Vec::new();
                let mut aboard = Vec::new();
                for i in out.into_iter().take(n) {
                    let id = self.p.m.objects[i].id;
                    match self.cell_in(i, r, false) {
                        Some(c) => {
                            self.p.carry(&[id], c);
                            moved.push(id);
                        }
                        None if self.p.sim.objects[i].holder.is_none() => aboard.push(i),
                        None => {}
                    }
                }
                if let Some(c) = aboard.first().and_then(|&i| self.cell_in(i, r, true)) {
                    moved.extend(self.embark(&mut aboard, c));
                }
                if moved.is_empty() {
                    return None;
                }
                Some(format!("portati in {to:?}: {moved:?}"))
            }
            Event::Entered(s, h) => {
                let holder = self.holder(h)?;
                let listed = self.listed(s);
                let n = self.wanted(s, listed.len()).max(1);
                let ids: Vec<usize> = listed
                    .into_iter()
                    .filter(|&i| self.p.sim.objects[i].holder != Some(holder))
                    .take(n)
                    .collect();
                for &i in &ids {
                    let at = [self.p.m.objects[holder].x, self.p.m.objects[holder].y];
                    position(&mut self.p.m.objects[i], at);
                    garrison(&mut self.p.sim, &mut self.p.m, holder, i);
                }
                Some(format!(
                    "dentro #{}: {} oggetti",
                    self.p.m.objects[holder].id,
                    ids.len()
                ))
            }
            Event::Destroyed(s) if s.by_class => {
                let n = self.probe(&s.count, &Suppose::default()).max(0) as usize;
                let lost: u32 = self
                    .p
                    .sim
                    .gameplay
                    .script
                    .lost
                    .iter()
                    .filter(|&(&k, _)| {
                        let p = k >> 8;
                        (1..=11).contains(&p)
                            && s.kingdoms & 1 << (p - 1) != 0
                            && crate::gameplay::scripting::class_code_matches(
                                s.class,
                                (k & 0xff) as usize,
                            )
                    })
                    .map(|(_, &n)| n)
                    .sum();
                // Of the player's, only the expendable: what more is wanted
                // comes later.
                let named = self.named();
                let victims: Vec<usize> = self
                    .by_worth(self.listed(s))
                    .into_iter()
                    .filter(|&i| {
                        !self.p.m.controls(self.p.m.objects[i].owner) || self.expendable(i, &named)
                    })
                    .take(n.saturating_sub(lost as usize))
                    .collect();
                if victims.is_empty() {
                    return None;
                }
                self.strike(&victims);
                Some(format!("abbattuti {}", victims.len()))
            }
            Event::Destroyed(s) => {
                let victims: Vec<usize> = self
                    .listed(s)
                    .into_iter()
                    .filter(|&i| self.present(i))
                    .collect();
                self.strike(&victims);
                Some(format!("abbattuti {}", victims.len()))
            }
            // None may be left: those over the count struck down.
            Event::Exists(s) if s.by_class && s.at_most => {
                let n = self.probe(&s.count, &Suppose::default()).max(0) as usize;
                let mut kept = self.by_worth(self.listed(s));
                kept.reverse();
                let victims: Vec<usize> = kept.into_iter().skip(n).collect();
                self.strike(&victims);
                Some(format!("abbattuti {}", victims.len()))
            }
            Event::Exists(s) if s.by_class => {
                let have = self.listed(s).len();
                let n = self.wanted(s, have);
                let owner = (1..=8u8).find(|&p| s.kingdoms & 1 << (p - 1) != 0)?;
                let civ = self
                    .p
                    .m
                    .kingdoms
                    .get(owner as usize - 1)
                    .map_or(1, |k| k.civilization.max(1));
                // The kingdom's own place: its first building, else the
                // player's home.
                let home = self
                    .p
                    .m
                    .objects
                    .iter()
                    .enumerate()
                    .find(|&(i, o)| o.owner == owner && o.building() && self.present(i))
                    .map(|(_, o)| [o.x, o.y])
                    .or(self.p.m.home())
                    .unwrap_or([self.p.g.side as u16 / 2; 2]);
                // A class of the code: the first fighter the kingdom has.
                let class = match s.class {
                    c if (c as usize) < crate::world::CLASSES.len() => c as usize,
                    crate::script::any::BUILDING => return None,
                    _ => (3..=38).find(|&k| {
                        crate::gameplay::cheats::spawn_probe(&self.p.lib, k, civ, owner, home)
                            .is_some()
                    })?,
                };
                if (39..=73).contains(&class) {
                    let [x, y] = home.map(i32::from);
                    let made: Vec<u16> = (have..n)
                        .map_while(|_| self.raise(class, owner, [x - 16, y - 16, x + 16, y + 16]))
                        .collect();
                    return Some(format!("eretti {made:?} di classe {class}"));
                }
                let mut made = 0;
                for _ in have..n {
                    let Ok(i) = new_unit(
                        &mut self.p.sim,
                        &mut self.p.m,
                        &self.p.lib,
                        class,
                        owner,
                        civ,
                        home,
                    ) else {
                        break;
                    };
                    let o = self.p.m.objects[i].clone();
                    if let Some(at) =
                        free_near(&self.p.g, &self.p.m, &self.p.lib, &o, home, Some(i))
                    {
                        position(&mut self.p.m.objects[i], at);
                    }
                    let o = &mut self.p.m.objects[i];
                    o.flags &= !FLAG_CONSTRUCTION;
                    o.health = o.max_health;
                    made += 1;
                }
                Some(format!("costruiti {made} di classe {class}"))
            }
            // A place or a state of the objects the expression reads.
            Event::Expression(s, e) => {
                let player = self.player();
                // A place: each object carried where it holds for it.
                if placed(e) {
                    let objs: Vec<usize> = self
                        .listed(s)
                        .into_iter()
                        .filter(|&i| self.present(i))
                        .collect();
                    let (moved, _) = self.relocate(&objs, e, true);
                    if !moved.is_empty() {
                        return Some(format!("portati dove vale: {moved:?}"));
                    }
                }
                // A state of each object for which it does not hold yet: the
                // lightest wound, the player's ownership, the least cargo.
                let one = |i| Suppose {
                    object: Some(i),
                    ..Suppose::default()
                };
                let mut done = Vec::new();
                for i in self.listed(s) {
                    if !self.present(i) || self.probe(e, &one(i)) != 0 {
                        continue;
                    }
                    let before = self.p.m.objects[i].clone();
                    let cargo = self.p.sim.gameplay.actors[i].cargo;
                    for (tweak, how) in ["ferito", "preso", "carico"].into_iter().enumerate() {
                        let set = |me: &mut Pilot, v: u32| match tweak {
                            0 => me.p.m.objects[i].health = v as u16,
                            1 => me.p.m.objects[i].owner = player,
                            _ => me.p.sim.gameplay.actors[i].cargo = [v, 0, 0, 0],
                        };
                        let (low, high) = match tweak {
                            0 => (1, u32::from(before.health.max(1))),
                            1 => (0, 0),
                            _ => (1, 100_000),
                        };
                        set(self, if tweak == 0 { low } else { high });
                        if self.probe(e, &one(i)) == 0 {
                            self.p.m.objects[i] = before.clone();
                            self.p.sim.gameplay.actors[i].cargo = cargo;
                            continue;
                        }
                        // The wound as light, the cargo as small, as will do.
                        let (mut lo, mut hi) = (low, high);
                        while tweak != 1 && lo < hi {
                            if tweak == 0 {
                                let mid = lo + (hi - lo).div_ceil(2);
                                set(self, mid);
                                if self.probe(e, &one(i)) != 0 {
                                    lo = mid;
                                } else {
                                    hi = mid - 1;
                                }
                            } else {
                                let mid = lo + (hi - lo) / 2;
                                set(self, mid);
                                if self.probe(e, &one(i)) != 0 {
                                    hi = mid;
                                } else {
                                    lo = mid + 1;
                                }
                            }
                        }
                        if tweak != 1 {
                            set(self, lo);
                        }
                        done.push(format!("#{} {how}", before.id));
                        break;
                    }
                }
                (!done.is_empty()).then(|| done.join(", "))
            }
            Event::Defeated(mask) => {
                let victims: Vec<usize> = (0..self.p.m.objects.len())
                    .filter(|&i| {
                        let o = &self.p.m.objects[i];
                        (1..=8).contains(&o.owner)
                            && mask & 1 << (o.owner - 1) != 0
                            && self.present(i)
                            && (crate::selection::unit(o) || o.building())
                    })
                    .collect();
                self.strike(&victims);
                Some(format!("regni spazzati ({} oggetti)", victims.len()))
            }
            _ => None,
        }
    }
    /// `objs` with the expendable first.
    fn by_worth(&self, mut objs: Vec<usize>) -> Vec<usize> {
        let named = self.named();
        objs.sort_by_key(|&i| !self.expendable(i, &named));
        objs
    }
    /// An object with no name whose id the events do not name.
    fn expendable(&self, i: usize, named: &BTreeSet<u16>) -> bool {
        let o = &self.p.m.objects[i];
        o.name.is_empty() && !named.contains(&o.id)
    }
    /// The ids the events name.
    fn named(&self) -> BTreeSet<u16> {
        let s = &self.p.m.script;
        let mut named: BTreeSet<u16> = BTreeSet::new();
        for e in &s.events {
            match &e.value {
                Event::Exists(x)
                | Event::Destroyed(x)
                | Event::Entered(x, _)
                | Event::InArea(x, _)
                | Event::Expression(x, _) => named.extend(&x.ids),
                _ => {}
            }
            if let Event::InArea(_, a) = &e.value {
                named.insert(a.object);
            }
        }
        named
    }
    /// Objects struck down, as the battle would; the end test follows.
    fn strike(&mut self, victims: &[usize]) {
        for &i in victims {
            kill(&mut self.p.sim, &mut self.p.m, &self.p.g, &self.p.lib, i);
        }
        self.p.sim.gameplay.script.end_due = true;
    }
    /// A mission won by the game's own rules: the kingdoms at war with the
    /// player swept after a while.
    fn sweep(&mut self) -> String {
        let player = self.player();
        let foes: u16 = (1..=8u8)
            .filter(|&k| {
                k != player
                    && self
                        .p
                        .m
                        .kingdoms
                        .get(k as usize - 1)
                        .is_some_and(|x| x.active)
                    && !self.p.m.controls(k)
                    && !self.p.sim.gameplay.script.kingdoms[k as usize].defeated
            })
            .fold(0, |m, k| m | 1 << (k - 1));
        if foes == 0 {
            return "nessun nemico".into();
        }
        let event = Event::Defeated(foes);
        let now = self.now();
        let until = *self.tries.entry("<nemici>".into()).or_insert(now + 300);
        if now < until {
            return "nessuna regola di vittoria: attesa".into();
        }
        self.tries.insert("<nemici>".into(), now + 120);
        if let Some(how) = self.force(&event) {
            self.forced.push(how.clone());
            return how;
        }
        "nessun nemico da spazzare".into()
    }

    /// Saves the copy of the game from a little before the victory.
    fn save_before_victory(&self, dir: &std::path::Path) -> std::result::Result<String, String> {
        let won_at = self.now();
        let (_, snap) = self
            .ring
            .iter()
            .rev()
            .find(|(t, _)| *t + 15 <= won_at)
            .or_else(|| self.ring.front())
            .ok_or("nessuna copia")?;
        // The heroes as the game has them, hurtable.
        let mut snap = snap.clone();
        for o in &mut snap.m.objects {
            if self.godded.contains(&o.id) {
                o.flags &= !FLAG_GOD;
            }
        }
        save_game(&snap, &self.scenario, self.campaign, self.mission, dir)
    }
}

/// The `name = value` statements of an expression, single `=` only.
fn assignments(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for statement in text.split(';') {
        let b = statement.as_bytes();
        let Some(at) = (0..b.len()).find(|&i| {
            b[i] == b'='
                && b.get(i + 1) != Some(&b'=')
                && (i == 0 || !matches!(b[i - 1], b'=' | b'<' | b'>' | b'!'))
        }) else {
            continue;
        };
        let lhs = statement[..at].trim().to_owned();
        let rhs = statement[at + 1..].trim().to_owned();
        if !lhs.is_empty() {
            out.push((lhs, rhs));
        }
    }
    out
}
/// Whether `name` stands negated in `text`: `!name`, `!(name)`.
fn negated(text: &str, name: &str) -> bool {
    let b = text.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$' | b'@');
    text.match_indices(name).any(|(at, _)| {
        let end = at + name.len();
        if (at > 0 && word(b[at - 1])) || (end < b.len() && word(b[end])) {
            return false;
        }
        text[..at].trim_end_matches([' ', '(']).ends_with('!')
    })
}
/// The stocks of given kingdoms a condition reads, `@gold(7)`: resource
/// index and kingdom.
fn stocks(text: &str) -> Vec<(u8, u8)> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    for (r, f) in ["@gold(", "@stone(", "@food(", "@wood("]
        .into_iter()
        .enumerate()
    {
        for (at, _) in lower.match_indices(f) {
            let digits: String = lower[at + f.len()..]
                .trim_start()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(k) = digits.parse::<u8>() {
                out.push((r as u8, k));
            }
        }
    }
    out
}
/// Whether an expression reads its object's health.
fn wounds(e: &str) -> bool {
    crate::script::audit::symbols(e)
        .iter()
        .any(|s| s.eq_ignore_ascii_case("$health"))
}
/// Whether an expression reads where its object stands.
fn placed(e: &str) -> bool {
    crate::script::audit::symbols(e)
        .iter()
        .any(|s| s.eq_ignore_ascii_case("$x") || s.eq_ignore_ascii_case("$y"))
}
fn suppose(combo: &[&Atom]) -> Suppose {
    let mut s = Suppose::default();
    for a in combo {
        match a {
            Atom::Event(n) => {
                s.events.insert(n.clone(), true);
            }
            Atom::Not(n) => {
                s.events.insert(n.clone(), false);
            }
            Atom::Var(n, v) => {
                s.variables.insert(n.clone(), *v);
            }
            Atom::Rich => s.rich = true,
            Atom::Tribute(_, k) => {
                s.stocks.insert(i32::from(*k));
            }
        }
    }
    s
}
/// Whether a selector's class code takes the object (0x4b9670).
fn class_code(code: u16, o: &MapObject) -> bool {
    crate::gameplay::scripting::class_code_matches(code, o.class)
}

/// A game saved as the window saves it, under the pilot's name.
fn save_game(
    p: &Play,
    scenario: &Scenario,
    campaign: usize,
    mission: usize,
    dir: &std::path::Path,
) -> std::result::Result<String, String> {
    let assets = GameAssets::open(root()).map_err(|e| e.to_string())?;
    let c = scenario.container().map_err(|e| e.to_string())?;
    let source_entries = c
        .entries
        .iter()
        .map(|e| {
            c.read(e.inode)
                .map(|b| (e.name.clone(), std::sync::Arc::new(b)))
                .map_err(|e| e.to_string())
        })
        .collect::<std::result::Result<_, _>>()?;
    let visuals = p
        .lib
        .build(&assets, &p.m, p.g.side)
        .map_err(|e| e.to_string())?;
    let flyers = visuals
        .flights
        .iter()
        .map(crate::flight::Flyer::new)
        .collect();
    let player = p.m.player();
    let at = p
        .m
        .objects
        .iter()
        .find(|o| o.owner == player && !o.name.is_empty() && alive(o) && crate::selection::unit(o))
        .map(|o| [o.x, o.y])
        .or_else(|| p.m.home())
        .unwrap_or([0, 0]);
    let meta = crate::menu::SavedSession {
        version: crate::menu::SAVE_VERSION,
        snapshot: true,
        title: scenario.title.clone(),
        name: format!("Pilota - {}", scenario.title),
        file: crate::save::new_file(dir),
        scenario: Some(scenario.id.clone()),
        selection: crate::menu::Selection::Scenario {
            campaign: Some(campaign),
            mission,
        },
        camera: [at[0] as f32 * 32. + 16., -(at[1] as f32 * 32. + 16.), 1.],
        setup: None,
        missing: false,
    };
    let snapshot = crate::save::Snapshot {
        language: crate::lang::current().index(),
        schema: 1,
        grid: p.g.clone(),
        biome: p.biome.clone(),
        title: scenario.title.clone(),
        description: scenario.description.clone(),
        starting_positions: p.m.starting_positions,
        mission: p.m.clone(),
        simulation: p.sim.clone(),
        source_entries,
        external_entries: Default::default(),
        portraits: crate::game::portraits(&c, &p.m).into_iter().collect(),
        hud: Default::default(),
        groups: Default::default(),
        health_kept: false,
        idle: 0,
        visual_time: Duration::ZERO,
        flyers,
    };
    let path = crate::save::save_path(dir, &meta.file).map_err(|e| e.to_string())?;
    crate::save::write(&path, &meta, &snapshot).map_err(|e| e.to_string())?;
    Ok(meta.file)
}

/// The campaign of the file `file`, flown from its first map along the maps
/// its victories open, then each map not reached on its own; one line per
/// map.
fn fly_campaign(file: &str) -> Option<Vec<std::result::Result<String, String>>> {
    let catalog = ScenarioCatalog::load(root()).unwrap();
    let campaign = catalog.campaigns.iter().position(|c| {
        c.missions[0]
            .file
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case(file))
    })?;
    let saves = std::env::var_os("RETZAR_PILOT_SAVES").map(std::path::PathBuf::from);
    let missions = catalog.campaigns[campaign].missions.clone();
    let mut reached = BTreeSet::new();
    let mut lines = Vec::new();
    let run = |pilot: &mut Pilot, lines: &mut Vec<std::result::Result<String, String>>| {
        let title = pilot.scenario.title.clone();
        let started = pilot.now();
        let result = pilot.fly();
        let time = pilot.now() - started;
        let forced = if pilot.forced.is_empty() {
            "nessuna forzatura".to_owned()
        } else {
            format!("forzati: {}", pilot.forced.join("; "))
        };
        match result {
            Ok(()) => {
                let saved = saves
                    .as_ref()
                    .map(|dir| match pilot.save_before_victory(dir) {
                        Ok(f) => format!(", salvata in {f}"),
                        Err(e) => format!(", salvataggio non riuscito: {e}"),
                    })
                    .unwrap_or_default();
                lines.push(Ok(format!(
                    "{title}: vinta in {}:{:02}{saved}; {forced}",
                    time / 60,
                    time % 60
                )));
                true
            }
            Err(e) => {
                lines.push(Err(format!("{title}: {e}; {forced}").replace('\n', " |")));
                false
            }
        }
    };
    // The story, carrying the heroes.
    let mut at = Some(0);
    let mut p = Play::open_at(&catalog, campaign, 0);
    while let Some(k) = at {
        reached.insert(k);
        let mut pilot = Pilot::new(&catalog, campaign, k, p.clone());
        if !run(&mut pilot, &mut lines) {
            break;
        }
        let Some(next) = pilot.p.sim.gameplay.script.next_map.clone() else {
            break;
        };
        let wanted = next.replace('\\', "/");
        at = missions.iter().position(|m| {
            m.nested
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(&wanted))
        });
        if at.is_some_and(|k| reached.contains(&k)) {
            break;
        }
        if at.is_some() {
            p = pilot.p.next();
        }
    }
    // The maps the story did not reach, each from its start.
    for k in 0..missions.len() {
        if reached.contains(&k) {
            continue;
        }
        let mut pilot = Pilot::new(&catalog, campaign, k, Play::open_at(&catalog, campaign, k));
        run(&mut pilot, &mut lines);
    }
    Some(lines)
}

fn report(file: &str) {
    let Some(lines) = fly_campaign(file) else {
        return;
    };
    for line in &lines {
        match line {
            Ok(l) => eprintln!("OK  {l}"),
            Err(l) => eprintln!("NO  {l}"),
        }
    }
    let failed: Vec<&String> = lines.iter().filter_map(|l| l.as_ref().err()).collect();
    assert!(
        failed.is_empty(),
        "{} mappe non vinte:\n{}",
        failed.len(),
        failed
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
#[ignore = "Requires installed original archives and the American CD in USA"]
fn the_pilot_wins_every_map_of_the_american_campaign() {
    report("Tzar.wcm");
}

#[test]
#[ignore = "Requires installed original archives and the Spanish edition"]
fn the_pilot_wins_every_map_of_la_reconquista() {
    report("La Reconquista.wcm");
}

#[test]
#[ignore = "Requires installed original archives and the Spanish edition"]
fn the_pilot_wins_every_map_of_los_dominios_de_la_magia() {
    report("Los Dominios de la Magia.wcm");
}
