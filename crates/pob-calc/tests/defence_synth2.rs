//! R4-synth-2: the CalcPerform.lua branches the six goldens never execute (reservations: BloodMagic / `*Forced` / cost-as-reservation / mines /
//! Blood Sacrament / `GrantReserved*AsAura`; stacking Shock/Scorch, Bonechill, Elemental Equilibrium by tree version, guards, Choir of the Storm,
//! tinctures over the limit) against desktop LuaJIT, scenario by scenario.
//!
//! Corpus `tests/fixtures/defence-synth2/<scenario>.{xml,luajit.json}` (producer `tools/lua-corpus/export-defence-synth2.ts`): the XML is Dual
//! Savior with `<Config>` replaced (+ extra gear / socket groups / tree version); the JSON holds `output.{nums,strs,bools}` (the scalars of
//! `build.calcsTab.mainOutput`, `%.17g`), `checkpoints.CP1..CP4.{player,enemy}` (a FRESH `initEnv` + `perform`, wrappers as the R6.1 probe: modDB as
//! a multiset of `formatSourceMod`, conditions, multipliers, output) and `covers` (the CalcPerform.lua lines the scenario executes, `grep -n`'d).
//!
//! Rule: CP1-CP4 EXACTLY as `perform_cp.rs` (same snapshot, same single pinned class [`TABLE_OUTPUT`]); the final output as `defence_synth.rs`
//! (`calculate_file`, every emitted key in `ALLOWLIST` must match round4 AND bit for bit, a key LuaJIT lacks is a mismatch). Nothing is
//! tolerated or skipped: a mismatch is a Rust bug, listed in full. A panic / `Err` inside Rust is a divergence of its own (the others still run).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::{
    ActorState, CalculateOptions, Checkpoint, Env, InitEnvOptions, OutValue, ParseMode, calculate_file, init_env_with, is_allowed,
    perform_with,
};
use pob_golden::compare::{stats_exact, stats_match};
use pob_golden::value::StatValue as Gv;
use pob_lua::format_g;
use pob_mods::format::format_source_mod;
use pob_mods::{StoreKind, StoreRef};
use serde_json::{Map, Value as Json, json};

const MIN_SCENARIOS: usize = 30;
const CPS: [&str; 4] = ["CP1", "CP2", "CP3", "CP4"];
/// Proven class `output-table-value` (perform_cp.rs, R6-ARCH §6): CalcPerform.lua:2156 stores a requirement-source TABLE in `output["Req<Attr>Item"]`.
const TABLE_OUTPUT: [&str; 4] = ["ReqStrItem", "ReqDexItem", "ReqIntItem", "ReqOmniItem"];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/defence-synth2")
}

fn n17(x: f64) -> Json {
    Json::String(format_g(x, 17))
}

fn snap(db: &StoreRef, st: &ActorState) -> Json {
    let s = db.borrow();
    let StoreKind::Db(d) = &s.kind else { panic!("not a ModDB") };
    let mut mods: Vec<String> = d.mods.values().flat_map(|b| b.iter().map(|m| format_source_mod(m))).collect();
    mods.sort();
    let conditions: Map<String, Json> = s.conditions.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    let multipliers: Map<String, Json> = s.multipliers.iter().map(|(k, v)| (k.to_string(), n17(*v))).collect();
    let output: Map<String, Json> = st
        .output
        .iter()
        .map(|(k, v)| {
            let j = match v {
                OutValue::Num(x) => n17(*x),
                OutValue::Str(t) => json!(t.to_string()),
                OutValue::Bool(b) => json!(b),
            };
            (k.to_string(), j)
        })
        .collect();
    json!({ "modDB": mods, "modCount": n17(mods.len() as f64), "conditions": conditions, "multipliers": multipliers, "output": output })
}

/// Rust's CP1..CP4 on a FRESH `initEnv` + `perform` (the hook of `perform_with`, as `perform_cp.rs`).
fn run_cps(xml: &Path) -> BTreeMap<&'static str, Json> {
    let model = pob_build::load_build_xml(&std::fs::read_to_string(xml).expect("xml")).expect("xml parses");
    let opts = InitEnvOptions {
        parse_mode: ParseMode::Full,
        ..InitEnvOptions::default()
    };
    let mut env: Env = init_env_with(Rc::new(model), opts, true).unwrap_or_else(|e| panic!("initEnv: {e}"));
    let mut cps: BTreeMap<&'static str, Json> = BTreeMap::new();
    let mut hook = |cp: Checkpoint, env: &Env, player: &ActorState, enemy: &ActorState| {
        let key = match cp {
            Checkpoint::Cp1 => "CP1",
            Checkpoint::Cp2 => "CP2",
            Checkpoint::Cp3 => "CP3",
            Checkpoint::Cp4 => "CP4",
            _ => return, // the offence checkpoints (R7) are gated by `offence_cp.rs`
        };
        let (pdb, edb) = (env.player.borrow().mod_db.clone(), env.enemy.borrow().mod_db.clone());
        cps.entry(key)
            .or_insert_with(|| json!({ "player": snap(&pdb, player), "enemy": snap(&edb, enemy) }));
    };
    let _ = perform_with(&mut env, Some(pob_calc::defence), Some(pob_calc::estimations), Some(&mut hook));
    cps
}

/// Every difference between LuaJIT's snapshot of one actor and Rust's, as text lines.
fn diff_actor(who: &str, lj: &Json, rust: &Json) -> Vec<String> {
    let mut out = Vec::new();
    let mut want: BTreeMap<String, i64> = BTreeMap::new();
    for m in lj["modDB"].as_array().expect("modDB") {
        *want.entry(m.as_str().unwrap().to_owned()).or_default() += 1;
    }
    for m in rust["modDB"].as_array().expect("modDB") {
        *want.entry(m.as_str().unwrap().to_owned()).or_default() -= 1;
    }
    for (m, c) in &want {
        if *c != 0 {
            out.push(format!(
                "{who}.modDB: only {} x{}: {m}",
                if *c > 0 { "LuaJIT" } else { "Rust" },
                c.abs()
            ));
        }
    }
    if lj["modCount"] != rust["modCount"] {
        out.push(format!("{who}.modCount: lj {} rust {}", lj["modCount"], rust["modCount"]));
    }
    for part in ["conditions", "multipliers", "output"] {
        let (w, g) = (lj[part].as_object().expect(part), rust[part].as_object().expect(part));
        for k in w.keys().chain(g.keys()).collect::<BTreeSet<_>>() {
            let (a, b) = (w.get(k).unwrap_or(&Json::Null), g.get(k).unwrap_or(&Json::Null));
            if a != b {
                out.push(format!("{who}.{part}.{k}: lj {a} rust {b}"));
            }
        }
    }
    out
}

/// Removes the pinned `output-table-value` differences (`player.output.Req<Attr>Item: lj {table} rust null`); returns how many.
fn strip_table_output(d: &mut Vec<String>) -> usize {
    let before = d.len();
    d.retain(|x| {
        !TABLE_OUTPUT
            .iter()
            .any(|k| x.starts_with(&format!("player.output.{k}: lj {{")) && x.ends_with(" rust null"))
    });
    before - d.len()
}

fn parse_num(s: &str) -> f64 {
    match s {
        "nan" | "-nan" => f64::NAN,
        "inf" => f64::INFINITY,
        "-inf" => f64::NEG_INFINITY,
        _ => s.parse::<f64>().unwrap_or_else(|e| panic!("bad number {s:?}: {e}")),
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

/// Final output (`calculate_file`, ALLOWLIST keys) vs `output.{nums,strs,bools}`: (compared, divergences).
fn final_output(xml: &Path, fx: &Json) -> (usize, Vec<String>) {
    let mut lua: BTreeMap<String, Gv> = BTreeMap::new();
    let empty = Map::new();
    let obj = |k: &str| fx["output"][k].as_object().unwrap_or(&empty);
    lua.extend(
        obj("nums")
            .iter()
            .map(|(k, v)| (k.clone(), Gv::Num(parse_num(v.as_str().expect("num"))))),
    );
    lua.extend(
        obj("strs")
            .iter()
            .map(|(k, v)| (k.clone(), Gv::Str(v.as_str().expect("str").to_owned()))),
    );
    lua.extend(obj("bools").iter().map(|(k, v)| (k.clone(), Gv::Bool(v.as_bool().expect("bool")))));
    let opts = CalculateOptions {
        parse_mode: ParseMode::Full,
        skills: true,
        ..CalculateOptions::default()
    };
    let res = match calculate_file(xml, opts) {
        Ok(r) => r,
        Err(e) => return (0, vec![format!("<final output>: rust Err: {e}")]),
    };
    let (mut compared, mut bad) = (0, Vec::new());
    for (k, v) in res.stats.iter().filter(|(k, _)| is_allowed(k)) {
        let rust = match v {
            pob_calc::StatValue::Num(n) => Gv::Num(*n),
            pob_calc::StatValue::Str(s) => Gv::Str(s.clone()),
            pob_calc::StatValue::Bool(b) => Gv::Bool(*b),
        };
        compared += 1;
        match lua.get(k) {
            Some(l) if stats_match(l, &rust) && stats_exact(l, &rust) => {}
            Some(l) => bad.push(format!(
                "output.{k}: rust {} luajit {}{}",
                show(Some(&rust)),
                show(Some(l)),
                if stats_match(l, &rust) { " (round4-ok, NOT exact)" } else { "" }
            )),
            None => bad.push(format!("output.{k}: rust {} luajit nil", show(Some(&rust)))),
        }
    }
    (compared, bad)
}

struct Row {
    name: String,
    cp: [usize; 4],
    pinned: usize,
    compared: usize,
    final_bad: usize,
    lines: Vec<String>,
}

fn run_scenario(name: &str) -> Row {
    let fx: Json =
        serde_json::from_str(&std::fs::read_to_string(dir().join(format!("{name}.luajit.json"))).expect("fixture")).expect("fixture json");
    assert_eq!(fx["scenario"].as_str(), Some(name), "{name}: scenario field");
    let xml = dir().join(format!("{name}.xml"));
    let mut row = Row {
        name: name.to_owned(),
        cp: [0; 4],
        pinned: 0,
        compared: 0,
        final_bad: 0,
        lines: Vec::new(),
    };
    let guard = |what: &str, r: std::thread::Result<()>, lines: &mut Vec<String>| {
        if let Err(p) = r {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default();
            lines.push(format!("{name} | {what} | rust PANIC: {msg}"));
        }
    };
    let mut lines = Vec::new();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let cps = run_cps(&xml);
        for (i, cp) in CPS.iter().enumerate() {
            let (Some(w), Some(r)) = (fx["checkpoints"].get(cp), cps.get(cp)) else {
                row.cp[i] += 1;
                lines.push(format!(
                    "{name} | {cp} | checkpoint missing (lj {}, rust {})",
                    fx["checkpoints"].get(cp).is_some(),
                    cps.contains_key(cp)
                ));
                continue;
            };
            let mut d = diff_actor("player", &w["player"], &r["player"]);
            d.extend(diff_actor("enemy", &w["enemy"], &r["enemy"]));
            row.pinned += strip_table_output(&mut d);
            row.cp[i] = d.len();
            lines.extend(d.into_iter().map(|x| format!("{name} | {cp} | {x}")));
        }
    }));
    guard("CP1-CP4", r, &mut lines);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (compared, bad) = final_output(&xml, &fx);
        row.compared = compared;
        row.final_bad = bad.len();
        lines.extend(bad.into_iter().map(|x| format!("{name} | final | {x}")));
    }));
    guard("final output", r, &mut lines);
    row.lines = lines;
    row
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

/// Corpus shape (always on): >= 30 scenarios, each with `covers`, all four checkpoints for both actors and a non-empty final output.
#[test]
fn defence_synth2_corpus_shape() {
    let names = scenario_names();
    assert!(names.len() >= MIN_SCENARIOS, "{} scenarios < {MIN_SCENARIOS}", names.len());
    for n in &names {
        let fx: Json = serde_json::from_str(&std::fs::read_to_string(dir().join(format!("{n}.luajit.json"))).unwrap()).unwrap();
        assert!(dir().join(format!("{n}.xml")).exists(), "{n}: xml");
        assert!(!fx["covers"].as_array().expect("covers").is_empty(), "{n}: empty covers");
        for cp in CPS {
            assert!(
                fx["checkpoints"][cp]["player"]["modDB"].is_array() && fx["checkpoints"][cp]["enemy"]["modDB"].is_array(),
                "{n}: {cp}"
            );
        }
        assert!(fx["output"]["nums"].as_object().is_some_and(|o| !o.is_empty()), "{n}: output");
    }
}

#[test]
fn defence_synth2_corpus_matches_luajit() {
    let names = scenario_names();
    assert!(names.len() >= MIN_SCENARIOS, "{} scenarios < {MIN_SCENARIOS}", names.len());
    let rows: Vec<Row> = names.iter().map(|n| run_scenario(n)).collect();
    let mut t = String::new();
    let _ = writeln!(
        t,
        "{:<34} {:>5} {:>5} {:>5} {:>5} {:>7} {:>9} {:>7}",
        "scenario", "CP1", "CP2", "CP3", "CP4", "pinned", "final cmp", "final !"
    );
    for r in &rows {
        let _ = writeln!(
            t,
            "{:<34} {:>5} {:>5} {:>5} {:>5} {:>7} {:>9} {:>7}",
            r.name, r.cp[0], r.cp[1], r.cp[2], r.cp[3], r.pinned, r.compared, r.final_bad
        );
    }
    let total: usize = rows.iter().map(|r| r.lines.len()).sum();
    let clean = rows.iter().filter(|r| r.lines.is_empty()).count();
    let _ = writeln!(
        t,
        "TOTAL {} scenarios, {clean} without divergences, {total} divergent entries",
        rows.len()
    );
    println!("{t}--- every divergence (scenario | checkpoint | difference) ---");
    for r in &rows {
        r.lines.iter().for_each(|l| println!("{l}"));
    }
    assert_eq!(
        total,
        0,
        "{total} divergences from LuaJIT in {} scenarios, see the report above",
        rows.len() - clean
    );
}
