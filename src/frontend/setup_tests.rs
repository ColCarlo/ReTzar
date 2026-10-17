use super::*;
use retzar::{
    menu::SetupOption as K,
    world::{GameRules, GameSetup, KingdomChoice, MissionWorld, class},
};

#[test]
#[ignore = "Requires installed original archives; no window"]
fn random_menu_options_players_preferences_and_full_snapshot_are_integrated() {
    use retzar::menu::RandomOption as R;
    let root = test_root("random-complete");
    let (mut app, _) = headless(&root, (1280, 720).into());
    let selection = {
        let mut m = app.world_mut().resource_mut::<MenuState>();
        m.open(Screen::Random);
        m.random_side = 128;
        m.random_players = 2;
        m.random_options = retzar::random_map::Options::default();
        m.random_rules = GameRules {
            resource_level: 1,
            ..Default::default()
        };
        for action in [
            Action::SetupOption(K::Players, 2),
            Action::SetupRace(0, 2),
            Action::SetupRace(1, 3),
            Action::SetupColor(0),
            Action::SetupTeam(0),
            Action::SetupTeam(1),
            Action::SetupPlayer(2),
            Action::RandomOption(R::Kind, 5),
            Action::RandomOption(R::Resources, 1),
            Action::RandomOption(R::Cliffs, 1),
            Action::SetupOption(K::Population, -1),
            Action::SetupOption(K::Difficulty, 1),
            Action::SetupOption(K::Victory, 3),
            Action::SetupOption(K::VictoryLimit, 1),
            Action::RandomVisibility(true),
        ] {
            m.activate(action).unwrap();
        }
        let widgets = m.widgets(&app_assets_locale());
        assert!(
            widgets
                .iter()
                .any(|w| w.action == Action::SetupRace(3, 1) && w.enabled)
        );
        assert!(!widgets.iter().any(|w| w.action == Action::SetupRace(4, 1)));
        assert!(
            widgets
                .iter()
                .any(|w| w.action == Action::RandomOption(R::Cliffs, 1)
                    && w.enabled
                    && w.checked == Some(true))
        );
        let retzar::menu::Effect::Load(s) = m.activate(Action::Generate).unwrap() else {
            panic!()
        };
        s
    };
    let reopened = retzar::menu::Menu::new(std::path::Path::new(".."), italian(&root)).unwrap();
    assert_eq!(reopened.random_players, 4);
    assert_eq!(reopened.random_options.kind, 5);
    assert!(reopened.random_options.cliffs);
    // The rows are built anew on each visit (0x4c7ac5): the player first.
    assert!(reopened.random_rows[0].human);
    assert_eq!(reopened.random_rules.victory_limit, 400);
    menu_ui::load_now(app.world_mut(), selection, None).unwrap();
    let scene = app.world().resource::<Loaded>().scene.as_ref().unwrap();
    // 0x4cbbc0: the player took the third row with their civilization.
    assert_eq!(scene.mission.player(), 3);
    assert_eq!(scene.mission.kingdoms[2].civilization, 2);
    assert_eq!(scene.mission.kingdoms[1].civilization, 3);
    // Teams as shown: the player and the computers of rows 1 and 4 on «2»
    // are allies (0x440635), the kingdom moved to «3» fights them all.
    use retzar::world::Relation::{Ally, Enemy};
    for (a, b, relation) in [
        (3, 1, Ally),
        (3, 4, Ally),
        (1, 4, Ally),
        (3, 2, Enemy),
        (2, 1, Enemy),
    ] {
        assert_eq!(scene.mission.relation(a, b), Some(relation), "{a} → {b}");
    }
    // The computers keep the hero names of their rows (0x4cab02).
    let rows = &app.world().resource::<MenuState>().random_rows;
    assert_eq!(scene.mission.kingdoms[0].name, rows[0].name);
    assert_eq!(scene.mission.kingdoms[1].name, rows[1].name);
    assert_eq!(scene.mission.game_rules.population, 150);
    let sim = scene.mission.simulation.as_ref().unwrap();
    assert!(sim.fog.explored.iter().all(|v| *v));
    assert!(sim.fog.visible.iter().any(|v| !*v));
    let positions: Vec<_> = scene.mission.objects.iter().map(|o| (o.x, o.y)).collect();
    let metadata = retzar::menu::SavedSession {
        version: 3,
        snapshot: true,
        title: scene.title.clone(),
        name: String::new(),
        file: String::new(),
        scenario: None,
        selection: scene.selection.clone(),
        camera: [0., 0., 1.],
        setup: app.world().resource::<MenuState>().setup.clone(),
        missing: false,
    };
    let snapshot = retzar::save::Snapshot {
        language: retzar::lang::current().index(),
        schema: 1,
        grid: scene.grid.clone(),
        biome: scene.biome.clone(),
        title: scene.title.clone(),
        description: scene.description.clone(),
        starting_positions: scene.mission.starting_positions,
        mission: scene.mission.clone(),
        simulation: sim.clone(),
        source_entries: scene.source_entries.clone(),
        external_entries: Default::default(),
        portraits: Default::default(),
        hud: Default::default(),
        groups: Default::default(),
        health_kept: false,
        idle: 0,
        visual_time: Default::default(),
        flyers: scene.flights.iter().map(|f| f.flyer.clone()).collect(),
    };
    let path = root.join("saves/random.rtz");
    retzar::save::write(&path, &metadata, &snapshot).unwrap();
    let restored = scene::prepare_snapshot(&app.world().resource::<Game>().assets, &path).unwrap();
    assert_eq!(scene.grid.cells, restored.grid.cells);
    assert_eq!(
        positions,
        restored
            .mission
            .objects
            .iter()
            .map(|o| (o.x, o.y))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        serde_json::to_value(&scene.mission).unwrap(),
        serde_json::to_value(&restored.mission).unwrap()
    );
    assert_eq!(metadata.setup, restored.restored.unwrap().metadata.setup);
    menu_ui::close_scene(app.world_mut());
    let _ = std::fs::remove_dir_all(root);
}

fn app_assets_locale() -> retzar::assets::Ini {
    retzar::assets::Ini::parse(
        &retzar::assets::GameAssets::open(std::path::Path::new(".."))
            .unwrap()
            .get("LOCAL/STRINGS.INI")
            .unwrap(),
    )
}

#[test]
#[ignore = "Requires installed original archives; no window"]
fn setup_controls_reach_world_and_restore_map_defaults_without_starting_gameplay() {
    let root = test_root("setup-rules");
    let (mut app, _) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    let locale = app.world().resource::<Game>().skin.locale.clone();
    let selection = {
        let mut state = app.world_mut().resource_mut::<MenuState>();
        let (None, index) = state.catalog.locate("mp4.wmp::maps/mp4").unwrap() else {
            panic!()
        };
        state.activate(Action::Custom(index)).unwrap();
        state.activate(Action::Setup).unwrap();
        assert!(state.setup_map_defaults);
        assert_eq!(state.setup_players, 5);
        assert!(state.setup_options.no_fog && state.setup_options.explored);
        assert_eq!(state.setup_rows[1].race, 3, "Use kingdominfo byte 88");
        // 0x4c4282, 0x4c6b01: every row starts without a team, and with the
        // map's details colour and team cannot change.
        assert!(state.setup_rows.iter().all(|r| r.team == 0));
        // No team is the empty frame of TEAMS.BMP (0x4c6850), not «1».
        assert!(
            state
                .widgets(&locale)
                .iter()
                .filter(|w| matches!(w.action, Action::SetupTeam(_)))
                .all(|w| w.style == retzar::menu::Style::Team(4))
        );
        let colour = state.setup_rows[0].color;
        state.activate(Action::SetupColor(0)).unwrap();
        state.activate(Action::SetupTeam(0)).unwrap();
        assert_eq!(
            (state.setup_rows[0].color, state.setup_rows[0].team),
            (colour, 0)
        );
        assert!(!state.widgets(&locale).iter().any(|w| {
            matches!(w.action, Action::SetupColor(_) | Action::SetupTeam(_)) && w.enabled
        }));
        state
            .activate(Action::SetupOption(K::MapDefaults, 1))
            .unwrap();
        state.activate(Action::SetupTeam(0)).unwrap();
        state.activate(Action::SetupTeam(1)).unwrap();
        state.activate(Action::SetupColor(0)).unwrap();
        assert_eq!((state.setup_rows[0].team, state.setup_rows[1].team), (1, 1));
        assert_ne!(state.setup_rows[0].color, colour);
        // 0x4c6f90: checking it again brings back the map's rows.
        state
            .activate(Action::SetupOption(K::MapDefaults, 1))
            .unwrap();
        assert_eq!(
            (state.setup_rows[0].color, state.setup_rows[0].team),
            (colour, 0)
        );
        state
            .activate(Action::SetupOption(K::MapDefaults, 1))
            .unwrap();
        // Exercise a reproducible custom configuration independently of the
        // preferences carried by the installed original game's Tzar.ini.
        state.setup_options = state.setup_map_options.clone();
        assert!(
            state
                .widgets(&locale)
                .iter()
                .any(|w| w.action == Action::SetupRace(0, 1) && w.enabled)
        );
        for action in [
            Action::SetupRace(0, 1),
            Action::SetupRace(1, 1),
            Action::SetupRace(1, 1),
            Action::SetupOption(K::Players, -1),
            Action::SetupOption(K::Population, 1),
            Action::SetupOption(K::GameType, 1),
            Action::SetupOption(K::Resources, 1),
            Action::SetupOption(K::Resources, 1),
            Action::SetupOption(K::NoFog, 1),
            Action::SetupOption(K::Explored, 1),
            Action::SetupOption(K::Victory, 1),
            Action::SetupOption(K::Victory, 1),
            Action::SetupOption(K::VictoryLimit, 1),
            Action::SetupPlayer(1),
        ] {
            state.activate(action).unwrap();
        }
        let custom = state.setup_options.clone();
        assert_eq!(
            (custom.population, custom.victory, custom.victory_limit),
            (50, 2, 60)
        );
        state
            .activate(Action::SetupOption(K::MapDefaults, 1))
            .unwrap();
        assert_eq!(state.setup_options, state.setup_map_options);
        assert!(
            !state
                .widgets(&locale)
                .iter()
                .any(|w| w.action == Action::SetupRace(0, 1) && w.enabled)
        );
        state.activate(Action::SetupRace(0, 1)).unwrap();
        assert_eq!(state.setup_rows[0].race, 1);
        state
            .activate(Action::SetupOption(K::MapDefaults, 1))
            .unwrap();
        let mut restored_custom = custom.clone();
        restored_custom.no_fog = state.setup_map_options.no_fog;
        restored_custom.explored = state.setup_map_options.explored;
        assert_eq!(state.setup_options, restored_custom);
        // 0x4c7300 reloads the custom options only: the rows keep the map's.
        assert_eq!(state.setup_rows[0].race, 1);
        assert_eq!(state.setup_players, 4);
        // A human chosen in an extra map-default row cannot stay in a
        // disabled row after the custom player count is restored.
        state
            .activate(Action::SetupOption(K::MapDefaults, 1))
            .unwrap();
        state.activate(Action::SetupPlayer(4)).unwrap();
        state
            .activate(Action::SetupOption(K::MapDefaults, 1))
            .unwrap();
        assert!(state.setup_rows[0].human);
        state.activate(Action::SetupPlayer(1)).unwrap();
        for action in [
            Action::SetupRace(0, 1),
            Action::SetupRace(1, 1),
            Action::SetupRace(1, 1),
        ] {
            state.activate(action).unwrap();
        }
        state.activate(Action::SetupOption(K::NoFog, 1)).unwrap();
        state.activate(Action::SetupOption(K::Explored, 1)).unwrap();
        let retzar::menu::Effect::Load(s) = state.activate(Action::Play).unwrap() else {
            panic!()
        };
        assert!(state.setup.as_ref().unwrap().custom_start);
        s
    };
    menu_ui::load_now(app.world_mut(), selection.clone(), None).unwrap();
    app.update();
    let initial = {
        let scene = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        let m = &scene.mission;
        assert_eq!(m.player(), 2);
        assert_eq!(m.active_kingdoms().count(), 4);
        assert_eq!(m.game_rules.population, 50);
        assert!(!m.game_rules.no_fog && !m.game_rules.explored);
        assert_eq!(m.player_kingdom().unwrap().resources, [5000; 4]);
        for (owner, _) in m.active_kingdoms() {
            let forces: Vec<_> = m
                .objects
                .iter()
                .filter(|o| o.owner == owner && (retzar::selection::unit(o) || o.building()))
                .collect();
            assert_eq!(forces.len(), 4);
            assert_eq!(
                forces.iter().filter(|o| o.class == class::CASTLE).count(),
                1
            );
            assert_eq!(forces.iter().filter(|o| o.class == class::PEON).count(), 3);
        }
        assert!(
            m.objects
                .iter()
                .any(|o| o.owner == 10 && o.class == class::WALL_STONE)
        );
        for o in &m.objects {
            if o.owner == 1 && (retzar::selection::unit(o) || o.building()) {
                assert_eq!(o.civilization, 2);
            }
            if o.owner == 2 && (retzar::selection::unit(o) || o.building()) {
                assert_eq!(o.civilization, 1);
            }
        }
        serde_json::to_vec(m).unwrap()
    };
    app.update();
    assert_eq!(
        initial,
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
    // Save round-trip and restart keep the resolved civilizations and visibility.
    let setup = app.world().resource::<MenuState>().setup.clone().unwrap();
    let restored: GameSetup = serde_json::from_slice(&serde_json::to_vec(&setup).unwrap()).unwrap();
    assert_eq!(restored, setup);
    menu_ui::load_now(app.world_mut(), selection, None).unwrap();
    assert_eq!(
        initial,
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
    menu_ui::close_scene(app.world_mut());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires installed original archives; no window"]
fn campaign_difficulty_and_random_terrain_visibility_survive_loading_and_restart() {
    let root = test_root("setup-random-campaign");
    let (mut app, _) = headless(&root, (1280, 720).into());
    app.insert_resource(mission::StoryOnOpen(false));
    let campaign = {
        let mut menu = app.world_mut().resource_mut::<MenuState>();
        menu.open(Screen::Campaign);
        menu.activate(Action::Difficulty(1)).unwrap();
        let retzar::menu::Effect::Load(s) = menu.activate(Action::Play).unwrap() else {
            panic!()
        };
        assert_eq!(
            menu.setup
                .as_ref()
                .unwrap()
                .options
                .as_ref()
                .unwrap()
                .difficulty,
            2
        );
        s
    };
    menu_ui::load_now(app.world_mut(), campaign, None).unwrap();
    assert_eq!(
        app.world()
            .resource::<Loaded>()
            .scene
            .as_ref()
            .unwrap()
            .mission
            .game_rules
            .difficulty,
        2
    );
    for biome in ["desert", "grass", "snow", "summer"] {
        let selection = {
            let mut menu = app.world_mut().resource_mut::<MenuState>();
            menu.open(Screen::Random);
            menu.random_side = 64;
            let i = menu
                .random_terrains
                .iter()
                .position(|t| t.0 == biome)
                .unwrap()
                + 1;
            menu.random_terrain = i - 1;
            menu.activate(Action::RandomTerrain(1)).unwrap();
            menu.random_rules.no_fog = false;
            menu.random_rules.explored = true;
            menu.activate(Action::RandomVisibility(false)).unwrap();
            let retzar::menu::Effect::Load(s) = menu.activate(Action::Generate).unwrap() else {
                panic!()
            };
            assert_eq!(
                menu.setup.as_ref().unwrap().random_biome.as_deref(),
                Some(biome)
            );
            s
        };
        menu_ui::load_now(app.world_mut(), selection.clone(), None).unwrap();
        let initial = {
            let loaded = app.world().resource::<Loaded>();
            let scene = loaded.scene.as_ref().unwrap();
            assert_eq!(scene.biome, biome);
            assert_eq!(scene.mission.active_kingdoms().count(), 2);
            assert_eq!(
                scene
                    .mission
                    .objects
                    .iter()
                    .filter(|o| o.class == class::CASTLE)
                    .count(),
                2
            );
            let sim = scene.mission.simulation.as_ref().unwrap();
            assert!(sim.fog.visible.iter().all(|&v| v));
            assert_eq!(sim.snow_allowed, biome == "snow");
            serde_json::to_vec(&scene.mission).unwrap()
        };
        menu_ui::load_now(app.world_mut(), selection.clone(), None).unwrap();
        assert_eq!(
            initial,
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
        // With unexplored terrain only the human kingdom's initial sight is revealed.
        app.world_mut()
            .resource_mut::<MenuState>()
            .setup
            .as_mut()
            .unwrap()
            .options
            .as_mut()
            .unwrap()
            .explored = false;
        app.world_mut()
            .resource_mut::<MenuState>()
            .setup
            .as_mut()
            .unwrap()
            .options
            .as_mut()
            .unwrap()
            .no_fog = false;
        menu_ui::load_now(app.world_mut(), selection, None).unwrap();
        assert!(
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
                .explored
                .iter()
                .any(|&v| !v)
        );
    }
    menu_ui::close_scene(app.world_mut());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires installed original archives"]
fn visibility_setup_has_four_distinct_outcomes_and_civilizations_resolve_original_art() {
    let root = test_root("setup-visibility");
    let (mut app, _) = headless(&root, (1280, 720).into());
    let a = app.world().resource::<Game>().assets.clone();
    let cat = &app.world().resource::<MenuState>().catalog;
    let (None, index) = cat.locate("mp4.wmp::maps/mp4").unwrap() else {
        panic!()
    };
    let scenario = cat.missions[index].clone();
    let c = scenario.container().unwrap();
    let grid = scenario.grid().unwrap();
    let mut original = MissionWorld::read(&c, &grid).unwrap();
    // The scene starts from the corrected map (the tower of Tzar 5).
    original.correct_map_errors(&grid.name);
    let lib = retzar::world::ObjectLibrary::load(&a, "grass").unwrap();
    original.game_rules = GameRules::default();
    let hidden = retzar::simulation::Simulation::new(&original, &lib, grid.side, "grass");
    let far = original
        .objects
        .iter()
        .position(|o| o.owner != original.player() && o.class == class::PEON && !hidden.known(o))
        .unwrap();
    for (no_fog, explored) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut m = MissionWorld::read(&c, &grid).unwrap();
        m.game_rules = GameRules {
            no_fog,
            explored,
            ..Default::default()
        };
        let sim = retzar::simulation::Simulation::new(&m, &lib, grid.side, "grass");
        assert_eq!(sim.known(&m.objects[far]), explored);
        assert_eq!(sim.visible(&m.objects[far]), no_fog && explored);
        assert!(
            sim.visible(
                m.objects
                    .iter()
                    .find(|o| o.owner == m.player() && o.class == class::PEON)
                    .unwrap()
            )
        );
    }
    for civ in 1..=3 {
        let setup = GameSetup {
            kingdoms: original
                .active_kingdoms()
                .map(|(owner, k)| KingdomChoice {
                    record: owner as usize - 1,
                    color: k.color,
                    team: retzar::world::shown_team(k.team),
                    civilization: Some(civ),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let prepared = scene::prepare(
            &a,
            Selection::Scenario {
                campaign: None,
                mission: index,
            },
            Some(scenario.clone()),
            Some(setup),
        )
        .unwrap();
        assert!(prepared.objects.missing.is_empty(), "civilization {civ}");
        for (before, after) in original.objects.iter().zip(&prepared.mission.objects) {
            assert_eq!(
                (before.id, before.owner, before.class, before.x, before.y),
                (after.id, after.owner, after.class, after.x, after.y)
            );
        }
    }
    menu_ui::close_scene(app.world_mut());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn old_setup_saves_preserve_map_rules_and_clicks_already_selected_still_raise_audio_events() {
    let setup: GameSetup =
        serde_json::from_str(r#"{"kingdoms":[{"record":0,"color":3,"team":1}]}"#).unwrap();
    assert_eq!(setup.kingdoms[0].civilization, None);
    assert!(!setup.custom_start);
    assert!(setup.options.is_none() && setup.players.is_none() && setup.human.is_none());
    let mut hud = hud::HudState {
        selected: vec![0],
        ..Default::default()
    };
    hud.selection_changed();
    let first = hud.selection_event;
    hud.selection_changed();
    assert_eq!(hud.selected, vec![0]);
    assert_eq!(hud.selection_event, first + 1);
}
