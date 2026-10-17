//! Holders against the original rules: garrisons and their healing,
//! passengers of a fallen holder, unloading, captures, the ninja's murder
//! and the spy hidden in a unit.

use super::tests::{request, setup};
use super::*;

fn ticks(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &mut MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    n: u64,
) {
    for _ in 0..n {
        advance(sim, m, g, lib, c, Duration::from_millis(TICK_MS), 10, false);
    }
}
fn class_of(name: &str) -> usize {
    CLASSES.iter().position(|&s| s == name).unwrap()
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn castles_take_in_and_heal_their_garrison() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [12, 7]).unwrap();
    m.objects[footman].health = m.objects[footman].max_health - 7;
    // 0x4e4280: a right click on a finished holder that accepts it.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![footman], [6, 6], Some(0)),
    )
    .unwrap();
    assert_eq!(sim.gameplay.actors[footman].intent, Some(Intent::Board(0)));
    let mut entered = None;
    for t in 0..400 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if sim.objects[footman].holder == Some(0) {
            entered = Some(t);
            break;
        }
    }
    assert!(entered.is_some());
    // 0x422a00: 2 a second, from the entry.
    let start = m.objects[footman].health;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 20);
    assert_eq!(m.objects[footman].health, start + 2);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 80);
    assert_eq!(m.objects[footman].health, m.objects[footman].max_health);
    assert_eq!(sim.gameplay.actors[0].heal_due_ms, None);
    // A trader never boards a carpet (0x4ed590).
    let trader = new_unit(&mut sim, &mut m, &lib, 2, 1, 1, [30, 30]).unwrap();
    let carpet = new_unit(&mut sim, &mut m, &lib, 22, 1, 1, [31, 30]).unwrap();
    assert!(!boardable(&sim, &m, &lib, &c, trader, carpet));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn passengers_of_a_fallen_castle_step_onto_free_land() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    let mut inside = Vec::new();
    for _ in 0..3 {
        let k = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
        garrison(&mut sim, &mut m, 0, k);
        inside.push(k);
    }
    movement::refresh(&mut sim, &m, &g, &lib);
    let cells = sim.gameplay.pathing.cells.clone();
    m.objects[0].health = 1;
    apply_damage(&mut sim, &mut m, &g, &lib, &c, 0, 5000, 3, None);
    let mut taken = BTreeSet::new();
    for k in inside {
        let o = &m.objects[k];
        assert_eq!(sim.objects[k].holder, None);
        assert!(alive(o));
        // Each on its own land cell the castle's picture leaves free (its
        // corners are), within 8 rings of the castle's centre.
        let p = [o.x as i32, o.y as i32];
        assert!(cells.spawnable(p, 1), "{p:?}");
        assert!(taken.insert(p));
    }
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn captures_go_on_until_the_building_is_whole() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let house = new_unit(&mut sim, &mut m, &lib, class_of("house"), 2, 1, [40, 40]).unwrap();
    m.objects[house].flags &= !FLAG_CONSTRUCTION;
    position(&mut m.objects[1], [39, 40]);
    // One peasant takes 5 a cycle while the house gets back 10 every 2 s
    // (0x41f271): start at the last 5.
    sim.objects[house].resistance = 5;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("capture", vec![1], [40, 40], Some(house)),
    )
    .unwrap();
    let full = resistance_target(&sim, &m, &lib, house);
    for _ in 0..2000 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if sim.gameplay.actors[1].intent.is_none() {
            break;
        }
    }
    // Taken at once (0x505040), then +5 a cycle back to the target.
    assert_eq!(m.objects[house].owner, 1);
    assert_eq!(sim.objects[house].resistance as i32, full);
    assert_eq!(sim.gameplay.actors[1].intent, None);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_peasant_right_click_restores_friends_and_takes_neutral_towers() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    m.diplomacy[1] = 0x20;
    position(&mut m.objects[1], [39, 40]);
    let context = |sim: &mut Simulation, m: &mut MissionWorld, j: usize| {
        let at = [m.objects[j].x, m.objects[j].y];
        issue(
            sim,
            m,
            &g,
            &lib,
            &c,
            &request("context", vec![1], at, Some(j)),
        )
        .unwrap();
        sim.gameplay.actors[1].intent.clone()
    };
    // 0x4682e0: an own building below its resistance target is captured
    // back to it; a whole one is not.
    let house = new_unit(&mut sim, &mut m, &lib, class_of("house"), 1, 1, [40, 40]).unwrap();
    m.objects[house].flags &= !FLAG_CONSTRUCTION;
    sim.objects[house].resistance = 10;
    assert_eq!(
        context(&mut sim, &mut m, house),
        Some(Intent::Capture(house))
    );
    sim.objects[house].resistance = resistance_target(&sim, &m, &lib, house) as u16;
    assert_ne!(
        context(&mut sim, &mut m, house),
        Some(Intent::Capture(house))
    );
    // A neutral kingdom's stone or wooden tower is taken, its castle not.
    let tower = new_unit(
        &mut sim,
        &mut m,
        &lib,
        class_of("tower_wood"),
        2,
        1,
        [44, 40],
    )
    .unwrap();
    m.objects[tower].flags &= !FLAG_CONSTRUCTION;
    assert_eq!(
        context(&mut sim, &mut m, tower),
        Some(Intent::Capture(tower))
    );
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 2, 1, [44, 46]).unwrap();
    m.objects[castle].flags &= !FLAG_CONSTRUCTION;
    assert_ne!(
        context(&mut sim, &mut m, castle),
        Some(Intent::Capture(castle))
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_gateway_takes_the_aura_of_a_castle_and_the_field_does_not() {
    let (_, lib, mut m, _, mut sim) = setup(1);
    // 0x41f920 asks 0x272a first: the base building's classes, the magic
    // gateway (vtable 0x575834) included, take the EMIT of their
    // neighbours; the field and the dummy answer 1.
    let castle = new_unit(&mut sim, &mut m, &lib, class::CASTLE, 1, 1, [40, 40]).unwrap();
    m.objects[castle].flags &= !FLAG_CONSTRUCTION;
    let house = new_unit(&mut sim, &mut m, &lib, class_of("house"), 1, 1, [46, 40]).unwrap();
    m.objects[house].flags &= !FLAG_CONSTRUCTION;
    let gateway = new_unit(&mut sim, &mut m, &lib, class::MAGIC_GATEWAY, 1, 1, [40, 46]).unwrap();
    m.objects[gateway].flags &= !FLAG_CONSTRUCTION;
    let field = new_unit(&mut sim, &mut m, &lib, class::FIELD, 1, 1, [34, 40]).unwrap();
    m.objects[field].flags &= !FLAG_CONSTRUCTION;
    let aura = auras(&m, &lib);
    assert_eq!(aura[house], 400);
    assert_eq!(aura[gateway], 400);
    assert_eq!(aura[field], 0);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_spy_takes_no_machine_animal_ninja_or_flyer() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let spy = new_unit(&mut sim, &mut m, &lib, 14, 1, 1, [30, 30]).unwrap();
    // 0x44ecc0 and vt+0xc0 (0x4e6010, 0x4ba9f0).
    for (k, (name, ok)) in [
        ("catapult", false),
        ("bear", false),
        ("ninja", false),
        ("bat", false),
        ("footman", true),
    ]
    .into_iter()
    .enumerate()
    {
        let x = new_unit(
            &mut sim,
            &mut m,
            &lib,
            class_of(name),
            2,
            1,
            [32 + 2 * k as u16, 30],
        )
        .unwrap();
        let at = [m.objects[x].x, m.objects[x].y];
        let r = issue(
            &mut sim,
            &mut m,
            &g,
            &lib,
            &c,
            &request("impersonate", vec![spy], at, Some(x)),
        );
        assert_eq!(r.is_ok(), ok, "{name}");
    }
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_ninja_kills_with_one_blow_of_fifteen() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let ninja = new_unit(&mut sim, &mut m, &lib, 13, 1, 3, [30, 30]).unwrap();
    let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [31, 30]).unwrap();
    m.objects[foe].max_health = 5000;
    m.objects[foe].health = 5000;
    sim.gameplay.actors[foe].stance = 2;
    sim.objects[ninja].max_mana = 100;
    sim.objects[ninja].mana = 60;
    let attack = sim.effective_attributes(ninja, &m).unwrap().attack;
    let defence = sim.effective_attributes(foe, &m).unwrap().defence;
    let murder = request("murder", vec![ninja], [31, 30], Some(foe));
    sim.gameplay.actors[ninja].intent = Some(Intent::Murder(foe));
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    // Op 0x5b: attack × 15 through the armour, all the mana spent.
    assert_eq!(
        5000 - m.objects[foe].health,
        physical_damage((attack * 15) as u16, defence, 0)
    );
    assert_eq!(sim.objects[ninja].mana, 0);
    // The program ends; the default program then strikes the foe in reach.
    assert_ne!(sim.gameplay.actors[ninja].intent, Some(Intent::Murder(foe)));
    // Without 60 mana: nothing, and the original message.
    let before = m.objects[foe].health;
    let _ = issue(&mut sim, &mut m, &g, &lib, &c, &murder);
    sim.gameplay.actors[ninja].intent = Some(Intent::Murder(foe));
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    assert_eq!(m.objects[foe].health, before);
    assert!(sim.gameplay.notices.contains(&c.notmanna));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_ninja_with_60_mana_murders_what_it_is_ordered_to_attack() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let ninja = new_unit(&mut sim, &mut m, &lib, 13, 1, 3, [30, 30]).unwrap();
    let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [31, 30]).unwrap();
    m.objects[foe].max_health = 5000;
    m.objects[foe].health = 5000;
    sim.gameplay.actors[foe].stance = 2;
    sim.objects[ninja].max_mana = 100;
    sim.objects[ninja].mana = 60;
    let attack = sim.effective_attributes(ninja, &m).unwrap().attack;
    let defence = sim.effective_attributes(foe, &m).unwrap().defence;
    // 0x4eb950: the right click's Attack program strikes with Murder.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![ninja], [31, 30], Some(foe)),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(
        5000 - m.objects[foe].health,
        physical_damage((attack * 15) as u16, defence, 0)
    );
    assert_eq!(sim.objects[ninja].mana, 0);
    // The program goes on as an attack (0x4eb9d0).
    assert_eq!(sim.gameplay.actors[ninja].intent, Some(Intent::Attack(foe)));
    assert!(!sim.gameplay.actors[ninja].murder_blow);
    // Its next blow is an ordinary one, after the picture's lead.
    let after = m.objects[foe].health;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    assert_eq!(m.objects[foe].health, after);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 100);
    assert!(m.objects[foe].health < after);
    assert!(after - m.objects[foe].health < physical_damage((attack * 15) as u16, defence, 0));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn spies_hide_in_a_unit_and_come_out_again() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let spy = new_unit(&mut sim, &mut m, &lib, 14, 1, 1, [30, 30]).unwrap();
    let host = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [31, 30]).unwrap();
    sim.gameplay.actors[host].stance = 2;
    assert_eq!(sim.gameplay.actors[spy].spy_energy, 100);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("impersonate", vec![spy], [31, 30], Some(host)),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    // 0x4e4510: the spy is gone, the footman stays the enemy's but the
    // spy's kingdom commands it.
    assert!(!alive(&m.objects[spy]));
    assert_eq!(m.objects[host].owner, 2);
    assert_eq!(m.objects[host].mole, 1);
    assert!(m.commands(&m.objects[host]));
    assert_eq!(sim.gameplay.actors[host].mole.as_ref().unwrap().energy, 0);
    // From the host, Travestiti (op 0x60) moves the hidden spy on once it
    // has 60 energy again; the old host stays its kingdom's.
    let other = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [32, 30]).unwrap();
    sim.gameplay.actors[other].stance = 2;
    let jump = request("impersonate", vec![host], [32, 30], Some(other));
    issue(&mut sim, &mut m, &g, &lib, &c, &jump).unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    assert_eq!(m.objects[other].mole, 0);
    sim.gameplay.actors[host].mole.as_mut().unwrap().energy = 60;
    issue(&mut sim, &mut m, &g, &lib, &c, &jump).unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    assert_eq!((m.objects[host].mole, m.objects[other].mole), (0, 1));
    assert!(alive(&m.objects[host]) && !m.commands(&m.objects[host]));
    let host = other;
    // Op 0x61: the spy comes back on the host's cell, the host is gone.
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("reveal_self", vec![host]),
    )
    .unwrap();
    assert!(!alive(&m.objects[host]));
    let back = m.objects.len() - 1;
    assert_eq!((m.objects[back].class, m.objects[back].owner), (14, 1));
    assert_eq!([m.objects[back].x, m.objects[back].y], [32, 30]);
    // Bribes: never a peasant, any other unit not of the kingdom (0x437450).
    let peon = new_unit(&mut sim, &mut m, &lib, class::PEON, 2, 1, [40, 40]).unwrap();
    let knight = new_unit(&mut sim, &mut m, &lib, class_of("knight"), 0, 1, [42, 40]).unwrap();
    assert!(!bribable(&m, 1, peon));
    assert!(bribable(&m, 1, knight));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn resource_items_left_in_the_castle_go_to_its_kingdom() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [14, 8]).unwrap();
    let storage = new_unit(&mut sim, &mut m, &lib, 51, 1, 1, [20, 14]).unwrap();
    m.objects[storage].flags &= !FLAG_CONSTRUCTION;
    let carry = |sim: &mut Simulation, m: &mut MissionWorld, g: &MapGrid, kind: u8| {
        let item = spawn_resources(sim, m, g, &lib, [30, 30], kind, 50, "item_gold").unwrap();
        sim.objects[footman].contents.push(item);
        sim.objects[item].holder = Some(footman);
        m.objects[item].flags |= FLAG_HELD;
        item
    };
    // 0x464fdc: the castle (object 0) gives its kingdom any kind.
    let gold = carry(&mut sim, &mut m, &g, 0);
    let before = m.kingdoms[0].resources[0];
    let mut drop = request("item_drop", vec![gold], [6, 6], Some(0));
    drop.selected = vec![gold];
    issue(&mut sim, &mut m, &g, &lib, &c, &drop).unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 300);
    assert_eq!(m.kingdoms[0].resources[0], before + 50);
    assert!(m.objects[gold].flags & FLAG_INACTIVE != 0);
    // The storage has no room (no `max_obj_in_holder`): the item cannot
    // enter it (0x504440) and lands beside the carrier, nothing credited.
    let wood = carry(&mut sim, &mut m, &g, 3);
    let before = m.kingdoms[0].resources[3];
    let mut drop = request("item_drop", vec![wood], [20, 14], Some(storage));
    drop.selected = vec![wood];
    issue(&mut sim, &mut m, &g, &lib, &c, &drop).unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 300);
    assert_eq!(m.kingdoms[0].resources[3], before);
    assert_eq!(sim.objects[wood].holder, None);
    assert!(m.objects[wood].flags & FLAG_INACTIVE == 0);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn scrolls_open_their_story_when_picked_up_unless_marked_minus_one() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [14, 8]).unwrap();
    let scroll = |sim: &mut Simulation, m: &mut MissionWorld, mark: u32, p: [u16; 2]| {
        let j = new_unit(sim, m, &lib, class::ITEM_SCROLL, 0, 0, p).unwrap();
        let data = &mut m.objects[j].data;
        data.resize(0x528, 0);
        data[0x60..0x65].copy_from_slice(b"Hello");
        data[0x520..0x524].copy_from_slice(&mark.to_le_bytes());
        j
    };
    // 0x463850: -1 keeps the story for its use.
    let quiet = scroll(&mut sim, &mut m, u32::MAX, [15, 8]);
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("pick_item", vec![footman], [15, 8], Some(quiet)),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 60);
    assert_eq!(sim.objects[quiet].holder, Some(footman));
    assert!(m.message.is_none());
    // 0x4532f0: what enters a holder leaves the selection.
    assert!(
        sim.gameplay
            .script
            .outbox
            .iter()
            .any(|s| matches!(s, Show::Unselect(j) if *j == quiet))
    );
    // Any other value opens it at once (0x4638c4), with the record's music
    // (0x4e0) and closing (0x524).
    let told = scroll(&mut sim, &mut m, 0, [16, 8]);
    m.objects[told].data[0x4e0..0x4e9].copy_from_slice(b"USER\\a.mi");
    m.objects[told].data[0x524] = 1;
    sim.gameplay.script.outbox.0.clear();
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("pick_item", vec![footman], [16, 8], Some(told)),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 60);
    assert_eq!(sim.objects[told].holder, Some(footman));
    assert_eq!(m.message.as_ref().unwrap().text, "Hello");
    assert!(sim.gameplay.script.modal);
    let story = |sim: &Simulation| {
        sim.gameplay.script.outbox.iter().find_map(|s| match s {
            Show::Story { close, music, .. } => Some((*close, music.clone())),
            _ => None,
        })
    };
    assert_eq!(story(&sim), Some((true, "USER\\a.mi".to_owned())));
    // Used (0x4639b0), it shows again, never closing by itself.
    sim.gameplay.script.outbox.0.clear();
    sim.gameplay.script.modal = false;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("item_use", vec![told]),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    assert_eq!(story(&sim), Some((false, "USER\\a.mi".to_owned())));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_drunk_potion_sounds_and_sparkles_around_the_drinker() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sim.fog.explore_all();
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [14, 8]).unwrap();
    let potion = new_unit(&mut sim, &mut m, &lib, class::ITEM_HEALTH, 0, 0, [1, 1]).unwrap();
    m.objects[potion].data.resize(0x6a, 0);
    m.objects[potion].data[0x12..0x14].copy_from_slice(&50u16.to_le_bytes());
    sim.objects[footman].contents.push(potion);
    sim.objects[potion].holder = Some(footman);
    m.objects[potion].flags |= FLAG_HELD;
    sim.flashes.clear();
    sim.sounds.clear();
    let mut use_it = Request::command("item_use", vec![potion]);
    use_it.selected = vec![potion];
    issue(&mut sim, &mut m, &g, &lib, &c, &use_it).unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 2);
    // 0x464b94: the healing sound, then ten stars over the cells around the
    // drinker, each twinkle over within 2 s.
    assert!(sim.sounds.iter().any(|s| s.property == "@spells/heal.wav"));
    let stars: Vec<&Flash> = sim
        .flashes
        .iter()
        .filter(|f| f.image == "magic_star")
        .collect();
    assert!((11..=20).contains(&stars.len()), "{}", stars.len());
    assert_eq!(stars[0].delay_ms, 0);
    assert!(stars.iter().all(|f| {
        f.delay_ms + 1000 <= 2000
            && (13 * 32 + 1..=16 * 32 - 12).contains(&f.at[0])
            && (7 * 32 + 1..=9 * 32 - 12).contains(&f.at[1])
    }));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn passengers_step_off_where_they_came_in_until_the_holder_moves() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    sync(&mut sim, &mut m, &lib);
    // 0x5044be: the castle keeps the cell each one came in by.
    let rect = movement::footprint_rect(&lib, &m.objects[0]);
    let door = [rect[2] as u16 + 1, rect[1] as u16 + 1];
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, door).unwrap();
    garrison(&mut sim, &mut m, 0, footman);
    assert_eq!(sim.objects[footman].entry, Some(door));
    movement::refresh(&mut sim, &m, &g, &lib);
    // 0x5037fa → 0x504550: out on that very cell, then into the selection
    // (0x453240).
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("emerge", vec![0]),
    )
    .unwrap();
    assert_eq!(sim.objects[footman].holder, None);
    assert_eq!([m.objects[footman].x, m.objects[footman].y], door);
    assert_eq!(sim.objects[footman].entry, None);
    assert!(
        sim.gameplay
            .script
            .outbox
            .iter()
            .any(|s| *s == Show::Select(footman))
    );
    // 0x500acc: a holder that moves wipes the cell; the way out is then
    // around the holder's own rectangle (0x5075a8), never on it.
    let carpet = new_unit(&mut sim, &mut m, &lib, 22, 1, 1, [30, 30]).unwrap();
    let rider = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [29, 30]).unwrap();
    garrison(&mut sim, &mut m, carpet, rider);
    assert_eq!(sim.objects[rider].entry, Some([29, 30]));
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("move", vec![carpet], [40, 30], None),
    )
    .unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 400);
    assert_eq!([m.objects[carpet].x, m.objects[carpet].y], [40, 30]);
    assert_eq!(sim.objects[rider].entry, None);
    // 0x500ac2: the rider takes each cell the carpet takes.
    assert_eq!([m.objects[rider].x, m.objects[rider].y], [40, 30]);
    assert!(step_off(&mut sim, &mut m, &lib, carpet, rider));
    let o = &m.objects[rider];
    let ring = (o.x as i32 - 40).abs().max((o.y as i32 - 30).abs());
    assert_eq!(ring, 1, "{:?}", [o.x, o.y]);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_ship_comes_to_meet_the_units_sent_aboard() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    for y in 20..40 {
        for x in 30..40 {
            g.cells[y * 64 + x] = 0;
        }
    }
    let ship = new_unit(&mut sim, &mut m, &lib, 23, 1, 1, [35, 30]).unwrap();
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [25, 30]).unwrap();
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![footman], [35, 30], Some(ship)),
    )
    .unwrap();
    assert_eq!(
        sim.gameplay.actors[footman].intent,
        Some(Intent::Board(ship))
    );
    // 0x4e85d0 (vt+0x1a0 of the ship).
    assert_eq!(
        sim.gameplay.actors[ship].intent,
        Some(Intent::Move([25, 30]))
    );
}

/// The intent a right click of `unit` on object `j` gives.
fn clicked(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    unit: usize,
    j: usize,
) -> Option<Intent> {
    let at = [m.objects[j].x, m.objects[j].y];
    issue(
        sim,
        m,
        g,
        lib,
        c,
        &request("context", vec![unit], at, Some(j)),
    )
    .unwrap();
    sim.gameplay.actors[unit].intent.clone()
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn traders_unload_at_markets_and_join_friendly_routes() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let market = new_unit(&mut sim, &mut m, &lib, class::MARKET, 1, 1, [30, 30]).unwrap();
    let far = new_unit(&mut sim, &mut m, &lib, class::MARKET, 1, 1, [50, 30]).unwrap();
    for k in [market, far] {
        m.objects[k].flags &= !FLAG_CONSTRUCTION;
    }
    let trader = new_unit(&mut sim, &mut m, &lib, 2, 1, 1, [36, 36]).unwrap();
    let load = |sim: &mut Simulation, amount: u32| {
        let a = &mut sim.gameplay.actors[trader];
        a.route = Some(Route {
            resources: [3, 0],
            amount: 300,
            cargo: amount,
            cargo_type: 3,
            ..Default::default()
        });
        a.trade_origin = Some(far);
        a.trade_partner = Some(market);
    };
    load(&mut sim, 300);
    // 0x4ed4e0: on a market, Unload Resource; the Trade op with argument 0
    // sells the load to the market's kingdom with no profit (0x4e7cdb).
    assert_eq!(
        clicked(&mut sim, &mut m, &g, &lib, &c, trader, market),
        Some(Intent::Sell(market))
    );
    let before = m.kingdoms[0].resources;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 300);
    assert_eq!(m.kingdoms[0].resources[3], before[3] + 300);
    assert_eq!(sim.gameplay.actors[trader].route.as_ref().unwrap().cargo, 0);
    assert_eq!(sim.gameplay.actors[trader].intent, None);
    // At a castle the op does not start (vt+0x264 = 0x407090): the trader
    // walks there and keeps its goods.
    load(&mut sim, 200);
    assert_eq!(
        clicked(&mut sim, &mut m, &g, &lib, &c, trader, 0),
        Some(Intent::Sell(0))
    );
    let before = m.kingdoms[0].resources;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 400);
    assert_eq!(m.kingdoms[0].resources, before);
    assert_eq!(
        sim.gameplay.actors[trader].route.as_ref().unwrap().cargo,
        200
    );
    assert_eq!(sim.gameplay.actors[trader].intent, None);
    // 0x4e8220: units carrying trade goods join a friendly trader's route,
    // its load cut to their room (0x5042f8) and their own goods kept.
    sim.gameplay.actors[trader].intent = Some(Intent::Trade(far));
    let second = new_unit(&mut sim, &mut m, &lib, 2, 1, 1, [36, 38]).unwrap();
    assert_eq!(
        clicked(&mut sim, &mut m, &g, &lib, &c, second, trader),
        Some(Intent::Trade(far))
    );
    let r = sim.gameplay.actors[second].route.clone().unwrap();
    assert_eq!((r.resources, r.amount, r.cargo), ([3, 0], 300, 0));
    assert_eq!(sim.gameplay.actors[second].trade_partner, Some(market));
    let asian = new_unit(&mut sim, &mut m, &lib, class::PEON, 1, 3, [36, 40]).unwrap();
    assert_eq!(
        clicked(&mut sim, &mut m, &g, &lib, &c, asian, trader),
        Some(Intent::Trade(far))
    );
    // Neither a European peasant nor a footman carries trade goods.
    for (class, civ) in [(class::PEON, 1), (class::FOOTMAN, 1)] {
        let k = new_unit(&mut sim, &mut m, &lib, class, 1, civ, [38, 40]).unwrap();
        assert_ne!(
            clicked(&mut sim, &mut m, &g, &lib, &c, k, trader),
            Some(Intent::Trade(far))
        );
    }
    // An enemy's route is not joined (relation bit 0x10).
    let foreign = new_unit(&mut sim, &mut m, &lib, 2, 2, 1, [44, 44]).unwrap();
    sim.gameplay.actors[foreign] = sim.gameplay.actors[trader].clone();
    assert_ne!(
        clicked(&mut sim, &mut m, &g, &lib, &c, second, foreign),
        Some(Intent::Trade(far))
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn ships_unload_at_allied_harbors_only() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let own = new_unit(&mut sim, &mut m, &lib, class::HARBOR, 1, 1, [30, 30]).unwrap();
    let foe = new_unit(&mut sim, &mut m, &lib, class::HARBOR, 2, 1, [40, 30]).unwrap();
    for class in [23, 24, 25, class::FISHING_BOAT] {
        let ship = new_unit(&mut sim, &mut m, &lib, class, 1, 1, [35, 40]).unwrap();
        // 0x4e96e0: Unload Resource at a harbor of an ally (relation 0x40).
        assert_eq!(
            clicked(&mut sim, &mut m, &g, &lib, &c, ship, own),
            Some(Intent::Sell(own))
        );
        assert_ne!(
            clicked(&mut sim, &mut m, &g, &lib, &c, ship, foe),
            Some(Intent::Sell(foe))
        );
    }
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_loaded_peasant_walks_to_a_market_and_keeps_its_load() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let market = new_unit(&mut sim, &mut m, &lib, class::MARKET, 2, 1, [30, 30]).unwrap();
    m.objects[market].flags &= !FLAG_CONSTRUCTION;
    position(&mut m.objects[1], [40, 40]);
    sim.gameplay.actors[1].cargo = [0, 0, 0, 30];
    // 0x4add9f: Return goods to any finished market or harbor (vt+0x260)…
    assert_eq!(
        clicked(&mut sim, &mut m, &g, &lib, &c, 1, market),
        Some(Intent::Deliver(market))
    );
    let before = (m.kingdoms[0].resources, m.kingdoms[1].resources);
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 400);
    // …where deliverres (0x4ae990) takes nothing.
    assert_eq!(sim.gameplay.actors[1].intent, None);
    assert_eq!(sim.gameplay.actors[1].cargo, [0, 0, 0, 30]);
    assert_eq!((m.kingdoms[0].resources, m.kingdoms[1].resources), before);
    assert!(distance(&lib, &m.objects[1], &m.objects[market]) <= 1);
    // Owner 0 has no kingdom to pay or be paid (0x467400).
    let kept = m.kingdoms.iter().map(|k| k.resources).collect::<Vec<_>>();
    credit(&mut m, 0, [5; 4]);
    assert!(debit(&c, &mut m, 0, [1; 4]).is_err());
    assert_eq!(
        m.kingdoms.iter().map(|k| k.resources).collect::<Vec<_>>(),
        kept
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn the_pointer_shows_the_cursor_of_the_program_a_right_click_starts() {
    let (c, lib, mut m, g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    sync(&mut sim, &mut m, &lib);
    let cursor = |sim: &Simulation, m: &MissionWorld, unit: usize, j: usize, alt: bool| {
        let at = [m.objects[j].x, m.objects[j].y];
        let action = right_click(sim, m, &g, &lib, &c, unit, Some(j), Some(at));
        click_cursor(m, action, Some(j), alt)
    };
    let cow = new_unit(&mut sim, &mut m, &lib, class::COW, 1, 1, [20, 20]).unwrap();
    let mine = new_unit(&mut sim, &mut m, &lib, class::RES_GOLD, 0, 0, [24, 20]).unwrap();
    let site = new_unit(&mut sim, &mut m, &lib, class_of("house"), 1, 1, [30, 20]).unwrap();
    m.objects[site].flags |= FLAG_CONSTRUCTION;
    let foe = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 2, 1, [34, 20]).unwrap();
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [36, 20]).unwrap();
    // 0x42da60: Tend cows keeps the arrow, Mine and Build harvest.
    assert_eq!(cursor(&sim, &m, 1, cow, false), "");
    assert_eq!(cursor(&sim, &m, 1, mine, false), "cs_harvest");
    assert_eq!(cursor(&sim, &m, 1, site, false), "cs_harvest");
    // Attack, or Attack type with Alt; a walk keeps the arrow.
    assert_eq!(cursor(&sim, &m, footman, foe, false), "cs_attack");
    assert_eq!(cursor(&sim, &m, footman, foe, true), "cs_attacktype");
    assert_eq!(cursor(&sim, &m, footman, cow, false), "");
    // Capture shows `cs_magicharm` (program 9).
    let tower = new_unit(
        &mut sim,
        &mut m,
        &lib,
        class_of("tower_wood"),
        2,
        1,
        [40, 20],
    )
    .unwrap();
    m.objects[tower].flags &= !FLAG_CONSTRUCTION;
    assert_eq!(
        right_click(&sim, &m, &g, &lib, &c, 1, Some(tower), None),
        "capture"
    );
    assert_eq!(
        click_cursor(&m, "capture", Some(tower), false),
        "cs_magicharm"
    );
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn passengers_go_down_with_a_ship_sunk_far_from_land() {
    let (_, lib, mut m, g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let mut g = g;
    for y in 10..60 {
        for x in 10..60 {
            g.cells[y * 64 + x] = 0;
        }
    }
    let board = |sim: &mut Simulation, m: &mut MissionWorld, ship: usize, j: usize| {
        sim.objects[ship].contents.push(j);
        sim.objects[j].holder = Some(ship);
        m.objects[j].flags |= FLAG_HELD;
    };
    // 0x4e9421: no land in the ship's rect grown by 5.
    let far = new_unit(&mut sim, &mut m, &lib, 23, 1, 1, [35, 35]).unwrap();
    let lost = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [35, 35]).unwrap();
    board(&mut sim, &mut m, far, lost);
    kill(&mut sim, &mut m, &g, &lib, far);
    assert!(!alive(&m.objects[lost]));
    assert_eq!(m.objects[lost].flags & FLAG_DYING, 0);
    // Land 6 cells off the ship (5 off its rect): they step ashore.
    let near = new_unit(&mut sim, &mut m, &lib, 23, 1, 1, [15, 35]).unwrap();
    let saved = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [15, 35]).unwrap();
    board(&mut sim, &mut m, near, saved);
    kill(&mut sim, &mut m, &g, &lib, near);
    assert!(alive(&m.objects[saved]));
    assert_eq!(sim.objects[saved].holder, None);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_ninja_stays_hidden_on_its_own_mana() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    let ninja = new_unit(&mut sim, &mut m, &lib, class::NINJA, 1, 3, [30, 30]).unwrap();
    sim.objects[ninja].max_mana = 100;
    sim.objects[ninja].mana = 100;
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &Request::command("cloak", vec![ninja]),
    )
    .unwrap();
    let paid = sim.objects[ninja].mana;
    assert!(sim.gameplay.actors[ninja].cloak_drain);
    // 0x4ebba9: 1 mana every 2000 ms, still hidden, no regeneration
    // (0x4ebb91).
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 200);
    assert!(sim.gameplay.actors[ninja].effect_until > sim.ticks);
    assert_eq!(sim.objects[ninja].mana, paid - 5);
    // Out of mana, it is seen at the next payment.
    sim.objects[ninja].mana = 0;
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 41);
    assert!(sim.gameplay.actors[ninja].effect_until <= sim.ticks);
    assert!(!sim.gameplay.actors[ninja].cloak_drain);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn idle_ninjas_and_spies_unmask_hidden_enemies() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    // The watcher, our ninja at [30, 30].
    new_unit(&mut sim, &mut m, &lib, class::NINJA, 1, 3, [30, 30]).unwrap();
    let hidden = new_unit(&mut sim, &mut m, &lib, class::NINJA, 2, 3, [33, 30]).unwrap();
    let friend = new_unit(&mut sim, &mut m, &lib, class::NINJA, 1, 3, [30, 33]).unwrap();
    for k in [hidden, friend] {
        sim.gameplay.actors[k].effect_until = sim.ticks + 100_000;
    }
    // (Held still, the enemy cannot unmask the friend in turn.)
    sim.gameplay.actors[hidden].frozen_until = sim.ticks + 1000;
    // 0x4eb7f0 → 0x4eb730: at its idle tick the ninja makes the hidden
    // enemy in its sight show itself; a friend stays hidden.
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 40);
    assert!(sim.gameplay.actors[hidden].effect_until <= sim.ticks);
    assert!(sim.gameplay.actors[friend].effect_until > sim.ticks);
    // A ninja's idle ticks come 2000 ms apart after the first.
    let lone = new_unit(&mut sim, &mut m, &lib, class::NINJA, 1, 3, [55, 10]).unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
    let due = sim.gameplay.actors[lone].scan_due_ms.unwrap();
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 30);
    assert_eq!(sim.gameplay.actors[lone].scan_due_ms, Some(due + 2000));
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_pick_up_the_unit_cannot_reach_ends() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    // An item on a pond: no land unit walks onto it.
    for y in 26..=34 {
        for x in 36..=44 {
            g.cells[y * 64 + x] = 0;
        }
    }
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [30, 30]).unwrap();
    let item = new_unit(&mut sim, &mut m, &lib, class::ITEM_HEALTH, 0, 0, [40, 30]).unwrap();
    set_order(&mut sim, footman, Intent::Pick(item), false).unwrap();
    // Its Move op fails and the program ends (0x42f750), as other walks do.
    let mut ended = false;
    for _ in 0..400 {
        ticks(&mut sim, &mut m, &mut g, &lib, &c, 1);
        if sim.gameplay.actors[footman].intent.is_none() {
            ended = true;
            break;
        }
    }
    assert!(ended, "{:?}", sim.gameplay.actors[footman].intent);
    assert_eq!(sim.objects[item].holder, None);
}

#[test]
#[ignore = "Requires the installed original game archives"]
fn a_unit_boards_a_ship_across_a_cell_of_shore() {
    let (c, lib, mut m, mut g, mut sim) = setup(1);
    m.game_rules.no_fog = true;
    // Water from column 30, a column of shore (its second surface water,
    // the first ground: kind 3, 0x476da0) at 29, land before it.
    for y in 20..40 {
        g.cells[y * 64 + 29] = 1 << 4;
        for x in 30..40 {
            g.cells[y * 64 + x] = 0;
        }
    }
    let ship = new_unit(&mut sim, &mut m, &lib, 23, 1, 1, [30, 30]).unwrap();
    let footman = new_unit(&mut sim, &mut m, &lib, class::FOOTMAN, 1, 1, [27, 30]).unwrap();
    issue(
        &mut sim,
        &mut m,
        &g,
        &lib,
        &c,
        &request("context", vec![footman], [30, 30], Some(ship)),
    )
    .unwrap();
    // Neither can tread the shore. The Enter program (0x42e2b0) walks with
    // range 1 to the ship's rect (0x42e318, 0x502550): one cell short of
    // it is aboard.
    ticks(&mut sim, &mut m, &mut g, &lib, &c, 200);
    assert_eq!(
        sim.objects[footman].holder,
        Some(ship),
        "{:?} {:?}",
        [m.objects[footman].x, m.objects[footman].y],
        [m.objects[ship].x, m.objects[ship].y]
    );
}
