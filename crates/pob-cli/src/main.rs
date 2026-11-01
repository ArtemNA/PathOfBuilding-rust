use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: pob-cli calc <path.xml | -> [--json] [--cache-only]\n  \
     '-' reads the build XML (UTF-8) from stdin; POB_EXPERIMENTAL=1 disables the stats allowlist\n       \
     pob-cli bench <path.xml | golden name | all> [--runs N] [--cache-only]\n  \
     median [min-max] ms of the full calculate_file over N runs (default 10, one untimed warm-up)\n  \
     mod lines: ModCache, else the real ModParser (desktop); --cache-only = ModCache only (pob-ts)
       pob-cli power <path.xml | golden name | all> [--stat <powerStat>] [--threads N]
  \n     node power of every tree node (CalcsTab:PowerBuilder), wall ms; default stat: Offence/Defence, threads: all cores";

fn fail(msg: &str) -> ExitCode {
    eprintln!("pob-cli: {msg}");
    ExitCode::from(2)
}

/// The build text as desktop PoB reads it (`io.open(path, "r")`, Build.lua:2267: CRLF -> LF, `pob_build::file`). Stdin
/// gets the same treatment: the oracle adapter pipes the file's content.
fn read_input(src: &str) -> Result<String, String> {
    if src == "-" {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|e| format!("cannot read stdin: {e}"))?;
        let text = String::from_utf8(bytes).map_err(|e| format!("stdin is not valid UTF-8: {e}"))?;
        Ok(pob_build::desktop_text_mode(&text))
    } else {
        pob_build::read_build_file(src).map_err(|e| format!("cannot read '{src}': {e}"))
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("calc") => run_calc(&args[1..]),
        Some("bench") => run_bench(&args[1..]),
        Some("power") => run_power(&args[1..]),
        Some(other) => fail(&format!("unknown command '{other}'\n{USAGE}")),
    }
}

fn run_calc(args: &[String]) -> ExitCode {
    let mut json = false;
    let mut mode = pob_cli::ParseMode::Full;
    let mut input: Option<&str> = None;
    for a in args {
        match a.as_str() {
            "--json" => json = true,
            "--cache-only" => mode = pob_cli::ParseMode::CacheOnly,
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-" => {
                if input.replace("-").is_some() {
                    return fail("more than one input given");
                }
            }
            flag if flag.starts_with('-') => return fail(&format!("unknown flag '{flag}'\n{USAGE}")),
            path => {
                if input.replace(path).is_some() {
                    return fail("more than one input given");
                }
            }
        }
    }
    let Some(src) = input else {
        return fail(&format!("calc: missing input\n{USAGE}"));
    };

    let out = match read_input(src).and_then(|xml| pob_cli::calc_mode(&xml, pob_cli::experimental_from_env(), mode)) {
        Ok(out) => out,
        Err(e) => return fail(&e),
    };
    if json {
        println!("{}", out.to_json());
    } else {
        println!("stats: {} keys, unimplemented: {}", out.stats.len(), out.unimplemented.len());
        for u in &out.unimplemented {
            println!("  - {u}");
        }
    }
    ExitCode::SUCCESS
}

fn fmt_row(name: &str, v: &[f64]) -> String {
    let (med, min, max) = pob_cli::median_min_max(v);
    format!("{name} {med:.2} [{min:.2}-{max:.2}]")
}

fn run_bench(args: &[String]) -> ExitCode {
    let mut runs = 10usize;
    let mut mode = pob_cli::ParseMode::Full;
    let mut target: Option<&str> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--cache-only" => mode = pob_cli::ParseMode::CacheOnly,
            "--runs" => match it.next().and_then(|n| n.parse::<usize>().ok()).filter(|n| *n > 0) {
                Some(n) => runs = n,
                None => return fail("--runs needs a positive integer"),
            },
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            flag if flag.starts_with('-') => return fail(&format!("unknown flag '{flag}'\n{USAGE}")),
            t => {
                if target.replace(t).is_some() {
                    return fail("more than one target given");
                }
            }
        }
    }
    let Some(target) = target else {
        return fail(&format!("bench: missing target\n{USAGE}"));
    };
    let jobs: Vec<(String, PathBuf)> = if target == "all" {
        pob_golden::GOLDEN_BUILDS
            .iter()
            .map(|g| (g.name.to_owned(), g.xml_path()))
            .collect()
    } else if let Some(g) = pob_golden::find_golden(target) {
        vec![(g.name.to_owned(), g.xml_path())]
    } else {
        vec![(target.to_owned(), PathBuf::from(target))]
    };
    println!("full calculate_file ({mode:?}), {runs} runs (+1 warm-up), ms: median [min-max]");
    for (name, path) in jobs {
        match pob_cli::bench(&path, runs, mode) {
            Ok(r) => {
                println!(
                    "{name}: {} | {} | {} | {} | {}",
                    fmt_row("total", &r.total),
                    fmt_row("load", &r.load),
                    fmt_row("initEnv", &r.init_env),
                    fmt_row("perform", &r.perform),
                    fmt_row("estimate", &r.estimate),
                );
                if !r.init_env_all.is_empty() {
                    println!(
                        "    {} | {} (x{})",
                        fmt_row("buildOutput", &r.build_output),
                        fmt_row("initEnv all", &r.init_env_all),
                        r.init_env_calls
                    );
                }
            }
            Err(e) => return fail(&e),
        }
    }
    ExitCode::SUCCESS
}

fn run_power(args: &[String]) -> ExitCode {
    let mut stat: Option<String> = None;
    let mut threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let mut target: Option<&str> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--stat" => match it.next() {
                Some(s) => stat = Some(s.clone()),
                None => return fail("--stat needs a stat name (data.powerStatList)"),
            },
            "--threads" => match it.next().and_then(|n| n.parse::<usize>().ok()).filter(|n| *n > 0) {
                Some(n) => threads = n,
                None => return fail("--threads needs a positive integer"),
            },
            flag if flag.starts_with('-') => return fail(&format!("unknown flag '{flag}'\n{USAGE}")),
            t => {
                if target.replace(t).is_some() {
                    return fail("more than one target given");
                }
            }
        }
    }
    let Some(target) = target else {
        return fail(&format!("power: missing target\n{USAGE}"));
    };
    let jobs: Vec<(String, PathBuf)> = if target == "all" {
        pob_golden::GOLDEN_BUILDS
            .iter()
            .map(|g| (g.name.to_owned(), g.xml_path()))
            .collect()
    } else if let Some(g) = pob_golden::find_golden(target) {
        vec![(g.name.to_owned(), g.xml_path())]
    } else {
        vec![(target.to_owned(), PathBuf::from(target))]
    };
    println!(
        "node power ({}), {threads} thread(s), wall ms incl. session set-up",
        stat.as_deref().unwrap_or("Offence/Defence")
    );
    for (name, path) in jobs {
        match pob_cli::power(&path, stat.as_deref(), threads) {
            Ok(r) => println!("{name}: {:.0} ms, {} calculator calls, {} units", r.ms, r.calc_calls, r.units),
            Err(e) => return fail(&e),
        }
    }
    ExitCode::SUCCESS
}
