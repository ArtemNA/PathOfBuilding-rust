//! `Classes/CompareTab.lua`: the comparison screen of a second build next to the current one.
//!
//! An entry ([`CompareEntry`], `Classes/CompareEntry.lua`) owns its own [`pob_engine::Engine`]: upstream's `CompareEntry` is a build object with its own tabs over the process-global
//! `GlobalCache`, `data.jewelRadius` and `grantedEffect` tables; the engine has its own session, so what upstream's shared cache leaks between the builds is not reproduced.

mod calcs;
mod config;
mod entry;
mod folder;
mod items;
mod power;
mod skills;
mod summary;
mod tree;

pub use entry::CompareEntry;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_engine::engine::Engine;
use pob_gfx::{Frame, Gfx};

use crate::colors::color_code;
use crate::control::{Control, ControlRef, Rect, anchor, anchor_collapse, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::scroll_bar::ScrollBarControl;
use crate::host::{ControlHost, InputEvent, InputEvents, ipairs};
use crate::modules::build::{BuildMode, SkillControls, apply_skill_select};
use crate::modules::main::{BareControl, main};
use crate::sg;
use crate::tooltip::Tooltip;
use crate::undo::UndoHost;

/// CompareTab.lua:50-98 `LAYOUT`.
#[allow(dead_code)] // the sub-tabs not ported yet read them
pub(crate) mod layout {
    pub const CONTROL_BAR_HEIGHT: f64 = 126.0;
    pub const TREE_HEADER_HEIGHT: f64 = 74.0;
    pub const TREE_FOOTER_HEIGHT: f64 = 30.0;
    pub const TREE_OVERLAY_CHECK_X: f64 = 155.0;
    pub const SUMMARY_COL1: f64 = 10.0;
    pub const SUMMARY_COL2_RIGHT: f64 = 440.0;
    pub const SUMMARY_COL3_RIGHT: f64 = 580.0;
    pub const SUMMARY_HEADER_HEIGHT: f64 = 38.0;
    pub const ITEMS_CHECKBOX_OFFSET: f64 = 78.0;
    pub const ITEMS_H_SCROLL_BAR_HEIGHT: f64 = 16.0;
    pub const SKILLS_HEADER_HEIGHT: f64 = 24.0;
    pub const SKILLS_H_SCROLL_BAR_HEIGHT: f64 = 16.0;
    pub const POWER_REPORT_LEFT: f64 = 10.0;
    pub const CONFIG_FIXED_HEADER_HEIGHT: f64 = 82.0;
}
use layout::*;

/// `dropDownControl.list` entries of `calcsBuffModeDropList` (CompareTab.lua:488-493).
const BUFF_MODES: [(&str, &str); 4] = [
    ("Unbuffed", "UNBUFFED"),
    ("Buffed", "BUFFED"),
    ("In Combat", "COMBAT"),
    ("Effective DPS", "EFFECTIVE"),
];

/// One side's calcs skill controls (`primCalcs*` / `cmpCalcs*`, CompareTab.lua:495-723).
pub struct CalcsSkillControls {
    pub socket_group: Rc<DropDownControl>,
    pub main_skill: Rc<DropDownControl>,
    pub skill_part: Rc<DropDownControl>,
    pub stage_count: Rc<EditControl>,
    pub mine_count: Rc<EditControl>,
    pub show_minion: Rc<CheckBoxControl>,
    pub minion: Rc<DropDownControl>,
    pub minion_skill: Rc<DropDownControl>,
    pub mode: Rc<DropDownControl>,
}

pub struct CompareTab {
    pub anchor: Rc<BareControl>,
    pub host: ControlHost,
    pub primary: Weak<BuildMode>,
    me: Weak<CompareTab>,
    /// `self.compareEntries`.
    pub entries: RefCell<Vec<Rc<CompareEntry>>>,
    /// `self.activeCompareIndex`.
    pub active: Cell<usize>,
    /// `self.compareViewMode`.
    pub view_mode: RefCell<&'static str>,
    pub scroll_y: Cell<f64>,
    pub summary_total_content_height: Cell<f64>,
    pub items_total_content_height: Cell<f64>,
    pub skills_total_content_height: Cell<f64>,
    pub config_total_content_height: Cell<f64>,
    pub tree_search_needs_sync: Cell<bool>,
    pub tree_overlay_mode: Cell<bool>,
    pub items_expanded_mode: Cell<bool>,
    pub calcs_show_only_differences: Cell<bool>,
    pub config_needs_rebuild: Cell<bool>,
    pub config_toggle: Cell<bool>,
    /// `self.configControls`: var, info.
    config_controls: RefCell<Vec<(String, Rc<config::ConfigInfo>)>>,
    /// `self.configSections`.
    config_sections: RefCell<Vec<config::ConfigSectionInfo>>,
    /// `self.configSectionLayout`.
    config_section_layout: RefCell<Vec<config::LaidSection>>,
    /// `self.configCompareId`.
    config_compare_id: Cell<Option<usize>>,
    /// `self.calcsSkillHeaderHeight`.
    calcs_skill_header_height: Cell<f64>,
    /// The `outputRevision` of the primary build at the last draw.
    primary_revision: Cell<u64>,
    /// `self.itemsColWidth`.
    pub items_col_width: Cell<Option<f64>>,
    pub item_tooltip: RefCell<Tooltip>,
    pub calcs_tooltip: RefCell<Tooltip>,
    pub c: RefCell<Option<Rc<Controls>>>,
    /// The `PowerStat` of `comparePowerStat`.
    pub compare_power_stat: RefCell<Option<pob_calc::power::PowerStat>>,
    pub compare_power_dirty: Cell<bool>,
    /// `comparePowerCompareId`.
    compare_power_id: RefCell<Option<Rc<CompareEntry>>>,
    /// `comparePowerResults`.
    compare_power_results: RefCell<Option<Vec<power::PowerRowRef>>>,
    compare_power_progress: Cell<f64>,
    compare_power_list_synced: Cell<bool>,
    /// `comparePowerCoroutine`.
    compare_power_run: RefCell<Option<power::PowerRun>>,
    /// The `listHost` of the open "Import from Folder" popup.
    folder_host: RefCell<Weak<folder::FolderHost>>,
}

/// `self.controls` of the top bar and the shared scroll bars.
pub struct Controls {
    pub sub_tab_anchor: Rc<BareControl>,
    pub sub_tabs: Vec<Rc<ButtonControl>>,
    pub compare_build_label: Rc<LabelControl>,
    pub compare_build_select: Rc<DropDownControl>,
    pub import_btn: Rc<ButtonControl>,
    pub reimport_btn: Rc<ButtonControl>,
    pub remove_btn: Rc<ButtonControl>,
    pub compare_spec_select: Rc<DropDownControl>,
    pub compare_skill_set_select: Rc<DropDownControl>,
    pub compare_item_set_select: Rc<DropDownControl>,
    pub compare_config_set_select: Rc<DropDownControl>,
    pub cmp_socket_group: Rc<DropDownControl>,
    pub cmp_main_skill: Rc<DropDownControl>,
    pub cmp_skill_part: Rc<DropDownControl>,
    pub cmp_stage_count: Rc<EditControl>,
    pub cmp_mine_count: Rc<EditControl>,
    pub cmp_minion: Rc<DropDownControl>,
    pub cmp_minion_skill: Rc<DropDownControl>,
    /// `{ shown = false }` (CompareTab.lua:2255).
    pub cmp_minion_library: Rc<ButtonControl>,
    pub prim_calcs: CalcsSkillControls,
    pub cmp_calcs: CalcsSkillControls,
    pub tree: tree::TreeControls,
    pub calcs_show_only_differences_check: Rc<CheckBoxControl>,
    pub copy_config_btn: Rc<ButtonControl>,
    pub config_toggle_btn: Rc<ButtonControl>,
    pub config_search_edit: Rc<EditControl>,
    pub config_primary_set_label: Rc<LabelControl>,
    pub config_primary_set_select: Rc<DropDownControl>,
    pub items_expanded_check: Rc<CheckBoxControl>,
    pub primary_item_set_label: Rc<LabelControl>,
    pub primary_item_set_select: Rc<DropDownControl>,
    pub compare_item_set_label2: Rc<LabelControl>,
    pub compare_item_set_select2: Rc<DropDownControl>,
    pub primary_tree_set_label: Rc<LabelControl>,
    pub primary_tree_set_select: Rc<DropDownControl>,
    pub compare_tree_set_label: Rc<LabelControl>,
    pub compare_tree_set_select: Rc<DropDownControl>,
    pub power_stat_select: Rc<DropDownControl>,
    pub power_tree_check: Rc<CheckBoxControl>,
    pub power_items_check: Rc<CheckBoxControl>,
    pub power_gems_check: Rc<CheckBoxControl>,
    pub power_support_gems_check: Rc<CheckBoxControl>,
    pub power_config_check: Rc<CheckBoxControl>,
    pub power_report_list: Rc<crate::controls::list::ListControl<power::PowerRowRef>>,
    pub power_status: Rc<crate::modules::build::DrawControl>,
    pub power_list_state: Rc<power::PowerListState>,
    pub view_scroll_bar: Rc<ScrollBarControl>,
    pub calcs_scroll_bar: Rc<ScrollBarControl>,
    pub items_h_scroll_bar: Rc<ScrollBarControl>,
    pub skills_h_scroll_bar: Rc<ScrollBarControl>,
}

type SelFn = Rc<dyn Fn(&mut Gfx, usize, &DropItem)>;

/// `buildSites.websiteList`: `label`, `matchURL` (Modules/BuildSiteTools.lua:19-41).
const BUILD_SITES: [(&str, &str); 8] = [
    ("Maxroll", "^https://maxroll%.gg/poe/pob/.*"),
    ("pob.codes", "^https://pob%.codes/b/.+"),
    ("pobb.in", "^https://pobb%.in/.+"),
    ("PoeNinja", "^https://poe%.ninja/?p?o?e?1?/pob/%w+"),
    ("Pastebin.com", "^https://pastebin%.com/%w+"),
    ("PastebinP.com", "^https://pastebinp%.com/%w+"),
    ("Rentry.co", "^https://rentry%.co/%w+"),
    ("poedb.tw", "^https://poedb%.tw/.+"),
];

fn items(list: &[String]) -> Vec<DropItem> {
    list.iter().map(|s| DropItem::str(s)).collect()
}

/// `control:SelByValue(buffMode, "buffMode")`.
#[allow(dead_code)] // RefreshCalcsSkillControls (:2277), not ported yet
fn sel_buff_mode(c: &DropDownControl, v: Option<&str>) {
    if let Some(i) = BUFF_MODES.iter().position(|(_, m)| Some(*m) == v) {
        c.sel_index.set(i + 1);
    }
}

impl CompareTab {
    /// CompareTab.lua:129-198.
    pub fn new(g: &mut Gfx, primary: Weak<BuildMode>) -> Rc<Self> {
        let tab = Rc::new_cyclic(|me: &Weak<Self>| Self {
            anchor: BareControl::new(None, rect(0.0, 0.0, 0.0, 0.0)),
            host: ControlHost::new(),
            primary,
            me: me.clone(),
            entries: RefCell::new(Vec::new()),
            active: Cell::new(0),
            view_mode: RefCell::new("SUMMARY"),
            scroll_y: Cell::new(0.0),
            summary_total_content_height: Cell::new(0.0),
            items_total_content_height: Cell::new(0.0),
            skills_total_content_height: Cell::new(0.0),
            config_total_content_height: Cell::new(0.0),
            tree_search_needs_sync: Cell::new(true),
            tree_overlay_mode: Cell::new(true),
            items_expanded_mode: Cell::new(false),
            calcs_show_only_differences: Cell::new(true),
            config_needs_rebuild: Cell::new(true),
            config_toggle: Cell::new(false),
            config_controls: RefCell::new(Vec::new()),
            config_sections: RefCell::new(Vec::new()),
            config_section_layout: RefCell::new(Vec::new()),
            config_compare_id: Cell::new(None),
            calcs_skill_header_height: Cell::new(0.0),
            primary_revision: Cell::new(0),
            items_col_width: Cell::new(None),
            item_tooltip: RefCell::new(Tooltip::new()),
            calcs_tooltip: RefCell::new(Tooltip::new()),
            c: RefCell::new(None),
            compare_power_stat: RefCell::new(None),
            compare_power_dirty: Cell::new(false),
            compare_power_id: RefCell::new(None),
            compare_power_results: RefCell::new(None),
            compare_power_progress: Cell::new(0.0),
            compare_power_list_synced: Cell::new(false),
            compare_power_run: RefCell::new(None),
            folder_host: RefCell::new(Weak::new()),
        });
        let c = tab.init_controls(g);
        *tab.c.borrow_mut() = Some(c);
        tab
    }

    pub fn primary(&self) -> Rc<BuildMode> {
        self.primary.upgrade().expect("primaryBuild")
    }

    pub fn c(&self) -> Rc<Controls> {
        self.c.borrow().clone().expect("compare controls")
    }

    /// `self:GetActiveCompare()` (:1328-1333).
    pub fn active_compare(&self) -> Option<Rc<CompareEntry>> {
        let a = self.active.get();
        let e = self.entries.borrow();
        (a > 0 && a <= e.len()).then(|| e[a - 1].clone())
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("compareTab")
    }

    /// `InitControls` (:200-1071).
    fn init_controls(&self, g: &mut Gfx) -> Rc<Controls> {
        let w = self.me.clone();
        let t = move || w.upgrade().expect("compareTab");
        let has_entries = {
            let t = t.clone();
            move |_: &Gfx| !t().entries.borrow().is_empty()
        };
        let active_entry = {
            let t = t.clone();
            move || t().active_compare()
        };
        // :202-230
        let sub_tab_anchor = BareControl::new(None, rect(0.0, 0.0, 0.0, 20.0));
        let sa: ControlRef = sub_tab_anchor.clone();
        self.host.add("subTabAnchor", sa.clone());
        let mut sub_tabs: Vec<Rc<ButtonControl>> = Vec::new();
        for (i, (name, mode)) in [
            ("Summary", "SUMMARY"),
            ("Tree", "TREE"),
            ("Skills", "SKILLS"),
            ("Items", "ITEMS"),
            ("Calcs", "CALCS"),
            ("Config", "CONFIG"),
        ]
        .into_iter()
        .enumerate()
        {
            let a = match sub_tabs.last() {
                None => anchor("TOPLEFT", &sa, "TOPLEFT"),
                Some(p) => {
                    let pr: ControlRef = p.clone();
                    anchor("LEFT", &pr, "RIGHT")
                }
            };
            let tt = t.clone();
            let b = ButtonControl::new(
                a,
                rect(if i == 0 { 0.0 } else { 4.0 }, 0.0, 72.0, 20.0),
                name.to_string(),
                move |_g| {
                    let s = tt();
                    *s.view_mode.borrow_mut() = mode;
                    s.scroll_y.set(0.0);
                    if mode == "TREE" {
                        s.tree_search_needs_sync.set(true);
                    }
                },
            );
            b.base.shown.set_fn(has_entries.clone());
            let tt = t.clone();
            b.locked.set_fn(move |_| *tt().view_mode.borrow() == mode);
            self.host.add(&format!("subTab{name}"), b.clone());
            sub_tabs.push(b);
        }
        // :233-286
        let compare_build_label = LabelControl::new(
            anchor("TOPLEFT", &sa, "TOPLEFT"),
            rect(0.0, -88.0, 0.0, 16.0),
            "^7Compare with:".to_string(),
        );
        let cbl: ControlRef = compare_build_label.clone();
        let tt = t.clone();
        let compare_build_select = DropDownControl::new(
            g,
            anchor("LEFT", &cbl, "RIGHT"),
            rect(4.0, 0.0, 250.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |_g: &mut Gfx, index: usize, _v: &DropItem| {
                let s = tt();
                if index > 0 && index <= s.entries.borrow().len() {
                    s.active.set(index);
                    s.tree_search_needs_sync.set(true);
                }
            })),
            None,
            false,
        );
        compare_build_select.base.enabled.set_fn(has_entries.clone());
        let cbs: ControlRef = compare_build_select.clone();
        let tt = t.clone();
        let import_btn = ButtonControl::new(
            anchor("LEFT", &cbs, "RIGHT"),
            rect(8.0, 0.0, 100.0, 20.0),
            "Import...".to_string(),
            move |g| tt().open_import_popup(g),
        );
        let ib: ControlRef = import_btn.clone();
        let reimport_btn = ButtonControl::new(
            anchor("LEFT", &ib, "RIGHT"),
            rect(4.0, 0.0, 140.0, 20.0),
            "Re-import Current".to_string(),
            // :250-252 ReimportPrimary, the body in `ImportTab::reimport_character`
            {
                let tt = t.clone();
                move |g| {
                    if let Some(tab) = tt().primary().import_tab.borrow().clone() {
                        tab.reimport_character(g);
                    }
                }
            },
        );
        // :253-270
        let tt = t.clone();
        reimport_btn.tip.set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, _: ()| {
            if let Some(tab) = tt().primary().import_tab.borrow().as_ref() {
                tab.reimport_tooltip(tooltip, g);
            }
        });
        // :271-276
        let tt = t.clone();
        reimport_btn.base.enabled.set_fn(move |_| {
            tt().primary()
                .import_tab
                .borrow()
                .as_ref()
                .is_some_and(|tab| tab.reimport_enabled())
        });
        let rb: ControlRef = reimport_btn.clone();
        let tt = t.clone();
        let remove_btn = ButtonControl::new(
            anchor("LEFT", &rb, "RIGHT"),
            rect(4.0, 0.0, 70.0, 20.0),
            "Remove".to_string(),
            move |g| {
                let s = tt();
                let a = s.active.get();
                if a > 0 && a <= s.entries.borrow().len() {
                    s.remove_build(g, a);
                }
            },
        );
        remove_btn.base.enabled.set_fn(has_entries.clone());
        self.host.add("compareBuildLabel", compare_build_label.clone());
        self.host.add("compareBuildSelect", compare_build_select.clone());
        self.host.add("importBtn", import_btn.clone());
        self.host.add("reimportBtn", reimport_btn.clone());
        self.host.add("removeBtn", remove_btn.clone());
        // :291-347 the set selectors
        let compare_spec_label = LabelControl::new(
            anchor("TOPLEFT", &sa, "TOPLEFT"),
            rect(0.0, -54.0, 0.0, 16.0),
            "^7Tree set:".to_string(),
        );
        compare_spec_label.base.shown.set_fn(has_entries.clone());
        let l: ControlRef = compare_spec_label.clone();
        let ae = active_entry.clone();
        let compare_spec_select = DropDownControl::new(
            g,
            anchor("LEFT", &l, "RIGHT"),
            rect(2.0, 0.0, 150.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                if let Some(e) = ae() {
                    e.set_active_spec(g, index);
                }
            })),
            None,
            false,
        );
        compare_spec_select.base.enabled.set_fn(has_entries.clone());
        compare_spec_select.max_dropped_width.set(500.0);
        compare_spec_select.enable_dropped_width.set(true);
        let l2: ControlRef = compare_spec_select.clone();
        let compare_skill_set_label =
            LabelControl::new(anchor("LEFT", &l2, "RIGHT"), rect(8.0, 0.0, 0.0, 16.0), "^7Skill set:".to_string());
        compare_skill_set_label.base.shown.set_fn(has_entries.clone());
        let l: ControlRef = compare_skill_set_label.clone();
        let ae = active_entry.clone();
        let compare_skill_set_select = DropDownControl::new(
            g,
            anchor("LEFT", &l, "RIGHT"),
            rect(2.0, 0.0, 150.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                if let Some(e) = ae() {
                    e.set_active_skill_set_index(g, index);
                }
            })),
            None,
            false,
        );
        compare_skill_set_select.base.enabled.set_fn(has_entries.clone());
        let l2: ControlRef = compare_skill_set_select.clone();
        let compare_item_set_label = LabelControl::new(anchor("LEFT", &l2, "RIGHT"), rect(8.0, 0.0, 0.0, 16.0), "^7Item set:".to_string());
        compare_item_set_label.base.shown.set_fn(has_entries.clone());
        let l: ControlRef = compare_item_set_label.clone();
        let ae = active_entry.clone();
        let compare_item_set_select = DropDownControl::new(
            g,
            anchor("LEFT", &l, "RIGHT"),
            rect(2.0, 0.0, 150.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                if let Some(e) = ae() {
                    e.set_active_item_set_index(g, index);
                }
            })),
            None,
            false,
        );
        compare_item_set_select.base.enabled.set_fn(has_entries.clone());
        let l2: ControlRef = compare_item_set_select.clone();
        let compare_config_set_label =
            LabelControl::new(anchor("LEFT", &l2, "RIGHT"), rect(8.0, 0.0, 0.0, 16.0), "^7Config set:".to_string());
        compare_config_set_label.base.shown.set_fn(has_entries.clone());
        let l: ControlRef = compare_config_set_label.clone();
        let ae = active_entry.clone();
        let tt = t.clone();
        let compare_config_set_select = DropDownControl::new(
            g,
            anchor("LEFT", &l, "RIGHT"),
            rect(2.0, 0.0, 150.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                if let Some(e) = ae()
                    && e.set_active_config_set_index(g, index)
                {
                    tt().config_needs_rebuild.set(true);
                }
            })),
            None,
            false,
        );
        compare_config_set_select.base.enabled.set_fn(has_entries.clone());
        compare_config_set_select.enable_dropped_width.set(true);
        self.host.add("compareSpecLabel", compare_spec_label);
        self.host.add("compareSpecSelect", compare_spec_select.clone());
        self.host.add("compareSkillSetLabel", compare_skill_set_label);
        self.host.add("compareSkillSetSelect", compare_skill_set_select.clone());
        self.host.add("compareItemSetLabel", compare_item_set_label);
        self.host.add("compareItemSetSelect", compare_item_set_select.clone());
        self.host.add("compareConfigSetLabel", compare_config_set_label);
        self.host.add("compareConfigSetSelect", compare_config_set_select.clone());
        // :352-483 the main skill of the comparison build
        let cmp_skill_label = LabelControl::new(
            anchor_collapse("TOPLEFT", &sa, "TOPLEFT"),
            rect(0.0, -32.0, 0.0, 16.0),
            "^7Skill:".to_string(),
        );
        cmp_skill_label.base.shown.set_fn(has_entries.clone());
        let l: ControlRef = cmp_skill_label.clone();
        let edit = |f: fn(&mut Engine, usize, &DropItem) -> Result<(), String>| -> Option<SelFn> {
            let ae = active_entry.clone();
            Some(Rc::new(move |_g: &mut Gfx, index: usize, v: &DropItem| {
                if let Some(entry) = ae() {
                    f(entry.b.engine.borrow_mut().as_mut().expect("engine"), index, v).unwrap_or_else(|e| panic!("{e}"));
                    entry.b.build_flag.set(true);
                }
            }))
        };
        let cmp_socket_group = DropDownControl::new(
            g,
            anchor_collapse("LEFT", &l, "RIGHT"),
            rect(4.0, 0.0, 200.0, 20.0),
            Vec::new(),
            edit(|e, i, _| {
                e.select_main_socket_group(i as f64);
                Ok(())
            }),
            None,
            false,
        );
        cmp_socket_group.base.shown.set_fn(has_entries.clone());
        cmp_socket_group.max_dropped_width.set(500.0);
        cmp_socket_group.enable_dropped_width.set(true);
        let l: ControlRef = cmp_socket_group.clone();
        let cmp_main_skill = DropDownControl::new(
            g,
            anchor_collapse("LEFT", &l, "RIGHT"),
            rect(4.0, 0.0, 225.0, 20.0),
            Vec::new(),
            edit(|e, i, _| e.set_main_active_skill(i as f64)),
            None,
            false,
        );
        cmp_main_skill.base.shown.set(false);
        let l: ControlRef = cmp_main_skill.clone();
        let cmp_skill_part = DropDownControl::new(
            g,
            anchor_collapse("LEFT", &l, "RIGHT"),
            rect(4.0, 0.0, 200.0, 20.0),
            Vec::new(),
            edit(|e, i, _| e.set_skill_part(i as f64)),
            None,
            false,
        );
        cmp_skill_part.base.shown.set(false);
        let l: ControlRef = cmp_skill_part.clone();
        let cmp_stage_count_label = LabelControl::new(
            anchor_collapse("LEFT", &l, "RIGHT"),
            rect(6.0, 0.0, 0.0, 16.0),
            "^7Stages:".to_string(),
        );
        let l: ControlRef = cmp_stage_count_label.clone();
        let num_edit =
            |g: &Gfx, a, r, f: fn(&mut Engine, Option<f64>) -> Result<(), String>, ae: Rc<dyn Fn() -> Option<Rc<CompareEntry>>>| {
                EditControl::new(
                    g,
                    a,
                    r,
                    EditArgs {
                        init: Some(String::new()),
                        filter: Some("%D".into()),
                        limit: Some(5),
                        change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                            if let Some(entry) = ae() {
                                f(entry.b.engine.borrow_mut().as_mut().expect("engine"), buf.parse::<f64>().ok())
                                    .unwrap_or_else(|e| panic!("{e}"));
                                entry.b.build_flag.set(true);
                            }
                        })),
                        ..EditArgs::default()
                    },
                )
            };
        let ae_rc: Rc<dyn Fn() -> Option<Rc<CompareEntry>>> = Rc::new(active_entry.clone());
        let cmp_stage_count = num_edit(
            g,
            anchor_collapse("LEFT", &l, "RIGHT"),
            rect(4.0, 0.0, 52.0, 20.0),
            |e, n| e.set_skill_stage_count(n),
            ae_rc.clone(),
        );
        cmp_stage_count.base.shown.set(false);
        let sc = cmp_stage_count.clone();
        cmp_stage_count_label.base.shown.set_fn(move |g| sc.is_shown(g));
        let l: ControlRef = cmp_stage_count.clone();
        let cmp_mine_count_label = LabelControl::new(
            anchor_collapse("LEFT", &l, "RIGHT"),
            rect(6.0, 0.0, 0.0, 16.0),
            "^7Mines:".to_string(),
        );
        let l: ControlRef = cmp_mine_count_label.clone();
        let cmp_mine_count = num_edit(
            g,
            anchor_collapse("LEFT", &l, "RIGHT"),
            rect(4.0, 0.0, 52.0, 20.0),
            |e, n| e.set_skill_mine_count(n),
            ae_rc.clone(),
        );
        cmp_mine_count.base.shown.set(false);
        let sc = cmp_mine_count.clone();
        cmp_mine_count_label.base.shown.set_fn(move |g| sc.is_shown(g));
        let l: ControlRef = cmp_mine_count.clone();
        let cmp_minion = DropDownControl::new(
            g,
            anchor_collapse("LEFT", &l, "RIGHT"),
            rect(6.0, 0.0, 140.0, 20.0),
            Vec::new(),
            edit(|e, _, v| {
                if let Some(m) = v.data.as_ref().and_then(|d| d.downcast_ref::<pob_engine::shell::MinionEntry>()) {
                    e.set_skill_minion(m)?;
                }
                Ok(())
            }),
            None,
            false,
        );
        cmp_minion.base.shown.set(false);
        let l: ControlRef = cmp_minion.clone();
        let cmp_minion_skill = DropDownControl::new(
            g,
            anchor_collapse("LEFT", &l, "RIGHT"),
            rect(4.0, 0.0, 140.0, 20.0),
            Vec::new(),
            edit(|e, i, _| e.set_skill_minion_skill(i as f64)),
            None,
            false,
        );
        cmp_minion_skill.base.shown.set(false);
        self.host.add("cmpSkillLabel", cmp_skill_label);
        self.host.add("cmpSocketGroup", cmp_socket_group.clone());
        self.host.add("cmpMainSkill", cmp_main_skill.clone());
        self.host.add("cmpSkillPart", cmp_skill_part.clone());
        self.host.add("cmpStageCountLabel", cmp_stage_count_label);
        self.host.add("cmpStageCount", cmp_stage_count.clone());
        self.host.add("cmpMineCountLabel", cmp_mine_count_label);
        self.host.add("cmpMineCount", cmp_mine_count.clone());
        self.host.add("cmpMinion", cmp_minion.clone());
        self.host.add("cmpMinionSkill", cmp_minion_skill.clone());
        let cmp_minion_library = ButtonControl::new(None, rect(0.0, 0.0, 0.0, 0.0), String::new(), |_| {});
        // :495-723
        let prim_calcs = self.calcs_skill_controls(g, "prim");
        let cmp_calcs = self.calcs_skill_controls(g, "cmp");
        let tree_controls = self.init_tree_controls(g);
        // :725-730
        let tt = t.clone();
        let calcs_show_only_differences_check = CheckBoxControl::new(
            g,
            None,
            [0.0, 0.0, 18.0],
            Some("Show only differences"),
            Some(Rc::new(move |_g: &mut Gfx, state: bool| {
                tt().calcs_show_only_differences.set(state)
            })),
            Some("Show only rows that differ between both builds. Disable to include unchanged rows."),
            false,
        );
        let tt = t.clone();
        calcs_show_only_differences_check.base.shown.set_fn(move |_| {
            let s = tt();
            *s.view_mode.borrow() == "CALCS" && s.active_compare().is_some()
        });
        self.host
            .add("calcsShowOnlyDifferencesCheck", calcs_show_only_differences_check.clone());
        // :921-964 the config view
        let tt = t.clone();
        let config_shown = move |_: &Gfx| {
            let s = tt();
            *s.view_mode.borrow() == "CONFIG" && s.active_compare().is_some()
        };
        let tt = t.clone();
        let copy_config_btn = ButtonControl::new(
            None,
            rect(0.0, 0.0, 240.0, 20.0),
            "Copy Config from Compare Build".to_string(),
            move |g| {
                tt().copy_compare_config(g);
            },
        );
        copy_config_btn.base.shown.set_fn(config_shown.clone());
        let tt = t.clone();
        let config_toggle_btn = ButtonControl::new(None, rect(0.0, 0.0, 240.0, 20.0), String::new(), move |_g| {
            let s = tt();
            s.config_toggle.set(!s.config_toggle.get());
        });
        let tt = t.clone();
        config_toggle_btn.label.set_fn(move |_| {
            if tt().config_toggle.get() {
                "Hide Ineligible Configurations"
            } else {
                "Show All Configurations"
            }
            .to_string()
        });
        config_toggle_btn.base.shown.set_fn(config_shown.clone());
        let config_search_edit = EditControl::new(
            g,
            None,
            rect(0.0, 0.0, 240.0, 20.0),
            EditArgs {
                init: Some(String::new()),
                prompt: Some("Search".into()),
                filter: Some("%c".into()),
                limit: Some(100),
                clearable: true,
                ..EditArgs::default()
            },
        );
        config_search_edit.base.shown.set_fn(config_shown.clone());
        let config_primary_set_label = LabelControl::new(None, rect(0.0, 0.0, 0.0, 16.0), "^7Config set:".to_string());
        config_primary_set_label.base.shown.set_fn(config_shown.clone());
        let tt = t.clone();
        let config_primary_set_select = DropDownControl::new(
            g,
            None,
            rect(0.0, 0.0, 150.0, 20.0),
            Vec::new(),
            Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let s = tt();
                let p = s.primary();
                let id = p
                    .engine
                    .borrow()
                    .as_ref()
                    .expect("engine")
                    .config_set_order()
                    .get(index.wrapping_sub(1))
                    .copied();
                if let Some(id) = id {
                    p.config_tab.borrow().clone().expect("configTab").set_active_config_set(g, id);
                    s.config_needs_rebuild.set(true);
                }
            })),
            None,
            false,
        );
        config_primary_set_select.base.shown.set_fn(config_shown);
        config_primary_set_select.enable_dropped_width.set(true);
        let tt = t.clone();
        config_primary_set_select
            .base
            .enabled
            .set_fn(move |_| tt().primary().engine.borrow().as_ref().expect("engine").config_set_order().len() > 1);
        self.host.add("copyConfigBtn", copy_config_btn.clone());
        self.host.add("configToggleBtn", config_toggle_btn.clone());
        self.host.add("configSearchEdit", config_search_edit.clone());
        self.host.add("configPrimarySetLabel", config_primary_set_label.clone());
        self.host.add("configPrimarySetSelect", config_primary_set_select.clone());
        // :772-836 the items view
        let tt = t.clone();
        let items_shown = move |_: &Gfx| {
            let s = tt();
            *s.view_mode.borrow() == "ITEMS" && s.active_compare().is_some()
        };
        let tt = t.clone();
        let items_expanded_check = CheckBoxControl::new(
            g,
            None,
            [0.0, 0.0, 20.0],
            Some("Expanded mode"),
            Some(Rc::new(move |_g: &mut Gfx, state: bool| {
                let s = tt();
                s.items_expanded_mode.set(state);
                s.scroll_y.set(0.0);
            })),
            None,
            false,
        );
        items_expanded_check.base.shown.set_fn(items_shown.clone());
        let set_label = |text: &str| {
            let l = LabelControl::new(None, rect(0.0, 0.0, 0.0, 16.0), text.to_string());
            l.base.shown.set_fn(items_shown.clone());
            l
        };
        let set_drop = |g: &Gfx, f: SelFn, wide: bool| {
            let d = DropDownControl::new(g, None, rect(0.0, 0.0, 216.0, 20.0), Vec::new(), Some(f), None, false);
            d.base.enabled.set_fn(items_shown.clone());
            d.base.shown.set_fn(items_shown.clone());
            if wide {
                d.max_dropped_width.set(500.0);
                d.enable_dropped_width.set(true);
            }
            d
        };
        let primary_item_set_label = set_label("^7Item set:");
        let tt = t.clone();
        let primary_item_set_select = set_drop(
            g,
            Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let p = tt().primary();
                let id = p
                    .engine
                    .borrow()
                    .as_ref()
                    .expect("engine")
                    .item_sets()
                    .get(index.wrapping_sub(1))
                    .map(|s| s.id);
                if let Some(id) = id {
                    let tab = p.items_tab.borrow().clone().expect("itemsTab");
                    tab.set_active_item_set(g, id);
                    tab.add_undo_state(false);
                }
            }),
            false,
        );
        let compare_item_set_label2 = set_label("^7Item set:");
        let ae = active_entry.clone();
        let compare_item_set_select2 = set_drop(
            g,
            Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                if let Some(e) = ae() {
                    e.set_active_item_set_index(g, index);
                }
            }),
            false,
        );
        let primary_tree_set_label = set_label("^7Tree set:");
        let tt = t.clone();
        let primary_tree_set_select = set_drop(
            g,
            Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let p = tt().primary();
                let n = p.engine.borrow().as_ref().expect("engine").spec_list_labels().len();
                if index >= 1 && index <= n {
                    p.mod_flag.set(true);
                    p.tree_tab_made(g).set_active_spec(g, index);
                }
            }),
            true,
        );
        let compare_tree_set_label = set_label("^7Tree set:");
        let ae = active_entry.clone();
        let compare_tree_set_select = set_drop(
            g,
            Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                if let Some(e) = ae() {
                    e.set_active_spec(g, index);
                }
            }),
            true,
        );
        self.host.add("itemsExpandedCheck", items_expanded_check.clone());
        self.host.add("primaryItemSetLabel", primary_item_set_label.clone());
        self.host.add("primaryItemSetSelect", primary_item_set_select.clone());
        self.host.add("compareItemSetLabel2", compare_item_set_label2.clone());
        self.host.add("compareItemSetSelect2", compare_item_set_select2.clone());
        self.host.add("primaryTreeSetLabel", primary_tree_set_label.clone());
        self.host.add("primaryTreeSetSelect", primary_tree_set_select.clone());
        self.host.add("compareTreeSetLabel", compare_tree_set_label.clone());
        self.host.add("compareTreeSetSelect", compare_tree_set_select.clone());
        // :969-1034 the compare power report
        let tt = t.clone();
        let power_shown = move |_: &Gfx| {
            let s = tt();
            *s.view_mode.borrow() == "SUMMARY" && !s.entries.borrow().is_empty()
        };
        let mut list = vec![DropItem::table("-- Select Metric --")];
        for p in pob_calc::power::power_stat_list() {
            if p.stat.is_some() && !p.ignore_for_nodes {
                let mut item = DropItem::table(&p.label);
                item.data = Some(Rc::new(p));
                list.push(item);
            }
        }
        let tt = t.clone();
        let power_stat_select = DropDownControl::new(
            g,
            None,
            rect(0.0, 0.0, 200.0, 20.0),
            list,
            Some(Rc::new(move |_g: &mut Gfx, _i: usize, v: &DropItem| {
                let s = tt();
                let new = v
                    .data
                    .as_ref()
                    .and_then(|d| d.downcast_ref::<pob_calc::power::PowerStat>())
                    .cloned();
                match new {
                    Some(p) => {
                        let same = s.compare_power_stat.borrow().as_ref().map(|x| x.stat.clone()) == Some(p.stat.clone());
                        if !same {
                            *s.compare_power_stat.borrow_mut() = Some(p);
                            s.compare_power_dirty.set(true);
                        }
                    }
                    None => {
                        *s.compare_power_stat.borrow_mut() = None;
                        *s.compare_power_results.borrow_mut() = None;
                        *s.compare_power_run.borrow_mut() = None;
                        s.compare_power_list_synced.set(false);
                    }
                }
            })),
            None,
            false,
        );
        power_stat_select.base.shown.set_fn(power_shown.clone());
        let ps = power_stat_select.clone();
        power_stat_select
            .tip
            .set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, (mode, _i, _v)| {
                tooltip.clear(false);
                if mode == "OUT" || ps.dropped.get() {
                    return;
                }
                tooltip.add_line(g, 14.0, Some("Select a metric to calculate power report"));
            });
        let check = |g: &Gfx, label: &str, tip: &str| {
            let tt = t.clone();
            let c = CheckBoxControl::new(
                g,
                None,
                [0.0, 0.0, 18.0],
                Some(label),
                Some(Rc::new(move |_g: &mut Gfx, _state: bool| tt().compare_power_dirty.set(true))),
                Some(tip),
                true,
            );
            c.base.shown.set_fn(power_shown.clone());
            c
        };
        let power_tree_check = check(g, "Tree:", "Include passive tree nodes from compared build");
        let power_items_check = check(g, "Items:", "Include items from compared build");
        let power_gems_check = check(g, "Skill gems:", "Include skill gem groups unique to compared build");
        let power_support_gems_check = check(g, "Support gems:", "Include support gems from compared build's active skill");
        let power_config_check = check(g, "Config:", "Include config option differences from compared build");
        self.host.add("comparePowerStatSelect", power_stat_select.clone());
        self.host.add("comparePowerTreeCheck", power_tree_check.clone());
        self.host.add("comparePowerItemsCheck", power_items_check.clone());
        self.host.add("comparePowerGemsCheck", power_gems_check.clone());
        self.host.add("comparePowerSupportGemsCheck", power_support_gems_check.clone());
        self.host.add("comparePowerConfigCheck", power_config_check.clone());
        // :1037-1039
        let (power_report_list, power_status, power_list_state) = Self::new_power_list(self.me.clone());
        power_report_list.base.shown.set_fn(power_shown.clone());
        power_status.base.shown.set_fn(power_shown.clone());
        self.host.add("comparePowerReportList", power_report_list.clone());
        self.host.add("comparePowerStatus", power_status.clone());
        // :1042-1070 the scroll bars
        let calcs_scroll_bar = ScrollBarControl::new(None, rect(0.0, 0.0, 18.0, 0.0), Some(50.0), Some("VERTICAL"), true);
        let (tt, sb) = (t.clone(), calcs_scroll_bar.clone());
        calcs_scroll_bar.base.shown.set_fn(move |g| {
            let s = tt();
            *s.view_mode.borrow() == "CALCS" && s.active_compare().is_some() && sb.base.enabled.get(g)
        });
        let view_scroll_bar = ScrollBarControl::new(None, rect(0.0, 0.0, 18.0, 0.0), Some(50.0), Some("VERTICAL"), true);
        let (tt, sb) = (t.clone(), view_scroll_bar.clone());
        view_scroll_bar.base.shown.set_fn(move |g| {
            let s = tt();
            let m = *s.view_mode.borrow();
            s.active_compare().is_some() && m != "TREE" && m != "CALCS" && sb.base.enabled.get(g)
        });
        let items_h_scroll_bar = ScrollBarControl::new(
            None,
            rect(0.0, 0.0, 0.0, ITEMS_H_SCROLL_BAR_HEIGHT),
            Some(60.0),
            Some("HORIZONTAL"),
            true,
        );
        let (tt, sb) = (t.clone(), items_h_scroll_bar.clone());
        items_h_scroll_bar.base.shown.set_fn(move |g| {
            let s = tt();
            *s.view_mode.borrow() == "ITEMS" && s.active_compare().is_some() && sb.base.enabled.get(g)
        });
        let skills_h_scroll_bar = ScrollBarControl::new(
            None,
            rect(0.0, 0.0, 0.0, SKILLS_H_SCROLL_BAR_HEIGHT),
            Some(60.0),
            Some("HORIZONTAL"),
            true,
        );
        let (tt, sb) = (t, skills_h_scroll_bar.clone());
        skills_h_scroll_bar.base.shown.set_fn(move |g| {
            let s = tt();
            *s.view_mode.borrow() == "SKILLS" && s.active_compare().is_some() && sb.base.enabled.get(g)
        });
        self.host.add("calcsScrollBar", calcs_scroll_bar.clone());
        self.host.add("viewScrollBar", view_scroll_bar.clone());
        self.host.add("itemsHScrollBar", items_h_scroll_bar.clone());
        self.host.add("skillsHScrollBar", skills_h_scroll_bar.clone());
        Rc::new(Controls {
            sub_tab_anchor,
            sub_tabs,
            compare_build_label,
            compare_build_select,
            import_btn,
            reimport_btn,
            remove_btn,
            compare_spec_select,
            compare_skill_set_select,
            compare_item_set_select,
            compare_config_set_select,
            cmp_socket_group,
            cmp_main_skill,
            cmp_skill_part,
            cmp_stage_count,
            cmp_mine_count,
            cmp_minion,
            cmp_minion_skill,
            cmp_minion_library,
            prim_calcs,
            cmp_calcs,
            tree: tree_controls,
            calcs_show_only_differences_check,
            copy_config_btn,
            config_toggle_btn,
            config_search_edit,
            config_primary_set_label,
            config_primary_set_select,
            items_expanded_check,
            primary_item_set_label,
            primary_item_set_select,
            compare_item_set_label2,
            compare_item_set_select2,
            primary_tree_set_label,
            primary_tree_set_select,
            compare_tree_set_label,
            compare_tree_set_select,
            power_stat_select,
            power_tree_check,
            power_items_check,
            power_gems_check,
            power_support_gems_check,
            power_config_check,
            power_report_list,
            power_status,
            power_list_state,
            view_scroll_bar,
            calcs_scroll_bar,
            items_h_scroll_bar,
            skills_h_scroll_bar,
        })
    }

    /// :495-594 (`prim`) / :597-723 (`cmp`): the same controls over the primary build or the active entry; the writes are to the `*Calcs` fields, `buildFlag` of that build.
    fn calcs_skill_controls(&self, g: &mut Gfx, side: &'static str) -> CalcsSkillControls {
        let w = self.me.clone();
        let target = move || -> Option<Rc<BuildMode>> {
            let t = w.upgrade().expect("compareTab");
            if side == "prim" {
                Some(t.primary())
            } else {
                t.active_compare().map(|e| e.b.clone())
            }
        };
        let edit = |f: fn(&mut Engine, usize, &DropItem) -> Result<(), String>| -> Option<SelFn> {
            let tg = target.clone();
            Some(Rc::new(move |_g: &mut Gfx, index: usize, v: &DropItem| {
                if let Some(b) = tg() {
                    f(b.engine.borrow_mut().as_mut().expect("engine"), index, v).unwrap_or_else(|e| panic!("{e}"));
                    b.build_flag.set(true);
                }
            }))
        };
        let dd = |g: &Gfx, w: f64, h: f64, list: Vec<DropItem>, f: Option<SelFn>| {
            let d = DropDownControl::new(g, None, rect(0.0, 0.0, w, h), list, f, None, false);
            d.base.shown.set(false);
            d
        };
        let socket_group = dd(
            g,
            200.0,
            18.0,
            Vec::new(),
            edit(|e, i, _| {
                e.set_calcs_input("skill_number", Some(pob_calc::pob_build::InputValue::Num(i as f64)));
                Ok(())
            }),
        );
        socket_group.max_dropped_width.set(400.0);
        socket_group.enable_dropped_width.set(true);
        let main_skill = dd(g, 200.0, 18.0, Vec::new(), edit(|e, i, _| e.set_main_active_skill_calcs(i as f64)));
        let skill_part = dd(g, 150.0, 18.0, Vec::new(), edit(|e, i, _| e.set_skill_part_calcs(i as f64)));
        let num = |g: &Gfx, f: fn(&mut Engine, Option<f64>) -> Result<(), String>| {
            let tg = target.clone();
            let c = EditControl::new(
                g,
                None,
                rect(0.0, 0.0, 52.0, 18.0),
                EditArgs {
                    init: Some(String::new()),
                    filter: Some("%D".into()),
                    limit: Some(5),
                    change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                        if let Some(b) = tg() {
                            f(b.engine.borrow_mut().as_mut().expect("engine"), buf.parse::<f64>().ok()).unwrap_or_else(|e| panic!("{e}"));
                            b.build_flag.set(true);
                        }
                    })),
                    ..EditArgs::default()
                },
            );
            c.base.shown.set(false);
            c
        };
        let stage_count = num(g, |e, n| e.set_skill_stage_count_calcs(n));
        let mine_count = num(g, |e, n| e.set_skill_mine_count_calcs(n));
        let tg = target.clone();
        let show_minion = CheckBoxControl::new(
            g,
            None,
            [0.0, 0.0, 18.0],
            None,
            Some(Rc::new(move |_g: &mut Gfx, state: bool| {
                if let Some(b) = tg() {
                    b.engine
                        .borrow_mut()
                        .as_mut()
                        .expect("engine")
                        .set_calcs_input("showMinion", Some(pob_calc::pob_build::InputValue::Bool(state)));
                    b.build_flag.set(true);
                }
            })),
            Some("Show stats for the minion instead of the player."),
            false,
        );
        show_minion.base.shown.set(false);
        let minion = dd(
            g,
            140.0,
            18.0,
            Vec::new(),
            edit(|e, _, v| {
                if let Some(m) = v.data.as_ref().and_then(|d| d.downcast_ref::<pob_engine::shell::MinionEntry>()) {
                    e.set_skill_minion_calcs(m)?;
                }
                Ok(())
            }),
        );
        let minion_skill = dd(g, 140.0, 18.0, Vec::new(), edit(|e, i, _| e.set_skill_minion_skill_calcs(i as f64)));
        let tg = target;
        let mode = dd(
            g,
            120.0,
            18.0,
            BUFF_MODES.iter().map(|(l, _)| DropItem::table(l)).collect(),
            Some(Rc::new(move |_g: &mut Gfx, index: usize, _v: &DropItem| {
                if let Some(b) = tg() {
                    b.engine.borrow_mut().as_mut().expect("engine").set_calcs_input(
                        "misc_buffMode",
                        Some(pob_calc::pob_build::InputValue::Str(BUFF_MODES[index - 1].1.into())),
                    );
                    b.build_flag.set(true);
                }
            })),
        );
        let pre = if side == "prim" { "primCalcs" } else { "cmpCalcs" };
        self.host.add(&format!("{pre}SocketGroup"), socket_group.clone());
        self.host.add(&format!("{pre}MainSkill"), main_skill.clone());
        self.host.add(&format!("{pre}SkillPart"), skill_part.clone());
        self.host.add(&format!("{pre}StageCount"), stage_count.clone());
        self.host.add(&format!("{pre}MineCount"), mine_count.clone());
        self.host.add(&format!("{pre}ShowMinion"), show_minion.clone());
        self.host.add(&format!("{pre}Minion"), minion.clone());
        self.host.add(&format!("{pre}MinionSkill"), minion_skill.clone());
        self.host.add(&format!("{pre}Mode"), mode.clone());
        CalcsSkillControls {
            socket_group,
            main_skill,
            skill_part,
            stage_count,
            mine_count,
            show_minion,
            minion,
            minion_skill,
            mode,
        }
    }

    /// `GetShortBuildName` (:1074-1081).
    pub fn short_build_name(full: Option<&str>) -> String {
        let Some(full) = full else { return "Your Build".into() };
        match full.find(" - ") {
            Some(p) => full[p + 3..].to_string(),
            None => full.to_string(),
        }
    }

    /// `ImportBuild` (:1245-1258).
    pub fn import_build(&self, g: &mut Gfx, xml: &str, label: &str) -> bool {
        // CompareEntry.lua:183 `wipeGlobalCache()`
        if let Some(e) = self.primary().engine.borrow().as_ref() {
            e.wipe_misc_cache();
        }
        match CompareEntry::new(g, xml, label, self.primary.clone()) {
            Ok(e) => {
                self.entries.borrow_mut().push(e);
                self.active.set(self.entries.borrow().len());
                self.update_build_selector();
                true
            }
            Err(err) => {
                // CompareEntry.lua:67, :71: `ConPrintf`, no entry
                g.con_printf(&format!("CompareEntry: {err}"));
                false
            }
        }
    }

    /// `RemoveBuild` (:1273-1290).
    pub fn remove_build(&self, _g: &mut Gfx, index: usize) {
        let n = {
            let mut e = self.entries.borrow_mut();
            if index < 1 || index > e.len() {
                return;
            }
            e.remove(index - 1);
            e.len()
        };
        if self.active.get() > n {
            self.active.set(n);
        }
        if self.active.get() == 0 && n > 0 {
            self.active.set(1);
        }
        let c = self.c();
        if n == 0 {
            c.cmp_main_skill.base.shown.set(false);
            c.cmp_skill_part.base.shown.set(false);
            c.cmp_stage_count.base.shown.set(false);
            c.cmp_mine_count.base.shown.set(false);
            c.cmp_minion.base.shown.set(false);
            c.cmp_minion_skill.base.shown.set(false);
        }
        self.update_build_selector();
    }

    /// `UpdateBuildSelector` (:1316-1325).
    fn update_build_selector(&self) {
        let c = self.c();
        let list: Vec<String> = self.entries.borrow().iter().map(|e| e.label.clone()).collect();
        let n = list.len();
        *c.compare_build_select.list.borrow_mut() = items(&list);
        let a = self.active.get();
        if a > 0 && a <= n {
            c.compare_build_select.sel_index.set(a);
        }
    }

    /// `OpenImportPopup` (:1467-1530).
    pub fn open_import_popup(&self, g: &mut Gfx) {
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        let state_text = Rc::new(RefCell::new(String::new()));
        let label = LabelControl::new(
            None,
            rect(0.0, 20.0, 0.0, 16.0),
            "^7Paste a build code or URL to import as comparison:".to_string(),
        );
        let input = EditControl::new(
            g,
            None,
            rect(0.0, 50.0, 450.0, 20.0),
            EditArgs {
                init: Some(String::new()),
                clearable: true,
                ..EditArgs::default()
            },
        );
        let name = EditControl::new(
            g,
            None,
            rect(0.0, 80.0, 450.0, 20.0),
            EditArgs {
                init: Some(String::new()),
                prompt: Some("Name (optional)".into()),
                limit: Some(100),
                ..EditArgs::default()
            },
        );
        let nr: ControlRef = name.clone();
        let st = state_text.clone();
        let state = LabelControl::new(anchor("TOPLEFT", &nr, "BOTTOMLEFT"), rect(0.0, 4.0, 0.0, 16.0), String::new());
        state.label.set_fn(move |_| st.borrow().clone());
        let tab = self.rc();
        let (inp, nm, st) = (input.clone(), name.clone(), state_text);
        let go = ButtonControl::new(None, rect(-118.0, 130.0, 80.0, 20.0), "Import".to_string(), move |g| {
            let buf = inp.buf.borrow().clone();
            if buf.is_empty() {
                return;
            }
            let custom = nm.buf.borrow().clone();
            let custom = (!custom.is_empty()).then_some(custom);
            // :1493-1511
            for (i, (site, m)) in BUILD_SITES.into_iter().enumerate() {
                if pob_lua::gsub(buf.as_bytes(), m.as_bytes(), pob_lua::Repl::Str(b""), Some(1)).is_ok_and(|r| r.1 > 0) {
                    *st.borrow_mut() = format!("{}Downloading...", color_code("WARNING").expect("WARNING"));
                    let (tab, st, custom) = (tab.clone(), st.clone(), custom.clone());
                    crate::build_sites::download_build(&buf, i + 1, move |g, ok, data| {
                        if !ok {
                            *st.borrow_mut() = format!("{}{data}", color_code("NEGATIVE").expect("NEGATIVE"));
                            return;
                        }
                        match pob_build::code::decode_build_code(&data).map(|x| String::from_utf8_lossy(&x).into_owned()) {
                            Some(xml) => {
                                let label = custom.unwrap_or_else(|| format!("Imported from {site}"));
                                tab.import_build(g, &xml, &label);
                                main().close_popup();
                            }
                            None => {
                                *st.borrow_mut() = format!("{}Failed to decode build data", color_code("NEGATIVE").expect("NEGATIVE"));
                            }
                        }
                    });
                    return;
                }
            }
            match pob_build::code::decode_build_code(&buf).map(|x| String::from_utf8_lossy(&x).into_owned()) {
                Some(xml) => {
                    tab.import_build(g, &xml, custom.as_deref().unwrap_or("Imported build"));
                    main().close_popup();
                }
                None => {
                    *st.borrow_mut() = format!("{}Invalid build code", color_code("NEGATIVE").expect("NEGATIVE"));
                }
            }
        });
        let gr = go.clone();
        let inp = input.clone();
        *input.enter_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, _buf: &str| {
            if !inp.buf.borrow().is_empty() {
                gr.click(g);
            }
        }));
        let tab = self.rc();
        let import_folder = ButtonControl::new(None, rect(0.0, 130.0, 140.0, 20.0), "Import from Folder".to_string(), move |g| {
            main().close_popup();
            tab.open_import_folder_popup(g);
        });
        let cancel = ButtonControl::new(None, rect(118.0, 130.0, 80.0, 20.0), "Cancel".to_string(), |_| main().close_popup());
        controls.insert("label".into(), label);
        controls.insert("input".into(), input);
        controls.insert("name".into(), name);
        controls.insert("state".into(), state);
        controls.insert("go".into(), go);
        controls.insert("importFolder".into(), import_folder);
        controls.insert("cancel".into(), cancel);
        main().open_popup(
            g,
            500.0,
            160.0,
            "Import Comparison Build",
            controls,
            Some("go"),
            Some("input"),
            Some("cancel"),
            None,
        );
    }

    /// `Draw` (:1650-1866).
    pub fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, input_events: &mut InputEvents) {
        sg::text_log_begin();
        main().draw_background(f, view_port);
        let c = self.c();
        c.sub_tab_anchor.base.x.set(view_port.x + 4.0);
        c.sub_tab_anchor.base.y.set(view_port.y + 96.0);
        if !self.entries.borrow().is_empty() {
            sg::set_draw_color(f, 0.25, 0.25, 0.25);
            sg::draw_rect(f, view_port.x + 4.0, view_port.y + 32.0, view_port.width - 8.0, 2.0);
            sg::draw_rect(f, view_port.x + 4.0, view_port.y + 88.0, view_port.width - 8.0, 2.0);
            sg::draw_rect(f, view_port.x + 4.0, view_port.y + 122.0, view_port.width - 8.0, 2.0);
        }
        c.compare_build_label.base.x.set(0.0);
        let content_vp = Rect {
            x: view_port.x,
            y: view_port.y + CONTROL_BAR_HEIGHT,
            width: view_port.width,
            height: view_port.height - CONTROL_BAR_HEIGHT,
        };
        let entry = self.active_compare();
        // Build.lua:1260 `wipeGlobalCache()` of the primary build: the cache of the entries too
        let rev = self.primary().engine.borrow().as_ref().expect("engine").output_revision();
        if self.primary_revision.replace(rev) != rev {
            for e in self.entries.borrow().iter() {
                if let Some(en) = e.b.engine.borrow().as_ref() {
                    en.wipe_misc_cache();
                }
            }
        }
        if let Some(e) = &entry
            && e.b.build_flag.get()
        {
            e.rebuild(f);
        }
        let tree_layout = match &entry {
            Some(e) if *self.view_mode.borrow() == "TREE" => Some(self.layout_tree_view(f, &content_vp, e)),
            _ => None,
        };
        if let Some(e) = &entry
            && *self.view_mode.borrow() == "CONFIG"
        {
            self.layout_config_view(f, &content_vp, e);
        }
        if let Some(e) = &entry {
            self.update_set_selectors(f, e);
        }
        // :1690-1696
        if *self.view_mode.borrow() == "CONFIG"
            && entry.is_some()
            && ipairs(input_events).any(|(_, ev)| matches!(ev, InputEvent::KeyDown { key, .. } if key == "f"))
            && sg::is_key_down(f, "CTRL")
        {
            self.host.select_control(f, Some(c.config_search_edit.clone()));
        }
        // :1698-1707
        self.calcs_skill_header_height.set(0.0);
        if *self.view_mode.borrow() == "CALCS"
            && let Some(e) = &entry
        {
            let h = self.layout_calcs_skill_controls(f, &content_vp, e);
            self.calcs_skill_header_height.set(h);
            if h > 0.0 {
                sg::set_draw_color(f, 0.05, 0.05, 0.05);
                sg::draw_rect(f, content_vp.x, content_vp.y, content_vp.width, h);
            }
        }
        let mode = *self.view_mode.borrow();
        let vsb = &c.view_scroll_bar;
        if entry.is_some() && mode != "TREE" && mode != "CALCS" {
            let (top, bottom, content) = match mode {
                "SUMMARY" => (SUMMARY_HEADER_HEIGHT, 0.0, self.summary_total_content_height.get()),
                "CONFIG" => (CONFIG_FIXED_HEADER_HEIGHT, 0.0, self.config_total_content_height.get()),
                "ITEMS" => (
                    ITEMS_CHECKBOX_OFFSET,
                    if c.items_h_scroll_bar.base.enabled.get(f) {
                        ITEMS_H_SCROLL_BAR_HEIGHT
                    } else {
                        0.0
                    },
                    self.items_total_content_height.get(),
                ),
                _ => (
                    SKILLS_HEADER_HEIGHT,
                    if c.skills_h_scroll_bar.base.enabled.get(f) {
                        SKILLS_H_SCROLL_BAR_HEIGHT
                    } else {
                        0.0
                    },
                    self.skills_total_content_height.get(),
                ),
            };
            let view_height = (content_vp.height - top - bottom).max(0.0);
            vsb.base.x.set(content_vp.x + content_vp.width - 18.0);
            vsb.base.y.set(content_vp.y + top);
            vsb.base.height.set(view_height);
            vsb.set_content_dimension(f, content, view_height);
            vsb.set_offset(self.scroll_y.get());
            self.scroll_y.set(vsb.offset.get());
        } else {
            vsb.set_content_dimension(f, 0.0, content_vp.height);
            vsb.set_offset(0.0);
        }
        self.host.process_controls_input(f, input_events, view_port);
        self.handle_scroll_input(f, &content_vp, input_events);
        if vsb.is_shown(f) {
            self.scroll_y.set(vsb.offset.get());
        }
        let mode = *self.view_mode.borrow();
        let drawing_tree = mode == "TREE" && entry.is_some();
        if drawing_tree && let Some(e) = &entry {
            self.draw_tree(f, &content_vp, input_events, e, tree_layout);
        }
        match &entry {
            None => {
                sg::set_viewport(f, Some([content_vp.x, content_vp.y, content_vp.width, content_vp.height]));
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
                sg::draw_string(f, 0.0, 40.0, Some("CENTER"), 20.0, "VAR", "^7No comparison build loaded.");
                sg::draw_string(
                    f,
                    0.0,
                    70.0,
                    Some("CENTER"),
                    16.0,
                    "VAR",
                    &format!(
                        "^7Click {}Import...^7 above to import a build to compare against.",
                        color_code("POSITIVE").expect("POSITIVE")
                    ),
                );
                sg::set_viewport(f, None);
            }
            Some(e) => {
                if mode == "ITEMS" {
                    self.layout_items_header(f, &content_vp, e);
                }
                match mode {
                    "SUMMARY" => self.draw_summary(f, &content_vp, e),
                    "ITEMS" => self.draw_items(f, &content_vp, e, input_events),
                    "SKILLS" => self.draw_skills(f, &content_vp, e),
                    "CALCS" => self.draw_calcs(f, &content_vp, e, self.calcs_skill_header_height.get()),
                    "CONFIG" => self.draw_config(f, &content_vp, e, false),
                    _ => {}
                }
            }
        }
        self.host.draw_controls(f, view_port, None);
        if *self.view_mode.borrow() == "CONFIG"
            && let Some(e) = &entry
        {
            self.draw_config(f, &content_vp, e, true);
            self.draw_config_header_controls(f, view_port);
        }
        if drawing_tree {
            sg::set_draw_layer(f, Some(0), 0);
        }
        if vsb.is_shown(f) {
            self.scroll_y.set(vsb.offset.get());
        }
        sg::text_log_end();
    }

    /// `UpdateSetSelectors` (:2228-2259).
    fn update_set_selectors(&self, g: &mut Gfx, entry: &Rc<CompareEntry>) {
        let c = self.c();
        let eng = entry.b.engine.borrow();
        let e = eng.as_ref().expect("engine");
        *c.compare_spec_select.list.borrow_mut() = spec_list_drop(e);
        c.compare_spec_select.sel_index.set(e.active_spec());
        let order = |ids: Vec<(f64, String)>, active: f64, control: &DropDownControl, g: &Gfx| populate(ids, active, control, g);
        let skill_sets = e
            .skill_sets()
            .into_iter()
            .map(|(id, t)| (id, t.unwrap_or_else(|| "Default".into())))
            .collect();
        order(skill_sets, e.skills_tab().active_skill_set_id, &c.compare_skill_set_select, g);
        let item_sets = e
            .item_sets()
            .iter()
            .map(|s| (s.id, s.title.clone().unwrap_or_else(|| "Default".into())))
            .collect();
        order(item_sets, e.active_item_set_id(), &c.compare_item_set_select, g);
        let config_sets = e
            .config_set_order()
            .into_iter()
            .map(|id| (id, e.config_set_title(id).unwrap_or_else(|| "Default".into())))
            .collect();
        order(config_sets, e.config_active_set(), &c.compare_config_set_select, g);
        let sel = e.compare_skill_select(false).unwrap_or_else(|err| panic!("{err}"));
        apply_skill_select(
            g,
            &SkillControls {
                main_socket_group: &c.cmp_socket_group,
                main_skill: &c.cmp_main_skill,
                main_skill_part: &c.cmp_skill_part,
                main_skill_stage_count: &c.cmp_stage_count,
                main_skill_mine_count: &c.cmp_mine_count,
                main_skill_minion: &c.cmp_minion,
                main_skill_minion_library: &c.cmp_minion_library,
                main_skill_minion_skill: &c.cmp_minion_skill,
            },
            &sel,
        );
    }

    /// `HandleScrollInput` (:2412-2461).
    fn handle_scroll_input(&self, g: &mut Gfx, content_vp: &Rect, input_events: &mut InputEvents) {
        let c = self.c();
        let (cx, cy) = g.get_cursor_pos();
        let in_content =
            cx >= content_vp.x && cx < content_vp.x + content_vp.width && cy >= content_vp.y && cy < content_vp.y + content_vp.height;
        let over_list = c.power_report_list.is_shown(g) && c.power_report_list.is_mouse_over(g) == Some(true);
        let over_vsb = c.view_scroll_bar.is_shown(g) && c.view_scroll_bar.mouse_over(g).0;
        let mode = *self.view_mode.borrow();
        let evs: Vec<(usize, String)> = ipairs(input_events)
            .filter_map(|(id, e)| match e {
                InputEvent::KeyUp { key, .. } => Some((id, key.clone())),
                _ => None,
            })
            .collect();
        for (id, key) in evs {
            if !in_content || over_list {
                continue;
            }
            if mode == "CALCS" {
                if key == "WHEELUP" {
                    c.calcs_scroll_bar.scroll(-1.0);
                    input_events[id] = None;
                } else if key == "WHEELDOWN" {
                    c.calcs_scroll_bar.scroll(1.0);
                    input_events[id] = None;
                }
            } else if key == "WHEELUP" && mode != "TREE" && !over_vsb {
                self.scroll_y.set((self.scroll_y.get() - 40.0).max(0.0));
                c.view_scroll_bar.set_offset(self.scroll_y.get());
                self.scroll_y.set(c.view_scroll_bar.offset.get());
                input_events[id] = None;
            } else if key == "WHEELDOWN" && mode != "TREE" && !over_vsb {
                let h = content_vp.height;
                let max = match mode {
                    "CONFIG" => (self.config_total_content_height.get() - (h - CONFIG_FIXED_HEADER_HEIGHT)).max(0.0),
                    "SUMMARY" if self.summary_total_content_height.get() > 0.0 => {
                        (self.summary_total_content_height.get() - (h - SUMMARY_HEADER_HEIGHT)).max(0.0)
                    }
                    "ITEMS" if self.items_total_content_height.get() > 0.0 => {
                        let r = if c.items_h_scroll_bar.base.enabled.get(g) {
                            ITEMS_H_SCROLL_BAR_HEIGHT
                        } else {
                            0.0
                        };
                        (self.items_total_content_height.get() - (h - ITEMS_CHECKBOX_OFFSET - r)).max(0.0)
                    }
                    "SKILLS" if self.skills_total_content_height.get() > 0.0 => {
                        let r = if c.skills_h_scroll_bar.base.enabled.get(g) {
                            SKILLS_H_SCROLL_BAR_HEIGHT
                        } else {
                            0.0
                        };
                        (self.skills_total_content_height.get() - (h - SKILLS_HEADER_HEIGHT - r)).max(0.0)
                    }
                    _ => 0.0,
                };
                self.scroll_y.set((self.scroll_y.get() + 40.0).min(max));
                c.view_scroll_bar.set_offset(self.scroll_y.get());
                self.scroll_y.set(c.view_scroll_bar.offset.get());
                input_events[id] = None;
            }
        }
    }

    /// The state of the gate (the runner's `compareDump`).
    pub fn debug_dump(&self, g: &Gfx) -> serde_json::Value {
        let c = self.c();
        let drop = |d: &DropDownControl| {
            serde_json::json!({
                "shown": d.is_shown(g),
                "enabled": d.is_enabled(g),
                "sel": d.sel_index.get(),
                "list": d.list.borrow().iter().map(|v| v.label.clone().unwrap_or_default()).collect::<Vec<_>>(),
            })
        };
        let edit = |e: &EditControl| {
            if e.is_shown(g) {
                e.buf.borrow().clone()
            } else {
                "hidden".to_string()
            }
        };
        let entries: Vec<_> = self
            .entries
            .borrow()
            .iter()
            .map(|e| {
                let eng = e.b.engine.borrow();
                let en = eng.as_ref().expect("engine");
                let o = &en.result().perform.player.output;
                let num = |k: &str| match o.get(k) {
                    Some(pob_calc::OutValue::Num(n)) => {
                        Some(pob_lua::format::format("%.17g", &[pob_lua::format::Arg::Num(*n)]).expect("format"))
                    }
                    _ => None,
                };
                serde_json::json!({
                    "label": e.label,
                    "level": en.model().level,
                    "mainSocketGroup": en.main_socket_group(),
                    "Life": num("Life"),
                    "TotalDPS": num("TotalDPS"),
                    "FullDPS": num("FullDPS"),
                    "TotalEHP": num("TotalEHP"),
                })
            })
            .collect();
        serde_json::json!({
            "mode": *self.view_mode.borrow(),
            "active": self.active.get(),
            "entries": entries,
            "scrollY": self.scroll_y.get(),
            "buildSelect": drop(&c.compare_build_select),
            "specSelect": drop(&c.compare_spec_select),
            "skillSetSelect": drop(&c.compare_skill_set_select),
            "itemSetSelect": drop(&c.compare_item_set_select),
            "configSetSelect": drop(&c.compare_config_set_select),
            "cmpSocketGroup": drop(&c.cmp_socket_group),
            "cmpMainSkill": drop(&c.cmp_main_skill),
            "cmpSkillPart": drop(&c.cmp_skill_part),
            "cmpMinion": drop(&c.cmp_minion),
            "cmpMinionSkill": drop(&c.cmp_minion_skill),
            "powerRows": self.power_rows(),
            "cmpStageCount": edit(&c.cmp_stage_count),
            "cmpMineCount": edit(&c.cmp_mine_count),
        })
    }

    /// The set selectors of the Items view, for the gate.
    pub fn debug_sets(&self) -> serde_json::Value {
        let c = self.c();
        let drop = |d: &DropDownControl| {
            serde_json::json!({
                "sel": d.sel_index.get(),
                "list": d.list.borrow().iter().map(|v| v.label.clone().unwrap_or_default()).collect::<Vec<_>>(),
            })
        };
        serde_json::json!({
            "primaryItem": drop(&c.primary_item_set_select),
            "compareItem": drop(&c.compare_item_set_select2),
            "primaryTree": drop(&c.primary_tree_set_select),
            "compareTree": drop(&c.compare_tree_set_select),
        })
    }

    /// `control:SetSel(index)` of the gate.
    pub fn debug_select(&self, g: &mut Gfx, name: &str, index: i64) {
        let c = self.c();
        match name {
            "comparePowerStatSelect" => c.power_stat_select.set_sel(g, index, false),
            "cmpSocketGroup" => c.cmp_socket_group.set_sel(g, index, false),
            "rightVersionSelect" => c.tree.right_version.set_sel(g, index, false),
            other => panic!("debug_select {other}"),
        }
    }

    /// The control of the gate's cursor steps.
    pub fn debug_control(&self, name: &str) -> Option<ControlRef> {
        self.host.get(name)
    }
}

/// The state of the real window after a frame (the runner's `RealState`, tools/sg-oracle/compare-rust-input.ps1).
pub fn debug_input_state(g: &Gfx) -> serde_json::Value {
    use serde_json::json;
    if main().mode.borrow().as_deref() != Some("BUILD") {
        return serde_json::Value::Null;
    }
    let b = main().build_mode.clone();
    let Some(t) = b.compare_tab.borrow().clone() else {
        return serde_json::Value::Null;
    };
    let pos = |ctl: &dyn Control| {
        let (x, y) = ctl.get_pos(g);
        let (w, h) = ctl.get_size(g);
        json!({ "x": (x + w / 2.0).floor(), "y": (y + h / 2.0).floor() })
    };
    let shown = |ctl: Option<ControlRef>| match ctl {
        Some(c) if c.is_shown(g) => pos(&*c),
        _ => json!({ "x": 0, "y": 0 }),
    };
    let c = t.c();
    let m = main();
    let popups = m.popups.borrow();
    let p = popups.first();
    let pc = |name: &str| p.and_then(|p| p.host.get(name));
    let list = pc("buildList");
    let row2 = list.as_ref().map_or(json!({ "x": 0, "y": 0 }), |l| {
        let (x, y) = l.get_pos(g);
        json!({ "x": x + 20.0, "y": y + 2.0 + 20.0 + 10.0 })
    });
    let folder = t.debug_folder(g);
    let labels: Vec<String> = t.entries.borrow().iter().map(|e| e.label.clone()).collect();
    let str_value = |ctl: Option<ControlRef>| ctl.and_then(|c| c.debug_value(g)).unwrap_or(json!(""));
    json!({
        "viewMode": *b.view_mode.borrow(),
        "modeCompare": pos(&*b.host.get("modeCompare").expect("modeCompare")),
        "importBtn": pos(&*c.import_btn),
        "removeBtn": shown(Some(c.remove_btn.clone())),
        "popups": popups.len(),
        "popupTitle": p.map_or(String::new(), |p| p.title.get(g)),
        "input": shown(pc("input")),
        "inputBuf": str_value(pc("input")),
        "state": str_value(pc("state")),
        "go": shown(pc("go")),
        "importFolder": shown(pc("importFolder")),
        "folderRow2": row2,
        "folderN": folder.as_ref().filter(|_| list.is_some()).map_or(0, |f| f["rows"].as_array().map_or(0, Vec::len)),
        "folderSel": folder.as_ref().filter(|_| list.is_some()).map_or(json!(0), |f| f["sel"].clone()),
        "folderOpen": shown(pc("open")),
        "entries": labels,
        "active": t.active.get(),
        "mode": *t.view_mode.borrow(),
        "scrollY": t.scroll_y.get(),
        "subItems": shown(t.host.get("subTabItems")),
        "subConfig": shown(t.host.get("subTabConfig")),
        "subTree": shown(t.host.get("subTabTree")),
        "configSearch": shown(Some(c.config_search_edit.clone())),
        "configBuf": c.config_search_edit.buf.borrow().clone(),
    })
}

/// `PopulateSetDropdown` (:1087-1102).
pub(crate) fn populate(ids: Vec<(f64, String)>, active: f64, control: &DropDownControl, g: &Gfx) {
    let mut list = Vec::new();
    for (i, (id, title)) in ids.iter().enumerate() {
        list.push(title.clone());
        if *id == active {
            control.sel_index.set(i + 1);
        }
    }
    control.set_list(g, Some(items(&list)));
}

/// `treeTab:GetSpecList()` (TreeTab.lua:491-497).
pub(crate) fn spec_list_drop(e: &Engine) -> Vec<DropItem> {
    e.spec_list_labels().iter().map(|s| DropItem::str(s)).collect()
}
