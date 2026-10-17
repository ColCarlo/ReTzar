//! What a kingdom can make, as the records of its types say (0x4976a0):
//! which type trains or researches each one (0x405a20) and whether a type
//! is in its tree with what it needs (0x407d50, 0x494c90).
use super::*;

/// The bat, the golem, the carpet, the jinn and the dragon are summoned by
/// the mage (0x405a64).
const SUMMONED: [usize; 5] = [15, 16, 22, 30, 32];

/// A blank object of a class and civilization, to read its record.
pub(crate) fn probe(class: usize, civilization: u8) -> MapObject {
    MapObject {
        id: 0,
        class,
        flags: 0,
        x: 0,
        y: 0,
        owner: 0,
        civilization,
        variant: 0,
        health: 0,
        max_health: 0,
        name: String::new(),
        subtitle: String::new(),
        portrait: String::new(),
        portrait_large: String::new(),
        custom_image: String::new(),
        data: vec![0; 77],
        mole: 0,
        control: [0; 8],
        sight: [0; 8],
    }
}
/// The command menu of a class for a civilization (+0x7c of its record).
fn menu(lib: &ObjectLibrary, civ: u8, class: usize) -> Option<&str> {
    lib.property(&probe(class, civ), "menu")
}

/// 0x405a20: the type whose own command menu trains (`create`, 0x17) or
/// researches (`technology`, 0x30) `kind`, the last in number order; the
/// mage for the summoned creatures. Submenus are not read.
pub fn producer(c: &Catalog, lib: &ObjectLibrary, civ: u8, kind: usize) -> Option<usize> {
    let name = c.ai.types.get(kind)?;
    let mut found = SUMMONED.contains(&kind).then_some(class::MAGE);
    for class in 0..CLASSES.len() {
        let Some(menu) = menu(lib, civ, class) else {
            continue;
        };
        for (key, _) in c.menus.entries(menu) {
            let action = c.action(key);
            if matches!(
                action.split_whitespace().next(),
                Some("create" | "technology")
            ) && c.class_name(key).eq_ignore_ascii_case(name)
            {
                found = Some(class);
            }
        }
    }
    found
}

/// The record's flag +0x107: the type is in the civilization's tree, that
/// is on a menu its objects show, submenus included.
fn present(c: &Catalog, lib: &ObjectLibrary, civ: u8, name: &str) -> bool {
    let mut seen = BTreeSet::new();
    for class in 0..CLASSES.len() {
        let Some(first) = menu(lib, civ, class) else {
            continue;
        };
        let mut pending = vec![first.to_owned()];
        while let Some(menu) = pending.pop() {
            if !seen.insert(menu.to_ascii_lowercase()) {
                continue;
            }
            for (key, _) in c.menus.entries(&menu) {
                if let Some(sub) = c.orders.get(key, "submenu") {
                    pending.push(sub.trim().to_owned());
                    continue;
                }
                let action = c.action(key);
                if matches!(
                    action.split_whitespace().next(),
                    Some("create" | "build" | "technology" | "upgrade")
                ) && c.class_name(key).eq_ignore_ascii_case(name)
                {
                    return true;
                }
            }
        }
    }
    false
}

/// The names [`present`] finds in a civilization's tree, in lower case,
/// read in one walk of its menus.
pub(crate) fn names(c: &Catalog, lib: &ObjectLibrary, civ: u8) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut found = BTreeSet::new();
    for class in 0..CLASSES.len() {
        let Some(first) = menu(lib, civ, class) else {
            continue;
        };
        let mut pending = vec![first.to_owned()];
        while let Some(menu) = pending.pop() {
            if !seen.insert(menu.to_ascii_lowercase()) {
                continue;
            }
            for (key, _) in c.menus.entries(&menu) {
                if let Some(sub) = c.orders.get(key, "submenu") {
                    pending.push(sub.trim().to_owned());
                    continue;
                }
                if matches!(
                    c.action(key).split_whitespace().next(),
                    Some("create" | "build" | "technology" | "upgrade")
                ) {
                    found.insert(c.class_name(key).to_ascii_lowercase());
                }
            }
        }
    }
    found
}

/// 0x407d50 with 0x494ba0 and 0x494c90: the type is not forbidden, is in
/// the tree (a research not done nor under way), its research is done and
/// its required building finished.
pub fn available(
    sim: &Simulation,
    m: &MissionWorld,
    c: &Catalog,
    lib: &ObjectLibrary,
    k: u8,
    civ: u8,
    name: &str,
) -> bool {
    // A kingdom without lists of its own forbids nothing and knows nothing.
    let none = world::KingdomRules::default();
    let r = m.rules.get((k as usize).wrapping_sub(1)).unwrap_or(&none);
    // «notech» (0x494bbf, 0x494c71, 0x494c90): neither the forbidden lists
    // nor what the type depends on are asked.
    let free = m.cheats.no_tech;
    if !free && forbidden(r, name) {
        return false;
    }
    let known = |dep: &str| {
        r.initial_researches
            .iter()
            .any(|s| s.eq_ignore_ascii_case(dep.trim()))
    };
    let research = c
        .technologies
        .section_names()
        .any(|s| s.eq_ignore_ascii_case(name));
    if research && (known(name) || researching(sim, m, k, name)) {
        return false;
    }
    if !present(c, lib, civ, name) {
        return false;
    }
    if free {
        return true;
    }
    let Some(tree) = c.trees.get(civ_section(civ)) else {
        return true;
    };
    if let Some(dep) = tree.get("depend", name)
        && !known(dep)
    {
        return false;
    }
    if let Some(dep) = tree.get("required", name)
        && !m.objects.iter().any(|x| {
            x.owner == k
                && rules::counted(x)
                && x.flags & FLAG_CONSTRUCTION == 0
                && x.class_name().eq_ignore_ascii_case(dep.trim())
        })
    {
        return false;
    }
    true
}
