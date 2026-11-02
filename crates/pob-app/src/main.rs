//! Path of Building (Rust): opens the window and runs the ported `Launch.lua` / `Main.lua` (U2b: the build list; the
//! build screen is U2c).
//!   cargo run -p pob-app --release [-- --pob <PathOfBuilding checkout>] [--user-path <documents dir>] [--secs N]
//! - `--pob`: the PoB checkout whose `src/` (tree assets), `runtime/SimpleGraphic/Fonts`, `manifest.xml`,
//!   `changelog.txt` and `help.txt` are read (read-only; default `POB_ROOT` or the project's checkout, UI-ARCH §10 п. 7).
//! - `--user-path`: replaces the "Documents" folder (`GetUserPath`); user data goes to
//!   `<it>/Path of Building (Rust)/` (UI-ARCH §10 п. 2) — never into the original's `Documents\Path of Building`.
//! - `--secs N`: closes the window after N seconds (smoke runs).

use pob_gfx::window::{MainObject, WindowConfig};
use pob_gfx::{Frame, Gfx};
use pob_ui::launch::{App, LaunchConfig};
use std::time::Instant;

const DEFAULT_POB_ROOT: &str = "C:/pob-work/project_15/PathOfBuilding";

/// The data folder of the Rust port (UI-ARCH §10 п. 2).
const USER_FOLDER: &str = "Path of Building (Rust)";

struct Runner {
    app: App,
    secs: Option<f64>,
    start: Instant,
}

impl MainObject for Runner {
    fn on_init(&mut self, gfx: &mut Gfx) {
        self.app.on_init(gfx);
    }
    fn on_frame(&mut self, frame: &mut Frame<'_>) {
        self.app.on_frame(frame);
        if self.secs.is_some_and(|s| self.start.elapsed().as_secs_f64() > s) {
            frame.exit(None);
        }
    }
    fn on_key_down(&mut self, gfx: &mut Gfx, key: &str, double_click: bool) {
        self.app.on_key_down(gfx, key, double_click);
    }
    fn on_key_up(&mut self, gfx: &mut Gfx, key: &str) {
        self.app.on_key_up(gfx, key);
    }
    fn on_char(&mut self, gfx: &mut Gfx, ch: &str) {
        self.app.on_char(gfx, ch);
    }
    fn can_exit(&mut self, gfx: &mut Gfx) -> bool {
        self.app.can_exit(gfx)
    }
    fn on_exit(&mut self, gfx: &mut Gfx) {
        self.app.on_exit(gfx);
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .map(|i| args.get(i + 1).unwrap_or_else(|| panic!("{name} needs a value")).clone())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let pob = arg(&args, "--pob")
        .or_else(|| std::env::var("POB_ROOT").ok())
        .unwrap_or_else(|| DEFAULT_POB_ROOT.to_string())
        .replace('\\', "/");
    let pob = pob.trim_end_matches('/').to_string();
    let user_path = arg(&args, "--user-path")
        .map(|p| p.replace('\\', "/"))
        .or_else(pob_gfx::fs::get_user_path);
    let secs = arg(&args, "--secs").map(|s| s.parse::<f64>().expect("--secs N"));
    let mut cfg = LaunchConfig {
        user_path,
        user_folder: USER_FOLDER.to_string(),
        script_path: format!("{pob}/src/"),
        doc_path: format!("{pob}/"),
        version_number: "?".into(),
        version_branch: None,
        version_platform: None,
    };
    cfg.read_manifest(&format!("{pob}/manifest.xml"));
    let runner = Runner {
        app: App::new(cfg),
        secs,
        start: Instant::now(),
    };
    let wcfg = WindowConfig {
        title: pob_ui::launch::APP_NAME.into(),
        size: (1280, 800),
        fonts_dir: format!("{pob}/runtime/SimpleGraphic/Fonts").into(),
    };
    match pob_gfx::run(runner, wcfg) {
        Ok((_, stats)) => {
            let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
            eprintln!(
                "first frame {:.1} ms, OnFrame calls {}, presented {}",
                stats.first_frame.map_or(f64::NAN, ms),
                stats.frames,
                stats.presented
            );
        }
        Err(e) => {
            eprintln!("pob-app: {e}");
            std::process::exit(1);
        }
    }
}
