//! Player-controlled gameplay. The catalog comes from the shipped tables,
//! orders, menus and civilization technology trees; no computer player issues orders.
use crate::{
    Result,
    assets::{GameAssets, Ini, legacy_text},
    map::MapGrid,
    simulation::{ObjectState, OrderState, Simulation},
    world::{
        self, CLASSES, FLAG_CARRIED, FLAG_CONSTRUCTION, FLAG_DEAD, FLAG_DYING, FLAG_GOD, FLAG_HELD,
        FLAG_HERO, FLAG_INACTIVE, FLAG_LOCKED, FLAG_STORY, MapObject, MissionWorld, ObjectLibrary,
        civ_section, class,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Duration,
};

pub const TICK_MS: u64 = 50;
const QUEUE_LIMIT: usize = 20;
/// Most units of one kind queued in a building (0x41ec63).
const JOB_TYPE_LIMIT: usize = 250;

pub mod ai;
pub mod cheats;
mod combat;
pub mod islands;
mod movement;
mod pathing;
mod scripting;
pub use movement::{
    Group, GroupPath, JUMP_PICTURE, Movement, Pathing, Walk, air_path, blocked_since, jump_frame,
    motion_offset, walk_progress,
};
pub use scripting::{
    END_DELAY_MS, Hero, ScriptState, Show, Trace, answer, close_window, heroes, restore_heroes,
};
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub cost: [u32; 4],
    pub mana_cost: u16,
    pub time_ms: u64,
    pub range: u32,
}
pub struct Catalog {
    pub orders: Ini,
    menus: Ini,
    trees: BTreeMap<String, Ini>,
    stats: BTreeMap<(String, String), Stats>,
    pub technologies: Ini,
    bonus: Ini,
    /// The hero table of HEROES.INI ([`hero_table`]).
    heroes: Vec<(String, u16)>,
    /// `[viewmsg] poplimit1` and `poplimit2` of STRINGS.INI.
    poplimit: [String; 2],
    /// `[viewmsg] notmanna`.
    notmanna: String,
    /// `[viewmsg] notres` for gold, stone, food and wood (0x4679b0).
    notres: [String; 4],
    /// `[viewmsg] magicgatewaycap`.
    gateway_taken: String,
    /// `[err] ctfcantspawn`: no free land for the flag (0x465bbd).
    no_flag_room: String,
    /// `[msg] resadded`, the line of resources received (0x48fc6a).
    resadded: String,
    /// The computer players' scripts.
    pub ai: ai::script::Library,
}
impl Catalog {
    pub fn load(a: &GameAssets) -> Result<Self> {
        let technologies = Ini::parse(&a.get("DATA/TECHNOLOGIES.INI")?);
        let upgrades = Ini::parse(&a.get("DATA/UPGRADES.INI")?);
        let mut stats: BTreeMap<(String, String), Stats> = BTreeMap::new();
        for civ in ["comm", "euro", "arab", "asia"] {
            for kind in ["BUILDINGS", "UNITS"] {
                let p = format!("DATA/{civ} {kind}.TXT");
                if !a.contains(&p) {
                    continue;
                }
                let mut columns: Vec<String> = Vec::new();
                for line in legacy_text(&a.get(&p)?).lines() {
                    let fields: Vec<_> = line.split('\t').map(str::trim).collect();
                    if matches!(fields.first(), Some(&("UNIT" | "BUILDING"))) {
                        columns = fields.iter().map(|s| s.to_ascii_uppercase()).collect();
                        continue;
                    }
                    let at = |k: &str| {
                        columns
                            .iter()
                            .position(|s| s == k)
                            .and_then(|i| fields.get(i).copied())
                    };
                    let Some(name) = at("CLASS").filter(|s| !s.is_empty()) else {
                        continue;
                    };
                    let s = stats
                        .entry((civ.into(), name.to_ascii_lowercase()))
                        .or_default();
                    if let Some(raw) = at("COST") {
                        let (cost, mana) = parse_cost(raw);
                        s.cost = cost;
                        s.mana_cost = mana;
                    }
                    if let Some(raw) = at("TIME") {
                        s.time_ms = raw.parse::<u64>().unwrap_or(0) * 1000;
                    }
                    if let Some(raw) = at("RANGE") {
                        s.range = raw.parse().unwrap_or(0);
                    }
                }
            }
        }
        for table in [&technologies, &upgrades] {
            for name in table.section_names() {
                let num = |k| {
                    table
                        .get(name, k)
                        .and_then(|n| n.trim().parse::<u64>().ok())
                        .unwrap_or(0)
                };
                stats.insert(
                    ("comm".into(), name.to_ascii_lowercase()),
                    Stats {
                        cost: [
                            num("gold") as u32,
                            num("stone") as u32,
                            num("food") as u32,
                            num("wood") as u32,
                        ],
                        mana_cost: num("manna") as u16,
                        time_ms: num("time"),
                        ..Default::default()
                    },
                );
            }
        }
        Ok(Self {
            orders: Ini::parse(&a.get("DATA/ORDERS.INI")?),
            menus: Ini::parse(&a.get("DATA/MENU.INI")?),
            trees: ["euro", "arab", "asia"]
                .into_iter()
                .map(|c| {
                    Ok((
                        c.into(),
                        Ini::parse(&a.get(&format!("DATA/TECHTREE {c}.INI"))?),
                    ))
                })
                .collect::<Result<_>>()?,
            stats,
            technologies,
            bonus: Ini::parse(&a.get("DATA/BONUS.INI")?),
            heroes: hero_table(&legacy_text(&a.get("DATA/HEROES.INI")?)),
            poplimit: {
                let locale = Ini::locale(a)?;
                ["poplimit1", "poplimit2"].map(|k| locale.label("viewmsg", k))
            },
            notmanna: Ini::locale(a)?.label("viewmsg", "notmanna"),
            notres: {
                let locale = Ini::locale(a)?;
                let text = locale.label("viewmsg", "notres");
                ["gold", "stone", "food", "wood"].map(|k| {
                    text.replace("%s", &locale.label("viewmsg", k))
                        .trim()
                        .to_owned()
                })
            },
            gateway_taken: Ini::locale(a)?.label("viewmsg", "magicgatewaycap"),
            no_flag_room: Ini::locale(a)?.label("err", "ctfcantspawn"),
            resadded: Ini::locale(a)?.label("msg", "resadded"),
            ai: ai::script::Library::load(a)?,
        })
    }
    pub fn stats(&self, o: &MapObject, name: &str, researches: &[String]) -> Stats {
        let civ = civ_section(o.civilization);
        let mut s = self
            .stats
            .get(&(civ.into(), name.into()))
            .or_else(|| self.stats.get(&("comm".into(), name.into())))
            .cloned()
            .unwrap_or_default();
        // A research completed twice cuts the record twice (0x4d33d0).
        let has = |n: &str| crate::simulation::completions(researches, n);
        // Each handler multiplies the record in place, truncating: Feudalism
        // and Slavery both cut the peasant's food (0x4d4159), 50 → 40 → 32;
        // Horsemanship every cost of the cavalry (0x4d3a2d), Meditation the
        // Asian monk's (0x4d3d7d).
        let cuts: &[(&str, u64, bool)] = match name {
            "peon" => &[("te_feudalism", 80, true), ("te_slavery", 80, true)],
            "horse_archer" | "heavy_cavalry" => &[("te_horsemanship", 75, false)],
            "monk" if civ == "asia" => &[("te_meditation", 75, false)],
            _ => &[],
        };
        for &(research, percent, food_only) in cuts {
            for _ in 0..has(research) {
                for (i, cost) in s.cost.iter_mut().enumerate() {
                    if !food_only || i == 2 {
                        *cost = (*cost as u64 * percent / 100) as u32;
                    }
                }
            }
        }
        if has("te_architecture") > 0
            && world::CLASSES
                .iter()
                .position(|c| *c == name)
                // 0x4d347e: every class whose INI says `type = building`, the
                // training dummy included.
                .is_some_and(|k| (39..=73).contains(&k) || k == class::DUMMY)
        {
            for _ in 0..has("te_architecture") {
                s.time_ms = s.time_ms * 70 / 100;
            }
        }
        s
    }
    /// The research an order without a class needs (`[orders]` of the
    /// TECHTREE files): one per order, loaded euro, arab, asia, the last
    /// winning (0x496362, 0x4975b7).
    pub fn order_research(&self, key: &str) -> Option<&str> {
        ["asia", "arab", "euro"]
            .iter()
            .find_map(|c| self.trees.get(*c)?.get("orders", key))
    }
    pub fn action(&self, key: &str) -> String {
        self.orders
            .get(key, "action")
            .unwrap_or(key)
            .trim()
            .to_ascii_lowercase()
    }
    pub fn class_name(&self, key: &str) -> String {
        self.orders
            .get(key, "classname")
            .unwrap_or(key)
            .trim()
            .to_ascii_lowercase()
    }
    fn group(&self, key: &str, o: &MapObject, depth: usize) -> bool {
        if depth > 8 {
            return false;
        }
        let name = key.trim();
        if let Some((prefix, class)) = name.split_once(' ') {
            let civ = match prefix {
                "e" => 1,
                "r" => 2,
                "a" => 3,
                _ => 0,
            };
            if civ != 0 {
                return o.civilization == civ && o.class_name() == class.trim();
            }
        }
        o.class_name() == name
            || self
                .bonus
                .get("groups", name)
                .is_some_and(|s| s.split(',').any(|k| self.group(k, o, depth + 1)))
    }
    /// The BONUS.INI entry of `a` against `b` in the matrix of 0x49a090: its
    /// value and whether it is a percentage.
    fn bonus(&self, a: &MapObject, b: &MapObject) -> Option<(i16, bool)> {
        let mut selected = None;
        for section in self.bonus.section_names().filter(|s| *s != "groups") {
            if !self.group(section, a, 0) {
                continue;
            }
            for (key, value) in self.bonus.entries(section) {
                if !self.group(key, b, 0) {
                    continue;
                }
                selected = Some(value.trim());
            }
        }
        // Native 0x499ff1 assigns a single matrix entry. Later overlapping
        // BONUS.INI rules overwrite it; the getter does not sum groups.
        let value = selected?;
        let amount = value.trim_end_matches('%').parse::<i64>().unwrap_or(0);
        Some((amount as i16, value.ends_with('%')))
    }
    /// 0x4dd983: the blow received by `b` from `a`, both of a class below 73,
    /// grows by the matrix entry, a percentage or a fixed amount, in 16 bits.
    fn received(&self, a: &MapObject, b: &MapObject, amount: u16) -> u16 {
        if a.class >= 73 || b.class >= 73 {
            return amount;
        }
        match self.bonus(a, b) {
            Some((pct, true)) => amount.wrapping_add((amount as i32 * pct as i32 / 100) as u16),
            Some((value, false)) => amount.wrapping_add(value as u16),
            None => amount,
        }
    }
    /// Whether an order's refusal is a line of the original, which its
    /// message list shows: «notres» (0x4679b0) and «notmanna» (0x4e055b),
    /// said by the player's object (vt+0x40, 0x501470). The other refusals
    /// of this version the original leaves unsaid.
    pub fn said(&self, text: &str) -> bool {
        !text.is_empty() && (text == self.notmanna || self.notres.iter().any(|t| t == text))
    }
    pub fn targeted(&self, key: &str) -> bool {
        let a = self.action(key);
        a.starts_with("build ")
            || a.starts_with("spell ")
            // PLACE = crusadeanchor: the cathedral points where it gathers.
            || a.eq_ignore_ascii_case("create crusadeanchor")
            || matches!(
                a.as_str(),
                "move"
                    | "move_advance"
                    | "move_sneak"
                    | "attack"
                    | "attack_ground"
                    | "repair"
                    | "capture"
                    | "harvest"
                    | "dig"
                    | "chop"
                    | "guard"
                    | "patrol"
                    | "explore"
                    | "pick_item"
                    | "fishing"
                    | "murder"
                    | "bribe"
                    | "impersonate"
                    | "unload"
                    | "item_drop"
                    | "item_use"
            )
    }
    fn permitted(&self, key: &str, o: &MapObject, lib: &ObjectLibrary) -> bool {
        // 0x4d1f91: the tavern shows loan_return in the place of the loan
        // submenu while a loan is outstanding.
        let key = if key.eq_ignore_ascii_case("loan_return") {
            "loan"
        } else if key.eq_ignore_ascii_case("fireatwill") {
            // tower_stone3 shows fire at will in the place of hold fire.
            "holdfire"
        } else {
            key
        };
        // A hotkey or submenu cannot grant orders absent from this class's native menu.
        let Some(menu) = lib.property(o, "menu") else {
            return false;
        };
        let mut pending = vec![menu.to_owned()];
        // The native ship getter changes its menu with the cannon level.
        // Execution checks the specific next upgrade against runtime state.
        if menu == "e_galleon" {
            pending.extend((2..=6).map(|n| format!("e_galleon{n}")));
        }
        if o.class == 14 {
            pending.push("spy_add".into());
        }
        let mut seen = BTreeSet::new();
        while let Some(menu) = pending.pop() {
            if !seen.insert(menu.to_ascii_lowercase()) {
                continue;
            }
            for (order, _) in self.menus.entries(&menu) {
                if order.eq_ignore_ascii_case(key) {
                    return true;
                }
                if let Some(sub) = self.orders.get(order, "submenu") {
                    pending.push(sub.to_owned());
                }
            }
        }
        false
    }
    fn available(
        &self,
        key: &str,
        o: &MapObject,
        m: &MissionWorld,
    ) -> std::result::Result<(), String> {
        let Some(r) = m.rules.get(o.owner.wrapping_sub(1) as usize) else {
            return Err("Regno non valido".into());
        };
        let name = self.class_name(key);
        let full = self.action(key);
        let action = full.split_whitespace().next().unwrap_or("");
        // «notech» (0x494bbf, 0x494c90): neither the forbidden lists nor
        // what the type depends on are asked; a research done stays done.
        let free = m.cheats.no_tech;
        if !free && forbidden(r, &name) {
            return Err("Ordine vietato in questa missione".into());
        }
        if action == "technology"
            && r.initial_researches
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&name))
        {
            return Err("Ricerca già completata".into());
        }
        let known = |dep: &str| {
            r.initial_researches
                .iter()
                .any(|s| s.eq_ignore_ascii_case(dep.trim()))
        };
        if free {
            return Ok(());
        }
        // 0x496362: `[orders]` is one per order, the last tree loaded wins.
        if let Some(dep) = self.order_research(key)
            && !known(dep)
        {
            return Err(format!("Richiede {dep}"));
        }
        if let Some(tree) = self.trees.get(civ_section(o.civilization)) {
            if let Some(dep) = tree.get("depend", &name)
                && !known(dep)
            {
                return Err(format!("Richiede {dep}"));
            }
            if let Some(dep) = tree.get("required", &name) {
                if !m.objects.iter().any(|x| {
                    x.owner == o.owner
                        && alive(x)
                        && x.flags & FLAG_CONSTRUCTION == 0
                        && x.class_name().eq_ignore_ascii_case(dep.trim())
                }) {
                    return Err(format!("Richiede {dep}"));
                }
            }
        }
        Ok(())
    }
}
/// The four forbidden lists of a scenario mark the same flag of the named
/// record (0x440ef3, 0x494ba0): a name forbids wherever it is listed.
pub fn forbidden(r: &world::KingdomRules, name: &str) -> bool {
    [
        &r.forbidden_buildings,
        &r.forbidden_units,
        &r.forbidden_researches,
        &r.forbidden_upgrades,
    ]
    .iter()
    .any(|list| list.iter().any(|s| s.eq_ignore_ascii_case(name)))
}
/// A research some building of the kingdom is running now (+0x2cc,
/// 0x41fc75 to 0x41f396).
pub fn researching(sim: &Simulation, m: &MissionWorld, owner: u8, name: &str) -> bool {
    sim.gameplay.actors.iter().enumerate().any(|(k, a)| {
        m.objects.get(k).is_some_and(|o| o.owner == owner)
            && matches!(a.jobs.front(), Some(Job { kind: JobKind::Research(n), .. }) if n.eq_ignore_ascii_case(name))
    })
}
fn parse_cost(raw: &str) -> ([u32; 4], u16) {
    let mut cost = [0; 4];
    let mut mana = 0;
    let mut n = String::new();
    for c in raw.chars() {
        if c.is_ascii_digit() {
            n.push(c)
        } else if let Some(i) = match c.to_ascii_lowercase() {
            'g' => Some(0),
            's' => Some(1),
            'f' => Some(2),
            'w' => Some(3),
            'm' => Some(4),
            _ => None,
        } {
            let value = n.parse::<u32>().unwrap_or(0);
            if i == 4 {
                mana = value.min(u16::MAX as u32) as u16
            } else {
                cost[i] = value;
            }
            n.clear();
        }
    }
    (cost, mana)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Intent {
    Move([u16; 2]),
    /// A Move given an object and the unit's RANGE (op 9 with +0xc and
    /// +8), of the Attack program a Shift list makes (0x4e3979): the walk
    /// ends within that range of the rect the object has when it starts.
    Reach(usize),
    Explore([u16; 2]),
    Attack(usize),
    AttackGround([u16; 2]),
    Cast {
        key: String,
        cell: [u16; 2],
        target: Option<usize>,
    },
    Harvest(usize),
    Chop([u16; 2]),
    Build(usize),
    /// A computer player's builder walking to a site still to be laid
    /// («AI far build», program 0x19, 0x42e570).
    FarBuild(u32),
    /// A computer player's fighter sent to a place (Approach, program
    /// 0x28, 0x42dde0).
    Approach(ai::war::Approach),
    /// A computer player's ship carrying its passengers (Transport,
    /// program 0x2b, 0x430200).
    Transport(Box<ai::transport::Transport>),
    /// A computer player's unit going by ship (Use transport, program
    /// 0x2c, 0x4306a0).
    UseTransport(ai::transport::Ride),
    Repair(usize),
    Capture(usize),
    Bribe(usize),
    Pick(usize),
    /// Pick an item up, then use it at once (Ctrl+right click, program 0x41).
    PickUse(usize),
    /// Use a held item on a target (program 17, 0x42eab9): walk within the
    /// item's range, then apply it.
    UseItem {
        item: usize,
        target: Option<usize>,
        cell: [u16; 2],
    },
    /// Drop a held item on a cell or give it to a unit (program 16, 0x42e993).
    DropItem {
        item: usize,
        target: Option<usize>,
        cell: [u16; 2],
    },
    Guard(usize),
    Patrol([u16; 2], [u16; 2]),
    /// A patrol through several Shift points (0x42f7d0): the next point, and
    /// whether the last leads back to the first.
    Circuit(Vec<[u16; 2]>, usize, bool),
    Board(usize),
    Trade(usize),
    /// Return goods (program 0x35) to a chosen store.
    Deliver(usize),
    /// Unload Resource (program 0x21): walk to a market, harbor or castle,
    /// then sell the load there (op 0x34 without a route).
    Sell(usize),
    /// Unload units (program 0x16): sail to the point, then unload ashore.
    Unload([u16; 2]),
    /// A flight (Runaway, program 0x3f, 0x430be0): walk to the point, then
    /// rest so many ms.
    Flee([u16; 2], u32),
    /// The ninja's single deadly blow (program 0x2d, op 0x5b).
    Murder(usize),
    /// The spy goes into a unit (program 0x2f, op 0x60).
    Impersonate(usize),
    /// A prisoner of owner 9 waiting to be freed (program 0x3d, op 0x7e):
    /// the next check of its 3×3 in ms.
    Rescue(u64),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum JobKind {
    Train(usize),
    Research(String),
    Upgrade(String, Option<usize>),
    /// Magic workshop item (0x4742e0): category, index and gold paid.
    Item(u8, u8, u32),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Job {
    pub kind: JobKind,
    /// Milliseconds of TIME still to run and in all, read when the step
    /// starts (0x41fc29), and the next 666 ms tick of the step.
    pub remaining: u64,
    pub total: u64,
    pub paid: [u32; 4],
    #[serde(default)]
    pub due_ms: Option<u64>,
    /// The population block was already announced (step flag +8).
    #[serde(default)]
    pub waiting: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Actor {
    pub intent: Option<Intent>,
    pub queued: VecDeque<Intent>,
    pub path: VecDeque<[u16; 2]>,
    /// Where the current walk leads, for the destination marker.
    pub goal: Option<[u16; 2]>,
    /// The Move op under way, and the step its picture is making.
    pub movement: Option<Movement>,
    pub walk: Option<Walk>,
    /// Steps started since the unit set off (the walking frames count on).
    pub walk_cells: u32,
    /// The unit's cell is marked in transit (bits 26–28).
    pub transit: bool,
    /// The cell this unit waits to enter behind another (+0xea).
    pub waiting_for: Option<[u16; 2]>,
    /// The group it was ordered with (+0xe6).
    pub group: Option<u32>,
    pub cooldown: u32,
    /// Capture and other work must not inherit the previous attack/work timer.
    pub capture_target: Option<usize>,
    pub capture_cooldown: u32,
    pub work_target: Option<usize>,
    pub work_cell: Option<[u16; 2]>,
    pub work_cooldown: u32,
    /// Active animation time; paused/frozen workers do not advance this clock.
    pub action_elapsed_ms: u64,
    pub action_image: String,
    pub action_point: Option<[u16; 2]>,
    /// Cannon upgrade (+0x146), independent of the unit's experience level.
    pub ship_level: u8,
    pub cargo: [u32; 4],
    pub cargo_source: usize,
    pub depositing: Option<usize>,
    pub jobs: VecDeque<Job>,
    pub production_blocked: Option<String>,
    pub rally: Option<[u16; 2]>,
    /// The rally program (+0x124): 0 plain walk (0x33), 1 default action
    /// (0x3e), 2 advance (0x40).
    pub rally_mode: u8,
    pub construction: u64,
    pub construction_total: u64,
    pub direction: u8,
    pub stance: u8,
    pub hold_fire: bool,
    pub frozen_until: u64,
    pub effect_until: u64,
    pub disguise: Option<Disguise>,
    #[serde(default)]
    pub path_cooldown: u32,
    pub trade_origin: Option<usize>,
    pub trade_partner: Option<usize>,
    pub debt: [u32; 4],
    pub effects: Vec<Effect>,
    /// Attack cycle (0x4e24bc / 0x4df800): time of the last blow, the next
    /// blow at `actionrow`, and the wait before restarting the animation, in ms.
    pub last_hit_ms: u64,
    pub attack_due_ms: u64,
    pub attack_wait_ms: u64,
    /// `image_die` plays until this tick, then the corpse is removed (0x4e2110).
    pub dying_until: u64,
    /// Move program: 0 Move, 1 Move Advance (Ctrl), 2 move sneak (Alt).
    pub move_mode: u8,
    /// "Attacca tipo" (Alt + right click): preferred class of the idle scan.
    pub attack_type: Option<usize>,
    /// Engagement started by the unit itself (idle scan or reaction), and the
    /// order it interrupted, resumed afterwards (0x413800).
    pub automatic: bool,
    pub resume: Option<Intent>,
    /// Hypnosis (effect 13) copies the caster's relations without a new owner.
    pub allegiance: Option<u8>,
    /// Field work: native countdown of 12–18 cycles per load (0x44cc70).
    pub field_cycles: u8,
    /// A soul gem's bound unit (0x473403), a whirlpool's caster (0x4fce90).
    pub bound: Option<usize>,
    /// Moves a whirlpool has left before it vanishes (+0xc6, 15 at the start).
    pub moves_left: u8,
    /// A tavern's resource exchange in progress.
    pub exchange: Option<Exchange>,
    /// A tavern's offers of mercenaries and of heroes (submenus +0x158 and
    /// +0x15c, at most eight each, oldest first) and the next refill of a
    /// tavern or a market (messages 0x4204, 0x4209).
    pub hires: [Vec<Hire>; 2],
    pub hire_due: Option<u64>,
    /// A trader's route.
    pub route: Option<Route>,
    /// A cathedral's next crusader tick (message 0x4219).
    pub crusade_tick: u64,
    /// The tick the object came into play: its own timers run from it
    /// (0x4d4790, due = now + period), unlike the shared tickers of
    /// 0x4312b0.
    pub born: u64,
    /// The unit could move when its Idle started (the step's +8, 0x4e1bc7):
    /// from the third idle tick it moves again (0x4e4ebb). An Idle after an
    /// order holds the place till then (0x4e1bd3); stand_ground keeps it
    /// until the next order.
    pub temporary_hold: bool,
    /// Idle time (+0xe2), grown by 1500 at each idle tick, and the next idle
    /// tick or tower look.
    pub idle_ms: u32,
    pub scan_due_ms: Option<u64>,
    /// When the unit last stood idle (the Idle op restarts `image_idle`).
    pub idle_since_ms: u64,
    /// The next look of an advancing move or a patrol leg (message 0x4011).
    pub advance_due_ms: Option<u64>,
    /// The first object this unit guards (vt+0x23c, the head of +0xca):
    /// the one its idle scan walks back to.
    pub guarding: Option<usize>,
    /// Every object it guards (+0xca), whose alarms it answers, in the
    /// order the order registered them (0x4e3d65).
    pub guard_list: Vec<usize>,
    /// The class it guards with Alt (+0xce, vt+0x250), none at 0.
    pub guard_class: u16,
    /// Last call for help of this unit (+0xd2, at most every 100 ms).
    pub alarm_tick: u64,
    /// Spells learned from books (0x234, the unit's own menu `spells_%d`).
    pub spells: Vec<u8>,
    /// A spy's disguise energy (+0xf6 +0xa): 100 at first, +1 a second.
    pub spy_energy: u8,
    /// The spy living in this unit (0x4e4510), to come out again.
    pub mole: Option<Mole>,
    /// The 1000 ms heal of a castle's or outpost's garrison (message 0x4211).
    pub heal_due_ms: Option<u64>,
    /// The next build, repair or capture cycle, in ms.
    pub work_due_ms: u64,
    /// A damaged building's fire level (+0x10c), its fires in map pixels
    /// (two small, then a big one) and the next `fire` sound (message 0x2213).
    pub fire_level: u8,
    pub fires: [Option<[i32; 2]>; 3],
    pub fire_due_ms: Option<u64>,
    /// The cast picture plays to its end before the order goes on (0x4e0173).
    pub pose_until: Option<u64>,
    /// When the spell op runs next (ms): the picture's `actionrow`, then a
    /// whole `anim_time` after each repeated cast (0x4e00a0).
    pub cast_due_ms: u64,
    /// A Delay step of a work program (2 s after a failed walk to a tree,
    /// 4 s after one to a store): the tick when the program goes on.
    pub search_at: Option<u64>,
    /// The Explore program's Move target: the cell op 0x41 found
    /// (0x4e29f5), none at first, when it is the program's point.
    pub explore_target: Option<[u16; 2]>,
    /// The Dig program of a computer's field (0x40f7b4 passes 1): after each
    /// delivery op 0x25 looks for a field again (step 7 of 0x42e600).
    pub dig_again: bool,
    /// The object a Move was given on (0x503fa0 → 0x42dd00) and the rect
    /// it took from it at the start (vt+0x134, a field's vt+0x130).
    pub move_target: Option<(usize, [i32; 4])>,
    /// A gate's drawbridge on the move (op 0x1a, the picture row +0x154).
    pub gate_motion: Option<GateMotion>,
    /// 500 ms steps before a gate's automatism resumes after an order (+0x166).
    pub gate_hold: u16,
    /// The tick a gate came into service: its 500 ms timer (message 0x4200)
    /// runs from then (0x456173).
    pub gate_since: u64,
    /// A ninja's attack ordered on a unit with 60 mana: its Attack op is
    /// Murder (0x4eb950) until it strikes or its mana falls (0x4eb9d0).
    pub murder_blow: bool,
    /// The default program of a cow or a bear (vt+0x178): Cow Idle
    /// (0x42f450) or Bear Idle (0x42fea0).
    pub pasture: Option<Pasture>,
    /// The tick a summoned or raised creature's Animate program ends.
    pub appearing_until: u64,
    /// Rubble (class 87): its next frame, and whether it stays on its last.
    pub rubble: Option<Rubble>,
    /// A ninja hidden on its own mana (+0x124).
    pub cloak_drain: bool,
    /// The 8×8 sector the object stands in, and when it came in: the game
    /// millisecond and the arrivals' count.
    pub sector: Option<[u16; 2]>,
    pub sector_stamp: u64,
    #[serde(default)]
    pub sector_ms: u64,
    /// When it joined its kingdom's list (0x467450), counted by
    /// `Runtime::joins`: the list goes from the latest; the objects of the
    /// map joined in their order.
    pub joined: u64,
    /// The orders it was given (a new program, 0x4ae5e0 for a peasant).
    pub orders: u32,
    /// A wall marked for the computer's breaches (+0x160, 0x417150).
    pub marked: bool,
    /// The priority of the program that ended or gave way last (+0xe0,
    /// vt+0x17c = 0x4e50e0).
    pub last_priority: u16,
    /// The Idle under way was installed by an order (Stop, stand ground,
    /// hold steady: vt+0x160 with 4000, 0x50389e), not the default one of
    /// priority 1000 (0x42df09).
    #[serde(default)]
    pub ordered_idle: bool,
    /// It came into a new cell during its update (0x4de310 runs after it).
    #[serde(skip)]
    pub entered: bool,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rubble {
    pub due_ms: u64,
    pub lasting: bool,
}
/// An animal's own program: Idle, Wander to a random cell, Move, round and
/// round (see `pasture`).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pasture {
    /// The cell the program was made on: a cow wanders within 2 of it.
    pub home: [u16; 2],
    /// The walk under way is the program's own Move.
    pub wandering: bool,
    /// The Wander op is pending: its next draw of a cell.
    pub wander_due: Option<u64>,
    /// The cow's Idle op (0x4ed100, 0x4ecb13): its step (+0xa), the cycles
    /// left (+0xc), the picture shown (`GRAZE`), played backwards from its
    /// last frame (+0x122), since when, and the next tick.
    pub step: u8,
    pub left: u8,
    pub picture: u8,
    pub backwards: bool,
    pub since_ms: u64,
    pub due_ms: u64,
}
/// The cow's grazing pictures (0x4ef250): standing, lowering or raising
/// its head, eating.
pub const GRAZE: [&str; 3] = ["cow_idle", "cow_down", "cow_eat"];
/// The drawbridge raising (rows 6 → 0) or lowering (0 → 6): one row every
/// 100 ms from `row` by `step` until `end`, then back one (0x456f40, 0x4573dc).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GateMotion {
    pub row: i8,
    pub end: i8,
    pub step: i8,
    pub due_ms: u64,
}
/// The record of a spy gone into another unit.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mole {
    pub kingdom: u8,
    pub civilization: u8,
    /// Its name and subtitle (+0xc, +0x4c of the record, 0x4ec1a4).
    pub name: String,
    #[serde(default)]
    pub subtitle: String,
    pub flags: u32,
    pub health: u16,
    pub max_health: u16,
    pub data: Vec<u8>,
    pub level: u16,
    pub xp: u16,
    pub attack: u32,
    pub defence: u32,
    pub sight: u16,
    pub energy: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Disguise {
    pub class: usize,
    pub civilization: u8,
    pub owner: u8,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Effect {
    pub until: u64,
    pub attack: i32,
    pub defence: i32,
    /// Native effect number (bitmask +0x6e): 5 blessing, 6 freeze,
    /// 7 discord, 13 hypnosis, 14 stone, 20 curse; 0 for potions.
    #[serde(default)]
    pub kind: u8,
    /// Discord keeps the original owner as the timer parameter (0x4e58e9).
    #[serde(default)]
    pub owner: u8,
    /// Values a curse or a freeze saved, plus one (0x4e5841, 0x4e5882); they
    /// come back when the effect ends.
    #[serde(default)]
    pub saved: u32,
}
pub mod effect {
    pub const BLESS: u8 = 5;
    pub const FREEZE: u8 = 6;
    pub const DISCORD: u8 = 7;
    pub const HYPNOSIS: u8 = 13;
    pub const STONE: u8 = 14;
    pub const CURSE: u8 = 20;
}
/// The special kingdom of discorded units (0x5050a0), hostile to everybody.
pub const DISCORD_OWNER: u8 = 10;
/// A frozen or petrified object: its pictures stop (0x4e5fa0).
pub fn still(sim: &Simulation, i: usize) -> bool {
    has_effect(sim, i, effect::FREEZE) || has_effect(sim, i, effect::STONE)
}
pub(crate) fn has_effect(sim: &Simulation, i: usize, kind: u8) -> bool {
    sim.gameplay.actors.get(i).is_some_and(|a| {
        a.effects
            .iter()
            .any(|e| e.kind == kind && e.until > sim.ticks)
    })
}
/// A native effect: a new application restarts the timer and keeps the
/// parameter of the first one (0x4e57e0).
/// The messiah's own vt+0x154 (0x4ef200) refuses freeze, discord, hypnosis,
/// stone and curse.
fn immune(o: &MapObject, kind: u8) -> bool {
    o.class == class::MESSIAH
        && matches!(
            kind,
            effect::FREEZE | effect::DISCORD | effect::HYPNOSIS | effect::STONE | effect::CURSE
        )
}
fn add_timed_effect(actor: &mut Actor, effect: Effect) {
    if let Some(current) = actor.effects.iter_mut().find(|e| e.kind == effect.kind) {
        current.until = effect.until;
    } else if actor.effects.len() < 32 {
        actor.effects.push(effect);
    }
}
/// Discord (effect 7, 20 s, 0x4e58e9): the unit joins the special kingdom
/// 10 and returns to its own owner when the effect ends.
fn discord(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary, j: usize) {
    if immune(&m.objects[j], effect::DISCORD) {
        return;
    }
    let refresh = has_effect(sim, j, effect::DISCORD);
    let until = sim.ticks + 400;
    add_timed_effect(
        &mut sim.gameplay.actors[j],
        Effect {
            until,
            kind: effect::DISCORD,
            owner: m.objects[j].owner,
            ..Default::default()
        },
    );
    if !refresh {
        transfer_owner(sim, m, lib, j, DISCORD_OWNER);
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Runtime {
    pub actors: Vec<Actor>,
    /// The sector lists' arrivals (0x5001a0, 0x500da0): a count bumped for
    /// each object that comes into an 8×8 sector, which goes to the head of
    /// its list.
    pub sector_clock: u64,
    pub resources: BTreeMap<usize, u32>,
    pub trees: BTreeMap<usize, u32>,
    /// Felled trees not yet turned into trunks (flag 0x20000000 with the
    /// tree's image, which nothing draws): their cell has no tree left.
    pub depleted_trees: BTreeSet<usize>,
    /// Felled trees turned into trunks during the game (0x4555bf, 0x453efc).
    pub stumps: BTreeSet<usize>,
    /// The fields and bridges standing at the last step, whose trees fall
    /// (0x44c930, 0x41c010).
    pub grounds: Vec<bool>,
    /// The game stream and the stream of sounds and pictures.
    pub game_rng: crate::simulation::NativeRng,
    pub aux_rng: crate::simulation::NativeRng,
    /// Clear and rain durations (ms) drawn when the game starts (0x4fc260).
    pub weather_pairs: Vec<[u32; 2]>,
    /// Real time into the current 50 ms turn, and game time already given
    /// for it (ns, 0x48cda7).
    pub scaled_remainder: u64,
    pub turn_game: u64,
    pub visions: Vec<Vision>,
    pub consumed_corpses: BTreeSet<usize>,
    /// The ten most recent events of the player (0x9ee338), newest first.
    pub events: VecDeque<GameEvent>,
    /// Missiles in flight: their damage lands on impact.
    pub projectiles: Vec<Projectile>,
    /// Tick of the last work on each field; idle fields decay (0x44d1e0).
    pub fields: BTreeMap<usize, u64>,
    /// Each kingdom's Tournaments pool (+0x112).
    pub kungfu: BTreeMap<u8, u16>,
    /// Loans and crusades of each kingdom.
    pub treasury: BTreeMap<u8, Treasury>,
    /// Meat of each cow (+0x120), 300 until its first tick.
    pub meat: BTreeMap<usize, u16>,
    /// The number of the next tavern or market offer.
    pub next_hire: u32,
    /// The Asian market's items of each kingdom (Jewelry).
    pub jewels: BTreeMap<u8, Jewels>,
    /// Items in the order they went into a holder: each pick adds its
    /// `sellitem_<id>` to the `market_sell` menu (0x4629e3).
    pub picks: Vec<usize>,
    /// Potions and books a computer's unit has just taken up, which it
    /// drinks, learns or uses before the step ends (0x4629aa, 0x463d90).
    #[serde(default)]
    pub ai_taken: Vec<usize>,
    /// When each lightning bolt alive goes (their count, 0x60a634, sets the
    /// segments and outer lines of new ones).
    pub lightning: Vec<u64>,
    /// The first net drawn on each fish patch (+0xcb), R(0, 2) when a boat
    /// starts fishing on an unfished patch (0x451090).
    pub nets: BTreeMap<usize, u8>,
    /// Each kingdom's raw score (0x443430), shown as (score + 50) / 100.
    pub score: BTreeMap<u8, u64>,
    /// The flag of the capture the flag game (0x608b4c): the first `item_flag`
    /// to come into play, and each one put back after it scored.
    #[serde(default)]
    pub flag: Option<usize>,
    /// When each of the map's sounds (`MissionWorld::sounds`) is due on the
    /// game's clock, in milliseconds (its timer of message 0x2009); none
    /// once it stopped.
    #[serde(default)]
    pub map_sounds: Vec<Option<u64>>,
    /// Kingdoms that may not rescue prisoners nor take ruins and magic
    /// gateways: K+0x122, on from the kingdom's creation (0x467388) and
    /// cleared by `@kingdomcanrescue` (0x4b82b0).
    pub no_rescue: BTreeSet<u8>,
    /// The mission scripts and the end of the game.
    pub script: ScriptState,
    /// The computer players.
    #[serde(default = "ai::State::missing")]
    pub ai: ai::State,
    /// The map of islands (0x60a630); empty until the first step builds it.
    pub islands: islands::Islands,
    /// What the step before saw of each object, for the hooks of the
    /// islands.
    pub census: Vec<islands::Seen>,
    /// Cell bits and regions of the pathfinder.
    pub pathing: Pathing,
    /// Groups of units ordered together, by id.
    pub groups: BTreeMap<u32, Group>,
    /// Wooden wall rows being raised to stone, by the segment whose queue
    /// they share (+0x14c, 0x4fa580): every segment of the row.
    pub wall_rows: BTreeMap<usize, Vec<usize>>,
    /// The objects that joined a kingdom since the map (see `Actor::joined`).
    pub joins: u64,
    /// Set while the pool hands out its points: no Body Building then (0x59085c).
    #[serde(skip)]
    pub distributing: bool,
    #[serde(skip)]
    pub notices: Vec<String>,
    /// A line for the status line at the bottom of the map and how many
    /// milliseconds it stays (0x4f8350), until a hint takes its place.
    #[serde(skip)]
    pub status: Option<(String, u64)>,
    /// The objects the local player has selected (flag 1 of the original),
    /// which the interface hands over before each step for `$isselected`.
    #[serde(skip)]
    pub selected: BTreeSet<usize>,
}
/// Kingdom state of the tavern loans and the crusades.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Treasury {
    /// Amount the next loan gives (K+0xfe): 1000, +200 for each repaid loan.
    pub loan_amount: u32,
    /// A repaid loan sets the next amount to its own + 200 60 s later
    /// (message 0x4206): the tick and the new amount.
    pub raise: Option<(u64, u32)>,
    /// Resource of the outstanding loan (K+0x100).
    pub loan: Option<u8>,
    /// Crusaders ready (K+0xd8, 16 at the start); None while a crusade runs.
    pub crusaders: Option<u16>,
    /// Full crusades preached (K+0xd6), raising the ceiling by 4 each.
    pub full_crusades: u16,
    /// The crusade under way: its anchor cell, crusaders still to come and
    /// the next arrival.
    pub crusade: Option<Crusade>,
}
impl Default for Treasury {
    fn default() -> Self {
        Self {
            loan_amount: 1000,
            raise: None,
            loan: None,
            crusaders: Some(16),
            full_crusades: 0,
            crusade: None,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Crusade {
    /// The crusade anchor (class 116).
    pub anchor: usize,
    /// Crusaders still to come (+0xc8) and the next arrival (500 ms apart).
    pub left: u16,
    pub next: u64,
    /// Land cells without trees within 7 of the anchor, taken when preached.
    pub cells: Vec<[u16; 2]>,
    pub crusaders: Vec<usize>,
}
/// The tavern's resource exchange (0x4d3104): paid up front, delivered one
/// unit every 300 ms.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Exchange {
    pub from: u8,
    pub to: u8,
    pub paid: u32,
    pub total: u32,
    pub delivered: u32,
    pub next: u64,
}
/// A tavern offer (0x4d2030): `count` units of `class` drawn from the
/// barracks menu of `civilization`, recruited at `level`; `order` is the
/// barracks button it came from, whose picture the offer shows.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Hire {
    pub id: u32,
    pub class: usize,
    pub civilization: u8,
    pub count: u16,
    pub level: u16,
    pub order: String,
}
/// The Asian market's items of a kingdom (0x9dba98 + 0x4a × kingdom): the
/// market that offers them (K+0x12e) and at most eight, oldest first.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Jewels {
    pub market: Option<usize>,
    pub offers: Vec<Jewel>,
}
/// An item on offer: its class and the record its purchase gives
/// (randomized by vt+0x2c8 when offered).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Jewel {
    pub id: u32,
    pub class: usize,
    pub image: String,
    pub size: usize,
    pub fields: Vec<(usize, u16)>,
    pub spell: u8,
}
/// A trade route (0x4e8060): resources loaded at each end, the load, the
/// distance between the markets and the map size when it was set.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Route {
    pub resources: [u8; 2],
    pub amount: u32,
    pub distance: u32,
    pub map_size: u32,
    pub cargo: u32,
    pub cargo_type: u8,
    /// Tick of the next loading attempt (1000 ms apart).
    pub retry: u64,
    /// The market this trader holds while trading there.
    pub reserved: Option<usize>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Vision {
    pub cell: [u16; 2],
    pub radius: u16,
    pub until: u64,
    /// Magic vision follows this unit (0x473xxx), shared with the caster's allies.
    #[serde(default)]
    pub object: Option<usize>,
    #[serde(default)]
    pub owner: u8,
}
/// Kinds of the native event list: 1 attacked, 2 building completed,
/// 3 unit produced, 4 technology, 5 upgrade.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameEvent {
    pub kind: u8,
    pub cell: [u16; 2],
    pub object: usize,
    pub tick: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Projectile {
    pub attacker: usize,
    pub target: usize,
    pub amount: u16,
    /// What a shell or rocket deals around a direct hit (attack >> 1).
    #[serde(default)]
    pub splash: u16,
    pub category: u8,
    pub from: [u16; 2],
    /// The target's cell at launch, where shells and bolts land.
    #[serde(default)]
    pub cell: [u16; 2],
    /// One of [`missile`].
    #[serde(default)]
    pub kind: u8,
    pub launched: u64,
    pub lands: u64,
    /// A flame ball's position and destination in map pixels and the new
    /// cells it has entered (0x431f40).
    #[serde(default)]
    pub pos: [i32; 2],
    #[serde(default)]
    pub goal: [i32; 2],
    #[serde(default)]
    pub entered: u8,
    /// Presentation: the shooter's class and the flight's start and end in
    /// map pixels (0x431340).
    #[serde(default)]
    pub shooter: u16,
    #[serde(default)]
    pub start: [i32; 2],
    #[serde(default)]
    pub end: [i32; 2],
    /// A missed arrow flying on (0x432bac): the tick it reached its end, 0
    /// before.
    #[serde(default)]
    pub flown: u64,
}
/// A one-shot animation of the original (class 0x52, 0x497d40): picture,
/// hotspot point in map pixels, frames per second (0 for the picture's own),
/// plays, draw priority, last frame kept for `hold_ms`, the brighten strength
/// of mode 3 (0 for a plain picture) and the picture's column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flash {
    pub image: &'static str,
    pub at: [i32; 2],
    pub fps: u16,
    pub loops: u8,
    pub prio: u16,
    pub hold_ms: u32,
    pub glow: u8,
    pub column: u8,
    /// Milliseconds before it starts (the staggered sparkles, 0x473ed0).
    pub delay_ms: u32,
}
fn flash(sim: &mut Simulation, flash: Flash) {
    if sim.flashes.len() < 256 {
        sim.flashes.push(flash);
    }
}
/// Record an event of the local player (0x4931e0) at the centre of `at`
/// (the attack adds half the width to both coordinates, 0x4dda4d), about
/// `object`, and give its notice in the voices of units (attack, unit
/// ready: category 24) or of buildings (25), from where it happened. An
/// attack within 24 cells of an earlier attack renews that event, silently
/// and in its place until 15 s passed since the last blow there; a unit
/// within 24 cells of an earlier one of its class renews that event. A
/// renewed event otherwise moves to the front of the ten.
fn event(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    kind: u8,
    at: usize,
    object: usize,
) {
    use crate::audio::{Channel, category};
    let Some(o) = m.objects.get(at) else {
        return;
    };
    if o.owner != m.player() || (kind == 1 && o.class == class::DUMMY) {
        return;
    }
    let [w, h] = footprint(lib, o).map(|n| n as u16 / 2);
    let cell = [o.x + w, o.y + if kind == 1 { w } else { h }];
    let class = m.objects.get(object).map(|o| o.class);
    let now = sim.ticks;
    let events = &mut sim.gameplay.events;
    let renewed = events.iter().position(|e| {
        matches!(kind, 1 | 3)
            && e.kind == kind
            && (kind == 1 || m.objects.get(e.object).map(|o| o.class) == class)
            && e.cell[0].abs_diff(cell[0]).max(e.cell[1].abs_diff(cell[1])) <= 24
    });
    let fresh = GameEvent {
        kind,
        cell,
        object,
        tick: now,
    };
    match renewed {
        Some(k) => {
            // 0x49331d: a renewed attack stays in its place while silent.
            let quiet = now.saturating_sub(events[k].tick);
            events[k] = fresh;
            if kind == 1 && quiet < 15_000 / TICK_MS {
                return;
            }
            if let Some(e) = events.remove(k) {
                events.push_front(e);
            }
        }
        None => {
            events.push_front(fresh);
            events.truncate(10);
        }
    }
    let (property, category, channel) = match kind {
        1 => ("@notify/attack.wav", category::UNIT_READY, Channel::Unit),
        2 => (
            "@notify/bldgdone.wav",
            category::BUILDING_READY,
            Channel::Building,
        ),
        3 => ("@notify/unitdone.wav", category::UNIT_READY, Channel::Unit),
        4 => (
            "@notify/techdone.wav",
            category::BUILDING_READY,
            Channel::Building,
        ),
        _ => (
            "@notify/upgrdone.wav",
            category::BUILDING_READY,
            Channel::Building,
        ),
    };
    push_sound(
        sim,
        SoundEvent {
            object,
            property,
            category,
            channel,
            cell: Some(cell),
            fixed: false,
        },
    );
}
impl Runtime {
    /// What an old snapshot without gameplay reads.
    pub fn missing() -> Self {
        Self {
            ai: ai::State::missing(),
            ..Default::default()
        }
    }
    pub fn validate(&self, n: usize, side: usize) -> bool {
        let cell = |p: [u16; 2]| (p[0] as usize) < side && (p[1] as usize) < side;
        let intent = |x: &Intent| match x {
            Intent::Move(p)
            | Intent::Explore(p)
            | Intent::Chop(p)
            | Intent::AttackGround(p)
            | Intent::Unload(p) => cell(*p),
            Intent::Cast {
                key,
                cell: p,
                target,
            } => key.len() <= 64 && cell(*p) && target.is_none_or(|j| j < n),
            Intent::Patrol(a, b) => cell(*a) && cell(*b),
            Intent::Flee(p, _) => cell(*p),
            Intent::Rescue(_) | Intent::FarBuild(_) => true,
            Intent::Approach(ap) => {
                cell(ap.point) && (1..=4).contains(&ap.step) && ap.sector.is_none_or(cell)
            }
            Intent::Transport(t) => {
                cell(t.point)
                    && (1..=11).contains(&t.step)
                    && t.passengers.len() <= n
                    && t.passengers.iter().all(|&j| j < n)
                    && t.fetching.is_none_or(|j| j < n)
                    && t.war.is_none_or(|x| x.sector.is_none_or(cell))
            }
            Intent::UseTransport(r) => r.ship < n && cell(r.point) && (1..=8).contains(&r.step),
            Intent::Circuit(points, next, _) => {
                points.len() <= 32 && *next < points.len().max(1) && points.iter().all(|p| cell(*p))
            }
            Intent::Attack(i)
            | Intent::Harvest(i)
            | Intent::Build(i)
            | Intent::Repair(i)
            | Intent::Capture(i)
            | Intent::Bribe(i)
            | Intent::Pick(i)
            | Intent::PickUse(i)
            | Intent::Guard(i)
            | Intent::Board(i)
            | Intent::Trade(i)
            | Intent::Deliver(i)
            | Intent::Sell(i)
            | Intent::Murder(i)
            | Intent::Impersonate(i)
            | Intent::Reach(i) => *i < n,
            Intent::UseItem {
                item,
                target,
                cell: p,
            }
            | Intent::DropItem {
                item,
                target,
                cell: p,
            } => *item < n && target.is_none_or(|j| j < n) && cell(*p),
        };
        (self.actors.is_empty() || self.actors.len() == n)
            && self.groups.len() <= n
            && self.groups.values().all(|x| {
                x.members.len() <= n
                    && x.members.iter().all(|&k| k < n)
                    && x.paths.len() <= 4096
                    && x.paths
                        .iter()
                        .all(|p| p.path.len() <= side * side && p.path.iter().copied().all(cell))
            })
            && self.actors.iter().all(|a| {
                a.direction < 8
                    && a.ship_level <= 6
                    && a.disguise.as_ref().is_none_or(|d| {
                        d.class < CLASSES.len() && d.civilization <= 3 && d.owner <= 8
                    })
                    && a.stance < 3
                    && a.guarding.is_none_or(|j| j < n)
                    && a.guard_list.iter().all(|&j| j < n)
                    && a.construction <= a.construction_total
                    && a.capture_target.is_none_or(|j| j < n)
                    && a.work_target.is_none_or(|j| j < n)
                    && a.work_cell.is_none_or(cell)
                    && a.explore_target.is_none_or(cell)
                    && a.move_target.is_none_or(|(j, _)| j < n)
                    && a.action_point.is_none_or(cell)
                    && a.action_image.len() <= 64
                    && a.jobs.len() <= 64 * JOB_TYPE_LIMIT
                    && a.queued.len() <= QUEUE_LIMIT
                    && a.path.len() <= side * side
                    && a.path.iter().copied().all(cell)
                    && a.goal.is_none_or(cell)
                    && a.rally.is_none_or(cell)
                    && a.intent.as_ref().is_none_or(intent)
                    && a.pasture
                        .is_none_or(|p| p.picture < 3 && p.step < 4 && cell(p.home))
                    && a.queued.iter().all(intent)
                    && a.depositing.is_none_or(|i| i < n)
                    && a.trade_origin.is_none_or(|i| i < n)
                    && a.trade_partner.is_none_or(|i| i < n)
                    && a.effects.len() <= 32
                    && a.effects.iter().all(|e| e.owner <= DISCORD_OWNER)
                    && a.move_mode <= 2
                    && a.rally_mode <= 2
                    && a.attack_type.is_none_or(|c| c < CLASSES.len())
                    && a.allegiance.is_none_or(|o| o <= DISCORD_OWNER)
                    && a.resume.as_ref().is_none_or(intent)
                    && a.field_cycles <= 19
                    && a.cargo_source < CLASSES.len()
                    && a.walk
                        .as_ref()
                        .is_none_or(|w| cell(w.from) && w.step_ms > 0 && w.step_ms <= 1000)
                    && a.movement
                        .as_ref()
                        .is_none_or(|mv| mv.state <= 2 && mv.target.is_none_or(|j| j < n))
                    && a.waiting_for.is_none_or(cell)
                    && a.group.is_none_or(|k| self.groups.contains_key(&k))
                    && a.production_blocked.as_ref().is_none_or(|s| s.len() <= 256)
                    && a.exchange.as_ref().is_none_or(|x| {
                        x.from < 4 && x.to < 4 && x.delivered < x.total && x.total <= 2000
                    })
                    && a.route.as_ref().is_none_or(|r| {
                        r.resources.iter().all(|&k| k < 4)
                            && r.cargo_type < 4
                            && r.amount <= 500
                            && r.reserved.is_none_or(|k| k < n)
                    })
                    && a.jobs.iter().all(|j| {
                        j.total > 0
                            && j.remaining <= j.total
                            && match &j.kind {
                                JobKind::Train(c) => *c < CLASSES.len(),
                                JobKind::Research(s) => s.len() <= 128,
                                JobKind::Upgrade(s, c) => {
                                    s.len() <= 128 && c.is_none_or(|c| c < CLASSES.len())
                                }
                                JobKind::Item(category, index, _) => *category < 3 && *index < 21,
                            }
                    })
            })
            && self.pathing.regions.valid(side)
            && self.scaled_remainder < 75
            && self.events.len() <= 10
            && self.events.iter().all(|e| e.object < n && cell(e.cell))
            && self.picks.iter().all(|&j| j < n)
            && self.projectiles.len() <= 4096
            && self
                .projectiles
                .iter()
                .all(|p| p.attacker < n && p.target < n && cell(p.cell) && cell(p.from))
            && self.fields.keys().all(|&i| i < n)
            && self
                .visions
                .iter()
                .all(|v| v.object.is_none_or(|j| j < n) && v.owner <= DISCORD_OWNER)
            && self.visions.len() <= 1000
            && self
                .visions
                .iter()
                .all(|v| cell(v.cell) && v.radius as usize <= side)
            && self.resources.keys().all(|&i| i < n)
            && self.consumed_corpses.iter().all(|&i| i < n)
            && self
                .trees
                .keys()
                .chain(self.depleted_trees.iter())
                .chain(self.stumps.iter())
                .all(|&i| i < side * side)
            && self.grounds.len() <= n
            && self.score.keys().all(|k| (1..=8).contains(k))
            && self.meat.iter().all(|(&k, &v)| k < n && v <= 600)
            && self.treasury.iter().all(|(&k, t)| {
                k <= DISCORD_OWNER
                    && t.loan.is_none_or(|r| r < 4)
                    && t.crusade.as_ref().is_none_or(|x| {
                        x.anchor < n
                            && x.left <= 36
                            && x.cells.len() <= 225
                            && x.cells.iter().all(|&p| cell(p))
                            && x.crusaders.len() <= 36
                            && x.crusaders.iter().all(|&j| j < n)
                    })
            })
    }
}
#[derive(Clone, Debug, Default)]
pub struct Request {
    pub order: String,
    pub selected: Vec<usize>,
    pub cell: Option<[u16; 2]>,
    pub target: Option<usize>,
    pub append: bool,
    /// Ctrl and Alt held with a right click (0x4e4303).
    pub ctrl: bool,
    pub alt: bool,
    /// Choices of the original dialogs: the resources offered and asked
    /// (gambling, exchange, trade routes), an amount (stake, offer, load,
    /// gold of the magic workshop), the gambling odds or the workshop
    /// category, and the workshop item.
    pub resource: u8,
    pub resource2: u8,
    pub amount: u32,
    pub option: u8,
    pub item: u8,
    /// The first market of a trade route (D_TRADE); `target` is the second.
    pub origin: Option<usize>,
    /// The Shift points of a patrol.
    pub waypoints: Vec<[u16; 2]>,
    /// The objects a guard or repair command gathered with Shift (the
    /// command's list, +0x346/+0x34a), sent when Shift is released.
    pub targets: Vec<usize>,
    /// Where the order was clicked on the map, in map pixels, for the
    /// interface's marker; the simulation does not read it.
    pub point: Option<[f32; 2]>,
    /// A command of the cheat panel (order `cheat`).
    pub cheat: Option<cheats::Cheat>,
}
#[derive(Clone, Debug)]
pub struct SoundEvent {
    pub object: usize,
    pub property: &'static str,
    /// The category of the original (`crate::audio::category`): its
    /// `[priority]` and how many effects of it play at once.
    pub category: u8,
    /// Effects, or the voices of units or of buildings (the notices).
    pub channel: crate::audio::Channel,
    /// A sound of a place (impacts) rather than of an object.
    pub cell: Option<[u16; 2]>,
    /// Heard where the object was when it started (owner 0xfffe of
    /// 0x4d01c0) rather than following it.
    pub fixed: bool,
}
fn push_sound(sim: &mut Simulation, event: SoundEvent) {
    // Bound presentation work during large catch-up updates.
    if sim.sounds.len() < 64 {
        sim.sounds.push(event);
    }
}
/// An object's sound on the effect channels (vt+0x1c8 = 0x505710), following
/// the object when the view moves.
fn sound(sim: &mut Simulation, object: usize, property: &'static str, category: u8) {
    push_sound(
        sim,
        SoundEvent {
            object,
            property,
            category,
            channel: crate::audio::Channel::Effect,
            cell: None,
            fixed: false,
        },
    );
}
/// An object's sound heard where the object is now (owner 0xfffe): deaths,
/// spells and potions.
fn sound_here(sim: &mut Simulation, object: usize, property: &'static str, category: u8) {
    push_sound(
        sim,
        SoundEvent {
            object,
            property,
            category,
            channel: crate::audio::Channel::Effect,
            cell: None,
            fixed: true,
        },
    );
}
/// A sound at a map cell (an impact, 0x433aaa).
fn sound_at(sim: &mut Simulation, cell: [u16; 2], property: &'static str, category: u8) {
    push_sound(
        sim,
        SoundEvent {
            object: usize::MAX,
            property,
            category,
            channel: crate::audio::Channel::Effect,
            cell: Some(cell),
            fixed: true,
        },
    );
}
/// Category of an attacker's strike (vt+0x2ec of each class): bows,
/// mages, catapults and the messiah 8; longbows, ships and the buildings
/// that shoot (0x4dbbb0) 9; the dragon 10; a priest's flame bolt 13
/// (0x4ea4c0); blows 11 (0x4df6e0).
fn strike_category(m: &MissionWorld, c: &Catalog, i: usize) -> u8 {
    use crate::audio::category;
    let o = &m.objects[i];
    if o.building() {
        return category::HEAVY_SHOT;
    }
    match o.class {
        4 | 10 | 17 | 19..=21 | 38 => category::SHOT,
        6 | 23..=25 | 136 => category::HEAVY_SHOT,
        32 => category::DRAGON,
        18 if combat::max_range(m, c, i) > 0 => category::SPELL,
        _ => category::BLOW,
    }
}
impl Request {
    pub fn command(order: impl Into<String>, selected: Vec<usize>) -> Self {
        Self {
            order: order.into(),
            selected,
            cell: None,
            target: None,
            append: false,
            ..Default::default()
        }
    }
}
/// Researches of a kingdom; neutral objects and the special owners have none.
fn researches(m: &MissionWorld, owner: u8) -> &[String] {
    m.rules
        .get(owner.wrapping_sub(1) as usize)
        .map_or(&[], |r| r.initial_researches.as_slice())
}
/// What a holder keeps as an object rather than as a passenger: items and
/// resource items, the objects with the item flag 0x400.
fn carried(o: &MapObject) -> bool {
    o.item() || o.class == class::RESITEM
}
/// The objects a unit guards (+0xca): its list, or the one object of a save
/// made before lists.
pub(crate) fn guarded(a: &Actor) -> impl Iterator<Item = usize> + '_ {
    let legacy = a.guard_list.is_empty().then_some(a.guarding).flatten();
    a.guard_list.iter().copied().chain(legacy)
}
fn alive(o: &MapObject) -> bool {
    o.flags & (FLAG_INACTIVE | FLAG_DEAD) == 0
        && (o.health > 0 || (!world::class::is_unit(o.class) && !o.building()))
}
/// 0x4affab: the mine gave its last load. It stays on the map as the pit
/// (its last row), dead: its items fall around it (0x504890), it leaves
/// the selection, lies on the ground (priority 1), frees its cells
/// (vt+0x74 = 0x41df30, which passes the rect to the islands and the
/// regions) and is heard collapsing (`sound_die`, category 23). The AI
/// sees it leave (0x402c50) with the next step's exits.
fn exhaust(sim: &mut Simulation, m: &mut MissionWorld, g: &MapGrid, lib: &ObjectLibrary, j: usize) {
    if m.objects[j].flags & FLAG_DEAD != 0 {
        return;
    }
    for k in sim.objects[j].contents.clone() {
        if carried(&m.objects[k]) {
            release_item(sim, m, g, lib, j, k);
        }
    }
    m.objects[j].flags |= FLAG_DEAD;
    sound_here(sim, j, "sound_die", crate::audio::category::COLLAPSE);
}
fn mobile(o: &MapObject) -> bool {
    world::class::is_unit(o.class) || matches!(o.class, class::COW | class::FISHING_BOAT)
}
/// Housing of the kingdom's completed buildings (`pop_count`).
pub(crate) fn housing_sum(m: &MissionWorld, lib: &ObjectLibrary, owner: u8) -> usize {
    m.objects
        .iter()
        .filter(|o| {
            o.owner == owner && alive(o) && o.building() && o.flags & FLAG_CONSTRUCTION == 0
        })
        .filter_map(|o| lib.property(o, "pop_count")?.trim().parse::<usize>().ok())
        .sum()
}
/// Shared with production and the HUD (0x468420): units, passengers
/// included and cows excluded (K+0x5e), and min(housing, cap). Housing
/// (K+0x60) starts at 1 (0x4671b0) and grows by `pop_count` of every
/// completed building (0x467450, 0x41e278); `@setpoplimit` replaces it
/// (0x4b8370) and later buildings add to and take from the new value. The
/// cap (K+0x62) is the game's limit, a quarter more for each of Supply
/// Storage and Seafood (0x4d3819, 0x4d385a).
pub fn population(m: &MissionWorld, lib: &ObjectLibrary, owner: u8) -> (usize, usize) {
    let (count, housing, cap) = population_parts(m, lib, owner);
    (count, housing.min(cap))
}
/// Units (K+0x5e), housing (K+0x60) and cap (K+0x62) of a kingdom.
fn population_parts(m: &MissionWorld, lib: &ObjectLibrary, owner: u8) -> (usize, usize, usize) {
    let count = m
        .objects
        .iter()
        .filter(|o| o.owner == owner && alive(o) && mobile(o) && o.class != class::COW)
        .count();
    let sum = housing_sum(m, lib, owner);
    let housing = match m.population_limit.get(&owner) {
        Some(&n) => {
            let base = m.population_base.get(&owner).copied().unwrap_or(sum);
            (n as i64 + sum as i64 - base as i64).max(0) as usize
        }
        None => 1 + sum,
    };
    let limit = m.game_rules.population;
    let bonus = cheats::supply_completions(m, owner);
    // «nopop» set the cap (0x42455f); later researches add to it.
    let (base, counted) = match m.population_cap.get(&owner) {
        Some(&[cap, then]) => (cap, bonus.saturating_sub(then)),
        None => (limit, bonus),
    };
    (
        count,
        housing,
        base.saturating_add(limit * 25 / 100 * counted),
    )
}
/// An unfinished building a peasant of `owner` may build: one of an ally
/// (relation 0x40, 0x4adcca).
fn site(m: &MissionWorld, owner: u8, j: usize) -> bool {
    m.objects.get(j).is_some_and(|x| {
        alive(x)
            && x.building()
            && x.flags & FLAG_CONSTRUCTION != 0
            && relation(m, owner, x.owner) & 0x40 != 0
    })
}
/// Find unfinished building (op 0x22, 0x4ad356): within twice the peasant's
/// sight of its cell, the unfinished building of a kingdom sharing control
/// with it (relation 4 both ways, 0x44dc70 and 0x44f260) nearest by
/// Chebyshev distance to its centre (0x454d70), the first on ties.
fn unfinished_near(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
) -> Option<usize> {
    let o = &m.objects[i];
    let reach = 2 * sim.objects[i].sight as i32;
    let here = [o.x as i32, o.y as i32];
    let mut best: Option<(i32, usize)> = None;
    for (j, x) in m.objects.iter().enumerate() {
        if !alive(x)
            || !x.building()
            || x.flags & FLAG_CONSTRUCTION == 0
            || relation(m, o.owner, x.owner) & 4 == 0
            || relation(m, x.owner, o.owner) & 4 == 0
            || (x.x as i32 - here[0]).abs() > reach
            || (x.y as i32 - here[1]).abs() > reach
        {
            continue;
        }
        let [w, h] = footprint(lib, x).map(|n| n as i32);
        let centre = [x.x as i32 + w / 2, x.y as i32 + h / 2];
        let d = (here[0] - centre[0]).abs().max((here[1] - centre[1]).abs());
        if best.is_none_or(|(b, _)| d < b) {
            best = Some((d, j));
        }
    }
    best.map(|(_, j)| j)
}
/// A building of `owner`'s at `p` (0x4fd4c0): its price paid by the kingdom
/// first; a site to build, or a field born complete and ripe.
pub(super) fn lay_site(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    by: &MapObject,
    class: usize,
    p: [u16; 2],
) -> std::result::Result<usize, String> {
    let stats = c.stats(by, CLASSES[class], researches(m, by.owner));
    debit(c, m, by.owner, stats.cost)?;
    let j = match new_unit(sim, m, lib, class, by.owner, by.civilization, p) {
        Ok(j) => j,
        Err(e) => {
            credit(m, by.owner, stats.cost);
            return Err(e);
        }
    };
    found_site(sim, m, lib, c, j);
    Ok(j)
}
/// A new building `j` on the map (0x4fd4c0): a gate takes the place of the
/// walls under it; a site to build, or a field born complete and ripe.
pub(super) fn found_site(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    j: usize,
) {
    let class = m.objects[j].class;
    let stats = c.stats(
        &m.objects[j],
        CLASSES[class],
        researches(m, m.objects[j].owner),
    );
    // 0x455eae: a gate takes the place of the walls under it.
    if matches!(class, class::GATE_WOOD | class::GATE_STONE) {
        let r = movement::footprint_rect(lib, &m.objects[j]);
        for k in 0..m.objects.len() {
            let x = &m.objects[k];
            if matches!(x.class, class::WALL_STONE | class::WALL_WOOD)
                && alive(x)
                && (r[0]..=r[2]).contains(&(x.x as i32))
                && (r[1]..=r[3]).contains(&(x.y as i32))
            {
                vanish(sim, m, k);
            }
        }
    }
    if class == class::FIELD {
        // 0x44c7a0: a field is born complete and ripe, with a random column
        // (0x44c880).
        m.objects[j].flags &= !FLAG_CONSTRUCTION;
        m.objects[j].health = m.objects[j].max_health;
        m.objects[j].variant = random(sim, 9) as u16;
        event(sim, m, lib, 2, j, j);
    } else {
        m.objects[j].flags |= FLAG_CONSTRUCTION;
        m.objects[j].health = 1;
        let a = &mut sim.gameplay.actors[j];
        a.construction_total = stats.time_ms.div_ceil(TICK_MS).max(1);
        a.construction = a.construction_total;
    }
}
/// A site finished (0x41e0c0): whole, at full health, and the event of a
/// building done (2).
pub(super) fn complete_site(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    j: usize,
) {
    let a = &mut sim.gameplay.actors[j];
    a.construction = 0;
    // 0x456080: a finished gate starts its 500 ms timer now.
    a.gate_since = sim.ticks;
    m.objects[j].flags &= !FLAG_CONSTRUCTION;
    m.objects[j].health = m.objects[j].max_health;
    if m.objects[j].class == class::FIELD {
        // 0x41e10c → 0x44c880: a new field shows a random column of its
        // ripe picture.
        m.objects[j].variant = random(sim, 9) as u16;
    } else if m.objects[j].class == class::HOUSE {
        // 0x460080: a finished house takes a random column of its sheet,
        // R(0, columns − 1).
        let columns = lib
            .property(&m.objects[j], "image")
            .and_then(|s| lib.images.get(s, "cols"))
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(1);
        m.objects[j].variant = random(sim, columns) as u16;
    }
    event(sim, m, lib, 2, j, j);
}
/// Steps 4-6 of the Build program: a site handed on with the order goes
/// first (0x48f160), then the nearest unfinished one; with none the program
/// ends.
fn next_site(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, i: usize) {
    let a = &mut sim.gameplay.actors[i];
    if let Some(Intent::Build(k)) = a.queued.front().cloned() {
        // Steps of the same program: the build picture stays on.
        a.queued.pop_front();
        a.intent = Some(Intent::Build(k));
        halt(a);
        return;
    }
    match unfinished_near(sim, m, lib, i) {
        Some(k) => {
            let a = &mut sim.gameplay.actors[i];
            a.intent = Some(Intent::Build(k));
            a.work_target = None;
            halt(a);
        }
        None => finish(sim, i),
    }
}
/// What a peasant of `owner` repairs (0x4adc20): a damaged complete
/// building of a kingdom that is not its enemy, or a damaged machine of an
/// ally (attributes 0x271a, 0x2719).
fn repairable(m: &MissionWorld, owner: u8, j: usize) -> bool {
    m.objects.get(j).is_some_and(|x| {
        alive(x)
            && x.health < x.max_health
            && if x.building() {
                x.flags & FLAG_CONSTRUCTION == 0 && !enemy(m, owner, x.owner)
            } else {
                crate::selection::unit(x) && machine(x) && relation(m, owner, x.owner) & 0x40 != 0
            }
    })
}
/// Query 0x271a: buildings but 57 and 74 (walls and gates through their
/// first piece, 0x4fa6b0), ruins, the dummy and the machines.
fn mended(o: &MapObject) -> bool {
    matches!(o.class, 39..=56 | 58..=73 | class::RUIN | class::DUMMY) || machine(o)
}
/// 0x4e87c0(1): a cell of the ship's rect, grown by one to the top and
/// left, is not water.
fn ashore(sim: &Simulation, m: &MissionWorld, g: &MapGrid, lib: &ObjectLibrary, j: usize) -> bool {
    let o = &m.objects[j];
    let [wd, ht] = footprint(lib, o).map(|n| n as i32);
    let side = g.side as i32;
    let (x, y) = (o.x as i32, o.y as i32);
    let (x0, y0) = ((x - 1).max(0), (y - 1).max(0));
    let (x1, y1) = ((x + wd).min(side), (y + ht).min(side));
    let fresh;
    let cells = if sim.gameplay.pathing.cells.side == g.side {
        &sim.gameplay.pathing.cells
    } else {
        fresh = movement::cells_now(sim, m, g, lib);
        &fresh
    };
    (y0..y1).any(|cy| (x0..x1).any(|cx| cells.at([cx, cy]) & (3 << 17) != pathing::WATER))
}
/// 0x44f2f0: what a peasant of `k` mends on its own: an object of its
/// kingdom or of one allied both ways (0x44dc70), damaged, of the kind that
/// is mended, not a field; a ship only beside land.
fn mends(
    sim: &Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    k: u8,
    j: usize,
) -> bool {
    let o = &m.objects[j];
    if !alive(o)
        || !mended(o)
        || o.health >= o.max_health
        || o.class == class::FIELD
        || (o.owner != k && relation(m, k, o.owner) & 0x40 == 0)
        || relation(m, o.owner, k) & 0x40 == 0
    {
        return false;
    }
    !matches!(o.class, 23..=25 | class::FISHING_BOAT) || ashore(sim, m, g, lib, j)
}
/// 0x44e690 round `area` with 0x44f2f0: the object kingdom `k` mends
/// nearest to `from` by its centre (0x454d70), the first met in the sector
/// lists on ties.
pub(crate) fn mend_near(
    sim: &Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    k: u8,
    from: [i32; 2],
    area: [i32; 4],
) -> Option<usize> {
    let mut order: Vec<usize> = (0..m.objects.len().min(sim.gameplay.actors.len()))
        .filter(|&j| {
            let r = movement::footprint_rect(lib, &m.objects[j]);
            !(r[0] > area[2] || r[2] < area[0] || r[1] > area[3] || r[3] < area[1])
                && sim.gameplay.actors[j].sector.is_some()
                && mends(sim, m, g, lib, k, j)
        })
        .collect();
    order.sort_by_key(|&j| sector_order(sim, j));
    let centre = |o: &MapObject| {
        let [w, h] = footprint(lib, o).map(|n| n as i32);
        let c = [o.x as i32 + w / 2, o.y as i32 + h / 2];
        (from[0] - c[0]).abs().max((from[1] - c[1]).abs())
    };
    let mut best: Option<(i32, usize)> = None;
    for j in order {
        let d = centre(&m.objects[j]);
        if best.is_none_or(|(b, _)| d < b) {
            best = Some((d, j));
        }
    }
    best.map(|(_, j)| j)
}
/// Step 3 of the Repair program (op 0x36, 0x4ad3e9): the nearest object the
/// peasant mends within twice its sight (+0x6a) round its cell; with none
/// the program ends.
fn next_repair(sim: &mut Simulation, m: &MissionWorld, g: &MapGrid, lib: &ObjectLibrary, i: usize) {
    let o = &m.objects[i];
    let reach = 2 * sim.objects[i].sight as i32;
    let (x, y) = (o.x as i32, o.y as i32);
    let area = [x - reach, y - reach, x + reach, y + reach];
    match mend_near(sim, m, g, lib, o.owner, [x, y], area) {
        Some(k) => {
            let a = &mut sim.gameplay.actors[i];
            a.intent = Some(Intent::Repair(k));
            a.work_target = None;
            halt(a);
        }
        None => finish(sim, i),
    }
}
/// 0x437450: any unit of another kingdom but peasants and crusaders.
fn bribable(m: &MissionWorld, owner: u8, j: usize) -> bool {
    m.objects.get(j).is_some_and(|x| {
        standing(m, j)
            && crate::selection::unit(x)
            && x.owner != owner
            && !matches!(x.class, class::PEON | 11)
    })
}
/// A peasant's right click captures (0x4682e0): a building of an ally whose
/// resistance is below its target, which it restores, a stone or wooden
/// tower (39–42) of any kingdom that is not an ally, any other building of
/// an enemy.
fn capturable(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    owner: u8,
    target: usize,
) -> bool {
    if !capture_target(m, lib, target) {
        return false;
    }
    let x = &m.objects[target];
    if allied(m, owner, x.owner)
        && (sim.objects[target].resistance as i32) < resistance_target(sim, m, lib, target)
    {
        return true;
    }
    if (39..=42).contains(&x.class) {
        return !allied(m, owner, x.owner);
    }
    enemy(m, owner, x.owner)
}
fn capture_target(m: &MissionWorld, lib: &ObjectLibrary, target: usize) -> bool {
    m.objects.get(target).is_some_and(|o| {
        alive(o)
            && o.building()
            && o.flags & FLAG_CONSTRUCTION == 0
            && lib
                .objects
                .get(o.class_name())
                .and_then(|ini| ini.get("general", "type"))
                .is_some_and(|s| {
                    s.split(',')
                        .any(|v| v.trim().eq_ignore_ascii_case("capture"))
                })
    })
}
/// A unit category of the `[accept]` and `[enter]` lists (keywords read at
/// 0x4957b9, tested at 0x45531f), else a class or a group of BONUS.INI.
fn listed(lib: &ObjectLibrary, c: &Catalog, key: &str, o: &MapObject) -> bool {
    match key.trim() {
        "allunits" => mobile(o),
        // 0x1003: the land layer (`teren` 1), the jinn included.
        "earthunits" => lib.layer(o) == 1,
        // 0x1004 and 0x1005 are read but never matched (0x45531f).
        "waterunits" | "airunits" => false,
        // Attribute 0x271b: the classes whose own query answers it
        // (0x4eec30, 0x4ee520, 0x4ee6d0, 0x4ebdc0, 0x4aeb40).
        "infantryunits" => matches!(
            o.class,
            1 | 3..=7 | 12 | 13 | 14 | 17 | 18 | 27..=30 | 33 | 38
        ),
        // Attribute 0x271d: the `mounted` type of the definition (0x4e483d).
        "mountedunits" => lib
            .objects
            .get(o.class_name())
            .and_then(|ini| ini.get("general", "type"))
            .is_some_and(|t| t.split(',').any(|w| w.trim() == "mounted")),
        key => c.group(key, o, 0),
    }
}
/// Whether `held` may go into `holder` (0x4552dc): the holder's `[accept]`
/// names it, or its own `[enter]` names the holder. Items name the units
/// that can carry them.
fn accepts(lib: &ObjectLibrary, c: &Catalog, holder: &MapObject, held: &MapObject) -> bool {
    let names = |owner: &MapObject, section: &str, other: &MapObject| {
        lib.objects.get(owner.class_name()).is_some_and(|ini| {
            ini.entries(section)
                .iter()
                .any(|(key, _)| listed(lib, c, key, other))
        })
    };
    names(holder, "accept", held) || names(held, "enter", holder)
}
/// The room of an object's holder, fixed when it was made.
/// 0x455280 with the default action's control test: a finished holder
/// that is not held, whose `[accept]` or the unit's `[enter]` matches, of a
/// kingdom the unit's shares control with (relation 4, owner 0 included);
/// a trader never boards a carpet (0x4ed590).
fn boardable(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    unit: usize,
    holder: usize,
) -> bool {
    let (Some(u), Some(h)) = (m.objects.get(unit), m.objects.get(holder)) else {
        return false;
    };
    unit != holder
        && standing(m, holder)
        && h.flags & FLAG_CONSTRUCTION == 0
        && sim.objects[holder].holder.is_none()
        && relation(m, h.owner, u.owner) & 4 != 0
        && !(u.class == 2 && h.class == 22)
        && accepts(lib, c, h, u)
}
fn holder_capacity(sim: &Simulation, m: &MissionWorld, lib: &ObjectLibrary, i: usize) -> usize {
    sim.objects.get(i).and_then(|s| s.capacity).map_or_else(
        || holder_room(m, lib, &m.objects[i]) as usize,
        |n| n as usize,
    )
}
/// 0x4fffc4: `max_obj_in_holder` grown by the kingdom's Landing Tactics
/// (+0x106, 50 a research), for any class.
fn holder_room(m: &MissionWorld, lib: &ObjectLibrary, o: &MapObject) -> u16 {
    let base = lib
        .property(o, "max_obj_in_holder")
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);
    let bonus = 50 * crate::simulation::completions(researches(m, o.owner), "te_landingtactics");
    (base + base * bonus / 100).min(u16::MAX as u32) as u16
}
/// Religious resistance bonuses the kingdom gives a building it makes.
fn faith_of(m: &MissionWorld, o: &MapObject) -> u8 {
    if !o.building() || o.owner == 0 {
        return 0;
    }
    ["te_budhism", "te_fanaticism"]
        .iter()
        .map(|t| crate::simulation::completions(researches(m, o.owner), t))
        .sum::<u32>()
        .min(u8::MAX as u32) as u8
}
fn target_known(sim: &Simulation, m: &MissionWorld, observer: u8, j: usize) -> bool {
    let Some(o) = m.objects.get(j) else {
        return false;
    };
    alive(o)
        && sim.objects[j].holder.is_none()
        && (o.owner == observer || sim.gameplay.actors[j].effect_until <= sim.ticks)
        && (observer != m.player() || o.owner == observer || m.game_rules.no_fog || sim.visible(o))
}
/// Native peon work procedures return the selected animation's cycle duration.
fn action_ticks(lib: &ObjectLibrary, o: &MapObject, property: &str) -> u32 {
    cycles_ticks(lib, o, property, 1)
}
/// A picture's `anim_time` in ms (100 without one).
fn action_ms(lib: &ObjectLibrary, o: &MapObject, property: &str) -> u64 {
    let Some(section) = lib.property(o, property) else {
        return 100;
    };
    let rows = lib
        .images
        .get(section, "rows")
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    crate::animation::Timeline::directional(&lib.images, section, rows)
        .duration_ms
        .max(1)
}
/// The time of the picture a unit shows (0x4dfebf).
fn picture_ms(lib: &ObjectLibrary, o: &MapObject, property: &str) -> u64 {
    lib.property(o, property)
        .map_or(100, |section| section_ms(lib, section))
}
/// The time of a picture (0x4dfebf, 0x4ed163): the length of its animation
/// record (+0x68, sheets with directions), else rows × 1000 / fps, else
/// 100 ms.
fn section_ms(lib: &ObjectLibrary, section: &str) -> u64 {
    let number = |key: &str| {
        lib.images
            .get(section, key)
            .and_then(|s| s.trim().parse::<u64>().ok())
    };
    let rows = number("rows").unwrap_or(1).max(1);
    if number("cols").unwrap_or(1) > 1 {
        return crate::animation::Timeline::directional(&lib.images, section, rows as usize)
            .duration_ms;
    }
    match number("fps").filter(|&f| f > 0) {
        Some(fps) => rows * 1000 / fps,
        None => 100,
    }
}
/// `n` cycles of a picture in ticks: the cycles keep their milliseconds
/// (each due at the previous one plus `anim_time`), rounded once.
fn cycles_ticks(lib: &ObjectLibrary, o: &MapObject, property: &str, n: u32) -> u32 {
    (action_ms(lib, o, property) * n as u64)
        .div_ceil(TICK_MS)
        .max(1)
        .min(u32::MAX as u64) as u32
}
/// A work cycle due `ms` after the previous one (the queue re-arms at
/// due + R, 0x4d4430): whether it is due now, and the next armed.
fn cycle_due(sim: &mut Simulation, i: usize, ms: u64) -> bool {
    let now = sim.ticks * TICK_MS;
    let a = &mut sim.gameplay.actors[i];
    if now < a.work_due_ms {
        return false;
    }
    a.work_due_ms += ms;
    true
}

/// 0x4fd450: the direction from `at` toward `centre`, in octants whose
/// edges have a 1:2 slope (table 0x591010); the same cell gives west.
pub(super) fn heading(centre: [i32; 2], at: [i32; 2]) -> u8 {
    const TABLE: [u8; 16] = [6, 5, 7, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 3, 1, 2];
    let (dx, dy) = (centre[0] - at[0], at[1] - centre[1]);
    let idx = 8 * usize::from(2 * dx + dy > 0)
        + 4 * usize::from(2 * dx - dy > 0)
        + 2 * usize::from(dx - 2 * dy > 0)
        + usize::from(dx + 2 * dy > 0);
    TABLE[idx]
}
/// Work faces the centre of what it works on (0x4abb3f, 0x4ad1b1…).
fn face_area(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    p: [u16; 2],
    size: [usize; 2],
) {
    let (x, y) = (p[0] as i32, p[1] as i32);
    let rect = [x, y, x + size[0] as i32 - 1, y + size[1] as i32 - 1];
    face(sim, m, lib, i, rect, None);
}
/// The unit turns to the centre of `rect`, its column shifted by the
/// `startcol` of the picture it fights or casts with (+0xaa; 2 for the
/// knight's, the longbowman's and the ships' attack sheets).
fn face(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    rect: [i32; 4],
    picture: Option<&str>,
) {
    let o = &m.objects[i];
    let centre = [
        rect[0] + (rect[2] - rect[0] + 1) / 2,
        rect[1] + (rect[3] - rect[1] + 1) / 2,
    ];
    let start = picture
        .and_then(|p| lib.property(o, p))
        .and_then(|s| lib.images.get(s, "startcol"))
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0);
    sim.gameplay.actors[i].direction = (heading(centre, [o.x as i32, o.y as i32]) + start) & 7;
}

fn work_animation(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    pose: OrderState,
    property: &str,
    effect: Option<&'static str>,
    point: [u16; 2],
    size: [usize; 2],
) {
    face_area(sim, m, lib, i, point, size);
    let a = &mut sim.gameplay.actors[i];
    if sim.objects[i].order != pose || a.action_image != property || a.action_point != Some(point) {
        a.action_elapsed_ms = 0;
        a.action_image = property.into();
        a.action_point = Some(point);
    }
    sim.objects[i].order = pose;
    let before = a.action_elapsed_ms;
    a.action_elapsed_ms = before.saturating_add(TICK_MS);
    let Some(effect) = effect else { return };
    let Some(section) = lib.property(&m.objects[i], property) else {
        return;
    };
    let rows = lib
        .images
        .get(section, "rows")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let timeline = crate::animation::Timeline::directional(&lib.images, section, rows);
    let Some(row) = lib
        .images
        .get(section, "actionrow")
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&r| r < rows)
    else {
        return;
    };
    let hit = if row == 0 {
        0
    } else {
        timeline.ends()[row - 1]
    };
    let next = before / timeline.duration_ms * timeline.duration_ms + hit;
    let next = if next < before {
        next + timeline.duration_ms
    } else {
        next
    };
    if next < a.action_elapsed_ms {
        // 0x4ab8fd, 0x4abc64, 0x4ac1f4, 0x4ac338: chopping, building, digging.
        let category = match effect {
            "@chop" => crate::audio::category::CHOP,
            "@build" => crate::audio::category::BUILD,
            _ => crate::audio::category::DIG,
        };
        sound(sim, i, effect, category);
    }
}
fn resource_yield(m: &MissionWorld, owner: u8, resource: usize) -> u32 {
    // Native peon getter 0x4ae720 reads the kingdom's resource yields,
    // +2 a completion of each «plus» research, +4 of Irrigation to food.
    let n = |name: &str| crate::simulation::completions(researches(m, owner), name);
    let research = ["te_goldplus", "te_stoneplus", "te_fields", "te_woodplus"][resource];
    10 + 2 * n(research)
        + if resource == 2 {
            4 * n("te_irrigation")
        } else {
            0
        }
}
fn work_ready(a: &mut Actor, target: Option<usize>, cell: Option<[u16; 2]>, ticks: u32) -> bool {
    if a.work_target != target || a.work_cell != cell {
        a.work_target = target;
        a.work_cell = cell;
        a.work_cooldown = ticks;
        return false;
    }
    if a.work_cooldown > 0 {
        return false;
    }
    a.work_cooldown = ticks;
    true
}
fn mana_regeneration(m: &MissionWorld, i: usize) -> u16 {
    // Native 0x502ea2, once per 1000 ms: neutral 1, kingdom 2,
    // concentration adds 4 to the kingdom's +0x10c word.
    let owner = m.objects[i].owner;
    if owner == 0 {
        return 1;
    }
    2 + 4 * crate::simulation::completions(researches(m, owner), "te_concentration") as u16
}
fn naval(o: &MapObject) -> bool {
    matches!(o.class, 23..=25 | class::FISHING_BOAT)
}
pub fn ship_level(sim: &Simulation, i: usize) -> u8 {
    sim.gameplay
        .actors
        .get(i)
        .map_or(1, |a| a.ship_level.max(1))
}
fn ship_period(sim: &Simulation, m: &MissionWorld, i: usize) -> u64 {
    // Native 0x4e8f00: cannon upgrades shorten the firing period.
    match ship_level(sim, i) {
        2 => 2400,
        3 => 1900,
        4 => 1500,
        5 => 1200,
        6 => 800,
        _ if m.objects[i].class == 25 => 1300,
        _ => 3000,
    }
}
pub fn ship_upgrade_available(sim: &Simulation, i: usize, key: &str) -> bool {
    key.strip_prefix("to_ship_level").is_none_or(|n| {
        n.parse::<u8>()
            .ok()
            .is_some_and(|n| n == ship_level(sim, i) + 1 && n <= 6)
    })
}
pub fn object_menu(
    sim: Option<&Simulation>,
    lib: &ObjectLibrary,
    o: &MapObject,
    i: usize,
) -> Option<String> {
    // 0x463d70: an item has its menu only inside a holder (0x400000).
    if o.item() && o.flags & FLAG_HELD == 0 {
        return None;
    }
    let menu = lib.property(o, "menu")?;
    let level = sim.map_or(1, |s| ship_level(s, i));
    // 0x4db708 refreshes the tower's button: fire at will while it holds.
    let holding = sim.is_some_and(|s| s.gameplay.actors.get(i).is_some_and(|a| a.hold_fire));
    Some(if menu == "e_galleon" && level > 1 {
        format!("e_galleon{level}")
    } else if menu == "tower_stone2" && holding {
        "tower_stone3".into()
    } else {
        menu.to_owned()
    })
}
fn enemy(m: &MissionWorld, a: u8, b: u8) -> bool {
    a != b
        && (b == 0
            || a > 8
            || b > 8
            || m.diplomacy
                .get(a.saturating_sub(1) as usize * 8 + b.saturating_sub(1) as usize)
                .is_none_or(|d| d & 0x10 != 0))
}
fn sync(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary) {
    let first = sim.gameplay.actors.len();
    while sim.objects.len() < m.objects.len() {
        let o = &m.objects[sim.objects.len()];
        let mut s = ObjectState::read(o, lib);
        if o.owner > 0 {
            s.apply_researches(o, researches(m, o.owner))
        }
        s.capacity = Some(holder_room(m, lib, o));
        s.faith = Some(faith_of(m, o));
        sim.objects.push(s);
    }
    sim.gameplay
        .actors
        .resize_with(m.objects.len(), Default::default);
    // 0x4ec190: a spy starts with all its energy.
    for k in first..m.objects.len() {
        sim.gameplay.actors[k].born = sim.ticks;
        if m.objects[k].class == 14 {
            sim.gameplay.actors[k].spy_energy = 100;
        }
        // 0x465520: the first flag to come into play is the flag.
        if m.objects[k].class == class::ITEM_FLAG
            && m.objects[k].flags & FLAG_INACTIVE == 0
            && sim.gameplay.flag.is_none()
        {
            sim.gameplay.flag = Some(k);
        }
    }
    // Footprints are not saved: a restored simulation measures them again.
    for (o, state) in m.objects.iter().zip(sim.objects.iter_mut()) {
        if state.size.is_none() {
            state.size = Some(crate::simulation::footprint(o, lib));
        }
        // Objects of the map and older saves: their room and faith as the
        // kingdom gives them now.
        if state.capacity.is_none() {
            state.capacity = Some(holder_room(m, lib, o));
        }
        if state.faith.is_none() {
            state.faith = Some(faith_of(m, o));
        }
    }
    for (o, state) in m.objects.iter_mut().zip(&sim.objects) {
        if state.holder.is_some() {
            o.flags |= FLAG_HELD
        } else {
            o.flags &= !FLAG_HELD
        }
    }
    // Older snapshots kept the equipment bonus out of the stored values.
    for j in 0..m.objects.len() {
        refresh_equipment(sim, m, j);
    }
}
/// Pay `cost` (0x467790); short of it, the message names the first resource
/// missing in the order gold, stone, wood, food (0x4679b0). Both count the
/// stock shared with the kingdom ([`stock`], [`spend`]).
fn debit(
    c: &Catalog,
    m: &mut MissionWorld,
    owner: u8,
    cost: [u32; 4],
) -> std::result::Result<(), String> {
    // What has no kingdom pays nothing and makes nothing, without a word
    // (0x41ecb0: no kingdom, no production).
    if owner == 0 {
        return Err(String::new());
    }
    if usize::from(owner) > m.kingdoms.len() {
        return Err("Regno non valido".into());
    }
    let have = stock(m, owner);
    if let Some(r) = [0, 1, 3, 2].into_iter().find(|&r| have[r] < cost[r]) {
        return Err(c.notres[r].clone());
    }
    spend(m, owner, cost);
    Ok(())
}
/// What a kingdom may spend (0x4677d0, 0x467830): the resources of every
/// existing kingdom that shares its own with it (bit 2 of that kingdom's
/// relation toward it, 0x467d10), itself included.
pub fn stock(m: &MissionWorld, owner: u8) -> [u32; 4] {
    let mut out = [0u32; 4];
    if owner == 0 {
        return out;
    }
    for (k, kingdom) in (1..=8u8).zip(&m.kingdoms) {
        if kingdom.active && relation(m, k, owner) & 2 != 0 {
            for (a, b) in out.iter_mut().zip(kingdom.resources) {
                *a = a.saturating_add(b);
            }
        }
    }
    out
}
/// Spending (0x467790 → 0x4676c0), resource by resource in the order
/// gold, stone, wood, food: none of one the stock does not cover; the
/// kingdom's own first, then what is still owed from each kingdom sharing
/// with it, in kingdom order. Nothing at all in King of the Hill with
/// «molte» resources.
pub(crate) fn spend(m: &mut MissionWorld, owner: u8, cost: [u32; 4]) {
    if unlimited(m) || owner == 0 {
        return;
    }
    for r in [0, 1, 3, 2] {
        let mut left = cost[r];
        if left == 0 || stock(m, owner)[r] < left {
            continue;
        }
        let sharing: Vec<usize> = (1..=8u8)
            .zip(&m.kingdoms)
            .filter(|&(k, kingdom)| kingdom.active && relation(m, k, owner) & 2 != 0)
            .map(|(k, _)| usize::from(k) - 1)
            .collect();
        let order = std::iter::once(usize::from(owner) - 1).chain(sharing);
        for k in order {
            let Some(have) = m.kingdoms.get_mut(k).map(|x| &mut x.resources[r]) else {
                continue;
            };
            let take = (*have).min(left);
            *have -= take;
            left -= take;
            if left == 0 {
                break;
            }
        }
    }
}
/// King of the Hill with «molte» resources (0x5cf808 = 1, 0x5877b4 = 2):
/// what a kingdom gains or spends leaves its stock as it is (0x467640,
/// 0x4676c0).
fn unlimited(m: &MissionWorld) -> bool {
    m.game_rules.game_type == 1 && m.game_rules.resource_level == 2
}
/// A script's write of a kingdom's resource (0x4678d0): its own, as
/// given; nothing in King of the Hill with «molte» resources.
fn set_own(m: &mut MissionWorld, owner: u8, r: usize, v: i32) {
    if unlimited(m) {
        return;
    }
    if let Some(k) = owner
        .checked_sub(1)
        .and_then(|k| m.kingdoms.get_mut(k as usize))
    {
        k.resources[r] = v.max(0) as u32;
    }
}
/// A kingdom's own resources (K+4), as the computer reads them for its
/// plans (0x40b090).
fn resources_of(m: &MissionWorld, owner: u8) -> [u32; 4] {
    owner
        .checked_sub(1)
        .and_then(|k| m.kingdoms.get(k as usize))
        .map_or([0; 4], |k| k.resources)
}
/// Gold cost of a creature in the European table (group 1, `+0xda`).
fn creature_gold(c: &Catalog, o: &MapObject, class: usize) -> u32 {
    let mut probe = o.clone();
    probe.civilization = 1;
    c.stats(&probe, CLASSES[class], &[]).cost[0]
}
/// Least gold for a book of each spell (0x4a5220); summons cost four times
/// the creature's gold.
pub fn book_minimum(c: &Catalog, o: &MapObject, spell: u8) -> u32 {
    match spell {
        0 | 6 => 200,
        7 | 13 | 17 | 20 => 120,
        1 | 15 => 80,
        4 | 8 | 9 | 12 | 14 => 40,
        3 => 4 * creature_gold(c, o, 22),
        10 => 4 * creature_gold(c, o, 16),
        16 => 4 * creature_gold(c, o, 32),
        19 => 4 * creature_gold(c, o, 30),
        _ => 20,
    }
}
/// Gold per charge of a book of each spell (0x465ca0): charges are
/// (gold / 4) / divisor; summons use the creature's gold.
fn book_divisor(c: &Catalog, o: &MapObject, spell: u8) -> u32 {
    const D: [u32; 21] = [
        50, 20, 5, 0, 1, 5, 50, 30, 10, 10, 0, 5, 10, 30, 10, 20, 0, 30, 5, 0, 30,
    ];
    match spell {
        3 => creature_gold(c, o, 22),
        10 => creature_gold(c, o, 16),
        16 => creature_gold(c, o, 32),
        19 => creature_gold(c, o, 30),
        s => D.get(s as usize).copied().unwrap_or(1),
    }
    .max(1)
}
/// Resources moved between kingdoms (message 0x42, handler 0x48fbb6): the
/// tributes (0x447c49) and the tavern's dice, whose prize comes from nobody
/// and whose stake goes to nobody (owner 0, 0xffff in the message;
/// 0x4a4149, 0x4a4184). The amount is cut to what the sender may spend; the
/// recipient gets it, the sender loses it, and a local recipient reads
/// `[msg] resadded`. Returns the amount moved.
fn transfer(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    c: &Catalog,
    from: u8,
    to: u8,
    amount: u32,
    res: usize,
) -> u32 {
    if res >= 4 {
        return 0;
    }
    let amount = if from == 0 {
        amount
    } else {
        amount.min(stock(m, from)[res])
    };
    if amount == 0 {
        return 0;
    }
    let mut cost = [0; 4];
    cost[res] = amount;
    credit(m, to, cost);
    if from != 0 {
        spend(m, from, cost);
    }
    if to != 0 && to == m.player() {
        sim.gameplay.notices.push(c.resadded.clone());
    }
    amount
}
fn credit(m: &mut MissionWorld, owner: u8, cost: [u32; 4]) {
    if unlimited(m) {
        return;
    }
    // Owner 0 has no kingdom (0x467400).
    if let Some(k) = owner
        .checked_sub(1)
        .and_then(|k| m.kingdoms.get_mut(k as usize))
    {
        for (a, b) in k.resources.iter_mut().zip(cost) {
            *a = a.saturating_add(b);
        }
    }
}
fn new_unit(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    class: usize,
    owner: u8,
    civ: u8,
    cell: [u16; 2],
) -> std::result::Result<usize, String> {
    let mut ids = m.objects.iter().map(|o| o.id).collect();
    let first = m
        .objects
        .iter()
        .map(|o| o.id)
        .filter(|&id| id < 0xfc17)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let o = world::starting::new_object(&mut ids, first, class, owner, civ, cell, lib)
        .map_err(|e| e.to_string())?;
    let mut o = o;
    if civ == 3
        && m.rules
            .get(owner.wrapping_sub(1) as usize)
            .is_some_and(|r| {
                r.initial_researches
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case("te_greatwall"))
            })
    {
        crate::simulation::great_wall_health(&mut o);
    }
    if matches!(class, 61 | 62 | 67 | 68) {
        o.data.resize(64, 0);
    }
    if matches!(class, class::GATE_WOOD | class::GATE_STONE) {
        world::automatic_gate(&mut o);
    }
    let i = m.objects.len();
    m.objects.push(o);
    m.records = m.objects.len();
    sync(sim, m, lib);
    sim.gameplay.joins += 1;
    sim.gameplay.actors[i].joined = sim.gameplay.joins;
    // 0x4eb340, 0x4ea410: monks and priests start at the kingdom's level
    // (2 with Philosophy).
    if matches!(class, 12 | 18) && owner != 0 {
        let level = experience_rules(m, owner).monks;
        add_levels(sim, m, lib, i, level.saturating_sub(sim.objects[i].level));
    }
    Ok(i)
}
fn change_class(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    class: usize,
) {
    let old = sim.objects[i].clone();
    let o = &mut m.objects[i];
    let fraction = (o.health as u64, o.max_health.max(1) as u64);
    o.class = class;
    let base = lib.attributes.get(o);
    o.max_health = base.health;
    o.health = (base.health as u64 * fraction.0 / fraction.1).min(base.health as u64) as u16;
    if o.civilization == 3 {
        let times = m
            .rules
            .get(o.owner.wrapping_sub(1) as usize)
            .map_or(0, |r| {
                crate::simulation::completions(&r.initial_researches, "te_greatwall")
            });
        for _ in 0..times {
            crate::simulation::great_wall_health(o);
        }
    }
    o.data.resize(o.data.len().max(56), 0);
    for (at, value) in [
        (2, class::stored_base(class) as u16),
        (18, o.health),
        (20, o.max_health),
        (22, base.attack),
        (24, base.defence),
        (28, old.level),
    ] {
        o.data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    o.data[26] = base.sight.min(255) as u8;
    let o = o.clone();
    let mut state = ObjectState::read(&o, lib);
    state.contents = old.contents;
    state.holder = old.holder;
    state.order = old.order;
    state.apply_researches(&o, researches(m, o.owner));
    sim.objects[i] = state;
}
/// A unit made anew in another class (op 0x5d at 0x4ac74e, op 0x76 at
/// 0x4eb470): the old one goes (0x4ff350) and one of the new class comes
/// from its record (0x4fd4c0) at the head of its kingdom's list, with
/// nothing of the old program, load or work; only the spy it hides (+0xf6)
/// passes on. ReTzar keeps the index.
fn convert(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    class: usize,
) {
    change_class(sim, m, lib, i, class);
    let old = std::mem::take(&mut sim.gameplay.actors[i]);
    sim.gameplay.joins += 1;
    let a = &mut sim.gameplay.actors[i];
    a.mole = old.mole;
    a.sector = old.sector;
    a.sector_stamp = old.sector_stamp;
    a.joined = sim.gameplay.joins;
    a.born = sim.ticks;
    sim.objects[i].order = OrderState::Idle;
}
fn set_order(
    sim: &mut Simulation,
    i: usize,
    intent: Intent,
    append: bool,
) -> std::result::Result<(), String> {
    let a = &mut sim.gameplay.actors[i];
    // 0x48f6ba: every order lets the unit move again (+0xd6 = 1) and ends
    // a guard (0x4dd7f0).
    a.stance = 0;
    a.temporary_hold = false;
    a.guarding = None;
    a.guard_list.clear();
    a.guard_class = 0;
    // An animal's program is made again where the order leaves it; an
    // order ends a creature's Animate at once (0x4e01c0 has no priority).
    a.pasture = None;
    a.appearing_until = 0;
    if append && a.intent.is_some() {
        if a.queued.len() >= QUEUE_LIMIT {
            return Err("Coda ordini piena".into());
        }
        a.queued.push_back(intent);
    } else {
        give_way(a);
        a.intent = Some(intent);
        a.automatic = false;
        a.resume = None;
        a.murder_blow = false;
        a.move_mode = 0;
        a.group = None;
        a.queued.clear();
        halt(a);
        a.depositing = None;
        a.path_cooldown = 0;
        a.capture_target = None;
        a.capture_cooldown = 0;
        a.work_target = None;
        a.work_cell = None;
        a.action_image.clear();
        a.action_elapsed_ms = 0;
        a.action_point = None;
        a.work_cooldown = 0;
        a.search_at = None;
        a.explore_target = None;
        a.move_target = None;
        a.dig_again = false;
        // A new program: its first op starts again (vt+0x168).
        a.advance_due_ms = None;
        a.orders = a.orders.wrapping_add(1);
    }
    Ok(())
}
/// Attack and defence of an object from its record, level and researches.
fn base_stats(
    m: &MissionWorld,
    lib: &ObjectLibrary,
    o: &MapObject,
    known: Option<&[String]>,
) -> [u32; 2] {
    let mut base = ObjectState::read(o, lib);
    if let Some(known) = known.or_else(|| (o.owner > 0).then(|| researches(m, o.owner))) {
        base.apply_researches(o, known);
    }
    [base.attack, base.defence]
}
/// Move attack and defence by the change of their base, keeping what
/// potions, blessings and curses did to the current values.
fn rebase(sim: &mut Simulation, i: usize, before: [u32; 2], after: [u32; 2]) {
    let s = &mut sim.objects[i];
    s.attack = (s.attack as i64 + after[0] as i64 - before[0] as i64).max(0) as u32;
    s.defence = (s.defence as i64 + after[1] as i64 - before[1] as i64).max(0) as u32;
}
fn add_levels(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    levels: u16,
) {
    if levels == 0 {
        return;
    }
    let level = sim.objects[i].level.saturating_add(levels);
    set_level(sim, m, lib, i, level);
}
/// The level written as is (+0x66, 0x505f21): attack and defence follow it,
/// as the getters count it at each use (0x4de7d0, 0x4de880), down as up.
fn set_level(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    level: u16,
) {
    let before = base_stats(m, lib, &m.objects[i], None);
    let data = &mut m.objects[i].data;
    data.resize(data.len().max(56), 0);
    m.objects[i].data[28..30].copy_from_slice(&level.to_le_bytes());
    let after = base_stats(m, lib, &m.objects[i], None);
    sim.objects[i].level = level;
    rebase(sim, i, before, after);
}
/// The sections of HEROES.INI, names kept as written.
fn hero_names(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut names: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut section = String::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = s.trim().to_ascii_lowercase();
        } else if !line.starts_with(';') {
            names
                .entry(section.clone())
                .or_default()
                .push(line.to_owned());
        }
    }
    names
}
/// A kingdom's experience rules, written by its researches (0x4d33d0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Experience {
    /// Highest level reached by experience (+0xdc): 0, plus 4 with
    /// Learning and 10 for each finished academy, unlimited with Heroism.
    cap: u16,
    /// Threshold percentage (+0xde): 100, Warfare 70, Tactics 50, Sun Tzu 35.
    speed: u16,
    /// Maximum and current health gained at each level (+0xee, Body Building).
    health: u16,
    /// Leadership (+0xe8): a hero's experience reaches the units around it.
    nobility: bool,
    /// Hero against hero damage bonus in percent (+0xec, Honor).
    honor: u16,
    /// Tournaments enable the Kung Fu pool (+0x112 = 0).
    tournaments: bool,
    /// Starting level of monks and priests (+0x114, Philosophy).
    monks: u16,
    /// Levels added to tavern recruits (+0xfc, Fame).
    fame: u16,
}
fn experience_rules(m: &MissionWorld, owner: u8) -> Experience {
    // The handlers of 0x4d33d0 in the order the researches completed: some
    // add to a field of the kingdom at each completion, others set it.
    let mut e = Experience {
        cap: 0,
        speed: 100,
        health: 0,
        nobility: false,
        honor: 0,
        tournaments: false,
        monks: 0,
        fame: 0,
    };
    for r in researches(m, owner) {
        match r.to_ascii_lowercase().as_str() {
            "te_heroism" => e.cap = 30000,
            "te_learning" => e.cap = e.cap.saturating_add(4),
            "te_suntzubook" => e.speed = 35,
            "te_tactics" => e.speed = 50,
            "te_warfare" => e.speed = 70,
            "te_bodybuilding" => e.health = e.health.saturating_add(5),
            "te_nobility" => e.nobility = true,
            "te_honor" => e.honor = e.honor.saturating_add(50),
            "te_tournaments" => e.tournaments = true,
            "te_philosophy" => e.monks = 2,
            "te_fame" => e.fame = e.fame.saturating_add(4),
            _ => {}
        }
    }
    // 0x401850 (vt+0x2bc of the academy, called when it is finished,
    // 0x41e10c): each academy of the kingdom adds 10; its removal (0x4017c0)
    // and a change of owner (0x401800) take them back.
    let academies = m
        .objects
        .iter()
        .filter(|o| {
            o.class == class::ACADEMY
                && o.owner == owner
                && o.flags & (FLAG_INACTIVE | FLAG_CONSTRUCTION) == 0
        })
        .count();
    e.cap = e
        .cap
        .saturating_add((10 * academies).min(u16::MAX as usize) as u16);
    e
}
/// A hero (0x401920): a promoted unit (0x506d60), not dying. Scenario
/// names do not make heroes; such units are promoted at level 12 too.
pub fn hero(o: &MapObject) -> bool {
    o.flags & FLAG_HERO != 0 && o.flags & FLAG_DYING == 0
}
/// Experience points (vtable +0x1f4). Base 0x5022e0: nothing at or above the
/// kingdom's cap; the points add to the counter and reaching
/// trunc(speed × level / 100) gives one level and empties the counter.
/// The unit override 0x4de900 then adds Body Building, the promotion to hero
/// at level 12, Leadership and the Tournaments pool.
fn add_experience(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    amount: u16,
    propagate: bool,
) {
    let o = &m.objects[i];
    // Bats, siege engines, ships, fishing boats and buildings never gain
    // experience (0x4e7500, 0x4dbba0).
    if !alive(o)
        || !world::class::is_unit(o.class)
        || matches!(o.class, 15 | 19..=21 | 23..=25 | class::FISHING_BOAT)
    {
        return;
    }
    let owner = o.owner;
    let rules = experience_rules(m, owner);
    let level = sim.objects[i].level;
    if level >= rules.cap {
        return;
    }
    let s = &mut sim.objects[i];
    s.xp = s.xp.saturating_add(amount);
    let raised = s.xp as u32 >= rules.speed as u32 * level as u32 / 100;
    if raised {
        add_levels(sim, m, lib, i, 1);
        sim.objects[i].xp = 0;
    }
    // The spy keeps only the base rule (0x4ec3d0).
    if m.objects[i].class == 14 {
        return;
    }
    if raised && rules.health > 0 && !sim.gameplay.distributing {
        let x = &mut m.objects[i];
        x.max_health = x.max_health.saturating_add(rules.health);
        x.health = x.health.saturating_add(rules.health).min(x.max_health);
    }
    if !hero(&m.objects[i]) {
        if sim.objects[i].level >= 12 {
            become_hero(m, c, i);
        }
    } else if propagate && rules.nobility {
        // Leadership: own units below level 11 closer than the hero's sight.
        let at = [m.objects[i].x, m.objects[i].y];
        let sight = sim.objects[i].sight;
        let near: Vec<usize> = (0..m.objects.len())
            .filter(|&k| {
                let x = &m.objects[k];
                k != i
                    && x.owner == owner
                    && alive(x)
                    && crate::selection::unit(x)
                    && sim.objects[k].level < 11
                    && x.x.abs_diff(at[0]).max(x.y.abs_diff(at[1])) < sight
            })
            .collect();
        for k in near {
            add_experience(sim, m, lib, c, k, amount, false);
        }
    }
    if propagate && rules.tournaments {
        // Tournaments: every 75 points of the kingdom, +1 to each Asian
        // peasant, monk, ninja and priest, without Body Building.
        let pool = sim.gameplay.kungfu.entry(owner).or_insert(0);
        *pool = pool.saturating_add(amount);
        if *pool >= 75 {
            *pool = 0;
            let pupils: Vec<usize> = (0..m.objects.len())
                .filter(|&k| {
                    let x = &m.objects[k];
                    x.owner == owner
                        && alive(x)
                        && x.civilization == 3
                        && matches!(x.class, 1 | 12 | 13 | 18)
                })
                .collect();
            sim.gameplay.distributing = true;
            for k in pupils {
                add_experience(sim, m, lib, c, k, 1, false);
            }
            sim.gameplay.distributing = false;
        }
    }
}
/// Promotion (0x506d60): the hero flag and, for a unit without a name yet
/// (0x506d6d), the next name of the hero table for its civilization
/// (0x4999d0), else «Eroe» (`hero3`).
fn become_hero(m: &mut MissionWorld, c: &Catalog, i: usize) {
    m.objects[i].flags |= FLAG_HERO;
    if !m.objects[i].name.is_empty() {
        return;
    }
    let civ = m.objects[i].civilization;
    m.objects[i].name =
        take_hero(&c.heroes, &mut m.heroes_taken, civ).unwrap_or_else(|| "Eroe".into());
}
/// The hero table (0x499b60): the names of HEROES.INI, European, Asian,
/// Arab and common in that order, each with the civilizations it serves
/// (bit `1 << civilization`; every one for the common names).
pub fn hero_table(text: &str) -> Vec<(String, u16)> {
    let names = hero_names(text);
    [("euro", 2), ("asia", 8), ("arab", 4), ("comm", 0xffff)]
        .into_iter()
        .flat_map(|(section, civs)| {
            names
                .get(section)
                .into_iter()
                .flatten()
                .map(move |n| (n.clone(), civs))
        })
        .collect()
}
/// 0x4999d0: the first name of the table not taken yet that serves
/// civilization `civ` (any for 0), now taken. The names stay taken until
/// the game ends (0x4999b0): the random map screen's computer kingdoms,
/// its generator's records and every promotion draw from them.
pub fn take_hero(table: &[(String, u16)], taken: &mut Vec<bool>, civ: u8) -> Option<String> {
    let civs = if civ == 0 {
        0xffff
    } else {
        1u16 << civ.min(15)
    };
    taken.resize(taken.len().max(table.len()), false);
    let k = (0..table.len()).find(|&k| !taken[k] && table[k].1 & civs != 0)?;
    taken[k] = true;
    Some(table[k].0.clone())
}
/// The object a right click of unit `i` at the middle of `cell` falls on,
/// as vt+0x190 finds it from a point alone (a script's default order,
/// 0x4bb1a4). The peasant's handler takes first a building over the cell
/// (0x4adc8d: 0x44e400 with flag 0x20) and then a standing tree, which
/// [`right_click`] chops; every unit then takes another unit on the cell
/// (0x44e900, within 30 pixels of the cell's middle), else a building or
/// an item over it (0x4e42e7: 0x44e400 with 0x420).
pub(crate) fn point_target(
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    cell: [u16; 2],
) -> Option<usize> {
    let [cx, cy] = cell.map(usize::from);
    let over = |o: &MapObject| {
        let [w, h] = lib.cell_footprint(o);
        let [ox, oy] = [o.x as usize, o.y as usize];
        (ox..ox + w).contains(&cx)
            && (oy..oy + h).contains(&cy)
            && !lib.free_drawn_cell(o, cx - ox, cy - oy)
    };
    let standing = |j: usize| {
        let o = &m.objects[j];
        j != i && alive(o) && o.flags & FLAG_INACTIVE == 0
    };
    if m.objects[i].class == class::PEON {
        if let Some(j) = (0..m.objects.len())
            .rev()
            .find(|&j| standing(j) && m.objects[j].building() && over(&m.objects[j]))
        {
            return Some(j);
        }
        if clicked_tree(g, lib, cell).is_some() {
            return None;
        }
    }
    (0..m.objects.len())
        .rev()
        .find(|&j| {
            let o = &m.objects[j];
            standing(j) && crate::selection::unit(o) && [o.x, o.y] == cell
        })
        .or_else(|| {
            (0..m.objects.len()).rev().find(|&j| {
                let o = &m.objects[j];
                standing(j) && (o.building() || o.item()) && over(o)
            })
        })
}
/// The right click of unit `i` on `target` or on `cell` (vt+0x190): the
/// order of its class's own handler (peasant 0x4adc20, trader 0x4ed4e0,
/// ships 0x4e96e0, priest 0x4ea7b0), then of the trade goods' (0x4e8220),
/// then of the units' (0x4e4280).
#[allow(clippy::too_many_arguments)]
pub fn right_click(
    sim: &Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    target: Option<usize>,
    cell: Option<[u16; 2]>,
) -> &'static str {
    let o = &m.objects[i];
    if o.building() && !(39..=43).contains(&o.class) {
        return "move";
    }
    let Some((j, t)) = target.and_then(|j| m.objects.get(j).map(|t| (j, t))) else {
        return if o.class == class::PEON && cell.and_then(|p| clicked_tree(g, lib, p)).is_some() {
            "chop"
        } else {
            "move"
        };
    };
    let loaded = sim.gameplay.actors[i].cargo.iter().any(|&n| n > 0);
    // 0x4e449e: an object with the item flag (0x400), resource items
    // included, is picked up (program 6); a peasant gathers resource items
    // instead (0x4adc20).
    if t.item() || (t.class == class::RESITEM && o.class != class::PEON) {
        "pick_item"
    } else if o.class == class::FISHING_BOAT && t.class == class::FISHPATCH {
        "harvest"
    } else if o.class == 2 && matches!(t.class, class::MARKET | class::HARBOR | class::CASTLE) {
        // 0x4ed4e0: the trader unloads at a market, harbor or castle of
        // anyone (Unload Resource, program 0x21).
        "sell"
    } else if matches!(o.class, 23..=25 | class::FISHING_BOAT)
        && t.class == class::HARBOR
        && relation(m, t.owner, o.owner) & 0x40 != 0
    {
        // 0x4e96e0: a ship at an allied harbor.
        "sell"
    } else if o.class == class::PEON
        && (matches!(t.class, class::RES_GOLD | class::RES_STONE)
            || t.class == class::RESITEM
            || (t.class == class::COW && !enemy(m, o.owner, t.owner)))
    {
        "harvest"
    } else if o.class == class::PEON && t.class == class::FIELD {
        // 0x4adcec: a free field is dug, an occupied one only reached.
        if field_worker(sim, j).is_some_and(|w| w != i) {
            "move"
        } else {
            "harvest"
        }
    } else if o.class == class::PEON
        && loaded
        && relation(m, t.owner, o.owner) & 2 != 0
        && t.flags & FLAG_CONSTRUCTION == 0
        && store_accepts(t, sim.gameplay.actors[i].cargo, naval(o))
    {
        // 0x4adc20: Return goods to a store of the carried resource.
        "deliver"
    } else if o.class == class::PEON
        && loaded
        && matches!(t.class, class::MARKET | class::HARBOR)
        && t.flags & FLAG_CONSTRUCTION == 0
    {
        // 0x4add9f: Return goods to any finished market or harbor too
        // (vt+0x260), where nothing is delivered (0x4ae990).
        "deliver"
    } else if o.class == 18
        && ["te_healing", "te_medicine"].iter().any(|t| {
            researches(m, o.owner)
                .iter()
                .any(|r| r.eq_ignore_ascii_case(t))
        })
        && crate::selection::unit(t)
        && t.health < t.max_health
        && !machine(t)
        && relation(m, t.owner, o.owner) & 0x40 != 0
    {
        // 0x4ea7b0: the priest's right click heals a wounded friend.
        "spell spell_heal"
    } else if o.class == class::PEON && capturable(sim, m, lib, o.owner, j) {
        "capture"
    } else if o.class == class::PEON && site(m, o.owner, j) {
        "repair"
    } else if joins_route(sim, m, i, j)
        && !(o.class == class::PEON
            && matches!(t.class, 19..=21 | 23..=25)
            && relation(m, t.owner, o.owner) & 0x40 != 0)
    {
        // 0x4e8220, before the units' own checks: a trader's wagon is
        // joined, not boarded. A peasant repairs or boards a friendly ship
        // or machine instead (0x4ade08).
        "join_route"
    } else if t.class == class::DUMMY && crate::selection::unit(o) && !untrainable(o) {
        // 0x4e43d9: a right click on a training dummy trains on it
        // (program 0x42), whoever owns the dummy; peasants reach it too
        // through 0x4e8220.
        "attack"
    } else if t.class == class::BRIDGE {
        // 0x4e4339: a bridge is walked on, never attacked.
        "move"
    } else if auto_hostile(sim, m, i, j) && combat::can_attack(sim, m, lib, c, i, j, false) {
        // 0x4e43fb: a hostile object (0x468190) the unit may hit (vt+0x21c).
        "attack"
    } else if o.class == class::PEON && repairable(m, o.owner, j) {
        "repair"
    } else if mobile(o) && boardable(sim, m, lib, c, i, j) {
        // 0x4e4280: a holder the unit may enter (0x455280).
        "board"
    } else {
        "move"
    }
}
/// The cursor over a right click's target (0x4f45b0 → 0x42da60): that of
/// the program the click starts. A plain walk, Return goods, trading,
/// unloading, fishing, milking, gathering items and healing keep the arrow.
pub fn click_cursor(
    m: &MissionWorld,
    action: &str,
    target: Option<usize>,
    alt: bool,
) -> &'static str {
    let t = target.and_then(|j| m.objects.get(j));
    match action {
        // Programs 3 and 0x42; with Alt, Attack type (0x38).
        "attack" if alt => "cs_attacktype",
        "attack" => "cs_attack",
        // Programs 0xf and 0x15.
        "board" => "cs_move",
        // Chop (2), Dig (7), Mine (8) and Build (0xd).
        "chop" => "cs_harvest",
        "harvest"
            if t.is_some_and(|t| {
                matches!(t.class, class::RES_GOLD | class::RES_STONE | class::FIELD)
            }) =>
        {
            "cs_harvest"
        }
        "repair" if t.is_some_and(|t| t.flags & FLAG_CONSTRUCTION != 0) => "cs_harvest",
        // Repair (0x18).
        "repair" => "cs_repair",
        // Capture (9).
        "capture" => "cs_magicharm",
        // Pick up (6).
        "pick_item" => "cs_pick",
        _ => "",
    }
}
/// 0x4e8220: a unit that carries trade goods (0x2723) clicks a friendly
/// trader with a route (filter 0x44eac0): a living carrier with markets and
/// a load set, a ship only while it trades.
fn joins_route(sim: &Simulation, m: &MissionWorld, i: usize, j: usize) -> bool {
    let (o, t) = (&m.objects[i], &m.objects[j]);
    let a = &sim.gameplay.actors[j];
    i != j
        && cargo_room(o) > 0
        && cargo_room(t) > 0
        && alive(t)
        && a.trade_origin.is_some()
        && a.trade_partner.is_some()
        && a.route.as_ref().is_some_and(|r| r.amount > 0)
        && (!matches!(t.class, 23..=25)
            || a.intent
                .as_ref()
                .is_none_or(|x| matches!(x, Intent::Trade(_))))
        && relation(m, t.owner, o.owner) & 0x10 == 0
}
/// The trade goods a unit carries at most (message 0x2723): Asian peasants
/// and traders 500, galleons 1000, hulks 500, xebecs 300 (0x4aeb40,
/// 0x4e7f90, 0x4ea980, 0x4eabe0, 0x4eaa50).
fn cargo_room(o: &MapObject) -> u32 {
    match o.class {
        class::PEON if o.civilization == 3 => 500,
        2 | 24 => 500,
        23 => 1000,
        25 => 300,
        _ => 0,
    }
}
fn target(req: &Request, m: &MissionWorld) -> std::result::Result<usize, String> {
    req.target
        .filter(|&i| m.objects.get(i).is_some_and(alive))
        .ok_or_else(|| "Seleziona un bersaglio valido".into())
}
fn point(req: &Request, g: &MapGrid) -> std::result::Result<[u16; 2], String> {
    req.cell
        .filter(|p| (p[0] as usize) < g.side && (p[1] as usize) < g.side)
        .ok_or_else(|| "Scegli un punto della mappa".into())
}

/// Commands are checked again in the simulation, including ownership, menus,
/// dependencies, resources and placement. The HUD is not the authority.
pub fn issue(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    req: &Request,
) -> std::result::Result<String, String> {
    issue_as(sim, m, g, lib, c, req, false)
}
/// An order of the player, or of a script (`ObjOrder`), which commands the
/// objects of every kingdom.
fn issue_as(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    req: &Request,
    script: bool,
) -> std::result::Result<String, String> {
    sync(sim, m, lib);
    if req.order == "leave" {
        return leave_holder(sim, m, g, lib, req);
    }
    if req.order == "cheat" {
        return cheats::apply(sim, m, g, lib, c, req);
    }
    // Send of the tribute window (0x447bd1): an amount of a resource from
    // the player to the kingdom of a row (0x490dd0).
    if req.order == "tribute" {
        let player = m.player();
        let to = req
            .target
            .and_then(|t| u8::try_from(t).ok())
            .filter(|&t| (1..=8).contains(&t) && t != player)
            .ok_or("Regno non valido")?;
        transfer(sim, m, c, player, to, req.amount, req.resource as usize);
        return Ok(String::new());
    }
    // Alt + click on an item of the inventory (0x4a1ba8): it becomes the
    // holder's active one (vt+0x2d0 with 1, 0x463140).
    if req.order == "item_mark" {
        let j = req
            .selected
            .iter()
            .copied()
            .find(|&j| m.objects.get(j).is_some_and(|o| carried(o) && alive(o)))
            .ok_or("Seleziona un oggetto nell'inventario")?;
        let h = sim.objects[j]
            .holder
            .ok_or("L'oggetto deve essere raccolto")?;
        if !m.controls(m.objects[h].owner) {
            return Err("L'inventario non appartiene al tuo regno".into());
        }
        mark_item(sim, m, j);
        return Ok(String::new());
    }
    if c.action(&req.order).starts_with("item_") {
        return item_command(sim, m, g, lib, c, req);
    }
    let mut selected: Vec<_> = req
        .selected
        .iter()
        .copied()
        .filter(|&i| {
            m.objects.get(i).is_some_and(|o| {
                (script || m.commands(o) && o.flags & FLAG_LOCKED == 0 && !still(sim, i))
                    && alive(o)
                    && sim.objects[i].holder.is_none()
            })
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if selected.is_empty() {
        return Err("Seleziona un'unità o un edificio del tuo regno".into());
    }
    // Of several buildings, a research goes to the one with the shortest queue.
    if c.action(&req.order).starts_with("technology") {
        selected.sort_by_key(|&i| sim.gameplay.actors[i].jobs.len());
    }
    let contextual = req.order == "context";
    let full_action = if req.order == "cancel_job" {
        "cancel_job".into()
    } else {
        c.action(&req.order)
    };
    let action = if full_action.starts_with("spell ") || full_action.starts_with("loan ") {
        full_action.as_str()
    } else {
        full_action.split_whitespace().next().unwrap_or("")
    };
    let name = c.class_name(&req.order);
    let mut accepted = 0;
    let mut failure = None;
    let mut movers = Vec::new();
    // The turn of a guard list (its counter, +6 of the list, 0x4e3d37).
    let mut guard_turn = 0usize;
    for i in selected.clone() {
        let o = m.objects[i].clone();
        let uncloaking = o.class == 13
            && req.order == "uncloak"
            && sim.gameplay.actors[i].effect_until > sim.ticks;
        // The Delete key (0x4f562f) sends `die` without a menu button.
        // A spy's host shows the spy's orders (0x49f0cc, ops 0x60 and 0x61).
        let host = matches!(req.order.as_str(), "reveal_self" | "impersonate")
            && o.mole != 0
            && m.controls(o.mole);
        // A script's order (0x4bb090 → 0x4909c0) runs from its message 0x38
        // (0x48f751): any living object, of whatever kingdom, takes the
        // command (vt+0x188) without its menu or the mission's lists.
        if !script
            && !contextual
            && !host
            && !matches!(req.order.as_str(), "cancel" | "cancel_job" | "die")
            && !(req.order.starts_with("hire:") && o.class == class::TAVERN)
            && !((req.order.starts_with("buy:") || req.order.starts_with("sell:"))
                && o.class == class::MARKET)
            && !uncloaking
            && !c.permitted(&req.order, &o, lib)
            && !sim.gameplay.actors[i].spells.iter().any(|&s| {
                world::spell_key(s) == Some(c.action(&req.order).trim_start_matches("spell "))
            })
        {
            failure = Some("Questa unità non può eseguire l'ordine".into());
            continue;
        }
        if !script && let Err(e) = c.available(&req.order, &o, m) {
            failure = Some(e);
            continue;
        }
        let actual = if contextual && o.building() && !(39..=43).contains(&o.class) {
            "move"
        } else if contextual {
            right_click(sim, m, g, lib, c, i, req.target, req.cell)
        } else {
            action
        };
        let result = (|| -> std::result::Result<(), String> {
            match actual {
                // 0x4e3725: the program (0x42f3a0) takes the point clicked.
                "explore" => {
                    let p = point(req, g)?;
                    set_order(sim, i, Intent::Explore(p), req.append)?;
                }
                "move" | "move_advance" | "move_sneak" => {
                    let p = point(req, g)?;
                    // Ctrl selects Move Advance (0x31), Alt move sneak (0x39).
                    let mode = if actual == "move_advance" || req.ctrl {
                        1
                    } else if actual == "move_sneak" || req.alt {
                        2
                    } else {
                        0
                    };
                    if o.building() {
                        // 0x41e760: only a finished building that trains units
                        // (message 0x2727, vt+0x2c0) keeps a rally point; the
                        // others ignore the click.
                        if !rally_building(o.class) || o.flags & FLAG_CONSTRUCTION != 0 {
                            return Ok(());
                        }
                        // 0x41e7a3: a right click sets the rally point with its
                        // default action, Ctrl a plain one, Ctrl+Alt an advance.
                        // 0x41e877: a click inside the building clears it.
                        let r = movement::footprint_rect(lib, &o);
                        let (x, y) = (p[0] as i32, p[1] as i32);
                        let a = &mut sim.gameplay.actors[i];
                        if x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3] {
                            a.rally = None;
                            return Ok(());
                        }
                        a.rally = Some(p);
                        a.rally_mode = if req.ctrl && req.alt {
                            2
                        } else if contextual && !req.ctrl {
                            1
                        } else {
                            0
                        };
                        return Ok(());
                    }
                    if !mobile(&o) {
                        return Err("L'oggetto non può muoversi".into());
                    }
                    // Every unit gets the clicked point (0x4f3155); a group
                    // walks it in formation.
                    set_order(sim, i, Intent::Move(p), req.append)?;
                    sim.gameplay.actors[i].move_mode = mode;
                    // 0x503fa0: a click on an object gives the Move its
                    // rect, taken now (vt+0x134; a field's own cell,
                    // vt+0x130).
                    if let Some(j) = req.target.filter(|&j| j != i && j < m.objects.len())
                        && !req.append
                    {
                        let rect = if m.objects[j].class == class::FIELD {
                            movement::footprint_rect(lib, &m.objects[j])
                        } else {
                            movement::target_rect(sim, m, lib, j)
                        };
                        sim.gameplay.actors[i].move_target = Some((j, rect));
                    }
                }
                "stop" | "stand_ground" | "hold_steady" => {
                    // A human kingdom's stopped unit holds its place for a
                    // few seconds (0x4e1bd3); stand_ground until the next
                    // order (0x503890); hold_steady waits without fighting
                    // (program 0x3c).
                    let human = !ai_managed(sim, m, i);
                    let a = &mut sim.gameplay.actors[i];
                    give_way(a);
                    a.ordered_idle = true;
                    a.intent = None;
                    halt(a);
                    a.queued.clear();
                    a.goal = None;
                    a.guarding = None;
                    a.guard_list.clear();
                    a.guard_class = 0;
                    a.automatic = false;
                    a.resume = None;
                    a.search_at = None;
                    // Stop installs the Idle program itself (priority 4000):
                    // no default program, the idle ticks from now.
                    a.idle_ms = 0;
                    a.scan_due_ms = Some(sim.ticks * TICK_MS + 1500);
                    a.idle_since_ms = sim.ticks * TICK_MS;
                    // Its Idle holds the place as any Idle after an order
                    // (0x4e1bd3): when the program it stopped was one.
                    (a.stance, a.temporary_hold) = match actual {
                        "stand_ground" => (1, false),
                        "hold_steady" => (2, false),
                        _ if human && a.last_priority >= 4000 => (1, true),
                        _ => (0, false),
                    };
                    sim.objects[i].order = OrderState::Idle;
                }
                "holdfire" | "fireatwill" => {
                    // 0x4db708: both buttons toggle the tower's hold fire.
                    let a = &mut sim.gameplay.actors[i];
                    a.hold_fire = !a.hold_fire;
                    a.intent = None;
                }
                "build" => {
                    if o.class != class::PEON {
                        return Err("Richiede un contadino".into());
                    }
                    if req.append
                        && req.selected.iter().any(|&worker| {
                            m.objects.get(worker).is_some_and(|x| {
                                x.owner == o.owner && x.class == class::PEON && alive(x)
                            }) && sim.gameplay.actors[worker].intent.is_some()
                                && sim.gameplay.actors[worker].queued.len() >= QUEUE_LIMIT
                        })
                    {
                        return Err("Coda ordini piena".into());
                    }
                    let class = CLASSES
                        .iter()
                        .position(|&s| s == name)
                        .ok_or("Edificio sconosciuto")?;
                    let p = point(req, g)?;
                    // As the preview tests it (`building_at`): a fresh record.
                    let mut probe = o.clone();
                    probe.class = class;
                    probe.x = p[0];
                    probe.y = p[1];
                    probe.custom_image.clear();
                    probe.variant = 0;
                    probe.flags = 0;
                    probe.data = vec![0; 77];
                    if !site_test(sim, m, g, lib, &probe)
                        || (o.owner == m.player() && !explored(sim, lib, &probe))
                    {
                        return Err("Spazio non edificabile o occupato".into());
                    }
                    let j = lay_site(sim, m, lib, c, &o, class, p)?;
                    // The peasants dig a field at once (0x48f5f0, program 7).
                    let field = class == class::FIELD;
                    for &worker in &req.selected {
                        if m.objects.get(worker).is_some_and(|x| {
                            x.owner == o.owner && x.class == class::PEON && alive(x)
                        }) {
                            if field {
                                // 0x48f603: a field starts Dig at once.
                                set_order(sim, worker, Intent::Harvest(j), req.append)?;
                                continue;
                            }
                            // 0x48f160: a peasant at work on its Build program
                            // (not waiting or searching) takes the new site
                            // after its own.
                            let a = &sim.gameplay.actors[worker];
                            let busy = a.search_at.is_none()
                                && matches!(a.intent, Some(Intent::Build(k)) if site(m, m.objects[worker].owner, k));
                            if busy {
                                if sim.gameplay.actors[worker].queued.len() < QUEUE_LIMIT {
                                    sim.gameplay.actors[worker]
                                        .queued
                                        .push_back(Intent::Build(j));
                                }
                            } else {
                                set_order(sim, worker, Intent::Build(j), req.append)?;
                            }
                        }
                    }
                }
                "create" if name == "crusadeanchor" => {
                    // 0x439760: the cathedral places an anchor where its
                    // crusaders gather; one crusade per kingdom at a time.
                    let p = point(req, g)?;
                    if o.flags & FLAG_CONSTRUCTION != 0 {
                        return Err("Edificio ancora in costruzione".into());
                    }
                    let t = sim.gameplay.treasury.entry(o.owner).or_default().clone();
                    let (Some(ready), None) = (t.crusaders, &t.crusade) else {
                        return Err("Una crociata è già in corso".into());
                    };
                    let mut probe = o.clone();
                    probe.class = class::CRUSADEANCHOR;
                    let [w, h] = footprint(lib, &probe);
                    probe.x = p[0].saturating_sub((w / 2) as u16);
                    probe.y = p[1].saturating_sub((h / 2) as u16);
                    if !placement(g, m, lib, &probe, None, true) {
                        return Err("Spazio non disponibile per la crociata".into());
                    }
                    begin_crusade(sim, m, g, lib, c, &o, ready, [probe.x, probe.y])?;
                }
                "create" | "technology" | "upgrade" => {
                    if actual == "upgrade" && !ship_upgrade_available(sim, i, &name) {
                        return Err("Aggiornamento navale non disponibile a questo livello".into());
                    }
                    if o.flags & FLAG_CONSTRUCTION != 0 {
                        return Err("Edificio ancora in costruzione".into());
                    }
                    // There is no cap on the queue, only 250 of each unit
                    // (0x41ec38); research and upgrades join the same queue.
                    let queued = sim.gameplay.actors[i]
                        .jobs
                        .iter()
                        .filter(|j| matches!(&j.kind, JobKind::Train(k) if CLASSES[*k] == name))
                        .count();
                    if actual == "create" && queued >= JOB_TYPE_LIMIT {
                        return Err("Coda di produzione piena".into());
                    }
                    let stats = c.stats(&o, &name, researches(m, o.owner));
                    let kind = match actual {
                        "create" => JobKind::Train(
                            CLASSES
                                .iter()
                                .position(|&s| s == name)
                                .ok_or("Unità sconosciuta")?,
                        ),
                        "technology" => {
                            // 0x41eab0: refused while another building
                            // researches it, when this one is idle.
                            if sim.gameplay.actors[i].jobs.is_empty()
                                && researching(sim, m, o.owner, &name)
                            {
                                return Err("Ricerca già in corso".into());
                            }
                            JobKind::Research(name.clone())
                        }
                        _ => JobKind::Upgrade(
                            name.clone(),
                            full_action
                                .split_whitespace()
                                .nth(1)
                                .and_then(|n| CLASSES.iter().position(|&s| s == n)),
                        ),
                    };
                    // 0x4fab10: raising a wooden wall to stone costs a
                    // segment's price for each segment of its row, a gate
                    // counting ten (0x4fa710); the row then shares this
                    // segment's queue (0x4fa580).
                    let row = (actual == "upgrade"
                        && name == "to_wall_stone"
                        && o.class == class::WALL_WOOD)
                        .then(|| wall_line(m, lib, i));
                    let cost = row.as_ref().map_or(stats.cost, |row| {
                        let count: u32 = row
                            .iter()
                            .map(|&k| {
                                if m.objects[k].class == class::GATE_WOOD {
                                    10
                                } else {
                                    1
                                }
                            })
                            .sum();
                        stats.cost.map(|n| n.saturating_mul(count))
                    });
                    debit(c, m, o.owner, cost)?;
                    if let Some(row) = row {
                        for other in sim.gameplay.wall_rows.values_mut() {
                            other.retain(|k| !row.contains(k));
                        }
                        sim.gameplay.wall_rows.insert(i, row);
                    }
                    // TIME is read again when the step starts (0x41fc29).
                    let total = stats.time_ms.max(1);
                    sim.gameplay.actors[i].jobs.push_back(Job {
                        kind,
                        remaining: total,
                        total,
                        paid: cost,
                        due_ms: None,
                        waiting: false,
                    });
                    start_step(sim, i);
                    sim.gameplay.actors[i].production_blocked = None;
                }
                "cancel" | "cancel_job" => {
                    // 0x4fa810: a wall segment hands the order to its row's
                    // leader.
                    let i = producer(sim, i);
                    let slot = if req.order == "cancel_job" {
                        req.target.unwrap_or(usize::MAX)
                    } else {
                        sim.gameplay.actors[i].jobs.len().saturating_sub(1)
                    };
                    if sim.gameplay.actors[i].jobs.is_empty()
                        && let Some(x) = sim.gameplay.actors[i].exchange.take()
                    {
                        // 0x4d26d0: what was not delivered is refunded in
                        // proportion, (total − delivered) × paid / total.
                        let mut amount = [0; 4];
                        amount[x.from as usize] = ((x.total - x.delivered) as u64 * x.paid as u64
                            / x.total.max(1) as u64)
                            as u32;
                        credit(m, o.owner, amount);
                        return Ok(());
                    }
                    let Some(job) = sim.gameplay.actors[i].jobs.remove(slot) else {
                        return Err("Nessuna attività da annullare".into());
                    };
                    // 0x41f6ec: a step under way gives back the cost of its
                    // record as it is now; the next step starts.
                    let refund = if slot == 0 && job.due_ms.is_some() {
                        match &job.kind {
                            JobKind::Train(k) => {
                                c.stats(&o, CLASSES[*k], researches(m, o.owner)).cost
                            }
                            JobKind::Upgrade(n, _) if sim.gameplay.wall_rows.contains_key(&i) => {
                                // 0x4fab10 again: the row as it is now.
                                let count: u32 = wall_line(m, lib, i)
                                    .iter()
                                    .map(|&k| {
                                        if m.objects[k].class == class::GATE_WOOD {
                                            10
                                        } else {
                                            1
                                        }
                                    })
                                    .sum();
                                c.stats(&o, n, researches(m, o.owner))
                                    .cost
                                    .map(|v| v.saturating_mul(count))
                            }
                            JobKind::Research(n) | JobKind::Upgrade(n, _) => {
                                c.stats(&o, n, researches(m, o.owner)).cost
                            }
                            JobKind::Item(..) => job.paid,
                        }
                    } else if let JobKind::Train(k) = &job.kind {
                        // 0x41ea3f: a unit gives back its class's cost as
                        // it is now, wherever it waits.
                        c.stats(&o, CLASSES[*k], researches(m, o.owner)).cost
                    } else {
                        job.paid
                    };
                    credit(m, o.owner, refund);
                    // Case 0x2f of 0x4fa810: the row breaks up.
                    if matches!(&job.kind, JobKind::Upgrade(n, _) if n == "to_wall_stone") {
                        sim.gameplay.wall_rows.remove(&i);
                    }
                    // 0x41ea81: the unit under way with no other unit after
                    // it ends the building's whole program, so what was
                    // queued after it goes, with nothing given back.
                    if slot == 0
                        && job.due_ms.is_some()
                        && matches!(job.kind, JobKind::Train(_))
                        && !sim.gameplay.actors[i]
                            .jobs
                            .iter()
                            .any(|j| matches!(j.kind, JobKind::Train(_)))
                    {
                        sim.gameplay.actors[i].jobs.clear();
                    }
                    if slot == 0
                        && let Some(next) = sim.gameplay.actors[i].jobs.front().cloned()
                    {
                        let total = job_time(c, m, &o, &next);
                        let front = sim.gameplay.actors[i].jobs.front_mut().unwrap();
                        front.total = total;
                        front.remaining = total;
                        front.due_ms = Some(sim.ticks * TICK_MS + 50);
                    }
                    sim.gameplay.actors[i].production_blocked = None;
                }
                "attack_ground" => {
                    // 0x4db7be: an explicit attack ends a tower.s hold fire.
                    sim.gameplay.actors[i].hold_fire = false;
                    if sim.objects[i].attack == 0 {
                        return Err("L'unità non può attaccare".into());
                    }
                    set_order(sim, i, Intent::AttackGround(point(req, g)?), req.append)?;
                }
                "murder" => {
                    // 0x437250: any unit.
                    let j = target(req, m)?;
                    if j == i
                        || !crate::selection::unit(&m.objects[j])
                        || !target_known(sim, m, o.owner, j)
                    {
                        return Err("Bersaglio non visibile o non disponibile".into());
                    }
                    set_order(sim, i, Intent::Murder(j), req.append)?;
                }
                "attack" => {
                    sim.gameplay.actors[i].hold_fire = false;
                    // 0x4e3979: the objects gathered with Shift (0x4356c2)
                    // make a program of a Move within the unit's range of
                    // each (op 9) and an Attack, but no Attack is given its
                    // object (+0xc stays 0, 0x42bab0): each ends at once
                    // (0x4df83d), and the unit only walks up to them in turn.
                    if req.targets.len() > 1 {
                        if req.targets.iter().any(|&j| j >= m.objects.len() || j == i) {
                            return Err("Bersaglio non attaccabile".into());
                        }
                        if sim.objects[i].attack == 0 {
                            return Err("L'unità non può attaccare".into());
                        }
                        set_order(sim, i, Intent::Reach(req.targets[0]), false)?;
                        let a = &mut sim.gameplay.actors[i];
                        a.queued
                            .extend(req.targets[1..].iter().map(|&j| Intent::Reach(j)));
                        return Ok(());
                    }
                    let j = target(req, m)?;
                    if !target_known(sim, m, o.owner, j) {
                        return Err("Bersaglio non visibile o non disponibile".into());
                    }
                    // 0x435500: the Attack order takes the unit or building
                    // under the cursor, one a selected unit can hit first
                    // (0x44ed30), any other else (0x44ea20), allies and the
                    // own kingdom included, and the unit's Attack program
                    // takes it as it is (0x4e3923). Only the right click
                    // looks at the relations (0x4e43fb).
                    let x = &m.objects[j];
                    if contextual && !auto_hostile(sim, m, i, j) && x.class != class::DUMMY {
                        return Err("Bersaglio alleato".into());
                    }
                    if sim.objects[i].attack == 0 {
                        return Err("L'unità non può attaccare".into());
                    }
                    if j == i || !(crate::selection::unit(x) || x.building()) {
                        return Err("Bersaglio non attaccabile".into());
                    }
                    set_order(sim, i, Intent::Attack(j), req.append)?;
                    // 0x4eb950 (vt+0x194): a ninja with 60 mana ordered to
                    // attack a unit murders it with its first blow.
                    sim.gameplay.actors[i].murder_blow = o.class == 13
                        && crate::selection::unit(x)
                        && sim.objects[i].mana >= 60
                        && !req.append;
                    // Alt: "Attack type of objects" (program 0x38), from the
                    // right click as from the Attack command (0x435603).
                    sim.gameplay.actors[i].attack_type = req.alt.then_some(m.objects[j].class);
                }
                "harvest" | "dig" | "mine" | "fishing" => {
                    if req.target.is_none() && o.class == class::PEON {
                        if let Some(p) = req.cell.and_then(|p| clicked_tree(g, lib, p)) {
                            set_order(sim, i, Intent::Chop(p), req.append)?;
                            return Ok(());
                        }
                    }
                    let j = target(req, m)?;
                    if !matches!(o.class, class::PEON | class::FISHING_BOAT) {
                        return Err("Richiede un lavoratore".into());
                    }
                    let t = &m.objects[j];
                    let harvestable = match t.class {
                        class::RES_GOLD | class::RES_STONE | class::FIELD => o.class == class::PEON,
                        // Gather Resource Items (program 0x1e).
                        class::RESITEM => o.class == class::PEON,
                        // Tend Cows only milks the kingdom's own cows (0x4adf02).
                        class::COW => o.class == class::PEON && !enemy(m, o.owner, t.owner),
                        class::FISHPATCH => o.class == class::FISHING_BOAT,
                        _ => false,
                    };
                    if !harvestable {
                        return Err("Risorsa non raccoglibile".into());
                    }
                    set_order(sim, i, Intent::Harvest(j), req.append)?;
                }
                "deliver" => {
                    let j = target(req, m)?;
                    set_order(sim, i, Intent::Deliver(j), req.append)?;
                }
                "sell" => {
                    let j = target(req, m)?;
                    set_order(sim, i, Intent::Sell(j), req.append)?;
                }
                "join_route" => {
                    // Trade (program 0x17, 0x5042f8): the markets and the
                    // resources of the clicked trader, its load cut to this
                    // unit's room; the goods carried stay (0x4e8060).
                    let j = target(req, m)?;
                    let a = &sim.gameplay.actors[j];
                    let (Some(origin), Some(partner), Some(theirs)) =
                        (a.trade_origin, a.trade_partner, a.route.clone())
                    else {
                        return Err("Nessuna rotta da seguire".into());
                    };
                    let mine = sim.gameplay.actors[i].route.clone().unwrap_or_default();
                    sim.gameplay.actors[i].route = Some(Route {
                        resources: theirs.resources,
                        amount: theirs.amount.min(cargo_room(&o)),
                        distance: route_distance(lib, m, origin, partner),
                        map_size: g.side as u32,
                        cargo: mine.cargo,
                        cargo_type: mine.cargo_type,
                        ..Default::default()
                    });
                    sim.gameplay.actors[i].trade_origin = Some(origin);
                    sim.gameplay.actors[i].trade_partner = Some(partner);
                    set_order(sim, i, Intent::Trade(origin), req.append)?;
                }
                "chop" => {
                    let p = clicked_tree(g, lib, point(req, g)?)
                        .filter(|_| o.class == class::PEON)
                        .ok_or("Seleziona un albero")?;
                    set_order(sim, i, Intent::Chop(p), req.append)?;
                }
                "repair" => {
                    // 0x4ada2d: a list gathered with Shift makes a program of
                    // a Move and a Repair per object, but every Move is given
                    // the first object and no Repair any: the peasant walks to
                    // the first and repairs nothing.
                    if req.targets.len() > 1 {
                        let j = req.targets[0];
                        if j >= m.objects.len() || o.class != class::PEON {
                            return Err("Seleziona un edificio del tuo regno".into());
                        }
                        let p = [m.objects[j].x, m.objects[j].y];
                        set_order(sim, i, Intent::Move(p), req.append)?;
                        let rect = movement::target_rect(sim, m, lib, j);
                        sim.gameplay.actors[i].move_target = Some((j, rect));
                        return Ok(());
                    }
                    let j = target(req, m)?;
                    if o.class != class::PEON || !(site(m, o.owner, j) || repairable(m, o.owner, j))
                    {
                        return Err("Seleziona un edificio del tuo regno".into());
                    }
                    set_order(
                        sim,
                        i,
                        if m.objects[j].flags & FLAG_CONSTRUCTION != 0 {
                            Intent::Build(j)
                        } else {
                            Intent::Repair(j)
                        },
                        req.append,
                    )?;
                }
                "capture" => {
                    let j = target(req, m)?;
                    // 0x435c30: any finished building that may be captured.
                    if !capture_target(m, lib, j) || !mobile(&o) {
                        return Err("Bersaglio non catturabile".into());
                    }
                    set_order(sim, i, Intent::Capture(j), req.append)?;
                }
                "guard" => {
                    // 0x436400: Alt guards the clicked object's class: the
                    // unit keeps its program (0x4e3d1c, vt+0x250).
                    if req.alt && req.targets.len() <= 1 {
                        let j = target(req, m)?;
                        let a = &mut sim.gameplay.actors[i];
                        a.stance = 0;
                        a.temporary_hold = false;
                        a.guarding = None;
                        a.guard_list.clear();
                        a.guard_class = m.objects[j].class as u16;
                        return Ok(());
                    }
                    // 0x4e3cf7: the unit joins the guardians of each object
                    // and stays idle where it is; its idle scan brings it
                    // back within 3 cells of the first (0x4e4f0f). Of a
                    // list, each guard takes the next object in turn as its
                    // main one, registered last.
                    let list: Vec<usize> = if req.targets.is_empty() {
                        vec![target(req, m)?]
                    } else {
                        req.targets.clone()
                    };
                    if list.iter().any(|&j| j >= m.objects.len()) {
                        return Err("Bersaglio non valido".into());
                    }
                    if list == [i] {
                        return Err("Scegli un altro oggetto da proteggere".into());
                    }
                    let main = if list.len() > 1 {
                        let k = guard_turn % list.len();
                        guard_turn += 1;
                        k
                    } else {
                        0
                    };
                    let mut guards: Vec<usize> = list
                        .iter()
                        .enumerate()
                        .filter(|&(k, _)| k != main)
                        .map(|(_, &j)| j)
                        .collect();
                    guards.push(list[main]);
                    guards.dedup();
                    set_order(sim, i, Intent::Guard(guards[0]), false)?;
                    let a = &mut sim.gameplay.actors[i];
                    a.intent = None;
                    a.guarding = Some(guards[0]);
                    a.guard_list = guards;
                    sim.objects[i].order = OrderState::Idle;
                }
                "patrol" => {
                    // 0x42f7d0: one point goes back and forth from the start;
                    // several (Shift) make a round, closed when the last is
                    // within 8 cells of the first.
                    let mut points: Vec<[u16; 2]> = req
                        .waypoints
                        .iter()
                        .copied()
                        .filter(|p| (p[0] as usize) < g.side && (p[1] as usize) < g.side)
                        .take(32)
                        .collect();
                    if points.is_empty() {
                        points.push(point(req, g)?);
                    }
                    let intent = if points.len() == 1 {
                        if points[0] == [o.x, o.y] {
                            return Err("Scegli un punto diverso dalla posizione".into());
                        }
                        Intent::Patrol([o.x, o.y], points[0])
                    } else {
                        let (a, b) = (points[0], points[points.len() - 1]);
                        let closed = a[0].abs_diff(b[0]).max(a[1].abs_diff(b[1])) < 8;
                        Intent::Circuit(points, 0, closed)
                    };
                    set_order(sim, i, intent, req.append)?;
                }
                "pick_item" => {
                    let j = target(req, m)?;
                    if !carried(&m.objects[j])
                        || sim.objects[j].holder.is_some()
                        || !accepts(lib, c, &o, &m.objects[j])
                    {
                        return Err("Oggetto non disponibile".into());
                    }
                    // Ctrl on a right click (0x4e44bf): program 0x41, pick and use.
                    let intent = if contextual && req.ctrl {
                        Intent::PickUse(j)
                    } else {
                        Intent::Pick(j)
                    };
                    set_order(sim, i, intent, req.append)?;
                }
                "board" => {
                    let j = target(req, m)?;
                    if !mobile(&o)
                        || !boardable(sim, m, lib, c, i, j)
                        || sim.objects[j].contents.len() >= holder_capacity(sim, m, lib, j)
                    {
                        return Err("Questo contenitore non può accogliere l'unità".into());
                    }
                    set_order(sim, i, Intent::Board(j), req.append)?;
                }
                "unload" => {
                    if sim.objects[i]
                        .contents
                        .iter()
                        .all(|&j| carried(&m.objects[j]))
                    {
                        return Err("Nessuna unità a bordo".into());
                    }
                    set_order(sim, i, Intent::Unload(point(req, g)?), req.append)?;
                }
                "gateopen" | "gateclose" | "gate_subai_on" | "gate_subai_off" => {
                    let o = &mut m.objects[i];
                    o.data.resize(o.data.len().max(77), 0);
                    // 0x456fd0. Native names describe the drawbridge itself:
                    // gateopen raises it and closes the passage (Italian
                    // "Chiudi porta") when no land unit stands in it;
                    // gateclose lowers it when nothing stands there. Either
                    // keeps the automatism away for 20 of its 500 ms steps.
                    let raised = o.byte(57) != 0;
                    match actual {
                        "gateopen" if !raised && gate_passage_free(sim, m, i) => {
                            move_gate(sim, m, g, lib, i, true);
                            sim.gameplay.actors[i].gate_hold = 20;
                        }
                        "gateclose" if raised && !gate_passage_held(sim, m, lib, i) => {
                            sim.gameplay.actors[i].gate_hold = 20;
                            move_gate(sim, m, g, lib, i, false);
                        }
                        "gate_subai_on" | "gate_subai_off" => {
                            m.objects[i].data[65..69].copy_from_slice(
                                &u32::from(actual == "gate_subai_on").to_le_bytes(),
                            );
                        }
                        _ => {}
                    }
                }
                "trade" | "trade_ship" => {
                    let j = target(req, m)?;
                    if !matches!(m.objects[j].class_name(), "market" | "harbor") {
                        return Err("Seleziona un mercato o porto".into());
                    }
                    // D_TRADE (0x4a2f36) names both markets, of any owner.
                    let market = |k: usize| {
                        m.objects.get(k).is_some_and(|x| {
                            alive(x) && matches!(x.class_name(), "market" | "harbor")
                        })
                    };
                    let origin = req
                        .origin
                        .filter(|&k| market(k))
                        .ok_or("Scegli il mercato o porto di partenza")?;
                    // 0x4e8060: the load per trip (500, 300 or 100 by
                    // civilization, Asian peasants only), the Chebyshev
                    // distance between the centres (0 below 12) and the map size.
                    let cap = trade_capacity(&o);
                    if cap == 0 {
                        return Err("Questa unità non commercia".into());
                    }
                    // The goods already carried stay (0x4e8060).
                    let mine = sim.gameplay.actors[i].route.clone().unwrap_or_default();
                    sim.gameplay.actors[i].route = Some(Route {
                        resources: [req.resource.min(3), req.resource2.min(3)],
                        amount: req.amount.clamp(50, cap),
                        distance: route_distance(lib, m, origin, j),
                        map_size: g.side as u32,
                        cargo: mine.cargo,
                        cargo_type: mine.cargo_type,
                        ..Default::default()
                    });
                    sim.gameplay.actors[i].trade_origin = Some(origin);
                    sim.gameplay.actors[i].trade_partner = Some(j);
                    set_order(sim, i, Intent::Trade(origin), req.append)?;
                }
                "emerge" => {
                    // 0x5037fa: everyone steps off as from a dead holder
                    // (0x504890(-1, -1)); then, for the player, each of them
                    // joins the selection (0x453240).
                    let contents = sim.objects[i].contents.clone();
                    movement::refresh(sim, m, g, lib);
                    release_passengers(sim, m, lib, i);
                    if o.owner == m.player() {
                        for j in contents {
                            if sim.gameplay.script.outbox.len() < 1024 {
                                sim.gameplay.script.outbox.push(Show::Select(j));
                            }
                        }
                    }
                }
                "die" => {
                    // Command 1 (0x5038df): the kingdom loses 100 points
                    // (0x443430 type 3); the magic gateway ignores it (0x421190).
                    if o.class == class::MAGIC_GATEWAY {
                        return Ok(());
                    }
                    if (1..=8).contains(&o.owner) {
                        let s = sim.gameplay.score.entry(o.owner).or_default();
                        *s = s.saturating_sub(100);
                    }
                    refund_site(sim, m, c, i);
                    kill(sim, m, g, lib, i);
                }
                "bribe" => {
                    let j = target(req, m)?;
                    if !bribable(m, o.owner, j) || !target_known(sim, m, o.owner, j) {
                        return Err("Seleziona un'unità avversaria".into());
                    }
                    set_order(sim, i, Intent::Bribe(j), req.append)?;
                }
                "impersonate" => {
                    // 0x437390 → 0x44ecc0: a unit the spy can be (vt+0xc0).
                    let j = target(req, m)?;
                    if j == i || !impersonable(lib, &m.objects[j]) {
                        return Err("Seleziona un'unità".into());
                    }
                    set_order(sim, i, Intent::Impersonate(j), req.append)?;
                }
                "gamble" => {
                    // D_GAMBLE (0x4a3fee): a roll of 1–100 wins at 50, 33, 20
                    // or 10 and pays the stake 1, 2, 5 or 10 times; a loss
                    // takes the stake. The stake is not taken before the roll.
                    let r = req.resource.min(3) as usize;
                    let stake = req.amount;
                    if stake < 50 || stake % 50 != 0 {
                        return Err("Scegli una puntata di almeno 50".into());
                    }
                    if stake > stock(m, o.owner)[r] {
                        return Err("Risorse insufficienti per la puntata".into());
                    }
                    let (chance, pays) =
                        [(50, 1), (33, 2), (20, 5), (10, 10)][req.option.min(3) as usize];
                    let roll = 1 + random_aux(sim, 100);
                    if roll <= chance {
                        transfer(sim, m, c, 0, o.owner, stake * pays, r);
                        sound(sim, i, "@gamble/win.wav", crate::audio::category::ORDER);
                    } else {
                        transfer(sim, m, c, o.owner, 0, stake, r);
                        sound(sim, i, "@gamble/lose.wav", crate::audio::category::ORDER);
                    }
                }
                a if a.starts_with("loan ") => {
                    // 0x4d2cec: one loan per kingdom at a time, of K+0xfe.
                    let r = match a.split_whitespace().nth(1) {
                        Some("gold") => 0,
                        Some("stone") => 1,
                        Some("food") => 2,
                        _ => 3,
                    };
                    let t = sim.gameplay.treasury.entry(o.owner).or_default();
                    if t.loan.is_some() {
                        return Err("Restituisci prima il prestito precedente".into());
                    }
                    t.loan = Some(r as u8);
                    let mut amount = [0; 4];
                    amount[r] = t.loan_amount;
                    credit(m, o.owner, amount);
                }
                "loan_return" => {
                    // 0x4d2d6a: the amount plus 200; the next loan grows by
                    // 200 a minute later.
                    let t = sim.gameplay.treasury.entry(o.owner).or_default().clone();
                    let r = t.loan.ok_or("Nessun prestito da restituire")? as usize;
                    let mut amount = [0; 4];
                    amount[r] = t.loan_amount + 200;
                    debit(c, m, o.owner, amount)?;
                    let now = sim.ticks;
                    let t = sim.gameplay.treasury.entry(o.owner).or_default();
                    t.loan = None;
                    t.raise = Some((now + 1200, t.loan_amount + 200));
                }
                "trade1" => {
                    // D_TRADE1 (0x4d3104): offer 500–20000 in steps of 500
                    // for a tenth of it, delivered one unit every 300 ms.
                    // K+0x126: one exchange per kingdom at a time.
                    if exchanging(sim, m, o.owner) {
                        return Err("Scambio già in corso".into());
                    }
                    let (from, to) = (req.resource.min(3), req.resource2.min(3));
                    let paid = req.amount;
                    // The dialog also accepts the same resource on both sides.
                    if !(500..=20000).contains(&paid) || paid % 500 != 0 {
                        return Err("Scegli una quantità da 500 a 20000".into());
                    }
                    let mut cost = [0; 4];
                    cost[from as usize] = paid;
                    debit(c, m, o.owner, cost)?;
                    sim.gameplay.actors[i].exchange = Some(Exchange {
                        from,
                        to,
                        paid,
                        total: paid / 10,
                        delivered: 0,
                        next: sim.ticks + 6,
                    });
                }
                a if a.starts_with("hire:") => {
                    // Command 0x66 of a tavern offer (0x4d2e2d): refused while
                    // the population is full (0x4683f0); the price is paid,
                    // then the units come out by a corner of the tavern drawn
                    // R(0, 3), each on the free cell nearest to it, a hero
                    // promoted at once (vt+0x12c); the offer goes.
                    let id: u32 = a[5..].parse().map_err(|_| "Offerta non valida")?;
                    let Some(list) = (0..2)
                        .find(|&l| sim.gameplay.actors[i].hires[l].iter().any(|h| h.id == id))
                    else {
                        return Err("Offerta non più disponibile".into());
                    };
                    let (pop, housing, cap) = population_parts(m, lib, o.owner);
                    if pop >= housing.min(cap) {
                        return Err(c.poplimit[usize::from(cap >= housing)].clone());
                    }
                    let offer = sim.gameplay.actors[i].hires[list]
                        .iter()
                        .find(|h| h.id == id)
                        .unwrap()
                        .clone();
                    let mut probe = o.clone();
                    probe.class = offer.class;
                    probe.civilization = offer.civilization;
                    let cost = c
                        .stats(&probe, CLASSES[offer.class], researches(m, o.owner))
                        .cost;
                    debit(c, m, o.owner, hire_price(cost, offer.count, offer.level))?;
                    let [w, h] = footprint(lib, &o).map(|n| n as i32);
                    let (x, y) = (o.x as i32, o.y as i32);
                    let corner = match random(sim, 4) {
                        0 => [x + w, y + h],
                        1 => [x - 1, y + h],
                        2 => [x + w, y - 1],
                        _ => [x - 1, y - 1],
                    };
                    let last = g.side as i32 - 1;
                    let at = corner.map(|n| n.clamp(0, last) as u16);
                    for _ in 0..offer.count {
                        let Ok(j) =
                            new_unit(sim, m, lib, offer.class, o.owner, offer.civilization, at)
                        else {
                            break;
                        };
                        let unit = m.objects[j].clone();
                        if let Some(p) = free_near(g, m, lib, &unit, at, Some(j)) {
                            position(&mut m.objects[j], p);
                        }
                        add_levels(sim, m, lib, j, offer.level);
                        if list == 1 {
                            become_hero(m, c, j);
                        }
                    }
                    sim.gameplay.actors[i].hires[list].retain(|h| h.id != id);
                }
                a if a.starts_with("buy:") => {
                    // Command 0x66 of a market item (0x47a254): a corner of
                    // the market drawn R(0, 3), the item made on the free
                    // cell nearest to it, then four times its value paid in
                    // gold (0x47a370), the item taken back when the kingdom
                    // cannot pay; the offer goes.
                    let id: u32 = a[4..].parse().map_err(|_| "Offerta non valida")?;
                    let jewel = sim
                        .gameplay
                        .jewels
                        .get(&o.owner)
                        .and_then(|j| j.offers.iter().find(|x| x.id == id))
                        .cloned()
                        .ok_or("Offerta non più disponibile")?;
                    let [w, h] = footprint(lib, &o).map(|n| n as i32);
                    let (x, y) = (o.x as i32, o.y as i32);
                    let corner = match random(sim, 4) {
                        0 => [x + w, y + h],
                        1 => [x - 1, y + h],
                        2 => [x + w, y - 1],
                        _ => [x - 1, y - 1],
                    };
                    let last = g.side as i32 - 1;
                    let at = corner.map(|n| n.clamp(0, last) as u16);
                    let item = jewel_object(&o, &jewel);
                    let price = 4 * item_value(&item, |k| creature_gold(c, &o, k));
                    debit(c, m, o.owner, [price, 0, 0, 0])?;
                    let j = new_unit(sim, m, lib, jewel.class, o.owner, 0, at)?;
                    m.objects[j].data.truncate(0x12);
                    fill_item(
                        &mut m.objects[j],
                        &jewel.image,
                        jewel.size,
                        &jewel.fields,
                        jewel.spell,
                    );
                    let unit = m.objects[j].clone();
                    let p = free_near(g, m, lib, &unit, at, Some(j)).unwrap_or(at);
                    position(&mut m.objects[j], p);
                    sim.objects[j] = ObjectState::read(&m.objects[j], lib);
                    if let Some(j) = sim.gameplay.jewels.get_mut(&o.owner) {
                        j.offers.retain(|x| x.id != id);
                    }
                }
                a if a.starts_with("sell:") => {
                    // Command 0x75 (0x47a1e1): an item a unit of the market's
                    // kingdom holds is sold for its value in gold (vt+0x2c4,
                    // 0x467680), though the hint shows four times as much
                    // (0x47a4c8); the item goes.
                    let j: usize = a[5..].parse().map_err(|_| "Oggetto non valido")?;
                    if !sellable_items(sim, m, o.owner).contains(&j) {
                        return Err("Oggetto non più disponibile".into());
                    }
                    let value = item_value(&m.objects[j], |k| creature_gold(c, &o, k));
                    credit(m, o.owner, [value, 0, 0, 0]);
                    consume(sim, m, j);
                }
                "build_item" => {
                    // D_build (0x4a5220): category, item and gold, paid up
                    // front; gold × 250 ms (books × 500 ms).
                    let (category, index, gold) = (req.option, req.item, req.amount);
                    let ok = match category {
                        0 => (index as usize) < 21 && gold >= book_minimum(c, &o, index),
                        1 => index < 5 && gold > 0,
                        2 => index < 3 && gold > 0,
                        _ => false,
                    };
                    if !ok {
                        return Err("Oro insufficiente per questo oggetto".into());
                    }
                    // K+0x12a: one magic item per kingdom at a time.
                    if crafting(sim, m, o.owner) {
                        return Err("Un oggetto magico è già in lavorazione".into());
                    }
                    debit(c, m, o.owner, [gold, 0, 0, 0])?;
                    let ms = (gold as u64 * if category == 0 { 500 } else { 250 }).max(1);
                    sim.gameplay.actors[i].jobs.push_back(Job {
                        kind: JobKind::Item(category, index, gold),
                        remaining: ms,
                        total: ms,
                        paid: [gold, 0, 0, 0],
                        due_ms: None,
                        waiting: false,
                    });
                    start_step(sim, i);
                }
                "raisedead" => {
                    cast(sim, m, g, lib, c, i, "spell_raisedead", req)?;
                }
                "cloak" | "uncloak" => {
                    if actual == "cloak" {
                        let (_, cost) = spell_parameters(c, m, &o, "spell_cloak");
                        if sim.objects[i].mana < cost {
                            return Err(c.notmanna.clone());
                        }
                        sim.objects[i].mana -= cost;
                    }
                    // Invisibility lasts 45 s, 2 s from a ninja (effect 9);
                    // uncloak takes it off (op 0x61 at 0x4ebe25).
                    if actual == "cloak" {
                        let ticks = if o.class == 13 { 40 } else { 900 };
                        cloak(sim, m, i, ticks);
                    } else {
                        let a = &mut sim.gameplay.actors[i];
                        a.effect_until = 0;
                        a.cloak_drain = false;
                    }
                }
                "reveal_self" => reveal_self(sim, m, lib, c, i)?,
                "become_priest" | "become_jihad_warrior" => {
                    // Op 0x76 (0x4eb427) and op 0x5d (0x4ac74e): a hypnotised
                    // unit (effect 13) stays as it is; a monk pays the
                    // priest's price of its kingdom first (0x4eb470).
                    if has_effect(sim, i, effect::HYPNOSIS) {
                        return Err("Un'unità ipnotizzata non può convertirsi".into());
                    }
                    let class = if actual == "become_priest" { 18 } else { 33 };
                    if class == 18 {
                        let cost = c.stats(&o, CLASSES[18], researches(m, o.owner)).cost;
                        debit(c, m, o.owner, cost)?;
                    }
                    convert(sim, m, lib, i, class);
                }
                a if a.starts_with("spell ") => {
                    let key = a.trim_start_matches("spell ");
                    let (_, cost) = spell_parameters(c, m, &o, key);
                    if sim.objects[i].mana < cost {
                        return Err(c.notmanna.clone());
                    }
                    let p = req
                        .cell
                        .or_else(|| {
                            req.target
                                .and_then(|j| m.objects.get(j))
                                .map(|x| [x.x, x.y])
                        })
                        .unwrap_or([o.x, o.y]);
                    set_order(
                        sim,
                        i,
                        Intent::Cast {
                            key: key.into(),
                            cell: p,
                            target: req.target,
                        },
                        req.append,
                    )?;
                }
                _ => return Err(format!("Ordine {actual}: comportamento da completare")),
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                accepted += 1;
                // 0x4f31cf: every unit of an order given to several forms a
                // group, except for Dig (program 7, the fields).
                let a = &sim.gameplay.actors[i];
                let field = matches!(a.intent, Some(Intent::Harvest(j))
                    if m.objects.get(j).is_some_and(|t| t.class == class::FIELD));
                if mobile(&m.objects[i]) && a.intent.is_some() && !field {
                    movers.push(i);
                }
            }
            Err(e) => failure = Some(e),
        }
        // Placement and spells use one executor, rather than charging every selected worker;
        // a research runs once, in the first of several selected buildings that
        // takes it (a selection of buildings is this version's).
        if accepted > 0
            && (actual == "build" || actual.starts_with("spell ") || actual == "technology")
        {
            break;
        }
    }
    // vt+0x1a0 of the target (0x4f3231): a carpet (0x4e7a40) or a ship
    // (0x4e85d0) that may move goes to meet the first ordered unit boarding
    // it, a ship also the first peasant sent to repair it.
    if let Some(t) = req.target.filter(|&t| {
        m.objects
            .get(t)
            .is_some_and(|x| matches!(x.class, 22..=25) && alive(x))
            && !selected.contains(&t)
            && combat::free(sim, m, t)
    }) {
        let ship = m.objects[t].class != 22;
        let meet = selected
            .iter()
            .copied()
            .find(|&k| match sim.gameplay.actors[k].intent {
                Some(Intent::Board(j)) => j == t,
                Some(Intent::Repair(j)) => ship && j == t && m.objects[k].class == class::PEON,
                _ => false,
            });
        if let Some(k) = meet {
            let p = [m.objects[k].x, m.objects[k].y];
            let _ = set_order(sim, t, Intent::Move(p), false);
        }
    }
    // 0x4518f0: units ordered together share their paths as a group.
    if movers.len() > 1 && !req.append {
        let id = sim.gameplay.groups.keys().next_back().map_or(1, |k| k + 1);
        for &i in &movers {
            sim.gameplay.actors[i].group = Some(id);
        }
        sim.gameplay.groups.insert(
            id,
            Group {
                members: movers,
                paths: Vec::new(),
            },
        );
    }
    let used: BTreeSet<u32> = sim.gameplay.actors.iter().filter_map(|a| a.group).collect();
    sim.gameplay.groups.retain(|k, _| used.contains(k));
    if accepted == 0 {
        Err(failure.unwrap_or_else(|| "Ordine non disponibile".into()))
    } else {
        Ok(failure.map_or("Ordine impartito".into(), |e| {
            format!("Ordine impartito; {e}")
        }))
    }
}

fn position(o: &mut MapObject, p: [u16; 2]) {
    o.x = p[0];
    o.y = p[1];
    if o.data.len() >= 12 {
        o.data[8..10].copy_from_slice(&p[0].to_le_bytes());
        o.data[10..12].copy_from_slice(&p[1].to_le_bytes());
    }
}
/// The cells an object stands on (+0x1a, +0x1c): a building's and a mine's
/// by their picture, a unit's by its class ([`world::unit_cells`]), a fish
/// patch's 2×2 (its constructor 0x450a40, over a picture of one cell).
fn footprint(lib: &ObjectLibrary, o: &MapObject) -> [usize; 2] {
    if o.building() || matches!(o.class, class::RES_GOLD | class::RES_STONE) {
        lib.cell_footprint(o)
    } else if world::class::is_unit(o.class) {
        world::unit_cells(o.class)
    } else if o.class == class::FISHPATCH {
        [2, 2]
    } else {
        [1, 1]
    }
}
fn distance(lib: &ObjectLibrary, a: &MapObject, b: &MapObject) -> u32 {
    distance_to_area([a.x, a.y], [b.x, b.y], footprint(lib, b))
}
fn distance_to_area(p: [u16; 2], dest: [u16; 2], size: [usize; 2]) -> u32 {
    let x = (p[0] as i32).clamp(dest[0] as i32, dest[0] as i32 + size[0] as i32 - 1);
    let y = (p[1] as i32).clamp(dest[1] as i32, dest[1] as i32 + size[1] as i32 - 1);
    (p[0] as i32 - x)
        .unsigned_abs()
        .max((p[1] as i32 - y).unsigned_abs())
}
fn occupied(
    g: &MapGrid,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    ignore: Option<usize>,
    for_build: bool,
) -> Vec<bool> {
    occupied_by(g, m, lib, m.objects.iter().enumerate(), ignore, for_build)
}
/// `occupied` over the objects given, with their indices.
fn occupied_by<'a>(
    g: &MapGrid,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    objects: impl Iterator<Item = (usize, &'a MapObject)>,
    ignore: Option<usize>,
    for_build: bool,
) -> Vec<bool> {
    let mut out = vec![false; g.side * g.side];
    for (i, o) in objects {
        if Some(i) == ignore
            || !alive(o)
            || o.flags & FLAG_HELD != 0
            || carried(o)
            || world::class::is_marker(o.class)
        {
            continue;
        }
        if m.simulation
            .as_ref()
            .is_some_and(|s| s.objects.get(i).is_some_and(|x| x.holder.is_some()))
        {
            continue;
        }
        if !for_build && (lib.layer(o) == 0 || matches!(o.class, class::BRIDGE | class::FIELD)) {
            continue;
        }
        if !for_build && matches!(o.class, class::GATE_STONE | class::GATE_WOOD) && o.byte(57) == 0
        {
            continue;
        }
        if !o.building() && !mobile(o) && !matches!(o.class, class::RES_GOLD | class::RES_STONE) {
            continue;
        }
        let [w, h] = footprint(lib, o);
        for y in o.y as usize..(o.y as usize + h).min(g.side) {
            for x in o.x as usize..(o.x as usize + w).min(g.side) {
                out[y * g.side + x] = true;
            }
        }
    }
    out
}
fn bridge_cells(g: &MapGrid, m: &MissionWorld, lib: &ObjectLibrary) -> Vec<bool> {
    let mut cells = vec![false; g.side * g.side];
    for o in m
        .objects
        .iter()
        .filter(|o| o.class == class::BRIDGE && alive(o) && o.flags & FLAG_CONSTRUCTION == 0)
    {
        let [w, h] = footprint(lib, o);
        for y in o.y as usize..(o.y as usize + h).min(g.side) {
            for x in o.x as usize..(o.x as usize + w).min(g.side) {
                cells[y * g.side + x] = true;
            }
        }
    }
    cells
}
/// 0x4554d0: a standing tree falls (flag 0x20000000). It no longer blocks,
/// nor can be chopped or drawn, and its cell opens on the map of islands
/// (0x468a60). With no sight of the player over a corner of its cell it
/// turns into a trunk at once; in view, when a sight comes or goes over the
/// cell's top left corner (0x453da0, `settle_stumps`).
fn fell(sim: &mut Simulation, g: &mut MapGrid, lib: &ObjectLibrary, cell: usize) {
    let tree = (g.cells[cell] >> 11) & 0x1f;
    if tree == 0 || tree as u8 <= lib.trunks {
        return;
    }
    g.cells[cell] &= !0xf800;
    sim.gameplay.depleted_trees.insert(cell);
    let p = [(cell % g.side) as i32, (cell / g.side) as i32];
    if sim.gameplay.islands.built() {
        sim.gameplay.islands.opened([p[0], p[1], p[0], p[1]]);
    }
    let lit = [[0, 0], [1, 0], [0, 1], [1, 1]].iter().any(|d| {
        let v = [p[0] + d[0], p[1] + d[1]];
        sim.fog
            .sources
            .iter()
            .any(|&s| crate::simulation::Fog::covers(s, v))
    });
    if !lit {
        stump(sim, g, lib, cell);
    }
}
/// 0x4555bf, 0x453efc: the felled tree becomes the `(3x + 11y) mod n + 1`-th
/// image, one of the `n` trunks of `[treename]`.
fn stump(sim: &mut Simulation, g: &mut MapGrid, lib: &ObjectLibrary, cell: usize) {
    if !sim.gameplay.depleted_trees.remove(&cell) || lib.trunks == 0 {
        return;
    }
    let (x, y) = (cell % g.side, cell / g.side);
    let image = (3 * x + 11 * y) % lib.trunks as usize + 1;
    g.cells[cell] = (g.cells[cell] & !0xf800) | (image << 11) as u16;
    sim.gameplay.stumps.insert(cell);
}
/// 0x453da0: when a sight comes or goes, every vertex it covers is updated,
/// and a felled tree whose cell has that vertex as its top left corner
/// turns into a trunk.
fn settle_stumps(sim: &mut Simulation, g: &mut MapGrid, lib: &ObjectLibrary) {
    let changed = std::mem::take(&mut sim.fog.changed);
    if changed.is_empty() || sim.gameplay.depleted_trees.is_empty() {
        return;
    }
    let side = g.side;
    let due: Vec<usize> = sim
        .gameplay
        .depleted_trees
        .iter()
        .copied()
        .filter(|&c| {
            let v = [(c % side) as i32, (c / side) as i32];
            changed
                .iter()
                .any(|&s| crate::simulation::Fog::covers(s, v))
        })
        .collect();
    for cell in due {
        stump(sim, g, lib, cell);
    }
}
/// 0x44c930, 0x41c010: a field or a bridge put on the map fells the trees
/// under it. Their site tests keep them off standing trees, so only maps
/// and scripts can put one there.
fn fell_under_grounds(
    sim: &mut Simulation,
    m: &MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
) {
    sim.gameplay.grounds.resize(m.objects.len(), false);
    for i in 0..m.objects.len() {
        let o = &m.objects[i];
        let now = matches!(o.class, class::FIELD | class::BRIDGE)
            && o.flags & (FLAG_INACTIVE | FLAG_DYING | FLAG_HELD) == 0;
        if !now || std::mem::replace(&mut sim.gameplay.grounds[i], now) {
            sim.gameplay.grounds[i] = now;
            continue;
        }
        let [w, h] = lib.cell_footprint(o);
        for y in o.y as usize..(o.y as usize + h).min(g.side) {
            for x in o.x as usize..(o.x as usize + w).min(g.side) {
                fell(sim, g, lib, y * g.side + x);
            }
        }
    }
}
/// A choppable tree (0x4ad13e): a tree image above the trunks, not felled.
pub fn tree_at(g: &MapGrid, lib: &ObjectLibrary, p: [u16; 2]) -> bool {
    let (x, y) = (p[0] as usize, p[1] as usize);
    x < g.side && y < g.side && ((g.cells[y * g.side + x] >> 11) & 0x1f) as u8 > lib.trunks
}
/// The tree a click designates (0x504151): the first tree among the cell and
/// the two below it, whose canopies are drawn over it.
pub fn clicked_tree(g: &MapGrid, lib: &ObjectLibrary, p: [u16; 2]) -> Option<[u16; 2]> {
    (0..3)
        .map(|d| [p[0], p[1].saturating_add(d)])
        .find(|&q| tree_at(g, lib, q))
}
fn terrain_free(g: &MapGrid, lib: &ObjectLibrary, o: &MapObject, p: [u16; 2], build: bool) -> bool {
    let [x, y] = p.map(|n| n as usize);
    if x >= g.side || y >= g.side {
        return false;
    }
    if lib.layer(o) == 0 && !build {
        return true;
    }
    let cell = g.cells[y * g.side + x];
    // 0x5072a0 and 0x420310: a cliff, or a tree above the trunk images,
    // stops walkers and buildings; trunks and stumps do not.
    let tree = (cell >> 11) & 0x1f;
    if cell & 0x400 != 0 || (tree > 0 && tree as u8 > lib.trunks) {
        return false;
    }
    let water = (cell >> 4) & 7 == 0 && (cell >> 7) & 7 == 0;
    if o.class == class::BRIDGE && build {
        return water;
    }
    if naval(o) && !build {
        water
    } else {
        (cell >> 4) & 7 != 0 && (cell >> 7) & 7 != 0
    }
}
fn placement(
    g: &MapGrid,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    o: &MapObject,
    ignore: Option<usize>,
    build: bool,
) -> bool {
    let blocked = occupied(g, m, lib, ignore, build);
    let bridges = bridge_cells(g, m, lib);
    let [w, h] = footprint(lib, o);
    if build && o.class == class::HARBOR {
        return harbor_site(g, &blocked, lib, o);
    }
    // 0x420310: no building on the cells beside an open gate's door (bit 30).
    let beside_gate = |p: [u16; 2]| {
        m.objects.iter().any(|x| {
            matches!(x.class, class::GATE_WOOD | class::GATE_STONE)
                && alive(x)
                && x.byte(57) == 0
                && {
                    let r = movement::gate_beside(x);
                    (r[0]..=r[2]).contains(&(p[0] as i32)) && (r[1]..=r[3]).contains(&(p[1] as i32))
                }
        })
    };
    o.x as usize + w <= g.side
        && o.y as usize + h <= g.side
        && (0..h).all(|dy| {
            (0..w).all(|dx| {
                let p = [o.x + dx as u16, o.y + dy as u16];
                (terrain_free(g, lib, o, p, build)
                    || (!build && !naval(o) && bridges[p[1] as usize * g.side + p[0] as usize]))
                    && !blocked[p[1] as usize * g.side + p[0] as usize]
                    && !(build && beside_gate(p))
            })
        })
}
/// The harbour's own site test (0x45b070, its vt+0x28c): no cell of plain
/// land (the kind of its `teren` 1), none taken by a unit or a building;
/// along its border, walked round from the top left corner, the kind of
/// terrain changes once or twice and at least one cell is not water, but
/// not all of them (at most 2·(w + h) − 5, the last side's water not being
/// counted).
fn harbor_site(g: &MapGrid, blocked: &[bool], lib: &ObjectLibrary, o: &MapObject) -> bool {
    let [w, h] = footprint(lib, o).map(|n| n as i32);
    let [x0, y0] = [o.x as i32, o.y as i32];
    let side = g.side as i32;
    if x0 + w > side || y0 + h > side {
        return false;
    }
    let kind = |x: i32, y: i32| pathing::kind(g.cells[(y * side + x) as usize]) >> 17;
    if !(y0..y0 + h).all(|y| (x0..x0 + w).all(|x| harbor_cell(g, blocked, [x, y]))) {
        return false;
    }
    let mut last = kind(x0, y0);
    let (mut changes, mut other) = (0, 0);
    let mut visit = |k: u32| {
        if k != last {
            last = k;
            changes += 1;
        }
        if k != 2 {
            other += 1;
        }
    };
    (x0..x0 + w).for_each(|x| visit(kind(x, y0)));
    (y0 + 1..y0 + h).for_each(|y| visit(kind(x0 + w - 1, y)));
    (x0..=x0 + w - 2)
        .rev()
        .for_each(|x| visit(kind(x, y0 + h - 1)));
    (y0..=y0 + h - 2).rev().for_each(|y| visit(kind(x0, y)));
    (1..=2).contains(&changes) && other >= 1 && other <= 2 * (w + h) - 5
}
/// One cell of `harbor_site`: not plain land, nothing standing on it.
fn harbor_cell(g: &MapGrid, blocked: &[bool], p: [i32; 2]) -> bool {
    let i = p[1] as usize * g.side + p[0] as usize;
    pathing::kind(g.cells[i]) >> 17 != 1 && !blocked[i]
}
/// The cells of `area` (x0, y0, x1, y1, the ends left out), row by row,
/// that a building of `o`'s kind could not cover as far as the player
/// knows: this version's placement grid. In view, the rule of its site
/// test on the cells as they are, units included; in the fog, the same
/// rule on the terrain and the standing objects the player sees or
/// remembers. Unexplored cells, black under the fog, are never marked.
pub fn refused_cells(
    g: &MapGrid,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    o: &MapObject,
    area: [i32; 4],
) -> Vec<bool> {
    let [x0, y0, x1, y1] = area;
    let points = || (y0..y1).flat_map(move |y| (x0..x1).map(move |x| [x, y]));
    let Some(sim) = m.simulation.as_ref() else {
        return points().map(|_| false).collect();
    };
    let truth = movement::cells_now(sim, m, g, lib);
    let fog = |p: [i32; 2]| match truth.inside(p) {
        true => sim.fog.at(p[0] as usize, p[1] as usize),
        false => (false, false),
    };
    let fogged = points().any(|p| fog(p) == (true, false));
    let known = match fogged {
        true => movement::known_standing(sim, m),
        false => Vec::new(),
    };
    let known_cells = match fogged {
        true => movement::cells_known(g, lib, &known),
        false => Default::default(),
    };
    let gate = matches!(o.class, class::GATE_WOOD | class::GATE_STONE);
    let harbor = o.class == class::HARBOR;
    let [covered, covered_known] = if gate {
        [
            gate_cover(g.side, lib, m.objects.iter()),
            gate_cover(g.side, lib, known.iter().map(|&(_, x)| x)),
        ]
    } else {
        Default::default()
    };
    let [blocked, blocked_known] = if harbor {
        [
            occupied(g, m, lib, None, true),
            occupied_by(g, m, lib, known.iter().copied(), None, true),
        ]
    } else {
        Default::default()
    };
    let units: BTreeSet<[i32; 2]> = m
        .objects
        .iter()
        .enumerate()
        .filter(|&(k, x)| o.class == class::FIELD && land_unit(sim, lib, k, x))
        .map(|(_, x)| [x.x as i32, x.y as i32])
        .collect();
    let seen_unit = |p: [i32; 2]| units.contains(&p);
    let no_unit = |_: [i32; 2]| false;
    points()
        .map(|p| {
            let (explored, view) = fog(p);
            let (cells, cover, blocked, unit): (_, _, _, &dyn Fn([i32; 2]) -> bool) = if view {
                (&truth, &covered, &blocked, &seen_unit)
            } else {
                (&known_cells, &covered_known, &blocked_known, &no_unit)
            };
            explored
                && !if gate {
                    gate_cell(cells, lib, cover, p)
                } else if harbor {
                    harbor_cell(g, blocked, p)
                } else {
                    site_cell(cells, lib, o, p, unit)
                }
        })
        .collect()
}
pub fn can_place(g: &MapGrid, m: &MissionWorld, lib: &ObjectLibrary, o: &MapObject) -> bool {
    match m.simulation.as_ref() {
        Some(s) => site_test(s, m, g, lib, o) && (o.owner != m.player() || explored(s, lib, o)),
        None => placement(g, m, lib, o, None, true),
    }
}
/// The site test of a building (vt+0x28c) on the map's cells as they are
/// (0x5ccaf4). Most buildings (0x420310) want every cell inside the map, of
/// their terrain kind (`teren`), with no field, gate door, land occupant or
/// bridge (`0xc1200000`) and no standing tree. The castle asks the units'
/// test (0x422b60 → 0x505b20: vt+0x90 = 0x500bb0 on each cell); the bridge
/// no terrain kind (0x41c4e0); the field lets land units stand on its cells
/// (0x44ca70); the harbor and the gates have their own (0x45b070,
/// 0x456b50).
fn site_test(
    sim: &Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    o: &MapObject,
) -> bool {
    if o.class == class::HARBOR {
        return placement(g, m, lib, o, None, true);
    }
    let cells = movement::cells_now(sim, m, g, lib);
    if matches!(o.class, class::GATE_WOOD | class::GATE_STONE) {
        return gate_site(&cells, m, lib, o);
    }
    let [w, h] = footprint(lib, o).map(|n| n as i32);
    let [x0, y0] = [o.x as i32, o.y as i32];
    let land_unit = |p: [i32; 2]| {
        m.objects
            .iter()
            .enumerate()
            .any(|(k, x)| land_unit(sim, lib, k, x) && [x.x as i32, x.y as i32] == p)
    };
    (y0..y0 + h).all(|y| (x0..x0 + w).all(|x| site_cell(&cells, lib, o, [x, y], &land_unit)))
}
/// A land unit out of any holder, which a field lets stay on its cells.
fn land_unit(sim: &Simulation, lib: &ObjectLibrary, k: usize, x: &MapObject) -> bool {
    mobile(x)
        && alive(x)
        && lib.layer(x) == 1
        && sim.objects.get(k).is_some_and(|s| s.holder.is_none())
}
/// One cell of `site_test`; `land_unit` tells where a land unit stands,
/// which the field lets stay.
fn site_cell(
    cells: &pathing::Cells,
    lib: &ObjectLibrary,
    o: &MapObject,
    p: [i32; 2],
    land_unit: &dyn Fn([i32; 2]) -> bool,
) -> bool {
    use pathing::{BRIDGE, FIELD, GATEWAY, OCCUPIED};
    if !cells.inside(p) {
        return false;
    }
    let teren = lib.layer(o) as u32;
    let mask = FIELD | GATEWAY | OCCUPIED[1] | BRIDGE;
    let word = cells.at(p);
    let tree = (word >> 11) & 31 > lib.trunks as u32;
    match o.class {
        class::CASTLE => cells.free(1, p),
        class::BRIDGE => word & mask == 0 && !tree,
        class::FIELD => {
            (word >> 17) & 3 == teren
                && word & (FIELD | GATEWAY | BRIDGE) == 0
                && (cells.free(1, p) || land_unit(p))
        }
        _ => word & mask == 0 && (word >> 17) & 3 == teren && !tree,
    }
}
/// The gate's site test (0x456b50): over its footprint and its passage
/// (0x457980), every cell inside the map, no standing tree beyond the first
/// two `[treename]` counts, and no unit, building or item on it but walls
/// (flag 0x1000), which the gate replaces (0x455e20). It asks no terrain
/// kind and no explored cell.
fn gate_site(cells: &pathing::Cells, m: &MissionWorld, lib: &ObjectLibrary, o: &MapObject) -> bool {
    let [w, h] = footprint(lib, o).map(|n| n as i32);
    let foot = [
        o.x as i32,
        o.y as i32,
        o.x as i32 + w - 1,
        o.y as i32 + h - 1,
    ];
    let pass = gate_passage(o);
    let within =
        |r: [i32; 4], p: [i32; 2]| (r[0]..=r[2]).contains(&p[0]) && (r[1]..=r[3]).contains(&p[1]);
    let cover = gate_cover(cells.side, lib, m.objects.iter());
    for y in foot[1].min(pass[1])..=foot[3].max(pass[3]) {
        for x in foot[0].min(pass[0])..=foot[2].max(pass[2]) {
            let p = [x, y];
            if (within(pass, p) || within(foot, p)) && !gate_cell(cells, lib, &cover, p) {
                return false;
            }
        }
    }
    true
}
/// One cell of `gate_site`, with the cells of `gate_cover`.
fn gate_cell(cells: &pathing::Cells, lib: &ObjectLibrary, cover: &[bool], p: [i32; 2]) -> bool {
    let limit = lib.trunks as u32 + lib.shrubs as u32;
    cells.inside(p) && (cells.at(p) >> 11) & 31 <= limit && !cover[cells.index(p)]
}
/// The cells where something stops a gate (0x456b50): a unit, a building
/// or an item, but not a wall.
fn gate_cover<'a>(
    side: usize,
    lib: &ObjectLibrary,
    objects: impl Iterator<Item = &'a MapObject>,
) -> Vec<bool> {
    let mut out = vec![false; side * side];
    for t in objects.filter(|t| {
        alive(t)
            && t.flags & FLAG_HELD == 0
            && !matches!(t.class, class::WALL_STONE | class::WALL_WOOD)
            && (t.building() || mobile(t) || carried(t))
    }) {
        let r = movement::footprint_rect(lib, t);
        for y in r[1].max(0)..=r[3].min(side as i32 - 1) {
            for x in r[0].max(0)..=r[2].min(side as i32 - 1) {
                out[y as usize * side + x as usize] = true;
            }
        }
    }
    out
}
/// The explored vertex a building asks for (0x4203c5); not a gate.
fn explored(sim: &Simulation, lib: &ObjectLibrary, o: &MapObject) -> bool {
    matches!(o.class, class::GATE_WOOD | class::GATE_STONE) || explored_site(&sim.fog, lib, o)
}
/// 0x4203c5: a building needs at least one explored vertex of its footprint
/// in the player's memory of the map (0x5b7828).
fn explored_site(fog: &crate::simulation::Fog, lib: &ObjectLibrary, o: &MapObject) -> bool {
    let [w, h] = footprint(lib, o);
    (0..=h).any(|dy| (0..=w).any(|dx| fog.vertex(o.x as usize + dx, o.y as usize + dy).0))
}
/// A wooden wall row (0x4fa710 over the neighbours of 0x4fa480): `i` and
/// every finished wooden wall or gate reached through the cells left, right,
/// above and below each segment, of the same kingdom.
fn wall_line(m: &MissionWorld, lib: &ObjectLibrary, i: usize) -> Vec<usize> {
    let wooden = |x: &MapObject| {
        alive(x)
            && matches!(x.class, class::WALL_WOOD | class::GATE_WOOD)
            && x.flags & FLAG_CONSTRUCTION == 0
    };
    let owner = m.objects[i].owner;
    let at = |p: [i32; 2]| {
        m.objects.iter().position(|x| {
            let [w, h] = footprint(lib, x).map(|n| n as i32);
            wooden(x)
                && x.owner == owner
                && (x.x as i32..x.x as i32 + w).contains(&p[0])
                && (x.y as i32..x.y as i32 + h).contains(&p[1])
        })
    };
    let mut line = vec![i];
    let mut k = 0;
    while let Some(&j) = line.get(k) {
        k += 1;
        let [x, y] = [m.objects[j].x as i32, m.objects[j].y as i32];
        for p in [[x, y - 1], [x + 1, y], [x, y + 1], [x - 1, y]] {
            if let Some(n) = at(p)
                && !line.contains(&n)
            {
                line.push(n);
            }
        }
    }
    line
}
/// The object whose queue a building shows and takes: a wall segment's row
/// leader (0x4fa640), otherwise itself.
pub fn producer(sim: &Simulation, i: usize) -> usize {
    sim.gameplay
        .wall_rows
        .iter()
        .find(|(_, row)| row.contains(&i))
        .map_or(i, |(&leader, _)| leader)
}
/// The cells of a wall dragged from `start` to `end` (0x4fb8f0): the start,
/// the longer side first (the other one with Alt, unless the line is
/// straight), the corner, the other side and the end. When placed, the row
/// begins at its end nearer the builder (0x4fb19d).
pub fn wall_row(
    start: [u16; 2],
    end: [u16; 2],
    alt: bool,
    builder: Option<[u16; 2]>,
) -> Vec<[u16; 2]> {
    let [x0, y0] = start.map(|v| v as i32);
    let [x1, y1] = end.map(|v| v as i32);
    let between = |a: i32, b: i32| -> Vec<i32> {
        if a < b {
            (a + 1..b).collect()
        } else {
            (b + 1..a).rev().collect()
        }
    };
    let mut vertical = (x0 - x1).abs() < (y0 - y1).abs();
    if alt && x0 != x1 && y0 != y1 {
        vertical = !vertical;
    }
    let mut row = vec![[x0, y0]];
    if vertical {
        row.extend(between(y0, y1).into_iter().map(|y| [x0, y]));
        if x0 != x1 {
            row.push([x0, y1]);
        }
        row.extend(between(x0, x1).into_iter().map(|x| [x, y1]));
    } else {
        row.extend(between(x0, x1).into_iter().map(|x| [x, y0]));
        if y0 != y1 {
            row.push([x1, y0]);
        }
        row.extend(between(y0, y1).into_iter().map(|y| [x1, y]));
    }
    if [x1, y1] != [x0, y0] {
        row.push([x1, y1]);
    }
    let mut row: Vec<[u16; 2]> = row
        .into_iter()
        .map(|p| [p[0] as u16, p[1] as u16])
        .collect();
    if let Some(b) = builder
        && let (Some(&first), Some(&last)) = (row.first(), row.last())
    {
        let d = |p: [u16; 2]| {
            (p[0] as i32 - b[0] as i32)
                .abs()
                .max((p[1] as i32 - b[1] as i32).abs())
        };
        if d(first) > d(last) {
            row.reverse();
        }
    }
    row
}
/// A wall segment, placed by dragging a row (0x4fb0be).
pub fn wall_order(c: &Catalog, order: &str) -> bool {
    matches!(c.class_name(order).as_str(), "wall_stone" | "wall_wood")
}
pub fn building_at(
    lib: &ObjectLibrary,
    c: &Catalog,
    worker: &MapObject,
    order: &str,
    pointer: [u16; 2],
) -> Option<MapObject> {
    let mut probe = worker.clone();
    probe.class = CLASSES.iter().position(|&s| s == c.class_name(order))?;
    probe.custom_image.clear();
    probe.variant = 0;
    probe.flags = 0;
    probe.data = vec![0; 77];
    let base = lib.attributes.get(&probe);
    probe.health = base.health;
    probe.max_health = base.health;
    let size = footprint(lib, &probe);
    probe.x = pointer[0].checked_sub((size[0] / 2) as u16)?;
    probe.y = pointer[1].checked_sub((size[1] / 2) as u16)?;
    Some(probe)
}
fn free_near(
    g: &MapGrid,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    o: &MapObject,
    p: [u16; 2],
    ignore: Option<usize>,
) -> Option<[u16; 2]> {
    free_near_reserved(g, m, lib, o, p, ignore, &BTreeSet::new())
}
fn free_near_reserved(
    g: &MapGrid,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    o: &MapObject,
    p: [u16; 2],
    ignore: Option<usize>,
    reserved: &BTreeSet<[u16; 2]>,
) -> Option<[u16; 2]> {
    let blocked = occupied(g, m, lib, ignore, false);
    let bridges = bridge_cells(g, m, lib);
    for r in 0..g.side as i32 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue;
                }
                let x = p[0] as i32 + dx;
                let y = p[1] as i32 + dy;
                if x < 0 || y < 0 || x >= g.side as i32 || y >= g.side as i32 {
                    continue;
                }
                let p = [x as u16, y as u16];
                if !reserved.contains(&p)
                    && (terrain_free(g, lib, o, p, false)
                        || (!naval(o) && bridges[y as usize * g.side + x as usize]))
                    && (!blocked[y as usize * g.side + x as usize] || lib.layer(o) == 0)
                {
                    return Some(p);
                }
            }
        }
    }
    None
}
/// Op 0x41, Find unexplored tile (0x4e299e → 0x415860): the engine's flood
/// over the eight neighbours from the unit's cell, through the cells its
/// layer may enter now (0x415940 → 0x4a9820, 0x4a9ac0, 0x4a9b40). A cell
/// none of whose four vertices the kingdom explored is a candidate; the
/// nearest to the program's point (Chebyshev, 0x454dc0) wins, the first on
/// a tie. The flood ends at depth 32 or, with a candidate known, at the
/// first cell of a depth that is a multiple of 3. The explored vertices are
/// the player's (0x5b7828 keeps a bit for each kingdom).
fn unexplored(
    sim: &Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    point: [u16; 2],
) -> Option<[u16; 2]> {
    let o = &m.objects[i];
    let layer = lib.layer(o);
    let cells = movement::cells_now(sim, m, g, lib);
    let side = g.side as i32;
    let mut flood =
        ai::build::Flood::new([o.x as i32, o.y as i32], [0, 0, side - 1, side - 1], g.side);
    let mut best: Option<([i32; 2], i32)> = None;
    flood.run_levels(u32::MAX, |p, level, stop| {
        if !cells.enterable(layer, p) {
            return false;
        }
        *stop = level >= 32 || (best.is_some() && level % 3 == 0);
        let explored = [[0, 0], [1, 0], [0, 1], [1, 1]].iter().any(|d| {
            sim.fog
                .vertex((p[0] + d[0]) as usize, (p[1] + d[1]) as usize)
                .0
        });
        if !explored {
            let d = (p[0] - point[0] as i32)
                .abs()
                .max((p[1] - point[1] as i32).abs());
            if best.is_none_or(|(_, b)| d < b) {
                best = Some((p, d));
            }
        }
        true
    });
    best.map(|(p, _)| [p[0] as u16, p[1] as u16])
}
/// The Explore program after its Move (0x42f3a0): op 0x41 gives the next
/// cell to walk to, or the program ends.
fn explore_on(
    sim: &mut Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    point: [u16; 2],
) {
    match unexplored(sim, m, g, lib, i, point) {
        Some(p) => {
            let a = &mut sim.gameplay.actors[i];
            a.explore_target = Some(p);
            halt(a);
        }
        None => finish(sim, i),
    }
}
/// Walk onto a cell, or within `range` of it (a Move to the point's rect).
fn approach(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    dest: [u16; 2],
    range: u32,
) -> bool {
    let rect = [
        dest[0] as i32,
        dest[1] as i32,
        dest[0] as i32,
        dest[1] as i32,
    ];
    movement::walk(sim, m, g, lib, i, rect, range, None) == Some(true)
}
/// Walk next to a cell: into the 3×3 cells around it.
fn approach_near(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    dest: [u16; 2],
) -> bool {
    let [x, y] = [dest[0] as i32, dest[1] as i32];
    movement::walk(sim, m, g, lib, i, [x - 1, y - 1, x + 1, y + 1], 0, None) == Some(true)
}
/// Walk to an object, following it: next to it (its rect, vt+0x134) with
/// range 0, or within `range` of that rect.
fn approach_object(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    target: usize,
    range: u32,
) -> bool {
    let rect = movement::target_rect(sim, m, lib, target);
    movement::walk(sim, m, g, lib, i, rect, range, Some(target)) == Some(true)
}
/// Walk onto an object's own cells (a field, an item on the ground).
fn approach_inside(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    target: usize,
) -> bool {
    let rect = movement::footprint_rect(lib, &m.objects[target]);
    movement::walk(sim, m, g, lib, i, rect, 0, None) == Some(true)
}
/// The last walk failed: the destination cannot be reached for now.
fn unreachable(sim: &Simulation, i: usize) -> bool {
    movement::failed(sim, i)
}
/// Leave the walk: the step under way still ends on its cell.
fn halt(a: &mut Actor) {
    a.path.clear();
    a.goal = None;
    a.movement = None;
}
/// vt+0x17c (0x4e50e0): the program under way, ending or giving way to
/// another (0x503059, 0x503362), leaves its priority at +0xe0: a flight
/// 1000, an engagement 3000, an order 4000, the default Idle 1000
/// (0x42df09).
fn give_way(a: &mut Actor) {
    a.last_priority = match a.intent {
        None if a.ordered_idle => 4000,
        Some(Intent::Flee(..)) | None => 1000,
        Some(_) if a.automatic => 3000,
        Some(_) => 4000,
    };
    a.ordered_idle = false;
}
fn finish(sim: &mut Simulation, i: usize) {
    let a = &mut sim.gameplay.actors[i];
    if a.intent.is_some() {
        give_way(a);
    }
    a.intent = if a.automatic {
        // An engagement or a flight of the unit itself: back to what it
        // kept (0x4e4c14); a kept attack is itself an engagement.
        let kept = a.resume.take();
        a.automatic = matches!(kept, Some(Intent::Attack(_)));
        kept
    } else {
        // 0x4e4c14: the default program takes up the program kept (a
        // computer player's order waiting for its transport).
        a.queued.pop_front().or_else(|| a.resume.take())
    };
    if a.intent.is_none() {
        a.move_mode = 0;
    }
    a.action_image.clear();
    halt(a);
    a.depositing = None;
    a.search_at = None;
    a.explore_target = None;
    a.move_target = None;
    sim.objects[i].order = OrderState::Idle;
}
/// The scenario's objects as the loader starts them: every unit of owner 9
/// begins the Rescue program (0x44092d); in the records' order each
/// training dummy draws its picture R(0, 3) from the game stream, 0 with
/// the record's bit 4 (0x448090), and each gold or stone mine its look
/// R(0, 2) (0x4afeae), both kept as the variant. The units get their
/// program state here, at time 0, as the first step would give it.
pub fn hold_prisoners(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary) {
    sync(sim, m, lib);
    for i in 0..m.objects.len() {
        let o = &m.objects[i];
        if o.flags & FLAG_INACTIVE != 0 {
            continue;
        }
        if o.owner == 9 && crate::selection::unit(o) {
            if let Some(a) = sim.gameplay.actors.get_mut(i) {
                a.intent = Some(Intent::Rescue(0));
            }
        } else if o.class == class::DUMMY {
            m.objects[i].variant = if o.flags & 4 != 0 {
                0
            } else {
                sim.gameplay.game_rng.range(0, 3) as u16
            };
        } else if matches!(o.class, class::RES_GOLD | class::RES_STONE) {
            m.objects[i].variant = sim.gameplay.game_rng.range(0, 2) as u16;
        }
    }
}
/// A kingdom 1–8 that exists and may rescue prisoners and take ruins and
/// magic gateways (K+0x122).
fn rescuing_kingdom(sim: &Simulation, m: &MissionWorld, owner: u8) -> bool {
    (1..=8).contains(&owner)
        && m.kingdoms.get(owner as usize - 1).is_some_and(|k| k.active)
        && !sim.gameplay.no_rescue.contains(&owner)
}
/// Native 0x5050a0 transfers ownership recursively, preserving civilization.
fn transfer_owner(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    owner: u8,
) {
    // 0x4e50cb, 0x41fb76: a unit or a building changing hands is followed by
    // the end test.
    sim.gameplay.script.end_due = true;
    let mut pending = vec![i];
    let mut visited = BTreeSet::new();
    while let Some(j) = pending.pop() {
        if !visited.insert(j) || j >= m.objects.len() {
            continue;
        }
        // 0x5050a0 → 0x501600(reset 0): the class record changes, the
        // object's own words stay; only the kingdom's getters follow the
        // new owner.
        if sim.objects[j].capacity.is_none() {
            sim.objects[j].capacity = Some(holder_room(m, lib, &m.objects[j]));
        }
        if sim.objects[j].faith.is_none() {
            sim.objects[j].faith = Some(faith_of(m, &m.objects[j]));
        }
        let before = ObjectState::live_bonus(&m.objects[j], researches(m, m.objects[j].owner));
        m.objects[j].owner = owner;
        m.objects[j].data[12] = owner;
        // 0x5050a0: it joins the head of its new kingdom's list.
        sim.gameplay.joins += 1;
        sim.gameplay.actors[j].joined = sim.gameplay.joins;
        let after = ObjectState::live_bonus(&m.objects[j], researches(m, owner));
        rebase(sim, j, before, after);
        // Current resistance is preserved; the building clock restores it.
        let a = &mut sim.gameplay.actors[j];
        a.intent = None;
        a.queued.clear();
        halt(a);
        a.jobs.clear();
        a.production_blocked = None;
        sim.objects[j].order = OrderState::Idle;
        if owner != 0 {
            pending.extend(sim.objects[j].contents.iter().copied());
        }
    }
}
/// Native Die program (0x4e2110): effects end, the inventory falls, the cells
/// are freed and the owner becomes 0; `image_die` then plays to its end and
/// the corpse is removed. Objects without a death picture vanish at once.
/// 0x4200c0: a finished building burns by its health: above half not at
/// all, down to 36% with one small fire, down to 16% with two, below with
/// two and a big one. A new fire stands at a random pixel 31 inside the
/// footprint, drawn from the stream of sounds and pictures (0x41ff90); the
/// `fire` sound plays every 500 ms while it burns. Ships (vt+0xc4 =
/// 0x4e89e0) burn by the same steps after a blow or a repair.
fn burn(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, j: usize) {
    let o = &m.objects[j];
    let ship = naval(o);
    if !(o.building() || ship) || o.flags & FLAG_CONSTRUCTION != 0 || !alive(o) {
        return;
    }
    let pct = (o.health as i32 * 100 / o.max_health.max(1) as i32) as u16;
    let level = match pct {
        51.. => 0,
        36.. => 1,
        16.. => 2,
        _ => 3,
    };
    if sim.gameplay.actors[j].fire_level == level {
        return;
    }
    let want = [level >= 1, level >= 2, level >= 3];
    let [w, h] = footprint(lib, o).map(|n| n as i32 * 32 + 1);
    let origin = [o.x as i32 * 32, o.y as i32 * 32];
    let margin = |side: i32| if side <= 1 { 15 } else { 31 };
    for slot in 0..3 {
        if !want[slot] {
            sim.gameplay.actors[j].fires[slot] = None;
        } else if ship {
            // 0x4e89e0, 0x4e8b60: a ship's fires burn on the centre of its
            // picture and follow it (0x4e8c10); the frontend places them.
            sim.gameplay.actors[j].fires[slot].get_or_insert([0, 0]);
        } else if sim.gameplay.actors[j].fires[slot].is_none() {
            let x = origin[0] + sim.gameplay.aux_rng.range(margin(w), w - margin(w));
            let y = origin[1] + sim.gameplay.aux_rng.range(margin(h), h - margin(h));
            sim.gameplay.actors[j].fires[slot] = Some([x, y]);
        }
    }
    let now = sim.ticks * TICK_MS;
    let a = &mut sim.gameplay.actors[j];
    a.fire_level = level;
    // The `fire` sound is the building's timer (0x4200c0); a ship's fires
    // are silent animations.
    a.fire_due_ms = if level == 0 || ship {
        None
    } else {
        a.fire_due_ms.or(Some(now + 1))
    };
}
/// An item went into a holder (0x462990, the items' vt+0x1b4; a resource
/// item has its own, 0x464f30): last in the sell list (0x4629e3), and the
/// active one when its holder has none or has equipment marked (0x5054c0,
/// class 0x51).
fn picked(sim: &mut Simulation, m: &MissionWorld, j: usize) {
    let holder = sim.objects[j].holder;
    // 0x4629aa → 0x413f10, 0x463d90: a computer's unit drinks a potion at
    // once, which goes in neither the sale list nor the mark, and judges a
    // book (`ai_take_up`).
    if let Some(h) = holder
        && ai_managed(sim, m, h)
        && matches!(m.objects[j].class, class::ITEM_HEALTH | class::ITEM_MAGIC)
    {
        if sim.gameplay.ai_taken.len() < 256 {
            sim.gameplay.ai_taken.push(j);
        }
        if m.objects[j].class == class::ITEM_HEALTH {
            return;
        }
    }
    sim.gameplay.picks.retain(|&k| k != j);
    if sim.gameplay.picks.len() < 4096 {
        sim.gameplay.picks.push(j);
    }
    let Some(h) = holder else {
        return;
    };
    if active_item(sim, m, h).is_none_or(|k| m.objects[k].class == class::ITEM_PLUSPLUS) {
        mark_item(sim, m, j);
    }
}
/// 0x18000000 in the engine's hooks of the computer players: an object of
/// a kingdom that has one, not of the story (record flag 0x2000 →
/// 0x8000000). The marks 0x10000000 the computer puts on its own units are
/// not kept.
pub(crate) fn ai_managed(sim: &Simulation, m: &MissionWorld, i: usize) -> bool {
    m.objects
        .get(i)
        .is_some_and(|o| o.flags & FLAG_STORY == 0 && ai::controls(sim, o.owner))
}
/// The summoning books (carpet, bat, golem, dragon, genie: spells 3, 4,
/// 10, 16, 19), which a computer's unit uses as soon as they are full.
fn summoning(spell: u8) -> bool {
    matches!(spell, 3 | 4 | 10 | 16 | 19)
}
/// A book's charges (+0xd4); an endless book never comes to its last.
fn book_charges(o: &MapObject) -> u16 {
    if o.byte(99) != 0 {
        o.word(102)
    } else {
        u16::MAX
    }
}
/// What a computer's unit does with what it has just taken up: a potion
/// it drinks (0x413f10); a book at its last charge it learns, when it has
/// mana and does not know the spell; a summoning book with more it uses at
/// once on itself (0x463d90), and learns after when that leaves one.
fn ai_take_up(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
) {
    for j in std::mem::take(&mut sim.gameplay.ai_taken) {
        let Some(h) = sim.objects.get(j).and_then(|s| s.holder) else {
            continue;
        };
        if !alive(&m.objects[j]) || !ai_managed(sim, m, h) {
            continue;
        }
        match m.objects[j].class {
            class::ITEM_HEALTH => drink(sim, m, lib, c, h, j),
            class::ITEM_MAGIC => {
                let o = &m.objects[j];
                let charges = book_charges(o);
                let teachable = sim.objects[h].max_mana > 0
                    && o.spell_key()
                        .is_some_and(|key| !knows_spell(sim, m, c, lib, h, key));
                if charges == 1 && teachable {
                    let _ = read_book(sim, m, g, lib, c, h, j, None, None);
                } else if charges > 1 && summoning(o.byte(96)) {
                    let _ = read_book(sim, m, g, lib, c, h, j, Some(h), None);
                }
            }
            _ => {}
        }
    }
}
/// The first object of a holder's with the item flag 0x400 and the mark
/// (0x5054c0).
fn active_item(sim: &Simulation, m: &MissionWorld, h: usize) -> Option<usize> {
    sim.objects[h]
        .contents
        .iter()
        .copied()
        .find(|&k| carried(&m.objects[k]) && sim.objects[k].active)
}
/// 0x463140 with 1 (vt+0x2d0 of the items): the item becomes the active
/// one, and the holder's other objects with the item flag lose the mark.
fn mark_item(sim: &mut Simulation, m: &MissionWorld, j: usize) {
    sim.objects[j].active = true;
    let Some(h) = sim.objects[j].holder else {
        return;
    };
    for k in sim.objects[h].contents.clone() {
        if k != j && carried(&m.objects[k]) {
            sim.objects[k].active = false;
        }
    }
}
/// The item Backspace uses (0x4f625c): the first marked, else the last
/// book, else the first potion of holder `h`.
pub fn quick_item(sim: &Simulation, m: &MissionWorld, h: usize) -> Option<usize> {
    let mut pick = None;
    for &k in &sim.objects.get(h)?.contents {
        let o = &m.objects[k];
        if !carried(o) {
            continue;
        }
        if o.class == class::ITEM_MAGIC {
            pick = Some(k);
        }
        if pick.is_none() && o.class == class::ITEM_HEALTH {
            pick = Some(k);
        }
        if sim.objects[k].active {
            return Some(k);
        }
    }
    pick
}
/// An object with the item flag leaves holder `h` (0x462c20 → 0x463140
/// with 0): it loses the mark, and the holder's first content takes it when
/// it has the item flag, even with another one marked: two then show as
/// active and Backspace takes the first.
fn item_left(sim: &mut Simulation, m: &MissionWorld, h: usize, j: usize) {
    sim.objects[j].active = false;
    if let Some(&first) = sim.objects[h].contents.first()
        && carried(&m.objects[first])
    {
        sim.objects[first].active = true;
    }
}
/// 0x504440: `held` goes into `holder`. A castle or an outpost starts
/// healing a wounded newcomer (0x422b00).
fn garrison(sim: &mut Simulation, m: &mut MissionWorld, holder: usize, held: usize) {
    sim.objects[holder].contents.push(held);
    sim.objects[held].holder = Some(holder);
    // vt+0x1b4 once in the list (0x5044df).
    if m.objects[held].item() {
        picked(sim, m, held);
    }
    sim.objects[held].entry = Some([m.objects[held].x, m.objects[held].y]);
    m.objects[held].flags |= FLAG_HELD;
    // 0x504523 → 0x4532f0: what goes in leaves the selection.
    if sim.gameplay.script.outbox.len() < 1024 {
        sim.gameplay.script.outbox.push(Show::Unselect(held));
    }
    let h = &m.objects[held];
    if matches!(m.objects[holder].class, class::CASTLE | 52) && h.health < h.max_health {
        let now = sim.ticks * TICK_MS;
        sim.gameplay.actors[holder]
            .heal_due_ms
            .get_or_insert(now + 1000);
    }
}
/// Message 0x4211 (0x422a00): every second a castle or outpost gives 2 to
/// each occupant not of stone, while any is still wounded.
fn heal_garrison(sim: &mut Simulation, m: &mut MissionWorld, i: usize) {
    let Some(due) = sim.gameplay.actors[i].heal_due_ms else {
        return;
    };
    let now = sim.ticks * TICK_MS;
    if now < due {
        return;
    }
    let contents = sim.objects[i].contents.clone();
    let mut wounded = false;
    for k in contents {
        let x = &mut m.objects[k];
        if crate::selection::unit(x) && !has_effect(sim, k, effect::STONE) {
            x.health = x.health.saturating_add(2).min(x.max_health);
        }
        wounded |= x.health < x.max_health;
    }
    sim.gameplay.actors[i].heal_due_ms = wounded.then_some(due + 1000);
}
/// 0x4ff350: an object leaves the map with no death (no event, experience
/// or score).
fn vanish(sim: &mut Simulation, m: &mut MissionWorld, j: usize) {
    for k in std::mem::take(&mut sim.objects[j].contents) {
        sim.objects[k].holder = None;
        m.objects[k].flags &= !FLAG_HELD;
        vanish(sim, m, k);
    }
    m.objects[j].flags = (m.objects[j].flags & !(FLAG_DYING | FLAG_HELD)) | FLAG_INACTIVE;
    m.objects[j].health = 0;
    sim.objects[j].holder = None;
    sim.objects[j].order = OrderState::Idle;
    let a = &mut sim.gameplay.actors[j];
    a.intent = None;
    a.queued.clear();
    a.jobs.clear();
    halt(a);
}
/// An unfinished building leaving the map gives back its cost (0x41d6a8):
/// all of it before any work, otherwise `cost × f / 1000` of each resource
/// with `f = (1000·TIME − 1000·progress) / (2·TIME)` truncated.
fn refund_site(sim: &Simulation, m: &mut MissionWorld, c: &Catalog, i: usize) {
    let o = &m.objects[i];
    let Some(a) = sim.gameplay.actors.get(i) else {
        return;
    };
    if o.flags & (FLAG_INACTIVE | FLAG_DYING) != 0
        || o.flags & FLAG_CONSTRUCTION == 0
        || !o.building()
        || a.construction_total == 0
    {
        return;
    }
    let (t, p) = (
        a.construction_total,
        a.construction_total.saturating_sub(a.construction),
    );
    let cost = c.stats(o, o.class_name(), researches(m, o.owner)).cost;
    let owner = o.owner;
    let back = if p == 0 {
        cost
    } else {
        let f = ((1000 * t - 1000 * p) / (2 * t)) as u16 as u32;
        cost.map(|n| n * f / 1000)
    };
    credit(m, owner, back);
}
fn kill(sim: &mut Simulation, m: &mut MissionWorld, g: &MapGrid, lib: &ObjectLibrary, i: usize) {
    if m.objects[i].flags & (FLAG_INACTIVE | FLAG_DYING) != 0 {
        return;
    }
    // 0x4e9421, the ships' Die: with no land within 5 cells of the ship its
    // passengers go down with it, with no death (0x4ff350); a crusader's
    // anchor is freed with it.
    if matches!(m.objects[i].class, 23..=25 | class::FISHING_BOAT)
        && sim.objects[i]
            .contents
            .iter()
            .any(|&j| !carried(&m.objects[j]))
        && !movement::land_near(sim, m, g, lib, i)
    {
        for j in sim.objects[i].contents.clone() {
            if !carried(&m.objects[j]) {
                sim.objects[i].contents.retain(|&k| k != j);
                sim.objects[j].holder = None;
                m.objects[j].flags &= !FLAG_HELD;
                vanish(sim, m, j);
            }
        }
    }
    // The fires go with the building.
    let a = &mut sim.gameplay.actors[i];
    (a.fire_level, a.fires, a.fire_due_ms) = (0, [None; 3], None);
    let dead = &m.objects[i];
    sim.gameplay.script.died(dead.owner, dead.class);
    // The peasant's Die (0x4acc44) first drops his load as a resource item
    // (0x4aea70).
    if m.objects[i].class == class::PEON
        && sim
            .gameplay
            .actors
            .get(i)
            .is_some_and(|a| a.cargo.iter().any(|&n| n > 0))
    {
        drop_cargo(sim, m, g, lib, i);
    }
    // 0x4e2163: the dying unit falls silent, then its death is heard where
    // it fell (0x4e218a; buildings 0x41d626).
    let category = if m.objects[i].building() {
        crate::audio::category::COLLAPSE
    } else {
        crate::audio::category::DEATH
    };
    sound_here(sim, i, "sound_die", category);
    // 0x4e219f: the soul gems bound to the dead unit vanish (0x465440).
    for j in 0..m.objects.len() {
        if m.objects[j].class == class::ITEM_SOUL
            && sim.gameplay.actors[j].bound == Some(i)
            && m.objects[j].flags & FLAG_INACTIVE == 0
        {
            consume(sim, m, j);
        }
    }
    // 0x4e21eb → 0x504890: before the cells are freed, the items fall one
    // per free cell in the rings around the holder.
    for j in sim.objects[i].contents.clone() {
        if carried(&m.objects[j]) {
            release_item(sim, m, g, lib, i, j);
        }
    }
    m.objects[i].health = 0;
    release_passengers(sim, m, lib, i);
    if let Some(holder) = sim.objects[i].holder.take() {
        sim.objects[holder].contents.retain(|&j| j != i);
    }
    let die = (world::class::is_unit(m.objects[i].class) || m.objects[i].building())
        .then(|| animation_timing(lib, &m.objects[i], "image_die"))
        .flatten();
    // 0x402c50 as the object leaves the map: a marked wall takes the marks
    // round it off (0x417190) before its record is cleared.
    if sim.gameplay.ai.manager.is_some() {
        ai::breach::fallen(sim, m, lib, i);
    }
    sim.gameplay.actors[i] = Default::default();
    match die {
        Some((_, duration)) => {
            let o = &mut m.objects[i];
            o.flags |= FLAG_DYING;
            o.owner = 0;
            if o.data.len() > 12 {
                o.data[12] = 0;
            }
            let a = &mut sim.gameplay.actors[i];
            a.action_image = "image_die".into();
            a.dying_until = sim.ticks + duration.div_ceil(TICK_MS).max(1);
            sim.objects[i].order = OrderState::Dying;
        }
        None => {
            // 0x41bf00: the units on a fallen bridge go with it, with no
            // death.
            if m.objects[i].class == class::BRIDGE {
                let r = movement::footprint_rect(lib, &m.objects[i]);
                for j in 0..m.objects.len() {
                    let x = &m.objects[j];
                    if world::class::is_unit(x.class)
                        && alive(x)
                        && sim.objects[j].holder.is_none()
                        && (r[0]..=r[2]).contains(&(x.x as i32))
                        && (r[1]..=r[3]).contains(&(x.y as i32))
                    {
                        vanish(sim, m, j);
                    }
                }
            }
            leave_debris(sim, m, lib, i);
            m.objects[i].flags |= FLAG_INACTIVE;
            sim.objects[i].order = OrderState::Idle;
            remove_passengers(sim, m, lib, i);
        }
    }
}
/// 0x504890 → 0x504550: each passenger steps on land within 8 rings of
/// the holder ([`way_out`], 0x5073f0, type 1); from the first that finds no
/// cell on, they stay inside.
fn release_passengers(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary, i: usize) {
    while let Some(&j) = sim.objects[i].contents.first() {
        if !step_off(sim, m, lib, i, j) {
            return;
        }
    }
}
/// 0x504550 without a point: `j` leaves `h` for a land cell within 8 rings
/// ([`way_out`]); false when there is none and it stays inside.
fn step_off(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    h: usize,
    j: usize,
) -> bool {
    let (rect, start) = way_out(sim, m, lib, h, j);
    // 0x504691: a large unit by 0x5077a0, in its own movement type.
    let size = footprint(lib, &m.objects[j]);
    let place = if size == [1, 1] {
        exit_cell(sim, rect, start, 1, 8)
    } else {
        large_exit(sim, rect, lib.layer(&m.objects[j]), size)
    };
    let Some(q) = place else {
        return false;
    };
    leave(sim, m, h, j, q);
    true
}
/// An occupant clicked in the inventory leaves its holder (0x4a1c8c →
/// 0x490ad0, message 0x45 → 0x48f7f3) as from 0x504550 with no point; with
/// Ctrl it becomes the player's selection (0x453340, 0x453240).
fn leave_holder(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    req: &Request,
) -> std::result::Result<String, String> {
    let h = req
        .selected
        .first()
        .copied()
        .filter(|&h| h < m.objects.len())
        .ok_or("Seleziona il contenitore")?;
    let j = req
        .target
        .filter(|&j| sim.objects.get(j).is_some_and(|s| s.holder == Some(h)))
        .ok_or("L'unità non è nel contenitore")?;
    if !m.controls(m.objects[h].owner) || carried(&m.objects[j]) {
        return Err("Ordine non disponibile".into());
    }
    // Between two steps the cells are those the last step began with; the
    // original looks at the map as it is.
    movement::refresh(sim, m, g, lib);
    if !step_off(sim, m, lib, h, j) {
        return Err("Nessuno spazio per uscire".into());
    }
    if req.ctrl && m.objects[j].owner == m.player() && sim.gameplay.script.outbox.len() < 1023 {
        sim.gameplay.script.outbox.push(Show::ClearSelection);
        sim.gameplay.script.outbox.push(Show::Select(j));
    }
    Ok("Ordine impartito".into())
}
/// `j` is out of `h`, on `q`.
fn leave(sim: &mut Simulation, m: &mut MissionWorld, h: usize, j: usize, q: [u16; 2]) {
    sim.objects[h].contents.retain(|&k| k != j);
    sim.objects[j].holder = None;
    if carried(&m.objects[j]) {
        item_left(sim, m, h, j);
    }
    sim.objects[j].entry = None;
    m.objects[j].flags &= !FLAG_HELD;
    position(&mut m.objects[j], q);
    sim.gameplay
        .pathing
        .cells
        .set([q[0] as i32, q[1] as i32], pathing::OCCUPIED[1], true);
}
/// A destroyed building leaves rubble (vt+0x1d8 = 0x41db50 → vt+0x2b4): a
/// `debris` object (class 87) on its cell, `debris<W>x<H>` of its size up to
/// 6 (`debris3x3` without one, 0x41dba0, 0x444aa0); the harbor `debris
/// harbor` (0x45b420), the bridge its own (0x41c770), the dummy
/// `dummy1_demol` or `dummy2_demol` by its picture (0x448110). Gates
/// (`g{w|s}<n>_demol`, 0x4578c0) and walls (`walls_{wood|stone}_debris`,
/// 0x4fbd10) leave lasting rubble. Fields die as units do, with none.
fn leave_debris(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary, i: usize) {
    let o = &m.objects[i];
    let stone = matches!(o.class, class::WALL_STONE | class::GATE_STONE);
    let (picture, lasting) = match o.class {
        39..=53 | 55 | 56 | 58..=60 | 65 | 66 | 69..=71 | 76 | 78 | 83 => {
            let [w, h] = footprint(lib, o).map(|n| n.min(6));
            let named = format!("debris{w}x{h}");
            if lib.images.get(&named, "image").is_some() {
                (named, false)
            } else {
                ("debris3x3".to_owned(), false)
            }
        }
        class::HARBOR => ("debris harbor".to_owned(), false),
        // 0x41c770, by the bridge's way.
        class::BRIDGE => (
            if o.data.get(56).copied().unwrap_or(0) == 0 {
                "bridge_horz_demol"
            } else {
                "bridge_vert_demol"
            }
            .to_owned(),
            false,
        ),
        class::DUMMY => (
            if o.variant < 2 {
                "dummy1_demol"
            } else {
                "dummy2_demol"
            }
            .to_owned(),
            false,
        ),
        class::GATE_WOOD | class::GATE_STONE => {
            let n = o.byte(0x38);
            let n = if n > 3 { n - 2 } else { n };
            (format!("g{}{n}_demol", if stone { 's' } else { 'w' }), true)
        }
        class::WALL_WOOD | class::WALL_STONE => (
            if stone {
                "walls_stone_debris"
            } else {
                "walls_wood_debris"
            }
            .to_owned(),
            true,
        ),
        _ => return,
    };
    let (at, civilization) = ([o.x, o.y], o.civilization);
    let Ok(j) = new_unit(sim, m, lib, class::DEBRIS, 0, civilization, at) else {
        return;
    };
    m.objects[j].custom_image = picture;
    m.objects[j].variant = 0;
    // The first frame changes 2000 ms later (0x444ba8).
    sim.gameplay.actors[j].rubble = Some(Rubble {
        due_ms: sim.ticks * TICK_MS + 2000,
        lasting,
    });
}
/// Rubble's timer (0x444bd0), every 2000 ms: the next frame; past the last
/// the rubble goes, or stays on its last frame when lasting (+0xca).
fn crumble(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary) {
    let now = sim.ticks * TICK_MS;
    for i in 0..m.objects.len() {
        let o = &m.objects[i];
        if o.class != class::DEBRIS || o.flags & FLAG_INACTIVE != 0 {
            continue;
        }
        let Some(mut r) = sim.gameplay.actors[i].rubble.filter(|r| now >= r.due_ms) else {
            continue;
        };
        let rows = lib
            .images
            .get(&o.custom_image, "rows")
            .and_then(|s| s.trim().parse::<u16>().ok())
            .unwrap_or(1)
            .max(1);
        let next = (o.variant + 1) % rows;
        if next != 0 {
            m.objects[i].variant = next;
            r.due_ms += 2000;
            sim.gameplay.actors[i].rubble = Some(r);
        } else if r.lasting {
            m.objects[i].variant = rows - 1;
            sim.gameplay.actors[i].rubble = None;
        } else {
            vanish(sim, m, i);
        }
    }
}
/// The sector lists (0x5ce958, 8×8 cells): an object that comes on the map
/// or into another sector goes to the head of its sector's list (0x5001a0,
/// 0x500da0). A unit's step does it as it takes the cell
/// (`movement::step`); the other arrivals are read here once a step, in the
/// objects' order.
fn arrive_in_sectors(sim: &mut Simulation, m: &MissionWorld) {
    let now = sim.ticks * TICK_MS;
    for (i, o) in m.objects.iter().enumerate() {
        let on_map = o.flags & (FLAG_INACTIVE | FLAG_HELD) == 0
            && sim.objects.get(i).is_some_and(|s| s.holder.is_none());
        let sector = on_map.then_some([o.x / 8, o.y / 8]);
        let Some(a) = sim.gameplay.actors.get_mut(i) else {
            continue;
        };
        if a.sector != sector {
            a.sector = sector;
            if sector.is_some() {
                sim.gameplay.sector_clock += 1;
                a.sector_stamp = sim.gameplay.sector_clock;
                a.sector_ms = now;
            }
        }
    }
}
/// The order a search over an area meets objects in (0x44e690): sector
/// columns from the west, each from the north, each list from its head (the
/// latest arrival).
pub(super) fn sector_order(
    sim: &Simulation,
    k: usize,
) -> (u16, u16, std::cmp::Reverse<(u64, u64)>) {
    let a = &sim.gameplay.actors[k];
    let [sx, sy] = a.sector.unwrap_or([u16::MAX; 2]);
    (sx, sy, std::cmp::Reverse((a.sector_ms, a.sector_stamp)))
}
/// When a holder leaves the map (0x5003af) its passengers try once more to
/// step off; those still without a cell go with it, with no death
/// (vt+0x1b0).
fn remove_passengers(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary, i: usize) {
    release_passengers(sim, m, lib, i);
    for j in std::mem::take(&mut sim.objects[i].contents) {
        sim.objects[j].holder = None;
        m.objects[j].flags &= !FLAG_HELD;
        vanish(sim, m, j);
    }
}
/// The corpse disappears when its death animation has ended (0x504eb0).
fn decay_corpse(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary, i: usize) {
    let a = &mut sim.gameplay.actors[i];
    a.action_elapsed_ms = a.action_elapsed_ms.saturating_add(TICK_MS);
    if sim.ticks >= a.dying_until {
        a.dying_until = 0;
        a.action_image.clear();
        m.objects[i].flags = (m.objects[i].flags & !FLAG_DYING) | FLAG_INACTIVE;
        sim.objects[i].order = OrderState::Idle;
        remove_passengers(sim, m, lib, i);
    }
}
/// `(T, P)` of an animation (0x4986b3): the time before `actionrow` (the
/// last row when absent) and the whole `anim_time`.
fn animation_timing(lib: &ObjectLibrary, o: &MapObject, property: &str) -> Option<(u64, u64)> {
    let section = lib.property(o, property)?;
    let rows = lib
        .images
        .get(section, "rows")
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    let timeline = crate::animation::Timeline::directional(&lib.images, section, rows);
    let row = lib
        .images
        .get(section, "actionrow")
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|&r| r < rows)
        .unwrap_or(rows - 1);
    let ends = timeline.ends();
    let before = if row == 0 {
        0
    } else {
        ends.get(row - 1).copied().unwrap_or(0)
    };
    (timeline.duration_ms > 0).then_some((before, timeline.duration_ms))
}
/// What the Attack op (0xd) tells its program: keep striking, follow a
/// target out of reach (result 2) or look for another (result 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Strike {
    On,
    Away,
    Over,
}
/// The target is there to be struck: its record exists and it is not dying.
fn standing(m: &MissionWorld, j: usize) -> bool {
    m.objects
        .get(j)
        .is_some_and(|x| alive(x) && x.flags & FLAG_DYING == 0)
}
#[allow(clippy::too_many_arguments)]
fn attack(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    j: usize,
) -> Strike {
    if sim.gameplay.actors[i].hold_fire {
        return Strike::On;
    }
    // 0x4df884: training on the dummy ends at level 8 or at the cap, and
    // never starts for what answers 1 to 0x2729.
    if m.objects[j].class == class::DUMMY
        && (untrainable(&m.objects[i])
            || sim.objects[i].level >= experience_rules(m, m.objects[i].owner).cap.min(8))
    {
        sim.objects[i].order = OrderState::Idle;
        return Strike::Over;
    }
    let now = sim.ticks * TICK_MS;
    let class = m.objects[i].class;
    sim.objects[i].order = OrderState::Attacking;
    // Towers (0x4dba30) and warships (0x4e8f00) fire on a fixed period,
    // without an attack animation.
    let fixed = if (39..=43).contains(&class) {
        Some(if class >= 42 { 3000 } else { 1000 })
    } else if matches!(class, 23..=25) {
        Some(ship_period(sim, m, i))
    } else {
        None
    };
    if let Some(period) = fixed {
        // 0x4dbc43: the cannon and rocket towers never fire within two cells.
        if matches!(class, 42 | 43)
            && combat::within(m, lib, i, movement::target_rect(sim, m, lib, j), 2)
        {
            return Strike::Over;
        }
        let a = &sim.gameplay.actors[i];
        if a.last_hit_ms == 0 || now.saturating_sub(a.last_hit_ms) >= period {
            sim.gameplay.actors[i].last_hit_ms = now.max(1);
            // 0x4df800: the ship turns broadside (its attack sheet's startcol).
            if matches!(class, 23..=25) {
                let rect = movement::footprint_rect(lib, &m.objects[j]);
                face(sim, m, lib, i, rect, Some("image_attack"));
            }
            blow(sim, m, g, lib, c, i, j);
            if !standing(m, j) {
                return Strike::Over;
            }
        }
        return Strike::On;
    }
    // Without `image_attack` the native unit never leaves its preparation.
    // It turns to the target when its picture changes (0x4e2459…) and at
    // every blow (0x4df96f, 0x4dfa41).
    let (image, wait) = {
        let a = &sim.gameplay.actors[i];
        (a.action_image.clone(), a.attack_wait_ms)
    };
    let swung = swing(sim, m, lib, i);
    let a = &sim.gameplay.actors[i];
    if matches!(swung, Some(Swing::Blow { .. }))
        || a.action_image != image
        || (wait == 0 && a.attack_wait_ms != 0)
    {
        let picture = if a.action_image.is_empty() {
            "image_idle".to_owned()
        } else {
            a.action_image.clone()
        };
        let rect = movement::footprint_rect(lib, &m.objects[j]);
        face(sim, m, lib, i, rect, Some(&picture));
    }
    let Some(Swing::Blow { due, lead, period }) = swung else {
        return Strike::On;
    };
    // 0x4dfab4: at the blow the target may stand one cell beyond RANGE; a
    // target inside the minimum range ends the op (0x4dfae9).
    let rect = movement::target_rect(sim, m, lib, j);
    if !combat::within(m, lib, i, rect, combat::max_range(m, c, i) + 1) {
        return Strike::Away;
    }
    if combat::too_close(sim, m, lib, i, j) {
        return Strike::Over;
    }
    // 0x4dfbbd: before each blow a computer's unit, but on a wall at
    // priority 4000, looks for its best target (vt+0x2d0): taken on by its
    // retaliation (0x413410) it calls for help and the blow is not struck;
    // so too when its program against the target is no attack (vt+0x2b0).
    let breaching = ai::breach::on_wall(sim, m, i, j);
    if !breaching && ai_managed(sim, m, i) && m.objects[i].flags & FLAG_DYING == 0 {
        if let Some(best) = find_target(sim, m, lib, c, i)
            && retaliate(sim, m, lib, c, i, best)
        {
            call_for_help(sim, m, lib, c, i, best);
            return Strike::On;
        }
        let program = program_against(sim, m, lib, c, i, j);
        if !matches!(program, Intent::Attack(_)) {
            let a = &mut sim.gameplay.actors[i];
            a.intent = Some(program);
            halt(a);
            a.action_image.clear();
            return Strike::On;
        }
    }
    // 0x4dfc73: the blow is stamped with its own due time, so the period
    // keeps its milliseconds whatever the tick.
    sim.gameplay.actors[i].last_hit_ms = due;
    blow(sim, m, g, lib, c, i, j);
    // 0x4dfc54: after each blow on the wall, one chance in ten that the
    // attack goes down to priority 3000.
    if breaching && sim.gameplay.game_rng.range(0, 9) == 0 {
        sim.gameplay.actors[i].automatic = true;
    }
    // 0x4dfc8a: a fallen target ends the op; one out of RANGE is followed.
    if !standing(m, j) {
        return Strike::Over;
    }
    let rect = movement::target_rect(sim, m, lib, j);
    if !combat::within(m, lib, i, rect, combat::max_range(m, c, i)) {
        return Strike::Away;
    }
    next_picture(sim, m, lib, i, due, lead, period);
    Strike::On
}
/// Where the attack picture's cycle stands.
enum Swing {
    Wait,
    /// A blow lands now; it was due at `due` (ms), with the lead before
    /// `actionrow` and the period of `image_attack`.
    Blow {
        due: u64,
        lead: u64,
        period: u64,
    },
}
/// The attack picture's cycle (0x4e24bc): what remains of the previous
/// period beyond the lead is waited with the standing picture, then
/// `image_attack` runs and the blow comes at its `actionrow`. None without
/// an attack picture.
fn swing(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, i: usize) -> Option<Swing> {
    let now = sim.ticks * TICK_MS;
    let Some((lead, period)) = animation_timing(lib, &m.objects[i], "image_attack") else {
        sim.objects[i].order = OrderState::Idle;
        return None;
    };
    let a = &mut sim.gameplay.actors[i];
    if matches!(a.action_image.as_str(), "image_attack2" | "image_attack3")
        && now >= a.attack_wait_ms
    {
        // After `image_attack2` the blow is prepared again with `image_attack`.
        a.action_image = "image_attack".into();
        a.action_elapsed_ms = 0;
        a.attack_wait_ms = 0;
    }
    if !a.action_image.starts_with("image_attack") {
        let since = if a.last_hit_ms == 0 {
            u64::MAX
        } else {
            now.saturating_sub(a.last_hit_ms)
        };
        let remaining = period.saturating_sub(since);
        if remaining > lead {
            if a.attack_wait_ms == 0 {
                a.attack_wait_ms = now + remaining - lead;
            }
            if now < a.attack_wait_ms {
                sim.objects[i].order = OrderState::Idle;
                return Some(Swing::Wait);
            }
        }
        a.attack_wait_ms = 0;
        a.action_image = "image_attack".into();
        a.action_elapsed_ms = 0;
        a.attack_due_ms = now + lead;
    }
    if now < a.attack_due_ms {
        a.action_elapsed_ms = a.action_elapsed_ms.saturating_add(TICK_MS);
        return Some(Swing::Wait);
    }
    Some(Swing::Blow {
        due: a.attack_due_ms.max(1),
        lead,
        period,
    })
}
/// The picture of the next blow (0x4e5720): half of the blows continue
/// with `image_attack2` when the unit has one, both restarting from their
/// `actionrow`; an Asian peasant of a Kung Fu kingdom picks among three
/// (0x4ae090, K+0x116).
fn next_picture(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    due: u64,
    lead: u64,
    period: u64,
) {
    let o = &m.objects[i];
    let kungfu = o.class == class::PEON
        && o.civilization == 3
        && researches(m, o.owner)
            .iter()
            .any(|r| r.eq_ignore_ascii_case("te_kungfu"))
        && lib.property(o, "image_attack3").is_some();
    let next = if kungfu {
        ["image_attack", "image_attack2", "image_attack3"][random(sim, 3) as usize]
    } else if lib.property(o, "image_attack2").is_some() && random(sim, 2) == 1 {
        "image_attack2"
    } else {
        "image_attack"
    };
    let a = &mut sim.gameplay.actors[i];
    if next != "image_attack" {
        let (lead2, period2) = animation_timing(lib, &m.objects[i], next).unwrap_or((0, period));
        a.action_image = next.into();
        a.action_elapsed_ms = lead2;
        a.attack_wait_ms = due + period2;
        a.attack_due_ms = due + period2 + lead;
    } else {
        a.action_image = "image_attack".into();
        a.action_elapsed_ms = lead;
        a.attack_due_ms = due + period;
    }
}
/// A shot at a cell (Attack ground, programs 0x430a77/0x430b25): the
/// unit's missile lands there and hurts what it finds.
fn shoot_ground(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    cell: [u16; 2],
) {
    let class = m.objects[i].class;
    let shot = if class == 18 {
        (combat::max_range(m, c, i) > 0).then_some((missile::ARROW, 500))
    } else {
        missile(class)
    };
    let Some((kind, speed)) = shot else {
        return;
    };
    let a = &mut sim.gameplay.actors[i];
    a.effect_until = 0;
    a.disguise = None;
    let source = sim
        .effective_attributes(i, m)
        .unwrap_or_else(|| sim.objects[i].clone());
    // 0x4e8e89: a warship's shell carries its class record's ATTACK.
    let amount = if matches!(class, 23..=25) {
        lib.attributes.get(&m.objects[i]).attack
    } else {
        (source.attack & 0xffff) as u16
    };
    let category = strike_category(m, c, i);
    sound(sim, i, "sound_attack", category);
    let end = [cell[0] as i32 * 32 + 16, cell[1] as i32 * 32 + 16];
    let start = shot_start(lib, m, i, end, sim.gameplay.actors[i].direction);
    let pixels = ((end[0] - start[0]) as f64).hypot((end[1] - start[1]) as f64);
    let ms = pixels.trunc() as u64 * 1000 / speed as u64;
    if sim.gameplay.projectiles.len() < 4096 {
        sim.gameplay.projectiles.push(Projectile {
            attacker: i,
            target: usize::MAX,
            amount,
            splash: amount >> 1,
            category: damage_category(m, c, i),
            from: [m.objects[i].x, m.objects[i].y],
            cell,
            kind,
            launched: sim.ticks,
            lands: sim.ticks + ms.div_ceil(TICK_MS).max(1),
            shooter: class as u16,
            start,
            end,
            ..Default::default()
        });
    }
}
/// The damage category of a blow, fixed by the routine that deals it: melee
/// 0 (0x4df6e0), arrows 1 (0x432b95), shells, rockets and ballista bolts 2
/// (0x43389f, 0x432fdf, 0x43422f), flame bolts and the dragon 3 (0x431baf,
/// 0x4eea56). The priest without RANGE strikes in melee (0x4ea464).
fn damage_category(m: &MissionWorld, c: &Catalog, i: usize) -> u8 {
    match m.objects[i].class {
        4 | 6 | 10 | 25 | 39..=41 => 1,
        19..=21 | 23 | 24 | 42 | 43 => 2,
        17 | 32 | 38 => 3,
        18 if combat::max_range(m, c, i) > 0 => 3,
        _ => 0,
    }
}
/// 0x4eefd0: in 7 of 31 blows the messiah tries a spell of table 0x4ef1a8
/// it has the mana for: heal on itself at three quarters of its health or
/// less, raise dead anywhere, the others within their range less one.
fn messiah_spell(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    j: usize,
) -> bool {
    const SPELLS: [&str; 7] = [
        "spell_heal",
        "spell_firerain",
        "spell_lightning",
        "spell_freeze",
        "spell_raisedead",
        "spell_flamebolt",
        "spell_curse",
    ];
    let Some(&key) = SPELLS.get(random(sim, 31) as usize) else {
        return false;
    };
    let o = m.objects[i].clone();
    let (range, cost) = spell_parameters(c, m, &o, key);
    if cost > sim.objects[i].mana {
        return false;
    }
    let target = match key {
        "spell_heal" if o.health as u32 > o.max_health as u32 * 3 / 4 => return false,
        "spell_heal" => i,
        "spell_raisedead" => j,
        _ if distance(lib, &o, &m.objects[j]) as i64 > range as i64 - 1 => return false,
        _ => j,
    };
    let x = &m.objects[target];
    let req = Request {
        order: key.into(),
        selected: vec![i],
        cell: Some([x.x, x.y]),
        target: Some(target),
        ..Default::default()
    };
    let _ = cast(sim, m, g, lib, c, i, key, &req);
    true
}
/// The blow itself (vt+0x2ec): attacking reveals an invisible unit; melee
/// damage is immediate, missiles carry the raw attack to where they land,
/// and every victim adds its own bonus.
fn blow(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    j: usize,
) {
    let a = &mut sim.gameplay.actors[i];
    a.effect_until = 0;
    a.disguise = None;
    let class = m.objects[i].class;
    if class == class::MESSIAH && messiah_spell(sim, m, g, lib, c, i, j) {
        return;
    }
    let source = sim
        .effective_attributes(i, m)
        .unwrap_or_else(|| sim.objects[i].clone());
    let mut amount = (source.attack & 0xffff) as u16;
    // 0x4dbd8e and 0x4e8e89: cannon and rocket towers and warships load
    // their shells with the ATTACK of their kingdom's class record, which
    // only Gunpowder changes (+10 to the cannon tower's, 0x4d3bec), not with
    // what levels, spells or the map gave the object.
    if matches!(class, 42 | 43 | 23..=25) {
        let gunpowder = if class == 42 {
            crate::simulation::completions(researches(m, m.objects[i].owner), "te_gunpowder")
        } else {
            0
        };
        amount = lib.attributes.get(&m.objects[i]).attack + 10 * gunpowder as u16;
    }
    // 0x4ec830: the stone golem strikes buildings for 13/10.
    if class == 16 && m.objects[j].building() {
        amount = (amount as u32 * 13 / 10) as u16;
    }
    let category = damage_category(m, c, i);
    // 0x4ea4c0: a priest with RANGE shoots a flame bolt (its picture
    // `exorcism`).
    let shot = if class == 18 {
        (combat::max_range(m, c, i) > 0).then_some((missile::ARROW, 500))
    } else {
        missile(class)
    };
    let strike = strike_category(m, c, i);
    sound(sim, i, "sound_attack", strike);
    match shot {
        Some((kind, speed)) => {
            let from = [m.objects[i].x, m.objects[i].y];
            let cell = [m.objects[j].x, m.objects[j].y];
            // 0x4314d3: flight time = trunc(distance in pixels) × 1000 / speed.
            let (start, end) = shot_points(lib, m, i, j, sim.gameplay.actors[i].direction);
            let pixels = ((end[0] - start[0]) as f64).hypot((end[1] - start[1]) as f64);
            let ms = pixels.trunc() as u64 * 1000 / speed as u64;
            if sim.gameplay.projectiles.len() < 4096 {
                sim.gameplay.projectiles.push(Projectile {
                    attacker: i,
                    target: j,
                    amount,
                    // 0x4ed74a: shells and rockets hurt the 5×5 square
                    // around a direct hit with half the attack.
                    splash: amount >> 1,
                    category,
                    from,
                    cell,
                    kind,
                    launched: sim.ticks,
                    lands: sim.ticks + ms.div_ceil(TICK_MS).max(1),
                    shooter: class as u16,
                    start,
                    end,
                    ..Default::default()
                });
            }
            // 0x4e8dd3: the galleon's and the hulk's muzzle flash.
            if matches!(class, 23 | 24) {
                flash(
                    sim,
                    Flash {
                        image: "explosion_small",
                        at: [start[0], start[1] - 24],
                        fps: 0,
                        loops: 1,
                        prio: 200,
                        hold_ms: 0,
                        glow: 100,
                        column: 0,
                        delay_ms: 0,
                    },
                );
            }
        }
        None => {
            apply_damage(sim, m, g, lib, c, j, amount, category, Some(i));
            if class == 32 {
                dragon_fire(sim, m, lib, i);
            }
        }
    }
    // 0x4ee780: after its blow a ghost curses the hostile units around it,
    // for free, in 71 of 1001 cases.
    if class == 31 && random(sim, 1001) <= 70 {
        let req = Request {
            order: "spell_curse".into(),
            selected: vec![i],
            cell: Some([m.objects[i].x, m.objects[i].y]),
            ..Default::default()
        };
        let _ = cast(sim, m, g, lib, c, i, "spell_curse", &req);
    }
}
/// The dragon's breath (0x4eea00): the blow lands at once, then
/// `dragon_fire` plays once in the dragon's column from the centre of its
/// attack picture's selection rectangle (vt+0x13c, 0x501d60), moved toward
/// where it faces (table 0x4eeb58).
fn dragon_fire(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, i: usize) {
    const TOWARD: [[i32; 2]; 8] = [
        [0, 90],
        [78, 63],
        [110, 0],
        [78, -63],
        [0, -90],
        [-63, -63],
        [-110, 0],
        [-78, 63],
    ];
    let o = &m.objects[i];
    let Some(section) = lib.property(o, "image_attack") else {
        return;
    };
    let number = |key: &str, k: usize| {
        lib.images
            .numbers(section, key)
            .get(k)
            .copied()
            .unwrap_or(0)
    };
    let x1 = o.x as i32 * 32 - number("object", 0) + number("selection", 0);
    let y1 = o.y as i32 * 32 - number("object", 1) + number("selection", 1);
    let x2 = x1 + number("selection", 2) - 1;
    let y2 = y1 + number("selection", 3) - 1;
    let column = sim.gameplay.actors[i].direction & 7;
    let [dx, dy] = TOWARD[column as usize];
    flash(
        sim,
        Flash {
            image: "dragon_fire",
            at: [(x1 + x2) / 2 + dx, (y1 + y2) / 2 + dy],
            fps: 0,
            loops: 1,
            prio: 200,
            hold_ms: 0,
            glow: 0,
            column,
            delay_ms: 0,
        },
    );
}
/// Start and end of a missile in map pixels: the target's centre (its cells
/// × 32, vt+0x140); from the point of the attack picture's column for
/// archers, horse archers, longbowmen and the ballista, the corner past the
/// first cell for the xebec and the arrow towers (0x4eaa80, 0x4dbc9d), 48
/// pixels toward the target for the galleon and the hulk (0x4e8d20), the
/// centre for the others.
fn shot_points(
    lib: &ObjectLibrary,
    m: &MissionWorld,
    i: usize,
    j: usize,
    column: u8,
) -> ([i32; 2], [i32; 2]) {
    let t = &m.objects[j];
    let [tw, th] = footprint(lib, t).map(|n| n as i32);
    let end = [t.x as i32 * 32 + tw * 16, t.y as i32 * 32 + th * 16];
    (shot_start(lib, m, i, end, column), end)
}
fn shot_start(
    lib: &ObjectLibrary,
    m: &MissionWorld,
    i: usize,
    end: [i32; 2],
    column: u8,
) -> [i32; 2] {
    // Archers and horse archers (0x4e9b60, 0x4eb090, table 0x4e9cd8),
    // longbowmen (0x4e9d90, 0x4e9f08) and the ballista (0x4ed888, 0x590aac)
    // loose from a point of their attack picture chosen by its column.
    const ARCHER: [[i32; 2]; 8] = [
        [34, 36],
        [44, 35],
        [50, 30],
        [50, 22],
        [43, 16],
        [33, 16],
        [27, 22],
        [27, 30],
    ];
    const LONGBOW: [[i32; 2]; 8] = [
        [48, 45],
        [53, 51],
        [62, 58],
        [68, 49],
        [71, 43],
        [66, 37],
        [58, 39],
        [50, 39],
    ];
    const BALLISTA: [[i32; 2]; 8] = [
        [69, 123],
        [115, 93],
        [135, 51],
        [115, 15],
        [68, 2],
        [21, 15],
        [5, 51],
        [14, 93],
    ];
    let o = &m.objects[i];
    let [w, h] = footprint(lib, o).map(|n| n as i32);
    let centre = [o.x as i32 * 32 + w * 16, o.y as i32 * 32 + h * 16];
    let table = match o.class {
        4 | 10 => Some(&ARCHER),
        6 => Some(&LONGBOW),
        21 => Some(&BALLISTA),
        _ => None,
    };
    if let Some(table) = table
        && let Some(section) = lib.property(o, "image_attack")
    {
        let hot = lib.images.numbers(section, "object");
        let [dx, dy] = table[column as usize & 7];
        return [
            o.x as i32 * 32 - hot.first().copied().unwrap_or(0) + dx,
            o.y as i32 * 32 - hot.get(1).copied().unwrap_or(0) + dy,
        ];
    }
    match o.class {
        25 | 39..=41 => [(o.x as i32 + 1) * 32, (o.y as i32 + 1) * 32],
        23 | 24 => {
            let d = [end[0] - centre[0], end[1] - centre[1]];
            let len = (d[0] as f64).hypot(d[1] as f64);
            if len > 0. {
                [
                    centre[0] + (d[0] as f64 * 48. / len) as i32,
                    centre[1] + (d[1] as f64 * 48. / len) as i32,
                ]
            } else {
                centre
            }
        }
        _ => centre,
    }
}
/// The picture a spell plays over a unit (0x473d00): once at its own pace,
/// centred 10 pixels above the unit's rectangle and left there; none over
/// an invisible unit.
fn overlay(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    j: usize,
    image: &'static str,
) {
    if sim
        .gameplay
        .actors
        .get(j)
        .is_some_and(|a| a.effect_until > sim.ticks)
    {
        return;
    }
    let o = &m.objects[j];
    let [w, _] = footprint(lib, o);
    let x0 = o.x as i32 * 32;
    let x1 = x0 + w as i32 * 32 - 1;
    flash(
        sim,
        Flash {
            image,
            at: [(x0 + x1) / 2, o.y as i32 * 32 - 10],
            fps: 0,
            loops: 1,
            prio: 101,
            hold_ms: 0,
            glow: 0,
            column: 0,
            delay_ms: 0,
        },
    );
}
/// Twinkling stars (class 0x69, 0x473ed0): `n` stars of `magic_star` (11×11,
/// ten rows) in the rectangle for `ms`. Each star is placed from the stream
/// of pictures (x, y within the rectangle less the picture, 0x473ff0); the
/// first starts at once, the others wait a random number of frames. Every
/// 100 ms (0x4740d0) each star moves a frame; at the end of a twinkle it
/// starts another at a new place, unless that would outlast `ms`.
fn sparkles(sim: &mut Simulation, rect: [i32; 4], n: u32, ms: u32) {
    const ROWS: u32 = 10;
    const SIZE: i32 = 11;
    let place = |sim: &mut Simulation, active: bool| {
        let rng = &mut sim.gameplay.aux_rng;
        let x = rng.range(rect[0] + 1, rect[2] - SIZE - 1);
        let y = rng.range(rect[1] + 1, rect[3] - SIZE - 1);
        let frame = if active {
            0
        } else {
            rng.range(0, ROWS as i32 - 1) as u32
        };
        ([x, y], frame)
    };
    let mut stars: Vec<([i32; 2], u32)> = (0..n).map(|_| place(sim, false)).collect();
    let mut twinkles = Vec::new();
    if let Some(first) = stars.first_mut() {
        first.1 = 0;
        twinkles.push((first.0, 0));
    }
    let mut t = 100;
    while t <= ms {
        for k in 0..stars.len() {
            stars[k].1 += 1;
            if stars[k].1 == ROWS && t + ROWS * 100 <= ms {
                stars[k] = place(sim, true);
                twinkles.push((stars[k].0, t));
            }
        }
        t += 100;
    }
    for (at, delay_ms) in twinkles {
        flash(
            sim,
            Flash {
                image: "magic_star",
                at,
                fps: 10,
                loops: 1,
                prio: 200,
                hold_ms: 0,
                glow: 0,
                column: 0,
                delay_ms,
            },
        );
    }
}
/// An object's rectangle in pixels grown by `d`.
fn rect_around(lib: &ObjectLibrary, o: &MapObject, d: i32) -> [i32; 4] {
    let [w, h] = footprint(lib, o).map(|n| n as i32);
    let (x, y) = (o.x as i32 * 32, o.y as i32 * 32);
    [x - d, y - d, x + w * 32 - 1 + d, y + h * 32 - 1 + d]
}
/// Classes whose direct hit makes the big explosion (table 0x433acc).
const BLAST_CLASSES: [usize; 15] = [2, 19, 20, 21, 23, 24, 25, 40, 41, 46, 54, 57, 62, 67, 136];
/// Open water under a cell.
fn water_cell(g: &MapGrid, cell: [u16; 2]) -> bool {
    let (x, y) = (cell[0] as usize, cell[1] as usize);
    x < g.side && y < g.side && {
        let v = g.cells[y * g.side + x];
        (v >> 4) & 7 == 0 && (v >> 7) & 7 == 0
    }
}
/// The crater or the splash where a shell, bolt or meteor lands
/// (0x43392a, 0x43426c, 0x450820): `expl64` when it hits one of the blast
/// classes or a building (cannons and rockets only), otherwise a hole kept
/// for 10 s on land or a splash on water.
fn impact(
    sim: &mut Simulation,
    g: &MapGrid,
    at: [i32; 2],
    cell: [u16; 2],
    blast: bool,
    hold: bool,
) {
    let water = water_cell(g, cell);
    let (image, prio, hold_ms, glow) = if blast {
        ("expl64", 200, 10_000, 100)
    } else if water {
        ("expl_small_water", 1, 0, 0)
    } else {
        ("expl_small_hole", 1, if hold { 10_000 } else { 0 }, 0)
    };
    flash(
        sim,
        Flash {
            image,
            at,
            fps: 11,
            loops: 1,
            prio,
            hold_ms,
            glow,
            column: 0,
            delay_ms: 0,
        },
    );
    if water && !blast {
        sound_at(
            sim,
            cell,
            "@effects/water blop.wav",
            crate::audio::category::SPLASH,
        );
    } else {
        sound_at(
            sim,
            cell,
            "@effects/small explosion.wav",
            crate::audio::category::BLAST,
        );
    }
}
pub mod missile {
    /// An arrow: the damage follows the target it was shot at (0x432af0).
    pub const ARROW: u8 = 0;
    /// Cannon ball or rocket: the target's cell at launch, half damage
    /// around it after a direct hit (0x433770 / 0x432eb0).
    pub const SHELL: u8 = 1;
    /// Ballista bolt: whatever stands on the impact cell (0x434120).
    pub const BOLT: u8 = 2;
    /// Freeze bolt: freezes the units around the impact (0x432590).
    pub const FREEZE: u8 = 3;
    /// A fireball of the fire rain: 40 to everything around it (0x450820).
    pub const FIRE: u8 = 4;
    /// Flame ball: burns the cells it crosses (0x431f40).
    pub const FLAME: u8 = 5;
}
/// Missile and speed in pixels per second of each shooter (0x4e9b60…0x4ed800, 0x4dc100).
fn missile(class: usize) -> Option<(u8, u32)> {
    Some(match class {
        // Mage and messiah shoot a flame bolt (class 0x79, 0x4ea280) that
        // follows its target like an arrow.
        4 | 6 | 10 | 17 | 38 | 39..=41 => (missile::ARROW, 500),
        25 => (missile::ARROW, 600),
        23 | 24 => (missile::SHELL, 300),
        19 | 20 => (missile::SHELL, 100),
        42 => (missile::SHELL, 250),
        43 => (missile::SHELL, 400),
        21 => (missile::BOLT, 498),
        _ => return None,
    })
}
/// `0..n` from the game stream (0x43b050).
fn random(sim: &mut Simulation, n: u64) -> u64 {
    let hi = n.clamp(1, i32::MAX as u64) as i32 - 1;
    sim.gameplay.game_rng.range(0, hi) as u64
}
/// `0..n` from the stream of sounds and pictures (0x5cf588).
fn random_aux(sim: &mut Simulation, n: u64) -> u64 {
    let hi = n.clamp(1, i32::MAX as u64) as i32 - 1;
    sim.gameplay.aux_rng.range(0, hi) as u64
}
/// Missiles that reach their target this tick.
fn land_projectiles(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
) {
    if sim.gameplay.projectiles.is_empty() {
        return;
    }
    // 0x4507b0: a fireball of the fire rain, as it is made, sounds three
    // times in ten (`effects\meteor.wav`, category 15, where it starts).
    let made: Vec<[u16; 2]> = sim
        .gameplay
        .projectiles
        .iter()
        .filter(|p| p.kind == missile::FIRE && p.launched == sim.ticks)
        .map(|p| {
            [
                (p.start[0].max(0) / 32) as u16,
                (p.start[1].max(0) / 32) as u16,
            ]
        })
        .collect();
    for cell in made {
        if sim.gameplay.aux_rng.range(0, 100) < 30 {
            sound_at(
                sim,
                cell,
                "@effects/meteor.wav",
                crate::audio::category::BLAST,
            );
        }
    }
    let (landed, flying): (Vec<_>, Vec<_>) = std::mem::take(&mut sim.gameplay.projectiles)
        .into_iter()
        .partition(|p| p.lands <= sim.ticks);
    sim.gameplay.projectiles = flying;
    for p in landed {
        // A spent arrow has flown on for its 500 ms and goes.
        if p.flown != 0 {
            continue;
        }
        let attacker = Some(p.attacker).filter(|&a| {
            m.objects
                .get(a)
                .is_some_and(|x| x.flags & FLAG_INACTIVE == 0)
        });
        // Objects whose cells contain the impact cell.
        let on_cell = |sim: &Simulation, m: &MissionWorld| -> Vec<usize> {
            m.objects
                .iter()
                .enumerate()
                .filter(|(k, x)| {
                    alive(x)
                        && sim.objects[*k].holder.is_none()
                        && (crate::selection::unit(x) || x.building())
                        && distance_to_area(p.cell, [x.x, x.y], footprint(lib, x)) == 0
                })
                .map(|(k, _)| k)
                .collect()
        };
        // Where it lands, in map pixels (older saves: the cell's centre).
        let at = if p.end == [0, 0] {
            [p.cell[0] as i32 * 32 + 16, p.cell[1] as i32 * 32 + 16]
        } else {
            p.end
        };
        match p.kind {
            missile::FREEZE => {
                // 0x432590: the freeze explosion, or a splash on water.
                if water_cell(g, p.cell) {
                    impact(sim, g, at, p.cell, false, false);
                } else {
                    flash(
                        sim,
                        Flash {
                            image: "freeze_bolt_expl",
                            at,
                            fps: 12,
                            loops: 1,
                            prio: 200,
                            hold_ms: 0,
                            glow: 0,
                            column: 0,
                            delay_ms: 0,
                        },
                    );
                }
                freeze_impact(sim, m, g, lib, c, attacker, p.cell)
            }
            missile::FIRE => {
                impact(sim, g, at, p.cell, false, false);
                // 0x450820: 40 magic damage, with no attacker, to every unit
                // and building meeting the 5×5 square around the impact.
                for k in in_square(sim, m, lib, p.cell, 2) {
                    apply_damage(sim, m, g, lib, c, k, p.amount, 3, None);
                }
            }
            missile::ARROW => {
                // 0x431b00: the flame bolt of mages, priests and the
                // messiah bursts.
                if matches!(p.shooter, 17 | 18 | 38) {
                    flash(
                        sim,
                        Flash {
                            image: "flame_bolt_expl",
                            at,
                            fps: 12,
                            loops: 1,
                            prio: 200,
                            hold_ms: 0,
                            glow: 100,
                            column: 0,
                            delay_ms: 0,
                        },
                    );
                }
                if m.objects.get(p.target).is_some_and(alive) {
                    apply_damage(sim, m, g, lib, c, p.target, p.amount, p.category, attacker);
                } else if let Some(&k) = on_cell(sim, m).first() {
                    apply_damage(sim, m, g, lib, c, k, p.amount, p.category, attacker);
                } else if sim.gameplay.projectiles.len() < 4096 {
                    // 0x432bac: with nobody there the arrow flies on for
                    // another 500 ms, then goes.
                    sim.gameplay.projectiles.push(Projectile {
                        flown: p.lands,
                        lands: sim.ticks + 500 / TICK_MS,
                        ..p
                    });
                }
            }
            missile::BOLT => {
                impact(sim, g, at, p.cell, false, true);
                for k in on_cell(sim, m) {
                    apply_damage(sim, m, g, lib, c, k, p.amount, p.category, attacker);
                }
            }
            _ => {
                let hit = on_cell(sim, m);
                let blast = hit.first().is_some_and(|&k| {
                    BLAST_CLASSES.contains(&m.objects[k].class) || m.objects[k].building()
                });
                impact(sim, g, at, p.cell, blast, true);
                if hit.is_empty() {
                    continue;
                }
                // 0x44de50: every object whose cells meet the 5×5 square
                // around the impact; half the raw attack, nothing when 0.
                let splash: Vec<usize> = in_square(sim, m, lib, p.cell, 2)
                    .into_iter()
                    .filter(|k| !hit.contains(k))
                    .collect();
                for k in hit {
                    apply_damage(sim, m, g, lib, c, k, p.amount, p.category, attacker);
                }
                for k in splash {
                    apply_damage(sim, m, g, lib, c, k, p.splash, p.category, attacker);
                }
            }
        }
    }
}
fn push_projectile(
    sim: &mut Simulation,
    attacker: usize,
    target: usize,
    amount: u16,
    cell: [u16; 2],
    kind: u8,
    lands: u64,
    end: [i32; 2],
) {
    if sim.gameplay.projectiles.len() < 4096 {
        sim.gameplay.projectiles.push(Projectile {
            attacker,
            target,
            amount,
            category: 3,
            cell,
            kind,
            launched: sim.ticks,
            lands: lands.max(sim.ticks + 1),
            // 0x450620: a meteor falls 150 pixels straight down.
            start: [end[0], end[1] - 150],
            end,
            ..Default::default()
        });
    }
}
/// A spell missile from the caster's centre to the centre of `cell` at
/// `speed` pixels per second, moved in 50 ms steps (`+0x42` = 0x32).
#[allow(clippy::too_many_arguments)]
fn launch(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    caster: usize,
    target: usize,
    cell: [u16; 2],
    kind: u8,
    speed: u32,
    amount: u16,
) {
    let [w, h] = footprint(lib, &m.objects[caster]);
    let o = &m.objects[caster];
    let goal = [cell[0] as i32 * 32 + 16, cell[1] as i32 * 32 + 16];
    // 0x432350: the freeze bolt falls straight down from 500 pixels above
    // its goal, wherever the caster stands.
    let pos = if kind == missile::FREEZE {
        [goal[0], goal[1] - 500]
    } else {
        [
            o.x as i32 * 32 + w as i32 * 16,
            o.y as i32 * 32 + h as i32 * 16,
        ]
    };
    let pixels = ((goal[0] - pos[0]) as f64).hypot((goal[1] - pos[1]) as f64);
    let ms = (pixels * 1000. / speed as f64) as u64;
    let lands = if kind == missile::FLAME {
        // The flame ball lands when its steps reach the goal.
        u64::MAX
    } else {
        sim.ticks + ms.div_ceil(TICK_MS).max(1)
    };
    if sim.gameplay.projectiles.len() < 4096 {
        sim.gameplay.projectiles.push(Projectile {
            attacker: caster,
            target,
            amount,
            splash: 0,
            category: 3,
            from: [o.x, o.y],
            cell,
            kind,
            launched: sim.ticks,
            lands,
            pos,
            goal,
            entered: 0,
            shooter: o.class as u16,
            start: pos,
            end: goal,
            flown: 0,
        });
    }
}
/// Every cell of a 3×3 square is open water (0x4fd030).
fn water_square(g: &MapGrid, corner: [u16; 2]) -> bool {
    (0..3).all(|dy| {
        (0..3).all(|dx| {
            let (x, y) = (corner[0] as usize + dx, corner[1] as usize + dy);
            x < g.side && y < g.side && {
                let cell = g.cells[y * g.side + x];
                (cell >> 4) & 7 == 0 && (cell >> 7) & 7 == 0
            }
        })
    })
}
/// A raised body is deleted at once (0x4737a0).
fn remove_corpse(sim: &mut Simulation, m: &mut MissionWorld, i: usize) {
    let a = &mut sim.gameplay.actors[i];
    a.dying_until = 0;
    a.action_image.clear();
    m.objects[i].flags = (m.objects[i].flags & !FLAG_DYING) | FLAG_INACTIVE;
    sim.objects[i].order = OrderState::Idle;
}
/// Freeze bolt impact (0x4326c7): every unit within ±3 of the cell, the
/// caster included, is frozen for 20 s with its defence saved and zeroed
/// (0x4e5882), and takes min(40, HP − 1) magic damage.
fn freeze_impact(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    caster: Option<usize>,
    at: [u16; 2],
) {
    let now = sim.ticks;
    let units: Vec<usize> = m
        .objects
        .iter()
        .enumerate()
        .filter(|(k, x)| {
            alive(x)
                && crate::selection::unit(x)
                && !machine(x)
                && !immune(x, effect::FREEZE)
                && sim.objects[*k].holder.is_none()
                && x.x.abs_diff(at[0]) <= 3
                && x.y.abs_diff(at[1]) <= 3
        })
        .map(|(k, _)| k)
        .collect();
    for j in units {
        let saved = if has_effect(sim, j, effect::FREEZE) {
            0
        } else {
            let saved = sim.objects[j].defence + 1;
            sim.objects[j].defence = 0;
            saved
        };
        let a = &mut sim.gameplay.actors[j];
        add_timed_effect(
            a,
            Effect {
                until: now + 400,
                kind: effect::FREEZE,
                saved,
                ..Default::default()
            },
        );
        a.frozen_until = a.frozen_until.max(now + 400);
        let damage = m.objects[j].health.saturating_sub(1).min(40);
        if damage > 0 {
            apply_damage(sim, m, g, lib, c, j, damage, 3, caster);
        }
    }
}
/// The flame balls move one 50 ms step (0x4316f0). On each new cell the
/// first three hit the first object there that is not allied to the
/// caster for twice the base damage; later cells burn every object of the
/// 3×3 around the ball but the caster, the full base on the ball's own
/// object and half beside it (0x431f40). Arrival does nothing more.
fn fly_flames(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
) {
    if !sim
        .gameplay
        .projectiles
        .iter()
        .any(|p| p.kind == missile::FLAME)
    {
        return;
    }
    let mut flying = Vec::new();
    for mut p in std::mem::take(&mut sim.gameplay.projectiles) {
        if p.kind != missile::FLAME {
            flying.push(p);
            continue;
        }
        let step = 700 * TICK_MS as i32 / 1000;
        let (dx, dy) = (p.goal[0] - p.pos[0], p.goal[1] - p.pos[1]);
        let left = (dx as f64).hypot(dy as f64);
        let before = [p.pos[0].div_euclid(32), p.pos[1].div_euclid(32)];
        let arrived = left <= step as f64;
        if arrived {
            p.pos = p.goal;
        } else {
            p.pos[0] += (dx as f64 * step as f64 / left).round() as i32;
            p.pos[1] += (dy as f64 * step as f64 / left).round() as i32;
        }
        let cell = [p.pos[0].div_euclid(32), p.pos[1].div_euclid(32)];
        if cell != before
            && cell[0] >= 0
            && cell[1] >= 0
            && (cell[0] as usize) < g.side
            && (cell[1] as usize) < g.side
        {
            let at = [cell[0] as u16, cell[1] as u16];
            let caster = p.attacker;
            let from = Some(caster).filter(|&k| m.objects.get(k).is_some_and(alive));
            let owner = m.objects.get(caster).map_or(0, |o| o.owner);
            if p.entered < 3 {
                let first = in_square(sim, m, lib, at, 0)
                    .into_iter()
                    .find(|&k| k != caster && !allied(m, owner, m.objects[k].owner));
                if let Some(k) = first {
                    apply_damage(sim, m, g, lib, c, k, p.amount * 2, 3, from);
                }
            } else {
                for k in in_square(sim, m, lib, at, 1) {
                    if k == caster || !alive(&m.objects[k]) {
                        continue;
                    }
                    let inside = distance_to_area(
                        at,
                        [m.objects[k].x, m.objects[k].y],
                        footprint(lib, &m.objects[k]),
                    ) == 0;
                    let amount = if inside { p.amount } else { p.amount / 2 };
                    apply_damage(sim, m, g, lib, c, k, amount, 3, from);
                }
            }
            p.entered = p.entered.saturating_add(1);
        }
        if !arrived {
            flying.push(p);
        }
    }
    // Damage above may have launched nothing new, but keep any that did.
    flying.append(&mut sim.gameplay.projectiles);
    sim.gameplay.projectiles = flying;
}
/// Whirlpools (0x4fd0e0): every 250 ms 20 magic damage, credited to the
/// caster, to each ship on their 3×3 cells; every 2 s a random step of one
/// cell onto open water; gone after 15 steps.
fn whirlpools(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
) {
    let now = sim.ticks * TICK_MS;
    for w in 0..m.objects.len() {
        if m.objects[w].class != class::WHIRLPOOL || !alive(&m.objects[w]) {
            continue;
        }
        if now >= sim.gameplay.actors[w].attack_due_ms {
            sim.gameplay.actors[w].attack_due_ms = now + 250;
            let caster = sim.gameplay.actors[w]
                .bound
                .filter(|&k| m.objects.get(k).is_some_and(alive));
            let at = [m.objects[w].x + 1, m.objects[w].y + 1];
            for k in in_square(sim, m, lib, at, 1) {
                if naval(&m.objects[k]) {
                    apply_damage(sim, m, g, lib, c, k, 20, 3, caster);
                }
            }
        }
        if now >= sim.gameplay.actors[w].attack_wait_ms {
            sim.gameplay.actors[w].attack_wait_ms = now + 2000;
            let a = &mut sim.gameplay.actors[w];
            a.moves_left = a.moves_left.saturating_sub(1);
            if a.moves_left == 0 {
                m.objects[w].flags |= FLAG_INACTIVE;
                continue;
            }
            let dx = random(sim, 3) as i32 - 1;
            let dy = random(sim, 3) as i32 - 1;
            let x = m.objects[w].x as i32 + dx;
            let y = m.objects[w].y as i32 + dy;
            if x >= 0 && y >= 0 && water_square(g, [x as u16, y as u16]) {
                position(&mut m.objects[w], [x as u16, y as u16]);
            }
        }
    }
}
/// After a victory, an automatic engagement resumes the interrupted order.
fn end_engagement(sim: &mut Simulation, i: usize) {
    sim.gameplay.actors[i].action_image.clear();
    finish(sim, i);
}
/// vt+0x2d0: the best target around `i` (0x4e4d90); a bear (0x4eeea0) looks
/// only when R(0, 100) < 30.
fn find_target(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
) -> Option<usize> {
    if m.objects[i].class == 26 && sim.gameplay.game_rng.range(0, 100) >= 30 {
        return None;
    }
    combat::search(sim, m, lib, c, i)
}
/// Steps 9 and 4 of the Attack program (0x42df70): unhooked from a target
/// that fell, cannot be attacked or was not reached, the unit looks at once
/// for the next one (Find enemy, op 0x23 with vt+0x2d0) and attacks it at
/// priority 3000; with nobody around the program ends. An order queued
/// with Shift comes first.
fn next_target(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, c: &Catalog, i: usize) {
    let a = &mut sim.gameplay.actors[i];
    a.action_image.clear();
    a.search_at = None;
    // Find enemy (op 0x23, vt+0x2d0) is a step of the Attack program
    // (0x42df96): an order queued with Shift waits until it finds nobody.
    match find_target(sim, m, lib, c, i) {
        Some(t) => {
            // 0x4e0ee2: a computer player's unit takes its own program.
            let intent = program_against(sim, m, lib, c, i, t);
            let a = &mut sim.gameplay.actors[i];
            a.intent = Some(intent);
            a.automatic = true;
            halt(a);
        }
        None => end_engagement(sim, i),
    }
}
/// What follows a run of the Attack op.
fn struck(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    strike: Strike,
) {
    match strike {
        Strike::On => {}
        Strike::Away => sim.gameplay.actors[i].action_image.clear(),
        Strike::Over => next_target(sim, m, lib, c, i),
    }
}
/// The kingdom whose relations a unit follows: hypnosis lends the caster's.
fn side(sim: &Simulation, m: &MissionWorld, k: usize) -> u8 {
    sim.gameplay
        .actors
        .get(k)
        .and_then(|a| a.allegiance)
        .unwrap_or(m.objects[k].owner)
}
/// Whether `k` fights `t`. Units under Discord attack anybody: 0x4e4d90 tests
/// the effect bit, then filters with 0x4e4cb0, which ignores owners. Objects
/// placed with owner 10 are otherwise ordinary and spare each other.
fn hostile(sim: &Simulation, m: &MissionWorld, k: usize, t: usize) -> bool {
    k != t && (has_effect(sim, k, effect::DISCORD) || enemy(m, side(sim, m, k), side(sim, m, t)))
}
/// The kingdoms' relation word R[a][b] (0x609068, set at 0x4404ce): itself,
/// owner 0 and towards 0 friendly (0x47); nobody fights owner 9 (0x40);
/// owners 9, 10 and 11 fight the players and owners 10 and 11 are fought by
/// everyone (0x10); between players the mission's diplomacy, hostile when
/// it says nothing.
fn relation(m: &MissionWorld, a: u8, b: u8) -> u32 {
    if a == b || a == 0 || b == 0 {
        0x47
    } else if b == 9 {
        0x40
    } else if b >= 10 || a >= 9 {
        0x10
    } else {
        let d = m
            .diplomacy
            .get((a - 1) as usize * 8 + (b - 1) as usize)
            .copied()
            .unwrap_or(0);
        let players = m.kingdoms.len() as u8;
        if d & 0x70 == 0 && a <= players && b <= players {
            d | 0x10
        } else {
            d
        }
    }
}
/// Whether `k` attacks `t` on its own (0x468190, used by the target filter
/// 0x412bf0): towers and fighters unless allied, other units and buildings
/// only when at war; birds never, the training dummy, once finished, only
/// for a computer player's kingdom (0x4681dd). Discord ignores the
/// relations.
fn auto_hostile(sim: &Simulation, m: &MissionWorld, k: usize, t: usize) -> bool {
    if k == t {
        return false;
    }
    if has_effect(sim, k, effect::DISCORD) {
        return true;
    }
    hostile_to(sim, m, side(sim, m, k), t)
}
/// 0x468190 for kingdom `k`: whether it attacks `t` on its own.
pub(crate) fn hostile_to(sim: &Simulation, m: &MissionWorld, k: u8, t: usize) -> bool {
    let r = relation(m, k, side(sim, m, t));
    let x = &m.objects[t];
    match x.class {
        39..=42 => r & 0x40 == 0,
        class::BIRD | class::EAGLE => false,
        class::DUMMY => x.flags & FLAG_CONSTRUCTION == 0 && combat::computer(sim, k),
        _ if x.building() => r & 0x10 != 0,
        _ if crate::selection::unit(x) && military(sim, m, t) => r & 0x40 == 0,
        _ if crate::selection::unit(x) => r & 0x10 != 0,
        _ => false,
    }
}
/// Objects that look for targets on their own: the native flag 0x10000 of
/// the military constructors, towers and warships.
fn military(sim: &Simulation, m: &MissionWorld, k: usize) -> bool {
    let o = &m.objects[k];
    sim.objects[k].attack > 0
        && ((crate::selection::unit(o)
            && !matches!(
                o.class,
                class::PEON | 2 | 14 | class::COW | class::FISHING_BOAT
            ))
            || ((39..=43).contains(&o.class) && o.flags & FLAG_CONSTRUCTION == 0))
}
/// Native 0x401fa0: attack + defence + range / 5 + max HP / 20, doubled for towers.
fn strength(sim: &Simulation, m: &MissionWorld, c: &Catalog, k: usize) -> u64 {
    let o = &m.objects[k];
    let s = sim
        .effective_attributes(k, m)
        .unwrap_or_else(|| sim.objects[k].clone());
    let range = c.stats(o, o.class_name(), researches(m, o.owner)).range as u64;
    let value = s.attack as u64 + s.defence as u64 + range / 5 + o.max_health as u64 / 20;
    if (39..=43).contains(&o.class) {
        value * 2
    } else {
        value
    }
}
/// The automatic checks of an object, each on its own timer.
fn watch(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
) -> Option<u32> {
    let now = sim.ticks * TICK_MS;
    let o = &m.objects[i];
    if (39..=43).contains(&o.class) {
        // 0x4db660: a tower first looks around 2000 ms after it stands, then
        // every 3000 ms (message 0x4217), when it has nothing to do and may
        // fire; it then attacks its best target (program 0x42ff60).
        if !military(sim, m, i) {
            sim.gameplay.actors[i].scan_due_ms = None;
            return None;
        }
        let a = &mut sim.gameplay.actors[i];
        let due = *a.scan_due_ms.get_or_insert(now + 2000);
        if now < due {
            return None;
        }
        a.scan_due_ms = Some(due + 3000);
        if a.intent.is_some() || a.hold_fire {
            return None;
        }
        if let Some(t) = find_target(sim, m, lib, c, i) {
            let a = &mut sim.gameplay.actors[i];
            a.intent = Some(Intent::Attack(t));
            a.automatic = true;
        }
        return None;
    }
    if !crate::selection::unit(o) {
        return None;
    }
    // Advancing moves and patrol legs (Move with argument 2) look around on
    // the shared 1000 ms timer of the units (message 0x4015) and on their
    // own, 1 ms after the leg starts and every 3000 ms while nobody is found
    // (message 0x4011). A target found meets retaliation and the call for
    // help (0x4e151c).
    let a = &sim.gameplay.actors[i];
    let advancing = match a.intent {
        Some(Intent::Move(_)) => a.move_mode == 1,
        // The last step of Approach is a Move with argument 2 (0x42de9b).
        Some(Intent::Approach(ap)) => ap.step == 4,
        Some(Intent::Patrol(..) | Intent::Circuit(..)) => true,
        _ => false,
    };
    if advancing {
        // A leg starts (the op start vt+0x168): a computer player's ninja
        // may hide (0x4ebe10).
        if sim.gameplay.actors[i].advance_due_ms.is_none() {
            ai::spells::set_off(sim, m, lib, c, i);
        }
        let a = &mut sim.gameplay.actors[i];
        let due = *a.advance_due_ms.get_or_insert(now);
        let own = now >= due;
        if own {
            a.advance_due_ms = Some(due + 3000);
        }
        if (own || sim.ticks % 20 == 0)
            && let Some(t) = find_target(sim, m, lib, c, i)
        {
            retaliate(sim, m, lib, c, i, t);
            call_for_help(sim, m, lib, c, i, t);
        }
    } else {
        sim.gameplay.actors[i].advance_due_ms = None;
    }
    // The idle tick (0x4dfd10), 1500 ms apart from the moment the unit
    // went idle; a ninja's and a spy's come every 2000 ms after the first
    // (0x4eb7f0, 0x4ec350).
    let every = idle_period(&m.objects[i]);
    let a = &mut sim.gameplay.actors[i];
    if a.intent.is_none()
        && let Some(due) = a.scan_due_ms
        && now >= due
    {
        a.scan_due_ms = Some(due + every);
        // Bear Idle (0x4eeed0): at each tick of its wait a bear sets off to
        // wander when R(0, 100) ≤ 20, without looking around first.
        if a.pasture.is_some() && sim.gameplay.game_rng.range(0, 100) <= 20 {
            let a = &mut sim.gameplay.actors[i];
            a.scan_due_ms = Some(u64::MAX);
            if let Some(p) = a.pasture.as_mut() {
                p.wander_due = Some(now);
            }
            return None;
        }
        let computer = scan(sim, m, lib, c, i);
        let a = &mut sim.gameplay.actors[i];
        let idle = a.idle_ms;
        a.idle_ms = a.idle_ms.saturating_add(every as u32);
        // A computer player's peasant asks its player (0x4ae4a5), with
        // its idle time as it was.
        return computer.then_some(idle);
    }
    None
}
/// Op 0x61 (0x4e034d): the spy comes out on the host's cell with its
/// record, and the host is gone; anyone else drops its invisibility and its
/// disguise (a ninja's own, 0x4ebe25).
fn reveal_self(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
) -> std::result::Result<(), String> {
    let Some(mole) = sim.gameplay.actors[i].mole.take() else {
        let a = &mut sim.gameplay.actors[i];
        a.effect_until = 0;
        a.cloak_drain = false;
        a.disguise = None;
        return Ok(());
    };
    let at = [m.objects[i].x, m.objects[i].y];
    let k = new_unit(sim, m, lib, 14, mole.kingdom, mole.civilization, at)?;
    let x = &mut m.objects[k];
    x.name = mole.name;
    x.subtitle = mole.subtitle;
    x.flags = (x.flags & !FLAG_HERO) | (mole.flags & FLAG_HERO);
    x.health = mole.health;
    x.max_health = mole.max_health;
    if !mole.data.is_empty() {
        let keep = [x.x, x.y];
        x.data = mole.data;
        position(x, keep);
        x.data[12] = mole.kingdom;
    }
    let s = &mut sim.objects[k];
    (s.level, s.xp, s.attack, s.defence, s.sight) =
        (mole.level, mole.xp, mole.attack, mole.defence, mole.sight);
    sim.gameplay.actors[k].spy_energy = mole.energy;
    // 0x4ec4a2: a hero's record promotes the spy again (vt+0x12c), naming
    // it if it has no name.
    if mole.flags & FLAG_HERO != 0 {
        become_hero(m, c, k);
    }
    m.objects[i].mole = 0;
    if m.objects[i].owner != 0 {
        set_control(sim, m, i, mole.kingdom, 0);
    }
    vanish(sim, m, i);
    Ok(())
}
/// 0x506c30 (vt+0x2a0) for kingdom `k`, effects aside: as the object's
/// control byte says (+0xb0), else when it hides a spy of `k` (vt+0xbc),
/// or its owner or the spy's kingdom shares its units with `k` (relation
/// 4).
pub(crate) fn commanded_by(m: &MissionWorld, i: usize, k: u8) -> bool {
    let x = &m.objects[i];
    match x.control.get(usize::from(k).wrapping_sub(1)) {
        Some(&hold) if hold != 0 => hold == 1,
        _ => {
            (x.mole != 0 && x.mole == k)
                || relation(m, x.owner, k) & 4 != 0
                || (x.mole != 0 && relation(m, x.mole, k) & 4 != 0)
        }
    }
}
/// 0x506d00 (vt+0x2a4): kingdom `k`'s hold on object `i` (+0xb0). Whatever
/// changed, an object whose kingdom does not share its units with the
/// player leaves a selection of several (0x467d10 with 4, 0x4532f0).
fn set_control(sim: &mut Simulation, m: &mut MissionWorld, i: usize, k: u8, hold: u8) {
    if let Some(c) = m.objects[i].control.get_mut(usize::from(k).wrapping_sub(1)) {
        *c = hold;
    }
    let selected = &mut sim.gameplay.selected;
    if !m.controls(m.objects[i].owner) && selected.len() > 1 && selected.remove(&i) {
        let outbox = &mut sim.gameplay.script.outbox;
        if outbox.len() < 1024 {
            outbox.push(Show::Unselect(i));
        }
    }
}
/// The time between two idle ticks of a unit (vt+0x320).
fn idle_period(o: &MapObject) -> u64 {
    if matches!(o.class, class::NINJA | 14) {
        2000
    } else {
        1500
    }
}
/// A ninja's or a spy's idle tick (0x4eb730, 0x4ec280; the ninja's also on
/// guard): every enemy ninja hidden from its kingdom and every unit hiding
/// an enemy spy within its sight (0x44de50 with 0x44ec40, vt+0x220) shows
/// itself (program 0x30, op 0x61).
fn unmask(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary, c: &Catalog, k: usize) {
    let owner = m.objects[k].owner;
    let sight = sim.objects[k].sight as u32;
    let found: Vec<usize> = (0..m.objects.len())
        .filter(|&j| {
            let x = &m.objects[j];
            j != k
                && alive(x)
                && sim.objects[j].holder.is_none()
                && ((x.class == class::NINJA
                    && sim.gameplay.actors[j].effect_until > sim.ticks
                    && relation(m, owner, x.owner) & 0x40 == 0)
                    || (x.mole != 0 && x.mole != x.owner && relation(m, owner, x.mole) & 0x40 == 0))
                && movement::in_range(sim, m, lib, k, j, sight)
        })
        .collect();
    for j in found {
        let _ = reveal_self(sim, m, lib, c, j);
    }
}
/// The default program (0x4e4c10) of a unit left without one: a fighter
/// attacks at once a target it already reaches (vt+0x224); otherwise it
/// stands idle (Idle op 0x4e1b93), its idle time from zero and its idle
/// ticks 1500 ms apart. Cows and bears have their own (`pasture`).
fn idle(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, c: &Catalog, i: usize) {
    let now = sim.ticks * TICK_MS;
    if matches!(m.objects[i].class, class::COW | 26) {
        pasture(sim, m, lib, i, now);
        return;
    }
    let a = &sim.gameplay.actors[i];
    if military(sim, m, i)
        && a.stance != 2
        && a.effect_until <= sim.ticks
        && let Some(t) = find_target(sim, m, lib, c, i)
        && combat::in_reach(sim, m, lib, c, i, t)
    {
        let intent = program_against(sim, m, lib, c, i, t);
        let a = &mut sim.gameplay.actors[i];
        a.intent = Some(intent);
        a.automatic = true;
        return;
    }
    // 0x4e1bd3: the Idle keeps whether the unit could move (+0xd6) and,
    // after a program of priority 4000 (an order), holds the place of a
    // human kingdom's unit or of one of the story (record flag 0x2000);
    // from the third idle tick it moves again if it could before
    // (0x4e4ebb). A unit held when an engagement interrupts that Idle
    // stays held at the next one, until an order.
    let human = !ai_managed(sim, m, i);
    let a = &mut sim.gameplay.actors[i];
    a.ordered_idle = false;
    if a.stance <= 1 {
        a.temporary_hold = a.stance == 0;
        if human && a.last_priority >= 4000 {
            a.stance = 1;
        }
    }
    a.idle_ms = 0;
    a.scan_due_ms = Some(now + 1500);
    a.idle_since_ms = now;
}
/// Cow Idle (0x42f450) and Bear Idle (0x42fea0), the default programs of
/// the animals (vt+0x178 = 0x4ed270, 0x4eee70): Idle, then Wander, then
/// Move, round and round, with no look for a target first. The program is
/// made where the animal stands; back from its own walk it keeps it, and
/// any order makes a new one. The bear's Idle is the units' (image_idle,
/// ticks 1500 ms apart that look around); the cow's is its grazing.
fn pasture(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, i: usize, now: u64) {
    let o = &m.objects[i];
    let a = &mut sim.gameplay.actors[i];
    let home = match a.pasture {
        Some(p) if p.wandering => p.home,
        _ => [o.x, o.y],
    };
    a.idle_ms = 0;
    a.idle_since_ms = now;
    a.pasture = Some(Pasture {
        home,
        ..Default::default()
    });
    if o.class == class::COW {
        // 0x4ed120: 30 cycles, cow_idle from its first frame.
        a.scan_due_ms = Some(u64::MAX);
        if let Some(p) = a.pasture.as_mut() {
            p.left = 30;
            show(p, lib, 0, false, now);
        }
    } else {
        a.scan_due_ms = Some(now + 1500);
    }
}
/// The grazing picture an idle cow shows (`GRAZE`), whether it plays
/// backwards from its last frame, and since when (game ms).
pub fn grazing(
    sim: &Simulation,
    object_class: usize,
    i: usize,
) -> Option<(&'static str, bool, u64)> {
    let a = sim.gameplay.actors.get(i)?;
    let p = a
        .pasture
        .filter(|_| a.intent.is_none() && object_class == class::COW)?;
    Some((GRAZE[p.picture as usize], p.backwards, p.since_ms))
}
/// The cow shows one of its grazing pictures from now; the next tick of
/// its Idle op comes when the picture has played (0x4ed163).
fn show(p: &mut Pasture, lib: &ObjectLibrary, picture: u8, backwards: bool, now: u64) {
    p.picture = picture;
    p.backwards = backwards;
    p.since_ms = now;
    p.due_ms = now + section_ms(lib, GRAZE[picture as usize]);
}
/// The steps of an animal's program that are not the bear's wait: the
/// Wander op and the cow's grazing.
fn animal(sim: &mut Simulation, m: &MissionWorld, g: &MapGrid, lib: &ObjectLibrary, i: usize) {
    let now = sim.ticks * TICK_MS;
    let a = &sim.gameplay.actors[i];
    let Some(p) = a.pasture else {
        return;
    };
    if a.intent.is_some() || !alive(&m.objects[i]) || sim.objects[i].holder.is_some() {
        return;
    }
    if let Some(due) = p.wander_due {
        if now >= due {
            wander(sim, m, g, lib, i, now);
        }
    } else if m.objects[i].class == class::COW && now >= p.due_ms {
        graze(sim, m, g, lib, i, now);
    }
}
/// The cow's Idle op (0x4ecb13): while cycles are left each tick draws
/// R(0, 2); on 0 a standing cow lowers its head (cow_down forward, then
/// cow_eat) and an eating one raises it (cow_down backwards, then
/// cow_idle). With no cycle left a standing cow nobody milks goes to
/// wander, as does a frozen or petrified one; otherwise it first raises its
/// head and, if milked, grazes 4 cycles more.
fn graze(
    sim: &mut Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    now: u64,
) {
    let milked = milked(sim, i);
    let still = has_effect(sim, i, effect::FREEZE) || has_effect(sim, i, effect::STONE);
    let Some(mut p) = sim.gameplay.actors[i].pasture else {
        return;
    };
    let mut roam = false;
    match p.step {
        0 if p.left == 0 => {
            if still || (p.picture == 0 && !milked) {
                roam = true;
            } else {
                p.step = 3;
                show(&mut p, lib, 1, true, now);
            }
        }
        0 => {
            p.left -= 1;
            if sim.gameplay.game_rng.range(0, 2) != 0 {
                p.due_ms = now + section_ms(lib, GRAZE[p.picture as usize]);
            } else if p.picture == 0 {
                p.step = 1;
                show(&mut p, lib, 1, false, now);
            } else {
                p.step = 2;
                show(&mut p, lib, 1, true, now);
            }
        }
        1 => {
            p.step = 0;
            show(&mut p, lib, 2, false, now);
        }
        2 => {
            p.step = 0;
            show(&mut p, lib, 0, false, now);
        }
        _ if milked => {
            p.step = 0;
            p.left = 4;
            show(&mut p, lib, 0, false, now);
        }
        _ => roam = true,
    }
    if roam {
        // The Idle op ends; Wander starts at once and shows image_idle
        // (0x4e2ec4).
        show(&mut p, lib, 0, false, now);
        p.wander_due = Some(now);
    }
    sim.gameplay.actors[i].pasture = Some(p);
    if roam {
        wander(sim, m, g, lib, i, now);
    }
}
/// Wander (op 0x42, 0x4e0a23): a cell drawn R(lo, hi) for x, then for y,
/// within the radius of the centre (a cow's home and 2, a bear's own cell
/// and 6) and inside the map. A cell the animal cannot step into now
/// (vt+0x90) is drawn again 200 ms later. Then the program's Move; the
/// bear's (argument 2) looks around as it goes.
fn wander(
    sim: &mut Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    now: u64,
) {
    let o = &m.objects[i];
    let Some(p) = sim.gameplay.actors[i].pasture else {
        return;
    };
    let (centre, radius) = if o.class == class::COW {
        (p.home, 2)
    } else {
        ([o.x, o.y], 6)
    };
    let last = g.side as i32 - 1;
    let x = sim.gameplay.game_rng.range(
        (centre[0] as i32 - radius).max(0),
        (centre[0] as i32 + radius).min(last),
    );
    let y = sim.gameplay.game_rng.range(
        (centre[1] as i32 - radius).max(0),
        (centre[1] as i32 + radius).min(last),
    );
    let cell = [x as u16, y as u16];
    let free = movement::can_step(sim, m, g, lib, i, cell);
    let a = &mut sim.gameplay.actors[i];
    let Some(p) = a.pasture.as_mut() else {
        return;
    };
    if !free {
        p.wander_due = Some(now + 200);
        return;
    }
    p.wander_due = None;
    p.wandering = true;
    a.intent = Some(Intent::Move(cell));
    a.move_mode = u8::from(o.class == 26);
    a.scan_due_ms = None;
}
/// The idle scan (0x4e4e90). Stop's hold ends once the unit has been idle
/// 3000 ms (+0xe2, tested before it grows); a guard outside the guarded
/// object's rect grown by 1 + 3 walks back to it (0x4e4f1f); otherwise the
/// best target (vt+0x2d0) is attacked at priority 3000 and the kingdom is
/// called (0x4e5075). Hold steady never scans, nor does an invisible unit.
fn scan(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    k: usize,
) -> bool {
    let a = &mut sim.gameplay.actors[k];
    if a.temporary_hold && a.idle_ms >= 3000 {
        a.temporary_hold = false;
        a.stance = 0;
    }
    if m.objects[k].class == class::PEON {
        if ai_managed(sim, m, k) {
            return true;
        }
        take_up_work(sim, m, k);
        return false;
    }
    // 0x4e508b: a computer's unit that does not fight goes to its player's
    // idle (0x4140b0), where a fishing boat looks for a patch (0x415730).
    if m.objects[k].class == class::FISHING_BOAT {
        return ai_managed(sim, m, k) && m.objects[k].flags & FLAG_DYING == 0;
    }
    if a.stance == 2 || !military(sim, m, k) || sim.objects[k].holder.is_some() {
        return false;
    }
    if sim.gameplay.actors[k].guarding.is_some() {
        // 0x4dd860: a guarded object that dies leaves its guards' lists.
        let a = &mut sim.gameplay.actors[k];
        if a.guard_list.is_empty() {
            a.guard_list.extend(a.guarding);
        }
        a.guard_list
            .retain(|&j| m.objects.get(j).is_some_and(alive));
        a.guarding = a.guard_list.first().copied();
    }
    if let Some(j) = sim.gameplay.actors[k].guarding {
        {
            let r = movement::target_rect(sim, m, lib, j);
            let o = &m.objects[k];
            let (x, y) = (o.x as i32, o.y as i32);
            if x < r[0] - 3 || x > r[2] + 3 || y < r[1] - 3 || y > r[3] + 3 {
                let a = &mut sim.gameplay.actors[k];
                a.temporary_hold = false;
                a.stance = 0;
                a.intent = Some(Intent::Guard(j));
                halt(a);
                return false;
            }
        }
    }
    if sim.gameplay.actors[k].effect_until > sim.ticks {
        return false;
    }
    let computer = ai_managed(sim, m, k) && m.objects[k].flags & FLAG_DYING == 0;
    let Some(t) = find_target(sim, m, lib, c, k) else {
        // 0x4e4fc9: nobody to fight, the computer's unit goes to its
        // player's idle (0x4140b0).
        return computer;
    };
    // 0x4e4fd5: for 3000 ms after a program above priority 1000 the
    // computer's unit takes on only what it already reaches.
    let a = &sim.gameplay.actors[k];
    if computer
        && a.last_priority > 1000
        && a.idle_ms < 3000
        && !movement::in_range(sim, m, lib, k, t, combat::max_range(m, c, k))
    {
        return false;
    }
    let intent = program_against(sim, m, lib, c, k, t);
    let a = &mut sim.gameplay.actors[k];
    give_way(a);
    a.intent = Some(intent);
    a.automatic = true;
    halt(a);
    a.action_image.clear();
    call_for_help(sim, m, lib, c, k, t);
    false
}
/// The picture of the Murder op (0x4e197d): `image_murder` of the record
/// (+0x50), the attack's for a class without one.
fn murder_image(lib: &ObjectLibrary, o: &MapObject) -> &'static str {
    if lib.property(o, "image_murder").is_some() {
        "image_murder"
    } else {
        "image_attack"
    }
}
/// The ninja's Murder op (0x5b, 0x4e076a): it shows itself, spends all its
/// mana and strikes once with fifteen times its attack.
fn murder(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    j: usize,
) {
    sim.objects[i].mana = 0;
    let attack = sim
        .effective_attributes(i, m)
        .map_or(sim.objects[i].attack, |s| s.attack);
    let a = &mut sim.gameplay.actors[i];
    a.effect_until = 0;
    a.action_image = "image_attack".into();
    a.action_elapsed_ms = 0;
    sim.objects[i].order = OrderState::Attacking;
    sound(sim, i, "sound_attack", crate::audio::category::BLOW);
    let amount = (attack.min(0xffff) * 15).min(u16::MAX as u32) as u16;
    apply_damage(sim, m, g, lib, c, j, amount, 0, Some(i));
}
/// The peasant's own idle tick (vt+0x1a4 = 0x4ae440): idle for 30 s, a
/// peasant of a human kingdom takes up the work of the nearest peasant of
/// its kingdom within twice its sight that chops, mines, captures, builds,
/// repairs, gathers resource items or tends cows (filter 0x44fa30; not one
/// digging a field), with a copy of its program (0x42db10).
fn take_up_work(sim: &mut Simulation, m: &MissionWorld, k: usize) {
    let o = &m.objects[k];
    if sim.gameplay.actors[k].idle_ms < 30_000
        || sim.objects[k].holder.is_some()
        || ai_managed(sim, m, k)
    {
        return;
    }
    let reach = 2 * sim.objects[k].sight as u16;
    let job = |intent: &Intent| match intent {
        Intent::Chop(_) | Intent::Capture(_) | Intent::Build(_) | Intent::Repair(_) => true,
        Intent::Harvest(j) => m.objects.get(*j).is_some_and(|t| {
            matches!(
                t.class,
                class::RES_GOLD | class::RES_STONE | class::RESITEM | class::COW
            )
        }),
        _ => false,
    };
    let near = (0..m.objects.len())
        .filter(|&j| {
            let x = &m.objects[j];
            j != k
                && x.class == class::PEON
                && x.owner == o.owner
                && alive(x)
                && sim.objects[j].holder.is_none()
                && x.x.abs_diff(o.x) <= reach
                && x.y.abs_diff(o.y) <= reach
                && sim.gameplay.actors[j].intent.as_ref().is_some_and(job)
        })
        .min_by_key(|&j| {
            let x = &m.objects[j];
            x.x.abs_diff(o.x).max(x.y.abs_diff(o.y))
        });
    if let Some(j) = near
        && let Some(intent) = sim.gameplay.actors[j].intent.clone()
    {
        let _ = set_order(sim, k, intent, false);
    }
}
/// vt+0x2b0: the program a unit takes against a target, an attack; a
/// computer player's unit has its own (0x4ae580, 0x4ebeb0, 0x4dd8c0).
fn program_against(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    u: usize,
    t: usize,
) -> Intent {
    let o = &m.objects[u];
    if ai_managed(sim, m, u) && o.flags & FLAG_DYING == 0 {
        ai::units::target_program(sim, m, lib, c, u, t)
    } else {
        Intent::Attack(t)
    }
}
/// 0x413800: the unit attacks at priority 3000. A human kingdom's unit
/// keeps what it was doing to take it up afterwards (+0xf2), unless a plain
/// Move is already kept; the attack type stays.
fn engage(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    unit: usize,
    target: usize,
) {
    let human = !ai_managed(sim, m, unit);
    let a = &mut sim.gameplay.actors[unit];
    give_way(a);
    let current = a.intent.take();
    if human && !matches!(a.resume, Some(Intent::Move(_))) {
        a.resume = current;
    }
    a.automatic = true;
    a.intent = None;
    a.search_at = None;
    halt(a);
    a.action_image.clear();
    sim.gameplay.actors[unit].intent = Some(program_against(sim, m, lib, c, unit, target));
}
/// Retaliation of a fighter or tower (0x413410) against `attacker`: a
/// building within its range band, a unit outside its minimum range and,
/// when it keeps its place, within RANGE. With nothing to do, or when its
/// target fell, a unit takes its own scan's best target instead when that
/// one ranks higher (0x412bf0); moving in advance or sneak mode it turns at
/// once; attacking at priority 3000 or less, or the dummy, it changes only
/// for a better target. Whether it took the attacker on.
fn retaliate(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    unit: usize,
    attacker: usize,
) -> bool {
    let (Some(o), Some(x)) = (m.objects.get(unit), m.objects.get(attacker)) else {
        return false;
    };
    if !standing(m, unit)
        || !alive(x)
        || !military(sim, m, unit)
        || sim.objects[unit].holder.is_some()
        || sim.gameplay.actors[unit].frozen_until > sim.ticks
        || sim.gameplay.actors[unit].stance == 2
        || !combat::can_attack(sim, m, lib, c, unit, attacker, true)
    {
        return false;
    }
    let gap = combat::object_gap(lib, o, x);
    let min = combat::min_range(lib, o);
    let max = combat::max_range(m, c, unit);
    let building = !crate::selection::unit(o);
    if building {
        if sim.gameplay.actors[unit].hold_fire || gap < min || gap > max {
            return false;
        }
    } else if gap < min {
        return false;
    }
    if !combat::free(sim, m, unit) && gap > max {
        return false;
    }
    let a = &sim.gameplay.actors[unit];
    let (intent, move_mode, automatic) = (a.intent.clone(), a.move_mode, a.automatic);
    // Idle, or the target fell: the better of the attacker and the scan's
    // best (0x41355e, 0x4136b1).
    let choose = |sim: &mut Simulation| -> bool {
        if building {
            engage(sim, m, lib, c, unit, attacker);
            return true;
        }
        match find_target(sim, m, lib, c, unit) {
            Some(best) if best != attacker => {
                let hooks = combat::Hooks::new(sim, m);
                if combat::better(sim, m, lib, c, &hooks, unit, Some(attacker), best) {
                    let intent = program_against(sim, m, lib, c, unit, best);
                    let a = &mut sim.gameplay.actors[unit];
                    a.intent = Some(intent);
                    a.automatic = true;
                    a.search_at = None;
                    halt(a);
                    a.action_image.clear();
                    false
                } else {
                    engage(sim, m, lib, c, unit, attacker);
                    true
                }
            }
            _ => {
                engage(sim, m, lib, c, unit, attacker);
                true
            }
        }
    };
    match intent {
        None => choose(sim),
        Some(Intent::Move(_)) if move_mode != 0 => {
            engage(sim, m, lib, c, unit, attacker);
            true
        }
        Some(Intent::Approach(ap)) if ap.step == 4 => {
            engage(sim, m, lib, c, unit, attacker);
            true
        }
        Some(Intent::Patrol(..) | Intent::Circuit(..)) => {
            engage(sim, m, lib, c, unit, attacker);
            true
        }
        Some(Intent::Attack(t)) if standing(m, t) => {
            if t == attacker || (m.objects[t].class != class::DUMMY && !automatic) {
                return false;
            }
            let hooks = combat::Hooks::new(sim, m);
            if combat::better(sim, m, lib, c, &hooks, unit, Some(t), attacker) {
                engage(sim, m, lib, c, unit, attacker);
                true
            } else {
                false
            }
        }
        Some(Intent::Attack(_)) => choose(sim),
        // 0x41373d: a computer player's mage casting (program 0x12) goes on
        // at the spell op while its mana and 50 reach the cost; on its way
        // there it takes the attacker on.
        Some(Intent::Cast { key, cell, target })
            if o.class == class::MAGE
                && !matches!(key.as_str(), "spell_heal" | "spell_raiseexp")
                && ai_managed(sim, m, unit)
                && o.flags & FLAG_DYING == 0 =>
        {
            let p = target
                .and_then(|j| m.objects.get(j))
                .map_or(cell, |x| [x.x, x.y]);
            let (range, cost) = spell_parameters(c, m, o, &key);
            let casting = o.x.abs_diff(p[0]).max(o.y.abs_diff(p[1])) as u32 <= range;
            if casting && sim.objects[unit].mana as u32 + 50 >= cost as u32 {
                return false;
            }
            engage(sim, m, lib, c, unit, attacker);
            true
        }
        _ => false,
    }
}
/// Call for help (0x413950), at most every 100 ms per object: the fighters
/// and towers of the victim's kingdom (relation 4) whose cells meet the
/// square of 3 cells (6 for a computer kingdom) around its corner, and its
/// guardians wherever they are, retaliate (0x4138e0) unless frozen, stone
/// or invisible.
fn call_for_help(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    victim: usize,
    attacker: usize,
) {
    if sim.ticks < sim.gameplay.actors[victim].alarm_tick + 2
        && sim.gameplay.actors[victim].alarm_tick != 0
    {
        return;
    }
    sim.gameplay.actors[victim].alarm_tick = sim.ticks;
    let v = &m.objects[victim];
    let owner = v.owner;
    let r = if ai_managed(sim, m, victim) { 6 } else { 3 };
    let area = [
        v.x as i32 - r,
        v.y as i32 - r,
        v.x as i32 + r,
        v.y as i32 + r,
    ];
    let helper = |sim: &Simulation, h: usize| -> bool {
        let x = &m.objects[h];
        h != attacker
            && standing(m, h)
            && (crate::selection::unit(x) || x.building())
            && military(sim, m, h)
            && relation(m, x.owner, owner) & 4 != 0
            && !has_effect(sim, h, effect::FREEZE)
            && !has_effect(sim, h, effect::STONE)
            && sim.gameplay.actors[h].effect_until <= sim.ticks
    };
    let mut helpers: Vec<usize> = (0..m.objects.len())
        .filter(|&h| {
            let f = movement::footprint_rect(lib, &m.objects[h]);
            f[0] <= area[2]
                && f[2] >= area[0]
                && f[1] <= area[3]
                && f[3] >= area[1]
                && helper(sim, h)
        })
        .collect();
    let guards: Vec<usize> = (0..m.objects.len())
        .filter(|&h| guarded(&sim.gameplay.actors[h]).any(|j| j == victim))
        .collect();
    let guarded = !guards.is_empty();
    helpers.extend(guards.into_iter().filter(|&h| helper(sim, h)));
    let mut answered = false;
    for h in helpers {
        answered |= retaliate(sim, m, lib, c, h, attacker);
    }
    // 0x413a99: with no guards and nobody answering, a computer player's
    // victim calls its militia against the attacker.
    if !answered
        && !guarded
        && ai_managed(sim, m, victim)
        && m.objects[victim].flags & FLAG_DYING == 0
    {
        sim.gameplay.ai.alarms.push((owner, attacker));
    }
}
/// Native 0x4ae640, the peasant's own reaction: hit while idle or at work
/// it fights back when hostile to the attacker and at least as strong
/// (0x401fa0), otherwise it runs away (0x413ae0); either way it returns to
/// its work afterwards.
fn react(
    sim: &mut Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    victim: usize,
    attacker: usize,
) {
    let (Some(o), Some(x)) = (m.objects.get(victim), m.objects.get(attacker)) else {
        return;
    };
    if o.class != class::PEON
        || !standing(m, victim)
        || sim.objects[victim].holder.is_some()
        || sim.gameplay.actors[victim].frozen_until > sim.ticks
        || sim.gameplay.actors[victim].automatic
    {
        return;
    }
    let a = &sim.gameplay.actors[victim];
    if !matches!(a.intent, None | Some(Intent::Chop(_) | Intent::Harvest(_))) {
        return;
    }
    if hostile(sim, m, victim, attacker)
        && strength(sim, m, c, victim) >= strength(sim, m, c, attacker)
    {
        engage(sim, m, lib, c, victim, attacker);
        let a = &mut sim.gameplay.actors[victim];
        a.work_target = None;
        a.work_cell = None;
        return;
    }
    run_away(sim, m, g, lib, c, victim, [x.x, x.y], PEASANT_REST_MS);
}
/// The point of a Runaway (0x430be0): five cells straight away from
/// `from`, a random way when on its cell, inside a map of `side` cells.
pub(crate) fn flight_point(
    sim: &mut Simulation,
    side: usize,
    at: [u16; 2],
    from: [u16; 2],
) -> [u16; 2] {
    let (mut s, mut t) = (from[0] as i32 - at[0] as i32, from[1] as i32 - at[1] as i32);
    let mut n = s.abs().max(t.abs());
    if n == 0 {
        // Table 0x430d7c: its sixth and eighth ways are the same.
        const WAYS: [(i32, i32); 8] = [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (-1, 1),
        ];
        (s, t) = WAYS[random(sim, 8) as usize];
        n = 1;
    }
    let side = side.max(1) as i32 - 1;
    [
        (at[0] as i32 - s * 5 / n).clamp(0, side) as u16,
        (at[1] as i32 - t * 5 / n).clamp(0, side) as u16,
    ]
}
/// The rest of a peasant after its flight (0x4ae6fa).
const PEASANT_REST_MS: u32 = 3000;
/// 0x413ae0: unit `victim` runs away from `from` (Runaway, program 0x3f,
/// 0x430be0): five cells straight away, a random way when on its cell; then
/// it rests `rest` ms. A computer player's unit keeps no work to take up
/// again; wounded, it goes into a refuge nearer than 12 cells instead.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_away(
    sim: &mut Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    victim: usize,
    from: [u16; 2],
    rest: u32,
) {
    let o = &m.objects[victim];
    let computer = ai_managed(sim, m, victim) && o.flags & FLAG_DYING == 0;
    let refuge = (computer && o.health < o.max_health)
        .then(|| sim.gameplay.ai.players.get(&o.owner))
        .flatten()
        .and_then(|p| ai::idle::refuge(sim, m, lib, c, p, victim))
        .filter(|&r| {
            let own = |j: usize| movement::footprint_rect(lib, &m.objects[j]);
            combat::gap(own(victim), own(r)) < 12
        });
    let flight = match refuge {
        Some(r) => Intent::Board(r),
        None => Intent::Flee(flight_point(sim, g.side, [o.x, o.y], from), rest),
    };
    let a = &mut sim.gameplay.actors[victim];
    give_way(a);
    let current = a.intent.take();
    a.resume = if computer { None } else { current };
    a.automatic = true;
    a.intent = Some(flight);
    a.search_at = None;
    halt(a);
    a.action_image.clear();
    a.work_target = None;
    a.work_cell = None;
}
/// Native 0x504a3f: armour uses A²/(A+D), except magic category 3.
pub fn physical_damage(attack: u16, defence: u32, category: u8) -> u16 {
    if attack == 0 {
        return 0;
    }
    if category == 3 {
        return attack;
    }
    let attack = attack as u64;
    (attack * attack / (attack + defence as u64)).max(1) as u16
}
/// What an object takes from a blow (vt+0x1d0). A script's god takes
/// nothing (0x4df746) and the messiah no magic (0x4ef1d0). Honor between
/// heroes (0x4df740) and the attacker's BONUS.INI entry against this object
/// (0x4dd960) grow the blow before the damage itself (0x504900). A hostile
/// attacker then meets the victim's retaliation, if it still stands, and
/// its call for help (0x4dda89); a pikeman that survives a melee blow hits
/// back with a fifth of its attack (0x4ea0b0), a peasant fights or flees
/// (0x4ae640).
#[allow(clippy::too_many_arguments)]
fn apply_damage(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    j: usize,
    amount: u16,
    category: u8,
    from: Option<usize>,
) {
    let o = &m.objects[j];
    // The magic gateway and the ruins receive nothing (vt+0x1d0 = 0x4b6740).
    if o.flags & FLAG_GOD != 0
        || (o.class == class::MESSIAH && category == 3)
        || matches!(o.class, class::MAGIC_GATEWAY | class::RUIN)
    {
        return;
    }
    // The attacker while its record exists, even dying.
    let source = from.filter(|&a| {
        m.objects
            .get(a)
            .is_some_and(|x| x.flags & FLAG_INACTIVE == 0)
    });
    let mut amount = amount;
    if let Some(a) = source
        && crate::selection::unit(&m.objects[j])
        && hero(&m.objects[a])
        && hero(&m.objects[j])
    {
        let bonus = experience_rules(m, m.objects[a].owner).honor as u32;
        amount = (amount as u32 + amount as u32 * bonus / 100) as u16;
    }
    if let Some(a) = source {
        amount = c.received(&m.objects[a], &m.objects[j], amount);
    }
    event(sim, m, lib, 1, j, j);
    // 0x422a80: a hit castle or outpost heals its garrison again.
    if matches!(m.objects[j].class, class::CASTLE | 52) && !sim.objects[j].contents.is_empty() {
        let now = sim.ticks * TICK_MS;
        sim.gameplay.actors[j].heal_due_ms.get_or_insert(now + 1000);
    }
    let standing = wound(sim, m, g, lib, c, j, amount, category, source);
    burn(sim, m, lib, j);
    let Some(a) = source else {
        return;
    };
    if auto_hostile(sim, m, j, a) {
        if standing {
            retaliate(sim, m, lib, c, j, a);
        }
        call_for_help(sim, m, lib, c, j, a);
    }
    if m.objects[j].class == class::PIKEMAN
        && standing
        && category == 0
        && m.objects[a].class != class::PIKEMAN
    {
        let attack = sim
            .effective_attributes(j, m)
            .map_or(sim.objects[j].attack, |s| s.attack);
        let back = ((attack & 0xffff) * 20 / 100).max(1) as u16;
        apply_damage(sim, m, g, lib, c, a, back, 0, Some(j));
    }
    react(sim, m, g, lib, c, j, a);
}
/// 0x504900: armour A²/(A+D) except for magic, the share of a holder's
/// occupants (`[<civ> damage]`, then `[damage]`) and the loss of health;
/// whether the object still stands.
#[allow(clippy::too_many_arguments)]
fn wound(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    j: usize,
    amount: u16,
    category: u8,
    source: Option<usize>,
) -> bool {
    if !alive(&m.objects[j])
        || m.objects[j].flags & FLAG_DYING != 0
        || amount == 0
        || has_effect(sim, j, effect::STONE)
    {
        return false;
    }
    let before = m.objects[j].health;
    let def = sim
        .effective_attributes(j, m)
        .map_or(sim.objects[j].defence, |s| s.defence);
    // 0x504990: an easy campaign spares the player's side a fifth of each
    // blow from a stranger and adds a fifth to the player's own.
    let amount = match source {
        Some(a)
            if m.campaign
                && m.game_rules.difficulty == 0
                && relation(m, m.objects[j].owner, m.objects[a].owner) & 0x40 == 0 =>
        {
            let player = m.player();
            if relation(m, player, m.objects[j].owner) & 0x40 != 0 {
                ((amount as u32 * 80 / 100) as u16).max(1)
            } else if relation(m, player, m.objects[a].owner) & 0x40 != 0 {
                (amount as u32 * 120 / 100) as u16
            } else {
                amount
            }
        }
        _ => amount,
    };
    let damage = physical_damage(amount, def, category) as u32;
    let key = format!("{category:02}");
    let civ = format!("{} damage", world::civ_section(m.objects[j].civilization));
    let fractions: Vec<u32> = lib
        .objects
        .get(m.objects[j].class_name())
        .and_then(|ini| ini.get(&civ, &key).or_else(|| ini.get("damage", &key)))
        .unwrap_or("100,0")
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let taken = (damage * fractions.first().copied().unwrap_or(100) / 100) as u16 as i16;
    let distributed = (damage * fractions.get(1).copied().unwrap_or(0) / 100) as u16 as i16;
    let contents = sim.objects[j].contents.clone();
    if distributed != 0 && !contents.is_empty() {
        let each = match (distributed as i32 / contents.len() as i32) as u16 {
            0 => 1,
            n => n,
        };
        for k in contents {
            apply_damage(sim, m, g, lib, c, k, each, category, source);
        }
    }
    let attacker = source.filter(|&a| alive(&m.objects[a]));
    if (taken as i32) < m.objects[j].health as i32 {
        m.objects[j].health = m.objects[j].health.wrapping_sub(taken as u16);
        // The training dummy (0x4481e0): one point to the attacking unit for
        // every 75 hit points it loses.
        if m.objects[j].class == class::DUMMY
            && let Some(a) = attacker
            && (before as i32 - 1).div_euclid(75) != (m.objects[j].health as i32 - 1).div_euclid(75)
        {
            add_experience(sim, m, lib, c, a, 1, false);
        }
        return true;
    }
    // 0x504b5e: in the campaign Sartor's blows never kill Vardal.
    if m.campaign
        && source.is_some_and(|a| m.objects[a].owner == 1 && m.objects[a].name == "Sartor")
        && m.objects[j].owner == 8
        && m.objects[j].name == "Vardal"
    {
        return true;
    }
    m.objects[j].health = 0;
    // 0x504f20: one point for the kill, unless the victim was an ally.
    if let Some(a) = attacker
        && !allied(m, m.objects[j].owner, m.objects[a].owner)
    {
        add_experience(sim, m, lib, c, a, 1, true);
    }
    // 0x504cbe: the killer's kingdom scores 500 for a building, 200 for
    // anything else.
    if let Some(a) = source {
        let points = if m.objects[j].building() { 500 } else { 200 };
        add_score(sim, m.objects[a].owner, points);
    }
    // 0x4e22a9, 0x41db7b: the computer players' places count the loss.
    if let Some(a) = attacker {
        ai::places::died(sim, m, lib, c, j, a);
    }
    refund_site(sim, m, c, j);
    kill(sim, m, g, lib, j);
    false
}
/// The castle takes every resource; table 0x4adff0 sends gold and stone to
/// the storage, food to the farm, wood to the lumber mill; ships unload in harbours.
fn store_accepts(o: &MapObject, cargo: [u32; 4], ship: bool) -> bool {
    match o.class_name() {
        "castle" => !ship,
        "storage" => !ship && (cargo[0] > 0 || cargo[1] > 0),
        "lumber" => !ship && cargo[3] > 0,
        "farm" => !ship && cargo[2] > 0,
        "harbor" => ship,
        _ => false,
    }
}
/// Op 0x25, Find field (0x4ad621 → 0x44e4c0 with the filter 0x44f830): a
/// field nobody works within 20 cells of the peasant (Chebyshev to its
/// centre, 0x454d70), for a computer's peasant one of its own kingdom
/// (+0x14e read as the field's owner); a candidate takes the place of the
/// best so far when less grown, or else nearer. The list of fields is read
/// from the last made.
fn find_field(sim: &Simulation, m: &MissionWorld, lib: &ObjectLibrary, i: usize) -> Option<usize> {
    let o = &m.objects[i];
    let computer = ai_managed(sim, m, i) && o.flags & FLAG_DYING == 0;
    let gap = |k: usize| {
        let c = centre(lib, &m.objects[k]);
        o.x.abs_diff(c[0]).max(o.y.abs_diff(c[1]))
    };
    let mut best: Option<usize> = None;
    for k in (0..m.objects.len()).rev() {
        let f = &m.objects[k];
        if f.class != class::FIELD
            || !alive(f)
            || field_worker(sim, k).is_some_and(|w| w != i)
            || (computer && f.owner != o.owner)
            || gap(k) > 20
        {
            continue;
        }
        if best.is_none_or(|b| f.health < m.objects[b].health || gap(k) < gap(b)) {
            best = Some(k);
        }
    }
    best
}
/// The Dig program goes on at op 0x25: another field, or its end.
fn another_field(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, i: usize) {
    match find_field(sim, m, lib, i) {
        Some(k) => {
            let a = &mut sim.gameplay.actors[i];
            a.intent = Some(Intent::Harvest(k));
            a.work_target = None;
            halt(a);
        }
        None => finish(sim, i),
    }
}
/// The peasant digging a field: one at a time (0x44d020).
fn field_worker(sim: &Simulation, field: usize) -> Option<usize> {
    sim.gameplay
        .actors
        .iter()
        .position(|a| a.intent == Some(Intent::Harvest(field)))
}
/// The region map (0x60a630): the cells a walker of `o`'s layer reaches from
/// its own cell over terrain, bridges and around standing buildings and mines;
/// units do not divide regions. Of the `rings` (cells of each candidate, in
/// the order of preference) the first with a cell in the region: the flood
/// stops as soon as the first candidate is reached, else it covers the
/// region and the first candidate it reached wins.
fn first_in_region(
    g: &MapGrid,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    o: &MapObject,
    rings: &[Vec<usize>],
) -> Option<usize> {
    if rings.iter().all(Vec::is_empty) {
        return None;
    }
    let side = g.side;
    let bridges = bridge_cells(g, m, lib);
    let mut solid = vec![false; side * side];
    for x in m.objects.iter().filter(|x| {
        alive(x)
            && x.flags & FLAG_HELD == 0
            && (x.building() || matches!(x.class, class::RES_GOLD | class::RES_STONE))
            && !matches!(
                x.class,
                class::BRIDGE | class::FIELD | class::GATE_STONE | class::GATE_WOOD
            )
    }) {
        let [w, h] = footprint(lib, x);
        for y in x.y as usize..(x.y as usize + h).min(side) {
            for cx in x.x as usize..(x.x as usize + w).min(side) {
                solid[y * side + cx] = true;
            }
        }
    }
    let open = |k: usize| {
        let p = [(k % side) as u16, (k / side) as u16];
        !solid[k] && (terrain_free(g, lib, o, p, false) || (!naval(o) && bridges[k]))
    };
    let mut candidates: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (c, ring) in rings.iter().enumerate() {
        for &k in ring {
            candidates.entry(k).or_default().push(c);
        }
    }
    let mut best: Option<usize> = None;
    let mut reach = |k: usize| {
        for &c in candidates.get(&k).into_iter().flatten() {
            best = Some(best.map_or(c, |b| b.min(c)));
        }
        best == Some(0)
    };
    let mut seen = vec![false; side * side];
    let start = o.y as usize * side + o.x as usize;
    if start >= seen.len() {
        return None;
    }
    seen[start] = true;
    if reach(start) {
        return best;
    }
    let mut queue = VecDeque::from([start]);
    while let Some(k) = queue.pop_front() {
        let (x, y) = ((k % side) as i32, (k / side) as i32);
        for (dx, dy) in [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= side as i32 || ny >= side as i32 {
                continue;
            }
            let n = ny as usize * side + nx as usize;
            if !seen[n] && open(n) {
                seen[n] = true;
                if reach(n) {
                    return best;
                }
                queue.push_back(n);
            }
        }
    }
    best
}
/// Find tree (0x44d670): a flood from the peasant's cell over cells a walker
/// crosses, within twice its sight, neighbours in the native order
/// (0x44ffc0); the first tree reached is chopped next. With none the job ends.
fn next_tree(sim: &mut Simulation, m: &MissionWorld, g: &MapGrid, lib: &ObjectLibrary, i: usize) {
    let o = &m.objects[i];
    let reach = 2 * sim.objects[i].sight as i32;
    let side = g.side as i32;
    let blocked = occupied(g, m, lib, Some(i), false);
    let bridges = bridge_cells(g, m, lib);
    let inside = |x: i32, y: i32| {
        x >= 0
            && y >= 0
            && x < side
            && y < side
            && (x - o.x as i32).abs() <= reach
            && (y - o.y as i32).abs() <= reach
    };
    let mut seen = BTreeSet::from([[o.x, o.y]]);
    let mut queue = VecDeque::from([[o.x, o.y]]);
    let mut found = None;
    'search: while let Some([x, y]) = queue.pop_front() {
        for (dx, dy) in [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if !inside(nx, ny) || !seen.insert([nx as u16, ny as u16]) {
                continue;
            }
            let q = [nx as u16, ny as u16];
            if tree_at(g, lib, q) {
                found = Some(q);
                break 'search;
            }
            let k = ny as usize * g.side + nx as usize;
            if !blocked[k] && (terrain_free(g, lib, o, q, false) || bridges[k]) {
                queue.push_back(q);
            }
        }
    }
    match found {
        Some(q) => {
            let a = &mut sim.gameplay.actors[i];
            a.intent = Some(Intent::Chop(q));
            halt(a);
        }
        None => finish(sim, i),
    }
}
/// Steps 4-7 of the work programs: find a store, walk to it and deliver.
/// Find lumber/farm/storage (0x44e4c0) takes the own completed store nearest
/// by Chebyshev distance to its centre (0x454d70), the earlier one on ties,
/// among those whose surrounding ring lies in the peasant's region (0x469130).
/// With none the program ends with the load kept; a failed walk to the store
/// waits 4 s (0x42dc9f) and searches again.
fn deposit(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
) -> bool {
    let cargo = sim.gameplay.actors[i].cargo;
    if cargo.iter().all(|&n| n == 0) {
        return true;
    }
    if let Some(t) = sim.gameplay.actors[i].search_at {
        if sim.ticks < t {
            sim.objects[i].order = OrderState::Idle;
            return false;
        }
        sim.gameplay.actors[i].search_at = None;
    }
    let owner = m.objects[i].owner;
    let ship = naval(&m.objects[i]);
    let valid = |o: &MapObject| {
        o.owner == owner
            && alive(o)
            && o.flags & FLAG_CONSTRUCTION == 0
            && store_accepts(o, cargo, ship)
    };
    let j = sim.gameplay.actors[i]
        .depositing
        .filter(|&j| m.objects.get(j).is_some_and(valid))
        .or_else(|| {
            let o = &m.objects[i];
            let side = g.side as i32;
            // The stores nearest first, the earlier one on ties.
            let mut stores: Vec<(u32, usize, Vec<usize>)> = m
                .objects
                .iter()
                .enumerate()
                .filter(|(_, s)| valid(s))
                .map(|(j, s)| {
                    let [w, h] = footprint(lib, s);
                    let (x1, y1) = (s.x as i32 - 1, s.y as i32 - 1);
                    let (x2, y2) = (s.x as i32 + w as i32, s.y as i32 + h as i32);
                    let ring = (y1..=y2)
                        .flat_map(|y| (x1..=x2).map(move |x| (x, y)))
                        .filter(|&(x, y)| {
                            (x == x1 || x == x2 || y == y1 || y == y2)
                                && x >= 0
                                && y >= 0
                                && x < side
                                && y < side
                        })
                        .map(|(x, y)| y as usize * g.side + x as usize)
                        .collect();
                    let centre = [s.x as i32 + w as i32 / 2, s.y as i32 + h as i32 / 2];
                    let d = (o.x as i32 - centre[0])
                        .unsigned_abs()
                        .max((o.y as i32 - centre[1]).unsigned_abs());
                    (d, j, ring)
                })
                .collect();
            stores.sort_by_key(|&(d, j, _)| (d, j));
            let rings: Vec<Vec<usize>> = stores
                .iter_mut()
                .map(|s| std::mem::take(&mut s.2))
                .collect();
            first_in_region(g, m, lib, o, &rings).map(|c| stores[c].1)
        });
    let Some(j) = j else {
        // Fail target 0: the program ends and the load stays carried.
        sim.gameplay.actors[i].depositing = None;
        finish(sim, i);
        return false;
    };
    sim.gameplay.actors[i].depositing = Some(j);
    if distance(lib, &m.objects[i], &m.objects[j]) <= 1 {
        // deliverres (0x4ae990): the kingdom gets the load; gathering scores
        // only at the mine (0x4b0076).
        credit(m, owner, cargo);
        let a = &mut sim.gameplay.actors[i];
        a.cargo = [0; 4];
        a.cargo_source = 0;
        a.depositing = None;
        a.goal = None;
    } else if !approach_object(sim, m, g, lib, i, j, 0) && unreachable(sim, i) {
        let a = &mut sim.gameplay.actors[i];
        a.depositing = None;
        a.search_at = Some(sim.ticks + 4000 / TICK_MS);
    }
    false
}
/// The native ring search (0x5073f0) with the mask 0xff of its callers:
/// `rings` rings of 256 random tries, around `start` (a zero coordinate
/// counts as none; its first ring is the cell itself) or else around `rect`
/// (its first ring touches it); each try picks a side, then a place along
/// it, from the game stream, and the first cell `ok` takes wins.
fn ring_search(
    sim: &mut Simulation,
    rect: [i32; 4],
    start: Option<[i32; 2]>,
    rings: i32,
    mut ok: impl FnMut(&Simulation, [i32; 2]) -> bool,
) -> Option<[u16; 2]> {
    let [x1, y1, x2, y2] = rect;
    let (w, h) = (x2 - x1 + 1, y2 - y1 + 1);
    for r in 0..rings {
        for _ in 0..256 {
            let p = match start.filter(|s| s[0] != 0 && s[1] != 0) {
                Some([sx, sy]) => {
                    let along = |sim: &mut Simulation| random(sim, (2 * r + 1) as u64) as i32;
                    match random(sim, 4) {
                        0 => [sx - r, sy - r + along(sim)],
                        1 => [sx - r + along(sim), sy - r],
                        2 => [sx + r, sy - r + along(sim)],
                        _ => [sx - r + along(sim), sy + r],
                    }
                }
                None => match random(sim, 4) {
                    0 => [
                        x1 - r - 1,
                        y1 - r - 1 + random(sim, (h + 2 * r + 2) as u64) as i32,
                    ],
                    1 => [
                        x1 - r - 1 + random(sim, (w + 2 * r + 2) as u64) as i32,
                        y1 - r - 1,
                    ],
                    2 => [
                        x2 + r + 1,
                        y1 - r - 1 + random(sim, (h + 2 * r + 2) as u64) as i32,
                    ],
                    _ => [
                        x1 - r - 1 + random(sim, (w + 2 * r + 2) as u64) as i32,
                        y2 + r + 1,
                    ],
                },
            };
            if ok(sim, p) {
                return Some([p[0] as u16, p[1] as u16]);
            }
        }
    }
    None
}
/// [`ring_search`] for a unit: the cell must lie within the map less its
/// last row and column and take an object of movement type `kind`
/// (0x5072a0). Units leaving a building (0x41f406), passengers stepping off
/// (0x504550).
fn exit_cell(
    sim: &mut Simulation,
    rect: [i32; 4],
    start: Option<[i32; 2]>,
    kind: usize,
    rings: i32,
) -> Option<[u16; 2]> {
    let side = sim.gameplay.pathing.cells.side as i32;
    ring_search(sim, rect, start, rings, |sim, p| {
        p[0] >= 0
            && p[1] >= 0
            && p[0] <= side - 2
            && p[1] <= side - 2
            && sim.gameplay.pathing.cells.spawnable(p, kind)
    })
}
/// 0x5077a0, the place of a large unit of `size` leaving a holder: the edge
/// of `rect` walked clockwise from its top left corner within the map
/// (0x401be0, 0x401c90), the first corner whose cells all take an object of
/// movement type `kind` (0x507730 → 0x5072a0), the rect grown by one cell
/// after each round.
fn large_exit(sim: &Simulation, rect: [i32; 4], kind: usize, size: [usize; 2]) -> Option<[u16; 2]> {
    let cells = &sim.gameplay.pathing.cells;
    let side = cells.side as i32;
    let [w, h] = size.map(|n| n as i32);
    let [mut x1, mut y1, mut x2, mut y2] = rect;
    for _ in 0..side {
        let mut edge = Vec::new();
        edge.extend((x1..=x2).map(|x| [x, y1]));
        edge.extend((y1 + 1..=y2).map(|y| [x2, y]));
        edge.extend((x1..x2).rev().map(|x| [x, y2]));
        edge.extend((y1 + 1..y2).rev().map(|y| [x1, y]));
        for p in edge {
            if p[0] < 0 || p[1] < 0 || p[0] > side - w - 1 || p[1] > side - h - 1 {
                continue;
            }
            if movement::block([p[0] as u16, p[1] as u16], size).all(|c| cells.spawnable(c, kind)) {
                return Some([p[0] as u16, p[1] as u16]);
            }
        }
        (x1, y1, x2, y2) = (x1 - 1, y1 - 1, x2 + 1, y2 + 1);
    }
    None
}
/// Where 0x504550, given no point, lets `j` out of `h`: a holder standing
/// on the map (its sector +0x1e set) around its own rectangle, from the cell
/// `j` came in by while the holder has not moved since (+0x12/+0x14); a
/// holder off the map around its centre.
fn way_out(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    h: usize,
    j: usize,
) -> ([i32; 4], Option<[i32; 2]>) {
    let o = &m.objects[h];
    if sim.objects[h].holder.is_none() {
        let start = sim.objects[j].entry.map(|p| [p[0] as i32, p[1] as i32]);
        (movement::footprint_rect(lib, o), start)
    } else {
        let [w, d] = footprint(lib, o);
        let c = [o.x as i32 + w as i32 / 2, o.y as i32 + d as i32 / 2];
        ([c[0], c[1], c[0], c[1]], None)
    }
}
/// The buildings that answer message 0x2727 with 1 (0x41b460): castle,
/// barracks, farm, church, harbour, workshop and mage tower, those that
/// train units and keep a rally point.
pub fn rally_building(class: usize) -> bool {
    matches!(class, 44 | 45 | 47 | 49 | 54 | 59 | 60)
}
/// A new unit leaves for its building's rally point (0x41f55d): with the
/// default action of a right click there, worked out now (0x3e, 0x490770),
/// or walking, plainly (0x33) or in advance (0x40), to a cell picked at
/// random within two of it.
#[allow(clippy::too_many_arguments)]
fn rally_order(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    building: usize,
    unit: usize,
    rally: [u16; 2],
) {
    let mode = sim.gameplay.actors[building].rally_mode;
    if mode == 1 {
        // What a click on the cell would point at: a unit standing there,
        // else whatever covers it.
        let covers = |k: usize| {
            let x = &m.objects[k];
            let [w, h] = footprint(lib, x);
            k != unit
                && alive(x)
                && x.flags & FLAG_HELD == 0
                && sim.objects[k].holder.is_none()
                && rally[0] >= x.x
                && rally[0] < x.x + w as u16
                && rally[1] >= x.y
                && rally[1] < x.y + h as u16
        };
        let target = (0..m.objects.len())
            .filter(|&k| covers(k))
            .min_by_key(|&k| !mobile(&m.objects[k]));
        let req = Request {
            order: "context".into(),
            selected: vec![unit],
            cell: Some(rally),
            target,
            ..Default::default()
        };
        let _ = issue_as(sim, m, g, lib, c, &req, true);
        return;
    }
    let side = g.side as i64 - 1;
    let dx = random(sim, 5) as i64 - 2;
    let dy = random(sim, 5) as i64 - 2;
    let p = [
        (rally[0] as i64 + dx).clamp(0, side) as u16,
        (rally[1] as i64 + dy).clamp(0, side) as u16,
    ];
    if set_order(sim, unit, Intent::Move(p), false).is_ok() {
        sim.gameplay.actors[unit].move_mode = u8::from(mode == 2);
    }
}
/// An idle building starts the step it was just given now (0x503000).
fn start_step(sim: &mut Simulation, i: usize) {
    let now = sim.ticks * TICK_MS;
    let jobs = &mut sim.gameplay.actors[i].jobs;
    if jobs.len() == 1
        && let Some(job) = jobs.front_mut()
    {
        job.due_ms = Some(now + 50);
    }
}
/// TIME of a job's step, read when the step starts (0x41fc29); a magic
/// item keeps the time paid for.
fn job_time(c: &Catalog, m: &MissionWorld, o: &MapObject, job: &Job) -> u64 {
    let name = match &job.kind {
        JobKind::Train(k) => CLASSES[*k],
        JobKind::Research(n) | JobKind::Upgrade(n, _) => n.as_str(),
        JobKind::Item(..) => return job.total.max(1),
    };
    c.stats(o, name, researches(m, o.owner)).time_ms.max(1)
}
/// The building's queue (message 0x4000, 0x41f0bf): a step starts with its
/// TIME and runs its first tick 50 ms later (0x503290), then one every
/// 666 ms, each adding 666 ms of progress; it completes in the tick that
/// reaches TIME, and the next step starts at once.
fn production(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
) {
    if m.objects[i].flags & FLAG_CONSTRUCTION != 0 {
        return;
    }
    let now = sim.ticks * TICK_MS;
    loop {
        let o = &m.objects[i];
        let Some(job) = sim.gameplay.actors[i].jobs.front() else {
            return;
        };
        let Some(due) = job.due_ms else {
            let total = job_time(c, m, o, job);
            // A step saved by an older version keeps its share of progress.
            let remaining = if job.total > 0 && job.remaining < job.total {
                total * job.remaining / job.total
            } else {
                total
            };
            let job = sim.gameplay.actors[i].jobs.front_mut().unwrap();
            job.total = total;
            job.remaining = remaining;
            job.due_ms = Some(now + 50);
            return;
        };
        if now < due {
            return;
        }
        let instant = m.cheats.instant_build;
        let job = sim.gameplay.actors[i].jobs.front_mut().unwrap();
        if job.remaining > 0 {
            job.remaining = job.remaining.saturating_sub(666);
            // 0x41f113: with «buildozer» the step ends at this tick.
            if job.remaining > 0 && !instant {
                job.due_ms = Some(due + 666);
                return;
            }
        }
        if !complete(sim, m, g, lib, c, i, due) {
            return;
        }
        sim.gameplay.actors[i].jobs.pop_front();
        // The next step starts when this one ends.
        let Some(next) = sim.gameplay.actors[i].jobs.front() else {
            return;
        };
        let total = job_time(c, m, &m.objects[i], next);
        let next = sim.gameplay.actors[i].jobs.front_mut().unwrap();
        next.total = total;
        next.remaining = total;
        next.due_ms = Some(due + 50);
    }
}
/// The end of a step (0x41f300); false when a unit waits for population.
#[allow(clippy::too_many_arguments)]
fn complete(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    due: u64,
) -> bool {
    let job = sim.gameplay.actors[i].jobs.front().unwrap().clone();
    sim.gameplay.actors[i].production_blocked = None;
    let o = m.objects[i].clone();
    match &job.kind {
        JobKind::Train(class) => {
            // 0x41f153: a unit waits while the population is full, cows
            // excepted (attribute 0x2725); the message is shown once and the
            // test repeats every 2000 ms.
            let (pop, housing, cap) = population_parts(m, lib, o.owner);
            if *class != class::COW && pop >= housing.min(cap) {
                let message = c.poplimit[usize::from(cap >= housing)].clone();
                let job = sim.gameplay.actors[i].jobs.front_mut().unwrap();
                job.due_ms = Some(due + 2000);
                if !job.waiting {
                    job.waiting = true;
                    if o.owner == m.player() && sim.gameplay.notices.len() < 16 {
                        sim.gameplay.notices.push(message.clone());
                    }
                }
                sim.gameplay.actors[i].production_blocked = Some(message);
                return false;
            }
            let mut probe = o.clone();
            probe.class = *class;
            let kind = lib.layer(&probe);
            // 0x41f406: around the rally point brought onto the ring of the
            // building, or on the rings around it.
            let rect = movement::footprint_rect(lib, &o);
            let rally = sim.gameplay.actors[i].rally;
            let start = rally.map(|r| {
                [
                    (r[0] as i32).clamp(rect[0] - 1, rect[2] + 1),
                    (r[1] as i32).clamp(rect[1] - 1, rect[3] + 1),
                ]
            });
            let exit = exit_cell(sim, rect, start, kind, 6);
            let p = exit.unwrap_or([o.x, o.y]);
            match new_unit(sim, m, lib, *class, o.owner, o.civilization, p) {
                Ok(j) => {
                    sim.gameplay.pathing.cells.set(
                        [p[0] as i32, p[1] as i32],
                        pathing::OCCUPIED[kind.min(2)],
                        true,
                    );
                    // 0x41f549 → 0x504440: with no free cell the unit waits
                    // inside the building when it has room.
                    if exit.is_none()
                        && sim.objects[i].contents.len() < holder_capacity(sim, m, lib, i)
                    {
                        garrison(sim, m, i, j);
                    }
                    event(sim, m, lib, 3, i, j);
                    if let Some(rally) = rally
                        && sim.objects[j].holder.is_none()
                    {
                        rally_order(sim, m, g, lib, c, i, j, rally);
                    }
                }
                Err(e) => {
                    sim.gameplay.actors[i].production_blocked = Some(e.chars().take(200).collect());
                    return false;
                }
            }
        }
        JobKind::Research(name) => {
            event(sim, m, lib, 4, i, i);
            finish_research(sim, m, lib, o.owner, name);
        }
        JobKind::Upgrade(name, class) => {
            event(sim, m, lib, 5, i, i);
            let class = class.or_else(|| match name.as_str() {
                "to_wtower2" => Some(41),
                "to_stower" => Some(39),
                "to_stower2_1" | "to_stower2_2" => Some(42),
                "to_wall_stone" => Some(61),
                "to_gate_stone" => Some(68),
                _ => None,
            });
            if let Some(row) = sim.gameplay.wall_rows.remove(&i) {
                // 0x4fbca0 and the gate's 0x457850, along the row (0x4fa380):
                // every wooden segment and gate still standing turns to stone.
                for k in row {
                    let to = match m.objects.get(k).map(|x| (x.class, alive(x))) {
                        Some((class::WALL_WOOD, true)) => class::WALL_STONE,
                        Some((class::GATE_WOOD, true)) => class::GATE_STONE,
                        _ => continue,
                    };
                    if m.objects[k].flags & FLAG_CONSTRUCTION == 0 {
                        change_class(sim, m, lib, k, to);
                    }
                }
            } else if let Some(class) = class {
                change_class(sim, m, lib, i, class);
            } else if name.starts_with("to_ship_level") {
                sim.gameplay.actors[i].ship_level = ship_level(sim, i).saturating_add(1).min(6);
            }
            // No native upgrade raises a level (UPGRADES.INI: towers, walls,
            // gates, ship cannons and to_convertres).
        }
        JobKind::Item(category, index, gold) => {
            create_item(sim, m, g, lib, c, i, *category, *index, *gold);
        }
    }
    true
}
/// A research completed for kingdom `owner` (0x4d33d0): the name joins
/// its list and each of its objects takes the bonus, as a building's step
/// or the cheat panel's «discover» (0x424886) gives it.
pub(super) fn finish_research(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    owner: u8,
    name: &str,
) {
    let Some(rules) = m.rules.get_mut(owner.wrapping_sub(1) as usize) else {
        return;
    };
    // 0x41eed0 → 0x4d33d0: every completion runs the handler again, so a
    // research queued in two busy buildings counts twice (+0x2c8 is 2).
    let r = &mut rules.initial_researches;
    let previous = r.clone();
    r.push(name.to_owned());
    let known = r.clone();
    let count = crate::simulation::completions(&known, name);
    for (j, x) in m
        .objects
        .iter_mut()
        .enumerate()
        .filter(|(_, x)| x.owner == owner && alive(x))
    {
        if name.eq_ignore_ascii_case("te_greatwall") {
            crate::simulation::great_wall_health(x);
        }
        // Add the research's bonus once, keeping runtime mana, containers,
        // orders and what potions and spells changed.
        let mut before = ObjectState::read(x, lib);
        before.apply_dispatch(x, &previous);
        let mut after = ObjectState::read(x, lib);
        after.apply_dispatch(x, &known);
        let s = &mut sim.objects[j];
        match name.to_ascii_lowercase().as_str() {
            // 0x4d389f: +50 to K+0x106, then the kingdom's ships grow
            // to their capacity × (100 + K+0x106) / 100.
            "te_landingtactics" if matches!(x.class, 23..=25) => {
                let base = lib
                    .property(x, "max_obj_in_holder")
                    .and_then(|v| v.trim().parse::<u32>().ok())
                    .unwrap_or(0);
                let had = s
                    .capacity
                    .map_or(base + base * 50 * (count - 1) / 100, u32::from);
                s.capacity = Some((had * (100 + 50 * count) / 100).min(u16::MAX as u32) as u16);
            }
            // 0x4d4106: each building's own target.
            "te_budhism" | "te_fanaticism" if x.building() => {
                let had = s.faith.unwrap_or(0);
                s.faith = Some(had.saturating_add(1));
            }
            _ => {}
        }
        rebase(
            sim,
            j,
            [before.attack, before.defence],
            [after.attack, after.defence],
        );
    }
}
/// The record of an item: its size, picture and word fields; a book also
/// carries its spell (+0xce).
fn fill_item(item: &mut MapObject, image: &str, size: usize, fields: &[(usize, u16)], spell: u8) {
    item.data.resize(size, 0);
    item.data[0..2].copy_from_slice(&(size as u16).to_le_bytes());
    item.data[56..96].fill(0);
    item.data[56..56 + image.len()].copy_from_slice(image.as_bytes());
    item.custom_image = image.into();
    item.data[0x22..0x24].copy_from_slice(&10u16.to_le_bytes());
    if item.class == class::ITEM_MAGIC {
        item.data[0x60] = spell;
        item.data[0x61] = 0;
        item.data[0x62] = 0;
        item.data[0x63] = 1;
    }
    for &(at, value) in fields {
        item.data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    item.health = 0;
    item.max_health = 0;
}
/// The magic workshop's item (0x4743b0 → 0x465ca0), with `gold` already
/// paid; divisions truncate. The item appears on a random cell of the ring
/// around the building (0x41fd70) or the nearest free one, and the native
/// code then takes the same gold again when the kingdom can pay it
/// (0x466a87): books `charges × divisor`, the other items `gold`.
#[allow(clippy::too_many_arguments)]
fn create_item(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    category: u8,
    index: u8,
    gold: u32,
) {
    let o = m.objects[i].clone();
    let g4 = (gold / 4).min(u16::MAX as u32) as u16;
    let half = (gold / 2).min(u16::MAX as u32) as u16;
    let gold16 = gold.min(u16::MAX as u32) as u16;
    // (class, picture, record size, word fields)
    let (class, image, size, fields, again): (usize, &str, usize, Vec<(usize, u16)>, u32) =
        match (category, index) {
            (0, spell) => {
                let d = book_divisor(c, &o, spell);
                let charges = g4 as u32 / d;
                if charges == 0 {
                    // The book is discarded and the gold stays spent.
                    let least = 4 * d;
                    if sim.gameplay.notices.len() < 16 {
                        sim.gameplay.notices.push(format!(
                            "Questo libro magico richiede almeno {least} unità d'oro"
                        ));
                    }
                    return;
                }
                let charges = charges.min(u16::MAX as u32) as u16;
                (
                    class::ITEM_MAGIC,
                    "item_book1",
                    0x70,
                    vec![(0x64, 0), (0x66, charges)],
                    charges as u32 * d,
                )
            }
            (1, 0) => (
                class::ITEM_HEALTH,
                "item_potion2",
                0x6a,
                vec![(0x60, 4), (0x12, gold16)],
                gold,
            ),
            (1, 1) => (
                class::ITEM_HEALTH,
                "item_potion3",
                0x6a,
                vec![(0x60, 8), (0x1c, (gold / 5) as u16)],
                gold,
            ),
            (1, 2) => (
                class::ITEM_HEALTH,
                "item_potion1",
                0x6a,
                vec![(0x60, 0x10), (0x24, gold16)],
                gold,
            ),
            (1, 3) => (
                class::ITEM_HEALTH,
                "item_potion7",
                0x6a,
                vec![(0x60, 1), (0x16, half)],
                gold,
            ),
            (1, 4) => (
                class::ITEM_HEALTH,
                "item_potion6",
                0x6a,
                vec![(0x60, 2), (0x18, half)],
                gold,
            ),
            (2, 0) => (
                class::ITEM_PLUSPLUS,
                "item_sword1",
                0x6a,
                vec![(0x60, 1), (0x16, g4.min(250))],
                gold,
            ),
            (2, 1) => (
                class::ITEM_PLUSPLUS,
                "item_bow1",
                0x6a,
                vec![(0x60, 1), (0x16, g4.min(250) << 8)],
                gold,
            ),
            (2, 2) => (
                class::ITEM_PLUSPLUS,
                "item_shield1",
                0x6a,
                vec![(0x60, 2), (0x18, g4)],
                gold,
            ),
            _ => return,
        };
    let [w, h] = footprint(lib, &o);
    let rows = (o.y as u64).saturating_sub(1)..=(o.y as u64 + h as u64);
    let pick = |sim: &mut Simulation, r: &std::ops::RangeInclusive<u64>| {
        r.start() + random(sim, r.end() - r.start() + 1)
    };
    let columns = (o.x as u64).saturating_sub(1)..=(o.x as u64 + w as u64);
    let p = match random(sim, 4) {
        0 => [pick(sim, &columns), *rows.start()],
        1 => [pick(sim, &columns), *rows.end()],
        2 => [*columns.start(), pick(sim, &rows)],
        _ => [*columns.end(), pick(sim, &rows)],
    };
    let side = g.side as u64 - 1;
    let p = [p[0].min(side) as u16, p[1].min(side) as u16];
    let Ok(j) = new_unit(sim, m, lib, class, 0, 0, p) else {
        return;
    };
    fill_item(&mut m.objects[j], image, size, &fields, index);
    if let Some(q) = free_near(g, m, lib, &m.objects[j].clone(), p, Some(j)) {
        position(&mut m.objects[j], q);
    }
    sim.objects[j] = ObjectState::read(&m.objects[j], lib);
    let mut cost = [0; 4];
    cost[0] = again;
    if stock(m, o.owner)[0] >= again {
        let _ = debit(c, m, o.owner, cost);
    }
}
fn work(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    intent: Intent,
) {
    let o = m.objects[i].clone();
    match intent {
        Intent::Transport(t) => ai::transport::transport(sim, m, g, lib, i, *t),
        Intent::UseTransport(r) => ai::transport::ride(sim, m, g, lib, c, i, r),
        Intent::Reach(j) => {
            let rect = match sim.gameplay.actors[i].move_target {
                Some((t, rect)) if t == j => rect,
                _ => {
                    let rect = movement::target_rect(sim, m, lib, j);
                    sim.gameplay.actors[i].move_target = Some((j, rect));
                    rect
                }
            };
            // 0x4def3e, as for any Move given an object.
            let hidden = sim
                .gameplay
                .actors
                .get(j)
                .is_some_and(|a| a.effect_until > sim.ticks)
                && relation(m, side(sim, m, j), side(sim, m, i)) & 1 == 0;
            let range = combat::max_range(m, c, i);
            if hidden
                || movement::walk(sim, m, g, lib, i, rect, range, None) == Some(true)
                || unreachable(sim, i)
            {
                finish(sim, i);
            }
        }
        Intent::Move(dest) => {
            // Native program 1 (Move) only walks: a carried load stays with
            // the worker until a store is ordered or the work resumes.
            if let Some((j, rect)) = sim.gameplay.actors[i].move_target {
                // 0x4def3e: an invisible target whose kingdom does not
                // share its view with the unit's ends the Move.
                let hidden = sim
                    .gameplay
                    .actors
                    .get(j)
                    .is_some_and(|a| a.effect_until > sim.ticks)
                    && relation(m, side(sim, m, j), side(sim, m, i)) & 1 == 0;
                if hidden
                    || movement::walk(sim, m, g, lib, i, rect, 0, None) == Some(true)
                    || unreachable(sim, i)
                {
                    finish(sim, i);
                }
                return;
            }
            if approach(sim, m, g, lib, i, dest, 0) || unreachable(sim, i) {
                finish(sim, i);
            }
        }
        Intent::Cast {
            key,
            cell,
            mut target,
        } => {
            let heal = key == "spell_heal";
            // 0x4e0173: after its last cast the caster keeps its picture to
            // the end of the animation, then the order goes on.
            if let Some(t) = sim.gameplay.actors[i].pose_until {
                if sim.ticks < t {
                    return;
                }
                let a = &mut sim.gameplay.actors[i];
                a.pose_until = None;
                a.action_image.clear();
                if !heal {
                    finish(sim, i);
                    return;
                }
            }
            // Step 4 of the Heal program: 2000 ms after a failed walk or
            // cast, the healer looks again.
            if let Some(t) = sim.gameplay.actors[i].search_at {
                if sim.ticks < t {
                    return;
                }
                sim.gameplay.actors[i].search_at = None;
            }
            // Op 0x51 of the Heal program (0x4e2dcb): the target while it is
            // alive and damaged, otherwise the nearest damaged friend.
            if heal && target.is_none_or(|j| !wounded(sim, m, j)) {
                target = damaged_near(sim, m, lib, i);
                let Some(j) = target else {
                    finish(sim, i);
                    return;
                };
                let a = &mut sim.gameplay.actors[i];
                a.intent = Some(Intent::Cast {
                    key: key.clone(),
                    cell: [m.objects[j].x, m.objects[j].y],
                    target,
                });
                a.action_image.clear();
            }
            let p = target
                .and_then(|j| m.objects.get(j))
                .map_or(cell, |x| [x.x, x.y]);
            if target.is_some_and(|j| !target_known(sim, m, o.owner, j)) {
                finish(sim, i);
                return;
            }
            let (range, cost) = spell_parameters(c, m, &o, &key);
            if sim.gameplay.actors[i].cooldown > 0 && sim.gameplay.actors[i].action_image.is_empty()
            {
                // Waiting for mana (0x4e1d2e).
                return;
            }
            if o.x.abs_diff(p[0]).max(o.y.abs_diff(p[1])) as u32 > range {
                sim.gameplay.actors[i].action_image.clear();
                if !approach(sim, m, g, lib, i, p, range) && unreachable(sim, i) {
                    if heal {
                        sim.gameplay.actors[i].search_at = Some(sim.ticks + 2000 / TICK_MS);
                    } else {
                        finish(sim, i);
                    }
                }
                return;
            }
            let rect = target
                .and_then(|j| m.objects.get(j))
                .map_or([p[0] as i32, p[1] as i32, p[0] as i32, p[1] as i32], |x| {
                    movement::footprint_rect(lib, x)
                });
            face(sim, m, lib, i, rect, Some("image_spell"));
            let picture = lib.property(&o, "image_spell").is_some();
            let (before, period) = animation_timing(lib, &o, "image_spell").unwrap_or((0, 1000));
            let now_ms = sim.ticks * TICK_MS;
            if sim.gameplay.actors[i].action_image != "image_spell" {
                // 0x4e1c74: a computer player's fire rain or freeze first
                // sends its own units out of the area (0x472760).
                ai::spells::clear_area(sim, m, g, lib, c, i, &key, cell, target);
                // 0x4e1c30: short of mana the caster stands idle and looks
                // again in 2000 ms, its maximum raised to the cost if lower.
                if sim.objects[i].mana < cost {
                    if sim.objects[i].max_mana < cost {
                        sim.objects[i].max_mana = cost;
                        // 0x4e1d0a: the `pump` picture plays over the caster.
                        overlay(sim, m, lib, i, "pump");
                    }
                    sim.objects[i].order = OrderState::Idle;
                    sim.gameplay.actors[i].cooldown = (2000 / TICK_MS) as u32;
                    return;
                }
                // The spell takes effect when the casting reaches `actionrow`;
                // a caster without the picture waits 1000 ms.
                let a = &mut sim.gameplay.actors[i];
                a.action_image = "image_spell".into();
                a.action_elapsed_ms = 0;
                a.cast_due_ms = now_ms + if picture { before } else { 1000 };
            }
            sim.objects[i].order = OrderState::Casting;
            let a = &mut sim.gameplay.actors[i];
            a.action_elapsed_ms = a.action_elapsed_ms.saturating_add(TICK_MS);
            if now_ms < a.cast_due_ms {
                return;
            }
            // 0x4dff44: without the mana the spell op waits 2000 ms and
            // tries again; the order stays.
            if sim.objects[i].mana < cost {
                sim.gameplay.actors[i].cast_due_ms = now_ms + 2000;
                return;
            }
            let request = Request {
                order: key.clone(),
                selected: vec![i],
                cell: Some(p),
                target,
                append: false,
                ..Default::default()
            };
            match cast(sim, m, g, lib, c, i, &key, &request) {
                // 0x4e00a0 repeats a spell that returns 2 after `image_spell`,
                // due a whole `anim_time` after the cast before.
                Ok(true) => sim.gameplay.actors[i].cast_due_ms += period.max(1),
                // The picture plays to its end, then the order ends or the
                // Heal program looks for the next wounded (step 1).
                Ok(false) if picture => {
                    let a = &mut sim.gameplay.actors[i];
                    let end = a.cast_due_ms + period.saturating_sub(before);
                    a.pose_until = Some(end.div_ceil(TICK_MS));
                }
                // 0x4e0114: without `image_spell` three stars twinkle around
                // the caster for 2 s instead.
                Ok(false) if heal => {
                    let r = rect_around(lib, &m.objects[i], 4);
                    sparkles(sim, r, 3, 2000);
                    sim.gameplay.actors[i].action_image.clear();
                }
                Ok(false) => {
                    let r = rect_around(lib, &m.objects[i], 4);
                    sparkles(sim, r, 3, 2000);
                    finish(sim, i);
                }
                Err(_) if heal => {
                    let a = &mut sim.gameplay.actors[i];
                    a.action_image.clear();
                    a.search_at = Some(sim.ticks + 2000 / TICK_MS);
                }
                Err(error) => {
                    if sim.gameplay.notices.len() < 16 {
                        sim.gameplay.notices.push(error);
                    }
                    finish(sim, i);
                }
            }
        }
        Intent::AttackGround(p) => {
            // The Attack op on the ground (opcode 0x77 → 0x4df800): the same
            // cycle as against an object, the missile landing on the cell.
            let range = combat::max_range(m, c, i);
            let (x, y) = (p[0] as i32, p[1] as i32);
            let rect = [x - 1, y - 1, x + 1, y + 1];
            if !combat::within(m, lib, i, rect, range) {
                sim.gameplay.actors[i].action_image.clear();
                if !approach(sim, m, g, lib, i, p, range) && unreachable(sim, i) {
                    finish(sim, i);
                }
                return;
            }
            let min = combat::min_range(lib, &o);
            if min != 0 && combat::within(m, lib, i, rect, min) {
                finish(sim, i);
                return;
            }
            halt(&mut sim.gameplay.actors[i]);
            face(sim, m, lib, i, [x, y, x, y], Some("image_attack"));
            sim.objects[i].order = OrderState::Attacking;
            if let Some(Swing::Blow { due, lead, period }) = swing(sim, m, lib, i) {
                sim.gameplay.actors[i].last_hit_ms = due;
                shoot_ground(sim, m, lib, c, i, p);
                next_picture(sim, m, lib, i, due, lead, period);
            }
        }
        // The Explore program (0x42f3a0): Move to the point, then to each
        // cell op 0x41 finds; after a failed Move, a Delay of 2 s and op 0x41
        // from where the unit stands.
        Intent::Explore(point) => {
            if let Some(due) = sim.gameplay.actors[i].search_at {
                if sim.ticks >= due {
                    sim.gameplay.actors[i].search_at = None;
                    explore_on(sim, m, g, lib, i, point);
                }
                return;
            }
            let target = sim.gameplay.actors[i].explore_target.unwrap_or(point);
            if approach(sim, m, g, lib, i, target, 0) {
                explore_on(sim, m, g, lib, i, point);
            } else if unreachable(sim, i) {
                let a = &mut sim.gameplay.actors[i];
                halt(a);
                a.search_at = Some(sim.ticks + 2000 / TICK_MS);
            }
        }
        Intent::Murder(j) => {
            // Op 0x5b (0x4e076a): within attack range, 60 mana, the target
            // seen: the ninja shows itself, spends all its mana and strikes
            // once with fifteen times its attack.
            if !standing(m, j) || !target_known(sim, m, side(sim, m, i), j) {
                finish(sim, i);
                return;
            }
            let range = combat::max_range(m, c, i);
            if !movement::in_range(sim, m, lib, i, j, range) {
                if !approach_object(sim, m, g, lib, i, j, range) && unreachable(sim, i) {
                    finish(sim, i);
                }
                return;
            }
            halt(&mut sim.gameplay.actors[i]);
            let rect = movement::footprint_rect(lib, &m.objects[j]);
            face(sim, m, lib, i, rect, Some(murder_image(lib, &o)));
            if sim.objects[i].mana < 60 {
                if o.owner == m.player() && sim.gameplay.notices.len() < 16 {
                    sim.gameplay.notices.push(c.notmanna.clone());
                }
                finish(sim, i);
                return;
            }
            murder(sim, m, g, lib, c, i, j);
            finish(sim, i);
        }
        Intent::Impersonate(j) => {
            // Op 0x60 (0x4e0511): next to a living, ordinary unit with 60
            // energy, the spy hands its record to it and is gone.
            let ok = m
                .objects
                .get(j)
                .is_some_and(|x| standing(m, j) && !hero(x) && impersonable(lib, x));
            if !ok {
                finish(sim, i);
                return;
            }
            if !movement::in_range(sim, m, lib, i, j, 0) {
                if !approach_object(sim, m, g, lib, i, j, 0) && unreachable(sim, i) {
                    finish(sim, i);
                }
                return;
            }
            // A host passes on the record it carries (+0xf6): the hidden spy
            // moves into the new unit and the host stays as it was.
            if let Some(mole) = sim.gameplay.actors[i].mole.clone() {
                if mole.energy < 60 {
                    if mole.kingdom == m.player() && sim.gameplay.notices.len() < 16 {
                        sim.gameplay.notices.push(c.notmanna.clone());
                    }
                    finish(sim, i);
                    return;
                }
                // The new host takes the record (vt+0x2dc), then the old
                // one lets it go (vt+0x2e0), each with the spy's hold.
                let k = mole.kingdom;
                sim.gameplay.actors[j].mole = Some(Mole { energy: 0, ..mole });
                m.objects[j].mole = m.objects[i].mole;
                if m.objects[j].owner != 0 {
                    set_control(sim, m, j, k, 1);
                }
                sim.gameplay.actors[i].mole = None;
                m.objects[i].mole = 0;
                if m.objects[i].owner != 0 {
                    set_control(sim, m, i, k, 0);
                }
                finish(sim, i);
                return;
            }
            if sim.gameplay.actors[i].spy_energy < 60 {
                if o.owner == m.player() && sim.gameplay.notices.len() < 16 {
                    sim.gameplay.notices.push(c.notmanna.clone());
                }
                finish(sim, i);
                return;
            }
            let s = &sim.objects[i];
            sim.gameplay.actors[j].mole = Some(Mole {
                kingdom: o.owner,
                civilization: o.civilization,
                name: o.name.clone(),
                subtitle: o.subtitle.clone(),
                flags: o.flags,
                health: o.health,
                max_health: o.max_health,
                data: o.data.clone(),
                level: s.level,
                xp: s.xp,
                attack: s.attack,
                defence: s.defence,
                sight: s.sight,
                energy: 0,
            });
            m.objects[j].mole = o.owner;
            if m.objects[j].owner != 0 {
                set_control(sim, m, j, o.owner, 1);
            }
            vanish(sim, m, i);
        }
        Intent::Flee(dest, rest) => {
            if let Some(t) = sim.gameplay.actors[i].search_at {
                if sim.ticks >= t {
                    finish(sim, i);
                }
                return;
            }
            if approach(sim, m, g, lib, i, dest, 0) || unreachable(sim, i) {
                halt(&mut sim.gameplay.actors[i]);
                sim.gameplay.actors[i].search_at =
                    Some(sim.ticks + (rest as u64).div_ceil(TICK_MS));
            }
        }
        Intent::Patrol(a, b) => {
            if [o.x, o.y] == b {
                sim.gameplay.actors[i].intent = Some(Intent::Patrol(b, a));
                sim.gameplay.actors[i].goal = None;
                // Every leg is a Move of its own, with its own look-out.
                sim.gameplay.actors[i].advance_due_ms = None;
            } else {
                approach(sim, m, g, lib, i, b, 0);
            }
        }
        Intent::Circuit(points, next, closed) => {
            let Some(&p) = points.get(next) else {
                finish(sim, i);
                return;
            };
            if [o.x, o.y] != p {
                approach(sim, m, g, lib, i, p, 0);
                if !unreachable(sim, i) {
                    return;
                }
            }
            // An open round ends at its last point: its final step (op 0x45)
            // has no handler, so the unit stays there.
            let last = next + 1 == points.len();
            let a = &mut sim.gameplay.actors[i];
            a.goal = None;
            if !last || closed {
                let after = (next + 1) % points.len();
                a.intent = Some(Intent::Circuit(points, after, closed));
                a.advance_due_ms = None;
            }
        }
        Intent::Chop(p) => {
            // The Chop program (0x42dbe0).
            let cell = p[1] as usize * g.side + p[0] as usize;
            // Steps 4-7: a load of wood goes to the lumber mill first.
            if sim.gameplay.actors[i].cargo[3] > 0 || sim.gameplay.actors[i].depositing.is_some() {
                deposit(sim, m, g, lib, i);
                return;
            }
            // Step 9: two seconds after a failed walk, the peasant looks for
            // another tree from where he stands.
            if let Some(t) = sim.gameplay.actors[i].search_at {
                if sim.ticks < t {
                    sim.objects[i].order = OrderState::Idle;
                    return;
                }
                sim.gameplay.actors[i].search_at = None;
                next_tree(sim, m, g, lib, i);
                return;
            }
            // Step 1: back to the tree, or to its stump.
            if o.x.abs_diff(p[0]).max(o.y.abs_diff(p[1])) > 1 {
                if !approach_near(sim, m, g, lib, i, p) && unreachable(sim, i) {
                    sim.gameplay.actors[i].search_at = Some(sim.ticks + 2000 / TICK_MS);
                }
                return;
            }
            // Steps 3 and 8: at a stump the peasant searches the nearest tree.
            if !tree_at(g, lib, p) {
                next_tree(sim, m, g, lib, i);
                return;
            }
            work_animation(
                sim,
                m,
                lib,
                i,
                OrderState::Working,
                "image_chop",
                Some("@chop"),
                p,
                [1, 1],
            );
            // 0x4ad1cc: 50 ms, then six chop cycles of the animation's time.
            let ticks = 1 + cycles_ticks(lib, &o, "image_chop", 6);
            if work_ready(&mut sim.gameplay.actors[i], None, Some(p), ticks) {
                // 0x4ae2a0: a load of another kind is dropped on the ground.
                if sim.gameplay.actors[i].cargo.iter().any(|&n| n > 0) {
                    drop_cargo(sim, m, g, lib, i);
                }
                // 0x476e10: a tree gives ten loads (counted here by tens).
                let left = sim.gameplay.trees.entry(cell).or_insert(100);
                *left = left.saturating_sub(10);
                let felled = *left == 0;
                let a = &mut sim.gameplay.actors[i];
                a.cargo = [0, 0, 0, resource_yield(m, o.owner, 3)];
                a.work_cell = None;
                if felled {
                    fell(sim, g, lib, cell);
                }
            }
        }
        Intent::Harvest(j) => {
            let Some(t) = m.objects.get(j).cloned() else {
                finish(sim, i);
                return;
            };
            if t.class == class::RESITEM && alive(&t) && sim.gameplay.actors[i].depositing.is_none()
            {
                // Gather Resource Items (program 0x1e): the peasant walks to
                // the item and takes it (0x464f60).
                if distance(lib, &o, &t) > 1 {
                    if !approach_object(sim, m, g, lib, i, j, 0) && unreachable(sim, i) {
                        finish(sim, i);
                    }
                    return;
                }
                take_resources(sim, m, g, lib, i, j);
                return;
            }
            let milking = t.class == class::COW;
            let field = t.class == class::FIELD;
            if milking {
                // Milk a cow (0x4abf60): another load is dropped on the
                // ground first (0x4aea70).
                let a = &sim.gameplay.actors[i];
                if carry_type(a) != Some(4) && a.cargo.iter().any(|&n| n > 0) {
                    drop_cargo(sim, m, g, lib, i);
                }
            }
            let fishing = t.class == class::FISHPATCH;
            let mine = matches!(t.class, class::RES_GOLD | class::RES_STONE);
            let cargo = sim.gameplay.actors[i].cargo;
            let carrying = cargo.iter().any(|&n| n > 0);
            let milked_out = milking && sim.objects[j].mana == 0;
            // A boat goes on fishing until it holds 50 (HasFish, 0x4e92be).
            let full = cargo[2] >= 50 || !alive(&t);
            // 'Has mine res?' (0x4ad1da) sends to the store first only a load
            // of gold or stone; other loads are dropped when the new one is
            // taken.
            let deliver_first = if milking {
                milked_out || !alive(&t)
            } else if fishing {
                full
            } else if mine {
                cargo[0] > 0 || cargo[1] > 0
            } else {
                true
            };
            if sim.gameplay.actors[i].depositing.is_some() || (carrying && deliver_first) {
                deposit(sim, m, g, lib, i);
                if field
                    && sim.gameplay.actors[i].dig_again
                    && sim.gameplay.actors[i].cargo == [0; 4]
                {
                    another_field(sim, m, lib, i);
                }
                return;
            }
            if fishing && !alive(&t) {
                // FindFishPatch (op 0x3e, 0x4e9238): the nearest patch,
                // measured to its picture of one cell (0x454d70), not to
                // its 2×2.
                let next = m
                    .objects
                    .iter()
                    .enumerate()
                    .filter(|(_, x)| x.class == class::FISHPATCH && alive(x))
                    .min_by_key(|(_, x)| distance_to_area([o.x, o.y], [x.x, x.y], [1, 1]))
                    .map(|(k, _)| k);
                match next {
                    Some(k) => {
                        let a = &mut sim.gameplay.actors[i];
                        a.intent = Some(Intent::Harvest(k));
                        halt(a);
                    }
                    None => finish(sim, i),
                }
                return;
            }
            if !alive(&t) || (milking && enemy(m, o.owner, t.owner)) {
                finish(sim, i);
                return;
            }
            if milking
                && sim.gameplay.actors[i].work_target != Some(j)
                && sim.objects[j].mana < sim.objects[j].max_mana
            {
                // Step 0xe comes before every search for a cow: the nearest
                // food item is fetched and delivered first (steps 0xf, 0x10,
                // then 1).
                if !carrying && let Some(k) = food_item(sim, m, g, lib, i) {
                    let a = &mut sim.gameplay.actors[i];
                    a.queued.push_front(Intent::Harvest(j));
                    a.intent = Some(Intent::Harvest(k));
                    halt(a);
                    return;
                }
                // Find a cow to milk (0x4acade): the nearest own cow with full
                // milk; with none, the peasant waits for one to refill.
                let full = m
                    .objects
                    .iter()
                    .enumerate()
                    .filter(|&(k, x)| {
                        x.class == class::COW
                            && alive(x)
                            && !enemy(m, o.owner, x.owner)
                            && sim.objects[k].mana >= sim.objects[k].max_mana
                            && !sim
                                .gameplay
                                .actors
                                .iter()
                                .enumerate()
                                .any(|(w, a)| w != i && a.intent == Some(Intent::Harvest(k)))
                    })
                    .min_by_key(|(_, x)| distance(lib, &o, x))
                    .map(|(k, _)| k);
                match full {
                    Some(k) => {
                        let a = &mut sim.gameplay.actors[i];
                        a.intent = Some(Intent::Harvest(k));
                        halt(a);
                    }
                    None => sim.objects[i].order = OrderState::Idle,
                }
                return;
            }
            // A fish patch is reached around its picture of one cell
            // (vt+0x134), not its 2×2.
            let size = if fishing { [1, 1] } else { footprint(lib, &t) };
            if field {
                // Op 0x15 (0x4acff7) fails on a field another peasant
                // works, or off any field: op 0x25 looks for another.
                if !alive(&t) || field_worker(sim, j).is_some_and(|w| w != i) {
                    another_field(sim, m, lib, i);
                    return;
                }
                // Step 1 of the Dig program (0x42e600): a Move to the
                // field's middle cell; after a failed one the peasant digs
                // where it stands, if that is the field.
                let middle = [t.x + 1, t.y + 1];
                if [o.x, o.y] != middle {
                    if approach(sim, m, g, lib, i, middle, 0) {
                        return;
                    }
                    if !unreachable(sim, i) {
                        return;
                    }
                    if distance(lib, &o, &t) > 0 {
                        another_field(sim, m, lib, i);
                        return;
                    }
                }
            } else if distance_to_area([o.x, o.y], [t.x, t.y], size) > 1 {
                if !approach_object(sim, m, g, lib, i, j, 0) && unreachable(sim, i) {
                    finish(sim, i);
                }
                return;
            }
            let property = if milking {
                "image_milk"
            } else if matches!(t.class, class::RES_GOLD | class::RES_STONE) {
                "image_mine"
            } else {
                "image_dig"
            };
            if field && sim.gameplay.actors[i].work_target != Some(j) {
                // The dig starts (0x4acff7) facing a way drawn from the
                // stream of pictures (0x4ad078), then counts down 12–18
                // cycles (0x4ad088); `field_cycles` is the countdown + 1.
                let facing = random_aux(sim, 8) as u8;
                let n = 12 + random(sim, 7) as u8;
                let a = &mut sim.gameplay.actors[i];
                a.direction = facing;
                a.field_cycles = n + 1;
                sim.gameplay.fields.insert(j, sim.ticks);
            }
            let facing = sim.gameplay.actors[i].direction;
            work_animation(
                sim,
                m,
                lib,
                i,
                OrderState::Working,
                property,
                if milking {
                    None
                } else if property == "image_mine" {
                    Some("@mine")
                } else {
                    Some("@dig")
                },
                [t.x, t.y],
                footprint(lib, &t),
            );
            if field {
                // Standing on the field, the peasant keeps that way.
                sim.gameplay.actors[i].direction = facing;
            }
            let cycles = if milking || field { 1 } else { 6 };
            let ticks = if fishing {
                // Fishing (op 0x3b, 0x4e911e): a take every 1400 ms.
                (1400 / TICK_MS) as u32
            } else if mine {
                // 0x4ad2d2: 50 ms, then six mining cycles.
                1 + cycles_ticks(lib, &o, property, cycles)
            } else {
                cycles_ticks(lib, &o, property, cycles)
            };
            if fishing
                && sim.gameplay.actors[i].work_target != Some(j)
                && fishing_boats(sim, j) == 0
            {
                let first = random_aux(sim, 3) as u8;
                sim.gameplay.nets.insert(j, first);
            }
            if !work_ready(&mut sim.gameplay.actors[i], Some(j), None, ticks) {
                return;
            }
            if fishing {
                // At most four fish a take, up to 50; the patch starts with
                // the amount of its record (+0x38) and goes when emptied
                // (0x451050).
                let want = 4.min(50 - sim.gameplay.actors[i].cargo[2]);
                let left = sim.gameplay.resources.entry(j).or_insert(t.dword(0x38));
                let taken = want.min(*left);
                *left -= taken;
                let emptied = *left == 0;
                let a = &mut sim.gameplay.actors[i];
                a.cargo[2] += taken;
                a.cargo_source = class::FISHPATCH;
                if a.cargo[2] >= 50 {
                    a.work_target = None;
                }
                if emptied {
                    sim.gameplay.actors[i].work_target = None;
                    sim.gameplay.resources.remove(&j);
                    kill(sim, m, g, lib, j);
                }
                return;
            }
            if milking {
                // Native 0x4ac048 transfers at most three mana per milk cycle,
                // until the cow is empty; there is no load limit.
                let quantity = sim.objects[j].mana.min(3);
                sim.objects[j].mana -= quantity;
                sim.gameplay.actors[i].cargo[2] += quantity as u32;
                sim.gameplay.actors[i].cargo_source = class::COW;
                return;
            }
            if field {
                // Each cycle (0x4ac180) yields the kingdom's food once the
                // countdown is over; otherwise the field grows by 10
                // (0x44cc70) and the countdown goes on unless the field is
                // still short of 100 afterwards.
                if sim.gameplay.actors[i].field_cycles == 0 {
                    // A dig restored without its countdown draws it now.
                    let n = 12 + random(sim, 7) as u8;
                    sim.gameplay.actors[i].field_cycles = n + 1;
                }
                if sim.gameplay.actors[i].field_cycles == 1 {
                    let a = &mut sim.gameplay.actors[i];
                    a.field_cycles = 0;
                    a.cargo[2] = resource_yield(m, o.owner, 2);
                    a.cargo_source = class::FIELD;
                    a.work_target = None;
                    sim.gameplay.fields.insert(j, sim.ticks);
                    return;
                }
                let x = &mut m.objects[j];
                let growing = x.health < x.max_health;
                if growing {
                    x.health = x.health.saturating_add(10).min(x.max_health);
                    // The grow picture is set at column 0, and the ripe one
                    // keeps it (0x44cd0f, 0x501780).
                    x.variant = 0;
                }
                if !growing || x.health == x.max_health {
                    sim.gameplay.actors[i].field_cycles -= 1;
                }
                return;
            }
            let res = match t.class {
                class::RES_GOLD => 0,
                class::RES_STONE => 1,
                _ => 2,
            };
            let default = match t.class {
                class::RES_GOLD | class::RES_STONE => {
                    if t.byte(0x3f) != 0 {
                        u32::MAX
                    } else {
                        t.dword(0x3a)
                    }
                }
                _ => 0,
            };
            let desired = resource_yield(m, o.owner, res);
            let ore_percent = if res <= 1
                && researches(m, o.owner)
                    .iter()
                    .any(|r| r.eq_ignore_ascii_case("te_orefloatation"))
            {
                50
            } else {
                100
            };
            let left = sim.gameplay.resources.entry(j).or_insert(default);
            if *left == 0 {
                // A mine placed empty dies at its first take (0x4affab).
                if mine {
                    exhaust(sim, m, g, lib, j);
                }
                if deposit(sim, m, g, lib, i) {
                    finish(sim, i);
                }
                return;
            }
            let consumed = if *left == u32::MAX {
                desired * ore_percent / 100
            } else {
                (*left).min(desired * ore_percent / 100)
            };
            if *left != u32::MAX {
                *left -= consumed;
            }
            let left = *left;
            // 0x4ae2a0: a load of another kind is dropped on the ground.
            if sim.gameplay.actors[i].cargo.iter().any(|&n| n > 0) {
                drop_cargo(sim, m, g, lib, i);
            }
            let load = consumed * 100 / ore_percent;
            sim.gameplay.actors[i].cargo[res] = load;
            // The mine scores what it gives (0x443430 type 4, 0x4b0076).
            if mine {
                add_score(sim, o.owner, load as u64);
            }
            sim.gameplay.actors[i].cargo_source = t.class;
            sim.gameplay.actors[i].work_target = None;
            if matches!(t.class, class::RES_GOLD | class::RES_STONE) {
                let data = &mut m.objects[j].data;
                data.resize(data.len().max(64), 0);
                data[58..62].copy_from_slice(&left.to_le_bytes());
                if left == 0 {
                    exhaust(sim, m, g, lib, j);
                }
            }
        }
        Intent::Deliver(j) => {
            // Return goods (program 0x35, 0x4adc20): Move to the store, then
            // deliverres (op 0x5e, 0x4ae990), which takes the load only at a
            // store of a kingdom that shares its resources (relation bit 2,
            // own ones included) for the store's kingdom.
            let cargo = sim.gameplay.actors[i].cargo;
            if !m.objects.get(j).is_some_and(alive) || cargo.iter().all(|&n| n == 0) {
                finish(sim, i);
            } else if distance(lib, &o, &m.objects[j]) > 1 {
                if !approach_object(sim, m, g, lib, i, j, 0) && unreachable(sim, i) {
                    finish(sim, i);
                }
            } else {
                let x = &m.objects[j];
                if relation(m, x.owner, o.owner) & 2 != 0
                    && x.flags & FLAG_CONSTRUCTION == 0
                    && store_accepts(x, cargo, naval(&o))
                {
                    credit(m, x.owner, cargo);
                    let a = &mut sim.gameplay.actors[i];
                    a.cargo = [0; 4];
                    a.cargo_source = 0;
                }
                finish(sim, i);
            }
        }
        Intent::Sell(j) => {
            // Unload Resource (program 0x21): Move, then the Trade op with
            // argument 0 (0x4e7c40). Its start takes a finished market or
            // harbor that no trader holds (vt+0x264 = 0x45b7b0; any other
            // building refuses); then the load goes to the building's
            // kingdom, with no profit, and the op ends.
            if !m.objects.get(j).is_some_and(alive) {
                finish(sim, i);
                return;
            }
            if distance(lib, &o, &m.objects[j]) > 1 {
                if !approach_object(sim, m, g, lib, i, j, 0) && unreachable(sim, i) {
                    finish(sim, i);
                }
                return;
            }
            let x = &m.objects[j];
            let free = matches!(x.class, class::MARKET | class::HARBOR)
                && x.flags & FLAG_CONSTRUCTION == 0
                && !sim.gameplay.actors.iter().enumerate().any(|(k, a)| {
                    k != i
                        && a.intent == Some(Intent::Trade(j))
                        && a.route.as_ref().is_some_and(|r| r.reserved == Some(j))
                });
            if free {
                let mut amount = [0; 4];
                let a = &mut sim.gameplay.actors[i];
                if let Some(r) = a.route.as_mut() {
                    amount[r.cargo_type.min(3) as usize] += r.cargo;
                    r.cargo = 0;
                }
                // A fishing boat's catch is its load (+0x120 = 2, 0x4e9155).
                if o.class == class::FISHING_BOAT {
                    for (n, c) in amount.iter_mut().zip(&mut a.cargo) {
                        *n += std::mem::take(c);
                    }
                    a.cargo_source = 0;
                }
                credit(m, x.owner, amount);
            }
            finish(sim, i);
        }
        Intent::Unload(p) => {
            // Program 0x16: go to the point; within 8 cells of it (op 0x48)
            // and, for a ship, 2 of a shore (op 0x88, 0x4e9390), the
            // passengers step off (op 0x33): around the point within 4 for
            // a ship (0x4e90dd), around the holder within 8 otherwise
            // (0x504890(-1, -1), [`way_out`]), on land (0x504550).
            if [o.x, o.y] != p {
                approach(sim, m, g, lib, i, p, 0);
                if !unreachable(sim, i) {
                    return;
                }
            }
            if o.x.abs_diff(p[0]).max(o.y.abs_diff(p[1])) > 8 {
                finish(sim, i);
                return;
            }
            let ship = naval(&o);
            // 0x4e87c0(2): a cell not of water from two cells before the
            // ship to one past its size (x − 2 to x + w, y − 2 to y + h),
            // within the map.
            let shore = {
                let [w, h] = footprint(lib, &o).map(|n| n as i32);
                let side = g.side as i32;
                let span =
                    |at: u16, size: i32| (at as i32 - 2).max(0)..(at as i32 + size + 1).min(side);
                let cells = &sim.gameplay.pathing.cells;
                span(o.y, h).any(|y| {
                    span(o.x, w).any(|x| cells.at([x, y]) & pathing::KIND != pathing::WATER)
                })
            };
            if shore || !ship {
                let contents = sim.objects[i].contents.clone();
                for j in contents {
                    if carried(&m.objects[j]) {
                        continue;
                    }
                    // 0x504574: the ship's passengers around the point
                    // itself, from the cell they came in by (wiped once the
                    // ship sailed).
                    let out = if ship {
                        let [x, y] = [p[0] as i32, p[1] as i32];
                        let start = sim.objects[j].entry.map(|e| [e[0] as i32, e[1] as i32]);
                        match exit_cell(sim, [x, y, x, y], start, 1, 4) {
                            Some(q) => {
                                leave(sim, m, i, j, q);
                                true
                            }
                            None => false,
                        }
                    } else {
                        step_off(sim, m, lib, i, j)
                    };
                    if !out {
                        break;
                    }
                }
            }
            finish(sim, i);
        }
        Intent::Approach(mut ap) => {
            // Approach (0x42dde0): Check approach distance (op 0x4f), the walk
            // to the place's anchor within the range (op 9), Wait others (op
            // 0x50), then the advance to the point (op 9 with argument 2).
            match ap.step {
                1 => {
                    match ai::war::approach_check(sim, m, i, &ap) {
                        Some(sector) => {
                            (ap.step, ap.sector, ap.due) = (3, sector, sim.ticks);
                        }
                        None => ap.step = 2,
                    }
                    sim.gameplay.actors[i].intent = Some(Intent::Approach(ap));
                }
                2 => match movement::walk(sim, m, g, lib, i, ap.rect, ap.radius as u32, None) {
                    Some(true) => {
                        (ap.step, ap.due) = (3, sim.ticks);
                        let a = &mut sim.gameplay.actors[i];
                        a.intent = Some(Intent::Approach(ap));
                        halt(a);
                    }
                    Some(false) => finish(sim, i),
                    _ if unreachable(sim, i) => finish(sim, i),
                    _ => {}
                },
                3 => {
                    sim.objects[i].order = OrderState::Idle;
                    if sim.ticks < ap.due {
                        return;
                    }
                    // 0x413c80: a target in sight is fought, with help.
                    if let Some(t) = find_target(sim, m, lib, c, i) {
                        engage(sim, m, lib, c, i, t);
                        call_for_help(sim, m, lib, c, i, t);
                        return;
                    }
                    match ai::war::wait_others(sim, m, i, &ap) {
                        0 => {
                            ap.due = sim.ticks + ai::war::WAIT_MS / TICK_MS;
                            sim.gameplay.actors[i].intent = Some(Intent::Approach(ap));
                        }
                        1 => {
                            ap.step = 4;
                            let a = &mut sim.gameplay.actors[i];
                            a.intent = Some(Intent::Approach(ap));
                            a.advance_due_ms = None;
                        }
                        _ => finish(sim, i),
                    }
                }
                _ => {
                    if approach(sim, m, g, lib, i, ap.point, 0) || unreachable(sim, i) {
                        finish(sim, i);
                    }
                }
            }
        }
        Intent::FarBuild(site) => {
            // «AI far build» (0x42e570): a Move to the site within 10 cells
            // (op 9), then op 0x3a (0x4ac95d); a failed walk ends it.
            let Some(rect) = ai::build::far_rect(sim, site) else {
                finish(sim, i);
                return;
            };
            match movement::walk(sim, m, g, lib, i, rect, 10, None) {
                Some(true) => {
                    ai::build::far_arrival(sim, m, g, lib, c, i, site);
                    if sim.gameplay.actors[i].intent == Some(Intent::FarBuild(site)) {
                        finish(sim, i);
                    }
                }
                _ if unreachable(sim, i) => finish(sim, i),
                _ => {}
            }
        }
        Intent::Build(j) | Intent::Repair(j) => {
            let building = matches!(sim.gameplay.actors[i].intent, Some(Intent::Build(_)));
            // Step 3 of the Build program (0x42e4ad): two seconds after a
            // failed walk or site, the peasant looks for another site. The
            // Repair program (0x42efe0) waits 4 s after a failed walk, then
            // looks for what else to mend (step 3).
            if let Some(t) = sim.gameplay.actors[i].search_at {
                if sim.ticks < t {
                    sim.objects[i].order = OrderState::Idle;
                    return;
                }
                sim.gameplay.actors[i].search_at = None;
                if building {
                    next_site(sim, m, lib, i);
                } else {
                    next_repair(sim, m, g, lib, i);
                }
                return;
            }
            // 0x4aba8a: a site of an ally (relation 0x40); 0x4abe66 repairs
            // whatever it was sent to.
            let site_ok = m
                .objects
                .get(j)
                .is_some_and(|x| x.flags & FLAG_CONSTRUCTION == 0)
                || site(m, o.owner, j);
            if !m.objects.get(j).is_some_and(alive) || !site_ok {
                if building {
                    halt(&mut sim.gameplay.actors[i]);
                    sim.gameplay.actors[i].search_at = Some(sim.ticks + 2000 / TICK_MS);
                } else {
                    next_repair(sim, m, g, lib, i);
                }
                return;
            }
            if distance(lib, &o, &m.objects[j]) > 1 {
                if !approach_object(sim, m, g, lib, i, j, 0) && unreachable(sim, i) {
                    halt(&mut sim.gameplay.actors[i]);
                    let wait = if building { 2000 } else { 4000 };
                    sim.gameplay.actors[i].search_at = Some(sim.ticks + wait / TICK_MS);
                }
                return;
            }
            // 0x4acc5f: the Build op on a finished building and the Repair op
            // on one no longer damaged succeed at once: on to the next site
            // (step 4) or to what else to mend (step 3).
            let finished = m.objects[j].flags & FLAG_CONSTRUCTION == 0;
            if building && finished {
                next_site(sim, m, lib, i);
                return;
            }
            if !building && finished && m.objects[j].health >= m.objects[j].max_health {
                next_repair(sim, m, g, lib, i);
                return;
            }
            // 0x4abac6: a peasant still in its build picture from the site
            // before (no step in between) neither turns nor starts the
            // picture again (0x4abb7a).
            let a = &sim.gameplay.actors[i];
            let site_point = [m.objects[j].x, m.objects[j].y];
            let posed =
                sim.objects[i].order == OrderState::Building && a.action_image == "image_build";
            let point = match a.action_point {
                Some(p) if posed && (a.work_target != Some(j) || p != site_point) => p,
                _ => site_point,
            };
            work_animation(
                sim,
                m,
                lib,
                i,
                OrderState::Building,
                "image_build",
                Some("@build"),
                point,
                footprint(lib, &m.objects[j]),
            );
            let cycle = action_ticks(lib, &o, "image_build");
            // 0x4aba10 / 0x4abd20: the first cycle comes at the picture's
            // `actionrow`, the next ones a whole `anim_time` apart, each due
            // at the previous one; a posed peasant only waits what is left
            // of `anim_time` since its last blow (0x4abbd9).
            let period = action_ms(lib, &o, "image_build");
            let lead = animation_timing(lib, &o, "image_build").map_or(period, |(t, _)| t);
            let now = sim.ticks * TICK_MS;
            let worker = &mut sim.gameplay.actors[i];
            if worker.work_target != Some(j) {
                worker.work_target = Some(j);
                if !posed {
                    worker.work_due_ms = now + lead;
                    return;
                }
            }
            if !cycle_due(sim, i, period) {
                return;
            }
            // 0x4abc72: «buildozer» adds 10 000 000 to the blow.
            let bonus = u64::from(m.cheats.instant_build) * 10_000_000;
            let blow = cycle as u64 + bonus;
            let a = &mut sim.gameplay.actors[j];
            if a.construction > 0 {
                a.construction = a.construction.saturating_sub(blow);
                let completed = a.construction_total.saturating_sub(a.construction);
                // Native 0x41e0c0 updates the construction image and HP from
                // completed TIME, at work cycle boundaries, and fills HP on completion.
                m.objects[j].health = (m.objects[j].max_health as u64 * completed
                    / a.construction_total.max(1))
                .max(1) as u16;
                if a.construction == 0 {
                    complete_site(sim, m, lib, j);
                    if building {
                        next_site(sim, m, lib, i);
                    } else {
                        next_repair(sim, m, g, lib, i);
                    }
                }
            } else if crate::selection::unit(&m.objects[j])
                && m.objects[j].health < m.objects[j].max_health
            {
                // 0x4abd5d: a machine gains 5 a cycle; a ship's fires
                // follow its health (vt+0x1d4 = 0x4202a0).
                m.objects[j].health = m.objects[j]
                    .health
                    .saturating_add(5)
                    .min(m.objects[j].max_health);
                burn(sim, m, lib, j);
            } else if m.objects[j].health < m.objects[j].max_health {
                let cycle = action_ticks(lib, &o, "image_build");
                let time = c
                    .stats(
                        &m.objects[j],
                        m.objects[j].class_name(),
                        researches(m, o.owner),
                    )
                    .time_ms
                    .max(1);
                // Native 0x4abe66: max(2, cycle_ms * maxHP / (TIME_ms * 4)).
                let amount = (cycle as u64 * TICK_MS * m.objects[j].max_health as u64
                    / time.saturating_mul(4))
                .max(2)
                .min(u16::MAX as u64) as u16;
                m.objects[j].health = m.objects[j]
                    .health
                    .saturating_add(amount)
                    .min(m.objects[j].max_health);
                burn(sim, m, lib, j);
            } else {
                next_repair(sim, m, g, lib, i);
            }
        }
        Intent::Attack(j) => {
            // Step 5 of the program: 500 ms after a failed approach the unit
            // looks for another target.
            if let Some(t) = sim.gameplay.actors[i].search_at {
                if sim.ticks < t {
                    return;
                }
                sim.gameplay.actors[i].search_at = None;
                next_target(sim, m, lib, c, i);
                return;
            }
            // A unit's own engagement keeps the relations of its filter; an
            // order does not: op 0xd (0x4e2523) checks the target's life,
            // reach and visibility, never the relations, so the Attack
            // button reaches allies and the right click trains on dummies.
            let against = if sim.gameplay.actors[i].automatic {
                auto_hostile(sim, m, i, j)
            } else {
                i != j
            };
            if !standing(m, j) || !target_known(sim, m, side(sim, m, i), j) || !against {
                next_target(sim, m, lib, c, i);
                return;
            }
            // A blow under way is judged when it lands (0x4dfab4).
            if sim.gameplay.actors[i]
                .action_image
                .starts_with("image_attack")
            {
                let strike = attack(sim, m, g, lib, c, i, j);
                struck(sim, m, lib, c, i, strike);
                return;
            }
            // Op 0x87 (vt+0x21c) and the start of op 13 (0x4e25a0): a target
            // that can be attacked, outside the minimum range, within RANGE
            // of the target's rect.
            if !combat::can_attack(sim, m, lib, c, i, j, true)
                || combat::too_close(sim, m, lib, i, j)
            {
                next_target(sim, m, lib, c, i);
                return;
            }
            let range = combat::max_range(m, c, i);
            if movement::in_range(sim, m, lib, i, j, range) {
                halt(&mut sim.gameplay.actors[i]);
                // A Murder op in the Attack program (0x4eb950) strikes while
                // the ninja has 60 mana; below, it is an Attack op again
                // (0x4eb9d0).
                if std::mem::take(&mut sim.gameplay.actors[i].murder_blow)
                    && sim.objects[i].mana >= 60
                    && crate::selection::unit(&m.objects[j])
                {
                    let rect = movement::footprint_rect(lib, &m.objects[j]);
                    let picture = murder_image(lib, &m.objects[i]);
                    face(sim, m, lib, i, rect, Some(picture));
                    murder(sim, m, g, lib, c, i, j);
                    // The Attack op that follows starts its picture again
                    // (0x4e24bc): the murder stamps no blow time.
                    sim.gameplay.actors[i].action_image.clear();
                    if !standing(m, j) {
                        next_target(sim, m, lib, c, i);
                    }
                    return;
                }
                let strike = attack(sim, m, g, lib, c, i, j);
                struck(sim, m, lib, c, i, strike);
                return;
            }
            // Out of reach: the blow animation restarts once in range again.
            sim.gameplay.actors[i].action_image.clear();
            // Step 12 (Can I move): a unit keeping its place looks elsewhere.
            if !combat::free(sim, m, i) {
                next_target(sim, m, lib, c, i);
            } else if !approach_object(sim, m, g, lib, i, j, range) && unreachable(sim, i) {
                sim.gameplay.actors[i].search_at = Some(sim.ticks + 500 / TICK_MS);
            }
        }
        Intent::Rescue(due) => {
            // Op 0x7e (0x502e18), every 1000 ms: a fighter (flags 0x10010)
            // of a kingdom 1–8 that may rescue (0x5028a0), standing in the
            // 3×3 around the prisoner (0x501a20), takes it over.
            let now = sim.ticks * TICK_MS;
            if now < due {
                return;
            }
            sim.gameplay.actors[i].intent = Some(Intent::Rescue(due + 1000));
            let (x, y) = (o.x as i32, o.y as i32);
            let rescuer = (0..m.objects.len()).find(|&k| {
                let u = &m.objects[k];
                k != i
                    && rescuing_kingdom(sim, m, u.owner)
                    && crate::selection::unit(u)
                    && standing(m, k)
                    && sim.objects[k].holder.is_none()
                    && military(sim, m, k)
                    && (u.x as i32 - x).abs() <= 1
                    && (u.y as i32 - y).abs() <= 1
            });
            if let Some(k) = rescuer {
                let owner = m.objects[k].owner;
                transfer_owner(sim, m, lib, i, owner);
            }
        }
        Intent::Guard(j) => {
            // The way back to the guarded object (0x42dd00(0, 0, G, 1, 3)):
            // within 3 cells of its rect the unit is idle again.
            if !m.objects.get(j).is_some_and(alive) || movement::in_range(sim, m, lib, i, j, 3) {
                finish(sim, i);
                return;
            }
            if !approach_object(sim, m, g, lib, i, j, 3) && unreachable(sim, i) {
                finish(sim, i);
            }
        }
        Intent::Capture(j) => {
            if !capture_target(m, lib, j) {
                finish(sim, i);
                return;
            }
            if distance(lib, &o, &m.objects[j]) > 1 {
                approach_object(sim, m, g, lib, i, j, 0);
                return;
            }
            work_animation(
                sim,
                m,
                lib,
                i,
                OrderState::Working,
                "image_capture",
                Some("@dig"),
                [m.objects[j].x, m.objects[j].y],
                footprint(lib, &m.objects[j]),
            );
            let now = sim.ticks * TICK_MS;
            let a = &mut sim.gameplay.actors[i];
            if a.capture_target != Some(j) {
                a.capture_target = Some(j);
                a.work_due_ms = now;
            }
            // 0x4dfebf: a cycle of the capturing picture's time; the op
            // plays the `dig` sound (vt+0x1c8).
            if !cycle_due(sim, i, picture_ms(lib, &o, "image_capture")) {
                return;
            }
            // 0x4dfe52: 5 against a hostile building, −5 otherwise; 0x505040
            // takes it when no more than 5 is left; 0x41fab0 ends the work
            // once the resistance is back to its target, so the peasant goes
            // on after the capture until the building is whole.
            let r = sim.objects[j].resistance;
            if auto_hostile(sim, m, i, j) {
                if r <= 5 {
                    sim.objects[j].resistance = 0;
                    transfer_owner(sim, m, lib, j, o.owner);
                } else {
                    sim.objects[j].resistance = r - 5;
                }
            } else {
                sim.objects[j].resistance = r.saturating_add(5);
            }
            let target = resistance_target(sim, m, lib, j);
            if sim.objects[j].resistance as i32 >= target {
                sim.objects[j].resistance = target.clamp(0, u16::MAX as i32) as u16;
                finish(sim, i);
            }
        }
        Intent::Bribe(j) => {
            if !bribable(m, o.owner, j) || !target_known(sim, m, o.owner, j) {
                finish(sim, i);
                return;
            }
            if distance(lib, &o, &m.objects[j]) > 1 {
                approach_object(sim, m, g, lib, i, j, 0);
                return;
            }
            // Native 0x4ac62f: gold = floor((target level + 20) * 10 / 4).
            let cost = [(sim.objects[j].level as u32 + 20) * 10 / 4, 0, 0, 0];
            match debit(c, m, o.owner, cost) {
                Ok(()) => transfer_owner(sim, m, lib, j, o.owner),
                Err(e) if sim.gameplay.notices.len() < 16 => sim.gameplay.notices.push(e),
                Err(_) => {}
            }
            finish(sim, i);
        }
        Intent::Pick(j) | Intent::PickUse(j) | Intent::Board(j) => {
            if i == j || !m.objects.get(j).is_some_and(alive) || sim.objects[j].holder.is_some() {
                finish(sim, i);
                return;
            }
            // 0x4df4d3: the walk to a holder ends as soon as it is full.
            if !carried(&m.objects[j])
                && sim.objects[j].contents.len() >= holder_capacity(sim, m, lib, j)
            {
                finish(sim, i);
                return;
            }
            // 0x4e0b5e: an item is picked up only from under the unit. A walk
            // that fails ends the program (its Move op fails), but towards a
            // ship or a carpet, which may come to meet the unit.
            if carried(&m.objects[j]) {
                if distance(lib, &o, &m.objects[j]) > 0 {
                    if !approach_inside(sim, m, g, lib, i, j) && unreachable(sim, i) {
                        finish(sim, i);
                    }
                    return;
                }
            } else if !movement::in_range(sim, m, lib, i, j, 1) {
                // Enter (program 0x15, 0x42e2b0 from the right click's action
                // 21, 0x504247): its Move asks range 1 (op+8, 0x42e318), met
                // by 0x502550 against the holder's rect (vt+0x134), so a
                // unit boards across a cell of shore.
                if !approach_object(sim, m, g, lib, i, j, 1)
                    && unreachable(sim, i)
                    && m.objects[j].building()
                {
                    finish(sim, i);
                }
                return;
            }
            let (holder, held) = if carried(&m.objects[j]) {
                (i, j)
            } else {
                (j, i)
            };
            let max = holder_capacity(sim, m, lib, holder);
            if (if carried(&m.objects[held]) {
                accepts(lib, c, &m.objects[holder], &m.objects[held])
            } else {
                boardable(sim, m, lib, c, held, holder)
            }) && sim.objects[holder].contents.len() < max
            {
                garrison(sim, m, holder, held);
                refresh_equipment(sim, m, held);
                scroll_entered(sim, m, lib, held);
                // Ctrl (program 0x41): the item is used at once, with no
                // target (0x4e0c47).
                if matches!(sim.gameplay.actors[i].intent, Some(Intent::PickUse(_))) {
                    let _ = use_held(sim, m, g, lib, c, i, j);
                }
            }
            finish(sim, i);
        }
        Intent::UseItem {
            item: j,
            target,
            cell,
        } => {
            if sim.objects[j].holder != Some(i)
                || !alive(&m.objects[j])
                || target.is_some_and(|t| !m.objects.get(t).is_some_and(alive))
            {
                finish(sim, i);
                return;
            }
            // The item's range (+0xca): 0 for potions, the spell's range less
            // one for books (0x463ce4).
            let range = match m.objects[j].spell_key() {
                Some(key) if m.objects[j].class == class::ITEM_MAGIC => {
                    spell_parameters(c, m, &o, key).0.saturating_sub(1)
                }
                _ => 0,
            };
            // Measured by 0x502550 against the target's rect, or the cell.
            let point = [
                cell[0] as i32,
                cell[1] as i32,
                cell[0] as i32,
                cell[1] as i32,
            ];
            let far = match target {
                Some(t) => !movement::in_range(sim, m, lib, i, t, range),
                None => !movement::arrived([o.x, o.y], point, range),
            };
            if far {
                match target {
                    Some(t) => approach_object(sim, m, g, lib, i, t, range),
                    None => approach(sim, m, g, lib, i, cell, range),
                };
                return;
            }
            // Opcode 0x10 (0x4e10e5): face the target, then use the item.
            match target {
                Some(t) => {
                    let size = footprint(lib, &m.objects[t]);
                    face_area(sim, m, lib, i, [m.objects[t].x, m.objects[t].y], size);
                }
                None => face_area(sim, m, lib, i, cell, [1, 1]),
            }
            match m.objects[j].class {
                class::ITEM_HEALTH => drink(sim, m, lib, c, target.unwrap_or(i), j),
                class::ITEM_MAGIC => {
                    let _ = read_book(sim, m, g, lib, c, i, j, target, Some(cell));
                }
                _ => {}
            }
            finish(sim, i);
        }
        Intent::DropItem {
            item: j,
            target,
            cell,
        } => {
            if sim.objects[j].holder != Some(i) || !alive(&m.objects[j]) {
                finish(sim, i);
                return;
            }
            let receiver = target.filter(|&t| m.objects.get(t).is_some_and(alive));
            let far = match receiver {
                Some(t) => distance(lib, &o, &m.objects[t]) > 1,
                None => [o.x, o.y] != cell,
            };
            if far {
                match receiver {
                    Some(t) => approach_object(sim, m, g, lib, i, t, 0),
                    None => approach(sim, m, g, lib, i, cell, 0),
                };
                return;
            }
            // Opcode 0xe (0x4e0fde): handed to a unit, the item first leaves
            // the holder as with no point (0x504550(-1, -1), [`way_out`]) and
            // the unit takes it from there when it has room (0x504440);
            // otherwise it lies on the chosen cell when that is still free,
            // else around the holder.
            sim.objects[i].contents.retain(|&k| k != j);
            sim.objects[j].holder = None;
            item_left(sim, m, i, j);
            m.objects[j].flags &= !FLAG_HELD;
            refresh_equipment(sim, m, j);
            let item = m.objects[j].clone();
            let here = receiver.is_none() && {
                let occupied = occupied(g, m, lib, None, false);
                let bridges = bridge_cells(g, m, lib);
                let p = [cell[0] as i32, cell[1] as i32];
                item_cell_free(g, m, lib, &occupied, &bridges, &item, p)
            };
            let spot = if here {
                Some(cell)
            } else {
                let (rect, start) = way_out(sim, m, lib, i, j);
                ring_cell(sim, g, m, lib, &item, rect, start, 8)
            };
            let Some(p) = spot else {
                sim.objects[i].contents.push(j);
                sim.objects[j].holder = Some(i);
                picked(sim, m, j);
                m.objects[j].flags |= FLAG_HELD;
                refresh_equipment(sim, m, j);
                finish(sim, i);
                return;
            };
            position(&mut m.objects[j], p);
            sim.objects[j].entry = None;
            let taken = receiver.is_some_and(|t| {
                accepts(lib, c, &m.objects[t], &m.objects[j])
                    && sim.objects[t].contents.len() < holder_capacity(sim, m, lib, t)
            });
            if let Some(t) = receiver.filter(|_| taken) {
                sim.objects[t].contents.push(j);
                sim.objects[j].holder = Some(t);
                picked(sim, m, j);
                sim.objects[j].entry = Some(p);
                m.objects[j].flags |= FLAG_HELD;
                refresh_equipment(sim, m, j);
                scroll_entered(sim, m, lib, j);
                resource_item_entered(sim, m, g, lib, j, t);
            }
            finish(sim, i);
        }
        Intent::Trade(j) => {
            if !m
                .objects
                .get(j)
                .is_some_and(|x| alive(x) && matches!(x.class_name(), "market" | "harbor"))
            {
                finish(sim, i);
                return;
            }
            if distance(lib, &o, &m.objects[j]) > 1 {
                approach_object(sim, m, g, lib, i, j, 0);
                return;
            }
            let a = &sim.gameplay.actors[i];
            let (Some(origin), Some(partner), Some(mut route)) =
                (a.trade_origin, a.trade_partner, a.route.clone())
            else {
                finish(sim, i);
                return;
            };
            if !m.objects.get(origin).is_some_and(alive)
                || !m.objects.get(partner).is_some_and(alive)
            {
                finish(sim, i);
                return;
            }
            let here = usize::from(j != origin);
            let owner = m.objects[j].owner;
            // 0x4e7ec0, 0x45b7b0: one trader at a time in a market, and none
            // in an unfinished one; it stays reserved until the trader leaves
            // (0x4e7f30).
            if route.reserved != Some(j) {
                let busy = m.objects[j].flags & FLAG_CONSTRUCTION != 0
                    || sim.gameplay.actors.iter().enumerate().any(|(k, a)| {
                        k != i
                            && a.intent == Some(Intent::Trade(j))
                            && a.route.as_ref().is_some_and(|r| r.reserved == Some(j))
                    });
                if busy {
                    return;
                }
                route.reserved = Some(j);
                // 0x4e7c98: once per stop, the cargo of the other end is
                // sold to the building's owner, with the profit in the same
                // resource; a cargo of another resource is sold without it.
                if route.cargo > 0 {
                    let mut cargo = route.cargo;
                    if route.cargo_type == route.resources[1 - here] {
                        cargo += profit(
                            route.cargo,
                            route.distance,
                            trade_bonus(m, o.owner),
                            route.map_size,
                        );
                    }
                    let mut amount = [0; 4];
                    amount[route.cargo_type as usize] = cargo;
                    credit(m, owner, amount);
                    route.cargo = 0;
                }
                sim.gameplay.actors[i].route = Some(route.clone());
            }
            if sim.ticks < route.retry {
                return;
            }
            // Each attempt takes at most min(100, 1000 / trade buildings of
            // the owner), and waits 1000 ms until the load is complete.
            let buildings = m
                .objects
                .iter()
                .filter(|x| {
                    x.owner == owner
                        && alive(x)
                        && x.flags & FLAG_CONSTRUCTION == 0
                        && matches!(x.class_name(), "market" | "harbor")
                })
                .count() as u32;
            let take = if buildings == 0 {
                100
            } else {
                (1000 / buildings).min(100)
            };
            let resource = route.resources[here];
            let want = route.amount.saturating_sub(route.cargo).min(take);
            let mut cost = [0; 4];
            cost[resource as usize] = want;
            if want > 0 && debit(c, m, owner, cost).is_ok() {
                route.cargo += want;
                route.cargo_type = resource;
            }
            if route.cargo < route.amount {
                route.retry = sim.ticks + 20;
                sim.gameplay.actors[i].route = Some(route);
                return;
            }
            route.retry = 0;
            route.reserved = None;
            let a = &mut sim.gameplay.actors[i];
            a.route = Some(route);
            a.intent = Some(Intent::Trade(if j == origin { partner } else { origin }));
            a.goal = None;
        }
    }
}
/// Native 0x4721e0 range dispatch and 0x4723e0 mana dispatch. Summoning
/// costs are supplied by the civilization's creature row, not a flat spell price.
fn spell_parameters(c: &Catalog, m: &MissionWorld, o: &MapObject, key: &str) -> (u32, u16) {
    let range = match key {
        "spell_firerain" => 14,
        "spell_lightning" | "spell_freeze" | "spell_berserk" => 12,
        // The kingdom's healing range (+0xf0): 3, or 5 after Medicine.
        "spell_heal" => {
            let medicine = researches(m, o.owner)
                .iter()
                .any(|r| r.eq_ignore_ascii_case("te_medicine"));
            if medicine { 5 } else { 3 }
        }
        "spell_spawncarpet" | "spell_spawnstonegolem" | "spell_spawndragon" | "spell_spawnjin" => 4,
        "spell_spawnbat" | "spell_whirlpool" => 16,
        "spell_magicvision" => 1024,
        "spell_turntostone" => 20,
        "spell_flamebolt" => 18,
        "spell_raiseexp" => 1,
        _ => 8,
    };
    let mana = summoned(key)
        .map(|class| c.stats(o, CLASSES[class], &[]).mana_cost.max(1))
        .unwrap_or_else(|| match key {
            "spell_firerain" => 100,
            "spell_heal" => 4,
            "spell_freeze" => 60,
            "spell_berserk" | "spell_raisedead" => 70,
            "spell_magicvision" => 30,
            "spell_raiseexp" => 10,
            "spell_cloak" if o.class == 13 => 20,
            "spell_curse" if o.class == 31 => 0,
            _ => 50,
        });
    (range, mana)
}
/// Animate (program 0x36, 0x4309f0), started on every summoned creature
/// (0x473ab3) and raised skeleton (0x4737dd): op 0x74 shows `spawn`
/// (vt+0x2c4 = 0x4ec7a0: golem, carpet, jinn, dragon) or `skeleton_rise`
/// (0x4ee890), or leaves a bat its picture, for the picture's time
/// (0x4e16df); meanwhile the creature does nothing else.
fn appear(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, j: usize) {
    let o = &m.objects[j];
    let picture = match o.class {
        16 | 22 | 30 | 32 => Some("spawn"),
        class::SKELETON => Some("skeleton_rise"),
        _ => None,
    };
    let ms = match picture {
        Some(section) => section_ms(lib, section),
        None => picture_ms(lib, o, "image_idle"),
    };
    let a = &mut sim.gameplay.actors[j];
    a.action_image = picture.unwrap_or_default().into();
    a.action_elapsed_ms = 0;
    a.appearing_until = sim.ticks + ms.div_ceil(TICK_MS).max(1);
    sim.objects[j].order = OrderState::Appearing;
}
/// Invisibility (effect 9) on unit `j` for `ticks`. A visible ninja given
/// it for 2000 ms hides on its own mana (vt+0x154 = 0x4eb840 sets +0x124):
/// it pays 1 each time the effect's timer comes, every 2000 ms, and stays
/// hidden while it can (0x4ebba9); a ninja already hidden takes nothing.
fn cloak(sim: &mut Simulation, m: &MissionWorld, j: usize, ticks: u64) {
    let a = &mut sim.gameplay.actors[j];
    let hidden = a.effect_until > sim.ticks;
    if m.objects[j].class == 13 && ticks == 2000 / TICK_MS {
        if hidden {
            return;
        }
        a.cloak_drain = true;
    }
    a.effect_until = sim.ticks + ticks;
}
/// Spell 9 cast by unit `i` on itself at once (0x472b60, from 0x4ebe10),
/// as `cast` does it.
fn hide(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, c: &Catalog, i: usize) {
    let o = &m.objects[i];
    let (_, cost) = spell_parameters(c, m, o, "spell_cloak");
    if sim.objects[i].mana < cost {
        return;
    }
    sparkles(sim, rect_around(lib, o, 4), 3, 3000);
    cloak(sim, m, i, if o.class == 13 { 40 } else { 900 });
    sim.objects[i].mana -= cost;
    let p = [o.x, o.y];
    if reveal_point(sim, m, i, p, 3) {
        sound_at(
            sim,
            p,
            "@spells/invisible.wav",
            crate::audio::category::SPELL,
        );
    } else {
        sound(
            sim,
            i,
            "@spells/invisible.wav",
            crate::audio::category::SPELL,
        );
    }
}
/// Creature of a summoning spell (0x4739c6).
fn summoned(key: &str) -> Option<usize> {
    match key {
        "spell_spawncarpet" => Some(22),
        "spell_spawnbat" => Some(15),
        "spell_spawnstonegolem" => Some(16),
        "spell_spawndragon" => Some(32),
        "spell_spawnjin" => Some(30),
        _ => None,
    }
}
/// Answers 1 to the native query 0x2719: stone golem, siege engines, carpet,
/// ships, ghost, skeleton and fishing boat, which healing, curses, freezing
/// and raising do not affect.
/// A unit the Heal program keeps healing: alive, damaged, no machine and
/// not stone (op 0x51, vt+0x230 and query 0x2719).
fn wounded(sim: &Simulation, m: &MissionWorld, j: usize) -> bool {
    m.objects.get(j).is_some_and(|x| {
        standing(m, j)
            && crate::selection::unit(x)
            && x.health < x.max_health
            && !machine(x)
            && !has_effect(sim, j, effect::STONE)
    })
}
/// Op 0x51 (0x4e2dcb): the nearest wounded unit of an allied kingdom
/// (relation 0x40) within the healer's sight.
fn damaged_near(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
) -> Option<usize> {
    let o = &m.objects[i];
    let r = sim.objects[i].sight;
    (0..m.objects.len())
        .filter(|&k| {
            let x = &m.objects[k];
            x.x.abs_diff(o.x) <= r
                && x.y.abs_diff(o.y) <= r
                && sim.objects[k].holder.is_none()
                && wounded(sim, m, k)
                && relation(m, x.owner, o.owner) & 0x40 != 0
        })
        .min_by_key(|&k| distance(lib, o, &m.objects[k]))
}
fn machine(o: &MapObject) -> bool {
    matches!(o.class, 16 | 19..=25 | 31 | 36 | class::FISHING_BOAT)
}
/// Answers 1 to the native query 0x271c: creatures that leave no body to
/// raise (bat, carpet, golem, mage, dwarf, ork, jinn, ghost, skeleton, dragon).
fn bodiless(o: &MapObject) -> bool {
    matches!(o.class, 15 | 16 | 17 | 22 | 27 | 28 | 30 | 31 | 32 | 36)
}
/// vt+0xc0 (0x4e6010): a unit a spy can take the place of, on land, neither
/// a machine (0x2719) nor an animal (0x2728) nor hiding a spy (vt+0xbc);
/// never the ninja or the messiah (0x4ba9f0).
fn impersonable(lib: &ObjectLibrary, x: &MapObject) -> bool {
    crate::selection::unit(x)
        && lib.layer(x) == 1
        && !machine(x)
        && !matches!(x.class, 13 | 26 | 38 | class::COW)
        && x.mole == 0
}
/// Answers 1 to the native query 0x2729: what never trains on the dummy
/// (bat, golem, siege engines, carpet, ships, jinn, dragon, fishing boat,
/// towers).
fn untrainable(o: &MapObject) -> bool {
    matches!(o.class, 15 | 16 | 19..=25 | 30 | 32 | 39..=43 | 136)
}
/// The alliance relation (0x467d10 with mask 0x40); a kingdom is its own ally.
fn allied(m: &MissionWorld, a: u8, b: u8) -> bool {
    a == b
        || ((1..=8).contains(&a)
            && (1..=8).contains(&b)
            && m.diplomacy
                .get((a - 1) as usize * 8 + (b - 1) as usize)
                .is_some_and(|d| d & 0x40 != 0))
}
/// Centre cell of an object's footprint (0x454dc0 measures between centres).
fn centre(lib: &ObjectLibrary, o: &MapObject) -> [u16; 2] {
    let [w, h] = footprint(lib, o);
    [o.x + (w / 2) as u16, o.y + (h / 2) as u16]
}
/// Units and buildings whose cells meet the square of `r` cells around `at`,
/// as 0x44de50 gathers them for area spells.
fn in_square(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    at: [u16; 2],
    r: u16,
) -> Vec<usize> {
    m.objects
        .iter()
        .enumerate()
        .filter(|(k, x)| {
            alive(x)
                && sim.objects[*k].holder.is_none()
                && (crate::selection::unit(x) || x.building())
                && {
                    let [w, h] = footprint(lib, x);
                    (x.x as i32) <= at[0] as i32 + r as i32
                        && x.x as i32 + w as i32 > at[0] as i32 - r as i32
                        && (x.y as i32) <= at[1] as i32 + r as i32
                        && x.y as i32 + h as i32 > at[1] as i32 - r as i32
                }
        })
        .map(|(k, _)| k)
        .collect()
}
/// Equipment (pickup 0x463480, drop 0x4634f0): a held ITEM_PLUSPLUS adds
/// both its attack bytes to the holder's stored attack word and its defence
/// word to the stored defence, as 16-bit sums; the bonus leaves with the
/// item. Curse and freeze therefore save and restore values that include it.
fn refresh_equipment(sim: &mut Simulation, m: &MissionWorld, j: usize) {
    let Some(o) = m.objects.get(j).filter(|o| o.class == class::ITEM_PLUSPLUS) else {
        return;
    };
    let now = sim.objects[j].holder;
    let before = sim.objects[j].equipped;
    if before == now {
        return;
    }
    let attack = o.byte(0x16) as u16 + o.byte(0x17) as u16;
    let defence = o.word(0x18);
    if let Some(h) = before.filter(|&h| h < sim.objects.len()) {
        let s = &mut sim.objects[h];
        s.attack = (s.attack as u16).wrapping_sub(attack) as u32;
        s.defence = (s.defence as u16).wrapping_sub(defence) as u32;
    }
    if let Some(h) = now {
        let s = &mut sim.objects[h];
        s.attack = (s.attack as u16).wrapping_add(attack) as u32;
        s.defence = (s.defence as u16).wrapping_add(defence) as u32;
    }
    sim.objects[j].equipped = now;
}
/// Whether an item may lie on a cell (0x5072a0, land layer): inside the map,
/// ground a walker can reach, no standing object and no other item.
fn item_cell_free(
    g: &MapGrid,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    occupied: &[bool],
    bridges: &[bool],
    item: &MapObject,
    p: [i32; 2],
) -> bool {
    let side = g.side as i32;
    if p[0] < 0 || p[1] < 0 || p[0] > side - 2 || p[1] > side - 2 {
        return false;
    }
    let cell = [p[0] as u16, p[1] as u16];
    let k = p[1] as usize * g.side + p[0] as usize;
    (terrain_free(g, lib, item, cell, false) || bridges[k])
        && !occupied[k]
        && !m
            .objects
            .iter()
            .any(|o| carried(o) && alive(o) && o.flags & FLAG_HELD == 0 && [o.x, o.y] == cell)
}
/// [`ring_search`] for an item: a cell where it may lie.
#[allow(clippy::too_many_arguments)]
fn ring_cell(
    sim: &mut Simulation,
    g: &MapGrid,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    item: &MapObject,
    rect: [i32; 4],
    start: Option<[i32; 2]>,
    rings: i32,
) -> Option<[u16; 2]> {
    let occupied = occupied(g, m, lib, None, false);
    let bridges = bridge_cells(g, m, lib);
    ring_search(sim, rect, start, rings, |_, p| {
        item_cell_free(g, m, lib, &occupied, &bridges, item, p)
    })
}
/// An item leaves its holder (0x504550 with no point): it lands on a free
/// cell within 8 rings ([`way_out`]); without one it stays lost with the
/// holder.
fn release_item(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    h: usize,
    j: usize,
) -> bool {
    let (rect, start) = way_out(sim, m, lib, h, j);
    let item = m.objects[j].clone();
    let cell = ring_cell(sim, g, m, lib, &item, rect, start, 8);
    sim.objects[h].contents.retain(|&k| k != j);
    sim.objects[j].holder = None;
    item_left(sim, m, h, j);
    sim.objects[j].entry = None;
    m.objects[j].flags &= !FLAG_HELD;
    refresh_equipment(sim, m, j);
    match cell {
        Some(p) => {
            position(&mut m.objects[j], p);
            true
        }
        None => {
            m.objects[j].flags |= FLAG_INACTIVE;
            false
        }
    }
}
/// Health potion (0x464ac0) on `t`: permanent gains; health and mana raise
/// their maximum first, then fill by the same amount, except health on a
/// petrified unit (0x504d00 does not heal stone).
fn drink(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    t: usize,
    j: usize,
) {
    let item = m.objects[j].clone();
    let (health, mana) = (item.word(0x12), item.word(0x24));
    let stone = has_effect(sim, t, effect::STONE);
    let x = &mut m.objects[t];
    x.max_health = x.max_health.saturating_add(health);
    if !stone {
        x.health = x.health.saturating_add(health).min(x.max_health);
    }
    let s = &mut sim.objects[t];
    s.attack = s.attack.saturating_add(item.word(0x16) as u32);
    s.defence = s.defence.saturating_add(item.word(0x18) as u32);
    s.max_mana = s.max_mana.saturating_add(mana);
    s.mana = s.mana.saturating_add(mana).min(s.max_mana);
    // The payload word is experience, not levels (0x464b8e).
    add_experience(sim, m, lib, c, t, item.word(0x1c), true);
    // 0x464b94: the healing or the blessing sound, then ten stars for 2 s
    // over the cells around the drinker.
    let sound = if health != 0 {
        "@spells/heal.wav"
    } else {
        "@spells/bless.wav"
    };
    sound_here(sim, t, sound, crate::audio::category::SPELL);
    let (x, y) = (m.objects[t].x as i32, m.objects[t].y as i32);
    sparkles(
        sim,
        [(x - 1) * 32, (y - 1) * 32, (x + 2) * 32, (y + 1) * 32],
        10,
        2000,
    );
    // 0x464c40: the holder destroys the potion (vt+0x1b0, 0x5047f0), which
    // hears it left (0x504804) while still in the list: a potion first in
    // the list takes the mark away with it.
    if let Some(h) = sim.objects[j].holder.take() {
        item_left(sim, m, h, j);
        sim.objects[h].contents.retain(|&k| k != j);
    }
    consume(sim, m, j);
}
/// An item used up (0x4ff350 on a held item; books, soul gems, sales): it
/// leaves the inventory.
fn consume(sim: &mut Simulation, m: &mut MissionWorld, j: usize) {
    if let Some(h) = sim.objects[j].holder.take() {
        // 0x500360 → the holder's vt+0x1ac (0x504550): out of the list,
        // then the item hears it left (0x504799).
        sim.objects[h].contents.retain(|&k| k != j);
        item_left(sim, m, h, j);
    }
    refresh_equipment(sim, m, j);
    sim.gameplay.actors[j] = Default::default();
    m.objects[j].flags = (m.objects[j].flags & !FLAG_HELD) | FLAG_INACTIVE;
}
/// Book (0x4640f0). Without a target the holder learns the spell when it has
/// a mana pool and does not know it, and the book is spent. With a target a
/// full book casts for free, spends a charge and empties its mana; a book
/// with no charge left disappears.
#[allow(clippy::too_many_arguments)]
fn read_book(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    j: usize,
    target: Option<usize>,
    cell: Option<[u16; 2]>,
) -> std::result::Result<(), String> {
    let item = m.objects[j].clone();
    let spell = item.byte(96);
    let key = item.spell_key().ok_or("Libro non riconosciuto")?;
    if target.is_none() && cell.is_none() {
        if sim.objects[i].max_mana == 0 || knows_spell(sim, m, c, lib, i, key) {
            return Err("Questa unità non può imparare l'incantesimo".into());
        }
        sim.gameplay.actors[i].spells.push(spell);
        consume(sim, m, j);
        return Ok(());
    }
    let finite = item.byte(99) != 0;
    let book = &sim.objects[j];
    if book.mana < book.max_mana {
        return Err("Il libro non ha ancora ricaricato il mana".into());
    }
    if finite && item.word(102) == 0 {
        consume(sim, m, j);
        return Err("Il libro non ha più cariche".into());
    }
    let (_, cost) = spell_parameters(c, m, &m.objects[i], key);
    let previous = sim.objects[i].mana;
    sim.objects[i].mana = sim.objects[i].mana.max(cost);
    let mut req = Request::command(key, vec![i]);
    req.cell = cell.or(target.map(|t| [m.objects[t].x, m.objects[t].y]));
    req.target = target;
    let result = cast(sim, m, g, lib, c, i, key, &req);
    sim.objects[i].mana = previous;
    result?;
    // 0x4641ae: a cast from a book ends the holder's invisibility (effect
    // 9), whatever the spell, its own invisibility included.
    sim.gameplay.actors[i].effect_until = 0;
    sim.objects[j].mana = 0;
    if finite {
        let left = item.word(102).saturating_sub(1);
        let o = &mut m.objects[j];
        o.data.resize(o.data.len().max(104), 0);
        o.data[102..104].copy_from_slice(&left.to_le_bytes());
        if left == 0 {
            consume(sim, m, j);
        } else if left == 1
            && ai_managed(sim, m, i)
            && sim.objects[i].max_mana > 0
            && !knows_spell(sim, m, c, lib, i, key)
        {
            // 0x464206: at its last charge a computer's unit learns it.
            sim.gameplay.actors[i].spells.push(spell);
            consume(sim, m, j);
        }
    }
    Ok(())
}
/// An item used with no target (item->0x2bc(0, 0, 0)): a potion is drunk by
/// its holder, a book teaches its spell, a scroll shows its text.
#[allow(clippy::too_many_arguments)]
fn use_held(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    j: usize,
) -> std::result::Result<(), String> {
    match m.objects[j].class {
        class::ITEM_HEALTH => drink(sim, m, lib, c, i, j),
        class::ITEM_MAGIC => read_book(sim, m, g, lib, c, i, j, None, None)?,
        // 0x4639b0: the scroll's text in the story screen; it stays held.
        class::ITEM_SCROLL => scroll_story(sim, m, lib, j, false),
        _ => {}
    }
    Ok(())
}
/// A scroll's text (record 0x60, 0x4c34c0) in the story window: on entering
/// a holder (0x4638c4) with the record's closing, when used (0x4639b0)
/// never closing by itself.
fn scroll_story(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    j: usize,
    entered: bool,
) {
    let item = &m.objects[j];
    // The story screen with the scroll's own record: text at 0x60, picture
    // 0x460, voice 0x4a0, music 0x4e0, time 0x520, closing 0x524.
    let string = |at: usize| {
        let bytes = item.data.get(at..at + 64).unwrap_or(&[]);
        crate::assets::legacy_text(bytes.split(|b| *b == 0).next().unwrap_or(&[]))
    };
    let (image, sound, music) = (string(0x460), string(0x4a0), string(0x4e0));
    let ms = if item.data.len() >= 0x528 {
        item.dword(0x520) as u64
    } else {
        0
    };
    let close = entered && item.data.len() >= 0x528 && item.dword(0x524) != 0;
    m.message = Some(crate::script::Message {
        title: lib.label(item),
        text: lib.item_description(item),
        portrait: image,
        choices: Vec::new(),
    });
    let st = &mut sim.gameplay.script;
    st.modal = true;
    st.outbox.push(Show::Story {
        sound,
        ms,
        close,
        music,
    });
}
/// A scroll entering any holder (0x463850) opens its story at once when its
/// record's dword 0x520 (+0x58e, 0x4637e5) is not -1.
fn scroll_entered(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary, j: usize) {
    let o = &m.objects[j];
    if o.class == class::ITEM_SCROLL && o.data.len() >= 0x524 && o.dword(0x520) != u32::MAX {
        scroll_story(sim, m, lib, j, true);
    }
}
/// Whether unit `i` has spell `key` among its orders: those of its class or
/// learned from a book (0x238, the unit's own menu).
fn knows_spell(
    sim: &Simulation,
    m: &MissionWorld,
    c: &Catalog,
    lib: &ObjectLibrary,
    i: usize,
    key: &str,
) -> bool {
    c.permitted(key, &m.objects[i], lib)
        || sim.gameplay.actors[i]
            .spells
            .iter()
            .any(|&s| world::spell_key(s) == Some(key))
}
fn item_command(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    req: &Request,
) -> std::result::Result<String, String> {
    let j = req
        .selected
        .iter()
        .copied()
        .find(|&j| m.objects.get(j).is_some_and(|o| carried(o) && alive(o)))
        .ok_or("Seleziona un oggetto nell'inventario")?;
    let holder = sim.objects[j].holder;
    if holder.is_some_and(|i| !m.controls(m.objects[i].owner)) {
        return Err("L'inventario non appartiene al tuo regno".into());
    }
    let action = c.action(&req.order);
    // The inventory's click, Ctrl+click and right click (vt+0x2b4,
    // vt+0x2b8, `item_use_instant` at 0x4a1c58) use or drop any item,
    // whatever its menu.
    let gesture = matches!(
        action.as_str(),
        "item_use" | "item_use_instant" | "item_drop"
    );
    if !gesture && !c.permitted(&req.order, &m.objects[j], lib) {
        return Err("Ordine non disponibile per questo oggetto".into());
    }
    // Soul gem commands (0x465340) act on the bound unit, the gem held or
    // on the ground (anyone's to command, 0x506c30); the holder, if any,
    // strikes the blow. The gem stays.
    if matches!(action.as_str(), "item_kill" | "item_berserk") {
        let bound = sim.gameplay.actors[j]
            .bound
            .filter(|&k| m.objects.get(k).is_some_and(alive))
            .ok_or("L'anima legata non esiste più")?;
        if action == "item_kill" {
            apply_damage(sim, m, g, lib, c, bound, 10000, 3, holder);
        } else {
            discord(sim, m, lib, bound);
        }
        return Ok("Oggetto utilizzato".into());
    }
    let i = holder.ok_or("L'oggetto deve essere raccolto")?;
    let item = m.objects[j].clone();
    // The cursor's choice: an object under the pointer, or a cell (0x462d60).
    let chosen = req
        .target
        .filter(|&t| t != j && m.objects.get(t).is_some_and(alive));
    let cell = req
        .cell
        .or(chosen.map(|t| [m.objects[t].x, m.objects[t].y]));
    match action.as_str() {
        "item_drop" => {
            if let Some(cell) = cell {
                // 0x462760: a unit, else a finished building, that accepts
                // the item (a resource item enters the castle, the lumber
                // mill and the storage), else a passable cell without
                // another item.
                let unit = chosen.filter(|&t| {
                    t != i
                        && (crate::selection::unit(&m.objects[t])
                            || (m.objects[t].building()
                                && m.objects[t].flags & FLAG_CONSTRUCTION == 0))
                });
                if let Some(t) = unit {
                    if !accepts(lib, c, &m.objects[t], &item) {
                        return Err("Questa unità non può prendere l'oggetto".into());
                    }
                } else {
                    let occupied = occupied(g, m, lib, None, false);
                    let bridges = bridge_cells(g, m, lib);
                    let p = [cell[0] as i32, cell[1] as i32];
                    if !item_cell_free(g, m, lib, &occupied, &bridges, &item, p) {
                        return Err("Non si può lasciare l'oggetto qui".into());
                    }
                }
                let intent = Intent::DropItem {
                    item: j,
                    target: unit,
                    cell,
                };
                set_order(sim, i, intent, req.append)?;
                return Ok("Ordine impartito".into());
            }
            // With no point it leaves as from 0x504550(-1, -1) ([`way_out`]).
            let (rect, start) = way_out(sim, m, lib, i, j);
            let p = ring_cell(sim, g, m, lib, &item, rect, start, 8)
                .ok_or("Nessuno spazio per lasciare l'oggetto")?;
            sim.objects[i].contents.retain(|&k| k != j);
            sim.objects[j].holder = None;
            item_left(sim, m, i, j);
            sim.objects[j].entry = None;
            m.objects[j].flags &= !FLAG_HELD;
            refresh_equipment(sim, m, j);
            position(&mut m.objects[j], p);
        }
        "item_use" | "item_use_instant" => match item.class {
            // Potions and books choose a target that the holder walks to
            // (program 17); with none the holder uses them itself (0x2bc(0,0,0)).
            class::ITEM_HEALTH | class::ITEM_MAGIC if cell.is_some() => {
                // 0x464a60: a potion needs a unit under the pointer.
                if item.class == class::ITEM_HEALTH
                    && !chosen.is_some_and(|t| crate::selection::unit(&m.objects[t]))
                {
                    return Err("Scegli un'unità".into());
                }
                let intent = Intent::UseItem {
                    item: j,
                    target: chosen,
                    cell: cell.unwrap_or_default(),
                };
                set_order(sim, i, intent, req.append)?;
                return Ok("Ordine impartito".into());
            }
            class::ITEM_HEALTH | class::ITEM_MAGIC | class::ITEM_SCROLL => {
                use_held(sim, m, g, lib, c, i, j)?
            }
            class::ITEM_PLUSPLUS => return Ok("Equipaggiamento già attivo".into()),
            // The soul gem's use is empty (vt+0x2bc = 0x50b390).
            class::ITEM_SOUL => return Ok(String::new()),
            _ => return Err("Questo oggetto non si usa".into()),
        },
        _ => return Err("Ordine dell'oggetto non disponibile".into()),
    }
    Ok("Oggetto utilizzato".into())
}
/// Cast a spell at `req.cell` / `req.target`. `Ok(true)` asks for another
/// cast after `image_spell` (healing a still wounded unit).
#[allow(clippy::too_many_arguments)]
fn cast(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    key: &str,
    req: &Request,
) -> std::result::Result<bool, String> {
    let o = m.objects[i].clone();
    let p = req.cell.unwrap_or([o.x, o.y]);
    if p[0] as usize >= g.side || p[1] as usize >= g.side {
        return Err("Punto fuori mappa".into());
    }
    let (range, cost) = spell_parameters(c, m, &o, key);
    if sim.objects[i].mana < cost {
        return Err(c.notmanna.clone());
    }
    let target = req.target;
    // 0x472d3a: between centres, the range plus one, plus half the width of
    // an object target.
    let from = centre(lib, &o);
    let (to, half) = match target.and_then(|j| m.objects.get(j)) {
        Some(t) => (centre(lib, t), footprint(lib, t)[0] as u32 / 2),
        None => (p, 0),
    };
    if from[0].abs_diff(to[0]).max(from[1].abs_diff(to[1])) as u32 > range + 1 + half {
        return Err("Bersaglio fuori portata".into());
    }
    if target.is_some_and(|j| !target_known(sim, m, o.owner, j)) {
        return Err("Bersaglio non visibile o non disponibile".into());
    }
    let unit_target = |m: &MissionWorld| -> std::result::Result<usize, String> {
        let j = target.ok_or("Seleziona un bersaglio per l'incantesimo")?;
        m.objects
            .get(j)
            .filter(|x| alive(x) && crate::selection::unit(x))
            .map(|_| j)
            .ok_or_else(|| "Seleziona un'unità".into())
    };
    // Units inside a square of `r` cells around a point.
    let around = |sim: &Simulation, m: &MissionWorld, at: [u16; 2], r: u16| -> Vec<usize> {
        m.objects
            .iter()
            .enumerate()
            .filter(|(j, x)| {
                alive(x)
                    && crate::selection::unit(x)
                    && sim.objects[*j].holder.is_none()
                    && x.x.abs_diff(at[0]) <= r
                    && x.y.abs_diff(at[1]) <= r
            })
            .map(|(j, _)| j)
            .collect()
    };
    let mut cost = cost;
    let mut again = false;
    let now = sim.ticks;
    match key {
        "spell_heal" => {
            // 0x472ff1: one unit of any kingdom, +6 HP per cast (0x504d00).
            let j = unit_target(m)?;
            if machine(&m.objects[j]) {
                return Err("Non si applica alle macchine".into());
            }
            let stone = has_effect(sim, j, effect::STONE);
            let x = &mut m.objects[j];
            if !stone {
                x.health = x.health.saturating_add(6).min(x.max_health);
            }
            again = !stone && x.health < x.max_health;
            overlay(sim, m, lib, j, "healing");
        }
        "spell_berserk" => {
            // Discord (0x473250): every unit in x±2, y±2 but the caster joins
            // the special kingdom 10 for 20 s, then returns to its own.
            for j in around(sim, m, p, 2) {
                if j != i {
                    overlay(sim, m, lib, j, "curse");
                    discord(sim, m, lib, j);
                }
            }
        }
        "spell_bless" => {
            // Effect 5: units not hostile to the caster within ±4, 30 s. The
            // bonus goes straight into the attack (0x4e5809) and comes off at
            // the end (0x4e5ad7).
            for j in around(sim, m, p, 4) {
                if !hostile(sim, m, i, j) {
                    overlay(sim, m, lib, j, "bless");
                    if !has_effect(sim, j, effect::BLESS) {
                        sim.objects[j].attack = sim.objects[j].attack.saturating_add(5);
                    }
                    add_timed_effect(
                        &mut sim.gameplay.actors[j],
                        Effect {
                            until: now + 600,
                            kind: effect::BLESS,
                            ..Default::default()
                        },
                    );
                }
            }
        }
        "spell_curse" => {
            // Effect 20: hostile units within the caster's sight, 30 s. The
            // curse saves attack and defence (0x4e5841), halves the attack,
            // zeroes the defence and puts the saved values back at the end.
            let r = sim.objects[i].sight;
            for j in around(sim, m, p, r) {
                if hostile(sim, m, i, j) && !machine(&m.objects[j]) {
                    overlay(sim, m, lib, j, "curse");
                    if immune(&m.objects[j], effect::CURSE) {
                        continue;
                    }
                    let cursed = has_effect(sim, j, effect::CURSE);
                    let s = &mut sim.objects[j];
                    let saved = if cursed {
                        0
                    } else {
                        let saved = ((s.defence.min(0xffff) << 16) | s.attack.min(0xffff)) + 1;
                        s.attack /= 2;
                        s.defence = 0;
                        saved
                    };
                    add_timed_effect(
                        &mut sim.gameplay.actors[j],
                        Effect {
                            until: now + 600,
                            kind: effect::CURSE,
                            saved,
                            ..Default::default()
                        },
                    );
                }
            }
        }
        "spell_freeze" => {
            // A freeze bolt (class 124) flies at 700 px/s; the freezing
            // happens where it lands (0x432590).
            launch(sim, m, lib, i, i, p, missile::FREEZE, 700, 0);
        }
        "spell_turntostone" => {
            // Effect 14 for 120–300 s: no actions, immune to damage and healing.
            let j = unit_target(m)?;
            let r = rect_around(lib, &m.objects[j], 0);
            flash(
                sim,
                Flash {
                    image: "turn_to_stone",
                    at: [(r[0] + r[2]) / 2, (r[1] + r[3]) / 2],
                    fps: 0,
                    loops: 1,
                    prio: 200,
                    hold_ms: 0,
                    glow: 0,
                    column: 0,
                    delay_ms: 0,
                },
            );
            // 0x473913: Random(120000, 300000) ms.
            let until =
                now + (sim.gameplay.game_rng.range(120_000, 300_000) as u64).div_ceil(TICK_MS);
            if !immune(&m.objects[j], effect::STONE) {
                let a = &mut sim.gameplay.actors[j];
                add_timed_effect(
                    a,
                    Effect {
                        until,
                        kind: effect::STONE,
                        ..Default::default()
                    },
                );
                a.frozen_until = a.frozen_until.max(until);
            }
        }
        "spell_cloak" => {
            // Invisibility of the target unit (0x4734a1), 45 s; 2 s when cast
            // by a ninja.
            let j = unit_target(m)?;
            let r = rect_around(lib, &m.objects[j], 4);
            sparkles(sim, r, 3, 3000);
            cloak(sim, m, j, if o.class == 13 { 40 } else { 900 });
        }
        "spell_magicvision" => {
            // The target becomes an observer of the caster's allies for 60 s.
            if sim.gameplay.visions.len() >= 1000 {
                return Err("Troppe visioni magiche attive".into());
            }
            if let Some(j) = target.filter(|&j| m.objects.get(j).is_some_and(alive)) {
                sim.gameplay.visions.push(Vision {
                    cell: [m.objects[j].x, m.objects[j].y],
                    until: now + 1200,
                    object: Some(j),
                    owner: o.owner,
                    ..Default::default()
                });
            }
            // 0x4735ac: 16 stars over a 128-pixel square around the point.
            let (cx, cy) = (p[0] as i32 * 32 + 16, p[1] as i32 * 32 + 16);
            sparkles(sim, [cx - 64, cy - 64, cx + 63, cy + 63], 16, 3000);
        }
        "spell_hypnotize" => {
            // Effect 13 for 30 s: the unit follows the caster's relations.
            let j = unit_target(m)?;
            overlay(sim, m, lib, j, "hypnotise");
            if !immune(&m.objects[j], effect::HYPNOSIS) {
                // 0x473887: the kingdoms commanding the caster command the
                // unit, the others no longer (0x4e59b2).
                let mask: Vec<bool> = (1..=8).map(|k| commanded_by(m, i, k)).collect();
                for (k, mine) in (1..=8).zip(mask) {
                    set_control(sim, m, j, k, if mine { 1 } else { 2 });
                }
                let allegiance = side(sim, m, i);
                let a = &mut sim.gameplay.actors[j];
                add_timed_effect(
                    a,
                    Effect {
                        until: now + 600,
                        kind: effect::HYPNOSIS,
                        ..Default::default()
                    },
                );
                a.allegiance = Some(allegiance);
                a.intent = None;
                a.automatic = false;
                a.resume = None;
                a.path.clear();
            }
        }
        "spell_soulbind" => {
            // 0x473403: the target keeps its owner; a soul gem bound to it
            // appears in the caster's inventory.
            let j = unit_target(m)?;
            if sim.objects[i].contents.len() >= holder_capacity(sim, m, lib, i) {
                return Err("L'inventario è pieno".into());
            }
            let r = rect_around(lib, &m.objects[j], 4);
            sparkles(sim, r, 3, 3000);
            let gem = new_unit(sim, m, lib, class::ITEM_SOUL, 0, 0, [o.x, o.y])?;
            sim.gameplay.actors[gem].bound = Some(j);
            sim.objects[gem].holder = Some(i);
            sim.objects[gem].entry = Some([o.x, o.y]);
            sim.objects[i].contents.push(gem);
            picked(sim, m, gem);
            m.objects[gem].flags |= FLAG_HELD;
        }
        "spell_raiseexp" => {
            // Teaching (0x473bf0): an Asian peasant, monk, ninja or priest of
            // a lower level than the caster.
            let j = unit_target(m)?;
            let x = &m.objects[j];
            if x.civilization != 3
                || !matches!(x.class, 1 | 12 | 13 | 18)
                || sim.objects[j].level >= sim.objects[i].level
            {
                return Err("L'unità non può imparare da questo insegnante".into());
            }
            overlay(sim, m, lib, j, "convert_exp");
            add_experience(sim, m, lib, c, j, 1, false);
        }
        "spell_lightning" => {
            let j = target.ok_or("Seleziona un bersaglio per l'incantesimo")?;
            // 0x472e83: three bolts (class 101) from the centre of the
            // caster's cell to that of the target's, 700 ms each; segments
            // of 4 and steps of 3 while at most nine bolts live, else 15 and
            // 6; the first leaves the path more and sheds its outer lines
            // when many show. Each draws its first wander from the stream of
            // pictures as it is made (0x46d2d8).
            let now = sim.ticks;
            sim.gameplay.lightning.retain(|&until| until > now);
            let from = [o.x as i32 * 32 + 16, o.y as i32 * 32 + 16];
            let to = [p[0] as i32 * 32 + 16, p[1] as i32 * 32 + 16];
            for k in 0..3 {
                let busy = sim.gameplay.lightning.len() > 9;
                let bolt = crate::bolt::Bolt {
                    start: from,
                    end: to,
                    segment: if busy { 15 } else { 4 },
                    step: if busy { 6 } else { 3 },
                    away: if k == 0 { 9 } else { 5 },
                    back: 1,
                    ms: 700,
                    branches: if sim.gameplay.lightning.len() + 1 > 45 {
                        0
                    } else {
                        10
                    },
                    lean: k == 0,
                    ends: 0x001f,
                    middle: 0x7fff,
                };
                let (mut shape, nx, ny) = crate::bolt::shape(&bolt);
                crate::bolt::wander(&bolt, &mut shape, nx, ny, |lo, hi| {
                    sim.gameplay.aux_rng.range(lo, hi)
                });
                // The timer ends it after 700 ms and one more tick (0x46d510).
                sim.gameplay.lightning.push(now + 800 / TICK_MS);
                if sim.bolts.len() < 64 {
                    sim.bolts.push(crate::bolt::Strike {
                        bolt,
                        shape,
                        normal: (nx, ny),
                    });
                }
            }
            apply_damage(sim, m, g, lib, c, j, 150, 3, Some(i));
        }
        "spell_firerain" => {
            // 0x472ddb: twenty fireballs, one every 170 ms, each landing in a
            // random cell around the point (±64 pixels) about 375 ms later.
            let now_ms = now * TICK_MS;
            for k in 1..=20u64 {
                let px = p[0] as i64 * 32 + 64 - random(sim, 129) as i64;
                let py = p[1] as i64 * 32 + 64 - random(sim, 129) as i64;
                let cell = [
                    (px.max(0) / 32).min(g.side as i64 - 1) as u16,
                    (py.max(0) / 32).min(g.side as i64 - 1) as u16,
                ];
                let lands = (now_ms + 170 * k + 375).div_ceil(TICK_MS);
                let before = sim.gameplay.projectiles.len();
                push_projectile(
                    sim,
                    i,
                    usize::MAX,
                    40,
                    cell,
                    missile::FIRE,
                    lands,
                    [px as i32, py as i32],
                );
                // Each fireball is made when its turn comes (0x450750),
                // and only then shows and falls.
                if sim.gameplay.projectiles.len() > before
                    && let Some(p) = sim.gameplay.projectiles.last_mut()
                {
                    p.launched = (now_ms + 170 * k).div_ceil(TICK_MS);
                }
            }
        }
        "spell_flamebolt" => {
            // 0x473b44: a flame ball to the centre of the target's cell.
            let j = target.ok_or("Seleziona un bersaglio per l'incantesimo")?;
            let at = centre(lib, &m.objects[j]);
            launch(sim, m, lib, i, j, at, missile::FLAME, 700, 60);
        }
        "spell_whirlpool" => {
            // 0x4733c4: a 3×3 whirlpool centred on the point, on water only.
            let corner = [p[0].saturating_sub(1), p[1].saturating_sub(1)];
            if !water_square(g, corner) {
                return Err("Il vortice si forma solo sull'acqua".into());
            }
            let w = new_unit(sim, m, lib, class::WHIRLPOOL, 0, 0, corner)?;
            let a = &mut sim.gameplay.actors[w];
            a.bound = Some(i);
            a.moves_left = 15;
            a.attack_due_ms = now * TICK_MS + 250;
            a.attack_wait_ms = now * TICK_MS + 2000;
        }
        "spell_raisedead" => {
            // 0x47367c: a skeleton on the cell of each body whose cells meet
            // the caster's footprint widened by 8; the body goes. The mana is
            // paid even when no body is found. A body (0x44edd0) is a dead
            // land unit (`teren` 1), not a machine, a ghost or a cow.
            let [w, h] = footprint(lib, &o);
            let area = [
                o.x as i32 - 8,
                o.y as i32 - 8,
                o.x as i32 + w as i32 + 8,
                o.y as i32 + h as i32 + 8,
            ];
            let corpses: Vec<_> = m
                .objects
                .iter()
                .enumerate()
                .filter(|(_, x)| {
                    let [xw, xh] = footprint(lib, x);
                    x.flags & FLAG_DYING != 0
                        && world::class::is_unit(x.class)
                        && !machine(x)
                        && !bodiless(x)
                        && x.class != class::COW
                        && lib.layer(x) == 1
                        && (x.x as i32) < area[2]
                        && x.x as i32 + xw as i32 > area[0]
                        && (x.y as i32) < area[3]
                        && x.y as i32 + xh as i32 > area[1]
                })
                .map(|(j, x)| (j, [x.x, x.y]))
                .collect();
            let mut probe = o.clone();
            probe.class = class::SKELETON;
            for (corpse, at) in corpses {
                if free_near(g, m, lib, &probe, at, Some(corpse)) == Some(at) {
                    remove_corpse(sim, m, corpse);
                    let raised =
                        new_unit(sim, m, lib, class::SKELETON, o.owner, o.civilization, at)?;
                    appear(sim, m, lib, raised);
                    let r = rect_around(lib, &m.objects[raised], 4);
                    sparkles(sim, r, 3, 2000);
                }
            }
        }
        _ => {
            let Some(class) = summoned(key) else {
                return Err(format!("Incantesimo {key} da completare"));
            };
            // 0x472409: the mage spends its mana in multiples of the creature's
            // cost and calls one creature per multiple.
            let count = (sim.objects[i].mana / cost.max(1)).max(1);
            cost = count * cost;
            let mut probe = o.clone();
            probe.class = class;
            for _ in 0..count {
                let cell = free_near(g, m, lib, &probe, p, None)
                    .ok_or("Nessuno spazio per l'evocazione")?;
                let summoned = new_unit(sim, m, lib, class, o.owner, o.civilization, cell)?;
                appear(sim, m, lib, summoned);
            }
        }
    }
    sim.objects[i].mana -= cost.min(sim.objects[i].mana);
    // Casting reveals an invisible caster.
    if key != "spell_cloak" {
        sim.gameplay.actors[i].effect_until = 0;
    }
    let effect = match key {
        "spell_heal" => Some("@spells/heal.wav"),
        "spell_bless" => Some("@spells/bless.wav"),
        "spell_berserk" => Some("@spells/confusion.wav"),
        "spell_freeze" => Some("@spells/freeze.wav"),
        "spell_turntostone" => Some("@spells/turn to stone.wav"),
        "spell_cloak" => Some("@spells/invisible.wav"),
        "spell_lightning" => Some("@spells/lightning.wav"),
        "spell_flamebolt" => Some("@spells/flame ball.wav"),
        "spell_whirlpool" => Some("@spells/whirlpool.wav"),
        "spell_magicvision" => Some("@spells/reveal.wav"),
        "spell_raiseexp" => Some("@spells/teach.wav"),
        "spell_raisedead" => Some("@spells/ressurect.wav"),
        "spell_curse" => Some("@spells/curse.wav"),
        _ if summoned(key).is_some() => Some("@summon_spell"),
        _ => None,
    };
    // 0x472ab0: aimed beyond the caster's sight less r, a spell leaves a
    // fog revealer (class 77) of radius r at the point for 4000 ms
    // (0x435069), and its sound comes from there.
    let radius = match key {
        "spell_magicvision" => 8,
        "spell_firerain" | "spell_lightning" | "spell_bless" | "spell_curse" | "spell_berserk"
        | "spell_freeze" | "spell_whirlpool" | "spell_cloak" | "spell_hypnotize"
        | "spell_turntostone" | "spell_flamebolt" => 3,
        _ => 0,
    };
    let revealed = radius > 0 && reveal_point(sim, m, i, p, radius);
    if let Some(effect) = effect {
        // Spell sounds take category 13, the bolts and stone 19 (0x473fd9
        // to 0x473c7a); most are heard where they were cast (0xfffe).
        let category = match key {
            "spell_lightning" | "spell_turntostone" | "spell_flamebolt" | "spell_freeze" => {
                crate::audio::category::BOLT
            }
            _ => crate::audio::category::SPELL,
        };
        if revealed {
            sound_at(sim, p, effect, category);
            return Ok(again);
        }
        match key {
            "spell_lightning" | "spell_turntostone" | "spell_flamebolt" => {
                sound(sim, i, effect, crate::audio::category::BOLT)
            }
            "spell_freeze" => sound_here(sim, i, effect, crate::audio::category::BOLT),
            "spell_cloak" => sound(sim, i, effect, crate::audio::category::SPELL),
            _ => sound_here(sim, i, effect, crate::audio::category::SPELL),
        }
    }
    Ok(again)
}
/// The fog revealer of a spell (0x472ab0): when the point lies outside the
/// circle of the caster's sight less `radius` (squared distances from its
/// cell), the caster's kingdom sees `radius` cells around it for 4000 ms.
fn reveal_point(
    sim: &mut Simulation,
    m: &MissionWorld,
    i: usize,
    p: [u16; 2],
    radius: u16,
) -> bool {
    let o = &m.objects[i];
    let sight = sim.objects[i].sight;
    let (dx, dy) = (o.x as i32 - p[0] as i32, o.y as i32 - p[1] as i32);
    let reach = sight as i32 - radius as i32;
    if sight < radius || reach * reach >= dx * dx + dy * dy || sim.gameplay.visions.len() >= 1000 {
        return false;
    }
    sim.gameplay.visions.push(Vision {
        cell: p,
        radius,
        until: sim.ticks + 4000 / TICK_MS,
        owner: o.owner,
        ..Default::default()
    });
    sim.refresh_visibility(m, None);
    true
}

/// Execute every 50 ms step once. No autonomous strategic decisions, movement,
/// production or attacks are created for the other kingdoms.
pub fn advance(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    dt: Duration,
    speed: u8,
    weather: bool,
) {
    advance_with_diplomacy(sim, m, g, lib, c, dt, speed, weather, None)
}
/// The passage of a gate by its orientation (0x457980): its door and the
/// cells in front, three by three; orientation 1 reaches two cells north of
/// a stone gate (+0x144), one of a wooden one.
fn gate_passage(o: &MapObject) -> [i32; 4] {
    let (x, y) = (o.x as i32, o.y as i32);
    match o.byte(56) & 3 {
        0 => [x + 2, y, x + 4, y + 2],
        1 if o.class == class::GATE_STONE => [x + 2, y - 2, x + 4, y],
        1 => [x + 2, y - 1, x + 4, y],
        2 => [x - 2, y + 1, x, y + 3],
        _ => [x, y + 1, x + 2, y + 3],
    }
}
fn in_rect(x: &MapObject, r: [i32; 4], d: i32) -> bool {
    let (px, py) = (x.x as i32, x.y as i32);
    px >= r[0] - d && px <= r[2] + d && py >= r[1] - d && py <= r[3] + d
}
/// 0x4574e0 (vt+0x2ec): no land unit holds a cell of the passage (bit 24).
fn gate_passage_free(sim: &Simulation, m: &MissionWorld, i: usize) -> bool {
    let passage = gate_passage(&m.objects[i]);
    let cells = &sim.gameplay.pathing.cells;
    !(passage[1]..=passage[3]).any(|y| {
        (passage[0]..=passage[2])
            .any(|x| cells.inside([x, y]) && cells.at([x, y]) & pathing::OCCUPIED[1] != 0)
    })
}
/// The land units and buildings other than the gate standing in its
/// passage (0x457950, mask 0x30).
fn gate_passage_objects(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
) -> Vec<usize> {
    let passage = gate_passage(&m.objects[i]);
    (0..m.objects.len())
        .filter(|&j| {
            let x = &m.objects[j];
            j != i
                && standing(m, j)
                && sim.objects[j].holder.is_none()
                && lib.layer(x) == 1
                && if crate::selection::unit(x) {
                    in_rect(x, passage, 0)
                } else {
                    let r = movement::footprint_rect(lib, x);
                    x.building()
                        && r[0] <= passage[2]
                        && r[2] >= passage[0]
                        && r[1] <= passage[3]
                        && r[3] >= passage[1]
                }
        })
        .collect()
}
/// 0x457560 (vt+0x2e8): something stands in the passage.
fn gate_passage_held(sim: &Simulation, m: &MissionWorld, lib: &ObjectLibrary, i: usize) -> bool {
    !gate_passage_objects(sim, m, lib, i).is_empty()
}
/// Raise (`up`, state 1, the passage closed) or lower a gate. The state
/// changes at once (0x4575d0): the lowered drawbridge kills the land units
/// in its passage and breaks whatever else stands there but a bridge. The
/// drawbridge then moves one row every 100 ms (0x456f40, 0x4573dc), its
/// sound at the start (0x457450): "gate open" raising, "gate close"
/// lowering, as the original names them.
fn move_gate(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    up: bool,
) {
    let now = sim.ticks * TICK_MS;
    sim.gameplay.actors[i].gate_motion = Some(if up {
        GateMotion {
            row: 6,
            end: -1,
            step: -1,
            due_ms: now + 100,
        }
    } else {
        GateMotion {
            row: 0,
            end: 7,
            step: 1,
            due_ms: now + 100,
        }
    });
    sound(
        sim,
        i,
        if up {
            "@gate/gate open.wav"
        } else {
            "@gate/gate close.wav"
        },
        // Category 2 of vt+0x1c8 (0x4574a2): `obj_order`.
        crate::audio::category::ORDER,
    );
    if !up {
        for j in gate_passage_objects(sim, m, lib, i) {
            if m.objects[j].class != class::BRIDGE {
                kill(sim, m, g, lib, j);
            }
        }
    }
    m.objects[i].data[57..61].copy_from_slice(&u32::from(up).to_le_bytes());
}
/// The drawbridges on the move, one row every 100 ms; at the last row the
/// raised bridge rests on row 0, the lowered one on row 6.
fn move_drawbridges(sim: &mut Simulation) {
    let now = sim.ticks * TICK_MS;
    for a in sim.gameplay.actors.iter_mut() {
        while let Some(mut motion) = a.gate_motion
            && now >= motion.due_ms
        {
            motion.row += motion.step;
            if motion.row == motion.end {
                a.gate_motion = None;
            } else {
                motion.due_ms += 100;
                a.gate_motion = Some(motion);
            }
        }
    }
}
/// Boats fishing on patch `j` (+0xca): the fishing op counts itself in
/// when it starts and out when it ends (0x4e93d5, 0x4e967e).
fn fishing_boats(sim: &Simulation, j: usize) -> usize {
    sim.gameplay
        .actors
        .iter()
        .filter(|a| a.intent == Some(Intent::Harvest(j)) && a.work_target == Some(j))
        .count()
}
/// The nets drawn on a fish patch (0x4510f0): one a fishing boat, at most
/// three, from the first one on (`net1`–`net3`).
pub fn fishing_nets(sim: Option<&Simulation>, j: usize) -> Vec<usize> {
    let Some(sim) = sim else {
        return Vec::new();
    };
    let first = sim.gameplay.nets.get(&j).copied().unwrap_or(0) as usize;
    (0..fishing_boats(sim, j).min(3))
        .map(|k| (first + k) % 3)
        .collect()
}
/// The row of a gate's drawbridge: the moving one, else 0 raised and 6
/// lowered (0x456080).
pub fn drawbridge_row(sim: Option<&Simulation>, i: usize, o: &MapObject) -> usize {
    sim.and_then(|s| s.gameplay.actors.get(i))
        .and_then(|a| a.gate_motion)
        .map_or(if o.byte(57) != 0 { 0 } else { 6 }, |motion| {
            motion.row.clamp(0, 6) as usize
        })
}
/// Automatic drawbridges (message 0x4200, 0x4571a0, every 500 ms), on from
/// the gate's constructor: a land unit not friendly to the gate (relation
/// 0x40) within 4 cells of the passage raises it when the passage is free;
/// with nobody of the sort, and no friend in the passage, a raised gate
/// standing still comes down. An order keeps the automatism away for 20
/// steps (+0x166); nothing changes while the gate is upgraded or researching.
fn automatic_gates(sim: &mut Simulation, m: &mut MissionWorld, g: &MapGrid, lib: &ObjectLibrary) {
    for i in 0..m.objects.len() {
        let gate = &m.objects[i];
        if !matches!(gate.class, class::GATE_WOOD | class::GATE_STONE)
            || !alive(gate)
            || gate.flags & FLAG_CONSTRUCTION != 0
        {
            continue;
        }
        let a = &mut sim.gameplay.actors[i];
        // Every 500 ms from the gate's own start (0x456173).
        if sim.ticks <= a.gate_since || (sim.ticks - a.gate_since) % 10 != 0 {
            continue;
        }
        a.gate_hold = a.gate_hold.saturating_sub(1);
        if gate.byte(65) == 0
            || a.gate_hold != 0
            || a.jobs
                .front()
                .is_some_and(|j| matches!(j.kind, JobKind::Upgrade(..) | JobKind::Research(_)))
        {
            continue;
        }
        a.gate_hold = 1;
        let passage = gate_passage(gate);
        let raised = gate.byte(57) != 0;
        // Units the gate's kingdom sees (0x44dcd0 → 0x502270).
        let watched = gate.owner == m.player() && !m.game_rules.no_fog;
        let found = (0..m.objects.len()).any(|j| {
            let x = &m.objects[j];
            crate::selection::unit(x)
                && standing(m, j)
                && sim.objects[j].holder.is_none()
                && lib.layer(x) == 1
                && sim.gameplay.actors[j].effect_until <= sim.ticks
                && (!watched || sim.visible(x))
                && in_rect(x, passage, 4)
                && (relation(m, x.owner, gate.owner) & 0x40 == 0
                    || (raised && in_rect(x, passage, 0)))
        });
        if raised && !found && sim.gameplay.actors[i].gate_motion.is_none() {
            move_gate(sim, m, g, lib, i, false);
        } else if found && !raised && gate_passage_free(sim, m, i) {
            move_gate(sim, m, g, lib, i, true);
        }
    }
}
/// The resistance target of each building (+0x11a): its own, plus the
/// EMIT of every complete building whose rect grown by its sight meets it
/// (0x41f7b0, 0x41f920): `own` from an ally (relation 0x40), `other`
/// otherwise. Only the [`pulsed`] buildings of kingdoms 1–11 receive it.
fn auras(m: &MissionWorld, lib: &ObjectLibrary) -> Vec<i32> {
    let emitters: Vec<(usize, [i32; 4], u8, crate::simulation::Emit)> = m
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.building() && alive(o) && o.flags & FLAG_CONSTRUCTION == 0)
        .filter_map(|(k, o)| {
            let e = lib.attributes.emit(o)?;
            let r = movement::footprint_rect(lib, o);
            let d = e.sight as i32;
            Some((k, [r[0] - d, r[1] - d, r[2] + d, r[3] + d], o.owner, e))
        })
        .collect();
    m.objects
        .iter()
        .enumerate()
        .map(|(i, o)| {
            if emitters.is_empty() || !pulsed(o) || !(1..=11).contains(&o.owner) {
                return 0;
            }
            let r = movement::footprint_rect(lib, o);
            emitters
                .iter()
                .filter(|(k, a, _, _)| {
                    *k != i && a[0] <= r[2] && a[2] >= r[0] && a[1] <= r[3] && a[3] >= r[1]
                })
                .map(|&(_, _, owner, e)| {
                    if relation(m, o.owner, owner) & 0x40 != 0 {
                        e.own as i32
                    } else {
                        e.other as i32
                    }
                })
                .sum()
        })
        .collect()
}
/// The building's own resistance target: its class's, with the religious
/// bonuses it received.
fn own_resistance(sim: &Simulation, m: &MissionWorld, lib: &ObjectLibrary, i: usize) -> i32 {
    let o = &m.objects[i];
    let mut maximum = ObjectState::read(o, lib);
    match sim.objects[i].faith {
        Some(n) => {
            for _ in 0..n {
                maximum.resistance = maximum.resistance.saturating_add(maximum.resistance / 2);
            }
        }
        None => {
            if let Some(r) = m.rules.get(o.owner.wrapping_sub(1) as usize) {
                maximum.apply_researches(o, &r.initial_researches);
            }
        }
    }
    maximum.resistance as i32
}
/// The resistance target (+0x11a) of one building, auras included.
fn resistance_target(sim: &Simulation, m: &MissionWorld, lib: &ObjectLibrary, j: usize) -> i32 {
    own_resistance(sim, m, lib, j) + auras(m, lib)[j]
}
/// The buildings that answer 0 to 0x272a: the base building init (0x41d400)
/// registers them for the 2 s pulse (0x41d52d) and they take the auras
/// (0x41f920), the magic gateway, the evil castle and the ruin included; the
/// field, the dummy and the mines answer 1.
fn pulsed(o: &MapObject) -> bool {
    ((class::TOWER_STONE..=class::EVILCASTLE).contains(&o.class) && o.class != class::FIELD)
        || o.class == class::RUIN
}
/// The 2 s pulse of the buildings (0x41f271): the current resistance moves
/// up to 10 toward the target, never below 0; nothing moves while the
/// target is negative and the resistance gone.
fn restore_resistance(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, pulses: u64) {
    if pulses == 0 {
        return;
    }
    let aura = auras(m, lib);
    for (i, o) in m
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| pulsed(o) && alive(o) && o.flags & FLAG_CONSTRUCTION == 0)
    {
        let _ = o;
        // The object's own target (+0x11a), whatever its owner now.
        let target = own_resistance(sim, m, lib, i) + aura[i];
        let current = sim.objects[i].resistance as i32;
        if current == target || (target < 0 && current <= 0) {
            continue;
        }
        let amount = (pulses * 10).min(u16::MAX as u64) as i32;
        let next = if current < target {
            (current + amount).min(target)
        } else {
            (current - amount).max(target)
        };
        sim.objects[i].resistance = next.clamp(0, u16::MAX as i32) as u16;
    }
}
/// The diplomacy window's accepted choices are the player's diplomacy
/// messages (0x447072 → 0x48f876), each answered by a computer ally
/// (0x48f8bb): the relations that follow.
pub fn accept_diplomacy(m: &MissionWorld, relations: &[u32]) -> Vec<u32> {
    let Some(sim) = m.simulation.as_ref() else {
        return relations.to_vec();
    };
    let mut next = MissionWorld {
        diplomacy: relations.to_vec(),
        kingdoms: m.kingdoms.clone(),
        ..Default::default()
    };
    for (i, (was, now)) in m.diplomacy.iter().zip(relations).enumerate() {
        let (a, b) = ((i / 8 + 1) as u8, (i % 8 + 1) as u8);
        if was != now && a != b {
            ai::reciprocate(sim, &mut next, a, b);
        }
    }
    next.diplomacy
}
pub fn advance_with_diplomacy(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    dt: Duration,
    speed: u8,
    weather: bool,
    diplomacy: Option<&[u32]>,
) {
    sync(sim, m, lib);
    if let Some(relations) = diplomacy.filter(|relations| relations.len() == 64) {
        // 0x46813c: a change of diplomacy is followed by the end test.
        if m.diplomacy != relations {
            sim.gameplay.script.end_due = true;
        }
        m.diplomacy = relations.to_vec();
    }
    if dt.is_zero() {
        sim.update_weather(weather);
        return;
    }
    let previous = sim.ticks;
    // Native game speed (0x48c380): levels 0–20 in steps of 5, 10 normal.
    // Each 50 ms turn adds ms·(v + 10)/20 below 10, else ms·v/10, truncated
    // (37 ms at 5); inside a turn the clock moves in proportion up to that
    // share (0x48c3c0).
    const TURN: u64 = 50_000_000;
    let share = 1_000_000
        * if speed < 10 {
            50 * (speed as u64 + 10) / 20
        } else {
            50 * speed as u64 / 10
        };
    let mut real = sim
        .gameplay
        .scaled_remainder
        .saturating_add(dt.as_nanos().min(u64::MAX as u128) as u64);
    let mut given = sim.gameplay.turn_game.min(share);
    let mut game = 0u64;
    if real >= TURN {
        let turns = real / TURN;
        game = share - given + (turns - 1) * share;
        real %= TURN;
        given = 0;
    }
    let target = (real as u128 * share as u128 / TURN as u128) as u64;
    game += target.saturating_sub(given);
    sim.gameplay.scaled_remainder = real;
    sim.gameplay.turn_game = given.max(target);
    sim.advance(Duration::from_nanos(game), weather);
    let target = sim.ticks;
    sim.ticks = previous;
    while sim.ticks < target {
        // A story, a message or a conversation stops the game on the step
        // it opens (0x43ed40 from 0x47d9b5, 0x4c3538, 0x4be68c): the rest
        // of the time does not run, until the interface closes it.
        if sim.gameplay.script.modal {
            sim.elapsed = Duration::from_millis(sim.ticks * TICK_MS);
            break;
        }
        sim.ticks += 1;
        step(sim, m, g, lib, c, diplomacy);
    }
}
/// One 50 ms step of every object.
fn step(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    diplomacy: Option<&[u32]>,
) {
    fell_under_grounds(sim, m, g, lib);
    islands::watch(sim, m, g, lib);
    ai::prepare(sim, m, &c.ai);
    movement::refresh(sim, m, g, lib);
    if sim.ticks % 40 == 0 {
        restore_resistance(sim, m, lib, 1);
    }
    move_drawbridges(sim);
    automatic_gates(sim, m, g, lib);
    capture_points(sim, m, lib, c);
    flag_points(sim, m, g, lib, c);
    if sim.ticks % 100 == 0 {
        decay_fields(sim, m, g, lib);
    }
    expire_effects(sim, m, lib);
    fly_flames(sim, m, g, lib, c);
    land_projectiles(sim, m, g, lib, c);
    whirlpools(sim, m, g, lib, c);
    economy(sim, m, g, lib);
    tavern_offers(sim, m, lib, c);
    market_jewels(sim, m);
    cow_meat(sim, m, g, lib);
    crumble(sim, m, lib);
    arrive_in_sectors(sim, m);
    ai::places::watch(sim, m, lib, c);
    ai::think::watch(sim, m, g, lib, c);
    fish_patches(sim, m, g, lib);
    poach_cows(sim, m, lib);
    let count = m.objects.len();
    for i in 0..count {
        if m.objects[i].flags & FLAG_DYING != 0 {
            decay_corpse(sim, m, lib, i);
            continue;
        }
        if !alive(&m.objects[i]) {
            continue;
        }
        // 0x4ebba9: a ninja hidden on its own mana pays 1 when its
        // invisibility's timer comes and stays hidden 2000 ms more; without
        // mana, or once revealed, it is seen again.
        let a = &mut sim.gameplay.actors[i];
        if a.cloak_drain {
            if a.effect_until == 0 {
                a.cloak_drain = false;
            } else if sim.ticks >= a.effect_until {
                if sim.objects[i].mana >= 1 {
                    sim.objects[i].mana -= 1;
                    a.effect_until += 2000 / TICK_MS;
                } else {
                    a.cloak_drain = false;
                }
            }
        }
        if a.frozen_until > sim.ticks {
            continue;
        }
        a.cooldown = a.cooldown.saturating_sub(1);
        a.path_cooldown = a.path_cooldown.saturating_sub(1);
        a.capture_cooldown = a.capture_cooldown.saturating_sub(1);
        a.work_cooldown = a.work_cooldown.saturating_sub(1);
        // 0x502ea2 on the 1000 ms timer of every object given a mana pool
        // (0x504d90), held books included; the gate 0x504e30 skips frozen
        // and petrified objects and cows being milked.
        if sim.ticks % 20 == 0
            && sim.objects[i].mana < sim.objects[i].max_mana
            && !has_effect(sim, i, effect::FREEZE)
            && !has_effect(sim, i, effect::STONE)
            && !(m.objects[i].class == class::COW && milked(sim, i))
            && !channelling(sim, m, c, i)
            // 0x4ebb91: nor a ninja hidden on its own mana.
            && !sim.gameplay.actors[i].cloak_drain
        {
            sim.objects[i].mana = sim.objects[i]
                .mana
                .saturating_add(mana_regeneration(m, i))
                .min(sim.objects[i].max_mana);
        }
        // 0x464280: on the same timer, a full summoning book with more than
        // one charge is used at once on its holder, a computer's unit.
        if sim.ticks % 20 == 0
            && m.objects[i].class == class::ITEM_MAGIC
            && sim.objects[i].max_mana > 0
            && sim.objects[i].mana >= sim.objects[i].max_mana
            && book_charges(&m.objects[i]) > 1
            && summoning(m.objects[i].byte(96))
            && let Some(h) = sim.objects[i].holder
            && crate::selection::unit(&m.objects[h])
            && ai_managed(sim, m, h)
        {
            let _ = read_book(sim, m, g, lib, c, h, i, Some(h), None);
        }
        // Message 0x4015 (0x4e14b9): a spy's energy grows a point a second,
        // not while frozen or stone; so does that of a spy hidden in a unit.
        if sim.ticks % 20 == 0
            && !has_effect(sim, i, effect::FREEZE)
            && !has_effect(sim, i, effect::STONE)
        {
            let a = &mut sim.gameplay.actors[i];
            if m.objects[i].class == 14 {
                a.spy_energy = a.spy_energy.saturating_add(1).min(100);
            }
            if let Some(mole) = a.mole.as_mut() {
                mole.energy = mole.energy.saturating_add(1).min(100);
            }
        }
        if sim.objects[i].holder.is_some() {
            // 0x4e4e90 inside a holder: a computer player's idle unit still
            // ticks, for its player's idle (0x4140b0).
            let o = &m.objects[i];
            if crate::selection::unit(o)
                && ai_managed(sim, m, i)
                && o.flags & FLAG_DYING == 0
                && sim.gameplay.actors[i].intent.is_none()
            {
                let every = idle_period(o);
                let now = sim.ticks * TICK_MS;
                let a = &mut sim.gameplay.actors[i];
                let due = *a.scan_due_ms.get_or_insert(now + every);
                if now >= due {
                    a.scan_due_ms = Some(due + every);
                    let idle = a.idle_ms;
                    a.idle_ms = a.idle_ms.saturating_add(every as u32);
                    ai::idle::unit(sim, m, g, lib, c, i, idle);
                }
            }
            continue;
        }
        // A creature coming out does nothing else until its picture has
        // played (Animate, op 0x74); then the program ends (0x502c97) and the
        // default one starts.
        let a = &mut sim.gameplay.actors[i];
        if a.appearing_until > 0 {
            if sim.ticks < a.appearing_until {
                a.action_elapsed_ms += TICK_MS;
                continue;
            }
            a.appearing_until = 0;
            a.action_image.clear();
            a.scan_due_ms = None;
            sim.objects[i].order = OrderState::Idle;
        }
        heal_garrison(sim, m, i);
        if let Some(due) = sim.gameplay.actors[i].fire_due_ms
            && sim.ticks * TICK_MS >= due
        {
            sound(sim, i, "@fire", crate::audio::category::FIRE);
            sim.gameplay.actors[i].fire_due_ms = Some(due + 500);
        }
        production(sim, m, g, lib, c, i);
        let a = &sim.gameplay.actors[i];
        let unmasking = matches!(m.objects[i].class, class::NINJA | 14)
            && a.intent.is_none()
            && a.scan_due_ms.is_some_and(|due| sim.ticks * TICK_MS >= due);
        let asks = watch(sim, m, lib, c, i);
        if unmasking {
            unmask(sim, m, lib, c, i);
        }
        if let Some(idle) = asks {
            ai::idle::unit(sim, m, g, lib, c, i, idle);
        }
        animal(sim, m, g, lib, i);
        // Orders take effect when the step under way ends (0x503160).
        if movement::stepping(sim, i) {
            sim.objects[i].order = OrderState::Moving;
        } else if let Some(intent) = sim.gameplay.actors[i].intent.clone() {
            work(sim, m, g, lib, c, i, intent);
            movement::settle(sim, m, lib, i);
            if std::mem::take(&mut sim.gameplay.actors[i].entered) {
                ai::units::entered(sim, m, lib, c, i);
            }
        } else if sim.objects[i].order != OrderState::Casting
            || sim.gameplay.actors[i].cooldown == 0
        {
            sim.objects[i].order = OrderState::Idle;
        }
        // A unit whose program ended runs the default program at once.
        if crate::selection::unit(&m.objects[i]) && alive(&m.objects[i]) {
            let a = &sim.gameplay.actors[i];
            if a.intent.is_some() {
                sim.gameplay.actors[i].scan_due_ms = None;
            } else if a.scan_due_ms.is_none() {
                idle(sim, m, lib, c, i);
            }
        }
        let o = &mut m.objects[i];
        if o.data.len() >= 22 {
            o.data[18..20].copy_from_slice(&o.health.to_le_bytes());
            o.data[20..22].copy_from_slice(&o.max_health.to_le_bytes());
        }
        // The militias this object's update called (0x413a99) and the
        // attacks on a wall its path searches posted (0x4171e0).
        if !sim.gameplay.ai.alarms.is_empty() {
            ai::war::alarms(sim, m, g, lib, c);
        }
        if !sim.gameplay.ai.breaches.is_empty() {
            ai::breach::orders(sim, m, g, lib, c);
        }
    }
    if !sim.gameplay.ai_taken.is_empty() {
        ai_take_up(sim, m, g, lib, c);
    }
    scripting::run(sim, m, g, lib, c);
    if !sim.gameplay.ai.alarms.is_empty() {
        ai::war::alarms(sim, m, g, lib, c);
    }
    if !sim.gameplay.ai.breaches.is_empty() {
        ai::breach::orders(sim, m, g, lib, c);
    }
    if sim.ticks % 5 == 0 {
        sim.refresh_visibility(m, diplomacy);
        settle_stumps(sim, g, lib);
    }
}
/// 0x504e30: a unit casting heal or teaching (spells 2 and 0x12) with the
/// mana for it does not regenerate.
fn channelling(sim: &Simulation, m: &MissionWorld, c: &Catalog, i: usize) -> bool {
    match &sim.gameplay.actors[i].intent {
        Some(Intent::Cast { key, .. })
            if matches!(key.as_str(), "spell_heal" | "spell_raiseexp")
                && sim.objects[i].order == OrderState::Casting =>
        {
            sim.objects[i].mana >= spell_parameters(c, m, &m.objects[i], key).1
        }
        _ => false,
    }
}
fn add_score(sim: &mut Simulation, owner: u8, points: u64) {
    if (1..=8).contains(&owner) {
        let s = sim.gameplay.score.entry(owner).or_default();
        *s = s.saturating_add(points);
    }
}
/// Ruins (0x4b6760) and magic gateways (0x420fd0), every 1000 ms from their
/// own coming into play (message 0x4210, 0x4b65bd, 0x420fad). A ruin of a
/// kingdom gives it the `ruinpoints` of its picture (0x443430 type 6).
/// Then the fighters (flag 0x10000) standing in the footprint grown by one
/// (0x41fd70) take it over when they all belong to one kingdom that may
/// rescue (K+0x122); a taken gateway says "magicgatewaycap".
fn capture_points(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary, c: &Catalog) {
    for i in 0..m.objects.len() {
        let o = &m.objects[i];
        if !matches!(o.class, class::RUIN | class::MAGIC_GATEWAY) || o.flags & FLAG_INACTIVE != 0 {
            continue;
        }
        let born = sim.gameplay.actors[i].born;
        if sim.ticks <= born || (sim.ticks - born) % 20 != 0 {
            continue;
        }
        if o.class == class::RUIN && (1..=8).contains(&o.owner) {
            let points = lib
                .images
                .get(&o.custom_image, "ruinpoints")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(0);
            add_score(sim, o.owner, points);
        }
        let r = movement::footprint_rect(lib, o);
        let area = [r[0] - 1, r[1] - 1, r[2] + 1, r[3] + 1];
        let mut taker = None;
        let mut mixed = false;
        for (k, u) in m.objects.iter().enumerate() {
            let (x, y) = (u.x as i32, u.y as i32);
            if !crate::selection::unit(u)
                || !standing(m, k)
                || sim.objects[k].holder.is_some()
                || x < area[0]
                || x > area[2]
                || y < area[1]
                || y > area[3]
                || !military(sim, m, k)
                || !rescuing_kingdom(sim, m, u.owner)
            {
                continue;
            }
            match taker {
                None => taker = Some(u.owner),
                Some(t) if t != u.owner => {
                    mixed = true;
                    break;
                }
                _ => {}
            }
        }
        if let Some(t) = taker
            && !mixed
            && t != o.owner
        {
            if o.class == class::MAGIC_GATEWAY {
                let k = &m.kingdoms[t as usize - 1];
                let name = if k.name.trim().is_empty() {
                    &k.leader
                } else {
                    &k.name
                };
                sim.gameplay
                    .notices
                    .push(c.gateway_taken.replacen("%s", name.trim(), 1));
            }
            transfer_owner(sim, m, lib, i, t);
        }
    }
}
/// The flag of the original's capture the flag (class 135, `item_flag`).
/// No setup offers that game (0x4c4c42, 0x4c8469 and 0x426257 grey it), but
/// a map made with the editor may hold flags, and they work in any game: the
/// flag's timer (message 0x4017, 0x4656f0), every 1000 ms from its coming
/// into play, looks whether a unit holds it within six cells of a castle of
/// the unit's kingdom (0x44e3a0 with 0x44ea80); then that kingdom scores
/// 100 (0x443430 type 5) and the flag goes elsewhere (0x465b80).
fn flag_points(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
) {
    let Some(f) = sim.gameplay.flag else {
        return;
    };
    if m.objects
        .get(f)
        .is_none_or(|o| o.flags & FLAG_INACTIVE != 0)
    {
        return;
    }
    let born = sim.gameplay.actors[f].born;
    if sim.ticks <= born || !(sim.ticks - born).is_multiple_of(20) {
        return;
    }
    let Some(h) = sim.objects[f].holder else {
        return;
    };
    let owner = m.objects[h].owner;
    // A held item takes its holder's cell (0x500ac2).
    let [x, y] = [m.objects[f].x as i32, m.objects[f].y as i32];
    let near = m.objects.iter().any(|o| {
        let r = movement::footprint_rect(lib, o);
        o.class == class::CASTLE
            && o.owner == owner
            && o.flags & (FLAG_INACTIVE | FLAG_DYING | FLAG_HELD) == 0
            && r[0] <= x + 6
            && r[2] >= x - 6
            && r[1] <= y + 6
            && r[3] >= y - 6
    });
    if near {
        add_score(sim, owner, 100);
        flag_elsewhere(sim, m, g, lib, c, f);
    }
}
/// 0x465b80: the flag leaves the map (0x4ff350) and a new one, of the
/// picture `flag_capture`, lies where the kingdoms are fewest. Of 128 cells
/// of free land drawn at random (0x4658f0) it takes the one whose square of
/// 32 cells around holds the least, a building of any kingdom counting 2 and
/// a unit 1 (0x465ae0), drawing again among equals. Without free land the
/// game says `[err] ctfcantspawn` and there is no flag any more.
fn flag_elsewhere(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    f: usize,
) {
    sim.gameplay.flag = None;
    consume(sim, m, f);
    let cells = movement::cells_now(sim, m, g, lib);
    let lying: BTreeSet<[i32; 2]> = m
        .objects
        .iter()
        .filter(|o| o.item() && o.flags & (FLAG_INACTIVE | FLAG_HELD) == 0)
        .map(|o| [o.x as i32, o.y as i32])
        .collect();
    // Land with no static object and no standing tree (the count also leaves
    // out the cells where an item lies, the choice does not).
    let free = |w: u32| {
        (w >> 17) & 3 == pathing::LAND >> 17
            && w & pathing::STATIC == 0
            && ((w >> 11) & 31 <= cells.trunks || w & pathing::FELLED != 0)
    };
    let side = cells.side as i32;
    let order = (0..side).flat_map(|y| (0..side).map(move |x| [x, y]));
    let count = order
        .clone()
        .filter(|&p| free(cells.at(p)) && !lying.contains(&p))
        .count() as u64;
    if count == 0 {
        sim.gameplay.notices.push(c.no_flag_room.clone());
        return;
    }
    let mut drawn: Vec<u64> = (0..128).map(|_| random(sim, count)).collect();
    drawn.sort_unstable();
    // A draw of 0 is the corner (0, 0); a draw of n the n-th free cell.
    let mut places = vec![[0, 0]; drawn.iter().take_while(|&&n| n == 0).count()];
    let mut seen = 0;
    for p in order.filter(|&p| free(cells.at(p))) {
        seen += 1;
        while places.len() < drawn.len() && drawn[places.len()] == seen {
            places.push(p);
        }
        if places.len() == drawn.len() {
            break;
        }
    }
    let crowd = |p: [i32; 2]| -> u32 {
        m.objects
            .iter()
            .enumerate()
            .filter(|&(k, o)| {
                let r = movement::footprint_rect(lib, o);
                (1..=8).contains(&o.owner)
                    && o.flags & (FLAG_INACTIVE | FLAG_HELD | FLAG_DEAD) == 0
                    && sim.objects[k].holder.is_none()
                    && r[0] <= p[0] + 32
                    && r[2] >= p[0] - 32
                    && r[1] <= p[1] + 32
                    && r[3] >= p[1] - 32
            })
            .map(|(_, o)| {
                if o.building() {
                    2
                } else if crate::selection::unit(o) {
                    1
                } else {
                    0
                }
            })
            .sum()
    };
    let values: Vec<u32> = places.iter().map(|&p| crowd(p)).collect();
    let Some(&least) = values.iter().min() else {
        return;
    };
    let ties = values.iter().filter(|&&v| v == least).count() as u64;
    let pick = random(sim, ties) as usize;
    let Some(at) = places
        .iter()
        .zip(&values)
        .filter(|&(_, &v)| v == least)
        .nth(pick)
        .map(|(&p, _)| [p[0] as u16, p[1] as u16])
    else {
        return;
    };
    if let Ok(k) = new_unit(sim, m, lib, class::ITEM_FLAG, 0, 0, at) {
        m.objects[k].custom_image = "flag_capture".into();
    }
}
/// A delay of the original's object timers given in real milliseconds, on
/// the game's clock at the speed of the moment (0x48c380).
fn real_delay(speed: u8, ms: u64) -> u64 {
    let v = speed as u64;
    if v < 10 {
        ms * (v + 10) / 20
    } else {
        ms * v / 10
    }
}
/// What the map's sounds do now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapSounds {
    /// The sources that sound, and whether in a loop.
    pub start: Vec<(usize, bool)>,
    /// Objects gone whose sound still plays.
    pub silence: Vec<usize>,
}
/// The map's sounds placed with the editor, as each object's timer runs
/// them (message 0x2009, 0x502953). The map's loading starts the timer
/// (vt+0x1cc = 0x505580): at once, or with the start delayed after the
/// period (mode 0) or R(min, max) seconds (mode 2). When it comes, an
/// object whose sound or voice still plays (`sounding`, flags 0x2000000
/// and 0x800000) waits another second; otherwise it sounds (category 20,
/// in a loop in mode 1) and the timer comes again: in mode 0 after the
/// period, if any; in mode 1 after 2 s; in mode 2 after R(min, max)
/// seconds drawn from the stream of sounds and pictures. Times are real
/// seconds, scaled by the game's speed when the timer is set.
pub fn map_sounds(
    sim: &mut Simulation,
    m: &MissionWorld,
    sounding: impl Fn(usize) -> bool,
) -> MapSounds {
    let now = sim.elapsed.as_millis() as u64;
    let speed = sim.speed;
    let between = |sim: &mut Simulation, s: &world::SoundSource| {
        sim.gameplay
            .aux_rng
            .range(s.min as i32 * 1000, s.max as i32 * 1000)
            .max(0) as u64
    };
    if sim.gameplay.map_sounds.len() != m.sounds.len() {
        let mut due = Vec::new();
        for s in &m.sounds {
            let ms = match (s.mode, s.delayed) {
                _ if s.sound.is_empty() => {
                    due.push(None);
                    continue;
                }
                (0, true) => s.period as u64 * 1000,
                (2, true) => between(sim, s),
                _ => 1,
            };
            due.push(Some(now + real_delay(speed, ms)));
        }
        sim.gameplay.map_sounds = due;
    }
    let mut out = MapSounds::default();
    for (i, s) in m.sounds.iter().enumerate() {
        let Some(due) = sim.gameplay.map_sounds[i].filter(|&t| t <= now) else {
            continue;
        };
        let Some(o) = m.objects.get(s.object) else {
            sim.gameplay.map_sounds[i] = None;
            continue;
        };
        // An object that left the game takes its timer and its sound.
        if o.flags & FLAG_DEAD != 0 || (o.class != class::SOUND_OBJECT && o.health == 0) {
            sim.gameplay.map_sounds[i] = None;
            if sounding(s.object) {
                out.silence.push(s.object);
            }
            continue;
        }
        if sounding(s.object) {
            sim.gameplay.map_sounds[i] = Some(due + real_delay(speed, 1000));
            continue;
        }
        // A hidden object of the editor is not heard.
        if o.flags & FLAG_INACTIVE == 0 {
            out.start.push((i, s.mode == 1));
        }
        let next = match s.mode {
            0 => (s.period > 0).then_some(s.period as u64 * 1000),
            1 => Some(2000),
            2 => Some(between(sim, s)),
            _ => None,
        };
        sim.gameplay.map_sounds[i] = next.map(|ms| due + real_delay(speed, ms));
    }
    out
}
/// F4's scoreboard (0x4f2903): each kingdom taking part with its score and
/// its team's (the mean of the members), as shown, (raw + 50) / 100, from
/// the best.
pub fn scores(sim: &Simulation, m: &MissionWorld) -> Vec<(u8, u64, u64)> {
    let raw = |k: u8| sim.gameplay.score.get(&k).copied().unwrap_or(0);
    let players: Vec<u8> = (1..=m.kingdoms.len() as u8)
        .filter(|&k| m.kingdoms[k as usize - 1].active)
        .collect();
    let mut out: Vec<(u8, u64, u64)> = players
        .iter()
        .map(|&k| {
            let team = m.kingdoms[k as usize - 1].team;
            let members: Vec<u8> = players
                .iter()
                .copied()
                .filter(|&o| team != 0 && m.kingdoms[o as usize - 1].team == team)
                .collect();
            let team_raw = if members.is_empty() {
                raw(k)
            } else {
                members.iter().map(|&o| raw(o)).sum::<u64>() / members.len() as u64
            };
            (k, (raw(k) + 50) / 100, (team_raw + 50) / 100)
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}
/// The native kind of a load (table 0x58e584): 0 gold, 1 stone, 2 food,
/// 3 wood, 4 milk, 5 meat; milk and meat are delivered as food.
fn carry_type(a: &Actor) -> Option<u8> {
    let r = a.cargo.iter().position(|&n| n > 0)?;
    Some(match (r, a.cargo_source) {
        (2, class::COW) => 4,
        (2, class::RESITEM) => 5,
        (r, _) => r as u8,
    })
}
/// Pictures of the resource items by kind (0x58d174, 0x58a0e8, 0x58a0dc).
const RESOURCE_ITEMS: [&str; 6] = [
    "item_gold",
    "item_stone",
    "item_food",
    "item_wood",
    "item_milk",
    "item_meat",
];
/// A resource item (class 80, 0x497cc0): resource at +0x60, amount at
/// +0x62 and picture at +0x38, on the free cell nearest to `at` (0x4fd4c0).
#[allow(clippy::too_many_arguments)]
fn spawn_resources(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    at: [u16; 2],
    resource: u8,
    amount: u32,
    image: &str,
) -> Option<usize> {
    let j = new_unit(sim, m, lib, class::RESITEM, 0, 0, at).ok()?;
    let o = &mut m.objects[j];
    o.data.resize(0x64, 0);
    o.data[0..2].copy_from_slice(&0x64u16.to_le_bytes());
    o.data[56..96].fill(0);
    o.data[56..56 + image.len()].copy_from_slice(image.as_bytes());
    o.custom_image = image.into();
    o.data[0x60] = resource.min(3);
    o.data[0x62..0x64].copy_from_slice(&(amount.min(u16::MAX as u32) as u16).to_le_bytes());
    o.health = 0;
    o.max_health = 0;
    if let Some(q) = free_near(g, m, lib, &m.objects[j].clone(), at, Some(j)) {
        position(&mut m.objects[j], q);
    }
    sim.objects[j] = ObjectState::read(&m.objects[j], lib);
    Some(j)
}
/// A peasant drops what it carries as a resource item (0x4aea70).
fn drop_cargo(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
) {
    let a = &sim.gameplay.actors[i];
    let (Some(kind), Some(r)) = (carry_type(a), a.cargo.iter().position(|&n| n > 0)) else {
        return;
    };
    let amount = a.cargo[r];
    let at = [m.objects[i].x, m.objects[i].y];
    spawn_resources(
        sim,
        m,
        g,
        lib,
        at,
        r as u8,
        amount,
        RESOURCE_ITEMS[kind as usize],
    );
    let a = &mut sim.gameplay.actors[i];
    a.cargo = [0; 4];
    a.cargo_source = 0;
}
/// A resource item that came into a holder (vt+0x1b4 = 0x464f30 from
/// 0x5044df): a peasant loads it (0x464f60); the castle gives its kingdom any
/// kind, the lumber mill wood, food, gold or stone and the storage gold or
/// stone (the cases fall through one into the next, 0x464fee–0x46500c);
/// otherwise it stays inside. In the installed data only the castle has
/// room for one (`max_obj_in_holder`, checked by 0x504440). Taken, it goes
/// (0x4ff350) through the items' leave (0x464830 → 0x462c20).
fn resource_item_entered(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    j: usize,
    holder: usize,
) {
    if m.objects[j].class != class::RESITEM {
        return;
    }
    let kind = m.objects[j].byte(0x60) as usize;
    let taken = match m.objects[holder].class {
        class::PEON => {
            sim.objects[holder].contents.retain(|&k| k != j);
            sim.objects[j].holder = None;
            item_left(sim, m, holder, j);
            m.objects[j].flags &= !FLAG_HELD;
            take_resources(sim, m, g, lib, holder, j);
            return;
        }
        class::CASTLE => kind < 4,
        46 => kind < 4,
        class::FARM => kind < 3,
        51 => kind < 2,
        _ => false,
    };
    if taken {
        let mut amount = [0; 4];
        amount[kind] = m.objects[j].word(0x62) as u32;
        let owner = m.objects[holder].owner;
        credit(m, owner, amount);
        sim.objects[holder].contents.retain(|&k| k != j);
        sim.objects[j].holder = None;
        item_left(sim, m, holder, j);
        m.objects[j].flags = (m.objects[j].flags & !FLAG_HELD) | FLAG_INACTIVE;
    }
}
/// Find nearest resource item (op 0x43, 0x4e0c66) of Tend Cows: within 8
/// cells around the peasant (0x44e8a0), a food item on the ground on its row
/// or column, or else on a cell it can reach (vt+0x90); the nearest wins
/// (0x44d870).
fn food_item(
    sim: &mut Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
) -> Option<usize> {
    let o = &m.objects[i];
    let mut found: Vec<(u32, usize)> = (0..m.objects.len())
        .filter(|&k| {
            let t = &m.objects[k];
            t.class == class::RESITEM
                && alive(t)
                && t.byte(0x60) == 2
                && sim.objects.get(k).is_some_and(|s| s.holder.is_none())
                && (t.x as i32 - o.x as i32).abs() <= 8
                && (t.y as i32 - o.y as i32).abs() <= 8
        })
        .map(|k| (distance(lib, o, &m.objects[k]), k))
        .collect();
    found.sort_by_key(|&(d, _)| d);
    found.into_iter().map(|(_, k)| k).find(|&k| {
        let t = &m.objects[k];
        let o = &m.objects[i];
        t.x == o.x || t.y == o.y || movement::reachable(sim, m, g, lib, i, [t.x, t.y])
    })
}
/// A peasant takes a resource item (0x464f60): a load of the same kind
/// grows, another is dropped first. `item_milk` and `item_meat` carry as
/// milk and meat.
fn take_resources(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    j: usize,
) {
    let t = &m.objects[j];
    let r = (t.byte(0x60) as usize).min(3);
    let amount = t.word(0x62) as u32;
    let source = match t.custom_image.as_str() {
        "item_milk" if r == 2 => class::COW,
        "item_meat" if r == 2 => class::RESITEM,
        _ => 0,
    };
    let kind = match source {
        class::COW => 4,
        class::RESITEM => 5,
        _ => r as u8,
    };
    if carry_type(&sim.gameplay.actors[i]).is_some_and(|k| k != kind) {
        drop_cargo(sim, m, g, lib, i);
    }
    let a = &mut sim.gameplay.actors[i];
    a.cargo[r] = a.cargo[r].saturating_add(amount);
    a.cargo_source = source;
    a.work_target = None;
    m.objects[j].flags |= FLAG_INACTIVE;
}
/// The fish manager (timer 0x4216, 1250 ms): each turn handles the
/// patches whose place in the list has the turn's number in its low three
/// bits, so a patch every 10 s. One no boat is fishing grows by 5% up to
/// 2000 and drifts by −1..1 cells when the new place is free (0x450ba0).
fn fish_patches(sim: &mut Simulation, m: &mut MissionWorld, g: &MapGrid, lib: &ObjectLibrary) {
    if sim.ticks % 25 != 0 {
        return;
    }
    let turn = (sim.ticks / 25) as usize & 7;
    let patches: Vec<usize> = (0..m.objects.len())
        .filter(|&k| m.objects[k].class == class::FISHPATCH && alive(&m.objects[k]))
        .collect();
    for (place, j) in patches.into_iter().enumerate() {
        if place & 7 != turn {
            continue;
        }
        let fished = sim
            .gameplay
            .actors
            .iter()
            .any(|a| a.intent == Some(Intent::Harvest(j)) && a.work_target == Some(j));
        if fished {
            continue;
        }
        let start = m.objects[j].dword(0x38);
        let left = sim.gameplay.resources.entry(j).or_insert(start);
        if *left < 2000 {
            *left = (*left + *left * 5 / 100).min(2000);
        }
        let dx = random(sim, 3) as i32 - 1;
        let dy = random(sim, 3) as i32 - 1;
        let (x, y) = (m.objects[j].x as i32 + dx, m.objects[j].y as i32 + dy);
        if (dx, dy) == (0, 0) || x < 0 || y < 0 {
            continue;
        }
        // The patch leaves its cells (vt+0x74) and tries the new place
        // (0x450fa0): the cells of its picture free in its layer, as a
        // ship could step there now (0x505b20 with vt+0x90), and none of
        // its 2×2 cells marked by something else standing still.
        let o = &m.objects[j];
        let to = [x as u16, y as u16];
        let own = movement::footprint_rect(lib, o);
        let picture = lib.image_for(o).map_or([1, 1], |s| lib.picture_cells(&s));
        let layer = lib.layer(o);
        let cells = movement::cells_now(sim, m, g, lib);
        let free = movement::block(to, picture).all(|p| cells.free(layer, p));
        let clear = movement::block(to, footprint(lib, o)).all(|p| {
            let mine = (own[0]..=own[2]).contains(&p[0]) && (own[1]..=own[3]).contains(&p[1]);
            mine || !cells.inside(p) || cells.at(p) & pathing::STATIC == 0
        });
        if free && clear {
            position(&mut m.objects[j], to);
        }
    }
}
/// A peasant is milking the cow (+0x126 set by 0x4ed2c0): its milk does not
/// come back meanwhile (0x4ecf60).
fn milked(sim: &Simulation, cow: usize) -> bool {
    sim.gameplay
        .actors
        .iter()
        .any(|a| a.intent == Some(Intent::Harvest(cow)) && a.work_target == Some(cow))
}
/// Cow Poaching (timer 0x4207, every 2 s from the cow's birth, 0x4ec944,
/// 0x4ecff8): the peasants on the cells around a cow take it to their
/// kingdom when none of them is an ally of its owner and one of them knows
/// `te_cowpoaching`; that one loses its invisibility.
fn poach_cows(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary) {
    for cow in 0..m.objects.len() {
        let o = &m.objects[cow];
        if o.class != class::COW || !alive(o) {
            continue;
        }
        let born = sim.gameplay.actors[cow].born;
        if sim.ticks <= born || (sim.ticks - born) % 40 != 0 {
            continue;
        }
        let near: Vec<usize> = (0..m.objects.len())
            .filter(|&k| {
                let p = &m.objects[k];
                p.class == class::PEON
                    && alive(p)
                    && sim.objects[k].holder.is_none()
                    && p.x.abs_diff(o.x) <= 1
                    && p.y.abs_diff(o.y) <= 1
            })
            .collect();
        if near.iter().any(|&k| allied(m, o.owner, m.objects[k].owner)) {
            continue;
        }
        let Some(&thief) = near.iter().rev().find(|&&k| {
            researches(m, m.objects[k].owner)
                .iter()
                .any(|r| r.eq_ignore_ascii_case("te_cowpoaching"))
        }) else {
            continue;
        };
        let owner = m.objects[thief].owner;
        transfer_owner(sim, m, lib, cow, owner);
        sim.gameplay.actors[thief].effect_until = 0;
    }
}
/// Meat of the cows (+0x120): 300 at birth, +2 a second up to 600 while
/// neither frozen nor stone (0x4ecf2c). A dying cow leaves it as items of
/// at most 100 food within reach (0x4ed1ad, 0x4ed200, 0x4e5570).
fn cow_meat(sim: &mut Simulation, m: &mut MissionWorld, g: &MapGrid, lib: &ObjectLibrary) {
    let count = m.objects.len();
    for i in 0..count {
        if m.objects[i].class != class::COW {
            continue;
        }
        if m.objects[i].flags & FLAG_DYING != 0 {
            let mut meat = sim.gameplay.meat.get(&i).copied().unwrap_or(300);
            if meat == 0 {
                continue;
            }
            let at = [m.objects[i].x, m.objects[i].y];
            while meat > 0 {
                let piece = meat.min(100);
                meat -= piece;
                spawn_resources(sim, m, g, lib, at, 2, piece as u32, "item_meat");
            }
            sim.gameplay.meat.insert(i, 0);
        } else if alive(&m.objects[i])
            && sim.ticks % 20 == 0
            && !has_effect(sim, i, effect::FREEZE)
            && !has_effect(sim, i, effect::STONE)
        {
            let meat = sim.gameplay.meat.entry(i).or_insert(300);
            *meat = (*meat + 2).min(600);
            // 0x4ecf6e: a full cow nobody milks, of a kingdom whose computer
            // player runs its economy, is slaughtered (the Die program).
            let full = *meat >= 600;
            if full
                && !milked(sim, i)
                && m.objects[i].flags & FLAG_STORY == 0
                && sim
                    .gameplay
                    .ai
                    .players
                    .get(&m.objects[i].owner)
                    .is_some_and(|p| p.mode & 1 != 0)
            {
                kill(sim, m, g, lib, i);
            }
        }
    }
}
/// A tavern of the kingdom is exchanging resources (K+0x126 clear): the
/// exchange dialog does not open then (0x4a0b79).
pub fn exchanging(sim: &Simulation, m: &MissionWorld, owner: u8) -> bool {
    sim.gameplay.actors.iter().enumerate().any(|(i, a)| {
        a.exchange.is_some()
            && m.objects
                .get(i)
                .is_some_and(|o| o.owner == owner && alive(o))
    })
}
/// A magic workshop of the kingdom is making an item (K+0x12a clear).
pub fn crafting(sim: &Simulation, m: &MissionWorld, owner: u8) -> bool {
    sim.gameplay.actors.iter().enumerate().any(|(i, a)| {
        a.jobs.iter().any(|j| matches!(j.kind, JobKind::Item(..)))
            && m.objects
                .get(i)
                .is_some_and(|o| o.owner == owner && alive(o))
    })
}
/// The load per trip (attribute 0x2723, 0x4e7faf): 500, 300 or 100 by
/// civilization; peasants trade only when Asian.
pub fn trade_capacity(o: &MapObject) -> u32 {
    match o.civilization {
        _ if o.class == class::PEON && o.civilization != 3 => 0,
        1 => 500,
        2 => 300,
        _ => 100,
    }
}
/// K+0x11a, added to the base 40 of the trade profit: Trade taxes +2,
/// Banking +3.
fn trade_bonus(m: &MissionWorld, owner: u8) -> u64 {
    researches(m, owner)
        .iter()
        .map(|r| match r.to_ascii_lowercase().as_str() {
            "te_tradetaxes" => 2,
            "te_banking" => 3,
            _ => 0,
        })
        .sum()
}
/// 0x454e40: the Chebyshev distance between the markets' centres, 0 under
/// 12 cells or for the same market.
fn route_distance(lib: &ObjectLibrary, m: &MissionWorld, a: usize, b: usize) -> u32 {
    let (p, q) = (centre(lib, &m.objects[a]), centre(lib, &m.objects[b]));
    let span = p[0].abs_diff(q[0]).max(p[1].abs_diff(q[1])) as u32;
    if span < 12 || a == b { 0 } else { span }
}
/// The profit of a load between two markets, as D_TRADE previews it
/// (0x4a2844, "%d (+%d)") and the trader earns it (0x4e7c98).
pub fn trade_profit(
    m: &MissionWorld,
    lib: &ObjectLibrary,
    owner: u8,
    markets: [usize; 2],
    amount: u32,
    side: usize,
) -> u32 {
    if markets.iter().any(|&k| k >= m.objects.len()) {
        return 0;
    }
    let distance = route_distance(lib, m, markets[0], markets[1]);
    profit(amount, distance, trade_bonus(m, owner), side as u32)
}
fn profit(cargo: u32, distance: u32, bonus: u64, side: u32) -> u32 {
    (cargo as u64 * distance as u64 * (40 + bonus) / (side.max(1) as u64 * 100)) as u32
}
/// Ceiling of the crusaders a cathedral gathers (0x439640): 16, +4 for
/// each full crusade, at most 36.
fn crusade_ceiling(t: &Treasury) -> u16 {
    (16 + 4 * t.full_crusades).min(36)
}
/// The crusade's anchor laid at `at` for the kingdom of `by` (a cathedral
/// or its computer player) with `ready` crusaders gathered: its price
/// paid, the crusaders called to land cells round it (0x439760).
#[allow(clippy::too_many_arguments)]
pub(super) fn begin_crusade(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    by: &MapObject,
    ready: u16,
    at: [u16; 2],
) -> std::result::Result<usize, String> {
    let mut probe = by.clone();
    probe.class = class::CRUSADEANCHOR;
    // The anchor's price is in its own INI (`cost = 1000g,2000f`).
    let cost = parse_cost(lib.property(&probe, "cost").unwrap_or_default()).0;
    debit(c, m, by.owner, cost)?;
    let anchor = new_unit(
        sim,
        m,
        lib,
        class::CRUSADEANCHOR,
        by.owner,
        by.civilization,
        at,
    )?;
    // Land cells without forest within 7 of the anchor.
    let mut crusader = by.clone();
    crusader.class = 11;
    let a = [m.objects[anchor].x as i32, m.objects[anchor].y as i32];
    let cells = (a[1] - 7..=a[1] + 7)
        .flat_map(|y| (a[0] - 7..=a[0] + 7).map(move |x| [x, y]))
        .filter(|&[x, y]| x >= 0 && y >= 0)
        .map(|[x, y]| [x as u16, y as u16])
        .filter(|&p| terrain_free(g, lib, &crusader, p, false) && !tree_at(g, lib, p))
        .collect();
    let t = sim.gameplay.treasury.entry(by.owner).or_default();
    if ready >= crusade_ceiling(t) {
        t.full_crusades += 1;
    }
    t.crusaders = None;
    t.crusade = Some(Crusade {
        anchor,
        left: ready,
        next: sim.ticks + 10,
        cells,
        crusaders: Vec::new(),
    });
    sound_here(
        sim,
        anchor,
        "@spells/crusade.wav",
        crate::audio::category::SPELL,
    );
    Ok(anchor)
}
/// Kingdom timers of the tavern and the cathedral, and the crusades.
fn economy(sim: &mut Simulation, m: &mut MissionWorld, g: &MapGrid, lib: &ObjectLibrary) {
    let now = sim.ticks;
    // 0x4d24e9: the raised loan amount arrives a minute after repayment.
    for t in sim.gameplay.treasury.values_mut() {
        if let Some((at, amount)) = t.raise
            && at <= now
        {
            t.loan_amount = amount;
            t.raise = None;
        }
    }
    for i in 0..m.objects.len().min(sim.gameplay.actors.len()) {
        if !alive(&m.objects[i]) {
            continue;
        }
        let owner = m.objects[i].owner;
        // 0x4d25a3: one unit of the asked resource every 300 ms.
        if let Some(mut x) = sim.gameplay.actors[i].exchange.clone()
            && x.next <= now
        {
            let mut amount = [0; 4];
            amount[x.to as usize] = 1;
            credit(m, owner, amount);
            x.delivered += 1;
            x.next = now + 6;
            sim.gameplay.actors[i].exchange = (x.delivered < x.total).then_some(x);
        }
        // 0x422ca0: each cathedral adds a crusader up to the ceiling, every
        // 12 s in the first 15 minutes, 8 s until 20, 4 s until 30, then 1 s.
        if m.objects[i].class == class::CATHEDRAL && sim.gameplay.actors[i].crusade_tick <= now {
            let t = sim.gameplay.treasury.entry(owner).or_default();
            let ceiling = crusade_ceiling(t);
            if let Some(n) = t.crusaders.as_mut()
                && *n < ceiling
            {
                *n += 1;
            }
            let ms = match now * TICK_MS {
                t if t < 900_000 => 12_000,
                t if t < 1_200_000 => 8_000,
                t if t < 1_800_000 => 4_000,
                _ => 1_000,
            };
            sim.gameplay.actors[i].crusade_tick = now + ms / TICK_MS;
        }
    }
    let owners: Vec<u8> = sim
        .gameplay
        .treasury
        .iter()
        .filter(|(_, t)| t.crusade.is_some())
        .map(|(&k, _)| k)
        .collect();
    for owner in owners {
        let Some(mut crusade) = sim.gameplay.treasury[&owner].crusade.clone() else {
            continue;
        };
        let anchor = crusade.anchor;
        // A crusader leaves the crusade when it dies or changes owner
        // (0x4eaeb0 → 0x43a010).
        crusade.crusaders.retain(|&j| {
            m.objects
                .get(j)
                .is_some_and(|x| alive(x) && x.owner == owner)
        });
        let standing = m.objects.get(anchor).is_some_and(alive);
        // 0x439d50: every 500 ms a crusader arrives on one of the cells,
        // after at most cells / 4 + 1 tries for a free one.
        if standing && crusade.left > 0 && crusade.next <= now {
            crusade.next = now + 10;
            let tries = (crusade.cells.len() / 4 + 1).min(255);
            let civilization = m.objects[anchor].civilization;
            for _ in 0..tries {
                if crusade.cells.is_empty() {
                    break;
                }
                let p = crusade.cells[random(sim, crusade.cells.len() as u64) as usize];
                let mut probe = m.objects[anchor].clone();
                probe.class = 11;
                probe.x = p[0];
                probe.y = p[1];
                if !placement(g, m, lib, &probe, None, false) {
                    continue;
                }
                if let Ok(j) = new_unit(sim, m, lib, 11, owner, civilization, p) {
                    crusade.left -= 1;
                    crusade.crusaders.push(j);
                    // 0x439ec3: six stars for 3 s around the newcomer, its
                    // rectangle grown by 16 and kept on the map (0x473d80).
                    let last = g.side as i32 * 32 - 1;
                    let r = rect_around(lib, &m.objects[j], 16).map(|n| n.clamp(0, last));
                    sparkles(sim, r, 6, 3000);
                }
                break;
            }
        }
        // 0x43a010, 0x439a00: the anchor goes when its crusaders have all
        // arrived and died; then the cathedral starts again from 4.
        let t = sim.gameplay.treasury.entry(owner).or_default();
        if !standing || (crusade.left == 0 && crusade.crusaders.is_empty()) {
            t.crusade = None;
            t.crusaders = Some(4);
            if standing {
                kill(sim, m, g, lib, anchor);
            }
        } else {
            t.crusade = Some(crusade);
        }
    }
}
/// Refill of the taverns (message 0x4204, 0x4d24c0): a second after the
/// tavern comes into play (0x4d1e5f), then every 60 s, or 360 s in a game of
/// type 1 with resources 2 (the one played with `dm.ais`, 0x4d2567), each
/// tavern adds an offer of mercenaries when its kingdom knows Mercenaries
/// (K+0xf4) and one of heroes with Heroes Quest (K+0xf8).
fn tavern_offers(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, c: &Catalog) {
    for i in 0..m.objects.len().min(sim.gameplay.actors.len()) {
        if m.objects[i].class != class::TAVERN || !alive(&m.objects[i]) {
            continue;
        }
        let born = sim.gameplay.actors[i].born;
        let due = *sim.gameplay.actors[i]
            .hire_due
            .get_or_insert(born + 1000 / TICK_MS);
        if sim.ticks < due {
            continue;
        }
        let known = researches(m, m.objects[i].owner);
        let has = |key: &str| known.iter().any(|r| r.eq_ignore_ascii_case(key));
        let lists = [has("te_mercenaries"), has("te_heroesquest")];
        for (list, open) in lists.into_iter().enumerate() {
            if open {
                add_hire(sim, m, lib, c, i, list);
            }
        }
        let ms = if m.game_rules.game_type == 1 && m.game_rules.resource_level == 2 {
            360_000
        } else {
            60_000
        };
        sim.gameplay.actors[i].hire_due = Some(sim.ticks + ms / TICK_MS);
    }
}
/// A new offer (0x4d2030): a civilization R(1, 3), a button of its barracks
/// menu R(0, n − 1), then 8–12 mercenaries at level 0 or one hero at level
/// 10–30, plus Fame (0x4d22b6). A full list loses its oldest offer first.
fn add_hire(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
    list: usize,
) {
    let civilization = 1 + random(sim, 3) as u8;
    let mut barrack = m.objects[i].clone();
    barrack.class = class::BARRACK;
    barrack.civilization = civilization;
    let Some(menu) = lib.property(&barrack, "menu") else {
        return;
    };
    let entries = c.menus.entries(menu);
    if entries.is_empty() {
        return;
    }
    let order = entries[random(sim, entries.len() as u64) as usize]
        .0
        .clone();
    let name = c.class_name(&order);
    let Some(class) = CLASSES.iter().position(|k| k.eq_ignore_ascii_case(&name)) else {
        return;
    };
    let hires = &mut sim.gameplay.actors[i].hires[list];
    if hires.len() >= 8 {
        hires.remove(0);
    }
    let (count, level) = if list == 1 {
        (1, 10 + random(sim, 21) as u16)
    } else {
        (8 + random(sim, 5) as u16, 0)
    };
    let id = sim.gameplay.next_hire;
    sim.gameplay.next_hire += 1;
    sim.gameplay.actors[i].hires[list].push(Hire {
        id,
        class,
        civilization,
        count,
        level: level + experience_rules(m, m.objects[i].owner).fame,
        order,
    });
}
/// The price of an offer (vt+0xf8, 0x4d23b0): the class's cost for its
/// civilization in the kingdom's records × count × 1.6, truncated, plus 30
/// gold a level and 15 a level of each other resource the class costs.
pub fn hire_price(cost: [u32; 4], count: u16, level: u16) -> [u32; 4] {
    let mut price = [0; 4];
    for (r, p) in price.iter_mut().enumerate() {
        let base = (cost[r] as u64 * count as u64 * 160 / 100) as u32;
        if r == 0 || base != 0 {
            *p = base + if r == 0 { 30 } else { 15 } * level as u32;
        }
    }
    price
}
/// The Asian markets (message 0x4209, a second after the market appears and
/// then every 60 s, 0x479c34, 0x47a170): with Jewelry (K+0x10e) one market
/// of the kingdom offers its items (K+0x12e: the first to come, while it
/// stands), adding one of class book, equipment, book or potion drawn
/// R(0, 3) (0x58b124) with a random record (vt+0x2c8); a full list loses its
/// oldest first (0x479cc0).
fn market_jewels(sim: &mut Simulation, m: &MissionWorld) {
    for i in 0..m.objects.len().min(sim.gameplay.actors.len()) {
        if m.objects[i].class != class::MARKET || !alive(&m.objects[i]) {
            continue;
        }
        let born = sim.gameplay.actors[i].born;
        let due = *sim.gameplay.actors[i]
            .hire_due
            .get_or_insert(born + 1000 / TICK_MS);
        if sim.ticks < due {
            continue;
        }
        sim.gameplay.actors[i].hire_due = Some(sim.ticks + 60_000 / TICK_MS);
        let owner = m.objects[i].owner;
        if !researches(m, owner)
            .iter()
            .any(|r| r.eq_ignore_ascii_case("te_jewelry"))
        {
            continue;
        }
        let jewels = sim.gameplay.jewels.entry(owner).or_default();
        if jewels.market.is_some_and(|k| {
            k != i
                && m.objects
                    .get(k)
                    .is_some_and(|x| alive(x) && x.owner == owner)
        }) {
            continue;
        }
        jewels.market = Some(i);
        const CLASSES_ON_OFFER: [usize; 4] = [
            class::ITEM_MAGIC,
            class::ITEM_PLUSPLUS,
            class::ITEM_MAGIC,
            class::ITEM_HEALTH,
        ];
        let class = CLASSES_ON_OFFER[random(sim, 4) as usize];
        let (image, size, fields, spell) = random_item(sim, class);
        let id = sim.gameplay.next_hire;
        sim.gameplay.next_hire += 1;
        let offers = &mut sim.gameplay.jewels.entry(owner).or_default().offers;
        if offers.len() >= 8 {
            offers.remove(0);
        }
        offers.push(Jewel {
            id,
            class,
            image: image.into(),
            size,
            fields,
            spell,
        });
    }
}
/// The random record of a new item (vt+0x2c8): a book of a spell R(0, 20),
/// 8 giving 1 and 13 giving 17 (0x462380), with one charge in the first ten
/// minutes, 1–3 until twenty and 1–10 later (0x464650); equipment R(0, 2): a
/// sword or a bow of attack 5–20 or a shield of defence 5–20 (0x463610); a
/// potion R(0, 4) of attack or defence 5–20, health 25–100, experience 5–20
/// or mana 25–100 (0x464d20). Picture, record size, word fields and spell.
fn random_item(sim: &mut Simulation, class: usize) -> (&'static str, usize, Vec<(usize, u16)>, u8) {
    let roll = |sim: &mut Simulation, low: u16, high: u16| {
        low + random(sim, (high - low + 1) as u64) as u16
    };
    match class {
        class::ITEM_MAGIC => {
            let spell = match random(sim, 21) {
                8 => 1,
                13 => 17,
                s => s as u8,
            };
            let charges = match sim.ticks * TICK_MS / 60_000 {
                0..=10 => 1,
                11..=20 => roll(sim, 1, 3),
                _ => roll(sim, 1, 10),
            };
            ("item_book1", 0x70, vec![(0x64, 0), (0x66, charges)], spell)
        }
        class::ITEM_PLUSPLUS => match random(sim, 3) {
            0 => (
                "item_sword1",
                0x6a,
                vec![(0x60, 1), (0x16, roll(sim, 5, 20))],
                0,
            ),
            1 => (
                "item_bow1",
                0x6a,
                vec![(0x60, 1), (0x16, roll(sim, 5, 20) << 8)],
                0,
            ),
            _ => (
                "item_shield1",
                0x6a,
                vec![(0x60, 2), (0x18, roll(sim, 5, 20))],
                0,
            ),
        },
        _ => match random(sim, 5) {
            0 => (
                "item_potion6",
                0x6a,
                vec![(0x60, 1), (0x16, roll(sim, 5, 20))],
                0,
            ),
            1 => (
                "item_potion7",
                0x6a,
                vec![(0x60, 2), (0x18, roll(sim, 5, 20))],
                0,
            ),
            2 => (
                "item_potion2",
                0x6a,
                vec![(0x60, 4), (0x12, roll(sim, 25, 100))],
                0,
            ),
            3 => (
                "item_potion3",
                0x6a,
                vec![(0x60, 8), (0x1c, roll(sim, 5, 20))],
                0,
            ),
            _ => (
                "item_potion1",
                0x6a,
                vec![(0x60, 0x10), (0x24, roll(sim, 25, 100))],
                0,
            ),
        },
    }
}
/// The item a market offer gives, made from `base` (for its place and
/// owner).
pub fn jewel_object(base: &MapObject, jewel: &Jewel) -> MapObject {
    let mut o = base.clone();
    o.class = jewel.class;
    o.civilization = 0;
    o.data.truncate(0x12);
    fill_item(&mut o, &jewel.image, jewel.size, &jewel.fields, jewel.spell);
    o
}
/// What an item is worth in gold (vt+0x2c4): equipment 4 × (its two attack
/// bytes + defence) (0x4635d0); a book its charges × the spell's rate × 1.5,
/// a summoning rated at the creature's gold (0x4643e0); a potion health +
/// mana + 5 × experience + 2 × (attack + defence) (0x464c90). Other items
/// are worth nothing and cannot be sold (vt+0x2cc).
pub fn item_value(o: &MapObject, creature_gold: impl Fn(usize) -> u32) -> u32 {
    const RATE: [u32; 21] = [
        50, 20, 5, 0, 0, 5, 50, 30, 10, 10, 0, 5, 10, 30, 10, 20, 0, 30, 5, 0, 30,
    ];
    match o.class {
        class::ITEM_PLUSPLUS => {
            4 * (o.byte(0x16) as u32 + o.byte(0x17) as u32 + o.word(0x18) as u32)
        }
        class::ITEM_MAGIC => {
            let rate = match o.byte(0x60) {
                3 => creature_gold(22),
                4 => creature_gold(15),
                10 => creature_gold(16),
                16 => creature_gold(32),
                19 => creature_gold(30),
                s => RATE.get(s as usize).copied().unwrap_or(10),
            };
            let x = rate * o.word(0x66) as u32;
            x + x / 2
        }
        class::ITEM_HEALTH => {
            o.word(0x12) as u32
                + o.word(0x24) as u32
                + 5 * o.word(0x1c) as u32
                + 2 * (o.word(0x16) as u32 + o.word(0x18) as u32)
        }
        _ => 0,
    }
}
/// The items of the market's kingdom it can sell (0x462a77, 0x494b52):
/// equipment, books and potions held by one of the kingdom's objects.
pub fn sellable_items(sim: &Simulation, m: &MissionWorld, owner: u8) -> Vec<usize> {
    // Items held from the start first, then in the order they were picked.
    let n = m.objects.len().min(sim.objects.len());
    (0..n)
        .filter(|j| !sim.gameplay.picks.contains(j))
        .chain(sim.gameplay.picks.iter().copied().filter(|&j| j < n))
        .filter(|&j| {
            matches!(
                m.objects[j].class,
                class::ITEM_PLUSPLUS | class::ITEM_MAGIC | class::ITEM_HEALTH
            ) && m.objects[j].flags & FLAG_INACTIVE == 0
                && sim.objects[j]
                    .holder
                    .and_then(|h| m.objects.get(h))
                    .is_some_and(|h| h.owner == owner)
        })
        .collect()
}
/// A kingdom's spies (K+0x120, «Spy Count» at 0x58c95c): its living spies
/// and those hidden in another unit.
pub fn spy_count(m: &MissionWorld, owner: u8) -> usize {
    m.objects
        .iter()
        .filter(|o| {
            alive(o)
                && o.flags & FLAG_DYING == 0
                && ((o.class == 14 && o.owner == owner)
                    || (owner != 0 && o.class != 14 && o.mole == owner))
        })
        .count()
}
/// The tavern and list holding offer `id`.
pub fn hire_offer(sim: &Simulation, id: u32) -> Option<(usize, usize, &Hire)> {
    sim.gameplay.actors.iter().enumerate().find_map(|(i, a)| {
        a.hires
            .iter()
            .enumerate()
            .find_map(|(list, hires)| hires.iter().find(|h| h.id == id).map(|h| (i, list, h)))
    })
}
/// Timers of the native effects (messages 0x4100 + id): discord gives the
/// unit back to its kingdom, hypnosis ends the borrowed relations.
fn expire_effects(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary) {
    for i in 0..sim.gameplay.actors.len().min(m.objects.len()) {
        if sim.gameplay.actors[i].effects.is_empty() {
            continue;
        }
        let now = sim.ticks;
        let (ended, active): (Vec<_>, Vec<_>) = std::mem::take(&mut sim.gameplay.actors[i].effects)
            .into_iter()
            .partition(|e| e.until <= now);
        sim.gameplay.actors[i].effects = active;
        for e in ended {
            // 0x4e5aa0 undoes each effect; curse and freeze put back the
            // values they saved, which may overwrite later changes.
            match e.kind {
                effect::DISCORD if alive(&m.objects[i]) => transfer_owner(sim, m, lib, i, e.owner),
                effect::HYPNOSIS => {
                    sim.gameplay.actors[i].allegiance = None;
                    // 0x4e5c4b: every kingdom's hold goes back to the
                    // diplomacy.
                    for k in 1..=8 {
                        set_control(sim, m, i, k, 0);
                    }
                }
                effect::BLESS if e.saved == 0 && e.attack == 0 => {
                    let s = &mut sim.objects[i];
                    s.attack = s.attack.saturating_sub(5);
                }
                effect::CURSE if e.saved != 0 => {
                    let s = &mut sim.objects[i];
                    s.attack = (e.saved - 1) & 0xffff;
                    s.defence = (e.saved - 1) >> 16;
                }
                effect::FREEZE if e.saved != 0 => sim.objects[i].defence = e.saved - 1,
                _ => {}
            }
        }
    }
}
/// Native 0x44d1e0, every 5000 ms: a field nobody has worked for 60 s
/// loses 10 hit points and disappears at zero.
fn decay_fields(sim: &mut Simulation, m: &mut MissionWorld, g: &MapGrid, lib: &ObjectLibrary) {
    for i in 0..m.objects.len() {
        if m.objects[i].class != class::FIELD || !alive(&m.objects[i]) {
            continue;
        }
        let last = *sim.gameplay.fields.entry(i).or_insert(sim.ticks);
        if sim.ticks.saturating_sub(last) > 1200 {
            m.objects[i].health = m.objects[i].health.saturating_sub(10);
            // 0x44cd50: the grow picture, column 0.
            m.objects[i].variant = 0;
            if m.objects[i].health == 0 {
                sim.gameplay.fields.remove(&i);
                kill(sim, m, g, lib, i);
            }
        }
    }
}

#[cfg(test)]
#[path = "gameplay/parity_tests.rs"]
mod parity_tests;

#[cfg(test)]
#[path = "gameplay/ai_tests.rs"]
mod ai_tests;
#[cfg(test)]
#[path = "gameplay/cheats_tests.rs"]
mod cheats_tests;

#[cfg(test)]
#[path = "gameplay/animation_tests.rs"]
mod animation_tests;
#[cfg(test)]
#[path = "gameplay/button_tests.rs"]
mod button_tests;
#[cfg(test)]
#[path = "gameplay/campaign_tests.rs"]
mod campaign_tests;
#[cfg(test)]
#[path = "gameplay/combat_tests.rs"]
mod combat_tests;
#[cfg(test)]
#[path = "gameplay/gate_tests.rs"]
mod gate_tests;
#[cfg(test)]
#[path = "gameplay/holder_tests.rs"]
mod holder_tests;
#[cfg(test)]
#[path = "gameplay/mission_tests.rs"]
mod mission_tests;
#[cfg(test)]
#[path = "gameplay/movement_tests.rs"]
mod movement_tests;
#[cfg(test)]
#[path = "gameplay/production_tests.rs"]
mod production_tests;
#[cfg(test)]
#[path = "gameplay/stats_tests.rs"]
mod stats_tests;
#[cfg(test)]
#[path = "gameplay/systemic_tests.rs"]
mod systemic_tests;
#[cfg(test)]
#[path = "gameplay/technology_tests.rs"]
mod technology_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires installed original archives"]
    fn player_gameplay_population_wait_explains_and_resumes_without_double_charging() {
        for civ in 1..=3 {
            let (c, lib, mut m, mut g, mut sim) = setup(civ);
            m.game_rules.population = 1;
            let order = match civ {
                1 => "e_peon",
                2 => "r_peon",
                _ => "a_peon",
            };
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &Request::command(order, vec![0]),
            )
            .unwrap();
            let paid = m.kingdoms[0].resources;
            step(&mut sim, &mut m, &mut g, &lib, &c, 20);
            assert_eq!(sim.gameplay.actors[0].jobs[0].remaining, 0);
            assert!(
                sim.gameplay.actors[0]
                    .production_blocked
                    .as_deref()
                    .unwrap()
                    .contains("popolazione")
            );
            assert_eq!(m.objects.len(), 4);
            m.game_rules.population = 100;
            step(&mut sim, &mut m, &mut g, &lib, &c, 1);
            assert_eq!(m.objects.len(), 5);
            assert!(sim.gameplay.actors[0].jobs.is_empty());
            assert!(sim.gameplay.actors[0].production_blocked.is_none());
            assert_eq!(m.kingdoms[0].resources, paid);
            // The new unit is announced (0x41f6d9); `sound_create` (K+0x68,
            // 0x495325) is loaded but never played by the original.
            assert!(sim.sounds.iter().any(|s| s.object == 4
                && s.property == "@notify/unitdone.wav"
                && s.channel == crate::audio::Channel::Unit));
            assert!(!sim.sounds.iter().any(|s| s.property == "sound_create"));
        }
    }
    #[test]
    #[ignore = "Requires installed original game archives"]
    fn player_gameplay_reserved_scenery_ids_do_not_block_construction_or_training() {
        for civ in 1..=3 {
            let (c, lib, mut m, mut g, mut sim) = setup(civ);
            let mut decoration = m.objects[0].clone();
            decoration.class = class::GROUND_PATCH;
            decoration.id = 65535;
            decoration.owner = 0;
            decoration.health = 0;
            m.objects.push(decoration);
            let mut old = m.objects[1].clone();
            old.id = 0xfc16;
            old.flags |= FLAG_INACTIVE;
            m.objects.push(old);
            let before = m.objects.len();
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &request("e_barrack", vec![1], [20, 10], None),
            )
            .unwrap();
            step(&mut sim, &mut m, &mut g, &lib, &c, 80);
            assert_eq!(m.objects[before].flags & FLAG_CONSTRUCTION, 0);
            let prefix = match civ {
                1 => "e",
                2 => "r",
                _ => "a",
            };
            let order = if civ == 3 {
                "a_rhonin".into()
            } else {
                format!("{prefix}_footman")
            };
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &Request::command(order, vec![before]),
            )
            .unwrap();
            step(&mut sim, &mut m, &mut g, &lib, &c, 20);
            assert!(sim.gameplay.actors[before].jobs.is_empty());
            assert_eq!(m.objects.len(), before + 2);
            assert!(
                m.objects[before..]
                    .iter()
                    .all(|o| o.id < 0xfc17 && o.health > 0)
            );
            assert_ne!(m.objects[before].id, m.objects[before + 1].id);
        }
    }
    #[test]
    #[ignore = "Requires installed original game archives"]
    fn player_gameplay_movement_finishes_its_step_when_stopped_redirected_and_saved() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        position(&mut m.objects[1], [20, 20]);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("move", vec![1], [30, 20], None),
        )
        .unwrap();
        let tick = Duration::from_millis(50);
        advance(&mut sim, &mut m, &mut g, &lib, &c, 2 * tick, 10, false);
        // The unit already belongs to the cell it enters; its picture slides
        // there a pixel at a time (0x4debb0).
        assert_eq!([m.objects[1].x, m.objects[1].y], [21, 20]);
        let sliding = motion_offset(&sim, &m, 1);
        assert!(
            sliding[0] < 0. && sliding[0] > -32. && sliding[1] == 0.,
            "{sliding:?}"
        );
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("stop", vec![1]),
        )
        .unwrap();
        assert_eq!(motion_offset(&sim, &m, 1), sliding);
        let restored: Simulation =
            serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
        assert_eq!(motion_offset(&restored, &m, 1), sliding);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("move", vec![1], [20, 30], None),
        )
        .unwrap();
        // The new order waits for the step to end on its cell (0x4e2fe0).
        let first = sim.gameplay.actors[1].walk.clone().unwrap();
        assert_eq!(first.from, [20, 20]);
        while sim.ticks * TICK_MS < first.due_ms {
            assert_eq!([m.objects[1].x, m.objects[1].y], [21, 20]);
            advance(&mut sim, &mut m, &mut g, &lib, &c, tick, 10, false);
        }
        let next = sim.gameplay.actors[1].walk.clone().unwrap();
        assert_eq!(next.from, [21, 20]);
        assert_eq!(m.objects[1].y, 21);
        assert!(sim.gameplay.validate(m.objects.len(), g.side));
    }
    #[test]
    #[ignore = "Requires installed original game archives"]
    fn player_gameplay_native_carry_sheets_and_command_voices_resolve_for_every_civilization() {
        let assets = GameAssets::open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap(),
        )
        .unwrap();
        let audio = crate::archive::Archive::open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("Packs/sounds.wdt")
                .as_path(),
        )
        .unwrap();
        for civ in 1..=3 {
            let (_, lib, mut m, _, mut sim) = setup(civ);
            for sound in lib.sounds_for(&m.objects[1], "sound_command") {
                assert!(
                    sound.0.starts_with("SOUND/COMMAND/")
                        || sound.0.to_ascii_uppercase().starts_with("SOUND/COMMAND/")
                );
                crate::audio::Pcm::decode(&audio.get(&sound.0).unwrap()).unwrap();
            }
            assert_eq!(lib.sounds_for(&m.objects[1], "sound_command").len(), 8);
            for resource in 0..4 {
                let mut cargo = [0; 4];
                cargo[resource] = 10;
                let image = lib.carrying_image(&m.objects[1], cargo, 0).unwrap();
                let native_file = lib.images.get(&image, "image").unwrap();
                let bmp = format!("IMAGES/{}", native_file.replace('\\', "/"));
                let rle = format!("{}.RLE", &bmp[..bmp.len() - 4]);
                assert!(assets.contains(&bmp) || assets.contains(&rle), "{image}");
                sim.gameplay
                    .actors
                    .resize_with(m.objects.len(), Default::default);
                sim.objects[1].order = OrderState::Moving;
                sim.gameplay.actors[1].cargo = cargo;
                m.simulation = Some(sim.clone());
                let built = lib
                    .build_indices(&assets, &m, 64, Some(&BTreeSet::from([1])))
                    .unwrap();
                assert!(built.missing.is_empty());
                assert!(!built.visuals.is_empty());
                sim.objects[1].order = OrderState::Idle;
                m.simulation = Some(sim.clone());
                let stopped = lib
                    .build_indices(&assets, &m, 64, Some(&BTreeSet::from([1])))
                    .unwrap();
                assert!(stopped.visuals.iter().all(|v| v.animation.is_none()));
            }
        }
    }
    #[test]
    fn player_gameplay_repeated_effects_refresh_and_remain_saveable() {
        let mut runtime = Runtime {
            actors: vec![Actor::default()],
            ..Default::default()
        };
        // 0x502770: a new application re-arms the timer at now + duration,
        // shortening it too, and keeps the parameter of the first one.
        for (until, saved) in [(900, 7), (400, 9), (420, 11)] {
            add_timed_effect(
                &mut runtime.actors[0],
                Effect {
                    until,
                    kind: effect::CURSE,
                    saved,
                    ..Default::default()
                },
            );
        }
        assert_eq!(runtime.actors[0].effects.len(), 1);
        assert_eq!(runtime.actors[0].effects[0].until, 420);
        assert_eq!(runtime.actors[0].effects[0].saved, 7);
        for kind in 0..100 {
            add_timed_effect(
                &mut runtime.actors[0],
                Effect {
                    until: 500,
                    kind,
                    ..Default::default()
                },
            );
        }
        assert_eq!(runtime.actors[0].effects.len(), 32);
        assert!(runtime.validate(1, 64));
        let restored: Runtime =
            serde_json::from_slice(&serde_json::to_vec(&runtime).unwrap()).unwrap();
        assert_eq!(restored, runtime);
        assert!(restored.validate(1, 64));
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_tavern_loans_gambling_and_exchange_follow_the_native_rules() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        m.rules[0].initial_researches.push("te_usury".into());
        m.rules[0].initial_researches.push("te_gambling".into());
        let tavern = new_unit(&mut sim, &mut m, &lib, 48, 1, 1, [30, 30]).unwrap();
        let gold = |m: &MissionWorld| m.kingdoms[0].resources[0];
        // 0x4d2cec: one loan of 1000 per kingdom; repaid with 200 more.
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("loan_gold", vec![tavern]),
        )
        .unwrap();
        assert_eq!(gold(&m), 11000);
        assert!(
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &Request::command("loan_stone", vec![tavern])
            )
            .is_err()
        );
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("loan_return", vec![tavern]),
        )
        .unwrap();
        assert_eq!(gold(&m), 9800);
        // The next loan is 1200, but only a minute after the repayment.
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1199);
        assert_eq!(sim.gameplay.treasury[&1].loan_amount, 1000);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        assert_eq!(sim.gameplay.treasury[&1].loan_amount, 1200);
        // D_GAMBLE: the stake is lost, or paid back times the odds.
        let before = m.kingdoms[0].resources[3];
        let bet = Request {
            resource: 3,
            amount: 100,
            option: 3,
            ..Request::command("gamble", vec![tavern])
        };
        issue(&mut sim, &mut m, &g, &lib, &c, &bet).unwrap();
        let after = m.kingdoms[0].resources[3];
        assert!(
            after == before - 100 || after == before + 1000,
            "{before} -> {after}"
        );
        let too_much = Request {
            amount: after + 50,
            ..bet.clone()
        };
        assert!(issue(&mut sim, &mut m, &g, &lib, &c, &too_much).is_err());
        // D_TRADE1: 1000 stone for 100 gold, one unit every 300 ms.
        let (stone, gold0) = (m.kingdoms[0].resources[1], gold(&m));
        let swap = Request {
            resource: 1,
            resource2: 0,
            amount: 1000,
            ..Request::command("trade1", vec![tavern])
        };
        issue(&mut sim, &mut m, &g, &lib, &c, &swap).unwrap();
        assert_eq!(m.kingdoms[0].resources[1], stone - 1000);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 60);
        assert_eq!(gold(&m), gold0 + 10);
        // Cancelling refunds the undelivered part in proportion.
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("cancel", vec![tavern]),
        )
        .unwrap();
        assert_eq!(m.kingdoms[0].resources[1], stone - 1000 + 900);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 60);
        assert_eq!(gold(&m), gold0 + 10);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn the_active_item_follows_pickups_marks_drops_and_backspace() {
        let (c, lib, mut m, g, mut sim) = setup(1);
        let knight = new_unit(&mut sim, &mut m, &lib, 3, 1, 1, [20, 20]).unwrap();
        let item = |sim: &mut Simulation, m: &mut MissionWorld, class| {
            new_unit(sim, m, &lib, class, 0, 0, [21, 20]).unwrap()
        };
        let active = |sim: &Simulation| {
            sim.objects[knight]
                .contents
                .iter()
                .copied()
                .filter(|&k| sim.objects[k].active)
                .collect::<Vec<_>>()
        };
        // 0x4629e3: the first pickup becomes active, a later one not…
        let sword = item(&mut sim, &mut m, class::ITEM_PLUSPLUS);
        garrison(&mut sim, &mut m, knight, sword);
        assert_eq!(active(&sim), [sword]);
        // …unless the active one is equipment (class 0x51).
        let potion = item(&mut sim, &mut m, class::ITEM_HEALTH);
        garrison(&mut sim, &mut m, knight, potion);
        assert_eq!(active(&sim), [potion]);
        let book = item(&mut sim, &mut m, class::ITEM_MAGIC);
        garrison(&mut sim, &mut m, knight, book);
        assert_eq!(active(&sim), [potion]);
        // Alt + click (0x463140 with 1): one active at a time.
        let mark = Request {
            order: "item_mark".into(),
            selected: vec![book],
            ..Default::default()
        };
        issue(&mut sim, &mut m, &g, &lib, &c, &mark).unwrap();
        assert_eq!(active(&sim), [book]);
        // Backspace takes the active one (0x4f628c)…
        assert_eq!(quick_item(&sim, &m, knight), Some(book));
        // …and dropped, the holder's first item is marked (0x462c20)…
        assert!(release_item(&mut sim, &mut m, &g, &lib, knight, book));
        assert_eq!(active(&sim), [sword]);
        // …even when the item was not the marked one: two marks, and
        // Backspace takes the first (0x4f628c).
        let mark = Request {
            selected: vec![potion],
            ..mark
        };
        issue(&mut sim, &mut m, &g, &lib, &c, &mark).unwrap();
        let potion2 = item(&mut sim, &mut m, class::ITEM_HEALTH);
        garrison(&mut sim, &mut m, knight, potion2);
        assert_eq!(active(&sim), [potion]);
        assert!(release_item(&mut sim, &mut m, &g, &lib, knight, potion2));
        assert_eq!(active(&sim), [sword, potion]);
        assert_eq!(quick_item(&sim, &m, knight), Some(sword));
        // Without a mark: the last book, else the first potion (0x4f625c).
        sim.objects[sword].active = false;
        sim.objects[potion].active = false;
        assert_eq!(quick_item(&sim, &m, knight), Some(potion));
        // Used up through 0x4ff350 (books, soul gems, sales), the item is
        // out of the list when it hears it left: the next first takes the
        // mark.
        sim.objects[sword].active = true;
        consume(&mut sim, &mut m, sword);
        assert_eq!(active(&sim), [potion]);
        // A drunk potion hears it while still in the list (0x5047f0): the
        // first one takes the mark away with it.
        garrison(&mut sim, &mut m, knight, potion2);
        assert_eq!(active(&sim), [potion]);
        drink(&mut sim, &mut m, &lib, &c, knight, potion);
        assert_eq!(active(&sim), Vec::<usize>::new());
        assert_eq!(quick_item(&sim, &m, knight), Some(potion2));
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn the_selection_view_shows_own_names_subtitles_and_discord() {
        let (_, lib, mut m, _, mut sim) = setup(1);
        let knight = new_unit(&mut sim, &mut m, &lib, 3, 1, 1, [20, 20]).unwrap();
        let lines = |m: &MissionWorld, sim: &Simulation| {
            let mut w = m.clone();
            w.simulation = Some(sim.clone());
            crate::hud::name_lines(&w, &lib, &lib.locale, "Io", knight)
        };
        let class = lib.class_label(&m.objects[knight]);
        // 0x505860, 0x505890: the kingdom (the player's own name) and the class.
        assert_eq!(lines(&m, &sim), (Some("Io".into()), class.clone()));
        // An own name (+0x42) takes the first line, a subtitle (+0x46) the second.
        m.objects[knight].name = "Lancillotto".into();
        assert_eq!(lines(&m, &sim), (Some("Lancillotto".into()), class));
        m.objects[knight].subtitle = "del Lago".into();
        assert_eq!(
            lines(&m, &sim),
            (Some("Lancillotto".into()), "del Lago".into())
        );
        // 0x4e5ce3: a unit in discord shows «Persa».
        discord(&mut sim, &mut m, &lib, knight);
        assert_eq!(lines(&m, &sim).0, Some("Persa".into()));
        // A spy shows its record (0x4e5ce0, 0x4e5d50) to its kingdom: its
        // name, else the kingdom's, and its class with its energy (+0xa).
        let me = m.player();
        let other = if me == 1 { 2 } else { 1 };
        let spy = new_unit(&mut sim, &mut m, &lib, 14, me, 1, [24, 20]).unwrap();
        let spy_class = lib.class_label(&m.objects[spy]);
        let of = |m: &MissionWorld, sim: &Simulation, i| {
            let mut w = m.clone();
            w.simulation = Some(sim.clone());
            crate::hud::name_lines(&w, &lib, &lib.locale, "Io", i)
        };
        sim.gameplay.actors[spy].spy_energy = 73;
        assert_eq!(
            of(&m, &sim, spy),
            (Some("Io".into()), format!("{spy_class} (73)"))
        );
        // Hidden in another kingdom's unit, the host shows it with the
        // record's name and subtitle; the host's kingdom sees the host.
        m.objects[spy].name = "Mata".into();
        m.objects[spy].subtitle = "Hari".into();
        let host = new_unit(&mut sim, &mut m, &lib, 3, other, 1, [26, 20]).unwrap();
        let o = m.objects[spy].clone();
        sim.gameplay.actors[host].mole = Some(Mole {
            kingdom: o.owner,
            civilization: o.civilization,
            name: o.name.clone(),
            subtitle: o.subtitle.clone(),
            energy: 12,
            ..Default::default()
        });
        m.objects[host].mole = me;
        assert_eq!(
            of(&m, &sim, host),
            (Some("Mata".into()), "Hari (12)".into())
        );
        let host_class = lib.class_label(&m.objects[host]);
        m.objects[host].mole = 0;
        sim.gameplay.actors[host].mole.as_mut().unwrap().kingdom = other;
        let owner = of(&m, &sim, host).0;
        assert_ne!(owner, Some("Mata".into()));
        assert_eq!(of(&m, &sim, host).1, host_class);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn attack_and_defence_potions_take_opposite_pictures_in_the_workshop_and_at_random() {
        // A potion loads record +0x16 into +0x62, shown as attack, and +0x18
        // into +0x60, shown as defence (0x4647dc, 0x4648c3). The workshop
        // writes +0x16 under `item_potion7` (0x465f8d); a random potion
        // writes +0x62 under `item_potion6` (0x464d73).
        let (_, _, _, _, mut sim) = setup(1);
        let mut seen = BTreeMap::new();
        for _ in 0..200 {
            let (image, _, fields, _) = random_item(&mut sim, class::ITEM_HEALTH);
            seen.insert(image, fields);
        }
        assert_eq!(
            seen["item_potion6"],
            vec![(0x60, 1), (0x16, seen["item_potion6"][1].1)]
        );
        assert_eq!(
            seen["item_potion7"],
            vec![(0x60, 2), (0x18, seen["item_potion7"][1].1)]
        );
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_magic_workshop_makes_items_with_the_native_formulas() {
        // The magic workshop is Arab.
        let (c, lib, mut m, mut g, mut sim) = setup(2);
        let workshop = new_unit(&mut sim, &mut m, &lib, 69, 1, 2, [30, 30]).unwrap();
        let gold = |m: &MissionWorld| m.kingdoms[0].resources[0];
        let make = |category, item, amount| Request {
            option: category,
            item,
            amount,
            ..Request::command("build_item", vec![workshop])
        };
        // A sword of 100 gold: paid at once; its 25 000 ms run in 666 ms
        // ticks from 50 ms after the order, so it is ready at
        // 50 + 666 × 37 = 24 692 ms (0x41f0bf).
        issue(&mut sim, &mut m, &g, &lib, &c, &make(2, 0, 100)).unwrap();
        assert_eq!(gold(&m), 9900);
        let items = |m: &MissionWorld| {
            m.objects
                .iter()
                .enumerate()
                .filter(|(_, o)| o.item() && alive(o))
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        };
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 493);
        assert!(items(&m).is_empty());
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        let sword = items(&m)[0];
        assert_eq!(m.objects[sword].class, class::ITEM_PLUSPLUS);
        assert_eq!(m.objects[sword].custom_image, "item_sword1");
        assert_eq!(m.objects[sword].word(0x16), 25);
        // 0x466a87 takes the gold a second time when it can.
        assert_eq!(gold(&m), 9800);
        // On the ring around the building, or next to it.
        let w = &m.objects[workshop];
        let [fw, fh] = footprint(&lib, w);
        let s = &m.objects[sword];
        assert!(s.x + 2 >= w.x && s.x <= w.x + fw as u16 + 1);
        assert!(s.y + 2 >= w.y && s.y <= w.y + fh as u16 + 1);
        // A book of healing (5 gold per charge) with 100 gold: 5 charges,
        // ready after gold x 500 ms, then 25 more gold.
        issue(&mut sim, &mut m, &g, &lib, &c, &make(0, 2, 100)).unwrap();
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1000);
        let book = *items(&m).iter().find(|&&i| i != sword).unwrap();
        assert_eq!(m.objects[book].class, class::ITEM_MAGIC);
        assert_eq!(m.objects[book].spell_key(), Some("spell_heal"));
        assert_eq!(m.objects[book].word(0x66), 5);
        assert_eq!(gold(&m), 9800 - 100 - 25);
        // Cancelling gives the gold back.
        issue(&mut sim, &mut m, &g, &lib, &c, &make(1, 2, 60)).unwrap();
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("cancel", vec![workshop]),
        )
        .unwrap();
        assert_eq!(gold(&m), 9675);
        // Books below their minimum are refused (0x4a5220).
        assert!(issue(&mut sim, &mut m, &g, &lib, &c, &make(0, 0, 150)).is_err());
        // An attack potion of 20 gold: `item_potion7`, +0x16 = 10 (0x465f8d).
        issue(&mut sim, &mut m, &g, &lib, &c, &make(1, 3, 20)).unwrap();
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 200);
        let potion = items(&m)
            .into_iter()
            .find(|&i| m.objects[i].custom_image == "item_potion7")
            .unwrap();
        assert_eq!(
            (m.objects[potion].byte(0x60), m.objects[potion].word(0x16)),
            (1, 10)
        );
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_crusades_gather_and_end_like_the_native_cathedral() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        m.rules[0].initial_researches.push("te_crusade".into());
        let cathedral = new_unit(&mut sim, &mut m, &lib, 55, 1, 1, [30, 30]).unwrap();
        // 16 crusaders are ready at the start.
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        assert_eq!(sim.gameplay.treasury[&1].crusaders, Some(16));
        let food = m.kingdoms[0].resources[2];
        let preach = request("preach_crusade", vec![cathedral], [45, 45], None);
        issue(&mut sim, &mut m, &g, &lib, &c, &preach).unwrap();
        assert_eq!(m.kingdoms[0].resources[2], food - 2000);
        assert_eq!(sim.gameplay.treasury[&1].crusaders, None);
        assert_eq!(sim.gameplay.treasury[&1].full_crusades, 1);
        assert!(issue(&mut sim, &mut m, &g, &lib, &c, &preach).is_err());
        let anchor = sim.gameplay.treasury[&1].crusade.as_ref().unwrap().anchor;
        assert_eq!(m.objects[anchor].class, class::CRUSADEANCHOR);
        // One crusader every 500 ms, within 7 cells of the anchor, each with
        // six stars twinkling for 3 s (0x439ec3).
        sim.flashes.clear();
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 10);
        let stars: Vec<&Flash> = sim
            .flashes
            .iter()
            .filter(|f| f.image == "magic_star")
            .collect();
        // The first star at once; each star twinkles two or three times, on
        // the 100 ms timer, each twinkle over before the 3 s are up.
        assert_eq!(stars[0].delay_ms, 0);
        assert!(
            (2 * 6 + 1..=3 * 6).contains(&stars.len()),
            "{}",
            stars.len()
        );
        assert!(
            stars
                .iter()
                .all(|f| f.delay_ms % 100 == 0 && f.delay_ms + 1000 <= 3000)
        );
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 10 * 15);
        let crusaders = sim.gameplay.treasury[&1]
            .crusade
            .as_ref()
            .unwrap()
            .crusaders
            .clone();
        assert_eq!(crusaders.len(), 16);
        for &j in &crusaders {
            let o = &m.objects[j];
            assert_eq!((o.class, o.owner), (11, 1));
            assert!(
                o.x.abs_diff(m.objects[anchor].x) <= 7 && o.y.abs_diff(m.objects[anchor].y) <= 7
            );
        }
        // When they are all dead the anchor goes and the cathedral starts
        // again from 4, adding one every 12 s in the first 15 minutes.
        for j in crusaders {
            kill(&mut sim, &mut m, &g, &lib, j);
        }
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        assert!(!alive(&m.objects[anchor]));
        assert_eq!(sim.gameplay.treasury[&1].crusaders, Some(4));
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 240);
        assert_eq!(sim.gameplay.treasury[&1].crusaders, Some(5));
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_trade_routes_carry_cargo_with_the_native_profit() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        m.rules[0].initial_researches.push("te_traderoutes".into());
        let a = new_unit(&mut sim, &mut m, &lib, 53, 1, 1, [5, 40]).unwrap();
        let b = new_unit(&mut sim, &mut m, &lib, 53, 1, 1, [55, 40]).unwrap();
        let trader = new_unit(&mut sim, &mut m, &lib, 2, 1, 1, [9, 44]).unwrap();
        let start = m.kingdoms[0].resources;
        let route = Request {
            target: Some(b),
            origin: Some(a),
            resource: 0,
            resource2: 1,
            amount: 500,
            ..Request::command("trade", vec![trader])
        };
        issue(&mut sim, &mut m, &g, &lib, &c, &route).unwrap();
        // Gold is loaded at A, a hundred at a time (min(100, 1000 / 2
        // markets)), then sold at B with the profit
        // cargo x distance x 40 / (map x 100) = 500 x 50 x 40 / 6400 = 156.
        let mut sold = false;
        for _ in 0..4000 {
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
            if sim.gameplay.actors[trader]
                .route
                .as_ref()
                .is_some_and(|r| r.cargo_type == 1 && r.cargo > 0)
            {
                sold = true;
                break;
            }
        }
        assert!(sold);
        let now = m.kingdoms[0].resources;
        assert_eq!(now[0], start[0] - 500 + 500 + 156);
        assert!(now[1] < start[1] && (start[1] - now[1]) % 100 == 0);
        // B stays reserved for this trader while it loads there.
        let other = new_unit(&mut sim, &mut m, &lib, 2, 1, 1, [52, 44]).unwrap();
        let second = Request {
            target: Some(a),
            origin: Some(b),
            ..route.clone()
        };
        let second = Request {
            selected: vec![other],
            ..second
        };
        issue(&mut sim, &mut m, &g, &lib, &c, &second).unwrap();
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 40);
        assert_eq!(
            sim.gameplay.actors[trader].route.as_ref().unwrap().reserved,
            Some(b)
        );
        assert!(
            sim.gameplay.actors[other]
                .route
                .as_ref()
                .unwrap()
                .reserved
                .is_none()
        );
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_fields_count_down_and_show_their_growth_like_the_original() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let field = new_unit(&mut sim, &mut m, &lib, class::FIELD, 1, 1, [30, 30]).unwrap();
        m.objects[field].max_health = 100;
        m.objects[field].health = 50;
        m.objects[field].variant = 6;
        // A field short of 100 shows field_grow at row health × 4 / 100.
        assert_eq!(
            lib.image_for(&m.objects[field]).as_deref(),
            Some("field_grow")
        );
        let peon = 1;
        position(&mut m.objects[peon], [30, 30]);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("harvest", vec![peon], [30, 30], Some(field)),
        )
        .unwrap();
        // The first cycle starts the countdown (12–18, stored + 1) and grows
        // the field without counting down while it is still short of 100.
        let mut guard = 0;
        while m.objects[field].health == 50 {
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
            guard += 1;
            assert!(guard < 2000);
        }
        let start = sim.gameplay.actors[peon].field_cycles;
        assert!((13..=19).contains(&start), "{start}");
        assert_eq!((m.objects[field].health, m.objects[field].variant), (60, 0));
        while m.objects[field].health < 100 {
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
            guard += 1;
            assert!(guard < 4000);
        }
        // The cycle that reaches 100 counts down.
        assert_eq!(sim.gameplay.actors[peon].field_cycles, start - 1);
        assert_eq!(lib.image_for(&m.objects[field]).as_deref(), Some("field"));
        // Then one cycle per remaining count, and the food on the cycle after.
        let mut seen = vec![start - 1];
        while sim.gameplay.actors[peon].cargo[2] == 0 {
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
            let n = sim.gameplay.actors[peon].field_cycles;
            if seen.last() != Some(&n) {
                seen.push(n);
            }
            guard += 1;
            assert!(guard < 20000);
        }
        assert_eq!(seen, (0..start).rev().collect::<Vec<u8>>());
        assert_eq!(sim.gameplay.actors[peon].cargo[2], 10);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_cows_leave_meat_that_peasants_carry_home() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let cow = new_unit(&mut sim, &mut m, &lib, class::COW, 1, 1, [30, 30]).unwrap();
        // 300 at birth, +2 a second (every 20 ticks of the clock).
        let start = sim.ticks;
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 200);
        let seconds = (start + 1..=sim.ticks).filter(|t| t % 20 == 0).count() as u16;
        assert!(seconds >= 10);
        assert_eq!(sim.gameplay.meat[&cow], 300 + 2 * seconds);
        let fat = sim.gameplay.meat[&cow];
        // Frozen cows do not fatten.
        sim.gameplay.actors[cow].effects.push(Effect {
            kind: effect::FREEZE,
            until: sim.ticks + 200,
            ..Default::default()
        });
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 100);
        assert_eq!(sim.gameplay.meat[&cow], fat);
        // A dying cow leaves its meat in items of at most 100.
        sim.gameplay.meat.insert(cow, 250);
        kill(&mut sim, &mut m, &g, &lib, cow);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        let items: Vec<usize> = (0..m.objects.len())
            .filter(|&k| m.objects[k].class == class::RESITEM && alive(&m.objects[k]))
            .collect();
        let mut amounts: Vec<u16> = items.iter().map(|&k| m.objects[k].word(0x62)).collect();
        amounts.sort();
        assert_eq!(amounts, vec![50, 100, 100]);
        assert!(
            items
                .iter()
                .all(|&k| m.objects[k].custom_image == "item_meat")
        );
        // A peasant takes one as meat (image_carrymeat) and delivers food.
        let peon = 1;
        let food = m.kingdoms[0].resources[2];
        let item = items[0];
        let amount = m.objects[item].word(0x62) as u32;
        let p = [m.objects[item].x, m.objects[item].y];
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("context", vec![peon], p, Some(item)),
        )
        .unwrap();
        let mut carried = false;
        for _ in 0..2000 {
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
            let a = &sim.gameplay.actors[peon];
            if a.cargo[2] > 0 {
                carried = true;
                let image = lib
                    .carrying_image(&m.objects[peon], a.cargo, a.cargo_source)
                    .unwrap();
                assert!(image.ends_with("_peon_meat"), "{image}");
            }
            if carried && a.cargo[2] == 0 {
                break;
            }
        }
        assert!(carried && !alive(&m.objects[item]));
        assert_eq!(m.kingdoms[0].resources[2], food + amount);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_tending_cows_fetches_food_items_nearby_first() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let cow = new_unit(&mut sim, &mut m, &lib, class::COW, 1, 1, [20, 10]).unwrap();
        sim.objects[cow].mana = 0;
        // Step 0xe (0x4e0c66): food within 8 cells, other items left.
        let wood =
            spawn_resources(&mut sim, &mut m, &g, &lib, [21, 12], 3, 40, "item_wood").unwrap();
        let far =
            spawn_resources(&mut sim, &mut m, &g, &lib, [20, 30], 2, 50, "item_food").unwrap();
        let food_item =
            spawn_resources(&mut sim, &mut m, &g, &lib, [22, 13], 2, 30, "item_food").unwrap();
        let peon = 1;
        let food = m.kingdoms[0].resources[2];
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("harvest", vec![peon], [20, 10], Some(cow)),
        )
        .unwrap();
        for _ in 0..2000 {
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
            if m.kingdoms[0].resources[2] > food {
                break;
            }
        }
        assert_eq!(m.kingdoms[0].resources[2], food + 30);
        assert!(!alive(&m.objects[food_item]));
        assert!(alive(&m.objects[wood]) && alive(&m.objects[far]));
        // Then back to the cows, waiting for one to refill.
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
        assert_eq!(sim.gameplay.actors[peon].intent, Some(Intent::Harvest(cow)));
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_fishing_takes_four_every_1400_ms_and_patches_regrow() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        for y in 30..64 {
            for x in 0..64 {
                g.cells[y * 64 + x] = 0;
            }
        }
        let patch = new_unit(&mut sim, &mut m, &lib, class::FISHPATCH, 0, 0, [20, 40]).unwrap();
        m.objects[patch].data.resize(0x40, 0);
        m.objects[patch].data[0x38..0x3c].copy_from_slice(&10u32.to_le_bytes());
        let boat = new_unit(&mut sim, &mut m, &lib, class::FISHING_BOAT, 1, 1, [21, 40]).unwrap();
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("fishing", vec![boat], [20, 40], Some(patch)),
        )
        .unwrap();
        let mut takes = Vec::new();
        let mut nets = Vec::new();
        for t in 0..400 {
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
            if nets.is_empty() {
                nets = fishing_nets(Some(&sim), patch);
            }
            let n = sim.gameplay.actors[boat].cargo[2];
            if takes.last().map_or(n > 0, |&(_, last)| n != last) {
                takes.push((t, n));
            }
            if !alive(&m.objects[patch]) {
                break;
            }
        }
        // 4, 8, then the last 2 empty the patch, 1400 ms apart.
        let loads: Vec<u32> = takes.iter().map(|&(_, n)| n).collect();
        assert_eq!(loads, vec![4, 8, 10]);
        assert!(takes.windows(2).all(|w| w[1].0 - w[0].0 == 28), "{takes:?}");
        assert!(!alive(&m.objects[patch]));
        // The fishing boat draws one net, the first picked at random
        // (0x451090); none once it stops.
        assert_eq!(nets, vec![sim.gameplay.nets[&patch] as usize]);
        assert!(nets[0] < 3);
        assert!(fishing_nets(Some(&sim), patch).is_empty());
        // A patch no boat fishes grows by 5% every 10 s, up to 2000.
        let other = new_unit(&mut sim, &mut m, &lib, class::FISHPATCH, 0, 0, [40, 50]).unwrap();
        m.objects[other].data.resize(0x40, 0);
        m.objects[other].data[0x38..0x3c].copy_from_slice(&1000u32.to_le_bytes());
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 200);
        assert_eq!(sim.gameplay.resources[&other], 1050);
        sim.gameplay.resources.insert(other, 1990);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 200);
        assert_eq!(sim.gameplay.resources[&other], 2000);
        // It drifts at most one cell a turn, staying on water.
        let o = &m.objects[other];
        assert!(o.x.abs_diff(40) <= 2 && o.y.abs_diff(50) <= 2);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_fish_patches_hold_two_by_two_cells_and_drift_apart() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        // A channel of water one cell wide: a patch stands there with three
        // of its 2×2 cells on land, since only its picture's cell must be
        // water.
        for y in 20..60 {
            g.cells[y * 64 + 20] = 0;
        }
        let a = new_unit(&mut sim, &mut m, &lib, class::FISHPATCH, 0, 0, [20, 40]).unwrap();
        let b = new_unit(&mut sim, &mut m, &lib, class::FISHPATCH, 0, 0, [20, 42]).unwrap();
        for k in [a, b] {
            m.objects[k].data.resize(0x40, 0);
            m.objects[k].data[0x38..0x3c].copy_from_slice(&100u32.to_le_bytes());
        }
        // 0x450a40, 0x450c70: it stands on 2×2 cells and marks them as
        // standing still; a boat still comes to the 3×3 cells around its
        // picture of one cell (vt+0x134).
        assert_eq!(footprint(&lib, &m.objects[a]), [2, 2]);
        let cells = movement::cells_now(&sim, &m, &g, &lib);
        for p in [[20, 40], [21, 40], [20, 41], [21, 41]] {
            assert_ne!(cells.at(p) & pathing::STATIC, 0, "{p:?}");
        }
        assert_eq!(cells.at([22, 40]) & pathing::STATIC, 0);
        assert_eq!(movement::target_rect(&sim, &m, &lib, a), [19, 39, 21, 41]);
        // 0x450fa0: a patch moves only where its picture's cell is free
        // water and none of its 2×2 cells is marked by the other.
        let mut moved = false;
        for _ in 0..400 {
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 25);
            let pa = [m.objects[a].x, m.objects[a].y];
            let pb = [m.objects[b].x, m.objects[b].y];
            moved |= pa != [20, 40] || pb != [20, 42];
            assert!(pa[0] == 20 && pb[0] == 20, "{pa:?} {pb:?}");
            assert!(pa[1].abs_diff(pb[1]) >= 2, "{pa:?} {pb:?}");
        }
        assert!(moved);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_milked_cows_wait_and_poachers_take_them() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        // Kingdom 2 without its computer player, whose peasants would go to
        // work.
        sim.gameplay.ai.players.remove(&2);
        let cow = new_unit(&mut sim, &mut m, &lib, class::COW, 0, 0, [30, 30]).unwrap();
        // A peasant of kingdom 2 beside a neutral cow takes it only with
        // Cow Poaching.
        let thief = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [31, 30]).unwrap();
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 80);
        assert_eq!(m.objects[cow].owner, 0);
        m.rules[1].initial_researches.push("te_cowpoaching".into());
        sim.gameplay.actors[thief].effect_until = sim.ticks + 1000;
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 40);
        assert_eq!(m.objects[cow].owner, 2);
        assert_eq!(sim.gameplay.actors[thief].effect_until, 0);
        // An ally of the owner beside the cow protects it.
        let guard = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [29, 30]).unwrap();
        let other = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 1, [30, 31]).unwrap();
        m.rules[0].initial_researches.push("te_cowpoaching".into());
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 80);
        assert_eq!(m.objects[cow].owner, 2);
        let _ = (guard, other);
        // While a peasant milks it, its milk does not come back (0x4ecf60).
        let mut a = Actor::default();
        a.intent = Some(Intent::Harvest(cow));
        a.work_target = Some(cow);
        sim.gameplay.actors[guard] = a;
        sim.objects[cow].mana = 10;
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        let before = sim.objects[cow].mana;
        for _ in 0..40 {
            if sim.gameplay.actors[guard].intent != Some(Intent::Harvest(cow)) {
                break;
            }
            sim.gameplay.actors[guard].work_target = Some(cow);
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        }
        assert!(sim.objects[cow].mana <= before);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_stances_hold_like_the_native_flag() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        m.game_rules.no_fog = true;
        // A computer player taking the enemy in hand would let its units
        // move again (0x40c720): here it has none.
        sim.gameplay.ai.players.clear();
        let unit = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
        let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [34, 30]).unwrap();
        let attacking =
            |sim: &Simulation| sim.gameplay.actors[unit].intent == Some(Intent::Attack(foe));
        // Stand ground: only targets already in reach (0x503890).
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("stand_ground", vec![unit]),
        )
        .unwrap();
        // Keep the foe from walking in.
        sim.gameplay.actors[foe].stance = 2;
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 120);
        assert!(!attacking(&sim));
        assert_eq!(sim.gameplay.actors[unit].stance, 1);
        // Stop holds only until the third idle scan (0x4e1bd3, 0x4e4eae).
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("stop", vec![unit]),
        )
        .unwrap();
        assert!(sim.gameplay.actors[unit].temporary_hold);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 20);
        assert!(!attacking(&sim));
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 100);
        assert!(attacking(&sim));
        assert_eq!(sim.gameplay.actors[unit].stance, 0);
        // Any order lets the unit move again (0x48f6ba).
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("stand_ground", vec![unit]),
        )
        .unwrap();
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("move", vec![unit], [28, 28], None),
        )
        .unwrap();
        assert_eq!(sim.gameplay.actors[unit].stance, 0);
        // Holding steady neither scans nor answers a blow (program 0x3c).
        sim.gameplay.actors[unit] = Actor {
            stance: 2,
            ..Default::default()
        };
        apply_damage(&mut sim, &mut m, &g, &lib, &c, unit, 1, 0, Some(foe));
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 60);
        assert_eq!(sim.gameplay.actors[unit].intent, None);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn a_computers_unit_drinks_potions_and_reads_books_as_it_takes_them() {
        let (c, lib, mut m, g, mut sim) = setup(1);
        m.game_rules.no_fog = true;
        assert!(ai::controls(&sim, 2));
        let mage = caster(&mut sim, &mut m, &lib, 2, [30, 30]);
        let book = |sim: &mut Simulation, m: &mut MissionWorld, spell: u8, charges: u16| {
            let b = new_unit(sim, m, &lib, class::ITEM_MAGIC, 0, 0, [1, 1]).unwrap();
            m.objects[b].data.resize(110, 0);
            m.objects[b].data[96] = spell;
            m.objects[b].data[99] = 1;
            m.objects[b].data[102..104].copy_from_slice(&charges.to_le_bytes());
            sim.objects[b].max_mana = 5;
            sim.objects[b].mana = 5;
            b
        };
        // 0x4629aa → 0x413f10: a potion is drunk at once, never marked nor
        // listed for sale.
        let potion = new_unit(&mut sim, &mut m, &lib, class::ITEM_HEALTH, 0, 0, [1, 1]).unwrap();
        garrison(&mut sim, &mut m, mage, potion);
        assert!(!sim.objects[potion].active && !sim.gameplay.picks.contains(&potion));
        ai_take_up(&mut sim, &mut m, &g, &lib, &c);
        assert_ne!(m.objects[potion].flags & FLAG_INACTIVE, 0);
        // 0x463d90: a book at its last charge whose spell it lacks is learned.
        let spell = (0..21u8)
            .find(|&s| {
                !summoning(s)
                    && !knows_spell(&sim, &m, &c, &lib, mage, world::spell_key(s).unwrap())
            })
            .unwrap();
        let last = book(&mut sim, &mut m, spell, 1);
        garrison(&mut sim, &mut m, mage, last);
        ai_take_up(&mut sim, &mut m, &g, &lib, &c);
        assert!(sim.gameplay.actors[mage].spells.contains(&spell));
        assert_ne!(m.objects[last].flags & FLAG_INACTIVE, 0);
        // A summoning book with charges to spare is used on its holder at
        // once (0x463e5d), and again each time it is full (0x464280).
        let bats = book(&mut sim, &mut m, 4, 5);
        garrison(&mut sim, &mut m, mage, bats);
        ai_take_up(&mut sim, &mut m, &g, &lib, &c);
        assert_eq!(m.objects[bats].word(102), 4);
        assert_eq!(sim.objects[bats].mana, 0);
        // A unit of the story is left alone (0x18000000).
        m.objects[mage].flags |= FLAG_STORY;
        let other = new_unit(&mut sim, &mut m, &lib, class::ITEM_HEALTH, 0, 0, [1, 1]).unwrap();
        garrison(&mut sim, &mut m, mage, other);
        assert!(sim.gameplay.ai_taken.is_empty());
        assert!(sim.gameplay.picks.contains(&other));
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_an_idle_after_an_order_holds_its_place_for_a_while() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        m.game_rules.no_fog = true;
        sim.gameplay.ai.players.clear();
        let unit = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
        let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [36, 30]).unwrap();
        sim.gameplay.actors[foe].stance = 2;
        let attacking =
            |sim: &Simulation| sim.gameplay.actors[unit].intent == Some(Intent::Attack(foe));
        // The Move ends (priority 4000): its Idle holds the place
        // (0x4e1bd3), the foe in sight but out of reach stays alone…
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("move", vec![unit], [31, 30], None),
        )
        .unwrap();
        for _ in 0..200 {
            if sim.gameplay.actors[unit].intent.is_none() {
                break;
            }
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        }
        assert_eq!(sim.gameplay.actors[unit].last_priority, 4000);
        assert_eq!(sim.gameplay.actors[unit].stance, 1);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 20);
        assert!(!attacking(&sim));
        // …until the third idle tick, when it moves again (0x4e4ebb).
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 100);
        assert!(attacking(&sim));
        assert_eq!(sim.gameplay.actors[unit].stance, 0);
        // Stopping the default Idle (priority 1000) holds nothing; stopping
        // an order does.
        sim.gameplay.actors[unit] = Actor::default();
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("stop", vec![unit]),
        )
        .unwrap();
        assert_eq!(sim.gameplay.actors[unit].stance, 0);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("stop", vec![unit]),
        )
        .unwrap();
        assert_eq!(sim.gameplay.actors[unit].stance, 1);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_soldiers_retaliate_and_call_their_kingdom_for_help() {
        let (c, lib, mut m, g, mut sim) = setup(1);
        m.game_rules.no_fog = true;
        let victim = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
        let near = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [33, 30]).unwrap();
        let far = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [34, 34]).unwrap();
        let guardian = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [50, 50]).unwrap();
        let walker = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [31, 31]).unwrap();
        let peasant = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 1, [29, 29]).unwrap();
        let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [31, 30]).unwrap();
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("guard", vec![guardian], [30, 30], Some(victim)),
        )
        .unwrap();
        assert_eq!(sim.gameplay.actors[guardian].guarding, Some(victim));
        assert_eq!(sim.gameplay.actors[guardian].intent, None);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("move", vec![walker], [20, 31], None),
        )
        .unwrap();
        apply_damage(&mut sim, &mut m, &g, &lib, &c, victim, 1, 0, Some(foe));
        let on_foe =
            |sim: &Simulation, k: usize| sim.gameplay.actors[k].intent == Some(Intent::Attack(foe));
        // The idle victim strikes back (0x413410); its own kingdom's
        // fighters within 3 cells and its guardian at any distance join
        // (0x413950); a plain move goes on; a peasant does not fight for it.
        assert!(on_foe(&sim, victim));
        assert!(on_foe(&sim, near));
        assert!(!on_foe(&sim, far));
        assert!(on_foe(&sim, guardian));
        assert_eq!(
            sim.gameplay.actors[walker].intent,
            Some(Intent::Move([20, 31]))
        );
        assert!(!on_foe(&sim, peasant));
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_guards_come_back_and_patrols_follow_their_points() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let ward = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
        let guard = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [40, 40]).unwrap();
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("guard", vec![guard], [30, 30], Some(ward)),
        )
        .unwrap();
        // The guard stays put at the order and walks back at its next idle
        // scan, to within 3 cells (0x4e4f0f).
        assert_eq!(sim.gameplay.actors[guard].intent, None);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 400);
        let d = distance(&lib, &m.objects[guard], &m.objects[ward]);
        assert!(d <= 3, "{d}");
        assert_eq!(sim.gameplay.actors[guard].intent, None);
        // A new order ends the guard.
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("move", vec![guard], [44, 44], None),
        )
        .unwrap();
        assert_eq!(sim.gameplay.actors[guard].guarding, None);
        // Three points closing near the first make a round (0x42f7d0)…
        let walker = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [20, 20]).unwrap();
        let round = Request {
            waypoints: vec![[22, 20], [26, 24], [22, 24]],
            ..request("patrol", vec![walker], [22, 20], None)
        };
        issue(&mut sim, &mut m, &g, &lib, &c, &round).unwrap();
        let mut seen = Vec::new();
        for _ in 0..1500 {
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
            if let Some(Intent::Circuit(_, next, closed)) = &sim.gameplay.actors[walker].intent {
                assert!(*closed);
                if seen.last() != Some(next) {
                    seen.push(*next);
                }
            }
        }
        assert!(seen.starts_with(&[0, 1, 2, 0]), "{seen:?}");
        // …far apart they end at the last point.
        let open = Request {
            waypoints: vec![[22, 20], [34, 20]],
            ..request("patrol", vec![walker], [22, 20], None)
        };
        issue(&mut sim, &mut m, &g, &lib, &c, &open).unwrap();
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1500);
        assert_eq!((m.objects[walker].x, m.objects[walker].y), (34, 20));
        assert!(matches!(
            sim.gameplay.actors[walker].intent,
            Some(Intent::Circuit(_, 1, false))
        ));
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_native_relations_and_tower_hold_fire() {
        let (c, lib, mut m, g, mut sim) = setup(1);
        let ours = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
        let nine = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 9, 1, [32, 30]).unwrap();
        let ten = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 10, 1, [34, 30]).unwrap();
        let cow = new_unit(&mut sim, &mut m, &lib, class::COW, 0, 0, [36, 30]).unwrap();
        // R[x][9] = 0x40: nobody attacks owner 9; owner 10 fights the players
        // and is fought by them, but spares owner 9; owner 0 is never a target.
        assert!(!auto_hostile(&sim, &m, ours, nine));
        assert!(auto_hostile(&sim, &m, nine, ours));
        assert!(auto_hostile(&sim, &m, ours, ten));
        assert!(auto_hostile(&sim, &m, ten, ours));
        assert!(!auto_hostile(&sim, &m, ten, nine));
        assert!(!auto_hostile(&sim, &m, ours, cow));
        // Hold fire toggles; an explicit attack ends it (0x4db708, 0x4db790).
        let tower = new_unit(&mut sim, &mut m, &lib, 42, 1, 1, [40, 40]).unwrap();
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("holdfire", vec![tower]),
        )
        .unwrap();
        assert!(sim.gameplay.actors[tower].hold_fire);
        assert_eq!(
            object_menu(Some(&sim), &lib, &m.objects[tower], tower).as_deref(),
            Some("tower_stone3")
        );
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("fireatwill", vec![tower]),
        )
        .unwrap();
        assert!(!sim.gameplay.actors[tower].hold_fire);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("holdfire", vec![tower]),
        )
        .unwrap();
        let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [42, 40]).unwrap();
        m.game_rules.no_fog = true;
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("attack", vec![tower], [42, 40], Some(foe)),
        )
        .unwrap();
        assert!(!sim.gameplay.actors[tower].hold_fire);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_fog_remembers_buildings_as_last_seen() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let _ = (&c, &mut g);
        // Sight 1 lights the four corners of the cell (0x453a50): the cell
        // and its neighbours touch a lit vertex, nothing further does.
        let mut fog = crate::simulation::Fog::new(32);
        fog.reveal(10, 10, 1);
        fog.remember();
        assert!(fog.vertex(10, 10).1 && fog.vertex(11, 11).1);
        assert!(!fog.vertex(12, 10).1 && !fog.vertex(9, 10).1);
        assert!(fog.at(9, 9).1 && fog.at(11, 11).1 && !fog.at(12, 10).1);
        fog.reveal(10, 10, 0);
        // An enemy barracks seen by a scout is remembered when the fog
        // covers it, kept after it falls, and dropped when seen again.
        let scout = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [40, 40]).unwrap();
        let barracks = new_unit(&mut sim, &mut m, &lib, 46, 2, 1, [42, 42]).unwrap();
        sim.refresh_visibility(&m, None);
        assert!(sim.visible(&m.objects[barracks]));
        assert!(sim.memory[&barracks].seen);
        assert!(std::ptr::eq(
            sim.appearance(barracks, &m.objects[barracks]).unwrap(),
            &m.objects[barracks]
        ));
        position(&mut m.objects[scout], [5, 30]);
        sim.refresh_visibility(&m, None);
        assert!(!sim.visible(&m.objects[barracks]));
        let health = m.objects[barracks].health;
        m.objects[barracks].health = 1;
        kill(&mut sim, &mut m, &g, &lib, barracks);
        sim.refresh_visibility(&m, None);
        let shown = sim.appearance(barracks, &m.objects[barracks]).unwrap();
        assert_eq!(shown.health, health);
        assert_eq!(shown.flags & (FLAG_INACTIVE | FLAG_DYING), 0);
        position(&mut m.objects[scout], [40, 40]);
        sim.refresh_visibility(&m, None);
        assert!(sim.memory[&barracks].snapshot.is_none());
        // A building never seen stays hidden on explored ground.
        let hidden = new_unit(&mut sim, &mut m, &lib, 46, 2, 1, [55, 10]).unwrap();
        sim.fog.explore_all();
        sim.refresh_visibility(&m, None);
        assert!(sim.known(&m.objects[hidden]));
        assert!(sim.appearance(hidden, &m.objects[hidden]).is_none());
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_queues_have_no_total_cap_but_250_of_a_kind() {
        let (c, lib, mut m, g, mut sim) = setup(1);
        m.kingdoms[0].resources = [1_000_000; 4];
        let castle = 0;
        // 0x42bd40 grows the queue without a cap; research joins it.
        for _ in 0..30 {
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &Request::command("e_peon", vec![castle]),
            )
            .unwrap();
        }
        assert_eq!(sim.gameplay.actors[castle].jobs.len(), 30);
        let research = c
            .orders
            .section_names()
            .find(|key| {
                c.action(key).eq_ignore_ascii_case("technology")
                    && c.permitted(key, &m.objects[castle], &lib)
                    && c.available(key, &m.objects[castle], &m).is_ok()
            })
            .map(str::to_owned);
        if let Some(key) = research {
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &Request::command(&key, vec![castle]),
            )
            .unwrap();
            assert_eq!(sim.gameplay.actors[castle].jobs.len(), 31);
        }
        // 0x41ec63: at most 250 units of one kind.
        for _ in 30..250 {
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &Request::command("e_peon", vec![castle]),
            )
            .unwrap();
        }
        assert!(
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &Request::command("e_peon", vec![castle])
            )
            .is_err()
        );
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_experience_follows_the_native_thresholds_and_techs() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let unit = new_unit(&mut sim, &mut m, &lib, 3, 1, 1, [20, 20]).unwrap();
        // Without Learning the cap is 0: no experience at all (0x5022e0).
        add_experience(&mut sim, &mut m, &lib, &c, unit, 5, true);
        assert_eq!((sim.objects[unit].level, sim.objects[unit].xp), (0, 0));
        // Learning: cap 4; thresholds trunc(100 × level / 100), one level per
        // call and the excess lost.
        m.rules[0].initial_researches.push("te_learning".into());
        let attack = sim.objects[unit].attack;
        add_experience(&mut sim, &mut m, &lib, &c, unit, 9, true);
        assert_eq!((sim.objects[unit].level, sim.objects[unit].xp), (1, 0));
        assert_eq!(sim.objects[unit].attack, attack + 1);
        for _ in 0..1 + 2 + 3 + 10 {
            add_experience(&mut sim, &mut m, &lib, &c, unit, 1, true);
        }
        assert_eq!(sim.objects[unit].level, 4);
        // Body Building: +5 health at each level.
        m.rules[0].initial_researches.push("te_heroism".into());
        m.rules[0].initial_researches.push("te_bodybuilding".into());
        let max = m.objects[unit].max_health;
        for _ in 0..4 {
            add_experience(&mut sim, &mut m, &lib, &c, unit, 1, true);
        }
        assert_eq!(sim.objects[unit].level, 5);
        assert_eq!(m.objects[unit].max_health, max + 5);
        // Level 12 makes a hero with the first free name of HEROES.INI.
        for _ in 0..200 {
            add_experience(&mut sim, &mut m, &lib, &c, unit, 20, true);
        }
        assert!(sim.objects[unit].level >= 12);
        assert_eq!(m.objects[unit].name, "Lancelot");
        // 0x4999d0: the names stay taken; an Arab takes the first Arab one,
        // and with the European ones gone the common ones serve everyone.
        let other = new_unit(&mut sim, &mut m, &lib, 3, 1, 1, [21, 21]).unwrap();
        become_hero(&mut m, &c, other);
        assert_eq!(m.objects[other].name, "Arthur");
        m.objects[other].name.clear();
        m.objects[other].civilization = 2;
        become_hero(&mut m, &c, other);
        assert_eq!(m.objects[other].name, "Ibn Fadlan");
        let euro = c.heroes.iter().filter(|h| h.1 == 2).count();
        let common = c.heroes.iter().position(|h| h.1 == 0xffff).unwrap();
        m.heroes_taken[..euro].fill(true);
        m.objects[other].name.clear();
        m.objects[other].civilization = 1;
        become_hero(&mut m, &c, other);
        assert_eq!(m.objects[other].name, c.heroes[common].0);
        // Kills give one point to the killer, allies none.
        let killer = new_unit(&mut sim, &mut m, &lib, 3, 1, 1, [22, 20]).unwrap();
        let foe = new_unit(&mut sim, &mut m, &lib, 3, 2, 1, [23, 20]).unwrap();
        apply_damage(&mut sim, &mut m, &g, &lib, &c, foe, 10000, 3, Some(killer));
        assert_eq!(sim.objects[killer].level, 1);
        let friend = new_unit(&mut sim, &mut m, &lib, 3, 1, 1, [24, 20]).unwrap();
        apply_damage(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            friend,
            10000,
            3,
            Some(killer),
        );
        assert_eq!((sim.objects[killer].level, sim.objects[killer].xp), (1, 0));
        // Honor: +50% when a hero hits a hero.
        m.rules[0].initial_researches.push("te_honor".into());
        let rival = new_unit(&mut sim, &mut m, &lib, 3, 2, 1, [21, 21]).unwrap();
        m.objects[rival].name = "Merlin".into();
        m.objects[rival].flags |= FLAG_HERO;
        m.objects[rival].max_health = 5000;
        m.objects[rival].health = 5000;
        apply_damage(&mut sim, &mut m, &g, &lib, &c, rival, 100, 3, Some(unit));
        assert_eq!(m.objects[rival].health, 4850);
        let _ = &mut g;
    }
    /// A caster with plenty of mana and a request for `key` at `cell`.
    fn caster(
        sim: &mut Simulation,
        m: &mut MissionWorld,
        lib: &ObjectLibrary,
        owner: u8,
        at: [u16; 2],
    ) -> usize {
        m.game_rules.no_fog = true;
        let i = new_unit(sim, m, lib, 17, owner, 1, at).unwrap();
        sim.objects[i].max_mana = 5000;
        sim.objects[i].mana = 5000;
        sim.gameplay.actors[i].hold_fire = true;
        i
    }
    fn spell(key: &str, cell: [u16; 2], target: Option<usize>) -> Request {
        request(key, vec![], cell, target)
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn hypnosis_hands_the_unit_to_the_kingdoms_commanding_the_caster() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let me = m.player();
        let other = if me == 1 { 2 } else { 1 };
        let mage = caster(&mut sim, &mut m, &lib, me, [10, 30]);
        let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, other, 1, [12, 30]).unwrap();
        let own = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, me, 1, [11, 32]).unwrap();
        assert!(!m.commands(&m.objects[foe]));
        cast(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            mage,
            "spell_hypnotize",
            &spell("", [12, 30], Some(foe)),
        )
        .unwrap();
        // 0x473887 → 0x4e59b2: the kingdoms commanding the caster hold the
        // unit (1), every other loses it (2), its owner too.
        let mut hold = [2; 8];
        hold[me as usize - 1] = 1;
        assert_eq!(m.objects[foe].control, hold);
        assert!(m.commands(&m.objects[foe]));
        assert!(commanded_by(&m, foe, me) && !commanded_by(&m, foe, other));
        let walk = request("move", vec![foe], [14, 33], None);
        assert!(issue(&mut sim, &mut m, &g, &lib, &c, &walk).is_ok());
        // At the end (0x4e5c4b) every hold goes back to the diplomacy, and
        // the unit, of a kingdom not sharing its units, leaves a selection
        // of several (0x506d00 → 0x4532f0).
        sim.gameplay.selected = [foe, own].into();
        sim.gameplay.script.outbox.0.clear();
        for _ in 0..610 {
            advance(
                &mut sim,
                &mut m,
                &mut g,
                &lib,
                &c,
                Duration::from_millis(TICK_MS),
                10,
                false,
            );
            if !has_effect(&sim, foe, effect::HYPNOSIS) {
                break;
            }
        }
        assert_eq!(m.objects[foe].control, [0; 8]);
        assert!(!m.commands(&m.objects[foe]));
        assert_eq!(sim.gameplay.selected, [own].into());
        assert_eq!(
            sim.gameplay
                .script
                .outbox
                .iter()
                .filter(|s| **s == Show::Unselect(foe))
                .count(),
            1
        );
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn spells_aimed_beyond_sight_leave_a_revealer_at_the_point() {
        let (c, lib, mut m, g, mut sim) = setup(1);
        let mage = caster(&mut sim, &mut m, &lib, 1, [10, 30]);
        let sight = sim.objects[mage].sight as i32;
        assert!(sight >= 3 && (sight - 3).pow(2) < 14 * 14);
        let cast_at = |sim: &mut Simulation, m: &mut MissionWorld, key, at| {
            sim.objects[mage].mana = 5000;
            sim.sounds.clear();
            cast(sim, m, &g, &lib, &c, mage, key, &spell("", at, None)).unwrap();
        };
        // 0x472ab0: fire rain 14 cells away, outside sight − 3: three cells
        // around the point for 4000 ms, and the sound from there.
        cast_at(&mut sim, &mut m, "spell_firerain", [24, 30]);
        let v = sim.gameplay.visions.last().unwrap();
        assert_eq!((v.cell, v.radius, v.owner), ([24, 30], 3, 1));
        assert_eq!(v.until, sim.ticks + 80);
        assert!(sim.fog.at(26, 30).1);
        // Discord just outside the circle (11² + 7² > (15 − 3)²): its
        // sound comes from the revealer. On the circle nothing is left.
        assert_eq!(sight, 15);
        cast_at(&mut sim, &mut m, "spell_berserk", [21, 37]);
        assert_eq!(sim.sounds[0].cell, Some([21, 37]));
        let before = sim.gameplay.visions.len();
        cast_at(&mut sim, &mut m, "spell_berserk", [22, 30]);
        assert_eq!(sim.gameplay.visions.len(), before);
        // Within sight less three cells nothing is left, and the sound comes
        // from the caster.
        let before = sim.gameplay.visions.len();
        cast_at(&mut sim, &mut m, "spell_berserk", [11, 30]);
        assert_eq!(sim.gameplay.visions.len(), before);
        assert_eq!((sim.sounds[0].cell, sim.sounds[0].object), (None, mage));
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_fire_rain_and_flame_ball_follow_the_native_missiles() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        // A computer player taking the enemy in hand would let its units
        // move again (0x40c720): here it has none.
        sim.gameplay.ai.players.clear();
        let mage = caster(&mut sim, &mut m, &lib, 1, [10, 30]);
        let enemy = new_unit(&mut sim, &mut m, &lib, 3, 2, 1, [20, 30]).unwrap();
        let own = new_unit(&mut sim, &mut m, &lib, 3, 1, 1, [20, 31]).unwrap();
        for k in [enemy, own] {
            m.objects[k].max_health = 5000;
            m.objects[k].health = 5000;
            // Hold steady (program 0x3c): hit, they neither turn nor move.
            sim.gameplay.actors[k].stance = 2;
        }
        // Fire rain: twenty fireballs of 40 that spare nobody.
        cast(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            mage,
            "spell_firerain",
            &spell("", [20, 30], None),
        )
        .unwrap();
        assert_eq!(
            sim.gameplay
                .projectiles
                .iter()
                .filter(|p| p.kind == missile::FIRE)
                .count(),
            20
        );
        // 0x450750: one made every 170 ms, each falling for 375 ms.
        let mut made: Vec<(u64, u64)> = sim
            .gameplay
            .projectiles
            .iter()
            .filter(|p| p.kind == missile::FIRE)
            .map(|p| (p.launched, p.lands))
            .collect();
        made.sort();
        assert!(made.windows(2).all(|w| w[1].0 > w[0].0), "{made:?}");
        assert!(
            made.iter().all(|&(l, e)| (7..=9).contains(&(e - l))),
            "{made:?}"
        );
        // 0x4507b0: some sound `effects\meteor.wav` as they are made.
        let mut meteors = 0;
        for _ in 0..100 {
            sim.sounds.clear();
            step(&mut sim, &mut m, &mut g, &lib, &c, 1);
            meteors += sim
                .sounds
                .iter()
                .filter(|s| s.property == "@effects/meteor.wav")
                .count();
        }
        assert!((1..20).contains(&meteors), "{meteors}");
        assert!(sim.gameplay.projectiles.is_empty());
        for k in [enemy, own] {
            let lost = 5000 - m.objects[k].health;
            assert!(lost > 0 && lost % 40 == 0, "lost {lost}");
        }
        // Flame ball along a row: 30 beside the target, 60 on its cell.
        let before = m.objects[enemy].health;
        position(&mut m.objects[own], [40, 40]);
        cast(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            mage,
            "spell_flamebolt",
            &spell("", [20, 30], Some(enemy)),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 20);
        assert!(sim.gameplay.projectiles.is_empty());
        assert_eq!(before - m.objects[enemy].health, 90);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_whirlpool_drowns_ships_only_and_fades() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        for y in 20..40 {
            for x in 20..40 {
                g.cells[y * 64 + x] = 0;
            }
        }
        let mage = caster(&mut sim, &mut m, &lib, 1, [10, 30]);
        let ship = new_unit(&mut sim, &mut m, &lib, 23, 2, 1, [25, 29]).unwrap();
        let land = new_unit(&mut sim, &mut m, &lib, 3, 2, 1, [19, 30]).unwrap();
        for k in [ship, land] {
            sim.gameplay.actors[k].hold_fire = true;
        }
        let (ship_hp, land_hp) = (m.objects[ship].health, m.objects[land].health);
        assert!(
            cast(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                mage,
                "spell_whirlpool",
                &spell("", [12, 30], None)
            )
            .is_err()
        );
        cast(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            mage,
            "spell_whirlpool",
            &spell("", [25, 30], None),
        )
        .unwrap();
        let w = m.objects.len() - 1;
        assert_eq!(m.objects[w].class, class::WHIRLPOOL);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 10);
        assert_eq!(ship_hp - m.objects[ship].health, 40);
        assert_eq!(m.objects[land].health, land_hp);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 15 * 40);
        assert_ne!(m.objects[w].flags & FLAG_INACTIVE, 0);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_potions_last_and_curses_restore_what_they_saved() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        // Potion (0x464ac0): permanent attack, higher maximum health.
        let potion = new_unit(&mut sim, &mut m, &lib, class::ITEM_HEALTH, 0, 0, [1, 1]).unwrap();
        m.objects[potion].data.resize(110, 0);
        m.objects[potion].data[0x12..0x14].copy_from_slice(&20u16.to_le_bytes());
        m.objects[potion].data[0x16..0x18].copy_from_slice(&3u16.to_le_bytes());
        sim.objects[potion].holder = Some(1);
        sim.objects[1].contents.push(potion);
        let (attack, max) = (sim.objects[1].attack, m.objects[1].max_health);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("item_use", vec![potion]),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 1000);
        assert_eq!(sim.objects[1].attack, attack + 3);
        assert_eq!(m.objects[1].max_health, max + 20);
        // Bless then curse: the curse saved the blessed attack, so it stays
        // 5 higher once both have ended (0x4e5841, 0x4e5b02).
        let friend = caster(&mut sim, &mut m, &lib, 2, [30, 30]);
        let foe = caster(&mut sim, &mut m, &lib, 1, [26, 30]);
        let target = new_unit(&mut sim, &mut m, &lib, 3, 2, 1, [30, 31]).unwrap();
        let base = sim.objects[target].attack;
        cast(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            friend,
            "spell_bless",
            &spell("", [30, 31], None),
        )
        .unwrap();
        assert_eq!(sim.objects[target].attack, base + 5);
        cast(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            foe,
            "spell_curse",
            &spell("", [30, 31], None),
        )
        .unwrap();
        assert_eq!(sim.objects[target].attack, (base + 5) / 2);
        assert_eq!(sim.objects[target].defence, 0);
        sim.gameplay.actors[target].hold_fire = true;
        step(&mut sim, &mut m, &mut g, &lib, &c, 700);
        assert_eq!(sim.objects[target].attack, base + 5);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn frozen_or_stone_units_obey_nobody() {
        let (c, lib, mut m, g, mut sim) = setup(1);
        sync(&mut sim, &mut m, &lib);
        let peon = (0..m.objects.len())
            .find(|&i| m.objects[i].class == class::PEON && m.objects[i].owner == 1)
            .unwrap();
        let walk = request("move", vec![peon], [20, 20], None);
        // 0x506c30 exits for the effects 6 and 14 (+0x6e bits 0x40 and
        // 0x4000): no menu, no rectangle, no order, the owner's included.
        for kind in [effect::FREEZE, effect::STONE] {
            add_timed_effect(
                &mut sim.gameplay.actors[peon],
                Effect {
                    until: sim.ticks + 100,
                    kind,
                    ..Default::default()
                },
            );
            assert!(issue(&mut sim, &mut m, &g, &lib, &c, &walk).is_err());
            m.simulation = Some(sim.clone());
            assert!(m.commands(&m.objects[peon]) && !m.commands_at(peon));
            sim.gameplay.actors[peon].effects.clear();
        }
        m.simulation = Some(sim.clone());
        assert!(m.commands_at(peon));
        assert!(issue(&mut sim, &mut m, &g, &lib, &c, &walk).is_ok());
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn a_soul_gem_on_the_ground_is_anyones_to_command() {
        let (c, lib, mut m, g, mut sim) = setup(1);
        let mage = caster(&mut sim, &mut m, &lib, 1, [10, 30]);
        let victim = new_unit(&mut sim, &mut m, &lib, 3, 2, 1, [14, 30]).unwrap();
        sim.gameplay.actors[victim].hold_fire = true;
        cast(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            mage,
            "spell_soulbind",
            &spell("", [14, 30], Some(victim)),
        )
        .unwrap();
        let gem = *sim.objects[mage].contents.last().unwrap();
        // Its use, pointed anywhere, does nothing (vt+0x2bc = 0x50b390).
        let usage = request("item_use", vec![gem], [20, 30], None);
        assert_eq!(issue(&mut sim, &mut m, &g, &lib, &c, &usage).unwrap(), "");
        assert_eq!(m.objects[victim].owner, 2);
        // Dropped, of no kingdom: everyone commands it (0x506c30, row 0 of
        // the relations 0x47), and Discord and Kill, sent to it, still
        // need nobody to hold it (0x465340).
        sim.objects[mage].contents.retain(|&k| k != gem);
        sim.objects[gem].holder = None;
        m.objects[gem].flags &= !FLAG_HELD;
        position(&mut m.objects[gem], [12, 32]);
        assert_eq!(m.objects[gem].owner, 0);
        assert!(m.commands(&m.objects[gem]));
        let order = |key: &str| Request::command(key, vec![gem]);
        issue(&mut sim, &mut m, &g, &lib, &c, &order("item_berserk")).unwrap();
        assert_eq!(m.objects[victim].owner, DISCORD_OWNER);
        issue(&mut sim, &mut m, &g, &lib, &c, &order("item_kill")).unwrap();
        assert_eq!(m.objects[victim].health, 0);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_soul_gem_and_raised_bodies() {
        let (c, lib, mut m, g, mut sim) = setup(1);
        let mage = caster(&mut sim, &mut m, &lib, 1, [10, 30]);
        let victim = new_unit(&mut sim, &mut m, &lib, 3, 2, 1, [14, 30]).unwrap();
        sim.gameplay.actors[victim].hold_fire = true;
        // Soul bind keeps the owner and puts a gem in the caster's bag.
        cast(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            mage,
            "spell_soulbind",
            &spell("", [14, 30], Some(victim)),
        )
        .unwrap();
        assert_eq!(m.objects[victim].owner, 2);
        let gem = *sim.objects[mage].contents.last().unwrap();
        assert_eq!(m.objects[gem].class, class::ITEM_SOUL);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("item_kill", vec![gem]),
        )
        .unwrap();
        assert_eq!(m.objects[victim].health, 0);
        // The death of the bound unit destroys its gem (0x465440).
        assert!(!sim.objects[mage].contents.contains(&gem));
        assert_ne!(m.objects[gem].flags & FLAG_INACTIVE, 0);
        // The body becomes a skeleton on its own cell and disappears.
        let mana = sim.objects[mage].mana;
        cast(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            mage,
            "spell_raisedead",
            &spell("", [10, 30], None),
        )
        .unwrap();
        assert_ne!(m.objects[victim].flags & FLAG_INACTIVE, 0);
        let skeleton = m.objects.len() - 1;
        assert_eq!(m.objects[skeleton].class, class::SKELETON);
        assert_eq!([m.objects[skeleton].x, m.objects[skeleton].y], [14, 30]);
        // Animate (0x4737dd): `skeleton_rise` for its 1400 ms.
        assert_eq!(sim.objects[skeleton].order, OrderState::Appearing);
        assert_eq!(sim.gameplay.actors[skeleton].action_image, "skeleton_rise");
        assert_eq!(
            sim.gameplay.actors[skeleton].appearing_until,
            sim.ticks + 1400 / TICK_MS
        );
        // Without bodies the spell still costs its 70.
        cast(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            mage,
            "spell_raisedead",
            &spell("", [10, 30], None),
        )
        .unwrap();
        assert_eq!(sim.objects[mage].mana, mana - 140);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn summoned_creatures_come_out_of_their_spell_before_acting() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        m.game_rules.no_fog = true;
        let mage = caster(&mut sim, &mut m, &lib, 1, [10, 30]);
        sim.objects[mage].mana = 1000;
        let foe = new_unit(&mut sim, &mut m, &lib, 3, 2, 1, [16, 30]).unwrap();
        sim.gameplay.actors[foe].hold_fire = true;
        cast(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            mage,
            "spell_spawnstonegolem",
            &spell("", [15, 30], None),
        )
        .unwrap();
        let golem = (0..m.objects.len())
            .find(|&k| m.objects[k].class == 16)
            .unwrap();
        // 0x473ab3 → op 0x74: `spawn` (20 rows at 20 fps) for 1000 ms…
        assert_eq!(sim.objects[golem].order, OrderState::Appearing);
        assert_eq!(sim.gameplay.actors[golem].action_image, "spawn");
        let health = m.objects[foe].health;
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 19);
        assert_eq!(sim.objects[golem].order, OrderState::Appearing);
        assert_eq!(sim.gameplay.actors[golem].intent, None);
        assert_eq!(m.objects[foe].health, health);
        // …then its default program finds the enemy beside it.
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
        assert_eq!(sim.gameplay.actors[golem].intent, Some(Intent::Attack(foe)));
        // A bat keeps its own picture (vt+0x2c4 = 0x4ae000), for its time.
        cast(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            mage,
            "spell_spawnbat",
            &spell("", [10, 34], None),
        )
        .unwrap();
        let bat = (0..m.objects.len())
            .rfind(|&k| m.objects[k].class == 15)
            .unwrap();
        let a = &sim.gameplay.actors[bat];
        assert_eq!(
            (sim.objects[bat].order, a.action_image.as_str()),
            (OrderState::Appearing, "")
        );
        assert_eq!(
            a.appearing_until,
            sim.ticks + picture_ms(&lib, &m.objects[bat], "image_idle").div_ceil(TICK_MS)
        );
        // An order ends Animate at once (0x4e01c0).
        set_order(&mut sim, bat, Intent::Move([20, 34]), false).unwrap();
        assert_eq!(sim.gameplay.actors[bat].appearing_until, 0);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn fields_are_three_cells_wide_and_a_peasant_finds_another_one() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        // 0x498d34: `field` has no `object`, its 96-pixel frame makes 3×3;
        // `e_outpost` is 130 pixels high, truncated to 4 cells (0x498373).
        let a = new_unit(&mut sim, &mut m, &lib, class::FIELD, 1, 1, [20, 20]).unwrap();
        let b = new_unit(&mut sim, &mut m, &lib, class::FIELD, 1, 1, [26, 20]).unwrap();
        assert_eq!(lib.cell_footprint(&m.objects[a]), [3, 3]);
        assert_eq!(lib.picture_cells("e_outpost"), [4, 4]);
        let dig = |worker, field| Request {
            order: "harvest".into(),
            selected: vec![worker],
            cell: Some([21, 21]),
            target: Some(field),
            ..Default::default()
        };
        // Step 1 of the Dig program: the field's middle cell.
        issue(&mut sim, &mut m, &g, &lib, &c, &dig(1, a)).unwrap();
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 200);
        assert_eq!([m.objects[1].x, m.objects[1].y], [21, 21]);
        // Op 0x25: the field is worked, a second peasant takes the other.
        issue(&mut sim, &mut m, &g, &lib, &c, &dig(2, a)).unwrap();
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 200);
        assert_eq!(sim.gameplay.actors[2].intent, Some(Intent::Harvest(b)));
        assert_eq!([m.objects[2].x, m.objects[2].y], [27, 21]);
        // With no field left free, the program ends.
        issue(&mut sim, &mut m, &g, &lib, &c, &dig(3, a)).unwrap();
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 100);
        assert_eq!(sim.gameplay.actors[3].intent, None);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn guard_lists_take_turns_alt_guards_a_class_and_a_repair_list_only_walks() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let guard = |m: &mut MissionWorld, sim: &mut Simulation, p| {
            new_unit(sim, m, &lib, class::FOOTMAN, 1, 1, p).unwrap()
        };
        let (a, b) = (
            guard(&mut m, &mut sim, [20, 20]),
            guard(&mut m, &mut sim, [21, 20]),
        );
        let order = |selected: Vec<usize>, targets: Vec<usize>, alt: bool| Request {
            order: "guard".into(),
            selected,
            target: targets.first().copied(),
            targets: if targets.len() > 1 {
                targets
            } else {
                Vec::new()
            },
            alt,
            ..Default::default()
        };
        // 0x4e3cf7: each guard takes the next object of the list as its main
        // one, registered last; the first registered is the one it keeps to.
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &order(vec![a, b], vec![1, 2, 3], false),
        )
        .unwrap();
        assert_eq!(sim.gameplay.actors[a].guard_list, [2, 3, 1]);
        assert_eq!(sim.gameplay.actors[a].guarding, Some(2));
        assert_eq!(sim.gameplay.actors[b].guard_list, [1, 3, 2]);
        assert_eq!(sim.gameplay.actors[b].guarding, Some(1));
        // A guarded object that dies leaves the list.
        m.objects[2].health = 0;
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 100);
        assert_eq!(sim.gameplay.actors[a].guard_list, [3, 1]);
        // Alt guards the class and leaves the unit's program alone.
        set_order(&mut sim, a, Intent::Move([30, 30]), false).unwrap();
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &order(vec![a], vec![1], true),
        )
        .unwrap();
        assert_eq!(sim.gameplay.actors[a].guard_class, class::PEON as u16);
        assert!(sim.gameplay.actors[a].guard_list.is_empty());
        assert_eq!(sim.gameplay.actors[a].intent, Some(Intent::Move([30, 30])));
        // 0x4ada2d: a repair list walks to the first building only.
        m.objects[0].health = m.objects[0].max_health / 2;
        let house = new_unit(&mut sim, &mut m, &lib, class::HOUSE, 1, 1, [30, 5]).unwrap();
        m.objects[house].health = m.objects[house].max_health / 2;
        let repair = Request {
            order: "repair".into(),
            selected: vec![3],
            target: Some(0),
            targets: vec![0, house],
            ..Default::default()
        };
        let before = m.objects[0].health;
        issue(&mut sim, &mut m, &g, &lib, &c, &repair).unwrap();
        assert_eq!(
            sim.gameplay.actors[3].move_target,
            Some((0, movement::target_rect(&sim, &m, &lib, 0)))
        );
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 300);
        assert_eq!(sim.gameplay.actors[3].intent, None);
        assert_eq!(m.objects[0].health, before);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn a_move_on_an_object_walks_to_its_rect_and_drops_it_when_unseen() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        m.game_rules.no_fog = true;
        let order = |cell: [u16; 2], target| Request {
            order: "move".into(),
            selected: vec![1],
            cell: Some(cell),
            target: Some(target),
            ..Default::default()
        };
        // 0x503fa0: the castle's rect, taken at the start (vt+0x134).
        issue(&mut sim, &mut m, &g, &lib, &c, &order([6, 6], 0)).unwrap();
        let rect = movement::target_rect(&sim, &m, &lib, 0);
        assert_eq!(sim.gameplay.actors[1].move_target, Some((0, rect)));
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 200);
        assert_eq!(sim.gameplay.actors[1].intent, None);
        let at = [m.objects[1].x as i32, m.objects[1].y as i32];
        assert!(
            (rect[0]..=rect[2]).contains(&at[0]) && (rect[1]..=rect[3]).contains(&at[1]),
            "{at:?} {rect:?}"
        );
        // 0x4def3e: an invisible enemy target ends the Move.
        let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [40, 30]).unwrap();
        issue(&mut sim, &mut m, &g, &lib, &c, &order([40, 30], foe)).unwrap();
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
        assert!(sim.gameplay.actors[1].intent.is_some());
        sim.gameplay.actors[foe].effect_until = u64::MAX;
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 40);
        assert_eq!(sim.gameplay.actors[1].intent, None);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn explore_walks_to_its_point_then_to_the_unexplored_cells_nearest_it() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        sim.refresh_visibility(&m, None);
        let order = |cell| Request {
            order: "explore".into(),
            selected: vec![1],
            cell,
            ..Default::default()
        };
        // The order takes a point (PLACE = command_explore).
        assert!(issue(&mut sim, &mut m, &g, &lib, &c, &order(None)).is_err());
        // 0x415860: the flood from the unit finds a cell with four
        // unexplored corners, the nearest to the point; east of the unit
        // for a point far east, within the 32 levels of the flood.
        let p = unexplored(&sim, &m, &g, &lib, 1, [63, 5]).unwrap();
        let n = g.side + 1;
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            assert!(!sim.fog.explored_vertices[(p[1] as usize + y) * n + p[0] as usize + x]);
        }
        assert!(p[0] > m.objects[1].x && p[0].abs_diff(m.objects[1].x) <= 33);
        issue(&mut sim, &mut m, &g, &lib, &c, &order(Some([40, 5]))).unwrap();
        assert_eq!(
            sim.gameplay.actors[1].intent,
            Some(Intent::Explore([40, 5]))
        );
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 400);
        assert!(
            m.objects[1].x > 20,
            "{:?}",
            [m.objects[1].x, m.objects[1].y]
        );
        // With nothing left unexplored the program ends once the step under
        // way is over.
        sim.fog.explore_all();
        sim.gameplay.actors[1].explore_target = Some([m.objects[1].x, m.objects[1].y]);
        for _ in 0..100 {
            if sim.gameplay.actors[1].intent.is_none() {
                break;
            }
            ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        }
        assert_eq!(sim.gameplay.actors[1].intent, None);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn a_felled_tree_becomes_a_trunk_out_of_sight_or_when_a_sight_moves() {
        let (_c, lib, mut m, mut g, mut sim) = setup(1);
        assert!(lib.trunks > 0);
        sim.refresh_visibility(&m, None);
        sim.fog.changed.clear();
        let tree = (lib.trunks as u16 + 2) << 11;
        // 0x4555bf: out of every sight, a trunk at once, chosen by the cell.
        let far = 50 * 64 + 40;
        g.cells[far] |= tree;
        fell(&mut sim, &mut g, &lib, far);
        let trunk = (3 * 40 + 11 * 50) % lib.trunks as usize + 1;
        assert_eq!((g.cells[far] >> 11) as usize, trunk);
        assert!(sim.gameplay.stumps.contains(&far));
        assert!(sim.gameplay.depleted_trees.is_empty());
        // In the peasants' sight the cell has no tree until a sight comes
        // or goes over its top left corner (0x453da0).
        let near = 5 * 64 + 15;
        g.cells[near] |= tree;
        fell(&mut sim, &mut g, &lib, near);
        assert_eq!(g.cells[near] >> 11, 0);
        assert!(!tree_at(&g, &lib, [15, 5]));
        assert!(sim.gameplay.depleted_trees.contains(&near));
        sim.refresh_visibility(&m, None);
        settle_stumps(&mut sim, &mut g, &lib);
        assert!(sim.gameplay.depleted_trees.contains(&near));
        m.objects[3].x = 17;
        sim.refresh_visibility(&m, None);
        settle_stumps(&mut sim, &mut g, &lib);
        assert!(sim.gameplay.depleted_trees.is_empty());
        let trunk = (3 * 15 + 11 * 5) % lib.trunks as usize + 1;
        assert_eq!((g.cells[near] >> 11) as usize, trunk);
        // 0x44c930: a field put on a tree fells it.
        g.cells[30 * 64 + 30] |= tree;
        let mut ids: BTreeSet<_> = m.objects.iter().map(|o| o.id).collect();
        let field =
            world::starting::new_object(&mut ids, 1, class::FIELD, 1, 1, [30, 30], &lib).unwrap();
        m.objects.push(field);
        fell_under_grounds(&mut sim, &m, &mut g, &lib);
        assert!(!tree_at(&g, &lib, [30, 30]));
        assert!(sim.gameplay.stumps.contains(&(30 * 64 + 30)));
    }
    pub(super) fn setup(civ: u8) -> (Catalog, ObjectLibrary, MissionWorld, MapGrid, Simulation) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let assets = GameAssets::open(root).unwrap();
        let c = Catalog::load(&assets).unwrap();
        let lib = ObjectLibrary::load(&assets, "grass").unwrap();
        let mut m = MissionWorld::default();
        m.kingdoms = vec![
            world::Kingdom {
                name: "Player".into(),
                leader: String::new(),
                active: true,
                color: 1,
                team: 0,
                civilization: civ,
                resources: [10000; 4],
                computer: false,
                ..Default::default()
            },
            world::Kingdom {
                name: "Enemy".into(),
                leader: String::new(),
                active: true,
                color: 2,
                team: 1,
                civilization: 1,
                resources: [10000; 4],
                computer: true,
                ai_mode: 3,
                ..Default::default()
            },
        ];
        m.diplomacy = vec![0x10; 64];
        m.diplomacy[0] = 0x40;
        m.diplomacy[9] = 0x40;
        let mut ids = BTreeSet::new();
        for (class, p) in [(44, [5, 5]), (1, [14, 5]), (1, [15, 5]), (1, [16, 5])] {
            m.objects
                .push(world::starting::new_object(&mut ids, 1, class, 1, civ, p, &lib).unwrap());
        }
        let g = MapGrid {
            name: "test".into(),
            side: 64,
            cells: vec![0x90; 4096],
        };
        let sim = Simulation::new(&m, &lib, 64, "grass");
        (c, lib, m, g, sim)
    }
    /// Exactly `n` ticks of 50 ms.
    fn ticks(
        sim: &mut Simulation,
        m: &mut MissionWorld,
        g: &mut MapGrid,
        lib: &ObjectLibrary,
        c: &Catalog,
        n: u64,
    ) {
        for _ in 0..n {
            advance(sim, m, g, lib, c, Duration::from_millis(TICK_MS), 10, false);
        }
    }
    pub(super) fn step(
        sim: &mut Simulation,
        m: &mut MissionWorld,
        g: &mut MapGrid,
        lib: &ObjectLibrary,
        c: &Catalog,
        seconds: u64,
    ) {
        advance(sim, m, g, lib, c, Duration::from_secs(seconds), 10, false);
    }
    pub(super) fn request(
        order: &str,
        selected: Vec<usize>,
        cell: [u16; 2],
        target: Option<usize>,
    ) -> Request {
        Request {
            order: order.into(),
            selected,
            cell: Some(cell),
            target,
            append: false,
            ..Default::default()
        }
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_uses_other_resource_edges_when_the_nearest_is_blocked() {
        for civ in 1..=3 {
            let (c, lib, mut m, mut g, mut sim) = setup(civ);
            position(&mut m.objects[1], [25, 20]);
            let mine = new_unit(&mut sim, &mut m, &lib, class::RES_GOLD, 0, 0, [30, 20]).unwrap();
            m.objects[mine].data.resize(64, 0);
            m.objects[mine].data[58..62].copy_from_slice(&200u32.to_le_bytes());
            let size = footprint(&lib, &m.objects[mine]);
            assert!(size[0] > 1 && size[1] > 1);
            // The closest contact point is entirely obstructed, but the
            // remaining mine perimeter is accessible from open terrain.
            // Tree image 4: above the trunks, so it blocks (0x5072a0).
            for y in 19..=21 {
                for x in 29..=31 {
                    g.cells[y * g.side + x] |= 4 << 11;
                }
            }
            let before = m.kingdoms[0].resources[0];
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &request("harvest", vec![1], [30, 20], Some(mine)),
            )
            .unwrap();
            step(&mut sim, &mut m, &mut g, &lib, &c, 70);
            assert!(
                sim.gameplay.resources[&mine] < 200,
                "civ {civ}: no collection"
            );
            assert!(m.kingdoms[0].resources[0] > before, "civ {civ}: no deposit");
        }
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_all_civilizations_build_train_and_research() {
        for civ in 1..=3 {
            let (c, lib, mut m, mut g, mut sim) = setup(civ);
            let prefix = match civ {
                1 => "e",
                2 => "r",
                _ => "a",
            };
            let before = m.kingdoms[0].resources;
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &request("e_house", vec![1], [20, 10], None),
            )
            .unwrap();
            assert_eq!(m.objects.len(), 5);
            assert!(m.objects[4].flags & FLAG_CONSTRUCTION != 0);
            assert!(m.kingdoms[0].resources[3] < before[3]);
            step(&mut sim, &mut m, &mut g, &lib, &c, 60);
            assert_eq!(m.objects[4].flags & FLAG_CONSTRUCTION, 0);
            assert_eq!(m.objects[4].health, m.objects[4].max_health);
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &Request::command(format!("{prefix}_peon"), vec![0]),
            )
            .unwrap();
            step(&mut sim, &mut m, &mut g, &lib, &c, 20);
            assert_eq!(m.objects.len(), 6);
            assert_eq!(m.objects[5].civilization, civ);
            // Civilizations share the fields research but keep their own price/time tables.
            let research = c
                .menus
                .entries(lib.property(&m.objects[0], "menu").unwrap())
                .iter()
                .find(|(key, _)| c.action(key) == "technology")
                .map(|(k, _)| k.clone());
            if let Some(key) = research {
                let req = Request::command(&key, vec![0]);
                if issue(&mut sim, &mut m, &g, &lib, &c, &req).is_ok() {
                    step(&mut sim, &mut m, &mut g, &lib, &c, 90);
                    assert!(m.rules[0].initial_researches.contains(&c.class_name(&key)));
                }
            }
            assert!(sim.gameplay.validate(m.objects.len(), 64));
        }
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_fixed_steps_and_saved_orders_are_deterministic() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("move", vec![1], [25, 15], None),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 2);
        let bytes = serde_json::to_vec(&sim).unwrap();
        let mut loaded: Simulation = serde_json::from_slice(&bytes).unwrap();
        let mut m2 = m.clone();
        let mut g2 = g.clone();
        step(&mut sim, &mut m, &mut g, &lib, &c, 10);
        for _ in 0..200 {
            advance(
                &mut loaded,
                &mut m2,
                &mut g2,
                &lib,
                &c,
                Duration::from_millis(50),
                10,
                false,
            );
        }
        assert_eq!(
            serde_json::to_value(&sim).unwrap(),
            serde_json::to_value(&loaded).unwrap()
        );
        assert_eq!((m.objects[1].x, m.objects[1].y), (25, 15));
        assert_eq!((m2.objects[1].x, m2.objects[1].y), (25, 15));
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_game_speed_truncates_each_turn() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        // 0x48c380 at level 5: 37 ms of game time per 50 ms turn, not 37.5.
        for _ in 0..1000 {
            advance(
                &mut sim,
                &mut m,
                &mut g,
                &lib,
                &c,
                Duration::from_millis(50),
                5,
                false,
            );
        }
        assert_eq!(sim.elapsed, Duration::from_millis(37_000));
        // Frames shorter than a turn add up to the same total.
        let start = sim.elapsed;
        for _ in 0..3000 {
            advance(
                &mut sim,
                &mut m,
                &mut g,
                &lib,
                &c,
                Duration::from_micros(16_667),
                5,
                false,
            );
        }
        let turns = (3000 * 16_667) / 50_000;
        let total = sim.elapsed - start;
        assert!(total >= Duration::from_millis(turns * 37));
        assert!(total <= Duration::from_millis((turns + 1) * 37));
        // Level 15 is exact (a turn already begun carries over: start afresh).
        sim.gameplay.scaled_remainder = 0;
        sim.gameplay.turn_game = 0;
        let start = sim.elapsed;
        advance(
            &mut sim,
            &mut m,
            &mut g,
            &lib,
            &c,
            Duration::from_millis(500),
            15,
            false,
        );
        assert_eq!(sim.elapsed - start, Duration::from_millis(750));
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_saved_mid_combat_continues_identically() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        m.game_rules.no_fog = true;
        let archer = new_unit(&mut sim, &mut m, &lib, 4, 1, 1, [14, 5]).unwrap();
        let enemy = new_unit(&mut sim, &mut m, &lib, 3, 2, 1, [21, 5]).unwrap();
        let confused = new_unit(&mut sim, &mut m, &lib, 3, 2, 1, [40, 40]).unwrap();
        // Discord as `cast()` applies it: owner 10 now, the saved owner later.
        let now = sim.ticks;
        add_timed_effect(
            &mut sim.gameplay.actors[confused],
            Effect {
                until: now + 400,
                kind: effect::DISCORD,
                owner: m.objects[confused].owner,
                ..Default::default()
            },
        );
        transfer_owner(&mut sim, &mut m, &lib, confused, DISCORD_OWNER);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("attack", vec![archer], [21, 5], Some(enemy)),
        )
        .unwrap();
        // Save while an arrow is in flight: projectiles, attack cycle and the
        // temporary owner all live in the saved state.
        let mut saved_at = None;
        for _ in 0..200 {
            step(&mut sim, &mut m, &mut g, &lib, &c, 1);
            if !sim.gameplay.projectiles.is_empty() {
                saved_at = Some(sim.ticks);
                break;
            }
        }
        assert!(saved_at.is_some(), "the archer never shot");
        assert_eq!(m.objects[confused].owner, DISCORD_OWNER);
        let mut sim2: Simulation =
            serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
        let mut m2: MissionWorld =
            serde_json::from_slice(&serde_json::to_vec(&m).unwrap()).unwrap();
        let mut g2 = g.clone();
        step(&mut sim, &mut m, &mut g, &lib, &c, 500);
        step(&mut sim2, &mut m2, &mut g2, &lib, &c, 500);
        assert_eq!(
            serde_json::to_value(&sim).unwrap(),
            serde_json::to_value(&sim2).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&m.objects).unwrap(),
            serde_json::to_value(&m2.objects).unwrap()
        );
        // The discord ended in both and the owner came back.
        assert_eq!(m2.objects[confused].owner, 2);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_harvesting_deposits_and_depletes_resources() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let mut ids = m.objects.iter().map(|o| o.id).collect();
        let mut mine = world::starting::new_object(&mut ids, 10, 76, 0, 0, [20, 5], &lib).unwrap();
        mine.data.resize(64, 0);
        mine.data[58..62].copy_from_slice(&30u32.to_le_bytes());
        m.objects.push(mine);
        let before = m.kingdoms[0].resources[0];
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("harvest", vec![1], [20, 5], Some(4)),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 160);
        assert_eq!(m.kingdoms[0].resources[0], before + 30);
        assert_eq!(m.objects[4].dword(58), 0);
        // A real tree (image 4) with its last load left: loads are whole
        // (0x476e10 counts ten of them).
        g.cells[20 * 64 + 20] = 0x90 | 4 << 11;
        sim.gameplay.trees.insert(20 * 64 + 20, 10);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("context", vec![1], [20, 20], None),
        )
        .unwrap();
        let before = m.kingdoms[0].resources[3];
        step(&mut sim, &mut m, &mut g, &lib, &c, 100);
        assert_eq!(
            m.kingdoms[0].resources[3],
            before + resource_yield(&m, 1, 3)
        );
        // The tree fell and, once the peasant's sight moved off, became the
        // trunk of its cell (0x453efc).
        assert!(!tree_at(&g, &lib, [20, 20]));
        let trunk = (3 * 20 + 11 * 20) % lib.trunks as usize + 1;
        assert_eq!((g.cells[20 * 64 + 20] >> 11) as usize, trunk);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_gathering_follows_the_native_programs() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let cell = |x: usize, y: usize| y * 64 + x;
        // Trunks (images up to the first [treename] count) neither block nor
        // can be chopped; the canopy click finds the tree below (0x504151).
        g.cells[cell(30, 30)] = 0x90 | 3 << 11;
        g.cells[cell(30, 32)] = 0x90 | 4 << 11;
        assert!(!tree_at(&g, &lib, [30, 30]) && tree_at(&g, &lib, [30, 32]));
        assert_eq!(clicked_tree(&g, &lib, [30, 30]), Some([30, 32]));
        let peon = &m.objects[1];
        assert!(terrain_free(&g, &lib, peon, [30, 30], false));
        assert!(!terrain_free(&g, &lib, peon, [30, 32], false));
        // A tree gives ten loads; wood scores nothing (only mines do).
        let score = sim.gameplay.score.get(&1).copied().unwrap_or(0);
        let before = m.kingdoms[0].resources[3];
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("context", vec![1], [30, 30], None),
        )
        .unwrap();
        assert_eq!(sim.gameplay.actors[1].intent, Some(Intent::Chop([30, 32])));
        step(&mut sim, &mut m, &mut g, &lib, &c, 600);
        let wood = m.kingdoms[0].resources[3] - before;
        assert_eq!(wood, 10 * resource_yield(&m, 1, 3));
        assert!(!tree_at(&g, &lib, [30, 32]));
        assert_eq!(sim.gameplay.score.get(&1).copied().unwrap_or(0), score);
        // A load of another kind is dropped when a new one is taken, and a
        // dying peasant drops his load as a resource item.
        let mine = new_unit(&mut sim, &mut m, &lib, class::RES_GOLD, 0, 0, [20, 12]).unwrap();
        m.objects[mine].data.resize(64, 0);
        m.objects[mine].data[58..62].copy_from_slice(&500u32.to_le_bytes());
        sim.gameplay.actors[2].cargo = [0, 0, 0, 7];
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("harvest", vec![2], [20, 12], Some(mine)),
        )
        .unwrap();
        let items = |m: &MissionWorld| {
            m.objects
                .iter()
                .filter(|o| o.class == class::RESITEM && alive(o))
                .count()
        };
        let dropped = items(&m);
        for _ in 0..400 {
            step(&mut sim, &mut m, &mut g, &lib, &c, 1);
            if sim.gameplay.actors[2].cargo[0] > 0 {
                break;
            }
        }
        assert_eq!(sim.gameplay.actors[2].cargo[3], 0);
        assert_eq!(items(&m), dropped + 1);
        assert!(sim.gameplay.score.get(&1).copied().unwrap_or(0) > score);
        let p = [m.objects[2].x, m.objects[2].y];
        kill(&mut sim, &mut m, &g, &lib, 2);
        assert_eq!(items(&m), dropped + 2);
        assert!(m.objects.iter().any(|o| o.class == class::RESITEM
            && alive(o)
            && o.x.abs_diff(p[0]).max(o.y.abs_diff(p[1])) <= 2));
        // Without a store the program ends and the load stays carried.
        let castle = (0..m.objects.len())
            .find(|&k| m.objects[k].class == class::CASTLE)
            .unwrap();
        kill(&mut sim, &mut m, &g, &lib, castle);
        sim.gameplay.actors[3].cargo = [10, 0, 0, 0];
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("harvest", vec![3], [20, 12], Some(mine)),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 2);
        assert!(sim.gameplay.actors[3].intent.is_none());
        assert_eq!(sim.gameplay.actors[3].cargo[0], 10);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_rejects_orders_without_charging_and_cancel_refunds() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let before = m.kingdoms[0].resources;
        assert!(
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &request("e_house", vec![1], [5, 5], None)
            )
            .is_err()
        );
        assert_eq!(m.kingdoms[0].resources, before);
        assert!(
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &Request::command("e_knight", vec![1])
            )
            .is_err()
        );
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("e_peon", vec![0]),
        )
        .unwrap();
        assert!(m.kingdoms[0].resources[2] < before[2]);
        step(&mut sim, &mut m, &mut g, &lib, &c, 1);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("cancel", vec![0]),
        )
        .unwrap();
        assert_eq!(m.kingdoms[0].resources, before);
        assert!(sim.gameplay.actors[0].jobs.is_empty());
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_combat_reaction_without_enemy_ai() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        m.game_rules.no_fog = true;
        let mut ids = m.objects.iter().map(|o| o.id).collect();
        let enemy = world::starting::new_object(&mut ids, 10, 3, 2, 1, [22, 5], &lib).unwrap();
        m.objects.push(enemy);
        let before = m.kingdoms[1].resources;
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("attack", vec![1], [22, 5], Some(4)),
        )
        .unwrap();
        // The footman is stronger than the peon, so it counterattacks (0x4ae640);
        // the enemy kingdom itself stays inert because the AI is not implemented.
        step(&mut sim, &mut m, &mut g, &lib, &c, 20);
        assert_eq!(m.objects.len(), 5);
        assert_eq!(m.objects[1].health, 0);
        assert_eq!(m.objects[1].owner, 0);
        assert_ne!(m.objects[1].flags & FLAG_DYING, 0);
        let died_at = (m.objects[1].x, m.objects[1].y);
        assert!(m.objects[4].health > 0);
        assert!(
            sim.sounds
                .iter()
                .any(|s| s.object == 4 && s.property == "sound_attack")
        );
        assert!(
            sim.sounds
                .iter()
                .any(|s| s.object == 1 && s.property == "sound_die")
        );
        step(&mut sim, &mut m, &mut g, &lib, &c, 200);
        assert_eq!(m.objects.len(), 5);
        assert_ne!(m.objects[1].flags & FLAG_INACTIVE, 0);
        // The corpse stays on the cell where the peon fell.
        assert_eq!((m.objects[1].x, m.objects[1].y), died_at);
        assert_eq!(m.kingdoms[1].resources, before);
        assert!(sim.gameplay.actors[4].jobs.is_empty());
        let saved: Simulation = serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
        assert!(
            saved.sounds.is_empty(),
            "saving must not replay past sounds"
        );
    }
    #[test]
    fn player_gameplay_runtime_validation_rejects_invalid_references() {
        let mut rt = Runtime::default();
        rt.actors.push(Actor {
            intent: Some(Intent::Attack(1)),
            ..Default::default()
        });
        assert!(!rt.validate(1, 64));
        rt.actors[0].intent = Some(Intent::Move([65535, 0]));
        assert!(!rt.validate(1, 64));
        rt.actors[0].intent = None;
        assert!(rt.validate(1, 64));
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_original_building_catalog_completes_for_all_civilizations() {
        for civ in 1..=3 {
            let (c, lib, _, _, _) = setup(civ);
            let keys = c
                .orders
                .section_names()
                .filter(|key| c.action(key).starts_with("build "))
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let mut covered = 0;
            for key in keys {
                let (_, _, mut m, mut g, mut sim) = setup(civ);
                // A site needs an explored corner (0x4203c5).
                sim.fog.explore_all();
                if !c.permitted(&key, &m.objects[1], &lib) {
                    continue;
                }
                m.rules[0].initial_researches =
                    c.technologies.section_names().map(str::to_owned).collect();
                if let Some(required) = c
                    .trees
                    .get(civ_section(civ))
                    .and_then(|tree| tree.get("required", &c.class_name(&key)))
                {
                    let required_class = CLASSES
                        .iter()
                        .position(|s| s.eq_ignore_ascii_case(required.trim()))
                        .unwrap();
                    new_unit(&mut sim, &mut m, &lib, required_class, 1, civ, [40, 40]).unwrap();
                }
                let class = CLASSES
                    .iter()
                    .position(|s| *s == c.class_name(&key))
                    .unwrap();
                let mut probe = m.objects[1].clone();
                probe.class = class;
                probe.x = 20;
                probe.y = 20;
                let [w, h] = footprint(&lib, &probe);
                if class == class::BRIDGE {
                    for y in 20..20 + h {
                        for x in 20..20 + w {
                            g.cells[y * 64 + x] = 0;
                        }
                    }
                }
                if class == class::HARBOR {
                    // 0x45b070: a shore row along the top, the sea below.
                    for y in 20..20 + h {
                        for x in 20..20 + w {
                            g.cells[y * 64 + x] = if y == 20 { 0x10 } else { 0 };
                        }
                    }
                }
                position(&mut m.objects[1], [19, 20]);
                issue(
                    &mut sim,
                    &mut m,
                    &g,
                    &lib,
                    &c,
                    &request(&key, vec![1], [20, 20], None),
                )
                .unwrap_or_else(|e| panic!("civ {civ}, {key}: {e}"));
                let created = m.objects.len() - 1;
                let time = sim.gameplay.actors[created].construction_total;
                advance(
                    &mut sim,
                    &mut m,
                    &mut g,
                    &lib,
                    &c,
                    // Completion occurs at the first full work cycle after TIME.
                    Duration::from_millis(time * TICK_MS + 5000),
                    10,
                    false,
                );
                assert_eq!(
                    m.objects[created].flags & FLAG_CONSTRUCTION,
                    0,
                    "civ {civ}, {key}"
                );
                assert_eq!(m.objects[created].class, class);
                assert_eq!(m.objects[created].civilization, civ);
                covered += 1;
            }
            assert!(covered >= 20, "civ {civ}: {covered} buildings");
        }
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_casting_approaches_targets_and_consumes_mana_once() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let priest = new_unit(&mut sim, &mut m, &lib, 18, 1, 1, [15, 15]).unwrap();
        m.rules[0].initial_researches.push("te_healing".into());
        m.objects[1].health = 1;
        position(&mut m.objects[1], [48, 15]);
        let before = sim.objects[priest].mana;
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("spell_heal", vec![priest], [48, 15], Some(1)),
        )
        .unwrap();
        assert_eq!(sim.objects[priest].mana, before);
        step(&mut sim, &mut m, &mut g, &lib, &c, 20);
        assert!(m.objects[1].health > 1);
        assert!(m.objects[priest].x > 15);
        // 0x472ff1: +6 HP per cast, repeated while the unit is wounded.
        let healed = m.objects[1].health - 1;
        assert!(healed % 6 == 0 || m.objects[1].health == m.objects[1].max_health);
        assert_eq!(
            sim.gameplay.actors[priest].intent.is_some(),
            m.objects[1].health < m.objects[1].max_health
        );
        assert!(sim.objects[priest].mana <= sim.objects[priest].max_mana);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_original_creation_catalog_is_executable_for_each_civilization() {
        for civ in 1..=3 {
            let (c, lib, _, _, _) = setup(civ);
            let orders: Vec<_> = c
                .orders
                .section_names()
                .filter(|key| c.action(key).starts_with("create "))
                // The crusade anchor is placed on the map, not produced
                // (player_gameplay_crusades_gather_and_end_like_the_native_cathedral).
                .filter(|key| !c.targeted(key))
                .map(str::to_owned)
                .collect();
            let mut covered = 0;
            for key in orders {
                let (_, _, mut m, mut g, mut sim) = setup(civ);
                m.rules[0].initial_researches =
                    c.technologies.section_names().map(str::to_owned).collect();
                for y in 40..64 {
                    for x in 0..64 {
                        g.cells[y * 64 + x] = 0;
                    }
                }
                for class in 3..=73 {
                    let j = new_unit(&mut sim, &mut m, &lib, class, 1, civ, [40, 39]).unwrap();
                    m.objects[j].health = m.objects[j].health.max(1);
                }
                // Test each producer independently; overlap is intentional to keep the arena compact.
                let Some(source) = m.objects.iter().position(|o| c.permitted(&key, o, &lib)) else {
                    continue;
                };
                let result = issue(
                    &mut sim,
                    &mut m,
                    &g,
                    &lib,
                    &c,
                    &Request::command(&key, vec![source]),
                );
                if result.is_err() {
                    panic!("civ {civ}, {key}: {result:?}");
                }
                let before = m.objects.len();
                step(&mut sim, &mut m, &mut g, &lib, &c, 120);
                assert_eq!(m.objects.len(), before + 1, "civ {civ}, {key}");
                let assets = GameAssets::open(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .parent()
                        .unwrap(),
                )
                .unwrap();
                m.simulation = Some(sim.clone());
                let art = lib
                    .build_indices(&assets, &m, g.side, Some(&BTreeSet::from([before])))
                    .unwrap();
                assert!(
                    art.missing.is_empty(),
                    "civ {civ}, {key}: {:?}",
                    art.missing
                );
                covered += 1;
            }
            assert!(covered >= 20, "civ {civ}: {covered}");
        }
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn a_flag_carried_to_its_bearers_castle_scores_and_lies_down_elsewhere() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let castle = movement::footprint_rect(&lib, &m.objects[0]);
        // Seven cells to the right of the castle, a peasant holds the flag:
        // the first to come into play, its timer every second from then.
        let flag = new_unit(&mut sim, &mut m, &lib, class::ITEM_FLAG, 0, 0, [40, 40]).unwrap();
        assert_eq!(sim.gameplay.flag, Some(flag));
        let far = [castle[2] as u16 + 7, castle[1] as u16];
        let peon = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 1, far).unwrap();
        garrison(&mut sim, &mut m, peon, flag);
        movement::carry_contents(&mut sim, &mut m, peon);
        assert_eq!([m.objects[flag].x, m.objects[flag].y], far);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 45);
        assert_eq!(sim.gameplay.score.get(&1).copied().unwrap_or(0), 0);
        assert_eq!(sim.gameplay.flag, Some(flag));
        // Six cells away: at the next second the kingdom scores 100 and the
        // flag goes, lying again on free land of the map.
        position(&mut m.objects[peon], [far[0] - 1, far[1]]);
        movement::carry_contents(&mut sim, &mut m, peon);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 14);
        assert_eq!(sim.gameplay.score.get(&1).copied().unwrap_or(0), 0);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        assert_eq!(sim.gameplay.score.get(&1).copied(), Some(100));
        assert!(m.objects[flag].flags & FLAG_INACTIVE != 0);
        assert!(sim.objects[peon].contents.is_empty());
        let new = sim.gameplay.flag.unwrap();
        assert_ne!(new, flag);
        let o = &m.objects[new];
        assert_eq!((o.class, o.owner), (class::ITEM_FLAG, 0));
        assert_eq!(o.custom_image, "flag_capture");
        assert!(sim.objects[new].holder.is_none());
        assert!(o.flags & (FLAG_INACTIVE | FLAG_HELD) == 0);
        // Where the kingdoms are fewest: away from the castle and the
        // peasants, which all stand within the map's first 25 columns.
        assert!(
            o.x > 25 || o.y > 25 || [o.x, o.y] == [0, 0],
            "{:?}",
            [o.x, o.y]
        );
        // A flag only lying by the castle scores nothing.
        position(&mut m.objects[new], [far[0] - 1, far[1] + 1]);
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 45);
        assert_eq!(sim.gameplay.score.get(&1).copied(), Some(100));
        assert_eq!(sim.gameplay.flag, Some(new));
        // The flag its bearer waves: `item_flag`, eight frames.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let waving = lib
            .effect_picture(&GameAssets::open(root).unwrap(), "item_flag")
            .unwrap();
        assert_eq!(waving.frames.len(), 8);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn the_maps_sounds_keep_the_times_of_their_object_timers() {
        let (_c, _lib, mut m, _g, mut sim) = setup(1);
        let source = |object, mode, delayed, period, min, max| world::SoundSource {
            object,
            sound: "bird".into(),
            mode,
            delayed,
            period,
            min,
            max,
        };
        // Once; every 5 s after a first wait; in a loop; every 2–4 s after
        // a first wait; and one without a sound, which never sounds.
        m.sounds = vec![
            source(1, 0, false, 0, 0, 0),
            source(2, 0, true, 5, 0, 0),
            source(3, 1, false, 0, 0, 0),
            source(0, 2, true, 0, 2, 4),
            world::SoundSource {
                sound: String::new(),
                ..source(0, 0, false, 0, 0, 0)
            },
        ];
        sim.speed = 10;
        let at = |sim: &mut Simulation, ms: u64| sim.elapsed = Duration::from_millis(ms);
        let quiet = |_: usize| false;
        // The map's loading sets the timers: at once (1 ms) or after the wait.
        at(&mut sim, 0);
        assert!(map_sounds(&mut sim, &m, quiet).start.is_empty());
        let due = sim.gameplay.map_sounds.clone();
        assert_eq!(
            [due[0], due[1], due[2], due[4]],
            [Some(1), Some(5000), Some(1), None]
        );
        let d = due[3].unwrap();
        assert!((2000..=4000).contains(&d), "{d}");
        at(&mut sim, 1);
        assert_eq!(
            map_sounds(&mut sim, &m, quiet).start,
            [(0, false), (2, true)]
        );
        assert_eq!(sim.gameplay.map_sounds[0], None);
        assert_eq!(sim.gameplay.map_sounds[2], Some(2001));
        // The loop still sounding: it looks again a second later.
        at(&mut sim, 2001);
        assert!(map_sounds(&mut sim, &m, |o| o == 3).start.is_empty());
        assert_eq!(sim.gameplay.map_sounds[2], Some(3001));
        at(&mut sim, d);
        let r = map_sounds(&mut sim, &m, |o| o == 3);
        assert!(r.start.contains(&(3, false)), "{r:?}");
        let next = sim.gameplay.map_sounds[3].unwrap();
        assert!((d + 2000..=d + 4000).contains(&next), "{d} {next}");
        // At double speed a real second is two of the game.
        let looped = sim.gameplay.map_sounds[2].unwrap();
        sim.speed = 20;
        at(&mut sim, 5000);
        let r = map_sounds(&mut sim, &m, quiet);
        assert!(r.start.contains(&(1, false)) && r.start.contains(&(2, true)));
        assert_eq!(sim.gameplay.map_sounds[1], Some(15_000));
        assert_eq!(sim.gameplay.map_sounds[2], Some(looped + 4000));
        // Its object gone, the loop falls silent and its timer stops.
        m.objects[3].flags |= FLAG_DEAD;
        at(&mut sim, looped + 4000);
        let r = map_sounds(&mut sim, &m, |o| o == 3);
        assert_eq!(r.silence, [3]);
        assert!(!r.start.iter().any(|&(i, _)| i == 2));
        assert_eq!(sim.gameplay.map_sounds[2], None);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_items_name_the_units_that_carry_them() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        // ITEM_PLUSPLUS: [enter] infantryunits, mountedunits; ITEM_FLAG: peon.
        let sword = new_unit(&mut sim, &mut m, &lib, class::ITEM_PLUSPLUS, 0, 0, [18, 5]).unwrap();
        let flag = new_unit(&mut sim, &mut m, &lib, class::ITEM_FLAG, 0, 0, [18, 7]).unwrap();
        let footman = new_unit(&mut sim, &mut m, &lib, 3, 1, 1, [16, 5]).unwrap();
        let knight = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [16, 7]).unwrap();
        let catapult = new_unit(&mut sim, &mut m, &lib, 19, 1, 1, [16, 9]).unwrap();
        let can = |i: usize, j: usize| accepts(&lib, &c, &m.objects[i], &m.objects[j]);
        assert!(can(1, sword) && can(footman, sword) && can(knight, sword));
        assert!(!can(catapult, sword));
        assert!(can(1, flag) && !can(footman, flag) && !can(knight, flag));
        assert!(
            issue(
                &mut sim,
                &mut m,
                &g,
                &lib,
                &c,
                &request("pick_item", vec![catapult], [18, 5], Some(sword)),
            )
            .is_err()
        );
        let _ = &mut g;
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_inventory_bonuses_potions_and_drops_are_live() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let item = new_unit(&mut sim, &mut m, &lib, class::ITEM_PLUSPLUS, 0, 0, [18, 5]).unwrap();
        m.objects[item].data.resize(110, 0);
        m.objects[item].data[22] = 5;
        let base = sim.objects[1].attack;
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("pick_item", vec![1], [18, 5], Some(item)),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 10);
        assert_eq!(sim.objects[item].holder, Some(1));
        assert_eq!(sim.effective_attributes(1, &m).unwrap().attack, base + 5);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("item_drop", vec![item]),
        )
        .unwrap();
        assert_eq!(sim.objects[item].holder, None);
        assert_eq!(sim.effective_attributes(1, &m).unwrap().attack, base);
        let p = [m.objects[1].x, m.objects[1].y];
        let potion = new_unit(&mut sim, &mut m, &lib, class::ITEM_HEALTH, 0, 0, p).unwrap();
        m.objects[potion].data.resize(110, 0);
        m.objects[potion].data[18..20].copy_from_slice(&20u16.to_le_bytes());
        sim.objects[potion].holder = Some(1);
        sim.objects[1].contents.push(potion);
        m.objects[1].health = 10;
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("item_use", vec![potion]),
        )
        .unwrap();
        assert_eq!(m.objects[1].health, 30);
        assert!(m.objects[potion].flags & FLAG_INACTIVE != 0);
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_items_follow_the_native_programs() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let give = |sim: &mut Simulation, m: &mut MissionWorld, holder: usize, j: usize| {
            sim.objects[j].holder = Some(holder);
            sim.objects[holder].contents.push(j);
            m.objects[j].flags |= FLAG_HELD;
            refresh_equipment(sim, m, j);
        };
        // Equipment changes the stored attack (0x463480) and leaves with the
        // item; a pickup happens only on the item's own cell (0x4e0b5e).
        let sword = new_unit(&mut sim, &mut m, &lib, class::ITEM_PLUSPLUS, 0, 0, [20, 5]).unwrap();
        m.objects[sword].data.resize(110, 0);
        m.objects[sword].data[0x16] = 7;
        m.objects[sword].data[0x17] = 1;
        let base = sim.objects[1].attack;
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("pick_item", vec![1], [20, 5], Some(sword)),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 10);
        assert_eq!(sim.objects[sword].holder, Some(1));
        assert_eq!([m.objects[1].x, m.objects[1].y], [20, 5]);
        assert_eq!(sim.objects[1].attack, base + 8);
        // A drop on a chosen cell: the holder walks there (program 16).
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("item_drop", vec![sword], [25, 9], None),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 10);
        // The holder stands on the chosen cell, which a land unit occupies
        // (bit 24, 0x5072a0): the item falls on a free cell around it.
        assert_eq!(sim.objects[sword].holder, None);
        assert_eq!([m.objects[1].x, m.objects[1].y], [25, 9]);
        let p = [m.objects[sword].x, m.objects[sword].y];
        assert!(p != [25, 9] && p[0].abs_diff(25).max(p[1].abs_diff(9)) == 1);
        assert_eq!(sim.objects[1].attack, base);
        // A potion for another unit: the holder walks to it (program 17).
        let potion = new_unit(&mut sim, &mut m, &lib, class::ITEM_HEALTH, 0, 0, [1, 1]).unwrap();
        m.objects[potion].data.resize(110, 0);
        m.objects[potion].data[0x12..0x14].copy_from_slice(&20u16.to_le_bytes());
        give(&mut sim, &mut m, 1, potion);
        position(&mut m.objects[3], [30, 20]);
        let (max1, max3) = (m.objects[1].max_health, m.objects[3].max_health);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("item_use", vec![potion], [30, 20], Some(3)),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 15);
        assert_eq!(m.objects[3].max_health, max3 + 20);
        assert_eq!(m.objects[1].max_health, max1);
        assert!(m.objects[potion].flags & FLAG_INACTIVE != 0);
        assert!(distance(&lib, &m.objects[1], &m.objects[3]) <= 1);
        // Death scatters the items, one per free cell, within 8 (0x5073f0).
        let bag: Vec<usize> = (0..3)
            .map(|_| {
                let j =
                    new_unit(&mut sim, &mut m, &lib, class::ITEM_PLUSPLUS, 0, 0, [1, 1]).unwrap();
                give(&mut sim, &mut m, 2, j);
                j
            })
            .collect();
        let at = [m.objects[2].x, m.objects[2].y];
        kill(&mut sim, &mut m, &g, &lib, 2);
        let cells: BTreeSet<[u16; 2]> = bag
            .iter()
            .map(|&j| [m.objects[j].x, m.objects[j].y])
            .collect();
        assert_eq!(cells.len(), 3);
        for p in &cells {
            assert!(*p != at && p[0].abs_diff(at[0]).max(p[1].abs_diff(at[1])) <= 8);
            assert!(bag.iter().all(|&j| sim.objects[j].holder.is_none()));
        }
        // A one-charge heal book: it recharges like any mana pool, then a
        // cast spends the charge and the book disappears (0x4640f0).
        let mage = caster(&mut sim, &mut m, &lib, 1, [40, 30]);
        let book = new_unit(&mut sim, &mut m, &lib, class::ITEM_MAGIC, 0, 0, [1, 1]).unwrap();
        m.objects[book].data.resize(110, 0);
        m.objects[book].data[96] = 2;
        m.objects[book].data[99] = 1;
        m.objects[book].data[102..104].copy_from_slice(&1u16.to_le_bytes());
        sim.objects[book].max_mana = 5;
        sim.objects[book].mana = 0;
        give(&mut sim, &mut m, mage, book);
        step(&mut sim, &mut m, &mut g, &lib, &c, 6);
        assert_eq!(sim.objects[book].mana, 5);
        let patient = new_unit(&mut sim, &mut m, &lib, 3, 1, 1, [42, 30]).unwrap();
        m.objects[patient].health = 10;
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("item_use", vec![book], [42, 30], Some(patient)),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 5);
        assert!(m.objects[patient].health > 10);
        assert!(m.objects[book].flags & FLAG_INACTIVE != 0);
        // 0x4641ae: a cast from a book unveils its holder, an invisibility
        // book too.
        let cloak = new_unit(&mut sim, &mut m, &lib, class::ITEM_MAGIC, 0, 0, [1, 1]).unwrap();
        m.objects[cloak].data.resize(110, 0);
        m.objects[cloak].data[96] = 9;
        sim.objects[cloak].max_mana = 5;
        sim.objects[cloak].mana = 5;
        give(&mut sim, &mut m, mage, cloak);
        sim.gameplay.actors[mage].effect_until = sim.ticks + 1000;
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("item_use", vec![cloak], [42, 30], Some(patient)),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 5);
        assert!(sim.gameplay.actors[patient].effect_until > sim.ticks);
        assert_eq!(sim.gameplay.actors[mage].effect_until, 0);
        // Ctrl+right click on a book: picked up, then its spell is learned.
        let lesson = new_unit(&mut sim, &mut m, &lib, class::ITEM_MAGIC, 0, 0, [41, 31]).unwrap();
        m.objects[lesson].data.resize(110, 0);
        m.objects[lesson].data[96] = 2;
        let mut pick = request("context", vec![mage], [41, 31], Some(lesson));
        pick.ctrl = true;
        assert!(!c.permitted("spell_heal", &m.objects[mage], &lib));
        issue(&mut sim, &mut m, &g, &lib, &c, &pick).unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 5);
        assert_eq!(sim.gameplay.actors[mage].spells, [2]);
        assert!(m.objects[lesson].flags & FLAG_INACTIVE != 0);
        // A scenario name does not make a hero; the promotion keeps it.
        m.objects[1].name = "Arturo".into();
        assert!(!hero(&m.objects[1]));
        m.rules[0].initial_researches.push("te_heroism".into());
        sim.objects[1].level = 11;
        add_experience(&mut sim, &mut m, &lib, &c, 1, 10000, false);
        assert_eq!(sim.objects[1].level, 12);
        assert!(hero(&m.objects[1]));
        assert_eq!(m.objects[1].name, "Arturo");
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_groups_use_distinct_destinations_and_cliffs_block_paths() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("move", vec![1, 2, 3], [25, 15], None),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 20);
        let cells: BTreeSet<_> = (1..=3).map(|i| [m.objects[i].x, m.objects[i].y]).collect();
        assert_eq!(cells.len(), 3);
        assert!((1..=3).all(|i| sim.gameplay.actors[i].intent.is_none()));
        for y in 0..64 {
            g.cells[y * 64 + 30] |= 0x400;
        }
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("move", vec![1], [40, 15], None),
        )
        .unwrap();
        // The unit gets as near as it can to the unreachable cell and stops there.
        step(&mut sim, &mut m, &mut g, &lib, &c, 60);
        assert_eq!(m.objects[1].x, 29);
        assert!(sim.gameplay.actors[1].intent.is_none());
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_bridges_and_gate_state_allow_crossing() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let bridge = new_unit(&mut sim, &mut m, &lib, class::BRIDGE, 1, 1, [30, 5]).unwrap();
        let [w, _] = lib.cell_footprint(&m.objects[bridge]);
        assert!(30 + w + 2 < 64);
        for y in 0..64 {
            for x in 30..30 + w {
                g.cells[y * 64 + x] = 0;
            }
        }
        let goal = [(30 + w + 2) as u16, 5];
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("move", vec![1], goal, None),
        )
        .unwrap();
        step(&mut sim, &mut m, &mut g, &lib, &c, 40);
        assert_eq!([m.objects[1].x, m.objects[1].y], goal);
        let gate = new_unit(&mut sim, &mut m, &lib, class::GATE_WOOD, 1, 1, [20, 20]).unwrap();
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("gate_subai_off", vec![gate]),
        )
        .unwrap();
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("gateopen", vec![gate]),
        )
        .unwrap();
        assert_eq!(m.objects[gate].byte(57), 1);
        let mask = occupied(&g, &m, &lib, None, false);
        assert!(mask[20 * 64 + 20]);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("gateclose", vec![gate]),
        )
        .unwrap();
        assert_eq!(m.objects[gate].byte(57), 0);
        let mask = occupied(&g, &m, &lib, None, false);
        assert!(!mask[20 * 64 + 20]);
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command("gate_subai_on", vec![gate]),
        )
        .unwrap();
        assert_eq!(m.objects[gate].byte(65), 1);
        assert_eq!(lib.property(&m.objects[gate], "menu"), Some("gate_wood_11"));
    }
    #[test]
    #[ignore = "Requires the installed original game archives"]
    fn player_gameplay_fractional_speed_is_independent_of_frame_partition() {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        let mut m2 = m.clone();
        let mut g2 = g.clone();
        let mut sim2 = sim.clone();
        advance(
            &mut sim,
            &mut m,
            &mut g,
            &lib,
            &c,
            Duration::from_secs(1),
            37,
            false,
        );
        for _ in 0..1000 {
            advance(
                &mut sim2,
                &mut m2,
                &mut g2,
                &lib,
                &c,
                Duration::from_millis(1),
                37,
                false,
            );
        }
        assert_eq!(sim.elapsed, sim2.elapsed);
        assert_eq!(sim.ticks, sim2.ticks);
        assert_eq!(
            sim.gameplay.scaled_remainder,
            sim2.gameplay.scaled_remainder
        );
    }
}
