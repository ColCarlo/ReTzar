//! The computer players' presence, modes and scripts (0x402760, the loader
//! 0x440240, the AI command 0x404460) and the view of the native programs
//! they read. Expectations from the original's AI code and scripts.
use super::tests::setup;
use super::*;
use crate::script::{Action, Named, Rule, Script};

fn kingdom(name: &str, computer: bool, ai_mode: u8, ai_script: &str) -> world::Kingdom {
    world::Kingdom {
        name: name.into(),
        active: true,
        computer,
        ai_mode,
        ai_script: ai_script.into(),
        ..Default::default()
    }
}
fn modes(players: &BTreeMap<u8, ai::Player>) -> Vec<(u8, u16, &str)> {
    players
        .iter()
        .map(|(&k, p)| (k, p.mode, p.script.as_str()))
        .collect()
}

#[test]
fn kingdoms_with_a_mode_get_a_computer_player_at_the_start() {
    let mut m = MissionWorld {
        kingdoms: vec![
            kingdom("Artù", false, 0, ""),
            kingdom("Mordred", true, 2, "DATA\\AI\\War.ais"),
            kingdom("Assente", true, 3, ""),
            kingdom("Senza modalità", true, 0, ""),
        ],
        ..Default::default()
    };
    m.kingdoms[2].active = false;
    let state = ai::State::start(&m);
    assert_eq!(modes(&state.players), [(2, 2, "DATA\\AI\\War.ais")]);
    // Easy keeps the computers off other homes for 30 minutes (0x407140).
    assert_eq!(state.players[&2].peace_until, 0);
    m.game_rules.difficulty = 0;
    assert_eq!(ai::State::start(&m).players[&2].peace_until, 1_800_000);
}

#[test]
fn a_game_set_up_on_a_screen_gives_its_computers_full_players() {
    // 0x4402c3: computer rows mode 3, the others 0, whatever the records
    // said; a row's behaviour replaces the map's script, the default keeps
    // it (0x4ca9c4).
    let mut m = MissionWorld {
        kingdoms: vec![
            kingdom("Uno", false, 3, ""),
            kingdom("Due", false, 0, "DATA\\AI\\Magic.ais"),
            kingdom("Tre", true, 1, "DATA\\AI\\War.ais"),
        ],
        ..Default::default()
    };
    let setup = world::GameSetup {
        human: Some(0),
        kingdoms: vec![world::KingdomChoice {
            record: 2,
            ai: "DATA\\AI\\Religion.ais".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    m.apply_setup(&setup);
    assert_eq!(
        m.kingdoms.iter().map(|k| k.ai_mode).collect::<Vec<_>>(),
        [0, 3, 3]
    );
    assert_eq!(
        modes(&ai::State::start(&m).players),
        [
            (2, 3, "DATA\\AI\\Magic.ais"),
            (3, 3, "DATA\\AI\\Religion.ais")
        ]
    );
}

#[test]
fn the_local_kingdom_keeps_its_record_mode_only_as_a_computer() {
    // 0x440302: the player's kingdom drops the mode of its record.
    let mut m = MissionWorld {
        kingdoms: vec![
            kingdom("Giocatore", false, 3, ""),
            kingdom("Altro", false, 3, ""),
        ],
        ..Default::default()
    };
    m.apply_local_ai_rule();
    assert_eq!(m.kingdoms[0].ai_mode, 0);
    assert_eq!(m.kingdoms[1].ai_mode, 3);
}

#[test]
fn saves_without_computer_players_get_one_per_computer_kingdom() {
    let m = MissionWorld {
        kingdoms: vec![
            kingdom("Uno", false, 0, ""),
            kingdom("Due", true, 0, "DATA\\AI\\War.ais"),
        ],
        ..Default::default()
    };
    let mut old: Runtime = serde_json::from_str("{}").unwrap();
    old.ai.upgrade(&m);
    assert_eq!(modes(&old.ai.players), [(2, 3, "DATA\\AI\\War.ais")]);
    // A save that has them, even none, keeps them.
    let saved = serde_json::to_string(&Runtime::default()).unwrap();
    let mut new: Runtime = serde_json::from_str(&saved).unwrap();
    new.ai.upgrade(&m);
    assert!(new.ai.players.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn campaign_records_choose_the_computer_players_and_their_scripts() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let assets = GameAssets::open(root).unwrap();
    let catalog = crate::scenario::ScenarioCatalog::load(root).unwrap();
    let players = |id: &str| {
        let (campaign, mission) = catalog.locate(id).unwrap();
        let scenario = catalog.campaigns[campaign.unwrap()].missions[mission].clone();
        let selection = crate::menu::Selection::Scenario { campaign, mission };
        let opened = crate::game::open(&assets, &selection, Some(scenario), None).unwrap();
        let lib = ObjectLibrary::load(&assets, &opened.biome).unwrap();
        let mut m = opened.mission;
        crate::game::start(
            &mut m,
            &opened.grid,
            &lib,
            &opened.biome,
            &selection,
            None,
            crate::terrain::Climate::default(),
        )
        .unwrap();
        m.simulation.unwrap().gameplay.ai.players
    };
    // «I nobili ribelli»: kingdom 6 is not marked computer but has a mode.
    let p = players("excalibur e il re artu.wcm::maps/2 los nobles rebeldes");
    assert_eq!(
        modes(&p),
        [
            (2, 3, "DATA\\AI\\War.ais"),
            (5, 3, "DATA\\AI\\War.ais"),
            (6, 3, "DATA\\AI\\Warmagic.ais")
        ]
    );
    // «La proclamazione del Khan»: kingdom 1 is a computer, the player is 2;
    // kingdom 4 only runs the economy. A script name after a leading zero
    // is ignored.
    let p = players("impero di gengis khan.wcm::maps/gengis_khan01");
    assert_eq!(
        modes(&p),
        [
            (1, 3, ""),
            (3, 3, ""),
            (4, 1, ""),
            (5, 3, ""),
            (6, 3, ""),
            (7, 3, ""),
            (8, 3, "")
        ]
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn players_compile_their_script_or_leave_the_kingdom() {
    let (c, lib, mut m, mut g, _) = setup(1);
    let mut sim = Simulation::new(&m, &lib, 64, "grass");
    assert!(sim.gameplay.ai.players[&2].program.is_none());
    // Kingdom 3's script is missing (0x402760: no computer player).
    sim.gameplay.ai.players.insert(
        3,
        ai::Player {
            mode: 3,
            script: "nowhere.ais".into(),
            ..Default::default()
        },
    );
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
    // The enemy read the default script, `random.ais`.
    let p = sim.gameplay.ai.players[&2].program.as_ref().unwrap();
    assert_eq!(p, &c.ai.compile("random.ais").unwrap());
    assert!(!sim.gameplay.ai.players.contains_key(&3));
    assert!(!combat::computer(&sim, 3));
    assert!(sim.gameplay.script.outbox.iter().any(|s| matches!(
        s,
        Show::Line(t) if t.contains("nowhere.ais") && t.contains("File not found")
    )));
    // The program travels with the save.
    let saved = serde_json::to_string(&sim.gameplay.ai).unwrap();
    let back: ai::State = serde_json::from_str(&saved).unwrap();
    assert_eq!(back, sim.gameplay.ai);
}

#[test]
fn the_default_script_depends_on_the_game() {
    let mut rules = world::GameRules::default();
    assert_eq!(ai::script_name("", &rules), "data\\ai\\random.ais");
    assert_eq!(
        ai::script_name("DATA\\AI\\War.ais", &rules),
        "DATA\\AI\\War.ais"
    );
    // King of the Hill with unlimited resources (0x402760).
    rules.game_type = 1;
    rules.resource_level = 2;
    assert_eq!(ai::script_name("", &rules), "data\\ai\\dm.ais");
    rules.resource_level = 1;
    assert_eq!(ai::script_name("", &rules), "data\\ai\\random.ais");
}

#[test]
#[ignore = "Requires installed original archives"]
fn mission_scripts_turn_the_computer_players_on_and_off() {
    let (c, lib, mut m, mut g, _) = setup(1);
    let mut sim = Simulation::new(&m, &lib, 64, "grass");
    // The test world's enemy has a full computer player; its own checks see it.
    assert_eq!(modes(&sim.gameplay.ai.players), [(2, 3, "")]);
    assert!(combat::computer(&sim, 2) && !combat::computer(&sim, 1));
    let named = |name: &str, text: &str| Named {
        name: name.into(),
        value: Action::Expression(text.into()),
    };
    m.script = Script {
        actions: vec![named(
            "ai",
            "$a = @setai(2, 0); $b = @setai(1, 2); $c = @setai(1, 4); \
             $d = @setnoattacktime(3, 60000)",
        )],
        rules: vec![Rule {
            name: "start".into(),
            condition: "1".into(),
            actions: vec!["ai".into()],
        }],
        ..Default::default()
    };
    for _ in 0..2 {
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
    }
    let st = &sim.gameplay.script;
    let v = |n: &str| st.variables.map[n];
    // 0x4b82e0 answers 1 for modes 0–3, 0 otherwise; 0x4b81d0 always 1.
    assert_eq!([v("$a"), v("$b"), v("$c"), v("$d")], [1, 1, 0, 1]);
    // `none` removed kingdom 2's player; `war` made one for kingdom 1, which
    // then had its pause frozen at 60 s (kingdom 2, gone, was skipped).
    assert_eq!(modes(&sim.gameplay.ai.players), [(1, 0x82, "")]);
    assert_eq!(sim.gameplay.ai.players[&1].peace_until, 60_000);
    assert!(combat::computer(&sim, 1) && !combat::computer(&sim, 2));
    let lines: Vec<_> = st
        .outbox
        .iter()
        .filter_map(|s| match s {
            Show::Message(key, name) => Some((*key, name.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(lines, [("aioff3", "Enemy"), ("aiwar3", "Player")]);
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_native_program_of_a_unit_follows_its_order() {
    use ai::program::*;
    let (_, lib, mut m, _, mut sim) = setup(1);
    let mut used = m.objects.iter().map(|o| o.id).collect();
    let field = m.objects.len();
    m.objects.push(
        world::starting::new_object(&mut used, 200, class::FIELD, 1, 1, [30, 30], &lib).unwrap(),
    );
    let mine = m.objects.len();
    m.objects.push(
        world::starting::new_object(&mut used, 201, class::RES_GOLD, 0, 1, [40, 30], &lib).unwrap(),
    );
    sim.gameplay
        .actors
        .resize(m.objects.len(), Default::default());
    let peasant = 1;
    let mut seen = Vec::new();
    for (intent, automatic) in [
        (None, false),
        (Some(Intent::Move([20, 20])), false),
        (Some(Intent::Harvest(field)), false),
        (Some(Intent::Harvest(mine)), false),
        (Some(Intent::Chop([10, 10])), false),
        (Some(Intent::Attack(0)), true),
        (Some(Intent::Flee([12, 12], 3000)), false),
    ] {
        let a = &mut sim.gameplay.actors[peasant];
        a.intent = intent;
        a.automatic = automatic;
        let p = ai::program(&sim, &m, peasant);
        seen.push((p.kind, p.priority));
    }
    assert_eq!(
        seen,
        [
            (IDLE, 0),
            (MOVE, 4000),
            (DIG, 4000),
            (MINE, 4000),
            (CHOP, 4000),
            (ATTACK, 3000),
            (RUNAWAY, 1000)
        ]
    );
    sim.gameplay.actors[peasant].intent = Some(Intent::Move([20, 20]));
    sim.gameplay.actors[peasant].automatic = false;
    sim.gameplay.actors[peasant].move_mode = 1;
    assert_eq!(ai::program(&sim, &m, peasant).kind, MOVE_ADVANCE);
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_script_that_fails_at_the_start_stops_the_players_after_it() {
    let (c, lib, m, _, mut sim) = setup(1);
    let player = |script: &str| ai::Player {
        mode: 3,
        script: script.into(),
        ..Default::default()
    };
    // 0x402760: kingdom 1 fails, so 2 and 3 get no player either.
    sim.gameplay.ai.players.insert(1, player("nowhere.ais"));
    sim.gameplay.ai.players.insert(3, player(""));
    assert!(sim.gameplay.ai.starting);
    ai::prepare(&mut sim, &m, &c.ai);
    assert!(sim.gameplay.ai.players.is_empty());
    // Later (0x402440) a failure costs only its own kingdom.
    sim.gameplay.ai.players.insert(1, player("nowhere.ais"));
    sim.gameplay.ai.players.insert(3, player(""));
    ai::prepare(&mut sim, &m, &c.ai);
    assert_eq!(modes(&sim.gameplay.ai.players), [(3, 3, "")]);
    let _ = lib;
}

/// Two islands across a water of 96 cells.
fn two_islands() -> MapGrid {
    let side = 16;
    let ground = (1 << 4) | (1 << 7);
    MapGrid {
        name: "islands".into(),
        side,
        cells: (0..side * side)
            .map(|i| {
                if (5..11).contains(&(i % side)) {
                    0
                } else {
                    ground
                }
            })
            .collect(),
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn computer_players_cross_the_water_with_a_ship_of_theirs() {
    use ai::reach::{self, BY_LAND, BY_SEA, NO};
    let (c, lib, mut m, _, mut sim) = setup(1);
    sim.gameplay.islands = islands::Islands::from_grid(&two_islands(), lib.trunks);
    let (west, water, east) = (1, 2, 3);
    assert_eq!(
        sim.gameplay.islands.links(west, east),
        islands::link::BY_WATER
    );
    // The player asks for itself, out of the list as when it thinks.
    let ask = |sim: &mut Simulation, m: &MissionWorld| {
        let mut p = std::mem::take(sim.gameplay.ai.players.get_mut(&2).unwrap());
        let way = reach::regions(sim, m, &lib, &c, &mut p, west, east);
        *sim.gameplay.ai.players.get_mut(&2).unwrap() = p;
        way
    };
    assert_eq!(ask(&mut sim, &m), NO);
    // A water of 60 cells or more wants a ship as soon as it is asked
    // about (0x416d00); without a list of ships no more.
    assert_eq!(sim.gameplay.ai.players[&2].wanted_ships[&water], 1);
    let ship = new_unit(&mut sim, &mut m, &lib, 23, 2, 1, [20, 20]).unwrap();
    sim.gameplay
        .ai
        .players
        .get_mut(&2)
        .unwrap()
        .ships
        .insert(water, vec![ship]);
    // An idle ship with room carries the crossing; the answer by sea is
    // never remembered.
    assert_eq!(ask(&mut sim, &m), BY_SEA);
    assert_eq!(ask(&mut sim, &m), BY_SEA);
    let mut p = std::mem::take(sim.gameplay.ai.players.get_mut(&2).unwrap());
    assert_eq!(
        reach::point(&mut sim, &m, &lib, &c, &mut p, [0, 0], [12, 0, 14, 2]),
        BY_SEA
    );
    *sim.gameplay.ai.players.get_mut(&2).unwrap() = p;
    // A busy ship does not; the water then wants one more.
    sim.gameplay.actors[ship].intent = Some(Intent::Move([30, 30]));
    assert_eq!(ask(&mut sim, &m), NO);
    assert_eq!(sim.gameplay.ai.players[&2].wanted_ships[&water], 2);
    // Within one island the answer is by land.
    let mut p = std::mem::take(sim.gameplay.ai.players.get_mut(&2).unwrap());
    assert_eq!(
        reach::regions(&mut sim, &m, &lib, &c, &mut p, east, east),
        BY_LAND
    );
}

#[test]
#[ignore = "Requires installed original archives"]
fn a_computer_ally_shares_back_what_the_player_shares() {
    let (_, _, mut m, _, sim) = setup(1);
    m.simulation = Some(sim);
    // 0x48f8bb: the player, allied to the computer's kingdom 2, shares its
    // resources (2) and its units (4) with it: kingdom 2 shares them back.
    m.diplomacy[1] = 0x40;
    m.diplomacy[8] = 0x40;
    let mut d = m.diplomacy.clone();
    d[1] |= 2 | 4;
    let next = accept_diplomacy(&m, &d);
    assert_eq!((next[1], next[8]), (0x46, 0x46));
    // What it already shares stays; only the resources come.
    m.diplomacy[8] = 0x44;
    let mut d = m.diplomacy.clone();
    d[1] |= 2;
    assert_eq!(accept_diplomacy(&m, &d)[8], 0x46);
    // Not allied (0x467d10 with 0x40), nothing.
    m.diplomacy[1] = 0x20;
    m.diplomacy[8] = 0x20;
    let mut d = m.diplomacy.clone();
    d[1] |= 2 | 4;
    assert_eq!(accept_diplomacy(&m, &d)[8], 0x20);
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_chat_gives_the_ai_command() {
    let (_, _, m, _, mut sim) = setup(1);
    // 0x404880 in any case: «AI 2 talk on» shows kingdom 2's SAY lines
    // (0x404677), «talk off» hides them; the digit needs no space after.
    ai::chat(&mut sim, &m, "ai 2 TALK ON");
    assert_ne!(sim.gameplay.ai.players[&2].mode & ai::TALK, 0);
    ai::chat(&mut sim, &m, "AI 2talk off");
    assert_eq!(sim.gameplay.ai.players[&2].mode & ai::TALK, 0);
    // Without a digit, the local kingdom: «full» gives it a computer
    // player, with its line (0x404766).
    sim.gameplay.script.outbox.clear();
    ai::chat(&mut sim, &m, "AI full");
    assert_eq!(sim.gameplay.ai.players[&1].mode & 3, 3);
    assert!(matches!(
        sim.gameplay.script.outbox.0.last(),
        Some(Show::Message("aifull3", _))
    ));
    // «off» takes every computer player away (0x4044a4); «on» makes those
    // of the start again (0x402760), the local kingdom not among them.
    ai::chat(&mut sim, &m, "AI off");
    assert!(sim.gameplay.ai.players.is_empty());
    ai::chat(&mut sim, &m, "AI on");
    assert_eq!(
        sim.gameplay.ai.players.keys().copied().collect::<Vec<_>>(),
        [2]
    );
    // Only at the start of the line.
    assert!(!ai::chat_command("Ciao AI full"));
}
