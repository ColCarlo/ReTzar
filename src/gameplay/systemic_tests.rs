use super::tests::{setup, step};
use super::*;

#[test]
#[ignore = "Requires installed original archives"]
fn systemic_work_faces_all_eight_targets_and_sounds_follow_native_action_rows() {
    for civ in 1..=3 {
        let (c, lib, mut m, mut g, mut sim) = setup(civ);
        sync(&mut sim, &mut m, &lib);
        position(&mut m.objects[1], [30, 30]);
        for (direction, point) in [
            [30, 31],
            [31, 31],
            [31, 30],
            [31, 29],
            [30, 29],
            [29, 29],
            [29, 30],
            [29, 31],
        ]
        .into_iter()
        .enumerate()
        {
            sim.gameplay.actors[1].direction = (direction as u8 + 4) % 8;
            face_area(&mut sim, &m, &lib, 1, point, [1, 1]);
            assert_eq!(
                sim.gameplay.actors[1].direction, direction as u8,
                "civ {civ}"
            );
        }
        let mine = new_unit(&mut sim, &mut m, &lib, class::RES_GOLD, 0, 0, [31, 30]).unwrap();
        set_order(&mut sim, 1, Intent::Harvest(mine), false).unwrap();
        sim.sounds.clear();
        let section = lib.property(&m.objects[1], "image_mine").unwrap();
        let rows = lib.images.get(section, "rows").unwrap().parse().unwrap();
        let timeline = crate::animation::Timeline::directional(&lib.images, section, rows);
        let row: usize = lib
            .images
            .get(section, "actionrow")
            .unwrap()
            .parse()
            .unwrap();
        let hit = timeline.ends()[row - 1];
        // Actual runtime harvesting, before the first six-cycle load.
        advance(
            &mut sim,
            &mut m,
            &mut g,
            &lib,
            &c,
            Duration::from_secs(3),
            10,
            false,
        );
        let expected = (3000 - 1 - hit) / timeline.duration_ms + 1;
        assert_eq!(
            sim.sounds.iter().filter(|e| e.property == "@mine").count(),
            expected as usize
        );
        assert_eq!(sim.gameplay.actors[1].action_elapsed_ms, 3000);
        // 0x4fd450 aims at the centre of the mine, below and to the right:
        // south-east.
        assert_eq!(sim.gameplay.actors[1].direction, 1);
        assert_eq!(sim.gameplay.actors[1].cargo, [0; 4]);
        let before = sim.gameplay.actors[1].action_elapsed_ms;
        let cooldown = sim.gameplay.actors[1].work_cooldown;
        sim.gameplay.actors[1].frozen_until = sim.ticks + 40;
        advance(
            &mut sim,
            &mut m,
            &mut g,
            &lib,
            &c,
            Duration::from_secs(1),
            10,
            false,
        );
        assert_eq!(sim.gameplay.actors[1].action_elapsed_ms, before);
        assert_eq!(sim.gameplay.actors[1].work_cooldown, cooldown);
        let copy: Runtime =
            serde_json::from_slice(&serde_json::to_vec(&sim.gameplay).unwrap()).unwrap();
        assert_eq!(copy.actors[1], sim.gameplay.actors[1]);
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn systemic_groups_queue_in_every_barracks_and_cancel_only_the_requested_slot() {
    for civ in 1..=3 {
        let (c, lib, mut m, mut g, mut sim) = setup(civ);
        sync(&mut sim, &mut m, &lib);
        // The orders reach several producers, though the player can select
        // only one building at a time.
        let mut selected = Vec::new();
        for p in [[20, 20], [35, 20], [20, 35]] {
            selected.push(new_unit(&mut sim, &mut m, &lib, 45, 1, civ, p).unwrap());
        }
        assert_eq!(selected.len(), 3);
        let order = match civ {
            1 => "e_footman",
            2 => "r_footman",
            _ => "a_rhonin",
        };
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command(order, selected.clone()),
        )
        .unwrap();
        issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &Request::command(order, selected.clone()),
        )
        .unwrap();
        assert!(
            selected
                .iter()
                .all(|&i| sim.gameplay.actors[i].jobs.len() == 2)
        );
        let paid = sim.gameplay.actors[selected[1]].jobs[1].paid;
        let resources = m.kingdoms[0].resources;
        let mut cancel = Request::command("cancel_job", vec![selected[1]]);
        cancel.target = Some(1);
        issue(&mut sim, &mut m, &g, &lib, &c, &cancel).unwrap();
        assert_eq!(sim.gameplay.actors[selected[1]].jobs.len(), 1);
        assert_eq!(
            m.kingdoms[0].resources,
            std::array::from_fn(|i| resources[i] + paid[i])
        );
        let before = m.objects.len();
        step(&mut sim, &mut m, &mut g, &lib, &c, 120);
        assert_eq!(m.objects.len(), before + 5);
        assert_eq!(
            sim.sounds
                .iter()
                .filter(|e| e.property == "@notify/unitdone.wav")
                .count(),
            5
        );
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn systemic_delivery_uses_a_reachable_store_and_waits_without_walking_when_blocked() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    position(&mut m.objects[1], [25, 25]);
    let near = new_unit(&mut sim, &mut m, &lib, 51, 1, 1, [28, 25]).unwrap();
    let size = footprint(&lib, &m.objects[near]);
    for y in 23..27 + size[1] {
        for x in 26..30 + size[0] {
            g.cells[y * g.side + x] = 0;
        }
    }
    sim.gameplay.actors[1].cargo = [10, 0, 0, 0];
    let before = m.kingdoms[0].resources;
    movement::refresh(&mut sim, &m, &g, &lib);
    deposit(&mut sim, &mut m, &g, &lib, 1);
    assert_eq!(sim.gameplay.actors[1].depositing, Some(0));
    for _ in 0..1000 {
        sim.ticks += 1;
        deposit(&mut sim, &mut m, &g, &lib, 1);
    }
    assert_eq!(sim.gameplay.actors[1].cargo, [0; 4]);
    assert_eq!(m.kingdoms[0].resources[0], before[0] + 10);
    position(&mut m.objects[1], [55, 55]);
    for y in 54..=56 {
        for x in 54..=56 {
            if (x, y) != (55, 55) {
                g.cells[y * g.side + x] = 0;
            }
        }
    }
    movement::refresh(&mut sim, &m, &g, &lib);
    halt(&mut sim.gameplay.actors[1]);
    sim.gameplay.actors[1].path_cooldown = 0;
    // Nothing nearer than the start: the Move fails at once (0x4df364).
    assert!(!approach(&mut sim, &mut m, &g, &lib, 1, [50, 50], 0));
    assert!(unreachable(&sim, 1));
    assert!(sim.gameplay.actors[1].path.is_empty());
    let _ = c;
}

#[test]
#[ignore = "Requires installed original archives"]
fn systemic_action_groups_and_notifications_resolve_to_real_original_waves() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let assets = GameAssets::open(root).unwrap();
    let lib = ObjectLibrary::load(&assets, "grass").unwrap();
    let archive = crate::archive::Archive::open(&root.join("Packs/sounds.wdt")).unwrap();
    for group in [
        "mine",
        "chop",
        "build",
        "dig",
        "summon_spell",
        "notify/unitdone.wav",
        "notify/techdone.wav",
        "notify/bldgdone.wav",
        "notify/upgrdone.wav",
        "spells/heal.wav",
        "spells/bless.wav",
        "spells/freeze.wav",
        "spells/invisible.wav",
        "spells/lightning.wav",
        "spells/flame ball.wav",
        "spells/ressurect.wav",
    ] {
        let sounds = lib.sound_group(group);
        assert!(!sounds.is_empty(), "{group}");
        for (path, weight) in sounds {
            assert!(weight > 0);
            let pcm = crate::audio::Pcm::decode(&archive.get(&path).unwrap()).unwrap();
            assert!(pcm.seconds() > 0., "{path}");
        }
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn an_idle_peasant_takes_up_the_work_of_a_neighbour_after_30_seconds() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let cell = |x: usize, y: usize| y * 64 + x;
    g.cells[cell(30, 32)] = 0x90 | 4 << 11;
    position(&mut m.objects[1], [30, 30]);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &super::tests::request("context", vec![1], [30, 32], None),
    )
    .unwrap();
    let idle = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 1, [33, 30]).unwrap();
    let digger = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 1, [31, 28]).unwrap();
    let field = new_unit(&mut sim, &mut m, &lib, class::FIELD, 1, 1, [36, 26]).unwrap();
    m.objects[field].flags &= !FLAG_CONSTRUCTION;
    sim.gameplay.actors[digger].intent = Some(Intent::Harvest(field));
    let ticks = |sim: &mut Simulation, m: &mut MissionWorld, g: &mut MapGrid, n: u64| {
        for _ in 0..n {
            advance(
                sim,
                m,
                g,
                &lib,
                &c,
                Duration::from_millis(TICK_MS),
                10,
                false,
            );
        }
    };
    // 0x4ae440: not before 30 s of idleness.
    ticks(&mut sim, &mut m, &mut g, 500);
    assert_eq!(sim.gameplay.actors[idle].intent, None);
    // Then the nearest peasant's job, never digging a field (0x44fa30).
    ticks(&mut sim, &mut m, &mut g, 200);
    assert_eq!(
        sim.gameplay.actors[idle].intent,
        Some(Intent::Chop([30, 32]))
    );
}

/// `n` ticks of 50 ms, calling `each` before every one.
fn run(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    n: u64,
    mut each: impl FnMut(&mut Simulation, &mut MissionWorld),
) {
    for _ in 0..n {
        each(sim, m);
        advance(sim, m, g, lib, c, Duration::from_millis(TICK_MS), 10, false);
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn bears_wander_from_their_wait_and_keep_to_their_attacks() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let bear = new_unit(&mut sim, &mut m, &lib, 26, 0, 0, [45, 45]).unwrap();
    // Bear Idle (0x42fea0, 0x4eeed0): a wait that ends on R(0, 100) ≤ 20,
    // then a cell within 6 of the bear, walked as an advancing move.
    let mut walks = Vec::new();
    run(&mut sim, &mut m, &mut g, &lib, &c, 1200, |sim, m| {
        let a = &sim.gameplay.actors[bear];
        if let Some(Intent::Move(cell)) = a.intent
            && walks
                .last()
                .is_none_or(|w: &([u16; 2], [u16; 2], u8)| w.0 != cell)
        {
            walks.push((cell, [m.objects[bear].x, m.objects[bear].y], a.move_mode));
        }
    });
    assert!(walks.len() >= 3, "{walks:?}");
    for (cell, from, mode) in &walks {
        assert!(cell[0].abs_diff(from[0]) <= 6 && cell[1].abs_diff(from[1]) <= 6);
        assert_eq!(*mode, 1);
    }
    // An attack is no longer given up halfway.
    let footman = new_unit(&mut sim, &mut m, &lib, 3, 1, 1, [40, 45]).unwrap();
    m.objects[footman].max_health = 60000;
    m.objects[footman].health = 60000;
    set_order(&mut sim, bear, Intent::Attack(footman), false).unwrap();
    run(&mut sim, &mut m, &mut g, &lib, &c, 400, |_, _| {});
    assert_eq!(
        sim.gameplay.actors[bear].intent,
        Some(Intent::Attack(footman))
    );
    assert!(m.objects[footman].health < 60000);
}

#[test]
#[ignore = "Requires installed original archives"]
fn cows_graze_then_wander_around_where_they_were_left() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    // The pictures time the Idle op (+0x68 of the animation record).
    assert_eq!(section_ms(&lib, "cow_idle"), 2600);
    assert_eq!(section_ms(&lib, "cow_down"), 1000);
    assert_eq!(section_ms(&lib, "cow_eat"), 1000);
    let cow = new_unit(&mut sim, &mut m, &lib, class::COW, 1, 1, [45, 45]).unwrap();
    run(&mut sim, &mut m, &mut g, &lib, &c, 1, |_, _| {});
    let p = sim.gameplay.actors[cow].pasture.unwrap();
    assert_eq!((p.home, p.left, p.picture), ([45, 45], 30, 0));
    assert_eq!(p.due_ms, p.since_ms + 2600);
    // Grazing: head down, eating, head up, never two steps at once.
    let mut seen = [false; 3];
    let mut walk = None;
    run(&mut sim, &mut m, &mut g, &lib, &c, 3000, |sim, m| {
        let a = &sim.gameplay.actors[cow];
        if let Some(Intent::Move(cell)) = a.intent {
            walk.get_or_insert((cell, [m.objects[cow].x, m.objects[cow].y]));
        } else if let Some(p) = a.pasture
            && walk.is_none()
        {
            seen[p.picture as usize] = true;
        }
    });
    assert_eq!(seen, [true; 3]);
    // Thirty cycles last at least 30 s; then a cell within 2 of home.
    let (cell, _) = walk.unwrap();
    assert!(cell[0].abs_diff(45) <= 2 && cell[1].abs_diff(45) <= 2);
    assert_eq!(sim.gameplay.actors[cow].pasture.unwrap().home, [45, 45]);
    // An order makes the program again where the cow ends up.
    set_order(&mut sim, cow, Intent::Move([52, 45]), false).unwrap();
    run(&mut sim, &mut m, &mut g, &lib, &c, 200, |_, _| {});
    assert_eq!(sim.gameplay.actors[cow].pasture.unwrap().home, [52, 45]);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_milked_cow_keeps_grazing_where_it_stands() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let cow = new_unit(&mut sim, &mut m, &lib, class::COW, 1, 1, [45, 45]).unwrap();
    let peon = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 1, [46, 45]).unwrap();
    let mut refills = 0;
    let mut left = 30;
    run(&mut sim, &mut m, &mut g, &lib, &c, 2400, |sim, _| {
        let a = &mut sim.gameplay.actors[peon];
        a.intent = Some(Intent::Harvest(cow));
        a.work_target = Some(cow);
        assert!(sim.gameplay.actors[cow].intent.is_none());
        let Some(p) = sim.gameplay.actors[cow].pasture else {
            return;
        };
        if p.left > left {
            refills += 1;
            assert_eq!(p.left, 4);
        }
        left = p.left;
    });
    // 0x4ecd95: 4 more cycles each time the milked cow's run out.
    assert!(refills >= 2, "{refills}");
}

#[test]
#[ignore = "Requires installed original archives"]
fn destroyed_buildings_leave_rubble_that_crumbles_away() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let rubble = |m: &MissionWorld| {
        (0..m.objects.len())
            .rev()
            .find(|&k| m.objects[k].class == class::DEBRIS)
            .unwrap()
    };
    // 0x41dba0: `debris<W>x<H>` on the building's cell, a frame every 2 s
    // from frame 0, gone past the last (0x444bd0).
    let barrack = new_unit(&mut sim, &mut m, &lib, class::BARRACK, 2, 1, [40, 40]).unwrap();
    m.objects[barrack].flags &= !FLAG_CONSTRUCTION;
    let [w, h] = footprint(&lib, &m.objects[barrack]);
    kill(&mut sim, &mut m, &g, &lib, barrack);
    let d = rubble(&m);
    assert_eq!(m.objects[d].custom_image, format!("debris{w}x{h}"));
    assert_eq!(
        [m.objects[d].x, m.objects[d].y, m.objects[d].variant],
        [40, 40, 0]
    );
    run(&mut sim, &mut m, &mut g, &lib, &c, 40, |_, _| {});
    assert_eq!(m.objects[d].variant, 1);
    run(&mut sim, &mut m, &mut g, &lib, &c, 80, |_, _| {});
    assert_eq!(m.objects[d].variant, 3);
    assert_eq!(m.objects[d].flags & FLAG_INACTIVE, 0);
    run(&mut sim, &mut m, &mut g, &lib, &c, 40, |_, _| {});
    assert_ne!(m.objects[d].flags & FLAG_INACTIVE, 0);
    // 0x4fbd10: a wall's rubble lasts.
    let wall = new_unit(&mut sim, &mut m, &lib, class::WALL_WOOD, 2, 1, [20, 40]).unwrap();
    m.objects[wall].flags &= !FLAG_CONSTRUCTION;
    kill(&mut sim, &mut m, &g, &lib, wall);
    let d = rubble(&m);
    assert_eq!(m.objects[d].custom_image, "walls_wood_debris");
    run(&mut sim, &mut m, &mut g, &lib, &c, 400, |_, _| {});
    assert_eq!(m.objects[d].flags & FLAG_INACTIVE, 0);
    // 0x41bf00: the units on a fallen bridge go with it.
    let bridge = new_unit(&mut sim, &mut m, &lib, class::BRIDGE, 1, 1, [10, 10]).unwrap();
    m.objects[bridge].flags &= !FLAG_CONSTRUCTION;
    let walker = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [11, 11]).unwrap();
    kill(&mut sim, &mut m, &g, &lib, bridge);
    assert!(!alive(&m.objects[walker]));
    assert_eq!(m.objects[rubble(&m)].custom_image, "bridge_horz_demol");
}

#[test]
#[ignore = "Requires installed original archives"]
fn ships_burn_by_their_health_like_buildings() {
    let (_, lib, mut m, _, mut sim) = setup(1);
    for class in [23, 24, 25, class::FISHING_BOAT] {
        let ship = new_unit(&mut sim, &mut m, &lib, class, 1, 1, [30, 30]).unwrap();
        let max = m.objects[ship].max_health as u32;
        let at = |sim: &mut Simulation, m: &mut MissionWorld, pct: u32| {
            m.objects[ship].health = (max * pct / 100).max(1) as u16;
            burn(sim, m, &lib, ship);
            let a = &sim.gameplay.actors[ship];
            (a.fire_level, a.fires.map(|f| f.is_some()), a.fire_due_ms)
        };
        // 0x4e89e0: one small fire to 36%, two to 16%, then a big one,
        // with no sound of their own.
        assert_eq!(at(&mut sim, &mut m, 60), (0, [false; 3], None));
        assert_eq!(at(&mut sim, &mut m, 40), (1, [true, false, false], None));
        assert_eq!(at(&mut sim, &mut m, 20), (2, [true, true, false], None));
        assert_eq!(at(&mut sim, &mut m, 10), (3, [true; 3], None));
        // Repaired, they go out again (vt+0x1d4 = 0x4202a0).
        assert_eq!(at(&mut sim, &mut m, 90), (0, [false; 3], None));
    }
}
