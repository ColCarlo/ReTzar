//! The portable copy's data, `ReTzar.dat`: every file of the original game
//! that ReTzar reads, one record per resource, enciphered and authenticated
//! so that the copy is neither read nor changed with ordinary tools.
//!
//! Records keep the bytes of the original exactly, in modern forms: a BMP
//! as PNG, a WAVE as FLAC ([`crate::flac`]), anything else compressed with
//! deflate; the header and the trailer of a BMP or a WAVE are kept as they
//! are beside the picture or the sound. The builder checks that each record
//! gives its bytes back before it keeps that form. The movies, turned into
//! MP4 ([`crate::media`]), are stored as they are.
//!
//! The file is a header (`MAGIC`, then where the index lies and its length),
//! the records, then the index: what the archives (`Packs/*.wdt`), the HPFS
//! containers (`Maps/*.wmp`, `Campaign/*.wcm`) and the loose files hold, in
//! their own order, as JSON compressed with deflate. Records and index are
//! each enciphered with a BLAKE3 keystream of their own and end with a
//! 16-byte BLAKE3 tag; a changed byte makes the record unreadable.
use crate::{Result, invalid, read_at};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// The pack's name beside the executable.
pub const FILE: &str = "ReTzar.dat";
const MAGIC: &[u8; 8] = b"RTZDAT01";
const HEADER: u64 = 24;
const TAG: usize = 16;

/// The seed of the keys. It lives in the executable: whoever takes it apart
/// can read the pack, but nobody opens or changes it by chance.
const SEED: [u8; 32] = [
    0x54, 0x7a, 0x61, 0x72, 0x2d, 0xa1, 0x3c, 0x9e, 0x52, 0x65, 0x54, 0x7a, 0x61, 0x72, 0x0f, 0x77,
    0xc4, 0x18, 0xe2, 0x6b, 0x91, 0x2d, 0x5a, 0x33, 0xb7, 0x40, 0x0c, 0xde, 0x86, 0x19, 0x4f, 0xa2,
];
fn keys() -> &'static ([u8; 32], [u8; 32]) {
    static KEYS: OnceLock<([u8; 32], [u8; 32])> = OnceLock::new();
    KEYS.get_or_init(|| {
        (
            blake3::derive_key("ReTzar 2026-10-17 dati, cifratura", &SEED),
            blake3::derive_key("ReTzar 2026-10-17 dati, autenticazione", &SEED),
        )
    })
}
fn tag(context: &[u8], cipher: &[u8]) -> [u8; TAG] {
    let mut h = blake3::Hasher::new_keyed(&keys().1);
    h.update(&(context.len() as u64).to_le_bytes());
    h.update(context);
    h.update(cipher);
    let mut out = [0; TAG];
    out.copy_from_slice(&h.finalize().as_bytes()[..TAG]);
    out
}
fn keystream(context: &[u8], data: &mut [u8]) {
    let mut h = blake3::Hasher::new_keyed(&keys().0);
    h.update(context);
    let mut xof = h.finalize_xof();
    let mut block = [0u8; 4096];
    for chunk in data.chunks_mut(block.len()) {
        xof.fill(&mut block[..chunk.len()]);
        for (b, k) in chunk.iter_mut().zip(&block) {
            *b ^= k;
        }
    }
}
/// `plain` enciphered for `context` (never the same for two different
/// contents), followed by its tag.
pub fn seal(context: &[u8], plain: &[u8]) -> Vec<u8> {
    let mut out = plain.to_vec();
    keystream(context, &mut out);
    let t = tag(context, &out);
    out.extend_from_slice(&t);
    out
}
/// What [`seal`] enciphered, if nothing changed.
pub fn unseal(context: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
    let split = sealed
        .len()
        .checked_sub(TAG)
        .ok_or_else(|| invalid("Dati cifrati troncati"))?;
    let (cipher, t) = sealed.split_at(split);
    if tag(context, cipher) != t {
        return Err(invalid("Dati cifrati danneggiati o modificati"));
    }
    let mut out = cipher.to_vec();
    keystream(context, &mut out);
    Ok(out)
}
/// The start of what [`seal`] enciphered, unchecked: only for a list that
/// shows it, never for what is then used.
pub fn peek(context: &[u8], cipher: &[u8]) -> Vec<u8> {
    let mut out = cipher.to_vec();
    keystream(context, &mut out);
    out
}
fn record_context(id: u32) -> Vec<u8> {
    [b"record ".as_slice(), &id.to_le_bytes()].concat()
}
const INDEX_CONTEXT: &[u8] = b"index";

/// How a record keeps its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Codec {
    Stored,
    Deflate,
    /// A BMP: its header and trailer, and the picture as PNG.
    Png,
    /// A WAVE: its header and trailer, and the sound as FLAC.
    Flac,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub offset: u64,
    pub stored: u64,
    pub size: u64,
    pub codec: Codec,
}
/// The entries of an archive: names and records, in order.
pub type Entries = Vec<(String, u32)>;
/// What a path of the data holds.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Node {
    /// A file read whole.
    File(u32),
    /// An archive: its entries in their order, repeated names kept.
    Archive(Entries),
    /// An HPFS container.
    Container(Folder),
}
/// An HPFS container: its header and its entries.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Folder {
    pub block_size: u32,
    pub declared_blocks: u32,
    pub bytes: usize,
    pub entries: Vec<FolderEntry>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FolderEntry {
    pub name: String,
    pub inode: usize,
    pub size: usize,
    pub item: Item,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Item {
    Record(u32),
    /// A container held in this one (a campaign's maps).
    Nested(Folder),
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Index {
    pub records: Vec<Record>,
    /// Every path, relative to the data folder, `/` between its names.
    pub files: Vec<(String, Node)>,
}

/// A path as the index looks it up.
pub fn key(path: &str) -> String {
    path.replace('\\', "/")
        .trim_matches('/')
        .to_ascii_uppercase()
}

pub struct Pack {
    path: PathBuf,
    file: File,
    index: Index,
    lookup: HashMap<String, usize>,
}
impl Pack {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        let head = read_at(&file, 0, HEADER.min(len) as usize)?;
        if head.len() < HEADER as usize || &head[..8] != MAGIC {
            return Err(invalid(format!(
                "{}: non è un pacchetto di ReTzar",
                path.display()
            )));
        }
        let at = u64::from_le_bytes(head[8..16].try_into()?);
        let stored = u64::from_le_bytes(head[16..24].try_into()?);
        if at.checked_add(stored).is_none_or(|end| end > len) || stored > 1 << 30 {
            return Err(invalid(format!("{}: pacchetto troncato", path.display())));
        }
        let sealed = read_at(&file, at, stored as usize)?;
        let packed =
            unseal(INDEX_CONTEXT, &sealed).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut json = Vec::new();
        flate2::read::DeflateDecoder::new(packed.as_slice()).read_to_end(&mut json)?;
        let index: Index = serde_json::from_slice(&json)?;
        for r in &index.records {
            if r.offset.checked_add(r.stored).is_none_or(|end| end > at) {
                return Err(invalid(format!(
                    "{}: record fuori dal pacchetto",
                    path.display()
                )));
            }
        }
        let lookup = index
            .files
            .iter()
            .enumerate()
            .map(|(i, (p, _))| (key(p), i))
            .collect();
        Ok(Self {
            path: path.to_path_buf(),
            file,
            index,
            lookup,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn files(&self) -> &[(String, Node)] {
        &self.index.files
    }
    /// What a path holds, looked up without case.
    pub fn node(&self, path: &str) -> Option<&Node> {
        self.lookup.get(&key(path)).map(|&i| &self.index.files[i].1)
    }
    /// The original size of a record.
    pub fn size(&self, id: u32) -> usize {
        self.index
            .records
            .get(id as usize)
            .map_or(0, |r| r.size as usize)
    }
    /// The bytes of a record, as the original file has them.
    pub fn record(&self, id: u32) -> Result<Vec<u8>> {
        let r = self
            .index
            .records
            .get(id as usize)
            .ok_or_else(|| invalid("Record inesistente"))?;
        let sealed = read_at(&self.file, r.offset, r.stored as usize)?;
        let payload = unseal(&record_context(id), &sealed)?;
        let bytes = decode(r.codec, &payload)?;
        if bytes.len() as u64 != r.size {
            return Err(invalid("Record di misura errata"));
        }
        Ok(bytes)
    }
}

fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(data).read_to_end(&mut out)?;
    Ok(out)
}
fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).expect("compressione in memoria");
    e.finish().expect("compressione in memoria")
}

/// Header, trailer and stream of a PNG or FLAC record.
fn split_parts(payload: &[u8]) -> Result<(&[u8], &[u8], &[u8])> {
    let short = || invalid("Record troncato");
    let n = u32::from_le_bytes(payload.get(..4).ok_or_else(short)?.try_into()?) as usize;
    let head = payload.get(4..4 + n).ok_or_else(short)?;
    let rest = &payload[4 + n..];
    let m = u32::from_le_bytes(rest.get(..4).ok_or_else(short)?.try_into()?) as usize;
    let tail = rest.get(4..4 + m).ok_or_else(short)?;
    Ok((head, tail, &rest[4 + m..]))
}
fn join_parts(head: &[u8], tail: &[u8], stream: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + head.len() + tail.len() + stream.len());
    out.extend_from_slice(&(head.len() as u32).to_le_bytes());
    out.extend_from_slice(head);
    out.extend_from_slice(&(tail.len() as u32).to_le_bytes());
    out.extend_from_slice(tail);
    out.extend_from_slice(stream);
    out
}

fn decode(codec: Codec, payload: &[u8]) -> Result<Vec<u8>> {
    match codec {
        Codec::Stored => Ok(payload.to_vec()),
        Codec::Deflate => inflate(payload),
        Codec::Png => {
            let (head, tail, png) = split_parts(payload)?;
            png_to_bmp(head, tail, png)
        }
        Codec::Flac => {
            let (head, tail, flac) = split_parts(payload)?;
            flac_to_wav(head, tail, flac)
        }
    }
}

/// The layout of an uncompressed BMP of 8, 16 or 24 bits.
struct Bmp {
    pixels: usize,
    width: usize,
    rows: usize,
    bottom_up: bool,
    bits: usize,
    stride: usize,
}
fn bmp_layout(head: &[u8]) -> Option<Bmp> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(head.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(head.get(at..at + 4)?.try_into().ok()?));
    if head.get(..2)? != b"BM" || u32_at(14)? < 40 || u32_at(30)? != 0 || u16_at(26)? != 1 {
        return None;
    }
    let width = u32_at(18)? as i32;
    let height = u32_at(22)? as i32;
    let bits = u16_at(28)? as usize;
    if width <= 0 || height == 0 || width > 16384 || height.unsigned_abs() > 16384 {
        return None;
    }
    if ![8, 16, 24].contains(&bits) {
        return None;
    }
    let width = width as usize;
    Some(Bmp {
        pixels: u32_at(10)? as usize,
        width,
        rows: height.unsigned_abs() as usize,
        bottom_up: height > 0,
        bits,
        stride: (width * bits).div_ceil(32) * 4,
    })
}
/// A BMP as header, trailer and PNG, if its picture fits a PNG exactly.
fn bmp_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let b = bmp_layout(bytes)?;
    let end = b.pixels.checked_add(b.stride.checked_mul(b.rows)?)?;
    if b.pixels < 54 || end > bytes.len() {
        return None;
    }
    let (head, tail) = (&bytes[..b.pixels], &bytes[end..]);
    let channels = if b.bits == 8 { 1 } else { 3 };
    let mut image = Vec::with_capacity(b.width * b.rows * channels);
    for y in 0..b.rows {
        let row = if b.bottom_up { b.rows - 1 - y } else { y };
        let line = &bytes[b.pixels + row * b.stride..][..b.stride];
        match b.bits {
            8 => image.extend_from_slice(&line[..b.width]),
            16 => {
                for x in 0..b.width {
                    let v = u16::from_le_bytes([line[x * 2], line[x * 2 + 1]]);
                    for shift in [10, 5, 0] {
                        let c = (v >> shift & 31) as u8;
                        image.push(c << 3 | c >> 2);
                    }
                }
            }
            _ => {
                for x in 0..b.width {
                    image.extend_from_slice(&[line[x * 3 + 2], line[x * 3 + 1], line[x * 3]]);
                }
            }
        }
    }
    let mut png = Vec::new();
    {
        let mut e = png::Encoder::new(&mut png, b.width as u32, b.rows as u32);
        e.set_depth(png::BitDepth::Eight);
        e.set_compression(png::Compression::High);
        if b.bits == 8 {
            e.set_color(png::ColorType::Indexed);
            // The colour table after the info header, as RGB; always 256.
            let table = 14 + u32::from_le_bytes(bytes[14..18].try_into().ok()?) as usize;
            let mut palette = vec![0u8; 256 * 3];
            for i in 0..256 {
                if let Some(q) = bytes.get(table + i * 4..table + i * 4 + 3)
                    && table + i * 4 + 4 <= b.pixels
                {
                    palette[i * 3..i * 3 + 3].copy_from_slice(&[q[2], q[1], q[0]]);
                }
            }
            e.set_palette(palette);
        } else {
            e.set_color(png::ColorType::Rgb);
        }
        let mut w = e.write_header().ok()?;
        w.write_image_data(&image).ok()?;
        w.finish().ok()?;
    }
    Some(join_parts(head, tail, &png))
}
fn png_to_bmp(head: &[u8], tail: &[u8], png: &[u8]) -> Result<Vec<u8>> {
    let b = bmp_layout(head).ok_or_else(|| invalid("Intestazione BMP non valida"))?;
    let mut d = png::Decoder::new(std::io::Cursor::new(png));
    d.set_transformations(png::Transformations::IDENTITY);
    let mut reader = d.read_info().map_err(|e| invalid(format!("PNG: {e}")))?;
    let mut image = vec![0; reader.output_buffer_size().ok_or("PNG troppo grande")?];
    let info = reader
        .next_frame(&mut image)
        .map_err(|e| invalid(format!("PNG: {e}")))?;
    let channels = if b.bits == 8 { 1 } else { 3 };
    if (info.width as usize, info.height as usize) != (b.width, b.rows)
        || info.line_size != b.width * channels
    {
        return Err(invalid("PNG di misura diversa dal BMP"));
    }
    let mut out = Vec::with_capacity(head.len() + b.stride * b.rows + tail.len());
    out.extend_from_slice(head);
    out.resize(head.len() + b.stride * b.rows, 0);
    for y in 0..b.rows {
        let row = if b.bottom_up { b.rows - 1 - y } else { y };
        let src = &image[y * info.line_size..][..info.line_size];
        let line = &mut out[b.pixels + row * b.stride..][..b.stride];
        match b.bits {
            8 => line[..b.width].copy_from_slice(src),
            16 => {
                for x in 0..b.width {
                    let c = |i: usize| u16::from(src[x * 3 + i] >> 3);
                    let v = c(0) << 10 | c(1) << 5 | c(2);
                    line[x * 2..x * 2 + 2].copy_from_slice(&v.to_le_bytes());
                }
            }
            _ => {
                for x in 0..b.width {
                    line[x * 3..x * 3 + 3].copy_from_slice(&[
                        src[x * 3 + 2],
                        src[x * 3 + 1],
                        src[x * 3],
                    ]);
                }
            }
        }
    }
    out.extend_from_slice(tail);
    Ok(out)
}

/// The PCM layout of a WAVE: where its samples lie, channels and bits.
fn wav_layout(bytes: &[u8]) -> Option<(usize, usize, u32, u16, u16)> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut at = 12;
    let mut format = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        let body = at + 8;
        if id == b"fmt " && len >= 16 {
            let f = bytes.get(body..body + 16)?;
            let tag = u16::from_le_bytes([f[0], f[1]]);
            let channels = u16::from_le_bytes([f[2], f[3]]);
            let rate = u32::from_le_bytes(f[4..8].try_into().ok()?);
            let bits = u16::from_le_bytes([f[14], f[15]]);
            format = Some((tag, channels, rate, bits));
        } else if id == b"data" {
            let (tag, channels, rate, bits) = format?;
            if tag != 1 || !(1..=2).contains(&channels) || ![8, 16].contains(&bits) || rate == 0 {
                return None;
            }
            let end = body.checked_add(len)?.min(bytes.len());
            return Some((body, end, rate, channels, bits));
        }
        at = body.checked_add(len)?.checked_add(len & 1)?;
    }
    None
}
fn wav_to_flac(bytes: &[u8]) -> Option<Vec<u8>> {
    let (start, end, rate, channels, bits) = wav_layout(bytes)?;
    let data = &bytes[start..end];
    let frame = channels as usize * bits as usize / 8;
    if !data.len().is_multiple_of(frame) {
        return None;
    }
    let samples: Vec<i32> = if bits == 8 {
        data.iter().map(|&b| i32::from(b) - 128).collect()
    } else {
        data.as_chunks::<2>()
            .0
            .iter()
            .map(|b| i32::from(i16::from_le_bytes(*b)))
            .collect()
    };
    let flac = crate::flac::encode(&crate::flac::Sound {
        rate,
        channels,
        bits,
        samples,
    })
    .ok()?;
    Some(join_parts(&bytes[..start], &bytes[end..], &flac))
}
fn flac_to_wav(head: &[u8], tail: &[u8], flac: &[u8]) -> Result<Vec<u8>> {
    let sound = crate::flac::decode(flac)?;
    let mut out = Vec::with_capacity(head.len() + sound.samples.len() * 2 + tail.len());
    out.extend_from_slice(head);
    if sound.bits == 8 {
        out.extend(sound.samples.iter().map(|&s| (s + 128) as u8));
    } else {
        for &s in &sound.samples {
            out.extend_from_slice(&(s as i16).to_le_bytes());
        }
    }
    out.extend_from_slice(tail);
    Ok(out)
}

/// The best form of a file's bytes that gives them back exactly.
pub fn encode(name: &str, bytes: &[u8]) -> (Codec, Vec<u8>) {
    let lower = name.to_ascii_lowercase();
    let modern = if lower.ends_with(".bmp") {
        bmp_to_png(bytes).map(|p| (Codec::Png, p))
    } else if lower.ends_with(".wav") {
        wav_to_flac(bytes).map(|p| (Codec::Flac, p))
    } else if lower.ends_with(".mp4") {
        return (Codec::Stored, bytes.to_vec());
    } else {
        None
    };
    if let Some((codec, payload)) = modern
        && decode(codec, &payload).is_ok_and(|back| back == bytes)
    {
        return (codec, payload);
    }
    let packed = deflate(bytes);
    if packed.len() < bytes.len() {
        (Codec::Deflate, packed)
    } else {
        (Codec::Stored, bytes.to_vec())
    }
}

/// A pack being written.
pub struct PackWriter {
    file: std::io::BufWriter<File>,
    offset: u64,
    index: Index,
    seen: HashMap<[u8; 32], u32>,
    pub saved: [u64; 4],
}
impl PackWriter {
    pub fn create(path: &Path) -> Result<Self> {
        let mut file = std::io::BufWriter::new(File::create(path)?);
        file.write_all(&[0; HEADER as usize])?;
        Ok(Self {
            file,
            offset: HEADER,
            index: Index::default(),
            seen: HashMap::new(),
            saved: [0; 4],
        })
    }
    /// A record of `bytes`, `name` telling their kind; the same bytes are
    /// kept once.
    pub fn add(&mut self, name: &str, bytes: &[u8]) -> Result<u32> {
        let hash = *blake3::hash(bytes).as_bytes();
        if let Some(&id) = self.seen.get(&hash) {
            return Ok(id);
        }
        let (codec, payload) = encode(name, bytes);
        self.push(hash, codec, payload, bytes.len())
    }
    /// The record already holding these bytes.
    pub fn known(&self, bytes: &[u8]) -> Option<u32> {
        self.seen.get(blake3::hash(bytes).as_bytes()).copied()
    }
    /// A record already encoded (see [`encode`]).
    pub fn add_encoded(&mut self, bytes: &[u8], codec: Codec, payload: Vec<u8>) -> Result<u32> {
        let hash = *blake3::hash(bytes).as_bytes();
        if let Some(&id) = self.seen.get(&hash) {
            return Ok(id);
        }
        self.push(hash, codec, payload, bytes.len())
    }
    fn push(&mut self, hash: [u8; 32], codec: Codec, payload: Vec<u8>, size: usize) -> Result<u32> {
        let id = self.index.records.len() as u32;
        let sealed = seal(&record_context(id), &payload);
        self.file.write_all(&sealed)?;
        self.index.records.push(Record {
            offset: self.offset,
            stored: sealed.len() as u64,
            size: size as u64,
            codec,
        });
        self.offset += sealed.len() as u64;
        self.seen.insert(hash, id);
        let slot = match codec {
            Codec::Stored => 0,
            Codec::Deflate => 1,
            Codec::Png => 2,
            Codec::Flac => 3,
        };
        self.saved[slot] += (size as u64).saturating_sub(payload.len() as u64);
        Ok(id)
    }
    pub fn file(&mut self, path: &str, node: Node) {
        self.index.files.push((path.replace('\\', "/"), node));
    }
    pub fn finish(mut self) -> Result<u64> {
        let json = serde_json::to_vec(&self.index)?;
        let sealed = seal(INDEX_CONTEXT, &deflate(&json));
        self.file.write_all(&sealed)?;
        let mut file = self
            .file
            .into_inner()
            .map_err(|e| invalid(format!("Scrittura del pacchetto: {e}")))?;
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0))?;
        let mut head = Vec::with_capacity(HEADER as usize);
        head.extend_from_slice(MAGIC);
        head.extend_from_slice(&self.offset.to_le_bytes());
        head.extend_from_slice(&(sealed.len() as u64).to_le_bytes());
        file.write_all(&head)?;
        file.sync_all()?;
        Ok(self.offset + sealed.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bmp(bits: u16, width: i32, height: i32, fill: impl Fn(usize, usize) -> Vec<u8>) -> Vec<u8> {
        let stride = (width as usize * bits as usize).div_ceil(32) * 4;
        let palette = if bits == 8 { 256 * 4 } else { 0 };
        let pixels = 54 + palette;
        let rows = height.unsigned_abs() as usize;
        let mut out = vec![0u8; pixels + stride * rows];
        out[..2].copy_from_slice(b"BM");
        let total = out.len() as u32;
        out[2..6].copy_from_slice(&total.to_le_bytes());
        out[10..14].copy_from_slice(&(pixels as u32).to_le_bytes());
        out[14..18].copy_from_slice(&40u32.to_le_bytes());
        out[18..22].copy_from_slice(&width.to_le_bytes());
        out[22..26].copy_from_slice(&height.to_le_bytes());
        out[26..28].copy_from_slice(&1u16.to_le_bytes());
        out[28..30].copy_from_slice(&bits.to_le_bytes());
        for i in 0..palette / 4 {
            out[54 + i * 4..54 + i * 4 + 3].copy_from_slice(&[
                i as u8,
                255 - i as u8,
                (i * 7) as u8,
            ]);
        }
        for y in 0..rows {
            for x in 0..width as usize {
                let v = fill(x, y);
                let at = pixels + y * stride + x * v.len();
                out[at..at + v.len()].copy_from_slice(&v);
            }
        }
        out
    }

    #[test]
    fn pictures_go_to_png_and_back_exactly() {
        let eight = bmp(8, 13, 7, |x, y| vec![(x * 17 + y) as u8]);
        let sixteen = bmp(16, 9, -5, |x, y| {
            ((x * 1000 + y * 77) as u16 & 0x7fff).to_le_bytes().to_vec()
        });
        let mut twenty_four = bmp(24, 5, 3, |x, y| vec![x as u8, y as u8, 200]);
        twenty_four.extend_from_slice(b"coda");
        for b in [&eight, &sixteen, &twenty_four] {
            let (codec, payload) = encode("x.bmp", b);
            assert_eq!(codec, Codec::Png);
            assert_eq!(&decode(codec, &payload).unwrap(), b);
        }
        // The unused top bit of a 16-bit pixel set: kept by deflate.
        let odd = bmp(16, 4, 4, |_, _| 0x8001u16.to_le_bytes().to_vec());
        let (codec, payload) = encode("x.bmp", &odd);
        assert_eq!(codec, Codec::Deflate);
        assert_eq!(decode(codec, &payload).unwrap(), odd);
    }

    #[test]
    fn sounds_go_to_flac_and_back_exactly() {
        let mut wav =
            b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x02\0\x22\x56\0\0\x88\x58\x01\0\x04\0\x10\0data"
                .to_vec();
        let samples: Vec<u8> = (0..4000i32)
            .flat_map(|i| (((i as f64 * 0.02).sin() * 20000.) as i16).to_le_bytes())
            .collect();
        wav.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        wav.extend_from_slice(&samples);
        wav.extend_from_slice(b"LIST\x04\0\0\0abcd");
        let (codec, payload) = encode("voce.WAV", &wav);
        assert_eq!(codec, Codec::Flac);
        assert!(payload.len() < wav.len() / 2);
        assert_eq!(decode(codec, &payload).unwrap(), wav);
        // 8 bits.
        let mut small = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x22\x56\0\0\x22\x56\0\0\x01\0\x08\0data\x05\0\0\0".to_vec();
        small.extend_from_slice(&[0, 128, 255, 127, 129]);
        let (codec, payload) = encode("x.wav", &small);
        assert_eq!(decode(codec, &payload).unwrap(), small);
    }

    #[test]
    fn a_pack_reads_back_and_refuses_changes() {
        let dir = std::env::temp_dir().join(format!("retzar-pack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        let mut w = PackWriter::create(&path).unwrap();
        let a = w.add("a.ini", b"[sezione]\nchiave = valore\n").unwrap();
        let b = w.add("b.bin", &[7; 1000]).unwrap();
        assert_eq!(w.add("c.bin", &[7; 1000]).unwrap(), b);
        w.file("Tzar.ini", Node::File(a));
        w.file(
            "Packs/x.wdt",
            Node::Archive(vec![("A.INI".into(), a), ("B".into(), b)]),
        );
        w.finish().unwrap();
        let pack = Pack::open(&path).unwrap();
        let Some(Node::File(id)) = pack.node("tzar.INI") else {
            panic!()
        };
        assert_eq!(pack.record(*id).unwrap(), b"[sezione]\nchiave = valore\n");
        assert!(matches!(pack.node("packs\\X.WDT"), Some(Node::Archive(e)) if e.len() == 2));
        assert_eq!(pack.record(b).unwrap(), vec![7; 1000]);
        drop(pack);
        // One byte changed in a record.
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[HEADER as usize + 3] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        let pack = Pack::open(&path).unwrap();
        assert!(pack.record(a).is_err());
        assert!(pack.record(b).is_ok());
        drop(pack);
        // And in the index.
        let last = bytes.len() - 5;
        bytes[last] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        assert!(Pack::open(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sealing_needs_the_same_context() {
        let sealed = seal(b"uno", b"testo segreto");
        assert_ne!(&sealed[..13], b"testo segreto");
        assert_eq!(unseal(b"uno", &sealed).unwrap(), b"testo segreto");
        assert!(unseal(b"due", &sealed).is_err());
    }
}
