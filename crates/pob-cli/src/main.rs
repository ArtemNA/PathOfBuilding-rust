use std::io::Read;
use std::process::ExitCode;

const USAGE: &str = "usage: pob-cli calc <path.xml | -> [--json]\n  '-' reads the build XML (UTF-8) from stdin";

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
            return ExitCode::from(2);
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("calc") => {}
        Some(other) => return fail(&format!("unknown command '{other}'\n{USAGE}")),
    }

    let mut json = false;
    let mut input: Option<&str> = None;
    for a in &args[1..] {
        match a.as_str() {
            "--json" => json = true,
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

    let out = match read_input(src).and_then(|xml| pob_cli::calc(&xml)) {
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
