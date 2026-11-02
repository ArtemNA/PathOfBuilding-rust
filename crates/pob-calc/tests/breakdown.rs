//! R8.4/R8.5 gate: `env.player.breakdown` / `env.minion.breakdown` against LuaJIT, key by key.
//!
//! Corpus `tests/fixtures/breakdown/<slug>.json` (producer `tools/lua-corpus/export-breakdown.ts`): the 6 goldens, R4-synth
//! (`defence-synth`), R4-synth-2 (`defence-synth2`) and the 20 R7-synth scenarios of the R8.1 display fixtures. Per actor and key the
//! fixture holds the writers (`<file>:<line>` of every `breakdown[key] = ...`), `snap` (the table at `calcs.triggers`, i.e. after perform +
//! defence + estimations), `final` (after `BuildOutput`), `class` (`ported` = perform / defence / estimations, `R8.5` = offence / triggers /
//! mirages, `Calcs` = Calcs.lua SkillBuffs/SkillDebuffs) and `gate` (the value the R8.4 modules leave).
//!
//! Rule (R8.5): Rust's breakdown after `calculate_with` (raw XML, as the docker probe read it) is the state after `BuildOutput`: every key
//! of class `ported` or `R8.5` equals LuaJIT's `final` (strings byte for byte, numbers `%.17g`, structure; absent iff `final` is absent),
//! and Rust has no other key. `Calcs` keys are written by Calcs.lua after `calcs.perform` (not part of `calculate_with`): counted apart,
//! Rust must not have them. Nothing is tolerated. The nested hand modules (`breakdown.MainHand`/`.OffHand`) serialize as `{"$bd": ...}`
//! (R8.5b, see [`ser`]) and are also counted key by key; `skill`-class keys (a Data/Skills writer, R8.5b: the `fn-synth` scenarios of
//! every skill function that writes `breakdown`) are counted per writer line.

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
        "r81-synth" | "fn-synth" => fixtures().join(format!("offence-synth/{name}.xml")),
        k => panic!("unknown kind {k}"),
    };
    let xml = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    // R8.5b: a skill-part variant -- `skillPart="<n>"` on the scenario's single <Gem> (export-breakdown.ts `withPart`)
    match fx.get("skillPart").and_then(|p| p.as_u64()) {
        Some(n) => {
            assert_eq!(
                xml.matches("<Gem ").count(),
                1,
                "{name}: a skill-part variant needs exactly one <Gem>"
            );
            xml.replacen("<Gem ", &format!("<Gem skillPart=\"{n}\" "), 1)
        }
        None => xml,
    }
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
        // `breakdown.MainHand` / `.OffHand` (CalcOffence.lua:2006/:2023) is a module made by `require(calcs.breakdownModule)`: the probe
        // serializes the real table behind its proxy (R8.5b) as `{"$bd": {key: value}}`.
        BVal::Module(b) => {
            let e: Map<String, Json> = b.entries().map(|(k, v)| (k.clone(), ser(v))).collect();
            json!({ "$bd": e })
        }
        // export-breakdown.ts `SMod`: `{"$mod": {name, type, source, value, flags, keywordFlags, tags}}` -- the CALCS tail (`SkillBuffs` /
        // `SkillDebuffs`, class `Calcs`, not compared here) and, since 3dbca4463 (#10314), the `modList` of "You can't deal X damage"
        BVal::Mod(m) => {
            let m = &m.0;
            let mut o = Map::new();
            o.insert("name".into(), Json::String(m.name.to_string()));
            if let Some(t) = m.ty {
                o.insert("type".into(), Json::String(t.as_str().to_owned()));
            }
            if let Some(s) = &m.source {
                o.insert("source".into(), Json::String(s.to_string()));
            }
            let value = match &m.value {
                pob_mods::Value::Num(n) => ser(&BVal::Num(*n)),
                pob_mods::Value::Bool(b) => Json::Bool(*b),
                pob_mods::Value::Str(s) => Json::String(s.to_string()),
                other => json!({ "$unported-mod-value": other.type_name() }),
            };
            o.insert("value".into(), value);
            o.insert("flags".into(), Json::String(format_g(f64::from(m.flags), 17)));
            o.insert("keywordFlags".into(), Json::String(format_g(f64::from(m.keyword_flags), 17)));
            if !m.tags.is_empty() {
                // tag tables are not serialized here (no breakdown row compared by this gate carries one) -- a mismatch, not a pass
                o.insert("tags".into(), json!({ "$unported-tags": m.tags.len() }));
            }
            json!({ "$mod": o })
        }
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
    /// class -> (keys, equal)
    by_class: BTreeMap<String, (usize, usize)>,
    r85: usize,
    calcs: usize,
    /// R8.5b: the keys inside the hand modules (`breakdown.MainHand` / `.OffHand`): (keys, equal)
    hand: (usize, usize),
    /// R8.5b: the Data/Skills writer lines (`<file>:<line>`) of `skill`-class keys -> (keys, equal)
    skill_writers: BTreeMap<String, (usize, usize)>,
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
            "Calcs" => {
                t.calcs += 1;
                if rust_map.contains_key(k.as_str()) {
                    t.bad
                        .push(format!("{slug} | {actor}.{k} | rust writes a key only Calcs.lua writes"));
                }
                continue;
            }
            _ => {}
        }
        // R8.5b: the hand module's keys, one by one (a missing / extra key on either side counts as a divergent key)
        if let Some(lh) = e.get("final").and_then(|f| f.get("$bd")).and_then(Json::as_object) {
            let rh = rust_map.get(k.as_str()).and_then(|r| r.get("$bd")).and_then(Json::as_object);
            let mut names: Vec<&String> = lh.keys().collect();
            if let Some(rh) = rh {
                names.extend(rh.keys().filter(|x| !lh.contains_key(*x)));
            }
            for n in names {
                t.hand.0 += 1;
                let (r, l) = (rh.and_then(|rh| rh.get(n)), lh.get(n));
                if r == l {
                    t.hand.1 += 1;
                } else if rh.is_some() {
                    let s = |x: Option<&Json>| x.map_or("nil".to_owned(), Json::to_string);
                    t.bad
                        .push(format!("{slug} | {actor}.{k}.{n} |\n    rust   {}\n    luajit {}", s(r), s(l)));
                }
            }
        }
        let equal = match (rust_map.get(k.as_str()), e.get("final")) {
            (Some(r), Some(f)) => r == f,
            (None, None) => true,
            _ => false,
        };
        if e["class"] == "skill" {
            for w in e["writers"].as_array().expect("writers").iter().filter_map(Json::as_str) {
                if !w.starts_with("Calc") {
                    let s = t.skill_writers.entry(w.to_owned()).or_default();
                    s.0 += 1;
                    s.1 += usize::from(equal);
                }
            }
        }
        let m = t.by_module.entry(module_of(e["writers"].as_array().expect("writers"))).or_default();
        m.0 += 1;
        let cl = t.by_class.entry(e["class"].as_str().expect("class").to_owned()).or_default();
        cl.0 += 1;
        match (rust_map.get(k.as_str()), e.get("final")) {
            _ if equal => {
                m.1 += 1;
                cl.1 += 1;
            }
            // both hand modules: the divergent inner keys are reported above, one by one
            (Some(r), Some(f)) if r.get("$bd").is_some() && f.get("$bd").is_some() => {}
            (Some(r), Some(f)) => t.bad.push(format!("{slug} | {actor}.{k} |\n    rust   {r}\n    luajit {f}")),
            (None, Some(f)) => t.bad.push(format!("{slug} | {actor}.{k} | rust nil, luajit {f}")),
            (Some(r), None) => t.bad.push(format!("{slug} | {actor}.{k} | rust {r}, luajit nil")),
            (None, None) => unreachable!("nil = nil is equal"),
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
    assert!(list.len() >= 173, "breakdown fixtures: {} scenarios", list.len());
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
        for (c, v) in t.by_class {
            let e = total.by_class.entry(c).or_default();
            e.0 += v.0;
            e.1 += v.1;
        }
        total.r85 += t.r85;
        total.calcs += t.calcs;
        total.hand.0 += t.hand.0;
        total.hand.1 += t.hand.1;
        for (w, v) in t.skill_writers {
            let e = total.skill_writers.entry(w).or_default();
            e.0 += v.0;
            e.1 += v.1;
        }
        total.bad.extend(t.bad);
    }
    for (w, (n, eq)) in &total.skill_writers {
        println!("skill writer {w:<24} keys {n:>4}, equal {eq:>4}");
    }
    println!("hand module keys {}, equal {}", total.hand.0, total.hand.1);
    for (m, (g, eq)) in &total.by_module {
        println!("module {m:<12} gate keys {g:>6}, equal {eq:>6}");
    }
    for (c, (g, eq)) in &total.by_class {
        println!("class {c:<12} keys {g:>6}, equal {eq:>6}");
    }
    let (g, eq) = total.by_module.values().fold((0, 0), |a, v| (a.0 + v.0, a.1 + v.1));
    println!(
        "TOTAL {} scenarios: gate keys {g}, equal {eq}, R8.5 keys {}, Calcs keys {}, divergences {}",
        list.len(),
        total.r85,
        total.calcs,
        total.bad.len()
    );
    let show = std::env::var("BD_SHOW").ok().and_then(|s| s.parse().ok()).unwrap_or(80);
    for b in total.bad.iter().take(show) {
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
