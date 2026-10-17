//! The computer players of `Tzar.exe`: one per
//! kingdom, kept by the manager 0x5abb14. For now their presence, mode and
//! compiled script, the manager's places, the AI command and the view of
//! the native programs they read.
use super::*;

pub mod breach;
pub mod build;
pub mod defence;
pub mod goals;
pub mod harvest;
pub mod idle;
pub mod navy;
pub mod places;
pub mod reach;
pub mod records;
pub mod rules;
pub mod script;
pub mod spells;
pub mod think;
pub mod transport;
pub mod tree;
pub mod units;
pub mod war;

/// IA+0x1a20 on easy: no attack on another kingdom's home for the first
/// 30 minutes (0x407140).
const EASY_PEACE_MS: u64 = 1_800_000;

/// The manager's computer players, by kingdom.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct State {
    pub players: BTreeMap<u8, Player>,
    /// A save made before the computer players lacks them: `upgrade` gives
    /// them.
    #[serde(skip)]
    pub missing: bool,
    /// The players made at the start still compile together: one that
    /// fails stops the others after it (0x402760).
    pub starting: bool,
    /// The manager's places and forces, made with the first players.
    pub manager: Option<places::Manager>,
    /// What the players' hooks saw of each object at the step before.
    pub census: Vec<Option<think::Seen>>,
    /// The sites still to be laid, by number, and the next number.
    pub virtuals: BTreeMap<u32, build::Virtual>,
    pub next_virtual: u32,
    /// The order the players' messages were posted in (the queue 0xc97c54
    /// runs equal times in that order).
    pub sequence: u64,
    /// The militias called this step (0x413a99), by kingdom and threat;
    /// done before the step ends.
    #[serde(skip)]
    pub alarms: Vec<(u8, usize)>,
    /// The attacks on a wall the breaches posted this step (0x4171e0), by
    /// unit and wall; taken when the object's update ends.
    #[serde(skip)]
    pub breaches: Vec<(usize, usize)>,
    /// 0x5af414: the water a harbor is wanted for, set before its goal is
    /// carried out and taken by the next search of a harbor's site, of any
    /// player. The original does not save it; ReTzar does.
    pub harbor_water: u8,
    /// The AI command `off` took the manager away (0x581070 cleared); `on`
    /// may make it again.
    pub off: bool,
}
impl State {
    pub(crate) fn next_seq(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }
    pub(crate) fn next_virtual(&mut self) -> u32 {
        self.next_virtual = self.next_virtual.wrapping_add(1);
        self.next_virtual
    }
}

/// IA+0x9fa: the resources by what the player needs most (`Harvest`),
/// gold, stone, food and wood at first (0x407140).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Order(pub [u8; 4]);
impl Default for Order {
    fn default() -> Self {
        Self([0, 1, 2, 3])
    }
}

/// A kingdom's computer player (the 0x1a3c-byte object of 0x406ac0).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Player {
    /// IA+0x910: bit 0 economy, bit 1 war; 0x80 the attack pause frozen by
    /// `@setnoattacktime`, 0x4000 the `SAY` lines shown.
    pub mode: u16,
    /// The behaviour script (K+0x64): a file of `data\ai\`, empty for the
    /// default one.
    pub script: String,
    /// IA+0x1a20: no group goes to another kingdom's home before this game
    /// time, in ms.
    pub peace_until: u64,
    /// The compiled script, which saves keep (0x418930); none until
    /// `prepare` reads it.
    pub program: Option<script::Program>,
    /// IA+0x1a24: the player's ships by the water they were counted on, in
    /// the order they were.
    pub ships: BTreeMap<u8, Vec<usize>>,
    /// IA+0x1a34: the ships wanted on each water.
    pub wanted_ships: BTreeMap<u8, u16>,
    /// IA+0x1a38: the harbors wanted on each water, and the fish patch that
    /// asked for one (IA+0x1a30, none at first).
    pub wanted_harbors: BTreeMap<u8, u16>,
    pub harbor_targets: BTreeMap<u8, [u16; 2]>,
    /// IA+0xa0a: its view of every place, highest priority first.
    pub records: Vec<records::Record>,
    /// IA+0x900: the cell its home is at, once known (+0x904).
    pub home: Option<[i32; 2]>,
    /// IA+0x908: no building yet, the home moves and the places are
    /// evaluated at every look.
    pub homeless: bool,
    /// IA+0x90d and IA+0x90e: its kingdom and civilization.
    pub kingdom: u8,
    pub civilization: u8,
    /// IA+0x90c: the land region of its home.
    pub island: u8,
    /// IA+0x912: the population limit (K+0x62) its rules last read.
    pub cap: u16,
    /// When it looks next (message 0x4201), in game ms, and the message's
    /// place in the queue; none before the manager posts its first look.
    pub due: Option<u64>,
    pub seq: u64,
    /// IA+0x9d2: its search for a site, while it goes on.
    pub search: Option<build::Search>,
    /// IA+0x9fa.
    pub order: Order,
    /// IA+0x9fe: the gatherers it wants of each resource (`Harvest`).
    pub wanted: [u16; 4],
    /// IA+0xa06: the resources it may not gather, gold or stone with no
    /// mine left.
    pub blocked: [bool; 4],
    /// IA+0x9a0: the phase of its next look.
    pub phase: u8,
    /// IA+0x914: the kingdom's resources less those committed (IA+0x928),
    /// as the last Build phase read them, less what it paid since.
    pub available: [i32; 4],
    pub committed: [i32; 4],
    /// IA+0x93c: what each part of it books (`rules::bucket`).
    pub buckets: [[i32; 4]; 5],
    /// IA+0x988: `NEEDS_LOCKED` holds the parts of ships and defences; the
    /// fifth word of the script's part, it goes with it at each Build phase.
    pub locked: bool,
    /// The draws of its `RANDOM` conditions, by condition (+6).
    pub rolls: BTreeMap<u32, bool>,
    /// The rules that read each value.
    #[serde(skip)]
    pub watch: rules::Watch,
    pub lists: think::Lists,
    /// The search for a wood.
    pub forest: harvest::Forest,
    /// The kingdom's researches done that its rules saw.
    pub researched: usize,
    /// 0x5abb10: its Build phase is running.
    #[serde(skip)]
    pub building: bool,
}
impl Player {
    fn new(k: u8, civilization: u8, mode: u16, script: &str, difficulty: usize) -> Self {
        Self {
            mode,
            script: script.into(),
            peace_until: if difficulty == 0 { EASY_PEACE_MS } else { 0 },
            kingdom: k,
            civilization,
            ..Default::default()
        }
    }
}

/// The script a computer player reads (0x402760): the kingdom's, else
/// `dm.ais` in King of the Hill with unlimited resources (0x5cf808,
/// 0x5877b4), else `random.ais`.
pub fn script_name<'a>(script: &'a str, rules: &world::GameRules) -> &'a str {
    if !script.is_empty() {
        script
    } else if rules.game_type == 1 && rules.resource_level == 2 {
        "data\\ai\\dm.ais"
    } else {
        "data\\ai\\random.ais"
    }
}

/// 0x402760 at the start and 0x402440 for a player made later: the players
/// still without a program compile their script. One that does not compile
/// leaves the kingdom without a computer player, its error shown; at the
/// start the kingdoms after it get none either.
pub fn prepare(sim: &mut Simulation, m: &MissionWorld, library: &script::Library) {
    let starting = std::mem::take(&mut sim.gameplay.ai.starting);
    let waiting: Vec<u8> = sim
        .gameplay
        .ai
        .players
        .iter()
        .filter(|(_, p)| p.program.is_none())
        .map(|(&k, _)| k)
        .collect();
    for (n, &k) in waiting.iter().enumerate() {
        let player = &sim.gameplay.ai.players[&k];
        match library.compile(script_name(&player.script, &m.game_rules)) {
            Ok(program) => {
                if let Some(p) = sim.gameplay.ai.players.get_mut(&k) {
                    p.program = Some(program);
                }
            }
            Err(e) => {
                sim.gameplay.ai.players.remove(&k);
                if starting {
                    for later in &waiting[n + 1..] {
                        sim.gameplay.ai.players.remove(later);
                    }
                }
                let name = m
                    .kingdoms
                    .get((k as usize).wrapping_sub(1))
                    .map_or(String::new(), |x| x.name.clone());
                let outbox = &mut sim.gameplay.script.outbox;
                if outbox.len() < 1024 {
                    outbox.push(Show::Line(format!("IA di {name}: {e}")));
                }
                if starting {
                    return;
                }
            }
        }
    }
}

impl State {
    /// 0x402760 at the start of a game: kingdoms 1–8 that take part and
    /// whose mode (K+0xa) has the economy or war bit get a computer player.
    pub fn start(m: &MissionWorld) -> Self {
        let players = m
            .kingdoms
            .iter()
            .enumerate()
            .take(8)
            .filter(|(_, k)| k.active && k.ai_mode & 3 != 0)
            .map(|(i, k)| {
                (
                    i as u8 + 1,
                    Player::new(
                        i as u8 + 1,
                        k.civilization,
                        (k.ai_mode & 3) as u16,
                        &k.ai_script,
                        m.game_rules.difficulty,
                    ),
                )
            })
            .collect();
        Self {
            players,
            starting: true,
            ..Default::default()
        }
    }
    /// What a save without the computer players reads.
    pub fn missing() -> Self {
        Self {
            missing: true,
            ..Default::default()
        }
    }
}

/// A save made before the computer players existed: a full one for each
/// kingdom marked computer, as a game set up on a screen gives.
impl State {
    pub fn upgrade(&mut self, m: &MissionWorld) {
        if !self.missing {
            return;
        }
        for (i, k) in m.kingdoms.iter().enumerate().take(8) {
            if k.active && k.computer {
                let player = Player::new(
                    i as u8 + 1,
                    k.civilization,
                    3,
                    &k.ai_script,
                    m.game_rules.difficulty,
                );
                self.players.insert(i as u8 + 1, player);
            }
        }
        self.missing = false;
    }
}

/// The kingdom has a computer player (`0x5abb14` and its slot): what the
/// engine's own checks ask.
pub fn controls(sim: &Simulation, owner: u8) -> bool {
    sim.gameplay.ai.players.contains_key(&owner)
}

/// 0x48f8bb, at kingdom `a`'s diplomacy message toward `b` (0x3a, sent by
/// the diplomacy window, 0x447072): a computer player's kingdom `b`, `a`
/// being its ally (0x40), shares back the resources (2) and the units (4)
/// that `a` shares with it and it does not with `a`, by its own message
/// (0x490b90) merged as any other (0x467ef0).
pub(crate) fn reciprocate(sim: &Simulation, m: &mut MissionWorld, a: u8, b: u8) {
    if !controls(sim, b) || relation(m, a, b) & 0x40 == 0 {
        return;
    }
    let (ab, ba) = (relation(m, a, b), relation(m, b, a));
    let give = ab & !ba & 6;
    if give != 0 {
        scripting::merge_relation(m, b, a, ba | give);
    }
}

/// The modes of the AI command (0x404460) that `@setai` gives, 0–3.
pub const COMMANDS: [&str; 4] = ["none", "eco", "war", "full"];

/// 0x404460, the AI command (in lower case; the native compares without
/// case). `off` removes the manager with every computer player (`on`
/// remakes them as at the start, 0x402760, only after an `off`); `talk on`
/// and `talk off` show or hide the kingdom's `SAY` lines (mode bit
/// 0x4000); `none`, `eco`, `war` and `full` remove the kingdom's computer
/// player, make it (0x402440) or give it the new mode bits, and return the
/// line to show first, «%s ha disattivato/attivato l'assistente…»
/// (STRINGS3 `[msg]`, the key and the kingdom's name).
pub fn command(sim: &mut Simulation, m: &MissionWorld, kingdom: u8, command: &str) -> Option<Show> {
    let name = kingdom
        .checked_sub(1)
        .and_then(|k| m.kingdoms.get(k as usize))
        .map_or(String::new(), |k| k.name.clone());
    let talk = |sim: &mut Simulation, on: bool| {
        if let Some(p) = sim.gameplay.ai.players.get_mut(&kingdom) {
            p.mode = if on { p.mode | TALK } else { p.mode & !TALK };
        }
    };
    let mode: u16 = match command {
        "off" => {
            sim.gameplay.ai = State {
                off: true,
                ..Default::default()
            };
            return None;
        }
        "on" => {
            if sim.gameplay.ai.off && sim.gameplay.ai.players.is_empty() {
                sim.gameplay.ai = State::start(m);
            }
            return None;
        }
        "talk on" => {
            talk(sim, true);
            return None;
        }
        "talk off" => {
            talk(sim, false);
            return None;
        }
        "none" => {
            sim.gameplay.ai.players.remove(&kingdom);
            return Some(Show::Message("aioff3", name));
        }
        "eco" => 1,
        "war" => 2,
        "full" => 3,
        _ => return None,
    };
    sim.gameplay.ai.off = false;
    let key = match mode {
        3 => "aifull3",
        2 => "aiwar3",
        _ => "aieco3",
    };
    match sim.gameplay.ai.players.get_mut(&kingdom) {
        Some(p) => p.mode = p.mode & !3 | mode,
        None => {
            let record = kingdom
                .checked_sub(1)
                .and_then(|k| m.kingdoms.get(k as usize));
            let script = record.map_or("", |k| k.ai_script.as_str());
            let civilization = record.map_or(0, |k| k.civilization);
            let player = Player::new(kingdom, civilization, mode, script, m.game_rules.difficulty);
            sim.gameplay.ai.players.insert(kingdom, player);
        }
    }
    Some(Show::Message(key, name))
}

/// The mode bit of the `SAY` lines shown (`talk on`, 0x404677).
pub const TALK: u16 = 0x4000;

/// 0x404880: a line of the chat beginning with «AI » (in any case) is the
/// AI command, not a message (0x42572a).
pub fn chat_command(line: &str) -> bool {
    line.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("AI "))
}
/// 0x404880 for a chat line of the local kingdom: the AI command of the
/// kingdom whose digit 1–8 follows «AI », else of the local one, its
/// line sent to the screen.
pub fn chat(sim: &mut Simulation, m: &MissionWorld, line: &str) {
    if !chat_command(line) {
        return;
    }
    let rest = line[3..].trim_start_matches(' ');
    let (kingdom, rest) = match rest.as_bytes().first() {
        Some(&d @ b'1'..=b'8') => (d - b'0', &rest[1..]),
        _ => (m.player(), rest),
    };
    let text = rest.trim_start_matches(' ').to_ascii_lowercase();
    if let Some(show) = command(sim, m, kingdom, &text) {
        let outbox = &mut sim.gameplay.script.outbox;
        if outbox.len() < 1024 {
            outbox.push(show);
        }
    }
}

/// `@setnoattacktime(mask, ms)` (0x4b81d0): the computer players of the
/// kingdoms in the mask (bit 0 kingdom 1) freeze their attack pause and
/// keep away from other homes until `ms`.
pub fn no_attack_time(sim: &mut Simulation, mask: u32, ms: u32) {
    for k in 1..=8u8 {
        if mask & (1 << (k - 1)) != 0
            && let Some(p) = sim.gameplay.ai.players.get_mut(&k)
        {
            p.mode |= 0x80;
            p.peace_until = ms as u64;
        }
    }
}

/// What a computer player sees of a unit's program: the native program
/// number and its priority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Program {
    pub kind: u8,
    pub priority: u16,
}
/// Native program numbers the computer players test.
pub mod program {
    pub const MOVE: u8 = 0x01;
    pub const CHOP: u8 = 0x02;
    pub const ATTACK: u8 = 0x03;
    pub const PATROL: u8 = 0x05;
    pub const PICK_ITEM: u8 = 0x06;
    /// Field work (Dig, op 0x15, with Find farm and Find field).
    pub const DIG: u8 = 0x07;
    /// Mine work (Mine, op 0x27, with Has mine res? and Find storage).
    pub const MINE: u8 = 0x08;
    pub const CAPTURE: u8 = 0x09;
    pub const IDLE: u8 = 0x0a;
    pub const BUILD: u8 = 0x0d;
    /// A computer player's builder on its way to a site still to be laid
    /// (0x42e570).
    pub const AI_FAR_BUILD: u8 = 0x19;
    /// A computer player's group sent to a place (0x42dde0).
    pub const APPROACH: u8 = 0x28;
    /// A computer player's ship and its passengers (0x430200, 0x4306a0).
    pub const TRANSPORT: u8 = 0x2b;
    pub const USE_TRANSPORT: u8 = 0x2c;
    pub const DROP_ITEM: u8 = 0x10;
    pub const USE_ITEM: u8 = 0x11;
    pub const CAST: u8 = 0x12;
    pub const ENTER_SHIP: u8 = 0x15;
    pub const UNLOAD_UNITS: u8 = 0x16;
    pub const TRADE: u8 = 0x17;
    pub const REPAIR: u8 = 0x18;
    pub const FISHING: u8 = 0x1a;
    pub const EXPLORE: u8 = 0x1c;
    pub const GATHER_ITEMS: u8 = 0x1e;
    pub const TEND_COWS: u8 = 0x20;
    pub const UNLOAD_RESOURCE: u8 = 0x21;
    pub const MURDER: u8 = 0x2d;
    pub const IMPERSONATE: u8 = 0x2f;
    pub const MOVE_ADVANCE: u8 = 0x31;
    pub const BRIBE: u8 = 0x32;
    pub const RETURN_GOODS: u8 = 0x35;
    pub const MOVE_SNEAK: u8 = 0x39;
    pub const ATTACK_GROUND: u8 = 0x3b;
    pub const RESCUE: u8 = 0x3d;
    pub const RUNAWAY: u8 = 0x3f;
    pub const PICK_ITEM_USE: u8 = 0x41;
}

/// The unit's program as 0x401da0 gives it to a computer player: the order
/// in hand, else the default program (Idle). The priority follows
/// ReTzar's model of the native ones: a player's or a computer player's
/// order 4000, an engagement of the unit's own (`automatic`) 3000, a
/// flight 1000, the default program 0.
pub fn program(sim: &Simulation, m: &MissionWorld, i: usize) -> Program {
    use program::*;
    let a = &sim.gameplay.actors[i];
    let Some(intent) = &a.intent else {
        return Program {
            kind: IDLE,
            priority: 0,
        };
    };
    let kind = match intent {
        Intent::Move(_) => match a.move_mode {
            1 => MOVE_ADVANCE,
            2 => MOVE_SNEAK,
            _ => MOVE,
        },
        Intent::Explore(_) => EXPLORE,
        // A Shift list is an Attack program too (0x4e3979).
        Intent::Attack(_) | Intent::Reach(_) => ATTACK,
        Intent::AttackGround(_) => ATTACK_GROUND,
        Intent::Cast { .. } => CAST,
        Intent::Harvest(j) => match m.objects.get(*j).map(|o| o.class) {
            Some(class::FIELD) => DIG,
            Some(class::RES_GOLD | class::RES_STONE) => MINE,
            Some(class::COW) => TEND_COWS,
            Some(class::FISHPATCH) => FISHING,
            _ => GATHER_ITEMS,
        },
        Intent::Chop(_) => CHOP,
        Intent::Build(_) => BUILD,
        Intent::FarBuild(_) => AI_FAR_BUILD,
        Intent::Approach(_) => APPROACH,
        Intent::Transport(_) => TRANSPORT,
        Intent::UseTransport(_) => USE_TRANSPORT,
        Intent::Repair(_) => REPAIR,
        Intent::Capture(_) => CAPTURE,
        Intent::Bribe(_) => BRIBE,
        Intent::Pick(_) => PICK_ITEM,
        Intent::PickUse(_) => PICK_ITEM_USE,
        Intent::UseItem { .. } => USE_ITEM,
        Intent::DropItem { .. } => DROP_ITEM,
        // The guard is a field of the default program (0x4e3cf7).
        Intent::Guard(_) => IDLE,
        Intent::Patrol(..) | Intent::Circuit(..) => PATROL,
        Intent::Board(_) => ENTER_SHIP,
        Intent::Trade(_) => TRADE,
        Intent::Deliver(_) => RETURN_GOODS,
        Intent::Sell(_) => UNLOAD_RESOURCE,
        Intent::Unload(_) => UNLOAD_UNITS,
        Intent::Flee(..) => RUNAWAY,
        Intent::Murder(_) => MURDER,
        Intent::Impersonate(_) => IMPERSONATE,
        Intent::Rescue(_) => RESCUE,
    };
    let priority = match intent {
        Intent::Flee(..) => 1000,
        _ if a.automatic => 3000,
        _ => 4000,
    };
    Program { kind, priority }
}

/// The program Create (0x42e820) or the research (0x42fc80) a computer
/// player gives an idle building: one step, already paid.
pub(super) fn produce(
    sim: &mut Simulation,
    m: &MissionWorld,
    c: &Catalog,
    i: usize,
    kind: JobKind,
    paid: [i32; 4],
) {
    let name = match &kind {
        JobKind::Train(k) => CLASSES[*k].to_owned(),
        JobKind::Research(n) | JobKind::Upgrade(n, _) => n.clone(),
        _ => return,
    };
    let o = &m.objects[i];
    let total = c.stats(o, &name, researches(m, o.owner)).time_ms.max(1);
    let a = &mut sim.gameplay.actors[i];
    a.jobs.push_back(Job {
        kind,
        remaining: total,
        total,
        paid: paid.map(|v| v.max(0) as u32),
        due_ms: None,
        waiting: false,
    });
    start_step(sim, i);
    sim.gameplay.actors[i].production_blocked = None;
}
