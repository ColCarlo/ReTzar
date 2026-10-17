//! The map of islands against its native routines (0x468950 and the ones
//! it calls), on grids drawn here: `.` water, `#` land, `s` shore (ground
//! over water), `T` a standing tree, `t` a trunk, `^` a cliff.
use super::*;

const TRUNKS: u8 = 3;

fn grid(rows: &[&str]) -> MapGrid {
    let side = rows.len();
    let tile = |c: char| -> u16 {
        let ground = (1 << 4) | (1 << 7);
        match c {
            '.' => 0,
            '#' => ground,
            's' => 1 << 4,
            'T' => ground | (5 << 11),
            't' => ground | (2 << 11),
            '^' => ground | 0x400,
            _ => panic!("{c}"),
        }
    };
    let cells = rows
        .iter()
        .flat_map(|r| {
            assert_eq!(r.len(), side);
            r.chars().map(tile)
        })
        .collect();
    MapGrid {
        name: "test".into(),
        side,
        cells,
    }
}
fn islands(g: &MapGrid) -> Islands {
    Islands::from_grid(g, TRUNKS)
}

#[test]
fn the_kind_of_a_tile_reads_its_second_surface_first() {
    // 0x476da0 with one water surface: ground in bits 7–9 is land, ground
    // only in bits 4–6 a shore, a cliff nothing.
    assert_eq!(pathing::kind((1 << 4) | (2 << 7)), pathing::LAND);
    assert_eq!(pathing::kind(2 << 7), pathing::LAND);
    assert_eq!(pathing::kind(1 << 4), pathing::SHORE);
    assert_eq!(pathing::kind(0), pathing::WATER);
    assert_eq!(pathing::kind(0x400 | (1 << 7)), 0);
}

#[test]
fn lands_and_waters_become_regions_and_small_ones_are_wiped() {
    let g = grid(&[
        "............",
        "............",
        "..###..###..",
        "..###..###..",
        "..###..###..",
        "............",
        "............",
        "............",
        ".....#......",
        "............",
        "............",
        "............",
    ]);
    let s = islands(&g);
    // Row order: the water first, then the two islands; the lone cell has
    // fewer than 9 cells and is wiped.
    assert_eq!(s.next, 4);
    assert_eq!((s.kind(1), s.kind(2), s.kind(3)), (WATER, LAND, LAND));
    assert_eq!(s.region([3, 3]), 2);
    assert_eq!(s.region([8, 3]), 3);
    assert_eq!(s.region([5, 8]), WIPED);
    assert_eq!(s.regions[2].cells, 9);
    assert_eq!(s.regions[1].cells, 144 - 19);
    // Each island touches the water; the islands share it.
    assert_eq!(s.links(1, 2), link::ADJACENT);
    assert_eq!(s.links(2, 3), link::BY_WATER);
    assert_eq!(s.regions[2].other_links, 1);
    // 0x469da0 clears the wiped region's kind before unlinking it, so the
    // water keeps it among its land neighbours and takes it from the
    // others, which wrap.
    assert_eq!(s.regions[1].land_links, 3);
    assert_eq!(s.regions[1].other_links, u16::MAX);
    let mut s = s;
    assert!(!s.same_island(2, 3));
    assert!(s.same_island(3, 3));
}

#[test]
fn shores_go_to_the_last_region_that_reaches_them() {
    let g = grid(&[
        "..........",
        "..........",
        "..sssss...",
        "..s###s...",
        "..s###s...",
        "..s###s...",
        "..sssss...",
        "..........",
        "..........",
        "..........",
    ]);
    let s = islands(&g);
    // The water's flood takes the shore first, the island's flood takes it
    // back; each counts the shore cells it met.
    assert_eq!(s.region([2, 2]), 2);
    assert_eq!(s.region([6, 6]), 2);
    assert_eq!(s.regions[1].shore, 16);
    assert_eq!(s.regions[2].shore, 16);
    assert_eq!(s.regions[2].cells, 9);
    assert_eq!(s.links(1, 2), link::ADJACENT);
}

#[test]
fn water_counts_only_in_open_two_by_two_blocks() {
    // A channel one cell wide is no water region: the two lakes stay
    // apart, and the land around them is one region.
    let g = grid(&[
        "##########",
        "#...######",
        "#...######",
        "#...######",
        "#.........",
        "#####.####",
        "#####...##",
        "#####...##",
        "#####...##",
        "##########",
    ]);
    let s = islands(&g);
    assert_eq!(s.kind(1), LAND);
    assert_eq!((s.kind(2), s.kind(3)), (WATER, WATER));
    assert_eq!(s.region([5, 4]), NONE);
    assert_eq!(s.region([1, 4]), 2);
    // Two waters round one land.
    assert_eq!(s.links(2, 3), link::BY_WATER);
}

#[test]
fn trees_split_the_land_until_one_falls() {
    let g = grid(&[
        "##T#####", "##T#####", "##T#####", "##T#####", "##T#####", "##T#####", "##t#####",
        "##T#####",
    ]);
    let mut s = islands(&g);
    // The trunk is land: the two sides join through it.
    assert_eq!(s.next, 2);
    let g = grid(&[
        "##T#####", "##T#####", "##T#####", "##T#####", "##T#####", "##T#####", "##T#####",
        "##T#####",
    ]);
    s = islands(&g);
    assert_eq!(s.next, 3);
    let (left, right) = (s.region([0, 0]), s.region([5, 0]));
    assert!(!s.same_island(left, right));
    // 0x46a0e0: the felled tree takes the larger land around it and links
    // the two, which stay two regions.
    s.opened([2, 3, 2, 3]);
    assert_eq!(s.region([2, 3]), right);
    assert_eq!(s.links(left, right), link::ADJACENT);
    assert!(s.same_island(left, right));
    assert_eq!(s.regions[left as usize].land_links, 1);
}

#[test]
fn bridges_join_their_ends_until_the_last_goes() {
    let g = grid(&[
        "##########",
        "##########",
        "##########",
        "..........",
        "..........",
        "..........",
        "##########",
        "##########",
        "##########",
        "##########",
    ]);
    let mut s = islands(&g);
    let (north, south) = (s.region([0, 0]), s.region([0, 9]));
    assert!(!s.same_island(north, south));
    // A north–south bridge over rows 3–5: its ends are the rows above and
    // below it.
    s.bridge_at(&g, [4, 3, 5, 5], true, 7);
    s.bridge_at(&g, [7, 3, 7, 5], true, 8);
    assert_eq!(s.links(north, south), link::BRIDGE | link::BY_WATER);
    assert!(s.same_island(north, south));
    assert_eq!(
        (
            s.regions[north as usize].bridges,
            s.regions[north as usize].bridged
        ),
        (2, 1)
    );
    s.bridge_removed(7);
    assert!(s.same_island(north, south));
    s.bridge_removed(8);
    assert!(!s.same_island(north, south));
    assert_eq!(s.links(north, south), link::BY_WATER);
}

#[test]
fn past_254_regions_a_larger_one_wipes_the_smallest() {
    // 16 × 16 islands of 3 × 3 in one water: the water and 253 islands get
    // ids, the rest are wiped; the last island, of 4 × 4, wipes the first.
    let side = 80;
    let mut rows = vec![vec!['.'; side]; side];
    for i in 0..16 {
        for j in 0..16 {
            let n = if (i, j) == (15, 15) { 4 } else { 3 };
            for dy in 0..n {
                for dx in 0..n {
                    rows[i * 5 + 1 + dy][j * 5 + 1 + dx] = '#';
                }
            }
        }
    }
    let rows: Vec<String> = rows.into_iter().map(|r| r.into_iter().collect()).collect();
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    let s = islands(&grid(&refs));
    assert_eq!(s.next, 254);
    assert_eq!(s.region([1, 1]), WIPED);
    assert_eq!(s.region([6, 1]), 3);
    assert_eq!(s.region([76, 76]), 254);
    assert_eq!(s.kind(2), 0);
    // An island of 9 cells past the ids is wiped itself.
    assert_eq!(s.region([71, 76]), WIPED);
}

#[test]
fn the_nearest_region_is_found_column_by_column_then_row_by_row() {
    let g = grid(&[
        "###.......",
        "###.......",
        "###.......",
        "..........",
        "..........",
        "..........",
        ".......###",
        ".......###",
        ".......###",
        "..........",
    ]);
    let s = islands(&g);
    let (island, water, other) = (s.region([0, 0]), s.region([3, 0]), s.region([9, 6]));
    assert_eq!(
        (s.kind(island), s.kind(water), s.kind(other)),
        (LAND, WATER, LAND)
    );
    // 0x46aff0: the top and bottom cells of each column first, then the
    // left and right cells of each row; a kind filters the cells.
    assert_eq!(s.nearest(&g, [0, 0, 9, 9], 0), island);
    assert_eq!(s.nearest(&g, [3, 0, 9, 9], 0), water);
    assert_eq!(s.nearest(&g, [3, 0, 9, 9], LAND), other);
    assert_eq!(s.nearest(&g, [5, 2, 9, 6], LAND), other);
    assert_eq!(s.nearest(&g, [3, 0, 6, 9], LAND), NONE);
}
