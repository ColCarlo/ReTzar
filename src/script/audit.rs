//! A survey of a mission's script against what it names: the actions of
//! its rules, the symbols of its expressions, and the objects,
//! conversations, files and maps it refers to. Nothing here runs the
//! script; the findings point at what the runtime would read as 0 or skip.
use super::{Action, Area, Event, Holder, Script, Selector, any, expr};
use std::collections::BTreeSet;

/// What a mission offers to its script.
pub struct Context<'a> {
    /// Ids of the map's objects, hidden ones included.
    pub ids: &'a BTreeSet<u16>,
    pub conversations: &'a [crate::dialogue::Conversation],
    /// Whether a file named by the script (`USER\…`) exists.
    pub has_file: &'a dyn Fn(&str) -> bool,
    /// Whether `LoadMap` can open the map.
    pub has_map: &'a dyn Fn(&str) -> bool,
    /// The number of object classes.
    pub classes: usize,
}

/// The problems found, one line each.
pub fn audit(script: &Script, cx: &Context) -> Vec<String> {
    let mut out = Vec::new();
    let lower = |s: &str| s.trim().to_ascii_lowercase();
    let actions: BTreeSet<String> = script.actions.iter().map(|a| lower(&a.name)).collect();
    let events: BTreeSet<String> = script.events.iter().map(|e| lower(&e.name)).collect();
    let quests: BTreeSet<String> = script
        .actions
        .iter()
        .filter(|a| matches!(a.value, Action::AddQuest { .. }))
        .map(|a| lower(&a.name))
        .collect();
    let timers: BTreeSet<String> = script
        .actions
        .iter()
        .filter(|a| matches!(a.value, Action::SetTimer { .. }))
        .map(|a| lower(&a.name))
        .collect();
    // Every expression, where it is, and whether it has an object.
    let mut exprs: Vec<(String, String, bool)> = Vec::new();
    let selector = |out: &mut Vec<String>, at: &str, s: &Selector, exprs: &mut Vec<_>| {
        if s.by_class {
            let known = (s.class as usize) < cx.classes
                || matches!(
                    s.class,
                    any::OBJECT | any::UNIT | any::MILITARY | any::BUILDING | any::MAJOR
                );
            if !known {
                out.push(format!("{at}: classe {} sconosciuta", s.class));
            }
            if !s.count.trim().is_empty() {
                exprs.push((at.to_owned(), s.count.clone(), false));
            }
        }
        for id in &s.ids {
            if !cx.ids.contains(id) {
                out.push(format!("{at}: oggetto {id} assente dalla mappa"));
            }
        }
    };
    let area = |out: &mut Vec<String>, at: &str, a: &Area, exprs: &mut Vec<_>| {
        if a.around {
            if !cx.ids.contains(&a.object) {
                out.push(format!(
                    "{at}: area attorno all'oggetto {} assente",
                    a.object
                ));
            }
            if !a.radius.trim().is_empty() {
                exprs.push((at.to_owned(), a.radius.clone(), false));
            }
        } else if a.object != 0 && !cx.ids.contains(&a.object) {
            out.push(format!("{at}: oggetto {} dell'area assente", a.object));
        }
    };
    let holder = |out: &mut Vec<String>, at: &str, h: &Holder| {
        if !h.by_class && !cx.ids.contains(&h.object) {
            out.push(format!("{at}: contenitore {} assente", h.object));
        }
    };
    let file = |out: &mut Vec<String>, at: &str, what: &str, name: &str| {
        if !name.trim().is_empty() && !(cx.has_file)(name.trim()) {
            out.push(format!("{at}: {what} «{name}» mancante"));
        }
    };
    for r in &script.rules {
        let at = format!("regola «{}»", r.name);
        exprs.push((at.clone(), expr::condition(&r.condition), false));
        for a in &r.actions {
            if !actions.contains(&lower(a)) {
                out.push(format!("{at}: azione «{a}» inesistente"));
            }
        }
    }
    for e in &script.events {
        let at = format!("evento «{}»", e.name);
        match &e.value {
            Event::Exists(s) | Event::Destroyed(s) => selector(&mut out, &at, s, &mut exprs),
            Event::Entered(s, h) => {
                selector(&mut out, &at, s, &mut exprs);
                holder(&mut out, &at, h);
            }
            Event::InArea(s, a) => {
                selector(&mut out, &at, s, &mut exprs);
                area(&mut out, &at, a, &mut exprs);
            }
            Event::Expression(s, x) => {
                selector(&mut out, &at, s, &mut exprs);
                exprs.push((at.clone(), expr::condition(x), true));
            }
            Event::Left | Event::Defeated(_) | Event::Won(_) => {}
        }
    }
    for a in &script.actions {
        let at = format!("azione «{}»", a.name);
        match &a.value {
            Action::AddQuest { portrait, .. } => file(&mut out, &at, "ritratto", portrait),
            Action::RemoveQuest(n) => {
                if !quests.contains(&lower(n)) {
                    out.push(format!("{at}: obiettivo «{n}» mai aggiunto"));
                }
            }
            Action::Diplomacy { from, to, .. } => {
                if !(1..=8).contains(from) || !(1..=8).contains(to) {
                    out.push(format!("{at}: regni {from} e {to} fuori da 1–8"));
                }
            }
            Action::Owner(s, _) | Action::Destroy(s) | Action::Appear(s) | Action::Erase(s) => {
                selector(&mut out, &at, s, &mut exprs)
            }
            Action::Lock(s, _) => selector(&mut out, &at, s, &mut exprs),
            Action::SetProperty(s, x) => {
                selector(&mut out, &at, s, &mut exprs);
                exprs.push((at.clone(), expr::statements(x), true));
            }
            Action::Expression(x) => exprs.push((at.clone(), expr::statements(x), false)),
            Action::SetTimer { init, .. } => exprs.push((at.clone(), init.clone(), false)),
            Action::StopTimer(n) => {
                if !timers.contains(&lower(n)) {
                    out.push(format!("{at}: timer «{n}» mai avviato"));
                }
            }
            Action::LoadMap(path) => {
                if !(cx.has_map)(path) {
                    out.push(format!("{at}: mappa successiva «{path}» introvabile"));
                }
            }
            Action::LockView(time, a) => {
                exprs.push((at.clone(), time.clone(), false));
                area(&mut out, &at, a, &mut exprs);
            }
            Action::Story {
                image,
                sound,
                music,
                time,
                ..
            } => {
                file(&mut out, &at, "immagine", image);
                file(&mut out, &at, "voce", sound);
                file(&mut out, &at, "musica", music);
                exprs.push((at.clone(), time.clone(), false));
            }
            Action::Message { image, .. } => file(&mut out, &at, "immagine", image),
            Action::Spawn {
                class,
                count,
                area: a,
                ..
            } => {
                if *class as usize >= cx.classes {
                    out.push(format!("{at}: classe {class} sconosciuta"));
                }
                exprs.push((at.clone(), count.clone(), false));
                area(&mut out, &at, a, &mut exprs);
            }
            Action::Order(s, a, _) => {
                selector(&mut out, &at, s, &mut exprs);
                area(&mut out, &at, a, &mut exprs);
            }
            Action::ScrollView(a, b) => {
                area(&mut out, &at, a, &mut exprs);
                area(&mut out, &at, b, &mut exprs);
            }
            Action::Conversation(n) => {
                if !cx
                    .conversations
                    .iter()
                    .any(|c| c.name.eq_ignore_ascii_case(n.trim()))
                {
                    out.push(format!("{at}: conversazione «{n}» assente"));
                }
            }
            Action::Explore(a, time) => {
                area(&mut out, &at, a, &mut exprs);
                exprs.push((at.clone(), time.clone(), false));
            }
            Action::Delay(x) => exprs.push((at.clone(), x.clone(), false)),
            Action::Status { kingdom, .. } => {
                if !(1..=8).contains(kingdom) {
                    out.push(format!("{at}: regno {kingdom} fuori da 1–8"));
                }
            }
            Action::Lose(..)
            | Action::Win(..)
            | Action::KeepRule
            | Action::Chat(_)
            | Action::UnlockView => {}
        }
    }
    // The lines of the conversations: who speaks, their voice, the lines
    // that follow, their conditions and commands.
    for c in cx.conversations {
        let at = format!("conversazione «{}»", c.name);
        if !c.phrases.iter().any(|p| p.start) {
            out.push(format!("{at}: nessuna frase d'inizio"));
        }
        for p in &c.phrases {
            let at = format!("{at}, frase {}", p.index);
            if p.speaker != 0 && !cx.ids.contains(&p.speaker) {
                out.push(format!("{at}: chi parla ({}) assente", p.speaker));
            }
            file(&mut out, &at, "voce", &p.voice);
            for n in &p.next {
                if !c.phrases.iter().any(|q| q.index == *n) {
                    out.push(format!("{at}: frase seguente {n} assente"));
                }
            }
            if !p.condition.trim().is_empty() {
                exprs.push((at.clone(), expr::condition(&p.condition), false));
            }
            if !p.action.trim().is_empty() {
                exprs.push((at.clone(), expr::statements(&p.action), false));
            }
        }
    }
    // Names written somewhere (`name = …`) are variables; a timer reads as
    // its action's name.
    let assigned: BTreeSet<String> = exprs
        .iter()
        .flat_map(|(_, text, _)| assignments(text))
        .map(|n| n.to_ascii_lowercase())
        .chain(timers.iter().cloned())
        .collect();
    for (at, text, object) in &exprs {
        if text.matches('(').count() != text.matches(')').count() {
            out.push(format!("{at}: parentesi non bilanciate in «{text}»"));
        }
        for token in symbols(text) {
            let low = token.to_ascii_lowercase();
            if token.as_bytes()[0].is_ascii_digit() {
                continue;
            }
            if let Some(name) = low
                .strip_prefix('@')
                .map(|n| n.split('(').next().unwrap_or(n))
            {
                let name = format!("@{name}");
                let known = name == "@playmovie"
                    || expr::FUNCTIONS
                        .iter()
                        .any(|(f, _)| f.len() >= name.len() && f[..name.len()] == name[..]);
                if !known {
                    out.push(format!("{at}: funzione «{token}» sconosciuta"));
                }
                continue;
            }
            let property = expr::PROPERTIES.iter().any(|(p, _)| *p == low);
            if property {
                if !object {
                    out.push(format!("{at}: proprietà «{token}» senza oggetto"));
                }
                continue;
            }
            let global = expr::GLOBALS.iter().any(|(g, _)| *g == low)
                || low == "$candelobjects"
                || low == "$canusediplomacy";
            // `#` names are kept from the map before.
            if global || events.contains(&low) || assigned.contains(&low) || low.starts_with('#') {
                continue;
            }
            out.push(format!(
                "{at}: nome «{token}» né evento né variabile assegnata"
            ));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The operands of an expression, the arguments of `@str` and
/// `@playmovie` (text) left out.
pub(crate) fn symbols(text: &str) -> Vec<String> {
    let s = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < s.len() {
        let c = s[at];
        if !(c.is_ascii_alphanumeric() || matches!(c, b'$' | b'#' | b'_' | b'@')) {
            at += 1;
            continue;
        }
        let start = at;
        while at < s.len()
            && (s[at].is_ascii_alphanumeric() || matches!(s[at], b'$' | b'#' | b'_' | b'@'))
        {
            at += 1;
        }
        let token = &text[start..at];
        out.push(token.to_owned());
        let lower = token.to_ascii_lowercase();
        if (lower.starts_with("@str") || lower.starts_with("@playmovie"))
            && s.get(at) == Some(&b'(')
        {
            let mut depth = 0;
            while at < s.len() {
                match s[at] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            at += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                at += 1;
            }
        }
    }
    out
}

/// The names an expression assigns: an operand followed by a lone `=`.
fn assignments(text: &str) -> Vec<String> {
    let s = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < s.len() {
        if !(s[at].is_ascii_alphanumeric() || matches!(s[at], b'$' | b'#' | b'_')) {
            at += 1;
            continue;
        }
        let start = at;
        while at < s.len() && (s[at].is_ascii_alphanumeric() || matches!(s[at], b'$' | b'#' | b'_'))
        {
            at += 1;
        }
        let mut next = at;
        while s.get(next) == Some(&b' ') {
            next += 1;
        }
        if s.get(next) == Some(&b'=') && s.get(next + 1) != Some(&b'=') {
            let before = text[..start].trim_end().as_bytes().last().copied();
            if !matches!(before, Some(b'<' | b'>' | b'!' | b'=')) {
                out.push(text[start..at].to_owned());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires installed original archives"]
    fn every_mission_script_names_only_what_exists() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let found: Vec<(String, String)> = audit_catalog(root)
            .unwrap()
            .into_iter()
            .flat_map(|r| {
                let id = r.id.clone();
                r.found.into_iter().map(move |f| (id.clone(), f))
            })
            .collect();
        // The original data's own slips, which the original runs the same:
        // `==` where `=` was meant (the variables are 0 anyway), and orders
        // toward an area round object 0, that is cell 0, 0 (0x4bb0c0).
        let arthur = "excalibur e il re artu.wcm::maps/";
        let khan = "impero di gengis khan.wcm::maps/gengis_khan01";
        let mut expected: Vec<(String, String)> = [
            ("1_a merlín y el rey arturo", "cogelaespada"),
            ("1_b merlín y el rey arturo b", "ATAQUELANCEROS"),
            ("1_b merlín y el rey arturo b", "catapultasec"),
            ("3_b el poder de excalibur", "paraaroyns"),
        ]
        .iter()
        .map(|(m, a)| {
            (
                format!("{arthur}{m}"),
                format!("azione «{a}»: area attorno all'oggetto 0 assente"),
            )
        })
        .chain(["enanomineros2", "enanosmineros1"].iter().map(|a| {
            (
                khan.to_owned(),
                format!("azione «{a}»: area attorno all'oggetto 0 assente"),
            )
        }))
        .chain(["$abadia", "$gracias"].iter().map(|v| {
            (
                format!("{arthur}1_a merlín y el rey arturo"),
                format!("azione «todoacero»: nome «{v}» né evento né variabile assegnata"),
            )
        }))
        .collect();
        // The American campaign's, when its CD is there: `@VardalWon` names
        // no function and reads and writes a variable, a StopTimer with no
        // name stops nothing, and `HaveNinjas` lacks the `$` of the variable
        // its conversation sets, so it reads 0.
        if crate::scenario::american_campaign(root).is_some() {
            let vardal = "funzione «@VardalWon» sconosciuta";
            let fought = "conversazione «Sartor and Vardal have fought»";
            expected.extend(
                [
                    ("alex1", format!("azione «Vardal Won»: {vardal}")),
                    ("alex1", format!("{fought}, frase 1: {vardal}")),
                    ("alex1", format!("{fought}, frase 6: {vardal}")),
                    (
                        "benas",
                        "azione «Stop PurpleTimer»: timer «» mai avviato".to_owned(),
                    ),
                    (
                        "v_02",
                        "regola «SpeakWithHighPriest»: nome «HaveNinjas» né evento né \
                         variabile assegnata"
                            .to_owned(),
                    ),
                ]
                .map(|(m, f)| (format!("tzar.wcm::maps/{m}"), f)),
            );
        }
        // The Spanish edition's, when it is there: the same area.
        if !crate::scenario::spanish_campaigns(root).is_empty() {
            expected.push((
                "los dominios de la magia.wcm::maps/acorralada".to_owned(),
                "azione «arreglapuerta»: area attorno all'oggetto 0 assente".to_owned(),
            ));
        }
        let mut found = found;
        found.sort();
        expected.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn symbols_and_assignments_follow_the_native_operands() {
        assert_eq!(
            symbols("@food(1) >= 100 && $x == #kept || Arrived"),
            ["@food", "1", "100", "$x", "#kept", "Arrived"]
        );
        assert_eq!(symbols("@str(Il re è morto) ; n = 2"), ["@str", "n", "2"]);
        assert_eq!(
            assignments("n = 2; $y = n + 1; z == 3; a <= 4"),
            ["n", "$y"]
        );
    }
}

/// One mission's survey.
pub struct Report {
    pub id: String,
    pub title: String,
    /// Rules, actions and events.
    pub counts: [usize; 3],
    pub found: Vec<String>,
}
/// Every scenario of the installation at `source`, its campaign's maps as
/// the ones `LoadMap` may open.
pub fn audit_catalog(source: &std::path::Path) -> crate::Result<Vec<Report>> {
    let catalog = crate::scenario::ScenarioCatalog::load(source)?;
    let mut missions: Vec<(Option<usize>, &crate::scenario::Scenario)> =
        catalog.missions.iter().map(|m| (None, m)).collect();
    for (k, c) in catalog.campaigns.iter().enumerate() {
        missions.extend(c.missions.iter().map(|m| (Some(k), m)));
    }
    let mut out = Vec::new();
    for (campaign, m) in missions {
        let c = m.container()?;
        let grid = crate::map::MapGrid::read(&c, &m.header)?;
        let world = crate::world::MissionWorld::read(&c, &grid)?;
        let ids: BTreeSet<u16> = world.objects.iter().map(|o| o.id).collect();
        let entries: BTreeSet<String> = c
            .entries
            .iter()
            .map(|e| e.name.replace('\\', "/").to_ascii_lowercase())
            .collect();
        let has_file = |name: &str| entries.contains(&name.replace('\\', "/").to_ascii_lowercase());
        let has_map = |path: &str| {
            let wanted = path.replace('\\', "/");
            campaign.is_some_and(|k| {
                catalog.campaigns[k].missions.iter().any(|x| {
                    x.nested
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(&wanted))
                })
            })
        };
        let cx = Context {
            ids: &ids,
            conversations: &world.conversations,
            has_file: &has_file,
            has_map: &has_map,
            classes: crate::world::CLASSES.len(),
        };
        let s = &world.script;
        out.push(Report {
            id: m.id.clone(),
            title: m.title.clone(),
            counts: [s.rules.len(), s.actions.len(), s.events.len()],
            found: audit(s, &cx),
        });
    }
    Ok(out)
}
