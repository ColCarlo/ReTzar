//! The original's developer commands, typed in its chat after «hm» (parser
//! 0x424c40, handler 0x424140 by code), here as the buttons of the cheat
//! panel. The panel and its switch in the options are this version's;
//! what each command does is the original's. Every command but «next»
//! passes the gate 0x4240e0, which in a game alone lets it through and
//! marks the player a cheater (0x5cf818).
use super::{
    Catalog, Request, Show, commanded_by, credit, housing_sum, movement, new_unit,
    population_parts, relation, researches, scripting, set_control, site_test, still,
    transfer_owner, vanish,
};
use crate::{
    map::MapGrid,
    simulation::Simulation,
    world::{FLAG_GOD, FLAG_LOCKED, MapObject, MissionWorld, ObjectLibrary, class},
};

/// One command of the panel, with the original's code and word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cheat {
    /// 1 «nofog» (0x4424d0): the fog goes for the rest of the game
    /// (0x5877a4 = 0).
    NoFog,
    /// 2 «reveal» (0x442420): every vertex of the map explored.
    Reveal,
    /// 3 «spawn» (0x4f3250): an object of the class, of civilization
    /// `civ`, put where the mouse clicks (message 0x112). The original
    /// gives it to the kingdom of the first selected peasant, else to the
    /// player's (0x4f32b3); the panel names the kingdom.
    Spawn { class: usize, civ: u8, owner: u8 },
    /// 4 «dvaleva» (0x424277): 10 000 of each resource; «petleva» sends
    /// the same command five times (0x424dc4).
    Resources(u8),
    /// 5 «buildozer»: instant building, on or off.
    InstantBuild,
    /// 6 «notech»: no requirements, on or off.
    NoTech,
    /// 7 «kingdom n» (0x42436e): the selected objects go to kingdom n
    /// (vt+0x274).
    Kingdom(u8),
    /// 8 «dset a b bit», 9 «dclr» (0x424404): a bit of a's relation toward
    /// b, set or cleared (0x468160, 0x490b90).
    Relation {
        from: u8,
        to: u8,
        bit: u8,
        set: bool,
    },
    /// 10 «nopop» (0x424530): housing and cap of the kingdom at 1000
    /// (K+0x60, K+0x62).
    NoPop,
    /// 12 «god» (0x4245e3): each selected object invulnerable, or no more
    /// (flag 0x200).
    God,
    /// 13 «time h» (0x42465a): the day clock to hour h (0x42b520).
    Time(u8),
    /// 14 «hunt» (0x4246ed): every unit of kingdom 10 leaves the map
    /// (0x4ff350).
    Hunt,
    /// 15 «usurp» (0x424774): the player commands each selected object,
    /// or no more (vt+0x2a0, vt+0x2a4).
    Usurp,
    /// 16 «spy» (0x4247fd): the player sees through each selected object,
    /// or no more (vt+0x298, vt+0x29c).
    Spy,
    /// 17 «discover name» (0x424886): the research completed at once
    /// (0x4d33d0).
    Discover(String),
}
/// The commands without their values, for what the panel offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    NoFog,
    Reveal,
    Spawn,
    Resources,
    InstantBuild,
    NoTech,
    Kingdom,
    Relation,
    NoPop,
    God,
    Time,
    Hunt,
    Usurp,
    Spy,
    Discover,
    /// «next» (0x424b60), carried out by the interface: the first map the
    /// scripts' `LoadMap` actions name (list 0xc93a44, type 0xb).
    Next,
}

/// Housing and cap «nopop» gives (0x42454f).
pub const POPULATION: usize = 1000;
/// What «dvaleva» gives of each resource (0x424298).
pub const RESOURCES: u32 = 10_000;

/// Whether the command can do anything in this game: the panel greys the
/// others. The objects' commands take the one clicked after them;
/// «discover» is the interface's to judge, with the list of researches.
pub fn pertinent(m: &MissionWorld, lib: &ObjectLibrary, kind: Kind) -> bool {
    let player = m.player();
    match kind {
        Kind::NoFog => !m.game_rules.no_fog,
        Kind::Reveal => m
            .simulation
            .as_ref()
            .is_some_and(|s| s.fog.explored_vertices.iter().any(|&e| !e)),
        Kind::Resources => !super::unlimited(m) && usize::from(player) <= m.kingdoms.len(),
        Kind::NoPop => {
            let (_, housing, cap) = population_parts(m, lib, player);
            housing != POPULATION || cap != POPULATION
        }
        Kind::Relation => m.kingdoms.len() >= 2,
        Kind::Hunt => m.simulation.as_ref().is_some_and(|s| {
            !crate::gameplay::ai::think::kingdom_list(s, m, crate::gameplay::DISCORD_OWNER, true)
                .is_empty()
        }),
        Kind::Next => m.campaign && next_map(m).is_some(),
        Kind::Spawn
        | Kind::InstantBuild
        | Kind::NoTech
        | Kind::Time
        | Kind::Discover
        | Kind::Kingdom
        | Kind::God
        | Kind::Usurp
        | Kind::Spy => true,
    }
}

/// The map «next» opens: the first `LoadMap` of the scripts (0x424b7d).
pub fn next_map(m: &MissionWorld) -> Option<&str> {
    m.script.actions.iter().find_map(|a| match &a.value {
        crate::script::Action::LoadMap(path) => Some(path.as_str()),
        _ => None,
    })
}

/// The classes «spawn» offers here for a civilization: the units and the
/// buildings it has a record of (the original takes any name of its table,
/// 0x42b250, and puts nothing for one without a record, 0x4f32f7).
pub fn spawn_classes(lib: &ObjectLibrary, civ: u8) -> Vec<usize> {
    (class::PEON..=class::MESSIAH)
        .chain([class::COW, class::FISHING_BOAT])
        .chain(class::TOWER_STONE..=class::EVILCASTLE)
        .filter(|&k| {
            lib.attributes
                .get(&crate::gameplay::ai::tree::probe(k, civ))
                .health
                > 0
        })
        .collect()
}

/// The object «spawn» puts at `cell` (0x497970): a unit on the cell, a
/// building around it as the pointer holds it (0x41ff19).
pub fn spawn_probe(
    lib: &ObjectLibrary,
    class: usize,
    civ: u8,
    owner: u8,
    cell: [u16; 2],
) -> Option<MapObject> {
    let mut ids = Default::default();
    let mut o =
        crate::world::starting::new_object(&mut ids, 1, class, owner, civ, cell, lib).ok()?;
    if o.max_health == 0 {
        return None;
    }
    if o.building() {
        let [w, h] = super::footprint(lib, &o);
        o.x = cell[0].checked_sub((w / 2) as u16)?;
        o.y = cell[1].checked_sub((h / 2) as u16)?;
    }
    Some(o)
}

/// The site test of the spawned object (vt+0x28c): a building's own, a
/// unit's cells free for its layer (0x505b20).
pub fn spawn_site(
    sim: &Simulation,
    m: &MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    o: &MapObject,
) -> bool {
    if o.building() {
        return site_test(sim, m, g, lib, o);
    }
    let cells = movement::cells_now(sim, m, g, lib);
    // 0x4e4bb4 → 0x4e5220 with no flags: a large unit is tested by vt+0x90
    // from its own cells, which leaves them all out; this version still
    // asks the first cell free in its layer, as for any unit.
    let [w, h] = if crate::world::class::is_unit(o.class) {
        [1, 1]
    } else {
        super::footprint(lib, o).map(|n| n as i32)
    };
    let layer = lib.layer(o);
    (0..h).all(|dy| (0..w).all(|dx| cells.free(layer, [o.x as i32 + dx, o.y as i32 + dy])))
}

/// The researches «discover» offers the player: those of the technology
/// table in its civilization's tree (the kingdom's record, 0x4976a0) not
/// yet completed.
pub fn researches_left(c: &Catalog, lib: &ObjectLibrary, m: &MissionWorld) -> Vec<String> {
    let player = m.player();
    let civ = m
        .kingdoms
        .get(usize::from(player).wrapping_sub(1))
        .map_or(1, |k| k.civilization.clamp(1, 3));
    let done = researches(m, player);
    let tree = crate::gameplay::ai::tree::names(c, lib, civ);
    c.technologies
        .section_names()
        .filter(|name| tree.contains(&name.to_ascii_lowercase()))
        .filter(|name| crate::simulation::completions(done, name) == 0)
        .map(str::to_owned)
        .collect()
}

/// A command of the panel, from the player (`req.cheat`, `req.selected`,
/// and the cell of a spawn).
pub(super) fn apply(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    g: &MapGrid,
    lib: &ObjectLibrary,
    c: &Catalog,
    req: &Request,
) -> std::result::Result<String, String> {
    let Some(cheat) = req.cheat.clone() else {
        return Err(String::new());
    };
    let player = m.player();
    // 0x4240e0: in a game alone the command goes through and the player
    // is a cheater from then on.
    m.cheater = true;
    let selected: Vec<usize> = req
        .selected
        .iter()
        .copied()
        .filter(|&i| i < m.objects.len())
        .collect();
    match cheat {
        Cheat::NoFog => {
            m.game_rules.no_fog = true;
            sim.refresh_visibility(m, None);
        }
        Cheat::Reveal => {
            sim.fog.explore_all();
            m.game_rules.explored = true;
            sim.refresh_visibility(m, None);
        }
        Cheat::Spawn { class, civ, owner } => {
            let cell = req.cell.ok_or_else(String::new)?;
            let probe = spawn_probe(lib, class, civ, owner, cell).ok_or_else(String::new)?;
            // 0x41fe91, 0x4e4bb4: a refused place puts nothing.
            if !spawn_site(sim, m, g, lib, &probe) {
                return Err(String::new());
            }
            let j = new_unit(sim, m, lib, class, owner, civ, [probe.x, probe.y])?;
            // 0x41feb8: a building comes as a site to build (+0x12 = 1),
            // its price not asked. The panel finishes it at once, as asked
            // of this version: what the gates take and the event of a
            // building done are the site's.
            if m.objects[j].building() {
                super::found_site(sim, m, lib, c, j);
                if m.objects[j].flags & crate::world::FLAG_CONSTRUCTION != 0 {
                    super::complete_site(sim, m, lib, j);
                }
            }
        }
        Cheat::Resources(times) => {
            for _ in 0..times {
                credit(m, player, [RESOURCES; 4]);
            }
        }
        Cheat::InstantBuild => {
            m.cheats.instant_build = !m.cheats.instant_build;
            // 0x424311: «Build at once: %s» (ON or OFF, 0x581ebc, never
            // translated) on the status line for 2000 ms.
            let word = if m.cheats.instant_build { "ON" } else { "OFF" };
            sim.gameplay.status = Some((format!("Build at once: {word}"), 2000));
        }
        Cheat::NoTech => m.cheats.no_tech = !m.cheats.no_tech,
        Cheat::Kingdom(k) => {
            for &j in &selected {
                transfer_owner(sim, m, lib, j, k);
            }
        }
        Cheat::Relation { from, to, bit, set } => {
            let kingdoms = m.kingdoms.len().min(8) as u8;
            if from == to || !(1..=kingdoms).contains(&from) || !(1..=kingdoms).contains(&to) {
                return Err("Regno non valido".into());
            }
            let r = relation(m, from, to);
            let mask = 1u32 << (bit & 31);
            let new = if set { r | mask } else { r & !mask };
            scripting::merge_relation(m, from, to, new);
            let outbox = &mut sim.gameplay.script.outbox;
            if outbox.len() < 1024 {
                outbox.push(Show::Diplomacy);
            }
        }
        Cheat::NoPop => {
            let bonus = supply_completions(m, player);
            let housing = housing_sum(m, lib, player);
            m.population_limit.insert(player, POPULATION);
            m.population_base.insert(player, housing);
            m.population_cap.insert(player, [POPULATION, bonus]);
        }
        Cheat::God => {
            for &j in &selected {
                m.objects[j].flags ^= FLAG_GOD;
            }
        }
        Cheat::Time(hour) => sim.set_hour(hour),
        Cheat::Hunt => {
            for j in crate::gameplay::ai::think::kingdom_list(
                sim,
                m,
                crate::gameplay::DISCORD_OWNER,
                true,
            ) {
                vanish(sim, m, j);
            }
        }
        Cheat::Usurp => {
            for &j in &selected {
                let v = commanded_by(m, j, player)
                    && m.objects[j].flags & FLAG_LOCKED == 0
                    && !still(sim, j);
                set_control(sim, m, j, player, if v { 2 } else { 1 });
            }
        }
        Cheat::Spy => {
            for &j in &selected {
                let v = crate::simulation::sees_through(m, &m.objects[j], player);
                if let Some(s) = m.objects[j]
                    .sight
                    .get_mut(usize::from(player).wrapping_sub(1))
                {
                    *s = if v { 2 } else { 1 };
                }
            }
            sim.refresh_visibility(m, None);
        }
        Cheat::Discover(name) => {
            // 0x4976a0: a name the kingdom has no record of does nothing.
            if !c
                .technologies
                .section_names()
                .any(|s| s.eq_ignore_ascii_case(&name))
            {
                return Err(String::new());
            }
            super::finish_research(sim, m, lib, player, &name);
        }
    }
    Ok(String::new())
}

/// The completions of Supply Storage and Seafood, each a quarter more of
/// the game's limit on the cap (0x4d3819, 0x4d385a).
pub(super) fn supply_completions(m: &MissionWorld, owner: u8) -> usize {
    let done = researches(m, owner);
    ["te_supplystorage", "te_seafood"]
        .iter()
        .map(|name| crate::simulation::completions(done, name) as usize)
        .sum()
}
