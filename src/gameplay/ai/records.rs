//! What each computer player keeps of every place (0x40fc60, 0xa2-byte
//! records in the list IA+0xa0a): the friendly and hostile forces there and
//! on their way, the strength it wants there and how much the place
//! matters, kept in order of that priority.
use super::places::{Forces, Manager, kind, mask};
use super::*;

/// A computer player's view of a place.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Record {
    /// The place's slot, and its serial to tell it from a later one.
    pub place: usize,
    pub serial: u64,
    /// Forces of the player's allies (itself included) and of the others,
    /// present (+0xc, +0x2a) and on their way (+0x48, +0x66).
    pub friendly: Forces,
    pub hostile: Forces,
    pub friendly_incoming: Forces,
    pub hostile_incoming: Forces,
    // The groups on their way there (+0x84) are counted from the units
    // (`war::on_their_way`).
    /// The strength it needs there and the one it wants (+0x8a, +0x8e).
    pub minimum: u32,
    pub wanted: u32,
    /// +0x92: bit 0 towers, bit 1 barracks and outposts, bit 2 defence.
    pub flags: u32,
    /// From the player's home (+0x96).
    pub distance: u32,
    /// +0x9a.
    pub priority: u32,
    /// The place's stamp it was evaluated at (+0x9e).
    pub stamp: u32,
}

/// 0x401ad0: the empty cells between a point and a rect.
pub(crate) fn gap(p: [i32; 2], r: [i32; 4]) -> u32 {
    let dx = if p[0] < r[0] {
        r[0] - p[0] - 1
    } else {
        p[0] - r[2] - 1
    };
    let dy = if p[1] < r[1] {
        r[1] - p[1] - 1
    } else {
        p[1] - r[3] - 1
    };
    dx.max(dy).max(0) as u32
}
fn merge(into: &mut Forces, from: &Forces) {
    for (a, b) in into.0.iter_mut().zip(from.0.iter()) {
        a.count = a.count.wrapping_add(b.count);
        a.strength = a.strength.wrapping_add(b.strength);
    }
}

/// 0x40fdd0: the record evaluated again when its place changed (or while
/// the player's home moves): its distance, priority, wanted strengths and
/// forces. Nothing before the player knows its home.
pub fn evaluate(
    player: &mut Player,
    k: u8,
    at: usize,
    mgr: &Manager,
    m: &MissionWorld,
    side: usize,
) {
    let Some(home) = player.home else {
        return;
    };
    let Some(r) = player.records.get(at).copied() else {
        return;
    };
    let Some(place) = mgr.place(r.place).filter(|p| p.serial == r.serial) else {
        return;
    };
    if !player.homeless && r.stamp == place.stamp {
        return;
    }
    let size = side as u32;
    let distance = gap(home, place.rect);
    let mut flags = r.flags & !7;
    let (mut minimum, mut wanted) = (0u32, 0u32);
    let military = mgr.totals[k as usize].strength(mask::MILITARY);
    let difficulty = m.game_rules.difficulty;
    let priority = match place.kind {
        kind::HOME => {
            if place.param == k as u32 {
                flags |= 7;
                minimum = (military / 10).max(1);
                wanted = (military / 8).max(1);
            } else if relation(m, place.param as u8, k) & 0x40 != 0 {
                flags |= 6;
                minimum = military / 50;
                wanted = military / 10;
            }
            size.wrapping_mul(3).wrapping_sub(distance.wrapping_mul(3)) >> 1
        }
        kind::RESOURCE => {
            if difficulty != 0 && distance >= 16 {
                if distance < 32 {
                    flags |= 1;
                } else {
                    flags |= 7;
                    minimum = 1;
                    wanted = 1;
                }
            }
            // A mine the player works counts nearer.
            let worked = player
                .lists
                .mines
                .iter()
                .find(|e| Some(e.object) == place.anchor)
                .is_some_and(|e| e.count != 0);
            if worked {
                size.wrapping_sub(distance >> 1)
            } else {
                size.wrapping_sub(distance)
            }
        }
        kind::LOSSES => {
            let ours = (1..=12u8)
                .any(|b| relation(m, k, b) & 0x40 != 0 && place.param & (1 << (b - 1)) != 0);
            if ours { size.wrapping_sub(distance) } else { 0 }
        }
        kind::GATEWAY => {
            flags |= 4;
            minimum = 1;
            wanted = 1;
            size.wrapping_sub(distance)
        }
        kind::BASE => {
            if difficulty == 2 && place.param == k as u32 {
                flags |= 1;
            }
            size.wrapping_sub(distance)
        }
        kind::UNITS => size.wrapping_sub(distance).wrapping_mul(2) / 3,
        _ => 0,
    };
    let mut r = Record {
        place: r.place,
        serial: r.serial,
        flags,
        distance,
        priority,
        stamp: place.stamp,
        ..Default::default()
    };
    for b in 1..=8u8 {
        let (present, incoming) = (&place.present[b as usize], &place.incoming[b as usize]);
        if relation(m, k, b) & 0x40 != 0 {
            merge(&mut r.friendly, present);
            merge(&mut r.friendly_incoming, incoming);
        } else {
            merge(&mut r.hostile, present);
            merge(&mut r.hostile_incoming, incoming);
        }
    }
    let threat = r
        .hostile_incoming
        .strength(mask::MILITARY)
        .wrapping_add(r.hostile.strength(mask::MILITARY | mask::TOWERS));
    if threat != 0 {
        let three = threat.wrapping_mul(3);
        r.minimum = (three >> 1).max(minimum);
        r.wanted = three.max(wanted);
    } else {
        let rest = r
            .hostile_incoming
            .strength(mask::TOWERS | mask::BUILDINGS | mask::BARRACKS)
            .wrapping_add(r.hostile.strength(mask::CIVIL | mask::BUILDINGS));
        r.minimum = (rest / 5).max(minimum);
        r.wanted = rest.wrapping_mul(2).max(wanted);
    }
    player.records[at] = r;
}

/// 0x4102e0: the record moves up past the ones of lower priority, then
/// down past the ones of higher priority.
pub fn sort(player: &mut Player, mut at: usize) {
    let records = &mut player.records;
    while at > 0 && records[at - 1].priority < records[at].priority {
        records.swap(at - 1, at);
        at -= 1;
    }
    while at + 1 < records.len() && records[at + 1].priority > records[at].priority {
        records.swap(at, at + 1);
        at += 1;
    }
}

/// 0x40fc60: a new place enters every player's list at its head,
/// evaluated and sorted.
pub(crate) fn register(
    player: &mut Player,
    k: u8,
    slot: usize,
    serial: u64,
    mgr: &Manager,
    m: &MissionWorld,
    side: usize,
) {
    player.records.insert(
        0,
        Record {
            place: slot,
            serial,
            ..Default::default()
        },
    );
    evaluate(player, k, 0, mgr, m, side);
    sort(player, 0);
}
/// 0x40fcc0: a place gone leaves the lists; whether the player had it.
fn unregister(player: &mut Player, slot: usize, serial: u64) -> bool {
    let before = player.records.len();
    player
        .records
        .retain(|r| (r.place, r.serial) != (slot, serial));
    player.records.len() != before
}
/// 0x40fd10: with groups still on their way to the place gone, the units
/// of the kingdom (the latest first) that Approach it and have not begun
/// to wait go back to their default program.
fn stop_groups(sim: &mut Simulation, m: &MissionWorld, k: u8, slot: usize, serial: u64) {
    if super::war::on_their_way(sim, m, k, slot, serial) == 0 {
        return;
    }
    for i in super::think::kingdom_list(sim, m, k, true) {
        if matches!(sim.gameplay.actors[i].intent, Some(Intent::Approach(ap))
            if ap.place == slot && ap.serial == serial && ap.step < 3)
        {
            super::think::restart(sim, i);
        }
    }
}

/// The places made and deleted since the last call, passed to every
/// player in kingdom order, in the order they happened.
pub(crate) fn sync(sim: &mut Simulation, m: &MissionWorld) {
    let Some(mut mgr) = sim.gameplay.ai.manager.take() else {
        return;
    };
    let side = mgr.side;
    let mut gone = Vec::new();
    for (made, slot, serial) in std::mem::take(&mut mgr.events) {
        for (&k, player) in sim.gameplay.ai.players.iter_mut() {
            if made {
                if mgr.place(slot).is_some_and(|p| p.serial == serial) {
                    register(player, k, slot, serial, &mgr, m, side);
                }
            } else if unregister(player, slot, serial) {
                gone.push((k, slot, serial));
            }
        }
    }
    sim.gameplay.ai.manager = Some(mgr);
    for (k, slot, serial) in gone {
        stop_groups(sim, m, k, slot, serial);
    }
}
