//! The cheat panel (`TRUCCHI`), this version's window for the original's
//! developer commands (`gameplay::cheats`): five pages of controls on the
//! tribute window's art, each control with its hint on the line at the
//! bottom of the map. It opens from a button of the command grid shown
//! while nothing is selected, once the game's options switch it on.

use super::*;
use crate::gameplay::cheats::{self as commands, Kind};
use crate::lang::{fill, t};

/// The pages of the panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CheatTab {
    #[default]
    Game,
    Objects,
    Units,
    Buildings,
    Diplomacy,
}
/// Each page with its tab's words and hint.
pub const TABS: [(CheatTab, &str, &str); 5] = [
    (
        CheatTab::Game,
        "Partita",
        "Risorse, popolazione, costruzione, nebbia, mappa, ora del giorno, tecnologie e missione",
    ),
    (
        CheatTab::Objects,
        "Oggetti",
        "Trucchi da applicare a un'unità o a un edificio cliccandolo sulla mappa",
    ),
    (
        CheatTab::Units,
        "Unità",
        "Crea un'unità di qualunque regno nel punto della mappa che scegli",
    ),
    (
        CheatTab::Buildings,
        "Edifici",
        "Crea un edificio già costruito di qualunque regno nel punto della mappa che scegli",
    ),
    (
        CheatTab::Diplomacy,
        "Diplomazia",
        "Cambia a piacere i rapporti fra due regni",
    ),
];
/// What a click on an object of the map does after a button of the
/// «Oggetti» page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    God,
    Usurp,
    Spy,
    Kingdom,
}
/// A control of the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheatControl {
    Tab(CheatTab),
    /// «dvaleva» once, or five times («petleva»).
    Resources(u8),
    NoPop,
    InstantBuild,
    NoTech,
    NoFog,
    Reveal,
    /// The hour back (−1) or on (+1).
    Hour(i8),
    /// The research shown, back or on.
    Research(i8),
    Discover,
    Hunt,
    Next,
    /// The panel closes and waits for the object to click.
    Point(Target),
    /// The kingdom «Dai al regno» gives to, back or on.
    Kingdom(i8),
    /// The civilization of what the pages of units and buildings create.
    Civilization(u8),
    /// The kingdom it goes to, back or on.
    Owner(i8),
    /// The panel closes and waits for the place of an object of the class.
    Spawn(usize),
    /// The two kingdoms of the diplomacy page, back or on.
    From(i8),
    To(i8),
    /// A bit of the first kingdom's word toward the second, switched.
    Bit(u8),
}
/// The choices of the panel, kept while the game is open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CheatChoices {
    pub tab: CheatTab,
    /// «Dai al regno»: an index of [`transfer_kingdoms`].
    pub kingdom: usize,
    /// The diplomacy page's two kingdoms, indices of the players.
    pub from: usize,
    pub to: usize,
    /// What the pages of units and buildings create: the civilization, 0
    /// for the player's, and the kingdom, 0 for the player.
    pub civ: u8,
    pub owner: u8,
    /// «discover»: the researches left, read when the panel opens, and
    /// the one shown.
    pub researches: Vec<String>,
    pub research: usize,
}

/// The kingdoms «Dai al regno» offers: none, the players, Nature (10).
pub fn transfer_kingdoms(mission: &MissionWorld) -> Vec<u8> {
    std::iter::once(0)
        .chain(1..=mission.kingdoms.len().min(8) as u8)
        .chain([crate::gameplay::DISCORD_OWNER])
        .collect()
}
/// The kingdoms the pages of units and buildings create for: the players
/// and Nature.
pub fn spawn_kingdoms(mission: &MissionWorld) -> Vec<u8> {
    (1..=mission.kingdoms.len().min(8) as u8)
        .chain([crate::gameplay::DISCORD_OWNER])
        .collect()
}
/// A kingdom's name in the panel.
pub fn kingdom_name(mission: &MissionWorld, locale: &Ini, k: u8) -> String {
    match k {
        0 => "Nessun regno".into(),
        crate::gameplay::DISCORD_OWNER => locale.label("misc", "nature3"),
        k => mission
            .kingdoms
            .get(k as usize - 1)
            .map_or_else(|| fill("Regno {}", &[&k]), |r| r.name.clone()),
    }
}
/// Civilization names of the setup screens (`race_1`–`race_3`).
pub const CIVILIZATIONS: [&str; 3] = ["Europea", "Araba", "Asiatica"];
/// The civilization the pages of units and buildings create: the one
/// chosen, else the player's.
pub fn civilization(state: &HudState, mission: &MissionWorld) -> u8 {
    if (1..=3).contains(&state.cheat.civ) {
        return state.cheat.civ;
    }
    mission
        .kingdoms
        .get(usize::from(mission.player()).wrapping_sub(1))
        .map_or(1, |k| k.civilization.clamp(1, 3))
}
/// The kingdom the pages of units and buildings create for: the one
/// chosen, else the player.
pub fn owner(state: &HudState, mission: &MissionWorld) -> u8 {
    let kingdoms = spawn_kingdoms(mission);
    if kingdoms.contains(&state.cheat.owner) {
        state.cheat.owner
    } else {
        mission.player()
    }
}
/// The diplomacy page's two kingdoms.
pub fn relation_pair(state: &HudState, mission: &MissionWorld) -> (u8, u8) {
    let players = mission.kingdoms.len().clamp(1, 8);
    (
        (state.cheat.from % players) as u8 + 1,
        (state.cheat.to % players) as u8 + 1,
    )
}
/// The bits of a kingdom's word toward another that the diplomacy page
/// switches: the attitudes and sharings of `.diplomacy`, and the three
/// wishes the merge reads (0x467ef0). In the hints `{a}` is the first
/// kingdom and `{b}` the second.
pub const RELATION_BITS: [(u8, &str, &str); 10] = [
    (6, "Alleato", "{a} considera {b} un alleato: non lo attacca"),
    (5, "Neutrale", "{a} considera {b} neutrale: non lo attacca"),
    (4, "Nemico", "{a} considera {b} un nemico e lo attacca"),
    (
        0,
        "Condivide la vista",
        "{b} vede ciò che vedono le unità di {a}",
    ),
    (
        1,
        "Condivide le risorse",
        "{b} può spendere le risorse di {a}",
    ),
    (2, "Condivide le unità", "{b} può comandare le unità di {a}"),
    (3, "Vittoria condivisa", "{a} vince insieme a {b}"),
    (
        28,
        "Vuole la guerra",
        "{a} vuole la guerra con {b}: basta che uno dei due la voglia e diventano nemici",
    ),
    (
        29,
        "Vuole la pace",
        "{a} vuole la pace con {b}: quando la vogliono tutti e due diventano neutrali",
    ),
    (
        30,
        "Vuole l'alleanza",
        "{a} vuole l'alleanza con {b}: quando la vogliono tutti e due diventano alleati",
    ),
];

/// The help entries whose name is not the object's (`HELP\ITEMS.TXT`), by
/// class and civilization (0 for any).
const HELP_NAMES: [(&str, u8, &str); 9] = [
    ("longbowman", 0, "u_e_longbowman"),
    ("bat", 0, "u_c_bat"),
    ("carpet", 0, "u_c_carpet"),
    ("tcguild", 1, "b_e_merchants_guild"),
    ("tcguild", 2, "b_r_merchants_guild"),
    ("tcguild", 3, "b_a_merchants_guild"),
    ("mosque", 0, "b_r_mosque"),
    ("church", 3, "b_a_monastery"),
    ("shaolinn", 0, "b_a_shao_linn"),
];
/// The original's help on a class (`HELP\ITEMS.TXT`, the encyclopedia of
/// its help screens): the object's name, what it is, and its figures
/// without the price, which the panel does not ask.
pub fn about(
    skin: &HudSkin,
    library: &ObjectLibrary,
    class: usize,
    civ: u8,
) -> (String, String, String) {
    let probe = crate::gameplay::ai::tree::probe(class, civ);
    let name = library.class_label(&probe);
    let kind = if probe.building() { "b" } else { "u" };
    let letter = ["e", "r", "a"][usize::from(civ.clamp(1, 3)) - 1];
    let section = HELP_NAMES
        .iter()
        .find(|(c, k, _)| *c == CLASSES[class] && (*k == 0 || *k == civ))
        .map(|(_, _, s)| (*s).to_owned())
        .or_else(|| {
            skin.help
                .section_names()
                .filter(|s| {
                    s.starts_with(&format!("{kind}_{letter}_"))
                        || s.starts_with(&format!("{kind}_c_"))
                })
                .find(|s| {
                    skin.help
                        .get(s, "brief")
                        .is_some_and(|b| b.trim().eq_ignore_ascii_case(name.trim()))
                })
                .map(str::to_owned)
        });
    let detail = section
        .and_then(|s| skin.help.get(&s, "detail"))
        .unwrap_or_default();
    // «Costo: …. Portata visiva: …. Salute: … ~ What it is.»; a few
    // entries have no «~» and go on after the figures.
    // The other languages write them as the American release: «Cost-…,
    // Sight-…, Health-… ~ …», in the language the texts are shown in.
    let shown = crate::lang::texts();
    let (field, value, cost) = if shown == crate::lang::Language::Italian {
        (". ", ':', "Costo")
    } else {
        (", ", '-', crate::lang::t_in(shown, "Costo"))
    };
    let (figures, what) = match detail.split_once('~') {
        Some((f, w)) => (f.to_owned(), w.trim().to_owned()),
        None => {
            let sentences: Vec<&str> = detail.split(field).collect();
            let n = sentences
                .iter()
                .take_while(|s| s.contains(value) && s.len() < 40)
                .count();
            (sentences[..n].join(field), sentences[n..].join(field))
        }
    };
    let figures = figures
        .split(field)
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with(cost))
        .collect::<Vec<_>>()
        .join(" · ")
        .trim_end_matches('.')
        .to_owned();
    (name, what, figures)
}

/// One thing of the panel, relative to its window: a control, or words.
pub struct Item {
    pub rect: Rect,
    pub look: Look,
    pub control: Option<CheatControl>,
    pub enabled: bool,
    pub hint: String,
}
/// How an [`Item`] is drawn.
pub enum Look {
    /// A tab, and whether its page is shown.
    Tab(&'static str, bool),
    Button(String),
    /// A box ticked or not, with its words.
    Check(bool, String),
    /// The arrow to the left or (true) to the right of a choice.
    Arrow(bool),
    /// A class's icon, of a civilization, in a kingdom's colours.
    Icon(usize, u8, u8),
    /// A row's name.
    Label(String),
    /// A choice's value, between its arrows.
    Value(String),
    /// A note, wrapped in its box.
    Note(String),
}
/// What puts an [`Item`]: its place, look, control, whether it acts, hint.
type Put<'a> = dyn FnMut(Rect, Look, Option<CheatControl>, bool, &str) + 'a;
fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect { x, y, w, h }
}
/// The tabs' row.
const TAB_Y: i32 = 50;
/// Top of a page under the tabs.
const PAGE_Y: i32 = 86;
/// The rows of the «Partita» page.
const ROW: i32 = 21;
/// Where a row's control starts and how wide it is.
const CONTROL_X: i32 = 300;
const CONTROL_W: i32 = 264;
/// The icons of the pages of units and buildings: side, step, columns,
/// top.
const ICON: i32 = 44;
const ICON_STEP: i32 = 48;
const ICON_COLUMNS: usize = 11;
const ICONS_Y: i32 = 116;
/// The box under the icons with the one under the pointer.
const ABOUT_Y: i32 = 258;

/// The panel's items for the page shown.
pub fn items(
    skin: &HudSkin,
    locale: &Ini,
    state: &HudState,
    mission: &MissionWorld,
    library: &ObjectLibrary,
) -> Vec<Item> {
    let mut out = Vec::new();
    let mut put = |rect, look, control: Option<CheatControl>, enabled, hint: &str| {
        out.push(Item {
            rect,
            look,
            control,
            enabled,
            hint: hint.to_owned(),
        })
    };
    let tab = state.cheat.tab;
    for (k, &(t, words, hint)) in TABS.iter().enumerate() {
        put(
            r(32 + k as i32 * 108, TAB_Y, 104, 26),
            Look::Tab(words, t == tab),
            Some(CheatControl::Tab(t)),
            true,
            hint,
        );
    }
    let pertinent = |kind| commands::pertinent(mission, library, kind);
    let c = &state.cheat;
    match tab {
        CheatTab::Game => {
            let row = |n: i32| PAGE_Y + n * ROW;
            let label = |put: &mut Put, n: i32, words: &str, hint: &str| {
                put(
                    r(40, row(n), 250, 20),
                    Look::Label(words.into()),
                    None,
                    true,
                    hint,
                );
            };
            let resources = pertinent(Kind::Resources);
            let why = if resources {
                ""
            } else {
                t(" (In questa partita le risorse sono illimitate.)")
            };
            label(
                &mut put,
                0,
                "Risorse",
                "Aggiunge oro, pietra, cibo e legna al tuo regno",
            );
            put(
                r(CONTROL_X, row(0), 128, 20),
                Look::Button("+10 000".into()),
                Some(CheatControl::Resources(1)),
                resources,
                &format!(
                    "{}{why}",
                    t("Aggiunge 10 000 di oro, pietra, cibo e legna al tuo regno.")
                ),
            );
            put(
                r(CONTROL_X + 136, row(0), 128, 20),
                Look::Button("+50 000".into()),
                Some(CheatControl::Resources(5)),
                resources,
                &format!(
                    "{}{why}",
                    t("Aggiunge 50 000 di oro, pietra, cibo e legna al tuo regno.")
                ),
            );
            let nopop = pertinent(Kind::NoPop);
            label(
                &mut put,
                1,
                "Popolazione massima",
                "Il limite della popolazione del tuo regno",
            );
            put(
                r(CONTROL_X, row(1), CONTROL_W, 20),
                Look::Button(if nopop { "Porta a 1000" } else { "Già a 1000" }.into()),
                Some(CheatControl::NoPop),
                nopop,
                "Porta a 1000 il limite della popolazione del tuo regno, senza bisogno di case",
            );
            let instant = mission.cheats.instant_build;
            label(
                &mut put,
                2,
                "Costruzione immediata",
                "Cantieri, unità e ricerche si completano subito",
            );
            put(
                r(CONTROL_X, row(2), CONTROL_W, 20),
                Look::Check(instant, if instant { "attiva" } else { "spenta" }.into()),
                Some(CheatControl::InstantBuild),
                true,
                "Quando è attiva, ogni cantiere si completa al primo colpo di un contadino e le unità e le ricerche escono subito, per tutti i regni. Clicca per attivarla o spegnerla",
            );
            let free = mission.cheats.no_tech;
            label(
                &mut put,
                3,
                "Nessun requisito",
                "Costruire, creare e ricercare senza i requisiti",
            );
            put(
                r(CONTROL_X, row(3), CONTROL_W, 20),
                Look::Check(free, if free { "attivo" } else { "spento" }.into()),
                Some(CheatControl::NoTech),
                true,
                "Quando è attivo, si possono costruire edifici, creare unità e fare ricerche senza gli edifici e le tecnologie che servirebbero, per tutti i regni. Clicca per attivarlo o spegnerlo",
            );
            let fog = pertinent(Kind::NoFog);
            label(
                &mut put,
                4,
                "Nebbia di guerra",
                "La nebbia che nasconde ciò che le tue unità non vedono",
            );
            put(
                r(CONTROL_X, row(4), CONTROL_W, 20),
                Look::Button(if fog { "Togli" } else { "Già tolta" }.into()),
                Some(CheatControl::NoFog),
                fog,
                "Toglie la nebbia di guerra per il resto della partita: vedi tutto ciò che si muove sulla mappa",
            );
            let reveal = pertinent(Kind::Reveal);
            label(
                &mut put,
                5,
                "Mappa",
                "Le zone della mappa non ancora esplorate",
            );
            put(
                r(CONTROL_X, row(5), CONTROL_W, 20),
                Look::Button(
                    if reveal {
                        "Rivela tutta"
                    } else {
                        "Già esplorata"
                    }
                    .into(),
                ),
                Some(CheatControl::Reveal),
                reveal,
                "Rende esplorata tutta la mappa: vedi il terreno e gli edifici, ma le unità solo dove arriva la vista delle tue",
            );
            label(
                &mut put,
                6,
                "Ora del giorno",
                "L'ora dell'orologio della partita, che decide la luce del giorno e della notte",
            );
            let minutes = mission.simulation.as_ref().map_or(0, |s| s.clock_minutes());
            chooser(
                &mut put,
                r(CONTROL_X, row(6), CONTROL_W, 20),
                format!("{:02}:{:02}", minutes / 60, minutes % 60),
                CheatControl::Hour(-1),
                CheatControl::Hour(1),
                true,
                [
                    "Un'ora indietro (con Maiusc dieci)",
                    "Un'ora avanti (con Maiusc dieci)",
                ],
                "Cambia l'ora del giorno con le frecce",
            );
            label(
                &mut put,
                7,
                "Tecnologia",
                "Una ricerca del tuo regno, completata senza aspettare né pagare",
            );
            let research = c.researches.get(c.research);
            let any = research.is_some();
            let shown = research.map_or_else(
                || t("nessuna da scoprire").to_owned(),
                |r| locale.label("orders_names", r),
            );
            let what = research
                .and_then(|r| locale.get("orders_add", r))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map_or_else(|| shown.clone(), |d| format!("{shown}: {d}"));
            chooser(
                &mut put,
                r(CONTROL_X, row(7), CONTROL_W - 92, 20),
                shown,
                CheatControl::Research(-1),
                CheatControl::Research(1),
                any,
                ["La tecnologia precedente", "La tecnologia seguente"],
                &what,
            );
            put(
                r(CONTROL_X + CONTROL_W - 86, row(7), 86, 20),
                Look::Button("Scopri".into()),
                Some(CheatControl::Discover),
                any,
                &fill("Completa subito per il tuo regno: {}", &[&what]),
            );
            let hunt = pertinent(Kind::Hunt);
            label(
                &mut put,
                8,
                "Animali e mostri",
                "Le unità della Natura: animali selvatici e mostri",
            );
            put(
                r(CONTROL_X, row(8), CONTROL_W, 20),
                Look::Button(
                    if hunt {
                        "Togli dalla mappa"
                    } else {
                        "Nessuno sulla mappa"
                    }
                    .into(),
                ),
                Some(CheatControl::Hunt),
                hunt,
                "Fa sparire dalla mappa tutte le unità della Natura: animali selvatici e mostri",
            );
            let next = pertinent(Kind::Next);
            label(&mut put, 9, "Missione", "La missione di questa campagna");
            put(
                r(CONTROL_X, row(9), CONTROL_W, 20),
                Look::Button(
                    if next {
                        "Passa alla successiva"
                    } else if mission.campaign {
                        "Nessuna successiva"
                    } else {
                        "Solo nelle campagne"
                    }
                    .into(),
                ),
                Some(CheatControl::Next),
                next,
                "Lascia questa missione e apre subito la mappa successiva della campagna",
            );
        }
        CheatTab::Objects => {
            put(
                r(40, PAGE_Y, 524, 30),
                Look::Note(
                    "Scegli un trucco: la finestra si chiude e clicchi sulla mappa l'unità o l'edificio a cui applicarlo. Tieni premuto Maiusc per cliccarne altri, tasto destro per finire."
                        .into(),
                ),
                None,
                true,
                "",
            );
            let rows = [
                (
                    Target::God,
                    "Invulnerabile",
                    "Non subisce più danni. Un altro clic lo rende di nuovo vulnerabile.",
                    "Rende invulnerabile l'oggetto che clicchi: non subisce più danni. Cliccarlo di nuovo lo rende di nuovo vulnerabile",
                ),
                (
                    Target::Usurp,
                    "Prendi il comando",
                    "Gli dai ordini anche se è di un altro regno. Un altro clic lo restituisce.",
                    "Puoi dare ordini all'oggetto che clicchi anche se è di un altro regno. Cliccarlo di nuovo lo restituisce",
                ),
                (
                    Target::Spy,
                    "Guarda con i suoi occhi",
                    "Vedi ciò che vede, anche se è di un altro regno. Un altro clic smette.",
                    "Vedi ciò che vede l'oggetto che clicchi, anche se è di un altro regno. Cliccarlo di nuovo smette",
                ),
            ];
            for (n, (target, words, note, hint)) in rows.into_iter().enumerate() {
                let y = 126 + n as i32 * 40;
                put(
                    r(40, y, 200, 26),
                    Look::Button(words.into()),
                    Some(CheatControl::Point(target)),
                    true,
                    hint,
                );
                put(
                    r(252, y - 1, 312, 30),
                    Look::Note(note.into()),
                    None,
                    true,
                    "",
                );
            }
            let y = 126 + 3 * 40;
            let kingdoms = transfer_kingdoms(mission);
            let k = kingdoms[c.kingdom % kingdoms.len()];
            let to = kingdom_name(mission, locale, k);
            put(
                r(40, y, 200, 26),
                Look::Button("Dai al regno".into()),
                Some(CheatControl::Point(Target::Kingdom)),
                true,
                &fill("L'oggetto che clicchi passa al regno scelto: {}", &[&to]),
            );
            chooser(
                &mut put,
                r(252, y + 3, 312, 20),
                to,
                CheatControl::Kingdom(-1),
                CheatControl::Kingdom(1),
                true,
                ["Il regno precedente", "Il regno seguente"],
                "Il regno a cui passa l'oggetto che clicchi",
            );
        }
        CheatTab::Units | CheatTab::Buildings => {
            let civ = civilization(state, mission);
            for (k, words) in CIVILIZATIONS.iter().enumerate() {
                put(
                    r(40 + k as i32 * 84, PAGE_Y, 80, 24),
                    Look::Tab(words, usize::from(civ) == k + 1),
                    Some(CheatControl::Civilization(k as u8 + 1)),
                    true,
                    &fill(
                        "Crea le unità e gli edifici della civiltà {}",
                        &[&crate::lang::tr(&words.to_lowercase())],
                    ),
                );
            }
            let owner = owner(state, mission);
            put(
                r(300, PAGE_Y + 2, 52, 20),
                Look::Label("Regno".into()),
                None,
                true,
                "Il regno a cui va quello che crei",
            );
            chooser(
                &mut put,
                r(352, PAGE_Y + 2, 212, 20),
                kingdom_name(mission, locale, owner),
                CheatControl::Owner(-1),
                CheatControl::Owner(1),
                true,
                ["Il regno precedente", "Il regno seguente"],
                "Il regno a cui va quello che crei",
            );
            let buildings = tab == CheatTab::Buildings;
            let classes: Vec<usize> = commands::spawn_classes(library, civ)
                .into_iter()
                .filter(|&k| crate::gameplay::ai::tree::probe(k, civ).building() == buildings)
                .collect();
            for (n, &class) in classes.iter().enumerate() {
                let (name, what, _) = about(skin, library, class, civ);
                let hint = if what.is_empty() {
                    name
                } else {
                    format!("{name}: {what}")
                };
                put(
                    r(
                        36 + (n % ICON_COLUMNS) as i32 * ICON_STEP,
                        ICONS_Y + (n / ICON_COLUMNS) as i32 * ICON_STEP,
                        ICON,
                        ICON,
                    ),
                    Look::Icon(class, civ, owner),
                    Some(CheatControl::Spawn(class)),
                    true,
                    &hint,
                );
            }
        }
        CheatTab::Diplomacy => {
            let players = mission.kingdoms.len().clamp(1, 8);
            let (from, to) = relation_pair(state, mission);
            let name = |k| kingdom_name(mission, locale, k);
            put(
                r(40, PAGE_Y + 2, 52, 20),
                Look::Label("Regno".into()),
                None,
                true,
                "Il regno di cui cambi i rapporti",
            );
            chooser(
                &mut put,
                r(92, PAGE_Y + 2, 200, 20),
                name(from),
                CheatControl::From(-1),
                CheatControl::From(1),
                players > 1,
                ["Il regno precedente", "Il regno seguente"],
                "Il regno di cui cambi i rapporti",
            );
            put(
                r(306, PAGE_Y + 2, 46, 20),
                Look::Label("verso".into()),
                None,
                true,
                "Il regno verso cui cambiano",
            );
            chooser(
                &mut put,
                r(352, PAGE_Y + 2, 212, 20),
                name(to),
                CheatControl::To(-1),
                CheatControl::To(1),
                players > 1,
                ["Il regno precedente", "Il regno seguente"],
                "Il regno verso cui cambiano",
            );
            let valid = players > 1 && from != to;
            put(
                r(40, PAGE_Y + 30, 524, 16),
                Look::Note(if valid {
                    fill(
                        "Come {} considera {}: clicca una casella per accenderla o spegnerla.",
                        &[&name(from), &name(to)],
                    )
                } else {
                    "Scegli due regni diversi.".into()
                }),
                None,
                true,
                "",
            );
            let word = diplomacy_word(&mission.diplomacy, from, to);
            for (n, &(bit, words, hint)) in RELATION_BITS.iter().enumerate() {
                let on = word & (1 << bit) != 0;
                put(
                    r(
                        40 + (n / 5) as i32 * 268,
                        PAGE_Y + 54 + (n % 5) as i32 * 30,
                        256,
                        22,
                    ),
                    Look::Check(on, words.into()),
                    Some(CheatControl::Bit(bit)),
                    valid,
                    &fill(
                        "{}. Ora: {}",
                        &[
                            &t(hint)
                                .replace("{a}", &name(from))
                                .replace("{b}", &name(to)),
                            &t(if on { "sì" } else { "no" }),
                        ],
                    ),
                );
            }
        }
    }
    out
}
/// A choice with its two arrows: the value between them shows its hint.
#[allow(clippy::too_many_arguments)]
fn chooser(
    put: &mut Put,
    at: Rect,
    value: String,
    back: CheatControl,
    on: CheatControl,
    enabled: bool,
    hints: [&str; 2],
    hint: &str,
) {
    put(
        r(at.x, at.y, 20, at.h),
        Look::Arrow(false),
        Some(back),
        enabled,
        hints[0],
    );
    put(
        r(at.x + 22, at.y, at.w - 44, at.h),
        Look::Value(value),
        None,
        enabled,
        hint,
    );
    put(
        r(at.x + at.w - 20, at.y, 20, at.h),
        Look::Arrow(true),
        Some(on),
        enabled,
        hints[1],
    );
}

/// The original's button slot (`D_DIPLOMACY\BTN`) stretched to `at`, its
/// four-pixel edges kept.
fn slot(b: &mut Bitmap, art: &Bitmap, at: Rect) {
    let e = 4;
    let (w, h) = (art.width as i32, art.height as i32);
    let xs = [
        (0, e, at.x, e),
        (e, w - 2 * e, at.x + e, at.w - 2 * e),
        (w - e, e, at.x + at.w - e, e),
    ];
    let ys = [
        (0, e, at.y, e),
        (e, h - 2 * e, at.y + e, at.h - 2 * e),
        (h - e, e, at.y + at.h - e, e),
    ];
    for &(sx, sw, dx, dw) in &xs {
        for &(sy, sh, dy, dh) in &ys {
            if dw > 0 && dh > 0 {
                blit(b, art, r(sx, sy, sw, sh), r(dx, dy, dw, dh));
            }
        }
    }
}

/// The panel on the tribute window's art: its title, the page's items,
/// what the pointer shows on the pages of units and buildings, whether the
/// game counts the player a cheater (0x5cf818, «ischeater»), and Close.
pub(super) fn draw(
    skin: &MenuSkin,
    classic: &HudSkin,
    b: &mut Bitmap,
    v: &HudView,
    w: Rect,
    buttons: &[HudButton],
) {
    let at = |rect: Rect| Rect {
        x: w.x + rect.x,
        y: w.y + rect.y,
        ..rect
    };
    let back = &classic.windows["tribute"];
    blit(b, back, whole(back), w);
    text(
        &skin.font,
        b,
        "TRUCCHI",
        at(r(22, 24, 556, 18)),
        color::GOLD,
        true,
    );
    let hovered = |action: HudAction| {
        v.state
            .hover
            .and_then(|i| buttons.get(i))
            .is_some_and(|x| x.action == action)
    };
    let pressed = |action: HudAction| hovered(action) && v.state.press == Some(action);
    let art = &classic.windows["diplomacy_button"];
    let items = items(classic, &skin.locale, v.state, v.mission, v.library);
    for item in &items {
        let rect = at(item.rect);
        let action = item.control.map(HudAction::Cheat);
        let lit = item.enabled && action.is_some_and(hovered);
        let down = item.enabled && action.is_some_and(pressed);
        let shift = down as i32;
        let moved = Rect {
            x: rect.x + shift,
            y: rect.y + shift,
            ..rect
        };
        let words_tone = if !item.enabled {
            color::GREY
        } else if lit {
            color::GOLD
        } else {
            color::CREAM
        };
        let middle = |h: i32| Rect {
            y: moved.y + (moved.h - h) / 2,
            h,
            ..moved
        };
        match &item.look {
            Look::Tab(words, current) => {
                slot(b, art, rect);
                if *current {
                    frame(b, rect, [0xe2, 0xa3, 0x19, 255]);
                }
                let tone = if *current {
                    color::GOLD
                } else if lit {
                    color::CREAM
                } else {
                    color::DARK_GOLD
                };
                text(&skin.small, b, words, middle(14), tone, true);
            }
            Look::Button(words) => {
                slot(b, art, rect);
                text(&skin.small, b, words, middle(14), words_tone, true);
            }
            Look::Check(on, words) => {
                let chk = &classic.windows["chk_ex"];
                let side = rect.h.min(22);
                blit(
                    b,
                    chk,
                    column(chk, 4, (!on) as i32 + 2 * lit as i32),
                    r(rect.x, rect.y + (rect.h - side) / 2, side, side),
                );
                let tone = if !item.enabled {
                    color::GREY
                } else if lit {
                    color::GOLD
                } else if *on {
                    color::CREAM
                } else {
                    color::DARK_GOLD
                };
                text(
                    &skin.small,
                    b,
                    words,
                    Rect {
                        x: rect.x + side + 6,
                        y: rect.y + (rect.h - 14) / 2,
                        w: rect.w - side - 6,
                        h: 14,
                    },
                    tone,
                    false,
                );
            }
            Look::Arrow(right) => {
                slot(b, art, rect);
                text(
                    &classic.font12b,
                    b,
                    if *right { ">" } else { "<" },
                    middle(classic.font12b.height as i32),
                    words_tone,
                    true,
                );
            }
            Look::Icon(class, civ, owner) => {
                let probe = MapObject {
                    owner: *owner,
                    ..crate::gameplay::ai::tree::probe(*class, *civ)
                };
                if let Some((image, src)) = v
                    .library
                    .icon_for(&probe)
                    .and_then(|k| classic.icon_for_color(k, v.mission.color_slot(*owner)))
                {
                    blit(b, image, src, moved);
                }
                blit(b, &classic.frame1, whole(&classic.frame1), rect);
                if lit {
                    frame(b, rect, [0xe2, 0xa3, 0x19, 255]);
                }
            }
            Look::Label(words) => {
                let tone = if item.enabled {
                    color::GOLD
                } else {
                    color::GREY
                };
                text(&skin.small, b, words, middle(14), tone, false);
            }
            Look::Value(words) => {
                let tone = if item.enabled {
                    color::WHITE
                } else {
                    color::GREY
                };
                let shown = fit(&skin.small, words, rect.w);
                text(&skin.small, b, &shown, middle(14), tone, true);
            }
            Look::Note(words) => {
                for (k, line) in skin
                    .small
                    .wrap(words, rect.w.max(1) as usize)
                    .iter()
                    .enumerate()
                {
                    if (k as i32 + 1) * 14 > rect.h + 2 {
                        break;
                    }
                    text(
                        &skin.small,
                        b,
                        line,
                        Rect {
                            y: rect.y + k as i32 * 14,
                            h: 14,
                            ..rect
                        },
                        color::CREAM,
                        false,
                    );
                }
            }
        }
    }
    // The pages of units and buildings: the object under the pointer, its
    // name and figures from the help; else how to place one.
    if matches!(v.state.cheat.tab, CheatTab::Units | CheatTab::Buildings) {
        let shown = items
            .iter()
            .find_map(|item| match (item.look_class(), item.control) {
                (Some((class, civ)), Some(control)) if hovered(HudAction::Cheat(control)) => {
                    Some((class, civ))
                }
                _ => None,
            });
        match shown {
            Some((class, civ)) => {
                let (name, _, figures) = about(classic, v.library, class, civ);
                text(
                    &skin.font,
                    b,
                    &name,
                    at(r(40, ABOUT_Y, 524, 16)),
                    color::GOLD,
                    false,
                );
                for (k, line) in skin.small.wrap(&figures, 524).iter().take(2).enumerate() {
                    text(
                        &skin.small,
                        b,
                        line,
                        at(r(40, ABOUT_Y + 18 + k as i32 * 14, 524, 14)),
                        color::WHITE,
                        false,
                    );
                }
            }
            None => {
                let what = if v.state.cheat.tab == CheatTab::Units {
                    "un'unità"
                } else {
                    "un edificio, che arriva già costruito"
                };
                let words = fill(
                    "Clicca {}, poi il punto della mappa dove metterlo. Tieni premuto Maiusc per metterne altri, tasto destro per finire.",
                    &[&t(what)],
                );
                for (k, line) in skin.small.wrap(&words, 524).iter().take(2).enumerate() {
                    text(
                        &skin.small,
                        b,
                        line,
                        at(r(40, ABOUT_Y + 6 + k as i32 * 14, 524, 14)),
                        color::CREAM,
                        false,
                    );
                }
            }
        }
    }
    let mark = fill(
        "Partita con trucchi: {}",
        &[&t(if v.mission.cheater { "sì" } else { "no" })],
    );
    text(
        &skin.small,
        b,
        &mark,
        at(r(39, 324, 189, 14)),
        color::DARK_GOLD,
        true,
    );
    let button = &classic.windows["diplomacy_button"];
    let close = at(r(366, 318, 189, 27));
    blit(b, button, whole(button), close);
    let down = pressed(HudAction::Close) as i32;
    let tone = if hovered(HudAction::Close) {
        color::GOLD
    } else {
        color::DARK_GOLD
    };
    text(
        &skin.font,
        b,
        "CHIUDI",
        Rect {
            x: close.x + down,
            y: close.y + down,
            ..close
        },
        tone,
        true,
    );
}
impl Item {
    /// The class and civilization of an icon.
    fn look_class(&self) -> Option<(usize, u8)> {
        match self.look {
            Look::Icon(class, civ, _) => Some((class, civ)),
            _ => None,
        }
    }
}

/// The line at the bottom while the panel waits for a click on the map.
pub fn waiting(
    skin: &HudSkin,
    library: &ObjectLibrary,
    spawning: Option<(usize, u8, u8)>,
    pointing: Option<Target>,
) -> String {
    let more = t("Maiusc per continuare, tasto destro per finire.");
    if let Some((class, civ, _)) = spawning {
        let (name, _, _) = about(skin, library, class, civ);
        return fill("Clicca sulla mappa dove mettere: {}. {}", &[&name, &more]);
    }
    let what = match pointing {
        Some(Target::God) => {
            t("Clicca l'oggetto da rendere invulnerabile (o di nuovo vulnerabile).")
        }
        Some(Target::Usurp) => t("Clicca l'oggetto di cui prendere il comando (o da restituire)."),
        Some(Target::Spy) => t("Clicca l'oggetto con cui guardare (o con cui smettere)."),
        Some(Target::Kingdom) => t("Clicca l'oggetto da dare al regno scelto."),
        None => return String::new(),
    };
    format!("{what} {more}")
}
