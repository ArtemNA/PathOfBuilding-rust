//! Path of Building (Rust): opens the window and runs the ported `Launch.lua` / `Main.lua` (U2b: the build list; the
//! build screen is U2c).
//!   cargo run -p pob-app --release [-- --pob <PathOfBuilding checkout>] [--user-path <documents dir>] [--secs N] [--open <build name>]
//! - The root whose `src/` (tree assets), `runtime/SimpleGraphic/Fonts`, `manifest.xml`, `changelog.txt` and `help.txt` are read
//!   (read-only): the folder of the exe when these are there (the package, `tools/package.ps1`), then `--pob`, `POB_ROOT`, and
//!   in a development tree `pobRoot` of the `tools/upstream.json` found above the exe (UI-ARCH §10 п. 7).
//! - `--user-path`: replaces the "Documents" folder (`GetUserPath`); user data goes to
//!   `<it>/Path of Building (Rust)/` (UI-ARCH §10 п. 2) — never into the original's `Documents\Path of Building`.
//! - `--secs N`: closes the window after N seconds (smoke runs).
//! - `--fps-log`: prints the `OnFrame` calls of every second to stderr (the idle measurement).
//! - `--open <name>`: opens `<user data>/Builds/<name>.xml` at start (`main:SetMode("BUILD", ...)`, the idle measurement of the tree).
//! - The `package` feature: no console window. Everything on stderr (`Downloading page`, the frame stats) then goes nowhere.

#![cfg_attr(feature = "package", windows_subsystem = "windows")]

use pob_gfx::window::{MainObject, WindowConfig};
use pob_gfx::{Frame, Gfx};
use pob_ui::launch::{App, LaunchConfig};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The icon of the window (and, through build.rs, of the exe in the package).
const WINDOW_ICON: &[u8] = include_bytes!("../assets/icon.png");

/// The folder of the fonts under a root.
const FONTS: &str = "runtime/SimpleGraphic/Fonts";

/// What the application cannot start without under a root; `manifest.xml` is not here: without it the version is `?` (Launch.lua:24-26).
fn is_root(dir: &Path) -> bool {
    dir.join("src/TreeData").is_dir() && dir.join(FONTS).is_dir()
}

/// `pobRoot` of the `tools/upstream.json` nearest above `from` (absolute, or relative to the repo root).
fn dev_pob_root(from: &Path) -> Option<PathBuf> {
    from.ancestors().find_map(|dir| {
        let text = std::fs::read_to_string(dir.join("tools/upstream.json")).ok()?;
        let json: serde_json::Value = serde_json::from_str(&text).ok()?;
        Some(dir.join(json.get("pobRoot")?.as_str()?))
    })
}

/// The root to read from, or the message of why there is none.
fn find_root(args: &[String]) -> Result<PathBuf, String> {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    if let Some(dir) = exe_dir.as_deref().filter(|d| is_root(d)) {
        return Ok(dir.to_path_buf());
    }
    let (root, source) = match (arg(args, "--pob"), std::env::var_os("POB_ROOT")) {
        (Some(p), _) => (Some(PathBuf::from(p)), "--pob"),
        (None, Some(p)) => (Some(PathBuf::from(p)), "POB_ROOT"),
        (None, None) => (exe_dir.as_deref().and_then(dev_pob_root), "tools/upstream.json"),
    };
    match root {
        Some(root) if is_root(&root) => Ok(root),
        Some(root) => Err(format!(
            "The application's files were not found.\n\nNot next to the program, and {source} points to\n{}\nwhich has no src/TreeData or {FONTS}.\n\nKeep the program in its folder together with data, src and runtime.",
            root.display()
        )),
        None => Err(format!(
            "The application's files were not found.\n\nThe folder of the program has no src/TreeData or {FONTS}.\n\nKeep the program in its folder together with data, src and runtime."
        )),
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn message_box(text: &str) {
    use std::ffi::c_void;
    /// The title of an error box.
    const ERROR_TITLE: &str = pob_ui::launch::APP_NAME;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    /// `MB_OK | MB_ICONERROR`.
    const KIND: u32 = 0x10;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (text, caption) = (wide(text), wide(ERROR_TITLE));
    // SAFETY: both strings are NUL-terminated and outlive the call; a null owner window is allowed.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), KIND);
    }
}

#[cfg(not(windows))]
fn message_box(_text: &str) {}

/// An error that stops the start: shown in a window, because the package has no console.
fn fatal(msg: &str) -> ! {
    eprintln!("pob-app: {msg}");
    message_box(msg);
    std::process::exit(1);
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
        pob_ui::launch::launch().check_for_update_at_start();
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
            s["importReal"] = pob_ui::tabs::import::debug_input_state(frame);
            s["compareReal"] = pob_ui::tabs::compare::debug_input_state(frame);
            s["tradeReal"] = pob_ui::trade::debug_input_state(frame);
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
        .map(|i| args.get(i + 1).unwrap_or_else(|| fatal(&format!("{name} needs a value"))).clone())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let pob = find_root(&args).unwrap_or_else(|e| fatal(&e));
    if let Err(e) = pob_ui::launch::check_data() {
        fatal(&format!("The application's data could not be read.\n\n{e}"));
    }
    let pob = pob.to_string_lossy().replace('\\', "/");
    let pob = pob.trim_end_matches('/').to_string();
    let user_path = arg(&args, "--user-path")
        .map(|p| p.replace('\\', "/"))
        .or_else(pob_gfx::fs::get_user_path);
    let secs = arg(&args, "--secs").map(|s| s.parse::<f64>().unwrap_or_else(|_| fatal("--secs needs a number")));
    let mut cfg = LaunchConfig {
        user_path,
        user_folder: USER_FOLDER.to_string(),
        script_path: format!("{pob}/src/"),
        doc_path: format!("{pob}/"),
        version_number: "?".into(),
        version_branch: None,
        version_platform: None,
        app_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };
    cfg.read_manifest(&format!("{pob}/manifest.xml"));
    let runner = Runner {
        fps_log: args.iter().any(|a| a == "--fps-log").then_some((0, 0)),
        open: arg(&args, "--open"),
        app: {
            // the only place the real network is turned on; `--no-network`: a check of the window (tools/sg-oracle/trade-rust-input.ps1), every request refused
            let app = if args.iter().any(|a| a == "--no-network") {
                App::new(cfg)
            } else {
                App::new(cfg).with_transport(std::sync::Arc::new(pob_net::UreqTransport))
            };
            // Main.lua:46-49
            app.launch.no_ssl.set(args.iter().any(|a| a == "--no-ssl"));
            // Main.lua:71 `arg[1]`: the protocol handler starts the program with the URI. The port's own switches are not arguments of the original
            *app.launch.start_uri.borrow_mut() = args.get(1).filter(|a| !a.starts_with("--")).cloned();
            app
        },
        secs,
        start: Instant::now(),
        state_file: arg(&args, "--state-file"),
        events: Vec::new(),
    };
    let wcfg = WindowConfig {
        title: pob_ui::launch::APP_NAME.into(),
        size: (1280, 800),
        fonts_dir: format!("{pob}/{FONTS}").into(),
    };
    match pob_gfx::window::run_with_icon(runner, wcfg, Some(WINDOW_ICON)) {
        Ok((_, stats)) => {
            let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
            eprintln!(
                "first frame {:.1} ms, OnFrame calls {}, presented {}",
                stats.first_frame.map_or(f64::NAN, ms),
                stats.frames,
                stats.presented
            );
        }
        Err(e) => fatal(&e.to_string()),
    }
}
