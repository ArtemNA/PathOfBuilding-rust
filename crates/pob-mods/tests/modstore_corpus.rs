//! pob-mods against LuaJIT: every `tests/fixtures/modstore-*.json` corpus (cases of pob-ts `test/mods/cases*.ts` and the
//! fuzzer `tools/oracle/modstore-fuzz.ts`, answered by the LuaJIT docker with the real PoB loaded -- task R2.1) is replayed
//! through `harness.rs` and compared with `compareRuns` rules: numbers bit for bit (NaN == NaN), errors by text. Zero
//! mismatches is the gate.

mod harness;

use std::path::PathBuf;

use harness::{O, expected, run_case, same};
use serde_json::Value as J;

fn fixtures() -> Vec<PathBuf> {
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e} -- the corpora come from task R2.1", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("modstore-") && n.ends_with(".json"))
        })
        .collect();
    files.sort();
    files
}

struct Tally {
    cases: usize,
    queries: usize,
    errors: usize,
    mismatches: Vec<String>,
}

fn replay(path: &PathBuf) -> Tally {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let fx: J = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(fx["format"].as_f64(), Some(1.0), "{}: fixture format", path.display());
    let mut t = Tally {
        cases: 0,
        queries: 0,
        errors: 0,
        mismatches: Vec::new(),
    };
    for entry in fx["cases"].as_array().expect("cases") {
        let case = &entry["case"];
        let want: Vec<O> = entry["results"].as_array().expect("results").iter().map(expected).collect();
        let got = run_case(case);
        assert_eq!(want.len(), got.len(), "{}: result count", case["id"]);
        t.cases += 1;
        t.queries += want.len();
        for (i, (w, g)) in want.iter().zip(&got).enumerate() {
            if matches!(w, O::Err(_)) {
                t.errors += 1;
            }
            if !same(w, g) {
                t.mismatches.push(format!(
                    "{} query #{i} {}:\n  LuaJIT {w:?}\n  Rust   {g:?}",
                    case["id"], case["queries"][i]
                ));
            }
        }
    }
    t
}

#[test]
fn modstore_corpora_match_luajit() {
    let files = fixtures();
    assert!(
        !files.is_empty(),
        "no tests/fixtures/modstore-*.json -- the corpora come from task R2.1"
    );
    let (mut fuzz_queries, mut total_mismatches) = (0, 0);
    let mut report = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let t = replay(path);
        if name.starts_with("modstore-fuzz") {
            fuzz_queries += t.queries;
        }
        eprintln!(
            "{name}: {} cases, {} queries ({} error answers), {} mismatches",
            t.cases,
            t.queries,
            t.errors,
            t.mismatches.len()
        );
        total_mismatches += t.mismatches.len();
        report.extend(t.mismatches.into_iter().map(|m| format!("[{name}] {m}")));
    }
    eprintln!("fuzz total: {fuzz_queries} queries");
    for m in report.iter().take(40) {
        eprintln!("{m}");
    }
    assert_eq!(total_mismatches, 0, "{total_mismatches} mismatches against LuaJIT (first 40 above)");
    assert!(
        fuzz_queries >= 20_000,
        "fuzz corpus has only {fuzz_queries} queries (DoD: >= 20 000)"
    );
}
