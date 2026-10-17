//! The map of islands of `Tzar.exe` (0x60a630, built at the start of a game
//! by 0x468950): the land or water region of every cell, how the regions
//! touch, the bridges that join them and the questions asked of them. The
//! computer players find their way with it; the original saves it with the
//! game, its two caches excepted.
use super::*;

/// Region ids run from 1 to 254; 0 is no region, 255 a region wiped.
const IDS: u8 = 254;
pub const NONE: u8 = 0;
pub const WIPED: u8 = 0xff;
/// Region kinds, as the cell kinds (0x476da0).
pub const LAND: u8 = 1;
pub const WATER: u8 = 2;
const SHORE: u8 = 3;
/// A region needs this many cells to keep its id (0x469b90).
const SMALLEST: u32 = 9;
/// The pairs of regions (+8): the bits of pair (a, b), a > b, at
/// a(a − 1)/2 + b.
const PAIRS: usize = 0x7e81;
pub mod link {
    /// The regions touch.
    pub const ADJACENT: u8 = 1;
    /// They touch a same region (0x469f80): two islands on one water, or
    /// two waters round one island.
    pub const BY_WATER: u8 = 2;
    /// A bridge joins them (0x468b70).
    pub const BRIDGE: u8 = 4;
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Islands {
    pub side: usize,
    /// The region of each cell (+4); empty before the map is built.
    pub cells: Vec<u8>,
    /// The bits of each pair of regions (+8).
    pub links: Vec<u8>,
    /// The regions by id (+0x7e8a).
    pub regions: Vec<Region>,
    /// The id the next region takes (+0x9870).
    pub next: u8,
    /// The bridges and the land regions at their ends (+0x9872).
    pub bridges: Vec<Bridge>,
    /// The last answers of `same_island` and of the computer players'
    /// reachability (+0x9876), which a change of the links clears. The
    /// original does not save them; ReTzar does, so that a game taken up
    /// again goes on as it would have.
    pub(crate) pairs: Cache<(u8, u8)>,
    /// Their answers between a point's region and a rect (+0x98a6).
    pub(crate) rects: Cache<(u8, [i32; 4])>,
    /// The points the coasts of the regions are walked from (+0x9916): each
    /// region's run, from `Region::coast`, ends with (0xffff, 0xffff).
    pub coasts: Vec<[u16; 2]>,
    /// The shore cells the flood of the region being made met, in order
    /// (+0x9922), while the map is built.
    #[serde(skip)]
    starts: Vec<[u16; 2]>,
}

/// A region (0x1a bytes).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Region {
    /// 1 land, 2 water; 0 once wiped.
    pub kind: u8,
    /// The shore cells it took (+4), counted at each visit of its flood.
    pub shore: u16,
    pub cells: u32,
    /// Its neighbours of land (+0xa) and of other kinds (+0xc).
    pub land_links: u16,
    pub other_links: u16,
    /// The bridges ending in it (+0xe) and the regions they join it to
    /// (+0x10).
    pub bridges: u16,
    pub bridged: u16,
    /// The cell its flood started from (+0x12).
    pub seed: [u16; 2],
    /// Its first point in `Islands::coasts` (+0x16).
    pub coast: Option<u32>,
}

/// A bridge and the land regions at its two ends.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bridge {
    pub object: usize,
    pub ends: [u8; 2],
}

/// Eight answers, the latest first: a hit moves to the front, a new one
/// pushes the last out (0x4695d0, 0x469660).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Cache<K> {
    entries: Vec<(K, bool)>,
}
impl<K> Default for Cache<K> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}
impl<K: PartialEq + Copy> Cache<K> {
    pub(crate) fn get(&mut self, key: K) -> Option<bool> {
        let i = self.entries.iter().position(|e| e.0 == key)?;
        let hit = self.entries.remove(i);
        self.entries.insert(0, hit);
        Some(hit.1)
    }
    pub(crate) fn put(&mut self, key: K, answer: bool) {
        self.entries.insert(0, (key, answer));
        self.entries.truncate(8);
    }
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}
/// Not part of the state a save compares.
impl<K> PartialEq for Cache<K> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl<K> Eq for Cache<K> {}

/// The kind of a cell's terrain (0x476da0): 0 none, 1 land, 2 water, 3
/// shore.
pub fn cell_kind(g: &MapGrid, i: usize) -> u8 {
    (pathing::kind(g.cells[i]) >> 17) as u8
}
/// A standing tree: an image above the trunks (ReTzar clears the image of
/// a felled tree, which the original turns into a trunk, 0x4554d0).
fn tree(g: &MapGrid, trunks: u8, i: usize) -> bool {
    ((g.cells[i] >> 11) & 31) as u8 > trunks
}
fn pair(a: u8, b: u8) -> usize {
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi as usize * (hi as usize).saturating_sub(1)) / 2 + lo as usize
}

/// What a flood does with a cell.
#[derive(Clone, Copy)]
enum Fill {
    /// 0x469880: grow the region.
    Grow(u8),
    /// 0x469d60: wipe the region's cells.
    Wipe(u8),
}

impl Islands {
    /// 0x468950: the regions in row order, each with its coast (0x46a730),
    /// then the bridges and the pairs that share a neighbour. The runs of
    /// free land (0x469230) come with the phases that read them.
    pub fn build(g: &MapGrid, m: &MissionWorld, lib: &ObjectLibrary) -> Self {
        let mut s = Self::regions_of(g, lib.trunks);
        for (i, o) in m.objects.iter().enumerate() {
            if o.class == class::BRIDGE && o.flags & FLAG_INACTIVE == 0 {
                s.bridge_built(g, lib, o, i);
            }
        }
        s.by_water();
        s
    }
    /// The map of a grid without bridges.
    #[cfg(test)]
    pub(crate) fn from_grid(g: &MapGrid, trunks: u8) -> Self {
        let mut s = Self::regions_of(g, trunks);
        s.by_water();
        s
    }
    fn regions_of(g: &MapGrid, trunks: u8) -> Self {
        let side = g.side;
        let mut s = Self {
            side,
            cells: vec![NONE; side * side],
            links: vec![0; PAIRS],
            regions: vec![Region::default(); 255],
            next: 1,
            ..Default::default()
        };
        for y in 0..side {
            for x in 0..side {
                s.start(g, trunks, x, y);
            }
        }
        s
    }
    pub fn built(&self) -> bool {
        !self.cells.is_empty()
    }
    pub fn links(&self, a: u8, b: u8) -> u8 {
        self.links.get(pair(a, b)).copied().unwrap_or(0)
    }
    pub fn kind(&self, r: u8) -> u8 {
        self.regions.get(r as usize).map_or(0, |x| x.kind)
    }
    /// The region of a cell; none outside the map.
    pub fn region(&self, p: [i32; 2]) -> u8 {
        if p[0] < 0 || p[1] < 0 || p[0] as usize >= self.side || p[1] as usize >= self.side {
            return NONE;
        }
        self.cells[p[1] as usize * self.side + p[0] as usize]
    }
    /// 0x469a10: the water cell belongs to a 2×2 block of water.
    fn water_block(g: &MapGrid, x: usize, y: usize) -> bool {
        let side = g.side;
        let water = |x: usize, y: usize| cell_kind(g, y * side + x) == WATER;
        (x > 0 && y > 0 && water(x - 1, y) && water(x, y - 1) && water(x - 1, y - 1))
            || (x + 1 < side && y > 0 && water(x + 1, y) && water(x, y - 1) && water(x + 1, y - 1))
            || (x + 1 < side
                && y + 1 < side
                && water(x + 1, y)
                && water(x, y + 1)
                && water(x + 1, y + 1))
            || (x > 0 && y + 1 < side && water(x - 1, y) && water(x, y + 1) && water(x - 1, y + 1))
    }
    /// 0x469b90: a new region from an unclaimed cell of land or open water.
    fn start(&mut self, g: &MapGrid, trunks: u8, x: usize, y: usize) {
        let i = y * self.side + x;
        let kind = match cell_kind(g, i) {
            LAND if !tree(g, trunks, i) => LAND,
            WATER if Self::water_block(g, x, y) => WATER,
            _ => return,
        };
        if self.cells[i] != NONE {
            return;
        }
        let id = self.next;
        self.regions[id as usize] = Region {
            kind,
            seed: [x as u16, y as u16],
            ..Default::default()
        };
        self.starts.clear();
        self.flood(g, trunks, [x, y], Fill::Grow(id));
        let cells = self.regions[id as usize].cells;
        if cells >= SMALLEST && id < IDS {
            self.next += 1;
            self.coast_list(g, id);
            return;
        }
        // Too small, or no id left: a bigger region wipes the smallest one
        // smaller than itself, keeping the last id for its cells (the next
        // region reuses its record).
        let mut smallest = id;
        if cells > SMALLEST && id > 1 {
            for r in 1..id {
                if self.regions[r as usize].cells < self.regions[smallest as usize].cells {
                    smallest = r;
                }
            }
        }
        self.erase(g, trunks, smallest);
    }
    /// 0x46a730: the coast of a region just made, walked from each shore
    /// cell its flood met (0x46a7c0) while the walk finds cells not walked
    /// yet; each walk leaves its first cell among the region's points, in
    /// place of the point an earlier walk started from when this one ran
    /// into it (0x46a8d0).
    fn coast_list(&mut self, g: &MapGrid, r: u8) {
        if self.starts.is_empty() {
            return;
        }
        let side = self.side;
        let mut walked = vec![false; side * side];
        let index = |p: [i32; 2]| p[1] as usize * side + p[0] as usize;
        for start in std::mem::take(&mut self.starts) {
            let p = [start[0] as i32, start[1] as i32];
            if walked[index(p)] {
                continue;
            }
            walked[index(p)] = true;
            let mut at = p;
            let mut end = [0xffff; 2];
            if self.coast_step(g, r, &mut at, true) {
                loop {
                    if walked[index(at)] {
                        end = [at[0] as u16, at[1] as u16];
                        break;
                    }
                    walked[index(at)] = true;
                    if !self.coast_step(g, r, &mut at, true) {
                        break;
                    }
                }
            }
            self.add_coast(r, start, end);
        }
    }
    /// 0x46a8d0: a point of a region's coast, unless it has it; it takes the
    /// place of `end`. The region's run is the last of the list.
    fn add_coast(&mut self, r: u8, start: [u16; 2], end: [u16; 2]) {
        const END: [u16; 2] = [0xffff; 2];
        match self.regions[r as usize].coast {
            Some(first) => {
                let mut i = first as usize;
                while let Some(&e) = self.coasts.get(i) {
                    if e == END {
                        break;
                    }
                    if e == start {
                        return;
                    }
                    if e == end {
                        self.coasts[i] = start;
                        return;
                    }
                    i += 1;
                }
                if i < self.coasts.len() {
                    self.coasts[i] = start;
                } else {
                    self.coasts.push(start);
                }
                self.coasts.push(END);
            }
            None => {
                self.regions[r as usize].coast = Some(self.coasts.len() as u32);
                self.coasts.push(start);
                self.coasts.push(END);
            }
        }
    }
    /// 0x46a9b0: a step along the coast of region `r` from the shore cell
    /// at `p`, by the shape of its edge (the tile's low bits, 0x58a1f0, and
    /// the step of each shape, 0x58a200); `p` moves even when the step
    /// fails. It holds when the new cell is the region's; while the map is
    /// built nothing else does; later a cell of a region of a higher number
    /// does if one of its eight neighbours is the region's and no shore
    /// (above, below, then beside).
    pub fn coast_step(&self, g: &MapGrid, r: u8, p: &mut [i32; 2], building: bool) -> bool {
        const SHAPE: [u8; 16] = [1, 3, 3, 4, 0, 4, 4, 2, 1, 0, 3, 2, 1, 2, 0, 0];
        const STEP: [[i32; 2]; 5] = [[0, 0], [0, -1], [-1, 0], [1, 0], [0, 1]];
        let side = self.side as i32;
        let index = |q: [i32; 2]| q[1] as usize * self.side + q[0] as usize;
        let shape = SHAPE[(g.cells[index(*p)] & 0xf) as usize];
        if shape == 0 {
            return false;
        }
        let d = STEP[shape as usize];
        *p = [p[0] + d[0], p[1] + d[1]];
        let [x, y] = *p;
        if x < 0 || y < 0 || x >= side || y >= side {
            return false;
        }
        let here = self.cells[index(*p)];
        if here == r {
            return true;
        }
        if building || here < r {
            return false;
        }
        let ours = |q: [i32; 2]| self.cells[index(q)] == r && cell_kind(g, index(q)) != SHORE;
        for dy in [-1, 1] {
            if (dy < 0 && y == 0) || (dy > 0 && y + 1 >= side) {
                continue;
            }
            for dx in [0, -1, 1] {
                if (dx < 0 && x == 0) || (dx > 0 && x + 1 >= side) {
                    continue;
                }
                if ours([x + dx, y + dy]) {
                    return true;
                }
            }
        }
        (x > 0 && ours([x - 1, y])) || (x + 1 < side && ours([x + 1, y]))
    }
    /// 0x469da0: a region's cells wiped and its links dropped.
    fn erase(&mut self, g: &MapGrid, trunks: u8, r: u8) {
        self.regions[r as usize].kind = 0;
        let seed = self.regions[r as usize].seed;
        self.flood(
            g,
            trunks,
            [seed[0] as usize, seed[1] as usize],
            Fill::Wipe(r),
        );
        for other in 1..self.next {
            if other != r {
                self.unlink(r, other);
                self.links[pair(r, other)] = 0;
            }
        }
    }
    /// The engine's flood (0x44fdd0, 0x44ffc0): breadth first over the
    /// eight neighbours, in a ring of 4 × side cells that drops what does
    /// not fit; the start cell always spreads.
    fn flood(&mut self, g: &MapGrid, trunks: u8, start: [usize; 2], fill: Fill) {
        const LEVEL: [i32; 2] = [i32::MIN, 0];
        let side = self.side;
        let capacity = side * 4;
        let mut queue: std::collections::VecDeque<[i32; 2]> = Default::default();
        let mut seen = vec![false; side * side];
        let push = |queue: &mut std::collections::VecDeque<[i32; 2]>,
                    seen: &mut Vec<bool>,
                    p: [i32; 2]| {
            if queue.len() >= capacity {
                return;
            }
            if p != LEVEL {
                if p[0] < 0 || p[1] < 0 || p[0] as usize >= side || p[1] as usize >= side {
                    return;
                }
                let i = p[1] as usize * side + p[0] as usize;
                if seen[i] {
                    return;
                }
                seen[i] = true;
            }
            queue.push_back(p);
        };
        let origin = [start[0] as i32, start[1] as i32];
        push(&mut queue, &mut seen, origin);
        push(&mut queue, &mut seen, LEVEL);
        while let Some(p) = queue.pop_front() {
            if p == LEVEL {
                if queue.is_empty() {
                    break;
                }
                push(&mut queue, &mut seen, LEVEL);
                continue;
            }
            let spread = match fill {
                Fill::Grow(id) => self.grow(g, trunks, id, p[0] as usize, p[1] as usize),
                Fill::Wipe(r) => {
                    let i = p[1] as usize * side + p[0] as usize;
                    let ours = self.cells[i] == r;
                    if ours {
                        self.cells[i] = WIPED;
                    }
                    ours
                }
            };
            if spread || p == origin {
                for [dx, dy] in [
                    [-1, -1],
                    [0, -1],
                    [1, -1],
                    [-1, 0],
                    [1, 0],
                    [-1, 1],
                    [0, 1],
                    [1, 1],
                ] {
                    push(&mut queue, &mut seen, [p[0] + dx, p[1] + dy]);
                }
            }
        }
    }
    /// 0x469880: a cell met by the flood of region `id`. Shore cells join
    /// whichever region reaches them last, without spreading; a cell of a
    /// region of another kind links the two.
    fn grow(&mut self, g: &MapGrid, trunks: u8, id: u8, x: usize, y: usize) -> bool {
        let i = y * self.side + x;
        let kind = match cell_kind(g, i) {
            LAND if !tree(g, trunks, i) => LAND,
            WATER if Self::water_block(g, x, y) => WATER,
            SHORE => SHORE,
            _ => return false,
        };
        let own = self.regions[id as usize].kind;
        if kind == SHORE {
            if self.starts.len() < self.side * 64 {
                self.starts.push([x as u16, y as u16]);
            }
            let r = &mut self.regions[id as usize];
            r.shore = r.shore.wrapping_add(1);
        }
        let r = self.cells[i];
        if r != NONE {
            if r == WIPED || r == id {
                return false;
            }
            if self.regions[r as usize].kind != own {
                self.link(r, id);
            }
            if kind == SHORE {
                self.cells[i] = id;
            }
            return false;
        }
        if kind != SHORE && kind != own {
            return false;
        }
        self.cells[i] = id;
        if kind == SHORE {
            return false;
        }
        self.regions[id as usize].cells += 1;
        true
    }
    /// 0x46a5f0: the regions touch.
    fn link(&mut self, a: u8, b: u8) {
        let at = pair(a, b);
        if self.links[at] & link::ADJACENT != 0 {
            return;
        }
        self.links[at] |= link::ADJACENT;
        self.count(a, b, true);
    }
    /// 0x46a690.
    fn unlink(&mut self, a: u8, b: u8) {
        let at = pair(a, b);
        if self.links[at] & link::ADJACENT == 0 {
            return;
        }
        self.links[at] &= !link::ADJACENT;
        self.count(a, b, false);
    }
    fn count(&mut self, a: u8, b: u8, up: bool) {
        let step = |n: &mut u16| {
            *n = if up {
                n.wrapping_add(1)
            } else {
                n.wrapping_sub(1)
            }
        };
        for (x, other) in [(b, a), (a, b)] {
            let land = self.regions[other as usize].kind == LAND;
            let r = &mut self.regions[x as usize];
            step(if land {
                &mut r.land_links
            } else {
                &mut r.other_links
            });
        }
    }
    /// 0x469f80: two regions touching a same one get `BY_WATER`.
    fn by_water(&mut self) {
        let next = self.next;
        for a in 1..next {
            let mut b = 1u8;
            while (b as u16) + 1 < next as u16 {
                if b != a && self.links(b, a) & link::ADJACENT != 0 {
                    for c in b + 1..next {
                        if c != a && self.links(c, a) & link::ADJACENT != 0 {
                            self.links[pair(b, c)] |= link::BY_WATER;
                        }
                    }
                }
                b += 1;
            }
        }
    }
    /// 0x46aff0: the first region on the border of a rect, of a cell of
    /// `kind` when given: the top and bottom cells column by column, then
    /// the left and right cells row by row. Callers grow the rect ring by
    /// ring.
    pub fn nearest(&self, g: &MapGrid, r: [i32; 4], kind: u8) -> u8 {
        let side = self.side as i32;
        let at = |x: i32, y: i32| {
            let i = (y * side + x) as usize;
            if kind != 0 && cell_kind(g, i) != kind {
                return NONE;
            }
            match self.cells[i] {
                WIPED => NONE,
                c => c,
            }
        };
        for x in r[0].max(0)..=r[2].min(side - 1) {
            if r[1] >= 0 {
                let c = at(x, r[1]);
                if c != NONE {
                    return c;
                }
            }
            if r[3] < side {
                let c = at(x, r[3]);
                if c != NONE {
                    return c;
                }
            }
        }
        for y in (r[1] + 1).max(0)..r[3].min(side) {
            if r[0] >= 0 {
                let c = at(r[0], y);
                if c != NONE {
                    return c;
                }
            }
            if r[2] < side {
                let c = at(r[2], y);
                if c != NONE {
                    return c;
                }
            }
        }
        NONE
    }
    /// The ring round a rect, as 0x46a0e0 walks it: the top row, the right
    /// column, the bottom row and the left column, each corner once.
    fn ring(&self, r: [i32; 4]) -> Vec<[i32; 2]> {
        let side = self.side as i32;
        let mut out = Vec::new();
        if r[1] > 0 {
            for x in (r[0] - 1).max(0)..(r[2] + 1).min(side) {
                out.push([x, r[1] - 1]);
            }
        }
        if r[2] + 1 < side {
            for y in (r[1] - 1).max(0)..(r[3] + 1).min(side) {
                out.push([r[2] + 1, y]);
            }
        }
        if r[3] + 1 < side {
            for x in r[0].max(0)..=(r[2] + 1).min(side - 1) {
                out.push([x, r[3] + 1]);
            }
        }
        if r[0] > 0 {
            for y in r[1].max(0)..=(r[3] + 1).min(side - 1) {
                out.push([r[0] - 1, y]);
            }
        }
        out
    }
    /// 0x46a0e0, for a felled tree (0x468a60) or a mine gone (0x468ab0):
    /// the rect takes the land region with most cells on the ring round it
    /// (none when there is none) and links it to every other region there,
    /// land too, without merging them.
    pub fn opened(&mut self, r: [i32; 4]) {
        let ring = self.ring(r);
        let mut best = NONE;
        let mut most = 0;
        for &p in &ring {
            let c = self.region(p);
            if c == NONE || c == WIPED || c == best || self.kind(c) != LAND {
                continue;
            }
            let cells = self.regions[c as usize].cells;
            if best == NONE || cells > most {
                best = c;
                most = cells;
            }
        }
        for y in r[1]..=r[3] {
            for x in r[0]..=r[2] {
                if x >= 0 && y >= 0 && (x as usize) < self.side && (y as usize) < self.side {
                    self.cells[y as usize * self.side + x as usize] = best;
                }
            }
        }
        if best == NONE {
            return;
        }
        self.clear_caches();
        for &p in &ring {
            let c = self.region(p);
            if c != NONE && c != WIPED && c != best {
                self.link(best, c);
            }
        }
    }
    pub fn clear_caches(&mut self) {
        self.pairs.clear();
        self.rects.clear();
    }
    /// 0x468ae0: the land regions at the two ends of a bridge, on the rows
    /// above and below it (north–south) or the columns beside it.
    fn bridge_ends(&self, g: &MapGrid, f: [i32; 4], north_south: bool) -> [u8; 2] {
        let ring = [f[0] - 1, f[1] - 1, f[2] + 1, f[3] + 1];
        let (a, b) = if north_south {
            (
                [ring[0], ring[1], ring[2], ring[1]],
                [ring[0], ring[3], ring[2], ring[3]],
            )
        } else {
            (
                [ring[0], ring[1], ring[0], ring[3]],
                [ring[2], ring[1], ring[2], ring[3]],
            )
        };
        [self.nearest(g, a, LAND), self.nearest(g, b, LAND)]
    }
    /// 0x468b70: at the start of the game and when a bridge is finished
    /// (0x41c3f0).
    pub fn bridge_built(&mut self, g: &MapGrid, lib: &ObjectLibrary, o: &MapObject, i: usize) {
        let north_south = o.data.get(56).copied().unwrap_or(0) != 0;
        self.bridge_at(g, movement::footprint_rect(lib, o), north_south, i);
    }
    fn bridge_at(&mut self, g: &MapGrid, footprint: [i32; 4], north_south: bool, i: usize) {
        let [a, b] = self.bridge_ends(g, footprint, north_south);
        if a == NONE || b == NONE || a == b {
            return;
        }
        self.bridges.push(Bridge {
            object: i,
            ends: [a, b],
        });
        for r in [a, b] {
            let x = &mut self.regions[r as usize];
            x.bridges = x.bridges.wrapping_add(1);
        }
        let at = pair(a, b);
        if self.links[at] & link::BRIDGE == 0 {
            self.links[at] |= link::BRIDGE;
            for r in [a, b] {
                let x = &mut self.regions[r as usize];
                x.bridged = x.bridged.wrapping_add(1);
            }
            self.clear_caches();
        }
    }
    /// 0x468c80, when a bridge object goes (0x41bec0): the pair keeps its
    /// bit while another bridge joins the same ends, in the same order.
    pub fn bridge_removed(&mut self, i: usize) {
        let Some(k) = self.bridges.iter().position(|b| b.object == i) else {
            return;
        };
        let [a, b] = self.bridges.remove(k).ends;
        for r in [a, b] {
            let x = &mut self.regions[r as usize];
            x.bridges = x.bridges.saturating_sub(1);
        }
        let (ra, rb) = (self.regions[a as usize], self.regions[b as usize]);
        if ra.bridges >= ra.bridged
            && rb.bridges >= rb.bridged
            && self.bridges.iter().any(|x| x.ends == [a, b])
        {
            return;
        }
        self.links[pair(a, b)] &= !link::BRIDGE;
        for r in [a, b] {
            let x = &mut self.regions[r as usize];
            x.bridged = x.bridged.saturating_sub(1);
        }
        self.clear_caches();
    }
    /// 0x468db0: two land regions joined by land or bridges.
    pub fn same_island(&mut self, a: u8, b: u8) -> bool {
        if matches!(a, NONE | WIPED) || matches!(b, NONE | WIPED) {
            return false;
        }
        if a == b {
            return true;
        }
        if self.kind(a) != self.kind(b) || self.kind(a) != LAND {
            return false;
        }
        if let Some(answer) = self.pairs.get((a, b)) {
            return answer;
        }
        let mut seen = vec![false; 256];
        let answer = self.by_land(&mut seen, a, b);
        self.pairs.put((a, b), answer);
        answer
    }
    /// 0x468ec0: the region of the point and one along the border of `r`
    /// (columns first, the top and the bottom cell, then the rows between,
    /// the left and the right cell; a side asks only when its region
    /// changes) are on one island. The answer is remembered.
    pub fn point_island(&mut self, p: [i32; 2], r: [i32; 4]) -> bool {
        let here = self.region(p);
        if matches!(here, NONE | WIPED) {
            return false;
        }
        if let Some(answer) = self.rects.get((here, r)) {
            return answer;
        }
        let side = self.side as i32;
        let mut last = [NONE; 2];
        let mut cells = Vec::new();
        for x in r[0].max(0)..=r[2].min(side - 1) {
            if r[1] >= 0 {
                cells.push((0, [x, r[1]]));
            }
            if r[3] < side {
                cells.push((1, [x, r[3]]));
            }
        }
        for y in (r[1] + 1).max(0)..r[3].min(side) {
            if r[0] >= 0 {
                cells.push((0, [r[0], y]));
            }
            if r[2] < side {
                cells.push((1, [r[2], y]));
            }
        }
        let mut answer = false;
        for (at, cell) in cells {
            let c = self.region(cell);
            if c == last[at] {
                continue;
            }
            last[at] = c;
            if self.same_island(c, here) {
                answer = true;
                break;
            }
        }
        self.rects.put((here, r), answer);
        answer
    }
    /// 0x469130: a cell along the border of `a` is on one island with a
    /// region along the border of `b` (0x468ec0).
    pub fn rects_island(&mut self, a: [i32; 4], b: [i32; 4]) -> bool {
        let side = self.side as i32;
        for x in a[0].max(0)..=a[2].min(side - 1) {
            if a[1] >= 0 && self.point_island([x, a[1]], b) {
                return true;
            }
            if a[3] < side && self.point_island([x, a[3]], b) {
                return true;
            }
        }
        for y in (a[1] + 1).max(0)..a[3].min(side) {
            if a[0] >= 0 && self.point_island([a[0], y], b) {
                return true;
            }
            if a[2] < side && self.point_island([a[2], y], b) {
                return true;
            }
        }
        false
    }
    /// 0x4697a0.
    fn by_land(&self, seen: &mut [bool], a: u8, b: u8) -> bool {
        seen[a as usize] = true;
        for r in 1..self.next {
            if r == a || self.links(r, a) & (link::ADJACENT | link::BRIDGE) == 0 {
                continue;
            }
            if r == b {
                return true;
            }
            if !seen[r as usize] && self.kind(r) == LAND && self.by_land(seen, r, b) {
                return true;
            }
        }
        false
    }
}

/// What a step saw of an object.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Seen {
    pub present: bool,
    pub built: bool,
}
fn seen(o: &MapObject) -> Seen {
    Seen {
        // An exhausted mine frees its cells (0x41df30 → 0x469410).
        present: o.flags & (FLAG_INACTIVE | crate::world::FLAG_DEAD) == 0,
        built: o.flags & FLAG_CONSTRUCTION == 0,
    }
}

/// The map built at the first step of a game (0x468950 at the start, or
/// for a save made before it existed); then, once a step, what changed
/// since the step before: a bridge finished (0x41c3f0), a bridge or a mine
/// gone (0x41bec0, 0x4aff10).
pub(crate) fn watch(sim: &mut Simulation, m: &MissionWorld, g: &MapGrid, lib: &ObjectLibrary) {
    let gp = &mut sim.gameplay;
    if !gp.islands.built() {
        gp.islands = Islands::build(g, m, lib);
        gp.census = m.objects.iter().map(seen).collect();
        return;
    }
    gp.census.resize(m.objects.len(), Seen::default());
    for (i, o) in m.objects.iter().enumerate() {
        let now = seen(o);
        let before = std::mem::replace(&mut gp.census[i], now);
        if now == before || !before.present {
            continue;
        }
        match o.class {
            class::BRIDGE if now.present && now.built && !before.built => {
                gp.islands.bridge_built(g, lib, o, i)
            }
            class::BRIDGE if !now.present => gp.islands.bridge_removed(i),
            class::RES_GOLD | class::RES_STONE if !now.present => {
                gp.islands.opened(movement::footprint_rect(lib, o))
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "islands_tests.rs"]
mod tests;
