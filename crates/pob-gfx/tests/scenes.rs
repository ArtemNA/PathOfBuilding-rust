//! U1.2 gate (needs a GPU or WARP; `#[ignore]` by default — UI-ARCH §5.3):
//!   cargo test -p pob-gfx --release --test scenes -- --ignored --nocapture
//! Every SG-oracle scene (`tools/sg-oracle/scenes/*.json`) is replayed on `pob-gfx`, rendered offscreen at the oracle's
//! 1280x800 and compared with the original's screenshot (`fixtures/scenes/<name>.png`, top-left crop): the number of
//! pixels off by more than 2/255 in any channel must be 0 overall and on text; `@log` results must equal the manifest.

mod scene;

use pob_gfx::{Gfx, Renderer};
use scene::{SCREEN, Scene};
use serde_json::Value;

/// Scenes of `index.json` (U1.0) plus the U1.2 checks/holdouts (`run-scenes.ps1 -Tag u12`, `-Tag u12-holdout`) and the U1.3/U1.5
/// scenes (`-Tag u13`, `-Tag u13-h30probe`). `u13-blend`: SimpleGraphic v2.5.5 has no `SetBlendMode` (its calls are Lua
/// errors without effect), so the scene is plain ALPHA blending.
fn all_scenes() -> Vec<String> {
    let mut v = Scene::names();
    v.extend(
        [
            "u12-frac",
            "u12-dpi125",
            "u12-image",
            "u12-holdout",
            "u12-nearest",
            "u13-negative",
            "u13-layers",
            "u13-placeholder",
            "u13-dpi125",
            "u13-blend",
            "u13-h30probe",
            "u15-demo",
        ]
        .map(String::from),
    );
    v
}

fn manifest_logs(name: &str) -> Option<Value> {
    for file in [
        "manifest.json",
        "manifest-u12.json",
        "manifest-u12-holdout.json",
        "manifest-u13.json",
        "manifest-u13-h30probe.json",
    ] {
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
                (
                    e[0].as_u64().expect("index") as usize,
                    std::array::from_fn(|k| c[k].as_f64().expect("number")),
                )
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

/// U1.3 `frame-probe.lua` shots (full 1280x800, 24-bit): the frame `TakeScreenshot` captures (the one after the call),
/// the frame-start reset of viewport/layer, and a DPI switch in the middle of the captured frame — replayed with the
/// same frame sequence and compared pixel by pixel.
#[test]
#[ignore = "needs a GPU adapter (or POB_GFX_WARP=1); run with --ignored"]
fn frame_probe_equals_original() {
    let mut r = Renderer::new_offscreen().expect("wgpu adapter");
    let mut gfx = Gfx::new(&scene::fonts_dir(), SCREEN, 1.0).expect("fonts");
    // Runs frames until one is captured; `draw(frame, frame_no, shot_requested_before)`.
    let mut shoot = |gfx: &mut Gfx, draw: &dyn Fn(&mut pob_gfx::Frame<'_>, u32)| -> Vec<u8> {
        for n in 0.. {
            let mut f = gfx.begin_frame();
            draw(&mut f, n);
            if n == 8 {
                f.take_screenshot();
            }
            let list = f.finish();
            if gfx.frame_wants_screenshot() {
                return r.render_to_rgba(&list).expect("render");
            }
        }
        unreachable!()
    };
    let shot = shoot(&mut gfx, &|f, n| {
        // green before the call, red in the calling frame, blue after
        let c = if n < 8 {
            [0.0, 1.0, 0.0]
        } else if n == 8 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        f.set_draw_color(c[0], c[1], c[2], 1.0);
        f.draw_image(None, 0.0, 0.0, 1280.0, 800.0, None).unwrap();
    });
    let viewport = shoot(&mut gfx, &|f, _| {
        f.set_draw_color(1.0, 1.0, 0.0, 1.0);
        f.draw_image(None, 0.0, 0.0, 1280.0, 800.0, None).unwrap();
        f.set_viewport(Some([100.0, 100.0, 50.0, 50.0]));
        f.set_draw_layer(Some(3), Some(4)).unwrap();
        f.set_draw_color(0.0, 0.0, 1.0, 0.5);
    });
    let dpi = shoot(&mut gfx, &|f, n| {
        f.set_draw_color(0.0, 0.0, 0.0, 1.0);
        f.draw_image(None, 0.0, 0.0, 2000.0, 2000.0, None).unwrap();
        f.set_draw_color(0.0, 1.0, 0.0, 1.0);
        f.draw_image(None, 10.0, 10.0, 20.0, 20.0, None).unwrap();
        if n == 9 {
            f.set_dpi_scale_override_percent(150);
        }
        f.set_draw_color(1.0, 0.0, 0.0, 1.0);
        f.draw_image(None, 100.0, 100.0, 100.0, 100.0, None).unwrap();
    });
    for (name, ours) in [("shot", shot), ("viewport", viewport), ("dpi", dpi)] {
        let p = scene::oracle_dir().join(format!("fixtures/frame-probe/{name}.png"));
        let want = image::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_rgba8();
        assert_eq!(want.dimensions(), SCREEN, "{name}");
        let mask = vec![false; (SCREEN.0 * SCREEN.1) as usize];
        let (d, _) = scene::diff(&ours, SCREEN.0, &want, &mask);
        eprintln!("frame-probe {name}: {} px, bad {}, maxΔ {}", d.pixels, d.bad, d.max_delta);
        assert_eq!(d.bad, 0, "frame-probe {name}: {} px off, bbox {:?}", d.bad, d.bbox);
    }
}
