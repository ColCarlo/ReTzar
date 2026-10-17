//! Input requests, CPU simulation and incremental sprite synchronization.
use super::{
    Game, MenuState, SoundPlayer, gpu_image, heard,
    mission::MissionUi,
    scene::{Loaded, Scene, VisualAnimation},
    voice,
};
use bevy::{camera::visibility::RenderLayers, prelude::*, window::PrimaryWindow};
use retzar::{
    animation::Timeline,
    simulation::OrderState,
    world::{self, FLAG_INACTIVE, Layer, ObjectVisual},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Picture {
    visual: ObjectVisual,
    size: Vec2,
    image: Handle<Image>,
    frames: Vec<Handle<Image>>,
    timeline: Option<Timeline>,
}
/// The pictures of a trunk image: tree and shadow, their sizes and anchors.
struct Trunk {
    images: [Handle<Image>; 2],
    sizes: [Vec2; 2],
    anchors: [[f32; 2]; 2],
}
#[derive(Resource, Default)]
struct Renderer {
    generation: u32,
    signatures: BTreeMap<usize, Signature>,
    pictures: BTreeMap<String, Vec<Picture>>,
    depleted: usize,
    /// The trunks whose pictures replaced their tree's, and the pictures of
    /// each trunk image (tree, shadow).
    stumps: BTreeSet<usize>,
    trunks: BTreeMap<usize, Trunk>,
    offsets: BTreeMap<usize, Vec2>,
    /// Trees currently drawn see-through.
    transparent: bool,
}
/// What the fog lets the player see of object `i`: the object, its picture
/// kept in memory (fakebuilding), or nothing.
fn remembered(s: &Scene, i: usize) -> Option<&world::MapObject> {
    let sim = s.mission.simulation.as_ref()?;
    let live = &s.mission.objects[i];
    sim.appearance(i, live).filter(|o| !std::ptr::eq(*o, live))
}
/// What an object's sprites are made from: the key of its picture, and what
/// also renews them without changing the picture (where a fish patch is,
/// when a cow's grazing picture started).
type Signature = (String, String);
fn signature(s: &Scene, i: usize) -> Signature {
    let hidden = || ("hidden".into(), String::new());
    let sim = s.mission.simulation.as_ref().unwrap();
    let live = &s.mission.objects[i];
    if sim.appearance(i, live).is_none() {
        return hidden();
    }
    // A remembered building is drawn as it was, without its live state.
    let memory = remembered(s, i);
    let o = memory.unwrap_or(live);
    let state = &sim.objects[i];
    let actor = if memory.is_some() {
        None
    } else {
        sim.gameplay.actors.get(i)
    };
    if memory.is_none() && (o.flags & FLAG_INACTIVE != 0 || state.holder.is_some()) {
        return hidden();
    }
    let mut moment = String::new();
    let dying = o.flags & world::FLAG_DYING != 0;
    let pose = if world::class::is_unit(o.class) || dying {
        state.order
    } else {
        OrderState::Idle
    };
    let task = actor
        .and_then(|a| a.intent.as_ref())
        .map(|a| match a {
            retzar::gameplay::Intent::Chop(_) => 1,
            retzar::gameplay::Intent::Capture(_) => usize::MAX,
            retzar::gameplay::Intent::Harvest(j) => {
                s.mission.objects.get(*j).map_or(2, |x| x.class)
            }
            _ => 0,
        })
        .unwrap_or(0);
    let mut key = format!(
        "{}:{}:{:?}:{}:{}:{}:{:?}:{}:{}:{}:{}",
        o.class,
        o.civilization,
        s.mission.color_slot(o.owner),
        o.variant,
        o.word(56),
        o.byte(58),
        pose,
        actor.map_or(0, |a| a.direction),
        task,
        if matches!(o.class, 61 | 62) && o.max_health > 0 {
            o.health as usize * 16 / o.max_health as usize
        } else if o.class == world::class::FIELD && o.max_health > 0 {
            // Growth stage of the field.
            o.health as usize * 4 / o.max_health as usize
        } else {
            16
        },
        o.custom_image
    );
    if matches!(o.class, world::class::GATE_WOOD | world::class::GATE_STONE) {
        // The drawbridge's row, moving every 100 ms (0x4573dc).
        let live = memory.is_none().then_some(sim);
        key.push_str(&format!(
            ":bridge:{}",
            retzar::gameplay::drawbridge_row(live, i, o)
        ));
    }
    if o.flags & world::FLAG_CONSTRUCTION != 0 && o.max_health > 0 {
        // The site's stage row (0x41e0c0).
        key.push_str(&format!(
            ":construction:{}",
            o.health as usize * 4 / o.max_health as usize
        ));
    }
    // Fish patches drift (0x450ba0); the picture is chosen by the patch.
    if o.class == world::class::FISHPATCH {
        key.push_str(&format!(
            ":fish:{}:nets:{:?}",
            o.id,
            retzar::gameplay::fishing_nets(Some(sim), i)
        ));
        moment = format!("{}:{}", o.x, o.y);
    }
    // Attack, spell and death pictures follow the actor's current action.
    if matches!(
        pose,
        OrderState::Attacking | OrderState::Casting | OrderState::Dying | OrderState::Appearing
    ) && let Some(a) = actor
    {
        key.push_str(&format!(":{}", a.action_image));
    }
    if pose == OrderState::Moving && retzar::gameplay::jump_frame(sim, i).is_some() {
        key.push_str(":jump");
    }
    if pose == OrderState::Idle
        && let Some((image, backwards, since)) = retzar::gameplay::grazing(sim, live.class, i)
    {
        key.push_str(&format!(":graze:{image}"));
        moment = format!("{backwards}:{since}");
    }
    if let Some(disguise) = actor.and_then(|a| a.disguise.as_ref()) {
        key.push_str(&format!(
            ":disguise:{}:{}:{}",
            disguise.class, disguise.civilization, disguise.owner
        ));
    }
    if actor.is_some_and(|a| a.effect_until > sim.ticks) {
        key.push_str(":cloak");
    }
    if let Some(a) = actor {
        if let Some(image) = s.library.carrying_image(o, a.cargo, a.cargo_source) {
            key.push_str(&format!(":carry:{image}"));
        }
    }
    (key, moment)
}
/// Takes away the sprites of `objects` in one pass, keeping the others in
/// their order.
fn remove_objects(world: &mut World, s: &mut Scene, objects: &BTreeSet<usize>) {
    if objects.is_empty() {
        return;
    }
    let mut keep = Vec::with_capacity(s.visuals.len());
    let mut entities = Vec::with_capacity(s.visuals.len());
    let mut gone = std::collections::HashSet::new();
    for (v, e) in std::mem::take(&mut s.visuals)
        .into_iter()
        .zip(std::mem::take(&mut s.visual_entities))
    {
        if objects.contains(&v.object) {
            world.despawn(e);
            gone.insert(e);
        } else {
            keep.push(v);
            entities.push(e);
        }
    }
    if !gone.is_empty() {
        s.entities.retain(|x| !gone.contains(x));
        s.sprite_animations.retain(|a| !gone.contains(&a.entity));
    }
    s.visuals = keep;
    s.visual_entities = entities;
}
fn synchronize(
    world: &mut World,
    s: &mut Scene,
    renderer: &mut Renderer,
    assets: &retzar::assets::GameAssets,
) -> retzar::Result<()> {
    if s.visual_entities.len() != s.visuals.len() {
        return Ok(());
    }
    let mut removed = BTreeSet::new();
    let count = s.mission.objects.len();
    let mut changes = Vec::new();
    for i in 0..count {
        let o = &s.mission.objects[i];
        if !(world::class::is_unit(o.class)
            || o.building()
            || o.item()
            || matches!(
                o.class,
                world::class::COW
                    | world::class::FISHING_BOAT
                    | world::class::RES_GOLD
                    | world::class::RES_STONE
                    | world::class::RESITEM
                    | world::class::FISHPATCH
            ))
        {
            continue;
        }
        let now = signature(s, i);
        if renderer.signatures.get(&i) != Some(&now) {
            changes.push((i, now));
        }
    }
    // The objects that changed lose their sprites, then get new ones in
    // the same order.
    remove_objects(world, s, &changes.iter().map(|c| c.0).collect());
    for (i, now) in changes {
        renderer.offsets.insert(i, Vec2::ZERO);
        removed.insert(i);
        let key = now.0.clone();
        renderer.signatures.insert(i, now);
        // A remembered building is built from its picture in memory.
        let memory = remembered(s, i).cloned();
        if key == "hidden" {
            continue;
        }
        if !renderer.pictures.contains_key(&key) {
            let live = memory
                .clone()
                .map(|m| std::mem::replace(&mut s.mission.objects[i], m));
            let built =
                s.library
                    .build_indices(assets, &s.mission, s.side, Some(&BTreeSet::from([i])));
            if let Some(live) = live {
                s.mission.objects[i] = live;
            }
            let built = built?;
            if !built.missing.is_empty() {
                return Err(built.missing.join("; ").into());
            }
            let sizes: Vec<_> = built
                .images
                .iter()
                .map(|b| Vec2::new(b.width as f32, b.height as f32))
                .collect();
            let images: Vec<_> = built
                .images
                .into_iter()
                .map(|b| world.resource_mut::<Assets<Image>>().add(gpu_image(b)))
                .collect();
            s.images.extend(images.iter().cloned());
            let p = s.mission.objects[i].location();
            let pictures = built
                .visuals
                .into_iter()
                .map(|mut v| {
                    let size = sizes[v.image];
                    let image = images[v.image].clone();
                    let anim = v.animation.map(|j| &built.animations[j]);
                    let frames = anim.map_or(Vec::new(), |a| {
                        a.frames.iter().map(|&j| images[j].clone()).collect()
                    });
                    let timeline = anim.map(|a| a.timeline.clone());
                    v.top_left[0] -= p[0];
                    v.top_left[1] -= p[1];
                    v.bounds[0] -= p[0];
                    v.bounds[1] -= p[1];
                    v.footprint[0] -= p[0];
                    v.footprint[1] -= p[1];
                    Picture {
                        visual: v,
                        size,
                        image,
                        frames,
                        timeline,
                    }
                })
                .collect();
            renderer.pictures.insert(key.clone(), pictures);
        }
        let p = s.mission.objects[i].location();
        for picture in &renderer.pictures[&key] {
            let mut v = picture.visual;
            v.object = i;
            v.top_left[0] += p[0];
            v.top_left[1] += p[1];
            v.bounds[0] += p[0];
            v.bounds[1] += p[1];
            v.footprint[0] += p[0];
            v.footprint[1] += p[1];
            let color = if s
                .mission
                .simulation
                .as_ref()
                .is_some_and(|sim| sim.gameplay.actors[i].effect_until > sim.ticks)
            {
                Color::srgba(1., 1., 1., 0.5)
            } else {
                Color::WHITE
            };
            let e = world
                .spawn((
                    Sprite {
                        custom_size: Some(picture.size),
                        color,
                        ..Sprite::from_image(picture.image.clone())
                    },
                    Transform::from_xyz(
                        v.top_left[0] + picture.size.x / 2.,
                        v.top_left[1] - picture.size.y / 2.,
                        1.,
                    ),
                    RenderLayers::layer(1),
                ))
                .id();
            s.entities.push(e);
            s.visual_entities.push(e);
            s.visuals.push(v);
            if let Some(timeline) = &picture.timeline {
                s.sprite_animations.push(VisualAnimation {
                    entity: e,
                    object: i,
                    frames: picture.frames.clone(),
                    timeline: timeline.clone(),
                    phase_ms: 0,
                    // A new figure starts its timer now (vt+0xe8).
                    since_ms: Some(
                        s.mission.simulation.as_ref().unwrap().elapsed.as_millis() as u64
                    ),
                    start: v.start_row,
                    idle: None,
                    last: usize::MAX,
                    action: matches!(
                        s.mission.simulation.as_ref().unwrap().objects[i].order,
                        OrderState::Working
                            | OrderState::Building
                            | OrderState::Attacking
                            | OrderState::Casting
                            | OrderState::Dying
                            | OrderState::Appearing
                    )
                    .then_some(i),
                    motion: (s.mission.simulation.as_ref().unwrap().objects[i].order
                        == OrderState::Moving)
                        .then(|| {
                            let o = &s.mission.objects[i];
                            let actor = &s.mission.simulation.as_ref().unwrap().gameplay.actors[i];
                            let section = s
                                .library
                                .carrying_image(o, actor.cargo, actor.cargo_source)
                                .or_else(|| s.library.runtime_image(o, Some(OrderState::Moving)))
                                .unwrap_or_default();
                            (
                                i,
                                s.library
                                    .images
                                    .get(&section, "move_len")
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(32),
                            )
                        }),
                    // A dying flier's shadow comes down under its body
                    // (0x4e7ae0).
                    descent: {
                        let lift = world::shadow_lift(s.mission.objects[i].class);
                        (v.layer == Layer::Shadow
                            && lift != [0., 0.]
                            && s.mission.simulation.as_ref().unwrap().objects[i].order
                                == OrderState::Dying)
                            .then(|| {
                                let at = Vec2::new(
                                    v.top_left[0] + picture.size.x / 2. - lift[0],
                                    v.top_left[1] - picture.size.y / 2. + lift[1],
                                );
                                (lift, at)
                            })
                    },
                });
            }
        }
    }
    // Trees belong to the terrain visuals. A felled tree is not drawn (body
    // and shadow) until it turns into a trunk, which takes its pictures.
    if let Some(sim) = &s.mission.simulation {
        let cell = |v: &ObjectVisual| {
            let x = (v.footprint[0] / 32.).floor().max(0.) as usize;
            let y = (-(v.footprint[1] + v.footprint[3]) / 32.).floor().max(0.) as usize;
            y * s.side + x
        };
        let fresh: BTreeSet<usize> = sim
            .gameplay
            .stumps
            .difference(&renderer.stumps)
            .copied()
            .collect();
        for &c in &fresh {
            let value = (s.grid.cells[c] >> 11) as usize;
            if !renderer.trunks.contains_key(&value) {
                let (pictures, anchors) = world::tree_pictures(assets, &s.biome, value)?;
                let sizes = pictures
                    .each_ref()
                    .map(|b| Vec2::new(b.width as f32, b.height as f32));
                let mut images = world.resource_mut::<Assets<Image>>();
                let images = pictures.map(|b| images.add(gpu_image(b)));
                renderer.trunks.insert(
                    value,
                    Trunk {
                        images,
                        sizes,
                        anchors,
                    },
                );
            }
        }
        let mut drawn = BTreeSet::new();
        for (v, e) in s.visuals.iter().zip(&s.visual_entities) {
            if v.object != usize::MAX || !v.tree {
                continue;
            }
            let c = cell(v);
            if sim.gameplay.depleted_trees.contains(&c) {
                if let Some(mut vis) = world.get_mut::<Visibility>(*e) {
                    *vis = Visibility::Hidden;
                }
            } else if fresh.contains(&c) {
                let value = (s.grid.cells[c] >> 11) as usize;
                let image =
                    renderer.trunks[&value].images[usize::from(v.layer == Layer::Shadow)].clone();
                if let Some(mut sprite) = world.get_mut::<Sprite>(*e) {
                    sprite.image = image;
                }
                if let Some(mut vis) = world.get_mut::<Visibility>(*e) {
                    *vis = Visibility::Inherited;
                }
                drawn.insert(c);
            }
        }
        // A tree felled before a save came back without pictures: its trunk
        // gets new ones, as `add_trees` would draw it.
        let alpha = if renderer.transparent { 0.5 } else { 1. };
        let mut spawned = Vec::new();
        for &c in fresh.difference(&drawn) {
            let trunk = &renderer.trunks[&((s.grid.cells[c] >> 11) as usize)];
            let x = (c % s.side) as f32 * 32.;
            let y = -((c / s.side) as f32) * 32.;
            for (k, layer) in [(1, Layer::Shadow), (0, Layer::Body)] {
                let [ax, ay] = trunk.anchors[k];
                let size = trunk.sizes[k];
                let v = ObjectVisual {
                    object: usize::MAX,
                    image: 0,
                    top_left: [x - ax, y + ay],
                    bounds: [x, y - 32., 32., 32.],
                    footprint: [x, y - 32., 32., 32.],
                    ground: false,
                    animation: None,
                    phase_ms: 0,
                    start_row: 0,
                    layer,
                    tree: true,
                };
                let mut sprite = Sprite {
                    custom_size: Some(size),
                    ..Sprite::from_image(trunk.images[k].clone())
                };
                sprite.color.set_alpha(alpha);
                let e = world
                    .spawn((
                        sprite,
                        Transform::from_xyz(
                            v.top_left[0] + size.x / 2.,
                            v.top_left[1] - size.y / 2.,
                            1.,
                        ),
                        RenderLayers::layer(1),
                    ))
                    .id();
                s.entities.push(e);
                s.visual_entities.push(e);
                s.visuals.push(v);
                spawned.push(s.visuals.len() - 1);
            }
        }
        if !spawned.is_empty() {
            let depths = world::painter_depths(&s.visuals, &s.mission, s.side);
            for k in spawned {
                if let Some(mut t) = world.get_mut::<Transform>(s.visual_entities[k]) {
                    t.translation.z = depths[k];
                }
            }
        }
        renderer.stumps.extend(fresh);
    }
    let depleted = s.mission.simulation.as_ref().map_or(0, |s| {
        s.gameplay.depleted_trees.len() + s.gameplay.stumps.len()
    });
    if depleted != renderer.depleted {
        let terrain = retzar::terrain::Terrain::load(assets, &s.biome)?;
        s.cells = terrain.minimap(&s.grid)?;
        renderer.depleted = depleted;
    }
    let changed = !removed.is_empty()
        || s.visuals.iter().any(|v| {
            s.mission.objects.get(v.object).is_some_and(|o| {
                let p = o.location();
                v.footprint[0] != p[0] || v.footprint[1] + v.footprint[3] != p[1]
            })
        });
    let depths = changed.then(|| world::painter_depths(&s.visuals, &s.mission, s.side));
    for (k, (v, e)) in s.visuals.iter_mut().zip(&s.visual_entities).enumerate() {
        if let Some(o) = s.mission.objects.get(v.object) {
            let size = world
                .get::<Sprite>(*e)
                .and_then(|sprite| {
                    sprite.custom_size.or_else(|| {
                        world
                            .resource::<Assets<Image>>()
                            .get(&sprite.image)
                            .map(|image| Vec2::new(image.width() as f32, image.height() as f32))
                    })
                })
                .unwrap_or(Vec2::ONE);
            if let Some(mut t) = world.get_mut::<Transform>(*e) {
                let current = [t.translation.x - size.x / 2., t.translation.y + size.y / 2.];
                // Footprints follow the authoritative cell; interpolate the sprite between steps.
                let p = o.location();
                let ox = v.footprint[0];
                let oy = v.footprint[1] + v.footprint[3];
                let dx = p[0] - ox;
                let dy = p[1] - oy;
                v.top_left[0] += dx;
                v.top_left[1] += dy;
                v.bounds[0] += dx;
                v.bounds[1] += dy;
                v.footprint[0] += dx;
                v.footprint[1] += dy;
                let offset = s.mission.simulation.as_ref().map_or(Vec2::ZERO, |sim| {
                    Vec2::from_array(retzar::gameplay::motion_offset(sim, &s.mission, v.object))
                });
                let old_offset = renderer
                    .offsets
                    .get(&v.object)
                    .copied()
                    .unwrap_or(Vec2::ZERO);
                v.bounds[0] += offset.x - old_offset.x;
                v.bounds[1] += offset.y - old_offset.y;
                t.translation.x = current[0] + dx + size.x / 2. + offset.x;
                t.translation.y = current[1] + dy - size.y / 2. + offset.y;
                // Assign from visual coordinates, avoiding accumulation of interpolation offsets.
                t.translation.x = v.top_left[0] + size.x / 2. + offset.x;
                t.translation.y = v.top_left[1] - size.y / 2. + offset.y;
                if let Some(depths) = &depths {
                    t.translation.z = if v.layer == Layer::Air {
                        world::AIR_DEPTH
                    } else {
                        depths[k]
                    };
                }
            }
        }
    }
    if let Some(sim) = &s.mission.simulation {
        for i in 0..s.mission.objects.len() {
            renderer.offsets.insert(
                i,
                Vec2::from_array(retzar::gameplay::motion_offset(sim, &s.mission, i)),
            );
        }
    }
    let _ = removed;
    Ok(())
}

/// The open game's speed (0x587748).
fn game_speed(world: &World) -> Option<u8> {
    let scene = world.resource::<Loaded>().scene.as_ref()?;
    Some(scene.mission.simulation.as_ref()?.speed)
}
/// A new speed for the open game (0x43f0a0), from the keypad or Options
/// (0x47d121): the message line names it, `game_speed` and the word of its
/// level (0x43f154).
pub(super) fn change_speed(world: &mut World, speed: u8) {
    let mut loaded = world.resource_mut::<Loaded>();
    let Some(sim) = loaded
        .scene
        .as_mut()
        .and_then(|s| s.mission.simulation.as_mut())
    else {
        return;
    };
    sim.speed = speed;
    let locale = &world.resource::<Game>().skin.locale;
    let text = format!(
        "{} {}",
        locale.label("msg", "game_speed"),
        locale.label("msg", retzar::simulation::speed_label(speed))
    );
    let mut menu = world.resource_mut::<MenuState>();
    // From the open settings the speed is the page's until «OK» keeps it
    // or «Annulla» brings the old one back.
    if !menu.screen.is_settings() {
        menu.settings.speed = speed;
    }
    menu.notice = text;
    menu.dirty = true;
}

pub fn update(world: &mut World) {
    if !world.contains_resource::<Renderer>() {
        world.init_resource::<Renderer>();
    }
    let focused = {
        let mut q = world.query_filtered::<&Window, With<PrimaryWindow>>();
        q.iter(world).next().is_some_and(|w| w.focused)
    };
    let dt = world.resource::<Time>().delta();
    let settings = world.resource::<MenuState>().current_settings().clone();
    let view = world.resource::<super::view::MapView>().clone();
    // Sounds of the map are heard from the centre cell of the map view
    // (0x4f8260) and fade over its width in pixels (0x4d00e0).
    let (hearing, heard_width, heard_px) = {
        let mut q = world.query_filtered::<&Window, With<PrimaryWindow>>();
        q.iter(world).next().map_or(([0, 0], 0, [0, 0]), |w| {
            let surface = super::view::Surface::of(w);
            let layout = super::view::hud_layout(surface, &settings);
            let r = retzar::hud::play_rect(
                layout.size.x as i32,
                layout.size.y as i32,
                settings.layout == 1,
                view.frame,
            );
            let factor = layout.factor / surface.dpi;
            let lo = view.world(surface, Vec2::new(r.x as f32, r.y as f32) * factor);
            let hi = view.world(
                surface,
                Vec2::new((r.x + r.w) as f32, (r.y + r.h) as f32) * factor,
            );
            let centre = (lo + hi) / 2.;
            (
                [
                    (centre.x / 32.).floor() as i32,
                    (-centre.y / 32.).floor() as i32,
                ],
                (hi.x - lo.x).abs() as u32,
                [centre.x as i32, -centre.y as i32],
            )
        })
    };
    let place = |at: [u16; 2]| retzar::audio::Place {
        at: [at[0] as i32, at[1] as i32],
        centre: hearing,
        width: heard_width,
    };
    // 0x4f6f81: the sounds playing are heard again once the view has moved
    // more than three cells along an axis since the last time (0x4d01c0).
    let relocate = world
        .get_resource_mut::<SoundPlayer>()
        .is_some_and(|mut audio| {
            audio.hearing = (hearing, heard_width);
            let moved = audio.heard.is_none_or(|p| {
                (p[0] - heard_px[0]).abs() / 32 > 3 || (p[1] - heard_px[1]).abs() / 32 > 3
            });
            if moved {
                audio.heard = Some(heard_px);
            }
            moved
        });
    let paused = world.resource::<MissionUi>().hud.panel.is_some() || !focused;
    let key = world.resource_mut::<MissionUi>().speed_key.take();
    if let Some(speed) = game_speed(world)
        .zip(key)
        .and_then(|(speed, key)| retzar::simulation::speed_after(speed, key))
    {
        change_speed(world, speed);
    }
    // The Options slider shows the open game's speed.
    let speed = game_speed(world);
    if let Some(speed) = speed.filter(|&s| s != world.resource::<MenuState>().settings.speed) {
        world.resource_mut::<MenuState>().settings.speed = speed;
    }
    // The options show the open game's cheat panel switch.
    let cheats = world.resource::<MissionUi>().hud.cheats;
    if world.resource::<MenuState>().settings.cheats != cheats {
        world.resource_mut::<MenuState>().settings.cheats = cheats;
    }
    // The cheat panel's «next» (0x424b60): the map the scripts' first
    // LoadMap names (0x424b7d), opened as «CONTINUA» opens it (0x43ea20).
    if std::mem::take(&mut world.resource_mut::<MissionUi>().next_map) {
        let path = world
            .resource::<Loaded>()
            .scene
            .as_ref()
            .and_then(|s| retzar::gameplay::cheats::next_map(&s.mission).map(str::to_owned));
        if let Some(sim) = world
            .resource_mut::<Loaded>()
            .scene
            .as_mut()
            .and_then(|s| s.mission.simulation.as_mut())
            && path.is_some()
        {
            sim.gameplay.script.next_map = path;
        }
        if let Err(e) = super::menu_ui::next_map(world) {
            let mut menu = world.resource_mut::<MenuState>();
            menu.notice = e.to_string();
            menu.dirty = true;
        }
        return;
    }
    let transparent = world.resource::<MissionUi>().transparent_trees;
    let diplomacy = world.resource::<MissionUi>().hud.diplomacy.clone();
    let requests = std::mem::take(&mut world.resource_mut::<MissionUi>().requests);
    let said = std::mem::take(&mut world.resource_mut::<MissionUi>().said);
    let selected: BTreeSet<usize> = world
        .resource::<MissionUi>()
        .hud
        .selected
        .iter()
        .copied()
        .collect();
    world.resource_scope(|world, mut loaded: Mut<Loaded>| {
        let generation = loaded.generation;
        let Some(scene) = loaded.scene.as_mut() else {
            return;
        };
        let Some(mut sim) = scene.mission.simulation.take() else {
            return;
        };
        sim.gameplay.selected = selected;
        if let Some(relations) = diplomacy.as_ref().filter(|relations| relations.len() == 64) {
            scene.mission.diplomacy.clone_from(relations);
        }
        for line in &said {
            retzar::gameplay::ai::chat(&mut sim, &scene.mission, line);
        }
        world.resource_scope(|world, game: Mut<Game>| {
            for request in requests {
                let result = retzar::gameplay::issue(
                    &mut sim,
                    &mut scene.mission,
                    &scene.grid,
                    &scene.library,
                    &game.gameplay,
                    &request,
                );
                // The cheat panel's commands have no marker and no voice.
                if result.is_ok() && request.order != "cheat" {
                    if let Some(f) = order_marker(&sim, &scene.mission, &request) {
                        world.resource_mut::<MissionUi>().order_feedback =
                            Some(super::orders::Feedback {
                                started: world.resource::<Time>().elapsed_secs(),
                                generation,
                                ..f
                            });
                    }
                    // 0x4900b0, 0x470274: the first selected object answers
                    // with its command voice; an order on the map silences a
                    // unit first (0x490660).
                    if let Some(&first) = request.selected.first()
                        && let Some(audio) = world.get_resource::<SoundPlayer>()
                    {
                        let o = &scene.mission.objects[first];
                        let silenced = request.cell.is_some() && world::class::is_unit(o.class);
                        if silenced {
                            audio.player.stop_object(first);
                        }
                        let cue = voice(
                            &mut sim,
                            &scene.library,
                            &audio.player,
                            first,
                            o,
                            "sound_command",
                            retzar::audio::category::ORDER,
                            silenced,
                        );
                        if let Some(cue) = cue.filter(|c| heard(&settings, c.channel)) {
                            audio.player.play(retzar::audio::Cue {
                                place: Some(place([o.x, o.y])),
                                ..cue
                            });
                        }
                    }
                }
                // A refusal the original says goes to the message list
                // (vt+0x40, 0x501470); an order given says nothing.
                if let Err(e) = result
                    && game.gameplay.said(&e)
                {
                    let now = world.resource::<Time>().elapsed_secs();
                    super::mission::post(&mut world.resource_mut::<MissionUi>(), e, now);
                    world.resource_mut::<MenuState>().dirty = true;
                }
            }
            script_reply(world, scene, &mut sim, &game);
            let old = sim.ticks;
            if !paused {
                let speed = sim.speed;
                retzar::gameplay::advance_with_diplomacy(
                    &mut sim,
                    &mut scene.mission,
                    &mut scene.grid,
                    &scene.library,
                    &game.gameplay,
                    dt,
                    speed,
                    settings.weather,
                    diplomacy.as_deref(),
                );
            }
            if old / 5 != sim.ticks / 5 {
                world.resource_mut::<MenuState>().dirty = true;
            }
            for event in std::mem::take(&mut sim.sounds) {
                let Some(audio) = world.get_resource::<SoundPlayer>() else {
                    continue;
                };
                let object = scene.mission.objects.get(event.object);
                let Some(at) = event.cell.or(object.map(|o| [o.x, o.y])) else {
                    continue;
                };
                if event.channel != retzar::audio::Channel::Effect {
                    // 0x4931e0: the player's own notices go straight to the
                    // voices of units or buildings, from where they happened.
                    if heard(&settings, event.channel)
                        && let Some(group) = event.property.strip_prefix('@')
                        && let Some((name, _)) = scene.library.sound_group(group).pop()
                    {
                        audio.player.play(retzar::audio::Cue {
                            name,
                            category: event.category,
                            channel: event.channel,
                            place: Some(place(at)),
                            owner: Some(event.object),
                            looped: false,
                        });
                    }
                    continue;
                }
                // 0x4e2163: the dying object falls silent first.
                if event.property == "sound_die" {
                    audio.player.stop_object(event.object);
                }
                // 0x505610: heard only where the fog is lifted.
                let seen = match (event.cell, object) {
                    (Some(c), _) => sim.fog.at(c[0] as usize, c[1] as usize).1,
                    (None, Some(o)) => sim.visible(o),
                    _ => false,
                };
                let candidates = match (event.property.strip_prefix('@'), object) {
                    (Some(group), _) => scene.library.sound_group(group),
                    (None, Some(o)) => scene.library.sounds_for(o, event.property),
                    _ => Vec::new(),
                };
                if !seen || candidates.is_empty() {
                    continue;
                }
                // 0x455210: the clip is drawn from the stream of sounds and
                // pictures before a channel is sought.
                let roll = sim.gameplay.aux_rng.range(0, 1000);
                if let Some(name) = retzar::audio::pick(&candidates, roll) {
                    audio.player.play(retzar::audio::Cue {
                        name: name.to_owned(),
                        category: event.category,
                        channel: event.channel,
                        place: Some(place(at)),
                        owner: (event.cell.is_none() && !event.fixed).then_some(event.object),
                        looped: false,
                    });
                }
            }
            // The map's sounds placed with the editor (0x502953): the
            // object's own sound on the effects, category 20, heard where
            // the fog is lifted (0x505610), its clip drawn after that.
            if let Some(audio) = world.get_resource::<SoundPlayer>() {
                let owners = audio.player.owners();
                let acts =
                    retzar::gameplay::map_sounds(&mut sim, &scene.mission, |o| owners.contains(&o));
                for o in acts.silence {
                    audio.player.stop_object(o);
                }
                for (i, looped) in acts.start {
                    let s = &scene.mission.sounds[i];
                    let Some(o) = scene.mission.objects.get(s.object) else {
                        continue;
                    };
                    let candidates = scene.library.sound_group(&s.sound);
                    if !sim.visible(o) || candidates.is_empty() {
                        continue;
                    }
                    let roll = sim.gameplay.aux_rng.range(0, 1000);
                    if let Some(name) = retzar::audio::pick(&candidates, roll) {
                        audio.player.play(retzar::audio::Cue {
                            name: name.to_owned(),
                            category: retzar::audio::category::MAP_OBJECT,
                            channel: retzar::audio::Channel::Effect,
                            place: Some(place([o.x, o.y])),
                            owner: Some(s.object),
                            looped,
                        });
                    }
                }
            }
            if relocate && let Some(audio) = world.get_resource::<SoundPlayer>() {
                let objects = audio
                    .player
                    .owners()
                    .into_iter()
                    .filter_map(|i| {
                        let o = scene.mission.objects.get(i)?;
                        Some((i, [o.x as i32, o.y as i32]))
                    })
                    .collect();
                audio.player.listener(retzar::audio::Listener {
                    centre: hearing,
                    width: heard_width,
                    objects,
                });
            }
            // The step's lines, each in the message list (0x4f8a50); a
            // line for the status line stays its time (0x4f8350).
            let notices = std::mem::take(&mut sim.gameplay.notices);
            let status = sim.gameplay.status.take();
            if !notices.is_empty() || status.is_some() {
                let now = world.resource::<Time>().elapsed_secs();
                let mut ui = world.resource_mut::<MissionUi>();
                for notice in notices {
                    super::mission::post(&mut ui, notice, now);
                }
                if let Some((text, ms)) = status {
                    ui.status = Some((text, now + ms as f32 / 1000.));
                }
                world.resource_mut::<MenuState>().dirty = true;
            }
            script_shows(world, scene, &mut sim, &game);
            scene.mission.simulation = Some(sim);
            scene.mission.refresh_walls(&scene.library, scene.side);
            world.resource_scope(|world, mut renderer: Mut<Renderer>| {
                if renderer.generation != generation {
                    *renderer = Renderer {
                        generation,
                        ..Default::default()
                    };
                    // Imported sprites already match their restored action and direction.
                    for i in 0..scene.mission.objects.len() {
                        renderer.signatures.insert(i, signature(scene, i));
                    }
                }
                if let Err(e) = synchronize(world, scene, &mut renderer, &game.assets) {
                    let mut menu = world.resource_mut::<MenuState>();
                    menu.notice = format!("Grafica: {e}");
                    menu.dirty = true;
                }
                if renderer.transparent != transparent {
                    renderer.transparent = transparent;
                    let alpha = if transparent { 0.5 } else { 1. };
                    for (v, &e) in scene.visuals.iter().zip(&scene.visual_entities) {
                        if v.tree
                            && let Some(mut sprite) = world.get_mut::<Sprite>(e)
                        {
                            sprite.color.set_alpha(alpha);
                        }
                    }
                }
            });
            let mut ui = world.resource_mut::<MissionUi>();
            ui.hud
                .selected
                .retain(|&i| scene.mission.objects.get(i).is_some_and(|o| o.selectable()));
        });
    });
}

/// The player's reply to the script's window: a closed story or message
/// lets the rule go on; in a conversation it shows what follows.
fn script_reply(
    world: &mut World,
    scene: &mut Scene,
    sim: &mut retzar::simulation::Simulation,
    game: &Game,
) {
    let mut ui = world.resource_mut::<MissionUi>();
    let answer = ui.answer.take();
    let open = matches!(
        ui.hud.panel,
        Some(retzar::hud::Panel::Story | retzar::hud::Panel::Narration | retzar::hud::Panel::Say)
    );
    if !sim.gameplay.script.modal || (open && answer.is_none()) {
        return;
    }
    if sim.gameplay.script.conversation.is_some() {
        retzar::gameplay::answer(
            sim,
            &mut scene.mission,
            &scene.grid,
            &scene.library,
            &game.gameplay,
            answer.unwrap_or(0),
        );
    } else {
        retzar::gameplay::close_window(sim, &mut scene.mission);
    }
}

/// The corner of a cell, where the scripts put the view (0x4f6d00, 0x4f9140).
fn corner([x, y]: [u16; 2]) -> Vec2 {
    Vec2::new(x as f32 * 32., -(y as f32 * 32.))
}
/// The scripts' hold on the view, after the player's input of the frame.
/// LockView follows its object (0x5009d0), centred on the box of its picture
/// (vt+0x138), and ends without one, once it is gone or when its time is up
/// (0x4f3ee8). ScrollView moves the view every 75 ms at 1024 pixels a
/// second, then puts it on the last corner (0x4f44a9, 0x4f6e10).
fn script_camera(world: &mut World, scene: &Scene, now: f32) {
    let mut ui = world.resource_mut::<MissionUi>();
    let mut center = None;
    if let Some(scroll) = ui.view_scroll {
        let (at, done) = scroll.at(now);
        center = Some(at);
        if done {
            ui.view_scroll = None;
        }
    }
    if let Some(lock) = ui.view_lock {
        let body = lock.follow.and_then(|i| {
            let o = scene.mission.objects.get(i)?;
            if o.flags & (world::FLAG_DEAD | world::FLAG_INACTIVE) != 0 {
                return None;
            }
            scene
                .visuals
                .iter()
                .find(|v| v.object == i && v.layer == world::Layer::Body)
        });
        match body {
            Some(v) if lock.until.is_none_or(|t| now <= t) => {
                center = Some(Vec2::new(
                    v.bounds[0] + v.bounds[2] / 2.,
                    v.bounds[1] + v.bounds[3] / 2.,
                ));
            }
            _ => ui.view_lock = None,
        }
    }
    if let Some(at) = center {
        world.resource_mut::<super::view::MapView>().center = at;
    }
}

/// Carry out what the script asked of the interface, and open the end
/// screen once the game is over.
fn script_shows(
    world: &mut World,
    scene: &mut Scene,
    sim: &mut retzar::simulation::Simulation,
    game: &Game,
) {
    use retzar::gameplay::Show;
    let story = world
        .get_resource::<super::mission::StoryOnOpen>()
        .is_none_or(|s| s.0);
    // LockView, ScrollView and the timed story read the system clock
    // (`0x5cf568` vt+0x5c = GetTickCount: 0x4f923e, 0x4f6ddb, 0x4c3d3d), not
    // the game time the scripts run on: real time, with no cap on a long
    // frame. Apps without the time plugin (the tests) drive `Time` by hand.
    let elapsed = world
        .get_resource::<Time<Real>>()
        .map_or_else(|| world.resource::<Time>().elapsed(), Time::elapsed);
    let (now, now_ms) = (elapsed.as_secs_f32(), elapsed.as_millis() as u64);
    for show in std::mem::take(&mut sim.gameplay.script.outbox.0) {
        match show {
            Show::Story {
                sound,
                ms,
                close,
                music,
            } => {
                if story {
                    let mut ui = world.resource_mut::<MissionUi>();
                    ui.stories = ui.stories.wrapping_add(1);
                    let id = ui.stories;
                    ui.hud.set_panel(Some(retzar::hud::Panel::Narration));
                    ui.hud.crawl = (ms > 0).then_some([0, ms]);
                    // Its music, a MIDI of the map's files (0x537c90 through
                    // the engine's files, 0x514e40); an empty name or a
                    // missing file only silences the game's.
                    let key = music.replace('\\', "/");
                    let song = (!key.is_empty())
                        .then(|| {
                            scene
                                .source_entries
                                .iter()
                                .find(|(k, _)| k.eq_ignore_ascii_case(&key))
                                .map(|(_, data)| data.to_vec())
                        })
                        .flatten();
                    if let Some(audio) = world.get_resource::<SoundPlayer>() {
                        audio.player.story(id, song);
                    }
                    let mut ui = world.resource_mut::<MissionUi>();
                    ui.narration = Some(super::mission::NarrationRun {
                        id,
                        start: now_ms,
                        ms,
                        close,
                        voice: sound,
                        said: false,
                    });
                    world.resource_mut::<MenuState>().dirty = true;
                }
            }
            Show::Say => {
                if story {
                    say_bubbles(world, scene, sim);
                    let mut ui = world.resource_mut::<MissionUi>();
                    ui.hud.set_panel(Some(retzar::hud::Panel::Say));
                    let voices = !ui.hud.voices_off;
                    world.resource_mut::<MenuState>().dirty = true;
                    // A line being said speaks at once (0x4bf69b); choices
                    // only when one is taken.
                    let line = sim.gameplay.script.conversation.as_ref().and_then(|t| {
                        let p = t.phrase?;
                        Some(
                            scene.mission.conversations.get(t.conversation)?.phrases[p]
                                .voice
                                .clone(),
                        )
                    });
                    match line {
                        Some(voice) if voices => speak(world, scene, &voice),
                        _ => hush(world),
                    }
                }
            }
            Show::Close => {
                let mut ui = world.resource_mut::<MissionUi>();
                if matches!(
                    ui.hud.panel,
                    Some(
                        retzar::hud::Panel::Story
                            | retzar::hud::Panel::Narration
                            | retzar::hud::Panel::Say
                    )
                ) {
                    ui.hud.set_panel(None);
                }
                ui.hud.say.clear();
                hush(world);
            }
            Show::Note => {
                if story {
                    let mut ui = world.resource_mut::<MissionUi>();
                    ui.hud.set_panel(Some(retzar::hud::Panel::Story));
                    ui.hud.scroll = 0;
                }
            }
            Show::Line(text) => {
                super::mission::post(&mut world.resource_mut::<MissionUi>(), text, now);
                world.resource_mut::<MenuState>().dirty = true;
            }
            Show::Notice(key, name) => {
                let text = game.skin.locale.label("viewmsg", key).replace("%s", &name);
                super::mission::post(&mut world.resource_mut::<MissionUi>(), text, now);
                world.resource_mut::<MenuState>().dirty = true;
            }
            Show::Message(key, name) => {
                let text = game.skin.locale.label("msg", key).replace("%s", &name);
                super::mission::post(&mut world.resource_mut::<MissionUi>(), text, now);
                world.resource_mut::<MenuState>().dirty = true;
            }
            Show::Lock { cell, follow, ms } => {
                // 0x4f9140: a new lock takes the place of the old one.
                world.resource_mut::<super::view::MapView>().center = corner(cell);
                world.resource_mut::<MissionUi>().view_lock = Some(super::mission::ViewLock {
                    follow,
                    until: (ms > 0).then(|| now + ms as f32 / 1000.),
                });
            }
            Show::Scroll { from, to } => {
                world.resource_mut::<super::view::MapView>().center = corner(from);
                world.resource_mut::<MissionUi>().view_scroll =
                    Some(super::mission::ViewScroll::new(from, to, now));
            }
            // 0x453240 and 0x453340 do nothing while the view is locked
            // (0x589714).
            Show::Select(_) | Show::ClearSelection
                if world.resource::<MissionUi>().view_lock.is_some() => {}
            Show::Select(i) => {
                // 0x453240, as for a click.
                let mut ui = world.resource_mut::<MissionUi>();
                let before = ui.hud.selected.clone();
                retzar::selection::add(&mut ui.hud.selected, &[i], &scene.mission);
                if ui.hud.selected != before {
                    ui.hud.selection_changed();
                }
            }
            Show::Unselect(i) => {
                let mut ui = world.resource_mut::<MissionUi>();
                if ui.hud.selected.contains(&i) {
                    ui.hud.selected.retain(|&j| j != i);
                    ui.hud.selection_changed();
                }
            }
            Show::ClearSelection => {
                let mut ui = world.resource_mut::<MissionUi>();
                ui.hud.selected.clear();
                ui.hud.selection_changed();
            }
            Show::Diplomacy => {
                let mut ui = world.resource_mut::<MissionUi>();
                if ui.hud.diplomacy.is_some() {
                    ui.hud.diplomacy = Some(scene.mission.diplomacy.clone());
                }
            }
            Show::Unlock => world.resource_mut::<MissionUi>().view_lock = None,
            Show::Movie(name) => {
                // Over the game, which waits for it (`movie::inactive`).
                let source = world.resource::<MenuState>().source().to_path_buf();
                if let Some(request) = super::movie::Request::named(&source, &name)
                    && let Some(mut movie) = world.get_resource_mut::<super::movie::Movie>()
                {
                    movie.request = Some(request);
                }
            }
        }
    }
    script_camera(world, scene, now);
    // A bubble's eye (0x438030 with 2 → 0x4383c0): the view on its speaker,
    // the bubbles placed again (0x4386c0); the voices switched off stop
    // the one being said (0x4c06c1).
    let look = world.resource_mut::<MissionUi>().look.take();
    if let Some(i) = look
        .and_then(|k| world.resource::<MissionUi>().hud.say.get(k)?.object)
        .and_then(|i| {
            scene
                .visuals
                .iter()
                .find(|v| v.object == i && v.layer == Layer::Body)
        })
    {
        world.resource_mut::<super::view::MapView>().center = Vec2::new(
            i.bounds[0] + i.bounds[2] / 2.,
            i.bounds[1] + i.bounds[3] / 2.,
        );
        say_bubbles(world, scene, sim);
        world.resource_mut::<MenuState>().dirty = true;
    }
    if std::mem::take(&mut world.resource_mut::<MissionUi>().hush) {
        hush(world);
        world.resource_mut::<MenuState>().dirty = true;
    }
    // The story screen (0x4c3cc0): its voice 100 ms after it opens, the text
    // going up over its time, then its end when it closes by itself; the
    // voice goes with the screen.
    if let Some(run) = world.resource::<MissionUi>().narration.clone() {
        if world.resource::<MissionUi>().hud.panel != Some(retzar::hud::Panel::Narration) {
            world.resource_mut::<MissionUi>().narration = None;
            hush(world);
        } else {
            let elapsed = now_ms.saturating_sub(run.start);
            if !run.said && elapsed >= 100 {
                speak(world, scene, &run.voice);
                if let Some(r) = world.resource_mut::<MissionUi>().narration.as_mut() {
                    r.said = true;
                }
            }
            let mut ui = world.resource_mut::<MissionUi>();
            // It moves until its time is up, and stays then (0x4c3d54).
            if elapsed < run.ms {
                ui.hud.crawl = Some([elapsed, run.ms]);
            }
            if run.close && elapsed >= run.ms {
                ui.hud.set_panel(None);
            }
            world.resource_mut::<MenuState>().dirty = true;
        }
    }
    // The film framing (0x4f9b00): while the view is locked, glides, or a
    // conversation goes on (each blocks the view, +0x7e).
    let film = {
        let ui = world.resource::<MissionUi>();
        ui.view_lock.is_some()
            || ui.view_scroll.is_some()
            || sim.gameplay.script.conversation.is_some()
    };
    if world.resource::<MissionUi>().hud.cinematic != film {
        let mut ui = world.resource_mut::<MissionUi>();
        ui.hud.cinematic = film;
        // The film's end gives the map back its place beside the panels,
        // whatever F12 had made of it (0x4f9ade).
        if !film {
            ui.hud.full_map = false;
        }
        let frame = ui.hud.frame();
        world.resource_mut::<super::view::MapView>().frame = frame;
        world.resource_mut::<MenuState>().dirty = true;
    }
    let mut ui = world.resource_mut::<MissionUi>();
    // `$canusediplomacy = 0` (0x5877a0) keeps the window closed, and the
    // tribute window over it.
    if sim.gameplay.script.no_diplomacy
        && matches!(
            ui.hud.panel,
            Some(retzar::hud::Panel::Diplomacy | retzar::hud::Panel::Tribute)
        )
    {
        ui.hud.set_panel(None);
    }
    if let Some((won, at)) = sim.gameplay.script.over
        && !ui.ended
        && sim.ticks * retzar::gameplay::TICK_MS >= at + retzar::gameplay::END_DELAY_MS
    {
        ui.ended = true;
        let next = sim.gameplay.script.next_map.is_some();
        let mut menu = world.resource_mut::<MenuState>();
        menu.ending = Some((won, next));
        menu.ending_text = sim.gameplay.script.end_text.clone();
        menu.open(retzar::menu::Screen::End);
        menu.dirty = true;
        world
            .resource_mut::<NextState<super::Mode>>()
            .set(super::Mode::Menu);
    }
}

/// What a map order shows (0x490160 for the right click, the command
/// objects for the buttons): a right click on an object makes it blink, red
/// for an attack or a capture and green otherwise, and one on the ground
/// (or through an open gate or a bridge) leaves the red cross; a chosen
/// tree blinks through `treeselection`, whose picture `anim_none` has no
/// size. A command button makes its object blink red, blue for a guard, and
/// leaves nothing on the ground. Shift shows the waypoint flag instead.
fn order_marker(
    sim: &retzar::simulation::Simulation,
    mission: &retzar::world::MissionWorld,
    request: &retzar::gameplay::Request,
) -> Option<super::orders::Feedback> {
    use super::orders::{BLUE, Feedback, GREEN, RED};
    use retzar::gameplay::Intent;
    let cell = request.cell?;
    let first = *request.selected.first()?;
    let o = mission.objects.get(first)?;
    let rally = o.building() && retzar::gameplay::rally_building(o.class);
    if !world::class::is_unit(o.class) && !rally {
        return None;
    }
    let mut f = Feedback {
        cell,
        flag: request.append && !rally,
        target: None,
        color: GREEN,
        cross: None,
        started: 0.,
        generation: 0,
    };
    if request.append {
        return Some(f);
    }
    let intent = sim
        .gameplay
        .actors
        .get(first)
        .and_then(|a| a.intent.as_ref());
    let target = request.target.filter(|&t| t < mission.objects.len());
    if request.order != "context" {
        f.target = target;
        f.color = if request.order == "guard" { BLUE } else { RED };
        return Some(f);
    }
    match target {
        Some(t)
            if matches!(
                mission.objects[t].class,
                world::class::GATE_WOOD | world::class::GATE_STONE | world::class::BRIDGE
            ) && !matches!(intent, Some(Intent::Attack(_) | Intent::Capture(_))) =>
        {
            f.cross = request.point;
        }
        Some(t) => {
            f.target = Some(t);
            // Training on a dummy is action 0x42, not an attack: green.
            if matches!(intent, Some(Intent::Attack(_) | Intent::Capture(_)))
                && !rally
                && mission.objects[t].class != world::class::DUMMY
            {
                f.color = RED;
            }
        }
        None if matches!(intent, Some(Intent::Chop(_))) => {}
        None => f.cross = request.point,
    }
    Some(f)
}

/// The narration or a conversation's line, a WAV of the map's own files
/// (`USER\…`): the effects stop and it is said (0x4d0a00, 0x4d0a80); an
/// empty name or a missing file only silences the one before.
pub(super) fn speak(world: &World, scene: &Scene, name: &str) {
    // The maps' voices are recordings in the language of their texts (FX's
    // Italian campaigns; the American release has none), so in any other
    // language the lines are read, not heard.
    let key = if !retzar::lang::voiced(scene.mission.written_in) {
        String::new()
    } else {
        name.replace('\\', "/")
    };
    let pcm = (!key.is_empty())
        .then(|| {
            scene
                .source_entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&key))
                .and_then(|(_, data)| retzar::audio::Pcm::decode(data).ok())
        })
        .flatten();
    if let Some(audio) = world.get_resource::<SoundPlayer>() {
        audio.player.speech(pcm);
    }
}
/// The voice stops with its window (0x4bf83f).
pub(super) fn hush(world: &World) {
    if let Some(audio) = world.get_resource::<SoundPlayer>() {
        audio.player.speech(None);
    }
}

/// The view and the interface layout, to put map points on the interface.
fn interface_frame(world: &mut World) -> Option<(super::view::Surface, super::view::UiLayout)> {
    let settings = world.resource::<MenuState>().current_settings().clone();
    let mut q = world.query_filtered::<&Window, With<PrimaryWindow>>();
    let w = q.iter(world).next()?;
    let surface = super::view::Surface::of(w);
    Some((surface, super::view::hud_layout(surface, &settings)))
}
/// Interface pixel of a map point (y upwards).
fn interface_point(
    view: &super::view::MapView,
    surface: super::view::Surface,
    hud: &super::view::UiLayout,
    p: Vec2,
) -> Vec2 {
    let logical = surface.physical / surface.dpi;
    let screen = Vec2::new(
        (p.x - view.center.x) / view.scale + logical.x / 2.,
        logical.y / 2. - (p.y - view.center.y) / view.scale,
    );
    (screen * surface.dpi - hud.origin) / hud.factor
}
/// The box of an object's picture on the interface, while it is on the
/// visible map, between the film's bands (0x4f8d40).
fn on_screen(
    scene: &Scene,
    view: &super::view::MapView,
    surface: super::view::Surface,
    hud: &super::view::UiLayout,
    i: usize,
) -> Option<retzar::menu::Rect> {
    let v = scene
        .visuals
        .iter()
        .find(|v| v.object == i && v.layer == Layer::Body)?;
    let a = interface_point(
        view,
        surface,
        hud,
        Vec2::new(v.bounds[0], v.bounds[1] + v.bounds[3]),
    );
    let b = interface_point(
        view,
        surface,
        hud,
        Vec2::new(v.bounds[0] + v.bounds[2], v.bounds[1]),
    );
    let r = retzar::menu::Rect {
        x: a.x.round() as i32,
        y: a.y.round() as i32,
        w: (b.x - a.x).round().max(1.) as i32,
        h: (b.y - a.y).round().max(1.) as i32,
    };
    let band = retzar::hud::film_band(hud.size.y as i32);
    let (width, height) = (hud.size.x as i32, hud.size.y as i32);
    (r.x < width && r.x + r.w > 0 && r.y < height - band && r.y + r.h > band).then_some(r)
}
/// The bubbles of the conversation's line or choices (0x4380f0): one for
/// each speaker, the lines of the same speaker together, each in the free
/// place nearest to its speaker (0x4bf9a0). With no speaker on screen the
/// view goes to the first one first (0x4383c0).
fn say_bubbles(world: &mut World, scene: &Scene, sim: &retzar::simulation::Simulation) {
    let Some(talk) = sim.gameplay.script.conversation.clone() else {
        world.resource_mut::<MissionUi>().hud.say.clear();
        return;
    };
    let Some(phrases) = scene
        .mission
        .conversations
        .get(talk.conversation)
        .map(|c| c.phrases.clone())
    else {
        return;
    };
    // (speaker, lines with their answers, one line said)
    let mut groups: Vec<(u16, Vec<(usize, usize)>, bool)> = Vec::new();
    match talk.phrase {
        Some(p) => groups.push((phrases[p].speaker, vec![(0, p)], true)),
        None => {
            for (k, &p) in talk.choices.iter().enumerate() {
                let speaker = phrases[p].speaker;
                match groups.last_mut() {
                    Some(g) if g.0 == speaker => g.1.push((k, p)),
                    _ => groups.push((speaker, vec![(k, p)], false)),
                }
            }
        }
    }
    // The speaker, or the holder it is in (+0x7a).
    let objects: Vec<Option<usize>> = groups
        .iter()
        .map(|g| {
            let i = scene
                .mission
                .objects
                .iter()
                .position(|o| o.id == g.0 && g.0 != 0)?;
            Some(sim.objects.get(i).and_then(|s| s.holder).unwrap_or(i))
        })
        .collect();
    let Some((surface, hud)) = interface_frame(world) else {
        return;
    };
    let rects = |world: &World| {
        let view = *world.resource::<super::view::MapView>();
        objects
            .iter()
            .map(|o| o.and_then(|i| on_screen(scene, &view, surface, &hud, i)))
            .collect::<Vec<_>>()
    };
    let mut seen = rects(world);
    if seen.iter().all(Option::is_none)
        && let Some(v) = objects.iter().flatten().next().and_then(|&i| {
            scene
                .visuals
                .iter()
                .find(|v| v.object == i && v.layer == Layer::Body)
        })
    {
        world.resource_mut::<super::view::MapView>().center = Vec2::new(
            v.bounds[0] + v.bounds[2] / 2.,
            v.bounds[1] + v.bounds[3] / 2.,
        );
        seen = rects(world);
    }
    let mut used = [false; 6];
    let bubbles = groups
        .iter()
        .zip(&objects)
        .zip(&seen)
        .map(|((g, &object), &rect)| {
            let slot = retzar::hud::say_slot(&used, rect).unwrap_or(0);
            used[slot] = true;
            let portrait = object
                .map(|i| scene.mission.objects[i].portrait_large.clone())
                .filter(|p| {
                    scene
                        .portraits
                        .contains_key(&p.replace('\\', "/").to_ascii_uppercase())
                })
                .unwrap_or_default();
            retzar::hud::Bubble {
                slot,
                at: retzar::hud::SAY_SLOTS[slot],
                rows: g
                    .1
                    .iter()
                    .map(|&(k, p)| (k, phrases[p].text.clone()))
                    .collect(),
                portrait,
                speaker: rect.map(|r| [r.x + (r.w - 1) / 2, r.y + (r.h - 1) / 2]),
                object,
                voiced: retzar::lang::voiced(scene.mission.written_in)
                    && g.1
                        .iter()
                        .any(|&(_, p)| !phrases[p].voice.trim().is_empty()),
                single: g.2,
            }
        })
        .collect();
    world.resource_mut::<MissionUi>().hud.say = bubbles;
}
