//! Preparation of custom new games, without enabling gameplay.
use super::*;
use std::collections::BTreeSet;

impl MissionWorld {
    /// Native 0x43f2e0: explicit cell, castle nearest the building centroid,
    /// otherwise the mean cell of the kingdom's buildings and units.
    fn starting_cell(&self, owner: u8) -> Option<[u16; 2]> {
        if let Some(cell) = self.starting_positions[owner as usize - 1] {
            return Some(cell);
        }
        let buildings: Vec<_> = self
            .objects
            .iter()
            .filter(|o| o.owner == owner && o.building() && o.selectable())
            .collect();
        let mean = |objects: &[&MapObject]| -> Option<[u16; 2]> {
            (!objects.is_empty()).then(|| {
                [
                    (objects.iter().map(|o| o.x as u64).sum::<u64>() / objects.len() as u64) as u16,
                    (objects.iter().map(|o| o.y as u64).sum::<u64>() / objects.len() as u64) as u16,
                ]
            })
        };
        let center = mean(&buildings);
        if let Some(castle) = buildings
            .iter()
            .filter(|o| o.class == class::CASTLE)
            .min_by_key(|o| {
                let [x, y] = center.unwrap();
                (o.x as i64 - x as i64).pow(2) + (o.y as i64 - y as i64).pow(2)
            })
        {
            return Some([castle.x, castle.y]);
        }
        let mut forces = buildings;
        forces.extend(
            self.objects
                .iter()
                .filter(|o| o.owner == owner && crate::selection::unit(o) && o.selectable()),
        );
        mean(&forces)
    }

    /// Native custom loader 0x4411ab–0x4413bc removes the two kingdom lists,
    /// preserves nature/neutral objects, and creates a castle plus 3 peons.
    /// Only called during scene preparation; nothing is produced at runtime.
    pub fn reset_initial_forces(&mut self, grid: &MapGrid, lib: &ObjectLibrary) -> Result<()> {
        // Native loading advances the ID counter before deleting old forces.
        // Reusing their IDs could attach old inventory/holder references to
        // an unrelated new worker or castle.
        let first_id = self
            .objects
            .iter()
            .map(|o| o.id)
            .filter(|&id| id < 0xfc17)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let starts: Vec<_> = self
            .active_kingdoms()
            .map(|(owner, k)| {
                let cell = self.starting_cell(owner);
                (owner, k.civilization, cell)
            })
            .collect();
        self.objects.retain(|o| {
            !(1..=8).contains(&o.owner) || !(o.building() || crate::selection::unit(o))
        });
        let mut occupied = vec![false; grid.side * grid.side];
        for o in self.objects.iter().filter(|o| o.selectable()) {
            occupy(&mut occupied, grid.side, [o.x, o.y], lib.cell_footprint(o));
        }
        let mut ids: BTreeSet<_> = self.objects.iter().map(|o| o.id).collect();
        for (owner, civ, start) in starts {
            let start = start.ok_or_else(|| {
                invalid(crate::lang::fill(
                    "Regno {}: nessuna posizione iniziale nella mappa",
                    &[&owner],
                ))
            })?;
            // The original reserves 8×8 land cells even for the 7×7 Euro castle.
            let castle_cell =
                free_near(grid, &occupied, [start[0] as i32, start[1] as i32], [8, 8]).ok_or_else(
                    || {
                        invalid(crate::lang::fill(
                            "Regno {}: spazio insufficiente per il castello",
                            &[&owner],
                        ))
                    },
                )?;
            let castle = new_object(
                &mut ids,
                first_id,
                class::CASTLE,
                owner,
                civ,
                castle_cell,
                lib,
            )?;
            let footprint = lib.cell_footprint(&castle);
            occupy(&mut occupied, grid.side, castle_cell, footprint);
            self.starting_positions[owner as usize - 1] = Some(castle_cell);
            self.objects.push(castle);
            for _ in 0..3 {
                let cell = free_near(
                    grid,
                    &occupied,
                    [castle_cell[0] as i32, castle_cell[1] as i32],
                    [1, 1],
                )
                .ok_or_else(|| {
                    invalid(crate::lang::fill(
                        "Regno {}: spazio insufficiente per i contadini",
                        &[&owner],
                    ))
                })?;
                let worker = new_object(&mut ids, first_id, class::PEON, owner, civ, cell, lib)?;
                occupy(&mut occupied, grid.side, cell, [1, 1]);
                self.objects.push(worker);
            }
        }
        self.records = self.objects.len();
        self.hidden = self
            .objects
            .iter()
            .filter(|o| o.flags & FLAG_INACTIVE != 0)
            .count();
        Ok(())
    }
}

impl ObjectLibrary {
    /// Native object footprint in map cells, including rotated gates; the
    /// same picture's footprint is looked up once.
    pub fn cell_footprint(&self, o: &MapObject) -> [usize; 2] {
        let Some(key) = super::FootprintKey::of(o) else {
            return self.footprint_now(o);
        };
        if let Some(&f) = self.footprints.lock().unwrap().get(&key) {
            return f;
        }
        let f = self.footprint_now(o);
        self.footprints.lock().unwrap().insert(key, f);
        f
    }
    fn footprint_now(&self, o: &MapObject) -> [usize; 2] {
        let section = if matches!(o.class, class::GATE_STONE | class::GATE_WOOD) {
            format!(
                "g{}{}_anim",
                if o.class == class::GATE_STONE {
                    's'
                } else {
                    'w'
                },
                o.byte(56) & 3
            )
        } else {
            self.final_image(o).unwrap_or_default()
        };
        self.picture_cells(&section)
    }
    /// The cells of a picture as the image loader keeps them (0x498373,
    /// 0x498d34): `object` divided by 32 and truncated; a side left at zero
    /// is measured on a frame of a building's sheet, else one cell.
    pub fn picture_cells(&self, section: &str) -> [usize; 2] {
        let rect = self.images.numbers(section, "object");
        // Only the pictures with a side under 32 pixels look at their frame.
        let frame = || {
            self.frames
                .get(section)
                .or_else(|| self.frames.get(&section.to_ascii_lowercase()))
        };
        [2, 3].map(
            |k| match rect.get(k).copied().unwrap_or(0).max(0) as usize / 32 {
                0 => frame().map_or(1, |f| f[k - 2]),
                n => n,
            },
        )
    }
}

pub(crate) fn new_object(
    ids: &mut BTreeSet<u16>,
    first_id: u16,
    class: usize,
    owner: u8,
    civ: u8,
    cell: [u16; 2],
    lib: &ObjectLibrary,
) -> Result<MapObject> {
    let id = (first_id..0xfc17)
        .chain(1..first_id.min(0xfc17))
        .find(|id| !ids.contains(id))
        .ok_or_else(|| invalid("Identificatori degli oggetti esauriti"))?;
    ids.insert(id);
    let mut o = MapObject {
        id,
        class,
        owner,
        civilization: civ,
        x: cell[0],
        y: cell[1],
        flags: 0,
        variant: 0,
        health: 0,
        max_health: 0,
        name: String::new(),
        subtitle: String::new(),
        portrait: String::new(),
        portrait_large: String::new(),
        custom_image: String::new(),
        data: vec![0; 56],
        mole: 0,
        control: [0; 8],
        sight: [0; 8],
    };
    let base = lib.attributes.get(&o);
    o.health = base.health;
    o.max_health = o.health;
    for (at, value) in [
        (0, 56),
        (2, class::stored_base(class) as u16),
        (8, cell[0]),
        (10, cell[1]),
        (14, id),
        (18, o.health),
        (20, o.health),
        (22, base.attack),
        (24, base.defence),
    ] {
        o.data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    o.data[12] = owner;
    o.data[13] = civ;
    // 0x497970 initializes the sight byte from the civilization's attributes;
    // leaving it zero would incorrectly force five-cell visibility.
    o.data[26] = u8::try_from(base.sight).unwrap_or(255);
    Ok(o)
}

fn occupy(cells: &mut [bool], side: usize, at: [u16; 2], size: [usize; 2]) {
    for y in at[1] as usize..(at[1] as usize + size[1]).min(side) {
        for x in at[0] as usize..(at[0] as usize + size[0]).min(side) {
            cells[y * side + x] = true;
        }
    }
}

/// Expanding perimeter search, as the native placement helpers do. Reject
/// water/coast, trees, cliffs, occupied cells and footprints outside the map.
fn free_near(
    grid: &MapGrid,
    occupied: &[bool],
    at: [i32; 2],
    size: [usize; 2],
) -> Option<[u16; 2]> {
    let free = |x: i32, y: i32| {
        x >= 0
            && y >= 0
            && x as usize + size[0] < grid.side
            && y as usize + size[1] < grid.side
            && (y as usize..y as usize + size[1]).all(|cy| {
                (x as usize..x as usize + size[0]).all(|cx| {
                    let i = cy * grid.side + cx;
                    let cell = grid.cells[i];
                    !occupied[i]
                        && cell & 0xfc00 == 0
                        && (cell >> 4) & 7 != 0
                        && (cell >> 7) & 7 != 0
                })
            })
    };
    for radius in 0..grid.side as i32 {
        let (left, right, top, bottom) = (
            at[0] - radius,
            at[0] + radius,
            at[1] - radius,
            at[1] + radius,
        );
        for x in left..=right {
            for y in [top, bottom] {
                if free(x, y) {
                    return Some([x as u16, y as u16]);
                }
            }
        }
        for y in top + 1..bottom {
            for x in [left, right] {
                if free(x, y) {
                    return Some([x as u16, y as u16]);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placement_rejects_water_trees_cliffs_occupancy_and_map_edges() {
        let mut grid = MapGrid {
            name: String::new(),
            side: 32,
            cells: vec![0x90; 1024],
        };
        let mut occupied = vec![false; 1024];
        for code in [0, 0x80, 0x490, 0x890] {
            grid.cells[10 * 32 + 10] = code;
            assert_ne!(
                free_near(&grid, &occupied, [10, 10], [1, 1]),
                Some([10, 10])
            );
        }
        grid.cells[10 * 32 + 10] = 0x90;
        occupied[10 * 32 + 10] = true;
        assert_ne!(
            free_near(&grid, &occupied, [10, 10], [1, 1]),
            Some([10, 10])
        );
        occupied.fill(false);
        assert_eq!(
            free_near(&grid, &occupied, [10, 10], [8, 8]),
            Some([10, 10])
        );
        let cell = free_near(&grid, &occupied, [31, 31], [8, 8]).unwrap();
        assert!(cell[0] < 24 && cell[1] < 24);
    }
}
