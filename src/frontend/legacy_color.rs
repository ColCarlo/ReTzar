//! Tzar composites map pixels in gamma-encoded RGB555. The interface keeps
//! its own camera; only the map blends in sRGB and restores five-bit channels.
use bevy::{
    core_pipeline::{Core2dSystems, FullscreenShader, schedule::Core2d},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        texture::{FallbackImageZero, GpuImage},
        view::ViewTarget,
    },
};

#[derive(Component, Clone, ExtractComponent)]
#[extract_app(RenderApp)]
pub struct LegacyColor {
    /// rgb: the clock's tint; a: 1 keeps the original's RGB555 steps, 0 is
    /// the true colour of the «GRAFICA» page.
    pub light: Vec4,
    /// The light map of the «GRAFICA» page (`super::lights`): x how much the
    /// lights give back of what the night takes, y the halo, zw a screen
    /// position's scale into the map.
    pub glow: Vec4,
    pub lights: Option<Handle<Image>>,
    /// A screen position to map pixels (x scale, yz offsets) and the time
    /// (w), set by `super::soft_fog` while the fog or the water need them.
    pub world: Vec4,
    /// x the map's side; y «Nebbia morbida», z «Ambiente vivo», 1 when on.
    pub map: Vec4,
    /// The fog's vertices, explored and in view (`super::soft_fog`).
    pub fog: Option<Handle<Image>>,
    /// The cells of water (`super::living`).
    pub water: Option<Handle<Image>>,
}
impl Default for LegacyColor {
    fn default() -> Self {
        Self {
            light: Vec4::ONE,
            glow: Vec4::ZERO,
            lights: None,
            world: Vec4::ZERO,
            map: Vec4::ZERO,
            fog: None,
            water: None,
        }
    }
}
#[derive(ShaderType, Clone, Copy)]
struct Tint {
    lighting: Vec4,
    glow: Vec4,
    world: Vec4,
    map: Vec4,
}

pub struct LegacyColorPlugin;
impl Plugin for LegacyColorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ExtractComponentPlugin::<LegacyColor>::default());
        if app.get_sub_app(RenderApp).is_none() {
            return;
        }
        let shader = app
            .world_mut()
            .resource_mut::<Assets<Shader>>()
            .add(Shader::from_wesl(
                include_str!("legacy_color.wesl"),
                "retzar/legacy_color.wesl",
            ));
        app.sub_app_mut(RenderApp)
            .insert_resource(LegacyShader(shader))
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Core2d, quantize.in_set(Core2dSystems::PostProcess));
    }
}

#[derive(Resource)]
struct LegacyShader(Handle<Shader>);
#[derive(Resource)]
struct LegacyPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    id: CachedRenderPipelineId,
}
fn init_pipeline(
    mut commands: Commands,
    shader: Res<LegacyShader>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "retzar_rgb555",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                uniform_buffer::<Tint>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("retzar_rgb555".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: shader.0.clone(),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(LegacyPipeline {
        layout,
        sampler,
        id,
    });
}
fn quantize(
    view: ViewQuery<(&ViewTarget, &LegacyColor)>,
    pipeline: Res<LegacyPipeline>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    zero: Res<FallbackImageZero>,
    mut ctx: RenderContext,
    queue: Res<RenderQueue>,
) {
    let (target, color) = view.into_inner();
    let Some(pipeline_gpu) = cache.get_render_pipeline(pipeline.id) else {
        return;
    };
    // Without its light map the lights are off: black, and nothing read.
    let lights = color.lights.as_ref().and_then(|h| images.get(h));
    let glow = if lights.is_some() {
        color.glow
    } else {
        Vec4::ZERO
    };
    let lights = lights.map_or(&zero.texture_view, |i| &i.texture_view);
    // A missing texture switches its effect off.
    let fog = color.fog.as_ref().and_then(|h| images.get(h));
    let water = color.water.as_ref().and_then(|h| images.get(h));
    let mut map = color.map;
    if fog.is_none() {
        map.y = 0.;
    }
    if water.is_none() {
        map.z = 0.;
    }
    let fog = fog.map_or(&zero.texture_view, |i| &i.texture_view);
    let water = water.map_or(&zero.texture_view, |i| &i.texture_view);
    let output = target.post_process_write();
    let mut uniform = UniformBuffer::from(Tint {
        lighting: color.light,
        glow,
        world: color.world,
        map,
    });
    uniform.write_buffer(ctx.render_device(), &queue);
    let bindings = ctx.render_device().create_bind_group(
        "retzar_rgb555",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            output.source,
            uniform.binding().unwrap(),
            lights,
            &pipeline.sampler,
            fog,
            water,
        )),
    );
    let mut pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("retzar_rgb555"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: output.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(pipeline_gpu);
    pass.set_bind_group(0, &bindings, &[]);
    pass.draw(0..3, 0..1);
}
