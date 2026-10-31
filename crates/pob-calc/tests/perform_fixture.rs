//! R4.2 gate (R4-ARCH §2a, perform-only slice): `calculate` with stub defence/estimations on the 6 goldens = pob-ts
//! `perform(env, noop, noop)` + `finishOutput` (`tests/fixtures/<build>.perform-pobts.json`, producer
//! `tools/lua-corpus/export-perform-only.ts`), EXACTLY: the set of `stats` keys, every value bit for bit (`%.17g`, `-0`
//! included), player/enemy conditions and multipliers key for key, and the `unimplemented` entries perform and the
//! buildOutput tail push (same text, same order). No tolerance, no skipped key.
//!
//! The XML goes in "as desktop" (`pob_build::read_build_file`, CRLF -> LF): pob-ts normalises CRLF in its parser too.
//!
//! One class of differences is pinned EXACTLY (both values, every case; anything else fails): `pob-ts-no-breakdown`.
//! MAIN mode builds the breakdown (CalcSetup.lua:500 `env.buildBreakdown = mode == "MAIN" or mode == "CALCS"`), and
//! CalcPerform.lua:2157-2159 then writes `Req<Attr>String = colorCodes.NEGATIVE .. val` when the requirement is not met;
//! pob-ts never runs that branch (perform.ts:1161-1168). Measured on desktop-identical LuaJIT (docker, debug hook on
//! :2153/:2158 of a MAIN `calcs.perform`): `breakdown` is non-nil and :2158 runs; on the goldens the requirement is met
//! (Generals: Dex 224 >= 212 -> number) or a nested `calcs.perform` from CalcMirages.lua:52 (buildBreakdown false)
//! rewrites the key (Dual Savior) -- so the oracle never shows the string, but perform-only (item ModCache misses lower
//! Str/Dex) does. Rust follows Lua.

use pob_calc::{CalculateOptions, CalculateResult, OutValue, StatValue, calculate_file, noop_phase};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_lua::format_g;
use pob_mods::StoreRef;
use serde_json::{Map, Value as Json, json};

fn fixture(name: &str) -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.perform-pobts.json", name.replace(' ', "-")));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("fixture json")
}

fn stub_opts() -> CalculateOptions {
    CalculateOptions {
        defence: Some(noop_phase),
        estimations: Some(noop_phase),
        // R5.4: the "= pob-ts" gate reads mod lines like pob-ts
        parse_mode: pob_calc::ParseMode::CacheOnly,
        // R6.3: the "= pob-ts" gates run without the skill phase (pob-ts has none)
        skills: false,
    }
}

fn run(name: &str) -> CalculateResult {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden");
    calculate_file(g.xml_path(), stub_opts()).unwrap_or_else(|e| panic!("{name}: {e}"))
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

/// Applies the pin: a pinned key must hold exactly the pinned pair, and is then left out of the diff.
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

/// The entries perform/finishOutput push (initEnv's own are R3's gate, `env_fixture.rs`).
fn perform_unimplemented(r: &CalculateResult) -> Vec<String> {
    r.env
        .unimplemented
        .iter()
        .filter(|u| u.starts_with("CalcPerform.lua:") || u.starts_with("Calcs.lua:"))
        .cloned()
        .collect()
}

#[test]
fn perform_only_equals_pob_ts_for_six_goldens() {
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
        diff_obj("stats", &Json::Object(stats), &Json::Object(theirs), &mut d);
        if json!(neg_zero) != fx["negZero"] {
            d.push(format!("negZero: rust {neg_zero:?} != pob-ts {}", fx["negZero"]));
        }
        diff_obj("player.conditions", &conds(&r.env.mod_db), &fx["player"]["conditions"], &mut d);
        diff_obj("player.multipliers", &mults(&r.env.mod_db), &fx["player"]["multipliers"], &mut d);
        diff_obj("enemy.conditions", &conds(&r.env.enemy_db), &fx["enemy"]["conditions"], &mut d);
        diff_obj("enemy.multipliers", &mults(&r.env.enemy_db), &fx["enemy"]["multipliers"], &mut d);
        let unimpl = perform_unimplemented(&r);
        if json!(unimpl) != fx["unimplemented"] {
            d.push(format!("unimplemented: rust {unimpl:#?}\n!= pob-ts {:#}", fx["unimplemented"]));
        }
        report.push(format!(
            "{}: stats {} (pob-ts {}), pinned {}, player cond {} mult {}, enemy cond {} mult {}, unimplemented {}, diffs {}",
            g.name,
            stat_count,
            fx["statCount"],
            pinned,
            r.env.mod_db.borrow().conditions.len(),
            r.env.mod_db.borrow().multipliers.len(),
            r.env.enemy_db.borrow().conditions.len(),
            r.env.enemy_db.borrow().multipliers.len(),
            unimpl.len(),
            d.len()
        ));
        bad.extend(d.into_iter().map(|x| format!("{}: {x}", g.name)));
    }
    println!("{}", report.join("\n"));
    assert!(bad.is_empty(), "{} differences:\n{}", bad.len(), bad.join("\n"));
}

/// The comparator above is not vacuous: one flipped bit, one dropped key, one flipped condition are caught.
#[test]
fn comparator_is_not_vacuous() {
    let fx = fixture("Dual Savior");
    let r = run("Dual Savior");
    let (mut stats, _) = stats_dump(&r);
    let mut theirs = fx["stats"].as_object().unwrap().clone();
    let mut d = Vec::new();
    assert_eq!(unpin("Dual Savior", &mut stats, &mut theirs, &mut d), 2);
    let theirs = Json::Object(theirs);
    diff_obj("stats", &Json::Object(stats.clone()), &theirs, &mut d);
    assert!(d.is_empty(), "{d:?}");
    // A pin that does not hold is itself a failure.
    let (mut s2, _) = stats_dump(&r);
    s2.insert("ReqDexString".into(), json!("104"));
    let mut t2 = fx["stats"].as_object().unwrap().clone();
    let mut d2 = Vec::new();
    assert_eq!(unpin("Dual Savior", &mut s2, &mut t2, &mut d2), 1);
    assert_eq!(d2.len(), 1);
    let life = r.perform.player.output.num("Life");
    stats.insert("Life".into(), json!(format_g(f64::from_bits(life.to_bits() + 1), 17)));
    stats.shift_remove("Mana");
    diff_obj("stats", &Json::Object(stats), &theirs, &mut d);
    assert_eq!(d.len(), 2, "{d:?}");
    let mut c = conds(&r.env.mod_db);
    let k = c.as_object().unwrap().keys().next().unwrap().clone();
    let v = c[&k].as_bool().unwrap();
    c[&k] = json!(!v);
    let mut d = Vec::new();
    diff_obj("cond", &c, &fx["player"]["conditions"], &mut d);
    assert_eq!(d.len(), 1);
    // The output keeps non-numbers out of the mod core's numeric view.
    assert!(matches!(
        r.perform.player.output.get("GuardSkillActive"),
        Some(OutValue::Bool(false))
    ));
    assert!(
        r.env
            .player
            .borrow()
            .actor
            .borrow()
            .output
            .as_ref()
            .unwrap()
            .get("GuardSkillActive")
            .is_none()
    );
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// DoD perf (R4-ARCH §2ґ, user requirement): `calculate` perform-only per golden, median of 7 runs per phase.
/// `cargo test -p pob-calc --release --test perform_fixture -- --ignored --nocapture perf`.
#[test]
#[ignore = "timing run; release build"]
fn perf_calculate_perform_only() {
    for g in GOLDEN_BUILDS {
        let _ = run(g.name); // warm the data caches
        let (mut total, mut load, mut init, mut perf) = (vec![], vec![], vec![], vec![]);
        for _ in 0..7 {
            let t = std::time::Instant::now();
            let r = run(g.name);
            total.push(t.elapsed().as_secs_f64() * 1000.0);
            load.push(r.timings.load_ms);
            init.push(r.timings.init_env_ms);
            perf.push(r.timings.perform_ms);
        }
        println!(
            "{}: calculate {:.3} ms (min {:.3}, max {:.3}); load {:.3}, initEnv {:.3}, perform {:.3}",
            g.name,
            median(total.clone()),
            total.iter().copied().fold(f64::INFINITY, f64::min),
            total.iter().copied().fold(0.0, f64::max),
            median(load),
            median(init),
            median(perf)
        );
    }
}
