//! Native waypoint/rally artwork and targeting feedback.
use super::{
    Game, MenuState, Mode, gpu_image,
    mission::MissionUi,
    scene::Loaded,
    view::{MapView, Surface, hud_layout},
};
use bevy::{camera::visibility::RenderLayers, prelude::*, window::PrimaryWindow};
use retzar::{bitmap::Bitmap, gameplay::Intent, hud, sprite::SpriteSheet, world::Layer};
use std::collections::BTreeMap;

pub struct Feedback {
    pub cell: [u16; 2],
    /// The waypoint flag of a Shift order given to units (0x4351c1); a
    /// plain order or a building's rally point shows none.
    pub flag: bool,
    /// The object that blinks (0x5008c0) and in which colour.
    pub target: Option<usize>,
    pub color: [u8; 3],
    /// Where the red cross of a click on the ground shrinks (0x490627).
    pub cross: Option<[f32; 2]>,
    pub started: f32,
    pub generation: u32,
}
/// The colours of 0x43b240 the orders blink in: red (index 12) for an
/// attack, a capture and the command buttons, green (14) for the other
/// right clicks, blue (13) for a guard.
pub const RED: [u8; 3] = [255, 0, 0];
pub const GREEN: [u8; 3] = [0, 255, 0];
pub const BLUE: [u8; 3] = [0, 0, 255];
struct Flag {
    frames: Vec<Handle<Image>>,
    size: Vec2,
    hotspot: Vec2,
    period: f32,
}
#[derive(Resource, Default)]
pub struct Indicators {
    /// Flag animations by key and kingdom colour slot.
    flags: BTreeMap<(&'static str, Option<usize>), Flag>,
    /// The rally flag of each selected building: its cell and since when it
    /// shows. Natively a new object made when the building is selected or
    /// its point moves (0x41d7e0, 0x41e91f), so its animation starts over.
    rally: BTreeMap<usize, ([u16; 2], f32)>,
    entities: Vec<Entity>,
    pub cursor: &'static str,
}
#[cfg(test)]
impl Indicators {
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }
    pub fn rally_since(&self, i: usize) -> Option<([u16; 2], f32)> {
        self.rally.get(&i).copied()
    }
}
struct Mark {
    image: Option<Handle<Image>>,
    size: Vec2,
    centre: Vec2,
    color: Color,
}

fn flag(
    world: &mut World,
    state: &mut Indicators,
    key: &'static str,
    slot: Option<usize>,
) -> retzar::Result<()> {
    if state.flags.contains_key(&(key, slot)) {
        return Ok(());
    }
    let bitmaps = world.resource_scope(|_, game: Mut<Game>| -> retzar::Result<_> {
        let ini = retzar::assets::Ini::parse(&game.assets.get("DATA/IMAGES.INI")?);
        let file = format!(
            "IMAGES/{}",
            ini.get(key, "image").unwrap_or_default().replace('\\', "/")
        );
        let rle = format!(
            "{}.rle",
            file.rsplit_once('.').map_or(file.as_str(), |(s, _)| s)
        );
        let data = game.assets.get(if game.assets.contains(&rle) {
            &rle
        } else {
            &file
        })?;
        let rows = ini
            .get(key, "rows")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1);
        let frames = if data.starts_with(b"BM") {
            let b = Bitmap::decode_for(&data, true, slot)?;
            (0..rows)
                .map(|i| {
                    b.crop(
                        0,
                        i as u32 * b.height / rows as u32,
                        b.width,
                        b.height / rows as u32,
                    )
                })
                .collect::<retzar::Result<Vec<_>>>()?
        } else {
            let sheet = SpriteSheet::decode_for(&data, slot)?;
            (0..sheet.rows)
                .map(|i| sheet.frame(0, i))
                .collect::<retzar::Result<Vec<_>>>()?
        };
        let hot = ini
            .get(key, "hotspot")
            .unwrap_or("0,0")
            .split(',')
            .filter_map(|n| n.trim().parse::<f32>().ok())
            .collect::<Vec<_>>();
        let hotspot = Vec2::new(*hot.first().unwrap_or(&0.), *hot.get(1).unwrap_or(&0.));
        let fps = ini
            .get(key, "fps")
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(5.)
            .max(1.);
        Ok((frames, hotspot, 1. / fps))
    })?;
    let size = Vec2::new(bitmaps.0[0].width as f32, bitmaps.0[0].height as f32);
    let frames = bitmaps
        .0
        .into_iter()
        .map(|b| world.resource_mut::<Assets<Image>>().add(gpu_image(b)))
        .collect();
    state.flags.insert(
        (key, slot),
        Flag {
            frames,
            size,
            hotspot: bitmaps.1,
            period: bitmaps.2,
        },
    );
    Ok(())
}
fn add_flag(
    marks: &mut Vec<Mark>,
    state: &Indicators,
    (key, slot): (&'static str, Option<usize>),
    cell: [u16; 2],
    t: f32,
) {
    let Some(flag) = state.flags.get(&(key, slot)) else {
        return;
    };
    // 0x41d7e0 sets the rally flag on the cell's corner; the waypoint flags
    // on its centre (0x4351c1).
    let centre = if key == "gath_flag" { 0. } else { 16. };
    let anchor = Vec2::new(
        cell[0] as f32 * 32. + centre,
        -(cell[1] as f32 * 32. + centre),
    );
    marks.push(Mark {
        image: Some(flag.frames[(t / flag.period) as usize % flag.frames.len()].clone()),
        size: flag.size,
        centre: anchor
            + Vec2::new(
                flag.size.x / 2. - flag.hotspot.x,
                flag.hotspot.y - flag.size.y / 2.,
            ),
        color: Color::WHITE,
    });
}
/// 0x4f2528: a blinking object shows a one-pixel frame in its colour, one
/// pixel inside the rectangle of its selection.
fn add_blink(marks: &mut Vec<Mark>, bounds: [f32; 4], color: [u8; 3]) {
    let [l, b, w, h] = bounds;
    let (l, b, w, h) = (l + 1., b + 1., (w - 2.).max(1.), (h - 2.).max(1.));
    for r in [
        [l, b + h - 1., w, 1.],
        [l, b, w, 1.],
        [l, b, 1., h],
        [l + w - 1., b, 1., h],
    ] {
        marks.push(Mark {
            image: None,
            size: Vec2::new(r[2], r[3]),
            centre: Vec2::new(r[0] + r[2] / 2., r[1] + r[3] / 2.),
            color: Color::srgb_u8(color[0], color[1], color[2]),
        });
    }
}
pub fn update(world: &mut World) {
    let mut state = world.remove_resource::<Indicators>().unwrap_or_default();
    let mut marks = Vec::new();
    state.cursor = "";
    let playing = world
        .get_resource::<State<Mode>>()
        .is_none_or(|m| *m.get() == Mode::Mission)
        && world.resource::<Loaded>().scene.is_some();
    if !playing {
        state.rally.clear();
        for (_, flag) in std::mem::take(&mut state.flags) {
            for h in flag.frames {
                world.resource_mut::<Assets<Image>>().remove(h.id());
            }
        }
    }
    if playing {
        // 0x41d889: the rally flag takes the colour of the player's kingdom.
        let slot = world
            .resource::<Loaded>()
            .scene
            .as_ref()
            .and_then(|s| s.mission.color_slot(s.mission.player()));
        for (key, slot) in [("wp_flag", None), ("gath_flag", slot), ("cursor", None)] {
            if let Err(e) = flag(world, &mut state, key, slot) {
                world.resource_mut::<MenuState>().notice = format!("Indicatore {key}: {e}");
            }
        }
        let time = world.resource::<Time>().elapsed_secs();
        let window = {
            let mut q = world.query_filtered::<&Window, With<PrimaryWindow>>();
            q.iter(world).next().cloned()
        };
        let ui = world.resource::<MissionUi>();
        let loaded = world.resource::<Loaded>();
        let mut rally = BTreeMap::new();
        if let Some(scene) = &loaded.scene {
            if let Some(sim) = &scene.mission.simulation {
                for &i in &ui.hud.selected {
                    if let Some(actor) = sim.gameplay.actors.get(i) {
                        // Only the player's own building shows it (0x41d8b0).
                        if scene.mission.objects[i].building()
                            && scene.mission.objects[i].owner == scene.mission.player()
                            && let Some(cell) = actor.rally
                        {
                            let since = state
                                .rally
                                .get(&i)
                                .filter(|(at, _)| *at == cell)
                                .map_or(time, |&(_, since)| since);
                            rally.insert(i, (cell, since));
                            add_flag(&mut marks, &state, ("gath_flag", slot), cell, time - since);
                        }
                        let keys = world.resource::<ButtonInput<KeyCode>>();
                        if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
                            for intent in &actor.queued {
                                if let Intent::Move(cell)
                                | Intent::Chop(cell)
                                | Intent::AttackGround(cell) = intent
                                {
                                    add_flag(&mut marks, &state, ("wp_flag", None), *cell, time);
                                }
                            }
                        }
                    }
                }
            }
            state.rally = std::mem::take(&mut rally);
            if let Some(f) = &ui.order_feedback
                && f.generation == loaded.generation
            {
                let age = time - f.started;
                if f.flag && age < 1.2 {
                    add_flag(&mut marks, &state, ("wp_flag", None), f.cell, age);
                }
                // 0x5008c0 → 0x502a3b: on at 0, 200 and 400 ms, 100 ms each.
                if age < 0.5
                    && (age * 10.) as i32 % 2 == 0
                    && let Some(v) = scene
                        .visuals
                        .iter()
                        .find(|v| v.layer == Layer::Body && Some(v.object) == f.target)
                {
                    add_blink(&mut marks, v.bounds, f.color);
                }
                // 0x490627: `cursor`, ten frames in 600 ms on the point.
                if let Some(p) = f.cross
                    && age < 0.6
                    && let Some(cross) = state.flags.get(&("cursor", None))
                {
                    let frame = ((age / 0.06) as usize).min(cross.frames.len() - 1);
                    marks.push(Mark {
                        image: Some(cross.frames[frame].clone()),
                        size: cross.size,
                        centre: Vec2::new(p[0], -p[1])
                            + Vec2::new(
                                cross.size.x / 2. - cross.hotspot.x,
                                cross.hotspot.y - cross.size.y / 2.,
                            ),
                        color: Color::WHITE,
                    });
                }
            }
            if ui.hud.panel.is_none() {
                if let Some(w) = window
                    && let Some(cursor) = w.cursor_position()
                {
                    let surface = Surface::of(&w);
                    let settings = &world.resource::<MenuState>().settings;
                    let layout = hud_layout(surface, settings);
                    let p = layout.point(surface, cursor);
                    if !hud::blocks_map(
                        &ui.hud,
                        [p.x, p.y],
                        hud::Layout {
                            width: layout.size.x as i32,
                            height: layout.size.y as i32,
                            left: settings.layout == 1,
                        },
                    ) {
                        let p = world.resource::<MapView>().world(surface, cursor);
                        if let Some(key) = &ui.target_order {
                            let action = world.resource::<Game>().gameplay.action(key);
                            state.cursor =
                                match action.split_whitespace().next().unwrap_or_default() {
                                    "attack" | "murder" => "cs_attack",
                                    "attack_ground" => "cs_attackset",
                                    "move" | "move_advance" | "move_sneak" | "unload" => "cs_move",
                                    "harvest" | "chop" | "fishing" => "cs_harvest",
                                    "repair" | "capture" | "build" => "cs_repair",
                                    "guard" => "cs_guard",
                                    // 0x43a8c1: the table loads no
                                    // `cs_patrol`; ids 4 and 6 are both
                                    // `cs_patrol_ex`.
                                    "patrol" | "explore" => "cs_patrol_ex",
                                    "pick_item" => "cs_pick",
                                    "spell" | "spell_item" => "cs_spell",
                                    _ => "",
                                };
                        }
                        // 0x4f45b0: over the map the cursor is that of the
                        // right click the first selected unit would give
                        // there (vt+0x190 → 0x42da60); the arrow on a cell
                        // nobody explored.
                        let sim = scene.mission.simulation.as_ref();
                        let hovered = retzar::selection::pointed(
                            &scene.mission,
                            &scene.library,
                            &scene.visuals,
                            [p.x, p.y],
                            |_, o| {
                                o.class != retzar::world::class::DEBRIS
                                    && sim.is_none_or(|s| s.visible(o))
                            },
                        );
                        let cell = (p.x >= 0. && p.y <= 0.)
                            .then(|| [(p.x / 32.) as usize, (-p.y / 32.) as usize])
                            .filter(|&[x, y]| x < scene.side && y < scene.side);
                        if ui.target_order.is_none()
                            && let Some(&i) = ui.hud.selected.first()
                            && scene.mission.commands_at(i)
                            && let Some(sim) = scene.mission.simulation.as_ref()
                            && let Some([x, y]) = cell
                            && sim.fog.at(x, y).0
                        {
                            let keys = world.resource::<ButtonInput<KeyCode>>();
                            let alt =
                                keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
                            let action = retzar::gameplay::right_click(
                                sim,
                                &scene.mission,
                                &scene.grid,
                                &scene.library,
                                &world.resource::<Game>().gameplay,
                                i,
                                hovered,
                                Some([x as u16, y as u16]),
                            );
                            state.cursor = retzar::gameplay::click_cursor(
                                &scene.mission,
                                action,
                                hovered,
                                alt,
                            );
                        }
                    }
                }
            }
        }
    }
    while state.entities.len() > marks.len() {
        world.despawn(state.entities.pop().unwrap());
    }
    for (i, mark) in marks.into_iter().enumerate() {
        let sprite = if let Some(image) = mark.image {
            Sprite {
                custom_size: Some(mark.size),
                color: mark.color,
                ..Sprite::from_image(image)
            }
        } else {
            Sprite::from_color(mark.color, mark.size)
        };
        let transform = Transform::from_xyz(mark.centre.x, mark.centre.y, 421.);
        if let Some(&e) = state.entities.get(i) {
            world.entity_mut(e).insert((sprite, transform));
        } else {
            state.entities.push(
                world
                    .spawn((sprite, transform, RenderLayers::layer(1)))
                    .id(),
            );
        }
    }
    world.insert_resource(state);
}
