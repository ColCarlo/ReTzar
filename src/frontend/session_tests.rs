//! Regressions exercise the actual rendering and environment systems.
use super::*;
use retzar::world::{GameRules, GameSetup, Layer};

fn await_save(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while app.world().contains_resource::<menu_ui::SaveJob>() {
        menu_ui::finish_save(app.world_mut());
        assert!(Instant::now() < deadline, "save timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        app.world().resource::<MenuState>().notice,
        "Partita salvata"
    );
}

#[test]
#[ignore = "Requires installed original assets"]
fn saved_session_restores_world_ui_and_animation_without_reimporting_the_map() {
    let root = test_root("complete-session");
    let (mut app, _) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
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
    let own = {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let scene = loaded.scene.as_mut().unwrap();
        let own = scene
            .mission
            .objects
            .iter()
            .position(|o| o.owner == scene.mission.player() && retzar::selection::unit(o))
            .unwrap();
        scene.mission.objects[own].health = 17;
        scene.mission.kingdoms[0].resources = [123456, 7, 8, 9];
        scene.grid.cells[0] = scene.grid.cells[1];
        scene.source_entries.insert(
            "future.script.extension".into(),
            Arc::new(vec![0, 255, 2, 3]),
        );
        let sim = scene.mission.simulation.as_mut().unwrap();
        sim.elapsed = Duration::from_millis(163245);
        sim.fog.reveal(90, 90, 3);
        sim.fog.remember();
        sim.update_weather(true);
        sim.objects[own].attack = 123;
        // The speed belongs to the game and comes back with it (0x443d71).
        sim.speed = 15;
        scene.visual_time = Duration::from_millis(160007);
        for f in &mut scene.flights {
            f.flyer.advance(&f.spec, 731, &mut |_, _| Vec::new());
        }
        own
    };
    {
        let mut ui = app.world_mut().resource_mut::<MissionUi>();
        ui.hud.selected = vec![own];
        ui.hud.inspected = Some(own);
        ui.groups[4] = vec![own];
        ui.hud.panel = Some(Panel::Quests);
        ui.hud.chat.text = "Test persistente".into();
        ui.hud.command_menu = Some("ownership".into());
        ui.idle = 3;
        ui.hud.diplomacy = Some(vec![0x41; 64]);
    }
    app.world_mut().resource_mut::<Overlay>().show_health(true);
    // The fifth group shows its number beside the selection (0x4f24c5).
    app.update();
    assert_eq!(app.world().resource::<Overlay>().group_marks(), [5]);
    menu_ui::start_save(app.world_mut(), Some("slot1".into()), String::new()).unwrap();
    await_save(&mut app);
    let path = retzar::save::save_path(&root, "slot1").unwrap();
    let (meta, expected, _) = retzar::save::read(&path).unwrap();
    let mut fresh_menu = Menu::new(&source(), italian(&root)).unwrap();
    fresh_menu.catalog.missions.clear();
    fresh_menu.catalog.campaigns.clear();
    fresh_menu.save = Some(0);
    assert!(fresh_menu.saves[0].snapshot);
    // Full snapshot is independent of the installed scenario catalog.
    app.world_mut().resource_mut::<MenuState>().0 = fresh_menu;
    app.world_mut().resource_mut::<PendingAction>().0 = Some(Action::LoadSlot);
    menu_ui::apply_pending(app.world_mut());
    assert!(app.world().contains_resource::<menu_ui::LoadJob>());
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.world().contains_resource::<menu_ui::LoadJob>() {
        menu_ui::finish_load(app.world_mut());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let scene = app.world().resource::<Loaded>().scene.as_ref().unwrap();
    assert_eq!(
        serde_json::to_value(&scene.mission).unwrap(),
        serde_json::to_value(&expected.mission).unwrap()
    );
    assert_eq!(
        serde_json::to_value(scene.mission.simulation.as_ref()).unwrap(),
        serde_json::to_value(&expected.simulation).unwrap()
    );
    assert_eq!(scene.mission.simulation.as_ref().unwrap().speed, 15);
    assert_eq!(scene.grid.cells, expected.grid.cells);
    assert_eq!(scene.source_entries, expected.source_entries);
    assert_eq!(scene.visual_time, expected.visual_time);
    assert_eq!(
        serde_json::to_value(scene.flights.iter().map(|f| &f.flyer).collect::<Vec<_>>()).unwrap(),
        serde_json::to_value(&expected.flyers).unwrap()
    );
    let ui = app.world().resource::<MissionUi>();
    assert_eq!(
        serde_json::to_value(&ui.hud).unwrap(),
        serde_json::to_value(&expected.hud).unwrap()
    );
    assert_eq!(ui.idle, 3);
    assert_eq!(ui.groups, expected.groups);
    assert!(app.world().resource::<Overlay>().health_kept());
    assert_eq!(scene.scenario_id, meta.scenario);
    assert_eq!(ui.paged, expected.hud.selected);
    println!(
        "save bytes={} raw bytes={}",
        std::fs::metadata(path).unwrap().len(),
        serde_json::to_vec(&expected).unwrap().len()
    );
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::ZERO);
    app.update();
    assert_eq!(
        app.world().resource::<MissionUi>().hud.command_menu,
        expected.hud.command_menu
    );
    assert_eq!(
        app.world()
            .resource::<Loaded>()
            .scene
            .as_ref()
            .unwrap()
            .mission
            .simulation
            .as_ref()
            .unwrap()
            .fog
            .visible,
        expected.simulation.fog.visible
    );
    menu_ui::close_scene(app.world_mut());
}

#[test]
#[ignore = "Requires installed original assets"]
fn failed_restore_keeps_the_open_scene_and_valid_backup_recovers() {
    let root = test_root("restore-recovery");
    let (mut app, _) = headless(&root, (2560, 1440).into());
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: None,
            mission: 2,
        },
        None,
    )
    .unwrap();
    menu_ui::start_save(app.world_mut(), Some("slot1".into()), String::new()).unwrap();
    await_save(&mut app);
    let generation = app.world().resource::<Loaded>().generation;
    let path = retzar::save::save_path(&root, "slot1").unwrap();
    let valid = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"broken").unwrap();
    let restore = |app: &mut App| {
        app.world_mut().resource_mut::<PendingAction>().0 = Some(Action::LoadSlot);
        menu_ui::apply_pending(app.world_mut());
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.world().contains_resource::<menu_ui::LoadJob>() {
            menu_ui::finish_load(app.world_mut());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    restore(&mut app);
    assert_eq!(app.world().resource::<Loaded>().generation, generation);
    assert!(app.world().resource::<Loaded>().scene.is_some());
    assert!(
        app.world()
            .resource::<MenuState>()
            .notice
            .starts_with("Impossibile aprire")
    );
    std::fs::write(retzar::save::backup(&path), valid).unwrap();
    restore(&mut app);
    assert_eq!(app.world().resource::<Loaded>().generation, generation + 1);
    assert!(
        app.world()
            .resource::<MenuState>()
            .notice
            .starts_with("Recuperata")
    );
    menu_ui::close_scene(app.world_mut());
}

#[test]
#[ignore = "Requires installed original assets"]
fn visibility_cache_and_disabled_rain_update_while_paused() {
    let root = test_root("session-visibility");
    let (mut app, _) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    app.world_mut().resource_mut::<MenuState>().setup = Some(GameSetup {
        options: Some(GameRules::default()),
        ..Default::default()
    });
    let selection = Selection::Scenario {
        campaign: None,
        mission: 2,
    };
    menu_ui::load_now(app.world_mut(), selection.clone(), None).unwrap();
    app.update();
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::ZERO);
    let (enemy, owner, player, original, bounds) = {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        let v = s
            .visuals
            .iter()
            .find(|v| {
                v.layer == Layer::Body && {
                    let o = &s.mission.objects[v.object];
                    retzar::selection::unit(o)
                        && (1..=8).contains(&o.owner)
                        && o.owner != s.mission.player()
                        && !s.mission.simulation.as_ref().unwrap().visible(o)
                }
            })
            .unwrap();
        (
            v.object,
            s.mission.objects[v.object].owner,
            s.mission.player(),
            s.mission.diplomacy.clone(),
            v.bounds,
        )
    };
    let mut shared = original.clone();
    shared[(owner as usize - 1) * 8 + player as usize - 1] = 0x41;
    app.world_mut().resource_mut::<MissionUi>().hud.diplomacy = Some(shared);
    app.world_mut().resource_mut::<MapView>().center =
        Vec2::new(bounds[0] + bounds[2] / 2., bounds[1] + bounds[3] / 2.);
    // The health key held with Alt shows every kingdom (0x4f641e); the
    // Ctrl lock would show only the player's own.
    let key = app
        .world()
        .resource::<MenuState>()
        .settings
        .key("health_bars");
    let text = key.strip_prefix("char:").unwrap_or("`").to_owned();
    app.world_mut()
        .resource_mut::<super::input::TypedKeys>()
        .hold(KeyCode::Backquote, &text);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltLeft);
    app.update();
    let bars = |app: &mut App| -> Vec<Entity> {
        let mut q = app.world_mut().query::<(Entity, &Transform)>();
        let mut entities = q
            .iter(app.world())
            .filter(|(_, t)| t.translation.z == 421. || t.translation.z == 421.5)
            .map(|(e, _)| e)
            .collect::<Vec<_>>();
        entities.sort();
        entities
    };
    let bars_before = bars(&mut app);
    assert!(!bars_before.is_empty());
    app.world_mut().resource_mut::<MissionUi>().hud.diplomacy = Some(original);
    app.update();
    let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
    assert!(
        !s.mission
            .simulation
            .as_ref()
            .unwrap()
            .visible(&s.mission.objects[enemy])
    );
    let bars_after = bars(&mut app);
    assert_ne!(bars_before, bars_after);
    assert!(bars_after.len() < bars_before.len());

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
        sim.fog.visible.fill(true);
        sim.fog.explored.fill(true);
        sim.fog.visible_vertices.fill(true);
        sim.fog.explored_vertices.fill(true);
        sim.fog.revision += 1;
    }
    app.update();
    let particles = |app: &mut App| -> usize {
        let mut q = app.world_mut().query::<(&Transform, &Visibility)>();
        q.iter(app.world())
            .filter(|(t, v)| t.translation.z == 600. && **v == Visibility::Inherited)
            .count()
    };
    let rain_before = particles(&mut app);
    assert!(rain_before > 0);
    app.world_mut()
        .resource_mut::<MissionUi>()
        .hud
        .set_panel(Some(Panel::Story));
    app.world_mut().resource_mut::<MenuState>().settings.weather = false;
    app.update();
    let rain_after = particles(&mut app);
    assert_eq!(rain_after, 0);

    let elapsed = app
        .world()
        .resource::<Loaded>()
        .scene
        .as_ref()
        .unwrap()
        .mission
        .simulation
        .as_ref()
        .unwrap()
        .elapsed;
    app.world_mut().resource_mut::<MenuState>().settings.weather = true;
    app.update();
    assert_eq!(particles(&mut app), rain_before);
    assert_eq!(
        app.world()
            .resource::<Loaded>()
            .scene
            .as_ref()
            .unwrap()
            .mission
            .simulation
            .as_ref()
            .unwrap()
            .elapsed,
        elapsed
    );
    menu_ui::close_scene(app.world_mut());
}

fn type_key(app: &mut App, code: KeyCode, key: Key, text: Option<&str>) {
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: key,
        state: ButtonState::Pressed,
        text: text.map(Into::into),
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    tap(app, code);
}

#[test]
#[ignore = "Requires installed original assets"]
fn save_page_takes_a_typed_name_and_enter_saves_once() {
    let root = test_root("named-save");
    let (mut app, _) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
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
    app.world_mut()
        .resource_mut::<MenuState>()
        .open(retzar::menu::Screen::Save);
    app.world_mut()
        .resource_mut::<NextState<Mode>>()
        .set(Mode::Menu);
    app.update();
    // The page proposes the scenario's title; the player writes another name.
    let proposed = app.world().resource::<MenuState>().save_name.clone();
    assert!(!proposed.is_empty());
    for _ in proposed.chars() {
        type_key(&mut app, KeyCode::Backspace, Key::Backspace, None);
    }
    type_key(
        &mut app,
        KeyCode::KeyA,
        Key::Character("A".into()),
        Some("A"),
    );
    // Space is a character of the name, not a click on the focused control.
    type_key(&mut app, KeyCode::Space, Key::Space, Some(" "));
    type_key(
        &mut app,
        KeyCode::KeyB,
        Key::Character("B".into()),
        Some("B"),
    );
    let menu = app.world().resource::<MenuState>();
    assert_eq!(menu.save_name, "A B");
    assert_eq!(menu.screen, retzar::menu::Screen::Save);
    type_key(&mut app, KeyCode::Enter, Key::Enter, None);
    await_save(&mut app);
    let saves = retzar::save::list(&root);
    assert_eq!(saves.len(), 1, "Enter saves once");
    assert_eq!(saves[0].name, "A B");
    menu_ui::close_scene(app.world_mut());
}
