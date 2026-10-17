//! A computer player's look at the world every period (message 0x4201,
//! 0x407c20): the home it keeps (0x407690), the lists of its objects
//! (0x407110, 0x40c720, 0x40c810, 0x40be30) that the engine's events keep
//! up to date (0x408670, 0x408730, 0x408920), and its five phases in turn
//! (0x408d80).
use super::rules::{Other, Watched, World};
use super::*;

/// The period by difficulty (0x5aa0e8 from 0x581050).
const PERIODS: [u64; 3] = [2000, 1500, 1000];
pub fn period(m: &MissionWorld) -> u64 {
    PERIODS[m.game_rules.difficulty.min(2)]
}
/// A home island smaller than this and on the water wants a ship
/// (0x4078c1).
const SMALL_ISLAND: u32 = 3000;

/// The groups of peasants (+0x9a6 idle, +0x9aa builders, +0x9ae milkers,
/// +0x9b2 the gatherers of gold, stone, food in the fields and wood).
pub mod group {
    pub const IDLE: u8 = 0;
    pub const BUILDERS: u8 = 1;
    pub const MILKERS: u8 = 2;
    pub const GOLD: u8 = 3;
    pub const STONE: u8 = 4;
    pub const FOOD: u8 = 5;
    pub const WOOD: u8 = 6;
}

/// The objects of a kingdom's list (K+0x56 the units, K+0x52 the rest),
/// the latest to join it first (0x467450, 0x507940).
pub fn kingdom_list(sim: &Simulation, m: &MissionWorld, k: u8, units: bool) -> Vec<usize> {
    let mut out: Vec<usize> = (0..m.objects.len())
        .filter(|&i| {
            let o = &m.objects[i];
            o.owner == k
                && alive(o)
                && if units {
                    crate::selection::unit(o)
                } else {
                    o.building()
                }
        })
        .collect();
    out.sort_by_key(|&i| {
        std::cmp::Reverse((sim.gameplay.actors.get(i).map_or(0, |a| a.joined), i))
    });
    out
}

/// What the hooks saw of an object at the step before: a unit or a
/// building on the map, its owner, whether it is finished and a peasant's
/// program (its kind and the order that gave it).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Seen {
    pub owner: u8,
    pub class: u16,
    pub built: bool,
    pub program: (u8, u32),
}
pub(super) fn seen(sim: &Simulation, m: &MissionWorld, i: usize) -> Option<Seen> {
    let o = &m.objects[i];
    let mine = matches!(o.class, class::RES_GOLD | class::RES_STONE);
    if !alive(o) || !(crate::selection::unit(o) || o.building() || mine) {
        return None;
    }
    let program = if o.class == class::PEON {
        (program(sim, m, i).kind, sim.gameplay.actors[i].orders)
    } else {
        (0, 0)
    };
    Some(Seen {
        owner: o.owner,
        class: o.class as u16,
        built: o.flags & FLAG_CONSTRUCTION == 0,
        program,
    })
}
#[derive(Clone, Copy, Debug)]
enum Event {
    /// 0x402a70 → 0x408670.
    Added(usize),
    /// 0x402c50 → 0x408730.
    Removed(usize, Seen),
    /// A peasant's new program (0x4ae5e0 → 0x402e70 → 0x408920).
    Program(usize),
}

/// A peasant's work as its player sorted it: its group, the object it
/// works (+0x146), whether it is counted on its field's entry (+0x148) and
/// the rect of its work (+0x13e).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Peon {
    pub group: u8,
    pub target: Option<usize>,
    /// The site still to be laid it walks to.
    pub site: Option<u32>,
    pub field: bool,
    pub rect: [i32; 4],
}
/// An entry of the mines (+0xc bit 0: worked) or of the fields (+0xc: its
/// farmers).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub object: usize,
    pub count: u32,
}

/// The lists of a computer player, each in the native order: appended
/// ones oldest first, the groups of peasants and the ships newest first.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Lists {
    /// +0x9c2: every peasant.
    pub peasants: Vec<usize>,
    /// The groups of `group`.
    pub groups: [Vec<usize>; 7],
    pub peons: BTreeMap<usize, Peon>,
    /// +0x9c6 castles, +0x9ca abandoned sites, +0x9ce mages, +0x9e6
    /// storages, +0x9ea lumber mills, +0x9ee farms.
    pub castles: Vec<usize>,
    /// The castles and lumber mills with no tree within 8 when last looked
    /// at (+0xc of their entries), never looked at again.
    pub bare: Vec<usize>,
    pub sites: Vec<usize>,
    pub mages: Vec<usize>,
    pub storages: Vec<usize>,
    pub lumber: Vec<usize>,
    pub farms: Vec<usize>,
    /// +0x9f2: the mines, nearest the home first.
    pub mines: Vec<Entry>,
    /// +0x9f6: the kingdom's fields.
    pub fields: Vec<Entry>,
    /// +0x1a10 towers, +0x1a14 outposts, +0x1a18 workshops, +0x1a1c
    /// training dummies, +0x1a28 fishing boats.
    pub towers: Vec<usize>,
    pub outposts: Vec<usize>,
    pub workshops: Vec<usize>,
    pub dummies: Vec<usize>,
    pub fishing: Vec<usize>,
    /// +0xa0e: the fighters (0x10000).
    pub army: Vec<usize>,
    /// +0x1a2c: the harbors on each water.
    pub harbors: BTreeMap<u8, Vec<usize>>,
}
/// 0x40ce60: appended unless already there.
fn add(list: &mut Vec<usize>, i: usize) {
    if !list.contains(&i) {
        list.push(i);
    }
}
/// 0x52e9b0 after a search: the first one goes.
fn take(list: &mut Vec<usize>, i: usize) {
    if let Some(at) = list.iter().position(|&x| x == i) {
        list.remove(at);
    }
}

/// The default program, at priority 4000 (vt+0x178, vt+0x160).
pub(super) fn restart(sim: &mut Simulation, i: usize) {
    let a = &mut sim.gameplay.actors[i];
    a.intent = None;
    a.queued.clear();
    a.automatic = false;
    a.resume = None;
    halt(a);
    a.depositing = None;
    a.search_at = None;
    a.scan_due_ms = None;
    sim.objects[i].order = OrderState::Idle;
}

/// vt+0x134: the object's rect grown by a cell.
fn around(lib: &ObjectLibrary, o: &MapObject) -> [i32; 4] {
    let r = movement::footprint_rect(lib, o);
    [r[0] - 1, r[1] - 1, r[2] + 1, r[3] + 1]
}
/// The water of a ship or harbor (0x416110, 0x4162f0): its cell's if it is
/// water, else the first water on the border of its rect grown by one.
pub(super) fn water_of(w: &World, i: usize) -> u8 {
    let o = &w.m.objects[i];
    let isl = &w.sim.gameplay.islands;
    let p = [o.x as i32, o.y as i32];
    let r = isl.region(p);
    let cell = p[1] as usize * w.g.side + p[0] as usize;
    if r != islands::NONE
        && r != islands::WIPED
        && w.g
            .cells
            .get(cell)
            .is_some_and(|_| islands::cell_kind(w.g, cell) == islands::WATER)
    {
        return r;
    }
    isl.nearest(w.g, around(w.lib, o), islands::WATER)
}

impl Player {
    /// 0x40c810: an object of the kingdom put in its list.
    fn classify(&mut self, w: &mut World, i: usize) {
        let o = &w.m.objects[i];
        if o.building() && o.flags & FLAG_CONSTRUCTION == 0 {
            take(&mut self.lists.sites, i);
        }
        let l = &mut self.lists;
        match o.class {
            class::PEON => {
                l.peasants.push(i);
                self.sort_peasant(w, i);
                return;
            }
            class::MAGE => l.mages.push(i),
            class::CASTLE => return add(&mut l.castles, i),
            class::LUMBER => return add(&mut l.lumber, i),
            class::FARM => return add(&mut l.farms, i),
            class::STORAGE => return add(&mut l.storages, i),
            class::OUTPOST => return add(&mut l.outposts, i),
            class::WORKSHOP => return add(&mut l.workshops, i),
            class::DUMMY => return add(&mut l.dummies, i),
            class::FISHING_BOAT => return l.fishing.push(i),
            class::HARBOR => {
                let water = water_of(w, i);
                return add(self.lists.harbors.entry(water).or_default(), i);
            }
            _ => {}
        }
        let o = &w.m.objects[i];
        if crate::selection::unit(o) {
            if crate::selection::combat(o) {
                self.lists.army.push(i);
            }
            if (23..=25).contains(&o.class) {
                let water = water_of(w, i);
                self.ships.entry(water).or_default().insert(0, i);
            }
        } else if (39..=43).contains(&o.class) {
            add(&mut self.lists.towers, i);
        }
    }
    /// 0x40cb50: an object of the kingdom out of its lists.
    fn unclassify(&mut self, w: &mut World, i: usize, class: usize) {
        take(&mut self.lists.sites, i);
        let l = &mut self.lists;
        match class {
            class::PEON => {
                self.leave(w, i);
                return take(&mut self.lists.peasants, i);
            }
            class::HARBOR => {
                let water = water_of(w, i);
                if let Some(list) = self.lists.harbors.get_mut(&water) {
                    take(list, i);
                }
                return;
            }
            class::CASTLE => {
                take(&mut l.bare, i);
                return take(&mut l.castles, i);
            }
            class::LUMBER => {
                take(&mut l.bare, i);
                return take(&mut l.lumber, i);
            }
            class::STORAGE => return take(&mut l.storages, i),
            class::FARM => return take(&mut l.farms, i),
            class::FISHING_BOAT => return take(&mut l.fishing, i),
            class::OUTPOST => return take(&mut l.outposts, i),
            class::WORKSHOP => return take(&mut l.workshops, i),
            class::DUMMY => return take(&mut l.dummies, i),
            class::MAGE => take(&mut l.mages, i),
            _ => {}
        }
        if world::class::is_unit(class) {
            take(&mut self.lists.army, i);
            if (23..=25).contains(&class) {
                // The ship is looked for on the water it is on now.
                let water = water_of(w, i);
                if let Some(list) = self.ships.get_mut(&water) {
                    take(list, i);
                }
            }
        } else if (39..=43).contains(&class) {
            take(&mut self.lists.towers, i);
        }
    }

    /// 0x40bc80: a peasant leaves its group. A field loses a farmer; a site
    /// left by its last builder is left alone: not begun, it dies; begun,
    /// it waits among the abandoned sites.
    fn leave(&mut self, w: &mut World, i: usize) {
        let Some(peon) = self.lists.peons.remove(&i) else {
            return;
        };
        match peon.group {
            group::FOOD if peon.field => {
                if let Some(f) = peon.target
                    && let Some(e) = self.lists.fields.iter_mut().find(|e| e.object == f)
                {
                    e.count = e.count.wrapping_sub(1);
                }
            }
            group::BUILDERS if peon.site.is_some() => {
                let id = peon.site.unwrap_or_default();
                let alone = !self.lists.groups[group::BUILDERS as usize]
                    .iter()
                    .any(|&b| {
                        b != i && self.lists.peons.get(&b).is_some_and(|x| x.site == Some(id))
                    });
                if alone && let Some(v) = w.sim.gameplay.ai.virtuals.get(&id).copied() {
                    // 0x40bd43: its price is given back and it goes.
                    let c = rules::cost(w, self, v.class);
                    rules::release(self, c);
                    places::virtual_site(w.sim, w.m, w.lib, w.c, id, false);
                    w.sim.gameplay.ai.virtuals.remove(&id);
                    self.touch(Watched::Type(v.class));
                    if matches!(v.class as usize, class::CASTLE | class::LUMBER) {
                        rules::start_reserve(w, self, true);
                    }
                }
            }
            group::BUILDERS => {
                if let Some(site) = peon.target.filter(|&s| alive(&w.m.objects[s]))
                    && !self.lists.groups[group::BUILDERS as usize]
                        .iter()
                        .any(|&b| {
                            b != i
                                && self
                                    .lists
                                    .peons
                                    .get(&b)
                                    .is_some_and(|x| x.target == Some(site))
                        })
                    && w.m.objects[site].owner == w.m.objects[i].owner
                {
                    let a = &w.sim.gameplay.actors[site];
                    if a.construction == a.construction_total {
                        // 0x42df20: the Die program.
                        refund_site(w.sim, w.m, w.c, site);
                        kill(w.sim, w.m, w.g, w.lib, site);
                    } else if w.m.objects[site].flags & FLAG_CONSTRUCTION != 0 {
                        add(&mut self.lists.sites, site);
                    }
                }
            }
            _ => {}
        }
        take(&mut self.lists.groups[peon.group as usize], i);
    }

    /// An order of the player (0x40bab0) and, when the unit's program
    /// changed, the player's hook on it at once (`hooked`).
    pub(super) fn command(
        &mut self,
        w: &mut World,
        i: usize,
        intent: Intent,
        point: [i32; 2],
        target: Option<usize>,
    ) -> bool {
        let done = super::build::order(w, self, i, intent, point, target);
        self.hooked(w, i);
        done
    }
    /// The hook on a new program (0x4ae5e0 → 0x402e70 → 0x408920) comes at
    /// once for a program the player itself gives or ends: its peasant is
    /// sorted again, and the census keeps the program as seen.
    pub(super) fn hooked(&mut self, w: &mut World, i: usize) {
        let Some(now) = seen(w.sim, w.m, i) else {
            return;
        };
        let Some(Some(was)) = w.sim.gameplay.ai.census.get_mut(i) else {
            return;
        };
        if was.program == now.program {
            return;
        }
        was.program = now.program;
        if self.home.is_some() && now.owner == self.kingdom && now.class as usize == class::PEON {
            self.sort_peasant(w, i);
        }
    }

    /// An object of the kingdom replaced by one of another class (op 0x5d):
    /// the hooks of its removal and of the new one come at once.
    pub(super) fn replaced(&mut self, w: &mut World, i: usize, was: Seen) {
        let k = self.kingdom;
        self.removed(w, k, i, was);
        let now = seen(w.sim, w.m, i);
        if let Some(slot) = w.sim.gameplay.ai.census.get_mut(i) {
            *slot = now;
        }
        if now.is_some() {
            self.added(w, k, i);
        }
    }
    /// The hook on a new object (0x402a70 → 0x408670) comes at once for one
    /// the player itself lays.
    pub(super) fn appeared(&mut self, w: &mut World, i: usize) {
        let Some(now) = seen(w.sim, w.m, i) else {
            return;
        };
        let census = &mut w.sim.gameplay.ai.census;
        if census.len() <= i {
            census.resize(i + 1, None);
        }
        if census[i].is_some() {
            return;
        }
        census[i] = Some(now);
        let k = self.kingdom;
        self.added(w, k, i);
    }

    /// 0x40be30: a peasant sorted by its program.
    pub(super) fn sort_peasant(&mut self, w: &mut World, i: usize) {
        self.leave(w, i);
        let o = &w.m.objects[i];
        let own = movement::footprint_rect(w.lib, o);
        let prog = program(w.sim, w.m, i);
        let target = match &w.sim.gameplay.actors[i].intent {
            Some(Intent::Harvest(j) | Intent::Build(j)) => Some(*j),
            _ => None,
        };
        let rect_of = |j: usize| movement::footprint_rect(w.lib, &w.m.objects[j]);
        let peon = match prog.kind {
            program::IDLE => Some(Peon {
                group: group::IDLE,
                rect: own,
                ..Default::default()
            }),
            program::AI_FAR_BUILD => match &w.sim.gameplay.actors[i].intent {
                Some(Intent::FarBuild(id)) => w.sim.gameplay.ai.virtuals.get(id).map(|v| Peon {
                    group: group::BUILDERS,
                    site: Some(*id),
                    rect: v.rect(),
                    ..Default::default()
                }),
                _ => None,
            },
            program::BUILD => Some(
                match target.filter(|&j| {
                    let s = &w.m.objects[j];
                    s.building() && alive(s) && s.flags & FLAG_CONSTRUCTION != 0
                }) {
                    Some(j) => Peon {
                        group: group::BUILDERS,
                        target: Some(j),
                        rect: rect_of(j),
                        ..Default::default()
                    },
                    None => Peon {
                        group: group::IDLE,
                        rect: own,
                        ..Default::default()
                    },
                },
            ),
            program::CHOP => match &w.sim.gameplay.actors[i].intent {
                Some(Intent::Chop(p)) => Some(Peon {
                    group: group::WOOD,
                    rect: [p[0] as i32, p[1] as i32, p[0] as i32, p[1] as i32],
                    ..Default::default()
                }),
                _ => None,
            },
            program::DIG => {
                target.map(
                    |j| match self.lists.fields.iter_mut().find(|e| e.object == j) {
                        Some(e) if e.count < 1 => {
                            e.count += 1;
                            Peon {
                                group: group::FOOD,
                                target: Some(j),
                                field: true,
                                rect: rect_of(j),
                                site: None,
                            }
                        }
                        _ => Peon {
                            group: group::IDLE,
                            rect: own,
                            ..Default::default()
                        },
                    },
                )
            }
            program::MINE => target
                .filter(|&j| matches!(w.m.objects[j].class, class::RES_GOLD | class::RES_STONE))
                .map(|j| {
                    if let Some(e) = self.lists.mines.iter_mut().find(|e| e.object == j) {
                        e.count |= 1;
                    }
                    Peon {
                        group: if w.m.objects[j].class == class::RES_GOLD {
                            group::GOLD
                        } else {
                            group::STONE
                        },
                        target: Some(j),
                        rect: rect_of(j),
                        ..Default::default()
                    }
                }),
            program::TEND_COWS => target.map(|j| Peon {
                group: group::MILKERS,
                target: Some(j),
                rect: rect_of(j),
                ..Default::default()
            }),
            _ => None,
        };
        if let Some(peon) = peon {
            self.lists.groups[peon.group as usize].insert(0, i);
            self.lists.peons.insert(i, peon);
        }
    }

    /// 0x408670: an object came: a field of the kingdom joins its list; an
    /// object of the kingdom has its type's rules evaluated again and is
    /// put in its list.
    fn added(&mut self, w: &mut World, k: u8, i: usize) {
        if self.home.is_none() {
            return;
        }
        let o = &w.m.objects[i];
        if o.class == class::FIELD && o.owner == k {
            if !self.lists.fields.iter().any(|e| e.object == i) {
                self.lists.fields.push(Entry {
                    object: i,
                    count: 0,
                });
            }
            self.touch(Watched::Type(class::FIELD as u16));
        }
        // 0x4086ce: anything but a field (or a site still to be laid) may
        // call the militia.
        if w.m.objects[i].class != class::FIELD {
            self.alarm(w, i);
        }
        let o = &w.m.objects[i];
        if o.owner == k {
            self.touch(Watched::Type(o.class as u16));
            if crate::selection::unit(o) {
                self.touch(Watched::Type(0));
            }
            self.classify(w, i);
        }
    }
    /// 0x408730: an object went (or changed owner): a mine leaves the list,
    /// a field its farmers; a site its builders; an object of the kingdom
    /// its lists.
    fn removed(&mut self, w: &mut World, k: u8, i: usize, was: Seen) {
        if self.home.is_none() {
            return;
        }
        let class = was.class as usize;
        if matches!(class, class::RES_GOLD | class::RES_STONE)
            && let Some(at) = self.lists.mines.iter().position(|e| e.object == i)
        {
            self.lists.mines.remove(at);
        }
        if class == class::FIELD
            && was.owner == k
            && let Some(at) = self.lists.fields.iter().position(|e| e.object == i)
        {
            if self.lists.fields[at].count != 0 {
                for b in self.lists.groups[group::FOOD as usize].clone() {
                    if self
                        .lists
                        .peons
                        .get(&b)
                        .is_some_and(|x| x.field && x.target == Some(i))
                    {
                        if let Some(x) = self.lists.peons.get_mut(&b) {
                            x.field = false;
                        }
                        restart(w.sim, b);
                        self.hooked(w, b);
                    }
                }
            }
            self.lists.fields.remove(at);
        }
        if was.owner != k {
            return;
        }
        if world::class::is_unit(class) {
        } else if !was.built {
            for b in self.lists.groups[group::BUILDERS as usize].clone() {
                if alive(&w.m.objects[b])
                    && self
                        .lists
                        .peons
                        .get(&b)
                        .is_some_and(|x| x.target == Some(i))
                {
                    restart(w.sim, b);
                    self.hooked(w, b);
                }
            }
        }
        self.touch(Watched::Type(class as u16));
        if world::class::is_unit(class) {
            self.touch(Watched::Type(0));
        }
        if matches!(class, class::CASTLE | class::LUMBER) {
            rules::start_reserve(w, self, true);
        }
        self.unclassify(w, i, class);
    }

    /// 0x407690: the home: a castle's centre, of the castle nearest the
    /// buildings' centroid when there are several; else the building (or
    /// the unit) nearest the centroid of all, looked for again every period
    /// while there is no building. Then its island, and a ship wanted on
    /// the largest region beside a small coastal one.
    fn find_home(&mut self, w: &World, k: u8) {
        self.homeless = true;
        let buildings = kingdom_list(w.sim, w.m, k, false);
        let units = kingdom_list(w.sim, w.m, k, true);
        let total = (buildings.len() + units.len()) as u32;
        if total == 0 {
            return;
        }
        let gap = super::records::gap;
        let rect = |i: usize| movement::footprint_rect(w.lib, &w.m.objects[i]);
        let at = |i: usize| [w.m.objects[i].x as i32, w.m.objects[i].y as i32];
        let (mut sx, mut sy) = (0u32, 0u32);
        let mut castles = 0;
        let mut castle = None;
        for &i in &buildings {
            let [x, y] = at(i);
            sx = sx.wrapping_add(x as u32);
            sy = sy.wrapping_add(y as u32);
            if w.m.objects[i].class == class::CASTLE {
                castles += 1;
                castle = Some(i);
            }
        }
        if castles > 1 {
            let n = buildings.len() as u32;
            let mid = [(sx / n) as u16 as i32, (sy / n) as u16 as i32];
            castle = None;
            let mut best = 0;
            for &i in &buildings {
                if w.m.objects[i].class != class::CASTLE {
                    continue;
                }
                let d = gap(mid, rect(i));
                if castle.is_none() || d < best {
                    castle = Some(i);
                    best = d;
                }
            }
        }
        let home = if let Some(c) = castle {
            let [wd, ht] = footprint(w.lib, &w.m.objects[c]).map(|n| n as i32);
            let [x, y] = at(c);
            self.homeless = false;
            [x + wd / 2, y + ht / 2]
        } else {
            for &i in &units {
                let [x, y] = at(i);
                sx = sx.wrapping_add(x as u32);
                sy = sy.wrapping_add(y as u32);
            }
            let mid = [(sx / total) as u16 as i32, (sy / total) as u16 as i32];
            let list = if buildings.is_empty() {
                &units
            } else {
                &buildings
            };
            let mut near = list[0];
            let mut best = gap(mid, rect(near));
            for &i in &list[1..] {
                let d = gap(mid, rect(i));
                if d < best {
                    near = i;
                    best = d;
                }
            }
            self.homeless = buildings.is_empty();
            at(near)
        };
        self.home = Some(home);
        let isl = &w.sim.gameplay.islands;
        let mut r = [home[0], home[1], home[0], home[1]];
        self.island = islands::NONE;
        for _ in 0..5 {
            let found = isl.nearest(w.g, r, islands::LAND);
            if found != islands::NONE && found != islands::WIPED {
                self.island = found;
                break;
            }
            r = [r[0] - 1, r[1] - 1, r[2] + 1, r[3] + 1];
        }
        let island = self.island;
        if island != islands::NONE
            && let Some(region) = isl.regions.get(island as usize)
            && region.cells < SMALL_ISLAND
            && region.other_links != 0
        {
            let mut best: Option<(u8, u32)> = None;
            for other in 1..isl.next {
                if isl.links(island, other) & islands::link::ADJACENT == 0 {
                    continue;
                }
                let cells = isl.regions.get(other as usize).map_or(0, |x| x.cells);
                if best.is_none_or(|(_, c)| c < cells) {
                    best = Some((other, cells));
                }
            }
            if let Some((water, _)) = best {
                let wanted = self.wanted_ships.entry(water).or_default();
                if *wanted == 0 {
                    *wanted = 1;
                }
            }
        }
    }

    /// 0x40db60: the mines, nearest the home first, and the kingdom's
    /// fields. With `fresh`, the search for a wood starts again from the
    /// home.
    fn list_mines(&mut self, w: &World, k: u8, fresh: bool) {
        let home = self.home.unwrap_or_default();
        if fresh {
            let f = &mut self.forest;
            (f.radius, f.saved, f.at, f.wood) = (0, 0, home, None);
        }
        let gap = super::records::gap;
        let rect = |i: usize| movement::footprint_rect(w.lib, &w.m.objects[i]);
        self.lists.mines.clear();
        // The game's list of mines (0xc8b0ac), the newest first.
        for i in (0..w.m.objects.len()).rev() {
            let o = &w.m.objects[i];
            if !matches!(o.class, class::RES_GOLD | class::RES_STONE) || !alive(o) {
                continue;
            }
            let entry = Entry {
                object: i,
                count: 0,
            };
            let mines = &mut self.lists.mines;
            if mines.is_empty() {
                mines.insert(0, entry);
                continue;
            }
            let d = gap(home, rect(i));
            if gap(home, rect(mines[0].object)) > d {
                mines.insert(0, entry);
                continue;
            }
            let mut at = 0;
            while at + 1 < mines.len() && gap(home, rect(mines[at + 1].object)) < d {
                at += 1;
            }
            mines.insert(at + 1, entry);
        }
        // The game's list of fields (0x5e4924), the newest first.
        for i in (0..w.m.objects.len()).rev() {
            let o = &w.m.objects[i];
            if o.class == class::FIELD && o.owner == k && alive(o) {
                self.lists.fields.push(Entry {
                    object: i,
                    count: 0,
                });
            }
        }
    }

    /// 0x407110: the player takes its kingdom in hand. Its lists are made
    /// (0x40c720), every unit may move again, its records are evaluated
    /// and sorted, and the start's reserve is committed.
    fn take_over(&mut self, w: &mut World, k: u8) {
        self.list_mines(w, k, true);
        self.forest_start(w);
        let l = &mut self.lists;
        for g in &mut l.groups {
            g.clear();
        }
        l.peasants.clear();
        l.peons.clear();
        for i in kingdom_list(w.sim, w.m, k, true) {
            self.classify(w, i);
            let a = &mut w.sim.gameplay.actors[i];
            a.stance = 0;
            a.temporary_hold = false;
        }
        for i in kingdom_list(w.sim, w.m, k, false) {
            self.classify(w, i);
        }
        if let Some(mgr) = w.sim.gameplay.ai.manager.take() {
            for at in 0..self.records.len() {
                super::records::evaluate(self, k, at, &mgr, w.m, mgr.side);
            }
            self.records.sort_by_key(|r| std::cmp::Reverse(r.priority));
            w.sim.gameplay.ai.manager = Some(mgr);
        }
        rules::start_reserve(w, self, true);
    }

    /// 0x408d80: one phase, in turn.
    fn think(&mut self, w: &mut World, k: u8, others: &BTreeMap<u8, Other>) {
        match self.phase {
            0 if self.mode & 1 != 0 => rules::build(w, self, k, others),
            1 if self.mode & 1 != 0 => self.harvest(w),
            2 if self.mode & 1 != 0 => self.storages(w),
            3 if self.mode & 2 != 0 => self.war(w),
            4 => self.special(w),
            _ => {}
        }
        self.phase = (self.phase + 1) % 5;
    }

    /// Message 0x4201 (0x407c20).
    fn look(&mut self, w: &mut World, k: u8, others: &BTreeMap<u8, Other>) {
        let first = self.home.is_none();
        if first || self.homeless {
            self.find_home(w, k);
            if first && self.home.is_some() {
                self.take_over(w, k);
            }
        }
        if self.home.is_some() {
            self.think(w, k, others);
        }
    }
}

/// Once a step, after the places: the engine's events passed to the
/// players, the researches done, then the players whose period came.
pub(crate) fn watch(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
) {
    // The militias called by what happened before the units (missiles).
    if !sim.gameplay.ai.alarms.is_empty() {
        super::war::alarms(sim, m, g, lib, c);
    }
    let n = m.objects.len();
    let mut census = std::mem::take(&mut sim.gameplay.ai.census);
    census.resize(n, None);
    let mut events = Vec::new();
    for (i, slot) in census.iter_mut().enumerate() {
        let now = seen(sim, m, i);
        match (*slot, now) {
            (None, None) => {}
            (Some(b), None) => events.push(Event::Removed(i, b)),
            (None, Some(_)) => events.push(Event::Added(i)),
            (Some(b), Some(t)) => {
                // A new owner, or a new class (the object replaced, op 0x5d),
                // is a removal and an arrival.
                if b.owner != t.owner || b.class != t.class {
                    events.push(Event::Removed(i, b));
                    events.push(Event::Added(i));
                } else {
                    if t.built && !b.built {
                        events.push(Event::Added(i));
                    }
                    if t.program != b.program {
                        events.push(Event::Program(i));
                    }
                }
            }
        }
        *slot = now;
    }
    sim.gameplay.ai.census = census;
    if sim.gameplay.ai.manager.is_none() {
        return;
    }
    let kingdoms: Vec<u8> = sim.gameplay.ai.players.keys().copied().collect();
    let mut w = World {
        sim,
        m,
        g,
        lib,
        c,
        cells: None,
    };
    for e in events {
        // 0x402c50 begins with the marks of a wall that falls (0x417190).
        if let Event::Removed(i, _) = e {
            super::breach::fallen(w.sim, w.m, w.lib, i);
        }
        for &k in &kingdoms {
            let Some(mut p) = w.sim.gameplay.ai.players.get_mut(&k).map(std::mem::take) else {
                continue;
            };
            match e {
                Event::Added(i) => p.added(&mut w, k, i),
                Event::Removed(i, b) => p.removed(&mut w, k, i, b),
                Event::Program(i) => {
                    if p.home.is_some()
                        && w.m.objects[i].owner == k
                        && w.m.objects[i].class == class::PEON
                    {
                        p.sort_peasant(&mut w, i);
                    }
                }
            }
            if let Some(slot) = w.sim.gameplay.ai.players.get_mut(&k) {
                *slot = p;
            }
        }
    }
    // 0x41eed0: a research done has the rules reading it evaluated again.
    for &k in &kingdoms {
        let done = researches(w.m, k).len();
        let Some(p) = w.sim.gameplay.ai.players.get_mut(&k) else {
            continue;
        };
        let before = std::mem::replace(&mut p.researched, done).min(done);
        let names: Vec<String> = researches(w.m, k)[before..].to_vec();
        for name in names {
            if let Some(kind) =
                c.ai.types
                    .iter()
                    .position(|t| t.eq_ignore_ascii_case(&name))
            {
                p.touch(Watched::Type(kind as u16));
            }
        }
    }
    let now = w.sim.ticks * TICK_MS;
    let period = period(w.m);
    // A player made after the start receives the places and looks at once
    // (0x402440).
    for &k in &kingdoms {
        let Some(p) = w.sim.gameplay.ai.players.get(&k) else {
            continue;
        };
        if p.due.is_some() {
            continue;
        }
        let mgr = w.sim.gameplay.ai.manager.take();
        if let (Some(mgr), Some(p)) = (mgr.as_ref(), w.sim.gameplay.ai.players.get_mut(&k)) {
            for slot in 0..mgr.places.len() {
                if let Some(place) = mgr.place(slot) {
                    super::records::register(p, k, slot, place.serial, mgr, w.m, mgr.side);
                }
            }
            p.due = Some(now);
        }
        if let Some(p) = w.sim.gameplay.ai.players.get_mut(&k) {
            p.seq = w.sim.gameplay.ai.sequence + 1;
            w.sim.gameplay.ai.sequence += 1;
        }
        w.sim.gameplay.ai.manager = mgr;
    }
    // The queue (0xc97c54): every message due, by time and by posting: the
    // looks (0x4201), the searches for a site (0x2203) and for a wood
    // (0x2202).
    const LOOK: u8 = 0;
    const SITE: u8 = 1;
    const WOOD: u8 = 2;
    loop {
        let next = w
            .sim
            .gameplay
            .ai
            .players
            .iter()
            .flat_map(|(&k, p)| {
                let look = p.due.map(|d| (d, p.seq, k, LOOK));
                let site = p.search.as_ref().map(|s| (s.due, s.seq, k, SITE));
                let wood = p.forest.due.map(|d| (d, p.forest.seq, k, WOOD));
                look.into_iter().chain(site).chain(wood)
            })
            .filter(|t| t.0 <= now)
            .min();
        let Some((due, _, k, what)) = next else {
            break;
        };
        if what != LOOK {
            let Some(mut p) = w.sim.gameplay.ai.players.get_mut(&k).map(std::mem::take) else {
                break;
            };
            if let Some(slot) = w.sim.gameplay.ai.players.get_mut(&k) {
                slot.mode = p.mode;
            }
            if what == SITE {
                p.search_on(&mut w);
            } else {
                p.forest_on(&mut w);
            }
            if let Some(slot) = w.sim.gameplay.ai.players.get_mut(&k) {
                *slot = p;
            }
            continue;
        }
        let others: BTreeMap<u8, Other> = w
            .sim
            .gameplay
            .ai
            .players
            .iter()
            .filter(|(b, _)| **b != k)
            .map(|(&b, x)| {
                (
                    b,
                    Other {
                        available: x.available,
                        fields: x.lists.fields.len() as u32,
                    },
                )
            })
            .collect();
        let Some(mut p) = w.sim.gameplay.ai.players.get_mut(&k).map(std::mem::take) else {
            break;
        };
        p.due = Some(due + period);
        p.seq = w.sim.gameplay.ai.next_seq();
        // The placeholder keeps the kingdom a computer's meanwhile.
        if let Some(slot) = w.sim.gameplay.ai.players.get_mut(&k) {
            slot.mode = p.mode;
        }
        p.look(&mut w, k, &others);
        if let Some(slot) = w.sim.gameplay.ai.players.get_mut(&k) {
            *slot = p;
        }
    }
}

/// The first looks of the players made with the manager (0x402760), one
/// period apart over their number, in kingdom order.
pub(crate) fn schedule(sim: &mut Simulation, m: &MissionWorld) {
    let now = sim.ticks * TICK_MS;
    let count = sim.gameplay.ai.players.len().max(1) as u64;
    let step = period(m) / (count & 0xff).max(1);
    let mut delay = 0;
    let mut seq = sim.gameplay.ai.sequence;
    for p in sim.gameplay.ai.players.values_mut() {
        seq += 1;
        p.due = Some(now + delay);
        p.seq = seq;
        delay += step;
    }
    sim.gameplay.ai.sequence = seq;
}

#[cfg(test)]
#[path = "think_tests.rs"]
mod tests;
