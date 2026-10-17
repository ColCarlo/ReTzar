use retzar::{
    menu::{SavedSession, Selection},
    save::{self, Snapshot},
    simulation::{Fog, ObjectState, Simulation, Weather},
    world::{MapObject, MissionWorld},
};
use std::{collections::BTreeMap, time::Duration};

fn snapshot() -> (SavedSession, Snapshot) {
    let object = |id, class| MapObject {
        id,
        class,
        flags: 0,
        x: 10,
        y: 10,
        owner: 1,
        civilization: 1,
        variant: 0,
        health: 80,
        max_health: 80,
        name: "Eroe".into(),
        subtitle: String::new(),
        portrait: String::new(),
        portrait_large: String::new(),
        custom_image: String::new(),
        data: vec![0; 128],
        mole: 0,
        control: [0; 8],
        sight: [0; 8],
    };
    let mission = MissionWorld {
        objects: vec![object(1, 3), object(2, 81)],
        ..Default::default()
    };
    let mut states = vec![ObjectState::default(); 2];
    states[0].contents = vec![1];
    states[1].holder = Some(0);
    let meta = SavedSession {
        version: 3,
        snapshot: true,
        name: String::new(),
        file: String::new(),
        title: "Sessione".into(),
        selection: Selection::Random { side: 64, seed: 17 },
        scenario: None,
        camera: [320., -320., 1.],
        setup: None,
        missing: false,
    };
    let s = Snapshot {
        language: retzar::lang::current().index(),
        schema: 1,
        grid: retzar::map::MapGrid {
            name: "snapshot".into(),
            side: 64,
            cells: vec![0; 4096],
        },
        biome: "grass".into(),
        title: meta.title.clone(),
        description: String::new(),
        starting_positions: [None; 8],
        mission,
        simulation: Simulation {
            elapsed: Duration::from_millis(163245),
            ticks: 3264,
            objects: states,
            fog: Fog::new(64),
            weather: Weather::Rain,
            precipitation: 0.75,
            clouds: true,
            rain_allowed: true,
            snow_allowed: false,
            seed: 123,
            gameplay: Default::default(),
            sounds: Vec::new(),
            flashes: Vec::new(),
            bolts: Vec::new(),
            memory: Default::default(),
            day_shift: 0,
            speed: retzar::simulation::NORMAL_SPEED,
            footprints: Default::default(),
        },
        source_entries: BTreeMap::from([(
            "unknown.extension".into(),
            std::sync::Arc::new(vec![1, 2, 255, 0]),
        )]),
        external_entries: BTreeMap::new(),
        portraits: BTreeMap::new(),
        hud: Default::default(),
        groups: Default::default(),
        health_kept: true,
        idle: 0,
        visual_time: Duration::from_millis(161123),
        flyers: vec![],
    };
    (meta, s)
}
#[test]
fn complete_snapshot_roundtrips_and_is_compressed() {
    let (meta, s) = snapshot();
    let bytes = save::encode(&meta, &s).unwrap();
    let (restored, decoded) = save::decode(&bytes).unwrap();
    assert_eq!(
        serde_json::to_value(s).unwrap(),
        serde_json::to_value(decoded).unwrap()
    );
    assert_eq!(restored.camera, meta.camera);
    assert!(bytes.len() < 5000);
}
#[test]
fn corrupted_truncated_oversized_and_future_saves_are_rejected() {
    let (meta, s) = snapshot();
    let bytes = save::encode(&meta, &s).unwrap();
    for at in [8, 16, 32, 64, bytes.len() - 1] {
        let mut bad = bytes.clone();
        bad[at] ^= 0x80;
        assert!(save::decode(&bad).is_err());
    }
    for len in [0, 7, 32, 63, bytes.len() - 1] {
        assert!(save::decode(&bytes[..len]).is_err());
    }
    let mut bad = bytes.clone();
    bad[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(save::decode(&bad).is_err());
    let mut future = s.clone();
    future.schema = 2;
    assert!(save::encode(&meta, &future).is_err());
}
#[test]
fn invalid_world_references_are_rejected_before_writing() {
    let (meta, mut s) = snapshot();
    s.simulation.objects[1].holder = Some(999);
    assert!(save::encode(&meta, &s).is_err());
    let (_, mut s) = snapshot();
    s.simulation.objects[1].contents = vec![0];
    s.simulation.objects[0].holder = Some(1);
    assert!(save::encode(&meta, &s).is_err());
    let (_, mut s) = snapshot();
    s.groups[0] = vec![999];
    assert!(s.validate().is_err());
    let (_, mut s) = snapshot();
    s.grid.cells.pop();
    assert!(s.validate().is_err());
}
#[test]
fn atomic_overwrites_recover_last_valid_copy_and_never_back_up_corruption() {
    let root = std::env::temp_dir().join(format!(
        "retzar-save-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let path = save::save_path(&root, "slot1").unwrap();
    let (meta, mut s) = snapshot();
    save::write(&path, &meta, &s).unwrap();
    s.simulation.elapsed = Duration::from_secs(99);
    save::write(&path, &meta, &s).unwrap();
    assert_eq!(
        save::read(&path).unwrap().1.simulation.elapsed,
        Duration::from_secs(99)
    );
    std::fs::write(&path, b"broken").unwrap();
    let (_, recovered, backup_used) = save::read(&path).unwrap();
    assert!(backup_used);
    assert_eq!(recovered.simulation.elapsed, Duration::from_millis(163245));
    save::write(&path, &meta, &s).unwrap();
    assert_eq!(
        save::read(&save::backup(&path))
            .unwrap()
            .1
            .simulation
            .elapsed,
        Duration::from_millis(163245)
    );
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        2
    );
    assert!(save::save_path(&root, "../x").is_err());
    assert!(save::save_path(&root, "").is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn named_saves_are_listed_newest_first_and_deleted_with_their_copies() {
    let root = std::env::temp_dir().join(format!("retzar-named-saves-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (mut meta, s) = snapshot();
    assert_eq!(save::new_file(&root), "save1");
    for (file, name) in [("save1", "Assedio"), ("save2", "")] {
        meta.name = name.into();
        save::write(&save::save_path(&root, file).unwrap(), &meta, &s).unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    // Rewriting keeps a recovery copy; the list shows the save once.
    meta.name = "Assedio, giorno 2".into();
    save::write(&save::save_path(&root, "save1").unwrap(), &meta, &s).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(
        root.join("saves/slot4.json"),
        br#"{"version":2,"selection":{"Random":{"side":64,"seed":3}},"camera":[0,0,1]}"#,
    )
    .unwrap();
    let saves = save::list(&root);
    let files: Vec<_> = saves.iter().map(|s| s.file.as_str()).collect();
    assert_eq!(files, ["slot4", "save1", "save2"]);
    assert_eq!(saves[1].name, "Assedio, giorno 2");
    assert_eq!(save::new_file(&root), "save3");
    save::delete(&root, "save1").unwrap();
    save::delete(&root, "slot4").unwrap();
    let files: Vec<_> = save::list(&root).into_iter().map(|s| s.file).collect();
    assert_eq!(files, ["save2"]);
    assert!(!root.join("saves/save1.rtzs.bak").exists());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn immutable_map_chunks_are_shared_verified_and_repaired() {
    let root = std::env::temp_dir().join(format!(
        "retzar-chunks-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let (meta, mut s) = snapshot();
    s.source_entries.insert(
        "user/voice.wav".into(),
        std::sync::Arc::new((0..100_000).map(|i| (i * 17) as u8).collect()),
    );
    let path = save::save_path(&root, "slot1").unwrap();
    save::write(&path, &meta, &s).unwrap();
    save::write(&save::save_path(&root, "slot2").unwrap(), &meta, &s).unwrap();
    let chunks: Vec<_> = std::fs::read_dir(root.join("saves/blobs"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(chunks.len(), 1);
    let (_, restored, recovered) = save::read(&path).unwrap();
    assert!(!recovered);
    assert_eq!(restored.source_entries, s.source_entries);
    assert!(std::fs::metadata(&path).unwrap().len() < 5000);
    std::fs::write(&chunks[0], b"truncated").unwrap();
    assert!(save::read(&path).is_err());
    save::write(&path, &meta, &s).unwrap();
    assert_eq!(
        save::read(&path).unwrap().1.source_entries,
        s.source_entries
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// Saves and the map data beside them are enciphered: neither their names
/// nor the original game's bytes can be read in them; the list still shows
/// them, and plain map data of earlier versions are read and replaced.
#[test]
fn saves_and_their_map_data_are_enciphered() {
    use std::io::Write;
    let root = std::env::temp_dir().join(format!("retzar-sealed-saves-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let contains = |bytes: &[u8], what: &[u8]| bytes.windows(what.len()).any(|w| w == what);
    let (mut meta, mut s) = snapshot();
    meta.name = "Partita segreta".into();
    let voice = b"RIFF voce originale in chiaro ".repeat(500);
    s.source_entries
        .insert("user/voice.wav".into(), std::sync::Arc::new(voice.clone()));
    let path = save::save_path(&root, "save1").unwrap();
    save::write(&path, &meta, &s).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(!contains(&bytes, b"Partita segreta") && !contains(&bytes, b"camera"));
    assert_eq!(save::list(&root)[0].name, "Partita segreta");
    let blob = std::fs::read_dir(root.join("saves/blobs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let sealed = std::fs::read(&blob).unwrap();
    assert!(sealed.starts_with(b"RTZBLOB2") && !contains(&sealed, b"voce originale"));
    // A plain datum of an earlier version: read, then sealed again.
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&voice).unwrap();
    let plain = [
        b"RTZBLOB\0".as_slice(),
        &(voice.len() as u64).to_le_bytes(),
        &z.finish().unwrap(),
    ]
    .concat();
    std::fs::write(&blob, &plain).unwrap();
    let (_, restored, _) = save::read(&path).unwrap();
    assert_eq!(
        restored.source_entries["user/voice.wav"].as_slice(),
        voice.as_slice()
    );
    save::write(&path, &meta, &s).unwrap();
    assert!(std::fs::read(&blob).unwrap().starts_with(b"RTZBLOB2"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn all_installed_maps_roundtrip_every_original_entry_and_runtime_state() {
    use retzar::{
        assets::GameAssets, map::MapGrid, scenario::ScenarioCatalog, terrain::Terrain,
        world::ObjectLibrary,
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let assets = GameAssets::open(root).unwrap();
    let catalog = ScenarioCatalog::load(root).unwrap();
    let mut count = 0;
    let mut raw_total = 0;
    let mut packed_total = 0;
    let store = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("save-map-roundtrip-{}", std::process::id()));
    for map in catalog
        .missions
        .iter()
        .chain(catalog.campaigns.iter().flat_map(|c| &c.missions))
    {
        let c = map.container().unwrap();
        let grid = MapGrid::read(&c, &map.header).unwrap();
        let terrain = Terrain::from_header(&assets, &c.get(&map.header).unwrap()).unwrap();
        let lib = ObjectLibrary::load(&assets, &terrain.biome).unwrap();
        let mission = MissionWorld::read(&c, &grid).unwrap();
        let sim = Simulation::new(&mission, &lib, grid.side, &terrain.biome);
        let (mut meta, mut s) = snapshot();
        meta.scenario = Some(map.id.clone());
        meta.title = map.title.clone();
        s.grid = grid;
        s.biome = terrain.biome;
        s.title = map.title.clone();
        s.description = map.description.clone();
        s.starting_positions = mission.starting_positions;
        s.mission = mission;
        s.simulation = sim;
        s.source_entries = c
            .entries
            .iter()
            .map(|e| {
                (
                    e.name.clone(),
                    std::sync::Arc::new(c.read(e.inode).unwrap()),
                )
            })
            .collect();
        let raw = serde_json::to_vec(&s).unwrap();
        let path = save::save_path(&store, &format!("slot{}", count % 5 + 1)).unwrap();
        save::write(&path, &meta, &s).unwrap();
        let (_, restored, _) = save::read(&path).unwrap();
        assert_eq!(serde_json::to_vec(&restored).unwrap(), raw, "{}", map.id);
        raw_total += raw.len();
        packed_total += std::fs::metadata(path).unwrap().len();
        count += 1;
    }
    // With the American CD, its campaign's nineteen maps too.
    let american = retzar::scenario::american_campaign(root).is_some();
    assert_eq!(count, 26 + 19 * american as usize);
    let chunks: Vec<_> = std::fs::read_dir(store.join("saves/blobs"))
        .unwrap()
        .map(|e| e.unwrap())
        .collect();
    let shared: u64 = chunks.iter().map(|e| e.metadata().unwrap().len()).sum();
    println!(
        "26 maps: full JSON={raw_total}, session files={packed_total}, shared chunks={shared}, chunk count={}",
        chunks.len()
    );
    std::fs::remove_dir_all(store).unwrap();
}
/// A save of an earlier ReTzar: the same envelope around the JSON it wrote.
fn envelope(meta: &SavedSession, raw: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let metadata = serde_json::to_vec(meta).unwrap();
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(raw).unwrap();
    let packed = z.finish().unwrap();
    let mut h = b"RTZSNAP\0".to_vec();
    h.extend(1u32.to_le_bytes());
    h.extend((metadata.len() as u32).to_le_bytes());
    h.extend((raw.len() as u64).to_le_bytes());
    h.extend((packed.len() as u64).to_le_bytes());
    let mut hash = blake3::Hasher::new();
    hash.update(&h);
    hash.update(&metadata);
    hash.update(&packed);
    h.extend(hash.finalize().as_bytes());
    [h, metadata, packed].concat()
}
#[test]
fn saves_whose_orders_no_longer_read_resume_without_them() {
    let (meta, s) = snapshot();
    // The units' orders as an earlier ReTzar wrote them (a Move op was a
    // number): the world reads, the orders start empty.
    let mut v = serde_json::to_value(&s).unwrap();
    v["simulation"]["gameplay"]["actors"] = serde_json::json!([{ "movement": 0 }]);
    let (_, old) = save::decode(&envelope(&meta, &serde_json::to_vec(&v).unwrap())).unwrap();
    assert!(old.simulation.gameplay.actors.is_empty());
    assert_eq!(old.mission.objects.len(), s.mission.objects.len());
    // Anything else that no longer reads is refused, with a clear message.
    v["mission"] = serde_json::json!(0);
    let e = save::decode(&envelope(&meta, &serde_json::to_vec(&v).unwrap())).unwrap_err();
    assert!(e.to_string().contains("versione precedente"));
}
