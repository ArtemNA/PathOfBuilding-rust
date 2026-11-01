//! R8.6 gate: the CALCS run (`calcsTab.calcsEnv`, Calcs.lua `buildOutput(build, "CALCS")`) against LuaJIT -- `calcsOutput` (every key) and
//! `calcsEnv.{player,minion}.breakdown` (every key).
//!
//! Corpus `crates/pob-engine/tests/fixtures/calcs/<slug>.json` (producer `tools/lua-corpus/export-calcs.ts`): the 6 goldens, the 20 R7-synth
//! scenarios of the R8.1 display set as their XML says (`r81--`), the same with `skill_number = mainSocketGroup` (`main--`: the CALCS run
//! views the scenario's main skill) and 2 goldens x 3 buff modes (`buff-<mode>--`). The probe's path is `CalcsTab:BuildOutput()`: MAIN, then
//! CALCS, then `getMiscCalculator` -- Rust runs the same three on one [`CalcSession`], the XML as raw bytes, the fixture's
//! `calcsInputOverride` written into the model's Calcs inputs first.
//!
//! Compared exactly (canonical JSON, the probe's serialization `S`): scalars (`{"$n": "%.17g"}`, strings, booleans) and the table values
//! (`MainHand` / `OffHand`, `Minion`, `SkillDPS`, the warning lists, `Req<Attr>Item` with `sourceGem` as its `nameSpec`). Breakdown: every
//! key both ways, the R13 normalisation of `export-calcs.ts` (rows of the `pairs`-ordered tables sorted, R13 gem sets, `SkillBuffs` /
//! `SkillDebuffs` rows sorted); a nested hand module is `{}` (the probe's proxy). Nothing is tolerated.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::breakdown::{BTable, BVal, Breakdown};
use pob_calc::{ActorState, CalcMode, CalcSession, CalculateOptions, Env, OutTable, OutValue};
use pob_lua::format_g;
use pob_mods::{Mod, Value};
use serde_json::{Map, Value as Json, json};

fn calcs_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../pob-engine/tests/fixtures/calcs")
}

fn scenarios() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(calcs_fixtures())
        .expect("calcs fixtures")
        .filter_map(|e| e.ok()?.file_name().into_string().ok()?.strip_suffix(".json").map(str::to_owned))
        .filter(|s| !s.starts_with('_'))
        .collect();
    v.sort();
    v
}

pub fn xml_of(fx: &Json) -> String {
    let name = fx["build"].as_str().expect("build");
    let path = match fx["kind"].as_str().expect("kind") {
        "golden" => pob_golden::registry::find_golden(name).expect("golden").xml_path(),
        "r81-synth" => Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/offence-synth/{name}.xml")),
        k => panic!("unknown kind {k}"),
    };
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The session after `CalcsTab:BuildOutput()` (MAIN, CALCS, misc calculator) and the CALCS run.
fn calcs_run(fx: &Json) -> (CalcSession, pob_calc::RunRef) {
    let mut model = pob_build::load_build_xml(&xml_of(fx)).expect("xml");
    if let Some(ov) = fx["calcsInputOverride"].as_object() {
        for (k, v) in ov {
            let iv = match v {
                Json::String(s) => pob_build::InputValue::Str(s.clone()),
                Json::Object(o) => pob_build::InputValue::Num(o["$n"].as_str().expect("$n").parse().expect("number")),
                Json::Bool(b) => pob_build::InputValue::Bool(*b),
                other => panic!("override {k}: {other}"),
            };
            model.calcs_input.insert(k.clone(), iv);
        }
    }
    let session = CalcSession::new(Rc::new(model), CalculateOptions::default());
    session.build_output(CalcMode::Main).expect("MAIN");
    let out = session.build_output(CalcMode::Calcs).expect("CALCS");
    session.get_misc_calculator().expect("misc calculator");
    (session, out.run)
}

fn n(x: f64) -> Json {
    json!({ "$n": if x.is_nan() { "nan".to_owned() } else { format_g(x, 17) } })
}

fn out_value(v: &OutValue) -> Json {
    match v {
        OutValue::Num(x) => n(*x),
        OutValue::Str(s) => Json::String(s.to_string()),
        OutValue::Bool(b) => Json::Bool(*b),
    }
}

fn list(items: Vec<Json>) -> Json {
    if items.is_empty() { json!({}) } else { json!({ "[]": items }) }
}

fn out_table(env: &Env, t: &OutTable) -> Json {
    match t {
        OutTable::ReqSource(r) => {
            let mut o = Map::new();
            for (k, v) in [("Str", r.str_), ("Dex", r.dex), ("Int", r.int)] {
                if let Some(v) = v {
                    o.insert(k.into(), n(v));
                }
            }
            o.insert("source".into(), json!(r.source));
            if let Some(i) = &r.source_item {
                o.insert("sourceItem".into(), json!({ "$item": i.name }));
            }
            if let Some(s) = &r.source_slot {
                o.insert("sourceSlot".into(), json!(s));
            }
            if let Some((g, i)) = r.source_gem {
                let st = env.skills.as_ref().expect("skills");
                let gi = &st.tab.socket_group_list()[g].gem_list[i];
                o.insert(
                    "sourceGem".into(),
                    json!({ "$gem": gi.name_spec.clone().unwrap_or_else(|| "nil".into()) }),
                );
            }
            Json::Object(o)
        }
        OutTable::SkillDps(v) => list(
            v.iter()
                .map(|s| {
                    let mut o = Map::new();
                    o.insert("name".into(), json!(s.name));
                    o.insert("dps".into(), n(s.dps));
                    o.insert("count".into(), n(s.count));
                    for (k, x) in [("trigger", &s.trigger), ("skillPart", &s.skill_part), ("source", &s.source)] {
                        if let Some(x) = x {
                            o.insert(k.into(), json!(x));
                        }
                    }
                    Json::Object(o)
                })
                .collect(),
        ),
        OutTable::Names(v) => list(v.iter().map(|s| json!(s)).collect()),
        OutTable::Minion(m) => actor_output(env, &m.0.borrow()),
    }
}

pub fn actor_output(env: &Env, a: &ActorState) -> Json {
    let mut o: Map<String, Json> = a.output.iter().map(|(k, v)| (k.to_string(), out_value(v))).collect();
    for (k, t) in a.output.tables() {
        o.insert(k.to_string(), out_table(env, t));
    }
    for (i, name) in ["MainHand", "OffHand"].iter().enumerate() {
        if let Some(h) = a.hand_output(i) {
            o.insert(
                (*name).into(),
                Json::Object(h.iter().map(|(k, v)| (k.to_string(), out_value(v))).collect()),
            );
        }
    }
    Json::Object(o)
}

// --- breakdown (export-calcs.ts `S`) ---

fn value(v: &Value) -> Option<Json> {
    Some(match v {
        Value::Nil => return None,
        Value::Bool(b) => Json::Bool(*b),
        Value::Num(x) => n(*x),
        Value::Str(s) => Json::String(s.to_string()),
        Value::Mod(m) => ser_mod(m),
        Value::Func(_) => json!({ "$fn": 1 }),
        Value::Table(t) => {
            let mut o = Map::new();
            let arr: Vec<Json> = t.array.iter().filter_map(value).collect();
            if !arr.is_empty() {
                o.insert("[]".into(), Json::Array(arr));
            }
            for (k, x) in &t.hash {
                if let Some(j) = value(x) {
                    o.insert(k.to_string(), j);
                }
            }
            Json::Object(o)
        }
    })
}

/// export-calcs.ts `S`: a table is written as a mod (`$mod`) when its `type` is one of `MODTYPES`; any other mod (`DUMMY`, `CHANCE`) is a
/// plain table there -- its tags as the array part, `flags` / `keywordFlags` as numbers.
fn ser_mod(m: &Mod) -> Json {
    let probe_mod = matches!(
        m.ty.map(|t| t.as_str()),
        Some("BASE" | "INC" | "MORE" | "FLAG" | "LIST" | "OVERRIDE" | "MAX" | "MIN")
    );
    let mut o = Map::new();
    o.insert("name".into(), json!(m.name.as_ref()));
    if let Some(t) = m.ty {
        o.insert("type".into(), json!(t.as_str()));
    }
    if let Some(s) = &m.source {
        o.insert("source".into(), json!(s.as_ref()));
    }
    if let Some(v) = value(&m.value) {
        o.insert("value".into(), v);
    }
    o.insert("flags".into(), json!(format_g(f64::from(m.flags), 17)));
    o.insert("keywordFlags".into(), json!(format_g(f64::from(m.keyword_flags), 17)));
    if !m.tags.is_empty() {
        let tags: Vec<Json> = m
            .tags
            .iter()
            .map(|t| {
                let tab = t.table();
                let mut to = Map::new();
                let arr: Vec<Json> = tab.array.iter().filter_map(value).collect();
                if !arr.is_empty() {
                    to.insert("[]".into(), Json::Array(arr));
                }
                let mut keys: Vec<String> = tab.hash.keys().map(|k| k.to_string()).collect();
                for extra in ["div", "warcryPowerBonus"] {
                    if !keys.iter().any(|k| k == extra) {
                        keys.push(extra.into());
                    }
                }
                for k in keys {
                    if let Some(j) = value(&t.get(&k)) {
                        to.insert(k, j);
                    }
                }
                Json::Object(to)
            })
            .collect();
        o.insert(if probe_mod { "tags" } else { "[]" }.into(), Json::Array(tags));
    }
    if !probe_mod {
        o.insert("flags".into(), n(f64::from(m.flags)));
        o.insert("keywordFlags".into(), n(f64::from(m.keyword_flags)));
        return Json::Object(o);
    }
    json!({ "$mod": o })
}

pub fn ser(v: &BVal) -> Json {
    match v {
        BVal::Str(s) => Json::String(s.clone()),
        BVal::Bool(b) => Json::Bool(*b),
        BVal::Num(x) => n(*x),
        BVal::Item(name) => json!({ "$item": name }),
        BVal::Fn => json!({ "$fn": 1 }),
        BVal::Table(t) => ser_table(t),
        // the probe's proxy of a nested breakdown module serializes as `{}` (export-breakdown.ts / export-calcs.ts)
        BVal::Module(_) => json!({}),
        BVal::Mod(m) => ser_mod(&m.0),
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

pub fn canon(v: &Json) -> String {
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

/// export-calcs.ts `normValue` on Rust's value.
fn normalize(key: &str, v: &mut Json, gem_sets: &[String]) {
    if (key == "SkillBuffs" || key == "SkillDebuffs")
        && let Some(rows) = v.get_mut("modList").and_then(|r| r.get_mut("[]")).and_then(Json::as_array_mut)
    {
        rows.sort_by_key(canon);
    }
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

#[derive(Default)]
struct Tally {
    out_keys: usize,
    out_eq: usize,
    bd_keys: usize,
    bd_eq: usize,
    bad: Vec<String>,
}

fn check_breakdown(slug: &str, actor: &str, lua: &Json, rust: Option<&Breakdown>, gem_sets: &[String], t: &mut Tally) {
    match (lua.is_null() || lua.get("absent").is_some(), rust) {
        (true, None) => return,
        (true, Some(_)) => {
            t.bad.push(format!("{slug} | {actor} breakdown | rust has one, LuaJIT none"));
            return;
        }
        (false, None) => {
            t.bad.push(format!("{slug} | {actor} breakdown | rust none"));
            return;
        }
        (false, Some(_)) => {}
    }
    let rust = rust.expect("some");
    let keys = lua["keys"].as_object().expect("keys");
    let rust_map: BTreeMap<String, Json> = rust
        .entries()
        .map(|(k, v)| {
            let mut j = ser(v);
            normalize(k, &mut j, gem_sets);
            (k.clone(), j)
        })
        .collect();
    for (k, lv) in keys {
        t.bd_keys += 1;
        match rust_map.get(k) {
            Some(r) if canon(r) == canon(lv) => t.bd_eq += 1,
            Some(r) => t.bad.push(format!(
                "{slug} | breakdown {actor}.{k} |\n    rust   {}\n    luajit {}",
                short(r),
                short(lv)
            )),
            None => t
                .bad
                .push(format!("{slug} | breakdown {actor}.{k} | rust nil, luajit {}", short(lv))),
        }
    }
    for (k, r) in &rust_map {
        if !keys.contains_key(k) {
            t.bad
                .push(format!("{slug} | breakdown {actor}.{k} | rust {}, luajit nil", short(r)));
        }
    }
}

fn short(j: &Json) -> String {
    let s = canon(j);
    if s.len() > 600 && std::env::var_os("CALCS_FULL").is_none() {
        format!("{}...", &s[..600])
    } else {
        s
    }
}

#[test]
fn calcs_output_and_breakdown_match_luajit() {
    let list = scenarios();
    assert!(list.len() >= 52, "calcs fixtures: {} scenarios", list.len());
    let filter = std::env::var("CALCS_FILTER").ok();
    let mut total = Tally::default();
    let mut ran = 0;
    println!("scenario -> output keys / equal, breakdown keys / equal, divergences");
    for slug in &list {
        if filter.as_deref().is_some_and(|f| !slug.contains(f)) {
            continue;
        }
        ran += 1;
        let fx: Json =
            serde_json::from_str(&std::fs::read_to_string(calcs_fixtures().join(format!("{slug}.json"))).expect("fixture")).expect("json");
        let (_session, run) = calcs_run(&fx);
        let r = run.borrow();
        let mut t = Tally::default();
        let rust_out = actor_output(&r.env, &r.state.player);
        let lua_out = fx["output"].as_object().expect("output");
        let ro = rust_out.as_object().expect("object");
        for (k, lv) in lua_out {
            t.out_keys += 1;
            match ro.get(k) {
                Some(rv) if canon(rv) == canon(lv) => t.out_eq += 1,
                Some(rv) => t
                    .bad
                    .push(format!("{slug} | output.{k} |\n    rust   {}\n    luajit {}", short(rv), short(lv))),
                None => t.bad.push(format!("{slug} | output.{k} | rust nil, luajit {}", short(lv))),
            }
        }
        for (k, rv) in ro {
            if !lua_out.contains_key(k) {
                t.bad.push(format!("{slug} | output.{k} | rust {}, luajit nil", short(rv)));
            }
        }
        let gem_sets: Vec<String> = fx["r13"]["gemSets"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        check_breakdown(
            slug,
            "player",
            &fx["breakdown"]["player"],
            r.state.player.breakdown.as_ref(),
            &gem_sets,
            &mut t,
        );
        let mb = r.state.minion.as_ref().map(|m| m.borrow().breakdown.clone());
        check_breakdown(
            slug,
            "minion",
            &fx["breakdown"]["minion"],
            mb.as_ref().and_then(Option::as_ref),
            &gem_sets,
            &mut t,
        );
        println!(
            "{slug:<55} {:>4} {:>4}  {:>4} {:>4}  {}",
            t.out_keys,
            t.out_eq,
            t.bd_keys,
            t.bd_eq,
            t.bad.len()
        );
        total.out_keys += t.out_keys;
        total.out_eq += t.out_eq;
        total.bd_keys += t.bd_keys;
        total.bd_eq += t.bd_eq;
        total.bad.extend(t.bad);
    }
    println!(
        "TOTAL {ran} scenarios: output keys {} equal {}, breakdown keys {} equal {}, divergences {}",
        total.out_keys,
        total.out_eq,
        total.bd_keys,
        total.bd_eq,
        total.bad.len()
    );
    let show = std::env::var("CALCS_SHOW").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    for b in total.bad.iter().take(show) {
        println!("  {b}");
    }
    assert!(total.bad.is_empty(), "{} divergences", total.bad.len());
}

/// R8.6 perf: the CALCS `buildOutput` (after MAIN on the same session, fresh session each run) on every scenario of the corpus, median
/// of 7 runs after 1 warm-up. Run: `cargo test -p pob-calc --release --test calcs_mode -- --ignored --nocapture perf`.
#[test]
#[ignore = "perf"]
fn perf_calcs_build_output() {
    for slug in scenarios() {
        let fx: Json =
            serde_json::from_str(&std::fs::read_to_string(calcs_fixtures().join(format!("{slug}.json"))).expect("fixture")).expect("json");
        let mut xs = Vec::new();
        for i in 0..8 {
            let mut model = pob_build::load_build_xml(&xml_of(&fx)).expect("xml");
            if let Some(ov) = fx["calcsInputOverride"].as_object() {
                for (k, v) in ov {
                    let iv = match v {
                        Json::String(s) => pob_build::InputValue::Str(s.clone()),
                        Json::Object(o) => pob_build::InputValue::Num(o["$n"].as_str().expect("$n").parse().expect("number")),
                        _ => unreachable!(),
                    };
                    model.calcs_input.insert(k.clone(), iv);
                }
            }
            let session = CalcSession::new(Rc::new(model), CalculateOptions::default());
            session.build_output(CalcMode::Main).expect("MAIN");
            let t0 = std::time::Instant::now();
            let out = session.build_output(CalcMode::Calcs).expect("CALCS");
            let el = t0.elapsed().as_secs_f64() * 1000.0;
            drop(out);
            if i > 0 {
                xs.push(el);
            }
        }
        xs.sort_by(f64::total_cmp);
        println!("PERF {slug:<55} CALCS buildOutput {:>8.3} ms", xs[xs.len() / 2]);
    }
}
