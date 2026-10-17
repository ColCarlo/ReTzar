//! A computer player's gatherers, stores and wood against the native
//! routines (0x401be0, 0x401c90, 0x40e7a0, 0x40dd90, 0x40eab0, 0x40f370,
//! 0x40f880, 0x40e150, 0x40d5c0, 0x40d790), in the test world: kingdom 2 a
//! European computer player on a 64-cell map of grass.
use super::*;
use crate::gameplay::ai::script::Library;
use crate::gameplay::ai::think::Entry;
use crate::gameplay::tests::setup;

fn tick(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
) {
    advance(sim, m, g, lib, c, Duration::from_millis(TICK_MS), 10, false);
}
/// A script whose only rule holds: the Build phase does nothing.
fn quiet(sim: &mut Simulation, c: &Catalog) {
    let lib = Library::new(
        [(
            "data\\ai\\t.ais".to_owned(),
            b"rule have 1 castle end".to_vec(),
        )],
        c.ai.types.clone(),
    );
    sim.gameplay.ai.players.get_mut(&2).unwrap().program = Some(lib.compile("t.ais").unwrap());
}
/// `f` on kingdom 2's player and the world, as its phases run.
fn on_player<R>(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    f: impl FnOnce(&mut Player, &mut World) -> R,
) -> R {
    let mut w = World {
        sim,
        m,
        g,
        lib,
        c,
        cells: None,
    };
    let mut p = std::mem::take(w.sim.gameplay.ai.players.get_mut(&2).unwrap());
    let out = f(&mut p, &mut w);
    *w.sim.gameplay.ai.players.get_mut(&2).unwrap() = p;
    out
}
fn gold_mine(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    at: [u16; 2],
) -> usize {
    let j = new_unit(sim, m, lib, class::RES_GOLD, 0, 0, at).unwrap();
    m.objects[j].data.resize(64, 0);
    m.objects[j].data[58..62].copy_from_slice(&5000u32.to_le_bytes());
    j
}
fn peasants(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    at: impl IntoIterator<Item = [u16; 2]>,
) -> Vec<usize> {
    at.into_iter()
        .map(|p| new_unit(sim, m, lib, class::PEON, 2, 1, p).unwrap())
        .collect()
}

#[test]
fn the_wood_is_looked_for_ring_by_ring_clockwise() {
    let walk = |lo: [i32; 2], hi: [i32; 2], side: i32| {
        let mut out = Vec::new();
        let mut p = ring_first(lo, hi, side);
        while p != NOWHERE {
            out.push(p);
            p = ring_next(lo, hi, p, side);
        }
        out
    };
    // 0x401be0, 0x401c90: the top row rightwards, the right column down,
    // the bottom row leftwards, the left column up to below the corner.
    assert_eq!(walk([5, 5], [5, 5], 20), [[5, 5]]);
    assert_eq!(
        walk([4, 4], [6, 6], 20),
        [
            [4, 4],
            [5, 4],
            [6, 4],
            [6, 5],
            [6, 6],
            [5, 6],
            [4, 6],
            [4, 5]
        ]
    );
    // Only the ring's cells on the map, from the first of them in that turn.
    assert_eq!(walk([-1, -1], [1, 1], 20), [[1, 0], [1, 1], [0, 1]]);
    assert_eq!(walk([18, 18], [20, 20], 20), [[18, 18], [19, 18], [18, 19]]);
    // A ring round the whole map has none.
    assert_eq!(ring_first([-1, -1], [20, 20], 20), NOWHERE);
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_peasants_are_shared_by_what_each_resource_lacks() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        p.lists.groups[group::IDLE as usize] = (100..110).collect();
        p.available = [0; 4];
        // Gold: the script's 100 half again, then half again: 225. Food 50.
        // Wood: the defences' 100 halved without barracks: 50.
        p.buckets[bucket::SCRIPT][0] = 100;
        p.buckets[bucket::WORKERS][2] = 50;
        p.buckets[bucket::DEFENCE][3] = 100;
        p.want(w);
        // 0x40e780: needs in hundreds, the later resource first on ties.
        assert_eq!(p.order.0, [3, 0, 2, 1]);
        // 0x40e7a0: 6, 1 and 1 of the ten by 225, 50 and 50 of 325; the
        // two left go one each to the first in order.
        assert_eq!(p.wanted, [7, 0, 1, 2]);
        // A blocked resource needs nothing and gets nobody.
        p.blocked[0] = true;
        p.want(w);
        assert_eq!(p.order.0, [3, 2, 1, 0]);
        assert_eq!(p.wanted, [0, 0, 5, 5]);
        // Nothing short: the ten are shared evenly among the others, one
        // more to the first in order.
        p.available = [1000; 4];
        p.want(w);
        assert_eq!(p.wanted, [0, 3, 3, 4]);
        // King of the Hill with unlimited resources wants nobody.
        w.m.game_rules.game_type = 1;
        w.m.game_rules.resource_level = 2;
        p.want(w);
        assert_eq!(p.wanted, [0; 4]);
    });
}

#[test]
#[ignore = "Requires installed original archives"]
fn gatherers_go_to_the_nearest_mine_ten_at_most() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [28, 28]).unwrap();
    let near = gold_mine(&mut sim, &mut m, &lib, [34, 28]);
    let far = gold_mine(&mut sim, &mut m, &lib, [26, 36]);
    // One column, each a cell farther from the near mine.
    let men = peasants(&mut sim, &mut m, &lib, (31..43).map(|y| [36, y]));
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        assert_eq!(p.lists.mines[0].object, near);
        p.available = [0; 4];
        p.buckets = [[0; 4]; 5];
        p.buckets[bucket::SCRIPT][0] = 1000;
        p.harvest(w);
        assert_eq!(p.wanted, [12, 0, 0, 0]);
        // 0x40eab0 / 0x40f370: the nearest mine takes the ten nearest; the
        // next mine the rest. The hook sorts them at once.
        for (n, &i) in men.iter().enumerate() {
            let mine = if n < 10 { near } else { far };
            assert_eq!(
                w.sim.gameplay.actors[i].intent,
                Some(Intent::Harvest(mine)),
                "{n}"
            );
        }
        assert_eq!(p.lists.groups[group::GOLD as usize].len(), 12);
        assert!(p.lists.mines.iter().all(|e| e.count & 1 != 0));
        // 0x40f880: two too many go back idle, the farthest from their work
        // first: the last two of the near mine's.
        p.release(w, 0, 2);
        for (n, &i) in men.iter().enumerate() {
            let idle = matches!(n, 8 | 9);
            assert_eq!(w.sim.gameplay.actors[i].intent.is_none(), idle, "{n}");
        }
        assert_eq!(p.lists.groups[group::GOLD as usize].len(), 10);
    });
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_mine_without_a_store_near_gets_one_and_its_gatherers_wait() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 50]).unwrap();
    let mine = gold_mine(&mut sim, &mut m, &lib, [50, 30]);
    let men = peasants(&mut sim, &mut m, &lib, [[44, 30], [44, 32]]);
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        p.available = [10000; 4];
        p.buckets = [[0; 4]; 5];
        p.buckets[bucket::SCRIPT][0] = 1000;
        // 0x40f370: no castle or storage within 16: nobody goes, but the
        // mine is marked worked; what it could not take goes to wood.
        p.harvest(w);
        assert!(
            men.iter()
                .all(|&i| w.sim.gameplay.actors[i].intent.is_none())
        );
        assert_eq!(
            p.lists.mines,
            [Entry {
                object: mine,
                count: 1
            }]
        );
        assert_eq!(p.wanted[0], 0);
        // 0x40e150: the worked mine without gatherers gets a storage 1 to
        // 16 from it, built by the nearest idle peasant.
        p.storages(w);
        let site = (0..w.m.objects.len())
            .find(|&j| w.m.objects[j].class == class::STORAGE && w.m.objects[j].owner == 2)
            .expect("a storage site");
        let d = build::rect_gap(rect_of(w, mine), rect_of(w, site));
        assert!((1..=16).contains(&d), "{d}");
        let builders: Vec<usize> = men
            .iter()
            .copied()
            .filter(|&i| w.sim.gameplay.actors[i].intent == Some(Intent::Build(site)))
            .collect();
        assert_eq!(builders.len(), 1);
        assert_eq!(p.lists.groups[group::BUILDERS as usize], builders);
    });
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_field_is_laid_and_farmed_at_the_next_harvest() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [28, 28]).unwrap();
    new_unit(&mut sim, &mut m, &lib, class::FARM, 2, 1, [38, 22]).unwrap();
    let men = peasants(&mut sim, &mut m, &lib, [[38, 30], [38, 32]]);
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // No food: what the script books of it is short (a site laid reads the
    // kingdom's resources again, 0x40b090).
    m.kingdoms[1].resources[2] = 0;
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        p.available = [10000, 10000, 0, 10000];
        p.buckets = [[0; 4]; 5];
        let fields = |w: &World| -> Vec<usize> {
            (0..w.m.objects.len())
                .filter(|&j| w.m.objects[j].class == class::FIELD && w.m.objects[j].owner == 2)
                .collect()
        };
        let farming = |w: &World, f: usize| {
            men.iter()
                .filter(|&&i| w.sim.gameplay.actors[i].intent == Some(Intent::Harvest(f)))
                .count()
        };
        // 0x40efeb: no field free, so one is laid round the home and listed
        // at once (0x408670); 0x40b610 gives it back only to a builder in
        // Build, so nobody goes this time and food's need goes to wood.
        p.buckets[bucket::SCRIPT][2] = 1000;
        p.harvest(w);
        let first = fields(w);
        assert_eq!(first.len(), 1);
        assert_eq!(
            p.lists.fields,
            [Entry {
                object: first[0],
                count: 0
            }]
        );
        assert!(
            men.iter()
                .all(|&i| w.sim.gameplay.actors[i].intent.is_none())
        );
        assert_eq!(p.wanted, [0, 0, 0, 2]);
        // Next time the field takes one farmer, and another field is laid.
        p.harvest(w);
        let both = fields(w);
        assert_eq!(both.len(), 2);
        assert_eq!(farming(w, both[0]), 1);
        assert_eq!(
            p.lists.fields,
            [
                Entry {
                    object: both[0],
                    count: 1
                },
                Entry {
                    object: both[1],
                    count: 0
                }
            ]
        );
        assert_eq!(p.wanted, [0, 0, 1, 1]);
        // Then the other field its own.
        p.harvest(w);
        assert_eq!(fields(w).len(), 2);
        assert_eq!(farming(w, both[1]), 1);
        assert_eq!(p.lists.groups[group::FOOD as usize].len(), 2);
    });
}

#[test]
#[ignore = "Requires installed original archives"]
fn woodcutters_go_four_to_a_tree_near_the_lumber_mill() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 50]).unwrap();
    let mill = new_unit(&mut sim, &mut m, &lib, class::LUMBER, 2, 1, [40, 20]).unwrap();
    let r = rect_of_object(&lib, &m, mill);
    // A tree a cell right of the mill, another farther.
    let tree = [r[2] + 2, r[1]];
    let other = [r[2] + 5, r[1]];
    for t in [tree, other] {
        g.cells[t[1] as usize * 64 + t[0] as usize] = 0x90 | 4 << 11;
    }
    let men = peasants(&mut sim, &mut m, &lib, (0..6).map(|n| [30 + n, 30]));
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        p.available = [0; 4];
        p.buckets = [[0; 4]; 5];
        p.buckets[bucket::SCRIPT][3] = 1000;
        p.harvest(w);
        // 0x40eab0: the first tree of the flood from the mill's centre,
        // every time: four at once, then the rest, to the same tree.
        let chop = Some(Intent::Chop([tree[0] as u16, tree[1] as u16]));
        for &i in &men {
            assert_eq!(w.sim.gameplay.actors[i].intent, chop);
        }
        assert_eq!(p.lists.groups[group::WOOD as usize].len(), 6);
        // The mill keeps its tree; the castle was not looked at.
        assert!(p.lists.bare.is_empty());
    });
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_wood_search_keeps_the_first_tree_and_stops_at_the_next_wood() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [28, 28]).unwrap();
    let [wd, ht] = footprint(&lib, &m.objects[castle]).map(|n| n as i32);
    let home = [28 + wd / 2, 28 + ht / 2];
    // A block of 3×3 trees from the sixth ring, clear of the castle, and a
    // lone tree on the 15th.
    let mut plant = |x: i32, y: i32| g.cells[y as usize * 64 + x as usize] = 0x90 | 4 << 11;
    for dx in 6..=8 {
        for dy in -1..=1 {
            plant(home[0] + dx, home[1] + dy);
        }
    }
    plant(home[0], home[1] + 15);
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert_eq!(sim.gameplay.ai.players[&2].home, Some(home));
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        p.forest = Forest {
            at: home,
            ..Default::default()
        };
        // 0x40d5c0: the first tree woodcutters may go to is the wood (the
        // ring's right column down, its top cell first); the block's centre
        // has no open neighbour (0x40d790), so its ring is kept; the lone
        // tree 10 or more away ends the search while the wood stands.
        assert!(!p.forest_search(w));
        assert_eq!(p.forest.wood, Some([home[0] + 6, home[1] - 1]));
        assert_eq!(p.forest.saved, 7);
        assert_eq!(p.forest.at, [home[0], home[1] + 15]);
        assert!(!p.forest.none);
    });
}

fn rect_of_object(lib: &ObjectLibrary, m: &MissionWorld, i: usize) -> [i32; 4] {
    movement::footprint_rect(lib, &m.objects[i])
}
