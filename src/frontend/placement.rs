//! Building preview uses the same original sheets and footprint as construction.
use super::{
    Game, MenuState, gpu_image,
    grid::{self, GridMaterial},
    mission::MissionUi,
    scene::Loaded,
    view::{MapView, Surface, hud_layout},
};
use bevy::{camera::visibility::RenderLayers, prelude::*, window::PrimaryWindow};
use retzar::{
    gameplay, hud,
    world::{FLAG_INACTIVE, class},
};
use std::collections::BTreeSet;

#[derive(Resource, Default)]
pub struct Placement {
    pub valid: bool,
    pub cell: Option<[u16; 2]>,
    key: String,
    parts: Vec<(Entity, Vec2, Vec2, bool)>,
    images: Vec<Handle<Image>>,
    /// The cells of a wall row being dragged (0x4fb850).
    row: Vec<Entity>,
    /// The placement grid of this version.
    grid: Option<Grid>,
}
/// The placement grid over the map of scene `generation`, and the state
/// its refused cells were last marked for: tick, cell area, building kind
/// and owner.
struct Grid {
    entity: Entity,
    material: Handle<GridMaterial>,
    marks: Handle<Image>,
    generation: u32,
    marked: Option<(u64, [i32; 4], usize, u8)>,
}
impl Placement {
    /// The grid's uniforms while it is shown (`GridMaterial`).
    #[cfg(test)]
    pub fn grid_uniforms(&self, world: &World) -> Option<[Vec4; 2]> {
        let grid = self.grid.as_ref()?;
        let m = world
            .resource::<Assets<GridMaterial>>()
            .get(&grid.material)?;
        Some([m.look, m.tone])
    }
    /// The cells the grid marks refused, as its texture holds them.
    #[cfg(test)]
    pub fn grid_refused(&self, world: &World, side: usize) -> Option<Vec<bool>> {
        let grid = self.grid.as_ref()?;
        let image = world.resource::<Assets<Image>>().get(&grid.marks)?;
        let data = image.data.as_ref()?;
        Some(data[..side * side].iter().map(|&b| b != 0).collect())
    }
    /// No building is being placed: the preview and the grid go.
    fn clear(&mut self, world: &mut World) {
        self.clear_preview(world);
        self.hide_grid(world);
    }
    fn hide_grid(&mut self, world: &mut World) {
        if let Some(grid) = self.grid.take() {
            world.despawn(grid.entity);
        }
    }
    /// The grid over the whole map of scene `generation`, darker around
    /// `focus`, with one of [`grid::TONES`].
    fn show_grid(
        &mut self,
        world: &mut World,
        side: usize,
        generation: u32,
        focus: Option<Vec2>,
        width: f32,
        strength: usize,
    ) {
        if self
            .grid
            .as_ref()
            .is_some_and(|g| g.generation != generation)
        {
            self.hide_grid(world);
        }
        let grid = self.grid.get_or_insert_with(|| {
            let edge = side as f32 * 32.;
            let mesh = world
                .resource_mut::<Assets<Mesh>>()
                .add(Rectangle::new(edge, edge));
            let marks = world
                .resource_mut::<Assets<Image>>()
                .add(grid::marks_image(side));
            let material = world
                .resource_mut::<Assets<GridMaterial>>()
                .add(GridMaterial {
                    look: Vec4::ZERO,
                    tone: Vec4::ZERO,
                    marks: marks.clone(),
                });
            let entity = world
                .spawn((
                    Mesh2d(mesh),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(edge / 2., -edge / 2., grid::DEPTH),
                    RenderLayers::layer(1),
                ))
                .id();
            Grid {
                entity,
                material,
                marks,
                generation,
                marked: None,
            }
        });
        let look = match focus {
            Some(p) => Vec4::new(p.x, p.y, 1., width),
            None => Vec4::new(0., 0., 0., width),
        };
        let tone = Vec4::from((
            Vec3::from_array(grid::TONES[strength % grid::TONES.len()]),
            0.,
        ));
        let mut materials = world.resource_mut::<Assets<GridMaterial>>();
        if materials
            .get(&grid.material)
            .is_some_and(|m| m.look != look || m.tone != tone)
            && let Some(mut m) = materials.get_mut(&grid.material)
        {
            m.look = look;
            m.tone = tone;
        }
    }
    /// Marks on the grid's texture the cells of `area` that a building like
    /// `probe` could not cover, as the player knows them; again only when
    /// the game, the area or the building changed.
    fn mark_grid(
        &mut self,
        world: &mut World,
        s: &super::scene::Scene,
        probe: &retzar::world::MapObject,
        area: [i32; 4],
    ) {
        let Some(grid) = self.grid.as_mut() else {
            return;
        };
        let ticks = s.mission.simulation.as_ref().map_or(0, |sim| sim.ticks);
        let key = (ticks, area, probe.class, probe.owner);
        if grid.marked == Some(key) {
            return;
        }
        grid.marked = Some(key);
        let refused = gameplay::refused_cells(&s.grid, &s.mission, &s.library, probe, area);
        let mut images = world.resource_mut::<Assets<Image>>();
        let Some(mut image) = images.get_mut(&grid.marks) else {
            return;
        };
        let Some(data) = image.data.as_mut() else {
            return;
        };
        let [x0, y0, x1, _] = area;
        let width = (x1 - x0).max(1) as usize;
        for (k, refused) in refused.into_iter().enumerate() {
            let (x, y) = (x0 as usize + k % width, y0 as usize + k / width);
            data[y * s.side + x] = if refused { 255 } else { 0 };
        }
    }
    fn clear_preview(&mut self, world: &mut World) {
        for (e, _, _, _) in self.parts.drain(..) {
            world.despawn(e);
        }
        for e in self.row.drain(..) {
            world.despawn(e);
        }
        self.images.clear();
        self.key.clear();
        self.cell = None;
        self.valid = false;
    }
}
pub fn update(world: &mut World) {
    let mut preview = world.remove_resource::<Placement>().unwrap_or_default();
    let order = world.resource::<MissionUi>().target_order.clone();
    let wall_from = world.resource::<MissionUi>().wall_from;
    let alt = {
        let keys = world.resource::<ButtonInput<KeyCode>>();
        keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight)
    };
    let selected = world.resource::<MissionUi>().hud.selected.clone();
    let spawning = world.resource::<MissionUi>().spawning;
    let window = {
        let mut q = world.query_filtered::<&Window, With<PrimaryWindow>>();
        q.iter(world).next().cloned()
    };
    let Some(window) = window else {
        preview.clear(world);
        world.insert_resource(preview);
        return;
    };
    let surface = Surface::of(&window);
    let cursor = window.cursor_position();
    let settings = world.resource::<MenuState>().current_settings().clone();
    let layout = hud_layout(surface, &settings);
    let panel = world.resource::<MissionUi>().hud.panel.is_some();
    let pointer = cursor.map(|p| world.resource::<MapView>().world(surface, p));
    let magnification = world.resource::<MapView>().magnification(surface);
    // The map points at the top left and bottom right of the window.
    let screen = {
        let view = world.resource::<MapView>();
        [
            view.world(surface, Vec2::ZERO),
            view.world(surface, surface.physical / surface.dpi),
        ]
    };
    let over_map = cursor.is_some_and(|p| {
        let p = layout.point(surface, p);
        !hud::blocks_map(
            &world.resource::<MissionUi>().hud,
            [p.x, p.y],
            hud::Layout {
                width: layout.size.x as i32,
                height: layout.size.y as i32,
                left: settings.layout == 1,
            },
        )
    });
    world.resource_scope(|world, loaded: Mut<Loaded>| {
        let Some(s) = &loaded.scene else {
            preview.clear(world);
            return;
        };
        world.resource_scope(|world, game: Mut<Game>| {
            let inside = |p: &Vec2| {
                over_map
                    && !panel
                    && p.x >= 0.
                    && p.y <= 0.
                    && p.x < s.side as f32 * 32.
                    && -p.y < s.side as f32 * 32.
            };
            // The cheat panel's «spawn»: the object at the pointer, green
            // where its site test passes (vt+0x28c).
            if let Some((class, civ, owner)) = spawning {
                preview.hide_grid(world);
                let probe = pointer.filter(inside).and_then(|p| {
                    let cell = [(p.x / 32.) as u16, (-p.y / 32.) as u16];
                    gameplay::cheats::spawn_probe(&s.library, class, civ, owner, cell)
                });
                let Some(probe) = probe else {
                    preview.clear_preview(world);
                    return;
                };
                let valid = s.mission.simulation.as_ref().is_some_and(|sim| {
                    gameplay::cheats::spawn_site(sim, &s.mission, &s.grid, &s.library, &probe)
                });
                let key = format!("{}:spawn:{class}:{civ}:{owner}", loaded.generation);
                show_preview(&mut preview, world, s, &game, &probe, valid, key);
                preview.valid = valid;
                preview.cell = Some([probe.x, probe.y]);
                return;
            }
            // Buildings, and the crusade anchor a cathedral places.
            let Some(order) = order.as_ref().filter(|o| {
                let a = game.gameplay.action(o);
                a.starts_with("build ") || a.eq_ignore_ascii_case("create crusadeanchor")
            }) else {
                preview.clear(world);
                return;
            };
            let builder = if game.gameplay.action(order).starts_with("build ") {
                class::PEON
            } else {
                class::CATHEDRAL
            };
            let Some(worker) = selected
                .iter()
                .filter_map(|&i| s.mission.objects.get(i))
                .find(|o| {
                    o.owner == s.mission.player()
                        && o.class == builder
                        && o.flags & FLAG_INACTIVE == 0
                })
            else {
                preview.clear(world);
                return;
            };
            let on_map = pointer.filter(|p| {
                over_map
                    && !panel
                    && p.x >= 0.
                    && p.y <= 0.
                    && p.x < s.side as f32 * 32.
                    && -p.y < s.side as f32 * 32.
            });
            if settings.placement_grid {
                preview.show_grid(
                    world,
                    s.side,
                    loaded.generation,
                    on_map,
                    grid::line_width(magnification),
                    settings.grid_strength,
                );
                // The cells on screen, those cut by its edges included.
                let side = s.side as f32;
                let area = [
                    (screen[0].x / 32.).floor().clamp(0., side),
                    (-screen[0].y / 32.).floor().clamp(0., side),
                    (screen[1].x / 32.).ceil().clamp(0., side),
                    (-screen[1].y / 32.).ceil().clamp(0., side),
                ]
                .map(|n| n as i32);
                // Only the kind and owner of the building matter, not where.
                if let Some(probe) =
                    gameplay::building_at(&s.library, &game.gameplay, worker, order, [16, 16])
                {
                    preview.mark_grid(world, s, &probe, area);
                }
            } else {
                preview.hide_grid(world);
            }
            let Some(p) = on_map else {
                for (e, _, _, _) in &preview.parts {
                    if let Some(mut v) = world.get_mut::<Visibility>(*e) {
                        *v = Visibility::Hidden;
                    }
                }
                preview.cell = None;
                return;
            };
            let Some(probe) = gameplay::building_at(
                &s.library,
                &game.gameplay,
                worker,
                order,
                [(p.x / 32.) as u16, (-p.y / 32.) as u16],
            ) else {
                preview.clear_preview(world);
                return;
            };
            let valid = gameplay::can_place(&s.grid, &s.mission, &s.library, &probe);
            let key = format!(
                "{}:{order}:{}:{}",
                loaded.generation, worker.civilization, worker.owner
            );
            show_preview(&mut preview, world, s, &game, &probe, valid, key);
            // A wall row being dragged shows each of its cells (0x4fb850).
            let row = match wall_from {
                Some(from) => super::mission::wall_preview(
                    s,
                    &game,
                    order,
                    &selected,
                    from,
                    [probe.x, probe.y],
                    alt,
                ),
                None => Vec::new(),
            };
            while preview.row.len() < row.len() {
                let e = world
                    .spawn((
                        Sprite::from_color(Color::WHITE, Vec2::splat(30.)),
                        Transform::from_xyz(0., 0., 404.),
                        RenderLayers::layer(1),
                    ))
                    .id();
                preview.row.push(e);
            }
            let extra: Vec<Entity> = preview.row.drain(row.len()..).collect();
            for e in extra {
                world.despawn(e);
            }
            for (&e, &(p, ok)) in preview.row.iter().zip(&row) {
                if let Some(mut t) = world.get_mut::<Transform>(e) {
                    t.translation.x = p[0] as f32 * 32. + 16.;
                    t.translation.y = -(p[1] as f32) * 32. - 16.;
                }
                if let Some(mut sprite) = world.get_mut::<Sprite>(e) {
                    sprite.color = if ok {
                        Color::srgba(0.3, 1., 0.3, 0.35)
                    } else {
                        Color::srgba(1., 0.2, 0.2, 0.4)
                    };
                }
            }
            preview.valid = valid;
            preview.cell = Some([probe.x, probe.y]);
        });
    });
    world.insert_resource(preview);
}

/// The object being placed at its place: its pictures and its cells, made
/// again when `key` changes, green where it can stand and red elsewhere.
fn show_preview(
    preview: &mut Placement,
    world: &mut World,
    s: &super::scene::Scene,
    game: &Game,
    probe: &retzar::world::MapObject,
    valid: bool,
    key: String,
) {
    if preview.key != key {
        preview.clear_preview(world);
        let mut mission = s.mission.clone();
        mission.simulation = None;
        let index = mission.objects.len();
        let mut art = probe.clone();
        art.x = 0;
        art.y = 0;
        mission.objects.push(art);
        if let Ok(built) = s.library.build_indices(
            &game.assets,
            &mission,
            s.side,
            Some(&BTreeSet::from([index])),
        ) {
            let sizes: Vec<_> = built
                .images
                .iter()
                .map(|b| Vec2::new(b.width as f32, b.height as f32))
                .collect();
            preview.images = built
                .images
                .into_iter()
                .map(|b| world.resource_mut::<Assets<Image>>().add(gpu_image(b)))
                .collect();
            for v in built.visuals {
                let size = sizes[v.image];
                let e = world
                    .spawn((
                        Sprite {
                            custom_size: Some(size),
                            ..Sprite::from_image(preview.images[v.image].clone())
                        },
                        Transform::from_xyz(0., 0., 405.),
                        RenderLayers::layer(1),
                    ))
                    .id();
                preview
                    .parts
                    .push((e, Vec2::from_array(v.top_left), size, false));
            }
        }
        let size = s.library.cell_footprint(probe);
        for y in 0..size[1] {
            for x in 0..size[0] {
                let e = world
                    .spawn((
                        Sprite::from_color(Color::WHITE, Vec2::splat(30.)),
                        Transform::from_xyz(0., 0., 404.),
                        RenderLayers::layer(1),
                    ))
                    .id();
                preview.parts.push((
                    e,
                    Vec2::new(x as f32 * 32., -(y as f32) * 32.),
                    Vec2::splat(30.),
                    true,
                ));
            }
        }
        preview.key = key;
    }
    for (e, at, size, tile) in &preview.parts {
        if let Some(mut t) = world.get_mut::<Transform>(*e) {
            t.translation.x = probe.x as f32 * 32. + at.x + size.x / 2.;
            t.translation.y = -(probe.y as f32) * 32. + at.y - size.y / 2.;
        }
        if let Some(mut sprite) = world.get_mut::<Sprite>(*e) {
            sprite.color = if valid {
                Color::srgba(0.3, 1., 0.3, if *tile { 0.25 } else { 0.6 })
            } else {
                Color::srgba(1., 0.2, 0.2, if *tile { 0.3 } else { 0.6 })
            };
        }
        if let Some(mut v) = world.get_mut::<Visibility>(*e) {
            *v = Visibility::Visible;
        }
    }
}
