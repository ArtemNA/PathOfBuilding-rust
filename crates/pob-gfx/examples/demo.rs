//! U1.5 demo: a window with the gallery of SG-oracle scene `u15-demo` (7 fonts, colours/escapes, layers, a clipped
//! "edit box", quads, images read from `PathOfBuilding/src/Assets`) plus a live input panel under it (last callbacks,
//! `GetCursorPos`, `IsKeyDown`). Keys: Ctrl+C copies the panel text, Ctrl+V pastes into it, F12 = `TakeScreenshot`,
//! Escape = `Exit`.
//!   cargo run -p pob-gfx --release --example demo [-- --secs N] [--check]
//! `--secs N`: exit after N seconds (perf runs: idle CPU of the process is measured from outside, see the U1.5 report).
//! `--check`: takes a screenshot through the window path (frame 3), exits, and compares its top-left 640x400 with the
//! original's screenshot of the same scene (`tools/sg-oracle/fixtures/scenes/u15-demo.png`).

#[path = "../tests/scene/mod.rs"]
mod scene;

use pob_gfx::text::FontFamily;
use pob_gfx::window::{MainObject, WindowConfig};
use pob_gfx::{Align, Frame, Gfx};
use scene::Scene;
use std::time::Instant;

struct Demo {
    scene: Scene,
    secs: Option<f64>,
    check: bool,
    start: Instant,
    frames: u64,
    events: Vec<String>,
    typed: String,
}

impl Demo {
    fn log(&mut self, s: String) {
        self.events.push(s);
        if self.events.len() > 6 {
            self.events.remove(0);
        }
    }
}

impl MainObject for Demo {
    fn on_init(&mut self, gfx: &mut Gfx) {
        gfx.render_init(&["DPI_AWARE"]);
        gfx.set_dpi_scale_override_percent(self.scene.dpi);
        gfx.set_window_title("pob-gfx demo (U1.5)");
        gfx.set_screenshot_dir(scene::out_dir().join("demo-shots"));
    }

    fn on_frame(&mut self, f: &mut Frame<'_>) {
        self.frames += 1;
        if self.secs.is_some_and(|s| self.start.elapsed().as_secs_f64() > s) || (self.check && self.frames == 5) {
            f.exit(None);
        }
        if self.check && self.frames == 3 {
            f.take_screenshot();
        }
        // The gallery: exactly the oracle scene's calls (scene-runner resets layer/viewport/colour first).
        f.set_draw_layer(Some(0), Some(0)).expect("layer");
        f.set_viewport(None);
        f.set_draw_color(1.0, 1.0, 1.0, 1.0);
        self.scene.draw(f);
        // Live panel below it (not part of the --check crop).
        let (w, h) = f.get_virtual_screen_size();
        f.set_draw_layer(Some(0), Some(0)).expect("layer");
        f.set_viewport(None);
        f.set_draw_color(0.05, 0.05, 0.07, 1.0);
        f.draw_image(None, 0.0, 400.0, w, h - 400.0, None).expect("panel");
        let (cx, cy) = f.get_cursor_pos();
        let mods: Vec<&str> = ["CTRL", "SHIFT", "ALT", "LEFTBUTTON"]
            .into_iter()
            .filter(|k| f.is_key_down(k).unwrap_or(false))
            .collect();
        f.set_draw_color(1.0, 1.0, 1.0, 1.0);
        let head = format!("^7GetCursorPos ^xFFFF77{cx} {cy}^7   IsKeyDown: ^x88FF88{}", mods.join(" "));
        f.draw_string(10.0, 410.0, Align::Left, 16.0, FontFamily::Var, &head);
        f.draw_string(
            10.0,
            432.0,
            Align::Left,
            16.0,
            FontFamily::Fixed,
            &format!("^8typed:^7 {}", self.typed),
        );
        let ev = self.events.join("\n");
        f.draw_string(10.0, 456.0, Align::Left, 14.0, FontFamily::Fixed, &ev);
        f.draw_string(
            10.0,
            h - 20.0,
            Align::Left,
            14.0,
            FontFamily::Var,
            "^8Ctrl+C copy, Ctrl+V paste, F12 screenshot, Esc exit",
        );
    }

    fn on_key_down(&mut self, g: &mut Gfx, key: &str, dbl: bool) {
        self.log(format!("OnKeyDown({key:?}, {dbl})"));
        let ctrl = g.is_key_down("CTRL").unwrap_or(false);
        match key {
            "ESCAPE" => g.exit(None),
            "F12" => g.take_screenshot(),
            "c" if ctrl => g.copy(&self.typed),
            "v" if ctrl => {
                if let Some(t) = g.paste() {
                    self.typed.push_str(&t);
                }
            }
            _ => {}
        }
    }

    fn on_key_up(&mut self, _g: &mut Gfx, key: &str) {
        self.log(format!("OnKeyUp({key:?})"));
    }

    fn on_char(&mut self, _g: &mut Gfx, ch: &str) {
        self.log(format!("OnChar({ch:?})"));
        match ch {
            "\u{8}" => {
                self.typed.pop();
            }
            c if c.len() == 1 && c.as_bytes()[0] >= 32 => self.typed.push_str(c),
            _ => {}
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs = args
        .iter()
        .position(|a| a == "--secs")
        .map(|i| args[i + 1].parse::<f64>().expect("--secs N"));
    let check = args.iter().any(|a| a == "--check");
    let demo = Demo {
        scene: Scene::load("u15-demo"),
        secs,
        check,
        start: Instant::now(),
        frames: 0,
        events: Vec::new(),
        typed: String::new(),
    };
    let shots = scene::out_dir().join("demo-shots");
    let before: Vec<_> = std::fs::read_dir(&shots)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let cfg = WindowConfig {
        title: "pob-gfx demo".into(),
        size: (1280, 800),
        fonts_dir: scene::fonts_dir(),
    };
    let (demo, stats) = pob_gfx::run(demo, cfg).expect("window loop");
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
    println!(
        "cold start to first presented frame: {:.1} ms (window {:.1}, gpu {:.1}, fonts {:.1}); OnFrame {} / presented {} / screenshots {} in {:.2} s",
        stats.first_frame.map_or(f64::NAN, ms),
        ms(stats.window_ready),
        ms(stats.gpu_ready),
        ms(stats.fonts_ready),
        stats.frames,
        stats.presented,
        stats.screenshots,
        demo.start.elapsed().as_secs_f64()
    );
    if check {
        let shot = std::fs::read_dir(&shots)
            .expect("demo-shots")
            .flatten()
            .map(|e| e.path())
            .find(|p| !before.contains(p))
            .expect("a new screenshot");
        let ours = image::open(&shot).expect("png").to_rgba8();
        let oracle = scene::load_oracle_png("u15-demo");
        let (w, _) = ours.dimensions();
        let mask = vec![false; ours.as_raw().len() / 4];
        let (d, _) = scene::diff(ours.as_raw(), w, &oracle, &mask);
        println!(
            "window-path screenshot {} vs original u15-demo: {} px, {} off by > {} (maxΔ {})",
            shot.display(),
            d.pixels,
            d.bad,
            scene::TOLERANCE,
            d.max_delta
        );
        std::process::exit(i32::from(d.bad > 0));
    }
}
