//! `Modules/Build.lua` — the `BUILD` mode shell (U2c): the top bar (back / save / save as, build name, point display, level,
//! class / ascendancy / bloodline, loadouts), the side bar (tab buttons, main-skill selection, the stat box and warnings), the
//! save popups and `buildFlag` → [`Engine::refresh`]. The build itself lives in `pob_engine::Engine`; this module keeps no copy
//! of it (UI-ARCH §4.3).
//!
//! Not ported here (later phases; each place is marked): the tabs (TREE U3, CALCS U4, SKILLS U5, ITEMS U6, CONFIG/NOTES U7,
//! IMPORT/PARTY/COMPARE U8 — `viewMode` switches, the tab area shows a placeholder), the sidebar breakdown popup
//! (`CalcBreakdownControl`, `SetDisplayStat`, overlay panes — U4), the level tooltip, class and
//! ascendancy changes and loadout switching (`PassiveSpec:SelectClass` …, U3/U5–U7), the
//! version-conversion popup, dev-mode autosave and `~~temp~~` builds (devMode is never set in the port), import links (U8).

use super::main::{BareControl, HelpSection, Mode, ModeArg, file_exists, main, sel_by_value};
use crate::control::{Anchor, Control, ControlBase, ControlRef, Rect, RectProps, anchor, anchor_nil, rect};
use crate::controls::build_list::FolderListControl;
use crate::controls::{
    ButtonControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl, TextColumn, TextLine, TextListControl,
};
use crate::host::{ControlHost, InputEvent, InputEvents};
use crate::launch::launch;
use crate::lua::{self, tostring_num};
use crate::sg;
use crate::tabs::calc_breakdown::{CalcBreakdownControl, SidebarData, Source};
use crate::tabs::calcs::{CalcSection, CalcsTab};
use crate::tooltip::Tooltip;
use crate::tree_view::PassiveTreeView;
use indexmap::IndexMap;
use pob_engine::calcs::{Entry, sections};
use pob_engine::calcs_tab::{BdEnv, BreakdownIndexEntry, ForceActor, breakdown_index};
use pob_engine::display::BreakdownKey;
use pob_engine::display::DisplaySettings;
use pob_engine::engine::Engine;
use pob_engine::shell::{MinionEntry, MinionSel, SkillLabel, SkillSelect};
use pob_gfx::{Frame, Gfx};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// `liveTargetVersion` (GameVersions.lua:4).
const LIVE_TARGET_VERSION: &str = "3_0";

/// `self.dbFileName`: `nil`, `false` (an unsaved new build) or the file.
#[derive(Clone, Debug, PartialEq)]
pub enum DbFile {
    Nil,
    False,
    Path(String),
}

impl DbFile {
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Path(p) => Some(p),
            _ => None,
        }
    }
}

/// A `new("Control"):Control(...)` whose `Draw` is assigned by the caller (`control.Draw = function(control) … end`).
pub struct DrawControl {
    pub base: ControlBase,
    /// `control.str` (`pointDisplay` / `warnings`) — `nil` = `None`.
    pub str_: RefCell<Option<String>>,
    pub req: RefCell<String>,
    #[allow(clippy::type_complexity)]
    pub draw_fn: RefCell<Option<Rc<dyn Fn(&DrawControl, &mut Frame<'_>)>>>,
}

impl DrawControl {
    pub fn new(anchor: Option<Anchor>, r: RectProps) -> Rc<Self> {
        Rc::new(Self {
            base: ControlBase::new(anchor, r),
            str_: RefCell::new(None),
            req: RefCell::new(String::new()),
            draw_fn: RefCell::new(None),
        })
    }
}

impl Control for DrawControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn has_draw(&self) -> bool {
        self.draw_fn.borrow().is_some()
    }
    fn draw(&self, f: &mut Frame<'_>, _view_port: &Rect, _no_tooltip: bool) {
        let d = self.draw_fn.borrow().clone();
        if let Some(d) = d {
            d(self, f);
        }
    }
}

/// `{point, other, otherPoint, true}`: an anchor that collapses onto `other` when it is hidden.
fn anchor_c(point: &'static str, other: &ControlRef, other_point: &'static str) -> Option<Anchor> {
    let mut a = anchor(point, other, other_point)?;
    a.collapse = true;
    Some(a)
}

fn item<T: 'static>(label: &str, data: T) -> DropItem {
    let mut it = DropItem::table(label);
    it.data = Some(Rc::new(data) as Rc<dyn Any>);
    it
}

/// The controls of the shell (`self.controls.*` of Build.lua, typed).
pub struct Controls {
    pub anchor_top_bar_left: Rc<BareControl>,
    pub anchor_top_bar_right: Rc<BareControl>,
    pub anchor_side_bar: Rc<BareControl>,
    pub back: Rc<ButtonControl>,
    pub save: Rc<ButtonControl>,
    pub save_as: Rc<ButtonControl>,
    pub build_name: Rc<DrawControl>,
    pub point_display: Rc<DrawControl>,
    pub level_scaling_button: Rc<ButtonControl>,
    pub character_level: Rc<EditControl>,
    pub class_drop: Rc<DropDownControl>,
    pub ascend_drop: Rc<DropDownControl>,
    pub secondary_ascend_drop: Rc<DropDownControl>,
    pub build_loadouts: Rc<DropDownControl>,
    pub main_socket_group: Rc<DropDownControl>,
    pub main_skill: Rc<DropDownControl>,
    pub main_skill_part: Rc<DropDownControl>,
    pub main_skill_stage_count: Rc<EditControl>,
    pub main_skill_mine_count: Rc<EditControl>,
    pub main_skill_minion: Rc<DropDownControl>,
    pub main_skill_minion_library: Rc<ButtonControl>,
    pub main_skill_minion_skill: Rc<DropDownControl>,
    pub stat_box: Rc<TextListControl>,
    pub warnings: Rc<DrawControl>,
}

/// The controls `RefreshSkillSelectControls` fills (the side bar's and the Calcs tab's).
pub struct SkillControls<'a> {
    pub main_socket_group: &'a Rc<DropDownControl>,
    pub main_skill: &'a Rc<DropDownControl>,
    pub main_skill_part: &'a Rc<DropDownControl>,
    pub main_skill_stage_count: &'a Rc<EditControl>,
    pub main_skill_mine_count: &'a Rc<EditControl>,
    pub main_skill_minion: &'a Rc<DropDownControl>,
    pub main_skill_minion_library: &'a Rc<ButtonControl>,
    pub main_skill_minion_skill: &'a Rc<DropDownControl>,
}

/// `self.breakdownInputs` (Build.lua:1378).
#[derive(Clone, PartialEq)]
struct BreakdownInputs {
    data: SidebarData,
    pin: bool,
    actor: Option<ForceActor>,
    source: Option<Rc<SidebarData>>,
}

pub struct BuildMode {
    pub host: ControlHost,
    /// `self.calcsTab`.
    pub calcs_tab: RefCell<Option<Rc<CalcsTab>>>,
    /// `self.skillsTab`.
    pub skills_tab: RefCell<Option<Rc<crate::tabs::skills::SkillsTab>>>,
    pub items_tab: RefCell<Option<Rc<crate::tabs::items::ItemsTab>>>,
    /// `self.controls.breakdown`.
    pub breakdown: RefCell<Option<Rc<CalcBreakdownControl>>>,
    /// `self.overlayPanes`.
    pub overlay_panes: RefCell<Vec<Rc<CalcSection>>>,
    overlay_breakdown_control: RefCell<Option<ControlRef>>,
    breakdown_inputs: RefCell<Option<BreakdownInputs>>,
    breakdown_index: RefCell<Option<Rc<std::collections::HashMap<String, BreakdownIndexEntry>>>>,
    me: Weak<BuildMode>,
    pub engine: RefCell<Option<Engine>>,
    /// `treeTab.viewer` (`None` until the tab is first drawn; a new one per opened build).
    pub tree_view: RefCell<Option<PassiveTreeView>>,
    /// `build.treeTab` (made at the tab's first draw; a new one per opened build).
    pub tree_tab: RefCell<Option<Rc<crate::tabs::tree::TreeTab>>>,
    pub config_tab: RefCell<Option<Rc<crate::tabs::config::ConfigTab>>>,
    pub notes_tab: RefCell<Option<Rc<crate::tabs::notes::NotesTab>>>,
    pub compare_tab: RefCell<Option<Rc<crate::tabs::compare::CompareTab>>>,
    pub import_tab: RefCell<Option<Rc<crate::tabs::import::ImportTab>>>,
    pub party_tab: RefCell<Option<Rc<crate::tabs::party::PartyTab>>>,
    pub db_file_name: RefCell<DbFile>,
    pub build_name: RefCell<Option<String>>,
    pub db_file_sub_path: RefCell<Option<String>>,
    pub save_as_sort_mode: RefCell<String>,
    pub view_mode: RefCell<String>,
    pub target_version: RefCell<Option<String>>,
    pub mod_flag: Cell<bool>,
    pub build_flag: Cell<bool>,
    pub unsaved: Cell<bool>,
    /// `nil` = `None`.
    pub abort_save: Cell<Option<bool>>,
    pub action_on_save: RefCell<Option<String>>,
    pub str_width: Cell<f64>,
    pub str_limited: Cell<bool>,
    pub misc_tooltip: RefCell<Tooltip>,
    pub c: RefCell<Option<Rc<Controls>>>,
    /// `warnings.lines`.
    pub warning_lines: RefCell<Vec<String>>,
    /// `lastShowThousandsSeparators` & co. (FormatStat) — the settings of the last `RefreshStatList`.
    last_display: RefCell<Option<DisplaySettings>>,
    last_show_titlebar_name: Cell<Option<bool>>,
    /// `secondaryAscendDropEntryCount` / the list last set (UpdateSecondaryAscendancyDropdown).
    secondary_list: RefCell<Option<Vec<(String, f64)>>>,
    pub initialised: Cell<bool>,
}

impl BuildMode {
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            host: ControlHost::new(),
            calcs_tab: RefCell::new(None),
            skills_tab: RefCell::new(None),
            items_tab: RefCell::new(None),
            breakdown: RefCell::new(None),
            overlay_panes: RefCell::new(Vec::new()),
            overlay_breakdown_control: RefCell::new(None),
            breakdown_inputs: RefCell::new(None),
            breakdown_index: RefCell::new(None),
            me: me.clone(),
            engine: RefCell::new(None),
            tree_view: RefCell::new(None),
            tree_tab: RefCell::new(None),
            config_tab: RefCell::new(None),
            notes_tab: RefCell::new(None),
            compare_tab: RefCell::new(None),
            import_tab: RefCell::new(None),
            party_tab: RefCell::new(None),
            db_file_name: RefCell::new(DbFile::Nil),
            build_name: RefCell::new(None),
            db_file_sub_path: RefCell::new(None),
            save_as_sort_mode: RefCell::new("NAME".into()),
            view_mode: RefCell::new("TREE".into()),
            target_version: RefCell::new(None),
            mod_flag: Cell::new(false),
            build_flag: Cell::new(false),
            unsaved: Cell::new(false),
            abort_save: Cell::new(None),
            action_on_save: RefCell::new(None),
            str_width: Cell::new(0.0),
            str_limited: Cell::new(false),
            misc_tooltip: RefCell::new(Tooltip::new()),
            c: RefCell::new(None),
            warning_lines: RefCell::new(Vec::new()),
            last_display: RefCell::new(None),
            last_show_titlebar_name: Cell::new(None),
            secondary_list: RefCell::new(None),
            initialised: Cell::new(false),
        })
    }

    pub fn c(&self) -> Rc<Controls> {
        self.c.borrow().clone().expect("build controls")
    }

    fn rc(&self) -> Rc<BuildMode> {
        self.me.upgrade().expect("buildMode")
    }

    /// `main.showThousandsSeparators` & co. as the engine's display settings.
    pub fn display_settings() -> DisplaySettings {
        let m = main();
        let s = m.s.borrow();
        DisplaySettings {
            show_thousands_separators: s.show_thousands_separators,
            thousands_separator: s.thousands_separator.clone(),
            decimal_separator: s.decimal_separator.clone(),
            use_compact_values: s.use_compact_values,
        }
    }

    /// `buildMode:CloseBuild()` (Build.lua:971-974).
    pub fn close_build(&self, g: &mut Gfx) {
        main().set_window_title_subtext(g, None);
        let name = match (&*self.db_file_name.borrow(), &*self.build_name.borrow()) {
            (DbFile::Path(_), Some(n)) => ModeArg::Str(n.clone()),
            _ => ModeArg::Bool(false),
        };
        let mut args = vec![name];
        if let Some(sp) = self.db_file_sub_path.borrow().clone() {
            args.push(ModeArg::Str(sp));
        }
        main().set_mode("LIST", args);
    }

    /// `self.spec:SetWindowTitleWithBuildClass()` (PassiveSpec.lua:2390-2396).
    fn set_window_title_with_build_class(&self, g: &mut Gfx) {
        let class_text = self.engine.borrow().as_ref().map(|e| e.class_select().title_class);
        if let Some(t) = class_text {
            let name = self.build_name.borrow().clone().unwrap_or_default();
            main().set_window_title_subtext(g, Some(&format!("{name} ({t})")));
        }
        self.last_show_titlebar_name.set(Some(main().s.borrow().show_titlebar_name));
    }

    /// `RefreshStatList` (Build.lua:2078-2119): the engine's last stat list into `statBox` / `warnings`, and
    /// `EstimatePlayerProgress`'s point display / level text (:910-939).
    fn apply_stat_list(&self, g: &mut Gfx) {
        let c = self.c();
        let eng = self.engine.borrow();
        let e = eng.as_ref().expect("engine");
        let list = e.stat_list();
        *c.stat_box.list.borrow_mut() = list
            .stat_box
            .iter()
            .map(|s| TextLine {
                height: s.height,
                font: None,
                x: s.x,
                align: s.align,
                underline: s
                    .underline
                    .map_or_else(Vec::new, |u| u.iter().map(|b| b.unwrap_or(false)).collect()),
                cols: vec![s.text.clone(), s.value.clone()],
            })
            .collect();
        *self.warning_lines.borrow_mut() = list.warnings.clone();
        let pd = e.point_display().clone();
        *c.point_display.str_.borrow_mut() = Some(pd.str);
        *c.point_display.req.borrow_mut() = pd.req;
        let (auto, level) = (e.model().character_level_auto_mode, e.model().level);
        drop(eng);
        let text = tostring_num(level);
        if auto && *c.character_level.buf.borrow() != text {
            c.character_level.set_text(g, &text, false);
        }
        *self.last_display.borrow_mut() = Some(Self::display_settings());
    }

    /// `buildMode:SaveDBFile()` (Build.lua:2304-2333).
    /// `self:SaveDB(fileName)` (Build.lua:2277-2301) with what the savers do first.
    pub fn save_db_text(&self) -> Result<String, String> {
        // NotesTab:Save (NotesTab.lua:84-87)
        if let Some(t) = self.notes_tab.borrow().as_ref() {
            t.before_save();
        }
        if let Some(t) = self.party_tab.borrow().as_ref() {
            t.sync();
        }
        // `viewMode = self.viewMode` (Build.lua:1012)
        if let Some(e) = self.engine.borrow_mut().as_mut() {
            e.set_view_mode(&self.view_mode.borrow());
        }
        self.engine.borrow().as_ref().expect("engine").save_xml()
    }

    /// `self.dbFileName`, `self.buildName` as `Init` takes them (ImportTab.lua:652).
    /// `buildMode:OpenConversionPopup()` (:1394-1419).
    fn open_conversion_popup(&self, g: &mut Gfx) {
        let versions = pob_data::tree::tree_versions().expect("GameVersions");
        let current = versions
            .tree_versions
            .iter()
            .find(|v| v.ver == versions.latest_tree_version)
            .map_or(String::new(), |v| v.display.clone());
        let tip = crate::colors::color_code("TIP").expect("TIP");
        let warning = crate::colors::color_code("WARNING").expect("WARNING");
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert(
            "note".into(),
            LabelControl::new(
                None,
                rect(0.0, 20.0, 0.0, 16.0),
                format!("{tip}Info:^7 You are trying to load a build created for a version of Path of Exile that is\nnot supported by us. You will have to convert it to the current game version to load it.\nTo use a build newer than the current supported game version, you may have to update.\nTo use a build older than the current supported game version, we recommend loading it\nin an older version of Path of Building Community instead.\n"),
            ),
        );
        controls.insert(
            "label".into(),
            LabelControl::new(
                None,
                rect(0.0, 110.0, 0.0, 16.0),
                format!("{warning}Warning:^7 Converting a build to a different game version may have side effects.\nFor example, if the passive tree has changed, then some passives may be deallocated.\nYou should create a backup copy of the build before proceeding.\n"),
            ),
        );
        let w = self.me.clone();
        controls.insert(
            "convert".into(),
            ButtonControl::new(None, rect(-40.0, 170.0, 120.0, 20.0), format!("Convert to {current}"), move |g| {
                main().close_popup();
                let b = w.upgrade().expect("buildMode");
                let (db, name) = b.init_args();
                b.shutdown(g);
                b.init(g, &[db, name, ModeArg::Bool(false), ModeArg::Bool(true)]);
            }),
        );
        let w = self.me.clone();
        controls.insert(
            "cancel".into(),
            ButtonControl::new(None, rect(60.0, 170.0, 70.0, 20.0), "Cancel".to_string(), move |g| {
                main().close_popup();
                w.upgrade().expect("buildMode").close_build(g);
            }),
        );
        main().open_popup(g, 580.0, 200.0, "Game Version", controls, Some("convert"), None, Some("cancel"), None);
    }

    pub fn init_args(&self) -> (ModeArg, ModeArg) {
        let db = match &*self.db_file_name.borrow() {
            DbFile::Path(p) => ModeArg::Str(p.clone()),
            DbFile::False | DbFile::Nil => ModeArg::Bool(false),
        };
        let name = self.build_name.borrow().clone().map_or(ModeArg::Bool(false), ModeArg::Str);
        (db, name)
    }

    pub fn save_db_file(&self, g: &mut Gfx) -> bool {
        let Some(path) = self.db_file_name.borrow().path().map(str::to_string) else {
            self.open_save_as_popup(g);
            return false;
        };
        let xml = match self.save_db_text() {
            Ok(x) => x,
            Err(e) => {
                launch().show_err_msg(&format!("Error saving '{path}': {e}"));
                return true;
            }
        };
        if std::fs::write(&path, xml).is_err() {
            main().open_message_popup(
                g,
                "Error",
                &format!("Couldn't save the build file:\n{path}\nMake sure the save folder exists and is writable."),
            );
            return true;
        }
        let action = self.action_on_save.borrow_mut().take();
        // `ResetModFlags` (:1100-1111): the tabs' flags are the engine's edits — one flag here
        self.mod_flag.set(false);
        if let Some(t) = self.calcs_tab.borrow().as_ref() {
            t.undo.mod_flag.set(false);
        }
        if let Some(t) = self.notes_tab.borrow().as_ref() {
            t.reset_mod_flags();
        }
        if let Some(t) = self.party_tab.borrow().as_ref() {
            t.reset_mod_flags();
        }
        if let Some(t) = self.config_tab.borrow().as_ref() {
            t.undo.mod_flag.set(false);
        }
        if let Some(t) = self.skills_tab.borrow().as_ref() {
            t.undo.mod_flag.set(false);
        }
        if let Some(t) = self.items_tab.borrow().as_ref() {
            t.undo.mod_flag.set(false);
        }
        match action.as_deref() {
            Some("LIST") => self.close_build(g),
            Some("EXIT") => g.exit(None),
            Some("UPDATE") => unimplemented!("Build.lua:2331 launch:ApplyUpdate: no auto-update in the port (UI-ARCH §10 п. 6)"),
            _ => {}
        }
        false
    }

    /// `buildMode:OpenSavePopup(mode)` (Build.lua:1421-1448).
    pub fn open_save_popup(&self, g: &mut Gfx, mode: &str) {
        let desc = match mode {
            "LIST" => "now?",
            "EXIT" => "before exiting?",
            "UPDATE" => "before updating?",
            _ => "",
        };
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert(
            "label".into(),
            LabelControl::new(
                None,
                rect(0.0, 20.0, 0.0, 16.0),
                format!("^7This build has unsaved changes.\nDo you want to save them {desc}"),
            ),
        );
        let (w, m1) = (self.me.clone(), mode.to_string());
        controls.insert(
            "save".into(),
            ButtonControl::new(None, rect(-90.0, 70.0, 80.0, 20.0), "Save".to_string(), move |g| {
                main().close_popup();
                let b = w.upgrade().expect("buildMode");
                *b.action_on_save.borrow_mut() = Some(m1.clone());
                b.save_db_file(g);
            }),
        );
        let (w, m2) = (self.me.clone(), mode.to_string());
        controls.insert(
            "noSave".into(),
            ButtonControl::new(None, rect(0.0, 70.0, 80.0, 20.0), "Don't Save".to_string(), move |g| {
                main().close_popup();
                match m2.as_str() {
                    "LIST" => w.upgrade().expect("buildMode").close_build(g),
                    "EXIT" => g.exit(None),
                    _ => unimplemented!("Build.lua:1441 launch:ApplyUpdate: no auto-update in the port"),
                }
            }),
        );
        controls.insert(
            "close".into(),
            ButtonControl::new(None, rect(90.0, 70.0, 80.0, 20.0), "Cancel".to_string(), |_| main().close_popup()),
        );
        main().open_popup(g, 300.0, 100.0, "Save Changes", controls, None, None, None, None);
    }

    /// `buildMode:OpenSaveAsPopup()` (Build.lua:1450-1519).
    pub fn open_save_as_popup(&self, g: &mut Gfx) {
        let new_file = Rc::new(RefCell::new((String::new(), String::new())));
        let edit_slot: Rc<RefCell<Option<Rc<EditControl>>>> = Rc::new(RefCell::new(None));
        let save_slot: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
        let folder_slot: Rc<RefCell<Option<Rc<FolderListControl>>>> = Rc::new(RefCell::new(None));
        let update_build_name = {
            let (nf, es, ss, fs) = (new_file.clone(), edit_slot.clone(), save_slot.clone(), folder_slot.clone());
            Rc::new(move || {
                let (Some(edit), Some(save), Some(folder)) = (es.borrow().clone(), ss.borrow().clone(), fs.borrow().clone()) else {
                    return;
                };
                let buf = edit.buf.borrow().clone();
                let name = format!("{}{}{buf}.xml", main().build_path.borrow(), folder.sub_path.borrow());
                *nf.borrow_mut() = (name.clone(), buf.clone());
                save.base.enabled.set(false);
                if lua::is_match(&buf, "%S") && !file_exists(&name) {
                    save.base.enabled.set(true);
                }
            })
        };
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert(
            "label".into(),
            LabelControl::new(None, rect(0.0, 20.0, 0.0, 16.0), "^7Enter new build name:".to_string()),
        );
        // `not self.dbFileName and main.predefinedBuildName or (self.buildName or self.dbFileName):gsub(…)`
        let predefined = main().predefined_build_name.borrow().clone().filter(|_| self.db_file_name.borrow().path().is_none());
        let init = predefined.unwrap_or_else(|| {
            let bn = self.build_name.borrow().clone();
            let src = bn
                .or_else(|| self.db_file_name.borrow().path().map(str::to_string))
                .unwrap_or_default();
            lua::gsub(&src, "[\\/:%*%?\"<>|%c]", "-")
        });
        let ub = update_build_name.clone();
        let edit = EditControl::new(
            g,
            None,
            rect(0.0, 40.0, 450.0, 20.0),
            EditArgs {
                init: Some(init),
                filter: Some("\\/:%*%?\"<>|%c".into()),
                limit: Some(100),
                change_func: Some(Rc::new(move |_g: &mut Gfx, _buf: &str, _| ub())),
                ..Default::default()
            },
        );
        *edit_slot.borrow_mut() = Some(edit.clone());
        controls.insert("edit".into(), edit);
        controls.insert(
            "folderLabel".into(),
            LabelControl::new(
                anchor_nil("TOPLEFT", "TOPLEFT"),
                rect(10.0, 70.0, 0.0, 16.0),
                "^7Folder:".to_string(),
            ),
        );
        let fs = folder_slot.clone();
        controls.insert(
            "newFolder".into(),
            ButtonControl::new(
                anchor_nil("TOPLEFT", "TOPLEFT"),
                rect(100.0, 67.0, 94.0, 20.0),
                "New Folder...".to_string(),
                move |g| {
                    let folder = fs.borrow().clone().expect("folder");
                    let path = format!("{}{}", main().build_path.borrow(), folder.sub_path.borrow());
                    let f2 = folder.clone();
                    main().open_new_folder_popup(
                        g,
                        &path,
                        Some(Rc::new(move |g: &mut Gfx, name: Option<&str>| {
                            if let Some(n) = name {
                                f2.open_folder(g, n);
                            }
                        })),
                    );
                },
            ),
        );
        let ub = update_build_name.clone();
        let sub_path = self.db_file_sub_path.borrow().clone();
        let folder = FolderListControl::new(
            g,
            None,
            rect(0.0, 115.0, 450.0, 400.0),
            sub_path.as_deref(),
            Some(Rc::new(move |_g: &mut Gfx, _sub: &str| ub())),
        );
        *folder.sort_mode.borrow_mut() = self.save_as_sort_mode.borrow().clone();
        folder.sort_list();
        *folder_slot.borrow_mut() = Some(folder.clone());
        controls.insert("folder".into(), folder.list.clone());
        let (w, nf, fs) = (self.me.clone(), new_file.clone(), folder_slot.clone());
        let save = ButtonControl::new(None, rect(-45.0, 525.0, 80.0, 20.0), "Save".to_string(), move |g| {
            main().close_popup();
            let b = w.upgrade().expect("buildMode");
            let (file, name) = nf.borrow().clone();
            *b.db_file_name.borrow_mut() = DbFile::Path(file);
            *b.build_name.borrow_mut() = Some(name);
            *b.db_file_sub_path.borrow_mut() = Some(fs.borrow().as_ref().expect("folder").sub_path.borrow().clone());
            b.save_db_file(g);
            b.set_window_title_with_build_class(g);
        });
        *save_slot.borrow_mut() = Some(save.clone());
        controls.insert("save".into(), save.clone());
        let w = self.me.clone();
        controls.insert(
            "close".into(),
            ButtonControl::new(None, rect(45.0, 525.0, 80.0, 20.0), "Cancel".to_string(), move |_| {
                main().close_popup();
                *w.upgrade().expect("buildMode").action_on_save.borrow_mut() = None;
            }),
        );
        let has_name = self.db_file_name.borrow().path().is_some() || self.build_name.borrow().is_some();
        if has_name {
            save.base.enabled.set(true);
            update_build_name();
        } else {
            save.base.enabled.set(false);
        }
        let (w, fs) = (self.me.clone(), folder_slot.clone());
        let sort = DropDownControl::new(
            g,
            crate::control::anchor_nil("TOPRIGHT", "TOPRIGHT"),
            rect(-10.0, 70.0, 120.0, 18.0),
            vec![
                item("Sort By Name", "NAME".to_string()),
                item("Sort By Last Edited", "EDITED".to_string()),
            ],
            Some(Rc::new(move |_g: &mut Gfx, _i, v: &DropItem| {
                let mode = v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned().expect("mode");
                *w.upgrade().expect("buildMode").save_as_sort_mode.borrow_mut() = mode.clone();
                let f = fs.borrow().clone().expect("folder");
                *f.sort_mode.borrow_mut() = mode;
                f.sort_list();
            })),
            None,
            false,
        );
        sort.tip.tooltip_text.set(Some("Sort folders by name or date modified.".into()));
        let cur = self.save_as_sort_mode.borrow().clone();
        sel_by_value(&sort, &cur);
        controls.insert("buildSortMode".into(), sort);
        let title = if self.db_file_name.borrow().path().is_some() {
            "Save As"
        } else {
            "Save"
        };
        main().open_popup(g, 470.0, 555.0, title, controls, Some("save"), Some("edit"), Some("close"), None);
    }

    /// `buildMode:SyncLoadouts()` (Build.lua:771-896): the loadout list and the index to select.
    /// Build.lua:320-357
    fn open_new_loadout_popup(&self, g: &mut Gfx) {
        let save: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
        let s1 = save.clone();
        let edit = crate::controls::edit::EditControl::new(
            g,
            None,
            rect(0.0, 40.0, 350.0, 20.0),
            crate::controls::edit::EditArgs {
                init: Some("New Loadout".to_string()),
                limit: Some(100),
                change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _: bool| {
                    let ok = buf.chars().any(|c| !c.is_whitespace());
                    if let Some(b) = s1.borrow().as_ref() {
                        b.base.enabled.set(ok);
                    }
                })),
                ..Default::default()
            },
        );
        let me = self.me.clone();
        let e2 = edit.clone();
        let btn = ButtonControl::new(None, rect(-45.0, 70.0, 80.0, 20.0), "Save".to_string(), move |g| {
            let s = me.upgrade().expect("buildMode");
            let name = e2.buf.borrow().clone();
            {
                let mut eng = s.engine.borrow_mut();
                let e = eng.as_mut().expect("engine");
                let mut spec = e.new_spec_model();
                spec.title = Some(name.clone());
                e.push_spec(spec);
                e.new_item_set(Some(&name), None);
                e.new_skill_set(&name).unwrap_or_else(|er| panic!("{er}"));
                let mut set = e.config_new_set();
                set.title = Some(name.clone());
                e.config_push_set(set);
            }
            s.sync_loadouts_pub(g);
            s.mod_flag.set(true);
            main().close_popup();
        });
        btn.base.enabled.set(false);
        *save.borrow_mut() = Some(btn.clone());
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert(
            "label".into(),
            LabelControl::new(None, rect(0.0, 20.0, 0.0, 16.0), "^7Enter name for this loadout:".to_string()),
        );
        controls.insert("edit".into(), edit);
        controls.insert("save".into(), btn);
        controls.insert(
            "cancel".into(),
            ButtonControl::new(None, rect(45.0, 70.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup()),
        );
        main().open_popup(g, 370.0, 100.0, "Set Name", controls, Some("save"), Some("edit"), Some("cancel"), None);
    }

    fn sync_loadouts(&self, g: &mut Gfx) {
        let (list, sel) = {
            let eng = self.engine.borrow();
            sync_loadouts(eng.as_ref().expect("engine").model())
        };
        let c = self.c();
        *c.build_loadouts.list.borrow_mut() = list.iter().map(|s| DropItem::str(s)).collect();
        c.build_loadouts.set_sel(g, sel as i64, false);
    }

    /// `RefreshSkillSelectControls(self.controls, self.mainSocketGroup, "")` (Build.lua:1618-1716).
    fn refresh_skill_select_controls(&self, g: &Gfx) {
        let c = self.c();
        let sel = {
            let eng = self.engine.borrow();
            eng.as_ref().expect("engine").skill_select().unwrap_or_else(|e| panic!("{e}"))
        };
        c.warnings.base.shown.set(!self.warning_lines.borrow().is_empty());
        apply_skill_select(
            g,
            &SkillControls {
                main_socket_group: &c.main_socket_group,
                main_skill: &c.main_skill,
                main_skill_part: &c.main_skill_part,
                main_skill_stage_count: &c.main_skill_stage_count,
                main_skill_mine_count: &c.main_skill_mine_count,
                main_skill_minion: &c.main_skill_minion,
                main_skill_minion_library: &c.main_skill_minion_library,
                main_skill_minion_skill: &c.main_skill_minion_skill,
            },
            &sel,
        );
    }

    /// `buildMode:UpdateSecondaryAscendancyDropdown()` + the class / ascendancy selections of `OnFrame` (Build.lua:1252-1256).
    fn update_class_dropdowns(&self, g: &mut Gfx) {
        let c = self.c();
        let cs = self.engine.borrow().as_ref().expect("engine").class_select();
        if let Some(id) = cs.class_id {
            sel_by_value(&c.class_drop, &id);
        }
        // `ascendDrop.list = classDrop:GetSelValueByKey("ascendancies")`
        let cur = cs.classes.iter().find(|e| {
            c.class_drop
                .get_sel_value()
                .and_then(|v| v.data.as_ref().and_then(|d| d.downcast_ref::<usize>().copied()))
                == Some(e.class_id)
        });
        let asc: Vec<DropItem> = cur
            .map(|e| e.ascendancies.iter().enumerate().map(|(i, a)| item(a, i)).collect())
            .unwrap_or_default();
        *c.ascend_drop.list.borrow_mut() = asc;
        if let Some(id) = cs.ascend_class_id {
            sel_by_value(&c.ascend_drop, &id);
        }
        c.ascend_drop.check_dropped_width(g, true);
        let changed = self.secondary_list.borrow().as_ref() != Some(&cs.secondary_list);
        if changed {
            c.secondary_ascend_drop
                .set_list(g, Some(cs.secondary_list.iter().map(|(l, id)| item(l, *id)).collect()));
            c.secondary_ascend_drop.check_dropped_width(g, true);
            *self.secondary_list.borrow_mut() = Some(cs.secondary_list.clone());
        }
        sel_by_value(&c.secondary_ascend_drop, &cs.secondary_ascend_class_id);
        c.secondary_ascend_drop.base.enabled.set(cs.secondary_list.len() > 1);
    }

    /// `controls.buildName.width` (Build.lua:148-157), the conditional of :144-146.
    fn build_name_conditional(&self, g: &Gfx) -> bool {
        self.c().anchor_top_bar_right.get_pos(g).0 < 800.0
    }

    /// Build.lua:119-602: the controls.
    fn create_controls(&self, g: &mut Gfx) -> Rc<Controls> {
        let w = self.me.clone();
        let bm = move || w.upgrade().expect("buildMode");
        let anchor_top_bar_left = BareControl::new(None, rect(4.0, 4.0, 0.0, 20.0));
        let atl: ControlRef = anchor_top_bar_left.clone();
        let b = bm.clone();
        let back = ButtonControl::new(
            anchor("LEFT", &atl, "RIGHT"),
            rect(0.0, 0.0, 60.0, 20.0),
            "<< Back".to_string(),
            move |g| {
                let s = b();
                if s.unsaved.get() {
                    s.open_save_popup(g, "LIST");
                } else {
                    s.close_build(g);
                }
            },
        );
        let back_r: ControlRef = back.clone();
        let b = bm.clone();
        let save = ButtonControl::new(
            anchor("LEFT", &back_r, "RIGHT"),
            rect(8.0, 0.0, 50.0, 20.0),
            "Save".to_string(),
            move |g| {
                b().save_db_file(g);
            },
        );
        let b = bm.clone();
        save.base
            .enabled
            .set_fn(move |_| b().db_file_name.borrow().path().is_none() || b().unsaved.get());
        let save_r: ControlRef = save.clone();
        let b = bm.clone();
        let save_as = ButtonControl::new(
            anchor("LEFT", &save_r, "RIGHT"),
            rect(8.0, 0.0, 70.0, 20.0),
            "Save As".to_string(),
            move |g| {
                b().open_save_as_popup(g);
            },
        );
        let b = bm.clone();
        // `enabled = function() return self.dbFileName end` (false/nil -> disabled)
        save_as.base.enabled.set_fn(move |_| b().db_file_name.borrow().path().is_some());
        let save_as_r: ControlRef = save_as.clone();
        let build_name = DrawControl::new(anchor("LEFT", &save_as_r, "RIGHT"), rect(4.0, 36.0, 0.0, 20.0));
        let b = bm.clone();
        build_name.base.width.set_fn(move |g| {
            let s = b();
            let c = s.c();
            let limit = if s.build_name_conditional(g) {
                203.0
            } else {
                c.anchor_top_bar_right.get_pos(g).0
                    - 98.0
                    - 62.0
                    - c.point_display.get_size(g).0
                    - c.level_scaling_button.get_size(g).0
                    - c.character_level.get_size(g).0
                    - c.back.get_size(g).0
                    - c.save.get_size(g).0
                    - c.save_as.get_size(g).0
            };
            let bnw = sg::draw_string_width(g, 16.0, "VAR", s.build_name.borrow().as_deref().unwrap_or(""));
            s.str_width.set(bnw.min(limit));
            s.str_limited.set(bnw > limit);
            s.str_width.get() + 98.0
        });
        let b = bm.clone();
        *build_name.draw_fn.borrow_mut() = Some(Rc::new(move |control: &DrawControl, f: &mut Frame<'_>| {
            let s = b();
            let (x, y) = control.get_pos(f);
            let (width, height) = control.get_size(f);
            let sw = s.str_width.get();
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
            sg::draw_rect(f, x + 91.0, y, sw + 6.0, 20.0);
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
            sg::draw_rect(f, x + 92.0, y + 1.0, sw + 4.0, 18.0);
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            sg::set_viewport(f, Some([x, y + 2.0, sw + 94.0, 16.0]));
            let name = s.build_name.borrow().clone().unwrap_or_default();
            sg::draw_string(f, 0.0, 0.0, Some("LEFT"), 16.0, "VAR", &format!("Current build:  {name}"));
            sg::set_viewport(f, None);
            if control.is_mouse_in_bounds(f) {
                sg::set_draw_layer(f, None, 10);
                let mut tt = s.misc_tooltip.borrow_mut();
                tt.clear(false);
                let sub = s.db_file_sub_path.borrow().clone();
                match sub {
                    Some(sp) if !sp.is_empty() => tt.add_line(f, 16.0, Some(&format!("{sp}{name}"))),
                    _ if s.str_limited.get() => tt.add_line(f, 16.0, Some(&name)),
                    _ => {}
                }
                tt.draw(f, x, y, Some((width, height)), &main().view_port.get());
                drop(tt);
                sg::set_draw_layer(f, None, 0);
            }
        }));
        let b = bm.clone();
        build_name
            .base
            .x
            .set_fn(move |g| if b().build_name_conditional(g) { -196.0 } else { 8.0 });
        let b = bm.clone();
        build_name
            .base
            .y
            .set_fn(move |g| if b().build_name_conditional(g) { 32.0 } else { 0.0 });

        let anchor_top_bar_right = BareControl::new(None, rect(0.0, 4.0, 0.0, 20.0));
        anchor_top_bar_right.base.x.set_fn(|_| crate::common::screen_size().0 / 2.0 + 6.0);
        let atr: ControlRef = anchor_top_bar_right.clone();
        let point_display = DrawControl::new(anchor("LEFT", &atr, "RIGHT"), rect(0.0, 0.0, 0.0, 20.0));
        let b = bm.clone();
        // `getPointDisplayX()`
        point_display.base.x.set_fn(move |g| {
            let c = b().c();
            -(23.0 + c.point_display.get_size(g).0 + c.level_scaling_button.get_size(g).0 + c.character_level.get_size(g).0)
        });
        let pdw = Rc::downgrade(&point_display);
        point_display.base.width.set_fn(move |g| {
            let p = pdw.upgrade().expect("pointDisplay");
            let s = p
                .str_
                .borrow()
                .clone()
                .expect("bad argument #3 to 'DrawStringWidth' (string expected, got nil)");
            sg::draw_string_width(g, 16.0, "FIXED", &s) + 8.0
        });
        let b = bm.clone();
        *point_display.draw_fn.borrow_mut() = Some(Rc::new(move |control: &DrawControl, f: &mut Frame<'_>| {
            let (x, y) = control.get_pos(f);
            let (width, height) = control.get_size(f);
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            sg::draw_rect(f, x, y, width + 2.0, height);
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
            sg::draw_rect(f, x + 1.0, y + 1.0, width, height - 2.0);
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            let s = control.str_.borrow().clone().unwrap_or_default();
            sg::draw_string(f, x + 4.0, y + 2.0, Some("LEFT"), 16.0, "FIXED", &s);
            if control.is_mouse_in_bounds(f) {
                sg::set_draw_layer(f, None, 10);
                let bs = b();
                let mut tt = bs.misc_tooltip.borrow_mut();
                tt.clear(false);
                let req = control.req.borrow().clone();
                tt.add_line(f, 16.0, Some(&req));
                tt.draw(f, x, y, Some((width, height)), &main().view_port.get());
                drop(tt);
                sg::set_draw_layer(f, None, 0);
            }
        }));
        let pd_r: ControlRef = point_display.clone();
        let auto = self.engine.borrow().as_ref().is_some_and(|e| e.model().character_level_auto_mode);
        let b = bm.clone();
        let level_scaling_button = ButtonControl::new(
            anchor("LEFT", &pd_r, "RIGHT"),
            rect(7.0, 0.0, 50.0, 20.0),
            if auto { "Auto" } else { "Manual" }.to_string(),
            move |_| {
                let s = b();
                let auto = {
                    let mut eng = s.engine.borrow_mut();
                    let e = eng.as_mut().expect("engine");
                    let a = !e.model().character_level_auto_mode;
                    e.set_character_level_auto_mode(a);
                    a
                };
                s.c()
                    .level_scaling_button
                    .label
                    .set(if auto { "Auto" } else { "Manual" }.to_string());
                s.mod_flag.set(true);
                s.build_flag.set(true);
            },
        );
        let lsb_r: ControlRef = level_scaling_button.clone();
        let b = bm.clone();
        let character_level = EditControl::new(
            g,
            anchor("LEFT", &lsb_r, "RIGHT"),
            rect(5.0, 0.0, 106.0, 20.0),
            EditArgs {
                init: Some(String::new()),
                prompt: Some("Level".into()),
                filter: Some("%D".into()),
                limit: Some(3),
                change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                    let s = b();
                    s.engine
                        .borrow_mut()
                        .as_mut()
                        .expect("engine")
                        .set_character_level(buf.parse::<f64>().ok());
                    s.mod_flag.set(true);
                    s.build_flag.set(true);
                    s.c().level_scaling_button.label.set("Manual".to_string());
                })),
                ..Default::default()
            },
        );
        // `characterLevel.tooltipFunc` (experience multipliers, data.monsterExperienceLevelMap): U5 with the data tables.
        let level = self.engine.borrow().as_ref().map_or(1.0, |e| e.model().level);
        character_level.set_text(g, &tostring_num(level), false);
        let cl_r: ControlRef = character_level.clone();
        let class_drop = DropDownControl::new(
            g,
            anchor("LEFT", &cl_r, "RIGHT"),
            rect(10.0, 0.0, 85.0, 20.0),
            Vec::new(),
            Some(Rc::new(|g: &mut Gfx, _i, v: &DropItem| {
                crate::tabs::tree::class_drop_selected(g, v)
            })),
            None,
            false,
        );
        let cd_r: ControlRef = class_drop.clone();
        let ascend_drop = DropDownControl::new(
            g,
            anchor("LEFT", &cd_r, "RIGHT"),
            rect(4.0, 0.0, 120.0, 20.0),
            Vec::new(),
            Some(Rc::new(|g: &mut Gfx, _i, v: &DropItem| {
                crate::tabs::tree::ascend_drop_selected(g, v)
            })),
            None,
            false,
        );
        let ad_r: ControlRef = ascend_drop.clone();
        let secondary_ascend_drop = DropDownControl::new(
            g,
            anchor("LEFT", &ad_r, "RIGHT"),
            rect(4.0, 0.0, 160.0, 20.0),
            vec![item("None", 0.0f64)],
            Some(Rc::new(|g: &mut Gfx, _i, v: &DropItem| {
                crate::tabs::tree::secondary_drop_selected(g, v)
            })),
            None,
            false,
        );
        secondary_ascend_drop.enable_dropped_width.set(true);
        secondary_ascend_drop.max_dropped_width.set(360.0);
        let sad_r: ControlRef = secondary_ascend_drop.clone();
        let b = bm.clone();
        let build_loadouts = DropDownControl::new(
            g,
            anchor("LEFT", &sad_r, "RIGHT"),
            rect(4.0, 0.0, 190.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, _i, v: &DropItem| {
                let s = b();
                let c = s.c();
                let value = v.label.clone().unwrap_or_default();
                match value.as_str() {
                    "^7^7Loadouts:" | "^7^7-----" => c.build_loadouts.set_sel(g, 1, false),
                    "^7^7Sync" => {
                        s.sync_loadouts(g);
                        c.build_loadouts.set_sel(g, 1, false);
                    }
                    "^7^7Help >>" => {
                        main().open_about_popup(g, Some(HelpSection::Index(7)));
                        c.build_loadouts.set_sel(g, 1, false);
                    }
                    "^7^7New Loadout" => {
                        s.open_new_loadout_popup(g);
                        c.build_loadouts.set_sel(g, 1, false);
                    }
                    _ => {
                        let same = {
                            let eng = s.engine.borrow();
                            loadout_is_active(eng.as_ref().expect("engine").model(), &value)
                        };
                        match same {
                            Some(true) => c.build_loadouts.sel_by_label(&value),
                            Some(false) => {
                                let ids = {
                                    let eng = s.engine.borrow();
                                    loadout_set_ids(eng.as_ref().expect("engine").model(), &value)
                                };
                                // Build.lua:401-403 no exact match nor special grouping: bail
                                let Some((spec_id, item_id, skill_id, config_id)) = ids else {
                                    return;
                                };
                                let (active_spec, active_item, active_skill, active_config) = {
                                    let eng = s.engine.borrow();
                                    let e = eng.as_ref().expect("engine");
                                    (
                                        e.model().tree.active_spec as usize,
                                        e.active_item_set_id(),
                                        e.skills_tab().active_skill_set_id,
                                        e.model().config.active_config_set,
                                    )
                                };
                                if spec_id != active_spec {
                                    let tab = s.tree_tab.borrow().clone().expect("treeTab");
                                    tab.set_active_spec(g, spec_id);
                                }
                                if item_id != active_item {
                                    let tab = s.items_tab.borrow().clone().expect("itemsTab");
                                    tab.set_active_item_set(g, item_id);
                                }
                                if skill_id != active_skill {
                                    let tab = s.skills_tab.borrow().clone().expect("skillsTab");
                                    tab.set_active_skill_set(g, skill_id);
                                }
                                if config_id != active_config {
                                    let tab = s.config_tab.borrow().clone().expect("configTab");
                                    tab.set_active_config_set(g, config_id);
                                }
                                c.build_loadouts.sel_by_label(&value);
                            }
                            None => {}
                        }
                    }
                }
            })),
            None,
            false,
        );

        let anchor_side_bar = BareControl::new(None, rect(4.0, 60.0, 0.0, 0.0));
        let b = bm.clone();
        anchor_side_bar
            .base
            .y
            .set_fn(move |g| if b().build_name_conditional(g) { 60.0 } else { 36.0 });
        let asb: ControlRef = anchor_side_bar.clone();
        let mode_button = |a: Option<Anchor>, r: RectProps, label: &str, mode: &'static str| {
            let b = bm.clone();
            let btn = ButtonControl::new(a, r, label.to_string(), move |_| {
                *b().view_mode.borrow_mut() = mode.to_string();
            });
            let b = bm.clone();
            btn.locked.set_fn(move |_| *b().view_mode.borrow() == mode);
            btn
        };
        let mode_import = mode_button(
            anchor("TOPLEFT", &asb, "TOPLEFT"),
            rect(0.0, 0.0, 134.0, 20.0),
            "Import/Export Build",
            "IMPORT",
        );
        let mi_r: ControlRef = mode_import.clone();
        // Build.lua:442-445
        let bi = bm.clone();
        *mode_import.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| {
            *bi().view_mode.borrow_mut() = "IMPORT".to_string();
            let tab = bi().import_tab.borrow().clone();
            if let Some(t) = tab {
                t.try_fetch_character_list(g);
            }
        });
        let mode_notes = mode_button(anchor("LEFT", &mi_r, "RIGHT"), rect(4.0, 0.0, 58.0, 20.0), "Notes", "NOTES");
        let mode_config = mode_button(
            anchor("TOPRIGHT", &asb, "TOPLEFT"),
            rect(300.0, 0.0, 100.0, 20.0),
            "Configuration",
            "CONFIG",
        );
        let mode_tree = mode_button(anchor("TOPLEFT", &asb, "TOPLEFT"), rect(0.0, 26.0, 72.0, 20.0), "Tree", "TREE");
        let mt_r: ControlRef = mode_tree.clone();
        let mode_skills = mode_button(anchor("LEFT", &mt_r, "RIGHT"), rect(4.0, 0.0, 72.0, 20.0), "Skills", "SKILLS");
        let ms_r: ControlRef = mode_skills.clone();
        let mode_items = mode_button(anchor("LEFT", &ms_r, "RIGHT"), rect(4.0, 0.0, 72.0, 20.0), "Items", "ITEMS");
        let mit_r: ControlRef = mode_items.clone();
        let mode_calcs = mode_button(anchor("LEFT", &mit_r, "RIGHT"), rect(4.0, 0.0, 72.0, 20.0), "Calcs", "CALCS");
        let mode_party = mode_button(anchor("TOPLEFT", &asb, "TOPLEFT"), rect(0.0, 52.0, 72.0, 20.0), "Party", "PARTY");
        let mp_r: ControlRef = mode_party.clone();
        let mode_compare = mode_button(anchor("LEFT", &mp_r, "RIGHT"), rect(4.0, 0.0, 72.0, 20.0), "Compare", "COMPARE");

        let main_skill_label = LabelControl::new(
            anchor("TOPLEFT", &asb, "TOPLEFT"),
            rect(0.0, 80.0, 300.0, 16.0),
            "^7Main Skill:".to_string(),
        );
        let msl_r: ControlRef = main_skill_label.clone();
        let b = bm.clone();
        let main_socket_group = DropDownControl::new(
            g,
            anchor("TOPLEFT", &msl_r, "BOTTOMLEFT"),
            rect(0.0, 2.0, 300.0, 18.0),
            Vec::new(),
            Some(Rc::new(move |_g: &mut Gfx, index, _v: &DropItem| {
                let s = b();
                s.engine
                    .borrow_mut()
                    .as_mut()
                    .expect("engine")
                    .select_main_socket_group(index as f64);
                s.mod_flag.set(true);
                s.build_flag.set(true);
            })),
            None,
            false,
        );
        main_socket_group.max_dropped_width.set(500.0);
        // :487-492
        let b = bm.clone();
        main_socket_group
            .tip
            .set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, (_mode, index, _v)| {
                let s = b();
                let eng = s.engine.borrow();
                let e = eng.as_ref().expect("engine");
                if e.socket_groups().get(index.wrapping_sub(1)).is_some() && tooltip.check_for_update((index, e.output_revision())) {
                    crate::gem_tooltip::add_socket_group_tooltip(tooltip, g, e, index);
                }
            });
        let msg_r: ControlRef = main_socket_group.clone();
        let edit_skill = |f: fn(&mut Engine, usize, &DropItem) -> Result<(), String>| {
            let b = bm.clone();
            Some(Rc::new(move |_g: &mut Gfx, index: usize, v: &DropItem| {
                let s = b();
                f(s.engine.borrow_mut().as_mut().expect("engine"), index, v).unwrap_or_else(|e| panic!("{e}"));
                s.mod_flag.set(true);
                s.build_flag.set(true);
            }) as Rc<dyn Fn(&mut Gfx, usize, &DropItem)>)
        };
        let main_skill = DropDownControl::new(
            g,
            anchor("TOPLEFT", &msg_r, "BOTTOMLEFT"),
            rect(0.0, 2.0, 300.0, 18.0),
            Vec::new(),
            edit_skill(|e, i, _| e.set_main_active_skill(i as f64)),
            None,
            false,
        );
        let msk_r: ControlRef = main_skill.clone();
        let main_skill_part = DropDownControl::new(
            g,
            anchor_c("TOPLEFT", &msk_r, "BOTTOMLEFT"),
            rect(0.0, 2.0, 300.0, 18.0),
            Vec::new(),
            edit_skill(|e, i, _| e.set_skill_part(i as f64)),
            None,
            false,
        );
        let msp_r: ControlRef = main_skill_part.clone();
        let stage_label = LabelControl::new(
            anchor_c("TOPLEFT", &msp_r, "BOTTOMLEFT"),
            rect(0.0, 3.0, 0.0, 16.0),
            "^7Stages:".to_string(),
        );
        let stl_r: ControlRef = stage_label.clone();
        let b = bm.clone();
        let main_skill_stage_count = EditControl::new(
            g,
            anchor_c("LEFT", &stl_r, "RIGHT"),
            rect(2.0, 0.0, 60.0, 18.0),
            EditArgs {
                filter: Some("%D".into()),
                change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                    let s = b();
                    s.engine
                        .borrow_mut()
                        .as_mut()
                        .expect("engine")
                        .set_skill_stage_count(buf.parse::<f64>().ok())
                        .unwrap_or_else(|e| panic!("{e}"));
                    s.mod_flag.set(true);
                    s.build_flag.set(true);
                })),
                ..Default::default()
            },
        );
        let sc = Rc::downgrade(&main_skill_stage_count);
        stage_label
            .base
            .shown
            .set_fn(move |g| sc.upgrade().expect("stageCount").is_shown(g));
        let mine_label = LabelControl::new(
            anchor_c("TOPLEFT", &stl_r, "BOTTOMLEFT"),
            rect(0.0, 3.0, 0.0, 16.0),
            "^7Active Mines:".to_string(),
        );
        let mnl_r: ControlRef = mine_label.clone();
        let b = bm.clone();
        let main_skill_mine_count = EditControl::new(
            g,
            anchor_c("LEFT", &mnl_r, "RIGHT"),
            rect(2.0, 0.0, 60.0, 18.0),
            EditArgs {
                filter: Some("%D".into()),
                change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                    let s = b();
                    s.engine
                        .borrow_mut()
                        .as_mut()
                        .expect("engine")
                        .set_skill_mine_count(buf.parse::<f64>().ok())
                        .unwrap_or_else(|e| panic!("{e}"));
                    s.mod_flag.set(true);
                    s.build_flag.set(true);
                })),
                ..Default::default()
            },
        );
        let mc = Rc::downgrade(&main_skill_mine_count);
        mine_label.base.shown.set_fn(move |g| mc.upgrade().expect("mineCount").is_shown(g));
        let main_skill_minion = DropDownControl::new(
            g,
            anchor_c("TOPLEFT", &mnl_r, "BOTTOMLEFT"),
            rect(0.0, 3.0, 178.0, 18.0),
            Vec::new(),
            edit_skill(|e, _, v| {
                let entry = v
                    .data
                    .as_ref()
                    .and_then(|d| d.downcast_ref::<MinionEntry>())
                    .cloned()
                    .unwrap_or_else(|| MinionEntry::Text(v.label.clone().unwrap_or_default()));
                e.set_skill_minion(&entry)
            }),
            None,
            false,
        );
        // `CanReceiveDrag` / `ReceiveDrag` (items onto the minion's item set) and `tooltipFunc`: U6.
        let mm_r: ControlRef = main_skill_minion.clone();
        let main_skill_minion_library = ButtonControl::new(
            anchor("LEFT", &mm_r, "RIGHT"),
            rect(2.0, 0.0, 120.0, 18.0),
            "Manage Spectres...".to_string(),
            crate::tabs::minion_list::open_spectre_library,
        );
        let main_skill_minion_skill = DropDownControl::new(
            g,
            anchor_c("TOPLEFT", &mm_r, "BOTTOMLEFT"),
            rect(0.0, 2.0, 200.0, 16.0),
            Vec::new(),
            edit_skill(|e, i, _| e.set_skill_minion_skill(i as f64)),
            None,
            false,
        );
        let mmsk_r: ControlRef = main_skill_minion_skill.clone();
        let stat_box_anchor = BareControl::new(anchor_c("TOPLEFT", &mmsk_r, "BOTTOMLEFT"), rect(0.0, 2.0, 0.0, 0.0));
        let sba_r: ControlRef = stat_box_anchor.clone();
        let stat_box = TextListControl::new(
            anchor("TOPLEFT", &sba_r, "BOTTOMLEFT"),
            rect(0.0, 2.0, 300.0, 0.0),
            Some(vec![
                TextColumn {
                    x: 170.0,
                    align: "RIGHT_X",
                },
                TextColumn { x: 174.0, align: "LEFT" },
            ]),
            Vec::new(),
            None,
        );
        let (b, sbw) = (bm.clone(), Rc::downgrade(&stat_box));
        stat_box.base.height.set_fn(move |g| {
            let s = b();
            let (_, y) = sbw.upgrade().expect("statBox").get_pos(g);
            let m = main();
            let warn_height = if m.s.borrow().show_warnings && !s.warning_lines.borrow().is_empty() {
                18.0
            } else {
                0.0
            };
            crate::common::screen_size().1 - m.main_bar_height.get() - 4.0 - y - warn_height
        });
        let b = bm.clone();
        *stat_box.on_click.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, hovered| b().set_display_stat(g, hovered, true)));
        let sb_r: ControlRef = stat_box.clone();
        let warnings = DrawControl::new(anchor_c("TOPLEFT", &sb_r, "BOTTOMLEFT"), rect(0.0, 0.0, 0.0, 18.0));
        let ww = Rc::downgrade(&warnings);
        warnings.base.width.set_fn(move |g| {
            let wc = ww.upgrade().expect("warnings");
            let s = wc.str_.borrow().clone();
            s.map_or(0.0, |s| sg::draw_string_width(g, 16.0, "FIXED", &s) + 8.0)
        });
        let b = bm.clone();
        *warnings.draw_fn.borrow_mut() = Some(Rc::new(move |control: &DrawControl, f: &mut Frame<'_>| {
            let s = b();
            let lines = s.warning_lines.borrow().clone();
            if !lines.is_empty() {
                let neg = crate::colors::color_code("NEGATIVE").expect("NEGATIVE");
                *control.str_.borrow_mut() = Some(format!("{neg}{} Warnings", lines.len()));
                let (x, y) = control.get_pos(f);
                let (width, height) = control.get_size(f);
                let st = control.str_.borrow().clone().unwrap_or_default();
                sg::draw_string(f, x, y + 2.0, Some("LEFT"), 16.0, "FIXED", &st);
                if control.is_mouse_in_bounds(f) {
                    sg::set_draw_layer(f, None, 10);
                    let mut tt = s.misc_tooltip.borrow_mut();
                    tt.clear(false);
                    for l in &lines {
                        tt.add_line(f, 16.0, Some(l));
                    }
                    tt.draw(f, x, y, Some((width, height)), &main().view_port.get());
                    drop(tt);
                    sg::set_draw_layer(f, None, 0);
                }
            } else {
                // `control.str = {}`: a table, never measured while there are no lines
                *control.str_.borrow_mut() = None;
            }
        }));

        let h = &self.host;
        h.add("back", back.clone());
        h.add("save", save.clone());
        h.add("saveAs", save_as.clone());
        h.add("buildName", build_name.clone());
        h.add("pointDisplay", point_display.clone());
        h.add("levelScalingButton", level_scaling_button.clone());
        h.add("characterLevel", character_level.clone());
        h.add("classDrop", class_drop.clone());
        h.add("ascendDrop", ascend_drop.clone());
        h.add("secondaryAscendDrop", secondary_ascend_drop.clone());
        h.add("buildLoadouts", build_loadouts.clone());
        h.add("modeImport", mode_import);
        h.add("modeNotes", mode_notes);
        h.add("modeConfig", mode_config);
        h.add("modeTree", mode_tree);
        h.add("modeSkills", mode_skills);
        h.add("modeItems", mode_items);
        h.add("modeCalcs", mode_calcs);
        h.add("modeParty", mode_party);
        h.add("modeCompare", mode_compare);
        h.add("mainSkillLabel", main_skill_label);
        h.add("mainSocketGroup", main_socket_group.clone());
        h.add("mainSkill", main_skill.clone());
        h.add("mainSkillPart", main_skill_part.clone());
        h.add("mainSkillStageCountLabel", stage_label);
        h.add("mainSkillStageCount", main_skill_stage_count.clone());
        h.add("mainSkillMineCountLabel", mine_label);
        h.add("mainSkillMineCount", main_skill_mine_count.clone());
        h.add("mainSkillMinion", main_skill_minion.clone());
        h.add("mainSkillMinionLibrary", main_skill_minion_library.clone());
        h.add("mainSkillMinionSkill", main_skill_minion_skill.clone());
        h.add("statBoxAnchor", stat_box_anchor);
        h.add("statBox", stat_box.clone());
        h.add("warnings", warnings.clone());
        Rc::new(Controls {
            anchor_top_bar_left,
            anchor_top_bar_right,
            anchor_side_bar,
            back,
            save,
            save_as,
            build_name,
            point_display,
            level_scaling_button,
            character_level,
            class_drop,
            ascend_drop,
            secondary_ascend_drop,
            build_loadouts,
            main_socket_group,
            main_skill,
            main_skill_part,
            main_skill_stage_count,
            main_skill_mine_count,
            main_skill_minion,
            main_skill_minion_library,
            main_skill_minion_skill,
            stat_box,
            warnings,
        })
    }

    /// `UpdateClassDropdowns()` (Build.lua:1553-1574) over the spec's tree.
    fn fill_class_drop(&self, g: &Gfx) {
        let cs = self.engine.borrow().as_ref().expect("engine").class_select();
        let c = self.c();
        *c.class_drop.list.borrow_mut() = cs.classes.iter().map(|e| item(&e.label, e.class_id)).collect();
        let _ = g;
    }
}

/// Build.lua:1619-1716 `RefreshSkillSelectControls` over the engine's answer.
pub fn apply_skill_select(g: &Gfx, c: &SkillControls<'_>, sel: &SkillSelect) {
    c.main_socket_group.sel_index.set(sel.main_group as usize);
    *c.main_socket_group.list.borrow_mut() = sel.groups.iter().map(|l| DropItem::table(l)).collect();
    c.main_socket_group.check_dropped_width(g, true);
    let Some(m) = &sel.main else {
        *c.main_socket_group.list.borrow_mut() = vec![DropItem::table("<No skills added yet>")];
        for s in [&c.main_skill, &c.main_skill_part, &c.main_skill_minion, &c.main_skill_minion_skill] {
            s.base.shown.set(false);
        }
        c.main_skill_mine_count.base.shown.set(false);
        c.main_skill_stage_count.base.shown.set(false);
        return;
    };
    *c.main_skill.list.borrow_mut() = m
        .skills
        .iter()
        .map(|l| match l {
            SkillLabel::Name(n) => DropItem::table(n),
            SkillLabel::From(n, rarity) => {
                let col = crate::colors::color_code(rarity.as_deref().unwrap_or("NORMAL")).unwrap_or_default();
                DropItem::table(&format!("From {col}{n}"))
            }
        })
        .collect();
    c.main_skill.base.enabled.set(m.skills.len() > 1);
    c.main_skill.sel_index.set(m.skill_sel as usize);
    c.main_skill.base.shown.set(true);
    c.main_skill_part.base.shown.set(false);
    c.main_skill_mine_count.base.shown.set(false);
    c.main_skill_stage_count.base.shown.set(false);
    c.main_skill_minion.base.shown.set(false);
    c.main_skill_minion_library.base.shown.set(false);
    c.main_skill_minion_skill.base.shown.set(false);
    if let Some((parts, ps)) = &m.part {
        c.main_skill_part.base.shown.set(true);
        *c.main_skill_part.list.borrow_mut() = parts.iter().map(|p| DropItem::table(p)).collect();
        c.main_skill_part.sel_index.set(*ps as usize);
    }
    if let Some(buf) = &m.stage_count {
        c.main_skill_stage_count.base.shown.set(true);
        *c.main_skill_stage_count.buf.borrow_mut() = buf.clone();
    }
    if let Some(buf) = &m.mine_count {
        c.main_skill_mine_count.base.shown.set(true);
        *c.main_skill_mine_count.buf.borrow_mut() = buf.clone();
    }
    if let Some(ms) = &m.minion {
        let mut list: Vec<DropItem> = Vec::new();
        for e in &ms.list {
            list.push(match e {
                MinionEntry::ItemSet { label, .. } | MinionEntry::Minion { label, .. } => item(label, e.clone()),
                MinionEntry::Text(t) => DropItem::str(t),
            });
        }
        c.main_skill_minion_library.base.shown.set(ms.library_shown);
        *c.main_skill_minion.list.borrow_mut() = list;
        match &ms.sel {
            MinionSel::ItemSet(id) => {
                let idx = ms
                    .list
                    .iter()
                    .position(|e| matches!(e, MinionEntry::ItemSet { item_set_id, .. } if item_set_id == id));
                if let Some(i) = idx {
                    c.main_skill_minion.sel_index.set(i + 1);
                }
            }
            MinionSel::Minion(Some(id)) => {
                let idx = ms
                    .list
                    .iter()
                    .position(|e| matches!(e, MinionEntry::Minion { minion_id, .. } if minion_id == id));
                if let Some(i) = idx {
                    c.main_skill_minion.sel_index.set(i + 1);
                }
            }
            MinionSel::Minion(None) => {}
        }
        c.main_skill_minion.base.enabled.set(ms.enabled);
        c.main_skill_minion.base.shown.set(true);
        if let Some((names, s)) = &ms.skills {
            *c.main_skill_minion_skill.list.borrow_mut() = names.iter().map(|n| DropItem::str(n)).collect();
            c.main_skill_minion_skill.sel_index.set(*s as usize);
            c.main_skill_minion_skill.base.shown.set(true);
            c.main_skill_minion_skill.base.enabled.set(names.len() > 1);
        } else {
            c.main_skill_minion_skill.list.borrow_mut().clear();
        }
    }
}

/// `hexToRGB` of a colour code (`^xRRGGBB`).
fn hex_to_rgb(code: &str) -> [f64; 3] {
    let h = code.trim_start_matches("^x");
    let p = |i: usize| f64::from(u8::from_str_radix(&h[i..i + 2], 16).expect("colour code")) / 255.0;
    [p(0), p(2), p(4)]
}

/// `loadout == <the active sets' loadout>`: `Some(true)` when every set id `findSetId` finds is the active one (the selection is a
/// no-op), `Some(false)` when one differs, `None` when one is not found (the selFunc returns, Build.lua:401-403).
fn loadout_is_active(m: &pob_build::BuildModel, value: &str) -> Option<bool> {
    let specs = &m.tree.specs;
    let spec_ids: Vec<String> = specs
        .iter()
        .map(|s| {
            let t = s.title.clone().unwrap_or_else(|| "Default".into());
            spec_list_name(&s.tree_version, &t)
        })
        .collect();
    let link = lua::lmatch(value, "%{(%w+)%}");
    let spec_id = spec_ids.iter().position(|s| s == value).map(|i| i + 1);
    let active_spec = (m.tree.active_spec as usize).clamp(1, specs.len().max(1));
    if link.is_some() {
        // a {link} loadout goes through `loadout_set_ids`
        return Some(false);
    }
    let spec_id = spec_id?;
    let one_item = m.item_sets.len() == 1;
    let one_skill = m.skills.sets.len() <= 1;
    let one_config = m.config.sets.len() <= 1;
    let find = |titles: Vec<(f64, Option<String>)>| titles.iter().find(|(_, t)| t.as_deref().unwrap_or("Default") == value).map(|x| x.0);
    let item_id = if one_item {
        Some(1.0)
    } else {
        find(m.item_sets.iter().map(|s| (s.id, s.title.clone())).collect())
    }?;
    let skill_id = if one_skill {
        Some(1.0)
    } else {
        find(m.skills.sets.iter().map(|s| (s.id, s.title.clone())).collect())
    }?;
    let config_id = if one_config {
        Some(1.0)
    } else {
        find(m.config.sets.iter().map(|s| (s.id, s.title.clone())).collect())
    }?;
    Some(
        spec_id == active_spec
            && (one_item || item_id == m.active_item_set)
            && (one_skill || skill_id == m.skills.active_skill_set)
            && (one_config || config_id == m.config.active_config_set),
    )
}

/// Build.lua:395-398: `(newSpecId, newConfigId)` when this loadout does not change the item or the skill set.
fn loadout_set_ids(m: &pob_build::BuildModel, value: &str) -> Option<(usize, f64, f64, f64)> {
    let link = lua::lmatch(value, "%{(%w+)%}");
    // `setSpecialLinks[linkId].setId` (:811-846), and `setSpecialLinks[linkMatch]["setId"]` of a missing link is a Lua error
    let link_of_sets = |titles: &[(f64, Option<String>)], link: &str| -> f64 {
        for (id, t) in titles {
            let t = t.clone().unwrap_or_else(|| "Default".into());
            if let Some((ids, _)) = link_of(&t)
                && ids.split(',').any(|x| x == link)
            {
                return *id;
            }
        }
        panic!("Modules/Build.lua:368: attempt to index a nil value (field '?')");
    };
    let find_set = |titles: Vec<(f64, Option<String>)>| -> Option<f64> {
        for (id, t) in &titles {
            if t.as_deref().unwrap_or("Default") == value {
                return Some(*id);
            }
            if let Some(l) = &link {
                return Some(link_of_sets(&titles, l));
            }
        }
        None
    };
    let specs: Vec<(f64, Option<String>)> = m.tree.specs.iter().enumerate().map(|(i, s)| ((i + 1) as f64, s.title.clone())).collect();
    let mut spec_id = None;
    for (i, s) in m.tree.specs.iter().enumerate() {
        let t = s.title.clone().unwrap_or_else(|| "Default".into());
        if spec_list_name(&s.tree_version, &t) == value {
            spec_id = Some(i + 1);
            break;
        }
        if let Some(l) = &link {
            spec_id = Some(link_of_sets(&specs, l) as usize);
            break;
        }
    }
    let item_id = if m.item_sets.len() == 1 { 1.0 } else { find_set(m.item_sets.iter().map(|s| (s.id, s.title.clone())).collect())? };
    let skill_id = if m.skills.sets.len() == 1 { 1.0 } else { find_set(m.skills.sets.iter().map(|s| (s.id, s.title.clone())).collect())? };
    let config_id = if m.config.sets.len() == 1 { 1.0 } else { find_set(m.config.sets.iter().map(|s| (s.id, s.title.clone())).collect())? };
    Some((spec_id?, item_id, skill_id, config_id))
}

/// `treeVersions[v].display` with the `[...] ` prefix of a non-latest spec (Build.lua:812).
fn spec_list_name(tree_version: &str, title: &str) -> String {
    let tv = pob_data::tree::tree_versions().expect("GameVersions");
    if tree_version != tv.latest_tree_version {
        let display = tv
            .tree_versions
            .iter()
            .find(|v| v.ver == tree_version)
            .map_or_else(|| tree_version.to_string(), |v| v.display.clone());
        format!("[{display}] {title}")
    } else {
        title.to_string()
    }
}

/// `string.match(title, "%{([%w,]+)%}")` and the set name without it (Build.lua:792-799).
fn link_of(title: &str) -> Option<(String, String)> {
    let link = lua::lmatch(title, "%{([%w,]+)%}")?;
    let name = lua::gsub(&lua::gsub(&lua::gsub(title, &format!("%{{{link}%}}"), ""), "^%s*", ""), "%s*$", "");
    Some((link, if name.is_empty() { "Default".into() } else { name }))
}

/// Build.lua:771-896 `SyncLoadouts` over the model: the list and the index `SetSel` gets.
pub fn sync_loadouts(m: &pob_build::BuildModel) -> (Vec<String>, usize) {
    let mut filtered = vec!["^7^7Loadouts:".to_string()];
    let mut tree_list: Vec<String> = Vec::new();
    let mut sorted_tree_links: Vec<(String, String)> = Vec::new(); // (setName, linkId)
    for s in &m.tree.specs {
        let title = s.title.clone().unwrap_or_else(|| "Default".into());
        match link_of(&title) {
            Some((link, name)) => {
                for id in link.split(',').filter(|x| !x.is_empty()) {
                    sorted_tree_links.push((name.clone(), id.to_string()));
                }
            }
            None => tree_list.push(spec_list_name(&s.tree_version, &title)),
        }
    }
    let identify = |titles: Vec<Option<String>>| {
        let mut names = std::collections::HashSet::new();
        let mut links = std::collections::HashSet::new();
        for t in titles {
            let t = t.unwrap_or_else(|| "Default".into());
            match link_of(&t) {
                Some((link, _)) => {
                    for id in link.split(',').filter(|x| !x.is_empty()) {
                        links.insert(id.to_string());
                    }
                }
                None => {
                    names.insert(t);
                }
            }
        }
        (names, links)
    };
    let (item_list, item_links) = identify(m.item_sets.iter().map(|s| s.title.clone()).collect());
    let (skill_list, skill_links) = identify(m.skills.sets.iter().map(|s| s.title.clone()).collect());
    let (config_list, config_links) = identify(m.config.sets.iter().map(|s| s.title.clone()).collect());
    let one_skill = m.skills.sets.len() == 1;
    let one_item = m.item_sets.len() == 1;
    let one_config = m.config.sets.len() == 1;
    for t in &tree_list {
        if (one_item || item_list.contains(t)) && (one_skill || skill_list.contains(t)) && (one_config || config_list.contains(t)) {
            filtered.push(t.clone());
        }
    }
    for (name, id) in &sorted_tree_links {
        if (one_item || item_links.contains(id)) && (one_skill || skill_links.contains(id)) && (one_config || config_links.contains(id)) {
            filtered.push(format!("{name} {{{id}}}"));
        }
    }
    for s in ["^7^7-----", "^7^7New Loadout", "^7^7Sync", "^7^7Help >>"] {
        filtered.push(s.to_string());
    }
    // :870-892
    let specs = &m.tree.specs;
    if !specs.is_empty() {
        let active = (m.tree.active_spec as usize).clamp(1, specs.len());
        let tree_name = specs[active - 1].title.clone().unwrap_or_else(|| "Default".into());
        if let Some(i) = filtered.iter().position(|l| *l == tree_name) {
            let link = lua::lmatch(&tree_name, "%{(%w+)%}").unwrap_or_else(|| tree_name.clone());
            let title_of = |sets: Vec<(f64, Option<String>)>, active: f64| {
                sets.into_iter()
                    .find(|(id, _)| *id == active)
                    .and_then(|(_, t)| t)
                    .unwrap_or_else(|| "Default".into())
            };
            let skill_name = title_of(
                m.skills.sets.iter().map(|s| (s.id, s.title.clone())).collect(),
                m.skills.active_skill_set,
            );
            let item_name = title_of(m.item_sets.iter().map(|s| (s.id, s.title.clone())).collect(), m.active_item_set);
            let config_name = title_of(
                m.config.sets.iter().map(|s| (s.id, s.title.clone())).collect(),
                m.config.active_config_set,
            );
            if (one_skill || skill_name.contains(&link))
                && (one_item || item_name.contains(&link))
                && (one_config || config_name.contains(&link))
            {
                return (filtered, i + 1);
            }
        }
    }
    (filtered, 1)
}

impl Mode for BuildMode {
    /// `buildMode:Init(dbFileName, buildName, buildXML, convertBuild, importLink)` (Build.lua:64-727).
    fn init(&self, g: &mut Gfx, args: &[ModeArg]) {
        let db = match args.first() {
            Some(ModeArg::Str(p)) => DbFile::Path(p.clone()),
            Some(ModeArg::Bool(false)) => DbFile::False,
            _ => DbFile::Nil,
        };
        let build_name = args.get(1).and_then(ModeArg::as_str).map(str::to_string);
        // Build.lua:89-94 `LoadDB(buildXML, "Unnamed build")`: the build on screen stays until the new one has loaded (the frame goes on over its tabs)
        let build_xml = args.get(2).and_then(ModeArg::as_str).map(str::to_string);
        let imported = match &build_xml {
            None => None,
            Some(x) => {
                let parsed = match pob_build::parse_xml_document(x) {
                    Err(e) => Err(format!("^1{e}")),
                    Ok(doc) => match doc.first() {
                        Some(pob_build::XmlNode::Element(e)) if e.name == "PathOfBuilding" => Engine::load_xml(x).map_err(|e| format!("^1{e}")),
                        _ => Err("^1Error parsing 'Unnamed build': 'PathOfBuilding' root element missing".to_string()),
                    },
                };
                match parsed {
                    Ok(e) => Some(e),
                    Err(msg) => {
                        *self.db_file_name.borrow_mut() = db.clone();
                        *self.build_name.borrow_mut() = build_name.clone();
                        if let Some(e) = self.engine.borrow_mut().as_mut() {
                            e.set_level_of_failed_init(main().s.borrow().default_char_level.unwrap_or(1.0).clamp(1.0, 100.0));
                        }
                        launch().show_err_msg(&msg);
                        self.close_build(g);
                        return;
                    }
                }
            }
        };
        *self.db_file_name.borrow_mut() = db.clone();
        *self.build_name.borrow_mut() = build_name.clone();
        let m = main();
        *self.db_file_sub_path.borrow_mut() = match (&db, &build_name) {
            // `dbFileName:sub(#main.buildPath + 1, -#buildName - 5)`
            (DbFile::Path(p), Some(n)) => Some(lua::sub(p, m.build_path.borrow().len() as i64 + 1, -(n.len() as i64) - 5)),
            (DbFile::Path(p), None) => Some(lua::sub(p, m.build_path.borrow().len() as i64 + 1, -5)),
            _ => Some(m.list_mode.sub_path.borrow().clone()),
        };
        if build_name.is_none() {
            m.set_mode("LIST", Vec::new());
        }
        *self.save_as_sort_mode.borrow_mut() = "NAME".into();
        *self.view_mode.borrow_mut() = "TREE".into();
        *self.engine.borrow_mut() = None;
        *self.tree_view.borrow_mut() = None;
        *self.tree_tab.borrow_mut() = None;
        self.initialised.set(false);
        // LoadDBFile / LoadDB (Build.lua:2222-2275): the XML into the engine (the tabs' Load + BuildOutput + RefreshStatList)
        let xml = match &db {
            _ if imported.is_some() => None,
            DbFile::Path(p) => {
                f_con_printf(g, &format!("Loading '{p}'..."));
                match pob_build::read_build_file(std::path::Path::new(p)) {
                    Ok(t) => Some(t),
                    Err(_) => {
                        *self.db_file_name.borrow_mut() = DbFile::Nil;
                        self.close_build(g);
                        return;
                    }
                }
            }
            // a new build: no sections -- the tabs' defaults; TreeTab's is one empty spec of the latest tree
            // (`new("PassiveSpec"):PassiveSpec(build, latestTreeVersion)`, class 0), which the engine needs as a `<Spec>`
            _ => Some(format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<PathOfBuilding>\n\t<Build level=\"{}\" targetVersion=\"{LIVE_TARGET_VERSION}\" characterLevelAutoMode=\"{}\"/>\n\t<Tree activeSpec=\"1\">\n\t\t<Spec treeVersion=\"{}\" classId=\"0\" ascendClassId=\"0\" secondaryAscendClassId=\"nil\" nodes=\"\"/>\n\t</Tree>\n</PathOfBuilding>",
                tostring_num(m.s.borrow().default_char_level.unwrap_or(1.0).clamp(1.0, 100.0)),
                m.s.borrow().default_char_level.is_none_or(|l| l == 1.0),
                pob_data::tree::tree_versions().expect("GameVersions").latest_tree_version
            )),
        };
        let mut engine = match imported {
            Some(e) => e,
            None => match Engine::load_xml(&xml.unwrap_or_default()) {
                Ok(e) => e,
                Err(e) => {
                    launch().show_err_msg(&format!("^1{e}"));
                    self.close_build(g);
                    return;
                }
            },
        };
        // SkillsTab.lua:96 `self.defaultGemQuality = main.defaultGemQuality`, until a `<Skills>` loads over it (:536)
        if engine.extras().nil_default_gem_quality
            && let Some(q) = m.s.borrow().default_gem_quality
        {
            engine.set_default_gem_quality(Some(q)).unwrap_or_else(|e| panic!("{e}"));
        }
        // Build.lua:67 `self.importLink = importLink`: the link of a build that came from a site wins over the one in its XML (:2247)
        if let Some(link) = args.get(4).and_then(ModeArg::as_str) {
            engine.import_set_build_link(link);
        }
        *self.target_version.borrow_mut() = Some(engine.model().target_version.clone());
        // Build.lua:94 / :100
        self.mod_flag.set(build_xml.is_some());
        // :103-110
        if matches!(args.get(3), Some(ModeArg::Bool(true))) {
            engine.set_target_version(LIVE_TARGET_VERSION);
            *self.target_version.borrow_mut() = Some(LIVE_TARGET_VERSION.to_string());
        }
        if engine.model().target_version != LIVE_TARGET_VERSION {
            *self.target_version.borrow_mut() = None;
            *self.db_file_name.borrow_mut() = db.clone();
            *self.build_name.borrow_mut() = build_name.clone();
            self.open_conversion_popup(g);
            return;
        }
        *self.view_mode.borrow_mut() = engine.model().view_mode.clone().unwrap_or_else(|| "TREE".into());
        crate::tree_view::set_jewel_radii_globally(&engine.spec_rc().tree_version);
        *self.engine.borrow_mut() = Some(engine);
        self.abort_save.set(Some(true));
        self.host.controls.borrow_mut().clear();
        self.host.select_control(g, None);
        *self.secondary_list.borrow_mut() = None;
        let c = self.create_controls(g);
        *self.c.borrow_mut() = Some(c.clone());
        // Build.lua:615-626
        let tab = CalcsTab::new(g, self.me.clone());
        let breakdown = CalcBreakdownControl::new(Rc::downgrade(&tab), BdEnv::Main);
        breakdown
            .pinned_colour
            .set(hex_to_rgb(&crate::colors::color_code("CUSTOM").expect("CUSTOM")));
        let w = self.me.clone();
        *breakdown.clear_display_func.borrow_mut() =
            Some(Rc::new(move |g: &mut Gfx| w.upgrade().expect("buildMode").clear_display_stat(g)));
        self.host.add("breakdown", breakdown.clone());
        *self.calcs_tab.borrow_mut() = Some(tab);
        // Build.lua:608-611
        *self.import_tab.borrow_mut() = Some(crate::tabs::import::ImportTab::new(g, self.me.clone()));
        *self.party_tab.borrow_mut() = Some(crate::tabs::party::PartyTab::new(g, self.me.clone()));
        *self.notes_tab.borrow_mut() = Some(crate::tabs::notes::NotesTab::new(g, self.me.clone()));
        *self.config_tab.borrow_mut() = Some(crate::tabs::config::ConfigTab::new(g, self.me.clone()));
        *self.compare_tab.borrow_mut() = Some(crate::tabs::compare::CompareTab::new(g, self.me.clone()));
        *self.breakdown.borrow_mut() = Some(breakdown);
        let skills = crate::tabs::skills::new_skills_tab(g, self.me.clone());
        *self.skills_tab.borrow_mut() = Some(skills);
        let items = crate::tabs::items::ItemsTab::new(g, self.me.clone());
        *self.items_tab.borrow_mut() = Some(items);
        self.overlay_panes.borrow_mut().clear();
        *self.overlay_breakdown_control.borrow_mut() = None;
        *self.breakdown_inputs.borrow_mut() = None;
        *self.breakdown_index.borrow_mut() = None;
        // `UpdateClassDropdowns()`
        self.fill_class_drop(g);
        // the engine ran BuildOutput + RefreshStatList with the default formatting; RefreshStatList with main's settings
        {
            let mut eng = self.engine.borrow_mut();
            let e = eng.as_mut().expect("engine");
            let ds = Self::display_settings();
            if *e.settings() != ds {
                e.set_display_settings(ds);
                e.refresh_stat_list().unwrap_or_else(|err| panic!("{err}"));
            }
        }
        self.apply_stat_list(g);
        self.build_flag.set(false);
        self.set_window_title_with_build_class(g);
        self.abort_save.set(Some(false));
        self.sync_loadouts(g);
        self.initialised.set(true);
    }

    /// `buildMode:GetArgs()` (Build.lua:967-969).
    fn get_args(&self) -> Vec<ModeArg> {
        let first = match &*self.db_file_name.borrow() {
            DbFile::Path(p) => ModeArg::Str(p.clone()),
            DbFile::False => ModeArg::Bool(false),
            DbFile::Nil => return Vec::new(),
        };
        match self.build_name.borrow().clone() {
            Some(n) => vec![first, ModeArg::Str(n)],
            None => vec![first],
        }
    }

    /// `buildMode:CanExit(mode)` (Build.lua:943-949).
    fn can_exit(&self, g: &mut Gfx, reason: &str) -> Option<bool> {
        if !self.unsaved.get() {
            return Some(true);
        }
        self.open_save_popup(g, reason);
        Some(false)
    }

    /// `buildMode:Shutdown()` (Build.lua:951-965): dev-mode autosave is never active in the port.
    fn shutdown(&self, _g: &mut Gfx) {
        self.abort_save.set(None);
    }

    fn view_mode(&self) -> Option<String> {
        Some(self.view_mode.borrow().clone())
    }

    /// `buildMode:OnFrame(inputEvents)` (Build.lua:1160-1351).
    fn on_frame(&self, f: &mut Frame<'_>, input_events: &mut InputEvents, _view_port: &Rect) {
        if self.target_version.borrow().is_none() {
            main().draw_background(f, &main().view_port.get());
            return;
        }
        if self.abort_save.get() == Some(true) {
            self.close_build(f);
        }
        if !self.initialised.get() {
            return;
        }
        let me = self.rc();
        // `for id, event in ipairs(inputEvents)`: up to the first hole (MINEFIELD U2); `inputEvents[id] = nil` below
        #[allow(clippy::needless_range_loop)]
        for id in 0..input_events.len() {
            let Some(InputEvent::KeyDown { key, .. }) = input_events[id].clone() else {
                if input_events[id].is_none() {
                    break;
                }
                continue;
            };
            if key == "MOUSE4" {
                if self.unsaved.get() {
                    self.open_save_popup(f, "LIST");
                } else {
                    self.close_build(f);
                }
            } else if sg::is_key_down(f, "CTRL") {
                match key.as_str() {
                    "i" => {
                        *self.view_mode.borrow_mut() = "IMPORT".into();
                        let tab = self.import_tab.borrow().clone();
                        tab.expect("importTab").select_import_code(f);
                    }
                    "s" => {
                        self.save_db_file(f);
                        input_events[id] = None;
                    }
                    "w" => {
                        if self.unsaved.get() {
                            self.open_save_popup(f, "LIST");
                        } else {
                            self.close_build(f);
                        }
                    }
                    "1" => *self.view_mode.borrow_mut() = "TREE".into(),
                    "2" => *self.view_mode.borrow_mut() = "SKILLS".into(),
                    "3" => *self.view_mode.borrow_mut() = "ITEMS".into(),
                    "4" => *self.view_mode.borrow_mut() = "CALCS".into(),
                    "5" => *self.view_mode.borrow_mut() = "CONFIG".into(),
                    "6" => *self.view_mode.borrow_mut() = "NOTES".into(),
                    "7" => *self.view_mode.borrow_mut() = "PARTY".into(),
                    _ => {}
                }
            }
        }
        self.overlay_input(f, input_events);
        let vp = main().view_port.get();
        me.host.process_controls_input(f, input_events, &vp);
        self.update_class_dropdowns(f);
        if self.build_flag.get() {
            self.build_flag.set(false);
            let level_moved = {
                let mut eng = self.engine.borrow_mut();
                let e = eng.as_mut().expect("engine");
                let before = e.model().level;
                e.refresh().unwrap_or_else(|err| panic!("{err}"));
                e.model().character_level_auto_mode && e.model().level != before
            };
            // `EstimatePlayerProgress` moved the auto-mode level: its `configTab:BuildModList()` (Build.lua:913) re-applies the
            // level-dependent placeholders with `SetPlaceholder(…, true)` (ConfigOptions.lua:2088-2186), whose changeFunc sets
            // `build.buildFlag = true` (ConfigTab.lua:313): the next frame recalculates at the new level (measured by the oracle).
            if level_moved {
                self.build_flag.set(true);
            }
            // CalcsTab:BuildOutput: `self.powerBuildFlag = true`
            if let Some(t) = self.tree_tab.borrow().as_ref() {
                t.power_build_flag.set(true);
            }
            let tab = self.calcs_tab.borrow().clone().expect("calcsTab");
            tab.after_build_output(f);
            let config = self.config_tab.borrow().clone();
            if let Some(t) = config {
                t.after_recalc(f);
            }
            // :1267-1270
            let inputs = self.breakdown_inputs.borrow().clone();
            if let Some(i) = inputs {
                let bd = self.breakdown.borrow().clone().expect("breakdown");
                if bd.pinned.get() {
                    bd.set_breakdown_data(f, None, false, None);
                    bd.set_breakdown_data(f, i.source.clone().map(Source::Sidebar), i.pin, i.actor);
                }
            }
            self.apply_stat_list(f);
        }
        let ds = Self::display_settings();
        if self.last_display.borrow().as_ref() != Some(&ds) {
            {
                let mut eng = self.engine.borrow_mut();
                let e = eng.as_mut().expect("engine");
                e.set_display_settings(ds);
                e.refresh_stat_list().unwrap_or_else(|err| panic!("{err}"));
            }
            self.apply_stat_list(f);
        }
        if self.last_show_titlebar_name.get() != Some(main().s.borrow().show_titlebar_name) {
            self.set_window_title_with_build_class(f);
        }
        self.refresh_skill_select_controls(f);
        // the tab (TREE U3, SKILLS U5, ITEMS U6, CALCS U4, CONFIG/NOTES U7, IMPORT/PARTY/COMPARE U8)
        if let Some(e) = self.engine.borrow().as_ref() {
            // Calcs.lua:102 `build.viewMode == "TREE"`
            e.view_mode_is_tree.set(self.view_mode.borrow().as_str() == "TREE");
        }
        let side_bar_width = 312.0;
        let (screen_w, screen_h) = crate::common::screen_size();
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        if self.view_mode.borrow().as_str() == "TREE" {
            self.draw_tree_tab(f, side_bar_width, screen_w, screen_h, input_events);
        } else if self.view_mode.borrow().as_str() == "CALCS" {
            let tab = self.calcs_tab.borrow().clone().expect("calcsTab");
            let view_port = Rect {
                x: side_bar_width,
                y: 32.0,
                width: screen_w - side_bar_width,
                height: screen_h - 32.0,
            };
            tab.draw(f, &view_port, input_events);
        } else if self.view_mode.borrow().as_str() == "SKILLS" {
            let tab = self.skills_tab.borrow().clone().expect("skillsTab");
            let view_port = Rect {
                x: side_bar_width,
                y: 32.0,
                width: screen_w - side_bar_width,
                height: screen_h - 32.0,
            };
            tab.draw(f, &view_port, input_events);
        } else if self.view_mode.borrow().as_str() == "ITEMS" {
            let tab = self.items_tab.borrow().clone().expect("itemsTab");
            let view_port = Rect {
                x: side_bar_width,
                y: 32.0,
                width: screen_w - side_bar_width,
                height: screen_h - 32.0,
            };
            tab.draw(f, &view_port, input_events);
        } else if self.view_mode.borrow().as_str() == "COMPARE" {
            let tab = self.compare_tab.borrow().clone().expect("compareTab");
            let view_port = Rect {
                x: side_bar_width,
                y: 32.0,
                width: screen_w - side_bar_width,
                height: screen_h - 32.0,
            };
            tab.draw(f, &view_port, input_events);
        } else if matches!(self.view_mode.borrow().as_str(), "IMPORT" | "PARTY") {
            let view_port = Rect {
                x: side_bar_width,
                y: 32.0,
                width: screen_w - side_bar_width,
                height: screen_h - 32.0,
            };
            if self.view_mode.borrow().as_str() == "IMPORT" {
                let tab = self.import_tab.borrow().clone().expect("importTab");
                tab.draw(f, &view_port, input_events);
            } else {
                let tab = self.party_tab.borrow().clone().expect("partyTab");
                tab.draw(f, &view_port, input_events);
            }
        } else if matches!(self.view_mode.borrow().as_str(), "CONFIG" | "NOTES") {
            let view_port = Rect {
                x: side_bar_width,
                y: 32.0,
                width: screen_w - side_bar_width,
                height: screen_h - 32.0,
            };
            if self.view_mode.borrow().as_str() == "CONFIG" {
                let tab = self.config_tab.borrow().clone().expect("configTab");
                tab.draw(f, &view_port, input_events);
            } else {
                let tab = self.notes_tab.borrow().clone().expect("notesTab");
                tab.draw(f, &view_port, input_events);
            }
        } else {
            sg::draw_string(
                f,
                side_bar_width + 20.0,
                52.0,
                Some("LEFT"),
                16.0,
                "VAR",
                &format!("^8The {} tab is not ported yet.", self.view_mode.borrow().to_lowercase()),
            );
        }
        // :1321-1325
        let panes = self.overlay_panes.borrow().clone();
        for pane in panes {
            if pane.is_overlay.get() {
                pane.draw_overlay(f, &vp);
            }
        }
        let calcs_mod = self.calcs_tab.borrow().as_ref().is_some_and(|t| t.undo.mod_flag.get());
        let notes_mod = self.notes_tab.borrow().as_ref().is_some_and(|t| t.mod_flag.get());
        let party_mod = self.party_tab.borrow().as_ref().is_some_and(|t| t.mod_flag.get());
        let config_mod = self.config_tab.borrow().as_ref().is_some_and(|t| t.undo.mod_flag.get());
        let skills_mod = self.skills_tab.borrow().as_ref().is_some_and(|t| t.undo.mod_flag.get());
        let items_mod = self.items_tab.borrow().as_ref().is_some_and(|t| t.undo.mod_flag.get());
        self.unsaved.set(self.mod_flag.get() || calcs_mod || notes_mod || party_mod || config_mod || skills_mod || items_mod);
        sg::set_draw_layer(f, Some(5), 0);
        sg::set_draw_color(f, 0.2, 0.2, 0.2);
        sg::draw_rect(f, 0.0, 0.0, screen_w, 28.0);
        sg::set_draw_color(f, 0.85, 0.85, 0.85);
        sg::draw_rect(f, 0.0, 28.0, screen_w, 4.0);
        sg::draw_rect(f, screen_w / 2.0 - 2.0, 0.0, 4.0, 28.0);
        sg::set_draw_color(f, 0.1, 0.1, 0.1);
        sg::draw_rect(f, 0.0, 32.0, side_bar_width - 4.0, screen_h - 32.0);
        sg::set_draw_color(f, 0.85, 0.85, 0.85);
        sg::draw_rect(f, side_bar_width - 4.0, 32.0, 4.0, screen_h - 32.0);
        let hovered = self.c().stat_box.hovered_line.get();
        self.set_display_stat(f, hovered, false);
        me.host.draw_controls(f, &vp, None);
        // :1348-1350
        let bd = self.breakdown.borrow().clone().expect("breakdown");
        if bd.pinned.get() && self.breakdown_inputs.borrow().is_some() && me.host.sel().is_none() {
            me.host.select_control(f, Some(bd));
        }
    }
}

impl BuildMode {
    /// Build.lua:1211-1248: the mouse events of the overlay panes and of a pinned breakdown over one.
    fn overlay_input(&self, f: &mut Frame<'_>, input_events: &mut InputEvents) {
        let (cursor_x, cursor_y) = f.get_cursor_pos();
        let tab = self.calcs_tab.borrow().clone().expect("calcsTab");
        let breakdown = tab.breakdown.clone();
        let overlay_breakdown = tab.display_pinned.get() && tab.display_data.borrow().is_some_and(|k| tab.sections[k.0].is_overlay.get());
        for i in (0..input_events.len()).rev() {
            let Some(event) = input_events[i].clone() else { break };
            match event {
                InputEvent::KeyDown { key, double_click } if key.contains("BUTTON") => {
                    let panes = self.overlay_panes.borrow().clone();
                    for pane in panes.iter().rev() {
                        if pane.is_overlay.get() && pane.is_mouse_in_overlay(f, cursor_x, cursor_y) {
                            pane.handle_overlay_click(f, &key, cursor_x, cursor_y);
                            input_events[i] = None;
                            break;
                        }
                    }
                    if input_events[i].is_some() && overlay_breakdown && breakdown.is_mouse_over(f) == Some(true) {
                        *self.overlay_breakdown_control.borrow_mut() = breakdown.clone().on_key_down(f, &key, double_click);
                        input_events[i] = None;
                    }
                }
                InputEvent::KeyUp { key } if key.contains("BUTTON") => {
                    for pane in self.overlay_panes.borrow().iter() {
                        if pane.is_overlay.get() {
                            pane.handle_overlay_release(&key);
                        }
                    }
                    let held = self.overlay_breakdown_control.borrow_mut().take();
                    if let Some(c) = held {
                        c.on_key_up(f, &key);
                        input_events[i] = None;
                    }
                }
                InputEvent::KeyUp { key }
                    if overlay_breakdown
                        && breakdown.is_mouse_over(f) == Some(true)
                        && (crate::controls::scroll_bar::ScrollBarControl::is_scroll_down_key(&key)
                            || crate::controls::scroll_bar::ScrollBarControl::is_scroll_up_key(&key)) =>
                {
                    breakdown.clone().on_key_up(f, &key);
                    input_events[i] = None;
                }
                _ => {}
            }
        }
    }

    /// :1387-1391
    pub fn clear_display_stat(&self, g: &mut Gfx) {
        let bd = self.breakdown.borrow().clone().expect("breakdown");
        bd.set_breakdown_data(g, None, false, None);
        *self.breakdown_inputs.borrow_mut() = None;
    }

    /// :1816-1869 `GetSidebarBreakdown(key, modNames, ignoredSections, actorName)`.
    fn get_sidebar_breakdown(
        &self,
        key: &str,
        mod_names: Option<&'static [&'static str]>,
        ignored: Option<&'static [&'static str]>,
        actor: Option<&str>,
    ) -> Vec<Entry> {
        let source_key = self.engine.borrow().as_ref().expect("engine").breakdown_source(key, actor);
        let index = self
            .breakdown_index
            .borrow_mut()
            .get_or_insert_with(|| Rc::new(breakdown_index()))
            .clone();
        let plain = |k: &str| Entry {
            breakdown: Some(k.to_string()),
            ..Entry::default()
        };
        let Some(entry) = index.get(&source_key) else {
            let mut out = vec![plain(key)];
            if let Some(m) = mod_names {
                out.push(Entry {
                    mod_name: m.iter().map(ToString::to_string).collect(),
                    mod_name_is_list: true,
                    ..Entry::default()
                });
            }
            return out;
        };
        let (s, ss, r, c) = entry.col;
        let mut display: Vec<Entry> = Vec::new();
        for cell in &sections()[s].sub_sections[ss].rows[r].cells[c].entries {
            let mut d = cell.clone();
            if d.breakdown.as_deref() == Some(source_key.as_str()) {
                d.breakdown = Some(key.to_string());
            }
            display.push(d);
        }
        if let Some(m) = mod_names {
            for k in m {
                if let Some(list) = entry.mod_sections_by_name.get(*k) {
                    display.extend(list.iter().cloned());
                }
            }
        }
        let mut visited: Vec<String> = Vec::new();
        let mut i = display.len();
        while i > 0 {
            i -= 1;
            if let Some(label) = display[i].label.clone() {
                let seen = visited.contains(&label) || ignored.is_some_and(|x| x.contains(&label.as_str()));
                if seen {
                    display.remove(i);
                }
                visited.push(label);
            }
        }
        if display.is_empty() {
            return vec![plain(key)];
        }
        display
    }

    /// :1353-1385 `SetDisplayStat(hovered, pin)`.
    pub fn set_display_stat(&self, g: &mut Gfx, hovered: Option<(usize, f64)>, pin: bool) {
        let bd = self.breakdown.borrow().clone().expect("breakdown");
        let pinned = bd.pinned.get();
        let Some((line, y)) = hovered.filter(|_| pin || !pinned) else {
            if !pinned {
                self.clear_display_stat(g);
            }
            return;
        };
        let entry = {
            let eng = self.engine.borrow();
            eng.as_ref().expect("engine").stat_list().stat_box[line].clone()
        };
        let key = match &entry.breakdown {
            Some(BreakdownKey::Key(k)) => Some(k.clone()),
            _ => None,
        };
        if pin && pinned {
            self.clear_display_stat(g);
            return;
        }
        if pin && !pinned && key.is_none() {
            return;
        }
        // a hovered line without a key: `GetSidebarBreakdown(nil, ...)` -> `{ { breakdown = nil } }`, no sections
        let entries = match &key {
            Some(k) => self.get_sidebar_breakdown(k, entry.mod_names, entry.ignored_sections, entry.actor_name),
            None => {
                let mut out = vec![Entry::default()];
                if let Some(m) = entry.mod_names {
                    out.push(Entry {
                        mod_name: m.iter().map(ToString::to_string).collect(),
                        mod_name_is_list: true,
                        ..Entry::default()
                    });
                }
                out
            }
        };
        let c = self.c();
        let (x, _) = c.stat_box.get_pos(g);
        let (width, _) = c.stat_box.get_size(g);
        let data = SidebarData {
            entries,
            x: x - 1.0,
            y,
            width,
        };
        if let Some(col) = entry.breakdown_colour {
            bd.border_colour.set(col);
        }
        let actor = match entry.actor_name {
            Some("player") => Some(ForceActor::Player),
            Some("minion") => Some(ForceActor::Minion),
            _ => None,
        };
        let mut inputs = BreakdownInputs {
            data: data.clone(),
            pin,
            actor,
            source: None,
        };
        // `tableDeepEquals(breakdownInputs, self.breakdownInputs)`
        if let Some(old) = &*self.breakdown_inputs.borrow()
            && old.data == inputs.data
            && old.pin == pin
            && old.actor == actor
        {
            return;
        }
        let rc = Rc::new(data);
        inputs.source = Some(rc.clone());
        *self.breakdown_inputs.borrow_mut() = Some(inputs);
        bd.set_breakdown_data(g, Some(Source::Sidebar(rc)), pin, actor);
    }

    /// `cursorStat` of the gate: a stat box line whose text has this string.
    pub fn debug_stat_pos(&self, g: &Gfx, text: &str) -> (f64, f64) {
        let c = self.c();
        let (x, y) = c.stat_box.get_pos(g);
        let mut row_y = y - c.stat_box.scroll_bar.offset.get() + 2.0;
        for l in c.stat_box.list.borrow().iter() {
            if l.cols.first().and_then(Option::as_deref).is_some_and(|t| t.contains(text)) {
                return (x + 100.0, row_y + 2.0);
            }
            row_y += l.height;
        }
        panic!("cursorStat {text}");
    }

    /// `treeTab:Draw(tabViewPort, inputEvents)` (Build.lua:1320).
    fn draw_tree_tab(&self, f: &mut Frame<'_>, side_bar_width: f64, screen_w: f64, screen_h: f64, input_events: &mut InputEvents) {
        let view_port = Rect {
            x: side_bar_width,
            y: 32.0,
            width: screen_w - side_bar_width,
            height: screen_h - 32.0,
        };
        let tab = self.tree_tab.borrow().clone();
        let tab = match tab {
            Some(t) => t,
            None => {
                let t = crate::tabs::tree::new_tree_tab(f);
                *self.tree_tab.borrow_mut() = Some(t.clone());
                t.sync_active_spec_quiet();
                t
            }
        };
        tab.draw(f, &view_port, input_events);
    }

    /// `groupList:GetRowIcon(1, idx, group)`.
    pub fn skills_group_icon(&self, idx: usize) -> Option<pob_gfx::ImageHandle> {
        let tab = self.skills_tab.borrow().clone()?;
        let dummy = crate::controls::list::ByRef(Rc::new(()));
        tab.group_list.hooks.get_row_icon(&tab.group_list, 1, idx, &dummy)
    }

    /// `primaryBuild.treeTab`: made at the first draw of the tab in the port.
    pub fn tree_tab_made(&self, g: &mut Gfx) -> Rc<crate::tabs::tree::TreeTab> {
        let tab = self.tree_tab.borrow().clone();
        match tab {
            Some(t) => t,
            None => {
                let t = crate::tabs::tree::new_tree_tab(g);
                *self.tree_tab.borrow_mut() = Some(t.clone());
                t.sync_active_spec_quiet();
                t
            }
        }
    }

    /// What `CompareEntry:LoadFromXML` makes of this build object (CompareEntry.lua:63-186): the engine and `configTab`, the other tabs are read through the engine.
    pub fn init_compare_entry(&self, g: &mut Gfx, xml: &str, label: &str) -> Result<(), String> {
        *self.build_name.borrow_mut() = Some(label.to_string());
        let engine = Engine::load_xml(xml)?;
        // CompareEntry.lua:102-104
        *self.target_version.borrow_mut() = Some(LIVE_TARGET_VERSION.to_string());
        *self.view_mode.borrow_mut() = engine.model().view_mode.clone().unwrap_or_else(|| "TREE".into());
        engine.view_mode_is_tree.set(self.view_mode.borrow().as_str() == "TREE");
        crate::tree_view::set_jewel_radii_globally(&engine.spec_rc().tree_version);
        *self.engine.borrow_mut() = Some(engine);
        // the tabs read the controls of their build; they are never drawn for an entry
        let c = self.create_controls(g);
        *self.c.borrow_mut() = Some(c);
        *self.config_tab.borrow_mut() = Some(crate::tabs::config::ConfigTab::new(g, self.me.clone()));
        *self.items_tab.borrow_mut() = Some(crate::tabs::items::ItemsTab::new(g, self.me.clone()));
        *self.skills_tab.borrow_mut() = Some(crate::tabs::skills::new_skills_tab(g, self.me.clone()));
        self.initialised.set(true);
        Ok(())
    }

    /// `spec:SetWindowTitleWithBuildClass()`.
    pub fn set_title_pub(&self, g: &mut Gfx) {
        self.set_window_title_with_build_class(g);
    }

    /// `build:SyncLoadouts()` for the tab.
    pub fn sync_loadouts_pub(&self, g: &mut Gfx) {
        self.sync_loadouts(g);
    }

    /// What `SetActiveSpec` does on the build (TreeTab.lua:552-556, :583): the window title, the class drop-downs, the loadouts.
    pub fn after_spec_change(&self, g: &mut Gfx) {
        self.set_window_title_with_build_class(g);
        self.fill_class_drop(g);
        self.sync_loadouts(g);
    }

    /// The view's part of `TreeTab:Draw` (:450-451): `viewer.compareSpec`, `viewer:Draw(build, treeViewPort, inputEvents)`.
    pub fn draw_tree_view(
        &self,
        f: &mut Frame<'_>,
        tree_view_port: &Rect,
        input_events: &mut InputEvents,
        compare: Option<Rc<pob_tree::PassiveSpec>>,
    ) {
        let root = launch().cfg.script_path.clone();
        if let Some(v) = self.tree_view.borrow_mut().as_mut() {
            v.compare_spec = compare.clone();
        }
        self.tree_view
            .borrow_mut()
            .get_or_insert_with(|| {
                let mut v = PassiveTreeView::new(&root);
                v.load(&self.engine.borrow().as_ref().expect("engine").extras().tree_view);
                v.compare_spec = compare;
                v
            })
            .draw(
                f,
                &root,
                &self.engine,
                &self.build_flag,
                &self.mod_flag,
                tree_view_port,
                input_events,
            );
        // `PassiveTreeView:Save` (:105-114) reads the live view at `SaveDB`
        let tv = self.tree_view.borrow();
        let v = tv.as_ref().expect("just made");
        let st = pob_engine::save::TreeViewState {
            zoom_level: Some(v.zoom_level),
            zoom_x: Some(v.zoom_x),
            zoom_y: Some(v.zoom_y),
            search_str: v.search_str.clone(),
            show_stat_differences: v.show_stat_differences,
        };
        let mut eng = self.engine.borrow_mut();
        let e = eng.as_mut().expect("engine");
        if e.extras().tree_view != st {
            e.set_tree_view_state(st);
        }
    }

    /// `treeTab.viewer`: made at the first use in the port; the viewer of an entry knows its build.
    pub fn ensure_tree_view(&self) {
        let root = launch().cfg.script_path.clone();
        let mine = self.me.upgrade().is_some_and(|r| Rc::ptr_eq(&r, &main().build_mode));
        let mut tv = self.tree_view.borrow_mut();
        let v = tv.get_or_insert_with(|| {
            let mut v = PassiveTreeView::new(&root);
            v.load(&self.engine.borrow().as_ref().expect("engine").extras().tree_view);
            v
        });
        if !mine {
            v.owner = Some(self.me.clone());
        }
    }

    /// CompareTab.lua:3425-3428.
    pub fn set_tree_cursor_offset(&self, o: (f64, f64)) {
        if let Some(v) = self.tree_view.borrow_mut().as_mut() {
            v.cursor_offset = o;
        }
    }

    /// The current tree's art (the test harness).
    pub fn tree_art(&self) -> Rc<crate::tree_art::TreeArt> {
        let root = launch().cfg.script_path.clone();
        let mut tv = self.tree_view.borrow_mut();
        let view = tv.get_or_insert_with(|| PassiveTreeView::new(&root));
        let spec = self.engine.borrow().as_ref().expect("engine").spec_rc();
        view.art(&root, &spec.tree)
    }

    /// The test harness' waits (as the oracle's): the ASYNC images of the current tree.
    pub fn wait_tree_assets(&self) {
        let root = launch().cfg.script_path.clone();
        let mut tv = self.tree_view.borrow_mut();
        let view = tv.get_or_insert_with(|| PassiveTreeView::new(&root));
        let spec = self.engine.borrow().as_ref().expect("engine").spec_rc();
        for h in &view.art(&root, &spec.tree).async_handles {
            h.wait();
        }
    }
}

fn f_con_printf(g: &mut Gfx, s: &str) {
    g.con_printf(s);
}
