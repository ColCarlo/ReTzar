//! «Ambiente vivo» of the «GRAFICA» page, an addition of this version: the
//! trees sway in the wind and the water glints. A tree swings through five
//! pictures of its own whose upper rows are moved by whole pixels (−2 to 2
//! at the top, none at the foot), so the art keeps its pixels; a wave runs
//! across the woods, wider in rain or snow. The water's sparkles are drawn
//! by the map's last pass over a mask of the cells of water.
use super::{Game, MapCamera, MenuState, gpu_image, legacy_color::LegacyColor, scene::Loaded};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use retzar::{bitmap::Bitmap, world::Layer};
use std::collections::HashMap;

#[derive(Resource, Default)]
pub struct Living {
    /// The scene the mask and the pictures were made for.
    generation: Option<u32>,
    water: Option<Handle<Image>>,
    /// The five pictures of each tree image, −2 to 2 pixels at the top.
    sway: HashMap<usize, [Handle<Image>; 5]>,
    /// The picture of each swaying tree, to put back.
    base: HashMap<Entity, Handle<Image>>,
}

/// A tree's picture bent by `top` pixels at its top row, down to none at
/// its foot.
pub fn bend(b: &Bitmap, top: i32) -> Bitmap {
    let (w, h) = (b.width as usize, b.height as usize);
    let mut out = Bitmap {
        width: b.width,
        height: b.height,
        rgba: vec![0; b.rgba.len()],
    };
    for y in 0..h {
        let rise = 1. - y as f32 / (h.max(2) - 1) as f32;
        let shift = (top as f32 * rise.powf(1.5)).round() as isize;
        for x in 0..w {
            let from = x as isize - shift;
            if (0..w as isize).contains(&from) {
                let (d, s) = ((y * w + x) * 4, (y * w + from as usize) * 4);
                out.rgba[d..d + 4].copy_from_slice(&b.rgba[s..s + 4]);
            }
        }
    }
    out
}

/// The cells of water, one texel each: open water (types 0 under both
/// layers of the cell, as `gameplay::water_cell`) full, the animated coast
/// at a little more than half.
fn water_mask(grid: &retzar::map::MapGrid, coast: impl Iterator<Item = [usize; 2]>) -> Image {
    let side = grid.side;
    let mut data = vec![0u8; side * side * 4];
    for [x, y] in coast {
        if x < side && y < side {
            data[(y * side + x) * 4] = 160;
        }
    }
    for (k, &v) in grid.cells.iter().enumerate().take(side * side) {
        if (v >> 4) & 7 == 0 && (v >> 7) & 7 == 0 {
            data[k * 4] = 255;
        }
    }
    Image::new(
        Extent3d {
            width: side.max(1) as u32,
            height: side.max(1) as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    )
}

pub fn update(world: &mut World) {
    if !world.contains_resource::<Living>() {
        world.init_resource::<Living>();
    }
    let on = world
        .resource::<MenuState>()
        .current_settings()
        .living_world;
    let t = world.resource::<Time>().elapsed_secs_wrapped();
    world.resource_scope(|world, mut living: Mut<Living>| {
        let generation = world.resource::<Loaded>().generation;
        let has_scene = world.resource::<Loaded>().scene.is_some();
        if !on || !has_scene {
            // Back to the trees' own pictures.
            for (e, base) in std::mem::take(&mut living.base) {
                if let Some(mut sprite) = world.get_mut::<Sprite>(e) {
                    sprite.image = base;
                }
            }
            if !has_scene {
                *living = Living::default();
            }
            let mut cameras = world.query_filtered::<&mut LegacyColor, With<MapCamera>>();
            for mut color in cameras.iter_mut(world) {
                if color.water.is_some() {
                    color.water = None;
                }
            }
            return;
        }
        if living.generation != Some(generation) {
            *living = Living {
                generation: Some(generation),
                ..default()
            };
            let mask = {
                let loaded = world.resource::<Loaded>();
                let s = loaded.scene.as_ref().unwrap();
                water_mask(
                    &s.grid,
                    s.water_meshes
                        .iter()
                        .flat_map(|m| m.tiles.iter().map(|t| t.cell)),
                )
            };
            living.water = Some(world.resource_mut::<Assets<Image>>().add(mask));
        }
        let water = living.water.clone();
        let mut cameras = world.query_filtered::<&mut LegacyColor, With<MapCamera>>();
        for mut color in cameras.iter_mut(world) {
            if color.water != water {
                color.water = water.clone();
            }
        }
        // The trees: which picture each shows now.
        let mut wanted: Vec<(Entity, usize, usize)> = Vec::new();
        {
            let loaded = world.resource::<Loaded>();
            let s = loaded.scene.as_ref().unwrap();
            let Some(sim) = s.mission.simulation.as_ref() else {
                return;
            };
            // A breeze, a wind in rain or snow.
            let amp = if sim.precipitation > 0. { 2. } else { 1. };
            for (v, &e) in s.visuals.iter().zip(&s.visual_entities) {
                if !v.tree || v.layer != Layer::Body || v.object != usize::MAX {
                    continue;
                }
                let x = (v.footprint[0] / 32.).floor().max(0.) as usize;
                let y = (-(v.footprint[1] + v.footprint[3]) / 32.).floor().max(0.) as usize;
                let c = y * s.side + x;
                if c >= s.grid.cells.len()
                    || sim.gameplay.depleted_trees.contains(&c)
                    || sim.gameplay.stumps.contains(&c)
                {
                    continue;
                }
                let value = (s.grid.cells[c] >> 11) as usize;
                let wave = (t * 1.7 + x as f32 * 0.35 + y as f32 * 0.22).sin();
                let k = (amp * wave).round().clamp(-2., 2.) as i32;
                wanted.push((e, value, (k + 2) as usize));
            }
        }
        // A felled tree keeps the trunk its scene gives it.
        let standing: std::collections::HashSet<Entity> =
            wanted.iter().map(|&(e, _, _)| e).collect();
        living.base.retain(|e, _| standing.contains(e));
        for (e, value, k) in wanted {
            if !living.sway.contains_key(&value) {
                let pictures = {
                    let game = world.resource::<Game>();
                    let biome = world
                        .resource::<Loaded>()
                        .scene
                        .as_ref()
                        .unwrap()
                        .biome
                        .clone();
                    retzar::world::tree_pictures(&game.assets, &biome, value).ok()
                };
                let Some(([body, _], _)) = pictures else {
                    continue;
                };
                let mut images = world.resource_mut::<Assets<Image>>();
                let sway = [-2, -1, 0, 1, 2].map(|top| images.add(gpu_image(bend(&body, top))));
                living.sway.insert(value, sway);
            }
            let Some(mut sprite) = world.get_mut::<Sprite>(e) else {
                continue;
            };
            let base = living
                .base
                .entry(e)
                .or_insert_with(|| sprite.image.clone())
                .clone();
            let image = if k == 2 {
                base
            } else {
                living.sway[&value][k].clone()
            };
            if sprite.image != image {
                sprite.image = image;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_bent_tree_keeps_its_foot_and_moves_its_top_by_whole_pixels() {
        // A 3×3 picture whose middle column is opaque.
        let mut b = Bitmap {
            width: 3,
            height: 3,
            rgba: vec![0; 36],
        };
        for y in 0..3 {
            b.rgba[(y * 3 + 1) * 4..(y * 3 + 2) * 4].copy_from_slice(&[9, 9, 9, 255]);
        }
        let bent = bend(&b, 1);
        let opaque = |y: usize| (0..3).find(|&x| bent.rgba[(y * 3 + x) * 4 + 3] == 255);
        assert_eq!(opaque(0), Some(2), "the top moves a whole pixel");
        assert_eq!(opaque(2), Some(1), "the foot stays");
        assert_eq!(bend(&b, 0).rgba, b.rgba);
    }
}
