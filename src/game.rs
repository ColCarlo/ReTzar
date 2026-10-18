//! A game opened for play: the scenario read or the random map generated,
//! then its world prepared and the simulation started as the original's
//! loader does. The window and the headless tools share these steps, so
//! both run the same game.
use crate::{
    Result,
    assets::GameAssets,
    bitmap::Bitmap,
    hpfs::Container,
    map::MapGrid,
    menu::Selection,
    random_map,
    scenario::Scenario,
    simulation::{self, Simulation},
    terrain::{self, Climate},
    world::{GameSetup, MissionWorld, ObjectLibrary},
};
use std::collections::HashMap;

/// A map read or generated, before its world is prepared.
pub struct Opened {
    pub grid: MapGrid,
    pub mission: MissionWorld,
    pub biome: String,
    pub title: String,
    pub description: String,
    /// The scenario's archive, for its portraits and the copy of its entries
    /// that saves keep; none for a random map.
    pub container: Option<Container>,
}

/// Read the scenario of `selection`, or generate its random map.
pub fn open(
    assets: &GameAssets,
    selection: &Selection,
    scenario: Option<Scenario>,
    setup: Option<&GameSetup>,
) -> Result<Opened> {
    match (selection, scenario) {
        (Selection::Scenario { .. }, Some(m)) => {
            let c = m.container()?;
            let grid = MapGrid::read(&c, &m.header)?;
            let mut mission = MissionWorld::read(&c, &grid)?;
            mission.correct_map_errors(&grid.name);
            let (no_fog, explored) = (mission.game_rules.no_fog, mission.game_rules.explored);
            if let Some(setup) = setup {
                mission.apply_setup(setup);
            }
            // A campaign map keeps the fog and the exploration of its own
            // header (0x441b93; `LoadMap` puts both back on, 0x424ba4),
            // whatever the setup carried from earlier maps or saves.
            if matches!(
                selection,
                Selection::Scenario {
                    campaign: Some(_),
                    ..
                }
            ) {
                mission.game_rules.no_fog = no_fog;
                mission.game_rules.explored = explored;
            }
            Ok(Opened {
                biome: terrain::biome_of(&c.get(&m.header)?)?,
                grid,
                mission,
                title: m.title,
                description: m.description,
                container: Some(c),
            })
        }
        (Selection::Random { side, seed }, _) => {
            let generated = random_map::generate(assets, *side, *seed, setup)?;
            Ok(Opened {
                description: crate::lang::fill(
                    "{} · {}×{} · seme {}",
                    &[
                        &crate::lang::t(random_map::TYPES[generated.kind]),
                        side,
                        side,
                        seed,
                    ],
                ),
                grid: generated.grid,
                mission: generated.mission,
                biome: generated.biome,
                title: crate::lang::fill("Mappa casuale {}", &[seed]),
                container: None,
            })
        }
        (Selection::Scenario { .. }, None) => Err("Missione non trovata".into()),
    }
}

/// Prepare the world of an opened map and start its simulation in
/// `mission.simulation`.
pub fn start(
    mission: &mut MissionWorld,
    grid: &MapGrid,
    library: &ObjectLibrary,
    biome: &str,
    selection: &Selection,
    setup: Option<&GameSetup>,
    climate: Climate,
) -> Result<()> {
    mission.campaign = matches!(
        selection,
        Selection::Scenario {
            campaign: Some(_),
            ..
        }
    );
    if mission.campaign {
        mission.game_rules.start_campaign();
    }
    if setup.is_some_and(|s| s.custom_start)
        && matches!(selection, Selection::Scenario { campaign: None, .. })
    {
        mission.reset_initial_forces(grid, library)?;
        // 0x440aa4: without the map's details the scenario's conversations,
        // actions, events and rules are not read (0x4b70b0).
        let defeat = std::mem::take(&mut mission.script.defeat);
        mission.script = Default::default();
        mission.script.defeat = defeat;
        mission.conversations.clear();
    }
    if matches!(selection, Selection::Scenario { .. }) {
        mission.apply_loader_rules();
    }
    if setup.is_none_or(|s| s.human.is_none()) {
        mission.apply_local_ai_rule();
    }
    for o in &mut mission.objects {
        if crate::selection::unit(o) || o.building() {
            let base = library.attributes.get(o).health;
            if o.health == 0 || o.flags & 0x40 != 0 {
                o.health = base;
            }
            if o.max_health == 0 || o.flags & 0x80 != 0 {
                o.max_health = base;
            }
        }
    }
    simulation::initial_health(mission);
    mission.refresh_walls(library, grid.side);
    let mut sim = Simulation::new(mission, library, grid.side, biome);
    sim.clouds = climate.clouds;
    sim.rain_allowed = climate.rain;
    sim.snow_allowed = climate.snow;
    // 0x44092d: owner 9's units wait to be freed (program Rescue).
    crate::gameplay::hold_prisoners(&mut sim, mission, library);
    mission.simulation = Some(sim);
    Ok(())
}

/// Decode the portraits the interface may show: heroes (48×48) and
/// objectives (96×96), 16-bit bitmaps stored in the scenario. A missing or
/// damaged picture only leaves its frame empty.
pub fn portraits(c: &Container, mission: &MissionWorld) -> HashMap<String, Bitmap> {
    use crate::script::Action;
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
                .filter(|o| o.class == crate::world::class::ITEM_SCROLL)
                .filter_map(|o| o.data.get(0x460..0x4a0))
                .map(|b| crate::assets::legacy_text(b.split(|c| *c == 0).next().unwrap_or(&[]))),
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
