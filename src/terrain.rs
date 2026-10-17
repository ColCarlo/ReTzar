use crate::{
    Result,
    assets::{GameAssets, Ini, legacy_text},
    bitmap::Bitmap,
    invalid,
    map::MapGrid,
};
use std::collections::BTreeMap;

pub struct Sheet {
    pub name: String,
    pub image: Bitmap,
    pub columns: usize,
    pub rows: usize,
    /// Additional temporal pages; all share the base page's UV coordinates.
    pub frames: Vec<Bitmap>,
}
#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub sheet: usize,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub auxiliary: u16,
}
pub struct Terrain {
    pub biome: String,
    pub clouds: bool,
    pub rain: bool,
    pub snow: bool,
    /// `[colors] selection`: colour of the selection corners (gold on snow).
    pub selection: [u8; 3],
    pub sheets: Vec<Sheet>,
    pairs: BTreeMap<(u16, u16), usize>,
    /// A composed tile depends on the cell code and on its position modulo
    /// every uniform atlas and the coast variant pattern (x & 1, y & 2).
    period: [usize; 2],
    prepared: BTreeMap<(u16, usize, usize), Tile>,
    animated: BTreeMap<(u16, usize, usize), Vec<Tile>>,
    /// Minimap colours of `[minidatargb]`: the eight terrain types, trees
    /// and cliffs.
    mini: [Option<[u8; 3]>; 8],
    mini_tree: [u8; 3],
    mini_cliff: [u8; 3],
}
/// The biome named in a mission header (`grass`, `snow`, `desert`, …).
pub fn biome_of(header: &[u8]) -> Result<String> {
    Ok(legacy_text(
        header
            .get(0xd6..0x116)
            .ok_or_else(|| invalid("Missing map biome"))?
            .split(|b| *b == 0)
            .next()
            .unwrap_or_default(),
    )
    .trim()
    .to_ascii_lowercase())
}
fn lcm(a: usize, b: usize) -> usize {
    let (mut x, mut y) = (a, b);
    while y != 0 {
        (x, y) = (y, x % y);
    }
    a / x * b
}
/// The INI of a biome (`IMAGES/TERRAIN/<biome>.TERRAIN.INI`).
fn biome_ini(assets: &GameAssets, biome: &str) -> Result<Ini> {
    if biome.is_empty() || !biome.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(invalid("Invalid map biome"));
    }
    Ok(Ini::parse(
        &assets.get(&format!("IMAGES/TERRAIN/{biome}.TERRAIN.INI"))?,
    ))
}
/// The weather a biome allows (`[terrain] clouds`, `rain`, `snow`), which
/// the simulation needs without the terrain's pictures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Climate {
    pub clouds: bool,
    pub rain: bool,
    pub snow: bool,
}
impl Climate {
    pub fn load(assets: &GameAssets, biome: &str) -> Result<Self> {
        Ok(Self::from_ini(&biome_ini(assets, biome)?))
    }
    fn from_ini(ini: &Ini) -> Self {
        Self {
            clouds: ini.get("terrain", "clouds") == Some("1"),
            rain: ini.get("terrain", "rain") == Some("1"),
            snow: ini.get("terrain", "snow") == Some("1"),
        }
    }
}
impl Terrain {
    pub fn from_header(assets: &GameAssets, header: &[u8]) -> Result<Self> {
        Self::load(assets, &biome_of(header)?)
    }
    pub fn load(assets: &GameAssets, biome: &str) -> Result<Self> {
        let ini = biome_ini(assets, biome)?;
        let dir = ini.get("terrain", "dir").unwrap_or(biome);
        let mut sheets = Vec::new();
        let mut pairs = BTreeMap::new();
        for (key, file) in ini.entries("terrainfiles") {
            if key.len() != 2 || !key.bytes().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let size = ini
                .get("terrainsize", key)
                .or_else(|| ini.get("terrainsize", file.trim_end_matches(".bmp")));
            let Some(size) = size else {
                continue;
            };
            let dims: Vec<usize> = size
                .split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect();
            if dims.len() < 2 || dims[0] == 0 || dims[1] == 0 {
                return Err(invalid("Invalid terrain atlas grid"));
            }
            let name = format!("IMAGES/TERRAIN/{dir}/{file}");
            let at = sheets
                .iter()
                .position(|s: &Sheet| {
                    s.name.eq_ignore_ascii_case(&name) && s.columns == dims[0] && s.rows == dims[1]
                })
                .unwrap_or(sheets.len());
            if at == sheets.len() {
                let source = Bitmap::decode(&assets.get(&name)?, false)?;
                let width = dims[0] as u32 * 32;
                let height = dims[1] as u32 * 32;
                let mut frames = Vec::new();
                // Native terrain animation advances vertical square pages;
                // coast masks advance horizontal pages (terrainsize's third field).
                let coast_frames = dims.get(2).copied().unwrap_or(0);
                let count = if key == "00" {
                    source.height / height
                } else {
                    coast_frames.max(1) as u32
                };
                let page = |phase| -> Result<Bitmap> {
                    if coast_frames == 0 {
                        return source.crop(0, phase * height, width, height);
                    }
                    let mut image = Bitmap {
                        width,
                        height,
                        rgba: vec![0; (width * height * 4) as usize],
                    };
                    // Each variation contains all four wave frames, rather
                    // than the whole atlas repeating every two columns.
                    for col in 0..dims[0] as u32 {
                        let tile = source.crop((col * count + phase) * 32, 0, 32, height)?;
                        for row in 0..height {
                            let dst = ((row * width + col * 32) * 4) as usize;
                            let src = (row * 32 * 4) as usize;
                            image.rgba[dst..dst + 128].copy_from_slice(&tile.rgba[src..src + 128]);
                        }
                    }
                    Ok(image)
                };
                for phase in 1..count {
                    frames.push(page(phase)?);
                }
                let image = page(0)?;
                if image.width < dims[0] as u32 * 32 || image.height < dims[1] as u32 * 32 {
                    return Err(invalid(format!("Terrain atlas too small: {name}")));
                }
                sheets.push(Sheet {
                    name,
                    image,
                    columns: dims[0],
                    rows: dims[1],
                    frames,
                });
            }
            pairs.insert(
                (
                    (key.as_bytes()[0] - b'0') as u16,
                    (key.as_bytes()[1] - b'0') as u16,
                ),
                at,
            );
        }
        let mut period = [2, 4];
        for (&(a, b), &sheet) in &pairs {
            if a == b {
                let s: &Sheet = &sheets[sheet];
                period = [lcm(period[0], s.columns), lcm(period[1], s.rows)];
            }
        }
        if period[0] * period[1] > 4096 {
            return Err(invalid("Terrain atlas periods are too large"));
        }
        let rgb = |key: &str| match ini.numbers("minidatargb", key)[..] {
            [r, g, b, ..] => Some([r as u8, g as u8, b as u8]),
            _ => None,
        };
        let mini = std::array::from_fn(|i| rgb(&i.to_string()));
        let climate = Climate::from_ini(&ini);
        Ok(Self {
            biome: biome.into(),
            clouds: climate.clouds,
            rain: climate.rain,
            snow: climate.snow,
            selection: match ini.numbers("colors", "selection")[..] {
                [r, g, b] => [r as u8, g as u8, b as u8],
                _ => [255; 3],
            },
            sheets,
            pairs,
            period,
            prepared: BTreeMap::new(),
            animated: BTreeMap::new(),
            mini,
            mini_tree: rgb("tree").unwrap_or([11, 131, 11]),
            mini_cliff: rgb("cliff").unwrap_or([142, 107, 72]),
        })
    }
    fn key(&self, cell: u16, x: usize, y: usize) -> (u16, usize, usize) {
        (cell & 0x7ff, x % self.period[0], y % self.period[1])
    }
    pub fn tile(&self, cell: u16, x: usize, y: usize) -> Result<Tile> {
        if let Some(t) = self.prepared.get(&self.key(cell, x, y)) {
            return Ok(Tile {
                auxiliary: cell & 0xfc00,
                ..*t
            });
        }
        // Verified in Tzar 1.11 at 0x43fd43 and 0x476db0.
        let a = (cell >> 7) & 7;
        // Native renderer 0x4f1be6 paints terrain a beneath cliff overlays;
        // the other field is a cliff frame, not a second terrain type.
        let b = if cell & 0x400 != 0 {
            a
        } else {
            (cell >> 4) & 7
        };
        let shape = (cell & 15) as usize;
        let sheet = *self
            .pairs
            .get(&(a, b))
            .or_else(|| self.pairs.get(&(b, a)))
            .ok_or_else(|| invalid(format!("Missing terrain pair {a}{b} in {}", self.biome)))?;
        let s = &self.sheets[sheet];
        let (column, row) = if a == b {
            (x % s.columns, y % s.rows)
        } else {
            // Native renderer alternates coast variants at odd x / y bit 1.
            (
                usize::from(x & 1 != 0 && y & 2 != 0).min(s.columns - 1),
                shape,
            )
        };
        if row >= s.rows {
            return Err(invalid(format!("Invalid terrain transition {shape}")));
        }
        Ok(Tile {
            sheet,
            x: column as u32 * 32,
            y: row as u32 * 32,
            width: 32,
            height: 32,
            auxiliary: cell & 0xfc00,
        })
    }
    /// Replace the RGB555 marker colors with the two surface textures, as in
    /// native routine 0x459de0. Prepared tiles are cached in compact GPU atlases.
    pub fn prepare(&mut self, grid: &MapGrid) -> Result<()> {
        self.prepared.clear();
        self.animated.clear();
        self.sheets.retain(|s| !s.name.starts_with("composite/"));
        let mut unique = BTreeMap::new();
        for (i, &cell) in grid.cells.iter().enumerate() {
            if cell & 0x400 == 0 && (cell >> 7) & 7 != (cell >> 4) & 7 {
                unique.insert(self.key(cell, i % grid.side, i / grid.side), ());
            }
        }
        let mut pages: Vec<Bitmap> = Vec::new();
        let first_sheet = self.sheets.len();
        // Static transitions need a single page; water transitions include the
        // complete shared cycle of sea and coast, without per-frame CPU uploads.
        let mut slot = 1024;
        let mut page = 0;
        for &(cell, x, y) in unique.keys() {
            let mask = self.tile(cell, x, y)?;
            let a = (cell >> 7) & 7;
            let b = (cell >> 4) & 7;
            let ta = self.tile((a << 7) | (a << 4), x, y)?;
            let tb = self.tile((b << 7) | (b << 4), x, y)?;
            let lengths = [mask, ta, tb].map(|t| self.sheets[t.sheet].frames.len() + 1);
            let gcd = |mut a: usize, mut b: usize| {
                while b != 0 {
                    (a, b) = (b, a % b);
                }
                a
            };
            let count = lengths.into_iter().fold(1, |a, b| a / gcd(a, b) * b);
            if slot + count > 1024 {
                page = pages.len();
                slot = 0;
                pages.push(Bitmap {
                    width: 1024,
                    height: 1024,
                    rgba: vec![0; 1024 * 1024 * 4],
                });
            }
            let mut tiles = Vec::new();
            for phase in 0..count {
                let px = ((slot + phase) % 32) as u32 * 32;
                let py = ((slot + phase) / 32) as u32 * 32;
                let out = &mut pages[page];
                for gy in 0..32 {
                    for gx in 0..32 {
                        let pixel = |t: Tile| {
                            let sheet = &self.sheets[t.sheet];
                            let p = phase % (sheet.frames.len() + 1);
                            let s = if p == 0 {
                                &sheet.image
                            } else {
                                &sheet.frames[p - 1]
                            };
                            let at = (((t.y + gy) * s.width + t.x + gx) * 4) as usize;
                            &s.rgba[at..at + 4]
                        };
                        let p = pixel(mask);
                        let p = if p[..3] == [255, 0, 255] {
                            pixel(ta)
                        } else if p[..3] == [0, 255, 0] {
                            pixel(tb)
                        } else {
                            p
                        };
                        let dst = (((py + gy) * 1024 + px + gx) * 4) as usize;
                        out.rgba[dst..dst + 4].copy_from_slice(p);
                    }
                }
                tiles.push(Tile {
                    sheet: first_sheet + page,
                    x: px,
                    y: py,
                    width: 32,
                    height: 32,
                    auxiliary: 0,
                });
            }
            self.prepared.insert((cell, x, y), tiles[0]);
            if count > 1 {
                self.animated.insert((cell, x, y), tiles);
            }
            slot += count;
        }
        for (i, image) in pages.into_iter().enumerate() {
            self.sheets.push(Sheet {
                name: format!("composite/{i}"),
                image,
                columns: 32,
                rows: 32,
                frames: Vec::new(),
            });
        }
        Ok(())
    }
    pub fn animated_tiles(&self, cell: u16, x: usize, y: usize) -> Option<&[Tile]> {
        self.animated.get(&self.key(cell, x, y)).map(Vec::as_slice)
    }
    pub fn validate(&self, grid: &MapGrid) -> Result<usize> {
        let mut count = 0;
        for (i, &cell) in grid.cells.iter().enumerate() {
            let t = self.tile(cell, i % grid.side, i / grid.side)?;
            let s = &self.sheets[t.sheet];
            if t.x + 32 > s.image.width || t.y + 32 > s.image.height {
                return Err(invalid("Terrain tile outside atlas"));
            }
            count += 1;
        }
        Ok(count)
    }
    /// One pixel per cell, coloured like the original minimap (0x476b60):
    /// trees and cliffs in their own colours, other cells the average of the
    /// `[minidatargb]` colours of their two terrain types.
    pub fn minimap(&self, grid: &MapGrid) -> Result<Bitmap> {
        let mut rgba = Vec::with_capacity(grid.cells.len() * 4);
        for (i, &cell) in grid.cells.iter().enumerate() {
            // Types without a listed colour use the middle of their tile.
            let colour = |kind: u16| -> Result<[u8; 3]> {
                if let Some(c) = self.mini[kind as usize] {
                    return Ok(c);
                }
                let t = self.tile(cell, i % grid.side, i / grid.side)?;
                let s = &self.sheets[t.sheet];
                let at = (((t.y + 16) * s.image.width + t.x + 16) * 4) as usize;
                Ok([s.image.rgba[at], s.image.rgba[at + 1], s.image.rgba[at + 2]])
            };
            let c = if (cell >> 11) & 0x1f != 0 {
                self.mini_tree
            } else if cell & 0x400 != 0 {
                self.mini_cliff
            } else {
                let a = colour((cell >> 7) & 7)?;
                let b = colour((cell >> 4) & 7)?;
                [0, 1, 2].map(|k| ((a[k] as u16 + b[k] as u16) / 2) as u8)
            };
            rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        Ok(Bitmap {
            width: grid.side as u32,
            height: grid.side as u32,
            rgba,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transition_markers_choose_the_correct_surfaces_and_keep_details() {
        let solid = |color: [u8; 4]| Bitmap {
            width: 32,
            height: 32,
            rgba: color.repeat(32 * 32),
        };
        let mut mask = solid([255, 0, 255, 255]);
        for y in 0..32 {
            for x in 16..32 {
                mask.rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4]
                    .copy_from_slice(&[0, 255, 0, 255]);
            }
        }
        mask.rgba[..4].copy_from_slice(&[99, 57, 0, 255]);
        let mut t = Terrain {
            biome: "test".into(),
            clouds: false,
            rain: false,
            snow: false,
            selection: [255; 3],
            sheets: vec![
                Sheet {
                    name: "a".into(),
                    image: solid([100, 10, 20, 255]),
                    columns: 1,
                    rows: 1,
                    frames: Vec::new(),
                },
                Sheet {
                    name: "b".into(),
                    image: solid([20, 10, 100, 255]),
                    columns: 1,
                    rows: 1,
                    frames: Vec::new(),
                },
                Sheet {
                    name: "mask".into(),
                    image: mask,
                    columns: 1,
                    rows: 1,
                    frames: Vec::new(),
                },
            ],
            pairs: BTreeMap::from([((1, 1), 0), ((2, 2), 1), ((1, 2), 2)]),
            period: [2, 4],
            prepared: BTreeMap::new(),
            animated: BTreeMap::new(),
            mini: [None; 8],
            mini_tree: [11, 131, 11],
            mini_cliff: [142, 107, 72],
        };
        let grid = MapGrid {
            name: "test".into(),
            side: 1,
            cells: vec![(1 << 7) | (2 << 4) | 0x800],
        };
        t.prepare(&grid).unwrap();
        let tile = t.tile(grid.cells[0], 0, 0).unwrap();
        let p = &t.sheets[tile.sheet].image;
        assert_eq!(&p.rgba[..4], &[99, 57, 0, 255]);
        assert_eq!(&p.rgba[4..8], &[100, 10, 20, 255]);
        assert_eq!(&p.rgba[16 * 4..17 * 4], &[20, 10, 100, 255]);
        assert_eq!(tile.auxiliary, 0x800);
        let len = t.sheets.len();
        t.prepare(&grid).unwrap();
        assert_eq!(len, t.sheets.len());
        let batches = crate::scene::batches(&grid, &t).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].positions.len(), 4);
        assert_eq!(batches[0].indices, [0, 1, 2, 0, 2, 3]);
    }
}
