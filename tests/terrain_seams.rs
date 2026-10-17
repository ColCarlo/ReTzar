//! Exercise production terrain UVs at fractional zoom/camera coordinates.
#![cfg(feature = "game")]
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::{
        RenderPlugin,
        render_resource::*,
        settings::{Backends, RenderCreation, WgpuSettings},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    winit::{WinitPlugin, WinitSettings},
};
use std::sync::{Arc, Mutex};

#[test]
#[ignore = "Requires original archives, Vulkan and a hidden native window"]
fn coast_tiles_never_sample_adjacent_animation_frames_at_fractional_zoom() {
    let assets = retzar::assets::GameAssets::open(std::path::Path::new("..")).unwrap();
    let grid = retzar::map::MapGrid {
        name: "coast seam regression".into(),
        side: 8,
        cells: vec![0x1d; 64],
    };
    let mut terrain = retzar::terrain::Terrain::load(&assets, "desert").unwrap();
    terrain.prepare(&grid).unwrap();
    // Only the selected frame is black. Every neighbouring frame is white:
    // a UV escaping its tile becomes an unambiguous bright seam.
    for s in &mut terrain.sheets {
        s.image.rgba.fill(255);
    }
    for y in 0..grid.side {
        for x in 0..grid.side {
            let t = terrain.tile(grid.cells[y * grid.side + x], x, y).unwrap();
            let b = &mut terrain.sheets[t.sheet].image;
            for yy in t.y..t.y + 32 {
                for xx in t.x..t.x + 32 {
                    let at = ((yy * b.width + xx) * 4) as usize;
                    b.rgba[at..at + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
    }
    let batches = retzar::scene::batches(&grid, &terrain).unwrap();
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let results = Arc::new(Mutex::new(0));
    let f = failures.clone();
    let r = results.clone();
    let mut app = App::new();
    app.insert_resource(WinitSettings::continuous())
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "ReTzar: coast UV regression".into(),
                        resolution: (512, 512).into(),
                        visible: false,
                        ..default()
                    }),
                    ..default()
                })
                .set(WinitPlugin {
                    run_on_any_thread: true,
                    ..default()
                })
                .set(ImagePlugin::default_nearest())
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                        backends: Some(Backends::VULKAN),
                        ..default()
                    })),
                    ..default()
                }),
        );
    let materials: Vec<_> = terrain
        .sheets
        .into_iter()
        .map(|s| {
            let image = app
                .world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new(
                    Extent3d {
                        width: s.image.width,
                        height: s.image.height,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    s.image.rgba,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                ));
            app.world_mut()
                .resource_mut::<Assets<ColorMaterial>>()
                .add(ColorMaterial {
                    texture: Some(image),
                    ..default()
                })
        })
        .collect();
    for b in batches {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, b.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, b.uv);
        mesh.insert_indices(Indices::U32(b.indices));
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        app.world_mut().spawn((
            Mesh2d(mesh),
            MeshMaterial2d(materials[b.sheet].clone()),
            Transform::from_xyz(b.origin[0], b.origin[1], 0.),
        ));
    }
    app.world_mut().spawn((
        Camera2d,
        Transform::from_xyz(128.5, -127.5, 1000.),
        Msaa::Off,
    ));
    app.add_systems(
        Update,
        move |mut commands: Commands,
              mut frame: Local<u32>,
              mut cameras: Query<&mut Projection>,
              mut exit: MessageWriter<AppExit>| {
            *frame += 1;
            let phase = (*frame / 40).min(3) as usize;
            let zoom = [1., 1.5, 2., 2.5][phase];
            for mut p in &mut cameras {
                if let Projection::Orthographic(p) = &mut *p {
                    p.scale = 1. / zoom;
                }
            }
            if [25, 65, 105, 145].contains(&*frame) {
                let f = f.clone();
                let r = r.clone();
                commands.spawn(Screenshot::primary_window()).observe(
                    move |capture: On<ScreenshotCaptured>| {
                        let img = capture.image.clone().try_into_dynamic().unwrap().to_rgb8();
                        let mut bad = 0;
                        for y in 0..512 {
                            for x in 0..512 {
                                let wx = 128.5 + (x as f32 + 0.5 - 256.) / zoom;
                                let wy = 127.5 + (y as f32 + 0.5 - 256.) / zoom;
                                if wx > 1.
                                    && wy > 1.
                                    && wx < 255.
                                    && wy < 255.
                                    && img.get_pixel(x, y).0 != [0; 3]
                                {
                                    bad += 1;
                                }
                            }
                        }
                        if bad > 0 {
                            f.lock().unwrap().push(format!(
                                "zoom {zoom}: {bad} bright pixels leaked from neighbouring frames"
                            ));
                        }
                        *r.lock().unwrap() += 1;
                    },
                );
            }
            if *frame >= 175 {
                exit.write(AppExit::Success);
            }
        },
    )
    .run();
    assert_eq!(*results.lock().unwrap(), 4);
    assert!(
        failures.lock().unwrap().is_empty(),
        "{:?}",
        failures.lock().unwrap()
    );
}
