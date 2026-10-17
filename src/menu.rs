//! Window-independent front end, drawn like the original: the artwork of
//! `IMAGES\SCREENS`, the controls' own bitmaps, the original fonts and text
//! colours, and the control positions of the original executable.
use crate::{
    Result,
    assets::{GameAssets, Ini, legacy_text},
    bitmap::Bitmap,
    font::BitmapFont,
    invalid,
    map::MapGrid,
    scenario::{Scenario, ScenarioCatalog},
    world::{GameRules, GameSetup, KingdomChoice, MissionWorld},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

mod flags;
mod settings;
pub use settings::SettingsTab;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// The window's size in physical pixels when not full screen: the last
    /// one it had, dragged by its edges or chosen among [`RESOLUTIONS`].
    pub window: [u32; 2],
    /// Read from the files of earlier versions only: an index of their
    /// sizes, now `window`.
    #[serde(skip_serializing)]
    pub resolution: Option<usize>,
    pub fullscreen: bool,
    pub vsync: bool,
    pub sound: u8,
    pub music: u8,
    /// The original "Volume" check boxes of the sound and music groups.
    pub sound_on: bool,
    pub music_on: bool,
    /// Unit acknowledgements and building selection sounds.
    pub speech: bool,
    pub building_sounds: bool,
    /// The most frames per second, 0 for no limit; with `vsync` the screen's
    /// rate caps them too.
    pub max_fps: u32,
    /// Read from the files of earlier versions only: an index of their
    /// caps (`FRAME_LIMITS`), now `max_fps`.
    #[serde(skip_serializing)]
    pub frame_limit: Option<usize>,
    /// Nature sounds ("Effetti ambientali").
    pub ambience: bool,
    pub scroll_speed: u8,
    /// The open game's speed (0x587748) for the Options slider, which
    /// shows a fifth of it (0x47b625); the game keeps it, not this file.
    #[serde(skip)]
    pub speed: u8,
    pub weather: bool,
    pub day_night: bool,
    /// Original HWCURSOR: H_DEF instead of S_DEF, both with hotspot (2, 0).
    pub system_cursor: bool,
    /// 0: panel on the right, 1: panel on the left.
    pub layout: usize,
    pub edge_scroll: bool,
    /// Faint cell edges while a building is being placed, an addition of
    /// this version.
    pub placement_grid: bool,
    /// How strong the grid is: an index of [`GRID_STRENGTHS`].
    pub grid_strength: usize,
    /// How the texts over the map (messages, hints, F4) are drawn: an index
    /// of [`MAP_TEXTS`]. 0 is the original's shadow at +1, +1; the outline
    /// and the dark band are additions of this version, for light ground
    /// such as snow or sand.
    pub map_text: usize,
    /// The modern graphics of the «GRAFICA» page, additions of this version,
    /// all off at first: the map without the original's RGB555 steps.
    pub true_color: bool,
    /// How map pixels are scaled: an index of [`PIXEL_FILTERS`] (the
    /// original's nearest pixels, even pixels at any zoom, smoothed art).
    pub pixel_filter: usize,
    /// Scan lines and the curve of an old monitor over the whole screen.
    pub crt: bool,
    /// Warm light round fires, buildings and spells when night falls.
    pub night_lights: bool,
    /// A soft halo round fire, magic and lightning.
    pub bloom: bool,
    /// Fog with blurred edges instead of the original's dithered tiles.
    pub soft_fog: bool,
    /// Trees swaying in the wind and glinting water.
    pub living_world: bool,
    /// Gliding zoom with more steps instead of the three levels.
    pub smooth_zoom: bool,
    /// Interface texts drawn at the screen's resolution.
    pub hd_text: bool,
    /// The open game's cheat panel (`HudState::cheats`), an addition of
    /// this version: the «ALTRO» page offers it only when opened from a
    /// game, which keeps it; this file does not.
    #[serde(skip)]
    pub cheats: bool,
    /// 0: whole-number pixel scaling (crisp original art), 1: fill the screen.
    pub ui_mode: usize,
    /// The campaign missions already reached stay on the campaign screen's
    /// list, an addition of this version: the original lists only each
    /// campaign's entry missions.
    pub campaign_progress: bool,
    /// The language of texts and voices, an index of
    /// [`crate::lang::LANGUAGES`]: English by default, or Italian, the
    /// installation's own, or one of this version's translations.
    pub language: usize,
    /// The player's name, shown over their units like the original's
    /// «Tzar 1»; the Windows user name until changed in the settings.
    pub player_name: String,
    /// Keys chosen by the player for the commands of [`crate::keys::ACTIONS`];
    /// the others keep the original key. An empty name leaves a command unbound.
    pub keys: BTreeMap<String, String>,
    /// The folder of the original game the portable copy plays beside
    /// ([`crate::original::require`]), kept here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_folder: Option<PathBuf>,
}
/// Longest player name, as the original's name field (20 characters).
pub const NAME_LENGTH: usize = 20;
/// Longest name of a save, as many characters as the field shows.
pub const SAVE_NAME_LENGTH: usize = 40;
/// The Windows user name, else the original default «Tzar 1».
pub fn default_player_name() -> String {
    std::env::var("USERNAME")
        .ok()
        .map(|n| n.trim().chars().take(NAME_LENGTH).collect::<String>())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Tzar 1".into())
}
/// The window sizes the settings offer ready-made.
pub const RESOLUTIONS: [(u32, u32); 5] = [
    (1280, 720),
    (1600, 900),
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
];
/// The sizes and frame caps earlier versions saved by index.
const LEGACY_RESOLUTIONS: [(u32, u32); 4] = [(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)];
const FRAME_LIMITS: [u32; 6] = [30, 60, 120, 144, 240, 0];
/// The frames per second a cap may be written as.
pub const FPS_RANGE: (u32, u32) = (10, 999);
/// The smallest window.
pub const MIN_WINDOW: [u32; 2] = [800, 600];
pub use settings::{fps_slider, slider_fps};
pub const UI_MODES: [&str; 2] = ["pixel nitidi", "adatta allo schermo"];
/// The strengths of the placement grid, from faint to marked.
pub const GRID_STRENGTHS: [&str; 3] = ["leggera", "media", "marcata"];
/// The ways of drawing the texts over the map (`Settings::map_text`).
pub const MAP_TEXTS: [&str; 3] = ["originali", "contorno", "fondo scuro"];
/// The scalings of the map's pixels (`Settings::pixel_filter`).
pub const PIXEL_FILTERS: [&str; 3] = ["originali", "regolari", "levigati"];
/// Sides of the random maps, shown with the original size names.
pub const RANDOM_SIDES: [usize; 7] = crate::random_map::SIDES;

/// The lines of one section of an original INI as written, `key = value`
/// or a bare name: `Ini` folds keys to lower case, the names are shown.
fn ini_lines(data: &[u8], section: &str) -> Vec<(String, String)> {
    let mut inside = false;
    let mut lines = Vec::new();
    for line in legacy_text(data).lines().map(str::trim) {
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = name.trim().eq_ignore_ascii_case(section);
        } else if inside {
            let (key, value) = line.split_once('=').unwrap_or((line, ""));
            lines.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    lines
}
// Original random-screen update (0x4cae70): small maps disable entries,
// rather than accepting the same player count and geography at every size.
fn random_player_limit(side: usize) -> usize {
    match side {
        64 => 4,
        96 => 6,
        _ => 8,
    }
}
fn random_kind_allowed(side: usize, kind: usize) -> bool {
    !matches!((side, kind), (64, 1 | 2 | 4) | (96, 1 | 4) | (128, 1))
}
fn constrain_random(
    side: usize,
    players: &mut usize,
    rows: &mut [SetupRow],
    options: &mut crate::random_map::Options,
) {
    *players = (*players).clamp(2, random_player_limit(side));
    if !random_kind_allowed(side, options.kind) {
        options.kind = 0;
    }
    // 0x4cb930 moves the human's entire slot to the last remaining row.
    if let Some(human) = rows.iter().position(|r| r.human) {
        if human >= *players && rows.len() >= *players {
            rows.swap(human, *players - 1);
            for (i, row) in rows.iter_mut().enumerate() {
                row.record = i;
            }
        }
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            window: [2560, 1440],
            resolution: None,
            fullscreen: false,
            vsync: true,
            sound: 75,
            music: 50,
            sound_on: true,
            music_on: true,
            speech: true,
            building_sounds: true,
            max_fps: 0,
            frame_limit: None,
            ambience: true,
            scroll_speed: 50,
            speed: crate::simulation::NORMAL_SPEED,
            weather: true,
            day_night: true,
            system_cursor: true,
            layout: 0,
            edge_scroll: true,
            placement_grid: true,
            grid_strength: 0,
            map_text: 0,
            true_color: false,
            pixel_filter: 0,
            crt: false,
            night_lights: false,
            bloom: false,
            soft_fog: false,
            living_world: false,
            smooth_zoom: false,
            hd_text: false,
            cheats: false,
            ui_mode: 0,
            language: crate::lang::Language::English.index(),
            campaign_progress: true,
            player_name: default_player_name(),
            keys: BTreeMap::new(),
            original_folder: None,
        }
    }
}
impl Settings {
    /// What the settings change outside the menus' own drawing, put in place
    /// by `Effect::SettingsChanged`: the window, its synchronization, the
    /// language, the open game's speed and cheat panel.
    fn outside(&self) -> (bool, [u32; 2], bool, usize, u8, bool) {
        (
            self.fullscreen,
            self.window,
            self.vsync,
            self.language,
            self.speed,
            self.cheats,
        )
    }
    /// The frames per second the pacing holds to: none with the vertical
    /// synchronization, which already follows the screen.
    pub fn frame_cap(&self) -> u32 {
        if self.vsync { 0 } else { self.max_fps }
    }
    /// Strict load: a missing file gives the defaults, a damaged one an error.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut s: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        s.normalize();
        Ok(s)
    }
    /// Startup load: a damaged file never prevents the game from starting.
    /// It is kept as `settings.json.corrotto` and the defaults are used.
    pub fn load_or_default(path: &Path) -> (Self, Option<String>) {
        match Self::load(path) {
            Ok(s) => (s, None),
            Err(e) => {
                let mut kept = path.as_os_str().to_owned();
                kept.push(".corrotto");
                let _ = std::fs::rename(path, PathBuf::from(kept));
                (
                    Self::default(),
                    Some(crate::lang::fill(
                        "Impostazioni non leggibili ({}): ripristinati i valori predefiniti.",
                        &[&e],
                    )),
                )
            }
        }
    }
    pub fn normalize(&mut self) {
        if let Some(i) = self.resolution.take() {
            let (w, h) = LEGACY_RESOLUTIONS[i % LEGACY_RESOLUTIONS.len()];
            self.window = [w, h];
        }
        for (v, min) in self.window.iter_mut().zip(MIN_WINDOW) {
            *v = (*v).clamp(min, 16384);
        }
        self.map_text %= MAP_TEXTS.len();
        self.pixel_filter %= PIXEL_FILTERS.len();
        self.layout %= 2;
        self.grid_strength %= GRID_STRENGTHS.len();
        if let Some(i) = self.frame_limit.take() {
            self.max_fps = FRAME_LIMITS[i % FRAME_LIMITS.len()];
        }
        if self.max_fps != 0 {
            self.max_fps = self.max_fps.clamp(FPS_RANGE.0, FPS_RANGE.1);
        }
        self.ui_mode %= UI_MODES.len();
        self.language %= crate::lang::LANGUAGES.len();
        let name: String = self.player_name.trim().chars().take(NAME_LENGTH).collect();
        self.player_name = if name.is_empty() {
            default_player_name()
        } else {
            name
        };
        for v in [&mut self.sound, &mut self.music, &mut self.scroll_speed] {
            *v = (*v).min(100);
        }
        self.speed = self.speed.min(100) / 5 * 5;
        self.keys.retain(|action, key| {
            crate::keys::ACTIONS.iter().any(|(a, _)| a == action) && key.len() <= 32
        });
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        crate::write_atomic(path, &serde_json::to_vec_pretty(self)?)
    }
    /// The key of a map command: the player's choice or the original one.
    pub fn key(&self, action: &str) -> String {
        self.keys
            .get(action)
            .cloned()
            .unwrap_or_else(|| crate::keys::default_key(action))
    }
    pub fn crisp(&self) -> bool {
        self.ui_mode == 0
    }
    /// Effects volume after the group's on/off box.
    pub fn sound_volume(&self) -> u8 {
        if self.sound_on { self.sound } else { 0 }
    }
    pub fn music_volume(&self) -> u8 {
        if self.music_on { self.music } else { 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Main,
    Single,
    Campaign,
    Missions,
    /// Setup of a custom mission (`S_SINGLESETUP`): kingdoms, colours, teams.
    Setup,
    /// The settings: one page of this version with tabs, in place of the
    /// original options dialog (`settings.rs`).
    Options,
    Random,
    Load,
    Save,
    Multiplayer,
    Credits,
    Help,
    Pause,
    Quit,
    /// "Sei sicuro di volere abbandonare?" from the mission menu.
    Resign,
    /// The end of a game (0x442700): won or lost, with its buttons.
    End,
}
impl Screen {
    pub const ALL: [Screen; 16] = [
        Self::Main,
        Self::Single,
        Self::Campaign,
        Self::Missions,
        Self::Setup,
        Self::Options,
        Self::Random,
        Self::Load,
        Self::Save,
        Self::Multiplayer,
        Self::Credits,
        Self::Help,
        Self::Pause,
        Self::Quit,
        Self::Resign,
        Self::End,
    ];
    pub fn is_settings(self) -> bool {
        self == Self::Options
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionKey {
    Sound,
    Music,
    SoundOn,
    MusicOn,
    Speech,
    BuildingSounds,
    /// «Fotogrammi al secondo»: the cap, along its slider.
    Frames,
    /// «Sincronizzazione verticale».
    Vsync,
    Ambience,
    Scroll,
    GameSpeed,
    Weather,
    DayNight,
    SystemCursor,
    Layout,
    Resolution,
    Fullscreen,
    Edge,
    /// The placement grid: off, or one of [`GRID_STRENGTHS`].
    Grid,
    MapText,
    TrueColor,
    PixelFilter,
    Crt,
    NightLights,
    Bloom,
    SoftFog,
    LivingWorld,
    SmoothZoom,
    HdText,
    UiMode,
    /// The cheat panel of the open game.
    Cheats,
    /// «Lingua».
    Language,
    /// «Ricorda le missioni raggiunte».
    CampaignProgress,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Open(Screen),
    Back,
    Campaign(usize),
    Mission(usize),
    Custom(usize),
    Change(OptionKey, i16),
    Difficulty(i16),
    Apply,
    Play,
    RandomSize(i16),
    RandomTerrain(i16),
    RandomVisibility(bool),
    RandomOption(RandomOption, i16),
    Generate,
    /// A save of the list, by its position.
    Slot(usize),
    /// The «NUOVA» row of the save list: a new save.
    NewSave,
    /// The type of game listed on the load page: mission, campaign or demo.
    LoadType(i16),
    LoadSlot,
    SaveSlot,
    DeleteSlot,
    /// Keys of the map: write or read the quick save.
    QuickSave,
    QuickLoad,
    Resume,
    Restart,
    /// «CONTINUA»: the map the script chose.
    NextMap,
    Exit,
    /// Controls of the original that this version shows but does not use.
    Inert,
    /// Start writing the player's name.
    EditName,
    /// Wait for the new key of a command of [`crate::keys::ACTIONS`].
    BindKey(usize),
    /// A tab of the settings.
    Tab(SettingsTab),
    /// A setting given one of its choices, by index.
    Set(OptionKey, i16),
    /// «Predefiniti»: the settings of the tab open as they first are.
    Defaults,
    /// Open or close the list of a setting.
    Dropdown(OptionKey),
    CloseDropdown,
    /// Start writing the frames per second.
    EditFps,
    /// The «?» ball (`COMMONBTN_HELP_B.BMP`): hint mode on or off.
    Help,
    /// Open the setup screen of the selected custom mission.
    Setup,
    /// Next free colour, or next team, of a kingdom row of the setup screen.
    SetupColor(usize),
    SetupTeam(usize),
    SetupRace(usize, i16),
    SetupPlayer(usize),
    SetupOption(SetupOption, i16),
    /// The computer icon of a random map row: its «COMPORTAMENTO IA».
    ChooseAi(usize),
    /// An entry of the «COMPORTAMENTO IA» list, by position.
    AiScript(usize),
    AiAccept,
    AiCancel,
    /// «Resa dell'IA» of the random map screen (`AIRESIGN.BMP`).
    AiResign,
    Scroll {
        target: ScrollTarget,
        delta: i16,
        max: usize,
    },
    ScrollTo {
        target: ScrollTarget,
        offset: usize,
        max: usize,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupOption {
    Players,
    GameType,
    Victory,
    VictoryLimit,
    Population,
    Difficulty,
    Resources,
    NoFog,
    Explored,
    MapDefaults,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RandomOption {
    Kind,
    Resources,
    Guarded,
    Animals,
    Items,
    Cliffs,
    Gateways,
}

/// What the random map screen keeps between games, like the original's
/// `[SINGLE_RANDOM]` (0x4cc2c0). The kingdom rows are not kept: the screen
/// builds them anew each time (0x4c7ac5).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RandomPreferences {
    side: usize,
    terrain: usize,
    players: usize,
    rules: GameRules,
    options: crate::random_map::Options,
    #[serde(default)]
    ai_resign: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Selection {
    Scenario {
        campaign: Option<usize>,
        mission: usize,
    },
    Random {
        side: usize,
        seed: u64,
    },
}
/// Current slot format. Version 1 stored only catalog indices.
pub const SAVE_VERSION: u32 = 3;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedSession {
    #[serde(default)]
    pub snapshot: bool,
    #[serde(default)]
    pub title: String,
    /// The player's name for the save; empty in saves of earlier versions.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// File name in `saves/`, without extension.
    #[serde(skip)]
    pub file: String,
    #[serde(default = "legacy_version")]
    pub version: u32,
    /// Stable scenario id; indices in `selection` move when maps are added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    pub selection: Selection,
    /// Camera centre in map pixels and view scale (map pixels per logical pixel).
    pub camera: [f32; 3],
    /// Colours and teams chosen on the setup screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<GameSetup>,
    /// Set when the saved scenario is no longer installed.
    #[serde(skip)]
    pub missing: bool,
}
/// The saves of `root`, with their scenarios found again in the catalog.
fn list_saves(root: &Path, catalog: &ScenarioCatalog) -> Vec<SavedSession> {
    let mut saves = crate::save::list(root);
    for saved in &mut saves {
        if let Some(id) = &saved.scenario {
            match catalog.locate(id) {
                Some((campaign, mission)) => {
                    saved.selection = Selection::Scenario { campaign, mission }
                }
                None => saved.missing = !saved.snapshot,
            }
        }
    }
    saves
}
/// The campaign missions reached, in the settings' folder: a nonce and the
/// JSON list sealed as the saves are ([`crate::pack::seal`]).
const PROGRESS_FILE: &str = "campaign-progress.rtzp";
/// Their plain form of earlier versions, read when the sealed one is not
/// there and replaced by it at the next mission reached.
const LEGACY_PROGRESS: &str = "campaign-progress.json";
fn progress_context(nonce: &[u8]) -> Vec<u8> {
    [b"progress ".as_slice(), nonce].concat()
}
/// The bytes of [`PROGRESS_FILE`].
fn sealed_progress(reached: &std::collections::BTreeSet<String>) -> Vec<u8> {
    let json = serde_json::to_vec(reached).unwrap_or_default();
    let mut nonce = blake3::Hasher::new();
    nonce.update(&json);
    nonce.update(
        &std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    let hash = nonce.finalize();
    let nonce = &hash.as_bytes()[..16];
    [nonce, &crate::pack::seal(&progress_context(nonce), &json)].concat()
}
/// The missions reached of [`PROGRESS_FILE`], or else of the plain
/// [`LEGACY_PROGRESS`].
fn read_progress(
    sealed: Option<Vec<u8>>,
    legacy: Option<Vec<u8>>,
) -> std::collections::BTreeSet<String> {
    sealed
        .and_then(|b| {
            let (nonce, rest) = b.split_at_checked(16)?;
            crate::pack::unseal(&progress_context(nonce), rest).ok()
        })
        .or(legacy)
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
/// File of the quick save written and read by the map keys.
pub const QUICK_SAVE: &str = "quick";
fn legacy_version() -> u32 {
    1
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
impl Rect {
    pub fn has(self, x: f32, y: f32) -> bool {
        x >= self.x as f32
            && y >= self.y as f32
            && x < (self.x + self.w) as f32
            && y < (self.y + self.h) as f32
    }
}
const fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect { x, y, w, h }
}

/// How a control is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Text button over a strip of `IMAGES\SCREENS\<art>.BMP`.
    Button(&'static str),
    /// A row of a list.
    Item,
    /// A row of the custom map list, with its map-type icon.
    Map(u8),
    /// Arrow of a selector or of a slider.
    Arrow {
        right: bool,
    },
    /// Value of a selector, between its arrows.
    Value,
    /// The settings' slider: groove, fill and the original thumb, its value
    /// (`label`) written beside.
    Slider,
    Check,
    Scroll,
    /// A click area without artwork.
    Hidden,
    /// A text field; `checked` while it takes the keyboard.
    Field,
    /// Name of a kingdom row of the setup screen, written on the row's
    /// own slot; `checked` while it takes the keyboard.
    Name,
    /// Kingdom colour (frame of `COMMONCOLOR BARS.BMP`).
    Swatch(u8),
    /// Team, as the frame of `COMMONTEAMS.BMP`: «1»–«4», then none.
    Team(u8),
    /// The computer icon of a random map row (`COMMONAI.BMP`), lit when
    /// the kingdom has a behaviour of its own.
    AiIcon,
    /// «Resa dell'IA» (`IMAGES\AIRESIGN.BMP`): white flag or swords.
    AiResign,
    /// The «?» ball (its art), red while hint mode is on.
    HelpBall(&'static str),
    /// The key of a command on the keys page; `checked` while it waits.
    KeyBox,
    /// A tab of the settings; `checked` when open.
    Tab,
    /// The label of a row of the settings, `checked` while the pointer is on
    /// the row, lit `band` pixels wide; `dim` when its control is disabled.
    Row {
        dim: bool,
        band: i32,
    },
    /// The title of a group of rows, with its line.
    Heading,
    /// A line of information under the rows.
    Note,
    /// On or off, as a switch; `checked` when on.
    Switch,
    /// One of the choices of a row, side by side, as a stone button pressed
    /// when chosen (`checked`).
    Segment,
    /// A button of the settings, the options' stone at any width.
    Plate,
    /// A number written by hand, in a groove; `checked` while written.
    Edit,
    /// The box of a list, in a groove with its arrow; `checked` while open.
    Dropdown,
    /// A row of an open list; `checked` when chosen.
    DropItem,
}
#[derive(Clone, Debug)]
pub struct Widget {
    pub rect: Rect,
    pub label: String,
    pub action: Action,
    pub enabled: bool,
    pub checked: Option<bool>,
    pub value: Option<u8>,
    /// Key of `LOCALHELP.INI` with the hint, for controls the screen names.
    pub hint: Option<&'static str>,
    pub style: Style,
}
/// Fonts of the original menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// `Arial12B_aa`: labels, lists and texts.
    Small,
    /// `Arial14B_aa`: buttons and dialog titles.
    Big,
    /// `cgb_22b`: the lines of the end window (0x449e9d).
    Title,
}
/// A fixed text of a screen.
#[derive(Clone, Debug)]
pub struct Text {
    pub rect: Rect,
    pub text: String,
    pub face: Face,
    pub color: [u8; 3],
    pub centered: bool,
}

/// Text colours of the original interface (entries 31–35 of its colour table).
pub mod color {
    pub const GOLD: [u8; 3] = [0xe2, 0xa3, 0x19];
    pub const DARK_GOLD: [u8; 3] = [0xa3, 0x7b, 0x1b];
    pub const DISABLED: [u8; 3] = [0x5d, 0x49, 0x11];
    pub const CREAM: [u8; 3] = [0xff, 0xf3, 0xd1];
    pub const GREY: [u8; 3] = [0x60, 0x60, 0x58];
    pub const WHITE: [u8; 3] = [0xff, 0xff, 0xff];
    pub const BLACK: [u8; 3] = [0, 0, 0];
}

/// Height of a list row (Arial12B and its spacing).
pub const ROW: i32 = 16;
/// Rows of the load and save lists.
pub const SAVE_ROWS: usize = 9;
/// Side of the scroll arrows and thumbs (`COMMON\BTN UP.BMP` frames).
pub const ARROW: i32 = 27;
/// Top-left corners of the dialogs on the 800×600 canvas.
pub const PAUSE_AT: (i32, i32) = (254, 93);
pub const FILE_AT: (i32, i32) = (125, 140);
pub const MESSAGE_AT: (i32, i32) = (150, 140);
impl Menu {
    /// The end window, centred on the screen (0x449ced): `back.bmp` of
    /// 500×320, or `back2.bmp` of 500×415 when the script gave a text.
    fn end_at(&self) -> (i32, i32) {
        (150, if self.ending_text.is_empty() { 140 } else { 92 })
    }
    /// The dialogs of the draggable class 0x47eec0: the game menu
    /// (0x47a692), the message window (0x481356, also for leaving and the
    /// instructions) and the end of a game (0x449c87).
    pub fn draggable(screen: Screen) -> bool {
        matches!(
            screen,
            Screen::Pause
                | Screen::Quit
                | Screen::Resign
                | Screen::Help
                | Screen::Multiplayer
                | Screen::End
        )
    }
    /// Where the dialog of the open screen opens: centred.
    fn dialog_centre(&self) -> (i32, i32) {
        match self.screen {
            Screen::Pause => PAUSE_AT,
            Screen::End => self.end_at(),
            _ => MESSAGE_AT,
        }
    }
    /// Where the dialog of the open screen lies, in the pixels of the canvas
    /// as it opened: centred, or where the player dragged it.
    pub fn dialog_place(&self) -> (i32, i32) {
        match self.moved.iter().find(|&&(s, _)| s == self.screen) {
            Some(&(_, at)) => at,
            None => self.dialog_centre(),
        }
    }
    /// Where the dialog is drawn on the canvas. Over the mission the whole
    /// canvas, clear around the dialog, goes with it instead
    /// ([`Menu::canvas_shift`]), so that it can go anywhere on the screen.
    pub fn dialog_at(&self) -> (i32, i32) {
        if self.over_map() {
            self.dialog_centre()
        } else {
            self.dialog_place()
        }
    }
    /// How far the canvas lies from its place for a dialog dragged over the
    /// mission.
    pub fn canvas_shift(&self) -> (i32, i32) {
        if !self.over_map() {
            return (0, 0);
        }
        let ((x, y), (cx, cy)) = (self.dialog_place(), self.dialog_centre());
        (x - cx, y - cy)
    }
    /// The open dialog of `size` dragged so that its top-left corner goes
    /// to `at`, kept with 50 pixels within `screen` (0x47f02b), all in the
    /// pixels of the canvas as it opened; whether it moved.
    pub fn drag_dialog(&mut self, at: (i32, i32), size: (i32, i32), screen: Rect) -> bool {
        if !Self::draggable(self.screen) {
            return false;
        }
        let place = |v: i32, size: i32, from: i32, span: i32| {
            let v = if v < from + 50 - size {
                from + 50 - size
            } else {
                v
            };
            if v > from + span - 50 {
                from + span - 50
            } else {
                v
            }
        };
        let to = (
            place(at.0, size.0, screen.x, screen.w),
            place(at.1, size.1, screen.y, screen.h),
        );
        if to == self.dialog_place() {
            return false;
        }
        let screen = self.screen;
        self.moved.retain(|&(s, _)| s != screen);
        self.moved.push((screen, to));
        self.dirty = true;
        true
    }
}
/// «COMPORTAMENTO IA» (`D_CHOOSEAIBACK.BMP`, 500×320), centred (0x48a122).
pub const CHOOSE_AI_AT: (i32, i32) = (150, 140);
/// Rows of its list (46, 88)–(422, 231).
const AI_ROWS: usize = 9;
/// The selector of the time or score limit (0x4c506d, 0x4c88b8): as wide as
/// its own frame `COMMONSCROLL CTRL.BMP`, which the backgrounds lack.
const VALUE_SELECTOR: Rect = r(66, 541, 191, 27);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollTarget {
    List,
    Description(usize),
}
#[derive(Clone, Copy, Debug)]
pub struct Scrollbar {
    pub target: ScrollTarget,
    /// The whole control: up arrow, groove and down arrow.
    pub track: Rect,
    pub total: usize,
    pub visible: usize,
    pub offset: usize,
    /// False for lists scrolled only with the wheel (the credits).
    pub shown: bool,
}
impl Scrollbar {
    pub fn max(self) -> usize {
        self.total.saturating_sub(self.visible)
    }
    fn travel(self) -> i32 {
        (self.track.h - 3 * ARROW).max(0)
    }
    pub fn up(self) -> Rect {
        Rect {
            h: ARROW,
            ..self.track
        }
    }
    pub fn down(self) -> Rect {
        Rect {
            y: self.track.y + self.track.h - ARROW,
            h: ARROW,
            ..self.track
        }
    }
    /// The original thumb has a fixed size and runs between the arrows.
    pub fn thumb(self) -> Rect {
        let step = (self.travel() as f32 * self.offset.min(self.max()) as f32
            / self.max().max(1) as f32)
            .round() as i32;
        Rect {
            y: self.track.y + ARROW + step,
            h: ARROW,
            ..self.track
        }
    }
    pub fn offset_at(self, y: f32, grab: f32) -> usize {
        let travel = self.travel().max(1) as f32;
        (((y - (self.track.y + ARROW) as f32 - grab) / travel).clamp(0., 1.) * self.max() as f32)
            .round() as usize
    }
}
#[derive(Debug)]
pub enum Effect {
    None,
    Exit,
    SettingsChanged,
    Load(Selection),
    Restore(SavedSession),
    /// Write the open game into `file` (a new one when `None`) under `name`.
    Save {
        file: Option<String>,
        name: String,
    },
    Resume,
    Restart,
    NextMap,
}

/// A kingdom of the setup screen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SetupRow {
    pub record: usize,
    pub name: String,
    pub color: u8,
    /// Team 1–4, or 0 for none. The original keeps 0–3 for the teams and 4
    /// for none, the frames of `TEAMS.BMP` (0x4c6850).
    pub team: u8,
    /// Original combo index: random, European, Arab, Asian.
    pub race: usize,
    pub human: bool,
    /// The behaviour chosen in «COMPORTAMENTO IA» (0x48a060): a script of
    /// `DATA\AI\AI.INI`, empty for the default one. Random maps only.
    #[serde(default)]
    pub ai: String,
}
impl SetupRow {
    /// The `TEAMS.BMP` frame of the team: «1»–«4», then the empty one.
    pub fn team_frame(&self) -> u8 {
        (self.team + 4) % 5
    }
}
/// The «COMPORTAMENTO IA» window opened over the random map screen by the
/// computer icon of a kingdom row (0x4ca6d3 → 0x48a060).
#[derive(Clone, Debug, PartialEq)]
pub struct ChooseAi {
    pub row: usize,
    /// Entry of `Menu::ai_scripts` highlighted in the list.
    pub selected: usize,
}

pub struct Menu {
    pub screen: Screen,
    /// The tab of the settings, kept while the game runs.
    pub tab: SettingsTab,
    pub catalog: ScenarioCatalog,
    pub settings: Settings,
    pub draft: Settings,
    pub campaign: usize,
    pub mission: usize,
    pub custom: usize,
    /// Campaign difficulty: 0 easy, 1 normal, 2 hard.
    pub difficulty: usize,
    /// Saves in `saves/`, newest first, and the one chosen in the list.
    pub saves: Vec<SavedSession>,
    pub save: Option<usize>,
    /// Type listed on the load page: 0 missions, 1 campaigns, 2 demos.
    pub load_type: usize,
    /// Name written in the field of the save page.
    pub save_name: String,
    pub random_side: usize,
    pub random_seed: u64,
    pub random_terrain: usize,
    pub random_terrains: Vec<(String, String)>,
    pub random_rules: GameRules,
    pub random_options: crate::random_map::Options,
    pub random_players: usize,
    pub random_rows: Vec<SetupRow>,
    /// «Resa dell'IA» (`AIRESIGN`): computer kingdoms may surrender.
    pub random_ai_resign: bool,
    /// Whether the installation has the surrender of the computer players,
    /// an addition of the Italian 1.11 (`IMAGES\AIRESIGN.BMP`,
    /// `DATA\AI\RESIGN.AI`): the American release has no such choice.
    pub ai_resign_offered: bool,
    /// Entries of «COMPORTAMENTO IA»: script path and name, the default
    /// first (0x48a780).
    pub ai_scripts: Vec<(String, String)>,
    pub choose_ai: Option<ChooseAi>,
    /// HEROES.INI in the order of the original table (0x499b60): the names
    /// the random map screen gives computer kingdoms.
    hero_names: Vec<String>,
    /// Names already taken since the last game ended (0x4999d0, 0x4999b0).
    heroes_taken: usize,
    pub selection: Option<Selection>,
    pub notice: String,
    pub audio_status: String,
    pub scroll: i32,
    pub description_scroll: [usize; 2],
    pub focus: usize,
    pub hover: Option<usize>,
    pub keyboard_focus: bool,
    pub pressed: Option<usize>,
    pub dirty: bool,
    /// Canvas point where the pointer entered the hovered control: the
    /// original's hint opens below it.
    pub pointer: Option<[f32; 2]>,
    /// The player-name field takes the keyboard.
    pub editing_name: bool,
    /// The frames per second being written, while their field takes the
    /// keyboard.
    pub editing_fps: Option<String>,
    /// The setting whose list is open.
    pub dropdown: Option<OptionKey>,
    /// The command of the keys page waiting for its new key.
    pub capturing: Option<usize>,
    /// Hint mode of the «?» ball: hints under the pointer, buttons inert.
    pub help: bool,
    /// The dialogs the player dragged (0x47ef80), with their top-left
    /// corner: the open one, and the game menu under its own windows.
    pub moved: Vec<(Screen, (i32, i32))>,
    /// The game that ended: won, and whether a next map was chosen.
    pub ending: Option<(bool, bool)>,
    /// The text the script gave the end window (`PlayerWin`, `PlayerLose`).
    pub ending_text: String,
    /// Colours and teams of the game being played, chosen on the setup
    /// screen; `None` keeps the scenario's own.
    pub setup: Option<GameSetup>,
    /// Kingdom rows of the setup screen.
    pub setup_rows: Vec<SetupRow>,
    pub setup_options: GameRules,
    pub setup_map_options: GameRules,
    pub setup_map_races: Vec<usize>,
    pub setup_map_defaults: bool,
    pub setup_players: usize,
    pub setup_min_players: usize,
    setup_custom: Option<(GameRules, Vec<usize>, usize)>,
    setup_preset: GameRules,
    setup_locks: [bool; 4],
    return_options: Screen,
    return_load: Screen,
    pub root: PathBuf,
    /// The installation.
    source: PathBuf,
    /// The ids of the campaign missions reached ([`PROGRESS_FILE`], and
    /// those of the saved games).
    reached: std::collections::BTreeSet<String>,
}
/// What the menus read in the language of the moment.
struct Texts {
    catalog: ScenarioCatalog,
    random_terrains: Vec<(String, String)>,
    ai_scripts: Vec<(String, String)>,
    hero_names: Vec<String>,
    ai_resign_offered: bool,
}
impl Texts {
    fn load(source: &Path) -> Result<Self> {
        let catalog = ScenarioCatalog::load(source)?;
        let assets = GameAssets::open_in(source, crate::lang::current())?;
        let mut random_terrains = Vec::new();
        for name in assets
            .names()
            .iter()
            .filter(|n| n.starts_with("IMAGES/TERRAIN/") && n.ends_with(".TERRAIN.INI"))
        {
            let ini = Ini::parse(&assets.get(name)?);
            let biome = name
                .trim_start_matches("IMAGES/TERRAIN/")
                .trim_end_matches(".TERRAIN.INI")
                .to_ascii_lowercase();
            random_terrains.push((biome, ini.get("terrain", "name").unwrap_or(name).to_owned()));
        }
        random_terrains.sort_by(|a, b| a.0.cmp(&b.0));
        // 0x499b60: the hero table, European, Asian, Arab and common names.
        let heroes = assets.get("DATA/HEROES.INI").unwrap_or_default();
        let hero_names = ["euro", "asia", "arab", "comm"]
            .iter()
            .flat_map(|s| ini_lines(&heroes, s).into_iter().map(|(name, _)| name))
            .collect();
        // 0x48a780: the default behaviour, then the scripts of AI.INI.
        let ai = assets.get("DATA/AI/AI.INI").unwrap_or_default();
        let ai_scripts = std::iter::once((String::new(), String::new()))
            .chain(
                ini_lines(&ai, "scripts")
                    .into_iter()
                    .filter(|(_, file)| !file.is_empty())
                    .map(|(name, file)| (format!("DATA\\AI\\{file}"), name)),
            )
            .collect();
        let ai_resign_offered = ["IMAGES/AIRESIGN.BMP", "DATA/AI/RESIGN.AI"]
            .iter()
            .all(|n| assets.get(n).is_ok());
        Ok(Self {
            catalog,
            random_terrains,
            ai_scripts,
            hero_names,
            ai_resign_offered,
        })
    }
}
impl Menu {
    pub fn new(source: &Path, root: &Path) -> Result<Self> {
        let (settings, warning) = Settings::load_or_default(&root.join("settings.json"));
        // The whole program speaks the language of the settings.
        crate::lang::set(crate::lang::Language::from_index(settings.language));
        let Texts {
            catalog,
            random_terrains,
            ai_scripts,
            hero_names,
            ai_resign_offered,
        } = Texts::load(source)?;
        let original_ini =
            Ini::parse(&crate::vfs::read(&source.join("Tzar.ini")).unwrap_or_default());
        let preset = |key: &str, default: usize| {
            original_ini
                .get("SINGLE_CUSTOM", key)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(default)
        };
        let setup_preset = std::fs::read(root.join("setup-options.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| GameRules {
                game_type: preset("GAMETYPE", 0).min(1),
                victory: preset("VICTCOND", 0).min(3),
                difficulty: preset("DIFFLVL", 0).min(2),
                victory_limit: preset("VALUE", 30).max(30) as u32,
                resource_level: preset("RES", 0).min(2),
                population: (preset("POPLIMIT", 0).min(3) + 1) * 50,
                ..Default::default()
            });
        let native = |key: &str, default: usize| {
            original_ini
                .get("SINGLE_RANDOM", key)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(default)
        };
        let mut random = std::fs::read(root.join("random-options.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<RandomPreferences>(&b).ok())
            .unwrap_or_else(|| RandomPreferences {
                side: RANDOM_SIDES[native("MAPSIZE", 4).min(6)],
                terrain: native("MAPTERR", 0).min(random_terrains.len()),
                players: native("NUMPL", 2).clamp(2, 8),
                rules: GameRules {
                    population: (native("POPLIMIT", 3).min(3) + 1) * 50,
                    resource_level: native("RES", 1).min(2),
                    difficulty: native("DIFFLVL", 1).min(2),
                    game_type: native("GAMETYPE", 0).min(1),
                    victory: native("VICTCOND", 0).min(3),
                    victory_limit: native("VALUE", 30).clamp(30, 2000) as u32,
                    // 0x4c7a18: LASTFOG and LASTEXPL keep the fog and the
                    // unexplored map, both on when absent.
                    no_fog: native("LASTFOG", 1) == 0,
                    explored: native("LASTEXPL", 1) == 0,
                    ..Default::default()
                },
                options: crate::random_map::Options {
                    kind: native("MAPTYPE", 0).min(5),
                    map_resources: native("MAPRES", 1).min(2),
                    guarded: native("MAPGT", 1) != 0,
                    animals: native("MAPANM", 1) != 0,
                    items: native("MAPMIT", 1) != 0,
                    cliffs: native("MAPCLF", 0) != 0,
                    gateways: native("MAPGWAYS", 1) != 0,
                },
                ai_resign: native("AIRESIGN", 0) != 0,
            });
        if !RANDOM_SIDES.contains(&random.side) {
            random.side = 256;
        }
        random.terrain = random.terrain.min(random_terrains.len());
        random.players = random.players.clamp(2, 8);
        random.options.normalize();
        random.rules.population = (random.rules.population / 50).clamp(1, 4) * 50;
        random.rules.resource_level = random.rules.resource_level.min(2);
        random.rules.difficulty = random.rules.difficulty.min(2);
        random.rules.game_type = random.rules.game_type.min(1);
        random.rules.victory = random.rules.victory.min(3);
        let step = if random.rules.victory == 3 { 200 } else { 30 };
        let max = if random.rules.victory == 3 { 2000 } else { 180 };
        random.rules.victory_limit =
            (random.rules.victory_limit / step).clamp(1, max / step) * step;
        let saves = list_saves(&root, &catalog);
        let mut menu = Self {
            screen: Screen::Main,
            catalog,
            draft: settings.clone(),
            settings,
            campaign: 0,
            mission: 0,
            custom: 0,
            difficulty: 1,
            saves,
            save: None,
            load_type: 0,
            save_name: String::new(),
            random_side: random.side,
            random_seed: 1,
            random_terrain: random.terrain,
            random_terrains,
            random_rules: random.rules,
            random_options: random.options,
            random_players: random.players,
            random_rows: Vec::new(),
            random_ai_resign: random.ai_resign && ai_resign_offered,
            ai_resign_offered,
            ai_scripts,
            choose_ai: None,
            hero_names,
            heroes_taken: 0,
            selection: None,
            notice: warning.unwrap_or_default(),
            audio_status: "Inizializzazione dell'audio…".into(),
            scroll: 0,
            description_scroll: [0; 2],
            focus: 0,
            hover: None,
            keyboard_focus: false,
            pressed: None,
            dirty: true,
            editing_name: false,
            editing_fps: None,
            dropdown: None,
            capturing: None,
            help: false,
            moved: Vec::new(),
            pointer: None,
            ending: None,
            ending_text: String::new(),
            setup: None,
            setup_rows: Vec::new(),
            setup_options: GameRules::default(),
            setup_map_options: GameRules::default(),
            setup_map_races: Vec::new(),
            setup_map_defaults: true,
            setup_players: 2,
            setup_min_players: 2,
            setup_custom: None,
            setup_preset,
            setup_locks: [false; 4],
            return_options: Screen::Main,
            tab: SettingsTab::default(),
            return_load: Screen::Single,
            root: root.to_path_buf(),
            source: source.to_path_buf(),
            reached: Default::default(),
        };
        menu.load_progress();
        // Rows for a random map started without its screen; opening the
        // screen builds them again and takes the names.
        menu.reset_random_rows();
        menu.heroes_taken = 0;
        Ok(menu)
    }
    /// The missions the campaign screen lists, by position in the campaign
    /// (0x4225e3): those `MAPS\Maps.ini` opens with «yes». The original's
    /// check for the others (0x4b9d80) always says no, so they are reached
    /// only by winning the one before («CONTINUA»). With «Ricorda le missioni
    /// raggiunte», an addition of this version, the missions already reached
    /// are listed too.
    pub fn campaign_missions(&self) -> Vec<usize> {
        self.catalog
            .campaigns
            .get(self.campaign)
            .map_or_else(Vec::new, |c| {
                c.missions
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| {
                        m.initially_available
                            || (self.settings.campaign_progress && self.reached.contains(&m.id))
                    })
                    .map(|(i, _)| i)
                    .collect()
            })
    }
    /// A campaign mission has been reached: it is remembered.
    pub fn reach(&mut self, id: &str) {
        if self.reached.insert(id.to_owned()) {
            let bytes = sealed_progress(&self.reached);
            if crate::write_atomic(&self.root.join(PROGRESS_FILE), &bytes).is_ok() {
                let _ = std::fs::remove_file(self.root.join(LEGACY_PROGRESS));
            }
        }
    }
    fn load_progress(&mut self) {
        let legacy = std::fs::read(self.root.join(LEGACY_PROGRESS)).ok();
        let plain = legacy.is_some();
        self.reached = read_progress(std::fs::read(self.root.join(PROGRESS_FILE)).ok(), legacy);
        // The plain file of an earlier version is sealed at once.
        if plain
            && crate::write_atomic(
                &self.root.join(PROGRESS_FILE),
                &sealed_progress(&self.reached),
            )
            .is_ok()
        {
            let _ = std::fs::remove_file(self.root.join(LEGACY_PROGRESS));
        }
        // The missions of the saved games were reached too.
        for saved in &self.saves {
            if let (
                Some(id),
                Selection::Scenario {
                    campaign: Some(_), ..
                },
            ) = (&saved.scenario, &saved.selection)
            {
                self.reached.insert(id.clone());
            }
        }
    }
    /// The installation the menus read.
    pub fn source(&self) -> &Path {
        &self.source
    }
    /// The texts that follow the language, read again after it changes.
    pub fn reload_language(&mut self) -> Result<()> {
        let texts = Texts::load(&self.source)?;
        self.catalog = texts.catalog;
        self.random_terrains = texts.random_terrains;
        self.ai_scripts = texts.ai_scripts;
        self.hero_names = texts.hero_names;
        self.saves = list_saves(&self.root, &self.catalog);
        self.dirty = true;
        Ok(())
    }
    /// The kingdom rows the random map screen starts from (0x4c7ac5): colours
    /// in order, random civilizations, default behaviours, the player on
    /// team 1 and every computer on team 2, the computers named after the
    /// first heroes not taken yet (0x4999d0, «Eroe» once all are).
    fn reset_random_rows(&mut self) {
        let rows: Vec<SetupRow> = (0..8)
            .map(|i| SetupRow {
                record: i,
                name: if i == 0 {
                    self.settings.player_name.clone()
                } else {
                    let name = self.hero_names.get(self.heroes_taken).cloned();
                    self.heroes_taken += 1;
                    name.unwrap_or_else(|| "Eroe".into())
                },
                color: i as u8 + 1,
                team: if i == 0 { 1 } else { 2 },
                race: 0,
                human: i == 0,
                ai: String::new(),
            })
            .collect();
        self.random_rows = rows;
        constrain_random(
            self.random_side,
            &mut self.random_players,
            &mut self.random_rows,
            &mut self.random_options,
        );
    }
    /// The stable id of a selection, used by save slots.
    pub fn scenario_id(&self, selection: &Selection) -> Option<String> {
        self.scenario(selection).map(|s| s.id.clone())
    }
    pub fn scenario(&self, selection: &Selection) -> Option<&Scenario> {
        match selection {
            Selection::Scenario {
                campaign: Some(c),
                mission,
            } => self.catalog.campaigns.get(*c)?.missions.get(*mission),
            Selection::Scenario {
                campaign: None,
                mission,
            } => self.catalog.missions.get(*mission),
            _ => None,
        }
    }
    pub fn current_settings(&self) -> &Settings {
        if self.screen.is_settings() {
            &self.draft
        } else {
            &self.settings
        }
    }
    /// Dialogs that the original opens over the running mission: the map and
    /// its panel stay visible around them. The settings opened from the game
    /// menu too: 0x47b700 centres their 800×600 page on the screen, over the
    /// paused game.
    pub fn over_map(&self) -> bool {
        self.selection.is_some()
            && match self.screen {
                Screen::Pause | Screen::Save | Screen::Help | Screen::Resign | Screen::End => true,
                Screen::Load => self.return_load == Screen::Pause,
                _ => self.in_game(),
            }
    }
    /// Key of `LOCAL/HELP.INI` with the hint of a control: the names the
    /// original gives its controls (`SP_CAMPAIGN`, `CN_PLAY`, `O_OK`, …).
    pub fn hint_key(&self, w: &Widget) -> Option<&'static str> {
        if w.hint.is_some() {
            return w.hint;
        }
        Some(match (self.screen, &w.action) {
            (Screen::Single, Action::Open(Screen::Campaign)) => "SP_CAMPAIGN",
            (Screen::Single, Action::Open(Screen::Missions)) => "SP_CUSTOM_MAP",
            (Screen::Single, Action::Open(Screen::Random)) => "SP_RANDOM_MAP",
            (Screen::Single, Action::Open(Screen::Load)) => "SP_LOAD_GAME",
            (Screen::Single, Action::Open(Screen::Main)) => "SP_BACK",
            (Screen::Campaign, Action::Campaign(_)) => "CN_CAMPAIGNS",
            (Screen::Campaign, Action::Mission(_)) => "CN_ENTRY_MISSONS",
            (Screen::Campaign, Action::Play) => "CN_PLAY",
            (Screen::Campaign, Action::Back) => "CN_BACK",
            (Screen::Campaign, Action::Difficulty(_)) => "MPS_DIFFICULTY_LEVEL",
            (
                Screen::Campaign,
                Action::Scroll {
                    target: ScrollTarget::Description(0),
                    ..
                },
            ) => "CN_CAMPAIGN_DESCRIPTION",
            (
                Screen::Campaign,
                Action::Scroll {
                    target: ScrollTarget::Description(_),
                    ..
                },
            ) => "CN_ENTRY_MISSONS_DESCRIPTION",
            (Screen::Missions, Action::Custom(_)) => "CM_MAPS",
            (Screen::Missions, Action::Setup) => "CM_PLAY",
            (Screen::Missions, Action::Back) => "CM_BACK",
            (
                Screen::Missions,
                Action::Scroll {
                    target: ScrollTarget::Description(_),
                    ..
                },
            ) => "CM_MAP_DECRIPTION",
            (Screen::Setup, Action::SetupColor(_)) => "CM1_PLAYER_COLOR",
            (Screen::Setup, Action::SetupTeam(_)) => "CM1_PLAYER_TEAM",
            (Screen::Setup, Action::EditName) => "CM1_PLAYER_NAME",
            (Screen::Setup, Action::Play) => "CM1_PLAY",
            (Screen::Setup, Action::Back) => "CM1_BACK",
            (
                Screen::Setup,
                Action::Scroll {
                    target: ScrollTarget::Description(_),
                    ..
                },
            ) => "CM1_MAP_DECRIPTION",
            (Screen::Random, Action::RandomSize(_)) => "RM_MAP_SIZE",
            (Screen::Random, Action::RandomTerrain(_)) => "RM_TERRAIN",
            (Screen::Random, Action::RandomVisibility(false)) => "RM_NO_FOG",
            (Screen::Random, Action::RandomVisibility(true)) => "RM_EXPLORE",
            (Screen::Random, Action::Generate) => "RM_PLAY",
            (Screen::Random, Action::Back) => "RM_BACK",
            (Screen::Load, Action::Slot(_)) => "LG_SELECT_GAME",
            (Screen::Load, Action::LoadType(_)) => "LG_CHOOSE",
            (Screen::Load, Action::LoadSlot) => "LG_LOAD",
            (Screen::Load, Action::DeleteSlot) => "LG_DELETE",
            (Screen::Load, Action::Back) => "LG_CANCEL",
            (Screen::Save, Action::Slot(_)) => "SG_LIST",
            (Screen::Save, Action::NewSave) => "SG_NEW",
            (Screen::Save, Action::SaveSlot) => "SG_SAVE",
            (Screen::Save, Action::DeleteSlot) => "SG_DELETE",
            (Screen::Save, Action::Back) => "SG_CANCEL",
            (Screen::Pause, Action::Restart) => "SG_RESTART",
            (Screen::Pause, Action::Open(Screen::Resign)) => "SG_QUIT",
            _ => return None,
        })
    }
    /// Characters typed into the player-name field.
    pub fn type_name(&mut self, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()) {
            if self.draft.player_name.chars().count() < NAME_LENGTH {
                self.draft.player_name.push(c);
            }
        }
        self.dirty = true;
    }
    pub fn erase_name(&mut self) {
        self.draft.player_name.pop();
        self.dirty = true;
    }
    /// Leave the name field; an empty name goes back to the default.
    pub fn stop_editing(&mut self) {
        self.editing_name = false;
        if self.draft.player_name.trim().is_empty() {
            self.draft.player_name = default_player_name();
        }
        self.commit_fps();
        self.dirty = true;
    }
    /// Digits typed into the frames' field.
    pub fn type_fps(&mut self, text: &str) {
        if let Some(typed) = &mut self.editing_fps {
            for c in text.chars().filter(char::is_ascii_digit) {
                if typed.len() < 3 {
                    typed.push(c);
                }
            }
            self.dirty = true;
        }
    }
    pub fn erase_fps(&mut self) {
        if let Some(typed) = &mut self.editing_fps {
            typed.pop();
            self.dirty = true;
        }
    }
    /// The frames written become the cap: 0 none, else within
    /// [`FPS_RANGE`]; nothing written keeps the one before.
    pub fn commit_fps(&mut self) {
        if let Some(typed) = self.editing_fps.take() {
            if let Ok(fps) = typed.parse::<u32>() {
                self.draft.max_fps = if fps == 0 {
                    0
                } else {
                    fps.clamp(FPS_RANGE.0, FPS_RANGE.1)
                };
            }
            self.dirty = true;
        }
    }
    /// Give the captured command the key `name`. A command that had the same
    /// key is left unbound, so a key never does two things.
    pub fn bind_key(&mut self, name: &str) {
        let Some(i) = self.capturing.take() else {
            return;
        };
        let id = crate::keys::ACTIONS[i].0;
        for (other, _) in crate::keys::ACTIONS {
            if other != id && self.draft.key(other) == name {
                self.draft.keys.insert(other.into(), String::new());
            }
        }
        self.draft.keys.insert(id.into(), name.into());
        self.draft
            .keys
            .retain(|action, key| *key != crate::keys::default_key(action));
        self.dirty = true;
    }
    pub fn cancel_capture(&mut self) {
        self.capturing = None;
        self.dirty = true;
    }
    /// The settings pages were opened from the menu of an open game.
    pub fn in_game(&self) -> bool {
        self.screen.is_settings() && self.return_options == Screen::Pause
    }
    pub fn open(&mut self, screen: Screen) {
        self.editing_name = false;
        self.editing_fps = None;
        self.dropdown = None;
        self.capturing = None;
        self.help = false;
        // A dialog opens centred; the game menu keeps its place under the
        // windows it opens over itself.
        if screen != self.screen {
            let over_menu = self.selection.is_some()
                && matches!(
                    screen,
                    Screen::Pause
                        | Screen::Help
                        | Screen::Resign
                        | Screen::Load
                        | Screen::Save
                        | Screen::Options
                );
            self.moved.retain(|&(s, _)| over_menu && s == Screen::Pause);
        } else if Self::draggable(screen) {
            // The same dialog opened again is a new window.
            self.moved.clear();
        }
        if screen.is_settings() && !self.screen.is_settings() {
            self.return_options = self.screen;
            self.draft = self.settings.clone();
        }
        // The load page is reached from the single-player page or from the
        // pause menu of an open map; Back returns to where it came from.
        if screen == Screen::Load && self.screen != Screen::Load {
            self.return_load = if self.screen == Screen::Pause {
                Screen::Pause
            } else {
                Screen::Single
            };
        }
        if screen == Screen::Random {
            self.draft = self.settings.clone();
            // Like its constructor, every visit starts from new rows.
            if self.screen != Screen::Random {
                self.reset_random_rows();
            }
        }
        self.choose_ai = None;
        // The save page opens on a new save named after the open scenario.
        if matches!(screen, Screen::Load | Screen::Save) && self.screen != screen {
            self.saves = list_saves(&self.root, &self.catalog);
            self.save = None;
            self.save_name = self.default_save_name();
        }
        // A mission no longer listed (the setting changed) is not chosen.
        if screen == Screen::Campaign && !self.campaign_missions().contains(&self.mission) {
            self.mission = self.campaign_missions().first().copied().unwrap_or(0);
        }
        self.screen = screen;
        if screen == Screen::Load && self.save.is_none() {
            self.save = self.listed_saves().first().copied();
        }
        self.focus = 0;
        self.hover = None;
        self.pressed = None;
        self.scroll = 0;
        self.description_scroll = [0; 2];
        self.notice.clear();
        self.dirty = true;
    }
    pub fn allows_action(&self, action: &Action) -> bool {
        // In hint mode the original (0x45c340) takes every input: a click
        // does nothing, scroll bars included, but on the «?» ball.
        !self.help || *action == Action::Help
    }
    pub fn activate(&mut self, action: Action) -> Result<Effect> {
        // The settings work as soon as they are changed: what lies outside
        // the menu (window, language, the open game) follows each change of
        // the page, and its closing with «Annulla» brings back the old.
        let before = self
            .screen
            .is_settings()
            .then(|| self.current_settings().outside());
        let effect = self.act(action)?;
        // Back in the game the menu window is gone: it opens centred again.
        if matches!(effect, Effect::Resume) {
            self.moved.clear();
        }
        if matches!(effect, Effect::None)
            && before.is_some_and(|b| b != self.current_settings().outside())
        {
            return Ok(Effect::SettingsChanged);
        }
        Ok(effect)
    }
    fn act(&mut self, action: Action) -> Result<Effect> {
        if !self.allows_action(&action) {
            return Ok(Effect::None);
        }
        self.notice.clear();
        self.dirty = true;
        self.editing_name = action == Action::EditName;
        if action != Action::EditFps {
            if action == Action::Back && self.editing_fps.is_some() {
                self.editing_fps = None;
                return Ok(Effect::None);
            }
            self.commit_fps();
        }
        // An open list closes with Esc or a click elsewhere.
        if !matches!(action, Action::Dropdown(_))
            && self.dropdown.take().is_some()
            && matches!(action, Action::Back | Action::CloseDropdown)
        {
            return Ok(Effect::None);
        }
        // The frames' limit rests while the vertical synchronization holds
        // them to the screen.
        if self.screen.is_settings()
            && self.draft.vsync
            && matches!(
                action,
                Action::EditFps | Action::Change(OptionKey::Frames, _)
            )
        {
            return Ok(Effect::None);
        }
        // Esc closes «COMPORTAMENTO IA» like its «Annulla».
        if self.choose_ai.is_some() && action == Action::Back {
            self.choose_ai = None;
            return Ok(Effect::None);
        }
        if self.screen == Screen::Random && self.random_setup_action(&action) {
            return Ok(Effect::None);
        }
        let effect = match action {
            Action::BindKey(i) => {
                self.capturing = (i < crate::keys::ACTIONS.len()).then_some(i);
                Effect::None
            }
            Action::Defaults => {
                self.settings_defaults();
                Effect::None
            }
            Action::Tab(tab) => {
                self.tab = tab;
                self.capturing = None;
                Effect::None
            }
            Action::Dropdown(k) => {
                self.dropdown = (self.dropdown != Some(k)).then_some(k);
                Effect::None
            }
            Action::CloseDropdown => Effect::None,
            Action::EditFps => {
                self.editing_fps = Some(String::new());
                Effect::None
            }
            Action::Set(OptionKey::Resolution, v) => {
                if let Some(&(w, h)) = RESOLUTIONS.get(v.max(0) as usize) {
                    self.draft.window = [w, h];
                }
                Effect::None
            }
            Action::Set(k, v) => {
                let delta = v - self.option_value(k);
                if delta != 0 {
                    return self.act(Action::Change(k, delta));
                }
                Effect::None
            }
            Action::Open(s) => {
                self.open(s);
                Effect::None
            }
            Action::Back => {
                if self.screen == Screen::Random {
                    self.save_random()?;
                }
                if self.screen == Screen::Setup && !self.setup_map_defaults {
                    self.setup_preset = self.setup_options.clone();
                    crate::write_atomic(
                        &self.root.join("setup-options.json"),
                        &serde_json::to_vec_pretty(&self.setup_preset)?,
                    )?;
                }
                let to = match self.screen {
                    Screen::Main => Screen::Quit,
                    Screen::Single | Screen::Credits | Screen::Multiplayer | Screen::Quit => {
                        Screen::Main
                    }
                    Screen::Options => self.return_options,
                    Screen::Pause | Screen::End => return Ok(Effect::Resume),
                    Screen::Save | Screen::Help | Screen::Resign => Screen::Pause,
                    Screen::Load => self.return_load,
                    Screen::Setup => Screen::Missions,
                    _ => Screen::Single,
                };
                self.open(to);
                Effect::None
            }
            Action::Campaign(i) => {
                if i < self.catalog.campaigns.len() {
                    self.campaign = i;
                    self.scroll = 0;
                    self.description_scroll = [0; 2];
                    self.mission = self.campaign_missions().first().copied().unwrap_or(0);
                }
                Effect::None
            }
            Action::Mission(i) => {
                if self.campaign_missions().contains(&i) {
                    self.mission = i;
                    self.description_scroll[1] = 0;
                }
                Effect::None
            }
            Action::Custom(i) => {
                if i < self.catalog.missions.len() {
                    self.custom = i;
                    self.description_scroll[1] = 0;
                }
                Effect::None
            }
            Action::Change(k, delta) => {
                let s = &mut self.draft;
                let bump = |v: &mut u8| *v = (*v as i16 + delta).clamp(0, 100) as u8;
                let cycle = |v: &mut usize, n: usize| {
                    *v = (*v as i16 + delta).rem_euclid(n as i16) as usize;
                };
                match k {
                    OptionKey::Sound => bump(&mut s.sound),
                    OptionKey::Music => bump(&mut s.music),
                    OptionKey::Scroll => bump(&mut s.scroll_speed),
                    OptionKey::GameSpeed => {
                        // The slider shows the five native levels.
                        let v = (s.speed as i16 * 5 + delta).clamp(0, 100);
                        s.speed = ((v + 12) / 25 * 5) as u8;
                    }
                    OptionKey::SoundOn => s.sound_on = !s.sound_on,
                    OptionKey::MusicOn => s.music_on = !s.music_on,
                    OptionKey::Speech => s.speech = !s.speech,
                    OptionKey::BuildingSounds => s.building_sounds = !s.building_sounds,
                    OptionKey::Ambience => s.ambience = !s.ambience,
                    OptionKey::Weather => s.weather = !s.weather,
                    OptionKey::DayNight => s.day_night = !s.day_night,
                    OptionKey::SystemCursor => s.system_cursor = !s.system_cursor,
                    OptionKey::Frames => {
                        let v = (fps_slider(s.max_fps) as i16 + delta).clamp(0, 100);
                        s.max_fps = slider_fps(v as u8);
                    }
                    OptionKey::Vsync => s.vsync = !s.vsync,
                    OptionKey::Layout => cycle(&mut s.layout, 2),
                    OptionKey::Resolution => {
                        let mut at = RESOLUTIONS
                            .iter()
                            .position(|&(w, h)| [w, h] == s.window)
                            .unwrap_or(0);
                        cycle(&mut at, RESOLUTIONS.len());
                        s.window = [RESOLUTIONS[at].0, RESOLUTIONS[at].1];
                    }
                    OptionKey::UiMode => cycle(&mut s.ui_mode, UI_MODES.len()),
                    OptionKey::Fullscreen => s.fullscreen = !s.fullscreen,
                    OptionKey::Edge => s.edge_scroll = !s.edge_scroll,
                    OptionKey::Grid => {
                        let mut at = if s.placement_grid {
                            s.grid_strength + 1
                        } else {
                            0
                        };
                        cycle(&mut at, GRID_STRENGTHS.len() + 1);
                        s.placement_grid = at > 0;
                        if at > 0 {
                            s.grid_strength = at - 1;
                        }
                    }
                    OptionKey::MapText => cycle(&mut s.map_text, MAP_TEXTS.len()),
                    OptionKey::TrueColor => s.true_color = !s.true_color,
                    OptionKey::PixelFilter => cycle(&mut s.pixel_filter, PIXEL_FILTERS.len()),
                    OptionKey::Crt => s.crt = !s.crt,
                    OptionKey::NightLights => s.night_lights = !s.night_lights,
                    OptionKey::Bloom => s.bloom = !s.bloom,
                    OptionKey::SoftFog => s.soft_fog = !s.soft_fog,
                    OptionKey::LivingWorld => s.living_world = !s.living_world,
                    OptionKey::SmoothZoom => s.smooth_zoom = !s.smooth_zoom,
                    OptionKey::HdText => s.hd_text = !s.hd_text,
                    OptionKey::Cheats => s.cheats = !s.cheats,
                    OptionKey::Language => cycle(&mut s.language, crate::lang::LANGUAGES.len()),
                    OptionKey::CampaignProgress => s.campaign_progress = !s.campaign_progress,
                }
                Effect::None
            }
            Action::Difficulty(d) => {
                self.difficulty = (self.difficulty as i16 + d).rem_euclid(3) as usize;
                Effect::None
            }
            Action::Apply => {
                self.draft.normalize();
                self.draft.save(&self.root.join("settings.json"))?;
                self.settings = self.draft.clone();
                self.open(self.return_options);
                Effect::SettingsChanged
            }
            Action::Play => {
                let selection = if self.screen == Screen::Campaign {
                    Selection::Scenario {
                        campaign: Some(self.campaign),
                        mission: self.mission,
                    }
                } else {
                    Selection::Scenario {
                        campaign: None,
                        mission: self.custom,
                    }
                };
                if self.scenario(&selection).is_none() {
                    return Err("Nessuna missione selezionata".into());
                }
                self.setup = if self.screen == Screen::Setup {
                    if !self.setup_map_defaults {
                        self.setup_preset = self.setup_options.clone();
                        crate::write_atomic(
                            &self.root.join("setup-options.json"),
                            &serde_json::to_vec_pretty(&self.setup_preset)?,
                        )?;
                    }
                    // The name written on the setup screen becomes the player's.
                    if self.draft.player_name != self.settings.player_name {
                        let mut settings = self.settings.clone();
                        settings.player_name = self.draft.player_name.clone();
                        settings.normalize();
                        settings.save(&self.root.join("settings.json"))?;
                        self.settings = settings;
                    }
                    Some(GameSetup {
                        custom_start: !self.setup_map_defaults,
                        kingdoms: self
                            .setup_rows
                            .iter()
                            .enumerate()
                            .map(|(i, r)| KingdomChoice {
                                record: r.record,
                                color: r.color,
                                team: r.team,
                                civilization: (!self.setup_map_defaults
                                    && self.setup_map_races.get(i).copied() != Some(r.race))
                                .then(|| {
                                    if r.race == 0 {
                                        // Resolve random only at launch, so save/restart keep it.
                                        let seed = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .map_or(1, |d| d.as_nanos() as u64);
                                        (seed.wrapping_mul(1664525).wrapping_add(r.record as u64)
                                            % 3
                                            + 1) as u8
                                    } else {
                                        r.race as u8
                                    }
                                }),
                                ..Default::default()
                            })
                            .collect(),
                        options: Some(self.setup_options.clone()),
                        players: (!self.setup_map_defaults).then_some(self.setup_players),
                        human: self.setup_rows.iter().find(|r| r.human).map(|r| r.record),
                        // Original custom-map loader 0x4403cf: starting-resource
                        // presets override stocks only in King of the Hill.
                        resources: (self.setup_options.game_type == 1)
                            .then(|| match self.setup_options.resource_level {
                                1 => Some([5000; 4]),
                                2 => Some([0x1fff_ffff; 4]),
                                _ => None,
                            })
                            .flatten(),
                        heroes_taken: self.heroes_taken,
                        ..Default::default()
                    })
                } else if self.screen == Screen::Campaign {
                    let scenario = self.scenario(&selection).unwrap();
                    let mut rules =
                        GameRules::from_header(&scenario.container()?.get(&scenario.header)?);
                    rules.difficulty = self.difficulty;
                    Some(GameSetup {
                        options: Some(rules),
                        heroes_taken: self.heroes_taken,
                        ..Default::default()
                    })
                } else {
                    None
                };
                self.selection = Some(selection.clone());
                Effect::Load(selection)
            }
            Action::RandomSize(d) => {
                let i = RANDOM_SIDES
                    .iter()
                    .position(|&s| s == self.random_side)
                    .unwrap_or(1);
                self.random_side =
                    RANDOM_SIDES[(i as i16 + d).rem_euclid(RANDOM_SIDES.len() as i16) as usize];
                constrain_random(
                    self.random_side,
                    &mut self.random_players,
                    &mut self.random_rows,
                    &mut self.random_options,
                );
                Effect::None
            }
            Action::Generate => {
                constrain_random(
                    self.random_side,
                    &mut self.random_players,
                    &mut self.random_rows,
                    &mut self.random_options,
                );
                // Like the original, every random map is a new one.
                self.random_seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(1, |d| d.as_nanos() as u64 % 1_000_000_007 + 1);
                let selection = Selection::Random {
                    side: self.random_side,
                    seed: self.random_seed,
                };
                let index = if self.random_terrain == 0 {
                    (self.random_seed as usize) % self.random_terrains.len().max(1)
                } else {
                    self.random_terrain - 1
                };
                self.stop_editing();
                self.settings.player_name = self
                    .draft
                    .player_name
                    .trim()
                    .chars()
                    .take(NAME_LENGTH)
                    .collect();
                self.settings.save(&self.root.join("settings.json"))?;
                self.save_random()?;
                self.setup = Some(GameSetup {
                    options: Some(self.random_rules.clone()),
                    random_biome: self.random_terrains.get(index).map(|t| t.0.clone()),
                    random_generation: Some(self.random_options.clone()),
                    players: Some(self.random_players),
                    human: self.random_rows.iter().position(|r| r.human),
                    // 0x4ca9c4: the rows' names, colours, civilizations,
                    // teams and behaviours become the kingdoms'.
                    kingdoms: self
                        .random_rows
                        .iter()
                        .map(|r| KingdomChoice {
                            record: r.record,
                            color: r.color,
                            team: r.team,
                            civilization: Some(r.race as u8),
                            name: Some(if r.human {
                                self.settings.player_name.clone()
                            } else {
                                r.name.clone()
                            }),
                            ai: r.ai.clone(),
                        })
                        .collect(),
                    ai_resign: self.random_ai_resign,
                    heroes_taken: self.heroes_taken,
                    ..Default::default()
                });
                self.selection = Some(selection.clone());
                Effect::Load(selection)
            }
            Action::RandomOption(key, d) => {
                let o = &mut self.random_options;
                match key {
                    RandomOption::Kind => {
                        o.kind = (o.kind as i16 + d).rem_euclid(6) as usize;
                        while !random_kind_allowed(self.random_side, o.kind) {
                            o.kind =
                                (o.kind as i16 + if d < 0 { -1 } else { 1 }).rem_euclid(6) as usize;
                        }
                    }
                    RandomOption::Resources => {
                        o.map_resources = (o.map_resources as i16 + d).rem_euclid(3) as usize
                    }
                    RandomOption::Guarded => o.guarded = !o.guarded,
                    RandomOption::Animals => o.animals = !o.animals,
                    RandomOption::Items => o.items = !o.items,
                    RandomOption::Cliffs => o.cliffs = !o.cliffs,
                    RandomOption::Gateways => o.gateways = !o.gateways,
                }
                Effect::None
            }
            Action::RandomTerrain(d) => {
                self.random_terrain = (self.random_terrain as i16 + d)
                    .rem_euclid(self.random_terrains.len() as i16 + 1)
                    as usize;
                Effect::None
            }
            Action::RandomVisibility(explore) => {
                if explore {
                    self.random_rules.explored = !self.random_rules.explored;
                } else {
                    self.random_rules.no_fog = !self.random_rules.no_fog;
                }
                Effect::None
            }
            Action::Slot(i) => {
                if i < self.saves.len() {
                    self.save = Some(i);
                    if self.screen == Screen::Save {
                        self.save_name = self.save_label(&self.saves[i]);
                    }
                }
                Effect::None
            }
            Action::LoadType(d) => {
                self.load_type = (self.load_type as i16 + d).rem_euclid(3) as usize;
                self.save = self.listed_saves().first().copied();
                self.scroll = 0;
                Effect::None
            }
            Action::NewSave => {
                self.save = None;
                self.save_name = self.default_save_name();
                Effect::None
            }
            Action::LoadSlot => match self.save.and_then(|i| self.saves.get(i)).cloned() {
                Some(saved) => self.restore(saved)?,
                None => Effect::None,
            },
            Action::QuickLoad => {
                self.refresh_saves();
                match self.saves.iter().find(|s| s.file == QUICK_SAVE).cloned() {
                    Some(saved) => self.restore(saved)?,
                    None => Effect::None,
                }
            }
            Action::SaveSlot => {
                let name = self.save_name.trim();
                Effect::Save {
                    file: self
                        .save
                        .and_then(|i| self.saves.get(i))
                        .map(|s| s.file.clone()),
                    name: if name.is_empty() {
                        self.default_save_name()
                    } else {
                        name.to_owned()
                    },
                }
            }
            Action::QuickSave => Effect::Save {
                file: Some(QUICK_SAVE.into()),
                name: String::new(),
            },
            Action::DeleteSlot => {
                if let Some(saved) = self.save.and_then(|i| self.saves.get(i)) {
                    crate::save::delete(&self.root, &saved.file)?;
                    self.refresh_saves();
                }
                Effect::None
            }
            Action::Resume => Effect::Resume,
            Action::Restart => Effect::Restart,
            Action::NextMap => Effect::NextMap,
            Action::Exit => Effect::Exit,
            Action::Inert | Action::EditName => Effect::None,
            // Controls of the random map screen, handled above.
            Action::ChooseAi(_)
            | Action::AiScript(_)
            | Action::AiAccept
            | Action::AiCancel
            | Action::AiResign => Effect::None,
            Action::Help => {
                self.help = !self.help;
                Effect::None
            }
            Action::Setup => {
                self.setup_rows = self.setup_rows()?;
                let m = &self.catalog.missions[self.custom];
                let header = m.container()?.get(&m.header)?;
                self.setup_options = GameRules::from_header(&header);
                self.setup_locks = [
                    header.get(0x126) == Some(&3),
                    header.get(0x127) == Some(&4),
                    header.get(0x50) == Some(&3),
                    header.get(0x128..0x12a) == Some(&[0, 0]),
                ];
                self.setup_map_options = self.setup_options.clone();
                self.setup_map_races = self.setup_rows.iter().map(|r| r.race).collect();
                self.setup_players = self.setup_rows.len();
                self.setup_min_players =
                    (m.players[0] as usize).clamp(1, self.setup_players.max(1));
                self.setup_map_defaults = true;
                self.setup_custom = None;
                self.open(Screen::Setup);
                // The human's name can be changed here, as in the options.
                self.draft = self.settings.clone();
                Effect::None
            }
            // 0x4c6b01: with "Dettagli predefiniti" the colour and team
            // buttons of every row are disabled, like the civilizations.
            Action::SetupColor(i) => {
                if !self.setup_map_defaults
                    && i < self.setup_players
                    && let Some(row) = self.setup_rows.get(i)
                {
                    let old = row.color;
                    let next = old % 8 + 1;
                    // 0x4c635e: cycling onto an occupied colour swaps colours.
                    for (j, r) in self.setup_rows.iter_mut().enumerate() {
                        if j == i {
                            r.color = next;
                        } else if r.color == next {
                            r.color = old;
                        }
                    }
                }
                Effect::None
            }
            Action::SetupTeam(i) => {
                if !self.setup_map_defaults
                    && i < self.setup_players
                    && let Some(row) = self.setup_rows.get_mut(i)
                {
                    row.team = (row.team + 1) % (crate::world::TEAMS + 1);
                }
                Effect::None
            }
            Action::SetupPlayer(i) => {
                if i < self.setup_players {
                    for (j, r) in self.setup_rows.iter_mut().enumerate() {
                        r.human = i == j;
                    }
                }
                Effect::None
            }
            Action::SetupRace(i, d) => {
                if !self.setup_map_defaults
                    && i < self.setup_players
                    && let Some(row) = self.setup_rows.get_mut(i)
                {
                    row.race = (row.race as i16 + d).rem_euclid(4) as usize;
                }
                Effect::None
            }
            Action::SetupOption(key, d) => {
                use SetupOption as K;
                let cycle = |v: usize, n: usize| (v as i16 + d).rem_euclid(n as i16) as usize;
                if key == K::MapDefaults {
                    self.setup_map_defaults = !self.setup_map_defaults;
                    if self.setup_map_defaults {
                        self.setup_preset = self.setup_options.clone();
                        crate::write_atomic(
                            &self.root.join("setup-options.json"),
                            &serde_json::to_vec_pretty(&self.setup_preset)?,
                        )?;
                        self.setup_custom = Some((
                            self.setup_options.clone(),
                            self.setup_rows.iter().map(|r| r.race).collect(),
                            self.setup_players,
                        ));
                        self.setup_options = self.setup_map_options.clone();
                        self.setup_players = self.setup_rows.len();
                        // 0x4c6f90: the rows come back from the map, colour,
                        // civilization and human kingdom, every team none.
                        let map = self.setup_rows()?;
                        for (r, m) in self.setup_rows.iter_mut().zip(map) {
                            r.color = m.color;
                            r.team = m.team;
                            r.race = m.race;
                            r.human = m.human;
                        }
                    } else if let Some((options, _, players)) = self.setup_custom.take() {
                        // 0x4c7300 reloads the custom options; the rows stay.
                        let visibility = (self.setup_options.no_fog, self.setup_options.explored);
                        self.setup_options = options;
                        self.setup_options.no_fog = visibility.0;
                        self.setup_options.explored = visibility.1;
                        self.setup_players = players;
                        if self
                            .setup_rows
                            .iter()
                            .position(|r| r.human)
                            .is_some_and(|i| i >= players)
                        {
                            for (i, r) in self.setup_rows.iter_mut().enumerate() {
                                r.human = i == 0;
                            }
                        }
                    } else {
                        let visibility = (self.setup_options.no_fog, self.setup_options.explored);
                        self.setup_options = self.setup_preset.clone();
                        self.setup_options.no_fog = visibility.0;
                        self.setup_options.explored = visibility.1;
                    }
                } else if self.setup_option_enabled(key) {
                    let o = &mut self.setup_options;
                    match key {
                        K::Players => {
                            self.setup_players = self.setup_min_players
                                + cycle(
                                    self.setup_players - self.setup_min_players,
                                    self.setup_rows.len() - self.setup_min_players + 1,
                                );
                            if self
                                .setup_rows
                                .iter()
                                .position(|r| r.human)
                                .is_some_and(|i| i >= self.setup_players)
                            {
                                for (i, r) in self.setup_rows.iter_mut().enumerate() {
                                    r.human = i == 0;
                                }
                            }
                        }
                        K::GameType => o.game_type = cycle(o.game_type, 2),
                        K::Victory => {
                            o.victory = cycle(o.victory, 4);
                            o.victory_limit = if o.victory == 3 { 200 } else { 30 };
                        }
                        K::VictoryLimit if o.victory >= 2 => {
                            let (step, count) = if o.victory == 2 { (30, 6) } else { (200, 10) };
                            o.victory_limit =
                                (cycle((o.victory_limit as usize / step).saturating_sub(1), count)
                                    + 1) as u32
                                    * step as u32;
                        }
                        K::Population => {
                            o.population =
                                (cycle((o.population / 50).saturating_sub(1), 4) + 1) * 50
                        }
                        K::Difficulty => o.difficulty = cycle(o.difficulty, 3),
                        K::Resources => o.resource_level = cycle(o.resource_level, 4),
                        K::NoFog => o.no_fog = !o.no_fog,
                        K::Explored => o.explored = !o.explored,
                        _ => {}
                    }
                }
                Effect::None
            }
            Action::Scroll { target, delta, max } => {
                let offset = match target {
                    ScrollTarget::List => self.scroll.max(0) as usize,
                    ScrollTarget::Description(panel) => {
                        *self.description_scroll.get(panel).unwrap_or(&0)
                    }
                };
                self.set_scroll(
                    target,
                    (offset as i64 + delta as i64).clamp(0, max as i64) as usize,
                );
                Effect::None
            }
            Action::ScrollTo {
                target,
                offset,
                max,
            } => {
                self.set_scroll(target, offset.min(max));
                Effect::None
            }
        };
        // 0x4999b0 frees the hero names when a game ends; a game starting
        // here ends the menu's.
        if matches!(effect, Effect::Load(_)) {
            self.heroes_taken = 0;
        }
        Ok(effect)
    }
    fn save_random(&self) -> Result<()> {
        crate::write_atomic(
            &self.root.join("random-options.json"),
            &serde_json::to_vec_pretty(&RandomPreferences {
                side: self.random_side,
                terrain: self.random_terrain,
                players: self.random_players,
                rules: self.random_rules.clone(),
                options: self.random_options.clone(),
                ai_resign: self.random_ai_resign,
            })?,
        )
    }
    fn random_setup_action(&mut self, action: &Action) -> bool {
        match *action {
            Action::SetupColor(i) if i < self.random_players => {
                let old = self.random_rows[i].color;
                let next = old % 8 + 1;
                for (j, r) in self.random_rows.iter_mut().enumerate() {
                    if j == i {
                        r.color = next;
                    } else if r.color == next {
                        r.color = old;
                    }
                }
            }
            Action::SetupTeam(i) if i < self.random_players => {
                self.random_rows[i].team = (self.random_rows[i].team + 1) % 5
            }
            // 0x4cbbc0: the player's row and the clicked one trade places,
            // name, colour, civilization, team and behaviour included.
            Action::SetupPlayer(i) if i < self.random_players => {
                if let Some(h) = self.random_rows.iter().position(|r| r.human) {
                    self.random_rows.swap(h, i);
                    let (a, b) = (self.random_rows[h].record, self.random_rows[i].record);
                    self.random_rows[h].record = b;
                    self.random_rows[i].record = a;
                }
            }
            Action::SetupRace(i, d) if i < self.random_players => {
                self.random_rows[i].race =
                    (self.random_rows[i].race as i16 + d).rem_euclid(4) as usize
            }
            Action::SetupOption(key, d) => {
                use SetupOption as K;
                let cycle = |v: usize, n: usize| (v as i16 + d).rem_euclid(n as i16) as usize;
                let o = &mut self.random_rules;
                match key {
                    K::Players => {
                        self.random_players = 2 + cycle(
                            self.random_players.saturating_sub(2),
                            random_player_limit(self.random_side) - 1,
                        );
                    }
                    K::GameType => o.game_type = cycle(o.game_type, 2),
                    K::Victory => {
                        o.victory = cycle(o.victory, 4);
                        o.victory_limit = if o.victory == 3 { 200 } else { 30 };
                    }
                    K::VictoryLimit if o.victory >= 2 => {
                        let (step, count) = if o.victory == 2 { (30, 6) } else { (200, 10) };
                        o.victory_limit =
                            (cycle((o.victory_limit as usize / step).saturating_sub(1), count) + 1)
                                as u32
                                * step as u32;
                    }
                    K::Population => {
                        o.population = (cycle((o.population / 50).saturating_sub(1), 4) + 1) * 50
                    }
                    K::Difficulty => o.difficulty = cycle(o.difficulty, 3),
                    K::Resources => o.resource_level = cycle(o.resource_level.min(2), 3),
                    K::NoFog => o.no_fog = !o.no_fog,
                    K::Explored => o.explored = !o.explored,
                    _ => {}
                }
            }
            Action::ChooseAi(i) if i < self.random_players => {
                let selected = self
                    .ai_scripts
                    .iter()
                    .position(|(path, _)| *path == self.random_rows[i].ai)
                    .unwrap_or(0);
                self.choose_ai = Some(ChooseAi { row: i, selected });
                self.scroll = 0;
                self.focus = 0;
                self.hover = None;
            }
            Action::AiScript(k) if k < self.ai_scripts.len() => {
                if let Some(d) = &mut self.choose_ai {
                    d.selected = k;
                }
            }
            // 0x5f5cc53: the chosen script becomes the row's.
            Action::AiAccept => {
                if let Some(d) = self.choose_ai.take()
                    && let Some((path, _)) = self.ai_scripts.get(d.selected)
                {
                    self.random_rows[d.row].ai = path.clone();
                }
                self.hover = None;
            }
            Action::AiCancel => {
                self.choose_ai = None;
                self.hover = None;
            }
            // 0x4ca82a.
            Action::AiResign => self.random_ai_resign = !self.random_ai_resign,
            Action::SetupColor(_)
            | Action::SetupTeam(_)
            | Action::SetupPlayer(_)
            | Action::SetupRace(_, _)
            | Action::ChooseAi(_)
            | Action::AiScript(_) => {}
            _ => return false,
        }
        constrain_random(
            self.random_side,
            &mut self.random_players,
            &mut self.random_rows,
            &mut self.random_options,
        );
        true
    }
    fn setup_option_enabled(&self, key: SetupOption) -> bool {
        use SetupOption as K;
        if matches!(key, K::MapDefaults | K::NoFog | K::Explored) {
            return true;
        }
        if self.setup_map_defaults {
            return false;
        }
        match key {
            K::Players => self.setup_rows.len() > self.setup_min_players,
            K::GameType => !self.setup_locks[0],
            K::Victory | K::VictoryLimit => !self.setup_locks[1],
            K::Difficulty => !self.setup_locks[2],
            K::Population => !self.setup_locks[3],
            _ => true,
        }
    }
    fn set_scroll(&mut self, target: ScrollTarget, offset: usize) {
        match target {
            ScrollTarget::List => self.scroll = offset.min(i32::MAX as usize) as i32,
            ScrollTarget::Description(panel) => {
                if let Some(v) = self.description_scroll.get_mut(panel) {
                    *v = offset;
                }
            }
        }
    }
    /// The name of a save in the load and save lists: the player's name, or
    /// for saves of earlier versions the title of the game.
    pub fn save_label(&self, s: &SavedSession) -> String {
        if !s.name.is_empty() {
            return s.name.clone();
        }
        if s.missing {
            return "(missione non installata)".into();
        }
        if s.snapshot {
            return s.title.clone();
        }
        self.scenario(&s.selection).map_or_else(
            || match s.selection {
                Selection::Random { side, .. } => format!("{side} × {side}"),
                _ => String::new(),
            },
            |m| m.title.clone(),
        )
    }
    /// Positions in `saves` of the games the page lists: on the load page
    /// only the chosen type (missions and random maps, or campaigns; the
    /// original's recorded demos are not played back).
    pub fn listed_saves(&self) -> Vec<usize> {
        (0..self.saves.len())
            .filter(|&i| {
                self.screen != Screen::Load
                    || match self.saves[i].selection {
                        Selection::Scenario {
                            campaign: Some(_), ..
                        } => self.load_type == 1,
                        _ => self.load_type == 0,
                    }
            })
            .collect()
    }
    /// The name proposed for a new save: the title of the open scenario.
    /// Empty for a random map, whose save then takes the game's title.
    fn default_save_name(&self) -> String {
        self.selection
            .as_ref()
            .and_then(|s| self.scenario(s))
            .map_or_else(String::new, |m| m.title.clone())
    }
    pub fn refresh_saves(&mut self) {
        let chosen = self
            .save
            .and_then(|i| self.saves.get(i))
            .map(|s| s.file.clone());
        self.saves = list_saves(&self.root, &self.catalog);
        self.save = chosen.and_then(|f| self.saves.iter().position(|s| s.file == f));
        self.dirty = true;
    }
    fn restore(&mut self, saved: SavedSession) -> Result<Effect> {
        if saved.missing
            || (!saved.snapshot
                && matches!(&saved.selection, Selection::Scenario { .. })
                && self.scenario(&saved.selection).is_none())
        {
            return Err("La missione del salvataggio non è presente".into());
        }
        self.selection = Some(saved.selection.clone());
        self.setup = saved.setup.clone();
        Ok(Effect::Restore(saved))
    }
    /// Characters typed into the name field of the save page.
    pub fn type_save_name(&mut self, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()) {
            if self.save_name.chars().count() < SAVE_NAME_LENGTH {
                self.save_name.push(c);
            }
        }
        self.dirty = true;
    }
    pub fn erase_save_name(&mut self) {
        self.save_name.pop();
        self.dirty = true;
    }

    /// The kingdoms of the selected custom mission, as its setup screen
    /// lists them: the active kingdom records, in order.
    fn setup_rows(&self) -> Result<Vec<SetupRow>> {
        let m = self
            .catalog
            .missions
            .get(self.custom)
            .ok_or("Nessuna missione selezionata")?;
        let c = m.container()?;
        let world = MissionWorld::read(&c, &MapGrid::read(&c, &m.header)?)?;
        let player = world.player();
        Ok(world
            .active_kingdoms()
            .map(|(owner, k)| SetupRow {
                record: owner as usize - 1,
                name: [k.leader.trim(), k.name.trim()]
                    .into_iter()
                    .find(|n| !n.is_empty())
                    .map_or_else(|| format!("Tzar {owner}"), str::to_owned),
                color: world.color_slot(owner).map_or(owner, |s| s as u8 + 1),
                // 0x4c4282: every row starts without a team; the record's
                // team byte (equal to the colour in the original scenarios)
                // is not shown.
                team: 0,
                race: world.civilization(owner) + 1,
                human: owner == player,
                ai: String::new(),
            })
            .collect())
    }

    /// Interactive controls of the current screen, in focus order.
    pub fn widgets(&self, locale: &Ini) -> Vec<Widget> {
        match &self.choose_ai {
            Some(d) if self.screen == Screen::Random => self.choose_ai_widgets(d, locale),
            _ => self.screen_widgets(self.screen, locale),
        }
    }
    /// «COMPORTAMENTO IA» (0x48a060), centred on the screen and the only
    /// window taking input while open: the list of behaviours, «Accetta»
    /// and «Annulla».
    fn choose_ai_widgets(&self, d: &ChooseAi, locale: &Ini) -> Vec<Widget> {
        let mut c = Controls::default();
        let (x, y) = CHOOSE_AI_AT;
        let first =
            (self.scroll.max(0) as usize).min(self.ai_scripts.len().saturating_sub(AI_ROWS));
        for (k, (_, name)) in self.ai_scripts.iter().enumerate().skip(first).take(AI_ROWS) {
            c.add(
                r(x + 46, y + 88 + (k - first) as i32 * ROW, 377, ROW),
                if name.is_empty() {
                    locale.label("chooseai", "default")
                } else {
                    name.clone()
                },
                Action::AiScript(k),
                Style::Item,
            )
            .checked = Some(d.selected == k);
        }
        for (dx, key, action) in [
            (47, "ok", Action::AiAccept),
            (261, "cancel", Action::AiCancel),
        ] {
            c.add(
                r(x + dx, y + 254, 189, 27),
                locale.label("chooseai", key),
                action,
                Style::Button("D_CHOOSEAI/BTN"),
            );
        }
        c.0
    }
    /// Controls of `screen` with the current state; also used to draw the
    /// screen beneath a dialog.
    fn screen_widgets(&self, screen: Screen, locale: &Ini) -> Vec<Widget> {
        let mut c = Controls::default();
        match screen {
            Screen::Main => {
                for (i, (key, to)) in [
                    ("single", Screen::Single),
                    ("multi", Screen::Multiplayer),
                    ("pref", Screen::Options),
                    ("credits", Screen::Credits),
                    ("quit", Screen::Quit),
                ]
                .into_iter()
                .enumerate()
                {
                    let w = c.add(
                        r(270, 277 + 60 * i as i32, 235, 25),
                        locale.label("mainscr", key),
                        Action::Open(to),
                        Style::Button("S_MAIN/BTN"),
                    );
                    // No network game in this version.
                    w.enabled = to != Screen::Multiplayer;
                }
            }
            Screen::Single => {
                for (key, y, to) in [
                    ("campaign", 141, Screen::Campaign),
                    ("custom", 233, Screen::Missions),
                    ("random", 329, Screen::Random),
                    ("load", 425, Screen::Load),
                    ("back", 521, Screen::Main),
                ] {
                    c.add(
                        r(299, y, 235, 25),
                        locale.label("singlescr", key),
                        Action::Open(to),
                        Style::Button("S_SINGLEPLAYER/BTN"),
                    );
                }
            }
            Screen::Campaign => {
                for (i, campaign) in self.catalog.campaigns.iter().enumerate().take(15) {
                    c.add(
                        r(80, 43 + i as i32 * ROW, 251, ROW),
                        campaign.title.clone(),
                        Action::Campaign(i),
                        Style::Item,
                    )
                    .checked = Some(i == self.campaign);
                }
                if let Some(campaign) = self.catalog.campaigns.get(self.campaign) {
                    let first = self.scroll.max(0) as usize;
                    let listed = self.campaign_missions();
                    for (row, &i) in listed.iter().skip(first).take(9).enumerate() {
                        c.add(
                            r(80, 352 + row as i32 * ROW, 252, ROW),
                            campaign.missions[i].title.clone(),
                            Action::Mission(i),
                            Style::Item,
                        )
                        .checked = Some(i == self.mission);
                    }
                }
                spin(
                    &mut c,
                    r(332, 542, 180, 27),
                    locale.label("mp_setupmap", &format!("difflvl_{}", self.difficulty)),
                    [Action::Difficulty(-1), Action::Difficulty(1)],
                    true,
                );
                c.add(
                    r(78, 545, 235, 25),
                    locale.label("campaign", "play"),
                    Action::Play,
                    Style::Button("S_CAMPAIGN/BTN"),
                )
                .enabled = !self.catalog.campaigns.is_empty();
                c.add(
                    r(535, 544, 235, 25),
                    locale.label("campaign", "back"),
                    Action::Back,
                    Style::Button("S_CAMPAIGN/BTN"),
                );
            }
            Screen::Missions => {
                let first = self.scroll.max(0) as usize;
                for (i, m) in self
                    .catalog
                    .missions
                    .iter()
                    .enumerate()
                    .skip(first)
                    .take(15)
                {
                    c.add(
                        r(79, 41 + (i - first) as i32 * ROW, 251, ROW),
                        format!("({} - {})  {}", m.players[0], m.players[1], m.title),
                        Action::Custom(i),
                        Style::Map(m.map_type),
                    )
                    .checked = Some(i == self.custom);
                }
                // As in the original, PLAY opens the setup of the mission.
                c.add(
                    r(77, 544, 235, 25),
                    locale.label("sp_choosemap", "play"),
                    Action::Setup,
                    Style::Button("S_SINGLECHOOSEMAP/BTN"),
                )
                .enabled = !self.catalog.missions.is_empty();
                c.add(
                    r(534, 543, 235, 25),
                    locale.label("sp_choosemap", "back"),
                    Action::Back,
                    Style::Button("S_SINGLECHOOSEMAP/BTN"),
                );
            }
            Screen::Random => {
                let s = "sp_setuprnd";
                let size = RANDOM_SIDES
                    .iter()
                    .position(|&v| v == self.random_side)
                    .unwrap_or(1);
                spin(
                    &mut c,
                    r(565, 56, 180, 27),
                    locale.label(s, &format!("m_size_{size}")),
                    [Action::RandomSize(-1), Action::RandomSize(1)],
                    true,
                );
                for (i, row) in self
                    .random_rows
                    .iter()
                    .enumerate()
                    .take(self.random_players.min(8))
                {
                    // Controls of the row (0x4c7d42): colour, name, computer
                    // icon, civilization, team.
                    let y = 35 * i as i32;
                    let w = c.add(
                        r(73, 31 + y, 13, 13),
                        row.name.clone(),
                        Action::SetupColor(i),
                        Style::Swatch(row.color - 1),
                    );
                    w.enabled = i < self.random_players;
                    w.hint = Some("RM_PLAYER_COLOR");
                    // The computer icon covers the right end of the name
                    // field (273–292), so the name stops before it.
                    let name = if row.human {
                        let w = c.add(
                            r(110, 30 + y, 163, 14),
                            self.draft.player_name.clone(),
                            Action::EditName,
                            Style::Name,
                        );
                        w.checked = Some(self.editing_name);
                        w
                    } else {
                        let w = c.add(
                            r(110, 30 + y, 163, 14),
                            row.name.clone(),
                            Action::SetupPlayer(i),
                            Style::Name,
                        );
                        w.enabled = i < self.random_players;
                        w
                    };
                    name.hint = Some("RM_PLAYER_NAME");
                    let w = c.add(
                        r(273, 26 + y, 20, 20),
                        locale.label("chooseai", "title"),
                        Action::ChooseAi(i),
                        Style::AiIcon,
                    );
                    w.checked = Some(!row.ai.is_empty());
                    w.enabled = i < self.random_players;
                    spin(
                        &mut c,
                        r(311, 23 + y, 180, 27),
                        locale.label(s, &format!("race_{}", row.race)),
                        [Action::SetupRace(i, -1), Action::SetupRace(i, 1)],
                        i < self.random_players,
                    );
                    for w in c.0.iter_mut().rev().take(3) {
                        w.hint = Some("RM_PLAYER_CIVILIVIZATION");
                    }
                    let w = c.add(
                        r(511, 31 + y, 13, 13),
                        format!("{} {}", row.name, row.team),
                        Action::SetupTeam(i),
                        Style::Team(row.team_frame()),
                    );
                    w.enabled = i < self.random_players;
                    w.hint = Some("RM_PLAYER_TEAM");
                }
                use SetupOption as K;
                let o = &self.random_rules;
                for (x, y, value, hint, key) in [
                    (
                        66,
                        369,
                        self.random_players.to_string(),
                        "RM_NUMBER_OF_PLAYERS",
                        K::Players,
                    ),
                    (
                        66,
                        427,
                        locale.label(s, &format!("g_type_{}", o.game_type)),
                        "RM_GAME_TYPE",
                        K::GameType,
                    ),
                    (
                        66,
                        487,
                        locale.label(s, &format!("vcond_{}", o.victory)),
                        "RM_VICTORY_CONDITION",
                        K::Victory,
                    ),
                    (
                        270,
                        369,
                        o.population.to_string(),
                        "RM_POP_LIMIT",
                        K::Population,
                    ),
                    (
                        270,
                        427,
                        locale.label(s, &format!("difflvl_{}", o.difficulty)),
                        "RM_DIFFICULTY_LEVEL",
                        K::Difficulty,
                    ),
                    (
                        270,
                        487,
                        locale.label(s, &format!("resurcs_{}", o.resource_level.min(2))),
                        "RM_STARTING_RESOURCES",
                        K::Resources,
                    ),
                ] {
                    spin(
                        &mut c,
                        r(x, y, 180, 27),
                        value,
                        [Action::SetupOption(key, -1), Action::SetupOption(key, 1)],
                        true,
                    );
                    for w in c.0.iter_mut().rev().take(3) {
                        w.hint = Some(hint);
                    }
                }
                if o.victory >= 2 {
                    spin(
                        &mut c,
                        VALUE_SELECTOR,
                        o.victory_limit.to_string(),
                        [
                            Action::SetupOption(K::VictoryLimit, -1),
                            Action::SetupOption(K::VictoryLimit, 1),
                        ],
                        true,
                    );
                    for w in c.0.iter_mut().rev().take(3) {
                        w.hint = Some("RM_VALUE");
                    }
                }
                for (y, key, value, hint) in [
                    (
                        111,
                        RandomOption::Kind,
                        locale.label(s, &format!("m_type_{}", self.random_options.kind)),
                        "RM_MAP_TYPE",
                    ),
                    (
                        228,
                        RandomOption::Resources,
                        locale.label(s, &format!("m_res_{}", self.random_options.map_resources)),
                        "RM_MAP_RESOURCES",
                    ),
                ] {
                    spin(
                        &mut c,
                        r(565, y, 180, 27),
                        value,
                        [Action::RandomOption(key, -1), Action::RandomOption(key, 1)],
                        true,
                    );
                    for w in c.0.iter_mut().rev().take(3) {
                        w.hint = Some(hint);
                    }
                }
                spin(
                    &mut c,
                    r(565, 169, 180, 27),
                    self.random_terrain
                        .checked_sub(1)
                        .and_then(|i| self.random_terrains.get(i))
                        .map_or_else(|| locale.label(s, "m_type_0"), |t| t.1.clone()),
                    [Action::RandomTerrain(-1), Action::RandomTerrain(1)],
                    !self.random_terrains.is_empty(),
                );
                let o = &self.random_options;
                for (y, key, on, hint, option) in [
                    (
                        267,
                        "t_gslocations",
                        o.guarded,
                        "RM_GUARDED_TOWN",
                        RandomOption::Guarded,
                    ),
                    (
                        303,
                        "t_animals",
                        o.animals,
                        "RM_AMNIMALS",
                        RandomOption::Animals,
                    ),
                    (
                        339,
                        "t_mitems",
                        o.items,
                        "RM_MAGIC_ITEMS",
                        RandomOption::Items,
                    ),
                    (376, "t_cliffs", o.cliffs, "RM_CLIFFS", RandomOption::Cliffs),
                    (
                        411,
                        "t_gateways",
                        o.gateways,
                        "RM_GATEWAYS",
                        RandomOption::Gateways,
                    ),
                ] {
                    let w = c.add(
                        r(564, y, 30, 30),
                        locale.label(s, key),
                        Action::RandomOption(option, 1),
                        Style::Check,
                    );
                    w.checked = Some(on);
                    w.hint = Some(hint);
                }
                // 0x4c92b0: «Resa dell'IA», without a hint of its own in the
                // original; this version gives it one.
                if self.ai_resign_offered {
                    let w = c.add(
                        r(471, 427, 32, 32),
                        "Resa dell'IA".into(),
                        Action::AiResign,
                        Style::AiResign,
                    );
                    w.checked = Some(self.random_ai_resign);
                    w.hint = Some("RZ_AI_RESIGN");
                }
                for (y, key, explore, on) in [
                    (539, "t_fog", false, self.random_rules.no_fog),
                    (569, "t_expolre", true, self.random_rules.explored),
                ] {
                    c.add(
                        r(268, y, 30, 30),
                        locale.label(s, key),
                        Action::RandomVisibility(explore),
                        Style::Check,
                    )
                    .checked = Some(on);
                }
                c.add(
                    r(556, 499, 210, 27),
                    locale.label(s, "play"),
                    Action::Generate,
                    Style::Button("S_SINGLERND/BTN"),
                );
                c.add(
                    r(556, 554, 210, 27),
                    locale.label(s, "back"),
                    Action::Back,
                    Style::Button("S_SINGLERND/BTN"),
                );
            }
            Screen::Setup => {
                // Positions of the original constructor (0x4c4100): one row
                // every 35 pixels for each kingdom, the game options below.
                let s = "sp_setupmap";
                for (i, row) in self.setup_rows.iter().enumerate().take(8) {
                    let y = 35 * i as i32;
                    c.add(
                        r(73, 31 + y, 13, 13),
                        row.name.clone(),
                        Action::SetupColor(i),
                        Style::Swatch(row.color - 1),
                    )
                    .enabled = !self.setup_map_defaults && i < self.setup_players;
                    let name = if row.human {
                        let w = c.add(
                            r(110, 30 + y, 182, 14),
                            self.draft.player_name.clone(),
                            Action::EditName,
                            Style::Name,
                        );
                        w.checked = Some(self.editing_name);
                        w
                    } else {
                        let w = c.add(
                            r(110, 30 + y, 182, 14),
                            row.name.clone(),
                            Action::SetupPlayer(i),
                            Style::Name,
                        );
                        w.enabled = i < self.setup_players;
                        w
                    };
                    name.hint = Some("CM1_PLAYER_NAME");
                    spin(
                        &mut c,
                        r(311, 23 + y, 180, 27),
                        locale.label(s, &format!("race_{}", row.race)),
                        [Action::SetupRace(i, -1), Action::SetupRace(i, 1)],
                        !self.setup_map_defaults && i < self.setup_players,
                    );
                    for w in c.0.iter_mut().rev().take(3) {
                        w.hint = Some("CM1_PLAYER_CIVILIVIZATION");
                    }
                    c.add(
                        r(511, 31 + y, 13, 13),
                        format!("{} {}", row.name, row.team),
                        Action::SetupTeam(i),
                        Style::Team(row.team_frame()),
                    )
                    .enabled = !self.setup_map_defaults && i < self.setup_players;
                }
                use SetupOption as K;
                let o = &self.setup_options;
                for (x, y, value, hint, key) in [
                    (
                        66,
                        369,
                        self.setup_players.to_string(),
                        "CM1_NUMBER_OF_PLAYERS",
                        K::Players,
                    ),
                    (
                        66,
                        427,
                        locale.label(s, &format!("g_type_{}", o.game_type)),
                        "CM1_GAME_TYPE",
                        K::GameType,
                    ),
                    (
                        66,
                        487,
                        locale.label(s, &format!("vcond_{}", o.victory)),
                        "CM1_VICTORY_CONDITION",
                        K::Victory,
                    ),
                    (
                        270,
                        369,
                        o.population.to_string(),
                        "CM1_POP_LIMIT",
                        K::Population,
                    ),
                    (
                        270,
                        427,
                        locale.label(s, &format!("difflvl_{}", o.difficulty)),
                        "CM1_DIFFICULTY_LEVEL",
                        K::Difficulty,
                    ),
                    (
                        270,
                        487,
                        locale.label(s, &format!("resurcs_{}", o.resource_level)),
                        "CM1_STARTING_RESOURCES",
                        K::Resources,
                    ),
                ] {
                    spin(
                        &mut c,
                        r(x, y, 180, 27),
                        value,
                        [Action::SetupOption(key, -1), Action::SetupOption(key, 1)],
                        self.setup_option_enabled(key),
                    );
                    for w in c.0.iter_mut().rev().take(3) {
                        w.hint = Some(hint);
                    }
                }
                if o.victory >= 2 {
                    spin(
                        &mut c,
                        VALUE_SELECTOR,
                        o.victory_limit.to_string(),
                        [
                            Action::SetupOption(K::VictoryLimit, -1),
                            Action::SetupOption(K::VictoryLimit, 1),
                        ],
                        self.setup_option_enabled(K::VictoryLimit),
                    );
                    for w in c.0.iter_mut().rev().take(3) {
                        w.hint = Some("CM1_VALUE");
                    }
                }
                for (x, y, key, on, hint, option) in [
                    (268, 539, "t_fog", o.no_fog, "CM1_NO_FOG", K::NoFog),
                    (
                        268,
                        569,
                        "t_expolre",
                        o.explored,
                        "CM1_EXPLORE",
                        K::Explored,
                    ),
                    (
                        548,
                        309,
                        "t_mapdef",
                        self.setup_map_defaults,
                        "CM1_USE_MAP_SETTINGS",
                        K::MapDefaults,
                    ),
                ] {
                    let w = c.add(
                        r(x, y, 30, 30),
                        locale.label(s, key),
                        Action::SetupOption(option, 1),
                        Style::Check,
                    );
                    w.checked = Some(on);
                    w.hint = Some(hint);
                }
                c.add(
                    r(556, 499, 210, 27),
                    locale.label(s, "play"),
                    Action::Play,
                    Style::Button("S_SINGLESETUP/BTN"),
                )
                .enabled = !self.setup_rows.is_empty();
                c.add(
                    r(556, 554, 210, 27),
                    locale.label(s, "back"),
                    Action::Back,
                    Style::Button("S_SINGLESETUP/BTN"),
                );
            }
            Screen::Load | Screen::Save => {
                let (x, y) = FILE_AT;
                let load = screen == Screen::Load;
                let (section, art) = if load {
                    ("load", "D_LOAD/BTN")
                } else {
                    ("save", "D_SAVE/BTN")
                };
                if load {
                    spin(
                        &mut c,
                        r(x + 330, y + 32, 180, 27),
                        locale.label("load", &format!("type_{}", self.load_type)),
                        [Action::LoadType(-1), Action::LoadType(1)],
                        true,
                    );
                    for w in c.0.iter_mut().rev().take(3) {
                        w.hint = Some("LG_CHOOSE");
                    }
                }
                if !load {
                    // The name of the save, in the field of the original art.
                    let w = c.add(
                        r(x + 121, y + 39, 276, 16),
                        self.save_name.clone(),
                        Action::Inert,
                        Style::Name,
                    );
                    w.checked = Some(true);
                    w.hint = Some("SG_SAVE_GAME");
                }
                // Nine rows; on the save page the first is a new save.
                let first = usize::from(!load);
                let listed = self.listed_saves();
                let total = listed.len() + first;
                for row in 0..SAVE_ROWS {
                    let k = row + self.scroll.max(0) as usize;
                    if k >= total {
                        break;
                    }
                    let rect = r(x + 44, y + 77 + row as i32 * ROW, 431, ROW);
                    if k < first {
                        c.add(
                            rect,
                            locale.label(section, "new"),
                            Action::NewSave,
                            Style::Item,
                        )
                        .checked = Some(self.save.is_none());
                    } else {
                        let i = listed[k - first];
                        c.add(
                            rect,
                            self.save_label(&self.saves[i]),
                            Action::Slot(i),
                            Style::Item,
                        )
                        .checked = Some(self.save == Some(i));
                    }
                }
                let used = self.save.is_some_and(|i| i < self.saves.len());
                c.add(
                    r(x + 42, y + 253, 137, 27),
                    locale.label(section, section),
                    if load {
                        Action::LoadSlot
                    } else {
                        Action::SaveSlot
                    },
                    Style::Button(art),
                )
                .enabled = !load || used;
                c.add(
                    r(x + 206, y + 253, 137, 27),
                    locale.label(section, "del"),
                    Action::DeleteSlot,
                    Style::Button(art),
                )
                .enabled = used;
                c.add(
                    r(x + 370, y + 253, 137, 27),
                    locale.label(section, "cancel"),
                    Action::Back,
                    Style::Button(art),
                );
            }
            Screen::Pause => {
                let (x, y) = self.dialog_at();
                for (key, dy, action) in [
                    ("inst", 46, Action::Open(Screen::Help)),
                    ("load", 86, Action::Open(Screen::Load)),
                    ("save", 126, Action::Open(Screen::Save)),
                    ("opti", 166, Action::Open(Screen::Options)),
                    ("rest", 206, Action::Restart),
                    ("quit", 246, Action::Open(Screen::Resign)),
                    ("back", 326, Action::Resume),
                ] {
                    c.add(
                        r(x + 63, y + dy, 150, 25),
                        locale.label("menu", key),
                        action,
                        Style::Button("D_MENU/BTN BACK"),
                    );
                }
            }
            Screen::Quit | Screen::Resign => {
                let (x, y) = self.dialog_at();
                c.add(
                    r(x + 50, y + 258, 189, 27),
                    locale.label("general", "yes"),
                    if screen == Screen::Quit {
                        Action::Exit
                    } else {
                        Action::Open(Screen::Main)
                    },
                    Style::Button("D_MSGWND/BTN"),
                );
                c.add(
                    r(x + 261, y + 258, 189, 27),
                    locale.label("general", "no"),
                    Action::Back,
                    Style::Button("D_MSGWND/BTN"),
                );
            }
            Screen::Help | Screen::Multiplayer => {
                let (x, y) = self.dialog_at();
                c.add(
                    r(x + 155, y + 258, 189, 27),
                    locale.label("general", "close"),
                    Action::Back,
                    Style::Button("D_MSGWND/BTN"),
                );
            }
            Screen::End => {
                // 0x442700: won → restart, next, quit, hide; lost → restart,
                // load, quit, hide. «CONTINUA» needs the map of a script.
                // 0x449d72: the buttons of `d_endgame/btn.bmp` in a column.
                let (x, y) = self.dialog_at();
                let (won, next) = self.ending.unwrap_or((false, false));
                // 0x442861: a next map gives the buttons of a victory even
                // to a defeat; without one «CONTINUA» is disabled (0x4428c8).
                let second = if won || next {
                    ("next", if next { Action::NextMap } else { Action::Inert })
                } else {
                    ("load", Action::Open(Screen::Load))
                };
                for (i, (key, action)) in [
                    ("restart", Action::Restart),
                    second,
                    ("quit", Action::Open(Screen::Main)),
                    ("hide", Action::Resume),
                ]
                .into_iter()
                .enumerate()
                {
                    c.add(
                        r(x + 233, y + 114 + 48 * i as i32, 210, 27),
                        locale.label("endgamedlg", key),
                        action,
                        Style::Button("D_ENDGAME/BTN"),
                    )
                    .enabled = key != "next" || next;
                }
            }
            Screen::Credits => {
                // The original leaves the credits with a click anywhere.
                c.add(
                    r(0, 0, 800, 600),
                    locale.label("campaign", "back"),
                    Action::Back,
                    Style::Hidden,
                );
            }
            Screen::Options => self.settings_controls(&mut c, locale),
        }
        // Original help constructor 0x45c800: a 2×2-state ornament anchored
        // at the parent's top-left corner, 57×74 on the pages, the small
        // 50×43 on the load and save windows (second argument 0, 0x46e865,
        // 0x4be6c7).
        let help_at = match screen {
            Screen::Campaign
            | Screen::Missions
            | Screen::Setup
            | Screen::Random
            | Screen::Options => Some((0, 0, 57, 74, "COMMON/BTN_HELP_B")),
            Screen::Load | Screen::Save => {
                Some((FILE_AT.0, FILE_AT.1, 50, 43, "COMMON/BTN_HELP_S"))
            }
            _ => None,
        };
        if let Some((x, y, w, h, art)) = help_at {
            c.add(
                r(x, y, w, h),
                "Aiuto".into(),
                Action::Help,
                Style::HelpBall(art),
            )
            .checked = Some(self.help);
        }
        c.0
    }

    /// Fixed texts of the current screen.
    pub fn texts(&self, locale: &Ini) -> Vec<Text> {
        let mut out = Vec::new();
        let mut label = |rect: Rect, text: String| {
            out.push(Text {
                rect,
                text,
                face: Face::Small,
                color: color::GOLD,
                centered: false,
            });
        };
        match self.screen {
            Screen::Campaign => {
                for (key, x, y) in [
                    ("t_cam", 78, 17),
                    ("t_descr", 401, 17),
                    ("t_emiss", 78, 326),
                    ("t_emisdes", 401, 326),
                ] {
                    label(r(x, y, 199, 14), locale.label("campaign", key));
                }
                label(
                    r(338, 526, 149, 13),
                    locale.label("mp_setupmap", "t_diff_lvl"),
                );
                if let Some(t) = out.last_mut() {
                    t.centered = true;
                }
            }
            Screen::Missions => {
                for (key, x, y) in [
                    ("t_custommap", 72, 17),
                    ("t_map", 402, 17),
                    ("t_description", 72, 326),
                ] {
                    label(r(x, y, 199, 14), locale.label("sp_choosemap", key));
                }
                // Without a title picture the original writes the map name.
                let name = self
                    .catalog
                    .missions
                    .get(self.custom)
                    .map(|m| m.title.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| locale.label("defmsg", "nomaptitle"));
                out.push(Text {
                    rect: r(406, 41, 340, 18),
                    text: name,
                    face: Face::Big,
                    color: color::DARK_GOLD,
                    centered: false,
                });
            }
            Screen::Random => {
                let s = "sp_setuprnd";
                for (x, y, key) in [
                    (68, 353, "t_num_players"),
                    (68, 410, "t_game_type"),
                    (68, 470, "t_vict_cond"),
                    (273, 353, "t_poplimit"),
                    (273, 412, "t_diff_lvl"),
                    (273, 470, "t_st_res"),
                    (302, 549, "t_fog"),
                    (302, 579, "t_expolre"),
                    (568, 43, "t_map_size"),
                    (568, 98, "t_map_type"),
                    (568, 155, "t_terrain"),
                    (568, 214, "t_map_res"),
                    (610, 278, "t_gslocations"),
                    (610, 313, "t_animals"),
                    (610, 350, "t_mitems"),
                    (610, 386, "t_cliffs"),
                    (610, 423, "t_gateways"),
                ] {
                    let mut text = locale.label(s, key);
                    if key == "t_num_players" {
                        text.push_str(&format!(" (2 - {})", random_player_limit(self.random_side)));
                    }
                    label(r(x, y, 190, 13), text);
                }
                if self.random_rules.victory >= 2 {
                    label(
                        r(68, 527, 149, 13),
                        locale.label(
                            s,
                            if self.random_rules.victory == 2 {
                                "t_time_lim"
                            } else {
                                "t_score_lim"
                            },
                        ),
                    );
                }
            }
            Screen::Setup => {
                let s = "sp_setupmap";
                let players = self
                    .catalog
                    .missions
                    .get(self.custom)
                    .map_or([1, 8], |m| m.players);
                for (x, y, key) in [
                    (68, 353, "t_num_players"),
                    (68, 410, "t_game_type"),
                    (68, 470, "t_vict_cond"),
                    (273, 353, "t_poplimit"),
                    (273, 412, "t_diff_lvl"),
                    (273, 470, "t_st_res"),
                    (302, 549, "t_fog"),
                    (302, 579, "t_expolre"),
                    (584, 320, "t_mapdef"),
                    (549, 35, "t_map_decr"),
                ] {
                    let mut text = locale.label(s, key);
                    if key == "t_num_players" {
                        // " (%d - %d)": the players the map allows.
                        text.push_str(&format!(" ({} - {})", players[0], players[1]));
                    }
                    label(r(x, y, 199, 13), text);
                }
                if self.setup_options.victory >= 2 {
                    label(
                        r(68, 527, 149, 13),
                        locale.label(
                            s,
                            if self.setup_options.victory == 2 {
                                "t_time_lim"
                            } else {
                                "t_score_lim"
                            },
                        ),
                    );
                }
            }
            Screen::Load | Screen::Save => {
                let (x, y) = FILE_AT;
                let (section, key) = if self.screen == Screen::Load {
                    ("load", "t_load")
                } else {
                    ("save", "t_save")
                };
                out.push(Text {
                    rect: r(x + 38, y + 38, 280, 20),
                    text: locale.label(section, key),
                    face: Face::Big,
                    color: color::GOLD,
                    centered: false,
                });
            }
            Screen::Quit | Screen::Resign | Screen::Help | Screen::Multiplayer => {
                let (x, y) = self.dialog_at();
                let (title, body) = match self.screen {
                    Screen::Quit => (locale.label("q", "quit_title"), locale.label("q", "quit")),
                    Screen::Resign => (locale.label("menu", "quit"), locale.label("q", "resign")),
                    Screen::Help => (locale.label("menu", "inst"), String::new()),
                    _ => (
                        locale.label("mainscr", "multi"),
                        "La modalità multigiocatore non è disponibile in questa versione.".into(),
                    ),
                };
                out.push(Text {
                    rect: r(x + 44, y + 46, 406, 20),
                    text: title,
                    face: Face::Big,
                    color: color::GOLD,
                    centered: true,
                });
                if !body.is_empty() {
                    out.push(Text {
                        rect: r(x + 50, y + 92, 400, 140),
                        text: body,
                        face: Face::Small,
                        color: color::GOLD,
                        centered: true,
                    });
                }
            }
            Screen::End => {
                // 0x449e49: two lines in cgb_22b and white (colour 11) right
                // of the picture; 0x449efa: the script's text in Arial12B and
                // dark gold (32) under the buttons.
                let (x, y) = self.dialog_at();
                let won = self.ending.is_some_and(|e| e.0);
                let lines = if won {
                    ["win1", "win2"]
                } else {
                    ["lose1", "lose2"]
                };
                for (i, key) in lines.into_iter().enumerate() {
                    out.push(Text {
                        rect: r(x + 224, y + 38 + 30 * i as i32, 228, 29),
                        text: locale.label("endgamedlg", key),
                        face: Face::Title,
                        color: color::WHITE,
                        centered: true,
                    });
                }
                if !self.ending_text.is_empty() {
                    out.push(Text {
                        rect: r(x + 44, y + 309, 397, 62),
                        text: self.ending_text.clone(),
                        face: Face::Small,
                        color: color::DARK_GOLD,
                        centered: false,
                    });
                }
            }
            Screen::Main => {
                out.push(Text {
                    rect: r(500, 580, 272, 16),
                    text: format!(
                        "{} {}",
                        locale.label("general", "version"),
                        env!("CARGO_PKG_VERSION")
                    ),
                    face: Face::Big,
                    color: color::GREY,
                    centered: false,
                });
            }
            _ => {}
        }
        out
    }
}

/// Collects the controls of a screen.
#[derive(Default)]
struct Controls(Vec<Widget>);
impl Controls {
    fn add(&mut self, rect: Rect, label: String, action: Action, style: Style) -> &mut Widget {
        self.0.push(Widget {
            rect,
            label,
            action,
            enabled: true,
            checked: None,
            value: None,
            hint: None,
            style,
        });
        self.0.last_mut().unwrap()
    }
}

/// Hints of the controls this version adds outside the settings, whose
/// own are in `settings.rs`. `LOCAL/HELP.INI` has none for them; they
/// follow its style.
const EXTRA_HINTS: [(&str, &str); 1] = [(
    "RZ_AI_RESIGN",
    "RESA DELL'IA: con la BANDIERA bianca i regni del computer si arrendono quando perdono, con le SPADE combattono fino alla fine",
)];
/// Depth of the bevel of the options' frames.
const BEVEL: i32 = 8;

/// A selector: left arrow, value, right arrow.
fn spin(c: &mut Controls, rect: Rect, value: String, [left, right]: [Action; 2], enabled: bool) {
    let label = value.clone();
    c.add(
        Rect { w: ARROW, ..rect },
        format!("‹ {label}"),
        left,
        Style::Arrow { right: false },
    )
    .enabled = enabled;
    c.add(
        Rect {
            x: rect.x + ARROW,
            w: rect.w - 2 * ARROW,
            ..rect
        },
        value,
        right.clone(),
        Style::Value,
    )
    .enabled = enabled;
    c.add(
        Rect {
            x: rect.x + rect.w - ARROW,
            w: ARROW,
            ..rect
        },
        format!("{label} ›"),
        right,
        Style::Arrow { right: true },
    )
    .enabled = enabled;
}

pub struct MenuSkin {
    pub locale: Ini,
    pub title_font: BitmapFont,
    pub font: BitmapFont,
    pub small: BitmapFont,
    art: BTreeMap<String, Bitmap>,
    /// Credits: style (`@0` heading, `@1` names, `@2` title, `@3` notes) and text, wrapped.
    pub credit_lines: Vec<(u8, String)>,
    /// Font of each credits style, as listed by `DATA\CREDITSFNT.INI`.
    credit_fonts: Vec<BitmapFont>,
    /// The original hotkey help, wrapped with its highlight switches.
    pub help_lines: Vec<Vec<(u8, String)>>,
    /// `LOCAL/HELP.INI`: the hints shown under the pointer.
    pub hints: Ini,
}
/// Text column of the credits book's right page.
const CREDITS_PAGE: Rect = r(492, 160, 250, 395);
const HELP_TEXT: Rect = r(MESSAGE_AT.0 + 48, MESSAGE_AT.1 + 88, 372, 144);
const BACKGROUNDS: [&str; 9] = [
    "S_MAIN",
    "S_SINGLEPLAYER",
    "S_CAMPAIGN",
    "S_SINGLECHOOSEMAP",
    "S_SINGLESETUP",
    "S_SINGLERND",
    "S_CREDITS",
    "D_OPTIONS",
    "D_MENU",
];
const PARTS: [&str; 41] = [
    "D_ENDGAME/BACK",
    "D_ENDGAME/BACK2",
    "D_ENDGAME/BTN",
    "WIN",
    "LOSE",
    "COMMON/SCROLL CTRL",
    "COMMON/AI",
    "D_CHOOSEAI/BACK",
    "D_CHOOSEAI/BTN",
    "S_SINGLESETUP/ROW BACK",
    "S_SINGLERND/ROW BACK",
    "S_SINGLESETUP/BTN",
    "COMMON/COLOR BARS",
    "COMMON/TEAMS",
    "D_LOAD/BACK",
    "D_SAVE/BACK",
    "D_MSGWND/BACK",
    "S_MAIN/BTN",
    "S_SINGLEPLAYER/BTN",
    "S_CAMPAIGN/BTN",
    "S_SINGLECHOOSEMAP/BTN",
    "S_SINGLERND/BTN",
    "S_SINGLERND/FRAME",
    "D_LOAD/BTN",
    "D_SAVE/BTN",
    "D_OPTIONS/BTN",
    "D_MENU/BTN BACK",
    "D_MSGWND/BTN",
    "COMMON/BTN UP",
    "COMMON/BTN DOWN",
    "COMMON/BTN LEFT",
    "COMMON/BTN RIGHT",
    "COMMON/THUMB VERT",
    "COMMON/THUMB HORZ",
    "COMMON/CHK",
    "COMMON/MAP TYPES",
    "COMMON/BTN_HELP_B",
    "COMMON/BTN_HELP_S",
    "D_MENU/BACK",
    "S_SINGLEPLAYER/BACK",
    "D_OPTIONS/BACK",
];
impl MenuSkin {
    pub fn load(assets: &GameAssets) -> Result<Self> {
        let locale = Ini::locale(assets)?;
        let mut art = BTreeMap::new();
        for folder in BACKGROUNDS {
            // D_MENU is a window with a green (transparent) surround.
            let b = Bitmap::decode(
                &assets.get(&format!("IMAGES/SCREENS/{folder}/BACK.BMP"))?,
                folder == "D_MENU",
            )?;
            if folder != "D_MENU" && (b.width, b.height) != (800, 600) {
                return Err(invalid(format!("{folder}/BACK.BMP deve essere 800×600")));
            }
            art.insert(folder.to_owned(), b);
        }
        for part in PARTS {
            if art.contains_key(part) {
                continue;
            }
            let b = Bitmap::decode(&assets.get(&format!("IMAGES/SCREENS/{part}.BMP"))?, true)?;
            art.insert(part.to_owned(), b);
        }
        // «Resa dell'IA» of the random map screen, outside SCREENS; only
        // the Italian 1.11 has it.
        if let Ok(data) = assets.get("IMAGES/AIRESIGN.BMP") {
            art.insert("AIRESIGN".into(), Bitmap::decode(&data, true)?);
        }
        // Marble without painted controls, for the settings.
        let single = &art["S_SINGLEPLAYER"];
        let mut clean = single.clone();
        for y in 120..600usize {
            clean.rgba[y * 800 * 4..(y + 1) * 800 * 4]
                .copy_from_slice(&single.rgba[(y % 120) * 800 * 4..(y % 120 + 1) * 800 * 4]);
        }
        let page = settings::settings_page(&art["D_OPTIONS"], &clean)?;
        art.insert("PAGE_SETTINGS".into(), page);
        let groove = settings::groove_art(&art["D_OPTIONS"])?;
        art.insert("SETTINGS/GROOVE".into(), groove);
        art.insert("CLEAN".into(), clean);
        let small = BitmapFont::decode(&assets.get("LOCAL/FONTS/ARIAL12B_AA.FNT")?)?;
        let font = BitmapFont::decode(&assets.get("LOCAL/FONTS/ARIAL14B_AA.FNT")?)?;
        let title_font = BitmapFont::decode(&assets.get("LOCAL/FONTS/CGB_22B.FNT")?)?;
        let credits_ini = Ini::parse(&assets.get("DATA/CREDITSFNT.INI")?);
        let credit_fonts = (0..)
            .map_while(|i: u8| {
                credits_ini
                    .get("credits", &i.to_string())
                    .map(str::to_owned)
            })
            .map(|path| BitmapFont::decode(&assets.get(&path.replace('\\', "/"))?))
            .collect::<Result<Vec<_>>>()?;
        if credit_fonts.len() < 2 {
            return Err(crate::invalid("CREDITSFNT.INI lists no fonts"));
        }
        let mut credit_lines = Vec::new();
        let mut style = 1u8;
        for line in legacy_text(&assets.get("LOCAL/CREDITS.TXT")?).lines() {
            let mut text = line;
            if let Some(rest) = line.strip_prefix('@')
                && let Some(d) = rest.chars().next().and_then(|c| c.to_digit(10))
            {
                style = d as u8;
                text = &rest[1..];
            }
            let face = &credit_fonts[(style as usize).min(credit_fonts.len() - 1)];
            if text.trim().is_empty() {
                credit_lines.push((style, String::new()));
            }
            for wrapped in face.wrap(text.trim(), CREDITS_PAGE.w as usize) {
                if !wrapped.is_empty() {
                    credit_lines.push((style, wrapped));
                }
            }
        }
        let help_lines = rich_wrap(
            &small,
            &legacy_text(&assets.get("HELP/HK.TXT")?),
            HELP_TEXT.w as usize,
        );
        Ok(Self {
            locale,
            title_font,
            font,
            small,
            art,
            credit_lines,
            credit_fonts,
            help_lines,
            hints: Ini::parse(&assets.get("LOCAL/HELP.INI")?),
        })
    }
    /// Distance between the lines of a text box.
    pub fn line(&self) -> i32 {
        self.small.height as i32 + 1
    }
    fn background(menu: &Menu) -> Option<&'static str> {
        if menu.over_map() && !menu.screen.is_settings() {
            return None;
        }
        Some(match menu.screen {
            Screen::Main | Screen::Quit | Screen::Multiplayer => "S_MAIN",
            Screen::Single | Screen::Load => "S_SINGLEPLAYER",
            Screen::Campaign => "S_CAMPAIGN",
            Screen::Missions => "S_SINGLECHOOSEMAP",
            Screen::Setup => "S_SINGLESETUP",
            Screen::Random => "S_SINGLERND",
            Screen::Credits => "S_CREDITS",
            Screen::Options => "PAGE_SETTINGS",
            _ => "CLEAN",
        })
    }
    /// The open dialog the player may drag, where it lies in the pixels of
    /// the canvas as it opened ([`Menu::dialog_place`]).
    pub fn drag_rect(&self, menu: &Menu) -> Option<Rect> {
        if !Menu::draggable(menu.screen) {
            return None;
        }
        let (key, _) = Self::dialog(menu)?;
        let (x, y) = menu.dialog_place();
        let b = self.art.get(key)?;
        Some(r(x, y, b.width as i32, b.height as i32))
    }
    fn dialog(menu: &Menu) -> Option<(&'static str, (i32, i32))> {
        match menu.screen {
            Screen::Pause => Some(("D_MENU", menu.dialog_at())),
            Screen::Load => Some(("D_LOAD/BACK", FILE_AT)),
            Screen::Save => Some(("D_SAVE/BACK", FILE_AT)),
            Screen::Quit | Screen::Resign | Screen::Help | Screen::Multiplayer => {
                Some(("D_MSGWND/BACK", menu.dialog_at()))
            }
            Screen::End if menu.ending_text.is_empty() => {
                Some(("D_ENDGAME/BACK", menu.dialog_at()))
            }
            Screen::End => Some(("D_ENDGAME/BACK2", menu.dialog_at())),
            _ => None,
        }
    }
    /// Description boxes: text, text area and visible lines.
    pub fn description<'a>(&self, menu: &'a Menu, panel: usize) -> Option<(&'a str, Rect, usize)> {
        let (text, rect) = match (menu.screen, panel) {
            (Screen::Campaign, 0) => (
                menu.catalog
                    .campaigns
                    .get(menu.campaign)?
                    .description
                    .as_str(),
                r(408, 45, 328, 250),
            ),
            (Screen::Campaign, 1) => (
                menu.catalog
                    .campaigns
                    .get(menu.campaign)?
                    .missions
                    .get(menu.mission)?
                    .description
                    .as_str(),
                r(406, 352, 332, 145),
            ),
            (Screen::Missions, 1) => (
                menu.catalog.missions.get(menu.custom)?.description.as_str(),
                r(80, 352, 658, 146),
            ),
            (Screen::Setup, 1) => (
                menu.catalog.missions.get(menu.custom)?.description.as_str(),
                r(555, 64, 179, 223),
            ),
            _ => return None,
        };
        let rows = (rect.h / self.line()).max(1) as usize;
        Some((text, rect, rows))
    }
    fn description_text<'a>(&self, menu: &'a Menu, panel: usize) -> Option<(String, Rect, usize)> {
        let (text, rect, rows) = self.description(menu, panel)?;
        let text = if text.trim().is_empty() {
            self.locale.label("defmsg", "nodecr")
        } else {
            text.to_owned()
        };
        Some((text, rect, rows))
    }
    /// The text a hint key shows: `LOCAL/HELP.INI`, or this version's own.
    pub fn hint_text(&self, key: &str) -> String {
        if let Some((_, t, _)) = settings::SETTINGS_HINTS.iter().find(|(k, _, _)| *k == key) {
            return (*t).into();
        }
        EXTRA_HINTS
            .iter()
            .find(|(k, _)| *k == key)
            .map_or_else(|| self.hints.label("HELP", key), |(_, t)| (*t).into())
    }
    pub fn widgets(&self, menu: &Menu) -> Vec<Widget> {
        let mut widgets = menu.widgets(&self.locale);
        for bar in self.scrollbars(menu).into_iter().filter(|b| b.shown) {
            widgets.push(Widget {
                rect: bar.track,
                label: "Scorrimento".into(),
                action: Action::Scroll {
                    target: bar.target,
                    delta: bar.visible as i16,
                    max: bar.max(),
                },
                enabled: true,
                checked: None,
                value: None,
                hint: None,
                style: Style::Scroll,
            });
        }
        widgets
    }
    pub fn scrollbars(&self, menu: &Menu) -> Vec<Scrollbar> {
        let mut bars = Vec::new();
        let (fx, fy) = FILE_AT;
        let (mx, my) = menu.dialog_at();
        let list = match menu.screen {
            Screen::Campaign => Some((
                menu.campaign_missions().len(),
                9,
                r(332, 349, 26, 150),
                true,
            )),
            Screen::Missions => Some((menu.catalog.missions.len(), 15, r(332, 40, 26, 258), true)),
            Screen::Load => Some((
                menu.listed_saves().len(),
                SAVE_ROWS,
                r(fx + 478, fy + 74, 26, 153),
                true,
            )),
            Screen::Save => Some((
                menu.saves.len() + 1,
                SAVE_ROWS,
                r(fx + 478, fy + 74, 26, 153),
                true,
            )),
            Screen::Help => Some((
                self.help_lines.len(),
                (HELP_TEXT.h / self.line()) as usize,
                r(mx + 425, my + 84, 26, 152),
                true,
            )),
            Screen::Credits => Some((self.credit_lines.len(), 20, CREDITS_PAGE, false)),
            // The list of «COMPORTAMENTO IA» and its bar at (423, 88).
            Screen::Random if menu.choose_ai.is_some() => Some((
                menu.ai_scripts.len(),
                AI_ROWS,
                r(CHOOSE_AI_AT.0 + 423, CHOOSE_AI_AT.1 + 88, 26, 144),
                true,
            )),
            _ => None,
        };
        if let Some((total, visible, track, shown)) = list {
            bars.push(Scrollbar {
                target: ScrollTarget::List,
                track,
                total,
                visible,
                offset: menu.scroll.max(0) as usize,
                shown,
            });
        }
        for panel in 0..2 {
            if let Some((text, rect, visible)) = self.description_text(menu, panel) {
                let track = match (menu.screen, panel) {
                    (Screen::Campaign, 0) => r(743, 40, 26, 256),
                    (Screen::Setup, _) => r(736, 62, 26, 226),
                    _ => r(745, 349, 26, 150),
                };
                bars.push(Scrollbar {
                    target: ScrollTarget::Description(panel),
                    track,
                    total: self.small.wrap(&text, rect.w as usize).len(),
                    visible,
                    offset: menu.description_scroll[panel],
                    shown: true,
                });
            }
        }
        bars
    }

    fn face(&self, face: Face) -> &BitmapFont {
        match face {
            Face::Small => &self.small,
            Face::Big => &self.font,
            Face::Title => &self.title_font,
        }
    }
    fn write(&self, out: &mut Bitmap, t: &Text) {
        let font = self.face(t.face);
        let lines = font.wrap(&t.text, t.rect.w.max(1) as usize);
        if lines.len() <= 1 {
            write(font, out, &t.text, t.rect, t.color, t.centered);
            return;
        }
        let step = font.height as i32 + 1;
        for (i, line) in lines.iter().enumerate() {
            let y = t.rect.y + i as i32 * step;
            if y + step > t.rect.y + t.rect.h + 2 {
                break;
            }
            write(
                font,
                out,
                line,
                r(t.rect.x, y, t.rect.w, step),
                t.color,
                t.centered,
            );
        }
    }

    pub fn render(&self, menu: &Menu) -> Bitmap {
        let mut out = match Self::background(menu) {
            Some(key) => self.art[key].clone(),
            None => Bitmap {
                width: 800,
                height: 600,
                rgba: vec![0; 800 * 600 * 4],
            },
        };
        // The screen a dialog was opened from stays visible around it.
        let beneath = match menu.screen {
            Screen::Quit | Screen::Multiplayer => Some(Screen::Main),
            Screen::Load if !menu.over_map() => Some(Screen::Single),
            _ => None,
        };
        if let Some(screen) = beneath {
            for w in menu.screen_widgets(screen, &self.locale) {
                self.widget(&mut out, &w, false, false);
            }
        }
        if let Some((key, (x, y))) = Self::dialog(menu) {
            let b = &self.art[key];
            blit(
                &mut out,
                b,
                whole(b),
                r(x, y, b.width as i32, b.height as i32),
            );
            // 0x44a1e6: `screens/win.bmp` or `lose.bmp` centred on (118, 157)
            // of the end window.
            if menu.screen == Screen::End {
                let won = menu.ending.is_some_and(|e| e.0);
                let p = &self.art[if won { "WIN" } else { "LOSE" }];
                let (w, h) = (p.width as i32, p.height as i32);
                blit(
                    &mut out,
                    p,
                    whole(p),
                    r(x + 118 - (w - 1) / 2, y + 157 - (h - 1) / 2, w, h),
                );
            }
        }
        // Under «COMPORTAMENTO IA» the random map screen stays as it was.
        let dialog = menu
            .choose_ai
            .as_ref()
            .filter(|_| menu.screen == Screen::Random);
        let widgets = if dialog.is_some() {
            menu.screen_widgets(Screen::Random, &self.locale)
        } else {
            self.widgets(menu)
        };
        if menu.screen == Screen::Options {
            self.settings_backdrop(&mut out, &widgets);
        }
        for t in menu.texts(&self.locale) {
            if menu.screen == Screen::Main {
                // The version, right-aligned with the original's black shadow.
                let width = self.font.measure(&t.text) as i32;
                let x = 800 - 28 - width;
                write(
                    &self.font,
                    &mut out,
                    &t.text,
                    r(x + 1, t.rect.y + 1, width + 2, t.rect.h),
                    color::BLACK,
                    false,
                );
                write(
                    &self.font,
                    &mut out,
                    &t.text,
                    r(x, t.rect.y, width + 2, t.rect.h),
                    t.color,
                    false,
                );
            } else {
                self.write(&mut out, &t);
            }
        }
        // Description boxes.
        for panel in 0..2 {
            let Some((text, rect, rows)) = self.description_text(menu, panel) else {
                continue;
            };
            let lines = self.small.wrap(&text, rect.w as usize);
            let offset = menu.description_scroll[panel].min(lines.len().saturating_sub(rows));
            for (i, line) in lines.iter().skip(offset).take(rows).enumerate() {
                self.small.draw(
                    &mut out.rgba,
                    800,
                    600,
                    line,
                    rect.x,
                    rect.y + i as i32 * self.line(),
                    color::DARK_GOLD,
                );
            }
        }
        match menu.screen {
            Screen::Credits => self.credits(&mut out, menu.scroll.max(0) as usize),
            Screen::Help => {
                let rows = (HELP_TEXT.h / self.line()) as usize;
                let offset =
                    (menu.scroll.max(0) as usize).min(self.help_lines.len().saturating_sub(rows));
                // The text box goes with its window.
                let (wx, wy) = menu.dialog_at();
                let (tx, ty) = (
                    HELP_TEXT.x - MESSAGE_AT.0 + wx,
                    HELP_TEXT.y - MESSAGE_AT.1 + wy,
                );
                for (i, line) in self.help_lines.iter().skip(offset).take(rows).enumerate() {
                    let mut x = tx;
                    for (style, text) in line {
                        let c = if *style == 1 {
                            color::CREAM
                        } else {
                            color::GOLD
                        };
                        self.small.draw(
                            &mut out.rgba,
                            800,
                            600,
                            text,
                            x,
                            ty + i as i32 * self.line(),
                            c,
                        );
                        x += self.small.measure(text) as i32;
                    }
                }
            }
            Screen::Setup => {
                // The slots of each kingdom row (0x4c5f90): from (62, 20), 35 apart.
                let b = &self.art["S_SINGLESETUP/ROW BACK"];
                for i in 0..menu.setup_rows.len().min(8) as i32 {
                    blit(
                        &mut out,
                        b,
                        whole(b),
                        r(62, 20 + 35 * i, b.width as i32, b.height as i32),
                    );
                }
                if menu.setup_options.victory >= 2 {
                    self.value_frame(&mut out);
                }
            }
            Screen::Random => {
                // Native draw 0x4ca360: only active rows, at (62, 20), 35 apart.
                let b = &self.art["S_SINGLERND/ROW BACK"];
                for i in 0..menu.random_players.min(8) as i32 {
                    blit(
                        &mut out,
                        b,
                        whole(b),
                        r(62, 20 + 35 * i, b.width as i32, b.height as i32),
                    );
                }
                if menu.random_rules.victory >= 2 {
                    self.value_frame(&mut out);
                }
            }
            Screen::Campaign => {
                // The campaign list keeps its scroll bar even when all fit.
                self.bar(
                    &mut out,
                    r(332, 40, 26, 256),
                    r(332, 40 + ARROW, 26, ARROW),
                    None,
                );
            }
            _ => {}
        }
        for (i, w) in widgets.iter().enumerate() {
            let active = dialog.is_none() && w.enabled && menu.allows_action(&w.action);
            let hovered = active && !menu.keyboard_focus && menu.hover == Some(i);
            let focused = active && menu.keyboard_focus && menu.focus == i;
            let pressed = active && menu.pressed == Some(i) && menu.hover == Some(i);
            self.widget(&mut out, w, hovered || focused, pressed);
            if focused && w.style != Style::Hidden {
                focus_outline(&mut out, w.rect);
            }
        }
        for bar in self
            .scrollbars(menu)
            .into_iter()
            .filter(|b| b.shown && dialog.is_none())
        {
            let pressed = widgets.iter().enumerate().find_map(|(j, w)| {
                (matches!(w.action, Action::Scroll { target, .. } if target == bar.target)
                    && menu.pressed == Some(j))
                .then_some(())
            });
            self.bar(&mut out, bar.track, bar.thumb(), pressed);
        }
        if dialog.is_some() {
            self.choose_ai_window(&mut out, menu);
            return out;
        }
        if menu.screen == Screen::Options {
            self.settings_overlay(&mut out, menu, &widgets);
            self.settings_strip(&mut out, menu, &widgets);
        }
        // The hint of the control under the pointer, as the original draws it
        // (0x45c6c6): Arial12B in black on cream, 20 pixels below the
        // pointer, its width the text's plus 20, pushed left in proportion to
        // the pointer's position so it never leaves the screen.
        if menu.help
            && let (Some(i), Some([px, py]), false) =
                (menu.hover, menu.pointer, menu.keyboard_focus)
            && let Some(w) = widgets.get(i)
            && let Some(key) = menu.hint_key(w)
        {
            let text = self.hint_text(key);
            let width = self.small.measure(&text) as i32 + 20;
            let x = (px - px * width as f32 / 800.) as i32;
            let y = ((py + 20.) as i32).min(600 - 16);
            let r = r(x, y, width, 16);
            pixels(&mut out, r, |p| p.copy_from_slice(&[0, 0, 0, 255]));
            let inner = Rect {
                x: r.x + 1,
                y: r.y + 1,
                w: r.w - 2,
                h: r.h - 2,
            };
            pixels(&mut out, inner, |p| {
                p.copy_from_slice(&[0xff, 0xf3, 0xd1, 255])
            });
            write(&self.small, &mut out, &text, r, color::BLACK, true);
        }
        if !menu.notice.is_empty() {
            let rect = r(40, 556, 720, 22);
            veil(&mut out, rect, 210);
            write(
                &self.small,
                &mut out,
                &menu.notice,
                rect,
                color::CREAM,
                true,
            );
        }
        out
    }
    /// The frame of the time or score limit (0x4c32b0): `SCROLL CTRL.BMP`
    /// centred on its selector, as the backgrounds have no box there.
    fn value_frame(&self, out: &mut Bitmap) {
        let b = &self.art["COMMON/SCROLL CTRL"];
        let (w, h) = (b.width as i32, b.height as i32);
        let s = VALUE_SELECTOR;
        blit(
            out,
            b,
            whole(b),
            r(s.x + (s.w - w) / 2, s.y + (s.h - h) / 2, w, h),
        );
    }
    /// «COMPORTAMENTO IA» over the random map screen (0x48a060): its marble,
    /// the title centred at (44, 48), the list, its scroll bar and buttons.
    fn choose_ai_window(&self, out: &mut Bitmap, menu: &Menu) {
        let (x, y) = CHOOSE_AI_AT;
        let back = &self.art["D_CHOOSEAI/BACK"];
        blit(
            out,
            back,
            whole(back),
            r(x, y, back.width as i32, back.height as i32),
        );
        write(
            &self.font,
            out,
            &self.locale.label("chooseai", "title"),
            r(x + 44, y + 48, 407, 17),
            color::GOLD,
            true,
        );
        let widgets = self.widgets(menu);
        for (i, w) in widgets.iter().enumerate() {
            let hovered = !menu.keyboard_focus && menu.hover == Some(i);
            let focused = menu.keyboard_focus && menu.focus == i;
            let pressed = menu.pressed == Some(i) && menu.hover == Some(i);
            self.widget(
                out,
                w,
                w.enabled && (hovered || focused),
                w.enabled && pressed,
            );
            if focused && w.enabled {
                focus_outline(out, w.rect);
            }
        }
        for bar in self.scrollbars(menu).into_iter().filter(|b| b.shown) {
            let pressed = widgets.iter().enumerate().find_map(|(j, w)| {
                (matches!(w.action, Action::Scroll { target, .. } if target == bar.target)
                    && menu.pressed == Some(j))
                .then_some(())
            });
            self.bar(out, bar.track, bar.thumb(), pressed);
        }
    }
    /// A scroll bar of the original: arrows, groove, fixed thumb.
    fn bar(&self, out: &mut Bitmap, track: Rect, thumb: Rect, hovered: Option<()>) {
        let up = &self.art["COMMON/BTN UP"];
        let down = &self.art["COMMON/BTN DOWN"];
        let knob = &self.art["COMMON/THUMB VERT"];
        let s = hovered.is_some() as i32;
        blit(out, up, strip(up, 2, 0), r(track.x, track.y, ARROW, ARROW));
        blit(
            out,
            down,
            strip(down, 2, 0),
            r(track.x, track.y + track.h - ARROW, ARROW, ARROW),
        );
        blit(
            out,
            knob,
            strip(knob, 2, s),
            r(thumb.x, thumb.y, ARROW, ARROW),
        );
    }
    fn widget(&self, out: &mut Bitmap, w: &Widget, lit: bool, pressed: bool) {
        let rect = w.rect;
        match w.style {
            Style::Button(key) => {
                let b = &self.art[key];
                if key == "D_MENU/BTN BACK" {
                    // Four frames (normal, rollover, pressed, disabled), gold
                    // text with a black shadow, white under the pointer.
                    let (frame, c) = if !w.enabled {
                        (3, color::DISABLED)
                    } else if pressed {
                        (2, color::GOLD)
                    } else if lit {
                        (1, color::WHITE)
                    } else {
                        (0, color::GOLD)
                    };
                    blit(out, b, strip(b, 4, frame), rect);
                    let d = pressed as i32;
                    let at = r(rect.x + d, rect.y + d, rect.w, rect.h);
                    write(
                        &self.font,
                        out,
                        &w.label,
                        r(at.x + 1, at.y + 1, at.w, at.h),
                        color::BLACK,
                        true,
                    );
                    write(&self.font, out, &w.label, at, c, true);
                } else {
                    // Two frames, normal and pressed ("NS"): the pointer only
                    // lights the text, as in the original.
                    blit(out, b, strip(b, 2, (w.enabled && pressed) as i32), rect);
                    let c = if !w.enabled {
                        color::DISABLED
                    } else if lit || pressed {
                        color::GOLD
                    } else {
                        color::DARK_GOLD
                    };
                    let d = pressed as i32;
                    write(
                        &self.font,
                        out,
                        &w.label,
                        r(rect.x + d, rect.y + d, rect.w, rect.h),
                        c,
                        true,
                    );
                }
            }
            Style::Item | Style::Map(_) => {
                let mut x = rect.x + 2;
                if let Style::Map(kind) = w.style {
                    let icons = &self.art["COMMON/MAP TYPES"];
                    blit(
                        out,
                        icons,
                        column(icons, 4, kind as i32),
                        r(x, rect.y + 2, 12, 12),
                    );
                    x += 15;
                }
                let c = if w.checked == Some(true) {
                    color::GOLD
                } else {
                    color::DARK_GOLD
                };
                write(
                    &self.small,
                    out,
                    &w.label,
                    r(x, rect.y, rect.x + rect.w - x, rect.h),
                    c,
                    false,
                );
            }
            Style::Arrow { right } => {
                let b = &self.art[if right {
                    "COMMON/BTN RIGHT"
                } else {
                    "COMMON/BTN LEFT"
                }];
                // Normal, rollover and pressed ("NRS").
                let frame = if pressed {
                    2
                } else if lit && w.enabled {
                    1
                } else {
                    0
                };
                blit(out, b, strip(b, 3, frame), rect);
                if !w.enabled {
                    darken(out, rect);
                }
            }
            Style::Value => {
                let c = if w.enabled {
                    color::GOLD
                } else {
                    color::DISABLED
                };
                write(&self.small, out, &w.label, rect, c, true);
            }
            Style::Check => {
                let b = &self.art["COMMON/CHK"];
                let frame = if w.checked == Some(true) { 0 } else { 1 };
                let at = r(rect.x, rect.y, 31, 31);
                blit(out, b, column(b, 2, frame), at);
                if !w.enabled {
                    darken(out, at);
                }
            }
            Style::Tab
            | Style::Row { .. }
            | Style::Heading
            | Style::Note
            | Style::Switch
            | Style::Segment
            | Style::Plate
            | Style::Edit
            | Style::Dropdown
            | Style::DropItem
            | Style::Slider => self.settings_widget(out, w, lit, pressed),
            Style::KeyBox => {
                // A darkened slot with a gold rim, in the colours of the fields.
                let waiting = w.checked == Some(true);
                veil(out, rect, if waiting { 200 } else { 150 });
                let rim = [
                    color::DARK_GOLD[0],
                    color::DARK_GOLD[1],
                    color::DARK_GOLD[2],
                    255,
                ];
                for (x, y, ww, hh) in [
                    (rect.x, rect.y, rect.w, 1),
                    (rect.x, rect.y + rect.h - 1, rect.w, 1),
                    (rect.x, rect.y, 1, rect.h),
                    (rect.x + rect.w - 1, rect.y, 1, rect.h),
                ] {
                    pixels(out, r(x, y, ww, hh), |p| p.copy_from_slice(&rim));
                }
                let c = if waiting || lit {
                    color::CREAM
                } else {
                    color::GOLD
                };
                write(
                    &self.small,
                    out,
                    &w.label,
                    r(rect.x + 4, rect.y + 6, rect.w - 8, 15),
                    c,
                    true,
                );
            }
            Style::Field => {
                let b = &self.art["S_SINGLERND/FRAME"];
                blit(out, b, whole(b), rect);
                let editing = w.checked == Some(true);
                let text = if editing {
                    format!("{}|", w.label)
                } else {
                    w.label.clone()
                };
                let c = if editing || lit {
                    color::CREAM
                } else {
                    color::GOLD
                };
                write(
                    &self.small,
                    out,
                    &text,
                    r(rect.x + 10, rect.y + 9, rect.w - 20, 15),
                    c,
                    false,
                );
            }
            Style::Name => {
                let editing = w.checked == Some(true);
                let text = if editing {
                    format!("{}|", w.label)
                } else {
                    w.label.clone()
                };
                let c = if editing || lit {
                    color::CREAM
                } else {
                    color::GOLD
                };
                write(&self.small, out, &text, rect, c, false);
            }
            Style::Swatch(frame) | Style::Team(frame) => {
                let (key, count) = if matches!(w.style, Style::Swatch(_)) {
                    ("COMMON/COLOR BARS", 8)
                } else {
                    ("COMMON/TEAMS", 5)
                };
                let b = &self.art[key];
                blit(out, b, column(b, count, frame as i32), rect);
            }
            // 0x4cba36: dim, lit once the kingdom has a behaviour of its own.
            Style::AiIcon => {
                let b = &self.art["COMMON/AI"];
                blit(out, b, column(b, 2, (w.checked == Some(true)) as i32), rect);
            }
            // 0x4cbb15: the white flag while computers may surrender, the
            // swords otherwise; the second column under the pointer.
            Style::AiResign => {
                let b = &self.art["AIRESIGN"];
                let (fw, fh) = (b.width as i32 / 2, b.height as i32 / 2);
                let row = (w.checked != Some(true)) as i32;
                blit(out, b, r(lit as i32 * fw, row * fh, fw, fh), rect);
            }
            Style::HelpBall(art) => {
                // 0x41cb20: the column is the state among «ND» (0x41ca27),
                // normal or disabled; hover and press keep the normal one.
                // The row (+0x252) is 1, green, at creation and when help
                // mode ends, 0, red, while it is on (0x45c960).
                let b = &self.art[art];
                let (fw, fh) = (b.width as i32 / 2, b.height as i32 / 2);
                let column = (!w.enabled) as i32;
                let row = if w.checked == Some(true) { 0 } else { 1 };
                blit(out, b, r(column * fw, row * fh, fw, fh), rect);
            }
            Style::Scroll | Style::Hidden => {}
        }
    }
    fn credits(&self, out: &mut Bitmap, offset: usize) {
        let mut y = CREDITS_PAGE.y;
        for (style, line) in self.credit_lines.iter().skip(offset) {
            let font = &self.credit_fonts[(*style as usize).min(self.credit_fonts.len() - 1)];
            let c = match style {
                2 => [70, 38, 18],
                0 => [112, 32, 16],
                _ => [52, 36, 22],
            };
            let h = font.height as i32 + 2;
            if y + h > CREDITS_PAGE.y + CREDITS_PAGE.h {
                break;
            }
            write(
                font,
                out,
                line,
                r(CREDITS_PAGE.x, y, CREDITS_PAGE.w, h),
                c,
                true,
            );
            y += h;
        }
    }
}

/// Text with the original colour switches (`@1` highlight, `@2` normal),
/// wrapped to `width`.
pub fn rich_wrap(font: &BitmapFont, text: &str, width: usize) -> Vec<Vec<(u8, String)>> {
    let mut lines = Vec::new();
    let mut style = 2u8;
    for paragraph in text.lines() {
        let mut spans: Vec<(u8, String)> = Vec::new();
        let mut rest = paragraph;
        while !rest.is_empty() {
            match rest.find('@') {
                Some(at)
                    if rest[at + 1..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_digit()) =>
                {
                    if at > 0 {
                        spans.push((style, rest[..at].to_owned()));
                    }
                    style = rest.as_bytes()[at + 1] - b'0';
                    rest = &rest[at + 2..];
                }
                Some(at) => {
                    spans.push((style, rest[..=at].to_owned()));
                    rest = &rest[at + 1..];
                }
                None => {
                    spans.push((style, rest.to_owned()));
                    rest = "";
                }
            }
        }
        let mut line: Vec<(u8, String)> = Vec::new();
        let mut used = 0;
        for (s, chunk) in spans {
            for word in chunk.split_inclusive(' ') {
                let mut word = word;
                let w = font.measure(word.trim_end());
                if used > 0 && used + w > width {
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                    word = word.trim_start();
                }
                match line.last_mut() {
                    Some((ls, text)) if *ls == s => text.push_str(word),
                    _ => line.push((s, word.to_owned())),
                }
                used += font.measure(word);
            }
        }
        lines.push(line);
    }
    lines
}

fn write(font: &BitmapFont, out: &mut Bitmap, text: &str, r: Rect, color: [u8; 3], centered: bool) {
    font.draw_fit(
        &mut out.rgba,
        [out.width as usize, out.height as usize],
        text,
        [r.x, r.y, r.w, r.h],
        color,
        centered,
    );
}
fn whole(b: &Bitmap) -> Rect {
    r(0, 0, b.width as i32, b.height as i32)
}
/// Frame `index` of `count` equal frames laid side by side.
fn strip(b: &Bitmap, count: i32, index: i32) -> Rect {
    let w = b.width as i32 / count.max(1);
    r(w * index.clamp(0, count - 1), 0, w, b.height as i32)
}
/// Frame `index` of `count` equal frames stacked vertically.
fn column(b: &Bitmap, count: i32, index: i32) -> Rect {
    let h = b.height as i32 / count.max(1);
    r(0, h * index.clamp(0, count - 1), b.width as i32, h)
}
/// Copy `src` region `from` at the top-left corner of `to`, skipping
/// transparent pixels (no scaling: the art is drawn at its own size).
fn blit(out: &mut Bitmap, src: &Bitmap, from: Rect, to: Rect) {
    // A visible pixel covers the texts recorded under it (`font::record`).
    let canvas = [out.width as usize, out.height as usize];
    crate::font::with_ids(canvas, |mut cover| {
        for y in 0..from.h {
            for x in 0..from.w {
                let (sx, sy, tx, ty) = (from.x + x, from.y + y, to.x + x, to.y + y);
                if sx < 0
                    || sy < 0
                    || tx < 0
                    || ty < 0
                    || sx >= src.width as i32
                    || sy >= src.height as i32
                    || tx >= out.width as i32
                    || ty >= out.height as i32
                {
                    continue;
                }
                let s = ((sy as u32 * src.width + sx as u32) * 4) as usize;
                let t = ((ty as u32 * out.width + tx as u32) * 4) as usize;
                if src.rgba[s + 3] != 0 {
                    out.rgba[t..t + 4].copy_from_slice(&src.rgba[s..s + 4]);
                    if let Some(cover) = cover.as_mut() {
                        cover.mark(t / 4);
                    }
                }
            }
        }
    });
}
fn pixels(b: &mut Bitmap, r: Rect, mut f: impl FnMut(&mut [u8])) {
    // What it changes covers the texts recorded under it (`font::record`).
    let canvas = [b.width as usize, b.height as usize];
    crate::font::with_ids(canvas, |mut cover| {
        for y in r.y.max(0)..(r.y + r.h).min(b.height as i32) {
            for x in r.x.max(0)..(r.x + r.w).min(b.width as i32) {
                let k = y as usize * b.width as usize + x as usize;
                f(&mut b.rgba[k * 4..k * 4 + 4]);
                if let Some(cover) = cover.as_mut() {
                    cover.mark(k);
                }
            }
        }
    });
}
/// Greyed-out control.
fn darken(b: &mut Bitmap, r: Rect) {
    pixels(b, r, |p| {
        for v in &mut p[..3] {
            *v /= 2;
        }
    });
}
/// A dark band that stays readable over the map.
fn veil(b: &mut Bitmap, r: Rect, alpha: u8) {
    pixels(b, r, |p| {
        for v in &mut p[..3] {
            *v = (*v as u16 * (255 - alpha) as u16 / 255) as u8;
        }
        p[3] = p[3].max(alpha);
    });
}
fn focus_outline(out: &mut Bitmap, r: Rect) {
    let c = [245, 236, 210, 255];
    let mut dot = |x: i32, y: i32| {
        pixels(out, Rect { x, y, w: 1, h: 1 }, |p| p.copy_from_slice(&c));
    };
    for x in (r.x + 1..r.x + r.w - 1).step_by(3) {
        dot(x, r.y + 1);
        dot(x, r.y + r.h - 2);
    }
    for y in (r.y + 1..r.y + r.h - 1).step_by(3) {
        dot(r.x + 1, y);
        dot(r.x + r.w - 2, y);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_missions_reached_are_kept_sealed_and_read_from_the_plain_file_of_before() {
        let reached: std::collections::BTreeSet<String> = [
            "tzar.wcm::maps/saa3".to_owned(),
            "tzar.wcm::maps/v_00".to_owned(),
        ]
        .into();
        let sealed = super::sealed_progress(&reached);
        assert!(!sealed.windows(4).any(|w| w == b"saa3"));
        assert_eq!(super::read_progress(Some(sealed.clone()), None), reached);
        let plain = serde_json::to_vec(&reached).unwrap();
        assert_eq!(super::read_progress(None, Some(plain.clone())), reached);
        // A sealed file changed is not believed; the plain one of before is.
        let mut changed = sealed;
        changed[20] ^= 1;
        assert_eq!(super::read_progress(Some(changed), Some(plain)), reached);
        assert!(super::read_progress(None, None).is_empty());
    }
    use super::*;

    /// A test's folder with settings in Italian: the tests read the
    /// installation's own texts, the default is English, and the language
    /// is one for the whole process.
    fn italian(root: &Path) -> &Path {
        let settings = root.join("settings.json");
        if !settings.exists() {
            std::fs::create_dir_all(root).unwrap();
            let language = crate::lang::Language::Italian.index();
            std::fs::write(settings, format!("{{\"language\": {language}}}")).unwrap();
        }
        root
    }

    #[test]
    fn random_screen_limits_match_original_machine_code() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/random-native/screen-size-rules.json"
        ))
        .unwrap();
        for c in cases.as_array().unwrap() {
            let side = c["side"].as_u64().unwrap() as usize;
            let kind = c["input_kind"].as_u64().unwrap() as usize;
            let mut players = 8;
            let mut rows: Vec<_> = (0..8)
                .map(|i| SetupRow {
                    record: i,
                    name: format!("Tzar {}", i + 1),
                    color: i as u8 + 1,
                    team: i as u8 % 5,
                    race: i % 4,
                    human: i == 7,
                    ai: String::new(),
                })
                .collect();
            let human = rows[7].clone();
            let mut options = crate::random_map::Options {
                kind,
                ..Default::default()
            };
            constrain_random(side, &mut players, &mut rows, &mut options);
            assert_eq!(players, c["players"].as_u64().unwrap() as usize);
            assert_eq!(options.kind, c["kind"].as_u64().unwrap() as usize);
            for (k, enabled) in c["enabled"].as_array().unwrap().iter().enumerate() {
                assert_eq!(random_kind_allowed(side, k), enabled.as_bool().unwrap());
            }
            let moved = &rows[players - 1];
            assert!(moved.human);
            assert_eq!(
                (moved.color, moved.team, moved.race),
                (human.color, human.team, human.race)
            );
            assert!(rows.iter().enumerate().all(|(i, r)| r.record == i));
        }
    }

    #[test]
    #[ignore = "Requires installed original archives; no window"]
    fn random_rows_teams_names_and_behaviours_follow_the_original() {
        let source = Path::new("..");
        let assets = GameAssets::open(source).unwrap();
        let skin = MenuSkin::load(&assets).unwrap();
        let root = std::env::temp_dir().join(format!("retzar-random-rows-{}", std::process::id()));
        let mut menu = Menu::new(source, italian(&root)).unwrap();
        // The European names of HEROES.INI open the original table (0x499b60).
        let text = legacy_text(&assets.get("DATA/HEROES.INI").unwrap());
        let euro: Vec<&str> = text
            .lines()
            .map(str::trim)
            .skip_while(|l| !l.eq_ignore_ascii_case("[euro]"))
            .skip(1)
            .take_while(|l| !l.starts_with('['))
            .filter(|l| !l.is_empty())
            .collect();
        menu.open(Screen::Single);
        menu.open(Screen::Random);
        menu.random_players = 8;
        // 0x4c7ac5: colours in order, random civilizations, the player on
        // team «1» and every computer on «2», computers named after heroes.
        let rows = menu.random_rows.clone();
        assert_eq!(rows[0].name, menu.settings.player_name);
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row.color, i as u8 + 1);
            assert_eq!(row.race, 0);
            assert_eq!(row.human, i == 0);
            assert_eq!(
                (row.team, row.team_frame()),
                if i == 0 { (1, 0) } else { (2, 1) }
            );
            assert!(row.ai.is_empty());
            if i > 0 {
                assert_eq!(row.name, euro[i - 1]);
            }
        }
        // A new visit takes the next free names, until a game starts.
        menu.open(Screen::Single);
        menu.open(Screen::Random);
        assert_eq!(menu.random_rows[1].name, euro[7]);
        // Teams cycle «1»…«4», then none (the empty frame 4).
        let frames: Vec<u8> = (0..5)
            .map(|_| {
                menu.activate(Action::SetupTeam(1)).unwrap();
                menu.random_rows[1].team_frame()
            })
            .collect();
        assert_eq!(frames, [2, 3, 4, 0, 1]);
        // 0x4cbbc0: clicking a computer's name trades the two rows.
        let merlin = menu.random_rows[3].clone();
        menu.activate(Action::SetupPlayer(3)).unwrap();
        assert!(menu.random_rows[3].human);
        assert_eq!(
            (menu.random_rows[3].color, menu.random_rows[3].team),
            (1, 1)
        );
        assert_eq!(menu.random_rows[0].name, merlin.name);
        assert_eq!(menu.random_rows[0].color, merlin.color);
        assert!(
            menu.random_rows
                .iter()
                .enumerate()
                .all(|(i, r)| r.record == i)
        );
        // «COMPORTAMENTO IA»: the only window taking input while open.
        menu.activate(Action::ChooseAi(2)).unwrap();
        let widgets = skin.widgets(&menu);
        assert!(widgets.iter().all(|w| matches!(
            w.action,
            Action::AiScript(_) | Action::AiAccept | Action::AiCancel | Action::Scroll { .. }
        )));
        assert_eq!(menu.ai_scripts[0].0, "");
        let war = menu
            .ai_scripts
            .iter()
            .position(|(_, name)| name == "Guerra")
            .unwrap();
        menu.activate(Action::AiScript(war)).unwrap();
        menu.activate(Action::AiAccept).unwrap();
        assert_eq!(menu.random_rows[2].ai, "DATA\\AI\\War.ais");
        // Esc leaves the window like «Annulla».
        menu.activate(Action::ChooseAi(2)).unwrap();
        assert_eq!(menu.choose_ai.as_ref().map(|d| d.selected), Some(war));
        menu.activate(Action::AiScript(0)).unwrap();
        menu.activate(Action::Back).unwrap();
        assert_eq!(
            (menu.screen, menu.random_rows[2].ai.as_str()),
            (Screen::Random, "DATA\\AI\\War.ais")
        );
        assert!(
            skin.widgets(&menu)
                .iter()
                .any(|w| w.action == Action::ChooseAi(2) && w.checked == Some(true))
        );
        menu.activate(Action::AiResign).unwrap();
        let Effect::Load(_) = menu.activate(Action::Generate).unwrap() else {
            panic!()
        };
        let setup = menu.setup.clone().unwrap();
        assert!(setup.ai_resign);
        assert_eq!(setup.kingdoms[2].ai, "DATA\\AI\\War.ais");
        assert_eq!(
            setup.kingdoms[0].name.as_deref(),
            Some(merlin.name.as_str())
        );
        assert_eq!(
            setup.kingdoms[3].name.as_deref(),
            Some(menu.settings.player_name.as_str())
        );
        // The game frees the names (0x4999b0); the rows are not kept.
        menu.open(Screen::Single);
        menu.open(Screen::Random);
        assert_eq!(menu.random_rows[1].name, euro[0]);
        assert!(menu.random_rows[0].human);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "Requires installed original archives; no window"]
    fn victory_limit_selector_has_its_frame_and_every_control_its_hint() {
        let source = Path::new("..");
        let assets = GameAssets::open(source).unwrap();
        let skin = MenuSkin::load(&assets).unwrap();
        let root = std::env::temp_dir().join(format!("retzar-hints-{}", std::process::id()));
        let mut menu = Menu::new(source, italian(&root)).unwrap();
        let (None, custom) = menu.catalog.locate("mp4.wmp::maps/mp4").unwrap() else {
            panic!()
        };
        menu.open(Screen::Missions);
        menu.activate(Action::Custom(custom)).unwrap();
        menu.activate(Action::Setup).unwrap();
        menu.activate(Action::SetupOption(SetupOption::MapDefaults, 1))
            .unwrap();
        let plain = skin.render(&menu);
        menu.activate(Action::SetupOption(SetupOption::Victory, 2))
            .unwrap();
        // 0x4c506d: the limit at (66, 541), 191 wide, in its own frame
        // centred on it; its label at (68, 527).
        let widgets = skin.widgets(&menu);
        let limit: Vec<_> = widgets
            .iter()
            .filter(|w| matches!(w.action, Action::SetupOption(SetupOption::VictoryLimit, _)))
            .collect();
        assert_eq!(limit.len(), 3);
        assert_eq!((limit[0].rect.x, limit[0].rect.y), (66, 541));
        assert_eq!(limit[2].rect.x + limit[2].rect.w, 66 + 191);
        assert!(limit.iter().all(|w| w.hint == Some("CM1_VALUE")));
        let framed = skin.render(&menu);
        let frame = &skin.art["COMMON/SCROLL CTRL"];
        let (x, y) = (66 + 180, 538 + 1);
        let at = |b: &Bitmap| b.rgba[(y * 800 + x) * 4..][..4].to_vec();
        assert_eq!(
            framed.rgba[(y * 800 + x) * 4..][..3],
            frame.rgba[(180 + frame.width as usize) * 4..][..3]
        );
        assert_ne!(at(&framed), at(&plain));
        // Every control of the pages before a game and of the options
        // shows a hint, but those the original leaves without one: the
        // «?» ball, list bars, the computer icons and «Resa dell'IA».
        let mut pages = vec![(Screen::Setup, skin.widgets(&menu))];
        menu.open(Screen::Single);
        menu.open(Screen::Random);
        pages.push((Screen::Random, skin.widgets(&menu)));
        // Every tab of the settings, from the menu and from a game.
        for from in [Screen::Single, Screen::Pause] {
            for tab in SettingsTab::ALL {
                menu.open(from);
                menu.tab = tab;
                menu.open(Screen::Options);
                pages.push((Screen::Options, skin.widgets(&menu)));
            }
        }
        for (screen, widgets) in pages {
            menu.screen = screen;
            for w in widgets.iter().filter(|w| {
                !matches!(
                    w.action,
                    Action::Help
                        | Action::ChooseAi(_)
                        | Action::AiResign
                        | Action::Scroll {
                            target: ScrollTarget::List,
                            ..
                        }
                ) && !matches!(w.style, Style::Heading | Style::Note)
            }) {
                let key = menu
                    .hint_key(w)
                    .unwrap_or_else(|| panic!("{screen:?}: {:?} senza suggerimento", w.action));
                assert_ne!(skin.hint_text(key), key, "{screen:?}: {key}");
            }
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "Requires installed original artwork; no window"]
    fn random_screen_uses_original_row_pixels_and_hides_unused_slots() {
        let source = Path::new("..");
        let assets = GameAssets::open(source).unwrap();
        let skin = MenuSkin::load(&assets).unwrap();
        let row = Bitmap::decode(
            &assets
                .get("IMAGES/SCREENS/S_SINGLERND/ROW BACK.BMP")
                .unwrap(),
            true,
        )
        .unwrap();
        let back = Bitmap::decode(
            &assets.get("IMAGES/SCREENS/S_SINGLERND/BACK.BMP").unwrap(),
            false,
        )
        .unwrap();
        assert_eq!((row.width, row.height), (471, 33));
        let root =
            std::env::temp_dir().join(format!("retzar-random-screen-{}", std::process::id()));
        let mut menu = Menu::new(source, italian(&root)).unwrap();
        menu.open(Screen::Random);
        for players in [2, 4, 6, 8] {
            menu.random_players = players;
            let actual = skin.render(&menu);
            for i in 0..8 {
                for x in 0..row.width as usize {
                    let dst = ((20 + 35 * i) * 800 + 62 + x) * 4;
                    let pixel = &row.rgba[x * 4..x * 4 + 4];
                    let expected = if i < players && pixel[3] != 0 {
                        pixel
                    } else {
                        &back.rgba[dst..dst + 4]
                    };
                    assert_eq!(&actual.rgba[dst..dst + 4], expected, "row {i}, x {x}");
                }
            }
            assert!(
                !menu
                    .widgets(&skin.locale)
                    .iter()
                    .any(|w| matches!(w.action, Action::SetupRace(i, _) if i >= players))
            );
            if let Some(dir) = std::env::var_os("TZAR_RANDOM_REVIEW_DIR") {
                let dir = Path::new(&dir);
                std::fs::create_dir_all(dir).unwrap();
                std::fs::write(
                    dir.join(format!("setup-{players}-players.bmp")),
                    actual.to_bmp([0; 3]),
                )
                .unwrap();
            }
        }
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn scrollbar_drag_reaches_endpoints_and_preserves_grab_position() {
        let mut bar = Scrollbar {
            target: ScrollTarget::List,
            track: Rect {
                x: 100,
                y: 40,
                w: 26,
                h: 200,
            },
            total: 100,
            visible: 10,
            offset: 0,
            shown: true,
        };
        // The thumb runs between the arrows and keeps its size.
        assert_eq!(bar.thumb().y, bar.track.y + ARROW);
        assert_eq!(bar.thumb().h, ARROW);
        assert_eq!(bar.offset_at(45., 5.), 0);
        assert_eq!(bar.offset_at(500., 5.), 90);
        bar.offset = 90;
        assert_eq!(bar.thumb().y + bar.thumb().h, bar.down().y);
        assert_eq!(bar.offset_at(bar.thumb().y as f32 + 7., 7.), 90);
        assert_eq!(bar.offset_at(-100., 7.), 0);
    }
    #[test]
    fn earlier_window_sizes_and_frame_caps_become_the_new_ones() {
        // Files of earlier versions kept both by index.
        let mut s: Settings =
            serde_json::from_str("{\"resolution\":1,\"frame_limit\":3,\"vsync\":true}").unwrap();
        s.normalize();
        assert_eq!((s.window, s.max_fps, s.vsync), ([1920, 1080], 144, true));
        let written = serde_json::to_string(&s).unwrap();
        assert!(!written.contains("resolution") && !written.contains("frame_limit"));
        let back: Settings = serde_json::from_str(&written).unwrap();
        assert_eq!((back.window, back.max_fps), ([1920, 1080], 144));
        // A cap is kept within the range; a window never below the smallest.
        let mut s: Settings =
            serde_json::from_str("{\"max_fps\":5000,\"window\":[10,10]}").unwrap();
        s.normalize();
        assert_eq!((s.max_fps, s.window), (FPS_RANGE.1, MIN_WINDOW));
    }
    #[test]
    fn the_frames_slider_runs_from_thirty_to_no_limit() {
        assert_eq!(
            (slider_fps(0), slider_fps(99), slider_fps(100)),
            (30, 300, 0)
        );
        assert_eq!(fps_slider(0), 100);
        for fps in [30, 60, 120, 144, 240, 300] {
            let back = slider_fps(fps_slider(fps));
            assert!(back.abs_diff(fps) <= 2, "{fps} → {back}");
        }
        // Past its range a typed cap stands at the slider's last step.
        assert_eq!(fps_slider(999), 99);
    }
    #[test]
    #[ignore = "Requires installed original data"]
    fn every_control_of_every_menu_screen_answers() {
        let root =
            std::env::temp_dir().join(format!("retzar-menu-controls-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let skin = MenuSkin::load(&GameAssets::open(Path::new("..")).unwrap()).unwrap();
        // What a control can change: the screen, the line of notices, every
        // control's words, state, value and use, the kingdoms' rows (their
        // colours are drawn, not written) and the scrolled lists and texts.
        let seen = |menu: &Menu| {
            let controls: Vec<_> = skin
                .widgets(menu)
                .into_iter()
                .map(|w| (w.label, w.checked, w.value, w.enabled, w.action))
                .collect();
            format!(
                "{:?} {} {controls:?} {:?} {:?} {} {:?}",
                menu.screen,
                menu.notice,
                menu.random_rows,
                menu.setup_rows,
                menu.scroll,
                menu.description_scroll
            )
        };
        // Each screen is reached as the player reaches it.
        let fresh = |screen: Screen| {
            let mut menu = Menu::new(Path::new(".."), italian(&root)).unwrap();
            let from = match screen {
                Screen::Campaign | Screen::Missions | Screen::Random | Screen::Load => {
                    vec![Screen::Single]
                }
                Screen::Setup => vec![Screen::Single, Screen::Missions],
                Screen::Save | Screen::Help | Screen::Resign => vec![Screen::Pause],
                _ => vec![],
            };
            for s in from {
                menu.open(s);
            }
            menu.open(screen);
            menu
        };
        let mut silent = Vec::new();
        let mut tried = 0;
        for screen in Screen::ALL {
            let shown = skin.widgets(&fresh(screen));
            for w in shown {
                // A bar with nothing to scroll: the original keeps it.
                let idle = matches!(w.action, Action::Scroll { max: 0, .. });
                if !w.enabled || w.action == Action::Inert || idle {
                    continue;
                }
                let mut menu = fresh(screen);
                // «Predefiniti» has something to put back.
                if w.action == Action::Defaults {
                    menu.draft.sound = 0;
                }
                let before = seen(&menu);
                tried += 1;
                match menu.activate(w.action.clone()) {
                    // A refusal tells why; any other effect leaves the menu.
                    Err(_) | Ok(Effect::Exit | Effect::Load(_) | Effect::Restore(_)) => {}
                    Ok(Effect::Save { .. } | Effect::Resume | Effect::Restart) => {}
                    Ok(Effect::NextMap | Effect::SettingsChanged) => {}
                    Ok(Effect::None) => {
                        // A choice already made may stay as it is.
                        if seen(&menu) == before && w.checked != Some(true) {
                            silent.push(format!("{screen:?}: {:?} «{}»", w.action, w.label));
                        }
                    }
                }
            }
        }
        let _ = std::fs::remove_dir_all(&root);
        assert!(tried > 100, "{tried}");
        assert!(silent.is_empty(), "{}", silent.join("\n"));
    }
    #[test]
    #[ignore = "Requires installed original data"]
    fn every_control_of_the_settings_changes_what_it_shows() {
        let root =
            std::env::temp_dir().join(format!("retzar-settings-controls-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut menu = Menu::new(Path::new(".."), italian(&root)).unwrap();
        let skin = MenuSkin::load(&GameAssets::open(Path::new("..")).unwrap()).unwrap();
        menu.open(Screen::Pause);
        menu.open(Screen::Options);
        assert!(menu.in_game());
        // Twice: with the defaults, then with every switch the other way.
        let mut checked = 0;
        for round in 0..2 {
            if round == 1 {
                menu.draft.vsync = false;
                menu.draft.weather = !menu.draft.weather;
                menu.draft.speech = !menu.draft.speech;
                menu.draft.fullscreen = !menu.draft.fullscreen;
            }
            for tab in SettingsTab::ALL {
                menu.activate(Action::Tab(tab)).unwrap();
                let mut keys: Vec<(OptionKey, bool)> = Vec::new();
                for w in skin.widgets(&menu) {
                    let key = match w.action {
                        Action::Change(k, _) | Action::Set(k, _) | Action::Dropdown(k) => k,
                        _ => continue,
                    };
                    let changed = |menu: &mut Menu| {
                        let before = menu.draft.clone();
                        menu.activate(w.action.clone()).unwrap();
                        // A list opens: its first other choice is taken.
                        if menu.dropdown.is_some() {
                            let items = skin.widgets(menu);
                            let other = items
                                .iter()
                                .find(|i| i.style == Style::DropItem && i.checked != Some(true))
                                .unwrap();
                            menu.activate(other.action.clone()).unwrap();
                        }
                        let changed = menu.draft != before;
                        // What the game reads is what the page shows.
                        assert_eq!(menu.current_settings(), &menu.draft);
                        menu.draft = before;
                        changed
                    };
                    let did = w.enabled && changed(&mut menu);
                    match keys.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, any)) => *any |= did,
                        None => keys.push((key, did)),
                    }
                }
                checked += keys.len();
                for (key, any) in keys {
                    let resting = key == OptionKey::Frames && menu.draft.vsync
                        || key == OptionKey::Resolution && menu.draft.fullscreen;
                    assert_eq!(any, !resting, "{tab:?}, {key:?}, round {round}");
                }
            }
        }
        assert!(checked >= 60);
        // A command's key, given on the keys' tab.
        menu.activate(Action::Tab(SettingsTab::Keys)).unwrap();
        let bind = skin
            .widgets(&menu)
            .into_iter()
            .find(|w| matches!(w.action, Action::BindKey(_)))
            .unwrap();
        menu.activate(bind.action.clone()).unwrap();
        let Action::BindKey(i) = bind.action else {
            unreachable!()
        };
        menu.bind_key("F5");
        assert_eq!(menu.current_settings().key(crate::keys::ACTIONS[i].0), "F5");
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    #[ignore = "Requires installed original data"]
    fn the_settings_work_at_once_type_the_frames_and_open_lists() {
        let root =
            std::env::temp_dir().join(format!("retzar-settings-page-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut menu = Menu::new(Path::new(".."), italian(&root)).unwrap();
        let skin = MenuSkin::load(&GameAssets::open(Path::new("..")).unwrap()).unwrap();
        let fps_enabled = |menu: &Menu| {
            skin.widgets(menu)
                .iter()
                .find(|w| w.action == Action::EditFps)
                .unwrap()
                .enabled
        };
        menu.tab = SettingsTab::Display;
        let kept = menu.settings.clone();
        let kept_file = std::fs::read(root.join("settings.json")).unwrap();
        menu.open(Screen::Options);
        // No «Applica»: three buttons.
        assert_eq!(
            skin.widgets(&menu)
                .iter()
                .filter(|w| w.style == Style::Plate)
                .count(),
            3
        );
        // With the vertical synchronization the frames' limit rests.
        assert!(menu.draft.vsync && !fps_enabled(&menu));
        menu.activate(Action::EditFps).unwrap();
        menu.activate(Action::Change(OptionKey::Frames, -5))
            .unwrap();
        assert_eq!((menu.editing_fps.as_ref(), menu.draft.max_fps), (None, 0));
        assert_eq!(menu.draft.frame_cap(), 0);
        // What changes outside the menu is put in place at once.
        assert!(matches!(
            menu.activate(Action::Change(OptionKey::Vsync, 1)).unwrap(),
            Effect::SettingsChanged
        ));
        assert!(fps_enabled(&menu));
        // The frames written by hand: digits only, 0 for no limit, the
        // range kept; Esc drops what is written and stays on the page.
        menu.activate(Action::EditFps).unwrap();
        menu.type_fps("1a44");
        menu.commit_fps();
        assert_eq!(menu.draft.max_fps, 144);
        menu.activate(Action::EditFps).unwrap();
        menu.type_fps("5");
        menu.activate(Action::Tab(SettingsTab::Display)).unwrap();
        assert_eq!(menu.draft.max_fps, FPS_RANGE.0);
        menu.activate(Action::EditFps).unwrap();
        menu.type_fps("0");
        menu.activate(Action::Back).unwrap();
        assert_eq!(
            (menu.screen, menu.draft.max_fps),
            (Screen::Options, FPS_RANGE.0)
        );
        assert_eq!(menu.current_settings().frame_cap(), FPS_RANGE.0);
        // A list opens over the rows, its choices first under the pointer;
        // a choice closes it, Esc only closes it.
        menu.activate(Action::Tab(SettingsTab::Game)).unwrap();
        menu.activate(Action::Dropdown(OptionKey::Language))
            .unwrap();
        let widgets = skin.widgets(&menu);
        assert_eq!(widgets[0].style, Style::DropItem);
        // English above Italian.
        assert_eq!(
            (widgets[0].label.as_str(), widgets[1].label.as_str()),
            ("English", "Italiano")
        );
        assert_eq!(
            widgets[crate::lang::LANGUAGES.len()].action,
            Action::CloseDropdown
        );
        menu.activate(Action::Back).unwrap();
        assert_eq!((menu.screen, menu.dropdown), (Screen::Options, None));
        menu.activate(Action::Dropdown(OptionKey::Language))
            .unwrap();
        assert!(matches!(
            menu.activate(Action::Set(OptionKey::Language, 0)).unwrap(),
            Effect::SettingsChanged
        ));
        assert_eq!((menu.draft.language, menu.dropdown), (0, None));
        // A size of the list for the window.
        assert!(matches!(
            menu.activate(Action::Set(OptionKey::Resolution, 1))
                .unwrap(),
            Effect::SettingsChanged
        ));
        assert_eq!(menu.current_settings().window, [1600, 900]);
        // A change seen only in the menus asks nothing of the rest.
        assert!(matches!(
            menu.activate(Action::Change(OptionKey::Weather, 1))
                .unwrap(),
            Effect::None
        ));
        // «Annulla» brings everything back and has it put in place again.
        assert!(matches!(
            menu.activate(Action::Back).unwrap(),
            Effect::SettingsChanged
        ));
        assert!(!menu.screen.is_settings());
        assert_eq!(menu.current_settings(), &kept);
        assert_eq!(
            std::fs::read(root.join("settings.json")).unwrap(),
            kept_file
        );
        // «OK» keeps them and saves.
        menu.open(Screen::Options);
        menu.activate(Action::Change(OptionKey::Vsync, 1)).unwrap();
        assert!(matches!(
            menu.activate(Action::Apply).unwrap(),
            Effect::SettingsChanged
        ));
        assert!(!Settings::load(&root.join("settings.json")).unwrap().vsync);
        assert!(!menu.current_settings().vsync);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn player_name_defaults_to_the_windows_user_and_is_limited() {
        let mut s = Settings {
            player_name: "   ".into(),
            ..Settings::default()
        };
        s.normalize();
        assert_eq!(s.player_name, default_player_name());
        assert!(!s.player_name.is_empty());
        s.player_name = "x".repeat(40);
        s.normalize();
        assert_eq!(s.player_name.chars().count(), NAME_LENGTH);
        // Files saved before the option existed get the default name.
        let old: Settings = serde_json::from_str("{\"sound\": 10}").unwrap();
        assert_eq!(old.player_name, default_player_name());
    }
    #[test]
    fn saved_settings_are_clamped_before_use() {
        let mut s: Settings = serde_json::from_str(
            "{\"resolution\":999,\"sound\":255,\"backend\":2,\"layout\":999,\"speed\":200}",
        )
        .unwrap();
        s.normalize();
        assert_eq!(s.window, [3840, 2160]);
        assert_eq!(s.sound, 100);
        // The game speed belongs to the game: an old file's is ignored.
        assert_eq!(s.speed, crate::simulation::NORMAL_SPEED);
        assert!(s.layout < 2);
        assert_eq!((s.resolution, s.frame_limit, s.max_fps), (None, None, 0));
        // The original on/off boxes silence a group without losing its level.
        s.sound_on = false;
        assert_eq!((s.sound_volume(), s.sound), (0, 100));
    }
    #[test]
    fn damaged_settings_never_block_startup_and_saving_is_atomic() {
        let dir = std::env::temp_dir().join(format!("retzar-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"{\"sound\": 300, \"music\": ").unwrap();
        assert!(Settings::load(&path).is_err());
        let (s, warning) = Settings::load_or_default(&path);
        assert_eq!(s, Settings::default());
        assert!(warning.is_some());
        assert!(!path.exists() && dir.join("settings.json.corrotto").exists());
        let custom = Settings {
            music: 12,
            ui_mode: 1,
            ..Settings::default()
        };
        custom.save(&path).unwrap();
        custom.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), custom);
        assert!(!dir.join("settings.json.tmp").exists());
        // Files written before the interface option existed keep working.
        std::fs::write(&path, b"{\"resolution\": 1}").unwrap();
        assert!(Settings::load(&path).unwrap().crisp());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn legacy_slots_without_scenario_id_still_load() {
        let old: SavedSession = serde_json::from_str(
            "{\"selection\":{\"Scenario\":{\"campaign\":0,\"mission\":2}},\"camera\":[1.0,2.0,0.75]}",
        )
        .unwrap();
        assert_eq!(old.version, 1);
        assert!(old.scenario.is_none() && !old.missing);
        let new = SavedSession {
            version: SAVE_VERSION,
            scenario: Some("mp1.wmp::maps/mp1".into()),
            ..old
        };
        let text = serde_json::to_string(&new).unwrap();
        assert!(text.contains("mp1.wmp::maps/mp1") && !text.contains("missing"));
    }
}
