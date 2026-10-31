//! pob-cli library: the `calc` entry that produces the oracle candidate output, and the `bench` timing loop.
//!
//! R4.5: XML -> [`pob_calc::calculate`] (initEnv + perform + defence + estimations) -> `stats` filtered by the measured
//! [`pob_calc::ALLOWLIST`] (like pob-ts `candidate.ts`); `POB_EXPERIMENTAL=1` publishes the unfiltered output.
//! R5.4: mod lines are read like the desktop (`ParseMode::Full`, ModCache then the real ModParser) unless `--cache-only`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::time::Instant;

pub use pob_calc::ParseMode;
use pob_calc::{CalculateOptions, CalculateResult, StatValue, Timings, calculate_file, calculate_with, filter_stats};
use pob_golden::{CandidateOutput, PartialMeta, Stats, value};

/// `POB_EXPERIMENTAL=1`: skip the allowlist (how a key is measured before it may be claimed).
pub fn experimental_from_env() -> bool {
    std::env::var("POB_EXPERIMENTAL").is_ok_and(|v| v == "1")
}

/// A Lua error upstream would raise (L18) is a panic in the port; it is caught and reported as an error (exit 2), never
/// as a half-filled output.
fn guarded<T>(what: &str, f: impl FnOnce() -> Result<T, pob_calc::CalcError>) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f))
        .map_err(|p| {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".into());
            format!("{what}: {msg}")
        })?
        .map_err(|e| format!("{what}: {e}"))
}

fn to_value(v: &StatValue) -> value::StatValue {
    match v {
        StatValue::Num(n) => value::StatValue::Num(*n),
        StatValue::Str(s) => value::StatValue::Str(s.clone()),
        StatValue::Bool(b) => value::StatValue::Bool(*b),
    }
}

/// The candidate output of a finished calculation. `experimental` = no allowlist filter.
pub fn candidate_output(res: &CalculateResult, experimental: bool) -> CandidateOutput {
    let m = &res.meta;
    let as_u32 = |n: usize| u32::try_from(n).expect("count fits u32");
    let meta = PartialMeta {
        alloc_nodes: Some(as_u32(m.alloc_nodes)),
        items: Some(as_u32(m.items)),
        socket_groups: Some(as_u32(m.socket_groups)),
        // A clamped `min(max(#groups, 1), mainSocketGroup)`: an integer whenever the XML's is.
        main_socket_group: (m.main_socket_group.fract() == 0.0).then_some(m.main_socket_group as i64),
        tree_version: Some(m.tree_version.clone()),
        class_name: Some(m.class_name.clone()),
        ascend_name: Some(m.ascend_name.clone()),
    };
    let stats: Stats = if experimental {
        res.stats.iter().map(|(k, v)| (k.clone(), to_value(v))).collect()
    } else {
        filter_stats(&res.stats).iter().map(|(k, v)| (k.clone(), to_value(v))).collect()
    };
    CandidateOutput {
        stats,
        meta: Some(meta),
        unimplemented: res.unimplemented.clone(),
    }
}

/// Computes the build output from the build text as given (the oracle adapter pipes the file's content on stdin).
pub fn calc_with(xml: &str, experimental: bool) -> Result<CandidateOutput, String> {
    calc_mode(xml, experimental, ParseMode::Full)
}

/// [`calc_with`] with an explicit [`ParseMode`].
pub fn calc_mode(xml: &str, experimental: bool, mode: ParseMode) -> Result<CandidateOutput, String> {
    if xml.is_empty() {
        return Err("empty XML".into());
    }
    let opts = CalculateOptions {
        parse_mode: mode,
        ..CalculateOptions::default()
    };
    let res = guarded("calculate", || calculate_with(xml, opts))?;
    Ok(candidate_output(&res, experimental))
}

/// [`calc_with`] honouring `POB_EXPERIMENTAL`.
pub fn calc(xml: &str) -> Result<CandidateOutput, String> {
    calc_with(xml, experimental_from_env())
}

/// One build's timing over `runs` full `calculate_file` calls (after one untimed warm-up), milliseconds.
#[derive(Clone, Debug)]
pub struct BenchRow {
    pub total: Vec<f64>,
    pub load: Vec<f64>,
    pub init_env: Vec<f64>,
    pub perform: Vec<f64>,
    pub estimate: Vec<f64>,
    /// `calcs.buildOutput(MAIN)` = initEnv + perform above (every nested run included).
    pub build_output: Vec<f64>,
    /// Every initEnv of the buildOutput session, summed (R7-perf).
    pub init_env_all: Vec<f64>,
    /// initEnv calls per buildOutput (0 on the pre-R6 path, which has no session).
    pub init_env_calls: usize,
}

/// Median / min / max of a non-empty sample (median of an even count = mean of the two middle values).
pub fn median_min_max(samples: &[f64]) -> (f64, f64, f64) {
    let mut s = samples.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    let med = if n % 2 == 1 { s[n / 2] } else { (s[n / 2 - 1] + s[n / 2]) / 2.0 };
    (med, s[0], s[n - 1])
}

/// Full `calculate_file` (read file + load + initEnv + perform/defence/estimations + EstimatePlayerProgress), no process start.
pub fn bench(path: &Path, runs: usize, mode: ParseMode) -> Result<BenchRow, String> {
    assert!(runs > 0);
    let opts = CalculateOptions {
        parse_mode: mode,
        ..CalculateOptions::default()
    };
    guarded("bench warm-up", || calculate_file(path, opts))?;
    let mut row = BenchRow {
        total: vec![],
        load: vec![],
        init_env: vec![],
        perform: vec![],
        estimate: vec![],
        build_output: vec![],
        init_env_all: vec![],
        init_env_calls: 0,
    };
    for _ in 0..runs {
        let t = Instant::now();
        let res = guarded("bench", || calculate_file(path, opts))?;
        row.total.push(t.elapsed().as_secs_f64() * 1000.0);
        let Timings {
            load_ms,
            init_env_ms,
            perform_ms,
            estimate_ms,
        } = res.timings;
        row.load.push(load_ms);
        row.init_env.push(init_env_ms);
        row.perform.push(perform_ms);
        row.estimate.push(estimate_ms);
        row.build_output.push(init_env_ms + perform_ms);
        if let Some(log) = &res.session_log {
            row.init_env_all.push(log.init_env_ms.iter().sum());
            row.init_env_calls = log.init_env_calls;
        }
    }
    Ok(row)
}
