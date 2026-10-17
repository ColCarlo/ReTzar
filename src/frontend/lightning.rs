//! The lightning bolts on the map (class 101): every 100 ms a new wander
//! (0x46d510 → vt+0x2b0), drawn as one-pixel lines (0x46dd70): the outer
//! pairs blended at 50 − 39 (k + 1) / branches per cent (0x46c230 with the
//! table of 0x458840) in the half colour, the two inner neighbours in the
//! half colour, the centre in the full one, segment by segment.
use super::{gpu_image, scene::Loaded};
use bevy::{camera::visibility::RenderLayers, prelude::*};
use retzar::{
    bitmap::Bitmap,
    bolt::{self, Bolt, Shape},
};

struct Live {
    bolt: Bolt,
    shape: Shape,
    normal: (f64, f64),
    born_ms: i64,
    ticks: i64,
    entity: Entity,
}
#[derive(Resource, Default)]
pub struct Bolts {
    generation: u32,
    live: Vec<Live>,
}

/// RGB555 as the decoder widens it.
fn rgb(c: u16) -> [f32; 3] {
    [10, 5, 0].map(|shift| {
        let v = ((c >> shift) & 0x1f) as u32;
        ((v << 3) | (v >> 2)) as f32 / 255.
    })
}
/// The native halving: each channel shifted down one bit (mask 0x3def).
fn half(c: u16) -> u16 {
    (c >> 1) & 0x3def
}
/// The surface's line (0x46c230) from `a` to `b`, unclipped: one pixel a
/// step of the longer axis (x when |dx| > |dy|), the other axis one step
/// further each time an accumulator, started at half a pixel (0x8000),
/// passes 0x10000 by adding 0x10000 × short / long, truncated (0x521030).
fn line(a: [i32; 2], b: [i32; 2], mut plot: impl FnMut(i32, i32)) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    if dx == 0 && dy == 0 {
        // vt+0x34: a single pixel.
        plot(a[0], a[1]);
        return;
    }
    let (long, short) = if dx.abs() > dy.abs() {
        (dx.abs(), dy.abs())
    } else {
        (dy.abs(), dx.abs())
    };
    let ratio = (0x10000i64 * short as i64) / long as i64;
    let (sx, sy) = (dx.signum(), dy.signum());
    let x_long = dx.abs() > dy.abs();
    let [mut x, mut y] = a;
    let mut acc = 0x8000i64;
    for _ in 0..=long {
        plot(x, y);
        acc += ratio;
        let across = acc > 0x10000;
        if across {
            acc -= 0x10000;
        }
        if x_long {
            x += sx;
            if across {
                y += sy;
            }
        } else {
            y += sy;
            if across {
                x += sx;
            }
        }
    }
}
/// The bolt in a picture of its bounding box (premultiplied while drawing,
/// straight at the end) and the box's corner in map pixels.
fn render(live: &Live, showing: usize) -> (Bitmap, [i32; 2]) {
    let s = &live.shape;
    let all = s
        .centre
        .iter()
        .chain(&s.minus)
        .chain(&s.plus)
        .chain(s.outer.iter().flat_map(|(a, b)| a.iter().chain(b)));
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for p in all {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let (w, h) = ((x1 - x0 + 1).max(1) as usize, (y1 - y0 + 1).max(1) as usize);
    let mut px = vec![[0f32; 4]; w * h];
    let mut put = |x: i32, y: i32, c: [f32; 3], a: f32| {
        let (u, v) = ((x - x0) as usize, (y - y0) as usize);
        if u < w && v < h {
            let d = &mut px[v * w + u];
            for k in 0..3 {
                d[k] = c[k] * a + d[k] * (1. - a);
            }
            d[3] = a + d[3] * (1. - a);
        }
    };
    // 0x46ddc5: more than six bolts drop a lean one's outer lines; more
    // than eighteen thin them all.
    let pairs = s.outer.len();
    let drawn = if showing > 6 && live.bolt.lean {
        0
    } else if showing > 18 {
        pairs * 18 / showing
    } else {
        pairs
    };
    for i in 1..s.centre.len() {
        let colour = s.colours[i];
        let dim = rgb(half(colour));
        for (k, (minus, plus)) in s.outer.iter().take(drawn).enumerate() {
            let level = (50 - 39 * (k as i32 + 1) / pairs as i32) as f32 / 100.;
            line(minus[i - 1], minus[i], |x, y| put(x, y, dim, level));
            line(plus[i - 1], plus[i], |x, y| put(x, y, dim, level));
        }
        line(s.minus[i - 1], s.minus[i], |x, y| put(x, y, dim, 1.));
        line(s.plus[i - 1], s.plus[i], |x, y| put(x, y, dim, 1.));
        let full = rgb(colour);
        line(s.centre[i - 1], s.centre[i], |x, y| put(x, y, full, 1.));
    }
    let rgba = px
        .iter()
        .flat_map(|p| {
            let a = p[3];
            let straight = |c: f32| if a > 0. { c / a } else { 0. };
            [
                (straight(p[0]) * 255.).round() as u8,
                (straight(p[1]) * 255.).round() as u8,
                (straight(p[2]) * 255.).round() as u8,
                (a * 255.).round() as u8,
            ]
        })
        .collect();
    (
        Bitmap {
            width: w as u32,
            height: h as u32,
            rgba,
        },
        [x0, y0],
    )
}
fn place(world: &mut World, live: &Live, showing: usize) {
    let (b, corner) = render(live, showing);
    let size = Vec2::new(b.width as f32, b.height as f32);
    let image = world.resource_mut::<Assets<Image>>().add(gpu_image(b));
    if let Some(mut sprite) = world.get_mut::<Sprite>(live.entity) {
        sprite.image = image;
        sprite.custom_size = Some(size);
    }
    if let Some(mut t) = world.get_mut::<Transform>(live.entity) {
        *t = Transform::from_xyz(
            corner[0] as f32 + size.x / 2.,
            -(corner[1] as f32 + size.y / 2.),
            395.,
        );
    }
}
pub fn update(world: &mut World) {
    if !world.contains_resource::<Bolts>() {
        world.init_resource::<Bolts>();
    }
    world.resource_scope(|world, mut bolts: Mut<Bolts>| {
        let (generation, now, new, mut rng) = {
            let mut loaded = world.resource_mut::<Loaded>();
            let generation = loaded.generation;
            let Some(sim) = loaded
                .scene
                .as_mut()
                .and_then(|s| s.mission.simulation.as_mut())
            else {
                return;
            };
            (
                generation,
                sim.elapsed.as_millis() as i64,
                std::mem::take(&mut sim.bolts),
                sim.gameplay.aux_rng,
            )
        };
        if bolts.generation != generation {
            for l in std::mem::take(&mut bolts.live) {
                if let Ok(e) = world.get_entity_mut(l.entity) {
                    e.despawn();
                }
            }
            bolts.generation = generation;
        }
        for s in new {
            let (b, shape, (nx, ny)) = (s.bolt, s.shape, s.normal);
            let entity = world
                .spawn((
                    Sprite::default(),
                    Transform::default(),
                    RenderLayers::layer(1),
                ))
                .id();
            bolts.live.push(Live {
                bolt: b,
                shape,
                normal: (nx, ny),
                born_ms: now,
                ticks: -1,
                entity,
            });
        }
        let showing = bolts.live.len();
        let mut k = 0;
        while k < bolts.live.len() {
            let age = now - bolts.live[k].born_ms;
            let ms = bolts.live[k].bolt.ms as i64;
            // 0x46d510: a tick every 100 ms; gone after its time.
            if age > ms + 100 {
                let l = bolts.live.remove(k);
                if let Ok(e) = world.get_entity_mut(l.entity) {
                    e.despawn();
                }
                continue;
            }
            let ticks = (age / 100).min(ms / 100);
            if ticks != bolts.live[k].ticks {
                if bolts.live[k].ticks >= 0 {
                    let (nx, ny) = bolts.live[k].normal;
                    let b = bolts.live[k].bolt.clone();
                    let mut shape = std::mem::take(&mut bolts.live[k].shape);
                    // 0x46d510: each new wander from the stream of sounds
                    // and pictures.
                    bolt::wander(&b, &mut shape, nx, ny, |lo, hi| rng.range(lo, hi));
                    bolts.live[k].shape = shape;
                }
                bolts.live[k].ticks = ticks;
                place(world, &bolts.live[k], showing);
            }
            // The bolt's flash for the light map of the «GRAFICA» page.
            let e = bolts.live[k].entity;
            if let (Some(t), Some(size)) = (
                world.get::<Transform>(e).map(|t| t.translation.truncate()),
                world.get::<Sprite>(e).and_then(|s| s.custom_size),
            ) && let Some(mut sources) = world.get_resource_mut::<super::lights::LightSources>()
            {
                sources.0.push(super::lights::Light {
                    at: t,
                    radius: size.max_element() * 0.6 + 32.,
                    color: [0.85, 0.9, 1.0],
                });
            }
            k += 1;
        }
        if let Some(sim) = world
            .resource_mut::<Loaded>()
            .scene
            .as_mut()
            .and_then(|s| s.mission.simulation.as_mut())
        {
            sim.gameplay.aux_rng = rng;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lines_cover_each_step_and_colours_halve_like_the_native_mask() {
        let mut seen = Vec::new();
        line([0, 0], [4, 2], |x, y| seen.push([x, y]));
        // 0x46c6f7: the short axis moves only once the accumulator passes
        // 0x10000, so a half slope rises on the second step.
        assert_eq!(seen, [[0, 0], [1, 0], [2, 1], [3, 1], [4, 2]]);
        seen.clear();
        line([4, 2], [0, 0], |x, y| seen.push([x, y]));
        assert_eq!(seen, [[4, 2], [3, 2], [2, 1], [1, 1], [0, 0]]);
        seen.clear();
        line([0, 0], [1, 3], |x, y| seen.push([x, y]));
        assert_eq!(seen, [[0, 0], [0, 1], [1, 2], [1, 3]]);
        // 0x46dd8b: white halves to (15, 15, 15), blue to (0, 0, 15).
        assert_eq!(half(0x7fff), 0x3def);
        assert_eq!(half(0x001f), 0x000f);
    }
}
