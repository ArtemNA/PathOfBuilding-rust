//! `Launch.lua` — the program entry point: loads and runs `main` in a protected environment (`PCall` → a Rust panic
//! caught with `catch_unwind`, UI-ARCH §4.5) and shows errors as a modal prompt.
//!
//! Not ported (UI-ARCH §10 п. 6): the update machinery (`first.run` install step, `CheckForUpdate`, `ApplyUpdate`,
//! `LaunchSubScript` sub-scripts, `DownloadPage` — network is U8) and the dev-mode hotkeys that need a Lua state
//! (F6 garbage collection, PAUSE profiler). `devMode` is always false: the Rust port has no "running from the
//! repository" mode, and its user data never goes next to the scripts (UI-ARCH §10 п. 2).

use crate::modules::main::Main;
use crate::sg;
use pob_gfx::{Frame, Gfx, MainObject};
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

/// `APP_NAME` (Launch.lua:9 "Path of Building"): "(Rust)" added by the user's decision, to tell the port's window from the original's.
pub const APP_NAME: &str = "Path of Building (Rust)";

/// Where things are (the original takes them from the working directory, `manifest.xml` and `GetUserPath`).
#[derive(Clone, Debug)]
pub struct LaunchConfig {
    /// `GetUserPath()` (the "Documents" folder); `None` = the user path cannot be obtained.
    pub user_path: Option<String>,
    /// The folder under it: `"Path of Building (Rust)"` (UI-ARCH §10 п. 2); tests use any name in a temp dir.
    pub user_folder: String,
    /// PoB's `src/` (assets: `TreeData/`, `Assets/`), with a trailing `/`.
    pub script_path: String,
    /// The folder of `changelog.txt` / `help.txt` (the original reads them from its working directory).
    pub doc_path: String,
    /// `<Version number= branch= platform=>` of `manifest.xml` (`"?"` when absent, Launch.lua:24-26).
    pub version_number: String,
    pub version_branch: Option<String>,
    pub version_platform: Option<String>,
}

impl LaunchConfig {
    /// Reads `<Version>` from `manifest.xml` at `path` (Launch.lua:46-58); missing file/element → `"?"`.
    pub fn read_manifest(&mut self, path: &str) {
        self.version_number = "?".into();
        self.version_branch = Some("?".into());
        self.version_platform = Some("?".into());
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let Ok(nodes) = pob_build::xml::parse_xml_document(&text) else {
            return;
        };
        let root = nodes.iter().find_map(|n| match n {
            pob_build::xml::XmlNode::Element(e) => Some(e),
            pob_build::xml::XmlNode::Text(_) => None,
        });
        if let Some(root) = root
            && root.name == "PoBVersion"
        {
            for node in root.child_elements() {
                if node.name == "Version" {
                    self.version_number = node.attr("number").unwrap_or("?").to_string();
                    self.version_branch = node.attr("branch").map(str::to_string);
                    self.version_platform = node.attr("platform").map(str::to_string);
                }
            }
        }
    }
}

/// The prompt's key handler (`promptFunc`): `true` dismisses the prompt.
pub type PromptFunc = Rc<dyn Fn(&mut Gfx, &str) -> bool>;

/// The `launch` object.
pub struct Launch {
    pub cfg: LaunchConfig,
    pub dev_mode: Cell<bool>,
    pub installed_mode: Cell<bool>,
    pub version_number: RefCell<String>,
    pub version_branch: RefCell<Option<String>>,
    pub version_platform: RefCell<Option<String>>,
    pub start_time: Cell<u64>,
    pub main: RefCell<Option<Rc<Main>>>,
    pub prompt_msg: RefCell<Option<String>>,
    pub prompt_col: Cell<(f64, f64, f64)>,
    pub prompt_func: RefCell<Option<PromptFunc>>,
    pub do_restart: RefCell<Option<String>>,
    /// Set by `Restart()`: the host reloads everything after this frame.
    pub restart_requested: Cell<bool>,
    pub dev_mode_alt: Cell<bool>,
    /// `launch.connectionProtocol` / `launch.proxyURL` / `launch.noSSL` (kept for Settings.xml; network is U8).
    pub connection_protocol: Cell<Option<f64>>,
    pub proxy_url: RefCell<Option<String>>,
    /// `launch.updateAvailable` / `updateErrMsg` / `updateCheckRunning`: never set (no update check in the port).
    pub update_available: RefCell<Option<String>>,
    pub update_err_msg: RefCell<Option<String>>,
}

thread_local! {
    static LAUNCH: RefCell<Option<Rc<Launch>>> = const { RefCell::new(None) };
}

/// The global `launch`.
pub fn launch() -> Rc<Launch> {
    LAUNCH
        .with(|l| l.borrow().clone())
        .expect("attempt to index global 'launch' (a nil value)")
}

/// `PCall(func, ...)`: `Err(message)` when the function raised an error (panicked).
pub fn pcall<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|e| {
        if let Some(s) = e.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown error".to_string()
        }
    })
}

impl Launch {
    /// `launch = { }` + `SetMainObject(launch)` (Launch.lua:16-17): registers the global.
    pub fn new(cfg: LaunchConfig) -> Rc<Self> {
        let l = Rc::new(Self {
            cfg,
            dev_mode: Cell::new(false),
            installed_mode: Cell::new(false),
            version_number: RefCell::new("?".into()),
            version_branch: RefCell::new(Some("?".into())),
            version_platform: RefCell::new(Some("?".into())),
            start_time: Cell::new(0),
            main: RefCell::new(None),
            prompt_msg: RefCell::new(None),
            prompt_col: Cell::new((0.0, 0.0, 0.0)),
            prompt_func: RefCell::new(None),
            do_restart: RefCell::new(None),
            restart_requested: Cell::new(false),
            dev_mode_alt: Cell::new(false),
            connection_protocol: Cell::new(None),
            proxy_url: RefCell::new(None),
            update_available: RefCell::new(None),
            update_err_msg: RefCell::new(None),
        });
        LAUNCH.with(|g| *g.borrow_mut() = Some(Rc::clone(&l)));
        l
    }

    pub fn main(&self) -> Option<Rc<Main>> {
        self.main.borrow().clone()
    }

    /// `launch:OnInit()` (Launch.lua:21-88).
    pub fn on_init(self: &Rc<Self>, g: &mut Gfx) {
        self.start_time.set(g.get_time());
        g.set_window_title(APP_NAME);
        g.con_execute("set vid_mode 8");
        g.con_execute("set vid_resizable 3");
        *self.version_number.borrow_mut() = self.cfg.version_number.clone();
        *self.version_branch.borrow_mut() = self.cfg.version_branch.clone();
        *self.version_platform.borrow_mut() = self.cfg.version_platform.clone();
        g.render_init(&["DPI_AWARE"]);
        g.con_printf("Loading main script...");
        match pcall(|| Main::init(g)) {
            Ok(m) => *self.main.borrow_mut() = Some(m),
            Err(e) => {
                // `self.main` is the module table even when `Init` failed (Launch.lua:72-81).
                *self.main.borrow_mut() = crate::modules::main::try_main();
                self.show_err_msg(&format!("In 'Init': {e}"));
            }
        }
    }

    /// `launch:CanExit()` (Launch.lua:90-101).
    pub fn can_exit(&self, g: &mut Gfx) -> bool {
        if let Some(main) = self.main()
            && self.prompt_msg.borrow().is_none()
        {
            return match pcall(|| main.can_exit(g)) {
                Ok(r) => r,
                Err(e) => {
                    self.show_err_msg(&format!("In 'CanExit': {e}"));
                    false
                }
            };
        }
        true
    }

    /// `launch:OnExit()` (Launch.lua:103-107).
    pub fn on_exit(&self, g: &mut Gfx) {
        if let Some(main) = self.main() {
            let _ = pcall(|| main.shutdown(g));
        }
    }

    /// `launch:OnFrame()` (Launch.lua:109-141).
    pub fn on_frame(&self, f: &mut Frame<'_>) {
        if let Some(main) = self.main()
            && let Err(e) = pcall(|| main.on_frame(f))
        {
            self.show_err_msg(&format!("In 'OnFrame': {e}"));
        }
        self.dev_mode_alt.set(self.dev_mode.get() && sg::is_key_down(f, "ALT"));
        sg::set_draw_layer(f, Some(1000), 0);
        f.set_viewport(None);
        let msg = self.prompt_msg.borrow().clone();
        if let Some(msg) = msg {
            let (r, g, b) = self.prompt_col.get();
            self.draw_popup(f, r, g, b, &format!("^0{msg}"));
        }
        let restart = self.do_restart.borrow().clone();
        if let Some(text) = restart {
            let (screen_w, screen_h) = f.get_virtual_screen_size();
            sg::set_draw_color_a(f, 0.0, 0.0, 0.0, 0.75);
            sg::draw_rect(f, 0.0, 0.0, screen_w, screen_h);
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            sg::draw_string(f, 0.0, screen_h / 2.0, Some("CENTER"), 24.0, "FIXED", &text);
            self.restart_requested.set(true);
        }
    }

    /// `launch:OnKeyDown(key, doubleClick)` (Launch.lua:143-175).
    pub fn on_key_down(&self, g: &mut Gfx, key: &str, double_click: bool) {
        if key == "F5" && self.dev_mode.get() {
            *self.do_restart.borrow_mut() = Some("Restarting...".into());
        } else if key == "u" && sg::is_key_down(g, "CTRL") {
            // `if not self.devMode then self:CheckForUpdate() end`: no update check in the port.
        } else if key == "PRINTSCREEN" && sg::is_key_down(g, "CTRL") {
            g.take_screenshot();
        } else if self.prompt_msg.borrow().is_some() {
            self.run_prompt_func(g, key);
        } else if let Some(main) = self.main()
            && let Err(e) = pcall(|| main.on_key_down(key, double_click))
        {
            self.show_err_msg(&format!("In 'OnKeyDown': {e}"));
        }
    }

    /// `launch:OnKeyUp(key)` (Launch.lua:177-186).
    pub fn on_key_up(&self, _g: &mut Gfx, key: &str) {
        if self.prompt_msg.borrow().is_none()
            && let Some(main) = self.main()
            && let Err(e) = pcall(|| main.on_key_up(key))
        {
            self.show_err_msg(&format!("In 'OnKeyUp': {e}"));
        }
    }

    /// `launch:OnChar(key)` (Launch.lua:188-199).
    pub fn on_char(&self, g: &mut Gfx, key: &str) {
        if self.prompt_msg.borrow().is_some() {
            self.run_prompt_func(g, key);
        } else if let Some(main) = self.main()
            && let Err(e) = pcall(|| main.on_char(key))
        {
            self.show_err_msg(&format!("In 'OnChar': {e}"));
        }
    }

    /// `launch:ShowPrompt(r, g, b, str, func)` (Launch.lua:358-379).
    pub fn show_prompt(&self, r: f64, g: f64, b: f64, s: &str, func: Option<PromptFunc>) {
        *self.prompt_msg.borrow_mut() = Some(s.to_string());
        self.prompt_col.set((r, g, b));
        let text = s.to_string();
        let default: PromptFunc = Rc::new(move |g: &mut Gfx, key: &str| {
            if key == "RETURN" || key == "ESCAPE" {
                return true;
            } else if key == "F4" {
                let l = launch();
                if let Some(main) = l.main() {
                    main.popups.borrow_mut().clear();
                    main.input_events.borrow_mut().clear();
                    main.set_mode("LIST", Vec::new());
                }
                return true;
            } else if key == "c" && sg::is_key_down(g, "CTRL") {
                let clean = crate::lua::gsub(&text, "%^%d", "");
                g.copy(&clean);
            } else if key == "F5" {
                *launch().do_restart.borrow_mut() = Some("Restarting...".into());
                return true;
            }
            false
        });
        *self.prompt_func.borrow_mut() = Some(func.unwrap_or(default));
    }

    /// `launch:ShowErrMsg(fmt, ...)` (Launch.lua:381-388): the message is already formatted.
    pub fn show_err_msg(&self, msg: &str) {
        if self.prompt_msg.borrow().is_none() {
            let version = format!(
                "^8v{}{}",
                self.version_number.borrow(),
                self.version_branch.borrow().as_ref().map(|b| format!(" {b}")).unwrap_or_default()
            );
            self.show_prompt(
                1.0,
                0.0,
                0.0,
                &format!(
                    "^1Error:\n\n^0{msg}\n{version}\n^0Press Enter/Escape to dismiss, F4 to return to build selection, or F5 to restart the application.\nPress CTRL + C to copy error text."
                ),
                None,
            );
        }
    }

    /// `launch:RunPromptFunc(key)` (Launch.lua:390-398).
    pub fn run_prompt_func(&self, g: &mut Gfx, key: &str) {
        let cur = self.prompt_msg.borrow().clone();
        let func = self.prompt_func.borrow().clone();
        let Some(func) = func else { return };
        match pcall(|| func(g, key)) {
            Err(e) => self.show_err_msg(&format!("In prompt func: {e}")),
            Ok(ret) => {
                if ret && *self.prompt_msg.borrow() == cur {
                    *self.prompt_msg.borrow_mut() = None;
                }
            }
        }
    }

    /// `launch:DrawPopup(r, g, b, fmt, ...)` (Launch.lua:400-416).
    pub fn draw_popup(&self, f: &mut Frame<'_>, r: f64, g: f64, b: f64, txt: &str) {
        let (screen_w, screen_h) = f.get_virtual_screen_size();
        sg::set_draw_color_a(f, 0.0, 0.0, 0.0, 0.5);
        sg::draw_rect(f, 0.0, 0.0, screen_w, screen_h);
        let w = sg::draw_string_width(f, 20.0, "VAR", txt) + 20.0;
        let h = (txt.bytes().filter(|&c| c == b'\n').count() as f64 + 2.0) * 20.0;
        let ox = (screen_w - w) / 2.0;
        let oy = (screen_h - h) / 2.0;
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_rect(f, ox, oy, w, h);
        sg::set_draw_color(f, r, g, b);
        sg::draw_rect(f, ox + 2.0, oy + 2.0, w - 4.0, h - 4.0);
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_rect(f, ox + 4.0, oy + 4.0, w - 8.0, h - 8.0);
        sg::draw_string(f, 0.0, oy + 10.0, Some("CENTER"), 20.0, "VAR", txt);
    }
}

/// The host's main object: forwards the callbacks to the global `launch` and performs `Restart()` (a fresh `launch`
/// with the same configuration, like SimpleGraphic reloading `Launch.lua`).
pub struct App {
    pub launch: Rc<Launch>,
}

impl App {
    pub fn new(cfg: LaunchConfig) -> Self {
        Self { launch: Launch::new(cfg) }
    }
}

impl MainObject for App {
    fn on_init(&mut self, gfx: &mut Gfx) {
        self.launch.on_init(gfx);
    }
    fn on_frame(&mut self, frame: &mut Frame<'_>) {
        self.launch.on_frame(frame);
        if self.launch.restart_requested.get() {
            let cfg = self.launch.cfg.clone();
            crate::modules::main::clear_main();
            self.launch = Launch::new(cfg);
            self.launch.on_init(frame);
        }
    }
    fn on_key_down(&mut self, gfx: &mut Gfx, key: &str, double_click: bool) {
        self.launch.on_key_down(gfx, key, double_click);
    }
    fn on_key_up(&mut self, gfx: &mut Gfx, key: &str) {
        self.launch.on_key_up(gfx, key);
    }
    fn on_char(&mut self, gfx: &mut Gfx, ch: &str) {
        self.launch.on_char(gfx, ch);
    }
    fn can_exit(&mut self, gfx: &mut Gfx) -> bool {
        self.launch.can_exit(gfx)
    }
    fn on_exit(&mut self, gfx: &mut Gfx) {
        self.launch.on_exit(gfx);
    }
}
