//! A computer player's buildings: where they go (0x40a990, the search
//! 0x40a050 with the engine's flood 0x44fbb0 and its test 0x40a360, the
//! spacing 0x40b300 and the free land 0x469530, the safety 0x411a30), who
//! builds them (0x40cec0) and their sites, laid at once or, with the builder
//! far, still to be laid while it walks there (0x40b610, «AI far build»
//! 0x19 and its arrival 0x40b9e0); and the orders it gives through the map
//! of islands (0x40bab0).
use super::rules::World;
use super::think::group;
use super::*;
use crate::gameplay::pathing::{self, Cells};

/// The first run of a search, and each later one every 100 ms (0x40a221,
/// 0x40a2d6, 0x44fd29).
const FIRST_RUN: u32 = 512;
const NEXT_RUN: u32 = 200;
pub const RUN_PERIOD: u64 = 100;
/// A builder farther than this walks to a site still to be laid (0x40b754);
/// it is laid within as much of where it was meant (0x40ba20).
const FAR: u32 = 10;
/// The marker between two rings of the flood (0x8000).
const LEVEL: i32 = i32::MIN;

/// A site still to be laid (an object with 0x200000, never on the map): its
/// builder walks to it; it counts with its kingdom's type and among the
/// places' arrivals.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Virtual {
    pub owner: u8,
    pub class: u16,
    pub at: [u16; 2],
    pub size: [u16; 2],
    pub sector: [u16; 2],
    pub category: u8,
    pub strength: u32,
}
impl Virtual {
    pub fn rect(&self) -> [i32; 4] {
        let [x, y] = self.at.map(|v| v as i32);
        [
            x,
            y,
            x + self.size[0] as i32 - 1,
            y + self.size[1] as i32 - 1,
        ]
    }
}

/// A site that was placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    Real(usize),
    Virtual(u32),
}

/// The engine's flood of 0x44fbb0 (0x44fdd0, 0x44ffc0, 0x44fea0): breadth
/// first over the eight neighbours inside its bounds, in a ring of
/// 4 × side cells that drops what does not fit; a cell is tested once.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Flood {
    pub start: [i32; 2],
    pub bounds: [i32; 4],
    pub queue: VecDeque<[i32; 2]>,
    pub capacity: usize,
    /// One bit a cell of the bounds.
    pub seen: Vec<u8>,
    pub level: i32,
}
impl Flood {
    pub(crate) fn new(start: [i32; 2], bounds: [i32; 4], side: usize) -> Self {
        let cells = ((bounds[2] - bounds[0] + 1) * (bounds[3] - bounds[1] + 1)).max(0) as usize;
        let mut f = Self {
            start,
            bounds,
            capacity: 4 * side,
            seen: vec![0; cells / 8 + 1],
            ..Default::default()
        };
        f.push(start);
        f.push([LEVEL, 0]);
        f
    }
    /// 0x44fea0.
    fn push(&mut self, p: [i32; 2]) {
        if self.queue.len() >= self.capacity {
            return;
        }
        if p[0] != LEVEL {
            let b = self.bounds;
            if p[0] < b[0] || p[1] < b[1] || p[0] > b[2] || p[1] > b[3] {
                return;
            }
            let i = ((p[1] - b[1]) * (b[2] - b[0] + 1) + p[0] - b[0]) as usize;
            if self.seen[i / 8] & (1 << (i % 8)) != 0 {
                return;
            }
            self.seen[i / 8] |= 1 << (i % 8);
        }
        self.queue.push_back(p);
    }
    /// 0x44ffc0: up to `budget` cells (and ring markers) tested; whether
    /// some are left, and the cell that stopped the flood.
    pub(super) fn run(
        &mut self,
        budget: u32,
        mut test: impl FnMut([i32; 2], &mut bool) -> bool,
    ) -> (bool, Option<[i32; 2]>) {
        self.run_levels(budget, |p, _, stop| test(p, stop))
    }
    /// `run`, the test also told the depth of the cell (+0x46).
    pub(crate) fn run_levels(
        &mut self,
        budget: u32,
        mut test: impl FnMut([i32; 2], i32, &mut bool) -> bool,
    ) -> (bool, Option<[i32; 2]>) {
        for _ in 0..budget {
            let Some(p) = self.queue.pop_front() else {
                return (false, None);
            };
            if p[0] == LEVEL {
                if self.queue.is_empty() {
                    return (false, None);
                }
                self.level += 1;
                self.push([LEVEL, self.level]);
                continue;
            }
            let mut stop = false;
            if test(p, self.level, &mut stop) || p == self.start {
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
                    self.push([p[0] + dx, p[1] + dy]);
                }
            }
            if stop {
                return (false, Some(p));
            }
        }
        (true, None)
    }
}

/// A search for a building's site (the 0x24-byte record of 0x40a050 and its
/// flood).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Search {
    pub class: u16,
    pub size: [i32; 2],
    pub min: u32,
    pub max: u32,
    /// What the distance is taken from: the mine of a storage, the castle
    /// at the home, else the centre (+0x14).
    pub reference: Option<[i32; 4]>,
    /// Where its builder comes from (+0x18).
    pub from: [i32; 2],
    /// Placed at a far builder's arrival (0x5af410): no price committed.
    pub hint: bool,
    /// Still running after its first 512 cells (+0x20).
    pub running: bool,
    /// Whether a cell a land unit may stand on was met (+0x1e).
    pub ground: bool,
    pub flood: Flood,
    /// When it runs again (message 0x2203), and its place in the queue.
    pub due: u64,
    pub seq: u64,
}

/// The cells as they stand, built again after an object is laid.
pub(super) fn cells<'a>(w: &'a mut World) -> &'a Cells {
    if w.cells.is_none() {
        let fresh = if w.sim.gameplay.pathing.cells.side == w.g.side {
            w.sim.gameplay.pathing.cells.clone()
        } else {
            movement::cells_now(w.sim, w.m, w.g, w.lib)
        };
        w.cells = Some(fresh);
    }
    w.cells.as_ref().unwrap()
}

/// 0x401b30: the empty cells between two rects, each side chosen by the
/// centres; 0 when they meet.
pub fn rect_gap(a: [i32; 4], b: [i32; 4]) -> u32 {
    let centre = |lo: i32, hi: i32| (lo + hi) / 2;
    let dx = if centre(a[0], a[2]) < centre(b[0], b[2]) {
        b[0] - a[2] - 1
    } else {
        a[0] - b[2] - 1
    };
    let dy = if centre(a[1], a[3]) < centre(b[1], b[3]) {
        b[1] - a[3] - 1
    } else {
        a[1] - b[3] - 1
    };
    dx.max(dy).max(0) as u32
}
fn rect_of(w: &World, i: usize) -> [i32; 4] {
    movement::footprint_rect(w.lib, &w.m.objects[i])
}
fn grown(r: [i32; 4]) -> [i32; 4] {
    [r[0] - 1, r[1] - 1, r[2] + 1, r[3] + 1]
}

/// The store of a resource carried (0x40d460): storage for gold and stone,
/// farm for food, lumber mill for wood.
pub(super) fn store_of(resource: usize) -> usize {
    match resource {
        0 | 1 => class::STORAGE,
        2 => class::FARM,
        _ => class::LUMBER,
    }
}
/// A peasant the player may take for other work (0x40a990, 0x40cec0): not
/// in the first six blows on its tree, nor digging, nor building, nor on
/// its way to a castle or to its resource's store with a load (those three
/// only outside the Build phase); a farmer of a field still growing is
/// kept; a milker is always free. Never one inside a holder (0x400000).
pub(super) fn free(w: &World, p: &Player, i: usize) -> bool {
    let o = &w.m.objects[i];
    if !alive(o) || o.flags & (FLAG_STORY | FLAG_DYING) != 0 || w.sim.objects[i].holder.is_some() {
        return false;
    }
    let a = &w.sim.gameplay.actors[i];
    let kind = program(w.sim, w.m, i).kind;
    let loaded = a.cargo.iter().any(|&n| n > 0);
    match kind {
        program::BUILD | program::AI_FAR_BUILD => return false,
        program::TEND_COWS => return true,
        program::CHOP => {
            let blows = matches!(a.intent, Some(Intent::Chop(t)) if a.work_cell == Some(t));
            if blows && !loaded && !p.building {
                return false;
            }
        }
        program::DIG => {
            let field = p.lists.peons.get(&i).and_then(|x| x.target);
            let digging = matches!(a.intent, Some(Intent::Harvest(j)) if a.work_target == Some(j));
            if digging && !loaded && !p.building {
                return false;
            }
            if let Some(f) = field.and_then(|f| w.m.objects.get(f))
                && f.health < f.max_health
            {
                return false;
            }
        }
        _ => {}
    }
    if loaded && !p.building {
        // The store of its load's kind (+0x120): milk and picked food have
        // none but the castle (0x40faf0).
        let store_class = match carry_type(a) {
            Some(r @ 0..=3) => Some(store_of(r as usize)),
            _ => None,
        };
        if let Some(store) = a.depositing.and_then(|j| w.m.objects.get(j))
            && (store.class == class::CASTLE || Some(store.class) == store_class)
        {
            return false;
        }
    }
    true
}

/// 0x469530 over the free runs of 0x469230: every row of the rect starts
/// with a cell of land without a standing tree or a building that has as
/// many such cells after it as the rect is wide (so one more column than
/// the rect itself must be free).
fn free_land(cells: &Cells, r: [i32; 4]) -> bool {
    let side = cells.side as i32;
    let width = r[2] - r[0] + 1;
    let open = |x: i32, y: i32| {
        let word = cells.at([x, y]);
        let tree = (word >> 11) & 31 > cells.trunks && word & pathing::FELLED == 0;
        word & (3 << 17) == pathing::LAND && word & pathing::STATIC == 0 && !tree
    };
    (r[1]..=r[3]).all(|y| {
        r[0] >= 0
            && y >= 0
            && r[0] < side
            && y < side
            && open(r[0], y)
            && (1..=width).all(|d| r[0] + d < side && open(r[0] + d, y))
    })
}

/// 0x40b300: the margin round the footprint is free land (2 cells; 1 for
/// barracks, cathedral, academy, mage tower, guild, house, Shaolin,
/// mosque and dummy; 0 for a field); towers, lumber mills and storages
/// keep 8 cells from the player's own; a lumber mill in the Build phase
/// wants 10 standing trees within 8.
fn spacing(w: &mut World, p: &Player, kind: u16, x: i32, y: i32, size: [i32; 2]) -> bool {
    let k = kind as usize;
    let (margin, apart): (i32, Option<&[usize]>) = match k {
        39..=42 => (1, Some(&p.lists.towers)),
        class::LUMBER => (1, Some(&p.lists.lumber)),
        class::STORAGE => (1, Some(&p.lists.storages)),
        class::FIELD => (0, None),
        class::BARRACK | 55 | 56 | 60 | 65 | 66 | 70 | 71 | class::DUMMY => (1, None),
        _ => (2, None),
    };
    let apart: Vec<usize> = apart.map_or(Vec::new(), |l| l.to_vec());
    let last = w.g.side as i32 - 1;
    let r = [
        (x - margin).max(0),
        (y - margin).max(0),
        (x + size[0] - 1 + margin).min(last),
        (y + size[1] - 1 + margin).min(last),
    ];
    if !free_land(cells(w), r) {
        return false;
    }
    for j in apart {
        if rect_gap(r, rect_of(w, j)) < 8 {
            return false;
        }
    }
    if p.building && k == class::LUMBER {
        let around = [
            (x - 8).max(0),
            (y - 8).max(0),
            (x + size[0] - 1 + 8).min(last),
            (y + size[1] - 1 + 8).min(last),
        ];
        let cells = cells(w);
        let mut trees = 0;
        for cx in around[0]..=around[2] {
            for cy in around[1]..=around[3] {
                let word = cells.at([cx, cy]);
                let image = (word >> 11) & 31;
                if image > cells.trunks && word & pathing::FELLED == 0 && image != 0 {
                    trees += 1;
                    if trees >= 10 {
                        return true;
                    }
                }
            }
        }
        return false;
    }
    true
}

/// The player's record of a place, as the place lists it (+0x22).
fn record_of<'a>(w: &World, p: &'a Player, slot: usize) -> Option<&'a records::Record> {
    let mgr = w.sim.gameplay.ai.manager.as_ref()?;
    let serial = mgr.place(slot)?.serial;
    p.records
        .iter()
        .find(|r| r.place == slot && r.serial == serial)
}

/// 0x411a30: a rect is safe unless a place of its sector has more hostile
/// strength (fighters and towers) than the player has fighters there. Then
/// outside its own home with hostile fighters it is not; else it is not
/// when a fighter, or a finished tower, of a kingdom not allied sees it
/// (0x411dd0) from a sector within two.
pub(super) fn safe(w: &World, p: &Player, x: i32, y: i32, size: [i32; 2]) -> bool {
    use places::mask;
    let last = w.g.side as i32 - 1;
    let (x, y) = (x.clamp(0, last), y.clamp(0, last));
    let Some(mgr) = w.sim.gameplay.ai.manager.as_ref() else {
        return true;
    };
    let sector = [x / 8, y / 8];
    let (mut threatened, mut fighters, mut towers, mut home) = (false, false, false, false);
    for &slot in mgr.sector_places(sector) {
        let Some(place) = mgr.place(slot) else {
            continue;
        };
        if let Some(r) = record_of(w, p, slot) {
            let own = r.friendly.count(mask::MILITARY) as u16;
            if r.hostile.strength(mask::MILITARY | mask::TOWERS) > own as u32 {
                threatened = true;
            }
            fighters |= r.hostile.count(mask::MILITARY) as u16 != 0;
            towers |= r.hostile.count(mask::TOWERS) as u16 != 0;
        }
        if place.kind == places::kind::HOME && place.param == p.kingdom as u32 {
            home = true;
        }
    }
    if !threatened {
        return true;
    }
    if !home && fighters {
        return false;
    }
    if !fighters && !towers {
        return true;
    }
    let rect = [x, y, x + size[0] - 1, y + size[1] - 1];
    let n = mgr.sectors_side as i32;
    for sy in (sector[1] - 2).max(0)..n.min(sector[1] + 3) {
        for sx in (sector[0] - 2).max(0)..n.min(sector[0] + 3) {
            if !sector_safe(w, p, [sx, sy], rect) {
                return false;
            }
        }
    }
    true
}
/// 0x411dd0: no fighter, nor finished tower, of a kingdom not allied in
/// the sector has the rect within its sight (+0x6a).
fn sector_safe(w: &World, p: &Player, s: [i32; 2], rect: [i32; 4]) -> bool {
    for (j, o) in w.m.objects.iter().enumerate() {
        if !alive(o) || w.sim.gameplay.actors[j].sector != Some([s[0] as u16, s[1] as u16]) {
            continue;
        }
        if relation(w.m, p.kingdom, o.owner) & 0x40 != 0 {
            continue;
        }
        let armed = if crate::selection::unit(o) {
            crate::selection::combat(o)
        } else {
            (39..=43).contains(&o.class) && o.flags & FLAG_CONSTRUCTION == 0
        };
        if armed && rect_gap(rect_of(w, j), rect) <= w.sim.objects[j].sight as u32 {
            return false;
        }
    }
    true
}
/// 0x411d50: an object's rect grown by a cell is safe.
fn safe_object(w: &World, p: &Player, i: usize) -> bool {
    let last = w.g.side as i32 - 1;
    let r = grown(rect_of(w, i));
    let r = [r[0].max(0), r[1].max(0), r[2].min(last), r[3].min(last)];
    safe(w, p, r[0], r[1], [r[2] - r[0] + 1, r[3] - r[1] + 1])
}

/// 0x417640 from the player.
pub(super) fn reach(w: &mut World, p: &mut Player, from: [i32; 2], rect: [i32; 4]) -> bool {
    reach::point(w.sim, w.m, w.lib, w.c, p, from, rect) != reach::NO
}

/// 0x40cec0: who builds a site: the peasant at its arrival, else the free
/// idle peasant nearest it if within 10; else, from the least wanted
/// resource, the free gatherer nearest it if within two thirds of that;
/// else that idle one. Each must reach the site.
fn builder(w: &mut World, p: &mut Player, site: [i32; 4], prefer: Option<usize>) -> Option<usize> {
    if prefer.is_some() {
        return prefer;
    }
    let around = grown(site);
    let mut best: Option<(u32, usize)> = None;
    for i in p.lists.groups[group::IDLE as usize].clone() {
        if !free(w, p, i) {
            continue;
        }
        let o = &w.m.objects[i];
        if !reach(w, p, [o.x as i32, o.y as i32], around) {
            continue;
        }
        let d = rect_gap(rect_of(w, i), site);
        if best.is_none_or(|(b, _)| d < b) {
            best = Some((d, i));
        }
    }
    if let Some((d, i)) = best
        && d < FAR
    {
        return Some(i);
    }
    let limit = best.map_or(u32::MAX, |(d, _)| ((2 * d as u64) / 3) as u32);
    for at in (0..4).rev() {
        let r = p.order.0[at] as usize;
        let mut near: Option<(u32, usize)> = None;
        for i in p.lists.groups[group::GOLD as usize + r].clone() {
            if !free(w, p, i) {
                continue;
            }
            let o = &w.m.objects[i];
            if !reach(w, p, [o.x as i32, o.y as i32], around) {
                continue;
            }
            let d = rect_gap(rect_of(w, i), site);
            if near.is_none_or(|(b, _)| d < b) {
                near = Some((d, i));
            }
        }
        if let Some((d, i)) = near
            && d <= limit
        {
            return Some(i);
        }
    }
    best.map(|(_, i)| i)
}

/// 0x40bab0: an order of the player to a unit, at priority 4000: the unit
/// may move again, and the order starts when its destination is on the
/// unit's island; across the water a ship of the player carries it
/// (0x415f80). Whether it started.
pub(super) fn order(
    w: &mut World,
    p: &mut Player,
    i: usize,
    intent: Intent,
    point: [i32; 2],
    target: Option<usize>,
) -> bool {
    let a = &mut w.sim.gameplay.actors[i];
    a.stance = 0;
    a.temporary_hold = false;
    let side = w.g.side;
    let isl = &w.sim.gameplay.islands;
    let land_at = |p: [i32; 2]| {
        let i = p[1] as usize * side + p[0] as usize;
        w.g.cells
            .get(i)
            .is_some_and(|_| islands::cell_kind(w.g, i) == islands::LAND)
    };
    let dest = match target {
        Some(j) => {
            let o = &w.m.objects[j];
            let at = [o.x as i32, o.y as i32];
            let r = isl.region(at);
            if !matches!(r, islands::NONE | islands::WIPED) && land_at(at) {
                r
            } else {
                isl.nearest(w.g, grown(rect_of(w, j)), islands::LAND)
            }
        }
        None => {
            let r = isl.region(point);
            if !matches!(r, islands::NONE | islands::WIPED) && isl.kind(r) == islands::LAND {
                r
            } else {
                isl.nearest(
                    w.g,
                    [point[0] - 1, point[1] - 1, point[0] + 1, point[1] + 1],
                    islands::LAND,
                )
            }
        }
    };
    // The order is kept (+0xf2) while 0x415f80 sees to the way, from the
    // target's cell if any; once landed, a unit walks on to it unless the
    // order is a group's Approach or an idle fighter's pick-up (the last
    // argument, 0 for the war and at 0x414237).
    let at = match target {
        Some(j) => [w.m.objects[j].x, w.m.objects[j].y],
        None => [point[0].max(0) as u16, point[1].max(0) as u16],
    };
    let walk = !matches!(intent, Intent::Approach(_) | Intent::Pick(_));
    match super::transport::organise(w, p, i, &intent, at, dest, walk) {
        0 => set_order(w.sim, i, intent, false).is_ok(),
        1 => false,
        // No way: the order stays kept, for when the program under way
        // ends (0x4e4c14).
        _ => {
            w.sim.gameplay.actors[i].resume = Some(intent);
            false
        }
    }
}

/// The strength and category a site of `class` would count with
/// (0x401fa0 on the record the site is made from).
fn weight(w: &World, k: u8, civ: u8, class: usize) -> (u8, u32) {
    let mut ids = BTreeSet::new();
    let Ok(probe) = world::starting::new_object(&mut ids, 1, class, k, civ, [0, 0], w.lib) else {
        return (places::category::BUILDINGS as u8, 0);
    };
    let [attack, defence] = base_stats(w.m, w.lib, &probe, None);
    let range = w.c.stats(&probe, CLASSES[class], researches(w.m, k)).range;
    let mut s = attack as u64 + defence as u64 + range as u64 / 5 + probe.max_health as u64 / 20;
    if (39..=43).contains(&class) {
        s *= 2;
    }
    let category = match class {
        class::BARRACK => places::category::BARRACKS,
        39..=43 => places::category::TOWERS,
        _ => places::category::BUILDINGS,
    };
    (category as u8, s.min(u32::MAX as u64) as u32)
}

impl Player {
    /// 0x40a990: a building of `kind` between `min` and `max` of `center`:
    /// an abandoned site of its type taken up again by a free peasant; with
    /// no peasant, or none free, nothing; else a search for its site. The
    /// builder `prefer` (0x5af40c) is taken if given; at a far builder's
    /// arrival (`hint`, 0x5af410) no price is committed.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn place(
        &mut self,
        w: &mut World,
        kind: u16,
        center: [i32; 2],
        min: u32,
        max: u32,
        prefer: Option<usize>,
        hint: bool,
    ) -> Option<Site> {
        if !rules::available(w, self, kind) {
            return None;
        }
        for s in self.lists.sites.clone() {
            let o = &w.m.objects[s];
            if o.flags & FLAG_CONSTRUCTION == 0 || o.class != kind as usize {
                continue;
            }
            if o.class != class::HARBOR {
                let d = records::gap(center, rect_of(w, s));
                if d < min || d > max {
                    continue;
                }
            }
            let Some(b) = builder(w, self, rect_of(w, s), prefer) else {
                continue;
            };
            if safe_object(w, self, s) {
                let o = &w.m.objects[s];
                self.command(w, b, Intent::Build(s), [o.x as i32, o.y as i32], Some(s));
                return Some(Site::Real(s));
            }
        }
        if rules::record(w.sim, w.m, &w.c.ai, self.kingdom, class::PEON as u16).0 == 0 {
            return None;
        }
        let some = self.lists.groups[group::IDLE as usize]
            .iter()
            .chain(
                self.lists.groups[group::GOLD as usize..=group::WOOD as usize]
                    .iter()
                    .flatten(),
            )
            .any(|&i| free(w, self, i));
        if !some {
            return None;
        }
        self.search(w, kind, center, min, max, prefer, hint)
    }

    /// 0x40a050: the search for a site round `center`, 512 cells at once;
    /// unfinished, it goes on 200 cells every 100 ms, one at a time for a
    /// player, its price committed meanwhile.
    #[allow(clippy::too_many_arguments)]
    fn search(
        &mut self,
        w: &mut World,
        kind: u16,
        center: [i32; 2],
        min: u32,
        max: u32,
        prefer: Option<usize>,
        hint: bool,
    ) -> Option<Site> {
        // 0x40a098: a harbor's site is on the coast (0x40a710).
        if kind as usize == class::HARBOR {
            let p = self.harbor_spot(w, kind, center)?;
            return self.lay(w, kind, p, hint, prefer);
        }
        let probe = super::tree::probe(kind as usize, self.civilization);
        let size = footprint(w.lib, &probe).map(|n| n as i32);
        let home = self.home.unwrap_or_default();
        let reference = if kind as usize == class::STORAGE {
            object_at(w, center, |o| {
                matches!(o.class, class::RES_GOLD | class::RES_STONE)
            })
        } else if center == home {
            let k = self.kingdom;
            object_at(w, [home[0] + 2, home[1] + 2], |o| {
                o.class == class::CASTLE && o.owner == k
            })
        } else {
            None
        };
        let from = match prefer {
            Some(i) => [w.m.objects[i].x as i32, w.m.objects[i].y as i32],
            None => home,
        };
        let last = w.g.side as i32 - 1;
        let bounds = [
            (center[0] - size[0] - max as i32).max(0),
            (center[1] - size[1] - max as i32).max(0),
            (center[0] + max as i32).min(last),
            (center[1] + max as i32).min(last),
        ];
        let mut s = Search {
            class: kind,
            size,
            min,
            max,
            reference: reference.map(|j| rect_of(w, j)),
            from,
            hint,
            flood: Flood::new(center, bounds, w.g.side),
            ..Default::default()
        };
        let (more, found) = self.run(w, &mut s, FIRST_RUN);
        if !more {
            return self.finish(w, s, found, prefer);
        }
        if self.search.is_some() {
            // One search at a time: the new one ends with no site.
            return None;
        }
        s.running = true;
        self.mode |= 0x8000;
        if !s.hint {
            let c = rules::cost(w, self, kind);
            rules::commit(self, c);
        }
        let seq = w.sim.gameplay.ai.next_seq();
        s.due = w.sim.ticks * TICK_MS + RUN_PERIOD;
        s.seq = seq;
        self.search = Some(s);
        None
    }
    /// Message 0x2203: the search goes on, or ends.
    pub(super) fn search_on(&mut self, w: &mut World) {
        let Some(mut s) = self.search.take() else {
            return;
        };
        let (more, found) = self.run(w, &mut s, NEXT_RUN);
        if more {
            s.due += RUN_PERIOD;
            s.seq = w.sim.gameplay.ai.next_seq();
            self.search = Some(s);
            return;
        }
        self.finish(w, s, found, None);
    }
    fn run(&mut self, w: &mut World, s: &mut Search, budget: u32) -> (bool, Option<[i32; 2]>) {
        let mut flood = std::mem::take(&mut s.flood);
        // The test reads the start while the flood is out of the search.
        let start = flood.start;
        let out = flood.run(budget, |p, stop| self.test(w, s, start, p, stop));
        s.flood = flood;
        out
    }
    /// 0x40a360: a cell of the search. It stops the search when its rect is
    /// within the distance, with its spacing, a site there, reachable from
    /// where the builder comes and safe. The flood spreads over cells a
    /// land unit may stand on, everywhere until it meets one.
    fn test(
        &mut self,
        w: &mut World,
        s: &mut Search,
        start: [i32; 2],
        p: [i32; 2],
        stop: &mut bool,
    ) -> bool {
        let rect = [p[0], p[1], p[0] + s.size[0] - 1, p[1] + s.size[1] - 1];
        let d = match s.reference {
            Some(r) => rect_gap(r, rect),
            None => records::gap(start, rect),
        };
        *stop = d >= s.min && d <= s.max;
        if *stop && !spacing(w, self, s.class, p[0], p[1], s.size) {
            *stop = false;
        }
        if *stop {
            let mut probe = super::tree::probe(s.class as usize, self.civilization);
            probe.owner = self.kingdom;
            probe.x = p[0] as u16;
            probe.y = p[1] as u16;
            *stop = site_test(w.sim, w.m, w.g, w.lib, &probe);
        }
        if *stop && !reach(w, self, s.from, rect) {
            *stop = false;
        }
        if *stop && !safe(w, self, p[0], p[1], s.size) {
            *stop = false;
        }
        if cells(w).free(1, p) {
            s.ground = true;
            true
        } else {
            !s.ground
        }
    }
    /// 0x40a610: the search ends: its committed price is given back; the
    /// cell found gets its site (0x40b610), else nothing ("No spot").
    fn finish(
        &mut self,
        w: &mut World,
        s: Search,
        found: Option<[i32; 2]>,
        prefer: Option<usize>,
    ) -> Option<Site> {
        if s.running {
            if !s.hint {
                let c = rules::cost(w, self, s.class);
                rules::release(self, c);
            }
            self.mode &= 0x7fff;
        }
        let p = found?;
        self.lay(w, s.class, p, s.hint, prefer)
    }

    /// 0x40b610: the site at `p`. The price must be there; the builder is
    /// chosen; within 10 cells on its island (or at a far builder's
    /// arrival, or for a field) the site is laid and paid at once, else it
    /// is still to be laid: its price committed, it counts with the
    /// kingdom's type and among the places' arrivals while the builder
    /// walks to it («AI far build»).
    fn lay(
        &mut self,
        w: &mut World,
        kind: u16,
        p: [i32; 2],
        hint: bool,
        prefer: Option<usize>,
    ) -> Option<Site> {
        self.available = rules::spendable(w.m, self);
        let cost = rules::cost(w, self, kind);
        if matches!(kind as usize, class::CASTLE | class::LUMBER) {
            rules::start_reserve(w, self, false);
        }
        if !rules::affordable(w, self, cost, rules::bucket::NONE, 1, false) {
            return None;
        }
        let probe = super::tree::probe(kind as usize, self.civilization);
        let size = footprint(w.lib, &probe).map(|n| n as i32);
        let rect = [p[0], p[1], p[0] + size[0] - 1, p[1] + size[1] - 1];
        let b = builder(w, self, rect, prefer)?;
        let mut d = rect_gap(rect_of(w, b), rect);
        if d <= FAR
            && !w
                .sim
                .gameplay
                .islands
                .rects_island(grown(rect_of(w, b)), grown(rect))
        {
            d = w.g.side as u32;
        }
        if !hint && kind as usize != class::FIELD && d > FAR {
            rules::commit(self, cost);
            let id = w.sim.gameplay.ai.next_virtual();
            let last = w.g.side as i32 - 1;
            let (category, strength) = weight(w, self.kingdom, self.civilization, kind as usize);
            let v = Virtual {
                owner: self.kingdom,
                class: kind,
                at: [p[0] as u16, p[1] as u16],
                size: [size[0] as u16, size[1] as u16],
                sector: [
                    (p[0].clamp(0, last) / 8) as u16,
                    (p[1].clamp(0, last) / 8) as u16,
                ],
                category,
                strength,
            };
            w.sim.gameplay.ai.virtuals.insert(id, v);
            places::virtual_site(w.sim, w.m, w.lib, w.c, id, true);
            self.touch(rules::Watched::Type(kind));
            self.command(w, b, Intent::FarBuild(id), p, None);
            return matches!(w.sim.gameplay.actors[b].intent, Some(Intent::FarBuild(x)) if x == id)
                .then_some(Site::Virtual(id));
        }
        let by = w.m.objects[b].clone();
        let mut by = by;
        by.civilization = self.civilization;
        let site = lay_site(
            w.sim,
            w.m,
            w.lib,
            w.c,
            &by,
            kind as usize,
            [p[0] as u16, p[1] as u16],
        )
        .ok()?;
        // The site's cells are taken at once (0x4fd4c0), for what the player
        // looks at next in this step.
        w.cells = Some(movement::cells_now(w.sim, w.m, w.g, w.lib));
        self.appeared(w, site);
        if kind as usize != class::FIELD {
            self.command(w, b, Intent::Build(site), p, Some(site));
        }
        if !hint {
            for (a, c) in self.available.iter_mut().zip(cost) {
                *a = a.wrapping_sub(c);
            }
        }
        matches!(w.sim.gameplay.actors[b].intent, Some(Intent::Build(x)) if x == site)
            .then_some(Site::Real(site))
    }

    /// 0x40b9e0: a far builder arrived: the site is laid within 10 cells of
    /// where it was meant, the builder preferred, nothing more committed;
    /// the other builders of the site still to be laid go to it.
    fn arrived(&mut self, w: &mut World, peasant: usize, id: u32) {
        let Some(v) = w.sim.gameplay.ai.virtuals.get(&id).copied() else {
            return;
        };
        let at = [v.at[0] as i32, v.at[1] as i32];
        let Some(Site::Real(site)) = self.place(w, v.class, at, 0, FAR, Some(peasant), true) else {
            return;
        };
        for b in self.lists.groups[group::BUILDERS as usize].clone() {
            if b != peasant && self.lists.peons.get(&b).is_some_and(|x| x.site == Some(id)) {
                let o = &w.m.objects[site];
                self.command(
                    w,
                    b,
                    Intent::Build(site),
                    [o.x as i32, o.y as i32],
                    Some(site),
                );
            }
        }
    }
}

/// 0x44e400: an object on the cell (any of its footprint) that passes the
/// filter.
fn object_at(w: &World, p: [i32; 2], filter: impl Fn(&MapObject) -> bool) -> Option<usize> {
    (0..w.m.objects.len()).find(|&j| {
        let o = &w.m.objects[j];
        if !alive(o) || !filter(o) {
            return false;
        }
        let r = rect_of(w, j);
        p[0] >= r[0] && p[0] <= r[2] && p[1] >= r[1] && p[1] <= r[3]
    })
}

/// The rect a far builder walks to (the site's, grown by a cell).
pub(crate) fn far_rect(sim: &Simulation, id: u32) -> Option<[i32; 4]> {
    sim.gameplay.ai.virtuals.get(&id).map(|v| grown(v.rect()))
}

/// Op 0x3a of «AI far build» (0x4ac95d): the builder is at its site.
pub(crate) fn far_arrival(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    peasant: usize,
    id: u32,
) {
    let k = m.objects[peasant].owner;
    let Some(mut p) = sim.gameplay.ai.players.get_mut(&k).map(std::mem::take) else {
        return;
    };
    if let Some(slot) = sim.gameplay.ai.players.get_mut(&k) {
        slot.mode = p.mode;
    }
    let mut w = World {
        sim,
        m,
        g,
        lib,
        c,
        cells: None,
    };
    if p.home.is_some() {
        p.arrived(&mut w, peasant, id);
    }
    if let Some(slot) = w.sim.gameplay.ai.players.get_mut(&k) {
        *slot = p;
    }
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
