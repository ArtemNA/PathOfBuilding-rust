//! R3.4 gate on the R3.1 fixtures (`crates/pob-calc/tests/fixtures/<build>.{luajit,pobts}.json`):
//!  * `applyRange`: every case (the `{range:}` lines of the 6 goldens + the 23 synthetic ones) = LuaJIT, string for string;
//!  * `items`: the Rust dump in the shape of the TS half of `ITEM_DUMP` (pob-ts `test/items/oracle.test.ts:303-362`) is
//!    EQUAL to pob-ts' dump; against LuaJIT, every difference falls in a pob-ts class (`item-miss`: the item has a ModCache
//!    miss; `tree-jewel`: `jewelData.funcList`), with the R3.1 counters -- all of this in `ParseMode::CacheOnly` (pob-ts);
//!  * R5.4, `ParseMode::Full` (the desktop: ModCache, else the real ModParser): `applyRange` = LuaJIT, and the items against
//!    LuaJIT with `item-miss` = 0 -- the only class left is `tree-jewel` (R5.5).

#[path = "support/dump.rs"]
mod dump;

use dump::{GoldenRun, Json, diff_exact, fixture, items_vs_luajit, items_vs_luajit_with, run_golden, run_golden_mode};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_items::apply_range;
use pob_parser::ParseMode;

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
    apply_range_against_luajit(ParseMode::CacheOnly);
}

/// R5.4: the same cases with the real parser behind `applyRange` (D18 now reads like the desktop).
#[test]
fn apply_range_full_equals_luajit_line_for_line() {
    apply_range_against_luajit(ParseMode::Full);
}

fn apply_range_against_luajit(mode: ParseMode) {
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
                mode,
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

/// Class `pob-ts-no-mutatedLines` (R5.5d): (build, item id) whose CacheOnly dump differs from pob-ts only by `rangeLineCount`.
const MUTATED_LINES_ITEMS: [(&str, usize); 9] = [
    ("Dual Wield Cospris CoC", 17),
    ("Dual Wield Cospris CoC", 24),
    ("Dual Wield Cospris CoC", 25),
    ("Dual Wield Cospris CoC", 28),
    ("Dual Wield Cospris CoC - Lethal Pride", 17),
    ("Dual Wield Cospris CoC - Lethal Pride", 24),
    ("Dual Wield Cospris CoC - Lethal Pride", 25),
    ("Dual Wield Cospris CoC - Lethal Pride", 28),
    ("Generals Perforate Zerker", 20),
];

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
        // `pob-ts-no-mutatedLines`: pob-ts never ported Item.lua:1551-1600, so the foulborn items it sees have
        // `rangeLineCount` one short (no `modId`/`newModId` line); pinned item by item, nothing else may differ.
        let pinned: Vec<String> = MUTATED_LINES_ITEMS
            .iter()
            .filter(|(b, _)| *b == g.name)
            .map(|(_, id)| format!("items.{id}.rangeLineCount: pob-ts \"0\", rust \"1\""))
            .collect();
        let before = d.len();
        d.retain(|x| !pinned.contains(x));
        assert_eq!(
            before - d.len(),
            pinned.len(),
            "{}: pob-ts-no-mutatedLines pin not fully hit",
            g.name
        );
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

/// R5.4 gate (`ParseMode::Full`) against LuaJIT: no item has an unanswered line (`item-miss` = 0); what is left is
/// `tree-jewel` (`jewelData.funcList`: the radius-jewel functions, R5.5) and the two gaps `item-miss` used to hide:
/// `mutated-lines` (Item.lua:1543, P5 marker on the item) and `nested-table` (probe encoding of `conqueredBy.conqueror`).
#[test]
fn items_full_against_luajit_item_miss_zero() {
    // (build, items, tree-jewel, mutated-lines, nested-table) -- measured
    const PINNED: [(&str, usize, usize, usize, usize); 6] = [
        ("Dual Savior", 3, 0, 0, 0),
        ("Dual Wield Cospris CoC", 28, 1, 0, 0),
        ("Dual Wield Cospris CoC - Lethal Pride", 28, 1, 0, 1),
        ("Generals Perforate Zerker", 29, 0, 0, 0),
        ("Mirage Archer Toxic Rain", 22, 0, 0, 0),
        ("OccVortex", 18, 0, 0, 0),
    ];
    let mut report = Vec::new();
    let mut failures = Vec::new();
    for (g, (name, n_items, tree_jewel, mutated_lines, nested_table)) in GOLDEN_BUILDS.iter().zip(PINNED) {
        assert_eq!(g.name, name);
        let GoldenRun { dump, misses, mutated, .. } = run_golden_mode(g, ParseMode::Full);
        let lj = fixture(g.name, "luajit");
        let r = items_vs_luajit_with(&lj["items"], &dump, &misses, &mutated);
        report.push(format!(
            "{}: items {}, lines {}, clean {}, item-miss {} ({} lines), tree-jewel {}, mutated-lines {}, nested-table {}",
            g.name, r.items, r.lines, r.clean, r.with_miss, r.miss_lines, r.tree_jewel, r.mutated_lines, r.nested_table
        ));
        if !r.violations.is_empty() {
            failures.push(format!(
                "{}: {} unclassified, first: {}",
                g.name,
                r.violations.len(),
                r.violations[..r.violations.len().min(10)].join("\n  ")
            ));
        }
        let got = (
            r.items,
            r.clean,
            r.with_miss,
            r.miss_lines,
            r.tree_jewel,
            r.mutated_lines,
            r.nested_table,
        );
        let want = (n_items, n_items, 0, 0, tree_jewel, mutated_lines, nested_table);
        if got != want {
            failures.push(format!(
                "{}: (items, clean, item-miss, miss lines, tree-jewel, mutated-lines, nested-table) = {got:?}, want {want:?}",
                g.name
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
