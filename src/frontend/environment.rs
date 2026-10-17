//! Clock, stationary visibility and atmospheric effects. No object orders are executed.
use super::{
    MapCamera, MenuState, MissionUi, gpu_image,
    legacy_color::LegacyColor,
    scene::Loaded,
    view::{MapView, Surface, hud_layout},
};
use bevy::{camera::visibility::RenderLayers, prelude::*, window::PrimaryWindow};
use retzar::{
    Result,
    assets::GameAssets,
    bitmap::Bitmap,
    hud,
    simulation::{NativeRng, Weather},
};

pub struct Art {
    pub rain: Vec<Bitmap>,
    pub drops: Vec<Bitmap>,
    pub clouds: Vec<Bitmap>,
    pub shadows: Vec<Bitmap>,
    /// The fog's 15 frames of explored corners and of corners out of view.
    pub explore: Vec<Bitmap>,
    pub fog: Vec<Bitmap>,
}
impl Art {
    pub fn load(a: &GameAssets) -> Result<Self> {
        fn sheet(a: &GameAssets, path: &str, cols: u32, rows: u32) -> Result<Vec<Bitmap>> {
            let b = Bitmap::decode(&a.get(path)?, true)?;
            let (w, h) = (b.width / cols, b.height / rows);
            (0..cols * rows)
                .map(|i| b.crop(i % cols * w, i / cols * h, w, h))
                .collect()
        }
        // Cloud BMPs are greyscale blend masks, not opaque colour pictures.
        // Native 0x4fc85e selects mode 3 (brighten) and 0x4fc874 selects
        // mode 2 (darken): 0x458740 / 0x4587c0 interpolate using the mask.
        let mut clouds = sheet(a, "IMAGES/SCENERY/CLOUD.BMP", 1, 4)?;
        for b in &mut clouds {
            for p in b.rgba.chunks_exact_mut(4) {
                let alpha = p[0];
                p.copy_from_slice(&[255, 255, 255, alpha]);
            }
        }
        let mut shadows = sheet(a, "IMAGES/SCENERY/CLOUD SHADOW.BMP", 1, 4)?;
        for b in &mut shadows {
            for p in b.rgba.chunks_exact_mut(4) {
                let alpha = 255 - p[0];
                p.copy_from_slice(&[0, 0, 0, alpha]);
            }
        }
        // The fog (0x43c771): 32×32 frames for the masks 1–14 of a tile's
        // corners. explore.bmp is black where unexplored; fog_trans.bmp
        // multiplies each channel by 1 − ½·(31 − a)/31 (vt+0x50 → 0x459140,
        // LUT 0x4587c0 with 50; its masks are grey, so one channel serves),
        // a black of that opacity. Both skip the colour of fog_trans at
        // (0, 30).
        let explore = Bitmap::decode(&a.get("IMAGES/SCENERY/EXPLORE.BMP")?, false)?;
        let trans = Bitmap::decode(&a.get("IMAGES/SCENERY/FOG_TRANS.BMP")?, false)?;
        let at = (30 * trans.width as usize) * 4;
        let key = [trans.rgba[at], trans.rgba[at + 1], trans.rgba[at + 2]];
        let frames = |b: &Bitmap, paint: &dyn Fn([u8; 3]) -> [u8; 4]| -> Result<Vec<Bitmap>> {
            (0..15)
                .map(|i| {
                    let mut frame = b.crop(i * 32, 0, 32, 32)?;
                    for p in frame.rgba.chunks_exact_mut(4) {
                        let rgb = [p[0], p[1], p[2]];
                        p.copy_from_slice(&if rgb == key { [0; 4] } else { paint(rgb) });
                    }
                    Ok(frame)
                })
                .collect()
        };
        let explore = frames(&explore, &|c| [c[0], c[1], c[2], 255])?;
        let fog = frames(&trans, &|c| {
            let a = (c[0] >> 3) as u32;
            [0, 0, 0, ((31 - a) * 255 / 62) as u8]
        })?;
        Ok(Self {
            rain: sheet(a, "IMAGES/ANIMS/RAIN.BMP", 2, 3)?,
            drops: sheet(a, "IMAGES/ANIMS/RAINDROPS.BMP", 5, 1)?,
            clouds,
            shadows,
            explore,
            fog,
        })
    }
}
pub struct EnvironmentScene {
    /// Fog pictures: unexplored black, the half-dark tile and the 15 frames
    /// of each mask; the sprites reused for the tiles in view and what they
    /// show (fog revision and tile rectangle).
    fog_black: Handle<Image>,
    fog_grey: Handle<Image>,
    fog_explore: Vec<Handle<Image>>,
    fog_trans: Vec<Handle<Image>>,
    fog_tiles: Vec<Entity>,
    fog_drawn: Option<(u64, [i32; 4])>,
    revision: u64,
    hour: u16,
    diplomacy: Vec<u32>,
    rain: Vec<Handle<Image>>,
    drops: Vec<Handle<Image>>,
    snow: Handle<Image>,
    particles: Vec<Entity>,
    /// The precipitation of the original (0x4a6ef0): particles, wind and
    /// its 70 ms clock.
    precipitation: Precipitation,
    clouds: Vec<(Entity, Entity, Vec2)>,
    /// The four cloud pictures with their shadows, and the clouds alive.
    cloud_art: Vec<(Handle<Image>, Handle<Image>, Vec2)>,
    sky: Sky,
}
pub fn restore_diplomacy(scene: &mut super::scene::Scene, hud: &hud::HudState) {
    if let Some(env) = scene.environment.as_mut() {
        env.diplomacy = hud
            .diplomacy
            .as_ref()
            .unwrap_or(&scene.mission.diplomacy)
            .clone();
    }
}
pub fn install(world: &mut World, scene: &mut super::scene::Scene, art: Art) {
    if scene.mission.simulation.is_none() {
        return;
    }
    let mut upload = |pictures: Vec<Bitmap>| -> Vec<Handle<Image>> {
        pictures
            .into_iter()
            .map(|b| {
                let h = world.resource_mut::<Assets<Image>>().add(gpu_image(b));
                scene.images.push(h.clone());
                h
            })
            .collect()
    };
    let rain = upload(art.rain);
    let drops = upload(art.drops);
    let clouds = upload(art.clouds);
    let shadows = upload(art.shadows);
    let fog_explore = upload(art.explore);
    let fog_trans = upload(art.fog);
    let mut fog_flat = upload(vec![
        Bitmap {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        },
        // 0x4590a0 halves every pixel of a tile wholly out of view.
        Bitmap {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 128],
        },
    ]);
    let fog_grey = fog_flat.remove(1);
    let fog_black = fog_flat.remove(0);
    // 0x4a6f94: snowflakes are solid white squares, 2×2 or 3×3.
    let snow = upload(vec![Bitmap {
        width: 1,
        height: 1,
        rgba: vec![255, 255, 255, 255],
    }])
    .remove(0);
    let particles = (0..256)
        .map(|_| {
            let e = world
                .spawn((
                    Sprite::from_image(rain[0].clone()),
                    Transform::from_xyz(0., 0., 600.),
                    Visibility::Hidden,
                    RenderLayers::layer(1),
                ))
                .id();
            scene.entities.push(e);
            e
        })
        .collect();
    let cloud_art: Vec<(Handle<Image>, Handle<Image>, Vec2)> = (0..4)
        .map(|i| {
            let size = world
                .resource::<Assets<Image>>()
                .get(&clouds[i])
                .unwrap()
                .size()
                .as_vec2();
            (clouds[i].clone(), shadows[i].clone(), size)
        })
        .collect();
    // 0x4fc2b0: at most 16 clouds.
    let clouds = (0..16)
        .map(|i| {
            let size = cloud_art[i % 4].2;
            let mut entity = |image: Handle<Image>, z| {
                let e = world
                    .spawn((
                        Sprite::from_image(image),
                        Transform::from_xyz(0., 0., z),
                        RenderLayers::layer(1),
                    ))
                    .id();
                scene.entities.push(e);
                e
            };
            (
                entity(clouds[i % 4].clone(), 410.),
                entity(shadows[i % 4].clone(), 409.9),
                size,
            )
        })
        .collect();
    scene.environment = Some(EnvironmentScene {
        fog_black,
        fog_grey,
        fog_explore,
        fog_trans,
        fog_tiles: Vec::new(),
        fog_drawn: None,
        revision: u64::MAX,
        hour: 600,
        diplomacy: vec![],
        rain,
        drops,
        snow,
        particles,
        precipitation: Precipitation::default(),
        clouds,
        cloud_art,
        sky: Sky::default(),
    });
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    _time: Res<Time>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut loaded: ResMut<Loaded>,
    mut menu: ResMut<MenuState>,
    mut ui: ResMut<MissionUi>,
    view: Res<MapView>,
    mut camera: Query<&mut LegacyColor, With<MapCamera>>,
    mut sprites: Query<(&mut Sprite, &mut Transform, &mut Visibility)>,
    mut commands: Commands,
) {
    let Some(scene) = loaded.scene.as_mut() else {
        return;
    };
    let Some(mut sim) = scene.mission.simulation.take() else {
        return;
    };
    // The weather is the game's; how it looks follows the settings'
    // draft while they are open.
    let settings = menu.current_settings().clone();
    sim.update_weather(menu.current_settings().weather);
    for mut color in &mut camera {
        let [r, g, b] = sim.lighting(settings.day_night);
        // The fourth value keeps the original's RGB555 steps; 0 is the true
        // colour of the «GRAFICA» page.
        color.light = Vec4::new(r, g, b, if settings.true_color { 0. } else { 1. });
    }
    let d = ui
        .hud
        .diplomacy
        .as_deref()
        .unwrap_or(&scene.mission.diplomacy);
    if let Some(env) = scene.environment.as_mut() {
        if env.diplomacy != d {
            sim.refresh_visibility(&scene.mission, Some(d));
            env.diplomacy = d.to_vec();
        }
        if env.revision != sim.fog.revision {
            env.revision = sim.fog.revision;
            for (v, &entity) in scene.visuals.iter().zip(&scene.visual_entities) {
                if let Some(o) = scene.mission.objects.get(v.object)
                    && let Ok((_, _, mut visible)) = sprites.get_mut(entity)
                {
                    // Others' buildings show only once seen, as remembered
                    // while the fog covers them (0x4204b0).
                    *visible = if !o.selectable()
                        || sim.visible(o)
                        || (!retzar::selection::unit(o)
                            && sim.known(o)
                            && sim.appearance(v.object, o).is_some())
                    {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                }
            }
            let n = ui.hud.selected.len();
            ui.hud
                .selected
                .retain(|&i| scene.mission.objects.get(i).is_some_and(|o| sim.visible(o)));
            if n != ui.hud.selected.len() {
                ui.hud.selection_changed();
                menu.dirty = true;
            }
        }
        let hour = sim.clock_minutes();
        if hour != env.hour {
            env.hour = hour;
            menu.dirty = true;
        }
        let surface = Surface::of(&window);
        let layout = hud_layout(surface, &settings);
        let rect = hud::play_rect(
            layout.size.x as i32,
            layout.size.y as i32,
            settings.layout == 1,
            view.frame,
        );
        let factor = layout.factor / surface.dpi;
        let lo = view.world(surface, Vec2::new(rect.x as f32, rect.y as f32) * factor);
        let hi = view.world(
            surface,
            Vec2::new((rect.x + rect.w) as f32, (rect.y + rect.h) as f32) * factor,
        );
        let span = (hi - lo).abs();
        // The fog, tile by tile (0x4f0960). Each tile reads its four corner
        // vertices: bit 1 top left, 2 top right, 4 bottom left, 8 bottom
        // right. None explored: black; some: explore.bmp frame E − 1. Then,
        // where explored, the corners out of view: all of them halve the
        // tile, some take the fog_trans frame F − 1.
        let side = scene.side as i32;
        let rect = [
            ((lo.x.min(hi.x) / 32.).floor() as i32 - 1).clamp(0, side),
            ((-lo.y.max(hi.y) / 32.).floor() as i32 - 1).clamp(0, side),
            ((lo.x.max(hi.x) / 32.).ceil() as i32 + 1).clamp(0, side),
            ((-lo.y.min(hi.y) / 32.).ceil() as i32 + 1).clamp(0, side),
        ];
        if settings.soft_fog {
            // «Nebbia morbida» (`soft_fog`) takes the place of the tiles.
            if env.fog_drawn.take().is_some() {
                for &e in &env.fog_tiles {
                    if let Ok((_, _, mut visible)) = sprites.get_mut(e) {
                        *visible = Visibility::Hidden;
                    }
                }
            }
        } else if env.fog_drawn != Some((sim.fog.revision, rect)) {
            env.fog_drawn = Some((sim.fog.revision, rect));
            let mut tiles: Vec<(Handle<Image>, i32, i32, f32)> = Vec::new();
            for ty in rect[1]..rect[3] {
                for tx in rect[0]..rect[2] {
                    let corners = [(0, 0, 1), (1, 0, 2), (0, 1, 4), (1, 1, 8)];
                    let (mut explored, mut dark) = (0, 0);
                    for (dx, dy, bit) in corners {
                        let (seen, view) = sim.fog.vertex((tx + dx) as usize, (ty + dy) as usize);
                        if seen {
                            explored |= bit;
                        }
                        if !view {
                            dark |= bit;
                        }
                    }
                    if explored == 0 {
                        tiles.push((env.fog_black.clone(), tx, ty, 451.));
                        continue;
                    }
                    if explored < 15 {
                        tiles.push((env.fog_explore[explored - 1].clone(), tx, ty, 451.));
                    }
                    if dark == 15 {
                        tiles.push((env.fog_grey.clone(), tx, ty, 450.));
                    } else if dark != 0 {
                        tiles.push((env.fog_trans[dark - 1].clone(), tx, ty, 450.));
                    }
                }
            }
            for (k, (image, tx, ty, z)) in tiles.iter().enumerate() {
                let at = Vec3::new(*tx as f32 * 32. + 16., -(*ty as f32 * 32. + 16.), *z);
                if let Some(&e) = env.fog_tiles.get(k) {
                    if let Ok((mut sprite, mut place, mut visible)) = sprites.get_mut(e) {
                        if sprite.image != *image {
                            sprite.image = image.clone();
                        }
                        place.translation = at;
                        *visible = Visibility::Inherited;
                    }
                } else {
                    let e = commands
                        .spawn((
                            Sprite {
                                custom_size: Some(Vec2::splat(32.)),
                                ..Sprite::from_image(image.clone())
                            },
                            Transform::from_translation(at),
                            RenderLayers::layer(1),
                        ))
                        .id();
                    env.fog_tiles.push(e);
                    scene.entities.push(e);
                }
            }
            for &e in &env.fog_tiles[tiles.len().min(env.fog_tiles.len())..] {
                if let Ok((_, _, mut visible)) = sprites.get_mut(e) {
                    *visible = Visibility::Hidden;
                }
            }
        }
        let sec = sim.elapsed.as_secs_f32();
        // Precipitation (0x4a6ef0): a new shower resets its wind; each 70 ms
        // tick moves the particles, which live in screen space and follow
        // the map when the view scrolls (0x4a78c0).
        let snow = sim.weather == Weather::Snow;
        let count = (256. * sim.precipitation) as usize;
        let size = [span.x.max(8.) as i32, span.y.max(8.) as i32];
        let origin = [
            lo.x.min(hi.x).round() as i32,
            (-lo.y.max(hi.y)).round() as i32,
        ];
        let rain = &mut env.precipitation;
        if count > 0 && !rain.active {
            *rain = Precipitation {
                a: [5, 5],
                b: [4, 6],
                countdown: 10,
                tick: (sec / 0.07) as i64 - 1,
                active: true,
                ..Default::default()
            };
        } else if count == 0 {
            rain.active = false;
            rain.particles.clear();
        }
        rain.scroll(origin, size);
        let now = (sec / 0.07) as i64;
        while rain.active && rain.tick < now {
            rain.tick += 1;
            rain.step(snow, count, size, &mut sim.gameplay.aux_rng);
        }
        let (a, b) = (rain.a, rain.b);
        for (i, &entity) in env.particles.iter().enumerate() {
            let Ok((mut sprite, mut at, mut visible)) = sprites.get_mut(entity) else {
                continue;
            };
            let Some(p) = env.precipitation.particles.get(i).copied() else {
                *visible = Visibility::Hidden;
                continue;
            };
            at.translation.x = (origin[0] + p.x) as f32 + 2.;
            at.translation.y = -((origin[1] + p.y) as f32) - 2.;
            let (cx, cy) = (
                (at.translation.x.max(0.) / 32.) as usize,
                ((-at.translation.y).max(0.) / 32.) as usize,
            );
            // 0x4a83d0: drawn only where all four corners are in view.
            let seen = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .iter()
                .all(|&(dx, dy)| sim.fog.vertex(cx + dx, cy + dy).1);
            *visible = if seen {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if snow {
                sprite.image = env.snow.clone();
                sprite.custom_size = Some(Vec2::splat(if p.big { 3. } else { 2. }));
                sprite.color = Color::WHITE;
            } else if p.life >= 3 {
                // A falling drop: the picture of its layer's slant, half
                // blended (0x4a8670).
                let slant = Precipitation::slant(if p.layer_b { b } else { a });
                sprite.image = env.drops[slant.min(env.drops.len() - 1)].clone();
                sprite.custom_size = Some(Vec2::splat(4.));
                sprite.color = Color::srgba(1., 1., 1., 0.5);
            } else {
                // The splash, row 2 − life, its water column on water.
                let water = scene
                    .grid
                    .cells
                    .get(cy * scene.side + cx)
                    .is_some_and(|&v| v >> 4 & 7 == 0 && v >> 7 & 7 == 0);
                if p.life < 0 {
                    *visible = Visibility::Hidden;
                }
                let frame = (2 - p.life.clamp(0, 2)) as usize * 2 + water as usize;
                sprite.image = env.rain[frame.min(env.rain.len() - 1)].clone();
                sprite.custom_size = Some(Vec2::splat(4.));
                sprite.color = Color::WHITE;
            }
        }
        // Clouds (0x4fc43e, 0x4fc9b0): drawn at priority 400, the shadow
        // just before its body, 50 pixels right and 88 down (0x4fcaf0).
        let view_cells = [
            (lo.x.min(hi.x) / 32.).floor() as i32,
            (-lo.y.max(hi.y) / 32.).floor() as i32,
            (lo.x.max(hi.x) / 32.).ceil() as i32,
            (-lo.y.min(hi.y) / 32.).ceil() as i32,
        ];
        let on = sim.clouds && settings.weather;
        env.sky.step(
            (sec * 1000.) as i64,
            scene.side as i32,
            view_cells,
            on,
            &mut sim.gameplay.aux_rng,
        );
        for (k, (body, shadow, _)) in env.clouds.iter().enumerate() {
            let cloud = env.sky.clouds.get(k).copied().filter(|_| on);
            for (&entity, is_shadow) in [(body, false), (shadow, true)] {
                let Ok((mut sprite, mut at, mut visible)) = sprites.get_mut(entity) else {
                    continue;
                };
                let Some(c) = cloud else {
                    *visible = Visibility::Hidden;
                    continue;
                };
                let (image, shade, size) = &env.cloud_art[c.row];
                let picture = if is_shadow { shade } else { image };
                if sprite.image != *picture {
                    sprite.image = picture.clone();
                }
                let (ox, oy) = if is_shadow { (50., 88.) } else { (0., 0.) };
                at.translation = Vec3::new(
                    c.x as f32 + ox + size.x / 2.,
                    -(c.y as f32 + oy + size.y / 2.),
                    if is_shadow { 409.9 } else { 410. },
                );
                *visible = Visibility::Inherited;
            }
        }
    }
    // The chosen tree (message 0x2204 of `treeselection`, 0x437ac0) follows
    // its wood and leaves the selection when felled or bare.
    if let Some((cell, shown)) = ui.hud.tree {
        let wood = retzar::selection::tree_wood(&sim, cell);
        if !ui.hud.selected.is_empty()
            || wood == 0
            || !retzar::selection::standing_tree(&scene.library, &scene.grid, cell)
        {
            ui.hud.tree = None;
            ui.hud.selection_changed();
            menu.dirty = true;
        } else if wood != shown {
            ui.hud.tree = Some((cell, wood));
            menu.dirty = true;
        }
    }
    scene.mission.simulation = Some(sim);
}
/// A cloud (class 84, 0x4fc43e): its corner in map pixels, its step every
/// 100 ms, its picture and when it may go.
#[derive(Clone, Copy)]
struct Cloud {
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
    row: usize,
    moved_ms: i64,
    ends_ms: i64,
}
/// The clouds' controller (0x4fc3b0): a new cloud every 4–6 s while fewer
/// than 16 are alive.
#[derive(Default)]
pub struct Sky {
    clouds: Vec<Cloud>,
    next_ms: i64,
}
impl Sky {
    /// Advance to `now`; `view` is the visible map in cells (x0, y0, x1, y1).
    fn step(&mut self, now: i64, side: i32, view: [i32; 4], on: bool, rng: &mut NativeRng) {
        let near = |x: i32, y: i32| {
            let (cx, cy) = (x / 32, y / 32);
            cx >= view[0] - 8 && cx <= view[2] + 8 && cy >= view[1] - 8 && cy <= view[3] + 8
        };
        if self.next_ms == 0 {
            self.next_ms = now + rng.range(4000, 6000) as i64;
        }
        while on && now >= self.next_ms {
            self.next_ms += rng.range(4000, 6000) as i64;
            if self.clouds.len() >= 16 {
                continue;
            }
            // Born out of the view enlarged by 8 cells, with a drift of
            // −2..2 pixels on each axis (never still) and 6–10 s of life.
            let (x, y) = (rng.range(0, side - 1) * 32, rng.range(0, side - 1) * 32);
            if near(x, y) {
                continue;
            }
            let dy = rng.range(-2, 2);
            let dx = rng.range(-2, 2);
            let (dx, dy) = if (dx, dy) == (0, 0) {
                (1, -1)
            } else {
                (dx, dy)
            };
            let life = rng.range(6000, 10000) as i64;
            let row = rng.range(0, 3) as usize;
            self.clouds.push(Cloud {
                x,
                y,
                dx,
                dy,
                row,
                moved_ms: now + 50,
                ends_ms: now + life,
            });
        }
        let mut k = 0;
        while k < self.clouds.len() {
            let c = &mut self.clouds[k];
            while now - c.moved_ms >= 100 {
                c.moved_ms += 100;
                c.x += c.dx;
                c.y += c.dy;
            }
            // At the end of its life a cloud near the view waits 500 ms more.
            if now >= c.ends_ms {
                if near(c.x, c.y) {
                    c.ends_ms += 500;
                } else {
                    self.clouds.remove(k);
                    continue;
                }
            }
            k += 1;
        }
    }
}
/// One raindrop or snowflake on the screen (0x4a6ef0): position in view
/// pixels, life, and the wind layer it follows (A or B).
#[derive(Clone, Copy, Default)]
struct Particle {
    x: i32,
    y: i32,
    life: i32,
    layer_b: bool,
    big: bool,
}
/// The rain object (0x4a6d62): two wind layers, the countdown to the next
/// change and the 70 ms clock, reset with each shower, and the view's
/// corner in map pixels.
#[derive(Default)]
pub struct Precipitation {
    particles: Vec<Particle>,
    a: [i32; 2],
    b: [i32; 2],
    countdown: i32,
    tick: i64,
    active: bool,
    origin: Option<[i32; 2]>,
}
impl Precipitation {
    /// The view moved to `origin` (0x4a78c0): every particle shifts back by
    /// the scroll, so it keeps its place on the map, and one pushed out of a
    /// view of `size` comes back from the other side (0x4a7a70).
    fn scroll(&mut self, origin: [i32; 2], size: [i32; 2]) {
        let Some(old) = self.origin.replace(origin) else {
            return;
        };
        let d = [old[0] - origin[0], old[1] - origin[1]];
        if d == [0, 0] {
            return;
        }
        let wrap = |v: i32, n: i32| {
            let n = n.max(1);
            let v = match v {
                _ if v >= 0 => v,
                _ if v >= -n => v + n,
                _ => v % n + n,
            };
            match v {
                _ if v <= n => v,
                _ if v < 2 * n => v - n,
                _ => v % n,
            }
        };
        for p in &mut self.particles {
            p.x = wrap(p.x + d[0], size[0] - 4);
            p.y = wrap(p.y + d[1], size[1] - 4);
        }
    }
    /// Slant of a wind (0x4a6ce0): one of five drop pictures.
    fn slant(v: [i32; 2]) -> usize {
        let r = v[0] * 100 / v[1].max(1);
        match r {
            _ if r < -80 => 0,
            _ if r > 80 => 4,
            _ if r < -30 => 1,
            _ if r > 30 => 3,
            _ => 2,
        }
    }
    /// A change of wind (0x4a72ce–0x4a7422).
    fn change(&mut self, snow: bool, rng: &mut NativeRng) {
        let mut wx = self.a[0];
        if snow {
            if wx > 4 {
                wx -= rng.range(1, 3);
            } else if wx < -4 {
                wx += rng.range(1, 3);
            } else {
                wx += rng.range(-2, 2);
            }
            let vy = rng.range(3, 6);
            self.a = [wx, vy];
            self.b = [wx + rng.range(-2, 2), vy + rng.range(-2, 2)];
        } else {
            // Past ±10 the wind turns back to 10 − R(1, 8), even from the
            // west (sic).
            if !(-10..=10).contains(&wx) {
                wx = 10 - rng.range(1, 8);
            } else {
                wx += rng.range(-8, 8);
            }
            let vy = rng.range(26, 36);
            self.a = [wx, vy];
            self.b = [wx / 2 + rng.range(-3, 3), vy / 2 + rng.range(-4, 4)];
        }
    }
    /// One 70 ms step of `count` particles in a view of `size` pixels.
    fn step(&mut self, snow: bool, count: usize, size: [i32; 2], rng: &mut NativeRng) {
        if self.countdown == 0 {
            self.change(snow, rng);
            self.countdown = rng.range(30, 90) + 1;
        } else {
            self.countdown -= 1;
        }
        while self.particles.len() < count {
            let p = Particle {
                x: rng.range(0, size[0] - 3),
                y: rng.range(0, size[1] - 3),
                life: 0,
                layer_b: if snow {
                    rng.range(101, 102) == 102
                } else {
                    rng.range(0, 9) < 5
                },
                big: false,
            };
            let big = snow && p.layer_b;
            self.particles.push(Particle { big, ..p });
        }
        self.particles.truncate(count);
        let (a, b) = (self.a, self.b);
        for k in 0..self.particles.len() {
            let mut p = self.particles[k];
            if p.life <= 0 {
                p.x = rng.range(0, size[0] - 5);
                p.y = rng.range(0, size[1] - 5);
                p.life = rng.range(-10, 10) + if snow { 45 } else { 20 };
            }
            let v = if p.layer_b { b } else { a };
            if snow {
                let jitter = rng.range(0, 4);
                p.x += if v[0] >= 0 {
                    v[0] + jitter
                } else {
                    v[0] - jitter
                };
                p.y += v[1] + rng.range(0, 4);
            } else if p.life >= 3 {
                p.x += v[0];
                p.y += v[1];
            }
            p.life -= 1;
            p.x = p.x.rem_euclid((size[0] - 3).max(1));
            p.y = p.y.rem_euclid((size[1] - 3).max(1));
            self.particles[k] = p;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drops(at: &[[i32; 2]]) -> Precipitation {
        Precipitation {
            particles: at
                .iter()
                .map(|&[x, y]| Particle {
                    x,
                    y,
                    life: 5,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
    fn places(rain: &Precipitation) -> Vec<[i32; 2]> {
        rain.particles.iter().map(|p| [p.x, p.y]).collect()
    }

    #[test]
    fn drops_keep_their_place_on_the_map_when_the_view_scrolls() {
        // A 644×484 view: 0x4a7a70 wraps within 640×480.
        let size = [644, 484];
        let mut rain = drops(&[[100, 100], [5, 470], [630, 10]]);
        rain.scroll([1000, 2000], size);
        assert_eq!(places(&rain), [[100, 100], [5, 470], [630, 10]]);
        // 0x4a78c0: the view goes 20 right and 15 down, the drops 20 left
        // and 15 up; who leaves the view comes back from the other side.
        rain.scroll([1020, 2015], size);
        assert_eq!(places(&rain), [[80, 85], [625, 455], [610, 475]]);
        // Back again: past the far edge a drop re-enters near the start.
        rain.scroll([1000, 2000], size);
        assert_eq!(places(&rain), [[100, 100], [5, 470], [630, 10]]);
        // A jump of several views wraps by the remainder.
        rain.scroll([3000, 2000], size);
        assert_eq!(places(&rain)[0], [(100 - 2000) % 640 + 640, 100]);
    }
}
