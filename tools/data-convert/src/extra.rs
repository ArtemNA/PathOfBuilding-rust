//! The second dump (lua/dump-extra.lua -> `<dump>.extra`: tree versions, ConfigOptions.lua, every passive tree) and the
//! files built from it: config-options.json, tree/**.json, then manifest.json over everything in the output dir.
//! Both dumps boot PoB on their own, so they run in parallel.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::ctx::{Ctx, R};
use crate::{config_options, dump, js, manifest, trees};

pub fn extra_path(dump_file: &Path) -> PathBuf {
    let mut p = dump_file.as_os_str().to_owned();
    p.push(".extra");
    PathBuf::from(p)
}

pub fn run_both(pob: &Path, dump_file: &Path) -> R<()> {
    let extra = extra_path(dump_file);
    std::thread::scope(|s| {
        let h = s.spawn(|| crate::run_script(pob, &extra, "dump-extra.lua"));
        let a = crate::run_script(pob, dump_file, "dump.lua");
        let b = h.join().map_err(|_| "dump-extra thread panicked".to_string())?;
        a.and(b)
    })
}

fn wanted(only: Option<&str>, id: &str) -> bool {
    only.is_none_or(|o| o.split(',').any(|x| x == id))
}

fn write(out: &Path, file: &str, v: &js::J) -> R<usize> {
    let text = js::stringify(v);
    let p = out.join(file);
    std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&p, &text).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(text.len())
}

/// `c`: the main dump's context (the manifest reads `data` from it).
pub fn convert(c: &mut Ctx, pob: &Path, dump_file: &Path, out: &Path, only: Option<&str>) -> R<()> {
    if !["config-options", "trees", "manifest"].iter().any(|id| wanted(only, id)) {
        return Ok(());
    }
    let t = Instant::now();
    let p = extra_path(dump_file);
    let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let d = dump::Dump::parse(&bytes)?;
    eprintln!("parse extra: {:.2} s ({} tables, {} functions)", t.elapsed().as_secs_f64(), d.tables.len(), d.funcs.len());
    let mut cx = Ctx::new(&d, pob.to_path_buf());
    let vers = trees::versions(&cx)?;

    if wanted(only, "config-options") {
        let t = Instant::now();
        let v = config_options::convert(&mut cx).map_err(|e| format!("[config-options] {e}"))?;
        let n = write(out, "config-options.json", &v)?;
        eprintln!("config-options: {n} bytes in {:.2} s", t.elapsed().as_secs_f64());
    }
    if wanted(only, "trees") {
        let t = Instant::now();
        let files = trees::convert(&mut cx, &vers).map_err(|e| format!("[trees] {e}"))?;
        let mut total = 0;
        for (file, v) in &files {
            total += write(out, file, v)?;
        }
        eprintln!("trees: {} files, {total} bytes in {:.2} s", files.len(), t.elapsed().as_secs_f64());
    }
    print!("{}", cx.report());
    if wanted(only, "manifest") {
        let t = Instant::now();
        let v = manifest::convert(c, &vers, out).map_err(|e| format!("[manifest] {e}"))?;
        let n = write(out, "manifest.json", &v)?;
        eprintln!("manifest: {n} bytes in {:.2} s", t.elapsed().as_secs_f64());
    }
    Ok(())
}
