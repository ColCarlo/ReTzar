//! Read-only HMMSYS PackFile and Tzar LZSS. No native code is executed.
//!
//! Archives opened from disk keep only the directory and the LZSS chunk table
//! in memory. Each resource is read and decompressed on demand, chunk by chunk,
//! so the 185 MB of original artwork never has to stay resident. An archive
//! of the portable copy's pack ([`crate::vfs`]) reads each resource from its
//! own record.
use crate::{Result, invalid, read_at, u32_at};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
};

const MAX_UNPACKED: usize = 512 * 1024 * 1024;
/// Directory bytes decoded before the first parse attempt; doubled on demand.
const DIRECTORY_PROBE: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub name: String,
    /// Where the resource starts in the archive; in a pack, its place among
    /// the archive's records.
    pub offset: usize,
    pub size: usize,
}

pub struct Archive {
    store: Store,
    pub entries: Vec<Entry>,
    index: HashMap<String, usize>,
}

enum Store {
    /// Fully unpacked bytes, used for tests and in-memory archives.
    Memory(Vec<u8>),
    /// Lazily read from the original file.
    File {
        file: File,
        len: usize,
        lzss: Option<ChunkTable>,
    },
    /// The records of a mounted pack, in the order of the entries.
    Pack {
        pack: std::sync::Arc<crate::pack::Pack>,
        records: Vec<u32>,
    },
    /// Loose files under an installation's folder, each entry a path
    /// relative to it ([`Archive::folder`]).
    Folder(PathBuf),
}

struct ChunkTable {
    chunk_size: usize,
    algorithm: u8,
    /// Start of every compressed chunk plus the end of the file.
    offsets: Vec<u64>,
}

/// Archive names use backslashes and mixed case; lookups ignore both.
pub fn key(name: &str) -> String {
    name.replace('\\', "/").to_ascii_uppercase()
}

impl Archive {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some((pack, list)) = crate::vfs::archive(path) {
            let entries: Vec<Entry> = list
                .iter()
                .enumerate()
                .map(|(i, (name, id))| Entry {
                    name: name.clone(),
                    offset: i,
                    size: pack.size(*id),
                })
                .collect();
            let mut index = HashMap::with_capacity(entries.len());
            for (i, e) in entries.iter().enumerate() {
                index.entry(key(&e.name)).or_insert(i);
            }
            return Ok(Self {
                store: Store::Pack {
                    pack,
                    records: list.iter().map(|(_, id)| *id).collect(),
                },
                entries,
                index,
            });
        }
        let file = File::open(path)?;
        let file_len = file.metadata()?.len() as usize;
        let head = read_at(&file, 0, 18.min(file_len))?;
        let store = if head.starts_with(b"LZSS") {
            if head.len() < 18 {
                return Err(invalid("truncated LZSS header"));
            }
            let size = u32_at(&head, 4)? as usize;
            let chunk_size = u32_at(&head, 8)? as usize;
            let base = u32_at(&head, 14)? as usize;
            validate_lzss(size, chunk_size, base, file_len)?;
            let table = read_at(&file, 14, base - 14)?;
            let mut offsets = Vec::with_capacity(table.len() / 4 + 1);
            for i in 0..table.len() / 4 {
                offsets.push(u32_at(&table, i * 4)? as u64);
            }
            offsets.push(file_len as u64);
            if offsets
                .windows(2)
                .any(|w| w[0] < base as u64 || w[0] > w[1])
            {
                return Err(invalid("invalid LZSS chunk offset"));
            }
            Store::File {
                file,
                len: size,
                lzss: Some(ChunkTable {
                    chunk_size,
                    algorithm: head[12],
                    offsets,
                }),
            }
        } else {
            Store::File {
                file,
                len: file_len,
                lzss: None,
            }
        };
        Self::from_store(store)
    }

    pub fn from_bytes(input: &[u8]) -> Result<Self> {
        Self::from_store(Store::Memory(unpack(input)?))
    }

    /// The loose files of an installation under the folders `tops`, with
    /// the names an archive would give them (`DATA\AI\AI.INI`). The engine
    /// reads these folders too, after its packs, and the digital releases
    /// keep their data this way, without a `Packs` folder. The folders match
    /// whatever their case; none of them gives an empty archive.
    pub fn folder(root: &Path, tops: &[&str]) -> Result<Self> {
        let mut pending = Vec::new();
        for dir in std::fs::read_dir(root)?.flatten() {
            let name = dir.file_name().to_string_lossy().into_owned();
            if dir.file_type().is_ok_and(|t| t.is_dir())
                && tops.iter().any(|t| t.eq_ignore_ascii_case(&name))
            {
                pending.push((dir.path(), name));
            }
        }
        let mut entries = Vec::new();
        while let Some((dir, prefix)) = pending.pop() {
            for file in std::fs::read_dir(&dir)?.flatten() {
                let name = format!("{prefix}\\{}", file.file_name().to_string_lossy());
                match file.file_type() {
                    Ok(t) if t.is_dir() => pending.push((file.path(), name)),
                    Ok(t) if t.is_file() => entries.push(Entry {
                        name,
                        offset: 0,
                        size: file.metadata()?.len() as usize,
                    }),
                    _ => {}
                }
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let mut index = HashMap::with_capacity(entries.len());
        for (i, e) in entries.iter().enumerate() {
            index.entry(key(&e.name)).or_insert(i);
        }
        Ok(Self {
            store: Store::Folder(root.to_path_buf()),
            entries,
            index,
        })
    }

    fn from_store(store: Store) -> Result<Self> {
        let len = store.len();
        let mut probe = DIRECTORY_PROBE.min(len);
        let entries = loop {
            let head = store.range(0, probe)?;
            match parse_directory(&head, len) {
                Ok(entries) => break entries,
                Err(Parse::Truncated) if probe < len => probe = (probe * 2).min(len),
                Err(Parse::Truncated) => return Err(invalid("truncated archive directory")),
                Err(Parse::Invalid(e)) => return Err(e),
            }
        };
        let mut index = HashMap::with_capacity(entries.len());
        for (i, e) in entries.iter().enumerate() {
            // The first occurrence wins, as with the previous linear search.
            index.entry(key(&e.name)).or_insert(i);
        }
        Ok(Self {
            store,
            entries,
            index,
        })
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.index.get(&key(name)).map(|&i| &self.entries[i])
    }

    pub fn get(&self, name: &str) -> Result<Vec<u8>> {
        let entry = self
            .entry(name)
            .ok_or_else(|| invalid(format!("Risorsa mancante: {name}")))?;
        self.read(entry)
    }

    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>> {
        match &self.store {
            Store::Pack { pack, records } => {
                let id = records
                    .get(entry.offset)
                    .ok_or_else(|| invalid("resource outside archive"))?;
                pack.record(*id)
            }
            Store::Folder(root) => Ok(std::fs::read(root.join(entry.name.replace('\\', "/")))?),
            store => store.range(entry.offset, entry.size),
        }
    }
}

impl Store {
    fn len(&self) -> usize {
        match self {
            Store::Memory(data) => data.len(),
            Store::File { len, .. } => *len,
            Store::Pack { .. } => 0,
            Store::Folder(_) => 0,
        }
    }

    fn range(&self, offset: usize, size: usize) -> Result<Vec<u8>> {
        let end = offset
            .checked_add(size)
            .filter(|&end| end <= self.len())
            .ok_or_else(|| invalid("resource outside archive"))?;
        match self {
            Store::Pack { .. } => Err(invalid("resource outside archive")),
            Store::Folder(_) => Err(invalid("resource outside archive")),
            Store::Memory(data) => Ok(data[offset..end].to_vec()),
            Store::File {
                file, lzss: None, ..
            } => read_at(file, offset as u64, size),
            Store::File {
                file,
                lzss: Some(table),
                len,
            } => {
                if size == 0 {
                    return Ok(Vec::new());
                }
                let first = offset / table.chunk_size;
                let last = (end - 1) / table.chunk_size;
                // One positional read covers every compressed chunk of the range.
                let base = table.offsets[first];
                let packed = read_at(file, base, (table.offsets[last + 1] - base) as usize)?;
                let mut output = Vec::with_capacity(size);
                for chunk in first..=last {
                    let start = (table.offsets[chunk] - base) as usize;
                    let stop = (table.offsets[chunk + 1] - base) as usize;
                    let logical = chunk * table.chunk_size;
                    let expected = table.chunk_size.min(len - logical);
                    let source = &packed[start..stop];
                    let decoded;
                    let bytes = if source.len() == table.chunk_size {
                        &source[..expected]
                    } else {
                        decoded = decode_chunk(source, table.algorithm, expected)?;
                        &decoded[..]
                    };
                    let from = offset.max(logical) - logical;
                    let to = end.min(logical + expected) - logical;
                    output.extend_from_slice(&bytes[from..to]);
                }
                Ok(output)
            }
        }
    }
}

enum Parse {
    Truncated,
    Invalid(Box<dyn std::error::Error + Send + Sync>),
}

fn parse_directory(data: &[u8], len: usize) -> std::result::Result<Vec<Entry>, Parse> {
    let truncated = |at: usize| {
        if at > data.len() && data.len() < len {
            Parse::Truncated
        } else {
            Parse::Invalid(invalid("truncated archive directory"))
        }
    };
    if data.len() < 0x28 {
        return Err(truncated(0x28));
    }
    if !data.starts_with(b"HMMSYS PackFile") {
        return Err(Parse::Invalid(invalid("missing HMMSYS PackFile signature")));
    }
    let count = u32_at(data, 0x20).map_err(Parse::Invalid)? as usize;
    if count > 100_000 {
        return Err(Parse::Invalid(invalid("implausible archive entry count")));
    }
    let mut cursor = 0x28;
    let mut previous = Vec::new();
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let (total, prefix) = match data.get(cursor..cursor + 2) {
            Some(b) => (b[0] as usize, b[1] as usize),
            None => return Err(truncated(cursor + 2)),
        };
        cursor += 2;
        if prefix > previous.len() || prefix > total {
            return Err(Parse::Invalid(invalid("invalid filename prefix")));
        }
        let suffix_end = cursor + total - prefix;
        let Some(suffix) = data.get(cursor..suffix_end) else {
            return Err(truncated(suffix_end + 8));
        };
        let mut name = previous[..prefix].to_vec();
        name.extend_from_slice(suffix);
        cursor = suffix_end;
        if cursor + 8 > data.len() {
            return Err(truncated(cursor + 8));
        }
        let offset = u32_at(data, cursor).map_err(Parse::Invalid)? as usize;
        let size = u32_at(data, cursor + 4).map_err(Parse::Invalid)? as usize;
        cursor += 8;
        if offset.checked_add(size).is_none_or(|end| end > len) {
            return Err(Parse::Invalid(invalid("entry outside archive")));
        }
        let text: String = name.iter().map(|b| char::from(*b)).collect();
        let text = text.replace('\\', "/");
        if text.starts_with('/')
            || text.contains(':')
            || text.split('/').any(|s| s == "..")
            || text.contains('\0')
        {
            return Err(Parse::Invalid(invalid("unsafe archive entry path")));
        }
        entries.push(Entry {
            name: text,
            offset,
            size,
        });
        previous = name;
    }
    if entries.iter().any(|e| e.size > 0 && e.offset < cursor) {
        return Err(Parse::Invalid(invalid("entry overlaps directory")));
    }
    Ok(entries)
}

fn validate_lzss(size: usize, chunk_size: usize, base: usize, input_len: usize) -> Result<()> {
    if size > MAX_UNPACKED
        || chunk_size == 0
        || chunk_size > MAX_UNPACKED
        || base < 18
        || base > input_len
        || (base - 14) % 4 != 0
    {
        return Err(invalid("invalid LZSS size or chunk table"));
    }
    if (base - 14) / 4 != size.div_ceil(chunk_size) {
        return Err(invalid("LZSS chunk count mismatch"));
    }
    Ok(())
}

/// Chunks have independent dictionaries. The last chunk's decoded capacity is
/// still chunk_size, but only the remaining logical bytes belong to the file.
pub fn unpack(input: &[u8]) -> Result<Vec<u8>> {
    if !input.starts_with(b"LZSS") {
        return Ok(input.to_vec());
    }
    let size = u32_at(input, 4)? as usize;
    let chunk_size = u32_at(input, 8)? as usize;
    let algorithm = *input
        .get(12)
        .ok_or_else(|| invalid("truncated LZSS header"))?;
    let base = u32_at(input, 14)? as usize;
    validate_lzss(size, chunk_size, base, input.len())?;
    let count = size.div_ceil(chunk_size);
    let mut output = Vec::with_capacity(size);
    for i in 0..count {
        let start = u32_at(input, 14 + i * 4)? as usize;
        let end = if i + 1 == count {
            input.len()
        } else {
            u32_at(input, 18 + i * 4)? as usize
        };
        if start < base || start > end || end > input.len() {
            return Err(invalid("invalid LZSS chunk offset"));
        }
        let expected = chunk_size.min(size - output.len());
        if end - start == chunk_size {
            output.extend_from_slice(&input[start..start + expected]);
        } else {
            output.extend_from_slice(&decode_chunk(&input[start..end], algorithm, expected)?);
        }
    }
    Ok(output)
}

struct Bits<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl Bits<'_> {
    fn read(&mut self, count: usize) -> Result<usize> {
        if self.position + count > self.bytes.len() * 8 {
            return Err(invalid("truncated LZSS token"));
        }
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1)
                | ((self.bytes[self.position / 8] >> (7 - self.position % 8)) & 1) as usize;
            self.position += 1;
        }
        Ok(value)
    }
}

pub fn decode_chunk(input: &[u8], algorithm: u8, expected: usize) -> Result<Vec<u8>> {
    let distance_bits = match algorithm >> 4 {
        0xa => 10,
        0xb => 11,
        0xc => 12,
        _ => return Err(invalid("unsupported LZSS algorithm")),
    };
    let length_bits = (algorithm & 0xf) as usize;
    if !(1..=5).contains(&length_bits) || expected > MAX_UNPACKED {
        return Err(invalid("unsupported LZSS token width"));
    }
    let mut bits = Bits {
        bytes: input,
        position: 0,
    };
    let mut output = Vec::with_capacity(expected);
    while output.len() < expected {
        if bits.read(1)? == 1 {
            output.push(bits.read(8)? as u8);
        } else {
            let encoded_distance = bits.read(distance_bits)?;
            let length = bits.read(length_bits)? + 2;
            let length = length.min(expected - output.len());
            if encoded_distance == 0 {
                output.resize(output.len() + length, 0);
            } else {
                let distance = (1 << distance_bits) - encoded_distance;
                if distance > output.len() {
                    return Err(invalid("LZSS reference before dictionary"));
                }
                for _ in 0..length {
                    output.push(output[output.len() - distance]);
                }
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encode(tokens: &[(usize, usize)]) -> Vec<u8> {
        let mut bits = Vec::new();
        for &(value, count) in tokens {
            for i in (0..count).rev() {
                bits.push((value >> i) & 1);
            }
        }
        let mut out = vec![0; bits.len().div_ceil(8)];
        for (i, bit) in bits.iter().enumerate() {
            out[i / 8] |= (*bit as u8) << (7 - i % 8);
        }
        out
    }
    #[test]
    fn literals_and_overlapping_reference() {
        let bytes = encode(&[
            (1, 1),
            (b'A' as usize, 8),
            (1, 1),
            (b'B' as usize, 8),
            (0, 1),
            (4094, 12),
            (4, 4),
        ]);
        assert_eq!(decode_chunk(&bytes, 0xc4, 8).unwrap(), b"ABABABAB");
    }
    #[test]
    fn zero_runs_and_invalid_tokens() {
        assert_eq!(
            decode_chunk(&encode(&[(0, 1), (0, 12), (2, 4)]), 0xc4, 4).unwrap(),
            [0; 4]
        );
        assert!(decode_chunk(&[], 0xc4, 1).is_err());
        assert!(decode_chunk(&[0x7f, 0xff, 0], 0xc4, 5).is_err());
        assert!(unpack(b"LZSS").is_err());
    }

    /// A pack with several resources split across literal-only LZSS chunks.
    fn compressed_pack(chunk_size: usize) -> (Vec<u8>, Vec<(String, Vec<u8>)>) {
        let resources: Vec<(String, Vec<u8>)> = (0..5)
            .map(|i| {
                let bytes = (0..37 + i * 23).map(|b| (b * 7 + i) as u8).collect();
                (format!("DIR\\File{i}.bin"), bytes)
            })
            .collect();
        let mut directory = vec![0; 0x28];
        directory[..15].copy_from_slice(b"HMMSYS PackFile");
        directory[0x20..0x24].copy_from_slice(&(resources.len() as u32).to_le_bytes());
        let names_len: usize = resources.iter().map(|(n, _)| 2 + n.len() + 8).sum();
        let mut offset = 0x28 + names_len;
        let mut payload = Vec::new();
        for (name, bytes) in &resources {
            directory.push(name.len() as u8);
            directory.push(0);
            directory.extend_from_slice(name.as_bytes());
            directory.extend_from_slice(&(offset as u32).to_le_bytes());
            directory.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            offset += bytes.len();
            payload.extend_from_slice(bytes);
        }
        let plain = [directory, payload].concat();
        let chunks: Vec<Vec<u8>> = plain
            .chunks(chunk_size)
            .map(|c| {
                encode(
                    &c.iter()
                        .flat_map(|&b| [(1, 1), (b as usize, 8)])
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let mut packed = b"LZSS".to_vec();
        packed.extend_from_slice(&(plain.len() as u32).to_le_bytes());
        packed.extend_from_slice(&(chunk_size as u32).to_le_bytes());
        packed.extend_from_slice(&[0xc4, 0]);
        let mut at = 14 + chunks.len() * 4;
        for c in &chunks {
            packed.extend_from_slice(&(at as u32).to_le_bytes());
            at += c.len();
        }
        for c in &chunks {
            packed.extend_from_slice(c);
        }
        (packed, resources)
    }

    #[test]
    fn lazy_file_reads_match_the_unpacked_archive() {
        for chunk_size in [16, 64, 4096] {
            let (packed, resources) = compressed_pack(chunk_size);
            let path = std::env::temp_dir().join(format!(
                "retzar-archive-{}-{chunk_size}.wdt",
                std::process::id()
            ));
            std::fs::write(&path, &packed).unwrap();
            let lazy = Archive::open(&path).unwrap();
            let memory = Archive::from_bytes(&packed).unwrap();
            assert_eq!(lazy.entries.len(), resources.len());
            for (name, bytes) in &resources {
                assert_eq!(&lazy.get(name).unwrap(), bytes);
                assert_eq!(&memory.get(name).unwrap(), bytes);
                // Lookups ignore case and slash direction.
                assert_eq!(
                    &lazy
                        .get(&name.to_ascii_lowercase().replace('\\', "/"))
                        .unwrap(),
                    bytes
                );
            }
            assert!(lazy.get("missing.bin").is_err());
            drop(lazy);
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn loose_folders_read_as_an_archive() {
        let root = std::env::temp_dir().join(format!("retzar-loose-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Data/Ai")).unwrap();
        std::fs::create_dir_all(root.join("SOUND")).unwrap();
        std::fs::create_dir_all(root.join("Maps")).unwrap();
        std::fs::write(root.join("Data/Ai/ai.ini"), b"[ai]").unwrap();
        std::fs::write(root.join("SOUND/Click.wav"), b"RIFF").unwrap();
        std::fs::write(root.join("Maps/MP1.wmp"), b"map").unwrap();
        let loose = Archive::folder(&root, &["DATA", "SOUND"]).unwrap();
        let names: Vec<&str> = loose.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Data\\Ai\\ai.ini", "SOUND\\Click.wav"]);
        assert_eq!(loose.get("DATA/AI/AI.INI").unwrap(), b"[ai]");
        assert_eq!(loose.get("sound\\click.wav").unwrap(), b"RIFF");
        assert!(loose.get("MAPS/MP1.WMP").is_err());
        assert!(
            Archive::folder(&root, &["IMAGES"])
                .unwrap()
                .entries
                .is_empty()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
