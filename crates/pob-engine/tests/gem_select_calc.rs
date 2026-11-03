//! U5-perf gate: a `GemSelectControl` candidate and the tab's other gem writes in the session of `GetMiscCalculator` ([`Engine::gem_edit_run`]):
//! - against the original's own numbers, every scalar of `output` to the bit (`gem_edits_equal_the_original`; `tools/sg-oracle/run.ps1 -Script skills-runner.lua -ScriptArgs u5probe`);
//! - against itself in another order of the candidates (`pairs(self.gems)`, R13);
//! - against the session over the edited model ([`Engine::gem_edit_run_own_session`], the path before U5-perf): the differences are listed (`#[ignore]`).

use std::collections::BTreeMap;

use pob_calc::output::OutValue;
use pob_calc::session::RunRef;
use pob_engine::engine::Engine;
use pob_engine::skills_tab::GemEdit;
use serde_json::Value as Json;

const GOLDENS: [&str; 6] = [
    "Dual Savior",
    "Dual Wield Cospris CoC",
    "Dual Wield Cospris CoC - Lethal Pride",
    "Generals Perforate Zerker",
    "Mirage Archer Toxic Rain",
    "OccVortex",
];

fn engine(name: &str) -> Engine {
    let g = pob_golden::registry::find_golden(name).expect("golden");
    let e = Engine::load_xml(&std::fs::read_to_string(g.xml_path()).expect("xml")).expect("load");
    // the tab calculates with its own view mode, not TREE
    e.view_mode_is_tree.set(false);
    e
}

/// `sortCache.pendingGems` of the control over all gems of the data (the control filters them by the tab's options).
fn pending(e: &Engine, group: usize) -> Vec<String> {
    let skills = pob_data::skills::skills();
    let data = pob_data::gems::gems();
    let ids: Vec<&str> = data.gems.0.keys().map(String::as_str).collect();
    let can = e.gem_can_support(group, &ids, false).expect("canSupport");
    ids.iter()
        .filter(|id| {
            let ge = skills.get(&data.gems.0[**id].granted_effect_id).expect("effect");
            can.contains(**id) || (ge.has_global_effect.get() && !ge.support)
        })
        .map(|id| (*id).to_owned())
        .collect()
}

fn scalars(run: &RunRef) -> BTreeMap<String, String> {
    let r = run.borrow();
    r.state
        .player
        .output
        .scalars()
        .into_iter()
        .map(|(k, v)| (k.to_string(), format!("{v:?}")))
        .collect()
}

fn diff(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    for (k, v) in a {
        match b.get(k) {
            Some(w) if w == v => {}
            w => out.push(format!("{k}: {v} vs {w:?}")),
        }
    }
    for k in b.keys() {
        if !a.contains_key(k) {
            out.push(format!("{k}: nil vs {}", b[k]));
        }
    }
    out
}

fn candidate(id: &str) -> GemEdit {
    GemEdit::Candidate {
        gem_id: id.to_owned(),
        imbued: false,
    }
}

fn oracle_probe(name: &str) -> Option<Json> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../tools/sg-oracle/fixtures/skills/u5probe-{name}.json"));
    Some(serde_json::from_str(&std::fs::read_to_string(p).ok()?).expect("json"))
}

/// The scalars of `output` that differ from the record of the original (`%.17g` of a number: the bits).
fn against_oracle(run: &RunRef, rec: &Json) -> Vec<String> {
    let r = run.borrow();
    let o = &r.state.player.output;
    let mut out = Vec::new();
    let rec = rec.as_object().expect("output");
    let ours = o.scalars();
    for (k, v) in rec {
        let want = v.as_str().expect("string");
        let same = match ours.get(k.as_str()) {
            Some(OutValue::Num(n)) => want
                .parse::<f64>()
                .is_ok_and(|w| w.to_bits() == n.to_bits() || (w.is_nan() && n.is_nan())),
            Some(OutValue::Bool(b)) => want == b.to_string(),
            Some(OutValue::Str(s)) => want == &**s,
            None => false,
        };
        if !same {
            out.push(format!("{k}: original {want}, ours {:?}", ours.get(k.as_str())));
        }
    }
    for (k, v) in &ours {
        if !rec.contains_key(&**k) {
            out.push(format!("{k}: original nil, ours {v:?}"));
        }
    }
    out
}

/// The other processes of the original on `name`: `u5probe-<name>~<n>.json` (R13: the sample of what `pairs(guards)` gives).
fn process_samples(name: &str) -> Vec<Json> {
    (2..).map_while(|n| oracle_probe(&format!("{name}~{n}"))).collect()
}

/// The row of `sample` for the same `calcFunc`: a process walks `pairs(self.gems)` in an order of its own.
fn same_row<'a>(sample: &'a Json, list: &str, rw: &Json) -> Option<&'a Json> {
    let key = ["group", "slot", "gem", "edit"];
    sample[list].as_array()?.iter().find(|o| key.iter().all(|k| o[*k] == rw[*k]))
}

/// One row of the record: `(group, slot, edit, use_full_dps, label)`. `self.gems` is keyed `Default:<id>`; `gemChangeFunc` strips it (GemSelectControl.lua:469).
fn row(rec: &Json) -> (usize, usize, GemEdit, Option<bool>, String) {
    let (g, s) = (
        rec["group"].as_u64().expect("g") as usize,
        rec["slot"].as_u64().expect("s") as usize,
    );
    match rec["gem"].as_str() {
        Some(id) => {
            let id = id.trim_start_matches("Default:");
            (g, s, candidate(id), Some(false), format!("g{g} s{s} {id}"))
        }
        None if rec["edit"] == "quality" => (g, s, GemEdit::Quality20, None, format!("g{g} s{s} quality")),
        None => (g, s, GemEdit::ToggleEnabled, None, format!("g{g} s{s} enable")),
    }
}

/// `pairs(guards)` (CalcPerform.lua:3320) chose among 2+ non-Vaal guards in this run: which one gets the slot differs between processes of the original (R13; Rust takes the insertion order).
fn guard_order_decides(run: &RunRef) -> bool {
    let r = run.borrow();
    r.env
        .skills
        .as_ref()
        .is_some_and(|s| s.order_notes.iter().any(|n| n.contains("pairs(guards)")))
}

/// Every candidate of a group (on a gem's slot and on the empty slot) and the tab's two other writes on every gem, in the original's order: each scalar of `output` to the bit.
#[test]
fn gem_edits_equal_the_original() {
    let mut missing = 0;
    let mut failures = Vec::new();
    for name in GOLDENS {
        let Some(rec) = oracle_probe(name) else {
            missing += 1;
            continue;
        };
        let e = engine(name);
        let samples = process_samples(name);
        let (mut n, mut r13) = (0, Vec::new());
        for list in ["edits", "candidates"] {
            for rw in rec[list].as_array().expect("rows") {
                let (g, s, edit, full, label) = row(rw);
                let run = e.gem_edit_run(g, s, &edit, full).expect("calc");
                n += 1;
                let d = against_oracle(&run, &rw["output"]);
                if d.is_empty() {
                    continue;
                }
                // R13: only with a measurement -- the numbers must be those of another process of the original
                let measured = samples
                    .iter()
                    .filter_map(|s| same_row(s, list, rw))
                    .any(|o| against_oracle(&run, &o["output"]).is_empty());
                if guard_order_decides(&run) && measured {
                    r13.push(label);
                } else {
                    failures.push(format!("{name} {label}: {} keys, {}", d.len(), d[..d.len().min(4)].join("; ")));
                }
            }
        }
        eprintln!("{name}: {n} calcFunc calls; differ by the order of pairs(guards) (R13): {r13:?}");
    }
    if missing > 0 {
        eprintln!("SKIPPED {missing} builds: no fixtures/skills/u5probe-*.json");
    }
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures[..failures.len().min(30)].join("\n")
    );
}

/// `pairs(self.gems)` walks the candidates in an order of its own in each LuaJIT process (R13): a shared `GlobalCache` must not give a candidate other numbers in another order, or the
/// values of the list would depend on the process.
#[test]
fn candidate_values_do_not_depend_on_their_order() {
    let mut differ = Vec::new();
    let builds = if cfg!(debug_assertions) { &GOLDENS[..1] } else { &GOLDENS[..] };
    for name in builds {
        let (fw, bw) = (engine(name), engine(name));
        let main_group = (fw.main_socket_group() as usize).max(1);
        let group = if fw.socket_groups().get(main_group - 1).is_some_and(|g| g.source.is_none()) {
            main_group
        } else {
            fw.socket_groups().iter().position(|g| g.source.is_none()).expect("a group") + 1
        };
        let ids = pending(&fw, group);
        let values = |e: &Engine, list: &[&String]| -> BTreeMap<String, BTreeMap<String, String>> {
            list.iter()
                .map(|id| {
                    (
                        (*id).clone(),
                        scalars(&e.gem_edit_run(group, 1, &candidate(id), Some(false)).expect("calc")),
                    )
                })
                .collect()
        };
        let forward: Vec<&String> = ids.iter().collect();
        let backward: Vec<&String> = ids.iter().rev().collect();
        let (a, b) = (values(&fw, &forward), values(&bw, &backward));
        for id in &ids {
            let d = diff(&a[id], &b[id]);
            if !d.is_empty() {
                differ.push(format!("{name} {id}: {} keys, {}", d.len(), d[..d.len().min(3)].join("; ")));
            }
        }
        eprintln!("{name}: group {group}, {} candidates in two orders", ids.len());
    }
    assert!(
        differ.is_empty(),
        "{} candidates depend on the order:\n{}",
        differ.len(),
        differ.join("\n")
    );
}

/// A whole `BuildOutput` with these gems reads the `GlobalCache` entry of a detached main skill (`cached_sd`, `skills::DETACHED`): it used to index past the list.
#[test]
fn detached_main_skill_in_the_cache_does_not_panic() {
    let e = engine("Dual Savior");
    let data = pob_data::gems::gems();
    for name in ["Summon Holy Relic", "Prismatic Burst"] {
        let id = data.gems.0.iter().find(|(_, g)| g.name == name).expect("gem").0;
        e.gem_edit_run_own_session(1, 3, &candidate(id), Some(false)).expect("calc");
    }
}

/// The rows of the original's record on the two paths: what the old one got wrong. `--ignored --nocapture`.
#[test]
#[ignore = "lists the differences of the path before U5-perf"]
fn own_session_differences() {
    for name in GOLDENS {
        let Some(rec) = oracle_probe(name) else {
            eprintln!("SKIPPED {name}: no record");
            continue;
        };
        let e = engine(name);
        let (mut n, mut dps, mut other, mut wrong, mut panics) = (0, 0, 0, 0, Vec::new());
        let mut keys: BTreeMap<String, usize> = BTreeMap::new();
        let mut samples = Vec::new();
        for list in ["edits", "candidates"] {
            for rw in rec[list].as_array().expect("rows") {
                let (g, s, edit, full, label) = row(rw);
                n += 1;
                let a = e.gem_edit_run(g, s, &edit, full).expect("new");
                // the old path runs a whole MAIN `BuildOutput` of the edited build: it can stop at a boundary of the port the calculator does not reach
                let Ok(b) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    e.gem_edit_run_own_session(g, s, &edit, full).expect("old")
                })) else {
                    panics.push(label);
                    continue;
                };
                let d = diff(&scalars(&a), &scalars(&b));
                if !against_oracle(&b, &rw["output"]).is_empty() && !guard_order_decides(&b) {
                    wrong += 1;
                }
                if d.is_empty() {
                    continue;
                }
                if d.iter().any(|l| l.starts_with("CombinedDPS:") || l.starts_with("TotalDPS:")) {
                    dps += 1;
                } else {
                    other += 1;
                }
                for l in &d {
                    *keys.entry(l.split(':').next().expect("key").to_owned()).or_default() += 1;
                }
                if samples.len() < 3 && d.iter().any(|l| !l.starts_with("Req")) {
                    samples.push(format!("  {label}: {} keys, {}", d.len(), d[..d.len().min(3)].join("; ")));
                }
            }
        }
        let mut top: Vec<_> = keys.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1));
        top.truncate(8);
        eprintln!(
            "{name}: {n} rows; new vs old: DPS differs {dps}, other scalars only {other} {top:?}; old vs original wrong {wrong}; old panicked {panics:?}"
        );
        for s in samples {
            eprintln!("{s}");
        }
    }
}
