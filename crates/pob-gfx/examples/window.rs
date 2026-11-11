//! Window smoke + perf: opens a 1280x800 window, draws a small static scene with a blinking caret (GetTime % 1000 < 500,
//! like EditControl.lua:374), logs input callbacks, and closes itself after `--secs` (default 3). Prints the cold
//! start (process start -> first presented frame), the `OnFrame` cadence and how many frames were elided.
//!   cargo run -p pob-gfx --release --example window [-- --secs 3]

#[path = "../tests/scene/mod.rs"]
mod scene;

use pob_gfx::text::FontFamily;
use pob_gfx::window::{MainObject, WindowConfig};
use pob_gfx::{Align, Frame, Gfx};
use std::time::Instant;

struct Demo {
    secs: f64,
    start: Instant,
    frames: Vec<f64>,
    events: Vec<String>,
}

impl MainObject for Demo {
    fn on_frame(&mut self, f: &mut Frame<'_>) {
        let t = self.start.elapsed().as_secs_f64();
        self.frames.push(t * 1e3);
        if t > self.secs {
            f.exit(None);
        }
        let (w, h) = f.get_virtual_screen_size();
        f.set_draw_color(0.08, 0.1, 0.12, 1.0);
        f.draw_image(None, 0.0, 0.0, w, h, None).unwrap();
        f.set_draw_color(1.0, 1.0, 1.0, 1.0);
        f.draw_string(
            10.0,
            10.0,
            Align::Left,
            16.0,
            FontFamily::Var,
            "pob-gfx U1.2 ^xFF8000window^7 smoke test",
        );
        f.draw_string(0.0, 40.0, Align::Center, 24.0, FontFamily::FontinSc, "Path of Building (Rust)");
        f.set_draw_layer(Some(10), None).unwrap();
        f.set_draw_color(0.2, 0.2, 0.4, 1.0);
        f.draw_image(None, 10.0, 80.0, 300.0, 24.0, None).unwrap();
        f.set_draw_color(1.0, 1.0, 1.0, 1.0);
        let text = "edit box";
        f.draw_string(14.0, 84.0, Align::Left, 16.0, FontFamily::Var, text);
        if f.get_time() % 1000 < 500 {
            let x = 14.0 + f.draw_string_width(16.0, FontFamily::Var, text) as f64;
            f.draw_image(None, x, 84.0, 1.0, 16.0, None).unwrap();
        }
        let last = self.events.iter().rev().take(5).cloned().collect::<Vec<_>>().join(" | ");
        f.draw_string(10.0, 120.0, Align::Left, 14.0, FontFamily::Fixed, &last);
    }
    fn on_key_down(&mut self, _g: &mut Gfx, key: &str, dbl: bool) {
        self.events.push(format!("down {key}{}", if dbl { " x2" } else { "" }));
    }
    fn on_key_up(&mut self, _g: &mut Gfx, key: &str) {
        self.events.push(format!("up {key}"));
    }
    fn on_char(&mut self, _g: &mut Gfx, ch: &str) {
        self.events.push(format!("char {ch:?}"));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs = args
        .iter()
        .position(|a| a == "--secs")
        .map_or(3.0, |i| args[i + 1].parse().expect("--secs N"));
    let demo = Demo {
        secs,
        start: Instant::now(),
        frames: Vec::new(),
        events: Vec::new(),
    };
    let cfg = WindowConfig {
        title: "pob-gfx window".into(),
        size: (1280, 800),
        fonts_dir: scene::fonts_dir(),
        state_file: None,
    };
    let (demo, stats) = pob_gfx::run(demo, cfg).expect("window loop");
    let mut d: Vec<f64> = demo.frames.windows(2).map(|w| w[1] - w[0]).collect();
    d.sort_by(f64::total_cmp);
    let med = d.get(d.len() / 2).copied().unwrap_or(0.0);
    let mean = d.iter().sum::<f64>() / d.len().max(1) as f64;
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
    println!(
        "cold start to first presented frame: {:.1} ms (window {:.1}, gpu {:.1}, fonts {:.1})",
        stats.first_frame.map_or(f64::NAN, ms),
        ms(stats.window_ready),
        ms(stats.gpu_ready),
        ms(stats.fonts_ready)
    );
    println!(
        "OnFrame calls {} in {:.2} s: interval median {:.1} ms, mean {:.1} ms, min {:.1}, max {:.1}; presented {} (rest elided)",
        stats.frames,
        secs,
        med,
        mean,
        d.first().copied().unwrap_or(0.0),
        d.last().copied().unwrap_or(0.0),
        stats.presented
    );
    println!("events: {}", demo.events.len());
}
