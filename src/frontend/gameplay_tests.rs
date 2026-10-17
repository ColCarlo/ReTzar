use super::*;
use retzar::{
    gameplay::{JobKind, Request},
    world::{FLAG_CONSTRUCTION, class},
};

#[test]
#[ignore = "Requires original assets; actual multi-building HUD left/right clicks"]
fn systemic_frontend_group_training_counts_and_right_click_refunds_every_producer() {
    let root = test_root("systemic-group-input");
    let (mut app, window) = headless(&root, (1280, 720).into());
    app.insert_resource(mission::StoryOnOpen(false));
    let (campaign, mission) = app
        .world()
        .resource::<MenuState>()
        .catalog
        .locate("mp4.wmp::maps/mp4")
        .unwrap();
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario { campaign, mission },
        None,
    )
    .unwrap();
    app.update();
    let (selected, order) = app
        .world_mut()
        .resource_scope(|world, mut loaded: Mut<Loaded>| {
            let s = loaded.scene.as_mut().unwrap();
            let owner = s.mission.player();
            s.mission.kingdoms[owner as usize - 1].resources = [10000; 4];
            let castle = s
                .mission
                .objects
                .iter()
                .position(|o| o.owner == owner && o.class == class::CASTLE)
                .unwrap();
            let mut copy = s.mission.objects[castle].clone();
            let mut cell = None;
            'find: for y in (2..s.side - 8).step_by(8) {
                for x in (2..s.side - 8).step_by(8) {
                    copy.x = x as u16;
                    copy.y = y as u16;
                    if retzar::gameplay::can_place(&s.grid, &s.mission, &s.library, &copy) {
                        cell = Some([copy.x, copy.y]);
                        break 'find;
                    }
                }
            }
            let cell = cell.unwrap();
            copy.x = cell[0];
            copy.y = cell[1];
            copy.id = (1..0xfc17)
                .find(|id| !s.mission.objects.iter().any(|o| o.id == *id))
                .unwrap();
            let second = s.mission.objects.len();
            s.mission.objects.push(copy);
            // Two buildings can only be commanded together through the
            // requests; the player selects one at a time (0x453240).
            let selected = vec![castle, second];
            let state = hud::HudState {
                selected: selected.clone(),
                ..default()
            };
            let game = world.resource::<Game>();
            let order = hud::commands(
                &game.classic,
                &game.skin.locale,
                &state,
                &s.mission,
                &s.library,
            )
            .iter()
            .find(|c| game.gameplay.class_name(&c.order) == "peon")
            .unwrap()
            .order
            .clone();
            (selected, order)
        });
    app.world_mut().resource_mut::<MissionUi>().hud.selected = selected.clone();
    click_gameplay_button(&mut app, window, &order);
    click_gameplay_button(&mut app, window, &order);
    {
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        assert!(selected.iter().all(|&i| {
            s.mission.simulation.as_ref().unwrap().gameplay.actors[i]
                .jobs
                .len()
                == 2
        }));
    }
    // The cursor remains over the create icon. Its right click must cancel
    // one matching job per producer, without issuing a map movement order.
    let before = app
        .world()
        .resource::<Loaded>()
        .scene
        .as_ref()
        .unwrap()
        .mission
        .kingdoms
        .clone();
    press(&mut app, MouseButton::Right);
    app.update();
    release(&mut app, MouseButton::Right);
    app.update();
    let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
    let owner = s.mission.player() as usize - 1;
    let sim = s.mission.simulation.as_ref().unwrap();
    assert!(
        selected
            .iter()
            .all(|&i| sim.gameplay.actors[i].jobs.len() == 1)
    );
    let paid = sim.gameplay.actors[selected[0]].jobs[0].paid;
    assert_eq!(
        s.mission.kingdoms[owner].resources,
        std::array::from_fn(|i| before[owner].resources[i] + 2 * paid[i])
    );
}

fn click_gameplay_button(app: &mut App, window: Entity, order: &str) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::ZERO);
    let cursor = {
        let sf = surface(app, window);
        let loaded = app.world().resource::<Loaded>();
        let scene = loaded.scene.as_ref().unwrap();
        let game = app.world().resource::<Game>();
        let ui = app.world().resource::<MissionUi>();
        let settings = &app.world().resource::<MenuState>().settings;
        let layout = hud_layout(sf, settings);
        let list = retzar::hud::commands(
            &game.classic,
            &game.skin.locale,
            &ui.hud,
            &scene.mission,
            &scene.library,
        );
        let index = list
            .iter()
            .position(|c| c.order == order)
            .unwrap_or_else(|| panic!("missing order {order}"));
        let widgets = retzar::hud::widgets(
            &game.classic,
            &game.skin.locale,
            &ui.hud,
            retzar::hud::Layout {
                width: layout.size.x as i32,
                height: layout.size.y as i32,
                left: settings.layout == 1,
            },
            &scene.mission,
            &scene.library,
        );
        let rect = widgets
            .iter()
            .find(|w| w.action == retzar::hud::HudAction::Command(index))
            .unwrap()
            .rect;
        (layout.origin
            + Vec2::new((rect.x + rect.w / 2) as f32, (rect.y + rect.h / 2) as f32) * layout.factor)
            / sf.dpi
    };
    move_to(app, window, cursor);
    press(app, MouseButton::Left);
    app.update();
    release(app, MouseButton::Left);
    app.update();
}

#[test]
#[ignore = "Requires original scenarios; uses real HUD clicks and original reserved scenery IDs"]
fn player_gameplay_original_maps_allow_building_placement_and_barracks_production() {
    for civ in 1..=3 {
        let root = test_root(&format!("gameplay-original-ui-{civ}"));
        let (mut app, window) = headless(&root, (1920, 1080).into());
        app.insert_resource(mission::StoryOnOpen(false));
        app.world_mut().resource_mut::<MenuState>().setup = Some(retzar::world::GameSetup {
            kingdoms: vec![retzar::world::KingdomChoice {
                record: 0,
                color: 1,
                team: 0,
                civilization: Some(civ),
                ..Default::default()
            }],
            players: Some(2),
            human: Some(0),
            resources: Some([10000; 4]),
            options: Some(retzar::world::GameRules {
                no_fog: true,
                explored: true,
                ..Default::default()
            }),
            ..Default::default()
        });
        let (campaign, mission) = app
            .world()
            .resource::<MenuState>()
            .catalog
            .locate("mp4.wmp::maps/mp4")
            .unwrap();
        menu_ui::load_now(
            app.world_mut(),
            Selection::Scenario { campaign, mission },
            None,
        )
        .unwrap();
        app.update();
        let (worker, point, count) = {
            let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
            let c = &app.world().resource::<Game>().gameplay;
            assert!(s.mission.objects.iter().any(|o| o.id == u16::MAX));
            let worker = s
                .mission
                .objects
                .iter()
                .position(|o| {
                    o.owner == s.mission.player() && o.class == class::PEON && o.selectable()
                })
                .unwrap();
            let o = &s.mission.objects[worker];
            let point = (2i32..35)
                .flat_map(|r| (-r..=r).flat_map(move |dy| (-r..=r).map(move |dx| (dx, dy))))
                .filter_map(|(dx, dy)| {
                    let x = o.x as i32 + dx;
                    let y = o.y as i32 + dy;
                    if x < 3 || y < 3 || x >= s.side as i32 || y >= s.side as i32 {
                        return None;
                    }
                    let p = [x as u16, y as u16];
                    let probe = retzar::gameplay::building_at(&s.library, c, o, "e_barrack", p)?;
                    retzar::gameplay::can_place(&s.grid, &s.mission, &s.library, &probe)
                        .then_some(p)
                })
                .next()
                .unwrap();
            (worker, point, s.mission.objects.len())
        };
        app.world_mut().resource_mut::<MissionUi>().hud.selected = vec![worker];
        click_gameplay_button(
            &mut app,
            window,
            if civ == 1 {
                "e_build"
            } else if civ == 2 {
                "r_build"
            } else {
                "a_build"
            },
        );
        click_gameplay_button(&mut app, window, "e_barrack");
        assert_eq!(
            app.world().resource::<MissionUi>().target_order.as_deref(),
            Some("e_barrack")
        );
        app.world_mut().resource_mut::<MapView>().center =
            Vec2::new(point[0] as f32 * 32. + 4., -(point[1] as f32) * 32. - 4.);
        let sf = surface(&app, window);
        move_to(&mut app, window, sf.physical / sf.dpi / 2.);
        // The placement grid this version adds goes with its setting, is
        // darker around the map point under the cursor, takes the chosen
        // strength and leaves unmarked the cells of a site the barracks can
        // take, while it marks others on screen.
        let uniforms = |app: &App| {
            let w = app.world();
            w.resource::<placement::Placement>().grid_uniforms(w)
        };
        app.world_mut()
            .resource_mut::<MenuState>()
            .settings
            .placement_grid = false;
        app.update();
        assert_eq!(uniforms(&app), None);
        {
            let mut menu = app.world_mut().resource_mut::<MenuState>();
            menu.settings.placement_grid = true;
            menu.settings.grid_strength = 2;
        }
        app.update();
        let [look, tone] = uniforms(&app).unwrap();
        let centre = app.world().resource::<MapView>().center;
        assert_eq!(look.z, 1.);
        assert!((Vec2::new(look.x, look.y) - centre).length() < 1.);
        assert_eq!(tone.truncate().to_array(), grid::TONES[2]);
        assert!(app.world().resource::<placement::Placement>().valid);
        {
            let w = app.world();
            let s = w.resource::<Loaded>().scene.as_ref().unwrap();
            let preview = w.resource::<placement::Placement>();
            let refused = preview.grid_refused(w, s.side).unwrap();
            let probe = retzar::gameplay::building_at(
                &s.library,
                &w.resource::<Game>().gameplay,
                &s.mission.objects[worker],
                "e_barrack",
                point,
            )
            .unwrap();
            assert_eq!(preview.cell, Some([probe.x, probe.y]));
            let [fw, fh] = s.library.cell_footprint(&probe);
            for y in probe.y as usize..probe.y as usize + fh {
                for x in probe.x as usize..probe.x as usize + fw {
                    assert!(!refused[y * s.side + x], "civ {civ}: {x},{y} marked");
                }
            }
            assert!(refused.iter().any(|&r| r), "civ {civ}: nothing marked");
        }
        press(&mut app, MouseButton::Left);
        app.update();
        release(&mut app, MouseButton::Left);
        app.update();
        {
            let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
            assert_eq!(
                s.mission.objects.len(),
                count + 1,
                "civ {civ}: {}",
                app.world().resource::<MenuState>().notice
            );
            assert!(s.mission.objects[count].flags & FLAG_CONSTRUCTION != 0);
        }
        assert!(!app.world().resource::<placement::Placement>().valid);
        press(&mut app, MouseButton::Right);
        app.update();
        release(&mut app, MouseButton::Right);
        app.update();
        assert!(app.world().resource::<MissionUi>().target_order.is_none());
        assert_eq!(uniforms(&app), None);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(120));
        app.update();
        {
            let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
            assert_eq!(
                s.mission.objects[count].flags & FLAG_CONSTRUCTION,
                0,
                "civ {civ}: failed construction"
            );
        }
        app.world_mut().resource_mut::<MissionUi>().hud.selected = vec![count];
        app.world_mut()
            .resource_mut::<MissionUi>()
            .hud
            .selection_changed();
        let order = if civ == 3 {
            "a_rhonin"
        } else if civ == 2 {
            "r_footman"
        } else {
            "e_footman"
        };
        click_gameplay_button(&mut app, window, order);
        let stats = {
            let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
            retzar::hud::object_statistics(
                &app.world().resource::<Game>().classic,
                &s.mission,
                count,
            )
        };
        assert!(stats.iter().any(|(key, _)| *key == "buildicon"));
        // Real frame-sized updates, rather than a single fast-forward, must
        // finish the queue and create the unit's visible sprites.
        for _ in 0..1000 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(20));
            app.update();
        }
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        assert!(
            s.mission.simulation.as_ref().unwrap().gameplay.actors[count]
                .jobs
                .is_empty(),
            "civ {civ}: production blocked: {:?}",
            s.mission.simulation.as_ref().unwrap().gameplay.actors[count].production_blocked
        );
        // The computer kingdoms make their own units meanwhile: the
        // player's is the one of its kingdom after the barracks.
        let unit = (count + 1..s.mission.objects.len())
            .find(|&i| s.mission.objects[i].owner == s.mission.player())
            .expect("the barracks' unit");
        assert!(s.visuals.iter().any(|v| v.object == unit));
        assert_eq!(s.mission.objects[unit].civilization, civ);
    }
}

#[test]
#[ignore = "Requires installed original assets; exercises actual input and sprites"]
fn player_gameplay_right_click_and_training_work_in_the_frontend_for_all_civilizations() {
    for civ in 1..=3 {
        let root = test_root(&format!("gameplay-{civ}"));
        let (mut app, window) = headless(&root, (2560, 1440).into());
        app.insert_resource(mission::StoryOnOpen(false));
        app.world_mut().resource_mut::<MenuState>().setup = Some(retzar::world::GameSetup {
            kingdoms: vec![retzar::world::KingdomChoice {
                record: 0,
                color: 1,
                team: 0,
                civilization: Some(civ),
                ..Default::default()
            }],
            players: Some(2),
            human: Some(0),
            resources: Some([10000; 4]),
            options: Some(retzar::world::GameRules {
                no_fog: true,
                explored: true,
                ..Default::default()
            }),
            ..Default::default()
        });
        menu_ui::load_now(
            app.world_mut(),
            Selection::Random {
                side: 64,
                seed: 741,
            },
            None,
        )
        .unwrap();
        app.update();
        let (worker, castle, dest, walk, menu, train) = {
            let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
            let g = app.world().resource::<Game>();
            let worker = s
                .mission
                .objects
                .iter()
                .position(|o| o.owner == s.mission.player() && o.class == class::PEON)
                .unwrap();
            let castle = s
                .mission
                .objects
                .iter()
                .position(|o| o.owner == s.mission.player() && o.class == class::CASTLE)
                .unwrap();
            let o = &s.mission.objects[worker];
            let dest = (1..8)
                .flat_map(|r| (-r..=r).flat_map(move |dy| (-r..=r).map(move |dx| (dx, dy))))
                .filter_map(|(dx, dy)| {
                    let x = o.x as i32 + dx;
                    let y = o.y as i32 + dy;
                    if x < 0 || y < 0 || x >= s.side as i32 || y >= s.side as i32 {
                        return None;
                    }
                    let p = [x as u16, y as u16];
                    let cell = s.grid.cells[y as usize * s.side + x as usize];
                    (cell & 0xfc00 == 0
                        && (cell >> 4) & 7 != 0
                        && (cell >> 7) & 7 != 0
                        && !s
                            .mission
                            .objects
                            .iter()
                            .any(|b| b.selectable() && b.x == p[0] && b.y == p[1]))
                    .then_some(p)
                })
                .next()
                .unwrap();
            let mut hud = hud::HudState::default();
            hud.selected = vec![castle];
            let train = hud::commands(&g.classic, &g.skin.locale, &hud, &s.mission, &s.library)
                .into_iter()
                .find(|c| g.gameplay.class_name(&c.order) == "peon")
                .unwrap()
                .order;
            (
                worker,
                castle,
                dest,
                s.library.property(o, "image").unwrap().to_owned(),
                s.library.property(o, "menu").unwrap().to_owned(),
                train,
            )
        };
        app.world_mut().resource_mut::<MissionUi>().hud.selected = vec![worker];
        app.world_mut().resource_mut::<Overlay>().show_health(true);
        let at = Vec2::new(dest[0] as f32 * 32. + 8., -(dest[1] as f32) * 32. - 8.);
        app.world_mut().resource_mut::<MapView>().center = at;
        let sf = surface(&app, window);
        move_to(&mut app, window, sf.physical / sf.dpi / 2.);
        press(&mut app, MouseButton::Right);
        app.update();
        release(&mut app, MouseButton::Right);
        app.update();
        let outline_entities = {
            let mut query = app.world_mut().query::<(Entity, &Transform)>();
            query
                .iter(app.world())
                .filter(|(_, t)| t.translation.z == 420.)
                .map(|(e, _)| e)
                .collect::<Vec<_>>()
        };
        assert_eq!(outline_entities.len(), 8);
        assert!(
            !app.world()
                .resource::<MenuState>()
                .notice
                .contains("insufficienti")
        );
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(5));
        app.update();
        {
            let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
            assert_eq!(s.mission.objects[worker].civilization, civ);
            assert!(
                s.mission.simulation.as_ref().unwrap().gameplay.actors[worker]
                    .intent
                    .is_none(),
                "{menu}: {}",
                app.world().resource::<MenuState>().notice
            );
            assert!(!walk.is_empty());
            let v = s
                .visuals
                .iter()
                .find(|v| v.object == worker && v.layer == retzar::world::Layer::Body)
                .unwrap();
            assert_eq!(v.footprint[0], s.mission.objects[worker].x as f32 * 32.);
            let center = outline_entities
                .iter()
                .map(|&e| {
                    app.world()
                        .get::<Transform>(e)
                        .unwrap()
                        .translation
                        .truncate()
                })
                .sum::<Vec2>()
                / 8.;
            assert!(
                (center
                    - Vec2::new(
                        v.bounds[0] + v.bounds[2] / 2.,
                        v.bounds[1] + v.bounds[3] / 2.
                    ))
                .length()
                    < 0.01,
                "selection brackets must follow the moving unit"
            );
        }
        let count = app
            .world()
            .resource::<Loaded>()
            .scene
            .as_ref()
            .unwrap()
            .mission
            .objects
            .len();
        app.world_mut()
            .resource_mut::<MissionUi>()
            .requests
            .push(Request::command(train, vec![castle]));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::ZERO);
        app.update();
        assert!(matches!(
            app.world()
                .resource::<Loaded>()
                .scene
                .as_ref()
                .unwrap()
                .mission
                .simulation
                .as_ref()
                .unwrap()
                .gameplay
                .actors[castle]
                .jobs[0]
                .kind,
            JobKind::Train(class::PEON)
        ));
        menu_ui::start_save(app.world_mut(), Some("slot1".into()), String::new()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.world().contains_resource::<menu_ui::SaveJob>() {
            menu_ui::finish_save(app.world_mut());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let path = retzar::save::save_path(&root, "slot1").unwrap();
        let (_, mut saved, _) = retzar::save::read(&path).unwrap();
        assert_eq!(saved.simulation.gameplay.actors[castle].jobs.len(), 1);
        {
            let scene = app.world().resource::<Loaded>().scene.as_ref().unwrap();
            let catalog = &app.world().resource::<Game>().gameplay;
            retzar::gameplay::advance(
                &mut saved.simulation,
                &mut saved.mission,
                &mut saved.grid,
                &scene.library,
                catalog,
                Duration::from_secs(20),
                10,
                false,
            );
        }
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(20));
        app.update();
        let s = app.world().resource::<Loaded>().scene.as_ref().unwrap();
        // The computer players build and train too: the live game and the
        // saved one made the same objects, the player's peasant among them.
        assert!(s.mission.objects.len() > count);
        assert_eq!(s.mission.objects.len(), saved.mission.objects.len());
        let trained = (count..s.mission.objects.len())
            .find(|&i| {
                let o = &s.mission.objects[i];
                o.owner == s.mission.player() && o.class == class::PEON
            })
            .unwrap();
        assert_eq!(s.mission.objects[trained].civilization, civ);
        for i in count..s.mission.objects.len() {
            let (a, b) = (&s.mission.objects[i], &saved.mission.objects[i]);
            assert_eq!((a.class, a.owner, a.x, a.y), (b.class, b.owner, b.x, b.y));
        }
        assert!(saved.simulation.gameplay.actors[castle].jobs.is_empty());
        assert!(s.visuals.iter().any(|v| v.object == trained));
        assert!(
            s.mission
                .objects
                .iter()
                .filter(|o| o.owner == s.mission.player())
                .all(|o| o.flags & FLAG_CONSTRUCTION == 0)
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
#[ignore = "Requires original assets; effects layer without a window"]
fn effects_draw_missiles_and_one_shot_animations_where_the_original_does() {
    let root = test_root("effects-layer");
    let (mut app, _) = headless(&root, (1280, 720).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: None,
            mission: 0,
        },
        None,
    )
    .unwrap();
    app.update();
    {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let sim = loaded
            .scene
            .as_mut()
            .unwrap()
            .mission
            .simulation
            .as_mut()
            .unwrap();
        // A crater kept for 10 s, under every object (priority 1).
        sim.flashes.push(retzar::gameplay::Flash {
            image: "expl_small_hole",
            at: [320, 320],
            fps: 11,
            loops: 1,
            prio: 1,
            hold_ms: 10_000,
            glow: 0,
            column: 0,
            delay_ms: 0,
        });
        // An arrow flying east, drawn at priority 200 with the east row.
        sim.gameplay.projectiles.push(retzar::gameplay::Projectile {
            attacker: 0,
            target: 0,
            kind: retzar::gameplay::missile::ARROW,
            shooter: 4,
            launched: sim.ticks,
            lands: sim.ticks + 100,
            start: [100, 200],
            end: [600, 200],
            ..Default::default()
        });
    }
    app.update();
    let mut q = app
        .world_mut()
        .query::<(&Sprite, &Transform, &Visibility)>();
    let placed: Vec<Transform> = q
        .iter(app.world())
        .filter(|(_, _, v)| **v != Visibility::Hidden)
        .map(|(_, t, _)| *t)
        .collect();
    // expl_small_hole's hotspot (25, 40) lies on the point.
    assert!(
        placed.iter().any(|t| t.translation.z == 5.),
        "crater under the objects"
    );
    assert!(
        placed
            .iter()
            .any(|t| t.translation.z == 390.5 && (t.translation.y + 200.).abs() < 40.),
        "arrow over the ground objects, on its line"
    );
}

#[test]
#[ignore = "Requires this installation's original game data; still creates no window"]
fn a_peasant_carrying_the_flag_waves_it_over_its_head() {
    use retzar::world::{FLAG_DEAD, FLAG_HELD, class};
    let root = test_root("waving-flag");
    let (mut app, _) = headless(&root, (1280, 720).into());
    app.insert_resource(mission::StoryOnOpen(false));
    menu_ui::load_now(
        app.world_mut(),
        Selection::Scenario {
            campaign: None,
            mission: 3,
        },
        None,
    )
    .unwrap();
    app.update();
    // The pictures at the flag's priority (0x65) within a cell of `at`.
    let near = |app: &mut App, at: Vec2| {
        let mut q = app
            .world_mut()
            .query::<(&Sprite, &Transform, &Visibility)>();
        q.iter(app.world())
            .filter(|(_, t, v)| {
                **v != Visibility::Hidden
                    && t.translation.z == 390.5
                    && t.translation.truncate().distance(at) < 32.
            })
            .count()
    };
    // An item of the map made a flag, in the hands of a peasant.
    let at = {
        let mut loaded = app.world_mut().resource_mut::<Loaded>();
        let m = &mut loaded.scene.as_mut().unwrap().mission;
        let player = m.player();
        let peon = m
            .objects
            .iter()
            .position(|o| o.owner == player && o.class == class::PEON)
            .unwrap();
        let item = (0..m.objects.len())
            .find(|&j| m.objects[j].item() && m.objects[j].flags & (FLAG_HELD | FLAG_DEAD) == 0)
            .unwrap();
        m.objects[item].class = class::ITEM_FLAG;
        m.objects[item].flags |= FLAG_HELD;
        let sim = m.simulation.as_mut().unwrap();
        sim.objects[item].holder = Some(peon);
        sim.objects[peon].contents.push(item);
        let o = &m.objects[peon];
        Vec2::new(o.x as f32 * 32. + 15., -(o.y as f32 * 32. - 10.))
    };
    let before = near(&mut app, at);
    app.update();
    assert_eq!(near(&mut app, at), before + 1);
    let _ = std::fs::remove_dir_all(root);
}
