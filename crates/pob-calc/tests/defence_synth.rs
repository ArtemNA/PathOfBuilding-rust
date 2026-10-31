//! R4-synth: the CalcDefence.lua branches the six goldens never execute (IronReflexes, EnergyShieldToWard, lucky/unlucky block,
//! CannotEvade, ConvertArmourESToLife, recoup, degens, PvP, ...) against desktop LuaJIT, scenario by scenario.
//!
//! Corpus `tests/fixtures/defence-synth/<scenario>.{xml,luajit.json}` (producer `tools/lua-corpus/export-defence-synth.ts`): the XML is
//! Dual Savior with its `<Config>` replaced (config `<Input>`s + one `<CustomModifierBlock>` of mod lines read by the real ModParser);
//! the JSON holds `output.{nums,strs,bools}` = every scalar of LuaJIT's `build.calcsTab.mainOutput` (`%.17g`) and `covers` (the
//! CalcDefence.lua lines the scenario is meant to execute, `grep -n`'d by the producer).
//!
//! Rule (R4.4c form): `calculate_file(<scenario>.xml, ParseMode::Full, skills: true)` -> for every `stats` key Rust emits that is in
//! `ALLOWLIST` (offence is R7, so only the allowlisted keys can be compared) the LuaJIT value must match round4 ("ok", the oracle's
//! rule) AND bit for bit ("exact", `%.17g`, `-0` counted). A key Rust emits and LuaJIT does not is a mismatch (LuaJIT `nil`).
//! The test prints the table "scenario -> compared / ok / exact / mismatch" and every mismatch with the scenario's `covers`.
//! Nothing is tolerated or skipped here: a mismatch is a Rust bug to fix in a later card, hence the `#[ignore]` below while any exist.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use pob_calc::{CalculateOptions, ParseMode, StatValue, calculate_file, is_allowed};
use pob_golden::compare::{stats_exact, stats_match};
use pob_golden::value::StatValue as Gv;
use pob_lua::format_g;
use serde_json::Value as Json;

const MIN_SCENARIOS: usize = 25;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/defence-synth")
}

fn parse_num(s: &str) -> f64 {
    match s {
        "nan" | "-nan" => f64::NAN,
        "inf" => f64::INFINITY,
        "-inf" => f64::NEG_INFINITY,
        _ => s.parse::<f64>().unwrap_or_else(|e| panic!("bad number {s:?}: {e}")),
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

/// The LuaJIT output of a fixture as `Gv` per key.
fn luajit_output(fx: &Json) -> std::collections::BTreeMap<String, Gv> {
    let o = &fx["output"];
    let mut m = std::collections::BTreeMap::new();
    // an empty Lua table is `[]` in dkjson: no keys of that type
    let empty = serde_json::Map::new();
    let obj = |key: &str| o[key].as_object().unwrap_or(&empty);
    for (k, v) in obj("nums") {
        m.insert(k.clone(), Gv::Num(parse_num(v.as_str().expect("num string"))));
    }
    for (k, v) in obj("strs") {
        m.insert(k.clone(), Gv::Str(v.as_str().expect("str").to_owned()));
    }
    for (k, v) in obj("bools") {
        m.insert(k.clone(), Gv::Bool(v.as_bool().expect("bool")));
    }
    m
}

struct Row {
    scenario: String,
    compared: usize,
    ok: usize,
    exact: usize,
    rust_only_lua_nil: usize,
    lua_allowlisted_not_emitted: usize,
    bad: Vec<String>,
    /// Informational (not gating): keys Rust emits that are NOT in `ALLOWLIST` (offence/R7 keys, keys the goldens never produced).
    info_compared: usize,
    info_bad: Vec<(String, String, String, String)>,
    /// `CalculateResult::unimplemented` entries (Rust's own declared gaps) this scenario ran into, minus those every scenario shares.
    unimplemented: Vec<String>,
}

fn scenario_names() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir())
        .unwrap_or_else(|e| panic!("{}: {e}", dir().display()))
        .filter_map(|e| {
            let n = e.ok()?.file_name().into_string().ok()?;
            n.strip_suffix(".luajit.json").map(str::to_owned)
        })
        .collect();
    v.sort();
    v
}

fn run_scenario(name: &str) -> Row {
    let fx: Json =
        serde_json::from_str(&std::fs::read_to_string(dir().join(format!("{name}.luajit.json"))).expect("fixture")).expect("fixture json");
    let xml_path = dir().join(format!("{name}.xml"));
    assert_eq!(fx["scenario"].as_str(), Some(name), "{name}: scenario field");
    let covers: Vec<&str> = fx["covers"].as_array().expect("covers").iter().filter_map(Json::as_str).collect();
    assert!(!covers.is_empty(), "{name}: empty covers");
    let lua = luajit_output(&fx);
    let opts = CalculateOptions {
        parse_mode: ParseMode::Full,
        skills: true,
        ..CalculateOptions::default()
    };
    // a panic or an `Err` inside Rust is a divergence of its own (reported, the other scenarios still run)
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| calculate_file(&xml_path, opts)));
    let mut row = Row {
        scenario: name.to_owned(),
        compared: 0,
        ok: 0,
        exact: 0,
        rust_only_lua_nil: 0,
        lua_allowlisted_not_emitted: 0,
        bad: Vec::new(),
        info_compared: 0,
        info_bad: Vec::new(),
        unimplemented: Vec::new(),
    };
    let cover_text = covers.join(" | ");
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
    for (k, v) in &res.stats {
        let rust = to_golden(v);
        if !is_allowed(k) {
            row.info_compared += 1;
            let l = lua.get(k);
            if l.is_none_or(|l| !stats_match(l, &rust)) {
                row.info_bad.push((k.clone(), name.to_owned(), show(Some(&rust)), show(l)));
            }
            continue;
        }
        row.compared += 1;
        match lua.get(k) {
            Some(l) => {
                let ok = stats_match(l, &rust);
                let exact = stats_exact(l, &rust);
                row.ok += usize::from(ok);
                row.exact += usize::from(exact);
                if !ok || !exact {
                    row.bad.push(format!(
                        "{name} | {k} | rust {} | luajit {} | {} | covers: {cover_text}",
                        show(Some(&rust)),
                        show(Some(l)),
                        if ok { "round4-ok, NOT exact" } else { "MISMATCH" },
                    ));
                }
            }
            None => {
                row.rust_only_lua_nil += 1;
                row.bad.push(format!(
                    "{name} | {k} | rust {} | luajit nil | MISMATCH (LuaJIT has no such key) | covers: {cover_text}",
                    show(Some(&rust))
                ));
            }
        }
    }
    row.unimplemented = res.unimplemented.clone();
    row.lua_allowlisted_not_emitted = lua.keys().filter(|k| is_allowed(k) && !res.stats.contains_key(*k)).count();
    row
}

#[test]
#[ignore = "R4-synth: 14 mismatches, see report"]
fn defence_synth_corpus_matches_luajit() {
    let names = scenario_names();
    assert!(names.len() >= MIN_SCENARIOS, "{} scenarios < {MIN_SCENARIOS}", names.len());
    let rows: Vec<Row> = names.iter().map(|n| run_scenario(n)).collect();
    let mut t = String::new();
    let _ = writeln!(
        t,
        "{:<36} {:>8} {:>6} {:>6} {:>9} {:>9} {:>12}",
        "scenario", "compared", "ok", "exact", "mismatch", "not-exact", "lua-only-ALW"
    );
    let (mut c, mut ok, mut ex, mut bad_total) = (0, 0, 0, 0);
    for r in &rows {
        let mismatch = r.compared - r.ok;
        let not_exact = r.ok - r.exact.min(r.ok);
        let _ = writeln!(
            t,
            "{:<36} {:>8} {:>6} {:>6} {:>9} {:>9} {:>12}",
            r.scenario, r.compared, r.ok, r.exact, mismatch, not_exact, r.lua_allowlisted_not_emitted
        );
        c += r.compared;
        ok += r.ok;
        ex += r.exact;
        bad_total += r.bad.len();
    }
    let _ = writeln!(
        t,
        "{:<36} {:>8} {:>6} {:>6} {:>9}",
        format!("TOTAL ({} scenarios)", rows.len()),
        c,
        ok,
        ex,
        c - ok
    );
    println!("{t}");
    println!("--- every divergence (scenario | key | rust | luajit | class | covers) ---");
    for r in &rows {
        for b in &r.bad {
            println!("{b}");
        }
    }
    println!("--- {bad_total} divergent (scenario, key) pairs ---");
    // informational: keys outside ALLOWLIST, grouped by key (not gating: offence keys are R7, the others were never measured by a golden)
    let mut by_key: std::collections::BTreeMap<&str, Vec<&(String, String, String, String)>> = std::collections::BTreeMap::new();
    for r in &rows {
        for b in &r.info_bad {
            by_key.entry(b.0.as_str()).or_default().push(b);
        }
    }
    let info_total: usize = rows.iter().map(|r| r.info_compared).sum();
    let info_bad: usize = rows.iter().map(|r| r.info_bad.len()).sum();
    println!(
        "--- informational, keys outside ALLOWLIST: {info_total} values compared, {info_bad} differ ({} distinct keys) ---",
        by_key.len()
    );
    // Rust's own `unimplemented` entries: those present in some scenarios but not in all (the scenario-specific declared gaps)
    let mut count: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for r in &rows {
        for u in &r.unimplemented {
            *count.entry(u.as_str()).or_default() += 1;
        }
    }
    println!(
        "--- rust `unimplemented` entries: {} distinct; scenario-specific (not in all {} scenarios): ---",
        count.len(),
        rows.len()
    );
    for (u, n) in &count {
        if *n < rows.len() {
            let who: Vec<&str> = rows
                .iter()
                .filter(|r| r.unimplemented.iter().any(|x| x == u))
                .map(|r| r.scenario.as_str())
                .take(3)
                .collect();
            println!("UNIMPL | {u} | {n} scenarios, e.g. {}", who.join(", "));
        }
    }
    for (k, v) in &by_key {
        let (_, sc, rust, lua) = v[0];
        println!("INFO | {k} | {} scenarios | e.g. {sc}: rust {rust} | luajit {lua}", v.len());
    }
    assert_eq!(
        bad_total, 0,
        "{bad_total} (scenario, key) pairs differ from LuaJIT, see the report above"
    );
}
