use retzar::{
    assets::GameAssets,
    hud::{self, HudSkin, HudState},
    map::MapGrid,
    scenario::ScenarioCatalog,
    simulation::{ObjectState, Simulation, Weather},
    terrain::Terrain,
    world::{MissionWorld, ObjectLibrary, class},
};
use std::{path::PathBuf, time::Duration};
fn source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn initial_researches_apply_native_weapon_and_class_bonuses_once() {
    let assets = GameAssets::open(&source()).unwrap();
    let lib = ObjectLibrary::load(&assets, "grass").unwrap();
    let mut m = mission("mp3.wmp::maps/mp3");
    let template = m
        .objects
        .iter()
        .find(|o| o.class == class::PEON)
        .unwrap()
        .clone();
    m.objects = [1, 3, 4, 10, 15, 17, 19, 32, 35, 42, 61]
        .into_iter()
        .map(|class| {
            let mut o = template.clone();
            o.class = class;
            o.owner = 1;
            o.civilization = 1;
            o.flags = 0x300;
            o.data[0x16..0x1a].fill(0);
            o.data[0x1c..0x1e].fill(0);
            o.health = 100;
            o.max_health = 200;
            o
        })
        .collect();
    let researches = [
        "te_temperiron",
        "te_scimitar",
        "te_crossbow",
        "te_chemistry",
        "te_scalearmor",
        "te_chainmail",
        "te_engineering",
        "te_ridinghorses",
        "te_architecture",
        "te_greatwall",
        "te_gunpowder",
        "te_kungfu",
    ];
    m.rules[0].initial_researches = researches.iter().map(|s| (*s).into()).collect();
    let sim = Simulation::new(&m, &lib, 256, "grass");
    for (i, (attack, defence)) in [
        (3, 3),
        (2, 3),
        (2, 3),
        (4, 1),
        (2, 0),
        (0, 3),
        (5, 0),
        (0, 0),
        (4, 1),
        (10, 0),
        (0, 0),
    ]
    .into_iter()
    .enumerate()
    {
        let base = ObjectState::read(&m.objects[i], &lib);
        assert_eq!(
            sim.objects[i].attack,
            base.attack + attack,
            "class {} attack",
            m.objects[i].class
        );
        assert_eq!(
            sim.objects[i].defence,
            base.defence + defence,
            "class {} defence",
            m.objects[i].class
        );
    }
    assert_eq!(
        sim.objects[10].resistance,
        lib.attributes.get(&m.objects[10]).resistance * 70 / 100
    );
    retzar::simulation::initial_health(&mut m);
    assert_eq!((m.objects[10].health, m.objects[10].max_health), (130, 260));
    assert_eq!((m.objects[0].health, m.objects[0].max_health), (100, 200));
    m.objects[0].owner = 0;
    let neutral = Simulation::new(&m, &lib, 256, "grass");
    assert_eq!(
        neutral.objects[0].attack,
        ObjectState::read(&m.objects[0], &lib).attack
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn research_and_upgrade_hints_show_the_original_costs() {
    let assets = GameAssets::open(&source()).unwrap();
    let lib = ObjectLibrary::load(&assets, "grass").unwrap();
    let skin = HudSkin::load(&assets).unwrap();
    assert!(
        skin.cost_text(&lib.locale, "euro", "te_temperiron")
            .unwrap()
            .ends_with(":400")
    );
    assert!(
        skin.cost_text(&lib.locale, "asia", "to_stower2_2")
            .unwrap()
            .ends_with(":500")
    );
    let mut m = mission("mp3.wmp::maps/mp3");
    let mut smith = m
        .objects
        .iter()
        .find(|o| o.class == class::PEON)
        .unwrap()
        .clone();
    smith.class = 58;
    smith.owner = m.player();
    smith.civilization = 1;
    smith.custom_image.clear();
    m.objects = vec![smith];
    let state = HudState {
        selected: vec![0],
        ..Default::default()
    };
    let list = hud::commands(&skin, &lib.locale, &state, &m, &lib);
    assert!(
        list.iter()
            .find(|c| c.order == "te_temperiron")
            .unwrap()
            .cost
            .as_ref()
            .unwrap()
            .ends_with(":400")
    );
    assert!(!list.iter().any(|c| c.order == "te_chainmail"));
    m.rules[m.player() as usize - 1]
        .initial_researches
        .push("te_scalearmor".into());
    let list = hud::commands(&skin, &lib.locale, &state, &m, &lib);
    assert!(
        list.iter()
            .find(|c| c.order == "te_chainmail")
            .unwrap()
            .unavailable
            .is_none()
    );
    assert!(!list.iter().any(|c| c.order == "te_scalearmor"));
    let widgets = hud::widgets(
        &skin,
        &lib.locale,
        &state,
        hud::Layout {
            width: 800,
            height: 600,
            left: false,
        },
        &m,
        &lib,
    );
    let index = list
        .iter()
        .position(|c| c.order == "te_temperiron")
        .unwrap();
    let hint = &widgets
        .iter()
        .find(|w| w.action == hud::HudAction::Command(index))
        .unwrap()
        .label;
    assert!(hint.contains(lib.locale.get("orders_add", "te_temperiron").unwrap()));
    m.rules[m.player() as usize - 1]
        .forbidden_researches
        .push("TE_TEMPERIRON".into());
    assert!(
        !hud::commands(&skin, &lib.locale, &state, &m, &lib)
            .iter()
            .any(|c| c.order == "te_temperiron")
    );
    let food = skin.cost_text(&lib.locale, "euro", "peon").unwrap();
    let discounted = skin
        .researched_cost_text(&lib.locale, "euro", "peon", &["te_feudalism".into()])
        .unwrap();
    let amount = food.rsplit(':').next().unwrap().parse::<u32>().unwrap();
    assert_eq!(
        discounted
            .rsplit(':')
            .next()
            .unwrap()
            .parse::<u32>()
            .unwrap(),
        amount * 80 / 100
    );
    for path in ["DATA/TECHNOLOGIES.INI", "DATA/UPGRADES.INI"] {
        let ini = retzar::assets::Ini::parse(&assets.get(path).unwrap());
        for name in ini.section_names() {
            let expected: Vec<_> = ["gold", "wood", "food", "stone", "manna"]
                .into_iter()
                .filter_map(|key| ini.get(name, key)?.trim().parse::<u32>().ok())
                .filter(|&n| n > 0)
                .collect();
            let cost = skin.cost_text(&lib.locale, "comm", name);
            assert_eq!(cost.is_some(), !expected.is_empty(), "{name}");
            for n in expected {
                assert!(cost.as_ref().unwrap().contains(&format!(":{n}")), "{name}");
            }
        }
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn busy_castles_keep_native_training_orders_and_hide_researches() {
    let assets = GameAssets::open(&source()).unwrap();
    let orders = retzar::assets::Ini::parse(&assets.get("DATA/ORDERS.INI").unwrap());
    let lib = ObjectLibrary::load(&assets, "grass").unwrap();
    let skin = HudSkin::load(&assets).unwrap();
    for civ in 1..=3 {
        let mut m = mission("mp3.wmp::maps/mp3");
        let mut castle = m
            .objects
            .iter()
            .find(|o| o.class == class::CASTLE)
            .unwrap()
            .clone();
        castle.owner = m.player();
        castle.civilization = civ;
        castle.custom_image.clear();
        m.rules[m.player() as usize - 1].initial_researches.clear();
        m.objects = vec![castle];
        let state = HudState {
            selected: vec![0],
            ..Default::default()
        };
        let idle = hud::commands(&skin, &lib.locale, &state, &m, &lib);
        assert!(
            idle.iter()
                .any(|c| orders.get(&c.order, "action") == Some("TECHNOLOGY")),
            "civ {civ}"
        );
        let mut sim = Simulation::new(&m, &lib, 256, "grass");
        sim.gameplay.actors.resize_with(1, Default::default);
        sim.gameplay.actors[0]
            .jobs
            .push_back(retzar::gameplay::Job {
                kind: retzar::gameplay::JobKind::Train(class::PEON),
                total: 200,
                remaining: 100,
                paid: [0; 4],
                due_ms: None,
                waiting: false,
            });
        m.simulation = Some(sim);
        let busy = hud::commands(&skin, &lib.locale, &state, &m, &lib);
        assert!(
            !busy
                .iter()
                .any(|c| orders.get(&c.order, "action") == Some("TECHNOLOGY"))
        );
        assert!(
            busy.iter()
                .any(|c| orders.get(&c.order, "classname") == Some("peon"))
        );
        assert!(
            hud::object_statistics(&skin, &m, 0)
                .iter()
                .any(|(k, v)| *k == "buildicon" && v == "50%")
        );
    }
}
fn mission(id: &str) -> MissionWorld {
    let cat = ScenarioCatalog::load(&source()).unwrap();
    let (campaign, index) = cat.locate(id).unwrap();
    let m = campaign.map_or(&cat.missions[index], |c| &cat.campaigns[c].missions[index]);
    let c = m.container().unwrap();
    let g = MapGrid::read(&c, &m.header).unwrap();
    MissionWorld::read(&c, &g).unwrap()
}
#[test]
#[ignore = "Requires the installed original game archives"]
fn clock_climate_and_visibility_advance_without_advancing_game_objects() {
    let a = GameAssets::open(&source()).unwrap();
    let lib = ObjectLibrary::load(&a, "grass").unwrap();
    let mut m = mission("mp3.wmp::maps/mp3");
    // Explicitly exercise shroud and fog, overriding this map's all-explored defaults.
    m.game_rules.no_fog = false;
    m.game_rules.explored = false;
    let immutable = serde_json::to_vec(&m).unwrap();
    let mut sim = Simulation::new(&m, &lib, 256, "grass");
    assert_eq!(sim.clock_minutes(), 600);
    let own = m
        .objects
        .iter()
        .find(|o| o.owner == m.player() && o.selectable())
        .unwrap();
    assert!(sim.visible(own));
    let enemy = m
        .objects
        .iter()
        .find(|o| {
            (1..=8).contains(&o.owner) && o.owner != m.player() && o.selectable() && !sim.known(o)
        })
        .unwrap();
    assert!(!sim.visible(enemy));
    let enemy_owner = enemy.owner;
    let mut diplomacy = m.diplomacy.clone();
    diplomacy[(enemy_owner as usize - 1) * 8 + m.player() as usize - 1] = 0x41;
    sim.refresh_visibility(&m, Some(&diplomacy));
    assert!(sim.visible(enemy));
    sim.refresh_visibility(&m, None);
    assert!(!sim.visible(enemy));
    assert!(sim.known(enemy));
    let rain_time = sim.weather_interval(0).0 / 4 + Duration::from_secs(30);
    sim.advance(rain_time, true);
    assert_eq!(sim.weather, Weather::Rain);
    assert_eq!(sim.precipitation, 1.);
    sim.advance(Duration::from_secs(0), false);
    assert_eq!(sim.weather, Weather::Clear);
    sim.advance(Duration::from_secs(1800) - rain_time, false);
    assert_eq!(sim.clock_minutes(), 600);
    assert_eq!(sim.ticks, 36000);
    m.simulation = Some(sim);
    assert_eq!(serde_json::to_vec(&m).unwrap(), immutable);
    for biome in ["grass", "snow", "desert"] {
        let t = Terrain::load(&a, biome).unwrap();
        let mut sim = Simulation::new(&m, &lib, 256, biome);
        assert_eq!(
            (sim.clouds, sim.rain_allowed, sim.snow_allowed),
            (t.clouds, t.rain, t.snow)
        );
        let rain_time = sim.weather_interval(0).0 / 4 + Duration::from_secs(30);
        sim.advance(rain_time, true);
        assert_eq!(
            sim.weather,
            match biome {
                "grass" => Weather::Rain,
                "snow" => Weather::Snow,
                _ => Weather::Clear,
            }
        );
    }
}
#[test]
#[ignore = "Requires the installed original game archives"]
fn object_attributes_items_inventory_and_selection_audio_use_original_data() {
    let a = GameAssets::open(&source()).unwrap();
    let lib = ObjectLibrary::load(&a, "grass").unwrap();
    let skin = HudSkin::load(&a).unwrap();
    let mut m = mission("mp4.wmp::maps/mp4");
    let shield = m
        .objects
        .iter()
        .position(|o| o.class == class::ITEM_PLUSPLUS && o.custom_image == "item_shield2")
        .unwrap();
    assert_eq!(lib.icon_for(&m.objects[shield]), Some("item_shield2"));
    assert_eq!(
        lib.image_for(&m.objects[shield]).as_deref(),
        Some("item_shield2")
    );
    assert_eq!(
        hud::object_statistics(&skin, &m, shield),
        vec![("defenceicon", m.objects[shield].word(24).to_string())]
    );
    let book = m
        .objects
        .iter()
        .position(|o| o.class == class::ITEM_MAGIC && o.custom_image == "item_book2")
        .unwrap();
    assert_eq!(
        lib.image_for(&m.objects[book]).as_deref(),
        Some("item_book2")
    );
    m.simulation = Some(Simulation::new(&m, &lib, 256, "grass"));
    assert!(
        hud::object_statistics(&skin, &m, book)
            .contains(&("attackicon_hand", m.objects[book].word(102).to_string()))
    );
    let mut soldier = m.objects[0].clone();
    soldier.class = class::FOOTMAN;
    soldier.civilization = 1;
    soldier.flags = 0;
    soldier.owner = 1;
    soldier.data = vec![0; 56];
    soldier.data[28] = 5;
    let state = ObjectState::read(&soldier, &lib);
    assert_eq!((state.attack, state.defence, state.level), (11, 6, 5));
    soldier.class = 4;
    let state = ObjectState::read(&soldier, &lib);
    assert_eq!((state.attack, state.defence, state.level), (8, 2, 5));
    soldier.owner = 0;
    assert_eq!(ObjectState::read(&soldier, &lib).attack, 10);
    soldier.owner = 1;
    for (class, ranged) in [(17, true), (21, true), (25, true), (29, false), (37, false)] {
        soldier.class = class;
        assert_eq!(ObjectState::read(&soldier, &lib).ranged, ranged);
    }
    soldier.class = class::TOWER_STONE;
    let state = ObjectState::read(&soldier, &lib);
    assert_eq!(state.attack, lib.attributes.get(&soldier).attack as u32 + 5);
    let tower_index = m.objects.len();
    m.objects.push(soldier.clone());
    m.simulation.as_mut().unwrap().objects.push(state);
    assert!(
        hud::object_statistics(&skin, &m, tower_index)
            .iter()
            .any(|(k, _)| *k == "attackicon_bow")
    );
    let mut magic = soldier.clone();
    magic.class = 17;
    magic.data[36..38].copy_from_slice(&23u16.to_le_bytes());
    magic.data[38..40].copy_from_slice(&100u16.to_le_bytes());
    let state = ObjectState::read(&magic, &lib);
    assert_eq!((state.mana, state.max_mana), (23, 100));
    soldier.class = class::PEON;
    assert!(
        lib.sounds_for(&soldier, "sound_select").is_empty(),
        "Original empty sound_select is silent; sound_command is for orders"
    );
    soldier.class = class::CASTLE;
    assert_eq!(
        lib.sounds_for(&soldier, "sound_select"),
        vec![("SOUND/buildings/e_castle1.wav".into(), 1000)]
    );
    let mut s = m.simulation.take().unwrap();
    let base = s.objects[0].defence;
    // A held shield adds its defence to the holder's stored value (0x463480).
    s.objects[0].contents = vec![shield];
    s.objects[shield].holder = Some(0);
    s.objects[shield].equipped = Some(0);
    s.objects[0].defence += m.objects[shield].word(24) as u32;
    assert_eq!(
        s.effective_attributes(0, &m).unwrap().defence,
        base + m.objects[shield].word(24) as u32
    );
    let before = serde_json::to_vec(&m).unwrap();
    s.advance(Duration::from_secs(300), true);
    m.simulation = Some(s);
    assert_eq!(before, serde_json::to_vec(&m).unwrap());
    let ui = HudState {
        selected: vec![0],
        contents_owner: Some(0),
        ..Default::default()
    };
    assert_eq!(hud::contents(&ui, &m), &[shield]);
    assert_eq!(lib.item_description(&m.objects[shield]), "Scudo (Tipo 2)");
    let mut potion = m.objects[shield].clone();
    potion.class = class::ITEM_HEALTH;
    potion.data[0x12..0x14].copy_from_slice(&250u16.to_le_bytes());
    potion.data[0x16..0x18].copy_from_slice(&7u16.to_le_bytes());
    potion.data[0x18..0x1a].copy_from_slice(&3u16.to_le_bytes());
    potion.data[0x1c..0x1e].copy_from_slice(&2u16.to_le_bytes());
    potion.data[0x24..0x26].copy_from_slice(&100u16.to_le_bytes());
    let index = m.objects.len();
    m.objects.push(potion);
    assert_eq!(
        hud::object_statistics(&skin, &m, index),
        vec![
            ("attackicon_hand", "7".into()),
            ("defenceicon", "3".into()),
            ("powericon", "250".into()),
            ("mannaicon", "100".into()),
            ("levelicon", "2".into()),
        ]
    );
    m.objects[book].data[102..104].copy_from_slice(&9999u16.to_le_bytes());
    assert!(
        !hud::object_statistics(&skin, &m, book)
            .iter()
            .any(|(k, _)| *k == "attackicon_hand")
    );
}
#[test]
#[ignore = "Requires the installed original HUD font"]
fn numeric_fields_preserve_height_and_10000_keeps_native_glyphs() {
    let a = GameAssets::open(&source()).unwrap();
    let skin = HudSkin::load(&a).unwrap();
    let font = &skin.font;
    let mut actual = vec![0; 80 * 20 * 4];
    let mut expected = actual.clone();
    font.draw_number(
        &mut actual,
        [80, 20],
        "10000",
        [0, 0, 48, 14],
        [255; 3],
        false,
    );
    font.draw_fit(
        &mut expected,
        [80, 20],
        "10000",
        [0, 0, 48, 14],
        [255; 3],
        false,
    );
    assert_eq!(actual, expected);
    assert!(font.measure("10000") <= 48);
    let mut long = vec![0; 80 * 20 * 4];
    font.draw_number(
        &mut long,
        [80, 20],
        "4294967295",
        [0, 0, 39, 14],
        [255; 3],
        false,
    );
    let rows = long
        .chunks_exact(80 * 4)
        .filter(|r| r.chunks_exact(4).any(|p| p[3] > 0))
        .count();
    assert!(rows >= 7, "large counters collapsed to {rows} rows");
}
