// Regressions reproduced during the code audit, now asserting the corrected behavior.
use super::*;

#[test]
fn regression_systemic_technology_catalog_and_group_production_hud_all_civilizations() {
    use retzar::{
        gameplay::{Job, JobKind},
        simulation::Simulation,
        world::{CLASSES, MapObject},
    };
    let (_, menu, skin, library, mut mission) = selection_fixture();
    let owner = mission.player();
    let template = mission
        .objects
        .iter()
        .find(|o| o.owner == owner && retzar::selection::unit(o))
        .unwrap()
        .clone();
    let catalog = retzar::gameplay::Catalog::load(&GameAssets::open(&source()).unwrap()).unwrap();
    let techs = catalog
        .technologies
        .section_names()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut checked = 0;
    for civ in 1..=3 {
        mission.objects = (39..=73)
            .map(|class| MapObject {
                class,
                civilization: civ,
                flags: 0x300,
                health: 100,
                max_health: 100,
                ..template.clone()
            })
            .collect();
        let rules = &mut mission.rules[owner as usize - 1];
        rules.forbidden_researches.clear();
        rules.forbidden_upgrades.clear();
        rules.forbidden_units.clear();
        rules.forbidden_buildings.clear();
        mission.simulation = Some(Simulation::new(&mission, &library, 64, "grass"));
        for (i, class) in (39..=73).enumerate() {
            let Some(menu_name) = library.property(&mission.objects[i], "menu") else {
                continue;
            };
            let menus = retzar::assets::Ini::parse(
                &GameAssets::open(&source())
                    .unwrap()
                    .get("DATA/MENU.INI")
                    .unwrap(),
            );
            for (key, _) in menus
                .entries(menu_name)
                .iter()
                .filter(|(key, _)| catalog.action(key) == "technology")
            {
                let name = catalog.class_name(key);
                mission.rules[owner as usize - 1].initial_researches = techs
                    .iter()
                    .filter(|n| !n.eq_ignore_ascii_case(&name))
                    .cloned()
                    .collect();
                let state = hud::HudState {
                    selected: vec![i],
                    ..default()
                };
                let commands = hud::commands(&skin, &menu.locale, &state, &mission, &library);
                assert!(
                    commands.iter().any(|c| c.order.eq_ignore_ascii_case(key)),
                    "civ {civ}, {}, missing {key}",
                    CLASSES[class]
                );
                checked += 1;
            }
        }
        mission.rules[owner as usize - 1].initial_researches.clear();
        let barracks = 45 - 39;
        let duplicate = mission.objects[barracks].clone();
        mission.objects.push(duplicate);
        let second = mission.objects.len() - 1;
        let mut sim = mission.simulation.take().unwrap();
        let mut g = retzar::map::MapGrid {
            name: "fixture".into(),
            side: 64,
            cells: vec![0x90; 4096],
        };
        retzar::gameplay::advance(
            &mut sim,
            &mut mission,
            &mut g,
            &library,
            &catalog,
            Duration::ZERO,
            10,
            false,
        );
        for i in [barracks, second] {
            for remaining in [50, 100] {
                sim.gameplay.actors[i].jobs.push_back(Job {
                    kind: JobKind::Train(3),
                    total: 100,
                    remaining,
                    paid: [0; 4],
                    due_ms: None,
                    waiting: false,
                });
            }
        }
        mission.simulation = Some(sim);
        let state = hud::HudState {
            selected: vec![barracks, second],
            ..default()
        };
        let queue = hud::production_queue(&skin, &menu.locale, &state, &mission, &library);
        assert_eq!(queue.len(), 4);
        assert_eq!(queue.iter().filter(|e| e.progress == Some(50)).count(), 2);
        assert_eq!(queue[2].producer, second);
        let commands = hud::commands(&skin, &menu.locale, &state, &mission, &library);
        assert!(!commands.is_empty());
        let image = hud::render(
            &menu,
            &skin,
            skin.background(800, 600, true),
            hud::HudView {
                width: 800,
                height: 600,
                left: true,
                title: "",
                description: "",
                state: &state,
                mission: &mission,
                library: &library,
                portraits: &std::collections::HashMap::new(),
                camera: [0.; 3],
                side: 64,
                pixels: [800., 600.],
                messages: &[],
                status: "",
                player_name: "Test",
                selection: [255, 255, 255],
                map_text: 0,
            },
        );
        assert_eq!((image.width, image.height), (800, 600));
    }
    assert!(checked > 100, "only {checked} technologies checked");
}

#[test]
fn regression_a_soul_gem_has_its_menu_only_while_held() {
    use retzar::world::{FLAG_HELD, MapObject, class};
    let (_, menu, skin, library, mut mission) = selection_fixture();
    let template = mission.objects[0].clone();
    // A soul gem of no kingdom, which everyone commands (0x506c30): lying
    // on the map it has no menu; held (0x400000) its menu is `item_soul` of
    // MENU.INI (0x463d70), where Kill and Discord have `IMG =
    // ORDERS\move.bmp`, picture 0 (0x518e90).
    mission.objects = vec![MapObject {
        class: class::ITEM_SOUL,
        owner: 0,
        civilization: 0,
        flags: 0,
        ..template
    }];
    let state = hud::HudState {
        selected: vec![0],
        ..default()
    };
    let listed = |mission: &retzar::world::MissionWorld| {
        hud::commands(&skin, &menu.locale, &state, mission, &library)
            .into_iter()
            .map(|c| (c.order, c.hotkey, c.icon))
            .collect::<Vec<(String, String, usize)>>()
    };
    assert!(listed(&mission).is_empty());
    mission.objects[0].flags |= FLAG_HELD;
    let listed = listed(&mission);
    assert_eq!(
        listed,
        [
            ("item_kill".to_string(), "K".to_string(), 0),
            ("item_berserk".to_string(), "B".to_string(), 0),
            ("item_drop".to_string(), "D".to_string(), 30)
        ]
    );
}

#[test]
fn regression_upgrades_wait_out_of_sight_for_their_research() {
    use retzar::world::MapObject;
    let (_, menu, skin, library, mut mission) = selection_fixture();
    let owner = mission.player();
    let template = mission
        .objects
        .iter()
        .find(|o| o.owner == owner && o.building())
        .unwrap()
        .clone();
    // A finished second wooden tower (class 41): stone needs Architecture,
    // the cannon tower Cannon Tower (TECHTREE *.INI [depend]).
    mission.objects = vec![MapObject {
        class: 41,
        civilization: 1,
        flags: 0x300,
        health: 100,
        max_health: 100,
        ..template
    }];
    let rules = &mut mission.rules[owner as usize - 1];
    rules.forbidden_upgrades.clear();
    rules.initial_researches.clear();
    let state = hud::HudState {
        selected: vec![0],
        ..default()
    };
    let listed = |mission: &retzar::world::MissionWorld| {
        hud::commands(&skin, &menu.locale, &state, mission, &library)
            .into_iter()
            .map(|c| c.order.to_ascii_lowercase())
            .collect::<Vec<_>>()
    };
    // 0x494c51: hidden, not greyed, while the research is missing.
    let none = listed(&mission);
    assert!(!none.iter().any(|o| o.starts_with("to_stower")), "{none:?}");
    mission.rules[owner as usize - 1]
        .initial_researches
        .push("te_architecture".into());
    let some = listed(&mission);
    assert!(some.iter().any(|o| o == "to_stower"), "{some:?}");
    assert!(!some.iter().any(|o| o == "to_stower2_1"), "{some:?}");
}

#[test]
fn regression_systemic_partial_busy_group_keeps_technology_and_order_flags_release() {
    use retzar::{
        gameplay::{Job, JobKind},
        simulation::Simulation,
        world::MapObject,
    };
    let (_, menu, skin, library, mut m) = selection_fixture();
    let o = m
        .objects
        .iter()
        .find(|o| o.owner == m.player())
        .unwrap()
        .clone();
    m.objects = vec![
        MapObject {
            class: 60,
            civilization: 1,
            flags: 0x300,
            health: 100,
            max_health: 100,
            ..o.clone()
        };
        2
    ];
    m.rules[m.player() as usize - 1].initial_researches.clear();
    m.rules[m.player() as usize - 1]
        .forbidden_researches
        .clear();
    let mut sim = Simulation::new(&m, &library, 64, "grass");
    let catalog = retzar::gameplay::Catalog::load(&GameAssets::open(&source()).unwrap()).unwrap();
    let mut g = retzar::map::MapGrid {
        name: "fixture".into(),
        side: 64,
        cells: vec![0x90; 4096],
    };
    retzar::gameplay::advance(
        &mut sim,
        &mut m,
        &mut g,
        &library,
        &catalog,
        Duration::ZERO,
        10,
        false,
    );
    sim.gameplay.actors[0].jobs.push_back(Job {
        kind: JobKind::Train(17),
        total: 100,
        remaining: 50,
        paid: [0; 4],
        due_ms: None,
        waiting: false,
    });
    m.simulation = Some(sim);
    for selected in [vec![0, 1], vec![1, 0]] {
        let state = hud::HudState {
            selected,
            ..default()
        };
        assert!(
            hud::commands(&skin, &menu.locale, &state, &m, &library)
                .iter()
                .any(|c| c.order.starts_with("te_"))
        );
    }
    let root = test_root("systemic-order-flags");
    let (mut app, window) = headless(&root, (1280, 720).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: Some(0),
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    let generation = app.world().resource::<Loaded>().generation;
    app.world_mut().resource_mut::<MissionUi>().order_feedback = Some(orders::Feedback {
        cell: [20, 20],
        flag: true,
        target: None,
        color: orders::GREEN,
        cross: None,
        started: 0.,
        generation,
    });
    app.update();
    assert_eq!(
        app.world().resource::<orders::Indicators>().entity_count(),
        1
    );
    app.world_mut()
        .resource_mut::<NextState<Mode>>()
        .set(Mode::Menu);
    app.update();
    assert_eq!(
        app.world().resource::<orders::Indicators>().entity_count(),
        0
    );
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: Some(0),
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    let (worker, cell) = {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let s = loaded.scene.as_mut().unwrap();
        s.mission
            .simulation
            .as_mut()
            .unwrap()
            .fog
            .visible
            .fill(true);
        let worker = s
            .mission
            .objects
            .iter()
            .position(|o| o.owner == s.mission.player() && o.class == retzar::world::class::PEON)
            .unwrap();
        let tree = s
            .grid
            .cells
            .iter()
            .enumerate()
            .find(|(i, c)| {
                **c & 0xf800 != 0
                    && i % s.side > 20
                    && i % s.side < s.side - 20
                    && i / s.side > 20
                    && i / s.side < s.side - 20
            })
            .map(|(i, _)| i)
            .unwrap();
        (worker, [tree % s.side, tree / s.side])
    };
    let sf = surface(&app, window);
    app.world_mut().resource_mut::<MapView>().scale = 0.5;
    app.world_mut().resource_mut::<MapView>().center =
        Vec2::new(cell[0] as f32 * 32. + 16., -(cell[1] as f32 * 32. + 16.));
    {
        let mut ui = app.world_mut().resource_mut::<MissionUi>();
        ui.hud.selected = vec![worker];
        ui.order_feedback = None;
    }
    move_to(&mut app, window, sf.physical / sf.dpi / 2.);
    app.update();
    assert_eq!(
        app.world().resource::<orders::Indicators>().cursor,
        "cs_harvest"
    );
    // 0x4f246c: the original marks only selected and blinking objects,
    // nothing under the pointer.
    assert_eq!(
        app.world().resource::<orders::Indicators>().entity_count(),
        0
    );
    // 0x490627: a right click on the ground leaves the red cross, and
    // 0x5008c0 makes a target blink: a frame of four bars, on for 100 ms.
    let now = app.world().resource::<Time>().elapsed_secs();
    let generation = app.world().resource::<Loaded>().generation;
    app.world_mut().resource_mut::<MissionUi>().order_feedback = Some(orders::Feedback {
        cell: [cell[0] as u16, cell[1] as u16],
        flag: false,
        target: Some(worker),
        color: orders::RED,
        cross: Some([cell[0] as f32 * 32. + 7., cell[1] as f32 * 32. + 9.]),
        started: now,
        generation,
    });
    app.update();
    assert_eq!(
        app.world().resource::<orders::Indicators>().entity_count(),
        5
    );
    app.world_mut().resource_mut::<MissionUi>().order_feedback = None;
    app.world_mut().resource_mut::<MissionUi>().target_order = Some("move".into());
    app.update();
    assert_eq!(
        app.world().resource::<orders::Indicators>().cursor,
        "cs_move"
    );
    let Some(CursorIcon::Custom(CustomCursor::Image(cursor))) =
        app.world().get::<CursorIcon>(window)
    else {
        panic!("missing native move cursor")
    };
    // HWCURSOR (on, as Tzar.ini has it): the hardware `ch_move` (0x43ab31).
    assert_eq!(cursor.hotspot, (11, 11));
    // 0x41d7e0/0x41d8b0: the rally flag is made when its building is selected
    // and when its point moves, and goes when it is deselected.
    let castle = {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let s = loaded.scene.as_mut().unwrap();
        let player = s.mission.player();
        let castle = s
            .mission
            .objects
            .iter()
            .position(|o| o.owner == player && o.building())
            .unwrap();
        s.mission.simulation.as_mut().unwrap().gameplay.actors[castle].rally = Some([10, 10]);
        castle
    };
    {
        let mut ui = app.world_mut().resource_mut::<MissionUi>();
        ui.target_order = None;
        ui.hud.selected = vec![castle];
    }
    app.update();
    let shown = app
        .world()
        .resource::<orders::Indicators>()
        .rally_since(castle);
    let (cell, since) = shown.unwrap();
    assert_eq!(cell, [10, 10]);
    app.update();
    assert_eq!(
        app.world()
            .resource::<orders::Indicators>()
            .rally_since(castle),
        Some(([10, 10], since))
    );
    {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let s = loaded.scene.as_mut().unwrap();
        s.mission.simulation.as_mut().unwrap().gameplay.actors[castle].rally = Some([12, 10]);
    }
    app.update();
    let (cell, moved) = app
        .world()
        .resource::<orders::Indicators>()
        .rally_since(castle)
        .unwrap();
    assert_eq!(cell, [12, 10]);
    assert!(moved >= since);
    app.world_mut()
        .resource_mut::<MissionUi>()
        .hud
        .selected
        .clear();
    app.update();
    assert_eq!(
        app.world()
            .resource::<orders::Indicators>()
            .rally_since(castle),
        None
    );
    // 0x4a2e19: Alt on D_TRADE's origin button centres the view on that
    // market instead of picking one.
    let (x, y) = {
        let loaded = app.world().resource::<Loaded>();
        let o = &loaded.scene.as_ref().unwrap().mission.objects[castle];
        (o.x, o.y)
    };
    {
        let mut ui = app.world_mut().resource_mut::<MissionUi>();
        ui.hud.selected = vec![castle];
    }
    app.update();
    {
        let mut ui = app.world_mut().resource_mut::<MissionUi>();
        ui.hud.dialog = Some(retzar::dialogs::Dialog::route(
            castle,
            [Some(castle), None],
            [0, 0],
            None,
        ));
    }
    app.update();
    assert!(app.world().resource::<MissionUi>().hud.dialog.is_some());
    app.world_mut().resource_mut::<MapView>().center = Vec2::ZERO;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltLeft);
    regression_hud_click(
        &mut app,
        window,
        hud::HudAction::Dialog(retzar::dialogs::Control::Market(0)),
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::AltLeft);
    let d = app
        .world()
        .resource::<MissionUi>()
        .hud
        .dialog
        .clone()
        .unwrap();
    assert_eq!(d.picking, None);
    let view = app.world().resource::<MapView>().center;
    assert_ne!(view, Vec2::ZERO);
    let reach = (view - Vec2::new(x as f32 * 32., -(y as f32 * 32.))).abs();
    assert!(reach.x < 400. && reach.y < 400., "{view:?} for {x},{y}");
}
pub(super) fn regression_hud_click(app: &mut App, window: Entity, action: hud::HudAction) {
    let s = surface(app, window);
    let rect = {
        let world = app.world();
        let menu = world.resource::<MenuState>();
        let scene = world.resource::<Loaded>().scene.as_ref().unwrap();
        let game = world.resource::<Game>();
        let ui = world.resource::<MissionUi>();
        let h = hud_layout(s, &menu.settings);
        let layout = hud::Layout {
            width: h.size.x as i32,
            height: h.size.y as i32,
            left: menu.settings.layout == 1,
        };
        hud::widgets(
            &game.classic,
            &game.skin.locale,
            &ui.hud,
            layout,
            &scene.mission,
            &scene.library,
        )
        .into_iter()
        .find(|b| b.action == action)
        .unwrap()
        .rect
    };
    let settings = app.world().resource::<MenuState>().settings.clone();
    let h = hud_layout(s, &settings);
    let p = Vec2::new(rect.x as f32 + 3., rect.y as f32 + 3.) * h.factor / s.dpi;
    move_to(app, window, p);
    press(app, MouseButton::Left);
    app.update();
    release(app, MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
}

#[test]
fn regression_original_special_items_are_selectable_on_the_ground() {
    use retzar::world::{FLAG_INACTIVE, MissionWorld, class};
    let catalog = retzar::scenario::ScenarioCatalog::load(&source()).unwrap();
    // [Italian edition, American campaign]: items checked, maps with any.
    let mut checked = [0usize; 2];
    let mut affected = [0usize; 2];
    for m in catalog
        .missions
        .iter()
        .chain(catalog.campaigns.iter().flat_map(|c| &c.missions))
    {
        let edition = retzar::scenario::is_american(&m.file) as usize;
        let c = m.container().unwrap();
        let grid = retzar::map::MapGrid::read(&c, &m.header).unwrap();
        let mission = MissionWorld::read(&c, &grid).unwrap();
        let items: Vec<_> = mission
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                [
                    class::ITEM_PLUSPLUS,
                    class::ITEM_SCROLL,
                    class::ITEM_MAGIC,
                    class::ITEM_HEALTH,
                ]
                .contains(&o.class)
                    && o.flags & FLAG_INACTIVE == 0
            })
            .collect();
        if !items.is_empty() {
            affected[edition] += 1;
        }
        // On the ground they are no item of the player's list (0x462340).
        let listed = hud::list_entries(hud::ListKind::Items, &mission);
        for (i, o) in items {
            assert!(o.selectable());
            assert!(!listed.contains(&i));
            checked[edition] += 1;
        }
    }
    assert_eq!((checked[0], affected[0]), (345, 10));
    if retzar::scenario::american_campaign(&source()).is_some() {
        assert_eq!((checked[1], affected[1]), (22, 4));
    }
}

#[test]
fn regression_campaigns_carry_only_the_heroes_their_records_flag() {
    use retzar::{gameplay, simulation::Simulation, world::FLAG_HERO};
    let assets = GameAssets::open(&source()).unwrap();
    let catalog = retzar::scenario::ScenarioCatalog::load(&source()).unwrap();
    let library = retzar::world::ObjectLibrary::load(&assets, "grass").unwrap();
    let open = |title: &str| {
        let m = catalog
            .campaigns
            .iter()
            .flat_map(|c| &c.missions)
            .find(|m| m.title == title)
            .unwrap();
        let c = m.container().unwrap();
        let grid = retzar::map::MapGrid::read(&c, &m.header).unwrap();
        let mission = retzar::world::MissionWorld::read(&c, &grid).unwrap();
        let sim = Simulation::new(&mission, &library, grid.side, "grass");
        (mission, sim, grid)
    };
    let rules = gameplay::Catalog::load(&assets).unwrap();
    // The first step: the script notes the objects not on the map yet.
    let start = |(mut mission, mut sim, mut grid): (_, Simulation, _)| {
        gameplay::advance(
            &mut sim,
            &mut mission,
            &mut grid,
            &library,
            &rules,
            std::time::Duration::from_millis(gameplay::TICK_MS),
            10,
            false,
        );
        (mission, sim)
    };
    // No record of King Arthur's maps has flag 0x1000: nobody goes on.
    let (mission, sim) = start(open("Merlino e il Re Artù 2"));
    assert!(gameplay::heroes(&sim, &mission).is_empty());
    // Gengis Khan's: the flagged ones on the map, kingdom by kingdom, each
    // in the order of its unit list, where the map's records went in one
    // by one at its head (K+0x56, 0x507940), then those not on it yet,
    // promoted (0x43ea20).
    let (mission, sim) = start(open("La proclamazione del Khan"));
    let mut heroes = gameplay::heroes(&sim, &mission);
    let names: Vec<_> = heroes.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Jagadai", "Batu", "Zhorin", "Unojo", "Tugril", "Yamuga", "Temujin", "Vecchio",
            "Bochurgi", "Borta", "Borta"
        ]
    );
    let jagadai = heroes.iter_mut().find(|h| h.name == "Jagadai").unwrap();
    assert_eq!(
        (jagadai.subtitle.as_str(), jagadai.promoted),
        ("Mago", false)
    );
    jagadai.health = 7;
    assert!(
        heroes
            .iter()
            .find(|h| h.name == "Temujin")
            .unwrap()
            .promoted
    );
    // The next map gives them back to its flagged objects of the same name
    // (0x440c58).
    let (mut mission, mut sim, _) = open("La conquista di Pechino");
    gameplay::restore_heroes(&mut sim, &mut mission, &library, &heroes);
    let named = |n: &str| mission.objects.iter().find(|o| o.name == n).unwrap();
    assert_eq!(named("Jagadai").subtitle, "Mago");
    assert_eq!(named("Jagadai").health, 7);
    assert_ne!(named("Temujin").flags & FLAG_HERO, 0);
    assert_eq!(named("Jagadai").flags & FLAG_HERO, 0);
}

#[test]
fn regression_mission_script_runs_its_opening_and_keeps_future_quests() {
    use retzar::{gameplay, script::Action, simulation::Simulation};
    let assets = GameAssets::open(&source()).unwrap();
    let catalog = retzar::scenario::ScenarioCatalog::load(&source()).unwrap();
    let m = catalog
        .campaigns
        .iter()
        .flat_map(|c| &c.missions)
        .find(|m| m.title == "La proclamazione del Khan")
        .unwrap();
    let c = m.container().unwrap();
    let mut grid = retzar::map::MapGrid::read(&c, &m.header).unwrap();
    let mut mission = retzar::world::MissionWorld::read(&c, &grid).unwrap();
    // Names longer than 16 bytes resolve: «mision audiencia Tugril».
    let header = c.get(&format!("{}.header", grid.name)).unwrap();
    let [rules, actions, events, _] = retzar::script::counts(&header);
    let script = &mission.script;
    assert_eq!(
        (
            script.rules.len(),
            script.actions.len(),
            script.events.len()
        ),
        (rules as usize, actions as usize, events as usize)
    );
    let i = script.action("casado0").unwrap();
    assert_eq!(
        script.actions[i].value,
        Action::Expression("$casado=0".into())
    );
    assert!(script.action("mision audiencia Tugril").is_some());
    let library = retzar::world::ObjectLibrary::load(&assets, "grass").unwrap();
    let gameplay = gameplay::Catalog::load(&assets).unwrap();
    let mut sim = Simulation::new(&mission, &library, grid.side, "grass");
    // A minute of play, every window closed and every first phrase chosen.
    for _ in 0..1200 {
        gameplay::advance(
            &mut sim,
            &mut mission,
            &mut grid,
            &library,
            &gameplay,
            std::time::Duration::from_millis(gameplay::TICK_MS),
            10,
            false,
        );
        sim.gameplay.script.outbox.clear();
        if sim.gameplay.script.conversation.is_some() {
            gameplay::answer(&mut sim, &mut mission, &grid, &library, &gameplay, 0);
        } else if sim.gameplay.script.modal {
            gameplay::close_window(&mut sim, &mut mission);
        }
    }
    let st = &sim.gameplay.script;
    assert_eq!(st.variables.map.get("$casado"), Some(&0));
    for future in [
        "Produci unità di cibo fino ad averne 400.",
        "Sconfiggi i nani.",
        "Sconfiggi gli orchi.",
    ] {
        assert!(!mission.quests.iter().any(|q| q.text == future), "{future}");
    }
    assert!(st.over.is_none());
}

#[test]
fn regression_original_rectangle_excludes_buildings_even_with_multiple_visuals() {
    use retzar::world::{Layer, class};
    let root = test_root("audit-gate");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: Some(0),
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    let (index, v, copies) = {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        let index = s
            .mission
            .objects
            .iter()
            .position(|o| {
                [class::GATE_WOOD, class::GATE_STONE].contains(&o.class) && o.selectable()
            })
            .unwrap();
        let v = *s
            .visuals
            .iter()
            .find(|v| v.layer == Layer::Body && v.object == index)
            .unwrap();
        let copies = s
            .visuals
            .iter()
            .filter(|v| v.layer == Layer::Body && v.object == index)
            .count();
        (index, v, copies)
    };
    let c = Vec2::new(
        v.footprint[0] + v.footprint[2] / 2.,
        v.footprint[1] + v.footprint[3] / 2.,
    );
    app.world_mut().insert_resource(MapView {
        center: c,
        scale: 0.5,
        frame: Default::default(),
    });
    let s = surface(&app, window);
    let view = *app.world().resource::<MapView>();
    let cursor_for = |at: Vec2| {
        s.physical / s.dpi / 2. + Vec2::new(at.x - view.center.x, view.center.y - at.y) / view.scale
    };
    move_to(&mut app, window, cursor_for(c - Vec2::splat(10.)));
    press(&mut app, MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    // Read the clamped camera so the release position still surrounds the same gate.
    let view = *app.world().resource::<MapView>();
    let end = c + Vec2::splat(10.);
    let p = s.physical / s.dpi / 2.
        + Vec2::new(end.x - view.center.x, view.center.y - end.y) / view.scale;
    move_to(&mut app, window, p);
    release(&mut app, MouseButton::Left);
    app.update();
    let selected = &app.world().resource::<MissionUi>().hud.selected;
    let occurrences = selected.iter().filter(|&&i| i == index).count();
    assert!(copies > 1);
    assert_eq!(occurrences, 0);
}

#[test]
fn regression_diplomacy_draft_is_discarded_on_toolbar_close() {
    let root = test_root("audit-diplomacy");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: Some(0),
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    regression_hud_click(&mut app, window, hud::HudAction::Panel(Panel::Diplomacy));
    let owner = {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        hud::other_kingdoms(&s.mission)[0]
    };
    regression_hud_click(
        &mut app,
        window,
        hud::HudAction::Share(owner, hud::diplomacy::VISION),
    );
    assert!(
        app.world()
            .resource::<MissionUi>()
            .hud
            .diplomacy_draft
            .is_some()
    );
    regression_hud_click(&mut app, window, hud::HudAction::Panel(Panel::Diplomacy));
    assert_eq!(app.world().resource::<MissionUi>().hud.panel, None);
    assert!(app.world().resource::<MissionUi>().hud.diplomacy.is_none());
    regression_hud_click(&mut app, window, hud::HudAction::Panel(Panel::Diplomacy));
    assert_eq!(
        app.world().resource::<MissionUi>().hud.diplomacy_draft,
        None
    );
    // Switching to chat also cancels; accepting preserves the committed values.
    regression_hud_click(
        &mut app,
        window,
        hud::HudAction::Share(owner, hud::diplomacy::VISION),
    );
    regression_hud_click(&mut app, window, hud::HudAction::Chat);
    assert!(
        app.world()
            .resource::<MissionUi>()
            .hud
            .diplomacy_draft
            .is_none()
    );
    regression_hud_click(&mut app, window, hud::HudAction::Panel(Panel::Diplomacy));
    regression_hud_click(
        &mut app,
        window,
        hud::HudAction::Share(owner, hud::diplomacy::VISION),
    );
    let accepted = app
        .world()
        .resource::<MissionUi>()
        .hud
        .diplomacy_draft
        .clone()
        .unwrap();
    regression_hud_click(&mut app, window, hud::HudAction::Accept);
    regression_hud_click(&mut app, window, hud::HudAction::Panel(Panel::Diplomacy));
    assert_eq!(
        app.world().resource::<MissionUi>().hud.diplomacy.as_ref(),
        Some(&accepted)
    );
    assert!(
        app.world()
            .resource::<MissionUi>()
            .hud
            .diplomacy_draft
            .is_none()
    );
}

/// A right click on the HUD control of `action`.
fn regression_hud_right_click(app: &mut App, window: Entity, action: hud::HudAction) {
    let s = surface(app, window);
    let settings = app.world().resource::<MenuState>().settings.clone();
    let h = hud_layout(s, &settings);
    let rect = {
        let world = app.world();
        let scene = world.resource::<Loaded>().scene.as_ref().unwrap();
        let game = world.resource::<Game>();
        let layout = hud::Layout {
            width: h.size.x as i32,
            height: h.size.y as i32,
            left: settings.layout == 1,
        };
        hud::widgets(
            &game.classic,
            &game.skin.locale,
            &world.resource::<MissionUi>().hud,
            layout,
            &scene.mission,
            &scene.library,
        )
        .into_iter()
        .find(|b| b.action == action)
        .unwrap()
        .rect
    };
    move_to(
        app,
        window,
        Vec2::new(rect.x as f32 + 3., rect.y as f32 + 3.) * h.factor / s.dpi,
    );
    press(app, MouseButton::Right);
    app.update();
    release(app, MouseButton::Right);
    app.update();
}

#[test]
fn regression_tributes_open_over_diplomacy_and_send_the_chosen_amounts() {
    let root = test_root("tributes");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: Some(0),
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    let (owner, player, before) = {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let s = loaded.scene.as_mut().unwrap();
        let player = s.mission.player();
        s.mission.kingdoms[player as usize - 1].resources[0] = 250;
        let owner = hud::other_kingdoms(&s.mission)[0];
        let before = s.mission.kingdoms[owner as usize - 1].resources[0];
        (owner, player, before)
    };
    let hud = |app: &App| app.world().resource::<MissionUi>().hud.clone();
    regression_hud_click(&mut app, window, hud::HudAction::Panel(Panel::Diplomacy));
    regression_hud_click(
        &mut app,
        window,
        hud::HudAction::Share(owner, hud::diplomacy::VISION),
    );
    let draft = hud(&app).diplomacy_draft;
    assert!(draft.is_some());
    // The tribute window opens over the diplomacy one, which keeps its
    // choices (0x446f7d).
    regression_hud_click(&mut app, window, hud::HudAction::Tribute);
    assert_eq!(hud(&app).panel, Some(Panel::Tribute));
    assert_eq!(hud(&app).diplomacy_draft, draft);
    // 100 a left click, the total held to the stock (0x447970), 100 off
    // with a right click.
    for _ in 0..3 {
        regression_hud_click(&mut app, window, hud::HudAction::Amount(owner, 0));
    }
    assert_eq!(hud(&app).tribute[owner as usize - 1][0], 250);
    regression_hud_right_click(&mut app, window, hud::HudAction::Amount(owner, 0));
    assert_eq!(hud(&app).tribute[owner as usize - 1][0], 150);
    // Send moves it and goes back to the diplomacy window.
    regression_hud_click(&mut app, window, hud::HudAction::Send);
    app.update();
    assert_eq!(hud(&app).panel, Some(Panel::Diplomacy));
    assert_eq!(hud(&app).diplomacy_draft, draft);
    {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        assert_eq!(s.mission.kingdoms[player as usize - 1].resources[0], 100);
        assert_eq!(
            s.mission.kingdoms[owner as usize - 1].resources[0],
            before + 150
        );
    }
    // Esc goes back to the diplomacy window (0x447ac0), Cancel closes that
    // one; F6 opens both (0x4f5d66), with no amounts.
    regression_hud_click(&mut app, window, hud::HudAction::Tribute);
    regression_hud_click(&mut app, window, hud::HudAction::Amount(owner, 3));
    tap(&mut app, KeyCode::Escape);
    assert_eq!(hud(&app).panel, Some(Panel::Diplomacy));
    regression_hud_click(&mut app, window, hud::HudAction::Close);
    assert_eq!(hud(&app).panel, None);
    tap(&mut app, KeyCode::F6);
    assert_eq!(hud(&app).panel, Some(Panel::Tribute));
    assert_eq!(hud(&app).tribute, [[0; 4]; 8]);
}

#[test]
fn regression_heroes_spies_and_items_are_lists_in_the_grid() {
    use retzar::world::{FLAG_HELD, class};
    let root = test_root("grid-lists");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: Some(0),
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    // A held potion and an enemy unit hiding the player's spy.
    let (holder, item, host) = {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let s = loaded.scene.as_mut().unwrap();
        let player = s.mission.player();
        let holder = (0..s.mission.objects.len())
            .find(|&i| retzar::selection::own_unit(&s.mission.objects[i], &s.mission))
            .unwrap();
        let item = (0..s.mission.objects.len())
            .find(|&i| s.mission.objects[i].class == class::ITEM_HEALTH)
            .unwrap_or_else(|| {
                let mut o = s.mission.objects[holder].clone();
                o.class = class::ITEM_HEALTH;
                o.owner = 0;
                s.mission.objects.push(o);
                s.mission.objects.len() - 1
            });
        s.mission.objects[item].flags |= FLAG_HELD;
        let host = (0..s.mission.objects.len())
            .find(|&i| {
                let o = &s.mission.objects[i];
                retzar::selection::unit(o) && o.owner != player && (1..=8).contains(&o.owner)
            })
            .unwrap();
        s.mission.objects[host].mole = player;
        let sim = s.mission.simulation.as_mut().unwrap();
        while sim.objects.len() <= item {
            sim.objects.push(Default::default());
        }
        sim.objects[item].holder = Some(holder);
        (holder, item, host)
    };
    let hud = |app: &App| app.world().resource::<MissionUi>().hud.clone();
    let entries = |app: &App, kind| {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        hud::list_entries(kind, &s.mission)
    };
    // 0x462340, 0x49d720: the item the player's unit holds, the unit
    // hiding the player's spy.
    assert_eq!(entries(&app, hud::ListKind::Items), [item]);
    assert!(entries(&app, hud::ListKind::Spies).contains(&host));
    // I shows the items in the grid; the pointer shows one in the panel,
    // a click adds its holder to the selection (0x49e7e2 → 0x453240).
    tap(&mut app, KeyCode::KeyI);
    assert_eq!(hud::listing(&hud(&app)), Some(hud::ListKind::Items));
    assert_eq!(hud(&app).panel, None);
    regression_hud_click(&mut app, window, hud::HudAction::Listed(item));
    assert!(hud(&app).selected.contains(&holder));
    // The heroes of the campaign's first map: a click adds the first.
    tap(&mut app, KeyCode::Escape);
    app.world_mut()
        .resource_mut::<MissionUi>()
        .hud
        .selected
        .clear();
    app.update();
    tap(&mut app, KeyCode::KeyH);
    let heroes = entries(&app, hud::ListKind::Heroes);
    assert_eq!(hud::listing(&hud(&app)), Some(hud::ListKind::Heroes));
    regression_hud_click(&mut app, window, hud::HudAction::Listed(heroes[0]));
    assert_eq!(hud(&app).selected, [heroes[0]]);
}

#[test]
fn regression_ctrl_and_shift_with_f5_to_f8_keep_and_recall_the_view() {
    let root = test_root("bookmarks");
    let (mut app, _) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: Some(0),
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    let with = |app: &mut App, modifier: KeyCode, key: KeyCode| {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(modifier);
        tap(app, key);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(modifier);
        app.update();
    };
    let kept = app.world().resource::<MapView>().center;
    // Ctrl + F7 keeps the view in bookmark 3 (0x4f9060) with its line.
    with(&mut app, KeyCode::ControlLeft, KeyCode::F7);
    assert_eq!(app.world().resource::<MissionUi>().bookmarks[2], Some(kept));
    assert_eq!(
        app.world().resource::<MenuState>().notice,
        "Situazione geografica determinata in 3"
    );
    // Shift + F7 brings it back; with Ctrl or Shift, F5 and F6 open nothing.
    app.world_mut().resource_mut::<MapView>().center = kept + Vec2::new(320., -160.);
    with(&mut app, KeyCode::ShiftLeft, KeyCode::F7);
    assert_eq!(app.world().resource::<MapView>().center, kept);
    with(&mut app, KeyCode::ControlLeft, KeyCode::F5);
    with(&mut app, KeyCode::ShiftLeft, KeyCode::F6);
    assert_eq!(app.world().resource::<MissionUi>().hud.panel, None);
}

#[test]
fn regression_working_commands_use_advertised_hotkeys() {
    let root = test_root("audit-hotkeys");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: Some(0),
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    let index = {
        let w = app.world();
        let game = w.resource::<Game>();
        let ui = w.resource::<MissionUi>();
        let s = w.resource::<Loaded>().scene.as_ref().unwrap();
        let commands = hud::commands(
            &game.classic,
            &game.skin.locale,
            &ui.hud,
            &s.mission,
            &s.library,
        );
        let index = commands
            .iter()
            .position(|c| c.order == "view_quests")
            .unwrap();
        assert_eq!(commands[index].hotkey, "Q");
        index
    };
    tap(&mut app, KeyCode::KeyQ);
    assert_eq!(
        app.world().resource::<MissionUi>().hud.panel,
        Some(Panel::Quests)
    );
    tap(&mut app, KeyCode::Escape);
    regression_hud_click(&mut app, window, hud::HudAction::Command(index));
    assert_eq!(
        app.world().resource::<MissionUi>().hud.panel,
        Some(Panel::Quests)
    );
    // H, S and I show their lists in the grid (`GET heroes`, …), each only
    // when it is not empty (0x49dd21); Escape goes back.
    tap(&mut app, KeyCode::Escape);
    for (key, kind) in [
        (KeyCode::KeyS, hud::ListKind::Spies),
        (KeyCode::KeyI, hud::ListKind::Items),
        (KeyCode::KeyH, hud::ListKind::Heroes),
    ] {
        let listed = {
            let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
            !hud::list_entries(kind, &s.mission).is_empty()
        };
        tap(&mut app, key);
        let hud = app.world().resource::<MissionUi>().hud.clone();
        assert_eq!(hud.panel, None);
        assert_eq!(hud::listing(&hud), listed.then_some(kind));
        if kind != hud::ListKind::Heroes {
            tap(&mut app, KeyCode::Escape);
        } else {
            assert!(listed, "the campaign opens with its heroes");
        }
    }
    // The list takes the grid's place: Q opens nothing there.
    tap(&mut app, KeyCode::KeyQ);
    assert_eq!(app.world().resource::<MissionUi>().hud.panel, None);
    tap(&mut app, KeyCode::Escape);
    assert_eq!(hud::listing(&app.world().resource::<MissionUi>().hud), None);
    // A dialog consumes shortcuts; typing Q in chat cannot open Quests.
    regression_hud_click(&mut app, window, hud::HudAction::Chat);
    tap(&mut app, KeyCode::KeyQ);
    assert!(matches!(
        app.world().resource::<MissionUi>().hud.panel,
        Some(Panel::Chat(_))
    ));
    tap(&mut app, KeyCode::Escape);
    tap(&mut app, KeyCode::KeyN);
    assert_eq!(app.world().resource::<MissionUi>().hud.selected.len(), 1);
    app.world_mut()
        .resource_mut::<MissionUi>()
        .hud
        .selected
        .clear();
    tap(&mut app, KeyCode::KeyP);
    app.update();
    assert_eq!(*app.world().resource::<State<Mode>>().get(), Mode::Menu);
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Help);
}

#[test]
fn regression_infinite_resource_label_is_supported_by_original_font() {
    let a = GameAssets::open(&source()).unwrap();
    let skin = HudSkin::load(&a).unwrap();
    let catalog = retzar::scenario::ScenarioCatalog::load(&source()).unwrap();
    let m = &catalog.missions[0];
    let c = m.container().unwrap();
    let grid = retzar::map::MapGrid::read(&c, &m.header).unwrap();
    let mission = retzar::world::MissionWorld::read(&c, &grid).unwrap();
    let mut resource = mission.objects[0].clone();
    resource.class = retzar::world::class::RES_GOLD;
    resource.data.resize(resource.data.len().max(0x40), 0);
    resource.data[0x3f] = 1;
    assert_eq!(
        hud::statistics(&skin, &resource),
        vec![("resgold", "Infinito".to_owned())]
    );
    let mut infinity = vec![0u8; 96 * 32 * 4];
    let mut question = infinity.clone();
    skin.font
        .draw(&mut infinity, 96, 32, "Infinito", 0, 0, [255, 255, 255]);
    skin.font
        .draw(&mut question, 96, 32, "?", 0, 0, [255, 255, 255]);
    assert_ne!(infinity, question);
    assert!(infinity.iter().any(|&v| v != 0));
}

fn selection_fixture() -> (
    GameAssets,
    MenuSkin,
    HudSkin,
    retzar::world::ObjectLibrary,
    retzar::world::MissionWorld,
) {
    let a = GameAssets::open(&source()).unwrap();
    let catalog = retzar::scenario::ScenarioCatalog::load(&source()).unwrap();
    let m = &catalog.missions[0];
    let c = m.container().unwrap();
    let grid = retzar::map::MapGrid::read(&c, &m.header).unwrap();
    let mission = retzar::world::MissionWorld::read(&c, &grid).unwrap();
    let library = retzar::world::ObjectLibrary::load(&a, "grass").unwrap();
    let skin = MenuSkin::load(&a).unwrap();
    let hud = HudSkin::load(&a).unwrap();
    (a, skin, hud, library, mission)
}

#[test]
fn regression_spy_orders_join_only_a_selection_that_carries_a_spy() {
    let (_, menu, skin, library, mut mission) = selection_fixture();
    let spy_orders = |mission: &retzar::world::MissionWorld, selected: Vec<usize>| {
        let state = hud::HudState {
            selected,
            ..default()
        };
        hud::commands(&skin, &menu.locale, &state, mission, &library)
            .iter()
            .filter(|c| matches!(c.order.as_str(), "impersonate" | "reveal_self"))
            .count()
    };
    // 0x49ee25: nothing selected shows only the kingdom's menu.
    assert_eq!(spy_orders(&mission, vec![]), 0);
    let unit = mission
        .objects
        .iter()
        .position(|o| o.owner == mission.player() && retzar::selection::unit(o))
        .unwrap();
    assert_eq!(spy_orders(&mission, vec![unit]), 0);
    // 0x49f0cc: the player's spy brings «Travestiti» and «Svela l'identità».
    mission.objects[unit].class = 14;
    assert_eq!(spy_orders(&mission, vec![unit]), 2);
}

#[test]
fn regression_selection_portraits_use_all_eight_kingdom_palettes() {
    use retzar::{assets::Ini, bitmap::Bitmap, world::class};
    let (a, menu, skin, library, mut mission) = selection_fixture();
    let i = mission
        .objects
        .iter()
        .position(|o| o.class == class::PEON && o.owner == mission.player())
        .unwrap();
    assert!(mission.objects[i].portrait.is_empty());
    let owner = mission.objects[i].owner as usize - 1;
    let icons = Ini::parse(&a.get("DATA/ICONS.INI").unwrap());
    let key = library.icon_for(&mission.objects[i]).unwrap();
    let path = format!(
        "IMAGES/{}",
        icons.get(key, "image").unwrap().replace('\\', "/")
    );
    let index: usize = icons.get(key, "index").unwrap().parse().unwrap();
    let data = a.get(&path).unwrap();
    let frame = Bitmap::decode(&a.get("IMAGES/ORDERS/FRAME1.BMP").unwrap(), true).unwrap();
    assert!(Bitmap::is_indexed(&data));
    let w = u32::from_le_bytes(data[18..22].try_into().unwrap()) as usize;
    let h = i32::from_le_bytes(data[22..26].try_into().unwrap()) as usize;
    let offset = u32::from_le_bytes(data[10..14].try_into().unwrap()) as usize;
    let stride = w.div_ceil(4) * 4;
    let state = hud::HudState {
        selected: vec![i],
        ..default()
    };
    let portraits = std::collections::HashMap::new();
    for slot in 0..10 {
        // Intentionally differs from the owner number: setup colours must win.
        if slot < 8 {
            mission.kingdoms[owner].color = slot as u8 + 1;
        }
        mission.objects[i].owner = match slot {
            8 => 0,
            9 => 10,
            _ => owner as u8 + 1,
        };
        let image = hud::render(
            &menu,
            &skin,
            skin.background(800, 600, true),
            hud::HudView {
                width: 800,
                height: 600,
                left: true,
                title: "",
                description: "",
                state: &state,
                mission: &mission,
                library: &library,
                portraits: &portraits,
                camera: [0.; 3],
                side: 64,
                pixels: [1.; 2],
                messages: &[],
                status: "",
                player_name: "Carlo",
                selection: [255, 255, 255],
                map_text: 0,
            },
        );
        let crop = image.crop(12, 174, 48, 48).unwrap();
        let mut reserved = 0;
        let mut unchanged = 0;
        for y in 0..48 {
            for x in 0..48 {
                let at = (y * 48 + x) * 4;
                if frame.rgba[at + 3] != 0 {
                    continue;
                }
                let raw = data[offset + (h - 1 - (index * 48 + y)) * stride + x] as usize;
                let mapped = if raw >= 240 && slot < 8 {
                    112 + 16 * slot + raw - 240
                } else {
                    raw
                };
                let palette = &data[54 + mapped * 4..54 + mapped * 4 + 3];
                if palette == [0, 255, 0] {
                    continue;
                }
                assert_eq!(
                    &crop.rgba[at..at + 3],
                    &[palette[2], palette[1], palette[0]],
                    "slot {slot}, ({x},{y}), index {raw}"
                );
                if raw >= 240 {
                    reserved += 1
                } else {
                    unchanged += 1
                }
            }
        }
        assert!(reserved > 10 && unchanged > 100);
    }
}

#[test]
fn regression_rgb555_hero_portrait_keeps_its_original_artwork() {
    use retzar::bitmap::Bitmap;
    let (a, menu, skin, library, mut mission) = selection_fixture();
    let i = mission
        .objects
        .iter()
        .position(|o| retzar::selection::own_unit(o, &mission))
        .unwrap();
    let owner = mission.objects[i].owner as usize - 1;
    let data = a.get("IMAGES/ICONS/HERO.BMP").unwrap();
    assert!(!Bitmap::is_indexed(&data));
    let portrait = Bitmap::decode(&data, true).unwrap();
    mission.objects[i].portrait = "USER/HERO.BMP".into();
    let portraits = std::collections::HashMap::from([("USER/HERO.BMP".to_owned(), portrait)]);
    let state = hud::HudState {
        selected: vec![i],
        ..default()
    };
    let mut previous = None;
    for slot in 1..=8 {
        mission.kingdoms[owner].color = slot;
        let image = hud::render(
            &menu,
            &skin,
            skin.background(800, 600, true),
            hud::HudView {
                width: 800,
                height: 600,
                left: true,
                title: "",
                description: "",
                state: &state,
                mission: &mission,
                library: &library,
                portraits: &portraits,
                camera: [0.; 3],
                side: 64,
                pixels: [1.; 2],
                messages: &[],
                status: "",
                player_name: "Carlo",
                selection: [255, 255, 255],
                map_text: 0,
            },
        )
        .crop(12, 174, 48, 48)
        .unwrap();
        if let Some(old) = previous {
            assert_eq!(image.rgba, old);
        }
        previous = Some(image.rgba);
    }
}

#[test]
fn regression_original_group_limit_toggle_membership_and_mean_health() {
    use retzar::{selection, world::class};
    let (_, _, skin, library, mut mission) = selection_fixture();
    let mut unit = mission
        .objects
        .iter()
        .find(|o| selection::own_unit(o, &mission))
        .unwrap()
        .clone();
    unit.class = class::PEON;
    unit.civilization = 1;
    unit.health = 50;
    unit.max_health = 50;
    let mut army = unit.clone();
    army.class = class::FOOTMAN;
    army.health = 325;
    army.max_health = 650;
    let mut enemy = army.clone();
    enemy.owner = 8;
    let mut building = unit.clone();
    building.class = class::CASTLE;
    mission.objects = vec![unit.clone(), army, enemy, building];
    mission.objects.extend((4..70).map(|_| unit.clone()));
    let mut selected = Vec::new();
    selection::apply(&mut selected, &[0, 1], false, &mission);
    let mut state = hud::HudState {
        selected,
        ..default()
    };
    assert_eq!(
        hud::group_health(&state, &mission),
        75,
        "mean of 100% and 50%, not pooled HP"
    );
    let commands = hud::commands(
        &skin,
        &retzar::assets::Ini::default(),
        &state,
        &mission,
        &library,
    );
    assert!(commands.iter().any(|c| c.order == "move"));
    assert!(
        !commands
            .iter()
            .any(|c| c.order.contains("build") || c.order == "harvest")
    );
    state.inspect_next();
    assert_eq!(state.inspected, Some(0));
    state.inspect_next();
    assert_eq!(state.inspected, Some(1));
    state.inspect_next();
    assert_eq!(state.inspected, Some(0));
    assert_eq!(state.selected, vec![0, 1]);
    selection::apply(&mut state.selected, &[0], true, &mission);
    assert_eq!(state.selected, vec![1]);
    selection::apply(&mut state.selected, &[2], true, &mission);
    assert_eq!(
        state.selected,
        vec![2],
        "an enemy is inspected individually"
    );
    selection::apply(&mut state.selected, &[0, 1], true, &mission);
    assert_eq!(state.selected, vec![0, 1]);
    selection::apply(&mut state.selected, &[3], true, &mission);
    assert_eq!(
        state.selected,
        vec![3],
        "a building is always alone (0x453240)"
    );
    selection::apply(&mut state.selected, &[0], true, &mission);
    assert_eq!(state.selected, vec![0], "and leaves when a unit joins");
    selection::apply(
        &mut state.selected,
        &(4..70).collect::<Vec<_>>(),
        false,
        &mission,
    );
    assert_eq!(state.selected.len(), 50);
    selection::apply(&mut state.selected, &[0, 1], true, &mission);
    assert_eq!(state.selected.len(), 50, "Shift respects the limit too");
    let mut groups = [const { Vec::new() }; 9];
    selection::assign_group(&mut groups, 0, &[0, 1, 2], &mission);
    assert_eq!(groups[0], vec![0, 1]);
    selection::assign_group(&mut groups, 1, &[1], &mission);
    assert_eq!(groups[0], vec![0]);
    assert_eq!(groups[1], vec![1]);
    selection::assign_group(&mut groups, 2, &[3], &mission);
    assert_eq!(
        groups[2],
        vec![3],
        "a single own building can be saved in a group"
    );
    selection::remove_from_groups(&mut groups, &[0, 1]);
    assert!(groups[0].is_empty() && groups[1].is_empty());
    assert_eq!(groups[2], vec![3]);
}

#[test]
fn regression_map_selection_double_click_box_tab_escape_and_group_shortcuts() {
    use retzar::world::{Layer, class};
    let root = test_root("selection-input");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: None,
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let s = loaded.scene.as_mut().unwrap();
        let mut o = s
            .mission
            .objects
            .iter()
            .find(|o| retzar::selection::own_unit(o, &s.mission))
            .unwrap()
            .clone();
        o.class = class::PEON;
        o.civilization = 1;
        o.x = 32;
        o.y = 32;
        let mut body = *s.visuals.iter().find(|v| v.layer == Layer::Body).unwrap();
        body.animation = None;
        s.mission.objects = (0..6)
            .map(|i| {
                let mut unit = o.clone();
                unit.id = 100 + i;
                match i {
                    1 => unit.x += 1,
                    2 => {
                        unit.x += 2;
                        unit.owner = if o.owner == 1 { 2 } else { 1 };
                    }
                    3 => {
                        unit.y += 2;
                        unit.class = class::CASTLE;
                    }
                    4 => {
                        unit.y += 1;
                        unit.class = class::FOOTMAN;
                    }
                    5 => {
                        unit.x = 0;
                        unit.y = 0;
                    }
                    _ => (),
                }
                unit
            })
            .collect();
        s.visuals = s
            .mission
            .objects
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let mut v = body;
                v.object = i;
                v.bounds = [o.x as f32 * 32., -(o.y as f32) * 32., 32., 32.];
                v.footprint = v.bounds;
                v
            })
            .collect();
        s.visuals.push(s.visuals[0]); // Multiple bodies must never duplicate an object.
        // This test relocates every object into a synthetic input arena. Rebuild
        // the observer state too, rather than retaining the real map's visibility.
        s.mission.simulation = Some(retzar::simulation::Simulation::new(
            &s.mission, &s.library, s.side, &s.biome,
        ));
    }
    let center = Vec2::new(1040., -1008.);
    app.world_mut().insert_resource(MapView {
        center,
        scale: 0.25,
        frame: Default::default(),
    });
    let cursor_for = |app: &App, at: Vec2| {
        let s = surface(app, window);
        let view = app.world().resource::<MapView>();
        s.physical / s.dpi / 2. + Vec2::new(at.x - view.center.x, view.center.y - at.y) / view.scale
    };
    let click_at = |app: &mut App, at: Vec2| {
        let p = cursor_for(app, at);
        move_to(app, window, p);
        press(app, MouseButton::Left);
        app.update();
        release(app, MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
    };
    click_at(&mut app, center);
    assert_eq!(app.world().resource::<MissionUi>().hud.selected, vec![0]);
    click_at(&mut app, center);
    assert_eq!(
        app.world().resource::<MissionUi>().hud.selected,
        vec![0, 1],
        "double click: same type, visible, own, deduplicated and nearest first"
    );
    tap(&mut app, KeyCode::Tab);
    assert_eq!(app.world().resource::<MissionUi>().hud.inspected, Some(0));
    tap(&mut app, KeyCode::Tab);
    assert_eq!(app.world().resource::<MissionUi>().hud.inspected, Some(1));
    assert_eq!(app.world().resource::<MissionUi>().hud.selected, vec![0, 1]);
    tap(&mut app, KeyCode::Digit9);
    assert_eq!(
        app.world().resource::<MissionUi>().hud.selected,
        vec![0, 1],
        "empty group does not deselect"
    );
    for (key, state) in [
        (KeyCode::ControlLeft, ButtonState::Pressed),
        (KeyCode::Digit1, ButtonState::Pressed),
        (KeyCode::ControlLeft, ButtonState::Released),
    ] {
        app.world_mut().write_message(keyboard_event(key, state));
    }
    tap(&mut app, KeyCode::Digit1);
    assert_eq!(app.world().resource::<MissionUi>().groups[0], vec![0, 1]);
    for (key, state) in [
        (KeyCode::ControlLeft, ButtonState::Pressed),
        (KeyCode::Digit0, ButtonState::Pressed),
        (KeyCode::ControlLeft, ButtonState::Released),
    ] {
        app.world_mut().write_message(keyboard_event(key, state));
    }
    tap(&mut app, KeyCode::Digit0);
    assert!(
        app.world()
            .resource::<MissionUi>()
            .groups
            .iter()
            .all(Vec::is_empty)
    );
    // Box spans buildings, an enemy and a second class; only own units enter.
    let p = cursor_for(&app, center - Vec2::new(20., 85.));
    move_to(&mut app, window, p);
    press(&mut app, MouseButton::Left);
    app.update();
    let p = cursor_for(&app, center + Vec2::new(90., 20.));
    move_to(&mut app, window, p);
    release(&mut app, MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    assert_eq!(
        app.world().resource::<MissionUi>().hud.selected,
        vec![0, 1, 4]
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    click_at(&mut app, center);
    assert_eq!(
        app.world().resource::<MissionUi>().hud.selected,
        vec![1, 4],
        "Shift click removes a member"
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ShiftLeft);
    // Escape cancels an unfinished drag as well as the existing group.
    let p = cursor_for(&app, center);
    move_to(&mut app, window, p);
    press(&mut app, MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert!(app.world().resource::<MissionUi>().hud.selected.is_empty());
    assert!(app.world().resource::<MissionUi>().select_start.is_none());
    release(&mut app, MouseButton::Left);
    app.update();
    assert!(app.world().resource::<MissionUi>().hud.selected.is_empty());
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    assert_eq!(*app.world().resource::<State<Mode>>().get(), Mode::Mission);
    tap(&mut app, KeyCode::F5);
    assert_eq!(
        app.world().resource::<MissionUi>().hud.panel,
        Some(Panel::Diplomacy)
    );
    tap(&mut app, KeyCode::Escape);
    tap(&mut app, KeyCode::F8);
    assert_eq!(
        app.world().resource::<MissionUi>().hud.panel,
        Some(Panel::Quests)
    );
    tap(&mut app, KeyCode::Escape);
    tap(&mut app, KeyCode::F10);
    app.update();
    assert_eq!(*app.world().resource::<State<Mode>>().get(), Mode::Menu);
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Pause);
}

#[test]
fn original_click_reaches_units_trees_and_a_box_empties_the_selection() {
    use retzar::world::{Layer, class};
    let root = test_root("selection-click");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: None,
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    let tree = {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let s = loaded.scene.as_mut().unwrap();
        let mut o = s
            .mission
            .objects
            .iter()
            .find(|o| retzar::selection::own_unit(o, &s.mission))
            .unwrap()
            .clone();
        o.class = class::PEON;
        o.civilization = 1;
        o.y = 32;
        let mut body = *s.visuals.iter().find(|v| v.layer == Layer::Body).unwrap();
        body.animation = None;
        s.mission.objects = [32u16, 34]
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                let mut unit = o.clone();
                unit.id = 100 + i as u16;
                unit.x = x;
                unit
            })
            .collect();
        // Each picture's box over its own cell.
        s.visuals = s
            .mission
            .objects
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let mut v = body;
                v.object = i;
                v.bounds = [o.x as f32 * 32., -(o.y as f32 + 1.) * 32., 32., 32.];
                v.footprint = v.bounds;
                v
            })
            .collect();
        s.mission.simulation = Some(retzar::simulation::Simulation::new(
            &s.mission, &s.library, s.side, &s.biome,
        ));
        let side = s.side;
        let (tree, bare) = (35 * side + 34, 31 * side + 31);
        let image = (s.library.trunks as u16 + 1) << 11;
        s.grid.cells[tree] = (s.grid.cells[tree] & !0xf800) | image;
        s.grid.cells[bare] &= !0xf800;
        tree
    };
    let cursor_for = |app: &App, at: Vec2| {
        let s = surface(app, window);
        let view = app.world().resource::<MapView>();
        s.physical / s.dpi / 2. + Vec2::new(at.x - view.center.x, view.center.y - at.y) / view.scale
    };
    let pixel = |x: f32, y: f32| Vec2::new(x, -y);
    app.world_mut().insert_resource(MapView {
        center: pixel(1088., 1088.),
        scale: 0.25,
        frame: Default::default(),
    });
    let click_at = |app: &mut App, at: Vec2| {
        let p = cursor_for(app, at);
        move_to(app, window, p);
        press(app, MouseButton::Left);
        app.update();
        release(app, MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
    };
    let ui = |app: &App| app.world().resource::<MissionUi>().hud.clone();
    // 0x44e900: 20 pixels right of the first peasant's middle, off its
    // box, takes it; the other is 44 pixels away.
    click_at(&mut app, pixel(1040. + 20., 1040.));
    assert_eq!(ui(&app).selected, vec![0]);
    // 31 pixels each way from it, on a cell with nothing: nothing.
    click_at(&mut app, pixel(1040. - 31., 1040. - 31.));
    assert!(ui(&app).selected.is_empty());
    // 0x453440: a standing tree in view goes alone into the selection, with
    // its wood of 100; Shift + a peasant puts it out.
    click_at(&mut app, pixel(34. * 32. + 16., 35. * 32. + 16.));
    assert_eq!(ui(&app).tree, Some((tree, 100)));
    assert!(ui(&app).selected.is_empty());
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    click_at(&mut app, pixel(1040., 1040.));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ShiftLeft);
    app.update();
    assert_eq!(ui(&app).selected, vec![0]);
    assert_eq!(ui(&app).tree, None);
    // A felled tree leaves the selection (message 0x2204).
    click_at(&mut app, pixel(34. * 32. + 16., 35. * 32. + 16.));
    assert_eq!(ui(&app).tree, Some((tree, 100)));
    app.world_mut()
        .resource_mut::<Loaded>()
        .scene
        .as_mut()
        .unwrap()
        .grid
        .cells[tree] &= !0xf800;
    app.update();
    assert_eq!(ui(&app).tree, None);
    // 0x4f49a8: once the drag is a rectangle, the selection empties before
    // the release, and the rectangle is drawn from then on.
    click_at(&mut app, pixel(1040., 1040.));
    assert_eq!(ui(&app).selected, vec![0]);
    let p = cursor_for(&app, pixel(1060., 1100.));
    move_to(&mut app, window, p);
    press(&mut app, MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    assert!(!app.world().resource::<MissionUi>().boxing);
    assert_eq!(ui(&app).selected, vec![0]);
    let p = cursor_for(&app, pixel(1070., 1110.));
    move_to(&mut app, window, p);
    app.update();
    assert!(app.world().resource::<MissionUi>().boxing);
    assert!(ui(&app).selected.is_empty());
    release(&mut app, MouseButton::Left);
    app.update();
    assert!(ui(&app).selected.is_empty());
}

#[test]
fn regression_command_dialogs_take_the_place_of_the_grid() {
    use retzar::{bitmap::Bitmap, dialogs::Dialog, hud::HudAction};
    let (a, menu, skin, library, mission) = selection_fixture();
    let i = mission
        .objects
        .iter()
        .position(|o| o.owner == mission.player())
        .unwrap();
    let layout = hud::Layout {
        width: 800,
        height: 600,
        left: true,
    };
    let portraits = std::collections::HashMap::new();
    for (folder, dialog, hint) in [
        ("D_GAMBLE", Dialog::gamble(i), "Risorsa da scommettere"),
        (
            "D_TRADE1",
            Dialog::exchange(i, None),
            "Risorsa che consegni",
        ),
        (
            "D_TRADE",
            Dialog::route(i, [None; 2], [0, 0], None),
            "Origine",
        ),
        (
            "D_BUILD",
            Dialog::workshop(i, (0, 0)),
            "Libro degli incantesimi",
        ),
    ] {
        let state = hud::HudState {
            selected: vec![i],
            dialog: Some(dialog),
            ..default()
        };
        let buttons = hud::widgets(&skin, &menu.locale, &state, layout, &mission, &library);
        // No command button under the dialog; the first control's hint is
        // its [orders_names] text.
        assert!(
            !buttons
                .iter()
                .any(|b| matches!(b.action, HudAction::Command(_)))
        );
        let first = buttons
            .iter()
            .find(|b| matches!(b.action, HudAction::Dialog(_)))
            .unwrap();
        assert_eq!(first.label, hint, "{folder}");
        let image = hud::render(
            &menu,
            &skin,
            skin.background(800, 600, true),
            hud::HudView {
                width: 800,
                height: 600,
                left: true,
                title: "",
                description: "",
                state: &state,
                mission: &mission,
                library: &library,
                portraits: &portraits,
                camera: [0.; 3],
                side: 64,
                pixels: [1.; 2],
                messages: &[],
                status: "",
                player_name: "Carlo",
                selection: [255, 255, 255],
                map_text: 0,
            },
        );
        // back_v.bmp lies at (16,332), over the grid.
        let path = format!("IMAGES/SCREENS/{folder}/BACK_V.BMP");
        let back = Bitmap::decode(&a.get(&path).unwrap(), true).unwrap();
        assert_eq!((back.width, back.height), (116, 225));
        for (x, y) in [(1usize, 1usize), (114, 223), (58, 2), (2, 112)] {
            let at = (y * back.width as usize + x) * 4;
            if back.rgba[at + 3] == 0 {
                continue;
            }
            let out = ((332 + y) * 800 + 16 + x) * 4;
            assert_eq!(
                &image.rgba[out..out + 3],
                &back.rgba[at..at + 3],
                "{folder} ({x},{y})"
            );
        }
    }
}

#[test]
fn regression_effect_pictures_of_the_original_all_load() {
    let (a, _, _, library, _) = selection_fixture();
    for name in [
        "arrow",
        "arrow_big",
        "flame_bolt",
        "flame_bolt_expl",
        "cb_stone",
        "cb_metal",
        "cb_fire",
        "cb_metal_shadow",
        "trail",
        "arrow_rocket",
        "rocket_trail",
        "arrow_ballista",
        "arrow_ballista_shadow",
        "freeze_ball_trail",
        "flame_ball_trail",
        "freeze_bolt_expl",
        "fire_rain",
        "expl64",
        "expl_small_hole",
        "expl_small_water",
        "explosion_small",
        "healing",
        "bless",
        "curse",
        "hypnotise",
        "convert_exp",
        "turn_to_stone",
        "magic_star",
    ] {
        let p = library
            .effect_picture(&a, name)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(p.frames.len(), p.columns * p.rows, "{name}");
        assert!(
            p.frames.iter().all(|f| f.width > 0 && f.height > 0),
            "{name}"
        );
    }
    // 24 directions for the arrows; the explosion's hotspot (31, 31).
    assert_eq!(library.effect_picture(&a, "arrow").unwrap().rows, 24);
    assert_eq!(
        library.effect_picture(&a, "expl64").unwrap().anchor,
        [31, 31]
    );
}

#[test]
fn units_hiding_the_players_spy_show_its_orders() {
    use retzar::{
        simulation::Simulation,
        world::{MapObject, class},
    };
    let (_, menu, skin, library, mut mission) = selection_fixture();
    let player = mission.player();
    let template = mission
        .objects
        .iter()
        .find(|o| o.owner == player && retzar::selection::unit(o))
        .unwrap()
        .clone();
    let enemy = (1..=8u8).find(|&k| k != player).unwrap();
    let object = |class, owner, civilization, mole| MapObject {
        class,
        owner,
        civilization,
        mole,
        control: [0; 8],
        sight: [0; 8],
        flags: 0x300,
        health: 100,
        max_health: 100,
        ..template.clone()
    };
    mission.objects = vec![
        object(14, player, 1, 0),
        object(class::FOOTMAN, enemy, 1, player),
        object(class::PEON, enemy, 2, player),
        object(class::FOOTMAN, enemy, 1, 0),
    ];
    mission.simulation = Some(Simulation::new(&mission, &library, 64, "grass"));
    let orders = |i: usize| {
        let state = hud::HudState {
            selected: vec![i],
            ..default()
        };
        hud::commands(&skin, &menu.locale, &state, &mission, &library)
            .into_iter()
            .map(|c| c.order)
            .collect::<Vec<_>>()
    };
    let spy = ["impersonate".to_owned(), "reveal_self".to_owned()];
    // 0x49f0cc: the player's own spy and a unit hiding it carry its orders.
    assert!(orders(0).ends_with(&spy));
    let footman = orders(1);
    assert!(footman.contains(&"attack".to_owned()) && footman.ends_with(&spy));
    // 0x4aede0: a peasant hiding it shows the spy's peasant menu of its
    // civilization (Arab: become a jihad warrior, bribe).
    let peasant = orders(2);
    assert!(peasant.contains(&"become_jihadw".to_owned()));
    assert!(!peasant.iter().any(|o| o == "r_build"));
    assert!(peasant.ends_with(&spy));
    // Anyone else's unit shows nothing.
    assert!(orders(3).is_empty());
}

#[test]
fn regression_the_cheat_panel_opens_from_its_button_with_nothing_selected() {
    use hud::HudAction;
    use hud::cheat_panel::{CheatControl as C, CheatTab};
    use retzar::menu::OptionKey;
    use retzar::world::{FLAG_CONSTRUCTION, class};
    let root = test_root("cheats");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    let switch = |app: &App| {
        let w = app.world();
        w.resource::<Game>()
            .skin
            .widgets(w.resource::<MenuState>())
            .iter()
            .any(|b| b.action == Action::Change(OptionKey::Cheats, 1))
    };
    let act = |app: &mut App, a: Action| {
        app.world_mut().resource_mut::<PendingAction>().0 = Some(a);
        app.update();
    };
    let widgets = |app: &App| {
        let w = app.world();
        let menu = w.resource::<MenuState>();
        let scene = w.resource::<Loaded>().scene.as_ref().unwrap();
        let game = w.resource::<Game>();
        let h = hud_layout(surface(app, window), &menu.settings);
        hud::widgets(
            &game.classic,
            &game.skin.locale,
            &w.resource::<MissionUi>().hud,
            hud::Layout {
                width: h.size.x as i32,
                height: h.size.y as i32,
                left: menu.settings.layout == 1,
            },
            &scene.mission,
            &scene.library,
        )
    };
    let button = |app: &App| {
        widgets(app)
            .iter()
            .any(|b| b.action == HudAction::OpenCheats)
    };
    // The settings of the main menu do not offer it.
    {
        let mut menu = app.world_mut().resource_mut::<MenuState>();
        menu.tab = retzar::menu::SettingsTab::Game;
        menu.open(Screen::Options);
    }
    assert!(!switch(&app));
    app.world_mut()
        .resource_mut::<MenuState>()
        .open(Screen::Main);
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: Some(0),
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    let hud = |app: &App| app.world().resource::<MissionUi>().hud.clone();
    app.world_mut()
        .resource_mut::<MissionUi>()
        .hud
        .selected
        .clear();
    app.update();
    // Off, neither the button nor F7.
    assert!(!button(&app));
    tap(&mut app, KeyCode::F7);
    assert_eq!(hud(&app).panel, None);
    // Switched on from the game's menu the panel does not open by itself.
    tap(&mut app, KeyCode::F10);
    app.update();
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Pause);
    act(&mut app, Action::Open(Screen::Options));
    act(&mut app, Action::Tab(retzar::menu::SettingsTab::Game));
    assert!(switch(&app));
    // The game's speed follows its slider at once; «Annulla» takes it back.
    let speed = |app: &App| {
        let loaded = app.world().resource::<Loaded>();
        let scene = loaded.scene.as_ref().unwrap();
        scene.mission.simulation.as_ref().unwrap().speed
    };
    let normal = speed(&app);
    act(&mut app, Action::Change(OptionKey::GameSpeed, 25));
    assert_ne!(speed(&app), normal);
    act(&mut app, Action::Back);
    assert_eq!(speed(&app), normal);
    act(&mut app, Action::Open(Screen::Options));
    // The switch works at once, and «Annulla» takes it back.
    act(&mut app, Action::Change(OptionKey::Cheats, 1));
    assert!(hud(&app).cheats);
    act(&mut app, Action::Back);
    assert!(!hud(&app).cheats);
    act(&mut app, Action::Open(Screen::Options));
    act(&mut app, Action::Change(OptionKey::Cheats, 1));
    act(&mut app, Action::Apply);
    act(&mut app, Action::Back);
    assert!(hud(&app).cheats);
    assert_eq!(hud(&app).panel, None);
    // The button shows only while nothing is selected.
    assert!(button(&app));
    let unit = {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        let player = s.mission.player();
        s.mission
            .objects
            .iter()
            .position(|o| o.owner == player && retzar::selection::unit(o))
            .unwrap()
    };
    app.world_mut().resource_mut::<MissionUi>().hud.selected = vec![unit];
    app.update();
    assert!(!button(&app));
    tap(&mut app, KeyCode::F7);
    assert_eq!(hud(&app).panel, None);
    app.world_mut()
        .resource_mut::<MissionUi>()
        .hud
        .selected
        .clear();
    app.update();
    regression_hud_click(&mut app, window, HudAction::OpenCheats);
    assert_eq!(hud(&app).panel, Some(Panel::Cheats));
    // Every page explains each of its controls on the hint line.
    for tab in [
        CheatTab::Game,
        CheatTab::Objects,
        CheatTab::Units,
        CheatTab::Buildings,
        CheatTab::Diplomacy,
    ] {
        regression_hud_click(&mut app, window, HudAction::Cheat(C::Tab(tab)));
        assert_eq!(hud(&app).cheat.tab, tab);
        let all = widgets(&app);
        let controls: Vec<_> = all
            .iter()
            .filter(|b| matches!(b.action, HudAction::Cheat(_)))
            .collect();
        assert!(controls.len() > 5, "{tab:?}");
        assert!(controls.iter().all(|b| b.label.len() > 8), "{tab:?}");
        // Each object to create says what it is, from the original's help.
        for b in all.iter() {
            if let HudAction::Cheat(C::Spawn(class)) = b.action {
                assert!(b.label.contains(": "), "{class}: {}", b.label);
            }
        }
    }
    // A button gives the player 10 000 of each resource and marks the game.
    regression_hud_click(&mut app, window, HudAction::Cheat(C::Tab(CheatTab::Game)));
    let resources = |app: &App| {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        (
            s.mission.kingdoms[s.mission.player() as usize - 1].resources,
            s.mission.cheater,
        )
    };
    let (before, cheater) = resources(&app);
    assert!(!cheater);
    regression_hud_click(&mut app, window, HudAction::Cheat(C::Resources(1)));
    app.update();
    assert_eq!(resources(&app), (before.map(|r| r + 10_000), true));
    // An icon closes the panel and waits for a click on the map, which puts
    // the object there: a house comes built.
    let place = |app: &mut App, class: usize| {
        regression_hud_click(app, window, HudAction::OpenCheats);
        let tab = if class == class::HOUSE {
            CheatTab::Buildings
        } else {
            CheatTab::Units
        };
        regression_hud_click(app, window, HudAction::Cheat(C::Tab(tab)));
        regression_hud_click(app, window, HudAction::Cheat(C::Spawn(class)));
        assert_eq!(hud(app).panel, None);
        let (spawned, civ, owner) = app.world().resource::<MissionUi>().spawning.unwrap();
        assert_eq!(spawned, class);
        let (cell, count) = {
            let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
            let sim = s.mission.simulation.as_ref().unwrap();
            assert_eq!(owner, s.mission.player());
            let mine = &s.mission.objects[unit];
            let cell = (0..30u16)
                .flat_map(|d| {
                    [
                        [mine.x + d, mine.y + 4],
                        [mine.x.saturating_sub(d), mine.y + 4],
                        [mine.x + d, mine.y.saturating_sub(4)],
                    ]
                })
                .find(|&p| {
                    retzar::gameplay::cheats::spawn_probe(&s.library, class, civ, owner, p)
                        .is_some_and(|o| {
                            retzar::gameplay::cheats::spawn_site(
                                sim, &s.mission, &s.grid, &s.library, &o,
                            )
                        })
                })
                .unwrap();
            (cell, s.mission.objects.len())
        };
        app.world_mut().insert_resource(MapView {
            center: Vec2::new(cell[0] as f32 * 32. + 16., -(cell[1] as f32 * 32. + 16.)),
            scale: 0.5,
            frame: Default::default(),
        });
        let middle = surface(app, window).physical / surface(app, window).dpi / 2.;
        move_to(app, window, middle);
        press(app, MouseButton::Left);
        app.update();
        release(app, MouseButton::Left);
        app.update();
        assert!(app.world().resource::<MissionUi>().spawning.is_none());
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        assert_eq!(s.mission.objects.len(), count + 1);
        s.mission.objects.last().unwrap().clone()
    };
    let peasant = place(&mut app, class::PEON);
    assert_eq!(peasant.class, class::PEON);
    let house = place(&mut app, class::HOUSE);
    assert_eq!(house.class, class::HOUSE);
    assert_eq!(house.flags & FLAG_CONSTRUCTION, 0);
    assert_eq!(house.health, house.max_health);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn regression_the_message_list_keeps_five_lines_and_lets_one_go_every_seven_and_a_half_seconds() {
    let root = test_root("message-list");
    let (mut app, _) = headless(&root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: None,
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    app.world_mut().resource_mut::<MissionUi>().messages.clear();
    let messages = |app: &App| app.world().resource::<MissionUi>().messages.clone();
    // 0x4f8a50: a sixth line pushes the oldest out.
    for n in 1..=6 {
        app.world_mut().resource_mut::<MenuState>().notice = format!("riga {n}");
        app.update();
    }
    assert_eq!(
        messages(&app),
        (2..=6).map(|n| format!("riga {n}")).collect::<Vec<_>>()
    );
    // 0x4f3e7f: 7500 ms after the list last changed the oldest goes, then
    // the next 7500 ms later.
    let wait = |app: &mut App, ms: u64| {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(ms));
        app.update();
    };
    wait(&mut app, 7000);
    assert_eq!(messages(&app).len(), 5);
    wait(&mut app, 600);
    assert_eq!(messages(&app).first().map(String::as_str), Some("riga 3"));
    wait(&mut app, 7600);
    assert_eq!(messages(&app).first().map(String::as_str), Some("riga 4"));
    // An order given says nothing; a refusal for want of resources says the
    // original's line (0x4679b0), the others nothing.
    app.world_mut().resource_mut::<MissionUi>().messages.clear();
    let (castle, train) = {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let s = loaded.scene.as_mut().unwrap();
        let player = s.mission.player();
        s.mission.kingdoms[player as usize - 1].resources = [0; 4];
        // The player's castle, or one made from a unit of theirs.
        let castle = (0..s.mission.objects.len())
            .find(|&i| {
                let o = &s.mission.objects[i];
                o.owner == player && o.class == retzar::world::class::CASTLE
            })
            .unwrap_or_else(|| {
                let mut o = s
                    .mission
                    .objects
                    .iter()
                    .find(|o| retzar::selection::own_unit(o, &s.mission))
                    .unwrap()
                    .clone();
                o.class = retzar::world::class::CASTLE;
                o.civilization = o.civilization.clamp(1, 3);
                o.id = 0xfc00;
                s.mission.objects.push(o);
                s.mission.records = s.mission.objects.len();
                s.mission.objects.len() - 1
            });
        let prefix = match s.mission.objects[castle].civilization {
            2 => "r",
            3 => "a",
            _ => "e",
        };
        (castle, format!("{prefix}_peon"))
    };
    let order = |app: &mut App, order: &str, selected: Vec<usize>| {
        app.world_mut()
            .resource_mut::<MissionUi>()
            .requests
            .push(retzar::gameplay::Request::command(order, selected));
        app.update();
    };
    order(&mut app, "nessun_ordine", vec![castle]);
    assert!(messages(&app).is_empty());
    order(&mut app, &train, vec![castle]);
    assert_eq!(messages(&app).len(), 1);
    assert!(
        messages(&app)[0].starts_with("Non c'è sufficiente"),
        "{:?}",
        messages(&app)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires installed original assets"]
fn regression_texts_over_the_map_keep_a_shadow_and_take_the_optional_styles() {
    let (_, menu, skin, library, mission) = selection_fixture();
    let state = hud::HudState::default();
    let play = hud::play_rect(800, 600, true, hud::MapFrame::Panels);
    // The bottom line of the map view, over snow-white ground.
    let line = retzar::menu::Rect {
        x: play.x,
        y: play.y + play.h - 30,
        w: play.w,
        h: 30,
    };
    // Pixels of the line at most `level` in every channel, for each style.
    let dark = |map_text: usize, level: u8| {
        let mut base = skin.background(800, 600, true);
        for y in play.y..play.y + play.h {
            for x in play.x..play.x + play.w {
                let k = (y as usize * 800 + x as usize) * 4;
                base.rgba[k..k + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let image = hud::render(
            &menu,
            &skin,
            base,
            hud::HudView {
                width: 800,
                height: 600,
                left: true,
                title: "",
                description: "",
                state: &state,
                mission: &mission,
                library: &library,
                portraits: &std::collections::HashMap::new(),
                camera: [0.; 3],
                side: 64,
                pixels: [800., 600.],
                messages: &[],
                status: "Costruisci una segheria",
                player_name: "Test",
                selection: [255, 255, 255],
                map_text,
            },
        );
        (line.y..line.y + line.h)
            .flat_map(|y| (line.x..line.x + line.w).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let k = (y as usize * 800 + x as usize) * 4;
                image.rgba[k..k + 3].iter().all(|&c| c <= level)
            })
            .count()
    };
    // 0x4f275f: the line is drawn black at +1, +1, then white.
    let shadow = dark(0, 0);
    assert!(shadow > 0, "the bottom line lost the original's shadow");
    // The options of this version: black all round the letters, or a band
    // that darkens the ground behind the whole line.
    assert!(dark(1, 0) > 2 * shadow);
    let band = dark(2, 128);
    assert!(band > 5 * dark(0, 128));
    assert_eq!(dark(2, 0), shadow, "the band keeps the original's shadow");
}
