//! The settings: one page of this version with five tabs, in place of the
//! original options dialog and of the pages once added to it. Its controls
//! are made of the original's pieces: the stone buttons of the options for
//! tabs and choices, the slider groove with its gold arrows and thumb, the
//! same groove without arrows for switches, fields and lists, the name
//! field's frame. The rows, the drop-down lists and the line that tells what
//! the setting under the pointer does are this version's.
use super::*;

/// The tabs of the settings, by subject.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsTab {
    #[default]
    Audio,
    Game,
    Display,
    Graphics,
    Keys,
}
impl SettingsTab {
    pub const ALL: [Self; 5] = [
        Self::Audio,
        Self::Game,
        Self::Display,
        Self::Graphics,
        Self::Keys,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::Audio => "Audio",
            Self::Game => "Gioco",
            Self::Display => "Schermo",
            Self::Graphics => "Grafica",
            Self::Keys => "Tasti",
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Self::Audio => "ST_TAB_AUDIO",
            Self::Game => "ST_TAB_GAME",
            Self::Display => "ST_TAB_DISPLAY",
            Self::Graphics => "ST_TAB_GRAPHICS",
            Self::Keys => "ST_TAB_KEYS",
        }
    }
    /// The tab by its title or name, as `--screen` takes it.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| {
            t.title().eq_ignore_ascii_case(name) || format!("{t:?}").eq_ignore_ascii_case(name)
        })
    }
}

/// The tabs over the frame, side by side, as the options' stone buttons.
const TABS: Rect = r(75, 28, 680, 27);
const TAB_GAP: i32 = 4;
/// The frame round the rows: the options' group frame, stretched.
const PANEL: Rect = r(75, 62, 680, 432);
/// Where the rows start, the labels stand and the controls begin.
const TOP: i32 = PANEL.y + BEVEL + 10;
const LABEL_X: i32 = PANEL.x + BEVEL + 14;
const CONTROL_X: i32 = 420;
const RIGHT: i32 = PANEL.x + PANEL.w - BEVEL - 12;
const ROW_H: i32 = 30;
const NAME_H: i32 = 38;
const HEADING_H: i32 = 26;
/// A switch: a groove with the gold thumb at one end.
const SWITCH_W: i32 = 72;
/// A slider with its arrows: after the switch's place, its value beside.
const SLIDER_X: i32 = CONTROL_X + SWITCH_W + 10;
const SLIDER_W: i32 = 180;
/// The frames' slider is shorter: their field follows it.
const FPS_FIELD: i32 = 66;
const FPS_SLIDER_W: i32 = RIGHT - FPS_FIELD - 8 - SLIDER_X;
/// A drop-down list: its box and its rows.
const LIST_W: i32 = 214;
const ITEM_H: i32 = 22;
/// The black line and bevel of the original's field frame, round the open
/// list.
const LIST_EDGE: i32 = 6;
/// The line under the frame that tells what the setting under the pointer
/// does, and whether the original has it.
const STRIP: Rect = r(77, 498, 676, 34);
/// The buttons at the bottom.
const BUTTON_W: i32 = 160;
/// «Predefiniti» on its own on the left, «Annulla» and «OK» on the right.
const BUTTONS: [i32; 3] = [76, 422, 595];
/// The keys tab: two columns of two groups.
const KEY_TITLES: [&str; 4] = ["VISUALE", "EVENTI E MAPPA", "FINESTRE", "PARTITA"];
/// The commands of [`crate::keys::ACTIONS`] in each group, in its order.
const KEY_GROUPS: [&[usize]; 4] = [
    &[0, 1, 2, 3, 4, 5, 9],
    &[6, 7, 8, 10, 11, 12, 27],
    &[18, 19, 21, 26, 22, 23, 25],
    &[13, 14, 15, 16, 17, 24, 20],
];
const KEY_ROW: i32 = 24;
/// The width of a column of the keys tab, and of a key's slot.
const KEY_COLUMN: i32 = (RIGHT - LABEL_X - 20) / 2;
const KEY_BOX: i32 = 104;

/// What a row of the settings changes, and how.
enum Control {
    /// On or off.
    Switch(OptionKey, bool),
    /// 0–100 with its value written beside, after the switch that turns
    /// it on, if any (the original's «Volume» boxes); `step` for its arrows.
    Slider {
        key: OptionKey,
        value: u8,
        text: String,
        step: i16,
        switch: Option<(OptionKey, bool)>,
    },
    /// One of a few choices, all in sight.
    Choice(OptionKey, Vec<&'static str>, usize),
    /// One of a list that opens under it.
    Dropdown {
        key: OptionKey,
        items: Vec<String>,
        chosen: Option<usize>,
        text: String,
    },
    /// The frames per second: a slider and the number, written by hand.
    Fps { value: u8, text: String },
    /// The player's name.
    Name,
}
struct Row {
    label: &'static str,
    hint: &'static str,
    enabled: bool,
    control: Control,
}
enum Line {
    Heading(&'static str),
    Row(Row),
    /// A line of information, such as the graphics card.
    Note(String),
}
fn row(label: &'static str, hint: &'static str, control: Control) -> Line {
    Line::Row(Row {
        label,
        hint,
        enabled: true,
        control,
    })
}
/// The five steps of the speeds, as the original's sliders give them.
fn steps(value: u8) -> String {
    format!("{}/5", (value as u32 + 12) / 25 + 1)
}
fn height(line: &Line) -> i32 {
    match line {
        Line::Heading(_) => HEADING_H,
        Line::Note(_) => 38,
        Line::Row(Row {
            control: Control::Name,
            ..
        }) => NAME_H,
        Line::Row(_) => ROW_H,
    }
}

impl Menu {
    /// The rows of the tab open.
    fn settings_lines(&self) -> Vec<Line> {
        use OptionKey as K;
        let s = &self.draft;
        let mut lines = Vec::new();
        match self.tab {
            SettingsTab::Audio => {
                for (label, hint, key, value, switch, on) in [
                    (
                        "Effetti sonori",
                        "ST_SOUND",
                        K::Sound,
                        s.sound,
                        K::SoundOn,
                        s.sound_on,
                    ),
                    (
                        "Musica",
                        "ST_MUSIC",
                        K::Music,
                        s.music,
                        K::MusicOn,
                        s.music_on,
                    ),
                ] {
                    lines.push(row(
                        label,
                        hint,
                        Control::Slider {
                            key,
                            value,
                            text: format!("{value}%"),
                            step: 10,
                            switch: Some((switch, on)),
                        },
                    ));
                }
                for (label, hint, key, on) in [
                    ("Voci delle unità", "ST_SPEECH", K::Speech, s.speech),
                    (
                        "Suoni degli edifici",
                        "ST_BUILDINGS",
                        K::BuildingSounds,
                        s.building_sounds,
                    ),
                    ("Suoni della natura", "ST_NATURE", K::Ambience, s.ambience),
                ] {
                    lines.push(row(label, hint, Control::Switch(key, on)));
                }
                if !self.audio_status.is_empty() {
                    lines.push(Line::Note(self.audio_status.clone()));
                }
            }
            SettingsTab::Game => {
                lines.push(Line::Heading("PARTITA"));
                let speed = s.speed.min(20) * 5;
                lines.push(Line::Row(Row {
                    label: "Velocità del gioco",
                    hint: "ST_GAME_SPEED",
                    // The speed is the open game's.
                    enabled: self.in_game(),
                    control: Control::Slider {
                        key: K::GameSpeed,
                        value: speed,
                        text: steps(speed),
                        step: 25,
                        switch: None,
                    },
                }));
                lines.push(row(
                    "Clima",
                    "ST_WEATHER",
                    Control::Switch(K::Weather, s.weather),
                ));
                lines.push(row(
                    "Giorno e notte",
                    "ST_DAY_NIGHT",
                    Control::Switch(K::DayNight, s.day_night),
                ));
                // Only the settings of an open game offer its cheat panel.
                if self.in_game() {
                    lines.push(row(
                        "Pannello dei trucchi",
                        "ST_CHEATS",
                        Control::Switch(K::Cheats, s.cheats),
                    ));
                }
                lines.push(Line::Heading("MAPPA"));
                lines.push(row(
                    "Velocità di scorrimento",
                    "ST_SCROLL",
                    Control::Slider {
                        key: K::Scroll,
                        value: s.scroll_speed,
                        text: steps(s.scroll_speed),
                        step: 25,
                        switch: None,
                    },
                ));
                lines.push(row(
                    "Scorrimento ai bordi",
                    "ST_EDGE",
                    Control::Switch(K::Edge, s.edge_scroll),
                ));
                lines.push(Line::Heading("GIOCATORE"));
                lines.push(row("Nome sulle proprie unità", "ST_NAME", Control::Name));
                lines.push(row(
                    "Lingua",
                    "ST_LANGUAGE",
                    Control::Dropdown {
                        key: K::Language,
                        items: crate::lang::LANGUAGES
                            .iter()
                            .map(|&n| n.to_owned())
                            .collect(),
                        chosen: Some(s.language),
                        text: crate::lang::LANGUAGES[s.language % crate::lang::LANGUAGES.len()]
                            .into(),
                    },
                ));
                lines.push(row(
                    "Ricorda le missioni raggiunte",
                    "ST_PROGRESS",
                    Control::Switch(K::CampaignProgress, s.campaign_progress),
                ));
            }
            SettingsTab::Display => {
                lines.push(Line::Heading("FINESTRA"));
                lines.push(row(
                    "Schermo intero",
                    "ST_FULLSCREEN",
                    Control::Switch(K::Fullscreen, s.fullscreen),
                ));
                let [w, h] = s.window;
                lines.push(Line::Row(Row {
                    label: "Dimensioni della finestra",
                    hint: "ST_RESOLUTION",
                    enabled: !s.fullscreen,
                    control: Control::Dropdown {
                        key: K::Resolution,
                        items: RESOLUTIONS
                            .iter()
                            .map(|(w, h)| format!("{w} × {h}"))
                            .collect(),
                        chosen: RESOLUTIONS
                            .iter()
                            .position(|&(rw, rh)| [rw, rh] == s.window),
                        text: format!("{w} × {h}"),
                    },
                }));
                lines.push(row(
                    "Sincronizzazione verticale",
                    "ST_VSYNC",
                    Control::Switch(K::Vsync, s.vsync),
                ));
                // The vertical synchronization already follows the screen.
                lines.push(Line::Row(Row {
                    label: "Fotogrammi al secondo",
                    hint: "ST_FPS",
                    enabled: !s.vsync,
                    control: Control::Fps {
                        value: fps_slider(s.max_fps),
                        text: match &self.editing_fps {
                            Some(typed) => format!("{typed}|"),
                            None if s.max_fps == 0 => "nessuno".into(),
                            None => s.max_fps.to_string(),
                        },
                    },
                }));
                lines.push(Line::Heading("INTERFACCIA"));
                lines.push(row(
                    "Scala",
                    "ST_UI_MODE",
                    Control::Choice(
                        K::UiMode,
                        vec!["Pixel nitidi", "Adatta allo schermo"],
                        s.ui_mode,
                    ),
                ));
                lines.push(row(
                    "Pannello dei comandi",
                    "ST_LAYOUT",
                    Control::Choice(K::Layout, vec!["A destra", "A sinistra"], s.layout),
                ));
                lines.push(row(
                    "Puntatore",
                    "ST_CURSOR",
                    Control::Choice(
                        K::SystemCursor,
                        vec!["Del gioco", "Di Windows"],
                        s.system_cursor as usize,
                    ),
                ));
                lines.push(row(
                    "Testi sulla mappa",
                    "ST_MAP_TEXT",
                    Control::Choice(
                        K::MapText,
                        vec!["Originali", "Contorno", "Fondo scuro"],
                        s.map_text,
                    ),
                ));
                lines.push(row(
                    "Griglia di piazzamento",
                    "ST_GRID",
                    Control::Choice(
                        K::Grid,
                        vec!["No", "Leggera", "Media", "Marcata"],
                        self.option_value(K::Grid) as usize,
                    ),
                ));
            }
            SettingsTab::Graphics => {
                lines.push(Line::Heading("IMMAGINE"));
                lines.push(row(
                    "Colori a 24 bit",
                    "ST_TRUE_COLOR",
                    Control::Switch(K::TrueColor, s.true_color),
                ));
                lines.push(row(
                    "Pixel della mappa",
                    "ST_PIXELS",
                    Control::Choice(
                        K::PixelFilter,
                        vec!["Originali", "Regolari", "Levigati"],
                        s.pixel_filter,
                    ),
                ));
                lines.push(row(
                    "Effetto monitor CRT",
                    "ST_CRT",
                    Control::Switch(K::Crt, s.crt),
                ));
                lines.push(row(
                    "Testi in alta definizione",
                    "ST_HD_TEXT",
                    Control::Switch(K::HdText, s.hd_text),
                ));
                lines.push(Line::Heading("MAPPA"));
                for (label, hint, key, on) in [
                    (
                        "Luci notturne",
                        "ST_NIGHT_LIGHTS",
                        K::NightLights,
                        s.night_lights,
                    ),
                    ("Bagliore di fuochi e magie", "ST_BLOOM", K::Bloom, s.bloom),
                    ("Nebbia morbida", "ST_SOFT_FOG", K::SoftFog, s.soft_fog),
                    ("Ambiente vivo", "ST_LIVING", K::LivingWorld, s.living_world),
                    (
                        "Zoom fluido",
                        "ST_SMOOTH_ZOOM",
                        K::SmoothZoom,
                        s.smooth_zoom,
                    ),
                ] {
                    lines.push(row(label, hint, Control::Switch(key, on)));
                }
            }
            SettingsTab::Keys => {}
        }
        lines
    }

    /// The controls of the settings page: tabs, rows, buttons. While a list
    /// is open its rows come first, then a cover that closes it on a click
    /// anywhere else: they are what the pointer finds.
    pub(super) fn settings_controls(&self, c: &mut Controls, locale: &Ini) {
        let lines = if self.tab == SettingsTab::Keys {
            Vec::new()
        } else {
            self.settings_lines()
        };
        if let Some(open) = self.dropdown {
            let mut y = TOP;
            for line in &lines {
                if let Line::Row(Row {
                    control:
                        Control::Dropdown {
                            key, items, chosen, ..
                        },
                    hint,
                    ..
                }) = line
                    && *key == open
                {
                    let panel = list_panel(y, items.len());
                    for (i, item) in items.iter().enumerate() {
                        let w = c.add(
                            r(
                                panel.x + LIST_EDGE,
                                panel.y + LIST_EDGE + 2 + i as i32 * ITEM_H,
                                panel.w - 2 * LIST_EDGE,
                                ITEM_H,
                            ),
                            item.clone(),
                            Action::Set(*key, i as i16),
                            Style::DropItem,
                        );
                        w.checked = Some(*chosen == Some(i));
                        w.hint = Some(hint);
                    }
                }
                y += height(line);
            }
            c.add(
                r(0, 0, 800, 600),
                String::new(),
                Action::CloseDropdown,
                Style::Hidden,
            );
        }
        let width = (TABS.w - TAB_GAP * (SettingsTab::ALL.len() as i32 - 1))
            / SettingsTab::ALL.len() as i32;
        for (i, tab) in SettingsTab::ALL.into_iter().enumerate() {
            let w = c.add(
                r(TABS.x + i as i32 * (width + TAB_GAP), TABS.y, width, TABS.h),
                tab.title().into(),
                Action::Tab(tab),
                Style::Tab,
            );
            w.checked = Some(tab == self.tab);
            w.hint = Some(tab.hint());
        }
        if self.tab == SettingsTab::Keys {
            self.key_controls(c);
        } else {
            let mut y = TOP;
            for line in lines {
                let h = height(&line);
                self.settings_line(c, line, y);
                y += h;
            }
        }
        // Every change works at once: «OK» keeps them, «Annulla» brings
        // back what there was when the page opened.
        for (x, label, action, hint) in [
            (
                BUTTONS[0],
                "Predefiniti".to_owned(),
                Action::Defaults,
                "ST_DEFAULTS",
            ),
            (BUTTONS[1], "Annulla".to_owned(), Action::Back, "ST_CANCEL"),
            (
                BUTTONS[2],
                locale.label("options", "ok"),
                Action::Apply,
                "O_OK",
            ),
        ] {
            let w = c.add(r(x, 539, BUTTON_W, 27), label, action, Style::Plate);
            w.hint = Some(hint);
        }
    }
    /// One line of a tab at `y`.
    fn settings_line(&self, c: &mut Controls, line: Line, y: i32) {
        match line {
            Line::Heading(title) => {
                let w = c.add(
                    r(LABEL_X - 4, y + 6, RIGHT - LABEL_X + 4, 16),
                    title.into(),
                    Action::Inert,
                    Style::Heading,
                );
                w.enabled = false;
            }
            Line::Note(text) => {
                let w = c.add(
                    r(LABEL_X, y + 8, RIGHT - LABEL_X, 30),
                    text,
                    Action::Inert,
                    Style::Note,
                );
                w.enabled = false;
            }
            Line::Row(row) => {
                let height = if matches!(row.control, Control::Name) {
                    NAME_H
                } else {
                    ROW_H
                };
                let first = c.0.len();
                // The label: a click on it turns a switch, writes the name.
                let action = match &row.control {
                    Control::Switch(key, _) => Action::Change(*key, 1),
                    Control::Name => Action::EditName,
                    _ => Action::Inert,
                };
                let clickable = action != Action::Inert && row.enabled;
                let w = c.add(
                    r(LABEL_X - 6, y, CONTROL_X - LABEL_X - 4, height),
                    row.label.into(),
                    action,
                    Style::Row {
                        dim: !row.enabled,
                        band: RIGHT + 12 - LABEL_X,
                    },
                );
                w.enabled = clickable;
                w.hint = Some(row.hint);
                match row.control {
                    Control::Switch(key, on) => {
                        switch(c, key, on, row.enabled, y, row.hint);
                    }
                    Control::Slider {
                        key,
                        value,
                        text,
                        step,
                        switch: toggle,
                    } => {
                        let mut on = row.enabled;
                        if let Some((k, state)) = toggle {
                            switch(c, k, state, row.enabled, y, row.hint);
                            on &= state;
                        }
                        slider(
                            c,
                            r(SLIDER_X, y + 1, SLIDER_W, ARROW),
                            (key, row.label),
                            value,
                            text,
                            step,
                            on,
                            row.hint,
                        );
                    }
                    Control::Fps { value, text } => {
                        slider(
                            c,
                            r(SLIDER_X, y + 1, FPS_SLIDER_W, ARROW),
                            (OptionKey::Frames, row.label),
                            value,
                            text.clone(),
                            1,
                            row.enabled,
                            row.hint,
                        );
                        // Its value is written in the field after it.
                        let n = c.0.len();
                        c.0[n - 3].checked = Some(true);
                        let w = c.add(
                            r(RIGHT - FPS_FIELD, y, FPS_FIELD, ROW_H),
                            text,
                            Action::EditFps,
                            Style::Edit,
                        );
                        w.checked = Some(self.editing_fps.is_some());
                        w.enabled = row.enabled;
                        w.hint = Some(row.hint);
                    }
                    Control::Choice(key, labels, chosen) => {
                        let n = labels.len() as i32;
                        let gap = 2;
                        let width = (RIGHT - CONTROL_X - gap * (n - 1)) / n;
                        for (i, label) in labels.into_iter().enumerate() {
                            let w = c.add(
                                r(CONTROL_X + i as i32 * (width + gap), y + 1, width, 27),
                                label.into(),
                                Action::Set(key, i as i16),
                                Style::Segment,
                            );
                            w.checked = Some(i == chosen);
                            w.enabled = row.enabled;
                            w.hint = Some(row.hint);
                        }
                    }
                    Control::Dropdown { key, text, .. } => {
                        let w = c.add(
                            r(CONTROL_X, y, LIST_W, ROW_H),
                            text,
                            Action::Dropdown(key),
                            Style::Dropdown,
                        );
                        w.checked = Some(self.dropdown == Some(key));
                        w.enabled = row.enabled;
                        w.hint = Some(row.hint);
                    }
                    Control::Name => {
                        let w = c.add(
                            r(CONTROL_X, y + 2, 187, 33),
                            self.draft.player_name.clone(),
                            Action::EditName,
                            Style::Field,
                        );
                        w.checked = Some(self.editing_name);
                        w.hint = Some(row.hint);
                    }
                }
                // The row lights up while the pointer is on any part of it.
                let lit = self.hover.is_some_and(|h| (first..c.0.len()).contains(&h));
                c.0[first].checked = Some(lit);
            }
        }
    }
    /// The keys tab: the commands in four groups, their keys in slots.
    fn key_controls(&self, c: &mut Controls) {
        let half = KEY_COLUMN;
        for column in 0..2 {
            let x = LABEL_X + column * (half + 20);
            let mut y = TOP;
            for group in [column as usize, column as usize + 2] {
                let w = c.add(
                    r(x - 4, y + 6, half, 16),
                    KEY_TITLES[group].into(),
                    Action::Inert,
                    Style::Heading,
                );
                w.enabled = false;
                y += HEADING_H;
                for &i in KEY_GROUPS[group] {
                    let (id, text) = crate::keys::ACTIONS[i];
                    let first = c.0.len();
                    let w = c.add(
                        r(x - 6, y, half - KEY_BOX - 2, KEY_ROW),
                        (*text).into(),
                        Action::BindKey(i),
                        Style::Row {
                            dim: false,
                            band: half + 6,
                        },
                    );
                    w.hint = Some("ST_BIND_KEY");
                    let w = c.add(
                        r(x + half - KEY_BOX, y + 1, KEY_BOX, KEY_ROW - 2),
                        if self.capturing == Some(i) {
                            "Premi un tasto…".into()
                        } else {
                            crate::keys::label(&self.draft.key(id))
                        },
                        Action::BindKey(i),
                        Style::KeyBox,
                    );
                    w.checked = Some(self.capturing == Some(i));
                    w.hint = Some("ST_BIND_KEY");
                    let lit = self.hover.is_some_and(|h| (first..c.0.len()).contains(&h));
                    c.0[first].checked = Some(lit);
                    y += KEY_ROW;
                }
                y += 4;
            }
        }
    }

    /// The index of a choice, as its segments show it.
    pub(super) fn option_value(&self, k: OptionKey) -> i16 {
        let s = &self.draft;
        (match k {
            OptionKey::Language => s.language,
            OptionKey::UiMode => s.ui_mode,
            OptionKey::Layout => s.layout,
            OptionKey::SystemCursor => s.system_cursor as usize,
            OptionKey::MapText => s.map_text,
            OptionKey::PixelFilter => s.pixel_filter,
            OptionKey::Grid if s.placement_grid => s.grid_strength + 1,
            _ => 0,
        }) as i16
    }
    /// «Predefiniti»: the settings of the tab open as they first are. The
    /// player's name, the language and the window's size stay: they are the
    /// player's.
    pub(super) fn settings_defaults(&mut self) {
        let d = Settings::default();
        let s = &mut self.draft;
        match self.tab {
            SettingsTab::Audio => {
                s.sound = d.sound;
                s.music = d.music;
                s.sound_on = d.sound_on;
                s.music_on = d.music_on;
                s.speech = d.speech;
                s.building_sounds = d.building_sounds;
                s.ambience = d.ambience;
            }
            SettingsTab::Game => {
                s.weather = d.weather;
                s.day_night = d.day_night;
                s.scroll_speed = d.scroll_speed;
                s.edge_scroll = d.edge_scroll;
                s.campaign_progress = d.campaign_progress;
            }
            SettingsTab::Display => {
                s.fullscreen = d.fullscreen;
                s.vsync = d.vsync;
                s.max_fps = d.max_fps;
                s.ui_mode = d.ui_mode;
                s.layout = d.layout;
                s.system_cursor = d.system_cursor;
                s.map_text = d.map_text;
                s.placement_grid = d.placement_grid;
                s.grid_strength = d.grid_strength;
            }
            SettingsTab::Graphics => {
                s.true_color = d.true_color;
                s.pixel_filter = d.pixel_filter;
                s.crt = d.crt;
                s.night_lights = d.night_lights;
                s.bloom = d.bloom;
                s.soft_fog = d.soft_fog;
                s.living_world = d.living_world;
                s.smooth_zoom = d.smooth_zoom;
                s.hd_text = d.hd_text;
            }
            SettingsTab::Keys => {
                self.capturing = None;
                s.keys.clear();
            }
        }
    }
}

/// A switch at the start of the controls' column.
fn switch(c: &mut Controls, key: OptionKey, on: bool, enabled: bool, y: i32, hint: &'static str) {
    let w = c.add(
        r(CONTROL_X, y, SWITCH_W, ROW_H),
        if on { "Sì" } else { "No" }.into(),
        Action::Change(key, 1),
        Style::Switch,
    );
    w.checked = Some(on);
    w.enabled = enabled;
    w.hint = Some(hint);
}
/// A slider as the original's: its groove under the track, drawn by it,
/// then the arrows at its ends; `rect` holds the arrows too. The arrows are
/// named after the row's `label`.
#[allow(clippy::too_many_arguments)]
fn slider(
    c: &mut Controls,
    rect: Rect,
    (key, label): (OptionKey, &str),
    value: u8,
    text: String,
    step: i16,
    enabled: bool,
    hint: &'static str,
) {
    let track = c.add(
        r(rect.x + ARROW, rect.y, rect.w - 2 * ARROW, ARROW),
        text,
        Action::Change(key, step),
        Style::Slider,
    );
    track.value = Some(value);
    track.enabled = enabled;
    track.hint = Some(hint);
    for (x, delta, right, sign) in [
        (rect.x, -step, false, "−"),
        (rect.x + rect.w - ARROW, step, true, "+"),
    ] {
        let w = c.add(
            r(x, rect.y, ARROW, ARROW),
            format!("{label} {sign}"),
            Action::Change(key, delta),
            Style::Arrow { right },
        );
        w.enabled = enabled;
        w.hint = Some(hint);
    }
}

/// The frames' slider: 30 to 300 along it, no limit at its end.
pub fn fps_slider(fps: u32) -> u8 {
    if fps == 0 {
        100
    } else {
        (((fps.clamp(30, 300) - 30) * 99 + 135) / 270).min(99) as u8
    }
}
pub fn slider_fps(value: u8) -> u32 {
    if value >= 100 {
        0
    } else {
        30 + (value as u32 * 270 + 49) / 99
    }
}

/// What each setting does, for the line under the frame, and whether it is
/// an addition of this version.
pub(super) const SETTINGS_HINTS: [(&str, &str, bool); 40] = [
    ("ST_TAB_AUDIO", "Volumi, voci e suoni del gioco.", false),
    (
        "ST_TAB_GAME",
        "La partita, lo scorrimento della mappa, il tuo nome e la lingua.",
        false,
    ),
    (
        "ST_TAB_DISPLAY",
        "La finestra del gioco e l'aspetto dell'interfaccia.",
        false,
    ),
    (
        "ST_TAB_GRAPHICS",
        "Gli effetti grafici moderni, tutti facoltativi e spenti all'inizio.",
        true,
    ),
    ("ST_TAB_KEYS", "I tasti dei comandi sulla mappa.", false),
    (
        "ST_SOUND",
        "Effetti, voci e rumori della mappa: l'interruttore li spegne, il cursore ne regola il volume.",
        false,
    ),
    (
        "ST_MUSIC",
        "La musica di menu e missioni: l'interruttore la spegne, il cursore ne regola il volume.",
        false,
    ),
    (
        "ST_SPEECH",
        "Le risposte delle unità quando le selezioni e dai loro ordini.",
        false,
    ),
    (
        "ST_BUILDINGS",
        "Il suono degli edifici quando li selezioni.",
        false,
    ),
    (
        "ST_NATURE",
        "Animali, vento e pioggia intorno alla parte di mappa che guardi.",
        false,
    ),
    (
        "ST_GAME_SPEED",
        "Quanto scorre veloce la partita aperta; si regola solo durante una partita.",
        false,
    ),
    (
        "ST_WEATHER",
        "Pioggia e neve sulla mappa, con il loro effetto sulle unità.",
        false,
    ),
    (
        "ST_DAY_NIGHT",
        "Il passare del giorno e della notte, che cambia luce e visibilità.",
        false,
    ),
    (
        "ST_CHEATS",
        "Il pannello dei trucchi di questa partita (tasto F7).",
        true,
    ),
    (
        "ST_SCROLL",
        "Quanto velocemente si sposta la mappa con frecce e bordi dello schermo.",
        false,
    ),
    (
        "ST_EDGE",
        "La mappa si sposta quando porti il puntatore ai bordi dello schermo.",
        true,
    ),
    ("ST_NAME", "Il nome mostrato sulle tue unità.", true),
    (
        "ST_LANGUAGE",
        "Apri l'elenco e scegli la lingua di testi e voci.",
        true,
    ),
    (
        "ST_PROGRESS",
        "Le missioni delle campagne già raggiunte restano da scegliere; l'originale mostra solo la prima.",
        true,
    ),
    (
        "ST_FULLSCREEN",
        "Il gioco occupa tutto lo schermo, senza bordi; altrimenti è in una finestra (anche Alt+Invio).",
        true,
    ),
    (
        "ST_RESOLUTION",
        "Trascina i bordi della finestra per cambiarne le dimensioni, che restano per la prossima volta; qui ne scegli una pronta.",
        true,
    ),
    (
        "ST_VSYNC",
        "I fotogrammi seguono la frequenza dello schermo, senza strappi nell'immagine; il limite dei fotogrammi allora non serve e si spegne.",
        true,
    ),
    (
        "ST_FPS",
        "I fotogrammi al secondo massimi: trascina la barra o scrivi il numero; in fondo alla barra nessun limite. Spento con la sincronizzazione verticale.",
        true,
    ),
    (
        "ST_UI_MODE",
        "Pixel nitidi: l'interfaccia ingrandita di un numero intero di volte. Adatta: riempie lo schermo.",
        true,
    ),
    (
        "ST_LAYOUT",
        "Da che parte dello schermo sta il pannello dei comandi durante la partita.",
        false,
    ),
    (
        "ST_CURSOR",
        "Il puntatore disegnato dal gioco o quello di Windows, con lo stesso disegno.",
        false,
    ),
    (
        "ST_MAP_TEXT",
        "Come si leggono messaggi e consigli sulla mappa: con l'ombra originale, un contorno nero o un fondo scuro.",
        true,
    ),
    (
        "ST_GRID",
        "Una griglia con le caselle libere e occupate mentre piazzi un edificio, e quanto si vede.",
        true,
    ),
    (
        "ST_TRUE_COLOR",
        "Sfumature lisce di notte, nelle ombre e nella nebbia, senza i 5 bit per colore dell'originale.",
        true,
    ),
    (
        "ST_PIXELS",
        "Pixel originali, regolari a ogni zoom o levigati, con i bordi smussati.",
        true,
    ),
    ("ST_CRT", "Righe e curvatura di un vecchio monitor.", true),
    (
        "ST_HD_TEXT",
        "Testi dell'interfaccia nitidi alla risoluzione dello schermo.",
        true,
    ),
    (
        "ST_NIGHT_LIGHTS",
        "Di notte fuochi, edifici e magie fanno luce intorno.",
        true,
    ),
    (
        "ST_BLOOM",
        "Un alone luminoso intorno a fuochi, magie e fulmini.",
        true,
    ),
    (
        "ST_SOFT_FOG",
        "Bordi sfumati della nebbia invece dei riquadri.",
        true,
    ),
    (
        "ST_LIVING",
        "Alberi che ondeggiano al vento e acqua che luccica.",
        true,
    ),
    (
        "ST_SMOOTH_ZOOM",
        "La rotella ingrandisce scorrendo, con più passi.",
        true,
    ),
    (
        "ST_BIND_KEY",
        "Fai clic e premi il nuovo tasto del comando; Esc annulla.",
        false,
    ),
    (
        "ST_DEFAULTS",
        "Rimette le impostazioni di questa scheda come all'inizio; nome, lingua e dimensioni della finestra restano.",
        false,
    ),
    (
        "ST_CANCEL",
        "Chiude rimettendo tutto com'era all'apertura delle opzioni (anche Esc).",
        false,
    ),
];

/// The marble of the settings: the options' background with its frames and
/// controls covered, the ornament kept, and one frame, the options' own,
/// stretched round the rows.
pub(super) fn settings_page(options: &Bitmap, clean: &Bitmap) -> Result<Bitmap> {
    let mut page = options.clone();
    let cover = r(60, 0, 740, 600);
    let marble = clean.crop(
        cover.x as u32,
        cover.y as u32,
        cover.w as u32,
        cover.h as u32,
    )?;
    blit(&mut page, &marble, whole(&marble), cover);
    // The first frame of the options (75, 70, 350 × 195), stretched: its
    // edges only, the marble within.
    nine(
        &mut page,
        options,
        r(75, 70, 350, 195),
        PANEL,
        BEVEL + 4,
        false,
    );
    Ok(page)
}
/// The options' slider groove without its arrows (`D_OPTIONS`, 222, 82,
/// 184 × 30): its outer edges and a stretch of its channel, for switches,
/// fields, lists and the keys' slots.
pub(super) fn groove_art(options: &Bitmap) -> Result<Bitmap> {
    let mut groove = Bitmap {
        width: 128,
        height: 30,
        rgba: vec![0; 128 * 30 * 4],
    };
    for (from, to) in [
        (r(222, 82, 4, 30), r(0, 0, 4, 30)),
        (r(252, 82, 120, 30), r(4, 0, 120, 30)),
        (r(402, 82, 4, 30), r(124, 0, 4, 30)),
    ] {
        let piece = options.crop(from.x as u32, from.y as u32, from.w as u32, from.h as u32)?;
        blit(&mut groove, &piece, whole(&piece), to);
    }
    Ok(groove)
}
/// `from` of `art` over `to`: its corners as they are, its edges and, with
/// `middle`, its middle repeated.
fn nine(out: &mut Bitmap, art: &Bitmap, from: Rect, to: Rect, cap: i32, middle: bool) {
    let cap = cap
        .min(to.w / 2)
        .min(to.h / 2)
        .min(from.w / 2)
        .min(from.h / 2);
    let tile = |out: &mut Bitmap, src: Rect, dst: Rect| {
        if src.w <= 0 || src.h <= 0 {
            return;
        }
        let mut y = dst.y;
        while y < dst.y + dst.h {
            let h = src.h.min(dst.y + dst.h - y);
            let mut x = dst.x;
            while x < dst.x + dst.w {
                let w = src.w.min(dst.x + dst.w - x);
                blit(out, art, r(src.x, src.y, w, h), r(x, y, w, h));
                x += w;
            }
            y += h;
        }
    };
    let (sx, sw) = (
        [from.x, from.x + cap, from.x + from.w - cap],
        [cap, from.w - 2 * cap, cap],
    );
    let (sy, sh) = (
        [from.y, from.y + cap, from.y + from.h - cap],
        [cap, from.h - 2 * cap, cap],
    );
    let (dx, dw) = (
        [to.x, to.x + cap, to.x + to.w - cap],
        [cap, to.w - 2 * cap, cap],
    );
    let (dy, dh) = (
        [to.y, to.y + cap, to.y + to.h - cap],
        [cap, to.h - 2 * cap, cap],
    );
    for i in 0..3 {
        for j in 0..3 {
            if i == 1 && j == 1 && !middle {
                continue;
            }
            tile(
                out,
                r(sx[j], sy[i], sw[j], sh[i]),
                r(dx[j], dy[i], dw[j], dh[i]),
            );
        }
    }
}

/// The flag beside a language's name, in «Lingua» and in its list.
fn language_flag(w: &Widget) -> Option<&'static Bitmap> {
    let i = match w.action {
        Action::Set(OptionKey::Language, i) => usize::try_from(i).ok()?,
        Action::Dropdown(OptionKey::Language) => {
            crate::lang::LANGUAGES.iter().position(|n| *n == w.label)?
        }
        _ => return None,
    };
    flags::flags().get(i)
}
/// The open list of the drop-down on the row at `y`, `n` items long: the
/// original's field frame right under the box, as wide as it with its
/// arrow; right over it when the frame of the rows has no room below.
fn list_panel(y: i32, n: usize) -> Rect {
    let h = n as i32 * ITEM_H + 2 * (LIST_EDGE + 2);
    let below = y + ROW_H - 1;
    let top = if below + h > PANEL.y + PANEL.h - BEVEL {
        y + 1 - h
    } else {
        below
    };
    r(CONTROL_X + 3, top, LIST_W - 3, h)
}
/// Within a stone button's bevel.
fn inside(rect: Rect) -> Rect {
    r(rect.x + 3, rect.y + 3, rect.w - 6, rect.h - 6)
}
/// A fill of `color` over `rect` at `alpha`, for lights and engravings.
fn tint(out: &mut Bitmap, rect: Rect, color: [u8; 3], alpha: f32) {
    pixels(out, rect, |p| {
        for (v, c) in p[..3].iter_mut().zip(color) {
            *v = (*v as f32 * (1. - alpha) + c as f32 * alpha).round() as u8;
        }
        p[3] = 255;
    });
}

impl MenuSkin {
    /// The options' stone button, raised or pressed, at any width.
    fn plate(&self, out: &mut Bitmap, rect: Rect, pressed: bool) {
        let b = &self.art["D_OPTIONS/BTN"];
        nine(out, b, strip(b, 2, pressed as i32), rect, 6, true);
    }
    /// The groove without arrows, at any size.
    fn groove(&self, out: &mut Bitmap, rect: Rect) {
        nine(
            out,
            &self.art["SETTINGS/GROOVE"],
            r(0, 0, 128, 30),
            rect,
            5,
            true,
        );
    }
    /// The text of a groove, in its channel.
    fn groove_text(&self, out: &mut Bitmap, rect: Rect, text: &str, c: [u8; 3]) {
        write(
            &self.small,
            out,
            text,
            r(rect.x + 6, rect.y + (rect.h - 13) / 2 - 1, rect.w - 12, 14),
            c,
            true,
        );
    }
    /// The controls of the settings page.
    pub(super) fn settings_widget(&self, out: &mut Bitmap, w: &Widget, lit: bool, pressed: bool) {
        let rect = w.rect;
        let on = w.checked == Some(true);
        let text = |enabled: bool, chosen: bool| {
            if !enabled {
                color::DISABLED
            } else if chosen {
                color::CREAM
            } else if lit {
                color::GOLD
            } else {
                color::DARK_GOLD
            }
        };
        match w.style {
            Style::Tab | Style::Plate => {
                let down = on || (w.enabled && pressed);
                self.plate(out, rect, down);
                if on {
                    tint(out, inside(rect), color::GOLD, 0.14);
                }
                let d = down as i32;
                write(
                    &self.font,
                    out,
                    &w.label,
                    r(rect.x + d, rect.y + 6 + d, rect.w, 17),
                    text(w.enabled, on),
                    true,
                );
            }
            Style::Segment => {
                self.plate(out, rect, on || (w.enabled && pressed));
                if on {
                    tint(out, inside(rect), color::GOLD, 0.14);
                }
                let d = on as i32;
                write(
                    &self.small,
                    out,
                    &w.label,
                    r(rect.x + 2 + d, rect.y + 7 + d, rect.w - 4, 14),
                    text(w.enabled, on),
                    true,
                );
                if !w.enabled {
                    darken(out, rect);
                }
            }
            Style::Row { dim, .. } => {
                let c = if dim {
                    color::DISABLED
                } else if on || lit {
                    color::CREAM
                } else {
                    color::GOLD
                };
                write(
                    &self.small,
                    out,
                    &w.label,
                    r(rect.x + 6, rect.y + (rect.h - 13) / 2, rect.w - 6, 14),
                    c,
                    false,
                );
            }
            Style::Heading => {
                write(
                    &self.small,
                    out,
                    &w.label,
                    r(rect.x, rect.y, rect.w, 14),
                    color::GOLD,
                    false,
                );
                // An engraved line: a dark cut with light under it.
                let width = self.small.measure(&crate::lang::tr(&w.label)) as i32 + 8;
                let line = r(rect.x + width, rect.y + 6, rect.w - width, 1);
                tint(out, line, [0, 0, 0], 0.6);
                tint(
                    out,
                    Rect {
                        y: line.y + 1,
                        ..line
                    },
                    color::CREAM,
                    0.14,
                );
            }
            Style::Note => {
                for (i, line) in self
                    .small
                    .wrap(&w.label, rect.w as usize)
                    .iter()
                    .take(2)
                    .enumerate()
                {
                    write(
                        &self.small,
                        out,
                        line,
                        r(rect.x, rect.y + i as i32 * self.line(), rect.w, 14),
                        color::DARK_GOLD,
                        false,
                    );
                }
            }
            Style::Switch => {
                // A groove with the gold thumb at its end: right is on,
                // with «Sì» lit where the thumb left; left is off.
                self.groove(out, rect);
                let thumb = r(
                    if on {
                        rect.x + rect.w - 4 - ARROW
                    } else {
                        rect.x + 4
                    },
                    rect.y + 1,
                    ARROW,
                    ARROW,
                );
                let free = if on {
                    r(rect.x + 4, rect.y + 4, thumb.x - rect.x - 4, rect.h - 11)
                } else {
                    r(
                        thumb.x + ARROW,
                        rect.y + 4,
                        rect.x + rect.w - 4 - thumb.x - ARROW,
                        rect.h - 11,
                    )
                };
                if on && w.enabled {
                    tint(out, free, color::GOLD, 0.16);
                }
                self.groove_text(out, free, &w.label, text(w.enabled, on));
                let b = &self.art["COMMON/THUMB HORZ"];
                blit(out, b, strip(b, 2, (lit && w.enabled) as i32), thumb);
                if !w.enabled {
                    darken(out, rect);
                }
            }
            Style::Slider => {
                // The groove of the original's sliders, its arrows over its
                // ends, the thumb on the channel and the value beside.
                let groove = r(rect.x - ARROW, rect.y - 1, rect.w + 2 * ARROW + 4, 30);
                let art = &self.art["D_OPTIONS"];
                let source = r(222, 82, 184, 30);
                // Ends as they are, the channel repeated.
                let (left, right) = (34, 30);
                blit(
                    out,
                    art,
                    r(source.x, source.y, left, 30),
                    r(groove.x, groove.y, left, 30),
                );
                let mut x = groove.x + left;
                while x < groove.x + groove.w - right {
                    let w = (source.w - left - right).min(groove.x + groove.w - right - x);
                    blit(
                        out,
                        art,
                        r(source.x + left, source.y, w, 30),
                        r(x, groove.y, w, 30),
                    );
                    x += w;
                }
                blit(
                    out,
                    art,
                    r(source.x + source.w - right, source.y, right, 30),
                    r(groove.x + groove.w - right, groove.y, right, 30),
                );
                let v = w.value.unwrap_or(0) as i32;
                let x = rect.x + (rect.w - ARROW) * v / 100;
                if w.enabled {
                    tint(
                        out,
                        r(rect.x + 2, rect.y + 4, (x - rect.x).max(0), rect.h - 11),
                        color::GOLD,
                        0.16,
                    );
                }
                let b = &self.art["COMMON/THUMB HORZ"];
                blit(
                    out,
                    b,
                    strip(b, 2, pressed as i32),
                    r(x, rect.y, ARROW, ARROW),
                );
                if !w.enabled {
                    darken(out, r(x, rect.y, ARROW, ARROW));
                }
                // A value with a field of its own is not written beside.
                if !on {
                    write(
                        &self.small,
                        out,
                        &w.label,
                        r(
                            rect.x + rect.w + ARROW + 10,
                            rect.y + 7,
                            RIGHT - rect.x - rect.w - ARROW - 10,
                            14,
                        ),
                        if w.enabled {
                            if lit { color::CREAM } else { color::GOLD }
                        } else {
                            color::DISABLED
                        },
                        false,
                    );
                }
            }
            Style::Edit => {
                self.groove(out, rect);
                self.groove_text(out, rect, &w.label, text(w.enabled, on || lit));
            }
            Style::Dropdown => {
                // The original's value selector (`COMMON\SCROLL CTRL.BMP`)
                // from its middle line on: the field, then the slot with
                // the down arrow over it, where the selector has its right
                // arrow (0x4c506d); the frame stretched by its channel.
                let b = &self.art["COMMON/SCROLL CTRL"];
                let frame = r(rect.x + 3, rect.y + 1, rect.w - 6, 27);
                let (left, right) = (7, 29);
                blit(out, b, r(26, 3, left, 27), r(frame.x, frame.y, left, 27));
                let end = frame.x + frame.w - right;
                let mut x = frame.x + left;
                while x < end {
                    // A stretch of the channel clear of its corners.
                    let w = 100.min(end - x);
                    blit(out, b, r(45, 3, w, 27), r(x, frame.y, w, 27));
                    x += w;
                }
                blit(out, b, r(159, 3, right, 27), r(end, frame.y, right, 27));
                let arrow = r(rect.x + rect.w - ARROW, frame.y, ARROW, ARROW);
                let indent = match language_flag(w) {
                    Some(flag) => {
                        let y = frame.y + (27 - flag.height as i32) / 2;
                        blit(out, flag, whole(flag), r(rect.x + 10, y, 24, 16));
                        30
                    }
                    None => 0,
                };
                write(
                    &self.small,
                    out,
                    &w.label,
                    r(
                        rect.x + 12 + indent,
                        frame.y + 7,
                        arrow.x - rect.x - 18 - indent,
                        14,
                    ),
                    text(w.enabled, on || lit),
                    false,
                );
                let b = &self.art["COMMON/BTN DOWN"];
                blit(
                    out,
                    b,
                    strip(b, 2, (w.enabled && (on || pressed)) as i32),
                    arrow,
                );
                if !w.enabled {
                    darken(out, rect);
                }
            }
            Style::DropItem => {
                if lit {
                    tint(out, rect, color::GOLD, 0.2);
                }
                let indent = match language_flag(w) {
                    Some(flag) => {
                        let y = rect.y + (rect.h - flag.height as i32) / 2;
                        blit(out, flag, whole(flag), r(rect.x + 4, y, 24, 16));
                        30
                    }
                    None => 0,
                };
                write(
                    &self.small,
                    out,
                    &w.label,
                    r(
                        rect.x + 3 + indent,
                        rect.y + (rect.h - 13) / 2,
                        rect.w - 6 - indent,
                        14,
                    ),
                    if lit || on { color::CREAM } else { color::GOLD },
                    false,
                );
            }
            _ => {}
        }
    }
    /// Under the rows: the light of the row the pointer is on.
    pub(super) fn settings_backdrop(&self, out: &mut Bitmap, widgets: &[Widget]) {
        for w in widgets {
            if let Style::Row { band, .. } = w.style
                && w.checked == Some(true)
            {
                tint(out, Rect { w: band, ..w.rect }, color::GOLD, 0.09);
            }
        }
    }
    /// Over everything: the open list, in a groove of its own.
    pub(super) fn settings_overlay(&self, out: &mut Bitmap, menu: &Menu, widgets: &[Widget]) {
        let items: Vec<(usize, &Widget)> = widgets
            .iter()
            .enumerate()
            .filter(|(_, w)| w.style == Style::DropItem)
            .collect();
        let (Some((_, first)), Some((_, last))) = (items.first(), items.last()) else {
            return;
        };
        let panel = r(
            first.rect.x - LIST_EDGE,
            first.rect.y - LIST_EDGE - 2,
            first.rect.w + 2 * LIST_EDGE,
            last.rect.y + last.rect.h - first.rect.y + 2 * (LIST_EDGE + 2),
        );
        // The rows under it covered with the page's marble, from the
        // controls' edge; a shadow; the marble darkened within, and the
        // frame of the original's fields (`S_SINGLERND\FRAME.BMP`) round.
        let page = &self.art["PAGE_SETTINGS"];
        let cover = r(
            CONTROL_X,
            panel.y,
            panel.x + panel.w + 4 - CONTROL_X,
            panel.h + 4,
        );
        blit(out, page, cover, cover);
        tint(
            out,
            r(panel.x + 4, panel.y + 4, panel.w, panel.h),
            [0, 0, 0],
            0.45,
        );
        blit(out, page, panel, panel);
        tint(
            out,
            r(
                panel.x + LIST_EDGE,
                panel.y + LIST_EDGE,
                panel.w - 2 * LIST_EDGE,
                panel.h - 2 * LIST_EDGE,
            ),
            [0, 0, 0],
            0.45,
        );
        nine(
            out,
            &self.art["S_SINGLERND/FRAME"],
            r(3, 3, 181, 27),
            panel,
            LIST_EDGE,
            false,
        );
        for (i, w) in items {
            let lit = menu.hover == Some(i);
            self.settings_widget(out, w, lit, false);
        }
    }
    /// The line under the frame: what the setting under the pointer does,
    /// and whether the original has it; else what the tab holds.
    pub(super) fn settings_strip(&self, out: &mut Bitmap, menu: &Menu, widgets: &[Widget]) {
        let hovered = menu
            .hover
            .filter(|_| !menu.keyboard_focus)
            .or(menu.keyboard_focus.then_some(menu.focus))
            .and_then(|i| widgets.get(i))
            .and_then(|w| menu.hint_key(w));
        let key = hovered.unwrap_or(menu.tab.hint());
        let text = if menu.capturing.is_some() {
            "Premi il nuovo tasto; Esc annulla.".to_owned()
        } else if menu.editing_fps.is_some() {
            "Scrivi il numero e premi Invio; 0 toglie il limite, Esc annulla.".to_owned()
        } else {
            self.hint_text(key)
        };
        let added = SETTINGS_HINTS
            .iter()
            .any(|(k, _, added)| *k == key && *added);
        write(
            &self.small,
            out,
            &text,
            r(STRIP.x, STRIP.y, STRIP.w, 14),
            color::GOLD,
            false,
        );
        if added {
            write(
                &self.small,
                out,
                "Aggiunta di questa versione, assente nell'originale.",
                r(STRIP.x, STRIP.y + self.line() + 2, STRIP.w, 14),
                color::DARK_GOLD,
                false,
            );
        }
    }
}
