//! Native pathfinding of Tzar.exe: the cell bits of 0x5ccaf4, the cell A*
//! 0x4a9060 with its cost functions, and the graph of regions in 8×8
//! sectors that guides it (0x4aa970, 0x4aab20, 0x4ab090).

use super::*;
use std::collections::HashMap;

// The first word of a cell (0x5ccaf4): 16 tile bits, then these.
pub(crate) const STATIC: u32 = 1 << 16;
pub(crate) const KIND: u32 = 3 << 17;
pub(crate) const LAND: u32 = 1 << 17;
pub(crate) const WATER: u32 = 2 << 17;
pub(crate) const WALL: u32 = 1 << 20;
pub(crate) const BRIDGE: u32 = 1 << 21;
pub(crate) const BLOCKED: u32 = 1 << 22;
/// Occupancy and transit bits of the air, land and water layers.
pub(crate) const OCCUPIED: [u32; 3] = [1 << 23, 1 << 24, 1 << 25];
pub(crate) const TRANSIT: [u32; 3] = [1 << 26, 1 << 27, 1 << 28];
pub(crate) const FELLED: u32 = 1 << 29;
/// A gate's door, and the cells beside it while it is open (0x4562cb,
/// 0x4577b4): a unit blocked there keeps waiting (0x4df043).
pub(crate) const GATEWAY: u32 = 1 << 30;
pub(crate) const FIELD: u32 = 1 << 31;
pub(crate) const CLIFF: u32 = 0x400;
const NO_REGION: u8 = 0xff;
/// Nodes of the shared pool (0xc8afd4 = 0x1000); a search stops past 4088.
const POOL: usize = 0x1000 - 8;

/// Kind 3 (0x60000): a tile whose second surface is water and first is
/// ground, as every shore of the original maps.
pub(crate) const SHORE: u32 = 3 << 17;
/// The surfaces below this are water (`[terrain] water`, 1 in every biome).
const WATER_SURFACES: u16 = 1;

/// The terrain kind of a tile (0x476da0): nothing for a cliff; land when
/// the second surface (bits 7–9) is ground, water when both are water,
/// kind 3 when only the first (bits 4–6) is ground.
pub(crate) fn kind(tile: u16) -> u32 {
    let (first, second) = ((tile >> 4) & 7, (tile >> 7) & 7);
    if tile as u32 & CLIFF != 0 {
        0
    } else if second >= WATER_SURFACES {
        LAND
    } else if first >= WATER_SURFACES {
        SHORE
    } else {
        WATER
    }
}

#[derive(Clone, Debug, Default)]
#[cfg_attr(test, derive(PartialEq))]
pub(crate) struct Cells {
    pub side: usize,
    pub word: Vec<u32>,
    /// Tree images up to this index are trunks (0x5cfc48).
    pub trunks: u32,
    /// The cells each standing object marks and the rect its changes pass
    /// to the regions (0x4aab20): its own, beside the door for a gate.
    pub marks: Vec<([i32; 4], [i32; 4])>,
}
impl Cells {
    pub fn new(g: &MapGrid, trunks: u8) -> Self {
        Self {
            side: g.side,
            word: g.cells.iter().map(|&t| t as u32 | kind(t)).collect(),
            trunks: trunks as u32,
            marks: Vec::new(),
        }
    }
    pub fn inside(&self, p: [i32; 2]) -> bool {
        p[0] >= 0 && p[1] >= 0 && (p[0] as usize) < self.side && (p[1] as usize) < self.side
    }
    pub(crate) fn index(&self, p: [i32; 2]) -> usize {
        p[1] as usize * self.side + p[0] as usize
    }
    pub fn at(&self, p: [i32; 2]) -> u32 {
        self.word[self.index(p)]
    }
    pub fn set(&mut self, p: [i32; 2], bits: u32, on: bool) {
        if self.inside(p) {
            let i = self.index(p);
            if on {
                self.word[i] |= bits;
            } else {
                self.word[i] &= !bits;
            }
        }
    }
    /// A standing tree: an image above the trunks, not felled.
    fn tree(&self, w: u32) -> bool {
        (w >> 11) & 31 > self.trunks && w & FELLED == 0
    }
    /// Ground a land unit may tread: a bridge, or land with no standing tree.
    fn ground(&self, w: u32) -> bool {
        w & BRIDGE != 0 || (w & KIND == LAND && !self.tree(w))
    }
    /// 0x5072a0: an object of movement type `kind` (0 air, 1 land, 2 water)
    /// may be put on the cell: of that kind of terrain (air any), no standing
    /// tree, no static object, its layer not occupied. Items lying there
    /// (bit 19) are tested by the caller.
    pub fn spawnable(&self, p: [i32; 2], kind: usize) -> bool {
        if !self.inside(p) {
            return false;
        }
        let w = self.at(p);
        if kind != 0 && (w & KIND) >> 17 != kind as u32 {
            return false;
        }
        !self.tree(w) && w & STATIC == 0 && OCCUPIED.get(kind).is_none_or(|&b| w & b == 0)
    }
    /// 0x4a9790: passable for land units, regardless of units; the regions
    /// are made of these cells.
    pub fn passable(&self, p: [i32; 2]) -> bool {
        let w = self.at(p);
        if w & BLOCKED != 0 {
            return false;
        }
        w & BRIDGE != 0
            || (w & KIND == LAND
                && !self.tree(w)
                && w & (OCCUPIED[1] | STATIC) != OCCUPIED[1] | STATIC)
    }
    /// A unit of `layer` (0 air, 1 land, 2 water) may enter the cell now:
    /// its layer's cost function with no query flags and no step before.
    pub fn enterable(&self, layer: usize, p: [i32; 2]) -> bool {
        let cost = match layer {
            0 => Cost::Air,
            2 => Cost::Water,
            _ => Cost::Land,
        };
        let q = Query {
            start: p,
            rect: [p[0], p[1], p[0], p[1]],
            range: 0,
            repath: false,
            cost,
            size: [1, 1],
        };
        self.inside(p) && self.cost(&q, None, None, p).is_some()
    }
    /// The cost functions (0x4a9820, 0x4a9ac0, 0x4a9b40, 0x4a99a0) of a step
    /// from `from`, itself reached from `before`, into `to`.
    fn cost(
        &self,
        q: &Query,
        before: Option<[i32; 2]>,
        from: Option<[i32; 2]>,
        to: [i32; 2],
    ) -> Option<u32> {
        if q.size != [1, 1] && matches!(q.cost, Cost::Land | Cost::Water | Cost::Air) {
            return self.large_cost(q, from, to);
        }
        let w = self.at(to);
        let layer = match q.cost {
            // 0x4a99a0: the ninja climbs a wall or a cliff for 800 from the
            // ground, crosses a second cell of it for nothing, never a third.
            Cost::Ninja if w & (WALL | CLIFF) != 0 => {
                if self.at(from?) & (WALL | CLIFF) == 0 {
                    return Some(800);
                }
                return match before {
                    Some(b) if self.at(b) & (WALL | CLIFF) != 0 => None,
                    _ => Some(0),
                };
            }
            Cost::Air => {
                if w & OCCUPIED[0] != 0 && (w & TRANSIT[0] == 0 || q.repath) {
                    return None;
                }
                0
            }
            Cost::Water => {
                if w & KIND != WATER || w & 0xf800 != 0 {
                    return None;
                }
                2
            }
            // 0x4170d0: the breach's search crosses a wall for 1, with no
            // other test.
            Cost::Breach if w & WALL != 0 => return Some(1),
            Cost::Land | Cost::Ninja | Cost::Breach => {
                if w & BLOCKED != 0 || !self.ground(w) {
                    return None;
                }
                1
            }
        };
        if layer != 0 && w & OCCUPIED[layer] != 0 && (w & TRANSIT[layer] == 0 || q.repath) {
            return None;
        }
        Some(match from {
            Some(f) if f[0] != to[0] && f[1] != to[1] => 22,
            _ => 16,
        })
    }
    /// The cost functions of a large unit (land 0x4a9be0, water 0x4a9d80, air
    /// 0x4a9ed0): every cell of its rect at `to`, but those it stands on at
    /// the start of the search, lies on the map and is of its terrain (land
    /// as 0x4a9820 sees it; water of kind 2, whatever stands on its tile),
    /// with nothing of its layer on it, and moving units only on a re-path,
    /// as against the cost functions of a single cell.
    fn large_cost(&self, q: &Query, from: Option<[i32; 2]>, to: [i32; 2]) -> Option<u32> {
        let [w, h] = q.size;
        let own = [
            q.start[0],
            q.start[1],
            q.start[0] + w - 1,
            q.start[1] + h - 1,
        ];
        let layer = match q.cost {
            Cost::Air => 0,
            Cost::Water => 2,
            _ => 1,
        };
        for x in to[0]..to[0] + w {
            for y in to[1]..to[1] + h {
                if within(own, [x, y]) {
                    continue;
                }
                if !self.inside([x, y]) {
                    return None;
                }
                let c = self.at([x, y]);
                let terrain = match layer {
                    0 => true,
                    2 => c & KIND == WATER,
                    _ => c & BLOCKED == 0 && self.ground(c),
                };
                if !terrain || c & OCCUPIED[layer] != 0 || (c & TRANSIT[layer] != 0 && !q.repath) {
                    return None;
                }
            }
        }
        Some(match from {
            Some(f) if f[0] != to[0] && f[1] != to[1] => 22,
            _ => 16,
        })
    }
    /// A large unit standing at `here` may step to `to` now (vt+0x90 =
    /// 0x4e53a0): its layer's large cost function, with no flags and no
    /// step before.
    pub fn large_step(&self, layer: usize, size: [i32; 2], here: [i32; 2], to: [i32; 2]) -> bool {
        let cost = match layer {
            0 => Cost::Air,
            2 => Cost::Water,
            _ => Cost::Land,
        };
        let q = Query {
            start: here,
            rect: [to[0], to[1], to[0], to[1]],
            range: 0,
            repath: false,
            cost,
            size,
        };
        self.large_cost(&q, None, to).is_some()
    }
    /// The cell is land (its kind, `& 0x60000 == 0x20000`).
    pub fn land(&self, p: [i32; 2]) -> bool {
        self.inside(p) && self.at(p) & KIND == LAND
    }
    /// 0x500bb0: a unit of `layer` may step into the cell now.
    pub fn free(&self, layer: usize, p: [i32; 2]) -> bool {
        if !self.inside(p) {
            return false;
        }
        let w = self.at(p);
        match layer {
            0 => w & OCCUPIED[0] == 0,
            _ => {
                let cost = if layer == 2 { Cost::Water } else { Cost::Land };
                let q = Query {
                    start: p,
                    rect: [p[0], p[1], p[0], p[1]],
                    range: 0,
                    repath: false,
                    cost,
                    size: [1, 1],
                };
                self.cost(&q, None, None, p).is_some() && w & TRANSIT[layer] == 0
            }
        }
    }
}

/// The cost function of a search: units are always one cell (0x4ffb60).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cost {
    Land,
    /// The ninja's: land that climbs walls and cliffs (0x4a99a0).
    Ninja,
    /// The computer's search through the walls (0x4170d0): land whose
    /// walls cost 1.
    Breach,
    Water,
    Air,
}
/// A path request (the context of 0x4a9060).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Query {
    pub start: [i32; 2],
    pub rect: [i32; 4],
    pub range: i32,
    /// Flag 1: a re-path after a block also avoids moving units.
    pub repath: bool,
    pub cost: Cost,
    /// The searcher's cells (+0x4, +0x6 of the context), more than one for
    /// a large unit (0x4df59d, 0x4e857c, 0x4e79dc).
    pub size: [i32; 2],
}

/// trunc(√(dx² + dy²) · 16), 0x4aada0.
fn distance(a: [i32; 2], b: [i32; 2]) -> u32 {
    let dx = (a[0] - b[0]).abs() as i16 as i32;
    let dy = (a[1] - b[1]).abs() as i16 as i32;
    (((dx * dx + dy * dy) as f64).sqrt() * 16.) as u32
}
/// (dx² + dy²) · 16, 0x4aae00.
fn square(a: [i32; 2], b: [i32; 2]) -> u32 {
    let dx = (a[0] - b[0]) as i16 as i32;
    let dy = (a[1] - b[1]) as i16 as i32;
    ((dx * dx + dy * dy) << 4) as u32
}
fn centre(r: [i32; 4]) -> [i32; 2] {
    [(r[0] + r[2]) / 2, (r[1] + r[3]) / 2]
}
fn within(r: [i32; 4], p: [i32; 2]) -> bool {
    p[0] >= r[0] && p[0] <= r[2] && p[1] >= r[1] && p[1] <= r[3]
}

/// Neighbours of a node (0x58e4b8), and the forward ones by the direction
/// it was entered from (0x58e4d8–0x58e544).
const ALL: [[i32; 2]; 8] = [
    [0, -1],
    [1, -1],
    [1, 0],
    [1, 1],
    [0, 1],
    [-1, 1],
    [-1, 0],
    [-1, -1],
];
const EAST: [[i32; 2]; 3] = [[1, 0], [1, -1], [1, 1]];
const SOUTH_EAST: [[i32; 2]; 5] = [[1, 1], [1, -1], [1, 0], [0, 1], [-1, 1]];
const SOUTH: [[i32; 2]; 3] = [[0, 1], [1, 1], [-1, 1]];
const SOUTH_WEST: [[i32; 2]; 5] = [[-1, 1], [1, 1], [0, 1], [-1, 0], [-1, -1]];
const WEST: [[i32; 2]; 3] = [[-1, 0], [-1, 1], [-1, -1]];
const NORTH_WEST: [[i32; 2]; 5] = [[-1, -1], [0, -1], [1, -1], [-1, 1], [-1, 0]];
const NORTH: [[i32; 2]; 3] = [[0, -1], [1, -1], [-1, -1]];
const NORTH_EAST: [[i32; 2]; 5] = [[1, -1], [0, -1], [1, 0], [1, 1], [-1, -1]];
fn forward(parent: [i32; 2], node: [i32; 2]) -> &'static [[i32; 2]] {
    use std::cmp::Ordering::*;
    match (parent[0].cmp(&node[0]), parent[1].cmp(&node[1])) {
        (Less, Less) => &SOUTH_EAST,
        (Less, Greater) => &NORTH_EAST,
        (Less, Equal) => &EAST,
        (Greater, Less) => &SOUTH_WEST,
        (Greater, Greater) => &NORTH_WEST,
        (Greater, Equal) => &WEST,
        (Equal, Less) => &SOUTH,
        (Equal, Greater) => &NORTH,
        (Equal, Equal) => &ALL,
    }
}

#[derive(Clone, Copy, Debug)]
struct Node {
    g: u32,
    h: u32,
    f: u32,
    open: bool,
    parent: Option<u32>,
}
/// The open list (0x4a8d30–0x4a8f10): a binary heap on f whose popped root
/// stays in place until the next operation.
#[derive(Default)]
struct Heap {
    items: Vec<u32>,
    count: usize,
    popped: bool,
}
impl Heap {
    fn up(&mut self, nodes: &[Node], mut pos: usize) {
        let n = self.items[pos - 1];
        while pos > 1 {
            let parent = pos / 2;
            if nodes[self.items[parent - 1] as usize].f > nodes[n as usize].f {
                self.items[pos - 1] = self.items[parent - 1];
                pos = parent;
            } else {
                break;
            }
        }
        self.items[pos - 1] = n;
    }
    fn down(&mut self, nodes: &[Node], mut pos: usize) {
        let n = self.items[pos - 1];
        loop {
            let mut c = 2 * pos;
            if c < self.count {
                if nodes[self.items[c] as usize].f < nodes[self.items[c - 1] as usize].f {
                    c += 1;
                }
            } else if c > self.count {
                break;
            }
            if nodes[self.items[c - 1] as usize].f < nodes[n as usize].f {
                self.items[pos - 1] = self.items[c - 1];
                pos = c;
            } else {
                break;
            }
        }
        self.items[pos - 1] = n;
    }
    fn push(&mut self, nodes: &[Node], n: u32) {
        if self.popped {
            self.items[0] = n;
            self.down(nodes, 1);
            self.popped = false;
        } else {
            if self.items.len() <= self.count {
                self.items.resize(self.count + 1, 0);
            }
            self.items[self.count] = n;
            self.count += 1;
            self.up(nodes, self.count);
        }
    }
    fn pop(&mut self, nodes: &[Node]) -> Option<u32> {
        if self.count == 0 {
            return None;
        }
        if self.popped {
            if self.count == 1 {
                return None;
            }
            self.count -= 1;
            self.items[0] = self.items[self.count];
            self.down(nodes, 1);
        }
        self.popped = true;
        Some(self.items[0])
    }
    fn update(&mut self, nodes: &[Node], n: u32) {
        if self.popped {
            self.count -= 1;
            self.items[0] = self.items[self.count];
            self.down(nodes, 1);
            self.popped = false;
        }
        let Some(i) = self.items[..self.count].iter().position(|&x| x == n) else {
            return;
        };
        self.up(nodes, i + 1);
        if self.items[i] == n {
            self.down(nodes, i + 1);
        }
    }
}

/// The target rect of a search within the map (0x4a9060 clamps it).
pub(crate) fn clamped(side: usize, mut rect: [i32; 4]) -> [i32; 4] {
    let side = side as i32;
    rect[0] = rect[0].max(0);
    rect[1] = rect[1].max(0);
    rect[2] = rect[2].min(side - 1);
    rect[3] = rect[3].min(side - 1);
    rect
}
/// 0x4aa000: a node of the search is at its goal: in the rect, or nearer
/// than the range to it.
pub(crate) fn arrives(side: usize, q: &Query, p: [i32; 2]) -> bool {
    let rect = clamped(side, q.rect);
    if within(rect, p) {
        return true;
    }
    let grown = [
        rect[0] - q.range,
        rect[1] - q.range,
        rect[2] + q.range,
        rect[3] + q.range,
    ];
    if !within(grown, p) {
        return false;
    }
    let c = [p[0].clamp(rect[0], rect[2]), p[1].clamp(rect[1], rect[3])];
    let d = [c[0] - p[0], c[1] - p[1]];
    q.range * q.range > d[0] * d[0] + d[1] * d[1]
}

/// 0x4a9060: the path from the start (excluded) toward the rect. A search
/// that runs out of nodes leads to the generated node of least f; `None`
/// when that is the start itself.
pub(crate) fn search(cells: &Cells, regions: &mut Regions, q: &Query) -> Option<Vec<[u16; 2]>> {
    let rect = clamped(cells.side, q.rect);
    let point = centre(rect);
    let goal = |p: [i32; 2]| arrives(cells.side, q, p);
    // Land searches follow the route between regions: the regions after the
    // start's, less the last, then straight to the target point.
    let mut limit: u32 = 0x80;
    let mut guide: Vec<u32> = Vec::new();
    let mut remaining = 0u32;
    match q.cost {
        Cost::Land => {
            if regions.side != cells.side || regions.sectors.is_empty() {
                regions.refresh(cells);
            }
            if let Some(start) = regions.region_at(q.start)
                && let Some(route) = regions.route(start, point, rect)
                && route.len() >= 3
            {
                guide = route[1..route.len() - 1].to_vec();
                for w in guide.windows(2) {
                    remaining =
                        remaining.wrapping_add(distance(regions.point(w[0]), regions.point(w[1])));
                }
                let last = *guide.last().unwrap();
                remaining = remaining.wrapping_add(distance(regions.point(last), point));
            }
        }
        // 0x4a9106: the breach's keeps 0x80 nodes, without the regions.
        Cost::Breach => {}
        // 0x4a910d: the other cost functions, the ninja's too, search 0x800
        // nodes without the regions.
        Cost::Water | Cost::Air | Cost::Ninja => limit = 0x800,
    }
    let mut cursor = (!guide.is_empty()).then_some(0usize);
    let heuristic = |p: [i32; 2], cursor: Option<usize>, remaining: u32, regions: &Regions| {
        let base = distance(p, point);
        match cursor {
            None => base.wrapping_mul(5),
            Some(_) if base == 0 => 0,
            Some(c) => {
                let via = distance(p, regions.point(guide[c])).wrapping_add(remaining);
                if base - base / 8 + 128 <= via {
                    base.wrapping_add(via.wrapping_mul(4))
                } else {
                    base.wrapping_mul(5)
                }
            }
        }
    };
    let mut nodes = Vec::with_capacity(256);
    let mut cell = Vec::with_capacity(256);
    let mut at: HashMap<[i32; 2], u32> = HashMap::new();
    let mut heap = Heap::default();
    let h = heuristic(q.start, cursor, remaining, regions);
    nodes.push(Node {
        g: 0,
        h,
        f: h,
        open: true,
        parent: None,
    });
    cell.push(q.start);
    at.insert(q.start, 0);
    heap.push(&nodes, 0);
    let mut last = 0u32;
    let mut found = None;
    let mut next = heap.pop(&nodes);
    while let Some(n) = next {
        nodes[n as usize].open = false;
        if limit == 0 {
            break;
        }
        limit -= 1;
        let p = cell[n as usize];
        if let Some(c) = cursor {
            let r = regions.get(guide[c]);
            if within(r.rect, p) && regions.id[cells.index(p)] == r.id {
                if c + 1 < guide.len() {
                    remaining =
                        remaining.wrapping_sub(distance(r.point, regions.point(guide[c + 1])));
                    cursor = Some(c + 1);
                } else {
                    cursor = None;
                }
            }
        }
        // Only the node expanded right after its parent prunes its neighbours.
        let table = if nodes[n as usize].parent == Some(last) {
            forward(cell[last as usize], p)
        } else {
            &ALL
        };
        last = n;
        if goal(p) {
            found = Some(n);
            break;
        }
        if nodes.len() > POOL {
            break;
        }
        for d in table {
            let to = [p[0] + d[0], p[1] + d[1]];
            if !cells.inside(to) {
                continue;
            }
            let before = nodes[n as usize].parent.map(|k| cell[k as usize]);
            let Some(step) = cells.cost(q, before, Some(p), to) else {
                continue;
            };
            let g = nodes[n as usize].g.wrapping_add(step);
            if let Some(&e) = at.get(&to) {
                let e = e as usize;
                if nodes[e].g <= g {
                    continue;
                }
                nodes[e].parent = Some(n);
                nodes[e].g = g;
                nodes[e].f = nodes[e].h.wrapping_add(g);
                if nodes[e].open {
                    heap.update(&nodes, e as u32);
                } else {
                    nodes[e].open = true;
                    heap.push(&nodes, e as u32);
                }
            } else {
                let h = heuristic(to, cursor, remaining, regions);
                let k = nodes.len() as u32;
                nodes.push(Node {
                    g,
                    h,
                    f: g.wrapping_add(h),
                    open: true,
                    parent: Some(n),
                });
                cell.push(to);
                at.insert(to, k);
                heap.push(&nodes, k);
            }
        }
        next = heap.pop(&nodes);
    }
    let end = match found {
        Some(n) => n as usize,
        None => {
            let mut best = 0;
            for k in 1..nodes.len() {
                if nodes[k].f < nodes[best].f {
                    best = k;
                }
            }
            if best == 0 {
                return None;
            }
            best
        }
    };
    let mut path = Vec::new();
    let mut k = end;
    while let Some(parent) = nodes[k].parent {
        path.push([cell[k][0] as u16, cell[k][1] as u16]);
        k = parent as usize;
    }
    path.reverse();
    Some(path)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(PartialEq))]
struct Region {
    /// The centre of its sector, the point of every region in it.
    point: [i32; 2],
    rect: [i32; 4],
    id: u8,
    /// At most 16 neighbouring regions (+0x22).
    links: Vec<u32>,
    /// Bit 0 a goal to check, bit 1 a confirmed goal (+0x10).
    flags: u8,
    /// The cached routes through it (+0xc).
    cached: u32,
    next: Option<u32>,
    search: Option<u32>,
}
#[derive(Clone, Copy, Debug)]
struct Step {
    region: u32,
    h: u32,
    parent: Option<u32>,
}

/// The regions of every 8×8 sector, their links and the 32 cached routes
/// between them (0xc8b010, 0xc8af3c).
///
/// Saved with the game, its cached routes too, so that a game taken up
/// again walks the same ways; the original builds it again at loading.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(PartialEq))]
#[serde(default)]
pub(crate) struct Regions {
    side: usize,
    width: usize,
    /// The region of each cell inside its sector, 0xff for none.
    id: Vec<u8>,
    /// Regions of each sector in creation order (the native list starts
    /// from the last).
    sectors: Vec<Vec<u32>>,
    nodes: Vec<Option<Region>>,
    spare: Vec<u32>,
    /// Passability the graph was built on.
    #[serde(with = "bits")]
    pass: Vec<bool>,
    /// The objects' marks it was built on, to find a removed one's rect.
    marks: Vec<([i32; 4], [i32; 4])>,
    /// Each slot runs from the route's last region to its first.
    cache: Vec<Option<Vec<u32>>>,
    cached: u32,
    slot: usize,
    /// Routes searched with more nodes than this are cached (0x58e558).
    threshold: u32,
}
/// A list of flags saved eight to a byte.
mod bits {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &[bool], s: S) -> Result<S::Ok, S::Error> {
        let mut out = vec![0u8; v.len().div_ceil(8)];
        for (i, &b) in v.iter().enumerate() {
            if b {
                out[i / 8] |= 1 << (i % 8);
            }
        }
        (v.len(), out).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<bool>, D::Error> {
        let (n, bytes): (usize, Vec<u8>) = Deserialize::deserialize(d)?;
        if n > bytes.len() * 8 {
            return Err(serde::de::Error::custom("flags"));
        }
        Ok((0..n).map(|i| bytes[i / 8] & (1 << (i % 8)) != 0).collect())
    }
}
impl Regions {
    /// The graph read from a save fits a map of `side` cells (or is empty,
    /// to be built at the first tick).
    pub(crate) fn valid(&self, side: usize) -> bool {
        if self.id.is_empty() && self.nodes.is_empty() {
            return true;
        }
        let width = (side.max(1) - 1) / 8 + 1;
        let n = self.nodes.len();
        let fits = |r: &u32| (*r as usize) < n;
        n <= 1 << 22
            && self.side == side
            && self.width == width
            && self.id.len() == side * side
            && self.pass.len() == side * side
            && self.sectors.len() == width * width
            && self
                .sectors
                .iter()
                .flatten()
                .all(|r| fits(r) && self.nodes[*r as usize].is_some())
            && self.nodes.iter().flatten().all(|g| {
                g.links.iter().all(fits)
                    && g.next.as_ref().is_none_or(fits)
                    && g.search.as_ref().is_none_or(fits)
            })
            && self.spare.iter().all(fits)
            && self.cache.len() <= 64
            && self.slot < self.cache.len().max(1)
            && self.cache.iter().flatten().flatten().all(fits)
    }
    fn get(&self, r: u32) -> &Region {
        self.nodes[r as usize].as_ref().expect("live region")
    }
    fn get_mut(&mut self, r: u32) -> &mut Region {
        self.nodes[r as usize].as_mut().expect("live region")
    }
    fn point(&self, r: u32) -> [i32; 2] {
        self.get(r).point
    }
    fn sector(&self, p: [i32; 2]) -> usize {
        (p[1] >> 3) as usize * self.width + (p[0] >> 3) as usize
    }
    /// The region of a cell (0x4a9137 and its siblings).
    fn region_at(&self, p: [i32; 2]) -> Option<u32> {
        if p[0] < 0 || p[1] < 0 || p[0] as usize >= self.side || p[1] as usize >= self.side {
            return None;
        }
        let id = self.id[p[1] as usize * self.side + p[0] as usize];
        if id == NO_REGION {
            return None;
        }
        self.sectors[self.sector(p)]
            .iter()
            .rev()
            .copied()
            .find(|&r| self.get(r).id == id)
    }
    /// 0x4aa970: every sector's regions, then their links.
    fn build(cells: &Cells) -> Self {
        let side = cells.side;
        let width = (side.max(1) - 1) / 8 + 1;
        let mut r = Self {
            side,
            width,
            id: vec![NO_REGION; side * side],
            sectors: vec![Vec::new(); width * width],
            pass: (0..side * side)
                .map(|i| cells.passable([(i % side) as i32, (i / side) as i32]))
                .collect(),
            marks: cells.marks.clone(),
            cache: vec![None; 32],
            threshold: 0x40,
            ..Self::default()
        };
        for sy in 0..width {
            for sx in 0..width {
                r.sectors[sy * width + sx] = r.regions(sx, sy);
            }
        }
        for s in 0..width * width {
            r.link(s);
        }
        r
    }
    /// The sector's cells, clamped to the map.
    fn rect(&self, sx: usize, sy: usize) -> [i32; 4] {
        let last = self.side as i32 - 1;
        let (x, y) = (sx as i32 * 8, sy as i32 * 8);
        [x, y, (x + 7).min(last), (y + 7).min(last)]
    }
    /// 0x4aa1f0: flood the sector into 8-connected regions.
    fn regions(&mut self, sx: usize, sy: usize) -> Vec<u32> {
        let rect = self.rect(sx, sy);
        for y in rect[1]..=rect[3] {
            for x in rect[0]..=rect[2] {
                self.id[y as usize * self.side + x as usize] = NO_REGION;
            }
        }
        let mut list = Vec::new();
        let mut next = 0u8;
        for y in rect[1]..=rect[3] {
            for x in rect[0]..=rect[2] {
                let i = y as usize * self.side + x as usize;
                if self.id[i] != NO_REGION || !self.pass[i] {
                    continue;
                }
                // 0x4aa0a0
                let mut stack = vec![[x, y]];
                while let Some(p) = stack.pop() {
                    if !within(rect, p) {
                        continue;
                    }
                    let i = p[1] as usize * self.side + p[0] as usize;
                    if self.id[i] != NO_REGION || !self.pass[i] {
                        continue;
                    }
                    self.id[i] = next;
                    for d in ALL {
                        stack.push([p[0] + d[0], p[1] + d[1]]);
                    }
                }
                let region = Region {
                    point: centre(rect),
                    rect,
                    id: next,
                    links: Vec::new(),
                    flags: 0,
                    cached: 0,
                    next: None,
                    search: None,
                };
                let k = match self.spare.pop() {
                    Some(k) => {
                        self.nodes[k as usize] = Some(region);
                        k
                    }
                    None => {
                        self.nodes.push(Some(region));
                        self.nodes.len() as u32 - 1
                    }
                };
                list.push(k);
                next = next.wrapping_add(1);
            }
        }
        list
    }
    /// 0x4aa3a0: a link from `r` to the region of a cell.
    fn add(&mut self, r: u32, p: [i32; 2]) {
        let Some(to) = self.region_at(p) else {
            return;
        };
        let links = &self.get(r).links;
        if links.contains(&to) || links.len() >= 16 {
            return;
        }
        self.get_mut(r).links.push(to);
    }
    /// 0x4aa470: the links of a sector's regions to the cells around it.
    fn link(&mut self, s: usize) {
        if self.sectors[s].is_empty() {
            return;
        }
        let [x1, y1, x2, y2] = self.get(self.sectors[s][0]).rect;
        for x in x1..=x2 {
            if let Some(r) = self.region_at([x, y1]) {
                for dx in -1..=1 {
                    self.add(r, [x + dx, y1 - 1]);
                }
            }
        }
        for x in x1..=x2 {
            if let Some(r) = self.region_at([x, y2]) {
                for dx in -1..=1 {
                    self.add(r, [x + dx, y2 + 1]);
                }
            }
        }
        for y in y1..=y2 {
            if let Some(r) = self.region_at([x1, y]) {
                for dy in -1..=1 {
                    self.add(r, [x1 - 1, y + dy]);
                }
            }
        }
        for y in y1..=y2 {
            if let Some(r) = self.region_at([x2, y]) {
                for dy in -1..=1 {
                    self.add(r, [x2 + 1, y + dy]);
                }
            }
        }
        for (corner, out) in [
            ([x1, y1], [x1 - 1, y1 - 1]),
            ([x2, y1], [x2 + 1, y1 - 1]),
            ([x1, y2], [x1 - 1, y2 + 1]),
            ([x2, y2], [x2 + 1, y2 + 1]),
        ] {
            if let Some(r) = self.region_at(corner) {
                self.add(r, out);
            }
        }
    }
    /// 0x4aae30: forget a cached route.
    fn forget(&mut self, slot: usize) {
        if let Some(route) = self.cache[slot].take() {
            self.cached -= 1;
            for r in route {
                if let Some(Some(x)) = self.nodes.get_mut(r as usize) {
                    x.cached &= !(1 << slot);
                }
            }
        }
    }
    /// Rebuild the regions after the passability changed, as each change
    /// does natively (0x4aab20) with the rect of the object that made it (a
    /// felled tree its cell).
    pub fn refresh(&mut self, cells: &Cells) {
        let n = cells.side * cells.side;
        self.refresh_at(cells, 0..n);
    }
    /// `refresh` when the passability can have changed only at `candidates`
    /// (cell indices in increasing order): the same rects, in the same order.
    pub fn refresh_at(&mut self, cells: &Cells, candidates: impl IntoIterator<Item = usize>) {
        if self.side != cells.side || self.sectors.is_empty() {
            *self = Self::build(cells);
            return;
        }
        let mut rects: Vec<[i32; 4]> = Vec::new();
        for i in candidates {
            let p = [(i % self.side) as i32, (i / self.side) as i32];
            let now = cells.passable(p);
            if now != self.pass[i] {
                self.pass[i] = now;
                let rect = cells
                    .marks
                    .iter()
                    .chain(&self.marks)
                    .find(|(own, _)| within(*own, p))
                    .map_or([p[0], p[1], p[0], p[1]], |&(_, rect)| rect);
                if !rects.contains(&rect) {
                    rects.push(rect);
                }
            }
        }
        self.marks.clone_from(&cells.marks);
        for rect in rects {
            self.update(rect);
        }
    }
    /// 0x4aab20 for a rect of cells: the routes cached in the sectors within
    /// two of its own are forgotten, its sectors flooded again and the links
    /// of those within one renewed.
    fn update(&mut self, rect: [i32; 4]) {
        let w = self.width as i32;
        let [x1, y1, x2, y2] = rect.map(|v| v >> 3);
        let around = |d: i32| {
            ((y1 - d).max(0)..=(y2 + d).min(w - 1)).flat_map(move |y| {
                ((x1 - d).max(0)..=(x2 + d).min(w - 1)).map(move |x| (y * w + x) as usize)
            })
        };
        for s in around(2) {
            for r in self.sectors[s].clone() {
                for slot in 0..32 {
                    if self.get(r).cached & (1 << slot) != 0 {
                        self.forget(slot);
                    }
                }
            }
        }
        for s in around(0) {
            for r in std::mem::take(&mut self.sectors[s]) {
                self.nodes[r as usize] = None;
                self.spare.push(r);
            }
            let (sx, sy) = (s % self.width, s / self.width);
            self.sectors[s] = self.regions(sx, sy);
        }
        for s in around(1) {
            for r in self.sectors[s].clone() {
                self.get_mut(r).links.clear();
            }
            self.link(s);
        }
    }
    /// 0x4aafb0: the region has cells inside the rect.
    fn contains(&self, r: u32, rect: [i32; 4]) -> bool {
        let x = self.get(r);
        let c = [
            rect[0].max(x.rect[0]),
            rect[1].max(x.rect[1]),
            rect[2].min(x.rect[2]),
            rect[3].min(x.rect[3]),
        ];
        if c == x.rect {
            return true;
        }
        (c[1]..=c[3])
            .any(|y| (c[0]..=c[2]).any(|cx| self.id[y as usize * self.side + cx as usize] == x.id))
    }
    /// 0x4ab090: the route of regions from `start` toward the rect, through
    /// the cache when a cached route reaches one of its regions. On failure
    /// it leads to the region of least f (then least h); `None` when that
    /// is the start.
    fn route(&mut self, start: u32, point: [i32; 2], rect: [i32; 4]) -> Option<Vec<u32>> {
        let mut limit = (self.side as i32 / 128 * 256).max(256) as u16;
        let mut goals = Vec::new();
        'sectors: for sy in rect[1] >> 3..=rect[3] >> 3 {
            for sx in rect[0] >> 3..=rect[2] >> 3 {
                for &r in self.sectors[sy as usize * self.width + sx as usize]
                    .iter()
                    .rev()
                {
                    if goals.len() == 0x80 {
                        break 'sectors;
                    }
                    goals.push(r);
                }
            }
        }
        for &r in &goals {
            self.get_mut(r).flags |= 1;
        }
        let mut pool = vec![Step {
            region: start,
            h: square(self.point(start), point),
            parent: None,
        }];
        let mut nodes = vec![Node {
            g: 0,
            h: pool[0].h,
            f: pool[0].h,
            open: true,
            parent: None,
        }];
        self.get_mut(start).search = Some(0);
        let mut heap = Heap::default();
        heap.push(&nodes, 0);
        let mut mask = 0u32;
        for &r in &goals {
            for slot in 0..32 {
                if self.cache[slot].as_ref().is_some_and(|c| c[0] == r) {
                    mask |= 1 << slot;
                }
            }
        }
        let mut found = None;
        let mut next = heap.pop(&nodes);
        'search: while let Some(n) = next {
            let n = n as usize;
            let r = pool[n].region;
            nodes[n].open = false;
            let before = limit;
            limit = limit.wrapping_sub(1);
            if before == 0 {
                break;
            }
            if self.get(r).flags & 1 != 0 {
                if self.contains(r, rect) {
                    self.get_mut(r).flags |= 2;
                } else {
                    self.get_mut(r).flags &= !1;
                }
            }
            if self.get(r).flags & 2 != 0 {
                self.get_mut(r).next = None;
                found = Some(n);
                break;
            }
            let mut bits = self.get(r).cached & mask;
            let mut slot = 0;
            while bits != 0 {
                if bits & 1 != 0 {
                    let route = self.cache[slot].clone().unwrap_or_default();
                    let end = route[0];
                    if self.get(end).flags & 1 != 0 {
                        if self.contains(end, rect) {
                            self.get_mut(end).flags |= 2;
                        } else {
                            self.get_mut(end).flags &= !1;
                        }
                    }
                    if self.get(end).flags & 2 != 0 {
                        // Follow the cached route from this region on.
                        self.get_mut(end).next = None;
                        for k in 0..route.len() - 1 {
                            if route[k] == r {
                                break;
                            }
                            self.get_mut(route[k + 1]).next = Some(route[k]);
                        }
                        if let Some(x) = self.get(r).next
                            && self.get(x).next.is_some()
                        {
                            found = Some(n);
                            break 'search;
                        }
                        mask = 0;
                        self.get_mut(r).next = None;
                        break;
                    }
                    mask &= !(1 << slot);
                }
                bits >>= 1;
                slot += 1;
            }
            if pool.len() > POOL {
                break;
            }
            for k in 0..self.get(r).links.len() {
                let to = self.get(r).links[k];
                let g = nodes[n]
                    .g
                    .wrapping_add(distance(self.point(r), self.point(to)));
                if let Some(e) = self.get(to).search {
                    let e = e as usize;
                    if nodes[e].g <= g {
                        continue;
                    }
                    pool[e].parent = Some(n as u32);
                    nodes[e].parent = Some(n as u32);
                    nodes[e].g = g;
                    nodes[e].f = nodes[e].h.wrapping_add(g);
                    if nodes[e].open {
                        heap.update(&nodes, e as u32);
                    } else {
                        nodes[e].open = true;
                        heap.push(&nodes, e as u32);
                    }
                } else {
                    let h = square(self.point(to), point);
                    let k = pool.len() as u32;
                    pool.push(Step {
                        region: to,
                        h,
                        parent: Some(n as u32),
                    });
                    nodes.push(Node {
                        g,
                        h,
                        f: g.wrapping_add(h),
                        open: true,
                        parent: Some(n as u32),
                    });
                    self.get_mut(to).search = Some(k);
                    heap.push(&nodes, k);
                }
            }
            next = heap.pop(&nodes);
        }
        for s in &pool {
            self.get_mut(s.region).search = None;
        }
        for &r in &goals {
            self.get_mut(r).flags &= !3;
        }
        let end = match found {
            Some(n) => n,
            None => {
                let mut best = 0;
                for k in 1..nodes.len() {
                    if nodes[k].f < nodes[best].f
                        || (nodes[k].f == nodes[best].f && nodes[k].h < nodes[best].h)
                    {
                        best = k;
                    }
                }
                self.get_mut(pool[best].region).next = None;
                if best == 0 {
                    return None;
                }
                best
            }
        };
        let mut k = end;
        while let Some(parent) = nodes[k].parent {
            let to = pool[k].region;
            self.get_mut(pool[parent as usize].region).next = Some(to);
            k = parent as usize;
        }
        let mut route = vec![start];
        while let Some(x) = self.get(*route.last().unwrap()).next {
            if route.len() > self.nodes.len() {
                break;
            }
            route.push(x);
        }
        // Long searches are cached, in the first free slot or in turn,
        // raising the bar when every slot is taken (0x4ab54f).
        let used = pool.len() as u32;
        if (used > self.threshold || self.cached < 32) && used > 5 {
            if self.cached < 32 {
                self.slot = self.cache.iter().position(Option::is_none).unwrap_or(0);
            } else {
                self.threshold += 5;
            }
            let slot = self.slot;
            self.forget(slot);
            for &r in &route {
                self.get_mut(r).cached |= 1 << slot;
            }
            self.cache[slot] = Some(route.iter().rev().copied().collect());
            self.cached += 1;
            self.slot = (self.slot + 1) & 31;
        } else {
            self.threshold = if self.threshold > 5 {
                self.threshold - 1
            } else {
                5
            };
        }
        Some(route)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(side: usize) -> Cells {
        Cells {
            side,
            word: vec![0x90 | LAND; side * side],
            trunks: 3,
            marks: Vec::new(),
        }
    }
    fn query(start: [i32; 2], to: [i32; 2]) -> Query {
        Query {
            start,
            rect: [to[0], to[1], to[0], to[1]],
            range: 0,
            repath: false,
            cost: Cost::Land,
            size: [1, 1],
        }
    }

    #[test]
    fn distances_match_the_native_helpers() {
        assert_eq!(distance([0, 0], [3, 4]), 80);
        assert_eq!(distance([0, 0], [1, 1]), 22);
        assert_eq!(square([0, 0], [3, 4]), 400);
        // 0x476da0: the second surface (bits 7–9) decides land; ground
        // only in the first is kind 3, as along the shores.
        assert_eq!(kind(0x90), LAND);
        assert_eq!(kind(0x80), LAND);
        assert_eq!(kind(0), WATER);
        assert_eq!(kind(0x10), SHORE);
        assert_eq!(kind(0x490), 0);
    }

    #[test]
    fn heap_keeps_the_popped_root_until_the_next_operation() {
        let mut nodes: Vec<Node> = [5, 3, 8, 3]
            .iter()
            .map(|&f| Node {
                g: 0,
                h: 0,
                f,
                open: true,
                parent: None,
            })
            .collect();
        let mut heap = Heap::default();
        for n in 0..4 {
            heap.push(&nodes, n);
        }
        assert_eq!(heap.pop(&nodes), Some(1));
        // A push after a pop replaces the root.
        nodes.push(Node {
            g: 0,
            h: 0,
            f: 1,
            open: true,
            parent: None,
        });
        heap.push(&nodes, 4);
        assert_eq!(heap.pop(&nodes), Some(4));
        assert_eq!(heap.pop(&nodes), Some(3));
        assert_eq!(heap.pop(&nodes), Some(0));
        assert_eq!(heap.pop(&nodes), Some(2));
        assert_eq!(heap.pop(&nodes), None);
    }

    #[test]
    fn straight_and_diagonal_paths_cut_corners() {
        let mut cells = open(16);
        let mut regions = Regions::default();
        let path = search(&cells, &mut regions, &query([2, 2], [6, 2])).unwrap();
        assert_eq!(path, [[3, 2], [4, 2], [5, 2], [6, 2]]);
        // Two trees touching at a corner do not stop a diagonal step.
        cells.set([5, 5], 31 << 11, true);
        cells.set([6, 6], 31 << 11, true);
        let path = search(&cells, &mut regions, &query([5, 6], [6, 5])).unwrap();
        assert_eq!(path, [[6, 5]]);
    }

    #[test]
    fn a_reached_start_gives_an_empty_path_and_a_blocked_one_none() {
        let mut cells = open(8);
        let mut regions = Regions::default();
        assert_eq!(
            search(&cells, &mut regions, &query([3, 3], [3, 3])),
            Some(vec![])
        );
        for d in ALL {
            cells.set([3 + d[0], 3 + d[1]], BLOCKED, true);
        }
        assert_eq!(search(&cells, &mut regions, &query([3, 3], [6, 6])), None);
    }

    #[test]
    fn moving_units_block_only_a_repath() {
        let mut cells = open(8);
        let mut regions = Regions::default();
        for y in 0..8 {
            if y != 4 {
                cells.set([4, y], BLOCKED, true);
            }
        }
        cells.set([4, 4], OCCUPIED[1] | TRANSIT[1], true);
        let mut q = query([2, 4], [6, 4]);
        assert_eq!(
            search(&cells, &mut regions, &q).unwrap().last(),
            Some(&[6, 4])
        );
        q.repath = true;
        // Nothing better than the start toward the target is left.
        let path = search(&cells, &mut regions, &q).unwrap();
        assert_ne!(path.last(), Some(&[6, 4]));
    }

    #[test]
    fn range_reaches_the_target_from_a_disk() {
        let cells = open(16);
        let mut regions = Regions::default();
        let mut q = query([1, 5], [10, 5]);
        q.range = 3;
        // Within the range means d² < r² from the target (0x4aa000).
        let path = search(&cells, &mut regions, &q).unwrap();
        assert_eq!(path.last(), Some(&[8, 5]));
    }

    #[test]
    fn regions_split_sectors_and_route_around_a_wall() {
        let mut cells = open(32);
        // A wall down column 12 with a gap at the bottom.
        for y in 0..30 {
            cells.set([12, y], BLOCKED, true);
        }
        let mut regions = Regions::default();
        regions.refresh(&cells);
        let a = regions.region_at([2, 2]).unwrap();
        let b = regions.region_at([20, 2]).unwrap();
        assert_ne!(a, b);
        // Column 12 splits sector (1, 0) into two regions.
        assert_eq!(regions.sectors[1].len(), 2);
        let route = regions.route(a, [20, 2], [20, 2, 20, 2]).unwrap();
        assert_eq!(route.first(), Some(&a));
        assert!(route.iter().any(|&r| regions.get(r).rect[1] >= 24));
        // The first guided search of 128 nodes heads down for the gap.
        let path = search(&cells, &mut regions, &query([2, 2], [20, 2])).unwrap();
        assert!(path.iter().any(|p| p[1] >= 20));
    }

    #[test]
    fn walks_around_a_lake_in_partial_searches() {
        let mut cells = open(40);
        for y in 4..30 {
            for x in 14..20 {
                cells.set([x, y], WATER, true);
                cells.set([x, y], LAND, false);
            }
        }
        let mut regions = Regions::default();
        // A walk searches again from the end of a partial path (0x4deb40).
        let mut at = [6, 16];
        for _ in 0..4 {
            let path = search(&cells, &mut regions, &query(at, [28, 16])).unwrap();
            let end = *path.last().unwrap();
            at = [end[0] as i32, end[1] as i32];
            if at == [28, 16] {
                break;
            }
        }
        assert_eq!(at, [28, 16]);
    }

    #[test]
    fn a_change_rebuilds_the_sectors_of_the_rect_its_object_passes() {
        let mut cells = open(32);
        cells.set([10, 7], BLOCKED, true);
        let mut regions = Regions::default();
        regions.refresh(&cells);
        assert_eq!(regions.region_at([10, 7]), None);
        // A gate facing down on row 7 passes the rows beside its door, in
        // the next row of sectors (0x4577dd): the door's sector keeps its
        // regions as they were.
        cells.marks.push(([10, 7, 10, 7], [10, 8, 10, 9]));
        cells.set([10, 7], BLOCKED, false);
        regions.refresh(&cells);
        assert_eq!(regions.region_at([10, 7]), None);
        // A building's own rect covers the cell: its sector is rebuilt.
        cells.marks = vec![([20, 5, 21, 6], [20, 5, 21, 6])];
        cells.set([20, 5], BLOCKED, true);
        regions.refresh(&cells);
        cells.set([20, 5], BLOCKED, false);
        cells.marks.clear();
        regions.refresh(&cells);
        assert!(regions.region_at([20, 5]).is_some());
        assert!(regions.region_at([10, 7]).is_none());
    }
    #[test]
    fn a_change_rebuilds_the_regions_of_its_sector() {
        let mut cells = open(32);
        for y in 0..32 {
            cells.set([12, y], BLOCKED, true);
        }
        let mut regions = Regions::default();
        regions.refresh(&cells);
        assert!(
            regions
                .route(regions.region_at([2, 2]).unwrap(), [20, 2], [20, 2, 20, 2])
                .is_some()
        );
        let east = regions.region_at([20, 2]).unwrap();
        assert!(
            regions
                .route(regions.region_at([2, 2]).unwrap(), [20, 2], [20, 2, 20, 2])
                .is_some_and(|r| *r.last().unwrap() != east)
        );
        cells.set([12, 20], BLOCKED, false);
        regions.refresh(&cells);
        let route = regions
            .route(regions.region_at([2, 2]).unwrap(), [20, 2], [20, 2, 20, 2])
            .unwrap();
        assert_eq!(*route.last().unwrap(), regions.region_at([20, 2]).unwrap());
    }
}
