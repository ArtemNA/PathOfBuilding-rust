//! `Modules/AddImplicitPopup.lua`: the `Add Implicit...` popup of the display item.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use indexmap::IndexMap;
use pob_calc::power::PowerStat;
use pob_gfx::Gfx;
use pob_items::Item;
use pob_items::craft::AffixMod;
use pob_items::item::ModLine;

use super::item_popups::{copy_of, mod_sort_list};
use super::items::ItemsTab;
use crate::control::{Control, ControlRef, anchor_nil, rect};
use crate::controls::basic::{ButtonControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::modules::main::main;
use crate::tooltip::Tooltip;

/// A `listMod`; of a vestigial source it is its group too (:153-157).
#[derive(Clone)]
struct ListMod {
    label: String,
    m: AffixMod,
    /// `listMod.type`: `exarch`, `eater`, `vestigial`.
    ty: &'static str,
    unique_title: Option<String>,
    /// `defaultOrder`: the mod's id.
    id: String,
    default_sort_order: usize,
    sort_values: HashMap<String, f64>,
    sort_value: f64,
}

/// An entry of `modGroups`.
#[derive(Clone)]
struct ModGroup {
    label: String,
    m: AffixMod,
    mod_list_index: usize,
    unique_title: Option<String>,
    vestigial: bool,
    default_sort_order: usize,
    sort_value: f64,
}

#[derive(Default)]
struct Lists {
    /// `modList`.
    mods: Vec<Vec<ListMod>>,
    /// `modGroups`.
    groups: Vec<ModGroup>,
}

/// The value of a row of `modGroupSelect` and `modSelect`.
struct Row {
    m: AffixMod,
    unique_title: Option<String>,
    vestigial: bool,
}

fn row_of(d: &DropDownControl, index: usize) -> Option<Rc<Row>> {
    let list = d.list.borrow();
    let v = list.get(index.wrapping_sub(1))?;
    v.data.clone()?.downcast::<Row>().ok()
}

/// `tonumber(defaultOrder:match("%d+$"))`.
fn trailing_number(id: &str) -> f64 {
    let digits = id.len() - id.bytes().rev().take_while(u8::is_ascii_digit).count();
    id[digits..].parse().expect("a mod id ending in digits")
}

fn group_has(m: &AffixMod, what: &str) -> bool {
    m.group.as_deref().is_some_and(|g| g.contains(what))
}

/// `buildMods(sourceId)` (:45-160).
fn build_mods(source_id: &str, item: &Item) -> Lists {
    let mut out = Lists::default();
    let mut group_indexes: HashMap<String, usize> = HashMap::new();
    if source_id == "EXARCH" || source_id == "EATER" {
        let none = HashSet::new();
        let ty: &'static str = if source_id == "EXARCH" { "exarch" } else { "eater" };
        for (id, m) in item.affix_mods().into_iter().flatten() {
            if item.get_mod_spawn_weight(m, &none, &none, None) > 0.0 && m.ty.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(source_id))
            {
                let label = m.lines.join("/");
                let group = m
                    .group
                    .clone()
                    .unwrap_or_default()
                    .replace("PinnaclePresence", "")
                    .replace("UniquePresence", "");
                let at = *group_indexes.entry(group).or_insert_with(|| {
                    out.mods.push(Vec::new());
                    out.groups.push(ModGroup {
                        label: label.clone(),
                        m: m.clone(),
                        mod_list_index: out.mods.len() - 1,
                        unique_title: None,
                        vestigial: false,
                        default_sort_order: 0,
                        sort_value: 0.0,
                    });
                    out.mods.len() - 1
                });
                out.mods[at].push(ListMod {
                    label,
                    m: m.clone(),
                    ty,
                    unique_title: None,
                    id: id.clone(),
                    default_sort_order: 0,
                    sort_values: HashMap::new(),
                    sort_value: 0.0,
                });
            }
        }
        pob_engine::lua::lua_sort(&mut out.groups, &mut |a, b| {
            let (x, y) = (&a.m, &b.m);
            for i in 0..x.lines.len().max(y.lines.len()) {
                if i >= x.lines.len() {
                    return true;
                } else if i >= y.lines.len() {
                    return false;
                } else if x.stat_order[i] != y.stat_order[i] {
                    return x.stat_order[i] < y.stat_order[i];
                }
            }
            x.level > y.level
        });
        for list in &mut out.mods {
            pob_engine::lua::lua_sort(list, &mut |a, b| {
                let (x, y) = (&a.m, &b.m);
                if x.group != y.group {
                    if group_has(x, "PinnaclePresence") {
                        return false;
                    } else if group_has(y, "PinnaclePresence") {
                        return true;
                    }
                    return !group_has(x, "UniquePresence");
                }
                // :102-115: the loop of the original ends in its first round
                match (x.lines.is_empty(), y.lines.is_empty()) {
                    (true, true) => x.level > y.level,
                    (true, false) => true,
                    (false, true) => false,
                    (false, false) if x.stat_order[0] != y.stat_order[0] => x.stat_order[0] < y.stat_order[0],
                    (false, false) => trailing_number(&a.id) < trailing_number(&b.id),
                }
            });
        }
        for g in &mut out.groups {
            let l = &out.mods[g.mod_list_index][0].label;
            let l = pob_items::lua::gsub(l, "%([%d%.]+%-[%d%.]+%)", "#", None);
            g.label = pob_items::lua::gsub(&l, "[%d%.]+", "#", None);
        }
    } else if source_id == "VESTIGIAL" {
        let display_title = item.title.as_deref().unwrap_or("").to_lowercase();
        let color = crate::colors::color_code("VESTIGIAL").expect("VESTIGIAL");
        let exclusive = pob_items::build_raw::item_exclusive_mods();
        let by_title = unique_by_title();
        for (id, unique_title) in pob_data::rules::rules().vestigial_mod_mappings.iter() {
            let title_lower = unique_title.to_lowercase();
            let Some(unique) = by_title.get(&title_lower) else { continue };
            // vestigial implicits can only end up on the same item type; not the item's own mods
            if unique.base.map(|b| b.ty.as_str()) != item.ty.as_deref() || display_title == title_lower {
                continue;
            }
            let m = exclusive.get(id).unwrap_or_else(|| panic!("data.itemMods.Vestigial[{id}]")).clone();
            out.mods.push(vec![ListMod {
                label: format!("{color}{}", m.lines.join("/")),
                m,
                ty: "vestigial",
                unique_title: Some(unique_title.clone()),
                id: id.clone(),
                default_sort_order: 0,
                sort_values: HashMap::new(),
                sort_value: 0.0,
            }]);
        }
        // `a.group == b.group`: a `listMod` has no field `group`, so the ids decide
        pob_engine::lua::lua_sort(&mut out.mods, &mut |a, b| a[0].id.as_bytes() < b[0].id.as_bytes());
        for (i, elem) in out.mods.iter().enumerate() {
            out.groups.push(ModGroup {
                label: elem[0].label.clone(),
                m: elem[0].m.clone(),
                mod_list_index: i,
                unique_title: elem[0].unique_title.clone(),
                vestigial: true,
                default_sort_order: 0,
                sort_value: 0.0,
            });
        }
    }
    // `setDefaultSortOrder()` (:30-42)
    for (i, g) in out.groups.iter_mut().enumerate() {
        // a vestigial group is its `listMod`: the second loop below overwrites the field with 1
        g.default_sort_order = if g.vestigial { 1 } else { i + 1 };
    }
    for list in &mut out.mods {
        for (i, m) in list.iter_mut().enumerate() {
            m.default_sort_order = i + 1;
        }
    }
    out
}

/// `main.uniqueDB.byTitle`: later items of the list win.
fn unique_by_title() -> HashMap<String, Rc<Item>> {
    super::items::unique_db_items()
        .into_iter()
        .filter_map(|i| Some((i.title.as_deref()?.to_lowercase(), i.clone())))
        .collect()
}

/// `applyCandidateMod(item, listMod)` (:186-211).
fn apply_candidate_mod(item: &mut Item, l: &ListMod) {
    let line_of = |line: &str| {
        let mut ml = ModLine {
            line: line.to_string(),
            mod_tags: l.m.mod_tags.clone(),
            ..ModLine::default()
        };
        ml.set_flag_pub(l.ty);
        ml
    };
    if l.ty == "exarch" || l.ty == "eater" {
        if let Some(index) = item.implicit_mod_lines.iter().position(|ml| ml.flag(l.ty)) {
            for (i, line) in l.m.lines.iter().enumerate() {
                let new = line_of(line);
                let at = index + i;
                if at < item.implicit_mod_lines.len() {
                    item.implicit_mod_lines[at] = new;
                } else {
                    item.implicit_mod_lines.push(new);
                }
            }
            return;
        }
    } else if l.ty == "vestigial" {
        item.implicit_mod_lines.clear();
    }
    for line in &l.m.lines {
        item.implicit_mod_lines.push(line_of(line));
    }
}

fn mod_rows(list: &[ListMod]) -> Vec<DropItem> {
    list.iter()
        .map(|l| {
            let mut it = DropItem::table(&l.label);
            it.data = Some(Rc::new(Row {
                m: l.m.clone(),
                unique_title: l.unique_title.clone(),
                vestigial: l.ty == "vestigial",
            }));
            it
        })
        .collect()
}

fn group_rows(groups: &[ModGroup]) -> Vec<DropItem> {
    groups
        .iter()
        .map(|g| {
            let mut it = DropItem::table(&g.label);
            it.data = Some(Rc::new(Row {
                m: g.m.clone(),
                unique_title: g.unique_title.clone(),
                vestigial: g.vestigial,
            }));
            it
        })
        .collect()
}

fn add_row_lines(tooltip: &mut Tooltip, g: &Gfx, row: &Row) {
    for line in &row.m.lines {
        tooltip.add_line(g, 16.0, Some(&format!("^7{line}")));
    }
    if let Some(title) = &row.unique_title {
        let color = crate::colors::color_code("UNIQUE").expect("UNIQUE");
        tooltip.add_line(g, 16.0, Some(&format!("^7Source: {color}{title}^7")));
    }
}

impl ItemsTab {
    /// `M.AddImplicitToDisplayItem(itemsTab, displayItem)` (:21-412).
    #[allow(clippy::too_many_lines)]
    pub(crate) fn add_implicit_popup(&self, g: &mut Gfx) {
        let Some(item) = self.display_item.borrow().clone() else { return };
        let mut source_list: Vec<(&str, &str)> = Vec::new();
        let title_lower = item.title.as_deref().map(str::to_lowercase);
        let display_db_unique = title_lower.as_ref().and_then(|t| unique_by_title().get(t).cloned());
        if item.rarity == "UNIQUE"
            && let Some(unique) = &display_db_unique
            && item
                .base
                .is_some_and(|b| pob_data::rules::rules().vestigial_unique_base_types.as_array().is_some_and(|l| l.iter().any(|t| t.as_str() == Some(b.ty.as_str()))))
            // the source field means the item comes from a mechanic; this one also drops in legion
            && (unique.source.is_none() || title_lower.as_deref() == Some("stasis prison"))
        {
            source_list.push(("Vestigial", "VESTIGIAL"));
        }
        if item.rarity != "UNIQUE"
            && item.rarity != "RELIC"
            && matches!(item.ty.as_deref(), Some("Helmet" | "Body Armour" | "Gloves" | "Boots"))
        {
            if item.influence("cleansing") {
                source_list.push(("Searing Exarch", "EXARCH"));
            }
            if item.influence("tangle") {
                source_list.push(("Eater of Worlds", "EATER"));
            }
        }
        source_list.push(("Custom", "CUSTOM"));
        let source_list = Rc::new(source_list);
        let first_source = source_list[0].1;
        let lists = Rc::new(RefCell::new(build_mods(first_source, &item)));
        let first_mods = if first_source == "CUSTOM" {
            Vec::new()
        } else {
            let l = lists.borrow();
            mod_rows(&l.mods[l.groups[0].mod_list_index])
        };
        let first_groups = group_rows(&lists.borrow().groups);
        let mod_select_label = LabelControl::new(
            anchor_nil("TOPRIGHT", "TOPLEFT"),
            rect(95.0, 70.0, 0.0, 16.0),
            "^7Modifier:".to_string(),
        );
        let mod_select = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(100.0, 70.0, 600.0, 18.0),
            first_mods,
            None,
            None,
            false,
        );
        let mod_group_select = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(100.0, 45.0, 600.0, 18.0),
            first_groups,
            None,
            None,
            false,
        );
        {
            let (l, ms) = (lists.clone(), mod_select.clone());
            let w = Rc::downgrade(&mod_group_select);
            *mod_group_select.sel_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let _ = &w;
                let at = l.borrow().groups[index - 1].mod_list_index;
                *ms.list.borrow_mut() = mod_rows(&l.borrow().mods[at]);
                ms.set_sel(g, 1, false);
            }));
        }
        let source = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(100.0, 20.0, 150.0, 18.0),
            source_list.iter().map(|(l, _)| DropItem::str(l)).collect(),
            None,
            None,
            false,
        );
        source.base.enabled.set(source_list.len() > 1);
        let sort = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(355.0, 20.0, 240.0, 18.0),
            mod_sort_list(),
            None,
            None,
            false,
        );
        let source_id = {
            let (s, l) = (source.clone(), source_list.clone());
            Rc::new(move || l[s.sel_index.get() - 1].1)
        };
        // `applySort(stat, selectFirst)` (:226-295): both callers ask for the first row
        let apply_sort: Rc<dyn Fn(&mut Gfx, Option<&PowerStat>)> = {
            let (lists, me, mgs, ms) = (lists.clone(), self.me.clone(), mod_group_select.clone(), mod_select.clone());
            Rc::new(move |g: &mut Gfx, stat: Option<&PowerStat>| {
                if !mgs.is_shown(g) {
                    return;
                }
                let t = me.upgrade().expect("itemsTab");
                let display = t.display_item.borrow().clone().expect("displayItem");
                let mut l = lists.borrow_mut();
                if let Some(stat) = stat {
                    let slot = display.get_primary_slot();
                    let key = stat.stat.clone().unwrap_or_default();
                    for list in &mut l.mods {
                        for m in list.iter_mut() {
                            m.sort_value = match m.sort_values.get(&key) {
                                Some(v) => *v,
                                None => {
                                    let mut it = copy_of(&display);
                                    apply_candidate_mod(&mut it, m);
                                    it.build_and_parse_raw();
                                    let v = t.power_of(&slot, it, stat);
                                    m.sort_values.insert(key.clone(), v);
                                    v
                                }
                            };
                        }
                        pob_engine::lua::lua_sort(list, &mut |a, b| {
                            if a.sort_value != b.sort_value {
                                a.sort_value > b.sort_value
                            } else {
                                a.default_sort_order < b.default_sort_order
                            }
                        });
                    }
                    let best: Vec<f64> = l
                        .groups
                        .iter()
                        .map(|gr| {
                            l.mods[gr.mod_list_index]
                                .iter()
                                .map(|m| m.sort_value)
                                .fold(None, |b: Option<f64>, v| Some(b.map_or(v, |b| if v > b { v } else { b })))
                                .unwrap_or(0.0)
                        })
                        .collect();
                    for (gr, v) in l.groups.iter_mut().zip(best) {
                        gr.sort_value = v;
                    }
                    pob_engine::lua::lua_sort(&mut l.groups, &mut |a, b| {
                        if a.sort_value != b.sort_value {
                            a.sort_value > b.sort_value
                        } else {
                            a.default_sort_order < b.default_sort_order
                        }
                    });
                } else {
                    for list in &mut l.mods {
                        pob_engine::lua::lua_sort(list, &mut |a, b| a.default_sort_order < b.default_sort_order);
                    }
                    pob_engine::lua::lua_sort(&mut l.groups, &mut |a, b| a.default_sort_order < b.default_sort_order);
                }
                *mgs.list.borrow_mut() = group_rows(&l.groups);
                mgs.search.update_search();
                mgs.set_sel(g, 1, true);
                let at = l.groups[mgs.sel_index.get() - 1].mod_list_index;
                *ms.list.borrow_mut() = mod_rows(&l.mods[at]);
                ms.search.update_search();
                ms.set_sel(g, 1, true);
            })
        };
        {
            let (a, lists, mgs, ms, msl, sort2, it) = (
                apply_sort.clone(),
                lists.clone(),
                mod_group_select.clone(),
                mod_select.clone(),
                mod_select_label.clone(),
                sort.clone(),
                item.clone(),
            );
            let l = source_list.clone();
            *source.sel_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
                let id = l[index - 1].1;
                if id == "CUSTOM" {
                    msl.base.y.set(45.0);
                    return;
                }
                msl.base.y.set(70.0);
                *lists.borrow_mut() = build_mods(id, &it);
                *mgs.list.borrow_mut() = group_rows(&lists.borrow().groups);
                mgs.set_sel(g, 1, false);
                let at = lists.borrow().groups[0].mod_list_index;
                *ms.list.borrow_mut() = mod_rows(&lists.borrow().mods[at]);
                ms.set_sel(g, 1, false);
                let stat = super::item_popups::sort_stat(&sort2);
                a(g, stat.as_ref());
            }));
        }
        *sort.sel_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, _i: usize, v: &DropItem| {
            apply_sort(g, v.data.as_ref().and_then(|x| x.downcast_ref::<PowerStat>()));
        }));
        let not_custom = {
            let s = source_id.clone();
            move || s() != "CUSTOM"
        };
        let sort_label = LabelControl::new(
            anchor_nil("TOPRIGHT", "TOPLEFT"),
            rect(350.0, 20.0, 0.0, 16.0),
            "^7Sort by:".to_string(),
        );
        let f = not_custom.clone();
        sort_label.base.shown.set_fn(move |_| f());
        let f = not_custom.clone();
        sort.base.shown.set_fn(move |_| f());
        let group_label = LabelControl::new(anchor_nil("TOPRIGHT", "TOPLEFT"), rect(95.0, 45.0, 0.0, 16.0), String::new());
        {
            let ms = mod_select.clone();
            group_label.label.set_fn(move |g| {
                if ms.is_shown(g) {
                    "^7Type:".to_string()
                } else {
                    "^7Modifier:".to_string()
                }
            });
        }
        let f = not_custom;
        let msl = mod_select_label.clone();
        group_label.base.shown.set_fn(move |_| {
            if !f() {
                msl.base.y.set(45.0);
            }
            f()
        });
        let s = source_id.clone();
        mod_group_select.base.shown.set_fn(move |_| s() != "CUSTOM");
        {
            let (w, me) = (Rc::downgrade(&mod_group_select), self.me.clone());
            mod_group_select.tip.set_tooltip_func(move |tooltip, g, (mode, index, _v)| {
                tooltip.clear(false);
                if let (true, Some(row)) = (mode != "OUT", row_of(&w.upgrade().expect("modGroupSelect"), index)) {
                    add_row_lines(tooltip, g, &row);
                    me.upgrade()
                        .expect("itemsTab")
                        .add_mod_comparison(tooltip, g, &row.m, row.vestigial);
                }
            });
        }
        let hidden = {
            let s = source_id.clone();
            move || !matches!(s(), "CUSTOM" | "VESTIGIAL")
        };
        let h = hidden.clone();
        mod_select.base.shown.set_fn(move |_| h());
        let h = hidden.clone();
        mod_select_label.base.shown.set_fn(move |_| h());
        {
            let (w, me) = (Rc::downgrade(&mod_select), self.me.clone());
            mod_select.tip.set_tooltip_func(move |tooltip, g, (mode, index, _v)| {
                tooltip.clear(false);
                if let (true, Some(row)) = (mode != "OUT", row_of(&w.upgrade().expect("modSelect"), index)) {
                    add_row_lines(tooltip, g, &row);
                    me.upgrade().expect("itemsTab").add_mod_comparison(tooltip, g, &row.m, false);
                }
            });
        }
        let custom = EditControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(100.0, 45.0, 440.0, 18.0),
            EditArgs::default(),
        );
        let s = source_id.clone();
        custom.base.shown.set_fn(move |_| s() == "CUSTOM");
        // `addModifier()` (:296-309)
        let add: Rc<dyn Fn() -> Item> = {
            let (me, s, ce, mgs, ms, lists) = (
                self.me.clone(),
                source_id,
                custom.clone(),
                mod_group_select.clone(),
                mod_select.clone(),
                lists,
            );
            Rc::new(move || {
                let t = me.upgrade().expect("itemsTab");
                let display = t.display_item.borrow().clone().expect("displayItem");
                let mut it = copy_of(&display);
                if s() == "CUSTOM" {
                    let buf = ce.buf.borrow().clone();
                    if pob_items::lua::match1(&buf, "%S").is_some() {
                        let mut ml = ModLine {
                            line: buf,
                            ..ModLine::default()
                        };
                        ml.set_flag_pub("custom");
                        it.implicit_mod_lines.push(ml);
                    }
                } else {
                    let l = lists.borrow();
                    let at = l.groups[mgs.sel_index.get() - 1].mod_list_index;
                    apply_candidate_mod(&mut it, &l.mods[at][ms.sel_index.get() - 1]);
                }
                it.build_and_parse_raw();
                it
            })
        };
        let (a1, me) = (add.clone(), self.me.clone());
        let save = ButtonControl::new(
            anchor_nil("BOTTOMRIGHT", "BOTTOM"),
            rect(-4.0, -8.0, 80.0, 20.0),
            "Add".to_string(),
            move |g| {
                let it = a1();
                me.upgrade().expect("itemsTab").set_display_item(g, Some(it));
                main().close_popup();
            },
        );
        let (a1, me) = (add, self.me.clone());
        save.tip.set_tooltip_func(move |tooltip, g, ()| {
            tooltip.clear(false);
            me.upgrade().expect("itemsTab").add_tooltip(tooltip, g, &Rc::new(a1()), None, false);
        });
        let close = ButtonControl::new(
            anchor_nil("BOTTOMLEFT", "BOTTOM"),
            rect(4.0, -8.0, 80.0, 20.0),
            "Cancel".to_string(),
            |_g| {
                main().close_popup();
            },
        );
        let popup_height = if hidden() { 130.0 } else { 110.0 };
        let default_control = (first_source == "CUSTOM").then_some("custom");
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert(
            "sourceLabel".into(),
            LabelControl::new(
                anchor_nil("TOPRIGHT", "TOPLEFT"),
                rect(95.0, 20.0, 0.0, 16.0),
                "^7Source:".to_string(),
            ),
        );
        controls.insert("source".into(), source);
        controls.insert("sortLabel".into(), sort_label);
        controls.insert("sort".into(), sort);
        controls.insert("modGroupSelectLabel".into(), group_label);
        controls.insert("modGroupSelect".into(), mod_group_select);
        controls.insert("modSelectLabel".into(), mod_select_label);
        controls.insert("modSelect".into(), mod_select);
        controls.insert("custom".into(), custom);
        controls.insert("save".into(), save);
        controls.insert("close".into(), close);
        main().open_popup(
            g,
            710.0,
            popup_height,
            "Add Implicit to Item",
            controls,
            Some("save"),
            default_control,
            None,
            None,
        );
    }
}
