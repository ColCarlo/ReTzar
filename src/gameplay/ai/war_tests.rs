//! A computer player's war against the native routines (0x52e320,
//! 0x410be0, 0x4113a0, 0x4111b0, 0x42dde0, 0x40fcc0), in the test world:
//! kingdom 2 a computer player on a 64-cell map of grass, at war with the
//! human kingdom 1.
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

#[test]
fn the_native_sort_moves_equal_elements_its_own_way() {
    let mut cmp = |a: &(char, u16), b: &(char, u16)| {
        if a.1 < b.1 {
            1
        } else if a.1 == b.1 {
            0
        } else {
            -1
        }
    };
    // 0x52e2b0 alone (seven or fewer): a bubble sort, ties kept in order.
    let mut v: Vec<(char, u16)> = "abcdefg".chars().zip([1, 1, 1, 1, 1, 1, 2]).collect();
    native_sort(&mut v, &mut cmp);
    assert_eq!(v.iter().map(|e| e.0).collect::<String>(), "gabcdef");
    // Eight: the pivot (the fifth) changes place with the last, which then
    // ends behind the others of its score.
    let mut v: Vec<(char, u16)> = "abcdefgh".chars().zip([1, 1, 1, 1, 1, 1, 1, 2]).collect();
    native_sort(&mut v, &mut cmp);
    assert_eq!(v.iter().map(|e| e.0).collect::<String>(), "habcdfge");
    // Longer runs still come out in order of score.
    let mut v: Vec<(usize, u16)> = (0..40).map(|i| (i, ((i * 7) % 11) as u16)).collect();
    native_sort(&mut v, &mut |a, b| cmp(&('x', a.1), &('x', b.1)));
    assert!(v.windows(2).all(|p| p[0].1 >= p[1].1));
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_threatened_home_calls_the_army_with_approach() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 8]).unwrap();
    // An enemy knight at the castle, three of the player's far away.
    new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [12, 16]).unwrap();
    let knights: Vec<usize> = [[50, 50], [52, 50], [54, 50]]
        .into_iter()
        .map(|p| new_unit(&mut sim, &mut m, &lib, 8, 2, 1, p).unwrap())
        .collect();
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    let home = on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| {
        let mgr = w.sim.gameplay.ai.manager.as_ref().unwrap();
        let home = p
            .records
            .iter()
            .position(|r| {
                mgr.place(r.place).is_some_and(|x| {
                    x.kind == places::kind::HOME && x.param == 2 && x.anchor == Some(castle)
                })
            })
            .expect("the home's record");
        assert_eq!(p.lists.army.len(), 3);
        p.war(w);
        p.records[home]
    });
    // 0x4113a0: each gets Approach to the same point of the home place,
    // with its own range, and they share their paths.
    let mut group = None;
    for &i in &knights {
        let Some(Intent::Approach(ap)) = sim.gameplay.actors[i].intent else {
            panic!("{:?}", sim.gameplay.actors[i].intent);
        };
        assert_eq!((ap.place, ap.serial, ap.step), (home.place, home.serial, 1));
        assert!((24..=32).contains(&ap.radius));
        let place = sim
            .gameplay
            .ai
            .manager
            .as_ref()
            .unwrap()
            .place(home.place)
            .unwrap();
        let [x, y] = ap.point.map(|v| v as i32);
        assert!(
            x >= place.rect[0] && x <= place.rect[2] && y >= place.rect[1] && y <= place.rect[3]
        );
        assert!(sim.gameplay.actors[i].group.is_some());
        group = group.or(sim.gameplay.actors[i].group);
        assert_eq!(sim.gameplay.actors[i].group, group);
    }
    assert_eq!(sim.gameplay.groups[&group.unwrap()].members.len(), 3);
    // Op 0x4f: out of the range they walk, counted on their way (+0x84).
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert_eq!(war::on_their_way(&sim, &m, 2, home.place, home.serial), 3);
    // 0x40fcc0: the place gone, those still walking go back to idleness.
    let mut mgr = sim.gameplay.ai.manager.take().unwrap();
    mgr.events.push((false, home.place, home.serial));
    sim.gameplay.ai.manager = Some(mgr);
    records::sync(&mut sim, &m);
    for &i in &knights {
        assert_eq!(sim.gameplay.actors[i].intent, None);
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_unit_of_the_story_is_never_sent_by_its_computer() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 8]).unwrap();
    new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [12, 16]).unwrap();
    let knights: Vec<usize> = [[50, 50], [52, 50], [54, 50]]
        .into_iter()
        .map(|p| new_unit(&mut sim, &mut m, &lib, 8, 2, 1, p).unwrap())
        .collect();
    // 0x410cd0 turns away the flags 0x18400002, the story's 0x8000000
    // among them (record flag 0x2000, 0x4ffe74), not the script's lock.
    m.objects[knights[0]].flags |= FLAG_STORY;
    m.objects[knights[1]].flags |= FLAG_LOCKED;
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| p.war(w));
    assert_eq!(sim.gameplay.actors[knights[0]].intent, None);
    for &i in &knights[1..] {
        assert!(matches!(
            sim.gameplay.actors[i].intent,
            Some(Intent::Approach(_))
        ));
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_group_near_the_place_waits_then_advances() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    // The home away from the player's objects, so that no place of the
    // knight's sector is threatened (0x4130a0).
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [44, 44]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [30, 30]).unwrap();
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    let (place, serial, anchor) = {
        let mgr = sim.gameplay.ai.manager.as_ref().unwrap();
        let slot = (0..mgr.places.len())
            .find(|&s| mgr.place(s).is_some_and(|x| x.anchor == Some(castle)))
            .unwrap();
        let p = mgr.place(slot).unwrap();
        (slot, p.serial, p.anchor_rect)
    };
    let ap = Approach {
        place,
        serial,
        rect: anchor,
        point: [20, 20],
        radius: 32,
        step: 1,
        sector: None,
        due: 0,
    };
    // Op 0x4f within the range: it counts the anchor's sector as its
    // heading and waits; nobody threatens the place, so it goes on and
    // advances to the point (op 0x50, then a Move with argument 2).
    assert_eq!(
        approach_check(&sim, &m, knight, &ap),
        Some(Some([(anchor[0] / 8) as u16, (anchor[1] / 8) as u16]))
    );
    set_order(&mut sim, knight, Intent::Approach(ap), false).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let Some(Intent::Approach(now)) = sim.gameplay.actors[knight].intent else {
        panic!();
    };
    assert_eq!(now.step, 3);
    assert_eq!(heading(&sim, knight), now.sector);
    assert_eq!(wait_others(&mut sim, &m, knight, &now), 1);
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert!(matches!(sim.gameplay.actors[knight].intent, Some(Intent::Approach(x)) if x.step == 4));
    assert_eq!(program(&sim, &m, knight).kind, program::APPROACH);
}

#[test]
#[ignore = "Requires installed original archives"]
fn an_enemy_tower_at_home_is_captured_by_resistance() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 8]).unwrap();
    let men: Vec<usize> = (0..12)
        .map(|n| new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [16 + n, 12]).unwrap())
        .collect();
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    // A finished stone tower of the enemy appears among them: the hook on
    // new objects (0x411ed0) calls the militia to capture it.
    let tower = new_unit(&mut sim, &mut m, &lib, class::TOWER_STONE, 1, 1, [16, 14]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    let r = resistance_target(&sim, &m, &lib, tower) + sim.objects[tower].resistance as i32;
    let (first, second) = ((r / 15 + 1) as usize, (r / 10 + 1) as usize);
    // One each: N1 counted, then sent until N2 (at least one).
    let sent = second.saturating_sub(first).max(1).min(men.len());
    let capturing = men
        .iter()
        .filter(|&&i| sim.gameplay.actors[i].intent == Some(Intent::Capture(tower)))
        .count();
    assert_eq!(capturing, sent, "resistance {r}");
}

/// The test world with `n` of kingdom 2's peasants in a row by its castle
/// and an enemy archer beside the first; the archer and the peasants.
fn raid(
    n: usize,
) -> (
    Catalog,
    ObjectLibrary,
    MissionWorld,
    MapGrid,
    Simulation,
    usize,
    Vec<usize>,
) {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 8]).unwrap();
    let men: Vec<usize> = (0..n)
        .map(|k| {
            new_unit(
                &mut sim,
                &mut m,
                &lib,
                class::PEON,
                2,
                1,
                [8 + k as u16, 20],
            )
            .unwrap()
        })
        .collect();
    let archer = new_unit(&mut sim, &mut m, &lib, 4, 1, 1, [6, 20]).unwrap();
    for _ in 0..2 {
        tick(&mut sim, &mut m, &mut g, &lib, &c);
    }
    (c, lib, m, g, sim, archer, men)
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_militia_sends_the_nearest_peasants_past_the_first_count() {
    let (c, lib, mut m, g, mut sim, archer, men) = raid(24);
    let s = |sim: &Simulation, m: &MissionWorld, i: usize| strength(sim, m, &c, i) as u32;
    let threat = s(&sim, &m, archer);
    let each = s(&sim, &m, men[0]);
    // 0x4120d0 takes the home place only when its fighters and peasants
    // reach three times the archer.
    assert!(each * men.len() as u32 >= 3 * threat, "{each} {threat}");
    // N1 = 3/2 of the threat counted first, then the peasants go from the
    // nearest while the count, going on, stays under N2 = 2 times.
    let mut sum = 0;
    for _ in &men {
        sum += each;
        if sum >= threat * 3 / 2 {
            break;
        }
    }
    let mut sent = 0;
    while sent < men.len() {
        sent += 1;
        sum += each;
        if sum >= threat * 2 {
            break;
        }
    }
    on_player(&mut sim, &mut m, &g, &lib, &c, |p, w| p.militia(w, archer));
    for (n, &i) in men.iter().enumerate() {
        let expected = (n < sent).then_some(Intent::Attack(archer));
        assert_eq!(sim.gameplay.actors[i].intent, expected, "{n}");
    }
    // The peasants stay in the order of their distance from the threat.
    assert_eq!(sim.gameplay.ai.players[&2].lists.peasants, men);
}

#[test]
#[ignore = "Requires installed original archives"]
fn an_unanswered_call_for_help_raises_the_militia() {
    let (c, lib, mut m, g, mut sim, archer, men) = raid(24);
    // 0x413a99: no fighter answers the peasant's call, so its player sends
    // peasants against the archer before the step ends.
    call_for_help(&mut sim, &m, &lib, &c, men[0], archer);
    assert_eq!(sim.gameplay.ai.alarms, [(2, archer)]);
    alarms(&mut sim, &mut m, &g, &lib, &c);
    assert!(sim.gameplay.ai.alarms.is_empty());
    assert_eq!(
        sim.gameplay.actors[men[0]].intent,
        Some(Intent::Attack(archer))
    );
    // Too few peasants for three times the threat: nobody goes.
    let (c, lib, mut m, g, mut sim, archer, men) = raid(1);
    call_for_help(&mut sim, &m, &lib, &c, men[0], archer);
    alarms(&mut sim, &mut m, &g, &lib, &c);
    assert_eq!(sim.gameplay.actors[men[0]].intent, None);
}

#[test]
#[ignore = "Requires installed original archives"]
fn without_its_castle_a_computer_resigns_when_allowed() {
    for allowed in [false, true] {
        let (c, lib, mut m, mut g, mut sim) = setup(1);
        // The chain of resign.ai, cut down to its castle.
        let text = b"{ critical rule when not trigger Resigned end
              rule when trigger Resign have resign trigger Resigned end
              critical rule when not trigger Resign end
              rule trigger !Castle end
              rule when castle trigger Castle end
              critical rule when not trigger Castle end
              rule trigger Resign end }";
        let lib_ai = Library::new(
            [("data\\ai\\t.ais".to_owned(), text.to_vec())],
            c.ai.types.clone(),
        );
        sim.gameplay.ai.players.get_mut(&2).unwrap().program =
            Some(lib_ai.compile("t.ais").unwrap());
        m.game_rules.ai_resign = allowed;
        let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [8, 8]).unwrap();
        new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [16, 12]).unwrap();
        tick(&mut sim, &mut m, &mut g, &lib, &c);
        let build = |sim: &mut Simulation, m: &mut MissionWorld| {
            on_player(sim, m, &g, &lib, &c, |p, w| {
                rules::build(w, p, 2, &BTreeMap::new())
            });
        };
        build(&mut sim, &mut m);
        build(&mut sim, &mut m);
        assert!(!sim.gameplay.script.kingdoms[2].resigned);
        // The castle falls: the switch it set stays on in the original;
        // with the resignation allowed the chain is read again (the decided
        // difference) and two phases later the kingdom resigns (0x43).
        kill(&mut sim, &mut m, &g, &lib, castle);
        for _ in 0..3 {
            build(&mut sim, &mut m);
        }
        assert_eq!(sim.gameplay.script.kingdoms[2].resigned, allowed);
    }
}
