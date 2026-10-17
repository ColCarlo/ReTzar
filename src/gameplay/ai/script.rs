//! The language of the computer players' scripts (`DATA\AI\*.ais` and the
//! `*.ai` they include): the compiler of `Tzar.exe` (0x418b80, statements
//! 0x418c60, tokens 0x4196c0, conditions 0x419ea0) and the program it
//! leaves. The program keeps the native layout, a rule's conditions,
//! switches and forgotten rules being runs of one shared list, so that its
//! quirks stay: a clause given twice in a rule starts its run again but
//! keeps counting (0x419e40, 0x41a730, 0x41a850).
use crate::{
    Result,
    assets::{GameAssets, Ini, legacy_text},
    world::CLASSES,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The files the scripts read and the names of the object types.
#[derive(Clone, Debug, Default)]
pub struct Library {
    /// Every file of `DATA\AI`, by its archive name (upper case, `/`).
    files: HashMap<String, Vec<u8>>,
    /// The object types by number (0x5a972c, filled by 0x406400): the
    /// classes, then the sections of UPGRADES.INI and TECHNOLOGIES.INI in
    /// the order 0x4975ad adds them (0x42b2d0).
    pub types: Vec<String>,
    /// The number of the first research among them.
    pub technologies: usize,
}
impl Library {
    pub fn load(a: &GameAssets) -> Result<Self> {
        let mut files = HashMap::new();
        for name in a.names().iter().filter(|n| n.starts_with("DATA/AI/")) {
            files.insert(name.clone(), a.get(name)?);
        }
        let mut types: Vec<String> = CLASSES.iter().map(|&s| s.to_owned()).collect();
        let mut technologies = 0;
        for ini in ["DATA/UPGRADES.INI", "DATA/TECHNOLOGIES.INI"] {
            technologies = types.len();
            types.extend(Ini::parse(&a.get(ini)?).section_names().map(str::to_owned));
        }
        Ok(Self {
            files,
            types,
            technologies,
        })
    }
    /// A library of the given files (names as the scripts write them); the
    /// researches are the types after the classes named `te_`.
    pub fn new(files: impl IntoIterator<Item = (String, Vec<u8>)>, types: Vec<String>) -> Self {
        let technologies = (CLASSES.len()..types.len())
            .find(|&i| types[i].starts_with("te_"))
            .unwrap_or(types.len());
        Self {
            files: files
                .into_iter()
                .map(|(n, b)| (crate::archive::key(&n), b))
                .collect(),
            types,
            technologies,
        }
    }
    /// The name of an object type.
    pub fn name(&self, kind: u16) -> Option<&str> {
        self.types.get(kind as usize).map(String::as_str)
    }
    /// 0x42b250: the type of a name, its case ignored.
    pub fn kind(&self, name: &str) -> Option<u16> {
        self.types
            .iter()
            .position(|t| t.eq_ignore_ascii_case(name))
            .map(|k| k as u16)
    }
    /// 0x401a10 on the type's record (+0x2ac): 1 unit, 2 building, 3
    /// research; 0 for the upgrades and the other classes.
    pub fn category(&self, kind: u16) -> u8 {
        let k = kind as usize;
        if k < CLASSES.len() {
            if crate::world::class::is_unit(k) {
                1
            } else if (39..=73).contains(&k) || k == crate::world::class::DUMMY {
                2
            } else {
                0
            }
        } else if k >= self.technologies && k < self.types.len() {
            3
        } else {
            0
        }
    }
    /// The names of the files that can be read.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }
    /// 0x418b80 for a computer player's script: a name without a folder is
    /// read from `data\ai\`.
    pub fn compile(&self, name: &str) -> std::result::Result<Program, Error> {
        let path = if name.contains(['\\', '/']) {
            name.to_owned()
        } else {
            format!("data\\ai\\{name}")
        };
        let mut c = Compiler {
            lib: self,
            program: Program::default(),
            names: Vec::new(),
            unresolved: Vec::new(),
            groups: Vec::new(),
            depth: 0,
        };
        if !c.file(&path)? {
            // 0x418dd2: an empty file is a script without rules.
            return Ok(Program::default());
        }
        c.finish(&path)?;
        Ok(c.program)
    }
}
/// 0x41a570: the first name of `table` equal to `word` in length and, but
/// for case, in letters.
fn find(table: &[impl AsRef<str>], word: &[u8]) -> Option<usize> {
    table
        .iter()
        .position(|t| t.as_ref().as_bytes().eq_ignore_ascii_case(word))
}

/// The state bits of a rule (+0).
pub mod flag {
    pub const CRITICAL: u32 = 0x01;
    /// To evaluate again.
    pub const DIRTY: u32 = 0x02;
    pub const TRUE: u32 = 0x04;
    pub const IN_PROGRESS: u32 = 0x08;
    pub const SATISFIED: u32 = 0x10;
    pub const FORGOTTEN: u32 = 0x20;
    pub const GROUP_START: u32 = 0x40;
    pub const GROUP_END: u32 = 0x80;
}
/// The `HAVE` goals that are not object types (0x581700: 0xfffe − index).
pub mod goal {
    pub const CONVERT: u16 = 0xfffe;
    pub const GOLD: u16 = 0xfffd;
    pub const STONE: u16 = 0xfffc;
    pub const FOOD: u16 = 0xfffb;
    pub const WOOD: u16 = 0xfffa;
    pub const NEEDS_LOCKED: u16 = 0xfff9;
    pub const RESIGN: u16 = 0xfff8;
}
const GOALS: [&str; 7] = [
    "convert",
    "gold",
    "stone",
    "food",
    "wood",
    "needs_locked",
    "resign",
];
/// 0x58105c.
const RESOURCES: [&str; 4] = ["gold", "stone", "food", "wood"];
/// A count with bit 15 is a percentage of the population limit.
pub const PERCENT: u32 = 0x8000;

/// A compiled script (the 0x1a3c-byte AI object from +4 on).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Program {
    /// Rules and the starts and ends of groups, in order (+4).
    pub rules: Vec<Rule>,
    /// The conditions, by number (+0xc).
    pub conditions: Vec<Condition>,
    /// The shared list (+0x1a): condition numbers, switch numbers (bit 31
    /// to turn it off) and rule numbers.
    pub list: Vec<u32>,
    /// The switches (+0x14, names at +0x8de).
    pub triggers: Vec<Trigger>,
    /// The expressions of `EXTERN`.
    pub texts: Vec<String>,
}
impl Program {
    /// The run of `list` a clause names; what lies past the list is not
    /// there.
    pub fn entries(&self, span: Span) -> &[u32] {
        let start = (span.start as usize).min(self.list.len());
        let end = start
            .saturating_add(span.count as usize)
            .min(self.list.len());
        &self.list[start..end]
    }
}
/// A rule or a group mark (0x24 bytes).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Rule {
    /// Empty when unnamed, and for group marks.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// `flag::*`.
    pub flags: u32,
    /// `WHEN` (+4, +8): condition numbers in `list`.
    #[serde(skip_serializing_if = "Span::is_empty")]
    pub when: Span,
    /// `HAVE` (+0xc, +0xe).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub have: Option<Have>,
    /// `TRIGGER` (+0x10, +0x14): switch numbers in `list`.
    #[serde(skip_serializing_if = "Span::is_empty")]
    pub triggers: Span,
    /// `FORGET` (+0x18, +0x1c): rule numbers in `list`.
    #[serde(skip_serializing_if = "Span::is_empty")]
    pub forget: Span,
    /// `SAY` (+0x20).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
    /// The start of a group: the number of its end (+0x18).
    #[serde(skip_serializing_if = "is_zero")]
    pub end: u32,
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}
/// A run of the shared list: start and count.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub count: u32,
}
impl Span {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}
/// The goal of a rule.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Have {
    /// An object type, or `goal::*`.
    pub kind: u16,
    /// As written: with `%` bit 15 is set (`PERCENT`).
    pub count: u16,
}
/// A switch of the script.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Trigger {
    pub name: String,
    pub on: bool,
}
/// A condition (10 bytes: kind, a, b); counts as written, with `PERCENT`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Condition {
    /// 1: the rule is satisfied.
    Rule(u32),
    /// 2: the switch is on.
    Trigger(u32),
    /// 3: 0 gold, 1 stone, 2 food, 3 wood.
    Resource { resource: u8, amount: u32 },
    /// 4: objects of a type; type 0 is `UNITS`.
    Count { kind: u16, count: u32 },
    /// 5: game time in ms (a `%` stays in bit 15).
    Time(u32),
    /// 6: the `%` dropped (0x41a3a8).
    Random(u32),
    /// 7: an expression of `texts`.
    Extern(u32),
    /// 8: another condition is false.
    Not(u32),
    /// 10: 1 European, 2 Arabian, 3 Asian.
    Civ(u8),
    /// 11: at least 0 easy, 1 normal, 2 hard.
    Difficulty(u8),
    /// 12: summed over the enemy kingdoms.
    Enemy { kind: u16, count: u32 },
    /// 13: summed over the allied kingdoms.
    Ally { kind: u16, count: u32 },
}

/// A script that does not compile (0x419070 shows it and the kingdom stays
/// without a computer player).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub file: String,
    /// From 1; 0 for a name still missing at the end.
    pub line: usize,
    pub column: usize,
    /// The native message.
    pub message: &'static str,
    /// The line of the script, or the missing name.
    pub text: String,
    /// The `INCLUDE`s that led to the file, the outermost first.
    pub included: Vec<(String, usize)>,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line == 0 {
            write!(f, "{}: {}", self.file, self.message)?;
            if !self.text.is_empty() {
                write!(f, ": {}", self.text)?;
            }
        } else {
            write!(
                f,
                "{}:{}:{}: {}: {}",
                self.file,
                self.line,
                self.column,
                self.message,
                self.text.trim()
            )?;
        }
        for (file, line) in self.included.iter().rev() {
            write!(f, " (incluso da {file}:{line})")?;
        }
        Ok(())
    }
}
impl std::error::Error for Error {}

const KEYWORDS: [&str; 24] = [
    "RULE",
    "CRITICAL",
    "WHEN",
    "HAVE",
    "TRIGGER",
    "FORGET",
    "THIS",
    "END",
    "TIME",
    "RANDOM",
    "EXTERN",
    "INCLUDE",
    "NOT",
    "CIV",
    "EUROPEAN",
    "ARABIAN",
    "ASIAN",
    "DIFFICULTY",
    "EASY",
    "NORMAL",
    "HARD",
    "SAY",
    "ENEMY",
    "ALLY",
];
mod kw {
    pub const RULE: u8 = 1;
    pub const CRITICAL: u8 = 2;
    pub const WHEN: u8 = 3;
    pub const HAVE: u8 = 4;
    pub const TRIGGER: u8 = 5;
    pub const FORGET: u8 = 6;
    pub const THIS: u8 = 7;
    pub const END: u8 = 8;
    pub const TIME: u8 = 9;
    pub const RANDOM: u8 = 10;
    pub const EXTERN: u8 = 11;
    pub const INCLUDE: u8 = 12;
    pub const NOT: u8 = 13;
    pub const CIV: u8 = 14;
    pub const EUROPEAN: u8 = 15;
    pub const ASIAN: u8 = 17;
    pub const DIFFICULTY: u8 = 18;
    pub const EASY: u8 = 19;
    pub const HARD: u8 = 21;
    pub const SAY: u8 = 22;
    pub const ENEMY: u8 = 23;
    pub const ALLY: u8 = 24;
}
/// The group nesting the native stack holds (+0x846).
const GROUPS: usize = 32;
/// ReTzar's guard against a file that includes itself.
const INCLUDES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    Symbol(u8),
    Number(u32),
    /// An identifier: its keyword (0 for none) and its bytes.
    Word {
        keyword: u8,
        start: usize,
        end: usize,
    },
    End,
}
impl Token {
    fn keyword(self) -> Option<u8> {
        match self {
            Token::Word { keyword, .. } if keyword != 0 => Some(keyword),
            _ => None,
        }
    }
}

/// A file being read.
struct Source {
    path: String,
    /// The text as written, for the messages.
    raw: Vec<u8>,
    /// In upper case (0x519de0), up to the first NUL.
    text: Vec<u8>,
    pos: usize,
    /// The start of the last token, where an error points.
    mark: usize,
}
impl Source {
    fn at(&self, i: usize) -> u8 {
        self.text.get(i).copied().unwrap_or(0)
    }
    fn word(&self, t: Token) -> &[u8] {
        match t {
            Token::Word { start, end, .. } => &self.text[start..end],
            _ => &[],
        }
    }
    /// 0x419850: blanks and `;` comments.
    fn blanks(&mut self) {
        loop {
            match self.at(self.pos) {
                b'\t' | b'\n' | b'\r' | b' ' => self.pos += 1,
                b';' => {
                    while !matches!(self.at(self.pos), 0 | b'\n' | b'\r') {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
        self.mark = self.pos;
    }
    /// 0x4196c0.
    fn token(&mut self) -> std::result::Result<Token, &'static str> {
        self.blanks();
        let c = self.at(self.pos);
        match c {
            0 => Ok(Token::End),
            b',' | b'{' | b'}' => {
                self.pos += 1;
                Ok(Token::Symbol(c))
            }
            b'0'..=b'9' => {
                let mut n = 0u32;
                while let d @ b'0'..=b'9' = self.at(self.pos) {
                    n = n.wrapping_mul(10).wrapping_add((d - b'0') as u32);
                    self.pos += 1;
                }
                if self.at(self.pos) == b'%' {
                    self.pos += 1;
                    n = n.min(0x7fff) | PERCENT;
                }
                Ok(Token::Number(n))
            }
            b'A'..=b'Z' | b'_' => {
                let start = self.pos;
                while matches!(self.at(self.pos), b'A'..=b'Z' | b'_' | b'0'..=b'9') {
                    self.pos += 1;
                }
                let word = &self.text[start..self.pos];
                let keyword = KEYWORDS
                    .iter()
                    .position(|k| k.as_bytes() == word)
                    .map_or(0, |i| i as u8 + 1);
                Ok(Token::Word {
                    keyword,
                    start,
                    end: self.pos,
                })
            }
            _ => Err("Unrecognized token"),
        }
    }
    /// A quoted text on one line (0x419a00, 0x419f7d, 0x41a970).
    fn quoted(&mut self) -> std::result::Result<std::ops::Range<usize>, &'static str> {
        self.blanks();
        if self.at(self.pos) != b'"' {
            return Err("Quote (\") expected");
        }
        self.pos += 1;
        let start = self.pos;
        while !matches!(self.at(self.pos), 0 | b'"' | b'\r' | b'\n') {
            self.pos += 1;
        }
        if self.at(self.pos) != b'"' {
            return Err("Quote (\") expected");
        }
        self.pos += 1;
        Ok(start..self.pos - 1)
    }
    fn error(&self, message: &'static str) -> Error {
        let at = self.mark.min(self.raw.len());
        let begin = self.raw[..at]
            .iter()
            .rposition(|&b| b == b'\n' || b == b'\r')
            .map_or(0, |i| i + 1);
        let finish = self.raw[at..]
            .iter()
            .position(|&b| b == b'\n' || b == b'\r')
            .map_or(self.raw.len(), |i| at + i);
        let breaks = |c| self.raw[..begin].iter().filter(|&&b| b == c).count();
        Error {
            file: self.path.clone(),
            line: 1 + breaks(b'\n').max(breaks(b'\r')),
            column: at - begin + 1,
            message,
            text: legacy_text(&self.raw[begin..finish]),
            included: Vec::new(),
        }
    }
}

/// A name met before its definition (14 bytes at +0x8ca).
struct Unresolved {
    name: Vec<u8>,
    trigger: bool,
    /// A condition's number, or a place of the list.
    at: Place,
}
enum Place {
    Condition(usize),
    List(usize),
}

struct Compiler<'a> {
    lib: &'a Library,
    program: Program,
    /// The rule names (+0x8d2), the one being read included.
    names: Vec<Vec<u8>>,
    unresolved: Vec<Unresolved>,
    /// The open groups' starts (+0x846).
    groups: Vec<u32>,
    depth: usize,
}
type Step<T = Token> = std::result::Result<T, &'static str>;
impl Compiler<'_> {
    /// 0x418c60 for one file; false when it is empty.
    fn file(&mut self, path: &str) -> std::result::Result<bool, Error> {
        let Some(raw) = self.lib.files.get(&crate::archive::key(path)) else {
            return Err(Error {
                file: path.to_owned(),
                line: 0,
                column: 0,
                message: "File not found",
                text: String::new(),
                included: Vec::new(),
            });
        };
        if raw.is_empty() {
            return Ok(false);
        }
        let mut text = raw.to_ascii_uppercase();
        if let Some(nul) = text.iter().position(|&b| b == 0) {
            text.truncate(nul);
        }
        let mut s = Source {
            path: path.to_owned(),
            raw: raw.clone(),
            text,
            pos: 0,
            mark: 0,
        };
        while s.pos < s.text.len() {
            let t = s.token().map_err(|e| s.error(e))?;
            let done = match t {
                Token::End => break,
                Token::Word {
                    keyword: kw::RULE, ..
                } => self.rule(&mut s),
                Token::Word {
                    keyword: kw::CRITICAL,
                    ..
                } => self.critical(&mut s),
                Token::Word {
                    keyword: kw::INCLUDE,
                    ..
                } => {
                    self.include(&mut s)?;
                    Ok(())
                }
                Token::Symbol(b'{') => self.open(),
                Token::Symbol(b'}') => self.close(),
                _ => Err("Statement expected"),
            };
            done.map_err(|e| s.error(e))?;
        }
        Ok(true)
    }
    /// The end of the top file: names met early are resolved (0x41aa20),
    /// in order; the first missing one is the error.
    fn finish(&mut self, path: &str) -> std::result::Result<(), Error> {
        for u in std::mem::take(&mut self.unresolved) {
            let found = if u.trigger {
                self.trigger_index(&u.name)
            } else {
                self.rule_index(&u.name)
            };
            let Some(i) = found else {
                return Err(Error {
                    file: path.to_owned(),
                    line: 0,
                    column: 0,
                    message: if u.trigger {
                        "Unresolved trigger"
                    } else {
                        "Unresolved rule"
                    },
                    text: String::from_utf8_lossy(&u.name).into_owned(),
                    included: Vec::new(),
                });
            };
            match u.at {
                Place::Condition(c) => match &mut self.program.conditions[c] {
                    Condition::Rule(x) | Condition::Trigger(x) => *x = i,
                    _ => {}
                },
                Place::List(l) => self.program.list[l] = i,
            }
        }
        // ReTzar: a group left open runs to the end of the script.
        let end = self.program.rules.len() as u32;
        for start in std::mem::take(&mut self.groups) {
            self.program.rules[start as usize].end = end;
        }
        Ok(())
    }
    fn rule_index(&self, name: &[u8]) -> Option<u32> {
        self.names.iter().position(|n| n == name).map(|i| i as u32)
    }
    fn trigger_index(&self, name: &[u8]) -> Option<u32> {
        self.program
            .triggers
            .iter()
            .position(|t| t.name.as_bytes() == name)
            .map(|i| i as u32)
    }
    /// 0x419540 and 0x419600: a name known now, or noted for the end.
    fn reference(&mut self, name: &[u8], trigger: bool, at: Place) -> u32 {
        let found = if trigger {
            self.trigger_index(name)
        } else {
            self.rule_index(name)
        };
        found.unwrap_or_else(|| {
            self.unresolved.push(Unresolved {
                name: name.to_vec(),
                trigger,
                at,
            });
            u32::MAX
        })
    }
    fn push_list(&mut self, n: u32) {
        self.program.list.push(n);
    }
    /// 0x4198a0.
    fn open(&mut self) -> Step<()> {
        if self.groups.len() >= GROUPS {
            return Err("Too deeep group nesting");
        }
        self.names.push(Vec::new());
        self.groups.push(self.program.rules.len() as u32);
        self.program.rules.push(Rule {
            flags: flag::GROUP_START,
            ..Default::default()
        });
        Ok(())
    }
    /// 0x419940: an empty group disappears; ReTzar drops its name too, which
    /// the original keeps, putting every later name one place off.
    fn close(&mut self) -> Step<()> {
        let Some(start) = self.groups.pop() else {
            return Err("Unexpected \"}\"");
        };
        if start as usize + 1 == self.program.rules.len() {
            self.program.rules.pop();
            self.names.pop();
            return Ok(());
        }
        self.names.push(Vec::new());
        let end = self.program.rules.len() as u32;
        self.program.rules.push(Rule {
            flags: flag::GROUP_END,
            ..Default::default()
        });
        self.program.rules[start as usize].end = end;
        Ok(())
    }
    /// 0x419a00: the file beside the current one; its error names this
    /// line too.
    fn include(&mut self, s: &mut Source) -> std::result::Result<(), Error> {
        let name = s.quoted().map_err(|e| s.error(e))?;
        if self.depth >= INCLUDES {
            return Err(s.error("Include nesting too deep"));
        }
        let folder = s.path.rfind(['\\', '/']).map_or(0, |i| i + 1);
        let path = format!(
            "{}{}",
            &s.path[..folder],
            String::from_utf8_lossy(&s.text[name])
        );
        self.depth += 1;
        let read = self.file(&path);
        self.depth -= 1;
        read.map(|_| ()).map_err(|mut e| {
            e.included.insert(0, (s.path.clone(), s.error("").line));
            e
        })
    }
    /// 0x419af0.
    fn critical(&mut self, s: &mut Source) -> Step<()> {
        match s.token()? {
            Token::Word {
                keyword: kw::RULE, ..
            } => {
                self.rule(s)?;
                if let Some(r) = self.program.rules.last_mut() {
                    r.flags |= flag::CRITICAL;
                }
                Ok(())
            }
            _ => Err("RULE expected"),
        }
    }
    /// 0x419b70.
    fn rule(&mut self, s: &mut Source) -> Step<()> {
        let mut rule = Rule {
            flags: flag::DIRTY,
            ..Default::default()
        };
        let first = s.token()?;
        let mut t = match first {
            Token::Word { keyword: 0, .. } => {
                let name = s.word(first).to_vec();
                if self.names.contains(&name) {
                    return Err("Rule name already used");
                }
                rule.name = String::from_utf8_lossy(&name).into_owned();
                self.names.push(name);
                let t = s.token()?;
                if t.keyword().is_none() {
                    return Err("Keyword expected");
                }
                t
            }
            Token::Word { .. } => {
                self.names.push(Vec::new());
                first
            }
            _ => return Err("Identifier expected"),
        };
        loop {
            t = match t.keyword() {
                Some(kw::END) => {
                    self.program.rules.push(rule);
                    return Ok(());
                }
                Some(kw::WHEN) => self.when(s, &mut rule)?,
                Some(kw::HAVE) => self.have(s, &mut rule)?,
                Some(kw::TRIGGER) => self.trigger(s, &mut rule)?,
                Some(kw::FORGET) => self.forget(s, &mut rule)?,
                Some(kw::SAY) => {
                    let text = s.quoted()?;
                    rule.say = Some(legacy_text(&s.text[text]));
                    s.token()?
                }
                _ => return Err("Statement expected"),
            };
            if t.keyword().is_none() {
                return Err("Keyword expected");
            }
        }
    }
    /// 0x419e40: conditions up to a keyword; whatever else follows one is
    /// taken as the separator.
    fn when(&mut self, s: &mut Source, rule: &mut Rule) -> Step {
        rule.when.start = self.program.list.len() as u32;
        loop {
            let t = self.condition(s, rule)?;
            if t.keyword().is_some() {
                return Ok(t);
            }
        }
    }
    /// 0x419ea0: one condition; the token after it is returned.
    fn condition(&mut self, s: &mut Source, rule: &mut Rule) -> Step {
        let t = s.token()?;
        let Token::Word { keyword, .. } = t else {
            return Err("Identifier expected");
        };
        // What the argument must be, whether it may be left out (count 1)
        // and whether it is the count (b) rather than the value (a).
        enum Arg {
            Number,
            Word,
        }
        let (mut c, arg, optional) = match keyword {
            0 => {
                let word = s.word(t);
                if word.eq_ignore_ascii_case(b"units") {
                    (Condition::Count { kind: 0, count: 0 }, Arg::Number, true)
                } else if let Some(r) = find(&RESOURCES, word) {
                    let resource = r as u8;
                    let c = Condition::Resource {
                        resource,
                        amount: 0,
                    };
                    (c, Arg::Number, false)
                } else {
                    let kind = self.type_of(word).ok_or("Bad condition")?;
                    (Condition::Count { kind, count: 0 }, Arg::Number, true)
                }
            }
            kw::RULE => (Condition::Rule(0), Arg::Word, false),
            kw::TRIGGER => (Condition::Trigger(0), Arg::Word, false),
            kw::TIME => (Condition::Time(0), Arg::Number, false),
            kw::RANDOM => (Condition::Random(0), Arg::Number, false),
            kw::EXTERN => {
                let text = s.quoted()?;
                let i = self.program.texts.len() as u32;
                self.program.texts.push(legacy_text(&s.text[text]));
                self.add_condition(rule, Condition::Extern(i));
                return s.token();
            }
            kw::NOT => {
                let next = self.condition(s, rule)?;
                let inner = self.program.conditions.len() as u32 - 1;
                let at = self.program.conditions.len() as u32;
                self.program.conditions.push(Condition::Not(inner));
                // The last entry of the run becomes the negation.
                let slot = (rule.when.start + rule.when.count - 1) as usize;
                if let Some(x) = self.program.list.get_mut(slot) {
                    *x = at;
                }
                return Ok(next);
            }
            kw::CIV => (Condition::Civ(0), Arg::Word, false),
            kw::DIFFICULTY => (Condition::Difficulty(0), Arg::Word, false),
            kw::ENEMY | kw::ALLY => {
                let t = s.token()?;
                if !matches!(t, Token::Word { .. }) {
                    return Err("Identifier expected");
                }
                if t.keyword().is_some() {
                    return Err("Unexpected keyword");
                }
                let word = s.word(t);
                let kind = if word.eq_ignore_ascii_case(b"units") {
                    0
                } else {
                    self.type_of(word).ok_or("Bad condition")?
                };
                let c = if keyword == kw::ENEMY {
                    Condition::Enemy { kind, count: 0 }
                } else {
                    Condition::Ally { kind, count: 0 }
                };
                (c, Arg::Number, true)
            }
            _ => return Err("Bad condition"),
        };
        let back = s.pos;
        let mut t = s.token()?;
        let fits = match arg {
            Arg::Number => matches!(t, Token::Number(_)),
            Arg::Word => matches!(t, Token::Word { .. }),
        };
        if !fits {
            if !optional {
                return Err(match arg {
                    Arg::Word => "Identifier expected",
                    Arg::Number => "Number expected",
                });
            }
            s.pos = back;
            t = Token::Number(1);
        }
        let at = self.program.conditions.len();
        match (t, &mut c) {
            (Token::Number(n), _) => {
                let mut n = n;
                if n & PERCENT != 0 {
                    match c {
                        Condition::Resource { .. } => return Err("Number expected"),
                        Condition::Random(_) => n &= !PERCENT,
                        _ => {}
                    }
                }
                match &mut c {
                    Condition::Resource { amount: x, .. }
                    | Condition::Count { count: x, .. }
                    | Condition::Enemy { count: x, .. }
                    | Condition::Ally { count: x, .. }
                    | Condition::Time(x)
                    | Condition::Random(x) => *x = n,
                    _ => {}
                }
            }
            (_, Condition::Rule(x)) => *x = self.reference(s.word(t), false, Place::Condition(at)),
            (_, Condition::Trigger(x)) => {
                *x = self.reference(s.word(t), true, Place::Condition(at))
            }
            (_, Condition::Civ(x)) => match t.keyword() {
                Some(k @ kw::EUROPEAN..=kw::ASIAN) => *x = k - kw::EUROPEAN + 1,
                _ => return Err("Invalid civilization"),
            },
            (_, Condition::Difficulty(x)) => match t.keyword() {
                Some(k @ kw::EASY..=kw::HARD) => *x = k - kw::EASY,
                _ => return Err("Invalid difficulty"),
            },
            _ => {}
        }
        if c == Condition::Rule(self.program.rules.len() as u32) {
            return Err("Recursed rule");
        }
        self.add_condition(rule, c);
        s.token()
    }
    fn add_condition(&mut self, rule: &mut Rule, c: Condition) {
        let i = self.program.conditions.len() as u32;
        self.program.conditions.push(c);
        self.push_list(i);
        rule.when.count += 1;
    }
    /// An object type by name, or by the name without its final `S`.
    fn type_of(&self, word: &[u8]) -> Option<u16> {
        find(&self.lib.types, word)
            .or_else(|| {
                word.strip_suffix(b"S")
                    .and_then(|w| find(&self.lib.types, w))
            })
            .map(|i| i as u16)
    }
    /// 0x41a5c0.
    fn have(&mut self, s: &mut Source, rule: &mut Rule) -> Step {
        let mut t = s.token()?;
        let count = match t {
            Token::Number(n) => {
                t = s.token()?;
                n as u16
            }
            _ => 1,
        };
        if !matches!(t, Token::Word { .. }) {
            return Err("Identifier expected");
        }
        if t.keyword().is_some() {
            return Err("Unexpected keyword");
        }
        let word = s.word(t);
        let kind = match find(&GOALS, word) {
            Some(i) => goal::CONVERT - i as u16,
            // Type 0 (`none`) is no goal.
            None => find(&self.lib.types, word)
                .filter(|&i| i != 0)
                .or_else(|| {
                    word.strip_suffix(b"S")
                        .and_then(|w| find(&self.lib.types, w))
                        .filter(|&i| i != 0)
                })
                .ok_or("Invalid type")? as u16,
        };
        rule.have = Some(Have { kind, count });
        s.token()
    }
    /// 0x41a730.
    fn trigger(&mut self, s: &mut Source, rule: &mut Rule) -> Step {
        rule.triggers.start = self.program.list.len() as u32;
        loop {
            s.blanks();
            let off = s.at(s.pos) == b'!';
            if off {
                s.pos += 1;
            }
            let t = s.token()?;
            if !matches!(t, Token::Word { .. }) {
                return Err("Identifier expected");
            }
            if t.keyword().is_some() {
                return Err("Unexpected keyword");
            }
            let name = s.word(t);
            let i = self.trigger_index(name).unwrap_or_else(|| {
                self.program.triggers.push(Trigger {
                    name: String::from_utf8_lossy(name).into_owned(),
                    on: false,
                });
                self.program.triggers.len() as u32 - 1
            });
            self.push_list(i | if off { 1 << 31 } else { 0 });
            rule.triggers.count += 1;
            let t = s.token()?;
            if t != Token::Symbol(b',') {
                return Ok(t);
            }
        }
    }
    /// 0x41a850.
    fn forget(&mut self, s: &mut Source, rule: &mut Rule) -> Step {
        rule.forget.start = self.program.list.len() as u32;
        let mut t = s.token()?;
        loop {
            if !matches!(t, Token::Word { .. }) {
                return Err("Identifier expected");
            }
            let i = match t.keyword() {
                Some(kw::THIS) => self.program.rules.len() as u32,
                Some(_) => return Err("Unexpected keyword"),
                None => {
                    let at = Place::List(self.program.list.len());
                    self.reference(s.word(t), false, at)
                }
            };
            self.push_list(i);
            rule.forget.count += 1;
            t = s.token()?;
            if t != Token::Symbol(b',') {
                return Ok(t);
            }
            t = s.token()?;
        }
    }
}

#[cfg(test)]
#[path = "script_tests.rs"]
mod tests;
