//! Map navigation, selection, targeting and player command input.
use super::{
    Game, MenuState, Mode, PendingAction, UiEntities,
    input::{self, GROUP_KEYS, KeyboardChordFrame, PointerFrame, TypedKeys},
    rgba_image,
    scene::Loaded,
    view::{
        MapView, Surface, UiLayout, glide_levels, glide_step, hud_layout, opening_zoom,
        zoom_levels, zoom_step,
    },
};
use bevy::{
    camera::visibility::RenderLayers,
    image::ImageSampler,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    },
    prelude::*,
    window::PrimaryWindow,
};
use retzar::{
    bitmap::Bitmap,
    dialogs::{Control, Dialog, Kind, Outcome},
    font::BitmapFont,
    hud::{self, HudAction, HudState, Panel, Recipients, diplomacy},
    menu::{Action, Screen, Settings},
    selection,
    world::{FLAG_DYING, FLAG_INACTIVE, Layer, MissionWorld, class},
};
use std::time::Instant;

/// Interface state of an open mission.
#[derive(Resource, Default)]
pub struct MissionUi {
    pub target_order: Option<String>,
    pub requests: Vec<retzar::gameplay::Request>,
    /// Chat lines of the AI command (0x404880), for the next step.
    pub said: Vec<String>,
    pub hud: HudState,
    pub groups: [Vec<usize>; 9],
    /// Map position where a left-button selection started.
    pub select_start: Option<Vec2>,
    /// The drag from `select_start` became a rectangle (0x4f49a3), drawn
    /// from then on.
    pub boxing: bool,
    /// Logical press position/time for double click recognition.
    pub click_start: Option<(Vec2, Instant)>,
    pub last_click: Option<(usize, Vec2, Instant)>,
    /// Last cursor position of a middle-button drag.
    pub drag: Option<Vec2>,
    pub hud_scroll_grab: Option<f32>,
    /// The conversation bubble held by the right button and where the
    /// pointer took it, from its corner (+0x206, +0x20a).
    pub say_drag: Option<(usize, [i32; 2])>,
    /// The views kept with Ctrl + F5–F8 (+0x16cc of the game screen).
    pub bookmarks: [Option<Vec2>; 4],
    /// Fractional wheel movement (touchpads report pixels, not notches).
    pub wheel: f32,
    /// «Zoom fluido» (an option of this version): the scale the view glides
    /// to, the screen point it keeps still and the scale last set.
    pub glide: Option<(f32, Option<Vec2>, f32)>,
    /// The message list at the top of the map (0x4f8a50): up to five
    /// lines, oldest first, and when it last changed (+0x1672).
    pub messages: Vec<String>,
    pub messages_at: f32,
    /// The menu's line last taken into the list.
    pub posted: String,
    /// The status line at the bottom of the map (0x4f8350) and when it
    /// goes.
    pub status: Option<(String, f32)>,
    /// The selection the command page belongs to: a new one starts at page 1.
    pub paged: Vec<usize>,
    /// Last peon shown by «Cerca contadino disoccupato».
    pub idle: usize,
    pub order_feedback: Option<super::orders::Feedback>,
    /// Stages of a Shift route, ordered when Shift is released.
    pub waypoints: Vec<retzar::gameplay::Request>,
    /// Shift held this frame: a create button then queues five (0x4a0e6a).
    pub shift: bool,
    /// Position in the event list visited with Home, PgUp and PgDn.
    pub event_cursor: usize,
    /// Trees drawn see-through (`\` key of the original).
    pub transparent_trees: bool,
    /// A game speed key waiting for the game (0x4f5862–0x4f5985).
    pub speed_key: Option<retzar::simulation::SpeedKey>,
    /// F11, carried out by the window system.
    pub screenshot: bool,
    /// The phrase chosen in a conversation, for the script.
    pub answer: Option<usize>,
    /// The story screen running (0x4c3cc0).
    pub narration: Option<NarrationRun>,
    /// Story screens opened so far, to tell one from the next.
    pub stories: u32,
    /// A bubble's eye pressed: the view goes to its speaker (0x4383c0).
    pub look: Option<usize>,
    /// The voices switched off: the one being said stops (0x4c06c1).
    pub hush: bool,
    /// The end screen was opened.
    pub ended: bool,
    /// The cell a wall row is being dragged from (0x4fb0be).
    pub wall_from: Option<[u16; 2]>,
    /// The scripts' hold on the view: LockView and ScrollView.
    pub view_lock: Option<ViewLock>,
    pub view_scroll: Option<ViewScroll>,
    /// The cheat panel's «spawn» waiting for its click: class,
    /// civilization and kingdom (the original's object at the pointer,
    /// 0x4f3250).
    pub spawning: Option<(usize, u8, u8)>,
    /// A command of the cheat panel's «Oggetti» page waiting for the
    /// object to click.
    pub pointing: Option<hud::cheat_panel::Target>,
    /// The cheat panel's «next», for the game screen to carry out.
    pub next_map: bool,
}
/// A story screen on (0x4c34c0): when it opened, in milliseconds of real
/// time (GetTickCount, +0x64), its time and whether it then closes (+0x5c,
/// +0x60), and its voice, said once 100 ms after it opens (0x4c3e08).
#[derive(Clone, Debug)]
pub struct NarrationRun {
    /// Its number among the stories opened, for the music.
    pub id: u32,
    pub start: u64,
    pub ms: u64,
    pub close: bool,
    pub voice: String,
    pub said: bool,
}
/// LockView (0x4f9140): the object the view follows and when the lock ends,
/// in seconds of real time; the player neither scrolls nor selects meanwhile.
#[derive(Clone, Copy, Debug)]
pub struct ViewLock {
    pub follow: Option<usize>,
    pub until: Option<f32>,
}
/// ScrollView (0x4f6d00): from one cell's corner to another's, in map
/// pixels (y down).
#[derive(Clone, Copy, Debug)]
pub struct ViewScroll {
    from: [i32; 2],
    to: [i32; 2],
    /// Pixels a second along each axis: 1024 times the sine and the cosine
    /// of the angle `fpatan` gives for (dx, dy), truncated.
    speed: [i32; 2],
    /// Distance × 1000 / 1024 milliseconds.
    duration_ms: u32,
    start: f32,
}
impl ViewScroll {
    pub fn new(from: [u16; 2], to: [u16; 2], now: f32) -> Self {
        let [from, to] = [from, to].map(|c| c.map(|v| v as i32 * 32));
        let [dx, dy] = [to[0] - from[0], to[1] - from[1]];
        let angle = (dx as f64).atan2(dy as f64);
        let distance = ((dx as f64).powi(2) + (dy as f64).powi(2)).sqrt() as u32;
        Self {
            from,
            to,
            speed: [angle.sin(), angle.cos()].map(|v| (v * 1024.) as i32),
            duration_ms: distance * 1000 >> 10,
            start: now,
        }
    }
    /// Where the view stands at `now` (scene coordinates, y up), moved at
    /// each 75 ms step of the timer, and whether the glide is over.
    pub fn at(&self, now: f32) -> (Vec2, bool) {
        let elapsed = ((now - self.start).max(0.) * 1000.) as u32;
        let point = |[x, y]: [i32; 2]| Vec2::new(x as f32, -(y as f32));
        if elapsed >= self.duration_ms {
            return (point(self.to), true);
        }
        let t = (elapsed / 75 * 75) as i32;
        let at = [0, 1].map(|k| self.from[k] + self.speed[k] * t / 1000);
        (point(at), false)
    }
}
/// A line leaves the message list 7500 ms after the list last changed
/// (0x4f3e7f), the oldest first.
const MESSAGE_SECONDS: f32 = 7.5;
/// Lines the message list holds (+0x1670 up to 4, 0x4f8aec).
const MESSAGE_LINES: usize = 5;

/// A line at the bottom of the message list (0x4f8a50); a sixth pushes
/// the oldest out.
pub(super) fn post(ui: &mut MissionUi, text: String, now: f32) {
    if text.is_empty() {
        return;
    }
    if ui.messages.len() >= MESSAGE_LINES {
        ui.messages.remove(0);
    }
    ui.messages.push(text);
    ui.messages_at = now;
}
/// The original draws its rectangle past 8 map pixels and selects with it
/// past 16 (0x4f4998, 0x4f4ef8); selection is this version's modern part,
/// and a drag of a few pixels is already a box.
const BOX_PIXELS: f32 = 4.;
/// The cells of the wall row dragged from `from` to the cursor's cell, and
/// whether each can take a segment (0x4fb23f).
pub(super) fn wall_preview(
    scene: &super::scene::Scene,
    game: &Game,
    order: &str,
    selected: &[usize],
    from: [u16; 2],
    to: [u16; 2],
    alt: bool,
) -> Vec<([u16; 2], bool)> {
    let builder = selected.first().and_then(|&i| scene.mission.objects.get(i));
    retzar::gameplay::wall_row(from, to, alt, builder.map(|b| [b.x, b.y]))
        .into_iter()
        .map(|p| {
            let ok = builder
                .and_then(|w| {
                    retzar::gameplay::building_at(&scene.library, &game.gameplay, w, order, p)
                })
                .is_some_and(|probe| {
                    retzar::gameplay::can_place(&scene.grid, &scene.mission, &scene.library, &probe)
                });
            (p, ok)
        })
        .collect()
}
/// A click on an occupant of the inventory panel (0x4a1830). A unit leaves
/// its holder whatever the button (0x490ad0; with Ctrl it is selected). An
/// item: Alt marks it active (0x463140), otherwise [`item_click`].
fn content_click(
    ui: &mut MissionUi,
    scene: &super::scene::Scene,
    j: usize,
    right: bool,
    ctrl: bool,
    alt: bool,
) {
    let Some(holder) = ui.hud.contents_owner else {
        return;
    };
    let Some(o) = scene.mission.objects.get(j) else {
        return;
    };
    if !o.item() {
        ui.requests.push(retzar::gameplay::Request {
            order: "leave".into(),
            selected: vec![holder],
            target: Some(j),
            ctrl,
            ..Default::default()
        });
        return;
    }
    // 0x4a1ba8: Alt marks the active item (0x463140), which Backspace uses.
    if alt {
        ui.requests.push(retzar::gameplay::Request {
            order: "item_mark".into(),
            selected: vec![j],
            ..Default::default()
        });
        return;
    }
    item_click(ui, scene, j, right, ctrl);
}
/// An item clicked in the inventory, or the one Backspace takes (both
/// vt+0x2b4, 0x4a1c01 and 0x4f62e7): left uses it, with a target unless it
/// needs none (+0xc9: scrolls and equipment, 0x462630) or Ctrl asks for it
/// at once; right drops it, with a place unless Ctrl drops it at once
/// (0x4626e0). The soul gem is no different: its use is empty (vt+0x2bc =
/// 0x50b390), and its menu belongs to it only while held (0x463d70),
/// where no click of the interface selects it.
fn item_click(ui: &mut MissionUi, scene: &super::scene::Scene, j: usize, right: bool, ctrl: bool) {
    let Some(o) = scene.mission.objects.get(j) else {
        return;
    };
    let instant = matches!(
        o.class,
        retzar::world::class::ITEM_PLUSPLUS | retzar::world::class::ITEM_SCROLL
    );
    let order = if right { "item_drop" } else { "item_use" };
    if ctrl || (!right && instant) {
        ui.requests.push(retzar::gameplay::Request {
            order: if right { order } else { "item_use_instant" }.into(),
            selected: vec![j],
            ..Default::default()
        });
    } else {
        // The pointer chooses the target or the place (0x4f3250).
        ui.hud.inspected_content = Some(j);
        ui.target_order = Some(order.into());
    }
}
fn command_selection(ui: &MissionUi) -> Vec<usize> {
    ui.hud
        .inspected_content
        .map_or_else(|| ui.hud.selected.clone(), |i| vec![i])
}
/// The visible (or remembered, for buildings) object at a map point, found
/// as the original finds a click's (`selection::pointed`, 0x4e4280).
fn object_at(scene: &super::scene::Scene, world: Vec2) -> Option<usize> {
    let sim = scene.mission.simulation.as_ref();
    selection::pointed(
        &scene.mission,
        &scene.library,
        &scene.visuals,
        [world.x, world.y],
        |i, o| {
            // Rubble is never a target (no `selection`, 0x444aa0).
            o.class != retzar::world::class::DEBRIS
                && o.flags & (FLAG_INACTIVE | FLAG_DYING) == 0
                && sim.is_none_or(|s| {
                    s.visible(o)
                        || (!selection::unit(o) && s.known(o) && s.appearance(i, o).is_some())
                })
        },
    )
}
/// The orders that open a command dialog in the place of the grid
/// (0x4a0b79). The exchange and the workshop do not open while the kingdom
/// already has one running (K+0x126, K+0x12a).
fn open_dialog(order: &str, ui: &mut MissionUi, menu: &mut MenuState, loaded: &Loaded) {
    let Some(scene) = &loaded.scene else { return };
    let Some(&object) = command_selection(ui).first() else {
        return;
    };
    let mission = &scene.mission;
    let Some(sim) = mission.simulation.as_ref() else {
        return;
    };
    let owner = mission.objects.get(object).map_or(0, |o| o.owner);
    let dialog = match order {
        "gamble" => Dialog::gamble(object),
        "trade1" if retzar::gameplay::exchanging(sim, mission, owner) => return,
        "trade1" => Dialog::exchange(object, ui.hud.exchanges.get(&object).copied()),
        "build_item" if retzar::gameplay::crafting(sim, mission, owner) => return,
        "build_item" => Dialog::workshop(object, ui.hud.workshop),
        _ => {
            // D_TRADE shows the unit's current route (0x4a2310).
            let a = sim.gameplay.actors.get(object);
            let route = a.and_then(|a| a.route.as_ref());
            Dialog::route(
                object,
                [
                    a.and_then(|a| a.trade_origin),
                    a.and_then(|a| a.trade_partner),
                ],
                route.map_or([0, 0], |r| r.resources),
                route.map(|r| r.amount),
            )
        }
    };
    ui.target_order = None;
    ui.hud.dialog = Some(dialog);
    menu.dirty = true;
}
/// A left or right click on a control of the command dialog.
fn dialog_click(
    ui: &mut MissionUi,
    menu: &mut MenuState,
    scene: &super::scene::Scene,
    game: &Game,
    control: Control,
    right: bool,
    shift: bool,
) {
    let mission = &scene.mission;
    let available = retzar::gameplay::stock(mission, mission.player());
    let Some(d) = ui.hud.dialog.as_mut() else {
        return;
    };
    let o = mission.objects.get(d.object);
    let cap = o.map_or(0, retzar::gameplay::trade_capacity);
    let minimum = |spell| {
        o.map_or(0, |o| {
            retzar::gameplay::book_minimum(&game.gameplay, o, spell)
        })
    };
    let outcome = d.click(control, right, shift, available, cap, minimum);
    let (kind, object, choice) = (d.kind, d.object, (d.resource, d.resource2, d.amount));
    match outcome {
        Outcome::Nothing => {}
        Outcome::Close => ui.hud.close_dialog(),
        Outcome::Message(text) => menu.notice = text,
        Outcome::Send(req, close) => {
            if kind == Kind::Exchange {
                ui.hud.exchanges.insert(object, choice);
            }
            ui.requests.push(req);
            if close {
                ui.hud.close_dialog();
            }
        }
    }
    menu.dirty = true;
}

/// Whether a newly opened mission starts on its story window (the visual tour
/// turns it off after its first mission).
#[derive(Resource)]
pub struct StoryOnOpen(pub bool);
impl Default for StoryOnOpen {
    fn default() -> Self {
        Self(true)
    }
}

/// Visible part of the window available to the map, in logical pixels.
fn play_area(
    surface: Surface,
    hud: &UiLayout,
    settings: &Settings,
    frame: hud::MapFrame,
) -> (Vec2, Vec2) {
    let r = hud::play_rect(
        hud.size.x as i32,
        hud.size.y as i32,
        settings.layout == 1,
        frame,
    );
    let to_logical = hud.factor / surface.dpi;
    (
        Vec2::new(r.x as f32, r.y as f32) * to_logical,
        Vec2::new((r.x + r.w) as f32, (r.y + r.h) as f32) * to_logical,
    )
}

/// Put map point `center` in the middle of the visible map area.
pub fn center_on(
    view: &mut MapView,
    surface: Surface,
    hud: &UiLayout,
    settings: &Settings,
    center: Vec2,
) {
    let (min, max) = play_area(surface, hud, settings, view.frame);
    let logical = surface.physical / surface.dpi;
    let play_center = (min + max) / 2.;
    view.center = Vec2::new(
        center.x - (play_center.x - logical.x / 2.) * view.scale,
        center.y - (logical.y / 2. - play_center.y) * view.scale,
    );
}

/// Show the whole map inside the visible area.
pub fn fit(view: &mut MapView, surface: Surface, hud: &UiLayout, settings: &Settings, side: usize) {
    let edge = side as f32 * 32.;
    let (min, max) = play_area(surface, hud, settings, view.frame);
    let size = (max - min).max(Vec2::ONE);
    view.scale = (edge / size.x).max(edge / size.y) * 1.05;
    center_on(
        view,
        surface,
        hud,
        settings,
        Vec2::new(edge / 2., -edge / 2.),
    );
}

/// Opening view of a mission: its start position at the default zoom.
pub fn opening_view(
    view: &mut MapView,
    surface: Surface,
    settings: &Settings,
    side: usize,
    start: Option<[u16; 2]>,
) {
    let hud = hud_layout(surface, settings);
    // Without a start position the original framing opens on the centre.
    let cell = start.map_or([side as f32 / 2.; 2], |s| [s[0] as f32, s[1] as f32]);
    view.set_magnification(surface, opening_zoom(&hud));
    center_on(
        view,
        surface,
        &hud,
        settings,
        Vec2::new(cell[0] * 32., -cell[1] * 32.),
    );
    clamp(view, surface, &hud, settings, side);
}

/// Keep the map inside the visible area, centring it when it is smaller.
pub fn clamp(
    view: &mut MapView,
    surface: Surface,
    hud: &UiLayout,
    settings: &Settings,
    side: usize,
) {
    let edge = side as f32 * 32.;
    let (min, max) = play_area(surface, hud, settings, view.frame);
    let logical = surface.physical / surface.dpi;
    let left = (logical.x / 2. - min.x) * view.scale;
    let right = (max.x - logical.x / 2.) * view.scale;
    let up = (logical.y / 2. - min.y) * view.scale;
    let down = (max.y - logical.y / 2.) * view.scale;
    view.center.x = if left + right >= edge {
        edge / 2. + (left - right) / 2.
    } else {
        view.center.x.clamp(left, edge - right)
    };
    view.center.y = if up + down >= edge {
        -edge / 2. + (down - up) / 2.
    } else {
        view.center.y.clamp(-edge + down, -up)
    };
}

fn focus_selection(
    view: &mut MapView,
    surface: Surface,
    hud: &UiLayout,
    settings: &Settings,
    loaded: &Loaded,
    selected: &[usize],
) {
    let Some(scene) = &loaded.scene else { return };
    let mut center = Vec2::ZERO;
    let mut count = 0.;
    for v in scene
        .visuals
        .iter()
        .filter(|v| v.layer == Layer::Body && selected.contains(&v.object))
    {
        center += Vec2::new(
            v.footprint[0] + v.footprint[2] / 2.,
            v.footprint[1] + v.footprint[3] / 2.,
        );
        count += 1.;
    }
    if count > 0. {
        center_on(view, surface, hud, settings, center / count);
    }
}

const LETTERS: [KeyCode; 26] = [
    KeyCode::KeyA,
    KeyCode::KeyB,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyQ,
    KeyCode::KeyR,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyU,
    KeyCode::KeyV,
    KeyCode::KeyW,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
];
fn hotkey_pressed(keys: &ButtonInput<KeyCode>, hotkey: &str) -> bool {
    hotkey.len() == 1
        && hotkey.as_bytes()[0].is_ascii_alphabetic()
        && keys.just_pressed(LETTERS[(hotkey.as_bytes()[0].to_ascii_uppercase() - b'A') as usize])
}
fn open_help(menu: &mut MenuState, next: &mut NextState<Mode>) {
    menu.open(Screen::Help);
    next.set(Mode::Menu);
}

/// Command buttons that work without the simulation: the windows of the
/// ownership menu, the idle peon finder and the help. Orders say that they
/// wait for the game to run.
#[allow(clippy::too_many_arguments)]
fn run_command(
    command: &hud::Command,
    ui: &mut MissionUi,
    menu: &mut MenuState,
    next: &mut NextState<Mode>,
    view: &mut MapView,
    (surface, hud, settings): (Surface, &UiLayout, &Settings),
    loaded: &Loaded,
    game: &Game,
) {
    // A grey command does nothing; its hint tells what it needs.
    if command.unavailable.is_some() {
        return;
    }
    // Heroes, spies and items are lists in the grid, by their submenus
    // (`GET heroes`, …, 0x49f1fc).
    if command.order == "view_quests" {
        ui.hud.set_panel(Some(Panel::Quests));
        ui.hud.scroll = 0;
        scroll_panel(ui, loaded, game, 0);
        return;
    }
    if let Some(submenu) = &command.submenu {
        ui.hud.command_history.push(ui.hud.command_menu.take());
        ui.hud.command_menu = Some(submenu.clone());
        ui.hud.page = 0;
        menu.dirty = true;
        return;
    }
    match command.order.as_str() {
        "nextidle" => {
            let Some(s) = &loaded.scene else { return };
            if ui.view_lock.is_some() {
                return;
            }
            let player = s.mission.player();
            let peons: Vec<usize> = s
                .mission
                .objects
                .iter()
                .enumerate()
                .filter(|(i, o)| {
                    o.owner == player
                        && o.class == class::PEON
                        && o.selectable()
                        && s.mission
                            .simulation
                            .as_ref()
                            .and_then(|sim| sim.gameplay.actors.get(*i))
                            .is_none_or(|a| a.intent.is_none())
                })
                .map(|(i, _)| i)
                .collect();
            if peons.is_empty() {
                return;
            }
            ui.idle = (ui.idle + 1) % peons.len();
            let i = peons[ui.idle];
            ui.hud.selected = vec![i];
            focus_selection(view, surface, hud, settings, loaded, &[i]);
        }
        "onlinehelp" => open_help(menu, next),
        "gamble" | "trade1" | "build_item" | "trade" | "trade_ship" => {
            open_dialog(&command.order, ui, menu, loaded)
        }
        "select_crusaders" => {
            if ui.view_lock.is_some() {
                return;
            }
            // 0x439f60: the crusaders the selected anchor gathered.
            if let Some(scene) = &loaded.scene
                && let Some(sim) = scene.mission.simulation.as_ref()
            {
                let player = scene.mission.player();
                let gathered = sim
                    .gameplay
                    .treasury
                    .get(&player)
                    .and_then(|t| t.crusade.as_ref())
                    .map(|c| c.crusaders.clone())
                    .unwrap_or_default();
                ui.hud.selected = gathered
                    .into_iter()
                    .filter(|&i| {
                        scene
                            .mission
                            .objects
                            .get(i)
                            .is_some_and(|o| o.owner == player && o.selectable())
                    })
                    .take(50)
                    .collect();
                ui.hud.selection_changed();
                focus_selection(view, surface, hud, settings, loaded, &ui.hud.selected);
                menu.dirty = true;
            }
        }
        _ => {
            let item_target = command.order != "item_use"
                || ui
                    .hud
                    .inspected_content
                    .and_then(|i| loaded.scene.as_ref()?.mission.objects.get(i))
                    .is_some_and(|o| matches!(o.class, class::ITEM_MAGIC | class::ITEM_HEALTH));
            if game.gameplay.targeted(&command.order) && item_target {
                ui.target_order = Some(command.order.clone());
            } else {
                // Shift on a create button queues five (0x4a0e6a).
                let times =
                    if ui.shift && game.gameplay.action(&command.order).starts_with("create ") {
                        5
                    } else {
                        1
                    };
                for _ in 0..times {
                    ui.requests.push(retzar::gameplay::Request::command(
                        &command.order,
                        command_selection(ui),
                    ));
                }
            }
        }
    }
}

/// Delete (0x4f562f): the first selected object dies, every selected one with
/// Shift; the castle and the great buildings only with Shift; birds never.
fn delete_selection(ui: &mut MissionUi, scene: &super::scene::Scene, shift: bool) {
    let mission = &scene.mission;
    // `$candelobjects = 0` in a script (0x5cf6b0).
    if mission
        .simulation
        .as_ref()
        .is_some_and(|s| s.gameplay.script.no_delete)
    {
        return;
    }
    let chosen: Vec<usize> = ui
        .hud
        .selected
        .iter()
        .copied()
        .take(if shift { usize::MAX } else { 1 })
        .filter(|&i| {
            mission.objects.get(i).is_some_and(|o| {
                selection::controllable(o, mission)
                    && !matches!(o.class, class::BIRD | class::EAGLE)
                    && (shift || !matches!(o.class, 44 | 55 | 56 | 60 | 65 | 70 | 71))
            })
        })
        .collect();
    if !chosen.is_empty() {
        ui.requests
            .push(retzar::gameplay::Request::command("die", chosen));
    }
}

/// Ctrl + letter: the next own building of the classes listed for that
/// letter in `DATA\KEYS.INI` [Ctrl], in turn (0x4062f2 → 0x4061a0).
fn cycle_buildings(ui: &mut MissionUi, game: &Game, scene: &super::scene::Scene, letter: char) {
    // 0x453240 and 0x453340 do nothing while the view is locked
    // (0x589714).
    if ui.view_lock.is_some() {
        return;
    }
    let Ok(data) = game.assets.get("DATA/KEYS.INI") else {
        return;
    };
    let ini = retzar::assets::Ini::parse(&data);
    let Some(classes) = ini.get("ctrl", &letter.to_string()) else {
        return;
    };
    let names: Vec<String> = classes
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .collect();
    let mission = &scene.mission;
    let matches: Vec<usize> = mission
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| {
            o.owner == mission.player()
                && o.selectable()
                && names.iter().any(|n| n == o.class_name())
        })
        .map(|(i, _)| i)
        .collect();
    let current = ui.hud.selected.first().copied();
    let next = matches
        .iter()
        .copied()
        .find(|&i| current.is_some_and(|c| i > c))
        .or_else(|| matches.first().copied());
    if let Some(i) = next {
        ui.hud.selected = vec![i];
        ui.hud.selection_changed();
    }
}

pub fn scroll_panel(ui: &mut MissionUi, loaded: &Loaded, game: &Game, delta: i32) {
    let Some(s) = &loaded.scene else { return };
    let story = hud::story_text(&s.mission, &s.title, &s.description);
    let max = hud::scroll_limit(&game.classic, &ui.hud, &s.mission, &story);
    ui.hud.scroll = (ui.hud.scroll as i64 + delta as i64).clamp(0, max as i64) as usize;
    ui.hud.scroll_max = max;
}

/// Leaving the window releases every drag and pressed control.
pub fn release_on_blur(
    window: Single<&Window, With<PrimaryWindow>>,
    mut ui: ResMut<MissionUi>,
    mut chords: ResMut<KeyboardChordFrame>,
) {
    if !window.focused {
        if ui.drag.is_some()
            || ui.select_start.is_some()
            || ui.hud_scroll_grab.is_some()
            || ui.hud.press.is_some()
        {
            ui.drag = None;
            ui.select_start = None;
            ui.hud_scroll_grab = None;
            ui.hud.press = None;
        }
        if ui.hud.grab.is_some() {
            ui.hud.grab = None;
        }
        *chords = KeyboardChordFrame::default();
        ui.last_click = None;
        ui.click_start = None;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn mission_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    window: Single<&Window, With<PrimaryWindow>>,
    (frame, chords, typed_keys): (Res<PointerFrame>, Res<KeyboardChordFrame>, Res<TypedKeys>),
    (mut view, mut ui): (ResMut<MapView>, ResMut<MissionUi>),
    mut menu: ResMut<MenuState>,
    loaded: Res<Loaded>,
    game: Res<Game>,
    mut pending: ResMut<PendingAction>,
    mut next: ResMut<NextState<Mode>>,
    mut typed: MessageReader<KeyboardInput>,
) {
    let Some(scene) = &loaded.scene else { return };
    if !window.focused {
        typed.clear();
        return;
    }
    // The chat window takes the keyboard: Enter sends, Backspace deletes.
    let chatting = matches!(ui.hud.panel, Some(Panel::Chat(_)));
    // Alt+Enter belongs to the window (`input::logical_navigation`).
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    for e in typed.read() {
        if !chatting
            || ui.hud.help.is_some()
            || e.state != ButtonState::Pressed
            || alt && matches!(e.key_code, KeyCode::Enter | KeyCode::NumpadEnter)
        {
            continue;
        }
        match &e.logical_key {
            Key::Enter => {
                let text = std::mem::take(&mut ui.hud.chat.text);
                // 0x42572a: «AI …» is the AI command, not a message.
                if retzar::gameplay::ai::chat_command(&text) {
                    ui.said.push(text);
                } else if !text.trim().is_empty() {
                    let name = menu.current_settings().player_name.clone();
                    menu.notice = format!("{name}: {}", text.trim());
                }
                ui.hud.set_panel(None);
                menu.dirty = true;
            }
            Key::Backspace => {
                ui.hud.chat.text.pop();
                menu.dirty = true;
            }
            _ => {
                if let Some(t) = &e.text {
                    for c in t.chars().filter(|c| !c.is_control()) {
                        if ui.hud.chat.text.chars().count() < 200 {
                            ui.hud.chat.text.push(c);
                        }
                    }
                    menu.dirty = true;
                }
            }
        }
    }
    // A new selection shows its first page of commands.
    if ui.paged != ui.hud.selected {
        ui.paged = ui.hud.selected.clone();
        ui.hud.selection_changed();
    }
    let settings = menu.current_settings().clone();
    let surface = Surface::of(&window);
    let hud = hud_layout(surface, &settings);
    let side = scene.side;
    let now = time.elapsed_secs();
    // The lines of the interface go to the message list once each.
    if menu.notice != ui.posted {
        ui.posted = menu.notice.clone();
        let text = menu.notice.clone();
        post(&mut ui, text, now);
        menu.dirty = true;
    }
    if !ui.messages.is_empty() && now - ui.messages_at > MESSAGE_SECONDS {
        ui.messages.remove(0);
        ui.messages_at = now;
        if ui.messages.is_empty() {
            menu.notice.clear();
            ui.posted.clear();
        }
        menu.dirty = true;
    }
    if ui.status.as_ref().is_some_and(|(_, until)| now > *until) {
        ui.status = None;
        menu.dirty = true;
    }
    let left = settings.layout == 1;
    let (width, height) = (hud.size.x as i32, hud.size.y as i32);
    // The command dialog belongs to its selected object; the dice roll on.
    if let Some(d) = &ui.hud.dialog {
        let object = d.object;
        let owned = scene.mission.objects.get(object).is_some_and(|o| {
            o.flags & (FLAG_INACTIVE | FLAG_DYING) == 0 && o.owner == scene.mission.player()
        });
        if !owned || !command_selection(&ui).contains(&object) {
            ui.hud.close_dialog();
            menu.dirty = true;
        } else if let Some(d) = ui.hud.dialog.as_mut() {
            let available = retzar::gameplay::stock(&scene.mission, scene.mission.player());
            let before = (d.face, d.rolling);
            d.update(available, (time.delta_secs() * 1000.) as u32);
            if (d.face, d.rolling) != before {
                menu.dirty = true;
            }
        }
    }
    // The help ball's mode (0x45c340) takes the mouse and the keyboard: the
    // keys do nothing (0x4b01c0), the other controls and the map neither
    // light up nor answer; the right button anywhere or the left one on the
    // ball ends it (0x45c600), and here Escape too, as in the menus.
    if ui.hud.help.is_some() {
        let layout = hud::Layout {
            width,
            height,
            left,
        };
        let point = window.cursor_position().map(|c| hud.point(surface, c));
        if let Some(p) = point {
            let at = [p.x.floor() as i32, p.y.floor() as i32];
            if ui.hud.help != Some(at) {
                ui.hud.help = Some(at);
                menu.dirty = true;
            }
        }
        let on_ball = point.is_some_and(|p| {
            hud::widgets(
                &game.classic,
                &game.skin.locale,
                &ui.hud,
                layout,
                &scene.mission,
                &scene.library,
            )
            .iter()
            .any(|b| b.action == HudAction::Help && b.rect.has(p.x, p.y))
        });
        if mouse.just_pressed(MouseButton::Right)
            || keys.just_pressed(KeyCode::Escape)
            || (mouse.just_pressed(MouseButton::Left) && on_ball)
        {
            ui.hud.help = None;
            menu.dirty = true;
        }
        ui.hud.hover = None;
        ui.hud.press = None;
        ui.select_start = None;
        ui.click_start = None;
        return;
    }
    // D_GAMBLE and D_build close with Escape (0x466e10).
    if keys.just_pressed(KeyCode::Escape)
        && ui
            .hud
            .dialog
            .as_ref()
            .is_some_and(|d| matches!(d.kind, Kind::Gamble | Kind::Workshop) && d.rolling == 0)
    {
        ui.hud.close_dialog();
        menu.dirty = true;
        return;
    }
    // The story screen closes with Enter as with Escape (0x4c3d38); during a
    // conversation the keyboard does nothing (0x444970).
    if ui.hud.panel == Some(Panel::Narration)
        && (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter))
    {
        ui.hud.set_panel(None);
        menu.dirty = true;
        return;
    }
    if ui.hud.panel == Some(Panel::Say) && keys.just_pressed(KeyCode::Escape) {
        return;
    }
    // The tribute window sends with Enter (0x447aac).
    if ui.hud.panel == Some(Panel::Tribute)
        && (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter))
    {
        send_tribute(&mut ui, scene);
        menu.dirty = true;
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        if ui.target_order.take().is_some()
            || ui.spawning.take().is_some()
            || ui.pointing.take().is_some()
        {
            ui.select_start = None;
            ui.click_start = None;
            menu.notice.clear();
            menu.dirty = true;
            return;
        }
        // A later mouse release must not restore a cancelled selection.
        ui.select_start = None;
        ui.click_start = None;
        ui.last_click = None;
        ui.drag = None;
        ui.hud.press = None;
        ui.hud_scroll_grab = None;
        if ui.hud.panel.is_some() {
            ui.hud.close_panel();
        } else if let Some(previous) = ui.hud.command_history.pop() {
            ui.hud.command_menu = previous;
            ui.hud.page = 0;
        } else if ui.hud.contents_owner.is_some() {
            ui.hud.contents_owner = None;
            ui.hud.inspected_content = None;
        } else if ui.view_lock.is_some() {
            // 0x453340 does nothing while the view is locked.
        } else if !ui.hud.selected.is_empty() || ui.hud.tree.is_some() {
            ui.hud.selected.clear();
            ui.hud.tree = None;
            ui.hud.selection_changed();
            ui.last_click = None;
        } else {
            menu.open(Screen::Pause);
            next.set(Mode::Menu);
            ui.drag = None;
            ui.select_start = None;
        }
        menu.dirty = true;
        return;
    }
    // Keys of the map, as bound on the options' keys page.
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    ui.shift = shift;
    let bound: std::collections::BTreeMap<&str, String> = retzar::keys::ACTIONS
        .iter()
        .map(|(action, _)| (*action, menu.current_settings().key(action)))
        .collect();
    let hit = |action: &str| !chatting && input::hit(&keys, &typed_keys, &bound[action]);
    // During a conversation or on the story screen Enter, F1–F3, F5, F6 and
    // F8–F10 do nothing (0x444970: 0x58779c cleared by 0x4382c0, 0xc97a6c
    // set by 0x4c34c0).
    let story = matches!(ui.hud.panel, Some(Panel::Say | Panel::Narration));
    let gated = |action: &str| !story && hit(action);
    // F5–F8 with Ctrl keep the view in bookmark 1–4 (`[vm] bookmark_set`),
    // with Shift take it back there unless it is locked (0x4f9060,
    // 0x4f6e40); either way the key does nothing else.
    for (n, key) in [KeyCode::F5, KeyCode::F6, KeyCode::F7, KeyCode::F8]
        .into_iter()
        .enumerate()
    {
        if chatting || !(ctrl || shift) || !keys.just_pressed(key) {
            continue;
        }
        if ctrl {
            ui.bookmarks[n] = Some(view.center);
            menu.notice = game
                .skin
                .locale
                .label("vm", "bookmark_set")
                .replace("%d", &(n + 1).to_string());
        } else if let Some(at) = ui.bookmarks[n]
            && ui.view_lock.is_none()
        {
            view.center = at;
        }
        menu.dirty = true;
        return;
    }
    if gated("quick_save") {
        pending.0 = Some(Action::QuickSave);
        return;
    }
    // The cheat panel's key, as its button: only while the button shows.
    if ui.hud.cheats && gated("cheats") {
        if ui.hud.panel == Some(Panel::Cheats) {
            ui.hud.set_panel(None);
        } else if hud::cheats_button(&ui.hud) {
            open_cheats(&mut ui, scene, &game);
        }
        menu.dirty = true;
        return;
    }
    if gated("quick_load") {
        pending.0 = Some(Action::QuickLoad);
        return;
    }
    if ui.hud.panel.is_none() && hit("next_in_group") {
        ui.hud.inspect_next();
        menu.dirty = true;
    }
    if gated("pause") {
        menu.open(Screen::Pause);
        next.set(Mode::Menu);
        ui.select_start = None;
        ui.last_click = None;
        menu.dirty = true;
        return;
    }
    if gated("help") {
        open_help(&mut menu, &mut next);
        ui.select_start = None;
        menu.dirty = true;
        return;
    }
    for (action, target) in [("diplomacy", Panel::Diplomacy), ("quests", Panel::Quests)] {
        if gated(action) {
            let panel = (ui.hud.panel != Some(target)).then_some(target);
            ui.hud.set_panel(panel);
            menu.dirty = true;
            return;
        }
    }
    // F6 (0x4f5d66): the diplomacy window with the tribute window over it
    // (`$canusediplomacy = 0` closes both, see `frontend::gameplay`).
    if gated("tribute") {
        if ui.hud.panel != Some(Panel::Tribute) {
            ui.hud.set_panel(Some(Panel::Tribute));
        }
        menu.dirty = true;
        return;
    }
    if gated("options") {
        menu.open(Screen::Pause);
        menu.open(Screen::Options);
        next.set(Mode::Menu);
        ui.select_start = None;
        menu.dirty = true;
        return;
    }
    if ui.hud.panel.is_none() && !ctrl && gated("chat") {
        ui.hud.set_panel(Some(Panel::Chat(false)));
        menu.dirty = true;
        return;
    }
    if hit("score") && !alt {
        // F4 (0x4f5bce): the scoreboard and the clock, the clock, nothing.
        ui.hud.score = (ui.hud.score + 1) % 3;
        menu.dirty = true;
    }
    if hit("transparent_trees") {
        ui.transparent_trees = !ui.transparent_trees;
    }
    // Game speed on the keypad (0x4f58ce, 0x4f592c) and the hidden turbo of
    // `*` (0x4f5862), for the game to take (`speed_after`).
    use retzar::simulation::SpeedKey;
    for (action, key) in [
        ("speed_up", SpeedKey::Faster),
        ("speed_down", SpeedKey::Slower),
    ] {
        if hit(action) {
            ui.speed_key = Some(key);
        }
    }
    if !chatting && keys.just_pressed(KeyCode::NumpadMultiply) {
        ui.speed_key = Some(SpeedKey::Turbo);
    }
    if !chatting && keys.just_pressed(KeyCode::F11) {
        ui.screenshot = true;
    }
    // F12 (0x4f61ce) lays the map over the whole screen and back, but not
    // in the film framing (+0x86).
    if !chatting && !ui.hud.cinematic && keys.just_pressed(KeyCode::F12) {
        ui.hud.full_map = !ui.hud.full_map;
        view.frame = ui.hud.frame();
        menu.dirty = true;
    }
    if ui.hud.panel.is_none() && hit("delete") {
        delete_selection(&mut ui, scene, shift);
        menu.dirty = true;
    }
    if ui.hud.contents_owner.is_some() {
        for (action, delta) in [("inventory_previous", -1i64), ("inventory_next", 1)] {
            if hit(action) {
                ui.hud.page = (ui.hud.page as i64 + delta).max(0) as usize;
                menu.dirty = true;
            }
        }
    }
    // Ctrl + letter selects the next own building of the classes in
    // `DATA\KEYS.INI` [Ctrl] (0x4061a0).
    if ctrl && ui.hud.panel.is_none() {
        for (letter, code) in LETTERS.iter().enumerate() {
            if keys.just_pressed(*code) {
                cycle_buildings(&mut ui, &game, scene, (b'A' + letter as u8) as char);
                menu.dirty = true;
            }
        }
    }
    // Use the same menu and dispatch as the visible command buttons. Dialogs
    // and text entry own the keyboard; modified chords belong to other actions,
    // and a letter bound to a map command is no longer a command shortcut.
    if ui.hud.panel.is_none() && !ctrl && !alt {
        let commands = hud::commands(
            &game.classic,
            &game.skin.locale,
            &ui.hud,
            &scene.mission,
            &scene.library,
        );
        let free = |hotkey: &str| {
            !bound.values().any(|k| {
                k.strip_prefix("Key")
                    .is_some_and(|l| l.eq_ignore_ascii_case(hotkey))
            })
        };
        if let Some(command) = commands
            .iter()
            .find(|c| free(&c.hotkey) && hotkey_pressed(&keys, &c.hotkey))
        {
            run_command(
                command,
                &mut ui,
                &mut menu,
                &mut next,
                &mut view,
                (surface, &hud, &settings),
                &loaded,
                &game,
            );
            ui.select_start = None;
            menu.dirty = true;
            return;
        }
        if game
            .classic
            .ownership_hotkey("onlinehelp")
            .is_some_and(|key| hotkey_pressed(&keys, key))
        {
            open_help(&mut menu, &mut next);
            ui.select_start = None;
            menu.dirty = true;
            return;
        }
    }
    let cursor = window.cursor_position();
    let point = cursor.map(|c| hud.point(surface, c));
    let layout = hud::Layout {
        width,
        height,
        left,
    };
    let widgets = hud::widgets(
        &game.classic,
        &game.skin.locale,
        &ui.hud,
        layout,
        &scene.mission,
        &scene.library,
    );
    let hover = point.and_then(|p| hud::under(&widgets, &ui.hud, layout, [p.x, p.y]));
    if hover != ui.hud.hover {
        ui.hud.hover = hover;
        menu.dirty = true;
    }
    // 0x4a1884 / 0x4a197c: an occupant under the pointer shows in the
    // information panel, the selection again once the pointer leaves.
    // In a list of the grid the same (0x49e4a6).
    if (ui.hud.contents_owner.is_some() || hud::listing(&ui.hud).is_some())
        && ui.target_order.is_none()
    {
        let under = hover.and_then(|i| match widgets[i].action {
            HudAction::InspectContent(j) | HudAction::Listed(j) => Some(j),
            _ => None,
        });
        if under != ui.hud.inspected_content {
            ui.hud.inspected_content = under;
            menu.dirty = true;
        }
    }
    let over_map = point.is_some_and(|p| !hud::blocks_map(&ui.hud, [p.x, p.y], layout));

    // A conversation bubble follows the right button (0x4c08bc, 0x4c0949,
    // 0x4c092a); the topmost one under the pointer is taken.
    if ui.hud.panel == Some(Panel::Say) {
        if let Some(p) = point {
            let at = [p.x.floor() as i32, p.y.floor() as i32];
            if mouse.just_pressed(MouseButton::Right) {
                ui.say_drag = ui
                    .hud
                    .say
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, b)| hud::say_rect(b).has(p.x, p.y))
                    .map(|(k, b)| (k, [at[0] - b.at[0], at[1] - b.at[1]]));
            } else if let Some((k, grab)) = ui.say_drag
                && let Some(b) = ui.hud.say.get_mut(k)
            {
                let to = hud::say_drag_to([at[0] - grab[0], at[1] - grab[1]], [width, height]);
                if to != b.at {
                    b.at = to;
                    menu.dirty = true;
                }
            }
        }
        if !mouse.pressed(MouseButton::Right) {
            ui.say_drag = None;
        }
        if mouse.just_pressed(MouseButton::Right) {
            return;
        }
    }

    if mouse.just_pressed(MouseButton::Right) && !over_map {
        // A right click takes 100 from a tribute amount, 500 with Shift
        // (message 0x3f2, 0x447b11).
        if let Some(HudAction::Amount(owner, res)) = hover.map(|i| widgets[i].action) {
            let step = if shift { 500 } else { 100 };
            ui.hud
                .tribute_click(&scene.mission, owner, res as usize, -step);
            menu.dirty = true;
            return;
        }
        if let Some(HudAction::InspectContent(j)) = hover.map(|i| widgets[i].action) {
            content_click(&mut ui, scene, j, true, ctrl, alt);
            menu.dirty = true;
            return;
        }
        if let Some(HudAction::Command(index)) = hover.map(|i| widgets[i].action) {
            let list = hud::commands(
                &game.classic,
                &game.skin.locale,
                &ui.hud,
                &scene.mission,
                &scene.library,
            );
            if let Some(command) = list.get(index) {
                let queue = hud::production_queue(
                    &game.classic,
                    &game.skin.locale,
                    &ui.hud,
                    &scene.mission,
                    &scene.library,
                );
                // 0x4a0ed0 → 0x41e950: only units are cancelled, the latest
                // of the kind first, five with Shift.
                let create = game.gameplay.action(&command.order).starts_with("create ");
                let times = if shift { 5 } else { 1 };
                let mut cancelled = std::collections::BTreeMap::<usize, usize>::new();
                for entry in queue
                    .iter()
                    .rev()
                    .filter(|e| create && e.order.eq_ignore_ascii_case(&command.order))
                {
                    let n = cancelled.entry(entry.producer).or_default();
                    if *n < times {
                        *n += 1;
                        let mut req =
                            retzar::gameplay::Request::command("cancel_job", vec![entry.producer]);
                        req.target = Some(entry.slot);
                        ui.requests.push(req);
                    }
                }
            }
        }
        if let Some(HudAction::Dialog(control)) = hover.map(|i| widgets[i].action) {
            dialog_click(&mut ui, &mut menu, scene, &game, control, true, shift);
        }
        return;
    }
    // D_TRADE's market buttons pick a market or harbour of any owner on
    // the map (command_trade, 0x437090) until the dialog closes.
    if let Some(k) = ui.hud.dialog.as_ref().and_then(|d| d.picking)
        && mouse.just_pressed(MouseButton::Left)
        && over_map
        && let Some(p) = cursor
    {
        if let Some(j) = object_at(scene, view.world(surface, p))
            && matches!(scene.mission.objects[j].class, 53 | 54)
            && let Some(d) = ui.hud.dialog.as_mut()
        {
            d.markets[k as usize] = Some(j);
        }
        menu.dirty = true;
        return;
    }

    // A wall row is placed when the button comes up (0x4fb169): one segment
    // on each cell that can take it, from the end nearer the builder; Shift
    // keeps the placement going.
    if let Some(from) = ui.wall_from
        && !mouse.pressed(MouseButton::Left)
    {
        ui.wall_from = None;
        let to = cursor
            .filter(|_| over_map)
            .map(|p| view.world(surface, p))
            .map(|w| [(w.x / 32.).floor() as i32, (-w.y / 32.).floor() as i32])
            .filter(|c| c[0] >= 0 && c[1] >= 0 && c[0] < side as i32 && c[1] < side as i32)
            .map_or(from, |c| [c[0] as u16, c[1] as u16]);
        if let Some(order) = ui.target_order.clone() {
            let selected = command_selection(&ui);
            for (p, ok) in wall_preview(scene, &game, &order, &selected, from, to, alt) {
                if ok {
                    ui.requests.push(retzar::gameplay::Request {
                        order: order.clone(),
                        selected: selected.clone(),
                        cell: Some(p),
                        ctrl,
                        alt,
                        ..Default::default()
                    });
                }
            }
            if !shift {
                ui.target_order = None;
            }
        }
        menu.dirty = true;
        return;
    }

    // The cheat panel's «spawn»: a left click on the map puts the object
    // there (message 0x112, 0x4f5011) and the button's release ends the
    // placement (0x4f5064), as a right click does.
    // With Shift held the placement goes on, as a building's does.
    if let Some((class, civ, owner)) = ui.spawning
        && ui.hud.panel.is_none()
        && over_map
        && (mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right))
    {
        if mouse.just_pressed(MouseButton::Right) || !shift {
            ui.spawning = None;
        }
        if mouse.just_pressed(MouseButton::Left)
            && let Some(p) = cursor
        {
            let world = view.world(surface, p);
            let cell = [
                (world.x / 32.).floor() as i32,
                (-world.y / 32.).floor() as i32,
            ];
            if cell[0] >= 0 && cell[1] >= 0 && cell[0] < side as i32 && cell[1] < side as i32 {
                ui.requests.push(retzar::gameplay::Request {
                    order: "cheat".into(),
                    cell: Some([cell[0] as u16, cell[1] as u16]),
                    cheat: Some(retzar::gameplay::cheats::Cheat::Spawn { class, civ, owner }),
                    ..Default::default()
                });
            }
        }
        ui.select_start = None;
        ui.click_start = None;
        menu.notice.clear();
        menu.dirty = true;
        return;
    }
    // A command of the «Oggetti» page: a left click gives it the object
    // under the pointer.
    if let Some(target) = ui.pointing
        && ui.hud.panel.is_none()
        && over_map
        && (mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right))
    {
        if mouse.just_pressed(MouseButton::Right) || !shift {
            ui.pointing = None;
        }
        if mouse.just_pressed(MouseButton::Left)
            && let Some(p) = cursor
            && let Some(object) = object_at(scene, view.world(surface, p))
        {
            use hud::cheat_panel::Target;
            use retzar::gameplay::cheats::Cheat;
            let cheat = match target {
                Target::God => Cheat::God,
                Target::Usurp => Cheat::Usurp,
                Target::Spy => Cheat::Spy,
                Target::Kingdom => {
                    let kingdoms = hud::cheat_panel::transfer_kingdoms(&scene.mission);
                    Cheat::Kingdom(kingdoms[ui.hud.cheat.kingdom % kingdoms.len()])
                }
            };
            ui.requests.push(retzar::gameplay::Request {
                order: "cheat".into(),
                selected: vec![object],
                cheat: Some(cheat),
                ..Default::default()
            });
        }
        ui.select_start = None;
        ui.click_start = None;
        menu.notice.clear();
        menu.dirty = true;
        return;
    }
    if ui.hud.panel.is_none()
        && (mouse.just_pressed(MouseButton::Right)
            || (ui.target_order.is_some() && mouse.just_pressed(MouseButton::Left)))
    {
        if over_map && let Some(p) = cursor {
            let world = view.world(surface, p);
            let cell = [
                (world.x / 32.).floor() as i32,
                (-world.y / 32.).floor() as i32,
            ];
            if cell[0] >= 0 && cell[1] >= 0 && cell[0] < side as i32 && cell[1] < side as i32 {
                let picked = object_at(scene, world);
                if mouse.just_pressed(MouseButton::Right) && ui.target_order.is_some() {
                    ui.target_order = None;
                    menu.notice.clear();
                    menu.dirty = true;
                    return;
                }
                let order = ui.target_order.take().unwrap_or_else(|| "context".into());
                let selected = command_selection(&ui);
                let mut destination = [cell[0] as u16, cell[1] as u16];
                if game.gameplay.action(&order).starts_with("build ") {
                    if let Some(worker) = selected
                        .iter()
                        .min()
                        .and_then(|&i| scene.mission.objects.get(i))
                    {
                        let Some(probe) = retzar::gameplay::building_at(
                            &scene.library,
                            &game.gameplay,
                            worker,
                            &order,
                            destination,
                        ) else {
                            ui.target_order = Some(order);
                            return;
                        };
                        destination = [probe.x, probe.y];
                    }
                    // 0x4fb0be: a wall row is dragged from here.
                    if retzar::gameplay::wall_order(&game.gameplay, &order)
                        && mouse.just_pressed(MouseButton::Left)
                    {
                        ui.wall_from = Some(destination);
                        ui.target_order = Some(order);
                        menu.dirty = true;
                        return;
                    }
                }
                let request = retzar::gameplay::Request {
                    order,
                    selected,
                    cell: Some(destination),
                    target: picked,
                    ctrl,
                    alt,
                    point: Some([world.x, -world.y]),
                    ..Default::default()
                };
                let building = game.gameplay.action(&request.order).starts_with("build ");
                let patrol = game.gameplay.action(&request.order) == "patrol";
                // 0x436400, 0x435640: the guard, repair and attack commands
                // gather the objects clicked with Shift in their list (an
                // Alt click attacks a type at once, 0x435603).
                let gathered = match game.gameplay.action(&request.order).as_str() {
                    "guard" | "repair" => true,
                    "attack" => !alt,
                    _ => false,
                } && request.target.is_some();
                if shift && !building && (request.order == "context" || patrol || gathered) {
                    // Shift collects the stages of one route; the order leaves
                    // when Shift is released (0x436730, 0x4f557d). A patrol
                    // takes all its points at once (0x42f7d0), a guard or a
                    // repair all its objects (0x436554).
                    if ui.waypoints.len() < 32 {
                        ui.waypoints.push(request.clone());
                    }
                    if patrol || gathered {
                        ui.target_order = Some(request.order.clone());
                    }
                } else {
                    // Placing a building goes on while Shift is held (0x4fb34a).
                    if building && shift {
                        ui.target_order = Some(request.order.clone());
                    }
                    ui.requests.push(request);
                }
                ui.select_start = None;
                ui.click_start = None;
                menu.dirty = true;
                return;
            }
        }
        if mouse.just_pressed(MouseButton::Right) && ui.target_order.take().is_some() {
            menu.notice.clear();
            menu.dirty = true;
            return;
        }
    }

    if !shift
        && ui
            .waypoints
            .first()
            .is_some_and(|w| game.gameplay.action(&w.order) == "patrol")
    {
        let stages = std::mem::take(&mut ui.waypoints);
        let mut order = stages[0].clone();
        order.waypoints = stages.iter().filter_map(|s| s.cell).collect();
        ui.target_order = None;
        ui.requests.push(order);
    }
    if !shift
        && ui.waypoints.first().is_some_and(|w| {
            matches!(
                game.gameplay.action(&w.order).as_str(),
                "guard" | "repair" | "attack"
            ) && w.target.is_some()
        })
    {
        let stages = std::mem::take(&mut ui.waypoints);
        let mut order = stages[0].clone();
        for s in &stages {
            if let Some(j) = s.target
                && !order.targets.contains(&j)
            {
                order.targets.push(j);
            }
        }
        ui.target_order = None;
        ui.requests.push(order);
    }
    if !shift && !ui.waypoints.is_empty() {
        for (n, mut stage) in std::mem::take(&mut ui.waypoints).into_iter().enumerate() {
            stage.append = n > 0;
            ui.requests.push(stage);
        }
    }
    // Camera. Keyboard and screen-edge scrolling work wherever the cursor is,
    // including over the stone frame; dragging keeps going once started.
    let navigating = ui.hud.panel.is_none();
    // Home, PgUp and PgDn visit the latest events (0x4937c0): Alt only the
    // attacks, Ctrl also selects the object. End shows the destination of
    // the selected unit or the rally point of the selected building.
    let event_step = if !navigating {
        None
    } else if hit("event_last") {
        Some(0)
    } else if hit("event_previous") {
        Some(-1)
    } else if hit("event_next") {
        Some(1)
    } else {
        None
    };
    if let Some(step) = event_step
        && let Some(sim) = scene.mission.simulation.as_ref()
    {
        let events: Vec<_> = sim
            .gameplay
            .events
            .iter()
            .filter(|e| !alt || e.kind == 1)
            .collect();
        if !events.is_empty() {
            ui.event_cursor = match step {
                0 => 0,
                s => (ui.event_cursor as i64 + s).rem_euclid(events.len() as i64) as usize,
            }
            .min(events.len() - 1);
            let e = events[ui.event_cursor];
            let center = Vec2::new(
                e.cell[0] as f32 * 32. + 16.,
                -(e.cell[1] as f32 * 32. + 16.),
            );
            center_on(&mut view, surface, &hud, &settings, center);
            if ctrl
                && scene
                    .mission
                    .objects
                    .get(e.object)
                    .is_some_and(|o| o.selectable())
                && ui.view_lock.is_none()
            {
                ui.hud.selected = vec![e.object];
                ui.hud.selection_changed();
            }
            menu.dirty = true;
        }
    }
    if navigating
        && hit("destination")
        && let Some(sim) = scene.mission.simulation.as_ref()
        && let Some(&i) = ui.hud.selected.first()
        && let Some(a) = sim.gameplay.actors.get(i)
    {
        let cell = a.rally.or(a.goal).or(match a.intent {
            Some(retzar::gameplay::Intent::Move(p)) => Some(p),
            _ => None,
        });
        if let Some(p) = cell {
            let center = Vec2::new(p[0] as f32 * 32. + 16., -(p[1] as f32 * 32. + 16.));
            center_on(&mut view, surface, &hud, &settings, center);
            // 0x4f636a: a unit's destination also shows the red cross.
            if a.rally.is_none() {
                ui.order_feedback = Some(super::orders::Feedback {
                    cell: p,
                    flag: false,
                    target: None,
                    color: super::orders::GREEN,
                    cross: Some([center.x, -center.y]),
                    started: time.elapsed_secs(),
                    generation: loaded.generation,
                });
            }
        }
    }
    let dt = time.delta_secs().min(0.1);
    if mouse.pressed(MouseButton::Middle)
        && (ui.drag.is_some() || (over_map && ui.select_start.is_none()))
    {
        if let (Some(prev), Some(now)) = (ui.drag, cursor) {
            let d = now - prev;
            let scale = view.scale;
            view.center += Vec2::new(-d.x, d.y) * scale;
        }
        ui.drag = cursor.or(ui.drag);
    } else {
        ui.drag = None;
        let mut d = Vec2::ZERO;
        let held = |action: &str| navigating && input::held(&keys, &typed_keys, &bound[action]);
        if held("scroll_left") {
            d.x -= 1.;
        }
        if held("scroll_right") {
            d.x += 1.;
        }
        if held("scroll_up") {
            d.y += 1.;
        }
        if held("scroll_down") {
            d.y -= 1.;
        }
        if settings.edge_scroll
            && let Some(p) = cursor
        {
            // As in the original, only the outermost pixels of the screen scroll.
            let edge = 3.;
            if p.x < edge {
                d.x -= 1.;
            }
            if p.x > window.width() - edge {
                d.x += 1.;
            }
            if p.y < edge {
                d.y += 1.;
            }
            if p.y > window.height() - edge {
                d.y -= 1.;
            }
        }
        // The original moved the map by 8 × speed + 16 map pixels every 20 ms,
        // speed 0–4 (2 by default): 800–2400 pixels per second, diagonals
        // on both axes at once.
        let level = (settings.scroll_speed as f32 / 25.).round();
        let speed = (8. * level + 16.) * 50.;
        view.center += d.clamp(Vec2::splat(-1.), Vec2::ONE) * speed * dt;
    }
    let amount = scroll.delta.y
        * if scroll.unit == MouseScrollUnit::Pixel {
            0.01
        } else {
            1.
        };
    if over_map && amount != 0. {
        if ui.wheel != 0. && ui.wheel.signum() != amount.signum() {
            ui.wheel = 0.;
        }
        ui.wheel += amount;
        let steps = ui.wheel.trunc();
        if steps != 0. {
            ui.wheel -= steps;
            let levels = zoom_levels(&hud);
            let glide = settings.smooth_zoom.then(|| glide_levels(&levels));
            let levels = glide.as_ref().map_or(&levels[..], |g| &g[..]);
            // Notches during a glide go on from where it is heading.
            let mut magnification = match ui.glide {
                Some((to, _, _)) if glide.is_some() => surface.dpi / to,
                _ => view.magnification(surface),
            };
            for _ in 0..steps.abs() as i32 {
                magnification = zoom_step(levels, magnification, steps.signum() as i32);
            }
            let target = (surface.dpi / magnification).clamp(0.02, 32.);
            if glide.is_some() {
                ui.glide = Some((target, cursor, view.scale));
            } else {
                view.zoom_at(surface, cursor, target);
            }
        }
    }
    // The glide: about a tenth of a second to the target, the point under
    // the pointer still; a scale set elsewhere (Home, a jump) ends it.
    if let Some((to, at, last)) = ui.glide {
        if view.scale != last || !settings.smooth_zoom {
            ui.glide = None;
        } else {
            let next = glide_step(view.scale, to, dt);
            view.zoom_at(surface, at, next);
            ui.glide = (next != to).then_some((to, at, view.scale));
        }
    }

    let control = |i: usize| {
        chords.groups[i].unwrap_or_else(|| {
            keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
        })
    };
    if navigating && hit("center") {
        let selected = ui
            .hud
            .inspected_object()
            .map_or_else(|| ui.hud.selected.clone(), |i| vec![i]);
        focus_selection(&mut view, surface, &hud, &settings, &loaded, &selected);
        // A chosen tree's `treeselection` stands on its cell.
        if let (true, Some((cell, _))) = (selected.is_empty(), ui.hud.tree) {
            let (x, y) = ((cell % side) as f32, (cell / side) as f32);
            let at = Vec2::new(x * 32. + 16., -(y * 32. + 16.));
            center_on(&mut view, surface, &hud, &settings, at);
        }
    }
    // Backspace, on its release (key 8, 0x4f621e): the first selected
    // object's active item, else its last book, else its first potion
    // (0x4f625c), used as by a left click on it (0x4f62b4–0x4f62e7).
    if navigating
        && !chatting
        && keys.just_released(KeyCode::Backspace)
        && let Some(&first) = ui.hud.selected.first()
        && let Some(j) = scene
            .mission
            .simulation
            .as_ref()
            .and_then(|sim| retzar::gameplay::quick_item(sim, &scene.mission, first))
    {
        item_click(&mut ui, scene, j, false, ctrl);
        menu.dirty = true;
    }
    for (i, key) in GROUP_KEYS.iter().enumerate() {
        if navigating && keys.just_pressed(*key) {
            if control(i) {
                let selected = ui.hud.selected.clone();
                selection::assign_group(&mut ui.groups, i, &selected, &scene.mission);
            } else {
                // 0x453790: a group recalls its members still controlled,
                // nothing if it is empty; Shift adds it to the selection,
                // Alt also centres the view on its first member (0x4f523a).
                let selected: Vec<usize> = ui.groups[i]
                    .iter()
                    .copied()
                    .filter(|&k| {
                        scene
                            .mission
                            .objects
                            .get(k)
                            .is_some_and(|o| selection::controllable(o, &scene.mission))
                    })
                    .collect();
                // 0x453240 adds nothing while the view is locked (0x589714).
                if !selected.is_empty() && ui.view_lock.is_none() {
                    // Alt comes before Shift (0x4f51f5).
                    if alt || !shift {
                        ui.hud.selected.clear();
                    }
                    selection::add(&mut ui.hud.selected, &selected, &scene.mission);
                    ui.hud.selection_changed();
                    if alt {
                        focus_selection(
                            &mut view,
                            surface,
                            &hud,
                            &settings,
                            &loaded,
                            &selected[..1],
                        );
                    }
                }
            }
            menu.dirty = true;
        }
    }
    if navigating
        && keys.just_pressed(KeyCode::Digit0)
        && chords.zero.unwrap_or_else(|| {
            keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
        })
    {
        let selected = ui.hud.selected.clone();
        selection::remove_from_groups(&mut ui.groups, &selected);
        menu.dirty = true;
    }

    let minimap = hud::map_rect(width, height, left);
    if mouse.just_pressed(MouseButton::Left) {
        let press = frame.press.or(cursor);
        let press_hud = press.map(|p| hud.point(surface, p));
        if let Some(p) = press_hud
            && let Some(track) = hud::scroll_track(&ui.hud, layout)
            && let Some(thumb) = hud::scroll_thumb(&ui.hud, layout)
            && ui.hud.scroll_max > 0
            && track.has(p.x, p.y)
        {
            ui.hud_scroll_grab = Some(if thumb.has(p.x, p.y) {
                p.y - thumb.y as f32
            } else {
                thumb.h as f32 / 2.
            });
        }
        ui.hud.press = press_hud
            .and_then(|p| hud::under(&widgets, &ui.hud, layout, [p.x, p.y]))
            .map(|i| widgets[i].action);
        menu.dirty = true;
        // 0x47ef80: the left button on a window's own ground (no control
        // under it) takes the window along until it comes up.
        if ui.hud.press.is_none()
            && ui.hud_scroll_grab.is_none()
            && let Some(p) = press_hud
            && let Some(panel) = ui.hud.panel.filter(|p| p.draggable())
        {
            let w = ui.hud.window(layout, panel);
            if w.has(p.x, p.y) {
                ui.hud.grab = Some([p.x.floor() as i32 - w.x, p.y.floor() as i32 - w.y]);
            }
        }
        if ui.hud.press.is_none()
            && ui.hud_scroll_grab.is_none()
            && let Some(p) = press_hud
            && !minimap.has(p.x, p.y)
            && !hud::blocks_map(&ui.hud, [p.x, p.y], layout)
        {
            ui.select_start = press.map(|p| view.world(surface, p));
            ui.click_start = press.map(|p| (p, Instant::now()));
            ui.boxing = false;
        }
    }
    // The window follows the pointer (message 0x101, 0x47f020) while the
    // button is down.
    if !mouse.pressed(MouseButton::Left) {
        if ui.hud.grab.is_some() {
            ui.hud.grab = None;
        }
    } else if let (Some(grab), Some(p)) = (ui.hud.grab, point)
        && ui.hud.drag_window(
            layout,
            [p.x.floor() as i32 - grab[0], p.y.floor() as i32 - grab[1]],
        )
    {
        menu.dirty = true;
    }
    // 0x4f496c: once the drag is a rectangle it is drawn, and without Shift
    // the selection empties at that moment (0x4f49a8), before the release.
    if mouse.pressed(MouseButton::Left)
        && !ui.boxing
        && let (Some(start), Some(p)) = (ui.select_start, cursor)
        && (view.world(surface, p) - start).abs().max_element() > BOX_PIXELS
    {
        ui.boxing = true;
        if ui.view_lock.is_none()
            && !(keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight))
            && (!ui.hud.selected.is_empty() || ui.hud.tree.is_some())
        {
            ui.hud.selected.clear();
            ui.hud.tree = None;
            ui.hud.selection_changed();
            menu.dirty = true;
        }
    }
    if mouse.pressed(MouseButton::Left)
        && let (Some(grab), Some(p)) = (ui.hud_scroll_grab, point)
        && let (Some(track), Some(thumb)) = (
            hud::scroll_track(&ui.hud, layout),
            hud::scroll_thumb(&ui.hud, layout),
        )
    {
        let ratio =
            ((p.y - grab - track.y as f32) / (track.h - thumb.h).max(1) as f32).clamp(0., 1.);
        ui.hud.scroll = (ratio * ui.hud.scroll_max as f32).round() as usize;
        menu.dirty = true;
    }
    if mouse.pressed(MouseButton::Left)
        && let Some(p) = point
        && ui.select_start.is_none()
        && ui.hud.press.is_none()
        && ui.hud_scroll_grab.is_none()
        && ui.hud.grab.is_none()
        && minimap.has(p.x, p.y)
    {
        let edge = side as f32 * 32.;
        let target = Vec2::new(
            (p.x - minimap.x as f32) / minimap.w as f32 * edge,
            -(p.y - minimap.y as f32) / minimap.h as f32 * edge,
        );
        center_on(&mut view, surface, &hud, &settings, target);
    }
    if amount != 0.
        && point.is_some_and(|p| {
            ui.hud
                .panel
                .is_some_and(|panel| ui.hud.window(layout, panel).has(p.x, p.y))
        })
    {
        scroll_panel(&mut ui, &loaded, &game, (-amount.signum() * 3.) as i32);
        menu.dirty = true;
    }
    if mouse.just_released(MouseButton::Left) {
        ui.hud_scroll_grab = None;
        if let Some(action) = ui.hud.press.take()
            && hover.is_some_and(|i| widgets[i].action == action)
        {
            match action {
                HudAction::Pause => {
                    menu.open(Screen::Pause);
                    next.set(Mode::Menu);
                }
                HudAction::Panel(p) => {
                    let panel = if ui.hud.panel == Some(p) {
                        None
                    } else {
                        Some(p)
                    };
                    ui.hud.set_panel(panel);
                    ui.hud.scroll = 0;
                    scroll_panel(&mut ui, &loaded, &game, 0);
                }
                HudAction::Close => ui.hud.close_panel(),
                HudAction::Tribute => ui.hud.set_panel(Some(Panel::Tribute)),
                HudAction::Amount(owner, res) => {
                    let step = if shift { 500 } else { 100 };
                    ui.hud
                        .tribute_click(&scene.mission, owner, res as usize, step);
                }
                HudAction::Send => send_tribute(&mut ui, scene),
                HudAction::OpenCheats => open_cheats(&mut ui, scene, &game),
                HudAction::Cheat(control) => cheat_control(&mut ui, scene, control, shift),
                HudAction::Fit => fit(&mut view, surface, &hud, &settings, side),
                HudAction::Answer(i) => {
                    ui.answer = Some(i);
                    ui.hud.set_panel(None);
                }
                HudAction::SayEye(k) => ui.look = Some(k),
                HudAction::SayVoices => {
                    ui.hud.voices_off = !ui.hud.voices_off;
                    ui.hush = ui.hud.voices_off;
                }
                HudAction::Listed(i) => {
                    // 0x49e7e2 → 0x453240: the object joins the selection;
                    // of an item, the unit holding it.
                    let target = if scene.mission.objects[i].item() {
                        scene
                            .mission
                            .simulation
                            .as_ref()
                            .and_then(|s| s.objects.get(i)?.holder)
                    } else {
                        Some(i)
                    };
                    if let Some(t) = target {
                        retzar::selection::add(&mut ui.hud.selected, &[t], &scene.mission);
                    }
                }
                HudAction::Holder => {
                    if let Some(&i) = ui.hud.selected.first() {
                        ui.hud.contents_owner = if ui.hud.contents_owner.is_some() {
                            None
                        } else {
                            Some(i)
                        };
                        ui.hud.inspected_content = None;
                        ui.hud.page = 0;
                    }
                }
                HudAction::InspectContent(i) => content_click(&mut ui, scene, i, false, ctrl, alt),
                HudAction::BackMenu => {
                    if let Some(previous) = ui.hud.command_history.pop() {
                        ui.hud.command_menu = previous;
                        ui.hud.page = 0;
                    }
                }
                HudAction::Scroll(n) => scroll_panel(&mut ui, &loaded, &game, n),
                HudAction::Page(n) => {
                    if ui.hud.panel == Some(Panel::Quests) {
                        scroll_panel(&mut ui, &loaded, &game, n);
                    } else {
                        ui.hud.page = (ui.hud.page as i64 + n as i64).max(0) as usize;
                    }
                }
                HudAction::Chat => {
                    let panel = if chatting {
                        None
                    } else {
                        Some(Panel::Chat(ui.hud.chat.to == Recipients::Custom))
                    };
                    ui.hud.set_panel(panel);
                }
                HudAction::Recipients(to) => {
                    ui.hud.chat.to = to;
                    ui.hud
                        .set_panel(Some(Panel::Chat(to == Recipients::Custom)));
                }
                HudAction::Inert => {}
                // 0x45c960: the ball turns red and its mode begins.
                HudAction::Help => {
                    ui.hud.help =
                        Some(point.map_or([0, 0], |p| [p.x.floor() as i32, p.y.floor() as i32]));
                }
                HudAction::ChatTo(owner) => {
                    if let Some(on) = ui.hud.chat.custom.get_mut(owner as usize - 1) {
                        *on = !*on;
                    }
                }
                HudAction::Relation(owner, bit) => {
                    edit_diplomacy(&mut ui, &scene.mission, |d, player| {
                        let i = (player as usize - 1) * 8 + owner as usize - 1;
                        d[i] = (d[i] & !diplomacy::ATTITUDE) | bit;
                    });
                }
                HudAction::Share(owner, bit) => {
                    edit_diplomacy(&mut ui, &scene.mission, |d, player| {
                        d[(player as usize - 1) * 8 + owner as usize - 1] ^= bit;
                    });
                }
                HudAction::SharedVictory => {
                    // 0x446fdf: the box flips; Accept writes it toward each
                    // other kingdom (0x447044).
                    let others = hud::other_kingdoms(&scene.mission);
                    edit_diplomacy(&mut ui, &scene.mission, |d, player| {
                        let row = (player as usize - 1) * 8;
                        let on = others
                            .iter()
                            .all(|&o| d[row + o as usize - 1] & diplomacy::VICTORY != 0);
                        for &o in &others {
                            let i = row + o as usize - 1;
                            d[i] = if on {
                                d[i] & !diplomacy::VICTORY
                            } else {
                                d[i] | diplomacy::VICTORY
                            };
                        }
                    });
                }
                HudAction::Accept => {
                    if let Some(d) = ui.hud.diplomacy_draft.take() {
                        ui.hud.diplomacy =
                            Some(retzar::gameplay::accept_diplomacy(&scene.mission, &d));
                    }
                    ui.hud.set_panel(None);
                }
                HudAction::Dialog(control) => {
                    // 0x4a2e19: Alt on a market button of D_TRADE centres the
                    // view on that market's corner (0x4f6e40) instead of
                    // picking one.
                    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
                    let market = match (control, ui.hud.dialog.as_ref()) {
                        (Control::Market(k), Some(d)) if alt && d.kind == Kind::Route => {
                            Some(d.markets.get(k as usize).copied().flatten())
                        }
                        _ => None,
                    };
                    match market {
                        Some(Some(j)) => {
                            if let Some(o) = scene.mission.objects.get(j) {
                                let at = Vec2::new(o.x as f32 * 32., -(o.y as f32 * 32.));
                                center_on(&mut view, surface, &hud, &settings, at);
                            }
                        }
                        Some(None) => {}
                        None => {
                            dialog_click(&mut ui, &mut menu, scene, &game, control, false, shift)
                        }
                    }
                }
                HudAction::Command(i) => {
                    let list = hud::commands(
                        &game.classic,
                        &game.skin.locale,
                        &ui.hud,
                        &scene.mission,
                        &scene.library,
                    );
                    if let Some(command) = list.get(i) {
                        run_command(
                            command,
                            &mut ui,
                            &mut menu,
                            &mut next,
                            &mut view,
                            (surface, &hud, &settings),
                            &loaded,
                            &game,
                        );
                    }
                }
            }
            menu.dirty = true;
        }
        // While LockView holds the view no click or rectangle selects
        // (0x589714 stops 0x453240 and 0x453340).
        if let Some(start) = ui.select_start.take()
            && ui.view_lock.is_none()
            && let Some(p) = cursor
        {
            let end = view.world(surface, p);
            let area = (end - start).abs().max_element() > BOX_PIXELS;
            let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            // Without Shift the release empties the selection first
            // (0x4f4ec9), a chosen tree with it.
            if !shift {
                ui.hud.tree = None;
            }
            let mut tree = None;
            let selectable = |v: &&retzar::world::ObjectVisual| {
                v.layer == Layer::Body
                    && v.object < scene.mission.objects.len()
                    && scene.mission.objects[v.object].selectable()
                    && scene
                        .mission
                        .simulation
                        .as_ref()
                        .is_none_or(|s| s.visible(&scene.mission.objects[v.object]))
            };
            let mut selected: Vec<usize> = if area {
                let (lo, hi) = (start.min(end), start.max(end));
                // An object is in the box with its cell (0x4534b0) or with
                // the middle of its picture.
                let inside = |v: &&retzar::world::ObjectVisual| {
                    let o = &scene.mission.objects[v.object];
                    let (cx, cy) = (
                        v.bounds[0] + v.bounds[2] / 2.,
                        v.bounds[1] + v.bounds[3] / 2.,
                    );
                    ((o.x as f32) >= (lo.x / 32.).floor()
                        && (o.x as f32) <= (hi.x / 32.).floor()
                        && (o.y as f32) >= (-hi.y / 32.).floor()
                        && (o.y as f32) <= (-lo.y / 32.).floor())
                        || (cx >= lo.x && cx <= hi.x && cy >= lo.y && cy <= hi.y)
                };
                let mut seen = std::collections::HashSet::new();
                let mut boxed = scene
                    .visuals
                    .iter()
                    .filter(selectable)
                    .filter(inside)
                    // 0x4534b0: units the player commands (0x468380, so
                    // not frozen nor stone), then 0x453200: units whose
                    // kingdom shares them with the player; a spy's host or
                    // a hypnotised unit goes alone.
                    .filter(|v| {
                        scene.mission.commands_at(v.object)
                            && selection::mixable(&scene.mission.objects[v.object], &scene.mission)
                    })
                    .map(|v| v.object)
                    .filter(|i| seen.insert(*i))
                    .collect::<Vec<_>>();
                // Without units, the own buildings of the kind met first.
                if boxed.is_empty() {
                    let mut kind = None;
                    boxed = scene
                        .visuals
                        .iter()
                        .filter(selectable)
                        .filter(inside)
                        .filter(|v| {
                            let o = &scene.mission.objects[v.object];
                            o.building()
                                && selection::controllable(o, &scene.mission)
                                && *kind.get_or_insert(o.class) == o.class
                        })
                        .map(|v| v.object)
                        .filter(|i| seen.insert(*i))
                        .collect();
                }
                // 0x4534b0 walks the cells row by row from the top left.
                boxed.sort_by_key(|&i| (scene.mission.objects[i].y, scene.mission.objects[i].x));
                boxed
            } else {
                match selection::click(
                    &scene.mission,
                    &scene.library,
                    &scene.grid,
                    &scene.visuals,
                    [end.x, end.y],
                ) {
                    Some(selection::Pick::Object(i)) => vec![i],
                    Some(selection::Pick::Tree(cell)) => {
                        tree = Some(cell);
                        Vec::new()
                    }
                    None => Vec::new(),
                }
            };
            let mut multiple_click = false;
            if !area && let Some(&i) = selected.first() {
                let (press, at) = ui.click_start.take().unwrap_or((p, Instant::now()));
                let double = ui.last_click.is_some_and(|(old, pos, time)| {
                    old == i
                        && at >= time
                        && at.duration_since(time).as_secs_f32() <= double_click_seconds()
                        && (press - pos).abs().max_element() <= 4.
                });
                ui.last_click = if double { None } else { Some((i, press, at)) };
                let o = &scene.mission.objects[i];
                // 0x4f4b72: only a controlled unit starts a double click; this
                // version also takes the own buildings of the clicked kind.
                let building = o.building() && selection::controllable(o, &scene.mission);
                if double && (selection::mixable(o, &scene.mission) || building) {
                    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
                    let ctrl =
                        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
                    multiple_click = true;
                    let (min, max) = play_area(surface, &hud, &settings, view.frame);
                    let a = view.world(surface, min);
                    let z = view.world(surface, max);
                    let (lo, hi) = (a.min(z), a.max(z));
                    let mut seen = std::collections::HashSet::new();
                    selected = scene
                        .visuals
                        .iter()
                        .filter(selectable)
                        .filter(|v| {
                            let other = &scene.mission.objects[v.object];
                            selection::controllable(other, &scene.mission)
                                && other.owner == o.owner
                                && if building {
                                    // Finished ones with a finished one.
                                    other.class == o.class
                                        && (other.flags & retzar::world::FLAG_CONSTRUCTION)
                                            == (o.flags & retzar::world::FLAG_CONSTRUCTION)
                                } else if alt {
                                    // Alt: the same combat flag (0x10000).
                                    selection::combat(other) == selection::combat(o)
                                } else {
                                    other.class == o.class && other.civilization == o.civilization
                                }
                                && (!ctrl
                                    || scene.mission.simulation.as_ref().is_none_or(|s| {
                                        s.objects.get(v.object).map(|o| o.order)
                                            == s.objects.get(i).map(|o| o.order)
                                    }))
                                && v.bounds[0] < hi.x
                                && v.bounds[0] + v.bounds[2] > lo.x
                                && v.bounds[1] < hi.y
                                && v.bounds[1] + v.bounds[3] > lo.y
                        })
                        .map(|v| v.object)
                        .filter(|i| seen.insert(*i))
                        .collect();
                    // 0x4f47a0 orders the matches by distance from the clicked unit.
                    selected.sort_by_key(|&v| {
                        let other = &scene.mission.objects[v];
                        (other.x as i32 - o.x as i32).pow(2) + (other.y as i32 - o.y as i32).pow(2)
                    });
                }
            } else {
                ui.last_click = None;
                ui.click_start = None;
            }
            // A box and a double click only add, Shift keeping the current
            // selection (0x4534b0, 0x4f4bfd); a click with Shift toggles.
            if area || multiple_click {
                if !shift {
                    ui.hud.selected.clear();
                }
                selection::add(&mut ui.hud.selected, &selected, &scene.mission);
            } else {
                selection::apply(&mut ui.hud.selected, &selected, shift, &scene.mission);
            }
            // The `treeselection` object never shares the selection
            // (0x453240): a tree goes in alone, anything else puts it out.
            if let Some(cell) = tree {
                ui.hud.selected.clear();
                let wood = scene.mission.simulation.as_ref();
                ui.hud.tree = Some((cell, wood.map_or(100, |s| selection::tree_wood(s, cell))));
            } else if !ui.hud.selected.is_empty() {
                ui.hud.tree = None;
            }
            ui.hud.selection_changed();
            ui.paged = ui.hud.selected.clone();
            menu.dirty = true;
        }
    }
    clamp(&mut view, surface, &hud, &settings, side);
}

fn double_click_seconds() -> f32 {
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetDoubleClickTime() -> u32;
        }
        // Read-only Win32 query, no pointer arguments or state changes.
        unsafe { GetDoubleClickTime() as f32 / 1000. }
    }
    #[cfg(not(windows))]
    {
        0.5
    }
}

/// Change the diplomacy window's choices: a full 8×8 copy of the shown
/// diplomacy, edited on the human player's row.
fn edit_diplomacy(ui: &mut MissionUi, mission: &MissionWorld, f: impl FnOnce(&mut [u32], u8)) {
    let player = mission.player();
    if !(1..=8).contains(&player) {
        return;
    }
    let mut d = ui.hud.diplomacy(mission).to_vec();
    d.resize(64, diplomacy::ENEMY);
    f(&mut d, player);
    ui.hud.diplomacy_draft = Some(d);
}

/// Send of the tribute window (0x447bd1): one transfer for each amount the
/// player holds (0x490dd0), then back to the diplomacy window.
fn send_tribute(ui: &mut MissionUi, scene: &super::scene::Scene) {
    for (owner, resource, amount) in ui.hud.tribute_sends(&scene.mission) {
        ui.requests.push(retzar::gameplay::Request {
            order: "tribute".into(),
            target: Some(owner as usize),
            resource,
            amount,
            ..Default::default()
        });
    }
    ui.hud.close_panel();
}

/// The cheat panel switched on or off from the options of the game: on,
/// its button shows in the command grid while nothing is selected; off,
/// the panel closes and a click it waits for is given up.
pub(super) fn switch_cheats(world: &mut World, on: bool) {
    let mut ui = world.resource_mut::<MissionUi>();
    if ui.hud.cheats == on {
        return;
    }
    ui.hud.cheats = on;
    if !on {
        if ui.hud.panel == Some(Panel::Cheats) {
            ui.hud.set_panel(None);
        }
        ui.spawning = None;
        ui.pointing = None;
    }
}

/// The cheat panel opens with the researches the player has left and two
/// different kingdoms for the relation rows.
pub(super) fn open_cheats(ui: &mut MissionUi, scene: &super::scene::Scene, game: &super::Game) {
    let c = &mut ui.hud.cheat;
    c.researches =
        retzar::gameplay::cheats::researches_left(&game.gameplay, &scene.library, &scene.mission);
    c.research = c.research.min(c.researches.len().saturating_sub(1));
    let players = scene.mission.kingdoms.len().clamp(1, 8);
    if c.from % players == c.to % players {
        c.to = (c.from + 1) % players;
    }
    ui.spawning = None;
    ui.pointing = None;
    ui.target_order = None;
    ui.hud.set_panel(Some(Panel::Cheats));
}

/// A control of the cheat panel: a page, a choice, a command for the game
/// (order `cheat`), the wait for a place or an object on the map, or
/// «next». Shift makes a step of a choice ten.
fn cheat_control(
    ui: &mut MissionUi,
    scene: &super::scene::Scene,
    control: hud::cheat_panel::CheatControl,
    shift: bool,
) {
    use hud::cheat_panel::{self as panel, CheatControl as C};
    use retzar::gameplay::cheats::Cheat;
    let mission = &scene.mission;
    let players = mission.kingdoms.len().clamp(1, 8);
    let turn = |v: &mut usize, n: usize, step: i8| {
        let step = if shift { step as i64 * 10 } else { step as i64 };
        *v = (*v as i64 + step).rem_euclid(n.max(1) as i64) as usize;
    };
    let c = &mut ui.hud.cheat;
    let cheat = match control {
        C::Tab(tab) => {
            c.tab = tab;
            return;
        }
        C::Resources(times) => Cheat::Resources(times),
        C::NoPop => Cheat::NoPop,
        C::InstantBuild => Cheat::InstantBuild,
        C::NoTech => Cheat::NoTech,
        C::NoFog => Cheat::NoFog,
        C::Reveal => Cheat::Reveal,
        C::Hour(step) => {
            let hour = mission
                .simulation
                .as_ref()
                .map_or(0, |s| s.clock_minutes() / 60) as i64;
            let step = if shift { step as i64 * 10 } else { step as i64 };
            Cheat::Time((hour + step).rem_euclid(24) as u8)
        }
        C::Research(step) => {
            turn(&mut c.research, c.researches.len(), step);
            return;
        }
        C::Discover => {
            if c.research >= c.researches.len() {
                return;
            }
            let name = c.researches.remove(c.research);
            c.research = c.research.min(c.researches.len().saturating_sub(1));
            Cheat::Discover(name)
        }
        C::Hunt => Cheat::Hunt,
        C::Next => {
            ui.next_map = true;
            ui.hud.set_panel(None);
            return;
        }
        C::Point(target) => {
            ui.pointing = Some(target);
            ui.spawning = None;
            ui.target_order = None;
            ui.hud.set_panel(None);
            return;
        }
        C::Kingdom(step) => {
            turn(
                &mut c.kingdom,
                panel::transfer_kingdoms(mission).len(),
                step,
            );
            return;
        }
        C::Civilization(civ) => {
            c.civ = civ;
            return;
        }
        C::Owner(step) => {
            let kingdoms = panel::spawn_kingdoms(mission);
            let owner = panel::owner(&ui.hud, mission);
            let mut at = kingdoms.iter().position(|&k| k == owner).unwrap_or(0);
            turn(&mut at, kingdoms.len(), step);
            ui.hud.cheat.owner = kingdoms[at];
            return;
        }
        C::Spawn(class) => {
            let civ = panel::civilization(&ui.hud, mission);
            let owner = panel::owner(&ui.hud, mission);
            ui.spawning = Some((class, civ, owner));
            ui.pointing = None;
            ui.target_order = None;
            ui.hud.set_panel(None);
            return;
        }
        C::From(step) => {
            turn(&mut c.from, players, step);
            return;
        }
        C::To(step) => {
            turn(&mut c.to, players, step);
            return;
        }
        C::Bit(bit) => {
            let (from, to) = panel::relation_pair(&ui.hud, mission);
            let word = hud::diplomacy_word(&mission.diplomacy, from, to);
            Cheat::Relation {
                from,
                to,
                bit,
                set: word & (1 << bit) == 0,
            }
        }
    };
    ui.requests.push(retzar::gameplay::Request {
        order: "cheat".into(),
        cheat: Some(cheat),
        ..Default::default()
    });
}

/// Selection marks, health bars and the rubber band, drawn like the
/// original: white corner brackets around each selected object, and health
/// bars only while the health key is held. Rebuilt only when something they
/// show changes.
#[derive(Resource, Default)]
pub struct Overlay {
    outlines: Vec<Entity>,
    outline_bounds: Vec<[f32; 4]>,
    bars: Vec<Entity>,
    band: Vec<Entity>,
    selection: Vec<usize>,
    generation: u32,
    /// Health bars: 0 hidden, 1 own kingdom, 2 every kingdom; and the view
    /// they were built for.
    health: u8,
    health_view: [u64; 9],
    /// Ctrl + health key keeps the bars shown.
    health_kept: bool,
    /// Shift + health key switches between the two styles (0xc99dbd); the
    /// key held on the previous frame, for the release.
    health_style: u8,
    health_held: bool,
    /// Group numbers beside the selected objects and what they show.
    digits: Vec<Entity>,
    digit_marks: Vec<(u8, [f32; 4])>,
}
impl Overlay {
    /// Keep the health bars shown, as Ctrl + the health key does.
    pub fn health_kept(&self) -> bool {
        self.health_kept
    }
    pub fn show_health(&mut self, on: bool) {
        self.health_kept = on;
    }
    /// Group numbers shown beside the selection.
    #[cfg(test)]
    pub fn group_marks(&self) -> Vec<u8> {
        self.digit_marks.iter().map(|(n, _)| *n).collect()
    }
    pub fn clear(&mut self, commands: &mut Commands) {
        for e in self
            .outlines
            .drain(..)
            .chain(self.bars.drain(..))
            .chain(self.band.drain(..))
            .chain(self.digits.drain(..))
        {
            commands.entity(e).despawn();
        }
        self.digit_marks.clear();
        self.selection.clear();
        self.outline_bounds.clear();
        self.generation = u32::MAX;
        self.health = 0;
    }
}

fn bar(commands: &mut Commands, out: &mut Vec<Entity>, r: [f32; 4], color: Color, z: f32) {
    if r[2] <= 0. || r[3] <= 0. {
        return;
    }
    out.push(
        commands
            .spawn((
                Sprite::from_color(color, Vec2::new(r[2], r[3])),
                Transform::from_xyz(r[0] + r[2] / 2., r[1] + r[3] / 2., z),
                RenderLayers::layer(1),
            ))
            .id(),
    );
}

/// The original selection mark: six-pixel corner brackets, one pixel thick
/// (map pixels), around the object's selection box. `r` is left, bottom,
/// width, height in world units.
fn brackets(commands: &mut Commands, out: &mut Vec<Entity>, r: [f32; 4], color: Color, z: f32) {
    for piece in bracket_pieces(r) {
        bar(commands, out, piece, color, z);
    }
}
pub(super) fn bracket_pieces(r: [f32; 4]) -> [[f32; 4]; 8] {
    let [l, b, w, h] = r;
    let (lx, ly) = (6f32.min(w), 6f32.min(h));
    [
        [l, b + h - 1., lx, 1.],
        [l + w - lx, b + h - 1., lx, 1.],
        [l, b, lx, 1.],
        [l + w - lx, b, lx, 1.],
        [l, b + h - ly, 1., ly],
        [l + w - 1., b + h - ly, 1., ly],
        [l, b, 1., ly],
        [l + w - 1., b, 1., ly],
    ]
}

fn frame_lines(commands: &mut Commands, out: &mut Vec<Entity>, r: [f32; 4], color: Color, z: f32) {
    let [l, b, w, h] = r;
    for piece in [
        [l, b + h - 1., w, 1.],
        [l, b, w, 1.],
        [l, b, 1., h],
        [l + w - 1., b, 1., h],
    ] {
        bar(commands, out, piece, color, z);
    }
}

pub fn update_overlay(
    mut commands: Commands,
    mut overlay: ResMut<Overlay>,
    loaded: Res<Loaded>,
    ui: Res<MissionUi>,
    view: Res<MapView>,
    (keys, typed, menu): (Res<ButtonInput<KeyCode>>, Res<TypedKeys>, Res<MenuState>),
    mode: Res<State<Mode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut geometry: Query<(&mut Sprite, &mut Transform)>,
    (game, mut images, mut digits): (
        Res<Game>,
        ResMut<Assets<Image>>,
        Local<Vec<(Handle<Image>, usize, usize)>>,
    ),
) {
    let Some(scene) = &loaded.scene else {
        overlay.clear(&mut commands);
        return;
    };
    let surface = Surface::of(&window);
    let selected_bounds = ui
        .hud
        .selected
        .iter()
        .filter_map(|&index| {
            scene
                .visuals
                .iter()
                .find(|v| v.layer == Layer::Body && v.object == index)
                .map(|v| v.bounds)
        })
        .collect::<Vec<_>>();
    if overlay.selection != ui.hud.selected
        || overlay.generation != loaded.generation
        || overlay.outline_bounds.len() != selected_bounds.len()
    {
        for e in overlay.outlines.drain(..) {
            commands.entity(e).despawn();
        }
        let mut out = Vec::new();
        for &index in &ui.hud.selected {
            if let Some(v) = scene
                .visuals
                .iter()
                .find(|v| v.layer == Layer::Body && v.object == index)
            {
                let [r, g, b] = scene.selection_color;
                brackets(
                    &mut commands,
                    &mut out,
                    v.bounds,
                    Color::srgb_u8(r, g, b),
                    420.,
                );
            }
        }
        overlay.outlines = out;
        overlay.selection = ui.hud.selected.clone();
        overlay.outline_bounds = selected_bounds.clone();
        overlay.generation = loaded.generation;
        // The bars follow the objects of the new mission.
        overlay.health_view = [u64::MAX; 9];
    } else if overlay.outline_bounds != selected_bounds {
        // Reuse the eight line entities instead of spawning new ones on every
        // moving frame. Bounds include the sprite's interpolated position.
        for (&entity, r) in overlay
            .outlines
            .iter()
            .zip(selected_bounds.iter().flat_map(|&r| bracket_pieces(r)))
        {
            if let Ok((mut sprite, mut transform)) = geometry.get_mut(entity) {
                sprite.custom_size = Some(Vec2::new(r[2], r[3]));
                transform.translation.x = r[0] + r[2] / 2.;
                transform.translation.y = r[1] + r[3] / 2.;
            }
        }
        overlay.outline_bounds = selected_bounds;
    }

    // 0x4f24c5: a selected object of a numbered group shows the number in
    // `small.fnt`, its top-left corner 8 pixels left of and above the bottom
    // left corner of the selection box.
    let marks: Vec<(u8, [f32; 4])> = ui
        .hud
        .selected
        .iter()
        .filter_map(|&index| {
            let group = ui.groups.iter().position(|g| g.contains(&index))?;
            let v = scene
                .visuals
                .iter()
                .find(|v| v.layer == Layer::Body && v.object == index)?;
            Some((group as u8 + 1, v.bounds))
        })
        .collect();
    if marks != overlay.digit_marks {
        for e in overlay.digits.drain(..) {
            commands.entity(e).despawn();
        }
        if digits.is_empty()
            && let Ok(font) = game
                .assets
                .get("LOCAL/FONTS/SMALL.FNT")
                .and_then(|b| BitmapFont::decode(&b))
        {
            for n in 0..=9 {
                let mut glyph = Bitmap {
                    width: font.width as u32,
                    height: font.height as u32,
                    rgba: vec![0; font.width * font.height * 4],
                };
                let text = n.to_string();
                font.draw(
                    &mut glyph.rgba,
                    font.width,
                    font.height,
                    &text,
                    0,
                    0,
                    [255; 3],
                );
                digits.push((images.add(super::gpu_image(glyph)), font.width, font.height));
            }
        }
        let [r, g, b] = scene.selection_color;
        for &(n, [l, bottom, _, _]) in &marks {
            if let Some((image, w, h)) = digits.get(n as usize) {
                let (w, h) = (*w as f32, *h as f32);
                let mut sprite = Sprite::from_image(image.clone());
                sprite.color = Color::srgb_u8(r, g, b);
                overlay.digits.push(
                    commands
                        .spawn((
                            sprite,
                            Transform::from_xyz(l - 8. + w / 2., bottom + 8. - h / 2., 421.),
                            RenderLayers::layer(1),
                        ))
                        .id(),
                );
            }
        }
        overlay.digit_marks = marks;
    }

    // Health bars (VK_OEM_3, 0x4f641e): the key held shows the own kingdom,
    // with Alt every kingdom. On release Ctrl locks them on for the own
    // kingdom, Shift switches the style, a plain release ends the lock.
    let playing = *mode.get() == Mode::Mission;
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let bars = menu.current_settings().key("health_bars");
    let held = playing && input::held(&keys, &typed, &bars);
    if overlay.health_held && !held {
        if ctrl {
            overlay.health_kept = !overlay.health_kept;
        } else if shift {
            overlay.health_style ^= 1;
        } else {
            overlay.health_kept = false;
        }
    }
    overlay.health_held = held;
    let health = if held {
        1 + alt as u8
    } else if overlay.health_kept {
        1
    } else {
        0
    };
    let key = [
        view.center.x.to_bits() as u64,
        view.center.y.to_bits() as u64,
        view.scale.to_bits() as u64,
        surface.physical.x.to_bits() as u64,
        surface.physical.y.to_bits() as u64,
        surface.dpi.to_bits() as u64,
        scene
            .mission
            .simulation
            .as_ref()
            .map_or(0, |s| s.fog.revision),
        scene.mission.simulation.as_ref().map_or(0, |s| s.ticks),
        overlay.health_style as u64,
    ];
    if health != overlay.health || (health != 0 && key != overlay.health_view) {
        for e in overlay.bars.drain(..) {
            commands.entity(e).despawn();
        }
        let mut out = Vec::new();
        if health != 0 {
            let half = surface.physical / surface.dpi * view.scale / 2. + Vec2::splat(64.);
            let player = scene.mission.player();
            let sim = scene.mission.simulation.as_ref();
            let mut seen = std::collections::HashSet::new();
            for v in scene.visuals.iter().filter(|v| v.layer == Layer::Body) {
                let Some(o) = scene.mission.objects.get(v.object) else {
                    continue;
                };
                if !seen.insert(v.object)
                    || o.max_health == 0
                    || o.health == 0
                    || o.flags & (FLAG_DYING | FLAG_INACTIVE) != 0
                    || (health == 1 && o.owner != player)
                    || sim.is_some_and(|s| !s.visible(o))
                {
                    continue;
                }
                // 0x501040: from the object's pixel position (footprint
                // corner), L = x + 6W and T = y − 24, a bar of 20W pixels.
                let offset = sim.map_or([0., 0.], |s| {
                    retzar::gameplay::motion_offset(s, &scene.mission, v.object)
                });
                let x = v.footprint[0] + offset[0];
                let top = v.footprint[1] + v.footprint[3] + offset[1];
                let tiles = (v.footprint[2] / 32.).round().max(1.);
                let (l, n) = (x + 6. * tiles, 20. * tiles - 1.);
                if (l - view.center.x).abs() > half.x || (top - view.center.y).abs() > half.y {
                    continue;
                }
                let state = sim.and_then(|s| s.objects.get(v.object));
                let level = state.map_or(0, |s| s.level);
                // 0x505450: a cyan mark when the object holds an item (flag
                // 0x400, resource items included).
                let cyan = Color::srgb(0., 1., 1.);
                let marker =
                    state.is_some_and(|s| {
                        s.contents.iter().any(|&k| {
                            scene.mission.objects.get(k).is_some_and(|x| {
                                x.item() || x.class == retzar::world::class::RESITEM
                            })
                        })
                    });
                // Level square: yellow from 4, orange from 8, red from 12.
                let rank = match level {
                    12.. => Some(Color::srgb(1., 0., 0.)),
                    8.. => Some(Color::srgb(1., 0.5, 0.)),
                    4.. => Some(Color::srgb(1., 1., 0.)),
                    1.. if overlay.health_style == 1 => Some(Color::srgb(1., 1., 0.)),
                    _ => None,
                };
                // Rows T, T+1 in map pixels: world y from top − 24 − 2.
                let row = |k: f32| top + 24. - 2. - 2. * k;
                if overlay.health_style == 1 {
                    // Style 1 (0x5013f0): only the level square.
                    if let Some(color) = rank {
                        bar(&mut commands, &mut out, [l, row(0.), 3., 2.], color, 421.5);
                    }
                    if marker {
                        bar(&mut commands, &mut out, [l, row(0.), 3., 2.], cyan, 421.6);
                    }
                    continue;
                }
                // Style 0: health on rows T..T+1, red under green.
                let green = (o.health as f32 * n / o.max_health as f32).floor() + 1.;
                bar(
                    &mut commands,
                    &mut out,
                    [l, row(0.), n + 1., 2.],
                    Color::srgb(1., 0., 0.),
                    421.,
                );
                bar(
                    &mut commands,
                    &mut out,
                    [l, row(0.), green.min(n + 1.), 2.],
                    Color::srgb(0., 1., 0.),
                    421.5,
                );
                if marker {
                    bar(
                        &mut commands,
                        &mut out,
                        [l - 4., row(1.), 3., 2.],
                        cyan,
                        421.5,
                    );
                }
                if let Some(color) = rank {
                    bar(
                        &mut commands,
                        &mut out,
                        [l - 4., row(0.), 3., 2.],
                        color,
                        421.5,
                    );
                }
                // Mana on rows T+2..T+3, blue over red (over cyan when the
                // mark was drawn: the native colour variable is reused).
                if let Some(s) = state.filter(|s| s.max_mana > 0) {
                    let blue = (s.mana as f32 * n / s.max_mana as f32).floor() + 1.;
                    bar(
                        &mut commands,
                        &mut out,
                        [l, row(1.), n + 1., 2.],
                        if marker {
                            cyan
                        } else {
                            Color::srgb(1., 0., 0.)
                        },
                        421.,
                    );
                    if s.mana > 0 {
                        bar(
                            &mut commands,
                            &mut out,
                            [l, row(1.), blue.min(n + 1.), 2.],
                            Color::srgb(0., 0., 1.),
                            421.5,
                        );
                    }
                }
            }
        }
        overlay.bars = out;
        overlay.health = health;
        overlay.health_view = key;
    }

    for e in overlay.band.drain(..) {
        commands.entity(e).despawn();
    }
    if let Some(start) = ui.select_start
        && ui.boxing
        && let Some(p) = window.cursor_position()
    {
        let end = view.world(surface, p);
        let (lo, hi) = (start.min(end), start.max(end));
        let mut band = Vec::new();
        // Above every object, the fog included.
        frame_lines(
            &mut commands,
            &mut band,
            [lo.x, lo.y, hi.x - lo.x, hi.y - lo.y],
            Color::WHITE,
            460.,
        );
        overlay.band = band;
    }
}

/// Native kingdom ramp endpoints (0x501750), rather than the HUD bars.
pub fn owner_color(scene: &super::scene::Scene, colors: &[[u8; 3]; 8], owner: u8) -> [u8; 4] {
    match scene.mission.color_slot(owner) {
        Some(slot) => {
            let [r, g, b] = colors[slot];
            [r, g, b, 255]
        }
        None => NEUTRAL,
    }
}
pub const NEUTRAL: [u8; 4] = [132, 132, 132, 255];
const ITEM_DOT: [u8; 4] = [0, 255, 0, 255];

/// Native property 0x2724: units 2, kingdom buildings 3, loose items 1.
/// Fields, neutral animals, scenery and unworked resource deposits have no
/// marker. Resources acquire a marker through their future mining building.
pub(super) fn minimap_marker(o: &retzar::world::MapObject) -> i32 {
    if o.flags & (FLAG_INACTIVE | FLAG_DYING | 4 | 0x2000) != 0 {
        return 0;
    }
    if o.item() {
        return 1;
    }
    if !(1..=8).contains(&o.owner) {
        return 0;
    }
    if class::is_unit(o.class) {
        return if o.flags & 2 == 0 { 2 } else { 0 };
    }
    if (o.building() && o.class != 63) || o.class == class::RUIN {
        return 3;
    }
    0
}
pub(super) fn minimap_center(
    scene: &super::scene::Scene,
    o: &retzar::world::MapObject,
) -> [usize; 2] {
    let [w, h] = scene.library.cell_footprint(o);
    [o.x as usize + w / 2, o.y as usize + h / 2]
}

/// Draw native-sized markers in logical minimap pixels, then enlarge with
/// nearest sampling: interface scaling must not blur away kingdom colours.
pub fn minimap_image(
    scene: &super::scene::Scene,
    colors: &[[u8; 3]; 8],
    selected: &[usize],
    size: u32,
) -> Bitmap {
    let mut terrain = scene.cells.clone();
    if let Some(sim) = &scene.mission.simulation {
        for y in 0..scene.side {
            for x in 0..scene.side {
                let (seen, visible) = sim.fog.at(x, y);
                let at = (y * scene.side + x) * 4;
                if !seen {
                    terrain.rgba[at..at + 3].fill(0);
                } else if !visible {
                    for c in &mut terrain.rgba[at..at + 3] {
                        *c /= 2;
                    }
                }
            }
        }
    }
    let logical_size = 128;
    let mut b = terrain.resample(logical_size, logical_size);
    let sim = scene.mission.simulation.as_ref();
    for (index, real) in scene.mission.objects.iter().enumerate() {
        // 0x4204b0: the fog covers another kingdom's building (flag 0x2000,
        // no marker, 0x41e402); one seen before leaves its picture in its
        // place (`fakebuilding`, 0x420520), a 2×2 marker (0x420f20) in the
        // colours of the kingdom it had, even once destroyed or taken. The
        // picture of a mine (`fakeresource`, 0x4b0200) is the same object.
        let covered = (real.building() || matches!(real.class, class::RES_GOLD | class::RES_STONE))
            && sim.is_some_and(|s| s.memory.contains_key(&index) && !s.visible(real));
        let (o, dot) = if covered {
            match sim.and_then(|s| s.memory.get(&index)?.snapshot.as_ref()) {
                Some(seen) => (seen, 2),
                None => continue,
            }
        } else {
            (real, minimap_marker(real))
        };
        if dot == 0
            || sim.is_some_and(|s| {
                s.objects
                    .get(index)
                    .is_some_and(|state| state.holder.is_some())
            })
        {
            continue;
        }
        let [cx, cy] = minimap_center(scene, o);
        // 0x46f57e: one of the four corners of the middle cell explored.
        let (known, visible) = sim.map_or((true, true), |s| {
            let mut known = false;
            let mut visible = false;
            for (x, y) in [(cx, cy), (cx + 1, cy), (cx, cy + 1), (cx + 1, cy + 1)] {
                let fog = s.fog.vertex(x, y);
                known |= fog.0;
                visible |= fog.1;
            }
            (known, visible)
        });
        // Units show only in view (0x4e480b), loose items as well.
        if !known || (!visible && (class::is_unit(o.class) || o.item())) {
            continue;
        }
        let x = (cx * logical_size as usize / scene.side) as i32;
        let y = (cy * logical_size as usize / scene.side) as i32;
        let offset = if dot == 3 { -1 } else { 0 };
        // 0x46f61e: a selected unit or building in the colour of the
        // selection frames.
        let color = if o.item() {
            ITEM_DOT
        } else if !covered && selected.contains(&index) {
            let [r, g, b] = scene.selection_color;
            [r, g, b, 255]
        } else {
            owner_color(scene, colors, o.owner)
        };
        for dy in offset..offset + dot {
            for dx in offset..offset + dot {
                let (px, py) = (x + dx, y + dy);
                if px >= 0 && py >= 0 && px < b.width as i32 && py < b.height as i32 {
                    let at = (py as usize * b.width as usize + px as usize) * 4;
                    b.rgba[at..at + 4].copy_from_slice(&color);
                }
            }
        }
    }
    let size = size.max(1);
    if size == logical_size {
        return b;
    }
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let at =
                ((y * logical_size / size * logical_size + x * logical_size / size) * 4) as usize;
            rgba.extend_from_slice(&b.rgba[at..at + 4]);
        }
    }
    Bitmap {
        width: size,
        height: size,
        rgba,
    }
}

/// The minimap and its view rectangle, drawn in interface space.
#[derive(Resource, Default)]
pub struct MinimapState {
    /// Scene generation, texture size, fog revision and selection of the
    /// current texture.
    key: Option<(u32, u32, u64, u64)>,
}

#[allow(clippy::too_many_arguments)]
pub fn update_minimap(
    loaded: Res<Loaded>,
    game: Res<Game>,
    menu: Res<MenuState>,
    view: Res<MapView>,
    ui_entities: Res<UiEntities>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui: Res<MissionUi>,
    mut state: ResMut<MinimapState>,
    mut images: ResMut<Assets<Image>>,
    mut sprites: Query<(&mut Sprite, &mut Transform)>,
) {
    let Some(scene) = &loaded.scene else {
        // Free the last minimap when the mission is closed.
        if state.key.take().is_some()
            && let Ok((mut sprite, _)) = sprites.get_mut(ui_entities.minimap)
        {
            let old = std::mem::take(&mut sprite.image);
            if old != Handle::default() {
                images.remove(old.id());
            }
        }
        return;
    };
    let settings = menu.current_settings();
    let surface = Surface::of(&window);
    let hud = hud_layout(surface, settings);
    let r = hud::map_rect(hud.size.x as i32, hud.size.y as i32, settings.layout == 1);
    let size = (r.w as f32 * hud.factor).round().max(1.) as u32;
    let key = (
        loaded.generation,
        size,
        scene
            .mission
            .simulation
            .as_ref()
            .map_or(0, |s| s.fog.revision),
        {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            ui.hud.selected.hash(&mut h);
            h.finish()
        },
    );
    if state.key != Some(key) {
        let mut image = rgba_image(minimap_image(
            scene,
            &game.classic.minimap_colors,
            &ui.hud.selected,
            size,
        ));
        image.sampler = ImageSampler::nearest();
        let handle = images.add(image);
        if let Ok((mut sprite, _)) = sprites.get_mut(ui_entities.minimap) {
            let old = std::mem::replace(&mut sprite.image, handle);
            if old != Handle::default() {
                images.remove(old.id());
            }
            sprite.custom_size = Some(Vec2::new(r.w as f32, r.h as f32));
        }
        state.key = Some(key);
    }
    if let Ok((_, mut t)) = sprites.get_mut(ui_entities.minimap) {
        let at = hud.place(
            surface,
            Vec2::new(r.x as f32, r.y as f32),
            Vec2::new(r.w as f32, r.h as f32),
        );
        t.translation = at.extend(0.5);
    }
    // View rectangle in minimap pixels, clamped to the minimap.
    let edge = scene.side as f32 * 32.;
    let (min, max) = play_area(surface, &hud, settings, view.frame);
    let a = view.world(surface, min);
    let b = view.world(surface, max);
    let to_map = |p: Vec2| {
        Vec2::new(
            r.x as f32 + (p.x / edge * r.w as f32).clamp(0., r.w as f32),
            r.y as f32 + (-p.y / edge * r.h as f32).clamp(0., r.h as f32),
        )
    };
    let (lo, hi) = (to_map(a), to_map(b));
    let thickness = 1.;
    let lines = [
        (lo, Vec2::new(hi.x - lo.x + thickness, thickness)),
        (
            Vec2::new(lo.x, hi.y),
            Vec2::new(hi.x - lo.x + thickness, thickness),
        ),
        (lo, Vec2::new(thickness, hi.y - lo.y + thickness)),
        (
            Vec2::new(hi.x, lo.y),
            Vec2::new(thickness, hi.y - lo.y + thickness),
        ),
    ];
    for (entity, (at, size)) in ui_entities.viewport.iter().zip(lines) {
        if let Ok((mut sprite, mut t)) = sprites.get_mut(*entity) {
            sprite.custom_size = Some(size);
            t.translation = hud.place(surface, at, size).extend(0.6);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opening_view_and_fit_respect_the_visible_map_area() {
        let surface = Surface {
            physical: Vec2::new(2560., 1440.),
            dpi: 1.,
        };
        let settings = Settings::default();
        let hud = hud_layout(surface, &settings);
        let mut view = MapView::default();
        opening_view(&mut view, surface, &settings, 192, Some([96, 96]));
        assert_eq!(view.magnification(surface), 2.);
        // The start cell is in the middle of the visible map area.
        let (min, max) = play_area(surface, &hud, &settings, view.frame);
        let middle = view.world(surface, (min + max) / 2.);
        assert!((middle - Vec2::new(96. * 32., -96. * 32.)).length() < 1.);
        fit(&mut view, surface, &hud, &settings, 512);
        clamp(&mut view, surface, &hud, &settings, 512);
        let a = view.world(surface, min);
        let b = view.world(surface, max);
        assert!(a.x <= 0. && b.x >= 512. * 32. && a.y >= 0. && b.y <= -512. * 32.);
    }
}

/// Where the view stays in the film framing: while blocked (+0x7e) the view
/// does not answer the player (0x4f6e40), only the scripts and the
/// conversation that block it. The zoom, an addition of this version, stays
/// as it is too.
#[derive(Resource, Default)]
pub struct HeldView(Option<(Vec2, f32)>);
/// The film framing (0x4f9b00): while the view is locked, glides, or a
/// conversation goes on (each blocks the view, +0x7e).
pub fn film(ui: &MissionUi, loaded: &Loaded) -> bool {
    ui.view_lock.is_some()
        || ui.view_scroll.is_some()
        || loaded
            .scene
            .as_ref()
            .and_then(|s| s.mission.simulation.as_ref())
            .is_some_and(|sim| sim.gameplay.script.conversation.is_some())
}
/// Before the player's input: the view to keep.
pub fn hold_view(
    ui: Res<MissionUi>,
    loaded: Res<Loaded>,
    view: Res<MapView>,
    mut held: ResMut<HeldView>,
) {
    held.0 = film(&ui, &loaded).then_some((view.center, view.scale));
}
/// After the player's input: the kept view comes back.
pub fn release_view(held: Res<HeldView>, mut view: ResMut<MapView>) {
    if let Some((center, scale)) = held.0
        && (view.center != center || view.scale != scale)
    {
        view.center = center;
        view.scale = scale;
    }
}
