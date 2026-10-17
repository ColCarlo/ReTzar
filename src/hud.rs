//! The mission interface of the original (Tzar.exe 1.11, LAYOUT800L/R):
//! the stone frame of `SCREEN 800 LEFT/RIGHT`, the selection view, the
//! command grid and the in-game windows, at the positions the executable
//! uses. Player orders, production progress and inventories are backed by the simulation.
//!
//! Panel geometry (800×600 art, left layout; the right layout mirrors it):
//! - selection view at (6,169)–(123,325): portrait 48×48 at +6,+5 with
//!   `ORDERS\frame1.bmp`, hit points “%d/%d” at +3,+66, bar 54×3 at +3,+81,
//!   name at +3,+93, five 20×20 statistics at +66,+3+22k with their values
//!   at +88,+6+22k, and three 25×25 buttons (previous page, holder, next
//!   page) at y 302;
//! - command grid at (16,332): 2×4 cells of 48×48 icons from
//!   `ORDERS\buttons.bmp`, 62 px apart across and 57 down, under
//!   `ORDERS\frame1.bmp`.
use crate::{
    Result,
    assets::{GameAssets, Ini, legacy_text},
    bitmap::Bitmap,
    dialogs::{self, Control, Dialog, Kind},
    font::BitmapFont,
    invalid,
    menu::{MenuSkin, Rect, color},
    world::{CLASSES, MapObject, MissionWorld, ObjectLibrary, civ_section, class},
};
use std::collections::{BTreeMap, HashMap};

pub mod cheat_panel;
use cheat_panel::{CheatChoices, CheatControl};

/// Base values of one unit or building class (`DATA\* UNITS.TXT`,
/// `DATA\* BUILDINGS.TXT`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClassStats {
    pub attack: Option<u32>,
    pub defence: Option<u32>,
    pub ranged: bool,
    pub resistance: Option<u32>,
}

/// The source artwork and coordinates are the original LAYOUT800L/R.
pub struct HudSkin {
    left: Bitmap,
    right: Bitmap,
    /// `SCREEN 1024 LEFT/RIGHT`: source of the bands that fill taller and
    /// wider windows.
    left_tall: Bitmap,
    right_tall: Bitmap,
    buttons: BTreeMap<String, Bitmap>,
    icons: Ini,
    icon_images: BTreeMap<String, Bitmap>,
    /// Only indexed atlases have reserved kingdom shades; RGB portraits keep their artwork.
    colored_icons: BTreeMap<String, [Bitmap; 8]>,
    clock: Bitmap,
    pub cursor: Bitmap,
    pub hardware_cursor: Bitmap,
    /// The eight kingdom colours of the original interface (COLOR BARS.BMP).
    pub player_colors: [[u8; 3]; 8],
    /// Bright ramp endpoints of IMAGES/PALETTE.BMP used by the native minimap.
    pub minimap_colors: [[u8; 3]; 8],
    /// `LOCAL\FONTS\Arial10_aa.fnt`, the panel font of the original.
    pub font: BitmapFont,
    /// Arial12B (`0x5ccaec`): the lines of the bubbles.
    pub font12b: BitmapFont,
    /// Arial14B (`0x5ccaf0`): the story's text and its button.
    pub font14b: BitmapFont,
    orders: Ini,
    menus: Ini,
    commands: Bitmap,
    frame1: Bitmap,
    /// `frame1` of a checked button (+0x3c, 0x49b8df): remapped through
    /// colour table 5 ([`tinted`]).
    frame1_checked: Bitmap,
    multiselect: Bitmap,
    /// Previous page, holder, next page: four 25×25 states each.
    arrows: [Bitmap; 3],
    windows: BTreeMap<&'static str, Bitmap>,
    stats: HashMap<(String, String), ClassStats>,
    /// COST column of the unit and building tables (`50f`, `"120g,600m"`).
    costs: HashMap<(String, String), String>,
    tech_trees: HashMap<String, Ini>,
    /// Art of the command dialogs under `IMAGESSCREENS` (`D_GAMBLE/BACK_V`, …).
    dialog_art: BTreeMap<&'static str, Bitmap>,
    /// `LOCALHELP.INI`: the hints of the dialogs' controls.
    pub hints: Ini,
    /// `HELP\ITEMS.TXT`: the help screens' encyclopedia of units,
    /// buildings, researches and items, which the cheat panel quotes.
    help: Ini,
    /// `[misc] inf3` of STRINGS3.INI, shown for inexhaustible mines.
    pub infinite: String,
}

/// Pieces of the command dialogs under `IMAGES\SCREENS` (side layouts).
const DIALOG_ART: [&str; 24] = [
    "D_GAMBLE/BACK_V",
    "D_GAMBLE/AMOUNT",
    "D_GAMBLE/PROP",
    "D_GAMBLE/BIG FRAME",
    "D_GAMBLE/GAMBLE",
    "D_GAMBLE/CANCEL",
    "D_GAMBLE/WINLOSE",
    "D_GAMBLE/ZAROVE",
    "D_TRADE1/BACK_V",
    "D_TRADE1/OBJECT",
    "D_TRADE1/TRADE",
    "D_TRADE1/CANCEL",
    "D_TRADE/BACK_V",
    "D_TRADE/OBJECT",
    "D_TRADE/AMOUNT",
    "D_TRADE/TRADE",
    "D_TRADE/CANCEL",
    "D_BUILD/BACK_V",
    "D_BUILD/BOOKS",
    "D_BUILD/POTIONS",
    "D_BUILD/POWER",
    "D_BUILD/AMOUNT",
    "D_BUILD/BUILD",
    "D_BUILD/CANCEL",
];
/// Window pieces under `IMAGES\SCREENS`.
const WINDOW_ART: [(&str, &str); 52] = [
    ("quests", "D_QUESTS/BACK"),
    ("quests_button", "D_QUESTS/BTN"),
    ("message", "D_MESSAGE/BACK"),
    ("message_button", "D_MESSAGE/BTN"),
    ("diplomacy", "D_DIPLOMACY/BACK"),
    ("diplomacy_row", "D_DIPLOMACY/ROW"),
    ("diplomacy_button", "D_DIPLOMACY/BTN"),
    ("allies", "D_DIPLOMACY/ALLIES"),
    ("neutral", "D_DIPLOMACY/NEUTRAL"),
    ("enemies", "D_DIPLOMACY/ENEMIES"),
    ("race", "D_DIPLOMACY/RACE"),
    ("sh_vision", "D_DIPLOMACY/SH_VISION"),
    ("sh_res", "D_DIPLOMACY/SH_RES"),
    ("cmd_units", "D_DIPLOMACY/CMD_UNITS"),
    ("tribute", "D_ADVANCED/BACK"),
    ("res_gold", "D_ADVANCED/RES_GOLD"),
    ("res_stone", "D_ADVANCED/RES_STONE"),
    ("res_food", "D_ADVANCED/RES_FOOD"),
    ("res_wood", "D_ADVANCED/RES_WOOD"),
    ("chk_ex", "COMMON/CHK EX"),
    ("chk", "COMMON/CHK"),
    ("ai", "COMMON/AI"),
    ("chat_small", "D_CHAT/BACK_SMALL"),
    ("chat_big", "D_CHAT/BACK_BIG"),
    ("chat_row", "D_CHAT/ROW"),
    ("to_all", "COMMON/ALL"),
    ("to_allies", "COMMON/ALLIES"),
    ("to_enemies", "COMMON/ENEMIES"),
    ("to_custom", "COMMON/CUSTOM"),
    ("help_ball", "COMMON/BTN_HELP_S"),
    ("story", "S_SCRPLAY/BACK1"),
    ("story_ok", "S_SCRPLAY/OK1"),
    ("say_back", "D_SAY/BACK"),
    ("say_frame", "D_SAY/FRAME"),
    ("say_eye", "D_SAY/EYE"),
    ("say_speaker", "D_SAY/SPEAKER"),
    ("baloonB_1", "D_SAY/BALOONB_1"),
    ("baloonB_2", "D_SAY/BALOONB_2"),
    ("baloonB_3", "D_SAY/BALOONB_3"),
    ("baloonB_4", "D_SAY/BALOONB_4"),
    ("baloonB_5", "D_SAY/BALOONB_5"),
    ("baloonB_6", "D_SAY/BALOONB_6"),
    ("baloonB_7", "D_SAY/BALOONB_7"),
    ("baloonB_8", "D_SAY/BALOONB_8"),
    ("baloonF_1", "D_SAY/BALOONF_1"),
    ("baloonF_2", "D_SAY/BALOONF_2"),
    ("baloonF_3", "D_SAY/BALOONF_3"),
    ("baloonF_4", "D_SAY/BALOONF_4"),
    ("baloonF_5", "D_SAY/BALOONF_5"),
    ("baloonF_6", "D_SAY/BALOONF_6"),
    ("baloonF_7", "D_SAY/BALOONF_7"),
    ("baloonF_8", "D_SAY/BALOONF_8"),
];

fn parse_table(
    text: &str,
    civ: &str,
    out: &mut HashMap<(String, String), ClassStats>,
    costs: &mut HashMap<(String, String), String>,
) {
    let mut columns: Vec<String> = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
        if matches!(fields.first(), Some(&("UNIT" | "BUILDING"))) {
            columns = fields.iter().map(|f| f.to_ascii_uppercase()).collect();
            continue;
        }
        let at = |name: &str| {
            columns
                .iter()
                .position(|c| c == name)
                .and_then(|i| fields.get(i).copied())
                .filter(|v| !v.is_empty())
        };
        let number = |name: &str| at(name).and_then(|v| v.parse::<u32>().ok());
        let Some(class) = at("CLASS") else { continue };
        if let Some(cost) = at("COST") {
            costs.insert(
                (civ.to_owned(), class.to_ascii_lowercase()),
                cost.to_owned(),
            );
        }
        let stats = out
            .entry((civ.to_owned(), class.to_ascii_lowercase()))
            .or_default();
        if columns.iter().any(|c| c == "ATTACK") {
            stats.attack = number("ATTACK");
        }
        if columns.iter().any(|c| c == "DEFENCE") {
            stats.defence = number("DEFENCE");
        }
        if columns.iter().any(|c| c == "RANGE") {
            stats.ranged = number("RANGE").is_some_and(|r| r > 1);
        }
        if columns.iter().any(|c| c == "RESISTANCE") {
            stats.resistance = number("RESISTANCE");
        }
    }
}

impl HudSkin {
    /// Original global shortcuts include help, which has no command-grid icon.
    pub fn ownership_hotkey(&self, order: &str) -> Option<&str> {
        self.menus.get("ownership", order).map(str::trim)
    }
    pub fn load(a: &GameAssets) -> Result<Self> {
        let art =
            |n: &str| Bitmap::decode(&a.get(&format!("IMAGES/SCREENS/S_VIEW/{n}.BMP"))?, true);
        let mut buttons = BTreeMap::new();
        for n in [
            "BTN MENU LEFT",
            "BTN MENU RIGHT",
            "BTN DIPLOMACY LEFT",
            "BTN DIPLOMACY RIGHT",
            "BTN CHAT LEFT",
            "BTN CHAT RIGHT",
        ] {
            buttons.insert(n.into(), art(n)?);
        }
        let icons = Ini::parse(&a.get("DATA/ICONS.INI")?);
        let mut icon_images = BTreeMap::new();
        let mut colored_icons = BTreeMap::new();
        for n in a
            .names()
            .iter()
            .filter(|n| n.starts_with("IMAGES/ICONS/") && n.ends_with(".BMP"))
        {
            let data = a.get(n)?;
            if Bitmap::is_indexed(&data) {
                let variants: Vec<_> = (0..8)
                    .map(|slot| Bitmap::decode_for(&data, true, Some(slot)))
                    .collect::<Result<_>>()?;
                colored_icons.insert(n.to_ascii_uppercase(), variants.try_into().ok().unwrap());
            }
            icon_images.insert(n.to_ascii_uppercase(), Bitmap::decode(&data, true)?);
        }
        let left = art("SCREEN 800 LEFT")?;
        let right = art("SCREEN 800 RIGHT")?;
        let left_tall = art("SCREEN 1024 LEFT")?;
        let right_tall = art("SCREEN 1024 RIGHT")?;
        if [&left_tall, &right_tall]
            .iter()
            .any(|b| (b.width, b.height) != (1024, 768))
        {
            return Err(invalid("SCREEN 1024 LEFT/RIGHT devono essere 1024×768"));
        }
        let clock = art("DAYNIGHT")?;
        // Layout code indexes these sheets directly; reject unexpected art early.
        if (left.width, left.height) != (800, 600) || (right.width, right.height) != (800, 600) {
            return Err(invalid("SCREEN 800 LEFT/RIGHT devono essere 800×600"));
        }
        if clock.width < 36 || clock.height < 13 * 36 {
            return Err(invalid("DAYNIGHT non contiene le 24 ore originali"));
        }
        // Eight horizontal bars, one per kingdom colour, top to bottom.
        let bars = Bitmap::decode(&a.get("IMAGES/SCREENS/COMMON/COLOR BARS.BMP")?, false)?;
        if bars.height < 8 {
            return Err(invalid("COLOR BARS.BMP non contiene gli otto colori"));
        }
        let player_colors = std::array::from_fn(|i| {
            let y = (bars.height as usize * (2 * i + 1)) / 16;
            let at = (y * bars.width as usize + bars.width as usize / 2) * 4;
            [bars.rgba[at], bars.rgba[at + 1], bars.rgba[at + 2]]
        });
        // 0x43b3ba: palette[112 + 16 * (slot + 1) - 1], converted
        // through the original RGB555 display format, rather than COLOR BARS.
        let palette = a.get("IMAGES/PALETTE.BMP")?;
        Bitmap::decode(&palette, false)?;
        let palette_start = 14 + crate::u32_at(&palette, 14)? as usize;
        if !Bitmap::is_indexed(&palette)
            || palette_start + 256 * 4 > crate::u32_at(&palette, 10)? as usize
        {
            return Err(invalid("PALETTE.BMP deve contenere la palette completa"));
        }
        let minimap_colors = std::array::from_fn(|slot| {
            let at = palette_start + (127 + slot * 16) * 4;
            [palette[at + 2], palette[at + 1], palette[at]].map(|c| {
                let v = c >> 3;
                (v << 3) | (v >> 2)
            })
        });
        let orders_art = |n: &str| Bitmap::decode(&a.get(&format!("IMAGES/ORDERS/{n}.BMP"))?, true);
        // Pure green is the transparent key: the stone slot shows around the icons.
        let commands = Bitmap::decode(&a.get("IMAGES/ORDERS/BUTTONS.BMP")?, true)?;
        if commands.width != 48 || commands.height % 48 != 0 {
            return Err(invalid("ORDERS/BUTTONS.BMP deve avere icone 48×48"));
        }
        let arrows = [
            orders_art("LEFT")?,
            orders_art("HOLDER")?,
            orders_art("RIGHT")?,
        ];
        if arrows.iter().any(|b| b.width != 100 || b.height != 25) {
            return Err(invalid(
                "LEFT/HOLDER/RIGHT devono avere quattro stati 25×25",
            ));
        }
        let mut windows = BTreeMap::new();
        for (key, path) in WINDOW_ART {
            let mut art = Bitmap::decode(&a.get(&format!("IMAGES/SCREENS/{path}.BMP"))?, true)?;
            // The bubble's back and the tail's discs are drawn at half
            // (mode 4 with 50, 0x4bf56d, 0x4c0bb7, 0x461bc2).
            if key == "say_back" || key.starts_with("baloonB") {
                for px in art.rgba.chunks_exact_mut(4).filter(|px| px[3] != 0) {
                    px[3] = 128;
                }
            }
            windows.insert(key, art);
        }
        let mut stats = HashMap::new();
        let mut costs = HashMap::new();
        for civ in ["euro", "asia", "arab", "comm"] {
            for kind in ["BUILDINGS", "UNITS"] {
                let name = format!("DATA/{} {kind}.TXT", civ.to_ascii_uppercase());
                if a.contains(&name) {
                    parse_table(&legacy_text(&a.get(&name)?), civ, &mut stats, &mut costs);
                }
            }
        }
        for path in ["DATA/TECHNOLOGIES.INI", "DATA/UPGRADES.INI"] {
            let table = Ini::parse(&a.get(path)?);
            for name in table.section_names() {
                let raw = [
                    ("gold", 'g'),
                    ("wood", 'w'),
                    ("food", 'f'),
                    ("stone", 's'),
                    ("manna", 'm'),
                ]
                .into_iter()
                .filter_map(|(key, suffix)| {
                    let value = table.get(name, key)?.trim().parse::<u32>().ok()?;
                    (value > 0).then(|| format!("{value}{suffix}"))
                })
                .collect::<Vec<_>>()
                .join(",");
                costs.insert(("comm".into(), name.to_ascii_lowercase()), raw);
            }
        }
        let mut dialog_art = BTreeMap::new();
        for n in DIALOG_ART {
            let path = format!("IMAGES/SCREENS/{n}.BMP");
            if a.contains(&path) {
                dialog_art.insert(n, Bitmap::decode(&a.get(&path)?, true)?);
            }
        }
        let frame1 = orders_art("FRAME1")?;
        Ok(Self {
            dialog_art,
            player_colors,
            minimap_colors,
            left,
            right,
            left_tall,
            right_tall,
            buttons,
            icons,
            icon_images,
            colored_icons,
            clock,
            cursor: Bitmap::decode(&a.get("IMAGES/CURSORS/S_DEF.BMP")?, true)?,
            hardware_cursor: Bitmap::decode(&a.get("IMAGES/CURSORS/H_DEF.BMP")?, true)?,
            font: BitmapFont::decode(&a.get("LOCAL/FONTS/ARIAL10_AA.FNT")?)?,
            font12b: BitmapFont::decode(&a.get("LOCAL/FONTS/ARIAL12B_AA.FNT")?)?,
            font14b: BitmapFont::decode(&a.get("LOCAL/FONTS/ARIAL14B_AA.FNT")?)?,
            orders: Ini::parse(&a.get("DATA/ORDERS.INI")?),
            menus: Ini::parse(&a.get("DATA/MENU.INI")?),
            commands,
            frame1_checked: tinted(&frame1, [0x80, 0, 0]),
            frame1,
            multiselect: Bitmap::decode(&a.get("IMAGES/ICONS/MULTISELECT.BMP")?, true)?,
            arrows,
            windows,
            stats,
            costs,
            tech_trees: ["euro", "arab", "asia"]
                .into_iter()
                .map(|civ| {
                    Ok((
                        civ.into(),
                        Ini::parse(&a.get(&format!("DATA/TECHTREE {civ}.INI"))?),
                    ))
                })
                .collect::<Result<_>>()?,
            hints: Ini::parse(&a.get("LOCAL/HELP.INI")?),
            help: if a.contains("HELP/ITEMS.TXT") {
                Ini::parse(&a.get("HELP/ITEMS.TXT")?)
            } else {
                Ini::default()
            },
            infinite: Ini::locale(a)?.label("misc", "inf3"),
        })
    }
    /// The cost of a class as the hint line shows it: «50 cibo, 20 legna»,
    /// with the resource names of `[viewmsg]`.
    pub fn cost_text(&self, locale: &Ini, civ: &str, class: &str) -> Option<String> {
        self.researched_cost_text(locale, civ, class, &[])
    }
    pub fn researched_cost_text(
        &self,
        locale: &Ini,
        civ: &str,
        class: &str,
        researches: &[String],
    ) -> Option<String> {
        let raw = self
            .costs
            .get(&(civ.to_owned(), class.to_owned()))
            .or_else(|| self.costs.get(&("comm".to_owned(), class.to_owned())))?;
        let mut parts = Vec::new();
        let mut digits = String::new();
        for c in raw.chars() {
            if c.is_ascii_digit() {
                digits.push(c);
            } else if let Some(key) = match c.to_ascii_lowercase() {
                'g' => Some("Gold"),
                'w' => Some("Wood"),
                'f' => Some("Food"),
                's' => Some("Stone"),
                'm' => Some("Manna"),
                _ => None,
            } {
                if !digits.is_empty() {
                    let percent = cost_percent(civ, class, key, researches);
                    let amount = digits.parse::<u64>().ok()?.checked_mul(percent)? / 100;
                    parts.push(format!("{}:{amount}", locale.label("viewmsg", key)));
                    digits.clear();
                }
            }
        }
        (!parts.is_empty()).then(|| parts.join(" "))
    }
    /// Gold, stone, food and wood of a class for the kingdom's researches.
    pub fn researched_cost(&self, civ: &str, class: &str, researches: &[String]) -> [u32; 4] {
        let mut cost = [0; 4];
        let Some(raw) = self
            .costs
            .get(&(civ.to_owned(), class.to_owned()))
            .or_else(|| self.costs.get(&("comm".to_owned(), class.to_owned())))
        else {
            return cost;
        };
        let mut digits = String::new();
        for c in raw.chars() {
            if c.is_ascii_digit() {
                digits.push(c);
                continue;
            }
            let (r, key) = match c.to_ascii_lowercase() {
                'g' => (0, "Gold"),
                's' => (1, "Stone"),
                'f' => (2, "Food"),
                'w' => (3, "Wood"),
                _ => {
                    digits.clear();
                    continue;
                }
            };
            let n = digits.parse::<u64>().unwrap_or(0);
            cost[r] = (n * cost_percent(civ, class, key, researches) / 100) as u32;
            digits.clear();
        }
        cost
    }
    fn icon(&self, key: &str) -> Option<(&Bitmap, Rect)> {
        self.icon_for_color(key, None)
    }
    fn icon_for_color(&self, key: &str, slot: Option<usize>) -> Option<(&Bitmap, Rect)> {
        let n = self.icons.get(key, "image")?;
        let path = format!("IMAGES/{}", n.replace('\\', "/")).to_ascii_uppercase();
        let image = slot
            .and_then(|s| self.colored_icons.get(&path)?.get(s))
            .or_else(|| self.icon_images.get(&path))?;
        let index = self
            .icons
            .get(key, "index")
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        let size = image.width as i32;
        Some((
            image,
            Rect {
                x: 0,
                y: index * size,
                w: size,
                h: size,
            },
        ))
    }
    /// Base statistics of a class for the object's civilization, falling
    /// back to the common tables.
    pub fn class_stats(&self, o: &MapObject) -> Option<ClassStats> {
        let civ = civ_section(o.civilization);
        let class = CLASSES[o.class].to_owned();
        self.stats
            .get(&(civ.to_owned(), class.clone()))
            .or_else(|| self.stats.get(&("comm".to_owned(), class)))
            .copied()
    }
    /// The stone frame stretched to a window. It depends only on the size and
    /// side, so callers cache it instead of rebuilding it for every redraw.
    pub fn background(&self, width: i32, height: i32, left: bool) -> Bitmap {
        let (src, tall) = if left {
            (&self.left, &self.left_tall)
        } else {
            (&self.right, &self.right_tall)
        };
        let mut out = Bitmap {
            width: width as u32,
            height: height as u32,
            rgba: vec![0; (width * height * 4) as usize],
        };
        // Bands of the 1024×768 art that the original stretched over: the
        // carved vines of the top bar and the plain stone under the commands.
        // They are repeated mirrored, so no seam shows.
        let mirror = |k: i32, from: i32, len: i32| {
            let t = k.rem_euclid(2 * len);
            from + if t < len { t } else { 2 * len - 1 - t }
        };
        // The 1024 bottom (carving and border) needs 680 rows; shorter
        // windows keep the 800×600 bottom border.
        let tall_bottom = height >= 680;
        let bottom = if tall_bottom { 120 } else { 40 };
        for y in 0..height {
            for x in 0..width {
                let (art, sx, sy) = if y < 30 {
                    if left {
                        // Bar and resources, then the vines to the right edge.
                        let sx = if x < 1024 {
                            x
                        } else {
                            mirror(x - 1024, 800, 224)
                        };
                        (tall, sx, y)
                    } else if x >= width - 294 {
                        (tall, x - (width - 1024), y)
                    } else if x < 460 {
                        (tall, x, y)
                    } else {
                        (tall, mirror(x - 460, 460, 270), y)
                    }
                } else {
                    let in_panel = if left { x < 218 } else { x >= width - 218 };
                    if !in_panel {
                        continue;
                    }
                    let ax = |art_width: i32| if left { x } else { x - (width - art_width) };
                    if y < 560 {
                        (src, ax(800), y)
                    } else if y >= height - bottom {
                        if tall_bottom {
                            (tall, ax(1024), y - (height - 768))
                        } else {
                            (src, ax(800), y - (height - 600))
                        }
                    } else {
                        (tall, ax(1024), mirror(y - 560, 560, 88))
                    }
                };
                if sx < 0 || sy < 0 || sx >= art.width as i32 || sy >= art.height as i32 {
                    continue;
                }
                let a = ((sy * art.width as i32 + sx) * 4) as usize;
                let b = ((y * width + x) * 4) as usize;
                out.rgba[b..b + 4].copy_from_slice(&art.rgba[a..a + 4]);
            }
        }
        out
    }
}

/// Window size and panel side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: i32,
    pub height: i32,
    pub left: bool,
}
impl Layout {
    /// A point of the 800×600 art in window pixels: the right panel and the
    /// right end of the top bar follow the window's right edge.
    fn at(self, x: i32, y: i32) -> (i32, i32) {
        if !self.left && x >= 480 {
            (x + self.width - 800, y)
        } else {
            (x, y)
        }
    }
    fn rect(self, x: i32, y: i32, w: i32, h: i32) -> Rect {
        let (x, y) = self.at(x, y);
        Rect { x, y, w, h }
    }
    pub fn minimap(self) -> Rect {
        if self.left {
            self.rect(4, 31, 128, 128)
        } else {
            self.rect(668, 31, 128, 128)
        }
    }
    /// The map, between the top bar and the side panel: `view` of
    /// LAYOUT800L (150,30,650,570) and LAYOUT800R (0,30,651,570).
    pub fn play(self) -> Rect {
        Rect {
            x: if self.left { 150 } else { 0 },
            y: 30,
            w: self.width - if self.left { 150 } else { 149 },
            h: self.height - 30,
        }
    }
    /// The map window in a frame (0x4f9b40): beside the panels, over the
    /// whole screen, or across it between the film's two bands.
    pub fn map_window(self, frame: MapFrame) -> Rect {
        match frame {
            MapFrame::Panels => self.play(),
            MapFrame::Whole => Rect {
                x: 0,
                y: 0,
                w: self.width,
                h: self.height,
            },
            MapFrame::Film => {
                let band = film_band(self.height);
                Rect {
                    x: 0,
                    y: band,
                    w: self.width,
                    h: self.height - 2 * band,
                }
            }
        }
    }
    /// The ornament that juts into the map beside the minimap (`mscr1`).
    fn corner(self) -> Rect {
        if self.left {
            self.rect(150, 30, 68, 111)
        } else {
            self.rect(582, 30, 70, 111)
        }
    }
    fn selection(self) -> Rect {
        if self.left {
            self.rect(6, 169, 118, 157)
        } else {
            self.rect(657, 169, 118, 157)
        }
    }
    /// Previous page, holder and next page.
    fn arrows(self) -> [Rect; 3] {
        let xs = if self.left {
            [25, 63, 101]
        } else {
            [675, 712, 750]
        };
        xs.map(|x| self.rect(x, 302, 25, 25))
    }
    /// The command dialogs' rectangle, over the grid (`back_v`, 116×225).
    pub fn dialog(self) -> Rect {
        self.rect(if self.left { 16 } else { 668 }, 332, 116, 225)
    }
    pub fn command(self, i: usize) -> Rect {
        let x0 = if self.left { 16 } else { 668 };
        self.rect(
            x0 + 3 + (i % 2) as i32 * 62,
            332 + 3 + (i / 2) as i32 * 57,
            48,
            48,
        )
    }
    fn menu_button(self) -> Rect {
        if self.left {
            self.rect(4, 0, 128, 25)
        } else {
            self.rect(668, 0, 128, 25)
        }
    }
    fn diplomacy_button(self) -> Rect {
        if self.left {
            self.rect(166, 0, 61, 25)
        } else {
            self.rect(573, 0, 61, 25)
        }
    }
    fn chat_button(self) -> Rect {
        if self.left {
            self.rect(233, 0, 61, 25)
        } else {
            self.rect(506, 0, 61, 25)
        }
    }
    /// Gold, wood, food, stone and population counters.
    fn resources(self) -> [Rect; 5] {
        if self.left {
            [
                self.rect(360, 0, 48, 18),
                self.rect(463, 0, 39, 18),
                self.rect(549, 0, 46, 18),
                self.rect(644, 0, 46, 18),
                self.rect(740, 0, 46, 18),
            ]
        } else {
            [
                self.rect(30, 0, 46, 18),
                self.rect(131, 0, 39, 18),
                self.rect(219, 0, 45, 18),
                self.rect(313, 0, 46, 18),
                self.rect(408, 0, 46, 18),
            ]
        }
    }
    fn clock(self) -> Rect {
        if self.left {
            self.rect(142, 31, 36, 36)
        } else {
            self.rect(622, 32, 36, 36)
        }
    }
    /// An in-game window, centred over the map.
    pub fn window(self, panel: Panel) -> Rect {
        // The story and the bubbles take the whole screen (0x4c35f9).
        if matches!(panel, Panel::Narration | Panel::Say) {
            return Rect {
                x: 0,
                y: 0,
                w: self.width,
                h: self.height,
            };
        }
        let (w, h) = match panel {
            Panel::Quests => (500, 368),
            Panel::Diplomacy => (500, 420),
            Panel::Tribute | Panel::Cheats => (600, 380),
            Panel::Story => (500, 320),
            Panel::Chat(false) => (500, 143),
            Panel::Chat(true) => (500, 320),
            Panel::Narration | Panel::Say => (self.width, self.height),
        };
        // The original centres its dialogs on the whole screen.
        Rect {
            x: (self.width - w) / 2,
            y: ((self.height - h) / 2).max(0),
            w,
            h,
        }
    }
}
/// Where the map window lies (0x4f9b40, `+0x82` of the view).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MapFrame {
    /// Beside the panels.
    #[default]
    Panels,
    /// Over the whole screen, the panels hidden: F12 (0x4f61ce).
    Whole,
    /// Between two black bands: LockView, ScrollView and conversations
    /// (0x4f9990).
    Film,
}
pub fn play_rect(width: i32, height: i32, left: bool, frame: MapFrame) -> Rect {
    Layout {
        width,
        height,
        left,
    }
    .map_window(frame)
}
pub fn map_rect(width: i32, height: i32, left: bool) -> Rect {
    Layout {
        width,
        height,
        left,
    }
    .minimap()
}

/// The lists of the ownership menu, shown in the command grid (`GET heroes`,
/// `GET spys`, `GET items` with their submenus HEROES, SPYS and ITEMS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ListKind {
    Heroes,
    Spies,
    Items,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Panel {
    /// `OBIETTIVI`: the objectives, two to a page (`D_QUESTS`).
    Quests,
    /// The story told when the mission opens (`D_MESSAGE`).
    Story,
    /// `ATTEGGIAMENTO DIPLOMATICO` (`D_DIPLOMACY`).
    Diplomacy,
    /// `INVIA RISORSE` (`D_ADVANCED`, 0x447270): the tributes, a window
    /// over the diplomacy one, to which it goes back.
    Tribute,
    /// `CONVERSAZIONE`: a message to other kingdoms (`D_CHAT`); `true` for
    /// the large window with the list of kingdoms (custom recipients).
    Chat(bool),
    /// The story screen of the scripts (`S_SCRPLAY`, 0x4c34c0): a book whose
    /// right page the text goes up.
    Narration,
    /// The bubbles of a conversation (`D_SAY`, 0x4bf3c0).
    Say,
    /// `TRUCCHI`, this version's panel of the original's developer
    /// commands (`gameplay::cheats`, [`cheat_panel`]), on the tribute
    /// window's art.
    Cheats,
}
impl Panel {
    /// The windows with the small help ball at their top-left corner
    /// (0x446545, 0x447817, 0x4237fe). Their controls' hints show only in
    /// its mode: the mission's hint line takes those of its own controls
    /// alone (0x45ca40 forwards ids 600000–600999).
    pub fn has_help(self) -> bool {
        matches!(self, Panel::Diplomacy | Panel::Tribute | Panel::Chat(_))
    }
    /// The windows of the draggable class 0x47eec0: diplomacy (0x44597f),
    /// tributes (0x4472be), objectives (0x4af0c0) and the message window
    /// (0x47d979); the cheat panel, on the tributes' art, with them. The
    /// chat window (0x422f00) is not one of them.
    pub fn draggable(self) -> bool {
        matches!(
            self,
            Panel::Diplomacy | Panel::Tribute | Panel::Quests | Panel::Story | Panel::Cheats
        )
    }
}
/// The ball's size: `BTN_HELP_S.BMP` holds 2×2 frames of 50×43 (0x45c833).
const HELP_BALL: [i32; 2] = [50, 43];
/// Who a chat message goes to: the four buttons of `D_CHAT`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Recipients {
    #[default]
    All,
    Allies,
    Enemies,
    /// The kingdoms ticked in the large window.
    Custom,
}
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChatState {
    pub text: String,
    pub to: Recipients,
    /// Kingdoms 1–8 chosen with [`Recipients::Custom`].
    pub custom: [bool; 8],
}
/// Diplomacy words of `.diplomacy`: attitude bits and sharing bits.
pub mod diplomacy {
    pub const ALLY: u32 = 0x40;
    pub const NEUTRAL: u32 = 0x20;
    pub const ENEMY: u32 = 0x10;
    pub const ATTITUDE: u32 = 0x70;
    pub const VISION: u32 = 0x1;
    pub const RESOURCES: u32 = 0x2;
    pub const UNITS: u32 = 0x4;
    pub const VICTORY: u32 = 0x8;
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HudAction {
    Holder,
    InspectContent(usize),
    BackMenu,
    Pause,
    Panel(Panel),
    Close,
    Fit,
    /// An object of a list in the grid (0x49b510): the pointer shows it,
    /// a click adds it, or the unit holding it, to the selection.
    Listed(usize),
    /// A phrase offered in a conversation, by position.
    Answer(usize),
    Scroll(i32),
    /// A command button of the current page, by its position in [`commands`].
    Command(usize),
    /// Previous (-1) or next (+1) page of commands or of a window.
    Page(i32),
    Chat,
    /// Diplomacy window: attitude toward kingdom `.0` (an attitude bit), a
    /// sharing bit toggled, shared victory, accept.
    Relation(u8, u32),
    Share(u8, u32),
    SharedVictory,
    Accept,
    /// Diplomacy window: the tribute window. Tribute window: the amount of
    /// resource `.1` for kingdom `.0` (left click adds, right click takes
    /// away), and Send.
    Tribute,
    Amount(u8, u8),
    Send,
    /// Chat window: recipients, and one kingdom of the custom list.
    Recipients(Recipients),
    ChatTo(u8),
    /// A grey control of the original: it only shows its hint.
    Inert,
    /// A control of the command dialog.
    Dialog(Control),
    /// The eye of a conversation's bubble: the view on its speaker
    /// (0x438030 with 2 → 0x4383c0).
    SayEye(usize),
    /// The speaker of a bubble: the conversations' voices on or off
    /// (0x58efc0).
    SayVoices,
    /// A control of the cheat panel.
    Cheat(CheatControl),
    /// The button of the command grid that opens the cheat panel, while
    /// nothing is selected and the game's options switch it on.
    OpenCheats,
    /// The small help ball of the diplomacy, tribute and chat windows
    /// (0x45c800 with 0): its mode shows their controls' hints.
    Help,
}
pub struct HudButton {
    pub rect: Rect,
    pub label: String,
    pub action: HudAction,
    pub active: bool,
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct HudState {
    pub selection_event: u64,
    pub command_menu: Option<String>,
    pub command_history: Vec<Option<String>>,
    pub contents_owner: Option<usize>,
    pub inspected_content: Option<usize>,
    pub selected: Vec<usize>,
    /// A tree taken by a click, alone and in the place of the selection:
    /// its cell and the wood shown, ten for each load left, of 100 (the
    /// `treeselection` object, 0x453440, 0x437bc0).
    #[serde(default)]
    pub tree: Option<(usize, u32)>,
    /// Object inspected with Tab, while the complete group remains selected.
    pub inspected: Option<usize>,
    #[serde(default, deserialize_with = "panel_or_none")]
    pub panel: Option<Panel>,
    pub scroll: usize,
    pub scroll_max: usize,
    #[serde(skip)]
    pub hover: Option<usize>,
    #[serde(skip)]
    pub press: Option<HudAction>,
    /// The help ball's mode (0x45c960 → 0x45c340), on with the pointer
    /// it last saw: the hint of the window's control under it shows in a
    /// box beneath it (0x45c600).
    #[serde(skip)]
    pub help: Option<[i32; 2]>,
    /// The open windows the player dragged, with their top-left corner
    /// (0x47ef80); a window opens centred again.
    #[serde(skip)]
    pub moved: Vec<(Panel, [i32; 2])>,
    /// The point of the window held while it is dragged (+0x34, +0x36).
    #[serde(skip)]
    pub grab: Option<[i32; 2]>,
    /// Page of the command grid (eight commands a page).
    pub page: usize,
    /// Diplomacy accepted during the mission, and the window's unsaved
    /// choices; both replace the scenario's `.diplomacy` when present.
    pub diplomacy: Option<Vec<u32>>,
    pub diplomacy_draft: Option<Vec<u32>>,
    /// The tribute window's amounts, by kingdom (1–8 at 0–7) and resource
    /// (+0x4e, cleared when it opens).
    #[serde(skip)]
    pub tribute: [[u32; 4]; 8],
    pub chat: ChatState,
    /// The command dialog in the place of the grid.
    #[serde(skip)]
    pub dialog: Option<Dialog>,
    /// Each tavern's last exchange (attributes 0x2714, 0x2715, 0x2720).
    #[serde(skip)]
    pub exchanges: BTreeMap<usize, (u8, u8, u32)>,
    /// The workshop's category and item, kept between openings.
    #[serde(skip)]
    pub workshop: (u8, u8),
    /// F4 (0xc99da0): 1 the scoreboard and the clock, 2 the clock only;
    /// back to 0 when a mission opens.
    #[serde(skip)]
    pub score: u8,
    /// The film framing of LockView, ScrollView and conversations
    /// (0x4f9990): the panels are hidden and the map fills the screen
    /// between two black bands (0x4f9b40).
    #[serde(skip)]
    pub cinematic: bool,
    /// F12 (0x4f61ce): the map over the whole screen, the panels hidden;
    /// a film's end brings them back (0x4f9ade).
    #[serde(skip)]
    pub full_map: bool,
    /// The story screen's crawl (0x4c3d3d): the milliseconds gone at its
    /// last move and the story's time; none for a story without a time.
    #[serde(skip)]
    pub crawl: Option<[u64; 2]>,
    /// The bubbles of the conversation on screen (0x4380f0).
    #[serde(skip)]
    pub say: Vec<Bubble>,
    /// The conversations' voices switched off with a bubble's speaker
    /// (0x58efc0).
    #[serde(skip)]
    pub voices_off: bool,
    /// The cheat panel's button, switched on in the options of this game
    /// and kept by its save (an addition of this version).
    #[serde(default)]
    pub cheats: bool,
    #[serde(skip)]
    pub cheat: CheatChoices,
}
/// A window saved by an older version that no longer has it opens as none.
fn panel_or_none<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Panel>, D::Error> {
    use serde::Deserialize;
    Ok(Option::<serde_json::Value>::deserialize(d)?.and_then(|v| serde_json::from_value(v).ok()))
}
/// One bubble of a conversation (0x438480): one speaker's lines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bubble {
    /// Its place among the six of the screen (0x58efa8, 0x4bf9a0).
    pub slot: usize,
    /// Its top left corner: its place's, then where the right button drags
    /// it ([`say_drag_to`]).
    pub at: [i32; 2],
    /// Its lines, each with the answer it gives (the choice's position).
    pub rows: Vec<(usize, String)>,
    /// The speaker's portrait, a picture of the map (vt+0x10c).
    pub portrait: String,
    /// The middle of the speaker's picture, in interface pixels, while it
    /// is on screen (0x4f8d40).
    pub speaker: Option<[i32; 2]>,
    /// The speaker, or the holder it is in (+0x7a): where the eye takes
    /// the view.
    pub object: Option<usize>,
    /// One of its lines has a voice: the speaker button shows (0x4bfee7).
    pub voiced: bool,
    /// One line being said: a click on the bubble goes on (0x4c088f).
    pub single: bool,
}
/// The six places of the bubbles on the screen (0x58efa8).
pub const SAY_SLOTS: [[i32; 2]; 6] = [
    [70, 90],
    [70, 230],
    [70, 370],
    [446, 90],
    [446, 230],
    [446, 370],
];
/// Size of a bubble (`D_SAY\back.bmp`).
pub const SAY_SIZE: [i32; 2] = [300, 140];
/// 0x4bf9a0: the free place nearest to the speaker that does not cover
/// it, distances between the middles truncated; without the speaker on
/// screen, or with none such, the first free place.
pub fn say_slot(used: &[bool; 6], speaker: Option<Rect>) -> Option<usize> {
    let free = || (0..6).find(|&s| !used[s]);
    let Some(p) = speaker.filter(|p| p.w > 0) else {
        return free();
    };
    let middle = |r: Rect| [r.x + (r.w - 1) / 2, r.y + (r.h - 1) / 2];
    let [px, py] = middle(p);
    let mut best: Option<(u32, usize)> = None;
    for s in (0..6).filter(|&s| !used[s]) {
        let [x, y] = SAY_SLOTS[s];
        let r = Rect {
            x,
            y,
            w: SAY_SIZE[0],
            h: SAY_SIZE[1],
        };
        let [sx, sy] = middle(r);
        let d = (((px - sx).pow(2) + (py - sy).pow(2)) as f64).sqrt() as u32;
        let apart = p.x > r.x + r.w - 1
            || p.x + p.w - 1 < r.x
            || p.y > r.y + r.h - 1
            || p.y + p.h - 1 < r.y;
        if apart && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, s));
        }
    }
    best.map(|(_, s)| s).or_else(free)
}
/// Corner of the story's 800×600 book, in the middle of the screen
/// (0x4c35a3).
pub fn story_origin(width: i32, height: i32) -> [i32; 2] {
    [(width - 800) / 2, (height - 600) / 2]
}
/// The rectangle of a bubble.
pub fn say_rect(b: &Bubble) -> Rect {
    Rect {
        x: b.at[0],
        y: b.at[1],
        w: SAY_SIZE[0],
        h: SAY_SIZE[1],
    }
}
/// Where a bubble dragged with the right button goes (0x4c0949): the
/// pointer less where it took the bubble (0x4c08bc), at or right of the
/// screen's left edge and with its right edge left of the screen's last
/// column, and the same downward.
pub fn say_drag_to(at: [i32; 2], screen: [i32; 2]) -> [i32; 2] {
    let keep = |v: i32, size: i32, last: i32| {
        let v = v.max(0);
        if v + size > last { last - size } else { v }
    };
    [
        keep(at[0], SAY_SIZE[0], screen[0] - 1),
        keep(at[1], SAY_SIZE[1], screen[1] - 1),
    ]
}
/// The lines of a bubble (0x4bfce5): text boxes of Arial12B with one pixel
/// between lines (+0x280), 10 below the top and 10 from the left, or 20 past
/// the portrait, 25 short of the right side, wrapped 5 short of their width
/// (0x47e607), each as tall as its text (flag 0x400) and the next 8 under
/// its last pixel.
pub fn say_rows(font: &BitmapFont, b: &Bubble) -> Vec<(usize, Rect, Vec<String>)> {
    let [bx, by] = b.at;
    let left = if b.portrait.is_empty() { 10 } else { 96 + 20 };
    let w = 274 - left;
    let step = font.height as i32 + 1;
    let mut y = by + 10;
    b.rows
        .iter()
        .map(|(answer, text)| {
            let lines = font.wrap(text, (w - 5).max(1) as usize);
            let h = lines.len().max(1) as i32 * step;
            let r = Rect {
                x: bx + left,
                y,
                w,
                h,
            };
            y += h + 7;
            (*answer, r, lines)
        })
        .collect()
}
/// The eye and the speaker of a bubble, 20×20 at its right corners, 10 in
/// (0x4bfe29, 0x4bfee7).
fn say_buttons(b: &Bubble) -> (Rect, Rect) {
    let [bx, by] = b.at;
    let x = bx + SAY_SIZE[0] - 1 - 20 - 10;
    (
        Rect {
            x,
            y: by + 10,
            w: 20,
            h: 20,
        },
        Rect {
            x,
            y: by + SAY_SIZE[1] - 1 - 20 - 10,
            w: 20,
            h: 20,
        },
    )
}
impl HudState {
    pub fn selection_changed(&mut self) {
        self.selection_event = self.selection_event.wrapping_add(1);
        self.command_menu = None;
        self.command_history.clear();
        self.contents_owner = None;
        self.inspected_content = None;
        self.inspected = None;
        self.page = 0;
        self.close_dialog();
    }
    /// Closes the command dialog, keeping the workshop's choice.
    pub fn close_dialog(&mut self) {
        if let Some(d) = self.dialog.take()
            && d.kind == Kind::Workshop
        {
            self.workshop = (d.option, d.item);
        }
    }
    pub fn inspect_next(&mut self) {
        if self.selected.len() > 1 {
            let next = self
                .inspected
                .and_then(|i| self.selected.iter().position(|&v| v == i))
                .map_or(0, |i| (i + 1) % self.selected.len());
            self.inspected = Some(self.selected[next]);
        }
    }
    /// Where the map window lies.
    pub fn frame(&self) -> MapFrame {
        if self.cinematic {
            MapFrame::Film
        } else if self.full_map {
            MapFrame::Whole
        } else {
            MapFrame::Panels
        }
    }
    pub fn inspected_object(&self) -> Option<usize> {
        if (self.contents_owner.is_some() || listing(self).is_some())
            && self.inspected_content.is_some()
        {
            return self.inspected_content;
        }
        if self.selected.len() == 1 {
            self.selected.first().copied()
        } else {
            self.inspected.filter(|i| self.selected.contains(i))
        }
    }
    /// Switching or closing a window discards choices that were not accepted;
    /// the diplomacy window keeps its own under the tribute window, which
    /// opens with no amounts (0x447416).
    pub fn set_panel(&mut self, panel: Option<Panel>) {
        if self.panel != panel {
            let over = matches!(
                (self.panel, panel),
                (Some(Panel::Diplomacy), Some(Panel::Tribute))
                    | (Some(Panel::Tribute), Some(Panel::Diplomacy))
            );
            if !over {
                self.diplomacy_draft = None;
            }
            if panel == Some(Panel::Tribute) {
                self.tribute = Default::default();
            }
            self.scroll = 0;
            self.hover = None;
            self.press = None;
            self.help = None;
            self.grab = None;
            // The diplomacy window stays where it was under the tributes.
            self.moved.retain(|&(p, _)| over && p == Panel::Diplomacy);
        }
        self.panel = panel;
    }
    /// Where an open window lies: centred as it opened (`Layout::window`),
    /// or where the player dragged it.
    pub fn window(&self, layout: Layout, panel: Panel) -> Rect {
        let w = layout.window(panel);
        match self.moved.iter().find(|&&(p, _)| p == panel) {
            Some(&(_, [x, y])) => Rect { x, y, ..w },
            None => w,
        }
    }
    /// The window on top dragged so that its top-left corner goes to `at`,
    /// kept with 50 pixels on the screen (0x47f02b); whether it moved.
    pub fn drag_window(&mut self, layout: Layout, at: [i32; 2]) -> bool {
        let Some(panel) = self.panel.filter(|p| p.draggable()) else {
            return false;
        };
        let w = self.window(layout, panel);
        let place = |v: i32, size: i32, screen: i32| {
            let v = if v < 50 - size { 50 - size } else { v };
            if v > screen - 50 { screen - 50 } else { v }
        };
        let to = [
            place(at[0], w.w, layout.width),
            place(at[1], w.h, layout.height),
        ];
        if [w.x, w.y] == to {
            return false;
        }
        self.moved.retain(|&(p, _)| p != panel);
        self.moved.push((panel, to));
        true
    }
    /// Closes the window on top: the tribute window goes back to the
    /// diplomacy one (Cancel and Esc, 0x447ba8; Send, 0x447c85).
    pub fn close_panel(&mut self) {
        let back = (self.panel == Some(Panel::Tribute)).then_some(Panel::Diplomacy);
        self.set_panel(back);
    }
    /// A click on an amount of the tribute window (0x447a90): `delta` is
    /// ±100, ±500 with Shift. Then 0x447970: the amount stays at 0 or more,
    /// and when the resource's total over the rows exceeds the player's
    /// stock the clicked row gives the excess back.
    pub fn tribute_click(&mut self, mission: &MissionWorld, owner: u8, res: usize, delta: i64) {
        let Some(row) = owner.checked_sub(1).map(usize::from).filter(|&r| r < 8) else {
            return;
        };
        if res >= 4 {
            return;
        }
        let stock = crate::gameplay::stock(mission, mission.player())[res] as i64;
        let value = self.tribute[row][res] as i64 + delta;
        let others: i64 = (0..8)
            .filter(|&r| r != row)
            .map(|r| self.tribute[r][res] as i64)
            .sum();
        let total = others + value;
        let mut value = value.max(0);
        if total > stock {
            value += stock - total;
        }
        self.tribute[row][res] = value.max(0) as u32;
    }
    /// What Send gives (0x447bd1): for each other kingdom and resource, the
    /// amount when it is not 0 and the player's stock covers it.
    pub fn tribute_sends(&self, mission: &MissionWorld) -> Vec<(u8, u8, u32)> {
        let stock = crate::gameplay::stock(mission, mission.player());
        let amounts = self.tribute;
        other_kingdoms(mission)
            .into_iter()
            .flat_map(|owner| {
                (0..4u8).filter_map(move |res| {
                    let a = amounts[owner as usize - 1][res as usize];
                    (a != 0 && a <= stock[res as usize]).then_some((owner, res, a))
                })
            })
            .collect()
    }
    /// The diplomacy shown: the window's choices, else the accepted ones,
    /// else the scenario's.
    pub fn diplomacy<'a>(&'a self, mission: &'a MissionWorld) -> &'a [u32] {
        self.diplomacy_draft
            .as_deref()
            .or(self.diplomacy.as_deref())
            .unwrap_or(&mission.diplomacy)
    }
}
/// Word of kingdom `from` toward kingdom `to` (both 1–8).
pub fn diplomacy_word(d: &[u32], from: u8, to: u8) -> u32 {
    match (from.checked_sub(1), to.checked_sub(1)) {
        (Some(a), Some(b)) if a < 8 && b < 8 => {
            d.get(a as usize * 8 + b as usize).copied().unwrap_or(0)
        }
        _ => 0,
    }
}
/// The other kingdoms in the diplomacy and chat windows, at most seven.
pub fn other_kingdoms(mission: &MissionWorld) -> Vec<u8> {
    let player = mission.player();
    mission
        .active_kingdoms()
        .map(|(owner, _)| owner)
        .filter(|&owner| owner != player)
        .take(7)
        .collect()
}

/// The cuts of the kingdom's researches on a class's cost, in percent of
/// the resource (as `gameplay::Catalog::stats` applies them).
fn cost_percent(civ: &str, class: &str, key: &str, researches: &[String]) -> u64 {
    let has = |name: &str| researches.iter().any(|r| r.eq_ignore_ascii_case(name));
    if class == "peon" && key == "Food" && (has("te_feudalism") || has("te_slavery")) {
        80
    } else if (matches!(class, "horse_archer" | "heavy_cavalry") && has("te_horsemanship"))
        || (class == "monk" && civ == "asia" && has("te_meditation"))
    {
        75
    } else {
        100
    }
}
/// One button of the command grid.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    /// Initial technologies and required buildings are checked without executing orders.
    pub unavailable: Option<String>,
    /// The order key of `DATA\ORDERS.INI` (`move`, `view_quests`, …).
    pub order: String,
    /// Frame of `ORDERS\buttons.bmp`.
    pub icon: usize,
    pub label: String,
    pub hotkey: String,
    /// What it costs, for the commands that build or train (`CLASSNAME`).
    pub cost: Option<String>,
    pub submenu: Option<String>,
}
const COMMANDS_PER_PAGE: usize = 8;
/// The cheat panel's button: the open book of `ORDERS\BUTTONS.BMP`.
const CHEATS_ICON: i32 = 140;

pub struct QueuedProduction {
    pub producer: usize,
    pub slot: usize,
    pub icon: usize,
    pub order: String,
    pub label: String,
    pub progress: Option<u32>,
}

/// Preserve the order and owner of each job, also for building groups.
pub fn production_queue(
    skin: &HudSkin,
    locale: &Ini,
    state: &HudState,
    mission: &MissionWorld,
    library: &ObjectLibrary,
) -> Vec<QueuedProduction> {
    let Some(sim) = &mission.simulation else {
        return vec![];
    };
    let mut out = Vec::new();
    for &i in &state.selected {
        let Some(o) = mission
            .objects
            .get(i)
            .filter(|o| o.owner == mission.player())
        else {
            continue;
        };
        // A wall segment shows its row leader's queue (0x4fa640).
        let i = crate::gameplay::producer(sim, i);
        let Some(actor) = sim.gameplay.actors.get(i) else {
            continue;
        };
        for (slot, job) in actor.jobs.iter().enumerate() {
            let (name, label) = match &job.kind {
                crate::gameplay::JobKind::Train(class) => {
                    let unit = MapObject {
                        class: *class,
                        ..o.clone()
                    };
                    (unit.class_name().to_owned(), library.class_label(&unit))
                }
                crate::gameplay::JobKind::Research(name)
                | crate::gameplay::JobKind::Upgrade(name, _) => {
                    (name.clone(), locale.label("orders_names", name))
                }
                crate::gameplay::JobKind::Item(..) => (
                    "build_item".to_owned(),
                    locale.label("orders_names", "build_item"),
                ),
            };
            let key = skin
                .menus
                .entries(&library.property(o, "menu").unwrap_or_default())
                .iter()
                .find(|(key, _)| {
                    skin.orders
                        .get(key, "classname")
                        .is_some_and(|s| s.eq_ignore_ascii_case(&name))
                })
                .map(|(key, _)| key.as_str())
                .unwrap_or(&name);
            let icon = skin
                .orders
                .get(key, "img")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let progress =
                (slot == 0).then(|| ((job.total - job.remaining) * 100 / job.total.max(1)) as u32);
            let label = format!(
                "{} · {}{}\n{}",
                library.class_label(o),
                label,
                progress.map_or(String::new(), |p| format!(
                    ": {p}%{}",
                    actor
                        .production_blocked
                        .as_ref()
                        .map_or(String::new(), |s| format!(" · {s}"))
                )),
                crate::lang::t("Annulla e restituisci il costo")
            );
            out.push(QueuedProduction {
                producer: i,
                slot,
                icon,
                order: key.to_owned(),
                label,
                progress,
            });
        }
    }
    out
}

/// The button picture of an item in the market's lists and what its offer
/// names (0x479ef6, 0x462b5a): a book its spell (0x115 + spell, `[spell_names]`);
/// equipment the shield 0x101, the bow 0xff or the sword 0x102 with defence
/// or attack; a potion on sale 0x103, on offer 0x10d health, 0x10e
/// experience, 0x10f mana, 0x111 defence or 0x110 attack.
fn item_button(locale: &Ini, o: &MapObject, selling: bool) -> (usize, String) {
    let misc = |key: &str| locale.label("misc", key);
    match o.class {
        crate::world::class::ITEM_MAGIC => (
            0x115 + o.byte(0x60) as usize,
            crate::world::spell_key(o.byte(0x60))
                .map(|k| locale.label("spell_names", k))
                .unwrap_or_default(),
        ),
        crate::world::class::ITEM_PLUSPLUS => {
            if o.word(0x18) != 0 {
                (0x101, misc("defence3"))
            } else if o.word(0x16) & 0xff00 != 0 {
                (0xff, misc("attack3"))
            } else {
                (0x102, misc("attack3"))
            }
        }
        _ if selling => (0x103, String::new()),
        _ => {
            let (icon, key) = if o.word(0x12) != 0 {
                (0x10d, "health3")
            } else if o.word(0x1c) != 0 {
                (0x10e, "exp3")
            } else if o.word(0x24) != 0 {
                (0x10f, "manna3")
            } else if o.word(0x18) != 0 {
                (0x111, "defence3")
            } else {
                (0x110, "attack3")
            };
            (icon, misc(key))
        }
    }
}
/// A price as the hint line shows it, resources of `[viewmsg]`.
fn price_text(locale: &Ini, price: [u32; 4]) -> String {
    ["Gold", "Stone", "Food", "Wood"]
        .iter()
        .zip(price)
        .filter(|&(_, n)| n > 0)
        .map(|(key, n)| format!("{}:{n}", locale.label("viewmsg", key)))
        .collect::<Vec<_>>()
        .join(" ")
}
fn queued_count(state: &HudState, mission: &MissionWorld, skin: &HudSkin, order: &str) -> usize {
    let Some(sim) = &mission.simulation else {
        return 0;
    };
    // A tavern offer shows its number of units (+0x14 of the menu entry).
    if let Some(id) = order.strip_prefix("hire:").and_then(|s| s.parse().ok()) {
        return crate::gameplay::hire_offer(sim, id).map_or(0, |(_, _, h)| h.count as usize);
    }
    // Query 0x2713: the units of the button's kind queued in the selected
    // building (0x49ed42), or in all of several selected ones.
    let name = skin.orders.get(order, "classname").unwrap_or(order);
    state
        .selected
        .iter()
        .filter_map(|&i| sim.gameplay.actors.get(crate::gameplay::producer(sim, i)))
        .map(|a| {
            a.jobs
                .iter()
                .filter(|j| {
                    matches!(&j.kind, crate::gameplay::JobKind::Train(c)
                        if crate::world::CLASSES[*c].eq_ignore_ascii_case(name))
                })
                .count()
        })
        .sum()
}

/// The commands the original lists for the selection: the class menu of
/// the player's own object (`menu` key, `DATA\MENU.INI`), the `ownership`
/// menu when nothing is selected, nothing for anyone else's objects.
pub fn commands(
    skin: &HudSkin,
    locale: &Ini,
    state: &HudState,
    mission: &MissionWorld,
    library: &ObjectLibrary,
) -> Vec<Command> {
    // 0x4a1180: the inventory takes the place of the commands.
    if state.contents_owner.is_some() {
        return vec![];
    }
    // Civilization of the costs: the selected object's, else the player's.
    let civ_code = state
        .selected
        .first()
        .map(|&i| mission.objects[i].civilization)
        .or_else(|| {
            let player = mission.player();
            mission
                .objects
                .iter()
                .find(|o| o.owner == player && o.civilization > 0)
                .map(|o| o.civilization)
        })
        .unwrap_or(1);
    let civ = civ_section(civ_code);
    let menu = match state.selected.first().map(|&i| &mission.objects[i]) {
        None => Some("ownership".to_owned()),
        // 0x506c30: the objects the player commands, a unit hiding the
        // player's spy included (vt+0xbc).
        Some(o) if mission.commands_at(state.selected[0]) => {
            if o.class == crate::world::class::PEON && o.mole != 0 && mission.controls(o.mole) {
                // 0x4aede0: a peasant hiding the player's spy shows the
                // spy's peasant menu of its own civilization.
                Some(
                    match o.civilization {
                        2 => "r_peon_spy",
                        3 => "a_peon_spy",
                        _ => "e_peon_spy",
                    }
                    .into(),
                )
            } else if o.class == 13
                && mission.simulation.as_ref().is_some_and(|sim| {
                    sim.gameplay
                        .actors
                        .get(state.selected[0])
                        .is_some_and(|a| a.effect_until > sim.ticks)
                })
            {
                Some("ninja_cloaked".into())
            } else {
                crate::gameplay::object_menu(
                    mission.simulation.as_ref(),
                    library,
                    o,
                    state.selected[0],
                )
            }
        }
        Some(_) => None,
    };
    let Some(menu) = menu.filter(|m| !m.is_empty()) else {
        return Vec::new();
    };
    let busy_menu = format!("{menu}_b");
    let busy = state.command_menu.is_none()
        && !state.selected.is_empty()
        && state.selected.iter().all(|&i| {
            mission
                .simulation
                .as_ref()
                .and_then(|s| s.gameplay.actors.get(crate::gameplay::producer(s, i)))
                .is_some_and(|a| !a.jobs.is_empty())
        });
    let menu = state.command_menu.as_deref().unwrap_or(
        if busy
            && skin
                .menus
                .section_names()
                .any(|s| s.eq_ignore_ascii_case(&busy_menu))
        {
            &busy_menu
        } else {
            &menu
        },
    );
    // A tavern's submenus list its offers (0x4d2030, 0x4948c0): «Contratta
    // %s» (`hire3`), the picture of the barracks button and the price
    // (vt+0xf8).
    if let Some(list) = match menu {
        "tavern_merc" => Some(0),
        "tavern_hero" => Some(1),
        _ => None,
    } && let Some(&i) = state.selected.first()
        && let Some(actor) = mission
            .simulation
            .as_ref()
            .and_then(|s| s.gameplay.actors.get(i))
    {
        let o = &mission.objects[i];
        let researches = mission
            .rules
            .get(o.owner.saturating_sub(1) as usize)
            .map_or(&[][..], |r| r.initial_researches.as_slice());
        return actor.hires[list]
            .iter()
            .map(|h| {
                let unit = MapObject {
                    class: h.class,
                    civilization: h.civilization,
                    ..o.clone()
                };
                let base =
                    skin.researched_cost(civ_section(h.civilization), CLASSES[h.class], researches);
                let price = crate::gameplay::hire_price(base, h.count, h.level);
                Command {
                    unavailable: None,
                    order: format!("hire:{}", h.id),
                    icon: skin
                        .orders
                        .get(&h.order, "img")
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0),
                    label: locale
                        .label("misc", "hire3")
                        .replace("%s", &library.class_label(&unit)),
                    hotkey: String::new(),
                    cost: Some(price_text(locale, price)),
                    submenu: None,
                }
            })
            .collect();
    }
    // The Asian market's submenus: the kingdom's items on offer
    // (`market_items`, «Compra %s - %s», four times the value) and the items
    // its units hold (`market_sell`, «Vendi questo oggetto», 0x462a77).
    if matches!(menu, "market_items" | "market_sell")
        && let Some(&i) = state.selected.first()
        && let Some(sim) = mission.simulation.as_ref()
    {
        let o = &mission.objects[i];
        let gold = |k: usize| skin.researched_cost("euro", CLASSES[k], &[])[0];
        let gold_name = locale.label("viewmsg", "Gold");
        if menu == "market_items" {
            let offers = sim
                .gameplay
                .jewels
                .get(&o.owner)
                .map_or(&[][..], |j| j.offers.as_slice());
            return offers
                .iter()
                .map(|jewel| {
                    let item = crate::gameplay::jewel_object(o, jewel);
                    let (icon, property) = item_button(locale, &item, false);
                    let value = crate::gameplay::item_value(&item, gold);
                    Command {
                        unavailable: None,
                        order: format!("buy:{}", jewel.id),
                        icon,
                        label: locale
                            .label("misc", "buy3")
                            .replacen("%s", &library.class_label(&item), 1)
                            .replacen("%s", &property, 1),
                        hotkey: String::new(),
                        cost: Some(format!("{gold_name}:{}", 4 * value)),
                        submenu: None,
                    }
                })
                .collect();
        }
        return crate::gameplay::sellable_items(sim, mission, o.owner)
            .into_iter()
            .map(|j| {
                let item = &mission.objects[j];
                let (icon, _) = item_button(locale, item, true);
                let value = crate::gameplay::item_value(item, gold);
                Command {
                    unavailable: None,
                    order: format!("sell:{j}"),
                    icon,
                    label: locale.label("misc", "sellitem3"),
                    hotkey: String::new(),
                    cost: Some(
                        locale
                            .label("viewmsg", "sell_item")
                            .replacen("%d", &(4 * value).to_string(), 1)
                            .replacen("%s", &gold_name, 1),
                    ),
                    submenu: None,
                }
            })
            .collect();
    }
    let mut menu_entries = skin.menus.entries(menu).to_vec();
    // 0x49f0cc: when every selected object carries a spy of a kingdom the
    // player commands (vt+0xbc: a spy its own, a host the hidden one), the
    // spy's orders join the menu. With nothing selected the native menu is
    // only the kingdom's own (0x49ee25).
    if state.command_menu.is_none()
        && !state.selected.is_empty()
        && state.selected.iter().all(|&i| {
            mission.objects.get(i).is_some_and(|o| {
                let spy = if o.class == 14 { o.owner } else { o.mole };
                spy != 0 && mission.controls(spy)
            })
        })
    {
        menu_entries.extend_from_slice(skin.menus.entries("spy_add"));
    }
    // Spells learned from books join the unit's own menu (0x4e3fb0).
    if state.command_menu.is_none()
        && let [i] = state.selected[..]
        && let Some(actor) = mission
            .simulation
            .as_ref()
            .and_then(|s| s.gameplay.actors.get(i))
    {
        for key in actor
            .spells
            .iter()
            .filter_map(|&s| crate::world::spell_key(s))
        {
            if !menu_entries
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case(key))
            {
                menu_entries.push((key.to_owned(), String::new()));
            }
        }
    }
    let result: Vec<Command> = menu_entries
        .iter()
        .filter(|(key, _)| !key.eq_ignore_ascii_case("default"))
        // 0x49edb0 intersects class menus, and suppresses trade in groups.
        // Tab changes information, not the group commands.
        .filter(|(key, _)| {
            state.command_menu.is_some()
                || state.selected.len() <= 1
                || (!key.eq_ignore_ascii_case("trade")
                    && state.selected.iter().skip(1).all(|&i| {
                        let Some(o) = mission.objects.get(i) else {
                            return false;
                        };
                        o.owner == mission.player()
                            && crate::gameplay::object_menu(
                                mission.simulation.as_ref(),
                                library,
                                o,
                                i,
                            )
                            .is_some_and(|menu| {
                                skin.menus
                                    .entries(&menu)
                                    .iter()
                                    .any(|(other, _)| key.eq_ignore_ascii_case(other))
                            })
                    }))
        })
        .filter(|(key, _)| {
            if let Some(sim) = mission.simulation.as_ref() {
                if !state
                    .selected
                    .iter()
                    .all(|&i| crate::gameplay::ship_upgrade_available(sim, i, key))
                {
                    return false;
                }
            }
            let Some(rules) = mission
                .rules
                .get(mission.player().saturating_sub(1) as usize)
            else {
                return true;
            };
            let action = skin
                .orders
                .get(key, "action")
                .unwrap_or("")
                .to_ascii_lowercase();
            let class = skin
                .orders
                .get(key, "classname")
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            // «notech» does not ask the forbidden lists (0x494bbf).
            let free = mission.cheats.no_tech;
            if action.starts_with("build ") || action.starts_with("create ") {
                return free || !crate::gameplay::forbidden(rules, &class);
            }
            let research = action
                .strip_prefix("research ")
                .unwrap_or(
                    if matches!(
                        action.split_whitespace().next(),
                        Some("technology" | "upgrade")
                    ) {
                        &class
                    } else {
                        key
                    },
                )
                .trim()
                .to_ascii_lowercase();
            (free || !crate::gameplay::forbidden(rules, &research))
                && !rules
                    .initial_researches
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(&research))
        })
        .filter_map(|(key, hotkey)| {
            // The INI number reader (0x518e90) takes the digits it finds:
            // `ORDERS\move.bmp` of the soul gem's two commands is picture 0.
            // An empty IMG (`default`, `onlinehelp`) is no button.
            let icon = skin
                .orders
                .get(key, "img")
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| v.parse::<usize>().unwrap_or(0))?;
            let researches = mission
                .rules
                .get(mission.player().saturating_sub(1) as usize)
                .map_or(&[][..], |r| r.initial_researches.as_slice());
            let class = skin
                .orders
                .get(key, "classname")
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(key)
                .trim()
                .to_ascii_lowercase();
            let tree = skin.tech_trees.get(civ);
            // `[orders]` is one per order, the last tree loaded wins (0x496362).
            let ordered = ["asia", "arab", "euro"]
                .iter()
                .find_map(|c| skin.tech_trees.get(*c)?.get("orders", key));
            // «notech»: nothing a type depends on is asked (0x494c90).
            let free = mission.cheats.no_tech;
            let missing_research = tree
                .and_then(|t| t.get("depend", &class))
                .or(ordered)
                .filter(|_| !free)
                .filter(|required| {
                    !researches
                        .iter()
                        .any(|r| r.eq_ignore_ascii_case(required.trim()))
                });
            let missing_building = tree
                .and_then(|t| t.get("required", &class))
                .filter(|_| !free)
                .filter(|required| {
                    !mission.objects.iter().any(|o| {
                        o.owner == mission.player()
                            && o.flags
                                & (crate::world::FLAG_INACTIVE
                                    | crate::world::FLAG_CONSTRUCTION
                                    | 4)
                                == 0
                            && o.class_name().eq_ignore_ascii_case(required.trim())
                    })
                });
            let action = skin
                .orders
                .get(key, "action")
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            match action.split_whitespace().next() {
                Some("technology") => {
                    // Hidden while a building runs it (+0x2cc, 0x494bff).
                    if missing_research.is_some()
                        || missing_building.is_some()
                        || mission.simulation.as_ref().is_some_and(|sim| {
                            crate::gameplay::researching(sim, mission, mission.player(), &class)
                        })
                    {
                        return None;
                    }
                }
                // 0x494c51: an upgrade waits out of sight for the research
                // it depends on (+0x108); only the other entries show greyed
                // with the requirement (0x494c90, 0x49e2a4).
                Some("upgrade")
                    if !free
                        && tree
                            .and_then(|t| t.get("depend", &class))
                            .is_some_and(|required| {
                                !researches
                                    .iter()
                                    .any(|r| r.eq_ignore_ascii_case(required.trim()))
                            }) =>
                {
                    return None;
                }
                _ => {}
            }
            // 0x49dd21: a list's button is off while the list is empty;
            // the spies' while the kingdom counts none (K+0x120).
            let empty_list = match key.as_str() {
                "view_heroes" => list_entries(ListKind::Heroes, mission).is_empty(),
                "view_spys" => crate::gameplay::spy_count(mission, mission.player()) == 0,
                "view_items" => list_entries(ListKind::Items, mission).is_empty(),
                _ => false,
            };
            let unavailable = missing_research
                .map(|r| {
                    format!(
                        "{} {}",
                        locale.label("misc", "req3"),
                        locale.label("orders_names", r.trim())
                    )
                })
                .or_else(|| {
                    missing_building.map(|r| {
                        format!(
                            "{} {}",
                            locale.label("misc", "req3"),
                            locale.label("objects", r.trim())
                        )
                    })
                })
                .or_else(|| empty_list.then(String::new));
            Some(Command {
                unavailable,
                order: key.to_ascii_lowercase(),
                icon,
                label: locale.label("orders_names", key),
                hotkey: hotkey.trim().to_owned(),
                cost: skin
                    .orders
                    .get(key, "classname")
                    .and_then(|class| {
                        skin.researched_cost_text(
                            locale,
                            civ,
                            &class.trim().to_ascii_lowercase(),
                            researches,
                        )
                    })
                    .or_else(|| {
                        let action = skin.orders.get(key, "action").unwrap_or("");
                        let research = action
                            .trim()
                            .strip_prefix("research ")
                            .unwrap_or(key)
                            .trim()
                            .to_ascii_lowercase();
                        skin.cost_text(locale, "comm", &research)
                    }),
                submenu: skin
                    .orders
                    .get(key, "submenu")
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.trim().to_owned()),
            })
        })
        .collect();
    result
}
fn pages(count: usize) -> usize {
    count.div_ceil(COMMANDS_PER_PAGE).max(1)
}
pub fn contents<'a>(state: &HudState, mission: &'a MissionWorld) -> &'a [usize] {
    state
        .contents_owner
        .and_then(|i| mission.simulation.as_ref()?.objects.get(i))
        .map_or(&[], |o| o.contents.as_slice())
}
fn page_count(state: &HudState, mission: &MissionWorld, commands: usize) -> usize {
    pages(if state.contents_owner.is_some() {
        contents(state, mission).len()
    } else if let Some(kind) = listing(state) {
        list_entries(kind, mission).len()
    } else {
        commands
    })
}
/// The inventory's button (0x49cc3e): live while the first selected object
/// holds something (its list at +0x76, message 0x2710, with a count at
/// +8), grey otherwise; an item holds nothing.
fn holder_enabled(state: &HudState, mission: &MissionWorld) -> bool {
    state.selected.first().is_some_and(|&i| {
        mission
            .simulation
            .as_ref()
            .and_then(|s| s.objects.get(i))
            .is_some_and(|o| !o.contents.is_empty())
    })
}

/// The objects of a list (0x49f1fc–0x49f4a3), in map order: the heroes
/// of the player's kingdom, promoted or named (0x49d590), then those of
/// kingdom 1 when the player holds it an ally (0x49d6c0); the objects
/// carrying a spy of the player, the spy itself or the unit it hides in
/// (vt+0xbc, 0x49d720); the items the player's units hold (0x462340).
pub fn list_entries(kind: ListKind, mission: &MissionWorld) -> Vec<usize> {
    use crate::world::{FLAG_DEAD, FLAG_DYING, FLAG_HELD, FLAG_HERO, FLAG_INACTIVE};
    let player = mission.player();
    let living = |o: &MapObject| o.flags & (FLAG_INACTIVE | FLAG_DYING | FLAG_DEAD) == 0;
    // A kingdom's units as its list holds them (K+0x56), the latest to join
    // it first.
    let units = |owner: u8| -> Vec<usize> {
        match mission.simulation.as_ref() {
            Some(sim) => crate::gameplay::ai::think::kingdom_list(sim, mission, owner, true),
            None => (0..mission.objects.len())
                .filter(|&i| {
                    let o = &mission.objects[i];
                    o.owner == owner && crate::selection::unit(o)
                })
                .collect(),
        }
        .into_iter()
        .filter(|&i| living(&mission.objects[i]))
        .collect()
    };
    let heroes = |owner: u8| {
        units(owner).into_iter().filter(|&i| {
            let o = &mission.objects[i];
            o.flags & FLAG_HERO != 0 || !o.name.is_empty()
        })
    };
    match kind {
        ListKind::Heroes => {
            let mut out: Vec<usize> = heroes(player).collect();
            if player != 1 && diplomacy_word(&mission.diplomacy, player, 1) & diplomacy::ALLY != 0 {
                out.extend(heroes(1));
            }
            out
        }
        ListKind::Spies if player != 0 => (1..=8u8)
            .flat_map(units)
            .filter(|&i| {
                let o = &mission.objects[i];
                // The spy (class 14) answers its owner, its host the spy's.
                (if o.class == 14 { o.owner } else { o.mole }) == player
            })
            .collect(),
        ListKind::Spies => Vec::new(),
        ListKind::Items => {
            let Some(sim) = mission.simulation.as_ref() else {
                return Vec::new();
            };
            (0..mission.objects.len())
                .filter(|&i| {
                    let o = &mission.objects[i];
                    o.item()
                        && o.flags & FLAG_HELD != 0
                        && sim
                            .objects
                            .get(i)
                            .and_then(|x| x.holder)
                            .and_then(|h| mission.objects.get(h))
                            .is_some_and(|h| h.owner == player)
                })
                .collect()
        }
    }
}
/// The list a submenu of the grid shows: HEROES, SPYS or ITEMS of
/// `DATA\ORDERS.INI` (0x49f1fc, 0x49f2d8, 0x49f449).
/// Whether the command grid shows the cheat panel's button: switched on,
/// nothing selected, the kingdom's menu in the grid.
pub fn cheats_button(state: &HudState) -> bool {
    state.cheats
        && state.selected.is_empty()
        && state.tree.is_none()
        && state.command_menu.is_none()
        && state.contents_owner.is_none()
        && state.dialog.is_none()
        && state.frame() == MapFrame::Panels
}
pub fn listing(state: &HudState) -> Option<ListKind> {
    match state.command_menu.as_deref()?.to_ascii_lowercase().as_str() {
        "heroes" => Some(ListKind::Heroes),
        "spys" => Some(ListKind::Spies),
        "items" => Some(ListKind::Items),
        _ => None,
    }
}
const LIST_ROWS: usize = 9;
/// Text box of the message window, relative to the window.
const MESSAGE_TEXT: Rect = Rect {
    x: 167,
    y: 45,
    w: 286,
    h: 182,
};
/// The message window's text (0x47db28): Arial12B in #A37B1B, five pixels
/// between lines (0x47e28e), wrapped 5 short of the box (0x47e607).
fn message_lines(font: &BitmapFont, text: &str) -> Vec<String> {
    font.wrap(text, (MESSAGE_TEXT.w - 5) as usize)
}
/// The lines the box shows: those whose letters end inside it (0x47e4e4).
fn message_rows(font: &BitmapFont) -> usize {
    let step = font.height as i32 + 5;
    ((MESSAGE_TEXT.h - font.height as i32 - 1) / step + 1).max(1) as usize
}

pub fn widgets(
    skin: &HudSkin,
    locale: &Ini,
    state: &HudState,
    layout: Layout,
    mission: &MissionWorld,
    library: &ObjectLibrary,
) -> Vec<HudButton> {
    let mut out = Vec::new();
    let mut add = |rect, label: String, action, active| {
        out.push(HudButton {
            rect,
            label,
            action,
            active,
        })
    };
    add(
        layout.menu_button(),
        locale.label("game_scr", "menu"),
        HudAction::Pause,
        false,
    );
    add(
        layout.diplomacy_button(),
        locale.label("game_scr", "diplomacy"),
        HudAction::Panel(Panel::Diplomacy),
        state.panel == Some(Panel::Diplomacy),
    );
    add(
        layout.chat_button(),
        locale.label("game_scr", "chat"),
        HudAction::Chat,
        matches!(state.panel, Some(Panel::Chat(_))),
    );
    if let Some(sim) = &mission.simulation {
        let min = sim.clock_minutes();
        add(
            layout.clock(),
            format!("{:02}:{:02}", min / 60, min % 60),
            HudAction::Inert,
            false,
        );
    }
    if let Some(i) = state.inspected_object()
        && let Some(o) = mission.objects.get(i)
    {
        let mut label = library.label(o);
        if o.item() {
            label = format!("{label} · {}", library.item_description(o));
        }
        add(
            Rect {
                x: layout.selection().x + 6,
                y: layout.selection().y + 5,
                w: 48,
                h: 48,
            },
            label,
            HudAction::Inert,
            false,
        );
        if let Some(actor) = mission
            .simulation
            .as_ref()
            .and_then(|sim| sim.gameplay.actors.get(crate::gameplay::producer(sim, i)))
        {
            if let Some(job) = actor.jobs.front() {
                let name = match &job.kind {
                    crate::gameplay::JobKind::Train(class) => library.class_label(&MapObject {
                        class: *class,
                        ..o.clone()
                    }),
                    crate::gameplay::JobKind::Research(name)
                    | crate::gameplay::JobKind::Upgrade(name, _) => {
                        locale.label("orders_names", name)
                    }
                    crate::gameplay::JobKind::Item(..) => {
                        locale.label("orders_names", "build_item")
                    }
                };
                let mut status = format!(
                    "{name}: {}%",
                    (job.total - job.remaining) * 100 / job.total.max(1)
                );
                if let Some(reason) = &actor.production_blocked {
                    status.push_str(&format!("\n{reason}"));
                }
                add(
                    Rect {
                        x: layout.selection().x + 66,
                        y: layout.selection().y + 25,
                        w: 74,
                        h: 20,
                    },
                    status,
                    HudAction::Inert,
                    false,
                );
            }
        }
    }
    let list = commands(skin, locale, state, mission, library);
    if holder_enabled(state, mission) {
        add(
            layout.arrows()[1],
            locale.label("misc", "view3"),
            HudAction::Holder,
            state.contents_owner.is_some(),
        );
    }
    if state.page == 0 && !state.command_history.is_empty() {
        add(
            layout.arrows()[0],
            "Indietro".into(),
            HudAction::BackMenu,
            false,
        );
    }
    let page = state.page.min(page_count(state, mission, list.len()) - 1);
    if page_count(state, mission, list.len()) > 1 {
        let [previous, _, next] = layout.arrows();
        if page > 0 {
            add(
                previous,
                locale.label("misc", "prevpage3"),
                HudAction::Page(-1),
                false,
            );
        }
        if page + 1 < page_count(state, mission, list.len()) {
            add(
                next,
                locale.label("misc", "nextpage3"),
                HudAction::Page(1),
                false,
            );
        }
    }
    // A command dialog takes the place of the grid; its controls have no
    // labels, only the hint of `[orders_names]` (the items show their name).
    if let Some(d) = &state.dialog {
        let origin = layout.dialog();
        for (control, r, key) in d.controls() {
            let label = match control {
                Control::Item if d.option == 0 => locale.label(
                    "spell_names",
                    crate::world::spell_key(d.item).unwrap_or(key),
                ),
                Control::Item => locale.label("images", dialogs::item_image(d.option, d.item)),
                Control::Market(k) => d.markets[k as usize]
                    .and_then(|i| mission.objects.get(i))
                    .map_or_else(
                        || locale.label("orders_names", key),
                        |o| {
                            format!(
                                "{} · {}",
                                locale.label("orders_names", key),
                                library.label(o)
                            )
                        },
                    ),
                _ => locale.label("orders_names", key),
            };
            add(
                Rect {
                    x: origin.x + r.x,
                    y: origin.y + r.y,
                    ..r
                },
                label,
                HudAction::Dialog(control),
                d.picking.is_some_and(|k| control == Control::Market(k)),
            );
        }
    }
    let shown = if state.dialog.is_some() {
        0
    } else {
        COMMANDS_PER_PAGE
    };
    if state.contents_owner.is_some() && state.dialog.is_none() {
        let on_page = contents(state, mission)
            .iter()
            .skip(page * COMMANDS_PER_PAGE)
            .take(COMMANDS_PER_PAGE);
        // The active item's button is checked (0x4a1435), and the panel
        // keeps one checked button (+0x25c, 0x4a1cf0): each marked item
        // takes the check from the one before, so the page's last shows it.
        let checked = on_page.clone().rev().copied().find(|&index| {
            mission.objects.get(index).is_some_and(|o| o.item())
                && mission
                    .simulation
                    .as_ref()
                    .is_some_and(|s| s.objects.get(index).is_some_and(|x| x.active))
        });
        for (slot, &index) in on_page.enumerate() {
            if let Some(o) = mission.objects.get(index) {
                add(
                    layout.command(slot),
                    library.label(o),
                    HudAction::InspectContent(index),
                    checked == Some(index),
                );
            }
        }
    }
    // A list in the grid (0x49fa70, 0x49fdb0, 0x49f750): a button for each
    // object, eight a page, with no hint (0x581f40).
    if let Some(kind) = listing(state)
        && state.contents_owner.is_none()
        && state.dialog.is_none()
    {
        let entries = list_entries(kind, mission);
        for (slot, &index) in entries
            .iter()
            .skip(page * COMMANDS_PER_PAGE)
            .take(COMMANDS_PER_PAGE)
            .enumerate()
        {
            add(
                layout.command(slot),
                String::new(),
                HudAction::Listed(index),
                false,
            );
        }
    }
    for (slot, (i, c)) in list
        .iter()
        .enumerate()
        .skip(page * COMMANDS_PER_PAGE)
        .take(shown)
        .enumerate()
    {
        let mut label = if c.hotkey.is_empty() {
            c.label.clone()
        } else {
            // The original's hint (0x4e6540): "Nome - 'T' ( pietra:200 cibo:200 )".
            format!("{} - '{}'", c.label, c.hotkey)
        };
        if let Some(cost) = &c.cost {
            label = format!("{label} ( {cost} )");
        }
        if let Some(reason) = &c.unavailable {
            label = format!("{label} · {reason}");
        }
        if let Some(description) = locale
            .get("orders_add", &c.order)
            .filter(|s| !s.trim().is_empty())
        {
            label = format!("{label}\n{description}");
        }
        add(layout.command(slot), label, HudAction::Command(i), false);
    }
    // The cheat panel's button, after the kingdom's menu of an empty
    // selection.
    let taken = list
        .len()
        .saturating_sub(page * COMMANDS_PER_PAGE)
        .min(shown);
    if cheats_button(state) && taken < COMMANDS_PER_PAGE {
        add(
            layout.command(taken),
            "Trucchi\nApre la finestra dei trucchi di questa partita".into(),
            HudAction::OpenCheats,
            false,
        );
    }
    // Hints of the original dialogs (LOCAL/HELP.INI).
    let help = |key: &str| skin.hints.label("HELP", key);
    if let Some(panel) = state.panel {
        let w = state.window(layout, panel);
        let at = |x, y, wd, h| Rect {
            x: w.x + x,
            y: w.y + y,
            w: wd,
            h,
        };
        match panel {
            Panel::Narration => {
                // `ok1.bmp` at (113, 510) of the book, «close» of
                // [general] (0x4c37eb).
                let [ox, oy] = story_origin(w.w, w.h);
                add(
                    Rect {
                        x: ox + 113,
                        y: oy + 510,
                        w: 150,
                        h: 25,
                    },
                    locale.label("general", "close"),
                    HudAction::Close,
                    false,
                );
            }
            Panel::Say => {
                for (k, bubble) in state.say.iter().enumerate() {
                    for (answer, r, lines) in say_rows(&skin.font12b, bubble) {
                        add(r, lines.join(" "), HudAction::Answer(answer), false);
                    }
                    let (eye, speaker) = say_buttons(bubble);
                    if bubble.speaker.is_some() {
                        add(eye, String::new(), HudAction::SayEye(k), false);
                    }
                    if bubble.voiced {
                        add(
                            speaker,
                            String::new(),
                            HudAction::SayVoices,
                            !state.voices_off,
                        );
                    }
                    // A line being said goes on with a click anywhere on
                    // its bubble (0x4c088f).
                    if bubble.single {
                        add(say_rect(bubble), String::new(), HudAction::Answer(0), false);
                    }
                }
            }
            Panel::Quests => {
                add(
                    at(246, 304, 210, 27),
                    locale.label("wnd_quests", "close"),
                    HudAction::Close,
                    false,
                );
                add(
                    at(38, 305, 25, 26),
                    String::new(),
                    HudAction::Page(-1),
                    false,
                );
                add(
                    at(191, 305, 25, 26),
                    String::new(),
                    HudAction::Page(1),
                    false,
                );
            }
            Panel::Diplomacy => {
                // Positions of the original dialog (constructor 0x445900): one
                // row of 32 pixels per kingdom from y 65; attitude buttons at x
                // 214, 246, 279, sharing at 331, 363, 395.
                for (row, owner) in other_kingdoms(mission).into_iter().enumerate() {
                    let y = 65 + 32 * row as i32;
                    for (x, bit, hint) in [
                        (214, diplomacy::ALLY, "D_ALLIES"),
                        (246, diplomacy::NEUTRAL, "D_NEUTRAL"),
                        (279, diplomacy::ENEMY, "D_ENEMIES"),
                    ] {
                        add(
                            at(x, y, 28, 28),
                            help(hint),
                            HudAction::Relation(owner, bit),
                            false,
                        );
                    }
                    for (x, bit, hint) in [
                        (331, diplomacy::VISION, "D_SHARED VISION"),
                        (363, diplomacy::RESOURCES, "D_SHARED RESOURCES"),
                        (395, diplomacy::UNITS, "D_SHARED UNIT CONTROL"),
                    ] {
                        add(
                            at(x, y, 28, 28),
                            help(hint),
                            HudAction::Share(owner, bit),
                            false,
                        );
                    }
                }
                add(
                    at(31, 304, 31, 31),
                    help("D_SHARED_VICTORY"),
                    HudAction::SharedVictory,
                    false,
                );
                // The wait before tributes (0x5cf814) runs only in network
                // games (0x4416db): here the button is always ready.
                add(
                    at(267, 304, 189, 27),
                    help("D_TRIBUTE"),
                    HudAction::Tribute,
                    false,
                );
                add(
                    at(38, 354, 189, 27),
                    help("D_ACCEPT"),
                    HudAction::Accept,
                    false,
                );
                add(
                    at(267, 354, 189, 27),
                    help("D_CANCEL"),
                    HudAction::Close,
                    false,
                );
            }
            Panel::Tribute => {
                // Constructor 0x447270: per row an amount of gold, stone,
                // food and wood at x 209 + 91·r, y 65 + 32·row (86×30);
                // Send at (39, 318) and Cancel at (366, 318).
                for (row, owner) in other_kingdoms(mission).into_iter().enumerate() {
                    for res in 0..4u8 {
                        add(
                            at(209 + 91 * res as i32, 65 + 32 * row as i32, 86, 30),
                            help("T_AMOUNT"),
                            HudAction::Amount(owner, res),
                            false,
                        );
                    }
                }
                add(at(39, 318, 189, 27), help("T_SEND"), HudAction::Send, false);
                add(
                    at(366, 318, 189, 27),
                    help("T_CANCEL"),
                    HudAction::Close,
                    false,
                );
            }
            Panel::Cheats => {
                // Every item with words for the hint line: a control, or
                // a grey one or a row's name that only shows its hint.
                for item in cheat_panel::items(skin, locale, state, mission, library) {
                    if item.hint.is_empty() {
                        continue;
                    }
                    let action = match item.control {
                        Some(c) if item.enabled => HudAction::Cheat(c),
                        _ => HudAction::Inert,
                    };
                    let r = item.rect;
                    add(at(r.x, r.y, r.w, r.h), item.hint, action, false);
                }
                add(
                    at(39, 318, 189, 27),
                    "Usato un trucco, la partita resta segnata come partita con trucchi".into(),
                    HudAction::Inert,
                    false,
                );
                add(
                    at(366, 318, 189, 27),
                    "Chiude la finestra dei trucchi".into(),
                    HudAction::Close,
                    false,
                );
            }
            Panel::Chat(large) => {
                // Constructor 0x422f00: recipients at y 87, x 37, 93, 149 and
                // 425; the large window lists the kingdoms from y 152.
                add(at(45, 58, 404, 20), help("CH_BOX"), HudAction::Inert, false);
                for (x, to, hint) in [
                    (37, Recipients::All, "CH_ALL"),
                    (93, Recipients::Allies, "CH_ALLIES"),
                    (149, Recipients::Enemies, "CH_ENEMIES"),
                    (425, Recipients::Custom, "CH_CUSTOM"),
                ] {
                    add(
                        at(x, 87, 28, 28),
                        help(hint),
                        HudAction::Recipients(to),
                        state.chat.to == to,
                    );
                }
                if large {
                    for (i, owner) in other_kingdoms(mission).into_iter().enumerate() {
                        let (x, y) = chat_row(i);
                        add(
                            at(x, y, 31, 31),
                            help("CH_SELECT_PLAYER"),
                            HudAction::ChatTo(owner),
                            state.chat.custom[owner as usize - 1],
                        );
                    }
                }
            }
            Panel::Story => {
                add(
                    at(206, 258, 210, 27),
                    locale.label("wnd_message", "close"),
                    HudAction::Close,
                    false,
                );
                // Phrases offered in a conversation are rows like a list's.
                if matches!(panel, Panel::Story)
                    && let Some(message) = &mission.message
                {
                    for (row, choice) in message.choices.iter().enumerate().take(LIST_ROWS) {
                        add(
                            Rect {
                                x: w.x + MESSAGE_TEXT.x,
                                y: w.y + MESSAGE_TEXT.y + row as i32 * 20,
                                w: MESSAGE_TEXT.w - 12,
                                h: 19,
                            },
                            choice.clone(),
                            HudAction::Answer(row),
                            false,
                        );
                    }
                }
            }
        }
        // The ball, made after the window's controls; it has no hint.
        if panel.has_help() {
            add(
                at(0, 0, HELP_BALL[0], HELP_BALL[1]),
                String::new(),
                HudAction::Help,
                state.help.is_some(),
            );
        }
    }
    // Without the panels only the open window answers: 0x4f9990 hides the
    // rest of the interface, and F12 lays the map over it.
    if state.frame() != MapFrame::Panels {
        let window = state.panel.map(|p| state.window(layout, p));
        out.retain(|b| {
            window.is_some_and(|w| {
                b.rect.x >= w.x
                    && b.rect.y >= w.y
                    && b.rect.x + b.rect.w <= w.x + w.w
                    && b.rect.y + b.rect.h <= w.y + w.h
            })
        });
    }
    out
}

/// Top-left corner of row `i` of the chat window's kingdom list: two
/// columns of four rows.
fn chat_row(i: usize) -> (i32, i32) {
    (37 + 230 * (i / 4) as i32, 152 + 32 * (i % 4) as i32)
}

/// How far the open window can scroll, in its own units (pages of
/// objectives, lines of the story, rows of a list).
pub fn scroll_limit(
    skin: &HudSkin,
    state: &HudState,
    mission: &MissionWorld,
    story: &str,
) -> usize {
    match state.panel {
        Some(Panel::Quests) => mission.quests.len().div_ceil(2).saturating_sub(1),
        Some(Panel::Story) => message_lines(&skin.font12b, story)
            .len()
            .saturating_sub(message_rows(&skin.font12b)),
        _ => 0,
    }
}

/// Scroll bar of the message window, inside its text box.
pub fn scroll_track(state: &HudState, layout: Layout) -> Option<Rect> {
    let panel = state.panel.filter(|p| matches!(p, Panel::Story))?;
    let w = state.window(layout, panel);
    Some(Rect {
        // Right of the text box, where the original keeps its arrows.
        x: w.x + MESSAGE_TEXT.x + MESSAGE_TEXT.w + 2,
        y: w.y + MESSAGE_TEXT.y,
        w: 8,
        h: MESSAGE_TEXT.h,
    })
}
pub fn scroll_thumb(state: &HudState, layout: Layout) -> Option<Rect> {
    let r = scroll_track(state, layout)?;
    let h = (r.h as f32 * 8. / (state.scroll_max + 8) as f32)
        .round()
        .max(16.) as i32;
    Some(Rect {
        y: r.y
            + if state.scroll_max == 0 {
                0
            } else {
                ((r.h - h) as f32 * state.scroll as f32 / state.scroll_max as f32).round() as i32
            },
        h: h.min(r.h),
        ..r
    })
}

/// The control under `p`. An open window lies over the rest of the
/// interface: where it is, only its own controls answer, and the
/// diplomacy window under the tributes takes the points it covers.
pub fn under(
    widgets: &[HudButton],
    state: &HudState,
    layout: Layout,
    p: [f32; 2],
) -> Option<usize> {
    if let Some(panel) = state.panel {
        let w = state.window(layout, panel);
        if w.has(p[0], p[1]) {
            return widgets
                .iter()
                .position(|b| b.rect.has(p[0], p[1]) && inside(b.rect, w));
        }
        if panel == Panel::Tribute && state.window(layout, Panel::Diplomacy).has(p[0], p[1]) {
            return None;
        }
    }
    widgets.iter().position(|b| b.rect.has(p[0], p[1]))
}

pub fn blocks_map(state: &HudState, p: [f32; 2], layout: Layout) -> bool {
    let frame = state.frame();
    !layout.map_window(frame).has(p[0], p[1])
        || frame == MapFrame::Panels && layout.corner().has(p[0], p[1])
        || state.panel.is_some_and(|panel| {
            state.window(layout, panel).has(p[0], p[1])
                || (panel == Panel::Tribute
                    && state.window(layout, Panel::Diplomacy).has(p[0], p[1]))
        })
}

/// Black of `alpha` laid over what `r` holds: pictures under it darken,
/// transparent pixels take the black, so the map below shows through.
fn shade(out: &mut Bitmap, r: Rect, alpha: u8) {
    let a = alpha as u32;
    let canvas = [out.width as usize, out.height as usize];
    crate::font::with_ids(canvas, |mut cover| {
        for y in r.y.max(0)..(r.y + r.h).min(out.height as i32) {
            for x in r.x.max(0)..(r.x + r.w).min(out.width as i32) {
                let k = y as usize * out.width as usize + x as usize;
                let p = &mut out.rgba[k * 4..k * 4 + 4];
                for c in &mut p[..3] {
                    *c = (*c as u32 * (255 - a) / 255) as u8;
                }
                p[3] = (a + p[3] as u32 * (255 - a) / 255) as u8;
                if let Some(cover) = cover.as_mut() {
                    cover.mark(k);
                }
            }
        }
    });
}
fn fill(out: &mut Bitmap, r: Rect, color: [u8; 4]) {
    // A visible fill covers the texts recorded under it (`font::record`).
    let canvas = [out.width as usize, out.height as usize];
    crate::font::with_ids(canvas, |mut cover| {
        for y in r.y.max(0)..(r.y + r.h).min(out.height as i32) {
            for x in r.x.max(0)..(r.x + r.w).min(out.width as i32) {
                let k = y as usize * out.width as usize + x as usize;
                out.rgba[k * 4..k * 4 + 4].copy_from_slice(&color);
                if color[3] != 0
                    && let Some(cover) = cover.as_mut()
                {
                    cover.mark(k);
                }
            }
        }
    });
}
fn frame(out: &mut Bitmap, r: Rect, color: [u8; 4]) {
    for q in [
        Rect { h: 1, ..r },
        Rect {
            y: r.y + r.h - 1,
            h: 1,
            ..r
        },
        Rect { w: 1, ..r },
        Rect {
            x: r.x + r.w - 1,
            w: 1,
            ..r
        },
    ] {
        fill(out, q, color)
    }
}
fn text(font: &BitmapFont, out: &mut Bitmap, text: &str, r: Rect, color: [u8; 3], centered: bool) {
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || "/(%).,-".contains(c))
    {
        font.draw_number(
            &mut out.rgba,
            [out.width as usize, out.height as usize],
            text,
            [r.x, r.y, r.w, r.h],
            color,
            centered,
        );
        return;
    }
    font.draw_fit(
        &mut out.rgba,
        [out.width as usize, out.height as usize],
        text,
        [r.x, r.y, r.w, r.h],
        color,
        centered,
    );
}
/// Copy `r` of `src` to `dst`, scaling when the sizes differ and skipping
/// transparent pixels.
fn blit(out: &mut Bitmap, src: &Bitmap, r: Rect, dst: Rect) {
    let canvas = [out.width as usize, out.height as usize];
    crate::font::with_ids(canvas, |cover| blit_over(out, src, r, dst, cover));
}
/// `blit`, clearing the marks of the recorded texts it covers.
fn blit_over(
    out: &mut Bitmap,
    src: &Bitmap,
    r: Rect,
    dst: Rect,
    mut cover: Option<crate::font::Cover>,
) {
    for y in 0..dst.h {
        for x in 0..dst.w {
            let (tx, ty) = (dst.x + x, dst.y + y);
            let (sx, sy) = (r.x + x * r.w / dst.w, r.y + y * r.h / dst.h);
            if tx < 0
                || ty < 0
                || tx >= out.width as i32
                || ty >= out.height as i32
                || sx < 0
                || sy < 0
                || sx >= src.width as i32
                || sy >= src.height as i32
            {
                continue;
            }
            let a = ((sy * src.width as i32 + sx) * 4) as usize;
            let b = ((ty * out.width as i32 + tx) * 4) as usize;
            if src.rgba[a + 3] != 0 {
                out.rgba[b..b + 4].copy_from_slice(&src.rgba[a..a + 4]);
                if let Some(cover) = cover.as_mut() {
                    cover.mark(b / 4);
                }
            }
        }
    }
}
/// A picture remapped through one of the engine's colour tables (0x45a2a0,
/// colours at 0x589a44; vt+0x5c mode 6 of a figure, 0x461c7d): each pixel,
/// read as RGB565, takes its luminance 0.29 R + 0.11 B + 0.6 G (truncated,
/// 0x45a1d4) as the step of a ramp from black to the colour (0–127:
/// colour × step / 127) and on to white (128 on: colour + (255 − colour) ×
/// (step − 128) / 128), written back as RGB565.
pub fn tinted(src: &Bitmap, tint: [u8; 3]) -> Bitmap {
    let mut out = src.clone();
    for px in out.rgba.chunks_exact_mut(4).filter(|px| px[3] != 0) {
        let [r, g, b] = [px[0] & 0xf8, px[1] & 0xfc, px[2] & 0xf8].map(f64::from);
        let step = (r * 0.29 + b * 0.11 + g * 0.6) as i32;
        let [r, g, b] = tint.map(|c| {
            let c = c as i32;
            (if step < 128 {
                c * step / 127
            } else {
                c + (255 - c) * (step - 128) / 128
            }) as u8
        });
        let [r, g, b] = [r & 0xf8, g & 0xfc, b & 0xf8];
        px[0] = r | r >> 5;
        px[1] = g | g >> 6;
        px[2] = b | b >> 5;
    }
    out
}
fn whole(b: &Bitmap) -> Rect {
    Rect {
        x: 0,
        y: 0,
        w: b.width as i32,
        h: b.height as i32,
    }
}
/// Frame `index` of a strip of `count` equal frames laid side by side.
fn strip(b: &Bitmap, count: i32, index: i32) -> Rect {
    let w = b.width as i32 / count.max(1);
    Rect {
        x: w * index.clamp(0, count - 1),
        y: 0,
        w,
        h: b.height as i32,
    }
}
/// Frame `index` of a strip of `count` equal frames stacked vertically.
fn column(b: &Bitmap, count: i32, index: i32) -> Rect {
    let h = b.height as i32 / count.max(1);
    Rect {
        x: 0,
        y: h * index.clamp(0, count - 1),
        w: b.width as i32,
        h,
    }
}

/// Text colours of the dialogs' numbers (Arial14B, centred): palette
/// indices 32 normal, 31 hover or pressed, 33 disabled.
const DIALOG_TEXT: [u8; 3] = [0xa3, 0x7b, 0x1b];
const DIALOG_HOVER: [u8; 3] = [0xe2, 0xa3, 0x19];
const DIALOG_DISABLED: [u8; 3] = [0x5d, 0x49, 0x11];
/// A command dialog over the grid: its background, icons and numbers.
fn draw_dialog(
    skin: &MenuSkin,
    classic: &HudSkin,
    b: &mut Bitmap,
    v: &HudView,
    layout: Layout,
    d: &Dialog,
    buttons: &[HudButton],
) {
    let folder = match d.kind {
        Kind::Gamble => "D_GAMBLE",
        Kind::Exchange => "D_TRADE1",
        Kind::Route => "D_TRADE",
        Kind::Workshop => "D_BUILD",
    };
    let art = |n: &str| classic.dialog_art.get(format!("{folder}/{n}").as_str());
    let origin = layout.dialog();
    if let Some(back) = art("BACK_V") {
        blit(b, back, whole(back), origin);
    }
    let available = crate::gameplay::stock(v.mission, v.mission.player());
    let sheet = classic.icon_images.get("IMAGES/ICONS/ICONS.BMP");
    let resource_icon = |b: &mut Bitmap, r: u8, at: Rect| {
        if let Some((image, src)) = classic.icon(dialogs::RESOURCE_ICONS[r as usize & 3]) {
            blit(b, image, src, at);
        }
    };
    for (i, button) in buttons.iter().enumerate() {
        let HudAction::Dialog(control) = button.action else {
            continue;
        };
        let enabled = d.enabled(control, available);
        let hover = v.state.hover == Some(i);
        let pressed = hover && v.state.press == Some(button.action);
        let shift = pressed as i32;
        let r = Rect {
            x: button.rect.x + shift,
            y: button.rect.y + shift,
            ..button.rect
        };
        let mut drawn = true;
        match (d.kind, control) {
            (_, Control::Cancel) => {
                if let Some(image) = art("CANCEL") {
                    blit(b, image, whole(image), r);
                }
            }
            (_, Control::Accept) => {
                let name = match d.kind {
                    Kind::Gamble => "GAMBLE",
                    Kind::Workshop => "BUILD",
                    _ => "TRADE",
                };
                if let Some(image) = art(name) {
                    blit(b, image, whole(image), r);
                }
            }
            (_, Control::Resource) => resource_icon(b, d.resource, r),
            (_, Control::Resource2) => resource_icon(b, d.resource2, r),
            (Kind::Gamble, Control::Odds) => {
                if let Some(image) = art("PROP") {
                    blit(b, image, column(image, 4, d.option as i32), r);
                }
                if let Some(image) = art("BIG FRAME") {
                    blit(b, image, whole(image), r);
                }
            }
            (Kind::Route, Control::Market(k)) => {
                let market = d.markets[k as usize].and_then(|i| v.mission.objects.get(i));
                match market.and_then(|o| {
                    v.library
                        .icon_for(o)
                        .and_then(|key| classic.icon_for_color(key, v.mission.color_slot(o.owner)))
                }) {
                    Some((image, src)) => blit(b, image, src, r),
                    None => {
                        if let Some(image) = art("OBJECT") {
                            blit(b, image, whole(image), r);
                        }
                    }
                }
            }
            (Kind::Workshop, Control::Category(c)) => {
                let name = ["BOOKS", "POTIONS", "POWER"][c as usize];
                // NRSD: the active category shows its brightest frame.
                let state = if !enabled {
                    3
                } else if pressed {
                    2
                } else if hover {
                    1
                } else {
                    0
                };
                if let Some(image) = art(name) {
                    blit(b, image, strip(image, 4, state), button.rect);
                }
                continue;
            }
            (Kind::Workshop, Control::Item) => {
                if let Some(image) = sheet {
                    let frames = image.height as i32 / 48;
                    let frame = dialogs::item_icon(d.option, d.item) as i32;
                    blit(b, image, column(image, frames, frame), r);
                }
            }
            (_, Control::Amount | Control::Received) => {
                let background = match d.kind {
                    Kind::Exchange => "OBJECT",
                    _ => "AMOUNT",
                };
                if let Some(image) = art(background) {
                    blit(b, image, whole(image), button.rect);
                }
                let value = d.number(control).unwrap_or(0);
                let label = match (d.kind, d.markets) {
                    // 0x4a2844: "%d (+%d)" once both markets are set.
                    (Kind::Route, [Some(a), Some(m)]) => {
                        let owner = v.mission.objects.get(d.object).map_or(0, |o| o.owner);
                        let profit = crate::gameplay::trade_profit(
                            v.mission,
                            v.library,
                            owner,
                            [a, m],
                            value,
                            v.side,
                        );
                        format!("{value} (+{profit})")
                    }
                    _ => value.to_string(),
                };
                let color = if !enabled {
                    DIALOG_DISABLED
                } else if hover {
                    DIALOG_HOVER
                } else {
                    DIALOG_TEXT
                };
                text(&skin.font, b, &label, button.rect, color, true);
                drawn = false;
            }
            _ => drawn = false,
        }
        // 0x49b510: these are buttons of the command grid's class, framed
        // by ORDERS\frame1.bmp, which does not move when pressed (0x49b8be);
        // the odds wear their own big frame.
        if matches!(
            control,
            Control::Resource
                | Control::Resource2
                | Control::Market(_)
                | Control::Item
                | Control::Cancel
                | Control::Accept
        ) {
            blit(b, &classic.frame1, whole(&classic.frame1), button.rect);
        }
        if button.active {
            frame(b, button.rect, [255, 255, 255, 255]);
        }
        if drawn && !enabled {
            for y in button.rect.y.max(0)..(button.rect.y + button.rect.h).min(b.height as i32) {
                for x in button.rect.x.max(0)..(button.rect.x + button.rect.w).min(b.width as i32) {
                    let at = (y as usize * b.width as usize + x as usize) * 4;
                    for c in &mut b.rgba[at..at + 3] {
                        *c /= 2;
                    }
                }
            }
        }
    }
    // D_GAMBLE: the result face at (8,115), the dice over it while they roll.
    if d.kind == Kind::Gamble {
        let r = Rect {
            x: origin.x + 8,
            y: origin.y + 115,
            w: 99,
            h: 48,
        };
        match (d.dice_frame(), art("ZAROVE")) {
            (Some(frame), Some(dice)) => blit(
                b,
                dice,
                strip(dice, dialogs::DICE_FRAMES as i32, frame as i32),
                Rect { w: 93, h: 42, ..r },
            ),
            _ => {
                if let Some(face) = art("WINLOSE") {
                    blit(b, face, column(face, 3, d.face as i32), r);
                }
            }
        }
    }
}
pub struct HudView<'a> {
    pub width: i32,
    pub height: i32,
    pub left: bool,
    pub title: &'a str,
    pub description: &'a str,
    pub state: &'a HudState,
    pub mission: &'a MissionWorld,
    pub library: &'a ObjectLibrary,
    /// Hero and objective portraits of the scenario, by archive path in
    /// upper case (`USER/MERLIN.BMP`).
    pub portraits: &'a HashMap<String, Bitmap>,
    pub camera: [f32; 3],
    pub side: usize,
    pub pixels: [f32; 2],
    /// The message list at the top of the map (0x4f8a50), oldest first.
    pub messages: &'a [String],
    /// The status line at the bottom of the map (0x4f8350) while no hint
    /// takes its place.
    pub status: &'a str,
    /// The human player's name (settings), shown over their objects.
    pub player_name: &'a str,
    /// `[colors] selection` of the biome: the scoreboard's text (0x4f2a4d).
    pub selection: [u8; 3],
    /// How the texts over the map are drawn (`Settings::map_text`): 0 the
    /// original's shadow, 1 a black outline, 2 a dark band behind them.
    pub map_text: usize,
}
impl HudView<'_> {
    fn layout(&self) -> Layout {
        Layout {
            width: self.width,
            height: self.height,
            left: self.left,
        }
    }
    fn portrait(&self, path: &str) -> Option<&Bitmap> {
        let key = path.replace('\\', "/").to_ascii_uppercase();
        self.portraits.get(&key)
    }
    /// The story window's text: the scenario's story, or its description.
    pub fn story(&self) -> String {
        story_text(self.mission, self.title, self.description)
    }
}
/// The text of the message window: what a script shows, or the mission's
/// description.
pub fn story_text(mission: &MissionWorld, title: &str, description: &str) -> String {
    match &mission.message {
        Some(message) => message.text.clone(),
        None if description.trim().is_empty() => title.to_owned(),
        None => format!("{title}\n\n{}", description.trim()),
    }
}

const TEXT: [u8; 3] = [247, 239, 214];
const TEXT_DIM: [u8; 3] = [214, 201, 160];
const TITLE: [u8; 3] = [247, 219, 140];
/// Hour of `DAYNIGHT.BMP` (one 36×36 frame per hour) a mission opens at:
/// the original sets its clock to 10:00 when a game starts (0x42b9b9).
const START_HOUR: i32 = 10;
/// Text of the selection view (colour 11 of the original table).
const WHITE: [u8; 3] = [255, 255, 255];

/// Statistics shown beside the portrait: icon key of `DATA\ICONS.INI` and value.
pub fn statistics(skin: &HudSkin, o: &MapObject) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if [class::RES_GOLD, class::RES_STONE].contains(&o.class) {
        let key = if o.class == class::RES_GOLD {
            "resgold"
        } else {
            "resstone"
        };
        let amount = if o.byte(0x3f) != 0 {
            skin.infinite.clone()
        } else {
            o.dword(0x3a).to_string()
        };
        out.push((key, amount));
        return out;
    }
    let Some(stats) = skin.class_stats(o) else {
        return out;
    };
    if o.building() {
        if let Some(r) = stats.resistance {
            out.push(("resistanceicon", r.to_string()));
        }
        if (39..=43).contains(&o.class) {
            out.push(("attackicon_bow", stats.attack.unwrap_or(0).to_string()));
        }
        return out;
    }
    if let Some(a) = stats.attack.filter(|&a| a > 0) {
        out.push((
            if crate::simulation::weapon_category(o.class) != 0 {
                "attackicon_bow"
            } else {
                "attackicon_hand"
            },
            a.to_string(),
        ));
    }
    if let Some(d) = stats.defence {
        out.push(("defenceicon", d.to_string()));
    }
    out
}

/// Individual imported attributes, equipment bonuses and book charges.
pub fn object_statistics(
    skin: &HudSkin,
    mission: &MissionWorld,
    index: usize,
) -> Vec<(&'static str, String)> {
    let Some(o) = mission.objects.get(index) else {
        return vec![];
    };
    if o.class == class::ITEM_PLUSPLUS {
        return [
            ("attackicon_hand", o.byte(0x16) as u16),
            ("attackicon_bow", o.byte(0x17) as u16),
            ("defenceicon", o.word(0x18)),
        ]
        .into_iter()
        .filter(|(_, n)| *n != 0)
        .map(|(k, n)| (k, n.to_string()))
        .collect();
    }
    if o.class == class::ITEM_HEALTH {
        // Native potion HUD 0x464909: temporary attack, defence, health,
        // mana and experience values, rather than equipment modifiers.
        return [
            ("attackicon_hand", o.word(0x16)),
            ("defenceicon", o.word(0x18)),
            ("powericon", o.word(0x12)),
            ("mannaicon", o.word(0x24)),
            ("levelicon", o.word(0x1c)),
        ]
        .into_iter()
        .filter(|(_, n)| *n != 0)
        .map(|(k, n)| (k, n.to_string()))
        .collect();
    }
    if matches!(o.class, class::RES_GOLD | class::RES_STONE) {
        return statistics(skin, o);
    }
    let Some(state) = mission
        .simulation
        .as_ref()
        .and_then(|s| s.effective_attributes(index, mission))
    else {
        return statistics(skin, o);
    };
    let mut rows = vec![];
    if o.building() {
        rows.push(("resistanceicon", state.resistance.to_string()));
        // Tower statistics include resistance and attack (original 0x4dc205).
        if (39..=43).contains(&o.class) {
            rows.push(("attackicon_bow", state.attack.to_string()));
        }
    } else if crate::selection::unit(o) {
        if state.attack > 0 {
            rows.push((
                if state.ranged {
                    "attackicon_bow"
                } else {
                    "attackicon_hand"
                },
                state.attack.to_string(),
            ));
        }
        rows.push(("defenceicon", state.defence.to_string()));
        if o.class_name() != "trader" {
            let level = if matches!(o.class, 23..=25) {
                mission
                    .simulation
                    .as_ref()
                    .map_or(1, |s| crate::gameplay::ship_level(s, index)) as u16
            } else {
                state.level
            };
            rows.push(("levelicon", level.to_string()));
        }
    }
    if o.class == class::ITEM_MAGIC && o.byte(99) != 0 && o.word(102) < 9999 {
        rows.push(("attackicon_hand", o.word(102).to_string()));
    }
    if let Some(actor) = mission.simulation.as_ref().and_then(|sim| {
        sim.gameplay
            .actors
            .get(crate::gameplay::producer(sim, index))
    }) {
        if let Some(job) = actor.jobs.front() {
            // Query 0x2712: "%02d%%" for the player's own or shared buildings,
            // "100%" once done but waiting to come out.
            if mission.controls(o.owner) {
                rows.push((
                    "buildicon",
                    if actor.production_blocked.is_some() {
                        "100%".into()
                    } else {
                        format!(
                            "{:02}%",
                            (job.total - job.remaining) * 100 / job.total.max(1)
                        )
                    },
                ));
            }
        } else if actor.construction > 0 {
            rows.push((
                "buildicon",
                format!(
                    "{}%",
                    actor.construction_total.saturating_sub(actor.construction) * 100
                        / actor.construction_total.max(1)
                ),
            ));
        }
        if let Some(resource) = actor.cargo.iter().position(|&n| n > 0) {
            rows.push((
                ["resgold", "resstone", "resfood", "reswood"][resource],
                actor.cargo[resource].to_string(),
            ));
        }
    }
    if state.max_mana > 0 {
        rows.push(("mannaicon", format!("{}/{}", state.mana, state.max_mana)));
    }
    rows
}

/// Draws the dynamic interface over `base`, a cached [`HudSkin::background`]
/// of the same size and side.
pub fn render(skin: &MenuSkin, classic: &HudSkin, base: Bitmap, v: HudView<'_>) -> Bitmap {
    debug_assert_eq!((base.width as i32, base.height as i32), (v.width, v.height));
    let layout = v.layout();
    if v.state.frame() != MapFrame::Panels {
        return film(skin, classic, &v, layout);
    }
    let mut b = base;
    let locale = &skin.locale;
    let font = &classic.font;
    // The minimap sits beneath the HUD; modal artwork covers its intersection.
    fill(&mut b, layout.minimap(), [0; 4]);

    // Top bar: counters of the human player (not always kingdom 1).
    let player = v.mission.player();
    // What the player may spend, shared resources included (0x480815 →
    // 0x4677d0).
    let resources = crate::gameplay::stock(v.mission, player);
    let (population, housing) = crate::gameplay::population(v.mission, v.library, player);
    // The record holds gold, stone, food, wood (the engine's order, as its
    // cost fields 0xda–0xe6); the bar shows gold, wood, food, stone and
    // «population/limit» (0x480e31: "%d/%d", the limit being the smaller of
    // the housing and the game's maximum).
    for (r, value) in layout.resources().into_iter().zip([
        resources[0].to_string(),
        resources[3].to_string(),
        resources[2].to_string(),
        resources[1].to_string(),
        format!("{population}/{}", housing),
    ]) {
        text(font, &mut b, &value, r, TEXT, true);
    }
    // The clock shows the hour the mission starts at (10:00, as the original).
    blit(
        &mut b,
        &classic.clock,
        Rect {
            x: 0,
            y: v.mission
                .simulation
                .as_ref()
                .map_or(START_HOUR, |s| (s.clock_minutes() / 60) as i32)
                * 36,
            w: 36,
            h: 36,
        },
        layout.clock(),
    );
    let buttons = widgets(classic, locale, v.state, layout, v.mission, v.library);
    let side = if v.left { "LEFT" } else { "RIGHT" };
    for (i, name) in ["MENU", "DIPLOMACY", "CHAT"].iter().enumerate() {
        let button = &buttons[i];
        let image = &classic.buttons[&format!("BTN {name} {side}")];
        let down = v.state.press == Some(button.action) || button.active;
        blit(&mut b, image, strip(image, 2, down as i32), button.rect);
        if i == 0 {
            text(
                &skin.small,
                &mut b,
                &button.label,
                Rect {
                    x: button.rect.x + down as i32,
                    y: button.rect.y + down as i32,
                    ..button.rect
                },
                TEXT,
                true,
            );
        }
    }

    selection_view(classic, locale, &mut b, &v, layout);

    // Command grid and page buttons.
    let list = commands(classic, locale, v.state, v.mission, v.library);
    let page_count = page_count(v.state, v.mission, list.len());
    let [previous, holder, next] = layout.arrows();
    let page = v.state.page.min(page_count - 1);
    for (rect, image, enabled, step) in [
        (
            previous,
            &classic.arrows[0],
            page > 0 || !v.state.command_history.is_empty(),
            -1,
        ),
        (
            holder,
            &classic.arrows[1],
            holder_enabled(v.state, v.mission),
            0,
        ),
        (next, &classic.arrows[2], page + 1 < page_count, 1),
    ] {
        let state = if !enabled {
            3
        } else if v.state.press
            == Some(if step == 0 {
                HudAction::Holder
            } else {
                if step == -1 && page == 0 && !v.state.command_history.is_empty() {
                    HudAction::BackMenu
                } else {
                    HudAction::Page(step)
                }
            })
            || (step == 0 && v.state.contents_owner.is_some())
        {
            2
        } else if buttons.iter().enumerate().any(|(i, w)| {
            w.action
                == if step == 0 {
                    HudAction::Holder
                } else {
                    if step == -1 && page == 0 && !v.state.command_history.is_empty() {
                        HudAction::BackMenu
                    } else {
                        HudAction::Page(step)
                    }
                }
                && v.state.hover == Some(i)
        }) {
            1
        } else {
            0
        };
        blit(&mut b, image, strip(image, 4, state), rect);
    }
    for (i, button) in buttons.iter().enumerate() {
        if let HudAction::InspectContent(index) | HudAction::Listed(index) = button.action {
            // 0x4a13e3: a button of the command grid's class (0x49b800): the
            // icon moves while pressed, the frame stays.
            let shift = (v.state.press == Some(button.action) && v.state.hover == Some(i)) as i32;
            if let Some(o) = v.mission.objects.get(index)
                && let Some((image, src)) = v
                    .library
                    .icon_for(o)
                    .and_then(|k| classic.icon_for_color(k, v.mission.color_slot(o.owner)))
            {
                let at = Rect {
                    x: button.rect.x + shift,
                    y: button.rect.y + shift,
                    ..button.rect
                };
                blit(&mut b, image, src, at);
            }
            // A checked button wears the frame of colour table 5 (0x49b8df).
            let frame = if button.active {
                &classic.frame1_checked
            } else {
                &classic.frame1
            };
            blit(&mut b, frame, whole(frame), button.rect);
            continue;
        }
        if button.action == HudAction::OpenCheats {
            // A command button like the others, with the open book of the
            // original's buttons (`ORDERS\BUTTONS.BMP`, 140).
            let pressed = v.state.press == Some(button.action) && v.state.hover == Some(i);
            let icons = classic.commands.height as i32 / 48;
            let shift = pressed as i32;
            blit(
                &mut b,
                &classic.commands,
                column(&classic.commands, icons, CHEATS_ICON),
                Rect {
                    x: button.rect.x + shift,
                    y: button.rect.y + shift,
                    ..button.rect
                },
            );
            blit(&mut b, &classic.frame1, whole(&classic.frame1), button.rect);
            continue;
        }
        let HudAction::Command(index) = button.action else {
            continue;
        };
        let pressed = v.state.press == Some(button.action) && v.state.hover == Some(i);
        let icons = classic.commands.height as i32 / 48;
        let shift = pressed as i32;
        blit(
            &mut b,
            &classic.commands,
            column(&classic.commands, icons, list[index].icon as i32),
            Rect {
                x: button.rect.x + shift,
                y: button.rect.y + shift,
                ..button.rect
            },
        );
        // The original frames the icon and only moves it while pressed.
        if list[index].unavailable.is_some() {
            for y in button.rect.y.max(0)..(button.rect.y + button.rect.h).min(b.height as i32) {
                for x in button.rect.x.max(0)..(button.rect.x + button.rect.w).min(b.width as i32) {
                    let at = (y as usize * b.width as usize + x as usize) * 4;
                    for c in &mut b.rgba[at..at + 3] {
                        *c /= 2;
                    }
                }
            }
        }
        blit(&mut b, &classic.frame1, whole(&classic.frame1), button.rect);
        let count = queued_count(v.state, v.mission, classic, &list[index].order);
        if count > 0 {
            // 0x49e9f0: "%3d" at the lower right, with nothing behind it.
            let r = Rect {
                x: button.rect.x + 24,
                y: button.rect.y + 31,
                w: 24,
                h: 15,
            };
            text(
                &classic.font,
                &mut b,
                &format!("{count:3}"),
                r,
                WHITE,
                false,
            );
        }
    }

    clock(skin, classic, &mut b, &v, layout.play());

    if let Some(d) = &v.state.dialog {
        draw_dialog(skin, classic, &mut b, &v, layout, d, &buttons);
    }

    if let Some(panel) = v.state.panel {
        window(skin, classic, &mut b, &v, layout, panel, &buttons);
    }

    message_list(classic, &mut b, &v, layout.play(), layout.left);
    hints(skin, &mut b, &v, layout, layout.play(), &buttons);
    b
}

/// F4 (0x4f2903): the clock at the top right of the map view, and the
/// scoreboard above its bottom right corner, in Arial12B with a shadow.
fn clock(skin: &MenuSkin, classic: &HudSkin, b: &mut Bitmap, v: &HudView, play: Rect) {
    if v.state.score != 0
        && let Some(sim) = &v.mission.simulation
    {
        let right = play.x + play.w;
        // 0x4f2a3a: black at +1, +1, then the biome's selection colour.
        let shadowed = |b: &mut Bitmap, s: &str, r: Rect| {
            map_text(&skin.small, b, s, r, v.selection, v.map_text)
        };
        let seconds = sim.ticks * crate::gameplay::TICK_MS / 1000;
        shadowed(
            b,
            &format!(
                "{}:{:02}:{:02}",
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60
            ),
            Rect {
                x: right - 80,
                y: play.y + 10,
                w: 70,
                h: 20,
            },
        );
        if v.state.score == 1 {
            let rows = crate::gameplay::scores(sim, v.mission);
            let bottom = play.y + play.h - 10;
            let top = bottom - 20 * rows.len() as i32;
            for (k, (kingdom, own, team)) in rows.iter().enumerate() {
                let y = top + 20 * k as i32;
                let name = &v.mission.kingdoms[*kingdom as usize - 1].name;
                // 0x4f29f6: each row ends 12 pixels left of the square.
                let line = format!("{name}  {own}/{team}");
                let w = skin.small.measure(&line) as i32;
                let x = right - 22 - w;
                shadowed(
                    b,
                    &line,
                    Rect {
                        x,
                        y,
                        w: w + 2,
                        h: 20,
                    },
                );
                // 0x4f2ad8: a defeated kingdom (+0x4e) is struck through.
                if sim
                    .gameplay
                    .script
                    .kingdoms
                    .get(*kingdom as usize)
                    .is_some_and(|s| s.defeated)
                {
                    let [r, g, bl] = v.selection;
                    fill(
                        b,
                        Rect {
                            x,
                            y: y + 6,
                            w: w + 1,
                            h: 1,
                        },
                        [r, g, bl, 255],
                    );
                }
                if let Some(slot) = v.mission.color_slot(*kingdom) {
                    let [r, g, bl] = classic.player_colors[slot];
                    fill(
                        b,
                        Rect {
                            x: right - 10 - 10,
                            y: y + 4,
                            w: 4,
                            h: 4,
                        },
                        [r, g, bl, 255],
                    );
                }
            }
        }
    }
}

/// The map view's lines at its bottom left, and the help ball's box.
fn hints(
    skin: &MenuSkin,
    b: &mut Bitmap,
    v: &HudView,
    layout: Layout,
    play: Rect,
    buttons: &[HudButton],
) {
    // Hint of the control under the pointer: the original's text line at the
    // bottom left of the map view, Arial12B in white over a black shadow at
    // +1, +1 (0x4f275f), its box 10 pixels from the view's left edge and 20
    // above its bottom (measured on the running original).
    // A line of the game (0x4f8350) holds it while no hint is shown.
    // The controls of a window with the help ball give theirs only to it.
    let helped = v
        .state
        .panel
        .filter(|p| p.has_help())
        .map(|p| v.state.window(layout, p));
    let hint = v
        .state
        .hover
        .and_then(|i| buttons.get(i))
        .filter(|button| !helped.is_some_and(|w| inside(button.rect, w)))
        .map(|button| button.label.as_str())
        .filter(|label| !label.is_empty())
        .or(Some(v.status).filter(|s| !s.is_empty()));
    if let Some(hint) = hint {
        let hint_lines = skin.small.wrap(hint, (play.w - 20).max(1) as usize);
        let lines = hint_lines.len().min(4) as i32;
        let r = Rect {
            x: play.x + 10,
            y: play.y + play.h - 6 - 14 * lines,
            w: play.w - 20,
            h: 14 * lines,
        };
        for (i, line) in hint_lines.iter().take(4).enumerate() {
            let at = Rect {
                y: r.y + i as i32 * 14,
                h: 14,
                ..r
            };
            map_text(&skin.small, b, line, at, color::WHITE, v.map_text);
        }
    }
    // The help ball's mode (0x45c600): the hint of the window's control
    // under the pointer, in the menus' box (Arial12B in black on cream,
    // 20 pixels below the pointer, its width the text's plus 20, pushed
    // left in proportion to the pointer's place on the screen).
    if let (Some([px, py]), Some(w)) = (v.state.help, helped)
        && let Some(button) = buttons
            .iter()
            .rev()
            .find(|b| b.rect.has(px as f32, py as f32))
        && inside(button.rect, w)
        && !button.label.is_empty()
    {
        let width = skin.small.measure(&button.label) as i32 + 20;
        let r = Rect {
            x: px - px * width / layout.width.max(1),
            y: (py + 20).min(layout.height - 16),
            w: width,
            h: 16,
        };
        fill(b, r, [0, 0, 0, 255]);
        let inner = Rect {
            x: r.x + 1,
            y: r.y + 1,
            w: r.w - 2,
            h: r.h - 2,
        };
        let [red, green, blue] = color::CREAM;
        fill(b, inner, [red, green, blue, 255]);
        text(&skin.small, b, &button.label, r, color::BLACK, true);
    }
}

/// Whether `r` lies wholly within `w`.
fn inside(r: Rect, w: Rect) -> bool {
    r.x >= w.x && r.y >= w.y && r.x + r.w <= w.x + w.w && r.y + r.h <= w.y + w.h
}

/// The story screen (0x4c3b50): black, the book in the middle, the
/// picture on its left page (297×395 from 87, 75), the text going up the
/// right page and the button (0x4c37eb).
fn narration(classic: &HudSkin, b: &mut Bitmap, v: &HudView, w: Rect, buttons: &[HudButton]) {
    fill(b, w, [0, 0, 0, 255]);
    let book = &classic.windows["story"];
    let [ox, oy] = story_origin(w.w, w.h);
    let at = |x, y, w, h| Rect {
        x: ox + x,
        y: oy + y,
        w,
        h,
    };
    blit(
        b,
        book,
        whole(book),
        at(0, 0, book.width as i32, book.height as i32),
    );
    let message = v.mission.message.as_ref();
    if let Some(image) = message.and_then(|m| v.portrait(&m.portrait)) {
        let [iw, ih] = [
            (image.width as i32).min(297),
            (image.height as i32).min(395),
        ];
        let r = Rect {
            x: 0,
            y: 0,
            w: iw,
            h: ih,
        };
        blit(b, image, r, at(87, 75, iw, ih));
    }
    // Arial14B in black, two pixels between lines (+0x280), wrapped 5 short
    // of the box's 215 pixels; over the story's time the text goes from just
    // under the box to just over it (0x4c3d7d), clipped by it.
    let font = &classic.font14b;
    let lines = font.wrap(message.map_or("", |m| m.text.as_str()), 210);
    let step = font.height as i32 + 2;
    let area = at(333, 172, 215, 363);
    let top = match v.state.crawl {
        Some([gone, ms]) if ms > 0 => {
            let rise = (area.h + lines.len() as i32 * step) as u64 * gone / ms;
            area.y + area.h - 1 - rise as i32
        }
        _ => area.y,
    };
    let mut page = Bitmap {
        width: area.w as u32,
        height: area.h as u32,
        rgba: vec![0; (area.w * area.h * 4) as usize],
    };
    for (i, line) in lines.iter().enumerate() {
        let y = top - area.y + i as i32 * step;
        if y + step > 0 && y < area.h {
            let r = Rect {
                x: 0,
                y,
                w: area.w,
                h: font.height as i32,
            };
            text(font, &mut page, line, r, color::BLACK, false);
        }
    }
    blit(b, &page, whole(&page), area);
    // Four states of 150×25 (NRSD), «close» in the middle in Arial14B,
    // #A37B1B, #E2A319 under the pointer or pressed (0x4c3846).
    if let Some((i, button)) = buttons
        .iter()
        .enumerate()
        .find(|(_, x)| x.action == HudAction::Close)
    {
        let ok = &classic.windows["story_ok"];
        let pressed = v.state.press == Some(HudAction::Close);
        let lit = pressed || v.state.hover == Some(i);
        let frame = if pressed { 2 } else { lit as i32 };
        let src = Rect {
            x: frame * 150,
            y: 0,
            w: 150,
            h: 25,
        };
        blit(b, ok, src, button.rect);
        let ink = if lit { color::GOLD } else { color::DARK_GOLD };
        text(font, b, &button.label, button.rect, ink, true);
    }
}
/// The bubbles of a conversation (0x4c0220, 0x4c0020): the tail of discs,
/// the black back at half and its frame, the portrait in a gold border,
/// the lines and the two buttons.
fn say(classic: &HudSkin, b: &mut Bitmap, v: &HudView, buttons: &[HudButton]) {
    let art = |k: &str| &classic.windows[k];
    let font = &classic.font12b;
    let hovered = |action: HudAction| {
        v.state
            .hover
            .and_then(|i| buttons.get(i))
            .is_some_and(|x| x.action == action)
    };
    for (k, bubble) in v.state.say.iter().enumerate() {
        let [bx, by] = bubble.at;
        let rect = Rect {
            x: bx,
            y: by,
            w: SAY_SIZE[0],
            h: SAY_SIZE[1],
        };
        let apart = |r: Rect| {
            r.x > rect.x + rect.w - 1
                || r.x + r.w - 1 < rect.x
                || r.y > rect.y + rect.h - 1
                || r.y + r.h - 1 < rect.y
        };
        // Eight discs at eighths of the way from the speaker's middle, the
        // smallest, to the bubble's, none over the bubble and none with the
        // speaker nearer than 100 pixels.
        if let Some([px, py]) = bubble.speaker {
            let [cx, cy] = [bx + (SAY_SIZE[0] - 1) / 2, by + (SAY_SIZE[1] - 1) / 2];
            if (((px - cx).pow(2) + (py - cy).pow(2)) as f64).sqrt() >= 100. {
                for i in 0..8 {
                    let back = art(&format!("baloonB_{}", i + 1));
                    let front = art(&format!("baloonF_{}", i + 1));
                    let [w, h] = [front.width as i32, front.height as i32];
                    let r = Rect {
                        x: (cx * i + px * (8 - i)) / 8 - w / 2,
                        y: (cy * i + py * (8 - i)) / 8 - h / 2,
                        w,
                        h,
                    };
                    if apart(r) {
                        blit(b, back, whole(back), r);
                        blit(b, front, whole(front), r);
                    }
                }
            }
        }
        blit(b, art("say_back"), whole(art("say_back")), rect);
        blit(b, art("say_frame"), whole(art("say_frame")), rect);
        if let Some(p) = v.portrait(&bubble.portrait) {
            let gold = [color::GOLD[0], color::GOLD[1], color::GOLD[2], 255];
            frame(
                b,
                Rect {
                    x: bx + 9,
                    y: by + 9,
                    w: 98,
                    h: 98,
                },
                gold,
            );
            let [iw, ih] = [(p.width as i32).min(96), (p.height as i32).min(96)];
            let src = Rect {
                x: 0,
                y: 0,
                w: iw,
                h: ih,
            };
            blit(
                b,
                p,
                src,
                Rect {
                    x: bx + 10,
                    y: by + 10,
                    w: iw,
                    h: ih,
                },
            );
        }
        // Arial12B with a black shadow (0x47e3a0): a line being said white,
        // the ones offered gold and white under the pointer (0x4c0563).
        let step = font.height as i32 + 1;
        for (answer, r, lines) in say_rows(font, bubble) {
            let ink = if bubble.single || hovered(HudAction::Answer(answer)) {
                color::WHITE
            } else {
                color::GOLD
            };
            for (i, line) in lines.iter().enumerate() {
                let y = r.y + i as i32 * step;
                if y + font.height as i32 > by + SAY_SIZE[1] {
                    break;
                }
                let at = |dx, dy| Rect {
                    x: r.x + dx,
                    y: y + dy,
                    w: r.w,
                    h: font.height as i32,
                };
                text(font, b, line, at(1, 1), color::BLACK, false);
                text(font, b, line, at(0, 0), ink, false);
            }
        }
        // The eye (NRS, 3 × 20×20) while the speaker is on screen, the
        // speaker (on and off, 2 × 20×20) when a line has a voice.
        let (eye, speaker) = say_buttons(bubble);
        if bubble.speaker.is_some() {
            let action = HudAction::SayEye(k);
            let frame = if v.state.press == Some(action) {
                2
            } else {
                hovered(action) as i32
            };
            let src = Rect {
                x: frame * 20,
                y: 0,
                w: 20,
                h: 20,
            };
            blit(b, art("say_eye"), src, eye);
        }
        if bubble.voiced {
            let src = Rect {
                x: 0,
                y: v.state.voices_off as i32 * 20,
                w: 20,
                h: 20,
            };
            blit(b, art("say_speaker"), src, speaker);
        }
    }
}
/// Height of each black band of the film framing: 12 % of the screen
/// (0x4f9bd0).
pub fn film_band(height: i32) -> i32 {
    height * 12 / 100
}
/// The map without the panels (0x4f9b40): over the whole screen (F12), or
/// in the film framing (0x4f9990) between two black bands; over it only the
/// open window and what the map view draws itself.
fn film(skin: &MenuSkin, classic: &HudSkin, v: &HudView, layout: Layout) -> Bitmap {
    let (w, h) = (v.width.max(0) as u32, v.height.max(0) as u32);
    let mut b = Bitmap {
        width: w,
        height: h,
        rgba: vec![0; (w * h * 4) as usize],
    };
    let frame = v.state.frame();
    let band = if frame == MapFrame::Film {
        film_band(v.height)
    } else {
        0
    };
    for r in [
        Rect {
            x: 0,
            y: 0,
            w: v.width,
            h: band,
        },
        Rect {
            x: 0,
            y: v.height - band,
            w: v.width,
            h: band,
        },
    ] {
        fill(&mut b, r, [0, 0, 0, 255]);
    }
    let buttons = widgets(classic, &skin.locale, v.state, layout, v.mission, v.library);
    if let Some(panel) = v.state.panel {
        window(skin, classic, &mut b, v, layout, panel, &buttons);
    }
    let view = layout.map_window(frame);
    clock(skin, classic, &mut b, v, view);
    message_list(classic, &mut b, v, view, v.left);
    hints(skin, &mut b, v, layout, view, &buttons);
    b
}

/// The message list of the original (0x4f2832): up to five lines in
/// Arial12B, one under the other at the height of an Arial14B line
/// (0x5cf904), from 20 pixels below the top of the map view and 20 from its
/// left edge, 50 with the panel on the left (+0x1676, 0x4f8848), clear of
/// the clock; each drawn first in black one pixel down and right, then in
/// white.
fn message_list(classic: &HudSkin, b: &mut Bitmap, v: &HudView, view: Rect, left: bool) {
    let x = view.x + if left { 50 } else { 20 };
    let step = classic.font14b.height as i32;
    for (k, line) in v.messages.iter().enumerate() {
        let r = Rect {
            x,
            y: view.y + 20 + k as i32 * step,
            w: (view.x + view.w - x).max(1),
            h: step,
        };
        map_text(&classic.font12b, b, line, r, WHITE, v.map_text);
    }
}
/// A text over the map: the original's black shadow at +1, +1 (0x4f2763,
/// 0x4f2855, 0x4f2a3a); with [`HudView::map_text`] 1 black all round, with 2
/// the shadow over a dark band that lets the map show through.
fn map_text(font: &BitmapFont, b: &mut Bitmap, s: &str, r: Rect, ink: [u8; 3], style: usize) {
    if style == 2 {
        // Where `text` puts the line: left, centred in the box's height.
        let width = (font.measure(s) as i32).min(r.w);
        let top = r.y + ((r.h - font.height as i32) / 2).max(0);
        let band = Rect {
            x: r.x - 4,
            y: top - 2,
            w: width + 9,
            h: font.height as i32 + 4,
        };
        shade(b, band, 160);
    }
    let around: &[[i32; 2]] = if style == 1 {
        &[
            [-1, -1],
            [0, -1],
            [1, -1],
            [-1, 0],
            [1, 0],
            [-1, 1],
            [0, 1],
            [1, 1],
        ]
    } else {
        &[[1, 1]]
    };
    for [dx, dy] in around {
        let at = Rect {
            x: r.x + dx,
            y: r.y + dy,
            ..r
        };
        text(font, b, s, at, [0, 0, 0], false);
    }
    text(font, b, s, r, ink, false);
}

/// The selection view of the original: portrait, hit points, name and
/// statistics of one object, or MULTISELECT and mean health for a group.
fn selection_view(classic: &HudSkin, locale: &Ini, b: &mut Bitmap, v: &HudView, layout: Layout) {
    let view = layout.selection();
    let at = |x, y, w, h| Rect {
        x: view.x + x,
        y: view.y + y,
        w,
        h,
    };
    // A chosen tree (0x437bc0): the class's icon, the wood of 100 and the
    // name «tree» of `msg`, with no owner.
    if let (None, Some((_, wood))) = (v.state.selected.first(), v.state.tree) {
        let r = at(6, 5, 48, 48);
        let class = crate::world::CLASSES[crate::world::class::TREESELECTION];
        if let Some((image, src)) = v
            .library
            .objects
            .get(class)
            .and_then(|d| d.get("comm", "icon"))
            .and_then(|k| classic.icon_for_color(k, None))
        {
            blit(b, image, src, r);
        }
        blit(b, &classic.frame1, whole(&classic.frame1), r);
        text(
            &classic.font,
            b,
            &format!("{wood}/100"),
            at(3, 66, 54, 13),
            TEXT,
            false,
        );
        let bar = at(3, 81, 54, 3);
        fill(b, bar, [16, 16, 16, 255]);
        let width = (wood as i32 * 54 / 100).clamp(0, 54);
        fill(b, Rect { w: width, ..bar }, [32, 200, 32, 255]);
        let name = locale.label("msg", "tree");
        text(&classic.font, b, &name, at(3, 108, 113, 15), WHITE, false);
        return;
    }
    let Some(&first) = v.state.selected.first() else {
        return;
    };
    let o = &v.mission.objects[v.state.inspected_object().unwrap_or(first)];
    let portrait = |b: &mut Bitmap, o: &MapObject, r: Rect| {
        if let Some(image) = (!o.portrait.is_empty())
            .then(|| v.portrait(&o.portrait))
            .flatten()
        {
            blit(b, image, whole(image), r);
        } else if let Some((image, src)) = v
            .library
            .icon_for(o)
            .and_then(|k| classic.icon_for_color(k, v.mission.color_slot(o.owner)))
        {
            blit(b, image, src, r);
        }
    };
    let count = v.state.selected.len();
    if v.state.inspected_object().is_some() {
        let r = at(6, 5, 48, 48);
        portrait(b, o, r);
        blit(b, &classic.frame1, whole(&classic.frame1), r);
        if o.max_health > 0 {
            text(
                &classic.font,
                b,
                &format!("{}/{}", o.health, o.max_health),
                at(3, 66, 54, 13),
                TEXT,
                false,
            );
            let bar = at(3, 81, 54, 3);
            fill(b, bar, [16, 16, 16, 255]);
            let width = (o.health as i32 * 54 / o.max_health as i32).clamp(0, 54);
            fill(b, Rect { w: width, ..bar }, [32, 200, 32, 255]);
        }
        // Two lines as in the original (0x49c080, 0x49d0e8), white.
        let index = v.state.inspected_object().unwrap_or(first);
        let (top, name) = name_lines(v.mission, v.library, locale, v.player_name, index);
        if let Some(top) = top {
            text(&classic.font, b, &top, at(3, 93, 113, 14), WHITE, false);
        }
        text(&classic.font, b, &name, at(3, 108, 113, 15), WHITE, false);
        for (k, (key, value)) in object_statistics(
            classic,
            v.mission,
            v.state.inspected_object().unwrap_or(first),
        )
        .into_iter()
        .take(5)
        .enumerate()
        {
            let y = 3 + 22 * k as i32;
            if let Some((image, src)) = classic.icon(key) {
                blit(b, image, src, at(66, y, 20, 20));
            }
            text(&classic.font, b, &value, at(88, y + 3, 48, 14), TEXT, false);
        }
    } else {
        // 0x49c080 draws the generic icon when the HUD inspection list is empty.
        // The stacked portraits belong to inspected holders/inventory, not to
        // ordinary multiple selection (those systems are not simulated yet).
        let r = at(6, 5, 48, 48);
        blit(b, &classic.multiselect, whole(&classic.multiselect), r);
        blit(b, &classic.frame1, whole(&classic.frame1), r);
        let percent = group_health(v.state, v.mission);
        text(
            &classic.font,
            b,
            &format!("{percent}/100(%)"),
            at(3, 66, 54, 13),
            TEXT,
            false,
        );
        let bar = at(3, 81, 54, 3);
        fill(b, bar, [16, 16, 16, 255]);
        fill(
            b,
            Rect {
                w: percent as i32 * 54 / 100,
                ..bar
            },
            [32, 200, 32, 255],
        );
        text(
            &classic.font,
            b,
            &format!("{count} {}", locale.label("selview", "units")),
            at(3, 93, 113, 14),
            WHITE,
            false,
        );
    }
}

/// 0x49cccf averages each member's integer percentage, not total HP/max HP.
pub fn group_health(state: &HudState, mission: &MissionWorld) -> u32 {
    if state.selected.is_empty() {
        return 0;
    }
    state
        .selected
        .iter()
        .filter_map(|&i| mission.objects.get(i))
        .map(|o| {
            if o.max_health == 0 {
                0
            } else {
                (o.health as u32 * 100 / o.max_health as u32).min(100)
            }
        })
        .sum::<u32>()
        / state.selected.len() as u32
}

/// Owner line of the selection: the player's own name, another kingdom's
/// leader (the scenario's «Tzar 2», …) or name, or the original's default «Tzar N» when the scenario has none.
fn owner_name(mission: &MissionWorld, player_name: &str, owner: u8) -> Option<String> {
    if !(1..=8).contains(&owner) {
        return None;
    }
    if owner == mission.player() {
        return Some(player_name.to_owned());
    }
    let name = mission.kingdoms.get(owner as usize - 1).and_then(|k| {
        [k.leader.trim(), k.name.trim()]
            .into_iter()
            .find(|n| !n.is_empty())
    });
    Some(name.map_or_else(|| format!("Tzar {owner}"), str::to_owned))
}

/// The two lines of the selection view for object `i`, from its info
/// (0x4e4850): its own name (+0x42), else its kingdom's (vt+0x114,
/// 0x505860), «Persa» for a unit in discord (0x4e5ce3); then its subtitle
/// (+0x46), else its class (vt+0x118, 0x505890). A spy's record (+0xf6:
/// a spy's own, or the one a unit hides) shows instead, to a kingdom the
/// spy's shares its vision with (relation 1), the spy's name, else its
/// kingdom's, and «subtitle (energy)», the subtitle else the spy's class
/// (0x4e5ce0, 0x4e5d50).
pub fn name_lines(
    mission: &MissionWorld,
    library: &ObjectLibrary,
    locale: &Ini,
    player_name: &str,
    i: usize,
) -> (Option<String>, String) {
    let o = &mission.objects[i];
    let discord = crate::selection::unit(o)
        && mission
            .simulation
            .as_ref()
            .is_some_and(|s| crate::gameplay::has_effect(s, i, crate::gameplay::effect::DISCORD));
    let sim = mission.simulation.as_ref();
    let record = match sim.and_then(|s| s.gameplay.actors.get(i)) {
        Some(a) if crate::selection::unit(o) && o.class == 14 => Some((
            o.owner,
            o.civilization,
            o.name.as_str(),
            o.subtitle.as_str(),
            a.spy_energy,
        )),
        Some(a) => a.mole.as_ref().map(|r| {
            (
                r.kingdom,
                r.civilization,
                r.name.as_str(),
                r.subtitle.as_str(),
                r.energy,
            )
        }),
        None => None,
    }
    .filter(|r| r.0 != 0 && mission.shares_vision(r.0));
    let top = if discord {
        Some(locale.label("selview", "berserkkingdom"))
    } else if let Some((kingdom, _, spy, ..)) = record {
        (!spy.is_empty())
            .then(|| spy.to_owned())
            .or_else(|| owner_name(mission, player_name, kingdom))
    } else if !o.name.is_empty() {
        Some(o.name.clone())
    } else {
        owner_name(mission, player_name, o.owner)
    };
    if let Some((_, civilization, _, subtitle, energy)) = record {
        let kind = if subtitle.is_empty() {
            library.class_label(&MapObject {
                class: 14,
                civilization,
                name: String::new(),
                subtitle: String::new(),
                ..o.clone()
            })
        } else {
            subtitle.to_owned()
        };
        return (top, format!("{kind} ({energy})"));
    }
    let name = if !o.subtitle.is_empty() {
        o.subtitle.clone()
    } else if o.item() {
        library.item_description(o)
    } else {
        library.class_label(o)
    };
    (top, name)
}

/// A kingdom's row in the diplomacy and tribute windows (0x4470e0,
/// 0x447d00): the row art at x 31, the colour at 41, the civilization at
/// 71 and the name in Arial14B at 96, on a row of window `w` at `y`.
#[allow(clippy::too_many_arguments)]
fn kingdom_row(
    skin: &MenuSkin,
    classic: &HudSkin,
    b: &mut Bitmap,
    v: &HudView,
    w: Rect,
    y: i32,
    owner: u8,
    name_width: i32,
) {
    let at = |x, y, wd, h| Rect {
        x: w.x + x,
        y: w.y + y,
        w: wd,
        h,
    };
    let row_art = &classic.windows["diplomacy_row"];
    blit(
        b,
        row_art,
        whole(row_art),
        at(31, y, row_art.width as i32, row_art.height as i32),
    );
    if let Some(slot) = v.mission.color_slot(owner) {
        let [red, green, blue] = classic.player_colors[slot];
        fill(b, at(41, y + 10, 13, 13), [red, green, blue, 255]);
    }
    let race = &classic.windows["race"];
    blit(
        b,
        race,
        column(race, 3, kingdom_race(v.mission, owner)),
        at(71, y + 3, 20, 20),
    );
    text(
        &skin.font,
        b,
        &v.mission.kingdoms[owner as usize - 1].name,
        at(96, y + 6, name_width, 18),
        color::GOLD,
        false,
    );
}

/// The text cut to the letters that fit in `width`.
fn fit(font: &BitmapFont, text: &str, width: i32) -> String {
    let mut out: String = text.to_owned();
    while !out.is_empty() && font.measure(&out) as i32 > width {
        out.pop();
    }
    out
}

/// The open in-game window, drawn on its original art.
fn window(
    skin: &MenuSkin,
    classic: &HudSkin,
    b: &mut Bitmap,
    v: &HudView,
    layout: Layout,
    panel: Panel,
    buttons: &[HudButton],
) {
    let locale = &skin.locale;
    let font = &classic.font;
    let w = v.state.window(layout, panel);
    let at = |x, y, wd, h| Rect {
        x: w.x + x,
        y: w.y + y,
        w: wd,
        h,
    };
    let art = |key: &str| &classic.windows[key];
    // A window under another (the diplomacy under the tributes) shows
    // its buttons up.
    let pressed = |action: HudAction| v.state.panel == Some(panel) && v.state.press == Some(action);
    if panel == Panel::Tribute {
        window(skin, classic, b, v, layout, Panel::Diplomacy, &[]);
    }
    match panel {
        Panel::Narration => narration(classic, b, v, w, buttons),
        Panel::Say => say(classic, b, v, buttons),
        Panel::Quests => {
            let back = art("quests");
            blit(b, back, whole(back), w);
            text(
                &skin.title_font,
                b,
                &locale.label("wnd_quests", "curquest"),
                at(20, 18, 460, 30),
                TITLE,
                true,
            );
            let page = v.state.scroll;
            let quests = &v.mission.quests;
            if quests.is_empty() {
                text(
                    font,
                    b,
                    &locale.label("wnd_quests", "none"),
                    at(176, 66, 276, 18),
                    TEXT,
                    false,
                );
            }
            for (row, quest) in quests.iter().skip(page * 2).take(2).enumerate() {
                let y = 62 + row as i32 * 120;
                if let Some(image) = v.portrait(&quest.portrait) {
                    blit(b, image, whole(image), at(43, y + 4, 96, 96));
                }
                for (line, s) in font.wrap(&quest.text, 272).iter().take(6).enumerate() {
                    text(
                        font,
                        b,
                        s,
                        at(178, y + 8 + line as i32 * 15, 272, 15),
                        TEXT,
                        false,
                    );
                }
            }
            text(
                font,
                b,
                &format!(
                    "{} {}/{}",
                    locale.label("wnd_quests", "page"),
                    page + 1,
                    quests.len().div_ceil(2).max(1)
                ),
                at(66, 305, 122, 26),
                TEXT_DIM,
                true,
            );
            let button = art("quests_button");
            let r = at(246, 304, 210, 27);
            let down = pressed(HudAction::Close);
            blit(b, button, strip(button, 2, down as i32), r);
            text(
                &skin.small,
                b,
                &locale.label("wnd_quests", "close"),
                Rect {
                    x: r.x + down as i32,
                    y: r.y + down as i32,
                    ..r
                },
                TEXT,
                true,
            );
        }
        Panel::Story => {
            let back = art("message");
            blit(b, back, whole(back), w);
            let name = v
                .mission
                .message
                .as_ref()
                .map(|m| m.title.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| v.title.to_owned());
            let lines = message_lines(&classic.font12b, &v.story());
            let f12 = &classic.font12b;
            for (i, line) in lines
                .iter()
                .skip(v.state.scroll)
                .take(message_rows(f12))
                .enumerate()
            {
                let y = MESSAGE_TEXT.y + i as i32 * (f12.height as i32 + 5);
                let r = at(MESSAGE_TEXT.x, y, MESSAGE_TEXT.w, f12.height as i32);
                text(f12, b, line, r, color::DARK_GOLD, false);
            }
            // The name: Arial14B in #E2A319 in the middle of (42, 167, 96,
            // 15) under the portrait (0x47dc53).
            text(
                &classic.font14b,
                b,
                &name,
                at(42, 167, 96, 15),
                color::GOLD,
                true,
            );
            // Phrases offered are rows framed under the pointer.
            for (i, button) in buttons.iter().enumerate() {
                if let HudAction::Answer(_) = button.action {
                    if v.state.hover == Some(i) {
                        frame(b, button.rect, [219, 187, 125, 255]);
                    }
                    text(font, b, &button.label, button.rect, TEXT, false);
                }
            }
            if let Some(image) = v
                .mission
                .message
                .as_ref()
                .and_then(|m| v.portrait(&m.portrait))
            {
                blit(b, image, whole(image), at(44, 44, 96, 96));
            }
            if let (Some(track), Some(thumb)) =
                (scroll_track(v.state, layout), scroll_thumb(v.state, layout))
                && v.state.scroll_max > 0
            {
                fill(b, track, [12, 18, 22, 255]);
                fill(b, thumb, [126, 112, 74, 255]);
            }
            // `d_message\btn.bmp` (NS) at (206, 258), «close» in Arial12B,
            // #A37B1B and #E2A319 under the pointer (0x47dbbb, 0x47dc0d).
            let r = at(206, 258, 210, 27);
            let button = art("message_button");
            let down = pressed(HudAction::Close);
            blit(b, button, strip(button, 2, down as i32), r);
            let lit = down
                || buttons
                    .iter()
                    .position(|x| x.action == HudAction::Close)
                    .is_some_and(|i| v.state.hover == Some(i));
            let ink = if lit { color::GOLD } else { color::DARK_GOLD };
            text(
                &classic.font12b,
                b,
                &locale.label("wnd_message", "close"),
                r,
                ink,
                true,
            );
        }
        Panel::Diplomacy => {
            // Drawing of the original (0x4470e0): row art at x 31, colour at
            // 41, civilization at 71, name in Arial14B at 96, computer icon at
            // 180; the six toggles keep their four frames.
            let back = art("diplomacy");
            blit(b, back, whole(back), w);
            text(
                &skin.small,
                b,
                &locale.label("diplomacy", "t_title"),
                at(40, 34, 400, 14),
                color::GOLD,
                false,
            );
            let player = v.mission.player();
            let d = v.state.diplomacy(v.mission);
            let hovered = |action: HudAction| {
                buttons
                    .iter()
                    .enumerate()
                    .any(|(i, w)| w.action == action && v.state.hover == Some(i))
            };
            for (row, owner) in other_kingdoms(v.mission).into_iter().enumerate() {
                let y = 65 + 32 * row as i32;
                kingdom_row(skin, classic, b, v, w, y, owner, 82);
                if v.mission.kingdoms[owner as usize - 1].computer {
                    let ai = art("ai");
                    blit(b, ai, column(ai, 2, 0), at(180, y + 5, 20, 20));
                }
                let word = diplomacy_word(d, player, owner);
                for (x, key, bit) in [
                    (214, "allies", diplomacy::ALLY),
                    (246, "neutral", diplomacy::NEUTRAL),
                    (279, "enemies", diplomacy::ENEMY),
                ] {
                    // Attitude frames: off, on, then the same under the pointer.
                    let on = word & diplomacy::ATTITUDE == bit
                        || (bit == diplomacy::ENEMY && word & diplomacy::ATTITUDE == 0);
                    let lit = hovered(HudAction::Relation(owner, bit)) as i32;
                    let image = art(key);
                    blit(
                        b,
                        image,
                        column(image, 4, on as i32 + 2 * lit),
                        at(x, y, 28, 28),
                    );
                }
                for (x, key, bit) in [
                    (331, "sh_vision", diplomacy::VISION),
                    (363, "sh_res", diplomacy::RESOURCES),
                    (395, "cmd_units", diplomacy::UNITS),
                ] {
                    // Sharing frames: on, off, then the same under the pointer.
                    let off = word & bit == 0;
                    let lit = hovered(HudAction::Share(owner, bit)) as i32;
                    let image = art(key);
                    blit(
                        b,
                        image,
                        column(image, 4, off as i32 + 2 * lit),
                        at(x, y, 28, 28),
                    );
                }
            }
            // Shared victory, then the tribute button (not available without
            // the economy) and Accept / Cancel.
            // The box is checked when every other kingdom has the bit (0x4467a9).
            let victory = other_kingdoms(v.mission)
                .into_iter()
                .all(|owner| diplomacy_word(d, player, owner) & diplomacy::VICTORY != 0);
            let chk = art("chk_ex");
            let lit = hovered(HudAction::SharedVictory) as i32;
            blit(
                b,
                chk,
                column(chk, 4, (!victory) as i32 + 2 * lit),
                at(31, 304, 31, 31),
            );
            text(
                &skin.small,
                b,
                &locale.label("diplomacy", "shvictory"),
                at(67, 313, 199, 14),
                color::GOLD,
                false,
            );
            let button = art("diplomacy_button");
            for (x, y, key, action) in [
                (267, 304, "tribute", HudAction::Tribute),
                (38, 354, "accept", HudAction::Accept),
                (267, 354, "cancel", HudAction::Close),
            ] {
                let r = at(x, y, 189, 27);
                blit(b, button, whole(button), r);
                let down = pressed(action) as i32;
                let c = if hovered(action) {
                    color::GOLD
                } else {
                    color::DARK_GOLD
                };
                text(
                    &skin.font,
                    b,
                    &locale.label("diplomacy", key),
                    Rect {
                        x: r.x + down,
                        y: r.y + down,
                        ..r
                    },
                    c,
                    true,
                );
            }
        }
        Panel::Tribute => {
            // Drawing of the original (0x447d00) over the diplomacy window:
            // the same rows, then the amounts (0x447970) in Arial14B white,
            // 10 pixels from the right of their box (label offset −10,
            // alignment 3 of 0x4dd2c0), and Send / Cancel on the
            // diplomacy's button.
            let back = art("tribute");
            blit(b, back, whole(back), w);
            text(
                &skin.small,
                b,
                &locale.label("tribute", "t_title"),
                at(31, 43, 199, 14),
                color::GOLD,
                false,
            );
            let hovered = |action: HudAction| {
                buttons
                    .iter()
                    .enumerate()
                    .any(|(i, w)| w.action == action && v.state.hover == Some(i))
            };
            for (row, owner) in other_kingdoms(v.mission).into_iter().enumerate() {
                let y = 65 + 32 * row as i32;
                kingdom_row(skin, classic, b, v, w, y, owner, 113);
                for (res, key) in ["res_gold", "res_stone", "res_food", "res_wood"]
                    .into_iter()
                    .enumerate()
                {
                    let r = at(209 + 91 * res as i32, y, 86, 30);
                    let image = art(key);
                    blit(b, image, whole(image), r);
                    let amount = v.state.tribute[owner as usize - 1][res].to_string();
                    let width = skin.font.measure(&amount) as i32;
                    text(
                        &skin.font,
                        b,
                        &amount,
                        Rect {
                            x: r.x + r.w - width - 10,
                            w: width,
                            ..r
                        },
                        color::WHITE,
                        false,
                    );
                }
            }
            let button = art("diplomacy_button");
            for (x, key, action) in [
                (39, "send", HudAction::Send),
                (366, "cancel", HudAction::Close),
            ] {
                let r = at(x, 318, 189, 27);
                blit(b, button, whole(button), r);
                let down = pressed(action) as i32;
                let c = if hovered(action) {
                    color::GOLD
                } else {
                    color::DARK_GOLD
                };
                text(
                    &skin.font,
                    b,
                    &locale.label("tribute", key),
                    Rect {
                        x: r.x + down,
                        y: r.y + down,
                        ..r
                    },
                    c,
                    true,
                );
            }
        }
        Panel::Cheats => cheat_panel::draw(skin, classic, b, v, w, buttons),
        Panel::Chat(large) => {
            let back = art(if large { "chat_big" } else { "chat_small" });
            blit(b, back, whole(back), w);
            text(
                &skin.font,
                b,
                &locale.label("chat", "t_chat"),
                at(38, 32, 420, 18),
                color::GOLD,
                false,
            );
            // The line being written, with its caret.
            text(
                &skin.small,
                b,
                &format!("{}|", v.state.chat.text),
                at(45, 58, 404, 16),
                color::GOLD,
                false,
            );
            for (x, key, to) in [
                (37, "to_all", Recipients::All),
                (93, "to_allies", Recipients::Allies),
                (149, "to_enemies", Recipients::Enemies),
                (425, "to_custom", Recipients::Custom),
            ] {
                let image = art(key);
                let off = v.state.chat.to != to;
                blit(b, image, column(image, 2, off as i32), at(x, 87, 28, 28));
            }
            if large {
                let row_art = art("chat_row");
                let chk = art("chk");
                for (i, owner) in other_kingdoms(v.mission).into_iter().enumerate() {
                    let (x, y) = chat_row(i);
                    blit(
                        b,
                        row_art,
                        whole(row_art),
                        at(x, y, row_art.width as i32, row_art.height as i32),
                    );
                    let on = v.state.chat.custom[owner as usize - 1];
                    blit(b, chk, column(chk, 2, (!on) as i32), at(x, y, 31, 31));
                    if let Some(slot) = v.mission.color_slot(owner) {
                        let [red, green, blue] = classic.player_colors[slot];
                        fill(b, at(x + 38, y + 8, 13, 13), [red, green, blue, 255]);
                    }
                    let name = &v.mission.kingdoms[owner as usize - 1].name;
                    text(
                        &skin.small,
                        b,
                        name,
                        at(x + 66, y + 8, 132, 14),
                        color::DARK_GOLD,
                        false,
                    );
                }
            }
        }
    }
    if panel.has_help() {
        // 0x41cb20: the column is the state among «ND», normal here, and
        // the row (+0x252) red while the mode is on, green otherwise.
        let ball = art("help_ball");
        let [fw, fh] = [ball.width as i32 / 2, ball.height as i32 / 2];
        let row = (v.state.panel != Some(panel) || v.state.help.is_none()) as i32;
        let frame = Rect {
            x: 0,
            y: row * fh,
            w: fw,
            h: fh,
        };
        blit(b, ball, frame, at(0, 0, fw, fh));
    }
}

/// Frame of `RACE.BMP` for a kingdom: European, Arab, Asian, from the
/// civilization most of its objects belong to.
fn kingdom_race(mission: &MissionWorld, owner: u8) -> i32 {
    mission.civilization(owner) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Every panel rectangle against `[LAYOUT800L]` and `[LAYOUT800R]` of the
    /// installed `DATA\LAYOUT.INI`.
    #[test]
    #[ignore = "Requires installed original archives"]
    fn panel_rectangles_match_layout_ini() {
        let root = std::env::var_os("TZAR_DATA_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
        let ini = Ini::parse(
            &GameAssets::open(&root)
                .unwrap()
                .get("DATA/LAYOUT.INI")
                .unwrap(),
        );
        for (section, left) in [("layout800l", true), ("layout800r", false)] {
            let l = Layout {
                width: 800,
                height: 600,
                left,
            };
            let r = l.resources();
            for (key, rect) in [
                ("view", l.play()),
                ("minimap", l.minimap()),
                ("mscr1", l.corner()),
                ("resgold", r[0]),
                ("reswood", r[1]),
                ("resfood", r[2]),
                ("resstone", r[3]),
                ("poplimit", r[4]),
                ("menubutton", l.menu_button()),
                ("diplomacybutton", l.diplomacy_button()),
                ("chatbutton", l.chat_button()),
                ("clock", l.clock()),
            ] {
                let v = ini.numbers(section, key);
                assert_eq!([rect.x, rect.y, rect.w, rect.h], v[..], "{section} {key}");
            }
        }
    }
    #[test]
    fn panel_geometry_matches_the_original_art() {
        for width in [800, 1067, 1920] {
            let left = Layout {
                width,
                height: 600,
                left: true,
            };
            let right = Layout {
                left: false,
                ..left
            };
            // Command cells 2 px inside the painted slots (x 17/79, y 333…).
            assert_eq!(
                left.command(0),
                Rect {
                    x: 19,
                    y: 335,
                    w: 48,
                    h: 48
                }
            );
            assert_eq!(
                left.command(7),
                Rect {
                    x: 81,
                    y: 506,
                    w: 48,
                    h: 48
                }
            );
            assert_eq!(right.command(1).x, 733 + width - 800);
            // The three small buttons sit on the black squares at y 302.
            assert_eq!(
                left.arrows()[2],
                Rect {
                    x: 101,
                    y: 302,
                    w: 25,
                    h: 25
                }
            );
            assert_eq!(right.arrows()[0].x, 675 + width - 800);
            assert_eq!(right.minimap().x, width - 132);
            for layout in [left, right] {
                let play = layout.play();
                for panel in [
                    Panel::Quests,
                    Panel::Diplomacy,
                    Panel::Story,
                    Panel::Chat(true),
                ] {
                    let w = layout.window(panel);
                    assert!(w.x >= play.x && w.x + w.w <= play.x + play.w);
                }
            }
        }
    }
    #[test]
    fn bubbles_take_the_free_place_nearest_their_speaker() {
        let at = |x, y| Rect { x, y, w: 20, h: 30 };
        let mut used = [false; 6];
        // A speaker under the fifth place: the nearest that does not cover
        // it is the sixth, then the fourth (0x4bf9a0).
        assert_eq!(say_slot(&used, Some(at(600, 300))), Some(5));
        used[5] = true;
        assert_eq!(say_slot(&used, Some(at(600, 300))), Some(3));
        // Off screen, or covered by every free place: the first free one.
        used[0] = true;
        assert_eq!(say_slot(&used, None), Some(1));
        let only = [true, true, true, true, false, true];
        assert_eq!(say_slot(&only, Some(at(600, 300))), Some(4));
        assert_eq!(say_slot(&[true; 6], None), None);
        // The 800×600 book in the middle of the screen (0x4c35a3).
        assert_eq!(story_origin(800, 600), [0, 0]);
        assert_eq!(story_origin(1024, 768), [112, 84]);
    }
    #[test]
    fn unit_tables_are_read_by_column_name() {
        let mut stats = HashMap::new();
        let mut costs = HashMap::new();
        parse_table(
            "UNIT\tCLASS\tCOST\tTIME\tSIGHT\tRANGE\tHEALTH\tATTACK\tDEFENCE\n\nArcher\tarcher\t\"40f, 20w\"\t12\t10\t6\t50\t5\t0\nFootman\tfootman\t50f\t12\t5\t\t65\t6\t3\n",
            "euro",
            &mut stats,
            &mut costs,
        );
        parse_table(
            "BUILDING\tCLASS\tCOST\tTIME\tSIGHT\tRESISTANCE\tHEALTH\nHouse\thouse\t75w\t20\t4\t50\t500\n",
            "euro",
            &mut stats,
            &mut costs,
        );
        let archer = stats[&("euro".into(), "archer".into())];
        assert_eq!(
            (archer.attack, archer.defence, archer.ranged),
            (Some(5), Some(0), true)
        );
        assert!(!stats[&("euro".into(), "footman".into())].ranged);
        assert_eq!(stats[&("euro".into(), "house".into())].resistance, Some(50));
        assert_eq!(costs[&("euro".into(), "archer".into())], "\"40f, 20w\"");
        assert_eq!(costs[&("euro".into(), "house".into())], "75w");
    }
}
