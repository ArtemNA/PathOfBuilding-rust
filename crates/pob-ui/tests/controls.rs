//! U2a gate: the original Lua controls (run by the SG oracle, `tools/sg-oracle/lua/controls-runner.lua`) vs this port, on the
//! same galleries and input scenarios (`tools/sg-oracle/scenes/controls/*.json`).
//! - `states_equal_original` (no GPU): every `dump` step = the oracle's record `fixtures/controls/<scenario>.json`.
//! - `pixels_equal_original` (GPU/WARP, `#[ignore]`): every screenshot = the original's PNG over the whole 1280x800 client,
//!   0 pixels beyond 2/255 (and 0 on text).
//!   cargo test -p pob-ui --release --test controls -- --include-ignored --nocapture

mod gallery;

use gallery::scene;
use pob_gfx::Renderer;
use serde_json::Value;

/// Lua numbers have no int/float split (dkjson writes `0`, Rust `0.0`): integral numbers compare as integers.
fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => Value::from(f as i64),
            _ => v.clone(),
        },
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        _ => v.clone(),
    }
}

#[test]
fn states_equal_original() {
    for name in gallery::scenario_names() {
        let rec = gallery::oracle_record(&name);
        assert_eq!(
            rec["errors"].as_array().map(Vec::len),
            Some(0),
            "{name}: oracle errors {}",
            rec["errors"]
        );
        let run = gallery::run(&name);
        let want: Vec<(u64, Value)> = rec["dumps"]
            .as_array()
            .expect("dumps")
            .iter()
            .map(|d| (d["step"].as_u64().expect("step"), d["state"].clone()))
            .collect();
        assert_eq!(run.dumps.len(), want.len(), "{name}: number of dumps");
        for ((s1, got), (s2, want)) in run.dumps.iter().zip(&want) {
            assert_eq!(s1, s2);
            assert_eq!(norm(got), norm(want), "{name} step {s1}");
        }
        let shots: Vec<&str> = rec["shots"]
            .as_array()
            .expect("shots")
            .iter()
            .map(|s| s.as_str().expect("shot"))
            .collect();
        let ours: Vec<&str> = run.shots.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(ours, shots, "{name}: screenshots");
    }
}

#[test]
#[ignore = "needs a GPU adapter (or POB_GFX_WARP=1); run with --ignored"]
fn pixels_equal_original() {
    let mut r = Renderer::new_offscreen().expect("wgpu adapter");
    let mut failures = Vec::new();
    for name in gallery::scenario_names() {
        for (shot, list) in gallery::run(&name).shots {
            let rgba = r.render_to_rgba(&list).expect("render");
            let p = scene::oracle_dir().join(format!("fixtures/controls/{shot}.png"));
            let oracle = image::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_rgba8();
            assert_eq!(oracle.dimensions(), scene::SCREEN, "{shot}: oracle size");
            let mask = scene::text_mask(&list, scene::SCREEN.0, scene::SCREEN.1);
            let (d, heat) = scene::diff(&rgba, scene::SCREEN.0, &oracle, &mask);
            eprintln!(
                "{shot:<22} bad {:>5} ({:.4}%) maxΔ {:>3} | text px {:>6} bad {:>4} maxΔ {:>3} | bbox {:?}",
                d.bad,
                d.frac() * 100.0,
                d.max_delta,
                d.text_pixels,
                d.text_bad,
                d.text_max_delta,
                d.bbox
            );
            if d.bad > 0 {
                let out = scene::out_dir();
                scene::save(&heat, &out.join(format!("{shot}.heat.png")));
                scene::save(
                    &scene::crop(&rgba, scene::SCREEN.0, scene::SCREEN.0, scene::SCREEN.1),
                    &out.join(format!("{shot}.ours.png")),
                );
                failures.push(format!("{shot}: {} px off (text {}), bbox {:?}", d.bad, d.text_bad, d.bbox));
            }
        }
    }
    assert!(failures.is_empty(), "controls differ from the original:\n{}", failures.join("\n"));
}
