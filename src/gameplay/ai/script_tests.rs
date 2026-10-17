//! The script compiler against the native one (0x418c60 and the routines
//! it calls), with small scripts written for each rule of the language.
use super::*;

fn types() -> Vec<String> {
    let mut t: Vec<String> = CLASSES.iter().map(|&s| s.to_owned()).collect();
    t.extend(["to_wtower2", "te_feudalism"].map(str::to_owned));
    t
}
fn library(files: &[(&str, &str)]) -> Library {
    Library::new(
        files
            .iter()
            .map(|(n, t)| (format!("data\\ai\\{n}"), t.as_bytes().to_vec())),
        types(),
    )
}
fn compile(text: &str) -> std::result::Result<Program, Error> {
    library(&[("test.ais", text)]).compile("test.ais")
}
fn error(text: &str) -> &'static str {
    compile(text).unwrap_err().message
}
fn conditions(p: &Program, rule: usize) -> Vec<Condition> {
    p.entries(p.rules[rule].when)
        .iter()
        .map(|&c| p.conditions[c as usize])
        .collect()
}
const PEON: u16 = 1;
const ARCHER: u16 = 4;
const BARRACK: u16 = 45;

#[test]
fn rules_take_their_clauses_in_any_order() {
    let p = compile(
        "; a comment\n\
         rule First when gold 100, archers 5 have 2 barracks trigger Army, !Peace say \"Ready\" end\n\
         critical rule have peon end\n\
         rule forget this, First end",
    )
    .unwrap();
    assert_eq!(p.rules.len(), 3);
    let r = &p.rules[0];
    assert_eq!(r.name, "FIRST");
    assert_eq!(r.flags, flag::DIRTY);
    assert_eq!(
        conditions(&p, 0),
        [
            Condition::Resource {
                resource: 0,
                amount: 100
            },
            Condition::Count {
                kind: ARCHER,
                count: 5
            }
        ]
    );
    assert_eq!(
        r.have,
        Some(Have {
            kind: BARRACK,
            count: 2
        })
    );
    // Text is read in upper case, the `SAY` too.
    assert_eq!(r.say.as_deref(), Some("READY"));
    assert_eq!(p.entries(r.triggers), [0, 1 | 1 << 31]);
    assert_eq!(
        p.triggers,
        [
            Trigger {
                name: "ARMY".into(),
                on: false
            },
            Trigger {
                name: "PEACE".into(),
                on: false
            }
        ]
    );
    // `HAVE` without a number wants one; `CRITICAL` marks the rule.
    assert_eq!(p.rules[1].flags, flag::DIRTY | flag::CRITICAL);
    assert_eq!(
        p.rules[1].have,
        Some(Have {
            kind: PEON,
            count: 1
        })
    );
    assert_eq!(p.rules[1].name, "");
    // `FORGET THIS` is the rule itself.
    assert_eq!(p.entries(p.rules[2].forget), [2, 0]);
}

#[test]
fn conditions_read_their_arguments_as_the_original() {
    let p = compile(
        "rule when units, units 30, peon 40%, time 60000, time 5%, random 40%, \
         civ asian, difficulty normal, enemy archers, ally units 3, \
         extern \"$aik == 2\" end",
    )
    .unwrap();
    assert_eq!(
        conditions(&p, 0),
        [
            Condition::Count { kind: 0, count: 1 },
            Condition::Count { kind: 0, count: 30 },
            Condition::Count {
                kind: PEON,
                count: 40 | PERCENT
            },
            Condition::Time(60000),
            // The `%` stays on a time and is dropped from a chance (0x41a3a8).
            Condition::Time(5 | PERCENT),
            Condition::Random(40),
            Condition::Civ(3),
            Condition::Difficulty(1),
            Condition::Enemy {
                kind: ARCHER,
                count: 1
            },
            Condition::Ally { kind: 0, count: 3 },
            Condition::Extern(0),
        ]
    );
    assert_eq!(p.texts, ["$AIK == 2"]);
    // A percentage is clamped to 32767 before its bit is set.
    let p = compile("rule have 40000% peon end").unwrap();
    assert_eq!(p.rules[0].have.unwrap().count, 0xffff);
    // Without `%` the number is taken whole, the goal keeping 16 bits.
    let p = compile("rule when gold 70000 have 70000 gold end").unwrap();
    assert_eq!(
        conditions(&p, 0),
        [Condition::Resource {
            resource: 0,
            amount: 70000
        }]
    );
    assert_eq!(
        p.rules[0].have,
        Some(Have {
            kind: goal::GOLD,
            count: 70000u32 as u16
        })
    );
}

#[test]
fn a_negation_takes_the_place_of_its_condition() {
    let p = compile("rule when gold 5, not trigger T, not not stone 7 trigger T end").unwrap();
    // The inner conditions stay in the table; the list names the negations.
    assert_eq!(
        p.conditions,
        [
            Condition::Resource {
                resource: 0,
                amount: 5
            },
            Condition::Trigger(0),
            Condition::Not(1),
            Condition::Resource {
                resource: 1,
                amount: 7
            },
            Condition::Not(3),
            Condition::Not(4),
        ]
    );
    assert_eq!(p.entries(p.rules[0].when), [0, 2, 5]);
}

#[test]
fn whatever_follows_a_condition_but_a_keyword_separates_it() {
    // 0x419e40 drops the token after a condition unless it is a keyword.
    let p = compile("rule when gold 1 } stone 2 3 food 4 end").unwrap();
    assert_eq!(conditions(&p, 0).len(), 3);
    assert_eq!(error("rule when gold 1 stone 2 end"), "Identifier expected");
}

#[test]
fn names_are_found_whole_or_without_their_final_s() {
    let p = compile("rule when barracks, peons, te_feudalism have 2 to_wtower2 end").unwrap();
    assert_eq!(
        conditions(&p, 0),
        [
            Condition::Count {
                kind: BARRACK,
                count: 1
            },
            Condition::Count {
                kind: PEON,
                count: 1
            },
            Condition::Count {
                kind: CLASSES.len() as u16 + 1,
                count: 1
            },
        ]
    );
    assert_eq!(p.rules[0].have.unwrap().kind, CLASSES.len() as u16);
    // The goals that are not types.
    let p = compile(
        "rule have convert end rule have 5 stone end rule have needs_locked end \
         rule have resign end",
    )
    .unwrap();
    let goals: Vec<u16> = p.rules.iter().map(|r| r.have.unwrap().kind).collect();
    assert_eq!(
        goals,
        [goal::CONVERT, goal::STONE, goal::NEEDS_LOCKED, goal::RESIGN]
    );
    assert_eq!(error("rule have none end"), "Invalid type");
    assert_eq!(error("rule have wizard end"), "Invalid type");
    assert_eq!(error("rule have units end"), "Invalid type");
    assert_eq!(error("rule when wizard end"), "Bad condition");
    assert_eq!(error("rule have rule end"), "Unexpected keyword");
}

#[test]
fn rules_and_switches_may_be_named_before_they_exist() {
    let p = compile(
        "rule when rule Later, trigger Flag forget Later end\n\
         rule Later trigger Flag end",
    )
    .unwrap();
    assert_eq!(
        conditions(&p, 0),
        [Condition::Rule(1), Condition::Trigger(0)]
    );
    assert_eq!(p.entries(p.rules[0].forget), [1]);
    assert_eq!(error("rule when rule Nowhere end"), "Unresolved rule");
    assert_eq!(error("rule forget Nowhere end"), "Unresolved rule");
    // A switch nobody sets is an error too.
    assert_eq!(error("rule when trigger Nowhere end"), "Unresolved trigger");
    assert_eq!(error("rule Me when rule Me end"), "Recursed rule");
    assert_eq!(
        error("rule Twice end rule Twice end"),
        "Rule name already used"
    );
}

#[test]
fn groups_mark_their_ends_and_empty_ones_vanish() {
    let p = compile(
        "{ critical rule when gold 1 end\n\
           { }\n\
           { rule A end }\n\
         }\n\
         rule B when rule A end",
    )
    .unwrap();
    let flags: Vec<u32> = p.rules.iter().map(|r| r.flags).collect();
    assert_eq!(
        flags,
        [
            flag::GROUP_START,
            flag::DIRTY | flag::CRITICAL,
            flag::GROUP_START,
            flag::DIRTY,
            flag::GROUP_END,
            flag::GROUP_END,
            flag::DIRTY
        ]
    );
    assert_eq!((p.rules[0].end, p.rules[2].end), (5, 4));
    // After the empty group the names still match the rules.
    assert_eq!(conditions(&p, 6), [Condition::Rule(3)]);
    assert_eq!(error("}"), "Unexpected \"}\"");
    assert_eq!(error(&"{ ".repeat(33)), "Too deeep group nesting");
    // ReTzar: a group left open reaches the end of the script.
    let p = compile("{ rule end rule end").unwrap();
    assert_eq!(p.rules[0].end, 3);
}

#[test]
fn a_clause_given_twice_restarts_its_run_but_keeps_counting() {
    // 0x41a730 moves the start of the run to the second clause and adds
    // to the count: the run reads one entry past it, here the next rule's
    // condition.
    let p = compile("rule trigger A trigger B end rule when gold 3 end").unwrap();
    assert_eq!(p.rules[0].triggers, Span { start: 1, count: 2 });
    assert_eq!(p.entries(p.rules[0].triggers), [1, 0]);
    // The last values of `HAVE` and `SAY` win.
    let p = compile("rule have peon say \"A\" have 3 archer say \"B\" end").unwrap();
    assert_eq!(
        p.rules[0].have,
        Some(Have {
            kind: ARCHER,
            count: 3
        })
    );
    assert_eq!(p.rules[0].say.as_deref(), Some("B"));
}

#[test]
fn statements_and_tokens_out_of_place_are_errors() {
    assert_eq!(error("when"), "Statement expected");
    assert_eq!(error("rule 5 end"), "Identifier expected");
    assert_eq!(error("rule Name 5 end"), "Keyword expected");
    assert_eq!(error("rule rule end"), "Statement expected");
    assert_eq!(error("critical when"), "RULE expected");
    assert_eq!(error("rule when gold 5% end"), "Number expected");
    assert_eq!(error("rule when time end"), "Number expected");
    assert_eq!(error("rule when civ french end"), "Invalid civilization");
    assert_eq!(
        error("rule when difficulty easy2 end"),
        "Invalid difficulty"
    );
    assert_eq!(error("rule say Hello end"), "Quote (\") expected");
    assert_eq!(error("rule say \"Hello\nend"), "Quote (\") expected");
    assert_eq!(error("rule when gold 1 & end"), "Unrecognized token");
    assert_eq!(error("rule trigger end end"), "Unexpected keyword");
    // An empty file is a script without rules (0x418dd2).
    assert_eq!(compile("").unwrap(), Program::default());
}

#[test]
fn includes_are_read_beside_the_including_file() {
    let lib = library(&[
        (
            "main.ais",
            "rule A end\ninclude \"part.ai\"\nrule when rule B end",
        ),
        ("part.ai", "rule B trigger T end\ninclude \"deeper.ai\""),
        ("deeper.ai", "rule when trigger T end"),
        ("broken.ais", "rule A end\n\n  include \"bad.ai\""),
        ("bad.ai", "rule C end\nrule\n  when bogus end"),
    ]);
    let p = lib.compile("main.ais").unwrap();
    let names: Vec<&str> = p.rules.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["A", "B", "", ""]);
    assert_eq!(conditions(&p, 3), [Condition::Rule(1)]);
    // An error inside an included file names its line and the include.
    let e = lib.compile("broken.ais").unwrap_err();
    assert_eq!(
        (e.file.as_str(), e.line, e.column, e.message),
        ("data\\ai\\BAD.AI", 3, 8, "Bad condition")
    );
    assert_eq!(e.text, "  when bogus end");
    assert_eq!(e.included, [("data\\ai\\broken.ais".to_owned(), 3)]);
    assert_eq!(
        lib.compile("missing.ais").unwrap_err().message,
        "File not found"
    );
    // A name with a folder is read as written.
    assert!(lib.compile("DATA\\AI\\MAIN.AIS").is_ok());
}

fn original() -> (std::path::PathBuf, Library) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let lib = Library::load(&GameAssets::open(&root).unwrap()).unwrap();
    (root, lib)
}

#[test]
#[ignore = "Requires installed original archives"]
fn every_original_script_compiles() {
    let (_, lib) = original();
    let mut scripts: Vec<_> = lib.files().filter(|n| n.ends_with(".AIS")).collect();
    scripts.sort();
    assert_eq!(scripts.len(), 12);
    for name in scripts {
        let p = lib.compile(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        // The original counts the switches in a byte (+0x18, 0x4192e0).
        assert!(p.triggers.len() < 256, "{name}");
    }
    // The default script reads every common part of the three civilizations.
    let p = lib.compile("random.ais").unwrap();
    for t in ["STARTED", "ARMY", "PATH1", "PATH2", "PATH1_TAKEN"] {
        assert!(p.triggers.iter().any(|x| x.name == t), "{t}");
    }
}

#[test]
#[ignore = "Requires installed original archives"]
fn the_scripts_the_maps_name_compile() {
    let (root, lib) = original();
    let catalog = crate::scenario::ScenarioCatalog::load(&root).unwrap();
    let scenarios = catalog
        .campaigns
        .iter()
        .flat_map(|c| &c.missions)
        .chain(&catalog.missions);
    let mut named = std::collections::BTreeSet::new();
    for s in scenarios {
        let c = s.container().unwrap();
        let grid = crate::map::MapGrid::read(&c, &s.header).unwrap();
        let m = crate::world::MissionWorld::read(&c, &grid).unwrap();
        for k in m.kingdoms.iter().filter(|k| k.active && k.ai_mode & 3 != 0) {
            let name = crate::gameplay::ai::script_name(&k.ai_script, &m.game_rules);
            if named.insert(name.to_ascii_uppercase()) {
                lib.compile(name)
                    .unwrap_or_else(|e| panic!("{}: {e}", s.id));
            }
        }
    }
    assert!(named.len() > 1, "{named:?}");
}
