//! R4.4b -- R4-ARCH gate (a): `calculate` with the real `perform` + `defence` + `buildDefenceEstimations` (the default
//! options) on the 6 goldens = the pob-ts full output (`tests/fixtures/<build>.r4-pobts.json`, R4.1), EXACTLY: the set of
//! `stats` keys, every value bit for bit (`%.17g`, `-0` included: `negZero`), player/enemy conditions and multipliers key
//! for key, the `unimplemented` entries perform/defence/estimations and the buildOutput tail push (same text, same order --
//! the `CalcPerform`/`CalcDefence`/`Calcs` ones, as `defence_fixture.rs`; the spec/initEnv entries carry R3's re-taken line
//! numbers and are gated by `env_fixture.rs`) and `_meta` (numbers compared as numbers). No tolerance, no skipped key.
//!
//! Pinned exactly (both values, every case; anything else fails):
//!  * `pob-ts-no-breakdown` -- the same five `Req<Attr>String` keys as `perform_fixture.rs` (CalcPerform.lua:2157-2159,
//!    MINEFIELD-RUST R8). `buildDefenceEstimations` adds no case: none of its 42 `if breakdown then` branches
//!    (CalcDefence.lua:1720-3870, listed in `estimations.rs`) writes anything but breakdown tables or locals.
//!  * `platform-libm`: 0 cases (the only `^` site of estimations is PvP, :3126-3127, which no golden enables).

use pob_calc::{CalculateOptions, CalculateResult, StatValue, calculate_file};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_lua::format_g;
use pob_mods::StoreRef;
use serde_json::{Map, Value as Json, json};

fn fixture(name: &str) -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.r4-pobts.json", name.replace(' ', "-")));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("fixture json")
}

fn run(name: &str) -> CalculateResult {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden");
    // R5.4: the "= pob-ts" gate reads mod lines like pob-ts
    let o = CalculateOptions {
        parse_mode: pob_calc::ParseMode::CacheOnly,
        // R6.3: the "= pob-ts" gate runs without the skill phase (pob-ts has none)
        skills: false,
        ..CalculateOptions::default()
    };
    calculate_file(g.xml_path(), o).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The fixture's encoding: number -> `%.17g`, string -> `{ "s": .. }`, boolean as is.
fn stats_dump(r: &CalculateResult) -> (Map<String, Json>, Vec<String>) {
    let mut out = Map::new();
    let mut neg_zero = Vec::new();
    let mut keys: Vec<&String> = r.stats.keys().collect();
    keys.sort();
    for k in keys {
        let v = match &r.stats[k] {
            StatValue::Num(n) => {
                if *n == 0.0 && n.is_sign_negative() {
                    neg_zero.push(k.clone());
                }
                json!(format_g(*n, 17))
            }
            StatValue::Str(s) => json!({ "s": s }),
            StatValue::Bool(b) => json!(b),
        };
        out.insert(k.clone(), v);
    }
    (out, neg_zero)
}

fn conds(db: &StoreRef) -> Json {
    let d = db.borrow();
    let mut keys: Vec<_> = d.conditions.keys().collect();
    keys.sort();
    Json::Object(keys.into_iter().map(|k| (k.to_string(), json!(d.conditions[k]))).collect())
}

fn mults(db: &StoreRef) -> Json {
    let d = db.borrow();
    let mut keys: Vec<_> = d.multipliers.keys().collect();
    keys.sort();
    Json::Object(
        keys.into_iter()
            .map(|k| (k.to_string(), json!(format_g(d.multipliers[k], 17))))
            .collect(),
    )
}

/// `pob-ts-no-breakdown` (module doc): (build, key, Rust value, pob-ts value) -- every case, exactly.
const PINNED: [(&str, &str, &str, &str); 5] = [
    ("Dual Savior", "ReqDexString", "^xDD0022104", "104"),
    ("Dual Savior", "ReqStrString", "^xDD0022122", "122"),
    ("Dual Wield Cospris CoC", "ReqStrString", "^xDD002262", "62"),
    ("Dual Wield Cospris CoC - Lethal Pride", "ReqStrString", "^xDD002262", "62"),
    ("Generals Perforate Zerker", "ReqDexString", "^xDD0022212", "212"),
];

fn unpin(build: &str, ours: &mut Map<String, Json>, theirs: &mut Map<String, Json>, out: &mut Vec<String>) -> usize {
    let mut n = 0;
    for (b, key, rust, pob_ts) in PINNED {
        if b != build {
            continue;
        }
        let (want_ours, want_theirs) = (json!({ "s": rust }), json!(pob_ts));
        if ours.get(key) == Some(&want_ours) && theirs.get(key) == Some(&want_theirs) {
            ours.shift_remove(key);
            theirs.shift_remove(key);
            n += 1;
        } else {
            out.push(format!(
                "pinned {key}: rust {:?} pob-ts {:?}, pinned {want_ours} / {want_theirs}",
                ours.get(key),
                theirs.get(key)
            ));
        }
    }
    n
}

fn diff_obj(label: &str, ours: &Json, theirs: &Json, out: &mut Vec<String>) {
    let (a, b) = (ours.as_object().expect("object"), theirs.as_object().expect("object"));
    let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        if a.get(k) != b.get(k) {
            out.push(format!("{label}[{k}]: rust {:?} != pob-ts {:?}", a.get(k), b.get(k)));
        }
    }
}

fn meta_json(r: &CalculateResult) -> Json {
    let m = &r.meta;
    json!({
        "allocNodes": m.alloc_nodes,
        "ascendName": m.ascend_name,
        "className": m.class_name,
        "items": m.items,
        "mainSocketGroup": m.main_socket_group,
        "socketGroups": m.socket_groups,
        "treeVersion": m.tree_version,
    })
}

/// The entries the phases push (perform, defence, estimations, the buildOutput tail).
fn phase_unimplemented(all: &[String]) -> Vec<String> {
    all.iter()
        .filter(|u| u.starts_with("CalcPerform.lua:") || u.starts_with("CalcDefence.lua:") || u.starts_with("Calcs.lua:"))
        .cloned()
        .collect()
}

/// Numbers as `f64` (`5` and `5.0` are the same Lua number).
fn norm(v: &Json) -> Json {
    match v {
        Json::Object(m) => Json::Object(m.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
        Json::Number(n) => json!(n.as_f64()),
        other => other.clone(),
    }
}

#[test]
fn full_output_equals_pob_ts_for_six_goldens() {
    let mut report = Vec::new();
    let mut bad = Vec::new();
    for g in GOLDEN_BUILDS {
        let fx = fixture(g.name);
        let r = run(g.name);
        let (mut stats, neg_zero) = stats_dump(&r);
        let stat_count = stats.len();
        let mut theirs = fx["stats"].as_object().expect("stats").clone();
        let their_count = theirs.len();
        let mut d = Vec::new();
        let pinned = unpin(g.name, &mut stats, &mut theirs, &mut d);
        let compared = stats.len();
        diff_obj("stats", &Json::Object(stats), &Json::Object(theirs), &mut d);
        if json!(neg_zero) != fx["negZero"] {
            d.push(format!("negZero: rust {neg_zero:?} != pob-ts {}", fx["negZero"]));
        }
        diff_obj("player.conditions", &conds(&r.env.mod_db), &fx["playerConditions"], &mut d);
        diff_obj("player.multipliers", &mults(&r.env.mod_db), &fx["playerMultipliers"], &mut d);
        diff_obj("enemy.conditions", &conds(&r.env.enemy_db), &fx["enemyConditions"], &mut d);
        diff_obj("enemy.multipliers", &mults(&r.env.enemy_db), &fx["enemyMultipliers"], &mut d);
        let unimpl = phase_unimplemented(&r.unimplemented);
        let their_unimpl: Vec<String> = fx["unimplemented"]
            .as_array()
            .expect("unimplemented")
            .iter()
            .map(|v| v.as_str().expect("string").to_owned())
            .collect();
        if unimpl != phase_unimplemented(&their_unimpl) {
            d.push(format!("unimplemented: rust {unimpl:#?}\n!= pob-ts {their_unimpl:#?}"));
        }
        diff_obj("meta", &norm(&meta_json(&r)), &norm(&fx["meta"]), &mut d);
        report.push(format!(
            "{}: stats {stat_count} (pob-ts {their_count}), exact {compared}, pinned pob-ts-no-breakdown {pinned}, platform-libm 0, \
             negZero {}, unimplemented {}, diffs {}",
            g.name,
            neg_zero.len(),
            unimpl.len(),
            d.len()
        ));
        bad.extend(d.into_iter().map(|x| format!("{}: {x}", g.name)));
    }
    println!("{}", report.join("\n"));
    assert!(bad.is_empty(), "{} differences:\n{}", bad.len(), bad.join("\n"));
}

/// The estimations call site is plugged: no "buildDefenceEstimations" entry in `unimplemented`, and its keys are there.
#[test]
fn default_options_run_estimations() {
    let r = run("OccVortex");
    assert!(r.stats.contains_key("TotalEHP"));
    assert!(r.stats.contains_key("PhysicalMaximumHitTaken"));
    assert!(!r.unimplemented.iter().any(|u| u.contains("buildDefenceEstimations")));
}

/// The comparator is not vacuous on the estimations keys: a flipped bit and a dropped key are caught.
#[test]
fn comparator_is_not_vacuous() {
    let fx = fixture("OccVortex");
    let r = run("OccVortex");
    let (mut stats, _) = stats_dump(&r);
    let theirs = Json::Object(fx["stats"].as_object().unwrap().clone());
    let ehp = r.perform.player.output.opt_num("TotalEHP").expect("TotalEHP");
    stats.insert("TotalEHP".into(), json!(format_g(f64::from_bits(ehp.to_bits() + 1), 17)));
    stats.shift_remove("SecondMinimalMaximumHitTaken");
    let mut d = Vec::new();
    diff_obj("stats", &Json::Object(stats), &theirs, &mut d);
    assert_eq!(d.len(), 2, "{d:?}");
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// `tools/.scratch/<file>` of the R4.1 bench, if present.
fn scratch(file: &str) -> Option<Json> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/.scratch")
        .join(file);
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// DoD perf: the full `calculate_file` (load + initEnv + perform with defence/estimations + EstimatePlayerProgress) per
/// golden, median of 15 runs after 3 warm-up runs (ms), next to the R4.1 bench (pob-ts `recalc`, wasm, LuaJIT
/// `buildOutput` with `wipeGlobalCache` -- the latter two FULL, with skills).
/// `cargo test -p pob-calc --release --test full_fixture -- --ignored --nocapture perf`.
#[test]
#[ignore = "timing run; release build"]
fn perf_full_calculate() {
    let bench = scratch("perf-r4-bench.json");
    let lj = scratch("perf-r4.json");
    let row = |build: &str, engine: &str, clock: &str, field: &str| -> Option<f64> {
        bench.as_ref()?["rows"]
            .as_array()?
            .iter()
            .find(|r| r["build"] == build && r["engine"] == engine && r["clock"] == clock)?[field]["median"]
            .as_f64()
    };
    println!(
        "| build | Rust total | load | initEnv | perform+def+est | estimate | pob-ts recalc | pob-ts total | wasm recalc | LuaJIT buildOutput (wipeCache) |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for g in GOLDEN_BUILDS {
        for _ in 0..3 {
            let _ = run(g.name);
        }
        let (mut total, mut load, mut init, mut perf, mut est) = (vec![], vec![], vec![], vec![], vec![]);
        for _ in 0..15 {
            let t = std::time::Instant::now();
            let r = run(g.name);
            total.push(t.elapsed().as_secs_f64() * 1000.0);
            load.push(r.timings.load_ms);
            init.push(r.timings.init_env_ms);
            perf.push(r.timings.perform_ms);
            est.push(r.timings.estimate_ms);
        }
        let f = |v: Option<f64>| v.map_or("-".to_owned(), |x| format!("{x:.2}"));
        let ljv = lj.as_ref().and_then(|j| j["builds"][g.name]["wipeCache_median_ms"].as_f64());
        println!(
            "| {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.4} | {} | {} | {} | {} |",
            g.name,
            median(total),
            median(load),
            median(init),
            median(perf),
            median(est),
            f(row(g.name, "ts", "wall", "recalcMs")),
            f(row(g.name, "ts", "wall", "totalMs")),
            f(row(g.name, "wasm", "wall", "recalcMs")),
            f(ljv),
        );
    }
}
