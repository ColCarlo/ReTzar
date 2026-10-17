//! The engine's hooks on a computer player's units: the program a unit
//! takes against a target (vt+0x2b0) and the items a fighter picks up as
//! it walks (0x4de310).
use super::*;

/// A ninja needs this much mana to murder (0x415821).
const MURDER_MANA: u16 = 60;
/// A unit with this much health is worth murdering (0x41584d).
const MURDER_HEALTH: u16 = 100;

/// 0x415800: the computer's ninja murders the target instead of attacking
/// it: a unit, the ninja with 60 mana, the target a mage, a hero or of 100
/// health or more.
fn murders(sim: &Simulation, m: &MissionWorld, u: usize, t: usize) -> bool {
    let x = &m.objects[t];
    crate::selection::unit(x)
        && sim.objects[u].mana >= MURDER_MANA
        && (x.class == class::MAGE || hero(x) || x.health >= MURDER_HEALTH)
}

/// 0x4de310 when a computer player's fighter comes into a cell, beyond the
/// easy level: the items it takes (`idle::fighter_item`) lying in the 3×3
/// cells round it, row by row, are picked up at once (0x455280, vt+0x1a8).
pub(crate) fn entered(
    sim: &mut Simulation,
    m: &mut MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    i: usize,
) {
    let o = &m.objects[i];
    if m.game_rules.difficulty == 0
        || !military(sim, m, i)
        || !controls(sim, o.owner)
        || o.flags & (FLAG_STORY | FLAG_DYING) != 0
    {
        return;
    }
    let last = sim.gameplay.pathing.cells.side as i32 - 1;
    let (x, y) = (o.x as i32, o.y as i32);
    for cy in (y - 1).max(0)..=(y + 1).min(last) {
        for cx in (x - 1).max(0)..=(x + 1).min(last) {
            let item = (0..m.objects.len()).find(|&j| {
                let t = &m.objects[j];
                idle::fighter_item(t.class)
                    && alive(t)
                    && t.flags & FLAG_INACTIVE == 0
                    && sim.objects[j].holder.is_none()
                    && [t.x as i32, t.y as i32] == [cx, cy]
            });
            let Some(j) = item else {
                continue;
            };
            if accepts(lib, c, &m.objects[i], &m.objects[j])
                && sim.objects[i].contents.len() < holder_capacity(sim, m, lib, i)
            {
                garrison(sim, m, i, j);
                refresh_equipment(sim, m, j);
                scroll_entered(sim, m, lib, j);
            }
        }
    }
}

/// vt+0x2b0 for a unit of a computer player: the peasant captures a
/// building that answers 0 to the query 0x271e (0x4ae580), the ninja
/// murders (0x4ebeb0); otherwise a unit may cast a spell first
/// (0x4dd8c0 → 0x414730), else it attacks (0x42df70).
pub(crate) fn target_program(
    sim: &mut Simulation,
    m: &MissionWorld,
    lib: &ObjectLibrary,
    c: &Catalog,
    u: usize,
    t: usize,
) -> Intent {
    let o = &m.objects[u];
    let x = &m.objects[t];
    match o.class {
        class::PEON if x.building() && !war::uncaptured(x.class) => return Intent::Capture(t),
        class::NINJA if murders(sim, m, u, t) => return Intent::Murder(t),
        _ => {}
    }
    if crate::selection::unit(o)
        && let Some(spell) = spells::before_attack(sim, m, lib, c, u, t)
    {
        return spell;
    }
    Intent::Attack(t)
}

#[cfg(test)]
#[path = "units_tests.rs"]
mod tests;
