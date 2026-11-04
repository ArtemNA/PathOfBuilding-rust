//! `Classes/TreeTab.lua` (upstream 3dbca4463): the controls under the tree view, the spec list, the popups (reset, convert, import/export,
//! mastery, tattoo), the power report. The specs are `Engine`'s model; the undo lists (`spec.undo`, UndoHandler.lua) live here, one per spec.
//!
//! `FindTimelessJewel` is `tabs::timeless`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use indexmap::IndexMap;
use pob_engine::tree_tab::{PowerProgress, PowerResult, PowerStat, ReportDist, ReportEntry, SpecSummary, SpecUndoState, power_stat_list};
use pob_gfx::{Frame, Gfx};

use crate::colors::color_code;
use crate::common::wrap_string;
use crate::control::{Control, ControlRef, Rect, anchor, anchor_nil, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::list::{ByRef, Column, ListControl, ListHooks};
use crate::host::{ControlHost, InputEvent, InputEvents};
use crate::modules::main::{BareControl, main};
use crate::sg;
use crate::tooltip::Tooltip;

fn cc(name: &str) -> String {
    color_code(name).unwrap_or_default()
}

fn tab() -> Rc<TreeTab> {
    main().build_mode.tree_tab.borrow().clone().expect("treeTab")
}

fn with_engine<R>(f: impl FnOnce(&mut pob_engine::engine::Engine) -> R) -> R {
    let b = main().build_mode.clone();
    let mut eng = b.engine.borrow_mut();
    f(eng.as_mut().expect("engine"))
}

fn engine_read<R>(f: impl FnOnce(&pob_engine::engine::Engine) -> R) -> R {
    let b = main().build_mode.clone();
    let eng = b.engine.borrow();
    f(eng.as_ref().expect("engine"))
}

fn ok_or_panic<T>(r: Result<T, String>) -> T {
    r.unwrap_or_else(|e| panic!("{e}"))
}

fn tree_version_display(ver: &str) -> String {
    pob_data::tree::tree_versions()
        .expect("GameVersions")
        .tree_versions
        .iter()
        .find(|v| v.ver == ver)
        .map_or_else(|| ver.to_string(), |v| v.display.clone())
}

fn latest_tree_version() -> &'static str {
    &pob_data::tree::tree_versions().expect("GameVersions").latest_tree_version
}

/// `treeVersions[ver].url` (GameVersions.lua; the data patch does not export it): read from the original's `GameVersions.lua`.
fn tree_version_url(root: &str, ver: &str) -> String {
    let text = std::fs::read_to_string(format!("{root}GameVersions.lua")).unwrap_or_default();
    let key = format!("[\"{ver}\"] = {{");
    text.find(&key)
        .and_then(|p| {
            let rest = &text[p..];
            let u = rest.find("url = \"")? + 7;
            let e = rest[u..].find('"')?;
            Some(rest[u..u + e].to_string())
        })
        .unwrap_or_default()
}

/// One `spec` of the undo mixin (UndoHandler.lua): `undo[1]` is the current state.
#[derive(Default)]
pub struct SpecUndo {
    /// `Load` ran `ResetUndo`: the first state is the loaded one; a spec made later starts with an empty list.
    from_file: bool,
    undo: Vec<SpecUndoState>,
    redo: Vec<SpecUndoState>,
}

pub struct TreeTab {
    pub host: ControlHost,
    pub is_comparing: Cell<bool>,
    pub is_custom_max_depth: Cell<bool>,
    pub active_compare_spec: Cell<usize>,
    pub show_convert: Cell<bool>,
    pub jump_to: Cell<Option<(f64, f64)>>,
    pub anchor_controls: Rc<BareControl>,
    pub spec_select: Rc<DropDownControl>,
    pub compare_check: Rc<CheckBoxControl>,
    pub compare_select: Rc<DropDownControl>,
    pub reset: Rc<ButtonControl>,
    pub version_text: Rc<LabelControl>,
    pub version_select: Rc<DropDownControl>,
    pub tree_search: Rc<EditControl>,
    pub find_timeless_jewel: Rc<ButtonControl>,
    pub tree_heat_map: Rc<CheckBoxControl>,
    pub node_power_max_depth_select: Rc<DropDownControl>,
    pub node_power_max_depth_custom: Rc<EditControl>,
    pub tree_heat_map_stat_select: Rc<DropDownControl>,
    pub power_report: Rc<ButtonControl>,
    pub power_report_list: Rc<ListControl<ByRef<ReportEntry>>>,
    pub power_filter_select: Rc<DropDownControl>,
    pub spec_convert_text: Rc<LabelControl>,
    pub spec_convert: Rc<ButtonControl>,
    pub spec_convert_all: Rc<ButtonControl>,
    pub undo: RefCell<Vec<SpecUndo>>,
    pub default_tattoo: RefCell<HashMap<String, usize>>,
    pub show_legacy_tattoo: Cell<bool>,
    /// `calcsTab.powerStat` / `nodePowerMaxDepth` / `powerBuildFlag` / `powerBuilder` / `node.power` + `powerMax`.
    pub power_stat: RefCell<Option<PowerStat>>,
    pub node_power_max_depth: Cell<Option<f64>>,
    pub power_build_flag: Cell<bool>,
    #[allow(clippy::type_complexity)]
    pub power_job: RefCell<Option<std::thread::JoinHandle<Result<Option<PowerResult>, String>>>>,
    pub power_progress: RefCell<Arc<PowerProgress>>,
    pub power: RefCell<Option<Rc<PowerResult>>>,
    pub power_toast: RefCell<Option<String>>,
    power_slice_start: Cell<u64>,
    power_last_progress: Cell<u64>,
    pub power_state: PowerListState,
    pub timeless: crate::tabs::timeless::TimelessPersist,
    /// The order `pairs(spec.nodes)` / `pairs(tree.clusterNodeMap)` of an oracle process (R13/R27, `tests/tree.rs`), as `PassiveTreeView::set_order_sample`.
    pub report_order_sample: RefCell<Option<(Vec<u32>, Vec<String>)>>,
    /// The specs behind `compareSpec`, with what their summaries read.
    compare: RefCell<Option<(usize, Rc<pob_tree::PassiveSpec>)>>,
    pub summaries: RefCell<Vec<SpecSummary>>,
}

/// The fields `PowerReportListControl` adds to `ListControl`.
#[derive(Default)]
pub struct PowerListState {
    pub power_label: RefCell<String>,
    pub original_list: RefCell<Vec<ByRef<ReportEntry>>>,
    pub show_clusters: Cell<bool>,
    pub show_masteries: Cell<bool>,
    pub allocated: Cell<bool>,
    pub by_id: RefCell<HashMap<u32, (f64, f64)>>,
}

// --- PowerReportListControl -------------------------------------------------------------------------------------------------------------

struct PowerHooks;

impl ListHooks<ByRef<ReportEntry>> for PowerHooks {
    /// PowerReportListControl.lua:134-141
    fn get_row_value(
        &self,
        _l: &ListControl<ByRef<ReportEntry>>,
        _g: &Gfx,
        column: usize,
        _index: usize,
        r: &ByRef<ReportEntry>,
    ) -> String {
        let r = &r.0;
        match column {
            1 => r.ty.as_str().to_string(),
            2 => r.name.clone(),
            3 => r.power_str.clone(),
            4 => match &r.path_dist {
                ReportDist::Num(n) if *n == 1000.0 => "Anoint".to_string(),
                ReportDist::Num(n) => pob_lua::tostring_num(*n),
                ReportDist::Str(s) => (*s).to_string(),
            },
            5 => r.path_power_str.clone(),
            _ => String::new(),
        }
    }

    fn has_add_value_tooltip(&self) -> bool {
        true
    }

    /// :143-153
    fn add_value_tooltip(
        &self,
        _l: &ListControl<ByRef<ReportEntry>>,
        g: &Gfx,
        tooltip: &mut Tooltip,
        _index: usize,
        r: &ByRef<ReportEntry>,
    ) {
        if !main().popups.borrow().is_empty() {
            tooltip.clear(false);
            return;
        }
        if tooltip.check_for_update((r.0.id, r.0.name.clone())) {
            let sd: Vec<String> = engine_read(|e| e.spec_rc().node(r.0.id).map(|n| n.sd().to_vec())).unwrap_or_default();
            for line in sd {
                tooltip.add_line(g, 16.0, Some(&line));
            }
        }
    }

    /// :128-132 + the callback of TreeTab.lua:278-286
    fn on_sel_click(
        &self,
        _l: &ListControl<ByRef<ReportEntry>>,
        _g: &mut Gfx,
        _index: usize,
        r: &ByRef<ReportEntry>,
        _double: bool,
    ) -> Option<bool> {
        let pos = engine_read(|e| {
            let s = e.spec_rc();
            s.node(r.0.id).and_then(|n| n.tree_node().x.zip(n.tree_node().y))
        });
        if let Some((x, y)) = pos {
            tab().jump_to.set(Some((x, y)));
        }
        None
    }

    fn has_re_sort(&self) -> bool {
        true
    }

    /// :62-100 (`t_sort`, unstable upstream: a stable sort here, D13)
    fn re_sort(&self, l: &ListControl<ByRef<ReportEntry>>, column: usize) {
        let allocated = tab().power_state.allocated.get();
        re_sort_list(&mut l.list.borrow_mut(), column, allocated);
    }
}

fn re_sort_list(list: &mut [ByRef<ReportEntry>], column: usize, allocated: bool) {
    let compare = move |a: f64, b: f64| if allocated { a < b } else { a > b };
    let sort = pob_engine::lua::lua_sort::<ByRef<ReportEntry>>;
    match column {
        1 => sort(list, &mut |a, b| {
            if a.0.ty == b.0.ty {
                compare(a.0.power, b.0.power)
            } else {
                a.0.ty.as_str() < b.0.ty.as_str()
            }
        }),
        3 => sort(list, &mut |a, b| compare(a.0.power, b.0.power)),
        4 => sort(list, &mut |a, b| match (&a.0.path_dist, &b.0.path_dist) {
            // `a.pathDist == "Anoint" or a.pathDist == "Cluster"`: false; the `b` test comes second
            (ReportDist::Str(_), _) => false,
            (_, ReportDist::Str(_)) => true,
            (ReportDist::Num(x), ReportDist::Num(y)) => {
                if x == y {
                    compare(a.0.power, b.0.power)
                } else {
                    x < y
                }
            }
        }),
        5 => sort(list, &mut |a, b| match (&a.0.path_dist, &b.0.path_dist) {
            (ReportDist::Num(x), ReportDist::Num(y)) if a.0.path_power == b.0.path_power => x < y,
            _ => compare(a.0.path_power, b.0.path_power),
        }),
        _ => {}
    }
}

impl TreeTab {
    /// `self.controls.powerReportList:ReList()` (PowerReportListControl.lua:102-126).
    fn power_re_list(&self) {
        let st = &self.power_state;
        let mut out = Vec::new();
        for item in st.original_list.borrow().iter() {
            let r = &item.0;
            let mut insert = r.power > 0.0;
            if !st.show_clusters.get() && matches!(r.path_dist, ReportDist::Str("Cluster")) {
                insert = false;
            }
            if st.allocated.get() {
                insert = r.allocated;
            } else if r.allocated {
                insert = false;
            }
            if !st.show_masteries.get() && r.ty == pob_tree::NodeType::Mastery {
                insert = false;
            }
            if insert {
                out.push(item.clone());
            }
        }
        *self.power_report_list.list.borrow_mut() = out;
    }

    /// `SetReport(stat, report)` (:49-60).
    fn power_set_report(&self, stat: Option<&PowerStat>, report: Option<Vec<ReportEntry>>) {
        *self.power_state.power_label.borrow_mut() = stat.map_or_else(String::new, |s| s.label.clone());
        let has = report.is_some();
        *self.power_state.original_list.borrow_mut() = report.unwrap_or_default().into_iter().map(|r| ByRef(Rc::new(r))).collect();
        if stat.is_some_and(|s| s.stat.is_some()) {
            self.power_report_list.label.set(Some(
                if has { "Click to focus node on tree" } else { "Building Tree..." }.to_string(),
            ));
        } else {
            let l = self.power_state.power_label.borrow().clone();
            self.power_report_list
                .label
                .set(Some(format!("^7\"{l}\" not supported.  Select a specific stat from the dropdown.")));
        }
        self.power_re_list();
    }

    /// TreeTab.lua:1046-1058 `SetPowerCalc(powerStat)`.
    pub fn set_power_calc(&self, g: &Gfx, stat: Option<PowerStat>) {
        let b = main().build_mode.clone();
        self.set_show_heat_map(true);
        b.build_flag.set(true);
        self.power_build_flag.set(true);
        *self.power_stat.borrow_mut() = stat.clone();
        self.power_set_report(stat.as_ref(), None);
        if let Some(id) = self.power_toast.borrow_mut().take() {
            main().toasts.clear_dismissed(Some(&id));
            main().toasts.remove(g, &id, true);
        }
    }

    fn set_show_heat_map(&self, on: bool) {
        let b = main().build_mode.clone();
        if let Some(v) = b.tree_view.borrow_mut().as_mut() {
            v.show_heat_map = on;
        }
    }

    pub fn show_heat_map(&self) -> bool {
        main().build_mode.tree_view.borrow().as_ref().is_some_and(|v| v.show_heat_map)
    }

    /// `CalcsTab:BuildPower()` (CalcsTab.lua:475-493) over a background walk (UI-ARCH §4.4): `powerBuildFlag` starts it, a finished one calls
    /// `powerBuilderCallback`. The original's coroutine fills `node.power` while it runs; here the numbers arrive whole at the end.
    pub fn build_power(&self, g: &Gfx) {
        if self.power_build_flag.get() {
            self.power_build_flag.set(false);
            *self.power.borrow_mut() = None;
            let stat = self.power_stat.borrow().clone();
            let depth = self.node_power_max_depth.get();
            let threads = std::thread::available_parallelism().map_or(1, usize::from);
            let (model, live) = engine_read(|e| (e.model().clone(), e.live_spec_paths()));
            let progress = Arc::new(PowerProgress::default());
            *self.power_progress.borrow_mut() = progress.clone();
            *self.power_job.borrow_mut() = Some(std::thread::spawn(move || {
                pob_engine::tree_tab::power_thread(&model, &live, stat.as_ref(), depth, threads, &progress)
            }));
            self.power_slice_start.set(g.get_time());
        }
        // CalcsTab.lua:655-660 the walk reports after each 100 ms
        let now = g.get_time();
        if self.power_job.borrow().is_some() && now.saturating_sub(self.power_slice_start.get()) > 100 {
            self.power_slice_start.set(now);
            let percent = self.power_progress.borrow().percent();
            self.power_builder_progress(now, percent);
        }
        let done = self.power_job.borrow().as_ref().is_some_and(std::thread::JoinHandle::is_finished);
        if done {
            self.finish_power(g);
        }
    }

    /// TreeTab.lua:291-311 `powerBuilderProgressCallback(percent)`. `percent` is `None` only while the sessions of the background walk are being made, before it has a plan.
    pub fn power_builder_progress(&self, now: u64, percent: Option<i64>) {
        if now.saturating_sub(self.power_last_progress.get()) < 100 {
            return;
        }
        let message = match percent {
            Some(p) => pob_lua::format::format("Building Power Report... (%d%%)", &[pob_lua::format::Arg::Num(p as f64)]).expect("format"),
            None => "Building Power Report...".to_string(),
        };
        self.power_report_list.label.set(Some(message.clone()));
        let mut toast = self.power_toast.borrow_mut();
        if toast.as_deref().is_some_and(|id| main().toasts.was_dismissed(id)) {
            return;
        }
        self.power_last_progress.set(now);
        match toast.as_deref() {
            Some(id) if main().toasts.exists(id) => {
                main().toasts.update(id, &message);
            }
            _ => *toast = Some(main().toasts.add(&message)),
        }
    }

    /// The test harness and `done` above: the walk's end, `powerBuilderCallback` (TreeTab.lua:313-323).
    pub fn finish_power(&self, g: &Gfx) {
        let Some(job) = self.power_job.borrow_mut().take() else { return };
        let result = ok_or_panic(job.join().expect("power thread"));
        let Some(result) = result else { return };
        let stat = self.power_stat.borrow().clone();
        // `local powerStat = self.build.calcsTab.powerStat or data.powerStatList[1]`
        let report_stat = stat.clone().or_else(|| power_stat_list().into_iter().next());
        let mut report = ok_or_panic(engine_read(|e| e.power_report(report_stat.as_ref(), &result)));
        let order = self.report_order_sample.borrow();
        engine_read(|e| e.order_power_report(&mut report, report_stat.as_ref(), order.as_ref()));
        self.power_set_report(report_stat.as_ref(), Some(report));
        *self.power.borrow_mut() = Some(Rc::new(result));
        if let Some(id) = self.power_toast.borrow_mut().take() {
            main().toasts.clear_dismissed(Some(&id));
            main().toasts.remove(g, &id, false);
        }
    }

    /// The undo list of the active spec, made on first need (`ResetUndo` at the spec's load).
    fn undo_index(&self) -> usize {
        let (active, n) = engine_read(|e| (e.active_spec(), e.spec_count()));
        let mut u = self.undo.borrow_mut();
        if u.len() < n {
            u.resize_with(n, SpecUndo::default);
        }
        if u[active - 1].from_file && u[active - 1].undo.is_empty() {
            u[active - 1].undo.push(engine_read(pob_engine::engine::Engine::create_undo_state));
        }
        active - 1
    }

    /// `spec:AddUndoState()` (UndoHandler.lua:31-38): call it after the mutator; the state before is `undo[1]` already.
    pub fn add_undo_state(&self) {
        let i = self.undo_index();
        let s = engine_read(pob_engine::engine::Engine::create_undo_state);
        let mut u = self.undo.borrow_mut();
        u[i].undo.insert(0, s);
        u[i].undo.truncate(101);
        u[i].redo.clear();
        main().build_mode.mod_flag.set(true);
    }

    /// `ensure`: before a mutator, so the first state of the spec is the one before it.
    pub fn ensure_undo(&self) {
        let _ = self.undo_index();
    }

    /// UndoHandler.lua:41-47
    pub fn do_undo(&self) {
        let i = self.undo_index();
        let state = {
            let mut u = self.undo.borrow_mut();
            if u[i].undo.len() >= 2 {
                let top = u[i].undo.remove(0);
                u[i].redo.insert(0, top);
                Some(u[i].undo.remove(0))
            } else {
                None
            }
        };
        if let Some(s) = state {
            ok_or_panic(with_engine(|e| e.restore_undo_state(&s)));
            let now = engine_read(pob_engine::engine::Engine::create_undo_state);
            let mut u = self.undo.borrow_mut();
            u[i].undo.insert(0, now);
            u[i].undo.truncate(101);
            main().build_mode.mod_flag.set(true);
        }
    }

    /// UndoHandler.lua:50-55
    pub fn do_redo(&self) {
        let i = self.undo_index();
        let state = {
            let mut u = self.undo.borrow_mut();
            if u[i].redo.is_empty() { None } else { Some(u[i].redo.remove(0)) }
        };
        if let Some(s) = state {
            ok_or_panic(with_engine(|e| e.restore_undo_state(&s)));
            let now = engine_read(pob_engine::engine::Engine::create_undo_state);
            let mut u = self.undo.borrow_mut();
            u[i].undo.insert(0, now);
            u[i].undo.truncate(101);
            main().build_mode.mod_flag.set(true);
        }
    }

    /// TreeTab.lua:548-584 `SetActiveSpec(specId)`.
    pub fn set_active_spec(&self, g: &mut Gfx, spec_id: usize) {
        let b = main().build_mode.clone();
        ok_or_panic(with_engine(|e| e.set_active_spec(spec_id)));
        b.build_flag.set(true);
        self.sync_active_spec(g);
    }

    /// The part of `SetActiveSpec` after `build.spec` changed.
    pub fn sync_active_spec(&self, g: &mut Gfx) {
        let b = main().build_mode.clone();
        let ver = engine_read(|e| e.spec_rc().tree_version.clone());
        self.show_convert.set(!ver.starts_with(latest_tree_version()));
        b.after_spec_change(g);
        // :573-578 `if itemOrderList[1] then itemsTab:PopulateSlots()`, `specSelect.selIndex = specId`
        let items_tab = b.items_tab.borrow().clone();
        if let Some(it) = items_tab {
            if !engine_read(|e| e.item_order().is_empty()) {
                it.populate_slots(g);
            }
            let spec_select = it.spec_select.borrow().clone();
            if let Some(s) = spec_select {
                s.sel_index.set(engine_read(pob_engine::engine::Engine::active_spec));
            }
        }
        // `versionSelect:SelByValue(curSpec.treeVersion, 'value')`
        let ix = self
            .version_select
            .list
            .borrow()
            .iter()
            .position(|v| v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).is_some_and(|s| *s == ver));
        if let Some(i) = ix {
            self.version_select.sel_index.set(i + 1);
        }
        *self.compare.borrow_mut() = None;
    }

    /// TreeTab.lua:586-591 `SetCompareSpec(specId)`.
    pub fn set_compare_spec(&self, spec_id: usize) {
        let n = engine_read(pob_engine::engine::Engine::spec_count);
        self.active_compare_spec.set(spec_id.min(n));
        *self.compare.borrow_mut() = None;
    }

    /// `self.isComparing and self.specList[self.activeCompareSpec] or nil`.
    pub fn compare_spec(&self) -> Option<Rc<pob_tree::PassiveSpec>> {
        if !self.is_comparing.get() {
            return None;
        }
        let i = self.active_compare_spec.get();
        let active = engine_read(pob_engine::engine::Engine::active_spec);
        if i == active {
            return Some(engine_read(pob_engine::engine::Engine::spec_rc));
        }
        if let Some((c, s)) = &*self.compare.borrow()
            && *c == i
        {
            return Some(s.clone());
        }
        let s = Rc::new(ok_or_panic(engine_read(|e| e.load_spec(i))));
        *self.compare.borrow_mut() = Some((i, s.clone()));
        Some(s)
    }

    /// The Lua keeps one object per spec: a list edit invalidates the index-keyed cache.
    pub fn invalidate_compare(&self) {
        *self.compare.borrow_mut() = None;
    }

    /// TreeTab.lua:593-617 + `self.modFlag = true`; `success`: the message popup.
    fn convert_to_version(&self, g: &mut Gfx, version: &str, remove: bool, success: bool, ignore_sub_type: bool) {
        let b = main().build_mode.clone();
        let active = engine_read(pob_engine::engine::Engine::active_spec);
        let used = ok_or_panic(with_engine(|e| e.convert_to_version(version, remove, ignore_sub_type)));
        {
            let mut u = self.undo.borrow_mut();
            let need = u.len().max(active);
            u.resize_with(need, SpecUndo::default);
            u.insert(active, SpecUndo::default());
            if remove {
                u.remove(active - 1);
            }
        }
        b.build_flag.set(true);
        b.mod_flag.set(true);
        self.sync_active_spec(g);
        if success {
            main().open_message_popup(
                g,
                "Tree Converted",
                &format!(
                    "The tree has been converted to {}.\nNote that some or all of the passives may have been de-allocated due to changes in the tree.\n\nYou can switch back to the old tree using the tree selector at the bottom left.",
                    tree_version_display(&used)
                ),
            );
        }
    }

    /// `getLatestTreeVersion` (:329-331).
    fn latest_for_active(&self) -> String {
        let latest = latest_tree_version();
        let cur = engine_read(|e| e.model().tree.specs[e.active_spec() - 1].tree_version.clone());
        match cur.strip_prefix(latest) {
            Some(rest) => format!("{latest}{rest}"),
            None => latest.to_string(),
        }
    }
}

/// `TreeTabClass:TreeTab(build)` (:25-348).
#[allow(clippy::too_many_lines)]
pub fn new_tree_tab(g: &mut Gfx) -> Rc<TreeTab> {
    let anchor_controls = BareControl::new(None, rect(0.0, 0.0, 0.0, 20.0));
    let ac: ControlRef = anchor_controls.clone();
    let spec_select = DropDownControl::new(
        g,
        anchor("LEFT", &ac, "RIGHT"),
        rect(0.0, 0.0, 190.0, 20.0),
        Vec::new(),
        Some(Rc::new(|g: &mut Gfx, index, _v: &DropItem| {
            let t = tab();
            if index <= engine_read(pob_engine::engine::Engine::spec_count) {
                main().build_mode.mod_flag.set(true);
                t.set_active_spec(g, index);
            } else {
                open_spec_manage_popup(g);
            }
        })),
        None,
        false,
    );
    spec_select.max_dropped_width.set(1000.0);
    spec_select.enable_dropped_width.set(true);
    spec_select.enable_change_box_width.set(true);
    spec_select.scroll_bar.base.enabled.set(true);
    spec_select.tip.set_tooltip_func(spec_select_tooltip);
    let ss: ControlRef = spec_select.clone();
    let compare_check = CheckBoxControl::new(
        g,
        anchor("LEFT", &ss, "RIGHT"),
        [74.0, 0.0, 20.0],
        Some("Compare:"),
        Some(Rc::new(|_g: &mut Gfx, state| {
            let t = tab();
            t.is_comparing.set(state);
            t.set_compare_spec(t.active_compare_spec.get());
            t.compare_select.base.shown.set(state);
            let (sel, chk): (ControlRef, ControlRef) = (t.compare_select.clone(), t.compare_check.clone());
            let target = if state { &sel } else { &chk };
            t.reset.base.set_anchor("LEFT", Some(Rc::downgrade(target)), "RIGHT", None, false);
        })),
        None,
        false,
    );
    let cc_r: ControlRef = compare_check.clone();
    let compare_select = DropDownControl::new(
        g,
        anchor("LEFT", &cc_r, "RIGHT"),
        rect(8.0, 0.0, 190.0, 20.0),
        Vec::new(),
        Some(Rc::new(|g: &mut Gfx, index, _v: &DropItem| {
            let t = tab();
            if index <= engine_read(pob_engine::engine::Engine::spec_count) {
                t.set_compare_spec(index);
            } else {
                open_spec_manage_popup(g);
            }
        })),
        None,
        false,
    );
    compare_select.base.shown.set(false);
    compare_select.max_dropped_width.set(1000.0);
    compare_select.enable_dropped_width.set(true);
    compare_select.enable_change_box_width.set(true);
    let reset = ButtonControl::new(
        anchor("LEFT", &cc_r, "RIGHT"),
        rect(8.0, 0.0, 145.0, 20.0),
        "Reset Tree/Tattoos".to_string(),
        open_reset_popup,
    );
    let r_r: ControlRef = reset.clone();
    let version_text = LabelControl::new(anchor("LEFT", &r_r, "RIGHT"), rect(8.0, 0.0, 0.0, 16.0), "^7Version:".to_string());
    let vt_r: ControlRef = version_text.clone();
    let versions: Vec<DropItem> = pob_data::tree::tree_versions()
        .expect("GameVersions")
        .tree_versions
        .iter()
        .map(|v| {
            let mut d = DropItem::table(&v.display);
            d.data = Some(Rc::new(v.ver.clone()));
            d
        })
        .collect();
    let n_versions = versions.len();
    let version_select = DropDownControl::new(
        g,
        anchor("LEFT", &vt_r, "RIGHT"),
        rect(8.0, 0.0, 100.0, 20.0),
        versions,
        Some(Rc::new(|g: &mut Gfx, _i, v: &DropItem| {
            let ver = v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned().expect("version");
            if ver != engine_read(|e| e.spec_rc().tree_version.clone()) {
                open_version_convert_popup(g, &ver, true);
            }
        })),
        None,
        false,
    );
    version_select.max_dropped_width.set(1000.0);
    version_select.enable_dropped_width.set(true);
    version_select.enable_change_box_width.set(true);
    version_select.check_dropped_width(g, true);
    version_select.sel_index.set(n_versions);
    let vs_r: ControlRef = version_select.clone();
    let tree_search = EditControl::new(
        g,
        anchor("LEFT", &vs_r, "RIGHT"),
        rect(8.0, 0.0, if main().portrait_mode.get() { 200.0 } else { 300.0 }, 20.0),
        EditArgs {
            init: Some(String::new()),
            prompt: Some("Search".into()),
            filter: Some("%c".into()),
            limit: Some(100),
            change_func: Some(Rc::new(|_g: &mut Gfx, buf: &str, _| {
                let b = main().build_mode.clone();
                if let Some(v) = b.tree_view.borrow_mut().as_mut() {
                    v.search_str = buf.to_string();
                }
            })),
            clearable: true,
            ..Default::default()
        },
    );
    tree_search.tip.tooltip_text.set(Some(
        "Uses Lua pattern matching for complex searches.\nPrefix your search with \"oil:\" to search by anoint recipe.\nTo search for multiple terms: (increased.fire.damage|increased.area.of.effect|etc)"
            .to_string(),
    ));
    let ts_r: ControlRef = tree_search.clone();
    let find_timeless_jewel = ButtonControl::new(
        anchor("LEFT", &ts_r, "RIGHT"),
        rect(8.0, 0.0, 150.0, 20.0),
        "Find Timeless Jewel".to_string(),
        crate::tabs::timeless::find_timeless_jewel,
    );
    let ft_r: ControlRef = find_timeless_jewel.clone();
    let tree_heat_map = CheckBoxControl::new(
        g,
        anchor("LEFT", &ft_r, "RIGHT"),
        [130.0, 0.0, 20.0],
        Some("Show Node Power:"),
        Some(Rc::new(|g: &mut Gfx, state| {
            let t = tab();
            t.set_show_heat_map(state);
            t.tree_heat_map_stat_select.base.shown.set(state);
            // :202-207
            if !state {
                t.power_report_list.base.shown.set(false);
                let id = t.power_toast.borrow().clone();
                if let Some(id) = id
                    && main().toasts.exists(&id)
                {
                    main().toasts.remove(g, &id, false);
                }
            }
        })),
        None,
        false,
    );
    let hm_r: ControlRef = tree_heat_map.clone();
    let depth_items = vec![
        DropItem::str("All"),
        DropItem::str("5"),
        DropItem::str("10"),
        DropItem::str("15"),
        DropItem::str("Custom"),
    ];
    let node_power_max_depth_select = DropDownControl::new(
        g,
        anchor("LEFT", &hm_r, "RIGHT"),
        rect(8.0, 0.0, 55.0, 20.0),
        depth_items,
        Some(Rc::new(|g: &mut Gfx, _i, v: &DropItem| {
            let t = tab();
            let value = v.label.clone().unwrap_or_default();
            t.is_custom_max_depth.set(value == "Custom");
            if value == "Custom" {
                t.node_power_max_depth_select.base.width.set(70.0);
                t.node_power_max_depth_custom.base.shown.set(true);
                let c: ControlRef = t.node_power_max_depth_custom.clone();
                t.tree_heat_map_stat_select
                    .base
                    .set_anchor("LEFT", Some(Rc::downgrade(&c)), "RIGHT", None, false);
                return;
            }
            t.node_power_max_depth_select.base.width.set(55.0);
            t.node_power_max_depth_custom.base.shown.set(false);
            let c: ControlRef = t.node_power_max_depth_select.clone();
            t.tree_heat_map_stat_select
                .base
                .set_anchor("LEFT", Some(Rc::downgrade(&c)), "RIGHT", None, false);
            let old_max = t.node_power_max_depth.get();
            let new_max = value.parse::<f64>().ok();
            t.node_power_max_depth.set(new_max);
            // :235-239 `oldMax ~= value`: a number against the number, nil against "All"
            if old_max != new_max
                && t.show_heat_map()
                && let Some(o) = old_max
                && new_max.is_none_or(|n| n > o)
            {
                let st = t.power_stat.borrow().clone();
                t.set_power_calc(g, st);
            }
        })),
        Some("Limit of Node distance to search (lower = faster)"),
        false,
    );
    let nd_r: ControlRef = node_power_max_depth_select.clone();
    let node_power_max_depth_custom = EditControl::new(
        g,
        anchor("LEFT", &nd_r, "RIGHT"),
        rect(8.0, 0.0, 70.0, 20.0),
        EditArgs {
            init: Some("0".into()),
            filter: Some("%D".into()),
            change_func: Some(Rc::new(|g: &mut Gfx, buf: &str, _| {
                let t = tab();
                t.node_power_max_depth.set(buf.parse::<f64>().ok());
                if t.show_heat_map() {
                    let st = t.power_stat.borrow().clone();
                    t.set_power_calc(g, st);
                }
            })),
            ..Default::default()
        },
    );
    node_power_max_depth_custom.base.shown.set(false);
    let tree_heat_map_stat_select = DropDownControl::new(
        g,
        anchor("LEFT", &nd_r, "RIGHT"),
        rect(8.0, 0.0, 150.0, 20.0),
        Vec::new(),
        Some(Rc::new(|g: &mut Gfx, _i, v: &DropItem| {
            let st = v.data.as_ref().and_then(|d| d.downcast_ref::<PowerStat>()).cloned();
            tab().set_power_calc(g, st);
        })),
        None,
        false,
    );
    tree_heat_map.tip.tooltip_text.set_fn(|_| {
        let theme = main().s.borrow().node_power_theme.clone();
        let (off, def) = theme.split_once('/').unwrap_or(("RED", "BLUE"));
        Some(format!(
            "When enabled, an estimate of the offensive and defensive strength of\neach unallocated passive is calculated and displayed visually.\nOffensive power shows as {}, defensive power as {}.",
            off.to_lowercase(),
            def.to_lowercase()
        ))
    });
    let hs_r: ControlRef = tree_heat_map_stat_select.clone();
    let power_report = ButtonControl::new(
        anchor("LEFT", &hs_r, "RIGHT"),
        rect(8.0, 0.0, 150.0, 20.0),
        crate::control::Prop::func(|g: &Gfx| {
            if tab().power_report_list.is_shown(g) {
                "Hide Power Report".to_string()
            } else {
                "Show Power Report".to_string()
            }
        }),
        |g| {
            let t = tab();
            let s = t.power_report_list.is_shown(g);
            t.power_report_list.base.shown.set(!s);
        },
    );
    // :277-286 PowerReportListControl
    let y_pos = 24.0;
    let width = 700.0;
    let power_report_list: Rc<ListControl<ByRef<ReportEntry>>> = ListControl::new(
        anchor("TOPLEFT", &ss, "BOTTOMLEFT"),
        rect(0.0, y_pos, width, 170.0),
        16.0,
        Some("VERTICAL"),
        false,
        Vec::new(),
        false,
        Box::new(PowerHooks),
    );
    {
        let col = |w: f64, label: crate::control::Prop<String>, sortable: bool| Column {
            width: Some(w.into()),
            label: Some(label),
            sortable,
            ..Column::default()
        };
        *power_report_list.col_list.borrow_mut() = vec![
            col(width * 0.15, "Type".to_string().into(), true),
            col(width * 0.45, "Node Name".to_string().into(), false),
            col(
                width * 0.16,
                crate::control::Prop::func(|_| tab().power_state.power_label.borrow().clone()),
                true,
            ),
            col(width * 0.05, "Points".to_string().into(), true),
            col(width * 0.16, "Per Point".to_string().into(), true),
        ];
    }
    power_report_list.col_labels.set(true);
    power_report_list.label.set(Some("Building Tree...".to_string()));
    let prl: ControlRef = power_report_list.clone();
    let power_filter_select = DropDownControl::new(
        g,
        anchor("BOTTOMRIGHT", &prl, "TOPRIGHT"),
        rect(0.0, -2.0, 200.0, 20.0),
        vec![
            DropItem::str("Show Unallocated"),
            DropItem::str("Show Unallocated & Clusters"),
            DropItem::str("Show Allocated"),
        ],
        Some(Rc::new(|_g: &mut Gfx, index, _v: &DropItem| {
            let t = tab();
            t.power_state.show_clusters.set(index == 2);
            t.power_state.allocated.set(index == 3);
            t.power_re_list();
            re_sort_list(&mut t.power_report_list.list.borrow_mut(), 3, index == 3);
        })),
        None,
        false,
    );
    power_report_list.host.add("filterSelect", power_filter_select.clone());
    let fs_r: ControlRef = power_filter_select.clone();
    let mastery_check = CheckBoxControl::new(
        g,
        anchor("RIGHT", &fs_r, "LEFT"),
        [-120.0, 0.0, 18.0],
        Some("Show Masteries:"),
        Some(Rc::new(|_g: &mut Gfx, state| {
            let t = tab();
            t.power_state.show_masteries.set(state);
            t.power_re_list();
            let a = t.power_state.allocated.get();
            re_sort_list(&mut t.power_report_list.list.borrow_mut(), 3, a);
        })),
        None,
        true,
    );
    power_report_list.host.add("masteryCheck", mastery_check);
    power_report_list.base.shown.set(false);

    // :325-343
    let spec_convert_text = LabelControl::new(
        anchor("BOTTOMLEFT", &ss, "TOPLEFT"),
        rect(0.0, -14.0, 0.0, 16.0),
        "^7This is an older tree version, which may not be fully compatible with the current game version.".to_string(),
    );
    spec_convert_text.base.shown.set_fn(|_| tab().show_convert.get());
    let sc_r: ControlRef = spec_convert_text.clone();
    let convert_label = || format!("{}Convert to {}", cc("POSITIVE"), tree_version_display(&tab().latest_for_active()));
    let convert_all_label = || {
        format!(
            "{}Convert all trees to {}",
            cc("POSITIVE"),
            tree_version_display(&tab().latest_for_active())
        )
    };
    let spec_convert = ButtonControl::new(
        anchor("LEFT", &sc_r, "RIGHT"),
        [
            8.0.into(),
            0.0.into(),
            crate::control::Prop::func(move |g: &Gfx| sg::draw_string_width(g, 16.0, "VAR", &convert_label()) + 20.0),
            20.0.into(),
        ],
        crate::control::Prop::func(move |_: &Gfx| convert_label()),
        |g| {
            let t = tab();
            let v = t.latest_for_active();
            t.convert_to_version(g, &v, false, true, false);
        },
    );
    let scv: ControlRef = spec_convert.clone();
    let spec_convert_all = ButtonControl::new(
        anchor("LEFT", &scv, "RIGHT"),
        [
            8.0.into(),
            0.0.into(),
            crate::control::Prop::func(move |g: &Gfx| sg::draw_string_width(g, 16.0, "VAR", &convert_all_label()) + 20.0),
            20.0.into(),
        ],
        crate::control::Prop::func(move |_: &Gfx| convert_all_label()),
        |g| {
            let v = tab().latest_for_active();
            open_version_convert_all_popup(g, &v);
        },
    );
    let t = Rc::new(TreeTab {
        host: ControlHost::new(),
        is_comparing: Cell::new(false),
        is_custom_max_depth: Cell::new(false),
        active_compare_spec: Cell::new(1),
        show_convert: Cell::new(false),
        jump_to: Cell::new(None),
        anchor_controls,
        spec_select,
        compare_check,
        compare_select,
        reset,
        version_text,
        version_select,
        tree_search,
        find_timeless_jewel,
        tree_heat_map,
        node_power_max_depth_select,
        node_power_max_depth_custom,
        tree_heat_map_stat_select,
        power_report,
        power_report_list,
        power_filter_select,
        spec_convert_text,
        spec_convert,
        spec_convert_all,
        undo: RefCell::new(
            (0..engine_read(pob_engine::engine::Engine::spec_count))
                .map(|_| SpecUndo {
                    from_file: matches!(&*main().build_mode.db_file_name.borrow(), crate::modules::build::DbFile::Path(_)),
                    ..SpecUndo::default()
                })
                .collect(),
        ),
        default_tattoo: RefCell::new(HashMap::new()),
        show_legacy_tattoo: Cell::new(false),
        power_stat: RefCell::new(None),
        node_power_max_depth: Cell::new(None),
        // the load's `CalcsTab:BuildOutput` set it
        power_build_flag: Cell::new(true),
        power_job: RefCell::new(None),
        power_progress: RefCell::new(Arc::default()),
        power: RefCell::new(None),
        power_toast: RefCell::new(None),
        power_slice_start: Cell::new(0),
        power_last_progress: Cell::new(0),
        power_state: PowerListState {
            show_masteries: Cell::new(true),
            ..PowerListState::default()
        },
        timeless: crate::tabs::timeless::TimelessPersist::default(),
        report_order_sample: RefCell::new(None),
        compare: RefCell::new(None),
        summaries: RefCell::new(Vec::new()),
    });
    t.tree_heat_map_stat_select.base.shown.set(false);
    for (name, c) in [
        ("specSelect", t.spec_select.clone() as ControlRef),
        ("compareCheck", t.compare_check.clone()),
        ("compareSelect", t.compare_select.clone()),
        ("reset", t.reset.clone()),
        ("versionText", t.version_text.clone()),
        ("versionSelect", t.version_select.clone()),
        ("treeSearch", t.tree_search.clone()),
        ("findTimelessJewel", t.find_timeless_jewel.clone()),
        ("treeHeatMap", t.tree_heat_map.clone()),
        ("nodePowerMaxDepthSelect", t.node_power_max_depth_select.clone()),
        ("nodePowerMaxDepthCustom", t.node_power_max_depth_custom.clone()),
        ("treeHeatMapStatSelect", t.tree_heat_map_stat_select.clone()),
        ("powerReport", t.power_report.clone()),
        ("powerReportList", t.power_report_list.clone()),
        ("specConvertText", t.spec_convert_text.clone()),
        ("specConvert", t.spec_convert.clone()),
        ("specConvertAll", t.spec_convert_all.clone()),
    ] {
        t.host.add(name, c);
    }
    t
}

/// `specSelect.tooltipFunc` (:54-106).
fn spec_select_tooltip(tooltip: &mut Tooltip, g: &Gfx, (mode, sel_index, _): (&'static str, usize, Option<DropItem>)) {
    tooltip.clear(false);
    if mode == "OUT" {
        return;
    }
    let summaries = ok_or_panic(engine_read(pob_engine::engine::Engine::spec_summaries));
    let Some(spec) = summaries.get(sel_index.wrapping_sub(1)) else {
        return;
    };
    tooltip.add_line(g, 16.0, Some(&format!("Class: {}", spec.class_name)));
    tooltip.add_line(g, 16.0, Some(&format!("Ascendancy: {}", spec.ascend_class_name)));
    tooltip.add_line(g, 16.0, Some(&format!("Points used: {}", spec.used)));
    if spec.sockets > 0 {
        tooltip.add_line(g, 16.0, Some(&format!("Jewel sockets: {}", spec.sockets)));
    }
    let active = engine_read(pob_engine::engine::Engine::active_spec);
    if sel_index != active {
        let lines = ok_or_panic(engine_read(|e| e.spec_stat_compare(sel_index)));
        for l in lines {
            tooltip.add_line(g, 14.0, Some(&l));
        }
        let (respec, respec_asc, gold) = ok_or_panic(engine_read(|e| e.respec_counts(sel_index)));
        if respec > 0 || respec_asc > 0 {
            let total = f64::from(respec) * gold + f64::from(respec_asc) * gold * 5.0;
            let s = engine_read(|e| pob_engine::display::format_num_sep(&pob_lua::tostring_num(total), e.settings()));
            tooltip.add_line(g, 16.0, Some(&format!("^xFFD700{s} Gold ^7required to switch to this tree.")));
            if respec > 0 {
                let w = if respec == 1 {
                    "Passive node to be refunded"
                } else {
                    "Passive nodes to be refunded"
                };
                tooltip.add_line(g, 16.0, Some(&format!("^7\t{respec} {w}.")));
            }
            if respec_asc > 0 {
                let w = if respec_asc == 1 {
                    "Ascendancy node to be refunded"
                } else {
                    "Ascendancy nodes to be refunded"
                };
                tooltip.add_line(g, 16.0, Some(&format!("^7\t{respec_asc} {w}.")));
            }
        }
    }
    tooltip.add_line(
        g,
        16.0,
        Some(&format!("^7Game Version: {}", tree_version_display(&spec.tree_version))),
    );
}

/// `reset` button (:132-157).
fn open_reset_popup(g: &mut Gfx) {
    let button_y = 65.0;
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert(
        "warningLabel".into(),
        LabelControl::new(
            None,
            rect(0.0, 30.0, 0.0, 16.0),
            "^7Warning: resetting your passive tree or removing all tattoos cannot be undone.\n".to_string(),
        ),
    );
    controls.insert(
        "reset".into(),
        ButtonControl::new(None, rect(-130.0, button_y, 100.0, 20.0), "Reset Tree".to_string(), |_g| {
            let t = tab();
            t.ensure_undo();
            ok_or_panic(with_engine(pob_engine::engine::Engine::reset_tree));
            t.add_undo_state();
            main().build_mode.build_flag.set(true);
            main().close_popup();
        }),
    );
    controls.insert(
        "removeTattoo".into(),
        ButtonControl::new(None, rect(0.0, button_y, 144.0, 20.0), "Remove All Tattoos".to_string(), |_g| {
            ok_or_panic(with_engine(pob_engine::engine::Engine::remove_all_tattoos));
            let b = main().build_mode.clone();
            b.mod_flag.set(true);
            b.build_flag.set(true);
            main().close_popup();
        }),
    );
    controls.insert(
        "cancel".into(),
        ButtonControl::new(None, rect(130.0, button_y, 100.0, 20.0), "Cancel".to_string(), |_g| {
            main().close_popup()
        }),
    );
    // `main:OpenPopup(570, 100, "Reset Tree/Tattoos", controls, nil, "edit", "cancel")`: there is no `edit`
    main().open_popup(g, 570.0, 100.0, "Reset Tree/Tattoos", controls, None, None, Some("cancel"), None);
}

/// `OpenVersionConvertPopup` (:654-671).
pub(crate) fn open_version_convert_popup(g: &mut Gfx, version: &str, ignore_sub_type: bool) {
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert(
        "warningLabel".into(),
        LabelControl::new(
            None,
            rect(0.0, 20.0, 0.0, 16.0),
            "^7Warning: some or all of the passives may be de-allocated due to changes in the tree.\n\nConvert will replace your current tree.\nCopy + Convert will backup your current tree.\n".to_string(),
        ),
    );
    let v = version.to_string();
    controls.insert(
        "convert".into(),
        ButtonControl::new(None, rect(-125.0, 105.0, 100.0, 20.0), "Convert".to_string(), move |g| {
            tab().convert_to_version(g, &v, true, false, ignore_sub_type);
            main().close_popup();
        }),
    );
    let v = version.to_string();
    controls.insert(
        "convertCopy".into(),
        ButtonControl::new(None, rect(0.0, 105.0, 125.0, 20.0), "Copy + Convert".to_string(), move |g| {
            tab().convert_to_version(g, &v, false, false, ignore_sub_type);
            main().close_popup();
        }),
    );
    controls.insert(
        "cancel".into(),
        ButtonControl::new(None, rect(125.0, 105.0, 100.0, 20.0), "Cancel".to_string(), |_g| {
            let t = tab();
            let ver = engine_read(|e| e.spec_rc().tree_version.clone());
            let ix = t
                .version_select
                .list
                .borrow()
                .iter()
                .position(|v| v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).is_some_and(|s| *s == ver));
            if let Some(i) = ix {
                t.version_select.sel_index.set(i + 1);
            }
            main().close_popup();
        }),
    );
    main().open_popup(
        g,
        570.0,
        140.0,
        &format!("Convert to Version {}", tree_version_display(version)),
        controls,
        Some("convert"),
        None,
        None,
        None,
    );
}

/// `OpenVersionConvertAllPopup` (:673-685).
fn open_version_convert_all_popup(g: &mut Gfx, version: &str) {
    let display = tree_version_display(version);
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert(
        "warningLabel".into(),
        LabelControl::new(
            None,
            rect(0.0, 20.0, 0.0, 16.0),
            format!(
                "^7Warning: some or all of the passives may be de-allocated due to changes in the tree.\n\nConvert will replace all trees that are not Version {display}.\nThis action cannot be undone.\n"
            ),
        ),
    );
    let v = version.to_string();
    controls.insert(
        "convert".into(),
        ButtonControl::new(None, rect(-58.0, 105.0, 100.0, 20.0), "Convert".to_string(), move |g| {
            let t = tab();
            ok_or_panic(with_engine(|e| e.convert_all_to_version(&v)));
            t.undo.borrow_mut().clear();
            let b = main().build_mode.clone();
            b.build_flag.set(true);
            b.mod_flag.set(true);
            t.sync_active_spec(g);
            main().close_popup();
        }),
    );
    controls.insert(
        "cancel".into(),
        ButtonControl::new(None, rect(58.0, 105.0, 100.0, 20.0), "Cancel".to_string(), |_g| {
            main().close_popup()
        }),
    );
    main().open_popup(
        g,
        570.0,
        140.0,
        &format!("Convert all to Version {display}"),
        controls,
        Some("convert"),
        None,
        None,
        None,
    );
}

// --- PassiveSpecListControl --------------------------------------------------------------------------------------------------------------

type SpecRow = ByRef<()>;

struct SpecListHooks;

fn spec_list_of() -> Vec<SpecRow> {
    (0..engine_read(pob_engine::engine::Engine::spec_count))
        .map(|_| ByRef(Rc::new(())))
        .collect()
}

fn refresh_summaries() {
    *tab().summaries.borrow_mut() = ok_or_panic(engine_read(pob_engine::engine::Engine::spec_summaries));
}

/// `UpdateItemsTabPassiveTreeDropdown` touches the items tab (U6); `build:SyncLoadouts()`.
fn after_list_change(g: &mut Gfx) {
    tab().invalidate_compare();
    refresh_summaries();
    main().build_mode.sync_loadouts_pub(g);
}

impl ListHooks<SpecRow> for SpecListHooks {
    /// PassiveSpecListControl.lua:76-84
    fn get_row_value(&self, _l: &ListControl<SpecRow>, _g: &Gfx, column: usize, index: usize, _v: &SpecRow) -> String {
        if column != 1 {
            return String::new();
        }
        let t = tab();
        let s = t.summaries.borrow();
        let Some(spec) = s.get(index - 1) else { return String::new() };
        let ver = if spec.tree_version == latest_tree_version() {
            String::new()
        } else {
            format!("[{}] ", tree_version_display(&spec.tree_version))
        };
        let class = if spec.ascend_class_name == "None" {
            &spec.class_name
        } else {
            &spec.ascend_class_name
        };
        let current = if index == engine_read(pob_engine::engine::Engine::active_spec) {
            "  ^9(Current)"
        } else {
            ""
        };
        format!(
            "{ver}{} ({class}, {} points){current}",
            spec.title.as_deref().unwrap_or("Default"),
            spec.used
        )
    }

    /// :86-91
    fn on_order_change(&self, _l: &ListControl<SpecRow>, old_index: usize, new_index: usize) {
        with_engine(|e| e.move_spec(old_index, new_index));
        let t = tab();
        {
            let mut u = t.undo.borrow_mut();
            let n = u.len().max(old_index).max(new_index);
            u.resize_with(n, SpecUndo::default);
            let x = u.remove(old_index - 1);
            u.insert(new_index - 1, x);
        }
        main().build_mode.mod_flag.set(true);
        t.invalidate_compare();
        refresh_summaries();
    }

    /// :93-97
    fn on_sel_click(&self, _l: &ListControl<SpecRow>, g: &mut Gfx, index: usize, _v: &SpecRow, double_click: bool) -> Option<bool> {
        if double_click && index != engine_read(pob_engine::engine::Engine::active_spec) {
            tab().set_active_spec(g, index);
        }
        None
    }

    /// :99-115
    fn on_sel_delete(&self, l: &ListControl<SpecRow>, g: &mut Gfx, index: usize, _v: &SpecRow) {
        if l.list.borrow().len() > 1 {
            confirm_delete_spec(g, index);
        }
    }

    /// :117-121
    fn on_sel_key_down(&self, _l: &ListControl<SpecRow>, g: &mut Gfx, index: usize, _v: &SpecRow, key: &str) {
        if key == "F2" {
            rename_spec_popup(g, RenameTarget::Existing(index), "Rename Tree");
        }
    }
}

thread_local! {
    static SPEC_LIST: RefCell<Weak<ListControl<SpecRow>>> = const { RefCell::new(Weak::new()) };
}

fn spec_list() -> Rc<ListControl<SpecRow>> {
    SPEC_LIST.with_borrow(Weak::upgrade).expect("spec list")
}

fn confirm_delete_spec(g: &mut Gfx, index: usize) {
    let title = tab()
        .summaries
        .borrow()
        .get(index - 1)
        .and_then(|s| s.title.clone())
        .unwrap_or_else(|| "Default".into());
    main().open_confirm_popup(
        g,
        "Delete Tree",
        &format!("Are you sure you want to delete '{title}'?"),
        "Delete",
        Rc::new(move |g: &mut Gfx| {
            let l = spec_list();
            l.list.borrow_mut().remove(index - 1);
            l.sel_index.set(None);
            *l.sel_value.borrow_mut() = None;
            let t = tab();
            {
                let mut u = t.undo.borrow_mut();
                if u.len() >= index {
                    u.remove(index - 1);
                }
            }
            let was_active = ok_or_panic(with_engine(|e| e.delete_spec(index)));
            if was_active {
                main().build_mode.build_flag.set(true);
                t.sync_active_spec(g);
            }
            main().build_mode.mod_flag.set(true);
            after_list_change(g);
        }),
        None,
    );
}

enum RenameTarget {
    Existing(usize),
    New(Box<pob_build::SpecModel>),
}

/// `RenameSpec(spec, title, addOnName)` (:50-74).
fn rename_spec_popup(g: &mut Gfx, target: RenameTarget, title: &str) {
    let init = match &target {
        RenameTarget::Existing(i) => tab().summaries.borrow().get(*i - 1).and_then(|s| s.title.clone()),
        RenameTarget::New(m) => m.title.clone(),
    };
    let target = Rc::new(RefCell::new(Some(target)));
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert(
        "label".into(),
        LabelControl::new(None, rect(0.0, 20.0, 0.0, 16.0), "^7Enter name for this passive tree:".to_string()),
    );
    let save_slot: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
    let slot = save_slot.clone();
    let edit = EditControl::new(
        g,
        None,
        rect(0.0, 40.0, 350.0, 20.0),
        EditArgs {
            init,
            limit: Some(100),
            change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                if let Some(s) = &*slot.borrow() {
                    s.base.enabled.set(buf.bytes().any(|c| !c.is_ascii_whitespace()));
                }
            })),
            ..Default::default()
        },
    );
    let e2 = edit.clone();
    let save = ButtonControl::new(None, rect(-45.0, 70.0, 80.0, 20.0), "Save".to_string(), move |g| {
        let text = e2.buf.borrow().clone();
        let b = main().build_mode.clone();
        match target.borrow_mut().take() {
            Some(RenameTarget::Existing(i)) => with_engine(|e| e.rename_spec(i, &text)),
            Some(RenameTarget::New(mut m)) => {
                m.title = Some(text);
                with_engine(|e| e.push_spec(*m));
                let l = spec_list();
                l.list.borrow_mut().push(ByRef(Rc::new(())));
                let n = l.list.borrow().len();
                l.sel_index.set(Some(n));
                let last = l.list.borrow().last().cloned();
                *l.sel_value.borrow_mut() = last;
            }
            None => {}
        }
        b.mod_flag.set(true);
        after_list_change(g);
        main().close_popup();
    });
    save.base.enabled.set(false);
    *save_slot.borrow_mut() = Some(save.clone());
    controls.insert("edit".into(), edit);
    controls.insert("save".into(), save);
    controls.insert(
        "cancel".into(),
        ButtonControl::new(None, rect(45.0, 70.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup()),
    );
    main().open_popup(g, 370.0, 100.0, title, controls, Some("save"), Some("edit"), None, None);
}

/// `OpenSpecManagePopup` (:634-652) + `PassiveSpecListControl` (:13-48).
pub fn open_spec_manage_popup(g: &mut Gfx) {
    refresh_summaries();
    let list = ListControl::new(
        None,
        rect(0.0, 50.0, 350.0, 200.0),
        16.0,
        Some("VERTICAL"),
        true,
        spec_list_of(),
        false,
        Box::new(SpecListHooks),
    );
    SPEC_LIST.with_borrow_mut(|w| *w = Rc::downgrade(&list));
    let lr: ControlRef = list.clone();
    let copy = ButtonControl::new(
        anchor("BOTTOMLEFT", &lr, "TOP"),
        rect(2.0, -4.0, 60.0, 18.0),
        "Copy".to_string(),
        |g| {
            let l = spec_list();
            let i = l.sel_index.get().expect("selValue");
            let m = ok_or_panic(engine_read(|e| e.copy_spec_model(i)));
            rename_spec_popup(g, RenameTarget::New(Box::new(m)), "Copy Tree");
        },
    );
    copy.base.enabled.set_fn(|_| spec_list().sel_value.borrow().is_some());
    let copy_r: ControlRef = copy.clone();
    let delete = ButtonControl::new(
        anchor("LEFT", &copy_r, "RIGHT"),
        rect(4.0, 0.0, 60.0, 18.0),
        "Delete".to_string(),
        |g| {
            let l = spec_list();
            if let Some(i) = l.sel_index.get()
                && l.list.borrow().len() > 1
            {
                confirm_delete_spec(g, i);
            }
        },
    );
    delete.base.enabled.set_fn(|_| {
        let l = spec_list();
        let n = l.list.borrow().len();
        l.sel_value.borrow().is_some() && n > 1
    });
    let rename = ButtonControl::new(
        anchor("BOTTOMRIGHT", &lr, "TOP"),
        rect(-2.0, -4.0, 60.0, 18.0),
        "Rename".to_string(),
        |g| {
            let i = spec_list().sel_index.get().expect("selValue");
            rename_spec_popup(g, RenameTarget::Existing(i), "Rename Tree");
        },
    );
    rename.base.enabled.set_fn(|_| spec_list().sel_value.borrow().is_some());
    let rn_r: ControlRef = rename.clone();
    let new_btn = ButtonControl::new(
        anchor("RIGHT", &rn_r, "LEFT"),
        rect(-4.0, 0.0, 60.0, 18.0),
        "New".to_string(),
        |g| {
            let m = engine_read(pob_engine::engine::Engine::new_spec_model);
            rename_spec_popup(g, RenameTarget::New(Box::new(m)), "New Tree");
        },
    );
    list.host.add("copy", copy);
    list.host.add("delete", delete);
    list.host.add("rename", rename);
    list.host.add("new", new_btn);
    let import_tree = ButtonControl::new(None, rect(-99.0, 259.0, 90.0, 20.0), "Import Tree".to_string(), open_import_popup);
    let it: ControlRef = import_tree.clone();
    let export_tree = ButtonControl::new(
        anchor("LEFT", &it, "RIGHT"),
        rect(8.0, 0.0, 90.0, 20.0),
        "Export Tree".to_string(),
        open_export_popup,
    );
    let et: ControlRef = export_tree.clone();
    let done = ButtonControl::new(anchor("LEFT", &et, "RIGHT"), rect(8.0, 0.0, 90.0, 20.0), "Done".to_string(), |_g| {
        main().close_popup()
    });
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert("1".into(), list);
    controls.insert("2".into(), import_tree);
    controls.insert("3".into(), export_tree);
    controls.insert("4".into(), done);
    main().open_popup(g, 370.0, 290.0, "Manage Passive Trees", controls, None, None, None, None);
}

// --- import / export --------------------------------------------------------------------------------------------------------------------

fn lua_captures(s: &str, pat: &str) -> Option<Vec<String>> {
    let caps = pob_lua::match_(s.as_bytes(), pat.as_bytes(), None).ok().flatten()?;
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(c) = caps.get(i) {
        out.push(String::from_utf8_lossy(c.bytes(s.as_bytes()).unwrap_or(b"")).into_owned());
        i += 1;
    }
    Some(out)
}

/// `validateTreeVersion(alternateType, major, minor)` (:732-746).
fn validate_tree_version(alternate: Option<&str>, version: Option<&[String]>) -> String {
    let t = pob_data::tree::tree_versions().expect("GameVersions");
    let suffix = alternate.map_or_else(String::new, |a| format!("_{}", a.replace('-', "_")));
    if let Some(v) = version
        && v.len() >= 2
        && let (Ok(major), Ok(minor)) = (v[0].parse::<f64>(), v[1].parse::<f64>())
    {
        let num: f64 = format!("{major}.{minor:02}").parse().unwrap_or(0.0);
        let num_of = |ver: &str| t.tree_versions.iter().find(|x| x.ver == ver).map_or(0.0, |x| x.num);
        if num >= num_of(&t.default_tree_version) && num <= num_of(&t.latest_tree_version) {
            return format!("{}_{}{suffix}", v[0], v[1]);
        }
    }
    format!("{}{suffix}", t.latest_tree_version)
}

/// `treeLink:gsub("/%?v=.+#","/")` (greedy: up to the last `#`).
fn strip_version(link: &str) -> String {
    match (link.find("/?v="), link.rfind('#')) {
        (Some(a), Some(b)) if a + 4 < b => format!("{}/{}", &link[..a], &link[b + 1..]),
        _ => link.to_string(),
    }
}

/// `poePlannerVersions` (GameVersions.lua:229), read from the checkout as [`tree_version_url`] reads the urls: the data patch does not export it.
fn poe_planner_versions(root: &str) -> HashMap<u32, String> {
    let text = std::fs::read_to_string(format!("{root}GameVersions.lua")).unwrap_or_default();
    let mut out = HashMap::new();
    let Some(p) = text.find("poePlannerVersions = {") else { return out };
    for line in text[p..].lines().skip(1) {
        let line = line.trim();
        if line.starts_with('}') {
            break;
        }
        // a commented-out row is not in the table
        if let Some(c) = lua_captures(line, r#"^%[(%d+)%] = "([^"]+)""#)
            && let Ok(k) = c[0].parse::<u32>()
        {
            out.insert(k, c[1].clone());
        }
    }
    out
}

/// What `decodeTreeLink` and `decodePoePlannerTreeLink` share after the decode (:702-710, :720-729).
fn push_imported_spec(g: &mut Gfx, m: pob_build::SpecModel) {
    ok_or_panic(with_engine(|e| e.push_decoded_spec(m)));
    let t = tab();
    let n = engine_read(pob_engine::engine::Engine::spec_count);
    t.set_active_spec(g, n);
    let b = main().build_mode.clone();
    b.mod_flag.set(true);
    t.add_undo_state();
    b.build_flag.set(true);
    if SPEC_LIST.with_borrow(|w| w.upgrade().is_some()) {
        spec_list().list.borrow_mut().push(ByRef(Rc::new(())));
        refresh_summaries();
    }
    main().close_popup();
}

/// A gate's way to the two popups (the "Manage trees" popup has its buttons without names).
pub fn debug_open_link_popup(g: &mut Gfx, export: bool) {
    if export {
        open_export_popup(g);
    } else {
        open_import_popup(g);
    }
}

/// `OpenImportPopup` (:687-815): the tree links.
fn open_import_popup(g: &mut Gfx) {
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert(
        "nameLabel".into(),
        LabelControl::new(None, rect(-180.0, 20.0, 0.0, 16.0), "Enter name for this passive tree:".to_string()),
    );
    let msg = LabelControl::new(None, rect(0.0, 65.0, 0.0, 16.0), String::new());
    let import_slot: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
    let other_slot: Rc<RefCell<(Option<Rc<EditControl>>, Option<Rc<EditControl>>)>> = Rc::new(RefCell::new((None, None)));
    let has_text = |s: &str| s.bytes().any(|c| !c.is_ascii_whitespace());
    let change = |own_is_name: bool| {
        let (m, is, os) = (msg.clone(), import_slot.clone(), other_slot.clone());
        Rc::new(move |_g: &mut Gfx, buf: &str, _: bool| {
            m.label.set(String::new());
            let o = os.borrow();
            let other = if own_is_name { o.1.as_ref() } else { o.0.as_ref() };
            let other_ok = other.is_some_and(|e| has_text(&e.buf.borrow()));
            if let Some(i) = &*is.borrow() {
                i.base.enabled.set(has_text(buf) && other_ok);
            }
        })
    };
    let name = EditControl::new(
        g,
        None,
        rect(100.0, 20.0, 350.0, 18.0),
        EditArgs {
            init: Some(String::new()),
            change_func: Some(change(true)),
            ..Default::default()
        },
    );
    let edit = EditControl::new(
        g,
        None,
        rect(100.0, 45.0, 350.0, 18.0),
        EditArgs {
            init: Some(String::new()),
            change_func: Some(change(false)),
            ..Default::default()
        },
    );
    *other_slot.borrow_mut() = (Some(name.clone()), Some(edit.clone()));
    controls.insert("name".into(), name.clone());
    controls.insert(
        "editLabel".into(),
        LabelControl::new(None, rect(-150.0, 45.0, 0.0, 16.0), "Enter passive tree link:".to_string()),
    );
    controls.insert("edit".into(), edit.clone());
    controls.insert("msg".into(), msg.clone());
    let (n2, e2, m2) = (name, edit, msg);
    let islot = import_slot.clone();
    let import = ButtonControl::new(None, rect(-45.0, 85.0, 80.0, 20.0), "Import".to_string(), move |g| {
        let tree_link = e2.buf.borrow().clone();
        if tree_link.is_empty() {
            return;
        }
        if tree_link.contains("poeurl.com/") {
            // :766-794
            let button = islot.borrow().clone().expect("import");
            button.base.enabled.set(false);
            m2.label.set("Resolving PoEURL link...".to_string());
            let l = crate::launch::launch();
            let req = l.net_request(&tree_link, None, None, false);
            let (m3, n3) = (m2.clone(), n2.clone());
            l.launch_sub(pob_net::Job::Http(req), move |g, outcome| {
                let redirect = match outcome {
                    pob_net::Outcome::Http(Ok(r)) => r.redirect_url().filter(|u| !u.contains("poeurl.com/")),
                    _ => None,
                };
                match redirect {
                    None => {
                        m3.label.set("^1Failed to resolve PoEURL link^7".to_string());
                        button.base.enabled.set(true);
                    }
                    Some(link) => {
                        let title = n3.buf.borrow().clone();
                        let alt = lua_captures(&link, "tree/(%l+%-?%l*)");
                        let v = lua_captures(&link, "tree/([0-9]+)%.([0-9]+)%.([0-9]+)/");
                        let version = validate_tree_version(alt.as_ref().and_then(|a| a.first()).map(String::as_str), v.as_deref());
                        match ok_or_panic(pob_engine::engine::Engine::decode_tree_url(&link, &version, &title)) {
                            Err(err) => m3.label.set(format!("^1{err}^7")),
                            Ok(m) => push_imported_spec(g, m),
                        }
                    }
                }
            });
            return;
        }
        if tree_link.contains("poeplanner.com/") {
            // :795-796 `decodePoePlannerTreeLink(treeLink:gsub("/%?v=.+#","/"))`, :690-711
            let link = strip_version(&tree_link);
            let root = crate::launch::launch().cfg.script_path.clone();
            let versions = poe_planner_versions(&root);
            let new_version = match pob_engine::engine::Engine::poe_planner_version(&link, &|v| versions.get(&v).cloned()) {
                Ok(v) => v,
                Err(err) => {
                    m2.label.set(format!("^1{err}"));
                    return;
                }
            };
            let title = n2.buf.borrow().clone();
            let m = ok_or_panic(pob_engine::engine::Engine::decode_poe_planner_url(&link, &new_version, &title));
            push_imported_spec(g, m);
            return;
        }
        let version_lookup = "tree/([0-9]+)%.([0-9]+)%.([0-9]+)/";
        let (url, version) = if tree_link.contains("poeskilltree.com/") {
            // :797-800
            let old = "/%?v=([0-9]+)%.([0-9]+)%.([0-9]+)%-?%w?%-?%w?#";
            let alt = lua_captures(&tree_link, "%-(%l+%-?%l*)#");
            let v = lua_captures(&tree_link, old);
            (
                strip_version(&tree_link),
                validate_tree_version(alt.as_ref().and_then(|a| a.first()).map(String::as_str), v.as_deref()),
            )
        } else {
            // :801-808
            let alt = lua_captures(&tree_link, "tree/(%l+%-?%l*)");
            let v = lua_captures(&tree_link, version_lookup);
            (
                tree_link.clone(),
                validate_tree_version(alt.as_ref().and_then(|a| a.first()).map(String::as_str), v.as_deref()),
            )
        };
        g.con_printf(&format!("Tree version: {version}"));
        let title = n2.buf.borrow().clone();
        // :713-731 `decodeTreeLink`
        match ok_or_panic(pob_engine::engine::Engine::decode_tree_url(&url, &version, &title)) {
            Err(err) => m2.label.set(format!("^1{err}^7")),
            Ok(m) => push_imported_spec(g, m),
        }
    });
    import.base.enabled.set(false);
    *import_slot.borrow_mut() = Some(import.clone());
    controls.insert("import".into(), import);
    controls.insert(
        "cancel".into(),
        ButtonControl::new(None, rect(45.0, 85.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup()),
    );
    main().open_popup(g, 580.0, 115.0, "Import Tree", controls, Some("import"), Some("name"), None, None);
}

/// `OpenExportPopup` (:817-844).
fn open_export_popup(g: &mut Gfx) {
    let root = crate::launch::launch().cfg.script_path.clone();
    let ver = engine_read(|e| e.spec_rc().tree_version.clone());
    let tree_link = ok_or_panic(engine_read(|e| e.encode_tree_url(&tree_version_url(&root, &ver))));
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert(
        "label".into(),
        LabelControl::new(None, rect(0.0, 20.0, 0.0, 16.0), "Passive tree link:".to_string()),
    );
    let edit = EditControl::new(
        g,
        None,
        rect(0.0, 40.0, 350.0, 18.0),
        EditArgs {
            init: Some(tree_link.clone()),
            filter: Some("%Z".into()),
            ..Default::default()
        },
    );
    controls.insert("edit".into(), edit.clone());
    // :821 `treeLink`, an upvalue the callback writes and `Copy` reads
    let link = Rc::new(RefCell::new(tree_link));
    let shrink_slot: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
    let (l2, e2, s2) = (link.clone(), edit, shrink_slot.clone());
    let shrink = ButtonControl::new(None, rect(-90.0, 70.0, 140.0, 20.0), "Shrink with PoEURL".to_string(), move |_g| {
        // :823-836
        let button = s2.borrow().clone().expect("shrink");
        button.base.enabled.set(false);
        button.label.set("Shrinking...".to_string());
        let url = format!("http://poeurl.com/shrink.php?url={}", l2.borrow());
        let (l3, e3) = (l2.clone(), e2.clone());
        crate::launch::launch().download_page(&url, crate::launch::PageParams::default(), move |g, response, err| {
            button.label.set("Done".to_string());
            if err.is_some() || response.body.bytes().all(|b| b.is_ascii_whitespace()) {
                main().open_message_popup(g, "PoEURL Shortener", "Failed to get PoEURL link. Try again later.");
            } else {
                let new_link = format!("http://poeurl.com/{}", response.body);
                e3.set_text(g, &new_link, false);
                *l3.borrow_mut() = new_link;
                let popup = main().popups.borrow().first().cloned();
                if let Some(p) = popup {
                    let c: ControlRef = e3.clone();
                    p.host.select_control(g, Some(c));
                }
            }
        });
    });
    *shrink_slot.borrow_mut() = Some(shrink.clone());
    controls.insert("shrink".into(), shrink);
    controls.insert(
        "copy".into(),
        ButtonControl::new(None, rect(30.0, 70.0, 80.0, 20.0), "Copy".to_string(), move |g| {
            g.copy(&link.borrow())
        }),
    );
    controls.insert(
        "done".into(),
        ButtonControl::new(None, rect(120.0, 70.0, 80.0, 20.0), "Done".to_string(), |_g| main().close_popup()),
    );
    main().open_popup(g, 380.0, 100.0, "Export Tree", controls, Some("done"), Some("edit"), None, None);
}

// --- PassiveMasteryControl / mastery popup ------------------------------------------------------------------------------------------------

struct MasteryHooks {
    node_id: u32,
    alt_path: Option<Vec<u32>>,
}

impl ListHooks<pob_engine::tree_tab::MasteryChoice> for MasteryHooks {
    fn get_row_value(
        &self,
        _l: &ListControl<pob_engine::tree_tab::MasteryChoice>,
        _g: &Gfx,
        column: usize,
        _i: usize,
        v: &pob_engine::tree_tab::MasteryChoice,
    ) -> String {
        if column == 1 { v.label.clone() } else { String::new() }
    }

    fn has_add_value_tooltip(&self) -> bool {
        true
    }

    /// PassiveMasteryControl.lua:50-56: `AddNodeTooltip` of the node with this effect.
    fn add_value_tooltip(
        &self,
        _l: &ListControl<pob_engine::tree_tab::MasteryChoice>,
        g: &Gfx,
        tooltip: &mut Tooltip,
        _i: usize,
        v: &pob_engine::tree_tab::MasteryChoice,
    ) {
        tooltip.clear(false);
        let b = main().build_mode.clone();
        let root = crate::launch::launch().cfg.script_path.clone();
        let mut tv = b.tree_view.borrow_mut();
        let view = tv.as_mut().expect("view");
        let eng = b.engine.borrow();
        let e = eng.as_ref().expect("engine");
        let spec = e.spec_rc();
        let art = view.art(&root, &spec.tree);
        crate::tree_tooltip::mastery_effect_tooltip(view, tooltip, g, e, &art, &spec, self.node_id, v.id);
    }

    /// :58-60 + TreeTab.lua:1000-1017 `SaveMasteryPopup`
    fn on_sel_click(
        &self,
        _l: &ListControl<pob_engine::tree_tab::MasteryChoice>,
        _g: &mut Gfx,
        _i: usize,
        v: &pob_engine::tree_tab::MasteryChoice,
        _double: bool,
    ) -> Option<bool> {
        let t = tab();
        t.ensure_undo();
        ok_or_panic(with_engine(|e| e.save_mastery(self.node_id, v.id, self.alt_path.as_deref())));
        t.add_undo_state();
        let b = main().build_mode.clone();
        b.mod_flag.set(true);
        b.build_flag.set(true);
        main().close_popup();
        None
    }
}

/// `OpenMasteryPopup(node, viewPort)` (:1019-1044).
pub fn open_mastery_popup(g: &mut Gfx, node_id: u32, alt_path: Option<Vec<u32>>) {
    let effects = engine_read(|e| e.mastery_choices(node_id));
    if effects.is_empty() {
        return;
    }
    let height = (effects.len() as f64 + 1.0) * 14.0 + 2.0;
    let mut width: f64 = 0.0;
    for e in &effects {
        width = width.max(sg::draw_string_width(g, 16.0, "VAR", &e.label) + 5.0);
    }
    let name = engine_read(|e| e.spec_rc().node(node_id).and_then(|n| n.name().map(str::to_owned))).unwrap_or_default();
    let effect: Rc<ListControl<pob_engine::tree_tab::MasteryChoice>> = ListControl::new(
        anchor_nil("TOPLEFT", "TOPLEFT"),
        rect(6.0, 25.0, width, height),
        16.0,
        None,
        false,
        effects,
        false,
        Box::new(MasteryHooks { node_id, alt_path }),
    );
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert(
        "close".into(),
        ButtonControl::new(None, rect(0.0, 30.0 + height, 90.0, 20.0), "Cancel".to_string(), |_g| {
            main().close_popup()
        }),
    );
    controls.insert("effect".into(), effect);
    main().open_popup(g, width + 12.0, height + 60.0, &name, controls, None, None, Some("close"), None);
}

// --- ModifyNodePopup --------------------------------------------------------------------------------------------------------------------

/// `ModifyNodePopup(selectedNode)` (:846-998).
#[allow(clippy::too_many_lines)]
pub fn modify_node_popup(g: &mut Gfx, node_id: u32) {
    let t = tab();
    let node_name = engine_read(|e| e.spec_rc().node(node_id).map(|n| n.tree_node().dn.clone())).unwrap_or_default();
    let mod_groups = Rc::new(RefCell::new(ok_or_panic(engine_read(|e| {
        e.tattoo_choices(node_id, t.show_legacy_tattoo.get())
    }))));
    let items = |groups: &[pob_engine::tree_tab::TattooChoice]| -> Vec<DropItem> {
        groups
            .iter()
            .map(|c| {
                let mut d = DropItem::table(&c.label);
                d.data = Some(Rc::new(c.descriptions.clone()));
                d
            })
            .collect()
    };
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    let holder: Rc<RefCell<Vec<ControlRef>>> = Rc::new(RefCell::new(Vec::new()));
    let label = LabelControl::new(
        anchor_nil("TOPRIGHT", "TOPLEFT"),
        rect(170.0, 25.0, 0.0, 16.0),
        "^7Modifier:".to_string(),
    );
    let slots: Rc<RefCell<Option<ModifySlots>>> = Rc::new(RefCell::new(None));
    let (s2, mg2) = (slots.clone(), mod_groups.clone());
    let mod_select = DropDownControl::new(
        g,
        anchor_nil("TOPLEFT", "TOPLEFT"),
        rect(175.0, 25.0, 250.0, 18.0),
        items(&mod_groups.borrow()),
        Some(Rc::new(move |g: &mut Gfx, idx, _v: &DropItem| {
            let descs = mg2.borrow().get(idx - 1).map(|c| c.descriptions.clone()).unwrap_or_default();
            construct_ui(g, &s2, &descs);
        })),
        None,
        false,
    );
    let default_idx = t.default_tattoo.borrow().get(&node_name).copied().unwrap_or(1);
    mod_select.sel_index.set(default_idx);
    mod_select.tip.set_tooltip_func(
        |tooltip: &mut Tooltip, g: &Gfx, (mode, _i, v): (&'static str, usize, Option<DropItem>)| {
            tooltip.clear(false);
            if mode != "OUT"
                && let Some(v) = v
                && let Some(d) = v.data.as_ref().and_then(|d| d.downcast_ref::<Vec<String>>())
            {
                for line in d {
                    tooltip.add_line(g, 16.0, Some(&format!("^7{line}")));
                }
            }
        },
    );
    let (ms2, mg3, name2) = (mod_select.clone(), mod_groups.clone(), node_name.clone());
    let save = ButtonControl::new(None, rect(-90.0, 75.0, 80.0, 20.0), "Add".to_string(), move |_g| {
        let idx = ms2.sel_index.get();
        let t = tab();
        t.ensure_undo();
        let key = mg3.borrow()[idx - 1].key.clone();
        ok_or_panic(with_engine(|e| e.add_tattoo(node_id, &key)));
        t.add_undo_state();
        let b = main().build_mode.clone();
        b.mod_flag.set(true);
        b.build_flag.set(true);
        t.default_tattoo.borrow_mut().insert(name2.clone(), idx);
        main().close_popup();
    });
    let name3 = node_name.clone();
    let reset = ButtonControl::new(None, rect(0.0, 75.0, 80.0, 20.0), "Reset Node".to_string(), move |_g| {
        let t = tab();
        t.ensure_undo();
        ok_or_panic(with_engine(|e| e.remove_tattoo(node_id)));
        t.add_undo_state();
        let b = main().build_mode.clone();
        b.mod_flag.set(true);
        b.build_flag.set(true);
        t.default_tattoo.borrow_mut().remove(&name3);
        main().close_popup();
    });
    let close = ButtonControl::new(None, rect(90.0, 75.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup());
    let (count, _) = engine_read(pob_engine::engine::Engine::tattoo_count);
    let count_str = if count > 50 {
        format!("{}{count}", cc("NEGATIVE"))
    } else {
        count.to_string()
    };
    let total_tattoos = ButtonControl::new(
        None,
        rect(0.0, 95.0, 145.0, 20.0),
        format!("^7Tattoo Count: {count_str}/50"),
        |_g| {},
    );
    total_tattoos.tip.set_tooltip_func(|tooltip: &mut Tooltip, g: &Gfx, ()| {
        tooltip.clear(false);
        let (_, lines) = engine_read(pob_engine::engine::Engine::tattoo_count);
        for (mult, line) in lines {
            tooltip.add_line(g, 16.0, Some(&format!("{}({mult}) ^7{line}", cc("COLD"))));
        }
    });
    let tt_r: ControlRef = total_tattoos.clone();
    controls.insert("modSelectLabel".into(), label);
    controls.insert("modSelect".into(), mod_select.clone());
    controls.insert("save".into(), save.clone());
    controls.insert("reset".into(), reset.clone());
    controls.insert("close".into(), close.clone());
    controls.insert("totalTattoos".into(), total_tattoos.clone());
    let popup = main().open_popup(
        g,
        600.0,
        105.0,
        "Replace Modifier of Node",
        controls,
        Some("save"),
        None,
        None,
        None,
    );
    *slots.borrow_mut() = Some(ModifySlots {
        popup: popup.clone(),
        mod_select: mod_select.clone(),
        save,
        reset,
        close,
        total_tattoos,
        labels: holder,
    });
    let descs = mod_groups
        .borrow()
        .get(default_idx - 1)
        .map(|c| c.descriptions.clone())
        .unwrap_or_default();
    construct_ui(g, &slots, &descs);
    // :993-997 the check box is added after the popup was opened: its anchor is its own
    let (mg4, ms3) = (mod_groups, mod_select);
    let legacy = CheckBoxControl::new(
        g,
        anchor("LEFT", &tt_r, "RIGHT"),
        [195.0, 0.0, 20.0],
        Some("Show Legacy Tattoos:"),
        Some(Rc::new(move |g: &mut Gfx, state| {
            let t = tab();
            t.show_legacy_tattoo.set(state);
            let list = ok_or_panic(engine_read(|e| e.tattoo_choices(node_id, state)));
            ms3.list.borrow_mut().clear();
            let it: Vec<DropItem> = list
                .iter()
                .map(|c| {
                    let mut d = DropItem::table(&c.label);
                    d.data = Some(Rc::new(c.descriptions.clone()));
                    d
                })
                .collect();
            ms3.set_list(g, Some(it));
            *mg4.borrow_mut() = list;
        })),
        None,
        t.show_legacy_tattoo.get(),
    );
    popup.host.add("showLegacyTattoo", legacy);
}

struct ModifySlots {
    popup: Rc<crate::controls::popup::PopupDialog>,
    mod_select: Rc<DropDownControl>,
    save: Rc<ButtonControl>,
    reset: Rc<ButtonControl>,
    close: Rc<ButtonControl>,
    total_tattoos: Rc<ButtonControl>,
    labels: Rc<RefCell<Vec<ControlRef>>>,
}

/// `constructUI(modGroup)` (:895-920).
fn construct_ui(g: &mut Gfx, slots: &Rc<RefCell<Option<ModifySlots>>>, descriptions: &[String]) {
    let s = slots.borrow();
    let Some(s) = s.as_ref() else { return };
    let mut total_height = 43.0;
    let max_width = 375.0;
    {
        let mut ctl = s.popup.host.controls.borrow_mut();
        let mut i = 1;
        while ctl.shift_remove(&i.to_string()).is_some() {
            i += 1;
        }
    }
    s.labels.borrow_mut().clear();
    let mut wrap = Vec::new();
    for d in descriptions {
        wrap.extend(wrap_string(g, d, 16.0, max_width));
    }
    for (idx, d) in wrap.iter().enumerate() {
        let prev: ControlRef = s.labels.borrow().last().cloned().unwrap_or_else(|| s.mod_select.clone());
        let l: ControlRef = LabelControl::new(anchor("TOPLEFT", &prev, "TOPLEFT"), rect(0.0, 20.0, 600.0, 16.0), format!("^7{d}"));
        s.labels.borrow_mut().push(l.clone());
        s.popup.host.add(&(idx + 1).to_string(), l);
        total_height += 20.0;
    }
    s.popup.base.height.set(total_height + 75.0);
    let button_height = total_height + 15.0;
    s.save.base.y.set(button_height);
    s.reset.base.y.set(button_height);
    s.close.base.y.set(button_height);
    s.total_tattoos.base.y.set(button_height + 30.0);
}

// --- Draw -------------------------------------------------------------------------------------------------------------------------------

impl TreeTab {
    /// `TreeTabClass:Draw(viewPort, inputEvents)` (:358-489).
    #[allow(clippy::too_many_lines, clippy::needless_range_loop)]
    pub fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, input_events: &mut InputEvents) {
        let build = main().build_mode.clone();
        self.anchor_controls.base.x.set(view_port.x + 4.0);
        self.anchor_controls.base.y.set(view_port.y + view_port.height - 24.0);
        for id in 0..input_events.len() {
            let Some(InputEvent::KeyDown { key, .. }) = input_events[id].clone() else {
                if input_events[id].is_none() {
                    break;
                }
                continue;
            };
            let ctrl = sg::is_key_down(f, "CTRL");
            match key.as_str() {
                "z" if ctrl => {
                    self.do_undo();
                    build.build_flag.set(true);
                    input_events[id] = None;
                }
                "y" if ctrl => {
                    self.do_redo();
                    build.build_flag.set(true);
                    input_events[id] = None;
                }
                "UP" => {
                    let active = engine_read(pob_engine::engine::Engine::active_spec);
                    if active > 1 && !(self.spec_select.is_mouse_over(f) == Some(true)) && !self.spec_select.dropped.get() {
                        build.mod_flag.set(true);
                        self.set_active_spec(f, active - 1);
                    }
                }
                "DOWN" if !(self.spec_select.is_mouse_over(f) == Some(true)) && !self.spec_select.dropped.get() => {
                    let (active, n) = engine_read(|e| (e.active_spec(), e.spec_count()));
                    if active < n {
                        build.mod_flag.set(true);
                        self.set_active_spec(f, active + 1);
                    }
                }
                "f" if ctrl => {
                    let c: ControlRef = self.tree_search.clone();
                    self.host.select_control(f, Some(c));
                }
                "m" if ctrl => open_spec_manage_popup(f),
                _ => {}
            }
        }
        self.host.process_controls_input(f, input_events, view_port);

        // :393-428
        let mut lines_height = 24.0;
        let right_margin = 10.0;
        let w = |c: &dyn Control, g: &Gfx| c.get_size(g).0;
        let x = |c: &dyn Control, g: &Gfx| c.base().x.get(g);
        let first = w(&*self.spec_select, f)
            + 8.0
            + w(&*self.compare_check, f)
            + x(&*self.compare_check, f)
            + w(&*self.reset, f)
            + x(&*self.reset, f)
            + w(&*self.version_text, f)
            + x(&*self.version_text, f)
            + w(&*self.version_select, f)
            + x(&*self.version_select, f)
            + if self.is_comparing.get() {
                w(&*self.compare_select, f) + x(&*self.compare_select, f)
            } else {
                0.0
            };
        let heat = self.show_heat_map();
        let second = w(&*self.tree_search, f)
            + 8.0
            + w(&*self.find_timeless_jewel, f)
            + x(&*self.find_timeless_jewel, f)
            + w(&*self.tree_heat_map, f)
            + 130.0
            + w(&*self.node_power_max_depth_select, f)
            + x(&*self.node_power_max_depth_select, f)
            + if self.is_custom_max_depth.get() {
                w(&*self.node_power_max_depth_custom, f) + x(&*self.node_power_max_depth_custom, f)
            } else {
                0.0
            }
            + if heat {
                w(&*self.tree_heat_map_stat_select, f)
                    + x(&*self.tree_heat_map_stat_select, f)
                    + w(&*self.power_report, f)
                    + x(&*self.power_report, f)
            } else {
                0.0
            };
        let (ss, ts, hm, vs, ft, sc): (ControlRef, ControlRef, ControlRef, ControlRef, ControlRef, ControlRef) = (
            self.spec_select.clone(),
            self.tree_search.clone(),
            self.tree_heat_map.clone(),
            self.version_select.clone(),
            self.find_timeless_jewel.clone(),
            self.spec_convert_text.clone(),
        );
        let spec_h = self.spec_select.base.height.get(f);
        if view_port.width >= first + second + right_margin {
            lines_height = 0.0;
            self.tree_search
                .base
                .set_anchor("LEFT", Some(Rc::downgrade(&vs)), "RIGHT", Some((8.0, 0.0)), false);
            self.power_report_list
                .base
                .set_anchor("TOPLEFT", Some(Rc::downgrade(&ss)), "BOTTOMLEFT", Some((0.0, spec_h + 6.0)), false);
        } else {
            self.tree_search
                .base
                .set_anchor("TOPLEFT", Some(Rc::downgrade(&ss)), "BOTTOMLEFT", Some((0.0, 4.0)), false);
            let h = self.tree_search.base.height.get(f);
            self.power_report_list
                .base
                .set_anchor("TOPLEFT", Some(Rc::downgrade(&ts)), "BOTTOMLEFT", Some((0.0, h + 6.0)), false);
        }
        if view_port.width >= second + right_margin {
            self.tree_heat_map
                .base
                .set_anchor("LEFT", Some(Rc::downgrade(&ft)), "RIGHT", Some((130.0, 0.0)), false);
        } else {
            lines_height *= 2.0;
            self.tree_heat_map
                .base
                .set_anchor("TOPLEFT", Some(Rc::downgrade(&ts)), "BOTTOMLEFT", Some((124.0, 4.0)), false);
            let h = self.tree_heat_map.base.height.get(f);
            self.power_report_list
                .base
                .set_anchor("TOPLEFT", Some(Rc::downgrade(&hm)), "BOTTOMLEFT", Some((-124.0, h + 6.0)), false);
        }
        // :430-440
        let mut convert_two_line_height = 24.0;
        let convert_max_width = 900.0;
        if view_port.width >= convert_max_width {
            convert_two_line_height = 0.0;
            self.spec_convert
                .base
                .set_anchor("LEFT", Some(Rc::downgrade(&sc)), "RIGHT", Some((8.0, 0.0)), false);
            self.spec_convert_text
                .base
                .set_anchor("BOTTOMLEFT", Some(Rc::downgrade(&ss)), "TOPLEFT", Some((0.0, -14.0)), false);
        } else {
            self.spec_convert
                .base
                .set_anchor("TOPLEFT", Some(Rc::downgrade(&sc)), "BOTTOMLEFT", Some((0.0, 4.0)), false);
            self.spec_convert_text
                .base
                .set_anchor("BOTTOMLEFT", Some(Rc::downgrade(&ss)), "TOPLEFT", Some((0.0, -38.0)), false);
        }
        let shown = self.power_report_list.is_shown(f);
        let bottom_drawer_height = if shown { 194.0 } else { 0.0 };
        self.spec_select.base.y.set(-bottom_drawer_height - lines_height);
        let show_convert = self.show_convert.get();
        let tree_view_port = Rect {
            x: view_port.x,
            y: view_port.y,
            width: view_port.width,
            height: view_port.height - (if show_convert { 64.0 } else { 32.0 } + bottom_drawer_height + lines_height),
        };
        if let Some((x, y)) = self.jump_to.take() {
            let spec = engine_read(pob_engine::engine::Engine::spec_rc);
            build
                .tree_view
                .borrow_mut()
                .as_mut()
                .expect("view")
                .focus(x, y, &tree_view_port, &spec.tree);
        }
        let compare = self.compare_spec();
        build.draw_tree_view(f, &tree_view_port, input_events, compare);

        // :453-462
        let mut new_spec_list: Vec<DropItem> = engine_read(pob_engine::engine::Engine::spec_list_labels)
            .iter()
            .map(|s| DropItem::str(s))
            .collect();
        self.compare_select.sel_index.set(self.active_compare_spec.get());
        self.compare_select.set_list(f, Some(new_spec_list.clone()));
        // the same table: `t_insert` (:456) adds the row to `compareSelect.list` too, after its `SetList`
        new_spec_list.push(DropItem::str("Manage trees... (ctrl-m)"));
        self.compare_select
            .list
            .borrow_mut()
            .push(DropItem::str("Manage trees... (ctrl-m)"));
        self.spec_select.sel_index.set(engine_read(pob_engine::engine::Engine::active_spec));
        self.spec_select.set_list(f, Some(new_spec_list));
        if !self.tree_search.base.has_focus.get() {
            let s = build.tree_view.borrow().as_ref().map_or_else(String::new, |v| v.search_str.clone());
            self.tree_search.set_text(f, &s, false);
        }
        // :464-471
        let heat = self.show_heat_map();
        self.tree_heat_map.state.set(heat);
        self.tree_heat_map_stat_select.base.shown.set(heat);
        if self.tree_heat_map_stat_select.list.borrow().is_empty() {
            let list: Vec<DropItem> = power_stat_list()
                .into_iter()
                .filter(|p| !p.ignore_for_nodes)
                .map(|p| {
                    let mut d = DropItem::table(&p.label);
                    d.data = Some(Rc::new(p));
                    d
                })
                .collect();
            *self.tree_heat_map_stat_select.list.borrow_mut() = list;
        }
        self.tree_heat_map_stat_select.sel_index.set(1);
        self.tree_heat_map_stat_select.check_dropped_width(f, true);
        if let Some(st) = self.power_stat.borrow().as_ref() {
            let ix = self.tree_heat_map_stat_select.list.borrow().iter().position(|v| {
                v.data
                    .as_ref()
                    .and_then(|d| d.downcast_ref::<PowerStat>())
                    .is_some_and(|p| p.stat == st.stat)
            });
            if let Some(i) = ix {
                self.tree_heat_map_stat_select.sel_index.set(i + 1);
            }
        }

        sg::set_draw_layer(f, Some(1), 0);
        sg::set_draw_color(f, 0.05, 0.05, 0.05);
        let vb = view_port.y + view_port.height;
        sg::draw_rect(
            f,
            view_port.x,
            vb - (30.0 + bottom_drawer_height + lines_height),
            view_port.width,
            30.0 + bottom_drawer_height + lines_height,
        );
        if show_convert {
            let height = if view_port.width < convert_max_width {
                bottom_drawer_height + lines_height
            } else {
                0.0
            };
            sg::set_draw_color(f, 0.05, 0.05, 0.05);
            sg::draw_rect(
                f,
                view_port.x,
                vb - (60.0 + bottom_drawer_height + lines_height + convert_two_line_height),
                view_port.width,
                28.0 + height,
            );
            sg::set_draw_color(f, 0.85, 0.85, 0.85);
            sg::draw_rect(
                f,
                view_port.x,
                vb - (64.0 + bottom_drawer_height + lines_height + convert_two_line_height),
                view_port.width,
                4.0,
            );
        }
        sg::set_draw_color(f, 0.85, 0.85, 0.85);
        sg::draw_rect(
            f,
            view_port.x,
            vb - (34.0 + bottom_drawer_height + lines_height),
            view_port.width,
            4.0,
        );
        self.host.draw_controls(f, view_port, None);
    }
}

// --- the oracle's view of the tab (tests/tree.rs, `tabDump` of tree-runner.lua) ----------------------------------------------------------

/// `tabDump(build)` of `tools/sg-oracle/lua/tree-runner.lua`.
pub fn debug_dump(g: &Gfx) -> serde_json::Value {
    use serde_json::json;
    let t = tab();
    let spec = engine_read(pob_engine::engine::Engine::spec_rc);
    let mut masteries: Vec<String> = spec
        .mastery_selections
        .iter()
        .map(|(id, e)| format!("{id}:{}", pob_lua::tostring_num(*e)))
        .collect();
    masteries.sort();
    let mut overrides: Vec<String> = spec.hash_overrides.iter().map(|(id, o)| format!("{id}:{}", o.dn)).collect();
    overrides.sort();
    let mut rects = serde_json::Map::new();
    let list: [(&str, ControlRef); 16] = [
        ("specSelect", t.spec_select.clone()),
        ("compareCheck", t.compare_check.clone()),
        ("compareSelect", t.compare_select.clone()),
        ("reset", t.reset.clone()),
        ("versionText", t.version_text.clone()),
        ("versionSelect", t.version_select.clone()),
        ("treeSearch", t.tree_search.clone()),
        ("findTimelessJewel", t.find_timeless_jewel.clone()),
        ("treeHeatMap", t.tree_heat_map.clone()),
        ("nodePowerMaxDepthSelect", t.node_power_max_depth_select.clone()),
        ("treeHeatMapStatSelect", t.tree_heat_map_stat_select.clone()),
        ("powerReport", t.power_report.clone()),
        ("powerReportList", t.power_report_list.clone()),
        ("specConvertText", t.spec_convert_text.clone()),
        ("specConvert", t.spec_convert.clone()),
        ("specConvertAll", t.spec_convert_all.clone()),
    ];
    let num = |v: f64| pob_lua::format::format("%g", &[pob_lua::format::Arg::Num(v)]).unwrap_or_default();
    for (k, c) in &list {
        if c.is_shown(g) {
            let (x, y) = c.get_pos(g);
            let (w, h) = c.get_size(g);
            rects.insert((*k).to_string(), json!(format!("{},{},{},{}", num(x), num(y), num(w), num(h))));
        }
    }
    let specs: Vec<String> = t
        .spec_select
        .list
        .borrow()
        .iter()
        .map(|v| v.label.clone().unwrap_or_default())
        .collect();
    let (undo, redo) = {
        let i = t.undo_index();
        let u = t.undo.borrow();
        (u[i].undo.len(), u[i].redo.len())
    };
    let mut rows: Vec<String> = t
        .power_report_list
        .list
        .borrow()
        .iter()
        .map(|r| {
            let r = &r.0;
            let dist = match &r.path_dist {
                ReportDist::Num(n) => pob_lua::tostring_num(*n),
                ReportDist::Str(s) => (*s).to_string(),
            };
            format!("{}|{}|{}|{dist}|{}", r.ty.as_str(), r.name, r.power_str, r.path_power_str)
        })
        .collect();
    rows.sort();
    let mut d = json!({
        "activeSpec": engine_read(pob_engine::engine::Engine::active_spec),
        "compareSpec": t.active_compare_spec.get(),
        "isComparing": t.is_comparing.get(),
        "showConvert": t.show_convert.get(),
        "showHeatMap": t.show_heat_map(),
        "specs": specs,
        "versionSel": t.version_select.sel_index.get(),
        "treeVersion": spec.tree_version,
        "classId": spec.cur_class_id,
        "ascendClassId": spec.cur_ascend_class_id,
        "secondary": spec.cur_secondary_ascend_class_id.map_or(json!("nil"), |v| json!(v)),
        "masteries": masteries,
        "overrides": overrides,
        "undo": undo,
        "redo": redo,
        "rects": rects,
        "reportShown": t.power_report_list.base.shown.get(g),
        "reportLabel": t.power_report_list.label.get(g).unwrap_or_else(|| "nil".into()),
        "report": rows,
    });
    if let Some(p) = main().popups.borrow().first() {
        let mut q = serde_json::Map::new();
        q.insert("title".into(), json!(p.title.get(g)));
        q.insert("width".into(), json!(p.base.width.get(g)));
        q.insert("height".into(), json!(p.base.height.get(g)));
        let any_row = |c: &ControlRef| -> Option<Vec<String>> {
            let any: Rc<dyn std::any::Any> = c.clone();
            if let Ok(l) = any.clone().downcast::<ListControl<SpecRow>>() {
                let v = l.list.borrow().clone();
                return Some(
                    v.iter()
                        .enumerate()
                        .map(|(i, r)| l.hooks.get_row_value(&l, g, 1, i + 1, r))
                        .collect(),
                );
            }
            if let Ok(l) = any.downcast::<ListControl<pob_engine::tree_tab::MasteryChoice>>() {
                return Some(l.list.borrow().iter().map(|r| r.label.clone()).collect());
            }
            None
        };
        if let Some(rows) = p
            .host
            .get("1")
            .as_ref()
            .and_then(any_row)
            .or_else(|| p.host.get("effect").as_ref().and_then(any_row))
        {
            q.insert("rows".into(), json!(rows));
        }
        if let Some(c) = p.host.get("modSelect") {
            let any: Rc<dyn std::any::Any> = c;
            if let Ok(m) = any.downcast::<DropDownControl>() {
                let v: Vec<String> = m.list.borrow().iter().map(|x| x.label.clone().unwrap_or_default()).collect();
                q.insert("mods".into(), json!(v));
                q.insert("modSel".into(), json!(m.sel_index.get()));
            }
        }
        if let Some(c) = p.host.get("totalTattoos") {
            let any: Rc<dyn std::any::Any> = c;
            if let Ok(b) = any.downcast::<ButtonControl>() {
                q.insert("tattooCount".into(), json!(b.label.get(g)));
            }
        }
        if let Some(c) = p.host.get("edit") {
            let any: Rc<dyn std::any::Any> = c;
            if let Ok(e) = any.downcast::<EditControl>() {
                q.insert("edit".into(), json!(e.buf.borrow().clone()));
            }
        }
        if let Some(c) = p.host.get("msg") {
            let any: Rc<dyn std::any::Any> = c;
            if let Ok(l) = any.downcast::<LabelControl>() {
                q.insert("msg".into(), json!(l.label.get(g)));
            }
        }
        if p.title.get(g) == "Find a Timeless Jewel" {
            q.insert("timeless".into(), crate::tabs::timeless::debug_dump(g));
        }
        d["popup"] = serde_json::Value::Object(q);
    }
    d
}

/// What `tools/sg-oracle/rust-input.ps1` reads after each frame of the real window: the view and the screen position of an unallocated node next to the tree.
pub fn debug_input_state(g: &Gfx) -> serde_json::Value {
    use serde_json::json;
    let b = main().build_mode.clone();
    if main().mode.borrow().as_deref() != Some("BUILD") || b.tree_tab.borrow().is_none() {
        return json!({ "ready": false });
    }
    let root = crate::launch::launch().cfg.script_path.clone();
    let mut tv = b.tree_view.borrow_mut();
    let Some(v) = tv.as_mut() else { return json!({ "ready": false }) };
    let eng = b.engine.borrow();
    let e = eng.as_ref().expect("engine");
    let spec = e.spec_rc();
    let art = v.art(&root, &spec.tree);
    let (w, h) = crate::common::screen_size();
    let t = tab();
    let vw = w - 312.0;
    let vh = h - 32.0 - (if t.show_convert.get() { 64.0 } else { 32.0 }) - 24.0;
    let scale = f64::min(vw, vh) / art.size * v.zoom;
    let on_screen = |n: &pob_tree::SpecNode| {
        let tn = n.tree_node();
        let (x, y) = (
            tn.x? * scale + v.zoom_x + 312.0 + vw / 2.0,
            tn.y? * scale + v.zoom_y + 32.0 + vh / 2.0,
        );
        (x > 320.0 && x < w - 8.0 && y > 40.0 && y < 32.0 + vh - 8.0).then_some((x.floor(), y.floor()))
    };
    let target = spec
        .nodes
        .iter()
        .filter(|n| {
            !n.alloc
                && n.ty() == pob_tree::NodeType::Normal
                && n.path.as_ref().is_some_and(|p| p.len() <= 4)
                && n.ascendancy_name().is_none()
        })
        .filter_map(|n| on_screen(n).map(|(x, y)| (n.id, x, y, n.path.as_ref().map_or(0, Vec::len))))
        .min_by_key(|t| t.0)
        .map(|(id, x, y, path)| json!({ "id": id, "x": x, "y": y, "path": path }));
    let _ = g;
    json!({
        "ready": true,
        "zoomLevel": v.zoom_level,
        "zoomX": v.zoom_x,
        "zoomY": v.zoom_y,
        "hover": v.hover_node.map(|i| spec.nodes[i].id),
        "alloc": spec.alloc_nodes.len(),
        "search": v.search_str,
        "target": target,
    })
}

/// The test harness: the draw and input order of the open popup's controls = `pairs(popup.controls)` of the oracle process (R13: a label without a colour escape takes the colour its
/// predecessor left, ControlHost.lua `DrawControls`).
pub fn set_popup_order_sample(names: &[String]) {
    if let Some(p) = main().popups.borrow().first() {
        let rank = |k: &str| names.iter().position(|n| n == k).unwrap_or(usize::MAX);
        p.host.controls.borrow_mut().sort_by(|a, _, b, _| rank(a).cmp(&rank(b)));
    }
}

/// The test harness: blocks until the power walk ended, as the oracle's frames do.
pub fn wait_power(g: &Gfx) {
    let t = tab();
    t.finish_power(g);
}

// --- the class drop-downs of Modules/Build.lua and the class switch ----------------------------------------------------------------------

fn after_class_edit(g: &mut Gfx) {
    if let Some(t) = main().build_mode.tree_tab.borrow().clone() {
        t.add_undo_state();
    }
    let b = main().build_mode.clone();
    b.set_title_pub(g);
    b.build_flag.set(true);
}

fn ensure_undo_if_tab() {
    if let Some(t) = main().build_mode.tree_tab.borrow().clone() {
        t.ensure_undo();
    }
}

/// `SelectClass` + for a node click (PassiveTreeView.lua:476-481) `SelectAscendClass` and `allocateClickedAscendancy`.
pub fn switch_class(g: &mut Gfx, class_id: usize, ascend: Option<(usize, u32)>) {
    ensure_undo_if_tab();
    ok_or_panic(with_engine(|e| e.select_class(class_id as f64)));
    if let Some((a, node)) = ascend {
        ok_or_panic(with_engine(|e| e.select_ascend_class(a as f64)));
        let spec = engine_read(pob_engine::engine::Engine::spec_rc);
        if spec.node(node).is_some_and(|n| !n.alloc) {
            ok_or_panic(with_engine(|e| e.alloc_node(node, None)));
        }
    }
    after_class_edit(g);
}

/// The class-change confirmation (Build.lua:267-279, PassiveTreeView.lua:484-500).
pub fn confirm_class_change(g: &mut Gfx, class_id: usize, class_name: &str, ascend: Option<(usize, u32)>) {
    main().open_confirm_popup(
        g,
        "Class Change",
        &format!(
            "Changing class to {class_name} will reset your passive tree.\nThis can be avoided by connecting one of the {class_name} starting nodes to your tree."
        ),
        "Continue",
        Rc::new(move |g: &mut Gfx| switch_class(g, class_id, ascend)),
        Some((
            "Connect Path",
            Rc::new(move |g: &mut Gfx| {
                ensure_undo_if_tab();
                if ok_or_panic(with_engine(|e| e.connect_to_class(class_id))) {
                    switch_class(g, class_id, ascend);
                }
            }),
        )),
    );
}

/// `controls.classDrop` selFunc (Build.lua:259-282).
pub fn class_drop_selected(g: &mut Gfx, v: &DropItem) {
    let class_id = v.data.as_ref().and_then(|d| d.downcast_ref::<usize>()).copied().expect("classId");
    let cur = engine_read(|e| e.spec_rc().cur_class_id);
    if Some(class_id) != cur {
        let (used, connected) = engine_read(|e| (e.spec_rc().count_alloc_nodes().used, e.is_class_connected(class_id)));
        if used == 0 || connected {
            switch_class(g, class_id, None);
        } else {
            confirm_class_change(g, class_id, v.label.as_deref().unwrap_or(""), None);
        }
    }
}

/// `controls.ascendDrop` selFunc (Build.lua:283-288).
pub fn ascend_drop_selected(g: &mut Gfx, v: &DropItem) {
    let id = v
        .data
        .as_ref()
        .and_then(|d| d.downcast_ref::<usize>())
        .copied()
        .expect("ascendClassId");
    ensure_undo_if_tab();
    ok_or_panic(with_engine(|e| e.select_ascend_class(id as f64)));
    after_class_edit(g);
}

/// `controls.secondaryAscendDrop` selFunc (Build.lua:291-299).
pub fn secondary_drop_selected(g: &mut Gfx, v: &DropItem) {
    let id = v
        .data
        .as_ref()
        .and_then(|d| d.downcast_ref::<f64>())
        .copied()
        .expect("ascendClassId");
    ensure_undo_if_tab();
    ok_or_panic(with_engine(|e| e.select_secondary_ascend_class(id)));
    after_class_edit(g);
}

impl TreeTab {
    /// The state `SetActiveSpec` leaves at the load: `showConvert` and `versionSelect`.
    pub fn sync_active_spec_quiet(&self) {
        let ver = engine_read(|e| e.spec_rc().tree_version.clone());
        self.show_convert.set(!ver.starts_with(latest_tree_version()));
        let ix = self
            .version_select
            .list
            .borrow()
            .iter()
            .position(|v| v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).is_some_and(|s| *s == ver));
        if let Some(i) = ix {
            self.version_select.sel_index.set(i + 1);
        }
        self.active_compare_spec
            .set(engine_read(pob_engine::engine::Engine::spec_count).min(1));
    }
}
