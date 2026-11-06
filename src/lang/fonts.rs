//! The Chinese fonts of this version.
//!
//! The original fonts hold the 256 characters of the code page; the Chinese
//! localization made its own, with the hanzi used by the translated texts
//! after them. [`cjk`](crate::font) reads the glyph of each hanzi from
//! `languages/zh/glyphs.txt`, which both were generated from the same order,
//! so a font and that table must always travel together: a font of the game's
//! own would show `?` for every hanzi.
//!
//! They are compiled in, like every other text of a language: the archives of
//! an installation hold no Chinese font, and the ones the original game's
//! folders hold are for its own executable, which stops at 1489 glyphs (the
//! height of its bitmap is signed 16 bit). The engine's own `font.rs`
//! rasterises in software and has room for all of them.
use super::Language;

/// The fonts, by the name the archives would give them.
const FONTS: [(&str, &[u8]); 6] = [
    ("ARIAL10_AA.FNT", include_bytes!("../../languages/zh/fonts/ARIAL10_AA.FNT")),
    ("ARIAL12B_AA.FNT", include_bytes!("../../languages/zh/fonts/ARIAL12B_AA.FNT")),
    ("ARIAL14B_AA.FNT", include_bytes!("../../languages/zh/fonts/ARIAL14B_AA.FNT")),
    ("AV10.FNT", include_bytes!("../../languages/zh/fonts/AV10.FNT")),
    ("CGB_22B.FNT", include_bytes!("../../languages/zh/fonts/CGB_22B.FNT")),
    ("SMALL.FNT", include_bytes!("../../languages/zh/fonts/SMALL.FNT")),
];
/// The folder of the fonts, as the engine asks for them.
const PREFIX: &str = "LOCAL/FONTS/";

/// The font `name` of this version's Chinese set, when the language is
/// Chinese and `name` is one of the fonts. `FIXSHAD.FNT` is not here: it holds
/// digits and shadows only, and is the original's, byte for byte.
pub fn get(language: Language, name: &str) -> Option<&'static [u8]> {
    if language != Language::Chinese {
        return None;
    }
    let key = crate::archive::key(name);
    let file = key.strip_prefix(PREFIX)?;
    FONTS
        .iter()
        .find(|(f, _)| *f == file)
        .map(|&(_, data)| data)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serves_only_the_chinese_fonts() {
        assert!(get(Language::Chinese, "LOCAL/FONTS/CGB_22B.FNT").is_some());
        assert!(get(Language::Chinese, "local\\fonts\\small.fnt").is_some());
        assert!(get(Language::Chinese, "LOCAL/FONTS/FIXSHAD.FNT").is_none());
        assert!(get(Language::Chinese, "LOCAL/STRINGS.INI").is_none());
        assert!(get(Language::English, "LOCAL/FONTS/CGB_22B.FNT").is_none());
    }
    /// What `font.rs` will accept, and that the six fonts agree with each
    /// other: they share one glyph order, so they must have one count.
    #[test]
    fn every_font_decodes_with_the_same_glyphs() {
        let mut counts = Vec::new();
        for (name, data) in FONTS {
            let font = crate::font::BitmapFont::decode(data)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            counts.push(font.count());
        }
        assert!(
            counts.iter().all(|&n| n == counts[0]),
            "glyph counts differ: {counts:?}"
        );
        // 256 of the code page, the hanzi after them; never past what
        // `font.rs` accepts (4096).
        assert!(counts[0] > 2000 && counts[0] <= 4096, "glyphs: {}", counts[0]);
    }
}
