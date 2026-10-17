//! Read-only Haemimont HPFS. Inodes contain a size, indirection depth and
//! 126 pointers; indirect pages contain 128 pointers without a header.
//!
//! Containers opened from disk read only the blocks they need. Scenarios
//! nested in a campaign are views over the parent file, so opening a mission
//! never copies the 120 MB campaign archive into memory. A container of the
//! portable copy's pack ([`crate::vfs`]) lists the same entries and reads
//! each from its own record.
use crate::{Result, invalid, read_at, u32_at};
use serde::Serialize;
use std::{collections::HashSet, fs::File, path::Path, sync::Arc};

const BLOCK: usize = 512;

#[derive(Debug, Serialize)]
pub struct Header {
    pub block_size: u32,
    pub declared_blocks: u32,
    pub bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct Entry {
    pub name: String,
    pub inode: usize,
    pub size: usize,
}
pub struct Container {
    source: Arc<Source>,
    pub header: Header,
    pub entries: Vec<Entry>,
}

/// A logical run of bytes: (start in the file, start in the parent, length).
type Run = (usize, u64, usize);

enum Source {
    Memory(Vec<u8>),
    File {
        file: File,
        len: usize,
    },
    Nested {
        parent: Arc<Source>,
        runs: Vec<Run>,
        len: usize,
    },
    /// A container of a mounted pack: its entries are records.
    Pack {
        pack: Arc<crate::pack::Pack>,
        folder: crate::pack::Folder,
    },
}
impl Source {
    fn len(&self) -> usize {
        match self {
            Source::Memory(d) => d.len(),
            Source::File { len, .. } | Source::Nested { len, .. } => *len,
            Source::Pack { folder, .. } => folder.bytes,
        }
    }
    fn range(&self, offset: usize, size: usize) -> Result<Vec<u8>> {
        let end = offset
            .checked_add(size)
            .filter(|&end| end <= self.len())
            .ok_or_else(|| invalid("truncated HPFS data block"))?;
        match self {
            Source::Pack { .. } => Err(invalid("HPFS block of a pack")),
            Source::Memory(d) => Ok(d[offset..end].to_vec()),
            Source::File { file, .. } => read_at(file, offset as u64, size),
            Source::Nested { parent, runs, .. } => {
                let mut out = Vec::with_capacity(size);
                let first = runs.partition_point(|r| r.0 + r.2 <= offset);
                for &(start, at, len) in &runs[first..] {
                    if start >= end {
                        break;
                    }
                    let from = offset.max(start);
                    let to = end.min(start + len);
                    out.extend_from_slice(
                        &parent.range((at + (from - start) as u64) as usize, to - from)?,
                    );
                }
                Ok(out)
            }
        }
    }
}

pub fn probe(data: &[u8]) -> Result<Header> {
    probe_header(data, data.len())
}
fn probe_header(head: &[u8], len: usize) -> Result<Header> {
    if !head.starts_with(b"HPFS") {
        return Err(invalid("missing HPFS signature"));
    }
    let block_size = u32_at(head, 4)?;
    let declared_blocks = u32_at(head, 8)?;
    if block_size != BLOCK as u32
        || declared_blocks == 0
        || declared_blocks as usize != len.div_ceil(BLOCK)
    {
        return Err(invalid("invalid HPFS geometry"));
    }
    Ok(Header {
        block_size,
        declared_blocks,
        bytes: len,
    })
}
impl Container {
    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        Self::from_source(Arc::new(Source::Memory(data)))
    }
    pub fn open(path: &Path) -> Result<Self> {
        if let Some((pack, folder)) = crate::vfs::container(path) {
            return Ok(Self::of_pack(pack, folder));
        }
        let file = File::open(path)?;
        let len = file.metadata()?.len() as usize;
        Self::from_source(Arc::new(Source::File { file, len }))
            .map_err(|e| format!("{}: {e}", path.display()).into())
    }
    fn from_source(source: Arc<Source>) -> Result<Self> {
        let header = probe_header(&source.range(0, 12.min(source.len()))?, source.len())?;
        let mut container = Self {
            source,
            header,
            entries: Vec::new(),
        };
        container.directory(1, "", &mut HashSet::new(), 0)?;
        Ok(container)
    }
    fn of_pack(pack: Arc<crate::pack::Pack>, folder: crate::pack::Folder) -> Self {
        Self {
            header: Header {
                block_size: folder.block_size,
                declared_blocks: folder.declared_blocks,
                bytes: folder.bytes,
            },
            entries: folder
                .entries
                .iter()
                .map(|e| Entry {
                    name: e.name.clone(),
                    inode: e.inode,
                    size: e.size,
                })
                .collect(),
            source: Arc::new(Source::Pack { pack, folder }),
        }
    }
    /// Open a container stored as a file of this one, without copying it.
    pub fn open_nested(&self, name: &str) -> Result<Self> {
        let entry = self.entry(name)?;
        if let Source::Pack { pack, folder } = &*self.source {
            return match folder.entries.iter().find(|e| e.inode == entry.inode) {
                Some(crate::pack::FolderEntry {
                    item: crate::pack::Item::Nested(nested),
                    ..
                }) => Ok(Self::of_pack(pack.clone(), nested.clone())),
                _ => Err(invalid(format!("{name}: not a container"))),
            };
        }
        let (len, runs) = self.runs(entry.inode)?;
        Self::from_source(Arc::new(Source::Nested {
            parent: self.source.clone(),
            runs,
            len,
        }))
        .map_err(|e| format!("{name}: {e}").into())
    }
    fn entry(&self, name: &str) -> Result<&Entry> {
        self.entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| invalid(format!("HPFS entry not found: {name}")))
    }
    pub fn get(&self, name: &str) -> Result<Vec<u8>> {
        self.read(self.entry(name)?.inode)
    }
    pub fn read(&self, inode: usize) -> Result<Vec<u8>> {
        if let Source::Pack { pack, folder } = &*self.source {
            return match folder.entries.iter().find(|e| e.inode == inode) {
                Some(crate::pack::FolderEntry {
                    item: crate::pack::Item::Record(id),
                    ..
                }) => pack.record(*id),
                Some(_) => Err(invalid("HPFS container read as a file")),
                None => Err(invalid(format!("HPFS inode not found: {inode}"))),
            };
        }
        let (size, runs) = self.runs(inode)?;
        let mut output = Vec::with_capacity(size);
        for (_, at, len) in runs {
            output.extend_from_slice(&self.source.range(at as usize, len)?);
        }
        Ok(output)
    }
    fn check_block(&self, block: usize) -> Result<usize> {
        if block == 0 || block >= self.header.declared_blocks as usize {
            return Err(invalid(format!("HPFS block outside container: {block}")));
        }
        Ok(block * BLOCK)
    }
    fn block(&self, block: usize) -> Result<Vec<u8>> {
        let at = self.check_block(block)?;
        self.source.range(at, BLOCK.min(self.source.len() - at))
    }
    /// Resolve an inode to its logical size and contiguous byte runs.
    fn runs(&self, inode: usize) -> Result<(usize, Vec<Run>)> {
        let node = self.block(inode)?;
        let size = u32_at(&node, 0)? as usize;
        let depth = u32_at(&node, 4)?;
        if size > 512 * 1024 * 1024 || depth > 3 {
            return Err(invalid("invalid HPFS inode"));
        }
        let count = size.div_ceil(BLOCK);
        let mut blocks = Vec::with_capacity(count);
        for i in 0..126 {
            if blocks.len() == count {
                break;
            }
            self.collect(
                u32_at(&node, 8 + i * 4)? as usize,
                depth,
                count,
                &mut blocks,
            )?;
        }
        if blocks.len() != count {
            return Err(invalid("truncated HPFS allocation tree"));
        }
        let mut runs: Vec<Run> = Vec::new();
        for (i, block) in blocks.into_iter().enumerate() {
            let logical = i * BLOCK;
            let len = BLOCK.min(size - logical);
            let at = (block * BLOCK) as u64;
            match runs.last_mut() {
                Some((start, parent, l)) if *parent + *l as u64 == at && *start + *l == logical => {
                    *l += len
                }
                _ => runs.push((logical, at, len)),
            }
        }
        Ok((size, runs))
    }
    fn collect(&self, block: usize, depth: u32, count: usize, out: &mut Vec<usize>) -> Result<()> {
        self.check_block(block)?;
        if depth == 0 {
            out.push(block);
            return Ok(());
        }
        let page = self.block(block)?;
        for i in 0..128 {
            if out.len() == count {
                break;
            }
            self.collect(u32_at(&page, i * 4)? as usize, depth - 1, count, out)?;
        }
        Ok(())
    }
    fn directory(
        &mut self,
        inode: usize,
        prefix: &str,
        seen: &mut HashSet<usize>,
        depth: usize,
    ) -> Result<()> {
        if depth > 32 || !seen.insert(inode) {
            return Err(invalid("HPFS directory cycle"));
        }
        let content = self.read(inode)?;
        let mut at = 0;
        while at < content.len() {
            let child = u32_at(&content, at)? as usize;
            let kind = content
                .get(at + 4..at + 6)
                .ok_or_else(|| invalid("truncated HPFS entry"))?;
            let kind = u16::from_le_bytes(kind.try_into()?);
            let length = content
                .get(at + 6..at + 8)
                .ok_or_else(|| invalid("truncated HPFS name length"))?;
            let length = u16::from_le_bytes(length.try_into()?) as usize;
            at += 8;
            let name = content
                .get(at..at + length)
                .ok_or_else(|| invalid("truncated HPFS name"))?;
            let name: String = name.iter().map(|b| char::from(*b)).collect();
            at += length;
            if name.is_empty()
                || name == "."
                || name == ".."
                || name.contains(['/', '\\', ':', '\0'])
            {
                return Err(invalid("invalid HPFS entry name"));
            }
            let name = format!("{prefix}{name}");
            if kind == 1 {
                self.directory(child, &format!("{name}/"), seen, depth + 1)?;
            } else if kind == 0 {
                let offset = self.check_block(child)?;
                let size = u32_at(&self.source.range(offset, 4)?, 0)? as usize;
                self.entries.push(Entry {
                    name,
                    inode: child,
                    size,
                });
            } else {
                return Err(invalid(format!("unsupported HPFS entry type: {kind}")));
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_geometry_and_directory_cycle_are_rejected() {
        assert!(probe(b"HPFS").is_err());
        let mut d = vec![0; 512 * 4];
        d[..4].copy_from_slice(b"HPFS");
        for (at, n) in [(4, 512u32), (8, 4), (512, 9), (520, 2), (1024, 1)] {
            d[at..at + 4].copy_from_slice(&n.to_le_bytes());
        }
        d[1028..1030].copy_from_slice(&1u16.to_le_bytes());
        d[1030..1032].copy_from_slice(&1u16.to_le_bytes());
        d[1032] = b'A';
        assert!(Container::from_bytes(d).is_err());
    }

    /// Builds a container holding one file whose blocks are deliberately out of
    /// order, so reads must follow the allocation table rather than offsets.
    fn container_with(name: &str, file: &[u8]) -> Vec<u8> {
        let data_blocks = file.len().div_ceil(512);
        let total = 4 + data_blocks;
        let mut d = vec![0; total * 512];
        d[..4].copy_from_slice(b"HPFS");
        d[4..8].copy_from_slice(&512u32.to_le_bytes());
        d[8..12].copy_from_slice(&(total as u32).to_le_bytes());
        let put =
            |d: &mut Vec<u8>, at: usize, n: u32| d[at..at + 4].copy_from_slice(&n.to_le_bytes());
        // Root directory inode (block 1) -> listing in block 2.
        let listing_len = 8 + name.len();
        put(&mut d, 512, listing_len as u32);
        put(&mut d, 520, 2);
        put(&mut d, 1024, 3);
        d[1030..1032].copy_from_slice(&(name.len() as u16).to_le_bytes());
        d[1032..1032 + name.len()].copy_from_slice(name.as_bytes());
        // File inode (block 3): data blocks stored in reverse order.
        put(&mut d, 1536, file.len() as u32);
        for i in 0..data_blocks {
            let block = total - 1 - i;
            put(&mut d, 1536 + 8 + i * 4, block as u32);
            let chunk = &file[i * 512..(i * 512 + 512).min(file.len())];
            d[block * 512..block * 512 + chunk.len()].copy_from_slice(chunk);
        }
        d
    }

    #[test]
    fn nested_views_and_files_read_the_same_bytes() {
        let payload: Vec<u8> = (0..1700u32).map(|i| (i * 31 % 251) as u8).collect();
        let inner = container_with("map.tiles", &payload);
        let outer = container_with("inner.wmp", &inner);
        // Names are single path components; folders are nested directories.
        assert!(Container::from_bytes(container_with("MAPS/inner.wmp", &inner)).is_err());
        let path = std::env::temp_dir().join(format!("retzar-hpfs-{}.wcm", std::process::id()));
        std::fs::write(&path, &outer).unwrap();
        for c in [
            Container::from_bytes(outer.clone()).unwrap(),
            Container::open(&path).unwrap(),
        ] {
            assert_eq!(c.get("INNER.wmp").unwrap(), inner);
            let nested = c.open_nested("inner.wmp").unwrap();
            assert_eq!(nested.entries[0].size, payload.len());
            assert_eq!(nested.get("map.tiles").unwrap(), payload);
            assert_eq!(
                Container::from_bytes(c.get("inner.wmp").unwrap())
                    .unwrap()
                    .get("map.tiles")
                    .unwrap(),
                payload
            );
        }
        std::fs::remove_file(path).unwrap();
    }
}
