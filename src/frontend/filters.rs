//! «Pixel della mappa» of the «GRAFICA» page, an option of this version: the
//! map camera draws into an image at one texel per map pixel, and a window
//! camera with nothing of its own brings it to the screen in a last pass,
//! with even pixels (sharp bilinear) or smoothed art (xBR). Off, the map
//! camera draws to the window as before.
use super::{Loaded, MapCamera, MenuState, view::MapView, view::Surface};
use bevy::{
    camera::{
        CompositingSpace, ImageRenderTarget, RenderTarget, Viewport, visibility::RenderLayers,
    },
    core_pipeline::{Core2dSystems, FullscreenShader, schedule::Core2d},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderQueue, ViewQuery},
        texture::GpuImage,
        view::ViewTarget,
    },
    window::{PrimaryWindow, WindowRef},
};

/// A layer no entity is on: the filter camera only runs its last pass.
const LAYER: usize = 5;

#[derive(ShaderType, Clone, Copy, Default, PartialEq)]
pub struct Params {
    /// xy: texel coordinate of the screen's top left corner; z: texels per
    /// screen pixel; w: the filter (1 even, 2 smoothed).
    a: Vec4,
    /// xy: texels rendered.
    b: Vec4,
}
/// On the filter camera: the map's image and how to read it.
#[derive(Component, Clone, ExtractComponent)]
#[extract_app(RenderApp)]
pub struct MapFilter {
    params: Params,
    image: Handle<Image>,
}
#[derive(Component)]
pub struct FilterCamera;

pub struct FiltersPlugin;
impl Plugin for FiltersPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ExtractComponentPlugin::<MapFilter>::default());
        if app.get_sub_app(RenderApp).is_none() {
            return;
        }
        let shader = app
            .world_mut()
            .resource_mut::<Assets<Shader>>()
            .add(Shader::from_wesl(
                include_str!("filters.wesl"),
                "retzar/filters.wesl",
            ));
        app.sub_app_mut(RenderApp)
            .insert_resource(FilterShader(shader))
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Core2d, draw.in_set(Core2dSystems::PostProcess));
    }
}

pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = images.add(Image::new_target_texture(
        64,
        64,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    commands.spawn((
        Camera2d,
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        Camera {
            // After the map's image, before the interface.
            order: 0,
            is_active: false,
            ..default()
        },
        FilterCamera,
        MapFilter {
            params: Params::default(),
            image,
        },
        Msaa::Off,
        CompositingSpace::Srgb,
        RenderLayers::layer(LAYER),
    ));
}

/// Moves the map camera onto the image while a filter is chosen, and back to
/// the window when none is.
#[allow(clippy::type_complexity)]
pub fn update(
    menu: Res<MenuState>,
    view: Res<MapView>,
    loaded: Res<Loaded>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut map: Query<
        (
            &mut Camera,
            &mut RenderTarget,
            &mut Projection,
            &mut Transform,
        ),
        (With<MapCamera>, Without<FilterCamera>),
    >,
    mut filter: Query<(&mut Camera, &mut MapFilter), (With<FilterCamera>, Without<MapCamera>)>,
) {
    let Ok((mut camera, mut render_target, mut projection, mut transform)) = map.single_mut()
    else {
        return;
    };
    let Ok((mut present, mut map_filter)) = filter.single_mut() else {
        return;
    };
    let kind = menu.current_settings().pixel_filter;
    let on = kind > 0 && loaded.scene.is_some() && camera.is_active;
    if present.is_active != on {
        present.is_active = on;
    }
    if !on {
        if !matches!(*render_target, RenderTarget::Window(_)) {
            *render_target = RenderTarget::Window(WindowRef::Primary);
            camera.viewport = None;
        }
        return;
    }
    let surface = Surface::of(&window);
    // Screen pixels per map pixel, and the map's point at the screen's corner.
    let m = surface.dpi / view.scale;
    let p = surface.physical;
    let left = view.center.x - p.x / 2. / m;
    let top = view.center.y + p.y / 2. / m;
    // Whole texels, one more on every side for the blending.
    let l = left.floor() - 1.;
    let t = top.ceil() + 1.;
    let w = (p.x / m).ceil() as u32 + 3;
    let h = (p.y / m).ceil() as u32 + 3;
    let image = map_filter.image.clone();
    if let Some(i) = images.get(&image)
        && (i.width() < w || i.height() < h)
    {
        let size = Extent3d {
            width: i.width().max(w),
            height: i.height().max(h),
            depth_or_array_layers: 1,
        };
        if let Some(mut i) = images.get_mut(&image) {
            i.resize(size);
        }
    }
    if !matches!(&*render_target, RenderTarget::Image(i) if i.handle == image) {
        *render_target = RenderTarget::Image(ImageRenderTarget {
            handle: image,
            scale_factor: 1.,
        });
    }
    camera.viewport = Some(Viewport {
        physical_position: UVec2::ZERO,
        physical_size: UVec2::new(w, h),
        ..default()
    });
    if let Projection::Orthographic(o) = &mut *projection {
        o.scale = 1.;
    }
    transform.translation.x = l + w as f32 / 2.;
    transform.translation.y = t - h as f32 / 2.;
    let params = Params {
        a: Vec4::new(left - l, t - top, 1. / m, kind as f32),
        b: Vec4::new(w as f32, h as f32, 0., 0.),
    };
    if map_filter.params != params {
        map_filter.params = params;
    }
}

#[derive(Resource)]
struct FilterShader(Handle<Shader>);
#[derive(Resource)]
struct FilterPipeline {
    layout: BindGroupLayoutDescriptor,
    id: CachedRenderPipelineId,
}
fn init_pipeline(
    mut commands: Commands,
    shader: Res<FilterShader>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "retzar_filters",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<Params>(false),
                texture_2d(TextureSampleType::Float { filterable: false }),
            ),
        ),
    );
    let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("retzar_filters".into()),
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
    commands.insert_resource(FilterPipeline { layout, id });
}
fn draw(
    view: ViewQuery<(&ViewTarget, &MapFilter)>,
    pipeline: Res<FilterPipeline>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    mut ctx: RenderContext,
    queue: Res<RenderQueue>,
) {
    let (target, filter) = view.into_inner();
    let (Some(pipeline_gpu), Some(image)) = (
        cache.get_render_pipeline(pipeline.id),
        images.get(&filter.image),
    ) else {
        return;
    };
    let output = target.post_process_write();
    let mut uniform = UniformBuffer::from(filter.params);
    uniform.write_buffer(ctx.render_device(), &queue);
    let bindings = ctx.render_device().create_bind_group(
        "retzar_filters",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((uniform.binding().unwrap(), &image.texture_view)),
    );
    let mut pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("retzar_filters"),
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
