//! `Classes/TradeQueryGenerator.lua`: the weights of the trade mods for a slot, and the weighted query.
//! `Data/QueryMods.lua` is read in place in the checkout. `InitMods`' other branch (:379-560: download `api/trade/data/stats` with curl and regenerate the file) is reached only when the checkout lacks the file,
//! and it writes into the checkout: not ported, an error names it.
//! The coroutine (`ExecuteQuery`) is a walk over the same entries in the same order, one quantum of 50 ms per frame (:603-607).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_calc::env::{CalcOverride, OverrideNode};
use pob_calc::output::Output;
use pob_engine::engine::Engine;
use pob_gfx::Gfx;
use pob_items::Item;
use pob_items::item::ModLine;
use serde_json::{Value as Json, json};

use super::dkjson;
use super::helpers::{get_trade_category, hash_stats, new_plain_numeric_edit, trade_stats};
use super::{SlotTable, TradeQuery, TradeStat};
use crate::colors::color_code;
use crate::control::{ControlRef, Prop, anchor, anchor_collapse, anchor_nil, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::popup::PopupDialog;
use crate::launch::launch;
use crate::lua::{gsub, lmatch};
use crate::lua_data::{LuaKey, LuaVal, parse_return};
use crate::modules::main::{BareControl, main};
use crate::stat_describer::describe_stats;
use crate::tabs::items::{ItemSlotControl, ItemsTab};

fn cc(name: &str) -> String {
    color_code(name).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------- Data/QueryMods.lua

/// One `modData[type][uniqueIndex]`.
#[derive(Debug, Default)]
pub struct ModEntry {
    pub key: String,
    pub trade_id: String,
    pub trade_text: String,
    pub sign: Option<String>,
    pub override_line: Option<String>,
    pub override_singular: Option<String>,
    /// `entry[category]`.
    pub ranges: HashMap<String, (f64, f64)>,
}

/// `self.modData`: by mod type, the entries in the order of the file.
pub type ModData = HashMap<String, Vec<Rc<ModEntry>>>;

thread_local! {
    static MOD_DATA: RefCell<Option<Rc<ModData>>> = const { RefCell::new(None) };
    /// R13: `pairs(self.modData[type])` of a process of the original, for the gate. An entry not named keeps its place after the named ones.
    static MOD_ORDER_SAMPLE: RefCell<Option<HashMap<String, Vec<String>>>> = const { RefCell::new(None) };
    /// R13: the keys of `pairs(entry)`-free lists: `pairs(self.calcContext.special.queryExtra)` has no effect, so only the mods have a sample.
    static PEARL_COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}

/// The gate's order of `pairs(self.modData[type])`.
pub fn set_mod_order_sample(sample: Option<HashMap<String, Vec<String>>>) {
    MOD_ORDER_SAMPLE.with(|s| *s.borrow_mut() = sample);
}

fn entry_of(key: &str, t: &crate::lua_data::LuaTab) -> ModEntry {
    let mut e = ModEntry {
        key: key.to_string(),
        ..ModEntry::default()
    };
    for (k, v) in &t.fields {
        let LuaKey::Str(k) = k else { continue };
        match &**k {
            "tradeMod" => {
                let m = v.as_tab().expect("tradeMod");
                e.trade_id = m.str("id").unwrap_or("").to_string();
                e.trade_text = m.str("text").unwrap_or("").to_string();
            }
            "specialCaseData" => {
                let m = v.as_tab().expect("specialCaseData");
                e.override_line = m.str("overrideModLine").map(str::to_string);
                e.override_singular = m.str("overrideModLineSingular").map(str::to_string);
            }
            "sign" => e.sign = v.as_str().map(str::to_string),
            "inverseKey" => {}
            cat => {
                if let Some(r) = v.as_tab() {
                    e.ranges
                        .insert(cat.to_string(), (r.num("min").unwrap_or(0.0), r.num("max").unwrap_or(0.0)));
                }
            }
        }
    }
    e
}

/// `InitMods` (:368-377).
pub fn mod_data() -> Rc<ModData> {
    if let Some(m) = MOD_DATA.with(|m| m.borrow().clone()) {
        return m;
    }
    let root = launch().cfg.script_path.clone();
    let path = format!("{root}Data/QueryMods.lua");
    let src = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("Classes/TradeQueryGenerator.lua:379-560 (regenerating QueryMods.lua is not ported): {path}: {e}"));
    let v = parse_return(&src).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut out: ModData = HashMap::new();
    for (k, t) in &v.as_tab().expect("modData").fields {
        let (LuaKey::Str(k), LuaVal::Table(t)) = (k, t) else { continue };
        let mut list = Vec::with_capacity(t.fields.len());
        for (mk, mv) in &t.fields {
            if let (LuaKey::Str(mk), LuaVal::Table(mv)) = (mk, mv) {
                list.push(Rc::new(entry_of(mk, mv)));
            }
        }
        out.insert(k.to_string(), list);
    }
    let m = Rc::new(out);
    MOD_DATA.with(|c| *c.borrow_mut() = Some(m.clone()));
    m
}

/// `pairs(self.modData[modType])`.
pub fn mods_in_pairs_order(data: &ModData, ty: &str) -> Vec<Rc<ModEntry>> {
    let mut list = data.get(ty).cloned().unwrap_or_default();
    sort_by_sample(&mut list, ty);
    list
}

fn sort_by_sample(list: &mut [Rc<ModEntry>], key: &str) {
    MOD_ORDER_SAMPLE.with(|s| {
        if let Some(order) = s.borrow().as_ref().and_then(|o| o.get(key)) {
            let at: HashMap<&str, usize> = order.iter().enumerate().map(|(i, k)| (k.as_str(), i)).collect();
            list.sort_by_key(|e| at.get(e.key.as_str()).copied().unwrap_or(usize::MAX));
        }
    });
}

fn pearl_support_count() -> usize {
    if let Some(n) = PEARL_COUNT.with(Cell::get) {
        return n;
    }
    let root = launch().cfg.script_path.clone();
    let path = format!("{root}Data/PearlSupports.lua");
    let src = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let v = parse_return(&src).unwrap_or_else(|e| panic!("{path}: {e}"));
    let n = v.as_tab().map_or(0, |t| t.len_seq());
    PEARL_COUNT.with(|c| c.set(Some(n)));
    n
}

/// `getStatEntries("pseudo")` (:109-133).
fn pseudo_entries() -> Vec<super::helpers::StatEntry> {
    trade_stats()
        .iter()
        .find(|c| c.id == "pseudo")
        .map(|c| c.entries.clone())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------- WeightedRatioOutputs

/// `output` or a `ReduceOutput` table.
#[derive(Clone, Copy)]
pub enum Out<'a> {
    Full(&'a Output),
    Small(&'a HashMap<String, f64>),
}

/// `data.powerStatList.GetFromOutput(output, statTable, true)` (Data.lua:186-212): the reduced table has no `Minion`, so a `Minion*` stat of it is 0.
pub fn get_stat(o: Out<'_>, stat: &str) -> f64 {
    match o {
        Out::Full(o) => pob_calc::power::get_from_output(
            o,
            &pob_calc::power::PowerStat {
                stat: Some(stat.to_string()),
                label: String::new(),
                ignore_for_nodes: false,
                transform: None,
            },
        ),
        Out::Small(m) => {
            if stat == "FullDPS" {
                if let Some(v) = m.get(stat) {
                    return *v;
                }
                return m.get("CombinedDPS").copied().unwrap_or(0.0);
            }
            if stat.starts_with("Minion") && stat.len() > "Minion".len() {
                return 0.0;
            }
            m.get(stat).copied().unwrap_or(0.0)
        }
    }
}

fn has_full_dps(o: Out<'_>) -> bool {
    match o {
        Out::Full(o) => o.get("FullDPS").is_some(),
        Out::Small(m) => m.contains_key("FullDPS"),
    }
}

/// `data.misc.maxStatIncrease` (Data.lua:297).
const MAX_STAT_INCREASE: f64 = 2.0;

/// `TradeQueryGeneratorClass.WeightedRatioOutputs(baseOutput, newOutput, statWeights)` (:164-199).
pub fn weighted_ratio_outputs(base: Out<'_>, new: Out<'_>, weights: &[TradeStat]) -> f64 {
    let ratio = |mods: &[&str]| -> f64 {
        let mut base_sum = 0.0;
        let mut new_sum = 0.0;
        for m in mods {
            base_sum += get_stat(base, m);
            new_sum += get_stat(new, m);
        }
        if base_sum == f64::INFINITY {
            return 0.0;
        }
        if new_sum == f64::INFINITY {
            return MAX_STAT_INCREASE;
        }
        let r = new_sum / if base_sum != 0.0 { base_sum } else { 1.0 };
        // `math.min(r, 2)`: NaN stays
        if MAX_STAT_INCREASE < r { MAX_STAT_INCREASE } else { r }
    };
    let mut mean = 0.0;
    for s in weights {
        let mut r = if s.stat == "FullDPS" && !(has_full_dps(base) && has_full_dps(new)) {
            ratio(&["TotalDPS", "TotalDotDPS", "CombinedDPS"])
        } else {
            ratio(&[s.stat.as_str()])
        };
        if s.negate {
            r = -r;
        }
        mean += r * s.weight_mult.unwrap_or_else(|| {
            panic!("Classes/TradeQueryGenerator.lua:196: attempt to perform arithmetic on field 'weightMult' (a nil value)")
        });
    }
    mean
}

/// `{ tradeModId, weight, meanStatDiff, invert }`.
#[derive(Clone, Debug, PartialEq)]
pub struct ModWeight {
    pub trade_mod_id: String,
    pub weight: f64,
    pub mean_stat_diff: f64,
    pub invert: bool,
}

/// `currencyTable` (:659-677).
const CURRENCY_TABLE: [(&str, Option<&str>); 17] = [
    ("Chaos Orb Equivalent", None),
    ("Chaos Orb", Some("chaos")),
    ("Divine Orb", Some("divine")),
    ("Orb of Alchemy", Some("alch")),
    ("Orb of Alteration", Some("alt")),
    ("Chromatic Orb", Some("chrome")),
    ("Exalted Orb", Some("exalted")),
    ("Blessed Orb", Some("blessed")),
    ("Cartographer's Chisel", Some("chisel")),
    ("Gemcutter's Prism", Some("gcp")),
    ("Jeweller's Orb", Some("jewellers")),
    ("Orb of Scouring", Some("scour")),
    ("Orb of Regret", Some("regret")),
    ("Orb of Fusing", Some("fusing")),
    ("Orb of Chance", Some("chance")),
    ("Regal Orb", Some("regal")),
    ("Vaal Orb", Some("vaal")),
];

/// `options` of `RequestQuery` / `StartQuery`.
#[derive(Clone, Default)]
pub struct QueryOptions {
    pub special_item: Option<String>,
    pub include_mirrored: Option<bool>,
    pub include_corrupted: Option<bool>,
    pub include_eldritch: Option<String>,
    pub include_scourge: Option<bool>,
    pub include_talisman: Option<bool>,
    pub include_all_we_mods: Option<bool>,
    pub jewel_type: Option<String>,
    pub max_price: Option<f64>,
    pub max_price_type: Option<&'static str>,
    pub max_level: Option<f64>,
    pub sockets: Option<f64>,
    pub links: Option<f64>,
    pub account: Option<String>,
    pub required_mods: Vec<ModSel>,
    pub blocked_mods: Vec<ModSel>,
    pub stat_weights: Vec<TradeStat>,
}

/// One row of a mod selector (`copyTable(val)`, with `value` from its box).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModSel {
    pub label: String,
    pub trade_id: Option<String>,
    pub value: Option<f64>,
}

/// What the walk has to do: a list of `GenerateModWeights`, or `GeneratePassiveNodeWeights`.
enum Phase {
    Mods(Vec<Rc<ModEntry>>),
    Nodes(Vec<Rc<ModEntry>>),
}

#[derive(Default)]
struct Special {
    query_filters: Option<Json>,
    query_extra: Vec<(&'static str, &'static str)>,
    calc_nodes: bool,
    watchers_eye: bool,
    pearl: bool,
}

struct CalcContext {
    item_category_query_str: Option<String>,
    item_category: String,
    special: Special,
    test_item: RefCell<Item>,
    base_stat_value: f64,
    options: QueryOptions,
    slot: Option<Rc<ItemSlotControl>>,
    phases: RefCell<Vec<Phase>>,
    phase_at: Cell<usize>,
    entry_at: Cell<usize>,
}

pub type RequesterCb = Rc<dyn Fn(&mut Gfx, Option<Json>, Option<String>)>;

/// `self.lastX` and the running query.
pub struct TradeQueryGenerator {
    query_tab: Weak<TradeQuery>,
    items_tab: Weak<ItemsTab>,
    calc: RefCell<Option<Rc<CalcContext>>>,
    pub last_max_price: Cell<Option<f64>>,
    pub last_max_price_type_index: Cell<Option<usize>>,
    pub last_max_level: Cell<Option<f64>>,
    pub last_sockets: Cell<Option<f64>>,
    pub last_include_corrupted: Cell<Option<bool>>,
    pub last_include_mirrored: Cell<Option<bool>>,
    pub last_include_scourge: Cell<Option<bool>>,
    pub last_include_talisman: Cell<Option<bool>>,
    pub last_include_eldritch: RefCell<Option<String>>,
    pub last_copy_enchant_mode: RefCell<Option<String>>,
    pub last_jewel_type: Cell<Option<usize>>,
    trade_type_index: Cell<usize>,
    /// `self.modWeights`, as it is when the query was made.
    pub mod_weights: RefCell<Vec<ModWeight>>,
    already_weighted: RefCell<HashSet<String>>,
    requester: RefCell<Option<RequesterCb>>,
}

fn tab_of(t: &Weak<ItemsTab>) -> Rc<ItemsTab> {
    t.upgrade().expect("itemsTab")
}

impl TradeQueryGenerator {
    /// `TradeQueryGenerator(queryTab)` (:152-162).
    pub fn new(query_tab: &Rc<TradeQuery>) -> Rc<Self> {
        let _ = mod_data();
        Rc::new(Self {
            query_tab: Rc::downgrade(query_tab),
            items_tab: query_tab.items_tab.clone(),
            calc: RefCell::new(None),
            last_max_price: Cell::new(None),
            last_max_price_type_index: Cell::new(None),
            last_max_level: Cell::new(None),
            last_sockets: Cell::new(None),
            last_include_corrupted: Cell::new(None),
            last_include_mirrored: Cell::new(None),
            last_include_scourge: Cell::new(None),
            last_include_talisman: Cell::new(None),
            last_include_eldritch: RefCell::new(None),
            last_copy_enchant_mode: RefCell::new(None),
            last_jewel_type: Cell::new(None),
            trade_type_index: Cell::new(1),
            mod_weights: RefCell::new(Vec::new()),
            already_weighted: RefCell::new(HashSet::new()),
            requester: RefCell::new(None),
        })
    }

    fn run_with_item(&self, e: &Engine, slot: &str, item: &Item) -> Result<pob_calc::session::RunRef, String> {
        e.item_override_run(CalcOverride {
            rep_slot_name: Some(slot.to_string()),
            rep_item: Some(Rc::new(item.clone())),
            ..CalcOverride::default()
        })
    }

    /// `meanStatDiff` of a calculator's output.
    fn mean_stat_diff(&self, cx: &CalcContext, e: &Engine, run: &pob_calc::session::RunRef) -> Result<f64, String> {
        let base = e.misc_base_run()?;
        let (b, n) = (base.borrow(), run.borrow());
        Ok(weighted_ratio_outputs(
            Out::Full(&b.state.player.output),
            Out::Full(&n.state.player.output),
            &cx.options.stat_weights,
        ) * 1000.0
            - cx.base_stat_value)
    }

    /// One entry of `GenerateModWeights` (:565-609).
    fn weigh_mod(&self, cx: &CalcContext, e: &Engine, entry: &ModEntry) -> Result<bool, String> {
        let Some(&(min, max)) = entry.ranges.get(&cx.item_category) else {
            return Ok(false);
        };
        if self.already_weighted.borrow().contains(&entry.trade_id) {
            return Ok(false);
        }
        // :569: `entry[category].subType` is in no line of QueryMods.lua: `includeTalisman == false` skips nothing
        let quality = main().s.borrow().default_item_affix_quality;
        let mod_value = ((max - min) * quality + min).ceil();
        let value_str = format!(
            "{}{}",
            entry.sign.as_deref().unwrap_or(""),
            pob_lua::format::tostring_num(mod_value)
        );
        let line = if mod_value == 1.0 && entry.override_singular.is_some() {
            entry.override_singular.clone().expect("checked")
        } else if let Some(l) = &entry.override_line {
            l.clone()
        } else {
            entry.trade_text.clone()
        };
        let line = line.replace('#', &value_str);
        let slot_name = cx.slot.as_ref().expect("`self.calcContext.slot.slotName` (:595)").slot_name.clone();
        let mut item = cx.test_item.borrow_mut();
        let mut ml = ModLine {
            line,
            ..ModLine::default()
        };
        ml.set_flag_pub("custom");
        if item.explicit_mod_lines.is_empty() {
            item.explicit_mod_lines.push(ml);
        } else {
            item.explicit_mod_lines[0] = ml;
        }
        item.build_and_parse_raw();
        let run = self.run_with_item(e, &slot_name, &item)?;
        let diff = self.mean_stat_diff(cx, e, &run)?;
        if diff > 0.01 {
            self.mod_weights.borrow_mut().push(ModWeight {
                trade_mod_id: entry.trade_id.clone(),
                weight: diff / mod_value,
                mean_stat_diff: diff,
                invert: entry.sign.as_deref() == Some("-"),
            });
        }
        self.already_weighted.borrow_mut().insert(entry.trade_id.clone());
        Ok(true)
    }

    /// One entry of `GeneratePassiveNodeWeights` (:615-641).
    fn weigh_node(&self, cx: &CalcContext, e: &Engine, entry: &ModEntry) -> Result<bool, String> {
        if self.already_weighted.borrow().contains(&entry.trade_id) {
            return Ok(false);
        }
        let Some(name) = lmatch(&entry.trade_text, "1 Added Passive Skill is (.*)").or_else(|| lmatch(&entry.trade_text, "Allocates (.*)"))
        else {
            return Ok(false);
        };
        let spec = e.spec_rc();
        let ti = spec
            .tree
            .cluster_node_map
            .get(&name)
            .or_else(|| spec.tree.notable_map.get(&name))
            .copied()
            .ok_or_else(|| "Classes/TradeQueryGenerator.lua:627: table index is nil".to_string())?;
        let run = e.item_override_run(CalcOverride {
            add_nodes: Some(vec![OverrideNode::Other(Rc::new(spec.tree_node_view(ti)))]),
            ..CalcOverride::default()
        })?;
        let diff = self.mean_stat_diff(cx, e, &run)?;
        if diff > 0.01 {
            self.mod_weights.borrow_mut().push(ModWeight {
                trade_mod_id: entry.trade_id.clone(),
                weight: diff,
                mean_stat_diff: diff,
                invert: false,
            });
        }
        self.already_weighted.borrow_mut().insert(entry.trade_id.clone());
        Ok(true)
    }

    /// `:OnFrame()` (:644-657).
    pub fn on_frame(&self, g: &mut Gfx) {
        let Some(cx) = self.calc.borrow().clone() else { return };
        let tab = tab_of(&self.items_tab);
        let start = g.get_time() as f64;
        let time = |g: &Gfx| g.get_time() as f64;
        let res = tab.read(|e| -> Result<bool, String> {
            let phases = cx.phases.borrow();
            while cx.phase_at.get() < phases.len() {
                let (list, nodes) = match &phases[cx.phase_at.get()] {
                    Phase::Mods(l) => (l, false),
                    Phase::Nodes(l) => (l, true),
                };
                while cx.entry_at.get() < list.len() {
                    let entry = list[cx.entry_at.get()].clone();
                    cx.entry_at.set(cx.entry_at.get() + 1);
                    let worked = if nodes {
                        self.weigh_node(&cx, e, &entry)?
                    } else {
                        self.weigh_mod(&cx, e, &entry)?
                    };
                    if worked && time(g) - start > 50.0 {
                        return Ok(false);
                    }
                }
                cx.phase_at.set(cx.phase_at.get() + 1);
                cx.entry_at.set(0);
            }
            Ok(true)
        });
        match res {
            Ok(false) => {}
            Ok(true) => {
                *self.calc.borrow_mut() = None;
                self.finish_query(g, &cx);
            }
            // `coroutine.resume` keeps the error; a dead coroutine goes on to FinishQuery (:650-656)
            Err(err) => {
                if launch().dev_mode.get() {
                    panic!("{err}");
                }
                g.con_printf(&format!("TradeQueryGenerator: {err}"));
                *self.calc.borrow_mut() = None;
                self.finish_query(g, &cx);
            }
        }
    }

    /// `:StartQuery(slot, options)` (:679-797).
    pub fn start_query(self: &Rc<Self>, g: &mut Gfx, slot: Option<Rc<ItemSlotControl>>, mut options: QueryOptions) {
        if let Some(p) = self.last_max_price.get() {
            options.max_price = Some(p);
        }
        if let Some(i) = self.last_max_price_type_index.get() {
            options.max_price_type = CURRENCY_TABLE[i - 1].1;
        }
        if let Some(l) = self.last_max_level.get() {
            options.max_level = Some(l);
        }
        let tab = tab_of(&self.items_tab);
        let existing = slot.as_ref().and_then(|s| tab.read(|e| e.item_by_id(s.sel_item_id.get())));
        let mut test_item_type = existing
            .as_ref()
            .and_then(|i| i.base_name.clone())
            .unwrap_or_else(|| "Unset Amulet".to_string());
        let mut query_str: Option<String> = None;
        let mut category: Option<String> = None;
        let mut special = Special::default();
        if let Some(name) = &options.special_item {
            match name.as_str() {
                "Megalomaniac" => {
                    special = Special {
                        query_filters: Some(json!([])),
                        query_extra: vec![("name", "Megalomaniac"), ("type", "Medium Cluster Jewel")],
                        calc_nodes: true,
                        ..Special::default()
                    };
                }
                "Watcher's Eye" => {
                    special = Special {
                        query_extra: vec![("name", "Watcher's Eye")],
                        query_filters: Some(
                            json!({"type_filters": {"filters": {"category": {"option": "jewel"}, "rarity": {"option": "unique"}}}}),
                        ),
                        watchers_eye: true,
                        ..Special::default()
                    };
                    category = Some("AnyJewel".to_string());
                    query_str = Some("jewel".to_string());
                }
                "Pearl of Tsoatha" => {
                    category = Some("Ring".to_string());
                    query_str = Some("ring".to_string());
                    test_item_type = "Prismatic Ring".to_string();
                    special = Special {
                        query_filters: Some(json!([])),
                        query_extra: vec![("name", "Pearl of Tsoatha")],
                        pearl: true,
                        ..Special::default()
                    };
                }
                _ => {}
            }
        } else {
            let s = slot.as_ref().expect("`slot.slotName` (:742)");
            let (q, c) = get_trade_category(&s.slot_name, existing.as_deref());
            query_str = q.map(str::to_string);
            category = c.map(str::to_string);
            if s.slot_name.contains("Jewel") && !s.slot_name.contains("Abyssal") {
                let ty = options.jewel_type.clone().expect("`options.jewelType` (:746)");
                let cat = format!("{ty}Jewel");
                query_str = Some(
                    match cat.as_str() {
                        "AbyssJewel" => "jewel.abyss",
                        "BaseJewel" => "jewel.base",
                        _ => "jewel",
                    }
                    .to_string(),
                );
                category = Some(cat);
            }
            if query_str.is_none() {
                g.con_printf(&format!(
                    "'{}' is not supported for weighted trade query generation",
                    existing.as_ref().and_then(|i| i.ty.clone()).unwrap_or_else(|| "n/a".to_string())
                ));
                return;
            }
        }
        let test_item = Item::parse_with(&format!("Rarity: RARE\nStat Tester\n{test_item_type}"), None, false);
        // :767-770
        let compute = tab.read(|e| -> Result<f64, String> {
            let base = e.misc_base_run()?;
            let run = match &slot {
                Some(s) => self.run_with_item(e, &s.slot_name, &test_item)?,
                None => base.clone(),
            };
            let (b, n) = (base.borrow(), run.borrow());
            Ok(weighted_ratio_outputs(
                Out::Full(&b.state.player.output),
                Out::Full(&n.state.player.output),
                &options.stat_weights,
            ) * 1000.0)
        });
        let base_stat_value = compute.unwrap_or_else(|e| panic!("{e}"));
        self.mod_weights.borrow_mut().clear();
        self.already_weighted.borrow_mut().clear();
        let phases = self.phases_of(&special, &options, category.as_deref().unwrap_or(""));
        let cx = Rc::new(CalcContext {
            item_category_query_str: query_str,
            item_category: category.unwrap_or_default(),
            special,
            test_item: RefCell::new(test_item),
            base_stat_value,
            options,
            slot,
            phases: RefCell::new(phases),
            phase_at: Cell::new(0),
            entry_at: Cell::new(0),
        });
        *self.calc.borrow_mut() = Some(cx);
        // :794-796
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert(
            "progressText".to_string(),
            LabelControl::new(
                anchor_nil("TOP", "TOP"),
                rect(0.0, 30.0, 0.0, 16.0),
                "Calculating Mod Weights...".to_string(),
            ),
        );
        main().open_popup(g, 280.0, 65.0, "Please Wait", controls, None, None, None, None);
    }

    /// `:ExecuteQuery()` (:799-878) as a list of lists.
    fn phases_of(&self, special: &Special, options: &QueryOptions, _category: &str) -> Vec<Phase> {
        let data = mod_data();
        let mods = |ty: &str| mods_in_pairs_order(&data, ty);
        if special.calc_nodes {
            return vec![Phase::Nodes(mods("PassiveNode"))];
        }
        if special.watchers_eye {
            let mut out = vec![Phase::Mods(mods("WatchersEye"))];
            if options.include_corrupted == Some(true) {
                out.push(Phase::Mods(mods("Corrupted")));
            }
            return out;
        }
        if special.pearl {
            let mut list = Vec::new();
            let count = pearl_support_count();
            for slot in 1..=4 {
                for support in 1..=count {
                    let stats = vec![
                        ("local_pearl_random_support_gem_2_level".to_string(), 20.0),
                        ("local_pearl_random_support_gem_2_slot_index".to_string(), f64::from(slot)),
                        ("local_pearl_random_support_gem_2_index".to_string(), support as f64),
                    ];
                    let text = describe_stats(&stats, "stat_descriptions", false).0.join(" ");
                    let mut ranges = HashMap::new();
                    ranges.insert("Ring".to_string(), (1.0, 1.0));
                    list.push(Rc::new(ModEntry {
                        key: String::new(),
                        trade_id: format!("explicit.stat_4089743927|{slot}|{support}"),
                        trade_text: text,
                        sign: Some(String::new()),
                        override_line: None,
                        override_singular: None,
                        ranges,
                    }));
                }
            }
            return vec![Phase::Mods(list)];
        }
        let mut out = vec![Phase::Mods(mods("Explicit")), Phase::Mods(mods("Implicit"))];
        if options.include_corrupted == Some(true) {
            out.push(Phase::Mods(mods("Corrupted")));
        }
        if options.include_talisman == Some(true) {
            out.push(Phase::Mods(mods("Enchant")));
        }
        if options.include_scourge == Some(true) {
            out.push(Phase::Mods(mods("Scourge")));
        }
        if let Some(opt) = options.include_eldritch.as_deref().filter(|o| o.starts_with("Keep")) {
            let omit = opt == "Keep regular";
            for ty in ["Eater", "Exarch"] {
                let mut l = mods(ty);
                if omit {
                    // `filterMods`: `pairs` of a new table (R13): a sample of its own
                    l.retain(|m| !(lmatch(&m.key, ".*PinnaclePresence$").is_some() || lmatch(&m.key, ".*UniquePresence$").is_some()));
                    sort_by_sample(&mut l, &format!("{ty}:filtered"));
                }
                out.push(Phase::Mods(l));
            }
        }
        out
    }

    /// `:addMoreWEMods()` (:880-908).
    fn add_more_we_mods(&self) {
        let tab = tab_of(&self.items_tab);
        let data = mod_data();
        let we = mods_in_pairs_order(&data, "WatchersEye");
        let gems = tab.read(|e| e.trade_aura_gems());
        for spec in gems {
            let aura = if spec.vaal_global2 {
                spec.name_spec
                    .replace("Vaal ", "")
                    .replace("Impurity", "Purity")
                    .replace("of", "Of")
                    .replace(' ', "")
            } else if spec.is_aura_or_from_item {
                spec.name_spec.replace("of", "Of").replace(' ', "")
            } else {
                continue;
            };
            for m in &we {
                if m.key.contains(&aura) && !self.mod_weights.borrow().iter().any(|w| w.trade_mod_id == m.trade_id) {
                    self.mod_weights.borrow_mut().push(ModWeight {
                        trade_mod_id: m.trade_id.clone(),
                        weight: 0.0,
                        mean_stat_diff: 0.0,
                        invert: false,
                    });
                }
            }
        }
    }

    /// `:FinishQuery()` (:910-1162).
    fn finish_query(&self, g: &mut Gfx, cx: &CalcContext) {
        let tab = tab_of(&self.items_tab);
        let original = cx.slot.as_ref().and_then(|s| tab.read(|e| e.item_by_id(s.sel_item_id.get())));
        {
            let mut item = cx.test_item.borrow_mut();
            item.explicit_mod_lines.clear();
            if let Some(o) = &original {
                for l in o
                    .explicit_mod_lines
                    .iter()
                    .chain(&o.scourge_mod_lines)
                    .chain(&o.implicit_mod_lines)
                    .chain(&o.crucible_mod_lines)
                {
                    item.explicit_mod_lines.push(l.clone());
                }
            }
            item.build_and_parse_raw();
        }
        let current_stat_diff = tab
            .read(|e| -> Result<f64, String> {
                let base = e.misc_base_run()?;
                let run = match (&original, &cx.slot) {
                    (Some(_), Some(s)) => self.run_with_item(e, &s.slot_name, &cx.test_item.borrow())?,
                    _ => base.clone(),
                };
                let (b, n) = (base.borrow(), run.borrow());
                Ok(weighted_ratio_outputs(
                    Out::Full(&b.state.player.output),
                    Out::Full(&n.state.player.output),
                    &cx.options.stat_weights,
                ) * 1000.0
                    - cx.base_stat_value)
            })
            .unwrap_or_else(|e| panic!("{e}"));
        if cx.options.include_all_we_mods == Some(true) {
            self.add_more_we_mods();
        }
        {
            let mut w = self.mod_weights.borrow_mut();
            pob_engine::lua::lua_sort(&mut w, &mut |a: &ModWeight, b: &ModWeight| {
                if a.mean_stat_diff == b.mean_stat_diff {
                    a.weight.abs() > b.weight.abs()
                } else {
                    a.mean_stat_diff > b.mean_stat_diff
                }
            });
        }
        // :946 `special.itemName` is set by no branch of `StartQuery`: the Megalomaniac's minimum is the plain one too
        let min_weight = current_stat_diff * 0.5;
        const TRADE_TYPES: [&str; 5] = ["securable", "available", "onlineleague", "online", "any"];
        let selected = TRADE_TYPES[self.trade_type_index.get() - 1];
        let mut filters = cx.special.query_filters.clone().unwrap_or_else(|| {
            json!({"type_filters": {"filters": {"category": {"option": cx.item_category_query_str}, "rarity": {"option": "nonunique"}}}})
        });
        let mut budget: i64 = 200 - 54 - 4 - 4;
        // :995-1003
        let pseudo_map = |h: &str| -> Option<&'static str> {
            Some(match h {
                "3372524247" => "pseudo.pseudo_total_fire_resistance",
                "4220027924" => "pseudo.pseudo_total_cold_resistance",
                "1671376347" => "pseudo.pseudo_total_lightning_resistance",
                "2923486259" => "pseudo.pseudo_total_chaos_resistance",
                "4080418644" => "pseudo.pseudo_total_strength",
                "3261801346" => "pseudo.pseudo_total_dexterity",
                "328541901" => "pseudo.pseudo_total_intelligence",
                _ => return None,
            })
        };
        let mut ignored: HashSet<String> = ["2901986750", "1379411836", "2897413282"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        for a in ["fire", "cold", "lightning", "chaos"] {
            for b in ["fire", "cold", "lightning", "chaos"] {
                ignored.insert(hash_stats(&[format!("{a}_and_{b}_damage_resistance_%")], None).to_string());
            }
        }
        for a in ["dexterity", "strength", "intelligence"] {
            for b in ["dexterity", "strength", "intelligence"] {
                ignored.insert(hash_stats(&[format!("base_{a}_and_{b}")], None).to_string());
                ignored.insert(hash_stats(&[format!("additional_{a}_and_{b}")], None).to_string());
            }
        }
        // (id, weight): a pseudo filter is updated in place
        let mut stat_filters: Vec<(String, f64)> = Vec::new();
        for entry in self.mod_weights.borrow().iter() {
            let hash = lmatch(&entry.trade_mod_id, "stat_(%d+)");
            let weight = if entry.invert { -entry.weight } else { entry.weight };
            if hash.as_ref().is_some_and(|h| ignored.contains(h)) {
                continue;
            }
            if let Some(trade) = hash.as_deref().and_then(pseudo_map) {
                if let Some(x) = stat_filters.iter_mut().find(|x| x.0 == trade) {
                    x.1 = if x.1 > weight { x.1 } else { weight };
                } else {
                    stat_filters.push((trade.to_string(), weight));
                }
            } else {
                stat_filters.push((entry.trade_mod_id.clone(), weight));
            }
        }
        let mut query = serde_json::Map::new();
        for (k, v) in &cx.special.query_extra {
            budget -= 2;
            query.insert((*k).to_string(), json!(v));
        }
        let mut and_filters = Vec::new();
        for e in &cx.options.required_mods {
            budget -= 4;
            and_filters.push(mod_filter(e));
        }
        let mut not_filters = Vec::new();
        for e in &cx.options.blocked_mods {
            budget -= 4;
            not_filters.push(mod_filter(e));
        }
        let set = |filters: &mut Json, k: &str, v: Json| {
            if !filters.is_object() {
                *filters = json!({});
            }
            filters[k] = v;
        };
        let o = &cx.options;
        if o.include_mirrored != Some(true) {
            budget -= 3;
            set(
                &mut filters,
                "misc_filters",
                json!({"disabled": false, "filters": {"mirrored": false}}),
            );
        }
        if let Some(p) = o.max_price.filter(|p| *p > 0.0) {
            budget -= 3;
            let mut price = json!({"max": dkjson::n(p)});
            if let Some(t) = o.max_price_type {
                price["option"] = json!(t);
            }
            set(&mut filters, "trade_filters", json!({"filters": {"price": price}}));
        }
        if let Some(a) = &o.account {
            budget -= 3;
            if filters["trade_filters"].is_null() {
                set(&mut filters, "trade_filters", json!({"filters": {}}));
            }
            filters["trade_filters"]["filters"]["account"] = json!({"input": a});
        }
        if let Some(l) = o.max_level.filter(|l| *l > 0.0) {
            budget -= 3;
            set(
                &mut filters,
                "req_filters",
                json!({"disabled": false, "filters": {"lvl": {"max": dkjson::n(l)}}}),
            );
        }
        if let Some(s) = o.sockets.filter(|s| *s > 0.0) {
            budget -= 3;
            set(
                &mut filters,
                "socket_filters",
                json!({"disabled": false, "filters": {"sockets": {"max": dkjson::n(s), "min": dkjson::n(s)}}}),
            );
        }
        if let Some(l) = o.links.filter(|l| *l > 0.0) {
            budget -= 3;
            let links = json!({"max": dkjson::n(l), "min": dkjson::n(l)});
            if filters["socket_filters"].is_null() {
                set(
                    &mut filters,
                    "socket_filters",
                    json!({"disabled": false, "filters": {"links": links}}),
                );
            } else {
                filters["socket_filters"]["filters"]["links"] = links;
            }
        }
        let mut weight_filters = Vec::new();
        for (id, weight) in stat_filters {
            if budget < 8 {
                break;
            }
            budget -= 4;
            weight_filters.push(json!({"id": id, "value": {"weight": dkjson::n(weight)}}));
        }
        g.con_printf(&format!("filters: {}, budget: {budget}", weight_filters.len()));
        let err = weight_filters
            .is_empty()
            .then(|| "Could not generate search, found no mods to search for".to_string());
        query.insert("filters".to_string(), filters);
        query.insert("status".to_string(), json!({"option": selected}));
        query.insert(
            "stats".to_string(),
            json!([
                {"type": "weight", "value": {"min": dkjson::n(min_weight)}, "filters": weight_filters},
                {"type": "and", "filters": and_filters},
                {"type": "not", "filters": not_filters},
            ]),
        );
        let requester = self.requester.borrow().clone();
        if let Some(r) = requester {
            r(g, Some(Json::Object(query)), err);
        }
        main().close_popup();
    }

    /// `:RequestQuery(slot, context, statWeights, callback)` (:1164-1605).
    pub fn request_query(
        self: &Rc<Self>,
        g: &mut Gfx,
        slot: Option<Rc<ItemSlotControl>>,
        slot_tbl: Rc<SlotTable>,
        stat_weights: Vec<TradeStat>,
        callback: RequesterCb,
    ) {
        *self.requester.borrow_mut() = Some(callback);
        let tq = self.query_tab.upgrade().expect("queryTab");
        let tab = tab_of(&self.items_tab);
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        let mut popup_height = 110.0;
        let popup_width = 480.0;
        let slot_name = slot.as_ref().map(|s| s.slot_name.clone());
        let name = slot_name.as_deref();
        let is_jewel = name.is_some_and(|n| n.contains("Jewel"));
        let is_abyssal = name.is_some_and(|n| n.contains("Abyssal"));
        let is_amulet = name == Some("Amulet");
        let is_belt = name == Some("Belt");
        let is_weapon = name == Some("Weapon 1") || name == Some("Weapon 2");
        let is_eldritch = matches!(name, Some("Body Armour" | "Helmet" | "Gloves" | "Boots" | "Amulet"));
        let unique = slot_tbl.unique;
        let row_name = slot_tbl.slot_name.clone();

        let include_corrupted = CheckBoxControl::new(
            g,
            anchor_nil("TOP", "TOP"),
            [-40.0, 30.0, 18.0],
            Some("Corrupted Mods:"),
            None,
            Some(
                "Includes corruption implicit modifiers in the weighted sum.\nNote that there is a maximum search filter count which means this might cause other weights to not be included.",
            ),
            false,
        );
        include_corrupted
            .state
            .set(!slot_tbl.already_corrupted && self.last_include_corrupted.get().is_none_or(|v| v));
        include_corrupted.base.enabled.set(!slot_tbl.already_corrupted);
        let mut last: ControlRef = include_corrupted.clone();
        controls.insert("includeCorrupted".into(), include_corrupted.clone());
        popup_height += 23.0;

        let pb_league = tq.pb_league.borrow().clone();
        let include_scourge_opt = pb_league.as_deref() == Some("Standard") || pb_league.as_deref() == Some("Hardcore");
        let mut options = QueryOptions::default();
        if unique {
            options.special_item = Some(row_name.clone());
        }
        // :1196-1215
        let mut jewel_slot: Option<Rc<DropDownControl>> = None;
        if row_name == "Watcher's Eye" {
            let mut sockets: Vec<Rc<ItemSlotControl>> = tab
                .sockets
                .borrow()
                .iter()
                // `spec.nodes[nodeId].containJewelSocket` is read in three places upstream and written in none
                .filter(|(_, s)| !s.inactive.get())
                .map(|(_, s)| s.clone())
                .collect();
            sockets.sort_by(|a, b| a.label.borrow().cmp(&b.label.borrow()));
            let list: Vec<DropItem> = sockets
                .iter()
                .map(|s| {
                    let mut d = DropItem::table(&s.label.borrow());
                    d.data = Some(s.clone());
                    d
                })
                .collect();
            let d = DropDownControl::new(
                g,
                anchor("TOPLEFT", &last, "BOTTOMLEFT"),
                rect(0.0, 5.0, 100.0, 18.0),
                list,
                None,
                None,
                false,
            );
            let r: ControlRef = d.clone();
            controls.insert(
                "jewelSlotLabel".into(),
                LabelControl::new(anchor("RIGHT", &r, "LEFT"), rect(-5.0, 0.0, 0.0, 16.0), "Jewel Slot:".to_string()),
            );
            for (i, s) in sockets.iter().enumerate() {
                if s.node_id.is_some() && s.node_id == slot_tbl.selected_jewel_node_id.get() {
                    d.sel_index.set(i + 1);
                    break;
                }
            }
            controls.insert("jewelSlot".into(), d.clone());
            jewel_slot = Some(d);
            last = r;
            popup_height += 23.0;
        }
        let mut ring_slot: Option<Rc<DropDownControl>> = None;
        if row_name == "Pearl of Tsoatha" {
            let mut rings: Vec<Rc<ItemSlotControl>> = Vec::new();
            for i in 1..=3 {
                if let Some(s) = tab.slots.borrow().get(&format!("Ring {i}"))
                    && s.drop.base.shown.get(g)
                {
                    rings.push(s.clone());
                }
            }
            rings.sort_by(|a, b| a.label.borrow().cmp(&b.label.borrow()));
            let list: Vec<DropItem> = rings
                .iter()
                .map(|s| {
                    let mut d = DropItem::table(&s.label.borrow());
                    d.data = Some(s.clone());
                    d
                })
                .collect();
            let d = DropDownControl::new(
                g,
                anchor("TOPLEFT", &last, "BOTTOMLEFT"),
                rect(0.0, 5.0, 100.0, 18.0),
                list,
                None,
                None,
                false,
            );
            let r: ControlRef = d.clone();
            controls.insert(
                "ringSlotLabel".into(),
                LabelControl::new(anchor("RIGHT", &r, "LEFT"), rect(-5.0, 0.0, 0.0, 16.0), "Ring Slot:".to_string()),
            );
            controls.insert("ringSlot".into(), d.clone());
            ring_slot = Some(d);
            last = r;
            popup_height += 23.0;
        }
        let mut include_mirrored = None;
        if !unique {
            let c = CheckBoxControl::new(
                g,
                anchor("TOPRIGHT", &last, "BOTTOMRIGHT"),
                [0.0, 5.0, 18.0],
                Some("Mirrored Items:"),
                None,
                None,
                false,
            );
            c.state.set(self.last_include_mirrored.get().is_none_or(|v| v));
            last = c.clone();
            controls.insert("includeMirrored".into(), c.clone());
            include_mirrored = Some(c);
            popup_height += 23.0;
        }
        let mut include_scourge = None;
        if !is_jewel && !is_abyssal && include_scourge_opt {
            let c = CheckBoxControl::new(
                g,
                anchor("TOPLEFT", &last, "BOTTOMLEFT"),
                [0.0, 5.0, 18.0],
                Some("Scourge Mods:"),
                None,
                None,
                false,
            );
            c.state.set(self.last_include_scourge.get().is_none_or(|v| v));
            last = c.clone();
            controls.insert("includeScourge".into(), c.clone());
            include_scourge = Some(c);
            popup_height += 23.0;
        }
        let mut include_talisman = None;
        if is_amulet {
            let c = CheckBoxControl::new(
                g,
                anchor("TOPRIGHT", &last, "BOTTOMRIGHT"),
                [0.0, 5.0, 18.0],
                Some("Talisman Mods:"),
                None,
                Some("Whether talisman enchant mods should be included. Disabled by default due to the maximum filter limit."),
                false,
            );
            c.state.set(self.last_include_talisman.get() == Some(true));
            last = c.clone();
            controls.insert("includeTalisman".into(), c.clone());
            include_talisman = Some(c);
            popup_height += 23.0;
        }
        let mut include_eldritch = None;
        if is_eldritch {
            let tip = "Controls the inclusion of eldritch mod weights in the weighted sum.\nCopy Current: implicits in weights are skipped and augments are replaced with the\ncurrent implicits when possible. Usually the best opinion as this ensures the\naugments make sense for your build.\n\nKeep regular: weights are generated and implicits are kept, but conditional\n\"while unique/atlas boss\" modifiers are removed from the items.\n\nKeep regular + presence: weights are generated and implicits are kept. Not\nrecommended as many of these implicits are impractical and appear powerful\nwith default PoB enemy configs.\n\nRemove: eldritch implicits are removed and ignored in the search.";
            let list = ["Copy Current", "Keep regular", "Keep regular+presence", "Remove"]
                .map(DropItem::str)
                .to_vec();
            let d = DropDownControl::new(
                g,
                anchor("TOPLEFT", &last, "BOTTOMLEFT"),
                rect(0.0, 5.0, 140.0, 18.0),
                list,
                None,
                Some(tip),
                false,
            );
            let r: ControlRef = d.clone();
            controls.insert(
                "includeEldritchLabel".into(),
                LabelControl::new(
                    anchor("RIGHT", &r, "LEFT"),
                    rect(-4.0, 0.0, 80.0, 16.0),
                    "Eldritch Mods:".to_string(),
                ),
            );
            if let Some(v) = &*self.last_include_eldritch.borrow() {
                d.sel_by_label(v);
            }
            controls.insert("includeEldritch".into(), d.clone());
            include_eldritch = Some(d);
            last = r;
            popup_height += 23.0;
        }
        let mut copy_enchant = None;
        if is_amulet || is_belt || is_weapon {
            let term = if is_weapon { "enchants" } else { "anoints" };
            let tip = format!(
                "Keep: {term} will be unchanged on the search results.\nCopy Current: current {term} will be applied to the search result items.\nRemove: {term} will be removed from the search results."
            );
            let list = ["Keep", "Copy Current", "Remove"].map(DropItem::str).to_vec();
            let d = DropDownControl::new(
                g,
                anchor("TOPLEFT", &last, "BOTTOMLEFT"),
                rect(0.0, 5.0, 120.0, 18.0),
                list,
                None,
                Some(&tip),
                false,
            );
            let r: ControlRef = d.clone();
            let text = if is_weapon { "^7Enchant Behaviour:" } else { "^7Anoint Behaviour:" };
            controls.insert(
                "copyEnchantModeLabel".into(),
                LabelControl::new(anchor("RIGHT", &r, "LEFT"), rect(-4.0, 0.0, 80.0, 16.0), text.to_string()),
            );
            controls.insert("copyEnchantMode".into(), d.clone());
            copy_enchant = Some(d);
            last = r;
            popup_height += 23.0;
        }
        // :1292-1305
        let set_all_mod_selectors: Rc<RefCell<Option<Rc<dyn Fn(&mut Gfx)>>>> = Rc::new(RefCell::new(None));
        let mut jewel_type: Option<Rc<DropDownControl>> = None;
        if is_jewel && !unique {
            let s = set_all_mod_selectors.clone();
            let d = DropDownControl::new(
                g,
                anchor("TOPLEFT", &last, "BOTTOMLEFT"),
                rect(0.0, 5.0, 100.0, 18.0),
                vec![DropItem::str("Base"), DropItem::str("Abyss")],
                Some(Rc::new(move |g: &mut Gfx, _i: usize, _v: &DropItem| {
                    let f = s.borrow().clone();
                    if let Some(f) = f {
                        f(g);
                    }
                })),
                None,
                false,
            );
            d.sel_index.set(self.last_jewel_type.get().unwrap_or(1));
            let r: ControlRef = d.clone();
            controls.insert(
                "jewelTypeLabel".into(),
                LabelControl::new(anchor("RIGHT", &r, "LEFT"), rect(-5.0, 0.0, 0.0, 16.0), "Jewel Type:".to_string()),
            );
            controls.insert("jewelType".into(), d.clone());
            jewel_type = Some(d);
            last = r;
            popup_height += 23.0;
        } else if is_abyssal {
            let d = DropDownControl::new(
                g,
                anchor("TOPLEFT", &last, "BOTTOMLEFT"),
                rect(0.0, 5.0, 100.0, 18.0),
                vec![DropItem::str("Abyss")],
                None,
                None,
                false,
            );
            let r: ControlRef = d.clone();
            controls.insert(
                "jewelTypeLabel".into(),
                LabelControl::new(anchor("RIGHT", &r, "LEFT"), rect(-5.0, 0.0, 0.0, 16.0), "Jewel Type:".to_string()),
            );
            controls.insert("jewelType".into(), d.clone());
            jewel_type = Some(d);
            last = r;
            popup_height += 23.0;
        }
        // :1307-1316
        let edit = |g: &Gfx, a, w: f64, init: Option<f64>| {
            let e = EditControl::new(
                g,
                a,
                rect(0.0, 5.0, w, 18.0),
                EditArgs {
                    filter: Some("%D".to_string()),
                    ..EditArgs::default()
                },
            );
            *e.buf.borrow_mut() = init.map_or(String::new(), pob_lua::format::tostring_num);
            e
        };
        let max_price = edit(g, anchor("TOPLEFT", &last, "BOTTOMLEFT"), 70.0, self.last_max_price.get());
        let mpr: ControlRef = max_price.clone();
        let max_price_type = DropDownControl::new(
            g,
            anchor("LEFT", &mpr, "RIGHT"),
            rect(5.0, 0.0, 150.0, 18.0),
            CURRENCY_TABLE.iter().map(|(n, _)| DropItem::str(n)).collect(),
            None,
            Some(
                "^7The trade site will filter out listings with other currencies,\nif anything other than \"Chaos Orb Equivalent\" is chosen and a maximum is specified.",
            ),
            false,
        );
        max_price_type.sel_index.set(self.last_max_price_type_index.get().unwrap_or(1));
        controls.insert(
            "maxPriceLabel".into(),
            LabelControl::new(
                anchor("RIGHT", &mpr, "LEFT"),
                rect(-5.0, 0.0, 0.0, 16.0),
                "^7Max Price:".to_string(),
            ),
        );
        controls.insert("maxPrice".into(), max_price.clone());
        controls.insert("maxPriceType".into(), max_price_type.clone());
        last = mpr;
        popup_height += 23.0;
        let max_level = edit(g, anchor("TOPLEFT", &last, "BOTTOMLEFT"), 100.0, self.last_max_level.get());
        let mlr: ControlRef = max_level.clone();
        controls.insert(
            "maxLevelLabel".into(),
            LabelControl::new(
                anchor("RIGHT", &mlr, "LEFT"),
                rect(-5.0, 0.0, 0.0, 16.0),
                "^7Max Level:".to_string(),
            ),
        );
        controls.insert("maxLevel".into(), max_level.clone());
        last = mlr;
        popup_height += 23.0;
        let (mut sockets_edit, mut links_edit) = (None, None);
        if let Some(n) = name
            && !is_jewel
            && !is_abyssal
            && !n.contains("Flask")
        {
            let s = edit(g, anchor("TOPLEFT", &last, "BOTTOMLEFT"), 70.0, self.last_sockets.get());
            let sr: ControlRef = s.clone();
            controls.insert(
                "socketsLabel".into(),
                LabelControl::new(
                    anchor("RIGHT", &sr, "LEFT"),
                    rect(-5.0, 0.0, 0.0, 16.0),
                    "^7# of Empty Sockets:".to_string(),
                ),
            );
            controls.insert("sockets".into(), s.clone());
            sockets_edit = Some(s);
            last = sr;
            popup_height += 23.0;
            if !n.contains("Belt") && !n.contains("Ring") && !n.contains("Amulet") {
                let l = edit(g, anchor("TOPLEFT", &last, "BOTTOMLEFT"), 70.0, None);
                let lr: ControlRef = l.clone();
                controls.insert(
                    "linksLabel".into(),
                    LabelControl::new(
                        anchor("RIGHT", &lr, "LEFT"),
                        rect(-5.0, 0.0, 0.0, 16.0),
                        "^7# of Links:".to_string(),
                    ),
                );
                controls.insert("links".into(), l.clone());
                links_edit = Some(l);
                last = lr;
                popup_height += 23.0;
            }
        }
        // :1337-1358 "tooltips do not actually work for labels": the label's `tooltipFunc` is not ported, nothing calls it
        let count = stat_weights.len();
        for (idx, stat) in stat_weights.iter().enumerate() {
            let i = idx + 1;
            let text = if i < (if count < 6 { 10 } else { 5 }) {
                let v = stat.weight_mult.unwrap_or_else(|| {
                    panic!("Classes/TradeQueryGenerator.lua:1338: bad argument #2 to 'format' (number expected, got nil)")
                });
                pob_lua::format::format(
                    "^7%.2f: %s",
                    &[pob_lua::format::Arg::Num(v), pob_lua::format::Arg::Str(&stat.label)],
                )
                .expect("format")
            } else {
                format!("+ {} Additional Stats", count as i64 - 4)
            };
            let l = LabelControl::new(
                anchor("TOPLEFT", &last, "BOTTOMLEFT"),
                rect(0.0, if i == 1 { 5.0 } else { 3.0 }, 70.0, 16.0),
                text,
            );
            let r: ControlRef = l.clone();
            controls.insert(format!("sortStatType{i}"), l);
            last = r.clone();
            popup_height += 19.0;
            if i == 1 {
                controls.insert(
                    "sortStatLabel".into(),
                    LabelControl::new(
                        anchor("RIGHT", &r, "LEFT"),
                        rect(-5.0, 0.0, 0.0, 16.0),
                        "^7Stat to Sort By:".to_string(),
                    ),
                );
            } else if i == 5 {
                break;
            }
        }
        popup_height += 4.0;
        let mut include_all_we = None;
        if row_name == "Watcher's Eye" {
            let c = CheckBoxControl::new(
                g,
                anchor("TOPRIGHT", &last, "BOTTOMRIGHT"),
                [0.0, 5.0, 18.0],
                Some("Include all Watcher's Eye mods:"),
                None,
                Some("Include mods that could not have a weight calculated for them at weight 0."),
                false,
            );
            last = c.clone();
            controls.insert("includeAllWEMods".into(), c.clone());
            include_all_we = Some(c);
            popup_height += 23.0;
        }
        let selected_mods: Rc<RefCell<Vec<ModSel>>> = Rc::new(RefCell::new(Vec::new()));
        let not_mods: Rc<RefCell<Vec<ModSel>>> = Rc::new(RefCell::new(Vec::new()));
        // :1369-1434
        let me = self.clone();
        let tq2 = tq.clone();
        let (sm, nm) = (selected_mods.clone(), not_mods.clone());
        let (je, rs, st) = (jewel_slot.clone(), ring_slot.clone(), slot_tbl.clone());
        let (cm, co, ce, cs, ct, cj) = (
            include_mirrored.clone(),
            include_corrupted.clone(),
            include_eldritch.clone(),
            include_scourge.clone(),
            include_talisman.clone(),
            jewel_type.clone(),
        );
        let (ep, ept, el, es, eli, ceh, iw) = (
            max_price.clone(),
            max_price_type.clone(),
            max_level.clone(),
            sockets_edit.clone(),
            links_edit.clone(),
            copy_enchant.clone(),
            include_all_we.clone(),
        );
        let (sl, ops, weights) = (slot.clone(), options.clone(), stat_weights.clone());
        let generate = ButtonControl::new(
            anchor_nil("BOTTOM", "BOTTOM"),
            rect(-45.0, -10.0, 80.0, 20.0),
            "Execute".to_string(),
            move |g| {
                main().close_popup();
                let mut options = ops.clone();
                let mut slot = sl.clone();
                me.trade_type_index.set(
                    tq2.trade_type_selection
                        .borrow()
                        .as_ref()
                        .expect("tradeTypeSelection")
                        .sel_index
                        .get(),
                );
                *me.last_copy_enchant_mode.borrow_mut() = ceh.as_ref().and_then(|c| c.get_sel_value()).and_then(|v| v.label);
                if let Some(c) = &cm {
                    me.last_include_mirrored.set(Some(c.state.get()));
                    options.include_mirrored = Some(c.state.get());
                }
                me.last_include_corrupted.set(Some(co.state.get()));
                options.include_corrupted = Some(co.state.get());
                if let Some(c) = &ce {
                    let v = c.get_sel_value().and_then(|v| v.label);
                    *me.last_include_eldritch.borrow_mut() = v.clone();
                    options.include_eldritch = v;
                }
                if let Some(c) = &cs {
                    me.last_include_scourge.set(Some(c.state.get()));
                    options.include_scourge = Some(c.state.get());
                }
                if let Some(c) = &ct {
                    me.last_include_talisman.set(Some(c.state.get()));
                    options.include_talisman = Some(c.state.get());
                }
                if let Some(c) = &cj {
                    me.last_jewel_type.set(Some(c.sel_index.get()));
                    options.jewel_type = c.get_sel_value().and_then(|v| v.label);
                }
                options.max_price = pob_lua::tonumber(&ep.buf.borrow());
                me.last_max_price.set(options.max_price);
                options.max_price_type = CURRENCY_TABLE[ept.sel_index.get() - 1].1;
                me.last_max_price_type_index.set(Some(ept.sel_index.get()));
                options.max_level = pob_lua::tonumber(&el.buf.borrow());
                me.last_max_level.set(options.max_level);
                if let Some(c) = &es {
                    options.sockets = pob_lua::tonumber(&c.buf.borrow());
                    me.last_sockets.set(options.sockets);
                }
                if let Some(c) = &eli {
                    options.links = pob_lua::tonumber(&c.buf.borrow());
                }
                if let Some(c) = &iw {
                    options.include_all_we_mods = Some(c.state.get());
                }
                options.required_mods.clone_from(&sm.borrow());
                options.blocked_mods.clone_from(&nm.borrow());
                options.stat_weights.clone_from(&weights);
                let slot_of = |d: &DropDownControl| {
                    d.get_sel_value()
                        .and_then(|v| v.data)
                        .and_then(|d| d.downcast::<ItemSlotControl>().ok())
                };
                if let Some(d) = &je {
                    slot = slot_of(d);
                    st.selected_jewel_node_id.set(slot.as_ref().and_then(|s| s.node_id));
                }
                if let Some(d) = &rs {
                    slot = slot_of(d);
                    *st.selected_slot_name.borrow_mut() = slot.as_ref().map(|s| s.slot_name.clone());
                }
                me.start_query(g, slot, options);
            },
        );
        if let Some(d) = &jewel_slot {
            let d = d.clone();
            generate.base.enabled.set_fn(move |_| d.get_sel_value().is_some());
            generate.tip.tooltip_text.set(Some("Requires an active Jewel Socket.".to_string()));
        }
        controls.insert("generateQuery".into(), generate);
        controls.insert(
            "cancel".into(),
            ButtonControl::new(
                anchor_nil("BOTTOM", "BOTTOM"),
                rect(45.0, -10.0, 80.0, 20.0),
                "Cancel".to_string(),
                |_g| {
                    main().close_popup();
                },
            ),
        );
        if unique {
            main().open_popup(g, popup_width, popup_height, "Query Options", controls, None, None, None, None);
            return;
        }
        // :1448-1605
        let total_width = 420.0;
        let field_width = 60.0;
        let button_size = 20.0;
        let x_spacing = 4.0;
        let aux = button_size + field_width + 2.0 * x_spacing;
        let (_, last_y) = last.get_pos(g);
        let (_, last_h) = last.get_size(g);
        let header = BareControl::new(
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect((popup_width - total_width) / 2.0, last_h + last_y, 0.0, 0.0),
        );
        last = header.clone();
        controls.insert("modSelectorHeaderAnchor".into(), header);
        popup_height += 23.0;

        let tabw = self.items_tab.clone();
        let slot2 = slot.clone();
        let jt = jewel_type.clone();
        // `getModList(firstLabel)`
        let get_mod_list = Rc::new(move |first: &str| -> Vec<DropItem> {
            let tab = tab_of(&tabw);
            let s = slot2.as_ref().expect("`slot.slotName` (:1465)");
            let existing = tab.read(|e| e.item_by_id(s.sel_item_id.get()));
            let (_, cat) = get_trade_category(&s.slot_name, existing.as_deref());
            let mut cat = cat.map(str::to_string);
            if let Some(j) = &jt
                && cat.as_deref() == Some("Jewel")
            {
                cat = Some(format!("{}Jewel", j.get_sel_value().and_then(|v| v.label).unwrap_or_default()));
            }
            let mut mods = vec![mod_item(first, None)];
            let mut included = HashSet::new();
            let data = mod_data();
            let cat = cat.unwrap_or_default();
            for ty in ["Explicit", "Implicit", "Corrupted"] {
                for m in mods_in_pairs_order(&data, ty) {
                    if m.ranges.contains_key(&cat) && !included.contains(&m.trade_id) {
                        let mut text = format!(
                            "{}{}",
                            cc("MAGIC"),
                            gsub(&m.trade_text, "(%a+) Passive Skills in Radius also grant ", "%1: ")
                        );
                        included.insert(m.trade_id.clone());
                        if ty != "Explicit" {
                            let colour = if ty == "Corrupted" { "^x9E3E38" } else { "^x989654" };
                            text.push_str(&format!(" {colour}({ty})"));
                        }
                        mods.push(mod_item(&text, Some(&m.trade_id)));
                    }
                }
            }
            const IGNORED: [&str; 6] = [
                "^pseudo.lake",
                "^pseudo.pseudo_lake",
                "^pseudo.pseudo_logbook",
                "^pseudo.pseudo_temple",
                "^pseudo.pseudo_map",
                "^pseudo.pseudo_ritual",
            ];
            for e in pseudo_entries() {
                if IGNORED.iter().any(|p| lmatch(&e.id, p).is_some()) {
                    continue;
                }
                mods.push(mod_item(&format!("{}{} (Pseudo)", cc("MEMORY"), e.text), Some(&e.id)));
            }
            mods
        });
        let popup_before = popup_height;
        const MAX_SELECTORS: usize = 5;
        // the rows of a list (`controls[prefix .. i]`, `controls[prefix .. "Min" .. i]`)
        type Rows = Rc<RefCell<Vec<(Rc<DropDownControl>, Rc<EditControl>)>>>;
        let set_mod_selectors = {
            let total = total_width;
            Rc::new(
                move |g: &mut Gfx, rows: &Rows, mod_list: Option<Vec<DropItem>>, selected: &Rc<RefCell<Vec<ModSel>>>| {
                    if mod_list.is_some() {
                        selected.borrow_mut().clear();
                    }
                    for (i, (selector, min_box)) in rows.borrow().iter().enumerate() {
                        if let Some(l) = &mod_list {
                            selector.set_list(g, Some(l.clone()));
                        }
                        let m = selected.borrow().get(i).cloned();
                        if let Some(m) = m {
                            selector.sel_by_label(&m.label);
                            selector.base.width.set(total - aux);
                            *min_box.buf.borrow_mut() = m.value.map_or(String::new(), pob_lua::format::tostring_num);
                        } else {
                            selector.sel_index.set(1);
                            selector.base.width.set(total);
                        }
                        selector.check_dropped_width(g, true);
                    }
                },
            )
        };
        let mod_rows: Rows = Rc::new(RefCell::new(Vec::new()));
        let not_rows: Rows = Rc::new(RefCell::new(Vec::new()));
        let create_rows = |g: &mut Gfx,
                           rows: &Rows,
                           selected: &Rc<RefCell<Vec<ModSel>>>,
                           prefix: &str,
                           last: &mut ControlRef,
                           popup_height: &mut f64,
                           controls: &mut IndexMap<String, ControlRef>| {
            for i in 1..=MAX_SELECTORS {
                let (sel, rs, sms) = (selected.clone(), rows.clone(), set_mod_selectors.clone());
                let dropdown = DropDownControl::new(
                    g,
                    anchor_collapse("TOPLEFT", last, "BOTTOMLEFT"),
                    rect(0.0, 4.0, total_width, 20.0),
                    Vec::new(),
                    Some(Rc::new(move |g: &mut Gfx, idx: usize, val: &DropItem| {
                        if idx == 1 {
                            if i <= sel.borrow().len() {
                                sel.borrow_mut().remove(i - 1);
                            }
                        } else {
                            let m = ModSel {
                                label: val.label.clone().unwrap_or_default(),
                                trade_id: val.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned(),
                                value: None,
                            };
                            let mut s = sel.borrow_mut();
                            if i - 1 < s.len() {
                                s[i - 1] = m;
                            } else {
                                s.push(m);
                            }
                        }
                        sms(g, &rs, None, &sel);
                    })),
                    None,
                    true,
                );
                let sel = selected.clone();
                dropdown.base.shown.set_fn(move |_| i == 1 || sel.borrow().len() >= i - 1);
                let dr: ControlRef = dropdown.clone();
                *last = dr.clone();
                *popup_height += 23.0;
                controls.insert(format!("{prefix}{i}"), dropdown.clone());
                let sel = selected.clone();
                let min_box = new_plain_numeric_edit(
                    g,
                    anchor("LEFT", &dr, "RIGHT"),
                    rect(x_spacing, 0.0, field_width, button_size),
                    "",
                    "Min",
                    6,
                    false,
                    Some(Rc::new(move |_g: &mut Gfx, val: &str, _u: bool| {
                        let mut s = sel.borrow_mut();
                        let m = s
                            .get_mut(i - 1)
                            .unwrap_or_else(|| panic!("Classes/TradeQueryGenerator.lua:1565: attempt to index a nil value"));
                        m.value = pob_lua::tonumber(val);
                    })),
                );
                let sel = selected.clone();
                min_box.base.shown.set_fn(move |_| sel.borrow().len() >= i);
                let mr: ControlRef = min_box.clone();
                controls.insert(format!("{prefix}Min{i}"), min_box.clone());
                let (sel, rs, sms) = (selected.clone(), rows.clone(), set_mod_selectors.clone());
                let clear = ButtonControl::new(
                    anchor("LEFT", &mr, "RIGHT"),
                    rect(x_spacing, 0.0, button_size, button_size),
                    "x".to_string(),
                    move |g| {
                        if i <= sel.borrow().len() {
                            sel.borrow_mut().remove(i - 1);
                        }
                        sms(g, &rs, None, &sel);
                    },
                );
                let sel = selected.clone();
                clear.base.shown.set_fn(move |_| sel.borrow().len() >= i);
                controls.insert(format!("{prefix}Clear{i}"), clear);
                rows.borrow_mut().push((dropdown, min_box));
            }
        };
        let and_label = LabelControl::new(
            anchor("TOPLEFT", &last, "BOTTOMLEFT"),
            rect(0.0, 8.0, total_width, 16.0),
            "^7Required Stats".to_string(),
        );
        last = and_label.clone();
        controls.insert("andLabel".into(), and_label);
        popup_height += 18.0;
        create_rows(
            g,
            &mod_rows,
            &selected_mods,
            "modSelector",
            &mut last,
            &mut popup_height,
            &mut controls,
        );
        let not_label = LabelControl::new(
            anchor_collapse("TOPLEFT", &last, "BOTTOMLEFT"),
            rect(0.0, 4.0, total_width, 16.0),
            "^7Blocked Stats".to_string(),
        );
        not_label.base.collapse_y.set(Some(8.0));
        last = not_label.clone();
        controls.insert("notLabel".into(), not_label);
        popup_height += 18.0;
        create_rows(
            g,
            &not_rows,
            &not_mods,
            "modNotSelector",
            &mut last,
            &mut popup_height,
            &mut controls,
        );
        let _ = last;
        let set_all: Rc<dyn Fn(&mut Gfx)> = {
            let (mr, nr, sm, nm, sms, gml) = (
                mod_rows.clone(),
                not_rows.clone(),
                selected_mods.clone(),
                not_mods.clone(),
                set_mod_selectors.clone(),
                get_mod_list.clone(),
            );
            Rc::new(move |g: &mut Gfx| {
                sms(g, &mr, Some(gml("^7+ Add Required Stat")), &sm);
                sms(g, &nr, Some(gml("^7+ Add Blocked Stat")), &nm);
            })
        };
        *set_all_mod_selectors.borrow_mut() = Some(set_all.clone());
        set_all(g);
        // :1601-1604 `resizeFunc`: `main.popups[1].height`. `PopupDialog.y` keeps the height of its constructor
        let (sm, nm) = (selected_mods.clone(), not_mods.clone());
        let resize: crate::controls::popup::PopupFunc = Rc::new(move |_g: &Gfx| {
            let height = sm.borrow().len().min(MAX_SELECTORS - 1) as f64 * 23.0
                + nm.borrow().len().min(MAX_SELECTORS - 1) as f64 * 23.0
                + 2.0 * 18.0;
            if let Some(p) = main().popups.borrow().first() {
                p.base.height.set(Prop::Val(popup_before + height));
            }
        });
        let popup = PopupDialog::new(
            g,
            popup_width,
            popup_height,
            "Query Options",
            controls,
            None,
            None,
            None,
            None,
            Some(resize),
        );
        main().popups.borrow_mut().insert(0, popup);
    }

    /// The running query's state, for a dump.
    pub fn is_running(&self) -> bool {
        self.calc.borrow().is_some()
    }
}

fn mod_item(label: &str, trade_id: Option<&str>) -> DropItem {
    let mut d = DropItem::table(label);
    d.data = trade_id.map(|t| Rc::new(t.to_string()) as Rc<dyn std::any::Any>);
    d
}

/// `{ id = entry.tradeId, value = { min = entry.value } }` (:1063, :1067): a nil min leaves an empty table.
fn mod_filter(e: &ModSel) -> Json {
    let value = match e.value {
        Some(v) => json!({"min": dkjson::n(v)}),
        None => json!([]),
    };
    let mut o = json!({"value": value});
    if let Some(id) = &e.trade_id {
        o["id"] = json!(id);
    }
    o
}
