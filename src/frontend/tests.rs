//! Front-end tests without a window or renderer: the plugin runs on a plain
//! ECS world with a primary window entity and manual input.
use super::*;
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::{AccumulatedMouseScroll, MouseButtonInput},
    },
    state::app::StatesPlugin,
    window::{CursorMoved, WindowEvent, WindowResolution},
};
use retzar::{
    hud::Panel,
    menu::{OptionKey, Selection},
};
use std::path::Path;

/// The mission as saved, apart from what moves with time alone in the
/// original: fish patches drift by themselves (0x450ba0), cows and bears
/// run their own programs (0x42f450, 0x42fea0).
fn still_world(m: &retzar::world::MissionWorld) -> Vec<u8> {
    let mut m = m.clone();
    for o in &mut m.objects {
        if drifts(o.class) {
            o.x = 0;
            o.y = 0;
            if o.data.len() >= 12 {
                o.data[8..12].fill(0);
            }
        }
    }
    serde_json::to_vec(&m).unwrap()
}
fn drifts(class: usize) -> bool {
    use retzar::world::class;
    matches!(class, class::FISHPATCH | class::COW | 26)
}

#[test]
fn frame_limit_keeps_whole_periods_and_can_be_disabled() {
    let start = Instant::now();
    let period = Duration::from_secs_f64(1. / 60.);
    // Work took 5 ms: wait the rest of the slot.
    let (wait, next) = frame_wait(start, start + Duration::from_millis(5), 60);
    assert_eq!(next, start + period);
    assert_eq!(wait, period - Duration::from_millis(5));
    // Oversleeping by 0.4 ms does not shift the following deadline.
    let (_, after) = frame_wait(next, next + Duration::from_micros(400), 60);
    assert_eq!(after, next + period);
    // A missed slot restarts from now instead of rushing to catch up.
    let late = start + Duration::from_millis(40);
    assert_eq!(frame_wait(start, late, 30), (Duration::ZERO, late));
    assert_eq!(frame_wait(start, late, 0), (Duration::ZERO, late));
}

#[test]
fn video_choices_apply_to_the_window_data_only() {
    let mut w = Window::default();
    let s = Settings {
        window: [2560, 1440],
        fullscreen: true,
        vsync: false,
        ..default()
    };
    window_settings(&mut w, &s);
    // The monitor selects borderless size; the window preset must not override it.
    assert_eq!((w.physical_width(), w.physical_height()), (1280, 720));
    assert_eq!(
        w.mode,
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    );
    assert_eq!(w.present_mode, PresentMode::AutoNoVsync);
    window_settings(
        &mut w,
        &Settings {
            fullscreen: false,
            ..s
        },
    );
    assert_eq!((w.physical_width(), w.physical_height()), (2560, 1440));
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn a_window_resized_by_hand_keeps_its_size_in_the_settings() {
    let root = test_root("window-size");
    let (mut app, window) = headless(&root, WindowResolution::new(1280, 720));
    app.add_message::<bevy::window::WindowResized>();
    app.insert_resource(WindowFit {
        active: false,
        frames: 5,
    });
    app.update();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(1500, 840);
    app.world_mut().write_message(bevy::window::WindowResized {
        window,
        width: 1500.,
        height: 840.,
    });
    app.update();
    let menu = app.world().resource::<MenuState>();
    assert_eq!(
        (menu.settings.window, menu.draft.window),
        ([1500, 840], [1500, 840])
    );
    // Saved for the next start (at once here: the tests run no real time).
    assert_eq!(
        Settings::load(&root.join("settings.json")).unwrap().window,
        [1500, 840]
    );
    // In full screen the window's size is the screen's, not the player's.
    app.world_mut().get_mut::<Window>(window).unwrap().mode =
        WindowMode::BorderlessFullscreen(MonitorSelection::Current);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(1920, 1080);
    app.world_mut().write_message(bevy::window::WindowResized {
        window,
        width: 1920.,
        height: 1080.,
    });
    app.update();
    assert_eq!(
        app.world().resource::<MenuState>().settings.window,
        [1500, 840]
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn the_settings_change_the_window_at_once_and_cancel_brings_it_back() {
    let root = test_root("settings-at-once");
    let (mut app, window) = headless(&root, WindowResolution::new(1280, 720));
    app.update();
    let kept = std::fs::read(root.join("settings.json")).unwrap();
    let act = |app: &mut App, a: Action| {
        app.world_mut().resource_mut::<PendingAction>().0 = Some(a);
        app.update();
    };
    let shown = |app: &App| {
        let w = app.world().get::<Window>(window).unwrap();
        (w.mode, w.present_mode)
    };
    act(&mut app, Action::Open(Screen::Options));
    act(&mut app, Action::Tab(retzar::menu::SettingsTab::Display));
    // Each switch works as soon as it is moved, with nothing to confirm.
    act(&mut app, Action::Change(OptionKey::Fullscreen, 1));
    act(&mut app, Action::Change(OptionKey::Vsync, 1));
    assert_eq!(
        shown(&app),
        (
            WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            PresentMode::AutoNoVsync
        )
    );
    // «Annulla» puts back what there was and saves nothing.
    act(&mut app, Action::Back);
    assert_eq!(shown(&app), (WindowMode::Windowed, PresentMode::AutoVsync));
    assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), kept);
    // «OK» keeps the changes.
    act(&mut app, Action::Open(Screen::Options));
    act(&mut app, Action::Change(OptionKey::Vsync, 1));
    act(&mut app, Action::Apply);
    assert_eq!(
        shown(&app),
        (WindowMode::Windowed, PresentMode::AutoNoVsync)
    );
    assert!(!Settings::load(&root.join("settings.json")).unwrap().vsync);
    let _ = std::fs::remove_dir_all(root);
}

fn source() -> PathBuf {
    std::env::var_os("TZAR_DATA_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .to_path_buf()
        })
}

fn headless(root: &Path, resolution: WindowResolution) -> (App, Entity) {
    let source = source();
    let assets = GameAssets::open(&source).unwrap();
    let skin = MenuSkin::load(&assets).unwrap();
    let classic = HudSkin::load(&assets).unwrap();
    let menu = Menu::new(&source, italian(root)).unwrap();
    let mut app = App::new();
    app.add_plugins(StatesPlugin)
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<ColorMaterial>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseScroll>()
        .insert_resource(Time::<()>::default())
        .add_message::<WindowEvent>()
        .add_message::<KeyboardInput>()
        .add_message::<AppExit>()
        .insert_resource(Game {
            gameplay: retzar::gameplay::Catalog::load(&assets).unwrap(),
            assets: Arc::new(assets),
            skin,
            classic,
        })
        .insert_resource(MenuState(menu))
        .add_plugins(FrontendPlugin);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                resolution,
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    app.update();
    (app, window)
}

/// One 50 ms step of the open mission: its script runs its first rules.
fn first_step(app: &mut App) {
    let mut time = app.world_mut().resource_mut::<Time>();
    time.advance_by(std::time::Duration::from_millis(50));
    app.update();
    let mut time = app.world_mut().resource_mut::<Time>();
    time.advance_by(std::time::Duration::ZERO);
}

/// A private settings and saves folder under `target/test-roots/<pid>`. Folders left by
/// earlier runs (other processes) are removed, so failed tests never accumulate.
fn test_root(name: &str) -> PathBuf {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-roots");
    let own = std::process::id().to_string();
    static SWEEP: std::sync::Once = std::sync::Once::new();
    SWEEP.call_once(|| {
        for entry in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            if entry.file_name() != own.as_str() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    });
    let root = base.join(own).join(name);
    let _ = std::fs::remove_dir_all(&root);
    root
}

/// A test's folder with settings in Italian: the tests read the
/// installation's own texts, the default is English, and the language is
/// one for the whole process.
fn italian(root: &Path) -> &Path {
    let settings = root.join("settings.json");
    if !settings.exists() {
        std::fs::create_dir_all(root).unwrap();
        let language = retzar::lang::Language::Italian.index();
        std::fs::write(settings, format!("{{\"language\": {language}}}")).unwrap();
    }
    root
}

fn surface(app: &App, window: Entity) -> Surface {
    Surface::of(app.world().get::<Window>(window).unwrap())
}

/// Logical cursor position of menu canvas pixel `p`.
fn menu_cursor(app: &App, window: Entity, p: Vec2) -> Vec2 {
    let s = surface(app, window);
    let menu = app.world().resource::<MenuState>();
    let layout = canvas_layout(s, menu);
    (layout.origin + p * layout.factor) / s.dpi
}

fn move_to(app: &mut App, window: Entity, cursor: Vec2) {
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(cursor));
}

fn press(app: &mut App, button: MouseButton) {
    let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    mouse.clear();
    mouse.press(button);
}
fn release(app: &mut App, button: MouseButton) {
    let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    mouse.clear();
    mouse.release(button);
}
fn tap(app: &mut App, key: KeyCode) {
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.clear();
        keys.press(key);
    }
    app.update();
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release(key);
    keys.clear();
}
fn keyboard_event(key: KeyCode, state: ButtonState) -> KeyboardInput {
    KeyboardInput {
        key_code: key,
        logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    }
}

fn wait_for_scene(app: &mut App) {
    for _ in 0..2000 {
        app.update();
        if app.world().get_resource::<LoadJob>().is_none()
            && app.world().resource::<Loaded>().scene.is_some()
            && *app.world().resource::<State<Mode>>().get() == Mode::Mission
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!(
        "mission did not open: {}",
        app.world().resource::<MenuState>().notice
    );
}

#[test]
#[ignore = "Requires original assets; creates no game window"]
fn visual_animation_changes_gpu_frames_without_changing_world_state_and_pauses() {
    let root = test_root("visual-test");
    let (mut app, _) = headless(&root, (2560, 1440).into());
    let baseline = app.world().resource::<Assets<Image>>().len();
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
    // A campaign mission opens on its story screen (0x4c34c0), which its script shows
    // at the first step; the first Escape closes it and leaves the mission
    // running.
    first_step(&mut app);
    assert_eq!(
        app.world().resource::<mission::MissionUi>().hud.panel,
        Some(Panel::Narration)
    );
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert_eq!(app.world().resource::<mission::MissionUi>().hud.panel, None);
    assert_eq!(*app.world().resource::<State<Mode>>().get(), Mode::Mission);
    // The rest checks the pictures over a still world: the mission's script,
    // which moves units and opens windows, stops here.
    {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let mission = &mut loaded.scene.as_mut().unwrap().mission;
        mission.script = Default::default();
        mission.simulation.as_mut().unwrap().gameplay.script = retzar::gameplay::ScriptState {
            started: true,
            ..Default::default()
        };
    }
    let (records, positions, material, mesh, first) = {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        assert!(
            !s.terrain_frames.is_empty()
                && !s.water_meshes.is_empty()
                && !s.sprite_animations.is_empty()
        );
        let positions = s
            .visual_entities
            .iter()
            .zip(&s.visuals)
            .filter(|(_, v)| {
                s.mission
                    .objects
                    .get(v.object)
                    .is_none_or(|o| !drifts(o.class))
            })
            .filter_map(|(&e, _)| app.world().get::<Transform>(e).map(|t| (e, *t)))
            .collect::<Vec<_>>();
        (
            still_world(&s.mission),
            positions,
            s.terrain_frames[0].material.clone(),
            s.water_meshes[0].mesh.clone(),
            s.sprite_animations
                .iter()
                .map(|a| a.last)
                .collect::<Vec<_>>(),
        )
    };
    let texture = |app: &App| {
        app.world()
            .resource::<Assets<ColorMaterial>>()
            .get(&material)
            .unwrap()
            .texture
            .clone()
    };
    let uv = |app: &App| {
        app.world()
            .resource::<Assets<Mesh>>()
            .get(&mesh)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_UV_0)
            .unwrap()
            .clone()
    };
    let (initial, initial_uv) = (texture(&app), uv(&app));
    // One water tick (250 ms, WATERFPS=4) changes the sea page and the coast UVs.
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(250));
    app.update();
    assert_ne!(initial, texture(&app));
    assert_ne!(initial_uv, uv(&app));
    let mut changed = false;
    for _ in 0..80 {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(250));
        app.update();
        changed |= app
            .world()
            .resource::<Loaded>()
            .scene
            .as_ref()
            .unwrap()
            .sprite_animations
            .iter()
            .map(|a| a.last)
            .collect::<Vec<_>>()
            != first;
    }
    assert!(changed);
    // The pause menu and a background window both freeze the animation clock.
    let elapsed = app
        .world()
        .resource::<Loaded>()
        .scene
        .as_ref()
        .unwrap()
        .visual_time;
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert_eq!(*app.world().resource::<State<Mode>>().get(), Mode::Menu);
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(250));
    app.update();
    let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
    assert!(s.visual_time.as_millis() <= elapsed.as_millis() + 250 * 2);
    assert_eq!(records, still_world(&s.mission));
    // Positions stay; the depth follows the order of every drawn object,
    // which the drifting fish patches change when they are drawn again.
    for (e, t) in positions {
        let now = *app.world().get::<Transform>(e).unwrap();
        assert_eq!(t.translation.truncate(), now.translation.truncate());
        assert_eq!((t.rotation, t.scale), (now.rotation, now.scale));
    }
    menu_ui::close_scene(app.world_mut());
    app.update();
    assert_eq!(baseline, app.world().resource::<Assets<Image>>().len());
    assert_eq!(0, app.world().resource::<Assets<Mesh>>().len());
    assert_eq!(0, app.world().resource::<Assets<ColorMaterial>>().len());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn help_mode_blocks_controls_and_escape_restores_navigation() {
    let root = test_root("help-test");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    app.world_mut()
        .resource_mut::<MenuState>()
        .open(Screen::Options);
    app.update();
    let click = |app: &mut App, p: Vec2| {
        let cursor = menu_cursor(app, window, p);
        move_to(app, window, cursor);
        press(app, MouseButton::Left);
        app.update();
        release(app, MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
    };
    let before = app.world().resource::<MenuState>().draft.clone();
    let normal = {
        let w = app.world();
        w.resource::<Game>()
            .skin
            .render(w.resource::<MenuState>())
            .crop(0, 0, 57, 74)
            .unwrap()
            .rgba
    };
    click(&mut app, Vec2::new(28., 37.));
    assert!(app.world().resource::<MenuState>().help);
    let helping = {
        let w = app.world();
        w.resource::<Game>()
            .skin
            .render(w.resource::<MenuState>())
            .crop(0, 0, 57, 74)
            .unwrap()
            .rgba
    };
    assert_ne!(
        helping, normal,
        "the original help ornament shows its active state"
    );
    click(&mut app, Vec2::new(430., 96.));
    assert_eq!(app.world().resource::<MenuState>().draft, before);
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Options);
    {
        let w = app.world();
        let menu = w.resource::<MenuState>();
        let skin = &w.resource::<Game>().skin;
        let widgets = skin.widgets(menu);
        let hovered = &widgets[menu.hover.unwrap()];
        assert_eq!(
            hovered.action,
            retzar::menu::Action::Change(retzar::menu::OptionKey::SoundOn, 1)
        );
        let key = menu.hint_key(hovered).unwrap();
        assert_ne!(skin.hint_text(key), key);
        // Hints add the original cream box beneath the pointer.
        let with_hint = skin.render(menu);
        let mut without_hint = retzar::menu::Menu::new(&source(), italian(&root)).unwrap();
        without_hint.open(Screen::Options);
        let plain = skin.render(&without_hint);
        assert_ne!(
            with_hint.crop(60, 112, 740, 24).unwrap().rgba,
            plain.crop(60, 112, 740, 24).unwrap().rgba
        );
    }
    click(&mut app, Vec2::new(28., 37.));
    assert!(!app.world().resource::<MenuState>().help);
    click(&mut app, Vec2::new(28., 37.));
    assert!(app.world().resource::<MenuState>().help);
    tap(&mut app, KeyCode::Escape);
    assert!(!app.world().resource::<MenuState>().help);
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Options);
    // 0x45c600: the right button anywhere also ends hint mode.
    click(&mut app, Vec2::new(28., 37.));
    assert!(app.world().resource::<MenuState>().help);
    press(&mut app, MouseButton::Right);
    app.update();
    release(&mut app, MouseButton::Right);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    assert!(!app.world().resource::<MenuState>().help);
    click(&mut app, Vec2::new(430., 96.));
    assert_ne!(
        app.world().resource::<MenuState>().draft.sound_on,
        before.sound_on
    );
    tap(&mut app, KeyCode::Escape);
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Main);
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn menu_pointer_scale_changes_and_scrollbar_capture_are_consistent() {
    let root = test_root("pointer-test");
    let (mut app, window) = headless(
        &root,
        WindowResolution::new(2560, 1440).with_scale_factor_override(1.5),
    );
    // Fill mode fills the window; crisp mode uses whole factors.
    {
        let mut menu = app.world_mut().resource_mut::<MenuState>();
        menu.settings.ui_mode = 1;
        menu.tab = retzar::menu::SettingsTab::Display;
        menu.open(Screen::Options);
    }
    let target = Vec2::new(100., 120.);
    let cursor = menu_cursor(&app, window, target);
    move_to(&mut app, window, cursor);
    app.update();
    let hovered = app.world().resource::<MenuState>().hover;
    assert!(hovered.is_some());
    // A stationary cursor must be re-tested when the menu changes size.
    app.world_mut().resource_mut::<MenuState>().draft.ui_mode = 0;
    app.update();
    assert_ne!(app.world().resource::<MenuState>().hover, hovered);
    for ui_mode in [0, 1] {
        for (page, tab) in [
            (Screen::Main, None),
            (Screen::Single, None),
            (Screen::Options, Some(retzar::menu::SettingsTab::Audio)),
            (Screen::Options, Some(retzar::menu::SettingsTab::Game)),
            (Screen::Options, Some(retzar::menu::SettingsTab::Display)),
            (Screen::Options, Some(retzar::menu::SettingsTab::Keys)),
            (Screen::Campaign, None),
        ] {
            {
                let mut menu = app.world_mut().resource_mut::<MenuState>();
                menu.settings.ui_mode = ui_mode;
                if let Some(tab) = tab {
                    menu.tab = tab;
                }
                menu.open(page);
            }
            let widgets = {
                let w = app.world();
                w.resource::<Game>().skin.widgets(w.resource::<MenuState>())
            };
            for (i, b) in widgets.iter().enumerate().filter(|(_, b)| b.enabled) {
                let c = menu_cursor(
                    &app,
                    window,
                    Vec2::new(b.rect.x as f32 + 1.5, b.rect.y as f32 + 1.5),
                );
                move_to(&mut app, window, c);
                app.update();
                assert_eq!(
                    app.world().resource::<MenuState>().hover,
                    Some(i),
                    "{} (mode {ui_mode})",
                    b.label
                );
            }
        }
    }
    app.world_mut()
        .resource_mut::<MenuState>()
        .open(Screen::Help);
    let bar = {
        let w = app.world();
        w.resource::<Game>()
            .skin
            .scrollbars(w.resource::<MenuState>())[0]
    };
    let c = menu_cursor(
        &app,
        window,
        Vec2::new(bar.track.x as f32 + 6., bar.thumb().y as f32 + 5.),
    );
    move_to(&mut app, window, c);
    press(&mut app, MouseButton::Left);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    // Keep dragging outside the track; pointer capture must continue and clamp.
    let c = menu_cursor(
        &app,
        window,
        Vec2::new(
            bar.track.x as f32 + 40.,
            (bar.track.y + bar.track.h + 30) as f32,
        ),
    );
    move_to(&mut app, window, c);
    app.update();
    assert_eq!(
        app.world().resource::<MenuState>().scroll as usize,
        bar.max()
    );
    let c = menu_cursor(
        &app,
        window,
        Vec2::new(bar.track.x as f32 + 40., (bar.track.y - 30) as f32),
    );
    move_to(&mut app, window, c);
    app.update();
    assert_eq!(app.world().resource::<MenuState>().scroll, 0);
    release(&mut app, MouseButton::Left);
    app.update();
    assert!(app.world().resource::<MenuPointer>().scroll_drag.is_none());

    fn fast_drag(app: &mut App, window: Entity, from: Vec2, to: Vec2) {
        move_to(app, window, from);
        move_to(app, window, to);
        for event in [
            WindowEvent::CursorMoved(CursorMoved {
                window,
                position: from,
                delta: None,
            }),
            WindowEvent::MouseButtonInput(MouseButtonInput {
                window,
                button: MouseButton::Left,
                state: ButtonState::Pressed,
            }),
            WindowEvent::CursorMoved(CursorMoved {
                window,
                position: to,
                delta: Some(to - from),
            }),
            WindowEvent::MouseButtonInput(MouseButtonInput {
                window,
                button: MouseButton::Left,
                state: ButtonState::Released,
            }),
        ] {
            app.world_mut().write_message(event);
        }
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.clear();
        mouse.press(MouseButton::Left);
        mouse.release(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
    }
    let from = menu_cursor(
        &app,
        window,
        Vec2::new(bar.track.x as f32 + 6., bar.thumb().y as f32 + 5.),
    );
    let to = menu_cursor(
        &app,
        window,
        Vec2::new(
            bar.track.x as f32 + 40.,
            (bar.track.y + bar.track.h + 30) as f32,
        ),
    );
    fast_drag(&mut app, window, from, to);
    assert_eq!(
        app.world().resource::<MenuState>().scroll as usize,
        bar.max()
    );
    assert!(app.world().resource::<MenuPointer>().scroll_drag.is_none());
    {
        let mut menu = app.world_mut().resource_mut::<MenuState>();
        menu.tab = retzar::menu::SettingsTab::Audio;
        menu.open(Screen::Options);
    }
    let slider = {
        let w = app.world();
        w.resource::<Game>()
            .skin
            .widgets(w.resource::<MenuState>())
            .into_iter()
            .find(|b| matches!(b.action, Action::Change(OptionKey::Music, _)))
            .unwrap()
    };
    let from = menu_cursor(
        &app,
        window,
        Vec2::new(slider.rect.x as f32 + 50., slider.rect.y as f32 + 10.),
    );
    let to = menu_cursor(
        &app,
        window,
        Vec2::new(slider.rect.x as f32 - 40., slider.rect.y as f32 + 90.),
    );
    fast_drag(&mut app, window, from, to);
    assert_eq!(app.world().resource::<MenuState>().draft.music, 0);
    // The wheel on the slider moves it by five.
    move_to(&mut app, window, from);
    app.world_mut()
        .resource_mut::<AccumulatedMouseScroll>()
        .delta = Vec2::new(0., 1.);
    app.update();
    app.world_mut()
        .resource_mut::<AccumulatedMouseScroll>()
        .delta = Vec2::ZERO;
    app.update();
    assert_eq!(app.world().resource::<MenuState>().draft.music, 5);
    app.world_mut()
        .resource_mut::<MenuState>()
        .open(Screen::Main);
    let button = {
        let w = app.world();
        w.resource::<Game>()
            .skin
            .widgets(w.resource::<MenuState>())
            .into_iter()
            .find(|b| b.action == Action::Open(Screen::Options))
            .unwrap()
    };
    // A drag that ends on a button without starting on it does not press it.
    let from = menu_cursor(&app, window, Vec2::new(100., 100.));
    let to = menu_cursor(
        &app,
        window,
        Vec2::new(button.rect.x as f32 + 10., button.rect.y as f32 + 10.),
    );
    fast_drag(&mut app, window, from, to);
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Main);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn complete_menu_and_scene_ecs_operate_without_render_or_window_plugins() {
    let root = test_root("ecs-menu");
    let (mut app, window) = headless(&root, (2560, 1440).into());
    // Click the first main-menu button: single player.
    let first = {
        let w = app.world();
        w.resource::<Game>().skin.widgets(w.resource::<MenuState>())[0].rect
    };
    let c = menu_cursor(
        &app,
        window,
        Vec2::new(first.x as f32 + 10., first.y as f32 + 10.),
    );
    move_to(&mut app, window, c);
    press(&mut app, MouseButton::Left);
    app.update();
    release(&mut app, MouseButton::Left);
    app.update();
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Single);

    // Opening a mission runs on a loading thread; the window keeps updating.
    {
        let world = app.world_mut();
        world.resource_mut::<MenuState>().open(Screen::Campaign);
        world.resource_mut::<PendingAction>().0 = Some(Action::Play);
    }
    app.update();
    assert!(
        app.world().get_resource::<LoadJob>().is_some()
            || app.world().resource::<Loaded>().scene.is_some()
    );
    wait_for_scene(&mut app);
    let counts = |app: &App| {
        (
            app.world().resource::<Assets<Image>>().len(),
            app.world().resource::<Assets<Mesh>>().len(),
            app.world().resource::<Assets<ColorMaterial>>().len(),
        )
    };
    app.update();
    let loaded = counts(&app);
    // Restart replaces the scene without leaking resources.
    tap(&mut app, KeyCode::Escape);
    app.update();
    app.world_mut().resource_mut::<PendingAction>().0 = Some(Action::Restart);
    app.update();
    wait_for_scene(&mut app);
    app.update();
    assert_eq!(loaded, counts(&app));
    // The restarted mission shows its story again; close it before the map tests.
    first_step(&mut app);
    assert_eq!(
        app.world().resource::<mission::MissionUi>().hud.panel,
        Some(Panel::Narration)
    );
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert_eq!(app.world().resource::<mission::MissionUi>().hud.panel, None);
    let s = surface(&app, window);
    let magnification = app.world().resource::<MapView>().magnification(s);
    assert_eq!(magnification, 2., "the map opens on the original framing");
    // The wheel moves through the three levels and stops at both ends.
    move_to(&mut app, window, Vec2::new(900., 700.));
    for (lines, expected) in [
        (1., 2.),
        (1., 2.),
        (-1., 1.5),
        (-1., 1.),
        (-1., 1.),
        (1., 1.5),
    ] {
        app.world_mut()
            .resource_mut::<AccumulatedMouseScroll>()
            .delta = Vec2::new(0., lines);
        app.update();
        app.world_mut()
            .resource_mut::<AccumulatedMouseScroll>()
            .delta = Vec2::ZERO;
        let m = app.world().resource::<MapView>().magnification(s);
        assert!((m - expected).abs() < 1e-3, "zoom {m}, expected {expected}");
    }
    // «Zoom fluido», an option of this version: a notch sets the next of
    // seven steps as the glide's target instead of jumping to it.
    app.world_mut()
        .resource_mut::<MenuState>()
        .settings
        .smooth_zoom = true;
    app.world_mut()
        .resource_mut::<AccumulatedMouseScroll>()
        .delta = Vec2::new(0., 1.);
    app.update();
    app.world_mut()
        .resource_mut::<AccumulatedMouseScroll>()
        .delta = Vec2::ZERO;
    let m = app.world().resource::<MapView>().magnification(s);
    assert!((m - 1.5).abs() < 1e-3, "no jump, zoom {m}");
    let (to, _, _) = app.world().resource::<mission::MissionUi>().glide.unwrap();
    let next = view::glide_levels(&[1., 1.5, 2.])[4];
    assert!((s.dpi / to - next).abs() < 1e-3, "target {}", s.dpi / to);
    app.world_mut()
        .resource_mut::<MenuState>()
        .settings
        .smooth_zoom = false;
    app.update();
    assert!(app.world().resource::<mission::MissionUi>().glide.is_none());
    // «Pixel della mappa»: with a filter the map draws into an image that
    // the filter camera brings to the window; without, to the window again.
    let targets = |app: &mut App| {
        let world = app.world_mut();
        let map = world
            .query_filtered::<&bevy::camera::RenderTarget, With<MapCamera>>()
            .single(world)
            .unwrap()
            .clone();
        let filter = world
            .query_filtered::<&Camera, With<filters::FilterCamera>>()
            .single(world)
            .unwrap()
            .is_active;
        (matches!(map, bevy::camera::RenderTarget::Image(_)), filter)
    };
    for (kind, expected) in [(2, (true, true)), (0, (false, false))] {
        app.world_mut()
            .resource_mut::<MenuState>()
            .settings
            .pixel_filter = kind;
        app.update();
        assert_eq!(targets(&mut app), expected, "filter {kind}");
    }
    // A burning building's fires are mode 3 quads (0x420079: strength 100,
    // mode 3), never plain pictures with their black ground.
    let building = {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let s = loaded.scene.as_mut().unwrap();
        let j = s.mission.objects.iter().position(|o| o.building()).unwrap();
        let (x, y) = (
            s.mission.objects[j].x as i32 * 32,
            s.mission.objects[j].y as i32 * 32,
        );
        s.mission.simulation.as_mut().unwrap().gameplay.actors[j].fires = [
            Some([x + 40, y + 40]),
            Some([x + 60, y + 50]),
            Some([x + 50, y + 60]),
        ];
        j
    };
    app.update();
    let world = app.world_mut();
    let quads = world
        .query::<(&MeshMaterial2d<glow::GlowMaterial>, Option<&Visibility>)>()
        .iter(world)
        .filter(|(_, v)| v.is_none_or(|v| *v != Visibility::Hidden))
        .count();
    assert!(quads >= 3, "{quads} fires in mode 3");
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
        .actors[building]
        .fires = [None; 3];
    // «Luci notturne»: the light camera draws the light map the map's last
    // pass reads; off, it rests and the map reads none.
    for on in [true, false] {
        app.world_mut()
            .resource_mut::<MenuState>()
            .settings
            .night_lights = on;
        app.update();
        let world = app.world_mut();
        let lights_on = world
            .query_filtered::<&Camera, With<lights::LightCamera>>()
            .single(world)
            .unwrap()
            .is_active;
        let read = world
            .query_filtered::<&legacy_color::LegacyColor, With<MapCamera>>()
            .single(world)
            .unwrap()
            .lights
            .is_some();
        assert_eq!((lights_on, read), (on, on), "night lights {on}");
    }
    // «Nebbia morbida»: the map's last pass reads the fog's vertices and the
    // original's tiles (z 450 and 451) stay hidden; off, the tiles again.
    for on in [true, false] {
        app.world_mut()
            .resource_mut::<MenuState>()
            .settings
            .soft_fog = on;
        app.update();
        let world = app.world_mut();
        let read = world
            .query_filtered::<&legacy_color::LegacyColor, With<MapCamera>>()
            .single(world)
            .unwrap()
            .fog
            .is_some();
        let tiles = world
            .query::<(&Sprite, &Transform, &Visibility)>()
            .iter(world)
            .filter(|(_, t, v)| {
                (450.0..=451.0).contains(&t.translation.z) && **v != Visibility::Hidden
            })
            .count();
        assert_eq!(read, on, "soft fog {on}");
        if on {
            assert_eq!(tiles, 0, "fog tiles under the soft fog");
        }
    }
    // «Ambiente vivo»: trees take bent pictures of their own and the water
    // mask reaches the map's last pass; off, every tree its own picture.
    let trees = |app: &App| -> Vec<Handle<Image>> {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        s.visuals
            .iter()
            .zip(&s.visual_entities)
            .filter(|(v, _)| v.tree && v.layer == retzar::world::Layer::Body)
            .filter_map(|(_, &e)| app.world().get::<Sprite>(e).map(|s| s.image.clone()))
            .collect()
    };
    let before = trees(&app);
    assert!(!before.is_empty());
    for on in [true, false] {
        app.world_mut()
            .resource_mut::<MenuState>()
            .settings
            .living_world = on;
        app.update();
        let now = trees(&app);
        let bent = now.iter().zip(&before).filter(|(a, b)| a != b).count();
        let world = app.world_mut();
        let water = world
            .query_filtered::<&legacy_color::LegacyColor, With<MapCamera>>()
            .single(world)
            .unwrap()
            .water
            .is_some();
        if on {
            assert!(bent > 0 && water, "{bent} trees bent, water {water}");
        } else {
            assert_eq!((bent, water), (0, false));
        }
    }

    // Middle-button drag inside the map, away from the camera's border clamps.
    let side = app
        .world()
        .resource::<Loaded>()
        .scene
        .as_ref()
        .unwrap()
        .side;
    app.world_mut().insert_resource(MapView {
        center: Vec2::new(side as f32 * 16., -(side as f32) * 16.),
        scale: 0.5,
        frame: Default::default(),
    });
    move_to(&mut app, window, Vec2::new(900., 700.));
    press(&mut app, MouseButton::Middle);
    app.update();
    let initial = *app.world().resource::<MapView>();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    move_to(&mut app, window, Vec2::new(950., 720.));
    app.update();
    let moved = *app.world().resource::<MapView>();
    assert!((moved.center - initial.center - Vec2::new(-50., 20.) * initial.scale).length() < 0.01);
    // Regression: the drag continues when it passes over the stone frame.
    move_to(&mut app, window, Vec2::new(2500., 720.));
    app.update();
    let over_frame = *app.world().resource::<MapView>();
    assert!(over_frame.center.x < moved.center.x);
    release(&mut app, MouseButton::Middle);
    app.update();

    // The opening of «Merlino e re Artù» locks the view on an object until
    // its own UnlockView («congelar», LockView 900000), and meanwhile the
    // edges do not scroll: a step that locks it is taken again unlocked.
    let mut locked = false;
    // Regression: screen-edge scrolling works over the top bar and the side panel.
    for (cursor, check) in [
        (Vec2::new(1280., 2.), 1usize),
        (Vec2::new(2558., 720.), 0usize),
        (Vec2::new(1., 720.), 2),
        (Vec2::new(1280., 1438.), 3),
    ] {
        app.world_mut().insert_resource(MapView {
            center: Vec2::new(side as f32 * 16., -(side as f32) * 16.),
            scale: 0.5,
            frame: Default::default(),
        });
        move_to(&mut app, window, cursor);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(50));
        app.update();
        if let Some(lock) = app
            .world_mut()
            .resource_mut::<mission::MissionUi>()
            .view_lock
            .take()
        {
            assert!(lock.follow.is_some());
            locked = true;
            app.world_mut().insert_resource(MapView {
                center: Vec2::new(side as f32 * 16., -(side as f32) * 16.),
                scale: 0.5,
                frame: Default::default(),
            });
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(50));
            app.update();
        }
        let before = Vec2::new(side as f32 * 16., -(side as f32) * 16.);
        let after = app.world().resource::<MapView>().center;
        let moved = match check {
            0 => after.x > before.x,
            1 => after.y > before.y,
            2 => after.x < before.x,
            _ => after.y < before.y,
        };
        assert!(moved, "edge {cursor:?}: {before:?} -> {after:?}");
    }
    assert!(locked, "the opening's LockView");
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::ZERO);
    move_to(&mut app, window, Vec2::new(900., 700.));

    // Groups inspect the same imported objects without issuing simulation orders.
    let object_state = |app: &App| {
        app.world()
            .resource::<Loaded>()
            .scene
            .as_ref()
            .unwrap()
            .mission
            .objects
            .iter()
            .map(|o| (o.id, o.x, o.y, o.health, o.owner))
            .collect::<Vec<_>>()
    };
    let before = object_state(&app);
    let group: Vec<_> = {
        let mission = &app
            .world()
            .resource::<Loaded>()
            .scene
            .as_ref()
            .unwrap()
            .mission;
        mission
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| retzar::selection::own_unit(o, mission))
            .take(2)
            .map(|(i, _)| i)
            .collect()
    };
    assert_eq!(group.len(), 2);
    app.world_mut().resource_mut::<MissionUi>().hud.selected = group.clone();
    // The native input bridge can deliver the complete chord in one frame.
    for (key, state) in [
        (KeyCode::ControlLeft, ButtonState::Pressed),
        (KeyCode::Digit1, ButtonState::Pressed),
        (KeyCode::ControlLeft, ButtonState::Released),
    ] {
        app.world_mut().write_message(keyboard_event(key, state));
    }
    tap(&mut app, KeyCode::Digit1);
    app.world_mut()
        .resource_mut::<MissionUi>()
        .hud
        .selected
        .clear();
    tap(&mut app, KeyCode::Digit1);
    assert_eq!(app.world().resource::<MissionUi>().hud.selected, group);
    // Groups are kept and recalled without a word, as in the original.
    assert!(
        !app.world()
            .resource::<MissionUi>()
            .messages
            .iter()
            .any(|m| m.contains("Gruppo"))
    );
    assert_eq!(before, object_state(&app));

    // Pause, then the load page and back: it returns to the pause menu.
    tap(&mut app, KeyCode::F10);
    app.update();
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Pause);
    assert_eq!(*app.world().resource::<State<Mode>>().get(), Mode::Menu);
    app.world_mut().resource_mut::<PendingAction>().0 = Some(Action::Open(Screen::Load));
    app.update();
    tap(&mut app, KeyCode::Escape);
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Pause);
    assert!(app.world().resource::<Loaded>().scene.is_some());
    // The settings opened from the game menu leave the game around their
    // page, as the original's centred window (0x47b700).
    app.world_mut().resource_mut::<PendingAction>().0 = Some(Action::Open(Screen::Options));
    app.update();
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Options);
    assert!(app.world().resource::<MenuState>().over_map());
    let hud = app.world().resource::<UiEntities>().hud;
    assert_eq!(
        app.world().get::<Visibility>(hud),
        Some(&Visibility::Inherited)
    );
    tap(&mut app, KeyCode::Escape);
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Pause);

    // Regression: changing modes must use the monitor, then recover the selected
    // window size even if Winit asynchronously restores old 800x600 bounds.
    app.world_mut().spawn((
        Monitor {
            name: Some("QHD test monitor".into()),
            physical_width: 2560,
            physical_height: 1440,
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: Some(144000),
            scale_factor: 1.,
            video_modes: Vec::new(),
        },
        PrimaryMonitor,
    ));
    {
        let mut w = app.world_mut().get_mut::<Window>(window).unwrap();
        w.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
        w.resolution.set_physical_resolution(3840, 2160);
    }
    app.world_mut().insert_resource(WindowFit::default());
    app.update();
    let w = app.world().get::<Window>(window).unwrap();
    assert_eq!((w.physical_width(), w.physical_height()), (2560, 1440));
    for _ in 0..5 {
        app.update();
    }
    app.world_mut().insert_resource(WindowFit::default());
    {
        let mut w = app.world_mut().get_mut::<Window>(window).unwrap();
        w.mode = WindowMode::Windowed;
        w.resolution.set_physical_resolution(800, 600);
    }
    app.update();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(800, 600);
    app.update();
    let w = app.world().get::<Window>(window).unwrap();
    assert_eq!((w.physical_width(), w.physical_height()), (2496, 1330));
    for _ in 0..5 {
        app.update();
    }
    assert!(!app.world().resource::<WindowFit>().active);

    // Back to the main menu closes the mission and frees its resources.
    app.world_mut().resource_mut::<PendingAction>().0 = Some(Action::Open(Screen::Main));
    app.update();
    app.update();
    assert!(app.world().resource::<Loaded>().scene.is_none());
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
    let _ = std::fs::remove_dir_all(root);
}

/// «Lingua» changed during a game: the names of units and buildings follow
/// it, as the mission's texts do (the language itself is left alone, being
/// one for the whole process).
#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn the_open_game_takes_the_names_of_the_new_language() {
    let root = test_root("relocalize");
    let (mut app, _) = headless(&root, (1280, 720).into());
    let mission = app
        .world()
        .resource::<MenuState>()
        .catalog
        .missions
        .iter()
        .position(|m| m.id == "mp1.wmp::maps/mp1")
        .unwrap();
    let selection = Selection::Scenario {
        campaign: None,
        mission,
    };
    menu_ui::load_now(app.world_mut(), selection, None).unwrap();
    app.update();
    let english = GameAssets::open_in(&source(), retzar::lang::Language::English).unwrap();
    let english = retzar::assets::Ini::locale(&english).unwrap();
    let mut loaded = app.world_mut().resource_mut::<Loaded>();
    let scene = loaded.scene.as_mut().unwrap();
    use retzar::world::class;
    let found: Vec<_> = [
        (class::PEON, "Contadino", "Peasant"),
        (class::RES_GOLD, "Oro", "Golden rocks"),
    ]
    .into_iter()
    .filter_map(|(c, it, en)| {
        let i = scene.mission.objects.iter().position(|o| o.class == c)?;
        Some((i, it, en))
    })
    .collect();
    assert!(!found.is_empty());
    let name = |s: &scene::Scene, i: usize| s.library.class_label(&s.mission.objects[i]);
    let before: Vec<String> = found.iter().map(|&(i, ..)| name(scene, i)).collect();
    scene.relocalize(retzar::lang::Language::English, english);
    for (&(i, it, en), before) in found.iter().zip(before) {
        assert_eq!((before.as_str(), name(scene, i).as_str()), (it, en));
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn large_minimaps_keep_visible_objects_and_hide_unexplored_kingdoms() {
    let root = test_root("minimap");
    let (mut app, _) = headless(&root, (2560, 1440).into());
    // «Il mito di Camelot»: a 512-cell map, larger than its 256-pixel minimap.
    let menu = &app.world().resource::<MenuState>().0;
    let (campaign, mission) = menu
        .catalog
        .campaigns
        .iter()
        .enumerate()
        .find_map(|(c, k)| {
            k.missions
                .iter()
                .position(|m| m.side == 512)
                .map(|m| (c, m))
        })
        .unwrap();
    app.world_mut().resource_mut::<MenuState>().setup = Some(retzar::world::GameSetup {
        options: Some(retzar::world::GameRules::default()),
        ..Default::default()
    });
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: Some(campaign),
            mission,
        },
        None,
    )
    .unwrap();
    app.update();
    app.update();
    let entities = app.world().resource::<UiEntities>();
    let sprite = app.world().get::<Sprite>(entities.minimap).unwrap();
    let image = app
        .world()
        .resource::<Assets<Image>>()
        .get(&sprite.image)
        .unwrap();
    // Texture size equals its on-screen size: 128 interface pixels at factor 2.
    assert_eq!((image.width(), image.height()), (256, 256));
    let scene = app.world().resource::<Loaded>().scene.as_ref().unwrap();
    let hud = &app.world().resource::<Game>().classic;
    assert_eq!(hud.player_colors[0], [0, 0, 189]);
    let palette = hud.minimap_colors;
    assert_eq!(
        palette,
        [
            [41, 115, 255],
            [231, 0, 0],
            [57, 115, 16],
            [231, 0, 231],
            [255, 255, 0],
            [0, 231, 231],
            [255, 148, 0],
            [90, 90, 90]
        ]
    );
    assert_eq!(
        app.world().get::<Msaa>(
            app.world()
                .iter_entities()
                .find(|e| e.contains::<MapCamera>())
                .unwrap()
                .id()
        ),
        Some(&Msaa::Off)
    );
    assert_eq!(image.sampler, bevy::image::ImageSampler::nearest());
    let pixels = mission::minimap_image(scene, &palette, &[], 256);
    let mut colors: Vec<_> = (0..=10)
        .map(|owner| mission::owner_color(scene, &palette, owner))
        .collect();
    colors.push([0, 255, 0, 255]);
    let mut checked = 0;
    // Native markers use the footprint centre. Items in holders and fields
    // have no standalone point, and nearby objects may share minimap pixels.
    for o in scene.mission.objects.iter().filter(|o| {
        mission::minimap_marker(o) > 0
            && scene
                .mission
                .simulation
                .as_ref()
                .is_none_or(|s| s.visible(o))
            && scene.mission.simulation.as_ref().is_none_or(|s| {
                let index = scene
                    .mission
                    .objects
                    .iter()
                    .position(|v| v.id == o.id)
                    .unwrap();
                s.objects[index].holder.is_none()
            })
    }) {
        let [cx, cy] = mission::minimap_center(scene, o);
        let (x, y) = (cx / 4 * 2, cy / 4 * 2);
        let at = (y * 256 + x) * 4;
        assert!(
            colors.iter().any(|c| c[..] == pixels.rgba[at..at + 4]),
            "object {} at ({}, {}) is missing from the minimap",
            o.id,
            o.x,
            o.y
        );
        checked += 1;
    }
    assert!(checked > 0);
    let sim = scene.mission.simulation.as_ref().unwrap();
    let unknown = scene
        .mission
        .objects
        .iter()
        .find(|o| {
            mission::minimap_marker(o) > 0
                && {
                    let [x, y] = mission::minimap_center(scene, o);
                    !sim.fog.at(x, y).0 && !sim.fog.at(x + 1, y + 1).0
                }
                && o.owner != scene.mission.player()
        })
        .unwrap();
    let [cx, cy] = mission::minimap_center(scene, unknown);
    let at = (cy / 4 * 2 * 256 + cx / 4 * 2) * 4;
    assert_eq!(&pixels.rgba[at..at + 3], &[0, 0, 0]);
    let _ = std::fs::remove_dir_all(root);
}

#[path = "setup_tests.rs"]
mod game_setup;
#[test]
fn original_cursor_variants_keep_native_pixels_hotspot_and_draft_preview() {
    let root = test_root("native-cursors");
    let (mut app, window) = headless(&root, WindowResolution::new(2560, 1440));
    let check = |app: &App, expected: &Bitmap| {
        let CursorIcon::Custom(CustomCursor::Image(cursor)) =
            app.world().get::<CursorIcon>(window).unwrap()
        else {
            panic!("The original hardware cursor uses H_DEF, not the generic Windows arrow");
        };
        assert_eq!(cursor.hotspot, (2, 0));
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&cursor.handle)
            .unwrap();
        assert_eq!(image.width(), expected.width);
        assert_eq!(image.height(), expected.height);
        assert_eq!(image.data.as_deref(), Some(expected.rgba.as_slice()));
    };
    check(
        &app,
        &app.world().resource::<Game>().classic.hardware_cursor,
    );
    app.world_mut()
        .resource_mut::<MenuState>()
        .open(Screen::Options);
    app.world_mut()
        .resource_mut::<MenuState>()
        .draft
        .system_cursor = false;
    app.update();
    check(&app, &app.world().resource::<Game>().classic.cursor);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(3840, 2160);
    app.world_mut().resource_mut::<MenuState>().draft.ui_mode = 1;
    app.update();
    check(&app, &app.world().resource::<Game>().classic.cursor);
    // Cancel restores the chosen hardware variant, without saving the preview.
    app.world_mut().resource_mut::<PendingAction>().0 = Some(Action::Back);
    app.update();
    app.update();
    check(
        &app,
        &app.world().resource::<Game>().classic.hardware_cursor,
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires the American release's movies (USA) and the installed Tzar.exe"]
fn the_american_movies_play_in_order_and_each_is_skipped_by_itself() {
    let root = test_root("american-movies");
    let (mut app, _) = headless(&root, WindowResolution::new(1280, 720));
    let Some(request) = movie::Request::american(&source()) else {
        return;
    };
    app.world_mut().resource_mut::<movie::Movie>().request = Some(request);
    let skip = |app: &mut App| {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::Escape);
        keys.clear();
        app.update();
    };
    // TalonSoft, Infinite Loop and Haemimont at 320×240, the intro at 320×200.
    for (n, height) in [240, 240, 240, 200].into_iter().enumerate() {
        app.update();
        app.update();
        let m = app.world().resource::<movie::Movie>();
        assert!(m.active(), "filmato {n}");
        assert_eq!(
            (m.status.width, m.status.height),
            (320, height),
            "filmato {n}"
        );
        skip(&mut app);
    }
    assert!(!app.world().resource::<movie::Movie>().active());
    // A script's movie (`@PlayMovie(movies\outro.ddv)`, the American
    // campaign's end): one clip; a name not in the archive plays nothing.
    assert!(movie::Request::named(&source(), "movies\\nothing.ddv").is_none());
    let outro = movie::Request::named(&source(), "movies\\outro.ddv").unwrap();
    assert_eq!(outro.0.len(), 1);
    app.world_mut().resource_mut::<movie::Movie>().request = Some(outro);
    app.update();
    app.update();
    let m = app.world().resource::<movie::Movie>();
    assert!(m.active());
    assert_eq!((m.status.width, m.status.height), (320, 200));
    skip(&mut app);
    assert!(!app.world().resource::<movie::Movie>().active());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(windows)]
#[test]
#[ignore = "Opens the installed decoder to exercise skip and input isolation"]
fn intro_skip_restores_cursor_without_activating_the_underlying_menu() {
    use bevy::window::CursorOptions;
    let root = test_root("skip-intro");
    let (mut app, window) = headless(&root, WindowResolution::new(1280, 720));
    // The movie overlaps the menu on the same window. A different sample
    // count makes the real GPU reject the shared depth/colour attachments.
    // Map, interface, movie, and the «GRAFICA» page's filter and lights.
    let mut cameras = app.world_mut().query::<(&Camera, &Msaa)>();
    assert_eq!(cameras.iter(app.world()).count(), 5);
    assert!(
        cameras
            .iter(app.world())
            .all(|(_, msaa)| *msaa == Msaa::Off)
    );
    app.world_mut()
        .resource_mut::<MenuState>()
        .open(Screen::Options);
    // The intro as the start of the game requests it.
    let intro = movie::Request::intro(&source());
    assert!(intro.is_some());
    app.world_mut().resource_mut::<movie::Movie>().request = intro;
    app.update();
    app.update();
    assert!(app.world().resource::<movie::Movie>().active());
    assert!(!app.world().get::<CursorOptions>(window).unwrap().visible);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert!(!app.world().resource::<movie::Movie>().active());
    assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Options);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.update();
    assert_eq!(app.world().resource::<MenuState>().screen, Screen::Options);
    let _ = std::fs::remove_dir_all(root);
}
#[path = "regression_tests.rs"]
mod regressions;
#[path = "environment_tests.rs"]
mod stationary_environment;

#[path = "button_tests.rs"]
mod buttons;
#[path = "gameplay_tests.rs"]
mod player_gameplay;
#[path = "session_tests.rs"]
mod session_regressions;

#[test]
#[ignore = "Requires original archives; headless minimap rendering"]
fn minimap_keeps_seen_buildings_in_the_fog_and_marks_the_selection() {
    use retzar::simulation::{Memory, Simulation};
    let root = test_root("minimap-memory");
    let (mut app, _) = headless(&root, (800, 600).into());
    menu_ui::load_now(
        app.world_mut(),
        Selection::Random {
            side: 128,
            seed: 846,
        },
        None,
    )
    .unwrap();
    app.update();
    let palette = app.world().resource::<Game>().classic.minimap_colors;
    let mut loaded = app.world_mut().resource_mut::<Loaded>();
    let scene = loaded.scene.as_mut().unwrap();
    let find = |class| {
        scene
            .mission
            .objects
            .iter()
            .find(|o| o.class == class)
            .unwrap()
            .clone()
    };
    let (mut castle, mut worker) = (
        find(retzar::world::class::CASTLE),
        find(retzar::world::class::PEON),
    );
    let player = scene.mission.player();
    let enemy = if player == 1 { 2 } else { 1 };
    (castle.owner, castle.x, castle.y) = (enemy, 40, 40);
    (worker.owner, worker.x, worker.y) = (player, 80, 80);
    scene.mission.objects = vec![castle, worker];
    scene
        .cells
        .rgba
        .chunks_exact_mut(4)
        .for_each(|p| p.copy_from_slice(&[0, 0, 0, 255]));
    let mut sim = Simulation::new(&scene.mission, &scene.library, scene.side, &scene.biome);
    sim.fog.explored.fill(true);
    sim.fog.explored_vertices.fill(true);
    sim.fog.visible.fill(false);
    sim.fog.visible_vertices.fill(false);
    sim.memory.clear();
    sim.memory.insert(0, Memory::default());
    scene.mission.simulation = Some(sim);
    let count = |scene: &scene::Scene, selected: &[usize], c: [u8; 4]| {
        mission::minimap_image(scene, &palette, selected, 128)
            .rgba
            .chunks_exact(4)
            .filter(|p| p[..3] == c[..3])
            .count()
    };
    let (theirs, ours) = (
        mission::owner_color(scene, &palette, enemy),
        mission::owner_color(scene, &palette, player),
    );
    // Covered by the fog and never seen: no marker (flag 0x2000).
    assert_eq!(count(scene, &[], theirs), 0);
    // Seen, then covered: its picture stays, 2×2 in the colours it had,
    // although it was taken meanwhile (0x420f20).
    let seen = scene.mission.objects[0].clone();
    scene.mission.objects[0].owner = player;
    let sim = scene.mission.simulation.as_mut().unwrap();
    sim.memory.insert(
        0,
        Memory {
            seen: true,
            snapshot: Some(seen),
        },
    );
    assert_eq!(count(scene, &[], theirs), 4);
    assert_eq!(count(scene, &[], ours), 0, "units only in view");
    // In view: the castle itself, 3×3 in its owner's colours, and the
    // selected peasant in the selection's (0x46f61e).
    let sim = scene.mission.simulation.as_mut().unwrap();
    sim.fog.visible.fill(true);
    sim.fog.visible_vertices.fill(true);
    let [r, g, b] = scene.selection_color;
    assert_eq!(count(scene, &[], theirs), 0);
    assert_eq!(count(scene, &[], ours), 9 + 4);
    assert_eq!(count(scene, &[1], ours), 9);
    assert_eq!(count(scene, &[1], [r, g, b, 255]), 4);
    drop(loaded);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires original archives; headless minimap rendering"]
fn minimap_native_markers_preserve_sizes_colours_centres_and_holders() {
    let root = test_root("native-minimap-markers");
    let (mut app, _) = headless(&root, (800, 600).into());
    menu_ui::load_now(
        app.world_mut(),
        Selection::Random {
            side: 128,
            seed: 846,
        },
        None,
    )
    .unwrap();
    app.update();
    let palette = app.world().resource::<Game>().classic.minimap_colors;
    let mut loaded = app.world_mut().resource_mut::<Loaded>();
    let scene = loaded.scene.as_mut().unwrap();
    let castle = scene
        .mission
        .objects
        .iter()
        .find(|o| o.class == retzar::world::class::CASTLE)
        .unwrap()
        .clone();
    let worker = scene
        .mission
        .objects
        .iter()
        .find(|o| o.class == retzar::world::class::PEON)
        .unwrap()
        .clone();
    scene.mission.simulation = None;
    scene.mission.objects.clear();
    scene
        .cells
        .rgba
        .chunks_exact_mut(4)
        .for_each(|p| p.copy_from_slice(&[0, 0, 0, 255]));
    for owner in 1..=8 {
        scene.mission.kingdoms[owner - 1].color = owner as u8;
        for (model, x, y) in [
            (&castle, owner as u16 * 13, 16),
            (&worker, owner as u16 * 13, 64),
        ] {
            let mut o = model.clone();
            o.owner = owner as u8;
            o.x = x;
            o.y = y;
            scene.mission.objects.push(o);
        }
    }
    for size in [128, 256] {
        let b = mission::minimap_image(scene, &palette, &[], size);
        for (owner, color) in palette.iter().enumerate() {
            let count = b
                .rgba
                .chunks_exact(4)
                .filter(|p| p[..3] == color[..])
                .count();
            assert_eq!(
                count,
                (9 + 4) * (size as usize / 128).pow(2),
                "kingdom {}",
                owner + 1
            );
        }
    }
    let b = mission::minimap_image(scene, &palette, &[], 128);
    let o = &scene.mission.objects[0];
    let [cx, cy] = mission::minimap_center(scene, o);
    assert_ne!([cx, cy], [o.x as usize, o.y as usize]);
    assert_eq!(
        &b.rgba[(cy * 128 + cx) * 4..(cy * 128 + cx) * 4 + 3],
        &palette[0]
    );
    let mut item = worker.clone();
    item.class = retzar::world::class::ITEM;
    item.owner = 0;
    item.x = 4;
    item.y = 4;
    scene.mission.objects = vec![item.clone()];
    assert_eq!(
        &mission::minimap_image(scene, &palette, &[], 128).rgba
            [(4 * 128 + 4) * 4..(4 * 128 + 4) * 4 + 3],
        &[0, 255, 0]
    );
    // Holder association and the native hidden flag suppress inventory dots.
    let mut sim = retzar::simulation::Simulation::new(
        &scene.mission,
        &scene.library,
        scene.side,
        &scene.biome,
    );
    sim.fog.explored.fill(true);
    sim.fog.visible.fill(true);
    sim.fog.explored_vertices.fill(true);
    sim.fog.visible_vertices.fill(true);
    scene.mission.simulation = Some(sim);
    let green = |scene: &scene::Scene| {
        mission::minimap_image(scene, &palette, &[], 128)
            .rgba
            .chunks_exact(4)
            .any(|p| p[..3] == [0, 255, 0])
    };
    assert!(green(scene));
    scene.mission.simulation.as_mut().unwrap().objects[0].holder = Some(0);
    assert!(!green(scene));
    scene.mission.simulation = None;
    scene.mission.objects[0].flags |= 0x2000;
    assert!(
        !mission::minimap_image(scene, &palette, &[], 128)
            .rgba
            .chunks_exact(4)
            .any(|p| p[..3] == [0, 255, 0])
    );
    for class in [
        63,
        retzar::world::class::RES_GOLD,
        retzar::world::class::GROUND_OBJECT,
    ] {
        item.class = class;
        item.owner = 1;
        assert_eq!(mission::minimap_marker(&item), 0);
    }
    drop(loaded);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn idle_frames_start_at_the_start_row_and_long_ones_vary() {
    use retzar::{animation::Timeline, simulation::NativeRng};
    use scene::IdleClock;
    // Three frames: 100 ms, 4000 ms (long: ±3000), 200 ms.
    let t = Timeline::new([100, 4000, 200]);
    let mut c = IdleClock::new(&t, 1000, 2);
    assert_eq!((c.frame, c.due), (2, 1200));
    let mut rng = NativeRng(666);
    let mut expect = NativeRng(666);
    // 0x4e144f: frame 0 for 100 ms, then frame 1 for 4000 ± 3000.
    c.advance(&t, 1200, false, &mut rng);
    assert_eq!((c.frame, c.due), (0, 1300));
    assert_eq!(rng, expect);
    c.advance(&t, 1300, false, &mut rng);
    let long = 4000 + expect.range(-3000, 3000) as u64;
    assert_eq!((c.frame, c.due), (1, 1300 + long));
    assert_eq!(rng, expect);
    // Frozen: the frame stays and the timer comes back every 10 s.
    let due = c.due;
    c.advance(&t, due, true, &mut rng);
    assert_eq!((c.frame, c.due), (1, due + 10_000));
}

#[test]
fn a_figure_runs_from_when_it_was_set_at_its_start_row() {
    use retzar::{animation::Timeline, world::class};
    let t = Timeline::new([100, 100, 100]);
    // vt+0xe8: set at 1000 ms on row 1, then round the delays.
    assert_eq!(scene::figure_frame(&t, 1000, 1000, 1), 1);
    assert_eq!(scene::figure_frame(&t, 1000, 1100, 1), 2);
    assert_eq!(scene::figure_frame(&t, 1000, 1250, 1), 0);
    // The scenery keeps its shared ticker (0x4312b0).
    assert!(scene::figure_set_on_creation(class::CASTLE));
    assert!(!scene::figure_set_on_creation(class::ANIMATION));
}

#[test]
fn script_scroll_glides_at_1024_pixels_a_second_in_75_ms_steps() {
    // 0x4f6d00: ten cells east, 320 pixels in 320 × 1000 / 1024 = 312 ms.
    let s = mission::ViewScroll::new([2, 3], [12, 3], 10.);
    assert_eq!(s.at(10.).0, Vec2::new(64., -96.));
    // The timer moves it every 75 ms (0x4f44c2): 1024 × 75 / 1000.
    assert_eq!(s.at(10.1).0, Vec2::new(64. + 76., -96.));
    assert_eq!(s.at(10.2).0, Vec2::new(64. + 153., -96.));
    // Then the last corner (0x4f6e10).
    assert_eq!(s.at(10.32), (Vec2::new(384., -96.), true));
    // Diagonally: sine and cosine of fpatan(dx, dy), truncated.
    let d = mission::ViewScroll::new([0, 0], [4, 4], 0.);
    assert_eq!(d.at(0.1).0, Vec2::new(54., -54.));
}

/// «Testi in alta definizione»: the menu's texts leave its picture for sharp
/// copies, laid over each other in order; one the hint box then covers keeps
/// the picture drawn as always, the box and the hint over it included.
#[test]
fn hd_text_keeps_covered_texts() {
    let source = source();
    let assets = GameAssets::open(&source).unwrap();
    let skin = MenuSkin::load(&assets).unwrap();
    let root = test_root("hd-text");
    let mut menu = Menu::new(&source, italian(&root)).unwrap();
    menu.open(Screen::Options);
    let record = |menu: &Menu| {
        let original = skin.render(menu);
        let (mut b, rec) = retzar::font::record(800, 600, || skin.render(menu));
        let count = rec.ops.len();
        let sharp = hd_text::resolve(&mut b, rec, || skin.render(menu));
        (original, b, count, sharp)
    };
    let (original, b, count, sharp) = record(&menu);
    assert!(count > 10, "{count} texts on the settings page");
    assert_eq!(sharp.len(), count, "nothing covers the settings' texts");
    let volume = sharp
        .iter()
        .find(|op| op.text == "Effetti sonori")
        .unwrap()
        .clone();
    let at = |b: &Bitmap, i: usize| b.rgba[i * 4..i * 4 + 4].to_vec();
    assert!(
        volume
            .pixels
            .iter()
            .any(|&(i, _)| at(&b, i) != at(&original, i)),
        "the text is out of the picture"
    );
    // The hint of a control, its box over «Effetti sonori».
    menu.help = true;
    menu.hover = skin
        .widgets(&menu)
        .iter()
        .position(|w| menu.hint_key(w).is_some());
    menu.pointer = Some([volume.x, volume.y - 22.]);
    let (original, b, _, sharp) = record(&menu);
    assert!(
        !sharp
            .iter()
            .any(|op| op.text == "Effetti sonori" && [op.x, op.y] == [volume.x, volume.y])
    );
    assert!(
        volume
            .pixels
            .iter()
            .all(|&(i, _)| at(&b, i) == at(&original, i)),
        "the covered text keeps the original picture"
    );
    let hint = sharp.last().unwrap();
    assert!(
        hint.y > volume.y - 4.,
        "the hint itself stays sharp, drawn last"
    );
}
