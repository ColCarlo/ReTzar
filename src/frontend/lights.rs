//! «Luci notturne» and «Bagliore di fuochi e magie» of the «GRAFICA» page,
//! additions of this version. A camera before the map's draws soft halos at
//! half its resolution into a light map: fires, explosions and spells (the
//! effects and the lightning add them), and with the night lights the
//! buildings in view as the night falls. The map's last pass reads it: the
//! lights give back what the night takes, and the halo brightens round them.
use super::{Loaded, MapCamera, MenuState, legacy_color::LegacyColor, view::Surface};
use bevy::{
    asset::RenderAssetUsages,
    camera::{ImageRenderTarget, RenderTarget, Viewport, visibility::RenderLayers},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::PrimaryWindow,
};

/// The layer of the halos, drawn only by the light camera.
const LAYER: usize = 6;
/// Fire, flames and explosions.
pub const WARM: [f32; 3] = [1.0, 0.6, 0.28];
/// Ice.
pub const COLD: [f32; 3] = [0.45, 0.7, 1.0];
/// Spells and other mode 3 pictures.
pub const MAGIC: [f32; 3] = [0.75, 0.7, 1.0];
/// The windows and torches of a building at night.
const LAMP: [f32; 3] = [1.0, 0.68, 0.36];

/// A source of light on the map, in world units.
#[derive(Clone, Copy)]
pub struct Light {
    pub at: Vec2,
    pub radius: f32,
    pub color: [f32; 3],
}
/// The lights of this frame, laid down by the effects and the lightning.
#[derive(Resource, Default)]
pub struct LightSources(pub Vec<Light>);

/// The colour of a picture's light, if it gives any: by its name, or mode
/// 3 (`glow`) for the spells.
pub fn emissive(name: &str, glow: bool) -> Option<[f32; 3]> {
    if name.contains("freeze") {
        Some(COLD)
    } else if ["fire", "flame", "expl", "explosion", "rocket"]
        .iter()
        .any(|k| name.contains(k))
    {
        Some(WARM)
    } else if glow
        || ["magic", "spell", "exorcism", "healing"]
            .iter()
            .any(|k| name.contains(k))
    {
        Some(MAGIC)
    } else {
        None
    }
}

#[derive(Component)]
pub struct LightCamera;
#[derive(Component)]
pub struct Halo;
#[derive(Resource)]
pub struct LightMap {
    image: Handle<Image>,
    halo: Handle<Image>,
    sprites: Vec<Entity>,
}

/// A white disc whose cover falls off smoothly to its rim.
fn halo_image() -> Image {
    const SIDE: u32 = 128;
    let mut data = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / SIDE as f32 * 2. - Vec2::ONE;
            let t = (1. - d.length()).clamp(0., 1.);
            data.extend([255, 255, 255, (t * t * (3. - 2. * t) * 255.) as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = images.add(Image::new_target_texture(
        64,
        64,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    let halo = images.add(halo_image());
    commands.spawn((
        Camera2d,
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        Camera {
            // Before the map, whose last pass reads the light map.
            order: -2,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        RenderTarget::Image(ImageRenderTarget {
            handle: image.clone(),
            scale_factor: 1.,
        }),
        LightCamera,
        Msaa::Off,
        RenderLayers::layer(LAYER),
    ));
    commands.insert_resource(LightMap {
        image,
        halo,
        sprites: Vec::new(),
    });
    commands.init_resource::<LightSources>();
}

/// Builds the light map of this frame and tells the map's last pass how to
/// read it; with both options off the light camera rests.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    menu: Res<MenuState>,
    loaded: Res<Loaded>,
    time: Res<Time>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut sources: ResMut<LightSources>,
    mut map_lights: ResMut<LightMap>,
    mut images: ResMut<Assets<Image>>,
    mut map: Query<
        (
            &Camera,
            &RenderTarget,
            &Projection,
            &Transform,
            &mut LegacyColor,
        ),
        (With<MapCamera>, Without<LightCamera>),
    >,
    mut light: Query<
        (&mut Camera, &mut Projection, &mut Transform),
        (With<LightCamera>, Without<MapCamera>),
    >,
    mut halos: Query<(&mut Sprite, &mut Transform, &mut Visibility), (With<Halo>, Without<Camera>)>,
) {
    let mut lights = std::mem::take(&mut sources.0);
    // The draft while the settings are open: a preview.
    let settings = menu.current_settings();
    let Ok((map_camera, map_target, map_projection, map_transform, mut color)) = map.single_mut()
    else {
        return;
    };
    let Ok((mut camera, mut projection, mut transform)) = light.single_mut() else {
        return;
    };
    let scene = loaded.scene.as_ref();
    let on = (settings.night_lights || settings.bloom) && scene.is_some() && map_camera.is_active;
    if camera.is_active != on {
        camera.is_active = on;
    }
    if !on {
        if color.lights.is_some() {
            color.lights = None;
            color.glow = Vec4::ZERO;
        }
        for (_, _, mut v) in &mut halos {
            v.set_if_neq(Visibility::Hidden);
        }
        return;
    }
    // The map camera's picture: its size in pixels and the world units of
    // one pixel (the window's are logical, the filter's image texels).
    let surface = Surface::of(&window);
    let scale = match map_projection {
        Projection::Orthographic(o) => o.scale,
        _ => 1.,
    };
    let (size, per_pixel) = match (map_target, &map_camera.viewport) {
        (RenderTarget::Image(_), Some(v)) => (v.physical_size.as_vec2(), scale),
        _ => (surface.physical, scale / surface.dpi),
    };
    // Half its resolution, the same world.
    let wanted = (size / 2.).ceil().max(Vec2::ONE).as_uvec2();
    let image = map_lights.image.clone();
    if let Some(i) = images.get(&image)
        && (i.width() < wanted.x || i.height() < wanted.y)
    {
        let grown = Extent3d {
            width: i.width().max(wanted.x),
            height: i.height().max(wanted.y),
            depth_or_array_layers: 1,
        };
        if let Some(mut i) = images.get_mut(&image) {
            i.resize(grown);
        }
    }
    let full = images.get(&image).map_or(wanted.as_vec2(), |i| {
        Vec2::new(i.width() as f32, i.height() as f32)
    });
    camera.viewport = Some(Viewport {
        physical_position: UVec2::ZERO,
        physical_size: wanted,
        ..default()
    });
    if let Projection::Orthographic(o) = &mut *projection {
        o.scale = per_pixel * 2.;
    }
    transform.translation = map_transform.translation.with_z(transform.translation.z);
    // A screen pixel of the map's picture to the light map's coordinates.
    let to_map = wanted.as_vec2() / full / size;
    color.lights = Some(image);
    let night = scene
        .and_then(|s| s.mission.simulation.as_ref())
        .map_or(0., |sim| {
            if settings.day_night {
                sim.night_amount()
            } else {
                0.
            }
        });
    color.glow = Vec4::new(
        if settings.night_lights { 1. } else { 0. },
        if settings.bloom { 0.6 } else { 0. },
        to_map.x,
        to_map.y,
    );
    // The buildings in view light up as the night falls.
    if settings.night_lights
        && night > 0.
        && let Some(s) = scene
        && let Some(sim) = s.mission.simulation.as_ref()
    {
        for o in s.mission.objects.iter().filter(|o| {
            o.building()
                && o.health > 0
                && !matches!(
                    o.class,
                    retzar::world::class::BRIDGE
                        | retzar::world::class::FIELD
                        | retzar::world::class::WALL_STONE
                        | retzar::world::class::WALL_WOOD
                        | retzar::world::class::GATE_STONE
                        | retzar::world::class::GATE_WOOD
                )
                && sim.visible(o)
        }) {
            let [w, h] = s.library.cell_footprint(o);
            let centre = Vec2::new(
                (o.x as f32 + w as f32 / 2.) * 32.,
                -(o.y as f32 + h as f32 / 2.) * 32.,
            );
            let [r, g, b] = LAMP;
            let k = 0.8 * night;
            lights.push(Light {
                at: centre,
                radius: w.max(h) as f32 * 32. * 1.1 + 24.,
                color: [r * k, g * k, b * k],
            });
        }
    }
    // Fires breathe a little.
    let t = time.elapsed_secs();
    for (k, l) in lights.iter().enumerate() {
        let flicker = 0.9 + 0.1 * (t * 9. + k as f32 * 1.7).sin();
        let [r, g, b] = l.color;
        let tint = Color::linear_rgb(r * flicker, g * flicker, b * flicker);
        let size = Vec2::splat(l.radius * 2.);
        let place = l.at.extend(k as f32 * 1e-3);
        if let Some(&e) = map_lights.sprites.get(k) {
            if let Ok((mut sprite, mut tr, mut v)) = halos.get_mut(e) {
                sprite.color = tint;
                sprite.custom_size = Some(size);
                tr.translation = place;
                v.set_if_neq(Visibility::Inherited);
            }
        } else {
            let e = commands
                .spawn((
                    Sprite {
                        color: tint,
                        custom_size: Some(size),
                        ..Sprite::from_image(map_lights.halo.clone())
                    },
                    Transform::from_translation(place),
                    Halo,
                    RenderLayers::layer(LAYER),
                ))
                .id();
            map_lights.sprites.push(e);
        }
    }
    for &e in &map_lights.sprites[lights.len().min(map_lights.sprites.len())..] {
        if let Ok((_, _, mut v)) = halos.get_mut(e) {
            v.set_if_neq(Visibility::Hidden);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pictures_give_light_by_their_name_or_mode() {
        assert_eq!(emissive("fire_small", true), Some(WARM));
        assert_eq!(emissive("explosion_small", true), Some(WARM));
        assert_eq!(emissive("freeze_ball_trail", true), Some(COLD));
        assert_eq!(emissive("exorcism", true), Some(MAGIC));
        assert_eq!(emissive("magic_star", false), Some(MAGIC));
        assert_eq!(emissive("arrow", false), None);
        assert_eq!(emissive("cb_stone", false), None);
    }
}
