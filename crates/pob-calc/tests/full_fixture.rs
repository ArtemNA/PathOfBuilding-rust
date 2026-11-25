//! R4.4b -- R4-ARCH gate (a): `calculate` with the default options (perform + defence + `buildDefenceEstimations`,
//! `ParseMode::Full`, the skill phase) on the 6 goldens = LuaJIT's `build.calcsTab.mainOutput` after `loadBuildFromXML` (the golden
//! snapshots `builds/corpus/goldens/<build>.json`, producer `tools/make-goldens.ts`), EXACTLY: the set of `stats` keys, every value
//! bit for bit (`-0` included: the snapshot's `negZero`), and `_meta`. No tolerance, no skipped key, no pinned stat.
//!
//! Until R-INDEPENDENT-UPDATE A4 the reference was pob-ts' full output (`<build>.r4-pobts.json`, `ParseMode::CacheOnly`, no skills;
//! ignored since U-R6, pob-ts was the 2.67.2 reference). What else that fixture held is gated against LuaJIT elsewhere: player/enemy
//! conditions and multipliers by `perform_cp.rs` (CP1-CP4) and `offence_cp.rs` (OC6); the `unimplemented` entries of the phases are
//! pinned exactly ([`P5_UNIMPLEMENTED`]: the party / buff export / display branches of P5, plus [`DUAL_SAVIOR_UNIMPLEMENTED`]'s
//! two Reflection info messages -- none of them changes a `mainOutput` key here).

use pob_calc::{CalculateOptions, CalculateResult, StatValue, calculate_file};
use pob_golden::compare::{compare_meta, stats_exact};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_golden::value::StatValue as Gv;
use pob_golden::{PartialMeta, Stats};

fn run(name: &str) -> CalculateResult {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden");
    calculate_file(g.xml_path(), CalculateOptions::default()).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn to_golden(v: &StatValue) -> Gv {
    match v {
        StatValue::Num(n) => Gv::Num(*n),
        StatValue::Str(s) => Gv::Str(s.clone()),
        StatValue::Bool(b) => Gv::Bool(*b),
    }
}

fn stats_of(r: &CalculateResult) -> Stats {
    r.stats.iter().map(|(k, v)| (k.clone(), to_golden(v))).collect()
}

fn show(v: Option<&Gv>) -> String {
    match v {
        None => "nil".to_owned(),
        Some(Gv::Num(n)) => format!("{n:e} ({:#018x})", n.to_bits()),
        Some(v) => format!("{v:?}"),
    }
}

/// Every key of either side whose values are not bit-identical (a key on one side only included).
fn diff_stats(want: &Stats, got: &Stats) -> Vec<String> {
    let mut keys: Vec<&String> = want.keys().chain(got.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| match (want.get(*k), got.get(*k)) {
            (Some(w), Some(g)) => !stats_exact(w, g),
            _ => true,
        })
        .map(|k| format!("stats[{k}]: rust {} != luajit {}", show(got.get(k)), show(want.get(k))))
        .collect()
}

fn meta_of(r: &CalculateResult) -> PartialMeta {
    let m = &r.meta;
    PartialMeta {
        alloc_nodes: Some(u32::try_from(m.alloc_nodes).expect("u32")),
        items: Some(u32::try_from(m.items).expect("u32")),
        socket_groups: Some(u32::try_from(m.socket_groups).expect("u32")),
        main_socket_group: (m.main_socket_group.fract() == 0.0).then_some(m.main_socket_group as i64),
        tree_version: Some(m.tree_version.clone()),
        class_name: Some(m.class_name.clone()),
        ascend_name: Some(m.ascend_name.clone()),
    }
}

/// What the phases push on every golden (same text, same order; anything else fails): the P5 modules pob-calc does not port.
const P5_UNIMPLEMENTED: [&str; 3] = [
    "CalcPerform.lua:1340 partyTab.enableExportBuffs / buff export (P5)",
    "CalcPerform.lua:3914-4104 minion output, buff export, party export (P5)",
    "Calcs.lua:450-464 env.skillsUsed / display stats (P5)",
];

/// Dual Savior's two Reflection skills push `infoMessage`/`infoTrigger` (read by CalcTriggers/offence only) between the first
/// and the second P5 entry: its whole list, pinned the same way.
const DUAL_SAVIOR_UNIMPLEMENTED: [&str; 5] = [
    "CalcPerform.lua:1340 partyTab.enableExportBuffs / buff export (P5)",
    "CalcPerform.lua:1526-1527 activeSkill.infoMessage/infoTrigger of skill 2 (Reflection) (read by CalcTriggers/offence, R7)",
    "CalcPerform.lua:1526-1527 activeSkill.infoMessage/infoTrigger of skill 3 (Reflection) (read by CalcTriggers/offence, R7)",
    "CalcPerform.lua:3914-4104 minion output, buff export, party export (P5)",
    "Calcs.lua:450-464 env.skillsUsed / display stats (P5)",
];

/// The entries the phases push (perform, defence, estimations, the buildOutput tail).
fn unimplemented(r: &CalculateResult) -> Vec<String> {
    r.unimplemented
        .iter()
        .filter(|u| u.starts_with("CalcPerform.lua:") || u.starts_with("CalcDefence.lua:") || u.starts_with("Calcs.lua:"))
        .cloned()
        .collect()
}

#[test]
fn full_output_equals_luajit_for_six_goldens() {
    let mut report = Vec::new();
    let mut bad = Vec::new();
    for g in GOLDEN_BUILDS {
        let snap = g.load_snapshot().unwrap_or_else(|e| panic!("{e}"));
        let r = run(g.name);
        let ours = stats_of(&r);
        let mut d = diff_stats(&snap.stats, &ours);
        for m in compare_meta(&snap.meta, Some(&meta_of(&r))) {
            d.push(format!("meta.{}: rust {:?} != luajit {:?}", m.field, m.actual, m.expected));
        }
        let unimpl = unimplemented(&r);
        let pinned: &[&str] = if g.name == "Dual Savior" {
            &DUAL_SAVIOR_UNIMPLEMENTED
        } else {
            &P5_UNIMPLEMENTED
        };
        if unimpl != pinned {
            d.push(format!("unimplemented: {unimpl:#?}"));
        }
        let neg_zero = ours
            .values()
            .filter(|v| matches!(v, Gv::Num(n) if *n == 0.0 && n.is_sign_negative()))
            .count();
        report.push(format!(
            "{}: stats {} (luajit {}), negZero {neg_zero}, diffs {}",
            g.name,
            ours.len(),
            snap.stats.len(),
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

/// The comparator is not vacuous on the estimations keys: a flipped bit, a dropped key and the sign of a zero are caught.
#[test]
fn comparator_is_not_vacuous() {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == "OccVortex").expect("golden");
    let snap = g.load_snapshot().unwrap();
    let mut ours = stats_of(&run(g.name));
    let Some(Gv::Num(ehp)) = ours.get("TotalEHP").cloned() else {
        panic!("TotalEHP")
    };
    ours.insert("TotalEHP".into(), Gv::Num(f64::from_bits(ehp.to_bits() + 1)));
    ours.remove("SecondMinimalMaximumHitTaken");
    let zero = snap
        .neg_zero
        .as_ref()
        .and_then(|z| z.first())
        .expect("a -0 key in the golden")
        .clone();
    ours.insert(zero, Gv::Num(0.0));
    let d = diff_stats(&snap.stats, &ours);
    assert_eq!(d.len(), 3, "{d:?}");
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// `tools/.scratch/<file>` of the R4.1 bench, if present.
fn scratch(file: &str) -> Option<serde_json::Value> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/.scratch")
        .join(file);
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// DoD perf: the full `calculate_file` (load + initEnv + perform with defence/estimations + EstimatePlayerProgress, default
/// options) per golden, median of 15 runs after 3 warm-up runs (ms), next to LuaJIT `buildOutput` with `wipeGlobalCache`
/// (`tools/.scratch/perf-r4.json`, `export-r4.ts perf`).
/// `cargo test -p pob-calc --release --test full_fixture -- --ignored --nocapture perf`.
#[test]
#[ignore = "timing run; release build"]
fn perf_full_calculate() {
    let lj = scratch("perf-r4.json");
    println!("| build | Rust total | load | initEnv | perform+def+est | estimate | LuaJIT buildOutput (wipeCache) |");
    println!("|---|---|---|---|---|---|---|");
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
        let ljv = lj.as_ref().and_then(|j| j["builds"][g.name]["wipeCache_median_ms"].as_f64());
        println!(
            "| {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.4} | {} |",
            g.name,
            median(total),
            median(load),
            median(init),
            median(perf),
            median(est),
            ljv.map_or("-".to_owned(), |x| format!("{x:.2}")),
        );
    }
}
