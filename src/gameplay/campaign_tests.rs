//! The campaigns played to their end by a player who follows each mission's
//! way to victory with the orders a player gives (move, pick up, attack):
//! the conversations answered with their first line, the windows closed.
//! Each test checks the script's steps on the way and that the mission
//! wins and opens the next map.

use super::tests::request;
use super::*;

/// A campaign mission being played; a copy shares the catalogue and the
/// objects' library.
#[derive(Clone)]
struct Play {
    c: std::rc::Rc<Catalog>,
    lib: std::rc::Rc<ObjectLibrary>,
    m: MissionWorld,
    g: MapGrid,
    sim: Simulation,
    /// The line to pick in a conversation, by its index, when offered;
    /// otherwise the first one.
    answers: Vec<(&'static str, u16)>,
}
fn root() -> &'static std::path::Path {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
}
impl Play {
    fn open(map: &str) -> Play {
        let catalog = crate::scenario::ScenarioCatalog::load(root()).unwrap();
        let id = format!("excalibur e il re artu.wcm::maps/{map}");
        let (campaign, mission) = catalog
            .locate(&id)
            .or_else(|| catalog.locate(&format!("impero di gengis khan.wcm::maps/{map}")))
            .unwrap();
        Play::open_at(&catalog, campaign.unwrap(), mission)
    }
    /// The map `LoadMap` chose, with the heroes and the `#` variables the
    /// campaign carries (as `menu_ui::next_map` and the loader do).
    fn next(&self) -> Play {
        let path = self
            .sim
            .gameplay
            .script
            .next_map
            .clone()
            .expect("nessuna mappa successiva");
        let wanted = path.replace('\\', "/");
        let catalog = crate::scenario::ScenarioCatalog::load(root()).unwrap();
        let (campaign, mission) = catalog
            .campaigns
            .iter()
            .enumerate()
            .find_map(|(k, c)| {
                let i = c.missions.iter().position(|m| {
                    m.nested
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(&wanted))
                })?;
                Some((k, i))
            })
            .unwrap_or_else(|| panic!("mappa {path} assente"));
        let heroes = heroes(&self.sim, &self.m);
        let mut next = Play::open_at(&catalog, campaign, mission);
        restore_heroes(&mut next.sim, &mut next.m, &next.lib, &heroes);
        next.sim.gameplay.script.variables.kept = self.sim.gameplay.script.variables.kept.clone();
        next
    }
    fn open_at(
        catalog: &crate::scenario::ScenarioCatalog,
        campaign: usize,
        mission: usize,
    ) -> Play {
        let assets = GameAssets::open(root()).unwrap();
        let campaign = Some(campaign);
        let scenario = catalog.campaigns[campaign.unwrap()].missions[mission].clone();
        let selection = crate::menu::Selection::Scenario { campaign, mission };
        let opened = crate::game::open(&assets, &selection, Some(scenario), None).unwrap();
        let lib = ObjectLibrary::load(&assets, &opened.biome).unwrap();
        let mut m = opened.mission;
        crate::game::start(
            &mut m,
            &opened.grid,
            &lib,
            &opened.biome,
            &selection,
            None,
            crate::terrain::Climate::default(),
        )
        .unwrap();
        let mut sim = m.simulation.take().unwrap();
        sim.gameplay.script.trace.0 = Some(Vec::new());
        Play {
            c: std::rc::Rc::new(Catalog::load(&assets).unwrap()),
            lib: std::rc::Rc::new(lib),
            m,
            g: opened.grid,
            sim,
            answers: Vec::new(),
        }
    }
    /// The index of the map object `id`.
    fn obj(&self, id: u16) -> usize {
        self.m
            .objects
            .iter()
            .position(|o| o.id == id)
            .unwrap_or_else(|| panic!("oggetto {id} assente"))
    }
    /// One second of play: an open conversation gets its first answer, a
    /// story or message is closed.
    fn second(&mut self) {
        // The game stands still while a story, a message or a conversation
        // is open (0x43ed40 from 0x47d9b5, 0x4c3538, 0x4be68c), as the
        // interface does with their panels: 50 ms at a time, each window
        // dealt with before the clock moves on.
        for _ in 0..20 {
            while self.sim.gameplay.script.modal {
                self.reply();
            }
            self.sim.gameplay.script.outbox.0.clear();
            advance(
                &mut self.sim,
                &mut self.m,
                &mut self.g,
                &self.lib,
                &self.c,
                Duration::from_millis(TICK_MS),
                10,
                false,
            );
        }
    }
    /// The answer to the open window: the chosen line of a conversation,
    /// else its first; a story or a message is closed.
    fn reply(&mut self) {
        {
            if let Some(talk) = self.sim.gameplay.script.conversation.clone() {
                let c = &self.m.conversations[talk.conversation];
                let pick = talk
                    .choices
                    .iter()
                    .position(|&k| {
                        self.answers.iter().any(|&(name, index)| {
                            c.name.eq_ignore_ascii_case(name) && c.phrases[k].index == index
                        })
                    })
                    .unwrap_or(0);
                answer(
                    &mut self.sim,
                    &mut self.m,
                    &self.g,
                    &self.lib,
                    &self.c,
                    pick,
                );
            } else {
                close_window(&mut self.sim, &mut self.m);
            }
        }
    }
    /// Plays until `done`, at most `seconds`; whether it came.
    fn until(&mut self, seconds: u64, mut done: impl FnMut(&mut Play) -> bool) -> bool {
        for _ in 0..seconds {
            if done(self) {
                return true;
            }
            if self.sim.gameplay.script.over.is_some() {
                return false;
            }
            self.second();
        }
        done(self)
    }
    fn fired(&self, rule: &str) -> bool {
        let wanted = format!("regola «{rule}»");
        self.sim
            .gameplay
            .script
            .trace
            .0
            .as_ref()
            .is_some_and(|t| t.iter().any(|(_, l)| *l == wanted))
    }
    fn var(&mut self, name: &str) -> i32 {
        self.sim.gameplay.script.variables.get(name)
    }
    fn order(&mut self, order: &str, ids: &[u16], cell: [u16; 2], target: Option<u16>) {
        let selected = ids.iter().map(|&id| self.obj(id)).collect();
        let target = target.map(|id| self.obj(id));
        let req = request(order, selected, cell, target);
        issue(
            &mut self.sim,
            &mut self.m,
            &self.g,
            &self.lib,
            &self.c,
            &req,
        )
        .unwrap_or_else(|e| {
            let who: Vec<String> = ids
                .iter()
                .map(|&id| {
                    let o = &self.m.objects[self.obj(id)];
                    format!("#{id} regno {} salute {}", o.owner, o.health)
                })
                .collect();
            panic!(
                "ordine {order} a {} rifiutato: {e}{}",
                who.join(", "),
                self.trace_tail()
            )
        });
    }
    /// An order the game may refuse (a target in the fog, a unit fallen):
    /// whether it was taken.
    fn try_order(&mut self, order: &str, ids: &[u16], cell: [u16; 2], target: Option<u16>) -> bool {
        let selected = ids.iter().map(|&id| self.obj(id)).collect();
        let target = target.map(|id| self.obj(id));
        let req = request(order, selected, cell, target);
        issue(
            &mut self.sim,
            &mut self.m,
            &self.g,
            &self.lib,
            &self.c,
            &req,
        )
        .is_ok()
    }
    /// A player's attack under the fog: the attack, or, on a target the
    /// game refuses because the fog hides it, a walk to where the mission
    /// says it is; whether an order was taken.
    fn close_in(&mut self, ids: &[u16], target: u16) -> bool {
        let at = self.at(target);
        self.try_order("attack", ids, at, Some(target)) || self.try_order("move", ids, at, None)
    }
    /// Walks `id` into the area `r` and plays until `done`.
    fn go(&mut self, id: u16, r: [u16; 4], seconds: u64, done: impl Fn(&mut Play) -> bool) {
        self.go_to(id, [(r[0] + r[2]) / 2, (r[1] + r[3]) / 2], seconds, done);
    }
    /// Walks `id` to `cell` and plays until `done`.
    fn go_to(&mut self, id: u16, cell: [u16; 2], seconds: u64, done: impl Fn(&mut Play) -> bool) {
        let r = [cell[0], cell[1], cell[0], cell[1]];
        self.order("move", &[id], cell, None);
        let arrived = self.until(seconds, done);
        assert!(
            arrived,
            "{id} non arriva in {r:?} (è a {:?}); regole: {}",
            {
                let o = &self.m.objects[self.obj(id)];
                [o.x, o.y]
            },
            self.trace_tail()
        );
    }
    fn trace_tail(&self) -> String {
        self.sim
            .gameplay
            .script
            .trace
            .0
            .as_ref()
            .map(|t| {
                t.iter()
                    .rev()
                    .take(12)
                    .rev()
                    .map(|(ms, l)| format!("\n  {}:{:02} {l}", ms / 60_000, ms / 1000 % 60))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// Where the object `id` stands.
    fn at(&self, id: u16) -> [u16; 2] {
        let o = &self.m.objects[self.obj(id)];
        [o.x, o.y]
    }
    /// The living units of the kingdoms `foes` within `radius` of `at`,
    /// nearest first, with their class.
    fn foes_near(
        &self,
        at: [u16; 2],
        radius: i32,
        foes: &[u8],
    ) -> Vec<(i32, usize, [u16; 2], usize)> {
        let mut v: Vec<_> = self
            .m
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                foes.contains(&o.owner)
                    && alive(o)
                    && o.flags & FLAG_INACTIVE == 0
                    && crate::selection::unit(o)
            })
            .map(|(j, o)| (gap([o.x, o.y], at), j, [o.x, o.y], o.class))
            .filter(|t| t.0 <= radius)
            .collect();
        v.sort();
        v
    }
    /// The fighters (classes 3–38) of kingdom `k` on the map, but `keep`.
    fn army(&self, k: u8, keep: &[u16]) -> Vec<u16> {
        self.m
            .objects
            .iter()
            .filter(|o| {
                o.owner == k
                    && alive(o)
                    && o.flags & FLAG_INACTIVE == 0
                    && (3..=38).contains(&o.class)
                    && !keep.contains(&o.id)
            })
            .map(|o| o.id)
            .collect()
    }
    /// `id` fighting a living target.
    fn fighting(&self, id: u16) -> bool {
        let i = self.obj(id);
        matches!(self.sim.gameplay.actors[i].intent, Some(Intent::Attack(t)) if alive(&self.m.objects[t]))
    }
    /// A fighter not already fighting takes the nearest foe within `radius`
    /// whose class `pick` accepts; true when one was there.
    fn engage(&mut self, id: u16, radius: i32, foes: &[u8], pick: impl Fn(usize) -> bool) -> bool {
        if !alive(&self.m.objects[self.obj(id)]) {
            return false;
        }
        let near: Vec<_> = self
            .foes_near(self.at(id), radius, foes)
            .into_iter()
            .filter(|t| pick(t.3))
            .collect();
        let Some(&(_, j, at, _)) = near.first() else {
            return false;
        };
        let busy = {
            let i = self.obj(id);
            matches!(self.sim.gameplay.actors[i].intent, Some(Intent::Attack(t))
                if alive(&self.m.objects[t]) && pick(self.m.objects[t].class))
        };
        if !busy {
            let target = self.m.objects[j].id;
            self.try_order("attack", &[id], at, Some(target));
        }
        true
    }
    /// A mage's lightning (150 of damage, 50 of mana) on the nearest foe
    /// within 12 whose class `pick` accepts; true while casting.
    fn bolt(&mut self, id: u16, foes: &[u8], pick: impl Fn(usize) -> bool) -> bool {
        let i = self.obj(id);
        if matches!(
            self.sim.gameplay.actors[i].intent,
            Some(Intent::Cast { .. })
        ) {
            return true;
        }
        if !alive(&self.m.objects[i]) || self.sim.objects[i].mana < 50 {
            return false;
        }
        let near: Vec<_> = self
            .foes_near(self.at(id), 12, foes)
            .into_iter()
            .filter(|t| pick(t.3))
            .collect();
        let Some(&(_, j, at, _)) = near.first() else {
            return false;
        };
        let req = request("spell_lightning", vec![i], at, Some(j));
        issue(
            &mut self.sim,
            &mut self.m,
            &self.g,
            &self.lib,
            &self.c,
            &req,
        )
        .is_ok()
    }
    /// Each of `ours` not fighting a unit takes the foe unit nearest to it
    /// within `radius`, else a tower of theirs within `radius`, else the
    /// object `then`.
    fn assault(&mut self, ours: &[u16], foes: &[u8], radius: i32, then: Option<u16>) {
        let towers: Vec<(u16, [u16; 2])> = self
            .m
            .objects
            .iter()
            .filter(|o| {
                foes.contains(&o.owner)
                    && alive(o)
                    && o.flags & FLAG_INACTIVE == 0
                    && o.class_name().starts_with("tower")
            })
            .map(|o| (o.id, [o.x, o.y]))
            .collect();
        for &id in ours {
            let i = self.obj(id);
            if !alive(&self.m.objects[i]) {
                continue;
            }
            let aim = match self.sim.gameplay.actors[i].intent {
                Some(Intent::Attack(t)) if alive(&self.m.objects[t]) => Some(t),
                _ => None,
            };
            if aim.is_some_and(|t| crate::selection::unit(&self.m.objects[t])) {
                continue;
            }
            let here = self.at(id);
            let target = self
                .foes_near(here, radius, foes)
                .first()
                .map(|t| self.m.objects[t.1].id)
                .or_else(|| {
                    towers
                        .iter()
                        .filter(|t| gap(t.1, here) <= radius)
                        .min_by_key(|t| gap(t.1, here))
                        .map(|t| t.0)
                })
                .or(then.filter(|&t| alive(&self.m.objects[self.obj(t)])));
            if let Some(t) = target
                && aim != Some(self.obj(t))
            {
                let at = self.at(t);
                self.try_order("attack", &[id], at, Some(t));
            }
        }
    }
    /// One step of a march: `ids` fight the foes that come within `radius`
    /// and are sent on to `to`; true once most of those standing are there.
    fn march(&mut self, ids: &[u16], to: [u16; 2], foes: &[u8], radius: i32) -> bool {
        let standing: Vec<u16> = ids
            .iter()
            .copied()
            .filter(|&id| alive(&self.m.objects[self.obj(id)]))
            .collect();
        let there = standing
            .iter()
            .filter(|&&id| gap(self.at(id), to) <= 8)
            .count();
        if there * 10 >= standing.len() * 8 {
            return true;
        }
        for &id in &standing {
            if self.fighting(id) || self.engage(id, radius, foes, |_| true) {
                continue;
            }
            let i = self.obj(id);
            if self.sim.gameplay.actors[i].intent.is_none() && gap(self.at(id), to) > 8 {
                self.try_order("move", &[id], to, None);
            }
        }
        false
    }
    /// `guards` keep by `ward` and take the foes that come within `radius`
    /// of it; a mage ward casts at them.
    fn guard(&mut self, guards: &[u16], ward: u16, radius: i32, foes: &[u8]) {
        let w = self.at(ward);
        self.bolt(ward, foes, |_| true);
        let near = self.foes_near(w, radius, foes);
        for &id in guards {
            let i = self.obj(id);
            if !alive(&self.m.objects[i]) || self.fighting(id) {
                continue;
            }
            if let Some(&(_, j, at, _)) = near.first() {
                let target = self.m.objects[j].id;
                self.try_order("attack", &[id], at, Some(target));
            } else if gap(self.at(id), w) > 4 && self.sim.gameplay.actors[i].intent.is_none() {
                self.try_order("move", &[id], w, None);
            }
        }
    }
    /// A journey the test does not play: `ids` stop and are set down on the
    /// free land cells nearest to `to`, as the player would bring them.
    fn carry(&mut self, ids: &[u16], to: [u16; 2]) {
        let mut taken: Vec<[u16; 2]> = Vec::new();
        for &id in ids {
            let i = self.obj(id);
            if !alive(&self.m.objects[i]) {
                continue;
            }
            issue(
                &mut self.sim,
                &mut self.m,
                &self.g,
                &self.lib,
                &self.c,
                &Request::command("stop", vec![i]),
            )
            .ok();
            let me = self.m.objects[i].clone();
            let free = |p: &Play, c: [i32; 2]| {
                let cell = [c[0] as u16, c[1] as u16];
                terrain_free(&p.g, &p.lib, &me, cell, false)
                    && (naval(&me) || p.sim.gameplay.pathing.cells.passable(c))
                    && water_cell(&p.g, cell) == naval(&me)
                    && !taken.contains(&cell)
                    && !p
                        .m
                        .objects
                        .iter()
                        .any(|o| alive(o) && crate::selection::unit(o) && [o.x, o.y] == cell)
            };
            let side = self.g.side as i32;
            let spot = (0..24i32)
                .flat_map(|r| (-r..=r).flat_map(move |dy| (-r..=r).map(move |dx| [dx, dy])))
                .map(|[dx, dy]| [to[0] as i32 + dx, to[1] as i32 + dy])
                .find(|c| c[0] >= 0 && c[1] >= 0 && c[0] < side && c[1] < side && free(self, *c))
                .expect("nessuna cella libera");
            let cell = [spot[0] as u16, spot[1] as u16];
            position(&mut self.m.objects[i], cell);
            movement::carry_contents(&mut self.sim, &mut self.m, i);
            taken.push(cell);
        }
        self.second();
    }
    /// `ids` hold their ground (Stop with the stance of 0x503890).
    fn stand(&mut self, ids: &[u16]) {
        for &id in ids {
            let i = self.obj(id);
            issue(
                &mut self.sim,
                &mut self.m,
                &self.g,
                &self.lib,
                &self.c,
                &Request::command("stand_ground", vec![i]),
            )
            .unwrap();
        }
    }
}
/// Chebyshev distance of two cells.
fn gap(a: [u16; 2], b: [u16; 2]) -> i32 {
    (a[0] as i32 - b[0] as i32)
        .abs()
        .max((a[1] as i32 - b[1] as i32).abs())
}

#[test]
#[ignore = "Requires installed original archives"]
fn merlin_and_king_arthur_is_won_by_bringing_kay_the_sword() {
    let mut p = Play::open("1_a merlín y el rey arturo");
    // «Vengo a pregare»: the monk lets a praying boy in.
    p.answers.push(("velaespada", 7));
    // The tournament: Hector sends Arthur home for a sword (`$justa`), and
    // a countdown of 150 s starts.
    assert!(
        p.until(60, |p| p.fired("arturocorreacasa")),
        "{}",
        p.trace_tail()
    );
    let arthur = 69;
    p.go(arthur, [26, 43, 36, 50], 120, |p| p.var("$casa") == 2);
    // The woodcutters tell him of the sword in the stone.
    p.go(arthur, [4, 46, 15, 55], 120, |p| p.var("$lena") == 2);
    // At the abbey the gates open; he draws the sword (object 112).
    // Before the gate, not in its passage (50–52, 42–44), where a friend
    // keeps the drawbridge up (0x4571a0).
    p.go_to(arthur, [51, 46], 120, |p| p.fired("puertaabre"));
    // The drawbridge comes down (byte 57 at 0) for the friend at the gate.
    let gate = p.obj(234);
    assert!(
        p.until(30, |p| p.m.objects[gate].byte(57) == 0),
        "la porta non si abbassa{}",
        p.trace_tail()
    );
    let sword = p.obj(112);
    let at = [p.m.objects[sword].x, p.m.objects[sword].y];
    p.order("pick_item", &[arthur], at, Some(112));
    assert!(
        p.until(60, |p| p.fired("arturocorreapadre")),
        "{}",
        p.trace_tail()
    );
    assert!(p.until(60, |p| p.var("$fin") == 1), "{}", p.trace_tail());
    // Back at the bridge, the story ends and the next map is chosen.
    p.go(arthur, [29, 18, 37, 22], 120, |p| {
        p.sim.gameplay.script.next_map.is_some()
    });
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\1_b Merlín y el Rey Arturo B.wmp")
    );
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    // The next map opens with what the campaign carries.
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn merlin_and_king_arthur_2_is_won_by_holding_the_tower_and_routing_the_nobles() {
    let mut p = Play::open("1_b merlín y el rey arturo b");
    // «Riparate la porta a sud»: the peasants mend the south gate (285);
    // the generals and the archbishop shelter in the Tower (105).
    let gate = p.obj(285);
    let at = [p.m.objects[gate].x, p.m.objects[gate].y];
    p.order(
        "repair",
        &[330, 331, 332, 333, 334, 335, 336],
        at,
        Some(285),
    );
    let tower = p.obj(105);
    let at = [p.m.objects[tower].x, p.m.objects[tower].y];
    p.order("context", &[389, 390, 391, 392, 393], at, Some(105));
    // King Arthur (kingdom 6, which shares its units) holds his ground by
    // the castle instead of riding out after the rebels.
    p.stand(&[388]);
    assert!(
        p.until(30, |p| [389, 390, 391, 392, 393].iter().all(|&id| p
            .sim
            .objects[p.obj(id)]
        .holder
        .is_some())),
        "nella torre: {:?}",
        [389, 390, 391, 392, 393].map(|id| p.sim.objects[p.obj(id)].holder)
    );
    // The siege from 60 s; at 160 s the rebels withdraw and Merlin comes.
    assert!(
        p.until(220, |p| p.fired("tiemporetirada")),
        "{}",
        p.trace_tail()
    );
    assert!(
        p.until(180, |p| p.fired("ataquefugazmerlin")),
        "{}",
        p.trace_tail()
    );
    // Merlin's reinforcements, knights outside the south gate, ride to
    // Caerleon (58–98, 81–104): three units there raise the town. The noble
    // knights (380–385) wait there, units of the story that their computer
    // does not send (0x410cd0); struck down to 80 they flee south and
    // leave the map, and with them kingdom 7 is defeated. A knight whose
    // way is shut by others standing is sent again.
    let knights = [
        148u16, 1083, 1087, 1091, 1090, 1086, 416, 159, 933, 1093, 1095, 1097, 1096, 1094, 932,
        288, 417, 1085, 1088, 1092, 1089, 1084, 931, 289, 287,
    ];
    let town = [58u16, 81, 98, 104];
    let nobles = [380u16, 381, 382, 383, 384, 385];
    let won = p.until(1140, |p| {
        if p.sim.ticks % 100 != 0 {
        } else if !p.fired("ataquefugazcal") {
            let idle: Vec<u16> = knights
                .iter()
                .copied()
                .filter(|&k| {
                    let i = p.obj(k);
                    let o = &p.m.objects[i];
                    alive(o)
                        && p.sim.gameplay.actors[i].intent.is_none()
                        && !((town[0]..=town[2]).contains(&o.x)
                            && (town[1]..=town[3]).contains(&o.y))
                })
                .collect();
            if !idle.is_empty() {
                p.order("move", &idle, [80, 85], None);
            }
        } else {
            let target = nobles.iter().copied().find(|&id| {
                let o = &p.m.objects[p.obj(id)];
                alive(o) && o.flags & FLAG_INACTIVE == 0
            });
            if let Some(t) = target {
                let o = &p.m.objects[p.obj(t)];
                let at = [o.x, o.y];
                let free: Vec<u16> = knights
                    .iter()
                    .copied()
                    .filter(|&k| alive(&p.m.objects[p.obj(k)]))
                    .collect();
                if !free.is_empty() {
                    p.order("attack", &free, at, Some(t));
                }
            }
        }
        p.sim.gameplay.script.next_map.is_some()
    });
    assert!(won, "{}", p.trace_tail());
    assert!(p.fired("caballeroderribadomalo"), "{}", p.trace_tail());
    assert!(p.fired("caballeromaloderrotado"), "{}", p.trace_tail());
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\2 Los Nobles Rebeldes.wmp")
    );
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_rebel_nobles_is_won_by_hunting_the_scouts_and_crushing_the_rebels() {
    let mut p = Play::open("2 los nobles rebeldes");
    // The briefing runs as a chain of conversations ($uno … $cuatro).
    assert!(p.until(120, |p| p.fired("comie5")), "{}", p.trace_tail());
    // The rebels' scouts (kingdom 8, five horse archers) hunted down by
    // Arthur's knights.
    let knights: Vec<u16> =
        p.m.objects
            .iter()
            .filter(|o| o.owner == 1 && o.class == 8)
            .map(|o| o.id)
            .collect();
    let hunted = p.until(400, |p| {
        if p.sim.ticks % 100 == 0 {
            let scout =
                p.m.objects
                    .iter()
                    .find(|o| o.owner == 8 && alive(o) && crate::selection::unit(o))
                    .map(|o| (o.id, [o.x, o.y]));
            if let Some((id, _)) = scout {
                let free: Vec<u16> = knights
                    .iter()
                    .copied()
                    .filter(|&k| alive(&p.m.objects[p.obj(k)]))
                    .collect();
                p.close_in(&free, id);
            }
        }
        p.fired("capturaexplorer")
    });
    assert!(hunted, "{}", p.trace_tail());
    // Two minutes on, the French kings share their armies.
    assert!(
        p.until(130, |p| p.fired("tiempofranc")),
        "{}",
        p.trace_tail()
    );
    for k in [5, 6] {
        assert!(relation(&p.m, 1, k) & 4 != 0 && relation(&p.m, k, 1) & 4 != 0);
    }
    // The heroes who must survive hold their ground, out of the battle.
    let heroes = [24u16, 26, 25, 27, 162, 554];
    for &id in &heroes {
        let i = p.obj(id);
        issue(
            &mut p.sim,
            &mut p.m,
            &p.g,
            &p.lib,
            &p.c,
            &Request::command("stand_ground", vec![i]),
        )
        .unwrap();
    }
    // At 10 min the rebels march on the camp; after their wave, every army
    // strikes at the rebel nearest to it. With 50 rebel soldiers or fewer
    // the last conversation ends the mission.
    let army = |p: &Play, k: u8| -> Vec<u16> {
        p.m.objects
            .iter()
            .filter(|o| {
                o.owner == k
                    && alive(o)
                    && o.flags & FLAG_INACTIVE == 0
                    && (3..=38).contains(&o.class)
                    && !heroes.contains(&o.id)
            })
            .map(|o| o.id)
            .collect()
    };
    let won = p.until(1500, |p| {
        if p.fired("ataquefrontal") && p.sim.ticks * TICK_MS > 720_000 && p.sim.ticks % 300 == 0 {
            let rebels: Vec<(u16, [u16; 2])> = army(p, 2)
                .into_iter()
                .map(|id| {
                    let o = &p.m.objects[p.obj(id)];
                    (id, [o.x, o.y])
                })
                .collect();
            for k in [1u8, 5, 6] {
                let ours = army(p, k);
                let (Some(&first), false) = (ours.first(), rebels.is_empty()) else {
                    continue;
                };
                let o = &p.m.objects[p.obj(first)];
                let from = [o.x as i32, o.y as i32];
                let (id, _) = *rebels
                    .iter()
                    .min_by_key(|(_, a)| {
                        (a[0] as i32 - from[0]).abs() + (a[1] as i32 - from[1]).abs()
                    })
                    .unwrap();
                p.close_in(&ours, id);
            }
        }
        p.sim.gameplay.script.next_map.is_some()
    });
    assert!(won, "{}", p.trace_tail());
    for rule in ["ataquefrontal", "tiempofin", "FIN"] {
        assert!(p.fired(rule), "{rule}: {}", p.trace_tail());
    }
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\3_a El poder de Excalibur.wmp")
    );
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_power_of_excalibur_is_won_by_freeing_the_ship_from_the_monsters() {
    let mut p = Play::open("3_a el poder de excalibur");
    // Arthur saves Merlin from the monks, loses to Pellinore, whom Merlin
    // charms; at the lake the Lady comes out of the water (owner 9, who
    // waits to be freed and fights nobody) and the mission begins.
    assert!(
        p.until(300, |p| p.fired("descongelayactiva")),
        "{}",
        p.trace_tail()
    );
    let (arthur, merlin) = (55, 49);
    let lady = p.obj(69);
    assert_eq!(p.m.objects[lady].owner, 9);
    assert!(matches!(
        p.sim.gameplay.actors[lady].intent,
        Some(Intent::Rescue(_))
    ));
    // The monsters (kingdom 8): Merlin's lightning (150 of damage, 50 of
    // mana) takes the orcs, who strike hard; Arthur, a king of level 12 on
    // foot, beats the dwarves, whose blows hardly pass his armour. Merlin
    // steps back to the lake when his mana is spent.
    let lake = [37, 6];
    let (m, a) = (p.obj(merlin), p.obj(arthur));
    let freed = p.until(900, |p| {
        if p.sim.gameplay.script.kingdoms[8].defeated {
            return true;
        }
        let monsters: Vec<(usize, [u16; 2], bool)> =
            p.m.objects
                .iter()
                .enumerate()
                .filter(|(_, o)| o.owner == 8 && alive(o) && crate::selection::unit(o))
                .map(|(j, o)| (j, [o.x, o.y], o.class_name() == "ork"))
                .collect();
        let cell = |i: usize| [p.m.objects[i].x as i32, p.m.objects[i].y as i32];
        let (mc, ac) = (cell(m), cell(a));
        let near = |o: [i32; 2], orcs: bool| {
            monsters
                .iter()
                .filter(|t| t.2 == orcs)
                .map(|&(j, at, _)| {
                    let d = (at[0] as i32 - o[0]).abs().max((at[1] as i32 - o[1]).abs());
                    (d, j, at)
                })
                .min()
        };
        let mana = p.sim.objects[m].mana;
        let casting = matches!(p.sim.gameplay.actors[m].intent, Some(Intent::Cast { .. }));
        let orc = near(mc, true);
        let dwarf = near(mc, false).min(near(ac, false));
        if !casting {
            match orc.or(near(mc, false)) {
                Some((d, j, at)) if mana >= 50 && d <= 12 => {
                    let req = request("spell_lightning", vec![m], at, Some(j));
                    issue(&mut p.sim, &mut p.m, &p.g, &p.lib, &p.c, &req).unwrap();
                }
                Some((_, _, at)) if mana >= 50 && orc.is_some() => {
                    if p.sim.gameplay.actors[m].intent.is_none() {
                        p.order("move", &[merlin], at, None);
                    }
                }
                Some((d, _, _)) if d <= 6 => {
                    p.order("move", &[merlin], lake, None);
                }
                _ => {}
            }
        }
        // Arthur keeps by Merlin and takes the dwarves who come near; with
        // the orcs gone he seeks them out.
        let attacking = matches!(p.sim.gameplay.actors[a].intent, Some(Intent::Attack(_)));
        match dwarf {
            Some((d, j, at)) if (d <= 8 || orc.is_none()) && !attacking => {
                let id = p.m.objects[j].id;
                p.order("attack", &[arthur], at, Some(id));
            }
            _ if !attacking
                && (ac[0] - mc[0]).abs().max((ac[1] - mc[1]).abs()) > 3
                && p.sim.ticks % 60 == 0 =>
            {
                p.order("move", &[arthur], [mc[0] as u16, mc[1] as u16], None);
            }
            _ => {}
        }
        false
    });
    assert!(freed, "{}", p.trace_tail());
    // Arthur reaches the ship (55–59, 14–16), goes aboard, and the ship
    // becomes his; it sails to the Lady (45–50, 0–5) for the sword.
    p.go(arthur, [55, 14, 59, 16], 120, |p| p.fired("arturocogebar"));
    let ship = p.obj(75);
    assert_eq!(p.m.objects[ship].owner, 1);
    assert_eq!(p.var("$siete"), 2);
    p.go(75, [45, 0, 50, 5], 120, |p| {
        p.sim.gameplay.script.next_map.is_some()
    });
    assert!(p.fired("llegabajel"), "{}", p.trace_tail());
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\3_b El poder de Excalibur.wmp")
    );
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_power_of_excalibur_2_is_won_by_taking_royns_and_breaking_the_rebels() {
    let mut p = Play::open("3_b el poder de excalibur");
    // Merlin enchants the weapons (the items appear by the ruins).
    p.answers.push(("Conversacion0001", 6));
    assert!(p.until(30, |p| p.fired("armas")), "{}", p.trace_tail());
    let (balin, balan, merlin, royns) = (1117u16, 1119u16, 1118u16, 1362u16);
    let duo = [balin, balan];
    let pike = |c: usize| c == 7;
    // The three ride along the very edge of the map, out of sight of the
    // longbowmen by the ruins and of the towers, to Royns (2, 118), the
    // knights waiting for Merlin at each stage. There Merlin's lightning
    // (150, a pikeman has 75) and the knights cut down his pikemen first
    // (600% against riders, BONUS.INI); then Balin strikes Royns down to
    // 70, when he is taken (owner 8, sharing his units with the player).
    let trio = [balin, balan, merlin];
    let route = [
        [186, 190],
        [140, 190],
        [90, 190],
        [40, 190],
        [3, 188],
        [3, 150],
        [6, 128],
    ];
    let mut leg = 0;
    let taken = p.until(600, |p| {
        if gap(p.at(merlin), p.at(royns)) <= 16 {
            p.bolt(merlin, &[2], pike);
        }
        if p.fired("roynspierdefuerzas") {
            return true;
        }
        let mut fighting = false;
        for &h in &duo {
            fighting |= p.engage(h, 7, &[2], pike) || p.engage(h, 7, &[2], |_| true);
        }
        if fighting {
        } else if leg < route.len() {
            if gap(p.at(balin), route[leg]) <= 3 && gap(p.at(merlin), route[leg]) <= 5 {
                leg += 1;
            } else if p.sim.ticks % 80 == 0 {
                p.order("move", &trio, route[leg], None);
            }
        } else if let Some(&(_, j, at, _)) =
            p.foes_near(p.at(royns), 8, &[2]).iter().find(|t| pike(t.3))
        {
            let id = p.m.objects[j].id;
            for &h in &duo {
                if !p.fighting(h) {
                    p.order("attack", &[h], at, Some(id));
                }
            }
        } else if !p.fighting(balin) {
            let at = p.at(royns);
            p.order("attack", &[balin], at, Some(royns));
        }
        false
    });
    assert!(taken, "{}", p.trace_tail());
    assert_eq!(p.m.objects[p.obj(royns)].owner, 8);
    // Royns walks back to the ruins (177–192, 177–192) the way they came,
    // the knights and Merlin with him.
    let back: Vec<[u16; 2]> = route.iter().rev().copied().chain([[184, 184]]).collect();
    let mut leg = 0;
    let home = p.until(600, |p| {
        p.bolt(merlin, &[2], |_| true);
        if p.fired("roynsenlazona") {
            return true;
        }
        let mut fighting = false;
        for &h in &duo {
            fighting |= p.engage(h, 7, &[2, 7], |_| true);
        }
        let r = p.at(royns);
        if gap(r, back[leg]) <= 3 {
            leg = (leg + 1).min(back.len() - 1);
        } else if p.sim.ticks % 80 == 0 && p.sim.gameplay.actors[p.obj(royns)].intent.is_none() {
            p.order("move", &[royns], back[leg], None);
        }
        if !fighting && p.sim.ticks % 80 == 0 {
            p.order("move", &trio, r, None);
        }
        false
    });
    assert!(home, "{}", p.trace_tail());
    // Merlin and Balin, worn by the ride, keep their ground at the ruins.
    p.stand(&[merlin, balin]);
    // Nero shows himself, Arthur's army comes, and the enemy castle (1047)
    // is to be destroyed.
    assert!(
        p.until(120, |p| p.fired("misiondosc")),
        "{}",
        p.trace_tail()
    );
    let heroes = [balin, balan, merlin, 277, 1512];
    for id in [277, 1512] {
        assert_eq!(p.m.objects[p.obj(id)].owner, 4);
    }
    // Arthur's army marches together along the south and west edges, out
    // of reach of the ring of towers round Terrabic, to the enemy castle
    // (1047) in the north-west, then strikes whatever stands near, the
    // towers next, the castle last. The knights of the story keep by Merlin
    // at the ruins.
    let castle = 1047;
    let guards = [balan, 277, 1512];
    let army = p.army(1, &heroes);
    let way = [[100, 189], [40, 189], [4, 186], [4, 120], [5, 62]];
    let mut leg = 0;
    let broken = p.until(1800, |p| {
        p.guard(&guards, merlin, 10, &[2, 7]);
        if p.sim.ticks % 60 != 0 {
        } else if leg < way.len() {
            if p.march(&army, way[leg], &[2, 7], 8) {
                leg += 1;
            }
        } else {
            p.assault(&army, &[2, 7], 12, Some(castle));
        }
        p.fired("castillodestruido")
    });
    assert!(broken, "{}", p.trace_tail());
    // Terrabic is the player's: its castle (440) shelters the heroes and
    // heals them 2 a second while one is hurt.
    let terrabic = 440;
    assert_eq!(p.m.objects[p.obj(terrabic)].owner, 1);
    // They ride to the castle first, then go in; one whose way stays shut
    // among the town's peasants is sent again.
    let at = p.at(terrabic);
    let t = p.obj(terrabic);
    p.order("move", &heroes, [103, 93], None);
    p.until(120, |p| {
        heroes.iter().all(|&h| gap(p.at(h), [103, 93]) <= 6)
    });
    assert!(
        p.until(240, |p| {
            if p.sim.ticks % 200 == 0 {
                let out: Vec<u16> = heroes
                    .iter()
                    .copied()
                    .filter(|&h| p.sim.objects[p.obj(h)].holder != Some(t))
                    .collect();
                p.try_order("context", &out, at, Some(terrabic));
            }
            heroes
                .iter()
                .all(|&h| p.sim.objects[p.obj(h)].holder == Some(t))
        }),
        "{:?}",
        heroes.map(|h| (p.at(h), p.sim.objects[p.obj(h)].holder))
    );
    // Thirty minutes after Arthur came, King Lot and his ten nobles bring
    // their armies from every side, and Terrabic's towers break them.
    assert!(
        p.until(2400, |p| p.fired("BATALLAFINAL")),
        "{}",
        p.trace_tail()
    );
    assert!(p.fired("TIEMPOLOTAPARECE"), "{}", p.trace_tail());
    let spawned = p.army(2, &[]).len();
    assert!(spawned >= 300, "{spawned}");
    assert!(
        p.until(1500, |p| p.army(2, &[]).len() <= spawned / 10),
        "{} ribelli",
        p.army(2, &[]).len()
    );
    // The nobles, units of the story that their computer does not send
    // (0x410cd0), wait on the west edge. The player's last battle against
    // them and the rebels left, which wants an army of pikemen raised with
    // Terrabic's peasants, is not played here: they fall as that army
    // would bring them down, and the end of the mission follows.
    let last: Vec<usize> =
        p.m.objects
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                alive(o)
                    && o.flags & FLAG_INACTIVE == 0
                    && (o.owner == 7 || (o.owner == 2 && (3..=38).contains(&o.class)))
            })
            .map(|(i, _)| i)
            .collect();
    for i in last {
        kill(&mut p.sim, &mut p.m, &p.g, &p.lib, i);
    }
    assert!(
        p.until(60, |p| p.sim.gameplay.script.next_map.is_some()),
        "{}",
        p.trace_tail()
    );
    for rule in ["Lot", "todosnoblesmuertos", "FINAL"] {
        assert!(p.fired(rule), "{rule}: {}", p.trace_tail());
    }
    for h in heroes {
        assert!(alive(&p.m.objects[p.obj(h)]), "#{h}");
    }
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\4 La Hermandad de la Mesa Redonda.wmp")
    );
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_brotherhood_of_the_round_table_is_won_by_fleeing_and_raising_the_cathedral() {
    let mut p = Play::open("4 la hermandad de la mesa redonda");
    // An ambush: the Vikings fall on the camp while the knights cover the
    // flight south of Arthur, Guinevere (a spy of 50) and the peasants.
    assert!(
        p.until(10, |p| p.fired("anademision01")),
        "{}",
        p.trace_tail()
    );
    let knights = [385u16, 386, 387, 388];
    let queen = 389;
    let peons: Vec<u16> =
        p.m.objects
            .iter()
            .filter(|o| o.owner == 1 && alive(o) && o.class == class::PEON)
            .map(|o| o.id)
            .collect();
    p.order("move", &knights, [100, 60], None);
    p.order("move", &[queen], [104, 52], None);
    p.order("move", &peons, [104, 50], None);
    // On the way the five Viking kings (kingdom 7) bar the road: the
    // knights strike them down, and the treasure of their defeat is paid.
    let kings = p.until(300, |p| {
        if p.fired("venalosreyesvik") {
            for &k in &knights {
                p.engage(k, 20, &[7], |_| true);
            }
        }
        p.fired("derrotareyes")
    });
    assert!(kings, "{}", p.trace_tail());
    assert!(p.fired("anademision02"));
    assert_eq!(p.m.kingdoms[0].resources, [3000, 6000, 3000, 6000]);
    // Two minutes on, the Viking castle shows itself and the last task is
    // given: a cathedral where the knights camped (120–162, 7–34), the
    // Vikings and the Saracens wiped out.
    assert!(p.until(200, |p| p.fired("fase2")), "{}", p.trace_tail());
    // The war against the Vikings (some four hundred) and the Saracens is
    // not played here: their kingdoms fall as the player's armies would
    // bring them down.
    let last: Vec<usize> =
        p.m.objects
            .iter()
            .enumerate()
            .filter(|(_, o)| matches!(o.owner, 2 | 5) && alive(o) && o.flags & FLAG_INACTIVE == 0)
            .map(|(i, _)| i)
            .collect();
    for i in last {
        kill(&mut p.sim, &mut p.m, &p.g, &p.lib, i);
    }
    assert!(
        p.until(10, |p| p.fired("notienesentidoderrotar")),
        "{}",
        p.trace_tail()
    );
    // The peasants raise the cathedral; the mission ends when it stands
    // finished (0x4b9ad0 does not count a site).
    let builder = p.obj(peons[0]);
    let mut site = None;
    'place: for y in (10..32).step_by(2) {
        for x in (122..160).step_by(2) {
            let req = request("cathedral", vec![builder], [x, y], None);
            if issue(&mut p.sim, &mut p.m, &p.g, &p.lib, &p.c, &req).is_ok() {
                site =
                    p.m.objects
                        .iter()
                        .rev()
                        .find(|o| o.class == class::CATHEDRAL && o.owner == 1)
                        .map(|o| o.id);
                break 'place;
            }
        }
    }
    let site = site.expect("nessun posto per la cattedrale");
    let at = p.at(site);
    p.order("context", &peons, at, Some(site));
    p.until(2, |_| false);
    assert!(!p.fired("VICTORIA"), "{}", p.trace_tail());
    assert!(
        p.until(600, |p| p.sim.gameplay.script.next_map.is_some()),
        "{}",
        p.trace_tail()
    );
    assert_eq!(
        p.m.objects[p.obj(site)].flags & FLAG_CONSTRUCTION,
        0,
        "la vittoria prima della cattedrale"
    );
    for h in knights.iter().chain([queen].iter()) {
        assert!(alive(&p.m.objects[p.obj(*h)]), "#{h}");
    }
    assert!(p.fired("abadiaeregida"));
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\5 Morgana Le Fay.wmp")
    );
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn morgan_le_fay_is_won_by_taking_back_excalibur_and_saving_lancelot() {
    let mut p = Play::open("5 morgana le fay");
    assert!(p.until(20, |p| p.fired("inicio02")), "{}", p.trace_tail());
    let party = [
        257u16, 259, 248, 252, 255, 256, 258, 262, 249, 253, 254, 251, 247, 250, 246, 260,
    ];
    let (ewain, priest) = (246u16, 367u16);
    // The walk north to the Saracen camp where the archbishop is held is
    // not played.
    p.carry(&party, [70, 192]);
    // The camp's guards fall and its wooden gate is broken. Sir Ewain alone
    // goes in: the first fighter beside a prisoner of owner 9 frees him for
    // his own kingdom (0x502e18), and the archbishop must be the player's
    // for the altar to spare him.
    let gate = 2912u16;
    assert!(
        p.until(240, |p| {
            if p.sim.ticks % 60 == 0 {
                p.assault(&party, &[2], 10, Some(gate));
            }
            !alive(&p.m.objects[p.obj(gate)])
        }),
        "{}",
        p.trace_tail()
    );
    let others: Vec<u16> = party.iter().copied().filter(|&h| h != ewain).collect();
    p.try_order("move", &others, [70, 196], None);
    let at = p.at(priest);
    p.order("move", &[ewain], [at[0] + 1, at[1]], None);
    assert!(
        p.until(120, |p| {
            if p.sim.ticks % 60 == 0 {
                for &u in &others {
                    p.engage(u, 8, &[2], |_| true);
                }
            }
            p.m.objects[p.obj(priest)].owner != 9
        }),
        "{}",
        p.trace_tail()
    );
    assert_eq!(p.m.objects[p.obj(priest)].owner, 1);
    assert!(p.fired("venalcura"));
    // The way to the ghost town on the east coast is not played; there a
    // surprise attack comes, and the carpets take the party away.
    let mut all = party.to_vec();
    all.push(priest);
    p.carry(&all, [325, 352]);
    assert!(p.until(30, |p| p.fired("vanalaciudadfantasma")));
    p.order("move", &all, [321, 341], None);
    assert!(
        p.until(60, |p| p.fired("ataquesorpresa02")),
        "{}",
        p.trace_tail()
    );
    p.order("move", &all, [319, 331], None);
    assert!(
        p.until(60, |p| p.fired("salenalfombras")),
        "{}",
        p.trace_tail()
    );
    // The altar's sun kills the friends; Nyneve, Sir Ewain and the
    // archbishop (who is the player's) wake up by Morgana's land.
    assert!(p.fired("existeelcura"), "{}", p.trace_tail());
    let trio = [2887u16, 2889, 2911];
    p.until(10, |_| false);
    for h in trio {
        assert!(alive(&p.m.objects[p.obj(h)]), "#{h}");
    }
    // The way through Morgana's lands and the fight with her three queens
    // are not played: the queens, the guard of the sword's passage and its
    // palisade fall.
    let fallen: Vec<usize> =
        p.m.objects
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                alive(o)
                    && (matches!(o.id, 2855..=2857 | 219 | 125 | 220)
                        || (o.owner == 8 && [o.x, o.y] == [154, 127]))
            })
            .map(|(i, _)| i)
            .collect();
    for i in fallen {
        kill(&mut p.sim, &mut p.m, &p.g, &p.lib, i);
    }
    p.carry(&trio, [154, 139]);
    // Sir Ewain takes Excalibur from the end of its walled passage and
    // carries it out: held, it takes each cell of its bearer (0x500a50),
    // and crosses the strip the script watches.
    let (bearer, sword) = (2889u16, 2859u16);
    let at = p.at(sword);
    p.order("pick_item", &[bearer], at, Some(sword));
    let b = p.obj(bearer);
    assert!(
        p.until(60, |p| p.sim.objects[p.obj(sword)].holder == Some(b)),
        "{:?}",
        p.at(bearer)
    );
    p.order("move", &[bearer], [154, 140], None);
    assert!(
        p.until(60, |p| p.fired("tienenexcalibur")),
        "{}",
        p.trace_tail()
    );
    assert_eq!(p.var("$excalibursi"), 2);
    // The way to Lancelot's prison is not played. He has dug his way out:
    // the heroes follow the tunnel with the sword and steal a ship.
    p.carry(&trio, [243, 140]);
    assert!(
        p.until(30, |p| p.fired("lleganylancelotpirao")),
        "{}",
        p.trace_tail()
    );
    let lancelot = 2909u16;
    assert!(p.until(30, |p| p.m.objects[p.obj(lancelot)].owner == 1));
    // Lancelot's flight past the orcs to the river's mouth is not played.
    p.carry(&[lancelot], [382, 362]);
    assert!(
        p.until(60, |p| p.sim.gameplay.script.next_map.is_some()),
        "{}",
        p.trace_tail()
    );
    assert!(p.fired("lancelotenlazona"));
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\6 El Santo Grial.wmp")
    );
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_holy_grail_is_won_by_raising_a_cathedral_in_sarras_and_bringing_it_back() {
    let mut p = Play::open("6 el santo grial");
    assert!(
        p.until(20, |p| p.fired("INICIO") && !p.sim.gameplay.script.modal),
        "{}",
        p.trace_tail()
    );
    // The caravan (the Grail's cart and four carts of wood and stone) rides
    // west to Sarras, the knights striking whoever comes near the Grail.
    let carts = [7u16, 328, 326, 329, 327];
    let knights: Vec<u16> =
        p.m.objects
            .iter()
            .filter(|o| o.owner == 1 && alive(o) && o.class == 8)
            .map(|o| o.id)
            .collect();
    // The three knights who must live take shelter in Sarras' outpost,
    // which heals its garrison, while the bandits of the ambush follow the
    // caravan into the city.
    let heroes = [250u16, 248, 249];
    let outpost =
        p.m.objects
            .iter()
            .find(|o| o.owner == 1 && alive(o) && o.class_name() == "outpost")
            .map(|o| o.id)
            .unwrap();
    let sarras = [30, 12];
    p.order("move", &carts, sarras, None);
    p.order("move", &knights, sarras, None);
    let oat = p.at(outpost);
    p.order("context", &heroes, oat, Some(outpost));
    let held = p.obj(outpost);
    // Bandits sack Sarras from the start and the defence of the city is
    // not played: its European peasants, who will lay the cathedral, take
    // shelter in the outpost too (its `[accept]` takes peasants).
    let europeans: Vec<u16> =
        p.m.objects
            .iter()
            .filter(|o| o.owner == 1 && alive(o) && o.class == class::PEON && o.civilization == 1)
            .map(|o| o.id)
            .collect();
    p.order("context", &europeans, oat, Some(outpost));
    // The carts are 2×2 (0x4ed490): they pass the narrow ways one by one and
    // string out, so the knights strike whoever comes near any cart still on
    // the road; the escort goes on until each is in.
    let rules = ["carrom01", "carrom02", "carrop01", "carrop02"];
    let arrived = p.until(900, |p| {
        if p.sim.ticks % 60 == 0 {
            let on_road: Vec<u16> = carts
                .iter()
                .copied()
                .filter(|&c| {
                    let i = p.obj(c);
                    alive(&p.m.objects[i]) && p.m.objects[i].flags & FLAG_INACTIVE == 0
                })
                .collect();
            let threat = on_road.iter().find_map(|&c| {
                p.foes_near(p.at(c), 10, &[2, 4, 5, 7, 8, 9, 10])
                    .first()
                    .map(|&(_, j, at, _)| (p.m.objects[j].id, at))
            });
            if let Some((id, at)) = threat {
                for &k in &knights {
                    if alive(&p.m.objects[p.obj(k)]) && !p.fighting(k) {
                        p.try_order("attack", &[k], at, Some(id));
                    }
                }
            }
            for &cart in &on_road {
                if p.sim.gameplay.actors[p.obj(cart)].intent.is_none() {
                    p.try_order("move", &[cart], sarras, None);
                }
            }
        }
        p.fired("carroenlazona") && rules.iter().all(|r| p.fired(r))
    });
    let where_carts = |p: &Play| {
        carts
            .iter()
            .map(|&c| {
                let i = p.obj(c);
                (
                    c,
                    p.at(c),
                    p.m.objects[i].health,
                    p.sim.gameplay.actors[i].intent.clone(),
                    p.sim.gameplay.actors[i]
                        .movement
                        .as_ref()
                        .map(|m| (m.state, m.failed)),
                )
            })
            .collect::<Vec<_>>()
    };
    assert!(arrived, "{:?} {}", where_carts(&p), p.trace_tail());
    for rule in rules {
        assert!(
            p.fired(rule),
            "{rule}: {:?} {}",
            where_carts(&p),
            p.trace_tail()
        );
    }
    assert_eq!(p.m.kingdoms[0].resources, [0, 3000, 0, 3000]);
    // Some hundred bandits (owner 10) camp round Sarras and followed the
    // caravan: the defence of the city against them is not played, those
    // within 60 cells of it fall.
    let bandits: Vec<usize> = p
        .foes_near([30, 15], 60, &[10])
        .iter()
        .map(|t| t.1)
        .collect();
    for i in bandits {
        kill(&mut p.sim, &mut p.m, &p.g, &p.lib, i);
    }
    assert!(
        heroes
            .iter()
            .all(|&h| p.sim.objects[p.obj(h)].holder == Some(held)),
        "{:?}",
        heroes.map(|h| (p.at(h), p.sim.objects[p.obj(h)].holder))
    );
    // The peasants are let out one by one from the outpost's inventory
    // (0x4a1c8c); the knights stay in.
    for &e in &europeans {
        assert_eq!(p.sim.objects[p.obj(e)].holder, Some(held), "#{e}");
        p.order("leave", &[outpost], oat, Some(e));
    }
    // A cathedral before nightfall: 2000 gold and 2000 stone. Sarras has a
    // gold mine but nowhere to bring the gold: the peasants raise a storage
    // by the mine, dig, and a European one lays the cathedral (the city's
    // peasants are of three civilizations; an Arab's menu has the mosque,
    // another class).
    let peons: Vec<u16> =
        p.m.objects
            .iter()
            .filter(|o| o.owner == 1 && alive(o) && o.class == class::PEON)
            .map(|o| o.id)
            .collect();
    let european =
        p.m.objects
            .iter()
            .find(|o| o.owner == 1 && alive(o) && o.class == class::PEON && o.civilization == 1)
            .map(|o| o.id)
            .expect("nessun contadino europeo");
    let builder = p.obj(european);
    let mine =
        p.m.objects
            .iter()
            .find(|o| o.class == class::RES_GOLD && [o.x, o.y] == [22, 9])
            .map(|o| o.id)
            .unwrap();
    let at = p.at(mine);
    let place = |p: &mut Play, key: &str, near: [u16; 2], class: usize| -> Option<u16> {
        for r in 2..20i32 {
            for dy in -r..=r {
                for dx in -r..=r {
                    let c = [
                        (near[0] as i32 + dx).max(0) as u16,
                        (near[1] as i32 + dy).max(0) as u16,
                    ];
                    let req = request(key, vec![builder], c, None);
                    if issue(&mut p.sim, &mut p.m, &p.g, &p.lib, &p.c, &req).is_ok() {
                        return p
                            .m
                            .objects
                            .iter()
                            .rev()
                            .find(|o| o.class == class && o.owner == 1)
                            .map(|o| o.id);
                    }
                }
            }
        }
        None
    };
    let storage = CLASSES.iter().position(|&c| c == "storage").unwrap();
    let store = place(&mut p, "e_storage", at, storage).expect("nessun posto per il magazzino");
    let sat = p.at(store);
    p.order("context", &peons, sat, Some(store));
    assert!(
        p.until(120, |p| p.m.objects[p.obj(store)].flags & FLAG_CONSTRUCTION
            == 0)
    );
    p.order("context", &peons, at, Some(mine));
    assert!(
        p.until(600, |p| p.m.kingdoms[0].resources[0] >= 2000),
        "{:?}{}",
        p.m.kingdoms[0].resources,
        p.trace_tail()
    );
    let site = place(&mut p, "cathedral", [30, 18], class::CATHEDRAL).expect("nessun posto");
    let cat = p.at(site);
    p.order("context", &peons, cat, Some(site));
    assert!(
        p.until(300, |p| p.fired("catedralconstruida")),
        "{}",
        p.trace_tail()
    );
    assert!(!p.fired("expiratiempo"));
    // A ship steals the Grail by night; it shows up by the Saracen castle,
    // where a vampire queen keeps it.
    assert!(
        p.until(120, |p| p.var("$reliquia") == 1),
        "{}",
        p.trace_tail()
    );
    let (grail, queen) = (388u16, 389u16);
    assert!(alive(&p.m.objects[p.obj(grail)]));
    // The way to the Grail and the fight with the queen are not played:
    // she falls, and the three knights stand by the Grail.
    let q = p.obj(queen);
    kill(&mut p.sim, &mut p.m, &p.g, &p.lib, q);
    let o = p.obj(outpost);
    issue(
        &mut p.sim,
        &mut p.m,
        &p.g,
        &p.lib,
        &p.c,
        &Request::command("emerge", vec![o]),
    )
    .unwrap();
    p.second();
    let g = p.at(grail);
    p.carry(&heroes, [g[0] - 3, g[1]]);
    // One of them touches the Grail's cart, which becomes the player's.
    p.order("move", &[250], [g[0] - 1, g[1]], None);
    assert!(
        p.until(60, |p| p.m.objects[p.obj(grail)].owner == 1),
        "{}",
        p.trace_tail()
    );
    // Its way back to Sarras is not played.
    p.carry(&[grail], [30, 14]);
    assert!(
        p.until(60, |p| p.sim.gameplay.script.next_map.is_some()),
        "{}",
        p.trace_tail()
    );
    assert!(p.fired("santogrialenlazona"));
    for h in heroes {
        assert!(alive(&p.m.objects[p.obj(h)]), "#{h}");
    }
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\7 La traición de Sir Mordred.wmp")
    );
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_treason_of_mordred_is_won_by_saving_guinevere_from_the_stake() {
    let mut p = Play::open("7 la traición de sir mordred");
    // The player is Lancelot's kingdom (2); Camelot is Arthur's.
    assert_eq!(p.m.player(), 2);
    assert!(
        p.until(30, |p| p.fired("INICIO") && !p.sim.gameplay.script.modal),
        "{}",
        p.trace_tail()
    );
    let (lancelot, bors, queen) = (1906u16, 1907u16, 88u16);
    // The way into Camelot, past its walls and towers, is not played:
    // Lancelot and Bors reach the stake, and their touch frees Guinevere.
    let at = p.at(queen);
    p.carry(&[lancelot, bors], [at[0] + 1, at[1] + 1]);
    assert!(
        p.until(10, |p| p.fired("DESATANAGINEBRA")),
        "{}",
        p.trace_tail()
    );
    // Her kingdom (7) and Lancelot's ally and share their units; Arthur's
    // and its ally's turn against her.
    assert_eq!(relation(&p.m, 7, 2), 0x45);
    assert_eq!(relation(&p.m, 2, 7), 0x45);
    assert_ne!(relation(&p.m, 1, 7) & 0x10, 0);
    assert_ne!(relation(&p.m, 8, 7) & 0x10, 0);
    // The flight through the streets of Camelot is not played; out of the
    // north passage, where the gate is the friendly kingdom 4's, they walk
    // to the end of the road.
    let party = [queen, lancelot, bors];
    p.carry(&party, [92, 12]);
    p.order("move", &party, [92, 1], None);
    assert!(
        p.until(120, |p| p.sim.gameplay.script.next_map.is_some()),
        "{}",
        p.trace_tail()
    );
    assert!(p.fired("FINAL"));
    for h in party {
        assert!(alive(&p.m.objects[p.obj(h)]), "#{h}");
    }
    assert!(alive(&p.m.objects[p.obj(2245)]), "re Artù");
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\8_a El mito de Camelot.wmp")
    );
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

/// Whether kingdom `k` still has an object the defeat test counts
/// (0x442d10: fighters and buildings whose class 0x442bd0 accepts).
fn standing_objects(p: &Play, k: u8) -> Vec<usize> {
    p.m.objects
        .iter()
        .enumerate()
        .filter(|(_, o)| {
            o.owner == k
                && alive(o)
                && !matches!(o.class, 2 | 61..=64 | 67 | 68 | 133 | 136)
                && (world::class::is_unit(o.class) || o.building())
        })
        .map(|(i, _)| i)
        .collect()
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_myth_of_camelot_is_won_by_razing_the_castles_of_the_traitors() {
    let mut p = Play::open("8_a el mito de camelot");
    assert_eq!(p.m.player(), 1);
    let (arthur, mordred) = (564u16, 865u16);
    // The truce of Salisbury breaks: Mordred rides off and hides in a
    // castle of his, and the quest is to raze them all.
    assert!(
        p.until(30, |p| p.fired("INICIO") && !p.sim.gameplay.script.modal),
        "{}",
        p.trace_tail()
    );
    assert!(
        p.until(20, |p| p
            .m
            .quests
            .iter()
            .any(|q| q.id == crate::script::quest_id("mision01"))),
        "{}",
        p.trace_tail()
    );
    assert!(!p.m.objects.iter().any(|o| o.id == mordred && alive(o)));
    // Arthur, whose fall loses the mission, keeps behind the lines; the
    // army charges Mordred's, which is larger.
    p.order("move", &[arthur], [350, 340], None);
    let army = p.army(1, &[]);
    p.until(60, |p| {
        if p.sim.ticks % 40 == 0 {
            p.assault(&army, &[2, 4, 5], 14, None);
        }
        false
    });
    // The rest of the battle is not played: Mordred's men still on the
    // field fall.
    let field: Vec<usize> =
        p.m.objects
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                o.owner == 2
                    && alive(o)
                    && crate::selection::unit(o)
                    && gap([o.x, o.y], [355, 300]) <= 45
            })
            .map(|(i, _)| i)
            .collect();
    for i in field {
        kill(&mut p.sim, &mut p.m, &p.g, &p.lib, i);
    }
    // The march west is not played; the survivors raze the castle of the
    // kingdom 4, its towers and its peasants.
    let army = p.army(1, &[]);
    assert!(army.len() > 50, "{}", army.len());
    p.carry(&army, [200, 336]);
    assert!(
        p.until(600, |p| {
            if p.sim.ticks % 40 == 0 {
                let c = p.at(army[0]);
                let next = standing_objects(p, 4)
                    .into_iter()
                    .min_by_key(|&i| gap([p.m.objects[i].x, p.m.objects[i].y], c))
                    .map(|i| p.m.objects[i].id);
                let ours: Vec<u16> = army
                    .iter()
                    .copied()
                    .filter(|&id| alive(&p.m.objects[p.obj(id)]))
                    .collect();
                p.assault(&ours, &[2, 4, 5], 12, next);
            }
            standing_objects(p, 4).is_empty()
        }),
        "{:?}",
        standing_objects(&p, 4)
            .iter()
            .map(|&i| p.m.objects[i].class_name())
            .collect::<Vec<_>>()
    );
    assert!(!p.fired("derrotatraidor"));
    // The other two castles fall the same way; not played.
    for k in [2u8, 5] {
        for i in standing_objects(&p, k) {
            kill(&mut p.sim, &mut p.m, &p.g, &p.lib, i);
        }
    }
    assert!(
        p.until(30, |p| p.sim.gameplay.script.next_map.is_some()),
        "{}",
        p.trace_tail()
    );
    assert!(p.fired("derrotatraidor"));
    assert!(!p.fired("Arturo"));
    assert!(alive(&p.m.objects[p.obj(arthur)]));
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\8_b El mito de Camelot.wmp")
    );
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_myth_of_camelot_2_is_won_by_bringing_arthur_to_the_raft() {
    let mut p = Play::open("8_b el mito de camelot");
    assert_eq!(p.m.player(), 1);
    let (arthur, bevidere, mordred) = (62u16, 63u16, 55u16);
    assert!(
        p.until(10, |p| p.fired("INICIO") && !p.sim.gameplay.script.modal),
        "{}",
        p.trace_tail()
    );
    // The last battle: Sir Bevidere, who must live to the end, keeps out of
    // it, and Arthur (of the kingdom 6, which shares its units) strikes
    // Mordred, the only one of his kingdom.
    p.order("move", &[bevidere], [20, 22], None);
    let at = p.at(mordred);
    p.order("attack", &[arthur], at, Some(mordred));
    assert!(
        p.until(60, |p| p.fired("derrotanaranja")),
        "{}",
        p.trace_tail()
    );
    // At 28 s the script ends the rest of both armies.
    assert!(p.until(30, |p| p.fired("muerterojos")));
    assert!(p.army(2, &[]).is_empty());
    assert_eq!(p.army(1, &[]), vec![bevidere]);
    // Bevidere throws the sword into the lake and comes back to the king;
    // the raft comes and both are sent to it.
    assert!(p.until(60, |p| p.fired("alladorey")), "{}", p.trace_tail());
    assert!(p.until(10, |p| {
        p.sim
            .gameplay
            .script
            .trace
            .0
            .as_ref()
            .is_some_and(|t| t.iter().any(|(_, l)| l.contains("todosalabalsa")))
    }));
    // The order's point is the middle of the area (0x4bb0d9), a shore cell
    // no one can reach. Bevidere goes to the nearest one; the king, a unit
    // of a computer kingdom, takes no path that falls short of its goal
    // when the way through the walls has no wall on it (0x4171e0, from the
    // request 0x4df5f0), and stays: the player walks him there.
    p.until(10, |_| false);
    assert!(gap(p.at(bevidere), [50, 7]) <= 1, "{:?}", p.at(bevidere));
    assert!(gap(p.at(arthur), [50, 7]) > 2, "{:?}", p.at(arthur));
    p.order("move", &[arthur], [51, 8], None);
    assert!(
        p.until(60, |p| p.sim.gameplay.script.over.is_some()),
        "{}",
        p.trace_tail()
    );
    for rule in ["enellago", "enlabalsa"] {
        assert!(p.fired(rule), "{rule}");
    }
    // The king leaves on the raft, and the campaign ends won.
    assert!(!p.m.objects.iter().any(|o| o.id == arthur && alive(o)));
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    assert!(p.sim.gameplay.script.next_map.is_none());
}

/// A building of `class` laid by `builder` with the menu key `key` on the
/// first free place around `near`.
fn lay(p: &mut Play, key: &str, builder: u16, near: [u16; 2], class: usize) -> u16 {
    let b = p.obj(builder);
    for r in 1..20i32 {
        for dy in -r..=r {
            for dx in -r..=r {
                let c = [
                    (near[0] as i32 + dx).max(0) as u16,
                    (near[1] as i32 + dy).max(0) as u16,
                ];
                let req = request(key, vec![b], c, None);
                if issue(&mut p.sim, &mut p.m, &p.g, &p.lib, &p.c, &req).is_ok() {
                    return p
                        .m
                        .objects
                        .iter()
                        .rev()
                        .find(|o| o.class == class && o.owner == p.m.objects[b].owner)
                        .map(|o| o.id)
                        .unwrap();
                }
            }
        }
    }
    panic!("nessun posto per {key} presso {near:?}")
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_rise_of_temujin_is_won_by_uniting_the_steppe() {
    let mut p = Play::open("gengis_khan01");
    assert_eq!(p.m.player(), 2);
    let quiet = |p: &Play| !p.sim.gameplay.script.modal;
    // The relatives leave; the Taichiuts strike the camp and take Temujín,
    // his mother and his brothers to a pen of theirs.
    assert!(
        p.until(120, |p| p.var("$crono01") == 1 && quiet(p)),
        "{}",
        p.trace_tail()
    );
    let family = [240u16, 239, 241, 242, 243, 244];
    let living = |p: &Play| -> Vec<u16> {
        family
            .iter()
            .copied()
            .filter(|&id| alive(&p.m.objects[p.obj(id)]))
            .collect()
    };
    // They break the pen's palisade, the guards wake, and they run home by
    // the south.
    let wall =
        p.m.objects
            .iter()
            .find(|o| o.class_name() == "wall_wood" && [o.x, o.y] == [210, 109])
            .map(|o| o.id)
            .unwrap();
    p.order("attack", &family, [210, 109], Some(wall));
    assert!(p.until(120, |p| !alive(&p.m.objects[p.obj(wall)])));
    for to in [[200u16, 135u16], [326, 252]] {
        let ids = living(&p);
        p.order("move", &ids, to, None);
        // A relative whose walk failed in the crowd at the breach is sent
        // again, as a player clicks once more.
        assert!(
            p.until(300, |p| {
                if p.sim.ticks % 40 == 0 {
                    let idle: Vec<u16> = living(p)
                        .into_iter()
                        .filter(|&id| {
                            gap(p.at(id), to) > 5
                                && p.sim.gameplay.actors[p.obj(id)].intent.is_none()
                        })
                        .collect();
                    if !idle.is_empty() {
                        p.try_order("move", &idle, to, None);
                    }
                }
                living(p).iter().all(|&id| gap(p.at(id), to) <= 5)
            }),
            "{}",
            p.trace_tail()
        );
    }
    assert!(alive(&p.m.objects[p.obj(240)]), "Temujín");
    // The clan has 100 wood: a lumber mill by the trees, then the wood of
    // a farm (200), which the winter asks for.
    let lumber = CLASSES.iter().position(|&c| c == "lumber").unwrap();
    let mill = lay(&mut p, "e_lumber", 240, [331, 251], lumber);
    let at = p.at(mill);
    let ids = living(&p);
    p.order("context", &ids, at, Some(mill));
    assert!(
        p.until(200, |p| p.m.objects[p.obj(mill)].flags & FLAG_CONSTRUCTION
            == 0)
    );
    let mut trees: Vec<(i32, [u16; 2])> = Vec::new();
    for y in at[1] - 15..at[1] + 15 {
        for x in at[0] - 15..at[0] + 15 {
            if tree_at(&p.g, &p.lib, [x, y]) {
                trees.push((gap([x, y], at), [x, y]));
            }
        }
    }
    trees.sort();
    for (k, id) in living(&p).into_iter().enumerate() {
        p.order("harvest", &[id], trees[k * 3].1, None);
    }
    assert!(p.until(400, |p| p.m.kingdoms[1].resources[3] >= 200));
    let farm = lay(&mut p, "e_farm", 240, [322, 252], class::FARM);
    let at = p.at(farm);
    let ids = living(&p);
    p.order("context", &ids, at, Some(farm));
    assert!(
        p.until(200, |p| p.fired("construidogranja")),
        "{}",
        p.trace_tail()
    );
    // Years go by: Temujín, grown up, rides after the horse thieves, and
    // Bochurgi (of the owner 10) is sent to him by the script.
    let (temujin, bochurgi) = (279u16, 245u16);
    assert!(
        p.until(200, |p| p.fired("bochurgiencuentraatemujin") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    assert!(p.until(10, |p| p.m.objects[p.obj(bochurgi)].owner == 2));
    // They strike the thieves by the stable; the peasants take it.
    let heroes = [temujin, bochurgi];
    let thieves = [263u16, 256, 253, 284, 285, 281, 280, 283, 282, 265, 262];
    assert!(
        p.until(400, |p| {
            if p.sim.ticks % 40 == 0 {
                p.march(&heroes, [292, 192], &[7], 10);
                p.assault(&heroes, &[7], 14, None);
            }
            thieves.iter().all(|&id| !alive(&p.m.objects[p.obj(id)]))
        }),
        "{}",
        p.trace_tail()
    );
    let stable = 286u16;
    let at = p.at(stable);
    let ids = living(&p);
    p.order("capture", &ids, at, Some(stable));
    assert!(
        p.until(300, |p| p.var("$crono03") == 1 && quiet(p)),
        "{}",
        p.trace_tail()
    );
    assert_eq!(p.m.objects[p.obj(stable)].owner, 2);
    // The ride to Tugril-Khan's court is not played. He asks a dowry.
    let (tugril, borta) = (301u16, 303u16);
    p.carry(&heroes, [162, 425]);
    assert!(
        p.until(60, |p| p.fired("tugrilentrevistado") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    assert_eq!(p.var("$casado"), 1);
    // The harvest of the dowry is not played: the clan has its 400 food,
    // which goes to the Kerait.
    p.m.kingdoms[1].resources[2] = 400;
    assert!(p.until(10, |p| p.fired("KERAITCOMIDA")));
    assert_eq!(p.m.kingdoms[1].resources[2], 0);
    // The Kerait, a computer kingdom, train peasants with it; three of them
    // seal the agreement, and the wedding is held.
    assert!(
        p.until(180, |p| p.var("$casandose") == 2 && quiet(p)),
        "{}",
        p.trace_tail()
    );
    // Borta goes out through the gap in the court's palisade and stops at
    // its edge, where the Merkit come for her. Their rider is sent to the
    // cell she stands on (a Move takes its target's rect when it is given,
    // 0x42df70): had she walked on, he would only shoot her from afar.
    // Temujín, at the court, is ambushed and asks Tugril-Khan for help.
    p.order("move", &[borta], [159, 416], None);
    p.until(30, |p| gap(p.at(borta), [159, 416]) <= 3);
    p.order("move", &[borta], [195, 416], None);
    assert!(
        p.until(120, |p| p.fired("Emboscadacaptura") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    let at = p.at(tugril);
    p.order("move", &heroes, [at[0] + 2, at[1] - 3], None);
    assert!(
        p.until(120, |p| p.fired("temujinpideayuda") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    // The ride north and the two ghosts by the potion of dragon bones are
    // not played. Only Bochurgi, a ninja, climbs to its ledge (0x4a99a0).
    let potion = 465u16;
    for ghost in [1133u16, 25] {
        let i = p.obj(ghost);
        kill(&mut p.sim, &mut p.m, &p.g, &p.lib, i);
    }
    p.carry(&heroes, [281, 68]);
    let at = p.at(potion);
    p.order("pick_item", &[bochurgi], at, Some(potion));
    let b = p.obj(bochurgi);
    assert!(p.until(30, |p| p.sim.objects[p.obj(potion)].holder == Some(b)));
    // The way back is not played. Tugril-Khan heals and joins the clan,
    // sharing his units.
    let at = p.at(tugril);
    p.carry(&[bochurgi, temujin], [at[0] + 2, at[1]]);
    assert!(
        p.until(30, |p| p.var("$crono05") == 1 && quiet(p)),
        "{}",
        p.trace_tail()
    );
    assert_eq!(relation(&p.m, 2, 4), 0x45);
    assert_eq!(relation(&p.m, 4, 2), 0x45);
    // The war on the Merkit is not played: their castle falls. Borta comes
    // back, and the relatives with their trade guild.
    let i = p.obj(6);
    kill(&mut p.sim, &mut p.m, &p.g, &p.lib, i);
    assert!(
        p.until(30, |p| p.var("$crono06") == 1 && quiet(p)),
        "{}",
        p.trace_tail()
    );
    assert_eq!(p.m.objects[p.obj(508)].owner, 2);
    // Two markets: the gathering is not played (the clan has the 400 food
    // and 400 wood), the peasants raise them.
    p.m.kingdoms[1].resources[2] = 400;
    p.m.kingdoms[1].resources[3] = 400;
    let peons: Vec<u16> =
        p.m.objects
            .iter()
            .filter(|o| {
                o.owner == 2 && alive(o) && o.class == class::PEON && o.flags & FLAG_INACTIVE == 0
            })
            .map(|o| o.id)
            .collect();
    for k in 0..2 {
        let site = lay(&mut p, "e_market", peons[0], [310, 265], class::MARKET);
        let at = p.at(site);
        let half: Vec<u16> = peons.iter().copied().skip(k).step_by(2).collect();
        p.order("context", &half, at, Some(site));
    }
    assert!(
        p.until(300, |p| p.var("$crono07") == 1 && quiet(p)),
        "{}",
        p.trace_tail()
    );
    // The magic bow shows up in a pen on the Taichiut hill: the ride there
    // is not played, Bochurgi climbs in and brings it to Temujín.
    let bow = 560u16;
    assert!(p.until(30, |p| p.var("$arcooo") == 1 && quiet(p)));
    p.until(2, |_| false);
    p.carry(&heroes, [250, 104]);
    let at = p.at(bow);
    p.order("pick_item", &[bochurgi], at, Some(bow));
    assert!(p.until(90, |p| p.sim.objects[p.obj(bow)].holder == Some(b)));
    let at = p.at(temujin);
    p.order("move", &[bochurgi], [at[0] - 1, at[1]], None);
    assert!(
        p.until(90, |p| p.fired("encuentraarco") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    // Yamuga, jealous, leaves; the vengeance on the Tartars follows, whose
    // war is not played: their castle falls, and Yamuga turns against the
    // clan.
    assert!(p.until(30, |p| p.fired("tartarosvenganza") && quiet(p)));
    let i = p.obj(320);
    kill(&mut p.sim, &mut p.m, &p.g, &p.lib, i);
    assert!(
        p.until(30, |p| p.var("$crono09") == 1 && quiet(p)),
        "{}",
        p.trace_tail()
    );
    // The hunt of Yamuga's men is not played; he shows up again among the
    // Naimans.
    let yamuga = 611u16;
    for id in [
        507u16, 503, 494, 498, 506, 501, 496, 499, 505, 502, 495, 500, 504, 497,
    ] {
        let i = p.obj(id);
        if alive(&p.m.objects[i]) {
            kill(&mut p.sim, &mut p.m, &p.g, &p.lib, i);
        }
    }
    assert!(
        p.until(30, |p| p.fired("yamugareaparece") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    // The ride to the Naimans is not played. Wounded, Yamuga surrenders.
    let at = p.at(yamuga);
    p.carry(&heroes, [at[0] + 6, at[1] + 6]);
    p.order("attack", &heroes, at, Some(yamuga));
    assert!(
        p.until(120, |p| p.var("$rendicion") == 1),
        "{}",
        p.trace_tail()
    );
    p.stand(&heroes);
    // The ride to Alana in the south-east and the way back to Yamuga are
    // not played: the truth about his daughter makes him loyal.
    let alana = 31u16;
    assert!(p.until(10, |p| quiet(p)
        && p.m.objects[p.obj(alana)].flags & FLAG_INACTIVE == 0));
    let at = p.at(alana);
    p.carry(&heroes, [at[0] - 5, at[1] - 5]);
    assert!(p.until(30, |p| p.var("$aclarado") == 1 && quiet(p)));
    let at = p.at(yamuga);
    p.carry(&heroes, [at[0] + 5, at[1] + 5]);
    assert!(
        p.until(60, |p| p.sim.gameplay.script.next_map.is_some()),
        "{}",
        p.trace_tail()
    );
    assert!(p.fired("yamugaleal"));
    for id in [temujin, yamuga] {
        assert!(alive(&p.m.objects[p.obj(id)]), "#{id}");
    }
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\Gengis_Khan02.wmp")
    );
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_conquest_of_beijing_is_won_by_taking_the_imperial_palace() {
    let mut p = Play::open("gengis_khan02");
    assert_eq!(p.m.player(), 2);
    // The heroes as the first mission's way leaves them (its test ends with
    // these): Tugril-Khan and Yamuga at level 20, Borta at 4, the old man
    // at 1; the dwarves, the orcs, the monks and the wizard were never won.
    let hero = |name: &str, health: u16, max_health: u16, level: u16| scripting::Hero {
        name: name.into(),
        subtitle: String::new(),
        health,
        max_health,
        level,
        promoted: false,
    };
    let carried = vec![
        hero("Temujin", 282, 335, 7),
        hero("Bochurgi", 201, 335, 7),
        hero("Unojo", 350, 350, 19),
        hero("Zhorin", 450, 450, 19),
        hero("Vecchio", 60, 60, 1),
        hero("Batu", 300, 300, 0),
        hero("Jagadai", 60, 60, 19),
        hero("Borta", 44, 60, 4),
        hero("Tugril", 180, 180, 20),
        hero("Yamuga", 900, 900, 20),
    ];
    restore_heroes(&mut p.sim, &mut p.m, &p.lib, &carried);
    assert!(
        p.until(60, |p| p.fired("inicio") && !p.sim.gameplay.script.modal),
        "{}",
        p.trace_tail()
    );
    // Each hero comes with his men when his level says he was won.
    let (temujin, yamuga, tugril, borta, bochurgi, old) =
        (4021u16, 4139u16, 3914u16, 4020u16, 61u16, 66u16);
    for id in [temujin, yamuga, tugril, borta, bochurgi, old] {
        assert!(alive(&p.m.objects[p.obj(id)]), "#{id}");
    }
    for (rule, id) in [
        ("zhorinnoesta", 3918u16),
        ("unojonoesta", 3917),
        ("batunoesta", 4138),
        ("jagadainoesta", 4137),
    ] {
        assert!(p.fired(rule), "{rule}");
        assert!(!alive(&p.m.objects[p.obj(id)]), "#{id}");
    }
    for rule in ["tugrilesta", "yamugaesta", "ancianoesta"] {
        assert!(p.fired(rule), "{rule}");
    }
    // `bortanivel2 !$bortalimita==1` lacks an operator: 0x4b7860 evaluates
    // from the top of the stack and leaves the level below, so the first
    // of Borta's rules gives the bow whatever her level.
    assert!(p.fired("bortaestaarco"));
    assert!(!p.fired("bortatodo"));
    assert!(alive(&p.m.objects[p.obj(4022)]));
    // The march on Beijing is not played; when the clan's peasants come
    // into the Chinese lands Yamuga's men withdraw and a messenger asks
    // Gengis Khan to come alone. He stays with his army.
    let palace = 1878u16;
    let at = p.at(palace);
    let guards: Vec<usize> =
        p.m.objects
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                matches!(o.owner, 1 | 4 | 6)
                    && alive(o)
                    && o.flags & FLAG_INACTIVE == 0
                    && (crate::selection::unit(o) || o.class_name().starts_with("tower"))
                    && gap([o.x, o.y], at) <= 20
            })
            .map(|(i, _)| i)
            .collect();
    // The fight for the palace is not played either: its guards fall.
    for i in guards {
        kill(&mut p.sim, &mut p.m, &p.g, &p.lib, i);
    }
    let peons: Vec<u16> =
        p.m.objects
            .iter()
            .filter(|o| o.owner == 2 && alive(o) && o.class == class::PEON)
            .map(|o| o.id)
            .take(10)
            .collect();
    p.carry(&peons, [at[0] + 6, at[1] + 8]);
    assert!(
        p.until(30, |p| p.var("$traicionado") == 1),
        "{}",
        p.trace_tail()
    );
    let men = [4197u16, 4160, 4158, 4155, 4147, 4196, 4153, 4157];
    for id in men {
        assert_eq!(p.m.objects[p.obj(id)].owner, 8, "#{id}");
    }
    // The peasants take the Imperial Palace.
    p.order("capture", &peons, at, Some(palace));
    assert!(
        p.until(120, |p| p.sim.gameplay.script.over.is_some()),
        "{}",
        p.trace_tail()
    );
    assert_eq!(p.m.objects[p.obj(palace)].owner, 2);
    assert!(p.fired("GANAR"));
    assert!(!p.fired("traicionado"));
    assert!(alive(&p.m.objects[p.obj(temujin)]));
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    assert!(p.sim.gameplay.script.next_map.is_none());
}

/// A mission of the introductory campaign.
fn tutorial(map: &str) -> Play {
    let catalog = crate::scenario::ScenarioCatalog::load(root()).unwrap();
    let (campaign, mission) = catalog
        .locate(&format!("tzartutorial.wcm::maps/{map}"))
        .unwrap();
    Play::open_at(&catalog, campaign.unwrap(), mission)
}
/// The trees nearest to `at`, within 20 cells.
fn trees_near(p: &Play, at: [u16; 2]) -> Vec<[u16; 2]> {
    let mut trees: Vec<(i32, [u16; 2])> = Vec::new();
    for y in at[1].saturating_sub(20)..at[1] + 20 {
        for x in at[0].saturating_sub(20)..at[0] + 20 {
            if tree_at(&p.g, &p.lib, [x, y]) {
                trees.push((gap([x, y], at), [x, y]));
            }
        }
    }
    trees.sort();
    trees.into_iter().map(|t| t.1).collect()
}
/// The living peasants of kingdom `k`.
fn peasants(p: &Play, k: u8) -> Vec<u16> {
    p.m.objects
        .iter()
        .filter(|o| {
            o.owner == k && alive(o) && o.class == class::PEON && o.flags & FLAG_INACTIVE == 0
        })
        .map(|o| o.id)
        .collect()
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_introduction_is_won_by_following_haaser_to_the_enemy_outpost() {
    let mut p = tutorial("intro");
    assert_eq!(p.m.player(), 1);
    let quiet = |p: &Play| !p.sim.gameplay.script.modal;
    // Haaser asks whether the player knows a strategy game; the first
    // answer (no) gives the lessons.
    assert!(p.until(30, |p| p.var("$selecting") == 2 && quiet(p)));
    // Select the peasant, as the interface hands over the selection.
    let peon = 114u16;
    let i = p.obj(peon);
    p.sim.gameplay.selected = [i].into();
    assert!(p.until(10, |p| p.var("$move") == 1 && quiet(p)));
    let castle = 1u16;
    let c = p.at(castle);
    p.order("move", &[peon], [c[0] + 4, c[1] + 6], None);
    assert!(
        p.until(60, |p| p.var("$chwood") == 1 && quiet(p)),
        "{}",
        p.trace_tail()
    );
    let trees = trees_near(&p, c);
    for (k, id) in peasants(&p, 1).into_iter().enumerate() {
        p.order("harvest", &[id], trees[k * 3], None);
    }
    // Wood brings three peasants for the fields.
    assert!(
        p.until(200, |p| p.fired("Wood") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    let fields: Vec<u16> =
        p.m.objects
            .iter()
            .filter(|o| o.owner == 1 && alive(o) && o.class == 63)
            .map(|o| o.id)
            .collect();
    for (k, id) in [25u16, 23, 22].into_iter().enumerate() {
        let f = fields[k % fields.len()];
        let at = p.at(f);
        p.order("harvest", &[id], at, Some(f));
    }
    // Food brings a fishing boat, which goes fishing.
    assert!(
        p.until(200, |p| p.fired("Fishing") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    let boat = 112u16;
    let b = p.at(boat);
    let patch =
        p.m.objects
            .iter()
            .filter(|o| o.class == class::FISHPATCH)
            .min_by_key(|o| gap([o.x, o.y], b))
            .map(|o| o.id)
            .unwrap();
    let at = p.at(patch);
    p.order("context", &[boat], at, Some(patch));
    // More food: a new peasant from the castle, who digs gold.
    assert!(
        p.until(300, |p| p.fired("Peasants") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    let before = peasants(&p, 1);
    p.order("e_peon", &[castle], c, None);
    assert!(
        p.until(120, |p| p.fired("MineGold") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    let new: Vec<u16> = peasants(&p, 1)
        .into_iter()
        .filter(|id| !before.contains(id))
        .collect();
    let gold = 5u16;
    let at = p.at(gold);
    p.order("context", &new, at, Some(gold));
    // The first quest: 200 of each resource.
    assert!(
        p.until(200, |p| p.var("$quest") == 2 && quiet(p)),
        "{}",
        p.trace_tail()
    );
    let peons = peasants(&p, 1);
    let stone = 113u16;
    let (ga, sa) = (p.at(gold), p.at(stone));
    p.order("context", &peons[0..2], ga, Some(gold));
    p.order("context", &peons[2..4], sa, Some(stone));
    assert!(
        p.until(400, |p| p.fired("QuestDone") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    // A barracks, and an army.
    let site = lay(
        &mut p,
        "e_barrack",
        peons[4],
        [c[0] + 8, c[1] + 8],
        class::BARRACK,
    );
    let at = p.at(site);
    p.order("context", &peons[4..], at, Some(site));
    assert!(
        p.until(200, |p| p.fired("BuiltBarracks") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    for _ in 0..5 {
        p.order("e_footman", &[site], at, None);
    }
    // The enemy comes from the south-east; the army beats it and razes its
    // barracks.
    assert!(
        p.until(200, |p| p.fired("EnemyAttack") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    let army = p.army(1, &[]);
    assert!(
        p.until(200, |p| {
            if p.sim.ticks % 40 == 0 {
                p.assault(&army, &[2], 20, None);
            }
            p.fired("AttackEnemy") && quiet(p)
        }),
        "{}",
        p.trace_tail()
    );
    let barracks = 121u16;
    assert!(
        p.until(300, |p| {
            if p.sim.ticks % 40 == 0 {
                let army = p.army(1, &[]);
                p.march(&army, [110, 42], &[2], 8);
                p.assault(&army, &[2], 12, Some(barracks));
            }
            p.fired("BarracksDestroyed") && quiet(p)
        }),
        "{}",
        p.trace_tail()
    );
    // Two watchtowers by the castle (300 wood each: more wood is cut for
    // the second).
    let peons = peasants(&p, 1);
    let first = lay(
        &mut p,
        "e_tower_wood",
        peons[0],
        [c[0] + 4, c[1] + 10],
        class::TOWER_WOOD,
    );
    let at = p.at(first);
    p.order("context", &peons[0..2], at, Some(first));
    let trees = trees_near(&p, c);
    for (k, &id) in peons[2..].iter().enumerate() {
        p.order("harvest", &[id], trees[k * 3], None);
    }
    assert!(p.until(200, |p| p.m.kingdoms[0].resources[3] >= 300));
    let second = lay(
        &mut p,
        "e_tower_wood",
        peons[0],
        [c[0] + 10, c[1] + 10],
        class::TOWER_WOOD,
    );
    let at = p.at(second);
    p.order("context", &peons[0..2], at, Some(second));
    assert!(
        p.until(200, |p| p.fired("Upgrade") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    // A minute later an enemy outpost shows up; the peasants take it, the
    // army at their side (soldiers do not capture).
    assert!(
        p.until(90, |p| p.fired("ConvertOutpost") && quiet(p)),
        "{}",
        p.trace_tail()
    );
    let outpost = 3u16;
    let at = p.at(outpost);
    let army = p.army(1, &[]);
    p.order("move", &army, [at[0] - 3, at[1]], None);
    let peons = peasants(&p, 1);
    p.order("capture", &peons, at, Some(outpost));
    assert!(
        p.until(300, |p| p.sim.gameplay.script.next_map.is_some()),
        "{}",
        p.trace_tail()
    );
    assert!(p.fired("MissionWon"));
    assert!(!p.fired("Fail1"));
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\tutmap1.wmp")
    );
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

/// Idle peasants of kingdom 1 go back to work: `job(k, id)` gives the
/// order for the k-th of them.
fn back_to_work(p: &mut Play, job: impl Fn(&mut Play, usize, u16)) {
    for (k, id) in peasants(p, 1).into_iter().enumerate() {
        let i = p.obj(id);
        if p.sim.gameplay.actors[i].intent.is_none() {
            job(p, k, id);
        }
    }
}
/// Peasant `id` cuts the `k`-th tree near `at`.
fn chop(p: &mut Play, k: usize, id: u16, at: [u16; 2]) {
    let trees = trees_near(p, at);
    if !trees.is_empty() {
        let tree = trees[k % trees.len().min(12)];
        p.try_order("harvest", &[id], tree, None);
    }
}
/// Unit `id` works object `j` with its right click.
fn work_on(p: &mut Play, id: u16, j: u16) {
    let at = p.at(j);
    p.try_order("context", &[id], at, Some(j));
}
/// A fishing boat goes to the nearest shoal.
fn fish(p: &mut Play, boat: u16) {
    let b = p.at(boat);
    if let Some(f) =
        p.m.objects
            .iter()
            .filter(|x| x.class == class::FISHPATCH && alive(x))
            .min_by_key(|x| gap([x.x, x.y], b))
            .map(|x| x.id)
    {
        work_on(p, boat, f);
    }
}
fn class_named(name: &str) -> usize {
    CLASSES.iter().position(|&s| s == name).unwrap()
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_first_lesson_is_won_by_houses_peasants_food_and_wood() {
    let mut p = tutorial("tutmap1");
    assert!(p.until(30, |p| p.fired("Mission start")
        && !p.sim.gameplay.script.modal));
    // The script's default orders on a field's cell set two peasants
    // digging (the peasant's right click at the point, 0x4adc20).
    for id in [40u16, 39] {
        let i = p.obj(id);
        assert!(
            matches!(p.sim.gameplay.actors[i].intent, Some(Intent::Harvest(f)) if p.m.objects[f].class == class::FIELD),
            "#{id} {:?}",
            p.sim.gameplay.actors[i].intent
        );
    }
    let castle =
        p.m.objects
            .iter()
            .find(|o| o.owner == 1 && o.class == class::CASTLE)
            .map(|o| o.id)
            .unwrap();
    let c = p.at(castle);
    let mut houses = 0;
    let mut t = 0;
    assert!(
        p.until(1200, |p| {
            t += 1;
            let r = p.m.kingdoms[0].resources;
            let peons = peasants(p, 1);
            if t % 5 == 0
                && peons.len() < 10
                && r[2] >= 50
                && p.sim.gameplay.actors[p.obj(castle)].jobs.is_empty()
            {
                p.try_order("e_peon", &[castle], c, None);
            }
            if houses < 2 && r[3] >= 75 && t % 5 == 0 {
                let b = *peons.last().unwrap();
                let h = lay(p, "e_house", b, [c[0] + 4, c[1] + 4], class::HOUSE);
                work_on(p, b, h);
                houses += 1;
            }
            p.sim.gameplay.script.next_map.is_some()
        }),
        "{}",
        p.trace_tail()
    );
    for rule in [
        "Houses Built",
        "Peons built",
        "Resources gathered",
        "Player win",
    ] {
        assert!(p.fired(rule), "{rule}");
    }
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\tutmap2.wmp")
    );
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_second_lesson_is_won_by_lumber_storage_farm_and_cattle() {
    let mut p = tutorial("tutmap2");
    assert!(p.until(30, |p| p.fired("Mission start")
        && !p.sim.gameplay.script.modal));
    let castle =
        p.m.objects
            .iter()
            .find(|o| o.owner == 1 && o.class == class::CASTLE)
            .map(|o| o.id)
            .unwrap();
    let c = p.at(castle);
    let (gold, stone) = (35u16, 36u16);
    let woodmen = [39u16, 38, 40];
    // A lumber mill by the wood, with the 100 wood the clan has.
    let mill = lay(&mut p, "e_lumber", 39, [17, 50], class_named("lumber"));
    let at = p.at(mill);
    p.order("context", &woodmen, at, Some(mill));
    assert!(
        p.until(120, |p| p.m.objects[p.obj(mill)].flags & FLAG_CONSTRUCTION
            == 0)
    );
    for (k, id) in woodmen.into_iter().enumerate() {
        chop(&mut p, k * 3, id, at);
    }
    assert!(p.until(300, |p| p.m.kingdoms[0].resources[3] >= 200));
    // A storage by the mines.
    let store = lay(&mut p, "e_storage", 44, [41, 25], class_named("storage"));
    let sat = p.at(store);
    p.order("context", &[44u16, 45], sat, Some(store));
    assert!(
        p.until(120, |p| p.m.objects[p.obj(store)].flags & FLAG_CONSTRUCTION
            == 0)
    );
    let mut farm: Option<u16> = None;
    let (mut researched, mut cows) = (false, 0);
    let mut t = 0;
    assert!(
        p.until(1200, |p| {
            t += 1;
            let r = p.m.kingdoms[0].resources;
            let peons = peasants(p, 1);
            // Two bears roam the map: the peasants by the mines strike the
            // one that comes, all together.
            let bear = [46u16, 47]
                .into_iter()
                .filter(|&b| alive(&p.m.objects[p.obj(b)]))
                .find(|&b| gap(p.at(b), sat) <= 14);
            if let Some(b) = bear {
                if t % 3 == 0 {
                    let at = p.at(b);
                    let near: Vec<u16> = peons
                        .iter()
                        .copied()
                        .filter(|&id| gap(p.at(id), at) <= 20)
                        .collect();
                    p.try_order("attack", &near, at, Some(b));
                }
                return false;
            }
            if t % 5 == 0
                && peons.len() < 10
                && p.sim.gameplay.actors[p.obj(castle)].jobs.is_empty()
            {
                p.try_order("e_peon", &[castle], c, None);
            }
            if t % 10 == 0 {
                back_to_work(p, |p, k, id| {
                    if woodmen.contains(&id) {
                        chop(p, k, id, at);
                    } else {
                        work_on(p, id, if k % 2 == 0 { gold } else { stone });
                    }
                });
            }
            if farm.is_none() && r[3] >= 200 {
                let f = lay(p, "e_farm", 39, [c[0] + 6, c[1] + 6], class::FARM);
                work_on(p, 39, f);
                farm = Some(f);
            }
            // The farm's Livestock (300 gold), then two cows.
            if let Some(f) = farm
                && p.m.objects[p.obj(f)].flags & FLAG_CONSTRUCTION == 0
            {
                let at = p.at(f);
                if !researched && t % 5 == 0 && p.try_order("te_livestock", &[f], at, None) {
                    researched = true;
                }
                if researched && cows < 2 && t % 5 == 0 && p.try_order("cow", &[f], at, None) {
                    cows += 1;
                }
            }
            p.sim.gameplay.script.next_map.is_some()
        }),
        "{} {:?}",
        p.trace_tail(),
        p.m.kingdoms[0].resources
    );
    for rule in [
        "Lumber built",
        "Storage built",
        "Farm built",
        "Resources gathered",
        "Cows built",
    ] {
        assert!(p.fired(rule), "{rule}");
    }
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\Perfect 3.wmp")
    );
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_third_lesson_is_won_by_two_barracks_and_the_hunt_of_the_raiders() {
    let mut p = tutorial("perfect 3");
    assert!(p.until(30, |p| p.fired("Start") && !p.sim.gameplay.script.modal));
    let castle = 61u16;
    let c = p.at(castle);
    let (stone, cow, farm, mill) = (64u16, 72u16, 66u16, 62u16);
    let (fa, ma) = (p.at(farm), p.at(mill));
    work_on(&mut p, 70, cow);
    work_on(&mut p, 69, stone);
    chop(&mut p, 0, 71, ma);
    // Two barracks (200 stone and 200 food each), two fields by the farm
    // for the food, more peasants, then ten footmen.
    let mut barracks: Vec<u16> = Vec::new();
    let mut fields: Vec<u16> = Vec::new();
    let mut t = 0;
    assert!(
        p.until(2400, |p| {
            t += 1;
            let r = p.m.kingdoms[0].resources;
            let peons = peasants(p, 1);
            let builder = *peons.last().unwrap();
            if (barracks.is_empty() || barracks.len() < 2 && fields.len() == 2)
                && r[1] >= 200
                && r[2] >= 200
            {
                let near = [c[0] + 8, c[1] + 8 * barracks.len() as u16];
                let b = lay(p, "e_barrack", builder, near, class::BARRACK);
                work_on(p, builder, b);
                barracks.push(b);
            }
            if fields.len() < 2 && r[3] >= 200 && !barracks.is_empty() {
                let near = [fa[0] - 4 + 4 * fields.len() as u16, fa[1] + 4];
                let f = lay(p, "field", builder, near, class::FIELD);
                work_on(p, builder, f);
                fields.push(f);
            }
            if t % 5 == 0
                && peons.len() < 8
                && r[2] >= 50
                && (barracks.len() == 2 || fields.len() == 2)
                && p.sim.gameplay.actors[p.obj(castle)].jobs.is_empty()
            {
                p.try_order("e_peon", &[castle], c, None);
            }
            if t % 10 == 0 {
                let fields = fields.clone();
                back_to_work(p, |p, k, id| {
                    let busy = |p: &Play, f: u16| {
                        let j = p.obj(f);
                        p.sim
                            .gameplay
                            .actors
                            .iter()
                            .filter(|a| a.intent == Some(Intent::Harvest(j)))
                            .count()
                    };
                    let free = fields.iter().copied().find(|&f| {
                        p.m.objects[p.obj(f)].flags & FLAG_CONSTRUCTION == 0 && busy(p, f) < 2
                    });
                    match free {
                        Some(f) => {
                            let at = p.at(f);
                            p.try_order("harvest", &[id], at, Some(f));
                        }
                        None if id == 69 => work_on(p, id, stone),
                        None => chop(p, k, id, ma),
                    }
                });
            }
            if barracks.len() == 2 && t % 3 == 0 {
                for &b in &barracks {
                    let bi = p.obj(b);
                    if p.m.objects[bi].flags & FLAG_CONSTRUCTION == 0
                        && p.sim.gameplay.actors[bi].jobs.is_empty()
                        && p.army(1, &[]).len() < 10
                    {
                        let at = p.at(b);
                        p.try_order("e_footman", &[b], at, None);
                    }
                }
            }
            p.fired("Have barracks and units") && !p.sim.gameplay.script.modal
        }),
        "{} {:?}",
        p.trace_tail(),
        p.m.kingdoms[0].resources
    );
    // The seven raiders of the region, ghosts included, fall to the army.
    let foes = [75u16, 77, 76, 80, 79, 78, 74];
    assert!(
        p.until(900, |p| {
            let army = p.army(1, &[]);
            let left: Vec<u16> = foes
                .iter()
                .copied()
                .filter(|&f| alive(&p.m.objects[p.obj(f)]))
                .collect();
            if p.sim.ticks % 40 == 0 && !left.is_empty() && !army.is_empty() {
                let here = p.at(army[0]);
                let f = *left.iter().min_by_key(|&&f| gap(p.at(f), here)).unwrap();
                for &u in &army {
                    if !p.fighting(u) {
                        p.close_in(&[u], f);
                    }
                }
            }
            p.sim.gameplay.script.next_map.is_some()
        }),
        "{}",
        p.trace_tail()
    );
    assert!(p.fired("Win"));
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\Perfect 4.wmp")
    );
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_fourth_lesson_is_won_by_a_fleet_and_a_landing_on_the_island() {
    let mut p = tutorial("perfect 4");
    assert!(p.until(30, |p| p.fired("Start") && !p.sim.gameplay.script.modal));
    let (castle, mill) = (100u16, 101u16);
    let (c, ma) = (p.at(castle), p.at(mill));
    let harbor = lay(&mut p, "e_harbor", 110, [56, 14], class::HARBOR);
    let ha = p.at(harbor);
    work_on(&mut p, 110, harbor);
    chop(&mut p, 0, 109, ma);
    chop(&mut p, 3, 108, ma);
    // Two fishing boats, three xebecs and two hulks; the warships leave
    // the harbor's quay to the fishing boats.
    let mut built = [("fishing_boat", 0, 2), ("e_xebec", 0, 3), ("e_hulk", 0, 2)];
    let mut t = 0;
    assert!(
        p.until(3600, |p| {
            t += 1;
            let r = p.m.kingdoms[0].resources;
            let peons = peasants(p, 1);
            if t % 5 == 0
                && peons.len() < 10
                && r[2] >= 150
                && p.sim.gameplay.actors[p.obj(castle)].jobs.is_empty()
            {
                p.try_order("e_peon", &[castle], c, None);
            }
            let ready = p.m.objects[p.obj(harbor)].flags & FLAG_CONSTRUCTION == 0;
            if ready && t % 3 == 0 && p.sim.gameplay.actors[p.obj(harbor)].jobs.is_empty() {
                if let Some(b) = built.iter_mut().find(|b| b.1 < b.2)
                    && p.try_order(b.0, &[harbor], ha, None)
                {
                    b.1 += 1;
                }
            }
            if t % 10 == 0 {
                back_to_work(p, |p, k, id| chop(p, k * 7, id, ma));
                let ships: Vec<(u16, usize)> =
                    p.m.objects
                        .iter()
                        .filter(|o| o.owner == 1 && alive(o) && naval(o))
                        .map(|o| (o.id, o.class))
                        .collect();
                for (s, class) in ships {
                    let i = p.obj(s);
                    if p.sim.gameplay.actors[i].intent.is_some() {
                        continue;
                    }
                    if class == class::FISHING_BOAT {
                        fish(p, s);
                    } else if gap(p.at(s), ha) <= 4 {
                        p.try_order("move", &[s], [44, 26], None);
                    }
                }
            }
            p.fired("Have fleet") && !p.sim.gameplay.script.modal
        }),
        "{} {:?}",
        p.trace_tail(),
        p.m.kingdoms[0].resources
    );
    let xebecs: Vec<u16> =
        p.m.objects
            .iter()
            .filter(|o| o.owner == 1 && alive(o) && o.class == 25)
            .map(|o| o.id)
            .collect();
    let hulk =
        p.m.objects
            .iter()
            .find(|o| o.owner == 1 && alive(o) && o.class == 24)
            .map(|o| o.id)
            .unwrap();
    // A hulk anchors off the beach below the base; the soldiers board it
    // across the cell of shore (the Enter program's range 1, 0x42e318).
    let troops: Vec<u16> = p
        .army(1, &[])
        .into_iter()
        .filter(|&id| !naval(&p.m.objects[p.obj(id)]))
        .collect();
    p.order("move", &[hulk], [65, 31], None);
    assert!(p.until(60, |p| p.sim.gameplay.actors[p.obj(hulk)].intent.is_none()));
    for &u in &troops {
        work_on(&mut p, u, hulk);
    }
    let h = p.obj(hulk);
    assert!(
        p.until(120, |p| troops
            .iter()
            .all(|&u| p.sim.objects[p.obj(u)].holder == Some(h))),
        "{:?}",
        troops
            .iter()
            .map(|&u| (p.at(u), p.sim.objects[p.obj(u)].holder))
            .collect::<Vec<_>>()
    );
    // The xebecs take on the enemy ships.
    p.until(400, |p| {
        let foes: Vec<u16> =
            p.m.objects
                .iter()
                .filter(|o| o.owner == 2 && alive(o) && naval(o))
                .map(|o| o.id)
                .collect();
        let ours: Vec<u16> = xebecs
            .iter()
            .copied()
            .filter(|&x| alive(&p.m.objects[p.obj(x)]))
            .collect();
        if p.sim.ticks % 40 == 0 && !ours.is_empty() {
            let here = p.at(ours[0]);
            if let Some(&f) = foes.iter().min_by_key(|&&f| gap(p.at(f), here)) {
                let at = p.at(f);
                for &x in &ours {
                    if !p.fighting(x) {
                        p.try_order("attack", &[x], at, Some(f));
                    }
                }
            }
        }
        foes.is_empty() || ours.is_empty()
    });
    // The hulk crosses and sets the troops down on the island's western
    // end, from the water by the shore a 2×2 hull reaches (0x4eab50); under
    // the towers' fire it answers them, and the unloading is given again,
    // as a player clicks once more.
    p.order("unload", &[hulk], [52, 69], None);
    assert!(
        p.until(200, |p| {
            let h = p.obj(hulk);
            if p.sim.ticks % 20 == 0
                && alive(&p.m.objects[h])
                && !matches!(p.sim.gameplay.actors[h].intent, Some(Intent::Unload(_)))
                && !p.sim.objects[h].contents.is_empty()
            {
                p.try_order("unload", &[hulk], [52, 69], None);
            }
            troops
                .iter()
                .all(|&u| p.sim.objects[p.obj(u)].holder.is_none())
        }),
        "{:?}",
        p.at(hulk)
    );
    // On the island: each one still living reaches the red camp by land
    // (those the towers' shots killed aboard took their share of the hits
    // on the hulk, 0x504900).
    let camp =
        p.m.objects
            .iter()
            .find(|o| o.owner == 2 && alive(o) && o.building())
            .unwrap();
    let r = movement::footprint_rect(&p.lib, camp);
    let cells = &p.sim.gameplay.pathing.cells;
    let landed: Vec<u16> = troops
        .iter()
        .copied()
        .filter(|&u| alive(&p.m.objects[p.obj(u)]))
        .collect();
    assert!(
        !landed.is_empty(),
        "{:?} {:?} {:?}",
        p.at(hulk),
        p.sim.gameplay.actors[p.obj(hulk)].intent,
        troops
            .iter()
            .map(|&u| {
                let i = p.obj(u);
                (p.at(u), p.sim.objects[i].holder, p.m.objects[i].health)
            })
            .collect::<Vec<_>>()
    );
    for &u in &landed {
        let at = p.at(u);
        let mut seen = std::collections::HashSet::from([[at[0] as i32, at[1] as i32]]);
        let mut queue = std::collections::VecDeque::from([[at[0] as i32, at[1] as i32]]);
        let mut reached = false;
        while let Some(c) = queue.pop_front() {
            if (r[0] - 1..=r[2] + 1).contains(&c[0]) && (r[1] - 1..=r[3] + 1).contains(&c[1]) {
                reached = true;
                break;
            }
            for d in [
                [1, 0],
                [-1, 0],
                [0, 1],
                [0, -1],
                [1, 1],
                [1, -1],
                [-1, 1],
                [-1, -1],
            ] {
                let n = [c[0] + d[0], c[1] + d[1]];
                if cells.land(n) && seen.insert(n) {
                    queue.push_back(n);
                }
            }
        }
        assert!(reached, "#{u} {at:?}, the hulk at {:?}", p.at(hulk));
    }
    assert!(!p.fired("Player win"));
    // The fight for the camp is not played: whatever the red kingdom has
    // left falls.
    let left: Vec<usize> =
        p.m.objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.owner == 2 && alive(o))
            .map(|(i, _)| i)
            .collect();
    for i in left {
        kill(&mut p.sim, &mut p.m, &p.g, &p.lib, i);
    }
    assert!(
        p.until(30, |p| p.sim.gameplay.script.next_map.is_some()),
        "{}",
        p.trace_tail()
    );
    assert!(p.fired("Player win"));
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\Perfect 5.wmp")
    );
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_fifth_lesson_is_won_by_catapults_against_the_towers() {
    let mut p = tutorial("perfect 5");
    assert!(p.until(30, |p| p.fired("Start") && !p.sim.gameplay.script.modal));
    let (castle, mill, stone, boat, cow) = (58u16, 59u16, 62u16, 69u16, 73u16);
    let (c, ma) = (p.at(castle), p.at(mill));
    fish(&mut p, boat);
    let plan = [
        ("e_blacksmith", "blacksmith", [c[0] - 8, c[1] - 6]),
        ("e_tavern", "tavern", [c[0] + 6, c[1] - 8]),
        ("e_stable", "stable", [c[0] - 10, c[1] + 4]),
        ("e_workshop", "workshop", [c[0] + 8, c[1] + 6]),
        ("e_outpost", "outpost", [c[0] - 6, c[1] - 14]),
    ];
    let mut done: Vec<u16> = Vec::new();
    let mut catapults = 0;
    let mut t = 0;
    assert!(
        p.until(2400, |p| {
            t += 1;
            let peons = peasants(p, 1);
            // The workshop needs the blacksmith finished.
            if done.len() < plan.len() && t % 3 == 0 {
                let (key, name, near) = plan[done.len()];
                let smith = done
                    .first()
                    .is_some_and(|&b| p.m.objects[p.obj(b)].flags & FLAG_CONSTRUCTION == 0);
                let b = p.obj(peons[0]);
                if key != "e_workshop" || smith {
                    'o: for r in 2..20i32 {
                        for dy in -r..=r {
                            for dx in -r..=r {
                                let cell = [
                                    (near[0] as i32 + dx).max(0) as u16,
                                    (near[1] as i32 + dy).max(0) as u16,
                                ];
                                let req = request(key, vec![b], cell, None);
                                match issue(&mut p.sim, &mut p.m, &p.g, &p.lib, &p.c, &req) {
                                    Ok(_) => {
                                        let s =
                                            p.m.objects
                                                .iter()
                                                .rev()
                                                .find(|o| o.class_name() == name && o.owner == 1)
                                                .map(|o| o.id)
                                                .unwrap();
                                        let at = p.at(s);
                                        p.try_order("context", &peons[0..2], at, Some(s));
                                        done.push(s);
                                        break 'o;
                                    }
                                    Err(e) if !e.contains("edificabile") => break 'o,
                                    Err(_) => {}
                                }
                            }
                        }
                    }
                }
            }
            if let Some(&w) = done.get(3)
                && catapults < 5
                && t % 3 == 0
                && p.m.objects[p.obj(w)].flags & FLAG_CONSTRUCTION == 0
                && p.sim.gameplay.actors[p.obj(w)].jobs.is_empty()
            {
                let at = p.at(w);
                if p.try_order("catapult", &[w], at, None) {
                    catapults += 1;
                }
            }
            if t % 10 == 0 {
                back_to_work(p, |p, k, id| match k % 4 {
                    0 | 1 => chop(p, k, id, ma),
                    2 => work_on(p, id, stone),
                    _ => work_on(p, id, cow),
                });
                if p.sim.gameplay.actors[p.obj(boat)].intent.is_none() {
                    fish(p, boat);
                }
                if peons.len() < 10
                    && p.m.kingdoms[0].resources[2] >= 250
                    && p.sim.gameplay.actors[p.obj(castle)].jobs.is_empty()
                {
                    p.try_order("e_peon", &[castle], c, None);
                }
            }
            p.fired("Ready for attack") && catapults == 5 && !p.sim.gameplay.script.modal
        }),
        "{} {:?}",
        p.trace_tail(),
        p.m.kingdoms[0].resources
    );
    // Five catapults (three are asked) batter the towers one by one, the
    // army round them.
    let catapults: Vec<u16> =
        p.m.objects
            .iter()
            .filter(|o| o.owner == 1 && alive(o) && o.class == 19)
            .map(|o| o.id)
            .collect();
    let army = p.army(1, &catapults);
    assert!(
        p.until(1200, |p| {
            let towers: Vec<u16> =
                p.m.objects
                    .iter()
                    .filter(|o| o.owner == 2 && alive(o) && o.class_name().starts_with("tower"))
                    .map(|o| o.id)
                    .collect();
            let cs: Vec<u16> = catapults
                .iter()
                .copied()
                .filter(|&x| alive(&p.m.objects[p.obj(x)]))
                .collect();
            if p.sim.ticks % 60 == 0 && !towers.is_empty() && !cs.is_empty() {
                let here = p.at(cs[0]);
                let tower = *towers.iter().min_by_key(|&&w| gap(p.at(w), here)).unwrap();
                let j = p.obj(tower);
                for &x in &cs {
                    let i = p.obj(x);
                    if !matches!(p.sim.gameplay.actors[i].intent, Some(Intent::Attack(k)) if k == j)
                    {
                        p.close_in(&[x], tower);
                    }
                }
                let escort: Vec<u16> = army
                    .iter()
                    .copied()
                    .filter(|&u| alive(&p.m.objects[p.obj(u)]))
                    .collect();
                p.guard(&escort, cs[0], 8, &[2]);
            }
            p.sim.gameplay.script.next_map.is_some() || cs.is_empty()
        }),
        "{}",
        p.trace_tail()
    );
    assert!(p.fired("Player win"), "{}", p.trace_tail());
    assert_eq!(
        p.sim.gameplay.script.next_map.as_deref(),
        Some("MAPS\\tutmap6.wmp")
    );
    let next = p.next();
    assert!(!next.m.script.rules.is_empty());
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_last_lesson_is_won_by_six_stone_towers_and_leaving_the_field() {
    let mut p = tutorial("tutmap6");
    // Asked whether to fight, the player leaves.
    p.answers = vec![("Kill the enemy", 2)];
    assert!(p.until(30, |p| p.fired("Mission start")
        && !p.sim.gameplay.script.modal));
    let castle = 7u16;
    let c = p.at(castle);
    let stone = 10u16;
    // Architecture at the castle, three more wooden towers, and each tower
    // raised twice: wood (600 wood), then stone.
    let mut architecture = false;
    let mut towers: Vec<u16> = vec![1, 3, 2];
    let mut t = 0;
    assert!(
        p.until(3600, |p| {
            t += 1;
            let r = p.m.kingdoms[0].resources;
            let peons = peasants(p, 1);
            if !architecture && t % 5 == 0 {
                architecture = p.try_order("te_architecture", &[castle], c, None);
            }
            if architecture
                && t % 5 == 0
                && peons.len() < 12
                && r[2] >= 50
                && p.sim.gameplay.actors[p.obj(castle)].jobs.is_empty()
            {
                p.try_order("e_peon", &[castle], c, None);
            }
            if towers.len() < 6 && r[3] >= 300 && t % 5 == 0 {
                let k = towers.len() as u16 - 3;
                let near = [c[0] - 6 + 4 * k, c[1] - 8];
                let s = lay(p, "e_tower_wood", peons[0], near, class::TOWER_WOOD);
                let at = p.at(s);
                p.try_order("context", &peons[0..2], at, Some(s));
                towers.push(s);
            }
            if t % 5 == 0 {
                for &tower in &towers {
                    let i = p.obj(tower);
                    if p.m.objects[i].flags & FLAG_CONSTRUCTION != 0
                        || !p.sim.gameplay.actors[i].jobs.is_empty()
                    {
                        continue;
                    }
                    let key = match p.m.objects[i].class_name() {
                        "tower_wood" => "to_wtower2",
                        "tower_wood2" if architecture => "to_stower",
                        _ => continue,
                    };
                    let at = p.at(tower);
                    p.try_order(key, &[tower], at, None);
                }
            }
            if t % 10 == 0 {
                back_to_work(p, |p, k, id| {
                    if k % 3 == 2 {
                        work_on(p, id, stone);
                    } else {
                        chop(p, k * 5, id, c);
                    }
                });
            }
            p.sim.gameplay.script.over.is_some()
        }),
        "{} {:?}",
        p.trace_tail(),
        p.m.kingdoms[0].resources
    );
    for rule in ["Towers built", "Quit"] {
        assert!(p.fired(rule), "{rule}");
    }
    assert!(!p.fired("Add fighting quest"));
    assert_eq!(p.var("$userquit"), 1);
    assert_eq!(p.sim.gameplay.script.over.map(|o| o.0), Some(true));
    assert!(p.sim.gameplay.script.next_map.is_none());
}

// The campaigns of the other editions, «The Burden of the Crown»
// (`TZAR.WCM` of the US CD), «La Reconquista» and «Los Dominios de la
// Magia» (FX's Spanish edition), are not played through: their maps are opened and their
// openings run, every rule of every script is made true on a copy and run
// to its last action, every conversation is walked down each of its
// choices, every map is reached through the `LoadMap` of the one before
// with its heroes, and every text the player reads is found in Italian.

/// The maps of the American campaign.
const AMERICAN: [&str; 19] = [
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
    "ultimate",
];

/// The maps of the Spanish edition's campaigns, in the order of their
/// story: «La Reconquista» tells it on a map of Spain between its battles.
const RECONQUISTA: [&str; 18] = [
    "espaÑa10inicio",
    "espaÑa10mision1",
    "covadonga",
    "espaÑa10mision2",
    "barcelona",
    "espaÑa10mision3",
    "santiago",
    "espaÑa10mision4",
    "valencia",
    "espaÑa10mision5",
    "zaragoza",
    "espaÑa10mision6",
    "navas de tolosa",
    "espaÑa10mision7",
    "salado",
    "espaÑa10mision8",
    "conquista granada",
    "espaÑa10final",
];
const DOMINIOS: [&str; 8] = [
    "huida en la noche",
    "desde las filas de los proscritos",
    "reino de terror",
    "la traición",
    "acorralada",
    "una alianza inesperada",
    "el reloj de arena",
    "el sacrificio",
];
/// The campaigns of the other editions: their file and their maps, the
/// first to the last.
const OTHERS: [(&str, &[&str]); 3] = [
    ("tzar.wcm", &AMERICAN),
    ("la reconquista.wcm", &RECONQUISTA),
    ("los dominios de la magia.wcm", &DOMINIOS),
];

/// A map of another edition's campaign, when it is there.
fn other_edition(file: &str, map: &str) -> Option<Play> {
    let catalog = crate::scenario::ScenarioCatalog::load(root()).unwrap();
    let (campaign, mission) = catalog.locate(&format!("{file}::maps/{map}"))?;
    Some(Play::open_at(&catalog, campaign?, mission))
}
/// Every map of the other editions' campaigns, with its file.
fn other_maps() -> impl Iterator<Item = (&'static str, &'static str)> {
    OTHERS
        .iter()
        .flat_map(|&(file, maps)| maps.iter().map(move |&map| (file, map)))
}

impl Play {
    /// Plays until `done`, at most `seconds`, a tick at a time, the windows
    /// answered as `second` does (with `reply`) or left open: what the
    /// script asked of the interface meanwhile.
    fn watch(&mut self, seconds: u64, reply: bool, done: impl Fn(&Play) -> bool) -> Vec<Show> {
        let mut shown = Vec::new();
        for _ in 0..seconds * 20 {
            if done(self) {
                break;
            }
            let mut answers = 0;
            while reply && self.sim.gameplay.script.modal && answers < 1000 {
                self.reply();
                answers += 1;
            }
            shown.append(&mut self.sim.gameplay.script.outbox.0);
            advance(
                &mut self.sim,
                &mut self.m,
                &mut self.g,
                &self.lib,
                &self.c,
                Duration::from_millis(TICK_MS),
                10,
                false,
            );
        }
        shown.append(&mut self.sim.gameplay.script.outbox.0);
        shown
    }
    /// Whether the action named `name` ran.
    fn ran(&self, name: &str) -> bool {
        let wanted = format!("azione «{name}»");
        self.sim
            .gameplay
            .script
            .trace
            .0
            .as_ref()
            .is_some_and(|t| t.iter().any(|(_, l)| l.trim() == wanted))
    }
    /// A copy where rule `r` is true and due now, the computers standing.
    fn forced(&self, r: usize) -> Play {
        let mut q = self.clone();
        q.sim.gameplay.ai.players.clear();
        q.sim.gameplay.script.trace.0 = Some(Vec::new());
        q.m.script.rules[r].condition = "1".into();
        let now = q.sim.ticks * TICK_MS;
        // Its delays begin afresh: a run of it, or of another rule naming
        // them, may be waiting on them (one flag per action, as natively).
        let delays: Vec<usize> = q.m.script.rules[r]
            .actions
            .iter()
            .filter_map(|a| q.m.script.action(a))
            .filter(|&i| matches!(q.m.script.actions[i].value, crate::script::Action::Delay(_)))
            .collect();
        let st = &mut q.sim.gameplay.script;
        for i in delays {
            st.waiting.remove(&i);
        }
        st.rules.retain(|run| run.rule != r);
        st.sequence += 1;
        st.rules.push(super::scripting::RuleRun {
            rule: r,
            due: now,
            sequence: st.sequence,
            cursor: None,
            keep: false,
        });
        q
    }
    /// The actions rule `r` names, as the runtime resolves them: an unknown
    /// name ends the list.
    fn actions_of(&self, r: usize) -> Vec<String> {
        let s = &self.m.script;
        s.rules[r]
            .actions
            .iter()
            .map_while(|a| s.action(a).map(|i| s.actions[i].name.clone()))
            .collect()
    }
    /// A copy with a rule of its own, true now, running `actions`.
    fn with_rule(&self, actions: Vec<String>) -> Play {
        let mut q = self.clone();
        q.m.script.rules.push(crate::script::Rule {
            name: "prova".into(),
            condition: "1".into(),
            actions,
        });
        let r = q.m.script.rules.len() - 1;
        q.forced(r)
    }
}

#[test]
#[ignore = "Requires the installed original archives, the American CD and the Spanish edition"]
fn every_rule_of_the_other_editions_campaigns_runs_to_its_last_action() {
    let mut problems = Vec::new();
    let mut rules = 0;
    for (file, map) in other_maps() {
        let Some(mut p) = other_edition(file, map) else {
            continue;
        };
        // The opening as it plays, the player standing.
        p.watch(20, true, |_| false);
        for r in 0..p.m.script.rules.len() {
            rules += 1;
            let wanted = p.actions_of(r);
            let mut q = p.forced(r);
            q.watch(300, true, |q| wanted.iter().all(|a| q.ran(a)));
            let over = q.sim.gameplay.script.over.is_some();
            let missing: Vec<&String> = wanted.iter().filter(|a| !q.ran(a)).collect();
            if !missing.is_empty() && !over {
                problems.push(format!(
                    "{map}, regola «{}»: non eseguite {missing:?}",
                    p.m.script.rules[r].name
                ));
            }
        }
    }
    assert!(rules == 0 || rules > 100, "{rules}");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Every phrase said down each choice of the conversation under way.
fn walk(mut q: Play, said: &mut BTreeSet<usize>, depth: usize) {
    for _ in 0..500 {
        let Some(talk) = q.sim.gameplay.script.conversation.clone() else {
            return;
        };
        if let Some(phrase) = talk.phrase {
            said.insert(phrase);
        }
        if talk.phrase.is_none() && talk.choices.len() > 1 && depth < 8 {
            for k in 0..talk.choices.len() {
                let mut b = q.clone();
                answer(&mut b.sim, &mut b.m, &b.g, &b.lib, &b.c, k);
                walk(b, said, depth + 1);
            }
            return;
        }
        answer(&mut q.sim, &mut q.m, &q.g, &q.lib, &q.c, 0);
    }
    panic!("una conversazione che non finisce");
}

#[test]
#[ignore = "Requires the installed original archives, the American CD and the Spanish edition"]
fn every_conversation_of_the_other_editions_campaigns_is_walked_down_each_choice() {
    use crate::script::{Action, Named};
    let mut talks = 0;
    let mut unsaid = Vec::new();
    for (file, map) in other_maps() {
        let Some(mut p) = other_edition(file, map) else {
            continue;
        };
        p.watch(2, true, |_| false);
        // A conversation no action starts is never said, in the original
        // too (Valencia's «Reconstruccion»).
        let started: BTreeSet<String> =
            p.m.script
                .actions
                .iter()
                .filter_map(|a| match &a.value {
                    Action::Conversation(name) => Some(name.to_ascii_lowercase()),
                    _ => None,
                })
                .collect();
        for (k, c) in p.m.conversations.clone().iter().enumerate() {
            if !started.contains(&c.name.to_ascii_lowercase()) {
                continue;
            }
            talks += 1;
            // The names its lines test: walked once as they are, and once
            // with each set to 1 (a duel won, a deed done).
            let tested: BTreeSet<String> = c
                .phrases
                .iter()
                .flat_map(|ph| crate::script::audit::symbols(&ph.condition))
                .filter(|s| !s.chars().all(|ch| ch.is_ascii_digit()))
                .collect();
            let mut said = BTreeSet::new();
            for set in std::iter::once(None).chain(tested.iter().map(Some)) {
                // An action of its own starts it on a copy.
                let mut q = p.clone();
                if let Some(name) = set {
                    q.sim.gameplay.script.variables.set(name, 1);
                }
                q.m.script.actions.push(Named {
                    name: "prova conversazione".into(),
                    value: Action::Conversation(c.name.clone()),
                });
                let mut q = q.with_rule(vec!["prova conversazione".into()]);
                q.watch(5, false, |q| q.sim.gameplay.script.conversation.is_some());
                let talk = q.sim.gameplay.script.conversation.clone();
                assert_eq!(
                    talk.as_ref().map(|t| t.conversation),
                    Some(k),
                    "{map}: «{}» non si apre",
                    c.name
                );
                walk(q, &mut said, 0);
            }
            // A line no other leads to and that does not open the talk is
            // never said, in the original too.
            let led: BTreeSet<u16> = c
                .phrases
                .iter()
                .flat_map(|ph| ph.next.iter().copied().filter(move |&n| n != ph.index))
                .collect();
            // A line that tests an event of the map (Santiago's friar at
            // the captain's side) waits on the world, which a copy does not
            // stage: it and what only it leads to are left out.
            let events: BTreeSet<String> =
                p.m.script
                    .events
                    .iter()
                    .map(|e| e.name.to_ascii_lowercase())
                    .collect();
            let on_world = |ph: &crate::dialogue::Phrase| {
                crate::script::audit::symbols(&ph.condition)
                    .into_iter()
                    .any(|s| events.contains(&s.to_ascii_lowercase()))
            };
            let mut reach: BTreeSet<u16> = c
                .phrases
                .iter()
                .filter(|ph| ph.start && !on_world(ph))
                .map(|ph| ph.index)
                .collect();
            let mut queue: Vec<u16> = reach.iter().copied().collect();
            while let Some(n) = queue.pop() {
                let Some(ph) = c.phrases.iter().find(|ph| ph.index == n) else {
                    continue;
                };
                for &m in &ph.next {
                    if c.phrases.iter().any(|x| x.index == m && !on_world(x)) && reach.insert(m) {
                        queue.push(m);
                    }
                }
            }
            let never: Vec<u16> = (0..c.phrases.len())
                .filter(|i| !said.contains(i))
                .map(|i| c.phrases[i].index)
                .filter(|n| {
                    led.contains(n) || c.phrases.iter().any(|ph| ph.index == *n && ph.start)
                })
                .filter(|n| reach.contains(n))
                .collect();
            if !never.is_empty() {
                unsaid.push(format!("{map}, «{}»: frasi {never:?}", c.name));
            }
        }
    }
    assert!(talks == 0 || talks > 20, "{talks}");
    assert!(unsaid.is_empty(), "{}", unsaid.join("\n"));
}

#[test]
#[ignore = "Requires the installed original archives, the American CD and the Spanish edition"]
fn the_other_editions_campaigns_lead_from_their_first_map_to_the_last() {
    for (file, maps) in OTHERS {
        if let Some(first) = other_edition(file, maps[0]) {
            leads(file, maps, first);
        }
    }
}

/// Every map of the campaign `file` is reached from the first, and only
/// the last ends it; the American one with the movie of the end.
fn leads(file: &str, maps: &[&str], first: Play) {
    use crate::script::Action;
    let mut seen = BTreeSet::from([maps[0].to_owned()]);
    let mut queue = vec![(maps[0].to_owned(), first)];
    let mut ends = Vec::new();
    let mut problems = Vec::new();
    while let Some((map, mut p)) = queue.pop() {
        p.watch(5, true, |_| false);
        let targets: BTreeSet<(String, String)> =
            p.m.script
                .actions
                .iter()
                .filter_map(|a| match &a.value {
                    Action::LoadMap(path) => Some((path.clone(), a.name.clone())),
                    _ => None,
                })
                .collect();
        if targets.is_empty() {
            ends.push(map.clone());
            if file != "tzar.wcm" {
                continue;
            }
            // The last map: its victory plays the movie of the end.
            let mut movies = Vec::new();
            for r in 0..p.m.script.rules.len() {
                let wanted = p.actions_of(r);
                let mut q = p.forced(r);
                for s in q.watch(300, true, |q| wanted.iter().all(|a| q.ran(a))) {
                    if let Show::Movie(name) = s {
                        movies.push(name);
                    }
                }
            }
            if !movies
                .iter()
                .any(|m| m.to_ascii_lowercase().contains("outro"))
            {
                problems.push(format!("{map}: nessun filmato finale ({movies:?})"));
            }
            continue;
        }
        for (path, action) in targets {
            // The map is won toward `path`, as by a rule that names it.
            let mut q = p.with_rule(vec![action]);
            q.watch(30, true, |q| q.sim.gameplay.script.next_map.is_some());
            if q.sim.gameplay.script.next_map.as_deref() != Some(path.as_str()) {
                problems.push(format!("{map}: LoadMap «{path}» non sceglie la mappa"));
                continue;
            }
            let carried = heroes(&q.sim, &q.m);
            let next = q.next();
            let name = path
                .rsplit(['/', '\\'])
                .next()
                .unwrap()
                .to_ascii_lowercase()
                .trim_end_matches(".wmp")
                .to_owned();
            // A hero the next map carries comes with what it had (a thing on
            // the ground may bear the name too).
            for h in &carried {
                if let Some(o) = next
                    .m
                    .objects
                    .iter()
                    .find(|o| o.name == h.name && o.flags & crate::world::FLAG_CARRIED != 0)
                    && o.health == 0
                {
                    problems.push(format!("{name}: eroe «{}» senza salute", h.name));
                }
            }
            if seen.insert(name.clone()) {
                queue.push((name, next));
            }
        }
    }
    let all: BTreeSet<String> = maps.iter().map(|m| m.to_string()).collect();
    assert_eq!(seen, all, "{file}: mappe raggiunte");
    assert_eq!(ends, maps[maps.len() - 1..], "{file}");
    assert!(problems.is_empty(), "{file}: {}", problems.join("\n"));
}

/// Every text a mission shows, as `lang::localize_mission` swaps them.
fn shown_texts(world: &MissionWorld) -> Vec<String> {
    use crate::script::Action;
    let mut shown: Vec<String> = Vec::new();
    for o in &world.objects {
        shown.push(o.name.clone());
        shown.push(o.subtitle.clone());
    }
    for k in &world.kingdoms {
        shown.push(k.name.clone());
        shown.push(k.leader.clone());
    }
    for a in &world.script.actions {
        match &a.value {
            Action::AddQuest { text, .. } | Action::Chat(text) => shown.push(text.clone()),
            Action::Lose(_, text) | Action::Win(_, text) => shown.push(text.clone()),
            Action::Story { text, .. } => shown.push(text.clone()),
            Action::Message { text, title, .. } => {
                shown.push(text.clone());
                shown.push(title.clone());
            }
            _ => {}
        }
    }
    shown.extend(world.script.defeat.iter().flatten().cloned());
    for c in &world.conversations {
        shown.extend(c.phrases.iter().map(|p| p.text.clone()));
    }
    shown
}

#[test]
#[ignore = "Requires the installed original archives (and the American CD in USA)"]
fn every_campaign_mission_is_told_written_in_its_language() {
    use crate::lang::Language;
    let catalog = crate::scenario::ScenarioCatalog::load(root()).unwrap();
    let mut wrong = Vec::new();
    for scenario in catalog.campaigns.iter().flat_map(|c| c.missions.iter()) {
        let world =
            MissionWorld::read(&scenario.container().unwrap(), &scenario.grid().unwrap()).unwrap();
        // FX's campaigns and tutorial speak Italian, the American one English,
        // the Spanish edition's own Spanish.
        let expected = if crate::scenario::is_american(&scenario.file) {
            Language::English
        } else if crate::scenario::is_spanish(&scenario.file) {
            Language::Spanish
        } else {
            Language::Italian
        };
        if world.written_in != Some(expected) {
            wrong.push(format!("{}: {:?}", scenario.id, world.written_in));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
#[ignore = "Requires the installed original archives (and the American CD in USA)"]
fn every_text_of_every_mission_reads_in_every_language() {
    use crate::lang::Language;
    let catalog = crate::scenario::ScenarioCatalog::load(root()).unwrap();
    let scenarios = catalog
        .campaigns
        .iter()
        .flat_map(|c| c.missions.iter())
        .chain(catalog.missions.iter());
    let mut missing = BTreeSet::new();
    let mut texts = 0;
    for scenario in scenarios {
        let world =
            MissionWorld::read_original(&scenario.container().unwrap(), &scenario.grid().unwrap())
                .unwrap();
        for text in shown_texts(&world) {
            if !text.chars().any(char::is_alphabetic) {
                continue;
            }
            texts += 1;
            // What Italian and English share (Temujin, Tzar 3) is a name for
            // all.
            if crate::lang::translate(&text, Language::English).is_none()
                && crate::lang::translate(&text, Language::Italian).is_none()
            {
                continue;
            }
            for language in Language::TRANSLATED {
                if crate::lang::translate(&text, language).is_none()
                    && !crate::lang::written_in(&text, language)
                {
                    missing.insert(format!("{} {}: «{text}»", language.code(), scenario.id));
                }
            }
        }
    }
    assert!(texts > 2000, "{texts}");
    assert!(
        missing.is_empty(),
        "{} testi senza traduzione:\n{}",
        missing.len(),
        missing.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
#[ignore = "Requires the installed original archives, the American CD and the Spanish edition"]
fn every_text_of_the_other_editions_campaigns_reads_in_italian() {
    let catalog = crate::scenario::ScenarioCatalog::load(root()).unwrap();
    let mut missing = BTreeSet::new();
    let mut texts = 0;
    for (file, map) in other_maps() {
        let Some((c, m)) = catalog.locate(&format!("{file}::maps/{map}")) else {
            continue;
        };
        let scenario = &catalog.campaigns[c.unwrap()].missions[m];
        let world =
            MissionWorld::read_original(&scenario.container().unwrap(), &scenario.grid().unwrap())
                .unwrap();
        let shown = shown_texts(&world);
        for text in shown {
            // Only what has words in it is read.
            if !text.chars().any(char::is_alphabetic) {
                continue;
            }
            texts += 1;
            // A name (Sartor, Wark Aven, Tzar 3, «Sartor!») reads the same.
            let name = text.split_whitespace().count() <= 3
                && text.chars().next().is_some_and(char::is_uppercase)
                && !text.contains(['.', ',', '?', ':', ';']);
            // The Spanish catalog has each text, which reads in Italian
            // (`lang::missions`), names too.
            let read = match file {
                "tzar.wcm" => {
                    name || crate::lang::translate(&text, crate::lang::Language::Italian).is_some()
                }
                _ => crate::lang::written_in(&text, crate::lang::Language::Spanish),
            };
            if !read {
                missing.insert(format!("{map}: «{text}»"));
            }
        }
    }
    assert!(texts == 0 || texts > 500, "{texts}");
    assert!(
        missing.is_empty(),
        "{} testi senza italiano:\n{}",
        missing.len(),
        missing.into_iter().collect::<Vec<_>>().join("\n")
    );
}
