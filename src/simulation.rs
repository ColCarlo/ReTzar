//! Object attributes, deterministic clock, visibility and player simulation state.
use crate::{
    Result,
    assets::{GameAssets, legacy_text},
    bitmap::Bitmap,
    world::{FLAG_INACTIVE, MapObject, MissionWorld, ObjectLibrary, civ_section, class},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

#[derive(Clone, Copy, Debug, Default)]
pub struct ClassAttributes {
    pub health: u16,
    pub attack: u16,
    pub defence: u16,
    pub sight: u16,
    pub range: u16,
    pub resistance: u16,
}
/// The EMIT column of a building (classdef +0x2d2..+0x2d8, 0x4966e0): the
/// sight of its row, and what it adds to the resistance of allied
/// buildings and of the others around it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Emit {
    pub sight: u16,
    pub own: i16,
    pub other: i16,
}
#[derive(Default)]
pub struct AttributeCatalog(
    HashMap<(String, String), ClassAttributes>,
    /// By class, with the rank of the table that wrote it last.
    HashMap<String, (u8, Emit)>,
);
/// C `atoi`: optional sign, then digits.
fn atoi(s: &str) -> i32 {
    let s = s.trim();
    let (sign, digits) = match s.as_bytes().first() {
        Some(b'-') => (-1, &s[1..]),
        Some(b'+') => (1, &s[1..]),
        _ => (1, s),
    };
    let n: i64 = digits
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0, |n, d| (n * 10 + (d - b'0') as i64).min(i32::MAX as i64));
    (sign * n) as i32
}
impl AttributeCatalog {
    pub fn load(a: &GameAssets) -> Result<Self> {
        let mut out = Self::default();
        // Native 0x49751a–0x4975a8 loads BUILDINGS before UNITS.
        // Towers occur in both: unit sight/health override the building row,
        // while resistance remains a building property.
        for civ in ["comm", "euro", "asia", "arab"] {
            for kind in ["BUILDINGS", "UNITS"] {
                let path = format!("DATA/{civ} {kind}.TXT");
                if !a.contains(&path) {
                    continue;
                }
                let mut columns = Vec::new();
                for line in legacy_text(&a.get(&path)?).lines() {
                    let fields: Vec<_> = line.split('\t').map(str::trim).collect();
                    if matches!(fields.first(), Some(&("UNIT" | "BUILDING"))) {
                        columns = fields.iter().map(|s| s.to_ascii_uppercase()).collect();
                        continue;
                    }
                    let at = |key: &str| {
                        columns
                            .iter()
                            .position(|s| s == key)
                            .and_then(|i| fields.get(i).copied())
                    };
                    let Some(name) = at("CLASS").filter(|s| !s.is_empty()) else {
                        continue;
                    };
                    let value = |key| at(key).and_then(|s| s.parse::<u16>().ok()).unwrap_or(0);
                    // EMIT belongs to the class, not to a civilization: the
                    // tables load arab, asia, euro, then comm (0x49751a), and
                    // the last one wins for every kingdom.
                    if let Some(raw) = columns
                        .iter()
                        .position(|s| s.starts_with("EMIT"))
                        .and_then(|i| fields.get(i).copied())
                    {
                        let rank = match civ {
                            "arab" => 0,
                            "asia" => 1,
                            "euro" => 2,
                            _ => 3,
                        };
                        let raw = raw.trim_matches('"');
                        let (own, other) = match raw.split_once(',') {
                            Some((a, b)) => (atoi(a), atoi(b)),
                            None => (atoi(raw), -atoi(raw)),
                        };
                        let emit = Emit {
                            sight: value("SIGHT"),
                            own: own as i16,
                            other: other as i16,
                        };
                        let slot = out
                            .1
                            .entry(name.to_ascii_lowercase())
                            .or_insert((rank, emit));
                        if rank >= slot.0 {
                            *slot = (rank, emit);
                        }
                    }
                    let data = out
                        .0
                        .entry((civ.into(), name.to_ascii_lowercase()))
                        .or_default();
                    // Update only columns present in this table. Zero and
                    // empty numeric fields are values too (native atoi setter
                    // 0x496610), not requests to retain a previous value.
                    for (dst, key) in [
                        (&mut data.health, "HEALTH"),
                        (&mut data.attack, "ATTACK"),
                        (&mut data.defence, "DEFENCE"),
                        (&mut data.sight, "SIGHT"),
                        (&mut data.range, "RANGE"),
                        (&mut data.resistance, "RESISTANCE"),
                    ] {
                        if columns.iter().any(|column| column == key) {
                            *dst = value(key);
                        }
                    }
                }
            }
        }
        Ok(out)
    }
    /// The class has a row in the tables.
    pub fn lists(&self, o: &MapObject) -> bool {
        self.0
            .contains_key(&(civ_section(o.civilization).into(), o.class_name().into()))
            || self.0.contains_key(&("comm".into(), o.class_name().into()))
    }
    /// The class's EMIT, when it changes anything.
    pub fn emit(&self, o: &MapObject) -> Option<Emit> {
        self.1
            .get(o.class_name())
            .map(|&(_, e)| e)
            .filter(|e| e.own != 0 || e.other != 0)
    }
    pub fn get(&self, o: &MapObject) -> ClassAttributes {
        self.0
            .get(&(civ_section(o.civilization).into(), o.class_name().into()))
            .or_else(|| self.0.get(&("comm".into(), o.class_name().into())))
            .copied()
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderState {
    #[default]
    Idle,
    Moving,
    Working,
    Attacking,
    Building,
    Guarding,
    Casting,
    /// `image_die` of a dead object, before its removal.
    Dying,
    /// A summoned or raised creature coming out (Animate, op 0x74).
    Appearing,
}
/// Constructor weapon byte +0x6b, including the tower initializer 0x4dc100.
/// This is an attack category, not a test of the projectile range.
pub fn weapon_category(class: usize) -> u8 {
    match class {
        4 | 6 | 10 | 21 | 25 | 39..=41 => 1,
        19 | 20 | 23 | 24 | 42 => 2,
        // The messiah constructor 0x4eef50 leaves +0x6b at 0.
        17 | 18 | 32 => 3,
        43 => 5,
        _ => 0,
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ObjectState {
    pub attack: u32,
    pub defence: u32,
    pub level: u16,
    /// Experience inside the current level (+0x68), emptied at each level.
    #[serde(default)]
    pub xp: u16,
    pub mana: u16,
    pub max_mana: u16,
    pub sight: u16,
    pub resistance: u16,
    pub ranged: bool,
    pub order: OrderState,
    pub contents: Vec<usize>,
    pub holder: Option<usize>,
    /// An equipment item: the holder its bonus was added to (0x463480).
    #[serde(default)]
    pub equipped: Option<usize>,
    /// Cells covered by buildings and mines, for the fog (0x501dc0); filled
    /// again after a restore.
    #[serde(skip)]
    pub size: Option<[u8; 2]>,
    /// Room of its holder, fixed when the object was made (0x4fffc4) with
    /// the kingdom's Landing Tactics; None for older saves.
    #[serde(default)]
    pub capacity: Option<u16>,
    /// Religious resistance bonuses (Buddhism, Fanaticism) baked into the
    /// object's own target (+0x11a); None for older saves.
    #[serde(default)]
    pub faith: Option<u8>,
    /// While held, +0x12/+0x14: the cell it came in from (0x5044be), wiped
    /// when its holder changes cell (0x500acc); the way out starts there
    /// (0x504550).
    #[serde(default)]
    pub entry: Option<[u16; 2]>,
    /// A held item marked as the active one of its holder (+0xcd, 0x463140):
    /// Backspace uses it (0x4f628c).
    #[serde(default)]
    pub active: bool,
}
impl ObjectState {
    /// Native kingdom bonuses (0x4de7d0 / 0x4de880) and initial research
    /// handlers (0x4d3300). Apply once on import, never on snapshot restore.
    /// The researches of the kingdom on a new object, from the class record
    /// as they changed it.
    pub fn apply_researches(&mut self, o: &MapObject, researches: &[String]) {
        self.researched(o, researches, false);
    }
    /// The research handlers (0x4d33d0) run on an object that already
    /// exists: Kung Fu reaches every peasant, not only Asian ones.
    pub fn apply_dispatch(&mut self, o: &MapObject, researches: &[String]) {
        self.researched(o, researches, true);
    }
    fn researched(&mut self, o: &MapObject, researches: &[String], existing: bool) {
        // A research completed twice runs its handler twice (0x4d33d0).
        let n = |name: &str| completions(researches, name);
        if o.owner == 0 {
            return;
        }
        let [attack, defence] = Self::live_bonus(o, researches);
        self.attack += attack;
        self.defence += defence;
        if crate::selection::unit(o) {
            if matches!(o.class, 10 | 35) {
                self.attack += 2 * n("te_ridinghorses");
                self.defence += n("te_ridinghorses");
            }
            // 0x4d3cbe: existing peasants of any civilization, new Asian ones
            // (only the Asian record changes).
            if o.class == class::PEON && (existing || o.civilization == 3) {
                self.attack += n("te_kungfu");
            }
        }
        if o.class == 42 {
            self.attack += 10 * n("te_gunpowder");
        }
        // 0x4d4064: these religious technologies increase resistance;
        // architecture changes construction TIME, not resistance.
        if o.building() {
            for _ in 0..n("te_budhism") + n("te_fanaticism") {
                self.resistance = self.resistance.saturating_add(self.resistance / 2);
            }
        }
    }
    /// The bonuses a kingdom's getters add while the object is its own
    /// (0x4de7d0 weapon category, 0x4de880 armour, 0x4ea600 priest): the
    /// only ones that follow a change of owner.
    pub fn live_bonus(o: &MapObject, researches: &[String]) -> [u32; 2] {
        let has = |name: &str| completions(researches, name);
        if o.owner == 0 {
            return [0, 0];
        }
        let mut b = [0, 0];
        if crate::selection::unit(o) {
            b[0] += match weapon_category(o.class) {
                0 => has("te_temperiron") + has("te_scimitar"),
                1 => has("te_crossbow") + has("te_chemistry"),
                2 | 5 => 5 * has("te_engineering"),
                _ => 0,
            };
            // Attribute 0x271b is a virtual class property, not simply 'unit'.
            // Infantry, workers, monks, casters and these creatures override it;
            // cavalry, siege engines, ships, bats and dragons do not.
            if matches!(o.class, 1 | 3..=7 | 12..=14 | 17 | 18 | 27..=31 | 33 | 36) {
                b[1] += has("te_scalearmor") + 2 * has("te_chainmail");
            }
        }
        if o.class == 18 {
            b[0] += 5 * has("te_inquisition");
        }
        b
    }
    pub fn read(o: &MapObject, lib: &ObjectLibrary) -> Self {
        let base = lib.attributes.get(o);
        let original = |offset, fallback, flag| {
            let value = o.word(offset);
            if value == 0 || o.flags & flag != 0 {
                fallback
            } else {
                value
            }
        };
        let level = o.word(0x1c);
        let attack = original(0x16, base.attack, 0x100) as u32;
        let defence = original(0x18, base.defence, 0x200) as u32;
        let max_mana = lib
            .property(o, "max_manna")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let mana = original(0x24, max_mana, 0x800);
        let max_mana = original(0x26, if max_mana > 0 { max_mana } else { mana }, 0x800);
        let (mana, max_mana) =
            if o.class == class::ITEM_MAGIC && (o.byte(98) != 0 || o.flags & 4 != 0) {
                (o.word(100), o.word(100))
            } else {
                (mana, max_mana)
            };
        // 0x494f66: `fog_range` of the INI seeds the record's sight; the
        // tables then overwrite it for the classes they list.
        let record_sight = if lib.attributes.lists(o) {
            base.sight
        } else {
            lib.property(o, "fog_range")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0)
        };
        let sight = match o.byte(0x1a) {
            0 => 5,
            255 => record_sight,
            n => n as u16,
        };
        let category = weapon_category(o.class);
        let [more_attack, more_defence] = level_bonus(o, level);
        Self {
            attack: attack + more_attack,
            defence: defence + more_defence,
            level,
            mana,
            max_mana,
            sight,
            resistance: base.resistance,
            ranged: category != 0,
            size: Some(footprint(o, lib)),
            ..Self::default()
        }
    }
}
/// What `level` adds to attack and defence (0x4de7d0, 0x4de880): the getters
/// count it at each use over the record's own values (+0x62, +0x60). Bows
/// gain half an attack point per level (rounded up) for a kingdom owner;
/// neutral units gain a full point. Defence gains a point every four bow
/// levels, two otherwise. Towers use the base object's full level bonus
/// (0x502630).
pub fn level_bonus(o: &MapObject, level: u16) -> [u32; 2] {
    let unit = crate::selection::unit(o);
    let bow = weapon_category(o.class) == 1;
    let tower = (39..=43).contains(&o.class);
    let attack = if unit || tower {
        if unit && bow && o.owner != 0 {
            (level as u32).div_ceil(2)
        } else {
            level as u32
        }
    } else {
        0
    };
    let defence = if unit {
        (level as u32).div_ceil(if bow { 4 } else { 2 })
    } else {
        0
    };
    [attack, defence]
}
/// Cells an object covers: buildings and mines by their picture, anything
/// else one cell.
pub fn footprint(o: &MapObject, lib: &ObjectLibrary) -> [u8; 2] {
    if o.building() || matches!(o.class, class::RES_GOLD | class::RES_STONE) {
        lib.cell_footprint(o).map(|n| n.min(255) as u8)
    } else {
        [1, 1]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Weather {
    #[default]
    Clear,
    Rain,
    Snow,
}
/// The game's random generator (0x51a110, range 0x51a0c0): a 32-bit shift
/// register stepped 32 times from state + 1. The game stream (0x5cf58c) and
/// the stream of sounds and pictures (0x5cf588) are reseeded with 666 when a
/// game starts.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeRng(pub u32);
impl NativeRng {
    pub fn next(&mut self) -> u32 {
        let mut x = self.0.wrapping_add(1);
        for _ in 0..32 {
            x <<= 1;
            if ((x >> 31) ^ (x >> 30)) & 1 != 0 {
                x += 1;
            }
        }
        self.0 = x;
        x
    }
    /// `lo..=hi`; `lo` without a draw when `hi <= lo`.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        let span = (hi as i64 - lo as i64 + 1) as u32;
        (lo as i64 + (self.next() % span) as i64) as i32
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Simulation {
    pub elapsed: Duration,
    pub ticks: u64,
    pub objects: Vec<ObjectState>,
    pub fog: Fog,
    pub weather: Weather,
    pub precipitation: f32,
    pub clouds: bool,
    pub rain_allowed: bool,
    pub snow_allowed: bool,
    pub seed: u32,
    /// Persistent player orders and activities. Old snapshots start with no orders.
    #[serde(default = "crate::gameplay::Runtime::missing")]
    pub gameplay: crate::gameplay::Runtime,
    /// Other kingdoms' buildings and mines as last seen (fakebuilding,
    /// 0x420520): whether each was ever seen, and its picture frozen while
    /// the fog covers it, kept after it is destroyed or captured.
    #[serde(default)]
    pub memory: std::collections::BTreeMap<usize, Memory>,
    /// Steps of the day clock moved by `@settime` (0x42b520).
    #[serde(default)]
    pub day_shift: u16,
    /// Game speed (0x587748), part of the game as in the original: 10, the
    /// normal, at every new map (0x44153f), kept by the save (0x443d71).
    #[serde(default = "normal_speed")]
    pub speed: u8,
    /// Cells each object covers, by object id, for the fog tests.
    #[serde(skip)]
    pub footprints: HashMap<u16, [u8; 2]>,
    /// One-shot presentation events are consumed by the frontend, never replayed from saves.
    #[serde(skip)]
    pub sounds: Vec<crate::gameplay::SoundEvent>,
    /// One-shot animations for the frontend, never saved.
    #[serde(skip)]
    pub flashes: Vec<crate::gameplay::Flash>,
    /// Lightning bolts for the frontend, never saved.
    #[serde(skip)]
    pub bolts: Vec<crate::bolt::Strike>,
}
/// The normal game speed, set at every new map (0x44153f).
pub const NORMAL_SPEED: u8 = 10;
fn normal_speed() -> u8 {
    NORMAL_SPEED
}
/// A game speed key of the numeric keypad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeedKey {
    Faster,
    Slower,
    Turbo,
}
/// Whether kingdom `k` sees what the object sees (vt+0x298, 0x506ae0): as
/// its byte for `k` says (+0xa4: 1 sees, 2 does not), else when it is its
/// own, hides one of its spies, or its kingdom shares its vision with `k`.
pub fn lends_sight(o: &MapObject, k: u8, shared: &dyn Fn(u8) -> bool) -> bool {
    match o.sight.get(usize::from(k).wrapping_sub(1)) {
        Some(&s) if s != 0 => s == 1,
        _ => o.owner == k || shared(o.owner) || (o.mole != 0 && o.mole == k),
    }
}
/// [`lends_sight`] with the mission's relations, as the fog reads them.
pub fn sees_through(m: &MissionWorld, o: &MapObject, k: u8) -> bool {
    lends_sight(o, k, &|owner| vision_shared(&m.diplomacy, owner, k))
}
/// Kingdom `owner` is `k`'s ally and shares its vision with it.
fn vision_shared(d: &[u32], owner: u8, k: u8) -> bool {
    (1..=8).contains(&owner)
        && (1..=8).contains(&k)
        && d.get((owner as usize - 1) * 8 + k as usize - 1)
            .is_some_and(|v| v & 0x41 == 0x41)
}
/// The speed after a key (0x4f5892–0x4f5985), None when the key does
/// nothing: + adds 5 below 20, − takes 5 above 0 (from 100 too: 95, 90…),
/// `*` goes to 100 or, above 20, back to the normal speed.
pub fn speed_after(speed: u8, key: SpeedKey) -> Option<u8> {
    match key {
        SpeedKey::Faster => (speed < 20).then(|| speed + 5),
        SpeedKey::Slower => (speed > 0).then(|| speed.saturating_sub(5)),
        SpeedKey::Turbo => Some(if speed > 20 { NORMAL_SPEED } else { 100 }),
    }
}
/// The `msg` word of the speed message, after `game_speed` (0x43f180).
pub fn speed_label(speed: u8) -> &'static str {
    match speed {
        0..5 => "game_speed_slowest",
        5..10 => "game_speed_slow",
        10..15 => "game_speed_normal",
        15..20 => "game_speed_fast",
        20..30 => "game_speed_fastest",
        _ => "game_speed_turbo",
    }
}
/// A building or mine of another kingdom in the player's memory.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Memory {
    /// Seen at least once (+0x116).
    pub seen: bool,
    /// The object as it looked when the fog covered it.
    pub snapshot: Option<MapObject>,
}
impl Simulation {
    pub fn new(m: &MissionWorld, lib: &ObjectLibrary, side: usize, biome: &str) -> Self {
        let mut out = Self {
            elapsed: Duration::ZERO,
            ticks: 0,
            objects: m
                .objects
                .iter()
                .map(|o| {
                    let mut state = ObjectState::read(o, lib);
                    if let Some(rules) = m.rules.get(o.owner.wrapping_sub(1) as usize) {
                        // 0x441072: the scenario's researches go through the
                        // handlers after the objects are made.
                        state.apply_dispatch(o, &rules.initial_researches);
                    }
                    state
                })
                .collect(),
            fog: Fog::new(side),
            weather: Weather::Clear,
            precipitation: 0.,
            clouds: true,
            rain_allowed: biome == "grass",
            snow_allowed: biome == "snow",
            seed: m.objects.first().map_or(1, |o| o.id as u32),
            gameplay: Default::default(),
            sounds: Vec::new(),
            flashes: Vec::new(),
            bolts: Vec::new(),
            memory: Default::default(),
            day_shift: 0,
            speed: NORMAL_SPEED,
            footprints: Default::default(),
        };
        // A new game reseeds both streams with 666 (0x43b080); the weather
        // draws its eight clear and rain pairs first (0x4fc260).
        let mut rng = NativeRng(666);
        out.gameplay.weather_pairs = (0..8)
            .map(|_| {
                let clear = rng.range(480_000, 720_000) as u32;
                [clear, rng.range(60_000, 120_000) as u32]
            })
            .collect();
        out.gameplay.game_rng = rng;
        out.gameplay.aux_rng = NativeRng(666);
        // 0x406450 → 0x402760: the computer players of the kingdoms.
        out.gameplay.ai = crate::gameplay::ai::State::start(m);
        if m.game_rules.explored {
            out.fog.explore_all();
        }
        out.refresh_visibility(m, None);
        out
    }
    /// Tzar's clock: 480 increments of 3 game minutes, every 3750 ms.
    /// A full day takes 1800 seconds; the first clock value is 10:00.
    pub fn clock_minutes(&self) -> u16 {
        ((200 + self.elapsed.as_millis() / 3750 + self.day_shift as u128) % 480) as u16 * 3
    }
    /// 0x42b520: the day clock to `hour` (under 24) and minute 0, its step
    /// (hour × 60) / 3; the next one comes at the clock's usual time.
    pub fn set_hour(&mut self, hour: u8) {
        if hour < 24 {
            let now = (200 + self.elapsed.as_millis() / 3750) % 480;
            self.day_shift = ((hour as u128 * 20 + 480 - now) % 480) as u16;
        }
    }
    pub fn night_amount(&self) -> f32 {
        let step = self.clock_minutes() / 3;
        match step {
            130..430 => 0.,
            120..130 => (130 - step) as f32 / 10.,
            430..440 => (step - 430) as f32 / 10.,
            _ => 1.,
        }
    }
    pub fn lighting(&self, enabled: bool) -> [f32; 3] {
        let n = if enabled { self.night_amount() } else { 0. };
        [1. - 0.45 * n, 1. - 0.32 * n, 1.]
    }
    /// `dt` is game time, already scaled by the game speed.
    pub fn advance(&mut self, dt: Duration, weather: bool) {
        // A fixed 50 ms tick, independent of rendering.
        self.elapsed += dt;
        self.ticks = self.elapsed.as_millis() as u64 / 50;
        self.update_weather(weather);
    }
    /// Apply display settings even when the simulation clock is paused.
    pub fn update_weather(&mut self, weather: bool) {
        // 0x4fc260: eight pairs of 480–720 s clear / 60–120 s precipitation.
        // 0x4fc34e starts the first clear interval at one quarter of its length.
        // ReTzar seeds its own generator, without advancing the original global RNG.
        let intervals: Vec<_> = (0..8).map(|i| self.weather_interval(i)).collect();
        let mut ms = self.elapsed.as_millis();
        let first = intervals[0].0.as_millis() / 4;
        let mut rain_phase = None;
        if ms >= first {
            ms -= first;
            let period: u128 = intervals
                .iter()
                .map(|(clear, rain)| clear.as_millis() + rain.as_millis())
                .sum();
            ms %= period;
            for i in 0..8 {
                let duration = intervals[i].1.as_millis();
                if ms < duration {
                    rain_phase = Some((ms as f32, duration as f32));
                    break;
                }
                ms -= duration;
                let clear = intervals[(i + 1) % 8].0.as_millis();
                if ms < clear {
                    break;
                }
                ms -= clear;
            }
        }
        // 0x4a71e0: fronts add or remove ten particles every second 70 ms
        // tick, to a cap of 256: 26 steps, about 3.64 s.
        self.precipitation = if weather && (self.rain_allowed || self.snow_allowed) {
            rain_phase.map_or(0., |(at, length)| {
                (at / 3640.).min((length - at) / 3640.).clamp(0., 1.)
            })
        } else {
            0.
        };
        self.weather = if self.precipitation == 0. {
            Weather::Clear
        } else if self.snow_allowed {
            Weather::Snow
        } else {
            Weather::Rain
        };
    }
    pub fn weather_interval(&self, index: usize) -> (Duration, Duration) {
        if let Some(&[clear, rain]) = self.gameplay.weather_pairs.get(index % 8) {
            return (
                Duration::from_millis(clear as u64),
                Duration::from_millis(rain as u64),
            );
        }
        // Snapshots older than the native pairs.
        let mut seed = self.seed;
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            seed as u64
        };
        for _ in 0..index % 8 {
            next();
            next();
        }
        (
            Duration::from_millis(480000 + next() % 240001),
            Duration::from_millis(60000 + next() % 60001),
        )
    }
    pub fn refresh_visibility(&mut self, mission: &MissionWorld, diplomacy: Option<&[u32]>) {
        let previous = std::mem::take(&mut self.fog.sources);
        self.fog.clear_visible();
        let player = mission.player();
        let d = diplomacy.unwrap_or(&mission.diplomacy);
        let shared = |owner: u8| vision_shared(d, owner, player);
        for (o, state) in mission.objects.iter().zip(&self.objects) {
            if o.flags & (FLAG_INACTIVE | 4) != 0
                || state.holder.is_some()
                || !lends_sight(o, player, &shared)
            {
                continue;
            }
            if crate::selection::unit(o) || o.building() || o.class == class::FOG_REVEALER {
                // A large object sees from its centre (0x50208b).
                let [w, h] = state.size.unwrap_or([1, 1]);
                self.fog.reveal(
                    o.x as i32 + w as i32 / 2,
                    o.y as i32 + h as i32 / 2,
                    state.sight as i32,
                );
            }
        }
        // Magic vision is shared by the caster's allies; a vision bound to a
        // unit follows it with that unit's sight.
        let allied = |owner: u8| owner == 0 || owner == player || shared(owner);
        for vision in self
            .gameplay
            .visions
            .iter()
            .filter(|v| v.until > self.ticks && allied(v.owner))
        {
            let (cell, radius) = match vision
                .object
                .and_then(|j| mission.objects.get(j).zip(self.objects.get(j)))
            {
                Some((o, state)) if o.flags & FLAG_INACTIVE == 0 => ([o.x, o.y], state.sight),
                Some(_) => continue,
                None => (vision.cell, vision.radius),
            };
            self.fog
                .reveal(cell[0] as i32, cell[1] as i32, radius as i32);
        }
        self.fog.note_changes(previous);
        self.fog.remember();
        // No fog removes the shroud over previously explored ground, while
        // exploration controls the initial black, undiscovered part of the map.
        if mission.game_rules.no_fog {
            self.fog.visible.clone_from(&self.fog.explored);
            self.fog
                .visible_vertices
                .clone_from(&self.fog.explored_vertices);
        }
        self.footprints = mission
            .objects
            .iter()
            .zip(&self.objects)
            .map(|(o, s)| (o.id, s.size.unwrap_or([1, 1])))
            .collect();
        // Other kingdoms' buildings and mines (0x4204b0): seen ones are
        // remembered as they were when the fog covers them (0x420b90), until
        // any of their cells is in view again (0x420e30).
        for (i, (o, state)) in mission.objects.iter().zip(&self.objects).enumerate() {
            if !(o.building() || matches!(o.class, class::RES_GOLD | class::RES_STONE))
                || o.owner == player
            {
                self.memory.remove(&i);
                continue;
            }
            // Buildings of a kingdom sharing its vision count as seen
            // (0x41d571, 0x41fb6c).
            if shared(o.owner) {
                let entry = self.memory.entry(i).or_default();
                entry.seen = true;
                entry.snapshot = None;
                continue;
            }
            let size = state.size.unwrap_or([1, 1]);
            let live = o.flags & (FLAG_INACTIVE | crate::world::FLAG_DYING) == 0;
            let entry = self.memory.entry(i).or_default();
            let view = match &entry.snapshot {
                Some(s) => self.fog.area([s.x, s.y], size).1,
                None => self.fog.area([o.x, o.y], size).1,
            };
            if view {
                entry.snapshot = None;
                entry.seen |= live;
            } else if live && entry.seen && entry.snapshot.is_none() {
                entry.snapshot = Some(o.clone());
            }
            if !entry.seen && entry.snapshot.is_none() && !live {
                self.memory.remove(&i);
            }
        }
    }
    pub fn visible(&self, o: &MapObject) -> bool {
        let size = self.footprints.get(&o.id).copied().unwrap_or([1, 1]);
        self.fog.area([o.x, o.y], size).1
    }
    /// Explored ground under any of the object's cells (0x505610).
    pub fn known(&self, o: &MapObject) -> bool {
        let size = self.footprints.get(&o.id).copied().unwrap_or([1, 1]);
        self.fog.area([o.x, o.y], size).0
    }
    /// What the player sees of object `i`: itself, the picture of it kept in
    /// memory, or nothing for another kingdom's building never seen. Units
    /// and everything else follow `visible` and `known`.
    pub fn appearance<'a>(&'a self, i: usize, o: &'a MapObject) -> Option<&'a MapObject> {
        let Some(memory) = self.memory.get(&i) else {
            return Some(o);
        };
        if self.visible(o) && memory.snapshot.is_none() {
            Some(o)
        } else if let Some(s) = &memory.snapshot {
            Some(s)
        } else if memory.seen {
            Some(o)
        } else {
            None
        }
    }
    /// Equipment and active effects contribute to both the HUD and combat.
    /// Books and potions do not add passive weapon bonuses.
    pub fn effective_attributes(&self, index: usize, m: &MissionWorld) -> Option<ObjectState> {
        // Equipment is already in the stored values (0x463480).
        let mut state = self.objects.get(index)?.clone();
        let _ = m;
        if let Some(actor) = self.gameplay.actors.get(index) {
            for effect in actor.effects.iter().filter(|e| e.until > self.ticks) {
                state.attack =
                    (state.attack as i64 + effect.attack as i64).clamp(0, u32::MAX as i64) as u32;
                state.defence =
                    (state.defence as i64 + effect.defence as i64).clamp(0, u32::MAX as i64) as u32;
            }
        }
        Some(state)
    }
}

/// How many times a kingdom completed a research: a research queued in two
/// busy buildings completes twice and its handler runs twice (0x4d33d0).
pub fn completions(researches: &[String], name: &str) -> u32 {
    researches
        .iter()
        .filter(|r| r.eq_ignore_ascii_case(name))
        .count() as u32
}
/// Great Wall's initial handler updates both existing hit point words by 30%.
pub fn initial_health(mission: &mut MissionWorld) {
    for o in &mut mission.objects {
        let times = mission
            .rules
            .get(o.owner.wrapping_sub(1) as usize)
            .map_or(0, |r| completions(&r.initial_researches, "te_greatwall"));
        for _ in 0..times {
            great_wall_health(o);
        }
    }
}

/// Apply once for each completion of the research, on what exists then and
/// on what is built later.
/// Native 0x4d3f81 adds floor(HP * 30 / 100) to each HP word independently.
pub fn great_wall_health(o: &mut MapObject) {
    if matches!(o.class, 39 | 42 | class::WALL_STONE | class::GATE_STONE) {
        o.health = o.health.saturating_add((o.health as u32 * 30 / 100) as u16);
        o.max_health = o
            .max_health
            .saturating_add((o.max_health as u32 * 30 / 100) as u16);
        if o.data.len() >= 22 {
            o.data[18..20].copy_from_slice(&o.health.to_le_bytes());
            o.data[20..22].copy_from_slice(&o.max_health.to_le_bytes());
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fog {
    pub side: usize,
    /// Cells with an explored corner, and cells with a corner in view.
    pub explored: Vec<bool>,
    pub visible: Vec<bool>,
    pub revision: u64,
    /// The native grid (0x5b7828): (side + 1)² vertices explored by the
    /// player and in view now. Older saves rebuild it from the cells.
    #[serde(default)]
    pub explored_vertices: Vec<bool>,
    #[serde(default)]
    pub visible_vertices: Vec<bool>,
    /// The sights of the last refresh: cell and radius.
    #[serde(default)]
    pub sources: Vec<[i32; 3]>,
    /// The sights that came or went since the game last looked: the
    /// original updates the counter of every vertex they cover (0x453da0).
    #[serde(default)]
    pub changed: Vec<[i32; 3]>,
}
impl Fog {
    pub fn new(side: usize) -> Self {
        Self {
            side,
            explored: vec![false; side * side],
            visible: vec![false; side * side],
            revision: 0,
            explored_vertices: vec![false; (side + 1) * (side + 1)],
            visible_vertices: vec![false; (side + 1) * (side + 1)],
            sources: Vec::new(),
            changed: Vec::new(),
        }
    }
    /// The vertex lies within the sight `[x, y, r]` (`reveal`).
    pub fn covers(s: [i32; 3], v: [i32; 2]) -> bool {
        let (dx, dy) = (v[0] - s[0], v[1] - s[1]);
        let r = s[2].clamp(0, 31);
        let d = ((dx as f64 - 0.5).powi(2) + (dy as f64 - 0.5).powi(2)).sqrt();
        (1 - r..=r).contains(&dx)
            && (1 - r..=r).contains(&dy)
            && (d + 0.9999999).trunc() as i32 <= r
    }
    pub fn at(&self, x: usize, y: usize) -> (bool, bool) {
        if x >= self.side || y >= self.side {
            (false, false)
        } else {
            (
                self.explored[y * self.side + x],
                self.visible[y * self.side + x],
            )
        }
    }
    /// A vertex of the grid: explored, in view.
    pub fn vertex(&self, x: usize, y: usize) -> (bool, bool) {
        let n = self.side + 1;
        if x >= n || y >= n || self.explored_vertices.len() != n * n {
            return self.at(
                x.min(self.side.saturating_sub(1)),
                y.min(self.side.saturating_sub(1)),
            );
        }
        (
            self.explored_vertices[y * n + x],
            self.visible_vertices[y * n + x],
        )
    }
    /// Vertex grids of a save made before they existed: a vertex counts as
    /// its neighbouring cells do.
    fn ensure_vertices(&mut self) {
        let n = self.side + 1;
        if self.explored_vertices.len() == n * n && self.visible_vertices.len() == n * n {
            return;
        }
        self.explored_vertices = vec![false; n * n];
        self.visible_vertices = vec![false; n * n];
        for y in 0..self.side {
            for x in 0..self.side {
                let (seen, view) = self.at(x, y);
                for (vx, vy) in [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)] {
                    self.explored_vertices[vy * n + vx] |= seen;
                    self.visible_vertices[vy * n + vx] |= view;
                }
            }
        }
    }
    /// Nothing in view until the sources reveal again.
    pub fn clear_visible(&mut self) {
        self.ensure_vertices();
        self.visible_vertices.fill(false);
        self.visible.fill(false);
    }
    /// A sight of `r` from the cell (x, y) (0x453da0): the vertices within
    /// `r` of the cell's centre, `trunc(√((dx−½)² + (dy−½)²) + 0.9999999) ≤ r`
    /// as the table 0x5e4c88 holds them (r up to 31).
    pub fn reveal(&mut self, x: i32, y: i32, r: i32) {
        self.ensure_vertices();
        let r = r.clamp(0, 31);
        self.sources.push([x, y, r]);
        let n = self.side as i32 + 1;
        for dy in 1 - r..=r {
            for dx in 1 - r..=r {
                let d = ((dx as f64 - 0.5).powi(2) + (dy as f64 - 0.5).powi(2)).sqrt();
                if (d + 0.9999999).trunc() as i32 > r {
                    continue;
                }
                let (vx, vy) = (x + dx, y + dy);
                if (0..n).contains(&vx) && (0..n).contains(&vy) {
                    self.visible_vertices[(vy * n + vx) as usize] = true;
                }
            }
        }
    }
    /// The sights that came or went since the last refresh.
    fn note_changes(&mut self, mut previous: Vec<[i32; 3]>) {
        let mut now = self.sources.clone();
        now.sort_unstable();
        previous.sort_unstable();
        let (mut i, mut j) = (0, 0);
        loop {
            match (now.get(i), previous.get(j)) {
                (Some(a), Some(b)) if a == b => {
                    i += 1;
                    j += 1;
                }
                (Some(a), b) if b.is_none_or(|b| a < b) => {
                    self.changed.push(*a);
                    i += 1;
                }
                (_, Some(b)) => {
                    self.changed.push(*b);
                    j += 1;
                }
                _ => break,
            }
        }
    }
    /// The whole map explored (0x442436 ORs every vertex).
    pub fn explore_all(&mut self) {
        self.ensure_vertices();
        self.explored.fill(true);
        self.explored_vertices.fill(true);
    }
    /// What is in view becomes explored; the cells follow their corners.
    pub fn remember(&mut self) {
        self.ensure_vertices();
        let n = self.side + 1;
        for (seen, view) in self
            .explored_vertices
            .iter_mut()
            .zip(&self.visible_vertices)
        {
            *seen |= *view;
        }
        let side = self.side;
        let corners = |c: usize| {
            let (x, y) = (c % side, c / side);
            [
                y * n + x,
                y * n + x + 1,
                (y + 1) * n + x,
                (y + 1) * n + x + 1,
            ]
        };
        // A cell explored from outside (a whole explored map, a restored
        // save) explores its corners. Only one with none explored: a cell
        // explored through one corner would explore the others, and the
        // ground would spread a cell at every refresh.
        let outside: Vec<usize> = (0..side * side)
            .filter(|&c| self.explored[c] && !corners(c).iter().any(|&v| self.explored_vertices[v]))
            .collect();
        for c in outside {
            for v in corners(c) {
                self.explored_vertices[v] = true;
            }
        }
        for c in 0..side * side {
            let k = corners(c);
            self.explored[c] = k.iter().any(|&v| self.explored_vertices[v]);
            self.visible[c] = k.iter().any(|&v| self.visible_vertices[v]);
        }
        self.revision += 1;
    }
    /// Whether any cell of an area is explored or in view.
    pub fn area(&self, at: [u16; 2], size: [u8; 2]) -> (bool, bool) {
        let mut out = (false, false);
        for y in at[1] as usize..at[1] as usize + size[1].max(1) as usize {
            for x in at[0] as usize..at[0] as usize + size[0].max(1) as usize {
                let (seen, view) = self.at(x, y);
                out.0 |= seen;
                out.1 |= view;
            }
        }
        out
    }
    pub fn bitmap(&self) -> Bitmap {
        Bitmap {
            width: self.side as u32,
            height: self.side as u32,
            rgba: self
                .explored
                .iter()
                .zip(&self.visible)
                .flat_map(|(&seen, &visible)| {
                    [
                        0,
                        0,
                        0,
                        if visible {
                            0
                        } else if seen {
                            128
                        } else {
                            255
                        },
                    ]
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explored_ground_stays_where_the_sights_went() {
        let mut fog = Fog::new(32);
        fog.reveal(10, 10, 3);
        fog.remember();
        let explored = fog.explored.clone();
        assert!(explored[10 * 32 + 10] && !explored[10 * 32 + 20]);
        // Sights gone, the explored ground neither spreads nor shrinks.
        for _ in 0..50 {
            fog.clear_visible();
            fog.remember();
        }
        assert_eq!(fog.explored, explored);
        // A cell explored from outside explores its corners once.
        fog.explored[20 * 32 + 20] = true;
        fog.remember();
        let once = fog.explored.clone();
        fog.remember();
        assert_eq!(fog.explored, once);
    }
    #[test]
    fn speed_keys_and_their_message_follow_the_original() {
        use SpeedKey::*;
        // + stops at 20, − at 0 (0x4f5906, 0x4f5965).
        assert_eq!(speed_after(15, Faster), Some(20));
        assert_eq!(speed_after(20, Faster), None);
        assert_eq!(speed_after(5, Slower), Some(0));
        assert_eq!(speed_after(0, Slower), None);
        // `*` goes to 100 and back to the normal, not to the speed before
        // (0x4f5892); − from 100 goes on in steps of 5, + does nothing.
        assert_eq!(speed_after(15, Turbo), Some(100));
        assert_eq!(speed_after(100, Turbo), Some(NORMAL_SPEED));
        assert_eq!(speed_after(100, Slower), Some(95));
        assert_eq!(speed_after(95, Faster), None);
        assert_eq!(speed_after(25, Turbo), Some(NORMAL_SPEED));
        // The word of each level (0x43f180).
        let words: Vec<_> = [0, 5, 10, 15, 20, 25, 95, 100]
            .map(speed_label)
            .into_iter()
            .collect();
        assert_eq!(
            words,
            [
                "game_speed_slowest",
                "game_speed_slow",
                "game_speed_normal",
                "game_speed_fast",
                "game_speed_fastest",
                "game_speed_fastest",
                "game_speed_turbo",
                "game_speed_turbo",
            ]
        );
        // Saves without the speed load at the normal one.
        assert_eq!(normal_speed(), NORMAL_SPEED);
    }
    #[test]
    fn clock_and_night_match_original_boundaries() {
        let mut sim = Simulation {
            elapsed: Duration::ZERO,
            ticks: 0,
            objects: vec![],
            fog: Fog::new(1),
            weather: Weather::Clear,
            precipitation: 0.,
            clouds: false,
            rain_allowed: true,
            snow_allowed: false,
            seed: 1,
            gameplay: Default::default(),
            sounds: Vec::new(),
            flashes: Vec::new(),
            bolts: Vec::new(),
            memory: Default::default(),
            day_shift: 0,
            speed: NORMAL_SPEED,
            footprints: Default::default(),
        };
        assert_eq!(sim.clock_minutes(), 600);
        assert_eq!(sim.lighting(true), [1.; 3]);
        sim.advance(Duration::from_secs(1800), true);
        assert_eq!(sim.clock_minutes(), 600);
        sim.elapsed = Duration::from_millis(230 * 3750);
        assert_eq!(sim.clock_minutes(), 1290);
        assert_eq!(sim.night_amount(), 0.);
        sim.elapsed += Duration::from_millis(10 * 3750);
        assert_eq!(sim.clock_minutes(), 1320);
        assert_eq!(sim.night_amount(), 1.);
        assert_eq!(sim.lighting(false), [1.; 3]);
    }
    #[test]
    fn fog_remembers_without_revealing_outside_circle() {
        let mut f = Fog::new(32);
        f.reveal(0, 0, 5);
        f.remember();
        assert_eq!(f.at(5, 0), (true, true));
        assert_eq!(f.at(5, 5), (false, false));
        f.clear_visible();
        f.remember();
        assert_eq!(f.at(5, 0), (true, false));
        assert_eq!(f.bitmap().rgba[5 * 4 + 3], 128);
        f.reveal(31, 31, 10000);
        f.remember();
        assert_eq!(f.explored.len(), 1024);
    }
}

#[cfg(test)]
mod rng_tests {
    use super::NativeRng;
    #[test]
    fn native_rng_follows_the_original_sequence() {
        let mut r = NativeRng(666);
        assert_eq!(
            [r.next(), r.next(), r.next(), r.next(), r.next()],
            [3930, 9178, 51418, 176858, 1027802]
        );
        // An empty range returns its bound without a draw (0x51a0c0).
        let state = r.0;
        assert_eq!(r.range(5, 5), 5);
        assert_eq!(r.0, state);
        // The first weather pair of every game (0x4fc260).
        let mut r = NativeRng(666);
        assert_eq!(
            [r.range(480_000, 720_000), r.range(60_000, 120_000)],
            [483_930, 69_178]
        );
    }
}
