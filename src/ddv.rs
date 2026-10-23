//! The American release's movies (`MOVIES\*.DDV` in `Packs\movies.wdt`, or
//! loose in a digital release), in Digital Dialect's «Masher» format that
//! the credits name. The Italian edition installs no movie, but its engine
//! keeps the player (sequence `0x476318–0x4764ac`, playback `0x47fc70`,
//! opening `0x543970`, decoder from `0x5488a0`), so the decoder's tables come
//! from a `Tzar.exe`: the Italian one, or any build that is not encrypted.
//! They are read from the installation's executable only where the
//! installation is used as it is: the release builder turns these movies
//! into MP4 ([`crate::media`]) and the portable copy holds nothing of
//! `Tzar.exe`.
//!
//! A movie is a header, the sizes of its chunks, the sound of the first
//! `interleave` frames, then one chunk per frame: a picture and, while
//! frames are left beyond the interleave, the sound of a frame to come.
//! Pictures are 16×16 macroblocks of DCT blocks (Cb, Cr and four Y) coded
//! with the MPEG-1 table of run and level; a block flagged as such adds its
//! levels to those of the previous frame instead of replacing them. Sound
//! is a three-sample predictor with a residual of up to three widths per
//! channel, optionally through a logarithmic table.
use crate::{Result, audio::Pcm, bitmap::Bitmap, invalid};

const END_OF_BLOCK: u16 = 0xFE00;
const ESCAPE: u16 = 0x7C1F;
/// The 11-bit value after an end of block that ends the picture.
const END_OF_PICTURE: u32 = 0x3FF;

/// Where the tables lie in the Italian 1.11 `Tzar.exe` (file offsets of its
/// `.data`) and their sizes, in the order [`Tables`] reads them: the
/// quantization of luminance and of colour (`0x548510`: coefficient `k`,
/// 1–63, takes the entry `k − 1` of each), the zigzag order, the codes of 13
/// bits and those of 9 bits after eight zeros.
const PARTS: [(usize, usize); 5] = [
    (0x1971f8, 4 * 63),
    (0x1972f8, 4 * 63),
    (0x1973f4, 4 * 64),
    (0x197ef8, 8 * 8192),
    (0x1a7ef8, 4 * 512),
];
const LUMA: usize = 0;
const CHROMA: usize = LUMA + PARTS[0].1;
const ZIGZAG: usize = CHROMA + PARTS[1].1;
const SHORT_CODES: usize = ZIGZAG + PARTS[2].1;
const LONG_CODES: usize = SHORT_CODES + PARTS[3].1;

/// The tables of an executable, one after the other: where the Italian
/// 1.11 one has them, or wherever another build keeps the same block, found
/// by its zigzag order. An encrypted executable (the American CD's) has
/// none to read.
fn extract(exe: &[u8]) -> Result<Vec<u8>> {
    let shifted = |shift: isize| {
        let mut out = Vec::new();
        for (at, len) in PARTS {
            let at = at.checked_add_signed(shift)?;
            out.extend_from_slice(exe.get(at..at + len)?);
        }
        Tables::parse(&out).is_ok().then_some(out)
    };
    if let Some(out) = shifted(0) {
        return Ok(out);
    }
    let start: Vec<u8> = [0u32, 1, 8, 16]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    exe.windows(start.len())
        .enumerate()
        .filter(|(_, w)| *w == start.as_slice())
        .find_map(|(at, _)| shifted(at as isize - PARTS[2].0 as isize))
        .ok_or_else(|| invalid("Tzar.exe non riconosciuto: tabelle dei filmati assenti"))
}

/// The decoder's tables, as `Tzar.exe` holds them.
pub struct Tables {
    /// 13 bits of the stream: bits used and up to three run/level words.
    short: Vec<(u8, [u16; 3])>,
    /// After eight zero bits, 9 bits: bits used and the word.
    long: Vec<(u8, u16)>,
    zigzag: [usize; 64],
    luma: [i32; 63],
    chroma: [i32; 63],
}
impl Tables {
    /// The tables of the installation's executable.
    pub fn of_installation(source: &std::path::Path) -> Result<Self> {
        Self::read(&std::fs::read(source.join("Tzar.exe"))?)
    }
    /// The tables of an executable, wherever it keeps them.
    pub fn read(exe: &[u8]) -> Result<Self> {
        Self::parse(&extract(exe)?)
    }
    fn parse(data: &[u8]) -> Result<Self> {
        let byte = |at: usize| {
            data.get(at)
                .copied()
                .ok_or_else(|| invalid("Tabelle dei filmati troncate"))
        };
        let word = |at: usize| Result::<u16>::Ok(u16::from_le_bytes([byte(at)?, byte(at + 1)?]));
        let dword = |at: usize| {
            Result::<u32>::Ok(u32::from_le_bytes([
                byte(at)?,
                byte(at + 1)?,
                byte(at + 2)?,
                byte(at + 3)?,
            ]))
        };
        let mut short = Vec::with_capacity(8192);
        for i in 0..8192 {
            let at = SHORT_CODES + 8 * i;
            short.push((byte(at)?, [word(at + 2)?, word(at + 4)?, word(at + 6)?]));
        }
        let mut long = Vec::with_capacity(512);
        for i in 0..512 {
            let at = LONG_CODES + 4 * i;
            long.push((byte(at)?, word(at + 2)?));
        }
        let mut zigzag = [0; 64];
        let mut luma = [0; 63];
        let mut chroma = [0; 63];
        for k in 0..64 {
            zigzag[k] = dword(ZIGZAG + 4 * k)? as usize;
        }
        for k in 0..63 {
            luma[k] = dword(LUMA + 4 * k)? as i32;
            chroma[k] = dword(CHROMA + 4 * k)? as i32;
        }
        // The decoder's tables as the Italian 1.11 executable has them:
        // anything else is not them.
        let mut seen = [false; 64];
        let permutation = zigzag
            .iter()
            .all(|&z| z < 64 && !std::mem::replace(&mut seen[z], true));
        if !permutation
            || zigzag[..4] != [0, 1, 8, 16]
            || luma[..3] != [12, 11, 10]
            || chroma[..3] != [18, 18, 24]
            || short[4096] != (2, [END_OF_BLOCK, 0, 0])
            || short[6144] != (3, [1, 0, 0])
        {
            return Err(invalid("Tabelle dei filmati non riconosciute"));
        }
        Ok(Self {
            short,
            long,
            zigzag,
            luma,
            chroma,
        })
    }
}

fn u32_at(data: &[u8], at: usize) -> Result<u32> {
    crate::u32_at(data, at)
}

/// A movie, its pictures decoded one after the other.
pub struct Movie {
    data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Where each picture's stream lies in `data`.
    pictures: Vec<std::ops::Range<usize>>,
    /// The whole sound, decoded at once.
    pub audio: Option<Pcm>,
    next: usize,
    /// Each block's coefficients: the level in the top ten bits, the
    /// dequantized value in the low word (`0x548580`).
    blocks: Vec<[i32; 64]>,
    scale: Option<i32>,
    luma: [i32; 64],
    chroma: [i32; 64],
    picture: Vec<[u8; 3]>,
}
impl Movie {
    pub fn open(data: Vec<u8>) -> Result<Self> {
        if data.get(..4) != Some(b"DDV\0") || u32_at(&data, 4)? != 1 {
            return Err(invalid("Filmato DDV non riconosciuto"));
        }
        let contains = u32_at(&data, 8)?;
        let fps = u32_at(&data, 12)?;
        let frames = u32_at(&data, 16)? as usize;
        let mut at = 20;
        if contains & 1 == 0 || fps == 0 || frames == 0 || frames > 100_000 {
            return Err(invalid("Filmato DDV senza immagini"));
        }
        let width = u32_at(&data, at + 4)?;
        let height = u32_at(&data, at + 8)?;
        at += 24;
        if width == 0 || height == 0 || width > 1024 || height > 1024 {
            return Err(invalid("Dimensioni del filmato DDV non valide"));
        }
        let sound = if contains & 2 != 0 {
            let format = u32_at(&data, at)?;
            let rate = u32_at(&data, at + 4)?;
            let samples = u32_at(&data, at + 12)? as usize;
            let interleave = u32_at(&data, at + 16)? as usize;
            at += 20;
            // 0x543b9f: bit 0 two channels, bit 1 sixteen bits.
            if format & 2 == 0 || interleave > frames || samples > 100_000 {
                return Err(invalid("Suono del filmato DDV non gestito"));
            }
            Some((
                if format & 1 != 0 { 2 } else { 1 },
                rate,
                samples,
                interleave,
            ))
        } else {
            None
        };
        let interleave = sound.map_or(0, |s| s.3);
        let mut sizes = Vec::with_capacity(interleave + frames);
        for i in 0..interleave + frames {
            sizes.push(u32_at(&data, at + 4 * i)? as usize);
        }
        at += 4 * sizes.len();
        let mut sound_chunks = Vec::new();
        for &size in &sizes[..interleave] {
            sound_chunks.push(at..at + size);
            at += size;
        }
        let mut pictures = Vec::with_capacity(frames);
        for (f, &size) in sizes[interleave..].iter().enumerate() {
            if sound.is_some() {
                let start = at + 4;
                let offset = u32_at(&data, at)? as usize;
                let end = at + 4 + size;
                pictures.push(start..(start + offset).min(end));
                if frames - f > interleave {
                    sound_chunks.push(start + offset..end);
                }
                at = end;
            } else {
                pictures.push(at..at + size);
                at += size;
            }
        }
        if at > data.len() {
            return Err(invalid("Filmato DDV troncato"));
        }
        let audio = match sound {
            Some((channels, rate, samples, _)) => {
                let mut pcm = Vec::with_capacity(sound_chunks.len() * samples * channels);
                for chunk in &sound_chunks {
                    decode_sound(&data[chunk.clone()], samples, channels, &mut pcm);
                }
                Some(Pcm {
                    rate,
                    channels: channels as u16,
                    bits: 16,
                    bytes: pcm.iter().flat_map(|s| s.to_le_bytes()).collect(),
                })
            }
            None => None,
        };
        let (columns, rows) = (width.div_ceil(16) as usize, height.div_ceil(16) as usize);
        Ok(Self {
            data,
            width,
            height,
            fps,
            pictures,
            audio,
            next: 0,
            blocks: vec![[0; 64]; columns * rows * 6],
            scale: None,
            luma: [16; 64],
            chroma: [16; 64],
            picture: vec![[0; 3]; columns * 16 * rows * 16],
        })
    }
    pub fn frames(&self) -> usize {
        self.pictures.len()
    }
    /// The next picture, until the last.
    pub fn next_frame(&mut self, tables: &Tables) -> Result<Option<Bitmap>> {
        let Some(range) = self.pictures.get(self.next).cloned() else {
            return Ok(None);
        };
        self.next += 1;
        let (scale, words) = decode_stream(&self.data[range], tables)?;
        if self.scale != Some(scale) {
            // 0x548510: the tables scaled by the picture's first word.
            self.scale = Some(scale);
            self.luma = [16; 64];
            self.chroma = [16; 64];
            if scale > 0 {
                for k in 1..64 {
                    self.luma[k] = scale * tables.luma[k - 1];
                    self.chroma[k] = scale * tables.chroma[k - 1];
                }
            }
        }
        let columns = self.width.div_ceil(16) as usize;
        let rows = self.height.div_ceil(16) as usize;
        let stride = columns * 16;
        let mut cursor = 0;
        let mut block = 0;
        // Macroblocks column by column (0x543e60's loops).
        for mx in 0..columns {
            for my in 0..rows {
                let mut out = [[0i32; 64]; 6];
                for (b, out) in out.iter_mut().enumerate() {
                    let luminance = b >= 2;
                    let quant = if luminance { &self.luma } else { &self.chroma };
                    cursor = decode_block(
                        &words,
                        cursor,
                        &mut self.blocks[block],
                        luminance,
                        quant,
                        tables,
                    );
                    idct(&self.blocks[block], out);
                    block += 1;
                }
                let [cb, cr, y1, y2, y3, y4] = &out;
                for y in 0..16 {
                    for x in 0..16 {
                        let luma = match (x < 8, y < 8) {
                            (true, true) => y1[y * 8 + x],
                            (false, true) => y2[y * 8 + x - 8],
                            (true, false) => y3[(y - 8) * 8 + x],
                            (false, false) => y4[(y - 8) * 8 + x - 8],
                        } as f32;
                        let c = (y / 2) * 8 + x / 2;
                        let (u, v) = (cb[c] as f32, cr[c] as f32);
                        let rgb = [
                            luma + 1.402 * v,
                            luma - 0.344_14 * u - 0.714_14 * v,
                            luma + 1.772 * u,
                        ]
                        .map(|c| c.round().clamp(0., 255.) as u8);
                        self.picture[(my * 16 + y) * stride + mx * 16 + x] = rgb;
                    }
                }
            }
        }
        let mut rgba = Vec::with_capacity((self.width * self.height * 4) as usize);
        for y in 0..self.height as usize {
            for x in 0..self.width as usize {
                let [r, g, b] = self.picture[y * stride + x];
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        Ok(Some(Bitmap {
            width: self.width,
            height: self.height,
            rgba,
        }))
    }
}

/// The stream read from the most significant bit of 16-bit words.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    acc: u64,
    count: u32,
}
impl Bits<'_> {
    fn fill(&mut self, n: u32) {
        while self.count < n {
            let word = match self.data.get(self.at..self.at + 2) {
                Some(w) => u16::from_le_bytes([w[0], w[1]]),
                None => 0,
            };
            self.at += 2;
            self.acc = (self.acc << 16) | word as u64;
            self.count += 16;
        }
    }
    fn peek(&mut self, n: u32) -> u32 {
        self.fill(n);
        ((self.acc >> (self.count - n)) & ((1u64 << n) - 1)) as u32
    }
    fn skip(&mut self, n: u32) {
        self.fill(n);
        self.count -= n;
    }
    fn read(&mut self, n: u32) -> u32 {
        let v = self.peek(n);
        self.count -= n;
        v
    }
    fn exhausted(&self) -> bool {
        self.at > self.data.len() + 8
    }
}

/// A picture's quantizer scale and its run/level words (`0x5488a0`): each
/// block a DC word (ten bits and the flag), its codes, an end of block.
fn decode_stream(data: &[u8], tables: &Tables) -> Result<(i32, Vec<u16>)> {
    let scale = data
        .get(..2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]) as i32)
        .ok_or_else(|| invalid("Immagine DDV vuota"))?;
    let mut bits = Bits {
        data: &data[2..],
        at: 0,
        acc: 0,
        count: 0,
    };
    let mut words = vec![bits.read(11) as u16];
    loop {
        if bits.exhausted() {
            return Err(invalid("Immagine DDV senza fine"));
        }
        let index = bits.peek(13) as usize;
        if index < 32 {
            let long = bits.peek(17) as usize;
            bits.skip(8);
            let (used, word) = tables.long[long];
            if used == 0 {
                return Err(invalid("Codice DDV non valido"));
            }
            bits.skip(used as u32);
            words.push(word);
            continue;
        }
        let (used, symbols) = tables.short[index];
        bits.skip(used as u32);
        for word in symbols {
            if word == 0 {
                break;
            }
            if word == ESCAPE {
                words.push(bits.read(16) as u16);
                break;
            }
            words.push(word);
            if word == END_OF_BLOCK {
                let dc = bits.read(11);
                if dc == END_OF_PICTURE {
                    return Ok((scale, words));
                }
                words.push(dc as u16);
            }
        }
    }
}

/// One block's coefficients from the words at `cursor` (`0x548580`); the
/// position after it.
fn decode_block(
    words: &[u16],
    mut cursor: usize,
    block: &mut [i32; 64],
    luminance: bool,
    quant: &[i32; 64],
    tables: &Tables,
) -> usize {
    let word = |i: usize| words.get(i).copied().unwrap_or(END_OF_BLOCK);
    while word(cursor) == END_OF_BLOCK && cursor < words.len() {
        cursor += 1;
    }
    let dc = word(cursor);
    cursor += 1;
    // Ten bits of DC from bit 1; luminance carries the level shift of 128.
    block[0] = ((luminance as i32) << 10) + 2 * (((dc as i32) << 21) >> 22);
    let accumulate = dc & 1 != 0;
    if !accumulate {
        block[1..].fill(0);
    }
    let mut k = 0usize;
    while k < 63 {
        let w = word(cursor);
        cursor += 1;
        if w == END_OF_BLOCK {
            break;
        }
        let run = (w >> 10) as usize;
        k += run;
        if k >= 63 {
            break;
        }
        let position = tables.zigzag[k + 1];
        let shifted = ((w as u32) << 22) as i32;
        let level = if accumulate {
            block[position].wrapping_add(shifted)
        } else {
            shifted
        };
        let value = (quant[k + 1].wrapping_mul(level >> 22) + 4) >> 3;
        block[position] = (level & !0xFFFF) | (value & 0xFFFF);
        k += 1;
    }
    cursor
}

/// The inverse DCT of the low words of a block: the transform of `0x548140`
/// (MMX there) in two integer passes, shifts 11 and 18.
fn idct(block: &[i32; 64], out: &mut [i32; 64]) {
    let input: [i64; 64] = std::array::from_fn(|i| (block[i] as i16) as i64);
    let mut temp = [0i64; 64];
    half(&input, &mut temp, 8, 1, 11);
    let mut result = [0i64; 64];
    half(&temp, &mut result, 1, 8, 18);
    for i in 0..64 {
        out[i] = result[i] as i32;
    }
}
fn half(s: &[i64; 64], d: &mut [i64; 64], pitch: usize, step: usize, shift: u32) {
    for i in 0..8 {
        let at = |k: usize| s[k * pitch + i * step];
        let t4 = at(0) * 8192 + at(2) * 10703 + at(4) * 8192 + at(6) * 4433;
        let t5 = at(0) * 8192 + at(2) * 4433 - at(4) * 8192 - at(6) * 10704;
        let t6 = at(0) * 8192 - at(2) * 4433 - at(4) * 8192 + at(6) * 10704;
        let t7 = at(0) * 8192 - at(2) * 10703 + at(4) * 8192 - at(6) * 4433;
        let t0 = at(1) * 11363 + at(3) * 9633 + at(5) * 6437 + at(7) * 2260;
        let t1 = at(1) * 9633 - at(3) * 2259 - at(5) * 11362 - at(7) * 6436;
        let t2 = at(1) * 6437 - at(3) * 11362 + at(5) * 2261 + at(7) * 9633;
        let t3 = at(1) * 2260 - at(3) * 6436 + at(5) * 9633 - at(7) * 11363;
        let mut put = |k: usize, v: i64| d[k * pitch + i * step] = v >> shift;
        put(0, t4 + t0);
        put(1, t5 + t1);
        put(2, t6 + t2);
        put(3, t7 + t3);
        put(4, t7 - t3);
        put(5, t6 - t2);
        put(6, t5 - t1);
        put(7, t4 - t0);
    }
}

/// The sound read from the least significant bit of 16-bit words.
struct SoundBits<'a> {
    data: &'a [u8],
    at: usize,
    acc: u64,
    count: u32,
}
impl SoundBits<'_> {
    fn refill(&mut self) {
        if self.count <= 16 {
            let word = match self.data.get(self.at..self.at + 2) {
                Some(w) => u16::from_le_bytes([w[0], w[1]]),
                None => 0,
            };
            self.at += 2;
            self.acc |= (word as u64) << self.count;
            self.count += 16;
        }
    }
    fn read(&mut self, n: u32) -> u32 {
        let n = n.min(16);
        let v = (self.acc & ((1u64 << n) - 1)) as u32;
        self.acc >>= n;
        self.count -= n;
        self.refill();
        v
    }
    /// After a channel the stream starts again on a byte.
    fn align(&mut self) {
        let drop = self.count & 7;
        self.acc >>= drop;
        self.count -= drop;
        self.refill();
    }
}
/// Bit lengths of 0–255 (`0xce922c`).
fn bit_length(i: usize) -> u32 {
    usize::BITS - i.leading_zeros()
}
/// The logarithmic table's compression (`0x549040`).
fn compress(value: i16) -> i32 {
    let magnitude = (value as i32).unsigned_abs() as u16 as i16 as i32;
    let e = bit_length((magnitude >> 7).clamp(0, 255) as usize);
    let r = ((e << 7) as u16 | (magnitude >> e) as u16) as i32;
    if value < 0 { -r } else { r }
}
/// Its expansion (`0x548b30`), with the x86 masking of the shifts.
fn expand(value: i16) -> i32 {
    let magnitude = (value as i32).unsigned_abs() as u16 as i16 as i32;
    let e = (magnitude >> 7) as u32;
    let low = (((magnitude & 0x7f) as u32) << (e & 31)) as u16;
    let high = (1u32 << (e.wrapping_sub(2) & 31)) as u16;
    let r = (low | high) as i32;
    if value < 0 { -r } else { r }
}
/// One chunk of sound (`0x547980`, `0x5490e0`): each channel in turn.
fn decode_sound(data: &[u8], samples: usize, channels: usize, out: &mut Vec<i16>) {
    let first = out.len();
    out.resize(first + samples * channels, 0);
    let mut bits = SoundBits {
        data,
        at: 4,
        acc: data
            .get(..4)
            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) as u64,
        count: 32,
    };
    for channel in 0..channels {
        let table = bits.read(16) != 0;
        let widths = [bits.read(16), bits.read(16), bits.read(16)];
        let mut history = [0i32; 3];
        for (i, h) in history.iter_mut().enumerate() {
            *h = bits.read(16) as u16 as i16 as i32;
            if i < samples {
                out[first + i * channels + channel] = *h as i16;
            }
        }
        // history: the samples three, two and one before.
        for i in 3..samples {
            let level = |bits: &mut SoundBits, width: u32| -> (i32, bool) {
                let width = width.clamp(1, 16);
                let mask = 1i32 << (width - 1);
                let v = bits.read(width) as u16 as i16 as i32;
                (v, v == mask)
            };
            let sign = |v: i32, width: u32| {
                let mask = 1i32 << (width.clamp(1, 16) - 1);
                if v & mask != 0 { -(v & !mask) } else { v }
            };
            let (mut residual, escape) = level(&mut bits, widths[0]);
            if escape {
                let (v, escape) = level(&mut bits, widths[1]);
                residual = if escape {
                    let (v, _) = level(&mut bits, widths[2]);
                    sign(v, widths[2])
                } else {
                    sign(v, widths[1])
                };
            } else {
                residual = sign(residual, widths[0]);
            }
            let [p3, p2, p1] = history;
            let predicted = (p3 + 5 * p1 - 4 * p2) >> 1;
            let sample = if table {
                expand((residual + compress(predicted as i16)) as i16) as i16
            } else {
                (predicted + residual) as i16
            };
            history = [p2, p1, sample as i32];
            out[first + i * channels + channel] = sample;
        }
        bits.align();
    }
}

/// The American release's movies, wherever they are: its archive in the
/// installation's `Packs` or in the extracted CD of `USA`, or the loose
/// `MOVIES` folder of a digital release.
pub fn american_movies(source: &std::path::Path) -> Option<crate::archive::Archive> {
    use crate::archive::Archive;
    [
        source.join("Packs/movies.wdt"),
        source.join("USA/CD/SETUP/Tzar/Packs/movies.wdt"),
    ]
    .into_iter()
    .find(|p| crate::vfs::is_file(p))
    .and_then(|p| Archive::open(&p).ok())
    .or_else(|| {
        source
            .is_dir()
            .then(|| Archive::folder(source, &["MOVIES"]).ok())
            .flatten()
    })
    .filter(|a| !a.entries.is_empty())
}
/// The movies the original plays at start (0x476318–0x4764ac), as the
/// American release has them: TalonSoft, «PAN», Haemimont, the intro.
pub const START: [&str; 4] = [
    "MOVIES/TALON.DDV",
    "MOVIES/PAN.DDV",
    "MOVIES/HMM.DDV",
    "MOVIES/TZAR.DDV",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logarithmic_table_round_trips() {
        for v in [-2000i16, -129, -1, 0, 1, 127, 128, 255, 1000, 2047] {
            let e = expand(compress(v) as i16);
            // The table keeps 7 bits of mantissa.
            assert!(
                (e - v as i32).abs() <= (v as i32).abs() / 64 + 1,
                "{v} -> {e}"
            );
        }
    }

    /// The installed Italian 1.11 `Tzar.exe` holds the tables, recognized.
    #[test]
    #[ignore = "Requires the installed Tzar.exe"]
    fn the_tables_of_tzar_exe_are_recognized() {
        let source = std::env::var_os("TZAR_DATA_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
        Tables::of_installation(&source).unwrap();
        assert!(Tables::parse(&[0; 100]).is_err());
    }

    #[test]
    #[ignore = "Requires the installed Tzar.exe"]
    fn the_tables_are_found_wherever_a_build_keeps_them() {
        let exe = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("Tzar.exe"),
        )
        .unwrap();
        let original = Tables::read(&exe).unwrap();
        // The same block at another place, as in another build.
        let mut moved = vec![0x90; 0x1234];
        moved.extend_from_slice(&exe[PARTS[0].0..PARTS[4].0 + PARTS[4].1]);
        let found = Tables::read(&moved).unwrap();
        assert_eq!(found.zigzag, original.zigzag);
        assert_eq!(found.short, original.short);
        assert_eq!(found.long, original.long);
        assert!(Tables::read(&exe[..0x1000]).is_err());
    }

    #[test]
    fn a_flat_block_gives_its_dc() {
        let mut block = [0i32; 64];
        block[0] = 1024;
        let mut out = [0; 64];
        idct(&block, &mut out);
        assert!(out.iter().all(|&v| v == 128));
    }
}
