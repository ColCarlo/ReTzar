//! Pictures against the original rules: facing (0x4fd450 and startcol),
//! fires of damaged buildings, the cast pose and building sites.

use super::tests::{request, setup};
use super::*;

fn ticks(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    n: u64,
) {
    for _ in 0..n {
        advance(sim, m, g, lib, c, Duration::from_millis(TICK_MS), 10, false);
    }
}
fn class_of(name: &str) -> usize {
    CLASSES.iter().position(|&s| s == name).unwrap()
}

#[test]
fn headings_use_the_native_octants() {
    // Table 0x591010 with 1:2-slope edges; the same cell gives west.
    assert_eq!(heading([17, 7], [10, 10]), 2);
    assert_eq!(heading([11, 11], [9, 10]), 1);
    assert_eq!(heading([10, 15], [10, 10]), 0);
    assert_eq!(heading([10, 5], [10, 10]), 4);
    assert_eq!(heading([5, 10], [10, 10]), 6);
    assert_eq!(heading([10, 10], [10, 10]), 6);
    assert_eq!(heading([12, 12], [10, 10]), 1);
    assert_eq!(heading([8, 8], [10, 10]), 5);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn knights_turn_to_their_target_with_the_sheets_start_column() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let knight = new_unit(&mut sim, &mut m, &lib, class_of("knight"), 1, 1, [30, 30]).unwrap();
    let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [31, 30]).unwrap();
    m.objects[foe].max_health = 5000;
    m.objects[foe].health = 5000;
    sim.gameplay.actors[foe].stance = 2;
    // e_knight_attack has startcol 2: a target due east shows column 4.
    let section = lib.property(&m.objects[knight], "image_attack").unwrap();
    assert_eq!(
        lib.images.get(section, "startcol").map(str::trim),
        Some("2")
    );
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("attack", vec![knight], [31, 30], Some(foe)),
    )
    .unwrap();
    for _ in 0..200 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if sim.gameplay.actors[knight]
            .action_image
            .starts_with("image_attack")
        {
            break;
        }
    }
    assert_eq!(sim.gameplay.actors[knight].direction, 4);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn buildings_burn_by_their_health() {
    let (_c, lib, mut m, _g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    let barrack = new_unit(&mut sim, &mut m, &lib, class_of("barrack"), 1, 1, [30, 30]).unwrap();
    m.objects[barrack].flags &= !FLAG_CONSTRUCTION;
    let max = m.objects[barrack].max_health as u32;
    let fires = |sim: &Simulation| sim.gameplay.actors[barrack].fires.iter().flatten().count();
    for (pct, expected) in [(40, 1), (20, 2), (10, 3), (60, 0)] {
        m.objects[barrack].health = (max * pct / 100) as u16;
        burn(&mut sim, &m, &lib, barrack);
        assert_eq!(fires(&sim), expected, "{pct}%");
    }
    // 0x41ff90: 31 pixels inside the footprint.
    m.objects[barrack].health = (max / 10) as u16;
    burn(&mut sim, &m, &lib, barrack);
    let [w, h] = footprint(&lib, &m.objects[barrack]).map(|n| n as i32 * 32);
    for [x, y] in sim.gameplay.actors[barrack].fires.iter().flatten() {
        assert!((30 * 32 + 31..=30 * 32 + w - 30).contains(x));
        assert!((30 * 32 + 31..=30 * 32 + h - 30).contains(y));
    }
    assert!(sim.gameplay.actors[barrack].fire_due_ms.is_some());
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn sites_show_their_stage_picture() {
    let (_c, lib, mut m, _g, mut sim) = setup(1);
    let house = new_unit(&mut sim, &mut m, &lib, class_of("house"), 1, 1, [30, 30]).unwrap();
    m.objects[house].flags |= FLAG_CONSTRUCTION;
    let [w, h] = lib.cell_footprint(&m.objects[house]);
    assert_eq!(
        lib.image_for(&m.objects[house]),
        Some(format!("build{w}x{h}"))
    );
    m.objects[house].flags &= !FLAG_CONSTRUCTION;
    assert_ne!(
        lib.image_for(&m.objects[house]),
        Some(format!("build{w}x{h}"))
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_finished_house_takes_a_random_column_of_its_sheet() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    // 0x460080: R(0, columns − 1) when the construction ends.
    let mut seen = BTreeSet::new();
    for k in 0..12u16 {
        let house = new_unit(
            &mut sim,
            &mut m,
            &lib,
            class::HOUSE,
            1,
            1,
            [10 + 4 * (k % 6), 20 + 4 * (k / 6)],
        )
        .unwrap();
        m.objects[house].flags |= FLAG_CONSTRUCTION;
        m.objects[house].health = 1;
        sim.gameplay.actors[house].construction = 1;
        sim.gameplay.actors[house].construction_total = 120;
        let at = [m.objects[house].x - 1, m.objects[house].y];
        position(&mut m.objects[1], at);
        sim.gameplay.actors[1].intent = Some(Intent::Build(house));
        for _ in 0..200 {
            advance(
                &mut sim,
                &mut m,
                &mut g,
                &lib,
                &c,
                Duration::from_millis(TICK_MS),
                10,
                false,
            );
            if m.objects[house].flags & FLAG_CONSTRUCTION == 0 {
                break;
            }
        }
        assert_eq!(m.objects[house].flags & FLAG_CONSTRUCTION, 0);
        assert!(m.objects[house].variant < 3);
        seen.insert(m.objects[house].variant);
    }
    assert!(seen.len() > 1);
}
