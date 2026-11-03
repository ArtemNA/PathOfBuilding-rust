//! U6-perf gate of `tree_view::main_pass_for_calculator`: a calculator over the short MAIN pass == a calculator over the whole `buildOutput(MAIN)` (cost warnings and all): the base pass and
//! `calcFunc` with every filled slot taken off, every scalar bit for bit.
//!   cargo test --release -p pob-engine --test misc_main_pass -- --include-ignored --nocapture

use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::env::CalcOverride;
use pob_calc::session::{CalcMode, CalcSession, MiscCalculator};
use pob_calc::{CalculateOptions, output_to_stats};
use pob_engine::engine::Engine;
use pob_engine::tree_view::main_pass_for_calculator;

/// `Err`: the first difference.
fn compare(xml: &str) -> Result<usize, String> {
    let e = Engine::load_xml(xml)?;
    let model = Rc::new(e.model().clone());
    let full = CalcSession::new(model.clone(), CalculateOptions::default());
    full.build_output(CalcMode::Main).map_err(|er| er.0)?;
    let full = full.get_misc_calculator().map_err(|er| er.0)?;
    let short = CalcSession::new(model, CalculateOptions::default());
    main_pass_for_calculator(&short)?;
    let short = short.get_misc_calculator().map_err(|er| er.0)?;
    let same = |what: &str, a: &MiscCalculator, b: &MiscCalculator, ov: Option<CalcOverride>| -> Result<(), String> {
        let (ra, rb) = match ov {
            None => (a.base.clone(), b.base.clone()),
            Some(ov) => {
                let ov = Rc::new(ov);
                (
                    a.calc(&ov, None, false).map_err(|er| er.0)?,
                    b.calc(&ov, None, false).map_err(|er| er.0)?,
                )
            }
        };
        let (ra, rb) = (ra.borrow(), rb.borrow());
        let (sa, sb) = (output_to_stats(&ra.state.player.output), output_to_stats(&rb.state.player.output));
        for (k, v) in &sa {
            if sb.get(k) != Some(v) {
                return Err(format!("{what}: {k}: whole {v:?}, short {:?}", sb.get(k)));
            }
        }
        if sa.len() != sb.len() {
            return Err(format!("{what}: {} stats, short {}", sa.len(), sb.len()));
        }
        Ok(())
    };
    same("base", &full, &short, None)?;
    let (mf, ms) = (full.session().main_merge(), short.session().main_merge());
    if mf.as_ref().map(|m| m.granted.len()) != ms.as_ref().map(|m| m.granted.len()) {
        return Err("the merge of the MAIN initEnv".to_string());
    }
    let mut compared = 1;
    for slot in &e.result().env.ordered_slots {
        if slot.sel_item_id != 0.0 {
            let ov = CalcOverride {
                rep_slot_name: Some(slot.slot_name.clone()),
                ..CalcOverride::default()
            };
            same(&format!("without {}", slot.slot_name), &full, &short, Some(ov))?;
            compared += 1;
        }
    }
    Ok(compared)
}

#[test]
fn short_main_pass_on_the_goldens() {
    let mut compared = 0;
    for g in pob_golden::registry::GOLDEN_BUILDS {
        let xml = std::fs::read_to_string(g.xml_path()).expect("golden");
        compared += compare(&xml).unwrap_or_else(|d| panic!("{}: {d}", g.name));
    }
    assert!(compared > 0);
    eprintln!("runs compared {compared}");
}

fn xmls(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            xmls(&p, out);
        } else if p.extension().is_some_and(|x| x == "xml") {
            out.push(p);
        }
    }
}

/// The personal corpus, local only.
#[test]
#[ignore = "the whole builds/pc corpus, a few minutes"]
fn short_main_pass_on_the_corpus() {
    let mut files = Vec::new();
    xmls(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../builds/pc"), &mut files);
    files.sort();
    let (mut builds, mut runs, mut skipped) = (0, 0, 0);
    let mut diffs = Vec::new();
    for f in &files {
        let Ok(xml) = std::fs::read_to_string(f) else { continue };
        let xml = xml.replace("\r\n", "\n");
        // a build the engine does not load or run is the business of its own gates
        let r = std::panic::catch_unwind(|| compare(&xml));
        match r {
            Ok(Ok(n)) => {
                builds += 1;
                runs += n;
            }
            Ok(Err(d)) if d.contains("whole") || d.contains("short") || d.contains("merge") => diffs.push(format!("{}: {d}", f.display())),
            Ok(Err(why)) => {
                skipped += 1;
                eprintln!("not run {}: {why}", f.display());
            }
            Err(p) => {
                skipped += 1;
                let why = p
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| p.downcast_ref::<&str>().copied())
                    .unwrap_or("panic");
                eprintln!("panicked {}: {why}", f.display());
            }
        }
    }
    eprintln!("builds {builds}, runs compared {runs}, not run {skipped}, differ {}", diffs.len());
    assert!(diffs.is_empty(), "{}", diffs[..diffs.len().min(10)].join("\n"));
}
