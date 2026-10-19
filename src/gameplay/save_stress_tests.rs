//! Stress of the saves: a game written and read back through the file
//! format of the save window (`save::encode`, `save::decode`) at awkward
//! moments goes on as the game never saved does, second by second, to the
//! last byte of its state.

use super::*;

/// The whole state a game goes on from, as the arena hashes it.
pub(crate) fn state(g: &MapGrid, m: &MissionWorld, sim: &Simulation) -> Vec<u8> {
    serde_json::to_vec(&(g, m, &m.starting_positions, sim)).expect("stato serializzabile")
}

/// The game saved as the save window saves it and loaded back as the load
/// window does.
pub(crate) fn round_trip(
    g: &MapGrid,
    m: &MissionWorld,
    sim: &Simulation,
    selection: crate::menu::Selection,
) -> std::result::Result<(MapGrid, MissionWorld, Simulation), String> {
    let meta = crate::menu::SavedSession {
        version: crate::menu::SAVE_VERSION,
        snapshot: true,
        title: "Prova".into(),
        name: "Prova".into(),
        file: "prova".into(),
        scenario: None,
        selection,
        camera: [0., 0., 1.],
        setup: None,
        missing: false,
    };
    let snapshot = crate::save::Snapshot {
        language: crate::lang::current().index(),
        schema: 1,
        grid: g.clone(),
        biome: String::new(),
        title: String::new(),
        description: String::new(),
        starting_positions: m.starting_positions,
        mission: m.clone(),
        simulation: sim.clone(),
        source_entries: Default::default(),
        external_entries: Default::default(),
        portraits: Default::default(),
        hud: Default::default(),
        groups: Default::default(),
        health_kept: false,
        idle: 0,
        visual_time: Duration::ZERO,
        flyers: Vec::new(),
    };
    let bytes = crate::save::encode(&meta, &snapshot).map_err(|e| e.to_string())?;
    let (_, back) = crate::save::decode(&bytes).map_err(|e| e.to_string())?;
    let mut mission = back.mission;
    mission.starting_positions = back.starting_positions;
    Ok((back.grid, mission, back.simulation))
}

/// The first places where two states differ, with both values.
pub(crate) fn differences(a: &[u8], b: &[u8], limit: usize) -> Vec<String> {
    fn walk(
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
        match (a, b) {
            (Value::Object(x), Value::Object(y)) => {
                for (k, v) in x {
                    let len = path.len();
                    path.push('.');
                    path.push_str(k);
                    walk(v, y.get(k).unwrap_or(&Value::Null), path, out, limit);
                    path.truncate(len);
                }
                for k in y.keys().filter(|k| !x.contains_key(*k)) {
                    out.push(format!("{path}.{k}: assente ≠ presente"));
                }
            }
            (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
                for (k, (v, w)) in x.iter().zip(y).enumerate() {
                    let len = path.len();
                    path.push_str(&format!("[{k}]"));
                    walk(v, w, path, out, limit);
                    path.truncate(len);
                }
            }
            _ => {
                let short = |v: &Value| v.to_string().chars().take(120).collect::<String>();
                out.push(format!("{path}: {} ≠ {}", short(a), short(b)));
            }
        }
    }
    let (Ok(a), Ok(b)) = (
        serde_json::from_slice::<serde_json::Value>(a),
        serde_json::from_slice::<serde_json::Value>(b),
    ) else {
        return vec!["stato illeggibile".into()];
    };
    let mut out = Vec::new();
    walk(&a, &b, &mut String::from("stato"), &mut out, limit);
    out
}

struct Game {
    c: std::rc::Rc<Catalog>,
    lib: std::rc::Rc<ObjectLibrary>,
    m: MissionWorld,
    g: MapGrid,
    sim: Simulation,
}
impl Game {
    fn second(&mut self) {
        for _ in 0..1000 / TICK_MS {
            advance(
                &mut self.sim,
                &mut self.m,
                &mut self.g,
                &self.lib,
                &self.c,
                Duration::from_millis(TICK_MS),
                10,
                false,
            );
        }
    }
    fn state(&self) -> Vec<u8> {
        state(&self.g, &self.m, &self.sim)
    }
}

/// A random map of eight kingdoms played by the computer in full mode, the
/// human's too, as the arena plays them.
fn battle(side: usize, seed: u64) -> Game {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let assets = crate::assets::GameAssets::open(root).unwrap();
    let selection = crate::menu::Selection::Random { side, seed };
    let setup = crate::world::GameSetup {
        players: Some(8),
        human: Some(0),
        ..Default::default()
    };
    let opened = crate::game::open(&assets, &selection, None, Some(&setup)).unwrap();
    let lib = ObjectLibrary::load(&assets, &opened.biome).unwrap();
    let climate = crate::terrain::Climate::load(&assets, &opened.biome).unwrap();
    let mut m = opened.mission;
    crate::game::start(
        &mut m,
        &opened.grid,
        &lib,
        &opened.biome,
        &selection,
        Some(&setup),
        climate,
    )
    .unwrap();
    let mut sim = m.simulation.take().unwrap();
    ai::command(&mut sim, &m, m.player(), "full");
    Game {
        c: std::rc::Rc::new(Catalog::load(&assets).unwrap()),
        lib: std::rc::Rc::new(lib),
        m,
        g: opened.grid,
        sim,
    }
}

/// What was going on in the game: the reasons a save there is awkward.
fn moment(game: &Game) -> String {
    let s = &game.sim;
    let held = s.objects.iter().filter(|o| o.holder.is_some()).count();
    let flying = s.gameplay.projectiles.len();
    let attacking = s
        .gameplay
        .actors
        .iter()
        .filter(|a| matches!(a.intent, Some(Intent::Attack(_))))
        .count();
    let building = game
        .m
        .objects
        .iter()
        .filter(|o| o.building() && o.flags & FLAG_CONSTRUCTION != 0 && alive(o))
        .count();
    format!(
        "{} oggetti, {attacking} all'attacco, {flying} proiettili in volo, {held} dentro altri, {building} cantieri",
        game.m.objects.iter().filter(|o| alive(o)).count()
    )
}

/// Plays `minutes` of an all-computer game, saving and loading every
/// `every` seconds: both copies then play `span` seconds side by side.
fn saves_in_battle(side: usize, seed: u64, minutes: u64, every: u64, span: u64) -> Vec<String> {
    let mut a = battle(side, seed);
    let mut faults = Vec::new();
    let selection = crate::menu::Selection::Random { side, seed };
    let mut second = 0;
    while second < minutes * 60 {
        a.second();
        second += 1;
        if second % every != 0 {
            continue;
        }
        let (g, m, sim) = match round_trip(&a.g, &a.m, &a.sim, selection.clone()) {
            Ok(x) => x,
            Err(e) => {
                faults.push(format!(
                    "{side}/{seed} a {second} s: salvataggio rifiutato: {e}"
                ));
                continue;
            }
        };
        let mut b = Game {
            c: a.c.clone(),
            lib: a.lib.clone(),
            m,
            g,
            sim,
        };
        let what = moment(&a);
        for k in 1..=span {
            a.second();
            b.second();
            second += 1;
            let (x, y) = (a.state(), b.state());
            if x != y {
                faults.push(format!(
                    "{side}/{seed}: salvata a {} s ({what}), diversa dopo {k} s:\n  {}",
                    second - k,
                    differences(&x, &y, 12).join("\n  ")
                ));
                break;
            }
        }
        eprintln!("SALVATAGGIO {side}/{seed} a {} s: {what}", second - span);
    }
    faults
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_battle_saved_at_any_moment_goes_on_as_it_was() {
    let mut faults = Vec::new();
    for (side, seed) in [(128, 11), (128, 12), (256, 13)] {
        faults.extend(saves_in_battle(side, seed, 24, 120, 30));
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}

#[test]
#[ignore = "Requires installed original archives"]
fn footprints_and_sizes_come_back_from_a_save() {
    let (c, lib, mut m, mut g, mut sim) = super::tests::setup(1);
    advance(
        &mut sim,
        &mut m,
        &mut g,
        &lib,
        &c,
        Duration::from_millis(TICK_MS),
        10,
        false,
    );
    assert!(!sim.footprints.is_empty());
    let selection = crate::menu::Selection::Random {
        side: g.side,
        seed: 1,
    };
    let (_, _, back) = round_trip(&g, &m, &sim, selection).unwrap();
    // A large building tested by its corner alone in the first step after
    // loading looked hidden: the units fighting it under the fog turned
    // on whatever else they saw.
    assert_eq!(back.footprints, sim.footprints);
    assert!(
        sim.objects
            .iter()
            .zip(&back.objects)
            .all(|(a, b)| a.size == b.size)
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_game_saves_with_a_shot_at_the_ground_in_the_air() {
    let (c, lib, mut m, mut g, mut sim) = super::tests::setup(1);
    let at = [m.objects[1].x + 4, m.objects[1].y + 4];
    let end = [at[0] as i32 * 32 + 16, at[1] as i32 * 32 + 16];
    let lands = sim.ticks + 40;
    push_projectile(&mut sim, 1, usize::MAX, 40, at, missile::FIRE, lands, end);
    let selection = crate::menu::Selection::Random {
        side: g.side,
        seed: 1,
    };
    // A rain of fire in the air refused every save (the target of a shot
    // at the ground is none).
    let (g2, m2, sim2) = round_trip(&g, &m, &sim, selection).unwrap();
    let mut b = (g2, m2, sim2);
    for _ in 0..60 {
        advance(
            &mut sim,
            &mut m,
            &mut g,
            &lib,
            &c,
            Duration::from_millis(TICK_MS),
            10,
            false,
        );
        advance(
            &mut b.2,
            &mut b.1,
            &mut b.0,
            &lib,
            &c,
            Duration::from_millis(TICK_MS),
            10,
            false,
        );
        assert!(state(&g, &m, &sim) == state(&b.0, &b.1, &b.2));
    }
}
