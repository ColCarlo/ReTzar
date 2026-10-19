//! Stress of the pathfinding on random maps: walkers sent far across the
//! land one by one, and an army sent twice across the map together. Every
//! step must stay on open ground; how many arrive, how long they take
//! against the shortest way of open cells, and who is left standing are
//! reported.

use super::pathing::{BLOCKED, BRIDGE, KIND, LAND};
use super::tests::request;
use super::*;
use std::collections::VecDeque;

struct Map {
    c: Catalog,
    lib: ObjectLibrary,
    m: MissionWorld,
    g: MapGrid,
    sim: Simulation,
}

fn open_random(side: usize, seed: u64) -> Map {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let assets = crate::assets::GameAssets::open(root).unwrap();
    let selection = crate::menu::Selection::Random { side, seed };
    let opened = crate::game::open(&assets, &selection, None, None).unwrap();
    let lib = ObjectLibrary::load(&assets, &opened.biome).unwrap();
    let mut m = opened.mission;
    crate::game::start(
        &mut m,
        &opened.grid,
        &lib,
        &opened.biome,
        &selection,
        None,
        crate::terrain::Climate::default(),
    )
    .unwrap();
    // Nobody but the walkers moves on its own.
    for k in &mut m.kingdoms {
        k.computer = false;
        k.ai_mode = 0;
    }
    let mut sim = m.simulation.take().unwrap();
    movement::refresh(&mut sim, &m, &opened.grid, &lib);
    Map {
        c: Catalog::load(&assets).unwrap(),
        lib,
        m,
        g: opened.grid,
        sim,
    }
}

fn open_ground(map: &Map, p: [i32; 2]) -> bool {
    map.sim.gameplay.pathing.cells.inside(p) && map.sim.gameplay.pathing.cells.passable(p)
}
/// A cell a walker may stand on: no building, wall or rock (BLOCKED), on
/// land or a bridge.
fn standing_ground(map: &Map, p: [i32; 2]) -> bool {
    let w = map.sim.gameplay.pathing.cells.at(p);
    w & BLOCKED == 0 && (w & BRIDGE != 0 || w & KIND == LAND)
}
/// Shortest ways in steps over open ground, 8 neighbours, from `from`.
fn distances(map: &Map, from: [i32; 2]) -> Vec<u32> {
    let side = map.g.side as i32;
    let mut d = vec![u32::MAX; (side * side) as usize];
    let at = |p: [i32; 2]| (p[1] * side + p[0]) as usize;
    d[at(from)] = 0;
    let mut queue = VecDeque::from([from]);
    while let Some(p) = queue.pop_front() {
        for dx in -1..=1 {
            for dy in -1..=1 {
                let n = [p[0] + dx, p[1] + dy];
                if (dx, dy) != (0, 0) && open_ground(map, n) && d[at(n)] == u32::MAX {
                    d[at(n)] = d[at(p)] + 1;
                    queue.push_back(n);
                }
            }
        }
    }
    d
}
fn way(map: &Map, d: &[u32], p: [i32; 2]) -> u32 {
    d[(p[1] * map.g.side as i32 + p[0]) as usize]
}

struct Walk {
    unit: usize,
    to: [u16; 2],
    way: u32,
    arrived: Option<u64>,
    moved: (u64, [u16; 2]),
}
#[derive(Default, Debug)]
struct Outcome {
    walkers: usize,
    arrived: usize,
    /// Those that stopped short for a minute, with where they stand.
    stuck: Vec<String>,
    /// Of those, how many could still reach their place.
    short: usize,
    off_ground: Vec<String>,
    /// Seconds of play per step of the shortest way, the worst arrival.
    worst_ratio: f64,
    /// Milliseconds of the machine per second of play.
    cost_ms: f64,
}

/// Plays up to `seconds` with the walkers' orders given, a second at a
/// time; arrived within `radius` cells of the place.
fn play(map: &mut Map, walks: &mut [Walk], seconds: u64, radius: i32) -> Outcome {
    let started = std::time::Instant::now();
    let mut out = Outcome {
        walkers: walks.len(),
        ..Default::default()
    };
    let mut elapsed = 0;
    for second in 1..=seconds {
        for _ in 0..1000 / TICK_MS {
            advance(
                &mut map.sim,
                &mut map.m,
                &mut map.g,
                &map.lib,
                &map.c,
                Duration::from_millis(TICK_MS),
                10,
                false,
            );
        }
        elapsed = second;
        for w in walks.iter_mut() {
            let o = &map.m.objects[w.unit];
            let p = [o.x, o.y];
            if !standing_ground(map, [p[0] as i32, p[1] as i32]) && out.off_ground.len() < 20 {
                out.off_ground
                    .push(format!("#{} in {p:?} a {second} s", o.id));
            }
            if p != w.moved.1 {
                w.moved = (second, p);
            }
            let gap = (p[0] as i32 - w.to[0] as i32)
                .abs()
                .max((p[1] as i32 - w.to[1] as i32).abs());
            if w.arrived.is_none() && gap <= radius {
                w.arrived = Some(second);
            }
        }
        if walks.iter().all(|w| w.arrived.is_some()) {
            break;
        }
    }
    for w in walks.iter() {
        match w.arrived {
            Some(t) => {
                out.arrived += 1;
                out.worst_ratio = out.worst_ratio.max(t as f64 / w.way.max(1) as f64);
            }
            None if elapsed - w.moved.0 >= 60 => {
                let o = &map.m.objects[w.unit];
                let here = [o.x as i32, o.y as i32];
                let d = distances(map, here);
                let reach = way(map, &d, [w.to[0] as i32, w.to[1] as i32]);
                out.short += (reach != u32::MAX) as usize;
                out.stuck.push(format!(
                    "#{} fermo in {:?} da {} s{}, verso {:?} (da qui {}), intento {:?}",
                    o.id,
                    here,
                    elapsed - w.moved.0,
                    if w.moved.0 == 0 { " (mai partito)" } else { "" },
                    w.to,
                    if reach == u32::MAX {
                        "irraggiungibile".to_owned()
                    } else {
                        format!("{reach} passi")
                    },
                    map.sim.gameplay.actors[w.unit].intent
                ));
            }
            None => {}
        }
    }
    out.cost_ms = started.elapsed().as_secs_f64() * 1000. / elapsed.max(1) as f64;
    out
}

/// A footman of the player's, unhurtable, on the open cell `p`.
fn walker(map: &mut Map, p: [i32; 2]) -> Option<usize> {
    let player = map.m.player();
    let civ = map.m.kingdoms[player as usize - 1].civilization.max(1);
    let cell = [p[0] as u16, p[1] as u16];
    let i = new_unit(
        &mut map.sim,
        &mut map.m,
        &map.lib,
        class::FOOTMAN,
        player,
        civ,
        cell,
    )
    .ok()?;
    map.m.objects[i].flags |= FLAG_GOD;
    Some(i)
}
/// The open cells of the map, every `step` in each direction, in a fixed
/// shuffled order.
fn spots(map: &Map, step: i32, seed: u64) -> Vec<[i32; 2]> {
    let side = map.g.side as i32;
    let mut out: Vec<[i32; 2]> = (step..side - step)
        .step_by(step as usize)
        .flat_map(|y| {
            (step..side - step)
                .step_by(step as usize)
                .map(move |x| [x, y])
        })
        .filter(|&p| open_ground(map, p))
        .collect();
    let mut r = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    for k in (1..out.len()).rev() {
        r = r
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        out.swap(k, (r >> 33) as usize % (k + 1));
    }
    out
}

/// Walkers each sent from an open cell to its own place far away on the
/// same land.
fn walkers(side: usize, seed: u64, count: usize) -> Outcome {
    let mut map = open_random(side, seed);
    let starts = spots(&map, 6, seed);
    let mut goals = spots(&map, 5, seed ^ 0xbeef);
    let mut walks = Vec::new();
    for &s in &starts {
        if walks.len() == count {
            break;
        }
        let d = distances(&map, s);
        let Some(k) = goals.iter().position(|&g| {
            let w = way(&map, &d, g);
            w != u32::MAX && w as usize >= side / 3
        }) else {
            continue;
        };
        let to = goals.remove(k);
        let Some(unit) = walker(&mut map, s) else {
            continue;
        };
        walks.push(Walk {
            unit,
            to: [to[0] as u16, to[1] as u16],
            way: way(&map, &d, to),
            arrived: None,
            moved: (0, [s[0] as u16, s[1] as u16]),
        });
    }
    for w in &walks {
        let req = request("move", vec![w.unit], w.to, None);
        issue(&mut map.sim, &mut map.m, &map.g, &map.lib, &map.c, &req).unwrap();
    }
    play(&mut map, &mut walks, side as u64 * 3, 2)
}

/// An army camped two cells apart, sent to a far place, then from there
/// all together to another far place: the two marches.
fn army(side: usize, seed: u64, count: usize) -> [Outcome; 2] {
    let mut map = open_random(side, seed);
    let ring = |c: [i32; 2]| {
        (-7..=7)
            .step_by(2)
            .flat_map(move |dy| (-7..=7).step_by(2).map(move |dx| [c[0] + dx, c[1] + dy]))
    };
    let camp = spots(&map, 8, seed)
        .into_iter()
        .find(|&c| ring(c).filter(|&p| open_ground(&map, p)).count() >= count)
        .unwrap();
    let d = distances(&map, camp);
    let first = spots(&map, 5, seed ^ 0xfeed)
        .into_iter()
        .find(|&g| {
            let w = way(&map, &d, g);
            w != u32::MAX && w as usize >= side / 3
        })
        .unwrap();
    let cells: Vec<[i32; 2]> = ring(camp)
        .filter(|&p| open_ground(&map, p))
        .take(count)
        .collect();
    let units: Vec<usize> = cells
        .into_iter()
        .filter_map(|p| walker(&mut map, p))
        .collect();
    let march = |map: &mut Map, to: [i32; 2], from: [i32; 2]| {
        let d = distances(map, from);
        let w = way(map, &d, to);
        let to = [to[0] as u16, to[1] as u16];
        let req = request("move", units.clone(), to, None);
        issue(&mut map.sim, &mut map.m, &map.g, &map.lib, &map.c, &req).unwrap();
        let starts: Vec<[u16; 2]> = units
            .iter()
            .map(|&u| [map.m.objects[u].x, map.m.objects[u].y])
            .collect();
        advance(
            &mut map.sim,
            &mut map.m,
            &mut map.g,
            &map.lib,
            &map.c,
            Duration::from_millis(TICK_MS),
            10,
            false,
        );
        // Each goes to its place in the army's array: the place sent to
        // shifted as the unit stood from the group's middle (0x4df318).
        let mut walks: Vec<Walk> = units
            .iter()
            .zip(starts)
            .map(|(&unit, start)| Walk {
                unit,
                to: map.sim.gameplay.actors[unit]
                    .movement
                    .as_ref()
                    .map_or(to, |mv| [mv.rect[0] as u16, mv.rect[1] as u16]),
                way: w,
                arrived: None,
                moved: (0, start),
            })
            .collect();
        play(map, &mut walks, side as u64 * 4, 2)
    };
    let one = march(&mut map, first, camp);
    // The second march starts from where the army stands.
    let d = distances(&map, first);
    let second = spots(&map, 5, seed ^ 0xcafe)
        .into_iter()
        .filter(|&g| way(&map, &d, g) != u32::MAX)
        .max_by_key(|&g| way(&map, &d, g))
        .unwrap();
    let two = march(&mut map, second, first);
    [one, two]
}

#[test]
#[ignore = "Requires installed original archives"]
fn walkers_cross_random_maps_on_open_ground() {
    let mut failures = Vec::new();
    for (side, seed) in [(128, 1), (128, 2), (128, 3), (256, 4), (256, 5), (512, 6)] {
        let out = walkers(side, seed, 40);
        eprintln!("VIANDANTI {side}/{seed}: {out:?}");
        if !out.off_ground.is_empty() || out.arrived < out.walkers {
            failures.push(format!("{side}/{seed}: {out:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "Requires installed original archives"]
fn an_army_marches_twice_across_random_maps() {
    let mut failures = Vec::new();
    for (side, seed) in [(128, 7), (256, 8), (256, 9)] {
        for (k, out) in army(side, seed, 40).into_iter().enumerate() {
            eprintln!("ESERCITO {side}/{seed} marcia {}: {out:?}", k + 1);
            // Units of the array whose place is out of reach stop as near as
            // they can; a few more stop in a narrow pass the others crowd,
            // where the search around the moving units finds no way on and
            // the Move ends (0x4df043, 0x4a9060).
            if !out.off_ground.is_empty() || out.short * 100 > out.walkers * 15 {
                failures.push(format!("{side}/{seed} marcia {}: {out:?}", k + 1));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
