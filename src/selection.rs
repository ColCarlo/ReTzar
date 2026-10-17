//! Inspection selection, matching Tzar.exe 1.11's 0x453200–0x45387f.
//! Allies that share their units (diplomacy bit 4) are controlled like own units.
use crate::map::MapGrid;
use crate::world::{Layer, MapObject, MissionWorld, ObjectLibrary, ObjectVisual, class};

/// 0x453254 rejects an addition when the existing count is greater than 49.
pub const MAX_SELECTED: usize = 50;

pub fn own_unit(o: &MapObject, mission: &MissionWorld) -> bool {
    o.selectable() && mission.commands(o) && unit(o)
}

/// Native controllability (0x2a0, 0x506c30): owned objects and those of
/// allies sharing their control, units and buildings alike. Groups, orders
/// and the production panel use it.
pub fn controllable(o: &MapObject, mission: &MissionWorld) -> bool {
    o.selectable() && mission.commands(o) && (unit(o) || o.building())
}
/// 0x453200: only controlled units share a selection; a building, an enemy
/// or a neutral object is always alone.
pub fn mixable(o: &MapObject, mission: &MissionWorld) -> bool {
    o.selectable() && mission.controls(o.owner) && unit(o)
}
/// Whether two objects may share a selection: controlled units, as in the
/// original, and, in this version, controlled buildings of one kind, so that
/// they train and set their rally point together.
pub fn together(a: &MapObject, b: &MapObject, mission: &MissionWorld) -> bool {
    (mixable(a, mission) && mixable(b, mission))
        || (a.building()
            && b.building()
            && a.class == b.class
            && controllable(a, mission)
            && controllable(b, mission))
}

pub fn unit(o: &MapObject) -> bool {
    (class::PEON..=class::MESSIAH).contains(&o.class)
        || [class::COW, class::FISHING_BOAT].contains(&o.class)
}
/// Combat units (flag 0x10000): every unit but peasants, traders, carpets,
/// cows, birds, eagles and fishing boats.
pub fn combat(o: &MapObject) -> bool {
    unit(o)
        && ![
            class::PEON,
            2,
            22,
            class::COW,
            class::BIRD,
            class::EAGLE,
            class::FISHING_BOAT,
        ]
        .contains(&o.class)
}

/// 0x453240, the only way into the selection: an object already selected
/// stays, at most 50, and a new object that cannot share the selection with
/// the first ([`together`]) clears it first.
pub fn add(selected: &mut Vec<usize>, candidates: &[usize], mission: &MissionWorld) {
    for &i in candidates {
        let Some(o) = mission.objects.get(i).filter(|o| o.selectable()) else {
            continue;
        };
        if selected.contains(&i) {
            continue;
        }
        if selected.first().is_some_and(|&v| {
            mission
                .objects
                .get(v)
                .is_none_or(|first| !together(first, o, mission))
        }) {
            selected.clear();
        }
        if selected.len() < MAX_SELECTED {
            selected.push(i);
        }
    }
}

/// A click (0x4533b0): without Shift the object alone, with Shift it is
/// taken out if selected, otherwise added (0x453320).
pub fn apply(selected: &mut Vec<usize>, candidates: &[usize], shift: bool, mission: &MissionWorld) {
    if !shift {
        selected.clear();
    }
    for &i in candidates {
        if shift && let Some(at) = selected.iter().position(|&v| v == i) {
            selected.remove(at);
            continue;
        }
        add(selected, &[i], mission);
    }
}

/// What a click on the map takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Object(usize),
    /// The cell of a standing tree, selected through a `treeselection`
    /// object (0x453440).
    Tree(usize),
}
/// The original's distance from a click to a unit, under which the unit is
/// taken (0x44ea10).
pub const CLICK_REACH: i32 = 30;

/// A click at the world point `p` (scene pixels, y upwards), as 0x4f4f14
/// and 0x4533b0 take it. Nothing on a cell with a corner out of view; then
/// the unit whose picture has its middle nearest the click, under
/// [`CLICK_REACH`] pixels each way (0x44e900: units on the cells round the
/// clicked one, seen by the player); else an object on the clicked cell
/// (0x44e400 with 0x44ea20: a unit on it, or a building or a mine over it
/// where its mask takes the cell); else a standing tree.
pub fn click(
    mission: &MissionWorld,
    lib: &ObjectLibrary,
    grid: &MapGrid,
    visuals: &[ObjectVisual],
    p: [f32; 2],
) -> Option<Pick> {
    let [px, py] = [p[0].floor() as i32, (-p[1]).floor() as i32];
    let side = grid.side as i32;
    let [cx, cy] = [px.div_euclid(32), py.div_euclid(32)];
    if !(0..side).contains(&cx) || !(0..side).contains(&cy) {
        return None;
    }
    let sim = mission.simulation.as_ref();
    if let Some(s) = sim
        && ![[0, 0], [1, 0], [0, 1], [1, 1]]
            .iter()
            .all(|d| s.fog.vertex(cx as usize + d[0], cy as usize + d[1]).1)
    {
        return None;
    }
    let seen = |o: &MapObject| o.selectable() && sim.is_none_or(|s| s.visible(o));
    if let Some(i) = pointed(mission, lib, visuals, p, |_, o| seen(o)) {
        return Some(Pick::Object(i));
    }
    let cell = cy as usize * grid.side + cx as usize;
    standing_tree(lib, grid, cell).then_some(Pick::Tree(cell))
}
/// The object at the world point `p` as the original finds it for a click
/// and for the target of a right click or an order (0x4e4280 and 0x4533b0
/// call the same two searches): the unit among those `seen` whose picture
/// has its middle nearest the point, under [`CLICK_REACH`] pixels each way
/// (0x44e900, units of the cells round the point's); else the newest object
/// on the point's cell, a unit standing on it or a building or a mine whose
/// mask takes it (0x44e400). Never by where pictures overlap: a unit on a
/// bridge is found before the bridge.
pub fn pointed(
    mission: &MissionWorld,
    lib: &ObjectLibrary,
    visuals: &[ObjectVisual],
    p: [f32; 2],
    seen: impl Fn(usize, &MapObject) -> bool,
) -> Option<usize> {
    let [px, py] = [p[0].floor() as i32, (-p[1]).floor() as i32];
    let [cx, cy] = [px.div_euclid(32), py.div_euclid(32)];
    if cx < 0 || cy < 0 {
        return None;
    }
    // The first picture of each object drawn as a body.
    let mut drawn = std::collections::BTreeMap::new();
    for v in visuals.iter().filter(|v| v.layer == Layer::Body) {
        if mission
            .objects
            .get(v.object)
            .is_some_and(|o| seen(v.object, o))
        {
            drawn.entry(v.object).or_insert(v);
        }
    }
    // The cells are walked by column, the newest object of a cell first.
    let mut near: Vec<(usize, &MapObject, &ObjectVisual)> = drawn
        .iter()
        .map(|(&i, &v)| (i, &mission.objects[i], v))
        .filter(|(_, o, _)| unit(o) && (o.x as i32 - cx).abs() <= 1 && (o.y as i32 - cy).abs() <= 1)
        .collect();
    near.sort_by_key(|&(i, o, _)| (o.x, o.y, std::cmp::Reverse(i)));
    let mut best: Option<(i32, usize)> = None;
    for (i, _, v) in near {
        // 0x44e9a5: the middle of the picture's box, by whole pixels.
        let [x, w, h] = [v.bounds[0], v.bounds[2], v.bounds[3]].map(|n| n.round() as i32);
        let top = -(v.bounds[1] + v.bounds[3]).round() as i32;
        let gap = (x + w / 2 - px).abs().max((top + h / 2 - py).abs());
        if best.is_none_or(|(b, _)| gap < b) {
            best = Some((gap, i));
        }
    }
    if let Some((gap, i)) = best
        && gap < CLICK_REACH
    {
        return Some(i);
    }
    let [ux, uy] = [cx as usize, cy as usize];
    drawn.keys().rev().copied().find(|&i| {
        let o = &mission.objects[i];
        if unit(o) {
            return (o.x as usize, o.y as usize) == (ux, uy);
        }
        let [w, h] = lib.cell_footprint(o);
        let [ox, oy] = [o.x as usize, o.y as usize];
        (ox..ox + w).contains(&ux)
            && (oy..oy + h).contains(&uy)
            && !lib.free_drawn_cell(o, ux - ox, uy - oy)
    })
}
/// The wood a chosen tree shows (0x437bc0): ten for each load left, of 100.
pub fn tree_wood(sim: &crate::simulation::Simulation, cell: usize) -> u32 {
    sim.gameplay.trees.get(&cell).copied().unwrap_or(100)
}
/// A standing tree on the cell: an image above the trunks (a felled one has
/// none in this version).
pub fn standing_tree(lib: &ObjectLibrary, grid: &MapGrid, cell: usize) -> bool {
    grid.cells
        .get(cell)
        .is_some_and(|&c| ((c >> 11) & 31) as u8 > lib.trunks)
}

/// An object belongs to one numbered group, as the original's byte +0x56.
pub fn assign_group(
    groups: &mut [Vec<usize>; 9],
    group: usize,
    selected: &[usize],
    mission: &MissionWorld,
) {
    remove_from_groups(groups, selected);
    groups[group] = selected
        .iter()
        .copied()
        .filter(|&i| {
            mission
                .objects
                .get(i)
                .is_some_and(|o| o.selectable() && mission.controls(o.owner))
        })
        .take(MAX_SELECTED)
        .collect();
}

pub fn remove_from_groups(groups: &mut [Vec<usize>; 9], selected: &[usize]) {
    for group in groups {
        group.retain(|i| !selected.contains(i));
    }
}
