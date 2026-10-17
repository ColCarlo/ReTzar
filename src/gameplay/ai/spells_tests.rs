//! The spells of a computer player's units against the native routines
//! (0x414730 with 0x4145e0, 0x413410, 0x414f10, 0x4ebe10, 0x472760), in
//! the test world: kingdom 2 a computer player at war with the human
//! kingdom 1, whose castle and peasants stand at the top left.
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
fn quiet(sim: &mut Simulation, c: &Catalog) {
    let lib = Library::new(
        [(
            "data\\ai\\t.ais".to_owned(),
            b"rule have 1 castle end".to_vec(),
        )],
        c.ai.types.clone(),
    );
    let p = sim.gameplay.ai.players.get_mut(&2).unwrap();
    p.program = Some(lib.compile("t.ais").unwrap());
    // Economy only: no group goes anywhere.
    p.mode = 1;
}
/// Kingdom 2 has the research of the spell.
fn study(m: &mut MissionWorld, c: &Catalog, spell: u8) {
    if let Some(dep) = c.order_research(key(spell)) {
        m.rules[1].initial_researches.push(dep.trim().to_owned());
    }
}
fn mana(sim: &mut Simulation, i: usize, mana: u16, max: u16) {
    (sim.objects[i].mana, sim.objects[i].max_mana) = (mana, max);
}
/// Nobody does anything.
fn still(sim: &mut Simulation, units: &[usize]) {
    for &u in units {
        let a = &mut sim.gameplay.actors[u];
        a.intent = None;
        halt(a);
    }
}
fn priest_idle(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
) -> bool {
    let mut p = std::mem::take(sim.gameplay.ai.players.get_mut(&2).unwrap());
    let mut w = World {
        sim,
        m,
        g,
        lib,
        c,
        cells: None,
    };
    let done = p.idle_priest(&mut w, i);
    *w.sim.gameplay.ai.players.get_mut(&2).unwrap() = p;
    done
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_computer_mage_rains_fire_where_the_enemy_crowds() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let mage = new_unit(&mut sim, &mut m, &lib, class::MAGE, 2, 1, [30, 30]).unwrap();
    let a = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [36, 30]).unwrap();
    let b = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [37, 30]).unwrap();
    let d = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [36, 31]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    still(&mut sim, &[mage, a, b, d]);
    mana(&mut sim, mage, 200, 200);
    // Without the research the mage has no spell (0x414d6e).
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, mage, a),
        Intent::Attack(a)
    );
    // With it, three hostiles round the target and nobody else: fire rain
    // at the target's centre (0x4145e0, 0x42ebc0).
    study(&mut m, &c, spell::FIRE_RAIN);
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, mage, a),
        Intent::Cast {
            key: "spell_firerain".into(),
            cell: [36, 30],
            target: Some(a),
        }
    );
    // Short of the mana, no spell (0x4723e0).
    mana(&mut sim, mage, 99, 200);
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, mage, a),
        Intent::Attack(a)
    );
    mana(&mut sim, mage, 200, 200);
    // One hostile in the area is not enough (0x414676).
    for (u, p) in [(b, [44u16, 30u16]), (d, [44, 40])] {
        m.objects[u].x = p[0];
        m.objects[u].y = p[1];
    }
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, mage, a),
        Intent::Attack(a)
    );
    // Nor a target one blow fells (vt+0x1e0, 0x414753).
    m.objects[b].x = 37;
    m.objects[b].y = 30;
    m.objects[a].health = sim.objects[mage].attack as u16;
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, mage, a),
        Intent::Attack(a)
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_computer_mage_spares_its_own_unless_it_is_attacked() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let mage = new_unit(&mut sim, &mut m, &lib, class::MAGE, 2, 1, [30, 30]).unwrap();
    let a = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [36, 30]).unwrap();
    let b = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [37, 30]).unwrap();
    let d = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [36, 31]).unwrap();
    let e = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [35, 32]).unwrap();
    let f = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [37, 32]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    still(&mut sim, &[mage, a, b, d, e, f]);
    mana(&mut sim, mage, 200, 200);
    study(&mut m, &c, spell::FIRE_RAIN);
    // Three knights against two of its own: not twice as strong.
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, mage, a),
        Intent::Attack(a)
    );
    // The target attacking the mage (0x5ab8fc): as strong is enough.
    sim.gameplay.actors[a].intent = Some(Intent::Attack(mage));
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, mage, a),
        Intent::Cast {
            key: "spell_firerain".into(),
            cell: [36, 30],
            target: Some(a),
        }
    );
    // A knight on its way does not count (op 9): one hostile left still.
    sim.gameplay.actors[a].intent = None;
    for u in [b, d] {
        set_order(&mut sim, u, Intent::Move([10, 50]), false).unwrap();
    }
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    still(&mut sim, &[mage, a, e, f]);
    assert!(sim.gameplay.actors[b].movement.is_some());
    sim.gameplay.actors[a].intent = Some(Intent::Attack(mage));
    assert_ne!(
        units::target_program(&mut sim, &m, &lib, &c, mage, a),
        Intent::Cast {
            key: "spell_firerain".into(),
            cell: [36, 30],
            target: Some(a),
        }
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_computer_unit_reads_its_book_and_its_attacked_mage_runs() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let mage = new_unit(&mut sim, &mut m, &lib, class::MAGE, 2, 1, [30, 30]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [33, 30]).unwrap();
    let book = new_unit(&mut sim, &mut m, &lib, class::ITEM_MAGIC, 0, 0, [1, 1]).unwrap();
    m.objects[book].data.resize(110, 0);
    m.objects[book].data[96] = spell::LIGHTNING;
    mana(&mut sim, book, 5, 5);
    garrison(&mut sim, &mut m, mage, book);
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    still(&mut sim, &[mage, knight]);
    mana(&mut sim, mage, 0, 200);
    // The knight attacking the mage: the charged book of lightning, though
    // the mage has no mana (0x414cd0, 0x42ea30).
    sim.gameplay.actors[knight].intent = Some(Intent::Attack(mage));
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, mage, knight),
        Intent::UseItem {
            item: book,
            target: Some(knight),
            cell: [33, 30],
        }
    );
    // The book not charged and no refuge: five cells away from the knight,
    // resting 100 ms (0x414c8b).
    mana(&mut sim, book, 4, 5);
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, mage, knight),
        Intent::Flee([25, 30], 100)
    );
    // A castle within 24 takes it in (0x413f50).
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [24, 34]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    still(&mut sim, &[mage, knight]);
    sim.gameplay.actors[knight].intent = Some(Intent::Attack(mage));
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, mage, knight),
        Intent::Board(castle)
    );
    // Not attacked, it attacks.
    sim.gameplay.actors[knight].intent = None;
    assert_eq!(
        units::target_program(&mut sim, &m, &lib, &c, mage, knight),
        Intent::Attack(knight)
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_casting_computer_mage_bears_blows_while_its_mana_holds() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let mage = new_unit(&mut sim, &mut m, &lib, class::MAGE, 2, 1, [30, 30]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [33, 30]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    still(&mut sim, &[mage, knight]);
    let rain = Intent::Cast {
        key: "spell_firerain".into(),
        cell: [36, 36],
        target: None,
    };
    // At the spell op with mana + 50 reaching the cost of 100 (0x4137d6).
    sim.gameplay.actors[mage].intent = Some(rain.clone());
    mana(&mut sim, mage, 50, 200);
    assert!(!retaliate(&mut sim, &m, &lib, &c, mage, knight));
    assert_eq!(sim.gameplay.actors[mage].intent, Some(rain.clone()));
    // Below, it takes the knight on (0x413800).
    mana(&mut sim, mage, 49, 200);
    assert!(retaliate(&mut sim, &m, &lib, &c, mage, knight));
    assert_eq!(
        sim.gameplay.actors[mage].intent,
        Some(Intent::Attack(knight))
    );
    // On its way to the point, whatever its mana (0x41379f).
    let far = Intent::Cast {
        key: "spell_firerain".into(),
        cell: [60, 60],
        target: None,
    };
    sim.gameplay.actors[mage].intent = Some(far);
    mana(&mut sim, mage, 200, 200);
    assert!(retaliate(&mut sim, &m, &lib, &c, mage, knight));
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_kingdom_runs_from_its_own_fire_rain() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let mage = new_unit(&mut sim, &mut m, &lib, class::MAGE, 2, 1, [30, 30]).unwrap();
    let own = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [37, 30]).unwrap();
    let foe = new_unit(&mut sim, &mut m, &lib, 8, 1, 1, [35, 31]).unwrap();
    let out = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [40, 30]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    still(&mut sim, &[mage, own, foe, out]);
    // Where they were made, should their work have moved them.
    for (u, p) in [(own, [37u16, 30u16]), (foe, [35, 31]), (out, [40, 30])] {
        (m.objects[u].x, m.objects[u].y) = (p[0], p[1]);
    }
    // 0x472760 at the point (36, 30): its peasant within 3 runs five cells
    // from it and rests 100 ms; the enemy and the peasant beyond stay.
    spells::clear_area(
        &mut sim,
        &m,
        &g,
        &lib,
        &c,
        mage,
        "spell_firerain",
        [36, 30],
        None,
    );
    assert_eq!(
        sim.gameplay.actors[own].intent,
        Some(Intent::Flee([42, 30], 100))
    );
    assert_eq!(sim.gameplay.actors[foe].intent, None);
    assert_eq!(sim.gameplay.actors[out].intent, None);
    // A frozen one is not commanded (0x506c54).
    sim.gameplay.actors[own].intent = None;
    let until = sim.ticks + 100;
    add_timed_effect(
        &mut sim.gameplay.actors[own],
        Effect {
            kind: effect::FREEZE,
            until,
            ..Default::default()
        },
    );
    spells::clear_area(
        &mut sim,
        &m,
        &g,
        &lib,
        &c,
        mage,
        "spell_firerain",
        [36, 30],
        None,
    );
    assert_eq!(sim.gameplay.actors[own].intent, None);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_computer_ninja_hides_setting_off_in_advance() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let ninja = new_unit(&mut sim, &mut m, &lib, class::NINJA, 2, 3, [30, 30]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    // A plain move: nothing (0x4ebe78).
    mana(&mut sim, ninja, 100, 100);
    set_order(&mut sim, ninja, Intent::Move([40, 30]), false).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert!(sim.gameplay.actors[ninja].effect_until <= sim.ticks);
    // Advancing with 90 mana it hides, paying the cloak (0x4ebe97).
    let cost = spell_parameters(&c, &m, &m.objects[ninja], "spell_cloak").1;
    mana(&mut sim, ninja, 90, 100);
    set_order(&mut sim, ninja, Intent::Move([40, 40]), false).unwrap();
    sim.gameplay.actors[ninja].move_mode = 1;
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert!(sim.gameplay.actors[ninja].effect_until > sim.ticks);
    assert!(sim.objects[ninja].mana <= 90 - cost);
    // With 89 it does not.
    sim.gameplay.actors[ninja].effect_until = 0;
    mana(&mut sim, ninja, 89, 100);
    set_order(&mut sim, ninja, Intent::Move([20, 40]), false).unwrap();
    sim.gameplay.actors[ninja].move_mode = 1;
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    assert!(sim.gameplay.actors[ninja].effect_until <= sim.ticks);
}

#[test]
#[ignore = "Requires installed original archives"]
fn an_idle_computer_priest_heals_the_nearest_wounded() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let priest = new_unit(&mut sim, &mut m, &lib, class::PRIEST, 2, 1, [30, 30]).unwrap();
    let near = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [34, 30]).unwrap();
    let far = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [30, 40]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    study(&mut m, &c, spell::HEAL);
    // Mana short of all its mana: no bless (0x414e19).
    mana(&mut sim, priest, 50, 100);
    for u in [near, far] {
        m.objects[u].health -= 10;
    }
    // One time in two (0x414f1f), the nearer of the two.
    let mut healed = 0;
    for _ in 0..40 {
        still(&mut sim, &[priest, near, far]);
        if priest_idle(&mut sim, &mut m, &g, &lib, &c, priest) {
            healed += 1;
            assert_eq!(
                sim.gameplay.actors[priest].intent,
                Some(Intent::Cast {
                    key: "spell_heal".into(),
                    cell: [34, 30],
                    target: Some(near),
                })
            );
        }
    }
    assert!((5..=35).contains(&healed), "{healed}");
    // Healed, the other one.
    m.objects[near].health = m.objects[near].max_health;
    let mut other = None;
    for _ in 0..40 {
        still(&mut sim, &[priest, near, far]);
        if priest_idle(&mut sim, &mut m, &g, &lib, &c, priest) {
            other = sim.gameplay.actors[priest].intent.clone();
        }
    }
    assert_eq!(
        other,
        Some(Intent::Cast {
            key: "spell_heal".into(),
            cell: [30, 40],
            target: Some(far),
        })
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn an_idle_computer_priest_teaches_or_blesses() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    quiet(&mut sim, &c);
    let priest = new_unit(&mut sim, &mut m, &lib, class::PRIEST, 2, 3, [30, 30]).unwrap();
    let pupil = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 3, [33, 30]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, 8, 2, 1, [28, 30]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    study(&mut m, &c, spell::RAISE_EXP);
    study(&mut m, &c, spell::BLESS);
    sim.objects[priest].level = 2;
    mana(&mut sim, priest, 100, 100);
    // Nobody wounded: half the time the Asian peasant of a lower level is
    // taught (0x4151fb).
    // (The player may have set it to work in the tick: where it stands.)
    let at = [m.objects[pupil].x, m.objects[pupil].y];
    let mut taught = 0;
    for _ in 0..40 {
        still(&mut sim, &[priest, pupil, knight]);
        if priest_idle(&mut sim, &mut m, &g, &lib, &c, priest) {
            taught += 1;
            assert_eq!(
                sim.gameplay.actors[priest].intent,
                Some(Intent::Cast {
                    key: "spell_raiseexp".into(),
                    cell: at,
                    target: Some(pupil),
                })
            );
        }
    }
    assert!((5..=35).contains(&taught), "{taught}");
    // A European priest cannot teach (0x415194): with all its mana it
    // blesses the nearest fighter not blessed (0x4152b7, 0x414e40), first
    // itself, a priest being one (flag 0x10000 of 0x4ea380).
    let european = new_unit(&mut sim, &mut m, &lib, class::PRIEST, 2, 1, [30, 34]).unwrap();
    tick(&mut sim, &mut m, &mut g, &lib, &c);
    sim.objects[european].level = 2;
    mana(&mut sim, european, 100, 100);
    let blesses = |sim: &mut Simulation, m: &mut MissionWorld, target: usize| {
        let mut blessed = None;
        for _ in 0..40 {
            still(sim, &[european, priest, pupil, knight]);
            if priest_idle(sim, m, &g, &lib, &c, european) {
                blessed = sim.gameplay.actors[european].intent.clone();
            }
        }
        let x = &m.objects[target];
        assert_eq!(
            blessed,
            Some(Intent::Cast {
                key: "spell_bless".into(),
                cell: [x.x, x.y],
                target: Some(target),
            })
        );
    };
    blesses(&mut sim, &mut m, european);
    // Itself and the other priest blessed, the knight.
    for u in [european, priest] {
        let until = sim.ticks + 1000;
        add_timed_effect(
            &mut sim.gameplay.actors[u],
            Effect {
                kind: effect::BLESS,
                until,
                ..Default::default()
            },
        );
    }
    blesses(&mut sim, &mut m, knight);
    // Short of all its mana, nothing.
    mana(&mut sim, european, 99, 100);
    for _ in 0..40 {
        still(&mut sim, &[european, pupil, knight]);
        assert!(!priest_idle(&mut sim, &mut m, &g, &lib, &c, european));
    }
}
