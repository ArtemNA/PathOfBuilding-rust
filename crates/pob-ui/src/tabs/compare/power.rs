//! `CompareTab:ComparePowerBuilder` + `RunComparePowerReport` (CompareTab.lua:2516-3157) and `Classes/ComparePowerReportListControl.lua`.
//!
//! The coroutine with its 100 ms quantum is a queue of jobs run in `Draw`: the same `GetTime() - start > 100` check between the jobs, the same progress.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use pob_calc::env::{CalcOverride, OverrideNode};
use pob_calc::pob_build::{InputValue, SocketGroupModel};
use pob_calc::power::PowerStat;
use pob_data::config_options::{ConfigVar, config_options};
use pob_engine::compare_tab::model_groups_mut;
use pob_engine::display::format_num_sep;
use pob_engine::engine::Engine;
use pob_engine::stats::DISPLAY_STATS;
use pob_gfx::{Frame, Gfx};
use pob_items::item::Item;
use pob_lua::format::Arg;

use super::{CompareEntry, CompareTab};
use crate::colors::color_code;
use crate::control::{Control, rect};
use crate::controls::list::{Column, ListControl, ListHooks};
use crate::modules::build::{BuildMode, DrawControl};
use crate::modules::main::main;
use crate::sg;
use crate::tooltip::Tooltip;

const CLUSTER_NODE_OFFSET: u32 = 65536;

fn code(n: &str) -> String {
    color_code(n).unwrap_or_else(|| panic!("colorCodes.{n}"))
}

/// One row of `comparePowerResults`.
pub struct PowerRow {
    pub category: &'static str,
    pub category_color: String,
    pub name_color: String,
    pub name: String,
    pub node_id: Option<u32>,
    pub item: Option<Rc<Item>>,
    pub impact: f64,
    pub combined_impact_str: String,
    pub path_dist: Option<f64>,
    pub per_point: Option<f64>,
    pub per_point_str: Option<String>,
}

/// A row by identity.
#[derive(Clone)]
pub struct PowerRowRef(pub Rc<PowerRow>);

impl std::ops::Deref for PowerRowRef {
    type Target = PowerRow;
    fn deref(&self) -> &PowerRow {
        &self.0
    }
}

impl PartialEq for PowerRowRef {
    fn eq(&self, o: &Self) -> bool {
        Rc::ptr_eq(&self.0, &o.0)
    }
}

/// `formatImpact` (:2627-2654): (str, displayVal, combinedStr, isZero).
struct Formatter {
    fmt: String,
    mult: f64,
    lower_is_better: bool,
    base: f64,
}

struct Impact {
    str_: String,
    val: f64,
    combined: String,
    is_zero: bool,
}

impl Formatter {
    fn of(stat: &PowerStat, base: f64) -> Self {
        let ds = DISPLAY_STATS.iter().find(|d| d.stat == stat.stat.as_deref());
        Self {
            fmt: format!("%{}", ds.and_then(|d| d.fmt).unwrap_or(".1f")),
            mult: if ds.is_some_and(|d| d.pc || d.mod_) { 100.0 } else { 1.0 },
            lower_is_better: ds.is_some_and(|d| d.lower_is_better),
            base,
        }
    }

    fn format(&self, impact: f64) -> Impact {
        let val = impact * self.mult;
        let raw = pob_lua::format::format(&self.fmt, &[Arg::Num(val)]).unwrap_or_else(|e| panic!("{e}"));
        let is_zero = raw.trim().parse::<f64>().is_ok_and(|x| x == 0.0);
        let num = format_num_sep(&raw, &BuildMode::display_settings());
        let positive = (val > 0.0 && !self.lower_is_better) || (val < 0.0 && self.lower_is_better);
        let negative = (val < 0.0 && !self.lower_is_better) || (val > 0.0 && self.lower_is_better);
        let color = if positive {
            code("POSITIVE")
        } else if negative {
            code("NEGATIVE")
        } else {
            "^7".to_string()
        };
        let sign = if val > 0.0 { "+" } else { "" };
        let str_ = format!("{color}{sign}{num}");
        let percent = if self.base != 0.0 { impact / self.base.abs() * 100.0 } else { 0.0 };
        let mut combined = str_.clone();
        if percent != 0.0 {
            let pct = pob_lua::format::format("%+.1f%%", &[Arg::Num(percent)]).unwrap_or_else(|e| panic!("{e}"));
            combined = format!("{str_} ^7({color}{pct}^7)");
        }
        Impact {
            str_,
            val,
            combined,
            is_zero,
        }
    }
}

/// `GetGemGrantedEffect` name and `support` over a run's gem.
fn gem_effect(e: &Engine, gem: &pob_calc::skills_tab::GemInstance) -> Option<(String, bool)> {
    e.compare_gem_effect(gem)
}

/// `gem.grantedEffect` itself: only a skill without a gem has it, `gemData` is another field.
fn own_effect(e: &Engine, gem: &pob_calc::skills_tab::GemInstance) -> Option<(String, bool)> {
    let bare = pob_calc::skills_tab::GemInstance {
        gem_data: None,
        ..gem.clone()
    };
    e.compare_gem_effect(&bare)
}

/// `gem.grantedEffect and gem.grantedEffect.name or gem.nameSpec` (:2479).
pub(super) fn gem_name(e: &Engine, gem: &pob_calc::skills_tab::GemInstance) -> Option<String> {
    own_effect(e, gem).map(|x| x.0).or_else(|| gem.name_spec.clone())
}

/// `GetSocketGroupSignature` (:2476-2486).
fn group_signature(e: &Engine, group: &pob_calc::skills_tab::SocketGroup) -> String {
    let mut names: Vec<String> = group.gem_list.iter().filter_map(|g| gem_name(e, g)).collect();
    names.sort();
    names.join("+")
}

/// `GetSocketGroupLabel` (:2489-2513).
fn group_label(e: &Engine, group: &pob_calc::skills_tab::SocketGroup) -> String {
    let mut names: Vec<String> = group
        .gem_list
        .iter()
        .filter(|g| !own_effect(e, g).is_some_and(|x| x.1))
        .filter_map(|g| gem_name(e, g))
        .collect();
    if names.is_empty() {
        names = group.gem_list.iter().filter_map(|g| gem_name(e, g)).collect();
    }
    if names.is_empty() {
        return "(empty group)".to_string();
    }
    names.join(" + ")
}

/// `NormalizeConfigVals` (:1123-1130) as what `pVal ~= cVal` compares.
fn normalized(var: &ConfigVar, v: Option<&InputValue>) -> Option<InputValue> {
    match var.ty.as_deref() {
        Some("check") => Some(v.cloned().unwrap_or(InputValue::Bool(false))),
        Some("count" | "integer" | "float") => Some(v.cloned().unwrap_or(InputValue::Num(0.0))),
        _ => v.cloned(),
    }
}

fn input_tostring(v: &InputValue) -> String {
    match v {
        InputValue::Num(n) => pob_lua::tostring_num(*n),
        InputValue::Bool(b) => b.to_string(),
        InputValue::Str(s) => s.to_string(),
    }
}

/// `FormatConfigValue` (:1105-1119).
pub(super) fn format_config_value(var: &ConfigVar, v: Option<&InputValue>) -> String {
    let Some(v) = v else { return "^8(not set)".to_string() };
    match var.ty.as_deref() {
        // `val and Yes or No`: every value but false is true
        Some("check") => {
            if matches!(v, InputValue::Bool(false)) {
                format!("{}No", code("NEGATIVE"))
            } else {
                format!("{}Yes", code("POSITIVE"))
            }
        }
        Some("list") if var.list.is_some() => {
            let s = input_tostring(v);
            var.list
                .as_ref()
                .and_then(|l| l.iter().find(|i| scalar_string(&i.val) == s))
                .map_or(s, |i| i.label.clone())
        }
        _ => input_tostring(v),
    }
}

pub(super) fn scalar_string(s: &pob_data::wire::Scalar) -> String {
    match s {
        pob_data::wire::Scalar::Num(n) => pob_lua::tostring_num(*n),
        pob_data::wire::Scalar::Bool(b) => b.to_string(),
        pob_data::wire::Scalar::Str(t) => t.clone(),
    }
}

/// `stripColors` (:3052-3054).
fn strip_colors(s: &str) -> String {
    let once = |x: &str, p: &str| {
        String::from_utf8_lossy(
            &pob_lua::gsub(x.as_bytes(), p.as_bytes(), pob_lua::Repl::Str(b""), None)
                .expect("pattern")
                .0,
        )
        .into_owned()
    };
    once(&once(s, "%^%x"), "%^x%x%x%x%x%x%x")
}

/// The state of the coroutine.
pub struct PowerRun {
    jobs: std::collections::VecDeque<Job>,
    results: Vec<PowerRow>,
    total: usize,
    processed: usize,
    fmt: Formatter,
    stat: PowerStat,
    entry: Rc<CompareEntry>,
    cache: HashMap<String, f64>,
    c_groups: Vec<SocketGroupModel>,
}

/// One step between two `coroutine.yield` checks of the builder.
enum Job {
    Node(u32),
    Item { slot: String },
    Jewel { node_id: u32 },
    Group(usize),
    Support { group: usize, gem: usize, name: String },
    Config(&'static ConfigVar),
}

fn base_slots(tab: &CompareTab, entry: &CompareEntry) -> Vec<String> {
    let mut slots: Vec<String> = [
        "Weapon 1",
        "Weapon 2",
        "Helmet",
        "Body Armour",
        "Gloves",
        "Boots",
        "Amulet",
        "Ring 1",
        "Ring 2",
        "Belt",
        "Flask 1",
        "Flask 2",
        "Flask 3",
        "Flask 4",
        "Flask 5",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    if tab.should_show_ring3(entry) {
        slots.insert(9, "Ring 3".to_string());
    }
    slots
}

impl CompareTab {
    /// `ShouldShowRing3` (:3668-3674).
    pub(super) fn should_show_ring3(&self, entry: &CompareEntry) -> bool {
        let has = |b: &BuildMode| {
            let eng = b.engine.borrow();
            let db = eng.as_ref().expect("engine").result().env.player.borrow().mod_db.clone();
            matches!(db.borrow().flag(None, &["AdditionalRingSlot"]), Ok(Some(true)))
        };
        has(&self.primary()) || has(&entry.b)
    }

    /// `AddAbyssSockets` (:3679-3693).
    pub(super) fn add_abyss_sockets(&self, g: &Gfx, entry: &CompareEntry, dest: &mut Vec<String>, require_both: bool) {
        let p = self.primary();
        let shown = |b: &BuildMode, name: &str| {
            let tab = b.items_tab.borrow().clone().expect("itemsTab");
            let slots = tab.slots.borrow();
            slots.get(name).is_some_and(|s| s.drop.base.shown.get(g))
        };
        for slot in [
            "Weapon 1",
            "Weapon 2",
            "Weapon 1 Swap",
            "Weapon 2 Swap",
            "Helmet",
            "Body Armour",
            "Gloves",
            "Boots",
            "Belt",
        ] {
            for n in 1..=6 {
                let name = format!("{slot} Abyssal Socket {n}");
                let (main_has, cmp_has) = (shown(&p, &name), shown(&entry.b, &name));
                if (require_both && main_has && cmp_has) || (!require_both && (main_has || cmp_has)) {
                    dest.push(name);
                }
            }
        }
    }

    /// `GetJewelComparisonSlots` (:1377-1441).
    pub(super) fn jewel_comparison_slots(&self, entry: &CompareEntry) -> Vec<JewelSlot> {
        let p = self.primary();
        let (peng, ceng) = (p.engine.borrow(), entry.b.engine.borrow());
        let (pe, ce) = (peng.as_ref().expect("engine"), ceng.as_ref().expect("engine"));
        let (ps, cs) = (pe.spec_rc(), ce.spec_rc());
        let mut ids: Vec<u32> = Vec::new();
        for s in [&ps, &cs] {
            for (id, item) in &s.jewels.0 {
                let id = *id as u32;
                if *item > 0.0 && !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        ids.sort_unstable();
        let (ptab, ctab) = (p.items_tab.borrow().clone(), entry.b.items_tab.borrow().clone());
        let mut out = Vec::new();
        for id in ids {
            let jewel = |s: &pob_tree::PassiveSpec, e: &Engine| s.jewels.get(f64::from(id)).and_then(|i| e.item_by_id(*i));
            let (pi, ci) = (jewel(&ps, pe), jewel(&cs, ce));
            if pi.is_none() && ci.is_none() {
                continue;
            }
            let slot_name = format!("Jewel {id}");
            let socket_label = |t: &Option<Rc<crate::tabs::items::ItemsTab>>| {
                t.as_ref()
                    .and_then(|t| t.sockets.borrow().get(&id).map(|s| s.label.borrow().clone()))
                    .filter(|l| !l.is_empty())
            };
            let label = socket_label(&ptab)
                .or_else(|| socket_label(&ctab))
                .unwrap_or_else(|| slot_name.clone());
            out.push(JewelSlot {
                label,
                node_id: id,
                p_item: pi,
                c_item: ci,
                slot_name,
                p_node_allocated: ps.node(id).is_some_and(|n| n.alloc),
                c_node_allocated: cs.node(id).is_some_and(|n| n.alloc),
            });
        }
        out
    }

    /// The counts and the order of `ComparePowerBuilder` (:2535-2615), as jobs.
    fn start_power(&self, g: &Gfx, entry: &Rc<CompareEntry>, stat: PowerStat) -> Result<Option<PowerRun>, String> {
        let c = self.c();
        let p = self.primary();
        let mut jobs: std::collections::VecDeque<Job> = Default::default();
        let mut total = 0;
        let (cats_tree, cats_items, cats_gems, cats_support, cats_config) = (
            c.power_tree_check.state.get(),
            c.power_items_check.state.get(),
            c.power_gems_check.state.get(),
            c.power_support_gems_check.state.get(),
            c.power_config_check.state.get(),
        );
        let (peng, ceng) = (p.engine.borrow(), entry.b.engine.borrow());
        let (pe, ce) = (peng.as_ref().expect("engine"), ceng.as_ref().expect("engine"));
        if cats_tree {
            let (ps, cs) = (pe.spec_rc(), ce.spec_rc());
            for id in cs.alloc_table_pairs() {
                if id < CLUSTER_NODE_OFFSET && !ps.node(id).is_some_and(|n| n.alloc) {
                    let Some(n) = ps.node(id) else { continue };
                    let t = n.tree_node();
                    if matches!(
                        t.ty,
                        pob_tree::tree::NodeType::Normal | pob_tree::tree::NodeType::Notable | pob_tree::tree::NodeType::Keystone
                    ) && t.ascendancy_name.is_none()
                    {
                        total += 1;
                        // :2667-2668 `modKey ~= ""` skips the job, not the count
                        if !n.mod_key().is_empty() {
                            jobs.push_back(Job::Node(id));
                        }
                    }
                }
            }
        }
        if cats_items {
            let mut slots = base_slots(self, entry);
            drop((peng, ceng));
            self.add_abyss_sockets(g, entry, &mut slots, true);
            let ceng = entry.b.engine.borrow();
            let ce = ceng.as_ref().expect("engine");
            for slot in slots {
                let sel = ce.slot_sel_item(&slot);
                if ce.item_by_id(sel).is_some() {
                    total += 1;
                }
                jobs.push_back(Job::Item { slot });
            }
            drop(ceng);
            for j in self.jewel_comparison_slots(entry) {
                if j.c_item.is_some() {
                    total += 1;
                }
                jobs.push_back(Job::Jewel { node_id: j.node_id });
            }
        } else {
            drop((peng, ceng));
        }
        let (peng, ceng) = (p.engine.borrow(), entry.b.engine.borrow());
        let (pe, ce) = (peng.as_ref().expect("engine"), ceng.as_ref().expect("engine"));
        let c_groups = ce.compare_group_models()?;
        if cats_gems {
            let n = ce.socket_groups().len();
            total += n;
            for i in 0..n {
                jobs.push_back(Job::Group(i));
            }
        }
        if cats_support {
            let (cm, pm) = (ce.main_socket_group() as usize, pe.main_socket_group() as usize);
            let (cgs, pgs) = (ce.socket_groups(), pe.socket_groups());
            if let (Some(cg), Some(pg)) = (cgs.get(cm.wrapping_sub(1)), pgs.get(pm.wrapping_sub(1))) {
                let have: HashSet<String> = pg
                    .gem_list
                    .iter()
                    .filter_map(|g| gem_effect(pe, g))
                    .filter(|x| x.1)
                    .map(|x| x.0)
                    .collect();
                for (k, gem) in cg.gem_list.iter().enumerate() {
                    if let Some((name, true)) = gem_effect(ce, gem)
                        && !have.contains(&name)
                    {
                        total += 1;
                        jobs.push_back(Job::Support {
                            group: pm - 1,
                            gem: k,
                            name,
                        });
                    }
                }
            }
        }
        if cats_config {
            for var in &config_options().vars {
                let Some(v) = var.var.as_deref() else { continue };
                if var.apply.is_some() && var.ty.as_deref() != Some("text") {
                    let (pv, cv) = (pe.config_state().input.get(v).cloned(), ce.config_state().input.get(v).cloned());
                    if normalized(var, pv.as_ref()) != normalized(var, cv.as_ref()) {
                        total += 1;
                        jobs.push_back(Job::Config(var));
                    }
                }
            }
        }
        if total == 0 {
            return Ok(None);
        }
        let base = pe.compare_base_stat(&stat)?;
        Ok(Some(PowerRun {
            jobs,
            results: Vec::new(),
            total,
            processed: 0,
            fmt: Formatter::of(&stat, base),
            stat,
            entry: entry.clone(),
            cache: HashMap::new(),
            c_groups,
        }))
    }

    /// `RunComparePowerReport` (:3129-3157).
    pub(super) fn run_compare_power_report(&self, g: &mut Gfx, entry: &Rc<CompareEntry>) {
        if self.compare_power_id.borrow().as_ref().is_none_or(|e| !Rc::ptr_eq(e, entry)) {
            *self.compare_power_id.borrow_mut() = Some(entry.clone());
            self.compare_power_dirty.set(true);
        }
        let stat = self.compare_power_stat.borrow().clone();
        if self.compare_power_dirty.get()
            && let Some(stat) = stat
        {
            self.compare_power_dirty.set(false);
            *self.compare_power_results.borrow_mut() = None;
            self.compare_power_progress.set(0.0);
            self.compare_power_list_synced.set(false);
            match self.start_power(g, entry, stat).unwrap_or_else(|e| panic!("{e}")) {
                Some(run) => {
                    *self.compare_power_run.borrow_mut() = Some(run);
                }
                None => {
                    *self.compare_power_results.borrow_mut() = Some(Vec::new());
                    self.compare_power_progress.set(100.0);
                }
            }
        }
        // one `coroutine.resume`
        let Some(mut run) = self.compare_power_run.borrow_mut().take() else {
            return;
        };
        let start = g.get_time();
        while let Some(job) = run.jobs.pop_front() {
            let counted = self.run_job(g, &mut run, job);
            if counted && g.get_time() - start > 100 {
                self.compare_power_progress
                    .set((run.processed as f64 / run.total as f64 * 100.0).floor());
                *self.compare_power_run.borrow_mut() = Some(run);
                return;
            }
        }
        let rows: Vec<PowerRowRef> = run.results.into_iter().map(|r| PowerRowRef(Rc::new(r))).collect();
        *self.compare_power_results.borrow_mut() = Some(rows);
        self.compare_power_progress.set(100.0);
    }

    /// One job; `true` when the Lua reaches its yield check after it.
    fn run_job(&self, g: &mut Gfx, run: &mut PowerRun, job: Job) -> bool {
        let p = self.primary();
        match job {
            Job::Node(id) => {
                let peng = p.engine.borrow();
                let pe = peng.as_ref().expect("engine");
                let spec = pe.spec_rc();
                let n = spec.node(id).expect("node");
                let key = n.mod_key().to_string();
                let impact = match run.cache.get(&key) {
                    Some(v) => *v,
                    None => {
                        let i = spec.node_idx(id).expect("node");
                        let v = pe
                            .compare_override_impact(
                                CalcOverride {
                                    add_nodes: Some(vec![OverrideNode::Spec(i)]),
                                    ..CalcOverride::default()
                                },
                                &run.stat,
                            )
                            .unwrap_or_else(|e| panic!("{e}"));
                        run.cache.insert(key, v);
                        v
                    }
                };
                // :2677-2681 `pNode.pathDist or 0`
                let mut dist = n.path_dist;
                if dist == 0.0 {
                    dist = n.path.as_ref().map_or(0, Vec::len) as f64;
                    if dist == 0.0 {
                        dist = 1.0;
                    }
                }
                let per_point = impact / dist;
                let f = run.fmt.format(impact);
                let pp = run.fmt.format(per_point);
                if !f.is_zero {
                    run.results.push(PowerRow {
                        category: "Tree",
                        category_color: "^7".into(),
                        name_color: "^7".into(),
                        name: n.tree_node().dn.clone(),
                        node_id: Some(id),
                        item: None,
                        impact: f.val,
                        combined_impact_str: f.combined,
                        path_dist: Some(dist),
                        per_point: Some(per_point * run.fmt.mult),
                        per_point_str: Some(pp.str_),
                    });
                }
                run.processed += 1;
                true
            }
            Job::Item { slot } => {
                let (c_item, p_same) = {
                    let (peng, ceng) = (p.engine.borrow(), run.entry.b.engine.borrow());
                    let (pe, ce) = (peng.as_ref().expect("engine"), ceng.as_ref().expect("engine"));
                    let ci = ce.item_by_id(ce.slot_sel_item(&slot));
                    let pi = pe.item_by_id(pe.slot_sel_item(&slot));
                    let same = matches!((&pi, &ci), (Some(a), Some(b)) if a.name == b.name);
                    (ci, same)
                };
                if let Some(ci) = c_item.filter(|_| !p_same) {
                    let mut new_item = Item::parse_with(&ci.raw, None, false);
                    new_item.normalise_quality();
                    let (impact, jewels) = self.item_impact(g, run, &slot, &new_item);
                    let f = run.fmt.format(impact);
                    if !f.is_zero {
                        let color = color_code(&ci.rarity).unwrap_or_else(|| code("NORMAL"));
                        let extra = if jewels > 0 {
                            format!(", {jewels} Jewel{}", if jewels > 1 { "s" } else { "" })
                        } else {
                            String::new()
                        };
                        run.results.push(PowerRow {
                            category: "Item",
                            category_color: color.clone(),
                            name_color: color,
                            name: format!("{}, {slot}{extra}", ci.name),
                            node_id: None,
                            item: Some(Rc::new(new_item)),
                            impact: f.val,
                            combined_impact_str: f.combined,
                            path_dist: None,
                            per_point: None,
                            per_point_str: None,
                        });
                    }
                }
                run.processed += 1;
                true
            }
            Job::Jewel { node_id } => {
                let slots = self.jewel_comparison_slots(&run.entry);
                let j = slots.iter().find(|j| j.node_id == node_id).expect("jewel");
                if let Some(ci) = j
                    .c_item
                    .as_ref()
                    .filter(|ci| !j.p_item.as_ref().is_some_and(|pi| pi.name == ci.name))
                {
                    let mut new_item = Item::parse_with(&ci.raw, None, false);
                    new_item.normalise_quality();
                    let new_item = Rc::new(new_item);
                    let peng = p.engine.borrow();
                    let pe = peng.as_ref().expect("engine");
                    let mut best: Option<f64> = None;
                    let mut label = j.label.clone();
                    if j.p_node_allocated {
                        best = Some(
                            pe.compare_override_impact(
                                CalcOverride {
                                    rep_slot_name: Some(j.slot_name.clone()),
                                    rep_item: Some(new_item.clone()),
                                    ..CalcOverride::default()
                                },
                                &run.stat,
                            )
                            .unwrap_or_else(|e| panic!("{e}")),
                        );
                    } else {
                        // `orderedSlots` with a `nodeId` (:2822-2835): the sockets of the items tab
                        let tab = p.items_tab.borrow().clone().expect("itemsTab");
                        let sockets: Vec<u32> = tab.sockets.borrow().keys().copied().collect();
                        let spec = pe.spec_rc();
                        for id in sockets {
                            let Some(n) = spec.node(id) else { continue };
                            let ov = CalcOverride {
                                rep_slot_name: Some(format!("Jewel {id}")),
                                rep_item: Some(new_item.clone()),
                                add_nodes: (!n.alloc).then(|| vec![OverrideNode::Spec(spec.node_idx(id).expect("node"))]),
                                ..CalcOverride::default()
                            };
                            let impact = pe.compare_override_impact(ov, &run.stat).unwrap_or_else(|e| panic!("{e}"));
                            if best.is_none_or(|b| impact > b) {
                                best = Some(impact);
                                label = format!("{} (best socket)", j.label);
                            }
                        }
                    }
                    if let Some(b) = best {
                        let f = run.fmt.format(b);
                        if !f.is_zero {
                            let color = color_code(&ci.rarity).unwrap_or_else(|| code("NORMAL"));
                            run.results.push(PowerRow {
                                category: "Item",
                                category_color: color.clone(),
                                name_color: color,
                                name: format!("{}, {label}", ci.name),
                                node_id: None,
                                item: Some(new_item),
                                impact: f.val,
                                combined_impact_str: f.combined,
                                path_dist: None,
                                per_point: None,
                                per_point_str: None,
                            });
                        }
                    }
                }
                run.processed += 1;
                true
            }
            Job::Group(i) => {
                let (peng, ceng) = (p.engine.borrow(), run.entry.b.engine.borrow());
                let (pe, ce) = (peng.as_ref().expect("engine"), ceng.as_ref().expect("engine"));
                let cg = &ce.socket_groups()[i];
                let sig = group_signature(ce, cg);
                let known = pe.socket_groups().iter().any(|g| group_signature(pe, g) == sig);
                if !sig.is_empty() && !known {
                    let model = run.c_groups.get(i).cloned();
                    p.build_flag.set(true);
                    let r = pe.compare_edited_impact(
                        |m| {
                            if let (Some(gs), Some(g)) = (model_groups_mut(m), model) {
                                gs.push(g);
                            }
                        },
                        &run.stat,
                    );
                    match r {
                        Err(e) => g.con_printf(&format!("Compare power (gem): {e}")),
                        Ok(impact) => {
                            let f = run.fmt.format(impact);
                            if !f.is_zero {
                                run.results.push(PowerRow {
                                    category: "Skill gem",
                                    category_color: code("GEM"),
                                    name_color: code("GEM"),
                                    name: group_label(ce, cg),
                                    node_id: None,
                                    item: None,
                                    impact: f.val,
                                    combined_impact_str: f.combined,
                                    path_dist: None,
                                    per_point: None,
                                    per_point_str: None,
                                });
                            }
                        }
                    }
                }
                run.processed += 1;
                true
            }
            Job::Support { group, gem, name } => {
                let peng = p.engine.borrow();
                let pe = peng.as_ref().expect("engine");
                let c_main = run.entry.b.engine.borrow().as_ref().expect("engine").main_socket_group() as usize;
                let model = run.c_groups.get(c_main.wrapping_sub(1)).and_then(|g| g.gems.get(gem)).cloned();
                p.build_flag.set(true);
                let r = pe.compare_edited_impact(
                    |m| {
                        if let (Some(gs), Some(g)) = (model_groups_mut(m), model)
                            && let Some(grp) = gs.get_mut(group)
                        {
                            grp.gems.push(g);
                        }
                    },
                    &run.stat,
                );
                match r {
                    Err(e) => g.con_printf(&format!("Compare power (support gem): {e}")),
                    Ok(impact) => {
                        let f = run.fmt.format(impact);
                        if !f.is_zero {
                            run.results.push(PowerRow {
                                category: "Support gem",
                                category_color: code("GEM"),
                                name_color: code("GEM"),
                                name,
                                node_id: None,
                                item: None,
                                impact: f.val,
                                combined_impact_str: f.combined,
                                path_dist: None,
                                per_point: None,
                                per_point_str: None,
                            });
                        }
                    }
                }
                run.processed += 1;
                true
            }
            Job::Config(var) => {
                let v = var.var.as_deref().expect("var");
                let (peng, ceng) = (p.engine.borrow(), run.entry.b.engine.borrow());
                let (pe, ce) = (peng.as_ref().expect("engine"), ceng.as_ref().expect("engine"));
                let (pv, cv) = (pe.config_state().input.get(v).cloned(), ce.config_state().input.get(v).cloned());
                p.build_flag.set(true);
                let r = pe.compare_config_impact(v, cv.clone(), &run.stat);
                match r {
                    Err(e) => g.con_printf(&format!("Compare power (config): {e}")),
                    Ok(impact) => {
                        let f = run.fmt.format(impact);
                        if !f.is_zero {
                            let label = var.label.as_deref().unwrap_or(v);
                            let label = label.strip_suffix(':').unwrap_or(label);
                            let (pd, cd) = (
                                strip_colors(&format_config_value(var, pv.as_ref())),
                                strip_colors(&format_config_value(var, cv.as_ref())),
                            );
                            run.results.push(PowerRow {
                                category: "Config",
                                category_color: code("FRACTURED"),
                                name_color: "^7".into(),
                                name: format!("{label}  ({pd} -> {cd})"),
                                node_id: None,
                                item: None,
                                impact: f.val,
                                combined_impact_str: f.combined,
                                path_dist: None,
                                per_point: None,
                                per_point_str: None,
                            });
                        }
                    }
                }
                run.processed += 1;
                true
            }
        }
    }

    /// :2738-2779: the abyssal jewels of the compared item go through the live slots of the primary build around `calcFunc`.
    fn item_impact(&self, g: &mut Gfx, run: &PowerRun, slot: &str, new_item: &Item) -> (f64, usize) {
        let p = self.primary();
        let ov = CalcOverride {
            rep_slot_name: Some(slot.to_string()),
            rep_item: Some(Rc::new(Item::parse_with(&new_item.build_raw(), None, false))),
            ..CalcOverride::default()
        };
        let mut old: Vec<(String, f64)> = Vec::new();
        let mut added: Vec<f64> = Vec::new();
        if new_item.abyssal_socket_count > 0.0 {
            let ptab = p.items_tab.borrow().clone().expect("itemsTab");
            for idx in 1..=(new_item.abyssal_socket_count as usize) {
                let name = format!("{slot} Abyssal Socket {idx}");
                let jewel = {
                    let ceng = run.entry.b.engine.borrow();
                    let ce = ceng.as_ref().expect("engine");
                    let sel = ce.slot_sel_item(&name);
                    if sel > 0.0 { ce.item_by_id(sel) } else { None }
                };
                let pslot = ptab.slot(&name);
                old.push((name, pslot.sel_item_id.get()));
                pslot.set_sel_item_id(0.0);
                if let Some(j) = jewel {
                    let copy = Item::parse_with(&j.build_raw(), None, false);
                    let id = ptab.add_item(g, &copy, false, None);
                    added.push(id);
                    pslot.set_sel_item_id(id);
                }
            }
        }
        let impact = {
            let peng = p.engine.borrow();
            peng.as_ref()
                .expect("engine")
                .compare_override_impact(ov, &run.stat)
                .unwrap_or_else(|e| panic!("{e}"))
        };
        if new_item.abyssal_socket_count > 0.0 {
            let ptab = p.items_tab.borrow().clone().expect("itemsTab");
            for (name, id) in &old {
                ptab.slot(name).set_sel_item_id(*id);
            }
            for id in &added {
                ptab.delete_item(g, *id, false);
            }
        }
        (impact, added.len())
    }

    /// :3273-3299.
    pub(super) fn sync_power_list(&self) {
        let list = self.compare_power_list();
        let st = self.c().power_list_state.clone();
        if self.compare_power_run.borrow().is_some() {
            set_progress(&list, &st, self.compare_power_progress.get());
            self.compare_power_list_synced.set(false);
        } else if self.compare_power_results.borrow().is_some() && !self.compare_power_list_synced.get() {
            let stat = self.compare_power_stat.borrow().clone();
            let results = self.compare_power_results.borrow().clone();
            set_report(&list, &st, stat.as_ref(), results);
            self.compare_power_list_synced.set(true);
        } else if self.compare_power_stat.borrow().is_none() && !self.compare_power_list_synced.get() {
            set_report(&list, &st, None, None);
            self.compare_power_list_synced.set(true);
        }
        if let Some(s) = self.compare_power_stat.borrow().as_ref() {
            *st.impact_label.borrow_mut() = s.label.clone();
        }
    }

    pub(super) fn compare_power_list(&self) -> Rc<ListControl<PowerRowRef>> {
        self.c().power_report_list.clone()
    }

    /// `controls.comparePowerReportList` + the status text of its `Draw` (ComparePowerReportListControl.lua:13-29, :56-72).
    pub(super) fn new_power_list(tab: Weak<CompareTab>) -> (Rc<ListControl<PowerRowRef>>, Rc<DrawControl>, Rc<PowerListState>) {
        let width = 750.0;
        let state = Rc::new(PowerListState::default());
        *state.status_text.borrow_mut() = Some("Select a metric above to generate the power report.".to_string());
        let list: Rc<ListControl<PowerRowRef>> = ListControl::new(
            None,
            rect(0.0, 0.0, width, 250.0),
            18.0,
            Some("VERTICAL"),
            false,
            Vec::new(),
            false,
            Box::new(PowerHooks { state: state.clone(), tab }),
        );
        let col = |w: f64, label: crate::control::Prop<String>, sortable: bool| Column {
            width: Some(w.into()),
            label: Some(label),
            sortable,
            ..Column::default()
        };
        let st = state.clone();
        *list.col_list.borrow_mut() = vec![
            col(width * 0.10, "Category".to_string().into(), true),
            col(width * 0.44, "Name".to_string().into(), false),
            col(
                width * 0.22,
                crate::control::Prop::func(move |_| st.impact_label.borrow().clone()),
                true,
            ),
            col(width * 0.06, "Points".to_string().into(), true),
            col(width * 0.14, "Per Point".to_string().into(), true),
        ];
        list.col_labels.set(true);
        list.show_row_separators.set(true);
        let status = DrawControl::new(None, rect(0.0, 0.0, 0.0, 0.0));
        let (l, st) = (Rc::downgrade(&list), state.clone());
        *status.draw_fn.borrow_mut() = Some(Rc::new(move |_c: &DrawControl, f: &mut Frame<'_>| {
            let list = l.upgrade().expect("list");
            let text = st.status_text.borrow().clone();
            if list.list.borrow().is_empty()
                && let Some(text) = text
            {
                let (x, y) = list.get_pos(f);
                let (w, h) = list.get_size(f);
                sg::set_viewport(f, Some([x + 2.0, y + 20.0, w - 20.0, h - 22.0]));
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
                sg::draw_string(f, 4.0, 4.0, Some("LEFT"), 14.0, "VAR", &text);
                sg::set_viewport(f, None);
            }
        }));
        (list, status, state)
    }

    /// For the report list of the gate.
    pub(super) fn power_rows(&self) -> Vec<String> {
        self.compare_power_list()
            .list
            .borrow()
            .iter()
            .map(|r| {
                format!(
                    "{}|{}|{}|{}|{}",
                    r.category,
                    r.name,
                    r.combined_impact_str,
                    r.path_dist.map_or(String::new(), pob_lua::tostring_num),
                    r.per_point_str.clone().unwrap_or_default()
                )
            })
            .collect()
    }
}

/// `GetJewelComparisonSlots`' records.
pub struct JewelSlot {
    pub label: String,
    pub node_id: u32,
    pub p_item: Option<Rc<Item>>,
    pub c_item: Option<Rc<Item>>,
    pub slot_name: String,
    pub p_node_allocated: bool,
    pub c_node_allocated: bool,
}

#[derive(Default)]
pub struct PowerListState {
    pub impact_label: RefCell<String>,
    pub status_text: RefCell<Option<String>>,
    pub report: RefCell<Vec<PowerRowRef>>,
    last_hover: Cell<Option<usize>>,
}

pub struct PowerHooks {
    pub state: Rc<PowerListState>,
    tab: Weak<CompareTab>,
}

/// `SetReport` (:31-47).
fn set_report(list: &ListControl<PowerRowRef>, st: &PowerListState, stat: Option<&PowerStat>, report: Option<Vec<PowerRowRef>>) {
    *st.impact_label.borrow_mut() = stat.map_or_else(String::new, |s| s.label.clone());
    let rows = report.clone().unwrap_or_default();
    *st.report.borrow_mut() = rows;
    *st.status_text.borrow_mut() = if stat.is_some_and(|s| s.stat.is_some()) {
        if report.is_some_and(|r| !r.is_empty()) {
            None
        } else {
            Some("No differences found.".to_string())
        }
    } else {
        Some("Select a metric above to generate the power report.".to_string())
    };
    *list.list.borrow_mut() = st.report.borrow().clone();
    re_sort(list, 3);
}

/// `SetProgress` (:49-54).
fn set_progress(list: &ListControl<PowerRowRef>, st: &PowerListState, progress: f64) {
    if progress < 100.0 {
        *st.status_text.borrow_mut() = Some(format!("Calculating... {}%", pob_lua::tostring_num(progress)));
        list.list.borrow_mut().clear();
    }
}

/// `ReSort` (:74-104).
fn re_sort(list: &ListControl<PowerRowRef>, col: usize) {
    let mut l = list.list.borrow_mut();
    match col {
        1 => pob_engine::lua::lua_sort(&mut l, &mut |a: &PowerRowRef, b: &PowerRowRef| {
            if a.category == b.category {
                a.impact.abs() > b.impact.abs()
            } else {
                a.category < b.category
            }
        }),
        3 => pob_engine::lua::lua_sort(&mut l, &mut |a: &PowerRowRef, b: &PowerRowRef| a.impact > b.impact),
        4 => pob_engine::lua::lua_sort(&mut l, &mut |a: &PowerRowRef, b: &PowerRowRef| {
            let (x, y) = (a.path_dist.unwrap_or(99999.0), b.path_dist.unwrap_or(99999.0));
            if x == y { a.impact.abs() > b.impact.abs() } else { x < y }
        }),
        5 => pob_engine::lua::lua_sort(&mut l, &mut |a: &PowerRowRef, b: &PowerRowRef| {
            a.per_point.unwrap_or(-99999.0) > b.per_point.unwrap_or(-99999.0)
        }),
        _ => {}
    }
}

impl ListHooks<PowerRowRef> for PowerHooks {
    /// :149-165.
    fn get_row_value(&self, _l: &ListControl<PowerRowRef>, _g: &Gfx, column: usize, _index: usize, r: &PowerRowRef) -> String {
        match column {
            1 => format!("{}{}", r.category_color, r.category),
            2 => format!("{}{}", r.name_color, r.name),
            3 => r.combined_impact_str.clone(),
            4 => r.path_dist.map_or_else(String::new, pob_lua::tostring_num),
            5 => r.per_point_str.clone().unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn has_add_value_tooltip(&self) -> bool {
        true
    }

    /// :116-147.
    fn add_value_tooltip(&self, _l: &ListControl<PowerRowRef>, g: &Gfx, tooltip: &mut Tooltip, index: usize, r: &PowerRowRef) {
        // :57-60 `self.tooltip.updateParams = nil` on a new hover
        if self.state.last_hover.replace(Some(index)) != Some(index) {
            tooltip.clear(true);
        }
        if !main().popups.borrow().is_empty() {
            tooltip.clear(false);
            return;
        }
        let Some(tab) = self.tab.upgrade() else {
            tooltip.clear(false);
            return;
        };
        let build = tab.primary();
        if let (Some(id), "Tree") = (r.node_id, r.category) {
            let rev = build.engine.borrow().as_ref().expect("engine").output_revision();
            if tooltip.check_for_update((id, sg::is_key_down(g, "SHIFT"), rev)) {
                build.ensure_tree_view();
                let root = crate::launch::launch().cfg.script_path.clone();
                let eng = build.engine.borrow();
                let e = eng.as_ref().expect("engine");
                let spec = e.spec_rc();
                let mut tv = build.tree_view.borrow_mut();
                let view = tv.as_mut().expect("view");
                let art = view.art(&root, &spec.tree);
                crate::tree_tooltip::plain_node_tooltip(view, tooltip, g, e, &art, &spec, id);
            }
        } else if let (Some(item), "Item") = (&r.item, r.category) {
            let rev = build.engine.borrow().as_ref().expect("engine").output_revision();
            if tooltip.check_for_update((Rc::as_ptr(item) as usize, sg::is_key_down(g, "SHIFT"), rev)) {
                let items = build.items_tab.borrow().clone().expect("itemsTab");
                items.add_tooltip(tooltip, g, item, None, false);
            }
        } else {
            tooltip.clear(false);
        }
    }

    fn has_re_sort(&self) -> bool {
        true
    }

    fn re_sort(&self, list: &ListControl<PowerRowRef>, column: usize) {
        re_sort(list, column);
    }
}
