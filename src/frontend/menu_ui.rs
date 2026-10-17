//! Menu pointer and keyboard handling, actions and background loading.
use super::{
    Game, MenuState, Mode, PendingAction, WindowFit,
    input::PointerFrame,
    mission::{self, MissionUi, Overlay, opening_view},
    scene::{self, Loaded, PreparedScene},
    view::{MapView, Surface, canvas_base, canvas_layout, hud_layout, zoom_levels},
    window_settings,
};
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    },
    prelude::*,
    window::PrimaryWindow,
};
use retzar::{
    Result,
    menu::{Action, Effect, ScrollTarget, Selection},
};
use std::{thread::JoinHandle, time::Instant};

/// Pointer capture between frames: the pressed widget and a dragged scrollbar.
#[derive(Resource, Default)]
pub struct MenuPointer {
    pub down: Option<usize>,
    pub last_cursor: Option<Vec2>,
    pub scroll_drag: Option<(ScrollTarget, f32)>,
    /// The point of the dialog held while it is dragged (0x47ef80).
    pub dialog_grab: Option<(i32, i32)>,
}

/// Value under the pointer of a slider whose 27-pixel thumb runs along its groove.
fn slider_target(b: &retzar::menu::Widget, p: Vec2) -> i16 {
    let thumb = retzar::menu::ARROW as f32;
    (((p.x - b.rect.x as f32 - thumb / 2.) / (b.rect.w as f32 - thumb).max(1.)) * 100.)
        .round()
        .clamp(0., 100.) as i16
}

#[allow(clippy::too_many_arguments)]
pub fn menu_input(
    mut menu: ResMut<MenuState>,
    game: Res<Game>,
    mut pointer: ResMut<MenuPointer>,
    mut pending: ResMut<PendingAction>,
    window: Single<&Window, With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    frame: Res<PointerFrame>,
    mut typed: MessageReader<KeyboardInput>,
) {
    // The keys page waits for a key; the player-name field takes the
    // keyboard while it is being written.
    let capturing = menu.capturing.is_some();
    let editing = menu.editing_name || menu.editing_fps.is_some();
    let naming = menu.screen == retzar::menu::Screen::Save;
    // Alt+Enter belongs to the window (`input::logical_navigation`).
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    for e in typed.read() {
        if e.state != ButtonState::Pressed
            || alt && matches!(e.key_code, KeyCode::Enter | KeyCode::NumpadEnter)
        {
            continue;
        }
        if menu.capturing.is_some() {
            if e.key_code == KeyCode::Escape {
                menu.cancel_capture();
            } else if let Some(name) = super::input::key_name(e) {
                menu.bind_key(&name);
            }
            continue;
        }
        // The save page's name field takes every typed character.
        if naming {
            match &e.logical_key {
                Key::Enter => pending.0 = Some(Action::SaveSlot),
                Key::Backspace => menu.erase_save_name(),
                Key::Escape => {}
                _ => {
                    if let Some(t) = &e.text {
                        menu.type_save_name(t);
                    }
                }
            }
            continue;
        }
        // The frames' field takes digits: Enter keeps them, Esc drops them.
        if menu.editing_fps.is_some() {
            match &e.logical_key {
                Key::Enter => menu.commit_fps(),
                Key::Escape => {
                    menu.editing_fps = None;
                    menu.dirty = true;
                }
                Key::Backspace => menu.erase_fps(),
                _ => {
                    if let Some(t) = &e.text {
                        menu.type_fps(t);
                    }
                }
            }
            continue;
        }
        if !menu.editing_name {
            continue;
        }
        match &e.logical_key {
            Key::Enter | Key::Escape => menu.stop_editing(),
            Key::Backspace => menu.erase_name(),
            _ => {
                if let Some(t) = &e.text {
                    menu.type_name(t);
                }
            }
        }
    }
    if capturing {
        return;
    }
    if !window.focused {
        pointer.down = None;
        pointer.scroll_drag = None;
        pointer.dialog_grab = None;
        if menu.hover.take().is_some() || menu.pressed.take().is_some() {
            menu.dirty = true;
        }
        return;
    }
    let surface = Surface::of(&window);
    let layout = canvas_layout(surface, &menu);
    let base = canvas_base(surface, &menu);
    let widgets = game.skin.widgets(&menu);
    let bars = game.skin.scrollbars(&menu);
    let cursor = window.cursor_position();
    let point = cursor.map(|c| layout.point(surface, c));
    let amount = scroll.delta.y
        * if scroll.unit == MouseScrollUnit::Pixel {
            0.01
        } else {
            1.
        };
    let mut action = None;
    if (cursor != pointer.last_cursor || mouse.just_pressed(MouseButton::Left))
        && menu.keyboard_focus
    {
        menu.keyboard_focus = false;
        menu.dirty = true;
    }
    // Recompute after scrolling, page changes and scale changes even if the cursor is stationary.
    // Grey controls with a hint still show it, but cannot be pressed.
    let hover = point.and_then(|p| {
        widgets
            .iter()
            .position(|b| (b.enabled || b.hint.is_some()) && b.rect.has(p.x, p.y))
    });
    pointer.last_cursor = cursor;
    if hover != menu.hover {
        menu.hover = hover;
        menu.pointer = point.map(|p| [p.x, p.y]);
        menu.dirty = true;
    }
    if mouse.just_pressed(MouseButton::Left) {
        let press_point = frame.press.map(|c| layout.point(surface, c)).or(point);
        let press_hover = press_point.and_then(|p| {
            widgets
                .iter()
                .position(|b| b.enabled && b.rect.has(p.x, p.y))
        });
        pointer.down = press_hover;
        // 0x47ef80: the left button on a dialog's own ground (no control
        // under it) takes the dialog along until it comes up; not in the
        // hint mode, which holds the mouse.
        pointer.dialog_grab = None;
        if !menu.help
            && let Some(p) = press_point
            && !widgets.iter().any(|b| b.rect.has(p.x, p.y))
            && let Some(q) = frame.press.or(cursor).map(|c| base.point(surface, c))
            && let Some(d) = game.skin.drag_rect(&menu)
            && d.has(q.x, q.y)
        {
            pointer.dialog_grab = Some((q.x.floor() as i32 - d.x, q.y.floor() as i32 - d.y));
        }
        if editing
            && press_hover
                .is_none_or(|i| !matches!(widgets[i].action, Action::EditName | Action::EditFps))
        {
            menu.stop_editing();
        }
        pointer.scroll_drag = None;
        if let Some(i) = press_hover {
            menu.focus = i;
            if let Action::Scroll { target, .. } = widgets[i].action {
                if let (Some(bar), Some(p)) =
                    (bars.iter().find(|bar| bar.target == target), press_point)
                {
                    let thumb = bar.thumb();
                    if bar.up().has(p.x, p.y) || bar.down().has(p.x, p.y) {
                        action = Some(Action::Scroll {
                            target,
                            delta: if bar.up().has(p.x, p.y) { -1 } else { 1 },
                            max: bar.max(),
                        });
                    } else if thumb.has(p.x, p.y) {
                        pointer.scroll_drag = Some((target, p.y - thumb.y as f32));
                    } else {
                        action = Some(Action::Scroll {
                            target,
                            delta: if p.y < thumb.y as f32 {
                                -(bar.visible as i16)
                            } else {
                                bar.visible as i16
                            },
                            max: bar.max(),
                        });
                    }
                }
                pointer.down = None;
            }
        }
    }
    // The dialog follows the pointer (0x47f020) while the button is down.
    if !mouse.pressed(MouseButton::Left) {
        pointer.dialog_grab = None;
    } else if let (Some((gx, gy)), Some(c), Some(d)) =
        (pointer.dialog_grab, cursor, game.skin.drag_rect(&menu))
    {
        // In the pixels of the canvas as it opened: over the mission the
        // canvas itself moves, and the screen is the whole window.
        let q = base.point(surface, c);
        let screen = if menu.over_map() {
            let o = -base.origin / base.factor;
            let size = hud_layout(surface, &menu.settings).size;
            retzar::menu::Rect {
                x: o.x.round() as i32,
                y: o.y.round() as i32,
                w: size.x as i32,
                h: size.y as i32,
            }
        } else {
            retzar::menu::Rect {
                x: 0,
                y: 0,
                w: 800,
                h: 600,
            }
        };
        let at = (q.x.floor() as i32 - gx, q.y.floor() as i32 - gy);
        menu.drag_dialog(at, (d.w, d.h), screen);
    }
    if mouse.pressed(MouseButton::Left) || mouse.just_released(MouseButton::Left) {
        if let (Some((target, grab)), Some(p)) = (pointer.scroll_drag, point) {
            if let Some(bar) = bars.iter().find(|bar| bar.target == target) {
                let offset = bar.offset_at(p.y, grab);
                if offset != bar.offset {
                    action = Some(Action::ScrollTo {
                        target,
                        offset,
                        max: bar.max(),
                    });
                }
            }
        } else if let (Some(i), Some(p)) = (pointer.down, point)
            && let Some(b) = widgets.get(i)
            && let (Action::Change(k, _), Some(value)) = (&b.action, b.value)
        {
            let target = slider_target(b, p);
            if target != value as i16 {
                action = Some(Action::Change(*k, target - value as i16));
            }
        }
    }
    if mouse.just_released(MouseButton::Left) {
        if pointer.down == hover
            && let Some(b) = hover.and_then(|i| widgets.get(i))
        {
            action = Some(
                if let (Action::Change(k, _), Some(value), Some(p)) = (&b.action, b.value, point) {
                    Action::Change(*k, slider_target(b, p) - value as i16)
                } else {
                    b.action.clone()
                },
            );
        }
        pointer.down = None;
        pointer.scroll_drag = None;
    }
    let pressed = pointer
        .down
        .filter(|&i| mouse.pressed(MouseButton::Left) && hover == Some(i));
    if menu.pressed != pressed {
        menu.pressed = pressed;
        menu.dirty = true;
    }
    // 0x45c600: in hint mode the right button anywhere ends it.
    if menu.help && mouse.just_pressed(MouseButton::Right) {
        action = Some(Action::Help);
    }
    if !editing && keys.just_pressed(KeyCode::Escape) {
        action = Some(if menu.help {
            Action::Help
        } else {
            Action::Back
        });
    }
    if [
        KeyCode::Enter,
        KeyCode::Space,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::Home,
        KeyCode::End,
    ]
    .iter()
    .any(|&k| keys.just_pressed(k))
        && !menu.keyboard_focus
        && !editing
        && !naming
    {
        if let Some(i) = hover {
            menu.focus = i;
        }
        menu.keyboard_focus = true;
        menu.dirty = true;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let direction = if editing {
        0
    } else if keys.just_pressed(KeyCode::ArrowUp) || (keys.just_pressed(KeyCode::Tab) && shift) {
        -1
    } else if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::Tab) {
        1
    } else {
        0
    };
    let focused_bar = widgets.get(menu.focus).and_then(|b| match b.action {
        Action::Scroll { target, max, .. }
            if menu.keyboard_focus && !keys.just_pressed(KeyCode::Tab) =>
        {
            Some((target, max))
        }
        _ => None,
    });
    if let (true, Some((target, max))) = (direction != 0, focused_bar) {
        action = Some(Action::Scroll {
            target,
            delta: direction as i16,
            max,
        });
    } else if direction != 0 && !widgets.is_empty() {
        for _ in 0..widgets.len() {
            menu.focus =
                (menu.focus as isize + direction).rem_euclid(widgets.len() as isize) as usize;
            if widgets[menu.focus].enabled {
                break;
            }
        }
        menu.keyboard_focus = true;
        menu.dirty = true;
    }
    // While the save name is written, Enter saves and Space is a character.
    if let Some(b) = widgets
        .get(menu.focus)
        .filter(|b| b.enabled && !editing && !naming)
    {
        if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
            action = Some(b.action.clone());
        }
        if let Action::Scroll { target, max, .. } = b.action {
            if keys.just_pressed(KeyCode::Home) {
                action = Some(Action::ScrollTo {
                    target,
                    offset: 0,
                    max,
                });
            }
            if keys.just_pressed(KeyCode::End) {
                action = Some(Action::ScrollTo {
                    target,
                    offset: max,
                    max,
                });
            }
        }
        let direction = if keys.just_pressed(KeyCode::ArrowLeft) {
            -1
        } else if keys.just_pressed(KeyCode::ArrowRight) {
            1
        } else {
            0
        };
        if direction != 0 {
            action = match b.action {
                Action::Change(k, _) => Some(Action::Change(
                    k,
                    if b.value.is_some() {
                        direction * 10
                    } else {
                        direction
                    },
                )),
                Action::RandomSize(_) => Some(Action::RandomSize(direction)),
                Action::RandomTerrain(_) => Some(Action::RandomTerrain(direction)),
                Action::RandomOption(k, _) => Some(Action::RandomOption(k, direction)),
                Action::SetupOption(k, _) => Some(Action::SetupOption(k, direction)),
                Action::SetupRace(i, _) => Some(Action::SetupRace(i, direction)),
                Action::Difficulty(_) => Some(Action::Difficulty(direction)),
                _ => action,
            };
        }
    }
    let page = if keys.just_pressed(KeyCode::PageDown) {
        6
    } else if keys.just_pressed(KeyCode::PageUp) {
        -6
    } else {
        0
    };
    if amount != 0. || page != 0 {
        let target = if page != 0 {
            focused_bar.map(|(target, _)| target)
        } else {
            None
        }
        .or_else(|| {
            bars.iter()
                .find(|bar| point.is_some_and(|p| bar.track.has(p.x, p.y)))
                .map(|bar| bar.target)
        })
        .or_else(|| {
            (0..2)
                .find(|&panel| {
                    game.skin
                        .description(&menu, panel)
                        .is_some_and(|(_, rect, _)| point.is_some_and(|p| rect.has(p.x, p.y)))
                })
                .map(ScrollTarget::Description)
        })
        .unwrap_or(ScrollTarget::List);
        if let Some(bar) = bars.iter().find(|bar| bar.target == target) {
            action = Some(Action::Scroll {
                target,
                delta: (-amount.round() as i32 + page).clamp(i16::MIN as i32, i16::MAX as i32)
                    as i16,
                max: bar.max(),
            });
        }
        // The settings: the wheel on a slider moves it, on a selector it
        // gives the next or the previous value.
        if amount != 0.
            && let Some(b) = hover.and_then(|i| widgets.get(i)).filter(|b| b.enabled)
            && let Action::Change(key, delta) = b.action
        {
            let sign = amount.signum() as i16;
            match b.style {
                retzar::menu::Style::Slider => action = Some(Action::Change(key, 5 * sign)),
                retzar::menu::Style::Arrow { .. } => {
                    action = Some(Action::Change(key, delta.abs() * sign))
                }
                _ => {}
            }
        }
    }
    if action.is_some() {
        pending.0 = action;
    }
}

/// Mission being prepared on a background thread.
#[derive(Resource)]
pub struct LoadJob {
    handle: Option<JoinHandle<Result<PreparedScene>>>,
    /// Saved camera to restore once the mission is on screen.
    camera: Option<[f32; 3]>,
    title: String,
    started: Instant,
}
pub fn loading(job: Option<Res<LoadJob>>) -> bool {
    job.is_some()
}
pub fn not_loading(job: Option<Res<LoadJob>>) -> bool {
    job.is_none()
}

/// What a load needs: the scenario, its title and the setup-screen choices.
type Request = (
    Option<retzar::scenario::Scenario>,
    String,
    Option<retzar::world::GameSetup>,
);
fn request(world: &mut World, selection: Selection) -> Result<Request> {
    let menu = world.resource::<MenuState>();
    Ok(match &selection {
        Selection::Scenario { .. } => {
            let s = menu
                .scenario(&selection)
                .ok_or("Missione non trovata")?
                .clone();
            let title = s.title.clone();
            // The colours and teams chosen for the game being opened.
            (Some(s), title, menu.setup.clone())
        }
        Selection::Random { seed, .. } => (
            None,
            retzar::lang::fill("Mappa casuale {}", &[&seed]),
            menu.setup.clone(),
        ),
    })
}

/// Start preparing a mission without blocking the window.
pub fn start_load(world: &mut World, selection: Selection, camera: Option<[f32; 3]>) -> Result<()> {
    if world.contains_resource::<LoadJob>() {
        return Ok(());
    }
    let (scenario, title, setup) = request(world, selection.clone())?;
    // A campaign mission opened has been reached.
    if let (
        Some(s),
        Selection::Scenario {
            campaign: Some(_), ..
        },
    ) = (&scenario, &selection)
    {
        world.resource_mut::<MenuState>().reach(&s.id);
    }
    let assets = world.resource::<Game>().assets.clone();
    let handle = std::thread::Builder::new()
        .name("Caricamento missione".into())
        .spawn(move || scene::prepare(&assets, selection, scenario, setup))?;
    let mut menu = world.resource_mut::<MenuState>();
    menu.notice = retzar::lang::fill("Caricamento di «{}»…", &[&title]);
    menu.dirty = true;
    world.insert_resource(LoadJob {
        handle: Some(handle),
        camera,
        title,
        started: Instant::now(),
    });
    Ok(())
}

fn start_restore(world: &mut World, saved: retzar::menu::SavedSession) -> Result<()> {
    if !saved.snapshot {
        return start_load(world, saved.selection, Some(saved.camera));
    }
    if world.contains_resource::<LoadJob>() {
        return Ok(());
    }
    let path = retzar::save::save_path(&world.resource::<MenuState>().root, &saved.file)?;
    let assets = world.resource::<Game>().assets.clone();
    let handle = std::thread::Builder::new()
        .name("Ripristino sessione".into())
        .spawn(move || {
            let mut prepared = scene::prepare_snapshot(&assets, &path)?;
            prepared.selection = saved.selection;
            Ok(prepared)
        })?;
    world.resource_mut::<MenuState>().notice =
        retzar::lang::fill("Ripristino di «{}»…", &[&saved.title]);
    world.resource_mut::<MenuState>().dirty = true;
    world.insert_resource(LoadJob {
        handle: Some(handle),
        camera: Some(saved.camera),
        title: saved.title,
        started: Instant::now(),
    });
    Ok(())
}

#[derive(Resource)]
pub struct SaveJob {
    handle: Option<JoinHandle<Result<retzar::menu::SavedSession>>>,
}
impl Drop for SaveJob {
    fn drop(&mut self) {
        // A window close may bypass the menu's Exit action. Finish the captured
        // write before process teardown; never abandon an acknowledged request.
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
/// Writes the open game into `file`, or a new file, under `name`; an empty
/// name lists the save under the game's title.
pub fn start_save(world: &mut World, file: Option<String>, name: String) -> Result<()> {
    if world.contains_resource::<SaveJob>() {
        return Err("Un salvataggio è già in corso".into());
    }
    let view = *world.resource::<MapView>();
    let menu = world.resource::<MenuState>();
    let scene = world
        .resource::<Loaded>()
        .scene
        .as_ref()
        .ok_or("Nessuna mappa aperta")?;
    let ui = world.resource::<MissionUi>();
    let meta = retzar::menu::SavedSession {
        version: retzar::menu::SAVE_VERSION,
        snapshot: true,
        title: scene.title.clone(),
        name,
        file: file.unwrap_or_else(|| retzar::save::new_file(&menu.root)),
        scenario: scene.scenario_id.clone(),
        selection: scene.selection.clone(),
        camera: [view.center.x, view.center.y, view.scale],
        setup: menu.setup.clone(),
        missing: false,
    };
    let path = retzar::save::save_path(&menu.root, &meta.file)?;
    let mut mission = scene.mission.clone();
    let simulation = mission
        .simulation
        .take()
        .ok_or("Simulazione non inizializzata")?;
    let snapshot = retzar::save::Snapshot {
        language: retzar::lang::current().index(),
        schema: 1,
        grid: scene.grid.clone(),
        biome: scene.biome.clone(),
        title: scene.title.clone(),
        description: scene.description.clone(),
        starting_positions: mission.starting_positions,
        mission,
        simulation,
        source_entries: scene.source_entries.clone(),
        external_entries: Default::default(),
        portraits: scene.portraits.clone().into_iter().collect(),
        hud: ui.hud.clone(),
        groups: ui.groups.clone(),
        idle: ui.idle,
        health_kept: world.resource::<Overlay>().health_kept(),
        visual_time: scene.visual_time,
        flyers: scene.flights.iter().map(|f| f.flyer.clone()).collect(),
    };
    let handle = std::thread::Builder::new()
        .name("Salvataggio sessione".into())
        .spawn(move || {
            retzar::save::write(&path, &meta, &snapshot)?;
            Ok(meta)
        })?;
    world.insert_resource(SaveJob {
        handle: Some(handle),
    });
    // 0x4f5aab: «saving» while it writes, «quicksaved» after (0x4f5b1c).
    let text = world.resource::<Game>().skin.locale.label("vm", "saving");
    let mut menu = world.resource_mut::<MenuState>();
    menu.notice = text;
    menu.dirty = true;
    Ok(())
}
pub fn finish_save(world: &mut World) {
    if !world
        .get_resource::<SaveJob>()
        .is_some_and(|j| j.handle.as_ref().is_none_or(|h| h.is_finished()))
    {
        return;
    }
    let mut job = world.remove_resource::<SaveJob>().unwrap();
    let saved = world
        .resource::<Game>()
        .skin
        .locale
        .label("viewmsg", "quicksaved");
    let result = job
        .handle
        .take()
        .ok_or("Salvataggio interrotto")
        .and_then(|h| h.join().map_err(|_| "Salvataggio interrotto"));
    let mut menu = world.resource_mut::<MenuState>();
    match result {
        Ok(Ok(meta)) => {
            menu.refresh_saves();
            menu.save = menu.saves.iter().position(|s| s.file == meta.file);
            menu.notice = saved;
        }
        Ok(Err(e)) => menu.notice = retzar::lang::fill("Salvataggio non riuscito: {}", &[&e]),
        Err(e) => menu.notice = e.into(),
    }
    menu.dirty = true;
}

/// Prepare and install on the calling thread.
#[cfg(test)]
pub fn load_now(world: &mut World, selection: Selection, camera: Option<[f32; 3]>) -> Result<()> {
    let (scenario, _, setup) = request(world, selection.clone())?;
    let assets = world.resource::<Game>().assets.clone();
    let prepared = scene::prepare(&assets, selection, scenario, setup)?;
    install_prepared(world, prepared, camera);
    Ok(())
}

/// Collect a finished background load and put it on screen.
pub fn finish_load(world: &mut World) {
    let finished = world
        .get_resource::<LoadJob>()
        .is_some_and(|job| job.handle.as_ref().is_none_or(|h| h.is_finished()));
    if !finished {
        return;
    }
    let mut job = world.remove_resource::<LoadJob>().unwrap();
    let result = match job.handle.take().map(JoinHandle::join) {
        Some(Ok(result)) => result,
        Some(Err(_)) | None => Err("Il caricamento si è interrotto".into()),
    };
    match result {
        Ok(prepared) => {
            install_prepared(world, prepared, job.camera);
            info!(
                "Missione «{}» pronta in {:.2} s",
                job.title,
                job.started.elapsed().as_secs_f32()
            );
        }
        Err(e) => {
            let previous = world
                .resource::<Loaded>()
                .scene
                .as_ref()
                .map(|s| s.selection.clone());
            let mut menu = world.resource_mut::<MenuState>();
            menu.selection = previous;
            menu.notice = format!("Impossibile aprire «{}»: {e}", job.title);
            menu.dirty = true;
        }
    }
}

/// Replace the open mission with a prepared one and show it.
pub fn install_prepared(world: &mut World, mut prepared: PreparedScene, camera: Option<[f32; 3]>) {
    let restored = prepared.restored.take();
    let is_snapshot = restored.is_some();
    let camera = restored.as_ref().map(|r| r.metadata.camera).or(camera);
    let old = world.resource_mut::<Loaded>().scene.take();
    if let Some(old) = old {
        scene::release(world, old);
        super::effects::release(world);
    }
    let mut overlay = std::mem::take(&mut *world.resource_mut::<Overlay>());
    overlay.clear(&mut world.commands());
    world.flush();
    let mut scene = scene::install(world, prepared);
    let surface = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .map(Surface::of)
        .unwrap_or(Surface {
            physical: Vec2::new(1280., 720.),
            dpi: 1.,
        });
    let settings = world.resource::<MenuState>().current_settings().clone();
    let mut view = MapView::default();
    let start = scene.mission.home();
    opening_view(&mut view, surface, &settings, scene.side, start);
    if let Some(c) = camera.filter(|c| c.iter().all(|v| v.is_finite())) {
        let edge = scene.side as f32 * 32.;
        view.center = Vec2::new(c[0].clamp(0., edge), c[1].clamp(-edge, 0.));
        // A saved zoom snaps to the nearest of the current levels, so views
        // saved with other levels (or another screen) stay on the wheel's steps.
        view.scale = c[2].clamp(0.02, 32.);
        let hud = hud_layout(surface, &settings);
        let m = view.magnification(surface);
        // On a tie the middle level wins.
        let [low, middle, high] = zoom_levels(&hud);
        let nearest = [middle, low, high]
            .into_iter()
            .min_by(|a, b| (a - m).abs().total_cmp(&(b - m).abs()))
            .unwrap_or(m);
        if !is_snapshot {
            view.set_magnification(surface, nearest);
        }
    }
    world.insert_resource(view);
    // The story of a new mission is opened by its script (`ShowStoryScreen`).
    let mut ui = MissionUi::default();
    let mut restore_notice = String::new();
    if let Some(r) = restored {
        ui.hud = r.hud;
        super::environment::restore_diplomacy(&mut scene, &ui.hud);
        ui.paged = ui.hud.selected.clone();
        ui.groups = r.groups;
        ui.idle = r.idle;
        scene.visual_time = r.visual_time;
        for (f, state) in scene.flights.iter_mut().zip(r.flyers) {
            f.flyer = state;
        }
        scene::refresh_visuals(world, &mut scene);
        world.resource_mut::<Overlay>().show_health(r.health_kept);
        world.resource_mut::<MenuState>().setup = r.metadata.setup;
        if r.recovered {
            restore_notice =
                retzar::lang::t("Recuperata la precedente copia valida del salvataggio.").into();
        }
    } else {
        if let Some(carry) = world.remove_resource::<CarryOver>()
            && let Some(mut sim) = scene.mission.simulation.take()
        {
            retzar::gameplay::restore_heroes(
                &mut sim,
                &mut scene.mission,
                &scene.library,
                &carry.heroes,
            );
            sim.gameplay.script.variables.kept = carry.kept;
            scene.mission.simulation = Some(sim);
        }
        world.resource_mut::<Overlay>().show_health(false);
        if camera.is_some() {
            restore_notice = retzar::lang::t(
                "Salvataggio precedente: ripristinata la vista iniziale, senza stato della simulazione.",
            )
            .into();
        }
    }
    world.insert_resource(ui);
    let selection = scene.selection.clone();
    {
        let mut loaded = world.resource_mut::<Loaded>();
        loaded.scene = Some(scene);
        loaded.generation = loaded.generation.wrapping_add(1);
    }
    world.resource_scope(|world, mut ui: Mut<MissionUi>| {
        mission::scroll_panel(
            &mut ui,
            world.resource::<Loaded>(),
            world.resource::<Game>(),
            0,
        );
    });
    let mut menu = world.resource_mut::<MenuState>();
    menu.selection = Some(selection);
    menu.notice = restore_notice;
    menu.dirty = true;
    world.resource_mut::<NextState<Mode>>().set(Mode::Mission);
}

/// Close the open mission and free its resources.
pub fn close_scene(world: &mut World) {
    let old = world.resource_mut::<Loaded>().scene.take();
    if let Some(old) = old {
        scene::release(world, old);
        super::effects::release(world);
        let mut overlay = std::mem::take(&mut *world.resource_mut::<Overlay>());
        overlay.clear(&mut world.commands());
        world.flush();
        world.resource_mut::<MenuState>().selection = None;
    }
}

/// Apply the action chosen this frame by the menu or the mission keys.
pub fn apply_pending(world: &mut World) {
    let Some(a) = world.resource_mut::<PendingAction>().0.take() else {
        return;
    };
    if let Err(e) = action(world, a) {
        let mut menu = world.resource_mut::<MenuState>();
        menu.notice = e.to_string();
        menu.dirty = true;
    }
}

fn action(world: &mut World, a: Action) -> Result<()> {
    if world.contains_resource::<SaveJob>()
        && matches!(
            a,
            Action::LoadSlot
                | Action::DeleteSlot
                | Action::SaveSlot
                | Action::QuickSave
                | Action::QuickLoad
                | Action::Exit
        )
    {
        return Err("Attendi il completamento del salvataggio".into());
    }
    if !world.resource::<MenuState>().allows_action(&a) {
        return Ok(());
    }
    let effect = world.resource_mut::<MenuState>().activate(a)?;
    match effect {
        Effect::None => {}
        Effect::Exit => {
            world.write_message(AppExit::Success);
        }
        Effect::SettingsChanged => {
            if let Err(e) = super::switch_language(world) {
                world.resource_mut::<MenuState>().notice = e.to_string();
            }
            // The page's while it is open, else the kept ones.
            let settings = world.resource::<MenuState>().current_settings().clone();
            let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
            if let Ok(mut w) = windows.single_mut(world) {
                window_settings(&mut w, &settings);
            }
            world.insert_resource(WindowFit::default());
            // A speed moved in Options goes to the open game (0x47d121);
            // with no game it is lost, the next map starting at the normal.
            let game = world
                .resource::<Loaded>()
                .scene
                .as_ref()
                .and_then(|s| s.mission.simulation.as_ref())
                .map(|sim| sim.speed);
            if game.is_some_and(|speed| speed != settings.speed) {
                super::gameplay::change_speed(world, settings.speed);
            }
            // The cheat panel of the open game: switched on, it opens.
            if world.resource::<Loaded>().scene.is_some() {
                super::mission::switch_cheats(world, settings.cheats);
            }
        }
        Effect::Load(s) => start_load(world, s, None)?,
        Effect::Restore(saved) => start_restore(world, saved)?,
        Effect::Save { file, name } => start_save(world, file, name)?,
        Effect::Resume => {
            if world.resource::<Loaded>().scene.is_some() {
                world.resource_mut::<NextState<Mode>>().set(Mode::Mission);
            }
        }
        Effect::Restart => {
            let selection = world
                .resource::<Loaded>()
                .scene
                .as_ref()
                .map(|s| s.selection.clone());
            if let Some(s) = selection {
                start_load(world, s, None)?;
            }
        }
        Effect::NextMap => next_map(world)?,
    }
    Ok(())
}

/// What a campaign takes to its next map (0x43ea20): the named heroes,
/// matched by name, and the `#` variables.
#[derive(Resource, Default)]
pub struct CarryOver {
    pub heroes: Vec<retzar::gameplay::Hero>,
    pub kept: std::collections::BTreeMap<String, i32>,
}

/// «CONTINUA»: open the map `LoadMap` chose, in the same campaign.
pub(super) fn next_map(world: &mut World) -> Result<()> {
    let Some(scene) = world.resource::<Loaded>().scene.as_ref() else {
        return Ok(());
    };
    let Some(sim) = scene.mission.simulation.as_ref() else {
        return Ok(());
    };
    let Some(path) = sim.gameplay.script.next_map.clone() else {
        return Ok(());
    };
    let Selection::Scenario {
        campaign: Some(c), ..
    } = scene.selection
    else {
        return Err("La mappa successiva appartiene a una campagna".into());
    };
    let wanted = path.replace('\\', "/");
    let mission = world.resource::<MenuState>().catalog.campaigns[c]
        .missions
        .iter()
        .position(|m| {
            m.nested
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(&wanted))
        })
        .ok_or_else(|| format!("Mappa successiva non trovata: {path}"))?;
    let carry = CarryOver {
        heroes: retzar::gameplay::heroes(sim, &scene.mission),
        kept: sim.gameplay.script.variables.kept.clone(),
    };
    world.insert_resource(carry);
    start_load(
        world,
        Selection::Scenario {
            campaign: Some(c),
            mission,
        },
        None,
    )
}
