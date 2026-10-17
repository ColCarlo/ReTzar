//! Mode 3 of the original painter (0x458740): a picture brightens what lies
//! under it channel by channel, `dst + (31 − dst) × src × strength / 3100` on
//! five-bit channels. On the GPU this is `src' × (1 − dst) + dst` with
//! `src' = src × strength / 100` stored in the texture.
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    mesh::MeshVertexBufferLayoutRef,
    prelude::*,
    render::render_resource::{
        AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState,
        RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPipeline, Material2dPlugin},
};
use retzar::bitmap::Bitmap;

const SHADER: Handle<Shader> = uuid_handle!("6b0f3c52-9a47-4d1e-8f3b-2c5a7e91d4b6");

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct GlowMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
}
impl Material2d for GlowMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
    fn specialize(
        _pipeline: &Material2dPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::OneMinusDst,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent {
                        src_factor: BlendFactor::Zero,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                });
            }
        }
        Ok(())
    }
}

pub struct GlowPlugin;
impl Plugin for GlowPlugin {
    fn build(&self, app: &mut App) {
        // Without a renderer (headless runs) the materials are only stored.
        if !app.world().contains_resource::<Assets<Shader>>() {
            app.init_resource::<Assets<GlowMaterial>>();
            return;
        }
        load_internal_asset!(app, SHADER, "glow.wesl", Shader::from_wesl);
        app.add_plugins(Material2dPlugin::<GlowMaterial>::default());
    }
}

/// The texture of a mode 3 picture: each channel times `strength` / 100,
/// black where the picture is transparent.
pub fn scale(b: &mut Bitmap, strength: u8) {
    for p in b.rgba.chunks_exact_mut(4) {
        if p[3] == 0 {
            p.copy_from_slice(&[0, 0, 0, 255]);
            continue;
        }
        for c in &mut p[..3] {
            *c = (*c as u32 * strength as u32 / 100).min(255) as u8;
        }
        p[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mode_three_follows_the_native_table() {
        // 0x458740: T[src][dst] = dst + (31 − dst) × src × strength / 3100,
        // the GPU's `src' × (1 − dst) + dst` on five-bit channels.
        for strength in [50u32, 100] {
            for src in 0..32u32 {
                for dst in 0..32u32 {
                    let native =
                        dst as f64 + (31 - dst) as f64 * src as f64 * strength as f64 / 3100.;
                    let s = src as f64 / 31. * strength as f64 / 100.;
                    let gpu = (s * (1. - dst as f64 / 31.) + dst as f64 / 31.) * 31.;
                    assert!((native - gpu).abs() < 1e-9);
                }
            }
        }
        let mut b = Bitmap {
            width: 2,
            height: 1,
            rgba: vec![200, 100, 0, 255, 9, 9, 9, 0],
        };
        scale(&mut b, 50);
        assert_eq!(b.rgba, vec![100, 50, 0, 255, 0, 0, 0, 255]);
    }
}
