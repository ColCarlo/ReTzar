//! Every button of the command grid, for every class the player can own in
//! every civilization: each page and submenu is opened and each button
//! pressed, on a copy of the world, with a target when it asks for one.

use super::tests::setup;
use super::*;
use crate::hud::{self, HudSkin, HudState};

/// Orders the interface itself carries out: panels, dialogs, the selection.
const OWN: [&str; 9] = [
    "view_quests",
    "nextidle",
    "onlinehelp",
    "gamble",
    "trade1",
    "build_item",
    "trade",
    "trade_ship",
    "select_crusaders",
];

/// What a press of `order` by `j` became: its result, on a copy.
#[allow(clippy::too_many_arguments)]
fn press(
    c: &Catalog,
    lib: &ObjectLibrary,
    m: &MissionWorld,
    g: &MapGrid,
    sim: &Simulation,
    j: usize,
    order: &str,
    targets: &Targets,
) -> std::result::Result<String, String> {
    let (mut m, mut sim) = (m.clone(), sim.clone());
    m.simulation = None;
    let o = &m.objects[j];
    let mut req = Request::command(order, vec![j]);
    if c.targeted(order) {
        let action = c.action(order);
        let word = action.split_whitespace().next().unwrap_or("");
        let near = [o.x + 4, o.y + 4];
        req.cell = Some(near);
        req.target = match word {
            "attack" | "murder" | "bribe" | "impersonate" | "spell" => Some(targets.enemy),
            "repair" | "guard" => Some(targets.own),
            "capture" => Some(targets.hostile_building),
            "harvest" | "dig" | "mine" => Some(targets.gold),
            "chop" => None,
            "pick_item" => Some(targets.item),
            _ => None,
        };
        if let Some(t) = req.target {
            req.cell = Some([m.objects[t].x, m.objects[t].y]);
        }
        if word == "build" {
            req.cell = Some([o.x.saturating_sub(8).max(2), o.y + 8]);
        }
    }
    issue(&mut sim, &mut m, g, lib, c, &req)
}

struct Targets {
    enemy: usize,
    own: usize,
    hostile_building: usize,
    gold: usize,
    item: usize,
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn every_command_button_of_every_class_does_its_work() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let assets = GameAssets::open(root).unwrap();
    let skin = HudSkin::load(&assets).unwrap();
    let locale = crate::assets::Ini::locale(&assets).unwrap();
    let mut broken = Vec::new();
    let mut pressed = 0;
    for civ in 1..=3 {
        let (c, lib, mut base, mut g, mut base_sim) = setup(civ);
        base_sim.fog.explore_all();
        base.game_rules.no_fog = true;
        base_sim.gameplay.ai.players.remove(&2);
        base.rules[0].initial_researches =
            c.technologies.section_names().map(str::to_owned).collect();
        for cell in g.cells.iter_mut() {
            *cell = 0x90;
        }
        let put = |m: &mut MissionWorld, sim: &mut Simulation, class: usize, owner: u8, at| {
            new_unit(sim, m, &lib, class, owner, civ, at).unwrap()
        };
        let enemy = put(&mut base, &mut base_sim, 3, 2, [24, 24]);
        let own = put(&mut base, &mut base_sim, class::HOUSE, 1, [50, 50]);
        base.objects[own].health = 1;
        let hostile_building = put(&mut base, &mut base_sim, class::HOUSE, 2, [50, 40]);
        let gold = put(&mut base, &mut base_sim, class::RES_GOLD, 0, [40, 50]);
        let item = put(&mut base, &mut base_sim, class::ITEM_HEALTH, 0, [22, 25]);
        base_sim.refresh_visibility(&base, None);
        let targets = Targets {
            enemy,
            own,
            hostile_building,
            gold,
            item,
        };
        for (class, kind) in CLASSES.iter().enumerate().skip(1) {
            if !(crate::world::class::is_unit(class)
                || (class::TOWER_STONE..=class::EVILCASTLE).contains(&class))
            {
                continue;
            }
            let (mut m, mut sim) = (base.clone(), base_sim.clone());
            let Ok(j) = new_unit(&mut sim, &mut m, &lib, class, 1, civ, [20, 20]) else {
                continue;
            };
            m.objects[j].flags &= !FLAG_CONSTRUCTION;
            m.objects[j].health = m.objects[j].max_health.max(1);
            sim.objects[j].mana = sim.objects[j].max_mana;
            m.simulation = Some(sim.clone());
            let mut state = HudState {
                selected: vec![j],
                ..Default::default()
            };
            let mut seen = BTreeSet::new();
            let mut menus = vec![None::<String>];
            while let Some(menu) = menus.pop() {
                if !seen.insert(menu.clone()) {
                    continue;
                }
                state.command_menu = menu.clone();
                for command in hud::commands(&skin, &locale, &state, &m, &lib) {
                    let at = format!(
                        "civ {civ}, {} ({class}), menu {:?}, {}",
                        kind, menu, command.order
                    );
                    if command.unavailable.is_some() {
                        continue;
                    }
                    if let Some(submenu) = command.submenu {
                        menus.push(Some(submenu));
                        continue;
                    }
                    if OWN.contains(&command.order.as_str()) {
                        continue;
                    }
                    pressed += 1;
                    match press(&c, &lib, &m, &g, &sim, j, &command.order, &targets) {
                        Err(e)
                            if e.contains("da completare")
                                || e.contains("non può eseguire")
                                || e == "Ordine non disponibile" =>
                        {
                            broken.push(format!("{at}: {e}"))
                        }
                        // The original's refusals tell why: no room, no mana,
                        // nothing aboard, an item the unit cannot carry.
                        _ => {}
                    }
                }
            }
        }
    }
    assert!(broken.is_empty(), "{}", broken.join("\n"));
    assert!(pressed > 300, "{pressed}");
}
