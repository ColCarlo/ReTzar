//! Native movement: the Move opcode (9, 0x4def20) and the steps it starts
//! (0x4debb0), timed by the walking picture's `move_len`, over the cell
//! bits of [`pathing`].

use super::pathing::{
    self, BLOCKED, BRIDGE, CLIFF, Cells, Cost, FIELD, GATEWAY, OCCUPIED, Query, Regions, STATIC,
    TRANSIT, WALL, WATER,
};
use super::*;

/// The map's cells and regions, refreshed every tick and kept up to date by
/// the steps within it. Loading builds the cells again; the graph of regions
/// is saved (see [`Regions`]).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Pathing {
    #[serde(skip)]
    pub(crate) cells: Cells,
    pub(crate) regions: Regions,
    #[serde(skip)]
    pub(crate) fixed: Fixed,
}
impl PartialEq for Pathing {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for Pathing {}

/// The step in progress: the unit already belongs to the cell it enters
/// and its picture slides from `from`, a pixel every `step_ms`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Walk {
    pub from: [u16; 2],
    pub start_ms: u64,
    pub due_ms: u64,
    pub step_ms: u32,
    /// A ninja's jump over a wall or a cliff: the pixel steps of
    /// `a_ninja_jump` (its rows), 0 for a plain step.
    #[serde(default)]
    pub jump: u8,
}
/// A Move op: its rect, range and followed object (program +0x8e), its
/// state (+0xa: 0 start, 1 walking, 2 blocked), waits (+0xd) and the time
/// it runs again.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Movement {
    /// The rect the program asked for, and the one walked to (shifted by
    /// the unit's place in its group).
    pub asked: [i32; 4],
    pub rect: [i32; 4],
    pub range: u32,
    pub target: Option<usize>,
    pub state: u8,
    pub waits: u8,
    pub due_ms: u64,
    pub failed: bool,
    /// The program ran the op during this tick (see [`settle`]).
    #[serde(skip)]
    pub ran: bool,
}

/// Units given one order together (0x4518f0, +0xe6), and the paths found
/// for them, newest first.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Group {
    pub members: Vec<usize>,
    pub paths: Vec<GroupPath>,
}
/// A path searched from a point of the group (0x451efd): its start, the
/// rect it leads to (grown by the range), the followed object and the range
/// it was asked for, its nodes, and the cells of the target's edge members
/// took, with who took them (+0x1a).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupPath {
    pub start: [i32; 2],
    pub rect: [i32; 4],
    #[serde(default)]
    pub target: Option<usize>,
    pub range: u32,
    pub path: Vec<[u16; 2]>,
    #[serde(default)]
    pub taken: Vec<([i32; 2], usize)>,
}
/// Chebyshev distance (0x454dc0).
fn chebyshev(a: [i32; 2], b: [i32; 2]) -> i32 {
    (a[0] - b[0]).abs().max((a[1] - b[1]).abs())
}
/// 0x451b40: the `n`th cell of the edge of `rect` from `p`, alternately
/// ahead and behind (down the left side, right along the bottom, up the
/// right side, left along the top).
pub(super) fn edge_cell(rect: [i32; 4], p: [i32; 2], n: i32) -> [i32; 2] {
    let steps = if n & 1 != 0 {
        2 * ((rect[3] - rect[1]) + (rect[2] - rect[0])) - n / 2 - 1
    } else {
        n / 2
    };
    let [mut x, mut y] = p;
    for _ in 0..steps.max(0) {
        if x == rect[0] && y < rect[3] {
            y += 1;
        } else if y == rect[3] && x < rect[2] {
            x += 1;
        } else if x == rect[2] && y > rect[1] {
            y -= 1;
        } else {
            x -= 1;
        }
    }
    [x, y]
}
/// 0x451cc0: the group's path found from a unit within ten cells, or a new
/// one searched from the unit averaged in turn with every member within ten
/// cells of the running centre, shifted by the unit's offset from that
/// start and kept on the map. To a point the point is shifted too and
/// returned. Toward an object with range 0 the path stops before eight
/// cells from the object's centre (0x454d70); the unit then takes the
/// first cell of the object's edge, from where the path ends, that no
/// member took and that is free or being entered, and walks there.
fn group_path(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    rect: [i32; 4],
    range: u32,
    target: Option<usize>,
) -> Option<(VecDeque<[u16; 2]>, Option<[i32; 4]>)> {
    let id = sim.gameplay.actors[i].group?;
    let o = &m.objects[i];
    let here = [o.x as i32, o.y as i32];
    let last = sim.gameplay.pathing.cells.side as i32 - 1;
    let r = range as i32;
    let base = match target {
        Some(j) if m.objects.get(j).is_some_and(alive) => target_rect(sim, m, lib, j),
        _ => rect,
    };
    let grown = [
        (base[0] - r).max(0),
        (base[1] - r).max(0),
        (base[2] + r).min(last),
        (base[3] + r).min(last),
    ];
    let group = sim.gameplay.groups.get(&id)?;
    let found = group.paths.iter().position(|p| {
        (target.is_some() && p.target == target || p.rect == rect)
            && p.range == range
            && chebyshev(p.start, here) < 10
    });
    let slot = match found {
        Some(k) => {
            // 0x451c60: the unit gives back the cell it took.
            let entry = &mut sim.gameplay.groups.get_mut(&id)?.paths[k];
            entry.taken.retain(|&(_, w)| w != i);
            k
        }
        None => {
            let mut centre = here;
            for &k in &group.members {
                if k == i || !m.objects.get(k).is_some_and(alive) {
                    continue;
                }
                let q = [m.objects[k].x as i32, m.objects[k].y as i32];
                if chebyshev(centre, q) < 10 {
                    centre = [(centre[0] + q[0]) / 2, (centre[1] + q[1]) / 2];
                }
            }
            let path = search_from(sim, lib, o, centre, grown, 0, false)?;
            if path.is_empty() {
                return None;
            }
            let entry = GroupPath {
                start: centre,
                rect: grown,
                target,
                range,
                path: path.into(),
                taken: Vec::new(),
            };
            sim.gameplay.groups.get_mut(&id)?.paths.insert(0, entry);
            0
        }
    };
    let entry = sim.gameplay.groups.get(&id)?.paths[slot].clone();
    let d = [here[0] - entry.start[0], here[1] - entry.start[1]];
    let keep = |p: [i32; 2]| [p[0].clamp(0, last), p[1].clamp(0, last)];
    let shifted = |p: &[u16; 2]| keep([p[0] as i32 + d[0], p[1] as i32 + d[1]]);
    if grown[0] == grown[2] && grown[1] == grown[3] {
        let point = keep([grown[0] + d[0], grown[1] + d[1]]);
        let path = entry
            .path
            .iter()
            .map(shifted)
            .map(|p| [p[0] as u16, p[1] as u16])
            .collect();
        return Some((path, Some([point[0], point[1], point[0], point[1]])));
    }
    let j = target.filter(|_| range == 0)?;
    let t = &m.objects[j];
    let [w, h] = footprint(lib, t).map(|n| n as i32);
    let centre = [t.x as i32 + w / 2, t.y as i32 + h / 2];
    let end = entry.path.last().map(|p| [p[0] as i32, p[1] as i32])?;
    let mut path = VecDeque::new();
    let mut near = false;
    for p in entry.path.iter().map(shifted) {
        if chebyshev(p, centre) < 8 {
            near = true;
            break;
        }
        path.push_back([p[0] as u16, p[1] as u16]);
    }
    if !near {
        return Some((path, None));
    }
    let layer = lib.layer(o);
    let cells = &sim.gameplay.pathing.cells;
    let edge = 2 * ((grown[3] - grown[1] + 1) + (grown[2] - grown[0] + 1)) - 4;
    let Some(cell) = (0..edge).map(|n| edge_cell(grown, end, n)).find(|&c| {
        cells.inside(c)
            && !entry.taken.iter().any(|&(q, _)| q == c)
            && (cells.free(layer, c) || cells.at(c) & TRANSIT[layer] != 0)
    }) else {
        return Some((path, None));
    };
    let from = path.back().map_or(here, |p| [p[0] as i32, p[1] as i32]);
    let point = [cell[0], cell[1], cell[0], cell[1]];
    let Some(tail) = search_from(sim, lib, o, from, point, 0, false) else {
        return Some((path, None));
    };
    sim.gameplay.groups.get_mut(&id)?.paths[slot]
        .taken
        .push((cell, i));
    path.extend(tail);
    Some((path, None))
}

fn now(sim: &Simulation) -> u64 {
    sim.ticks * TICK_MS
}
fn at(p: [u16; 2]) -> [i32; 2] {
    [p[0] as i32, p[1] as i32]
}

/// The object stands on the map: alive, neither held nor dying, in no holder.
fn on_map(sim: &Simulation, k: usize, o: &MapObject) -> bool {
    alive(o)
        && o.flags & (FLAG_HELD | FLAG_DYING) == 0
        && !sim.objects.get(k).is_some_and(|s| s.holder.is_some())
}
/// 0x500b30 / 0x500c60: a moving object's cell in its own layer; without
/// its occupancy bit when the open gate whose door it stands in comes after
/// it.
fn mark_mover(
    cells: &mut Cells,
    sim: &Simulation,
    lib: &ObjectLibrary,
    k: usize,
    o: &MapObject,
    t: u64,
    cleared: bool,
) {
    let p = [o.x as i32, o.y as i32];
    let layer = lib.layer(o);
    if !cleared {
        cells.set(p, OCCUPIED[layer], true);
    }
    if sim.gameplay.actors.get(k).is_some_and(|a| {
        a.transit && (a.movement.is_some() || a.walk.as_ref().is_some_and(|w| w.due_ms > t))
    }) {
        cells.set(p, TRANSIT[layer], true);
    }
}
/// The cells of the map with what stands on them (the occupy routines,
/// vt+0x70), every object in turn.
fn build(sim: &Simulation, m: &MissionWorld, g: &MapGrid, lib: &ObjectLibrary) -> Cells {
    let mut cells = Cells::new(g, lib.trunks);
    let t = now(sim);
    for (k, o) in m.objects.iter().enumerate() {
        if !on_map(sim, k, o) {
            continue;
        }
        if mobile(o) {
            mark_mover(&mut cells, sim, lib, k, o, t, false);
        } else {
            occupy(&mut cells, lib, o, &mut Vec::new());
        }
    }
    cells
}

/// What does not move: the terrain and the objects that stand still, kept
/// between ticks; where some of them change, only the cells they write are
/// made again, from the terrain and every standing object over them in the
/// objects' order. With the movers' marks added it equals `build`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Fixed {
    cells: Cells,
    /// The terrain it was built on.
    grid: Vec<u16>,
    /// Each object's print (`print`), 0 for one that does not stand on the
    /// map.
    prints: Vec<u64>,
    /// The cells each standing object may write (`reach`).
    reach: Vec<Option<[i32; 4]>>,
    /// The marks each standing object adds to `Cells::marks`.
    marks: Vec<Vec<([i32; 4], [i32; 4])>>,
    /// Cells whose last fixed write left the land occupancy off (the door of
    /// an open gate), with that gate's index: a mover before it in the
    /// objects' order loses the bit there, as in `build`.
    doors: BTreeMap<usize, usize>,
    /// The cells the movers marked at the last refresh.
    marked: Vec<usize>,
    /// The cells being made again, while they are.
    dirty: Vec<bool>,
}
/// FxHash's mixing, a word at a time: fast enough for every tick.
#[derive(Default)]
struct Fx(u64);
impl Fx {
    fn add(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}
impl std::hash::Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for &w in words {
            self.add(u64::from_le_bytes(w));
        }
        if !rest.is_empty() {
            let mut b = [0u8; 8];
            b[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(b) ^ (rest.len() as u64) << 56);
        }
    }
    fn write_u8(&mut self, n: u8) {
        self.add(n as u64)
    }
    fn write_u16(&mut self, n: u16) {
        self.add(n as u64)
    }
    fn write_u32(&mut self, n: u32) {
        self.add(n as u64)
    }
    fn write_u64(&mut self, n: u64) {
        self.add(n)
    }
    fn write_usize(&mut self, n: usize) {
        self.add(n as u64)
    }
}
/// What `occupy` reads of an object that stands still on the map (picture
/// and footprint, gate and bridge state, place), never 0; 0 for any other.
fn print(sim: &Simulation, k: usize, o: &MapObject) -> u64 {
    use std::hash::{Hash, Hasher};
    if mobile(o) || !on_map(sim, k, o) {
        return 0;
    }
    let mut h = Fx::default();
    (o.class, o.x, o.y, o.flags, o.variant, o.civilization).hash(&mut h);
    o.custom_image.hash(&mut h);
    o.data.hash(&mut h);
    (o.health < o.max_health, o.max_health > 0).hash(&mut h);
    h.finish() | 1
}
/// The movers on the map, in order, and every object's print.
fn movers_and_prints(sim: &Simulation, m: &MissionWorld) -> (Vec<usize>, Vec<u64>) {
    let mut movers = Vec::new();
    let mut prints = Vec::with_capacity(m.objects.len());
    for (k, o) in m.objects.iter().enumerate() {
        if mobile(o) && on_map(sim, k, o) {
            movers.push(k);
        }
        prints.push(print(sim, k, o));
    }
    (movers, prints)
}
/// The cells `occupy` may write for a standing object: its footprint, the
/// cells beside a gate's door, an anchor's own cell.
fn reach(lib: &ObjectLibrary, o: &MapObject) -> [i32; 4] {
    let [x, y] = [o.x as i32, o.y as i32];
    let [w, h] = lib.cell_footprint(o).map(|n| n as i32);
    let mut r = [x, y, x + w - 1, y + h - 1];
    let mut grow = |b: [i32; 4]| {
        r = [
            r[0].min(b[0]),
            r[1].min(b[1]),
            r[2].max(b[2]),
            r[3].max(b[3]),
        ];
    };
    match o.class {
        class::GATE_WOOD | class::GATE_STONE => grow(gate_beside(o)),
        class::CRUSADEANCHOR => grow([x, y, x, y]),
        _ => {}
    }
    r
}
fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}
/// The marks a standing object adds, without writing any cell.
fn marks_alone(lib: &ObjectLibrary, o: &MapObject) -> Vec<([i32; 4], [i32; 4])> {
    let mut nowhere = Cells::default();
    occupy(&mut nowhere, lib, o, &mut Vec::new());
    nowhere.marks
}
/// The doors of open gates that leave the land occupancy off where they
/// are, each with the last gate over it.
fn open_doors(
    cells: &Cells,
    doors: &mut BTreeMap<usize, usize>,
    cleared: impl IntoIterator<Item = ([i32; 2], usize)>,
) {
    for (p, k) in cleared {
        if cells.inside(p) && cells.at(p) & OCCUPIED[1] == 0 {
            let gate = doors.entry(cells.index(p)).or_insert(k);
            *gate = (*gate).max(k);
        }
    }
}
fn build_fixed(m: &MissionWorld, g: &MapGrid, lib: &ObjectLibrary, prints: Vec<u64>) -> Fixed {
    let n = m.objects.len();
    let mut cells = Cells::new(g, lib.trunks);
    let mut cleared = Vec::new();
    let mut reaches = vec![None; n];
    let mut marks = vec![Vec::new(); n];
    for (k, o) in m.objects.iter().enumerate() {
        if prints[k] == 0 {
            continue;
        }
        let first = cells.marks.len();
        let mut doors = Vec::new();
        occupy(&mut cells, lib, o, &mut doors);
        marks[k] = cells.marks[first..].to_vec();
        reaches[k] = Some(reach(lib, o));
        cleared.extend(doors.into_iter().map(|p| (p, k)));
    }
    let mut doors = BTreeMap::new();
    open_doors(&cells, &mut doors, cleared);
    Fixed {
        dirty: vec![false; cells.word.len()],
        cells,
        grid: g.cells.clone(),
        prints,
        reach: reaches,
        marks,
        doors,
        marked: Vec::new(),
    }
}
/// The fixed cells made again where something changed: the objects whose
/// print changed (where they were and where they are) and the terrain cells
/// that changed. Those cells go back to the terrain and every standing
/// object that may write them occupies them again in the objects' order,
/// its writes elsewhere undone; the marks and the doors follow. Returns the
/// cells made again, in increasing order.
fn update_fixed(
    f: &mut Fixed,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    prints: Vec<u64>,
) -> Vec<usize> {
    let n = m.objects.len();
    let side = g.side as i32;
    let old = |k: usize| f.prints.get(k).copied().unwrap_or(0);
    let changed: Vec<usize> = (0..n.max(f.prints.len()))
        .filter(|&k| prints.get(k).copied().unwrap_or(0) != old(k))
        .collect();
    let mut rects: Vec<[i32; 4]> = Vec::new();
    for &k in &changed {
        if let Some(Some(r)) = f.reach.get(k) {
            rects.push(*r);
        }
    }
    f.reach.resize(n, None);
    f.marks.resize(n, Vec::new());
    for &k in changed.iter().filter(|&&k| k < n) {
        let o = &m.objects[k];
        if prints[k] == 0 {
            f.reach[k] = None;
            f.marks[k] = Vec::new();
        } else {
            let r = reach(lib, o);
            rects.push(r);
            f.reach[k] = Some(r);
            f.marks[k] = marks_alone(lib, o);
        }
    }
    f.prints = prints;
    if !changed.is_empty() {
        f.cells.marks = f.marks.iter().flatten().copied().collect();
    }
    if f.grid != g.cells {
        for (i, (a, b)) in f.grid.iter_mut().zip(&g.cells).enumerate() {
            if a != b {
                *a = *b;
                let p = [(i % g.side) as i32, (i / g.side) as i32];
                rects.push([p[0], p[1], p[0], p[1]]);
            }
        }
    }
    rects = rects
        .into_iter()
        .map(|r| {
            [
                r[0].max(0),
                r[1].max(0),
                r[2].min(side - 1),
                r[3].min(side - 1),
            ]
        })
        .filter(|r| r[0] <= r[2] && r[1] <= r[3])
        .collect();
    let mut dirty = Vec::new();
    for r in &rects {
        for y in r[1]..=r[3] {
            for x in r[0]..=r[2] {
                let i = (y * side + x) as usize;
                if !f.dirty[i] {
                    f.dirty[i] = true;
                    dirty.push(i);
                }
            }
        }
    }
    if dirty.is_empty() {
        return dirty;
    }
    dirty.sort_unstable();
    for &i in &dirty {
        let t = g.cells[i];
        f.cells.word[i] = t as u32 | pathing::kind(t);
    }
    // Every standing object over those cells, in order; the marks it adds
    // again are dropped and its writes outside them undone.
    let marks = f.cells.marks.len();
    let mut cleared = Vec::new();
    let mut kept = Vec::new();
    for (k, o) in m.objects.iter().enumerate() {
        let Some(r) = f.reach[k] else {
            continue;
        };
        if !rects.iter().any(|d| overlap(*d, r)) {
            continue;
        }
        kept.clear();
        for y in r[1].max(0)..=r[3].min(side - 1) {
            for x in r[0].max(0)..=r[2].min(side - 1) {
                let i = (y * side + x) as usize;
                if !f.dirty[i] {
                    kept.push((i, f.cells.word[i]));
                }
            }
        }
        let mut doors = Vec::new();
        occupy(&mut f.cells, lib, o, &mut doors);
        f.cells.marks.truncate(marks);
        for &(i, w) in &kept {
            f.cells.word[i] = w;
        }
        cleared.extend(
            doors
                .into_iter()
                .filter(|&p| f.cells.inside(p) && f.dirty[f.cells.index(p)])
                .map(|p| (p, k)),
        );
    }
    f.doors.retain(|i, _| !f.dirty[*i]);
    open_doors(&f.cells, &mut f.doors, cleared);
    for &i in &dirty {
        f.dirty[i] = false;
    }
    dirty
}
fn occupy(cells: &mut Cells, lib: &ObjectLibrary, o: &MapObject, doors: &mut Vec<[i32; 2]>) {
    let [x, y] = [o.x as i32, o.y as i32];
    let size = |o: &MapObject| lib.cell_footprint(o).map(|n| n as i32);
    let each = |cells: &mut Cells, f: &mut dyn FnMut(&mut Cells, [i32; 2], bool)| {
        let [w, h] = size(o);
        for dy in 0..h {
            for dx in 0..w {
                let free = lib.free_cell(o, dx as usize, dy as usize);
                f(cells, [x + dx, y + dy], free);
            }
        }
    };
    let [w, h] = size(o);
    let own = [x, y, x + w - 1, y + h - 1];
    if (o.building()
        || matches!(
            o.class,
            class::RES_GOLD
                | class::RES_STONE
                | class::RUIN
                | class::GROUND_OBJECT
                | class::CRUSADEANCHOR
        ))
        && !matches!(o.class, class::FIELD | class::GATE_WOOD | class::GATE_STONE)
    {
        cells.marks.push((own, own));
    }
    match o.class {
        // 0x44c930: a field is walkable.
        class::FIELD => each(cells, &mut |c, p, _| c.set(p, FIELD, true)),
        // 0x41c010: the terrain under a bridge does not count; an unfinished
        // bridge blocks, a finished one only along its rails.
        class::BRIDGE => {
            let [w, h] = size(o);
            let building = o.flags & FLAG_CONSTRUCTION != 0;
            let vertical = o.data.get(56).copied().unwrap_or(0) != 0;
            each(cells, &mut |c, p, _| {
                let rail = if vertical {
                    p[0] == x || p[0] == x + w - 1
                } else {
                    p[1] == y || p[1] == y + h - 1
                };
                c.set(p, BRIDGE, true);
                c.set(p, BLOCKED, building || rail);
            });
        }
        // 0x456210 / 0x4576bc: an open gate frees only its door.
        class::GATE_WOOD | class::GATE_STONE => {
            let open = o.byte(57) == 0;
            let facing = o.byte(56) & 3;
            let vertical = facing >= 2;
            each(cells, &mut |c, p, _| {
                let door = if vertical {
                    p[0] == x && (y + 1..=y + 3).contains(&p[1])
                } else {
                    p[1] == y && (x + 2..=x + 4).contains(&p[0])
                };
                c.set(p, STATIC, true);
                c.set(p, OCCUPIED[1], !(open && door));
                if open && door {
                    doors.push(p);
                }
                // 0x4562cb: the door's cells (0x457a40) wait for it.
                if door {
                    c.set(p, GATEWAY, true);
                }
            });
            // 0x4577b4: while open, so do the cells beside the door
            // (0x457ac0); a stone gate facing up reaches two rows out. A
            // change of the gate passes that rect to the regions (0x4577dd).
            let beside = gate_beside(o);
            if open {
                for py in beside[1]..=beside[3] {
                    for px in beside[0]..=beside[2] {
                        cells.set([px, py], GATEWAY, true);
                    }
                }
            }
            cells.marks.push((own, beside));
        }
        // 0x439aa0: the anchor holds its own cell.
        class::CRUSADEANCHOR => cells.set([x, y], OCCUPIED[1], true),
        class::FAKEBUILDING | class::FAKERESOURCE | class::FAKEWALL | class::FAKEGATE => {}
        // 0x434d10 and the harbor's 0x45b4a0: water cells in the water
        // layer, the others in the land layer.
        class::GROUND_OBJECT | class::HARBOR => {
            let ground = o.class == class::GROUND_OBJECT;
            each(cells, &mut |c, p, free| {
                if free || !c.inside(p) {
                    return;
                }
                let water = c.at(p) & (3 << 17) == WATER;
                c.set(p, STATIC, true);
                if water {
                    c.set(p, OCCUPIED[2], true);
                } else if ground || c.at(p) & (3 << 17) == pathing::LAND {
                    c.set(p, OCCUPIED[1], true);
                }
            });
        }
        // 0x41ddc0: buildings, ruins and mines, by their picture's mask;
        // walls also mark the cells a ninja climbs (0x4fad01).
        _ if o.building()
            || matches!(o.class, class::RES_GOLD | class::RES_STONE | class::RUIN) =>
        {
            let wall = matches!(o.class, class::WALL_STONE | class::WALL_WOOD);
            each(cells, &mut |c, p, free| {
                if !free {
                    c.set(p, STATIC | OCCUPIED[1], true);
                    c.set(p, WALL, wall);
                }
            });
        }
        _ => {}
    }
}
/// The cells beside a gate's door (0x457ac0), by its facing; a stone gate
/// facing up reaches two rows out.
pub(super) fn gate_beside(o: &MapObject) -> [i32; 4] {
    let [x, y] = [o.x as i32, o.y as i32];
    match o.byte(56) & 3 {
        0 => [x + 2, y + 1, x + 4, y + 2],
        1 if o.class == class::GATE_STONE => [x + 2, y - 2, x + 4, y - 1],
        1 => [x + 2, y - 1, x + 4, y - 1],
        2 => [x - 2, y + 1, x - 1, y + 3],
        _ => [x + 1, y + 1, x + 2, y + 3],
    }
}
/// The cells of the map as they are now (0x5ccaf4), for a site test: what
/// `build` makes, from the fixed cells of the last refresh, made again
/// where something changed since, and the movers' marks.
pub(super) fn cells_now(
    sim: &Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
) -> Cells {
    let f = &sim.gameplay.pathing.fixed;
    let cells = if f.cells.side != g.side || f.dirty.len() != g.side * g.side {
        build(sim, m, g, lib)
    } else {
        let (movers, prints) = movers_and_prints(sim, m);
        if prints == f.prints && f.grid == g.cells {
            with_movers(f, sim, m, lib, &movers).0
        } else {
            let mut now = f.clone();
            update_fixed(&mut now, m, g, lib, prints);
            with_movers(&now, sim, m, lib, &movers).0
        }
    };
    #[cfg(test)]
    assert!(
        cells == build(sim, m, g, lib),
        "cells differ from a full build at tick {}",
        sim.ticks
    );
    cells
}
/// The fixed cells with the movers' marks, as `build` makes them, and the
/// cells marked, in order without repeats.
fn with_movers(
    fixed: &Fixed,
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    movers: &[usize],
) -> (Cells, Vec<usize>) {
    let t = now(sim);
    let mut cells = fixed.cells.clone();
    let mut marked = Vec::with_capacity(movers.len());
    for &k in movers {
        let o = &m.objects[k];
        let p = [o.x as i32, o.y as i32];
        let inside = cells.inside(p);
        let cleared = inside
            && fixed
                .doors
                .get(&cells.index(p))
                .is_some_and(|&gate| gate > k);
        mark_mover(&mut cells, sim, lib, k, o, t, cleared);
        if inside {
            marked.push(cells.index(p));
        }
    }
    marked.sort_unstable();
    marked.dedup();
    (cells, marked)
}
/// The objects that stand still as the player sees them or keeps them in
/// memory (`Simulation::appearance`), with their indices: what this
/// version's placement grid may show out of view.
pub(super) fn known_standing<'a>(
    sim: &'a Simulation,
    m: &'a MissionWorld,
) -> Vec<(usize, &'a MapObject)> {
    m.objects
        .iter()
        .enumerate()
        .filter(|(_, o)| !mobile(o))
        .filter_map(|(k, o)| {
            let seen = sim.appearance(k, o)?;
            (!std::ptr::eq(seen, o) || on_map(sim, k, o)).then_some((k, seen))
        })
        .collect()
}
/// The cells as the player knows them out of view: the terrain and the
/// objects of `known_standing`, no movers.
pub(super) fn cells_known(
    g: &MapGrid,
    lib: &ObjectLibrary,
    known: &[(usize, &MapObject)],
) -> Cells {
    let mut cells = Cells::new(g, lib.trunks);
    for (_, o) in known {
        occupy(&mut cells, lib, o, &mut Vec::new());
    }
    cells
}
/// Rebuild the cells and the regions that changed, at the start of a tick:
/// the fixed cells again only where what stands still or the terrain
/// changed, then the movers' marks; the regions are looked at where a mover
/// is or was and where the fixed cells were made again, or everywhere after
/// a first build. The result equals `build` and a full `Regions::refresh`,
/// which the tests check every tick.
pub(super) fn refresh(sim: &mut Simulation, m: &MissionWorld, g: &MapGrid, lib: &ObjectLibrary) {
    let (movers, prints) = movers_and_prints(sim, m);
    let fixed = &mut sim.gameplay.pathing.fixed;
    let stale = fixed.cells.side != g.side || fixed.dirty.len() != g.side * g.side;
    let remade = if stale {
        sim.gameplay.pathing.fixed = build_fixed(m, g, lib, prints);
        Vec::new()
    } else {
        update_fixed(fixed, m, g, lib, prints)
    };
    let (cells, marked) = with_movers(&sim.gameplay.pathing.fixed, sim, m, lib, &movers);
    #[cfg(test)]
    let before = sim.gameplay.pathing.regions.clone();
    let p = &mut sim.gameplay.pathing;
    if stale {
        p.regions.refresh(&cells);
    } else {
        let mut candidates = std::mem::take(&mut p.fixed.marked);
        candidates.extend_from_slice(&marked);
        candidates.extend_from_slice(&remade);
        candidates.sort_unstable();
        candidates.dedup();
        p.regions.refresh_at(&cells, candidates);
    }
    p.fixed.marked = marked;
    #[cfg(test)]
    {
        let mut full = before;
        full.refresh(&cells);
        assert!(
            full == sim.gameplay.pathing.regions,
            "regions differ from a full refresh at tick {}",
            sim.ticks
        );
        assert!(
            build(sim, m, g, lib) == cells,
            "cells differ from a full build at tick {}",
            sim.ticks
        );
    }
    sim.gameplay.pathing.cells = cells;
}
fn ensure(sim: &mut Simulation, m: &MissionWorld, g: &MapGrid, lib: &ObjectLibrary) {
    if sim.gameplay.pathing.cells.side != g.side {
        refresh(sim, m, g, lib);
    }
}

/// 0x502550 for a unit of one cell.
pub(super) fn arrived(p: [u16; 2], rect: [i32; 4], range: u32) -> bool {
    reaches(p, [1, 1], rect, range)
}
/// 0x502550: the centre of an object of `size` cells at `p` lies in the
/// rect, or within `range` of it, measured in half cells.
pub(super) fn reaches(p: [u16; 2], size: [u16; 2], rect: [i32; 4], range: u32) -> bool {
    let [w, h] = size.map(|n| n as i64);
    let c = [2 * p[0] as i64 + w, 2 * p[1] as i64 + h];
    let r = [
        2 * rect[0] as i64,
        2 * rect[1] as i64,
        2 * rect[2] as i64 + w,
        2 * rect[3] as i64 + h,
    ];
    let q = [c[0].clamp(r[0], r[2]), c[1].clamp(r[1], r[3])];
    if q == c {
        return true;
    }
    if range == 0 {
        return false;
    }
    let d = [q[0] - c[0], q[1] - c[1]];
    d[0] * d[0] + d[1] * d[1] <= 4 * (range as i64) * (range as i64)
}
/// The rect an object is approached by (vt+0x134): the 3×3 cells around a
/// unit (0x501a20), the holder's for a carried unit, a building's
/// footprint grown by one (0x41fd70).
pub(super) fn target_rect(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    j: usize,
) -> [i32; 4] {
    let mut j = j;
    for _ in 0..8 {
        match sim.objects.get(j).and_then(|s| s.holder) {
            Some(h) => j = h,
            None => break,
        }
    }
    let o = &m.objects[j];
    let [x, y] = [o.x as i32, o.y as i32];
    let [w, h] = footprint(lib, o).map(|n| n as i32);
    [x - 1, y - 1, x + w, y + h]
}
/// The object's own cells.
pub(super) fn footprint_rect(lib: &ObjectLibrary, o: &MapObject) -> [i32; 4] {
    let [x, y] = [o.x as i32, o.y as i32];
    let [w, h] = footprint(lib, o).map(|n| n as i32);
    [x, y, x + w - 1, y + h - 1]
}
pub(super) fn in_range(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    j: usize,
    range: u32,
) -> bool {
    combat::within(m, lib, i, target_rect(sim, m, lib, j), range)
}

/// Milliseconds per pixel of the picture the unit walks with:
/// (32000 / move_len) >> 5, a cell taking 32 of them (0x49891b).
fn step_ms(sim: &Simulation, lib: &ObjectLibrary, o: &MapObject, i: usize) -> u32 {
    let a = &sim.gameplay.actors[i];
    let section = lib
        .carrying_image(o, a.cargo, a.cargo_source)
        .or_else(|| lib.runtime_image(o, Some(OrderState::Moving)))
        .unwrap_or_default();
    picture_step_ms(lib, &section)
}
fn picture_step_ms(lib: &ObjectLibrary, section: &str) -> u32 {
    let move_len = lib
        .images
        .get(section, "move_len")
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(32);
    ((32000 / move_len) >> 5).max(1)
}
/// The ninja's jump picture (0x4ef278, 0xc97cf8).
pub const JUMP_PICTURE: &str = "a_ninja_jump";
/// The direction of a step (0x4fd450): S 0, SE 1, E 2, NE 3, N 4, …
fn direction(from: [u16; 2], to: [u16; 2]) -> u8 {
    let (dx, dy) = (to[0] as f64 - from[0] as f64, to[1] as f64 - from[1] as f64);
    (2 - (dy.atan2(dx) / std::f64::consts::FRAC_PI_4).round() as i32).rem_euclid(8) as u8
}

/// The path request of the unit's class (vt+0x2b4): land 0x4df540, air
/// 0x4e7980 and water 0x4e8520 (both aiming at the rect itself).
fn request(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    rect: [i32; 4],
    range: u32,
    repath: bool,
) -> Option<VecDeque<[u16; 2]>> {
    let o = &m.objects[i];
    let q = query(lib, o, [o.x as i32, o.y as i32], rect, range, repath);
    let p = &mut sim.gameplay.pathing;
    let path = pathing::search(&p.cells, &mut p.regions, &q);
    // 0x4df5f0: a land unit of a computer player (not of the story) may
    // break through the walls (0x4171e0).
    if q.cost == Cost::Land && o.flags & FLAG_DYING == 0 && super::ai_managed(sim, m, i) {
        return super::ai::breach::path(sim, m, lib, i, &q, path).map(VecDeque::from);
    }
    path.map(VecDeque::from)
}
fn search_from(
    sim: &mut Simulation,
    lib: &ObjectLibrary,
    o: &MapObject,
    start: [i32; 2],
    rect: [i32; 4],
    range: u32,
    repath: bool,
) -> Option<VecDeque<[u16; 2]>> {
    let q = query(lib, o, start, rect, range, repath);
    let p = &mut sim.gameplay.pathing;
    pathing::search(&p.cells, &mut p.regions, &q).map(VecDeque::from)
}
fn query(
    lib: &ObjectLibrary,
    o: &MapObject,
    start: [i32; 2],
    rect: [i32; 4],
    range: u32,
    repath: bool,
) -> Query {
    // vt+0x2b4: the ninja's request (0x4ebf60) climbs (0x4a99a0).
    let cost = match lib.layer(o) {
        0 => Cost::Air,
        2 => Cost::Water,
        _ if o.class == class::NINJA => Cost::Ninja,
        _ => Cost::Land,
    };
    Query {
        start,
        rect,
        range: if cost == Cost::Land { range as i32 } else { 0 },
        repath,
        cost,
    }
}
/// A cell the unit could walk toward: a search finds a first step.
pub(super) fn reachable(
    sim: &mut Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    p: [u16; 2],
) -> bool {
    ensure(sim, m, g, lib);
    let rect = [p[0] as i32, p[1] as i32, p[0] as i32, p[1] as i32];
    request(sim, m, lib, i, rect, 0, false).is_some_and(|p| !p.is_empty())
}
/// 0x4e9437: a land cell lies in the rect of `i` (vt+0x134) grown by 5.
pub(super) fn land_near(
    sim: &mut Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
) -> bool {
    ensure(sim, m, g, lib);
    let r = target_rect(sim, m, lib, i);
    let cells = &sim.gameplay.pathing.cells;
    (r[0] - 5..r[2] + 5).any(|x| (r[1] - 5..r[3] + 5).any(|y| cells.land([x, y])))
}
/// vt+0x90 (0x4e53a0) for a unit of one cell: it may step into the cell now
/// (0x500bb0).
pub(super) fn can_step(
    sim: &mut Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    p: [u16; 2],
) -> bool {
    ensure(sim, m, g, lib);
    let layer = lib.layer(&m.objects[i]);
    sim.gameplay
        .pathing
        .cells
        .free(layer, [p[0] as i32, p[1] as i32])
}
fn occupant(
    sim: &Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    layer: usize,
    p: [u16; 2],
    except: usize,
) -> Option<usize> {
    m.objects.iter().enumerate().position(|(k, o)| {
        k != except
            && [o.x, o.y] == p
            && mobile(o)
            && alive(o)
            && sim.objects.get(k).is_some_and(|s| s.holder.is_none())
            && lib.layer(o) == layer
    })
}
fn clear_transit(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, i: usize) {
    if std::mem::take(&mut sim.gameplay.actors[i].transit) {
        let o = &m.objects[i];
        let layer = lib.layer(o);
        sim.gameplay
            .pathing
            .cells
            .set([o.x as i32, o.y as i32], TRANSIT[layer], false);
    }
}

enum Begin {
    Step(u64),
    Arrived,
    Failed,
    Wait,
}
const WAIT_MS: u64 = 250;

/// 0x4ebc00, the ninja's step start (0x4ebf30): the wall and cliff cells at
/// the head of the path are left out and the step jumps to the ground
/// beyond. Three of them, or a path that ends on them, are searched again
/// first (flag 0). Whether the step is a jump.
fn leap(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    mv: &Movement,
) -> bool {
    let climb = |sim: &Simulation, p: [u16; 2]| {
        let cells = &sim.gameplay.pathing.cells;
        cells.inside(at(p)) && cells.at(at(p)) & (WALL | CLIFF) != 0
    };
    let path = &sim.gameplay.actors[i].path;
    let lead = path.iter().take(3).take_while(|&&p| climb(sim, p)).count();
    if lead >= 3 || (lead > 0 && lead == path.len()) {
        sim.gameplay.actors[i].path.clear();
        let o = &m.objects[i];
        if arrived([o.x, o.y], mv.rect, mv.range) {
            return false;
        }
        match request(sim, m, lib, i, mv.rect, mv.range, false) {
            Some(p) => sim.gameplay.actors[i].path = p,
            None => return false,
        }
    }
    let mut jumped = false;
    while let Some(&p) = sim.gameplay.actors[i].path.front()
        && climb(sim, p)
    {
        sim.gameplay.actors[i].path.pop_front();
        jumped = true;
    }
    jumped && !sim.gameplay.actors[i].path.is_empty()
}

/// 0x4debb0: start the step to the next node, wait behind a unit that is
/// entering it, or search again around the moving units too.
fn begin(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    t: u64,
) -> Begin {
    let here = [m.objects[i].x, m.objects[i].y];
    let layer = lib.layer(&m.objects[i]);
    let a = &mut sim.gameplay.actors[i];
    while a.path.front() == Some(&here) {
        a.path.pop_front();
    }
    let mv = a.movement.clone().unwrap_or_default();
    let jump = m.objects[i].class == class::NINJA && leap(sim, m, lib, i, &mv);
    let a = &mut sim.gameplay.actors[i];
    let Some(&next) = a.path.front() else {
        return if arrived(here, mv.rect, mv.range) {
            Begin::Arrived
        } else {
            Begin::Failed
        };
    };
    a.waiting_for = None;
    if let Some(mv) = a.movement.as_mut() {
        mv.waits = 0;
    }
    let cells = &sim.gameplay.pathing.cells;
    if cells.free(layer, at(next)) {
        return step(sim, m, lib, i, next, t, jump);
    }
    if cells.inside(at(next)) && cells.at(at(next)) & TRANSIT[layer] != 0 {
        if let Some(k) = occupant(sim, m, lib, layer, next, i)
            && sim.gameplay.actors[k].waiting_for != Some(here)
        {
            let a = &mut sim.gameplay.actors[i];
            a.waiting_for = Some(next);
            if let Some(mv) = a.movement.as_mut() {
                mv.state = 2;
            }
            return Begin::Wait;
        }
    }
    // Head on, or a unit standing there: search again avoiding units on the
    // move too (flag 1).
    sim.gameplay.actors[i].path.clear();
    let Some(path) = request(sim, m, lib, i, mv.rect, mv.range, true) else {
        return Begin::Failed;
    };
    let Some(&next) = path.front() else {
        return Begin::Failed;
    };
    sim.gameplay.actors[i].path = path;
    if sim.gameplay.pathing.cells.free(layer, at(next)) {
        step(sim, m, lib, i, next, t, false)
    } else {
        if let Some(mv) = sim.gameplay.actors[i].movement.as_mut() {
            mv.state = 2;
        }
        Begin::Wait
    }
}
/// The unit takes the cell it enters at once (vt+0x60 with flag 1) and
/// marks it in transit (vt+0x94); its picture follows in 32 pixel steps,
/// or for a ninja's jump in as many as the jump picture has rows, each
/// covering what is left divided by the steps left (0x4ebaaa). The Move op
/// waits 32 steps of the picture shown (0x4def20).
fn step(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    i: usize,
    next: [u16; 2],
    t: u64,
    jump: bool,
) -> Begin {
    let here = [m.objects[i].x, m.objects[i].y];
    let layer = lib.layer(&m.objects[i]);
    let cells = &mut sim.gameplay.pathing.cells;
    cells.set(at(here), OCCUPIED[layer] | TRANSIT[layer], false);
    cells.set(at(next), OCCUPIED[layer] | TRANSIT[layer], true);
    let rows = jump
        .then(|| {
            lib.images
                .get(JUMP_PICTURE, "rows")?
                .trim()
                .parse::<u8>()
                .ok()
        })
        .flatten()
        .filter(|&n| n > 0);
    let step_ms = match rows {
        Some(_) => picture_step_ms(lib, JUMP_PICTURE),
        None => step_ms(sim, lib, &m.objects[i], i),
    };
    position(&mut m.objects[i], next);
    // 0x500da0: into another sector, the head of its list at once, as it
    // takes the cell.
    let into = [next[0] / 8, next[1] / 8];
    if sim.gameplay.actors[i].sector.is_some_and(|s| s != into) {
        sim.gameplay.sector_clock += 1;
        let clock = sim.gameplay.sector_clock;
        let a = &mut sim.gameplay.actors[i];
        a.sector = Some(into);
        a.sector_stamp = clock;
        a.sector_ms = t;
    }
    carry_contents(sim, m, i);
    let a = &mut sim.gameplay.actors[i];
    a.entered = true;
    a.direction = direction(here, next);
    a.transit = true;
    a.walk_cells = a.walk_cells.saturating_add(1);
    a.walk = Some(Walk {
        from: here,
        start_ms: t,
        due_ms: t + 32 * step_ms as u64,
        step_ms,
        jump: rows.unwrap_or(0),
    });
    if let Some(mv) = a.movement.as_mut() {
        mv.state = 1;
    }
    Begin::Step(32 * step_ms as u64)
}

enum Op {
    Again(u64),
    End(bool),
}
/// One run of the Move op at time `t`.
fn op(sim: &mut Simulation, m: &mut MissionWorld, lib: &ObjectLibrary, i: usize, t: u64) -> Op {
    let here = [m.objects[i].x, m.objects[i].y];
    let layer = lib.layer(&m.objects[i]);
    let Some(mv) = sim.gameplay.actors[i].movement.clone() else {
        return Op::End(false);
    };
    let begun = |b: Begin| match b {
        Begin::Step(ms) => Op::Again(ms),
        Begin::Arrived => Op::End(true),
        Begin::Failed => Op::End(false),
        Begin::Wait => Op::Again(WAIT_MS),
    };
    match mv.state {
        0 => {
            // 0x4df318: a group shares one path, each unit shifted by its
            // place in the group.
            if sim.gameplay.actors[i].path.is_empty()
                && let Some((path, point)) =
                    group_path(sim, m, lib, i, mv.rect, mv.range, mv.target)
            {
                let a = &mut sim.gameplay.actors[i];
                a.path = path;
                if let Some(rect) = point {
                    a.goal = Some([rect[0] as u16, rect[1] as u16]);
                    if let Some(x) = a.movement.as_mut() {
                        x.rect = rect;
                    }
                }
            }
            let mv = sim.gameplay.actors[i].movement.clone().unwrap_or_default();
            if sim.gameplay.actors[i].path.is_empty() {
                match request(sim, m, lib, i, mv.rect, mv.range, false) {
                    Some(p) => sim.gameplay.actors[i].path = p,
                    None => return Op::End(arrived(here, mv.rect, mv.range)),
                }
            }
            begun(begin(sim, m, lib, i, t))
        }
        1 => {
            let mut repathed = false;
            if let Some(j) = mv.target {
                if !m.objects.get(j).is_some_and(alive) {
                    return Op::End(false);
                }
                // A followed object that moved: the path starts again.
                let rect = target_rect(sim, m, lib, j);
                if rect != mv.rect {
                    clear_transit(sim, m, lib, i);
                    let a = &mut sim.gameplay.actors[i];
                    a.path.clear();
                    if let Some(x) = a.movement.as_mut() {
                        x.rect = rect;
                    }
                    if arrived(here, rect, mv.range) {
                        return Op::End(true);
                    }
                    match request(sim, m, lib, i, rect, mv.range, false) {
                        Some(p) => sim.gameplay.actors[i].path = p,
                        None => return Op::End(false),
                    }
                    repathed = true;
                } else if arrived(here, rect, mv.range) {
                    return Op::End(true);
                }
            } else if arrived(here, mv.rect, mv.range) {
                return Op::End(true);
            }
            if !repathed {
                // 0x4deb40: drop the node reached; at the end of a partial
                // path, search again from here.
                let a = &mut sim.gameplay.actors[i];
                a.path.pop_front();
                if a.path.is_empty() {
                    match request(sim, m, lib, i, mv.rect, mv.range, false) {
                        Some(p) if !p.is_empty() => sim.gameplay.actors[i].path = p,
                        _ => return Op::End(false),
                    }
                }
            }
            begun(begin(sim, m, lib, i, t))
        }
        _ => {
            // Blocked: wait up to ten times 250 ms behind a unit entering the
            // cell, then search again avoiding the moving units.
            let waits = mv.waits + 1;
            if let Some(x) = sim.gameplay.actors[i].movement.as_mut() {
                x.waits = waits;
            }
            if let Some(&next) = sim.gameplay.actors[i].path.front() {
                let cells = &sim.gameplay.pathing.cells;
                if cells.free(layer, at(next)) {
                    return begun(begin(sim, m, lib, i, t));
                }
                // 0x4df043: a gate's cells keep the unit waiting too.
                if waits < 10
                    && cells.inside(at(next))
                    && cells.at(at(next)) & (TRANSIT[layer] | GATEWAY) != 0
                {
                    return Op::Again(WAIT_MS);
                }
            }
            sim.gameplay.actors[i].path.clear();
            match request(sim, m, lib, i, mv.rect, mv.range, true) {
                Some(p) => sim.gameplay.actors[i].path = p,
                None => return Op::End(false),
            }
            match begin(sim, m, lib, i, t) {
                Begin::Step(ms) => Op::Again(ms),
                Begin::Arrived => Op::End(true),
                Begin::Failed | Begin::Wait => Op::End(false),
            }
        }
    }
}

/// Walk toward a rect (the Move op): `Some(true)` once within `range` of
/// it, `Some(false)` when the walk failed, `None` while it goes on. A
/// failed walk is retried after a second.
pub(super) fn walk(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    i: usize,
    rect: [i32; 4],
    range: u32,
    target: Option<usize>,
) -> Option<bool> {
    if !mobile(&m.objects[i]) {
        return Some(false);
    }
    ensure(sim, m, g, lib);
    let t = now(sim);
    let side = g.side as i32 - 1;
    let rect = [
        rect[0].max(0),
        rect[1].max(0),
        rect[2].min(side),
        rect[3].min(side),
    ];
    let same = sim.gameplay.actors[i].movement.as_ref().is_some_and(|mv| {
        mv.range == range
            && match target {
                Some(j) => mv.target == Some(j),
                None => mv.target.is_none() && mv.asked == rect,
            }
    });
    if !same {
        // A new Move starts where the last step ended.
        clear_transit(sim, m, lib, i);
        let a = &mut sim.gameplay.actors[i];
        let since = match &a.walk {
            Some(w) if w.due_ms + TICK_MS > t => w.due_ms,
            _ => {
                a.walk_cells = 0;
                t
            }
        };
        a.path.clear();
        a.goal = Some([
            ((rect[0] + rect[2]) / 2) as u16,
            ((rect[1] + rect[3]) / 2) as u16,
        ]);
        a.movement = Some(Movement {
            asked: rect,
            rect,
            range,
            target,
            due_ms: since,
            ..Movement::default()
        });
    }
    let a = &mut sim.gameplay.actors[i];
    let mv = a.movement.as_mut()?;
    mv.ran = true;
    if mv.failed {
        if a.path_cooldown > 0 {
            return Some(false);
        }
        *mv = Movement {
            asked: rect,
            rect,
            range,
            target,
            due_ms: t,
            ..Movement::default()
        };
    }
    // Never catch up on time spent frozen.
    mv.due_ms = mv.due_ms.max(t.saturating_sub(TICK_MS - 1));
    loop {
        let due = sim.gameplay.actors[i].movement.as_ref()?.due_ms;
        if due > t {
            sim.objects[i].order = OrderState::Moving;
            return None;
        }
        match op(sim, m, lib, i, due) {
            Op::Again(ms) => {
                if let Some(mv) = sim.gameplay.actors[i].movement.as_mut() {
                    mv.due_ms = due + ms.max(1);
                }
            }
            Op::End(done) => {
                clear_transit(sim, m, lib, i);
                let a = &mut sim.gameplay.actors[i];
                a.path.clear();
                a.waiting_for = None;
                if done {
                    a.movement = None;
                } else if let Some(mv) = a.movement.as_mut() {
                    mv.failed = true;
                    a.path_cooldown = 20;
                }
                return Some(done);
            }
        }
    }
}
/// After the unit's program ran: a Move op it went past without walking (in
/// reach at once, or on to its next instruction) gets the op's cleanup
/// (vt+0x170 case 9, 0x4e334c): the path is freed and the cell stays
/// occupied but no longer in transit, so nobody waits behind a unit that
/// has stopped to work.
pub(super) fn settle(sim: &mut Simulation, m: &MissionWorld, lib: &ObjectLibrary, i: usize) {
    match sim.gameplay.actors[i].movement.as_mut() {
        Some(mv) if mv.ran => mv.ran = false,
        Some(_) => {
            clear_transit(sim, m, lib, i);
            let a = &mut sim.gameplay.actors[i];
            a.path.clear();
            a.goal = None;
            a.waiting_for = None;
            a.movement = None;
        }
        None => {}
    }
}
/// The last walk failed and waits for its retry.
pub(super) fn failed(sim: &Simulation, i: usize) -> bool {
    sim.gameplay.actors[i]
        .movement
        .as_ref()
        .is_some_and(|mv| mv.failed)
}
/// A step is still under way: the unit's program waits for its end.
pub(super) fn stepping(sim: &Simulation, i: usize) -> bool {
    sim.gameplay.actors[i]
        .walk
        .as_ref()
        .is_some_and(|w| w.due_ms > now(sim))
}

/// The picture's offset from the unit's cell, in pixels with y up: it
/// slides a pixel per `step_ms` from the cell it left (0x4e138b).
pub fn motion_offset(sim: &Simulation, m: &MissionWorld, i: usize) -> [f32; 2] {
    let (Some(a), Some(o)) = (sim.gameplay.actors.get(i), m.objects.get(i)) else {
        return [0., 0.];
    };
    let Some(w) = &a.walk else {
        return [0., 0.];
    };
    let ms = sim.elapsed.as_secs_f64() * 1000.;
    if ms >= w.due_ms as f64 {
        return [0., 0.];
    }
    let px = ((ms - w.start_ms as f64).max(0.) / w.step_ms.max(1) as f64)
        .floor()
        .min(32.);
    if w.jump > 0 {
        // 0x4ebaaa: each step covers what is left over the steps left.
        let mut left = [
            (w.from[0] as i32 - o.x as i32) * 32,
            (w.from[1] as i32 - o.y as i32) * 32,
        ];
        for k in 0..(px as u32).min(w.jump as u32) {
            let n = (w.jump as u32 - k) as i32;
            left = left.map(|d| d - d / n);
        }
        return [left[0] as f32, -left[1] as f32];
    }
    let d = [o.x as f64 - w.from[0] as f64, o.y as f64 - w.from[1] as f64];
    [(-d[0] * (32. - px)) as f32, (d[1] * (32. - px)) as f32]
}
/// The time the unit's Move op began waiting on a blocked cell (state 2),
/// from the end of its last step.
pub fn blocked_since(sim: &Simulation, i: usize) -> Option<u64> {
    let a = sim.gameplay.actors.get(i)?;
    a.movement.as_ref().filter(|mv| mv.state == 2)?;
    Some(a.walk.as_ref().map_or(0, |w| w.due_ms))
}
/// The frame of a ninja's jump under way: one more at each of its steps
/// (vt+0x6c), wrapping after the last row.
/// The cells a flyer goes through from `from` to `to` (0x4e7980: 0x4a9060
/// with the air costs, aiming at the cell itself), the start left out;
/// empty when there is no way. The scenery's birds ask it for their flights.
pub fn air_path(sim: &Simulation, from: [i32; 2], to: [i32; 2]) -> Vec<[i32; 2]> {
    let side = sim.gameplay.pathing.cells.side as i32;
    if from.iter().chain(&to).any(|&v| !(0..side).contains(&v)) {
        return Vec::new();
    }
    let q = Query {
        start: from,
        rect: [to[0], to[1], to[0], to[1]],
        range: 0,
        repath: false,
        cost: Cost::Air,
    };
    pathing::search(
        &sim.gameplay.pathing.cells,
        &mut pathing::Regions::default(),
        &q,
    )
    .unwrap_or_default()
    .into_iter()
    .map(|p| [p[0] as i32, p[1] as i32])
    .collect()
}
pub fn jump_frame(sim: &Simulation, i: usize) -> Option<u32> {
    let w = sim.gameplay.actors.get(i)?.walk.as_ref()?;
    let ms = sim.elapsed.as_secs_f64() * 1000.;
    if w.jump == 0 || ms >= w.due_ms as f64 {
        return None;
    }
    let k = ((ms - w.start_ms as f64).max(0.) / w.step_ms.max(1) as f64).floor() as u32;
    Some(k.min(w.jump as u32) % w.jump as u32)
}
/// Cells finished and pixels of the current one since the unit set off:
/// the walking frame advances every few pixels, counted again at each
/// cell (0x4e13b2, 0x4dec31).
pub fn walk_progress(sim: &Simulation, i: usize) -> (u32, u32) {
    let Some(a) = sim.gameplay.actors.get(i) else {
        return (0, 0);
    };
    let Some(w) = &a.walk else {
        return (a.walk_cells, 0);
    };
    let ms = sim.elapsed.as_secs_f64() * 1000.;
    if ms >= w.due_ms as f64 {
        return (a.walk_cells, 0);
    }
    let px = ((ms - w.start_ms as f64).max(0.) / w.step_ms.max(1) as f64)
        .floor()
        .min(32.) as u32;
    (a.walk_cells.saturating_sub(1), px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_range_is_measured_from_the_unit_centre_in_half_cells() {
        // An archer (RANGE 6) against a unit's 3×3 rect.
        let rect = [-1, -1, 1, 1];
        let at = |x: i32, y: i32| [(x + 100) as u16, (y + 100) as u16];
        let rect = rect.map(|v| v + 100);
        for (x, y) in [(7, 0), (-7, 0), (0, 7), (0, -7), (5, 5), (-5, -5), (-5, -6)] {
            assert!(arrived(at(x, y), rect, 6), "{x},{y}");
        }
        for (x, y) in [(8, 0), (6, 6), (-6, -6), (5, -6)] {
            assert!(!arrived(at(x, y), rect, 6), "{x},{y}");
        }
        // Range 0: only inside the rect.
        assert!(arrived(at(1, 1), rect, 0));
        assert!(!arrived(at(2, 0), rect, 0));
    }

    #[test]
    fn steps_take_the_time_of_the_walking_picture() {
        assert_eq!((32000 / 50) >> 5, 20);
        assert_eq!(direction([5, 5], [5, 6]), 0);
        assert_eq!(direction([5, 5], [6, 5]), 2);
        assert_eq!(direction([5, 5], [6, 4]), 3);
        assert_eq!(direction([5, 5], [4, 4]), 5);
    }
}

/// 0x500a50 for a holder (flag 0x20000) that takes a cell: what it holds
/// takes the same cell (vt+0x60, 0x500ac2), and so on down, each forgetting
/// the cell it came in by (+0x12/+0x14, 0x500acc).
pub(super) fn carry_contents(sim: &mut Simulation, m: &mut MissionWorld, i: usize) {
    let p = [m.objects[i].x, m.objects[i].y];
    for k in 0..sim.objects[i].contents.len() {
        let j = sim.objects[i].contents[k];
        position(&mut m.objects[j], p);
        sim.objects[j].entry = None;
        carry_contents(sim, m, j);
    }
}
