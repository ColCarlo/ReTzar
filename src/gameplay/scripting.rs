//! The mission scripts at run time (0x4b6800–0x4bd700) and the end of the
//! game (0x442d10, 0x442e30, 0x4431c0).
//!
//! Rules and timers are messages of the engine's timed queue: every rule is
//! posted 1 ms after the map loads, in file order, and tests its condition
//! again every 1000 ms; a timer adds or takes 1000 every 1000 ms. Equal
//! times run in posting order. A rule that holds runs its actions in the
//! same dispatch until one asks to wait; it then resumes at that action. It
//! is deleted after its last action unless one of them is `KeepRule`.
use super::*;
use crate::script::{
    Action, Area, Event, Holder, Message, Quest, Script, Selector, any,
    expr::{self, Function, Global, Host, Property, Variables},
};
use std::collections::HashMap;

/// Retest period of a rule and step of a timer (0x4b6a70, 0x4bd770).
const PERIOD: u64 = 1000;
/// What a waiting story, message or conversation returns (0x4bac60).
const WAIT: u64 = 100;
/// The end screen opens this long after the end of the game (0x442af0).
pub const END_DELAY_MS: u64 = 5000;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ScriptState {
    pub started: bool,
    /// Milliseconds of the queue already dispatched.
    pub clock: u64,
    pub sequence: u64,
    pub rules: Vec<RuleRun>,
    pub timers: Vec<TimerRun>,
    pub variables: Variables,
    /// Actions in their second phase: a story or message shown, a delay
    /// begun (the native flag of the action object).
    pub waiting: BTreeSet<usize>,
    /// Objects hidden by the map (flag 0x4000) that `AppearObj` may bring.
    pub hidden: BTreeSet<usize>,
    /// Objects of each kingdom and class that died (+0x2c0), by
    /// `owner << 8 | class`.
    pub lost: BTreeMap<u16, u32>,
    /// Status of kingdoms 1–8 (0x5ca708 + 0x6a·p).
    pub kingdoms: [Standing; 9],
    /// A story, message or conversation is open (0x9dbe24).
    pub modal: bool,
    /// The map `LoadMap` chose: «CONTINUA» opens it.
    pub next_map: Option<String>,
    /// The text after `|` of `PlayerWin` or `PlayerLose` (0x5cb5a8): the end
    /// window shows it under its buttons.
    pub end_text: String,
    /// The game is over: whether the local kingdom won, and when.
    pub over: Option<(bool, u64)>,
    /// An event the original follows with its end test (0x442e30): a death
    /// (0x4de019, 0x41d5fc), a change of owner (0x4e50cb, 0x41fb76) or of
    /// diplomacy (0x46813c), a resignation (0x48fd06), a script freeing a
    /// kingdom that could not win or lose (0x4bc763). Only the time and
    /// score limits test it otherwise; nothing does at the start.
    pub end_due: bool,
    pub conversation: Option<Talk>,
    /// `$candelobjects` and `$canusediplomacy`.
    pub no_delete: bool,
    pub no_diplomacy: bool,
    /// The text of the last `@str`.
    pub text: String,
    /// What the interface has to show; never saved.
    #[serde(skip)]
    pub outbox: Outbox,
    /// The rules that fired and the actions they ran, with their time in
    /// ms, when a test or a tool asks for them; never saved.
    #[serde(skip)]
    pub trace: Trace,
}
/// A record of the script's steps, kept only once switched on.
#[derive(Clone, Debug, Default)]
pub struct Trace(pub Option<Vec<(u64, String)>>);
impl PartialEq for Trace {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for Trace {}
impl Trace {
    fn note(&mut self, time: u64, what: impl FnOnce() -> String) {
        if let Some(log) = &mut self.0
            && log.len() < 100_000
        {
            log.push((time, what()));
        }
    }
}
/// Requests to the interface, drained every frame: not part of the state,
/// so neither saved nor compared.
#[derive(Clone, Debug, Default)]
pub struct Outbox(pub Vec<Show>);
impl PartialEq for Outbox {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for Outbox {}
impl std::ops::Deref for Outbox {
    type Target = Vec<Show>;
    fn deref(&self) -> &Vec<Show> {
        &self.0
    }
}
impl std::ops::DerefMut for Outbox {
    fn deref_mut(&mut self) -> &mut Vec<Show> {
        &mut self.0
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Standing {
    /// +0x4e.
    pub defeated: bool,
    /// +0x52: resigned (message 0x43, 0x48fc8d).
    pub resigned: bool,
    /// +0x5a.
    pub won: bool,
    /// +0x5e: lost by a script.
    pub lost: bool,
    /// +0x62, +0x66.
    pub cannot_win: bool,
    pub cannot_lose: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleRun {
    pub rule: usize,
    pub due: u64,
    pub sequence: u64,
    /// The action to resume at, by position in the rule.
    pub cursor: Option<usize>,
    pub keep: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimerRun {
    pub name: String,
    pub due: u64,
    pub sequence: u64,
    pub countdown: bool,
}
/// A conversation in progress: the phrase shown and the choices offered.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Talk {
    pub conversation: usize,
    /// Phrase shown, by position.
    pub phrase: Option<usize>,
    /// Phrases offered to choose from, by position.
    pub choices: Vec<usize>,
}
/// Requests to the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Show {
    /// The message window (0x47d930) on `MissionWorld::message`, with no
    /// voice (0x4bb025).
    Note,
    /// The conversation is over: its bubbles close (0x438380).
    Close,
    /// The story screen (0x4c34c0) on `MissionWorld::message`, its picture
    /// in `portrait`: the voice 100 ms after it opens (0x4c3e08), the text
    /// going up over `ms`, closed then when `close` (0x4c3d3d); `music` is
    /// played while it stays open, the game's taken away (0x4c389e).
    Story {
        sound: String,
        ms: u64,
        close: bool,
        music: String,
    },
    /// A conversation's line or choices (0x4380f0), as
    /// [`ScriptState::conversation`] has them: the bubbles (0x438480).
    Say,
    /// A line of text over the map.
    Line(String),
    /// A line of `[viewmsg]`, its `%s` replaced.
    Notice(&'static str, String),
    /// A line of STRINGS3 `[msg]`, its `%s` replaced.
    Message(&'static str, String),
    /// LockView (0x4bba60 → 0x4f9140): the view on the corner of `cell`,
    /// or following `follow`, for `ms` milliseconds (0: until UnlockView or
    /// until the object is gone, at once without one); meanwhile the
    /// player neither scrolls nor selects.
    Lock {
        cell: [u16; 2],
        follow: Option<usize>,
        ms: u64,
    },
    /// ScrollView (0x4bca60 → 0x4f6d00): the view glides from the corner
    /// of one cell to another's, scrolling locked.
    Scroll {
        from: [u16; 2],
        to: [u16; 2],
    },
    Unlock,
    Select(usize),
    Unselect(usize),
    ClearSelection,
    Movie(String),
    /// The relations changed.
    Diplomacy,
}

impl ScriptState {
    /// Count an object that died (called by `kill`).
    pub(super) fn died(&mut self, owner: u8, class: usize) {
        self.end_due = true;
        if (1..=11).contains(&owner) && class < 256 {
            *self
                .lost
                .entry((owner as u16) << 8 | class as u16)
                .or_default() += 1;
        }
    }
    fn post(&mut self, due: u64) -> u64 {
        self.sequence += 1;
        let _ = due;
        self.sequence
    }
    pub fn validate(&self, m: &MissionWorld) -> bool {
        let (rules, actions, n) = (
            m.script.rules.len(),
            m.script.actions.len(),
            m.objects.len(),
        );
        let phrases = |t: &Talk| m.conversations.get(t.conversation).map(|c| c.phrases.len());
        self.rules.len() <= rules
            && self.conversation.as_ref().is_none_or(|t| {
                phrases(t).is_some_and(|p| {
                    t.phrase.is_none_or(|i| i < p) && t.choices.iter().all(|&i| i < p)
                })
            })
            && self.rules.iter().all(|r| r.rule < rules)
            && self.timers.len() <= 4096
            && self.waiting.iter().all(|&a| a < actions)
            && self.hidden.iter().all(|&i| i < n)
            && self.variables.map.len() <= 256
            && self.variables.kept.len() <= 256
    }
}

/// The objects and kingdoms the script works on.
struct Ctx<'a> {
    sim: &'a mut Simulation,
    m: &'a mut MissionWorld,
    g: &'a MapGrid,
    lib: &'a ObjectLibrary,
    c: &'a Catalog,
    script: &'a Script,
    st: &'a mut ScriptState,
    ids: HashMap<u16, usize>,
    /// Events being evaluated: a condition that names itself reads 0.
    depth: u8,
}

/// Status flag of a kingdom.
fn standing(st: &ScriptState, p: u8) -> Standing {
    st.kingdoms.get(p as usize).copied().unwrap_or_default()
}

/// Run the scripts up to the current tick, then test the end of the game.
pub(super) fn run(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
) {
    let mut st = std::mem::take(&mut sim.gameplay.script);
    let script = std::mem::take(&mut m.script);
    let now = sim.ticks * TICK_MS;
    {
        let mut ctx = Ctx {
            sim,
            m,
            g,
            lib,
            c,
            script: &script,
            st: &mut st,
            ids: HashMap::new(),
            depth: 0,
        };
        ctx.index();
        if !ctx.st.started {
            ctx.start();
        }
        ctx.dispatch(now);
        ctx.st.clock = now;
        // 0x4d468d tests the time limit once it is past, 0x443530 the score
        // limit as the scores change; any other test follows an event.
        let rules = &ctx.m.game_rules;
        let limits =
            (rules.victory == 2 && now > rules.victory_limit as u64 * 60_000) || rules.victory == 3;
        if std::mem::take(&mut ctx.st.end_due) | limits && ctx.st.over.is_none() {
            ctx.check_end(None);
        }
    }
    m.script = script;
    sim.gameplay.script = st;
}

/// 0x4b7fc0 for a computer player's `EXTERN` (0x4081c2): the value of the
/// expression, read like a mission's with no object of its own.
pub(super) fn extern_value(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    text: &str,
) -> i32 {
    let mut st = std::mem::take(&mut sim.gameplay.script);
    let script = std::mem::take(&mut m.script);
    let value = {
        let mut ctx = Ctx {
            sim,
            m,
            g,
            lib,
            c,
            script: &script,
            st: &mut st,
            ids: HashMap::new(),
            depth: 0,
        };
        ctx.index();
        expr::eval(text, &mut ctx, None)
    };
    m.script = script;
    sim.gameplay.script = st;
    value
}

impl Ctx<'_> {
    fn index(&mut self) {
        self.ids.clear();
        for (i, o) in self.m.objects.iter().enumerate() {
            self.ids.entry(o.id).or_insert(i);
        }
    }
    /// 0x4b70b0: every rule tested 1 ms after the load, in file order.
    fn start(&mut self) {
        self.st.started = true;
        self.st.variables.map.clear();
        self.st.hidden = self
            .m
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.flags & FLAG_INACTIVE != 0 && o.flags & 4 == 0)
            .map(|(i, _)| i)
            .collect();
        for rule in 0..self.script.rules.len() {
            let sequence = self.st.post(1);
            self.st.rules.push(RuleRun {
                rule,
                due: 1,
                sequence,
                cursor: None,
                keep: false,
            });
        }
    }
    /// Dispatch everything due by `now`, each at its own time.
    fn dispatch(&mut self, now: u64) {
        loop {
            let rule = self
                .st
                .rules
                .iter()
                .enumerate()
                .filter(|(_, r)| r.due <= now)
                .min_by_key(|(_, r)| (r.due, r.sequence))
                .map(|(i, r)| (r.due, r.sequence, i));
            let timer = self
                .st
                .timers
                .iter()
                .enumerate()
                .filter(|(_, t)| t.due <= now)
                .min_by_key(|(_, t)| (t.due, t.sequence))
                .map(|(i, t)| (t.due, t.sequence, i));
            match (rule, timer) {
                (Some(r), Some(t)) if (t.0, t.1) < (r.0, r.1) => self.tick_timer(t.2, t.0),
                (Some(r), _) => self.run_rule(r.2, r.0),
                (None, Some(t)) => self.tick_timer(t.2, t.0),
                (None, None) => break,
            }
        }
    }
    /// 0x4bd770.
    fn tick_timer(&mut self, i: usize, time: u64) {
        let t = self.st.timers[i].clone();
        let v = self.st.variables.get(&t.name);
        if t.countdown {
            let v = v.wrapping_sub(PERIOD as i32);
            self.st.variables.set(&t.name, v);
            if v <= 0 {
                self.st.timers.remove(i);
                return;
            }
        } else {
            self.st
                .variables
                .set(&t.name, v.wrapping_add(PERIOD as i32));
        }
        let sequence = self.st.post(time + PERIOD);
        let timer = &mut self.st.timers[i];
        timer.due = time + PERIOD;
        timer.sequence = sequence;
    }
    /// 0x4b6a70.
    fn run_rule(&mut self, i: usize, time: u64) {
        let run = self.st.rules[i].clone();
        let script = self.script;
        let rule = &script.rules[run.rule];
        let mut keep = run.keep;
        let start = match run.cursor {
            Some(c) => c,
            None => {
                if expr::eval(&rule.condition, self, None) == 0 {
                    return self.repost(i, time, PERIOD, None, keep);
                }
                self.st
                    .trace
                    .note(time, || format!("regola «{}»", rule.name));
                0
            }
        };
        for at in start..rule.actions.len() {
            // An unknown name ends the list (0x4b73f0 leaves it empty).
            let Some(action) = script.action(&rule.actions[at]) else {
                break;
            };
            let wait = self.execute(action, time);
            if wait != 0 {
                return self.repost(i, time, wait, Some(at), keep);
            }
            if script.actions[action].value == Action::KeepRule {
                keep = true;
            }
        }
        if keep {
            self.repost(i, time, PERIOD, None, true);
        } else {
            self.st.rules.remove(i);
        }
    }
    fn repost(&mut self, i: usize, time: u64, delay: u64, cursor: Option<usize>, keep: bool) {
        let sequence = self.st.post(time + delay);
        let r = &mut self.st.rules[i];
        r.due = time + delay;
        r.sequence = sequence;
        r.cursor = cursor;
        r.keep = keep;
    }

    /// `kill`, counting the death here: the state in `sim` is set aside
    /// while the script runs.
    fn kill(&mut self, i: usize) {
        let o = &self.m.objects[i];
        if o.flags & (FLAG_INACTIVE | FLAG_DYING) == 0 {
            self.st.died(o.owner, o.class);
        }
        refund_site(self.sim, self.m, self.c, i);
        kill(self.sim, self.m, self.g, self.lib, i);
    }
    /// The "exists" filter (0x4b9970): present, not dying, buildings finished.
    fn exists(&self, i: usize) -> bool {
        let o = &self.m.objects[i];
        alive(o)
            && o.flags & (FLAG_DYING | 4) == 0
            && !(o.building() && o.flags & FLAG_CONSTRUCTION != 0)
    }
    fn present(&self, i: usize) -> bool {
        let o = &self.m.objects[i];
        alive(o) && o.flags & (FLAG_DYING | 4) == 0
    }
    fn class_matches(&self, code: u16, o: &MapObject) -> bool {
        match code {
            any::OBJECT => true,
            any::UNIT => world::class::is_unit(o.class),
            any::MILITARY => (3..=38).contains(&o.class),
            any::BUILDING => o.building(),
            any::MAJOR => (world::class::is_unit(o.class) || o.building()) && major(o.class),
            _ => o.class == code as usize,
        }
    }
    /// 0x4b9670: whether the selector holds, and the objects it matched.
    fn select(
        &mut self,
        s: &Selector,
        filter: &mut dyn FnMut(&mut Self, Option<usize>) -> bool,
    ) -> (bool, Vec<usize>) {
        let mut matched = Vec::new();
        if s.by_class {
            for i in 0..self.m.objects.len() {
                let o = &self.m.objects[i];
                let listed = (1..=8).contains(&o.owner)
                    && s.kingdoms & 1 << (o.owner - 1) != 0
                    && self.class_matches(s.class, o);
                if listed && filter(self, Some(i)) {
                    matched.push(i);
                }
            }
            let n = expr::eval(&s.count, self, None);
            let count = matched.len() as i32;
            let holds = if s.at_most { count <= n } else { count >= n };
            (holds, matched)
        } else {
            let mut passed = 0;
            for &id in &s.ids {
                let i = self
                    .ids
                    .get(&id)
                    .copied()
                    .filter(|&i| !self.st.hidden.contains(&i));
                if filter(self, i) {
                    passed += 1;
                    matched.extend(i);
                }
            }
            let holds = if s.all {
                passed == s.ids.len()
            } else {
                passed > 0
            };
            (holds, matched)
        }
    }
    fn select_existing(&mut self, s: &Selector) -> (bool, Vec<usize>) {
        self.select(s, &mut |ctx, i| i.is_some_and(|i| ctx.exists(i)))
    }
    /// The cells of an area, inclusive.
    fn area(&mut self, a: &Area) -> Option<[i32; 4]> {
        if !a.around {
            let [x1, y1, x2, y2] = a.rect.map(i32::from);
            return Some([x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2)]);
        }
        let i = *self.ids.get(&a.object)?;
        let o = &self.m.objects[i];
        let [w, h] = self.lib.cell_footprint(o);
        let (x, y) = (o.x as i32, o.y as i32);
        let d = expr::eval(&a.radius, self, None);
        Some([x - d, y - d, x + w as i32 - 1 + d, y + h as i32 - 1 + d])
    }
    fn area_centre(&mut self, a: &Area) -> Option<[u16; 2]> {
        let [x1, y1, x2, y2] = self.area(a)?;
        let side = self.g.side as i32 - 1;
        Some([
            ((x1 + x2) / 2).clamp(0, side) as u16,
            ((y1 + y2) / 2).clamp(0, side) as u16,
        ])
    }
    /// A point of ScrollView (0x4bca60): the cell of the area's object, or
    /// the middle of its rectangle, `x1 + (x2 − x1 + 1) / 2`; nothing when
    /// the object is gone.
    fn view_cell(&self, a: &Area) -> Option<[u16; 2]> {
        if a.object != 0 {
            let o = &self.m.objects[*self.ids.get(&a.object)?];
            return Some([o.x, o.y]);
        }
        let [x1, y1, x2, y2] = a.rect.map(i32::from);
        let side = self.g.side as i32 - 1;
        Some([x1 + (x2 - x1 + 1) / 2, y1 + (y2 - y1 + 1) / 2].map(|v| v.clamp(0, side) as u16))
    }
    fn holder_matches(&self, h: &Holder, i: usize) -> bool {
        let Some(j) = self.sim.objects[i].holder else {
            return false;
        };
        if h.by_class {
            let o = &self.m.objects[j];
            self.class_matches(h.class, o)
                && (1..=8).contains(&o.owner)
                && h.kingdoms & 1 << (o.owner - 1) != 0
        } else {
            self.ids.get(&h.object) == Some(&j)
        }
    }

    /// Events 1000–1007.
    fn evaluate(&mut self, event: &Event) -> bool {
        match event {
            Event::Exists(s) => self.select_existing(s).0,
            Event::Destroyed(s) if s.by_class => {
                let lost: u32 = (1..=11u16)
                    .filter(|p| s.kingdoms & 1 << (p - 1) != 0)
                    .map(|p| {
                        self.st
                            .lost
                            .iter()
                            .filter(|&(&k, _)| {
                                k >> 8 == p && class_code_matches(s.class, (k & 0xff) as usize)
                            })
                            .map(|(_, &n)| n)
                            .sum::<u32>()
                    })
                    .sum();
                let n = expr::eval(&s.count, self, None);
                if s.at_most {
                    (lost as i64) <= n as i64
                } else {
                    (lost as i64) >= n as i64
                }
            }
            Event::Destroyed(s) => {
                self.select(s, &mut |ctx, i| i.is_none_or(|i| !ctx.exists(i)))
                    .0
            }
            // 0x4b9be0 and 0x4b9ad0 filter as 0x4b9970 does: a building
            // still being built is not there yet.
            Event::Entered(s, h) => {
                self.select(s, &mut |ctx, i| {
                    i.is_some_and(|i| ctx.exists(i) && ctx.holder_matches(h, i))
                })
                .0
            }
            Event::Left => false,
            Event::InArea(s, a) => {
                let Some([x1, y1, x2, y2]) = self.area(a) else {
                    return false;
                };
                self.select(s, &mut |ctx, i| {
                    i.is_some_and(|i| {
                        let o = &ctx.m.objects[i];
                        let (x, y) = (o.x as i32, o.y as i32);
                        ctx.exists(i) && (x1..=x2).contains(&x) && (y1..=y2).contains(&y)
                    })
                })
                .0
            }
            Event::Defeated(mask) => {
                (1..=8u8).any(|p| mask & 1 << (p - 1) != 0 && standing(self.st, p).defeated)
            }
            Event::Won(mask) => {
                (1..=8u8).any(|p| mask & 1 << (p - 1) != 0 && standing(self.st, p).won)
            }
            Event::Expression(s, e) => {
                self.select(s, &mut |ctx, i| {
                    i.is_some_and(|i| ctx.exists(i) && expr::eval(e, ctx, Some(i)) != 0)
                })
                .0
            }
        }
    }

    /// An action's Execute (slot 2); the milliseconds to wait, or 0.
    fn execute(&mut self, index: usize, time: u64) -> u64 {
        let name = self.script.actions[index].name.clone();
        // A delay or a window comes back once more when it ends.
        if !self.st.waiting.contains(&index) {
            self.st.trace.note(time, || format!("  azione «{name}»"));
        }
        match self.script.actions[index].value.clone() {
            Action::AddQuest { text, portrait } => {
                // 0x443640: appended, never checked for duplicates.
                self.m.quests.push(Quest {
                    id: crate::script::quest_id(&name),
                    text,
                    portrait,
                });
                self.st.outbox.push(Show::Notice("newquest", String::new()));
            }
            Action::RemoveQuest(of) => {
                let id = crate::script::quest_id(&of);
                if let Some(at) = self.m.quests.iter().position(|q| q.id == id) {
                    self.m.quests.remove(at);
                }
            }
            Action::Diplomacy { from, to, flags } => {
                if (1..=8).contains(&from) && (1..=8).contains(&to) && from != to {
                    let new = flags | relation(self.m, from, to) & 0x70;
                    merge_relation(self.m, from, to, new);
                    self.st.end_due = true;
                    self.notify_relation(from, to, new);
                    self.st.outbox.push(Show::Diplomacy);
                }
            }
            Action::Owner(s, owner) => {
                let (holds, matched) = self.select_existing(&s);
                if holds {
                    for i in matched {
                        transfer_owner(self.sim, self.m, self.lib, i, owner);
                    }
                }
            }
            Action::SetProperty(s, e) => {
                let (_, matched) = self.select(&s, &mut |ctx, i| i.is_some_and(|i| ctx.present(i)));
                for i in matched {
                    expr::eval(&e, self, Some(i));
                }
            }
            Action::Expression(text) => {
                expr::eval(&expr::statements(&text), self, None);
            }
            Action::SetTimer { init, countdown } => {
                let v = expr::eval(&init, self, None);
                self.st.variables.set(&name, v);
                let sequence = self.st.post(time + PERIOD);
                self.st.timers.push(TimerRun {
                    name,
                    due: time + PERIOD,
                    sequence,
                    countdown,
                });
            }
            Action::StopTimer(of) => {
                if let Some(at) = self.st.timers.iter().position(|t| t.name == of) {
                    self.st.timers.remove(at);
                }
            }
            Action::Lose(mask, text) | Action::Win(mask, text) => {
                let win = matches!(self.script.actions[index].value, Action::Win(..));
                // 0x4ba2a0: the map before `|` is wiped by 0x442af0; only the
                // text after it is kept.
                self.st.end_text = text
                    .split_once('|')
                    .map_or_else(String::new, |(_, b)| b.to_owned());
                for p in 1..=8u8 {
                    let listed = mask & 1 << (p - 1) != 0;
                    let k = &mut self.st.kingdoms[p as usize];
                    if listed == win {
                        k.won = true;
                    } else {
                        k.lost = true;
                    }
                }
                self.end();
            }
            Action::KeepRule => {}
            Action::LoadMap(path) => {
                let local = self.m.player() as usize;
                self.st.kingdoms[local].won = true;
                self.end();
                self.st.next_map = Some(path);
            }
            Action::LockView(time, a) => {
                // 0x4bba60: the area's first cell, its object followed
                // when there is one, for the time the text gives.
                let side = self.g.side as i32 - 1;
                let cell = [a.rect[0], a.rect[1]].map(|v| (v as i32).clamp(0, side) as u16);
                let follow = (a.object != 0)
                    .then(|| self.ids.get(&a.object).copied())
                    .flatten();
                let ms = expr::eval(&time, self, None).max(0) as u64;
                self.st.outbox.push(Show::Lock { cell, follow, ms });
            }
            Action::Chat(text) => {
                // A text starting with `@` is evaluated for its string.
                let line = if text.starts_with('@') {
                    self.st.text.clear();
                    expr::eval(&text, self, None);
                    self.st.text.clone()
                } else {
                    text
                };
                self.st.outbox.push(Show::Line(line));
            }
            Action::Story {
                text,
                image,
                sound,
                music,
                time: length,
                close,
            } => {
                return self.window(index, |ctx| {
                    let ms = expr::eval(&length, ctx, None).max(0) as u64;
                    ctx.m.message = Some(Message {
                        title: String::new(),
                        text,
                        portrait: image,
                        choices: Vec::new(),
                    });
                    Show::Story {
                        sound,
                        ms,
                        close,
                        music,
                    }
                });
            }
            Action::Message { text, image, title } => {
                return self.window(index, |ctx| {
                    ctx.m.message = Some(Message {
                        title,
                        text,
                        portrait: image,
                        choices: Vec::new(),
                    });
                    Show::Note
                });
            }
            Action::Spawn {
                class,
                count,
                owner,
                area,
                civilization,
            } => {
                let n = expr::eval(&count, self, None).clamp(0, 1000);
                if let Some([x1, y1, x2, y2]) = self.area(&area)
                    && (class as usize) < CLASSES.len()
                {
                    let side = self.g.side as i32 - 1;
                    // The last byte is the record's civilization (0x4bbd1c),
                    // whatever the kingdom's; 0 takes the first block of the
                    // class, the European (0x497940).
                    let civ = match civilization {
                        1..=3 => civilization,
                        _ => 1,
                    };
                    for _ in 0..n {
                        let x = x1 + random(self.sim, (x2 - x1 + 1).max(1) as u64) as i32;
                        let y = y1 + random(self.sim, (y2 - y1 + 1).max(1) as u64) as i32;
                        let cell = [x.clamp(0, side) as u16, y.clamp(0, side) as u16];
                        if let Ok(i) =
                            new_unit(self.sim, self.m, self.lib, class as usize, owner, civ, cell)
                        {
                            let o = self.m.objects[i].clone();
                            if let Some(p) = free_near(self.g, self.m, self.lib, &o, cell, Some(i))
                            {
                                position(&mut self.m.objects[i], p);
                            }
                        }
                    }
                    self.index();
                }
            }
            Action::UnlockView => self.st.outbox.push(Show::Unlock),
            Action::Order(s, a, order) => self.order(&s, &a, order),
            Action::ScrollView(from, to) => {
                if let (Some(from), Some(to)) = (self.view_cell(&from), self.view_cell(&to)) {
                    self.st.outbox.push(Show::Scroll { from, to });
                }
            }
            Action::Conversation(of) => {
                // 0x4bc180: waits while a window is open or the game is over,
                // but not for the end of the conversation.
                if self.st.modal || self.st.over.is_some() {
                    return WAIT;
                }
                if let Some(at) = self
                    .m
                    .conversations
                    .iter()
                    .position(|c| c.name.eq_ignore_ascii_case(&of))
                {
                    self.converse(at);
                }
            }
            Action::Destroy(s) => {
                let (_, matched) = self.select_existing(&s);
                for i in matched {
                    self.kill(i);
                }
            }
            Action::Explore(a, length) => {
                let ms = expr::eval(&length, self, None).max(0) as u64;
                let until = self.sim.ticks + ms.div_ceil(TICK_MS);
                let owner = self.m.player();
                let vision = if a.around {
                    self.ids.get(&a.object).map(|&j| Vision {
                        cell: [self.m.objects[j].x, self.m.objects[j].y],
                        until,
                        object: Some(j),
                        owner,
                        ..Default::default()
                    })
                } else {
                    // 0x4bc3dc: a fog revealer in the middle of the area, of
                    // radius (w + h) / 4 when it is 1–31, else the class's
                    // own `fog_range` (0x435070); 4000 ms when the time is 0
                    // (0x435069).
                    let until = if ms == 0 {
                        self.sim.ticks + 4000 / TICK_MS
                    } else {
                        until
                    };
                    let own = self
                        .lib
                        .property(&ai::tree::probe(class::FOG_REVEALER, 1), "fog_range")
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(8);
                    self.area(&a).map(|[x1, y1, x2, y2]| Vision {
                        cell: [((x1 + x2) / 2).max(0) as u16, ((y1 + y2) / 2).max(0) as u16],
                        radius: match (x2 - x1 + 1 + y2 - y1 + 1) / 4 {
                            r @ 1..=31 => r as u16,
                            _ => own,
                        },
                        until,
                        owner,
                        ..Default::default()
                    })
                };
                if let Some(v) = vision
                    && self.sim.gameplay.visions.len() < 1000
                {
                    self.sim.gameplay.visions.push(v);
                }
            }
            Action::Appear(s) => {
                // 0x4bc5a0 creates the waiting record's object; one of owner
                // 9 begins the Rescue program as the loader's do (0x4bc63c).
                for id in s.ids {
                    if let Some(&i) = self.ids.get(&id)
                        && self.st.hidden.remove(&i)
                    {
                        self.m.objects[i].flags &= !FLAG_INACTIVE;
                        let o = &self.m.objects[i];
                        if o.owner == 9 && crate::selection::unit(o) {
                            let now = self.sim.ticks * TICK_MS;
                            if let Some(a) = self.sim.gameplay.actors.get_mut(i) {
                                a.intent = Some(Intent::Rescue(now));
                            }
                        }
                    }
                }
                sync(self.sim, self.m, self.lib);
            }
            Action::Status {
                kingdom,
                cannot_win,
                cannot_lose,
            } => {
                if let Some(k) = self
                    .st
                    .kingdoms
                    .get_mut(kingdom as usize)
                    .filter(|_| kingdom > 0)
                {
                    // 0x4bc710: freed from either, the kingdom is tested.
                    let freed = (k.cannot_win && !cannot_win) || (k.cannot_lose && !cannot_lose);
                    k.cannot_win = cannot_win;
                    k.cannot_lose = cannot_lose;
                    self.st.end_due |= freed;
                }
            }
            Action::Lock(s, unlock) => {
                let (_, matched) = self.select_existing(&s);
                for i in matched {
                    if unlock {
                        self.m.objects[i].flags &= !FLAG_LOCKED;
                    } else {
                        self.m.objects[i].flags |= FLAG_LOCKED;
                        self.st.outbox.push(Show::Unselect(i));
                    }
                }
            }
            Action::Erase(s) => {
                let (_, matched) = self.select_existing(&s);
                for i in matched {
                    erase(self.sim, self.m, i);
                }
            }
            Action::Delay(length) => {
                if self.st.waiting.remove(&index) {
                    return 0;
                }
                let ms = expr::eval(&length, self, None);
                if ms > 0 {
                    self.st.waiting.insert(index);
                    return ms as u64;
                }
            }
        }
        0
    }
    /// The two phases of a story or message (0x4bac60, 0x4bafa0): wait for
    /// any open window, open this one, wait until it is closed.
    fn window(&mut self, index: usize, open: impl FnOnce(&mut Self) -> Show) -> u64 {
        if self.st.modal {
            return WAIT;
        }
        if self.st.waiting.remove(&index) {
            return 0;
        }
        let show = open(self);
        self.st.outbox.push(show);
        self.st.modal = true;
        self.st.waiting.insert(index);
        WAIT
    }
    /// 0x467ef0's notice when the change concerns the local kingdom.
    fn notify_relation(&mut self, from: u8, to: u8, new: u32) {
        let _ = new;
        if to != self.m.player() {
            return;
        }
        let r = relation(self.m, from, to);
        let key = if r & 0x10 != 0 {
            "player_enemy"
        } else if r & 0x20 != 0 {
            "player_neutral"
        } else if r & 0x40 != 0 {
            "player_ally"
        } else {
            return;
        };
        let name = self.m.kingdoms[from as usize - 1].name.clone();
        self.st.outbox.push(Show::Notice(key, name));
    }
    /// `ObjOrder` (0x4bb090) through the player's own orders. The point is
    /// the middle of the area's object (0x4bb134) or of the area (0x4bb0d9);
    /// an area on an object that is gone leaves it at cell 0, 0 (0x4bb0c0).
    /// Move, attack, capture, pick and guard take the object itself; unload,
    /// advance and patrol only points, advance and patrol the object's own
    /// cell (0x4bb375, 0x4bb3ba); the default is a right click on the point
    /// (vt+0x190, 0x4bb1a4).
    fn order(&mut self, s: &Selector, a: &Area, order: u8) {
        use crate::script::order::*;
        let (_, matched) = self.select_existing(s);
        let target = a.around.then(|| self.ids.get(&a.object).copied()).flatten();
        let (centre, corner) = match target {
            Some(j) => {
                let o = &self.m.objects[j];
                let [w, h] = self.sim.objects[j].size.unwrap_or([1, 1]);
                let side = self.g.side as u16 - 1;
                let centre = [
                    (o.x + w as u16 / 2).min(side),
                    (o.y + h as u16 / 2).min(side),
                ];
                (Some(centre), Some([o.x, o.y]))
            }
            None if a.around => (Some([0, 0]), Some([0, 0])),
            None => (self.area_centre(a), self.area_centre(a)),
        };
        let (key, cell, target) = match (order, target) {
            (DEFAULT, _) => ("context", centre, target),
            (MOVE, _) => ("move", corner, target),
            (ATTACK, Some(_)) => ("attack", corner, target),
            (ATTACK, None) => ("attack_ground", centre, None),
            (CAPTURE, Some(_)) => ("capture", corner, target),
            (UNLOAD, _) => ("unload", centre, None),
            (PICK, Some(_)) => ("pick_item", corner, target),
            (PATROL, _) => ("patrol", corner, None),
            (ADVANCE, _) => ("move_advance", corner, None),
            (GUARD, Some(_)) => ("guard", corner, target),
            // 4 and 10–12 do nothing in this build, the others nothing
            // without an object.
            _ => return,
        };
        for i in matched {
            let mut req = Request::command(key, vec![i]);
            req.cell = cell;
            req.target = target;
            // 0x4bb1a4: the default order is the unit's right click at the
            // point (vt+0x190), which finds what lies there.
            if order == DEFAULT
                && target.is_none()
                && let Some(p) = cell
            {
                req.target = super::point_target(self.m, self.g, self.lib, i, p);
            }
            let _ = issue_as(self.sim, self.m, self.g, self.lib, self.c, &req, true);
        }
    }
    /// 0x4382c0: the conversation's opening phrases.
    fn converse(&mut self, at: usize) {
        let starts: Vec<usize> = self.m.conversations[at]
            .phrases
            .iter()
            .enumerate()
            .filter(|(_, p)| p.start)
            .map(|(i, _)| i)
            .take(8)
            .collect();
        self.st.conversation = Some(Talk {
            conversation: at,
            phrase: None,
            choices: Vec::new(),
        });
        self.st.modal = true;
        self.offer(starts);
    }
    /// Keep the phrases whose condition holds (0x4380f0); one is shown, several
    /// are offered, none ends the conversation (0x438380).
    fn offer(&mut self, candidates: Vec<usize>) {
        let Some(talk) = self.st.conversation.clone() else {
            return;
        };
        let phrases = self.m.conversations[talk.conversation].phrases.clone();
        let open: Vec<usize> = candidates
            .into_iter()
            .filter(|&i| {
                phrases[i].condition.trim().is_empty()
                    || expr::eval(&phrases[i].condition, self, None) != 0
            })
            .collect();
        match open.len() {
            0 => {
                self.st.conversation = None;
                self.st.modal = false;
                self.m.message = None;
                self.st.outbox.push(Show::Close);
            }
            1 => self.say(open[0]),
            _ => {
                let mut choices = open;
                // Grouped by speaker (qsort 0x52e2b0 with 0x4380b0).
                choices.sort_by_key(|&i| phrases[i].speaker);
                let first = choices[0];
                self.st.conversation = Some(Talk {
                    choices: choices.clone(),
                    phrase: None,
                    ..talk
                });
                let speaker = self.speaker(phrases[first].speaker);
                self.m.message = Some(Message {
                    title: speaker.0,
                    text: String::new(),
                    portrait: speaker.1,
                    choices: choices.iter().map(|&i| phrases[i].text.clone()).collect(),
                });
                self.st.outbox.push(Show::Say);
            }
        }
    }
    fn speaker(&self, id: u16) -> (String, String) {
        self.ids.get(&id).map_or_else(Default::default, |&i| {
            let o = &self.m.objects[i];
            let name = if o.name.is_empty() {
                self.lib.label(o)
            } else {
                o.name.clone()
            };
            (name, o.portrait_large.clone())
        })
    }
    fn say(&mut self, phrase: usize) {
        let Some(talk) = self.st.conversation.as_mut() else {
            return;
        };
        talk.phrase = Some(phrase);
        talk.choices.clear();
        let p = self.m.conversations[talk.conversation].phrases[phrase].clone();
        let (title, portrait) = self.speaker(p.speaker);
        self.m.message = Some(Message {
            title,
            text: p.text,
            portrait,
            choices: Vec::new(),
        });
        self.st.outbox.push(Show::Say);
    }

    /// 0x4431c0: the game ends for the local kingdom.
    fn end(&mut self) {
        if self.st.over.is_some() {
            return;
        }
        let local = self.m.player() as usize;
        let k = self.st.kingdoms[local];
        let won = !(k.defeated || k.lost);
        self.st.kingdoms[local].won = won;
        self.st.over = Some((won, self.sim.ticks * TICK_MS));
    }
    /// 0x442d10: whether kingdom `p` is now defeated.
    fn defeat_test(&mut self, p: u8, rules: (usize, i64), bonus: Option<(u8, i64)>) {
        let k = standing(self.st, p);
        if k.cannot_lose {
            return;
        }
        if k.lost || k.resigned {
            self.st.kingdoms[p as usize].defeated = true;
            return;
        }
        // 0x442d6c: a kingdom with spies (K+0x120, «Spy Count»), hidden
        // ones included, is not tested.
        if spy_count(self.m, p) > 0 {
            return;
        }
        let major_left = self.m.objects.iter().any(|o| {
            o.owner == p
                && alive(o)
                && o.flags & FLAG_DYING == 0
                && (world::class::is_unit(o.class) || o.building())
                && major(o.class)
        });
        if !major_left {
            self.st.kingdoms[p as usize].defeated = true;
        }
        let (kind, limit) = rules;
        if kind == 1 {
            let castles = self
                .m
                .objects
                .iter()
                .any(|o| o.owner == p && o.class == class::CASTLE && alive(o));
            if !castles {
                self.st.kingdoms[p as usize].defeated = true;
            }
        } else if kind == 3 {
            for q in 1..=8u8 {
                if q != p && self.active(q) && shown_score(self.sim, q, bonus) > limit {
                    self.st.kingdoms[p as usize].defeated = true;
                    return;
                }
            }
        }
    }
    fn active(&self, p: u8) -> bool {
        self.m
            .kingdoms
            .get(p as usize - 1)
            .is_some_and(|k| k.active)
    }
    /// 0x442e30, the single-player branch.
    fn check_end(&mut self, override_rules: Option<((usize, i64), (u8, i64))>) {
        let (rules, bonus) = match override_rules {
            Some((r, b)) => (r, Some(b)),
            None => {
                let kind = self.m.game_rules.victory;
                let limit = self.m.game_rules.victory_limit as i64;
                if kind == 2 && self.sim.ticks * TICK_MS > limit as u64 * 60_000 {
                    // The highest raw score gains 1,000,000 and the rules
                    // become a score limit of that score − 1.
                    let raw = |p: u8| self.sim.gameplay.score.get(&p).copied().unwrap_or(0) as i64;
                    let mut best = (0u8, -1i64);
                    for p in 1..=8u8 {
                        if raw(p) > best.1 {
                            best = (p, raw(p));
                        }
                    }
                    return self.check_end(Some(((3, best.1 - 1), (best.0, 1_000_000))));
                }
                ((kind, limit), None)
            }
        };
        let local = self.m.player();
        for p in 1..=8u8 {
            if !self.active(p) || standing(self.st, p).defeated {
                continue;
            }
            self.defeat_test(p, rules, bonus);
            if !standing(self.st, p).defeated {
                continue;
            }
            // A custom text, or the default one outside campaigns.
            if let Some(text) = &self.script.defeat[p as usize - 1] {
                self.st.outbox.push(Show::Line(text.clone()));
            } else if !self.m.campaign {
                let name = self.m.kingdoms[p as usize - 1].name.clone();
                self.st.outbox.push(Show::Notice("player_defeated", name));
            }
        }
        let mut losers = 0u16;
        for p in 1..=8u8 {
            if !self.active(p) {
                continue;
            }
            let k = standing(self.st, p);
            if k.defeated || k.cannot_win {
                losers |= 1 << p;
                continue;
            }
            for q in 1..=8u8 {
                if q != p
                    && self.active(q)
                    && !standing(self.st, q).defeated
                    && !(relation(self.m, p, q) & 0x40 != 0
                        && shared_victory(self.m, p)
                        && shared_victory(self.m, q))
                {
                    losers |= 1 << p;
                }
            }
        }
        let winner = (1..=8u8).any(|p| self.active(p) && losers & 1 << p == 0);
        if standing(self.st, local).defeated || winner {
            self.end();
        }
    }
}

/// The classes that keep a kingdom alive (0x442bd0): all but traders,
/// walls, fields, bridges, gates, the training dummy and fishing boats.
fn major(class: usize) -> bool {
    !matches!(class, 2 | 61..=64 | 67 | 68 | 133 | 136)
}
fn class_code_matches(code: u16, class: usize) -> bool {
    match code {
        any::OBJECT => true,
        any::UNIT => world::class::is_unit(class),
        any::MILITARY => (3..=38).contains(&class),
        any::BUILDING => (39..=73).contains(&class) || class == class::DUMMY,
        any::MAJOR => major(class),
        _ => class == code as usize,
    }
}
/// The score of 0x4435c0: (raw + 50) / 100.
fn shown_score(sim: &Simulation, p: u8, bonus: Option<(u8, i64)>) -> i64 {
    let raw = sim.gameplay.score.get(&p).copied().unwrap_or(0) as i64
        + bonus.filter(|b| b.0 == p).map_or(0, |b| b.1);
    (raw + 50) / 100
}
/// 0x4683a0: a kingdom shares its victory, bit 8 of its own relation
/// R[p][p], which only the map's diplomacy sets (0x4405e6). The diplomacy
/// window writes the bit on the row, toward the others (0x447044), where the
/// win test does not look: the box does not change who wins.
fn shared_victory(m: &MissionWorld, p: u8) -> bool {
    let i = (p as usize - 1) * 9;
    m.diplomacy.get(i).is_some_and(|d| d & 8 != 0)
}
/// 0x467ef0: merge the wanted relation of `a` toward `b` with what `b`
/// wants: war is one-sided, neutrality and alliance need both.
pub(crate) fn merge_relation(m: &mut MissionWorld, a: u8, b: u8, new: u32) {
    if m.diplomacy.len() != 64 {
        m.diplomacy = vec![0; 64];
    }
    let ab = (a as usize - 1) * 8 + b as usize - 1;
    let ba = (b as usize - 1) * 8 + a as usize - 1;
    let mut forward = new;
    let mut back = relation(m, b, a);
    if forward == back && forward == relation(m, a, b) {
        return;
    }
    let wants = forward | back;
    if wants & 0x1000_0000 != 0 && forward & 0x10 == 0 {
        forward = forward & !0x67 | 0x10;
        back = back & !0x67 | 0x10;
    } else if (forward & 0x2000_0000 != 0 && back & 0x2000_0000 != 0 && forward & 0x20 == 0)
        || (forward & 0x40 != 0 && wants & 0x2000_0000 != 0)
    {
        forward = forward & !0x57 | 0x20;
        back = back & !0x57 | 0x20;
    } else if forward & 0x4000_0000 != 0 && back & 0x4000_0000 != 0 && forward & 0x40 == 0 {
        forward = forward & !0x36 | 0x41;
        back = back & !0x36 | 0x41;
    }
    m.diplomacy[ab] = forward;
    m.diplomacy[ba] = back;
}
/// Remove an object without a death (0x4ff350).
fn erase(sim: &mut Simulation, m: &mut MissionWorld, i: usize) {
    let p = [m.objects[i].x, m.objects[i].y];
    for j in std::mem::take(&mut sim.objects[i].contents) {
        sim.objects[j].holder = None;
        // Each one out through 0x504550 loses the mark (0x462c20).
        sim.objects[j].active = false;
        m.objects[j].flags &= !FLAG_HELD;
        position(&mut m.objects[j], p);
    }
    if let Some(holder) = sim.objects[i].holder.take() {
        sim.objects[holder].contents.retain(|&j| j != i);
    }
    sim.gameplay.actors[i] = Default::default();
    sim.objects[i].order = OrderState::Idle;
    m.objects[i].flags |= FLAG_INACTIVE;
}

impl Host for Ctx<'_> {
    fn call(&mut self, f: Function, a: [i32; 8], object: Option<usize>) -> i32 {
        let kingdom = |p: i32| (1..=8).contains(&p).then_some(p as u8);
        // @getgold… read what the kingdom may spend (0x4b8170 → 0x4677d0);
        // @setgold… write its own and read that back (0x4b8190 → 0x4678d0).
        let resource = |ctx: &Self, p: i32, r: usize| {
            kingdom(p)
                .filter(|&p| usize::from(p) <= ctx.m.kingdoms.len())
                .map_or(0, |p| stock(ctx.m, p)[r] as i32)
        };
        let set = |ctx: &mut Self, p: i32, r: usize, v: i32| {
            let Some(p) = kingdom(p).filter(|&p| usize::from(p) <= ctx.m.kingdoms.len()) else {
                return 0;
            };
            set_own(ctx.m, p, r, v);
            stock(ctx.m, p)[r] as i32
        };
        let word = |ctx: &Self, a: i32, b: i32| match (kingdom(a), kingdom(b)) {
            (Some(a), Some(b)) => relation(ctx.m, a, b) as i32,
            _ => 0,
        };
        match f {
            Function::Gold => resource(self, a[0], 0),
            Function::Stone => resource(self, a[0], 1),
            Function::Food => resource(self, a[0], 2),
            Function::Wood => resource(self, a[0], 3),
            Function::SetGold => set(self, a[0], 0, a[1]),
            Function::SetStone => set(self, a[0], 1, a[1]),
            Function::SetFood => set(self, a[0], 2, a[1]),
            Function::SetWood => set(self, a[0], 3, a[1]),
            Function::Rnd => {
                let span = (a[1] as i64 - a[0] as i64 + 1).max(1) as u64;
                (a[0] as i64 + random(self.sim, span) as i64) as i32
            }
            Function::Str => {
                self.st.text = a[0].to_string();
                1
            }
            Function::Min => a[0].min(a[1]),
            Function::Max => a[0].max(a[1]),
            Function::GetDiplomacy => word(self, a[0], a[1]),
            Function::IsAlly => (word(self, a[0], a[1]) & 0x40 != 0) as i32,
            Function::IsNeutral => (word(self, a[0], a[1]) & 0x20 != 0) as i32,
            Function::IsEnemy => (word(self, a[0], a[1]) & 0x10 != 0) as i32,
            Function::HasSharedVision => (word(self, a[0], a[1]) & 1 != 0) as i32,
            Function::HasSharedRes => (word(self, a[0], a[1]) & 2 != 0) as i32,
            Function::HasSharedUnits => (word(self, a[0], a[1]) & 4 != 0) as i32,
            // 0x4b8230, 0x4b8250: the kingdom's record of the type (0x4976a0;
            // the kingdom's byte under 12, 255 for 0): +0x2c8 how many there
            // are, sites and researches done included, +0x2cc how many are
            // under way, sites, training and researching.
            Function::Count | Function::InProgress => {
                let k = match a[0] as u8 {
                    255 => 0,
                    k if k < 12 => k,
                    _ => return 0,
                };
                let (made, under) =
                    super::ai::rules::record(self.sim, self.m, &self.c.ai, k, a[1] as u16);
                (if f == Function::Count { made } else { under }) as i32
            }
            Function::SetPopLimit => {
                if let Some(p) = kingdom(a[0]) {
                    let sum = housing_sum(self.m, self.lib, p);
                    self.m.population_limit.insert(p, a[1].max(0) as usize);
                    self.m.population_base.insert(p, sum);
                }
                a[1].max(0)
            }
            // 0x4b82e0: the AI command (0x404460) for an existing kingdom,
            // mode 0–3 = none, eco, war, full.
            Function::SetAi => {
                let k = a[0];
                let exists = k == 0
                    || (9..=11).contains(&k)
                    || (1..=8).contains(&k)
                        && self
                            .m
                            .kingdoms
                            .get(k as usize - 1)
                            .is_some_and(|x| x.active);
                let command = usize::try_from(a[1]).ok().and_then(|n| ai::COMMANDS.get(n));
                match (exists, command) {
                    (true, Some(command)) => {
                        if let Some(line) = ai::command(self.sim, self.m, k as u8, command)
                            && self.st.outbox.len() < 1024
                        {
                            self.st.outbox.push(line);
                        }
                        1
                    }
                    _ => 0,
                }
            }
            // 0x4b81d0: the attack pause of the computer players in the mask.
            Function::SetNoAttackTime => {
                ai::no_attack_time(self.sim, a[0] as u32, a[1] as u32);
                1
            }
            // 0x4b82b0: K+0x122 of an existing kingdom.
            Function::KingdomCanRescue => {
                let k = a[0];
                let exists = k == 0
                    || (9..=11).contains(&k)
                    || (1..=8).contains(&k)
                        && self
                            .m
                            .kingdoms
                            .get(k as usize - 1)
                            .is_some_and(|x| x.active);
                if !exists {
                    0
                } else {
                    if a[1] != 0 {
                        self.sim.gameplay.no_rescue.remove(&(k as u8));
                    } else {
                        self.sim.gameplay.no_rescue.insert(k as u8);
                    }
                    1
                }
            }
            // 0x4b86e3 → 0x42b520: the day clock to the hour given (the low
            // byte, under 24) and minute 0, its step (hour × 60) / 3; the
            // next one comes at the clock's usual time. Gives 1.
            Function::SetTime => {
                self.sim.set_hour(a[0] as u8);
                1
            }
            // 0x42b560: the hour of the day clock.
            Function::GetTime => (self.sim.clock_minutes() / 60) as i32,
            Function::Select | Function::Unselect => {
                if let Some(i) = object {
                    self.st.outbox.push(if f == Function::Select {
                        Show::Select(i)
                    } else {
                        Show::Unselect(i)
                    });
                }
                0
            }
            Function::ClearSelection => {
                self.st.outbox.push(Show::ClearSelection);
                0
            }
            Function::God => {
                if let Some(i) = object {
                    if a[0] != 0 {
                        self.m.objects[i].flags |= FLAG_GOD;
                    } else {
                        self.m.objects[i].flags &= !FLAG_GOD;
                    }
                }
                0
            }
            Function::Build => {
                if let Some(i) =
                    object.filter(|&i| self.m.objects[i].flags & FLAG_CONSTRUCTION != 0)
                {
                    let a = &mut self.sim.gameplay.actors[i];
                    a.construction = 0;
                    let o = &mut self.m.objects[i];
                    o.flags &= !FLAG_CONSTRUCTION;
                    o.health = o.max_health;
                }
                0
            }
        }
    }
    fn movie(&mut self, name: &str) {
        self.st.outbox.push(Show::Movie(name.to_owned()));
    }
    fn property(&mut self, i: usize, p: Property) -> i32 {
        let o = &self.m.objects[i];
        let s = &self.sim.objects[i];
        match p {
            Property::Health => o.health as i32,
            Property::MaxHealth => o.max_health as i32,
            Property::Attack => s.attack as i32,
            Property::Defence => s.defence as i32,
            Property::Level => s.level as i32,
            Property::Manna => s.mana as i32,
            Property::MaxManna => s.max_mana as i32,
            Property::Kingdom => o.owner as i32,
            Property::Race => o.civilization as i32,
            Property::X => o.x as i32,
            Property::Y => o.y as i32,
            Property::Cargo => {
                let cargo = self.sim.gameplay.actors.get(i).map_or([0; 4], |a| a.cargo);
                cargo.iter().sum::<u32>() as i32
            }
            // Flag 1 of the object: selected by the local player.
            Property::IsSelected => self.sim.gameplay.selected.contains(&i) as i32,
            Property::InHolder => s.holder.map_or(0, |h| self.m.objects[h].id as i32),
            Property::Program => s.order as i32,
            Property::Flags => (o.flags | self.sim.gameplay.selected.contains(&i) as u32) as i32,
            Property::UnderFog => !self.sim.visible(o) as i32,
        }
    }
    fn set_property(&mut self, i: usize, p: Property, v: i32) {
        let clamp = |v: i32, min: i32| v.clamp(min, u16::MAX as i32) as u16;
        match p {
            Property::Health => {
                if v <= 0 {
                    self.kill(i);
                } else {
                    // 0x4b8f50 writes +0x5c as it is, above the maximum too.
                    self.m.objects[i].health = clamp(v, 1);
                }
            }
            Property::MaxHealth => self.m.objects[i].max_health = clamp(v, 1),
            Property::Attack => self.sim.objects[i].attack = v.max(1) as u32,
            Property::Defence => self.sim.objects[i].defence = v.max(0) as u32,
            Property::Level => {
                let target = clamp(v, 0);
                let level = self.sim.objects[i].level;
                if target > level {
                    add_levels(self.sim, self.m, self.lib, i, target - level);
                } else {
                    self.sim.objects[i].level = target;
                }
            }
            Property::Manna => self.sim.objects[i].mana = clamp(v, 0),
            Property::MaxManna => self.sim.objects[i].max_mana = clamp(v, 0),
            _ => {}
        }
    }
    fn global(&mut self, g: Global) -> i32 {
        let local = self.m.player();
        let k = self.m.kingdoms.get(local as usize - 1);
        match g {
            Global::Gold | Global::Stone | Global::Food | Global::Wood if k.is_some() => {
                let r = match g {
                    Global::Gold => 0,
                    Global::Stone => 1,
                    Global::Food => 2,
                    _ => 3,
                };
                // $gold… are what the local kingdom may spend (0x4b8d73).
                stock(self.m, local)[r] as i32
            }
            Global::Difficulty => self.m.game_rules.difficulty as i32,
            _ => 0,
        }
    }
    fn set_global(&mut self, g: Global, v: i32) {
        let local = self.m.player();
        let k = self.m.kingdoms.get_mut(local as usize - 1);
        let r = match g {
            Global::Gold => 0,
            Global::Stone => 1,
            Global::Food => 2,
            Global::Wood => 3,
            Global::CanDeleteObjects => {
                self.st.no_delete = v == 0;
                return;
            }
            Global::CanUseDiplomacy => {
                self.st.no_diplomacy = v == 0;
                return;
            }
            Global::Difficulty => return,
        };
        if k.is_some() {
            set_own(self.m, local, r, v);
        }
    }
    fn event(&mut self, name: &str) -> Option<bool> {
        let script = self.script;
        let event = script.events.iter().find(|e| e.name == name)?;
        if self.depth > 8 {
            return Some(false);
        }
        self.depth += 1;
        let value = self.evaluate(&event.value);
        self.depth -= 1;
        Some(value)
    }
    fn variables(&mut self) -> &mut Variables {
        &mut self.st.variables
    }
}

/// The interface closed the message window.
pub fn close_window(sim: &mut Simulation, m: &mut MissionWorld) {
    let st = &mut sim.gameplay.script;
    if st.conversation.is_none() {
        st.modal = false;
        m.message = None;
    }
}

/// The player goes on with the conversation, choosing `choice` when phrases
/// are offered: the phrase's action runs (0x43806b), then its follow-ups.
pub fn answer(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    choice: usize,
) {
    let mut st = std::mem::take(&mut sim.gameplay.script);
    let script = std::mem::take(&mut m.script);
    {
        let mut ctx = Ctx {
            sim,
            m,
            g,
            lib,
            c,
            script: &script,
            st: &mut st,
            ids: HashMap::new(),
            depth: 0,
        };
        ctx.index();
        if let Some(talk) = ctx.st.conversation.clone() {
            match talk.phrase {
                None => {
                    let pick = talk.choices.get(choice).or(talk.choices.first()).copied();
                    match pick {
                        Some(p) => ctx.say(p),
                        None => ctx.offer(Vec::new()),
                    }
                }
                Some(p) => {
                    let phrase = ctx.m.conversations[talk.conversation].phrases[p].clone();
                    if !phrase.action.trim().is_empty() {
                        expr::eval(&expr::statements(&phrase.action), &mut ctx, None);
                    }
                    let phrases = &ctx.m.conversations[talk.conversation].phrases;
                    let next = phrase
                        .next
                        .iter()
                        .filter_map(|&n| phrases.iter().position(|q| q.index == n))
                        .collect();
                    ctx.offer(next);
                }
            }
        }
    }
    m.script = script;
    sim.gameplay.script = st;
}

/// A hero carried to the next map of a campaign (0x43ea20): its name and
/// subtitle, health, level and whether it was promoted (vt+0x128).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hero {
    pub name: String,
    pub subtitle: String,
    pub health: u16,
    pub max_health: u16,
    pub level: u16,
    pub promoted: bool,
}
/// 0x43ea20: the named objects whose record carries [`FLAG_CARRIED`], first
/// those of kingdoms 1–11 still alive, kingdom by kingdom in the order of
/// each kingdom's list, then those not on the map yet (0xcd9f18), which go
/// as promoted heroes.
pub fn heroes(sim: &Simulation, m: &MissionWorld) -> Vec<Hero> {
    let carried = |o: &MapObject| o.flags & FLAG_CARRIED != 0 && !o.name.is_empty();
    let hero = |i: usize, promoted: bool| {
        let o = &m.objects[i];
        Hero {
            name: o.name.clone(),
            subtitle: o.subtitle.clone(),
            health: o.health,
            max_health: o.max_health,
            level: sim.objects[i].level,
            promoted,
        }
    };
    // Each kingdom's units as its list holds them, the latest to join it
    // first (K+0x56).
    let on_map = (1..=11u8).flat_map(|k| {
        ai::think::kingdom_list(sim, m, k, true)
            .into_iter()
            .filter(|&i| {
                let o = &m.objects[i];
                carried(o) && o.flags & FLAG_DYING == 0
            })
    });
    let waiting = sim
        .gameplay
        .script
        .hidden
        .iter()
        .copied()
        .filter(|&i| carried(&m.objects[i]));
    on_map
        .map(|i| hero(i, m.objects[i].flags & FLAG_HERO != 0))
        .chain(waiting.map(|i| hero(i, true)))
        .collect()
}
/// The next map's loader (0x440c58): every object on the map whose record
/// carries [`FLAG_CARRIED`] takes, from the first hero with its name
/// without regard to case, name, subtitle, health, maximum and level (set as
/// is, attack and defence following it), with no experience, and the
/// promotion (vt+0x294, 0x505ed0).
pub fn restore_heroes(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    heroes: &[Hero],
) {
    sync(sim, m, lib);
    for i in 0..m.objects.len() {
        let o = &m.objects[i];
        if o.flags & (FLAG_CARRIED | FLAG_INACTIVE | FLAG_DEAD) != FLAG_CARRIED || o.name.is_empty()
        {
            continue;
        }
        let Some(hero) = heroes.iter().find(|h| h.name.eq_ignore_ascii_case(&o.name)) else {
            continue;
        };
        let o = &mut m.objects[i];
        o.name.clone_from(&hero.name);
        o.subtitle.clone_from(&hero.subtitle);
        o.max_health = hero.max_health.max(1);
        o.health = hero.health.clamp(1, o.max_health);
        if hero.promoted {
            // vt+0x12c (0x506d60) on an object that has its name.
            o.flags |= FLAG_HERO;
        }
        set_level(sim, m, lib, i, hero.level);
        sim.objects[i].xp = 0;
    }
}

#[cfg(test)]
#[path = "scripting_tests.rs"]
mod tests;
