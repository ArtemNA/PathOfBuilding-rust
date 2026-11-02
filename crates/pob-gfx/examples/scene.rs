//! Renders SG-oracle scenes offscreen, writes `target/sg-scenes/<name>.{ours,heat}.png` and prints the pixel diff
//! against the original's screenshots plus frame timings.
//!   cargo run -p pob-gfx --release --example scene [-- <name>[,<name>…]] [--perf N]

#[path = "../tests/scene/mod.rs"]
mod scene;

use pob_gfx::{Gfx, Renderer};
use scene::{Scene, SCREEN};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let perf = args
        .iter()
        .position(|a| a == "--perf")
        .map_or(0, |i| args[i + 1].parse::<u32>().expect("--perf N"));
    let names = args
        .iter()
        .find(|a| !a.starts_with("--") && a.parse::<u32>().is_err())
        .map_or_else(Scene::names, |s| s.split(',').map(str::to_string).collect());
    let t0 = Instant::now();
    let mut renderer = Renderer::new_offscreen().expect("renderer");
    let info = renderer.adapter_info().clone();
    println!("adapter: {} ({:?}), init {:.1} ms", info.name, info.backend, t0.elapsed().as_secs_f64() * 1e3);
    let out = scene::out_dir();
    println!(
        "{:<15} {:>9} {:>7} {:>8} {:>5} | {:>7} {:>6} {:>5} | bbox",
        "scene", "pixels", "bad", "bad %", "maxΔ", "text px", "t.bad", "t.maxΔ"
    );
    for name in &names {
        let sc = Scene::load(name);
        let mut gfx = Gfx::new(&scene::fonts_dir(), SCREEN, 1.0).expect("fonts");
        let (list, logs) = sc.run(&mut gfx);
        let rgba = renderer.render_to_rgba(&list).expect("render");
        let oracle = scene::load_oracle_png(name);
        assert_eq!(oracle.dimensions(), sc.crop(), "{name}: crop size");
        let mask = scene::text_mask(&list, SCREEN.0, SCREEN.1);
        let (d, heat) = scene::diff(&rgba, SCREEN.0, &oracle, &mask);
        let (w, h) = oracle.dimensions();
        scene::save(&scene::crop(&rgba, SCREEN.0, w, h), &out.join(format!("{name}.ours.png")));
        scene::save(&heat, &out.join(format!("{name}.heat.png")));
        println!(
            "{:<15} {:>9} {:>7} {:>7.3}% {:>5} | {:>7} {:>6} {:>5} | {:?}",
            name,
            d.pixels,
            d.bad,
            d.frac() * 100.0,
            d.max_delta,
            d.text_pixels,
            d.text_bad,
            d.text_max_delta,
            d.bbox
        );
        if !logs.is_empty() {
            println!("  logs: {logs:?}");
        }
        if perf > 0 {
            // CPU: recording the frame; GPU: render + readback of the full 1280x800 frame.
            let t = Instant::now();
            for _ in 0..perf {
                std::hint::black_box(sc.run(&mut gfx));
            }
            let rec = t.elapsed().as_secs_f64() * 1e6 / f64::from(perf);
            let t = Instant::now();
            for _ in 0..perf {
                std::hint::black_box(renderer.render_to_rgba(&list).expect("render"));
            }
            let gpu = t.elapsed().as_secs_f64() * 1e3 / f64::from(perf);
            println!("  perf: {} commands, record {rec:.1} µs/frame, render+readback {gpu:.2} ms/frame", list.commands.len());
        }
    }
}
