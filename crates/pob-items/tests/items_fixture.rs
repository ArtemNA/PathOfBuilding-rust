//! R3.4 gate on the R3.1 fixtures (`crates/pob-calc/tests/fixtures/<build>.{luajit,pobts}.json`):
//!  * `applyRange`: every case (the `{range:}` lines of the 6 goldens + the 23 synthetic ones) = LuaJIT, string for string;
//!  * `items`: the Rust dump in the shape of the TS half of `ITEM_DUMP` (pob-ts `test/items/oracle.test.ts:303-362`) is
//!    EQUAL to pob-ts' dump; against LuaJIT, every difference falls in a pob-ts class (`item-miss`: the item has a ModCache
//!    miss; `tree-jewel`: `jewelData.funcList`), with the R3.1 counters.

#[path = "support/dump.rs"]
mod dump;

use dump::{GoldenRun, Json, diff_exact, fixture, items_vs_luajit, run_golden};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_items::apply_range;

/// R3.1 report: (build, items, clean, with miss, distinct miss lines).
const EXPECTED: [(&str, usize, usize, usize, usize); 6] = [
    ("Dual Savior", 3, 2, 1, 1),
    ("Dual Wield Cospris CoC", 28, 0, 28, 113),
    ("Dual Wield Cospris CoC - Lethal Pride", 28, 0, 28, 117),
    ("Generals Perforate Zerker", 29, 2, 27, 127),
    ("Mirage Archer Toxic Rain", 22, 1, 21, 83),
    ("OccVortex", 18, 2, 16, 76),
];

fn opt_num(j: &Json, k: &str) -> Option<f64> {
    j.get(k).and_then(Json::as_f64)
}

#[test]
fn apply_range_equals_luajit_line_for_line() {
    let (mut total, mut bad) = (0, Vec::new());
    for g in GOLDEN_BUILDS {
        let lj = fixture(g.name, "luajit");
        let ts = fixture(g.name, "pobts");
        let cases = lj["applyRange"]["cases"].as_array().expect("cases");
        let lj_results = lj["applyRange"]["results"].as_array().map(Vec::as_slice).unwrap_or_default();
        let ts_results = ts["applyRange"]["results"].as_array().map(Vec::as_slice).unwrap_or_default();
        assert_eq!(
            ts["applyRange"]["cases"], lj["applyRange"]["cases"],
            "{}: the same cases on both sides",
            g.name
        );
        for (i, c) in cases.iter().enumerate() {
            total += 1;
            let line = c["line"].as_str().expect("line");
            let got = apply_range(
                line,
                c["range"].as_f64().expect("range"),
                opt_num(c, "valueScalar"),
                opt_num(c, "baseValueScalar"),
            );
            let want = lj_results[i].as_str().expect("luajit result");
            if got != want {
                bad.push(format!(
                    "{} #{i}: applyRange({line:?}, {}) = {got:?}, LuaJIT {want:?}",
                    g.name, c["range"]
                ));
            }
            assert_eq!(
                ts_results[i].as_str(),
                Some(want),
                "{}: the fixture's pob-ts half agrees with LuaJIT",
                g.name
            );
        }
    }
    assert!(bad.is_empty(), "{} of {total} cases differ:\n{}", bad.len(), bad.join("\n"));
    assert_eq!(total, 117, "94 `{{range:}}` lines + 23 synthetic cases");
}

#[test]
fn items_equal_pob_ts_and_classified_against_luajit() {
    let mut report = Vec::new();
    let mut failures = Vec::new();
    for (g, (name, n_items, clean, with_miss, miss_lines)) in GOLDEN_BUILDS.iter().zip(EXPECTED) {
        assert_eq!(g.name, name);
        let GoldenRun { dump, misses, .. } = run_golden(g);
        let ts = fixture(g.name, "pobts");
        let lj = fixture(g.name, "luajit");
        // (a) = pob-ts, exactly
        let mut d = Vec::new();
        diff_exact("items", &ts["items"], &dump, &mut d);
        if !d.is_empty() {
            failures.push(format!(
                "{}: {} differences from pob-ts, first: {}",
                g.name,
                d.len(),
                d[..d.len().min(10)].join("\n  ")
            ));
        }
        // (b) LuaJIT, classified
        let r = items_vs_luajit(&lj["items"], &dump, &misses);
        report.push(format!(
            "{}: items {}, lines {}, clean {}, item-miss {} ({} lines), tree-jewel {}",
            g.name, r.items, r.lines, r.clean, r.with_miss, r.miss_lines, r.tree_jewel
        ));
        if !r.violations.is_empty() {
            failures.push(format!(
                "{}: {} unclassified, first: {}",
                g.name,
                r.violations.len(),
                r.violations[..r.violations.len().min(10)].join("\n  ")
            ));
        }
        if (r.items, r.clean, r.with_miss, r.miss_lines) != (n_items, clean, with_miss, miss_lines) {
            failures.push(format!(
                "{}: counters (items, clean, with miss, miss lines) = {:?}, R3.1 {:?}",
                g.name,
                (r.items, r.clean, r.with_miss, r.miss_lines),
                (n_items, clean, with_miss, miss_lines)
            ));
        }
    }
    println!("{}", report.join("\n"));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The parse is deterministic and leaves nothing behind: the same build twice in one process gives the same dump.
#[test]
fn second_parse_is_identical() {
    let g = &GOLDEN_BUILDS[4];
    let a = run_golden(g).dump;
    let b = run_golden(g).dump;
    assert_eq!(a, b);
}

/// Guard against a vacuous gate: the comparators do see the known LuaJIT <-> pob-ts differences (OccVortex flask 12
/// `gainMod`: a ModCache miss on its suffix), the dump covers every item, and every FindModifierSubstring probe splits the
/// 128 items (oracle.test.ts:513).
#[test]
fn comparators_are_not_vacuous() {
    let g = &GOLDEN_BUILDS[5];
    let lj = fixture(g.name, "luajit");
    let run = run_golden(g);
    let mut d = Vec::new();
    diff_exact("items", &lj["items"], &run.dump, &mut d);
    assert!(d.iter().any(|p| p.contains("items.12.flaskData.gainMod")), "{d:?}");
    let mut probes = [0usize; 15];
    let mut total = 0;
    for g in GOLDEN_BUILDS {
        let run = run_golden(&g);
        for item in run.dump.as_object().expect("object").values() {
            total += 1;
            for (i, c) in item["findModifierSubstring"].as_str().expect("fms").chars().enumerate() {
                probes[i] += usize::from(c == '1');
            }
        }
    }
    assert_eq!(total, 128);
    assert!(probes.iter().all(|&p| p > 0 && p < total), "{probes:?}");
}
