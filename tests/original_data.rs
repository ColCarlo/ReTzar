use retzar::{archive::Archive, bitmap::Bitmap, hpfs::Container, map::MapGrid};
use std::path::PathBuf;

fn source() -> PathBuf {
    std::env::var_os("TZAR_DATA_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .to_path_buf()
        })
}
/// A test's folder with settings in Italian: the tests read the
/// installation's own texts, the default is English, and the language is
/// one for the whole process.
fn italian(root: &std::path::Path) -> &std::path::Path {
    let settings = root.join("settings.json");
    if !settings.exists() {
        std::fs::create_dir_all(root).unwrap();
        let language = retzar::lang::Language::Italian.index();
        std::fs::write(settings, format!("{{\"language\": {language}}}")).unwrap();
    }
    root
}

#[test]
#[ignore = "Requires the original Italian unit sprite artwork"]
fn packed_idle_animation_keeps_the_original_direction() {
    use retzar::{assets::GameAssets, sprite::SpriteSheet};
    let assets = GameAssets::open(&source()).unwrap();
    let knight =
        SpriteSheet::decode(&assets.get("IMAGES/UNIT/EURO KNIGHT IDLE.RLE").unwrap()).unwrap();
    assert_eq!((knight.columns, knight.rows), (8, 7));
    let bounds = |image: &Bitmap| {
        let mut min = [image.width, image.height];
        let mut max = [0, 0];
        for (i, p) in image.rgba.chunks_exact(4).enumerate() {
            if p[3] == 0 {
                continue;
            }
            let x = i as u32 % image.width;
            let y = i as u32 / image.width;
            min[0] = min[0].min(x);
            min[1] = min[1].min(y);
            max[0] = max[0].max(x);
            max[1] = max[1].max(y);
        }
        [max[0] - min[0] + 1, max[1] - min[1] + 1]
    };
    // Observed bounds in the original RLE's first direction and side view.
    // A row/column mix-up rotates the horse while it is supposed to be idle.
    for (row, height) in [70, 70, 70, 73, 72, 70, 70].into_iter().enumerate() {
        assert_eq!(bounds(&knight.frame(0, row).unwrap()), [32, height]);
    }
    for (row, height) in [58, 57, 57, 57, 57, 58, 58].into_iter().enumerate() {
        assert_eq!(bounds(&knight.frame(2, row).unwrap()), [68, height]);
    }
    assert_ne!(
        knight.frame(0, 0).unwrap().rgba,
        knight.frame(0, 3).unwrap().rgba
    );
}

#[test]
#[ignore = "Requires the original Italian terrain artwork"]
fn water_and_coast_frames_use_original_pixel_pages_without_animating_land() {
    use retzar::{assets::GameAssets, terrain::Terrain};
    let assets = GameAssets::open(&source()).unwrap();
    let mut t = Terrain::load(&assets, "grass").unwrap();
    let sea = t
        .sheets
        .iter()
        .find(|s| s.name.ends_with("/00.bmp"))
        .unwrap();
    let raw = Bitmap::decode(&assets.get("IMAGES/TERRAIN/GRASS/00.BMP").unwrap(), false).unwrap();
    assert_eq!(sea.frames.len(), 4);
    for (phase, image) in std::iter::once(&sea.image).chain(&sea.frames).enumerate() {
        assert_eq!(
            image.rgba,
            raw.crop(0, phase as u32 * 512, 512, 512).unwrap().rgba
        );
    }
    let coast = t
        .sheets
        .iter()
        .find(|s| s.name.ends_with("/01.bmp"))
        .unwrap();
    let raw = Bitmap::decode(&assets.get("IMAGES/TERRAIN/GRASS/01.BMP").unwrap(), false).unwrap();
    assert_eq!(coast.frames.len(), 3);
    for (phase, image) in std::iter::once(&coast.image)
        .chain(&coast.frames)
        .enumerate()
    {
        for variation in 0..2 {
            assert_eq!(
                image.crop(variation * 32, 0, 32, 448).unwrap().rgba,
                raw.crop((variation * 4 + phase as u32) * 32, 0, 32, 448)
                    .unwrap()
                    .rgba
            );
        }
    }
    let grid = MapGrid {
        name: "Water regression".into(),
        side: 2,
        cells: vec![0, 1 << 4, (2 << 7) | (2 << 4), (2 << 7) | (2 << 4)],
    };
    t.prepare(&grid).unwrap();
    assert_eq!(t.animated_tiles(grid.cells[1], 1, 0).unwrap().len(), 20);
    assert!(t.animated_tiles(grid.cells[2], 0, 1).is_none());
    let animation = t.animated_tiles(grid.cells[1], 1, 0).unwrap();
    assert!(
        animation
            .iter()
            .all(|frame| frame.sheet == animation[0].sheet)
    );
    for frame in animation {
        let p = t.sheets[frame.sheet]
            .image
            .crop(frame.x, frame.y, 32, 32)
            .unwrap();
        assert!(
            p.rgba
                .chunks_exact(4)
                .all(|p| p[..3] != [255, 0, 255] && p[..3] != [0, 255, 0])
        );
    }
}

#[test]
#[ignore = "Requires this installation's original game data"]
fn original_packs_and_bitmaps_are_readable() {
    let mut entries = 0;
    let mut bitmaps = 0;
    for p in std::fs::read_dir(source().join("Packs")).unwrap() {
        let p = p.unwrap().path();
        let archive = Archive::open(&p).unwrap();
        entries += archive.entries.len();
        for entry in &archive.entries {
            if entry.name.to_ascii_lowercase().ends_with(".bmp") {
                let bitmap = Bitmap::decode(&archive.read(entry).unwrap(), false).unwrap();
                assert_eq!(
                    bitmap.rgba.len(),
                    bitmap.width as usize * bitmap.height as usize * 4
                );
                bitmaps += 1;
            }
        }
    }
    assert_eq!(entries, 2873);
    assert_eq!(bitmaps, 590);
}

#[test]
#[ignore = "Requires this installation's original game data"]
fn original_map_allocations_and_grid_dimensions_are_consistent() {
    let mut scenarios = 0;
    for folder in ["Maps", "Campaign"] {
        for p in std::fs::read_dir(source().join(folder)).unwrap() {
            let p = p.unwrap().path();
            let container = Container::open(&p).unwrap();
            let maps = retzar::map::scan(&container, &p.display().to_string()).unwrap();
            assert!(!maps.is_empty());
            scenarios += maps.len();
            for entry in &container.entries {
                assert_eq!(container.read(entry.inode).unwrap().len(), entry.size);
                if entry.name.to_ascii_lowercase().ends_with(".header") {
                    let grid = MapGrid::read(&container, &entry.name).unwrap();
                    assert_eq!(grid.cells.len(), grid.side * grid.side);
                }
            }
        }
    }
    assert_eq!(scenarios, 26);
}

#[test]
#[ignore = "Requires this installation's original game data"]
fn ponte_vicino_border_tower_goes_back_to_tzar_5() {
    let container = Container::open(&source().join("Maps/MP4.wmp")).unwrap();
    let grid = MapGrid::read(&container, "maps/mp4.header").unwrap();
    let mut mission = retzar::world::MissionWorld::read(&container, &grid).unwrap();
    let tower = |m: &retzar::world::MissionWorld| {
        let o = m.objects.iter().find(|o| o.id == 1100).unwrap();
        (o.owner, [o.x, o.y])
    };
    // The installed map saves it with owner 10; the correction is ours.
    assert_eq!(tower(&mission), (10, [155, 76]));
    mission.correct_map_errors(&grid.name);
    assert_eq!(tower(&mission), (5, [155, 76]));
    assert!(
        !mission
            .objects
            .iter()
            .any(|o| o.owner == 10 && o.class == retzar::world::class::TOWER_STONE)
    );
}

#[test]
#[ignore = "Requires this installation's original game data"]
fn original_menu_fonts_campaigns_and_terrain_are_readable() {
    use retzar::{
        assets::GameAssets,
        menu::{Action, Menu, MenuSkin, Screen, ScrollTarget},
        terrain::Terrain,
    };
    let assets = GameAssets::open(&source()).unwrap();
    let skin = MenuSkin::load(&assets).unwrap();
    let classic = retzar::hud::HudSkin::load(&assets).unwrap();
    let mission = retzar::world::MissionWorld::default();
    let library = retzar::world::ObjectLibrary::load(&assets, "grass").unwrap();
    for width in [800, 1067] {
        for left in [false, true] {
            for panel in [
                None,
                Some(retzar::hud::Panel::Quests),
                Some(retzar::hud::Panel::Story),
                Some(retzar::hud::Panel::Narration),
                Some(retzar::hud::Panel::Diplomacy),
            ] {
                let state = retzar::hud::HudState {
                    panel,
                    ..Default::default()
                };
                let b = retzar::hud::render(
                    &skin,
                    &classic,
                    classic.background(width, 600, left),
                    retzar::hud::HudView {
                        width,
                        height: 600,
                        left,
                        title: "QA",
                        description: "Descrizione",
                        state: &state,
                        mission: &mission,
                        library: &library,
                        portraits: &std::collections::HashMap::new(),
                        camera: [0., 0., 1.],
                        side: 64,
                        pixels: [2560., 1440.],
                        messages: &[],
                        status: "",
                        player_name: "Tzar 1",
                        selection: [255, 255, 255],
                        map_text: 0,
                    },
                );
                let r = retzar::hud::map_rect(width, 600, left);
                let layout = retzar::hud::Layout {
                    width,
                    height: 600,
                    left,
                };
                for y in r.y..r.y + r.h {
                    for x in r.x..r.x + r.w {
                        assert_eq!(
                            b.rgba[((y * width + x) * 4 + 3) as usize],
                            if panel.is_some_and(|p| layout.window(p).has(x as f32, y as f32)) {
                                255
                            } else {
                                0
                            }
                        );
                    }
                }
            }
        }
    }
    // The colour switches of the original texts are styles, never printed.
    assert!(skin.credit_lines.iter().all(|(_, l)| !l.contains('@')));
    assert!(skin.credit_lines.iter().any(|(s, _)| *s == 0));
    assert!(
        skin.help_lines
            .iter()
            .flatten()
            .all(|(_, t)| !t.contains("@1") && !t.contains("@2"))
    );
    // List rows hold one line of the original list font.
    assert!(skin.small.height as i32 <= retzar::menu::ROW);
    let mut menu = Menu::new(
        &source(),
        italian(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-empty")),
    )
    .unwrap();
    // The Italian edition's three, the American release's when its CD is
    // there and the Spanish edition's own when it is.
    let american = retzar::scenario::american_campaign(&source()).is_some();
    let spanish = retzar::scenario::spanish_campaigns(&source()).len();
    assert_eq!(
        menu.catalog.campaigns.len(),
        3 + american as usize + spanish
    );
    assert_eq!(menu.catalog.missions.len(), 6);
    for screen in Screen::ALL {
        menu.open(screen);
        let b = skin.render(&menu);
        assert_eq!((b.width, b.height, b.rgba.len()), (800, 600, 800 * 600 * 4));
        let widgets = skin.widgets(&menu);
        assert!(!widgets.is_empty());
        // Only a name field being written may be empty.
        assert!(
            widgets
                .iter()
                .all(|w| !w.label.is_empty() || matches!(w.style, retzar::menu::Style::Name))
        );
    }
    // Long original descriptions must remain readable through their last line.
    for campaign in 0..menu.catalog.campaigns.len() {
        menu.open(Screen::Campaign);
        menu.activate(retzar::menu::Action::Campaign(campaign))
            .unwrap();
        // Every mission, listed or still to reach.
        for mission in 0..menu.catalog.campaigns[campaign].missions.len() {
            menu.mission = mission;
            for panel in 0..2 {
                let (text, rect, rows) = skin.description(&menu, panel).unwrap();
                let lines = skin.small.wrap(text, rect.w as usize);
                assert!(
                    lines
                        .iter()
                        .all(|line| skin.small.measure(line) <= rect.w as usize)
                );
                assert!(rows as i32 * skin.line() <= rect.h);
                let max = lines.len().saturating_sub(rows);
                menu.activate(Action::Scroll {
                    target: ScrollTarget::Description(panel),
                    delta: i16::MAX,
                    max,
                })
                .unwrap();
                assert_eq!(menu.description_scroll[panel], max);
                menu.activate(Action::Scroll {
                    target: ScrollTarget::Description(panel),
                    delta: i16::MIN,
                    max,
                })
                .unwrap();
                assert_eq!(menu.description_scroll[panel], 0);
            }
        }
        menu.description_scroll = [8, 8];
        menu.scroll = 4;
        menu.activate(retzar::menu::Action::Campaign(campaign))
            .unwrap();
        assert_eq!(menu.description_scroll, [0; 2]);
        assert_eq!(menu.scroll, 0);
    }
    menu.open(Screen::Missions);
    for mission in 0..menu.catalog.missions.len() {
        menu.activate(retzar::menu::Action::Custom(mission))
            .unwrap();
        let (text, rect, _) = skin.description(&menu, 1).unwrap();
        assert!(
            skin.small
                .wrap(text, rect.w as usize)
                .iter()
                .all(|line| skin.small.measure(line) <= rect.w as usize)
        );
        menu.description_scroll[1] = 8;
        menu.activate(retzar::menu::Action::Custom(mission))
            .unwrap();
        assert_eq!(menu.description_scroll[1], 0);
    }
    let mut glyphs = vec![0; 800 * 40 * 4];
    skin.title_font.draw(
        &mut glyphs,
        800,
        40,
        "Excalibur e il Re Artù",
        0,
        0,
        [255; 3],
    );
    assert!(glyphs.iter().filter(|&&b| b > 0).count() > 100);
    let mut count = 0;
    for scenario in menu
        .catalog
        .missions
        .iter()
        .chain(menu.catalog.campaigns.iter().flat_map(|c| &c.missions))
    {
        let c = scenario.container().unwrap();
        let h = c.get(&scenario.header).unwrap();
        let grid = scenario.grid().unwrap();
        let mut terrain = Terrain::from_header(&assets, &h).unwrap();
        terrain.prepare(&grid).unwrap();
        terrain
            .validate(&grid)
            .unwrap_or_else(|e| panic!("{}: {e}", scenario.title));
        let minimap = terrain.minimap(&grid).unwrap();
        assert_eq!(minimap.width as usize, grid.side);
        let batches = retzar::scene::batches(&grid, &terrain).unwrap();
        assert_eq!(
            batches.iter().map(|b| b.positions.len() / 4).sum::<usize>(),
            grid.cells.len()
        );
        for b in &batches {
            assert_eq!(b.positions.len(), b.uv.len());
            assert_eq!(b.indices.len(), b.positions.len() / 4 * 6);
            assert!(b.uv.iter().flatten().all(|&v| (0.0..=1.0).contains(&v)));
            assert!(b.indices.iter().all(|&i| (i as usize) < b.positions.len()));
        }
        for sheet in terrain
            .sheets
            .iter()
            .filter(|s| s.name.starts_with("composite/"))
        {
            assert!(
                sheet
                    .image
                    .rgba
                    .chunks_exact(4)
                    .all(|p| p[..3] != [255, 0, 255] && p[..3] != [0, 255, 0])
            );
        }
        count += 1;
    }
    // The American campaign's nineteen maps and the Spanish edition's
    // twenty-six on the same terrains.
    assert_eq!(
        count,
        26 + 19 * american as usize + 26 * (spanish > 0) as usize
    );
}

#[test]
#[ignore = "Requires this installation's original game data"]
fn menu_navigation_settings_cancel_apply_and_local_slots() {
    use retzar::menu::{Action, Effect, Menu, MenuSkin, OptionKey, Screen, Settings, SettingsTab};
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("target/menu-test-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    let mut menu = Menu::new(&source(), italian(&root)).unwrap();
    let skin = MenuSkin::load(&retzar::assets::GameAssets::open(&source()).unwrap()).unwrap();
    let widgets = menu.widgets(&skin.locale);
    assert_eq!(
        (
            widgets[0].rect.x,
            widgets[0].rect.y,
            widgets[0].rect.w,
            widgets[0].rect.h
        ),
        (270, 277, 235, 25)
    );
    menu.activate(widgets[0].action.clone()).unwrap();
    assert_eq!(menu.screen, Screen::Single);
    menu.activate(Action::Open(Screen::Campaign)).unwrap();
    assert!(matches!(
        menu.activate(Action::Play).unwrap(),
        Effect::Load(_)
    ));
    // A legacy v2 fixture remains readable; the frontend now writes full .rtzs snapshots.
    let selection = menu.selection.clone().unwrap();
    let legacy = serde_json::json!({"version": 2, "scenario": menu.scenario_id(&selection),
        "selection": selection, "camera": [400., -300., 0.75], "setup": menu.setup});
    std::fs::create_dir_all(root.join("saves")).unwrap();
    std::fs::write(
        root.join("saves/slot3.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    // Slots name the scenario by its stable id, not by its position in the lists.
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("saves/slot3.json")).unwrap()).unwrap();
    assert_eq!(saved["version"], 2);
    assert!(
        saved["scenario"]
            .as_str()
            .is_some_and(|id| id.contains("::"))
    );
    menu.activate(Action::Open(Screen::Options)).unwrap();
    let saved = menu.settings.clone();
    let kept = std::fs::read(root.join("settings.json")).unwrap();
    menu.activate(Action::Change(OptionKey::Sound, -30))
        .unwrap();
    menu.activate(Action::Tab(SettingsTab::Display)).unwrap();
    menu.activate(Action::Change(OptionKey::Resolution, 1))
        .unwrap();
    menu.activate(Action::Set(OptionKey::Grid, 0)).unwrap();
    // The frames' limit moves without the vertical synchronization.
    menu.activate(Action::Change(OptionKey::Vsync, 1)).unwrap();
    menu.activate(Action::Change(OptionKey::Frames, -10))
        .unwrap();
    menu.activate(Action::Tab(SettingsTab::Game)).unwrap();
    menu.activate(Action::Change(OptionKey::Edge, 1)).unwrap();
    assert_eq!(menu.screen, Screen::Options);
    assert_ne!(menu.draft.window, saved.window);
    assert_ne!(menu.draft.sound, saved.sound);
    assert_ne!(menu.draft.edge_scroll, saved.edge_scroll);
    assert_ne!(menu.draft.placement_grid, saved.placement_grid);
    assert_ne!(menu.draft.max_fps, saved.max_fps);
    assert_ne!(menu.draft.vsync, saved.vsync);
    menu.activate(Action::Back).unwrap();
    assert_eq!(menu.settings, saved);
    assert_eq!(menu.current_settings(), &saved);
    assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), kept);
    menu.activate(Action::Open(Screen::Options)).unwrap();
    menu.activate(Action::Change(OptionKey::Resolution, 1))
        .unwrap();
    assert!(matches!(
        menu.activate(Action::Apply).unwrap(),
        Effect::SettingsChanged
    ));
    assert_eq!(
        Settings::load(&root.join("settings.json")).unwrap(),
        menu.settings
    );
    for tab in SettingsTab::ALL {
        menu.tab = tab;
        menu.open(Screen::Options);
        let widgets = menu.widgets(&skin.locale);
        for (i, a) in widgets.iter().enumerate() {
            assert!(
                a.rect.x >= 0
                    && a.rect.y >= 0
                    && a.rect.x + a.rect.w <= 800
                    && a.rect.y + a.rect.h <= 600
            );
            for b in &widgets[i + 1..] {
                let overlaps = a.rect.x < b.rect.x + b.rect.w
                    && b.rect.x < a.rect.x + a.rect.w
                    && a.rect.y < b.rect.y + b.rect.h
                    && b.rect.y < a.rect.y + a.rect.h;
                assert!(!overlaps, "Overlapping controls: {} / {}", a.label, b.label);
            }
        }
    }
    // A slot whose scenario is no longer installed is reported, not misloaded.
    std::fs::write(
        root.join("saves/slot5.json"),
        br#"{"version":2,"scenario":"assente.wmp::maps/x","selection":{"Scenario":{"campaign":null,"mission":0}},"camera":[0,0,1]}"#,
    )
    .unwrap();
    let mut reopened = Menu::new(&source(), italian(&root)).unwrap();
    // Old slots are listed by their file names among the saves.
    let at = |m: &Menu, file: &str| m.saves.iter().position(|s| s.file == file).unwrap();
    reopened
        .activate(Action::Slot(at(&reopened, "slot3")))
        .unwrap();
    let Effect::Restore(s) = reopened.activate(Action::LoadSlot).unwrap() else {
        panic!()
    };
    assert_eq!(s.camera, [400., -300., 0.75]);
    assert!(reopened.scenario(&s.selection).is_some());
    reopened.save = None;
    assert!(matches!(
        reopened.activate(Action::LoadSlot).unwrap(),
        Effect::None
    ));
    reopened
        .activate(Action::Slot(at(&reopened, "slot5")))
        .unwrap();
    assert!(reopened.saves[at(&reopened, "slot5")].missing);
    assert!(reopened.activate(Action::LoadSlot).is_err());
    // The load page lists one type of game: missions first, then campaigns
    // and demos, which are not played back.
    reopened.open(Screen::Load);
    let campaign = |m: &Menu, i: usize| {
        matches!(
            m.saves[i].selection,
            retzar::menu::Selection::Scenario {
                campaign: Some(_),
                ..
            }
        )
    };
    let missions = reopened.listed_saves();
    assert!(!missions.is_empty() && missions.iter().all(|&i| !campaign(&reopened, i)));
    assert_eq!(reopened.save, missions.first().copied());
    reopened.activate(Action::LoadType(1)).unwrap();
    assert!(
        reopened
            .listed_saves()
            .iter()
            .all(|&i| campaign(&reopened, i))
    );
    reopened.activate(Action::LoadType(1)).unwrap();
    assert!(reopened.listed_saves().is_empty());
    assert_eq!(reopened.save, None);
    reopened.activate(Action::LoadType(1)).unwrap();
    assert_eq!(reopened.load_type, 0);
    // The load window carries the small help ball (0x45c800 with 0), not
    // the pages' big one that would hide the start of its title.
    let ball = reopened
        .widgets(&skin.locale)
        .into_iter()
        .find(|w| w.action == Action::Help)
        .unwrap();
    assert_eq!((ball.rect.w, ball.rect.h), (50, 43));
    assert_eq!(
        ball.style,
        retzar::menu::Style::HelpBall("COMMON/BTN_HELP_S")
    );
    // «Resa dell'IA» has no hint in the original (0x4c92b0); this version
    // explains it in help mode.
    reopened.open(Screen::Random);
    let resign = reopened
        .widgets(&skin.locale)
        .into_iter()
        .find(|w| w.action == Action::AiResign)
        .unwrap();
    let key = reopened.hint_key(&resign).unwrap();
    assert!(skin.hint_text(key).contains("arrendono"));
    // The load page returns to where it was opened from.
    reopened.open(Screen::Pause);
    reopened.activate(Action::Open(Screen::Load)).unwrap();
    reopened.activate(Action::Back).unwrap();
    assert_eq!(reopened.screen, Screen::Pause);
    reopened.open(Screen::Single);
    reopened.activate(Action::Open(Screen::Load)).unwrap();
    reopened.activate(Action::Back).unwrap();
    assert_eq!(reopened.screen, Screen::Single);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "Requires installed original archives"]
fn campaigns_list_their_entry_missions_then_those_reached() {
    use retzar::menu::{Action, Menu, Screen};
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-progress");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut menu = Menu::new(&source(), italian(&root)).unwrap();
    menu.open(Screen::Campaign);
    // 0x4225e3: only the missions MAPS\Maps.ini opens with «yes», one per
    // installed campaign.
    for c in 0..menu.catalog.campaigns.len() {
        menu.activate(Action::Campaign(c)).unwrap();
        let listed = menu.campaign_missions();
        assert_eq!(listed.len(), 1, "{}", menu.catalog.campaigns[c].title);
        assert!(menu.catalog.campaigns[c].missions[listed[0]].initially_available);
        assert_eq!(menu.mission, listed[0]);
    }
    // Their order: the numbered names of the expansion, else the LoadMap
    // actions from the first, as the tutorial's lessons follow each other.
    let tutorial = menu
        .catalog
        .campaigns
        .iter()
        .find(|c| c.missions[0].id.starts_with("tzartutorial.wcm::"))
        .unwrap();
    let lessons: Vec<&str> = tutorial
        .missions
        .iter()
        .map(|m| m.id.rsplit('/').next().unwrap())
        .collect();
    assert_eq!(
        lessons,
        [
            "intro",
            "tutmap1",
            "tutmap2",
            "perfect 3",
            "perfect 4",
            "perfect 5",
            "tutmap6"
        ]
    );
    menu.activate(Action::Campaign(0)).unwrap();
    let missions = &menu.catalog.campaigns[0].missions;
    let second = (0..missions.len())
        .find(|&i| !missions[i].initially_available)
        .unwrap();
    let id = missions[second].id.clone();
    // Not listed, it cannot be chosen.
    menu.activate(Action::Mission(second)).unwrap();
    assert_ne!(menu.mission, second);
    // Reached, it stays on the list, also at the next start.
    menu.reach(&id);
    assert!(menu.campaign_missions().contains(&second));
    menu.activate(Action::Mission(second)).unwrap();
    assert_eq!(menu.mission, second);
    let reopened = Menu::new(&source(), italian(&root)).unwrap();
    assert!(reopened.campaign_missions().contains(&second));
    // Without the addition the list is the original's.
    menu.settings.campaign_progress = false;
    assert!(!menu.campaign_missions().contains(&second));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "Requires installed original archives and the American CD in USA"]
fn the_american_campaign_is_listed_and_reads_in_italian() {
    use retzar::{lang::Language, menu::Selection, script::Action};
    let source = source();
    if retzar::scenario::american_campaign(&source).is_none() {
        return;
    }
    let catalog = retzar::scenario::ScenarioCatalog::load(&source).unwrap();
    // Among the other campaigns, by file name.
    let files: Vec<String> = catalog
        .campaigns
        .iter()
        .map(|c| {
            c.missions[0]
                .file
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_lowercase()
        })
        .collect();
    let mut expected = vec![
        "excalibur e il re artu.wcm",
        "impero di gengis khan.wcm",
        "tzar.wcm",
        "tzartutorial.wcm",
    ];
    if !retzar::scenario::spanish_campaigns(&source).is_empty() {
        expected.insert(2, "la reconquista.wcm");
        expected.insert(3, "los dominios de la magia.wcm");
    }
    assert_eq!(files, expected);
    let american = files.iter().position(|f| f == "tzar.wcm").unwrap();
    let campaign = &catalog.campaigns[american];
    assert_eq!(campaign.title, "TZAR: Il peso della corona");
    assert_eq!(campaign.missions.len(), 19);
    // MAPS\Maps.ini opens «The Beginning» alone; the others come with
    // LoadMap, as in the Italian edition's campaigns.
    let open: Vec<&str> = campaign
        .missions
        .iter()
        .filter(|m| m.initially_available)
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(open, ["tzar.wcm::maps/v_00"]);
    // Listed in story order, from it along the LoadMap actions: the branch
    // of «Map 5.1» (Sartor captured) before «Map 6», the castle last.
    let ids: Vec<&str> = campaign
        .missions
        .iter()
        .map(|m| m.id.rsplit('/').next().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "v_00",
            "saa3",
            "saa4",
            "saa5",
            "saa6",
            "saa7",
            "saa8",
            "saa9",
            "saa10",
            "saa11",
            "saa12",
            "alex1",
            "v_01",
            "bigisland",
            "v_02",
            "vsaa",
            "rajba",
            "benas",
            "ultimate"
        ]
    );
    let assets = retzar::assets::GameAssets::open(&source).unwrap();
    // Every text the player reads is in Italian: an English one would have
    // no Italian of the catalog (`translate` toward English finds it).
    let italian = |text: &str| {
        retzar::lang::translate(text, Language::English).is_some()
            // Names and words the same in both languages.
            || ["Benas", "Rajba", "Sartor!", "|"].contains(&text.trim())
    };
    for (i, m) in campaign.missions.iter().enumerate() {
        let opened = retzar::game::open(
            &assets,
            &Selection::Scenario {
                campaign: Some(american),
                mission: i,
            },
            Some(m.clone()),
            None,
        )
        .unwrap();
        assert!(italian(&opened.title), "{}: {}", m.id, opened.title);
        let world = &opened.mission;
        for a in &world.script.actions {
            let text = match &a.value {
                Action::Story { text, .. } | Action::AddQuest { text, .. } => text,
                Action::Lose(_, text) | Action::Win(_, text) => text,
                _ => continue,
            };
            assert!(italian(text), "{}: {text}", m.id);
        }
        for p in world.conversations.iter().flat_map(|c| &c.phrases) {
            assert!(italian(&p.text), "{}: {}", m.id, p.text);
        }
    }
    // The story that opens the campaign, with its own music.
    let first = &campaign.missions[0];
    let world =
        retzar::world::MissionWorld::read(&first.container().unwrap(), &first.grid().unwrap())
            .unwrap();
    assert!(world.script.actions.iter().any(|a| matches!(
        &a.value,
        Action::Story { music, .. } if music == "USER\\11.mid"
    )));
    let song = retzar::audio::Midi::decode(&first.container().unwrap().get("USER/11.mid").unwrap())
        .unwrap();
    assert!(song.seconds > 10. && !song.events.is_empty());
}
