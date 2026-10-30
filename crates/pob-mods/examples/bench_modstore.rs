//! The R2.1 ModStore perf measurement on the Rust core (`cargo run -p pob-mods --release --example bench_modstore`):
//! fuzz cases seeds 1..50 (`tests/fixtures/modstore-fuzz-01.json`), the same two numbers `tools/.scratch/perf-modstore.json`
//! has for LuaJIT and pob-ts:
//!  * full: build stores + ops + all queries (+ result serialisation) per case -- us/case;
//!  * queries only: the Sum/More/Flag queries via `run_query` (cfg build + query + serialisation) on prebuilt stores -- ns/query.
//!
//! Each run loops for >= 200 ms; one warm-up run is discarded; 7 runs, median [min - max].

#[path = "../tests/harness.rs"]
mod harness;

use std::hint::black_box;
use std::time::{Duration, Instant};

use harness::{build_case, build_cfg, run_case, run_query};
use pob_mods::ModType;
use serde_json::Value as J;

const RUNS: usize = 7;
const MIN: Duration = Duration::from_millis(200);

/// Seconds per pass of `f`, looping for at least `MIN`.
fn timed(mut f: impl FnMut()) -> f64 {
    let t = Instant::now();
    let mut passes = 0u32;
    loop {
        f();
        passes += 1;
        if t.elapsed() >= MIN {
            break;
        }
    }
    t.elapsed().as_secs_f64() / f64::from(passes)
}

fn stats(mut v: Vec<f64>) -> String {
    v.sort_by(f64::total_cmp);
    format!("{:.2} [{:.2} - {:.2}]", v[v.len() / 2], v[0], v[v.len() - 1])
}

fn main() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/modstore-fuzz-01.json");
    let fx: J = serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("json");
    let cases: Vec<&J> = fx["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|e| &e["case"])
        .filter(|c| {
            c["id"]
                .as_str()
                .and_then(|id| id.strip_prefix("fuzz#"))
                .and_then(|n| n.parse::<u32>().ok())
                .is_some_and(|n| n <= 50)
        })
        .collect();
    let queries: usize = cases.iter().map(|c| c["queries"].as_array().map_or(0, Vec::len)).sum();
    let built: Vec<_> = cases.iter().map(|c| build_case(c)).collect();
    let selected: Vec<(usize, &J)> = cases
        .iter()
        .enumerate()
        .flat_map(|(i, c)| c["queries"].as_array().into_iter().flatten().map(move |q| (i, q)))
        .filter(|(_, q)| matches!(q["method"].as_str(), Some("Sum" | "More" | "Flag")))
        .collect();
    println!(
        "cases {}, queries {queries}, selected Sum/More/Flag {}",
        cases.len(),
        selected.len()
    );

    let full = || {
        for c in &cases {
            black_box(run_case(c));
        }
    };
    let only = || {
        for (i, q) in &selected {
            black_box(run_query(q, &built[*i]));
        }
    };
    // Core only: cfg and names pre-built, no serialisation -- separates the harness cost from the store's.
    let prepared: Vec<_> = selected
        .iter()
        .map(|(i, q)| {
            let names: Vec<&str> = q["names"]
                .as_array()
                .map_or_else(Vec::new, |a| a.iter().map(|n| n.as_str().unwrap()).collect());
            let ty = q["type"].as_str().and_then(ModType::parse);
            (*i, q["method"].as_str().unwrap(), ty, build_cfg(&q["cfg"]), names)
        })
        .collect();
    let core = || {
        for ((i, method, ty, cfg, names), (_, q)) in prepared.iter().zip(&selected) {
            let store = built[*i].stores.get(q["store"].as_str().unwrap()).unwrap().borrow();
            let cfg = cfg.as_ref();
            let r = match *method {
                "Sum" => store.sum(ty.unwrap(), cfg, names).map(|_| ()),
                "More" => store.more(cfg, names).map(|_| ()),
                _ => store.flag(cfg, names).map(|_| ()),
            };
            black_box(r).ok();
        }
    };
    timed(full); // warm-up
    let full_runs: Vec<f64> = (0..RUNS).map(|_| timed(full) * 1e6 / cases.len() as f64).collect();
    timed(only);
    let q_runs: Vec<f64> = (0..RUNS).map(|_| timed(only) * 1e9 / selected.len() as f64).collect();
    timed(core);
    let c_runs: Vec<f64> = (0..RUNS).map(|_| timed(core) * 1e9 / selected.len() as f64).collect();
    println!("Sum/More/Flag core only (prebuilt cfg, no ser): {} ns/query", stats(c_runs));
    println!("full: {} us/case   runs {full_runs:.2?}", stats(full_runs.clone()));
    println!("Sum/More/Flag: {} ns/query   runs {q_runs:.1?}", stats(q_runs.clone()));
}
