use super::super::tests::setup;
use super::*;
use crate::script::{Named, Rule};

fn named<T>(name: &str, value: T) -> Named<T> {
    Named {
        name: name.into(),
        value,
    }
}
fn rule(name: &str, condition: &str, actions: &[&str]) -> Rule {
    Rule {
        name: name.into(),
        condition: condition.into(),
        actions: actions.iter().map(|s| (*s).into()).collect(),
    }
}
fn ids(ids: &[u16]) -> Selector {
    Selector {
        ids: ids.to_vec(),
        ..Default::default()
    }
}
/// `n` ticks of 50 ms.
fn run_ticks(
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

#[test]
#[ignore = "Requires installed original archives"]
fn rules_timers_and_waits_keep_the_native_phases() {
    let (c, lib, mut m, mut g, _) = setup(1);
    // Kingdom 2 keeps a castle, so nobody wins meanwhile.
    let mut used = m.objects.iter().map(|o| o.id).collect();
    m.objects.push(
        world::starting::new_object(&mut used, 100, class::CASTLE, 2, 1, [40, 40], &lib).unwrap(),
    );
    let mut sim = Simulation::new(&m, &lib, 64, "grass");
    m.script = Script {
        actions: vec![
            named(
                "tiempoa",
                Action::SetTimer {
                    init: "0".into(),
                    countdown: false,
                },
            ),
            named(
                "cuenta",
                Action::SetTimer {
                    init: "2500".into(),
                    countdown: true,
                },
            ),
            named("hit", Action::Expression("$hit = $hit + 1".into())),
            named(
                "Story",
                Action::Story {
                    text: "C'era una volta".into(),
                    image: "USER\\libro.bmp".into(),
                    sound: String::new(),
                    music: String::new(),
                    time: "42000".into(),
                    close: false,
                },
            ),
            named(
                "Quest1",
                Action::AddQuest {
                    text: "Costruisci".into(),
                    portrait: String::new(),
                },
            ),
            named("Remove quest1", Action::RemoveQuest("Quest1".into())),
            named("keep", Action::KeepRule),
            named("count", Action::Expression("$n = $n + 1".into())),
            named("wait", Action::Delay("500".into())),
        ],
        rules: vec![
            rule("start", "1", &["tiempoa", "cuenta"]),
            // Tested at 1, 1001, …: the timer reaches 1000 at 1001 first.
            rule("phase", "tiempoa==1000", &["hit"]),
            rule("story", "1", &["Story", "Quest1", "wait", "count"]),
            rule("again", "$n >= 1 && $n < 3", &["count", "keep"]),
            rule("unknown", "1", &["missing action", "hit"]),
            rule("remove", "$n == 3", &["Remove quest1"]),
        ],
        ..Default::default()
    };
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    let st = &sim.gameplay.script;
    assert!(st.modal);
    // The story screen (0x4c34c0) with its picture, time and closing.
    assert_eq!(
        st.outbox[0],
        Show::Story {
            sound: String::new(),
            ms: 42000,
            close: false,
            music: String::new(),
        }
    );
    let message = m.message.as_ref().unwrap();
    assert_eq!(message.text, "C'era una volta");
    assert_eq!(message.portrait, "USER\\libro.bmp");
    // The story waits until its window is closed, and the window stops the
    // game (0x43ed40 from 0x4c3538): neither the timers nor the rules move.
    let (ticks, cuenta) = (sim.ticks, sim.gameplay.script.variables.map["cuenta"]);
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 30);
    assert!(m.quests.is_empty());
    assert_eq!(sim.ticks, ticks);
    assert_eq!(sim.gameplay.script.variables.map["cuenta"], cuenta);
    assert!(!sim.gameplay.script.variables.map.contains_key("$hit"));
    close_window(&mut sim, &mut m);
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 4);
    assert_eq!(m.quests.len(), 1);
    assert_eq!(m.quests[0].id, crate::script::quest_id("Quest1"));
    // `again` tests $n, which reads 0 until `count` runs.
    assert_eq!(sim.gameplay.script.variables.map["$n"], 0);
    // 500 ms of delay, then KeepRule repeats `again` every second.
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 10);
    assert_eq!(sim.gameplay.script.variables.map["$n"], 1);
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 60);
    let st = &sim.gameplay.script;
    assert_eq!(st.variables.map["$n"], 3);
    // The countdown stopped at 0; `phase` ran once; an unknown action name
    // ends the rule's list.
    assert_eq!(st.variables.map["cuenta"], -500);
    assert!(st.timers.iter().all(|t| t.name != "cuenta"));
    assert_eq!(st.variables.map["$hit"], 1);
    assert!(m.quests.is_empty());
    assert!(st.over.is_none());
}

#[test]
#[ignore = "Requires installed original archives"]
fn events_select_objects_and_actions_change_them() {
    let (c, lib, mut m, mut g, _) = setup(1);
    let mut used = m.objects.iter().map(|o| o.id).collect();
    let castle = m.objects.len();
    m.objects.push(
        world::starting::new_object(&mut used, 100, class::CASTLE, 2, 1, [40, 40], &lib).unwrap(),
    );
    let hidden = m.objects.len();
    let mut knight = world::starting::new_object(&mut used, 200, 8, 1, 1, [30, 30], &lib).unwrap();
    knight.flags |= FLAG_INACTIVE;
    knight.name = "Arturo".into();
    knight.flags |= FLAG_CARRIED;
    m.objects.push(knight);
    let mut sim = Simulation::new(&m, &lib, 64, "grass");
    let [castle_id, hidden_id, peon_id] =
        [m.objects[castle].id, m.objects[hidden].id, m.objects[1].id];
    let peons = |count: &str| Selector {
        by_class: true,
        class: class::PEON as u16,
        kingdoms: 1,
        count: count.into(),
        ..Default::default()
    };
    m.script = Script {
        events: vec![
            named("tres", Event::Exists(peons("3"))),
            named("cuatro", Event::Exists(peons("4"))),
            named(
                "dentro",
                Event::InArea(
                    ids(&[peon_id]),
                    Area {
                        rect: [10, 0, 20, 10],
                        ..Default::default()
                    },
                ),
            ),
            named("caido", Event::Destroyed(ids(&[castle_id]))),
            named(
                "herido",
                Event::Expression(ids(&[peon_id]), "$health < 10".into()),
            ),
        ],
        actions: vec![
            named(
                "a",
                Action::Expression("$a = tres && !cuatro && dentro".into()),
            ),
            named("arturo", Action::Appear(ids(&[hidden_id]))),
            named(
                "hurt",
                Action::SetProperty(ids(&[peon_id]), "$health = 5".into()),
            ),
            named("b", Action::Expression("$b = herido".into())),
            named("burn", Action::Destroy(ids(&[castle_id]))),
            named("c", Action::Expression("$c = caido".into())),
        ],
        rules: vec![rule("r", "1", &["a", "arturo", "hurt", "b", "burn", "c"])],
        ..Default::default()
    };
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    let v = &sim.gameplay.script.variables.map;
    assert_eq!((v["$a"], v["$b"], v["$c"]), (1, 1, 1));
    assert_eq!(m.objects[1].health, 5);
    assert_eq!(m.objects[hidden].flags & FLAG_INACTIVE, 0);
    assert!(!sim.gameplay.script.hidden.contains(&hidden));
    // The castle was kingdom 2's last major object: kingdom 1 wins.
    assert_eq!(
        sim.gameplay.script.lost[&(2 << 8 | class::CASTLE as u16)],
        1
    );
    assert!(sim.gameplay.script.kingdoms[2].defeated);
    assert_eq!(sim.gameplay.script.over.map(|o| o.0), Some(true));
    assert!(heroes(&sim, &m).iter().any(|h| h.name == "Arturo"));
}

#[test]
#[ignore = "Requires installed original archives"]
fn victory_follows_the_rules_and_the_status_of_each_kingdom() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    // Kingdom 2 owns nothing, but kingdom 1 cannot win by the rules.
    m.script = Script {
        actions: vec![
            named(
                "status",
                Action::Status {
                    kingdom: 1,
                    cannot_win: true,
                    cannot_lose: true,
                },
            ),
            named("next", Action::LoadMap("MAPS\\tutmap2.wmp".into())),
        ],
        rules: vec![
            rule("start", "1", &["status"]),
            rule("win", "$done", &["next"]),
        ],
        ..Default::default()
    };
    // 0x442d6c: a spy of kingdom 2, hidden in a unit of kingdom 1, keeps it
    // in the game.
    m.objects[1].mole = 2;
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 5);
    assert_eq!(spy_count(&m, 2), 1);
    assert!(!sim.gameplay.script.kingdoms[2].defeated);
    // 0x442e30 runs at events only: with the spy gone nothing tests
    // kingdom 2 until something dies (0x4de019).
    m.objects[1].mole = 0;
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 5);
    assert!(!sim.gameplay.script.kingdoms[2].defeated);
    super::super::kill(&mut sim, &mut m, &g, &lib, 3);
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 5);
    let st = &sim.gameplay.script;
    assert!(st.kingdoms[2].defeated && !st.kingdoms[1].defeated);
    assert!(st.over.is_none());
    sim.gameplay.script.variables.set("$done", 1);
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 20);
    let st = &sim.gameplay.script;
    assert_eq!(st.next_map.as_deref(), Some("MAPS\\tutmap2.wmp"));
    assert_eq!(st.over.map(|o| o.0), Some(true));
    assert!(st.kingdoms[1].won);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_script_giving_away_the_last_building_of_a_kingdom_ends_it() {
    let (c, lib, mut m, mut g, _) = setup(1);
    // Kingdom 2's only building, given to kingdom 1 by the script.
    m.objects[0].owner = 2;
    let mut sim = Simulation::new(&m, &lib, 64, "grass");
    let id = m.objects[0].id;
    m.script = Script {
        actions: vec![named("give", Action::Owner(ids(&[id]), 1))],
        rules: vec![rule("start", "1", &["give"])],
        ..Default::default()
    };
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 5);
    assert_eq!(m.objects[0].owner, 1);
    // 0x4e50cb: the change of owner is followed by the end test, as when
    // the player takes it.
    assert!(sim.gameplay.script.kingdoms[2].defeated);
}

#[test]
fn relations_merge_like_the_native_negotiation() {
    let mut m = MissionWorld {
        diplomacy: vec![0x10; 64],
        ..Default::default()
    };
    // One side asking for an alliance changes nothing.
    merge_relation(&mut m, 1, 2, 0x4000_0000 | 0x10);
    assert_eq!(m.diplomacy[1] & 0x70, 0x10);
    assert_eq!(m.diplomacy[8] & 0x70, 0x10);
    // Both asking: allied, sharing vision.
    let back = 0x4000_0000 | relation(&m, 2, 1) & 0x70;
    merge_relation(&mut m, 2, 1, back);
    assert_eq!(m.diplomacy[1] & 0x71, 0x41);
    assert_eq!(m.diplomacy[8] & 0x71, 0x41);
    // War needs one side.
    merge_relation(&mut m, 1, 2, 0x1000_0000 | 0x40);
    assert_eq!(m.diplomacy[1] & 0x70, 0x10);
    assert_eq!(m.diplomacy[8] & 0x70, 0x10);
}

#[test]
#[ignore = "Requires installed original archives"]
fn player_lose_keeps_only_the_text_after_the_bar() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    // 0x4ba2a0: «map|text»; 0x442af0 then wipes the map, the text stays for
    // the end window (0x5cb5a8).
    m.script = Script {
        actions: vec![named(
            "lose",
            Action::Lose(1, "MAPS\next.wmp|Merlino è caduto".into()),
        )],
        rules: vec![rule("start", "1", &["lose"])],
        ..Default::default()
    };
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 5);
    let st = &sim.gameplay.script;
    assert_eq!(st.end_text, "Merlino è caduto");
    assert_eq!(st.next_map, None);
    assert_eq!(st.over.map(|o| o.0), Some(false));
}

#[test]
#[ignore = "Requires installed original archives"]
fn view_actions_follow_an_object_or_glide_between_points() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let (index, id, at) = m
        .objects
        .iter()
        .enumerate()
        .find(|(_, o)| o.owner == 1 && o.selectable())
        .map(|(i, o)| (i, o.id, [o.x, o.y]))
        .unwrap();
    let rect = |r: [i16; 4], object: u16| Area {
        rect: r,
        object,
        ..Default::default()
    };
    m.script = Script {
        actions: vec![
            // 0x4bba60: the area's first corner and its object, for the
            // time of the text.
            named(
                "lock",
                Action::LockView("1000 + 2000".into(), rect([5, 6, 9, 9], id)),
            ),
            // 0x4bca60: the middle x1 + (x2 − x1 + 1) / 2, or the object's cell.
            named(
                "scroll",
                Action::ScrollView(rect([0, 0, 3, 4], 0), rect([0, 0, 0, 0], id)),
            ),
        ],
        rules: vec![rule("start", "1", &["lock", "scroll"])],
        ..Default::default()
    };
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    let shows = &sim.gameplay.script.outbox.0;
    assert!(shows.iter().any(|s| matches!(
        s,
        Show::Lock { cell: [5, 6], follow: Some(f), ms: 3000 } if *f == index
    )));
    assert!(shows.iter().any(|s| matches!(
        s,
        Show::Scroll { from: [2, 2], to } if *to == at
    )));
}

#[test]
#[ignore = "Requires installed original archives"]
fn settime_moves_the_day_clock_to_the_hour() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.script = Script {
        actions: vec![
            named(
                "set",
                Action::Expression("$a = @settime(18); $h = @gettime()".into()),
            ),
            named(
                "bad",
                Action::Expression("$b = @settime(24); $k = @gettime()".into()),
            ),
        ],
        rules: vec![rule("start", "1", &["set", "bad"])],
        ..Default::default()
    };
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    // 0x42b520: hour 18, minute 0; 24 is refused. Both give 1.
    let v = &sim.gameplay.script.variables.map;
    assert_eq!((v["$a"], v["$h"], v["$b"], v["$k"]), (1, 18, 1, 18));
    assert_eq!(sim.clock_minutes(), 18 * 60);
    // The clock goes on at its own steps of 3750 ms.
    while sim.elapsed.as_millis() < 3750 {
        run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    }
    assert_eq!(sim.clock_minutes(), 18 * 60 + 3);
}

#[test]
#[ignore = "Requires installed original archives"]
fn isselected_reads_the_local_players_selection() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let (index, id) = m
        .objects
        .iter()
        .enumerate()
        .find(|(_, o)| o.owner == 1 && o.selectable())
        .map(|(i, o)| (i, o.id))
        .unwrap();
    m.script = Script {
        events: vec![named(
            "chosen",
            Event::Expression(ids(&[id]), "$isselected == 1 && ($flags & 1) == 1".into()),
        )],
        actions: vec![named("seen", Action::Expression("$s = chosen".into()))],
        rules: vec![rule("r", "1", &["seen"])],
        ..Default::default()
    };
    // Flag 1 of the object: what the interface handed over.
    sim.gameplay.selected.insert(index);
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(sim.gameplay.script.variables.map["$s"], 1);
}

#[test]
#[ignore = "Requires installed original archives"]
fn count_and_inprogress_read_the_kingdoms_record_of_the_type() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let peons = m
        .objects
        .iter()
        .filter(|o| o.owner == 1 && o.class == class::PEON)
        .count() as i32;
    let site = new_unit(&mut sim, &mut m, &lib, class::BARRACK, 1, 1, [30, 30]).unwrap();
    m.objects[site].flags |= FLAG_CONSTRUCTION;
    m.script = Script {
        actions: vec![named(
            "n",
            Action::Expression(format!(
                "$p = @count(1, {}); $s = @count(1, {b}); $u = @inprogress(1, {b}); $x = @count(12, {})",
                class::PEON,
                class::PEON,
                b = class::BARRACK
            )),
        )],
        rules: vec![rule("r", "1", &["n"])],
        ..Default::default()
    };
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    // 0x4976a0: +0x2c8 counts the site with the others, +0x2cc the site
    // under way; kingdom 12 has no record.
    let v = &sim.gameplay.script.variables.map;
    assert_eq!((v["$p"], v["$s"], v["$u"], v["$x"]), (peons, 1, 1, 0));
}

#[test]
fn shared_victory_is_the_bit_of_the_kingdoms_own_relation() {
    let mut m = MissionWorld {
        diplomacy: vec![0x41; 64],
        ..Default::default()
    };
    // The diplomacy window writes the bit toward the others (0x447044),
    // where the win test does not look (0x4683a0)…
    m.diplomacy[1] |= 8;
    m.diplomacy[2] |= 8;
    assert!(!shared_victory(&m, 1));
    // …it reads R[p][p], set by the map's diplomacy (0x4405e6).
    m.diplomacy[0] |= 8;
    assert!(shared_victory(&m, 1));
    assert!(!shared_victory(&m, 2));
    m.diplomacy[9] |= 8;
    assert!(shared_victory(&m, 2));
}

#[test]
#[ignore = "Requires installed original archives"]
fn explore_and_spawn_keep_the_native_records() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let area = |rect: [i16; 4]| Area {
        rect,
        ..Default::default()
    };
    let explore =
        |name: &str, rect, time: &str| named(name, Action::Explore(area(rect), time.into()));
    m.kingdoms[1].civilization = 3;
    m.script = Script {
        actions: vec![
            // (w + h) / 4 = 5; time 0 lasts 4000 ms (0x435069).
            explore("wide", [10, 10, 19, 19], "0"),
            // 0 and 35 are out of 1–31: the class's `fog_range` (0x435070).
            explore("small", [30, 30, 30, 30], "1000"),
            explore("huge", [0, 0, 69, 69], "1000"),
            // Civilization 0 is the first block, the European, whatever
            // the kingdom's (0x4bbd1c, 0x497940).
            named(
                "spawn",
                Action::Spawn {
                    class: 3,
                    count: "1".into(),
                    owner: 2,
                    area: area([40, 40, 40, 40]),
                    civilization: 0,
                },
            ),
        ],
        rules: vec![rule("start", "1", &["wide", "small", "huge", "spawn"])],
        ..Default::default()
    };
    let before = m.objects.len();
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    let radii: Vec<_> = sim.gameplay.visions.iter().map(|v| v.radius).collect();
    assert_eq!(radii, [5, 8, 8]);
    let wide = &sim.gameplay.visions[0];
    assert_eq!(wide.cell, [14, 14]);
    assert!(wide.until >= sim.ticks + 3900 / TICK_MS);
    let spawned = &m.objects[before..];
    assert_eq!(spawned.len(), 1);
    assert_eq!((spawned[0].owner, spawned[0].civilization), (2, 1));
}

#[test]
#[ignore = "Requires installed original archives"]
fn obj_order_takes_the_native_points() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let unit = (0..m.objects.len())
        .find(|&i| m.objects[i].owner == 1 && world::class::is_unit(m.objects[i].class))
        .unwrap();
    let id = m.objects[unit].id;
    let order = |name: &str, object: u16, kind: u8| {
        named(
            name,
            Action::Order(
                ids(&[id]),
                Area {
                    around: true,
                    object,
                    ..Default::default()
                },
                kind,
            ),
        )
    };
    // An area on an object that is gone leaves the point at cell 0, 0
    // (0x4bb0c0): the unit sets off for the corner of the map.
    m.script = Script {
        actions: vec![order("gone", 0xfbff, crate::script::order::MOVE)],
        rules: vec![rule("start", "1", &["gone"])],
        ..Default::default()
    };
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    let a = &sim.gameplay.actors[unit];
    assert!(
        matches!(a.intent, Some(Intent::Move([0, 0]))) || a.goal == Some([0, 0]),
        "{:?} {:?}",
        a.intent,
        a.goal
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_conversation_says_its_lines_and_offers_its_choices_in_bubbles() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let unit = (0..m.objects.len())
        .find(|&i| m.objects[i].owner == 1 && world::class::is_unit(m.objects[i].class))
        .unwrap();
    let id = m.objects[unit].id;
    let phrase = |index, start, text: &str, next: &[u16]| crate::dialogue::Phrase {
        index,
        speaker: id,
        start,
        condition: String::new(),
        action: String::new(),
        voice: String::new(),
        text: text.into(),
        next: next.to_vec(),
    };
    m.conversations = vec![crate::dialogue::Conversation {
        name: "Prova".into(),
        phrases: vec![
            phrase(1, true, "Chi va là?", &[2, 3]),
            phrase(2, false, "Un amico.", &[]),
            phrase(3, false, "Nessuno.", &[]),
        ],
    }];
    m.script = Script {
        actions: vec![named("talk", Action::Conversation("prova".into()))],
        rules: vec![rule("start", "1", &["talk"])],
        ..Default::default()
    };
    // The one opening line is said in a bubble (0x4380f0).
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    let st = &mut sim.gameplay.script;
    assert!(st.modal);
    assert_eq!(st.outbox.0.first(), Some(&Show::Say));
    assert_eq!(st.conversation.as_ref().unwrap().phrase, Some(0));
    assert_eq!(m.message.as_ref().unwrap().text, "Chi va là?");
    st.outbox.0.clear();
    // Going on offers the two that follow, then says the one taken.
    answer(&mut sim, &mut m, &g, &lib, &c, 0);
    let st = &mut sim.gameplay.script;
    assert_eq!(st.outbox.0, vec![Show::Say]);
    assert_eq!(st.conversation.as_ref().unwrap().choices, vec![1, 2]);
    assert_eq!(
        m.message.as_ref().unwrap().choices,
        vec!["Un amico.".to_string(), "Nessuno.".to_string()]
    );
    st.outbox.0.clear();
    answer(&mut sim, &mut m, &g, &lib, &c, 1);
    assert_eq!(
        sim.gameplay.script.conversation.as_ref().unwrap().phrase,
        Some(2)
    );
    // Nothing follows: the bubbles close (0x438380).
    sim.gameplay.script.outbox.0.clear();
    answer(&mut sim, &mut m, &g, &lib, &c, 0);
    let st = &sim.gameplay.script;
    assert!(st.conversation.is_none() && !st.modal && m.message.is_none());
    assert_eq!(st.outbox.0, vec![Show::Close]);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_unit_of_owner_9_brought_in_by_the_script_waits_to_be_freed() {
    let (c, lib, mut m, mut g, _) = setup(1);
    let mut used = m.objects.iter().map(|o| o.id).collect();
    m.objects.push(
        world::starting::new_object(&mut used, 100, class::CASTLE, 2, 1, [40, 40], &lib).unwrap(),
    );
    // A mage of owner 9, who fights the players (0x4404ce), hidden beside
    // the player's peasant.
    let peon = 1;
    let at = [m.objects[peon].x + 2, m.objects[peon].y];
    let mage = CLASSES.iter().position(|&s| s == "mage").unwrap();
    let mut lady = world::starting::new_object(&mut used, 200, mage, 9, 1, at, &lib).unwrap();
    lady.flags |= FLAG_INACTIVE;
    let lady_id = lady.id;
    m.objects.push(lady);
    let lady = m.objects.len() - 1;
    let mut sim = Simulation::new(&m, &lib, 64, "grass");
    m.script = Script {
        actions: vec![named("dama", Action::Appear(ids(&[lady_id])))],
        rules: vec![rule("appare", "1", &["dama"])],
        ..Default::default()
    };
    // 0x4bc63c: brought in, she begins the Rescue program as the loader's
    // prisoners do (0x44092d), and fights nobody while she waits.
    let health = m.objects[peon].health;
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 60);
    assert_eq!(m.objects[lady].flags & FLAG_INACTIVE, 0);
    assert!(matches!(
        sim.gameplay.actors[lady].intent,
        Some(Intent::Rescue(_))
    ));
    assert_eq!(m.objects[peon].health, health);
    assert_eq!(m.objects[lady].owner, 9);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_building_counts_in_an_area_once_it_is_finished() {
    let (c, lib, mut m, mut g, _) = setup(1);
    let mut used = m.objects.iter().map(|o| o.id).collect();
    m.objects.push(
        world::starting::new_object(&mut used, 100, class::CASTLE, 2, 1, [40, 40], &lib).unwrap(),
    );
    let cathedral = CLASSES.iter().position(|&s| s == "cathedral").unwrap();
    let mut site =
        world::starting::new_object(&mut used, 200, cathedral, 1, 1, [20, 20], &lib).unwrap();
    site.flags |= FLAG_CONSTRUCTION;
    m.objects.push(site);
    let site = m.objects.len() - 1;
    let mut sim = Simulation::new(&m, &lib, 64, "grass");
    m.script = Script {
        events: vec![named(
            "abadia",
            Event::InArea(
                Selector {
                    by_class: true,
                    class: cathedral as u16,
                    kingdoms: 1,
                    count: "1".into(),
                    ..Default::default()
                },
                Area {
                    rect: [10, 10, 30, 30],
                    ..Default::default()
                },
            ),
        )],
        actions: vec![named("erigida", Action::Expression("$erigida = 1".into()))],
        rules: vec![rule("abadia", "abadia", &["erigida"])],
        ..Default::default()
    };
    // 0x4b9ad0: the area's filter asks 0x4b9970's question, so a site is
    // not there yet.
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 30);
    assert!(!sim.gameplay.script.variables.map.contains_key("$erigida"));
    m.objects[site].flags &= !FLAG_CONSTRUCTION;
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 30);
    assert_eq!(sim.gameplay.script.variables.map["$erigida"], 1);
}

#[test]
#[ignore = "Requires installed original archives"]
fn obj_order_commands_a_unit_of_any_owner() {
    let (c, lib, mut m, mut g, _) = setup(1);
    let mut used = m.objects.iter().map(|o| o.id).collect();
    m.objects.push(
        world::starting::new_object(&mut used, 100, class::CASTLE, 2, 1, [40, 40], &lib).unwrap(),
    );
    // An Asian ninja of owner 10, which has no kingdom record and no rules.
    let at = [m.objects[1].x + 4, m.objects[1].y];
    let ninja = CLASSES.iter().position(|&s| s == "ninja").unwrap();
    let unit = world::starting::new_object(&mut used, 200, ninja, 10, 3, at, &lib).unwrap();
    let id = unit.id;
    m.objects.push(unit);
    let unit = m.objects.len() - 1;
    let mut sim = Simulation::new(&m, &lib, 64, "grass");
    let to = [at[0] as i16 + 6, at[1] as i16 + 3];
    m.script = Script {
        actions: vec![named(
            "vai",
            Action::Order(
                ids(&[id]),
                Area {
                    rect: [to[0], to[1], to[0], to[1]],
                    ..Default::default()
                },
                crate::script::order::MOVE,
            ),
        )],
        rules: vec![rule("start", "1", &["vai"])],
        ..Default::default()
    };
    // 0x4bb090 sends the command by name (0x4909c0); its message 0x38
    // (0x48f751) runs it on any living object (vt+0x188), without the menu
    // of its class or the lists of its kingdom.
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    let a = &sim.gameplay.actors[unit];
    assert!(
        matches!(a.intent, Some(Intent::Move(p)) if p == [to[0] as u16, to[1] as u16]),
        "{:?}",
        a.intent
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_script_sets_health_above_the_maximum() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let peon = 1;
    let id = m.objects[peon].id;
    let max = m.objects[peon].max_health;
    m.script = Script {
        actions: vec![named(
            "forte",
            Action::SetProperty(ids(&[id]), format!("$health = {}", max + 225)),
        )],
        rules: vec![rule("start", "1", &["forte"])],
        ..Default::default()
    };
    // 0x4b8f50 stores the value at +0x5c with no bound (the kidnapper of
    // Borta in the first mission of Gengis Khan gets 300).
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    assert_eq!(m.objects[peon].health, max + 225);
    assert_eq!(m.objects[peon].max_health, max);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_script_reads_and_writes_attack_and_defence_without_the_level() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let at = [m.objects[1].x + 2, m.objects[1].y + 2];
    let unit = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, at).unwrap();
    add_levels(&mut sim, &mut m, &lib, unit, 4);
    let attack = sim.objects[unit].attack;
    let id = m.objects[unit].id;
    m.script = Script {
        actions: vec![named(
            "cid",
            Action::SetProperty(
                ids(&[id]),
                "$kept = $attack; $attack = 17; $defence = 8; $level = 1".into(),
            ),
        )],
        rules: vec![rule("start", "1", &["cid"])],
        ..Default::default()
    };
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    // 0x4b8b4f reads the record's +0x62, without the 4 points of level 4
    // the getter adds (0x4de7d0); the setter writes +0x62 and +0x60
    // (0x4b9003, 0x4b902c) and the level as is (0x4b9052): level 1 adds
    // 1 to each.
    assert_eq!(
        sim.gameplay.script.variables.get("$kept"),
        attack as i32 - 4
    );
    assert_eq!(
        (sim.objects[unit].attack, sim.objects[unit].defence),
        (18, 9)
    );
    assert_eq!(sim.objects[unit].level, 1);
}

#[test]
#[ignore = "Requires installed original archives"]
fn cargo_counts_what_is_aboard() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let at = [m.objects[1].x + 2, m.objects[1].y + 2];
    let castle = new_unit(
        &mut sim,
        &mut m,
        &lib,
        class::CASTLE,
        1,
        1,
        [at[0] + 6, at[1]],
    )
    .unwrap();
    let a = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 1, at).unwrap();
    let b = new_unit(
        &mut sim,
        &mut m,
        &lib,
        class::PEON,
        1,
        1,
        [at[0] + 1, at[1]],
    )
    .unwrap();
    let carrier = new_unit(
        &mut sim,
        &mut m,
        &lib,
        class::PEON,
        1,
        1,
        [at[0] + 2, at[1]],
    )
    .unwrap();
    garrison(&mut sim, &mut m, castle, a);
    garrison(&mut sim, &mut m, castle, b);
    sim.gameplay.actors[carrier].cargo = [0, 0, 0, 10];
    let [castle_id, carrier_id] = [castle, carrier].map(|i| m.objects[i].id);
    m.script = Script {
        actions: vec![
            named(
                "dentro",
                Action::SetProperty(ids(&[castle_id]), "$inside = $cargo".into()),
            ),
            named(
                "legna",
                Action::SetProperty(ids(&[carrier_id]), "$wood = $cargo".into()),
            ),
        ],
        rules: vec![rule("start", "1", &["dentro", "legna"])],
        ..Default::default()
    };
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    // 0x4b8c76: how many the holder's list holds (the hulks loaded with ten
    // each of the American campaign), not what a peasant carries.
    let v = &mut sim.gameplay.script.variables;
    assert_eq!((v.get("$inside"), v.get("$wood")), (2, 0));
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_script_finishes_a_site_as_its_builders_do() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    let at = [m.objects[1].x + 6, m.objects[1].y + 6];
    let house = new_unit(&mut sim, &mut m, &lib, class::HOUSE, 1, 1, at).unwrap();
    found_site(&mut sim, &mut m, &lib, &c, house);
    assert!(m.objects[house].flags & FLAG_CONSTRUCTION != 0);
    let id = m.objects[house].id;
    m.script = Script {
        actions: vec![named(
            "finisci",
            Action::SetProperty(ids(&[id]), "@build()".into()),
        )],
        rules: vec![rule("start", "1", &["finisci"])],
        ..Default::default()
    };
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    // 0x4b83a0 gives the site 999999 of building (vt+0x2b8): it ends as
    // finished sites do, with the player's notice of a building done.
    let o = &m.objects[house];
    assert!(o.flags & FLAG_CONSTRUCTION == 0 && o.health == o.max_health);
    assert!(
        sim.gameplay
            .events
            .iter()
            .any(|e| e.kind == 2 && e.object == house)
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_default_order_on_a_point_takes_what_lies_there() {
    let (c, lib, mut m, mut g, _) = setup(1);
    let mut used = m.objects.iter().map(|o| o.id).collect();
    m.objects.push(
        world::starting::new_object(&mut used, 100, class::CASTLE, 2, 1, [40, 40], &lib).unwrap(),
    );
    let peon = 1;
    let id = m.objects[peon].id;
    let at = [m.objects[peon].x + 3, m.objects[peon].y + 3];
    let field = world::starting::new_object(&mut used, 200, class::FIELD, 1, 1, at, &lib).unwrap();
    m.objects.push(field);
    let field = m.objects.len() - 1;
    let mut sim = Simulation::new(&m, &lib, 64, "grass");
    let cell = [at[0] as i16 + 1, at[1] as i16 + 1];
    m.script = Script {
        actions: vec![named(
            "lavora",
            Action::Order(
                ids(&[id]),
                Area {
                    rect: [cell[0], cell[1], cell[0], cell[1]],
                    ..Default::default()
                },
                crate::script::order::DEFAULT,
            ),
        )],
        rules: vec![rule("start", "1", &["lavora"])],
        ..Default::default()
    };
    // 0x4bb1a4: the right click at the point (vt+0x190); the peasant's
    // handler takes the building over the cell first (0x4adc8d), here a
    // field it digs.
    run_ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    let a = &sim.gameplay.actors[peon];
    assert_eq!(a.intent, Some(Intent::Harvest(field)), "{:?}", a.intent);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_carried_hero_takes_its_level_down_as_up_and_the_latest_unit_first() {
    use crate::world::FLAG_CARRIED;
    let (_c, lib, mut m, _g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    let footman = |sim: &mut Simulation, m: &mut MissionWorld, at| {
        let i = new_unit(sim, m, &lib, class::FOOTMAN, 1, 1, at).unwrap();
        m.objects[i].name = "Lancillotto".into();
        m.objects[i].flags |= FLAG_CARRIED;
        i
    };
    let old = footman(&mut sim, &mut m, [20, 20]);
    let new = footman(&mut sim, &mut m, [22, 20]);
    sync(&mut sim, &mut m, &lib);
    sim.gameplay.actors[new].joined = sim.gameplay.actors[old].joined + 1;
    add_levels(&mut sim, &mut m, &lib, old, 10);
    add_levels(&mut sim, &mut m, &lib, new, 10);
    // 0x43ea20 walks each kingdom's unit list (K+0x56), the latest to join
    // first (0x507940): of two namesakes the newer is carried first.
    m.objects[new].health = 33;
    let carried = heroes(&sim, &m);
    assert_eq!(carried.len(), 2);
    assert_eq!((carried[0].health, carried[0].level), (33, 10));
    // 0x505ed0 writes the level as is (+0x66): a footman back at level 4
    // loses the 6 attack points the getter added (0x4de7d0); health and
    // maximum come as they were (0x440d31).
    let attack = sim.objects[old].attack;
    let hero = Hero {
        name: "LANCILLOTTO".into(),
        subtitle: String::new(),
        health: 50,
        max_health: 80,
        level: 4,
        promoted: false,
    };
    restore_heroes(&mut sim, &mut m, &lib, &[hero]);
    assert_eq!(sim.objects[old].level, 4);
    assert_eq!(sim.objects[old].attack, attack - 6);
    assert_eq!((m.objects[old].health, m.objects[old].max_health), (50, 80));
}
