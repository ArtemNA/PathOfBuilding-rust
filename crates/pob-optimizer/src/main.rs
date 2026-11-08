//! Probe of the search: pob-optimizer probe <build.xml> [options]. The browser page comes later.

use std::process::ExitCode;

use pob_engine::engine::Engine;
use pob_optimizer::{Eval, Goal, Options, evaluate, optimize};

const USAGE: &str = "usage: pob-optimizer probe <build.xml> [--secs N] [--round N] [--threads N] [--seed N]
       [--dps-weight N] [--ehp-weight N] [--min-res N] [--min-chaos N] [--min-ehp N] [--out best.xml]";

fn value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn number<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> Result<T, String> {
    match value(args, name) {
        None => Ok(default),
        Some(v) => v.parse().map_err(|_| format!("{name}: not a number: {v}")),
    }
}

fn pct(a: f64, b: f64) -> String {
    if b > 0.0 {
        format!("{:+.2}%", (a / b - 1.0) * 100.0)
    } else {
        "n/a".into()
    }
}

fn line(name: &str, a: f64, b: f64) {
    println!("  {name:<18} {a:>14.2} -> {b:>14.2}  {}", pct(b, a));
}

fn run(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or(USAGE)?;
    let xml = std::fs::read_to_string(path)
        .map_err(|e| format!("{path}: {e}"))?
        .replace("\r\n", "\n");
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let optional = |name: &str| -> Result<Option<f64>, String> {
        value(args, name).map(|v| v.parse().map_err(|_| format!("{name}: {v}"))).transpose()
    };
    let goal = Goal {
        min_elemental_resist: number(args, "--min-res", 75.0)?,
        min_chaos_resist: optional("--min-chaos")?,
        min_ehp: optional("--min-ehp")?,
        dps_weight: number(args, "--dps-weight", 1.0)?,
        ehp_weight: number(args, "--ehp-weight", 1.0)?,
    };
    let opt = Options {
        seconds: number(args, "--secs", 60.0)?,
        moves_per_round: number(args, "--round", 400)?,
        threads: number(args, "--threads", threads)?,
        seed: number(args, "--seed", 1)?,
    };
    println!("{goal:?}\n{opt:?}");
    let r = optimize(&xml, &goal, opt)?;
    let (a, b): (&Eval, &Eval) = (&r.start, &r.best);
    println!(
        "tried {} variants in {:.1} s ({:.0}/s on {} threads), allowed {}, load {:.0} ms, recalculation median {:.1} ms",
        r.tried,
        r.seconds,
        r.tried as f64 / r.seconds.max(1e-9),
        opt.threads,
        r.accepted,
        r.load_ms,
        r.calc_ms_median
    );
    for (why, n) in &r.rejected {
        println!("  rejected {n:>6}: {}", why.label());
    }
    println!("rounds with an improvement: {}", r.steps.len());
    for s in &r.steps {
        println!(
            "  round {:>3}: score {:.4}, DPS {:.1}, EHP {:.0}, tried so far {}",
            s.round, s.score, s.eval.dps, s.eval.ehp, s.tried
        );
    }
    println!("start -> best (score {:.4}):", r.best_score);
    line("CombinedDPS", a.dps, b.dps);
    line("TotalEHP", a.ehp, b.ehp);
    line("Life", a.life, b.life);
    line("FireResist", a.fire, b.fire);
    line("ColdResist", a.cold, b.cold);
    line("LightningResist", a.lightning, b.lightning);
    line("ChaosResist", a.chaos, b.chaos);
    println!("warnings: {} -> {}", a.warnings.len(), b.warnings.len());
    let check = evaluate(&Engine::load_xml(&r.best_xml)?);
    println!("the saved best build, loaded again, gives the same numbers: {}", check == r.best);
    if let Some(out) = value(args, "--out") {
        std::fs::write(&out, &r.best_xml).map_err(|e| format!("{out}: {e}"))?;
        println!("saved");
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("probe") => run(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pob-optimizer: {e}");
            ExitCode::from(2)
        }
    }
}
