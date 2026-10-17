//! The expression language of the mission scripts (0x4b7fc0).
//!
//! Text is split at `;` into statements; each one is turned into postfix
//! (0x4b7ba0) and evaluated from the end (0x4b7860). All values are 32-bit
//! signed integers. The rules below are those of the native code, quirks
//! included: an operator only pops operators of strictly higher precedence,
//! so equal precedences group from the right (`8-4-2` is 6); `&&` and `||`
//! always evaluate both sides; division and remainder by zero give 0; a
//! missing operand reads as 0; `&` and `|` bind tighter than `+` and `-`.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The services an expression may call: kingdoms, objects, events.
pub trait Host {
    /// `@function(args)`, with the name matched as the native table does.
    fn call(&mut self, function: Function, args: [i32; 8], object: Option<usize>) -> i32;
    /// `@playmovie(name)`.
    fn movie(&mut self, name: &str);
    /// An object property (`$health`, …) of the context object.
    fn property(&mut self, object: usize, name: Property) -> i32;
    fn set_property(&mut self, object: usize, name: Property, value: i32);
    /// `$gold`, `$stone`, `$food`, `$wood` of the local kingdom, `$difficulty`.
    fn global(&mut self, name: Global) -> i32;
    fn set_global(&mut self, name: Global, value: i32);
    /// A named event of the map, evaluated now; `None` when there is none.
    fn event(&mut self, name: &str) -> Option<bool>;
    fn variables(&mut self) -> &mut Variables;
}

/// The two variable tables: `$name` and plain names (timers) live with the
/// map (0xc8f540); `#name` survive the next map of a campaign (0xc8b128).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variables {
    pub map: BTreeMap<String, i32>,
    pub kept: BTreeMap<String, i32>,
}
/// Each table holds 256 names (64 bytes each, 63 characters).
const TABLE: usize = 256;
impl Variables {
    fn table(&mut self, name: &str) -> &mut BTreeMap<String, i32> {
        if name.starts_with('#') {
            &mut self.kept
        } else {
            &mut self.map
        }
    }
    /// The value of a variable; an unknown name is created with 0.
    pub fn get(&mut self, name: &str) -> i32 {
        let key: String = name.chars().take(63).collect();
        let table = self.table(name);
        if let Some(v) = table.get(&key) {
            return *v;
        }
        if table.len() < TABLE {
            table.insert(key, 0);
        }
        0
    }
    pub fn set(&mut self, name: &str, value: i32) {
        let key: String = name.chars().take(63).collect();
        let table = self.table(name);
        if table.len() < TABLE || table.contains_key(&key) {
            table.insert(key, value);
        }
    }
}

/// The functions of the native table, in its order (0x4b83d0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Function {
    Gold,
    Stone,
    Food,
    Wood,
    Rnd,
    Str,
    KingdomCanRescue,
    SetAi,
    Select,
    Unselect,
    ClearSelection,
    God,
    SetTime,
    GetTime,
    Min,
    Max,
    GetDiplomacy,
    IsAlly,
    IsNeutral,
    IsEnemy,
    HasSharedVision,
    HasSharedUnits,
    HasSharedRes,
    SetPopLimit,
    Build,
    SetFood,
    SetWood,
    SetStone,
    SetGold,
    SetNoAttackTime,
    Count,
    InProgress,
}
pub(crate) const FUNCTIONS: [(&str, Function); 32] = [
    ("@gold", Function::Gold),
    ("@stone", Function::Stone),
    ("@food", Function::Food),
    ("@wood", Function::Wood),
    ("@rnd", Function::Rnd),
    ("@str", Function::Str),
    ("@kingdomcanrescue", Function::KingdomCanRescue),
    ("@setai", Function::SetAi),
    ("@select", Function::Select),
    ("@unselect", Function::Unselect),
    ("@clearselection", Function::ClearSelection),
    ("@god", Function::God),
    ("@settime", Function::SetTime),
    ("@gettime", Function::GetTime),
    ("@min", Function::Min),
    ("@max", Function::Max),
    ("@getdiplomacy", Function::GetDiplomacy),
    ("@isally", Function::IsAlly),
    ("@isneutral", Function::IsNeutral),
    ("@isenemy", Function::IsEnemy),
    ("@hassharedvision", Function::HasSharedVision),
    ("@hassharedunits", Function::HasSharedUnits),
    ("@hassharedres", Function::HasSharedRes),
    ("@setpoplimit", Function::SetPopLimit),
    ("@build", Function::Build),
    ("@setfood", Function::SetFood),
    ("@setwood", Function::SetWood),
    ("@setstone", Function::SetStone),
    ("@setgold", Function::SetGold),
    ("@setnoattacktime", Function::SetNoAttackTime),
    ("@count", Function::Count),
    ("@inprogress", Function::InProgress),
];
/// Object properties, readable only with a context object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Property {
    Health,
    MaxHealth,
    Attack,
    Defence,
    Level,
    Manna,
    MaxManna,
    Kingdom,
    Race,
    X,
    Y,
    Cargo,
    IsSelected,
    InHolder,
    Program,
    Flags,
    UnderFog,
}
pub(crate) const PROPERTIES: [(&str, Property); 17] = [
    ("$health", Property::Health),
    ("$maxhealth", Property::MaxHealth),
    ("$attack", Property::Attack),
    ("$defence", Property::Defence),
    ("$level", Property::Level),
    ("$manna", Property::Manna),
    ("$maxmanna", Property::MaxManna),
    ("$kingdom", Property::Kingdom),
    ("$race", Property::Race),
    ("$x", Property::X),
    ("$y", Property::Y),
    ("$cargo", Property::Cargo),
    ("$isselected", Property::IsSelected),
    ("$inholder", Property::InHolder),
    ("$program", Property::Program),
    ("$flags", Property::Flags),
    ("$underfog", Property::UnderFog),
];
/// Properties the setter (0x4b8f50) writes.
const WRITABLE: [Property; 7] = [
    Property::Health,
    Property::MaxHealth,
    Property::Attack,
    Property::Defence,
    Property::Level,
    Property::Manna,
    Property::MaxManna,
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Global {
    Gold,
    Stone,
    Food,
    Wood,
    Difficulty,
    /// `$candelobjects`, write only (0x5cf6b0 = !v).
    CanDeleteObjects,
    /// `$canusediplomacy`, write only (0x5877a0).
    CanUseDiplomacy,
}
pub(crate) const GLOBALS: [(&str, Global); 5] = [
    ("$gold", Global::Gold),
    ("$stone", Global::Stone),
    ("$food", Global::Food),
    ("$wood", Global::Wood),
    ("$difficulty", Global::Difficulty),
];

/// Expression actions turn line breaks into `;` (0x4b80a0).
pub fn statements(text: &str) -> String {
    text.replace(['\r', '\n'], ";")
}
/// Rule conditions turn line breaks into spaces (0x4b8060).
pub fn condition(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// Evaluate `text` (statements separated by `;`): the value of the last one.
pub fn eval(text: &str, host: &mut dyn Host, object: Option<usize>) -> i32 {
    // The native copy is 0x800 bytes.
    let text: String = text.chars().take(0x7ff).collect();
    let mut value;
    let mut rest = text.as_str();
    loop {
        let (statement, tail) = match rest.find(';') {
            Some(at) => (&rest[..at], Some(&rest[at + 1..])),
            None => (rest, None),
        };
        value = statement_value(statement, host, object);
        match tail {
            Some(t) if !t.is_empty() => rest = t,
            _ => break,
        }
    }
    value
}

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Operand(String),
    /// Operator code (one or two characters, the second in the high byte)
    /// and precedence including the bracket level.
    Operator(u16, i32),
}

fn operand_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'$' | b'#' | b'_' | b'@')
}

/// 0x4b7ba0: shunting-yard into postfix, then evaluation.
fn statement_value(text: &str, host: &mut dyn Host, object: Option<usize>) -> i32 {
    let s = text.as_bytes();
    let mut out: Vec<Node> = Vec::new();
    let mut stack: Vec<Node> = Vec::new();
    let mut level = 0i32;
    let mut at = 0;
    loop {
        while s.get(at) == Some(&b' ') {
            at += 1;
        }
        let Some(&c) = s.get(at) else { break };
        let pair = s.get(at + 1).map(|&d| u16::from(c) | u16::from(d) << 8);
        let (code, precedence, width) = match pair {
            Some(0x3d3d | 0x3d21 | 0x3d3c | 0x3d3e) => (pair.unwrap(), 50, 2),
            Some(0x2626) => (0x2626, 20, 2),
            Some(0x7c7c) => (0x7c7c, 10, 2),
            _ => match c {
                b'(' => {
                    level += 100;
                    at += 1;
                    continue;
                }
                b')' => {
                    level -= 100;
                    at += 1;
                    continue;
                }
                b'!' | b'~' => (c as u16, 90, 1),
                b'%' | b'*' | b'/' => (c as u16, 70, 1),
                b'&' | b'|' => (c as u16, 65, 1),
                b'+' | b'-' => (c as u16, 60, 1),
                b'<' | b'>' => (c as u16, 50, 1),
                b'=' => (c as u16, 5, 1),
                b',' => (c as u16, 3, 1),
                _ => (0, 0, 0),
            },
        };
        if width == 0 {
            // An operand: a name or number; `@name(` takes the bracketed
            // arguments with it.
            let start = at;
            while s.get(at).is_some_and(|&b| operand_char(b)) {
                at += 1;
            }
            if c == b'@' && s.get(at) == Some(&b'(') {
                let mut depth = 1;
                at += 1;
                while depth > 0 && at < s.len() {
                    match s[at] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    at += 1;
                }
            }
            if at == start {
                // The native loop would not advance on a stray character.
                at += 1;
                continue;
            }
            out.push(Node::Operand(text[start..at].to_owned()));
            continue;
        }
        at += width;
        let precedence = precedence + level;
        while let Some(Node::Operator(_, top)) = stack.last() {
            if *top <= precedence {
                break;
            }
            out.push(stack.pop().unwrap());
        }
        stack.push(Node::Operator(code, precedence));
    }
    if level != 0 {
        // "Expression: Brackets didn't match".
        return 0;
    }
    while let Some(node) = stack.pop() {
        out.push(node);
    }
    evaluate(&mut out, host, object)
}

/// 0x4b7860: pop a node; operators evaluate their right operand first.
fn evaluate(nodes: &mut Vec<Node>, host: &mut dyn Host, object: Option<usize>) -> i32 {
    loop {
        let Some(node) = nodes.pop() else { return 0 };
        let code = match node {
            Node::Operand(token) => return operand(&token, host, object),
            Node::Operator(code, _) => code,
        };
        let right = evaluate(nodes, host, object);
        if code == u16::from(b'=') {
            // Assignment takes the next node as the name to write.
            if let Some(Node::Operand(name)) = nodes.pop() {
                assign(&name, right, host, object);
            }
            return right;
        }
        if code == u16::from(b',') {
            // The right side is dropped and evaluation goes on.
            continue;
        }
        if matches!(code, 0x21 | 0x7e) {
            return if code == 0x21 {
                (right == 0) as i32
            } else {
                !right
            };
        }
        let left = evaluate(nodes, host, object);
        return match code {
            0x25 if right == 0 => 0,
            0x25 => left.wrapping_rem(right),
            0x2f if right == 0 => 0,
            0x2f => left.wrapping_div(right),
            0x2a => left.wrapping_mul(right),
            0x2b => left.wrapping_add(right),
            0x2d => left.wrapping_sub(right),
            0x26 => left & right,
            0x7c => left | right,
            0x3c => (left < right) as i32,
            0x3e => (left > right) as i32,
            0x3d3c => (left <= right) as i32,
            0x3d3e => (left >= right) as i32,
            0x3d3d => (left == right) as i32,
            0x3d21 => (left != right) as i32,
            0x2626 => (left != 0 && right != 0) as i32,
            0x7c7c => (left != 0 || right != 0) as i32,
            // "Expression: Unknown operator".
            _ => 0,
        };
    }
}

/// Decimal digits as `atoi` reads them.
fn atoi(token: &str) -> i32 {
    token
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0i32, |v, d| {
            v.wrapping_mul(10).wrapping_add((d - b'0') as i32)
        })
}

/// 0x4b83d0: functions, object properties, globals, events, variables.
fn operand(token: &str, host: &mut dyn Host, object: Option<usize>) -> i32 {
    if token.as_bytes()[0].is_ascii_digit() {
        return atoi(token);
    }
    if token.starts_with('@') {
        let open = token.find('(').unwrap_or(token.len());
        let name = &token[..open];
        if token.len() >= 10 && token[..10].eq_ignore_ascii_case("@playmovie") {
            let inner = token
                .get(open + 1..token.len().saturating_sub(1))
                .unwrap_or("");
            host.movie(inner);
            return 0;
        }
        // `_strnicmp` over the length of the name: a shorter name matches
        // the first function it begins.
        let Some(&(_, function)) = FUNCTIONS
            .iter()
            .find(|(f, _)| f.len() >= name.len() && f[..name.len()].eq_ignore_ascii_case(name))
        else {
            return host.variables().get(token);
        };
        let mut args = [0; 8];
        if open < token.len() {
            let inner = &token[open + 1..];
            let inner = inner.strip_suffix(')').unwrap_or(inner);
            for (slot, arg) in args.iter_mut().zip(arguments(inner)) {
                *slot = eval(arg, host, object);
            }
        }
        return host.call(function, args, object);
    }
    if let Some(i) = object
        && let Some(&(_, p)) = PROPERTIES
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(token))
    {
        return host.property(i, p);
    }
    if let Some(&(_, g)) = GLOBALS.iter().find(|(n, _)| n.eq_ignore_ascii_case(token)) {
        return host.global(g);
    }
    if let Some(v) = host.event(token) {
        return v as i32;
    }
    host.variables().get(token)
}

/// Top-level comma-separated arguments (0x4b80e0).
fn arguments(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ',' if depth == 0 => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// 0x4b8f50: properties and globals that have an effect; every name is also
/// stored in its variable table.
fn assign(name: &str, value: i32, host: &mut dyn Host, object: Option<usize>) {
    if let Some(i) = object
        && let Some(&(_, p)) = PROPERTIES
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
        && WRITABLE.contains(&p)
    {
        host.set_property(i, p, value);
    } else if let Some(&(_, g)) = GLOBALS[..4]
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
    {
        host.set_global(g, value);
    } else if name.eq_ignore_ascii_case("$candelobjects") {
        host.set_global(Global::CanDeleteObjects, value);
    } else if name.eq_ignore_ascii_case("$canusediplomacy") {
        host.set_global(Global::CanUseDiplomacy, value);
    }
    host.variables().set(name, value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Test {
        vars: Variables,
        gold: i32,
        health: i32,
        calls: Vec<(Function, [i32; 8])>,
        movies: Vec<String>,
    }
    impl Host for Test {
        fn call(&mut self, f: Function, args: [i32; 8], _: Option<usize>) -> i32 {
            self.calls.push((f, args));
            match f {
                Function::Food => 100 * args[0],
                Function::Max => args[0].max(args[1]),
                _ => 0,
            }
        }
        fn movie(&mut self, name: &str) {
            self.movies.push(name.into());
        }
        fn property(&mut self, _: usize, p: Property) -> i32 {
            if p == Property::Health {
                self.health
            } else {
                0
            }
        }
        fn set_property(&mut self, _: usize, p: Property, v: i32) {
            if p == Property::Health {
                self.health = v;
            }
        }
        fn global(&mut self, g: Global) -> i32 {
            if g == Global::Gold { self.gold } else { 0 }
        }
        fn set_global(&mut self, g: Global, v: i32) {
            if g == Global::Gold {
                self.gold = v;
            }
        }
        fn event(&mut self, name: &str) -> Option<bool> {
            (name == "inicio").then_some(true)
        }
        fn variables(&mut self) -> &mut Variables {
            &mut self.vars
        }
    }

    fn run(text: &str) -> i32 {
        eval(text, &mut Test::default(), None)
    }

    #[test]
    fn native_precedence_and_grouping() {
        assert_eq!(run("1 + 2 * 3"), 7);
        // Equal precedence groups from the right.
        assert_eq!(run("8 - 4 - 2"), 6);
        assert_eq!(run("16 / 4 / 2"), 8);
        // `&` binds tighter than `+`.
        assert_eq!(run("1 + 2 & 3"), 3);
        assert_eq!(run("(1 + 2) * 3"), 9);
        assert_eq!(run("5 / 0"), 0);
        assert_eq!(run("5 % 0"), 0);
        assert_eq!(run("-5"), -5);
        assert_eq!(run("!0 && 3 >= 2"), 1);
        assert_eq!(run("~0"), -1);
        assert_eq!(run("(1 + 2"), 0);
        assert_eq!(run("12abc + 1"), 13);
        assert_eq!(run("1;2;3"), 3);
    }

    #[test]
    fn variables_events_and_assignment() {
        let mut t = Test::default();
        assert_eq!(eval("$a = $b = 4", &mut t, None), 4);
        assert_eq!(t.vars.map["$a"], 4);
        assert_eq!(t.vars.map["$b"], 4);
        // An unknown name is created with 0; names are case-sensitive.
        assert_eq!(eval("$A", &mut t, None), 0);
        assert!(t.vars.map.contains_key("$A"));
        assert_eq!(eval("inicio && $a==4", &mut t, None), 1);
        assert_eq!(eval("#kept = 2", &mut t, None), 2);
        assert_eq!(t.vars.kept["#kept"], 2);
        // A comparison written as an action does nothing.
        eval(&statements("$a==0\r\n$c=1"), &mut t, None);
        assert_eq!((t.vars.map["$a"], t.vars.map["$c"]), (4, 1));
        eval("$gold = $gold + 500", &mut t, None);
        assert_eq!(t.gold, 500);
        // Object properties need a context object.
        t.health = 90;
        assert_eq!(eval("$health<=100", &mut t, Some(3)), 1);
        eval("$health=110", &mut t, Some(3));
        assert_eq!(t.health, 110);
        // Without one the name is a variable, which the setter also wrote.
        assert_eq!(eval("$health", &mut t, None), 110);
        t.health = 0;
        assert_eq!(eval("$health", &mut t, Some(3)), 0);
    }

    #[test]
    fn functions_take_bracketed_arguments() {
        let mut t = Test::default();
        assert_eq!(eval("@food(1) >= 100", &mut t, None), 1);
        assert_eq!(eval("@max(@food(1), 7*50)", &mut t, None), 350);
        assert_eq!(eval("@SetFood(4, @food(4) - @food(4))", &mut t, None), 0);
        assert_eq!(
            t.calls.last().unwrap(),
            &(Function::SetFood, [4, 0, 0, 0, 0, 0, 0, 0])
        );
        // A prefix of a name matches the first function it begins.
        eval("@go(2)", &mut t, None);
        assert_eq!(t.calls.last().unwrap().0, Function::Gold);
        eval("@playmovie(intro)", &mut t, None);
        assert_eq!(t.movies, ["intro"]);
    }
}
