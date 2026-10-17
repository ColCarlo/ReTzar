//! «Effetto monitor CRT» of the «GRAFICA» page, an option of this version: a
//! last pass of the interface's camera, which sees the map and the interface
//! together, rounds the corners and lays scan lines and a grille over them.
use bevy::{
    core_pipeline::{Core2dSystems, FullscreenShader, schedule::Core2d},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        view::ViewTarget,
    },
};

/// On the interface's camera while the effect is chosen: x screen pixels
/// per interface pixel, yz the screen in pixels.
#[derive(Component, Clone, Copy, ExtractComponent)]
#[extract_app(RenderApp)]
pub struct Crt(pub Vec4);

pub struct CrtPlugin;
impl Plugin for CrtPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ExtractComponentPlugin::<Crt>::default());
        if app.get_sub_app(RenderApp).is_none() {
            return;
        }
        let shader = app
            .world_mut()
            .resource_mut::<Assets<Shader>>()
            .add(Shader::from_wesl(
                include_str!("crt.wesl"),
                "retzar/crt.wesl",
            ));
        app.sub_app_mut(RenderApp)
            .insert_resource(CrtShader(shader))
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Core2d, draw.in_set(Core2dSystems::PostProcess));
    }
}

#[derive(Resource)]
struct CrtShader(Handle<Shader>);
#[derive(Resource)]
struct CrtPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    id: CachedRenderPipelineId,
}
fn init_pipeline(
    mut commands: Commands,
    shader: Res<CrtShader>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "retzar_crt",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<Vec4>(false),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("retzar_crt".into()),
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
    commands.insert_resource(CrtPipeline {
        layout,
        sampler,
        id,
    });
}
fn draw(
    view: ViewQuery<(&ViewTarget, &Crt)>,
    pipeline: Res<CrtPipeline>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
    queue: Res<RenderQueue>,
) {
    let (target, crt) = view.into_inner();
    let Some(pipeline_gpu) = cache.get_render_pipeline(pipeline.id) else {
        return;
    };
    let output = target.post_process_write();
    let mut uniform = UniformBuffer::from(crt.0);
    uniform.write_buffer(ctx.render_device(), &queue);
    let bindings = ctx.render_device().create_bind_group(
        "retzar_crt",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            output.source,
            &pipeline.sampler,
            uniform.binding().unwrap(),
        )),
    );
    let mut pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("retzar_crt"),
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

/// Puts the effect on the interface's camera while it is chosen.
pub fn sync(
    mut commands: Commands,
    menu: Res<super::MenuState>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<(Entity, Option<&Crt>), With<super::UiCamera>>,
) {
    for (entity, crt) in &cameras {
        if menu.current_settings().crt {
            let surface = super::view::Surface::of(&window);
            let factor = super::view::hud_layout(surface, menu.current_settings()).factor;
            let wanted = Vec4::new(factor, surface.physical.x, surface.physical.y, 0.);
            if crt.map(|c| c.0) != Some(wanted) {
                commands.entity(entity).insert(Crt(wanted));
            }
        } else if crt.is_some() {
            commands.entity(entity).remove::<Crt>();
        }
    }
}
