//! Independent native arithmetic fixtures and integration regressions.
use super::tests::{request, setup, step};
use super::*;

#[test]
#[ignore = "Requires installed original archives"]
fn native_bonus_matrix_overwrites_overlapping_groups_and_hidden_targets_are_rejected() {
    let (c, lib, mut m, g, mut s) = setup(1);
    sync(&mut s, &mut m, &lib);
    // Out of the castle's sight, which starts from its centre (0x50208b).
    let priest = new_unit(&mut s, &mut m, &lib, 18, 1, 1, [34, 34]).unwrap();
    let mage = new_unit(&mut s, &mut m, &lib, 17, 2, 2, [35, 34]).unwrap();
    // mage is also in magic_creatures; native 0x499ff1 overwrites the
    // earlier direct mage entry, rather than applying both +300% rules.
    assert_eq!(
        c.received(&m.objects[priest], &m.objects[mage], 20),
        80
    );
    assert!(
        issue(
            &mut s,
            &mut m,
            &g,
            &lib,
            &c,
            &request("attack", vec![priest], [35, 34], Some(mage))
        )
        .is_err()
    );
    m.game_rules.no_fog = true;
    assert!(
        issue(
            &mut s,
            &mut m,
            &g,
            &lib,
            &c,
            &request("attack", vec![priest], [35, 34], Some(mage))
        )
        .is_ok()
    );
    s.gameplay.actors[mage].effect_until = s.ticks + 100;
    assert!(
        issue(
            &mut s,
            &mut m,
            &g,
            &lib,
            &c,
            &request("attack", vec![priest], [35, 34], Some(mage))
        )
        .is_err()
    );
    s.gameplay.actors[mage].effect_until = 0;
    s.objects[mage].holder = Some(0);
    assert!(
        issue(
            &mut s,
            &mut m,
            &g,
            &lib,
            &c,
            &request("attack", vec![priest], [35, 34], Some(mage))
        )
        .is_err()
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn native_ore_extraction_preserves_yield_and_halves_depletion() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/native_simulation_rules.json"
    ))
    .unwrap();
    for civ in 1..=3 {
        for row in fixture["ore"].as_array().unwrap() {
            let (c, lib, mut m, mut g, mut s) = setup(civ);
            if row["yield_amount"] == 12 {
                m.rules[0].initial_researches.push("te_goldplus".into());
            }
            if row["percent"] == 50 {
                m.rules[0]
                    .initial_researches
                    .push("te_orefloatation".into());
            }
            let j = new_unit(&mut s, &mut m, &lib, class::RES_GOLD, 0, 0, [15, 9]).unwrap();
            position(&mut m.objects[1], [14, 9]);
            m.objects[j].data.resize(64, 0);
            m.objects[j].data[58..62]
                .copy_from_slice(&(row["before"].as_u64().unwrap() as u32).to_le_bytes());
            work(&mut s, &mut m, &mut g, &lib, &c, 1, Intent::Harvest(j));
            assert_eq!(s.gameplay.actors[1].cargo, [0; 4]);
            s.gameplay.actors[1].work_cooldown = 0;
            work(&mut s, &mut m, &mut g, &lib, &c, 1, Intent::Harvest(j));
            assert_eq!(
                s.gameplay.actors[1].cargo[0] as u64,
                row["load"].as_u64().unwrap()
            );
            assert_eq!(
                m.objects[j].dword(58) as u64,
                row["after"].as_u64().unwrap()
            );
        }
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn native_bribe_waits_for_arrival_and_charges_only_native_gold_cost() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/native_simulation_rules.json"
    ))
    .unwrap();
    for row in fixture["bribes"].as_array().unwrap() {
        let (c, lib, mut m, mut g, mut s) = setup(1);
        m.game_rules.no_fog = true;
        m.rules[0].initial_researches.push("te_bribery".into());
        let j = new_unit(&mut s, &mut m, &lib, class::FOOTMAN, 2, 2, [30, 30]).unwrap();
        s.objects[j].level = row["level"].as_u64().unwrap() as u16;
        let before = m.kingdoms[0].resources;
        issue(
            &mut s,
            &mut m,
            &g,
            &lib,
            &c,
            &request("bribe", vec![1], [30, 30], Some(j)),
        )
        .unwrap();
        assert_eq!(m.kingdoms[0].resources, before);
        assert_eq!(m.objects[j].owner, 2);
        position(&mut m.objects[1], [29, 30]);
        work(&mut s, &mut m, &mut g, &lib, &c, 1, Intent::Bribe(j));
        assert_eq!(m.objects[j].owner, 1);
        assert_eq!(m.objects[j].civilization, 2);
        let mut expected = before;
        expected[0] -= row["gold"].as_u64().unwrap() as u32;
        assert_eq!(m.kingdoms[0].resources, expected);
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn native_ship_upgrades_change_firing_period_without_experience_or_damage() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/native_simulation_rules.json"
    ))
    .unwrap();
    let assets = GameAssets::open(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap(),
    )
    .unwrap();
    let skin = crate::hud::HudSkin::load(&assets).unwrap();
    let locale = Ini::default();
    for civ in 1..=3 {
        for class in 23..=25 {
            let (c, lib, mut m, mut g, mut s) = setup(civ);
            let i = new_unit(&mut s, &mut m, &lib, class, 1, civ, [30, 30]).unwrap();
            let original = (
                s.objects[i].attack,
                s.objects[i].level,
                m.objects[i].word(28),
            );
            assert_eq!(
                ship_period(&s, &m, i),
                if class == 25 { 1300 } else { 3000 }
            );
            for row in fixture["ship_upgrades"].as_array().unwrap().iter().take(5) {
                let next = row["after_level"].as_u64().unwrap() as u8;
                let name = format!("to_ship_level{next}");
                assert!(ship_upgrade_available(&s, i, &name));
                assert!(!ship_upgrade_available(
                    &s,
                    i,
                    &format!("to_ship_level{}", next + 1)
                ));
                if class != 25 {
                    m.simulation = Some(s.clone());
                    let hud = crate::hud::HudState {
                        selected: vec![i],
                        ..Default::default()
                    };
                    let upgrades: Vec<_> = crate::hud::commands(&skin, &locale, &hud, &m, &lib)
                        .into_iter()
                        .filter(|cmd| cmd.order.starts_with("to_ship_level"))
                        .map(|cmd| cmd.order)
                        .collect();
                    assert_eq!(upgrades, vec![name.clone()]);
                    issue(
                        &mut s,
                        &mut m,
                        &g,
                        &lib,
                        &c,
                        &request(&name, vec![i], [30, 30], None),
                    )
                    .unwrap();
                    s.gameplay.actors[i].jobs.front_mut().unwrap().remaining = 1;
                    s.gameplay.actors[i].jobs.front_mut().unwrap().due_ms = Some(0);
                } else {
                    // Xebecs expose the terminal menu; this checks only their
                    // inherited native handler, not player-accessible upgrades.
                    assert!(!c.permitted(&name, &m.objects[i], &lib));
                    s.gameplay.actors[i].jobs.push_back(Job {
                        kind: JobKind::Upgrade(name, None),
                        remaining: 1,
                        total: 1,
                        paid: [0; 4],
                        due_ms: Some(0),
                        waiting: false,
                    });
                }
                production(&mut s, &mut m, &g, &lib, &c, i);
                assert_eq!(ship_level(&s, i), next);
                assert_eq!(ship_period(&s, &m, i), row["period_ms"].as_u64().unwrap());
                assert_eq!(
                    (
                        s.objects[i].attack,
                        s.objects[i].level,
                        m.objects[i].word(28)
                    ),
                    original
                );
                // The cannon fires once per native period, without an animation.
                let period = row["period_ms"].as_u64().unwrap().div_ceil(TICK_MS);
                s.ticks += 1000;
                s.gameplay.actors[i].last_hit_ms = 0;
                attack(&mut s, &mut m, &g, &lib, &c, i, 0);
                let first = s.gameplay.actors[i].last_hit_ms;
                assert!(first > 0);
                s.ticks += period - 1;
                attack(&mut s, &mut m, &g, &lib, &c, i, 0);
                assert_eq!(s.gameplay.actors[i].last_hit_ms, first);
                s.ticks += 1;
                attack(&mut s, &mut m, &g, &lib, &c, i, 0);
                assert!(s.gameplay.actors[i].last_hit_ms > first);
            }
            s.gameplay.actors[0].jobs.push_back(Job {
                kind: JobKind::Research("te_architecture".into()),
                remaining: 1,
                total: 1,
                paid: [0; 4],
                due_ms: None,
                waiting: false,
            });
            step(&mut s, &mut m, &mut g, &lib, &c, 1);
            assert_eq!(ship_period(&s, &m, i), 800);
            let restored: Simulation =
                serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
            assert_eq!(ship_period(&restored, &m, i), 800);
            assert_eq!(
                (
                    s.objects[i].attack,
                    s.objects[i].level,
                    m.objects[i].word(28)
                ),
                original
            );
        }
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn native_capture_transfers_passengers_and_population_technology_adds_a_quarter() {
    let (_c, lib, mut m, _g, mut s) = setup(1);
    sync(&mut s, &mut m, &lib);
    s.objects[0].contents.push(1);
    s.objects[1].holder = Some(0);
    let civ = m.objects[0].civilization;
    transfer_owner(&mut s, &mut m, &lib, 0, 2);
    assert_eq!(m.objects[0].civilization, civ);
    assert_eq!((m.objects[0].owner, m.objects[1].owner), (2, 2));
    assert_eq!(s.objects[1].holder, Some(0));
    assert_eq!(m.objects[1].byte(12), 2);
    m.game_rules.population = 4;
    assert_eq!(population(&m, &lib, 2).1, 4);
    m.rules[1]
        .initial_researches
        .push("te_supplystorage".into());
    // 0x4d3819: a quarter of the game's population limit, 4 + 4 × 25 / 100.
    assert_eq!(population(&m, &lib, 2).1, 5);
}

#[test]
fn native_physical_damage_matches_125_executed_x86_cases() {
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/native_simulation_rules.json"
    ))
    .unwrap();
    for row in data["damage"].as_array().unwrap() {
        assert_eq!(
            physical_damage(
                row["attack"].as_u64().unwrap() as u16,
                row["defence"].as_u64().unwrap() as u32,
                row["category"].as_u64().unwrap() as u8
            ) as u64,
            row["expected"].as_u64().unwrap(),
            "{row}"
        );
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn native_research_changes_time_and_live_future_wall_health() {
    for civ in 1..=3 {
        let (c, lib, m, _g, s) = setup(civ);
        assert_eq!(
            c.stats(&m.objects[0], "castle", &["te_architecture".into()])
                .time_ms,
            84000
        );
        let mut state = s.objects[0].clone();
        state.apply_researches(&m.objects[0], &["te_architecture".into()]);
        assert_eq!(
            state.resistance,
            lib.attributes.get(&m.objects[0]).resistance
        );
        state.apply_researches(&m.objects[0], &["te_budhism".into()]);
        assert_eq!(state.resistance, 1500);
    }
    let (c, lib, mut m, mut g, mut s) = setup(3);
    sync(&mut s, &mut m, &lib);
    let wall = new_unit(&mut s, &mut m, &lib, class::WALL_STONE, 1, 3, [25, 25]).unwrap();
    m.objects[wall].health = 1000;
    s.gameplay.actors[0].jobs.push_back(Job {
        kind: JobKind::Research("te_greatwall".into()),
        remaining: 1,
        total: 1,
        paid: [0; 4],
        due_ms: Some(0),
        waiting: false,
    });
    step(&mut s, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(
        (m.objects[wall].health, m.objects[wall].max_health),
        (1300, 5200)
    );
    let next = new_unit(&mut s, &mut m, &lib, class::WALL_STONE, 1, 3, [28, 25]).unwrap();
    assert_eq!(
        (m.objects[next].health, m.objects[next].max_health),
        (5200, 5200)
    );
    change_class(&mut s, &mut m, &lib, next, class::GATE_STONE);
    assert_eq!(m.objects[next].max_health, 3900);
    assert_eq!(m.objects[next].word(20), 3900);
}

#[test]
#[ignore = "Requires installed original archives"]
fn native_capture_matches_x86_pulses_and_preserves_civilization() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/native_simulation_rules.json"
    ))
    .unwrap();
    for civ in 1..=3 {
        for row in fixture["capture"].as_array().unwrap() {
            let (c, lib, mut m, mut g, mut s) = setup(civ);
            // Away from the player's castle and its EMIT aura (−200), which
            // would lower the target the pulse is clamped to.
            let j = new_unit(&mut s, &mut m, &lib, class::CASTLE, 2, 2, [40, 40]).unwrap();
            position(&mut m.objects[1], [39, 40]);
            s.objects[j].resistance = row["resistance"].as_u64().unwrap() as u16;
            s.gameplay.actors[1].cooldown = 20;
            issue(
                &mut s,
                &mut m,
                &g,
                &lib,
                &c,
                &request("capture", vec![1], [40, 40], Some(j)),
            )
            .unwrap();
            advance(
                &mut s,
                &mut m,
                &mut g,
                &lib,
                &c,
                Duration::from_millis(50),
                10,
                false,
            );
            let remaining = row["after_pulse"].as_u64().unwrap();
            if remaining == 0 {
                assert_eq!(s.objects[j].resistance, 0);
                assert_eq!(m.objects[j].owner, 1);
                assert_eq!(m.objects[j].civilization, 2);
                assert_eq!(m.objects[j].byte(12), 1);
            } else {
                assert_eq!(s.objects[j].resistance as u64, remaining);
                assert_eq!(m.objects[j].owner, 2);
            }
        }
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn native_mana_and_population_are_independent_of_frame_path_and_holder() {
    for active in [false, true] {
        for concentration in [false, true] {
            let (c, lib, mut m, mut g, mut s) = setup(1);
            sync(&mut s, &mut m, &lib);
            change_class(&mut s, &mut m, &lib, 1, 17);
            s.objects[1].mana = 0;
            s.objects[1].holder = Some(0);
            s.objects[0].contents.push(1);
            if concentration {
                m.rules[0]
                    .initial_researches
                    .push("te_concentration".into());
            }
            if active {
                s.gameplay.actors[0].jobs.push_back(Job {
                    kind: JobKind::Research("te_feudalism".into()),
                    remaining: 10000,
                    total: 10000,
                    paid: [0; 4],
                    due_ms: None,
                    waiting: false,
                });
            }
            step(&mut s, &mut m, &mut g, &lib, &c, 2);
            assert_eq!(s.objects[1].mana, if concentration { 12 } else { 4 });
            // Housing starts at 1 (0x4671b0) and the castle adds its 20.
            assert_eq!(population(&m, &lib, 1), (3, 21));
            let cow = new_unit(&mut s, &mut m, &lib, class::COW, 1, 1, [30, 30]).unwrap();
            assert_eq!(population(&m, &lib, 1).0, 3);
            assert_eq!(m.objects[cow].class, class::COW);
        }
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn native_holder_damage_uses_category_and_hurts_passengers() {
    let (c, lib, mut m, g, mut s) = setup(1);
    sync(&mut s, &mut m, &lib);
    s.objects[0].contents.push(1);
    s.objects[1].holder = Some(0);
    s.objects[0].defence = 0;
    s.objects[1].defence = 0;
    let before = (m.objects[0].health, m.objects[1].health);
    apply_damage(&mut s, &mut m, &g, &lib, &c, 0, 100, 0, None);
    assert_eq!(m.objects[0].health, before.0 - 90);
    assert_eq!(m.objects[1].health, before.1 - 10);
    let ship = new_unit(&mut s, &mut m, &lib, 23, 1, 1, [30, 30]).unwrap();
    assert!(accepts(&lib, &c, &m.objects[ship], &m.objects[1]));
    assert!(!accepts(&lib, &c, &m.objects[ship], &m.objects[ship]));
    assert_eq!(holder_capacity(&s, &m, &lib, ship), 24);
    m.rules[0]
        .initial_researches
        .push("te_landingtactics".into());
    // 0x4fffc4: the room is fixed when a holder is made.
    assert_eq!(holder_capacity(&s, &m, &lib, ship), 24);
    let later = new_unit(&mut s, &mut m, &lib, 23, 1, 1, [34, 30]).unwrap();
    assert_eq!(holder_capacity(&s, &m, &lib, later), 36);
}

#[test]
#[ignore = "Requires installed original archives"]
fn native_work_progress_is_pulsed_and_snapshot_bounds_are_checked() {
    let (c, lib, mut m, mut g, mut s) = setup(1);
    sync(&mut s, &mut m, &lib);
    let j = new_unit(&mut s, &mut m, &lib, class::WALL_STONE, 1, 1, [15, 9]).unwrap();
    position(&mut m.objects[1], [14, 9]);
    m.objects[j].flags |= FLAG_CONSTRUCTION;
    m.objects[j].health = 10;
    s.gameplay.actors[j].construction = 60;
    s.gameplay.actors[j].construction_total = 120;
    s.gameplay.actors[1].intent = Some(Intent::Build(j));
    advance(
        &mut s,
        &mut m,
        &mut g,
        &lib,
        &c,
        Duration::from_millis(50),
        10,
        false,
    );
    assert_eq!(m.objects[j].health, 10);
    step(&mut s, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(s.gameplay.actors[j].construction, 40);
    assert_eq!(m.objects[j].health, 2666);
    let mut broken = s.gameplay.clone();
    broken.actors[j].construction = 121;
    assert!(!broken.validate(m.objects.len(), 64));
    let restored: Simulation = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    assert_eq!(restored.gameplay, s.gameplay);
}
