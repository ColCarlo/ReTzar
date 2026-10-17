//! Decoder for the original engine's packed RLE sprite sheets. No legacy code is executed.
use crate::{Result, bitmap::Bitmap, invalid, u32_at};

pub struct SpriteSheet {
    pub columns: usize,
    pub rows: usize,
    pub image: Bitmap,
    pub frame_height: usize,
}
fn word(data: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        data.get(at..at + 2)
            .ok_or_else(|| invalid("Truncated sprite word"))?
            .try_into()?,
    ))
}
/// Palette indices 112–239 of indexed sprites hold the eight kingdom colours,
/// sixteen shades each, in the order of COLOR BARS.BMP.
pub const PLAYER_RAMPS: std::ops::Range<usize> = 112..240;
pub const RAMP_SHADES: usize = 16;
/// Run-only shadow sheets (`*_S.RLE`), drawn by the engine as a 50%
/// darkening of what lies below (its 0x3def / 0x7bef halving routines).
pub const SHADOW_FORMAT: u16 = 7;
pub const SHADOW: [u8; 4] = [0, 0, 0, 128];

struct Header {
    columns: usize,
    rows: usize,
    width: usize,
    height: usize,
    format: u16,
    colors: usize,
    base: usize,
}
enum Px {
    Index(usize),
    Rgb555(u16),
    /// Format 7 shadow sheets store runs only: every counted pixel darkens.
    Shadow,
}
fn header(data: &[u8]) -> Result<Header> {
    if data.len() < 38 || &data[..4] != b" elr" || &data[16..20] != b"RLE\0" {
        return Err(invalid("Invalid original RLE sprite"));
    }
    let columns = u32_at(data, 4)? as usize;
    let rows = u32_at(data, 8)? as usize;
    let width = word(data, 20)? as usize;
    let height = word(data, 22)? as usize;
    let format = word(data, 24)?;
    let colors = u32_at(data, 30)? as usize;
    if columns == 0
        || rows == 0
        || columns * rows > 4096
        || width == 0
        || height == 0
        || width > 4096
        || height < columns * rows
        || ![1, 3, SHADOW_FORMAT].contains(&format)
        || (format == 1 && !(1..=256).contains(&colors))
        || (format != 1 && colors != 0)
        || width * height > 32 * 1024 * 1024
    {
        return Err(invalid("Unsupported original sprite layout"));
    }
    let base = 38 + colors * 4;
    if base + height * 4 > data.len() {
        return Err(invalid("Truncated sprite row directory"));
    }
    Ok(Header {
        columns,
        rows,
        width,
        height,
        format,
        colors,
        base,
    })
}
/// Visit every opaque pixel as (x, y, value), validating every run.
fn pixels(data: &[u8], h: &Header, mut put: impl FnMut(usize, usize, Px)) -> Result<()> {
    for y in 0..h.height {
        let offset = u32_at(data, h.base + y * 4)? as usize;
        if offset < h.height * 4 {
            return Err(invalid("Sprite row overlaps directory"));
        }
        let mut at = h
            .base
            .checked_add(offset)
            .ok_or_else(|| invalid("Sprite row overflow"))?;
        let mut x = 0;
        while x < h.width {
            let pair = data
                .get(at..at + 2)
                .ok_or_else(|| invalid("Truncated sprite run"))?;
            let skip = pair[0] as usize;
            let count = pair[1] as usize;
            at += 2;
            if skip + count == 0 || x + skip + count > h.width {
                return Err(invalid("Invalid sprite run"));
            }
            x += skip;
            for _ in 0..count {
                let px = if h.format == SHADOW_FORMAT {
                    Px::Shadow
                } else if h.format == 1 {
                    let index = *data
                        .get(at)
                        .ok_or_else(|| invalid("Truncated indexed sprite"))?
                        as usize;
                    at += 1;
                    if index >= h.colors {
                        return Err(invalid("Invalid sprite palette index"));
                    }
                    Px::Index(index)
                } else {
                    let v = word(data, at)?;
                    at += 2;
                    Px::Rgb555(v)
                };
                put(x, y, px);
                x += 1;
            }
        }
    }
    Ok(())
}
impl SpriteSheet {
    pub fn decode(data: &[u8]) -> Result<Self> {
        Self::decode_for(data, None)
    }
    /// The size of one frame, from the header alone.
    pub fn frame_size(data: &[u8]) -> Option<[usize; 2]> {
        let h = header(data).ok()?;
        Some([h.width, h.height / (h.columns * h.rows)])
    }
    /// Indexed (format 1) sheets carry the kingdom-colour shades.
    pub fn is_indexed(data: &[u8]) -> bool {
        header(data).is_ok_and(|h| h.format == 1 && h.colors == 256)
    }
    /// Decode in the colours of kingdom slot `slot` (0–7): the sixteen light
    /// greys 240–255 take that kingdom's ramp, shade by shade, as in the
    /// original (a European peasant's cape, a knight's plume). `None` keeps
    /// the colours as stored.
    pub fn decode_for(data: &[u8], slot: Option<usize>) -> Result<Self> {
        let h = header(data)?;
        let (columns, rows, width, height) = (h.columns, h.rows, h.width, h.height);
        let mut palette = [[0u8; 4]; 256];
        for (i, entry) in palette.iter_mut().enumerate().take(h.colors) {
            let index = match slot {
                Some(s) if i >= crate::bitmap::LIGHT_GREYS && h.colors == 256 => {
                    PLAYER_RAMPS.start + s.min(7) * RAMP_SHADES + (i - crate::bitmap::LIGHT_GREYS)
                }
                _ => i,
            };
            let p = 38 + index * 4;
            *entry = [data[p + 2], data[p + 1], data[p], 255];
        }
        let mut rgba = vec![0; width * height * 4];
        let expand = |v: u16| ((v << 3) | (v >> 2)) as u8;
        pixels(data, &h, |x, y, px| {
            let color = match px {
                Px::Index(i) => palette[i],
                Px::Rgb555(v) => [
                    expand((v >> 10) & 31),
                    expand((v >> 5) & 31),
                    expand(v & 31),
                    255,
                ],
                Px::Shadow => SHADOW,
            };
            rgba[(y * width + x) * 4..(y * width + x + 1) * 4].copy_from_slice(&color);
        })?;
        // A few sheets (ANIMS/BRIBE.RLE: 17 frames of 40 rows in 681) carry
        // spare rows after the last frame; the frames themselves are whole.
        // Original RLE packs every temporal frame of one direction before
        // advancing to the next direction. Normalize to the same row-major
        // layout used by uncompressed BMP sheets and frame(column, row).
        let frame_height = height / (columns * rows);
        if columns > 1 && rows > 1 {
            let frame_len = width * frame_height * 4;
            let mut normalized = vec![0; rgba.len()];
            for column in 0..columns {
                for row in 0..rows {
                    let src = (column * rows + row) * frame_len;
                    let dst = (row * columns + column) * frame_len;
                    normalized[dst..dst + frame_len].copy_from_slice(&rgba[src..src + frame_len]);
                }
            }
            rgba = normalized;
        }
        Ok(Self {
            columns,
            rows,
            frame_height,
            image: Bitmap {
                width: width as u32,
                height: height as u32,
                rgba,
            },
        })
    }
    /// Decoded sheets are normalized vertically, by animation row then direction.
    pub fn frame(&self, column: usize, row: usize) -> Result<Bitmap> {
        if column >= self.columns || row >= self.rows {
            return Err(invalid("Sprite frame out of bounds"));
        }
        let start =
            (row * self.columns + column) * self.frame_height * self.image.width as usize * 4;
        let len = self.frame_height * self.image.width as usize * 4;
        Ok(Bitmap {
            width: self.image.width,
            height: self.frame_height as u32,
            rgba: self.image.rgba[start..start + len].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_malformed_runs_and_preserves_transparency() {
        let mut d = vec![0; 42];
        d[..4].copy_from_slice(b" elr");
        d[4..8].copy_from_slice(&1u32.to_le_bytes());
        d[8..12].copy_from_slice(&1u32.to_le_bytes());
        d[16..20].copy_from_slice(b"RLE\0");
        d[20..22].copy_from_slice(&3u16.to_le_bytes());
        d[22..24].copy_from_slice(&1u16.to_le_bytes());
        d[24..26].copy_from_slice(&3u16.to_le_bytes());
        d[38..42].copy_from_slice(&4u32.to_le_bytes());
        d.extend_from_slice(&[1, 1, 0, 124, 1, 0]);
        let s = SpriteSheet::decode(&d).unwrap();
        assert_eq!(s.image.rgba, [0, 0, 0, 0, 255, 0, 0, 255, 0, 0, 0, 0]);
        d[42] = 4;
        assert!(SpriteSheet::decode(&d).is_err());
        d[42] = 0;
        d[43] = 0;
        assert!(SpriteSheet::decode(&d).is_err());
        assert!(s.frame(1, 0).is_err());
    }

    /// A one-row sheet: header, optional palette, row table, row runs.
    fn sheet(format: u16, colors: usize, width: u16, height: u16, rows: &[&[u8]]) -> Vec<u8> {
        let mut d = vec![0; 38];
        d[..4].copy_from_slice(b" elr");
        d[4..8].copy_from_slice(&1u32.to_le_bytes());
        d[8..12].copy_from_slice(&1u32.to_le_bytes());
        d[16..20].copy_from_slice(b"RLE\0");
        d[20..22].copy_from_slice(&width.to_le_bytes());
        d[22..24].copy_from_slice(&height.to_le_bytes());
        d[24..26].copy_from_slice(&format.to_le_bytes());
        d[30..34].copy_from_slice(&(colors as u32).to_le_bytes());
        for i in 0..colors {
            // BGRA palette: index i has red = i, green = 255 - i.
            d.extend_from_slice(&[0, 255 - i as u8, i as u8, 0]);
        }
        let mut offset = rows.len() * 4;
        for r in rows {
            d.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += r.len();
        }
        for r in rows {
            d.extend_from_slice(r);
        }
        d
    }

    #[test]
    fn kingdom_shades_move_to_the_owner_ramp_and_shadows_darken() {
        // Keep already coloured pixels; only the sixteen reserved light
        // greys are the owner's colour, like the original's cape and plume.
        let d = sheet(1, 256, 5, 1, &[&[0, 5, 5, 115, 233, 243, 249]]);
        assert!(SpriteSheet::is_indexed(&d));
        let stored = SpriteSheet::decode(&d).unwrap().image.rgba;
        assert_eq!(&stored[..4], &[5, 250, 0, 255]);
        assert_eq!(&stored[4..8], &[115, 140, 0, 255]);
        // Slot 1 (red): grey shades 3 and 9 take ramp 128..143.
        let red = SpriteSheet::decode_for(&d, Some(1)).unwrap().image.rgba;
        assert_eq!(&red[..4], &stored[..4], "base colours never change");
        assert_eq!(&red[4..12], &stored[4..12], "existing ramps never change");
        assert_eq!(&red[12..16], &[131, 124, 0, 255]);
        assert_eq!(&red[16..20], &[137, 118, 0, 255]);
        // Format 7: runs only, every counted pixel is a 50% shadow.
        let shadow = sheet(SHADOW_FORMAT, 0, 4, 1, &[&[1, 2, 1, 0]]);
        assert!(!SpriteSheet::is_indexed(&shadow));
        let s = SpriteSheet::decode(&shadow).unwrap().image.rgba;
        assert_eq!(s, [[0; 4], SHADOW, SHADOW, [0; 4]].concat());
        // Spare rows after the last frame (ANIMS/BRIBE.RLE) are tolerated.
        let mut spare = sheet(3, 0, 1, 3, &[&[1, 0], &[1, 0], &[1, 0]]);
        spare[8..12].copy_from_slice(&2u32.to_le_bytes());
        let s = SpriteSheet::decode(&spare).unwrap();
        assert_eq!((s.rows, s.frame_height), (2, 1));
        assert!(s.frame(0, 1).is_ok());
    }
}
