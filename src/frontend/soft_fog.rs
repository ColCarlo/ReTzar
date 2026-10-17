//! «Nebbia morbida» of the «GRAFICA» page, an option of this version: the
//! fog's vertices (0x5b7828, explored and in view) go to a texture that the
//! map's last pass reads with smooth filtering, instead of the original's
//! dithered tiles (0x4f0960), which `environment` then leaves hidden.
use super::{Loaded, MapCamera, MenuState, legacy_color::LegacyColor, view::Surface};
use bevy::{
    asset::RenderAssetUsages,
    camera::RenderTarget,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::PrimaryWindow,
};

#[derive(Resource)]
pub struct SoftFog {
    image: Handle<Image>,
    /// The fog revision and side the texture shows.
    shown: Option<(u64, usize)>,
}

pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = images.add(vertices(1, |_, _| (false, false)));
    commands.insert_resource(SoftFog { image, shown: None });
}

/// `n × n` vertices: red explored, green in view.
fn vertices(n: usize, at: impl Fn(usize, usize) -> (bool, bool)) -> Image {
    let mut data = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let (seen, view) = at(x, y);
            data.extend([seen as u8 * 255, view as u8 * 255, 0, 255]);
        }
    }
    Image::new(
        Extent3d {
            width: n as u32,
            height: n as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    )
}

/// Refreshes the vertices when the fog changes and tells the map's last
/// pass where they lie under the screen.
#[allow(clippy::type_complexity)]
pub fn update(
    menu: Res<MenuState>,
    loaded: Res<Loaded>,
    time: Res<Time>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut fog: ResMut<SoftFog>,
    mut images: ResMut<Assets<Image>>,
    mut map: Query<
        (
            &Camera,
            &RenderTarget,
            &Projection,
            &Transform,
            &mut LegacyColor,
        ),
        With<MapCamera>,
    >,
) {
    let Ok((camera, target, projection, transform, mut color)) = map.single_mut() else {
        return;
    };
    let sim = loaded
        .scene
        .as_ref()
        .and_then(|s| s.mission.simulation.as_ref());
    let settings = menu.current_settings();
    let (true, Some(sim)) = (
        (settings.soft_fog || settings.living_world) && camera.is_active,
        sim,
    ) else {
        if color.fog.is_some() || color.map != Vec4::ZERO {
            color.fog = None;
            color.map = Vec4::ZERO;
        }
        fog.shown = None;
        return;
    };
    let side = sim.fog.side;
    if settings.soft_fog {
        if fog.shown != Some((sim.fog.revision, side)) {
            let fresh = vertices(side + 1, |x, y| sim.fog.vertex(x, y));
            if let Some(mut image) = images.get_mut(&fog.image) {
                *image = fresh;
            }
            fog.shown = Some((sim.fog.revision, side));
        }
        color.fog = Some(fog.image.clone());
    } else {
        color.fog = None;
        fog.shown = None;
    }
    // A pixel of the map camera's picture to map pixels: world units per
    // pixel (the window's are logical, the filter's image texels), y down.
    let surface = Surface::of(&window);
    let scale = match projection {
        Projection::Orthographic(o) => o.scale,
        _ => 1.,
    };
    let (size, per_pixel) = match (target, &camera.viewport) {
        (RenderTarget::Image(_), Some(v)) => (v.physical_size.as_vec2(), scale),
        _ => (surface.physical, scale / surface.dpi),
    };
    let at = transform.translation;
    color.world = Vec4::new(
        per_pixel,
        at.x - size.x / 2. * per_pixel,
        -at.y - size.y / 2. * per_pixel,
        (time.elapsed_secs_wrapped() % 3600.) as f32,
    );
    color.map = Vec4::new(
        side as f32,
        if settings.soft_fog { 1. } else { 0. },
        if settings.living_world { 1. } else { 0. },
        0.,
    );
}
