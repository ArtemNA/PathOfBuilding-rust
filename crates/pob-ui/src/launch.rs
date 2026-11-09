//! `Launch.lua` — the program entry point: loads and runs `main` in a protected environment (`PCall` → a Rust panic
//! caught with `catch_unwind`, UI-ARCH §4.5) and shows errors as a modal prompt.
//!
//! Not ported (UI-ARCH §10 п. 6): the update machinery (`first.run` install step, `CheckForUpdate`, `ApplyUpdate`: the
//! original's update is not ours, the port's own is the user's decision) and the dev-mode hotkeys that need a Lua state
//! (F6 garbage collection, PAUSE profiler). `devMode` is always false: the Rust port has no "running from the
//! repository" mode, and its user data never goes next to the scripts (UI-ARCH §10 п. 2).
//!
//! `LaunchSubScript`, `OnSubFinished`, `OnSubError`, `DownloadPage` are here over `pob_net`. The network is off until the application's `main` calls [`Launch::set_transport`]: a test
//! never reaches it by accident.

use crate::modules::main::Main;
use crate::sg;
use pob_gfx::{Frame, Gfx, MainObject};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;

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
    /// Only `pob-app` sets it: the port's own version, shown before upstream's in the `Version:` line. `None` (the tests and runners): that line as in the original.
    pub app_version: Option<String>,
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

/// The check of the converted data at the start of the application, before any window: the message of what is wrong with it.
pub fn check_data() -> Result<(), String> {
    let dir = pob_data::ensure_data().map_err(|e| e.to_string())?;
    crate::src_cache::check(dir)
}

/// `{ header = responseHeader, body = responseBody }` of `launch:DownloadPage`.
#[derive(Clone, Debug, Default)]
pub struct PageResponse {
    pub header: String,
    pub body: String,
}

/// `params = { header, body }`.
#[derive(Clone, Debug, Default)]
pub struct PageParams {
    pub header: Option<String>,
    pub body: Option<String>,
}

type SubCallback = Box<dyn FnOnce(&mut Gfx, pob_net::Outcome)>;

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
    /// `--no-ssl` (Main.lua:46-49).
    pub no_ssl: Cell<bool>,
    /// `arg[1]` (Main.lua:71): the URI the program was started with. Taken once: `arg[1] = nil` (:82), a restart has no argument.
    pub start_uri: RefCell<Option<String>>,
    /// The running sub-scripts: `self.subScripts`.
    pub net: pob_net::Net,
    subs: RefCell<HashMap<u64, SubCallback>>,
    /// A gate delivers the replies at its own step: the frames leave them waiting.
    hold_subs: Cell<bool>,
    /// `launch.updateAvailable` / `updateErrMsg` / `updateCheckRunning`: never set (no update check in the port).
    pub update_available: RefCell<Option<String>>,
    pub update_err_msg: RefCell<Option<String>>,
    /// The port's own update, while it is being looked for or downloaded.
    pub update_check_running: Cell<bool>,
    /// The update `update_available` names.
    pub update_pending: RefCell<Option<crate::update::Pending>>,
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

/// The script of `DownloadPage`, after `perform` (Launch.lua:301-313): `responseBody, errMsg, responseHeader`.
pub fn page_of(outcome: pob_net::Outcome) -> (PageResponse, Option<String>) {
    match outcome {
        pob_net::Outcome::Http(Ok(r)) => {
            let err = if r.code != 200 {
                Some(format!("Response code: {}", r.code))
            } else if r.body.is_empty() {
                Some("No data returned".to_string())
            } else {
                None
            };
            let body = r.text();
            (PageResponse { header: r.header, body }, err)
        }
        pob_net::Outcome::Http(Err(e)) | pob_net::Outcome::Crashed(e) => (PageResponse::default(), Some(e)),
        pob_net::Outcome::Oauth(_) | pob_net::Outcome::Download(_) => (PageResponse::default(), Some("not a download".to_string())),
    }
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
            no_ssl: Cell::new(false),
            start_uri: RefCell::new(None),
            net: pob_net::Net::default(),
            subs: RefCell::new(HashMap::new()),
            hold_subs: Cell::new(false),
            update_available: RefCell::new(None),
            update_err_msg: RefCell::new(None),
            update_check_running: Cell::new(false),
            update_pending: RefCell::new(None),
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

    /// The way out: only the application's `main` gives the real one.
    pub fn set_transport(&self, t: Arc<dyn pob_net::Transport>) {
        self.net.set_transport(t);
    }

    /// One `easy` handle with what every sub-script sets (Launch.lua:276-292).
    pub fn net_request(&self, url: &str, header: Option<String>, body: Option<Vec<u8>>, follow_redirects: bool) -> pob_net::Request {
        pob_net::Request {
            url: url.to_owned(),
            header,
            body,
            connection_protocol: self.connection_protocol.get(),
            proxy_url: self.proxy_url.borrow().clone(),
            no_ssl: self.no_ssl.get(),
            user_agent: format!("Path of Building/{}", self.version_number.borrow()),
            follow_redirects,
        }
    }

    /// `LaunchSubScript` + `RegisterSubScript`: `None` when it did not start, and the callback never comes.
    pub fn launch_sub(&self, job: pob_net::Job, cb: impl FnOnce(&mut Gfx, pob_net::Outcome) + 'static) -> Option<u64> {
        let id = self.net.spawn(job)?;
        self.subs.borrow_mut().insert(id, Box::new(cb));
        Some(id)
    }

    /// `launch:CheckForUpdate()` (Launch.lua:339-356), asked for by the user. `UpdateCheck.lua` asks for the original's manifest and writes `Update/` next to its scripts: the original's update is not the port's to do,
    /// so only the application (`app_version`) has one, from the port's releases ([`crate::update`]).
    pub fn check_for_update(&self, g: &mut Gfx) {
        if self.cfg.app_version.is_none() {
            crate::modules::main::not_ported(g, "Launch.lua:339-356 CheckForUpdate: the port does not update the original");
            return;
        }
        self.run_update_check(true);
    }

    /// The check at the start of the program: it says nothing unless there is an update.
    pub fn check_for_update_at_start(&self) {
        if self.cfg.app_version.is_some() {
            self.run_update_check(false);
        }
    }

    fn run_update_check(&self, asked: bool) {
        if self.update_check_running.get() || self.update_pending.borrow().is_some() {
            return;
        }
        self.update_check_running.set(true);
        let header = Some("Accept: application/vnd.github+json".to_string());
        let req = self.net_request(crate::update::LATEST_URL, header, None, true);
        let started = self.launch_sub(pob_net::Job::Http(req), move |_, outcome| {
            launch().update_check_finished(outcome, asked);
        });
        if started.is_none() {
            self.update_check_running.set(false);
        }
    }

    fn update_failed(&self, asked: bool, msg: String) {
        self.update_check_running.set(false);
        if asked {
            *self.update_err_msg.borrow_mut() = Some(msg);
        } else {
            eprintln!("Update check: {msg}");
        }
    }

    fn update_check_finished(&self, outcome: pob_net::Outcome, asked: bool) {
        let (page, err) = page_of(outcome);
        if let Some(e) = err {
            return self.update_failed(asked, e);
        }
        let release = match crate::update::parse_release(&page.body) {
            Ok(r) => r,
            Err(e) => return self.update_failed(asked, e),
        };
        let current = self.cfg.app_version.clone().unwrap_or_default();
        if !crate::update::is_newer(&release.version, &current) {
            self.update_check_running.set(false);
            if asked {
                *self.update_available.borrow_mut() = Some("none".to_string());
            }
            return;
        }
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(std::path::Path::to_path_buf));
        let kind = exe_dir.map_or(crate::update::Install::Manual, |d| crate::update::install_kind(&d));
        let asset = crate::update::asset_name(kind, &release.version).and_then(|n| release.assets.into_iter().find(|a| a.name == n));
        let Some(asset) = asset else {
            // Nothing this copy can replace itself with: the user gets the page.
            self.update_ready(crate::update::Pending {
                version: release.version,
                kind: crate::update::Install::Manual,
                file: None,
                page: release.page,
            });
            return;
        };
        let path = crate::update::download_path(&asset.name);
        let req = self.net_request(&asset.url, None, None, true);
        let pending = crate::update::Pending {
            version: release.version,
            kind,
            file: Some(path.clone()),
            page: release.page,
        };
        let job = pob_net::Job::Download {
            req,
            path,
            sha256: asset.sha256,
        };
        let started = self.launch_sub(job, move |_, outcome| {
            let l = launch();
            match outcome {
                pob_net::Outcome::Download(Ok(())) => l.update_ready(pending),
                pob_net::Outcome::Download(Err(e)) | pob_net::Outcome::Crashed(e) => l.update_failed(asked, e),
                _ => l.update_failed(asked, "the download did not run".to_string()),
            }
        });
        if started.is_none() {
            self.update_failed(asked, "the download could not start".to_string());
        }
    }

    fn update_ready(&self, pending: crate::update::Pending) {
        self.update_check_running.set(false);
        *self.update_available.borrow_mut() = Some(pending.version.clone());
        *self.update_pending.borrow_mut() = Some(pending);
    }

    /// `launch:ApplyUpdate(mode)` (Launch.lua:325-337): the installer takes over and starts the new version; a copy that cannot be replaced gets the release page.
    pub fn apply_update(&self, g: &mut Gfx) {
        let Some(pending) = self.update_pending.borrow().clone() else {
            return;
        };
        if pending.kind == crate::update::Install::Manual {
            g.open_url(&pending.page);
            return;
        }
        let exe = std::env::current_exe().unwrap_or_default();
        match crate::update::start_install(&pending, &exe) {
            Ok(()) => g.exit(None),
            Err(e) => *self.update_err_msg.borrow_mut() = Some(e),
        }
    }

    /// See `hold_subs`.
    pub fn hold_sub_scripts(&self, on: bool) {
        self.hold_subs.set(on);
    }

    /// `launch:DownloadPage(url, callback, params)` (Launch.lua:259-324).
    pub fn download_page(&self, url: &str, params: PageParams, cb: impl FnOnce(&mut Gfx, PageResponse, Option<String>) + 'static) {
        // :265 `ConPrintf("Downloading page at: %s", url)`, :312 `ConPrintf("Download complete. Status: %s", errMsg or "OK")`
        eprintln!("Downloading page at: {url}");
        let req = self.net_request(url, params.header, params.body.map(String::into_bytes), true);
        self.launch_sub(pob_net::Job::Http(req), move |g, outcome| {
            let (response, err) = page_of(outcome);
            eprintln!("Download complete. Status: {}", err.as_deref().unwrap_or("OK"));
            cb(g, response, err);
        });
    }

    /// `OnSubCall`, `OnSubFinished`, `OnSubError` of what has arrived (Launch.lua:201-244). The host calls them on the main thread between the frames, here at the start of one.
    pub fn poll_sub_scripts(&self, g: &mut Gfx) {
        for e in self.net.poll() {
            match e {
                pob_net::Event::OpenAuthUrl(_, url) => {
                    // LaunchServer.lua: `Copy(url)`, `OpenURL(url)`
                    g.copy(&url);
                    g.open_url(&url);
                }
                pob_net::Event::Finished(id, outcome) => {
                    let cb = self.subs.borrow_mut().remove(&id);
                    if let Some(cb) = cb
                        && let Err(err) = pcall(|| cb(g, outcome))
                    {
                        self.show_err_msg(&format!("In download callback: {err}"));
                    }
                }
            }
        }
    }

    /// `launch:OnFrame()` (Launch.lua:109-141).
    pub fn on_frame(&self, f: &mut Frame<'_>) {
        if !self.hold_subs.get() {
            self.poll_sub_scripts(f);
        }
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
            // `if not self.devMode then self:CheckForUpdate() end`
            if !self.dev_mode.get() {
                self.check_for_update(g);
            }
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
    /// What every `Launch` of this run talks through (a `Restart()` makes a new one).
    transport: Option<Arc<dyn pob_net::Transport>>,
}

impl App {
    pub fn new(cfg: LaunchConfig) -> Self {
        Self {
            launch: Launch::new(cfg),
            transport: None,
        }
    }

    /// The application's `main`: the real network.
    pub fn with_transport(mut self, t: Arc<dyn pob_net::Transport>) -> Self {
        self.launch.set_transport(t.clone());
        self.transport = Some(t);
        self
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
            let no_ssl = self.launch.no_ssl.get();
            self.launch = Launch::new(cfg);
            self.launch.no_ssl.set(no_ssl);
            if let Some(t) = &self.transport {
                self.launch.set_transport(t.clone());
            }
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
