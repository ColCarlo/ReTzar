//! Placement grid, an addition of this version that the player can turn off
//! (Opzioni → ALTRO, with three strengths): while a building is being placed
//! the cell edges show over the ground, under every object and the fog, and
//! the cells the building cannot cover are striped red
//! (`gameplay::refused_cells`).
use bevy::{
    asset::{RenderAssetUsages, load_internal_asset, uuid_handle},
    prelude::*,
    render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin},
};

const SHADER: Handle<Shader> = uuid_handle!("3f9d27c1-5b84-4e6a-a0d2-8c71e4b95f03");

/// Above the terrain and the ground pictures (1 and 2), below the shadows
/// of flyers (3) and every object (from 10).
pub const DEPTH: f32 = 2.5;

/// Each strength of the settings: the line's alpha, its extra near the
/// cursor, the alpha of a cell the building cannot cover.
pub const TONES: [[f32; 3]; 3] = [[0.14, 0.14, 0.16], [0.24, 0.16, 0.24], [0.36, 0.18, 0.34]];

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct GridMaterial {
    /// Map point under the cursor (x, y), 1 when there is one (z), line
    /// width in screen pixels (w).
    #[uniform(0)]
    pub look: Vec4,
    /// One of [`TONES`] (w unused).
    #[uniform(1)]
    pub tone: Vec4,
    /// One texel per cell, row by row: 255 where the building cannot stand.
    #[texture(2)]
    pub marks: Handle<Image>,
}
impl Material2d for GridMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

pub struct GridPlugin;
impl Plugin for GridPlugin {
    fn build(&self, app: &mut App) {
        // Without a renderer (headless runs) the materials are only stored.
        if !app.world().contains_resource::<Assets<Shader>>() {
            app.init_resource::<Assets<GridMaterial>>();
            return;
        }
        load_internal_asset!(app, SHADER, "grid.wesl", Shader::from_wesl);
        app.add_plugins(Material2dPlugin::<GridMaterial>::default());
    }
}

/// The texture of the refused cells of a map: one byte per cell, all clear;
/// kept on the CPU too, since it changes while the game runs.
pub fn marks_image(side: usize) -> Image {
    Image::new(
        Extent3d {
            width: side as u32,
            height: side as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0; side * side],
        TextureFormat::R8Unorm,
        RenderAssetUsages::default(),
    )
}

/// Line width in screen pixels: one map pixel, never under one.
pub fn line_width(magnification: f32) -> f32 {
    magnification.round().max(1.)
}
