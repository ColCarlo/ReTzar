//! Bevy front end. The menu, the mission view and the presentation are
//! separate systems ordered in one chain; game data lives in resources.
mod crt;
mod effects;
mod environment;
mod filters;
mod gameplay;
mod glow;
mod grid;
mod hd_text;
mod input;
mod legacy_color;
mod lightning;
mod lights;
mod living;
mod menu_ui;
mod mission;
mod movie;
mod orders;
mod placement;
mod scene;
mod soft_fog;
mod view;

use bevy::{
    asset::RenderAssetUsages,
    camera::{CompositingSpace, visibility::RenderLayers},
    image::ImageSampler,
    input::InputSystems,
    prelude::*,
    render::{
        RenderPlugin,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        settings::{
            Backends, InstanceFlags, MemoryHints, PowerPreference, RenderCreation, WgpuSettings,
        },
        view::screenshot::{Screenshot, save_to_disk},
    },
    window::{
        CursorIcon, CustomCursor, CustomCursorImage, Monitor, MonitorSelection, OnMonitor,
        PresentMode, PrimaryMonitor, PrimaryWindow, WindowMode, WindowPosition,
        WindowResizeConstraints, WindowResized,
    },
    winit::{UpdateMode, WinitSettings},
};
use input::{KeyboardChordFrame, PointerFrame};
use menu_ui::{LoadJob, MenuPointer};
use mission::{MinimapState, MissionUi, Overlay};
use retzar::{
    Result,
    assets::GameAssets,
    audio::{Player, SoundState},
    bitmap::Bitmap,
    hud::{self, HudSkin},
    menu::{Action, MIN_WINDOW, Menu, MenuSkin, Screen, Selection, Settings},
    paths::Locations,
};
use scene::Loaded;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use view::{MapView, Surface, canvas_layout, hud_layout};

/// Whether the map or the menu has the input.
#[derive(States, Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mode {
    #[default]
    Menu,
    Mission,
}

/// Read-only original data shared with the loading thread.
#[derive(Resource)]
pub struct Game {
    pub assets: Arc<GameAssets>,
    pub skin: MenuSkin,
    pub classic: HudSkin,
    pub gameplay: retzar::gameplay::Catalog,
}

#[derive(Resource, Deref, DerefMut)]
pub struct MenuState(pub Menu);

/// The window's title: the game's name in the language of the moment.
fn window_title() -> String {
    retzar::lang::game_title(retzar::lang::current()).to_owned()
}

/// Tzar's icon, resource 1 of the executable (build.rs), on the title bar
/// and the taskbar: winit gives its windows none. On the main thread, which
/// owns the winit windows, once the window exists.
#[cfg(windows)]
fn window_icon(
    _: bevy::ecs::system::NonSendMarker,
    window: Query<Entity, With<PrimaryWindow>>,
    mut done: Local<bool>,
) {
    use winit::{
        dpi::PhysicalSize,
        platform::windows::{IconExtWindows, WindowExtWindows},
        window::Icon,
    };
    if *done {
        return;
    }
    let Ok(entity) = window.single() else {
        return;
    };
    bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        if let Some(window) = windows.get_window(entity) {
            let side = (16. * window.scale_factor()).round() as u32;
            let small = Icon::from_resource(1, Some(PhysicalSize::new(side, side)));
            window.set_window_icon(small.ok());
            window.set_taskbar_icon(Icon::from_resource(1, None).ok());
            *done = true;
        }
    });
}

/// «Lingua» changed (an addition of this version): the archives, the menus,
/// the interface and the open game's texts are read again in the new
/// language, without leaving the game.
pub(crate) fn switch_language(world: &mut World) -> Result<()> {
    let language = retzar::lang::Language::from_index(
        world.resource::<MenuState>().current_settings().language,
    );
    if language == retzar::lang::current() {
        return Ok(());
    }
    retzar::lang::set(language);
    let source = world.resource::<MenuState>().source().to_path_buf();
    let assets = GameAssets::open_in(&source, language)?;
    let skin = MenuSkin::load(&assets)?;
    let classic = HudSkin::load(&assets)?;
    let gameplay = retzar::gameplay::Catalog::load(&assets)?;
    let locale = retzar::assets::Ini::locale(&assets)?;
    *world.resource_mut::<Game>() = Game {
        assets: Arc::new(assets),
        skin,
        classic,
        gameplay,
    };
    world.resource_mut::<MenuState>().reload_language()?;
    let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
    if let Ok(mut w) = windows.single_mut(world) {
        w.title = window_title();
    }
    if let Some(scene) = world.resource_mut::<scene::Loaded>().scene.as_mut() {
        scene.relocalize(language, locale);
    }
    Ok(())
}

/// One menu or mission action per frame, applied with world access.
#[derive(Resource, Default)]
pub struct PendingAction(pub Option<Action>);

#[derive(Resource)]
pub struct SoundPlayer {
    pub player: Player,
    last: Option<SoundState>,
    last_selection: Option<(u32, u64, Vec<usize>)>,
    /// The load whose nature and rain timers run.
    generation: u32,
    ambience: retzar::audio::Scheduler,
    /// Centre cell and width in pixels of the view sounds are heard from.
    hearing: ([i32; 2], u32),
    /// Where the view was, in map pixels, when the sounds were last placed.
    heard: Option<[i32; 2]>,
}

/// Entities of the interface layer.
#[derive(Resource)]
pub struct UiEntities {
    canvas: Entity,
    hud: Entity,
    minimap: Entity,
    viewport: [Entity; 4],
}

/// Window-size corrections after mode changes; winit applies them
/// asynchronously, so the target is held for a few confirming frames.
#[derive(Resource)]
pub struct WindowFit {
    active: bool,
    frames: u8,
}
impl Default for WindowFit {
    fn default() -> Self {
        Self {
            active: true,
            frames: 0,
        }
    }
}

/// What the interface textures were last drawn for.
#[derive(Resource, Default)]
struct Presented {
    menu_key: Option<(Vec2, bool)>,
    hud_key: Option<(Vec2, bool)>,
    hud_background: Option<((i32, i32, bool), Bitmap)>,
}

#[derive(Resource)]
struct FramePacer {
    deadline: Instant,
}

/// Screenshot after the start-up action settles (`--capture FILE`).
#[derive(Resource)]
struct Capture {
    path: PathBuf,
    frames: u32,
    taken_at: Option<u32>,
}

/// Command-line launch choices.
#[derive(Default, Clone, Debug)]
pub struct Launch {
    pub intro: bool,
    pub capture: Option<PathBuf>,
    pub start: Option<Start>,
    /// Opening camera of `--scenario`: world centre and scale (`--at`).
    pub camera: Option<[f32; 3]>,
}
#[derive(Clone, Debug)]
pub enum Start {
    /// Scenario id as listed by `--list` (e.g. `mp1.wmp::maps/mp1`).
    Scenario(String),
    Random {
        side: usize,
        seed: u64,
    },
    Screen(Screen),
    /// A tab of the settings.
    Settings(retzar::menu::SettingsTab),
}

pub fn rgba_image(b: Bitmap) -> Image {
    Image::new(
        Extent3d {
            width: b.width,
            height: b.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        b.rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}
/// Scene textures: uploaded once and never read back, so the CPU copy is
/// dropped after upload (a 512-cell mission holds hundreds of MB of pixels).
pub fn gpu_image(b: Bitmap) -> Image {
    let mut image = rgba_image(b);
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    image
}
/// Puts a new picture on a sprite. Bevy draws a sprite through a material
/// made once from its image, size and texture, so a picture drawn in place
/// over the old one would never show: it goes in a new image and the old
/// one is dropped.
pub fn renew_image(images: &mut Assets<Image>, sprite: &mut Sprite, image: Image) {
    let old = std::mem::replace(&mut sprite.image, images.add(image));
    if old != Handle::default() {
        images.remove(old.id());
    }
}
fn ui_image(b: Bitmap, crisp: bool) -> Image {
    let mut image = rgba_image(b);
    image.sampler = if crisp {
        ImageSampler::nearest()
    } else {
        ImageSampler::linear()
    };
    image
}

pub fn window_settings(w: &mut Window, s: &Settings) {
    let [width, height] = s.window;
    if !s.fullscreen {
        w.resolution.set_physical_resolution(width, height);
    }
    w.mode = if s.fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };
    w.present_mode = if s.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };
}

/// The renderer: Vulkan only, on the most capable graphics card. Release
/// builds also drop wgpu's checks of indirect draws, which ReTzar never
/// issues (bevy keeps them whenever DirectX 12 could be chosen). A computer
/// without a Vulkan driver gets an explanation instead of a crash.
fn vulkan_settings() -> Result<WgpuSettings> {
    let mut flags = InstanceFlags::default();
    if !cfg!(debug_assertions) {
        flags.remove(InstanceFlags::VALIDATION_INDIRECT_CALL);
    }
    let flags = flags.with_env();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: Backends::VULKAN,
        flags,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    if bevy::tasks::block_on(instance.enumerate_adapters(Backends::VULKAN)).is_empty() {
        return Err(retzar::lang::t(
            "Nessuna scheda video Vulkan trovata. ReTzar disegna solo con Vulkan: aggiorna i driver della scheda video.",
        )
        .into());
    }
    Ok(WgpuSettings {
        backends: Some(Backends::VULKAN),
        power_preference: PowerPreference::HighPerformance,
        instance_flags: flags,
        memory_hints: MemoryHints::Performance,
        ..default()
    })
}

pub fn run(locations: &Locations, launch: Launch) -> Result<()> {
    // The menu reads the settings and with them the language.
    let mut menu = Menu::new(&locations.source, &locations.root)?;
    let assets = GameAssets::open_in(&locations.source, retzar::lang::current())?;
    let skin = MenuSkin::load(&assets)?;
    let classic = HudSkin::load(&assets)?;
    let mut window = Window {
        title: window_title(),
        resizable: true,
        // The menu's 800×600 page fits in the smallest window.
        resize_constraints: WindowResizeConstraints {
            min_width: MIN_WINDOW[0] as f32,
            min_height: MIN_WINDOW[1] as f32,
            ..default()
        },
        ..default()
    };
    window_settings(&mut window, &menu.settings);
    window.position = WindowPosition::Centered(MonitorSelection::Primary);
    let vulkan = vulkan_settings()?;
    // Without sounds.wdt or an audio device the game still runs, silently.
    let player = Player::start(&locations.source)
        .inspect_err(|e| {
            // The cause goes to the log; the page says only that it failed.
            eprintln!("Audio non disponibile: {e}");
            menu.audio_status = retzar::lang::t("Audio non disponibile").into();
        })
        .ok();
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.025, 0.018, 0.012)))
        // Full speed while playing; about 15 updates per second in the
        // background, like the original game's background frame limit.
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::reactive_low_power(Duration::from_millis(66)),
        })
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(window),
                    ..default()
                })
                .set(ImagePlugin::default_nearest())
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(Box::new(vulkan)),
                    ..default()
                }),
        );
    match &launch.start {
        Some(Start::Screen(screen)) => menu.open(*screen),
        Some(Start::Settings(tab)) => {
            menu.tab = *tab;
            menu.open(Screen::Options);
        }
        _ => {}
    }
    let gameplay = retzar::gameplay::Catalog::load(&assets)?;
    app.insert_resource(Game {
        assets: Arc::new(assets),
        skin,
        classic,
        gameplay,
    })
    .insert_resource(MenuState(menu))
    .insert_resource(FramePacer {
        deadline: Instant::now(),
    })
    .add_plugins(FrontendPlugin)
    .add_systems(Last, pace_frame);
    #[cfg(windows)]
    app.add_systems(Update, window_icon);
    if let Some(player) = player {
        app.insert_resource(SoundPlayer {
            player,
            last: None,
            last_selection: None,
            generation: u32::MAX,
            ambience: Default::default(),
            hearing: ([0, 0], 0),
            heard: None,
        });
    }
    if launch.intro {
        app.world_mut().resource_mut::<movie::Movie>().request =
            movie::Request::start(&locations.source);
    }
    if let Some(path) = launch.capture {
        app.insert_resource(Capture {
            path,
            frames: 0,
            taken_at: None,
        });
    }
    let camera = launch.camera;
    match launch.start {
        Some(Start::Scenario(id)) => {
            let menu = &app.world().resource::<MenuState>().0;
            let (campaign, mission) = menu
                .catalog
                .locate(&id.to_ascii_lowercase())
                .ok_or_else(|| format!("Scenario sconosciuto: {id}. Usa --list."))?;
            app.add_systems(Startup, move |world: &mut World| {
                let s = Selection::Scenario { campaign, mission };
                world.resource_mut::<MenuState>().selection = Some(s.clone());
                if let Err(e) = menu_ui::start_load(world, s, camera) {
                    world.resource_mut::<MenuState>().notice = e.to_string();
                }
            });
        }
        Some(Start::Random { side, seed }) => {
            app.add_systems(Startup, move |world: &mut World| {
                let s = Selection::Random { side, seed };
                world.resource_mut::<MenuState>().selection = Some(s.clone());
                if let Err(e) = menu_ui::start_load(world, s, None) {
                    world.resource_mut::<MenuState>().notice = e.to_string();
                }
            });
        }
        _ => {}
    }
    app.run();
    Ok(())
}

/// Every system of the front end. Hosts provide `Game`, `MenuState`, the
/// asset collections, input resources and a primary window.
pub struct FrontendPlugin;
impl Plugin for FrontendPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(legacy_color::LegacyColorPlugin)
            .add_plugins(glow::GlowPlugin)
            .add_plugins(filters::FiltersPlugin)
            .add_plugins(crt::CrtPlugin)
            .add_plugins(grid::GridPlugin)
            .init_state::<Mode>()
            .init_resource::<PointerFrame>()
            .init_resource::<KeyboardChordFrame>()
            .init_resource::<input::TypedKeys>()
            .init_resource::<MenuPointer>()
            .init_resource::<PendingAction>()
            .init_resource::<Loaded>()
            .init_resource::<MissionUi>()
            .init_resource::<mission::HeldView>()
            .init_resource::<MapView>()
            .init_resource::<Overlay>()
            .init_resource::<MinimapState>()
            .init_resource::<Presented>()
            .init_resource::<hd_text::HdText>()
            .init_resource::<WindowFit>()
            .init_resource::<movie::Movie>()
            .add_systems(Startup, movie::setup)
            .add_systems(Startup, setup)
            .add_systems(Startup, filters::setup)
            .add_systems(Startup, lights::setup)
            .add_systems(Startup, soft_fog::setup)
            .add_systems(
                PreUpdate,
                (input::logical_navigation, input::pointer_events).after(InputSystems),
            )
            .add_systems(
                Update,
                (
                    (fit_window, remember_window).chain(),
                    movie::update,
                    mission::release_on_blur,
                    menu_ui::menu_input
                        .run_if(in_state(Mode::Menu))
                        .run_if(movie::inactive)
                        .run_if(menu_ui::not_loading),
                    (
                        mission::hold_view,
                        mission::mission_input.run_if(movie::inactive),
                        mission::release_view,
                    )
                        .chain()
                        .run_if(in_state(Mode::Mission)),
                    apply_pending,
                    menu_ui::finish_load.run_if(menu_ui::loading),
                    menu_ui::finish_save,
                    gameplay::update
                        .run_if(in_state(Mode::Mission))
                        .run_if(movie::inactive),
                    (effects::update, lightning::update)
                        .chain()
                        .run_if(in_state(Mode::Mission))
                        .run_if(movie::inactive),
                    (placement::update, orders::update).chain(),
                    animate
                        .run_if(in_state(Mode::Mission))
                        .run_if(movie::inactive),
                    (environment::update, living::update)
                        .chain()
                        .run_if(in_state(Mode::Mission))
                        .run_if(movie::inactive),
                    mission::update_overlay,
                    mission::update_minimap,
                    (
                        present,
                        filters::update,
                        crt::sync,
                        lights::update,
                        soft_fog::update,
                        hd_text::update,
                    )
                        .chain(),
                    update_cursor,
                    window_shortcuts,
                    sync_audio.run_if(resource_exists::<SoundPlayer>),
                    capture.run_if(resource_exists::<Capture>),
                )
                    .chain(),
            );
    }
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        Camera2d,
        // Bevy 0.20 gives 2D cameras linear tone mapping, with its dithering:
        // the original's colours go to the screen as they are.
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        Camera {
            // First: with a pixel filter its picture goes to the screen
            // through the filter camera (order 0).
            order: -1,
            is_active: false,
            ..default()
        },
        MapCamera,
        // MSAA evaluates coverage away from the nearest texture sample. At
        // tile borders it can fetch an adjacent atlas frame, making coast
        // seams. Original pixel artwork is drawn without multisampling.
        Msaa::Off,
        CompositingSpace::Srgb,
        legacy_color::LegacyColor::default(),
        RenderLayers::layer(1),
    ));
    commands.spawn((
        Camera2d,
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        UiCamera,
        Msaa::Off,
        // Both cameras share the encoded target, so ClearColorConfig::None
        // loads the map. Only MapCamera carries the RGB555 post-process.
        CompositingSpace::Srgb,
        RenderLayers::layer(2),
    ));
    let blank = || Bitmap {
        width: 1,
        height: 1,
        rgba: vec![0; 4],
    };
    let canvas = commands
        .spawn((
            Sprite::from_image(images.add(rgba_image(blank()))),
            RenderLayers::layer(2),
        ))
        .id();
    let hud = commands
        .spawn((
            Sprite::from_image(images.add(rgba_image(blank()))),
            Visibility::Hidden,
            RenderLayers::layer(2),
            Transform::from_xyz(0., 0., 1.),
        ))
        .id();
    let minimap = commands
        .spawn((
            Sprite::default(),
            Visibility::Hidden,
            RenderLayers::layer(2),
            Transform::from_xyz(0., 0., 0.5),
        ))
        .id();
    let viewport = std::array::from_fn(|_| {
        commands
            .spawn((
                Sprite::from_color(Color::srgb_u8(255, 246, 190), Vec2::ONE),
                Visibility::Hidden,
                RenderLayers::layer(2),
                Transform::from_xyz(0., 0., 0.6),
            ))
            .id()
    });
    commands.insert_resource(UiEntities {
        canvas,
        hud,
        minimap,
        viewport,
    });
}

#[derive(Component)]
struct MapCamera;
#[derive(Component)]
struct UiCamera;

fn apply_pending(world: &mut World) {
    menu_ui::apply_pending(world);
    // Leaving to the main menu closes the open mission.
    let menu = world.resource::<MenuState>();
    if menu.screen == Screen::Main
        && world.resource::<State<Mode>>().get() == &Mode::Menu
        && world.resource::<Loaded>().scene.is_some()
    {
        menu_ui::close_scene(world);
    }
}

fn animate(
    time: Res<Time>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui: Res<MissionUi>,
    mut loaded: ResMut<Loaded>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sprites: Query<(&mut Sprite, &mut Transform)>,
) {
    // Like the original, the scene stands still while the game is in the background.
    if !window.focused || ui.hud.panel.is_some() {
        return;
    }
    if let Some(scene) = loaded.scene.as_mut() {
        scene::animate(&time, scene, &mut materials, &mut meshes, &mut sprites);
    }
}

/// The monitor showing the window, or the primary one before winit reports it.
fn current_monitor<'a>(
    on: Option<&OnMonitor>,
    monitors: &'a Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
) -> Option<&'a Monitor> {
    on.and_then(|m| monitors.get(m.0).ok().map(|(_, m, _)| m))
        .or_else(|| monitors.iter().find(|(_, _, p)| *p).map(|(_, m, _)| m))
}

/// Fit the window to the monitor it is on: windowed sizes leave room for the
/// taskbar, borderless fullscreen uses that monitor's own resolution.
fn fit_window(
    mut fit: ResMut<WindowFit>,
    menu: Res<MenuState>,
    mut windows: Query<(&mut Window, Option<&OnMonitor>), With<PrimaryWindow>>,
    monitors: Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
) {
    if !fit.active {
        return;
    }
    let Ok((mut w, on)) = windows.single_mut() else {
        return;
    };
    let Some(monitor) = current_monitor(on, &monitors) else {
        return;
    };
    let target = if w.mode == WindowMode::Windowed {
        // Leave room for borders, title bar and the Windows taskbar at the current DPI.
        let max_w = monitor
            .physical_width
            .saturating_sub((64. * monitor.scale_factor) as u32)
            .max(320);
        let max_h = monitor
            .physical_height
            .saturating_sub((110. * monitor.scale_factor) as u32)
            .max(240);
        let [requested_w, requested_h] = menu.current_settings().window;
        (requested_w.min(max_w), requested_h.min(max_h))
    } else {
        (monitor.physical_width, monitor.physical_height)
    };
    if (w.physical_width(), w.physical_height()) != target {
        w.resolution.set_physical_resolution(target.0, target.1);
        if w.mode == WindowMode::Windowed {
            w.position = WindowPosition::Centered(MonitorSelection::Current);
        }
        fit.frames = 0;
    } else {
        fit.frames += 1;
    }
    // Winit restores previous bounds asynchronously after leaving fullscreen.
    // Keep the requested size until several frames confirm the transition settled.
    fit.active = fit.frames < 5;
}

/// The window's size dragged by the player stays in the settings, saved
/// once its edges have been still for half a second: the next start and the
/// return from full screen give it back.
fn remember_window(
    // Missing without the window plugin, as in the tests.
    resized: Option<MessageReader<WindowResized>>,
    fit: Option<Res<WindowFit>>,
    mut menu: ResMut<MenuState>,
    window: Single<&Window, With<PrimaryWindow>>,
    time: Option<Res<Time<Real>>>,
    mut pending: Local<Option<f32>>,
) {
    let now = time.map_or(f32::MAX, |t| t.elapsed_secs());
    let moved = resized.is_some_and(|mut r| r.read().count() > 0);
    let fitting = fit.is_some_and(|f| f.active);
    if moved && !fitting && window.mode == WindowMode::Windowed {
        let size = [window.physical_width(), window.physical_height()];
        if size.iter().all(|&v| v > 0) && size != menu.current_settings().window {
            menu.settings.window = size;
            menu.draft.window = size;
            menu.dirty = true;
            *pending = Some(now + 0.5);
        }
    }
    if pending.is_some_and(|t| now >= t) {
        *pending = None;
        let path = menu.root.join("settings.json");
        if let Err(e) = menu.settings.save(&path) {
            menu.notice = e.to_string();
        }
    }
}

/// Wait before the next frame and the deadline after it. Deadlines advance
/// by whole periods, so oversleeping never accumulates into a lower rate;
/// a missed slot (heavy frame, limit raised) restarts from now.
fn frame_wait(deadline: Instant, now: Instant, fps: u32) -> (Duration, Instant) {
    if fps == 0 {
        return (Duration::ZERO, now);
    }
    let next = deadline + Duration::from_secs_f64(1. / fps as f64);
    if next > now {
        (next - now, next)
    } else {
        (Duration::ZERO, now)
    }
}

fn pace_frame(menu: Res<MenuState>, mut pacer: ResMut<FramePacer>) {
    let fps = menu.current_settings().frame_cap();
    let (wait, deadline) = frame_wait(pacer.deadline, Instant::now(), fps);
    if !wait.is_zero() {
        std::thread::sleep(wait);
    }
    pacer.deadline = deadline;
}

#[allow(clippy::too_many_arguments)]
fn present(
    game: Res<Game>,
    mut menu: ResMut<MenuState>,
    mode: Res<State<Mode>>,
    loaded: Res<Loaded>,
    ui: Res<MissionUi>,
    view: Res<MapView>,
    entities: Res<UiEntities>,
    mut presented: ResMut<Presented>,
    mut hd: ResMut<hd_text::HdText>,
    mut images: ResMut<Assets<Image>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cameras: Query<
        (&mut Camera, &mut Projection, &mut Transform, Has<MapCamera>),
        Or<(With<MapCamera>, With<UiCamera>)>,
    >,
    mut items: Query<(&mut Visibility, &mut Transform, &mut Sprite), Without<Camera>>,
) {
    let surface = Surface::of(&window);
    let scene = loaded.scene.is_some();
    let playing = *mode.get() == Mode::Mission && scene;
    // Pause, save and load dialogs are drawn over the map and its panel.
    let overlay = scene && !playing && menu.over_map();
    let mission = playing || overlay;
    // The draft while the settings are open: a preview.
    let shown = menu.current_settings().clone();
    hd.menu.shown = !playing && shown.hd_text;
    hd.hud.shown = mission && shown.hd_text;
    let canvas = canvas_layout(surface, &menu);
    let layout = if mission {
        hud_layout(surface, &shown)
    } else {
        canvas
    };
    for (mut camera, mut projection, mut transform, is_map) in &mut cameras {
        if is_map {
            camera.is_active = loaded.scene.is_some();
            let at = view.snapped(surface).extend(transform.translation.z);
            transform.set_if_neq(Transform::from_translation(at));
            if let Projection::Orthographic(p) = &mut *projection
                && p.scale != view.scale
            {
                p.scale = view.scale;
            }
        } else {
            // The map camera clears the frame when a mission is open.
            let keep = loaded.scene.is_some();
            if keep != matches!(camera.clear_color, ClearColorConfig::None) {
                camera.clear_color = if keep {
                    ClearColorConfig::None
                } else {
                    ClearColorConfig::Default
                };
            }
            let scale = layout.camera_scale(surface);
            if let Projection::Orthographic(p) = &mut *projection
                && p.scale != scale
            {
                p.scale = scale;
            }
        }
    }
    let show = |on: bool| {
        if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    for (entity, visible) in [
        (entities.canvas, !playing),
        (entities.hud, mission),
        // The film framing hides the minimap with the panels (0x4f9990).
        (
            entities.minimap,
            mission && ui.hud.frame() == retzar::hud::MapFrame::Panels,
        ),
    ]
    .into_iter()
    .chain(entities.viewport.iter().map(|&e| {
        (
            e,
            mission && ui.hud.frame() == retzar::hud::MapFrame::Panels,
        )
    })) {
        if let Ok((mut v, _, _)) = items.get_mut(entity) {
            v.set_if_neq(show(visible));
        }
    }
    let key = Some((surface.physical, layout.crisp));
    // The menu and the mission interface share one dirty flag; switching
    // view forces the hidden one to redraw on its next appearance.
    let dirty = menu.dirty;
    if !playing {
        if !mission {
            presented.hud_key = None;
        }
        let canvas_key = Some((surface.physical, canvas.crisp));
        if dirty || presented.menu_key != canvas_key {
            // «Testi in alta definizione»: the texts come out of the picture.
            let bitmap = if shown.hd_text {
                let [w, h] = [view::DESIGN.x as usize, view::DESIGN.y as usize];
                let (mut b, rec) = retzar::font::record(w, h, || game.skin.render(&menu));
                hd.menu.ops = hd_text::resolve(&mut b, rec, || game.skin.render(&menu));
                b
            } else {
                hd.menu.ops.clear();
                game.skin.render(&menu)
            };
            if let Ok((_, _, mut sprite)) = items.get_mut(entities.canvas) {
                renew_image(&mut images, &mut sprite, ui_image(bitmap, canvas.crisp));
            }
            presented.menu_key = canvas_key;
            menu.dirty = false;
        }
        if let Ok((_, mut t, _)) = items.get_mut(entities.canvas) {
            let at = canvas.place(surface, Vec2::ZERO, view::DESIGN);
            let z = if overlay { 2. } else { 0. };
            t.set_if_neq(Transform::from_translation(at.extend(z)));
            hd.menu.layout = Some(canvas);
            hd.menu.z = z;
        }
    } else {
        presented.menu_key = None;
    }
    if !mission {
    } else if let Some(s) = &loaded.scene {
        let left = shown.layout == 1;
        let size = (layout.size.x as i32, layout.size.y as i32, left);
        if dirty || presented.hud_key != key {
            if presented
                .hud_background
                .as_ref()
                .is_none_or(|(k, _)| *k != size)
            {
                let base = game.classic.background(size.0, size.1, left);
                presented.hud_background = Some((size, base));
            }
            let base = presented.hud_background.as_ref().unwrap().1.clone();
            // While the cheat panel waits for a click on the map, the line at
            // the bottom says what it waits for.
            let waiting =
                hud::cheat_panel::waiting(&game.classic, &s.library, ui.spawning, ui.pointing);
            let record = shown.hd_text;
            let draw = |base| {
                hud::render(
                    &game.skin,
                    &game.classic,
                    base,
                    hud::HudView {
                        width: size.0,
                        height: size.1,
                        left,
                        title: &s.title,
                        description: &s.description,
                        state: &ui.hud,
                        mission: &s.mission,
                        library: &s.library,
                        camera: [view.center.x, view.center.y, view.scale],
                        side: s.side,
                        pixels: [surface.physical.x, surface.physical.y],
                        portraits: &s.portraits,
                        messages: &ui.messages,
                        status: ui
                            .status
                            .as_ref()
                            .map_or(waiting.as_str(), |(s, _)| s.as_str()),
                        player_name: &shown.player_name,
                        selection: s.selection_color,
                        map_text: shown.map_text,
                    },
                )
            };
            let bitmap = if record {
                let (w, h) = (size.0 as usize, size.1 as usize);
                let (mut b, rec) = retzar::font::record(w, h, || draw(base));
                let again = || draw(presented.hud_background.as_ref().unwrap().1.clone());
                hd.hud.ops = hd_text::resolve(&mut b, rec, again);
                b
            } else {
                hd.hud.ops.clear();
                draw(base)
            };
            if let Ok((_, _, mut sprite)) = items.get_mut(entities.hud) {
                renew_image(&mut images, &mut sprite, ui_image(bitmap, layout.crisp));
            }
            presented.hud_key = key;
            menu.dirty = false;
        }
        if let Ok((_, mut t, _)) = items.get_mut(entities.hud) {
            let at = layout.place(surface, Vec2::ZERO, layout.size);
            t.set_if_neq(Transform::from_translation(at.extend(1.)));
            hd.hud.layout = Some(layout);
            hd.hud.z = 1.;
        }
    }
}

/// HWCURSOR selects the original H_DEF/S_DEF artwork. Cursor pixels and
/// hotspot belong to the screen, independent of the menu and map scale.
fn update_cursor(
    mut commands: Commands,
    game: Res<Game>,
    menu: Res<MenuState>,
    window: Single<(Entity, &Window), With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    indicators: Option<Res<orders::Indicators>>,
    mut cache: Local<Vec<(String, Handle<Image>, (u16, u16))>>,
    mut current: Local<String>,
) {
    let (entity, _) = *window;
    // The original keeps its arrow over buttons too; the hand (`cs_pick`)
    // only picks up items.
    let system = menu.current_settings().system_cursor;
    let shape = indicators.as_ref().map_or("", |s| s.cursor);
    // HWCURSOR: the shapes of the hardware set, `ch_*` of `h_*.bmp`, as
    // H_DEF for the arrow (0x43ab31); the software set is `cs_*`.
    let hardware = shape
        .strip_prefix("cs_")
        .filter(|_| system)
        .map(|s| format!("ch_{s}"));
    let cursor = hardware.as_deref().unwrap_or(shape);
    let key = if cursor.is_empty() {
        if system { "hardware" } else { "arrow" }
    } else {
        cursor
    };
    if current.as_str() == key {
        return;
    }
    let (handle, hotspot) = match cache.iter().find(|(k, _, _)| k == key) {
        Some((_, h, hot)) => (h.clone(), *hot),
        None => {
            let (art, hot) = if !cursor.is_empty() {
                let ini = retzar::assets::Ini::parse(
                    &game.assets.get("DATA/IMAGES.INI").unwrap_or_default(),
                );
                // A shape the hardware set lacks keeps the software one.
                let cursor = if ini.get(cursor, "image").is_some() {
                    cursor
                } else {
                    shape
                };
                let file = format!(
                    "IMAGES/{}",
                    ini.get(cursor, "image")
                        .unwrap_or_default()
                        .replace('\\', "/")
                );
                let Some(art) = game
                    .assets
                    .get(&file)
                    .ok()
                    .and_then(|bytes| retzar::bitmap::Bitmap::decode(&bytes, true).ok())
                else {
                    return;
                };
                let hot = ini
                    .get(cursor, "hotspot")
                    .unwrap_or("0,0")
                    .split(',')
                    .filter_map(|v| v.trim().parse::<u16>().ok())
                    .collect::<Vec<_>>();
                (art, (*hot.first().unwrap_or(&0), *hot.get(1).unwrap_or(&0)))
            } else if system {
                (game.classic.hardware_cursor.clone(), (2, 0))
            } else {
                (game.classic.cursor.clone(), (2, 0))
            };
            let h = images.add(rgba_image(art));
            cache.push((key.into(), h.clone(), hot));
            (h, hot)
        }
    };
    commands
        .entity(entity)
        .insert(CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
            handle,
            hotspot,
            ..default()
        })));
    *current = key.into();
}

/// Whether the voices of units (PLAYUNITSOUNDS) or of buildings
/// (PLAYBUILDINGSOUNDS) are on; the other channels always are.
fn heard(settings: &retzar::menu::Settings, channel: retzar::audio::Channel) -> bool {
    match channel {
        retzar::audio::Channel::Unit => settings.speech,
        retzar::audio::Channel::Building => settings.building_sounds,
        _ => true,
    }
}
/// An object's voice (vt+0x1c8 = 0x505710 without its effect flag): in
/// the voices of buildings or of units by its kind, none while it already
/// speaks (0x505720, unless just silenced) or under the fog (0x505610);
/// otherwise its clip is drawn from the stream of sounds and pictures
/// (0x455210), even when that kind of voice is off.
#[allow(clippy::too_many_arguments)]
fn voice(
    sim: &mut retzar::simulation::Simulation,
    lib: &retzar::world::ObjectLibrary,
    player: &Player,
    i: usize,
    o: &retzar::world::MapObject,
    property: &str,
    category: u8,
    silenced: bool,
) -> Option<retzar::audio::Cue> {
    let channel = if o.building() {
        retzar::audio::Channel::Building
    } else if retzar::world::class::is_unit(o.class) {
        retzar::audio::Channel::Unit
    } else {
        return None;
    };
    if (!silenced && player.speaking(i)) || !sim.visible(o) {
        return None;
    }
    let candidates = lib.sounds_for(o, property);
    if candidates.is_empty() {
        return None;
    }
    let roll = sim.gameplay.aux_rng.range(0, 1000);
    Some(retzar::audio::Cue {
        name: retzar::audio::pick(&candidates, roll)?.to_owned(),
        category,
        channel,
        place: None,
        owner: Some(i),
        looped: false,
    })
}

#[allow(clippy::too_many_arguments)]
fn sync_audio(
    mut menu: ResMut<MenuState>,
    mut loaded: ResMut<Loaded>,
    view: Res<MapView>,
    ui: Res<MissionUi>,
    mode: Res<State<Mode>>,
    movie: Res<movie::Movie>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut audio: ResMut<SoundPlayer>,
    time: Res<Time<Real>>,
) {
    let s = menu.current_settings().clone();
    let generation = loaded.generation;
    let scene = loaded
        .scene
        .as_ref()
        .map(|scene| (scene.biome.clone(), generation));
    let state = SoundState {
        sound: s.sound_volume(),
        music: s.music_volume(),
        ambience: s.ambience,
        // Silent in the background, as the original's DirectSound buffers.
        focused: !movie.active() && window.focused,
        movie: movie.active(),
        story: ui
            .narration
            .as_ref()
            .filter(|_| *mode.get() == Mode::Mission)
            .map(|n| n.id),
        music_on: s.music_on,
        options: scene.is_some() && *mode.get() == Mode::Menu && menu.screen.is_settings(),
        scene,
    };
    if audio.last.as_ref() != Some(&state) {
        audio.player.state(state.clone());
        audio.last = Some(state);
    }
    if audio.generation != generation {
        audio.generation = generation;
        audio.ambience = retzar::audio::Scheduler::default();
        audio.heard = None;
    }
    let mission = *mode.get() == Mode::Mission;
    if let Some(scene) = loaded.scene.as_mut().filter(|_| mission)
        && let Some(sim) = scene.mission.simulation.as_mut()
    {
        // 0x455680: habitat and time of day of the explored centre cell.
        let x = (view.center.x / 32.)
            .floor()
            .clamp(0., (scene.grid.side - 1) as f32) as usize;
        let y = (-view.center.y / 32.)
            .floor()
            .clamp(0., (scene.grid.side - 1) as f32) as usize;
        let nature = sim.fog.at(x, y).0.then(|| {
            let c = scene.grid.cells[y * scene.grid.side + x];
            let habitat = if c & 0x400 == 0 && (c >> 4 & 7 == 0 || c >> 7 & 7 == 0) {
                retzar::audio::Habitat::Marine
            } else if c >> 11 != 0 {
                retzar::audio::Habitat::Forest
            } else {
                retzar::audio::Habitat::Plains
            };
            retzar::audio::NatureContext {
                habitat,
                night: s.day_night && sim.night_amount() >= 0.5,
            }
        });
        let moment = retzar::audio::Moment {
            now: time.elapsed().as_millis() as u64,
            paused: ui.hud.panel.is_some() || !window.focused,
            nature,
            raining: s.weather && sim.weather == retzar::simulation::Weather::Rain,
            biome: &scene.biome,
        };
        let SoundPlayer {
            player, ambience, ..
        } = &mut *audio;
        let clips = ambience.tick(&moment, &mut sim.gameplay.aux_rng, &player.nature, |n| {
            player.rain_ms.get(n).copied().unwrap_or(0)
        });
        for name in clips {
            player.play(retzar::audio::Cue {
                name,
                category: 0,
                channel: retzar::audio::Channel::Nature,
                place: None,
                owner: None,
                looped: false,
            });
        }
    }
    let selection = (generation, ui.hud.selection_event, ui.hud.selected.clone());
    if audio.last_selection.as_ref() != Some(&selection) {
        // 0x4535e0, 0x4532dc: the first object selected says its line.
        if mission
            && let Some(scene) = loaded.scene.as_mut()
            && let Some(&first) = ui.hud.selected.first()
            && let Some(o) = scene.mission.objects.get(first)
            // Enemy units give no answer.
            && (o.building() || o.owner == scene.mission.player())
            && let Some(sim) = scene.mission.simulation.as_mut()
            && let Some(cue) = voice(
                sim,
                &scene.library,
                &audio.player,
                first,
                o,
                "sound_select",
                retzar::audio::category::SELECT,
                false,
            )
            && heard(&s, cue.channel)
        {
            let (centre, width) = audio.hearing;
            audio.player.play(retzar::audio::Cue {
                place: Some(retzar::audio::Place {
                    at: [o.x as i32, o.y as i32],
                    centre,
                    width,
                }),
                ..cue
            });
        }
        audio.last_selection = Some(selection);
    }
    // Status texts are shown only on the settings pages.
    if !menu.screen.is_settings() {
        return;
    }
    let message = match audio.player.status.lock() {
        Ok(status) if !status.errors.is_empty() => {
            retzar::lang::t("Audio non disponibile").to_owned()
        }
        // Working audio needs no line on the page.
        Ok(status) if !status.pcm_backend.is_empty() && !status.midi_backend.is_empty() => {
            String::new()
        }
        _ => "Inizializzazione dell'audio…".into(),
    };
    if menu.audio_status != message {
        menu.audio_status = message;
        menu.dirty = true;
    }
}

/// F11 saves the screen in `Screenshots`, as in the original; Alt+Enter, the
/// shortcut of Windows, switches between full screen and window.
fn window_shortcuts(
    mut commands: Commands,
    mut ui: ResMut<MissionUi>,
    mut chords: ResMut<input::KeyboardChordFrame>,
    mut menu: ResMut<MenuState>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    if std::mem::take(&mut ui.screenshot) {
        let dir = menu.root.join("Screenshots");
        let number = std::fs::read_dir(&dir).map_or(0, |d| d.count()) + 1;
        let path = dir.join(format!("ReTzar{number:03}.png"));
        // 0x4f6140: the original says something only when it fails.
        if std::fs::create_dir_all(&dir).is_ok() {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
        } else {
            menu.notice = "Fail to create shots folder!".into();
            menu.dirty = true;
        }
    }
    if std::mem::take(&mut chords.alt_enter) {
        let fullscreen = !menu.current_settings().fullscreen;
        menu.settings.fullscreen = fullscreen;
        menu.draft.fullscreen = fullscreen;
        window_settings(&mut window, menu.current_settings());
        let _ = menu.settings.save(&menu.root.join("settings.json"));
    }
}

/// Take one screenshot once loading has settled, then quit.
fn capture(
    mut commands: Commands,
    mut capture: ResMut<Capture>,
    job: Option<Res<LoadJob>>,
    mut exit: MessageWriter<AppExit>,
) {
    if job.is_some() {
        return;
    }
    capture.frames += 1;
    match capture.taken_at {
        None if capture.frames >= 90 => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(capture.path.clone()));
            capture.taken_at = Some(capture.frames);
        }
        Some(at) if capture.frames >= at + 30 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
