//! A computer player's transports against the native routines (0x415f80,
//! 0x415ec0, 0x416d00, programs 0x2b and 0x2c), on two islands of the test
//! world: west at 10–19 and east at 40–49 across, both 20–29 down, each in
//! a ring of shore whose edges turn round it, water elsewhere; kingdom 2 a
//! computer player living on the west one.
use super::*;
use crate::gameplay::ai::script::Library;
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
/// An island of land at `x0..=x0 + 9` across and 20–29 down, in its ring of
/// shore (up 0, right 1, down 3, left 7).
fn island(g: &mut MapGrid, x0: i32) {
    let (a, b) = (x0 - 1, x0 + 10);
    for y in 19..=30 {
        for x in a..=b {
            let tile: u16 = if (x0..x0 + 10).contains(&x) && (20..=29).contains(&y) {
                0x90
            } else {
                let shape = if y == 19 && x < b {
                    1
                } else if x == b && y < 30 {
                    3
                } else if y == 30 && x > a {
                    7
                } else {
                    0
                };
                0x10 | shape
            };
            g.cells[y as usize * 64 + x as usize] = tile;
        }
    }
}
fn world() -> (Catalog, ObjectLibrary, MissionWorld, MapGrid, Simulation) {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    g.cells.iter_mut().for_each(|t| *t = 0);
    island(&mut g, 10);
    island(&mut g, 40);
    m.objects.clear();
    sim.objects.clear();
    sim.gameplay.actors.clear();
    let lib_ai = Library::new(
        [(
            "data\\ai\\t.ais".to_owned(),
            b"rule have 1 castle end".to_vec(),
        )],
        c.ai.types.clone(),
    );
    let p = sim.gameplay.ai.players.get_mut(&2).unwrap();
    p.program = Some(lib_ai.compile("t.ais").unwrap());
    // Economy only, and no harbor: the ship is the player's from the start.
    p.mode = 1;
    (c, lib, m, g, sim)
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_unit_sent_across_the_water_goes_by_ship() {
    let (c, lib, mut m, mut g, mut sim) = world();
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [12, 21]).unwrap();
    let soldier = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [16, 27]).unwrap();
    let ship = new_unit(&mut sim, &mut m, &lib, 24, 2, 1, [24, 25]).unwrap();
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    let point = [45, 25];
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        assert!(p.ships.values().flatten().any(|&s| s == ship));
        p.command(w, soldier, Intent::Move(point), [45, 25], None);
    });
    // 0x415f80: the islands are joined by the water only; the player's
    // idle ship takes the soldier (0x415ec0): Use transport for it, the
    // order kept; the ship's Transport to the point, landing on the east
    // island.
    let east = sim.gameplay.islands.region([45, 25]);
    let a = &sim.gameplay.actors[soldier];
    assert!(matches!(a.intent, Some(Intent::UseTransport(r)) if r.ship == ship && r.walk));
    assert_eq!(a.resume, Some(Intent::Move(point)));
    let Some(Intent::Transport(t)) = &sim.gameplay.actors[ship].intent else {
        panic!("{:?}", sim.gameplay.actors[ship].intent);
    };
    assert_eq!(
        (t.point, t.landing, &t.passengers),
        (point, east, &vec![soldier])
    );
    assert_eq!(program(&sim, &m, ship).kind, program::TRANSPORT);
    // The soldier boards, the ship crosses and lands it on the east
    // island; then it walks to the point and its order goes on.
    let mut landed = false;
    for _ in 0..4000 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
        let o = &m.objects[soldier];
        if sim.objects[soldier].holder.is_none()
            && sim.gameplay.islands.region([o.x as i32, o.y as i32]) == east
        {
            landed = true;
        }
        if landed && sim.gameplay.actors[soldier].intent.is_none() {
            break;
        }
    }
    assert!(landed);
    let o = &m.objects[soldier];
    assert_eq!([o.x, o.y], point);
    assert!(!matches!(
        sim.gameplay.actors[ship].intent,
        Some(Intent::Transport(_))
    ));
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_ship_loading_for_the_same_island_takes_another_passenger() {
    let (c, lib, mut m, mut g, mut sim) = world();
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [12, 21]).unwrap();
    let first = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [16, 27]).unwrap();
    let second = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [14, 27]).unwrap();
    let ship = new_unit(&mut sim, &mut m, &lib, 24, 2, 1, [24, 25]).unwrap();
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        p.command(w, first, Intent::Move([45, 25]), [45, 25], None);
        p.command(w, second, Intent::Move([42, 22]), [42, 22], None);
    });
    // 0x416dd4: the ship still loads for the east island, with room: the
    // second soldier joins its passengers, the Transport's point unchanged.
    let Some(Intent::Transport(t)) = &sim.gameplay.actors[ship].intent else {
        panic!();
    };
    assert_eq!(t.point, [45, 25]);
    assert_eq!(t.passengers, vec![first, second]);
    assert!(matches!(
        sim.gameplay.actors[second].intent,
        Some(Intent::UseTransport(r)) if r.ship == ship && r.point == [42, 22]
    ));
}

#[test]
#[ignore = "Requires installed original archives"]
fn without_a_ship_the_order_waits_kept() {
    let (c, lib, mut m, mut g, mut sim) = world();
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [12, 21]).unwrap();
    let soldier = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [16, 27]).unwrap();
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        p.command(w, soldier, Intent::Move([45, 25]), [45, 25], None);
    });
    // 0x40bc49: no ship (2): nothing starts, the order stays kept; the
    // water now wants a ship for the crossing (0x416d00).
    let a = &sim.gameplay.actors[soldier];
    assert_eq!(a.intent, None);
    assert_eq!(a.resume, Some(Intent::Move([45, 25])));
    let water = sim.gameplay.islands.region([30, 25]);
    assert!(
        sim.gameplay.ai.players[&2]
            .wanted_ships
            .get(&water)
            .copied()
            .unwrap_or(0)
            >= 1
    );
}
