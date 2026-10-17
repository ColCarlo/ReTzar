//! Missiles in flight and the one-shot animations of the original: arrows,
//! shells with their shadow and trail, spell missiles (0x431340 and its
//! classes), and the impacts, flashes and spell pictures made with the
//! animation class 0x52 (0x497d40).
use super::{
    Game,
    glow::{self, GlowMaterial},
    gpu_image, lights,
    scene::Loaded,
};
use bevy::{camera::visibility::RenderLayers, prelude::*};
use retzar::gameplay::{Projectile, TICK_MS, missile};
use std::collections::HashMap;

/// A picture's frames, uploaded once for each brighten strength; a mode 3
/// picture is drawn as a quad with one material a frame.
struct Picture {
    frames: Vec<Handle<Image>>,
    glow: Vec<Handle<GlowMaterial>>,
    quad: Option<Handle<Mesh>>,
    size: Vec2,
    columns: usize,
    rows: usize,
    fps: u32,
    anchor: Vec2,
}
/// An animation on the map.
struct Running {
    entity: Entity,
    picture: (&'static str, u8),
    start_ms: i64,
    fps: u32,
    loops: u8,
    hold_ms: u32,
    column: usize,
    prio: u16,
    at: [i32; 2],
}
#[derive(Resource, Default)]
pub struct Effects {
    generation: u32,
    pictures: HashMap<(&'static str, u8), Option<Picture>>,
    running: Vec<Running>,
    missiles: Vec<Entity>,
    /// The mode 3 quads among the missiles and fires shown.
    glowing: Vec<Entity>,
    /// Last trail of each missile, by shooter and launch tick.
    trails: HashMap<(usize, u64), i64>,
}
/// Closing the map: its effects and their pictures go with it.
pub fn release(world: &mut World) {
    let Some(mut effects) = world.get_resource_mut::<Effects>() else {
        return;
    };
    let generation = effects.generation;
    let old = std::mem::take(&mut *effects);
    effects.generation = generation;
    let entities: Vec<Entity> = old
        .running
        .iter()
        .map(|r| r.entity)
        .chain(old.missiles.iter().copied())
        .chain(old.glowing.iter().copied())
        .collect();
    for e in entities {
        if let Ok(e) = world.get_entity_mut(e) {
            e.despawn();
        }
    }
    // As the scene's own (`scene::release`): removed now, not when the
    // last handle goes.
    for p in old.pictures.into_values().flatten() {
        if let Some(quad) = p.quad {
            world.resource_mut::<Assets<Mesh>>().remove(quad.id());
        }
        for m in p.glow {
            world.resource_mut::<Assets<GlowMaterial>>().remove(m.id());
        }
        for i in p.frames {
            world.resource_mut::<Assets<Image>>().remove(i.id());
        }
    }
}
/// The painter's place of a draw priority: craters (1) under every object,
/// arrows, spell pictures and trails (101–299) over ground objects, balls
/// (300) over those, birds (400) and the fog (450) above all.
fn depth(prio: u16) -> f32 {
    match prio {
        0..=99 => 5.,
        100..=299 => 390.5,
        _ => 395.,
    }
}
/// The GPU stores of the effects.
struct Stores<'a> {
    images: &'a mut Assets<Image>,
    materials: &'a mut Assets<GlowMaterial>,
    meshes: &'a mut Assets<Mesh>,
}
fn picture<'a>(
    effects: &'a mut Effects,
    stores: &mut Stores,
    game: &Game,
    library: &retzar::world::ObjectLibrary,
    key: (&'static str, u8),
) -> Option<&'a Picture> {
    effects
        .pictures
        .entry(key)
        .or_insert_with(|| {
            let p = library.effect_picture(&game.assets, key.0).ok()?;
            let size = p
                .frames
                .first()
                .map_or(Vec2::ZERO, |b| Vec2::new(b.width as f32, b.height as f32));
            let frames: Vec<Handle<Image>> = p
                .frames
                .into_iter()
                .map(|mut b| {
                    if key.1 > 0 {
                        glow::scale(&mut b, key.1);
                    }
                    stores.images.add(gpu_image(b))
                })
                .collect();
            let glow = if key.1 > 0 {
                frames
                    .iter()
                    .map(|texture| {
                        stores.materials.add(GlowMaterial {
                            texture: texture.clone(),
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let quad = (key.1 > 0).then(|| stores.meshes.add(Rectangle::from_size(size)));
            Some(Picture {
                frames,
                glow,
                quad,
                size,
                columns: p.columns,
                rows: p.rows,
                fps: p.fps,
                anchor: Vec2::new(p.anchor[0] as f32, p.anchor[1] as f32),
            })
        })
        .as_ref()
}
/// Where a picture's frame goes: the anchor on the map point (0x4347ac).
fn transform(p: &Picture, at: [i32; 2], prio: u16) -> Transform {
    Transform::from_xyz(
        at[0] as f32 - p.anchor.x + p.size.x / 2.,
        -(at[1] as f32 - p.anchor.y + p.size.y / 2.),
        depth(prio),
    )
}
/// The 24-direction row of a missile (0x4329e7): 0 north, 6 east, clockwise.
fn direction(d: [i32; 2]) -> usize {
    let angle = (d[0] as f64).atan2(d[1] as f64) + std::f64::consts::PI;
    (24.5 - angle.to_degrees() / 15.).trunc().rem_euclid(24.) as usize
}
/// What a missile shows: its picture, shadow, arc (peak, start height),
/// timer period, trail picture, trail period and the trail's strength.
struct Look {
    body: Option<(&'static str, u8)>,
    shadow: Option<&'static str>,
    arc: Option<(f64, f64)>,
    tick_ms: i64,
    trail: Option<(&'static str, i64, u8)>,
    turn: bool,
}
fn look(p: &Projectile) -> Look {
    let plain = Look {
        body: None,
        shadow: None,
        arc: None,
        tick_ms: 50,
        trail: None,
        turn: false,
    };
    match (p.kind, p.shooter) {
        // 0x4e9b60, 0x4eb080, 0x4e9d90, 0x4eaa80, 0x4dbc9d; the towers pick
        // by class (0x4dc100): stone `arrow_big`, wood `arrow`.
        (missile::ARROW, 6 | 25 | 39) => Look {
            body: Some(("arrow_big", 0)),
            tick_ms: 75,
            turn: true,
            ..plain
        },
        // 0x4ea280: the flame bolt plays its rows once.
        (missile::ARROW, 17 | 38) => Look {
            body: Some(("flame_bolt", 100)),
            ..plain
        },
        // 0x4ea4c0: a priest's bolt is the same missile with `exorcism`.
        (missile::ARROW, 18) => Look {
            body: Some(("exorcism", 100)),
            ..plain
        },
        (missile::ARROW, _) => Look {
            body: Some(("arrow", 0)),
            tick_ms: 75,
            turn: true,
            ..plain
        },
        // 0x4ed680, 0x4e8d20, 0x4dbdf1: ball, shadow and trail.
        (missile::SHELL, 19 | 20) => Look {
            body: Some(("cb_stone", 0)),
            shadow: Some("cb_metal_shadow"),
            arc: Some((192., 48.)),
            tick_ms: 80,
            trail: Some(("trail", 250, 50)),
            ..plain
        },
        (missile::SHELL, 23 | 24) => Look {
            body: Some(("cb_metal", 0)),
            shadow: Some("cb_metal_shadow"),
            arc: Some((64., 24.)),
            tick_ms: 5,
            trail: Some(("trail", 150, 50)),
            ..plain
        },
        (missile::SHELL, 42) => Look {
            body: Some(("cb_fire", 0)),
            shadow: Some("cb_metal_shadow"),
            arc: Some((64., 32.)),
            tick_ms: 25,
            trail: Some(("trail", 100, 50)),
            ..plain
        },
        // 0x432dd9: the rocket and its trail.
        (missile::SHELL, _) => Look {
            body: Some(("arrow_rocket", 0)),
            tick_ms: 35,
            trail: Some(("rocket_trail", 35, 100)),
            turn: true,
            ..plain
        },
        // 0x4ed800: the ballista bolt turns with its shadow.
        (missile::BOLT, _) => Look {
            body: Some(("arrow_ballista", 0)),
            shadow: Some("arrow_ballista_shadow"),
            arc: Some((64., 16.)),
            turn: true,
            ..plain
        },
        // 0x432350, 0x4325e0: freeze and flame balls show only their trail.
        (missile::FREEZE, _) => Look {
            trail: Some(("freeze_ball_trail", 45, 100)),
            ..plain
        },
        (missile::FLAME, _) => Look {
            trail: Some(("flame_ball_trail", 45, 100)),
            ..plain
        },
        // 0x450620: a falling meteor cycles its rows every 30 ms.
        (missile::FIRE, _) => Look {
            body: Some(("fire_rain", 0)),
            tick_ms: 30,
            ..plain
        },
        _ => plain,
    }
}
pub fn update(world: &mut World) {
    if !world.contains_resource::<Effects>() {
        world.init_resource::<Effects>();
    }
    world.resource_scope(|world, mut loaded: Mut<Loaded>| {
        let generation = loaded.generation;
        let Some(scene) = loaded.scene.as_mut() else {
            return;
        };
        // A unit holding a flag on the map has it over its head, centred on
        // its cell and 10 pixels above it, following it as it walks
        // (0x465620, 0x4aec42).
        let carried: Vec<[i32; 2]> =
            scene
                .mission
                .simulation
                .as_ref()
                .map_or_else(Vec::new, |sim| {
                    let m = &scene.mission;
                    (0..m.objects.len())
                        .filter_map(|j| {
                            let o = &m.objects[j];
                            if o.class != retzar::world::class::ITEM_FLAG
                                || o.flags & retzar::world::FLAG_INACTIVE != 0
                            {
                                return None;
                            }
                            let h = sim.objects.get(j)?.holder?;
                            if sim.objects.get(h)?.holder.is_some() {
                                return None;
                            }
                            let b = &m.objects[h];
                            let [dx, dy] = retzar::gameplay::motion_offset(sim, m, h);
                            Some([
                                b.x as i32 * 32 + 15 + dx.round() as i32,
                                b.y as i32 * 32 - dy.round() as i32 - 10,
                            ])
                        })
                        .collect()
                });
        let Some(sim) = scene.mission.simulation.as_mut() else {
            return;
        };
        let flashes = std::mem::take(&mut sim.flashes);
        let now = sim.elapsed.as_millis() as i64;
        let projectiles = sim.gameplay.projectiles.clone();
        // Fires of damaged buildings: two small, then a big one (0x4200c0);
        // a ship's burn on the centre of its picture as it sails (0x4e8b60,
        // 0x4e8c10).
        let visuals = &scene.visuals;
        let objects = &scene.mission.objects;
        let fires: Vec<(&'static str, [i32; 2])> = sim
            .gameplay
            .actors
            .iter()
            .enumerate()
            .flat_map(|(i, a)| {
                let centre = objects
                    .get(i)
                    .filter(|o| matches!(o.class, 23..=25 | retzar::world::class::FISHING_BOAT))
                    .and_then(|_| {
                        visuals
                            .iter()
                            .find(|v| v.object == i && v.layer == retzar::world::Layer::Body)
                    })
                    .map(|v| {
                        [
                            (v.bounds[0] + v.bounds[2] / 2.) as i32,
                            -(v.bounds[1] + v.bounds[3] / 2.) as i32,
                        ]
                    });
                a.fires.iter().enumerate().filter_map(move |(slot, f)| {
                    f.map(|at| {
                        (
                            if slot == 2 { "fire_big" } else { "fire_small" },
                            centre.unwrap_or(at),
                        )
                    })
                })
            })
            .collect();
        world.resource_scope(|world, game: Mut<Game>| {
            world.resource_scope(|world, mut effects: Mut<Effects>| {
                if effects.generation != generation {
                    // A new scene: the old effects go with the old map.
                    let old: Vec<Entity> = effects
                        .running
                        .iter()
                        .map(|r| r.entity)
                        .chain(effects.missiles.iter().copied())
                        .chain(effects.glowing.iter().copied())
                        .collect();
                    for e in old {
                        if let Ok(e) = world.get_entity_mut(e) {
                            e.despawn();
                        }
                    }
                    *effects = Effects {
                        generation,
                        ..Default::default()
                    };
                }
                let mut spawned = Vec::new();
                world.resource_scope(|world, mut images: Mut<Assets<Image>>| {
                    world.resource_scope(|world, mut materials: Mut<Assets<GlowMaterial>>| {
                        world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
                            let mut stores = Stores {
                                images: &mut images,
                                materials: &mut materials,
                                meshes: &mut meshes,
                            };
                            let mut start =
                                |effects: &mut Effects,
                                 stores: &mut Stores,
                                 world: &mut World,
                                 key: (&'static str, u8),
                                 at: [i32; 2],
                                 prio: u16,
                                 run: (i64, u32, u8, u32, usize)| {
                                    let Some(p) =
                                        picture(effects, stores, &game, &scene.library, key)
                                    else {
                                        return;
                                    };
                                    let entity = match &p.quad {
                                        // Mode 3: a quad that brightens what lies under it.
                                        Some(quad) => world
                                            .spawn((
                                                Mesh2d(quad.clone()),
                                                MeshMaterial2d(p.glow[0].clone()),
                                                transform(p, at, prio),
                                                Visibility::Hidden,
                                                RenderLayers::layer(1),
                                            ))
                                            .id(),
                                        None => world
                                            .spawn((
                                                Sprite {
                                                    custom_size: Some(p.size),
                                                    ..Sprite::from_image(p.frames[0].clone())
                                                },
                                                transform(p, at, prio),
                                                Visibility::Hidden,
                                                RenderLayers::layer(1),
                                            ))
                                            .id(),
                                    };
                                    let (start_ms, fps, loops, hold_ms, column) = run;
                                    effects.running.push(Running {
                                        entity,
                                        picture: key,
                                        start_ms,
                                        fps,
                                        loops,
                                        hold_ms,
                                        column,
                                        prio,
                                        at,
                                    });
                                    spawned.push(entity);
                                };
                            for f in flashes {
                                start(
                                    &mut effects,
                                    &mut stores,
                                    world,
                                    (f.image, f.glow),
                                    f.at,
                                    f.prio,
                                    (
                                        now + f.delay_ms as i64,
                                        f.fps as u32,
                                        f.loops,
                                        f.hold_ms,
                                        f.column as usize,
                                    ),
                                );
                            }
                            // Missiles: the body moves in steps of its timer.
                            let mut shown: Vec<((&'static str, u8), usize, [i32; 2], u16)> =
                                Vec::new();
                            let mut live = std::collections::HashSet::new();
                            for p in &projectiles {
                                let l = look(p);
                                let key = (p.attacker, p.launched);
                                live.insert(key);
                                let launched = p.launched as i64 * TICK_MS as i64;
                                // A fireball of the fire rain not yet made.
                                if now < launched {
                                    continue;
                                }
                                // A missed arrow keeps its pace past the end
                                // (0x432bac).
                                let reach = if p.flown != 0 { p.flown } else { p.lands };
                                let flight = if p.lands == u64::MAX {
                                    1
                                } else {
                                    ((reach - p.launched) as i64 * TICK_MS as i64).max(1)
                                };
                                let span = if p.lands == u64::MAX {
                                    1
                                } else {
                                    ((p.lands - p.launched) as i64 * TICK_MS as i64).max(1)
                                };
                                let elapsed = (now - launched).clamp(0, span);
                                let stepped = elapsed / l.tick_ms.max(1) * l.tick_ms.max(1);
                                let (from, to) = (p.start, p.end);
                                let pos = if p.kind == missile::FLAME {
                                    p.pos
                                } else {
                                    [
                                        from[0]
                                            + ((to[0] - from[0]) as i64 * stepped / flight) as i32,
                                        from[1]
                                            + ((to[1] - from[1]) as i64 * stepped / flight) as i32,
                                    ]
                                };
                                let d = [to[0] - from[0], to[1] - from[1]];
                                let row = if l.turn { direction(d) } else { 0 };
                                // 0x4335e7: h = (a·s + b)·s + h0 over the Chebyshev distance.
                                let height = l.arc.map_or(0, |(peak, h0)| {
                                    let span = d[0].abs().max(d[1].abs()) as f64;
                                    if span == 0. {
                                        return 0;
                                    }
                                    let s = (pos[0] - from[0]).abs().max((pos[1] - from[1]).abs())
                                        as f64;
                                    let a = (2. * h0 - 2. * peak) / (span * span);
                                    let b = (2. * peak - 3. * h0) / span;
                                    ((a * s + b) * s + h0).trunc() as i32
                                });
                                let ball = [pos[0], pos[1] - height];
                                if let Some(body) = l.body {
                                    let frame = match p.kind {
                                        missile::FIRE => (stepped / 30) as usize,
                                        // The flame bolt plays its 18 rows at 18 fps, then holds.
                                        missile::ARROW if matches!(p.shooter, 17 | 38) => {
                                            (elapsed * 18 / 1000) as usize
                                        }
                                        _ => row,
                                    };
                                    shown.push((
                                        body,
                                        frame,
                                        ball,
                                        if l.arc.is_some() { 300 } else { 200 },
                                    ));
                                }
                                if let Some(shadow) = l.shadow {
                                    shown.push(((shadow, 0), row, pos, 200));
                                }
                                if let Some((trail, every, glow)) = l.trail {
                                    let last =
                                        effects.trails.get(&key).copied().unwrap_or(launched);
                                    if now - last >= every {
                                        effects.trails.insert(key, now);
                                        let column =
                                            if matches!(p.kind, missile::FREEZE | missile::FLAME) {
                                                (now / every % 3) as usize
                                            } else {
                                                0
                                            };
                                        start(
                                            &mut effects,
                                            &mut stores,
                                            world,
                                            (trail, glow),
                                            ball,
                                            200,
                                            (now, 0, 1, 0, column),
                                        );
                                    }
                                }
                            }
                            effects.trails.retain(|k, _| live.contains(k));
                            // A fire loops its rows at its picture's pace, in
                            // mode 3 at full strength (0x420079: 100, 3).
                            for (name, at) in fires {
                                let Some(p) = picture(
                                    &mut effects,
                                    &mut stores,
                                    &game,
                                    &scene.library,
                                    (name, 100),
                                ) else {
                                    continue;
                                };
                                let frame_ms = (1000 / p.fps.max(1)) as i64;
                                let frame = (now / frame_ms) as usize % p.rows.max(1);
                                // 0x420083: the fire's priority is the building's + 1.
                                shown.push(((name, 100), frame, at, 101));
                            }
                            // The flag waves over its bearer at 5 frames a second
                            // (0x465620, priority 0x65).
                            for at in &carried {
                                let Some(p) = picture(
                                    &mut effects,
                                    &mut stores,
                                    &game,
                                    &scene.library,
                                    ("item_flag", 0),
                                ) else {
                                    continue;
                                };
                                let frames = if p.rows > 1 { p.rows } else { p.columns };
                                let frame = (now / 200) as usize % frames.max(1);
                                shown.push((("item_flag", 0), frame, *at, 0x65));
                            }
                            let mut used = 0;
                            let mut lit = 0;
                            let mut glows = Vec::new();
                            for (key, frame, at, prio) in shown {
                                let Some(p) =
                                    picture(&mut effects, &mut stores, &game, &scene.library, key)
                                else {
                                    continue;
                                };
                                if let Some(color) = lights::emissive(key.0, !p.glow.is_empty()) {
                                    glows.push(lights::Light {
                                        at: transform(p, at, prio).translation.truncate(),
                                        radius: p.size.max_element() * 0.9 + 16.,
                                        color,
                                    });
                                }
                                let index = if p.rows > 1 {
                                    frame.min(p.rows - 1) * p.columns
                                } else {
                                    frame.min(p.columns.saturating_sub(1))
                                };
                                let index = index.min(p.frames.len() - 1);
                                let image = p.frames[index].clone();
                                let place = transform(p, at, prio);
                                let size = p.size;
                                // Mode 3: a quad that brightens what lies under it.
                                if let Some(quad) = p.quad.clone() {
                                    let material = p.glow[index].clone();
                                    if let Some(&e) = effects.glowing.get(lit) {
                                        if let Some(mut m) = world.get_mut::<Mesh2d>(e) {
                                            m.0 = quad;
                                        }
                                        if let Some(mut m) =
                                            world.get_mut::<MeshMaterial2d<GlowMaterial>>(e)
                                        {
                                            m.0 = material;
                                        }
                                        if let Some(mut t) = world.get_mut::<Transform>(e) {
                                            *t = place;
                                        }
                                        if let Some(mut v) = world.get_mut::<Visibility>(e) {
                                            *v = Visibility::Inherited;
                                        }
                                    } else {
                                        let e = world
                                            .spawn((
                                                Mesh2d(quad),
                                                MeshMaterial2d(material),
                                                place,
                                                RenderLayers::layer(1),
                                            ))
                                            .id();
                                        effects.glowing.push(e);
                                        spawned.push(e);
                                    }
                                    lit += 1;
                                    continue;
                                }
                                if let Some(&e) = effects.missiles.get(used) {
                                    if let Some(mut s) = world.get_mut::<Sprite>(e) {
                                        s.image = image;
                                        s.custom_size = Some(size);
                                    }
                                    if let Some(mut t) = world.get_mut::<Transform>(e) {
                                        *t = place;
                                    }
                                    if let Some(mut v) = world.get_mut::<Visibility>(e) {
                                        *v = Visibility::Inherited;
                                    }
                                } else {
                                    let e = world
                                        .spawn((
                                            Sprite {
                                                custom_size: Some(size),
                                                ..Sprite::from_image(image)
                                            },
                                            place,
                                            RenderLayers::layer(1),
                                        ))
                                        .id();
                                    effects.missiles.push(e);
                                    spawned.push(e);
                                }
                                used += 1;
                            }
                            for &e in effects.missiles[used..]
                                .iter()
                                .chain(&effects.glowing[lit..])
                            {
                                if let Some(mut v) = world.get_mut::<Visibility>(e) {
                                    *v = Visibility::Hidden;
                                }
                            }
                            // Animations: frames along the rows (or the columns of a
                            // one-row picture), then the last one held, then gone.
                            let mut finished = Vec::new();
                            for (k, r) in effects.running.iter().enumerate() {
                                let Some(Some(p)) = effects.pictures.get(&r.picture) else {
                                    finished.push(k);
                                    continue;
                                };
                                let along_columns = p.rows == 1 && p.columns > 1;
                                let count = if along_columns { p.columns } else { p.rows }.max(1);
                                let fps = if r.fps == 0 { p.fps.max(1) } else { r.fps };
                                let frame_ms = 1000 / fps as i64;
                                let elapsed = now - r.start_ms;
                                let total = count as i64 * frame_ms * r.loops.max(1) as i64;
                                if elapsed >= total + r.hold_ms as i64 {
                                    finished.push(k);
                                    continue;
                                }
                                let n = if elapsed < 0 {
                                    0
                                } else if elapsed >= total {
                                    count - 1
                                } else {
                                    (elapsed / frame_ms) as usize % count
                                };
                                let index = if along_columns {
                                    n
                                } else {
                                    n * p.columns + r.column.min(p.columns - 1)
                                };
                                let index = index.min(p.frames.len() - 1);
                                let image = p.frames[index].clone();
                                if let Some(mut s) = world.get_mut::<Sprite>(r.entity) {
                                    if s.image != image {
                                        s.image = image;
                                    }
                                }
                                if let Some(material) = p.glow.get(index)
                                    && let Some(mut m) =
                                        world.get_mut::<MeshMaterial2d<GlowMaterial>>(r.entity)
                                    && m.0 != *material
                                {
                                    m.0 = material.clone();
                                }
                                if let Some(mut v) = world.get_mut::<Visibility>(r.entity) {
                                    *v = if elapsed < 0 {
                                        Visibility::Hidden
                                    } else {
                                        Visibility::Inherited
                                    };
                                }
                                if elapsed >= 0
                                    && let Some(color) =
                                        lights::emissive(r.picture.0, !p.glow.is_empty())
                                {
                                    glows.push(lights::Light {
                                        at: transform(p, r.at, r.prio).translation.truncate(),
                                        radius: p.size.max_element() * 0.9 + 16.,
                                        color,
                                    });
                                }
                            }
                            for k in finished.into_iter().rev() {
                                let r = effects.running.remove(k);
                                if let Ok(e) = world.get_entity_mut(r.entity) {
                                    e.despawn();
                                }
                            }
                            if let Some(mut sources) =
                                world.get_resource_mut::<lights::LightSources>()
                            {
                                sources.0.extend(glows);
                            }
                        });
                    });
                });
                let _ = spawned;
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tower_arrows_follow_the_class_table() {
        // 0x4dc100: the stone tower `arrow_big`, the wooden ones `arrow`.
        let shot = |shooter| Projectile {
            kind: missile::ARROW,
            shooter,
            ..Default::default()
        };
        assert_eq!(look(&shot(39)).body.unwrap().0, "arrow_big");
        assert_eq!(look(&shot(40)).body.unwrap().0, "arrow");
        assert_eq!(look(&shot(41)).body.unwrap().0, "arrow");
        assert_eq!(look(&shot(6)).body.unwrap().0, "arrow_big");
    }
}
