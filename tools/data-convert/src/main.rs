//! data-convert (prototype): upstream PoB -> data/*.json with no pob-ts in the loop. See Cargo.toml for the shape.
//!
//!   data-convert dump    --pob <pobRoot> --dump <file>
//!   data-convert convert --pob <pobRoot> --dump <file> --out <dir> [--only gems,item-bases,...]
//!   data-convert all     --pob <pobRoot> --dump <file> --out <dir> [--only ...]

mod closures;
mod constants;
mod costs;
mod ctx;
mod dump;
mod fields;
mod gems;
mod globals;
mod item_bases;
mod js;
mod mod_cache;
mod mod_parser;
mod mods;
mod rules;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

type Converter = fn(&mut ctx::Ctx) -> ctx::R<js::J>;

const CONVERTERS: &[(&str, Converter)] = &[
    ("globals", globals::convert),
    ("constants", constants::convert),
    ("rules", rules::convert),
    ("costs", costs::convert),
    ("gems", gems::convert),
    ("item-bases", item_bases::convert),
    ("mod-parser", mod_parser::convert),
    ("mod-cache", mod_cache::convert),
    ("closures", closures::convert),
];

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1).cloned())
}

fn tool_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run_dump(pob: &Path, out: &Path) -> Result<(), String> {
    let exe = tool_root().join("../luajit-win/target/release/luajit-win.exe");
    if !exe.exists() {
        return Err(format!("{} missing: cargo build --release --manifest-path tools/luajit-win/Cargo.toml", exe.display()));
    }
    let script = tool_root().join("lua/dump.lua");
    let runtime = pob.join("runtime");
    let path = format!("{};{}", runtime.display(), std::env::var("PATH").unwrap_or_default());
    let t = Instant::now();
    let o = Command::new(&exe)
        .arg(&script)
        .current_dir(pob.join("src"))
        .env("PATH", path)
        .env("POB_DUMP_OUT", out)
        .env_remove("CI")
        .env_remove("REGENERATE_MOD_CACHE")
        .output()
        .map_err(|e| e.to_string())?;
    let stderr = String::from_utf8_lossy(&o.stderr);
    for l in stderr.lines().filter(|l| l.starts_with("data-dump") || l.contains("error")) {
        eprintln!("  {l}");
    }
    if !o.status.success() {
        return Err(format!("dump failed: {stderr}"));
    }
    eprintln!("dump: {:.2} s, {} -> {} bytes", t.elapsed().as_secs_f64(), String::from_utf8_lossy(&o.stdout), std::fs::metadata(out).unwrap().len());
    Ok(())
}

fn run_convert(pob: &Path, dump_file: &Path, out: &Path, only: Option<&str>) -> Result<(), String> {
    let t = Instant::now();
    let bytes = std::fs::read(dump_file).map_err(|e| e.to_string())?;
    let d = dump::Dump::parse(&bytes)?;
    eprintln!("parse: {:.2} s ({} tables, {} functions)", t.elapsed().as_secs_f64(), d.tables.len(), d.funcs.len());
    let mut c = ctx::Ctx::new(&d, pob.to_path_buf());
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for (id, f) in CONVERTERS {
        if only.is_some_and(|o| !o.split(',').any(|x| x == *id)) {
            continue;
        }
        let t = Instant::now();
        let v = f(&mut c).map_err(|e| format!("[{id}] {e}"))?;
        let text = js::stringify(&v);
        check_reader(id, &text)?;
        std::fs::write(out.join(format!("{id}.json")), &text).map_err(|e| e.to_string())?;
        eprintln!("{id}: {} bytes in {:.2} s", text.len(), t.elapsed().as_secs_f64());
    }
    print!("{}", c.report());
    let untouched = d.untouched("data");
    println!("coverage  data.* keys no converter read: {} -- {}", untouched.len(), untouched.join(", "));
    Ok(())
}

/// The port's own loader types are the final schema: what pob-data cannot read is an error here, at convert time.
fn check_reader(id: &str, text: &str) -> Result<(), String> {
    let r = match id {
        "gems" => serde_json::from_str::<pob_data::gems::Gems>(text).map(drop),
        "item-bases" => serde_json::from_str::<pob_data::item_bases::ItemBases>(text).map(drop),
        _ => Ok(()),
    };
    r.map_err(|e| format!("[{id}] pob-data cannot read the result: {e}"))
}

fn main() {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    let pob = PathBuf::from(arg("--pob").expect("--pob <pobRoot>"));
    let dump_file = PathBuf::from(arg("--dump").expect("--dump <file>"));
    let r = match cmd.as_str() {
        "dump" => run_dump(&pob, &dump_file),
        "convert" => run_convert(&pob, &dump_file, Path::new(&arg("--out").expect("--out")), arg("--only").as_deref()),
        "all" => run_dump(&pob, &dump_file)
            .and_then(|_| run_convert(&pob, &dump_file, Path::new(&arg("--out").expect("--out")), arg("--only").as_deref())),
        _ => Err("usage: data-convert dump|convert|all --pob <dir> --dump <file> [--out <dir>] [--only a,b]".into()),
    };
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
