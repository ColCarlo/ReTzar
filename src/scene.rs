//! CPU construction of GPU batches: one mesh per texture and 32×32-cell chunk.
use crate::{Result, map::MapGrid, terrain::Terrain};
use std::collections::BTreeMap;

pub struct Batch {
    pub origin: [f32; 2],
    pub sheet: usize,
    pub positions: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub animations: Vec<WaterTile>,
}
pub struct WaterTile {
    pub vertex: usize,
    pub cell: [usize; 2],
    pub uv: Vec<[[f32; 2]; 4]>,
}
impl WaterTile {
    pub fn frame(&self, tick: u64) -> usize {
        if self.uv.len() != 20 {
            return tick as usize % self.uv.len();
        }
        // Native 0x4f1aa0: localized coast waves, at half the water tick.
        // The legacy random table was generated at startup. A stable hash
        // gives the same spatial staggering without coupling it to gameplay RNG.
        let [x, y] = self.cell;
        let seed = ((13 * x + 7 * y) % 127) as u64;
        // The native table holds R(0, 19) values (0x4efcd2).
        let noise = (seed.wrapping_mul(1103515245).wrapping_add(12345) >> 16) % 20;
        let wave = (((tick & 65535) >> 1) + noise + 5 * y as u64 + 11 * x as u64) & 127;
        let wave = if wave < 4 { wave as usize } else { 0 };
        let sea = tick as usize % 5;
        sea + 5 * ((wave + 4 - sea % 4) % 4)
    }
}
pub fn batches(grid: &MapGrid, terrain: &Terrain) -> Result<Vec<Batch>> {
    terrain.validate(grid)?;
    let mut result = Vec::new();
    for cy in (0..grid.side).step_by(32) {
        for cx in (0..grid.side).step_by(32) {
            let mut parts = BTreeMap::<usize, Batch>::new();
            for y in cy..(cy + 32).min(grid.side) {
                for x in cx..(cx + 32).min(grid.side) {
                    let tile = terrain.tile(grid.cells[y * grid.side + x], x, y)?;
                    let image = &terrain.sheets[tile.sheet].image;
                    let b = parts.entry(tile.sheet).or_insert_with(|| Batch {
                        origin: [cx as f32 * 32., -(cy as f32) * 32.],
                        sheet: tile.sheet,
                        positions: Vec::new(),
                        uv: Vec::new(),
                        indices: Vec::new(),
                        animations: Vec::new(),
                    });
                    let n = b.positions.len() as u32;
                    let px = (x - cx) as f32 * 32.;
                    let py = -((y - cy) as f32) * 32.;
                    b.positions.extend([
                        [px, py, 0.],
                        [px, py - 32., 0.],
                        [px + 32., py - 32., 0.],
                        [px + 32., py, 0.],
                    ]);
                    let u = tile.x as f32 / image.width as f32;
                    let v = tile.y as f32 / image.height as f32;
                    let u1 = (tile.x + 32) as f32 / image.width as f32;
                    let v1 = (tile.y + 32) as f32 / image.height as f32;
                    b.uv.extend([[u, v], [u, v1], [u1, v1], [u1, v]]);
                    if let Some(frames) =
                        terrain.animated_tiles(grid.cells[y * grid.side + x], x, y)
                    {
                        b.animations.push(WaterTile {
                            vertex: n as usize,
                            cell: [x, y],
                            uv: frames
                                .iter()
                                .map(|t| {
                                    let u = t.x as f32 / image.width as f32;
                                    let v = t.y as f32 / image.height as f32;
                                    let u1 = (t.x + 32) as f32 / image.width as f32;
                                    let v1 = (t.y + 32) as f32 / image.height as f32;
                                    [[u, v], [u, v1], [u1, v1], [u1, v]]
                                })
                                .collect(),
                        });
                    }
                    b.indices.extend([n, n + 1, n + 2, n, n + 2, n + 3]);
                }
            }
            result.extend(parts.into_values());
        }
    }
    Ok(result)
}
