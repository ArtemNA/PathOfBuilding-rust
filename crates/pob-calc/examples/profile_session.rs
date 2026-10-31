//! R7-perf: where a `calcs.buildOutput(MAIN)` session spends its time, release build.
//!
//!   cargo run --release -p pob-calc --example profile_session [-- RUNS]
//!
//! Per golden: RUNS sessions (+1 untimed warm-up) of `CalcSession::new` + `build_output(MAIN)` on a model loaded once
//! (the build file is read and parsed untimed). Prints the median buildOutput wall time, the initEnv count, and initEnv by
//! phase (`pob_calc::init_env::profile`): mean ms per initEnv call over every initEnv of the RUNS sessions.

use std::rc::Rc;
use std::time::Instant;

use pob_calc::init_env::profile;
use pob_calc::{CalcMode, CalcSession, CalculateOptions};
use pob_golden::registry::GOLDEN_BUILDS;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

fn main() {
    let runs: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(10);
    for g in GOLDEN_BUILDS {
        let xml = pob_build::read_build_file(g.xml_path()).expect("read");
        let model = Rc::new(pob_build::load_build_xml(&xml).expect("xml"));
        let opts = CalculateOptions::default();
        let one = || {
            let t = Instant::now();
            let s = CalcSession::new(model.clone(), opts);
            let out = s.build_output(CalcMode::Main).expect("buildOutput");
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            let log = s.log();
            let init_sum: f64 = log.init_env_ms.iter().sum();
            let perform_top = log.perform.iter().filter(|p| p.top).map(|p| p.ms).sum::<f64>();
            let r = (ms, log.init_env_calls, init_sum, log.perform.len(), perform_top);
            drop(log);
            drop(out);
            r
        };
        one();
        let _ = profile::take();
        profile::enable(true);
        let mut total = Vec::new();
        let mut init = Vec::new();
        let mut calls = 0;
        let mut performs = 0;
        for _ in 0..runs {
            let (ms, n, i, p, _) = one();
            total.push(ms);
            init.push(i);
            calls = n;
            performs = p;
        }
        profile::enable(false);
        let phases = profile::take();
        let n_init = (calls * runs) as f64;
        println!(
            "{}: buildOutput {:.2} ms median | initEnv x{} = {:.2} ms ({:.3} ms each) | perform x{}",
            g.name,
            median(total),
            calls,
            median(init.clone()),
            median(init) / calls as f64,
            performs
        );
        for (phase, ms, _) in phases {
            println!("    {phase:<48} {:.3} ms/initEnv", ms / n_init);
        }
    }
}
