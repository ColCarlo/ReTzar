//! Languages, an addition of this version: Italian, English, French,
//! German, Spanish, Polish and Portuguese. An installation is written in one
//! of them (the Italian edition, the American release, the digital ones),
//! recognized from its own texts ([`detect`]); in that language the game
//! shows them as they are. The English texts of the original game come from
//! the American release (`languages/en`, copied from its CD); what that release
//! lacks, such as the campaigns of the Italian edition and the texts added
//! by ReTzar, is translated in the same style. The other languages
//! (`languages/fr`, `de`, `es`, `pl`, `pt`) are translations of this version,
//! made from the English and the Italian together (Portuguese is European,
//! of Portugal); so is the Italian of `languages/it`, from the English, for the
//! editions in another language.
//!
//! The language, English unless chosen otherwise (never guessed from the
//! system), is chosen on the «Gioco» tab of the settings and held here
//! for the whole program, so ReTzar's own texts (written in Italian, through
//! [`t`]) and the archives ([`crate::assets::GameAssets::open_in`]) follow
//! it.
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{
        OnceLock,
        atomic::{AtomicU8, Ordering},
    },
};

mod missions;
mod overlay;
mod own;

pub use missions::{localize_mission, mission_language, mission_text, translate};
#[cfg(test)]
pub(crate) use missions::written_in;
pub use overlay::{Mode, merge, names as overlay_names, overlays};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Language {
    #[default]
    English,
    Italian,
    French,
    German,
    Spanish,
    Polish,
    Portuguese,
}
/// The choices of «Lingua», each named in its own language, in the order of
/// [`Language`]: English first, the default, then the installation's
/// Italian. The settings and the saves keep this index; it was Italian
/// first until English became the default, so the settings of earlier
/// versions, which held the default, now open in English.
pub const LANGUAGES: [&str; 7] = [
    "English",
    "Italiano",
    "Français",
    "Deutsch",
    "Español",
    "Polski",
    "Português",
];
impl Language {
    pub const ALL: [Self; 7] = [
        Self::English,
        Self::Italian,
        Self::French,
        Self::German,
        Self::Spanish,
        Self::Polish,
        Self::Portuguese,
    ];
    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or_default()
    }
    pub fn index(self) -> usize {
        self as usize
    }
    /// The language's folder under `languages` and its prefix in the catalogs.
    pub fn code(self) -> &'static str {
        match self {
            Self::Italian => "it",
            Self::English => "en",
            Self::French => "fr",
            Self::German => "de",
            Self::Spanish => "es",
            Self::Polish => "pl",
            Self::Portuguese => "pt",
        }
    }
    /// The languages translated by this version, beyond the American
    /// release's English.
    pub const TRANSLATED: [Self; 5] = [
        Self::French,
        Self::German,
        Self::Spanish,
        Self::Polish,
        Self::Portuguese,
    ];
}

/// Until the settings are read, ReTzar's own texts stay as they are written,
/// in Italian.
static CURRENT: AtomicU8 = AtomicU8::new(Language::Italian as u8);

pub fn current() -> Language {
    Language::from_index(CURRENT.load(Ordering::Relaxed) as usize)
}
pub fn set(language: Language) {
    CURRENT.store(language.index() as u8, Ordering::Relaxed);
}
pub fn english() -> bool {
    current() == Language::English
}
pub fn italian() -> bool {
    current() == Language::Italian
}
/// The game's name, on the window: the American release's «Tzar: The Burden
/// of the Crown», translated, and in Italian the Italian edition's own.
pub fn game_title(language: Language) -> &'static str {
    match language {
        Language::English => "Tzar: The Burden of the Crown",
        Language::Italian => "Tzar: Excalibur e il Re Artù",
        Language::French => "Tzar : Le Fardeau de la couronne",
        Language::German => "Tzar: Die Bürde der Krone",
        Language::Spanish => "Tzar: La carga de la corona",
        Language::Polish => "Tzar: Ciężar korony",
        Language::Portuguese => "Tzar: O Fardo da Coroa",
    }
}

static INSTALLATION: AtomicU8 = AtomicU8::new(0);
/// [`INSTALLATION`] of an edition in a language this version lacks.
const UNKNOWN: u8 = u8::MAX;

/// The language of the installation's own texts, recognized when its
/// archives are opened ([`crate::assets::GameAssets::open_in`]); none for
/// an edition in a language this version lacks (the Czech retail CD).
pub fn installation() -> Option<Language> {
    match INSTALLATION.load(Ordering::Relaxed) {
        UNKNOWN => None,
        i => Some(Language::from_index(i as usize)),
    }
}
pub fn set_installation(language: Option<Language>) {
    INSTALLATION.store(
        language.map_or(UNKNOWN, |l| l.index() as u8),
        Ordering::Relaxed,
    );
}
/// The game speaks the installation's own language: its voices, its intro
/// and its localized screens belong to it.
pub fn native() -> bool {
    installation() == Some(current())
}
/// A map's recorded voices are heard: they speak the language of its texts
/// (`written_in`, the installation's when they are too few to tell), and
/// that is the language of the moment. FX's Italian campaigns speak
/// Italian over an English installation too.
pub fn voiced(written_in: Option<Language>) -> bool {
    written_in.or(installation()) == Some(current())
}
/// The language the original texts are shown in: the one of the moment,
/// or English where this version has no translation of them (Italian over
/// another edition), which [`overlays`] then lays over the installation's.
pub fn texts() -> Language {
    let l = current();
    if native() || !overlay::names(l).is_empty() {
        l
    } else {
        Language::English
    }
}
/// The language a text is written in, by its most common little words;
/// none when too few of them are found to tell, or when they are of an
/// edition in a language this version lacks (the Czech of the retail CD,
/// near Polish).
pub fn detect(text: &str) -> Option<Language> {
    const WORDS: [(Option<Language>, &[&str]); 8] = [
        (
            Some(Language::Italian),
            &["il", "di", "della", "delle", "che", "per", "non", "gli", "sono", "nel", "alla", "una"],
        ),
        (
            Some(Language::English),
            &["the", "of", "and", "to", "you", "your", "is", "with", "this", "are", "be", "an"],
        ),
        (
            Some(Language::French),
            &["le", "les", "des", "du", "et", "pour", "vous", "une", "est", "pas", "dans", "sur"],
        ),
        (
            Some(Language::German),
            &["der", "die", "das", "und", "ist", "nicht", "sie", "mit", "den", "ein", "eine", "auf"],
        ),
        (
            Some(Language::Spanish),
            &["el", "los", "las", "del", "que", "para", "con", "por", "una", "es", "se", "al"],
        ),
        (
            Some(Language::Polish),
            &["i", "w", "nie", "na", "się", "z", "do", "jest", "aby", "lub", "oraz", "jednostki"],
        ),
        (
            Some(Language::Portuguese),
            &["o", "os", "do", "da", "dos", "das", "não", "um", "uma", "com", "em", "ao", "você"],
        ),
        (
            None,
            &["a", "se", "je", "v", "ve", "s", "k", "pro", "jako", "nebo", "není", "jednotky"],
        ),
    ];
    let mut counts: HashMap<String, usize> = HashMap::new();
    for word in text
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
    {
        *counts.entry(word.to_lowercase()).or_default() += 1;
    }
    let (language, score) = WORDS
        .iter()
        .map(|(l, words)| (*l, words.iter().filter_map(|w| counts.get(*w)).sum::<usize>()))
        .max_by_key(|&(_, score)| score)?;
    language.filter(|_| score >= 20)
}

/// One of ReTzar's own texts, written in Italian, in the current language.
pub fn t(italian: &'static str) -> &'static str {
    t_in(current(), italian)
}
/// [`t`] in a given language.
pub fn t_in(language: Language, italian: &'static str) -> &'static str {
    match own_table(language) {
        Some(table) => table.get(italian).copied().unwrap_or(italian),
        None => italian,
    }
}
/// [`t`] for a text made at run time; unknown texts stay as they are.
pub fn tr(italian: &str) -> Cow<'_, str> {
    match own_table(current()).and_then(|table| table.get(italian)) {
        Some(&text) => Cow::Borrowed(text),
        None => Cow::Borrowed(italian),
    }
}
/// A translated template with `{}` replaced by the arguments in order.
pub fn fill(template: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut parts = t(template).split("{}");
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    for (i, part) in parts.enumerate() {
        if let Some(a) = args.get(i) {
            out.push_str(&a.to_string());
        }
        out.push_str(part);
    }
    out
}
type Table = HashMap<&'static str, &'static str>;
/// The own texts of a language: English from `own.rs`, the others from
/// their catalog (`languages/<code>/own.txt`), with English for any text a
/// catalog lacks.
fn own_table(language: Language) -> Option<&'static Table> {
    static TABLES: OnceLock<Vec<Table>> = OnceLock::new();
    if language == Language::Italian {
        return None;
    }
    let tables = TABLES.get_or_init(|| {
        let english: Table = own::ENGLISH.iter().copied().collect();
        Language::ALL
            .iter()
            .map(|&l| match own::catalog(l) {
                Some(catalog) => {
                    let mut table = english.clone();
                    for (italian, text) in missions::pairs_in(catalog, l.code()) {
                        // Each text once for the whole program: the keys
                        // must outlive the call, as `t` gives them back.
                        let italian: &'static str = Box::leak(italian.into_boxed_str());
                        table.insert(italian, Box::leak(text.into_boxed_str()));
                    }
                    table
                }
                None if l == Language::English => english.clone(),
                None => Table::new(),
            })
            .collect()
    });
    tables.get(language.index())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_texts_have_one_translation_each() {
        let mut seen = std::collections::HashSet::new();
        for (italian, english) in own::ENGLISH {
            assert!(seen.insert(italian), "doppione: {italian}");
            assert!(!english.trim().is_empty(), "vuoto: {italian}");
            // The same placeholders on both sides.
            assert_eq!(
                italian.matches("{}").count(),
                english.matches("{}").count(),
                "{italian}"
            );
        }
    }

    #[test]
    fn every_language_translates_every_own_text() {
        let italian: std::collections::HashSet<&str> =
            own::ENGLISH.iter().map(|(it, _)| *it).collect();
        for language in Language::TRANSLATED {
            let catalog = own::catalog(language).unwrap();
            let pairs = missions::pairs_in(catalog, language.code());
            let mut seen = std::collections::HashSet::new();
            for (it, text) in &pairs {
                assert!(seen.insert(it.as_str()), "{language:?}: doppione {it}");
                assert!(italian.contains(it.as_str()), "{language:?}: in più {it}");
                assert!(!text.trim().is_empty(), "{language:?}: vuoto {it}");
                for mark in ["{}", "{a}", "{b}", "\n"] {
                    assert_eq!(
                        it.matches(mark).count(),
                        text.matches(mark).count(),
                        "{language:?}: {mark:?} in {it}"
                    );
                }
            }
            for it in &italian {
                assert!(seen.contains(it), "{language:?}: manca {it}");
            }
        }
    }

    #[test]
    fn every_language_names_the_game_its_own_way() {
        let titles: std::collections::HashSet<_> =
            Language::ALL.iter().map(|&l| game_title(l)).collect();
        assert_eq!(titles.len(), Language::ALL.len());
        assert!(titles.iter().all(|t| t.starts_with("Tzar")));
    }

    #[test]
    fn fill_replaces_in_order() {
        assert_eq!(fill("{} e {}", &[&1, &"due"]), "1 e due");
    }

    #[test]
    fn indices_stay_as_the_settings_keep_them() {
        for (i, l) in Language::ALL.iter().enumerate() {
            assert_eq!(l.index(), i);
            assert_eq!(Language::from_index(i), *l);
        }
        assert_eq!(Language::from_index(99), Language::English);
        assert_eq!(
            (LANGUAGES[0], Language::default()),
            ("English", Language::English)
        );
    }

    #[test]
    fn an_installation_s_language_is_told_by_its_texts() {
        for (language, strings) in [
            (Language::English, include_str!("../languages/en/LOCAL/STRINGS.INI")),
            (Language::French, include_str!("../languages/fr/LOCAL/STRINGS.INI")),
            (Language::German, include_str!("../languages/de/LOCAL/STRINGS.INI")),
            (Language::Spanish, include_str!("../languages/es/LOCAL/STRINGS.INI")),
            (Language::Polish, include_str!("../languages/pl/LOCAL/STRINGS.INI")),
            (
                Language::Portuguese,
                include_str!("../languages/pt/LOCAL/STRINGS.INI"),
            ),
        ] {
            assert_eq!(detect(strings), Some(language));
        }
        // The Italian of ReTzar's own texts.
        let italian: String = include_str!("../languages/fr/own.txt")
            .lines()
            .filter_map(|l| l.strip_prefix("it:"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(detect(&italian), Some(Language::Italian));
        assert_eq!(detect("OK"), None);
        // GOG's Portuguese (of Brazil, near Spanish) is Portuguese; the
        // Czech of the retail CD, near Polish, is a language this version
        // lacks.
        let portuguese = "O castelo não pode ser construído aqui. Você precisa de mais ouro \
            para treinar os soldados. Os camponeses do reino trabalham com as ferramentas da \
            aldeia. Uma torre de pedra defende o porto contra os inimigos. Escolha um herói \
            para comandar o exército do rei e conquistar as terras dos bárbaros ao norte.";
        let czech = "Hrad není možné postavit na tomto místě. Je potřeba více zlata a dřeva \
            pro výcvik vojáků. Rolníci se vrátí do vesnice a budou pracovat na poli. Vyber \
            jednotky a pošli je k bráně, nebo počkej v pevnosti. Kamenná věž chrání přístav \
            jako štít a lučištníci střílí ze zdí s velkou přesností.";
        assert_eq!(
            detect(&[portuguese, portuguese].join("\n")),
            Some(Language::Portuguese)
        );
        assert_eq!(detect(&[czech, czech].join("\n")), None);
    }
}
