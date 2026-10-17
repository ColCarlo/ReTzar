//! Import scenario dimensions and encoded terrain cells. Tile encoding,
//! object records and scenario rules are preserved for subsequent decoding.
use crate::{Result, hpfs::Container, invalid};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MapGrid {
    pub name: String,
    pub side: usize,
    pub cells: Vec<u16>,
}
impl MapGrid {
    pub fn read(container: &Container, header_name: &str) -> Result<Self> {
        let header = container.get(header_name)?;
        if !header.starts_with(b"2mzt") {
            return Err(invalid("unsupported scenario header version"));
        }
        let side = u16::from_le_bytes(
            header
                .get(6..8)
                .ok_or_else(|| invalid("truncated map dimensions"))?
                .try_into()?,
        ) as usize;
        if !(32..=1024).contains(&side) {
            return Err(invalid("invalid map dimensions"));
        }
        let name = header_name
            .strip_suffix(".header")
            .or_else(|| header_name.strip_suffix(".HEADER"))
            .ok_or_else(|| invalid("invalid header name"))?;
        let tiles = container.get(&format!("{name}.tiles"))?;
        if tiles.len() != side * side * 2 {
            return Err(invalid("map dimensions do not match terrain cell count"));
        }
        let cells = tiles
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        Ok(Self {
            name: name.to_owned(),
            side,
            cells,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct MapSummary {
    pub container_path: String,
    pub name: String,
    pub side: usize,
    pub terrain_cells: usize,
}

pub fn scan(container: &Container, path: &str) -> Result<Vec<MapSummary>> {
    let mut result = Vec::new();
    for entry in &container.entries {
        if entry.name.to_ascii_lowercase().ends_with(".header") {
            let grid = MapGrid::read(container, &entry.name)?;
            result.push(MapSummary {
                container_path: path.to_owned(),
                name: grid.name,
                side: grid.side,
                terrain_cells: grid.cells.len(),
            });
        } else if entry.name.to_ascii_lowercase().ends_with(".wmp") {
            let nested = container.open_nested(&entry.name)?;
            for child in &nested.entries {
                nested.read(child.inode)?;
            }
            result.extend(scan(&nested, &format!("{path}/{}", entry.name))?);
        }
    }
    Ok(result)
}
