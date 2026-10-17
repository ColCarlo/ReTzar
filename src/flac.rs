//! FLAC, the lossless sound of the portable copy's pack: the WAVE files of
//! the game (16 or 8 bits, one or two channels) stored as FLAC streams and
//! given back sample for sample.
//!
//! The encoder writes standard FLAC (fixed predictors of order 0 to 4,
//! partitioned Rice residuals, the four stereo decorrelations, CRC-8 frame
//! headers and CRC-16 frames); the decoder reads what the encoder writes and
//! checks every CRC.
use crate::{Result, invalid};

/// Samples per channel in a frame.
const BLOCK: usize = 4096;
const MAX_PARTITION_ORDER: u32 = 8;

/// Interleaved samples, signed (8-bit WAVE samples minus 128).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sound {
    pub rate: u32,
    pub channels: u16,
    pub bits: u16,
    pub samples: Vec<i32>,
}

struct Writer {
    bytes: Vec<u8>,
    acc: u64,
    used: u32,
}
impl Writer {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            acc: 0,
            used: 0,
        }
    }
    fn put(&mut self, value: u64, bits: u32) {
        debug_assert!(bits <= 32);
        if bits == 0 {
            return;
        }
        self.acc = (self.acc << bits) | (value & ((1u64 << bits) - 1));
        self.used += bits;
        while self.used >= 8 {
            self.used -= 8;
            self.bytes.push((self.acc >> self.used) as u8);
        }
        self.acc &= (1u64 << self.used) - 1;
    }
    fn signed(&mut self, value: i64, bits: u32) {
        self.put(value as u64, bits);
    }
    fn unary(&mut self, zeros: u32) {
        let mut left = zeros;
        while left >= 32 {
            self.put(0, 32);
            left -= 32;
        }
        self.put(1, left + 1);
    }
    fn align(&mut self) {
        if self.used > 0 {
            self.put(0, 8 - self.used);
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    bit: u32,
}
impl<'a> Reader<'a> {
    fn byte(&self) -> Result<u8> {
        self.bytes
            .get(self.at)
            .copied()
            .ok_or_else(|| invalid("FLAC troncato"))
    }
    fn get(&mut self, bits: u32) -> Result<u64> {
        let mut value = 0u64;
        let mut need = bits;
        while need > 0 {
            let avail = 8 - self.bit;
            let take = avail.min(need);
            let chunk = (u64::from(self.byte()?) >> (avail - take)) & ((1u64 << take) - 1);
            value = (value << take) | chunk;
            self.bit += take;
            need -= take;
            if self.bit == 8 {
                self.bit = 0;
                self.at += 1;
            }
        }
        Ok(value)
    }
    fn signed(&mut self, bits: u32) -> Result<i64> {
        let v = self.get(bits)?;
        Ok(if bits > 0 && v >> (bits - 1) & 1 == 1 {
            v as i64 - (1i64 << bits)
        } else {
            v as i64
        })
    }
    fn unary(&mut self) -> Result<u32> {
        let mut zeros = 0;
        loop {
            // The bits left in this byte, at its top.
            let rest = self.byte()? << self.bit;
            if rest == 0 {
                zeros += 8 - self.bit;
                self.bit = 0;
                self.at += 1;
                continue;
            }
            let lead = rest.leading_zeros();
            zeros += lead;
            self.bit += lead + 1;
            if self.bit == 8 {
                self.bit = 0;
                self.at += 1;
            }
            return Ok(zeros);
        }
    }
    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.at += 1;
        }
    }
}

fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in bytes {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}
fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in bytes {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// The residual of the fixed predictor of `order` (0 to 4).
fn residual(signal: &[i64], order: usize) -> Vec<i64> {
    (order..signal.len())
        .map(|i| {
            let s = |k: usize| signal[i - k];
            signal[i]
                - match order {
                    0 => 0,
                    1 => s(1),
                    2 => 2 * s(1) - s(2),
                    3 => 3 * s(1) - 3 * s(2) + s(3),
                    _ => 4 * s(1) - 6 * s(2) + 4 * s(3) - s(4),
                }
        })
        .collect()
}
fn zigzag(r: i64) -> u64 {
    ((r << 1) ^ (r >> 63)) as u64
}
/// The best Rice parameter of a run of folded residuals and its cost.
fn rice_cost(folded: &[u64], max_parameter: u32) -> (u32, u64) {
    let sum: u64 = folded.iter().sum();
    let n = folded.len() as u64;
    let mut best = (0, u64::MAX);
    if n == 0 {
        return (0, 0);
    }
    // Around the estimate from the mean.
    let mean = sum / n;
    let guess = 64 - mean.leading_zeros();
    let low = guess.saturating_sub(2).min(max_parameter);
    for k in low..=(guess + 1).min(max_parameter) {
        let cost: u64 = folded.iter().map(|&u| (u >> k) + 1 + u64::from(k)).sum();
        if cost < best.1 {
            best = (k, cost);
        }
    }
    best
}
/// A Rice coding: partition order, parameters and the bits it takes.
type Partition = (u32, Vec<u32>, u64);
/// The cheapest Rice coding of a residual whose block has `block` samples.
fn partition(res: &[i64], block: usize, order: usize) -> Partition {
    let folded: Vec<u64> = res.iter().map(|&r| zigzag(r)).collect();
    let mut best: Partition = (0, Vec::new(), u64::MAX);
    for p in 0..=MAX_PARTITION_ORDER {
        let parts = 1usize << p;
        if !block.is_multiple_of(parts) || block / parts <= order {
            break;
        }
        let size = block / parts;
        let mut params = Vec::with_capacity(parts);
        let mut cost = 0u64;
        let mut at = 0;
        for i in 0..parts {
            let n = if i == 0 { size - order } else { size };
            let (k, c) = rice_cost(&folded[at..at + n], 30);
            params.push(k);
            cost += c + 5;
            at += n;
        }
        if cost < best.2 {
            best = (p, params, cost);
        }
    }
    best
}

/// One channel of a frame, written at `bits` per sample.
fn subframe(w: &mut Writer, signal: &[i64], bits: u32) {
    if signal.iter().all(|&s| s == signal[0]) {
        w.put(0, 1);
        w.put(0b000000, 6);
        w.put(0, 1);
        w.signed(signal[0], bits);
        return;
    }
    let verbatim = signal.len() as u64 * u64::from(bits);
    // The cheapest order: its residual, partitioning and bits in all.
    let mut best: Option<(usize, Vec<i64>, Partition, u64)> = None;
    for order in 0..=4.min(signal.len() - 1) {
        let res = residual(signal, order);
        let p = partition(&res, signal.len(), order);
        let cost = p.2 + order as u64 * u64::from(bits);
        if best.as_ref().is_none_or(|b| cost < b.3) {
            best = Some((order, res, p, cost));
        }
    }
    let (order, res, (p, params, _), cost) = best.unwrap();
    if cost >= verbatim {
        w.put(0, 1);
        w.put(0b000001, 6);
        w.put(0, 1);
        for &s in signal {
            w.signed(s, bits);
        }
        return;
    }
    w.put(0, 1);
    w.put(0b001000 | order as u64, 6);
    w.put(0, 1);
    for &s in &signal[..order] {
        w.signed(s, bits);
    }
    let wide = params.iter().any(|&k| k > 14);
    w.put(u64::from(wide), 2);
    w.put(u64::from(p), 4);
    let size = signal.len() >> p;
    let mut at = 0;
    for (i, &k) in params.iter().enumerate() {
        w.put(u64::from(k), if wide { 5 } else { 4 });
        let n = if i == 0 { size - order } else { size };
        for &r in &res[at..at + n] {
            let u = zigzag(r);
            w.unary((u >> k) as u32);
            w.put(u & ((1u64 << k) - 1), k);
        }
        at += n;
    }
}

fn utf8_number(w: &mut Writer, n: u64) {
    if n < 0x80 {
        w.put(n, 8);
        return;
    }
    let mut extra = 1;
    while n >> (6 * extra) >= 1u64 << (6 - extra) {
        extra += 1;
    }
    let lead = !0u64 << (7 - extra) & 0xff;
    w.put(lead | n >> (6 * extra), 8);
    for i in (0..extra).rev() {
        w.put(0x80 | (n >> (6 * i) & 0x3f), 8);
    }
}
fn rate_code(rate: u32) -> u64 {
    match rate {
        88200 => 1,
        176400 => 2,
        192000 => 3,
        8000 => 4,
        16000 => 5,
        22050 => 6,
        24000 => 7,
        32000 => 8,
        44100 => 9,
        48000 => 10,
        96000 => 11,
        _ => 0,
    }
}
fn size_code(bits: u16) -> u64 {
    match bits {
        8 => 1,
        12 => 2,
        16 => 4,
        20 => 5,
        24 => 6,
        _ => 0,
    }
}

/// A FLAC stream of the sound.
pub fn encode(sound: &Sound) -> Result<Vec<u8>> {
    let channels = sound.channels as usize;
    if !(1..=2).contains(&channels) || !(4..=24).contains(&sound.bits) || sound.rate == 0 {
        return Err(invalid("Suono non adatto a FLAC"));
    }
    if !sound.samples.len().is_multiple_of(channels) {
        return Err(invalid("Campioni incompleti"));
    }
    let frames = sound.samples.len() / channels;
    let bits = u32::from(sound.bits);
    let mut out = Writer::new();
    out.bytes.extend_from_slice(b"fLaC");
    // STREAMINFO, the last metadata block.
    out.put(1, 1);
    out.put(0, 7);
    out.put(34, 24);
    let block = BLOCK.min(frames.max(1));
    out.put(block as u64, 16);
    out.put(block as u64, 16);
    out.put(0, 24);
    out.put(0, 24);
    out.put(u64::from(sound.rate), 20);
    out.put(channels as u64 - 1, 3);
    out.put(u64::from(bits) - 1, 5);
    out.put((frames as u64) >> 32, 4);
    out.put(frames as u64 & 0xffff_ffff, 32);
    for _ in 0..4 {
        out.put(0, 32);
    }
    let mut bytes = out.bytes;
    for (number, start) in (0..frames).step_by(BLOCK).enumerate() {
        let n = BLOCK.min(frames - start);
        let channel = |c: usize| -> Vec<i64> {
            (start..start + n)
                .map(|i| i64::from(sound.samples[i * channels + c]))
                .collect()
        };
        // The channels as written, with the assignment and their widths.
        let (assignment, signals, widths): (u64, Vec<Vec<i64>>, Vec<u32>) = if channels == 2 {
            let (l, r) = (channel(0), channel(1));
            let side: Vec<i64> = l.iter().zip(&r).map(|(a, b)| a - b).collect();
            let mid: Vec<i64> = l.iter().zip(&r).map(|(a, b)| (a + b) >> 1).collect();
            let cost = |s: &[i64], b: u32| {
                let mut w = Writer::new();
                subframe(&mut w, s, b);
                w.bytes.len()
            };
            let (cl, cr, cs, cm) = (
                cost(&l, bits),
                cost(&r, bits),
                cost(&side, bits + 1),
                cost(&mid, bits),
            );
            let options = [(cl + cr, 1u64), (cl + cs, 8), (cs + cr, 9), (cm + cs, 10)];
            let choice = options.iter().min_by_key(|o| o.0).unwrap().1;
            match choice {
                8 => (8, vec![l, side], vec![bits, bits + 1]),
                9 => (9, vec![side, r], vec![bits + 1, bits]),
                10 => (10, vec![mid, side], vec![bits, bits + 1]),
                _ => (1, vec![l, r], vec![bits, bits]),
            }
        } else {
            (0, vec![channel(0)], vec![bits])
        };
        let mut w = Writer::new();
        w.put(0b11111111111110, 14);
        w.put(0, 1);
        w.put(0, 1);
        w.put(0b0111, 4);
        w.put(rate_code(sound.rate), 4);
        w.put(assignment, 4);
        w.put(size_code(sound.bits), 3);
        w.put(0, 1);
        utf8_number(&mut w, number as u64);
        w.put(n as u64 - 1, 16);
        let crc = crc8(&w.bytes);
        w.put(u64::from(crc), 8);
        for (signal, &width) in signals.iter().zip(&widths) {
            subframe(&mut w, signal, width);
        }
        w.align();
        let crc = crc16(&w.bytes);
        w.put(u64::from(crc), 16);
        bytes.extend_from_slice(&w.bytes);
    }
    Ok(bytes)
}

fn read_subframe(r: &mut Reader, n: usize, bits: u32, out: &mut Vec<i64>) -> Result<()> {
    if r.get(1)? != 0 {
        return Err(invalid("FLAC: sottoblocco non valido"));
    }
    let kind = r.get(6)? as u32;
    let mut wasted = 0;
    if r.get(1)? == 1 {
        wasted = r.unary()? + 1;
    }
    let bits = bits - wasted;
    out.clear();
    match kind {
        0 => {
            let v = r.signed(bits)?;
            out.resize(n, v);
        }
        1 => {
            for _ in 0..n {
                out.push(r.signed(bits)?);
            }
        }
        8..=12 => {
            let order = (kind - 8) as usize;
            if order > n {
                return Err(invalid("FLAC: ordine oltre il blocco"));
            }
            for _ in 0..order {
                out.push(r.signed(bits)?);
            }
            let wide = match r.get(2)? {
                0 => false,
                1 => true,
                _ => return Err(invalid("FLAC: codifica del residuo sconosciuta")),
            };
            let p = r.get(4)? as u32;
            let parts = 1usize << p;
            if !n.is_multiple_of(parts) || n / parts < order {
                return Err(invalid("FLAC: partizioni non valide"));
            }
            for i in 0..parts {
                let k = r.get(if wide { 5 } else { 4 })? as u32;
                let count = if i == 0 { n / parts - order } else { n / parts };
                let escape = if wide { 31 } else { 15 };
                for _ in 0..count {
                    let res = if k == escape {
                        unreachable_escape(r)?
                    } else {
                        let u = (u64::from(r.unary()?) << k) | r.get(k)?;
                        ((u >> 1) as i64) ^ -((u & 1) as i64)
                    };
                    let i = out.len();
                    let s = |d: usize| out[i - d];
                    let predicted = match order {
                        0 => 0,
                        1 => s(1),
                        2 => 2 * s(1) - s(2),
                        3 => 3 * s(1) - 3 * s(2) + s(3),
                        _ => 4 * s(1) - 6 * s(2) + 4 * s(3) - s(4),
                    };
                    out.push(predicted + res);
                }
            }
        }
        _ => return Err(invalid("FLAC: sottoblocco non usato da ReTzar")),
    }
    if wasted > 0 {
        for v in out.iter_mut() {
            *v <<= wasted;
        }
    }
    Ok(())
}
/// The escaped partitions this encoder never writes.
fn unreachable_escape(_: &mut Reader) -> Result<i64> {
    Err(invalid("FLAC: partizione non compressa"))
}

/// The sound of a FLAC stream written by [`encode`].
pub fn decode(bytes: &[u8]) -> Result<Sound> {
    if bytes.get(..4) != Some(b"fLaC") {
        return Err(invalid("Non è FLAC"));
    }
    let mut r = Reader {
        bytes,
        at: 4,
        bit: 0,
    };
    let mut info = None;
    loop {
        let last = r.get(1)? == 1;
        let kind = r.get(7)?;
        let len = r.get(24)? as usize;
        let start = r.at;
        if kind == 0 {
            r.get(16)?;
            r.get(16)?;
            r.get(24)?;
            r.get(24)?;
            let rate = r.get(20)? as u32;
            let channels = r.get(3)? as u16 + 1;
            let bits = r.get(5)? as u16 + 1;
            let total = r.get(36)? as usize;
            info = Some((rate, channels, bits, total));
        }
        r.at = start + len;
        r.bit = 0;
        if last {
            break;
        }
    }
    let (rate, channels, bits, total) = info.ok_or_else(|| invalid("FLAC senza STREAMINFO"))?;
    let ch = channels as usize;
    let mut samples = Vec::with_capacity(total * ch);
    let mut signals = vec![Vec::new(); ch];
    while samples.len() < total * ch {
        let frame_start = r.at;
        if r.get(14)? != 0b11111111111110 {
            return Err(invalid("FLAC: sincronismo perso"));
        }
        r.get(2)?;
        let size = r.get(4)?;
        let rate_code = r.get(4)?;
        let assignment = r.get(4)?;
        r.get(3)?;
        r.get(1)?;
        // The frame number, UTF-8 coded.
        let lead = r.get(8)?;
        let extra = (lead as u8).leading_ones().saturating_sub(1);
        for _ in 0..extra {
            r.get(8)?;
        }
        let n = match size {
            6 => r.get(8)? as usize + 1,
            7 => r.get(16)? as usize + 1,
            1 => 192,
            2..=5 => 576 << (size - 2),
            8..=15 => 256 << (size - 8),
            _ => return Err(invalid("FLAC: misura del blocco riservata")),
        };
        match rate_code {
            12 => {
                r.get(8)?;
            }
            13 | 14 => {
                r.get(16)?;
            }
            _ => {}
        }
        let header_end = r.at;
        if r.get(8)? as u8 != crc8(&bytes[frame_start..header_end]) {
            return Err(invalid("FLAC: CRC dell'intestazione errato"));
        }
        let bits = u32::from(bits);
        let widths: Vec<u32> = match assignment {
            0..=7 => vec![bits; assignment as usize + 1],
            8 => vec![bits, bits + 1],
            9 => vec![bits + 1, bits],
            10 => vec![bits, bits + 1],
            _ => return Err(invalid("FLAC: canali riservati")),
        };
        if widths.len() != ch {
            return Err(invalid("FLAC: canali diversi da STREAMINFO"));
        }
        for (signal, &width) in signals.iter_mut().zip(&widths) {
            read_subframe(&mut r, n, width, signal)?;
        }
        r.align();
        let frame_end = r.at;
        if r.get(16)? as u16 != crc16(&bytes[frame_start..frame_end]) {
            return Err(invalid("FLAC: CRC del blocco errato"));
        }
        if ch == 2 {
            for (&x, &y) in signals[0].iter().zip(&signals[1]).take(n) {
                let (a, b) = match assignment {
                    8 => (x, x - y),
                    9 => (x + y, y),
                    10 => {
                        let mid = (x << 1) | (y & 1);
                        ((mid + y) >> 1, (mid - y) >> 1)
                    }
                    _ => (x, y),
                };
                samples.extend([a as i32, b as i32]);
            }
        } else {
            samples.extend(signals[0].iter().take(n).map(|&x| x as i32));
        }
    }
    samples.truncate(total * ch);
    Ok(Sound {
        rate,
        channels,
        bits,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(seed: &mut u64) -> i32 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*seed >> 33) as i32
    }

    #[test]
    fn sounds_come_back_sample_for_sample() {
        let mut seed = 7;
        for (channels, bits, len) in [
            (1, 16, 10000),
            (2, 16, 9001),
            (1, 8, 4096),
            (2, 8, 3),
            (1, 16, 1),
        ] {
            let max = (1i32 << (bits - 1)) - 1;
            let samples: Vec<i32> = (0..len * channels)
                .map(|i| {
                    let t = (i / channels) as f64;
                    let tone = (t * 0.05 + (i % channels) as f64).sin() * max as f64 * 0.6;
                    let hiss = noise(&mut seed) % (max / 50 + 1);
                    (tone as i32 + hiss).clamp(-max - 1, max)
                })
                .collect();
            let sound = Sound {
                rate: 22050,
                channels: channels as u16,
                bits,
                samples,
            };
            let flac = encode(&sound).unwrap();
            assert_eq!(decode(&flac).unwrap(), sound, "{channels} {bits} {len}");
        }
        // Silence, full-scale extremes and a constant.
        for samples in [vec![0; 5000], [32767, -32768].repeat(3000), vec![-5; 4097]] {
            let sound = Sound {
                rate: 44100,
                channels: 1,
                bits: 16,
                samples,
            };
            assert_eq!(decode(&encode(&sound).unwrap()).unwrap(), sound);
        }
    }

    #[test]
    fn a_damaged_stream_is_refused() {
        let sound = Sound {
            rate: 22050,
            channels: 1,
            bits: 16,
            samples: (0..5000)
                .map(|i| ((i as f64 * 0.01).sin() * 9000.) as i32)
                .collect(),
        };
        let mut flac = encode(&sound).unwrap();
        let at = flac.len() / 2;
        flac[at] ^= 0x10;
        assert!(decode(&flac).is_err());
    }

    /// Windows' own FLAC decoder reads the streams as they are written.
    #[cfg(windows)]
    #[test]
    fn windows_reads_the_streams() {
        let samples: Vec<i32> = (0..44100)
            .flat_map(|i| {
                let t = i as f64 / 22050.;
                [
                    ((t * 440. * std::f64::consts::TAU).sin() * 12000.) as i32,
                    ((t * 660. * std::f64::consts::TAU).sin() * 9000.) as i32,
                ]
            })
            .collect();
        let sound = Sound {
            rate: 22050,
            channels: 2,
            bits: 16,
            samples,
        };
        let flac = encode(&sound).unwrap();
        let pcm = crate::media::decode_sound(&flac, "audio/flac").unwrap();
        assert_eq!((pcm.rate, pcm.channels, pcm.bits), (22050, 2, 16));
        let read: Vec<i32> = pcm
            .bytes
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as i32)
            .collect();
        assert_eq!(read, sound.samples);
    }
}
