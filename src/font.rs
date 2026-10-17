//! Original Haemimont bitmap fonts: 50-byte metrics, advance and glyph tables,
//! then an indexed BMP whose pixel values are coverage rather than a palette.
use crate::{Result, assets::BEYOND, invalid, u32_at};
use std::cell::RefCell;

/// A text the interface drew while its picture was recorded for «Testi in
/// alta definizione» (an option of this version): where it starts and its
/// size in interface pixels, its colour, and the pixels the font would have
/// covered (index, cover 0–255).
#[derive(Clone, Debug)]
pub struct TextOp {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: [u8; 3],
    pub pixels: Vec<(usize, u8)>,
}
/// The texts of a recorded picture and, for each pixel, how many texts had
/// been recorded when a picture last covered it: text `k` (1-based) shows
/// there while that count is below `k`. A text over a text hides nothing,
/// as their sharp copies are laid over each other in order.
pub struct Recording {
    pub width: usize,
    pub height: usize,
    pub ops: Vec<TextOp>,
    pub covered: Vec<u16>,
}
impl Recording {
    /// Whether text `k` (1-based) still shows at pixel `i`.
    pub fn shows(&self, k: usize, i: usize) -> bool {
        (self.covered[i] as usize) < k
    }
}
/// The pixels a picture covers while one is recorded (`with_ids`).
pub struct Cover<'a> {
    count: u16,
    covered: &'a mut [u16],
}
impl Cover<'_> {
    /// The texts recorded so far are hidden at pixel `i`.
    pub fn mark(&mut self, i: usize) {
        self.covered[i] = self.count;
    }
}
thread_local! {
    static RECORDING: RefCell<Option<Recording>> = const { RefCell::new(None) };
}
/// Runs `f` with the texts of the `width × height` picture recorded instead
/// of drawn into it.
pub fn record<R>(width: usize, height: usize, f: impl FnOnce() -> R) -> (R, Recording) {
    RECORDING.with(|r| {
        *r.borrow_mut() = Some(Recording {
            width,
            height,
            ops: Vec::new(),
            covered: vec![0; width * height],
        })
    });
    let out = f();
    let recording = RECORDING
        .with(|r| r.borrow_mut().take())
        .expect("recording");
    (out, recording)
}
/// The text marks of the picture being recorded, if `canvas` is it: the
/// interface's drawing hides the texts under what it covers.
pub fn with_ids<R>(canvas: [usize; 2], f: impl FnOnce(Option<Cover>) -> R) -> R {
    RECORDING.with(|r| {
        let mut r = r.borrow_mut();
        f(r.as_mut()
            .filter(|r| [r.width, r.height] == canvas)
            .map(|r| Cover {
                count: r.ops.len().min(u16::MAX as usize) as u16,
                covered: r.covered.as_mut_slice(),
            }))
    })
}
/// Keeps the text if `canvas` is being recorded.
fn capture(canvas: [usize; 2], op: impl FnOnce() -> TextOp) -> bool {
    RECORDING.with(|r| {
        let mut r = r.borrow_mut();
        let Some(rec) = r.as_mut().filter(|r| [r.width, r.height] == canvas) else {
            return false;
        };
        rec.ops.push(op());
        true
    })
}
pub struct BitmapFont {
    pub height: usize,
    pub width: usize,
    advances: [u8; 256],
    glyphs: [u8; 256],
    /// The glyph and advance of each letter of [`BEYOND`], which no
    /// original font has, made after the font's own ([`Self::compose`]).
    beyond: [(usize, u8); BEYOND.len()],
    pixels: Vec<u8>,
    levels: u8,
}
impl BitmapFont {
    pub fn decode(data: &[u8]) -> Result<Self> {
        let bmp = data.get(562..).ok_or_else(|| invalid("Truncated FNT"))?;
        if !bmp.starts_with(b"BM")
            || data.len() < 616
            || u32_at(bmp, 30)? != 0
            || bmp[28..30] != [8, 0]
        {
            return Err(invalid("Unsupported FNT"));
        }
        let height = u16::from_le_bytes(data[32..34].try_into()?) as usize;
        let width = u32_at(bmp, 18)? as usize;
        let rows = u32_at(bmp, 22)? as usize;
        if height == 0
            || height > 128
            || width == 0
            || width > 128
            || rows % height != 0
            || rows / height > 256
        {
            return Err(invalid("Invalid FNT dimensions"));
        }
        let offset = u32_at(bmp, 10)? as usize;
        let stride = width.div_ceil(4) * 4;
        let src = bmp
            .get(offset..offset + stride * rows)
            .ok_or_else(|| invalid("Truncated FNT pixels"))?;
        let mut pixels = Vec::with_capacity(width * rows);
        for y in 0..rows {
            pixels
                .extend_from_slice(&src[(rows - 1 - y) * stride..(rows - 1 - y) * stride + width]);
        }
        let levels = *pixels.iter().max().unwrap_or(&1);
        if levels > 7 {
            return Err(invalid("Color FNT is not a coverage font"));
        }
        let mut font = Self {
            height,
            width,
            advances: data[50..306].try_into()?,
            glyphs: data[306..562].try_into()?,
            beyond: [(0, 0); BEYOND.len()],
            pixels,
            levels: levels.max(1),
        };
        font.compose();
        Ok(font)
    }
    /// A character's glyph and advance: the code page's (Windows-1252), as
    /// the original draws its bytes, a letter of [`BEYOND`] as made here,
    /// anything else as `?`.
    fn slot(&self, c: char) -> (usize, u8) {
        if let Some(i) = BEYOND.iter().position(|&b| b == c) {
            return self.beyond[i];
        }
        let mut code = crate::assets::legacy_byte(c).map_or(63, usize::from);
        // A font without the typographic quotes and dashes, the original's
        // glyphs at their codes, shows the plain ones.
        if code != 63 && self.glyphs[code] == self.glyphs[63] {
            code = match c {
                '‘' | '’' | '‚' => 39,
                '“' | '”' | '„' => 34,
                '–' | '—' => 45,
                '…' => 46,
                _ => code,
            };
        }
        (self.glyphs[code] as usize, self.advances[code])
    }
    /// One glyph pixel over `px`. Over a transparent pixel (the map seen
    /// through the interface) the glyph keeps its colour and its coverage
    /// becomes the alpha, so text stays visible there.
    fn blend(&self, px: &mut [u8], color: [u8; 3], coverage: u8) {
        let levels = self.levels as u16;
        if px[3] == 0 {
            px[..3].copy_from_slice(&color);
            px[3] = (coverage as u16 * 255 / levels) as u8;
            return;
        }
        for ch in 0..3 {
            px[ch] = ((px[ch] as u16 * (levels - coverage as u16)
                + color[ch] as u16 * coverage as u16)
                / levels) as u8;
        }
    }
    pub fn measure(&self, text: &str) -> usize {
        let text = crate::lang::tr(text);
        text.chars().map(|c| self.slot(c).1 as usize).sum()
    }
    pub fn draw(
        &self,
        rgba: &mut [u8],
        canvas_width: usize,
        canvas_height: usize,
        text: &str,
        x: i32,
        y: i32,
        color: [u8; 3],
    ) {
        // ReTzar's own texts in the language of the moment.
        let text = crate::lang::tr(text);
        let text: &str = &text;
        let canvas = [canvas_width, canvas_height];
        if capture(canvas, || TextOp {
            text: text.to_owned(),
            x: x as f32,
            y: y as f32,
            width: self.measure(text) as f32,
            height: self.height as f32,
            color,
            pixels: self.cover(text, x, y, canvas),
        }) {
            return;
        }
        let mut x = x;
        for c in text.chars() {
            let (glyph, step) = self.slot(c);
            for gy in 0..self.height {
                for gx in 0..self.width {
                    let Some(&coverage) = self
                        .pixels
                        .get((glyph * self.height + gy) * self.width + gx)
                    else {
                        continue;
                    };
                    let (px, py) = (x + gx as i32, y + gy as i32);
                    if coverage == 0
                        || px < 0
                        || py < 0
                        || px >= canvas_width as i32
                        || py >= canvas_height as i32
                    {
                        continue;
                    }
                    let at = (py as usize * canvas_width + px as usize) * 4;
                    self.blend(&mut rgba[at..at + 4], color, coverage);
                }
            }
            x += step as i32;
        }
    }
    /// The pixels `draw` would cover, with their cover 0–255.
    fn cover(&self, text: &str, mut x: i32, y: i32, canvas: [usize; 2]) -> Vec<(usize, u8)> {
        let mut out = Vec::new();
        for c in text.chars() {
            let (glyph, step) = self.slot(c);
            for gy in 0..self.height {
                for gx in 0..self.width {
                    let coverage = *self
                        .pixels
                        .get((glyph * self.height + gy) * self.width + gx)
                        .unwrap_or(&0);
                    let (px, py) = (x + gx as i32, y + gy as i32);
                    if coverage == 0
                        || px < 0
                        || py < 0
                        || px >= canvas[0] as i32
                        || py >= canvas[1] as i32
                    {
                        continue;
                    }
                    let cover = (coverage as u16 * 255 / self.levels as u16) as u8;
                    out.push((py as usize * canvas[0] + px as usize, cover));
                }
            }
            x += step as i32;
        }
        out
    }
    pub fn wrap(&self, text: &str, width: usize) -> Vec<String> {
        let text = crate::lang::tr(text);
        let mut lines = Vec::new();
        for paragraph in text.lines() {
            let mut line = String::new();
            // A no-break space (French before «:», «;», «?», «!») holds its
            // words together.
            for word in paragraph.split_ascii_whitespace() {
                let next = if line.is_empty() {
                    word.to_owned()
                } else {
                    format!("{line} {word}")
                };
                if self.measure(&next) > width && !line.is_empty() {
                    lines.push(line);
                    line = word.to_owned();
                } else {
                    line = next;
                }
            }
            lines.push(line);
        }
        lines
    }
    /// Fit complete labels without deleting characters. Preserve glyph proportions.
    pub fn draw_fit(
        &self,
        rgba: &mut [u8],
        canvas: [usize; 2],
        text: &str,
        rect: [i32; 4],
        color: [u8; 3],
        centered: bool,
    ) {
        let text = crate::lang::tr(text);
        let text: &str = &text;
        let scale = ((rect[2].max(1) as f32) / self.measure(text).max(1) as f32).min(1.);
        let width = (self.measure(text) as f32 * scale).round() as i32;
        let height = (self.height as f32 * scale).ceil() as i32;
        let x = rect[0] + if centered { (rect[2] - width) / 2 } else { 0 };
        let y = rect[1] + ((rect[3] - height) / 2).max(0);
        if scale == 1. {
            self.draw(rgba, canvas[0], canvas[1], text, x, y, color);
            return;
        }
        if capture(canvas, || TextOp {
            text: text.to_owned(),
            x: x as f32,
            y: y as f32,
            width: self.measure(text) as f32 * scale,
            height: self.height as f32 * scale,
            color,
            pixels: self.cover_scaled(text, x, y, scale, height, canvas),
        }) {
            return;
        }
        let mut advance = 0f32;
        for c in text.chars() {
            let (glyph, step) = self.slot(c);
            for dy in 0..height {
                for dx in 0..(self.width as f32 * scale).ceil() as i32 {
                    let gx = ((dx as f32 / scale) as usize).min(self.width - 1);
                    let gy = ((dy as f32 / scale) as usize).min(self.height - 1);
                    let coverage = *self
                        .pixels
                        .get((glyph * self.height + gy) * self.width + gx)
                        .unwrap_or(&0);
                    let (px, py) = (x + advance.round() as i32 + dx, y + dy);
                    if coverage == 0
                        || px < 0
                        || py < 0
                        || px >= canvas[0] as i32
                        || py >= canvas[1] as i32
                    {
                        continue;
                    }
                    let at = (py as usize * canvas[0] + px as usize) * 4;
                    self.blend(&mut rgba[at..at + 4], color, coverage);
                }
            }
            advance += step as f32 * scale;
        }
    }
    /// The pixels the compressed `draw_fit` would cover.
    fn cover_scaled(
        &self,
        text: &str,
        x: i32,
        y: i32,
        scale: f32,
        height: i32,
        canvas: [usize; 2],
    ) -> Vec<(usize, u8)> {
        let mut out = Vec::new();
        let mut advance = 0f32;
        for c in text.chars() {
            let (glyph, step) = self.slot(c);
            for dy in 0..height {
                for dx in 0..(self.width as f32 * scale).ceil() as i32 {
                    let gx = ((dx as f32 / scale) as usize).min(self.width - 1);
                    let gy = ((dy as f32 / scale) as usize).min(self.height - 1);
                    let coverage = *self
                        .pixels
                        .get((glyph * self.height + gy) * self.width + gx)
                        .unwrap_or(&0);
                    let (px, py) = (x + advance.round() as i32 + dx, y + dy);
                    if coverage == 0
                        || px < 0
                        || py < 0
                        || px >= canvas[0] as i32
                        || py >= canvas[1] as i32
                    {
                        continue;
                    }
                    let cover = (coverage as u16 * 255 / self.levels as u16) as u8;
                    out.push((py as usize * canvas[0] + px as usize, cover));
                }
            }
            advance += step as f32 * scale;
        }
        out
    }
    /// Long numeric counters keep the font's height. Area sampling preserves
    /// strokes when the available width requires horizontal compression.
    pub fn draw_number(
        &self,
        rgba: &mut [u8],
        canvas: [usize; 2],
        text: &str,
        rect: [i32; 4],
        color: [u8; 3],
        centered: bool,
    ) {
        let text = crate::lang::tr(text);
        let text: &str = &text;
        let width = self.measure(text).max(1);
        if width <= rect[2].max(1) as usize {
            self.draw_fit(rgba, canvas, text, rect, color, centered);
            return;
        }
        let mut native = crate::bitmap::Bitmap {
            width: width as u32,
            height: self.height as u32,
            rgba: vec![0; width * self.height * 4],
        };
        self.draw(&mut native.rgba, width, self.height, text, 0, 0, color);
        let fitted = native.resample(rect[2].max(1) as u32, self.height as u32);
        let y = rect[1] + ((rect[3] - self.height as i32) / 2).max(0);
        if capture(canvas, || {
            let mut pixels = Vec::new();
            for gy in 0..self.height {
                for gx in 0..fitted.width as usize {
                    let (x, y) = (rect[0] + gx as i32, y + gy as i32);
                    let alpha = fitted.rgba[(gy * fitted.width as usize + gx) * 4 + 3];
                    if alpha == 0
                        || x < 0
                        || y < 0
                        || x >= canvas[0] as i32
                        || y >= canvas[1] as i32
                    {
                        continue;
                    }
                    pixels.push((y as usize * canvas[0] + x as usize, alpha));
                }
            }
            TextOp {
                text: text.to_owned(),
                x: rect[0] as f32,
                y: y as f32,
                width: fitted.width as f32,
                height: self.height as f32,
                color,
                pixels,
            }
        }) {
            return;
        }
        for gy in 0..self.height {
            for gx in 0..fitted.width as usize {
                let (x, y) = (rect[0] + gx as i32, y + gy as i32);
                if x < 0 || y < 0 || x >= canvas[0] as i32 || y >= canvas[1] as i32 {
                    continue;
                }
                let alpha = fitted.rgba[(gy * fitted.width as usize + gx) * 4 + 3] as u16;
                if alpha == 0 {
                    continue;
                }
                let px = &mut rgba[(y as usize * canvas[0] + x as usize) * 4..][..4];
                if px[3] == 0 {
                    px[..3].copy_from_slice(&color);
                    px[3] = alpha as u8;
                } else {
                    for ch in 0..3 {
                        px[ch] = ((px[ch] as u16 * (255 - alpha) + color[ch] as u16 * alpha) / 255)
                            as u8;
                    }
                }
            }
        }
    }
}

/// A glyph cell being made: coverage 0 to the font's levels.
#[derive(Clone)]
struct Cell {
    w: i32,
    h: i32,
    px: Vec<u8>,
}
impl Cell {
    fn get(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return 0;
        }
        self.px[(y * self.w + x) as usize]
    }
    fn put(&mut self, x: i32, y: i32, v: u8) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let p = &mut self.px[(y * self.w + x) as usize];
        *p = (*p).max(v);
    }
    /// The covered pixels with `y` in `rows`.
    fn pixels(&self, rows: std::ops::Range<i32>) -> Vec<(i32, i32, u8)> {
        let mut out = Vec::new();
        for y in rows.start.max(0)..rows.end.min(self.h) {
            for x in 0..self.w {
                let v = self.get(x, y);
                if v > 0 {
                    out.push((x, y, v));
                }
            }
        }
        out
    }
    /// Left, top, right and bottom of what is covered, inclusive.
    fn bounds(&self) -> Option<[i32; 4]> {
        bounds(&self.pixels(0..self.h))
    }
    /// The cell moved by `dx` columns.
    fn shifted(&self, dx: i32) -> Self {
        let mut out = Self {
            px: vec![0; self.px.len()],
            ..*self
        };
        for (x, y, v) in self.pixels(0..self.h) {
            out.put(x + dx, y, v);
        }
        out
    }
}
fn bounds(pixels: &[(i32, i32, u8)]) -> Option<[i32; 4]> {
    let mut it = pixels.iter();
    let &(x, y, _) = it.next()?;
    Some(it.fold([x, y, x, y], |[l, t, r, b], &(x, y, _)| {
        [l.min(x), t.min(y), r.max(x), b.max(y)]
    }))
}
/// Twice the middle column of a box, to compare middles without halves.
fn middle2(b: [i32; 4]) -> i32 {
    b[0] + b[2]
}

/// What a letter of [`BEYOND`] adds to its base.
#[derive(Clone, Copy)]
enum Mark {
    Acute,
    Dot,
    Ogonek,
    Stroke,
}
/// The plain letter and the mark of each letter of [`BEYOND`].
fn parts(letter: char) -> (u8, Mark) {
    let (base, mark) = match letter.to_lowercase().next().unwrap_or(letter) {
        'ą' => (b'a', Mark::Ogonek),
        'ć' => (b'c', Mark::Acute),
        'ę' => (b'e', Mark::Ogonek),
        'ł' => (b'l', Mark::Stroke),
        'ń' => (b'n', Mark::Acute),
        'ś' => (b's', Mark::Acute),
        'ź' => (b'z', Mark::Acute),
        _ => (b'z', Mark::Dot),
    };
    if letter.is_uppercase() {
        (base.to_ascii_uppercase(), mark)
    } else {
        (base, mark)
    }
}
impl BitmapFont {
    fn cell(&self, code: u8) -> Cell {
        let glyph = self.glyphs[code as usize] as usize;
        let size = self.width * self.height;
        let px = self
            .pixels
            .get(glyph * size..(glyph + 1) * size)
            .map_or_else(|| vec![0; size], <[u8]>::to_vec);
        Cell {
            w: self.width as i32,
            h: self.height as i32,
            px,
        }
    }
    /// The accent of `accented` over its plain `base`: what it covers above
    /// the plain letter's top.
    fn accent(&self, accented: u8, base: u8) -> Vec<(i32, i32, u8)> {
        let top = self.cell(base).bounds().map_or(0, |b| b[1]);
        self.cell(accented).pixels(0..top)
    }
    /// The Polish letters ([`BEYOND`]), which the fonts of the game lack,
    /// made of each font's own pieces so they share its weight and its
    /// shading: the acute of «é» and «É» on c, n, s, z, the dot of «i»
    /// on z, the cedilla of «ç» turned round as the ogonek of a and e, and
    /// a stroke across the stem of l.
    fn compose(&mut self) {
        let levels = self.levels;
        let acute = [self.accent(0xe9, b'e'), self.accent(0xc9, b'E')];
        let plain = [self.cell(b'e').bounds(), self.cell(b'E').bounds()];
        // The dot of «i» above the top of «n»; a font of small capitals,
        // whose «i» has none, gives the left dot of «ë».
        let n_top = self.cell(b'n').bounds().map_or(0, |b| b[1]);
        let mut dot = self.cell(b'i').pixels(0..n_top);
        if dot.is_empty() {
            let diaeresis = self.accent(0xeb, b'e');
            if let Some(d) = bounds(&diaeresis) {
                dot = diaeresis
                    .into_iter()
                    .filter(|&(x, _, _)| x * 2 < middle2(d))
                    .collect();
            }
        }
        // The cedilla below the foot of «c», turned round.
        let c_foot = self.cell(b'c').bounds().map_or(0, |b| b[3]);
        let cedilla = self.cell(0xe7).pixels(c_foot + 1..self.height as i32);
        let ogonek: Vec<_> = match bounds(&cedilla) {
            Some(b) => cedilla
                .iter()
                .map(|&(x, y, v)| (b[0] + b[2] - x, y, v))
                .collect(),
            None => Vec::new(),
        };
        for (i, &letter) in BEYOND.iter().enumerate() {
            let capital = letter.is_uppercase();
            let (base, mark) = parts(letter);
            let mut cell = self.cell(base);
            let mut step = self.advances[base as usize];
            if let Some(b) = cell.bounds() {
                let k = usize::from(capital);
                match mark {
                    Mark::Acute => {
                        // Over the letter as «é» sits over «e».
                        let dx = plain[k].map_or(0, |p| (middle2(b) - middle2(p)) / 2);
                        for &(x, y, v) in &acute[k] {
                            cell.put(x + dx, y, v);
                        }
                    }
                    Mark::Dot => {
                        if let Some(d) = bounds(&dot) {
                            // Over the middle; on a capital, as low as the
                            // acute.
                            let dx = (middle2(b) - middle2(d)) / 2;
                            let dy = match (capital, bounds(&acute[1])) {
                                (true, Some(a)) => a[3] - d[3],
                                _ => 0,
                            };
                            for &(x, y, v) in &dot {
                                cell.put(x + dx, y + dy, v);
                            }
                        }
                    }
                    Mark::Ogonek => {
                        if let Some(o) = bounds(&ogonek) {
                            // Under the right foot of a, a little within
                            // e's.
                            let right = if base.eq_ignore_ascii_case(&b'e') {
                                b[2] - (b[2] - b[0]) / 4
                            } else {
                                b[2]
                            };
                            let (dx, dy) = (right - o[2], b[3] + 1 - o[1]);
                            for &(x, y, v) in &ogonek {
                                cell.put(x + dx, y + dy, v);
                            }
                        }
                    }
                    Mark::Stroke => {
                        // The stem: the columns covered on the letter's
                        // middle row (the left one of «L»).
                        let middle = (b[1] + b[3]) / 2;
                        let covered: Vec<i32> = (b[0]..=b[2])
                            .filter(|&x| cell.get(x, middle) * 2 > levels)
                            .collect();
                        let left = covered.first().copied().unwrap_or(b[0]);
                        let right = covered
                            .iter()
                            .copied()
                            .take_while(|&x| x <= left + 3)
                            .last()
                            .unwrap_or(left);
                        // An «l» of a stem alone, or one with a foot as «L»
                        // (the small capitals), which holds the stroke.
                        let bare = b[2] - b[0] <= right - left + 2;
                        let reach = 1 + (right - left + 2) / 3;
                        // Room for the stroke's left end within the cell.
                        let shift = (reach - left).max(0);
                        if shift > 0 {
                            cell = cell.shifted(shift);
                        }
                        let (left, right) = (left + shift, right + shift);
                        let height = (b[3] - b[1]) as f32;
                        let centre = b[1] as f32 + height * if bare { 0.58 } else { 0.55 };
                        let (x0, x1) = ((left - reach) as f32, (right + reach) as f32 + 1.);
                        let rise = (x1 - x0) * 0.45;
                        let half = ((right - left + 1) as f32 * 0.4).clamp(0.5, 1.2);
                        draw_line(
                            &mut cell,
                            [x0, centre + rise / 2.],
                            [x1, centre - rise / 2.],
                            half,
                            levels,
                        );
                        step += if !bare {
                            shift as u8
                        } else {
                            // The stroke juts out on both sides of «l».
                            (reach * 2 - 1).max(1) as u8
                        };
                    }
                }
            }
            let glyph = self.pixels.len() / (self.width * self.height);
            self.pixels.extend(&cell.px);
            self.beyond[i] = (glyph, step);
        }
    }
}
/// A line of half width `half` from `a` to `b` in pixel coordinates, its
/// edges shaded in the font's levels.
fn draw_line(cell: &mut Cell, a: [f32; 2], b: [f32; 2], half: f32, levels: u8) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length2 = (dx * dx + dy * dy).max(1e-6);
    for x in (a[0].min(b[0]) - 2.) as i32..=(a[0].max(b[0]) + 2.) as i32 {
        for y in (a[1].min(b[1]) - 2.) as i32..=(a[1].max(b[1]) + 2.) as i32 {
            // Distance from the pixel's centre to the segment.
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let t = (((px - a[0]) * dx + (py - a[1]) * dy) / length2).clamp(0., 1.);
            let (qx, qy) = (a[0] + t * dx, a[1] + t * dy);
            let distance = ((px - qx).powi(2) + (py - qy).powi(2)).sqrt();
            let cover = (half + 0.5 - distance).clamp(0., 1.);
            let v = (cover * levels as f32).round() as u8;
            if v > 0 {
                cell.put(x, y, v);
            }
        }
    }
}

/// The cover of `text` in Windows' Arial Bold, `height` pixels from the top
/// of its capitals to the foot of its descenders (`tools/text_win.cpp`), for
/// «Testi in alta definizione»: its width, its cell height and the covers.
#[cfg(windows)]
pub fn arial(text: &str, height: u32) -> Option<(u32, u32, Vec<u8>)> {
    unsafe extern "C" {
        fn tz_text_size(
            text: *const u16,
            len: i32,
            height: i32,
            width: *mut i32,
            cell: *mut i32,
        ) -> i32;
        fn tz_text_draw(
            text: *const u16,
            len: i32,
            height: i32,
            out: *mut u8,
            width: i32,
            cell: i32,
        ) -> i32;
    }
    let wide: Vec<u16> = text.encode_utf16().collect();
    let (mut width, mut cell) = (0, 0);
    // SAFETY: the text outlives the calls, the buffer has width × cell bytes.
    let sized = unsafe {
        tz_text_size(
            wide.as_ptr(),
            wide.len() as i32,
            height as i32,
            &mut width,
            &mut cell,
        )
    };
    if sized == 0 || width <= 0 || cell <= 0 {
        return None;
    }
    let mut out = vec![0u8; (width * cell) as usize];
    let drawn = unsafe {
        tz_text_draw(
            wide.as_ptr(),
            wide.len() as i32,
            height as i32,
            out.as_mut_ptr(),
            width,
            cell,
        )
    };
    (drawn != 0).then_some((width as u32, cell as u32, out))
}
#[cfg(not(windows))]
pub fn arial(_: &str, _: u32) -> Option<(u32, u32, Vec<u8>)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each Polish letter is its plain one with something added, as wide or
    /// a little wider, in every font of the game.
    #[test]
    #[ignore = "Requires this installation's original game data"]
    fn the_polish_letters_are_their_plain_ones_with_their_marks() {
        let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let assets = crate::assets::GameAssets::open(&source).unwrap();
        for name in [
            "ARIAL10_AA",
            "ARIAL12B_AA",
            "ARIAL14B_AA",
            "AV10",
            "CGB_22B",
            "SMALL",
        ] {
            let font = BitmapFont::decode(&assets.get(&format!("LOCAL/FONTS/{name}.FNT")).unwrap())
                .unwrap();
            let size = font.width * font.height;
            let ink = |glyph: usize| -> usize {
                font.pixels[glyph * size..(glyph + 1) * size]
                    .iter()
                    .map(|&v| v as usize)
                    .sum()
            };
            for &letter in &BEYOND {
                let (base, _) = parts(letter);
                let (glyph, step) = font.slot(letter);
                let (plain, plain_step) = font.slot(base as char);
                assert!(ink(glyph) > ink(plain), "{name}: {letter} senza segno");
                assert!(
                    (plain_step..=plain_step + 3).contains(&step),
                    "{name}: {letter} largo {step}"
                );
            }
            // «œ» and «Œ», and the Portuguese letters, are the font's own,
            // not «?».
            for letter in "œŒãõÃÕâêôÂÊÔàÀçÇ".chars() {
                assert_ne!(font.slot(letter).0, font.slot('?').0, "{name}: {letter}");
            }
        }
    }
}
