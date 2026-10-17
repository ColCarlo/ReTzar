//! Read-only import of mission objects and scripts; the computer players remain inactive.
use crate::{
    Result,
    assets::{GameAssets, Ini, legacy_text},
    bitmap::Bitmap,
    hpfs::Container,
    invalid,
    map::MapGrid,
    sprite::SpriteSheet,
    u32_at,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub(crate) mod starting;
mod walls;

pub const CLASSES: [&str; 144] = [
    "none",
    "peon",
    "trader",
    "footman",
    "archer",
    "maceman",
    "longbowman",
    "pikeman",
    "knight",
    "samurai",
    "horse_archer",
    "crusader",
    "monk",
    "ninja",
    "spy",
    "bat",
    "troll",
    "mage",
    "priest",
    "catapult",
    "hcatapult",
    "ballista",
    "carpet",
    "galleon",
    "hulk",
    "xebec",
    "bear",
    "dwarf",
    "ork",
    "enichar",
    "jinn",
    "ghost",
    "dragon",
    "jihad_warrior",
    "horse_samurai",
    "heavy_cavalry",
    "skeleton",
    "chariot",
    "messiah",
    "tower_stone",
    "tower_wood",
    "tower_wood2",
    "tower_stone2",
    "tower_rocket",
    "castle",
    "barrack",
    "lumber",
    "farm",
    "tavern",
    "church",
    "stable",
    "storage",
    "outpost",
    "market",
    "harbor",
    "cathedral",
    "academy",
    "shipyard",
    "blacksmith",
    "workshop",
    "magetower",
    "wall_stone",
    "wall_wood",
    "field",
    "bridge",
    "tcguild",
    "house",
    "gate_wood",
    "gate_stone",
    "magicworkshop",
    "shaolinn",
    "mosque",
    "magicgateway",
    "evilcastle",
    "arrow",
    "cursor",
    "res_gold",
    "fog_revealer",
    "res_stone",
    "item",
    "resitem",
    "item_plusplus",
    "animation",
    "ruin",
    "cloud",
    "open_next_map",
    "not used 1",
    "debris",
    "not used 2",
    "explosion",
    "command_move",
    "command_attack",
    "command_harvest",
    "command_capture",
    "command_pickitem",
    "command_patrol",
    "cannon",
    "command_default",
    "fire_rain",
    "command_repair",
    "command_guard",
    "lightning",
    "treeselection",
    "item_scroll",
    "item_magic",
    "magic",
    "item_health",
    "ground_object",
    "ground_patch",
    "command_spell",
    "cow",
    "fish",
    "bird",
    "command_unload",
    "command_trade",
    "fishpatch",
    "crusadeanchor",
    "fakebuilding",
    "fakeresource",
    "eagle",
    "bolt",
    "flame_bolt",
    "visualobject",
    "command_explore",
    "freeze_bolt",
    "item_soul",
    "command_murder",
    "command_impersonate",
    "command_bribe",
    "command_board",
    "whirlpool",
    "sound_object",
    "flame_ball",
    "dummy",
    "rocket_arrow",
    "item_flag",
    "fishing_boat",
    "command_fishing",
    "command_attackground",
    "enemy_ground",
    "fakewall",
    "fakeitem",
    "rally_flag",
    "fakegate",
];

/// Indices into [`CLASSES`] that the importer and interface rely on.
pub mod class {
    pub const NONE: usize = 0;
    pub const PEON: usize = 1;
    pub const FOOTMAN: usize = 3;
    pub const PIKEMAN: usize = 7;
    pub const NINJA: usize = 13;
    /// Last unit class of the first group (`messiah`).
    pub const MESSIAH: usize = 38;
    /// First building class (`tower_stone`); buildings end at [`EVILCASTLE`].
    pub const TOWER_STONE: usize = 39;
    /// The wooden tower (`tower_wood`).
    pub const TOWER_WOOD: usize = 40;
    pub const WALL_STONE: usize = 61;
    pub const WALL_WOOD: usize = 62;
    pub const BRIDGE: usize = 64;
    pub const HOUSE: usize = 66;
    pub const GATE_WOOD: usize = 67;
    pub const GATE_STONE: usize = 68;
    pub const CASTLE: usize = 44;
    pub const FARM: usize = 47;
    pub const BARRACK: usize = 45;
    pub const LUMBER: usize = 46;
    pub const TAVERN: usize = 48;
    pub const STORAGE: usize = 51;
    pub const OUTPOST: usize = 52;
    pub const WORKSHOP: usize = 59;
    pub const MAGE: usize = 17;
    pub const PRIEST: usize = 18;
    pub const MARKET: usize = 53;
    pub const FIELD: usize = 63;
    pub const HARBOR: usize = 54;
    pub const CATHEDRAL: usize = 55;
    /// The warriors' guild (`academy`).
    pub const ACADEMY: usize = 56;
    pub const SKELETON: usize = 36;
    pub const MAGIC_GATEWAY: usize = 72;
    pub const EVILCASTLE: usize = 73;
    pub const CURSOR: usize = 75;
    pub const RES_GOLD: usize = 76;
    pub const FOG_REVEALER: usize = 77;
    pub const RES_STONE: usize = 78;
    pub const ITEM: usize = 79;
    pub const RESITEM: usize = 80;
    pub const ITEM_PLUSPLUS: usize = 81;
    pub const ANIMATION: usize = 82;
    pub const RUIN: usize = 83;
    pub const DEBRIS: usize = 87;
    pub const OPEN_NEXT_MAP: usize = 85;
    pub const TREESELECTION: usize = 102;
    pub const ITEM_SCROLL: usize = 103;
    pub const ITEM_MAGIC: usize = 104;
    pub const ITEM_HEALTH: usize = 106;
    pub const GROUND_OBJECT: usize = 107;
    pub const GROUND_PATCH: usize = 108;
    pub const COW: usize = 110;
    pub const BIRD: usize = 112;
    pub const EAGLE: usize = 119;
    pub const FISHPATCH: usize = 115;
    pub const CRUSADEANCHOR: usize = 116;
    pub const FAKEBUILDING: usize = 117;
    pub const FAKERESOURCE: usize = 118;
    pub const VISUALOBJECT: usize = 122;
    pub const WHIRLPOOL: usize = 130;
    pub const SOUND_OBJECT: usize = 131;
    pub const DUMMY: usize = 133;
    pub const ITEM_SOUL: usize = 125;
    pub const ITEM_FLAG: usize = 135;
    pub const FISHING_BOAT: usize = 136;
    pub const ENEMY_GROUND: usize = 139;
    pub const FAKEWALL: usize = 140;
    pub const FAKEITEM: usize = 141;
    pub const FAKEGATE: usize = 143;

    /// Engine-side markers and orders that have no picture of their own.
    pub fn is_marker(class: usize) -> bool {
        [
            NONE,
            CURSOR,
            FOG_REVEALER,
            OPEN_NEXT_MAP,
            FISHPATCH,
            CRUSADEANCHOR,
            FAKEBUILDING,
            FAKERESOURCE,
            SOUND_OBJECT,
            ENEMY_GROUND,
            FAKEWALL,
            FAKEITEM,
            FAKEGATE,
        ]
        .contains(&class)
            || super::CLASSES
                .get(class)
                .is_some_and(|n| n.starts_with("command_"))
    }
    /// Units, animals and ships: they have idle pictures and walk or fly.
    pub fn is_unit(class: usize) -> bool {
        (PEON..=MESSIAH).contains(&class) || [COW, BIRD, EAGLE, FISHING_BOAT].contains(&class)
    }
    /// Derived classes store their parent class in the payload.
    pub fn stored_base(class: usize) -> usize {
        match class {
            GROUND_OBJECT | GROUND_PATCH => ANIMATION,
            ITEM_PLUSPLUS | ITEM_SCROLL | ITEM_MAGIC | ITEM_HEALTH => ITEM,
            GATE_STONE => GATE_WOOD,
            _ => class,
        }
    }
    /// Classes whose record names a custom image section at payload offset 56.
    pub fn has_custom_image(class: usize) -> bool {
        if [ITEM_PLUSPLUS, ITEM_SCROLL, ITEM_MAGIC, ITEM_HEALTH].contains(&class) {
            return true;
        }
        [
            ITEM,
            ANIMATION,
            RUIN,
            GROUND_OBJECT,
            GROUND_PATCH,
            VISUALOBJECT,
            SOUND_OBJECT,
        ]
        .contains(&class)
    }
}
/// Inactive editor objects, activated later by scripts. Bit 4 marks abstract holders.
pub const FLAG_INACTIVE: u32 = 0x4000;
/// Record flag 0x1000 (runtime 0x40000000, 0x4ddf56): a campaign carries
/// the object to its next map (0x43ea20), where the object of the same name
/// with the flag takes it back (0x440c58).
pub const FLAG_CARRIED: u32 = 0x1000;
/// Record flag 0x2000 (runtime 0x8000000, 0x4ffe74): a unit of the story,
/// mostly the scenario's heroes, whose Idle holds its place after an order
/// even in a computer's kingdom (0x4e1bec).
pub const FLAG_STORY: u32 = 0x2000;
/// Native flag 2, dead: set on a mine that gave its last load (0x4affe0),
/// which stays on the map as the pit.
pub const FLAG_DEAD: u32 = 0x2;
/// ReTzar runtime marker of the native flag 0x100: locked by a script
/// (`LockUnlockUnits`), neither selected nor commanded.
pub const FLAG_LOCKED: u32 = 0x0800_0000;
/// ReTzar runtime marker of the native flag 0x200 set by `@god`: no damage.
pub const FLAG_GOD: u32 = 0x0400_0000;
/// ReTzar runtime marker of the native hero flag 0x4000000 (0x401920), set
/// only by the promotion (0x506d60): a scenario name alone is no hero.
pub const FLAG_HERO: u32 = 0x0200_0000;
/// ReTzar runtime construction marker; never reinterpret native flag 4.
pub const FLAG_CONSTRUCTION: u32 = 0x4000_0000;
/// ReTzar runtime marker of the native flag 0x400000: inside a holder
/// (set on the way in, 0x5044b7, cleared on the way out, 0x50475c).
pub const FLAG_HELD: u32 = 0x2000_0000;
/// ReTzar runtime marker of a dead object whose `image_die` is still playing.
pub const FLAG_DYING: u32 = 0x1000_0000;

fn word(d: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        d.get(at..at + 2)
            .ok_or_else(|| invalid("Truncated mission object"))?
            .try_into()?,
    ))
}
fn string(d: &[u8]) -> String {
    legacy_text(d.split(|b| *b == 0).next().unwrap_or_default())
        .trim()
        .to_owned()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MapObject {
    pub id: u16,
    pub class: usize,
    pub flags: u32,
    pub x: u16,
    pub y: u16,
    pub owner: u8,
    pub civilization: u8,
    pub variant: u16,
    pub health: u16,
    pub max_health: u16,
    pub name: String,
    pub subtitle: String,
    /// Hero portraits from the `.objext` record: 48×48 for the selection
    /// panel, 96×96 for dialogues (archive paths such as `USERMerlin.bmp`).
    pub portrait: String,
    pub portrait_large: String,
    pub custom_image: String,
    pub data: Vec<u8>,
    /// The kingdom of a spy living in this unit, which sees and commands it
    /// (0x4e4510: +0xa4 and +0xb0 of that kingdom); 0 for none.
    #[serde(default)]
    pub mole: u8,
    /// Each kingdom's hold on the object (+0xb0, 0x506d00): 0 as the
    /// diplomacy says, 1 commands it, 2 does not. A hidden spy gives its
    /// kingdom its host (0x4e4520); hypnosis gives the unit to those
    /// commanding the caster and takes it from the others (0x4e59b2).
    #[serde(default)]
    pub control: [u8; 8],
    /// Each kingdom's sight through the object (+0xa4, 0x506b40): 0 as
    /// the diplomacy says, 1 sees what it sees, 2 does not. A hidden spy
    /// and hypnosis write it too (0x4e4546, 0x4e599b), which ReTzar keeps
    /// in `mole` and `control`; here only the cheat panel's «spy»
    /// (0x4247fd) does.
    #[serde(default)]
    pub sight: [u8; 8],
}
impl MapObject {
    /// Native spell icon/name table at 0x58a2f0 (21 entries).
    pub fn spell_key(&self) -> Option<&'static str> {
        spell_key(self.byte(96))
    }
}
/// Native spell icon/name table at 0x58a2f0 (21 entries).
pub fn spell_key(index: u8) -> Option<&'static str> {
    // Spell identifiers follow the original game's dispatch table.
    const SPELLS: [&str; 21] = [
        "spell_firerain",
        "spell_lightning",
        "spell_heal",
        "spell_spawncarpet",
        "spell_spawnbat",
        "spell_bless",
        "spell_freeze",
        "spell_berserk",
        "spell_soulbind",
        "spell_cloak",
        "spell_spawnstonegolem",
        "spell_magicvision",
        "spell_raisedead",
        "spell_hypnotize",
        "spell_turntostone",
        "spell_whirlpool",
        "spell_spawndragon",
        "spell_flamebolt",
        "spell_raiseexp",
        "spell_spawnjin",
        "spell_curse",
    ];
    SPELLS.get(index as usize).copied()
}
impl MapObject {
    pub fn item(&self) -> bool {
        [
            class::ITEM,
            class::ITEM_PLUSPLUS,
            class::ITEM_SCROLL,
            class::ITEM_MAGIC,
            class::ITEM_HEALTH,
            class::ITEM_SOUL,
            class::ITEM_FLAG,
        ]
        .contains(&self.class)
    }
    pub fn building(&self) -> bool {
        (class::TOWER_STONE..=class::EVILCASTLE).contains(&self.class) || self.class == class::DUMMY
    }
    pub fn selectable(&self) -> bool {
        ((class::PEON..=class::EVILCASTLE).contains(&self.class)
            || self.item()
            || [
                class::RES_GOLD,
                class::RES_STONE,
                class::ITEM,
                class::COW,
                class::DUMMY,
                class::FISHING_BOAT,
            ]
            .contains(&self.class))
            && self.flags & (FLAG_INACTIVE | FLAG_HELD | FLAG_DYING | FLAG_LOCKED | FLAG_DEAD | 4)
                == 0
    }
    pub fn class_name(&self) -> &'static str {
        CLASSES[self.class]
    }
    /// The object's point in world pixels (y up). Ground patches are stored
    /// at cell (0, 0) with their exact pixel at payload 102/104, as the
    /// original loader reads them.
    pub fn location(&self) -> [f32; 2] {
        if self.class == class::GROUND_PATCH
            && self.x == 0
            && self.y == 0
            && let (Ok(x), Ok(y)) = (word(&self.data, 0x66), word(&self.data, 0x68))
        {
            return [(x as i16).max(0) as f32, -((y as i16).max(0) as f32)];
        }
        [self.x as f32 * 32., -(self.y as f32) * 32.]
    }
    /// The payload byte at `at`, or zero for short records.
    pub fn byte(&self, at: usize) -> u8 {
        self.data.get(at).copied().unwrap_or(0)
    }
    /// The little-endian payload word at `at`, or zero for short records.
    pub fn word(&self, at: usize) -> u16 {
        word(&self.data, at).unwrap_or(0)
    }
    /// The little-endian payload double word at `at`, or zero.
    pub fn dword(&self, at: usize) -> u32 {
        u32_at(&self.data, at).unwrap_or(0)
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Kingdom {
    pub name: String,
    pub leader: String,
    /// Byte 64: the kingdom takes part in the scenario; the objects of a
    /// kingdom that does not are left out at loading (mp3 has some).
    pub active: bool,
    pub color: u8,
    /// Byte 67, equal to the colour in every original scenario (the Mongol
    /// kingdoms included, so it is not a civilization); possibly the team.
    pub team: u8,
    /// Original kingdominfo byte 88 (1 European, 2 Arab, 3 Asian).
    pub civilization: u8,
    /// Bytes 68–83: gold, stone, food, wood, the engine's resource order
    /// (its names table at 0x465130 and the cost fields read at 0x495d1b).
    pub resources: [u32; 4],
    /// Byte 122: the computer plays this kingdom. Zero for the human player
    /// of every campaign and for all kingdoms of the multiplayer maps.
    pub computer: bool,
    /// Byte 121, the mode of its computer player (K+0xa, 0x4402a8): bit 0
    /// economy, bit 1 war, 0 none. The loader clears it when byte 122 is 2;
    /// a game set up on a screen gives 3 to its computers and 0 to the
    /// others (0x4402c3).
    #[serde(default)]
    pub ai_mode: u8,
    /// Bytes 89–120, its behaviour script (K+0x64, 0x44027f): a file of
    /// `data\ai\`, empty (or starting with a zero) for the default one.
    #[serde(default)]
    pub ai_script: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MissionWorld {
    /// Explicit kingdominfo starting cells, resolved before custom forces reset.
    #[serde(skip)]
    pub starting_positions: [Option<[u16; 2]>; 8],
    pub game_rules: GameRules,
    /// Stationary runtime. Scenario import remains immutable until the scene is prepared.
    #[serde(skip)]
    pub simulation: Option<crate::simulation::Simulation>,
    pub objects: Vec<MapObject>,
    pub kingdoms: Vec<Kingdom>,
    /// The scripts: events, actions, rules (header words 0x0c–0x12 count them).
    #[serde(default)]
    pub script: crate::script::Script,
    /// A campaign mission: defeats show no default message (0x442f53).
    #[serde(default)]
    pub campaign: bool,
    /// Housing set by `@setpoplimit` (kingdom +0x60), and the housing of the
    /// completed buildings at that moment: later ones add and take from it.
    #[serde(default)]
    pub population_limit: std::collections::BTreeMap<u8, usize>,
    #[serde(default)]
    pub population_base: std::collections::BTreeMap<u8, usize>,
    /// The cap (kingdom +0x62) set by the cheat panel's «nopop» (0x424530),
    /// with the Supply Storage and Seafood completions of that moment:
    /// each later one adds its quarter of the game's limit (0x4d3819).
    #[serde(default)]
    pub population_cap: std::collections::BTreeMap<u8, [usize; 2]>,
    /// The player used a cheat (0x5cf818, set by 0x4240e0), as the save
    /// keeps it (0x443f1b).
    #[serde(default)]
    pub cheater: bool,
    /// The language the map's own texts are written in, and its recorded
    /// voices with them (Italian for FX's campaigns, whatever the
    /// installation); none when its texts are too few to tell (an
    /// addition of this version).
    #[serde(default)]
    pub written_in: Option<crate::lang::Language>,
    /// The cheat panel's global switches, off at every new or loaded game
    /// (0x43f700) and never saved, as the original's.
    #[serde(skip)]
    pub cheats: Cheats,
    pub records: usize,
    pub hidden: usize,
    /// Dialogue lines of the scenario (`.conv`), in file order.
    pub conversations: Vec<crate::dialogue::Conversation>,
    /// Why the conversations could not be read; the map still opens.
    pub conversation_error: Option<String>,
    /// What the message window shows: a story, a message or a phrase.
    #[serde(default)]
    pub message: Option<crate::script::Message>,
    /// The objectives the scripts have given (`AddQuest`).
    pub quests: Vec<crate::script::Quest>,
    /// Attitude of kingdom `row` toward kingdom `column` (`.diplomacy`, 8×8
    /// words): 0x40 ally, 0x20 neutral, 0x10 enemy, low bits for sharing.
    pub diplomacy: Vec<u32>,
    pub rules: [KingdomRules; 8],
    /// The names of the hero table taken since the last game ended
    /// (0x4999d0, `gameplay::take_hero`): by the random map screen, its
    /// generator and the promotions. The original frees them when a game
    /// ends, so a loaded game starts with none taken; this version saves
    /// them, so that a resumed game names its heroes as the uninterrupted
    /// one.
    #[serde(default)]
    pub heroes_taken: Vec<bool>,
    /// The map's sounds placed with the editor (`<map>.sounds`).
    #[serde(default)]
    pub sounds: Vec<SoundSource>,
}
/// A sound of the map, placed with the editor: no original map has one.
/// The map loader (0x440dba) reads records of 112 bytes; each gives its
/// sound to an object of the map or to a `sound_object` it makes for it,
/// and starts the object's sound timer (vt+0x1cc = 0x505580).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundSource {
    /// The object that sounds.
    pub object: usize,
    /// A group of `SOUNDS.INI` or a WAV (+0x4a, record +0x20).
    pub sound: String,
    /// 0 once, or every `period` seconds when it is not 0; 1 in a loop;
    /// 2 every R(`min`, `max`) seconds (+0x4e, record +0x60).
    pub mode: u8,
    /// The first sound waits as the next ones do (+0x4f, record +0x61).
    pub delayed: bool,
    /// Seconds (+0x50, +0x52, +0x54; record +0x62, +0x64, +0x66).
    pub period: u16,
    pub min: u16,
    pub max: u16,
}
/// The records of `<map>.sounds` (0x440dba): with byte +0x68 the sound goes
/// to the object of the map whose id is at +0x6e, otherwise to a new
/// `sound_object` of nobody on the cell at +0x6a, +0x6c (0x440e36).
pub fn sound_sources(data: &[u8], objects: &mut Vec<MapObject>) -> Vec<SoundSource> {
    let mut out = Vec::new();
    for r in data.as_chunks::<112>().0 {
        let w = |at: usize| u16::from_le_bytes([r[at], r[at + 1]]);
        let object = if r[0x68] != 0 {
            let id = w(0x6e);
            match objects.iter().position(|o| o.id == id) {
                Some(i) => i,
                None => continue,
            }
        } else {
            let id = objects
                .iter()
                .map(|o| o.id)
                .filter(|&id| id < 0xfc17)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let cell = [w(0x6a), w(0x6c)];
            let mut data = vec![0; 56];
            for (at, value) in [
                (0, 56),
                (2, class::SOUND_OBJECT as u16),
                (8, cell[0]),
                (10, cell[1]),
                (14, id),
            ] {
                data[at..at + 2].copy_from_slice(&value.to_le_bytes());
            }
            objects.push(MapObject {
                id,
                class: class::SOUND_OBJECT,
                flags: 0,
                x: cell[0],
                y: cell[1],
                owner: 0,
                civilization: 0,
                variant: 0,
                health: 0,
                max_health: 0,
                name: String::new(),
                subtitle: String::new(),
                portrait: String::new(),
                portrait_large: String::new(),
                custom_image: String::new(),
                data,
                mole: 0,
                control: [0; 8],
                sight: [0; 8],
            });
            objects.len() - 1
        };
        out.push(SoundSource {
            object,
            sound: string(&r[0x20..0x60]),
            mode: r[0x60],
            delayed: r[0x61] != 0,
            period: w(0x62),
            min: w(0x64),
            max: w(0x66),
        });
    }
    out
}
/// The two switches of the original's developer commands (handler
/// 0x424140), for every kingdom alike, computer ones included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cheats {
    /// «buildozer» (0x5cf8f4): a step of a building's queue ends at its
    /// first tick (0x41f113) and a peasant's blow finishes the site
    /// (0x4abc72).
    pub instant_build: bool,
    /// «notech» (0x5cf5a8): the scenario's forbidden lists, the research
    /// and the building a type depends on and a spell's research are not
    /// asked (0x494ba0, 0x494c90, 0x494ac0, 0x494d30, 0x60a850).
    pub no_tech: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KingdomRules {
    pub forbidden_buildings: Vec<String>,
    pub forbidden_units: Vec<String>,
    pub forbidden_researches: Vec<String>,
    pub forbidden_upgrades: Vec<String>,
    pub initial_researches: Vec<String>,
}
/// Eight kingdom lists: NUL-terminated names, with an empty name ending each list.
fn kingdom_lists(data: &[u8]) -> Result<[Vec<String>; 8]> {
    let mut lists = std::array::from_fn(|_| Vec::new());
    let mut offset = 0;
    for list in &mut lists {
        loop {
            let tail = data
                .get(offset..)
                .ok_or_else(|| invalid("Truncated kingdom rules"))?;
            let n = tail
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| invalid("Unterminated kingdom rule"))?;
            offset += n + 1;
            if n == 0 {
                break;
            }
            list.push(legacy_text(&tail[..n]).trim().to_ascii_lowercase());
        }
    }
    Ok(lists)
}
/// Section of the object INIs for a civilization code of the scenarios: 1
/// European, 2 Arab, 3 Asian (byte 13 of the object records; the mosque and
/// the magic workshop, Arab only, carry 2, the Shaolin monastery 3).
pub fn civ_section(code: u8) -> &'static str {
    match code {
        2 => "arab",
        3 => "asia",
        _ => "euro",
    }
}
/// Kingdom choices and rules chosen on the setup screen of the missions
/// (`S_SINGLESETUP`), applied when the scenario opens.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GameSetup {
    /// Map defaults disabled: new custom missions start with castle + 3 workers.
    #[serde(default)]
    pub custom_start: bool,
    pub kingdoms: Vec<KingdomChoice>,
    #[serde(default)]
    pub options: Option<GameRules>,
    /// None preserves the scenario's original resource quantities.
    #[serde(default)]
    pub resources: Option<[u32; 4]>,
    #[serde(default)]
    pub players: Option<usize>,
    #[serde(default)]
    pub human: Option<usize>,
    /// Resolved terrain for generated previews; old saves retain grass.
    #[serde(default)]
    pub random_biome: Option<String>,
    #[serde(default)]
    pub random_generation: Option<crate::random_map::Options>,
    /// «Resa dell'IA» of the random map screen (`AIRESIGN`, 0x5cf6d4):
    /// computer kingdoms may surrender. Kept for the computer players.
    #[serde(default)]
    pub ai_resign: bool,
    /// The names at the head of the hero table the menu took before the
    /// game, for the random map screen's computer kingdoms (0x4c7ae5).
    #[serde(default)]
    pub heroes_taken: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KingdomChoice {
    /// Index of the kingdom record (owner − 1).
    pub record: usize,
    /// Colour 1–8, in `COLOR BARS.BMP` order.
    pub color: u8,
    /// Team 1–4, or 0 for none (shown as the frames «1»–«4» and the empty
    /// one of `TEAMS.BMP`).
    pub team: u8,
    /// None preserves map civilizations; zero resolves a random civilization once.
    #[serde(default)]
    pub civilization: Option<u8>,
    /// The kingdom's name on the random map screen (0x4cab02); None keeps
    /// the scenario's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Behaviour script of a computer kingdom (`DATA\AI\…`), empty for the
    /// default; kept for the computer players.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ai: String,
}
/// Rules of the original setup dialog. Gameplay rules are retained while the
/// stationary simulation only consumes visibility, resources and population.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameRules {
    pub no_fog: bool,
    pub explored: bool,
    pub population: usize,
    pub difficulty: usize,
    pub game_type: usize,
    pub victory: usize,
    pub victory_limit: u32,
    pub resource_level: usize,
    /// «Resa dell'IA» (`AIRESIGN`, 0x5cf6d4): the computer kingdoms may
    /// resign, from the setup of the game.
    pub ai_resign: bool,
}
impl Default for GameRules {
    fn default() -> Self {
        Self {
            no_fog: false,
            explored: false,
            population: 200,
            difficulty: 1,
            game_type: 0,
            victory: 0,
            victory_limit: 30,
            resource_level: 3,
            ai_resign: false,
        }
    }
}
impl GameRules {
    /// 0x444890, the start of a campaign mission: a standard game and
    /// victory (`0x5cf808`, `0x5cf80a`), resources at 1 (`0x5877b4`) and a
    /// population of 200 (`0x5877b6`, each kingdom's `K+0x62` at 0x4403c0),
    /// whatever the map's header says; the scripts decide the rest. Fog,
    /// exploration and difficulty stay as they are.
    pub fn start_campaign(&mut self) {
        self.game_type = 0;
        self.victory = 0;
        self.resource_level = 1;
        self.population = 200;
    }
    pub fn from_header(header: &[u8]) -> Self {
        let byte = |at| header.get(at).copied().unwrap_or(1);
        // 0x4c6fa0, 0x4292ec: the fog is on with byte 0x4e at 0 (0x5877a4)
        // and the map unexplored with byte 0x4f at 0 (0x5cf6e0), as in every
        // map of the game.
        let set = |at| header.get(at).is_some_and(|&b| b != 0);
        Self {
            no_fog: set(0x4e),
            explored: set(0x4f),
            population: word(header, 0x128).unwrap_or(200) as usize,
            difficulty: if byte(0x50) < 3 {
                byte(0x50) as usize
            } else {
                0
            },
            game_type: if byte(0x126) < 3 {
                byte(0x126) as usize
            } else {
                0
            },
            victory: if byte(0x127) < 4 {
                byte(0x127) as usize
            } else {
                0
            },
            victory_limit: u32_at(header, if byte(0x127) == 2 { 0x12a } else { 0x12e })
                .unwrap_or(30),
            resource_level: 3,
            ai_resign: false,
        }
    }
}
/// Teams of the setup screen: `TEAMS.BMP` holds «none» and 1–4.
/// A gate's automatism (+0x15e) is on from its constructor (0x455da0) and
/// the map record does not carry it (0x455e20); ReTzar keeps it at byte 65.
pub fn automatic_gate(o: &mut MapObject) {
    o.data.resize(o.data.len().max(77), 0);
    o.data[65..69].copy_from_slice(&1u32.to_le_bytes());
}
pub const TEAMS: u8 = 4;
/// The team the setup screen shows for a kingdom record: the scenarios give
/// every kingdom its own team 1–8, beyond the four the screen offers.
pub fn shown_team(team: u8) -> u8 {
    if team <= TEAMS { team } else { 0 }
}
/// How one kingdom treats another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Relation {
    Ally,
    Neutral,
    Enemy,
}
impl MissionWorld {
    /// The attitude of owner `from` toward owner `to` (both 1–8).
    pub fn relation(&self, from: u8, to: u8) -> Option<Relation> {
        let (a, b) = (
            (from as usize).checked_sub(1)?,
            (to as usize).checked_sub(1)?,
        );
        if a >= 8 || b >= 8 {
            return None;
        }
        let v = *self.diplomacy.get(a * 8 + b)?;
        Some(if v & 0x40 != 0 {
            Relation::Ally
        } else if v & 0x20 != 0 {
            Relation::Neutral
        } else {
            Relation::Enemy
        })
    }
    /// Apply the setup screen: new colours, and alliances from the teams.
    /// As at the start of a game (0x440635), two active kingdoms of the same
    /// team (1–4) become allies sharing their view both ways; every other
    /// pair keeps the map's attitude, an enemy when it has none. Teams add
    /// alliances, they never break those of the map.
    pub fn apply_setup(&mut self, setup: &GameSetup) {
        if let Some(human) = setup
            .human
            .filter(|&i| self.kingdoms.get(i).is_some_and(|k| k.active))
        {
            // 0x4402c3: the kingdoms of a game set up on a screen take their
            // computer players from it, in full mode.
            for (i, k) in self.kingdoms.iter_mut().enumerate() {
                k.computer = i != human;
                k.ai_mode = if k.computer { 3 } else { 0 };
            }
        }
        if let Some(options) = &setup.options {
            self.game_rules = options.clone();
        }
        self.game_rules.ai_resign = setup.ai_resign;
        let n = setup.heroes_taken.max(self.heroes_taken.len());
        self.heroes_taken.resize(n, false);
        self.heroes_taken[..setup.heroes_taken].fill(true);
        if let Some(count) = setup.players {
            let human = self.player() as usize - 1;
            let mut remaining = count.clamp(1, 8) - 1;
            for (i, k) in self.kingdoms.iter_mut().enumerate() {
                if i == human || !k.active {
                    continue;
                }
                k.active = remaining > 0;
                remaining = remaining.saturating_sub(1);
            }
            self.objects.retain(|o| {
                o.owner == 0
                    || self
                        .kingdoms
                        .get(o.owner as usize - 1)
                        .is_none_or(|k| k.active)
            });
        }
        if let Some(resources) = setup.resources {
            for k in self.kingdoms.iter_mut().filter(|k| k.active) {
                k.resources = resources;
            }
        }
        for c in &setup.kingdoms {
            let Some(k) = self.kingdoms.get_mut(c.record) else {
                continue;
            };
            if (1..=8).contains(&c.color) {
                k.color = c.color;
            }
            // 0x4671b0: the kingdom goes by the row's name (+0xc, the one
            // of the messages and the scores), not by its record's leader.
            if let Some(name) = c.name.as_ref().filter(|n| !n.trim().is_empty()) {
                k.name = name.clone();
                k.leader.clear();
            }
            if let Some(civ) = c.civilization.filter(|v| (1..=3).contains(v)) {
                k.civilization = civ;
                for o in self.objects.iter_mut().filter(|o| {
                    o.owner as usize == c.record + 1 && (crate::selection::unit(o) || o.building())
                }) {
                    o.civilization = civ;
                    if let Some(byte) = o.data.get_mut(13) {
                        *byte = civ;
                    }
                }
            }
            if c.team <= TEAMS && c.team != shown_team(k.team) {
                k.team = c.team;
            }
            // The row's behaviour (0x4ca9c4); the default keeps the map's.
            if !c.ai.is_empty() {
                k.ai_script = c.ai.clone();
            }
        }
        if self.diplomacy.len() != 64 {
            self.diplomacy = vec![0x10; 64];
        }
        let team: [u8; 8] = std::array::from_fn(|i| {
            self.kingdoms
                .get(i)
                .filter(|k| k.active)
                .map_or(0, |k| shown_team(k.team))
        });
        for a in 0..8 {
            for b in (0..8).filter(|&b| b != a && team[a] != 0 && team[a] == team[b]) {
                self.diplomacy[a * 8 + b] = 0x41;
            }
        }
    }
    /// 0x440302: outside a game set up on a screen, the local kingdom keeps
    /// the computer player of its record only when byte 122 makes it a
    /// computer.
    pub fn apply_local_ai_rule(&mut self) {
        let local = self.player() as usize - 1;
        if let Some(k) = self.kingdoms.get_mut(local)
            && !k.computer
        {
            k.ai_mode = 0;
        }
    }
    /// What the loader does with the scenario's objects (0x4407a0–0x44096e):
    /// those of a kingdom 1–8 that does not take part are not created (the
    /// records kept for `AppearObj` stay); in King of the Hill the gold and
    /// stone mines are left out with many resources and never run dry
    /// otherwise.
    pub fn apply_loader_rules(&mut self) {
        let kingdoms = &self.kingdoms;
        self.objects.retain(|o| {
            o.flags & FLAG_INACTIVE != 0
                || !(1..=8).contains(&o.owner)
                || kingdoms.get(o.owner as usize - 1).is_some_and(|k| k.active)
        });
        // 0x4eb340, 0x4ea410: a monk or priest with an owner takes its
        // kingdom's level (K+0x114) over its record's. The kingdoms start
        // zeroed (0x467160) and the initial researches, Philosophy's 2
        // included, come after the objects (0x440767, 0x440ffb): the map's
        // monks and priests start at level 0, hidden ones too.
        for o in self
            .objects
            .iter_mut()
            .filter(|o| matches!(o.class, 12 | 18) && o.owner != 0)
        {
            if let Some(level) = o.data.get_mut(0x1c..0x1e) {
                level.fill(0);
            }
        }
        if self.game_rules.game_type == 1 {
            let mine = |o: &MapObject| {
                o.flags & FLAG_INACTIVE == 0
                    && matches!(o.class, class::RES_GOLD | class::RES_STONE)
            };
            if self.game_rules.resource_level == 2 {
                self.objects.retain(|o| !mine(o));
            } else {
                for o in self.objects.iter_mut().filter(|o| mine(o)) {
                    o.data.resize(o.data.len().max(64), 0);
                    o.data[0x3f] = 1;
                }
            }
        }
    }
    /// Deliberate corrections of installed maps, not done by the original.
    /// «Ponte vicino» (`maps/mp4`): stone tower #1100 at (155, 76) sits
    /// among Tzar 5's border towers (#1095–#1102, mirrored by #1095 at
    /// (152, 189)) but was saved with owner 10, so it showed grey and
    /// Tzar 5's own towers shot it; it goes back to kingdom 5.
    pub fn correct_map_errors(&mut self, map: &str) {
        if !map.eq_ignore_ascii_case("maps/mp4") {
            return;
        }
        for o in &mut self.objects {
            if o.id == 1100
                && o.class == class::TOWER_STONE
                && o.owner == 10
                && [o.x, o.y] == [155, 76]
            {
                o.owner = 5;
                o.data[12] = 5;
            }
        }
    }
    /// Civilization of a kingdom, as the `RACE.BMP` frame (European, Arab,
    /// Asian): the one most of its objects belong to.
    pub fn civilization(&self, owner: u8) -> usize {
        if let Some(k) = owner
            .checked_sub(1)
            .and_then(|i| self.kingdoms.get(i as usize))
            && (1..=3).contains(&k.civilization)
        {
            return k.civilization as usize - 1;
        }
        let mut count = [0usize; 4];
        for o in self.objects.iter().filter(|o| o.owner == owner) {
            count[(o.civilization as usize).min(3)] += 1;
        }
        // Object codes 1 European, 2 Arab, 3 Asian: the frame is code − 1.
        (1..=3).max_by_key(|&c| count[c]).unwrap_or(1) - 1
    }
    /// The player commands the object (0x506c30, vt+0x2a0): as its control
    /// byte says (+0xb0), else when its kingdom, or that of the spy it
    /// hides (vt+0xbc), is the player's or shares its units with them.
    pub fn commands(&self, o: &MapObject) -> bool {
        match o.control.get(usize::from(self.player()).wrapping_sub(1)) {
            Some(&hold) if hold != 0 => hold == 1,
            _ => self.controls(o.owner) || (o.mole != 0 && self.controls(o.mole)),
        }
    }
    /// 0x506c30 in full for object `i`: commanded ([`Self::commands`]),
    /// neither locked by a script (flag 0x100) nor frozen or stone (bits
    /// 0x40 and 0x4000 of the effects, +0x6e). The menus (0x4e3f10,
    /// 0x41dc00), the rectangle (0x468380) and the orders ask it.
    pub fn commands_at(&self, i: usize) -> bool {
        self.objects
            .get(i)
            .is_some_and(|o| self.commands(o) && o.flags & FLAG_LOCKED == 0)
            && !self
                .simulation
                .as_ref()
                .is_some_and(|s| crate::gameplay::still(s, i))
    }
    /// Whether the local player commands objects of `owner`: their own,
    /// those of a kingdom sharing its units with them (0x467d10, mask 4),
    /// and those of no kingdom, whose row of the relations is 0x47 toward
    /// everyone (0x4405da): neutral animals, mines, items, buildings.
    pub fn controls(&self, owner: u8) -> bool {
        owner == 0 || self.shares(owner, 4)
    }
    /// Whether `owner` is the player's kingdom or shares its vision with
    /// them (0x467d10, mask 1).
    pub fn shares_vision(&self, owner: u8) -> bool {
        self.shares(owner, 1)
    }
    fn shares(&self, owner: u8, mask: u32) -> bool {
        let player = self.player();
        owner == player
            || ((1..=8).contains(&owner)
                && (1..=8).contains(&player)
                && self
                    .diplomacy
                    .get((owner as usize - 1) * 8 + player as usize - 1)
                    .is_some_and(|d| d & mask != 0))
    }
    /// Owner number of the human player: the first active kingdom the
    /// computer does not play (the Gengis Khan campaign starts as kingdom 2).
    pub fn player(&self) -> u8 {
        self.kingdoms
            .iter()
            .position(|k| k.active && !k.computer)
            .or_else(|| self.kingdoms.iter().position(|k| k.active))
            .map_or(1, |i| i as u8 + 1)
    }
    /// Kingdoms that take part in the scenario, with their owner numbers.
    pub fn active_kingdoms(&self) -> impl Iterator<Item = (u8, &Kingdom)> {
        self.kingdoms
            .iter()
            .enumerate()
            .filter(|(_, k)| k.active)
            .map(|(i, k)| (i as u8 + 1, k))
    }
    /// Starting kingdom record of the human player, if the scenario has one.
    pub fn player_kingdom(&self) -> Option<&Kingdom> {
        self.kingdoms.get(self.player() as usize - 1)
    }
    /// Where the player's view opens: their castle, otherwise the object
    /// nearest to the median of everything they own.
    pub fn home(&self) -> Option<[u16; 2]> {
        let player = self.player();
        let own: Vec<&MapObject> = self
            .objects
            .iter()
            .filter(|o| o.owner == player && o.selectable())
            .collect();
        if let Some(castle) = own.iter().find(|o| o.class == class::CASTLE) {
            return Some([castle.x, castle.y]);
        }
        let median = |mut v: Vec<i32>| {
            v.sort_unstable();
            v.get(v.len() / 2).copied()
        };
        let mx = median(own.iter().map(|o| o.x as i32).collect())?;
        let my = median(own.iter().map(|o| o.y as i32).collect())?;
        own.iter()
            .min_by_key(|o| (o.x as i32 - mx).pow(2) + (o.y as i32 - my).pow(2))
            .map(|o| [o.x, o.y])
    }
    /// Colour slot (0–7) of an owner: its kingdom's colour field, 1-based.
    pub fn color_slot(&self, owner: u8) -> Option<usize> {
        if !(1..=8).contains(&owner) {
            return None;
        }
        let color = self
            .kingdoms
            .get(owner as usize - 1)
            .map(|k| k.color)
            .filter(|c| (1..=8).contains(c))
            .unwrap_or(owner);
        Some(color as usize - 1)
    }
    /// Kingdom name of an owner. Owners 1–8 are the kingdom records in order.
    /// Owner 0 holds resources, items and ground details; owner 10 holds
    /// walls, bridges, animals and hostile creatures, which the original
    /// strings call «Natura» (`[misc] nature3`).
    pub fn read(c: &Container, grid: &MapGrid) -> Result<Self> {
        let mut result = Self::read_original(c, grid)?;
        result.written_in = crate::lang::mission_language(&result);
        // Names, objectives, stories and conversations in the language of
        // the moment (an addition of this version), whatever the language
        // of the map: the Italian edition's, the American release's. A
        // text already in that language stays.
        crate::lang::localize_mission(&mut result, crate::lang::current());
        Ok(result)
    }
    /// [`Self::read`] with the texts as the map writes them.
    pub fn read_original(c: &Container, grid: &MapGrid) -> Result<Self> {
        let header = c.get(&format!("{}.header", grid.name))?;
        let data = c.get(&format!("{}.objects", grid.name))?;
        let mut result = Self {
            game_rules: GameRules::from_header(&header),
            ..Self::default()
        };
        for (suffix, field) in [
            ("forbidden.buildings", 0),
            ("forbidden.units", 1),
            ("forbidden.researches", 2),
            ("forbidden.upgrades", 3),
            ("initial.researches", 4),
        ] {
            let name = format!("{}.{suffix}", grid.name);
            if let Ok(data) = c.get(&name) {
                for (rules, list) in result.rules.iter_mut().zip(kingdom_lists(&data)?) {
                    *match field {
                        0 => &mut rules.forbidden_buildings,
                        1 => &mut rules.forbidden_units,
                        2 => &mut rules.forbidden_researches,
                        3 => &mut rules.forbidden_upgrades,
                        _ => &mut rules.initial_researches,
                    } = list;
                }
            }
        }
        let mut at = 0;
        const BASE: [usize; 7] = [0, 0, 1, 3, 39, 44, 74];
        while at < data.len() {
            let code = u32_at(&data, at + 1)?;
            let len = u32_at(&data, at + 5)? as usize;
            if len < 56 || len > 65536 {
                return Err(invalid("Invalid mission object size"));
            }
            let p = data
                .get(at + 9..at + 9 + len)
                .ok_or_else(|| invalid("Truncated mission object record"))?;
            let group = (code >> 16) as usize;
            let class = BASE
                .get(group)
                .ok_or_else(|| invalid("Unknown original object group"))?
                + (code & 65535) as usize;
            let stored_class = word(p, 2)? as usize;
            if class >= CLASSES.len()
                || stored_class != class::stored_base(class)
                || word(p, 0)? as usize > len
            {
                return Err(invalid("Mission object class/size mismatch"));
            }
            let flags = u32_at(p, 4)?;
            // 0x4408a4: owner 0xff stands for nobody.
            let owner = if p[12] == 0xff { 0 } else { p[12] };
            result.objects.push(MapObject {
                id: word(p, 14)?,
                class,
                flags,
                x: word(p, 8)?,
                y: word(p, 10)?,
                owner,
                civilization: p[13],
                variant: word(p, 16)?,
                health: word(p, 18)?,
                max_health: word(p, 20)?,
                name: String::new(),
                subtitle: String::new(),
                portrait: String::new(),
                portrait_large: String::new(),
                // A 32-byte field: names such as `go_water_cliff_20` are longer
                // than 16 characters, and old text may follow the terminator.
                custom_image: if class::has_custom_image(class) && len > 56 {
                    string(&p[56..len.min(88)])
                } else {
                    String::new()
                },
                data: p.to_vec(),
                mole: 0,
                control: [0; 8],
                sight: [0; 8],
            });
            result.objects.last_mut().unwrap().data[12] = owner;
            if matches!(class, class::GATE_WOOD | class::GATE_STONE) {
                automatic_gate(result.objects.last_mut().unwrap());
            }
            if flags & FLAG_INACTIVE != 0 {
                result.hidden += 1;
            }
            at += 9 + len;
        }
        result.records = result.objects.len();
        if result.records != u32_at(&header, 8)? as usize {
            return Err(invalid("Mission object count mismatch"));
        }
        if let Ok(ext) = c.get(&format!("{}.objext", grid.name)) {
            if ext.len() % 258 != 0 {
                return Err(invalid("Invalid hero extension records"));
            }
            let mut by_id = std::collections::HashMap::new();
            for (i, o) in result.objects.iter().enumerate() {
                by_id.entry(o.id).or_insert(i);
            }
            for p in ext.chunks_exact(258) {
                if let Some(&i) = by_id.get(&word(p, 0)?) {
                    result.objects[i].name = string(&p[2..34]);
                    result.objects[i].subtitle = string(&p[34..66]);
                    result.objects[i].portrait = string(&p[66..130]);
                    result.objects[i].portrait_large = string(&p[130..194]);
                }
            }
        }
        if let Ok(d) = c.get(&format!("{}.sounds", grid.name)) {
            result.sounds = sound_sources(&d, &mut result.objects);
            result.records = result.objects.len();
        }
        if let Ok(d) = c.get(&format!("{}.diplomacy", grid.name))
            && d.len() == 256
        {
            result.diplomacy = d
                .chunks_exact(4)
                .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                .collect();
        }
        // 0x4b70b0 reads as many records as the header counts; a damaged
        // script must not keep the map from opening.
        let entry = |suffix: &str| {
            c.get(&format!("{}.{suffix}", grid.name))
                .unwrap_or_default()
        };
        if let Ok(script) = crate::script::Script::parse(
            crate::script::counts(&header),
            &entry("events"),
            &entry("actions"),
            &entry("rules"),
        ) {
            result.script = script;
            result.script.defeat = crate::script::defeat_messages(&entry("defeat.messages"));
        }
        if let Ok(conv) = c.get(&format!("{}.conv", grid.name)) {
            match crate::dialogue::parse(&conv) {
                Ok(list) => result.conversations = list,
                Err(e) => result.conversation_error = Some(e.to_string()),
            }
        }
        if let Ok(k) = c.get(&format!("{}.kingdominfo", grid.name)) {
            if k.len() != 8 * 152 {
                return Err(invalid("Invalid kingdom records"));
            }
            for p in k.chunks_exact(152) {
                let start = [word(p, 84)?, word(p, 86)?];
                let index = result.kingdoms.len();
                if index < 8 && start.iter().all(|&v| (v as i16) > 0) {
                    result.starting_positions[index] = Some(start);
                }
                result.kingdoms.push(Kingdom {
                    name: string(&p[..32]),
                    leader: string(&p[32..64]),
                    active: p[64] != 0,
                    color: p[66],
                    team: p[67],
                    civilization: p[88],
                    resources: [
                        u32_at(p, 68)?,
                        u32_at(p, 72)?,
                        u32_at(p, 76)?,
                        u32_at(p, 80)?,
                    ],
                    computer: p[122] != 0,
                    ai_mode: if p[122] == 2 { 0 } else { p[121] },
                    ai_script: string(&p[89..121]),
                });
            }
        }
        Ok(result)
    }
}

/// What a visual draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// The object itself; the only selectable layer.
    Body,
    /// Banners and flags in the owner's colour, above the body.
    Overlay,
    /// The object's shadow, immediately below its own body.
    Shadow,
    /// Birds in flight, above everything on the ground.
    Air,
}
#[derive(Clone, Copy, Debug)]
pub struct ObjectVisual {
    pub object: usize,
    pub image: usize,
    pub top_left: [f32; 2],
    pub bounds: [f32; 4],
    pub footprint: [f32; 4],
    pub ground: bool,
    pub animation: Option<usize>,
    /// Where this copy starts in its animation, so that equal objects do
    /// not move in step.
    pub phase_ms: u64,
    /// The row a new picture starts at (`startrow`, 0x5017f0).
    pub start_row: usize,
    pub layer: Layer,
    /// A tree or its shadow, which the transparent-trees key makes see-through.
    pub tree: bool,
}
pub struct SpriteAnimation {
    pub frames: Vec<usize>,
    pub timeline: crate::animation::Timeline,
}
/// A seagull or an eagle: it flies to random points of the map as the
/// original `eagle` class does, one pixel per step in eight directions,
/// turning through the in-between columns of its 24-column sheet.
#[derive(Clone, Debug)]
pub struct Flight {
    /// Indices into [`VisualScene::visuals`].
    pub body: usize,
    pub shadow: Option<usize>,
    /// Image ids by `[column][row]`.
    pub frames: Vec<Vec<usize>>,
    pub shadow_frames: Vec<Vec<usize>>,
    /// Milliseconds per one-pixel step (`(32000 / move_len) >> 5`).
    pub step_ms: u64,
    /// Steps between two frames of the wing cycle.
    pub steps_per_frame: u32,
    /// The object's point (world pixels, y up) and the picture's offset from it.
    pub start: [f32; 2],
    pub offset: [f32; 2],
    /// Destinations stay ten cells away from every edge: `[min, max]` pixels.
    pub range: [f32; 2],
    pub column: usize,
    pub seed: u32,
    /// Where the shadow falls from the body ([`shadow_lift`]).
    pub shadow_lift: [f32; 2],
}
/// How far a flier's shadow falls from its body, in screen pixels (+0x120,
/// +0x122): bats and carpets 20 right and 35 down (0x4e76f0), dragons and
/// eagles 40 and 70 (0x4ee95d, 0x4ed93d).
pub fn shadow_lift(class: usize) -> [f32; 2] {
    match class {
        15 | 22 => [20., 35.],
        32 | class::EAGLE => [40., 70.],
        _ => [0., 0.],
    }
}
/// The cells a unit stands on (+0x1a, +0x1c), as its constructor sets them:
/// 2×2 for the trader (0x4ed490), the catapults and the ballista (their base
/// 0x4e7480), the galleon, the hulk and the xebec (0x4ea8e0, 0x4eab50,
/// 0x4ea9b0) and the dragon (0x4ee910); one cell for the others (0x4ffb30).
pub fn unit_cells(class: usize) -> [usize; 2] {
    match class {
        2 | 19 | 20 | 21 | 23 | 24 | 25 | 32 => [2, 2],
        _ => [1, 1],
    }
}
/// The shadow of a dying flier at row `row` of its death (0x4e7ae0): each
/// new frame takes off the offset divided by the frames left, so it lies
/// under the body on the last.
pub fn descent(lift: [f32; 2], rows: usize, row: usize) -> [f32; 2] {
    let mut off = lift.map(|n| n as i32);
    for r in 1..=row.min(rows.saturating_sub(1)) {
        let left = (rows - r) as i32;
        off = off.map(|n| n - n / left);
    }
    off.map(|n| n as f32)
}
pub struct VisualScene {
    pub images: Vec<Bitmap>,
    pub visuals: Vec<ObjectVisual>,
    pub missing: Vec<String>,
    pub animations: Vec<SpriteAnimation>,
    pub flights: Vec<Flight>,
}

/// Movement of the eight directions in screen pixels (y down), from the
/// original table at 0x591000: south, south-east, east, … south-west. A
/// 24-column sheet draws direction `d` in column `3 d`.
pub const FLIGHT_STEPS: [[i32; 2]; 8] = [
    [0, 1],
    [1, 1],
    [1, 0],
    [1, -1],
    [0, -1],
    [-1, -1],
    [-1, 0],
    [-1, 1],
];

/// Offsets of the two gate flags (`gates c`, 20×20 cells) from the gate
/// picture's top-left corner, by flag column and row (Tzar.exe 0x589770).
/// Rows 0–6 follow the left flag through the bridge frames, rows 8–14 the
/// right one; rows 7 and 15 belong to the ruined gate.
const GATE_FLAGS: [[[i16; 2]; 16]; 8] = {
    const fn column(
        left: [[i16; 2]; 7],
        ruin: [i16; 2],
        right: [i16; 2],
        last: [i16; 2],
    ) -> [[i16; 2]; 16] {
        let mut out = [[0; 2]; 16];
        let mut i = 0;
        while i < 7 {
            out[i] = left[i];
            out[8 + i] = right;
            i += 1;
        }
        out[7] = ruin;
        out[15] = last;
        out
    }
    const fn same(p: [i16; 2]) -> [[i16; 2]; 7] {
        [p; 7]
    }
    [
        column(same([58, 67]), [62, 51], [212, 67], [212, 67]),
        column(same([91, 41]), [0, 0], [175, 41], [136, 126]),
        column(same([61, 34]), [65, 27], [211, 35], [210, 38]),
        column(same([90, 8]), [130, 107], [178, 8], [0, 0]),
        column(
            [
                [68, 24],
                [68, 23],
                [68, 23],
                [68, 23],
                [68, 23],
                [68, 23],
                [68, 23],
            ],
            [76, 19],
            [66, 150],
            [64, 143],
        ),
        column(same([70, 61]), [68, 121], [70, 150], [66, 151]),
        column(same([170, 22]), [180, 29], [170, 147], [160, 135]),
        column(same([168, 60]), [172, 118], [168, 151], [417, 150]),
    ]
};

/// A repeatable pseudo-random number for one object, standing in for the
/// original's per-game random draws (resource rocks, fish, bird routes).
pub fn object_noise(id: u16, salt: u32) -> u32 {
    let mut x = (id as u32) ^ salt.wrapping_mul(0x9E37_79B9);
    x = (x ^ (x >> 16)).wrapping_mul(0x7FEB_352D);
    x = (x ^ (x >> 15)).wrapping_mul(0x846C_A68B);
    x ^ (x >> 16)
}
pub struct ObjectLibrary {
    pub icons: Ini,
    pub sounds: Ini,
    pub attributes: crate::simulation::AttributeCatalog,
    pub images: Ini,
    pub locale: Ini,
    pub objects: BTreeMap<String, Ini>,
    /// Tree images at or below this index are trunks, never chopped: the
    /// count of the first `[treename]` entry of the terrain INI (0x5cfc48).
    pub trunks: u8,
    /// The count of the second `[treename]` entry (0x5cfc4a): a gate may
    /// stand on those trees too (0x456c7e).
    pub shrubs: u8,
    /// Cell masks of the pictures with a footprint, by image section.
    pub masks: BTreeMap<String, Vec<u8>>,
    /// The cells of a building picture whose `object` gives none on a side,
    /// measured on a frame of its sheet (0x498d34), by image section in
    /// lower case.
    pub frames: BTreeMap<String, [usize; 2]>,
    /// `cell_footprint` already worked out, by everything it reads.
    pub(crate) footprints: std::sync::Mutex<std::collections::HashMap<FootprintKey, [usize; 2]>>,
}
/// What `cell_footprint` reads of an object without a picture of its own:
/// the picture's choice (class, civilization, variant, the payload bytes 56
/// and 57 of walls, bridges and gates, the health of walls and fields).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FootprintKey {
    class: usize,
    civilization: u8,
    variant: u16,
    bytes: [Option<u8>; 2],
    hurt: bool,
    whole: bool,
}
impl FootprintKey {
    pub(crate) fn of(o: &MapObject) -> Option<Self> {
        o.custom_image.is_empty().then(|| Self {
            class: o.class,
            civilization: o.civilization,
            variant: o.variant,
            bytes: [o.data.get(56).copied(), o.data.get(57).copied()],
            hurt: o.health < o.max_health,
            whole: o.max_health > 0,
        })
    }
}
impl ObjectLibrary {
    pub fn load(a: &GameAssets, biome: &str) -> Result<Self> {
        let mut images = Ini::parse(&a.get("DATA/IMAGES.INI")?);
        if a.contains("IMAGES/TERRAIN/COMMON.IMAGES.INI") {
            images.merge(Ini::parse(&a.get("IMAGES/TERRAIN/COMMON.IMAGES.INI")?));
        }
        let biome_images = format!("IMAGES/TERRAIN/{biome}.IMAGES.INI");
        if a.contains(&biome_images) {
            images.merge(Ini::parse(&a.get(&biome_images)?));
        }
        let locale = Ini::locale(a)?;
        let mut objects = BTreeMap::new();
        for n in a.names().iter().filter(|n| {
            n.starts_with("DATA/OBJECTS/") && n.ends_with(".INI") && !n[13..].contains('/')
        }) {
            let ini = Ini::parse(&a.get(n)?);
            if let Some(class) = ini.get("general", "class") {
                // Class names compare without case (CRUSADE_ANCHOR.INI says
                // `CrusadeAnchor`).
                objects.insert(class.trim().to_ascii_lowercase(), ini);
            }
        }
        let tree_counts: Vec<Option<u8>> = a
            .get(&format!("IMAGES/TERRAIN/{biome}.TERRAIN.INI"))
            .ok()
            .map(|d| {
                Ini::parse(&d)
                    .entries("treename")
                    .iter()
                    .map(|(_, n)| n.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        let trunks = tree_counts.first().copied().flatten().unwrap_or(3);
        let shrubs = tree_counts.get(1).copied().flatten().unwrap_or(0);
        // 0x497ed0: beside a picture with a footprint, `<picture>.dat` holds
        // its pixel rect (8 bytes) and a byte per cell; a cell with bit 2
        // stays free when the object occupies the map (0x41de21).
        let mut masks = BTreeMap::new();
        for section in images.section_names() {
            let Some(file) = images.get(section, "image") else {
                continue;
            };
            let file = file.to_ascii_lowercase();
            if images.get(section, "object").is_none()
                || !(file.starts_with("building\\") || file.contains("\\scenery\\"))
            {
                continue;
            }
            let Some(stem) = file.strip_suffix(".bmp") else {
                continue;
            };
            let Ok(dat) = a.get(&format!("IMAGES/{}.dat", stem.replace('\\', "/"))) else {
                continue;
            };
            let rect = images.numbers(section, "object");
            let [w, h] =
                [2, 3].map(|k| (rect.get(k).copied().unwrap_or(0).max(0) as usize / 32).max(1));
            if dat.len() >= 8 && dat[..8].iter().any(|&b| b != 0) {
                let cells = dat[8..].iter().copied().take(w * h).collect();
                masks.insert(section.to_owned(), cells);
            }
        }
        // 0x498373, 0x498d34: a side of `object` under 32 pixels (or none)
        // is measured on a frame of the sheet, less the object's corner, to
        // the nearest cell and never under one.
        let mut frames = BTreeMap::new();
        for section in images.section_names() {
            let Some(file) = images.get(section, "image") else {
                continue;
            };
            let file = file.trim().to_ascii_lowercase();
            let rect = images.numbers(section, "object");
            let side = |k: usize| rect.get(k).copied().unwrap_or(0).max(0) / 32;
            if !file.starts_with("building\\")
                || !file.ends_with(".bmp")
                || (side(2) > 0 && side(3) > 0)
            {
                continue;
            }
            let path = format!("IMAGES/{}", file.replace('\\', "/"));
            let frame = if let Ok(data) = a.get(&path) {
                if data.len() < 26 || &data[..2] != b"BM" {
                    continue;
                }
                let grid = |k: &str| {
                    images
                        .get(section, k)
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(1)
                        .max(1)
                };
                let size =
                    [18, 22].map(|at| i32::from_le_bytes(data[at..at + 4].try_into().unwrap()));
                [
                    size[0].unsigned_abs() as usize / grid("cols"),
                    size[1].unsigned_abs() as usize / grid("rows"),
                ]
            } else {
                // The sheets packed as RLE sprites keep the `.bmp` name.
                let rle = format!("{}.rle", path.trim_end_matches(".bmp"));
                match a
                    .get(&rle)
                    .ok()
                    .and_then(|d| crate::sprite::SpriteSheet::frame_size(&d))
                {
                    Some(size) => size,
                    None => continue,
                }
            };
            let cells = [0, 1].map(|k| {
                let n = frame[k] as i32 - rect.get(k).copied().unwrap_or(0);
                (n / 32 + i32::from(n % 32 > 15)).max(1) as usize
            });
            frames.insert(section.to_ascii_lowercase(), cells);
        }
        Ok(Self {
            icons: Ini::parse(&a.get("DATA/ICONS.INI")?),
            sounds: Ini::parse(&a.get("DATA/SOUNDS.INI")?),
            attributes: crate::simulation::AttributeCatalog::load(a)?,
            images,
            locale,
            objects,
            trunks,
            shrubs,
            masks,
            frames,
            footprints: Default::default(),
        })
    }
    /// The layer an object moves in, INI `teren` (0x4ffd0a): 0 air, 1 land
    /// (the default), 2 water.
    pub fn layer(&self, o: &MapObject) -> usize {
        self.objects
            .get(CLASSES[o.class])
            .and_then(|d| d.get("general", "teren"))
            .and_then(|s| s.trim().parse().ok())
            .filter(|&n: &usize| n <= 2)
            .unwrap_or(1)
    }
    /// Whether the cell (dx, dy) of the object's footprint is left free by
    /// its picture's mask.
    /// 0x41ddc0 reads the object's own picture (`+0xe8`, the map's or the
    /// class's), which a building site keeps while it shows `build<W>x<H>`
    /// (0x41d8f0): a site already holds the finished building's cells.
    pub fn free_cell(&self, o: &MapObject, dx: usize, dy: usize) -> bool {
        let [w, _] = self.cell_footprint(o);
        self.final_image(o)
            .and_then(|s| self.masks.get(&s.to_ascii_lowercase()))
            .and_then(|m| m.get(dy * w + dx))
            .is_some_and(|&b| b & 2 != 0)
    }
    /// `free_cell` for the picture drawn now, which clicks follow: a
    /// building site shows a solid `build<W>x<H>`.
    pub fn free_drawn_cell(&self, o: &MapObject, dx: usize, dy: usize) -> bool {
        self.site_picture(o).is_none() && self.free_cell(o, dx, dy)
    }
    pub fn label(&self, o: &MapObject) -> String {
        if !o.name.is_empty() {
            return format!(
                "{}{}",
                o.name,
                if o.subtitle.is_empty() {
                    String::new()
                } else {
                    format!(" {}", o.subtitle)
                }
            );
        }
        self.class_label(o)
    }
    /// The class name alone, even for heroes with their own name.
    pub fn class_label(&self, o: &MapObject) -> String {
        let class = CLASSES[o.class];
        let suffix = match o.civilization {
            2 => "r",
            3 => "a",
            _ => "e",
        };
        for key in [
            format!("{class} {suffix}"),
            class.to_owned(),
            format!("{class} e"),
        ] {
            if self.locale.get("objects", &key).is_some() {
                return self.locale.label("objects", &key);
            }
        }
        class.to_owned()
    }
    pub fn image_for(&self, o: &MapObject) -> Option<String> {
        // 0x41d8f0: a site shows `build<W>x<H>` (the harbour its own),
        // `build3x3` when the size has none.
        self.site_picture(o).or_else(|| self.final_image(o))
    }
    /// The finished object's picture, which also gives its footprint.
    pub fn final_image(&self, o: &MapObject) -> Option<String> {
        if [class::WALL_STONE, class::WALL_WOOD].contains(&o.class) {
            let kind = if o.class == class::WALL_STONE {
                "stone"
            } else {
                "wood"
            };
            let mask = word(&o.data, 56).ok()?;
            return Some(format!(
                "walls_{kind}_{}{}",
                if mask >= 24 {
                    "vert"
                } else if mask >= 16 {
                    "horz"
                } else {
                    "tower"
                },
                if o.wall_damaged() { "_dest" } else { "" }
            ));
        }
        if o.class == class::BRIDGE {
            return Some(
                if o.data.get(56).copied().unwrap_or(0) == 0 {
                    "bridge_horz"
                } else {
                    "bridge_vert"
                }
                .to_owned(),
            );
        }
        if o.class == class::DUMMY {
            // 0x448270: `dummy_%d` of the variant drawn at loading; there is
            // no `dummy_0`, so variant 0 keeps the INI's `dummy_1`.
            let name = format!("dummy_{}", o.variant.max(1));
            if self.images.get(&name, "image").is_some() {
                return Some(name);
            }
        }
        // 0x44c880: a field short of full health shows its growth stage.
        if o.class == class::FIELD
            && o.health < o.max_health
            && self.images.get("field_grow", "image").is_some()
        {
            return Some("field_grow".into());
        }
        if !o.custom_image.is_empty() && self.images.get(&o.custom_image, "image").is_some() {
            return Some(o.custom_image.to_owned());
        }
        let def = self.objects.get(CLASSES[o.class])?;
        let civ = civ_section(o.civilization);
        let property = |key| {
            def.get(civ, key)
                .or_else(|| def.get("comm", key))
                .or_else(|| def.get("euro", key))
                .or_else(|| def.get("asia", key))
                .or_else(|| def.get("arab", key))
        };
        property("image_idle")
            .or_else(|| property("image"))
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }
    /// The picture of a building site (0x41d8f0).
    pub fn site_picture(&self, o: &MapObject) -> Option<String> {
        if !o.building() || o.flags & FLAG_CONSTRUCTION == 0 {
            return None;
        }
        let [w, h] = self.cell_footprint(o);
        let name = if o.class == class::HARBOR {
            "build harbor".to_owned()
        } else if matches!(o.class, class::WALL_STONE | class::WALL_WOOD) {
            // 0x4fbce0: a wall's own site, of its material.
            if o.class == class::WALL_STONE {
                "walls_stone_build"
            } else {
                "walls_wood_build"
            }
            .to_owned()
        } else if o.class == class::BRIDGE {
            // 0x41c800: a bridge's own site, of its way (+0x144).
            if o.data.get(56).copied().unwrap_or(0) == 0 {
                "bridge_horz_build"
            } else {
                "bridge_vert_build"
            }
            .to_owned()
        } else if matches!(o.class, class::GATE_STONE | class::GATE_WOOD) {
            // 0x456e91: `g%c%d_build`, material and way as its picture.
            format!(
                "g{}{}_build",
                if o.class == class::GATE_STONE {
                    's'
                } else {
                    'w'
                },
                o.byte(0x38) & 3
            )
        } else if o.class == class::DUMMY {
            // 0x4481a0: the dummy's own site, by the variant of its
            // `dummy_%d` (+0x144).
            if o.variant < 2 {
                "dummy1_build"
            } else {
                "dummy2_build"
            }
            .to_owned()
        } else {
            format!("build{w}x{h}")
        };
        [name, "build3x3".to_owned()]
            .into_iter()
            .find(|s| self.images.get(s, "image").is_some())
    }
    pub fn icon_for<'a>(&'a self, o: &'a MapObject) -> Option<&'a str> {
        if o.item() {
            // ITEM_MAGIC's native virtual icon getter (0x464320) resolves
            // the contained spell through 0x4722c0, independently of the
            // book's picture on the ground.
            if o.class == class::ITEM_MAGIC {
                return o
                    .spell_key()
                    .filter(|key| self.icons.get(key, "image").is_some());
            }
            if self.icons.get(&o.custom_image, "image").is_some() {
                return Some(&o.custom_image);
            }
            if o.class == class::ITEM_SCROLL {
                return Some("item_scroll");
            }
            // 0x464a10 looks up the current picture name. An unknown
            // custom weapon/potion must not silently become sword/potion 1.
            if !o.custom_image.is_empty() {
                return None;
            }
            if let Some(image) = self.property(o, "image") {
                return Some(image);
            }
        }
        let def = self.objects.get(CLASSES[o.class])?;
        let civ = civ_section(o.civilization);
        def.get(civ, "icon")
            .or_else(|| def.get("comm", "icon"))
            .or_else(|| def.get("euro", "icon"))
    }
    pub fn sounds_for(&self, o: &MapObject, property: &str) -> Vec<(String, u32)> {
        let key = self.property(o, property);
        let Some(key) = key else { return vec![] };
        self.sound_group(key)
    }
    /// Work and notification sounds belong to global groups, not unit voices.
    pub fn sound_group(&self, key: &str) -> Vec<(String, u32)> {
        let path = |name: &str| format!("SOUND/{}", name.replace('\\', "/"));
        if key.to_ascii_lowercase().ends_with(".wav") {
            return vec![(path(key), 1000)];
        };
        self.sounds
            .entries(key)
            .iter()
            .filter_map(|(name, weight)| {
                let weight = weight.split(';').next()?.trim().parse::<u32>().ok()?;
                (weight > 0).then(|| (path(name), weight))
            })
            .collect()
    }
    pub fn item_description(&self, o: &MapObject) -> String {
        if o.class == class::ITEM_SCROLL {
            return legacy_text(
                o.data
                    .get(96..)
                    .unwrap_or(&[])
                    .split(|b| *b == 0)
                    .next()
                    .unwrap_or(&[]),
            );
        }
        if o.class == class::ITEM_MAGIC {
            if let Some(name) = o.spell_key() {
                return self.locale.label("spell_names", name);
            }
        }
        if let Some(name) = self.locale.get("images", &o.custom_image) {
            return name.to_owned();
        }
        self.class_label(o)
    }
    /// Every picture of a mission, with the animation rules of the original
    /// classes. `side` is the map edge in cells (bird routes stay inside it).
    pub fn build(
        &self,
        a: &GameAssets,
        mission: &MissionWorld,
        side: usize,
    ) -> Result<VisualScene> {
        self.build_indices(a, mission, side, None)
    }
    /// A picture of IMAGES.INI as separate frames for the effects layer:
    /// the frames row by row, its columns and rows, its own frame rate
    /// (`fps`, 0 when absent) and the point placed on the effect's position,
    /// the hotspot if any, else the `object` offset (0x4347ac).
    pub fn effect_picture(&self, a: &GameAssets, section: &str) -> Result<EffectPicture> {
        let mut b = Builder {
            lib: self,
            assets: a,
            side: 1,
            out: VisualScene {
                images: Vec::new(),
                visuals: Vec::new(),
                missing: Vec::new(),
                animations: Vec::new(),
                flights: Vec::new(),
            },
            sheets: BTreeMap::new(),
            shared: Default::default(),
            frames: BTreeMap::new(),
            animations: Default::default(),
        };
        let key = b.sheet(section, None)?;
        let sheet = &b.sheets[&key];
        let mut frames = Vec::with_capacity(sheet.columns * sheet.rows);
        for row in 0..sheet.rows {
            for col in 0..sheet.columns {
                frames.push(sheet.frame(col, row)?);
            }
        }
        let number = |key: &str| self.images.numbers(section, key);
        let hot = number("hotspot");
        let object = number("object");
        let anchor = if hot.len() >= 2 {
            [hot[0], hot[1]]
        } else if object.len() >= 2 {
            [object[0], object[1]]
        } else {
            [0, 0]
        };
        Ok(EffectPicture {
            columns: sheet.columns,
            rows: sheet.rows,
            frames,
            fps: number("fps").first().copied().unwrap_or(0).max(0) as u32,
            anchor,
        })
    }
    pub fn build_indices(
        &self,
        a: &GameAssets,
        mission: &MissionWorld,
        side: usize,
        indices: Option<&std::collections::BTreeSet<usize>>,
    ) -> Result<VisualScene> {
        let mut b = Builder {
            lib: self,
            assets: a,
            side,
            out: VisualScene {
                images: Vec::new(),
                visuals: Vec::new(),
                missing: Vec::new(),
                animations: Vec::new(),
                flights: Vec::new(),
            },
            sheets: BTreeMap::new(),
            shared: Default::default(),
            frames: BTreeMap::new(),
            animations: Default::default(),
        };
        for (index, o) in mission.objects.iter().enumerate() {
            if indices.is_some_and(|set| !set.contains(&index)) {
                continue;
            }
            if o.flags & (FLAG_INACTIVE | 4) != 0
                || mission
                    .simulation
                    .as_ref()
                    .is_some_and(|s| s.objects.get(index).is_some_and(|x| x.holder.is_some()))
            {
                continue;
            }
            let slot = mission.color_slot(o.owner);
            match o.class {
                class::FISHPATCH => {
                    let nets = crate::gameplay::fishing_nets(mission.simulation.as_ref(), index);
                    b.fish(index, o, &nets)?;
                    continue;
                }
                // A gate under construction is a site like the others
                // (0x456e91).
                class::GATE_WOOD | class::GATE_STONE if o.flags & FLAG_CONSTRUCTION == 0 => {
                    let row =
                        crate::gameplay::drawbridge_row(mission.simulation.as_ref(), index, o);
                    b.gate(index, o, slot, row)?;
                    continue;
                }
                c if class::is_marker(c) => continue,
                _ => {}
            }
            let mut drawn = o.clone();
            if let Some(disguise) = mission
                .simulation
                .as_ref()
                .and_then(|s| s.gameplay.actors.get(index))
                .and_then(|a| a.disguise.as_ref())
            {
                drawn.class = disguise.class;
                drawn.civilization = disguise.civilization;
                drawn.owner = disguise.owner;
            }
            let appearance = drawn.clone();
            let o = &appearance;
            let slot = mission.color_slot(drawn.owner);
            let pose = mission
                .simulation
                .as_ref()
                .and_then(|s| s.objects.get(index))
                .map(|s| s.order);
            let direction = mission
                .simulation
                .as_ref()
                .and_then(|s| s.gameplay.actors.get(index))
                .map(|a| a.direction);
            let Some(mut section) = self.runtime_image(o, pose) else {
                b.out.missing.push(format!(
                    "{} #{}: immagine non definita",
                    CLASSES[o.class], o.id
                ));
                continue;
            };
            if matches!(
                pose,
                Some(crate::simulation::OrderState::Moving | crate::simulation::OrderState::Idle)
            ) {
                if let Some(image) = mission
                    .simulation
                    .as_ref()
                    .and_then(|s| s.gameplay.actors.get(index))
                    .and_then(|a| self.carrying_image(o, a.cargo, a.cargo_source))
                {
                    section = image;
                }
            }
            // A creature coming out shows `spawn` or `skeleton_rise` (op 0x74).
            if pose == Some(crate::simulation::OrderState::Appearing)
                && let Some(image) = mission
                    .simulation
                    .as_ref()
                    .and_then(|s| s.gameplay.actors.get(index))
                    .map(|a| a.action_image.as_str())
                    .filter(|s| !s.is_empty())
                && self.images.get(image, "image").is_some()
            {
                section = image.to_owned();
            }
            // An idle cow shows its grazing (0x4ecb13).
            if pose == Some(crate::simulation::OrderState::Idle)
                && let Some((image, ..)) = mission
                    .simulation
                    .as_ref()
                    .and_then(|s| crate::gameplay::grazing(s, mission.objects[index].class, index))
                && self.images.get(image, "image").is_some()
            {
                section = image.to_owned();
            }
            // A ninja jumping over a wall shows its jump (0x4ebd2d).
            if pose == Some(crate::simulation::OrderState::Moving)
                && mission
                    .simulation
                    .as_ref()
                    .is_some_and(|s| crate::gameplay::jump_frame(s, index).is_some())
                && self
                    .images
                    .get(crate::gameplay::JUMP_PICTURE, "image")
                    .is_some()
            {
                section = crate::gameplay::JUMP_PICTURE.to_owned();
            }
            // Attacks alternate `image_attack` and `image_attack2`; spells use
            // `image_spell`; a dead object plays `image_die`.
            if matches!(
                pose,
                Some(
                    crate::simulation::OrderState::Attacking
                        | crate::simulation::OrderState::Casting
                        | crate::simulation::OrderState::Dying
                )
            ) && let Some(image) = mission
                .simulation
                .as_ref()
                .and_then(|s| s.gameplay.actors.get(index))
                .filter(|a| !a.action_image.is_empty())
                .and_then(|a| self.property(o, &a.action_image))
            {
                section = image.to_owned();
            }
            if pose == Some(crate::simulation::OrderState::Working) {
                let key = mission
                    .simulation
                    .as_ref()
                    .and_then(|s| s.gameplay.actors.get(index))
                    .and_then(|a| a.intent.as_ref())
                    .map(|a| match a {
                        crate::gameplay::Intent::Chop(_) => "image_chop",
                        crate::gameplay::Intent::Capture(_) => "image_capture",
                        crate::gameplay::Intent::Harvest(i)
                            if mission
                                .objects
                                .get(*i)
                                .is_some_and(|o| o.class == class::COW) =>
                        {
                            "image_milk"
                        }
                        crate::gameplay::Intent::Harvest(i)
                            if mission.objects.get(*i).is_some_and(|o| {
                                !matches!(o.class, class::RES_GOLD | class::RES_STONE | class::COW)
                            }) =>
                        {
                            "image_dig"
                        }
                        _ => "image_mine",
                    });
                if let Some(image) = key.and_then(|key| self.property(o, key)) {
                    section = image.to_owned();
                }
            }
            let Some(file) = self.images.get(&section, "image") else {
                b.out
                    .missing
                    .push(format!("{section}: configurazione assente"));
                continue;
            };
            if file.is_empty() || section == "anim_none" {
                continue;
            }
            if let Some(direction) = direction.filter(|_| class::is_unit(o.class)) {
                drawn.variant = 0x8000 | direction as u16;
            }
            let first = b.out.visuals.len();
            b.object(index, &drawn, &section, slot)?;
            // Carry sheets contain walking frames only. A stopped carrier
            // keeps the load visible without walking on the spot.
            if pose == Some(crate::simulation::OrderState::Idle)
                && mission
                    .simulation
                    .as_ref()
                    .and_then(|s| s.gameplay.actors.get(index))
                    .is_some_and(|a| self.carrying_image(o, a.cargo, a.cargo_source).is_some())
            {
                for v in &mut b.out.visuals[first..] {
                    v.animation = None;
                }
            }
        }
        Ok(b.out)
    }
    /// The shipped action sheets, with a fallback to the standing picture.
    pub fn runtime_image(
        &self,
        o: &MapObject,
        pose: Option<crate::simulation::OrderState>,
    ) -> Option<String> {
        use crate::simulation::OrderState;
        let key = match pose.unwrap_or_default() {
            OrderState::Moving => "image",
            OrderState::Attacking => "image_attack",
            OrderState::Building => "image_build",
            OrderState::Working => "image_mine",
            OrderState::Casting => "image_spell",
            OrderState::Dying => "image_die",
            _ => "image_idle",
        };
        if class::is_unit(o.class) || pose == Some(OrderState::Dying) {
            self.property(o, key)
                .map(str::to_owned)
                .or_else(|| self.image_for(o))
        } else {
            self.image_for(o)
        }
    }
    pub fn carrying_image(&self, o: &MapObject, cargo: [u32; 4], source: usize) -> Option<String> {
        if o.class != class::PEON {
            return None;
        }
        let resource = cargo.iter().position(|&n| n > 0)?;
        if resource == 2 && source == class::COW {
            if let Some(image) = self.property(o, "image_carrymilk") {
                return Some(image.into());
            }
        }
        // Meat of a cow (carry kind 5, `image_carrymeat`, 0x49521c).
        if resource == 2 && source == class::RESITEM {
            if let Some(image) = self.property(o, "image_carrymeat") {
                return Some(image.into());
            }
        }
        let prefix = match o.civilization {
            2 => "r",
            3 => "a",
            _ => "e",
        };
        let section = format!(
            "{prefix}_peon_{}",
            ["gold", "stone", "food", "tree"][resource]
        );
        self.images.get(&section, "image").map(|_| section)
    }
    /// How a still-standing object moves, by its original class:
    /// - units loop their idle sheet (`image_idle`) on its animation record;
    /// - map animations step at the record's frame rate (payload 88: 0 and
    ///   10 mean the picture's own `fps`; zero leaves the picture still);
    /// - everything else (buildings, resources, items, walls) keeps one frame.
    fn timeline(
        &self,
        o: &MapObject,
        section: &str,
        columns: usize,
        rows: usize,
    ) -> Option<crate::animation::Timeline> {
        use crate::animation::Timeline;
        if rows <= 1 {
            return None;
        }
        match o.class {
            // Native ITEM_FLAG activation 0x465570–0x46559c starts
            // a 1000/fps timer. Capture logic remains inactive here.
            class::ITEM_FLAG => Timeline::stepped(&self.images, section, rows, 0),
            class::ANIMATION | class::GROUND_OBJECT | class::GROUND_PATCH => {
                let own = self
                    .images
                    .get(section, "fps")
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .unwrap_or(0);
                let fps = match o.word(0x58) {
                    0 | 10 => own,
                    f => f as u64,
                };
                (fps > 0).then(|| Timeline::uniform(rows, 1000 / fps))
            }
            c if class::is_unit(c) => {
                if columns > 1 {
                    Some(Timeline::directional(&self.images, section, rows))
                } else {
                    Timeline::stepped(&self.images, section, rows, 0)
                }
            }
            _ => None,
        }
    }
    /// The frame an object shows first.
    fn first_row(&self, o: &MapObject, section: &str, rows: usize) -> usize {
        if o.wall_damaged() {
            return o.wall_damage_row(rows);
        }
        match o.class {
            // Rows 0–2 are three looks of a full mine, drawn at loading
            // (the variant, 0x4afeae); the last row is the exhausted pit.
            // Growth stages: health × rows / maximum (0x44c89b).
            class::FIELD if section == "field_grow" => {
                (o.health as usize * rows / o.max_health.max(1) as usize).min(rows - 1)
            }
            // Rubble shows the frame its timer reached (0x444bd0).
            class::DEBRIS => (o.variant as usize).min(rows - 1),
            class::RES_GOLD | class::RES_STONE if rows > 1 => {
                if o.dword(0x3a) == 0 && o.byte(0x3f) == 0 {
                    rows - 1
                } else {
                    o.variant as usize % (rows - 1).min(3)
                }
            }
            _ => self
                .images
                .get(section, "startrow")
                .and_then(|n| n.trim().parse::<usize>().ok())
                .unwrap_or(0)
                .min(rows.saturating_sub(1)),
        }
    }
    /// The object's point in world pixels. Map animations whose picture has
    /// a hotspot are placed by it from the exact pixel in payload 102/104.
    fn anchor(&self, o: &MapObject, section: &str) -> [f32; 2] {
        if matches!(o.class, class::ANIMATION | class::GROUND_OBJECT) {
            let hot = self.images.numbers(section, "hotspot");
            if hot.len() >= 2 && hot[0] != 0 && hot[1] != 0 {
                let rect = self.images.numbers(section, "object");
                let ox = rect.first().copied().unwrap_or(0);
                let oy = rect.get(1).copied().unwrap_or(0);
                let x = ox + o.word(0x66) as i16 as i32 - hot[0];
                let y = oy + o.word(0x68) as i16 as i32 - hot[1];
                return [x as f32, -(y as f32)];
            }
        }
        o.location()
    }
    /// Picture corner, selection box and footprint of `section` drawn at `at`.
    fn place(&self, index: usize, o: &MapObject, section: &str, at: [f32; 2]) -> ObjectVisual {
        let rect = self.images.numbers(section, "object");
        let ox = rect.first().copied().unwrap_or(0) as f32;
        let oy = rect.get(1).copied().unwrap_or(0) as f32;
        let fw = rect.get(2).copied().unwrap_or(32).max(1) as f32;
        let fh = rect.get(3).copied().unwrap_or(32).max(1) as f32;
        let top = [at[0] - ox, at[1] + oy];
        let hit = self.images.numbers(section, "selection");
        let bx = hit.first().copied().unwrap_or(ox as i32) as f32;
        let by = hit.get(1).copied().unwrap_or(oy as i32) as f32;
        let bw = hit.get(2).copied().unwrap_or(fw as i32).max(1) as f32;
        let bh = hit.get(3).copied().unwrap_or(fh as i32).max(1) as f32;
        ObjectVisual {
            object: index,
            image: 0,
            top_left: top,
            bounds: [top[0] + bx, top[1] - by - bh, bw, bh],
            footprint: [at[0], at[1] - fh, fw, fh],
            ground: o.class == class::GROUND_PATCH,
            animation: None,
            phase_ms: 0,
            start_row: 0,
            layer: Layer::Body,
            tree: false,
        }
    }
    /// A property of the object's civilization section, with the same
    /// fallbacks as the picture.
    pub fn property(&self, o: &MapObject, key: &str) -> Option<&str> {
        // Native monk getter 0x4eb3c0 unlocks conversion at level three.
        if key == "menu" && o.class == 12 && o.civilization == 3 && o.word(28) >= 3 {
            return Some("a_monk_priest");
        }
        if key == "menu" && matches!(o.class, class::GATE_WOOD | class::GATE_STONE) {
            // Native getter 0x457d40: state 0 is lowered/open (menu *_10),
            // state 1 is raised/closed (*_00); automatic is serialized at 65.
            return Some(
                match (
                    o.class == class::GATE_STONE,
                    o.byte(57) != 0,
                    o.byte(65) != 0,
                ) {
                    (false, false, false) => "gate_wood_10",
                    (false, false, true) => "gate_wood_11",
                    (false, true, false) => "gate_wood_00",
                    (false, true, true) => "gate_wood_01",
                    (true, false, false) => "gate_stone_10",
                    (true, false, true) => "gate_stone_11",
                    (true, true, false) => "gate_stone_00",
                    (true, true, true) => "gate_stone_01",
                },
            );
        }
        let def = self.objects.get(CLASSES[o.class])?;
        let civ = civ_section(o.civilization);
        def.get(civ, key)
            .or_else(|| def.get("comm", key))
            .filter(|s| !s.is_empty())
    }
}

/// Sprite sheets, frames and animations shared by every object of a scene,
/// one copy per picture and kingdom colour.
struct Builder<'a> {
    lib: &'a ObjectLibrary,
    assets: &'a GameAssets,
    side: usize,
    out: VisualScene,
    sheets: BTreeMap<(String, Option<usize>), SpriteSheet>,
    /// Pictures without kingdom shades, loaded once for every colour.
    shared: std::collections::BTreeSet<String>,
    frames: BTreeMap<((String, Option<usize>), usize, usize), usize>,
    animations: std::collections::HashMap<((String, Option<usize>), usize, Vec<u64>), usize>,
}
type SheetKey = (String, Option<usize>);
/// The frames of an effect picture (see [`ObjectLibrary::effect_picture`]).
pub struct EffectPicture {
    pub columns: usize,
    pub rows: usize,
    pub frames: Vec<Bitmap>,
    pub fps: u32,
    pub anchor: [i32; 2],
}
impl Builder<'_> {
    /// One object drawn from its class picture: body, shadow and the
    /// kingdom overlay, each with the class's animation rule.
    fn object(
        &mut self,
        index: usize,
        o: &MapObject,
        section: &str,
        slot: Option<usize>,
    ) -> Result<()> {
        let lib = self.lib;
        let key = self.sheet(section, slot)?;
        let (columns, rows) = (self.sheets[&key].columns, self.sheets[&key].rows);
        let wall = [class::WALL_STONE, class::WALL_WOOD].contains(&o.class);
        // Connections choose a sheet, never its column: 0x4fae60 resets
        // refreshed walls to column zero; saved picture variants stay separate.
        let col = if class::is_unit(o.class) && o.variant & 0x8000 != 0 {
            (o.variant as usize & 7) * columns / 8
        } else {
            o.variant as usize % columns
        };
        let at = lib.anchor(o, section);
        let mut body = lib.place(index, o, section, at);
        if [class::BIRD, class::EAGLE].contains(&o.class) {
            return self.flight(o, &key, section, body, at);
        }
        // 0x41e0c0: a site's row follows its progress, rows × done / TIME,
        // which its health keeps in proportion.
        let site = lib.site_picture(o).is_some_and(|s| s == section);
        let row = if site {
            (rows * o.health as usize / o.max_health.max(1) as usize).min(rows - 1)
        } else {
            lib.first_row(o, section, rows)
        };
        body.image = self.frame(&key, col, row);
        if let Some(timeline) = lib.timeline(o, section, columns, rows) {
            // Scenery animations share one ticker per pace and all start
            // at their start row (0x4312b0, 0x434640): they run in step.
            body.phase_ms = if matches!(
                o.class,
                class::ANIMATION | class::GROUND_OBJECT | class::GROUND_PATCH
            ) {
                0
            } else {
                o.id as u64 * 2654435761 % timeline.duration_ms
            };
            body.animation = Some(self.animation(&key, col, timeline));
            body.start_row = row;
        }
        // Separate shadow sheet `<section>_s`: a darkening run mask with
        // the same canvas as the body, drawn immediately before its body. On the
        // same frame grid it follows the body's frame.
        let shadow_section = format!("{section}_s");
        if lib.images.get(&shadow_section, "image").is_some() {
            let shadow = self.sheet(&shadow_section, None)?;
            let (s_columns, s_rows) = (self.sheets[&shadow].columns, self.sheets[&shadow].rows);
            let same_grid = s_rows == rows;
            let image = self.frame(&shadow, col % s_columns, if same_grid { row } else { 0 });
            let animation = body
                .animation
                .filter(|_| same_grid)
                .map(|id| self.out.animations[id].timeline.clone())
                .map(|t| self.animation(&shadow, col % s_columns, t));
            let [dx, dy] = shadow_lift(o.class);
            self.out.visuals.push(ObjectVisual {
                image,
                animation,
                layer: Layer::Shadow,
                top_left: [body.top_left[0] + dx, body.top_left[1] - dy],
                ..body
            });
        }
        self.out.visuals.push(body);
        // A horizontal bridge has a second node (0x4f0df0 with 1, as +0x18
        // is set, 0x41be9f): its front railing `bridge_horz1` (0x41be8d),
        // placed by that picture's `object` from the bridge's corner and
        // sorted that far below its row (vt+0x3c = 0x41bfc0), so units on
        // the deck pass over the bridge and behind the railing.
        if o.class == class::BRIDGE
            && section == "bridge_horz"
            && lib.images.get(BRIDGE_RAILING, "image").is_some()
        {
            let key = self.sheet(BRIDGE_RAILING, slot)?;
            let image = self.frame(&key, 0, 0);
            let offset = lib.images.numbers(BRIDGE_RAILING, "object");
            let dx = offset.first().copied().unwrap_or(0) as f32;
            let dy = offset.get(1).copied().unwrap_or(0) as f32;
            self.out.visuals.push(ObjectVisual {
                image,
                animation: None,
                top_left: [body.top_left[0] + dx, body.top_left[1] - dy],
                footprint: [at[0] + dx, at[1] - dy - 32., body.footprint[2], 32.],
                layer: Layer::Overlay,
                ..body
            });
        }
        // Kingdom overlay (`image_c`): banners and flags in the owner's
        // colour, placed by its `object` offset from the body's corner.
        // Walls carry it on their towers only (wall mask below 16).
        // A site carries its picture's own flag (`<picture>_c`, vt+0x2c8).
        let overlay = if site {
            format!("{section}_c")
        } else {
            let Some(overlay) = lib.property(o, "image_c") else {
                return Ok(());
            };
            overlay.to_owned()
        };
        if lib.images.get(&overlay, "image").is_none() || (wall && (o.word(56) >= 16 || row != 0)) {
            return Ok(());
        }
        let key = self.sheet(&overlay, slot)?;
        let (o_columns, o_rows) = (self.sheets[&key].columns, self.sheets[&key].rows);
        let col = col % o_columns;
        let image = self.frame(&key, col, 0);
        // Building overlays step at their fps, ten frames a second without it.
        let animation = crate::animation::Timeline::stepped(&lib.images, &overlay, o_rows, 10)
            .map(|t| self.animation(&key, col, t));
        let offset = lib.images.numbers(&overlay, "object");
        let dx = offset.first().copied().unwrap_or(0) as f32;
        let dy = offset.get(1).copied().unwrap_or(0) as f32;
        self.out.visuals.push(ObjectVisual {
            image,
            animation,
            top_left: [body.top_left[0] + dx, body.top_left[1] - dy],
            layer: Layer::Overlay,
            ..body
        });
        Ok(())
    }
    /// Gates (`g{w|s}{0-3}_anim`): row 8 holds the two towers, rows 0–6 the
    /// drawbridge from raised to lowered (`bridge`, 0x456080), and two flags
    /// of `gates c` mark the owner, as the original draws them.
    fn gate(
        &mut self,
        index: usize,
        o: &MapObject,
        slot: Option<usize>,
        bridge: usize,
    ) -> Result<()> {
        let lib = self.lib;
        let stone = o.class == class::GATE_STONE;
        let orientation = o.byte(0x38) as usize & 3;
        let section = format!("g{}{orientation}_anim", if stone { 's' } else { 'w' });
        if lib.images.get(&section, "image").is_none() {
            self.out
                .missing
                .push(format!("{section}: configurazione assente"));
            return Ok(());
        }
        let key = self.sheet(&section, None)?;
        let (columns, rows) = (self.sheets[&key].columns, self.sheets[&key].rows);
        if rows < 9 {
            return Err(invalid(format!("{section}: cancello senza le torri")));
        }
        let body = lib.place(index, o, &section, o.location());
        let shadow_section = format!("{section}_s");
        let shadow = if lib.images.get(&shadow_section, "image").is_some() {
            Some(self.sheet(&shadow_section, None)?)
        } else {
            None
        };
        // Vertical gates split each frame in two columns, drawn back to front.
        for col in (0..columns).rev() {
            for row in [8, bridge] {
                if let Some(s) = &shadow
                    && col < self.sheets[s].columns
                    && row < self.sheets[s].rows
                {
                    let image = self.frame(s, col, row);
                    self.out.visuals.push(ObjectVisual {
                        image,
                        layer: Layer::Shadow,
                        ..body
                    });
                }
                let image = self.frame(&key, col, row);
                self.out.visuals.push(ObjectVisual { image, ..body });
            }
        }
        // Flag column by orientation (1, 0, 3, 2 → 0, 2, 4, 6), plus one for stone.
        let flag_column = [2, 0, 6, 4][orientation] + stone as usize;
        if lib.images.get("gates c", "image").is_some() {
            let flags = self.sheet("gates c", slot)?;
            let (f_columns, f_rows) = (self.sheets[&flags].columns, self.sheets[&flags].rows);
            for row in [bridge, bridge + 8] {
                if flag_column >= f_columns || row >= f_rows {
                    continue;
                }
                let [dx, dy] = GATE_FLAGS[flag_column][row];
                let image = self.frame(&flags, flag_column, row);
                self.out.visuals.push(ObjectVisual {
                    image,
                    top_left: [body.top_left[0] + dx as f32, body.top_left[1] - dy as f32],
                    layer: Layer::Overlay,
                    ..body
                });
            }
        }
        Ok(())
    }
    /// Fish patches show one of the five `fish_N` jumps, picked at random,
    /// stepped every 100 ms by the original fish manager from a random frame.
    /// The nets of the boats fishing there are drawn on top from the
    /// patch's corner (0x4510f0, pictures loaded by 0x4516f0).
    fn fish(&mut self, index: usize, o: &MapObject, nets: &[usize]) -> Result<()> {
        let section = format!("fish_{}", object_noise(o.id, 2) % 5 + 1);
        if self.lib.images.get(&section, "image").is_none() {
            return Ok(());
        }
        let key = self.sheet(&section, None)?;
        let rows = self.sheets[&key].rows;
        let mut visual = self.lib.place(index, o, &section, o.location());
        visual.image = self.frame(&key, 0, 0);
        visual.ground = true;
        if rows > 1 {
            visual.phase_ms = (object_noise(o.id, 3) as usize % rows) as u64 * 100;
            visual.animation =
                Some(self.animation(&key, 0, crate::animation::Timeline::uniform(rows, 100)));
        }
        self.out.visuals.push(visual);
        for &n in nets {
            let section = format!("net{}", n + 1);
            if self.lib.images.get(&section, "image").is_none() {
                continue;
            }
            let key = self.sheet(&section, None)?;
            let mut net = self.lib.place(index, o, &section, o.location());
            net.image = self.frame(&key, 0, 0);
            net.ground = true;
            net.layer = Layer::Overlay;
            self.out.visuals.push(net);
        }
        Ok(())
    }
    /// Seagulls and eagles: the body and its shadow are handed to a
    /// [`Flight`] that moves them while the scene runs.
    fn flight(
        &mut self,
        o: &MapObject,
        key: &SheetKey,
        section: &str,
        mut body: ObjectVisual,
        at: [f32; 2],
    ) -> Result<()> {
        let lib = self.lib;
        let (columns, rows) = (self.sheets[key].columns, self.sheets[key].rows);
        let frames: Vec<Vec<usize>> = (0..columns)
            .map(|c| (0..rows).map(|r| self.frame(key, c, r)).collect())
            .collect();
        let column = o.variant as usize % columns;
        body.image = frames[column][0];
        body.layer = Layer::Air;
        let body_index = self.out.visuals.len();
        self.out.visuals.push(body);
        let shadow_section = format!("{section}_s");
        let mut shadow_frames: Vec<Vec<usize>> = Vec::new();
        let mut shadow = None;
        if lib.images.get(&shadow_section, "image").is_some() {
            let s = self.sheet(&shadow_section, None)?;
            let (s_columns, s_rows) = (self.sheets[&s].columns, self.sheets[&s].rows);
            if s_columns == columns && s_rows == rows {
                shadow_frames = (0..columns)
                    .map(|c| (0..rows).map(|r| self.frame(&s, c, r)).collect())
                    .collect();
                shadow = Some(self.out.visuals.len());
                let [dx, dy] = shadow_lift(o.class);
                self.out.visuals.push(ObjectVisual {
                    image: shadow_frames[column][0],
                    layer: Layer::Shadow,
                    top_left: [body.top_left[0] + dx, body.top_left[1] - dy],
                    ..body
                });
            }
        }
        // The original animation record of a flier: one pixel every
        // (32000 / move_len) >> 5 ms, a new wing frame every
        // cycle × move_len / (rows × 1000) steps.
        let move_len = lib
            .images
            .get(section, "move_len")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(32);
        let cycle = crate::animation::Timeline::directional(&lib.images, section, rows).duration_ms;
        let step_ms = ((32000 / move_len) >> 5).max(1);
        let steps_per_frame = (cycle * move_len / (rows as u64 * 1000)).max(1) as u32;
        let edge = self.side.max(22) as f32 * 32.;
        self.out.flights.push(Flight {
            body: body_index,
            shadow,
            frames,
            shadow_frames,
            step_ms,
            steps_per_frame,
            start: at,
            offset: [body.top_left[0] - at[0], body.top_left[1] - at[1]],
            range: [320., edge - 352.],
            column,
            seed: object_noise(o.id, 4),
            shadow_lift: shadow_lift(o.class),
        });
        Ok(())
    }
    /// Load the sheet of an image section for a colour slot. Pictures without
    /// kingdom shades are shared by every slot.
    fn sheet(&mut self, section: &str, slot: Option<usize>) -> Result<SheetKey> {
        let file = self
            .lib
            .images
            .get(section, "image")
            .ok_or_else(|| invalid(format!("{section}: configurazione assente")))?;
        let file = format!("IMAGES/{}", file.replace('\\', "/"));
        // The INI names a BMP; the archive may store it as RLE instead.
        let rle = {
            let stem = file
                .get(..file.len().saturating_sub(4))
                .filter(|_| file.to_ascii_uppercase().ends_with(".BMP"))
                .unwrap_or(&file);
            format!("{stem}.RLE")
        };
        if self.shared.contains(&file) {
            return Ok((file, None));
        }
        if self.sheets.contains_key(&(file.clone(), slot)) {
            return Ok((file, slot));
        }
        let data_name = if self.assets.contains(&file) {
            file.clone()
        } else {
            rle
        };
        let data = self.assets.get(&data_name)?;
        let bmp = data_name == file && data.starts_with(b"BM");
        let recolor = if bmp {
            Bitmap::is_indexed(&data)
        } else {
            SpriteSheet::is_indexed(&data)
        };
        if !recolor {
            self.shared.insert(file.clone());
        }
        let key = (file.clone(), if recolor { slot } else { None });
        if self.sheets.contains_key(&key) {
            return Ok(key);
        }
        let sheet = if bmp {
            let image = Bitmap::decode_for(&data, true, key.1)?;
            // Uncompressed sheets retain their two-dimensional layout.
            let grid = |name| {
                self.lib
                    .images
                    .get(section, name)
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(1)
            };
            let (cols, rows) = (grid("cols"), grid("rows"));
            if cols == 0 || rows == 0 || cols > image.width as usize || rows > image.height as usize
            {
                return Err(invalid(format!(
                    "{section}: griglia {cols}×{rows} non valida"
                )));
            }
            let fw = image.width as usize / cols;
            let fh = image.height as usize / rows;
            let mut packed = Bitmap {
                width: fw as u32,
                height: (fh * cols * rows) as u32,
                rgba: Vec::with_capacity(image.rgba.len()),
            };
            for y in 0..rows {
                for x in 0..cols {
                    for py in 0..fh {
                        let at = ((y * fh + py) * image.width as usize + x * fw) * 4;
                        packed.rgba.extend_from_slice(&image.rgba[at..at + fw * 4]);
                    }
                }
            }
            SpriteSheet {
                columns: cols,
                rows,
                frame_height: fh,
                image: packed,
            }
        } else {
            SpriteSheet::decode_for(&data, key.1)?
        };
        self.sheets.insert(key.clone(), sheet);
        Ok(key)
    }
    fn frame(&mut self, key: &SheetKey, col: usize, row: usize) -> usize {
        let images = &mut self.out.images;
        let sheet = &self.sheets[key];
        *self
            .frames
            .entry((key.clone(), col, row))
            .or_insert_with(|| {
                images.push(sheet.frame(col, row).unwrap());
                images.len() - 1
            })
    }
    /// The frames of one column on a timeline, shared by every object that
    /// shows the same picture at the same pace.
    fn animation(
        &mut self,
        key: &SheetKey,
        col: usize,
        timeline: crate::animation::Timeline,
    ) -> usize {
        let entry = (key.clone(), col, timeline.ends().to_vec());
        if let Some(&id) = self.animations.get(&entry) {
            return id;
        }
        let rows = self.sheets[key].rows;
        let frames = (0..rows).map(|r| self.frame(key, col, r)).collect();
        let id = self.out.animations.len();
        self.out
            .animations
            .push(SpriteAnimation { frames, timeline });
        self.animations.insert(entry, id);
        id
    }
}

/// Draw order of a solid object: rows further down are in front; within a row
/// the original engine draws right to left. The column term stays below half a
/// row step so it can never reorder rows.
/// Overlays sit just above their own body: half a column step, so they never
/// cover the next object of the row.
/// Ground shadow depth for independently animated flying objects.
pub const SHADOW_DEPTH: f32 = 3.;
/// Birds in flight: above every object, banner and tree.
pub const AIR_DEPTH: f32 = 400.;
/// Native constructor priorities: the painter sorts these before coordinates.
pub fn presentation_priority(o: &MapObject) -> u16 {
    // 0x4b0028: an exhausted mine lies on the ground.
    if o.flags & FLAG_DEAD != 0 {
        return 1;
    }
    match o.class {
        15 | 22 | 32 | class::BIRD | class::EAGLE => 300,
        63 => 50,
        class::ITEM
        | class::ITEM_PLUSPLUS
        | class::ITEM_SCROLL
        | class::ITEM_MAGIC
        | class::ITEM_HEALTH => 81,
        // 0x444a4d: rubble lies on the ground.
        class::GROUND_PATCH | class::DEBRIS => 1,
        _ => 100,
    }
}
/// Rank the entire scene, avoiding floating-point collisions between priority bands
/// or neighbouring columns on large maps. Shadows and banners follow their body.
pub fn painter_depths(visuals: &[ObjectVisual], mission: &MissionWorld, side: usize) -> Vec<f32> {
    // Each picture's place worked out once; the stable sort keeps equal
    // places in the scene's order.
    let mut order: Vec<_> = visuals
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let priority = mission
                .objects
                .get(v.object)
                .map_or(100, presentation_priority);
            let layer = match v.layer {
                Layer::Shadow => 0,
                Layer::Body | Layer::Air => 1,
                Layer::Overlay => 2,
            };
            (priority, depth(v.footprint, side), layer, i)
        })
        .collect();
    order.sort_by(|(ap, ad, al, _), (bp, bd, bl, _)| {
        ap.cmp(bp).then(ad.total_cmp(bd)).then(al.cmp(bl))
    });
    let mut out = vec![0.; visuals.len()];
    let count = visuals.len().max(1) as f32;
    for (rank, (.., i)) in order.into_iter().enumerate() {
        out[i] = 10. + rank as f32 * 380. / count;
    }
    out
}
/// A horizontal bridge's front railing, its second painter node.
const BRIDGE_RAILING: &str = "bridge_horz1";
/// The painter's place of a footprint `[x, bottom, width, height]` (world
/// pixels, y up). Native nodes sort by the object's position, the corner of
/// its cell (+0x12, +0x14, set by 0x500950 from the cell × 32), not by the
/// bottom of its picture: a bridge is behind the units on its deck and a
/// large building behind those beside it.
pub fn depth(footprint: [f32; 4], side: usize) -> f32 {
    let edge = side.max(1) as f32 * 32.;
    let top = footprint[1] + footprint[3];
    let row = (-top / edge).clamp(0., 1.);
    let column = (footprint[0] / edge).clamp(0., 1.);
    // Native 0x4f1000 sorts equal rows by descending x. Wall pictures
    // deliberately overlap: drawing their left caps last hides each seam.
    10. + row * 100. + (1. - column) * 50. / side.max(1) as f32
}

/// The biome's tree atlases: 31 columns of tree images and of their shadows,
/// with their anchors.
struct TreeAtlas {
    tree: Bitmap,
    shadows: Bitmap,
    anchor: [f32; 2],
    shadow_anchor: [f32; 2],
}
impl TreeAtlas {
    fn load(a: &GameAssets, biome: &str) -> Result<Self> {
        let ini = Ini::parse(&a.get(&format!("IMAGES/TERRAIN/{biome}.TERRAIN.INI"))?);
        let dir = ini.get("terrain", "dir").unwrap_or(biome);
        let file = ini.get("trees", "img_trees").unwrap_or("trees.bmp");
        let tree = Bitmap::decode(&a.get(&format!("IMAGES/TERRAIN/{dir}/{file}"))?, true)?;
        if tree.width % 31 != 0 {
            return Err(invalid("Invalid original tree atlas"));
        }
        let anchor = ini.numbers("trees", "pos_tree");
        // Native 0x43cc84–0x43cda8 loads a separate atlas and sets its
        // monochrome shadow mode to 50%; its anchor differs from the tree.
        let shadow_file = ini.get("trees", "img_shadows").unwrap_or("trees_s.bmp");
        let mut shadows = Bitmap::decode(
            &a.get(&format!("IMAGES/TERRAIN/{dir}/{shadow_file}"))?,
            true,
        )?;
        if shadows.width % 31 != 0 {
            return Err(invalid("Invalid original tree shadow atlas"));
        }
        for pixel in shadows.rgba.chunks_exact_mut(4) {
            if pixel[3] != 0 {
                pixel.copy_from_slice(&crate::sprite::SHADOW);
            }
        }
        let shadow_anchor = ini.numbers("trees", "pos_shadow");
        Ok(Self {
            tree,
            shadows,
            anchor: [
                anchor.first().copied().unwrap_or(1) as f32 * 32.,
                anchor.get(1).copied().unwrap_or(3) as f32 * 32.,
            ],
            shadow_anchor: [
                shadow_anchor.first().copied().unwrap_or(0) as f32 * 32.,
                shadow_anchor.get(1).copied().unwrap_or(1) as f32 * 32.,
            ],
        })
    }
    /// The tree and the shadow of a cell's image (1-based).
    fn pictures(&self, value: usize) -> Result<[Bitmap; 2]> {
        let fw = self.tree.width as usize / 31;
        let mut image = Bitmap {
            width: fw as u32,
            height: self.tree.height,
            rgba: Vec::with_capacity(fw * self.tree.height as usize * 4),
        };
        for y in 0..self.tree.height as usize {
            let at = (y * self.tree.width as usize + (value - 1) * fw) * 4;
            image
                .rgba
                .extend_from_slice(&self.tree.rgba[at..at + fw * 4]);
        }
        let sw = self.shadows.width / 31;
        let shadow = self
            .shadows
            .crop((value as u32 - 1) * sw, 0, sw, self.shadows.height)?;
        Ok([image, shadow])
    }
}
/// The tree and the shadow of one tree image (1-based, as the cells hold it),
/// with their anchors: a cell's picture has its top left corner at the
/// cell's own minus the anchor's x and plus its y, as in `add_trees`.
pub fn tree_pictures(
    a: &GameAssets,
    biome: &str,
    value: usize,
) -> Result<([Bitmap; 2], [[f32; 2]; 2])> {
    if !(1..=31).contains(&value) {
        return Err(invalid("Invalid tree image"));
    }
    let atlas = TreeAtlas::load(a, biome)?;
    Ok((atlas.pictures(value)?, [atlas.anchor, atlas.shadow_anchor]))
}
/// Trees are encoded in bits 11..15 of each terrain cell (zero means no tree).
/// The original biome supplies a 31-column, 96×128 atlas and its tile anchor.
pub fn add_trees(
    a: &GameAssets,
    grid: &MapGrid,
    biome: &str,
    out: &mut VisualScene,
) -> Result<usize> {
    let atlas = TreeAtlas::load(a, biome)?;
    let [ox, oy] = atlas.anchor;
    let [sx, sy] = atlas.shadow_anchor;
    let mut frames = BTreeMap::new();
    let mut count = 0;
    for (i, &cell) in grid.cells.iter().enumerate() {
        let value = (cell >> 11) as usize;
        if value == 0 {
            continue;
        }
        let [image, shadow] = match frames.entry(value) {
            std::collections::btree_map::Entry::Occupied(e) => *e.get(),
            std::collections::btree_map::Entry::Vacant(e) => {
                let [body, shade] = atlas.pictures(value)?;
                let id = out.images.len();
                out.images.push(body);
                out.images.push(shade);
                *e.insert([id, id + 1])
            }
        };
        let x = (i % grid.side) as f32 * 32.;
        let y = -((i / grid.side) as f32) * 32.;
        out.visuals.push(ObjectVisual {
            object: usize::MAX,
            image: shadow,
            top_left: [x - sx, y + sy],
            bounds: [x, y - 32., 32., 32.],
            footprint: [x, y - 32., 32., 32.],
            ground: false,
            animation: None,
            phase_ms: 0,
            start_row: 0,
            layer: Layer::Shadow,
            tree: true,
        });
        out.visuals.push(ObjectVisual {
            object: usize::MAX,
            image,
            top_left: [x - ox, y + oy],
            bounds: [x, y - 32., 32., 32.],
            footprint: [x, y - 32., 32., 32.],
            ground: false,
            animation: None,
            phase_ms: 0,
            start_row: 0,
            layer: Layer::Body,
            tree: true,
        });
        count += 1;
    }
    Ok(count)
}

/// The cliff atlas of a terrain type. `[terrainfiles]` may redirect it: the
/// desert maps cliffs on type 2 to `18.bmp` and on type 4 to `38.bmp`.
pub fn cliff_file(ini: &Ini, kind: u16) -> String {
    ini.get("terrainfiles", &format!("{kind}8"))
        .filter(|f| !f.is_empty())
        .map_or_else(|| format!("{kind}8.bmp"), str::to_owned)
}

/// Cliff cells use a separate six-column, fourteen-row transparent atlas.
/// Bit 10 enables the overlay; bits 4..6 and 0..3 retain its column and row.
pub fn add_cliffs(
    a: &GameAssets,
    grid: &MapGrid,
    biome: &str,
    out: &mut VisualScene,
) -> Result<usize> {
    let ini = Ini::parse(&a.get(&format!("IMAGES/TERRAIN/{biome}.TERRAIN.INI"))?);
    let dir = ini.get("terrain", "dir").unwrap_or(biome);
    let mut sheets = BTreeMap::new();
    let mut frames = BTreeMap::new();
    let mut count = 0;
    for (i, &cell) in grid
        .cells
        .iter()
        .enumerate()
        .filter(|(_, c)| *c & 0x400 != 0)
    {
        let kind = (cell >> 7) & 7;
        let column = ((cell >> 4) & 7) as usize;
        let row = (cell & 15) as usize;
        let file = cliff_file(&ini, kind).to_ascii_uppercase();
        if !sheets.contains_key(&file) {
            let image = Bitmap::decode(&a.get(&format!("IMAGES/TERRAIN/{dir}/{file}"))?, true)?;
            sheets.insert(file.clone(), image);
        }
        let sheet = &sheets[&file];
        if column * 32 + 32 > sheet.width as usize || row * 32 + 32 > sheet.height as usize {
            return Err(invalid("Cliff frame outside original atlas"));
        }
        let image = *frames.entry((file, column, row)).or_insert_with(|| {
            let mut image = Bitmap {
                width: 32,
                height: 32,
                rgba: Vec::with_capacity(4096),
            };
            for y in 0..32 {
                let at = ((row * 32 + y) * sheet.width as usize + column * 32) * 4;
                image.rgba.extend_from_slice(&sheet.rgba[at..at + 128]);
            }
            let id = out.images.len();
            out.images.push(image);
            id
        });
        let x = (i % grid.side) as f32 * 32.;
        let y = -((i / grid.side) as f32) * 32.;
        out.visuals.push(ObjectVisual {
            object: usize::MAX,
            image,
            top_left: [x, y],
            bounds: [x, y - 32., 32., 32.],
            footprint: [x, y - 32., 32., 32.],
            ground: true,
            animation: None,
            phase_ms: 0,
            start_row: 0,
            layer: Layer::Body,
            tree: false,
        });
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_header_at_zero_keeps_the_fog_and_the_unexplored_map() {
        // 0x4c6fa0: the setup dialog ticks the fog for byte 0x4e at 0 and
        // the unexplored map for byte 0x4f at 0.
        let mut header = vec![0u8; 0x130];
        let rules = GameRules::from_header(&header);
        assert!(!rules.no_fog && !rules.explored);
        header[0x4e] = 1;
        header[0x4f] = 1;
        let rules = GameRules::from_header(&header);
        assert!(rules.no_fog && rules.explored);
        let rules = GameRules::from_header(&[]);
        assert!(!rules.no_fog && !rules.explored);
    }
    /// A record of `<map>.sounds` as the editor writes it.
    pub(crate) fn sound_record(
        sound: &str,
        mode: u8,
        delayed: bool,
        times: [u16; 3],
        on: Option<u16>,
        cell: [u16; 2],
    ) -> Vec<u8> {
        let mut r = vec![0u8; 112];
        r[0x20..0x20 + sound.len()].copy_from_slice(sound.as_bytes());
        r[0x60] = mode;
        r[0x61] = delayed as u8;
        for (at, v) in [(0x62, times[0]), (0x64, times[1]), (0x66, times[2])] {
            r[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }
        r[0x68] = on.is_some() as u8;
        r[0x6a..0x6c].copy_from_slice(&cell[0].to_le_bytes());
        r[0x6c..0x6e].copy_from_slice(&cell[1].to_le_bytes());
        r[0x6e..0x70].copy_from_slice(&on.unwrap_or(0).to_le_bytes());
        r
    }
    #[test]
    fn the_maps_sounds_go_to_their_object_or_to_a_sound_object_of_their_own() {
        let mut objects = vec![MapObject {
            id: 40,
            class: class::CASTLE,
            flags: 0,
            x: 3,
            y: 4,
            owner: 1,
            civilization: 1,
            variant: 0,
            health: 100,
            max_health: 100,
            name: String::new(),
            subtitle: String::new(),
            portrait: String::new(),
            portrait_large: String::new(),
            custom_image: String::new(),
            data: vec![0; 56],
            mole: 0,
            control: [0; 8],
            sight: [0; 8],
        }];
        let data = [
            sound_record("bird", 2, true, [0, 5, 9], None, [12, 7]),
            sound_record("SPELLS\\bless.wav", 1, false, [0, 0, 0], Some(40), [0, 0]),
            // An object the map does not have: the record is skipped.
            sound_record("bird", 0, false, [3, 0, 0], Some(77), [0, 0]),
        ]
        .concat();
        let sounds = sound_sources(&data, &mut objects);
        assert_eq!(sounds.len(), 2);
        let made = &objects[sounds[0].object];
        assert_eq!(
            (made.class, made.owner, made.x, made.y, made.id),
            (class::SOUND_OBJECT, 0, 12, 7, 41)
        );
        assert_eq!(
            sounds[0],
            SoundSource {
                object: 1,
                sound: "bird".into(),
                mode: 2,
                delayed: true,
                period: 0,
                min: 5,
                max: 9
            }
        );
        assert_eq!(sounds[1].object, 0);
        assert_eq!(sounds[1].sound, "SPELLS\\bless.wav");
        assert_eq!(sounds[1].mode, 1);
        assert_eq!(objects.len(), 2);
    }
    #[test]
    fn setup_choices_recolour_and_ally_by_team() {
        let kingdom = |n: u8| Kingdom {
            name: format!("Regno {n}"),
            leader: String::new(),
            active: n <= 3,
            color: n,
            team: n,
            civilization: 1,
            resources: [0; 4],
            computer: n != 1,
            ..Default::default()
        };
        let mut world = MissionWorld {
            kingdoms: (1..=8).map(kingdom).collect(),
            diplomacy: vec![0x20 | 1; 64],
            ..MissionWorld::default()
        };
        let choice = |record, color, team| KingdomChoice {
            record,
            color,
            team,
            civilization: None,
            ..Default::default()
        };
        // Kingdom 1 turns green and joins kingdom 2's team; kingdom 3 is untouched.
        world.diplomacy[2 * 8 + 1] = 0x40;
        world.apply_setup(&GameSetup {
            kingdoms: vec![choice(0, 3, 2), choice(1, 2, 2), choice(2, 3, 3)],
            ..Default::default()
        });
        assert_eq!(world.color_slot(1), Some(2));
        // 0x440635: the same team gives an alliance with shared view, both ways.
        assert_eq!(world.relation(1, 2), Some(Relation::Ally));
        assert_eq!(world.relation(2, 1), Some(Relation::Ally));
        assert_eq!(world.diplomacy[1], 0x41);
        // Other teams keep the scenario's attitude, alliances included.
        assert_eq!(world.relation(1, 3), Some(Relation::Neutral));
        assert_eq!(world.relation(2, 3), Some(Relation::Neutral));
        assert_eq!(world.relation(3, 2), Some(Relation::Ally));
        assert_eq!(world.diplomacy[2 * 8] & 1, 1);
        // Teams beyond the four of the screen show as none.
        assert_eq!(shown_team(7), 0);
    }
    #[test]
    fn named_classes_match_the_original_table() {
        for (id, name) in [
            (class::PEON, "peon"),
            (class::FOOTMAN, "footman"),
            (class::TOWER_STONE, "tower_stone"),
            (class::CASTLE, "castle"),
            (class::WALL_STONE, "wall_stone"),
            (class::WALL_WOOD, "wall_wood"),
            (class::BRIDGE, "bridge"),
            (class::GATE_WOOD, "gate_wood"),
            (class::GATE_STONE, "gate_stone"),
            (class::EVILCASTLE, "evilcastle"),
            (class::CURSOR, "cursor"),
            (class::RES_GOLD, "res_gold"),
            (class::FOG_REVEALER, "fog_revealer"),
            (class::RES_STONE, "res_stone"),
            (class::ITEM, "item"),
            (class::ITEM_PLUSPLUS, "item_plusplus"),
            (class::ANIMATION, "animation"),
            (class::RUIN, "ruin"),
            (class::OPEN_NEXT_MAP, "open_next_map"),
            (class::ITEM_SCROLL, "item_scroll"),
            (class::ITEM_MAGIC, "item_magic"),
            (class::ITEM_HEALTH, "item_health"),
            (class::GROUND_OBJECT, "ground_object"),
            (class::GROUND_PATCH, "ground_patch"),
            (class::COW, "cow"),
            (class::FISHPATCH, "fishpatch"),
            (class::CRUSADEANCHOR, "crusadeanchor"),
            (class::FAKEBUILDING, "fakebuilding"),
            (class::FAKERESOURCE, "fakeresource"),
            (class::VISUALOBJECT, "visualobject"),
            (class::SOUND_OBJECT, "sound_object"),
            (class::DUMMY, "dummy"),
            (class::FISHING_BOAT, "fishing_boat"),
            (class::ENEMY_GROUND, "enemy_ground"),
            (class::FAKEWALL, "fakewall"),
            (class::FAKEITEM, "fakeitem"),
            (class::FAKEGATE, "fakegate"),
        ] {
            assert_eq!(CLASSES[id], name);
        }
        // Every order marker is skipped, without listing them one by one.
        for (id, name) in CLASSES.iter().enumerate() {
            if name.starts_with("command_") {
                assert!(class::is_marker(id), "{name}");
            }
        }
        assert!(!class::is_marker(class::PEON) && !class::is_marker(class::BRIDGE));
    }
    fn object(class: usize, owner: u8, x: u16, y: u16) -> MapObject {
        MapObject {
            id: 0,
            class,
            flags: 0,
            x,
            y,
            owner,
            civilization: 1,
            variant: 0,
            health: 1,
            max_health: 1,
            name: String::new(),
            subtitle: String::new(),
            portrait: String::new(),
            portrait_large: String::new(),
            custom_image: String::new(),
            data: Vec::new(),
            mole: 0,
            control: [0; 8],
            sight: [0; 8],
        }
    }
    fn kingdom(active: bool, computer: bool) -> Kingdom {
        Kingdom {
            name: String::new(),
            leader: String::new(),
            active,
            color: 1,
            team: 1,
            civilization: 1,
            resources: [0; 4],
            computer,
            ..Default::default()
        }
    }
    #[test]
    fn the_player_commands_by_the_control_byte_then_owner_and_spy() {
        let mut world = MissionWorld {
            kingdoms: vec![
                kingdom(true, false),
                kingdom(true, true),
                kingdom(true, true),
            ],
            diplomacy: vec![0; 64],
            ..Default::default()
        };
        let mut foe = object(class::FOOTMAN, 2, 0, 0);
        let mut own = object(class::FOOTMAN, 1, 0, 0);
        assert!(world.commands(&own) && !world.commands(&foe));
        // 0x506c30: the byte first, 1 commands and 2 does not.
        own.control[0] = 2;
        foe.control[0] = 1;
        assert!(!world.commands(&own) && world.commands(&foe));
        // Without a byte: the spy hidden in it, the player's or that of a
        // kingdom sharing its units with the player (vt+0xbc, relation 4).
        foe.control = [0; 8];
        foe.mole = 3;
        assert!(!world.commands(&foe));
        world.diplomacy[2 * 8] = 4;
        assert!(world.commands(&foe));
        foe.mole = 1;
        world.diplomacy[2 * 8] = 0;
        assert!(world.commands(&foe));
        // Kingdom 0 shares its units with everyone (0x4405da): a neutral
        // cow is everyone's, unless a byte says otherwise.
        let mut cow = object(class::COW, 0, 0, 0);
        assert!(world.controls(0) && world.commands(&cow));
        cow.control[0] = 2;
        assert!(!world.commands(&cow));
    }
    #[test]
    fn the_human_kingdom_and_its_home_are_found() {
        // Gengis Khan: the first kingdom is computer-played, the player is 2.
        let mut world = MissionWorld {
            kingdoms: vec![
                kingdom(true, true),
                kingdom(true, false),
                kingdom(false, false),
            ],
            objects: vec![
                object(class::PEON, 1, 5, 5),
                object(class::FOOTMAN, 2, 100, 100),
                object(class::FOOTMAN, 2, 102, 101),
                object(class::FOOTMAN, 2, 180, 10),
                object(class::RES_GOLD, 0, 101, 100),
            ],
            ..MissionWorld::default()
        };
        assert_eq!(world.player(), 2);
        // The median ignores the straggler far away.
        assert_eq!(world.home(), Some([102, 101]));
        world.objects.push(object(class::CASTLE, 2, 60, 60));
        assert_eq!(world.home(), Some([60, 60]));
        // Multiplayer maps: nobody is computer-played; the first active plays.
        world.kingdoms = vec![kingdom(false, false), kingdom(true, false)];
        assert_eq!(world.player(), 2);
        assert_eq!(MissionWorld::default().player(), 1);
        assert_eq!(MissionWorld::default().home(), None);
    }
    #[test]
    fn cliff_atlases_follow_terrain_files_redirections() {
        let desert = Ini::parse(b"[terrainfiles]\n00 = 00.bmp\n28 = 18.bmp\n48 = 38.bmp\n");
        assert_eq!(cliff_file(&desert, 1), "18.bmp");
        assert_eq!(cliff_file(&desert, 2), "18.bmp");
        assert_eq!(cliff_file(&desert, 3), "38.bmp");
        assert_eq!(cliff_file(&desert, 4), "38.bmp");
        let grass = Ini::parse(b"[terrainfiles]\n48 = 48.bmp\n");
        assert_eq!(cliff_file(&grass, 2), "28.bmp");
    }
    #[test]
    fn draw_order_puts_lower_rows_in_front_and_breaks_ties_right_to_left() {
        for side in [64, 512, 1024] {
            let at = |x: usize, y: usize| {
                depth([x as f32 * 32., -(y as f32) * 32. - 32., 32., 32.], side)
            };
            assert!(
                at(0, 10) > at(side - 1, 9),
                "{side}: rows must dominate columns"
            );
            assert!(at(4, 10) > at(5, 10), "{side}: right to left within a row");
            // Selection outlines are drawn at 150, above every object.
            assert!(at(side - 1, side - 1) < 112.);
        }
    }
    #[test]
    fn flier_shadows_fall_aside_and_come_down_with_a_death() {
        // 0x4e76f0, 0x4ee95d, 0x4ed93d.
        assert_eq!(shadow_lift(15), [20., 35.]);
        assert_eq!(shadow_lift(22), [20., 35.]);
        assert_eq!(shadow_lift(32), [40., 70.]);
        assert_eq!(shadow_lift(class::EAGLE), [40., 70.]);
        assert_eq!(shadow_lift(class::FOOTMAN), [0., 0.]);
        // 0x4e7ae0: each frame takes off the offset over the frames left.
        assert_eq!(descent([20., 35.], 20, 0), [20., 35.]);
        assert_eq!(descent([20., 35.], 20, 1), [19., 34.]);
        assert_eq!(descent([20., 35.], 20, 19), [0., 0.]);
        let ys: Vec<f32> = (0..20).map(|r| descent([20., 35.], 20, r)[1]).collect();
        assert!(ys.windows(2).all(|w| w[1] <= w[0]));
    }
}
