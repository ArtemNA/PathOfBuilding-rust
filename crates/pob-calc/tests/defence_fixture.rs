//! R4.3 gate (R4-ARCH §2a, defence slice): `calculate` with the real `defence` and a stub `buildDefenceEstimations` on the
//! 6 goldens = pob-ts `perform(env, defence, noop)` + `finishOutput` (`tests/fixtures/<build>.defence-pobts.json`, producer
//! `tools/lua-corpus/export-defence-only.ts`), EXACTLY: the set of `stats` keys, every value bit for bit (`%.17g`, `-0`
//! included), player/enemy conditions and multipliers key for key, and the `unimplemented` entries perform/defence and the
//! buildOutput tail push (same text, same order). No tolerance, no skipped key.
//!
//! Pinned exactly (both values, every case; anything else fails): class `pob-ts-no-breakdown` -- the same five
//! `Req<Attr>String` keys as `perform_fixture.rs` (CalcPerform.lua:2157-2159, MINEFIELD-RUST R8). `calcs.defence` adds no
//! case: all 35 `if breakdown then` branches of CalcDefence.lua:511-1671 write breakdown tables only (see `defence.rs`).
//! Class `platform-libm`: 0 cases (no pin needed; the `^` sites are `hitChance` -- flattened by `round` -- and the
//! lucky/unlucky rolls, which no golden runs).

use std::cell::RefCell;

use pob_calc::{CalculateOptions, CalculateResult, Env, PerformState, StatValue, Who, calculate_file, defence, hit_chance, noop_phase};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_lua::format_g;
use pob_mods::StoreRef;
use serde_json::{Map, Value as Json, json};

fn fixture(name: &str) -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.defence-pobts.json", name.replace(' ', "-")));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("fixture json")
}

fn opts() -> CalculateOptions {
    CalculateOptions {
        defence: Some(defence),
        estimations: Some(noop_phase),
        // R5.4: the "= pob-ts" gate reads mod lines like pob-ts
        parse_mode: pob_calc::ParseMode::CacheOnly,
        // R6.3: the "= pob-ts" gates run without the skill phase (pob-ts has none)
        skills: false,
        live_alloc_pairs: None,
    }
}

fn run_with(name: &str, o: CalculateOptions) -> CalculateResult {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden");
    calculate_file(g.xml_path(), o).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn run(name: &str) -> CalculateResult {
    run_with(name, opts())
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

fn phase_unimplemented(r: &CalculateResult) -> Vec<String> {
    r.env
        .unimplemented
        .iter()
        .filter(|u| u.starts_with("CalcPerform.lua:") || u.starts_with("CalcDefence.lua:") || u.starts_with("Calcs.lua:"))
        .cloned()
        .collect()
}

#[test]
fn defence_only_equals_pob_ts_for_six_goldens() {
    let mut report = Vec::new();
    let mut bad = Vec::new();
    for g in GOLDEN_BUILDS {
        let fx = fixture(g.name);
        let r = run(g.name);
        let (mut stats, neg_zero) = stats_dump(&r);
        let stat_count = stats.len();
        let mut theirs = fx["stats"].as_object().expect("stats").clone();
        let mut d = Vec::new();
        let pinned = unpin(g.name, &mut stats, &mut theirs, &mut d);
        let compared = stats.len();
        diff_obj("stats", &Json::Object(stats), &Json::Object(theirs), &mut d);
        if json!(neg_zero) != fx["negZero"] {
            d.push(format!("negZero: rust {neg_zero:?} != pob-ts {}", fx["negZero"]));
        }
        diff_obj("player.conditions", &conds(&r.env.mod_db), &fx["player"]["conditions"], &mut d);
        diff_obj("player.multipliers", &mults(&r.env.mod_db), &fx["player"]["multipliers"], &mut d);
        diff_obj("enemy.conditions", &conds(&r.env.enemy_db), &fx["enemy"]["conditions"], &mut d);
        diff_obj("enemy.multipliers", &mults(&r.env.enemy_db), &fx["enemy"]["multipliers"], &mut d);
        let unimpl = phase_unimplemented(&r);
        if json!(unimpl) != fx["unimplemented"] {
            d.push(format!("unimplemented: rust {unimpl:#?}\n!= pob-ts {:#}", fx["unimplemented"]));
        }
        report.push(format!(
            "{}: stats {} (pob-ts {}), exact {compared}, pinned {pinned}, platform-libm 0, unimplemented {}, diffs {}",
            g.name,
            stat_count,
            fx["statCount"],
            unimpl.len(),
            d.len()
        ));
        bad.extend(d.into_iter().map(|x| format!("{}: {x}", g.name)));
    }
    println!("{}", report.join("\n"));
    assert!(bad.is_empty(), "{} differences:\n{}", bad.len(), bad.join("\n"));
}

/// The comparator is not vacuous on the defence keys: a flipped bit and a dropped key are caught.
#[test]
fn comparator_is_not_vacuous() {
    let fx = fixture("OccVortex");
    let r = run("OccVortex");
    let (mut stats, _) = stats_dump(&r);
    let theirs = Json::Object(fx["stats"].as_object().unwrap().clone());
    let ev = r.perform.player.output.num("Evasion");
    stats.insert("Evasion".into(), json!(format_g(f64::from_bits(ev.to_bits() + 1), 17)));
    stats.shift_remove("EnergyShieldRecoveryCap");
    let mut d = Vec::new();
    diff_obj("stats", &Json::Object(stats), &theirs, &mut d);
    assert_eq!(d.len(), 2, "{d:?}");
}

/// `calcs.hitChance` edge cases (L7): `hitChance(0, 0)` is `round(nan)` folded by `minsd/maxsd` = 100, not NaN.
#[test]
fn hit_chance_edges() {
    assert_eq!(hit_chance(0.0, 0.0), 100.0);
    assert_eq!(hit_chance(1000.0, -1.0), 5.0);
    assert_eq!(hit_chance(0.0, 500.0), 100.0);
    assert_eq!(hit_chance(1e9, 1.0), 5.0);
    assert_eq!(pob_calc::armour_reduction(0.0, 0.0), 0.0);
    assert_eq!(pob_calc::armour_reduction(1000.0, 200.0), 50.0);
}

thread_local! {
    static DEF_MS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
}

fn timed_defence(env: &mut Env, st: &mut PerformState, who: Who) {
    let t = std::time::Instant::now();
    defence(env, st, who);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    DEF_MS.with(|v| v.borrow_mut().push(ms));
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// DoD perf: `calcs.defence` alone per golden, median of 9 runs (µs).
/// `cargo test -p pob-calc --release --test defence_fixture -- --ignored --nocapture perf`.
#[test]
#[ignore = "timing run; release build"]
fn perf_defence() {
    let o = CalculateOptions {
        defence: Some(timed_defence),
        estimations: Some(noop_phase),
        parse_mode: pob_calc::ParseMode::CacheOnly,
        // R6.3: the "= pob-ts" gates run without the skill phase (pob-ts has none)
        skills: false,
        live_alloc_pairs: None,
    };
    for g in GOLDEN_BUILDS {
        let _ = run_with(g.name, o.clone());
        DEF_MS.with(|v| v.borrow_mut().clear());
        let mut perf = Vec::new();
        for _ in 0..9 {
            let r = run_with(g.name, o.clone());
            perf.push(r.timings.perform_ms);
        }
        let d = DEF_MS.with(|v| v.borrow().clone());
        println!(
            "{}: defence median {:.1} us (min {:.1}, max {:.1}, n {}); perform+defence {:.3} ms",
            g.name,
            median(d.clone()) * 1000.0,
            d.iter().copied().fold(f64::INFINITY, f64::min) * 1000.0,
            d.iter().copied().fold(0.0, f64::max) * 1000.0,
            d.len(),
            median(perf)
        );
    }
}
