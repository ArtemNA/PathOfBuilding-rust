//! `Modules/ConfigModBrowser.lua`: the "Add Mod" popup of a custom modifier group.

use std::collections::{BTreeSet, HashMap};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_gfx::Gfx;
use serde_json::Value as Json;

use super::config::ConfigTab;
use crate::colors::color_code;
use crate::common::wrap_string;
use crate::control::{Control, ControlRef, anchor, anchor_nil, rect};
use crate::controls::basic::{ButtonControl, LabelControl};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::list::{ByRef, ListControl, ListHooks};
use crate::launch::launch;
use crate::lua_data::{LuaVal, parse_return};
use crate::modules::main::main;
use crate::tooltip::Tooltip;
use crate::undo::UndoHost;

thread_local! {
    /// The gate: `pairs(value.sources)` of the oracle's process (R13); the app shows them sorted.
    static SOURCE_ORDER: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// The order the row tooltip lists its sources in, from a sample of the oracle.
pub fn set_source_order_sample(order: Option<Vec<String>>) {
    SOURCE_ORDER.with_borrow_mut(|o| *o = order);
}

const NO_MATCH_TEXT: &str = "No matching modifiers found";

const HELP_TEXT: &str = "This menu currently contains supported mod lines from tree nodes and item modifiers only.

This menu is not a representation of what PoB can parse, and this is only a limited list of mods. Additionally, some mods might be local to items and might not function in custom modifiers.

A mod being supported does not necessarily mean that it will be included in calculations, and only means that the mod parser accepts it.";

/// An entry of `modSet`.
#[derive(Clone, Debug, Default)]
pub struct ModEntry {
    pub text: String,
    /// `sources`: a set (the Lua's `pairs` order is the process's, R13).
    pub sources: BTreeSet<String>,
    pub supported: bool,
    pub template: String,
    pub sort_key: String,
    pub rank: u8,
}

fn gsub(s: &str, pat: &[u8], repl: &[u8]) -> String {
    let out = pob_lua::gsub(s.as_bytes(), pat, pob_lua::Repl::Str(repl), None)
        .expect("ConfigModBrowser pattern")
        .0;
    String::from_utf8_lossy(&out).into_owned()
}

/// :39-49 `getModTemplate`.
fn mod_template(text: &str) -> String {
    let a = gsub(text, b"([%+-]?)%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)", b"%1#");
    gsub(&a, b"%d+%.?%d*", b"#").to_ascii_lowercase()
}

/// :56-75 `addModLine`.
fn add_mod_line(set: &mut HashMap<String, ModEntry>, line: &str, source: &str, supported: Option<bool>) {
    let template = mod_template(line);
    let e = set.entry(template).or_default();
    let fresh = e.sources.is_empty();
    if fresh {
        e.text = line.to_string();
    }
    e.sources.insert(source.to_string());
    if !fresh && line < e.text.as_str() && (supported == Some(true) || !e.supported) {
        e.text = line.to_string();
    }
    if let Some(s) = supported {
        if s && !e.supported {
            e.text = line.to_string();
        }
        e.supported = e.supported || s;
    }
}

/// `(mods and not extra)`.
fn supported_line(line: &str) -> bool {
    let p = pob_parser::parse_mod_line(line, false, pob_parser::ParseMode::Full);
    p.mod_list.is_some() && p.extra.is_none()
}

/// :81-92 `addItemModLine`, with its session cache.
fn add_item_mod_line(set: &mut HashMap<String, ModEntry>, cache: &mut HashMap<String, (String, bool)>, line: &str, source: &str) {
    let (ranged, supported) = cache.entry(line.to_string()).or_insert_with(|| {
        let ranged = pob_items::apply_range::apply_range(line, 0.5, None, None, pob_parser::ParseMode::Full);
        let s = supported_line(&ranged);
        (ranged, s)
    });
    add_mod_line(set, ranged, source, Some(*supported));
}

/// :96-112 `addItemMod`, from a JSON record of `data/`.
fn add_item_mod(set: &mut HashMap<String, ModEntry>, cache: &mut HashMap<String, (String, bool)>, m: &Json, source: &str) {
    if let Some(t) = m.get("tradeHashes").and_then(Json::as_object) {
        for desc in t.values() {
            let parts: Vec<&str> = desc.as_array().into_iter().flatten().filter_map(Json::as_str).collect();
            add_item_mod_line(set, cache, &parts.join(" "), source);
        }
    } else {
        for line in m
            .get("lines")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
        {
            add_item_mod_line(set, cache, line, source);
        }
    }
}

/// `data.beastCraft`: not among the converted `data/` files; Data/BeastCraft.lua is a table of constants.
fn add_beast_craft(set: &mut HashMap<String, ModEntry>, cache: &mut HashMap<String, (String, bool)>) {
    let path = format!("{}Data/BeastCraft.lua", launch().cfg.script_path);
    let src = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let LuaVal::Table(t) = parse_return(&src).unwrap_or_else(|e| panic!("{path}: {e}")) else {
        panic!("{path}: no table");
    };
    for (_, m) in &t.fields {
        let LuaVal::Table(m) = m else { continue };
        if let Some(h) = m.tab("tradeHashes") {
            for (_, desc) in &h.fields {
                let LuaVal::Table(d) = desc else { continue };
                let parts: Vec<&str> = d.seq().iter().filter_map(|v| v.as_str()).collect();
                add_item_mod_line(set, cache, &parts.join(" "), "Item mod");
            }
        } else {
            for line in m.seq().iter().filter_map(|v| v.as_str()) {
                add_item_mod_line(set, cache, line, "Item mod");
            }
        }
    }
}

/// :167-235 the supported mods, sorted (:237). The build's tree is `specList[activeSpec].tree`.
fn supported_list(tab: &ConfigTab) -> Vec<Rc<ModEntry>> {
    let mut set: HashMap<String, ModEntry> = HashMap::new();
    let mut cache: HashMap<String, (String, bool)> = HashMap::new();
    let b = tab.build();
    let tree = {
        let eng = b.engine.borrow();
        eng.as_ref().expect("engine").spec_rc().tree.clone()
    };
    let raw = pob_data::tree::load_tree(&tree.tree_version).unwrap_or_else(|e| panic!("{e}"));
    for node in &tree.nodes {
        if node.ty == pob_tree::tree::NodeType::Mastery && !node.mastery_effect_ids.is_empty() {
            for id in &node.mastery_effect_ids {
                if let Some(effect) = tree.mastery_effects.get(&(*id as u64)) {
                    for line in &effect.sd {
                        add_mod_line(&mut set, line, "Mastery Node", Some(supported_line(line)));
                    }
                }
            }
            continue;
        }
        let mods = &node.stats.mods;
        let prefix = if raw.nodes.get(&node.id.to_string()).is_some_and(|n| n.is_bloodline == Some(true)) {
            "Bloodline "
        } else if node.ascendancy_name.is_some() {
            "Ascendancy "
        } else {
            ""
        };
        let mut i = 0;
        while i < node.sd.len() {
            let mut combined = node.sd[i].clone();
            while let Some(Some(next)) = mods.get(i + 1) {
                if next.combined {
                    combined = format!("{combined} {}", node.sd[i + 1]);
                    i += 1;
                } else {
                    break;
                }
            }
            let supported = mods
                .get(i)
                .and_then(Option::as_ref)
                .is_some_and(|m| m.list.is_some() && m.extra.is_none());
            add_mod_line(&mut set, &combined, &format!("{prefix}{} Node", node.ty.as_str()), Some(supported));
            i += 1;
        }
    }
    let read = |rel: &str| -> Json { pob_data::read_json(rel).unwrap_or_else(|e| panic!("{e}")) };
    for m in read("master-mods.json").as_array().into_iter().flatten() {
        add_item_mod(&mut set, &mut cache, m, "Crafted mod");
    }
    // `pairs(bData.itemMods)` without `Item` and `JewelCluster` (Data.lua:649-668)
    for cat in [
        "Explicit",
        "ItemExclusive",
        "Corrupted",
        "Delve",
        "Synthesis",
        "Scourge",
        "Eldritch",
        "Flask",
        "Tincture",
        "Graft",
        "Jewel",
        "JewelAbyss",
        "JewelCharm",
        "Foulborn",
        "Mercenary",
        // Data.lua:1496
        "WatchersEye",
    ] {
        let v = read(&format!("item-mods/{cat}.json"));
        for m in v.as_object().into_iter().flat_map(|o| o.values()) {
            add_item_mod(&mut set, &mut cache, m, "Item mod");
        }
        // Vestigial: ItemExclusive `^Divergent` (:669-673), the same mods again
        if cat == "ItemExclusive" {
            for (id, m) in v.as_object().into_iter().flatten() {
                if id.starts_with("Divergent") {
                    add_item_mod(&mut set, &mut cache, m, "Item mod");
                }
            }
        }
    }
    let v = read("veiled-mods.json");
    for m in v.as_object().into_iter().flat_map(|o| o.values()) {
        add_item_mod(&mut set, &mut cache, m, "Veiled mod");
    }
    add_beast_craft(&mut set, &mut cache);
    let mut out = Vec::new();
    for (template, mut e) in set {
        let k = gsub(&template.replace('#', " "), b"[^%a]+", b" ");
        e.sort_key = k.trim_matches(' ').to_string();
        e.template = template;
        if e.supported {
            out.push(Rc::new(e));
        }
    }
    out.sort_by(|a, b| sort_cmp(a, b));
    out
}

/// :113-121 `sortAlphabeticallyIgnoringValues`.
fn sort_cmp(a: &ModEntry, b: &ModEntry) -> std::cmp::Ordering {
    a.sort_key
        .as_bytes()
        .cmp(b.sort_key.as_bytes())
        .then_with(|| a.template.as_bytes().cmp(b.template.as_bytes()))
        .then_with(|| a.text.as_bytes().cmp(b.text.as_bytes()))
}

/// :5-38 `fuzzyScore`.
fn fuzzy_score(text: &str, search: &str, words: &[String]) -> Option<u8> {
    let lower = text.to_ascii_lowercase();
    if lower.contains(search) {
        return Some(1);
    }
    if words.len() > 1 && words.iter().all(|w| lower.contains(w.as_str())) {
        return Some(2);
    }
    if words.len() == 1 && search.len() >= 3 {
        let text_words: Vec<&str> = lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        for (i, w) in text_words.iter().enumerate() {
            for len1 in 2..search.len() {
                if !search.is_char_boundary(len1) {
                    continue;
                }
                let (p1, p2) = search.split_at(len1);
                if w.starts_with(p1) && text_words.get(i + 1).is_some_and(|n| n.starts_with(p2)) {
                    return Some(3);
                }
            }
        }
    }
    None
}

/// :124-161 `updateDisplayList`.
fn display_list(search: &str, mods: &[Rc<ModEntry>]) -> Vec<Rc<ModEntry>> {
    let lower = search.to_ascii_lowercase();
    let s = lower.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'));
    let mut out: Vec<Rc<ModEntry>> = Vec::new();
    if s.is_empty() {
        out.extend(mods.iter().cloned());
    } else {
        let words: Vec<String> = s.split_ascii_whitespace().map(str::to_owned).collect();
        for m in mods {
            if let Some(rank) = fuzzy_score(&m.text, s, &words) {
                let mut e = (**m).clone();
                e.rank = rank;
                out.push(Rc::new(e));
            }
        }
        out.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| sort_cmp(a, b)));
    }
    if out.is_empty() {
        out.push(Rc::new(ModEntry {
            text: NO_MATCH_TEXT.to_string(),
            ..ModEntry::default()
        }));
    }
    out
}

struct BrowserHooks {
    save: Rc<std::cell::RefCell<Option<Rc<ButtonControl>>>>,
}

type Row = ByRef<ModEntry>;

impl ListHooks<Row> for BrowserHooks {
    /// :245-251
    fn get_row_value(&self, _l: &ListControl<Row>, _g: &Gfx, _column: usize, _index: usize, v: &Row) -> String {
        if v.0.text == NO_MATCH_TEXT {
            v.0.text.clone()
        } else {
            format!("{}{}", color_code("MAGIC").expect("MAGIC"), v.0.text)
        }
    }

    fn has_add_value_tooltip(&self) -> bool {
        true
    }

    /// :253-260
    fn add_value_tooltip(&self, _l: &ListControl<Row>, g: &Gfx, tooltip: &mut Tooltip, _index: usize, v: &Row) {
        tooltip.clear(true);
        let mut lines: Vec<String> = v.0.sources.iter().map(|s| format!("^7{s}")).collect();
        SOURCE_ORDER.with_borrow(|o| {
            if let Some(o) = o {
                lines.sort_by_key(|l| o.iter().position(|x| x == l));
            }
        });
        for l in &lines {
            tooltip.add_line(g, 14.0, Some(l));
        }
    }

    /// :261-266
    fn on_sel_click(&self, l: &ListControl<Row>, g: &mut Gfx, index: usize, _v: &Row, double_click: bool) -> Option<bool> {
        l.select_index(g, Some(index));
        if double_click {
            let save = self.save.borrow().clone().expect("save");
            if save.is_enabled(g) {
                save.click(g);
            }
        }
        None
    }
}

/// The Mod Browser's part of the gate's `popup`.
pub fn debug_browser(g: &Gfx, p: &crate::controls::popup::PopupDialog, c: ControlRef, o: &mut serde_json::Map<String, Json>) {
    let any: Rc<dyn std::any::Any> = c;
    let l = any.downcast::<ListControl<Row>>().expect("listControl");
    let rows: Vec<String> = l
        .list
        .borrow()
        .iter()
        .take(40)
        .enumerate()
        .map(|(i, v)| l.hooks.get_row_value(&l, g, 1, i + 1, v))
        .collect();
    o.insert("rows".into(), serde_json::json!(rows));
    o.insert("count".into(), serde_json::json!(l.list.borrow().len()));
    o.insert("selIndex".into(), serde_json::json!(l.sel_index.get().unwrap_or(0)));
    o.insert(
        "saveEnabled".into(),
        serde_json::json!(p.host.get("save").is_some_and(|s| s.is_enabled(g))),
    );
    let mut tip: Vec<String> = l
        .tooltip
        .borrow()
        .lines
        .iter()
        .map(|x| x.text.clone().unwrap_or_else(|| "<sep>".into()))
        .collect();
    tip.sort();
    o.insert("tip".into(), serde_json::json!(tip));
}

/// `rowsOut` of the gate: every row of the open list.
pub fn debug_all_rows(g: &Gfx, c: ControlRef) -> Vec<String> {
    let any: Rc<dyn std::any::Any> = c;
    let l = any.downcast::<ListControl<Row>>().expect("listControl");
    let list = l.list.borrow();
    list.iter()
        .enumerate()
        .map(|(i, v)| l.hooks.get_row_value(&l, g, 1, i + 1, v))
        .collect()
}

fn wrap_rows(list: Vec<Rc<ModEntry>>) -> Vec<Row> {
    list.into_iter().map(ByRef).collect()
}

/// :164-319 `M.OpenAddModPopup`.
pub fn open_add_mod_popup(g: &mut Gfx, tab: Weak<ConfigTab>, block: usize) {
    let t = tab.upgrade().expect("configTab");
    let supported = Rc::new(supported_list(&t));
    let save_cell: Rc<std::cell::RefCell<Option<Rc<ButtonControl>>>> = Rc::new(std::cell::RefCell::new(None));
    let list = ListControl::new(
        anchor_nil("TOPLEFT", "TOPLEFT"),
        rect(10.0, 20.0, 700.0, 454.0),
        16.0,
        Some("VERTICAL"),
        false,
        wrap_rows(display_list("", &supported)),
        false,
        Box::new(BrowserHooks { save: save_cell.clone() }),
    );
    list.force_tooltip.set(true);
    list.font.set("VAR");
    let select_first = {
        let list = Rc::downgrade(&list);
        move || {
            let l = list.upgrade().expect("list");
            l.sel_index.set(Some(1));
            let first = l.list.borrow().first().cloned();
            *l.sel_value.borrow_mut() = first;
            l.scroll_bar_v.offset.set(0.0);
        }
    };
    select_first();
    let search_label = LabelControl::new(
        anchor_nil("TOPRIGHT", "TOPLEFT"),
        rect(65.0, 482.0, 0.0, 16.0),
        "^7Search:".to_string(),
    );
    let (lw, sup) = (Rc::downgrade(&list), supported.clone());
    let search = EditControl::new(
        g,
        anchor_nil("TOPLEFT", "TOPLEFT"),
        rect(70.0, 482.0, 640.0, 18.0),
        EditArgs {
            init: Some(String::new()),
            filter: Some("%c".into()),
            limit: Some(100),
            change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _p: bool| {
                let l = lw.upgrade().expect("list");
                *l.list.borrow_mut() = wrap_rows(display_list(buf, &sup));
                select_first();
            })),
            clearable: true,
            ..EditArgs::default()
        },
    );
    let s = Rc::downgrade(&search);
    search
        .button_clear
        .as_ref()
        .expect("clearable")
        .base
        .shown
        .set_fn(move |_| !s.upgrade().expect("search").buf.borrow().is_empty());
    let help_size = 24.0;
    let what = ButtonControl::new(
        anchor_nil("BOTTOM", "BOTTOM"),
        rect(0.0, -10.0, help_size, help_size),
        "?".to_string(),
        |_g| {},
    );
    what.force_tooltip.set(true);
    what.tip.tooltip_text.set(Some(wrap_string(g, HELP_TEXT, 16.0, 270.0).join("\n")));
    let what_r: ControlRef = what.clone();
    let (lw, tb) = (Rc::downgrade(&list), tab.clone());
    let save = ButtonControl::new(
        anchor("BOTTOMRIGHT", &what_r, "TOP"),
        rect(-2.0, -4.0, 80.0, 20.0),
        "Add".to_string(),
        move |g| {
            let l = lw.upgrade().expect("list");
            let t = tb.upgrade().expect("configTab");
            let sel = l.sel_index.get().unwrap_or(1);
            let selected = l.list.borrow().get(sel - 1).cloned();
            if let Some(e) = selected
                && e.0.text != NO_MATCH_TEXT
            {
                let mut text = t.block_text(block);
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&e.0.text);
                t.edit_block(block, |b| b.text = pob_build::BlockText::Str(text));
                t.update_custom_mods_controls(g);
                t.add_undo_state(false);
                t.build_mod_list(g);
                t.build().build_flag.set(true);
            }
            main().close_popup();
        },
    );
    let lw = Rc::downgrade(&list);
    save.base.enabled.set_fn(move |_| {
        let l = lw.upgrade().expect("list");
        let sel = l.sel_index.get().unwrap_or(1);
        let x = l.list.borrow();
        x.get(sel - 1).is_some_and(|e| e.0.text != NO_MATCH_TEXT)
    });
    *save_cell.borrow_mut() = Some(save.clone());
    let close = ButtonControl::new(
        anchor("BOTTOMLEFT", &what_r, "TOP"),
        rect(2.0, -4.0, 80.0, 20.0),
        "Cancel".to_string(),
        |_g| main().close_popup(),
    );
    // :316 `controls.search.y + controls.search.height + helpSize - controls.whatDoesItDo.y - controls.close.y + controls.close.height + 8`
    let popup_height = 482.0 + 18.0 + help_size + 10.0 + 4.0 + 20.0 + 8.0;
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert("listControl".into(), list);
    controls.insert("searchLabel".into(), search_label);
    controls.insert("search".into(), search);
    controls.insert("whatDoesItDo".into(), what);
    controls.insert("save".into(), save);
    controls.insert("close".into(), close);
    main().open_popup(
        g,
        720.0,
        popup_height,
        "Mod Browser",
        controls,
        Some("save"),
        Some("search"),
        Some("close"),
        None,
    );
}
