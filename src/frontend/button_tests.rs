// Every button of the game's interface clicked with the pointer, in the
// situations that show them: something must answer each click.
use super::*;
use hud::HudAction;
use hud::cheat_panel::{CheatControl, CheatTab};
use retzar::dialogs::Dialog;

/// What a click can change: the interface, the screen, the view, the
/// selected objects' orders and work, the kingdoms' stocks, the lines.
fn observe(app: &App) -> String {
    let w = app.world();
    let ui = w.resource::<MissionUi>();
    let menu = w.resource::<MenuState>();
    let mut hud = ui.hud.clone();
    hud.hover = None;
    hud.press = None;
    let scene = w.resource::<Loaded>().scene.as_ref().unwrap();
    let sim = scene.mission.simulation.as_ref().unwrap();
    let work: Vec<String> = hud
        .selected
        .iter()
        .filter_map(|&i| sim.gameplay.actors.get(i))
        .map(|a| {
            format!(
                "{:?} {:?} {} {:?} {} {} {}",
                a.intent,
                a.queued,
                a.jobs.len(),
                a.rally,
                a.hold_fire,
                a.stance,
                a.ordered_idle
            )
        })
        .collect();
    let view = w.resource::<MapView>();
    let m = &scene.mission;
    let living = m
        .objects
        .iter()
        .filter(|o| o.flags & retzar::world::FLAG_DEAD == 0 && o.health > 0)
        .count();
    [
        format!("{hud:?}"),
        format!("{:?} {:?}", ui.spawning, ui.pointing),
        format!("{:?} {:?} {:?}", m.cheats, m.game_rules, m.diplomacy),
        format!(
            "{:?} {:?} {:?}",
            m.population_limit, m.population_base, m.population_cap
        ),
        format!("{} {living}", sim.clock_minutes()),
        format!("{:?}", ui.target_order),
        format!("{:?} {:?}", menu.screen, w.resource::<State<Mode>>().get()),
        format!("{:?} {}", view.center, view.scale),
        format!("{work:?}"),
        format!(
            "{:?}",
            m.kingdoms.iter().map(|k| k.resources).collect::<Vec<_>>()
        ),
        format!("{:?} {}", ui.messages, ui.order_feedback.is_some()),
        format!("{:?} {:?} {}", ui.answer, ui.look, ui.hush),
        format!("{} {}", sim.gameplay.events.len(), menu.notice),
    ]
    .join("|")
}

/// The buttons shown now.
fn shown(app: &App, window: Entity) -> Vec<hud::HudButton> {
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
}

/// A random map open with the cheat panel switched on: the app, its
/// window, and the player's first building, peasant and other unit.
fn game(root: &Path) -> (App, Entity, Option<usize>, Option<usize>, Option<usize>) {
    use retzar::world::class;
    let (mut app, window) = headless(root, (2560, 1440).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Random {
            side: 64,
            seed: 741,
        },
        None,
    )
    .unwrap();
    app.update();
    app.world_mut().resource_mut::<MenuState>().settings.cheats = true;
    mission::switch_cheats(app.world_mut(), true);
    app.update();
    let scene = app.world().resource::<Loaded>().scene.as_ref().unwrap();
    let m = &scene.mission;
    let player = m.player();
    let mine = |f: &dyn Fn(&retzar::world::MapObject) -> bool| {
        m.objects
            .iter()
            .position(|o| o.owner == player && o.selectable() && f(o))
    };
    let found = (
        mine(&|o| o.building()),
        mine(&|o| o.class == class::PEON),
        mine(&|o| retzar::selection::unit(o) && o.class != class::PEON),
    );
    (app, window, found.0, found.1, found.2)
}

fn place_of(app: &App, i: usize) -> [u16; 2] {
    let o = &app
        .world()
        .resource::<Loaded>()
        .scene
        .as_ref()
        .unwrap()
        .mission
        .objects[i];
    [o.x, o.y]
}

/// An object of `class` for the player, made by the cheat panel's spawn on
/// the first free place a few cells from `near`.
fn spawn(app: &mut App, near: [u16; 2], class: usize, civ: u8) -> usize {
    app.world_mut()
        .resource_scope(|world, game: Mut<Game>| {
            let mut loaded = world.resource_mut::<Loaded>();
            let scene = loaded.scene.as_mut().unwrap();
            let player = scene.mission.player();
            let mut sim = scene.mission.simulation.take().unwrap();
            let mut made = None;
            'cells: for r in 4..20u16 {
                for (dx, dy) in [(r, 0), (0, r), (r, r), (0, 0)] {
                    let at = [near[0] + dx, near[1] + dy];
                    let request = retzar::gameplay::Request {
                        order: "cheat".into(),
                        cell: Some(at),
                        cheat: Some(retzar::gameplay::cheats::Cheat::Spawn {
                            class,
                            civ,
                            owner: player,
                        }),
                        ..Default::default()
                    };
                    if retzar::gameplay::issue(
                        &mut sim,
                        &mut scene.mission,
                        &scene.grid,
                        &scene.library,
                        &game.gameplay,
                        &request,
                    )
                    .is_ok()
                    {
                        made = scene
                            .mission
                            .objects
                            .iter()
                            .rposition(|o| o.class == class && o.owner == player);
                        break 'cells;
                    }
                }
            }
            scene.mission.simulation = Some(sim);
            made
        })
        .unwrap_or_else(|| panic!("{class} non creato"))
}

/// What is selected, the panel open, the cheats' page, the command
/// dialog, by a name.
type Situation = (
    String,
    Vec<usize>,
    Option<Panel>,
    Option<CheatTab>,
    Option<Dialog>,
);

/// Every button shown in each situation clicked, the situation made again
/// before each click: how many were clicked, and those that answered with
/// nothing.
fn sweep(app: &mut App, window: Entity, situations: Vec<Situation>) -> (usize, Vec<String>) {
    let mut silent = Vec::new();
    let mut clicked = 0;
    for (name, selected, panel, tab, dialog) in situations {
        // The situation, made once and given back before each click.
        {
            let world = app.world_mut();
            world.resource_scope(|world, game: Mut<Game>| {
                world.resource_scope(|world, loaded: Mut<Loaded>| {
                    let mut ui = world.resource_mut::<MissionUi>();
                    ui.hud.selected = selected.clone();
                    ui.paged = selected.clone();
                    ui.hud.command_menu = None;
                    ui.hud.command_history.clear();
                    ui.hud.contents_owner = None;
                    ui.hud.page = 0;
                    ui.hud.set_panel(None);
                    if panel == Some(Panel::Cheats) {
                        mission::open_cheats(&mut ui, loaded.scene.as_ref().unwrap(), &game);
                        ui.hud.cheat.tab = tab.unwrap_or_default();
                    } else if panel.is_some() {
                        ui.hud.set_panel(panel);
                    }
                    ui.hud.dialog = dialog.clone();
                    if name == "inventario" {
                        ui.hud.contents_owner = selected.first().copied();
                    }
                });
            });
        }
        app.update();
        app.update();
        let base = app.world().resource::<MissionUi>().hud.clone();
        let world_then = app
            .world()
            .resource::<Loaded>()
            .scene
            .as_ref()
            .unwrap()
            .mission
            .clone();
        let set = |app: &mut App| {
            app.world_mut()
                .resource_mut::<Loaded>()
                .scene
                .as_mut()
                .unwrap()
                .mission = world_then.clone();
            app.world_mut()
                .resource_mut::<NextState<Mode>>()
                .set(Mode::Mission);
            let mut ui = app.world_mut().resource_mut::<MissionUi>();
            ui.hud = base.clone();
            ui.paged = base.selected.clone();
            ui.target_order = None;
            ui.spawning = None;
            ui.pointing = None;
            ui.messages.clear();
            ui.order_feedback = None;
            ui.answer = None;
            ui.look = None;
            app.update();
            app.update();
        };
        let buttons = shown(app, window);
        let mut done = std::collections::BTreeSet::new();
        for b in buttons {
            if b.action == HudAction::Inert || !done.insert(format!("{:?}", b.action)) {
                continue;
            }
            set(app);
            // Only what the original lets press: a grey command only tells
            // what it needs.
            let order = match b.action {
                HudAction::Command(i) => {
                    let w = app.world();
                    let game = w.resource::<Game>();
                    let scene = w.resource::<Loaded>().scene.as_ref().unwrap();
                    let list = hud::commands(
                        &game.classic,
                        &game.skin.locale,
                        &w.resource::<MissionUi>().hud,
                        &scene.mission,
                        &scene.library,
                    );
                    match list.get(i) {
                        Some(c) if c.unavailable.is_none() => Some(c.order.clone()),
                        _ => continue,
                    }
                }
                // A control the dialog draws disabled (state 3) only shows.
                HudAction::Dialog(control) => {
                    let w = app.world();
                    let m = &w.resource::<Loaded>().scene.as_ref().unwrap().mission;
                    let d = w.resource::<MissionUi>().hud.dialog.clone().unwrap();
                    let stock = retzar::gameplay::stock(m, m.player());
                    if !d.enabled(control, stock) {
                        continue;
                    }
                    None
                }
                _ => None,
            };
            let s = surface(app, window);
            let settings = app.world().resource::<MenuState>().settings.clone();
            let h = hud_layout(s, &settings);
            let centre = Vec2::new(
                b.rect.x as f32 + b.rect.w as f32 / 2.,
                b.rect.y as f32 + b.rect.h as f32 / 2.,
            );
            // The first button under that point takes the click.
            let first = shown(app, window)
                .into_iter()
                .find(|o| o.rect.has(centre.x, centre.y))
                .map(|o| o.action);
            if first != Some(b.action) {
                silent.push(format!(
                    "{name}: {:?} «{}» coperto da {first:?}",
                    b.action, b.label
                ));
                continue;
            }
            move_to(app, window, centre * h.factor / s.dpi);
            app.update();
            let before = observe(app);
            press(app, MouseButton::Left);
            app.update();
            release(app, MouseButton::Left);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .clear();
            app.update();
            clicked += 1;
            if observe(app) != before {
                continue;
            }
            // What is already so stays so: the choice made, the page or the
            // window open, a list with nothing more to show.
            let hud = &app.world().resource::<MissionUi>().hud;
            let already = b.active
                || match b.action {
                    HudAction::Cheat(CheatControl::Tab(t)) => hud.cheat.tab == t,
                    HudAction::OpenCheats => hud.panel == Some(Panel::Cheats),
                    HudAction::Page(_) => hud.panel == Some(Panel::Quests) && hud.scroll_max == 0,
                    _ => {
                        order.as_deref() == Some("view_quests") && hud.panel == Some(Panel::Quests)
                    }
                };
            if !already {
                silent.push(format!("{name}: {:?} «{}»", b.action, b.label));
            }
        }
    }
    (clicked, silent)
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn every_button_of_the_game_interface_answers_its_click() {
    use retzar::world::class;
    let root = test_root("every-button");
    let (mut app, window, castle, peon, soldier) = game(&root);
    let near = place_of(&app, castle.unwrap());
    let spawn = |app: &mut App, class: usize, civ: u8| spawn(app, near, class, civ);
    let tavern = spawn(&mut app, class::TAVERN, 1);
    let trader = spawn(&mut app, 2, 1);
    let workshop = spawn(&mut app, 69, 2);
    let galleon = spawn(&mut app, 23, 1);
    app.update();
    // Each situation: what is selected, the panel open, the cheats' page,
    // the command dialog.
    let mut situations: Vec<Situation> = vec![("niente".into(), vec![], None, None, None)];
    for (name, object) in [
        ("edificio", castle),
        ("contadino", peon),
        ("soldato", soldier),
        ("taverna", Some(tavern)),
        ("mercante", Some(trader)),
        ("laboratorio", Some(workshop)),
        ("galeone", Some(galleon)),
    ] {
        if let Some(i) = object {
            situations.push((name.into(), vec![i], None, None, None));
        }
    }
    for (name, i, dialog) in [
        ("dadi", tavern, Dialog::gamble(tavern)),
        ("cambio", tavern, Dialog::exchange(tavern, None)),
        (
            "rotta",
            trader,
            Dialog::route(trader, [None; 2], [0, 0], None),
        ),
        (
            "rotta per mare",
            galleon,
            Dialog::route(galleon, [None; 2], [0, 0], None),
        ),
        (
            "oggetti magici",
            workshop,
            Dialog::workshop(workshop, (0, 0)),
        ),
    ] {
        situations.push((name.into(), vec![i], None, None, Some(dialog)));
    }
    for panel in [
        Panel::Quests,
        Panel::Diplomacy,
        Panel::Tribute,
        Panel::Chat(false),
        Panel::Chat(true),
    ] {
        situations.push((format!("{panel:?}"), vec![], Some(panel), None, None));
    }
    for (tab, _, _) in hud::cheat_panel::TABS {
        situations.push((
            format!("trucchi {tab:?}"),
            vec![],
            Some(Panel::Cheats),
            Some(tab),
            None,
        ));
    }
    // Stocks to give, so that the tributes have something to move.
    {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let m = &mut loaded.scene.as_mut().unwrap().mission;
        let player = m.player() as usize;
        m.kingdoms[player - 1].resources = [5000; 4];
    }
    let (clicked, silent) = sweep(&mut app, window, situations);
    assert!(clicked > 200, "{clicked}");
    let _ = std::fs::remove_dir_all(root);
    assert!(silent.is_empty(), "{}", silent.join("\n"));
}

/// The physical key of a default binding that is not a character.
fn key_of(name: &str) -> KeyCode {
    match name {
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "Space" => KeyCode::Space,
        "Tab" => KeyCode::Tab,
        "Home" => KeyCode::Home,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "End" => KeyCode::End,
        "Delete" => KeyCode::Delete,
        "NumpadAdd" => KeyCode::NumpadAdd,
        "NumpadSubtract" => KeyCode::NumpadSubtract,
        "Enter" => KeyCode::Enter,
        "F1" => KeyCode::F1,
        "F2" => KeyCode::F2,
        "F3" => KeyCode::F3,
        "F4" => KeyCode::F4,
        "F5" => KeyCode::F5,
        "F6" => KeyCode::F6,
        "F7" => KeyCode::F7,
        "F8" => KeyCode::F8,
        "F9" => KeyCode::F9,
        "F10" => KeyCode::F10,
        _ => panic!("tasto {name}"),
    }
}

/// The health bars over the map (z 421 and 421.5).
fn bars(app: &mut App) -> usize {
    let mut q = app.world_mut().query::<&Transform>();
    q.iter(app.world())
        .filter(|t| t.translation.z == 421. || t.translation.z == 421.5)
        .count()
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn every_bound_key_does_its_work() {
    use retzar::world::class;
    let root = test_root("every-key");
    let (mut app, _, castle, peon, _) = game(&root);
    let (castle, peon) = (castle.unwrap(), peon.unwrap());
    let near = place_of(&app, castle);
    // Two buildings done: two events for the event keys.
    spawn(&mut app, near, class::HOUSE, 1);
    spawn(&mut app, near, class::HOUSE, 1);
    app.update();
    let world_then = app
        .world()
        .resource::<Loaded>()
        .scene
        .as_ref()
        .unwrap()
        .mission
        .clone();
    let screen_then = app.world().resource::<MenuState>().screen;
    let middle = Vec2::new(32. * 32., -32. * 32.);
    let far = Vec2::new(4. * 32., -60. * 32.);
    let mut silent = Vec::new();
    for (action, _) in retzar::keys::ACTIONS {
        let (selected, view) = match action {
            "center" | "delete" => (vec![peon], far),
            "next_in_group" => (vec![castle, peon], middle),
            "destination" => (vec![castle], far),
            "inventory_previous" | "inventory_next" => (vec![peon], middle),
            "event_last" | "event_previous" | "event_next" => (vec![], far),
            _ => (vec![], middle),
        };
        app.world_mut()
            .resource_mut::<NextState<Mode>>()
            .set(Mode::Mission);
        app.world_mut().resource_mut::<MenuState>().screen = screen_then;
        {
            let mut loaded = app.world_mut().resource_mut::<Loaded>();
            let mission = &mut loaded.scene.as_mut().unwrap().mission;
            *mission = world_then.clone();
            if action == "destination" {
                let sim = mission.simulation.as_mut().unwrap();
                sim.gameplay.actors[castle].rally = Some([near[0] + 12, near[1] + 12]);
            }
        }
        {
            let mut ui = app.world_mut().resource_mut::<MissionUi>();
            ui.hud = hud::HudState {
                selected: selected.clone(),
                cheats: true,
                ..Default::default()
            };
            ui.paged = selected.clone();
            ui.target_order = None;
            ui.transparent_trees = false;
            ui.event_cursor = 0;
            if action.starts_with("inventory_") {
                ui.hud.contents_owner = Some(peon);
                ui.hud.page = usize::from(action == "inventory_previous");
            }
        }
        app.world_mut().resource_mut::<MapView>().center = view;
        app.update();
        app.update();
        let extra = |app: &mut App| {
            let bars = bars(app);
            let w = app.world();
            let ui = w.resource::<MissionUi>();
            let speed = w
                .resource::<Loaded>()
                .scene
                .as_ref()
                .and_then(|s| s.mission.simulation.as_ref())
                .map(|s| s.speed);
            format!(
                "{} {} {speed:?} {bars} {}",
                ui.transparent_trees,
                ui.event_cursor,
                w.contains_resource::<menu_ui::LoadJob>()
            )
        };
        let before = format!("{}|{}", observe(&app), extra(&mut app));
        let name = app
            .world()
            .resource::<MenuState>()
            .current_settings()
            .key(action);
        let quick = |root: &Path| {
            root.join("saves")
                .join(format!("{}.rtzs", retzar::menu::QUICK_SAVE))
        };
        match name.strip_prefix("char:") {
            // The health bars show while their key is held.
            Some(c) if action == "health_bars" => {
                app.world_mut()
                    .resource_mut::<input::TypedKeys>()
                    .hold(KeyCode::Backquote, c);
                app.update();
                let held = bars(&mut app);
                app.world_mut()
                    .resource_mut::<input::TypedKeys>()
                    .release(KeyCode::Backquote);
                app.update();
                if held == 0 {
                    silent.push(format!("{action} ({name})"));
                }
                continue;
            }
            Some(c) => {
                let event = |state| KeyboardInput {
                    key_code: KeyCode::Comma,
                    logical_key: Key::Character(c.into()),
                    state,
                    text: Some(c.into()),
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                };
                app.world_mut().write_message(event(ButtonState::Pressed));
                app.update();
                app.world_mut().write_message(event(ButtonState::Released));
                app.update();
            }
            // The view scrolls while an arrow is held.
            None if action.starts_with("scroll_") => {
                let key = key_of(&name);
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(key);
                // The view moves by the time gone by.
                for _ in 0..5 {
                    app.world_mut()
                        .resource_mut::<Time>()
                        .advance_by(std::time::Duration::from_millis(50));
                    app.update();
                }
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.release(key);
                keys.clear();
            }
            None => tap(&mut app, key_of(&name)),
        }
        app.update();
        // The quick save is written aside, the quick load reads it back.
        if action == "quick_save" {
            for _ in 0..2000 {
                if quick(&root).exists() && !app.world().contains_resource::<menu_ui::SaveJob>() {
                    break;
                }
                app.update();
            }
            if !quick(&root).exists() {
                silent.push(format!("{action} ({name})"));
            }
            continue;
        }
        if action == "quick_load" {
            if !app.world().contains_resource::<menu_ui::LoadJob>() {
                silent.push(format!("{action} ({name})"));
            }
            wait_for_scene(&mut app);
            continue;
        }
        if format!("{}|{}", observe(&app), extra(&mut app)) == before {
            silent.push(format!("{action} ({name})"));
        }
    }
    let _ = std::fs::remove_dir_all(root);
    assert!(silent.is_empty(), "{}", silent.join("\n"));
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn every_button_of_the_inventory_answers_its_click() {
    let root = test_root("every-item");
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
    // A peasant carrying two items of the map, its inventory open.
    let peon = {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let m = &mut loaded.scene.as_mut().unwrap().mission;
        let player = m.player();
        let peon = m
            .objects
            .iter()
            .position(|o| o.owner == player && o.class == retzar::world::class::PEON)
            .unwrap();
        let items: Vec<usize> = (0..m.objects.len())
            .filter(|&j| {
                m.objects[j].item()
                    && m.objects[j].flags & (retzar::world::FLAG_HELD | retzar::world::FLAG_DEAD)
                        == 0
            })
            .take(2)
            .collect();
        assert_eq!(items.len(), 2);
        for &j in &items {
            m.objects[j].flags |= retzar::world::FLAG_HELD;
        }
        let sim = m.simulation.as_mut().unwrap();
        for &j in &items {
            sim.objects[j].holder = Some(peon);
        }
        sim.objects[peon].contents = items;
        peon
    };
    let (clicked, silent) = sweep(
        &mut app,
        window,
        vec![("inventario".into(), vec![peon], None, None, None)],
    );
    let _ = std::fs::remove_dir_all(root);
    assert!(clicked > 2, "{clicked}");
    assert!(silent.is_empty(), "{}", silent.join("\n"));
}

/// The interface as the window shows it now.
fn picture(app: &App, window: Entity) -> retzar::bitmap::Bitmap {
    let w = app.world();
    let menu = w.resource::<MenuState>();
    let game = w.resource::<Game>();
    let s = w.resource::<Loaded>().scene.as_ref().unwrap();
    let ui = w.resource::<MissionUi>();
    let h = hud_layout(surface(app, window), &menu.settings);
    let (width, height) = (h.size.x as i32, h.size.y as i32);
    let left = menu.settings.layout == 1;
    hud::render(
        &game.skin,
        &game.classic,
        game.classic.background(width, height, left),
        hud::HudView {
            width,
            height,
            left,
            title: &s.title,
            description: &s.description,
            state: &ui.hud,
            mission: &s.mission,
            library: &s.library,
            camera: [0.; 3],
            side: s.side,
            pixels: [width as f32, height as f32],
            portraits: &s.portraits,
            messages: &[],
            status: "",
            player_name: "Prova",
            selection: s.selection_color,
            map_text: 0,
        },
    )
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn the_help_ball_of_the_windows_shows_their_hints_and_holds_them() {
    let root = test_root("help-ball-test");
    let (mut app, window, ..) = game(&root);
    let cream = |b: &retzar::bitmap::Bitmap, y0: i32, y1: i32| {
        (y0.max(0)..y1.min(b.height as i32))
            .flat_map(|y| (0..b.width as i32).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let k = (y as usize * b.width as usize + x as usize) * 4;
                b.rgba[k..k + 3] == [0xff, 0xf3, 0xd1]
            })
            .count()
    };
    let click = |app: &mut App, button: MouseButton| {
        press(app, button);
        app.update();
        release(app, button);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.update();
    };
    for panel in [
        Panel::Diplomacy,
        Panel::Tribute,
        Panel::Chat(false),
        Panel::Chat(true),
    ] {
        app.world_mut()
            .resource_mut::<MissionUi>()
            .hud
            .set_panel(Some(panel));
        app.update();
        let s = surface(&app, window);
        let settings = app.world().resource::<MenuState>().settings.clone();
        let h = hud_layout(s, &settings);
        let to = |app: &mut App, r: retzar::menu::Rect| {
            let centre = Vec2::new(r.x as f32 + r.w as f32 / 2., r.y as f32 + r.h as f32 / 2.);
            move_to(app, window, centre * h.factor / s.dpi);
            app.update();
            [centre.x as i32, centre.y as i32]
        };
        let layout = hud::Layout {
            width: h.size.x as i32,
            height: h.size.y as i32,
            left: settings.layout == 1,
        };
        let w = layout.window(panel);
        let buttons = shown(&app, window);
        // At the window's top-left corner, made after its controls.
        let ball = buttons
            .iter()
            .find(|b| b.action == HudAction::Help)
            .unwrap_or_else(|| panic!("{panel:?}: nessuna palla"))
            .rect;
        assert_eq!([ball.x, ball.y, ball.w, ball.h], [w.x, w.y, 50, 43]);
        // The last control with a hint that does something.
        let control = buttons
            .iter()
            .rev()
            .find(|b| {
                !b.label.is_empty()
                    && !matches!(b.action, HudAction::Inert | HudAction::Help)
                    && b.rect.y > ball.y + ball.h
            })
            .unwrap()
            .rect;
        // Without the mode its hint shows nowhere: not on the map's line.
        let bottom = hud::play_rect(layout.width, layout.height, layout.left, hud::MapFrame::Panels);
        let line = |b: &retzar::bitmap::Bitmap| {
            let y = bottom.y + bottom.h - 62;
            b.crop(bottom.x as u32, y as u32, bottom.w as u32, 62)
                .unwrap()
                .rgba
        };
        to(
            &mut app,
            retzar::menu::Rect {
                x: w.x + w.w - 12,
                y: w.y + w.h - 12,
                w: 4,
                h: 4,
            },
        );
        let calm = picture(&app, window);
        let at = to(&mut app, control);
        let hovered = picture(&app, window);
        assert_eq!(
            line(&calm),
            line(&hovered),
            "{panel:?}: suggerimento sulla riga"
        );
        assert_eq!(cream(&hovered, at[1] + 20, at[1] + 36), 0);
        // The ball starts the mode and turns red.
        to(&mut app, ball);
        let green = picture(&app, window)
            .crop(ball.x as u32, ball.y as u32, 50, 43)
            .unwrap();
        click(&mut app, MouseButton::Left);
        assert!(app.world().resource::<MissionUi>().hud.help.is_some());
        let red = picture(&app, window)
            .crop(ball.x as u32, ball.y as u32, 50, 43)
            .unwrap();
        assert_ne!(green.rgba, red.rgba, "{panel:?}: la palla non cambia");
        // The hint of the control under the pointer, in a box beneath it.
        let at = to(&mut app, control);
        let helping = picture(&app, window);
        assert!(
            cream(&helping, at[1] + 20, at[1] + 36) > 100,
            "{panel:?}: nessun riquadro"
        );
        // Clicks and keys do nothing; the right button ends the mode.
        let before = observe(&app);
        click(&mut app, MouseButton::Left);
        tap(&mut app, KeyCode::Enter);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key: Key::Character("a".into()),
            state: ButtonState::Pressed,
            text: Some("a".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        tap(&mut app, KeyCode::KeyA);
        app.update();
        assert_eq!(observe(&app), before, "{panel:?}: un controllo risponde");
        click(&mut app, MouseButton::Right);
        assert!(app.world().resource::<MissionUi>().hud.help.is_none());
        assert_eq!(
            app.world().resource::<MissionUi>().hud.panel,
            Some(panel),
            "{panel:?}: il tasto destro chiude la finestra"
        );
        // Escape ends it too, and the left button on the ball.
        to(&mut app, ball);
        click(&mut app, MouseButton::Left);
        tap(&mut app, KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<MissionUi>().hud.help.is_none());
        assert_eq!(app.world().resource::<MissionUi>().hud.panel, Some(panel));
        click(&mut app, MouseButton::Left);
        assert!(app.world().resource::<MissionUi>().hud.help.is_some());
        click(&mut app, MouseButton::Left);
        assert!(app.world().resource::<MissionUi>().hud.help.is_none());
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn the_game_windows_follow_the_pointer_held_on_their_ground() {
    let root = test_root("drag-windows");
    let (mut app, window, ..) = game(&root);
    let s = surface(&app, window);
    let settings = app.world().resource::<MenuState>().settings.clone();
    let h = hud_layout(s, &settings);
    let layout = hud::Layout {
        width: h.size.x as i32,
        height: h.size.y as i32,
        left: settings.layout == 1,
    };
    let at = |app: &mut App, p: [i32; 2]| {
        let p = Vec2::new(p[0] as f32 + 0.5, p[1] as f32 + 0.5);
        move_to(app, window, p * h.factor / s.dpi);
        app.update();
    };
    // Held at `from`, let go at `to`.
    let drag = |app: &mut App, from: [i32; 2], to: [i32; 2]| {
        at(app, from);
        press(app, MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        at(app, to);
        release(app, MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.update();
    };
    let open = |app: &mut App, panel: Panel| {
        app.world_mut()
            .resource_mut::<MissionUi>()
            .hud
            .set_panel(Some(panel));
        app.update();
    };
    let placed = |app: &App, panel: Panel| {
        app.world()
            .resource::<MissionUi>()
            .hud
            .window(layout, panel)
    };
    let ground = |w: retzar::menu::Rect| [w.x + w.w - 12, w.y + w.h - 12];
    open(&mut app, Panel::Diplomacy);
    let w0 = placed(&app, Panel::Diplomacy);
    assert_eq!(w0, layout.window(Panel::Diplomacy));
    // Held on its ground, it goes with the pointer, its controls with it.
    let g = ground(w0);
    drag(&mut app, g, [g[0] - 100, g[1] - 40]);
    let w1 = placed(&app, Panel::Diplomacy);
    assert_eq!([w1.x, w1.y], [w0.x - 100, w0.y - 40]);
    let accept = shown(&app, window)
        .into_iter()
        .find(|b| b.action == HudAction::Accept)
        .unwrap()
        .rect;
    assert_eq!([accept.x, accept.y], [w1.x + 38, w1.y + 354]);
    // Held on a control it stays.
    let attitude = shown(&app, window)
        .into_iter()
        .find(|b| matches!(b.action, HudAction::Relation(..)))
        .unwrap()
        .rect;
    drag(
        &mut app,
        [attitude.x + 5, attitude.y + 5],
        [attitude.x + 200, attitude.y + 100],
    );
    assert_eq!(placed(&app, Panel::Diplomacy), w1);
    // Fifty pixels of it stay on the screen.
    drag(&mut app, ground(w1), [0, 0]);
    let w = placed(&app, Panel::Diplomacy);
    assert_eq!([w.x, w.y], [50 - w.w, 50 - w.h]);
    drag(&mut app, ground(w), ground(w1));
    assert_eq!(placed(&app, Panel::Diplomacy), w1);
    // Below the help ball, on the window's own ground.
    drag(
        &mut app,
        [w1.x + 20, w1.y + 46],
        [layout.width - 1, layout.height - 1],
    );
    let w2 = placed(&app, Panel::Diplomacy);
    assert_eq!([w2.x, w2.y], [layout.width - 50, layout.height - 50]);
    drag(
        &mut app,
        [w2.x + 20, w2.y + 46],
        [w0.x + 20 - 60, w0.y + 46],
    );
    let w2 = placed(&app, Panel::Diplomacy);
    assert_eq!([w2.x, w2.y], [w0.x - 60, w0.y]);
    // The tributes open centred over it; back from them it is where it was.
    open(&mut app, Panel::Tribute);
    let t0 = placed(&app, Panel::Tribute);
    assert_eq!(t0, layout.window(Panel::Tribute));
    drag(
        &mut app,
        ground(t0),
        [ground(t0)[0] + 30, ground(t0)[1] + 30],
    );
    assert_eq!(placed(&app, Panel::Tribute).x, t0.x + 30);
    assert_eq!(placed(&app, Panel::Diplomacy), w2);
    app.world_mut()
        .resource_mut::<MissionUi>()
        .hud
        .close_panel();
    app.update();
    assert_eq!(placed(&app, Panel::Diplomacy), w2);
    // Closed and opened again, it is centred.
    app.world_mut()
        .resource_mut::<MissionUi>()
        .hud
        .set_panel(None);
    open(&mut app, Panel::Diplomacy);
    assert_eq!(placed(&app, Panel::Diplomacy), w0);
    // The objectives and the message window move too, the chat does not.
    for panel in [Panel::Quests, Panel::Story] {
        open(&mut app, panel);
        let w = placed(&app, panel);
        drag(&mut app, [w.x + 4, w.y + 4], [w.x + 54, w.y + 4]);
        assert_eq!(placed(&app, panel).x, w.x + 50, "{panel:?}");
    }
    open(&mut app, Panel::Chat(false));
    let w = placed(&app, Panel::Chat(false));
    drag(&mut app, [w.x + 480, w.y + 130], [w.x + 530, w.y + 130]);
    assert_eq!(placed(&app, Panel::Chat(false)), w);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn the_game_menu_follows_the_pointer_and_keeps_its_place_under_its_windows() {
    use retzar::menu::{Action, MESSAGE_AT, PAUSE_AT};
    let root = test_root("drag-menu");
    let (mut app, window, ..) = game(&root);
    tap(&mut app, KeyCode::F10);
    app.update();
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Pause);
    let rect = |app: &App| {
        let w = app.world();
        w.resource::<Game>()
            .skin
            .drag_rect(w.resource::<MenuState>())
            .unwrap()
    };
    let drag = |app: &mut App, from: (i32, i32), by: (i32, i32)| {
        let p = Vec2::new(from.0 as f32 + 0.5, from.1 as f32 + 0.5);
        let c = menu_cursor(app, window, p);
        move_to(app, window, c);
        app.update();
        press(app, MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        let c = menu_cursor(app, window, p + Vec2::new(by.0 as f32, by.1 as f32));
        move_to(app, window, c);
        app.update();
        release(app, MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.update();
    };
    let d = rect(&app);
    assert_eq!((d.x, d.y), PAUSE_AT);
    drag(&mut app, (d.x + 30, d.y + d.h / 2), (60, 30));
    let moved = (PAUSE_AT.0 + 60, PAUSE_AT.1 + 30);
    {
        let menu = app.world().resource::<MenuState>();
        assert_eq!(menu.dialog_place(), moved);
        // Over the game the canvas, clear around the dialog, goes with it,
        // so that the dialog may go anywhere on the screen.
        assert_eq!(menu.canvas_shift(), (60, 30));
        assert_eq!(menu.dialog_at(), PAUSE_AT);
    }
    // The instructions open centred over it; back from them it is still there.
    app.world_mut().resource_mut::<PendingAction>().0 = Some(Action::Open(Screen::Help));
    app.update();
    let help = rect(&app);
    assert_eq!((help.x, help.y), MESSAGE_AT);
    drag(&mut app, (help.x + 20, help.y + 20), (-40, 0));
    assert_eq!(
        app.world().resource::<MenuState>().dialog_place(),
        (help.x - 40, help.y)
    );
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Pause);
    assert_eq!(app.world().resource::<MenuState>().dialog_place(), moved);
    // Its buttons answer where it lies now: «Torna al gioco».
    let back = app
        .world()
        .resource::<Game>()
        .skin
        .widgets(app.world().resource::<MenuState>())
        .into_iter()
        .find(|w| w.action == Action::Resume)
        .unwrap()
        .rect;
    let centre = Vec2::new(
        back.x as f32 + back.w as f32 / 2.,
        back.y as f32 + back.h as f32 / 2.,
    );
    let c = menu_cursor(&app, window, centre);
    move_to(&mut app, window, c);
    app.update();
    press(&mut app, MouseButton::Left);
    app.update();
    release(&mut app, MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.update();
    assert_eq!(*app.world().resource::<State<Mode>>().get(), Mode::Mission);
    // In the menu again: centred.
    tap(&mut app, KeyCode::F10);
    app.update();
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Pause);
    assert_eq!(app.world().resource::<MenuState>().dialog_place(), PAUSE_AT);
    assert_eq!(app.world().resource::<MenuState>().canvas_shift(), (0, 0));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn the_leave_window_of_the_main_menu_moves_within_its_page() {
    use retzar::menu::{Action, MESSAGE_AT};
    let root = test_root("drag-quit");
    let (mut app, window) = headless(&root, (1280, 720).into());
    app.world_mut()
        .resource_mut::<MenuState>()
        .open(Screen::Quit);
    app.update();
    let d = {
        let w = app.world();
        w.resource::<Game>()
            .skin
            .drag_rect(w.resource::<MenuState>())
            .unwrap()
    };
    assert_eq!((d.x, d.y), MESSAGE_AT);
    let from = Vec2::new(d.x as f32 + 20.5, d.y as f32 + 20.5);
    let c = menu_cursor(&app, window, from);
    move_to(&mut app, window, c);
    app.update();
    press(&mut app, MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    // Far to the right: fifty pixels of it stay on the 800×600 page.
    let c = menu_cursor(&app, window, Vec2::new(799.5, from.y));
    move_to(&mut app, window, c);
    app.update();
    release(&mut app, MouseButton::Left);
    app.update();
    let menu = app.world().resource::<MenuState>();
    assert_eq!(menu.dialog_at(), (750, MESSAGE_AT.1));
    assert_eq!(menu.canvas_shift(), (0, 0));
    // Its buttons go with it.
    let no = app
        .world()
        .resource::<Game>()
        .skin
        .widgets(menu)
        .into_iter()
        .find(|w| w.action == Action::Back)
        .unwrap()
        .rect;
    assert_eq!((no.x, no.y), (750 + 261, MESSAGE_AT.1 + 258));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn f12_lays_the_map_over_the_screen_and_alt_enter_switches_the_window() {
    let root = test_root("f12-test");
    let (mut app, window, ..) = game(&root);
    let s = surface(&app, window);
    let settings = app.world().resource::<MenuState>().settings.clone();
    let h = hud_layout(s, &settings);
    let layout = hud::Layout {
        width: h.size.x as i32,
        height: h.size.y as i32,
        left: settings.layout == 1,
    };
    // A point of the side panel, under the minimap.
    let side = [
        if layout.left { 75 } else { layout.width - 75 },
        layout.height / 2,
    ];
    let alpha = |app: &App| {
        let b = picture(app, window);
        b.rgba[(side[1] as usize * b.width as usize + side[0] as usize) * 4 + 3]
    };
    let on_map = |app: &App| {
        !hud::blocks_map(
            &app.world().resource::<MissionUi>().hud,
            [side[0] as f32, side[1] as f32],
            layout,
        )
    };
    let frame = |app: &App| app.world().resource::<MapView>().frame;
    assert_eq!(alpha(&app), 255);
    assert!(!on_map(&app));
    assert!(!shown(&app, window).is_empty());
    // F12 (0x4f61ce): the panels go, the map takes the whole screen.
    tap(&mut app, KeyCode::F12);
    app.update();
    assert!(app.world().resource::<MissionUi>().hud.full_map);
    assert_eq!(frame(&app), hud::MapFrame::Whole);
    assert_eq!(alpha(&app), 0, "il pannello resta");
    assert!(on_map(&app), "la mappa non prende il posto del pannello");
    assert!(shown(&app, window).is_empty(), "i pulsanti dei pannelli restano");
    tap(&mut app, KeyCode::F12);
    app.update();
    assert_eq!(frame(&app), hud::MapFrame::Panels);
    assert_eq!(alpha(&app), 255);
    // In the film framing F12 does nothing, and the film's end brings the
    // panels back (0x4f9ade).
    tap(&mut app, KeyCode::F12);
    app.update();
    let unit = {
        let loaded = app.world().resource::<Loaded>();
        let s = loaded.scene.as_ref().unwrap();
        (0..s.mission.objects.len())
            .find(|&i| retzar::selection::own_unit(&s.mission.objects[i], &s.mission))
            .unwrap()
    };
    app.world_mut().resource_mut::<MissionUi>().view_lock = Some(mission::ViewLock {
        follow: Some(unit),
        until: None,
    });
    app.update();
    assert_eq!(frame(&app), hud::MapFrame::Film);
    tap(&mut app, KeyCode::F12);
    app.update();
    assert_eq!(frame(&app), hud::MapFrame::Film);
    app.world_mut().resource_mut::<MissionUi>().view_lock = None;
    app.update();
    assert!(!app.world().resource::<MissionUi>().hud.full_map);
    assert_eq!(frame(&app), hud::MapFrame::Panels);
    // Alt+Enter switches the window to full screen and back, and the game
    // does not take that Enter.
    let alt_enter = |app: &mut App| {
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.press(KeyCode::AltLeft);
            keys.press(KeyCode::Enter);
        }
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::Enter);
        keys.release(KeyCode::AltLeft);
        keys.clear();
        app.update();
    };
    let full = |app: &App| app.world().get::<Window>(window).unwrap().mode;
    let before = app.world().resource::<MenuState>().settings.fullscreen;
    alt_enter(&mut app);
    assert_eq!(
        app.world().resource::<MenuState>().settings.fullscreen,
        !before
    );
    assert_ne!(
        matches!(full(&app), bevy::window::WindowMode::Windowed),
        !before
    );
    assert!(app.world().resource::<MissionUi>().hud.panel.is_none(), "Invio apre la chat");
    alt_enter(&mut app);
    assert_eq!(
        app.world().resource::<MenuState>().settings.fullscreen,
        before
    );
    // Enter alone is the game's again.
    tap(&mut app, KeyCode::Enter);
    app.update();
    assert!(matches!(
        app.world().resource::<MissionUi>().hud.panel,
        Some(Panel::Chat(_))
    ));
    let _ = std::fs::remove_dir_all(root);
}
