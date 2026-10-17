use super::*;
use retzar::world::{Layer, class};

#[test]
#[ignore = "Requires installed original assets; uses a headless ECS world"]
fn stationary_environment_pauses_and_never_reveals_draft_shared_vision() {
    let root = test_root("environment-stationary");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    let art = environment::Art::load(&app.world().resource::<Game>().assets).unwrap();
    for (cloud, shadow) in art.clouds.iter().zip(&art.shadows) {
        assert!(cloud.rgba.chunks_exact(4).any(|p| p[3] == 0));
        assert!(cloud.rgba.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
        for (c, s) in cloud.rgba.chunks_exact(4).zip(shadow.rgba.chunks_exact(4)) {
            assert_eq!(&c[..3], &[255, 255, 255]);
            assert_eq!(&s[..3], &[0, 0, 0]);
            assert_eq!(c[3], s[3]);
        }
    }
    app.insert_resource(mission::StoryOnOpen(false));
    app.world_mut().resource_mut::<MenuState>().setup = Some(retzar::world::GameSetup {
        options: Some(retzar::world::GameRules::default()),
        ..Default::default()
    });
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: None,
            mission: 2,
        },
        None,
    )
    .unwrap();
    app.update();
    // The computer kingdoms would change the world as time passes; the test
    // reads only the environment, so they sit still here.
    app.world_mut()
        .resource_mut::<Loaded>()
        .scene
        .as_mut()
        .unwrap()
        .mission
        .simulation
        .as_mut()
        .unwrap()
        .gameplay
        .ai
        .players
        .clear();
    // The enemy's body sprite, found again each time: a building covered by
    // the fog is drawn anew from memory (fakebuilding, 0x420520).
    let body = |app: &App, object: usize| {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        s.visuals
            .iter()
            .zip(&s.visual_entities)
            .find(|(v, _)| v.object == object && v.layer == Layer::Body)
            .map(|(_, &e)| *app.world().get::<Visibility>(e).unwrap())
    };
    let (objects, enemy, _entity, owner, player, original_diplomacy) = {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        let sim = s.mission.simulation.as_ref().unwrap();
        let v = s
            .visuals
            .iter()
            .position(|v| {
                v.layer == Layer::Body
                    && s.mission.objects.get(v.object).is_some_and(|o| {
                        o.selectable()
                            && (1..=8).contains(&o.owner)
                            && o.owner != s.mission.player()
                            && !sim.known(o)
                    })
            })
            .unwrap_or_else(|| {
                panic!(
                    "no unexplored opponent: {} player={} candidates={:?}",
                    s.title,
                    s.mission.player(),
                    s.mission
                        .objects
                        .iter()
                        .filter(|o| o.selectable() && o.owner != s.mission.player())
                        .take(12)
                        .map(|o| (o.id, o.owner, o.class, sim.known(o), sim.visible(o)))
                        .collect::<Vec<_>>()
                )
            });
        let i = s.visuals[v].object;
        (
            still_world(&s.mission),
            i,
            s.visual_entities[v],
            s.mission.objects[i].owner,
            s.mission.player(),
            s.mission.diplomacy.clone(),
        )
    };
    assert_eq!(
        body(&app, enemy).unwrap_or(Visibility::Hidden),
        Visibility::Hidden
    );
    let elapsed = |app: &App| {
        app.world()
            .resource::<Loaded>()
            .scene
            .as_ref()
            .unwrap()
            .mission
            .simulation
            .as_ref()
            .unwrap()
            .elapsed
    };
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(10));
    app.update();
    assert_eq!(elapsed(&app), Duration::from_secs(10));
    app.world_mut().resource_mut::<MissionUi>().hud.panel = Some(Panel::Diplomacy);
    app.update();
    assert_eq!(elapsed(&app), Duration::from_secs(10));
    app.world_mut().resource_mut::<MissionUi>().hud.panel = None;
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    assert_eq!(elapsed(&app), Duration::from_secs(10));
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::ZERO);
    let mut shared = original_diplomacy.clone();
    shared[(owner as usize - 1) * 8 + player as usize - 1] = 0x41;
    app.world_mut()
        .resource_mut::<MissionUi>()
        .hud
        .diplomacy_draft = Some(shared.clone());
    app.update();
    let visible = |app: &App| {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        s.mission
            .simulation
            .as_ref()
            .unwrap()
            .visible(&s.mission.objects[enemy])
    };
    assert!(
        !visible(&app),
        "an unaccepted diplomacy draft must not reveal information"
    );
    app.world_mut().resource_mut::<MissionUi>().hud.diplomacy = Some(shared);
    app.update();
    assert!(visible(&app));
    assert_eq!(
        body(&app, enemy).unwrap_or(Visibility::Hidden),
        Visibility::Inherited
    );
    app.world_mut().resource_mut::<MissionUi>().hud.diplomacy = Some(original_diplomacy);
    app.update();
    assert!(!visible(&app));
    let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
    assert!(
        s.mission
            .simulation
            .as_ref()
            .unwrap()
            .known(&s.mission.objects[enemy])
    );
    assert_eq!(
        body(&app, enemy).unwrap_or(Visibility::Hidden),
        if retzar::selection::unit(&s.mission.objects[enemy]) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        }
    );
    {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let sim = loaded
            .scene
            .as_mut()
            .unwrap()
            .mission
            .simulation
            .as_mut()
            .unwrap();
        sim.elapsed = sim.weather_interval(0).0 / 4 + Duration::from_secs(30);
    }
    app.update();
    let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
    assert_eq!(objects, still_world(&s.mission));
    assert_eq!(
        s.mission.simulation.as_ref().unwrap().weather,
        retzar::simulation::Weather::Rain
    );
    let mut visible_particles = 0;
    for e in &s.entities {
        if let Some(t) = app.world().get::<Transform>(*e)
            && t.translation.z == 600.
            && app.world().get::<Visibility>(*e) == Some(&Visibility::Inherited)
        {
            visible_particles += 1;
            assert!(
                s.mission
                    .simulation
                    .as_ref()
                    .unwrap()
                    .fog
                    .at(
                        (t.translation.x / 32.) as usize,
                        (-t.translation.y / 32.) as usize
                    )
                    .1
            );
        }
    }
    assert!(visible_particles > 0);
    app.world_mut()
        .resource_mut::<MenuState>()
        .open(Screen::Pause);
    app.world_mut()
        .resource_mut::<NextState<Mode>>()
        .set(Mode::Menu);
    app.update();
    let paused = elapsed(&app);
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(10));
    app.update();
    assert_eq!(elapsed(&app), paused);
    menu_ui::close_scene(app.world_mut());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires installed original assets; uses original HUD widgets"]
fn submenus_inventory_items_and_mission_prohibitions_are_inspectable_without_orders() {
    let root = test_root("environment-hud");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: None,
            mission: 3,
        },
        None,
    )
    .unwrap();
    app.update();
    let (unit, shield, book) = {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        let unit = s
            .mission
            .objects
            .iter()
            .position(|o| o.owner == s.mission.player() && o.class == class::PEON)
            .unwrap();
        let shield = s
            .mission
            .objects
            .iter()
            .position(|o| o.custom_image == "item_shield2")
            .unwrap();
        let book = s
            .mission
            .objects
            .iter()
            .position(|o| o.class == class::ITEM_MAGIC)
            .unwrap();
        (unit, shield, book)
    };
    app.world_mut().resource_mut::<MissionUi>().hud.selected = vec![unit];
    app.update();
    let submenu = {
        let w = app.world();
        let s = w.resource::<Loaded>().scene.as_ref().unwrap();
        let game = w.resource::<Game>();
        hud::commands(
            &game.classic,
            &game.skin.locale,
            &w.resource::<MissionUi>().hud,
            &s.mission,
            &s.library,
        )
        .iter()
        .position(|c| c.submenu.is_some())
        .unwrap()
    };
    regressions::regression_hud_click(&mut app, window, hud::HudAction::Command(submenu));
    assert!(
        app.world()
            .resource::<MissionUi>()
            .hud
            .command_menu
            .is_some()
    );
    regressions::regression_hud_click(&mut app, window, hud::HudAction::BackMenu);
    assert!(
        app.world()
            .resource::<MissionUi>()
            .hud
            .command_menu
            .is_none()
    );
    {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let s = loaded.scene.as_mut().unwrap();
        s.mission.simulation.as_mut().unwrap().objects[unit].contents = vec![shield, book];
    }
    let records = serde_json::to_vec(
        &app.world()
            .resource::<Loaded>()
            .scene
            .as_ref()
            .unwrap()
            .mission,
    )
    .unwrap();
    regressions::regression_hud_click(&mut app, window, hud::HudAction::Holder);
    assert_eq!(
        app.world().resource::<MissionUi>().hud.contents_owner,
        Some(unit)
    );
    // Two marked items (the native leave can leave two): the panel keeps
    // one checked button, the page's last marked (0x4a1cf0).
    {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let sim = loaded
            .scene
            .as_mut()
            .unwrap()
            .mission
            .simulation
            .as_mut()
            .unwrap();
        sim.objects[shield].active = true;
        sim.objects[book].active = true;
    }
    let checked: Vec<_> = {
        let w = app.world();
        let s = w.resource::<Loaded>().scene.as_ref().unwrap();
        let game = w.resource::<Game>();
        hud::widgets(
            &game.classic,
            &game.skin.locale,
            &w.resource::<MissionUi>().hud,
            hud::Layout {
                width: 800,
                height: 600,
                left: false,
            },
            &s.mission,
            &s.library,
        )
        .into_iter()
        .filter(|b| b.active && matches!(b.action, hud::HudAction::InspectContent(_)))
        .map(|b| b.action)
        .collect()
    };
    assert_eq!(checked, [hud::HudAction::InspectContent(book)]);
    regressions::regression_hud_click(&mut app, window, hud::HudAction::InspectContent(shield));
    assert_eq!(
        app.world().resource::<MissionUi>().hud.inspected_object(),
        Some(shield)
    );
    tap(&mut app, KeyCode::Escape);
    assert!(
        app.world()
            .resource::<MissionUi>()
            .hud
            .contents_owner
            .is_none()
    );
    // 0x49cc3e: an item holds nothing, so with a book selected the
    // inventory's button stays grey.
    {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let s = loaded.scene.as_mut().unwrap();
        let o = &s.mission.objects[book];
        let sim = s.mission.simulation.as_mut().unwrap();
        sim.fog.reveal(o.x as i32, o.y as i32, 2);
        sim.fog.remember();
    }
    app.world_mut().resource_mut::<MissionUi>().hud.selected = vec![book];
    app.update();
    let offered = {
        let w = app.world();
        let s = w.resource::<Loaded>().scene.as_ref().unwrap();
        let game = w.resource::<Game>();
        hud::widgets(
            &game.classic,
            &game.skin.locale,
            &w.resource::<MissionUi>().hud,
            hud::Layout {
                width: 800,
                height: 600,
                left: false,
            },
            &s.mission,
            &s.library,
        )
        .iter()
        .any(|b| b.action == hud::HudAction::Holder)
    };
    assert!(!offered);
    assert_eq!(
        records,
        serde_json::to_vec(
            &app.world()
                .resource::<Loaded>()
                .scene
                .as_ref()
                .unwrap()
                .mission
        )
        .unwrap()
    );
    // The original campaign's kingdom-specific bans filter construction submenus.
    let game = app.world().resource::<Game>();
    let assets = &game.assets;
    let lib = retzar::world::ObjectLibrary::load(assets, "grass").unwrap();
    let catalog = &app.world().resource::<MenuState>().catalog;
    let scenario = catalog
        .campaigns
        .iter()
        .flat_map(|c| &c.missions)
        .find(|m| {
            m.nested
                .as_deref()
                .is_some_and(|n| n.to_ascii_lowercase().starts_with("maps/1_b "))
        })
        .unwrap();
    let c = scenario.container().unwrap();
    let grid = retzar::map::MapGrid::read(&c, &scenario.header).unwrap();
    let mut m = retzar::world::MissionWorld::read(&c, &grid).unwrap();
    let index = 0;
    m.objects[index].class = class::PEON;
    m.objects[index].civilization = 1;
    m.objects[index].owner = 1;
    let state = hud::HudState {
        selected: vec![index],
        command_menu: Some("e_common".into()),
        ..default()
    };
    assert!(m.rules[0].forbidden_buildings.contains(&"castle".into()));
    assert!(
        !hud::commands(&game.classic, &game.skin.locale, &state, &m, &lib)
            .iter()
            .any(|c| c.order == "e_castle")
    );
    menu_ui::close_scene(app.world_mut());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires installed original assets; uses original HUD widgets"]
fn the_inventory_panel_lists_occupants_alone_and_a_click_lets_one_out() {
    let root = test_root("environment-inventory");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: None,
            mission: 3,
        },
        None,
    )
    .unwrap();
    app.update();
    // A unit of the player put inside one of the player's buildings.
    let (holder, unit) = {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let s = loaded.scene.as_mut().unwrap();
        let player = s.mission.player();
        let holder = s
            .mission
            .objects
            .iter()
            .position(|o| {
                o.owner == player
                    && o.building()
                    && s.library.property(o, "max_obj_in_holder").is_some()
            })
            .unwrap();
        let unit = s
            .mission
            .objects
            .iter()
            .position(|o| o.owner == player && o.class == class::PEON)
            .unwrap();
        let sim = s.mission.simulation.as_mut().unwrap();
        sim.objects[holder].contents.push(unit);
        sim.objects[unit].holder = Some(holder);
        s.mission.objects[unit].flags |= retzar::world::FLAG_HELD;
        (holder, unit)
    };
    app.world_mut().resource_mut::<MissionUi>().hud.selected = vec![holder];
    app.update();
    regressions::regression_hud_click(&mut app, window, hud::HudAction::Holder);
    // 0x4a1180: the occupants take the place of the commands, none on top.
    let widgets = {
        let w = app.world();
        let s = w.resource::<Loaded>().scene.as_ref().unwrap();
        let game = w.resource::<Game>();
        let ui = w.resource::<MissionUi>();
        hud::widgets(
            &game.classic,
            &game.skin.locale,
            &ui.hud,
            hud::Layout {
                width: 800,
                height: 600,
                left: false,
            },
            &s.mission,
            &s.library,
        )
    };
    assert!(
        widgets
            .iter()
            .any(|b| b.action == hud::HudAction::InspectContent(unit))
    );
    assert!(
        !widgets
            .iter()
            .any(|b| matches!(b.action, hud::HudAction::Command(_)))
    );
    // 0x4a1c8c → 0x490ad0: a click lets the unit out next to its holder.
    regressions::regression_hud_click(&mut app, window, hud::HudAction::InspectContent(unit));
    app.update();
    let loaded = app.world().resource::<Loaded>();
    let s = loaded.scene.as_ref().unwrap();
    let sim = s.mission.simulation.as_ref().unwrap();
    assert_eq!(sim.objects[unit].holder, None);
    assert!(!sim.objects[holder].contents.contains(&unit));
    assert_eq!(s.mission.objects[unit].flags & retzar::world::FLAG_HELD, 0);
}

#[test]
#[ignore = "Requires installed original assets; uses a headless ECS world"]
fn the_keypad_sets_the_game_speed_with_the_original_message() {
    let root = test_root("environment-speed");
    let (mut app, _window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: None,
            mission: 3,
        },
        None,
    )
    .unwrap();
    app.update();
    let speed = |app: &App| {
        let loaded = app.world().resource::<Loaded>();
        loaded
            .scene
            .as_ref()
            .unwrap()
            .mission
            .simulation
            .as_ref()
            .unwrap()
            .speed
    };
    // A new map starts at the normal speed (0x44153f).
    assert_eq!(speed(&app), 10);
    let press = |app: &mut App, key| {
        tap(app, key);
        app.update();
        (
            speed(app),
            app.world().resource::<MenuState>().notice.clone(),
        )
    };
    // `*` turbo, then back to the normal, not to the speed before
    // (0x4f5892); the message line names the level (0x43f154).
    let (s, notice) = press(&mut app, KeyCode::NumpadAdd);
    assert_eq!(
        (s, notice.as_str()),
        (15, "Velocità del gioco cambiata in: rapido")
    );
    let (s, notice) = press(&mut app, KeyCode::NumpadMultiply);
    assert_eq!(
        (s, notice.as_str()),
        (100, "Velocità del gioco cambiata in: Turbo")
    );
    let (s, _) = press(&mut app, KeyCode::NumpadSubtract);
    assert_eq!(s, 95);
    let (s, notice) = press(&mut app, KeyCode::NumpadMultiply);
    assert_eq!(
        (s, notice.as_str()),
        (10, "Velocità del gioco cambiata in: normale")
    );
    // The Options slider shows the game's speed.
    assert_eq!(app.world().resource::<MenuState>().settings.speed, 10);
}

#[test]
#[ignore = "Requires installed original assets; uses a headless ECS world"]
fn a_locked_view_is_a_film_without_panels_the_player_cannot_move() {
    let root = test_root("environment-film");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: None,
            mission: 3,
        },
        None,
    )
    .unwrap();
    app.update();
    let widgets = |app: &App| {
        let w = app.world();
        let s = w.resource::<Loaded>().scene.as_ref().unwrap();
        let game = w.resource::<Game>();
        hud::widgets(
            &game.classic,
            &game.skin.locale,
            &w.resource::<mission::MissionUi>().hud,
            hud::Layout {
                width: 800,
                height: 600,
                left: false,
            },
            &s.mission,
            &s.library,
        )
    };
    assert!(
        widgets(&app)
            .iter()
            .any(|b| b.action == hud::HudAction::Pause)
    );
    // A script's LockView on a unit (0x4f9140): the panels go (0x4f9990)
    // and the player does not move the view (0x4f6e40 while blocked).
    let unit = {
        let loaded = app.world().resource::<Loaded>();
        let s = loaded.scene.as_ref().unwrap();
        (0..s.mission.objects.len())
            .find(|&i| retzar::selection::own_unit(&s.mission.objects[i], &s.mission))
            .unwrap()
    };
    app.world_mut()
        .resource_mut::<mission::MissionUi>()
        .view_lock = Some(mission::ViewLock {
        follow: Some(unit),
        until: None,
    });
    app.update();
    assert!(app.world().resource::<mission::MissionUi>().hud.cinematic);
    assert!(widgets(&app).is_empty());
    let before = app.world().resource::<MapView>().center;
    move_to(&mut app, window, Vec2::new(1., 720.));
    for _ in 0..3 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(50));
        app.update();
    }
    assert_eq!(app.world().resource::<MapView>().center, before);
    // Nor does the wheel zoom: the zoom is this version's, and the view
    // does not answer the player at all.
    let wheel = |app: &mut App, lines: f32| {
        app.world_mut()
            .resource_mut::<bevy::input::mouse::AccumulatedMouseScroll>()
            .delta = Vec2::new(0., lines);
        app.update();
        app.world_mut()
            .resource_mut::<bevy::input::mouse::AccumulatedMouseScroll>()
            .delta = Vec2::ZERO;
    };
    let scale = app.world().resource::<MapView>().scale;
    move_to(&mut app, window, Vec2::new(1280., 720.));
    wheel(&mut app, -1.);
    let held = *app.world().resource::<MapView>();
    assert_eq!((held.center, held.scale), (before, scale));
    move_to(&mut app, window, Vec2::new(1., 720.));
    // UnlockView: the panels come back and the edge scrolls again.
    app.world_mut()
        .resource_mut::<mission::MissionUi>()
        .view_lock = None;
    for _ in 0..3 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(50));
        app.update();
    }
    assert!(!app.world().resource::<mission::MissionUi>().hud.cinematic);
    assert!(app.world().resource::<MapView>().center.x < before.x);
    // And the wheel zooms again.
    move_to(&mut app, window, Vec2::new(1280., 720.));
    wheel(&mut app, -1.);
    assert_ne!(app.world().resource::<MapView>().scale, scale);
}

#[test]
#[ignore = "Requires installed original assets; uses a headless ECS world"]
fn stories_crawl_on_their_screen_and_conversations_speak_in_bubbles() {
    use retzar::dialogue::{Conversation, Phrase};
    use retzar::script::{Action, Named, Rule, Script};
    let root = test_root("environment-story");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: None,
            mission: 3,
        },
        None,
    )
    .unwrap();
    app.update();
    let step = |app: &mut App, ms: u64| {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(ms));
        app.update();
    };
    let ui = |app: &App| app.world().resource::<mission::MissionUi>().hud.clone();
    let unit = {
        let loaded = app.world().resource::<Loaded>();
        let s = loaded.scene.as_ref().unwrap();
        (0..s.mission.objects.len())
            .find(|&i| retzar::selection::own_unit(&s.mission.objects[i], &s.mission))
            .unwrap()
    };
    {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let mission = &mut loaded.scene.as_mut().unwrap().mission;
        let id = mission.objects[unit].id;
        let phrase = |index, start, text: &str, next: &[u16]| Phrase {
            index,
            speaker: id,
            start,
            condition: String::new(),
            action: String::new(),
            voice: String::new(),
            text: text.into(),
            next: next.to_vec(),
        };
        mission.conversations = vec![Conversation {
            name: "Prova".into(),
            phrases: vec![
                phrase(1, true, "Chi va là?", &[2, 3]),
                phrase(2, false, "Un amico.", &[]),
                phrase(3, false, "Nessuno.", &[]),
            ],
        }];
        let story = |text: &str, time: &str, close| Action::Story {
            text: text.into(),
            image: String::new(),
            sound: String::new(),
            music: String::new(),
            time: time.into(),
            close,
        };
        let named = |name: &str, value| Named {
            name: name.into(),
            value,
        };
        mission.script = Script {
            actions: vec![
                named("first", story("C'era una volta", "1000", true)),
                named("second", story("e c'è ancora", "0", false)),
                named("talk", Action::Conversation("Prova".into())),
            ],
            rules: vec![Rule {
                name: "start".into(),
                condition: "1".into(),
                actions: vec!["first".into(), "second".into(), "talk".into()],
            }],
            ..Default::default()
        };
        mission.simulation.as_mut().unwrap().gameplay.script =
            retzar::gameplay::ScriptState::default();
    }
    app.insert_resource(mission::StoryOnOpen(true));
    // The story screen (0x4c34c0): its text goes up over its time, then it
    // closes by itself.
    step(&mut app, 50);
    assert_eq!(ui(&app).panel, Some(Panel::Narration));
    assert_eq!(ui(&app).crawl, Some([0, 1000]));
    step(&mut app, 500);
    assert_eq!(ui(&app).crawl, Some([500, 1000]));
    step(&mut app, 500);
    assert_eq!(ui(&app).panel, None);
    // The next one stays, without going up, until Enter (0x4c3d38).
    for _ in 0..4 {
        step(&mut app, 50);
    }
    assert_eq!(ui(&app).panel, Some(Panel::Narration));
    assert_eq!(ui(&app).crawl, None);
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::ZERO);
    tap(&mut app, KeyCode::Enter);
    assert_eq!(ui(&app).panel, None);
    // The conversation: one bubble for the line said, by its speaker, who
    // is brought on screen; Escape does nothing meanwhile (0x444970).
    for _ in 0..4 {
        step(&mut app, 50);
    }
    let hud = ui(&app);
    assert_eq!(hud.panel, Some(Panel::Say));
    assert!(hud.cinematic);
    assert_eq!(hud.say.len(), 1);
    let bubble = &hud.say[0];
    assert_eq!(bubble.rows, vec![(0, "Chi va là?".to_string())]);
    assert!(bubble.single && !bubble.voiced);
    assert_eq!(bubble.object, Some(unit));
    let [sx, sy] = bubble.speaker.expect("the speaker on screen");
    let [bx, by] = hud::SAY_SLOTS[bubble.slot];
    assert!(
        !(bx..bx + hud::SAY_SIZE[0]).contains(&sx) || !(by..by + hud::SAY_SIZE[1]).contains(&sy)
    );
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert_eq!(ui(&app).panel, Some(Panel::Say));
    tap(&mut app, KeyCode::F5);
    assert_eq!(ui(&app).panel, Some(Panel::Say));
    // The right button drags the bubble (0x4c08bc, 0x4c0949), kept on the
    // screen.
    let (cursor, screen) = {
        let s = Surface::of(app.world().get::<Window>(window).unwrap());
        let h = hud_layout(s, &app.world().resource::<MenuState>().settings);
        (
            move |p: [i32; 2]| Vec2::new(p[0] as f32 + 0.5, p[1] as f32 + 0.5) * h.factor / s.dpi,
            [h.size.x as i32, h.size.y as i32],
        )
    };
    let drag = |app: &mut App, from: [i32; 2], to: [i32; 2]| {
        move_to(app, window, cursor(from));
        press(app, MouseButton::Right);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        move_to(app, window, cursor(to));
        app.update();
        release(app, MouseButton::Right);
        app.update();
    };
    let [bx, by] = ui(&app).say[0].at;
    drag(&mut app, [bx + 20, by + 20], [bx - 20, by + 45]);
    assert_eq!(ui(&app).say[0].at, [bx - 40, by + 25]);
    drag(&mut app, [bx, by + 45], [screen[0] - 2, screen[1] - 2]);
    assert_eq!(
        ui(&app).say[0].at,
        [
            screen[0] - 1 - hud::SAY_SIZE[0],
            screen[1] - 1 - hud::SAY_SIZE[1]
        ]
    );
    // A click goes on: the two answers of the same speaker in one bubble.
    let answer = |app: &mut App, i: usize| {
        let mut ui = app.world_mut().resource_mut::<mission::MissionUi>();
        ui.answer = Some(i);
        ui.hud.set_panel(None);
        for _ in 0..2 {
            step(app, 50);
        }
    };
    answer(&mut app, 0);
    let hud = ui(&app);
    assert_eq!(hud.panel, Some(Panel::Say));
    assert_eq!(hud.say.len(), 1);
    assert_eq!(
        hud.say[0].rows,
        vec![(0, "Un amico.".to_string()), (1, "Nessuno.".to_string())]
    );
    assert!(!hud.say[0].single);
    answer(&mut app, 1);
    assert_eq!(ui(&app).say[0].rows, vec![(0, "Nessuno.".to_string())]);
    // Nothing follows: the bubbles go and the view is free again.
    answer(&mut app, 0);
    let hud = ui(&app);
    assert_eq!(hud.panel, None);
    assert!(hud.say.is_empty());
    for _ in 0..2 {
        step(&mut app, 50);
    }
    assert!(!ui(&app).cinematic);
}
