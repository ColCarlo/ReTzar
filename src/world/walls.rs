//! Native wall adjacency and damage pictures (0x4fb690, 0x4fbde0).
use super::*;
use std::collections::BTreeMap;

fn material(o: &MapObject) -> Option<u8> {
    match o.class {
        class::WALL_STONE | class::GATE_STONE => Some(0),
        class::WALL_WOOD | class::GATE_WOOD => Some(1),
        _ => None,
    }
}

fn shape(neighbors: u16) -> u16 {
    match neighbors {
        5 => 24,  // north and south, without a junction
        10 => 16, // east and west, without a junction
        other => other,
    }
}

impl MissionWorld {
    /// Refresh after custom starts/player filtering. Gates occupy multiple
    /// cells; adjacent completed walls of the same material connect regardless
    /// of owner (the original visual lookup passes owner zero).
    pub fn refresh_walls(&mut self, lib: &ObjectLibrary, side: usize) {
        let mut cells = BTreeMap::new();
        for o in &self.objects {
            let Some(kind) =
                material(o).filter(|_| o.selectable() && o.flags & FLAG_CONSTRUCTION == 0)
            else {
                continue;
            };
            let size = lib.cell_footprint(o);
            for y in o.y as usize..(o.y as usize + size[1]).min(side) {
                for x in o.x as usize..(o.x as usize + size[0]).min(side) {
                    cells.insert((x as i32, y as i32), kind);
                }
            }
        }
        for o in self
            .objects
            .iter_mut()
            .filter(|o| matches!(o.class, class::WALL_STONE | class::WALL_WOOD) && o.selectable())
        {
            let (x, y) = (o.x as i32, o.y as i32);
            let kind = material(o).unwrap();
            let mask = [(x, y - 1), (x + 1, y), (x, y + 1), (x - 1, y)]
                .iter()
                .enumerate()
                .fold(0, |bits, (i, cell)| {
                    bits | if cells.get(cell) == Some(&kind) {
                        1 << i
                    } else {
                        0
                    }
                });
            // Native 0x4fae60 rebuilds the connection and resets both picture
            // coordinates to zero. The low mask bits are not a sprite column.
            let mask = shape(mask);
            o.variant = 0;
            if o.data.len() >= 18 {
                o.data[16..18].copy_from_slice(&0u16.to_le_bytes());
            }
            if o.data.len() < 58 {
                o.data.resize(58, 0);
            }
            o.data[56..58].copy_from_slice(&mask.to_le_bytes());
        }
    }
}

impl MapObject {
    pub(super) fn wall_damaged(&self) -> bool {
        matches!(self.class, class::WALL_STONE | class::WALL_WOOD)
            && self.max_health > 0
            && self.health < self.max_health
    }
    pub(super) fn wall_damage_row(&self, rows: usize) -> usize {
        if !self.wall_damaged() || rows == 0 {
            return 0;
        }
        // Native: rows - floor(health * rows / max_health) - 1.
        rows.saturating_sub(self.health as usize * rows / self.max_health as usize + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wall_damage_has_native_thresholds_and_no_division_by_zero() {
        let mut o = MapObject {
            id: 1,
            class: class::WALL_STONE,
            flags: 0,
            x: 0,
            y: 0,
            owner: 1,
            civilization: 1,
            variant: 0,
            health: 100,
            max_health: 100,
            name: String::new(),
            subtitle: String::new(),
            portrait: String::new(),
            portrait_large: String::new(),
            custom_image: String::new(),
            data: vec![0; 58],
            mole: 0,
            control: [0; 8],
            sight: [0; 8],
        };
        assert!(!o.wall_damaged());
        for (hp, row) in [
            (99, 0),
            (80, 0),
            (79, 1),
            (60, 1),
            (59, 2),
            (40, 2),
            (39, 3),
            (20, 3),
            (19, 4),
            (0, 4),
        ] {
            o.health = hp;
            assert!(o.wall_damaged());
            assert_eq!(o.wall_damage_row(5), row);
        }
        o.max_health = 0;
        assert!(!o.wall_damaged());
        assert_eq!(o.wall_damage_row(5), 0);
        assert_eq!(o.wall_damage_row(0), 0);
    }
}
