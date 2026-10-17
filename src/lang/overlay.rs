//! The text files of a language laid over the archives' own. Files that hold
//! only texts are replaced whole; the others, which mix texts with the
//! Italian edition's rules (terrains, help items, string tables with keys
//! the American release lacks), keep every line but the values the
//! language gives. A language translated by this version lies over the
//! English, so a value it lacked would read in English. Italian has its
//! own too (`languages/it`, this version's, from the English), for the
//! editions in another language: over the Italian edition its texts stay.
use super::Language;
use crate::{archive, assets::legacy_bytes};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The file stands for the archive's.
    Replace,
    /// `key = value` lines of the file replace those of the archive's.
    Merge,
}

/// The files of a language, UTF-8, under the archives' names.
macro_rules! files {
    ($code:literal) => {
        [
            files!(@ $code, "LOCAL/STRINGS.INI", Merge),
            files!(@ $code, "LOCAL/STRINGS2.INI", Merge),
            files!(@ $code, "LOCAL/STRINGS3.INI", Merge),
            files!(@ $code, "LOCAL/HELP.INI", Merge),
            files!(@ $code, "LOCAL/CREDITS.TXT", Replace),
            files!(@ $code, "HELP/HELP_STR.INI", Merge),
            files!(@ $code, "HELP/HK.TXT", Replace),
            files!(@ $code, "HELP/ITEMS.TXT", Merge),
            files!(@ $code, "DATA/AI/AI.INI", Replace),
            files!(@ $code, "IMAGES/TERRAIN/DESERT.TERRAIN.INI", Merge),
            files!(@ $code, "IMAGES/TERRAIN/GRASS.TERRAIN.INI", Merge),
            files!(@ $code, "IMAGES/TERRAIN/SNOW.TERRAIN.INI", Merge),
            files!(@ $code, "IMAGES/TERRAIN/SUMMER.TERRAIN.INI", Merge),
        ]
    };
    (@ $code:literal, $name:literal, $mode:ident) => {
        (
            $name,
            Mode::$mode,
            include_str!(concat!("../../languages/", $code, "/", $name)),
        )
    };
}
type Files = [(&'static str, Mode, &'static str); 13];
const ENGLISH: Files = files!("en");
const FRENCH: Files = files!("fr");
const GERMAN: Files = files!("de");
const SPANISH: Files = files!("es");
const POLISH: Files = files!("pl");
const PORTUGUESE: Files = files!("pt");
const ITALIAN: Files = files!("it");

fn files(language: Language) -> &'static [(&'static str, Mode, &'static str)] {
    match language {
        Language::Italian => &ITALIAN,
        Language::English => &ENGLISH,
        Language::French => &FRENCH,
        Language::German => &GERMAN,
        Language::Spanish => &SPANISH,
        Language::Polish => &POLISH,
        Language::Portuguese => &PORTUGUESE,
    }
}
/// The files a language lays over a resource of an installation written in
/// `native`, the lowest first: the English under a translation of this
/// version. None in the installation's own language, whose texts are kept;
/// every language has them over an edition in a language this version
/// lacks (`native` none).
pub fn overlays(
    native: Option<Language>,
    language: Language,
    name: &str,
) -> Vec<(Mode, &'static str)> {
    let key = archive::key(name);
    if native == Some(language) {
        return Vec::new();
    }
    let mut layers: Vec<Language> = Vec::new();
    if language != Language::English {
        layers.push(Language::English);
    }
    layers.push(language);
    layers
        .into_iter()
        .flat_map(|l| files(l).iter())
        .filter(|(n, _, _)| *n == key)
        .map(|&(_, mode, text)| (mode, text))
        .collect()
}
/// Every resource a language changes.
pub fn names(language: Language) -> Vec<&'static str> {
    files(language).iter().map(|(n, _, _)| *n).collect()
}

/// The original INI (Windows-1252) with the values of `overlay` (UTF-8):
/// sections, comments, order and every other line stay.
pub fn merge(original: &[u8], overlay: &str) -> Vec<u8> {
    let mut values = HashMap::new();
    let mut section = String::new();
    for line in overlay.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.split(']').next()) {
            section = name.trim().to_ascii_lowercase();
        } else if !line.starts_with(';')
            && let Some((key, value)) = line.split_once('=')
        {
            values.insert(
                (section.clone(), key.trim().to_ascii_lowercase()),
                value.trim(),
            );
        }
    }
    let mut out = Vec::with_capacity(original.len());
    let mut section = String::new();
    for (i, line) in original.split(|&b| b == b'\n').enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        let (body, cr) = match line.strip_suffix(b"\r") {
            Some(body) => (body, true),
            None => (line, false),
        };
        let text = crate::assets::legacy_text(body);
        let trimmed = text.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|l| l.split(']').next()) {
            section = name.trim().to_ascii_lowercase();
        } else if !trimmed.starts_with(';')
            && let Some(at) = body.iter().position(|&b| b == b'=')
        {
            let key = crate::assets::legacy_text(&body[..at])
                .trim()
                .to_ascii_lowercase();
            if let Some(value) = values.get(&(section.clone(), key)) {
                // The key and the spaces after `=` as they were.
                let mut keep = at + 1;
                while body.get(keep) == Some(&b' ') {
                    keep += 1;
                }
                out.extend_from_slice(&body[..keep]);
                out.extend_from_slice(&legacy_bytes(value));
                if cr {
                    out.push(b'\r');
                }
                continue;
            }
        }
        out.extend_from_slice(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_everything_but_the_values() {
        let original = b"; nota\r\n[terrain]\r\nwater = 1\r\nname = Prati\r\n[terrainname]\r\n0 = Mare\r\n1=Deserto";
        let merged = merge(
            original,
            "[Terrain]\nName = Grassland\n[terrainname]\n1 = Dirt\n[altro]\nwater = 9",
        );
        assert_eq!(
            merged,
            b"; nota\r\n[terrain]\r\nwater = 1\r\nname = Grassland\r\n[terrainname]\r\n0 = Mare\r\n1=Dirt"
        );
    }

    #[test]
    fn merge_writes_windows_1252() {
        let merged = merge(b"[a]\r\nk = x\r\n", "[a]\nk = caf\u{e9} \u{2019}");
        assert_eq!(merged, b"[a]\r\nk = caf\xe9 \x92\r\n");
    }

    #[test]
    fn the_installation_s_own_language_has_no_overlay() {
        let it = Some(Language::Italian);
        assert!(overlays(it, Language::Italian, "LOCAL/STRINGS.INI").is_empty());
        assert_eq!(
            overlays(it, Language::English, "local\\strings.ini").len(),
            1
        );
        assert_eq!(overlays(it, Language::French, "LOCAL/STRINGS.INI").len(), 2);
        // An English installation keeps its own texts in English.
        let en = Some(Language::English);
        assert!(overlays(en, Language::English, "LOCAL/STRINGS.INI").is_empty());
        assert_eq!(overlays(en, Language::Polish, "LOCAL/STRINGS.INI").len(), 2);
        // Italian over it: this version's, on the English.
        assert_eq!(overlays(en, Language::Italian, "LOCAL/STRINGS.INI").len(), 2);
        // Over an edition in another language even English lies on it.
        assert_eq!(
            overlays(None, Language::English, "LOCAL/STRINGS.INI").len(),
            1
        );
        assert_eq!(
            overlays(None, Language::Italian, "LOCAL/STRINGS.INI").len(),
            2
        );
    }

    /// The `key = value` lines of an INI, by section and key in lower case.
    fn values(text: &str) -> Vec<((String, String), String)> {
        let mut out = Vec::new();
        let mut section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.split(']').next()) {
                section = name.trim().to_ascii_lowercase();
            } else if !line.starts_with(';')
                && let Some((key, value)) = line.split_once('=')
            {
                out.push((
                    (section.clone(), key.trim().to_ascii_lowercase()),
                    value.trim().to_owned(),
                ));
            }
        }
        out
    }

    /// Each translation gives every value of the English, and only those,
    /// with the same marks for the program: `%` fields, `\n` breaks, the
    /// `~` between an item's figures and its words.
    #[test]
    fn every_language_gives_every_english_value() {
        for language in Language::TRANSLATED.into_iter().chain([Language::Italian]) {
            for (&(name, mode, english), &(_, _, theirs)) in ENGLISH.iter().zip(files(language)) {
                assert!(!theirs.trim().is_empty(), "{language:?}: {name} vuoto");
                if mode == Mode::Replace {
                    continue;
                }
                let mine: HashMap<_, _> = values(theirs).into_iter().collect();
                let english: HashMap<_, _> = values(english).into_iter().collect();
                assert_eq!(mine.len(), english.len(), "{language:?}: {name}");
                for (k, v) in english {
                    let Some(t) = mine.get(&k) else {
                        panic!("{language:?}: {name} senza {k:?}");
                    };
                    for mark in ["%", "\\n", "~"] {
                        assert_eq!(
                            v.matches(mark).count(),
                            t.matches(mark).count(),
                            "{language:?}: {name} {k:?} {mark}"
                        );
                    }
                }
            }
        }
    }
}
