//! Real GPU regression, including the production RGB555 post-process shader.
#![cfg(feature = "game")]
#[path = "../src/frontend/legacy_color.rs"]
mod legacy_color;

use bevy::{
    asset::RenderAssetUsages,
    camera::{CompositingSpace, visibility::RenderLayers},
    prelude::*,
    render::{
        RenderPlugin,
        render_resource::*,
        settings::{Backends, RenderCreation, WgpuSettings},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    winit::{WinitPlugin, WinitSettings},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
};

const COLORS: [[u8; 3]; 4] = [[255, 255, 255], [132, 66, 198], [33, 82, 165], [8, 16, 24]];
#[derive(Component)]
struct MapTest;
#[derive(Component)]
struct MenuTest;
fn image(pixel: [u8; 4]) -> Image {
    Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixel.to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}
fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        Camera2d,
        Camera {
            is_active: false,
            ..default()
        },
        MapTest,
        CompositingSpace::Srgb,
        legacy_color::LegacyColor::default(),
        RenderLayers::layer(1),
        Msaa::Off,
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::Default,
            ..default()
        },
        RenderLayers::layer(2),
        CompositingSpace::Srgb,
        Msaa::Off,
    ));
    // Render a menu first. After switching to the map, transparent UI regions
    // must load this frame's map instead of reusing that old menu framebuffer.
    commands.spawn((
        Sprite::from_color(Color::srgb_u8(255, 0, 0), Vec2::new(512., 256.)),
        Transform::from_xyz(0., 0., 10.),
        RenderLayers::layer(2),
        MenuTest,
    ));
    let shadow = images.add(image(retzar::sprite::SHADOW));
    for (i, [r, g, b]) in COLORS.into_iter().enumerate() {
        let x = i as f32 * 128. - 192.;
        commands.spawn((
            Sprite {
                image: images.add(image([r, g, b, 255])),
                custom_size: Some(Vec2::new(128., 256.)),
                ..default()
            },
            Transform::from_xyz(x, 0., 0.),
            RenderLayers::layer(1),
        ));
        for (y, z) in [(64., 1.), (-64., 1.), (-64., 2.)] {
            commands.spawn((
                Sprite {
                    image: shadow.clone(),
                    custom_size: Some(Vec2::new(128., 64.)),
                    ..default()
                },
                Transform::from_xyz(x, y, z),
                RenderLayers::layer(1),
            ));
        }
    }
    // This bitmap belongs to the UI camera and must keep its full eight bits.
    commands.spawn((
        Sprite {
            image: images.add(image([200, 100, 50, 255])),
            custom_size: Some(Vec2::splat(16.)),
            ..default()
        },
        Transform::from_xyz(0., 108., 0.),
        RenderLayers::layer(2),
    ));
}

#[test]
#[ignore = "Requires Vulkan and a native window; captures a hidden GPU test window"]
fn shadows_match_rgb555_halving_and_ui_keeps_its_colors() {
    let done = Arc::new(AtomicU32::new(0));
    let captured = done.clone();
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured_failures = failures.clone();
    let output = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("shadow-render.png");
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    App::new()
        .insert_resource(WinitSettings::continuous())
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "ReTzar: regressione ombre".into(),
                        resolution: (512, 256).into(),
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
        )
        .add_plugins(legacy_color::LegacyColorPlugin)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            move |mut commands: Commands,
                  mut frame: Local<u32>,
                  mut cameras: Query<(&mut Camera, Has<MapTest>)>,
                  mut lighting: Query<&mut legacy_color::LegacyColor, With<MapTest>>,
                  mut menus: Query<&mut Visibility, With<MenuTest>>,
                  mut exit: MessageWriter<AppExit>| {
                *frame += 1;
                if *frame == 12 {
                    for (mut camera, is_map) in &mut cameras {
                        if is_map {
                            camera.is_active = true;
                        } else {
                            camera.clear_color = ClearColorConfig::None;
                        }
                    }
                    for mut visibility in &mut menus {
                        *visibility = Visibility::Hidden;
                    }
                }
                if *frame == 70 {
                    for mut color in &mut lighting {
                        color.light = Vec4::new(0.55, 0.68, 1., 1.);
                    }
                }
                if *frame == 40 || *frame == 100 {
                    let night = *frame == 100;
                    let done = captured.clone();
                    let failures = captured_failures.clone();
                    let output = output.clone();
                    commands.spawn(Screenshot::primary_window()).observe(
                        move |capture: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                            let img = capture.image.clone().try_into_dynamic().unwrap().to_rgb8();
                            for (i, color) in COLORS.iter().enumerate() {
                                for (y, shift) in [(128, 0), (64, 1), (192, 2)] {
                                    let mut channel = 0;
                                    let expected = color.map(|c| {
                                        let five = (c >> 3) >> shift;
                                        let original = (five << 3) | (five >> 2);
                                        let result = if night {
                                            let tinted = (original as f32
                                                * [0.55, 0.68, 1.][channel])
                                                .round()
                                                as u8
                                                >> 3;
                                            (tinted << 3) | (tinted >> 2)
                                        } else {
                                            original
                                        };
                                        channel += 1;
                                        result
                                    });
                                    let actual = img.get_pixel(i as u32 * 128 + 64, y).0;
                                    if actual != expected {
                                        failures.lock().unwrap().push(format!("night={night}, color={color:?}, shadows={shift}: {actual:?} != {expected:?}"));
                                    }
                                }
                            }
                            if img.get_pixel(256, 20).0 != [200, 100, 50] {
                                failures.lock().unwrap().push(format!("night={night}: HUD was tinted"));
                            }
                            img.save(if night {
                                output.with_file_name("shadow-render-night.png")
                            } else {
                                output.clone()
                            })
                            .unwrap();
                            done.fetch_add(1, Ordering::SeqCst);
                            if night {
                                exit.write(AppExit::Success);
                            }
                        },
                    );
                }
                if *frame > 300 {
                    exit.write(AppExit::Success);
                }
            },
        )
        .run();
    assert!(
        done.load(Ordering::SeqCst) == 2,
        "GPU screenshot did not complete"
    );
    assert!(
        failures.lock().unwrap().is_empty(),
        "{:?}",
        failures.lock().unwrap()
    );
}
