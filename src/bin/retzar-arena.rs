//! retzar-arena: headless games run side by side, for testing the
//! simulation and, later, the computer players. A debug tool built only with
//! the `arena` feature: it is not part of the game.
//!
//! cargo build --release --no-default-features --features arena --bin retzar-arena
use retzar::{
    Result,
    assets::GameAssets,
    game,
    gameplay::{self, Catalog},
    map::MapGrid,
    menu::Selection,
    scenario::ScenarioCatalog,
    simulation::Simulation,
    terrain::Climate,
    world::{GameSetup, MissionWorld, ObjectLibrary},
};
use serde::Serialize;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const HELP: &str = "\
retzar-arena: partite senza grafica, in parallelo (strumento di prova, non fa parte del gioco)

Uso: retzar-arena [opzioni]
  --random LATO:SEME[-SEME]   mappe casuali (un seme o un intervallo), es. 128:1-16
  --players N                 regni delle mappe casuali (2–8, predefinito 4)
  --resign                    mappe casuali con la «Resa dell'IA» attiva
  --assistant                 il regno del giocatore ha un'IA completa (comando «AI full»), anche negli scenari
  --difficulty D              difficoltà delle mappe casuali: 0 facile, 1 normale, 2 difficile
  --scenario ID[,ID…]         scenari dell'installazione (vedi retzar-tools --list)
  --trace                     scrive per ogni partita le regole, le azioni e le finestre
                              del copione (traccia-*.txt)
  --custom-start              scenari senza «Dettagli predefiniti»: castello e tre
                              contadini per regno, il primo regno umano
  --minutes M                 minuti di gioco per partita (predefinito 10)
  --jobs J                    partite in parallelo (predefinito: tutti i thread)
  --verify                    ripete ogni partita salvando e ricaricando a metà:
                              confronta le impronte minuto per minuto
  --media                     conta i suoni, gli effetti, i proiettili e le animazioni
                              che le partite chiedono (media.txt) e segnala quelli
                              che i dati non risolvono
  --out CARTELLA              dove scrivere i risultati (predefinito reports/arena/<ora>)
";

#[derive(Clone, Debug)]
enum Map {
    Random {
        side: usize,
        seed: u64,
        players: usize,
        resign: bool,
        assistant: bool,
        difficulty: Option<usize>,
    },
    /// The id, whether the player's kingdom gets a computer player, and
    /// whether the map's details are left out (castle and three peasants).
    Scenario(String, bool, bool),
}
impl Map {
    fn label(&self) -> String {
        match self {
            Map::Random {
                side,
                seed,
                players,
                resign,
                assistant,
                difficulty,
            } => format!(
                "casuale {side}:{seed} ({players} regni{}{}{})",
                if *resign { ", resa" } else { "" },
                if *assistant { ", assistente" } else { "" },
                difficulty.map_or(String::new(), |d| format!(", difficoltà {d}"))
            ),
            Map::Scenario(id, assistant, custom) => format!(
                "scenario {id}{}{}",
                if *custom { ", senza dettagli" } else { "" },
                if *assistant { ", assistente" } else { "" }
            ),
        }
    }
}

#[derive(Serialize)]
struct Kingdom {
    kingdom: u8,
    resources: [u32; 4],
    units: usize,
    peasants: usize,
    /// Combat units (flag 0x10000).
    military: usize,
    buildings: usize,
    score: u64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    resigned: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    defeated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    ai: Option<AiView>,
}
/// What a computer player keeps of its harvest.
#[derive(Serialize)]
struct AiView {
    /// Gatherers wanted of gold, stone, food and wood, and the resources
    /// it may not gather.
    wanted: [u16; 4],
    blocked: [bool; 4],
    /// Peasants by group: idle, builders, milkers, gold, stone, food, wood.
    groups: [usize; 7],
    /// The wood its woodcutters go to, and «no wood on the map».
    wood: Option<[i32; 2]>,
    no_wood: bool,
    available: [i32; 4],
    /// Its units by native program, the idle ones left out.
    programs: std::collections::BTreeMap<u8, u16>,
}
#[derive(Serialize)]
struct Minute {
    minute: u64,
    hash: String,
    kingdoms: Vec<Kingdom>,
}
#[derive(Serialize, Default)]
struct Report {
    map: String,
    minutes: u64,
    ticks: u64,
    seconds: f64,
    objects: usize,
    /// The computer players at the end: kingdom, script, mode and
    /// civilization.
    players: Vec<(u8, String, u16, u8)>,
    series: Vec<Minute>,
    /// With --verify: the first minute whose state differs in the repeated
    /// game, before the restore (not deterministic) or after it.
    divergence: Option<String>,
    error: Option<String>,
    /// How the game ended for the local kingdom: won, and when (ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    over: Option<(bool, u64)>,
}

/// The folder of the script traces, when `--trace` asks for them.
static TRACE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
/// What the games asked of sounds and pictures, when `--media` asks for it.
static MEDIA: std::sync::OnceLock<Mutex<std::collections::BTreeMap<String, u64>>> =
    std::sync::OnceLock::new();

/// The sounds, effects, missiles and action pictures of the second just
/// played, each with whether the data resolves it («MUTO»: a sound with
/// nothing to play; «MANCANTE»: a picture the data does not have).
fn media(w: &World) {
    let Some(media) = MEDIA.get() else { return };
    let lib = &w.library;
    let mut seen = Vec::new();
    for event in &w.sim.sounds {
        let line = match event.property.strip_prefix('@') {
            Some(group) => {
                let silent = lib.sound_group(group).is_empty();
                format!("suono @{group}{}", if silent { " MUTO" } else { "" })
            }
            None => {
                let Some(o) = w.mission.objects.get(event.object) else {
                    seen.push(format!("suono {} senza oggetto", event.property));
                    continue;
                };
                let value = lib.property(o, event.property).map(str::trim);
                let state = match value {
                    None | Some("") => " (nessuno nei dati)",
                    Some(_) if lib.sounds_for(o, event.property).is_empty() => " MUTO",
                    Some(_) => "",
                };
                format!("suono {} {}{state}", event.property, o.class_name())
            }
        };
        seen.push(line);
    }
    for f in &w.sim.flashes {
        let missing = lib.images.get(f.image, "image").is_none();
        seen.push(format!(
            "effetto {}{}",
            f.image,
            if missing { " MANCANTE" } else { "" }
        ));
    }
    if !w.sim.bolts.is_empty() {
        seen.push("effetto fulmine".into());
    }
    for p in &w.sim.gameplay.projectiles {
        if p.launched + 20 > w.sim.ticks {
            let shooter = w
                .mission
                .objects
                .get(p.attacker)
                .map_or("?", |o| o.class_name());
            seen.push(format!("proiettile {} di {shooter}", p.kind));
        }
    }
    for (o, a) in w.mission.objects.iter().zip(&w.sim.gameplay.actors) {
        if a.action_image.is_empty() {
            continue;
        }
        // A picture the record lacks is its base `image` (0x4e5110).
        let has = |key: &str| {
            lib.property(o, key)
                .is_some_and(|s| lib.images.get(s.trim(), "image").is_some())
        };
        let state = if has(&a.action_image) {
            ""
        } else if has("image") {
            " (immagine base)"
        } else {
            " MANCANTE"
        };
        seen.push(format!(
            "animazione {} {}{state}",
            o.class_name(),
            a.action_image
        ));
    }
    let mut counts = media.lock().unwrap();
    for line in seen {
        *counts.entry(line).or_default() += 1;
    }
}

/// A world ready to run, as the game prepares it.
struct World {
    grid: MapGrid,
    mission: MissionWorld,
    library: ObjectLibrary,
    sim: Simulation,
}

fn open(assets: &GameAssets, catalog: &ScenarioCatalog, map: &Map) -> Result<World> {
    let (selection, scenario, setup) = match map {
        Map::Random {
            side,
            seed,
            players,
            resign,
            ..
        } => (
            Selection::Random {
                side: *side,
                seed: *seed,
            },
            None,
            Some(GameSetup {
                players: Some(*players),
                human: Some(0),
                ai_resign: *resign,
                ..Default::default()
            }),
        ),
        Map::Scenario(id, _, custom) => {
            let (campaign, mission) = catalog
                .locate(&id.to_ascii_lowercase())
                .ok_or_else(|| format!("Scenario sconosciuto: {id}"))?;
            let scenario = match campaign {
                Some(c) => catalog.campaigns[c].missions[mission].clone(),
                None => catalog.missions[mission].clone(),
            };
            // As the setup screen with «Dettagli predefiniti» off: the
            // first kingdom is the human, the others computers in full mode.
            let setup = custom.then(|| GameSetup {
                custom_start: true,
                human: Some(0),
                ..Default::default()
            });
            (
                Selection::Scenario { campaign, mission },
                Some(scenario),
                setup,
            )
        }
    };
    let opened = game::open(assets, &selection, scenario, setup.as_ref())?;
    let library = ObjectLibrary::load(assets, &opened.biome)?;
    let climate = Climate::load(assets, &opened.biome)?;
    let mut mission = opened.mission;
    game::start(
        &mut mission,
        &opened.grid,
        &library,
        &opened.biome,
        &selection,
        setup.as_ref(),
        climate,
    )?;
    if let Map::Random {
        difficulty: Some(d),
        ..
    } = map
    {
        mission.game_rules.difficulty = *d;
    }
    let mut sim = mission.simulation.take().ok_or("Simulazione non avviata")?;
    if matches!(
        map,
        Map::Random {
            assistant: true,
            ..
        } | Map::Scenario(_, true, _)
    ) {
        gameplay::ai::command(&mut sim, &mission, mission.player(), "full");
    }
    Ok(World {
        grid: opened.grid,
        mission,
        library,
        sim,
    })
}

/// One second of game time; the windows the scripts open are closed at once
/// (stories and messages read, the first answer of a conversation).
fn second(w: &mut World, c: &Catalog) {
    let ms = w.sim.ticks * 50;
    let tracing = w.sim.gameplay.script.trace.0.is_some();
    if tracing {
        // What the script showed, and the windows the arena closes.
        let shown: Vec<String> = w
            .sim
            .gameplay
            .script
            .outbox
            .0
            .iter()
            .filter(|s| !matches!(s, gameplay::Show::Unselect(_)))
            .map(|s| format!("    mostra {s:?}"))
            .collect();
        if let Some(log) = &mut w.sim.gameplay.script.trace.0 {
            log.extend(shown.into_iter().map(|s| (ms, s)));
        }
    }
    if w.sim.gameplay.script.modal {
        let message = w.mission.message.as_ref().map(|m| {
            let text: String = m.text.chars().take(160).collect();
            format!("{}: {text} {:?}", m.title, m.choices)
        });
        if let (Some(log), Some(message)) = (&mut w.sim.gameplay.script.trace.0, message) {
            log.push((ms, format!("    finestra {message}")));
        }
        if w.sim.gameplay.script.conversation.is_some() {
            gameplay::answer(&mut w.sim, &mut w.mission, &w.grid, &w.library, c, 0);
        } else {
            gameplay::close_window(&mut w.sim, &mut w.mission);
        }
    }
    w.sim.gameplay.script.outbox.0.clear();
    gameplay::advance(
        &mut w.sim,
        &mut w.mission,
        &mut w.grid,
        &w.library,
        c,
        Duration::from_secs(1),
        10,
        true,
    );
    media(w);
    w.sim.sounds.clear();
    w.sim.flashes.clear();
    w.sim.bolts.clear();
}

/// The live state as a save keeps it: map, world and simulation.
fn state(w: &World) -> Vec<u8> {
    serde_json::to_vec(&(&w.grid, &w.mission, &w.mission.starting_positions, &w.sim))
        .expect("serializzazione dello stato")
}
fn restore(w: &mut World) -> Result<()> {
    let bytes = state(w);
    let (grid, mut mission, starting, sim): (
        MapGrid,
        MissionWorld,
        [Option<[u16; 2]>; 8],
        Simulation,
    ) = serde_json::from_slice(&bytes)?;
    mission.starting_positions = starting;
    w.grid = grid;
    w.mission = mission;
    w.sim = sim;
    Ok(())
}

fn sample(w: &World, minute: u64) -> Minute {
    let m = &w.mission;
    let kingdoms = (1..=m.kingdoms.len() as u8)
        .filter(|&k| m.kingdoms[k as usize - 1].active)
        .map(|k| {
            let alive = |o: &&retzar::world::MapObject| o.owner == k && o.health > 0;
            let standing = w.sim.gameplay.script.kingdoms.get(k as usize);
            let units = m
                .objects
                .iter()
                .filter(alive)
                .filter(|o| retzar::selection::unit(o));
            Kingdom {
                kingdom: k,
                resources: m.kingdoms[k as usize - 1].resources,
                units: units.clone().count(),
                peasants: units
                    .clone()
                    .filter(|o| o.class == retzar::world::class::PEON)
                    .count(),
                military: units.filter(|o| retzar::selection::combat(o)).count(),
                buildings: m
                    .objects
                    .iter()
                    .filter(alive)
                    .filter(|o| o.building())
                    .count(),
                score: w.sim.gameplay.score.get(&k).copied().unwrap_or(0),
                resigned: standing.is_some_and(|s| s.resigned),
                defeated: standing.is_some_and(|s| s.defeated),
                ai: w.sim.gameplay.ai.players.get(&k).map(|p| AiView {
                    wanted: p.wanted,
                    blocked: p.blocked,
                    groups: std::array::from_fn(|g| p.lists.groups[g].len()),
                    wood: p.forest.wood,
                    no_wood: p.forest.none,
                    available: p.available,
                    programs: {
                        let mut n = std::collections::BTreeMap::new();
                        for (i, o) in m.objects.iter().enumerate() {
                            if o.owner == k
                                && o.health > 0
                                && retzar::selection::unit(o)
                                && i < w.sim.gameplay.actors.len()
                            {
                                let kind = gameplay::ai::program(&w.sim, m, i).kind;
                                if kind != gameplay::ai::program::IDLE {
                                    *n.entry(kind).or_insert(0u16) += 1;
                                }
                            }
                        }
                        n
                    },
                }),
            }
        })
        .collect();
    Minute {
        minute,
        hash: blake3::hash(&state(w)).to_hex()[..16].to_owned(),
        kingdoms,
    }
}

/// Run a game for `minutes`, sampling it every minute.
fn run(
    assets: &GameAssets,
    catalog: &ScenarioCatalog,
    c: &Catalog,
    map: &Map,
    minutes: u64,
) -> Result<Report> {
    let mut w = open(assets, catalog, map)?;
    if TRACE.get().is_some() {
        w.sim.gameplay.script.trace.0 = Some(Vec::new());
    }
    let start = Instant::now();
    let mut report = Report {
        map: map.label(),
        objects: w.mission.objects.len(),
        ..Default::default()
    };
    report.series.push(sample(&w, 0));
    for minute in 1..=minutes {
        for _ in 0..60 {
            second(&mut w, c);
        }
        report.minutes = minute;
        report.series.push(sample(&w, minute));
        if w.sim.gameplay.script.over.is_some() {
            break;
        }
    }
    report.ticks = w.sim.ticks;
    report.over = w.sim.gameplay.script.over;
    report.players = w
        .sim
        .gameplay
        .ai
        .players
        .iter()
        .map(|(&k, p)| (k, p.script.clone(), p.mode, p.civilization))
        .collect();
    report.seconds = start.elapsed().as_secs_f64();
    if let (Some(dir), Some(log)) = (TRACE.get(), &w.sim.gameplay.script.trace.0) {
        let name: String = map
            .label()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let mut text = String::new();
        for (ms, line) in log {
            text.push_str(&format!(
                "{:>4}:{:02} {line}\n",
                ms / 60_000,
                ms / 1000 % 60
            ));
        }
        let quests: Vec<&str> = w.mission.quests.iter().map(|q| q.text.as_str()).collect();
        text.push_str(&format!(
            "fine: {:?}; obiettivi aperti: {quests:?}\n",
            report.over
        ));
        std::fs::write(dir.join(format!("traccia-{name}.txt")), text)?;
    }
    Ok(report)
}

/// Where two JSON values differ: up to `limit` paths with both values.
fn differences(
    a: &serde_json::Value,
    b: &serde_json::Value,
    path: &mut String,
    out: &mut Vec<String>,
    limit: usize,
) {
    use serde_json::Value;
    if out.len() >= limit || a == b {
        return;
    }
    let mut descend = |key: String, x: &Value, y: &Value, out: &mut Vec<String>| {
        let len = path.len();
        path.push_str(&key);
        differences(x, y, path, out, limit);
        path.truncate(len);
    };
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, v) in x {
                let w = y.get(k).unwrap_or(&Value::Null);
                descend(format!(".{k}"), v, w, out);
            }
            for (k, w) in y {
                if !x.contains_key(k) {
                    descend(format!(".{k}"), &Value::Null, w, out);
                }
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (v, w)) in x.iter().zip(y).enumerate() {
                descend(format!("[{i}]"), v, w, out);
            }
        }
        _ => {
            let short = |v: &Value| {
                let s = v.to_string();
                if s.len() > 160 {
                    format!("{}…", &s[..160])
                } else {
                    s
                }
            };
            out.push(format!("{path}: {} ≠ {}", short(a), short(b)));
        }
    }
}

/// The game run twice side by side, the second saved and reloaded halfway:
/// both are compared every second of game time; the first difference is
/// described in `out`.
fn lockstep(
    assets: &GameAssets,
    catalog: &ScenarioCatalog,
    c: &Catalog,
    map: &Map,
    minutes: u64,
    out: &Path,
) -> Result<Option<String>> {
    let mut a = open(assets, catalog, map)?;
    let mut b = open(assets, catalog, map)?;
    let half = (minutes / 2).max(1) * 60;
    for s in 1..=minutes * 60 {
        second(&mut a, c);
        second(&mut b, c);
        if s == half {
            restore(&mut b)?;
        }
        let (x, y) = (state(&a), state(&b));
        if x != y {
            let what = if s <= half {
                format!("non deterministica dal secondo {s}")
            } else {
                format!("diversa dopo il ripristino (secondo {half}), dal secondo {s}")
            };
            let (x, y): (serde_json::Value, serde_json::Value) =
                (serde_json::from_slice(&x)?, serde_json::from_slice(&y)?);
            let mut found = Vec::new();
            differences(&x, &y, &mut String::from("stato"), &mut found, 60);
            let name: String = map
                .label()
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
                .collect();
            std::fs::write(
                out.join(format!("divergenza-{name}.txt")),
                format!(
                    "{} — {what}\n(originale ≠ ripristinata)\n{}\n",
                    map.label(),
                    found.join("\n")
                ),
            )?;
            return Ok(Some(what));
        }
        if a.sim.gameplay.script.over.is_some() {
            break;
        }
    }
    Ok(None)
}

fn game(
    assets: &GameAssets,
    catalog: &ScenarioCatalog,
    c: &Catalog,
    map: &Map,
    minutes: u64,
    verify: Option<&Path>,
) -> Report {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Report> {
        let mut report = run(assets, catalog, c, map, minutes)?;
        if let Some(out) = verify {
            report.divergence = lockstep(assets, catalog, c, map, minutes, out)?;
        }
        Ok(report)
    }));
    match outcome {
        Ok(Ok(report)) => report,
        Ok(Err(e)) => Report {
            map: map.label(),
            error: Some(e.to_string()),
            ..Default::default()
        },
        Err(panic) => Report {
            map: map.label(),
            error: Some(format!(
                "panico: {}",
                panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("?")
            )),
            ..Default::default()
        },
    }
}

fn value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.windows(2).find(|a| a[0] == key).map(|a| a[1].as_str())
}

fn maps(args: &[String]) -> Result<Vec<Map>> {
    let mut out = Vec::new();
    let players: usize = value(args, "--players").map_or(Ok(4), str::parse)?;
    let resign = args.iter().any(|a| a == "--resign");
    let assistant = args.iter().any(|a| a == "--assistant");
    let custom = args.iter().any(|a| a == "--custom-start");
    let difficulty = value(args, "--difficulty").map(str::parse).transpose()?;
    if let Some(r) = value(args, "--random") {
        let (side, seeds) = r.split_once(':').ok_or("Usa --random LATO:SEME[-SEME]")?;
        let side: usize = side.parse()?;
        let (a, b) = match seeds.split_once('-') {
            Some((a, b)) => (a.parse()?, b.parse()?),
            None => {
                let s: u64 = seeds.parse()?;
                (s, s)
            }
        };
        for seed in a..=b {
            out.push(Map::Random {
                side,
                seed,
                players,
                resign,
                assistant,
                difficulty,
            });
        }
    }
    if let Some(ids) = value(args, "--scenario") {
        out.extend(
            ids.split(',')
                .map(|id| Map::Scenario(id.trim().into(), assistant, custom)),
        );
    }
    if out.is_empty() {
        return Err("Indica --random o --scenario (vedi --help)".into());
    }
    Ok(out)
}

fn root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.ancestors()
                .find(|d| d.join("Tzar.exe").is_file())
                .map(Path::to_path_buf)
        })
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .into()
        })
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    let maps = maps(&args)?;
    let minutes: u64 = value(&args, "--minutes").map_or(Ok(10), str::parse)?;
    let verify = args.iter().any(|a| a == "--verify");
    let jobs: usize = match value(&args, "--jobs") {
        Some(j) => j.parse()?,
        None => std::thread::available_parallelism().map_or(1, |n| n.get()),
    };
    let source = root();
    let out = match value(&args, "--out") {
        Some(dir) => PathBuf::from(dir),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("reports/arena")
            .join(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs()
                    .to_string(),
            ),
    };
    std::fs::create_dir_all(&out)?;
    if args.iter().any(|a| a == "--trace") {
        let _ = TRACE.set(out.clone());
    }
    if args.iter().any(|a| a == "--media") {
        let _ = MEDIA.set(Mutex::new(Default::default()));
    }
    let assets = Arc::new(GameAssets::open(&source)?);
    let catalog = Arc::new(ScenarioCatalog::load(&source)?);
    println!(
        "{} partite da {minutes} minuti, {jobs} alla volta{} → {}",
        maps.len(),
        if verify { ", con verifica" } else { "" },
        out.display()
    );
    let next = Arc::new(AtomicUsize::new(0));
    let maps = Arc::new(maps);
    let results = Arc::new(Mutex::new(Vec::new()));
    let file = Arc::new(Mutex::new(std::fs::File::create(
        out.join("partite.jsonl"),
    )?));
    let wall = Instant::now();
    let workers: Vec<_> = (0..jobs.min(maps.len()))
        .map(|_| {
            let (assets, catalog, next, maps, results, file, dir) = (
                assets.clone(),
                catalog.clone(),
                next.clone(),
                maps.clone(),
                results.clone(),
                file.clone(),
                out.clone(),
            );
            std::thread::spawn(move || {
                let c = Catalog::load(&assets).expect("catalogo del gioco");
                loop {
                    let k = next.fetch_add(1, Ordering::SeqCst);
                    let Some(map) = maps.get(k) else { break };
                    let report = game(
                        &assets,
                        &catalog,
                        &c,
                        map,
                        minutes,
                        verify.then_some(dir.as_path()),
                    );
                    let line = serde_json::to_string(&report).expect("rapporto");
                    writeln!(file.lock().unwrap(), "{line}").expect("scrittura del rapporto");
                    let status = match (&report.error, &report.divergence) {
                        (Some(e), _) => format!("ERRORE {e}"),
                        (None, Some(d)) => format!("DIVERGENZA {d}"),
                        _ => format!(
                            "ok, {} min in {:.1} s ({:.0}× il tempo reale)",
                            report.minutes,
                            report.seconds,
                            report.ticks as f64 * 0.05 / report.seconds.max(1e-9)
                        ),
                    };
                    println!("[{}/{}] {}: {status}", k + 1, maps.len(), report.map);
                    results.lock().unwrap().push(report);
                }
            })
        })
        .collect();
    for w in workers {
        w.join()
            .map_err(|_| "un thread è terminato con un panico")?;
    }
    let results = results.lock().unwrap();
    let failed = results.iter().filter(|r| r.error.is_some()).count();
    let diverged = results.iter().filter(|r| r.divergence.is_some()).count();
    let game_minutes: u64 = results.iter().map(|r| r.minutes).sum();
    let secs = wall.elapsed().as_secs_f64();
    let summary = format!(
        "partite: {}\nerrori: {failed}\ndivergenze: {diverged}\nminuti di gioco: {game_minutes}{}\ntempo reale: {secs:.1} s\nresa: {:.0} minuti di gioco al minuto\n",
        results.len(),
        if verify { " (più le ripetizioni)" } else { "" },
        game_minutes as f64 * if verify { 2.0 } else { 1.0 } / (secs / 60.0)
    );
    std::fs::write(out.join("riepilogo.txt"), &summary)?;
    print!("{summary}");
    if let Some(media) = MEDIA.get() {
        let counts = media.lock().unwrap();
        let text: String = counts
            .iter()
            .map(|(k, n)| format!("{n:>8} {k}\n"))
            .collect();
        std::fs::write(out.join("media.txt"), &text)?;
        let bad: Vec<_> = counts
            .keys()
            .filter(|k| k.contains("MUTO") || k.contains("MANCANTE") || k.contains("senza"))
            .collect();
        println!("media: {} voci, {} da guardare", counts.len(), bad.len());
        for k in bad {
            println!("  {k}");
        }
    }
    Ok(())
}
