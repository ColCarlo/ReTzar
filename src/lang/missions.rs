//! The texts of the missions in each language: titles, descriptions,
//! kingdoms, heroes, objectives, stories, messages and conversations. The
//! maps and the tutorial take the American release's own texts in English,
//! matched to the Italian ones line by line; the campaigns of the Italian
//! edition, which the American release lacks, are translated, and so are the
//! American campaign and the Spanish edition's, which the Italian edition
//! lacks (their English and Spanish are the originals). Spanish is FX's own
//! where its edition has the text; French, German, Polish and Portuguese are
//! this version's.
//!
//! A catalog (`languages/<code>/missions/*.txt`) is a list of pairs, the
//! Italian and the language's:
//!
//! ```text
//! it: Merlino e il Re Artù
//! en: Merlin and King Arthur
//! ```
//!
//! `\n` stands for a line break and `\\` for a backslash; lines starting
//! with `#` are notes. A text is looked up as it is, without carriage
//! returns and with its ends trimmed, so a game saved in any language opens
//! in any other.
use super::{Language, current};
use crate::world::MissionWorld;
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

/// The catalogs of a language: the campaigns, the maps and the tutorial.
/// Where two campaigns write a text alike, the first one's Italian is read:
/// «Los Dominios de la Magia» comes first, as «Nigromante» is its
/// necromancer, not Genghis Khan's witch; «La Reconquista» comes before
/// the American campaign, as its «Reino 2» is a kingdom of its own, but
/// after FX's Excalibur, whose Spanish «Maleantes» stays.
macro_rules! catalogs {
    ($code:literal) => {
        [
            include_str!(concat!(
                "../../languages/",
                $code,
                "/missions/dominions.txt"
            )),
            include_str!(concat!("../../languages/", $code, "/missions/maps.txt")),
            include_str!(concat!("../../languages/", $code, "/missions/tutorial.txt")),
            include_str!(concat!(
                "../../languages/",
                $code,
                "/missions/excalibur.txt"
            )),
            include_str!(concat!(
                "../../languages/",
                $code,
                "/missions/genghis_khan.txt"
            )),
            include_str!(concat!(
                "../../languages/",
                $code,
                "/missions/reconquista.txt"
            )),
            include_str!(concat!(
                "../../languages/",
                $code,
                "/missions/burden_of_the_crown.txt"
            )),
        ]
    };
}
/// English first: where two languages write a text alike, it reads as the
/// English one.
const CATALOGS: [(Language, [&str; 7]); 6] = [
    (Language::English, catalogs!("en")),
    (Language::French, catalogs!("fr")),
    (Language::German, catalogs!("de")),
    (Language::Spanish, catalogs!("es")),
    (Language::Polish, catalogs!("pl")),
    (Language::Portuguese, catalogs!("pt")),
];

struct Tables {
    /// Each language's text of an Italian one, by [`Language::index`].
    from_italian: Vec<HashMap<String, String>>,
    /// The Italian of a text of any other language.
    to_italian: HashMap<String, String>,
    /// Each language's own texts, which stay as they are in it.
    known: Vec<HashSet<String>>,
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}
/// How a text is looked up.
fn key(text: &str) -> String {
    text.replace('\r', "").trim().to_owned()
}
/// The pairs of an English catalog, in order, as they are looked up.
#[cfg(test)]
pub(crate) fn pairs(catalog: &str) -> Vec<(String, String)> {
    pairs_in(catalog, "en")
        .into_iter()
        .map(|(it, text)| (key(&it), text))
        .collect()
}
/// The pairs of a catalog whose translations start with `code:`, in order,
/// each text exactly as written.
pub(crate) fn pairs_in(catalog: &str, code: &str) -> Vec<(String, String)> {
    let prefix = format!("{code}:");
    let mut out = Vec::new();
    let mut italian: Option<String> = None;
    for line in catalog.lines() {
        if let Some(text) = line.strip_prefix("it:") {
            italian = Some(unescape(text.strip_prefix(' ').unwrap_or(text)));
        } else if let Some(text) = line.strip_prefix(prefix.as_str())
            && let Some(it) = italian.take()
        {
            out.push((it, unescape(text.strip_prefix(' ').unwrap_or(text))));
        }
    }
    out
}
fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut from_italian = vec![HashMap::new(); Language::ALL.len()];
        let mut to_italian = HashMap::new();
        let mut known = vec![HashSet::new(); Language::ALL.len()];
        for (language, catalogs) in CATALOGS {
            for catalog in catalogs {
                for (italian, text) in pairs_in(catalog, language.code()) {
                    let italian = key(&italian);
                    known[language.index()].insert(key(&text));
                    to_italian
                        .entry(key(&text))
                        .or_insert_with(|| italian.clone());
                    from_italian[language.index()]
                        .entry(italian)
                        .or_insert(text);
                }
            }
        }
        Tables {
            from_italian,
            to_italian,
            known,
        }
    })
}

/// A mission text in `language`, if a catalog has it in another one.
pub fn translate(text: &str, language: Language) -> Option<&'static str> {
    let k = key(text);
    if k.is_empty() {
        return None;
    }
    let t = tables();
    if language == Language::Italian {
        // A text of the Italian maps is already Italian, even where another
        // language writes the same words for a different one.
        if t.from_italian[Language::English.index()].contains_key(&k) {
            return None;
        }
        return t.to_italian.get(&k).map(String::as_str);
    }
    let texts = &t.from_italian[language.index()];
    if let Some(text) = texts.get(&k) {
        return Some(text.as_str());
    }
    // A text already in the language stays as it is, even where two of
    // its texts stand for one Italian.
    if t.known[language.index()].contains(&k) {
        return None;
    }
    t.to_italian
        .get(&k)
        .and_then(|it| texts.get(it))
        .map(String::as_str)
}
/// Whether a catalog of `language` already writes `text` (Village, Pirates).
#[cfg(test)]
pub(crate) fn written_in(text: &str, language: Language) -> bool {
    tables().known[language.index()].contains(&key(text))
}
/// A mission text in the current language.
pub fn mission_text(text: &str) -> String {
    translate(text, current()).map_or_else(|| text.to_owned(), str::to_owned)
}
fn swap(text: &mut String, language: Language) {
    if let Some(t) = translate(text, language) {
        *text = t.to_owned();
    }
}

/// Every text of a mission in `language`.
pub fn localize_mission(world: &mut MissionWorld, language: Language) {
    use crate::script::Action;
    for o in &mut world.objects {
        // A hero of the game's own (0x506d60) was named from HEROES.INI,
        // not by a map: «Lancelot» there is not Arthur's knight.
        if o.flags & crate::world::FLAG_HERO == 0 {
            swap(&mut o.name, language);
        }
        swap(&mut o.subtitle, language);
    }
    for k in &mut world.kingdoms {
        swap(&mut k.name, language);
        swap(&mut k.leader, language);
    }
    for a in &mut world.script.actions {
        match &mut a.value {
            Action::AddQuest { text, .. } | Action::Chat(text) => swap(text, language),
            Action::Lose(_, text) | Action::Win(_, text) => swap(text, language),
            Action::Story { text, .. } => swap(text, language),
            Action::Message { text, title, .. } => {
                swap(text, language);
                swap(title, language);
            }
            _ => {}
        }
    }
    for text in world.script.defeat.iter_mut().flatten() {
        swap(text, language);
    }
    for c in &mut world.conversations {
        for p in &mut c.phrases {
            swap(&mut p.text, language);
        }
    }
    // What a game in progress shows: the open window and the objectives.
    if let Some(m) = &mut world.message {
        swap(&mut m.title, language);
        swap(&mut m.text, language);
        for c in &mut m.choices {
            swap(c, language);
        }
    }
    for q in &mut world.quests {
        swap(&mut q.text, language);
    }
    if let Some(s) = &mut world.simulation {
        swap(&mut s.gameplay.script.end_text, language);
    }
}
/// The language a map's own texts are written in, by its stories,
/// messages, objectives and conversations: the one whose catalogs know
/// most of them as originals, or, when none or two tie (a map no catalog
/// has, as the Spanish edition's), their most common little words
/// ([`super::detect`]).
pub fn mission_language(world: &MissionWorld) -> Option<Language> {
    use crate::script::Action;
    let mut texts: Vec<&str> = Vec::new();
    for a in &world.script.actions {
        match &a.value {
            Action::AddQuest { text, .. } | Action::Chat(text) => texts.push(text),
            Action::Lose(_, text) | Action::Win(_, text) => texts.push(text),
            Action::Story { text, .. } => texts.push(text),
            Action::Message { text, title, .. } => texts.extend([text.as_str(), title]),
            _ => {}
        }
    }
    for c in &world.conversations {
        texts.extend(c.phrases.iter().map(|p| p.text.as_str()));
    }
    let t = tables();
    let knows = |language: Language, text: &str| match language {
        // Every catalog pairs its Italian with an English text.
        Language::Italian => t.from_italian[Language::English.index()].contains_key(&key(text)),
        _ => t.known[language.index()].contains(&key(text)),
    };
    let mut votes: Vec<(usize, Language)> = Language::ALL
        .into_iter()
        .map(|l| (texts.iter().filter(|text| knows(l, text)).count(), l))
        .collect();
    votes.sort_by_key(|&(n, _)| std::cmp::Reverse(n));
    if votes[0].0 > votes[1].0 {
        return Some(votes[0].1);
    }
    super::detect(&texts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_pairs_and_escapes() {
        let p = pairs(
            "# nota\nit: Riga uno\\nRiga due\nen: Line one\\nLine two\n\nit: a\\\\b\nen: c\n",
        );
        assert_eq!(
            p,
            vec![
                (
                    "Riga uno\nRiga due".to_owned(),
                    "Line one\nLine two".to_owned()
                ),
                ("a\\b".to_owned(), "c".to_owned()),
            ]
        );
    }

    #[test]
    fn a_hero_of_the_game_keeps_the_name_heroes_ini_gave() {
        // «Lancelot» is Arthur's knight in the Italian campaign and also the
        // first European name of HEROES.INI, that a promotion takes.
        assert_eq!(
            translate("Lancelot", Language::Italian),
            Some("Lancillotto")
        );
        let mut world = MissionWorld::default();
        let mut knight = crate::world::MapObject::default();
        knight.name = "Lancelot".into();
        let mut promoted = knight.clone();
        promoted.flags |= crate::world::FLAG_HERO;
        world.objects = vec![knight, promoted];
        localize_mission(&mut world, Language::Italian);
        assert_eq!(world.objects[0].name, "Lancillotto");
        assert_eq!(world.objects[1].name, "Lancelot");
    }

    #[test]
    fn the_american_campaign_reads_both_ways_without_clashes() {
        let american = pairs(include_str!(
            "../../languages/en/missions/burden_of_the_crown.txt"
        ));
        assert!(!american.is_empty());
        for (italian, english) in &american {
            // Its English, the original, reads as this Italian...
            assert_eq!(
                translate(english, Language::Italian),
                Some(italian.as_str()),
                "{english}"
            );
            // ...and none is an Italian text of another catalog, which
            // reading the map in English would change.
            assert_eq!(
                translate(english, Language::English).filter(|e| e != english),
                None,
                "{english}"
            );
        }
    }

    #[test]
    fn the_spanish_campaigns_read_in_every_language() {
        // Los Dominios de la Magia and La Reconquista.
        for k in [0, 5] {
            let spanish = pairs_in(CATALOGS[3].1[k], "es");
            assert!(spanish.len() > 300);
            for (language, catalogs) in CATALOGS {
                let theirs: HashMap<String, String> = pairs_in(catalogs[k], language.code())
                    .into_iter()
                    .map(|(it, text)| (key(&it), text))
                    .collect();
                for (italian, original) in &spanish {
                    let shown = translate(original, language).unwrap_or(original);
                    // In Spanish the original stays, where two read as one
                    // Italian too.
                    let wanted = match language {
                        Language::Spanish => original,
                        _ => &theirs[&key(italian)],
                    };
                    assert_eq!(shown, wanted, "{language:?}: «{original}»");
                }
            }
            for (italian, original) in &spanish {
                // The Italian edition's Excalibur says these itself.
                if ["Espada", "Tiempo"].contains(&original.as_str()) {
                    continue;
                }
                let shown = translate(original, Language::Italian).unwrap_or(original);
                assert_eq!(shown, key(italian), "«{original}»");
            }
        }
    }

    #[test]
    fn every_catalog_reads_whole() {
        for (language, catalogs) in CATALOGS {
            let prefix = format!("{}:", language.code());
            for catalog in catalogs {
                let its = catalog.lines().filter(|l| l.starts_with("it:")).count();
                let texts = catalog.lines().filter(|l| l.starts_with(&prefix)).count();
                assert_eq!(its, texts, "{language:?}");
                let pairs = pairs_in(catalog, language.code());
                assert_eq!(pairs.len(), its, "{language:?}");
                assert!(pairs.iter().all(|(_, t)| !t.trim().is_empty()));
            }
        }
    }

    /// Every language says every text of the English catalogs, with the
    /// same line breaks.
    #[test]
    fn every_language_has_every_mission_text() {
        let [(_, english), translated @ ..] = CATALOGS;
        for (language, catalogs) in translated {
            for (en, other) in english.iter().zip(catalogs) {
                let theirs: HashMap<String, String> = pairs_in(other, language.code())
                    .into_iter()
                    .map(|(it, text)| (key(&it), text))
                    .collect();
                for (italian, text) in pairs(en) {
                    let Some(t) = theirs.get(&italian) else {
                        panic!("{language:?}: manca «{italian}»");
                    };
                    assert_eq!(
                        text.matches('\n').count(),
                        t.matches('\n').count(),
                        "{language:?}: a capo diversi in «{italian}»"
                    );
                }
            }
        }
    }
}
