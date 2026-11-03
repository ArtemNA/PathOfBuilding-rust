//! Path of Building (Rust): opens the window and runs the ported `Launch.lua` / `Main.lua` (U2b: the build list; the
//! build screen is U2c).
//!   cargo run -p pob-app --release [-- --pob <PathOfBuilding checkout>] [--user-path <documents dir>] [--secs N] [--open <build name>]
//! - `--pob`: the PoB checkout whose `src/` (tree assets), `runtime/SimpleGraphic/Fonts`, `manifest.xml`,
//!   `changelog.txt` and `help.txt` are read (read-only; default `POB_ROOT`, else `pobRoot` of `tools/upstream.json`, UI-ARCH §10 п. 7).
//! - `--user-path`: replaces the "Documents" folder (`GetUserPath`); user data goes to
//!   `<it>/Path of Building (Rust)/` (UI-ARCH §10 п. 2) — never into the original's `Documents\Path of Building`.
//! - `--secs N`: closes the window after N seconds (smoke runs).
//! - `--fps-log`: prints the `OnFrame` calls of every second to stderr (the idle measurement).
//! - `--open <name>`: opens `<user data>/Builds/<name>.xml` at start (`main:SetMode("BUILD", ...)`, the idle measurement of the tree).

use pob_gfx::window::{MainObject, WindowConfig};
use pob_gfx::{Frame, Gfx};
use pob_ui::launch::{App, LaunchConfig};
use std::time::Instant;

/// `tools/upstream.json` (the ONE upstream config, R-UP), read at compile time for the default `--pob`.
const UPSTREAM_JSON: &str = include_str!("../../../tools/upstream.json");

/// `pobRoot` of `tools/upstream.json` (absolute, or relative to the repo root).
fn default_pob_root() -> String {
    let key = UPSTREAM_JSON.find("\"pobRoot\"").expect("tools/upstream.json: pobRoot");
    let rest = &UPSTREAM_JSON[key + "\"pobRoot\"".len()..];
    let open = rest.find('"').expect("pobRoot value");
    let len = rest[open + 1..].find('"').expect("pobRoot value");
    let p = std::path::PathBuf::from(&rest[open + 1..open + 1 + len]);
    let p = if p.is_absolute() {
        p
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(p)
    };
    p.to_string_lossy().into_owned()
}

/// The data folder of the Rust port (UI-ARCH §10 п. 2).
const USER_FOLDER: &str = "Path of Building (Rust)";

struct Runner {
    fps_log: Option<(u64, u32)>,
    open: Option<String>,
    app: App,
    secs: Option<f64>,
    start: Instant,
    state_file: Option<String>,
    /// With `--state-file`: the key events the window delivered (rust-input.ps1).
    events: Vec<String>,
}

impl MainObject for Runner {
    fn on_init(&mut self, gfx: &mut Gfx) {
        self.app.on_init(gfx);
        if let Some(name) = self.open.take() {
            let main = pob_ui::modules::main::main();
            let path = format!("{}{name}.xml", main.build_path.borrow());
            main.set_mode(
                "BUILD",
                vec![pob_ui::modules::main::ModeArg::Str(path), pob_ui::modules::main::ModeArg::Str(name)],
            );
        }
    }
    fn on_frame(&mut self, frame: &mut Frame<'_>) {
        self.app.on_frame(frame);
        // tools/sg-oracle/rust-input.ps1: the tree's state after each frame, for posted window messages
        if let Some(p) = &self.state_file {
            let mut s = pob_ui::tabs::tree::debug_input_state(frame);
            s["events"] = serde_json::json!(self.events);
            s["configReal"] = pob_ui::tabs::config::debug_input_state(frame);
            s["skillsReal"] = pob_ui::tabs::skills::debug_input_state(frame);
            s["itemsReal"] = pob_ui::tabs::items::debug_input_state(frame);
            let tmp = format!("{p}.tmp");
            if std::fs::write(&tmp, s.to_string()).is_ok() {
                let _ = std::fs::rename(&tmp, p);
            }
        }
        if let Some((sec, n)) = &mut self.fps_log {
            let now = self.start.elapsed().as_secs();
            if now != *sec {
                eprintln!("fps t={sec} n={n}");
                *sec = now;
                *n = 0;
            }
            *n += 1;
        }
        if self.secs.is_some_and(|s| self.start.elapsed().as_secs_f64() > s) {
            frame.exit(None);
        }
    }
    fn on_key_down(&mut self, gfx: &mut Gfx, key: &str, double_click: bool) {
        if self.state_file.is_some() {
            self.events.push(format!("down {key}{}", if double_click { " double" } else { "" }));
        }
        self.app.on_key_down(gfx, key, double_click);
    }
    fn on_key_up(&mut self, gfx: &mut Gfx, key: &str) {
        if self.state_file.is_some() {
            self.events.push(format!("up {key}"));
        }
        self.app.on_key_up(gfx, key);
    }
    fn on_char(&mut self, gfx: &mut Gfx, ch: &str) {
        if self.state_file.is_some() {
            self.events.push(format!("char {ch}"));
        }
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
        .unwrap_or_else(default_pob_root)
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
        fps_log: args.iter().any(|a| a == "--fps-log").then_some((0, 0)),
        open: arg(&args, "--open"),
        app: App::new(cfg),
        secs,
        start: Instant::now(),
        state_file: arg(&args, "--state-file"),
        events: Vec::new(),
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
