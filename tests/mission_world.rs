use retzar::{
    assets::GameAssets,
    map::MapGrid,
    scenario::ScenarioCatalog,
    world::{MissionWorld, ObjectLibrary},
};
#[test]
#[ignore = "Requires the user's original Italian mission archives"]
fn import_every_original_mission_with_original_art() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = root.parent().unwrap();
    let a = GameAssets::open(source).unwrap();
    let catalog = ScenarioCatalog::load(source).unwrap();
    let mut reports = Vec::new();
    let mut arab_buildings = 0;
    let mut asian_buildings = 0;
    for m in catalog
        .campaigns
        .iter()
        .flat_map(|c| &c.missions)
        .chain(&catalog.missions)
    {
        let c = m.container().unwrap();
        let g = MapGrid::read(&c, &m.header).unwrap();
        let world = MissionWorld::read(&c, &g).unwrap_or_else(|e| panic!("{}: {e}", m.title));
        // Every record of every script reads whole, as many as the header counts.
        let [rules, actions, events, _] =
            retzar::script::counts(&c.get(&format!("{}.header", g.name)).unwrap());
        assert_eq!(
            [
                world.script.rules.len(),
                world.script.actions.len(),
                world.script.events.len()
            ],
            [rules as usize, actions as usize, events as usize],
            "{}",
            m.title
        );
        for rule in &world.script.rules {
            for name in &rule.actions {
                assert!(world.script.action(name).is_some(), "{}: {name}", m.title);
            }
        }
        for o in &world.objects {
            // These civilization-exclusive buildings verify the numeric codes
            // directly against original scenario records, beyond Euro artwork.
            match o.class_name() {
                "mosque" | "magicworkshop" => {
                    assert_eq!(o.civilization, 2, "{}: {}", m.title, o.class_name());
                    arab_buildings += 1;
                }
                "shaolinn" => {
                    assert_eq!(o.civilization, 3, "{}: {}", m.title, o.class_name());
                    asian_buildings += 1;
                }
                _ => {}
            }
        }
        assert!(
            world.conversation_error.is_none(),
            "{}: {:?}",
            m.title,
            world.conversation_error
        );
        // Every speaker is an object of the same scenario; 0 is the narrator.
        for p in world.conversations.iter().flat_map(|c| &c.phrases) {
            assert!(
                p.speaker == 0 || world.objects.iter().any(|o| o.id == p.speaker),
                "{}: speaker {} of «{}» is not on the map",
                m.title,
                p.speaker,
                p.text
            );
        }
        let header = c.get(&m.header).unwrap();
        let biome = retzar::assets::legacy_text(&header[0xd6..0x116])
            .trim_end_matches('\0')
            .to_owned();
        let lib = ObjectLibrary::load(&a, &biome).unwrap();
        let mut visuals = lib
            .build(&a, &world, g.side)
            .unwrap_or_else(|e| panic!("{}: {e}", m.title));
        let trees = retzar::world::add_trees(&a, &g, &biome, &mut visuals).unwrap();
        let cliffs = retzar::world::add_cliffs(&a, &g, &biome, &mut visuals).unwrap();
        assert!(
            visuals.missing.is_empty(),
            "{}: {:?}",
            m.title,
            visuals.missing
        );
        let outside: Vec<_> = world
            .objects
            .iter()
            .filter(|o| o.selectable() && (o.x as usize >= g.side || o.y as usize >= g.side))
            .map(|o| o.id)
            .collect();
        assert!(
            outside.is_empty(),
            "{}: objects outside map {outside:?}",
            m.title
        );
        for anim in &visuals.animations {
            assert!(anim.frames.len() > 1 && anim.timeline.duration_ms > 0);
            let first = &visuals.images[anim.frames[0]];
            assert!(
                anim.frames
                    .iter()
                    .all(|&i| visuals.images[i].width == first.width
                        && visuals.images[i].height == first.height)
            );
        }
        reports.push(serde_json::json!({"mission":m.title,"records":world.records,"hidden":world.hidden,"visuals":visuals.visuals.len(),"animated_sprites":visuals.visuals.iter().filter(|v|v.animation.is_some()).count(),"trees":trees,"cliffs":cliffs,"missing":visuals.missing,"script":[world.script.events.len(),world.script.actions.len(),world.script.rules.len()],"player":world.player(),"home":world.home(),"kingdoms":world.kingdoms,"conversations":world.conversations.len(),"phrases":world.conversations.iter().map(|c| c.phrases.len()).sum::<usize>()}));
    }
    // With the American CD, its campaign's nineteen maps too, and with the
    // Spanish edition its campaign's eight.
    let american = retzar::scenario::american_campaign(source).is_some();
    let spanish = !retzar::scenario::spanish_campaigns(source).is_empty();
    assert_eq!(
        reports.len(),
        26 + 19 * american as usize + 8 * spanish as usize
    );
    assert!(arab_buildings > 0 && asian_buildings > 0);
}
