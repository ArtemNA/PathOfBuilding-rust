//! R-party perf: `Engine::load_xml` + `Engine::recalc` on the build with a filled Party tab, and the same build without its `<Party>`. LuaJIT
//! medians: `node tools/lua-corpus/export-real.ts perf-load --out <dir> '^3.29/build-c'`. Personal build: skipped when it is not here.
//!   cargo test --release -p pob-engine --test party_perf -- --ignored --nocapture

use std::path::Path;
use std::time::Instant;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// `(load, recalc)` ms, median of 9 after a warm-up.
fn measure(xml: &str) -> (f64, f64) {
    let (mut l, mut r) = (Vec::new(), Vec::new());
    for i in 0..=9 {
        let t0 = Instant::now();
        let mut e = pob_engine::engine::Engine::load_xml(xml).expect("load");
        let t1 = Instant::now();
        e.recalc().expect("recalc");
        let t2 = Instant::now();
        if i > 0 {
            l.push((t1 - t0).as_secs_f64() * 1000.0);
            r.push((t2 - t1).as_secs_f64() * 1000.0);
        }
    }
    (median(l), median(r))
}

#[test]
#[ignore = "perf; needs builds/pc"]
fn party_build_perf() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../builds/pc/3.29/build-c.xml");
    let Ok(xml) = pob_build::read_build_file(&path) else {
        println!("party_perf: {} absent -- skipped", path.display());
        return;
    };
    let a = xml.find("<Party").expect("<Party");
    let b = xml.find("</Party>").expect("</Party>") + "</Party>".len();
    let noparty = format!("{}{}", &xml[..a], &xml[b..]);
    for (name, x) in [("build-c", xml.as_str()), ("build-c without <Party>", noparty.as_str())] {
        let (l, r) = measure(x);
        println!("{name}: load {l:.2} ms, recalc {r:.2} ms");
    }
}
