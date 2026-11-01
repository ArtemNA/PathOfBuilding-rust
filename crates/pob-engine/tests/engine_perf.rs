//! R8.8 perf (run with `--release -- --ignored --nocapture`): the engine on the 6 goldens -- load (`Engine::load_xml`: XML -> model ->
//! MAIN + CALCS + RefreshStatList), recalc, recalc after a mutation (setConfig of a check var), SaveDB (+ build code) and one power report.
//! Medians of 7 runs after one warm-up; the LuaJIT side is `tools/.scratch/perf-engine.json` (export-engine.ts: os.clock, jit.on) and
//! `tools/.scratch/perf-r8.json` (export-r8.ts perf: `build:SaveDB`), printed beside when present.

use std::path::Path;
use std::time::Instant;

use pob_calc::pob_build::InputValue;
use pob_engine::engine::Engine;
use serde_json::Value as Json;

fn med(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn time<T>(runs: usize, mut f: impl FnMut() -> T) -> f64 {
    let _ = f();
    med((0..runs)
        .map(|_| {
            let t = Instant::now();
            let _ = f();
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect())
}

fn scratch(name: &str) -> Option<Json> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/.scratch").join(name);
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

#[test]
#[ignore = "perf: cargo test --release -p pob-engine --test engine_perf -- --ignored --nocapture"]
fn engine_perf() {
    let lj = scratch("perf-engine.json");
    let lj_save = scratch("perf-r8.json");
    let ljm = |slug: &str, k: &str| -> String {
        lj.as_ref()
            .and_then(|j| j[slug][k].as_array().cloned())
            .map(|a| med(a.iter().filter_map(Json::as_f64).collect()))
            .map_or_else(|| "-".into(), |x| format!("{x:.1}"))
    };
    println!(
        "build | load Rust (first, then median) / LuaJIT | recalc Rust / LuaJIT | recalc after mutation Rust / LuaJIT | SaveDB Rust / LuaJIT | code Rust | power(Life, 8 thr) Rust"
    );
    for g in pob_golden::registry::GOLDEN_BUILDS {
        let xml = std::fs::read_to_string(g.xml_path()).expect("xml");
        let slug = g.name.replace(' ', "-");
        let t = Instant::now();
        let mut e = Engine::load_xml(&xml).expect("load");
        let first = t.elapsed().as_secs_f64() * 1e3;
        let load = time(7, || Engine::load_xml(&xml).expect("load"));
        let recalc = time(7, || e.recalc().expect("recalc"));
        let mut flip = false;
        let mutation = time(7, || {
            flip = !flip;
            e.set_config("conditionKilledRecently", Some(InputValue::Bool(flip)))
                .expect("setConfig");
        });
        e.clear_config("conditionKilledRecently").expect("clear");
        let save = time(7, || e.save_xml().expect("save"));
        let code = time(7, || e.build_code().expect("code"));
        let t = Instant::now();
        let power = e.power_report_json(Some("Life"), 8, None).expect("power").expect("not cancelled");
        let power_ms = t.elapsed().as_secs_f64() * 1e3;
        let lj_s = lj_save
            .as_ref()
            .and_then(|j| j["builds"][g.name]["saveDB"]["median"].as_f64())
            .map_or_else(|| "-".into(), |x| format!("{x:.1}"));
        println!(
            "{} | {first:.0} / {load:.1} / {} | {recalc:.1} / {} | {mutation:.1} / {} | {save:.1} / {lj_s} | {code:.1} | {power_ms:.0} ({} entries)",
            g.name,
            ljm(&slug, "load"),
            ljm(&slug, "recalc"),
            ljm(&slug, "mutationRecalc"),
            power["count"]
        );
    }
}
