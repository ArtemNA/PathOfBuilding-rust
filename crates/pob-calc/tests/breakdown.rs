//! R8.4 gate: `env.player.breakdown` / `env.minion.breakdown` against LuaJIT, key by key.
//!
//! Corpus `tests/fixtures/breakdown/<slug>.json` (producer `tools/lua-corpus/export-breakdown.ts`): the 6 goldens, R4-synth
//! (`defence-synth`), R4-synth-2 (`defence-synth2`) and the 20 R7-synth scenarios of the R8.1 display fixtures. Per actor and key the
//! fixture holds the writers (`<file>:<line>` of every `breakdown[key] = ...`), `snap` (the table at `calcs.triggers`, i.e. after perform +
//! defence + estimations), `final` (after `BuildOutput`), `class` (`ported` / `R8.5` / `Calcs`) and `gate` = the value the R8.4 modules leave
//! (`snap`, or `final` for a ported key written after triggers -- `GemLevel`/`GemQuality`, CalcPerform.lua:3948/:3971).
//!
//! Rule: Rust's breakdown after `calculate_with` (raw XML, as the docker probe read it) has exactly the keys that have a `gate`, each equal
//! to it (strings byte for byte, numbers `%.17g`, structure). Keys LuaJIT writes only from offence / triggers / mirages (or changes after
//! the snapshot) are R8.5 and counted apart; `Calcs` (Calcs.lua SkillBuffs/SkillDebuffs) likewise. Nothing is tolerated.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_calc::breakdown::{BTable, BVal, Breakdown};
use pob_calc::{CalculateOptions, CalculateResult, calculate_with};
use pob_lua::format_g;
use serde_json::{Map, Value as Json, json};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn scenarios() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(fixtures().join("breakdown"))
        .expect("breakdown fixtures")
        .filter_map(|e| e.ok()?.file_name().into_string().ok()?.strip_suffix(".json").map(str::to_owned))
        .collect();
    v.sort();
    v
}

fn xml_of(fx: &Json) -> String {
    let name = fx["build"].as_str().expect("build");
    let path = match fx["kind"].as_str().expect("kind") {
        "golden" => pob_golden::registry::find_golden(name).expect("golden").xml_path(),
        "defence-synth" => fixtures().join(format!("defence-synth/{name}.xml")),
        "defence-synth2" => fixtures().join(format!("defence-synth2/{name}.xml")),
        "r81-synth" => fixtures().join(format!("offence-synth/{name}.xml")),
        k => panic!("unknown kind {k}"),
    };
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The probe's serialization of a Lua value (export-breakdown.ts `S`).
pub fn ser(v: &BVal) -> Json {
    match v {
        BVal::Str(s) => Json::String(s.clone()),
        BVal::Bool(b) => Json::Bool(*b),
        BVal::Num(n) => json!({ "$n": if n.is_nan() { "nan".to_owned() } else { format_g(*n, 17) } }),
        BVal::Item(name) => json!({ "$item": name }),
        BVal::Fn => json!({ "$fn": 1 }),
        BVal::Table(t) => ser_table(t),
    }
}

fn ser_table(t: &BTable) -> Json {
    let mut o = Map::new();
    if !t.list.is_empty() {
        o.insert("[]".into(), Json::Array(t.list.iter().map(ser).collect()));
    }
    for (k, v) in &t.fields {
        o.insert(k.clone(), ser(v));
    }
    Json::Object(o)
}

/// Canonical JSON (keys sorted) -- export-breakdown.ts `ser` for the string-only rows compared here.
fn canon(v: &Json) -> String {
    match v {
        Json::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|k| format!("{}:{}", Json::String((*k).clone()), canon(&m[*k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Json::Array(a) => format!("[{}]", a.iter().map(canon).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

/// The probe's R13 normalisation (export-breakdown.ts `normValue`), applied to Rust's value: the rows of the `pairs`-ordered tables
/// sorted, a Gem requirement row naming one of an R13 variant set replaced by the set marker (Rust's name must be in the set).
fn normalize(key: &str, v: &mut Json, gem_sets: &[String]) {
    let row_sorted = ["MaximumHitTaken", "BuildDegen", "EnemyDegen"].iter().any(|s| key.ends_with(s)) || key == "TotalDegen";
    let Some(rows) = v.get_mut("rowList").and_then(|r| r.get_mut("[]")).and_then(Json::as_array_mut) else {
        return;
    };
    if row_sorted {
        rows.sort_by_key(canon);
    }
    if key.starts_with("Req") {
        for r in rows.iter_mut() {
            if r["source"] != "Gem" {
                continue;
            }
            let Some(name) = r["sourceName"].as_str() else { continue };
            // `<color><nameSpec> ^7<level>/<quality>`
            let (head, tail) = name.rsplit_once(" ^7").unwrap_or((name, ""));
            let (color, gem) = if head.starts_with("^x") && head.len() >= 8 {
                head.split_at(8)
            } else {
                ("", head)
            };
            if let Some(set) = gem_sets.iter().find(|s| s.split('|').any(|n| n == gem)) {
                r["sourceName"] = Json::String(format!("{color}<R13:{set}> ^7{tail}"));
            }
        }
    }
}

/// The module that writes a key (its first writer): `CalcPerform`, `CalcDefence` (calcs.defence, :1-1673) or `estimations` (:1674-).
fn module_of(writers: &[Json]) -> String {
    let w = writers.first().and_then(Json::as_str).unwrap_or("?");
    let (file, line) = w.rsplit_once(':').unwrap_or((w, "0"));
    let line: u32 = line.parse().unwrap_or(0);
    match file {
        "CalcDefence.lua" if line >= 1674 => "estimations".into(),
        "CalcDefence.lua" => "defence".into(),
        "CalcPerform.lua" => "perform".into(),
        f => f.trim_end_matches(".lua").into(),
    }
}

#[derive(Default)]
struct Tally {
    /// module -> (gate keys, equal)
    by_module: BTreeMap<String, (usize, usize)>,
    r85: usize,
    calcs: usize,
    bad: Vec<String>,
}

fn actor_breakdown(res: &CalculateResult, actor: &str) -> Option<Breakdown> {
    match actor {
        "player" => res.perform.player.breakdown.clone(),
        _ => res.perform.minion.as_ref().and_then(|m| m.borrow().breakdown.clone()),
    }
}

fn check_actor(slug: &str, actor: &str, fx: &Json, rust: Option<Breakdown>, t: &mut Tally) {
    let lua = &fx[actor];
    if lua.is_null() {
        if rust.is_some() {
            t.bad.push(format!("{slug} | {actor} | rust has a breakdown, LuaJIT has no actor"));
        }
        return;
    }
    let Some(rust) = rust else {
        t.bad.push(format!("{slug} | {actor} | rust has no breakdown"));
        return;
    };
    let keys = lua["keys"].as_object().expect("keys");
    let gem_sets: Vec<String> = fx["r13"]["gemSets"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    let rust_map: BTreeMap<&str, Json> = rust
        .entries()
        .map(|(k, v)| {
            let mut j = ser(v);
            normalize(k, &mut j, &gem_sets);
            (k.as_str(), j)
        })
        .collect();
    for (k, e) in keys {
        match e["class"].as_str().expect("class") {
            "R8.5" => t.r85 += 1,
            "Calcs" => t.calcs += 1,
            _ => {}
        }
        let Some(gate) = e.get("gate") else {
            if rust_map.contains_key(k.as_str()) {
                t.bad.push(format!(
                    "{slug} | {actor}.{k} | rust writes a key LuaJIT's ported modules do not (class {})",
                    e["class"]
                ));
            }
            continue;
        };
        let m = t.by_module.entry(module_of(e["writers"].as_array().expect("writers"))).or_default();
        m.0 += 1;
        match rust_map.get(k.as_str()) {
            Some(r) if r == gate => m.1 += 1,
            Some(r) => t.bad.push(format!("{slug} | {actor}.{k} |\n    rust   {r}\n    luajit {gate}")),
            None => t.bad.push(format!("{slug} | {actor}.{k} | rust nil, luajit {gate}")),
        }
    }
    for k in rust_map.keys() {
        if !keys.contains_key(*k) {
            t.bad.push(format!(
                "{slug} | {actor}.{k} | rust writes a key LuaJIT never has: {}",
                rust_map[k]
            ));
        }
    }
}

#[test]
fn breakdown_matches_luajit() {
    let list = scenarios();
    assert!(list.len() >= 136, "breakdown fixtures: {} scenarios", list.len());
    let mut total = Tally::default();
    let filter = std::env::var("BD_FILTER").ok();
    println!("scenario -> gate keys / equal / R8.5 / Calcs / divergences");
    for slug in &list {
        if filter.as_deref().is_some_and(|f| !slug.contains(f)) {
            continue;
        }
        let fx: Json = serde_json::from_str(&std::fs::read_to_string(fixtures().join(format!("breakdown/{slug}.json"))).expect("fixture"))
            .expect("json");
        let res = calculate_with(&xml_of(&fx), CalculateOptions::default()).unwrap_or_else(|e| panic!("{slug}: {e}"));
        let mut t = Tally::default();
        for actor in ["player", "minion"] {
            check_actor(slug, actor, &fx, actor_breakdown(&res, actor), &mut t);
        }
        let (g, eq) = t.by_module.values().fold((0, 0), |a, v| (a.0 + v.0, a.1 + v.1));
        println!("{slug:<60} {g:>4} {eq:>4} {:>4} {:>3} {}", t.r85, t.calcs, t.bad.len());
        for (m, v) in t.by_module {
            let e = total.by_module.entry(m).or_default();
            e.0 += v.0;
            e.1 += v.1;
        }
        total.r85 += t.r85;
        total.calcs += t.calcs;
        total.bad.extend(t.bad);
    }
    for (m, (g, eq)) in &total.by_module {
        println!("module {m:<12} gate keys {g:>6}, equal {eq:>6}");
    }
    let (g, eq) = total.by_module.values().fold((0, 0), |a, v| (a.0 + v.0, a.1 + v.1));
    println!(
        "TOTAL {} scenarios: gate keys {g}, equal {eq}, R8.5 keys {}, Calcs keys {}, divergences {}",
        list.len(),
        total.r85,
        total.calcs,
        total.bad.len()
    );
    for b in total.bad.iter().take(80) {
        println!("  {b}");
    }
    assert!(total.bad.is_empty(), "{} divergences", total.bad.len());
}

/// R8.4 perf: `calculate_with` on the 6 goldens -- `timings.perform_ms` (perform + the buildOutput tail, where the breakdown is built in
/// MAIN) and the whole call, median of 15 runs after 3 warm-ups. Run: `cargo test -p pob-calc --release --test breakdown -- --ignored --nocapture`.
#[test]
#[ignore = "perf"]
fn perf_build_output() {
    for g in pob_golden::registry::GOLDEN_BUILDS {
        let xml = std::fs::read_to_string(g.xml_path()).expect("xml");
        let (mut perf, mut total) = (Vec::new(), Vec::new());
        for i in 0..18 {
            let t0 = std::time::Instant::now();
            let r = calculate_with(&xml, CalculateOptions::default()).expect("calc");
            let el = t0.elapsed().as_secs_f64() * 1000.0;
            if i >= 3 {
                perf.push(r.timings.perform_ms);
                total.push(el);
            }
        }
        let med = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        println!(
            "PERF {:<45} perform+tail {:>8.3} ms   calculate {:>8.3} ms",
            g.name,
            med(&mut perf),
            med(&mut total)
        );
    }
}
