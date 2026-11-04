//! R-party perf: `Engine::load_xml` + `Engine::recalc` on the build with a filled Party tab, and the same build without its `<Party>`. LuaJIT
//! medians: `node tools/lua-corpus/export-real.ts perf-load --out <dir> '^3.29/<build>'`. Personal build: skipped when it is not here.
//!   cargo test --release -p pob-engine --test party_perf -- --ignored --nocapture

use std::path::Path;
use std::time::Instant;

/// A list of personal builds from the local `builds/local-builds.json` (not in git); empty without it.
fn local_builds(key: &str) -> Vec<String> {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../builds/local-builds.json");
    let v: Option<serde_json::Value> = std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok());
    v.and_then(|v| {
        v[key]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
    })
    .unwrap_or_default()
}
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
    let Some(build) = local_builds("party").into_iter().next() else {
        println!("no party build in builds/local-builds.json: skipped");
        return;
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../builds/pc/3.29/{build}.xml"));
    let Ok(xml) = pob_build::read_build_file(&path) else {
        println!("party_perf: {} absent -- skipped", path.display());
        return;
    };
    let a = xml.find("<Party").expect("<Party");
    let b = xml.find("</Party>").expect("</Party>") + "</Party>".len();
    let noparty = format!("{}{}", &xml[..a], &xml[b..]);
    for (name, x) in [("as saved", xml.as_str()), ("without <Party>", noparty.as_str())] {
        let (l, r) = measure(x);
        println!("{name}: load {l:.2} ms, recalc {r:.2} ms");
    }
}
