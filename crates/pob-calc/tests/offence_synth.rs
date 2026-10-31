//! R7-synth: the offence branches the six goldens never execute (FullDPS groups, a minion as the main skill, per-skill functions of
//! `Data/Skills/*.lua`, triggers, mirages) against desktop-path LuaJIT, scenario by scenario.
//!
//! Corpus `tests/fixtures/offence-synth/<scenario>.{xml,luajit.json}` (producer `tools/lua-corpus/export-offence-synth.ts`): the XML is one
//! of the golden builds with `<Skills>` replaced (+ optional gear / config inputs / one `<CustomModifierBlock>`); the JSON holds the full MAIN
//! `output` as LuaJIT leaves it after `wipeGlobalCache` + `calcs.buildOutput(build, "MAIN")` (`output.{nums,strs,bools}` = every scalar as `%.17g`,
//! `output.tables` = structural dumps), `fullDPS` (the `calcFullDPS` result), `minion` (the minion actor's output), `mainSkill`, and `covers`
//! (the lines of CalcOffence/CalcTriggers/CalcMirages/Calcs.lua and the `Data/Skills` functions the scenario really executes).
//!
//! `offence_synth_matches_luajit` (ignored until R7 ports offence): `calculate_file(<scenario>.xml, Full, skills: true)`; EVERY scalar key of the LuaJIT
//! output is compared with Rust's `stats` (round4 = "ok"; bit for bit = "exact"); a key LuaJIT has and Rust lacks is "missing", the reverse "extra".
//! Prints "scenario -> compared / ok / exact / mismatch / missing / extra", per-category totals and every divergence. Table-valued keys
//! (`SkillDPS`, `Minion`, ...) are counted, not compared (Rust's `stats` are the oracle's scalar-only flatten).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use pob_calc::{CalculateOptions, ParseMode, StatValue, calculate_file};
use pob_golden::compare::{stats_exact, stats_match};
use pob_golden::value::StatValue as Gv;
use pob_lua::format_g;
use serde_json::Value as Json;

const MIN_SCENARIOS: usize = 40;
const MIN_SKILL_FNS: usize = 60;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/offence-synth")
}

/// A LuaJIT number as the oracle wire carries it (`engine_outputJSON`): NaN and the infinities become the strings `tostring` renders.
fn lua_num(s: &str) -> Gv {
    match s {
        "nan" | "-nan" | "inf" | "-inf" => Gv::Str(s.to_owned()),
        _ => Gv::Num(s.parse::<f64>().unwrap_or_else(|e| panic!("bad number {s:?}: {e}"))),
    }
}

fn to_golden(v: &StatValue) -> Gv {
    match v {
        StatValue::Num(n) => Gv::Num(*n),
        StatValue::Str(s) => Gv::Str(s.clone()),
        StatValue::Bool(b) => Gv::Bool(*b),
    }
}

fn show(v: Option<&Gv>) -> String {
    match v {
        None => "nil".to_owned(),
        Some(Gv::Num(n)) => format_g(*n, 17),
        Some(Gv::Str(s)) => format!("{s:?}"),
        Some(Gv::Bool(b)) => b.to_string(),
    }
}

/// The scalar part of a LuaJIT output dump (`nums`/`strs`/`bools`); an empty Lua table is `[]` in dkjson, i.e. no keys of that type.
fn scalars(dump: &Json) -> BTreeMap<String, Gv> {
    let empty = serde_json::Map::new();
    let obj = |k: &str| dump[k].as_object().unwrap_or(&empty);
    let mut m = BTreeMap::new();
    for (k, v) in obj("nums") {
        m.insert(k.clone(), lua_num(v.as_str().expect("num string")));
    }
    for (k, v) in obj("strs") {
        m.insert(k.clone(), Gv::Str(v.as_str().expect("str").to_owned()));
    }
    for (k, v) in obj("bools") {
        m.insert(k.clone(), Gv::Bool(v.as_bool().expect("bool")));
    }
    m
}

fn scenario_names() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir())
        .unwrap_or_else(|e| panic!("{}: {e}", dir().display()))
        .filter_map(|e| {
            e.ok()?
                .file_name()
                .into_string()
                .ok()?
                .strip_suffix(".luajit.json")
                .map(str::to_owned)
        })
        .collect();
    v.sort();
    v
}

fn fixture(name: &str) -> Json {
    let text = std::fs::read_to_string(dir().join(format!("{name}.luajit.json"))).expect("fixture");
    serde_json::from_str(&text).expect("fixture json")
}

#[derive(Default)]
struct Row {
    scenario: String,
    category: String,
    compared: usize,
    ok: usize,
    exact: usize,
    missing: usize,
    extra: usize,
    tables_lua: usize,
    minion_keys: usize,
    bad: Vec<String>,
}

fn run_scenario(name: &str) -> Row {
    let fx = fixture(name);
    assert_eq!(fx["scenario"].as_str(), Some(name), "{name}: scenario field");
    let lua = scalars(&fx["output"]);
    let covers: Vec<&str> = fx["covers"].as_array().expect("covers").iter().filter_map(Json::as_str).collect();
    let cover_text = covers.iter().take(3).copied().collect::<Vec<_>>().join(" | ");
    let mut row = Row {
        scenario: name.to_owned(),
        category: fx["category"].as_str().unwrap_or("?").to_owned(),
        tables_lua: fx["output"]["tables"].as_object().map_or(0, |o| o.len()),
        minion_keys: fx["minion"].as_object().map_or(0, |_| scalars(&fx["minion"]).len()),
        ..Row::default()
    };
    let opts = CalculateOptions {
        parse_mode: ParseMode::Full,
        skills: true,
        ..CalculateOptions::default()
    };
    // a panic or an `Err` inside Rust is a divergence of its own (reported, the other scenarios still run)
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        calculate_file(dir().join(format!("{name}.xml")), opts)
    }));
    let res = match outcome {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            row.bad
                .push(format!("{name} | <whole scenario> | rust Err: {e} | covers: {cover_text}"));
            return row;
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default();
            row.bad
                .push(format!("{name} | <whole scenario> | rust PANIC: {msg} | covers: {cover_text}"));
            return row;
        }
    };
    let rust: BTreeMap<&str, Gv> = res.stats.iter().map(|(k, v)| (k.as_str(), to_golden(v))).collect();
    for (k, l) in &lua {
        row.compared += 1;
        match rust.get(k.as_str()) {
            Some(r) => {
                let (ok, exact) = (stats_match(l, r), stats_exact(l, r));
                row.ok += usize::from(ok);
                row.exact += usize::from(exact);
                if !ok || !exact {
                    row.bad.push(format!(
                        "{name} | {k} | rust {} | luajit {} | {} | covers: {cover_text}",
                        show(Some(r)),
                        show(Some(l)),
                        if ok { "round4-ok, NOT exact" } else { "MISMATCH" },
                    ));
                }
            }
            None => {
                row.missing += 1;
                row.bad.push(format!(
                    "{name} | {k} | rust nil | luajit {} | MISSING | covers: {cover_text}",
                    show(Some(l))
                ));
            }
        }
    }
    for (k, r) in &rust {
        if !lua.contains_key(*k) {
            row.extra += 1;
            row.bad.push(format!(
                "{name} | {k} | rust {} | luajit nil | EXTRA | covers: {cover_text}",
                show(Some(r))
            ));
        }
    }
    row
}

/// Fixture sanity (not ignored): the corpus is as large and as varied as the R7-synth card demands.
#[test]
fn offence_synth_corpus_is_wellformed() {
    let names = scenario_names();
    assert!(names.len() >= MIN_SCENARIOS, "{} scenarios < {MIN_SCENARIOS}", names.len());
    let mut cats: BTreeMap<String, usize> = BTreeMap::new();
    let mut fns = std::collections::BTreeSet::new();
    for n in &names {
        let fx = fixture(n);
        assert_eq!(fx["scenario"].as_str(), Some(n.as_str()));
        assert!(dir().join(format!("{n}.xml")).is_file(), "{n}: xml");
        assert!(!fx["covers"].as_array().expect("covers").is_empty(), "{n}: empty covers");
        assert!(fx["output"]["nums"].is_object(), "{n}: output.nums");
        assert!(fx["fullDPS"].is_object(), "{n}: fullDPS");
        *cats.entry(fx["category"].as_str().expect("category").to_owned()).or_default() += 1;
        for f in fx["fnCalled"].as_array().expect("fnCalled") {
            fns.insert(f.as_str().expect("closure path").to_owned());
        }
    }
    println!(
        "{} scenarios by category: {cats:?}; {} distinct Data/Skills functions called",
        names.len(),
        fns.len()
    );
    assert!(fns.len() >= MIN_SKILL_FNS, "{} skill functions < {MIN_SKILL_FNS}", fns.len());
}

#[test]
#[ignore = "R7: offence not ported yet"]
fn offence_synth_matches_luajit() {
    let names = scenario_names();
    assert!(names.len() >= MIN_SCENARIOS, "{} scenarios < {MIN_SCENARIOS}", names.len());
    let rows: Vec<Row> = names.iter().map(|n| run_scenario(n)).collect();
    let mut t = String::new();
    let _ = writeln!(
        t,
        "{:<40} {:<9} {:>8} {:>6} {:>6} {:>9} {:>8} {:>6} {:>7}",
        "scenario", "category", "compared", "ok", "exact", "mismatch", "missing", "extra", "tables"
    );
    let mut by_cat: BTreeMap<&str, [usize; 6]> = BTreeMap::new();
    for r in &rows {
        let mismatch = r.compared - r.ok - r.missing;
        let _ = writeln!(
            t,
            "{:<40} {:<9} {:>8} {:>6} {:>6} {:>9} {:>8} {:>6} {:>7}",
            r.scenario, r.category, r.compared, r.ok, r.exact, mismatch, r.missing, r.extra, r.tables_lua
        );
        let c = by_cat.entry(r.category.as_str()).or_default();
        for (i, v) in [r.compared, r.ok, r.exact, r.missing, r.extra, 1].into_iter().enumerate() {
            c[i] += v;
        }
    }
    println!("{t}");
    println!("--- per category: scenarios / compared / ok / exact / missing / extra ---");
    for (c, v) in &by_cat {
        println!(
            "{c:<10} {:>4} scen {:>7} compared {:>7} ok {:>7} exact {:>7} missing {:>5} extra",
            v[5], v[0], v[1], v[2], v[3], v[4]
        );
    }
    let bad_total: usize = rows.iter().map(|r| r.bad.len()).sum();
    println!("--- every divergence (scenario | key | rust | luajit | class | covers) ---");
    for r in &rows {
        for b in &r.bad {
            println!("{b}");
        }
    }
    println!(
        "--- {bad_total} divergent (scenario, key) pairs; Lua minion scalar keys in the corpus: {} ---",
        rows.iter().map(|r| r.minion_keys).sum::<usize>()
    );
    assert_eq!(
        bad_total, 0,
        "{bad_total} (scenario, key) pairs differ from LuaJIT, see the report above"
    );
}
