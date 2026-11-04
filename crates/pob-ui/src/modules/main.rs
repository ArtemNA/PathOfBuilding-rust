//! `Modules/Main.lua` — the `main` object (a global `ControlHost` in Lua): modes, popups, `Settings.xml`, the bottom
//! bar, the options / about / message / confirm / new-folder popups, folder copy/move helpers.
//!
//! Not ported here (later phases or never; each place is marked): the item databases and mod cache (`loadItemDBs`,
//! `SaveModCache` — U6), passive trees (`LoadTree` keeps only the background image `assets.Background2` — U3), the
//! the tabs of the `BUILD` mode ([`super::build`], U3–U8), build import from the command line (`arg[1]` — U8),
//! update popups and `SetManifestBranch` (no auto-update, UI-ARCH §10 п. 6), shared items are kept as raw text
//! (`new("Item")` is U6).

use super::build::BuildMode;
use super::build_list::ListMode;
use super::toast::ToastNotification;
use crate::colors::{default_color_code, update_color_code};
use crate::common::{screen_size, set_screen_size, wrap_string};
use crate::control::{Control, ControlBase, ControlRef, Rect, anchor, anchor_nil, rect};
use crate::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl, PopupDialog, RectangleOutlineControl,
    ScrollBarControl, SliderControl, TextColumn, TextLine, TextListControl,
};
use crate::host::{ControlHost, InputEvent, InputEvents, ipairs};
use crate::launch::{APP_NAME, launch};
use crate::lua::{self, tostring_num};
use crate::sg;
use indexmap::IndexMap;
use pob_build::xml::{XmlElement, XmlNode, compose_xml, parse_xml_document};
use pob_gfx::{Frame, Gfx, ImageHandle};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// A mode argument (`SetMode(newMode, ...)`; Settings.xml `<Arg number|string|boolean>`).
#[derive(Clone, Debug, PartialEq)]
pub enum ModeArg {
    Str(String),
    Num(f64),
    Bool(bool),
}

impl ModeArg {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// A mode table (`main.modes[name]`): `Init`, `Shutdown`, `OnFrame`, `GetArgs`, `CanExit` (Main.lua:513-518).
pub trait Mode {
    fn init(&self, g: &mut Gfx, args: &[ModeArg]);
    fn shutdown(&self, _g: &mut Gfx) {}
    fn on_frame(&self, f: &mut Frame<'_>, input_events: &mut InputEvents, view_port: &Rect);
    fn get_args(&self) -> Vec<ModeArg>;
    /// `nil` = the mode has no `CanExit`.
    fn can_exit(&self, _g: &mut Gfx, _reason: &str) -> Option<bool> {
        None
    }
    /// `modes[mode].viewMode` (F1 opens the help section of the current tab).
    fn view_mode(&self) -> Option<String> {
        None
    }
}

/// The item databases' share of a frame: the idle loop draws every 30 ms (`pob_gfx::window`), so it keeps drawing while they load.
const ITEM_DB_SLICE: std::time::Duration = std::time::Duration::from_millis(15);

/// `new("Control"):Control(nil, rect)`: a control without behaviour (anchors only).
pub struct BareControl {
    pub base: ControlBase,
}

impl BareControl {
    pub fn new(anchor: Option<crate::control::Anchor>, r: crate::control::RectProps) -> Rc<Self> {
        Rc::new(Self {
            base: ControlBase::new(anchor, r),
        })
    }
}

impl Control for BareControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }
}

/// The `main.*` options (Main.lua:102-126 defaults; Settings.xml `<Misc>`). `Option` = may be `nil` in Lua.
#[derive(Clone, Debug)]
pub struct Settings {
    pub build_sort_mode: String,
    pub connection_protocol: f64,
    pub node_power_theme: String,
    pub color_positive: String,
    pub color_negative: String,
    pub color_highlight: String,
    pub show_thousands_separators: bool,
    pub use_compact_values: bool,
    pub edge_search_highlight: bool,
    pub thousands_separator: String,
    pub decimal_separator: String,
    pub default_item_affix_quality: f64,
    pub show_titlebar_name: bool,
    pub dpi_scale_override_percent: f64,
    pub show_warnings: bool,
    pub slot_only_tooltips: bool,
    pub migrate_eldritch_implicits: bool,
    pub not_supported_mod_tooltips: bool,
    pub show_public_builds: bool,
    pub show_flavour_text: bool,
    pub show_animations: bool,
    pub show_all_item_affixes: bool,
    pub disable_scroll_control_interaction: bool,
    pub beta_test: Option<bool>,
    pub default_gem_quality: Option<f64>,
    pub default_char_level: Option<f64>,
    pub last_exported_website: Option<String>,
    pub invert_slider_scroll_direction: Option<bool>,
    pub disable_dev_auto_save: Option<bool>,
}

/// `<Accounts>` of Settings.xml.
#[derive(Clone, Debug, Default)]
pub struct Accounts {
    pub last_account_name: Option<String>,
    pub last_realm: Option<String>,
    pub last_league: Option<String>,
    pub last_token: Option<String>,
    pub last_refresh_token: Option<String>,
    pub token_expiry: Option<f64>,
}

/// `{ title, slots = { [slotName] = item } }` with items as raw text.
#[derive(Clone, Debug, Default)]
pub struct SharedItemSet {
    pub title: Option<String>,
    pub slots: IndexMap<String, String>,
}

pub struct Main {
    pub host: ControlHost,
    pub modes: IndexMap<&'static str, Rc<dyn Mode>>,
    pub list_mode: Rc<ListMode>,
    pub build_mode: Rc<BuildMode>,
    pub mode: RefCell<Option<String>>,
    pub new_mode: RefCell<Option<String>>,
    pub new_mode_args: RefCell<Vec<ModeArg>>,
    pub new_mode_change_to_tree: Cell<bool>,
    /// `self.popups`: index 0 = `popups[1]` (the top one).
    pub popups: RefCell<Vec<Rc<PopupDialog>>>,
    pub input_events: RefCell<InputEvents>,
    pub view_port: Cell<Rect>,
    pub portrait_mode: Cell<bool>,
    /// [`Main::load_background_data`]: 0 before the first frame's end, 1 while the item databases load, 2 after.
    startup_frames: Cell<u8>,
    pub user_path: RefCell<Option<String>>,
    pub build_path: RefCell<String>,
    pub default_build_path: RefCell<String>,
    pub s: RefCell<Settings>,
    pub error_reading_settings: Cell<bool>,
    pub accounts: RefCell<Accounts>,
    pub game_accounts: RefCell<IndexMap<String, Option<String>>>,
    /// `main.lastExportWebsite` (ImportTab.lua:536): the session only, no setting holds it.
    pub last_export_website: RefCell<Option<String>>,
    /// `main.predefinedBuildName` (ImportTab.lua:1931).
    pub predefined_build_name: RefCell<Option<String>>,
    /// `main.api`.
    pub api: Rc<crate::poe_api::PoeApi>,
    pub shared_item_list: RefCell<Vec<String>>,
    pub shared_item_set_list: RefCell<Vec<SharedItemSet>>,
    pub anchor_main: Rc<BareControl>,
    pub main_bar_height: Cell<f64>,
    pub toasts: ToastNotification,
    pub update_available_shown: Cell<bool>,
    /// `main.tree[latestTreeVersion].assets.Background2.handle` (U3 ports the tree; only the background is needed).
    pub background: ImageHandle,
    /// `main.filterBuildList` (the build list search text).
    pub filter_build_list: RefCell<Option<String>>,
    /// `itemLib.wiki.triggered` (U6).
    pub wiki_triggered: Cell<bool>,
    pub first_frame_done: Cell<bool>,
    /// `self.onFrameFuncs`.
    pub on_frame_funcs: RefCell<IndexMap<String, Rc<dyn Fn(&mut Gfx)>>>,
}

thread_local! {
    /// [`not_ported`]: the unported branches this session pressed.
    static NOT_PORTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// A control the original has and this port does not yet: no crash; the note (`file:line` of the branch) goes to the list, the console and a message popup.
pub fn not_ported(g: &mut Gfx, what: &str) {
    NOT_PORTED.with_borrow_mut(|n| {
        if !n.iter().any(|x| x == what) {
            n.push(what.to_string());
        }
    });
    g.con_printf(&format!("unimplemented: {what}"));
    main().open_message_popup(
        g,
        "Not ported yet",
        &format!("^7This branch of the original is not ported yet:\n{what}"),
    );
}

/// The same note, for what draws every frame (a tooltip): no popup.
pub fn note_not_ported(what: &str) {
    NOT_PORTED.with_borrow_mut(|n| {
        if !n.iter().any(|x| x == what) {
            n.push(what.to_string());
        }
    });
}

/// The notes of [`not_ported`].
pub fn not_ported_notes() -> Vec<String> {
    NOT_PORTED.with_borrow(Vec::clone)
}

thread_local! {
    static MAIN_OBJ: RefCell<Option<Rc<Main>>> = const { RefCell::new(None) };
}

/// The left of the two lines of [`version_lines`], from the main anchor; the About button ends at 140.
pub const VERSION_LINES_X: f64 = 144.0;

/// The application's two lines right of Options / About: the port's own version, then the upstream version of its data.
pub fn version_lines(app: &str, upstream: &str) -> (String, String) {
    (format!("^8PoB Rust {app}"), format!("^8Data: PoB {upstream}"))
}

/// The global `main`.
pub fn main() -> Rc<Main> {
    try_main().expect("attempt to index global 'main' (a nil value)")
}

pub fn try_main() -> Option<Rc<Main>> {
    MAIN_OBJ.with(|m| m.borrow().clone())
}

/// Drops the global (`Restart()`, tests).
pub fn clear_main() {
    MAIN_OBJ.with(|m| *m.borrow_mut() = None);
}

/// `DropDownControl:SelByValue(value, key)` for list items carrying a typed `data` value.
pub fn sel_by_value<T: PartialEq + 'static>(dd: &DropDownControl, value: &T) {
    let idx = dd
        .list
        .borrow()
        .iter()
        .position(|v| v.data.as_ref().and_then(|d| d.downcast_ref::<T>()) == Some(value));
    if let Some(i) = idx {
        dd.sel_index.set(i + 1);
    }
}

fn item<T: 'static>(label: &str, data: T) -> DropItem {
    let mut it = DropItem::table(label);
    it.data = Some(Rc::new(data) as Rc<dyn Any>);
    it
}

fn data<T: Clone + 'static>(it: &DropItem) -> T {
    it.data
        .as_ref()
        .and_then(|d| d.downcast_ref::<T>())
        .cloned()
        .expect("drop-down item data")
}

/// `tostring(v)` of a Lua boolean that may be `nil`.
fn tostring_opt_bool(v: Option<bool>) -> String {
    v.map_or("nil".into(), |b| b.to_string())
}

/// `color:gsub('^(^)', '0')`.
fn color_to_hex(c: &str) -> String {
    lua::gsub(c, "^(^)", "0")
}

impl Main {
    /// `main:Init()` (Main.lua:59-305).
    pub fn init(g: &mut Gfx) -> Rc<Main> {
        let l = launch();
        let list_mode = ListMode::new();
        let mut modes: IndexMap<&'static str, Rc<dyn Mode>> = IndexMap::new();
        modes.insert("LIST", list_mode.clone());
        let build_mode = BuildMode::new();
        modes.insert("BUILD", build_mode.clone());
        let anchor_main = BareControl::new(None, rect(4.0, 0.0, 0.0, 0.0));
        anchor_main.base.y.set_fn(|_| screen_size().1 - 4.0);
        let m = Rc::new(Main {
            host: ControlHost::new(),
            modes,
            list_mode,
            build_mode,
            mode: RefCell::new(None),
            new_mode: RefCell::new(None),
            new_mode_args: RefCell::new(Vec::new()),
            new_mode_change_to_tree: Cell::new(false),
            popups: RefCell::new(Vec::new()),
            input_events: RefCell::new(Vec::new()),
            view_port: Cell::new(Rect::default()),
            portrait_mode: Cell::new(false),
            user_path: RefCell::new(None),
            build_path: RefCell::new(String::new()),
            default_build_path: RefCell::new(String::new()),
            s: RefCell::new(Settings {
                build_sort_mode: "NAME".into(),
                connection_protocol: 0.0,
                node_power_theme: "RED/BLUE".into(),
                color_positive: default_color_code("POSITIVE").expect("POSITIVE"),
                color_negative: default_color_code("NEGATIVE").expect("NEGATIVE"),
                color_highlight: default_color_code("HIGHLIGHT").expect("HIGHLIGHT"),
                show_thousands_separators: true,
                use_compact_values: false,
                edge_search_highlight: true,
                thousands_separator: ",".into(),
                decimal_separator: ".".into(),
                default_item_affix_quality: 0.5,
                show_titlebar_name: true,
                dpi_scale_override_percent: f64::from(g.get_dpi_scale_override_percent()),
                show_warnings: true,
                slot_only_tooltips: true,
                migrate_eldritch_implicits: true,
                not_supported_mod_tooltips: true,
                show_public_builds: true,
                show_flavour_text: true,
                show_animations: true,
                show_all_item_affixes: true,
                disable_scroll_control_interaction: false,
                beta_test: None,
                default_gem_quality: None,
                default_char_level: None,
                last_exported_website: None,
                invert_slider_scroll_direction: None,
                disable_dev_auto_save: None,
            }),
            error_reading_settings: Cell::new(false),
            accounts: RefCell::new(Accounts::default()),
            game_accounts: RefCell::new(IndexMap::new()),
            last_export_website: RefCell::new(None),
            predefined_build_name: RefCell::new(None),
            api: Rc::new(crate::poe_api::PoeApi::default()),
            shared_item_list: RefCell::new(Vec::new()),
            shared_item_set_list: RefCell::new(Vec::new()),
            anchor_main,
            main_bar_height: Cell::new(58.0),
            toasts: ToastNotification::default(),
            update_available_shown: Cell::new(false),
            background: ImageHandle::new(),
            filter_build_list: RefCell::new(None),
            wiki_triggered: Cell::new(false),
            first_frame_done: Cell::new(false),
            on_frame_funcs: RefCell::new(IndexMap::new()),
            startup_frames: Cell::new(0),
        });
        MAIN_OBJ.with(|g| *g.borrow_mut() = Some(Rc::clone(&m)));
        m.sync_flags();
        // `arg[1]` (import a build from a URI) is U8.
        m.set_mode("BUILD", vec![ModeArg::Bool(false), ModeArg::Str("Unnamed build".into())]);
        if l.dev_mode.get() {
            unreachable!("devMode is never set in the port (launch.rs)");
        }
        match &l.cfg.user_path {
            None => m.open_path_popup(g, "", "Could not obtain Documents path", false),
            Some(p) => *m.user_path.borrow_mut() = Some(format!("{p}/{}/", l.cfg.user_folder)),
        }
        // `self:LoadTree(latestTreeVersion)`: PassiveTree:LoadImage("Background2.png", …, "ASYNC") (PassiveTree.lua:262-263, :909-932).
        let bg = format!("{}TreeData/Background2.png", l.cfg.script_path);
        if let Err(e) = m.background.load_lua(&bg, &["ASYNC"]) {
            panic!("{e}");
        }
        let up = m.user_path.borrow().clone();
        if let Some(up) = up {
            m.change_user_path(g, &up, false);
        }
        // Bottom bar (Main.lua:224-265).
        let am: ControlRef = m.anchor_main.clone();
        m.host.add(
            "options",
            ButtonControl::new(
                anchor("BOTTOMLEFT", &am, "BOTTOMLEFT"),
                rect(0.0, 0.0, 68.0, 20.0),
                "Options".to_string(),
                |g| {
                    main().open_options_popup(g, None);
                },
            ),
        );
        m.host.add(
            "about",
            ButtonControl::new(
                anchor("BOTTOMLEFT", &am, "BOTTOMLEFT"),
                rect(72.0, 0.0, 68.0, 20.0),
                "About".to_string(),
                |g| {
                    main().open_about_popup(g, None);
                },
            ),
        );
        let apply = ButtonControl::new(
            anchor("BOTTOMLEFT", &am, "BOTTOMLEFT"),
            rect(0.0, -24.0, 140.0, 20.0),
            "^x50E050Update Ready".to_string(),
            |_| unimplemented!("Main.lua:234-236 OpenUpdatePopup: no auto-update in the port (UI-ARCH §10 п. 6)"),
        );
        apply
            .base
            .shown
            .set_fn(|_| launch().update_available.borrow().as_deref().is_some_and(|u| u != "none"));
        m.host.add("applyUpdate", apply);
        // `checkUpdate` (Main.lua:240-251): no update check in the port — never shown (deliberate deviation, UI-ARCH §10 п. 6).
        let check = ButtonControl::new(
            anchor("BOTTOMLEFT", &am, "BOTTOMLEFT"),
            rect(0.0, -24.0, 140.0, 20.0),
            "Check for Update".to_string(),
            |_| {},
        );
        check.base.shown.set(false);
        m.host.add("checkUpdate", check);
        // With the application's own version (only pob-app sets it, the user's decision): the port's lines, 4 from the About button like the gap between the buttons.
        let label_x = if launch().cfg.app_version.is_some() { VERSION_LINES_X } else { 148.0 };
        let fork = LabelControl::new(
            anchor("BOTTOMLEFT", &am, "BOTTOMLEFT"),
            rect(label_x, -26.0, 0.0, 16.0),
            String::new(),
        );
        fork.label.set_fn(|_| {
            let l = launch();
            match &l.cfg.app_version {
                Some(app) => version_lines(app, &l.version_number.borrow()).0,
                None => "^8PoB Community Fork".to_string(),
            }
        });
        m.host.add("forkLabel", fork);
        let ver = LabelControl::new(anchor("BOTTOMLEFT", &am, "BOTTOMLEFT"), rect(label_x, -2.0, 0.0, 16.0), String::new());
        ver.label.set_fn(|_| {
            let l = launch();
            if let Some(app) = &l.cfg.app_version {
                return version_lines(app, &l.version_number.borrow()).1;
            }
            let branch = l.version_branch.borrow().clone();
            format!(
                "^8{}{}{}",
                if branch.as_deref() == Some("beta") { "Beta: " } else { "Version: " },
                l.version_number.borrow(),
                if branch.as_deref() == Some("dev") { " (Dev)" } else { "" }
            )
        });
        m.host.add("versionLabel", ver);
        let dev = LabelControl::new(
            anchor("BOTTOMLEFT", &am, "BOTTOMLEFT"),
            rect(0.0, -26.0, 0.0, 20.0),
            format!("{}Dev Mode", crate::colors::color_code("NEGATIVE").expect("NEGATIVE")),
        );
        dev.base.shown.set_fn(|_| launch().dev_mode.get());
        m.host.add("devMode", dev);
        m.main_bar_height.set(58.0);
        m.toasts.init(&am);
        m
    }

    /// The flags controls read from `main` (common.rs) follow the settings.
    fn sync_flags(&self) {
        let s = self.s.borrow();
        crate::common::MAIN.with(|f| {
            f.show_flavour_text.set(s.show_flavour_text);
            f.not_supported_mod_tooltips.set(s.not_supported_mod_tooltips);
            f.invert_slider_scroll_direction
                .set(s.invert_slider_scroll_direction.unwrap_or(false));
            f.disable_scroll_control_interaction.set(s.disable_scroll_control_interaction);
        });
    }

    /// `main:CanExit()` (Main.lua:365-372).
    pub fn can_exit(&self, g: &mut Gfx) -> bool {
        self.call_mode_can_exit(g, "EXIT").unwrap_or(true)
    }

    fn call_mode_can_exit(&self, g: &mut Gfx, reason: &str) -> Option<bool> {
        self.cur_mode()?.can_exit(g, reason)
    }

    /// `main:Shutdown()` (Main.lua:374-377).
    pub fn shutdown(&self, g: &mut Gfx) {
        if let Some(m) = self.cur_mode() {
            m.shutdown(g);
        }
        self.save_settings();
    }

    fn cur_mode(&self) -> Option<Rc<dyn Mode>> {
        let name = self.mode.borrow().clone()?;
        self.modes.get(name.as_str()).cloned()
    }

    /// What upstream has loaded by the time the program runs: the gem tooltips' files on their own thread, the item databases over the frames after the first one (`onFrameFuncs["LoadItems"]`).
    fn load_background_data(&self) {
        match self.startup_frames.get() {
            0 => {
                crate::gem_tooltip::start_preload(&launch().cfg.script_path);
                self.startup_frames.set(1);
            }
            1 if crate::tabs::items::load_item_dbs_step(Some(ITEM_DB_SLICE)) => self.startup_frames.set(2),
            _ => {}
        }
    }

    /// `not (main.uniqueDB.loading or main.rareDB.loading)`.
    pub fn item_dbs_ready(&self) -> bool {
        self.startup_frames.get() == 2
    }

    /// `main:OnFrame()` (Main.lua:379-493).
    pub fn on_frame(self: &Rc<Self>, f: &mut Frame<'_>) {
        let (screen_w, screen_h) = f.get_virtual_screen_size();
        set_screen_size(screen_w, screen_h);
        self.portrait_mode.set(screen_h > screen_w);
        loop {
            let Some(new_mode) = self.new_mode.borrow_mut().take() else { break };
            if let Some(m) = self.cur_mode() {
                m.shutdown(f);
            }
            *self.mode.borrow_mut() = Some(new_mode.clone());
            let args = self.new_mode_args.borrow().clone();
            let m = self.modes.get(new_mode.as_str()).cloned();
            if let Some(m) = m {
                m.init(f, &args);
            }
            // `newModeChangeToTree` (import from a link, U8) sets `viewMode = "TREE"` of the build mode.
            self.new_mode_change_to_tree.set(false);
        }
        let vp = Rect::new(0.0, 0.0, screen_w, screen_h);
        self.view_port.set(vp);
        let mut events = std::mem::take(&mut *self.input_events.borrow_mut());
        let top = self.popups.borrow().first().cloned();
        if let Some(p) = top {
            p.process_input(f, &mut events, &vp);
            events.clear();
        } else {
            self.host.process_controls_input(f, &mut events, &vp);
        }
        if let Some(m) = self.cur_mode() {
            m.on_frame(f, &mut events, &vp);
        }
        let l = launch();
        let err = l.update_err_msg.borrow_mut().take();
        if let Some(e) = err {
            self.toasts.add(&format!("Update check failed!\n{e}"));
        }
        let avail = l.update_available.borrow().clone();
        if let Some(a) = avail {
            if a == "none" {
                self.toasts.add("No update available\nYou are running the latest version.");
                *l.update_available.borrow_mut() = None;
            } else if !self.update_available_shown.get() {
                self.toasts
                    .add("Update Available\nAn update has been downloaded and is ready\nto be applied.");
                self.update_available_shown.set(true);
            }
        }
        self.load_background_data();
        self.toasts.update_frame(screen_h);
        let total_toast_height = self.toasts.render(f, &events, &vp);
        self.main_bar_height.set(58.0 + total_toast_height);
        sg::set_draw_color(f, 0.85, 0.85, 0.85);
        sg::draw_rect(f, 0.0, screen_h - 58.0, 312.0, 58.0);
        sg::set_draw_color(f, 0.1, 0.1, 0.1);
        sg::draw_rect(f, 0.0, screen_h - 54.0, 308.0, 54.0);
        self.host.draw_controls(f, &vp, None);
        let top = self.popups.borrow().first().cloned();
        if let Some(p) = top {
            sg::set_draw_layer(f, Some(10), 0);
            sg::set_draw_color_a(f, 0.0, 0.0, 0.0, 0.5);
            sg::draw_rect(f, 0.0, 0.0, screen_w, screen_h);
            p.draw_popup(f, &vp);
            sg::set_draw_layer(f, Some(0), 0);
        }
        let drag_text = crate::common::take_show_drag_text();
        if let Some(text) = drag_text {
            let (cursor_x, cursor_y) = f.get_cursor_pos();
            let str_width = sg::draw_string_width(f, 16.0, "VAR", &text);
            sg::set_draw_layer(f, Some(20), 0);
            sg::set_draw_color_a(f, 0.15, 0.15, 0.15, 0.75);
            sg::draw_rect(f, cursor_x, cursor_y - 8.0, str_width + 2.0, 18.0);
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            sg::draw_string(f, cursor_x + 1.0, cursor_y - 7.0, Some("LEFT"), 16.0, "VAR", &text);
        }
        if !self.wiki_triggered.get() {
            let f1 = ipairs(&events).any(|(_, e)| matches!(e, InputEvent::KeyUp { key } if key == "F1"));
            if f1 {
                let tab_name = format!(
                    "{} tab",
                    self.cur_mode()
                        .and_then(|m| m.view_mode())
                        .map_or("Build List".to_string(), |v| v.to_lowercase())
                );
                self.open_about_popup(f, Some(HelpSection::Name(tab_name)));
            }
        }
        self.wiki_triggered.set(false);
        events.clear();
        // `onFrameFuncs`: "FirstFrame" (Main.lua:283-289); "LoadItems" (item DBs) is U6.
        if !self.first_frame_done.get() {
            self.first_frame_done.set(true);
            let t = f.get_time() as f64 - launch().start_time.get() as f64;
            f.con_printf(&format!("Startup time: {} ms", t as i64));
        }
        // :489-491 `pairs(self.onFrameFuncs)`: "TradeQueryGenerator", "TradeQueryRequests" (U8d), in the order they were set
        let funcs: Vec<_> = self.on_frame_funcs.borrow().values().cloned().collect();
        for func in funcs {
            func(f);
        }
    }

    /// `main:OnKeyDown(key, doubleClick)` (Main.lua:495-497).
    pub fn on_key_down(&self, key: &str, double_click: bool) {
        self.input_events.borrow_mut().push(Some(InputEvent::KeyDown {
            key: key.to_string(),
            double_click,
        }));
    }

    pub fn on_key_up(&self, key: &str) {
        self.input_events
            .borrow_mut()
            .push(Some(InputEvent::KeyUp { key: key.to_string() }));
    }

    pub fn on_char(&self, key: &str) {
        self.input_events.borrow_mut().push(Some(InputEvent::Char { key: key.to_string() }));
    }

    /// `main:SetMode(newMode, ...)` (Main.lua:507-511).
    pub fn set_mode(&self, new_mode: &str, args: Vec<ModeArg>) {
        *self.new_mode.borrow_mut() = Some(new_mode.to_string());
        *self.new_mode_args.borrow_mut() = args;
    }

    /// `common.xml.LoadXMLFile(userPath.."Settings.xml")` + the error branches shared by LoadSettings / LoadSharedItems:
    /// `Ok(None)` = nothing to read (`return true`).
    fn read_settings_xml(&self, g: &mut Gfx) -> Option<XmlElement> {
        if self.error_reading_settings.get() {
            return None;
        }
        let path = format!("{}Settings.xml", self.user_path.borrow().clone().unwrap_or_default());
        let text = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                self.error_reading_settings.set(true);
                launch().show_err_msg(&format!("^1{path}: {e}"));
                return None;
            }
        };
        if text.is_empty() {
            // xml.lua: "file returns nil" (a cloud placeholder) → the cloud error popup.
            self.error_reading_settings.set(true);
            self.open_cloud_error_popup(g, Some(&path));
            return None;
        }
        let nodes = match parse_xml_document(&String::from_utf8_lossy(&text)) {
            Ok(n) => n,
            Err(e) => {
                self.error_reading_settings.set(true);
                launch().show_err_msg(&format!("^1{e}"));
                return None;
            }
        };
        let root = nodes.into_iter().find_map(|n| match n {
            XmlNode::Element(e) => Some(e),
            XmlNode::Text(_) => None,
        })?;
        if root.name != "PathOfBuilding" {
            launch().show_err_msg("^1Error parsing 'Settings.xml': 'PathOfBuilding' root element missing");
            return None;
        }
        Some(root)
    }

    /// `main:LoadSettings(ignoreBuild)` (Main.lua:520-681).
    pub fn load_settings(&self, g: &mut Gfx, ignore_build: bool) {
        let Some(root) = self.read_settings_xml(g) else { return };
        let l = launch();
        for node in root.child_elements() {
            if !ignore_build && node.name == "Mode" {
                let mode = node.attr("mode");
                if mode.is_none_or(|m| !self.modes.contains_key(m)) {
                    l.show_err_msg("^1Error parsing 'Settings.xml': Invalid mode attribute in 'Mode' element");
                    return;
                }
                let mut args = Vec::new();
                for child in node.child_elements() {
                    if child.name == "Arg" {
                        if let Some(n) = child.attr("number") {
                            // `t_insert(args, tonumber(...))`: a nil is not inserted.
                            if let Some(v) = pob_lua::tonumber(n) {
                                args.push(ModeArg::Num(v));
                            }
                        } else if let Some(s) = child.attr("string") {
                            args.push(ModeArg::Str(s.to_string()));
                        } else if let Some(b) = child.attr("boolean") {
                            args.push(ModeArg::Bool(b == "true"));
                        }
                    }
                }
                self.set_mode(mode.expect("mode"), args);
            } else if node.name == "Accounts" {
                let a = |k: &str| node.attr(k).map(str::to_string);
                *self.accounts.borrow_mut() = Accounts {
                    last_account_name: a("lastAccountName"),
                    last_realm: a("lastRealm"),
                    last_league: a("lastLeague"),
                    last_token: a("lastToken"),
                    last_refresh_token: a("lastRefreshToken"),
                    token_expiry: node.attr("tokenExpiry").and_then(pob_lua::tonumber),
                };
                for child in node.child_elements() {
                    if child.name == "Account"
                        && let Some(name) = child.attr("accountName")
                    {
                        self.game_accounts
                            .borrow_mut()
                            .insert(name.to_string(), child.attr("sessionID").map(str::to_string));
                    }
                }
            } else if node.name == "Misc" {
                let at = |k: &str| node.attr(k);
                let mut s = self.s.borrow_mut();
                if let Some(v) = at("buildSortMode") {
                    s.build_sort_mode = v.to_string();
                }
                l.connection_protocol.set(at("connectionProtocol").and_then(pob_lua::tonumber));
                *l.proxy_url.borrow_mut() = at("proxyURL").map(str::to_string);
                if let Some(v) = at("buildPath") {
                    *self.build_path.borrow_mut() = v.to_string();
                }
                if let Some(v) = at("nodePowerTheme") {
                    s.node_power_theme = v.to_string();
                }
                if let Some(v) = at("colorPositive") {
                    update_color_code("POSITIVE", v);
                    s.color_positive = v.to_string();
                }
                if let Some(v) = at("colorNegative") {
                    update_color_code("NEGATIVE", v);
                    s.color_negative = v.to_string();
                }
                if let Some(v) = at("colorHighlight") {
                    update_color_code("HIGHLIGHT", v);
                    s.color_highlight = v.to_string();
                }
                if let Some(v) = at("showThousandsCalcs") {
                    s.show_thousands_separators = v == "true";
                } else if let Some(v) = at("showThousandsSidebar") {
                    s.show_thousands_separators = v == "true";
                }
                let b = |k: &str, f: &mut bool| {
                    if let Some(v) = at(k) {
                        *f = v == "true";
                    }
                };
                b("showThousandsSeparators", &mut s.show_thousands_separators);
                if let Some(v) = at("thousandsSeparator") {
                    s.thousands_separator = v.to_string();
                }
                b("useCompactValues", &mut s.use_compact_values);
                if let Some(v) = at("decimalSeparator") {
                    s.decimal_separator = v.to_string();
                }
                b("showTitlebarName", &mut s.show_titlebar_name);
                if let Some(v) = at("betaTest") {
                    s.beta_test = Some(v == "true");
                }
                b("edgeSearchHighlight", &mut s.edge_search_highlight);
                if let Some(v) = at("defaultGemQuality") {
                    s.default_gem_quality = Some(pob_lua::tonumber(v).unwrap_or(0.0).min(23.0));
                }
                if let Some(v) = at("defaultCharLevel") {
                    s.default_char_level = Some(pob_lua::tonumber(v).unwrap_or(1.0).clamp(1.0, 100.0));
                }
                if let Some(v) = at("defaultItemAffixQuality") {
                    s.default_item_affix_quality = pob_lua::tonumber(v).unwrap_or(0.5).min(1.0);
                }
                if let Some(v) = at("lastExportedWebsite") {
                    s.last_exported_website = Some(v.to_string());
                }
                b("showWarnings", &mut s.show_warnings);
                b("slotOnlyTooltips", &mut s.slot_only_tooltips);
                b("migrateEldritchImplicits", &mut s.migrate_eldritch_implicits);
                b("notSupportedModTooltips", &mut s.not_supported_mod_tooltips);
                if let Some(v) = at("invertSliderScrollDirection") {
                    s.invert_slider_scroll_direction = Some(v == "true");
                }
                if let Some(v) = at("disableDevAutoSave") {
                    s.disable_dev_auto_save = Some(v == "true");
                }
                b("showPublicBuilds", &mut s.show_public_builds);
                b("showFlavourText", &mut s.show_flavour_text);
                b("showAnimations", &mut s.show_animations);
                b("showAllItemAffixes", &mut s.show_all_item_affixes);
                b("disableScrollControlInteraction", &mut s.disable_scroll_control_interaction);
                if let Some(v) = at("dpiScaleOverridePercent") {
                    s.dpi_scale_override_percent = pob_lua::tonumber(v).unwrap_or(0.0);
                    g.set_dpi_scale_override_percent(s.dpi_scale_override_percent as u32);
                }
            }
        }
        self.sync_flags();
    }

    /// `main:LoadSharedItems()` (Main.lua:683-736): items are kept as raw text (`new("Item")` is U6).
    pub fn load_shared_items(&self, g: &mut Gfx) {
        let Some(root) = self.read_settings_xml(g) else { return };
        for node in root.child_elements() {
            if node.name == "SharedItems" {
                for child in node.child_elements() {
                    if child.name == "Item" {
                        self.shared_item_list.borrow_mut().push(child.text());
                    } else if child.name == "ItemSet" {
                        let mut set = SharedItemSet {
                            title: child.attr("title").map(str::to_string),
                            slots: IndexMap::new(),
                        };
                        for gc in child.child_elements() {
                            if gc.name == "Item" {
                                set.slots.insert(gc.attr("slotName").unwrap_or("").to_string(), gc.text());
                            }
                        }
                        self.shared_item_set_list.borrow_mut().push(set);
                    }
                }
            }
        }
    }

    /// `main:SaveSettings()` (Main.lua:738-817). Attributes are written in source order (Lua: `pairs` order).
    pub fn save_settings(&self) {
        if self.error_reading_settings.get() {
            return;
        }
        let l = launch();
        let el = |name: &str, attrib: Vec<(&str, Option<String>)>, children: Vec<XmlNode>| XmlElement {
            name: name.to_string(),
            attrib: attrib.into_iter().filter_map(|(k, v)| v.map(|v| (k.to_string(), v))).collect(),
            children,
            empty: false,
        };
        let mut mode_children = Vec::new();
        if let Some(m) = self.cur_mode() {
            for val in m.get_args() {
                let attrib = match val {
                    ModeArg::Num(n) => vec![("number", Some(tostring_num(n)))],
                    ModeArg::Bool(b) => vec![("boolean", Some(b.to_string()))],
                    ModeArg::Str(s) => vec![("string", Some(s))],
                };
                mode_children.push(XmlNode::Element(el("Arg", attrib, Vec::new())));
            }
        }
        let mode_name = self.mode.borrow().clone();
        if mode_name.is_none() || mode_children.is_empty() {
            l.show_err_msg("^1Error saving 'Settings.xml': mode element is invalid");
            return;
        }
        let mode = el("Mode", vec![("mode", mode_name)], mode_children);
        let a = self.accounts.borrow().clone();
        let mut acc_children = Vec::new();
        for (name, sid) in self.game_accounts.borrow().iter() {
            acc_children.push(XmlNode::Element(el(
                "Account",
                vec![("accountName", Some(name.clone())), ("sessionID", sid.clone())],
                Vec::new(),
            )));
        }
        let accounts = el(
            "Accounts",
            vec![
                ("lastAccountName", a.last_account_name),
                ("lastRealm", a.last_realm),
                ("lastLeague", a.last_league),
                ("lastToken", a.last_token),
                ("lastRefreshToken", a.last_refresh_token),
                ("tokenExpiry", Some(a.token_expiry.map_or("nil".into(), tostring_num))),
            ],
            acc_children,
        );
        let mut shared = Vec::new();
        for raw in self.shared_item_list.borrow().iter() {
            shared.push(XmlNode::Element(el("Item", Vec::new(), vec![XmlNode::Text(raw.clone())])));
        }
        for set in self.shared_item_set_list.borrow().iter() {
            let mut children = Vec::new();
            for (slot, raw) in &set.slots {
                children.push(XmlNode::Element(el(
                    "Item",
                    vec![("slotName", Some(slot.clone()))],
                    vec![XmlNode::Text(raw.clone())],
                )));
            }
            shared.push(XmlNode::Element(el("ItemSet", vec![("title", set.title.clone())], children)));
        }
        let shared = el("SharedItems", Vec::new(), shared);
        let s = self.s.borrow().clone();
        let bp = self.build_path.borrow().clone();
        let dbp = self.default_build_path.borrow().clone();
        let t = |b: bool| Some(b.to_string());
        let misc = el(
            "Misc",
            vec![
                ("buildSortMode", Some(s.build_sort_mode)),
                (
                    "connectionProtocol",
                    Some(l.connection_protocol.get().map_or("nil".into(), tostring_num)),
                ),
                ("proxyURL", l.proxy_url.borrow().clone()),
                ("buildPath", (bp != dbp).then_some(bp)),
                ("nodePowerTheme", Some(s.node_power_theme)),
                ("colorPositive", Some(s.color_positive)),
                ("colorNegative", Some(s.color_negative)),
                ("colorHighlight", Some(s.color_highlight)),
                ("showThousandsSeparators", t(s.show_thousands_separators)),
                ("thousandsSeparator", Some(s.thousands_separator)),
                ("useCompactValues", t(s.use_compact_values)),
                ("decimalSeparator", Some(s.decimal_separator)),
                ("showTitlebarName", t(s.show_titlebar_name)),
                ("betaTest", Some(tostring_opt_bool(s.beta_test))),
                ("edgeSearchHighlight", t(s.edge_search_highlight)),
                ("defaultGemQuality", Some(tostring_num(s.default_gem_quality.unwrap_or(0.0)))),
                ("defaultCharLevel", Some(tostring_num(s.default_char_level.unwrap_or(1.0)))),
                ("defaultItemAffixQuality", Some(tostring_num(s.default_item_affix_quality))),
                ("lastExportedWebsite", s.last_exported_website),
                ("showWarnings", t(s.show_warnings)),
                ("slotOnlyTooltips", t(s.slot_only_tooltips)),
                ("migrateEldritchImplicits", t(s.migrate_eldritch_implicits)),
                ("notSupportedModTooltips", t(s.not_supported_mod_tooltips)),
                (
                    "invertSliderScrollDirection",
                    Some(tostring_opt_bool(s.invert_slider_scroll_direction)),
                ),
                ("disableDevAutoSave", Some(tostring_opt_bool(s.disable_dev_auto_save))),
                ("showPublicBuilds", t(s.show_public_builds)),
                ("showFlavourText", t(s.show_flavour_text)),
                ("showAnimations", t(s.show_animations)),
                ("showAllItemAffixes", t(s.show_all_item_affixes)),
                ("disableScrollControlInteraction", t(s.disable_scroll_control_interaction)),
                ("dpiScaleOverridePercent", Some(tostring_num(s.dpi_scale_override_percent))),
            ],
            Vec::new(),
        );
        let root = el(
            "PathOfBuilding",
            Vec::new(),
            vec![
                XmlNode::Element(mode),
                XmlNode::Element(accounts),
                XmlNode::Element(shared),
                XmlNode::Element(misc),
            ],
        );
        let path = format!("{}Settings.xml", self.user_path.borrow().clone().unwrap_or_default());
        if let Err(e) = std::fs::write(&path, compose_xml(&root)) {
            l.show_err_msg(&format!("Error saving 'Settings.xml': {path}: {e}"));
        }
    }

    /// `main:OpenPathPopup(invalidPath, errMsg, ignoreBuild)` (Main.lua:819-852).
    pub fn open_path_popup(&self, g: &mut Gfx, invalid_path: &str, err_msg: &str, ignore_build: bool) {
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        let invalid = Rc::new(RefCell::new(invalid_path.to_string()));
        let err = err_msg.to_string();
        let label = LabelControl::new(anchor_nil("TOPLEFT", "TOPLEFT"), rect(8.0, 20.0, 206.0, 16.0), String::new());
        let inv = invalid.clone();
        label.label.set_fn(move |_| {
            format!(
                "^7User settings path cannot be loaded: {err}\nCurrent Path: {}/Path of Building/\nIf this location is managed by OneDrive, navigate to that folder and manually try\nto open Settings.xml in a text editor before re-opening Path of Building\nOtherwise, specify a new location for your Settings.xml:",
                lua::gsub(&inv.borrow(), "?", "^1?^7")
            )
        });
        let label: ControlRef = label;
        controls.insert("label".into(), label.clone());
        let save_cell: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
        let (sc, inv) = (save_cell.clone(), invalid.clone());
        let user_path = EditControl::new(
            g,
            anchor("TOPLEFT", &label, "TOPLEFT"),
            rect(0.0, 60.0, 206.0, 20.0),
            EditArgs {
                init: Some(invalid_path.to_string()),
                change_func: Some(Rc::new(move |_g, buf, _| {
                    *inv.borrow_mut() = crate::common::sanitise_text(buf);
                    let enabled = !inv.borrow().contains('?');
                    if let Some(s) = sc.borrow().as_ref() {
                        s.base.enabled.set(enabled);
                    }
                })),
                ..Default::default()
            },
        );
        let up: ControlRef = user_path.clone();
        controls.insert("userPath".into(), up.clone());
        let save = ButtonControl::new(
            anchor("TOPLEFT", &up, "TOPLEFT"),
            rect(0.0, 26.0, 206.0, 20.0),
            "Save".to_string(),
            move |g| {
                let buf = user_path.buf.borrow().clone();
                let m = main();
                match pob_gfx::fs::make_dir(&buf) {
                    Err(msg) if msg != "No error" => {
                        m.open_message_popup(g, "Error", &format!("Couldn't create '{buf}' : {msg}"));
                    }
                    _ => {
                        m.change_user_path(g, &buf, ignore_build);
                        m.close_popup();
                    }
                }
            },
        );
        save.base.enabled.set(false);
        *save_cell.borrow_mut() = Some(save.clone());
        controls.insert("save".into(), save);
        controls.insert(
            "cancel".into(),
            ButtonControl::new(None, rect(0.0, 0.0, 0.0, 0.0), "Cancel".to_string(), |_| {}),
        );
        self.open_popup(
            g,
            600.0,
            150.0,
            "Change Settings Path",
            controls,
            Some("save"),
            None,
            Some("cancel"),
            None,
        );
    }

    /// `main:ChangeUserPath(newUserPath, ignoreBuild)` (Main.lua:854-862).
    pub fn change_user_path(&self, g: &mut Gfx, new_user_path: &str, ignore_build: bool) {
        *self.user_path.borrow_mut() = Some(new_user_path.to_string());
        let _ = pob_gfx::fs::make_dir(new_user_path);
        let dbp = format!("{new_user_path}Builds/");
        *self.default_build_path.borrow_mut() = dbp.clone();
        *self.build_path.borrow_mut() = dbp.clone();
        let _ = pob_gfx::fs::make_dir(&dbp);
        self.load_settings(g, ignore_build);
        self.load_shared_items(g);
    }

    /// `main:OpenOptionsPopup(savedState)` (Main.lua:865-1315).
    pub fn open_options_popup(&self, g: &mut Gfx, saved_state: Option<Settings>) {
        let saved = saved_state.unwrap_or_else(|| self.s.borrow().clone());
        let saved = Rc::new(saved);
        let (screen_w, screen_h) = screen_size();
        let one_column_height_req = 850.0;
        let column_width = 600.0;
        let starting_y = 20.0;
        let current_y = Rc::new(Cell::new(starting_y));
        let mut current_x = 0.0;
        let use_two_columns = screen_h < one_column_height_req && screen_w >= column_width * 2.0;
        let use_scroll_bar = screen_h < one_column_height_req && !use_two_columns;
        let scroll_bar_width = if use_scroll_bar { 18.0 } else { 0.0 };
        let popup_width = if use_two_columns { column_width * 2.0 } else { column_width };
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        let section_anchor = BareControl::new(anchor_nil("TOPLEFT", "TOPLEFT"), rect(0.0, 0.0, popup_width, 0.0));
        let sa: ControlRef = section_anchor.clone();
        controls.insert("sectionAnchor".into(), sa.clone());
        let next_row = |h: f64| current_y.set(current_y.get() + h * 26.0);
        let section_header = |controls: &mut IndexMap<String, ControlRef>, id: &str, title: &str, current_x: f64| {
            controls.insert(
                format!("section-{id}-bg"),
                RectangleOutlineControl::new(
                    anchor("TOPLEFT", &sa, "TOPLEFT"),
                    rect(
                        current_x + scroll_bar_width + 8.0,
                        current_y.get(),
                        column_width - scroll_bar_width * 2.0 - 17.0,
                        26.0,
                    ),
                    Some([0.6, 0.6, 0.6]),
                    Some(1.0),
                ),
            );
            next_row(0.2);
            controls.insert(
                format!("section-{id}-label"),
                LabelControl::new(
                    anchor("TOPLEFT", &sa, "TOPLEFT"),
                    rect(current_x + column_width / 2.0 - 60.0, current_y.get(), 0.0, 16.0),
                    format!("^7{title}"),
                ),
            );
            next_row(1.5);
        };
        let spacing = -4.0;
        let label_x = column_width * 0.45;
        let tl = |c: &ControlRef| anchor("TOPLEFT", c, "TOPLEFT");
        let label_for = |controls: &mut IndexMap<String, ControlRef>, name: &str, of: &ControlRef, w: f64, text: &str| {
            controls.insert(
                name.into(),
                LabelControl::new(anchor("RIGHT", of, "LEFT"), rect(spacing, 0.0, w, 16.0), text.to_string()),
            );
        };
        let check = |g: &Gfx,
                     controls: &mut IndexMap<String, ControlRef>,
                     name: &str,
                     x: f64,
                     y: f64,
                     text: &str,
                     f: Rc<dyn Fn(&mut Gfx, bool)>,
                     tip: Option<&str>| {
            let c = CheckBoxControl::new(g, tl(&sa), [x, y, 20.0], Some(text), Some(f), tip, false);
            controls.insert(name.into(), c.clone());
            c
        };
        macro_rules! setter {
            ($field:ident) => {
                Rc::new(|_g: &mut Gfx, state: bool| main().s.borrow_mut().$field = state) as Rc<dyn Fn(&mut Gfx, bool)>
            };
            (opt $field:ident) => {
                Rc::new(|_g: &mut Gfx, state: bool| main().s.borrow_mut().$field = Some(state)) as Rc<dyn Fn(&mut Gfx, bool)>
            };
        }
        let s = self.s.borrow().clone();
        let l = launch();

        section_header(&mut controls, "app", "Application options", current_x);
        let cp = DropDownControl::new(
            g,
            tl(&sa),
            rect(current_x + label_x, current_y.get(), 100.0, 18.0),
            vec![item("Auto", 0.0f64), item("IPv4", 1.0f64), item("IPv6", 2.0f64)],
            Some(Rc::new(|_g: &mut Gfx, _i, v: &DropItem| {
                main().s.borrow_mut().connection_protocol = data::<f64>(v)
            })),
            None,
            false,
        );
        let cpr: ControlRef = cp.clone();
        controls.insert("connectionProtocol".into(), cpr.clone());
        label_for(&mut controls, "connectionProtocolLabel", &cpr, 0.0, "^7Connection Protocol:");
        cp.tip.tooltip_text.set(Some(
            "Changes which protocol is used when downloading updates and importing builds.".to_string(),
        ));
        if let Some(p) = l.connection_protocol.get() {
            sel_by_value(&cp, &p);
        }

        next_row(1.0);
        let proxy_type = DropDownControl::new(
            g,
            tl(&sa),
            rect(current_x + label_x, current_y.get(), 80.0, 18.0),
            vec![
                item("HTTP", "http".to_string()),
                item("SOCKS", "socks5".to_string()),
                item("SOCKS5H", "socks5h".to_string()),
            ],
            None,
            None,
            false,
        );
        let ptr: ControlRef = proxy_type.clone();
        controls.insert("proxyType".into(), ptr.clone());
        label_for(&mut controls, "proxyLabel", &ptr, 0.0, "^7Proxy server:");
        let proxy_url = EditControl::new(g, anchor("LEFT", &ptr, "RIGHT"), rect(4.0, 0.0, 206.0, 18.0), EditArgs::default());
        controls.insert("proxyURL".into(), proxy_url.clone());
        let purl = l.proxy_url.borrow().clone();
        if let Some(purl) = purl
            && let Some(caps) = lua::lmatch_all(&purl, "(%w+)://(.+)")
        {
            sel_by_value(&proxy_type, &caps[0]);
            proxy_url.set_text(g, &caps[1], false);
        }

        next_row(1.0);
        let saved2 = saved.clone();
        let dpi = DropDownControl::new(
            g,
            tl(&sa),
            rect(current_x + label_x, current_y.get(), 150.0, 18.0),
            vec![
                item("Use system default", 0.0f64),
                item("100%", 100.0f64),
                item("125%", 125.0f64),
                item("150%", 150.0f64),
                item("175%", 175.0f64),
                item("200%", 200.0f64),
                item("225%", 225.0f64),
                item("250%", 250.0f64),
            ],
            Some(Rc::new(move |g: &mut Gfx, _i, v: &DropItem| {
                let m = main();
                let p = data::<f64>(v);
                m.s.borrow_mut().dpi_scale_override_percent = p;
                g.set_dpi_scale_override_percent(p as u32);
                let (w, h) = g.get_virtual_screen_size();
                set_screen_size(w, h);
                m.close_popup();
                m.open_options_popup(g, Some((*saved2).clone()));
            })),
            None,
            false,
        );
        let dpir: ControlRef = dpi.clone();
        controls.insert("dpiScaleOverride".into(), dpir.clone());
        label_for(&mut controls, "dpiScaleOverrideLabel", &dpir, 0.0, "^7UI scaling override:");
        dpi.tip.tooltip_text.set(Some(
            "Overrides Windows DPI scaling inside Path of Building.\nChoose a percentage between 100% and 250% or revert to the system default."
                .to_string(),
        ));
        sel_by_value(&dpi, &s.dpi_scale_override_percent);

        next_row(1.0);
        let build_path = EditControl::new(
            g,
            tl(&sa),
            rect(current_x + label_x, current_y.get(), 290.0, 18.0),
            EditArgs::default(),
        );
        let bpr: ControlRef = build_path.clone();
        controls.insert("buildPath".into(), bpr.clone());
        label_for(&mut controls, "buildPathLabel", &bpr, 0.0, "^7Build save path:");
        let (bp, dbp) = (self.build_path.borrow().clone(), self.default_build_path.borrow().clone());
        if bp != dbp {
            build_path.set_text(g, &bp, false);
        }
        build_path.tip.tooltip_text.set(Some(format!(
            "Overrides the default save location for builds.\nThe default location is: '{dbp}'"
        )));

        next_row(1.0);
        let npt = DropDownControl::new(
            g,
            tl(&sa),
            rect(current_x + label_x, current_y.get(), 100.0, 18.0),
            vec![
                item("Red & Blue", "RED/BLUE".to_string()),
                item("Red & Green", "RED/GREEN".to_string()),
                item("Green & Blue", "GREEN/BLUE".to_string()),
            ],
            Some(Rc::new(|_g: &mut Gfx, _i, v: &DropItem| {
                main().s.borrow_mut().node_power_theme = data::<String>(v)
            })),
            None,
            false,
        );
        let nptr: ControlRef = npt.clone();
        controls.insert("nodePowerTheme".into(), nptr.clone());
        label_for(&mut controls, "nodePowerThemeLabel", &nptr, 0.0, "^7Node Power colours:");
        npt.tip.tooltip_text.set(Some(
            "Changes the colour scheme used for the node power display on the passive tree.".to_string(),
        ));
        sel_by_value(&npt, &s.node_power_theme);

        for (code, name, init, what, tail) in [
            (
                "POSITIVE",
                "colorPositive",
                s.color_positive.clone(),
                "positive values:",
                "Overrides the default hex colour for positive values in breakdowns.",
            ),
            (
                "NEGATIVE",
                "colorNegative",
                s.color_negative.clone(),
                "negative values:",
                "Overrides the default hex colour for negative values in breakdowns.",
            ),
            (
                "HIGHLIGHT",
                "colorHighlight",
                s.color_highlight.clone(),
                "highlight nodes:",
                "Overrides the default hex colour for highlighting nodes in passive tree search.",
            ),
        ] {
            next_row(1.0);
            let e = EditControl::new(
                g,
                tl(&sa),
                rect(current_x + label_x, current_y.get(), 100.0, 18.0),
                EditArgs {
                    init: Some(color_to_hex(&init)),
                    limit: Some(8),
                    change_func: Some(Rc::new(move |_g, buf: &str, _| {
                        if let Some(m) = lua::lmatch(buf, "0x%x+")
                            && m.len() == 8
                        {
                            update_color_code(code, buf);
                            let mm = main();
                            let mut s = mm.s.borrow_mut();
                            match code {
                                "POSITIVE" => s.color_positive = buf.to_string(),
                                "NEGATIVE" => s.color_negative = buf.to_string(),
                                _ => s.color_highlight = buf.to_string(),
                            }
                        }
                    })),
                    ..Default::default()
                },
            );
            let er: ControlRef = e.clone();
            controls.insert(name.into(), er.clone());
            label_for(
                &mut controls,
                &format!("{name}Label"),
                &er,
                0.0,
                &format!("^7Hex colour for {what}"),
            );
            let dflt = color_to_hex(&default_color_code(code).expect("code"));
            e.tip.tooltip_text.set(Some(if code == "HIGHLIGHT" {
                format!("{tail} \nExpected format is 0x000000. The default value is {dflt}\nIf updating while inside a build, please re-load the build after saving.")
            } else {
                format!("{tail} \nExpected format is 0x000000. The default value is {dflt}.\nIf updating while inside a build, please re-load the build after saving.")
            }));
        }

        next_row(1.0);
        let beta = check(
            g,
            &mut controls,
            "betaTest",
            current_x + label_x,
            current_y.get(),
            "^7Opt-in to weekly beta test builds:",
            setter!(opt beta_test),
            None,
        );
        next_row(1.0);
        let edge = check(
            g,
            &mut controls,
            "edgeSearchHighlight",
            current_x + label_x,
            current_y.get(),
            "^7Show search circles at viewport edge",
            setter!(edge_search_highlight),
            None,
        );
        next_row(1.0);
        let public = check(
            g,
            &mut controls,
            "showPublicBuilds",
            current_x + label_x,
            current_y.get(),
            "^7Show Latest/Trending builds:",
            setter!(show_public_builds),
            None,
        );
        next_row(1.0);
        let flavour = check(
            g,
            &mut controls,
            "showFlavourText",
            current_x + label_x,
            current_y.get(),
            "^7Styled Tooltips with Flavour Text:",
            setter!(show_flavour_text),
            None,
        );
        flavour.tip.tooltip_text.set(Some(
            "If updating while inside a build, please re-load the build after saving.".to_string(),
        ));
        next_row(1.0);
        let anim = check(
            g,
            &mut controls,
            "showAnimations",
            current_x + label_x,
            current_y.get(),
            "^7Show Animations:",
            setter!(show_animations),
            None,
        );
        next_row(1.0);
        let affixes = check(
            g,
            &mut controls,
            "showAllItemAffixes",
            current_x + label_x,
            current_y.get(),
            "^7Show all item affixes sliders:",
            setter!(show_all_item_affixes),
            None,
        );
        affixes.tip.tooltip_text.set(Some(
            "Display all item affix slots as a stacked list instead of hiding them in dropdowns.".to_string(),
        ));
        next_row(1.0);
        let scroll_int = check(
            g,
            &mut controls,
            "disableScrollControlInteraction",
            current_x + label_x,
            current_y.get(),
            "^7Disable control scroll interaction:",
            setter!(disable_scroll_control_interaction),
            None,
        );
        scroll_int.tip.tooltip_text.set(Some(
            "Disable changing the values in controls such as dropdowns or numeric inputs when using the scroll wheel.".to_string(),
        ));
        next_row(1.0);
        let left_column_max_y = current_y.get();
        if use_two_columns {
            current_y.set(starting_y);
            current_x = column_width;
        }
        section_header(&mut controls, "build", "Build-related options", current_x);
        let thousands = check(
            g,
            &mut controls,
            "showThousandsSeparators",
            current_x + label_x,
            current_y.get(),
            "^7Show thousands separators:",
            setter!(show_thousands_separators),
            None,
        );
        thousands.state.set(s.show_thousands_separators);
        next_row(1.0);
        let compact = check(
            g,
            &mut controls,
            "useCompactValues",
            current_x + label_x,
            current_y.get(),
            "^7Compact large numbers (e.g. 12.3K):",
            setter!(use_compact_values),
            None,
        );
        compact.state.set(s.use_compact_values);
        next_row(1.0);
        let ts = EditControl::new(
            g,
            tl(&sa),
            rect(current_x + label_x, current_y.get(), 30.0, 20.0),
            EditArgs {
                init: Some(s.thousands_separator.clone()),
                filter: Some("%w".into()),
                limit: Some(1),
                change_func: Some(Rc::new(|_g, buf: &str, _| {
                    main().s.borrow_mut().thousands_separator = buf.to_string()
                })),
                ..Default::default()
            },
        );
        let tsr: ControlRef = ts;
        controls.insert("thousandsSeparator".into(), tsr.clone());
        label_for(&mut controls, "thousandsSeparatorLabel", &tsr, 92.0, "^7Thousands separator:");
        next_row(1.0);
        let ds = EditControl::new(
            g,
            tl(&sa),
            rect(current_x + label_x, current_y.get(), 30.0, 20.0),
            EditArgs {
                init: Some(s.decimal_separator.clone()),
                filter: Some("%w".into()),
                limit: Some(1),
                change_func: Some(Rc::new(|_g, buf: &str, _| {
                    main().s.borrow_mut().decimal_separator = buf.to_string()
                })),
                ..Default::default()
            },
        );
        let dsr: ControlRef = ds;
        controls.insert("decimalSeparator".into(), dsr.clone());
        label_for(&mut controls, "decimalSeparatorLabel", &dsr, 92.0, "^7Decimal separator:");
        next_row(1.0);
        let titlebar = check(
            g,
            &mut controls,
            "titlebarName",
            current_x + label_x,
            current_y.get(),
            "^7Show build name in window title:",
            setter!(show_titlebar_name),
            None,
        );
        next_row(1.0);
        let gq = EditControl::new(
            g,
            tl(&sa),
            rect(current_x + label_x, current_y.get(), 80.0, 20.0),
            EditArgs {
                init: s.default_gem_quality.map(tostring_num),
                filter: Some("%D".into()),
                limit: Some(2),
                change_func: Some(Rc::new(|_g, buf: &str, _| {
                    main().s.borrow_mut().default_gem_quality = Some(pob_lua::tonumber(buf).unwrap_or(0.0).min(23.0));
                })),
                ..Default::default()
            },
        );
        gq.tip.tooltip_text.set(Some(
            "Set the default quality that can be overwritten by build-related quality settings in the skill panel.".to_string(),
        ));
        let gqr: ControlRef = gq;
        controls.insert("defaultGemQuality".into(), gqr.clone());
        label_for(&mut controls, "defaultGemQualityLabel", &gqr, 0.0, "^7Default gem quality:");
        next_row(1.0);
        let cl = EditControl::new(
            g,
            tl(&sa),
            rect(current_x + label_x, current_y.get(), 80.0, 20.0),
            EditArgs {
                init: s.default_char_level.map(tostring_num),
                filter: Some("%D".into()),
                limit: Some(3),
                change_func: Some(Rc::new(|_g, buf: &str, _| {
                    main().s.borrow_mut().default_char_level = Some(pob_lua::tonumber(buf).unwrap_or(1.0).clamp(1.0, 100.0));
                })),
                ..Default::default()
            },
        );
        cl.tip.tooltip_text.set(Some(
            "Set the default level of your builds. If this is higher than 1, manual level mode will be enabled by default in new builds."
                .to_string(),
        ));
        let clr: ControlRef = cl;
        controls.insert("defaultCharLevel".into(), clr.clone());
        label_for(&mut controls, "defaultCharLevelLabel", &clr, 0.0, "^7Default character level:");
        next_row(1.0);
        let value_label: Rc<RefCell<Option<Rc<LabelControl>>>> = Rc::new(RefCell::new(None));
        let vl = value_label.clone();
        let slider = SliderControl::new(
            tl(&sa),
            rect(current_x + label_x, current_y.get(), 200.0, 20.0),
            Some(Rc::new(move |_g: &mut Gfx, value: f64| {
                let q = (value * 100.0 + 0.5).floor() / 100.0;
                main().s.borrow_mut().default_item_affix_quality = q;
                if let Some(l) = vl.borrow().as_ref() {
                    l.label.set(format!("{}%", tostring_num(q * 100.0)));
                }
            })),
            None,
        );
        let slr: ControlRef = slider.clone();
        controls.insert("defaultItemAffixQualitySlider".into(), slr.clone());
        label_for(
            &mut controls,
            "defaultItemAffixQualityLabel",
            &slr,
            92.0,
            "^7Default item affix quality:",
        );
        let value = LabelControl::new(anchor("LEFT", &slr, "RIGHT"), rect(-spacing, 0.0, 92.0, 16.0), "50%".to_string());
        controls.insert("defaultItemAffixQualityValue".into(), value.clone());
        *value_label.borrow_mut() = Some(value.clone());
        slider.val.set(s.default_item_affix_quality);
        value.label.set(format!("{}%", tostring_num(s.default_item_affix_quality * 100.0)));
        next_row(1.0);
        let warn = check(
            g,
            &mut controls,
            "showWarnings",
            current_x + label_x,
            current_y.get(),
            "^7Show build warnings:",
            setter!(show_warnings),
            None,
        );
        warn.state.set(s.show_warnings);
        next_row(1.0);
        let slot = check(
            g,
            &mut controls,
            "slotOnlyTooltips",
            current_x + label_x,
            current_y.get(),
            "^7Show tooltips only for affected slots:",
            setter!(slot_only_tooltips),
            Some("Shows comparisons in tooltips only for the slot you are currently placing the item in, instead of all slots."),
        );
        slot.state.set(s.slot_only_tooltips);
        next_row(1.0);
        let eld = check(
            g,
            &mut controls,
            "migrateEldritchImplicits",
            current_x + label_x,
            current_y.get(),
            "^7Copy Eldritch Implicits onto Display Item:",
            setter!(migrate_eldritch_implicits),
            None,
        );
        eld.tip.tooltip_text.set(Some(
            "Apply Eldritch Implicits from current gear when comparing new gear, given the new item doesn't have any influence".to_string(),
        ));
        eld.state.set(s.migrate_eldritch_implicits);
        next_row(1.0);
        let nsm = check(
            g,
            &mut controls,
            "notSupportedModTooltips",
            current_x + label_x,
            current_y.get(),
            "^7Show tooltip for unsupported mods :",
            setter!(not_supported_mod_tooltips),
            None,
        );
        nsm.tip
            .tooltip_text
            .set(Some("Show ^8(Not supported in PoB yet) ^7next to unsupported mods".to_string()));
        nsm.state.set(s.not_supported_mod_tooltips);
        next_row(1.0);
        let inv = check(
            g,
            &mut controls,
            "invertSliderScrollDirection",
            current_x + label_x,
            current_y.get(),
            "^7Invert slider scroll direction:",
            setter!(opt invert_slider_scroll_direction),
            None,
        );
        inv.tip.tooltip_text.set(Some(
            "Default scroll direction is:\nScroll Up = Move right\nScroll Down = Move left".to_string(),
        ));
        inv.state.set(s.invert_slider_scroll_direction.unwrap_or(false));
        // `if launch.devMode then … disableDevAutoSave …`: devMode is never set in the port.
        beta.state.set(s.beta_test.unwrap_or(false));
        edge.state.set(s.edge_search_highlight);
        titlebar.state.set(s.show_titlebar_name);
        public.state.set(s.show_public_builds);
        flavour.state.set(s.show_flavour_text);
        anim.state.set(s.show_animations);
        affixes.state.set(s.show_all_item_affixes);
        scroll_int.state.set(s.disable_scroll_control_interaction);
        current_y.set(left_column_max_y.max(current_y.get()));
        next_row(1.5);

        let (cpd, ptd, pud, bpd) = (cp.clone(), proxy_type.clone(), proxy_url.clone(), build_path.clone());
        let _ = cpd;
        controls.insert(
            "save".into(),
            ButtonControl::new(
                anchor_nil("BOTTOM", "BOTTOM"),
                rect(-45.0, -10.0, 80.0, 20.0),
                "Save".to_string(),
                move |g| {
                    let m = main();
                    let l = launch();
                    l.connection_protocol.set(Some(m.s.borrow().connection_protocol));
                    let purl = pud.buf.borrow().clone();
                    if lua::is_match(&purl, "%w") {
                        let scheme = ptd.get_sel_value().map(|v| data::<String>(&v)).unwrap_or_default();
                        *l.proxy_url.borrow_mut() = Some(format!("{scheme}://{purl}"));
                    } else {
                        *l.proxy_url.borrow_mut() = None;
                    }
                    let bp = bpd.buf.borrow().clone();
                    if lua::is_match(&bp, "%S") {
                        let mut bp = bp;
                        if !lua::is_match(&bp, "[\\/]$") {
                            bp.push('/');
                        }
                        *m.build_path.borrow_mut() = bp;
                    } else {
                        let d = m.default_build_path.borrow().clone();
                        *m.build_path.borrow_mut() = d;
                    }
                    if m.mode.borrow().as_deref() == Some("LIST") {
                        m.list_mode.build_list(g);
                    }
                    // `main:SetManifestBranch(...)` rewrites the install's manifest.xml: no auto-update in the port.
                    let p = m.s.borrow().dpi_scale_override_percent;
                    g.set_dpi_scale_override_percent(p as u32);
                    m.close_popup();
                    m.sync_flags();
                    m.save_settings();
                },
            ),
        );
        let saved3 = saved.clone();
        controls.insert(
            "cancel".into(),
            ButtonControl::new(
                anchor_nil("BOTTOM", "BOTTOM"),
                rect(45.0, -10.0, 80.0, 20.0),
                "Cancel".to_string(),
                move |g| {
                    let m = main();
                    let mut sv = (*saved3).clone();
                    // Fields the Lua cancel does not restore keep their current values.
                    {
                        let cur = m.s.borrow();
                        sv.build_sort_mode = cur.build_sort_mode.clone();
                        sv.connection_protocol = cur.connection_protocol;
                        sv.last_exported_website = cur.last_exported_website.clone();
                    }
                    update_color_code("POSITIVE", &sv.color_positive);
                    update_color_code("NEGATIVE", &sv.color_negative);
                    update_color_code("HIGHLIGHT", &sv.color_highlight);
                    g.set_dpi_scale_override_percent(sv.dpi_scale_override_percent as u32);
                    *m.s.borrow_mut() = sv;
                    m.sync_flags();
                    m.close_popup();
                },
            ),
        );
        let popup_height = if use_scroll_bar { screen_h - 20.0 } else { current_y.get() + 30.0 };
        let mut scroll_bar = None;
        if use_scroll_bar {
            let sb = ScrollBarControl::new(
                anchor_nil("TOPRIGHT", "TOPRIGHT"),
                rect(-2.0, 25.0, scroll_bar_width, popup_height - 65.0),
                Some(50.0),
                Some("VERTICAL"),
                true,
            );
            sb.set_content_dimension(g, current_y.get(), popup_height - 65.0);
            controls.insert("scrollBar".into(), sb.clone());
            scroll_bar = Some(sb);
        }
        let scroll_func: Option<crate::controls::popup::PopupFunc> = scroll_bar.clone().map(|sb| {
            let sa = section_anchor.clone();
            Rc::new(move |_g: &Gfx| sa.base.y.set(-sb.offset.get())) as crate::controls::popup::PopupFunc
        });
        let popup = self.open_popup(
            g,
            popup_width,
            popup_height,
            "Options",
            controls,
            Some("save"),
            None,
            Some("cancel"),
            scroll_func,
        );
        if use_scroll_bar {
            // Main.lua:1279-1314: draw / hover only the controls inside the scroll clip.
            let inside = |p: &PopupDialog, g: &Gfx, c: &ControlRef| -> bool {
                for n in ["scrollBar", "save", "cancel"] {
                    if p.host.get(n).is_some_and(|x| crate::control::same(&*x, &**c)) {
                        return true;
                    }
                }
                let (_, y) = p.get_pos(g);
                let (_, height) = p.get_size(g);
                let (_, cy) = c.get_pos(g);
                let (_, ch) = c.get_size(g);
                cy >= y + 20.0 && cy + ch <= y + height - 40.0
            };
            *popup.draw_controls_override.borrow_mut() = Some(Rc::new(move |p: &PopupDialog, f: &mut Frame<'_>, vp: &Rect| {
                let list: Vec<ControlRef> = p.host.controls.borrow().values().cloned().collect();
                for control in list {
                    if control.is_shown(f) && control.has_draw() && inside(p, f, &control) {
                        let sel = p.host.sel();
                        let no_tt = sel
                            .as_ref()
                            .is_some_and(|s| s.base().has_focus.get() && !crate::control::same(&**s, &*control));
                        control.draw(f, vp, no_tt);
                    }
                }
            }));
            let pw = Rc::downgrade(&popup);
            *popup.host.mouse_over_override.borrow_mut() = Some(Rc::new(move |g: &Gfx| {
                let p = pw.upgrade()?;
                let list: Vec<ControlRef> = p.host.controls.borrow().values().cloned().collect();
                list.into_iter()
                    .find(|c| c.is_mouse_over(g).is_some() && inside(&p, g, c) && c.is_mouse_over(g) == Some(true))
            }));
        }
    }

    /// `main:OpenAboutPopup(helpSectionIndex)` (Main.lua:1373-1484).
    pub fn open_about_popup(&self, g: &mut Gfx, help_section: Option<HelpSection>) {
        let (text_size, sub_title_size, title_size, popup_width) = (16.0, 20.0, 24.0, 810.0);
        let doc = launch().cfg.doc_path.clone();
        let read_lines = |name: &str| -> Option<Vec<String>> {
            let t = std::fs::read(format!("{doc}{name}")).ok()?;
            Some(String::from_utf8_lossy(&t).lines().map(str::to_string).collect())
        };
        let line = |h: f64, cols: Vec<String>| TextLine {
            height: h,
            cols: cols.into_iter().map(Some).collect(),
            ..Default::default()
        };
        let mut change_list: Vec<TextLine> = Vec::new();
        let mut change_version_heights: Vec<f64> = Vec::new();
        if let Some(lines) = read_lines("changelog.txt") {
            for l in lines {
                if let Some(c) = lua::lmatch_all(&l, "^VERSION%[(.+)%]%[(.+)%]$") {
                    if !change_list.is_empty() {
                        change_list.push(line(text_size / 2.0, Vec::new()));
                    }
                    change_version_heights.push(change_list.len() as f64 * text_size);
                    change_list.push(line(title_size, vec![format!("^7Version {} ({})", c[0], c[1])]));
                } else if lua::is_match(&l, "^---") {
                    change_list.push(line(sub_title_size, vec![format!("^7{l}")]));
                } else {
                    change_list.push(line(text_size, vec![format!("^7{l}")]));
                }
            }
        }
        let mut help_list: Vec<TextLine> = Vec::new();
        let mut help_sections: Vec<(String, f64)> = Vec::new();
        let mut help_section_heights: Vec<f64> = Vec::new();
        if let Some(lines) = read_lines("help.txt") {
            for l in lines {
                if let Some(title) = lua::lmatch(&l, "^---%[(.+)%]$") {
                    if !help_list.is_empty() {
                        help_list.push(line(text_size / 2.0, Vec::new()));
                    }
                    help_sections.push((title.clone(), help_list.len() as f64));
                    help_list.push(line(title_size, vec![format!("^7{title} ({})", help_sections.len())]));
                } else {
                    let dev = lua::lmatch(&l, "^DEV%[(.+)%]$");
                    // `if not (dev and not launch.devMode)`: devMode is never set → DEV lines are skipped.
                    if dev.is_none() {
                        let l = l.clone();
                        let col = "^7";
                        if let Some(c) = lua::lmatch_all(&l, "(.*)\t+(.*)") {
                            let (outdent, indent) = (c[0].clone(), c[1].clone());
                            let indent_lines = wrap_string(g, &indent, text_size, popup_width - 190.0);
                            if indent_lines.len() > 1 {
                                for (i, il) in indent_lines.iter().enumerate() {
                                    help_list.push(line(
                                        text_size,
                                        vec![if i == 0 { outdent.clone() } else { " ".into() }, format!("{col}{il}")],
                                    ));
                                }
                            } else {
                                help_list.push(line(text_size, vec![format!("{col}{outdent}"), format!("{col}{indent}")]));
                            }
                        } else {
                            for (i, l2) in wrap_string(g, &l, text_size, popup_width - 135.0).iter().enumerate() {
                                help_list.push(line(text_size, vec![format!("{col}{}{l2}", if i > 0 { "    " } else { "" })]));
                            }
                        }
                    }
                }
            }
            let mut contents_done = false;
            let n = help_sections.len();
            for si in 0..n {
                let section_index = si + 1;
                let (title, height) = help_sections[si].clone();
                if title == "Contents" {
                    // `t_insert(helpList, (sectionValues.height + sectionIndex), …)` (1-based positions).
                    let pos = height as usize + section_index;
                    help_list.insert(pos - 1, line(text_size, vec!["^7 ".into()]));
                    for (i, (t, _)) in help_sections.clone().iter().enumerate() {
                        let p = height as usize + (i + 1) + section_index;
                        help_list.insert(p - 1, line(text_size, vec![format!("^7{}. {t}", i + 1)]));
                    }
                }
                help_sections[si].1 = height + if contents_done { (n + 1) as f64 } else { 0.0 };
                help_section_heights.push(help_sections[si].1 * text_size);
                if title == "Contents" {
                    contents_done = true;
                }
            }
        }
        let help_section_index: Option<usize> = match help_section {
            None => None,
            Some(HelpSection::Index(i)) if i >= 1 && i <= help_sections.len() => Some(i),
            Some(HelpSection::Index(_)) => Some(1),
            Some(HelpSection::Name(name)) => Some(
                help_sections
                    .iter()
                    .position(|(t, _)| t.to_lowercase() == name)
                    .map_or(1, |i| i + 1),
            ),
        };
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert(
            "close".into(),
            ButtonControl::new(
                anchor_nil("TOPRIGHT", "TOPRIGHT"),
                rect(-10.0, 10.0, 50.0, 20.0),
                "Close".to_string(),
                |_| main().close_popup(),
            ),
        );
        controls.insert(
            "version".into(),
            LabelControl::new(
                None,
                rect(0.0, 18.0, 0.0, 18.0),
                format!("^7Path of Building Community Fork v{}", launch().version_number.borrow()),
            ),
        );
        controls.insert(
            "forum".into(),
            LabelControl::new(
                None,
                rect(0.0, 36.0, 0.0, 18.0),
                "^7Based on Openarl's Path of Building".to_string(),
            ),
        );
        controls.insert(
            "github".into(),
            ButtonControl::new(
                None,
                rect(0.0, 62.0, 438.0, 18.0),
                "^7GitHub page: ^x4040FFhttps://github.com/PathOfBuildingCommunity/PathOfBuilding".to_string(),
                |g| g.open_url("https://github.com/PathOfBuildingCommunity/PathOfBuilding"),
            ),
        );
        let has_section = help_section_index.is_some();
        let changelog = TextListControl::new(
            None,
            rect(0.0, 103.0, popup_width - 20.0, 515.0),
            Some(vec![TextColumn { x: 1.0, align: "LEFT" }, TextColumn { x: 135.0, align: "LEFT" }]),
            if has_section { help_list.clone() } else { change_list.clone() },
            Some(if has_section {
                help_section_heights.clone()
            } else {
                change_version_heights.clone()
            }),
        );
        let (cl, l1, h1) = (changelog.clone(), change_list, change_version_heights);
        controls.insert(
            "verLabel".into(),
            ButtonControl::new(
                anchor_nil("TOPLEFT", "TOPLEFT"),
                rect(10.0, 85.0, 100.0, 18.0),
                "^7Version history:".to_string(),
                move |_| {
                    *cl.list.borrow_mut() = l1.clone();
                    *cl.section_heights.borrow_mut() = Some(h1.clone());
                },
            ),
        );
        let (cl, l2, h2) = (changelog.clone(), help_list, help_section_heights);
        controls.insert(
            "helpLabel".into(),
            ButtonControl::new(
                anchor_nil("TOPRIGHT", "TOPRIGHT"),
                rect(-10.0, 85.0, 40.0, 18.0),
                "^7Help:".to_string(),
                move |_| {
                    *cl.list.borrow_mut() = l2.clone();
                    *cl.section_heights.borrow_mut() = Some(h2.clone());
                },
            ),
        );
        if let Some(i) = help_section_index {
            changelog.scroll_bar.offset.set(help_sections[i - 1].1 * text_size);
        }
        controls.insert("changelog".into(), changelog);
        self.open_popup(g, popup_width, 628.0, "About", controls, None, None, None, None);
    }

    /// `main:DrawBackground(viewPort)` (Main.lua:1486-1495).
    pub fn draw_background(&self, f: &mut Frame<'_>, vp: &Rect) {
        sg::set_draw_layer(f, None, -100);
        sg::set_draw_color(f, 0.5, 0.5, 0.5);
        sg::draw_image(
            f,
            Some(&self.background),
            vp.x,
            vp.y,
            vp.width,
            vp.height,
            Some([0.0, 0.0, vp.width / 100.0, vp.height / 100.0]),
        );
        sg::set_draw_layer(f, None, 0);
    }

    /// `main:StatColor(stat, base, limit)` (Main.lua:1574-1582).
    pub fn stat_color(stat: f64, base: Option<f64>, limit: Option<f64>) -> String {
        if limit.is_some_and(|l| stat > l) {
            crate::colors::color_code("NEGATIVE").expect("NEGATIVE")
        } else if base.is_some_and(|b| stat != b) {
            crate::colors::color_code("MAGIC").expect("MAGIC")
        } else {
            "^7".into()
        }
    }

    /// `main:MoveFolder(name, srcPath, dstPath)` (Main.lua:1584-1623).
    pub fn move_folder(&self, g: &mut Gfx, name: &str, src_path: &str, dst_path: &str) {
        if let Err(msg) = pob_gfx::fs::make_dir(&format!("{dst_path}{name}")) {
            self.open_message_popup(g, "Error", &format!("Couldn't move '{name}' to '{dst_path}' : {msg}"));
            return;
        }
        let mut handle = pob_gfx::fs::new_file_search(&format!("{src_path}{name}/*"), true);
        while let Some(h) = &mut handle {
            let n = h.get_file_name().to_string();
            self.move_folder(g, &n, &format!("{src_path}{name}/"), &format!("{dst_path}{name}/"));
            if !h.next_file() {
                break;
            }
        }
        let mut handle = pob_gfx::fs::new_file_search(&format!("{src_path}{name}/*"), false);
        while let Some(h) = &mut handle {
            let file_name = h.get_file_name().to_string();
            let src_name = format!("{src_path}{name}/{file_name}");
            let dst_name = format!("{dst_path}{name}/{file_name}");
            if let Err(msg) = os_rename(&src_name, &dst_name) {
                self.open_message_popup(g, "Error", &format!("Couldn't move '{src_name}' to '{dst_name}': {msg}"));
                return;
            }
            if !h.next_file() {
                break;
            }
        }
        if let Err(msg) = pob_gfx::fs::remove_dir(&format!("{src_path}{name}"), false) {
            self.open_message_popup(g, "Error", &format!("Couldn't delete '{dst_path}{name}' : {msg}"));
        }
    }

    /// `main:CopyFolder(srcName, dstName)` (Main.lua:1625-1658).
    pub fn copy_folder(&self, g: &mut Gfx, src_name: &str, dst_name: &str) {
        if let Err(msg) = pob_gfx::fs::make_dir(dst_name) {
            self.open_message_popup(g, "Error", &format!("Couldn't copy '{src_name}' to '{dst_name}' : {msg}"));
            return;
        }
        let mut handle = pob_gfx::fs::new_file_search(&format!("{src_name}/*"), true);
        while let Some(h) = &mut handle {
            let file_name = h.get_file_name().to_string();
            self.copy_folder(g, &format!("{src_name}/{file_name}"), &format!("{dst_name}/{file_name}"));
            if !h.next_file() {
                break;
            }
        }
        let mut handle = pob_gfx::fs::new_file_search(&format!("{src_name}/*"), false);
        while let Some(h) = &mut handle {
            let file_name = h.get_file_name().to_string();
            let s = format!("{src_name}/{file_name}");
            let d = format!("{dst_name}/{file_name}");
            if let Err(msg) = copy_file(&s, &d) {
                self.open_message_popup(g, "Error", &format!("Couldn't copy '{s}' to '{d}': {msg}"));
                return;
            }
            if !h.next_file() {
                break;
            }
        }
    }

    /// `main:OpenPopup(...)` (Main.lua:1660-1664).
    pub fn open_popup(
        &self,
        g: &mut Gfx,
        width: f64,
        height: f64,
        title: &str,
        controls: IndexMap<String, ControlRef>,
        enter_control: Option<&str>,
        default_control: Option<&str>,
        escape_control: Option<&str>,
        scroll_bar_func: Option<crate::controls::popup::PopupFunc>,
    ) -> Rc<PopupDialog> {
        let popup = PopupDialog::new(
            g,
            width,
            height,
            title,
            controls,
            enter_control,
            default_control,
            escape_control,
            scroll_bar_func,
            None,
        );
        self.popups.borrow_mut().insert(0, popup.clone());
        popup
    }

    /// `main:ClosePopup()` (Main.lua:1666-1668).
    pub fn close_popup(&self) {
        let mut p = self.popups.borrow_mut();
        if !p.is_empty() {
            p.remove(0);
        }
    }

    /// The numbered message labels of the message / confirm popups: `string.gmatch(msg .. "\n", "([^\n]*)\n")`.
    fn message_labels(controls: &mut IndexMap<String, ControlRef>, msg: &str) -> usize {
        let mut n = 0;
        for l in msg.split('\n') {
            controls.insert(
                (n + 1).to_string(),
                LabelControl::new(None, rect(0.0, 20.0 + n as f64 * 16.0, 0.0, 16.0), l.to_string()),
            );
            n += 1;
        }
        n
    }

    /// `main:OpenMessagePopup(title, msg)` (Main.lua:1670-1681).
    pub fn open_message_popup(&self, g: &mut Gfx, title: &str, msg: &str) -> Rc<PopupDialog> {
        let mut controls = IndexMap::new();
        let n = Self::message_labels(&mut controls, msg) as f64;
        controls.insert(
            "close".into(),
            ButtonControl::new(None, rect(0.0, 40.0 + n * 16.0, 80.0, 20.0), "Ok".to_string(), |_| {
                main().close_popup()
            }) as ControlRef,
        );
        let w = (sg::draw_string_width(g, 16.0, "VAR", msg) + 30.0).max(190.0);
        self.open_popup(g, w, 70.0 + n * 16.0, title, controls, Some("close"), None, None, None)
    }

    /// `main:OpenConfirmPopup(title, msg, confirmLabel, onConfirm, extraLabel, onExtra)` (Main.lua:1683-1726).
    pub fn open_confirm_popup(
        &self,
        g: &mut Gfx,
        title: &str,
        msg: &str,
        confirm_label: &str,
        on_confirm: Rc<dyn Fn(&mut Gfx)>,
        extra: Option<(&str, Rc<dyn Fn(&mut Gfx)>)>,
    ) -> Rc<PopupDialog> {
        let mut controls = IndexMap::new();
        let n = Self::message_labels(&mut controls, msg);
        let num = n as f64;
        let confirm_width = 80.0f64.max(sg::draw_string_width(g, 16.0, "VAR", confirm_label) + 10.0);
        let msg_w = sg::draw_string_width(g, 16.0, "VAR", msg) + 30.0;
        if let Some((extra_label, on_extra)) = extra {
            let extra_width = 80.0f64.max(sg::draw_string_width(g, 16.0, "VAR", extra_label) + 10.0);
            let cancel_width = 80.0;
            let spacing = 10.0;
            let total_width = confirm_width + extra_width + cancel_width + spacing * 2.0;
            let mut left_edge = -total_width / 2.0;
            let button_y = 40.0 + num * 16.0;
            let mut next = n + 1;
            let noop: Rc<dyn Fn(&mut Gfx)> = Rc::new(|_| {});
            for (width, label, on_click, is_confirm) in [
                (confirm_width, confirm_label, on_confirm, true),
                (extra_width, extra_label, on_extra, false),
                (cancel_width, "Cancel", noop, false),
            ] {
                let center_x = left_edge + width / 2.0;
                let ctrl = ButtonControl::new(None, rect(center_x, button_y, width, 20.0), label.to_string(), move |g| {
                    main().close_popup();
                    on_click(g);
                });
                if is_confirm {
                    controls.insert("confirm".into(), ctrl as ControlRef);
                } else {
                    controls.insert(next.to_string(), ctrl as ControlRef);
                    next += 1;
                }
                left_edge += width + spacing;
            }
            self.open_popup(
                g,
                msg_w.max(total_width + 40.0),
                70.0 + num * 16.0,
                title,
                controls,
                Some("confirm"),
                None,
                None,
                None,
            )
        } else {
            let half = (confirm_width / 2.0).ceil();
            controls.insert(
                "confirm".into(),
                ButtonControl::new(
                    None,
                    rect(-5.0 - half, 40.0 + num * 16.0, confirm_width, 20.0),
                    confirm_label.to_string(),
                    move |g| {
                        main().close_popup();
                        on_confirm(g);
                    },
                ) as ControlRef,
            );
            controls.insert(
                (n + 1).to_string(),
                ButtonControl::new(
                    None,
                    rect(5.0 + half, 40.0 + num * 16.0, confirm_width, 20.0),
                    "Cancel".to_string(),
                    |_| {
                        main().close_popup();
                    },
                ) as ControlRef,
            );
            self.open_popup(
                g,
                msg_w.max(190.0),
                70.0 + num * 16.0,
                title,
                controls,
                Some("confirm"),
                None,
                None,
                None,
            )
        }
    }

    /// `main:OpenNewFolderPopup(path, onClose)` (Main.lua:1729-1757).
    pub fn open_new_folder_popup(&self, g: &mut Gfx, path: &str, on_close: Option<Rc<dyn Fn(&mut Gfx, Option<&str>)>>) {
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert(
            "label".into(),
            LabelControl::new(None, rect(0.0, 20.0, 0.0, 16.0), "^7Enter folder name:".to_string()),
        );
        let create_cell: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
        let cc = create_cell.clone();
        let edit = EditControl::new(
            g,
            None,
            rect(0.0, 40.0, 350.0, 20.0),
            EditArgs {
                filter: Some("\\/:%*%?\"<>|%c".into()),
                limit: Some(100),
                change_func: Some(Rc::new(move |_g, buf: &str, _| {
                    if let Some(c) = cc.borrow().as_ref() {
                        c.base.enabled.set(lua::is_match(buf, "%S"));
                    }
                })),
                ..Default::default()
            },
        );
        controls.insert("edit".into(), edit.clone());
        let (p, oc) = (path.to_string(), on_close.clone());
        let create = ButtonControl::new(None, rect(-45.0, 70.0, 80.0, 20.0), "Create".to_string(), move |g| {
            let new_folder_name = edit.buf.borrow().clone();
            if let Err(msg) = pob_gfx::fs::make_dir(&format!("{p}{new_folder_name}")) {
                main().open_message_popup(g, "Error", &format!("Couldn't create '{new_folder_name}': {msg}"));
                return;
            }
            if let Some(oc) = &oc {
                oc(g, Some(&new_folder_name));
            }
            main().close_popup();
        });
        create.base.enabled.set(false);
        *create_cell.borrow_mut() = Some(create.clone());
        controls.insert("create".into(), create);
        controls.insert(
            "cancel".into(),
            ButtonControl::new(None, rect(45.0, 70.0, 80.0, 20.0), "Cancel".to_string(), move |g| {
                if let Some(oc) = &on_close {
                    oc(g, None);
                }
                main().close_popup();
            }),
        );
        self.open_popup(
            g,
            370.0,
            100.0,
            "New Folder",
            controls,
            Some("create"),
            Some("edit"),
            Some("cancel"),
            None,
        );
    }

    /// `main:OpenCloudErrorPopup(fileName)` (Main.lua:1761-1787).
    pub fn open_cloud_error_popup(&self, g: &mut Gfx, file_name: Option<&str>) -> Rc<PopupDialog> {
        let cloud = pob_gfx::fs::get_cloud_provider(file_name.unwrap_or(""));
        let (provider, status) = match &cloud {
            Some((p, _, s)) => (Some(p.clone()), Some(s.clone())),
            None => (None, None),
        };
        g.con_printf(&format!(
            "^1Error: file offline \"{}\" provider: \"{}\" status: \"{}\"",
            file_name.unwrap_or("?"),
            provider.as_deref().unwrap_or("nil"),
            status.as_deref().unwrap_or("nil")
        ));
        let fname = file_name.map_or(String::new(), |f| format!("\n\n^8'{f}'"));
        let l = launch();
        let version = format!(
            "^8v{}{}",
            l.version_number.borrow(),
            l.version_branch.borrow().as_ref().map(|b| format!(" {b}")).unwrap_or_default()
        );
        let title = " ^1Error ";
        let provider = provider.unwrap_or_else(|| "your cloud provider".into());
        let status_text = status.unwrap_or_else(|| "nil".into());
        let msg = format!(
            "\n^7Cannot read file.\n\nMake sure {provider} is running then restart {APP_NAME} and try again.{fname}\nstatus: {status_text}\n\n{version}"
        );
        let url = "https://github.com/PathOfBuildingCommunity/PathOfBuilding/wiki/CloudError";
        let mut controls = IndexMap::new();
        let n = Self::message_labels(&mut controls, &msg) as f64;
        let help = ButtonControl::new(None, rect(-55.0, 40.0 + n * 16.0, 80.0, 20.0), "Help (web)".to_string(), move |g| {
            g.open_url(url)
        });
        help.tip.tooltip_text.set(Some(url.to_string()));
        controls.insert("help".into(), help as ControlRef);
        controls.insert(
            "close".into(),
            ButtonControl::new(None, rect(55.0, 40.0 + n * 16.0, 80.0, 20.0), "Ok".to_string(), |_| {
                main().close_popup()
            }) as ControlRef,
        );
        let w = (sg::draw_string_width(g, 16.0, "VAR", &msg) + 30.0).max(190.0);
        self.open_popup(g, w, 70.0 + n * 16.0, title, controls, Some("close"), None, None, None)
    }

    /// `main:SetWindowTitleSubtext(subtext)` (Main.lua:1789-1795).
    pub fn set_window_title_subtext(&self, g: &mut Gfx, subtext: Option<&str>) {
        match subtext {
            Some(s) if self.s.borrow().show_titlebar_name => g.set_window_title(&format!("{s} - {APP_NAME}")),
            _ => g.set_window_title(APP_NAME),
        }
    }
}

/// `helpSectionIndex`: a number or a tab name ("build list tab").
#[derive(Clone, Debug)]
pub enum HelpSection {
    Index(usize),
    Name(String),
}

/// `os.rename(src, dst)` → `nil, "src: msg"` like Lua's.
pub fn os_rename(src: &str, dst: &str) -> Result<(), String> {
    std::fs::rename(src, dst).map_err(|e| format!("{src}: {}", io_msg(&e)))
}

/// `os.remove(name)`.
pub fn os_remove(name: &str) -> Result<(), String> {
    std::fs::remove_file(name).map_err(|e| format!("{name}: {}", io_msg(&e)))
}

/// The CRT `strerror` text Lua's `io`/`os` functions report.
fn io_msg(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory".into(),
        std::io::ErrorKind::PermissionDenied => "Permission denied".into(),
        std::io::ErrorKind::AlreadyExists => "File exists".into(),
        _ => e.to_string(),
    }
}

/// `copyFile(srcName, dstName)` (Common.lua:873-886): Windows text mode on both sides — reading turns `\r\n` into `\n`
/// and stops at Ctrl-Z, writing turns `\n` into `\r\n`.
pub fn copy_file(src: &str, dst: &str) -> Result<(), String> {
    let data = std::fs::read(src).map_err(|e| format!("Couldn't open '{src}': {src}: {}", io_msg(&e)))?;
    let mut out = Vec::with_capacity(data.len() + data.len() / 32);
    let mut i = 0;
    while i < data.len() && data[i] != 0x1A {
        if data[i] == b'\r' && data.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        if data[i] == b'\n' {
            out.push(b'\r');
        }
        out.push(data[i]);
        i += 1;
    }
    std::fs::write(dst, out).map_err(|e| format!("Couldn't create '{dst}': {dst}: {}", io_msg(&e)))
}

/// `io.open(name, "r")` succeeds.
pub fn file_exists(name: &str) -> bool {
    std::fs::File::open(name).is_ok()
}
