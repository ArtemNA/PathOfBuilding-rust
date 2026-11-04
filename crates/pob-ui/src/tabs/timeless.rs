//! `TreeTab:FindTimelessJewel` (TreeTab.lua:1169-2876), `TimelessJewelListControl`, `TimelessJewelSocketControl` (the socket viewer: `DropDownControl::post_draw`).
//!

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_gfx::{Frame, Gfx};

use crate::control::{Control, ControlRef, Prop, Rect, anchor, anchor_nil, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::list::{ByRef, ListControl, ListHooks};
use crate::controls::popup::PopupDialog;
use crate::controls::scroll_bar::SliderControl;
use crate::undo::UndoHost;
use crate::modules::main::{main, note_not_ported};
use serde_json::json;
use crate::sg;
use crate::tooltip::Tooltip;

const JEWEL_TYPES: [(&str, &str, usize); 11] = [
    ("Glorious Vanity", "vaal", 1),
    ("Lethal Pride", "karui", 2),
    ("Brutal Restraint", "maraketh", 3),
    ("Militant Faith", "templar", 4),
    ("Elegant Hubris", "eternal", 5),
    ("Heroic Tragedy", "kalguur", 6),
    ("Festering Vengeance", "abyss_murderous", 7),
    ("Extinguishing Grasp", "abyss_searching", 8),
    ("Baleful Dominion", "abyss_hypnotic", 9),
    ("Destructive Aspiration", "abyss_ghastly", 10),
    ("Reclaimed Malevolence", "abyss_special", 11),
];
const IGNORED_MODS: [&str; 9] = [
    "Might of the Vaal",
    "Legacy of the Vaal",
    "Strength",
    "Add Strength",
    "Dex",
    "Add Dexterity",
    "Devotion",
    "Price of Glory",
    "Ward",
];
const REVERSE_TOTAL_MOD_IDS: [&str; 9] = [
    "karui_notable_add_strength",
    "karui_attribute_strength",
    "karui_small_strength",
    "maraketh_notable_add_dexterity",
    "maraketh_attribute_dex",
    "maraketh_small_dex",
    "templar_notable_devotion",
    "templar_devotion_node",
    "templar_small_devotion",
];
const DEVOTION_VARIANTS: [(usize, &str); 17] = [
    (1, "Any"),
    (2, "Totem Damage"),
    (3, "Brand Damage"),
    (4, "Channelling Damage"),
    (5, "Area Damage"),
    (6, "Elemental Damage"),
    (7, "Elemental Resistances"),
    (8, "Effect of non-Damaging Ailments"),
    (9, "Elemental Ailment Duration"),
    (10, "Duration of Curses"),
    (11, "Minion Attack and Cast Speed"),
    (12, "Minions Accuracy Rating"),
    (13, "Mana Regen"),
    (14, "Mana Cost (legacy)"),
    (17, "Mana Cost Efficiency"),
    (15, "Non-Curse Aura Effect"),
    (16, "Defences from Shield"),
];

fn conqueror_types(jewel: usize) -> Vec<&'static str> {
    match jewel {
        1 => vec![
            "Any",
            "Doryani (Corrupted Soul)",
            "Xibaqua (Divine Flesh)",
            "Ahuana (Immortal Ambition)",
        ],
        2 => vec![
            "Any",
            "Kaom (Strength of Blood)",
            "Rakiata (Tempered by War)",
            "Akoya (Chainbreaker)",
        ],
        3 => vec![
            "Any",
            "Asenath (Dance with Death)",
            "Nasima (Second Sight)",
            "Balbala (The Traitor)",
        ],
        4 => vec![
            "Any",
            "Avarius (Power of Purpose)",
            "Dominus (Inner Conviction)",
            "Maxarius (Transcendence)",
        ],
        5 => vec![
            "Any",
            "Cadiro (Supreme Decadence)",
            "Victario (Supreme Grandstanding)",
            "Caspiro (Supreme Ostentation)",
        ],
        6 => vec![
            "Any",
            "Vorana (Black Scythe Training)",
            "Uhtred (Celestial Mathematics)",
            "Medved (The Unbreaking Circle)",
        ],
        7 => vec!["Tecrod (Overwhelming Hate)"],
        8 => vec!["Ulaman (Weighted Exchange)"],
        9 => vec!["Kurgal (Reconstructed Essence)"],
        10 => vec!["Amanamu (The Loyal Few)"],
        _ => vec!["Zorath"],
    }
}

fn total_mod(jewel: usize) -> Option<&'static str> {
    match jewel {
        2 => Some("Strength"),
        3 => Some("Dexterity"),
        4 => Some("Devotion"),
        _ => None,
    }
}

fn jewel_name(id: usize) -> &'static str {
    JEWEL_TYPES[id - 1].1
}

fn jewel_label(id: usize) -> &'static str {
    JEWEL_TYPES[id - 1].0
}

fn drop_items(list: impl IntoIterator<Item = String>) -> Vec<DropItem> {
    list.into_iter().map(|l| DropItem::table(&l)).collect()
}

fn fmt(f: &str, v: f64) -> String {
    pob_lua::format::format(f, &[pob_lua::format::Arg::Num(v)]).expect("format")
}

fn ts(v: f64) -> String {
    pob_lua::tostring_num(v)
}

/// A cell of `searchListTbl`: a string from the text or a number the sliders wrote.
#[derive(Clone, Debug, PartialEq)]
enum TCell {
    Str(String),
    Num(f64),
}

impl TCell {
    fn tonumber(&self) -> Option<f64> {
        match self {
            TCell::Str(s) => pob_lua::tonumber::tonumber(s),
            TCell::Num(n) => Some(*n),
        }
    }

    fn text(&self) -> String {
        match self {
            TCell::Str(s) => s.clone(),
            TCell::Num(n) => ts(*n),
        }
    }
}

#[derive(Clone)]
struct JewelSocket {
    label: String,
    keystone: String,
    id: f64,
}

#[derive(Clone)]
struct ModEntry {
    label: String,
    descriptions: Vec<String>,
    id: Option<String>,
}

#[derive(Clone, Default)]
struct Shared {
    jewel: usize,
    conqueror: usize,
    devotion1: usize,
    devotion2: usize,
    socket: Option<JewelSocket>,
    desired: IndexMap<String, Desired>,
}

#[derive(Clone)]
struct Desired {
    weight: f64,
    weight2: f64,
    display: String,
    idx: usize,
}

/// A row of `timelessData.searchResults`.
pub struct ResultRow {
    label: RefCell<String>,
    seed: f64,
    total: f64,
    socket_id: Option<f64>,
    socket_label: Option<String>,
    targets: IndexMap<String, Vec<String>>,
    alt1: Cell<Option<usize>>,
    alt2: Cell<Option<usize>>,
}

type Row = ByRef<ResultRow>;

/// What lives on `build` / `treeTab` across the popups.
#[derive(Default)]
pub struct TimelessPersist {
    results: RefCell<Vec<Row>>,
    shared: RefCell<Shared>,
    total_minimum_weight: Cell<Option<f64>>,
    last_search_more: Cell<bool>,
    trade_type_index: Cell<Option<usize>>,
    selected_realm_index: Cell<Option<usize>>,
    /// `self.tradeLeaguesList` (TreeTab.lua:188).
    trade_leagues: RefCell<HashMap<String, Vec<String>>>,
    /// `self.timelessJewelLeagueSelect`.
    league_select: RefCell<Option<String>>,
    /// `self.tradeQueryRequests`.
    requests: crate::trade::requests::TradeQueryRequests,
}

/// The locals of `FindTimelessJewel`.
#[derive(Default)]
struct Local {
    sockets: Vec<JewelSocket>,
    mods: Vec<ModEntry>,
    search: Vec<Vec<TCell>>,
    fallback: Vec<Vec<TCell>>,
    protected: HashMap<String, bool>,
    protected_count: i64,
    allocated_in_radius: i64,
    box_count: usize,
    slider_label: String,
    slider2_label: String,
    highlight: Cell<Option<usize>>,
    /// `controls.searchTradeButton.lastSearch`.
    last_search: Cell<Option<(usize, usize)>>,
    /// The URL of the last `OpenURL` of the popup, for the gate's dump.
    last_trade_url: RefCell<Option<String>>,
}

thread_local! {
    static CONTROLS: RefCell<HashMap<String, ControlRef>> = RefCell::new(HashMap::new());
    static BUILDING: RefCell<IndexMap<String, ControlRef>> = RefCell::new(IndexMap::new());
    static POPUP: RefCell<Option<Rc<PopupDialog>>> = const { RefCell::new(None) };
    static LOCAL: RefCell<Local> = RefCell::new(Local::default());
}

thread_local! {
    /// `pairs(nodesInRadius[3])` of each socket in a process of the original (R13: `pairs(self.nodes)` fills the table in another order in every process, tests/tree.rs), as `PassiveTreeView::set_order_sample`.
    /// Without it: the order of this process, one of those the original can have. It decides the order the search adds non-integer weights in (`Generate`), and so the ties of the results.
    static RADIUS_ORDER_SAMPLE: RefCell<Option<HashMap<u32, Vec<u32>>>> = const { RefCell::new(None) };
}

/// See [`RADIUS_ORDER_SAMPLE`].
pub fn set_radius_order_sample(sample: HashMap<u32, Vec<u32>>) {
    RADIUS_ORDER_SAMPLE.with_borrow_mut(|s| *s = Some(sample));
}

fn local<R>(f: impl FnOnce(&mut Local) -> R) -> R {
    LOCAL.with_borrow_mut(f)
}

/// `controls.<name> = control`.
fn reg(name: &str, c: ControlRef) {
    CONTROLS.with_borrow_mut(|m| m.insert(name.to_string(), c.clone()));
    let popup = POPUP.with_borrow(Clone::clone);
    match popup {
        Some(p) => p.host.add(name, c),
        None => {
            BUILDING.with_borrow_mut(|b| b.insert(name.to_string(), c));
        }
    }
}

fn unreg(name: &str) {
    CONTROLS.with_borrow_mut(|m| m.remove(name));
    let popup = POPUP.with_borrow(Clone::clone);
    match popup {
        Some(p) => {
            p.host.controls.borrow_mut().shift_remove(name);
        }
        None => {
            BUILDING.with_borrow_mut(|b| b.shift_remove(name));
        }
    }
}

fn get(name: &str) -> ControlRef {
    CONTROLS
        .with_borrow(|m| m.get(name).cloned())
        .unwrap_or_else(|| panic!("controls.{name} is nil"))
}

fn ctl<T: Control>(name: &str) -> Rc<T> {
    let any: Rc<dyn std::any::Any> = get(name);
    any.downcast::<T>().unwrap_or_else(|_| panic!("controls.{name}: another class"))
}

fn tab() -> Rc<crate::tabs::tree::TreeTab> {
    main().build_mode.tree_tab.borrow().clone().expect("treeTab")
}

fn engine_read<R>(f: impl FnOnce(&pob_engine::engine::Engine) -> R) -> R {
    let b = main().build_mode.clone();
    let eng = b.engine.borrow();
    f(eng.as_ref().expect("engine"))
}

/// `timelessData`'s saved fields.
fn td<R>(f: impl FnOnce(&mut pob_build::TimelessDataModel) -> R) -> R {
    let b = main().build_mode.clone();
    let mut eng = b.engine.borrow_mut();
    f(eng.as_mut().expect("engine").timeless_mut())
}

fn jewel_id() -> usize {
    td(|t| t.jewel_type_id).map_or(1, |v| v as usize)
}

fn mod_flag() {
    main().build_mode.mod_flag.set(true);
}

fn legion() -> Rc<pob_tree::PassiveTree> {
    engine_read(|e| e.spec_rc().tree.clone())
}

// --- buildMods, the search lists ---------------------------------------------------------------------------------------------------------

/// `buildMods()` (:1358-1411).
fn build_mods() -> Vec<ModEntry> {
    let tree = legion();
    let legion = tree.legion().expect("legion");
    let raw = pob_data::timeless::legion_passives().expect("legion");
    let jewel = jewel_id();
    let prefix = format!("{}_", jewel_name(jewel));
    let matches = |id: &str| id.len() > prefix.len() && id.starts_with(&prefix);
    let mut mods: Vec<ModEntry> = Vec::new();
    let mut small: Vec<ModEntry> = Vec::new();
    for (i, node) in legion.nodes.iter().enumerate() {
        let r = &raw.nodes[i];
        if matches(&node.id)
            && !node.id.starts_with("abyss_special_ascendancy_notable_")
            && !IGNORED_MODS.contains(&node.dn.as_str())
            && !r.ks
        {
            if r.not_ {
                let mut label = format!("{}                                                {}", node.dn, node.sd[0]);
                if let Some(s) = node.sd.get(1) {
                    label = format!("{label} {s}");
                }
                mods.push(ModEntry {
                    label,
                    descriptions: node.sd.clone(),
                    id: Some(node.id.clone()),
                });
            } else {
                small.push(ModEntry {
                    label: node.dn.clone(),
                    descriptions: node.sd.clone(),
                    id: Some(node.id.clone()),
                });
            }
        }
    }
    for a in &legion.additions {
        if matches(&a.id) && !IGNORED_MODS.contains(&a.dn.as_str()) && jewel_name(jewel) != "vaal" {
            mods.push(ModEntry {
                label: a.dn.clone(),
                descriptions: a.sd.clone(),
                id: Some(a.id.clone()),
            });
        }
    }
    pob_engine::lua::lua_sort(&mut mods, &mut |a: &ModEntry, b: &ModEntry| a.label.as_bytes() < b.label.as_bytes());
    pob_engine::lua::lua_sort(&mut small, &mut |a: &ModEntry, b: &ModEntry| {
        a.label.as_bytes() < b.label.as_bytes()
    });
    if let Some(t) = total_mod(jewel) {
        mods.insert(
            0,
            ModEntry {
                label: format!("Total {t}"),
                descriptions: vec![format!("This is a hybrid node containing all additions to {t}")],
                id: Some(format!("total_{}", t.to_lowercase())),
            },
        );
    }
    mods.insert(
        0,
        ModEntry {
            label: "...".into(),
            descriptions: Vec::new(),
            id: None,
        },
    );
    mods.extend(small);
    mods
}

fn refresh_mods() {
    let mods = build_mods();
    let items: Vec<DropItem> = drop_items(mods.iter().map(|m| m.label.clone()));
    local(|l| l.mods = mods);
    if CONTROLS.with_borrow(|m| m.contains_key("nodeSelect")) {
        *ctl::<DropDownControl>("nodeSelect").list.borrow_mut() = items;
    }
}

/// `getNodeWeights()` (:1413-1425).
fn node_weights(g: &Gfx) -> [TCell; 3] {
    let read = |n: &str| {
        let l = ctl::<LabelControl>(n).label.get(g);
        let s = l.get(2..).unwrap_or("").to_lowercase();
        match pob_lua::tonumber::tonumber(&s) {
            Some(v) => TCell::Num(pob_lua::number::round(v, Some(3.0))),
            None => TCell::Str(s),
        }
    };
    [read("nodeSliderValue"), read("nodeSlider2Value"), read("nodeSlider3Value")]
}

fn parse_rows(text: &str) -> Vec<Vec<TCell>> {
    let is_sep = |c: char| c == ',' || (c.is_ascii_whitespace() || c == '\u{b}');
    text.split(['\r', '\n'])
        .filter(|l| !l.is_empty())
        .map(|line| {
            line.split(is_sep)
                .filter(|s| !s.is_empty())
                .map(|s| TCell::Str(s.to_string()))
                .collect()
        })
        .collect()
}

/// `parseSearchList(0, fallback)` (:1430-1453).
fn parse_from_text(fallback: bool) {
    let text = td(|t| {
        if fallback {
            t.search_list_fallback.clone()
        } else {
            t.search_list.clone()
        }
    });
    if let Some(text) = text {
        let rows = parse_rows(&text);
        local(|l| if fallback { l.fallback = rows } else { l.search = rows });
    }
}

/// `parseSearchList(1, fallback)` (:1455-1500).
fn parse_to_text(g: &mut Gfx, fallback: bool) {
    let (edit, select) = (if fallback { "searchListFallback" } else { "searchList" }, "nodeSelect");
    if !CONTROLS.with_borrow(|m| m.contains_key(edit) && m.contains_key(select)) {
        return;
    }
    let sel = ctl::<DropDownControl>(select).sel_index.get();
    let sel_id = local(|l| l.mods.get(sel.wrapping_sub(1)).and_then(|m| m.id.clone()));
    let mut text = String::new();
    let rows = local(|l| if fallback { l.fallback.clone() } else { l.search.clone() });
    let mut out = rows;
    for row in &mut out {
        if row.first().map(TCell::text) == sel_id {
            let [a, b, c] = node_weights(g);
            row.resize(row.len().max(4), TCell::Num(0.0));
            row[1] = a;
            row[2] = b;
            row[3] = c;
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&row.iter().map(TCell::text).collect::<Vec<_>>().join(", "));
    }
    local(|l| if fallback { l.fallback = out } else { l.search = out });
    let same = td(|t| if fallback { t.search_list_fallback.as_deref() } else { t.search_list.as_deref() } == Some(text.as_str()));
    if !same {
        td(|t| {
            if fallback {
                t.search_list_fallback = Some(text.clone());
            } else {
                t.search_list = Some(text.clone());
            }
        });
        ctl::<EditControl>(edit).set_text(g, &text, false);
        mod_flag();
    }
}

/// `updateSearchList(text, fallback)` (:1504-1514).
fn update_search_list(g: &mut Gfx, text: &str, fallback: bool) {
    td(|t| {
        if fallback {
            t.search_list_fallback = Some(text.to_string());
        } else {
            t.search_list = Some(text.to_string());
        }
    });
    let edit = if fallback { "searchListFallback" } else { "searchList" };
    ctl::<EditControl>(edit).set_text(g, text, false);
    parse_from_text(fallback);
    mod_flag();
}

fn fallback_shown(g: &Gfx) -> bool {
    CONTROLS.with_borrow(|m| m.contains_key("searchListFallback")) && ctl::<EditControl>("searchListFallback").base.shown.get(g)
}

// --- the protected nodes -----------------------------------------------------------------------------------------------------------------

/// `buildNodeOptionCheckboxes(nodes)` (:1537-1563).
fn build_node_option_checkboxes(g: &mut Gfx, nodes: Option<Vec<(String, u32)>>) {
    let old = local(|l| {
        l.protected.clear();
        l.protected_count = 0;
        std::mem::take(&mut l.box_count)
    });
    for i in 1..=old {
        unreg(&format!("protectAllocatedBox{i}"));
    }
    let Some(nodes) = nodes else { return };
    for (i, (label, node_id)) in nodes.iter().enumerate() {
        let i = i + 1;
        let prev = if i > 1 {
            format!("protectAllocatedBox{}", i - 1)
        } else {
            "protectAllocatedLabel".to_string()
        };
        let prev = get(&prev);
        let name = label.clone();
        let cb = CheckBoxControl::new(
            g,
            anchor("TOPLEFT", &prev, "BOTTOMLEFT"),
            [0.0, 4.0, 18.0],
            Some(label),
            Some(Rc::new(move |_g: &mut Gfx, state| {
                local(|l| {
                    l.protected.insert(name.clone(), state);
                    l.protected_count += if state { 1 } else { -1 };
                });
            })),
            None,
            false,
        );
        cb.label_right.set(true);
        let id = *node_id;
        cb.tip.set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, _| {
            tooltip.clear(false);
            tree_node_tooltip(tooltip, g, id);
        });
        reg(&format!("protectAllocatedBox{i}"), cb);
        local(|l| l.box_count = i);
    }
}

/// `self.viewer:AddNodeTooltip(tooltip, node, self.build, true)` of a node of the tree.
fn tree_node_tooltip(tooltip: &mut Tooltip, g: &Gfx, node_id: u32) {
    let b = main().build_mode.clone();
    let root = crate::launch::launch().cfg.script_path.clone();
    let mut tv = b.tree_view.borrow_mut();
    let Some(view) = tv.as_mut() else { return };
    let eng = b.engine.borrow();
    let e = eng.as_ref().expect("engine");
    let spec = e.spec_rc();
    let art = view.art(&root, &spec.tree);
    crate::tree_tooltip::plain_node_tooltip(view, tooltip, g, e, &art, &spec, node_id);
}

/// `setAllocatedNodes` (:1602-1634).
fn set_allocated_nodes(g: &mut Gfx) {
    local(|l| l.allocated_in_radius = 0);
    let socket = td(|t| t.jewel_socket_id).unwrap_or(-1.0);
    let tree = legion();
    if socket == -1.0 || tree.node(socket as u32).is_none() {
        build_node_option_checkboxes(g, None);
        return;
    }
    let spec = engine_read(pob_engine::engine::Engine::spec_rc);
    let granted = engine_read(|e| e.granted_passives_rc());
    let mut options: Vec<(String, u32)> = Vec::new();
    if jewel_id() == 11 {
        for id in spec.alloc_table_pairs() {
            if let Some(base) = tree.node(id)
                && base.ascendancy_name.as_deref() == Some(spec.cur_ascend_class_name.as_str())
                && base.ty == pob_tree::NodeType::Notable
            {
                options.push((base.dn.clone(), id));
            }
        }
    } else {
        let radius = tree.nodes_in_radius(socket as u32).map(|r| r[2].clone()).unwrap_or_default();
        for id in radius {
            if (granted.contains_key(&id) || spec.node(id).is_some_and(|n| n.alloc))
                && let Some(base) = tree.node(id).filter(|b| b.ty == pob_tree::NodeType::Notable)
            {
                options.push((base.dn.clone(), id));
            }
        }
    }
    pob_engine::lua::lua_sort(&mut options, &mut |a: &(String, u32), b: &(String, u32)| {
        a.0.as_bytes() < b.0.as_bytes()
    });
    let n = options.len() as i64;
    build_node_option_checkboxes(g, Some(options));
    local(|l| l.allocated_in_radius = n);
}

// --- TimelessJewelListControl ------------------------------------------------------------------------------------------------------------

struct ListH;

impl ListHooks<Row> for ListH {
    fn get_row_value(&self, _l: &ListControl<Row>, _g: &Gfx, column: usize, _i: usize, v: &Row) -> String {
        if column == 1 { v.0.label.borrow().clone() } else { String::new() }
    }

    fn has_add_value_tooltip(&self) -> bool {
        true
    }

    /// TimelessJewelListControl.lua:291-333
    fn add_value_tooltip(&self, _l: &ListControl<Row>, g: &Gfx, tooltip: &mut Tooltip, _i: usize, v: &Row) {
        let rev = engine_read(pob_engine::engine::Engine::output_revision);
        if !tooltip.check_for_update((rev, v.0.seed.to_bits(), v.0.socket_id.map(f64::to_bits))) {
            return;
        }
        tooltip.clear(false);
        let t = tab();
        let shared = t.timeless.shared.borrow();
        if v.0.label.borrow().contains("B2B2B2") {
            tooltip.add_line(
                g,
                16.0,
                Some(&format!(
                    "^7{} {} was successfully added to your build.",
                    jewel_label(shared.jewel),
                    ts(v.0.seed)
                )),
            );
        } else {
            tooltip.add_line(g, 16.0, Some("^7Double click to add this jewel to your build."));
            if td(|t| t.socket_allocate) {
                let socket_id = v.0.socket_id.or(shared.socket.as_ref().map(|s| s.id));
                let allocated = socket_id.is_some_and(|id| engine_read(|e| e.spec_rc().node(id as u32).is_some_and(|n| n.alloc)));
                if allocated {
                    let label =
                        v.0.socket_label
                            .clone()
                            .or(shared.socket.as_ref().map(|s| s.label.clone()))
                            .unwrap_or_default();
                    tooltip.add_line(g, 16.0, Some(&format!("^7It will be socketed into {label}.")));
                } else {
                    tooltip.add_line(
                        g,
                        16.0,
                        Some(&format!(
                            "{}That jewel socket is not allocated, so the jewel will only be added to your items.",
                            crate::colors::color_code("WARNING").unwrap_or_default()
                        )),
                    );
                }
            }
        }
        let mut sorted: Vec<(usize, String)> = Vec::new();
        for (id, d) in &shared.desired {
            if let Some(names) = v.0.targets.get(id) {
                if names.is_empty() {
                    sorted.push((d.idx, format!("^7        {}:\n^8                None", d.display)));
                } else {
                    sorted.push((
                        d.idx,
                        format!("^7        {}:\n^8                {}", d.display, names.join("\n                ")),
                    ));
                }
            }
        }
        sorted.sort_by_key(|x| x.0);
        if !sorted.is_empty() {
            tooltip.add_line(g, 16.0, Some("^7Node List:"));
            for (_, s) in sorted {
                tooltip.add_line(g, 16.0, Some(&s));
            }
        }
        if v.0.total > 0.0 {
            tooltip.add_line(g, 16.0, Some(&format!("^7Combined Node Weight: {}", ts(v.0.total))));
        }
        // :330-331 `itemsTab:AddItemStatDifferences(tooltip, jewel, jewel.base, socket)`
        let tt = tab();
        let text = {
            let s = tt.timeless.shared.borrow();
            jewel_item_text(&v.0, &s)
        };
        let b = main().build_mode.clone();
        let items = b.items_tab.borrow().clone();
        if let Some(items) = items {
            let item = Rc::new(pob_items::Item::parse_with(&text, None, false));
            if item.base.is_some() {
                let socket = v.0.socket_id.or_else(|| tt.timeless.shared.borrow().socket.as_ref().map(|s| s.id)).filter(|i| *i != -1.0);
                items.add_stat_differences(tooltip, g, &item, socket.map(|i| i as u32));
            }
        }
    }

    /// :335-352
    fn on_sel_click(&self, _l: &ListControl<Row>, g: &mut Gfx, _i: usize, v: &Row, double_click: bool) -> Option<bool> {
        if double_click && !v.0.label.borrow().contains("B2B2B2") {
            let t = tab();
            let text = {
                let s = t.timeless.shared.borrow();
                jewel_item_text(&v.0, &s)
            };
            // :338-339 `itemsTab:AddItem(item, true)`: no recalculation here
            let added = {
                let b = main().build_mode.clone();
                let mut eng = b.engine.borrow_mut();
                eng.as_mut().expect("engine").add_item(&pob_items::Item::parse_with(&text, None, false), None)
            };
            let added = added.unwrap_or_else(|e| panic!("{e}"));
            let b = main().build_mode.clone();
            let items = b.items_tab.borrow().clone();
            if td(|t| t.socket_allocate)
                && let Some(items) = &items
            {
                // :340-347
                let socket = v.0.socket_id.or_else(|| t.timeless.shared.borrow().socket.as_ref().map(|s| s.id)).filter(|i| *i != -1.0);
                if let Some(n) = socket {
                    let n = n as u32;
                    let slot = items.sockets.borrow().get(&n).cloned();
                    let ok = slot.as_ref().is_some_and(|s| {
                        items.read(|e| {
                            e.spec_rc().alloc_nodes.contains_key(&n) && e.item_by_id(added).is_some_and(|i| e.is_item_valid_for_slot(&i, &s.slot_name))
                        })
                    });
                    if let (Some(s), true) = (slot, ok) {
                        s.set_sel_item_id(added);
                        b.build_flag.set(true);
                    }
                }
            }
            if let Some(items) = &items {
                items.populate_slots(g);
                items.add_undo_state(false);
                items.sync_item_rows();
            }
            let l = format!("^xB2B2B2{}", v.0.label.borrow());
            *v.0.label.borrow_mut() = l;
        }
        None
    }

    /// :30-47
    fn set_highlight_color(&self, l: &ListControl<Row>, f: &mut Frame<'_>, index: usize, _v: &Row) -> bool {
        let (Some(h), Some(s)) = (local(|x| x.highlight.get()), l.sel_index.get()) else {
            return false;
        };
        if s.min(h) <= index && s.max(h) >= index {
            if s == index || h == index {
                sg::set_draw_color(f, 1.0, 0.5, 0.0);
            } else {
                sg::set_draw_color(f, 1.0, 1.0, 0.0);
            }
            return true;
        }
        false
    }

    /// :55-64
    fn override_select_index(&self, l: &ListControl<Row>, g: &Gfx, index: usize) -> bool {
        if sg::is_key_down(g, "SHIFT") && l.sel_index.get().is_some() {
            local(|x| x.highlight.set(Some(index)));
            return true;
        }
        local(|x| x.highlight.set(None));
        false
    }
}

/// `GetJewelItem(data)` (:73-286): the item's text.
fn jewel_item_text(data: &ResultRow, shared: &Shared) -> String {
    let socket_info = data
        .socket_label
        .clone()
        .or(shared.socket.as_ref().map(|s| s.keystone.clone()))
        .unwrap_or_else(|| "Unknown".into());
    let seed = ts(data.seed);
    let label = format!("[{seed}; {}; {socket_info}]\n", ts(data.total));
    // :229-283
    let abyss = |head: &str, base: &str, line: String| {
        format!(
            "{head} {label}{base} Eye Jewel\nLeague: Allflame\nLimited to: 1 Historic\nImplicits: 0\n{line}\nPassives affected are Conquered by the Abyssal\nHistoric\n"
        )
    };
    let thrall = |name: &str| format!("Subjugating {seed} souls in the thrall of {name}");
    match shared.jewel {
        7 => return abyss("Festering Vengeance", "Murderous", thrall("Tecrod")),
        8 => return abyss("Extinguishing Grasp", "Searching", thrall("Ulaman")),
        9 => return abyss("Baleful Dominion", "Hypnotic", thrall("Kurgal")),
        10 => return abyss("Destructive Aspiration", "Ghastly", thrall("Amanamu")),
        11 => {
            return abyss(
                "Reclaimed Malevolence",
                "Assembled",
                format!("Binding {seed} souls to phylacteries to sustain Zorath"),
            );
        }
        _ => {}
    }
    let variant = if shared.conqueror == 1 {
        "1".to_string()
    } else {
        format!("{}\n", shared.conqueror - 1)
    };
    let three = |head: &str, names: [&str; 3], tail: [String; 3], conquered: &str| {
        format!(
            "{head} {label}Timeless Jewel\nLeague: Legion\nLimited to: 1 Historic\nVariant: {}\nVariant: {}\nVariant: {}\nSelected Variant: {variant}\nRadius: Large\nImplicits: 0\n{{variant:1}}{}\n{{variant:2}}{}\n{{variant:3}}{}\nPassives in radius are Conquered by the {conquered}\nHistoric\n",
            names[0], names[1], names[2], tail[0], tail[1], tail[2]
        )
    };
    match shared.jewel {
        1 => three(
            "Glorious Vanity",
            ["Doryani (Corrupted Soul)", "Xibaqua (Divine Flesh)", "Ahuana (Immortal Ambition)"],
            ["Doryani", "Xibaqua", "Ahuana"].map(|n| format!("Bathed in the blood of {seed} sacrificed in the name of {n}")),
            "Vaal",
        ),
        2 => three(
            "Lethal Pride",
            ["Kaom (Strength of Blood)", "Rakiata (Tempered by War)", "Akoya (Chainbreaker)"],
            ["Kaom", "Rakiata", "Akoya"].map(|n| format!("Commanded leadership over {seed} warriors under {n}")),
            "Karui",
        ),
        3 => three(
            "Brutal Restraint",
            ["Asenath (Dance with Death)", "Nasima (Second Sight)", "Balbala (The Traitor)"],
            ["Asenath", "Nasima", "Balbala"].map(|n| format!("Denoted service of {seed} dekhara in the akhara of {n}")),
            "Maraketh",
        ),
        4 => {
            // :133-139 `m_random(2, 17)` where no variant was chosen: a random pick upstream too
            let pick = |id: usize, kept: Option<usize>, salt: u64| {
                if id != 1 {
                    id
                } else {
                    kept.unwrap_or(2 + ((data.seed as u64).wrapping_mul(31).wrapping_add(salt) % 16) as usize)
                }
            };
            let mut alt = pick(shared.devotion1, data.alt1.get(), 1);
            let alt2 = pick(shared.devotion2, data.alt2.get(), 2);
            if alt == alt2 {
                alt = alt % 16 + 2;
            }
            data.alt1.set(Some(alt));
            data.alt2.set(Some(alt2));
            format!(
                "Militant Faith {label}Timeless Jewel\nLeague: Legion\nLimited to: 1 Historic\nHas Alt Variant: true\nHas Alt Variant Two: true\nVariant: Avarius (Power of Purpose)\nVariant: Dominus (Inner Conviction)\nVariant: Maxarius (Transcendence)\nVariant: Totem Damage\nVariant: Brand Damage\nVariant: Channelling Damage\nVariant: Area Damage\nVariant: Elemental Damage\nVariant: Elemental Resistances\nVariant: Effect of non-Damaging Ailments\nVariant: Elemental Ailment Duration\nVariant: Duration of Curses\nVariant: Minion Attack and Cast Speed\nVariant: Minions Accuracy Rating\nVariant: Mana Regen\nVariant: Mana Cost (legacy)\nVariant: Non-Curse Aura Effect\nVariant: Defences from Shield\nVariant: Mana Cost Efficiency\nSelected Variant: {variant}\nSelected Alt Variant: {}\nSelected Alt Variant Two: {}\nRadius: Large\nImplicits: 0\n{{variant:1}}Carved to glorify {seed} new faithful converted by High Templar Avarius\n{{variant:2}}Carved to glorify {seed} new faithful converted by High Templar Dominus\n{{variant:3}}Carved to glorify {seed} new faithful converted by High Templar Maxarius\n{{variant:4}}4% increased Totem Damage per 10 Devotion\n{{variant:5}}4% increased Brand Damage per 10 Devotion\n{{variant:6}}Channelling Skills deal 4% increased Damage per 10 Devotion\n{{variant:7}}4% increased Area Damage per 10 Devotion\n{{variant:8}}4% increased Elemental Damage per 10 Devotion\n{{variant:9}}+2% to all Elemental Resistances per 10 Devotion\n{{variant:10}}3% increased Effect of non-Damaging Ailments on Enemies per 10 Devotion\n{{variant:11}}4% reduced Elemental Ailment Duration on you per 10 Devotion\n{{variant:12}}4% reduced Duration of Curses on you per 10 Devotion\n{{variant:13}}1% increased Minion Attack and Cast Speed per 10 Devotion\n{{variant:14}}Minions have +60 to Accuracy Rating per 10 Devotion\n{{variant:15}}Regenerate 0.6 Mana per Second per 10 Devotion\n{{variant:16}}1% reduced Mana Cost of Skills per 10 Devotion\n{{variant:17}}1% increased effect of Non-Curse Auras per 10 Devotion\n{{variant:18}}3% increased Defences from Equipped Shield per 10 Devotion\n{{variant:19}}3% increased Mana Cost Efficiency per 10 Devotion\nPassives in radius are Conquered by the Templars\nHistoric\n",
                alt + 2,
                alt2 + 2
            )
        }
        5 => three(
            "Elegant Hubris",
            [
                "Cadiro (Supreme Decadence)",
                "Victario (Supreme Grandstanding)",
                "Caspiro (Supreme Ostentation)",
            ],
            ["Cadiro", "Victario", "Caspiro"].map(|n| format!("Commissioned {seed} coins to commemorate {n}")),
            "Eternal Empire",
        ),
        _ => three(
            "Heroic Tragedy",
            [
                "Vorana (Black Scythe Training)",
                "Uhtred (Celestial Mathematics)",
                "Medved (The Unbreaking Circle)",
            ],
            ["Vorana", "Uhtred", "Medved"].map(|n| format!("Remembrancing {seed} songworthy deeds by the line of {n}")),
            "Kalguur",
        ),
    }
}

// --- the search --------------------------------------------------------------------------------------------------------------------------

#[derive(Default, Clone)]
struct NodeResult {
    names: Vec<String>,
    total_weight: f64,
}

struct SearchResult {
    /// `pairs(resultNodes)` order; `None`: `resultNodes[seed] = nil` (the key stays out of `pairs`).
    seeds: Vec<(f64, IndexMap<String, NodeResult>)>,
    weights: HashMap<u64, f64>,
    desired: IndexMap<String, Desired>,
}

/// `searchSingleSocket(socketId, socketInfo)` (:2430-2717).
#[allow(clippy::too_many_lines)]
fn search_single_socket(g: &mut Gfx, socket_id: f64) -> Option<SearchResult> {
    let tree = legion();
    let tn = tree.node(socket_id as u32)?;
    if !tn.is_jewel_socket {
        return None;
    }
    let jewel = jewel_id();
    let is_abyss = jewel >= 7;
    let spec = engine_read(pob_engine::engine::Engine::spec_rc);
    // :2437-2440
    let zorath_path = if jewel == 11 {
        Some(spec.shortest_path_to_class_start(tn.id)?)
    } else {
        None
    };
    let radius_nodes: Vec<u32> = RADIUS_ORDER_SAMPLE
        .with_borrow(|s| s.as_ref().and_then(|m| m.get(&tn.id).cloned()))
        .unwrap_or_else(|| {
            pob_tree::lj_table::pairs_order(
                &tree
                    .nodes_in_radius(tn.id)
                    .map(|r| r[2].iter().map(|&i| f64::from(i)).collect::<Vec<_>>())
                    .unwrap_or_default(),
            )
            .into_iter()
            .map(|i| i as u32)
            .collect()
        });
    let leg = tree.legion().expect("legion");
    let (search, fallback, protected, protected_count, allocated_in_radius) = local(|l| {
        (
            l.search.clone(),
            l.fallback.clone(),
            l.protected.clone(),
            l.protected_count,
            l.allocated_in_radius,
        )
    });
    let mut combined: Vec<Vec<TCell>> = Vec::new();
    let mut found: Vec<String> = Vec::new();
    for row in &search {
        if let Some(f) = row.first() {
            found.push(f.text());
        }
        combined.push(row.clone());
    }
    for row in &fallback {
        if row.first().is_some_and(|f| !found.contains(&f.text())) {
            combined.push(row.clone());
        }
    }
    let mut desired: IndexMap<String, Desired> = IndexMap::new();
    let mut minimum: Vec<(String, f64)> = Vec::new();
    let mut desired_idx = 0;
    let n_combined = combined.len() as i64;
    let total = total_mod(jewel);
    for mut node in combined {
        if node.len() <= 1 {
            continue;
        }
        let mut display: Option<String> = None;
        let mut single_stat = false;
        if let Some(t) = total
            && node[0].text() == format!("total_{}", t.to_lowercase())
        {
            node[0] = TCell::Str("totalStat".into());
            display = Some(t.to_string());
        }
        let key = node[0].text();
        if display.is_none()
            && let Some(l) = leg.nodes.iter().find(|n| n.id == key)
        {
            if jewel > 1 && jewel < 7 {
                single_stat = true;
            }
            display = Some(l.sd.join(" + "));
        }
        if display.is_none()
            && let Some(a) = leg.additions.iter().find(|n| n.id == key)
        {
            single_stat = jewel < 7;
            display = Some(a.sd.join(" + "));
        }
        let Some(display) = display else { continue };
        // :2494-2504: `ipairs` sees the table as it changes
        let mut i = 1;
        while i <= node.len() {
            let val = node[i - 1].clone();
            if single_stat && i == 2 {
                let v = node[1].tonumber().or(node.get(2).and_then(TCell::tonumber)).unwrap_or(1.0);
                node[1] = TCell::Num(v);
            }
            if val == TCell::Str("required".into()) {
                node[i - 1] = if single_stat && i == 2 { node[1].clone() } else { TCell::Num(0.0) };
                match node.get(3) {
                    None => {
                        node.resize(3, TCell::Num(0.0));
                        node.push(TCell::Num(0.001));
                    }
                    Some(TCell::Num(n)) => {
                        if *n < 0.001 {
                            node[3] = TCell::Num(0.001);
                        }
                    }
                    Some(TCell::Str(_)) => panic!("Classes/TreeTab.lua:2500: attempt to compare string with number"),
                }
            }
            i += 1;
        }
        if let Some(c) = node.get(3) {
            let v = c
                .tonumber()
                .unwrap_or_else(|| panic!("Classes/TreeTab.lua:2505: attempt to compare nil with number"));
            if v > 0.0 {
                minimum.push((key.clone(), v));
            }
        }
        if jewel == 4 && protected_count > 0 && protected_count < allocated_in_radius && n_combined < allocated_in_radius {
            minimum.push((key.clone(), 1.0));
        }
        let key = node[0].text();
        let weight = node[1].tonumber().unwrap_or(0.001);
        let weight2 = node.get(2).and_then(TCell::tonumber).unwrap_or(0.001);
        let idx = if let Some(d) = desired.get(&key) {
            d.idx
        } else {
            desired_idx += 1;
            desired_idx
        };
        desired.insert(
            key,
            Desired {
                weight,
                weight2,
                display,
                idx,
            },
        );
    }
    let root_nodes: Vec<u32> = tree.classes.iter().filter_map(|c| c.start_node_id).collect();
    let filter = ctl::<CheckBoxControl>("socketFilter").state.get();
    let granted = engine_read(|e| e.granted_passives_rc());
    let mut allocated: HashMap<u32, bool> = HashMap::new();
    let mut unalloc_distance: HashMap<u32, f64> = HashMap::new();
    if filter && !is_abyss {
        let d = td(|t| t.socket_filter_distance);
        for &id in &radius_nodes {
            allocated.insert(id, granted.contains_key(&id) || spec.node(id).is_some_and(|n| n.alloc));
            if d > 0.0 {
                unalloc_distance.insert(id, spec.node(id).map_or(1000.0, |n| n.path_dist));
            }
        }
    }
    let dist = td(|t| t.socket_filter_distance);
    let mut target_nodes: Vec<u32> = Vec::new();
    let (mut attribute_smalls, mut other_smalls) = (0.0, 0.0);
    for &id in radius_nodes.iter().filter(|_| !is_abyss) {
        let n = tree.node(id).expect("radius node");
        if !root_nodes.contains(&id)
            && !n.is_jewel_socket
            && n.ty != pob_tree::NodeType::Keystone
            && (!filter
                || allocated.get(&id).copied().unwrap_or(false)
                || (dist > 0.0 && unalloc_distance.get(&id).copied().unwrap_or(1000.0) <= dist))
        {
            if n.ty == pob_tree::NodeType::Notable || jewel == 1 {
                target_nodes.push(id);
            } else if desired.contains_key("totalStat") {
                if ["Strength", "Intelligence", "Dexterity"].contains(&n.dn.as_str()) {
                    attribute_smalls += 1.0;
                } else {
                    other_smalls += 1.0;
                }
            }
        }
    }
    let target_nodes: Vec<u32> = pob_tree::lj_table::pairs_order(&target_nodes.iter().map(|&i| f64::from(i)).collect::<Vec<_>>())
        .into_iter()
        .map(|i| i as u32)
        .collect();
    let (_, seed_min, seed_max) = pob_data::timeless::timeless_jewel_type(jewel).expect("jewel type");
    let additions = pob_data::timeless::timeless_jewel_additions() as u32;
    let mult = if jewel == 5 { 20.0 } else { 1.0 };
    let mut table = pob_tree::lj_table::LjTable::new();
    let mut seeds: HashMap<u64, IndexMap<String, NodeResult>> = HashMap::new();
    let mut weights: HashMap<u64, f64> = HashMap::new();
    let targets: Vec<(u32, &str, bool)> = target_nodes
        .iter()
        .map(|&id| {
            let n = tree.node(id).expect("target");
            (id, n.name.as_deref().unwrap_or(""), protected.get(&n.dn).copied().unwrap_or(false))
        })
        .collect();
    let mut cur_seed = seed_min * mult;
    let last = seed_max * mult;
    while cur_seed <= last {
        let mut w = 0.0;
        let mut res: Option<IndexMap<String, NodeResult>> = Some(IndexMap::new());
        table.set(cur_seed);
        let add = |res: &mut IndexMap<String, NodeResult>, id: &str, weight: f64, name: &str, w: &mut f64| {
            if !res.contains_key(id) {
                res.insert(id.to_string(), NodeResult::default());
            }
            let e = res.get_mut(id).expect("just made");
            e.total_weight += weight;
            e.names.push(name.to_string());
            *w += weight;
        };
        if is_abyss {
            // :2572-2609 Abyss files name the affected passives directly
            let affected =
                pob_data::timeless::read_abyss_jewel_lut(cur_seed, tn.id, jewel, zorath_path.as_deref(), Some(&spec.cur_ascend_class_name))
                    .unwrap_or_else(|e| panic!("{}", e.0));
            let keys: Vec<f64> = affected.keys().map(|&k| f64::from(k)).collect();
            for target in pob_tree::lj_table::pairs_order(&keys) {
                let target = target as u32;
                let tree_node = tree.node(target);
                let mut score =
                    tree_node.is_some_and(|n| !root_nodes.contains(&target) && !n.is_jewel_socket && n.ty != pob_tree::NodeType::Keystone);
                let Some(treen) = tree_node.filter(|_| score) else { continue };
                if treen.ascendancy_name.is_some() && protected.get(&treen.dn).copied().unwrap_or(false) {
                    res = None;
                    break;
                }
                if filter && zorath_path.is_none() {
                    let is_allocated = granted.contains_key(&target) || spec.alloc_nodes.contains_key(&target);
                    let distance = spec
                        .node(target)
                        .unwrap_or_else(|| panic!("Classes/TreeTab.lua:2585: attempt to index a nil value (spec.nodes[{target}])"))
                        .path_dist;
                    score = is_allocated || (dist > 0.0 && distance <= dist);
                }
                if !score {
                    continue;
                }
                let name = treen.name.as_deref().unwrap_or("");
                for component in &affected[&target] {
                    // :2590 `data.resolveAbyssJewelComponent(component, treeData.legion)`
                    let changed: Option<(&str, &pob_data::wire::OrderedMap<pob_data::timeless::LegionStat>)> = match component.ty {
                        1 => (component.id + 1)
                            .checked_sub(additions + 1)
                            .and_then(|i| leg.nodes.get(i as usize))
                            .map(|n| (n.id.as_str(), n.stat_defs)),
                        2 => leg.additions.get(component.id as usize).map(|a| (a.id.as_str(), a.stats)),
                        _ => None,
                    };
                    let Some((changed_id, stats)) = changed else { continue };
                    let Some(d) = desired.get(changed_id) else { continue };
                    let (_, roll1) = pob_data::timeless::abyss_component_roll(component, stats, 1);
                    let (stat2, roll2) = pob_data::timeless::abyss_component_roll(component, stats, 2);
                    let mut weight = d.weight * roll1.unwrap_or(1.0);
                    if stat2.is_some() {
                        weight += d.weight2 * roll2.unwrap_or(1.0);
                    }
                    add(res.as_mut().expect("seed result"), changed_id, weight, name, &mut w);
                }
            }
        }
        for &(target, name, is_protected) in &targets {
            let data = pob_data::timeless::read_lut(cur_seed, target, jewel).unwrap_or_else(|e| panic!("{}", e.0));
            if data.is_empty() {
                g.con_printf(&format!("Missing LUT: {}", jewel_label(jewel)));
                continue;
            }
            if jewel == 4 && is_protected {
                if data[0] >= additions {
                    res = None;
                    break;
                }
                if !desired.contains_key("totalStat") {
                    desired.insert(
                        "totalStat".into(),
                        Desired {
                            weight: 0.1,
                            weight2: 0.0,
                            display: "Devotion".into(),
                            idx: desired_idx + 1,
                        },
                    );
                }
            }
            let replace = data[0] >= additions && !is_protected;
            let cur_node = if replace {
                leg.nodes.get((data[0] - additions) as usize)
            } else {
                None
            };
            let cur_add = if replace { None } else { leg.additions.get(data[0] as usize) };
            let mut cur_id: Option<&str> = cur_node.map(|n| n.id.as_str()).or(cur_add.map(|n| n.id.as_str()));
            if cur_id.is_some_and(|i| REVERSE_TOTAL_MOD_IDS.contains(&i)) && desired.contains_key("totalStat") {
                cur_id = Some("totalStat");
            }
            let r = res.as_mut().expect("seed result");
            let weights_of = |id: Option<&str>| id.and_then(|i| desired.get(i)).map(|d| (d.weight, d.weight2));
            if jewel == 1 {
                let header = data.len();
                if header == 2 || header == 3 {
                    if let Some((d1, d2)) = weights_of(cur_id) {
                        let (stats, sorted) = match (cur_node, cur_add) {
                            (Some(n), _) => (n.stat_defs, &n.sorted_stats),
                            (_, Some(a)) => (a.stats, &a.sorted_stats),
                            _ => panic!("Classes/TreeTab.lua:2648: attempt to index local 'curNode' (a nil value)"),
                        };
                        let roll = |k: &String| f64::from(data[stats.get(k).expect("stat").index as usize]);
                        let mut weight = d1 * roll(&sorted[0]);
                        if let Some(k2) = sorted.get(1)
                            && stats.get(k2).is_some()
                        {
                            weight += d2 * roll(k2);
                        }
                        add(r, cur_id.expect("id"), weight, name, &mut w);
                    }
                } else if header == 6 || header == 8 {
                    for i in 0..header / 2 {
                        let id = leg.additions.get(data[i] as usize).map(|a| a.id.as_str());
                        if let Some((d1, _)) = weights_of(id) {
                            add(r, id.expect("id"), d1 * f64::from(data[i + header / 2]), name, &mut w);
                        }
                    }
                }
            } else if let Some((d1, _)) = weights_of(cur_id) {
                add(r, cur_id.expect("id"), d1, name, &mut w);
            }
        }
        if let (Some(r), Some(d)) = (res.as_mut(), desired.get("totalStat")) {
            let e = r.entry("totalStat".into()).or_default();
            let mut added = if jewel == 4 {
                d.weight * (5.0 * other_smalls + 10.0 * attribute_smalls)
            } else {
                d.weight * (4.0 * other_smalls + 2.0 * attribute_smalls)
            };
            added += e.total_weight * if jewel == 4 { 4.0 } else { 19.0 };
            e.total_weight += added;
            w += added;
        }
        if let Some(r) = &res
            && minimum
                .iter()
                .any(|(id, weight)| r.get(id).map_or(0.0, |e| e.total_weight) < *weight)
        {
            res = None;
        }
        weights.insert(cur_seed.to_bits(), w);
        match res {
            Some(r) => {
                seeds.insert(cur_seed.to_bits(), r);
            }
            None => table.remove(cur_seed, false),
        }
        cur_seed += mult;
    }
    let order = table.pairs_keys();
    let seeds = order
        .into_iter()
        .filter_map(|s| seeds.remove(&s.to_bits()).map(|r| (s, r)))
        .collect();
    Some(SearchResult { seeds, weights, desired })
}

/// `formatSearchValue(input)` (:2719-2733).
fn format_search_value(input: f64) -> String {
    let s = format!(" {}", fmt("%006.2f", input));
    let sub = |s: Vec<u8>, p: &str, r: &str| {
        pob_lua::gsub(&s, p.as_bytes(), pob_lua::Repl::Str(r.as_bytes()), None)
            .expect("pattern")
            .0
    };
    let mut b = s.into_bytes();
    b = sub(b, " 0", "   ");
    b = sub(b, " 0", "   ");
    b = sub(b, ".0 ", "    ");
    b = sub(b, ".0 ", "    ");
    b = sub(b, "  %.", "0.");
    b = sub(b, "%.([0-9])0", ".%1  ");
    String::from_utf8_lossy(&b).into_owned()
}

/// `formatResults(...)` (:2735-2788).
fn format_results(r: &SearchResult, socket: Option<&JewelSocket>) -> Vec<Row> {
    let jewel = jewel_id();
    let min = tab().timeless.total_minimum_weight.get();
    let mut out = Vec::new();
    for (seed, data) in &r.seeds {
        let w = r.weights[&seed.to_bits()];
        if !(w > 0.0 && min.is_none_or(|m| w >= m)) {
            continue;
        }
        let prefix = socket.map_or_else(String::new, |s| format!("{} | ", s.label));
        let mut label = format!("{prefix}{}:", ts(*seed));
        if jewel == 1 || jewel == 3 || jewel >= 7 {
            if *seed < 1000.0 {
                label = format!("  {label}");
            }
        } else if jewel == 4 {
            if *seed < 10000.0 {
                label = format!("  {label}");
            }
        } else if *seed < 10000.0 {
            label = format!("    {label}");
        } else if *seed < 100_000.0 {
            label = format!("  {label}");
        }
        let mut sorted: Vec<Option<String>> = vec![None; 8];
        let mut targets = IndexMap::new();
        for (id, d) in &r.desired {
            if let Some(e) = data.get(id) {
                if d.idx == 8 {
                    sorted[7] = Some(" ...".into());
                } else if d.idx < 8 {
                    sorted[d.idx - 1] = Some(format_search_value(e.total_weight));
                }
                targets.insert(id.clone(), e.names.clone());
            } else if d.idx < 8 {
                sorted[d.idx - 1] = Some("     0     ".into());
            }
        }
        for s in sorted {
            match s {
                Some(s) => label.push_str(&s),
                None => break,
            }
        }
        out.push(ByRef(Rc::new(ResultRow {
            label: RefCell::new(label),
            seed: *seed,
            total: w,
            socket_id: socket.map(|s| s.id),
            socket_label: socket.map(|s| s.label.clone()),
            targets,
            alt1: Cell::new(None),
            alt2: Cell::new(None),
        })));
    }
    out
}

fn after_search() {
    let t = tab();
    let list = ctl::<ListControl<Row>>("searchResults");
    *list.list.borrow_mut() = t.timeless.results.borrow().clone();
    let n = list.list.borrow().len();
    let trade = ctl::<ButtonControl>("searchTradeButton");
    trade.base.enabled.set(n > 0);
    trade.label.set("Open Trade URL".to_string());
    local(|l| l.highlight.set(None));
    list.sel_index.set(Some(1));
}

fn shared_of(socket: Option<JewelSocket>, desired: IndexMap<String, Desired>) -> Shared {
    let (d1, d2) = td(|t| (t.devotion_variant1 as usize, t.devotion_variant2 as usize));
    Shared {
        jewel: jewel_id(),
        conqueror: td(|t| t.conqueror_type_id).map_or(1, |v| v as usize),
        devotion1: DEVOTION_VARIANTS[d1 - 1].0,
        devotion2: DEVOTION_VARIANTS[d2 - 1].0,
        socket,
        desired,
    }
}

/// The `Open Trade URL` button (TreeTab.lua:2220-2321).
fn open_trade_url(g: &mut Gfx) {
    let t = tab();
    let list = ctl::<ListControl<Row>>("searchResults");
    let n = t.timeless.results.borrow().len();
    let more = ctl::<CheckBoxControl>("searchMore").state.get();
    let (jewel, conqueror, devotion1, devotion2) = {
        let s = t.timeless.shared.borrow();
        (jewel_id(), s.conqueror, s.devotion1, s.devotion2)
    };
    let sel = list.sel_index.get();
    let (mut start_row, mut end_row) = match local(|l| l.highlight.get()) {
        Some(h) if !more => {
            let s = sel.expect("selIndex");
            (s.min(h), s.max(h))
        }
        _ => {
            let start = sel.unwrap_or(1);
            let max_filters = if more { 180 } else { 10 };
            (start, start + max_filters / if conqueror == 1 { 3 } else { 1 })
        }
    };
    // lua numbers: the count can go below zero when the selected row is past the list
    let mut seed_count = (n as i64 - start_row as i64).min(end_row as i64 - start_row as i64) + 1;
    if let Some((prev_row, prev_count)) = local(|l| l.last_search.get())
        && prev_row == start_row
        && prev_count as i64 == seed_count
    {
        start_row = end_row + 1;
        if start_row > n {
            return;
        }
        seed_count = ((n - start_row + 1) as i64).min(seed_count);
        end_row = start_row + seed_count as usize - 1;
    }
    list.sel_index.set(Some(start_row));
    list.scroll_bar_v.set_offset((start_row as f64 - 1.0) * 16.0);
    local(|l| {
        l.highlight.set(Some(end_row));
        l.last_search.set(Some((start_row, seed_count as usize)));
    });
    let ids = &pob_data::rules::rules().timeless_jewel_trade_ids[jewel - 1];
    let keystones: Vec<String> = ids["keystone"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    let trade_ids: Vec<String> = if jewel >= 7 {
        vec![keystones[0].clone()]
    } else if conqueror > 1 {
        vec![keystones[conqueror - 2].clone()]
    } else {
        keystones[..3].to_vec()
    };
    let mut seed_trades = Vec::new();
    {
        let results = t.timeless.results.borrow();
        for i in start_row..start_row + seed_count.max(0) as usize {
            let seed = results[i - 1].0.seed;
            for id in &trade_ids {
                seed_trades.push(json!({ "id": id, "value": { "min": seed, "max": seed } }));
            }
        }
    }
    let option = ["securable", "available", "onlineleague", "online", "any"][t.timeless.trade_type_index.get().unwrap_or(1) - 1];
    let mut stats = vec![json!({ "filters": seed_trades, "type": "count", "value": { "min": 1 } })];
    if let Some(devotion) = ids.get("devotion").and_then(|d| d.as_array()) {
        let mut filters = Vec::new();
        if devotion1 > 1 {
            filters.push(json!({ "id": devotion[devotion1 - 2] }));
        }
        if devotion2 > 1 {
            filters.push(json!({ "id": devotion[devotion2 - 2] }));
        }
        if !filters.is_empty() {
            stats.push(json!({ "filters": filters, "type": "and" }));
        }
    }
    let search = json!({ "status": { "option": option }, "stats": stats });
    let realm = ctl::<DropDownControl>("realmSelection")
        .get_sel_value()
        .and_then(|v| v.label)
        .expect("realm")
        .to_lowercase();
    let Some(league) = ctl::<DropDownControl>("searchTradeLeagueSelect").get_sel_value().and_then(|v| v.label) else {
        // the list of leagues has not come: `league:gsub` of TradeQueryRequests.lua:509
        panic!("Classes/TradeQueryRequests.lua:509: attempt to index local 'league' (a nil value)");
    };
    let url = crate::trade::requests::build_url(
        "https://www.pathofexile.com/trade/search",
        Some(&realm),
        &league,
        Some(&crate::trade::helpers::b64_gzip_encode(&crate::trade::dkjson::encode(&search))),
    );
    g.open_url(&url);
    g.copy(&url);
    local(|l| *l.last_trade_url.borrow_mut() = Some(url));
    ctl::<ButtonControl>("searchTradeButton").label.set("Open Next Trade URL".to_string());
}

/// A trade URL as the gate compares it: the text of `dkjson.encode` has its keys in the order of `pairs` (R13).
fn trade_url_parts(url: &str) -> serde_json::Value {
    let (root, code) = url.rsplit_once('/').expect("a trade URL");
    let text = crate::trade::helpers::b64_gzip_decode(code).expect("the query");
    json!({ "root": root, "search": serde_json::from_str::<serde_json::Value>(&text).expect("json") })
}

/// `updateLeagues()` (TreeTab.lua:2344-2382).
fn update_leagues(g: &mut Gfx) {
    let t = tab();
    let realm_id = ctl::<DropDownControl>("realmSelection")
        .get_sel_value()
        .and_then(|v| v.label)
        .expect("realm")
        .to_lowercase();
    let league_select = ctl::<DropDownControl>("searchTradeLeagueSelect");
    let list = t.timeless.trade_leagues.borrow_mut().entry(realm_id.clone()).or_default().clone();
    if !list.is_empty() {
        league_select.set_list(g, Some(drop_items(list.clone())));
        // restore the last league selected
        let last = t.timeless.league_select.borrow().clone();
        if let Some(i) = list.iter().position(|l| Some(l) == last.as_ref()) {
            league_select.set_sel(g, i as i64 + 1, false);
        }
        return;
    }
    let msg = ctl::<LabelControl>("msg");
    let weak_tab = Rc::downgrade(&t);
    let realm = realm_id.clone();
    t.timeless.requests.fetch_leagues(
        &realm_id,
        Rc::new(move |g: &mut Gfx, leagues: Vec<String>, err: Option<String>| {
            if let Some(err) = err {
                msg.label.set(format!("^1Error fetching league list, default league will be used\n{err}^7"));
                return;
            }
            let Some(t) = weak_tab.upgrade() else { return };
            let mut league_list = t.timeless.trade_leagues.borrow().get(&realm).cloned().unwrap_or_default();
            let mut temp: Vec<String> = Vec::new();
            for league in leagues {
                if !matches!(league.as_str(), "Standard" | "Ruthless" | "Hardcore" | "Hardcore Ruthless") {
                    if league.contains("Hardcore") || league.contains("Ruthless") {
                        league_list.push(league);
                    } else {
                        // the dynamic, base league goes first: the league shown on load
                        temp.push(league);
                        temp.extend(league_list.iter().cloned());
                        league_list.clone_from(&temp);
                    }
                }
            }
            league_list.extend(["Standard", "Hardcore", "Ruthless", "Hardcore Ruthless"].map(str::to_owned));
            t.timeless.trade_leagues.borrow_mut().insert(realm.clone(), league_list.clone());
            league_select.set_list(g, Some(drop_items(league_list)));
        }),
    );
}

/// The `Search` button (:2805-2870).
fn on_search(g: &mut Gfx) {
    let t = tab();
    let socket = td(|d| d.jewel_socket_id).unwrap_or(-1.0);
    let sockets = local(|l| l.sockets.clone());
    if socket == -1.0 {
        t.timeless.results.borrow_mut().clear();
        *t.timeless.shared.borrow_mut() = shared_of(None, IndexMap::new());
        for s in sockets.iter().skip(1) {
            if let Some(r) = search_single_socket(g, s.id) {
                t.timeless.shared.borrow_mut().desired.clone_from(&r.desired);
                t.timeless.results.borrow_mut().extend(format_results(&r, Some(s)));
            }
        }
        pob_engine::lua::lua_sort(&mut t.timeless.results.borrow_mut(), &mut |a: &Row, b: &Row| {
            if a.0.total == b.0.total {
                a.0.socket_label.as_deref().unwrap_or("").as_bytes() < b.0.socket_label.as_deref().unwrap_or("").as_bytes()
            } else {
                a.0.total > b.0.total
            }
        });
        after_search();
    } else {
        let info = sockets.iter().find(|s| s.id == socket).cloned();
        if let Some(r) = search_single_socket(g, socket) {
            *t.timeless.shared.borrow_mut() = shared_of(info, r.desired.clone());
            let mut rows = format_results(&r, None);
            pob_engine::lua::lua_sort(&mut rows, &mut |a: &Row, b: &Row| a.0.total > b.0.total);
            *t.timeless.results.borrow_mut() = rows;
            after_search();
        }
    }
}

// --- the fallback weights ----------------------------------------------------------------------------------------------------------------

thread_local! {
    /// `legionAddition.modListGenerated` of the variable-roll single-stat branch (:2089) by tree version and addition: the tree of a version lives as long as the process upstream. It decides `divisor` of
    /// the next `Generate`: only the branch that makes the list sets it (:2077-2091).
    static ADDITION_GENERATED: RefCell<std::collections::HashSet<(String, String)>> = RefCell::new(std::collections::HashSet::new());
}

/// `fallbackWeightsList` (:2119-2128).
fn fallback_stats() -> Vec<pob_engine::tree_tab::PowerStat> {
    let raw = pob_data::rules::rules().power_stat_list.as_array().expect("powerStatList");
    raw.iter()
        .zip(pob_engine::tree_tab::power_stat_list())
        .filter(|(e, p)| e.get("ignoreForItems").and_then(serde_json::Value::as_bool) != Some(true) && p.label != "Name")
        .map(|(_, p)| p)
        .collect()
}

/// `replaceHelperFunc` (:1980-1997).
fn replace_helper(stat_to_fix: &str, stat_key: &str, stat: &pob_data::timeless::LegionStat, mut value: f64) -> String {
    if stat.fmt.as_deref() == Some("g") {
        if stat_key.contains("per_minute") {
            value = pob_lua::number::round(value / 60.0, Some(1.0));
        } else if stat_key.contains("permyriad") {
            value /= 100.0;
        } else if stat_key.contains("_ms") {
            value /= 1000.0;
        }
    }
    let sub = |pattern: String| {
        let out = pob_lua::gsub(
            stat_to_fix.as_bytes(),
            pattern.as_bytes(),
            pob_lua::Repl::Str(ts(value).as_bytes()),
            None,
        )
        .unwrap_or_else(|e| panic!("Classes/TreeTab.lua:1992: {e:?}"))
        .0;
        String::from_utf8_lossy(&out).into_owned()
    };
    if stat.min != stat.max {
        sub(format!("%({}%-{}%)", ts(stat.min), ts(stat.max)))
    } else if stat.min != value {
        sub(ts(stat.min))
    } else {
        stat_to_fix.to_string()
    }
}

/// `buildStatModLists(legionPassive)` (:1998-2010): one list per stat, from `data.describeStats({ [stat] = 100, others = 0 }, "stat_descriptions")[1]`.
fn build_stat_mod_lists(tree: &pob_tree::PassiveTree, id: &str, sorted: &[String]) -> Vec<pob_engine::tree_tab::FallbackLine> {
    sorted
        .iter()
        .map(|key| {
            let line = legion_stat_line(sorted, key).unwrap_or_else(|| panic!("Classes/TreeTab.lua:2007: no description of {key} of {id}"));
            pob_engine::tree_tab::FallbackLine {
                mods: tree.parse_mod_list(&line),
                divisor: Some(100.0),
            }
        })
        .collect()
}

/// `data.describeStats({ [stat] = 100, others = 0 }, "stat_descriptions")[1]` (:2002-2006). `stats`: the keys of `legionPassive.stats`, which are its `sortedStats` (test `legion_stat_lines.rs`).
pub fn legion_stat_line(stats: &[String], stat: &str) -> Option<String> {
    let values: Vec<(String, f64)> = stats.iter().map(|k| (k.clone(), if k == stat { 100.0 } else { 0.0 })).collect();
    crate::stat_describer::describe_stats(&values, "stat_descriptions", false).0.into_iter().next()
}

/// `newNode.divisor = ...modList[1].value` (:2052, :2094).
fn first_mod_value(mods: &[Rc<pob_mods::Mod>], line: u32) -> f64 {
    match mods.first().map(|m| &m.value) {
        Some(pob_mods::Value::Num(n)) => *n,
        other => panic!("Classes/TreeTab.lua:{line}: modList[1].value is not a number: {other:?}"),
    }
}

/// `setupFallbackWeights()` (:1978-2117).
#[allow(clippy::too_many_lines)]
fn setup_fallback_weights(g: &mut Gfx) {
    use pob_engine::tree_tab::{FallbackLine, FallbackNode};
    let tree = legion();
    let leg = tree.legion().expect("legion");
    let jewel = jewel_id();
    let version = engine_read(|e| e.spec_rc().tree_version.clone());
    let uses_variable_rolls = jewel == 1 || jewel >= 7;
    let total_ids = |id: &str| -> &'static [&'static str] {
        match id {
            "total_strength" => &REVERSE_TOTAL_MOD_IDS[0..3],
            "total_dexterity" => &REVERSE_TOTAL_MOD_IDS[3..6],
            "total_devotion" => &REVERSE_TOTAL_MOD_IDS[6..9],
            _ => &[],
        }
    };
    let total_mod_entry = total_mod(jewel).map(|t| format!("total_{}", t.to_lowercase()));
    let mods = local(|l| l.mods.clone());
    let mut ids: Vec<String> = Vec::new();
    let mut nodes: Vec<FallbackNode> = Vec::new();
    for entry in &mods {
        let Some(mod_id) = entry.id.as_deref() else { continue };
        let is_total = total_mod_entry.as_deref() == Some(mod_id);
        let matches = |id: &str| id == mod_id || total_ids(mod_id).contains(&id);
        let mut new_node: Option<FallbackNode> = None;
        if let Some(node) = leg.nodes.iter().find(|n| matches(&n.id)) {
            let mut made = FallbackNode::default();
            if uses_variable_rolls {
                if node.sorted_stats.len() > 1 {
                    made.lines = build_stat_mod_lists(&tree, &node.id, &node.sorted_stats);
                } else {
                    let original = &node.sd[0];
                    let key = node.sorted_stats.first().unwrap_or_else(|| {
                        panic!(
                            "Classes/TreeTab.lua:1981: attempt to index local 'statMod' (a nil value), {}",
                            node.id
                        )
                    });
                    let line = replace_helper(original, key, node.stat_defs.get(key).expect("stat"), 100.0);
                    let list = if line == *original && node.sd.len() > 1 {
                        // Some fixed game stats represent several display lines; score the complete effect together.
                        node.stats.mod_list.mods.clone()
                    } else {
                        tree.parse_mod_list(&line)
                    };
                    made.lines = vec![FallbackLine { mods: list, divisor: None }];
                    made.divisor = Some(if line == *original { 1.0 } else { 100.0 });
                }
            } else {
                made.lines = vec![FallbackLine {
                    mods: node.stats.mod_list.mods.clone(),
                    divisor: None,
                }];
                if is_total {
                    made.divisor = Some(first_mod_value(&node.stats.mod_list.mods, 2052));
                }
            }
            new_node = Some(made);
        }
        if new_node.is_none()
            && let Some(addition) = leg.additions.iter().find(|a| matches(&a.id))
        {
            let mut made = FallbackNode::default();
            if uses_variable_rolls && addition.sorted_stats.len() > 1 {
                made.lines = build_stat_mod_lists(&tree, &addition.id, &addition.sorted_stats);
            } else {
                // `legionAddition.modListGenerated`: the list of the first time, no `divisor` the next times
                let key = (version.clone(), addition.id.clone());
                let generated = ADDITION_GENERATED.with_borrow(|m| m.contains(&key));
                let original = &addition.sd[0];
                let mut line = original.clone();
                if uses_variable_rolls {
                    for (k, stat) in addition.stats.iter() {
                        line = replace_helper(&line, k, stat, 100.0);
                    }
                }
                let list = tree.parse_mod_list(&line);
                if !generated {
                    ADDITION_GENERATED.with_borrow_mut(|m| m.insert(key));
                    made.divisor = Some(if line == *original { 1.0 } else { 100.0 });
                }
                if !uses_variable_rolls && is_total {
                    made.divisor = Some(first_mod_value(&list, 2094));
                }
                made.lines = vec![FallbackLine { mods: list, divisor: None }];
            }
            new_node = Some(made);
        }
        if let Some(n) = new_node {
            ids.push(mod_id.to_string());
            nodes.push(n);
        }
    }
    let list = ctl::<DropDownControl>("fallbackWeightsList");
    let stat = fallback_stats()
        .into_iter()
        .nth(list.sel_index.get().wrapping_sub(1))
        .expect("the selected fallback mode");
    let weights = engine_read(|e| e.fallback_weights(&nodes, &stat)).unwrap_or_else(|e| panic!("{e}"));
    // :2106-2116
    let round3 = |v: f64| ts(pob_lua::number::round(v * 100.0, Some(3.0)));
    let mut new_list = String::new();
    for (id, w) in ids.iter().zip(&weights) {
        let (w1, w2) = (w[0], w.get(1).copied());
        if w1 != 0.0 || w2.is_some_and(|v| v != 0.0) {
            if !new_list.is_empty() {
                new_list.push('\n');
            }
            new_list.push_str(&format!("{id}, {}, {}, 0", round3(w1), round3(w2.unwrap_or(0.0))));
        }
    }
    update_search_list(g, &new_list, true);
}

// --- the popup ---------------------------------------------------------------------------------------------------------------------------

fn label(name: &str, a: Option<crate::control::Anchor>, r: crate::control::RectProps, text: &str) {
    reg(name, LabelControl::new(a, r, text.to_string()));
}

/// :1748-1755 etc.: where a slider's tooltip goes (`limit`: the screen width from which it sits above the knob).
fn slider_tooltip_pos(slider: &Rc<SliderControl>, limit: f64) {
    let s = Rc::downgrade(slider);
    slider.tip.set_draw_hook(move |tt: &Tooltip, g: &Gfx, x, y| {
        let s = s.upgrade().expect("slider");
        let offset = pob_lua::number::round(184.0 * (1.0 - s.val.get()), None);
        let (_, h) = tt.get_size(g);
        if crate::common::screen_size().0 >= limit - offset {
            (x - 8.0 - offset, y - 4.0 - h)
        } else {
            (x, y)
        }
    });
}

/// `updateSliders(sliderData)` (:1850-1874).
fn update_sliders(g: &Gfx, row: &[TCell]) {
    let req = TCell::Str("required".into());
    let num = |i: usize| row.get(i).and_then(TCell::tonumber).unwrap_or(0.0);
    let (s1, l1) = (ctl::<SliderControl>("nodeSlider"), ctl::<LabelControl>("nodeSliderValue"));
    if row.get(1) == Some(&req) {
        s1.val.set(1.0);
        l1.label.set(format!("^7{}", fmt("%.3f", 10.0)));
    } else {
        s1.val.set((num(1) / 10.0).clamp(0.0, 1.0));
        l1.label.set(format!("^7{}", fmt("%.3f", s1.val.get() * 10.0)));
    }
    let (s2, l2) = (ctl::<SliderControl>("nodeSlider2"), ctl::<LabelControl>("nodeSlider2Value"));
    if s2.base.enabled.get(g) {
        if row.get(2) == Some(&req) {
            s2.val.set(1.0);
            l2.label.set(format!("^7{}", fmt("%.3f", 10.0)));
        } else {
            s2.val.set((num(2) / 10.0).clamp(0.0, 1.0));
            l2.label.set(format!("^7{}", fmt("%.3f", s2.val.get() * 10.0)));
        }
    }
    let (s3, l3) = (ctl::<SliderControl>("nodeSlider3"), ctl::<LabelControl>("nodeSlider3Value"));
    if row.get(3) == Some(&req) {
        s3.val.set(1.0);
        l3.label.set("^7Required".to_string());
    } else {
        s3.val.set((num(3) / 500.0).clamp(0.0, 1.0));
        l3.label.set(format!("^7{}", fmt("%.f", s3.val.get() * 500.0)));
    }
}

/// `getLegionStatLabels(legionPassive)` (:1877-1885).
fn stat_labels(jewel: usize, sorted: &[String], sd: &[String]) -> (usize, String, String) {
    let count = if jewel >= 7 { sorted.len() } else { sd.len() };
    if count > sd.len() {
        return (count, format!("Minimum value: {}", sd[0]), format!("Maximum value: {}", sd[0]));
    }
    (
        count,
        if count == 1 {
            sd.join(" + ")
        } else {
            sd.first().cloned().unwrap_or_else(|| "None".into())
        },
        sd.get(1).cloned().unwrap_or_else(|| "None".into()),
    )
}

/// `controls.nodeSelect` selFunc (:1886-1940).
fn on_node_select(g: &mut Gfx, index: usize) {
    local(|l| {
        l.slider_label = "None".into();
        l.slider2_label = "None".into();
    });
    let Some(id) = local(|l| l.mods.get(index - 1).and_then(|m| m.id.clone())) else {
        return;
    };
    let tree = legion();
    let leg = tree.legion().expect("legion");
    let jewel = jewel_id();
    let mut count = 0;
    if let Some(n) = leg.nodes.iter().find(|n| n.id == id) {
        let (c, a, b) = stat_labels(jewel, &n.sorted_stats, &n.sd);
        count = c;
        local(|l| {
            l.slider_label = a;
            l.slider2_label = b;
        });
    }
    if count == 0
        && let Some(n) = leg.additions.iter().find(|n| n.id == id)
    {
        let (c, a, b) = stat_labels(jewel, &n.sorted_stats, &n.sd);
        count = c;
        local(|l| {
            l.slider_label = a;
            l.slider2_label = b;
        });
    }
    let (s2, l2, v2) = (
        ctl::<SliderControl>("nodeSlider2"),
        ctl::<LabelControl>("nodeSlider2Label"),
        ctl::<LabelControl>("nodeSlider2Value"),
    );
    if count <= 1 {
        l2.label.set("^9Secondary Node Weight:".to_string());
        s2.val.set(0.0);
        v2.label.set(format!("^9{}", fmt("%.3f", 0.0)));
    } else {
        l2.label.set("^7Secondary Node Weight:".to_string());
        v2.label.set(format!("^7{}", fmt("%.3f", s2.val.get() * 10.0)));
    }
    s2.base.enabled.set(count > 1);
    let w = node_weights(g);
    let new_node = format!("{id}, {}, {}, {}", w[0].text(), w[1].text(), w[2].text());
    let fb = fallback_shown(g);
    let rows = local(|l| if fb { l.fallback.clone() } else { l.search.clone() });
    if let Some(row) = rows.iter().find(|r| r.first().is_some_and(|c| c.text() == id)) {
        update_sliders(g, row);
        return;
    }
    let edit = ctl::<EditControl>(if fb { "searchListFallback" } else { "searchList" });
    let len = edit.buf.borrow().len();
    edit.caret.set(len + 1);
    edit.insert(g, &format!("{}{new_node}", if len > 0 { "\n" } else { "" }));
    mod_flag();
}

/// TreeTab.lua:190 `self:FindTimelessJewel()`.
#[allow(clippy::too_many_lines)]
pub fn find_timeless_jewel(g: &mut Gfx) {
    CONTROLS.with_borrow_mut(HashMap::clear);
    BUILDING.with_borrow_mut(IndexMap::clear);
    POPUP.with_borrow_mut(|p| *p = None);
    LOCAL.with_borrow_mut(|l| *l = Local::default());
    let tree = legion();
    let t = tab();
    // :1208-1218, :1262-1272
    td(|d| {
        if d.jewel_type_id.is_none() {
            d.jewel_type_id = Some(1.0);
        }
    });
    let jewel = jewel_id();
    td(|d| {
        if d.conqueror_type_id.is_none() {
            d.conqueror_type_id = Some(1.0);
        }
    });
    // :1292-1303 Reclaimed Malevolence's required outcome
    let leg = tree.legion().expect("legion");
    let mut abyss: Vec<(String, String)> = leg
        .nodes
        .iter()
        .filter(|n| {
            n.id.strip_prefix("abyss_special_ascendancy_notable_")
                .is_some_and(|d| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()))
        })
        .map(|n| (n.dn.clone(), n.id.clone()))
        .collect();
    pob_engine::lua::lua_sort(&mut abyss, &mut |a: &(String, String), b: &(String, String)| {
        a.0.as_bytes() < b.0.as_bytes()
    });
    let abyss_ids: Rc<Vec<Option<String>>> = Rc::new(std::iter::once(None).chain(abyss.iter().map(|a| Some(a.1.clone()))).collect());
    let abyss_labels: Vec<String> = std::iter::once("Any".to_string())
        .chain(abyss.iter().map(|a| a.0.clone()))
        .collect();
    // :1304-1356
    let spec = engine_read(pob_engine::engine::Engine::spec_rc);
    let mut sockets: Vec<JewelSocket> = Vec::new();
    for s in &spec.nodes {
        let tn = s.tree_node();
        if tn.is_jewel_socket && s.name() != Some("Charm Socket") {
            let id = s.id;
            let mut keystone = "Unknown".to_string();
            match id {
                26725 => keystone = "Marauder".into(),
                54127 => keystone = "Duelist".into(),
                7960 => keystone = "Templar/Witch".into(),
                _ => {
                    let mut min_distance = f64::INFINITY;
                    let radius = tree
                        .nodes_in_radius(id)
                        .unwrap_or_else(|| panic!("Classes/TreeTab.lua:1321: attempt to index a nil value"));
                    for &r in &radius[2] {
                        let n = tree.node(r).expect("node");
                        if n.ty == pob_tree::NodeType::Keystone {
                            let distance =
                                ((n.x.expect("x") - tn.x.expect("x")).powi(2) + (n.y.expect("y") - tn.y.expect("y")).powi(2)).sqrt();
                            if distance < min_distance {
                                keystone = n.name.clone().unwrap_or_default();
                                min_distance = distance;
                            }
                        }
                    }
                }
            }
            let mut label = format!("{keystone}: {id}");
            if s.alloc {
                label = format!("# {label}");
            }
            sockets.push(JewelSocket {
                label,
                keystone,
                id: f64::from(id),
            });
        }
    }
    pob_engine::lua::lua_sort(&mut sockets, &mut |a: &JewelSocket, b: &JewelSocket| {
        a.label.as_bytes() < b.label.as_bytes()
    });
    sockets.insert(
        0,
        JewelSocket {
            label: "All Sockets".into(),
            keystone: "Multi-Socket Search".into(),
            id: -1.0,
        },
    );
    td(|d| {
        if d.jewel_socket_id.is_none() {
            d.jewel_socket_id = Some(-1.0);
        }
    });
    let socket_items: Vec<DropItem> = drop_items(sockets.iter().map(|s| s.label.clone()));
    let socket_ids: Rc<Vec<f64>> = Rc::new(sockets.iter().map(|s| s.id).collect());
    local(|l| l.sockets = sockets);
    parse_from_text(false);
    parse_from_text(true);

    let (row_spacing, row_height, label_height, label_spacing) = (6.0, 17.0, 16.0, 4.0);
    // :1516-1525 devotion
    label(
        "devotionSelectLabel",
        anchor_nil("TOPRIGHT", "TOPLEFT"),
        rect(920.0, 25.0, 0.0, 16.0),
        "^7Devotion modifiers:",
    );
    ctl::<LabelControl>("devotionSelectLabel").base.shown.set(jewel == 4);
    let dv = || drop_items(DEVOTION_VARIANTS.iter().map(|d| d.1.to_string()));
    let dl = get("devotionSelectLabel");
    let d1 = DropDownControl::new(
        g,
        anchor("TOP", &dl, "BOTTOM"),
        rect(0.0, 8.0, 200.0, 18.0),
        dv(),
        Some(Rc::new(|_g: &mut Gfx, index, _v: &DropItem| {
            td(|d| d.devotion_variant1 = index as f64)
        })),
        None,
        false,
    );
    d1.sel_index.set(td(|d| d.devotion_variant1) as usize);
    let d1r: ControlRef = d1.clone();
    reg("devotionSelect1", d1);
    let d2 = DropDownControl::new(
        g,
        anchor("TOP", &d1r, "BOTTOM"),
        rect(0.0, 7.0, 200.0, 18.0),
        dv(),
        Some(Rc::new(|_g: &mut Gfx, index, _v: &DropItem| {
            td(|d| d.devotion_variant2 = index as f64)
        })),
        None,
        false,
    );
    d2.sel_index.set(td(|d| d.devotion_variant2) as usize);
    reg("devotionSelect2", d2);
    // :1565-1590 the jewel type, the conqueror
    let jewel_select = DropDownControl::new(
        g,
        anchor_nil("TOPLEFT", "TOPLEFT"),
        rect(520.0, 25.0, 200.0, row_height),
        drop_items(JEWEL_TYPES.iter().map(|j| j.0.to_string())),
        Some(Rc::new(|g: &mut Gfx, index, _v: &DropItem| {
            td(|d| d.jewel_type_id = Some(index as f64));
            ctl::<LabelControl>("devotionSelectLabel").base.shown.set(index == 4);
            ctl::<LabelControl>("abyssAscendancyLabel").base.shown.set(index == 11);
            ctl::<DropDownControl>("abyssAscendancySelect").sel_index.set(1);
            let cq = ctl::<DropDownControl>("conquerorSelect");
            *cq.list.borrow_mut() = drop_items(conqueror_types(index).into_iter().map(str::to_string));
            cq.sel_index.set(1);
            td(|d| d.conqueror_type_id = Some(1.0));
            ctl::<DropDownControl>("nodeSelect").sel_index.set(1);
            refresh_mods();
            update_search_list(g, "", false);
            update_search_list(g, "", true);
            if ctl::<CheckBoxControl>("socketFilter").state.get() {
                set_allocated_nodes(g);
            }
        })),
        None,
        false,
    );
    jewel_select.sel_index.set(jewel);
    let js: ControlRef = jewel_select.clone();
    reg("jewelSelect", jewel_select);
    label(
        "jewelSelectLabel",
        anchor("RIGHT", &js, "LEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Jewel Type:",
    );
    let conqueror = DropDownControl::new(
        g,
        anchor("TOPLEFT", &js, "BOTTOMLEFT"),
        rect(0.0, row_spacing, 200.0, row_height),
        drop_items(conqueror_types(jewel).into_iter().map(str::to_string)),
        Some(Rc::new(|_g: &mut Gfx, index, _v: &DropItem| {
            td(|d| d.conqueror_type_id = Some(index as f64));
            mod_flag();
        })),
        None,
        false,
    );
    conqueror.sel_index.set(td(|d| d.conqueror_type_id).map_or(1, |v| v as usize));
    conqueror.tip.set_tooltip_func(
        |tt: &mut Tooltip, _g: &Gfx, (_, _, val): crate::controls::drop_down::DropTooltipArgs| {
            tt.clear(false);
            if val.and_then(|v| v.label).is_some_and(|l| l.ends_with(')')) {
                // the keystones of these jewels are legion nodes (`keystoneMap`, PassiveTree.lua:739-741)
                note_not_ported("TreeTab.lua:1591-1600 AddNodeTooltip of a legion keystone in the conqueror drop-down");
            }
        },
    );
    let cq: ControlRef = conqueror.clone();
    reg("conquerorSelect", conqueror);
    label(
        "conquerorSelectLabel",
        anchor("RIGHT", &cq, "LEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Conqueror:",
    );
    // :1637-1649 TimelessJewelSocketControl
    let ids2 = socket_ids.clone();
    let socket_select = DropDownControl::new(
        g,
        anchor("TOPLEFT", &cq, "BOTTOMLEFT"),
        rect(0.0, row_spacing, 200.0, row_height),
        socket_items,
        Some(Rc::new(move |g: &mut Gfx, index, _v: &DropItem| {
            td(|d| d.jewel_socket_id = Some(ids2[index - 1]));
            set_allocated_nodes(g);
            mod_flag();
        })),
        None,
        false,
    );
    let cur_socket = td(|d| d.jewel_socket_id).unwrap_or(-1.0);
    if let Some(i) = socket_ids.iter().position(|s| *s == cur_socket) {
        socket_select.sel_index.set(i + 1);
    }
    install_socket_viewer(&socket_select, socket_ids.clone());
    let ss: ControlRef = socket_select.clone();
    reg("socketSelect", socket_select);
    label(
        "socketSelectLabel",
        anchor("RIGHT", &ss, "LEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Jewel Socket:",
    );
    // :1651-1684 the filter, the socket check box
    let socket_filter = CheckBoxControl::new(
        g,
        anchor("TOPLEFT", &ss, "BOTTOMLEFT"),
        [0.0, row_spacing, row_height],
        None,
        Some(Rc::new(|g: &mut Gfx, value| {
            td(|d| d.socket_filter = value);
            mod_flag();
            ctl::<LabelControl>("socketFilterAdditionalDistanceLabel").base.shown.set(value);
            ctl::<SliderControl>("socketFilterAdditionalDistance").base.shown.set(value);
            ctl::<LabelControl>("socketFilterAdditionalDistanceValue").base.shown.set(value);
            if value {
                set_allocated_nodes(g);
            } else {
                build_node_option_checkboxes(g, None);
            }
        })),
        None,
        false,
    );
    socket_filter.tip.set_tooltip_func(|tooltip: &mut Tooltip, g: &Gfx, _| {
        tooltip.clear(false);
        tooltip.add_line(
            g,
            16.0,
            Some("^7Enable this option to exclude nodes that you do not have allocated on your active passive skill tree."),
        );
        tooltip.add_line(
            g,
            16.0,
            Some("^7This can be useful if you're never going to path towards those excluded nodes and don't care what happens to them."),
        );
    });
    let filter_state = td(|d| d.socket_filter);
    socket_filter.state.set(filter_state);
    let sf: ControlRef = socket_filter.clone();
    reg("socketFilter", socket_filter);
    label(
        "socketFilterLabel",
        anchor("RIGHT", &sf, "LEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Filter Nodes:",
    );
    let socket_allocate = CheckBoxControl::new(
        g,
        anchor("TOPLEFT", &sf, "BOTTOMLEFT"),
        [0.0, row_spacing, row_height],
        None,
        Some(Rc::new(|_g: &mut Gfx, value| {
            td(|d| d.socket_allocate = value);
            mod_flag();
        })),
        None,
        false,
    );
    socket_allocate.tip.set_tooltip_func(|tooltip: &mut Tooltip, g: &Gfx, _| {
        tooltip.clear(false);
        tooltip.add_line(
            g,
            16.0,
            Some("^7Double clicking a result also equips the jewel in its jewel socket."),
        );
        tooltip.add_line(
            g,
            16.0,
            Some("^7The socket must be allocated on your current tree; if it isn't, the jewel is only added to your item list."),
        );
        tooltip.add_line(g, 16.0, Some("^7A jewel already in that socket is replaced."));
    });
    socket_allocate.state.set(td(|d| d.socket_allocate));
    let sa: ControlRef = socket_allocate.clone();
    reg("socketAllocate", socket_allocate);
    label(
        "socketAllocateLabel",
        anchor("RIGHT", &sa, "LEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Socket Jewel:",
    );
    // :1687-1701 the protected nodes
    let protect = LabelControl::new(
        anchor_nil("TOPLEFT", "TOPLEFT"),
        [
            15.0.into(),
            Prop::func(|_| if jewel_id() == 11 { 85.0 } else { 25.0 }),
            0.0.into(),
            16.0.into(),
        ],
        "^7Protect allocated nodes from changing:".to_string(),
    );
    protect.base.shown.set_fn(|_| {
        let (j, s) = (jewel_id(), td(|d| d.jewel_socket_id).unwrap_or(-1.0));
        (j == 4 || j == 11) && ctl::<CheckBoxControl>("socketFilter").state.get() && s != -1.0
    });
    reg("protectAllocatedLabel", protect);
    if filter_state {
        set_allocated_nodes(g);
    }
    // :1705-1733 Reclaimed Malevolence
    label(
        "abyssAscendancyLabel",
        anchor_nil("TOPLEFT", "TOPLEFT"),
        rect(15.0, 25.0, 0.0, 16.0),
        "^7Required ascendancy outcome:",
    );
    ctl::<LabelControl>("abyssAscendancyLabel").base.shown.set(jewel == 11);
    let al = get("abyssAscendancyLabel");
    let ai = abyss_ids.clone();
    let abyss_select = DropDownControl::new(
        g,
        anchor("TOPLEFT", &al, "BOTTOMLEFT"),
        rect(0.0, 8.0, 200.0, 18.0),
        drop_items(abyss_labels),
        Some(Rc::new(move |g: &mut Gfx, index, _v: &DropItem| {
            let rows = local(|l| l.search.clone());
            let mut lines: Vec<String> = rows
                .iter()
                .filter(|r| !r.first().is_some_and(|c| c.text().starts_with("abyss_special_ascendancy_notable_")))
                .map(|r| r.iter().map(TCell::text).collect::<Vec<_>>().join(", "))
                .collect();
            if let Some(id) = &ai[index - 1] {
                lines.push(format!("{id}, 1, 0, 1"));
            }
            update_search_list(g, &lines.join("\n"), false);
        })),
        None,
        false,
    );
    abyss_select.sel_index.set(1);
    for row in local(|l| l.search.clone()) {
        if let Some(i) = abyss_ids
            .iter()
            .position(|o| o.is_some() && o.clone() == row.first().map(TCell::text))
        {
            abyss_select.sel_index.set(i + 1);
        }
    }
    reg("abyssAscendancySelect", abyss_select);
    // :1735-1760 the node distance
    let max_distance = 10.0;
    label(
        "socketFilterAdditionalDistanceLabel",
        anchor("LEFT", &sf, "RIGHT"),
        rect(10.0, 0.0, 0.0, 16.0),
        "^7Node Distance:",
    );
    let dl2 = get("socketFilterAdditionalDistanceLabel");
    let distance = SliderControl::new(
        anchor("LEFT", &dl2, "RIGHT"),
        rect(10.0, 0.0, 66.0, 18.0),
        Some(Rc::new(move |_g: &mut Gfx, value| {
            let d = (value * max_distance + 0.01).floor();
            td(|t| t.socket_filter_distance = d);
            ctl::<LabelControl>("socketFilterAdditionalDistanceValue")
                .label
                .set(format!("^7{}", fmt("%d", d)));
        })),
        Some([1.0, 1.0 / (max_distance * 2.0), 1.0 / max_distance]),
    );
    {
        let w = Rc::downgrade(&distance);
        distance.tip.set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, _| {
            tooltip.clear(false);
            if !w.upgrade().expect("slider").dragging.get() {
                tooltip.add_line(
                    g,
                    16.0,
                    Some("^7This controls the maximum amount of points that need to be spent to grab a node before its filtered out"),
                );
            }
        });
    }
    slider_tooltip_pos(&distance, 1384.0);
    let dr: ControlRef = distance.clone();
    reg("socketFilterAdditionalDistance", distance.clone());
    label(
        "socketFilterAdditionalDistanceValue",
        anchor("LEFT", &dr, "RIGHT"),
        rect(5.0, 0.0, 0.0, 16.0),
        "^70",
    );
    distance.set_val(g, td(|d| d.socket_filter_distance) / max_distance);
    for n in ["socketFilterAdditionalDistanceLabel", "socketFilterAdditionalDistanceValue"] {
        ctl::<LabelControl>(n).base.shown.set(filter_state);
    }
    distance.base.shown.set(filter_state);
    // :1762-1848 the weights
    let speed = [0.01, 0.0001, 0.001];
    let speed2 = [0.2, 0.002, 0.02];
    let weight_tip = |s: &Rc<SliderControl>, first: bool, none: &'static str| {
        let w = Rc::downgrade(s);
        s.tip.set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, _| {
            tooltip.clear(false);
            if !w.upgrade().expect("slider").dragging.get() {
                let stat = local(|l| if first { l.slider_label.clone() } else { l.slider2_label.clone() });
                if stat == "None" {
                    tooltip.add_line(g, 16.0, Some(none));
                } else {
                    tooltip.add_line(g, 16.0, Some("^7This slider controls the weight of the following stat:"));
                    tooltip.add_line(g, 16.0, Some(&format!("^7        {stat}")));
                }
            }
        });
    };
    local(|l| {
        l.slider_label = "None".into();
        l.slider2_label = "None".into();
    });
    let slider1 = SliderControl::new(
        anchor("TOPLEFT", &sa, "BOTTOMLEFT"),
        rect(0.0, row_spacing, 200.0, row_height),
        Some(Rc::new(|g: &mut Gfx, value| {
            ctl::<LabelControl>("nodeSliderValue")
                .label
                .set(format!("^7{}", fmt("%.3f", value * 10.0)));
            let fb = fallback_shown(g);
            parse_to_text(g, fb);
        })),
        Some(speed),
    );
    weight_tip(
        &slider1,
        true,
        "^7For nodes with multiple stats this slider controls the weight of the first stat listed.",
    );
    slider_tooltip_pos(&slider1, 1338.0);
    let s1r: ControlRef = slider1.clone();
    reg("nodeSlider", slider1.clone());
    label(
        "nodeSliderLabel",
        anchor("RIGHT", &s1r, "LEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Primary Node Weight:",
    );
    label(
        "nodeSliderValue",
        anchor("LEFT", &s1r, "RIGHT"),
        rect(5.0, 0.0, 0.0, 16.0),
        "^71.000",
    );
    let slider2 = SliderControl::new(
        anchor("TOPLEFT", &s1r, "BOTTOMLEFT"),
        rect(0.0, row_spacing, 200.0, row_height),
        Some(Rc::new(|g: &mut Gfx, value| {
            ctl::<LabelControl>("nodeSlider2Value")
                .label
                .set(format!("^7{}", fmt("%.3f", value * 10.0)));
            let fb = fallback_shown(g);
            parse_to_text(g, fb);
        })),
        Some(speed),
    );
    weight_tip(
        &slider2,
        false,
        "^7For nodes with multiple stats this slider controls the weight of the second stat listed.",
    );
    slider_tooltip_pos(&slider2, 1384.0);
    let s2r: ControlRef = slider2.clone();
    reg("nodeSlider2", slider2.clone());
    label(
        "nodeSlider2Label",
        anchor("RIGHT", &s2r, "LEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Secondary Node Weight:",
    );
    label(
        "nodeSlider2Value",
        anchor("LEFT", &s2r, "RIGHT"),
        rect(5.0, 0.0, 0.0, 16.0),
        "^71.000",
    );
    let slider3 = SliderControl::new(
        anchor("TOPLEFT", &s2r, "BOTTOMLEFT"),
        rect(0.0, row_spacing, 200.0, row_height),
        Some(Rc::new(|g: &mut Gfx, value| {
            let l = ctl::<LabelControl>("nodeSlider3Value");
            if value == 1.0 {
                l.label.set("^7Required".to_string());
            } else {
                l.label.set(format!("^7{}", fmt("%.f", value * 500.0)));
            }
            let fb = fallback_shown(g);
            parse_to_text(g, fb);
        })),
        Some(speed2),
    );
    {
        let w = Rc::downgrade(&slider3);
        slider3.tip.set_tooltip_func(move |tooltip: &mut Tooltip, g: &Gfx, _| {
            tooltip.clear(false);
            if !w.upgrade().expect("slider").dragging.get() {
                tooltip.add_line(
                    g,
                    16.0,
                    Some("^7Seeds that do not meet the minimum weight threshold for a desired node are excluded from the search results."),
                );
            }
        });
    }
    slider_tooltip_pos(&slider3, 1728.0);
    let s3r: ControlRef = slider3.clone();
    reg("nodeSlider3", slider3.clone());
    label(
        "nodeSlider3Label",
        anchor("RIGHT", &s3r, "LEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Minimum Node Weight:",
    );
    label("nodeSlider3Value", anchor("LEFT", &s3r, "RIGHT"), rect(5.0, 0.0, 0.0, 16.0), "^70");
    slider1.set_val(g, 0.1);
    slider2.set_val(g, 0.1);
    slider3.set_val(g, 0.0);
    // :1876-1949 the node drop-down
    refresh_mods();
    let mods = local(|l| l.mods.clone());
    let node_select = DropDownControl::new(
        g,
        anchor("TOPLEFT", &s3r, "BOTTOMLEFT"),
        rect(0.0, row_spacing, 200.0, row_height),
        drop_items(mods.iter().map(|m| m.label.clone())),
        Some(Rc::new(|g: &mut Gfx, index, _v: &DropItem| on_node_select(g, index))),
        None,
        false,
    );
    node_select.tip.set_tooltip_func(
        |tooltip: &mut Tooltip, g: &Gfx, (mode, index, _): crate::controls::drop_down::DropTooltipArgs| {
            tooltip.clear(false);
            if mode != "OUT" {
                let d = local(|l| l.mods.get(index.wrapping_sub(1)).map(|m| m.descriptions.clone())).unwrap_or_default();
                for line in d {
                    tooltip.add_line(g, 16.0, Some(&format!("^7{line}")));
                }
            }
        },
    );
    let ns: ControlRef = node_select.clone();
    reg("nodeSelect", node_select);
    label(
        "nodeSelectLabel",
        anchor("RIGHT", &ns, "LEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Search for Node:",
    );
    // :2119-2151 the fallback weights
    let fallback_items: Vec<String> = fallback_stats().iter().map(|p| format!("Sort by {}", p.label)).collect();
    let fw = DropDownControl::new(
        g,
        anchor("TOPLEFT", &ns, "BOTTOMLEFT"),
        rect(0.0, row_spacing, 200.0, row_height),
        drop_items(fallback_items),
        Some(Rc::new(|_g: &mut Gfx, index, _v: &DropItem| {
            td(|d| d.fallback_weight_mode_idx = Some(index as f64))
        })),
        None,
        false,
    );
    fw.sel_index.set(td(|d| d.fallback_weight_mode_idx).map_or(1, |v| v as usize));
    let fwr: ControlRef = fw.clone();
    reg("fallbackWeightsList", fw);
    label(
        "fallbackWeightsLabel",
        anchor("RIGHT", &fwr, "LEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Fallback Weight Mode:",
    );
    let fb = ButtonControl::new(
        anchor("LEFT", &fwr, "RIGHT"),
        rect(5.0, 0.0, 66.0, 18.0),
        "Generate".to_string(),
        |g| {
            setup_fallback_weights(g);
            ctl::<ButtonControl>("searchListFallbackButton")
                .label
                .set("^4Fallback Nodes".to_string());
        },
    );
    fb.tip.set_tooltip_func(|tooltip: &mut Tooltip, g: &Gfx, ()| {
        tooltip.clear(false);
        tooltip.add_line(
            g,
            16.0,
            Some("^7Click this button to generate new fallback node weights, replacing your old ones."),
        );
    });
    reg("fallbackWeightsButton", fb);
    let tm = EditControl::new(
        g,
        anchor("TOPLEFT", &fwr, "BOTTOMLEFT"),
        rect(0.0, row_spacing, 200.0, row_height),
        EditArgs {
            init: Some(String::new()),
            filter: Some("%D".into()),
            change_func: Some(Rc::new(|_g: &mut Gfx, val: &str, _| {
                tab().timeless.total_minimum_weight.set(pob_lua::tonumber::tonumber(val));
                mod_flag();
            })),
            ..Default::default()
        },
    );
    tm.tip.set_tooltip_func(|tooltip: &mut Tooltip, g: &Gfx, ()| {
        tooltip.clear(false);
        tooltip.add_line(
            g,
            16.0,
            Some("^7Optional: Only show results where total weight meets or exceeds this value."),
        );
    });
    let tmr: ControlRef = tm.clone();
    reg("totalMinimumWeight", tm);
    label(
        "totalMinimumWeightLabel",
        anchor("RIGHT", &tmr, "LEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Total Minimum Weight:",
    );
    // :2153-2214 the lists
    let (list_width, list_height, button_height, edge) = (500.0, 200.0, 20.0, 12.0);
    let list_y = -(button_height + edge * 2.0);
    let search_list = EditControl::new(
        g,
        anchor_nil("BOTTOMLEFT", "BOTTOMLEFT"),
        rect(edge, list_y, list_width, list_height),
        EditArgs {
            init: td(|d| d.search_list.clone()),
            filter: Some("^%C\t\n".into()),
            change_func: Some(Rc::new(|_g: &mut Gfx, value: &str, _| {
                td(|d| d.search_list = Some(value.to_string()));
                parse_from_text(false);
                mod_flag();
            })),
            line_height: Some(16.0),
            allow_zoom: true,
            ..Default::default()
        },
    );
    search_list.base.shown.set(true);
    search_list.base.enabled.set(true);
    search_list.set_text(g, &td(|d| d.search_list.clone()).unwrap_or_default(), false);
    reg("searchList", search_list);
    let search_fb = EditControl::new(
        g,
        anchor_nil("BOTTOMLEFT", "BOTTOMLEFT"),
        rect(edge, list_y, list_width, list_height),
        EditArgs {
            init: td(|d| d.search_list_fallback.clone()),
            filter: Some("^%C\t\n".into()),
            change_func: Some(Rc::new(|_g: &mut Gfx, value: &str, _| {
                td(|d| d.search_list_fallback = Some(value.to_string()));
                parse_from_text(true);
                mod_flag();
            })),
            line_height: Some(16.0),
            allow_zoom: true,
            ..Default::default()
        },
    );
    search_fb.base.shown.set(false);
    search_fb.base.enabled.set(false);
    search_fb.set_text(g, &td(|d| d.search_list_fallback.clone()).unwrap_or_default(), false);
    reg("searchListFallback", search_fb);
    let sl_btn = ButtonControl::new(
        anchor_nil("BOTTOMLEFT", "BOTTOMLEFT"),
        rect(edge, list_y - list_height - row_spacing, 106.0, button_height),
        "^7Desired Nodes".to_string(),
        |g| {
            if fallback_shown(g) {
                let (a, b) = (ctl::<EditControl>("searchListFallback"), ctl::<EditControl>("searchList"));
                a.base.shown.set(false);
                a.base.enabled.set(false);
                b.base.shown.set(true);
                b.base.enabled.set(true);
            }
        },
    );
    sl_btn.tip.set_tooltip_func(|tooltip: &mut Tooltip, g: &Gfx, ()| {
        tooltip.clear(false);
        tooltip.add_line(g, 16.0, Some("^7This contains a list of your desired nodes along with their primary, secondary, and minimum weights."));
        tooltip.add_line(g, 16.0, Some("^7This list can be updated manually or by selecting the node you want to update via the search dropdown list and then moving the node weight sliders."));
    });
    sl_btn.locked.set_fn(|g| ctl::<EditControl>("searchList").base.shown.get(g));
    let slb: ControlRef = sl_btn.clone();
    reg("searchListButton", sl_btn);
    let sf_btn = ButtonControl::new(
        anchor("LEFT", &slb, "RIGHT"),
        rect(5.0, 0.0, 110.0, button_height),
        "^7Fallback Nodes".to_string(),
        |_g| {
            let (a, b) = (ctl::<EditControl>("searchList"), ctl::<EditControl>("searchListFallback"));
            a.base.shown.set(false);
            a.base.enabled.set(false);
            b.base.shown.set(true);
            b.base.enabled.set(true);
            ctl::<ButtonControl>("searchListFallbackButton")
                .label
                .set("^7Fallback Nodes".to_string());
        },
    );
    sf_btn.tip.set_tooltip_func(|tooltip: &mut Tooltip, g: &Gfx, ()| {
        tooltip.clear(false);
        tooltip.add_line(g, 16.0, Some("^7This contains a list of your fallback nodes along with their primary, secondary, and minimum weights."));
        tooltip.add_line(g, 16.0, Some("^7This list can be updated manually or by selecting the node you want to update via the search dropdown list and then moving the node weight sliders."));
        tooltip.add_line(g, 16.0, Some("^7Fallback node weights are only used when no matching entry exists in the desired nodes list, allowing you to override or disable specific automatic weights."));
        tooltip.add_line(g, 16.0, Some("^7Fallback node weights typically contain automatically generated stat weights based on your current build."));
        tooltip.add_line(g, 16.0, Some("^7Any manual changes made to your fallback nodes are lost when you click the generate button, as it completely replaces them."));
    });
    sf_btn.locked.set_fn(|g| ctl::<EditControl>("searchListFallback").base.shown.get(g));
    reg("searchListFallbackButton", sf_btn);
    // :2216-2219 the results
    let results: Rc<ListControl<Row>> = ListControl::new(
        anchor_nil("BOTTOMLEFT", "BOTTOMLEFT"),
        rect(edge * 2.0 + list_width, -(button_height + edge * 2.0), list_width, list_height),
        16.0,
        Some("VERTICAL"),
        false,
        t.timeless.results.borrow().clone(),
        false,
        Box::new(ListH),
    );
    let rs: ControlRef = results.clone();
    reg("searchResults", results);
    reg("msg", LabelControl::new(None, rect(-280.0, 5.0, 0.0, 20.0), String::new()));
    // :2220-2427 the trade block
    let trade = ButtonControl::new(
        anchor("BOTTOMRIGHT", &rs, "TOPRIGHT"),
        rect(0.0, -row_spacing, 170.0, button_height),
        "Open Trade URL".to_string(),
        open_trade_url,
    );
    trade.base.enabled.set(!t.timeless.results.borrow().is_empty());
    trade.tip.set_tooltip_func(|tooltip: &mut Tooltip, g: &Gfx, ()| {
        tooltip.clear(false);
        tooltip.add_line(
            g,
            16.0,
            Some("^7Click to generate and open a trade URL for searching for jewels in this list."),
        );
        tooltip.add_line(g, 16.0, Some("^7Paste the URL in a web browser to search."));
        tooltip.add_line(g, 16.0, Some(""));
        tooltip.add_line(g, 16.0, Some("^7You can click to select a row so that search begins from there."));
        tooltip.add_line(
            g,
            16.0,
            Some("^7After selecting a row You can also shift+click on another row to select a range of rows to search."),
        );
    });
    let tr: ControlRef = trade.clone();
    reg("searchTradeButton", trade);
    let league = DropDownControl::new(
        g,
        anchor("RIGHT", &tr, "LEFT"),
        rect(-label_spacing, 0.0, 140.0, button_height),
        Vec::new(),
        Some(Rc::new(|_g: &mut Gfx, _index, v: &DropItem| {
            *tab().timeless.league_select.borrow_mut() = v.label.clone();
        })),
        None,
        false,
    );
    let lr: ControlRef = league.clone();
    reg("searchTradeLeagueSelect", league);
    label(
        "searchTradeLeagueLabel",
        anchor("TOPRIGHT", &lr, "TOPLEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7League:",
    );
    let realm = DropDownControl::new(
        g,
        anchor("BOTTOMLEFT", &lr, "TOPLEFT"),
        rect(0.0, -row_spacing, 50.0, button_height),
        drop_items(["PC", "Sony", "Xbox"].map(str::to_string)),
        Some(Rc::new(|g: &mut Gfx, index, _v: &DropItem| {
            tab().timeless.selected_realm_index.set(Some(index));
            update_leagues(g);
        })),
        None,
        false,
    );
    let rr: ControlRef = realm.clone();
    reg("realmSelection", realm.clone());
    // :2388-2392 the previous choice; `selFunc` by hand: it does not run when the selection stays
    realm.set_sel(g, t.timeless.selected_realm_index.get().unwrap_or(1) as i64, false);
    tab().timeless.selected_realm_index.set(Some(realm.sel_index.get()));
    update_leagues(g);
    label(
        "realmLabel",
        anchor("TOPRIGHT", &rr, "TOPLEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Realm:",
    );
    let trade_types = DropDownControl::new(
        g,
        anchor("BOTTOMRIGHT", &tr, "TOPRIGHT"),
        rect(0.0, -row_spacing, 205.0, button_height),
        drop_items(
            [
                "Instant buyout",
                "Instant buyout and in person",
                "In person (online in league)",
                "In person (online)",
                "Any (includes offline)",
            ]
            .map(str::to_string),
        ),
        Some(Rc::new(|_g: &mut Gfx, index, _v: &DropItem| {
            tab().timeless.trade_type_index.set(Some(index))
        })),
        None,
        false,
    );
    trade_types.sel_index.set(t.timeless.trade_type_index.get().unwrap_or(1));
    t.timeless.trade_type_index.set(Some(trade_types.sel_index.get()));
    let ttr: ControlRef = trade_types.clone();
    reg("tradeTypeSelection", trade_types);
    let search_more = CheckBoxControl::new(
        g,
        anchor("BOTTOMRIGHT", &ttr, "TOPRIGHT"),
        [0.0, -row_spacing, 19.0],
        None,
        Some(Rc::new(|_g: &mut Gfx, state| tab().timeless.last_search_more.set(state))),
        Some(
            "By default, this tool only searches for 4 or 10 (depending on conqueror) seeds at once to preserve sorting.\nThis option searches up to 180 or 60 seeds at once if you don't care about the sorting.",
        ),
        false,
    );
    if t.timeless.last_search_more.get() {
        search_more.state.set(true);
    }
    let smr: ControlRef = search_more.clone();
    reg("searchMore", search_more);
    label(
        "searchMoreLabel",
        anchor("RIGHT", &smr, "LEFT"),
        rect(-label_spacing, 0.0, 0.0, label_height),
        "^7Search Maximum Amount:",
    );
    // :2790-2875
    let panel_width = edge * 3.0 + list_width * 2.0;
    let (divider, button_width) = (10.0, 80.0);
    let reset = ButtonControl::new(
        anchor_nil("BOTTOMLEFT", "BOTTOMLEFT"),
        rect(panel_width / 2.0 - button_width / 2.0, -edge, button_width, button_height),
        "Reset".to_string(),
        |g| {
            update_search_list(g, "", true);
            update_search_list(g, "", false);
            ctl::<DropDownControl>("abyssAscendancySelect").sel_index.set(1);
            tab().timeless.results.borrow_mut().clear();
            ctl::<ListControl<Row>>("searchResults").list.borrow_mut().clear();
            ctl::<ButtonControl>("searchTradeButton").base.enabled.set(false);
            set_allocated_nodes(g);
        },
    );
    let rb: ControlRef = reset.clone();
    reg("resetButton", reset);
    reg(
        "closeButton",
        ButtonControl::new(
            anchor("LEFT", &rb, "RIGHT"),
            rect(divider, 0.0, button_width, button_height),
            "Cancel".to_string(),
            |_g| {
                main().close_popup();
            },
        ),
    );
    reg(
        "searchButton",
        ButtonControl::new(
            anchor("RIGHT", &rb, "LEFT"),
            rect(-divider, 0.0, button_width, button_height),
            "Search".to_string(),
            on_search,
        ),
    );
    let panel_height = 565.0 + row_spacing + row_height;
    let controls = BUILDING.with_borrow_mut(std::mem::take);
    let popup = main().open_popup(
        g,
        panel_width,
        panel_height,
        "Find a Timeless Jewel",
        controls,
        None,
        None,
        None,
        None,
    );
    POPUP.with_borrow_mut(|p| *p = Some(popup));
}

/// `TimelessJewelSocketControl:Draw` (TimelessJewelSocketControl.lua:25-58) after `DropDownControl.Draw`.
fn install_socket_viewer(drop: &Rc<DropDownControl>, ids: Rc<Vec<f64>>) {
    let root = crate::launch::launch().cfg.script_path.clone();
    let viewer = RefCell::new(crate::tree_view::PassiveTreeView::new(&root));
    let w = Rc::downgrade(drop);
    *drop.post_draw.borrow_mut() = Some(Rc::new(move |f: &mut Frame<'_>, view_port: &Rect, no_tooltip: bool| {
        let drop = w.upgrade().expect("socket control");
        let (x, y) = drop.get_pos(f);
        drop.host.draw_controls(f, view_port, None);
        if drop.is_mouse_over(f) != Some(true) || no_tooltip || main().popups.borrow().len() > 1 {
            return;
        }
        let hovered = usize::try_from(drop.get_hover_index()).unwrap_or(0);
        let id = ids[hovered - 1];
        let b = main().build_mode.clone();
        let spec = b.engine.borrow().as_ref().expect("engine").spec_rc();
        let Some(node) = (id >= 0.0).then(|| spec.node(id as u32)).flatten() else {
            return;
        };
        sg::set_draw_layer(f, None, 15);
        let viewer_y = if drop.drop_up.get() && drop.dropped.get() {
            y + 20.0
        } else {
            f64::min(y - 150.0 - 5.0, view_port.y + view_port.height - 154.0)
        };
        let viewer_x = x + 215.0;
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_rect(f, viewer_x, viewer_y, 304.0, 304.0);
        let mut v = viewer.borrow_mut();
        v.zoom = 5.0;
        let scale = v.art(&crate::launch::launch().cfg.script_path, &spec.tree).size / 1500.0;
        let tn = node.tree_node();
        v.zoom_x = -tn.x.expect("x") / scale;
        v.zoom_y = -tn.y.expect("y") / scale;
        sg::set_viewport(f, Some([viewer_x + 2.0, viewer_y + 2.0, 300.0, 300.0]));
        let mut events = crate::host::InputEvents::default();
        let root = crate::launch::launch().cfg.script_path.clone();
        v.draw(
            f,
            &root,
            &b.engine,
            &b.build_flag,
            &b.mod_flag,
            &Rect {
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 300.0,
            },
            &mut events,
        );
        sg::set_draw_layer(f, None, 30);
        sg::set_draw_color_a(f, 1.0, 1.0, 1.0, 0.2);
        sg::draw_rect(f, 149.0, 0.0, 2.0, 300.0);
        sg::draw_rect(f, 0.0, 149.0, 300.0, 2.0);
        sg::set_viewport(f, None);
        sg::set_draw_layer(f, None, 0);
    }));
}

/// `searchResults.tooltip` for the gate of the items tab.
pub fn debug_results_tooltip(c: &ControlRef) -> Option<Vec<String>> {
    let any: &dyn std::any::Any = &**c;
    let l = any.downcast_ref::<ListControl<Row>>()?;
    Some(l.tooltip.borrow().lines.iter().map(|x| x.text.clone().unwrap_or_else(|| "<sep>".to_string())).collect())
}

/// The `timeless` part of the `tabDump` of tree-runner.lua.
pub fn debug_dump(g: &Gfx) -> serde_json::Value {
    use serde_json::json;
    let labels = |n: &str| -> Vec<String> {
        ctl::<DropDownControl>(n)
            .list
            .borrow()
            .iter()
            .map(|v| v.label.clone().unwrap_or_default())
            .collect()
    };
    let sel = |n: &str| ctl::<DropDownControl>(n).sel_index.get();
    let text = |n: &str| ctl::<LabelControl>(n).label.get(g);
    let list = ctl::<ListControl<Row>>("searchResults");
    let results: Vec<String> = list.list.borrow().iter().map(|r| r.0.label.borrow().clone()).collect();
    let (data, n_box) = (td(|t| t.clone()), local(|l| l.box_count));
    let boxes: Vec<String> = (1..=n_box)
        .map(|i| {
            ctl::<CheckBoxControl>(&format!("protectAllocatedBox{i}"))
                .label
                .get(g)
                .unwrap_or_default()
        })
        .collect();
    let trade_url = local(|l| l.last_trade_url.borrow().clone()).map(|u| trade_url_parts(&u));
    let leagues = Some(labels("searchTradeLeagueSelect")).filter(|l| !l.is_empty());
    let msg = Some(text("msg")).filter(|m| !m.is_empty());
    json!({
        "jewelSel": sel("jewelSelect"),
        "conquerorSel": sel("conquerorSelect"),
        "conquerors": labels("conquerorSelect"),
        "socketSel": sel("socketSelect"),
        "sockets": labels("socketSelect"),
        "nodeSel": sel("nodeSelect"),
        "nodes": labels("nodeSelect"),
        "fallbackModes": labels("fallbackWeightsList"),
        "slider1": text("nodeSliderValue"),
        "slider2": text("nodeSlider2Value"),
        "slider3": text("nodeSlider3Value"),
        "slider2Label": text("nodeSlider2Label"),
        "slider2Enabled": ctl::<SliderControl>("nodeSlider2").base.enabled.get(g),
        "searchList": ctl::<EditControl>("searchList").buf.borrow().clone(),
        "searchListFallback": ctl::<EditControl>("searchListFallback").buf.borrow().clone(),
        "fallbackShown": fallback_shown(g),
        "results": results,
        "resultSel": list.sel_index.get().map_or(json!("nil"), |v| json!(v)),
        "highlight": local(|l| l.highlight.get()).map_or(json!("nil"), |v| json!(v)),
        "tradeEnabled": ctl::<ButtonControl>("searchTradeButton").base.enabled.get(g),
        "tradeLabel": ctl::<ButtonControl>("searchTradeButton").label.get(g),
        "tradeUrl": trade_url,
        "leagues": leagues,
        "msg": msg,
        "protect": boxes,
        "filter": ctl::<CheckBoxControl>("socketFilter").state.get(),
        "distance": data.socket_filter_distance,
        "jewelSocket": data.jewel_socket_id,
    })
}
