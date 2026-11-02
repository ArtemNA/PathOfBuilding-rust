//! U1.2 gate (needs a GPU or WARP; `#[ignore]` by default — UI-ARCH §5.3):
//!   cargo test -p pob-gfx --release --test scenes -- --ignored --nocapture
//! Every SG-oracle scene (`tools/sg-oracle/scenes/*.json`) is replayed on `pob-gfx`, rendered offscreen at the oracle's
//! 1280x800 and compared with the original's screenshot (`fixtures/scenes/<name>.png`, top-left crop): the number of
//! pixels off by more than 2/255 in any channel must be 0 overall and on text; `@log` results must equal the manifest.

mod scene;

use pob_gfx::{Gfx, Renderer};
use scene::{Scene, SCREEN};
use serde_json::Value;

/// Scenes of `index.json` (U1.0) plus the U1.2 checks/holdouts (`run-scenes.ps1 -Tag u12`, `-Tag u12-holdout`).
fn all_scenes() -> Vec<String> {
    let mut v = Scene::names();
    v.extend(["u12-frac", "u12-dpi125", "u12-image", "u12-holdout", "u12-nearest"].map(String::from));
    v
}

fn manifest_logs(name: &str) -> Option<Value> {
    for file in ["manifest.json", "manifest-u12.json", "manifest-u12-holdout.json"] {
        let p = scene::oracle_dir().join("fixtures/scenes").join(file);
        let m: Value = serde_json::from_str(&std::fs::read_to_string(&p).expect("manifest")).expect("manifest json");
        if let Some(e) = m.as_array().expect("array").iter().find(|e| e["name"] == name) {
            return Some(e["logs"].clone());
        }
    }
    None
}

#[test]
#[ignore = "needs a GPU adapter (or POB_GFX_WARP=1); run with --ignored"]
fn scenes_equal_original() {
    let mut r = Renderer::new_offscreen().expect("wgpu adapter");
    eprintln!("adapter: {:?}", r.adapter_info().name);
    let mut failures = Vec::new();
    for name in all_scenes() {
        let sc = Scene::load(&name);
        let mut gfx = Gfx::new(&scene::fonts_dir(), SCREEN, 1.0).expect("fonts");
        let (list, logs) = sc.run(&mut gfx);
        let rgba = r.render_to_rgba(&list).expect("render");
        let oracle = scene::load_oracle_png(&name);
        assert_eq!(oracle.dimensions(), sc.crop(), "{name}: crop");
        let mask = scene::text_mask(&list, SCREEN.0, SCREEN.1);
        let (d, heat) = scene::diff(&rgba, SCREEN.0, &oracle, &mask);
        eprintln!(
            "{name:<15} px {:>7} bad {:>5} ({:.3}%) maxΔ {:>3} | text px {:>6} bad {:>4} maxΔ {:>3}",
            d.pixels,
            d.bad,
            d.frac() * 100.0,
            d.max_delta,
            d.text_pixels,
            d.text_bad,
            d.text_max_delta
        );
        if d.bad > 0 {
            let out = scene::out_dir();
            scene::save(&heat, &out.join(format!("{name}.heat.png")));
            let (w, h) = oracle.dimensions();
            scene::save(&scene::crop(&rgba, SCREEN.0, w, h), &out.join(format!("{name}.ours.png")));
            failures.push(format!("{name}: {} px off (text {}), bbox {:?}", d.bad, d.text_bad, d.bbox));
        }
        let want = manifest_logs(&name).unwrap_or_else(|| panic!("{name}: no manifest entry"));
        // Manifest entries: [callIndex, "GetDrawColor", [r, g, b, a]] (dkjson writes 1 for 1.0).
        let want: Vec<(usize, [f64; 4])> = want
            .as_array()
            .expect("logs")
            .iter()
            .map(|e| {
                let c = e[2].as_array().expect("rgba");
                (e[0].as_u64().expect("index") as usize, std::array::from_fn(|k| c[k].as_f64().expect("number")))
            })
            .collect();
        let got: Vec<(usize, [f64; 4])> = logs.iter().map(|(i, c)| (*i, c.map(f64::from))).collect();
        assert_eq!(got, want, "{name}: @log GetDrawColor");
    }
    assert!(failures.is_empty(), "scenes differ from the original:\n{}", failures.join("\n"));
}

/// The window path (main pass + flipping blit into a surface-format texture) gives the same pixels as the readback.
#[test]
#[ignore = "needs a GPU adapter (or POB_GFX_WARP=1); run with --ignored"]
fn blit_path_equals_readback() {
    let instance = Renderer::new_instance();
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let mut r = Renderer::new(&instance, None, format).expect("wgpu adapter");
    let sc = Scene::load("u12-frac");
    let mut gfx = Gfx::new(&scene::fonts_dir(), SCREEN, 1.0).expect("fonts");
    let (list, _) = sc.run(&mut gfx);
    let want = r.render_to_rgba(&list).expect("readback");
    let got = r.render_blit_to_rgba(&list).expect("blit");
    // BGRA -> RGBA
    let got: Vec<u8> = got.chunks(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    let bad = want.chunks(4).zip(got.chunks(4)).filter(|(a, b)| a[..3] != b[..3]).count();
    assert_eq!(bad, 0, "blit path differs from readback in {bad} pixels");
}
