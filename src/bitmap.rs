//! Original BI_RGB RGB555 bitmaps -> RGBA8, preserving pixel positions.
use crate::{Result, invalid, u32_at};

/// First of the sixteen light greys (240–255) of the indexed palettes.
pub const LIGHT_GREYS: usize = 240;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
impl Bitmap {
    /// Area-average resize. Every source pixel contributes to the output, so
    /// one-cell details survive where nearest sampling would skip them.
    pub fn resample(&self, width: u32, height: u32) -> Self {
        let (sw, sh) = (self.width as usize, self.height as usize);
        let (dw, dh) = (width.max(1) as usize, height.max(1) as usize);
        let mut rgba = vec![0; dw * dh * 4];
        for dy in 0..dh {
            let y0 = dy * sh / dh;
            let y1 = ((dy + 1) * sh).div_ceil(dh).max(y0 + 1).min(sh);
            for dx in 0..dw {
                let x0 = dx * sw / dw;
                let x1 = ((dx + 1) * sw).div_ceil(dw).max(x0 + 1).min(sw);
                let mut sum = [0u32; 4];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let at = (y * sw + x) * 4;
                        for (s, &v) in sum.iter_mut().zip(&self.rgba[at..at + 4]) {
                            *s += v as u32;
                        }
                    }
                }
                let n = ((y1 - y0) * (x1 - x0)) as u32;
                let at = (dy * dw + dx) * 4;
                for (d, s) in rgba[at..at + 4].iter_mut().zip(sum) {
                    *d = ((s + n / 2) / n) as u8;
                }
            }
        }
        Self {
            width: dw as u32,
            height: dh as u32,
            rgba,
        }
    }
    /// 24-bit bottom-up BMP, transparent pixels filled with `background`.
    pub fn to_bmp(&self, background: [u8; 3]) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let stride = (w * 3).div_ceil(4) * 4;
        let size = 54 + stride * h;
        let mut out = Vec::with_capacity(size);
        out.extend_from_slice(b"BM");
        for v in [size as u32, 0, 54, 40, w as u32, h as u32] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&24u16.to_le_bytes());
        for v in [0u32, (stride * h) as u32, 2835, 2835, 0, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for y in (0..h).rev() {
            let start = out.len();
            for x in 0..w {
                let p = &self.rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                let [r, g, b] = if p[3] == 0 {
                    background
                } else {
                    [p[0], p[1], p[2]]
                };
                out.extend_from_slice(&[b, g, r]);
            }
            out.resize(start + stride, 0);
        }
        out
    }
    /// Integer nearest-neighbour enlargement that keeps every pixel square.
    pub fn scale_integer(&self, factor: u32) -> Self {
        let factor = factor.max(1) as usize;
        if factor == 1 {
            return self.clone();
        }
        let (w, h) = (self.width as usize, self.height as usize);
        let mut rgba = Vec::with_capacity(w * h * factor * factor * 4);
        for y in 0..h {
            let mut row = Vec::with_capacity(w * factor * 4);
            for x in 0..w {
                for _ in 0..factor {
                    row.extend_from_slice(&self.rgba[(y * w + x) * 4..(y * w + x) * 4 + 4]);
                }
            }
            for _ in 0..factor {
                rgba.extend_from_slice(&row);
            }
        }
        Self {
            width: (w * factor) as u32,
            height: (h * factor) as u32,
            rgba,
        }
    }
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Result<Self> {
        if width == 0
            || height == 0
            || x.saturating_add(width) > self.width
            || y.saturating_add(height) > self.height
        {
            return Err(invalid("Bitmap crop outside image"));
        }
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for row in y..y + height {
            let at = ((row * self.width + x) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[at..at + (width * 4) as usize]);
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }
    pub fn decode(data: &[u8], green_transparent: bool) -> Result<Self> {
        Self::decode_for(data, green_transparent, None)
    }
    /// Whether a bitmap is 8-bit indexed with a full palette, the only kind
    /// whose kingdom-colour shades can be moved to another colour slot.
    pub fn is_indexed(data: &[u8]) -> bool {
        data.len() >= 30 && data.starts_with(b"BM") && data[28..30] == [8, 0]
    }
    /// Decode in the colours of kingdom slot `slot`: the light greys 240–255
    /// of a full 8-bit palette take the kingdom ramp (112 + 16·slot), shade by
    /// shade. Banners and flags of the masks (`*_c`, `GATES C.BMP`) and the
    /// kingdom parts of the indexed sprites are painted in those greys.
    pub fn decode_for(data: &[u8], green_transparent: bool, slot: Option<usize>) -> Result<Self> {
        if !data.starts_with(b"BM") || data.len() < 54 {
            return Err(invalid("invalid BMP header"));
        }
        let offset = u32_at(data, 10)? as usize;
        if u32_at(data, 14)? < 40 {
            return Err(invalid("unsupported BMP DIB"));
        }
        let width = u32_at(data, 18)? as i32;
        let signed_height = u32_at(data, 22)? as i32;
        let depth = u16::from_le_bytes(data[28..30].try_into()?);
        if width <= 0
            || signed_height == 0
            || signed_height == i32::MIN
            || width > 16384
            || signed_height.unsigned_abs() > 16384
            || u32_at(data, 30)? != 0
            || ![8, 16, 24, 32].contains(&depth)
            || data[26..28] != [1, 0]
        {
            return Err(invalid(format!(
                "unsupported BMP: width={width}, height={signed_height}, depth={depth}, compression={}",
                u32_at(data, 30)?
            )));
        }
        let height = signed_height.unsigned_abs();
        let stride = (width as usize * depth as usize).div_ceil(32) * 4;
        let total = stride
            .checked_mul(height as usize)
            .ok_or_else(|| invalid("BMP size overflow"))?;
        if offset < 54 || offset.checked_add(total).is_none_or(|n| n > data.len()) {
            return Err(invalid("truncated BMP pixels"));
        }
        let palette_start = 14 + u32_at(data, 14)? as usize;
        let palette_count = if depth == 8 {
            let n = u32_at(data, 46)? as usize;
            if n == 0 { 256 } else { n }
        } else {
            0
        };
        if palette_count > 256 || (depth == 8 && palette_start + palette_count * 4 > offset) {
            return Err(invalid("invalid BMP palette"));
        }
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height as usize {
            let source_y = if signed_height > 0 {
                height as usize - 1 - y
            } else {
                y
            };
            for x in 0..width as usize {
                let p = offset + source_y * stride + x * depth as usize / 8;
                let (r, g, b) = if depth == 16 {
                    let value = u16::from_le_bytes(data[p..p + 2].try_into()?);
                    let expand = |v: u16| ((v << 3) | (v >> 2)) as u8;
                    (
                        expand((value >> 10) & 31),
                        expand((value >> 5) & 31),
                        expand(value & 31),
                    )
                } else if depth == 8 {
                    let index = data[p] as usize;
                    if index >= palette_count {
                        return Err(invalid("BMP palette index out of range"));
                    }
                    let index = match slot {
                        Some(s) if palette_count == 256 && index >= LIGHT_GREYS => {
                            crate::sprite::PLAYER_RAMPS.start
                                + s.min(7) * crate::sprite::RAMP_SHADES
                                + (index - LIGHT_GREYS)
                        }
                        _ => index,
                    };
                    let q = palette_start + index * 4;
                    (data[q + 2], data[q + 1], data[q])
                } else {
                    (data[p + 2], data[p + 1], data[p])
                };
                let alpha = if green_transparent && r == 0 && g == 255 && b == 0 {
                    0
                } else {
                    255
                };
                rgba.extend_from_slice(&[r, g, b, alpha]);
            }
        }
        Ok(Self {
            width: width as u32,
            height,
            rgba,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bmp(width: i32, height: i32, depth: u16, pixels: &[u8]) -> Vec<u8> {
        let mut data = vec![0; 54];
        data[..2].copy_from_slice(b"BM");
        for (offset, value) in [
            (10, 54u32),
            (14, 40),
            (18, width as u32),
            (22, height as u32),
        ] {
            data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        data[26..28].copy_from_slice(&1u16.to_le_bytes());
        data[28..30].copy_from_slice(&depth.to_le_bytes());
        data.extend_from_slice(pixels);
        data
    }
    #[test]
    fn rgb555_channels_orientation_padding_and_transparency() {
        // Bottom row green, top row red. Each one-pixel 16-bit row is padded.
        let data = bmp(1, 2, 16, &[0xe0, 0x03, 0, 0, 0, 0x7c, 0, 0]);
        let image = Bitmap::decode(&data, true).unwrap();
        assert_eq!(image.rgba, [255, 0, 0, 255, 0, 255, 0, 0]);
        let top_down = bmp(1, -1, 24, &[255, 0, 0, 0]);
        assert_eq!(
            Bitmap::decode(&top_down, false).unwrap().rgba,
            [0, 0, 255, 255]
        );
    }
    #[test]
    fn resampling_keeps_isolated_pixels_and_integer_scaling_keeps_squares() {
        let mut big = Bitmap {
            width: 512,
            height: 512,
            rgba: [0, 0, 0, 255].repeat(512 * 512),
        };
        // One bright cell must still be visible after shrinking 512 -> 256 -> 128.
        big.rgba[(301 * 512 + 77) * 4..(301 * 512 + 77) * 4 + 3].copy_from_slice(&[255; 3]);
        let small = big.resample(128, 128);
        assert_eq!((small.width, small.height), (128, 128));
        assert!(small.rgba.chunks_exact(4).any(|p| p[0] > 0));
        let odd = big.resample(300, 300);
        assert!(odd.rgba.chunks_exact(4).any(|p| p[0] > 0));
        let one = Bitmap {
            width: 2,
            height: 1,
            rgba: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        let two = one.scale_integer(2);
        assert_eq!((two.width, two.height), (4, 2));
        assert_eq!(
            &two.rgba[..16],
            &[1, 2, 3, 4, 1, 2, 3, 4, 5, 6, 7, 8, 5, 6, 7, 8]
        );
        assert_eq!(&two.rgba[..16], &two.rgba[16..]);
    }
    #[test]
    fn kingdom_colour_moves_only_the_light_greys() {
        // 8-bit, 256 entries: entry i is (i, i, i); pixels 255 (white), 239
        // (last shade of the grey kingdom ramp) and 10, padded to 4 bytes.
        let mut data = bmp(3, 1, 8, &[]);
        data[10..14].copy_from_slice(&(54u32 + 1024).to_le_bytes());
        data.extend((0..=255u8).flat_map(|i| [i, i, i, 0]));
        data.extend_from_slice(&[255, 239, 10, 0]);
        // Slot 1 is ramp 128–143: white takes its brightest shade, the ramp
        // colours are kept as drawn.
        let red = Bitmap::decode_for(&data, false, Some(1)).unwrap().rgba;
        assert_eq!(
            red,
            [143, 143, 143, 255, 239, 239, 239, 255, 10, 10, 10, 255]
        );
        let stored = Bitmap::decode(&data, false).unwrap().rgba;
        assert_eq!(stored[..4], [255, 255, 255, 255]);
    }
    #[test]
    fn missing_pixels_and_negative_width_are_rejected() {
        assert!(Bitmap::decode(&bmp(1, 1, 16, &[]), false).is_err());
        assert!(Bitmap::decode(&bmp(-1, 1, 16, &[0; 4]), false).is_err());
    }
}
