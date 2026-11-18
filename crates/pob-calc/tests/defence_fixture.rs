//! R4.3 (defence slice): `calcs.hitChance` / `calcs.armourReduction` edge cases and the timing of `calcs.defence` alone.
//!
//! Until R-INDEPENDENT-UPDATE A4 this file also gated `calculate` with the real `defence` and a stub estimations on the 6 goldens
//! against pob-ts (`<build>.defence-pobts.json`, `ParseMode::CacheOnly`, no skills; ignored since U-R6, pob-ts was the 2.67.2
//! reference). That slice is a pob-ts mode LuaJIT has no twin of; `calcs.defence` is gated against LuaJIT by `full_fixture.rs` (the
//! whole `mainOutput` of the 6 goldens, bit for bit), `defence_synth.rs` / `defence_synth2.rs` (the R4 synthetic scenarios) and
//! `real_builds.rs` (the build corpus).

use std::cell::RefCell;

use pob_calc::{CalculateOptions, CalculateResult, Env, PerformState, Who, calculate_file, defence, hit_chance, noop_phase};
use pob_golden::registry::GOLDEN_BUILDS;

fn run_with(name: &str, o: CalculateOptions) -> CalculateResult {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden");
    calculate_file(g.xml_path(), o).unwrap_or_else(|e| panic!("{name}: {e}"))
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
        ..CalculateOptions::default()
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
