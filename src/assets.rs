use crate::lang::Language;
use crate::{
    Result,
    archive::{self, Archive},
    invalid,
};
use std::{collections::HashMap, path::Path};

/// Later packs replace earlier resources, including the Italian expansion art.
/// One index maps every normalized name to the pack that provides it.
pub struct GameAssets {
    packs: Vec<Archive>,
    index: HashMap<String, (usize, usize)>,
    names: Vec<String>,
    language: Language,
    native: Option<Language>,
}
/// The archives of an installation, in the order the engine reads them.
const PACKS: [&str; 8] = [
    "data.wdt",
    "data2.wdt",
    "images.wdt",
    "images2.wdt",
    "terrains.wdt",
    "local.wdt",
    "help.wdt",
    "v101.wdt",
];
/// The localized screens of an edition (FX Interactive's `images2.wdt`:
/// title, menu, loading screens with their own lines).
const SCREENS: &str = "images2.wdt";
/// The folders of loose files that hold what those archives do: the whole
/// data of the digital releases (GOG's), which have no `Packs`.
pub const LOOSE: [&str; 4] = ["DATA", "HELP", "IMAGES", "LOCAL"];
/// The sounds and the music: `Packs\sounds.wdt` (also in a pack), or the
/// loose folders of an installation without it.
pub fn sounds(source: &Path) -> Result<Archive> {
    let pack = source.join("Packs/sounds.wdt");
    if crate::vfs::is_file(&pack) {
        Archive::open(&pack)
    } else {
        Archive::folder(source, &["SOUND", "MUSIC"])
    }
}
fn index_of(packs: &[(&str, Archive)], skip: Option<&str>) -> HashMap<String, (usize, usize)> {
    let mut index = HashMap::new();
    for (p, (name, pack)) in packs.iter().enumerate() {
        if Some(*name) == skip {
            continue;
        }
        for (e, entry) in pack.entries.iter().enumerate() {
            let key = archive::key(&entry.name);
            // Within one pack the first entry wins; a later pack overrides.
            if index.get(&key).is_none_or(|&(q, _)| q != p) {
                index.insert(key, (p, e));
            }
        }
    }
    index
}
impl GameAssets {
    /// The installation as it is, in its own language.
    pub fn open(source: &Path) -> Result<Self> {
        Self::open_with(source, None)
    }
    /// The installation in a language. In its own language everything is
    /// the installation's; in another the edition's localized screens give
    /// way to the original ones and the texts come from `languages`, when this
    /// version has them.
    pub fn open_in(source: &Path, language: Language) -> Result<Self> {
        Self::open_with(source, Some(language))
    }
    fn open_with(source: &Path, language: Option<Language>) -> Result<Self> {
        let mut packs = Vec::new();
        // The engine reads loose folders after its packs, so they win over
        // them; here they only give what the archives lack. A mod made for
        // the original's executable (a layout of its own) stays out, and an
        // installation without archives reads everything from them. A pack
        // (`ReTzar.dat`) holds archives only.
        if source.is_dir() {
            let loose = Archive::folder(source, &LOOSE)?;
            if !loose.entries.is_empty() {
                packs.push(("", loose));
            }
        }
        let files: Vec<_> = crate::vfs::list(&source.join("Packs")).unwrap_or_default();
        for name in PACKS {
            if let Some(path) = files
                .iter()
                .find(|p| p.file_name().is_some_and(|s| s.eq_ignore_ascii_case(name)))
            {
                let pack = Archive::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
                packs.push((name, pack));
            }
        }
        if packs.is_empty() {
            return Err(invalid(crate::lang::t("Nessun archivio del gioco trovato")));
        }
        // The installation's language, from its own texts; none for one
        // this version lacks.
        let plain = index_of(&packs, Some(SCREENS));
        let native = plain
            .get(&archive::key("LOCAL/STRINGS.INI"))
            .and_then(|&(p, e)| packs[p].1.read(&packs[p].1.entries[e]).ok())
            .and_then(|data| crate::lang::detect(&legacy_text(&data)));
        crate::lang::set_installation(native);
        let language = language.or(native).unwrap_or(Language::English);
        let index = if native == Some(language) {
            index_of(&packs, None)
        } else {
            plain
        };
        let mut names: Vec<String> = index.keys().cloned().collect();
        names.sort();
        Ok(Self {
            packs: packs.into_iter().map(|(_, p)| p).collect(),
            index,
            names,
            language,
            native,
        })
    }
    /// The language of the installation's own texts, if this version has it.
    pub fn native(&self) -> Option<Language> {
        self.native
    }
    pub fn language(&self) -> Language {
        self.language
    }
    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(&archive::key(name))
    }
    pub fn get(&self, name: &str) -> Result<Vec<u8>> {
        let mut data = self.original(name)?;
        for (mode, text) in crate::lang::overlays(self.native, self.language, name) {
            data = match mode {
                crate::lang::Mode::Replace => legacy_bytes(text),
                crate::lang::Mode::Merge => crate::lang::merge(&data, text),
            };
        }
        Ok(data)
    }
    /// A resource as the archives hold it, whatever the language.
    pub fn original(&self, name: &str) -> Result<Vec<u8>> {
        let &(pack, entry) = self
            .index
            .get(&archive::key(name))
            .ok_or_else(|| invalid(format!("Risorsa mancante: {name}")))?;
        let pack = &self.packs[pack];
        pack.read(&pack.entries[entry])
            .map_err(|e| format!("{name}: {e}").into())
    }
    /// Every resource name, normalized to upper case with forward slashes.
    pub fn names(&self) -> &[String] {
        &self.names
    }
}

#[derive(Default, Debug, Clone)]
struct Section {
    name: String,
    entries: Vec<(String, String)>,
    lookup: HashMap<String, usize>,
}
/// Original INI files keep their order; sections and keys are case-insensitive.
#[derive(Default, Debug, Clone)]
pub struct Ini {
    sections: Vec<Section>,
    lookup: HashMap<String, usize>,
}
const C1: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];
/// Letters beyond Windows-1252 that a language of this version writes: the
/// Polish ones. In the archives' bytes each is 0x81, a hole of the code
/// page that no original text holds, then 0xa0 plus its place here.
pub const BEYOND: [char; 16] = [
    'Ą', 'ą', 'Ć', 'ć', 'Ę', 'ę', 'Ł', 'ł', 'Ń', 'ń', 'Ś', 'ś', 'Ź', 'ź', 'Ż', 'ż',
];
/// Text back to the original bytes (Windows-1252, and [`BEYOND`]); other
/// characters become `?`.
pub fn legacy_bytes(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        if let Some(i) = C1.iter().position(|&x| x == c) {
            out.push(0x80 + i as u8);
        } else if let Some(i) = BEYOND.iter().position(|&x| x == c) {
            out.extend([0x81, 0xa0 + i as u8]);
        } else {
            out.push(u8::try_from(u32::from(c)).unwrap_or(b'?'));
        }
    }
    out
}
/// A character's byte in Windows-1252, if it has one.
pub fn legacy_byte(c: char) -> Option<u8> {
    match C1.iter().position(|&x| x == c) {
        Some(i) => Some(0x80 + i as u8),
        None => u8::try_from(u32::from(c)).ok(),
    }
}
pub fn legacy_text(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len());
    let mut bytes = data.iter().copied().peekable();
    while let Some(b) = bytes.next() {
        let beyond = (b == 0x81)
            .then(|| {
                bytes
                    .peek()
                    .and_then(|&n| BEYOND.get(n.wrapping_sub(0xa0) as usize))
            })
            .flatten();
        out.push(match beyond {
            Some(&c) => {
                bytes.next();
                c
            }
            None if (0x80..0xa0).contains(&b) => C1[(b - 0x80) as usize],
            None => char::from(b),
        });
    }
    out
}
impl Ini {
    /// A data table: values are kept verbatim, so Windows paths such as
    /// `anims\net1.bmp` survive.
    pub fn parse(data: &[u8]) -> Self {
        Self::read(data, false)
    }
    /// A text table (`LOCAL/STRINGS*.INI`): `\n` and `\r\n` in values are line breaks.
    pub fn parse_text(data: &[u8]) -> Self {
        Self::read(data, true)
    }
    /// The three string tables of the installation, merged in the original order.
    pub fn locale(a: &GameAssets) -> Result<Self> {
        let mut locale = Self::default();
        for name in [
            "LOCAL/STRINGS.INI",
            "LOCAL/STRINGS2.INI",
            "LOCAL/STRINGS3.INI",
        ] {
            if a.contains(name) {
                locale.merge(Self::parse_text(&a.get(name)?));
            }
        }
        Ok(locale)
    }
    fn read(data: &[u8], text_escapes: bool) -> Self {
        let text = legacy_text(data);
        let mut ini = Self::default();
        let mut section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with([';', '#']) {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            } else if let Some((key, value)) = line.split_once('=') {
                let value = value.trim();
                let value = if text_escapes {
                    value.replace("\\r\\n", "\n").replace("\\n", "\n")
                } else {
                    value.to_owned()
                };
                ini.set(&section, key.trim(), value);
            } else {
                ini.set(&section, line, String::new());
            }
        }
        ini
    }
    pub fn set(&mut self, section: &str, key: &str, value: String) {
        let section = section.to_ascii_lowercase();
        let at = match self.lookup.get(&section) {
            Some(&at) => at,
            None => {
                self.lookup.insert(section.clone(), self.sections.len());
                self.sections.push(Section {
                    name: section,
                    ..Section::default()
                });
                self.sections.len() - 1
            }
        };
        let s = &mut self.sections[at];
        let key = key.to_ascii_lowercase();
        if let Some(&i) = s.lookup.get(&key) {
            s.entries[i].1 = value;
        } else {
            s.lookup.insert(key.clone(), s.entries.len());
            s.entries.push((key, value));
        }
    }
    fn section(&self, section: &str) -> Option<&Section> {
        let at = match self.lookup.get(section) {
            Some(&at) => at,
            None => *self.lookup.get(&section.to_ascii_lowercase())?,
        };
        Some(&self.sections[at])
    }
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let s = self.section(section)?;
        let i = match s.lookup.get(key) {
            Some(&i) => i,
            None => *s.lookup.get(&key.to_ascii_lowercase())?,
        };
        Some(s.entries[i].1.as_str())
    }
    pub fn entries(&self, section: &str) -> &[(String, String)] {
        self.section(section).map_or(&[], |s| s.entries.as_slice())
    }
    pub fn section_names(&self) -> impl Iterator<Item = &str> {
        self.sections.iter().map(|s| s.name.as_str())
    }
    pub fn merge(&mut self, other: Self) {
        for section in other.sections {
            for (key, value) in section.entries {
                self.set(&section.name, &key, value);
            }
        }
    }
    pub fn label(&self, section: &str, key: &str) -> String {
        self.get(section, key)
            .unwrap_or(key)
            .split('|')
            .next()
            .unwrap()
            .trim()
            .to_owned()
    }
    /// Comma-separated integers, ignoring malformed items.
    pub fn numbers(&self, section: &str, key: &str) -> Vec<i32> {
        self.get(section, key)
            .unwrap_or("")
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn italian_text_overrides_keep_order_and_tooltips() {
        let mut ini = Ini::parse(b"[Menu]\nname=Art\xf9|Descrizione\nsecond=Prima\n");
        ini.merge(Ini::parse(b"[menu]\nSECOND=Dopo\nthird=Terza\n"));
        assert_eq!(ini.label("MENU", "NAME"), "Artù");
        assert_eq!(ini.entries("menu")[1], ("second".into(), "Dopo".into()));
        assert_eq!(ini.entries("Menu").len(), 3);
        assert_eq!(ini.get("menu", "Third"), Some("Terza"));
        assert_eq!(ini.get("other", "third"), None);
        assert_eq!(legacy_text(&[0x93, b'A', 0x94]), "“A”");
        // The Polish letters go through the archives' bytes and back.
        let polish = "Zażółć gęślą jaźń: ŁĄKA, ŻÓŁW, ŚĆ, Ę, Ń, Ź; œ é ß";
        assert_eq!(legacy_text(&legacy_bytes(polish)), polish);
        assert_eq!(legacy_text(&[0x81, b'a']), "\u{81}a");
        assert_eq!(
            Ini::parse(b"[a]\nn = 1, x,-2 ,3").numbers("A", "N"),
            [1, -2, 3]
        );
    }
}
