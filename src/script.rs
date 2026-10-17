//! Mission scripts: the `.events`, `.actions` and `.rules` entries of a
//! scenario, and its `.defeat.messages`.
//!
//! The three files are sequences of records: a 32-byte name (NUL-terminated,
//! the rest is old editor memory), a 16-bit kind (rules: the number of
//! action names), a 16-bit data length and the data (reader 0x4782d0). The
//! header words 0x0c–0x12 give the number of rules, actions and events and
//! the largest data length; the loader (0x4b70b0) reads that many records.
//!
//! Events (kinds 1000–1007) are named states that conditions test; actions
//! (kinds 0–27) are what rules run; rules hold a condition (an expression,
//! see [`expr`]) and the names of their actions. The runtime is in
//! `gameplay::scripting`.
use crate::{Result, assets::legacy_text, invalid};
use serde::{Deserialize, Serialize};

pub mod audit;
pub mod expr;

/// Objects a selector designates (parse 0x4b9270, evaluation 0x4b9670).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Selector {
    /// By class and kingdoms rather than by a list of ids.
    pub by_class: bool,
    /// Class mode: the count must be at most N instead of at least N.
    pub at_most: bool,
    /// A class index or one of the [`any`] codes.
    pub class: u16,
    /// Bit `p - 1` selects kingdom `p`.
    pub kingdoms: u16,
    /// List mode: all the listed objects must match, not any one.
    pub all: bool,
    pub ids: Vec<u16>,
    /// Class mode: N, an expression.
    pub count: String,
}
/// Class codes of a selector that stand for groups.
pub mod any {
    pub const OBJECT: u16 = 0xffff;
    pub const UNIT: u16 = 0xfffe;
    /// Classes 3–38.
    pub const MILITARY: u16 = 0xfffd;
    pub const BUILDING: u16 = 0xfffc;
    /// Every class except those of 0x442bd0.
    pub const MAJOR: u16 = 0xfffb;
}
/// A rectangle of cells, or the cells around an object (parse 0x4b94b0).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Area {
    pub around: bool,
    /// Inclusive cell rectangle: x1, y1, x2, y2.
    pub rect: [i16; 4],
    pub object: u16,
    /// Distance in cells around the object, an expression.
    pub radius: String,
}
/// Where objects must be held (parse 0x4b9420).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Holder {
    pub by_class: bool,
    pub class: u16,
    pub object: u16,
    pub kingdoms: u16,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Event {
    /// 1000: the selector matches existing objects.
    Exists(Selector),
    /// 1001: the listed objects no longer exist, or as many of the class
    /// were lost.
    Destroyed(Selector),
    /// 1002: the objects are inside the holder.
    Entered(Selector, Holder),
    /// 1003: never true (0x4b9d80).
    Left,
    /// 1004: the objects stand in the area.
    InArea(Selector, Area),
    /// 1005: one of the kingdoms is defeated.
    Defeated(u16),
    /// 1006: one of the kingdoms has won.
    Won(u16),
    /// 1007: the objects satisfy the expression, each in turn.
    Expression(Selector, String),
}
/// Orders of `ObjOrder` (0x4bb090).
pub mod order {
    pub const DEFAULT: u8 = 0;
    pub const MOVE: u8 = 1;
    pub const ATTACK: u8 = 2;
    pub const CAPTURE: u8 = 3;
    pub const UNLOAD: u8 = 5;
    pub const PICK: u8 = 6;
    pub const PATROL: u8 = 7;
    pub const ADVANCE: u8 = 8;
    pub const GUARD: u8 = 9;
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// 0: add an objective; its id is the hash of the action's name.
    AddQuest { text: String, portrait: String },
    /// 1: remove the objective added by the named action.
    RemoveQuest(String),
    /// 2: kingdom `from` changes its relation to `to`.
    Diplomacy { from: u8, to: u8, flags: u32 },
    /// 3: give the matched objects to a kingdom.
    Owner(Selector, u8),
    /// 4: evaluate the expression on each matched object.
    SetProperty(Selector, String),
    /// 5: evaluate the statements.
    Expression(String),
    /// 6: start the timer named like the action.
    SetTimer { init: String, countdown: bool },
    /// 7: stop the timer started by the named action.
    StopTimer(String),
    /// 8: the kingdoms lose, all others win.
    Lose(u16, String),
    /// 9: the kingdoms win, all others lose.
    Win(u16, String),
    /// 10: the rule stays and is tested again.
    KeepRule,
    /// 11: the mission is won and the next one is this map.
    LoadMap(String),
    /// 12: hold the view on the area.
    LockView(String, Area),
    /// 13: a line of text over the map.
    Chat(String),
    /// 14: the story screen.
    Story {
        text: String,
        image: String,
        sound: String,
        music: String,
        time: String,
        close: bool,
    },
    /// 15: a message window.
    Message {
        text: String,
        image: String,
        title: String,
    },
    /// 16: create objects in the area.
    Spawn {
        class: u16,
        count: String,
        owner: u8,
        area: Area,
        civilization: u8,
    },
    /// 17: release the view.
    UnlockView,
    /// 18: order the matched objects toward the area.
    Order(Selector, Area, u8),
    /// 19: move the view from one area to another.
    ScrollView(Area, Area),
    /// 20: start the named conversation.
    Conversation(String),
    /// 21: kill the matched objects.
    Destroy(Selector),
    /// 22: reveal the area to the local kingdom for a time.
    Explore(Area, String),
    /// 23: bring hidden objects into the game.
    Appear(Selector),
    /// 24: whether a kingdom can still win or lose by the rules.
    Status {
        kingdom: u8,
        cannot_win: bool,
        cannot_lose: bool,
    },
    /// 25: lock (false) or unlock (true) the matched objects.
    Lock(Selector, bool),
    /// 26: remove the matched objects without a death.
    Erase(Selector),
    /// 27: wait the given milliseconds.
    Delay(String),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Named<T> {
    pub name: String,
    pub value: T,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub condition: String,
    /// Action names, resolved without regard to case when the map loads.
    pub actions: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Script {
    pub events: Vec<Named<Event>>,
    pub actions: Vec<Named<Action>>,
    pub rules: Vec<Rule>,
    /// Custom text shown when each kingdom is defeated.
    pub defeat: [Option<String>; 8],
}
/// The text the message window shows: a story, a message or a phrase.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Empty for the story: the window shows the mission's title.
    pub title: String,
    pub text: String,
    /// Archive path of the picture beside the text.
    pub portrait: String,
    /// Phrases to choose from.
    pub choices: Vec<String>,
}
/// An objective as the quest window shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quest {
    /// Hash of the adding action's name (0x4bbf80).
    #[serde(default)]
    pub id: i32,
    pub text: String,
    /// Archive path of the large portrait (`USER\G….bmp`), possibly empty.
    pub portrait: String,
}
/// The id of an objective: `h = h * 5 + c` over the signed bytes of the
/// action's name.
pub fn quest_id(name: &str) -> i32 {
    crate::assets::legacy_bytes(name)
        .iter()
        .fold(0i32, |h, &c| h.wrapping_mul(5).wrapping_add(c as i8 as i32))
}

/// The record counts of `<map>.header` words 0x0c, 0x0e, 0x10, 0x12:
/// rules, actions, events, largest data length.
pub fn counts(header: &[u8]) -> [u16; 4] {
    std::array::from_fn(|i| {
        header
            .get(12 + 2 * i..14 + 2 * i)
            .map_or(0, |w| u16::from_le_bytes([w[0], w[1]]))
    })
}

fn records(data: &[u8], count: u16) -> Result<Vec<(String, u16, &[u8])>> {
    let mut out = Vec::new();
    let mut at = 0;
    for _ in 0..count {
        let head = data
            .get(at..at + 36)
            .ok_or_else(|| invalid("Record di script troncato"))?;
        let name = legacy_text(head[..32].split(|b| *b == 0).next().unwrap_or_default());
        let kind = u16::from_le_bytes([head[32], head[33]]);
        let len = u16::from_le_bytes([head[34], head[35]]) as usize;
        let body = data
            .get(at + 36..at + 36 + len)
            .ok_or_else(|| invalid("Dati di script troncati"))?;
        out.push((name.trim_end().to_owned(), kind, body));
        at += 36 + len;
    }
    Ok(out)
}

/// The fields of a record, read in order as the native loaders do.
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8]> {
        let s = self
            .data
            .get(self.at..self.at + n)
            .ok_or_else(|| invalid("Campo di script troncato"))?;
        self.at += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into()?))
    }
    /// A NUL-terminated string without leading and trailing bytes ≤ 0x20.
    fn text(&mut self) -> Result<String> {
        let rest = self.data.get(self.at..).unwrap_or_default();
        let n = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| invalid("Testo di script senza terminatore"))?;
        self.at += n + 1;
        let s = &rest[..n];
        let start = s.iter().position(|&b| b > 0x20).unwrap_or(s.len());
        let end = s.iter().rposition(|&b| b > 0x20).map_or(start, |e| e + 1);
        Ok(legacy_text(&s[start..end]))
    }
    fn selector(&mut self) -> Result<Selector> {
        let by_class = self.u8()? != 0;
        let at_most = self.u8()? != 0;
        let class = self.u16()?;
        let kingdoms = self.u16()?;
        let all = self.u8()? != 0;
        // An editor tag of the class, never read by the game.
        self.u8()?;
        let n = self.u16()?;
        let ids = (0..n).map(|_| self.u16()).collect::<Result<_>>()?;
        Ok(Selector {
            by_class,
            at_most,
            class,
            kingdoms,
            all,
            ids,
            count: self.text()?,
        })
    }
    fn area(&mut self) -> Result<Area> {
        let around = self.u8()? != 0;
        let mut rect = [0; 4];
        for r in &mut rect {
            *r = self.u32()? as i16;
        }
        Ok(Area {
            around,
            rect,
            object: self.u16()?,
            radius: self.text()?,
        })
    }
    fn holder(&mut self) -> Result<Holder> {
        Ok(Holder {
            by_class: self.u8()? != 0,
            class: self.u16()?,
            object: self.u16()?,
            kingdoms: self.u16()?,
        })
    }
}

fn event(kind: u16, data: &[u8]) -> Result<Event> {
    let mut r = Reader { data, at: 0 };
    Ok(match kind {
        1000 => Event::Exists(r.selector()?),
        1001 => Event::Destroyed(r.selector()?),
        1002 => Event::Entered(r.selector()?, r.holder()?),
        1003 => Event::Left,
        1004 => Event::InArea(r.selector()?, r.area()?),
        1005 => Event::Defeated(r.u16()?),
        1006 => Event::Won(r.u16()?),
        1007 => Event::Expression(r.selector()?, r.text()?),
        _ => return Err(invalid("Evento di script sconosciuto")),
    })
}

fn action(kind: u16, data: &[u8]) -> Result<Action> {
    let mut r = Reader { data, at: 0 };
    Ok(match kind {
        0 => Action::AddQuest {
            text: r.text()?,
            portrait: r.text()?,
        },
        1 => Action::RemoveQuest(r.text()?),
        2 => Action::Diplomacy {
            from: r.u8()?,
            to: r.u8()?,
            flags: r.u32()?,
        },
        3 => Action::Owner(r.selector()?, r.u8()?),
        4 => Action::SetProperty(r.selector()?, r.text()?),
        5 => Action::Expression(r.text()?),
        6 => Action::SetTimer {
            init: r.text()?,
            countdown: r.u8()? != 0,
        },
        7 => Action::StopTimer(r.text()?),
        8 => Action::Lose(r.u16()?, r.text()?),
        9 => Action::Win(r.u16()?, r.text()?),
        10 => Action::KeepRule,
        11 => Action::LoadMap(r.text()?),
        12 => Action::LockView(r.text()?, r.area()?),
        // Two trailing bytes are never read.
        13 => Action::Chat(r.text()?),
        14 => Action::Story {
            text: r.text()?,
            image: r.text()?,
            sound: r.text()?,
            music: r.text()?,
            time: r.text()?,
            close: r.u8().is_ok_and(|b| b != 0),
        },
        15 => Action::Message {
            text: r.text()?,
            image: r.text()?,
            title: r.text()?,
        },
        16 => Action::Spawn {
            class: r.u16()?,
            count: r.text()?,
            owner: r.u8()?,
            area: r.area()?,
            civilization: r.u8()?,
        },
        17 => Action::UnlockView,
        18 => Action::Order(r.selector()?, r.area()?, r.u8()?),
        19 => Action::ScrollView(r.area()?, r.area()?),
        20 => Action::Conversation(r.text()?),
        21 => Action::Destroy(r.selector()?),
        // The trailing byte (the editor's «remove fog») is never read.
        22 => Action::Explore(r.area()?, r.text()?),
        23 => Action::Appear(r.selector()?),
        24 => Action::Status {
            kingdom: r.u8()?,
            cannot_win: r.u8()? != 0,
            cannot_lose: r.u8()? != 0,
        },
        25 => Action::Lock(r.selector()?, r.u8()? != 0),
        26 => Action::Erase(r.selector()?),
        27 => Action::Delay(r.text()?),
        _ => return Err(invalid("Azione di script sconosciuta")),
    })
}

/// `.defeat.messages`: for kingdoms 1–8, a text and one byte; `00 01`
/// when there is none.
pub fn defeat_messages(data: &[u8]) -> [Option<String>; 8] {
    let mut out: [Option<String>; 8] = Default::default();
    let mut r = Reader { data, at: 0 };
    for slot in &mut out {
        let Ok(text) = r.text() else { break };
        if r.u8().is_err() {
            break;
        }
        *slot = Some(text).filter(|t| !t.is_empty());
    }
    out
}

impl Script {
    /// Parse the scripts with the counts of the map header.
    pub fn parse(counts: [u16; 4], events: &[u8], actions: &[u8], rules: &[u8]) -> Result<Self> {
        let [n_rules, n_actions, n_events, _] = counts;
        let events = records(events, n_events)?
            .into_iter()
            .map(|(name, kind, body)| {
                Ok(Named {
                    name,
                    value: event(kind, body)?,
                })
            })
            .collect::<Result<_>>()?;
        let actions = records(actions, n_actions)?
            .into_iter()
            .map(|(name, kind, body)| {
                Ok(Named {
                    name,
                    value: action(kind, body)?,
                })
            })
            .collect::<Result<_>>()?;
        let rules = records(rules, n_rules)?
            .into_iter()
            .map(|(name, count, body)| {
                let mut r = Reader { data: body, at: 0 };
                let condition = expr::condition(&r.text()?);
                // `count` action names follow the condition.
                let actions = (0..count)
                    .map_while(|_| r.text().ok())
                    .filter(|s| !s.is_empty())
                    .collect();
                Ok(Rule {
                    name,
                    condition,
                    actions,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            events,
            actions,
            rules,
            defeat: Default::default(),
        })
    }
    /// The index of an action, by name without regard to case (0x4b73f0).
    pub fn action(&self, name: &str) -> Option<usize> {
        self.actions
            .iter()
            .position(|a| a.name.eq_ignore_ascii_case(name))
    }
    /// The first story a rule tells, for the description of the mission.
    pub fn story(&self) -> Option<&str> {
        self.rules
            .iter()
            .flat_map(|r| &r.actions)
            .filter_map(|n| self.action(n))
            .map(|i| &self.actions[i].value)
            .find_map(|a| match a {
                Action::Story { text, .. } if !text.is_empty() => Some(text.as_str()),
                _ => None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, kind: u16, body: &[u8]) -> Vec<u8> {
        let mut out = vec![0x5a; 36];
        out[..name.len()].copy_from_slice(name.as_bytes());
        out[name.len()] = 0;
        out[32..34].copy_from_slice(&kind.to_le_bytes());
        out[34..36].copy_from_slice(&(body.len() as u16).to_le_bytes());
        out.extend(body);
        out
    }
    fn strings(s: &[&str]) -> Vec<u8> {
        s.iter().flat_map(|s| s.bytes().chain([0])).collect()
    }

    #[test]
    fn names_have_32_bytes_and_records_are_typed() {
        // Perfect 3: «Blue cannot win or lose» is longer than 16 bytes.
        let mut actions = record("Blue cannot win or lose", 24, &[1, 1, 1]);
        actions.extend(record("Quest1", 0, &strings(&["Costruisci 2 caserme", ""])));
        actions.extend(record(
            "Story",
            14,
            &[
                strings(&[
                    "  Hai dimostrato ",
                    "",
                    "USER\\narratore_004.wav",
                    "",
                    "42000",
                ]),
                vec![0],
            ]
            .concat(),
        ));
        let mut selector = vec![1, 0, 45, 0, 1, 0, 0, 0, 0, 0];
        selector.extend(strings(&["2"]));
        let events = record("have2barracks", 1000, &selector);
        let rules = record(
            "Start",
            4,
            &strings(&["1", "blue cannot win or lose", "Story", "Quest1", ""]),
        );
        let script = Script::parse([1, 3, 1, 0], &events, &actions, &rules).unwrap();
        assert_eq!(script.actions[0].name, "Blue cannot win or lose");
        assert_eq!(
            script.actions[0].value,
            Action::Status {
                kingdom: 1,
                cannot_win: true,
                cannot_lose: true
            }
        );
        assert_eq!(script.action(&script.rules[0].actions[0]), Some(0));
        assert_eq!(script.rules[0].actions.len(), 3);
        assert_eq!(script.story(), Some("Hai dimostrato"));
        let Event::Exists(s) = &script.events[0].value else {
            panic!()
        };
        assert_eq!(
            (s.by_class, s.class, s.kingdoms, s.count.as_str()),
            (true, 45, 1, "2")
        );
        // The counts of the header bound the records read.
        assert!(Script::parse([1, 4, 1, 0], &events, &actions, &rules).is_err());
    }

    #[test]
    fn defeat_messages_and_quest_ids() {
        let mut data = vec![0, 1];
        data.extend(b"Gli orchi sono caduti\0\x05");
        data.extend([0, 1].repeat(6));
        let m = defeat_messages(&data);
        assert_eq!(m[0], None);
        assert_eq!(m[1].as_deref(), Some("Gli orchi sono caduti"));
        assert_eq!(quest_id("Ab"), 65 * 5 + 98);
    }
}
