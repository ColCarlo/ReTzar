//! Mission scenes. `prepare` does every CPU step (decoding, composing atlases,
//! building meshes) and runs on a loading thread; `install` only hands the
//! results to Bevy, so the window stays responsive while a mission opens.
use super::gpu_image;
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use retzar::{
    Result,
    animation::Timeline,
    assets::GameAssets,
    bitmap::Bitmap,
    flight::Flyer,
    map::MapGrid,
    menu::Selection,
    scenario::Scenario,
    scene,
    simulation::NativeRng,
    terrain::{Sheet, Terrain},
    world::{
        AIR_DEPTH, Flight, GameSetup, Layer, MissionWorld, ObjectLibrary, ObjectVisual,
        SHADOW_DEPTH, VisualScene, add_cliffs, add_trees, painter_depths,
    },
};
use std::{collections::HashMap, time::Duration};

/// Everything a mission needs, computed away from the main thread.
pub struct PreparedScene {
    pub grid: MapGrid,
    pub source_entries: std::collections::BTreeMap<String, std::sync::Arc<Vec<u8>>>,
    pub restored: Option<Restored>,
    pub environment: super::environment::Art,
    pub selection: Selection,
    pub scenario_id: Option<String>,
    pub side: usize,
    pub title: String,
    pub description: String,
    pub biome: String,
    /// Selection corners of the biome (`[colors] selection`).
    pub selection_color: [u8; 3],
    pub sheets: Vec<Sheet>,
    pub batches: Vec<scene::Batch>,
    /// Terrain colour of every cell, one pixel per cell.
    pub cells: Bitmap,
    pub mission: MissionWorld,
    pub library: ObjectLibrary,
    pub objects: VisualScene,
    /// Hero and objective portraits of the scenario (`USER\…`), by upper-case path.
    pub portraits: HashMap<String, Bitmap>,
}

pub struct Restored {
    pub metadata: retzar::menu::SavedSession,
    pub hud: retzar::hud::HudState,
    pub groups: [Vec<usize>; 9],
    pub health_kept: bool,
    pub idle: usize,
    pub visual_time: Duration,
    pub flyers: Vec<Flyer>,
    pub recovered: bool,
}

/// Rebuild only derived rendering resources. Do not reimport or restart a saved world.
pub fn prepare_snapshot(assets: &GameAssets, path: &std::path::Path) -> Result<PreparedScene> {
    let (metadata, s, recovered) = retzar::save::read(path)?;
    let mut terrain = Terrain::load(assets, &s.biome)?;
    terrain.prepare(&s.grid)?;
    let batches = scene::batches(&s.grid, &terrain)?;
    let cells = terrain.minimap(&s.grid)?;
    let library = ObjectLibrary::load(assets, &terrain.biome)?;
    let mut mission = s.mission;
    mission.simulation = Some(s.simulation);
    mission.starting_positions = s.starting_positions;
    let mut objects = library.build(assets, &mission, s.grid.side)?;
    add_cliffs(assets, &s.grid, &terrain.biome, &mut objects)?;
    add_trees(assets, &s.grid, &terrain.biome, &mut objects)?;
    if !objects.missing.is_empty() {
        return Err(format!(
            "Immagini del salvataggio non risolte: {}",
            objects.missing.join("; ")
        )
        .into());
    }
    if s.flyers.len() != objects.flights.len()
        || s.flyers
            .iter()
            .zip(&objects.flights)
            .any(|(f, spec)| !f.valid(spec))
    {
        return Err("Stato delle animazioni di volo non valido".into());
    }
    Ok(PreparedScene {
        environment: super::environment::Art::load(assets)?,
        selection: metadata.selection.clone(),
        scenario_id: metadata.scenario.clone(),
        side: s.grid.side,
        grid: s.grid,
        title: s.title,
        description: s.description,
        selection_color: terrain.selection,
        biome: terrain.biome,
        sheets: terrain.sheets,
        batches,
        cells,
        mission,
        library,
        objects,
        source_entries: s.source_entries,
        portraits: s.portraits.into_iter().collect(),
        restored: Some(Restored {
            metadata,
            hud: s.hud,
            groups: s.groups,
            health_kept: s.health_kept,
            idle: s.idle,
            visual_time: s.visual_time,
            flyers: s.flyers,
            recovered,
        }),
    })
}

pub fn prepare(
    assets: &GameAssets,
    selection: Selection,
    scenario: Option<Scenario>,
    setup: Option<GameSetup>,
) -> Result<PreparedScene> {
    let scenario_id = scenario.as_ref().map(|s| s.id.clone());
    // In Spanish, FX's campaigns speak with the Spanish edition's voices.
    let spanish = match &scenario {
        Some(s) if retzar::lang::current() == retzar::lang::Language::Spanish => s.spanish_voices(),
        _ => Vec::new(),
    };
    let retzar::game::Opened {
        grid,
        mut mission,
        biome,
        title,
        description,
        container,
    } = retzar::game::open(assets, &selection, scenario, setup.as_ref())?;
    let (portraits, mut source_entries): (_, std::collections::BTreeMap<_, _>) = match &container {
        Some(c) => (
            portraits(c, &mission),
            c.entries
                .iter()
                .map(|e| Ok((e.name.clone(), std::sync::Arc::new(c.read(e.inode)?))))
                .collect::<Result<_>>()?,
        ),
        None => (HashMap::new(), std::collections::BTreeMap::new()),
    };
    if !spanish.is_empty() {
        for (name, data) in spanish {
            source_entries.retain(|k: &String, _| !k.eq_ignore_ascii_case(&name));
            source_entries.insert(name, std::sync::Arc::new(data));
        }
        mission.written_in = Some(retzar::lang::Language::Spanish);
    }
    let mut terrain = Terrain::load(assets, &biome)?;
    terrain.prepare(&grid)?;
    let batches = scene::batches(&grid, &terrain)?;
    let cells = terrain.minimap(&grid)?;
    let library = ObjectLibrary::load(assets, &terrain.biome)?;
    let climate = retzar::terrain::Climate {
        clouds: terrain.clouds,
        rain: terrain.rain,
        snow: terrain.snow,
    };
    retzar::game::start(
        &mut mission,
        &grid,
        &library,
        &terrain.biome,
        &selection,
        setup.as_ref(),
        climate,
    )?;
    // The mission panel shows the scenario description, as the original.
    let mut objects = library.build(assets, &mission, grid.side)?;
    add_cliffs(assets, &grid, &terrain.biome, &mut objects)?;
    add_trees(assets, &grid, &terrain.biome, &mut objects)?;
    if !objects.missing.is_empty() {
        return Err(format!(
            "Immagini originali non risolte: {}",
            objects.missing.join("; ")
        )
        .into());
    }
    Ok(PreparedScene {
        source_entries,
        restored: None,
        grid: grid.clone(),
        environment: super::environment::Art::load(assets)?,
        selection,
        scenario_id,
        side: grid.side,
        title,
        description,
        selection_color: terrain.selection,
        biome: terrain.biome,
        sheets: terrain.sheets,
        batches,
        cells,
        mission,
        library,
        objects,
        portraits,
    })
}

/// Decode the portraits the interface may show: heroes (48×48) and
/// objectives (96×96), 16-bit bitmaps stored in the scenario. A missing or
/// damaged picture only leaves its frame empty.
fn portraits(c: &retzar::hpfs::Container, mission: &MissionWorld) -> HashMap<String, Bitmap> {
    use retzar::script::Action;
    // The pictures of the scripts' messages and stories, and the scrolls'
    // (record 0x460).
    let pictures: Vec<String> = mission
        .script
        .actions
        .iter()
        .filter_map(|a| match &a.value {
            Action::Message { image, .. } | Action::Story { image, .. } => Some(image.clone()),
            _ => None,
        })
        .chain(
            mission
                .objects
                .iter()
                .filter(|o| o.class == retzar::world::class::ITEM_SCROLL)
                .filter_map(|o| o.data.get(0x460..0x4a0))
                .map(|b| retzar::assets::legacy_text(b.split(|c| *c == 0).next().unwrap_or(&[]))),
        )
        .collect();
    let paths = mission
        .objects
        .iter()
        .flat_map(|o| [o.portrait.as_str(), o.portrait_large.as_str()])
        .chain(mission.quests.iter().map(|q| q.portrait.as_str()))
        .chain(pictures.iter().map(String::as_str))
        .filter(|p| !p.is_empty());
    let mut out = HashMap::new();
    for path in paths {
        let key = path.replace('\\', "/").to_ascii_uppercase();
        if out.contains_key(&key) {
            continue;
        }
        if let Ok(image) = c.get(&key).and_then(|data| Bitmap::decode(&data, false)) {
            out.insert(key, image);
        }
    }
    out
}

pub struct TerrainFrames {
    pub material: Handle<ColorMaterial>,
    pub frames: Vec<Handle<Image>>,
}
pub struct WaterMesh {
    pub mesh: Handle<Mesh>,
    pub tiles: Vec<scene::WaterTile>,
}
pub struct VisualAnimation {
    pub entity: Entity,
    pub object: usize,
    pub frames: Vec<Handle<Image>>,
    pub timeline: Timeline,
    pub phase_ms: u64,
    /// When the object's figure was set (vt+0xe8), on the game clock: its
    /// timer starts there at the start row. None for the scenery, which
    /// shares one ticker per pace (0x4312b0), and the fish.
    pub since_ms: Option<u64>,
    /// The row the picture starts at (`startrow`, 0x5017f0).
    pub start: usize,
    /// The frame timer of an idle unit (message 0x2013).
    pub idle: Option<IdleClock>,
    pub last: usize,
    pub motion: Option<(usize, u32)>,
    pub action: Option<usize>,
    /// The shadow of a dying flier: its lift and where it would lie under
    /// the body ([`retzar::world::descent`]).
    pub descent: Option<([f32; 2], Vec2)>,
}
/// An idle unit's frame and when the next one is due, on the game clock,
/// for the idle period that began at `since`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IdleClock {
    pub since: u64,
    pub frame: usize,
    pub due: u64,
}
impl IdleClock {
    /// The start row, shown for its own delay (0x501780, 0x4e5f20).
    pub fn new(timeline: &Timeline, since: u64, start: usize) -> Self {
        Self {
            since,
            frame: start,
            due: since + timeline.delay(start).max(1),
        }
    }
    /// The frame timer up to `now` (message 0x2013, 0x4e144f): the next
    /// frame, and a frame shown 3500 ms or more gets R(−3000, 3000) from the
    /// stream of sounds and pictures; a frozen or petrified unit keeps its
    /// frame and its timer comes back in 10 s (0x4e1314).
    pub fn advance(&mut self, timeline: &Timeline, now: u64, still: bool, rng: &mut NativeRng) {
        let rows = timeline.ends().len().max(1);
        let mut steps = 0;
        while now >= self.due {
            steps += 1;
            if steps > 4096 {
                self.due = now + 1;
                return;
            }
            if still {
                self.due += 10_000;
                continue;
            }
            self.frame = (self.frame + 1) % rows;
            let mut delay = timeline.delay(self.frame) as i64;
            if delay >= 3500 {
                delay += rng.range(-3000, 3000) as i64;
            }
            self.due += delay.max(1) as u64;
        }
    }
}
/// A bird on the wing: its flight state and the two sprites it moves.
pub struct FlyingSprite {
    pub spec: Flight,
    pub flyer: Flyer,
    pub body: Entity,
    pub shadow: Option<Entity>,
    pub size: Vec2,
    pub frames: Vec<Vec<Handle<Image>>>,
    pub shadow_frames: Vec<Vec<Handle<Image>>>,
    pub last: (usize, usize),
}

/// A mission on screen and every GPU resource it owns.
pub struct Scene {
    pub grid: MapGrid,
    pub source_entries: std::collections::BTreeMap<String, std::sync::Arc<Vec<u8>>>,
    pub environment: Option<super::environment::EnvironmentScene>,
    pub visual_entities: Vec<Entity>,
    pub selection: Selection,
    pub scenario_id: Option<String>,
    pub side: usize,
    pub title: String,
    pub biome: String,
    pub selection_color: [u8; 3],
    pub description: String,
    pub mission: MissionWorld,
    pub library: ObjectLibrary,
    pub visuals: Vec<ObjectVisual>,
    pub cells: Bitmap,
    pub entities: Vec<Entity>,
    pub images: Vec<Handle<Image>>,
    pub meshes: Vec<Handle<Mesh>>,
    pub materials: Vec<Handle<ColorMaterial>>,
    pub visual_time: Duration,
    pub water_tick: u64,
    pub terrain_frames: Vec<TerrainFrames>,
    pub water_meshes: Vec<WaterMesh>,
    pub sprite_animations: Vec<VisualAnimation>,
    pub flights: Vec<FlyingSprite>,
    pub portraits: HashMap<String, Bitmap>,
}
impl Scene {
    /// The open game in another language: the names of units, buildings,
    /// items and spells, which the library reads from `locale` (the
    /// language's STRINGS.INI), and the mission's texts. A random map's
    /// names come from no catalog and stay as they are.
    pub fn relocalize(&mut self, language: retzar::lang::Language, locale: retzar::assets::Ini) {
        self.library.locale = locale;
        if matches!(self.selection, Selection::Random { .. }) {
            return;
        }
        retzar::lang::localize_mission(&mut self.mission, language);
        for text in [&mut self.title, &mut self.description] {
            if let Some(t) = retzar::lang::translate(text, language) {
                *text = t.to_owned();
            }
        }
    }
}

/// The open mission. `generation` changes with every installed scene; the
/// scene itself is mutated by animation every frame, so change detection on
/// this resource cannot tell a new mission from an animated one.
#[derive(Resource, Default)]
pub struct Loaded {
    pub scene: Option<Scene>,
    pub generation: u32,
}

/// Upload a prepared mission. Infallible: every check happened in `prepare`.
pub fn install(world: &mut World, p: PreparedScene) -> Scene {
    let mut textures = Vec::new();
    let mut materials = Vec::new();
    let mut terrain_frames = Vec::new();
    for sheet in p.sheets {
        let frames: Vec<_> = std::iter::once(sheet.image)
            .chain(sheet.frames)
            .map(|b| world.resource_mut::<Assets<Image>>().add(gpu_image(b)))
            .collect();
        let material = world
            .resource_mut::<Assets<ColorMaterial>>()
            .add(ColorMaterial {
                texture: Some(frames[0].clone()),
                ..default()
            });
        textures.extend(frames.iter().cloned());
        materials.push(material.clone());
        if frames.len() > 1 {
            terrain_frames.push(TerrainFrames { material, frames });
        }
    }
    let mut entities = Vec::new();
    let mut meshes = Vec::new();
    let mut water_meshes = Vec::new();
    for b in p.batches {
        let normals = vec![[0., 0., 1.]; b.positions.len()];
        // Only coast meshes change on the CPU (UV frames); the rest live on the GPU.
        let usage = if b.animations.is_empty() {
            RenderAssetUsages::RENDER_WORLD
        } else {
            RenderAssetUsages::default()
        };
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, usage);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, b.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, b.uv);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_indices(Indices::U32(b.indices));
        let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
        if !b.animations.is_empty() {
            water_meshes.push(WaterMesh {
                mesh: h.clone(),
                tiles: b.animations,
            });
        }
        entities.push(
            world
                .spawn((
                    Mesh2d(h.clone()),
                    MeshMaterial2d(materials[b.sheet].clone()),
                    Transform::from_xyz(b.origin[0], b.origin[1], 0.),
                    RenderLayers::layer(1),
                ))
                .id(),
        );
        meshes.push(h);
    }
    let sizes: Vec<Vec2> = p
        .objects
        .images
        .iter()
        .map(|b| Vec2::new(b.width as f32, b.height as f32))
        .collect();
    let object_textures: Vec<_> = p
        .objects
        .images
        .into_iter()
        .map(|b| world.resource_mut::<Assets<Image>>().add(gpu_image(b)))
        .collect();
    let mut sprite_animations = Vec::new();
    let mut entities_by_visual = Vec::with_capacity(p.objects.visuals.len());
    let painter = painter_depths(&p.objects.visuals, &p.mission, p.side);
    for (visual_index, v) in p.objects.visuals.iter().enumerate() {
        let size = sizes[v.image];
        let z = match v.layer {
            Layer::Shadow
                if p.objects
                    .flights
                    .iter()
                    .any(|f| f.shadow == Some(visual_index)) =>
            {
                SHADOW_DEPTH
            }
            Layer::Shadow => painter[visual_index],
            Layer::Air => AIR_DEPTH,
            Layer::Overlay => painter[visual_index],
            Layer::Body if v.ground => {
                if v.object == usize::MAX {
                    2.
                } else {
                    // Under every object, and among themselves in the
                    // painter's order (priority 1, then row and column):
                    // equal depths would leave overlaps to the renderer.
                    1. + (painter[visual_index] - 10.) / 380. * 0.9
                }
            }
            Layer::Body => painter[visual_index],
        };
        let entity = world
            .spawn((
                Sprite {
                    custom_size: Some(size),
                    color: p.mission.objects.get(v.object).map_or(Color::WHITE, |_| {
                        if p.mission.simulation.as_ref().is_some_and(|sim| {
                            sim.gameplay
                                .actors
                                .get(v.object)
                                .is_some_and(|a| a.effect_until > sim.ticks)
                        }) {
                            Color::srgba(1., 1., 1., 0.5)
                        } else {
                            Color::WHITE
                        }
                    }),
                    ..Sprite::from_image(object_textures[v.image].clone())
                },
                Transform::from_xyz(v.top_left[0] + size.x / 2., v.top_left[1] - size.y / 2., z),
                RenderLayers::layer(1),
            ))
            .id();
        entities.push(entity);
        entities_by_visual.push(entity);
        if let Some(index) = v.animation {
            let animation = &p.objects.animations[index];
            let phase_ms = v.phase_ms;
            // The objects of the map set their figures as it loads.
            let since_ms = p
                .mission
                .objects
                .get(v.object)
                .filter(|o| figure_set_on_creation(o.class))
                .map(|_| {
                    p.mission
                        .simulation
                        .as_ref()
                        .map_or(0, |sim| sim.elapsed.as_millis() as u64)
                });
            sprite_animations.push(VisualAnimation {
                entity,
                object: v.object,
                frames: animation
                    .frames
                    .iter()
                    .map(|&i| object_textures[i].clone())
                    .collect(),
                timeline: animation.timeline.clone(),
                phase_ms,
                since_ms,
                start: v.start_row,
                idle: None,
                last: usize::MAX,
                action: p.mission.simulation.as_ref().and_then(|sim| {
                    sim.objects
                        .get(v.object)
                        .filter(|o| {
                            matches!(
                                o.order,
                                retzar::simulation::OrderState::Working
                                    | retzar::simulation::OrderState::Building
                            )
                        })
                        .map(|_| v.object)
                }),
                motion: p.mission.simulation.as_ref().and_then(|sim| {
                    let o = p.mission.objects.get(v.object)?;
                    (sim.objects[v.object].order == retzar::simulation::OrderState::Moving)
                        .then(|| {
                            p.library
                                .carrying_image(
                                    o,
                                    sim.gameplay
                                        .actors
                                        .get(v.object)
                                        .map_or([0; 4], |a| a.cargo),
                                    sim.gameplay
                                        .actors
                                        .get(v.object)
                                        .map_or(0, |a| a.cargo_source),
                                )
                                .or_else(|| {
                                    p.library.runtime_image(
                                        o,
                                        Some(retzar::simulation::OrderState::Moving),
                                    )
                                })
                        })
                        .flatten()
                        .map(|section| {
                            (
                                v.object,
                                p.library
                                    .images
                                    .get(&section, "move_len")
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(32),
                            )
                        })
                }),
                descent: None,
            });
        }
    }
    let flights = p
        .objects
        .flights
        .into_iter()
        .map(|spec| {
            let handles = |frames: &Vec<Vec<usize>>| -> Vec<Vec<Handle<Image>>> {
                frames
                    .iter()
                    .map(|c| c.iter().map(|&i| object_textures[i].clone()).collect())
                    .collect()
            };
            FlyingSprite {
                flyer: Flyer::new(&spec),
                body: entities_by_visual[spec.body],
                shadow: spec.shadow.map(|i| entities_by_visual[i]),
                size: sizes[p.objects.visuals[spec.body].image],
                frames: handles(&spec.frames),
                shadow_frames: handles(&spec.shadow_frames),
                last: (usize::MAX, usize::MAX),
                spec,
            }
        })
        .collect();
    textures.extend(object_textures);
    let mut scene = Scene {
        grid: p.grid,
        source_entries: p.source_entries,
        environment: None,
        visual_entities: entities_by_visual,
        selection: p.selection,
        scenario_id: p.scenario_id,
        side: p.side,
        title: p.title,
        biome: p.biome,
        selection_color: p.selection_color,
        description: p.description,
        mission: p.mission,
        library: p.library,
        visuals: p.objects.visuals,
        cells: p.cells,
        entities,
        images: textures,
        meshes,
        materials,
        visual_time: Duration::ZERO,
        water_tick: u64::MAX,
        terrain_frames,
        water_meshes,
        sprite_animations,
        flights,
        portraits: p.portraits,
    };
    super::environment::install(world, &mut scene, p.environment);
    scene
}

/// Despawn a scene and free every texture, mesh and material it created.
pub fn release(world: &mut World, old: Scene) {
    for e in old.entities {
        world.despawn(e);
    }
    for h in old.meshes {
        world.resource_mut::<Assets<Mesh>>().remove(h.id());
    }
    for h in old.materials {
        world.resource_mut::<Assets<ColorMaterial>>().remove(h.id());
    }
    for h in old.images {
        world.resource_mut::<Assets<Image>>().remove(h.id());
    }
}

/// A restored paused session must already show the saved frame and bird position.
pub fn refresh_visuals(world: &mut World, scene: &mut Scene) {
    use bevy::ecs::system::SystemState;
    let mut state = SystemState::<(
        ResMut<Assets<ColorMaterial>>,
        ResMut<Assets<Mesh>>,
        Query<(&mut Sprite, &mut Transform)>,
    )>::new(world);
    let (mut materials, mut meshes, mut sprites) =
        state.get_mut(world).expect("Installed scene assets");
    animate(
        &Time::default(),
        scene,
        &mut materials,
        &mut meshes,
        &mut sprites,
    );
}

/// The frame of a figure set at `since` (vt+0xe8): its timer starts there
/// at the start row and goes round the picture's delays.
pub fn figure_frame(timeline: &Timeline, since: u64, now: u64, start: usize) -> usize {
    timeline.frame(now.saturating_sub(since), timeline.start_of(start))
}
/// Units, buildings and items: their animation runs from when their figure
/// was set; the scenery's and the fish's from their own phase.
pub fn figure_set_on_creation(class: usize) -> bool {
    use retzar::world::class;
    !matches!(
        class,
        class::ANIMATION | class::GROUND_OBJECT | class::GROUND_PATCH | class::FISHPATCH
    )
}
/// Water, coast, idle animations and birds in flight. Only images, texture
/// coordinates and bird positions change; of the mission state only the
/// stream of sounds and pictures is drawn from (idle frames).
pub fn animate(
    time: &Time,
    scene: &mut Scene,
    materials: &mut Assets<ColorMaterial>,
    meshes: &mut Assets<Mesh>,
    sprites: &mut Query<(&mut Sprite, &mut Transform)>,
) {
    let before = scene.visual_time.as_millis() as u64;
    scene.visual_time += time.delta();
    let ms = scene.visual_time.as_millis() as u64;
    // Tzar's WATERFPS=4 timer is separate from the unit animation timelines.
    let tick = ms / 250;
    if tick != scene.water_tick {
        for a in &scene.terrain_frames {
            if let Some(mut m) = materials.get_mut(&a.material) {
                m.texture = Some(a.frames[tick as usize % a.frames.len()].clone());
            }
        }
        for a in &scene.water_meshes {
            if let Some(mut mesh) = meshes.get_mut(&a.mesh)
                && let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) =
                    mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
            {
                for tile in &a.tiles {
                    uv[tile.vertex..tile.vertex + 4].copy_from_slice(&tile.uv[tile.frame(tick)]);
                }
            }
        }
        scene.water_tick = tick;
    }
    for a in &mut scene.sprite_animations {
        let frame = if let Some((i, length)) = a.motion {
            scene
                .mission
                .simulation
                .as_ref()
                .and_then(|s| s.gameplay.actors.get(i))
                .map_or(0, |_| {
                    let sim = scene.mission.simulation.as_ref().unwrap();
                    let rows = a.timeline.ends().len().max(1);
                    // A ninja's jump shows a frame per step (0x4ebaaa).
                    if let Some(k) = retzar::gameplay::jump_frame(sim, i) {
                        return k as usize % rows;
                    }
                    // A new frame every anim_time·move_len / (rows·1000)
                    // pixels, counted again at each cell (0x49893e, 0x4e13b2).
                    let (cells, px) = retzar::gameplay::walk_progress(sim, i);
                    let rows = a.timeline.ends().len().max(1) as u64;
                    let per = (a.timeline.duration_ms * length as u64 / (rows * 1000)).max(1);
                    let walked = (cells as u64 * (32 / per) + px as u64 / per) % rows;
                    // Waiting on a blocked cell a land unit holds its frame;
                    // flyers and ships go on at the picture's pace (0x4e1359).
                    let waited = retzar::gameplay::blocked_since(sim, i)
                        .filter(|_| scene.library.layer(&scene.mission.objects[i]) != 1)
                        .map_or(0, |since| {
                            let now = sim.elapsed.as_millis() as u64;
                            a.timeline.frame(now.saturating_sub(since), 0) as u64
                        });
                    // The walk starts at the picture's start row (0x5017f0).
                    ((a.start as u64 + walked + waited) % rows) as usize
                })
        } else if let Some(i) = a.action {
            scene
                .mission
                .simulation
                .as_ref()
                .and_then(|s| s.gameplay.actors.get(i))
                .map_or(0, |actor| a.timeline.frame(actor.action_elapsed_ms, 0))
        } else if let Some(sim) = scene.mission.simulation.as_mut() {
            // Frame timers run on the game's clock (queue 0xc97c54): they
            // stop with it and follow the game speed; a frozen or petrified
            // unit holds its frame (0x4e5fa0).
            let still = retzar::gameplay::still(sim, a.object);
            let now = sim.elapsed.as_millis() as u64;
            let rows = a.timeline.ends().len().max(1);
            let start = a.start % rows;
            // A grazing cow plays its picture from when it was set:
            // cow_down stops on its last frame, or runs from it back to the
            // first (message 0x2013 of 0x4ecab0).
            let class = scene.mission.objects.get(a.object).map_or(0, |o| o.class);
            let grazing = retzar::gameplay::grazing(sim, class, a.object);
            // A unit's idle picture starts again at its start row when it
            // stops (0x4e1b93, 0x501780).
            let idle_since = sim
                .gameplay
                .actors
                .get(a.object)
                .filter(|x| {
                    x.intent.is_none()
                        && x.idle_since_ms > 0
                        && scene
                            .mission
                            .objects
                            .get(a.object)
                            .is_some_and(retzar::selection::unit)
                })
                .map(|x| x.idle_since_ms);
            match idle_since {
                _ if still && grazing.is_some() => continue,
                _ if let Some((image, backwards, since)) = grazing => {
                    let elapsed = now.saturating_sub(since);
                    if image == retzar::gameplay::GRAZE[1] {
                        let ends = a.timeline.ends();
                        let passed = ends.partition_point(|&end| end <= elapsed).min(rows - 1);
                        if backwards { rows - 1 - passed } else { passed }
                    } else {
                        a.timeline.frame(elapsed, 0)
                    }
                }
                Some(since) => {
                    let mut c = a
                        .idle
                        .filter(|c| c.since == since)
                        .unwrap_or_else(|| IdleClock::new(&a.timeline, since, start));
                    c.advance(&a.timeline, now, still, &mut sim.gameplay.aux_rng);
                    a.idle = Some(c);
                    c.frame
                }
                None if still => continue,
                None => match a.since_ms {
                    Some(since) => figure_frame(&a.timeline, since, now, start),
                    None => a
                        .timeline
                        .frame(now, a.phase_ms + a.timeline.start_of(start)),
                },
            }
        } else {
            a.timeline.frame(ms, a.phase_ms)
        };
        if frame != a.last
            && let Ok((mut sprite, mut transform)) = sprites.get_mut(a.entity)
        {
            sprite.image = a.frames[frame].clone();
            a.last = frame;
            if let Some((lift, under)) = a.descent {
                let [dx, dy] = retzar::world::descent(lift, a.frames.len(), frame);
                transform.translation.x = under.x + dx;
                transform.translation.y = under.y - dy;
            }
        }
    }
    let sim = scene.mission.simulation.as_ref();
    let mut find = |from: [i32; 2], to: [i32; 2]| {
        sim.map_or_else(Vec::new, |s| retzar::gameplay::air_path(s, from, to))
    };
    for f in &mut scene.flights {
        f.flyer.advance(&f.spec, ms - before, &mut find);
        let [x, y] = f.flyer.top_left(&f.spec);
        let centre = Vec2::new(x + f.size.x / 2., y - f.size.y / 2.);
        let frame = f.flyer.frame();
        let [dx, dy] = f.spec.shadow_lift;
        for (entity, frames, shift) in [
            (Some(f.body), &f.frames, Vec2::ZERO),
            (f.shadow, &f.shadow_frames, Vec2::new(dx, -dy)),
        ] {
            let Some(Ok((mut sprite, mut transform))) = entity.map(|e| sprites.get_mut(e)) else {
                continue;
            };
            transform.translation.x = centre.x + shift.x;
            transform.translation.y = centre.y + shift.y;
            if frame != f.last
                && let Some(image) = frames.get(frame.0).and_then(|c| c.get(frame.1))
            {
                sprite.image = image.clone();
            }
        }
        f.last = frame;
    }
}
