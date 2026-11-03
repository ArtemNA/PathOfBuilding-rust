//! The popups of the display item: `EnchantDisplayItem` (ItemsTab.lua:2821-3030), `AnointDisplayItem` (:3138-3175) with `NotableDBControl`, `CorruptDisplayItem` (:3178-3571), `AddCustomModifierToDisplayItem` (:3576-3927).
//! `not_ported` (a note, no crash): the roll ranges of `CorruptDisplayItem` (:3403-3436); `Modules/AddImplicitPopup.lua`, `AddCrucibleModifierToDisplayItem`: `item_editor.rs`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_calc::env::CalcOverride;
use pob_calc::power::{PowerStat, power_stat_list};
use pob_engine::engine::Engine;
use pob_gfx::Gfx;
use pob_items::Item;
use pob_items::craft::AffixMod;
use pob_items::item::ModLine;

use super::item_editor::enchantments_of;
use super::items::{ItemsTab, cc};
use crate::control::{ControlRef, anchor, anchor_nil, rect};
use crate::controls::basic::{ButtonControl, CheckBoxControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::list::{ListControl, ListHooks};
use crate::modules::main::main;
use crate::tooltip::Tooltip;

/// `data.enchantmentSource` (Data.lua:632-644).
const ENCHANT_SOURCES: [(&str, &str); 10] = [
    ("ENKINDLING", "Enkindling Orb"),
    ("INSTILLING", "Instilling Orb"),
    ("RUNESMITH", "Runecraft Bench"),
    ("HEIST", "Heist"),
    ("HARVEST", "Harvest"),
    ("DEDICATION", "Dedication to the Goddess"),
    ("ENDGAME", "Eternal Labyrinth"),
    ("MERCILESS", "Merciless Labyrinth"),
    ("CRUEL", "Cruel Labyrinth"),
    ("NORMAL", "Normal Labyrinth"),
];

/// `buildModSortList()` (:76-86).
fn mod_sort_list() -> Vec<DropItem> {
    let mut out = vec![DropItem::table("Default")];
    for p in power_stat_list() {
        if p.stat.is_some() && !p.ignore_for_nodes {
            let mut it = DropItem::table(&p.label);
            it.data = Some(Rc::new(p));
            out.push(it);
        }
    }
    out
}

fn sort_stat(d: &DropDownControl) -> Option<PowerStat> {
    d.list
        .borrow()
        .get(d.sel_index.get().wrapping_sub(1))
        .and_then(|v| v.data.as_ref().and_then(|x| x.downcast_ref::<PowerStat>()).cloned())
}

/// `new("Item"):Item(self.displayItem:BuildRaw())` with `item.id = self.displayItem.id`.
fn copy_of(it: &Item) -> Item {
    let mut c = Item::parse_with(&it.build_raw(), None, false);
    c.id = it.id;
    c
}

fn crafted_line(line: &str) -> ModLine {
    let mut m = ModLine {
        line: line.to_string(),
        ..ModLine::default()
    };
    m.set_flag_pub("crafted");
    m
}

fn label(y: f64, x: f64, text: &str) -> Rc<LabelControl> {
    LabelControl::new(anchor_nil("TOPRIGHT", "TOPLEFT"), rect(x, y, 0.0, 16.0), text.to_string())
}

impl ItemsTab {
    /// `calcFunc({ repSlotName = slot, repItem = item }, useFullDPS)` read by `GetFromOutput`.
    fn power_of(&self, slot: &str, item: Item, stat: &PowerStat) -> f64 {
        let ov = CalcOverride {
            rep_slot_name: Some(slot.to_string()),
            rep_item: Some(Rc::new(item)),
            ..CalcOverride::default()
        };
        self.read(|e| e.item_override_power(ov, stat)).unwrap_or_else(|er| panic!("{er}"))
    }

    /// `AddModComparisonTooltip(tooltip, mod, replaceImplicits)` (:2560-2582).
    pub(crate) fn add_mod_comparison(&self, tooltip: &mut Tooltip, g: &Gfx, m: &AffixMod, replace_implicits: bool) {
        let Some(item) = self.display_item.borrow().clone() else { return };
        let slot = item.get_primary_slot();
        let mut new_item = Item::parse_with(&item.build_raw(), None, false);
        if replace_implicits {
            new_item.implicit_mod_lines.clear();
        }
        for sub in &m.lines {
            let mut ml = ModLine {
                line: self.check_line_for_allocates(sub),
                mod_tags: m.mod_tags.clone(),
                ..ModLine::default()
            };
            match m.ty.as_deref() {
                Some("Prefix") => ml.set_flag_pub("prefix"),
                Some("Suffix") => ml.set_flag_pub("suffix"),
                _ => {}
            }
            if replace_implicits {
                new_item.implicit_mod_lines.push(ml);
            } else {
                new_item.explicit_mod_lines.push(ml);
            }
        }
        new_item.build_and_parse_raw();
        let a = CalcOverride {
            rep_slot_name: Some(slot.clone()),
            rep_item: Some(item),
            ..CalcOverride::default()
        };
        let b = CalcOverride {
            rep_slot_name: Some(slot),
            rep_item: Some(Rc::new(new_item)),
            ..CalcOverride::default()
        };
        let lines = self
            .read(|e| e.item_compare_two(a, b, "\nAdding this mod will give: "))
            .unwrap_or_else(|er| panic!("{er}"));
        for l in lines {
            tooltip.add_line(g, 14.0, Some(&l));
        }
    }

    /// `checkLineForAllocates(line, nodes)` (:2550-2558).
    fn check_line_for_allocates(&self, line: &str) -> String {
        if line.contains("Allocates")
            && let Some(id) = pob_items::lua::match1(line, "%d+").and_then(|n| n.parse::<u32>().ok())
            && let Some(n) = self.read(|e| e.spec_rc().node(id).map(|x| x.tree_node().name.clone().unwrap_or_default()))
        {
            return format!("Allocates {n}");
        }
        line.to_string()
    }

    /// `ItemsTab:EnchantDisplayItem(enchantSlot)` (:2821-3030).
    #[allow(clippy::too_many_lines)]
    pub(crate) fn enchant_display_item_popup(&self, g: &mut Gfx, enchant_slot: usize) {
        self.enchant_slot.set(enchant_slot);
        let Some(item) = self.display_item.borrow().clone() else { return };
        let Some(enchantments) = enchantments_of(&item) else { return };
        let have_skills = !ENCHANT_SOURCES.iter().any(|(n, _)| enchantments.get(*n).is_some());
        let mut skills_used: HashSet<String> = HashSet::new();
        if have_skills {
            let data = pob_data::skills::skills();
            self.read(|e| {
                for sg in e.socket_groups() {
                    for gem in &sg.gem_list {
                        if let Some(d) = gem.gem_data.as_deref().and_then(super::gem_select::gem_of)
                            && let Some(ge) = data.get(&d.granted_effect_id)
                            && !ge.support
                            && enchantments.get(&ge.name).is_some()
                        {
                            skills_used.insert(ge.name.clone());
                        }
                    }
                }
            });
        }
        let build_skills = move |only_used: bool, used: &HashSet<String>| -> Vec<String> {
            let mut l: Vec<String> = enchantments
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, _)| k.clone())
                .filter(|k| !only_used || used.is_empty() || used.contains(k))
                .collect();
            l.sort();
            l
        };
        let table = move |skill: Option<&str>| -> &'static serde_json::Value {
            match skill {
                Some(s) => enchantments.get(s).unwrap_or(&serde_json::Value::Null),
                None => enchantments,
            }
        };
        let sources_of = move |skill: Option<&str>| -> Vec<(&'static str, &'static str)> {
            ENCHANT_SOURCES
                .iter()
                .copied()
                .filter(|(n, _)| table(skill).get(*n).is_some())
                .collect()
        };
        let lines_of = move |skill: Option<&str>, src: &str| -> Vec<String> {
            table(skill)
                .get(src)
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        };
        let skill_list = if have_skills {
            build_skills(true, &skills_used)
        } else {
            Vec::new()
        };
        let first_skill = skill_list.first().cloned();
        let srcs = sources_of(first_skill.as_deref());
        let enchant_items = |lines: Vec<String>| -> Vec<DropItem> { lines.iter().map(|l| DropItem::table(l)).collect() };
        let enchantment = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(100.0, 70.0, 495.0, 18.0),
            enchant_items(lines_of(first_skill.as_deref(), srcs.first().map_or("", |s| s.0))),
            None,
            None,
            false,
        );
        let source_drop_items = |s: &[(&str, &str)]| -> Vec<DropItem> {
            s.iter()
                .map(|(n, l)| {
                    let mut it = DropItem::table(l);
                    it.data = Some(Rc::new((*n).to_string()));
                    it
                })
                .collect()
        };
        let cur_skill: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(first_skill.clone()));
        let order: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let sort = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(355.0, 45.0, 240.0, 18.0),
            mod_sort_list(),
            None,
            None,
            false,
        );
        let source = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(100.0, 45.0, 180.0, 18.0),
            source_drop_items(&srcs),
            None,
            None,
            false,
        );
        // `applySort(stat, selectFirst)` (:2940-2974) + `buildEnchantmentList`
        let rebuild: Rc<dyn Fn(&mut Gfx)> = {
            let (en, so, sr, cs, me, slot) = (
                enchantment.clone(),
                sort.clone(),
                source.clone(),
                cur_skill.clone(),
                self.me.clone(),
                enchant_slot,
            );
            Rc::new(move |g: &mut Gfx| {
                let t = me.upgrade().expect("itemsTab");
                let src = sr
                    .list
                    .borrow()
                    .get(sr.sel_index.get().wrapping_sub(1))
                    .and_then(|v| v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned())
                    .unwrap_or_default();
                let mut lines = lines_of(cs.borrow().as_deref(), &src);
                if let Some(stat) = sort_stat(&so) {
                    let display = t.display_item.borrow().clone().expect("displayItem");
                    let slot_name = display.get_primary_slot();
                    let mut keyed: Vec<(f64, usize, String)> = lines
                        .into_iter()
                        .enumerate()
                        .map(|(i, l)| {
                            let it = enchant_with(&display, &l, slot);
                            (t.power_of(&slot_name, it, &stat), i, l)
                        })
                        .collect();
                    keyed.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
                    lines = keyed.into_iter().map(|x| x.2).collect();
                }
                *en.list.borrow_mut() = lines.iter().map(|l| DropItem::table(l)).collect();
                en.set_sel(g, 1, true);
            })
        };
        let r1 = rebuild.clone();
        *source.sel_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, _i: usize, _v: &DropItem| r1(g)));
        let r1 = rebuild.clone();
        *sort.sel_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, _i: usize, _v: &DropItem| r1(g)));
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        if have_skills {
            let (cs, sr, r1) = (cur_skill.clone(), source.clone(), rebuild.clone());
            let skill = DropDownControl::new(
                g,
                anchor_nil("TOPLEFT", "TOPLEFT"),
                rect(100.0, 20.0, 180.0, 18.0),
                skill_list.iter().map(|s| DropItem::str(s)).collect(),
                Some(Rc::new(move |g: &mut Gfx, _i: usize, v: &DropItem| {
                    *cs.borrow_mut() = v.label.clone();
                    let s = sources_of(v.label.as_deref());
                    *sr.list.borrow_mut() = s
                        .iter()
                        .map(|(n, l)| {
                            let mut it = DropItem::table(l);
                            it.data = Some(Rc::new((*n).to_string()));
                            it
                        })
                        .collect();
                    r1(g);
                })),
                None,
                false,
            );
            let (sk, used) = (skill.clone(), skills_used.clone());
            let all = CheckBoxControl::new(
                g,
                anchor_nil("TOPLEFT", "TOPLEFT"),
                [350.0, 20.0, 18.0],
                Some("All skills:"),
                Some(Rc::new(move |g: &mut Gfx, state: bool| {
                    *sk.list.borrow_mut() = build_skills(!state, &used).iter().map(|s| DropItem::str(s)).collect();
                    sk.set_sel(g, 1, false);
                })),
                Some("Show all skills, not just those used by this build."),
                false,
            );
            if skills_used.is_empty() {
                all.state.set(true);
                all.base.enabled.set(false);
            }
            controls.insert("skillLabel".into(), label(20.0, 95.0, "^7Skill:"));
            controls.insert("skill".into(), skill);
            controls.insert("allSkills".into(), all);
        }
        let (en, me) = (Rc::downgrade(&enchantment), self.me.clone());
        enchantment.tip.set_tooltip_func(move |tooltip, g, (_mode, index, _v)| {
            tooltip.clear(false);
            let t = me.upgrade().expect("itemsTab");
            let e = en.upgrade().expect("enchantment");
            let line = e.list.borrow().get(index.wrapping_sub(1)).and_then(|v| v.label.clone());
            let display = t.display_item.borrow().clone();
            if let (Some(l), Some(d)) = (line, display) {
                let it = Rc::new(enchant_with(&d, &l, t.enchant_slot.get()));
                t.add_tooltip(tooltip, g, &it, None, true);
            }
        });
        controls.insert("enchantmentSourceLabel".into(), label(45.0, 95.0, "^7Source:"));
        controls.insert("enchantmentSource".into(), source);
        controls.insert("sortLabel".into(), label(45.0, 350.0, "^7Sort by:"));
        controls.insert("sort".into(), sort);
        controls.insert("enchantmentLabel".into(), label(70.0, 95.0, "^7Enchantment:"));
        controls.insert("enchantment".into(), enchantment.clone());
        let (en, me) = (enchantment, self.me.clone());
        controls.insert(
            "save".into(),
            ButtonControl::new(None, rect(-88.0, 100.0, 80.0, 20.0), "Enchant".to_string(), move |g| {
                let t = me.upgrade().expect("itemsTab");
                let line = en
                    .list
                    .borrow()
                    .get(en.sel_index.get().wrapping_sub(1))
                    .and_then(|v| v.label.clone());
                let d = t.display_item.borrow().clone().expect("displayItem");
                let it = match line {
                    Some(l) => enchant_with(&d, &l, t.enchant_slot.get()),
                    None => copy_of(&d),
                };
                t.set_display_item(g, Some(it));
                main().close_popup();
            }),
        );
        let me = self.me.clone();
        controls.insert(
            "remove".into(),
            ButtonControl::new(None, rect(0.0, 100.0, 80.0, 20.0), "Remove".to_string(), move |g| {
                let t = me.upgrade().expect("itemsTab");
                let d = t.display_item.borrow().clone().expect("displayItem");
                let mut it = copy_of(&d);
                let slot = t.enchant_slot.get();
                if it.enchant_mod_lines.len() >= slot {
                    it.enchant_mod_lines.remove(slot - 1);
                }
                it.build_and_parse_raw();
                t.set_display_item(g, Some(it));
                main().close_popup();
            }),
        );
        controls.insert(
            "close".into(),
            ButtonControl::new(None, rect(88.0, 100.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup()),
        );
        let _ = order;
        main().open_popup(g, 605.0, 130.0, "Enchant Item", controls, None, None, None, None);
    }

    /// `ItemsTab:getAnoint(item)` (:3035-3050).
    fn get_anoint(item: &Item) -> Vec<String> {
        let mut out = Vec::new();
        for list in [
            &item.enchant_mod_lines,
            &item.scourge_mod_lines,
            &item.implicit_mod_lines,
            &item.explicit_mod_lines,
            &item.crucible_mod_lines,
        ] {
            for m in list {
                if let Some((s, _)) = pob_items::lua::find_pattern(&m.line, "Allocates ([a-zA-Z ]+)") {
                    out.push(m.line[s + "Allocates ".len()..].to_string());
                }
            }
        }
        out
    }

    /// `ItemsTab:anointItem(node)` (:3068-3080).
    pub(crate) fn anoint_item(&self, dn: Option<&str>) -> Item {
        let d = self.display_item.borrow().clone().expect("displayItem");
        let mut it = copy_of(&d);
        let slot = self.anoint_enchant_slot.get();
        if it.enchant_mod_lines.len() >= slot {
            it.enchant_mod_lines.remove(slot - 1);
        }
        if let Some(n) = dn {
            it.enchant_mod_lines
                .insert((slot - 1).min(it.enchant_mod_lines.len()), crafted_line(&format!("Allocates {n}")));
        }
        it.build_and_parse_raw();
        it
    }

    /// `ItemsTab:AppendAnointTooltip(tooltip, node, actionText)` (:3085-3123).
    pub(crate) fn append_anoint_tooltip(&self, tooltip: &mut Tooltip, g: &Gfx, node: Option<(u32, String)>) {
        let Some(d) = self.display_item.borrow().clone() else { return };
        let header = if let Some((id, dn)) = &node {
            if self.read(|e| e.spec_rc().alloc_nodes.contains_key(id)) {
                tooltip.add_line(
                    g,
                    14.0,
                    Some(&format!(
                        "^7Anointing {dn} changes nothing because this node is already allocated on the tree."
                    )),
                );
                return;
            }
            if Self::get_anoint(&d).iter().any(|a| a == dn) {
                tooltip.add_line(
                    g,
                    14.0,
                    Some(&format!("^7Anointing {dn} changes nothing because this node is already anointed.")),
                );
                return;
            }
            format!("^7Anointing {dn} will give you: ")
        } else {
            "^7Anointing nothing will give you: ".to_string()
        };
        let slot = d.base.map_or_else(|| "Amulet".to_string(), |b| b.ty.clone());
        let a = CalcOverride {
            rep_slot_name: Some(slot.clone()),
            rep_item: Some(d),
            ..CalcOverride::default()
        };
        let b = CalcOverride {
            rep_slot_name: Some(slot),
            rep_item: Some(Rc::new(self.anoint_item(node.as_ref().map(|n| n.1.as_str())))),
            ..CalcOverride::default()
        };
        let lines = self.read(|e| e.item_compare_two(a, b, &header)).unwrap_or_else(|er| panic!("{er}"));
        if let (Some((_, dn)), true) = (&node, lines.is_empty()) {
            tooltip.add_line(g, 14.0, Some(&format!("^7Anointing {dn} changes nothing.")));
        }
        for l in lines {
            tooltip.add_line(g, 14.0, Some(&l));
        }
    }

    /// `ItemsTab:AnointDisplayItem(enchantSlot)` (:3138-3175).
    pub(crate) fn anoint_display_item_popup(&self, g: &mut Gfx, enchant_slot: usize) {
        self.anoint_enchant_slot.set(enchant_slot);
        let db = NotableDb::new(g, self.me.clone());
        let list = db.list.clone();
        let (l1, me) = (Rc::downgrade(&list), self.me.clone());
        let label_of = Rc::new(move || -> String {
            let l = l1.upgrade().expect("notableDB");
            if let Some(n) = l.sel_value.borrow().as_ref() {
                return format!("Anoint {}", n.1);
            }
            let t = me.upgrade().expect("itemsTab");
            let d = t.display_item.borrow().clone();
            let cur = d.as_ref().map(|i| ItemsTab::get_anoint(i)).unwrap_or_default();
            cur.get(t.anoint_enchant_slot.get() - 1)
                .map_or_else(|| "No Anoint".to_string(), |a| format!("Remove {a}"))
        });
        let (lb, l1, me) = (label_of.clone(), Rc::downgrade(&list), self.me.clone());
        let save = ButtonControl::new(
            anchor_nil("BOTTOMLEFT", "BOTTOM"),
            rect(0.0, -4.0, 0.0, 20.0),
            String::new(),
            move |g| {
                let t = me.upgrade().expect("itemsTab");
                let n = l1.upgrade().expect("notableDB").sel_value.borrow().as_ref().map(|x| x.1.clone());
                let it = t.anoint_item(n.as_deref());
                t.set_display_item(g, Some(it));
                main().close_popup();
            },
        );
        save.label.set_fn({
            let lb = lb.clone();
            move |_| lb()
        });
        let lb2 = lb.clone();
        save.base
            .width
            .set_fn(move |g| crate::sg::draw_string_width(g, 16.0, "VAR", &lb2()) + 10.0);
        save.base
            .x
            .set_fn(move |g| -(crate::sg::draw_string_width(g, 16.0, "VAR", &lb()) + 10.0 + 90.0) / 2.0);
        let (l1, me) = (Rc::downgrade(&list), self.me.clone());
        save.tip.set_tooltip_func(move |tooltip, g, ()| {
            tooltip.clear(false);
            let n = l1
                .upgrade()
                .expect("notableDB")
                .sel_value
                .borrow()
                .as_ref()
                .map(|x| (x.0, x.1.clone()));
            me.upgrade().expect("itemsTab").append_anoint_tooltip(tooltip, g, n);
        });
        let sr: ControlRef = save.clone();
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert("notableDB".into(), list);
        controls.insert("save".into(), save);
        controls.insert(
            "close".into(),
            ButtonControl::new(
                anchor("TOPLEFT", &sr, "TOPRIGHT"),
                rect(10.0, 0.0, 80.0, 20.0),
                "Cancel".to_string(),
                |_g| main().close_popup(),
            ),
        );
        main().open_popup(g, 380.0, 448.0, "Anoint Item", controls, None, None, None, None);
    }

    /// `ItemsTab:CorruptDisplayItem()` (:3178-3571) without the roll ranges.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn corrupt_display_item_popup(&self, g: &mut Gfx) {
        let Some(item) = self.display_item.borrow().clone() else { return };
        let mut max_implicits = item.corrupt_implicit_count.unwrap_or(2.0);
        for ml in &item.implicit_mod_lines {
            for m in ml.mod_list.iter().flatten() {
                if &*m.name == "CorruptImplicitCount" {
                    max_implicits -= 1.0;
                }
            }
        }
        let n_max = max_implicits as usize;
        let is_glimpse = item.is_unique && item.title.as_deref().is_some_and(|t| t.to_lowercase().contains("glimpse of chaos"));
        let mut source_list = vec!["Corrupted".to_string(), "Scourge".to_string()];
        if item.base.is_some_and(|b| b.ty == "Helmet") {
            source_list.insert(if is_glimpse { 0 } else { source_list.len() }, "Glimpse of Chaos".to_string());
        }
        if item.rarity == "UNIQUE" || item.rarity == "RELIC" {
            super::items::note("Classes/ItemsTab.lua:3403-3482 CorruptDisplayItem: the roll ranges of the explicit lines");
        }
        let current: Rc<RefCell<String>> = Rc::new(RefCell::new(source_list[0].clone()));
        let empty: HashSet<String> = HashSet::new();
        let it2 = item.clone();
        let build_implicits = move |mod_type: &str| -> Vec<AffixMod> {
            let mut out: Vec<AffixMod> = if mod_type == "Glimpse of Chaos" {
                pob_items::build_raw::item_exclusive_mods()
                    .iter()
                    .filter(|(k, _)| k.starts_with("UniqueSpecialCorruption"))
                    .map(|(_, m)| m.clone())
                    .collect()
            } else {
                it2.affix_mods()
                    .into_iter()
                    .flatten()
                    .filter(|(_, m)| m.ty.as_deref() == Some(mod_type) && it2.get_mod_spawn_weight(m, &empty, &empty, None) > 0.0)
                    .map(|(_, m)| m.clone())
                    .collect()
            };
            let key = |m: &AffixMod| {
                let l = m.lines.first().map_or(String::new(), |x| x.to_lowercase());
                let l = pob_items::lua::gsub(&l, "%(.-%)", "$", None);
                let l = pob_items::lua::gsub(&l, "[%+%-%%]", "", None);
                pob_items::lua::gsub(&l, "%d+", "$", None)
            };
            pob_engine::lua::lua_sort(&mut out, &mut |a, b| {
                let (x, y) = (key(a), key(b));
                if x != y { x.as_bytes() < y.as_bytes() } else { a.level < b.level }
            });
            out
        };
        let mut drops: Vec<Rc<DropDownControl>> = Vec::new();
        let implicit_y = 35.0;
        let list_of = Rc::new(RefCell::new(build_implicits(&source_list[0])));
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        for i in 1..=5usize {
            let dd = DropDownControl::new(
                g,
                anchor_nil("TOPLEFT", "TOPLEFT"),
                rect(80.0, implicit_y + i as f64 * 20.0, 440.0, 18.0),
                Vec::new(),
                None,
                None,
                false,
            );
            let shown = i <= n_max && !item.implicits_cannot_be_changed;
            dd.base.shown.set(shown);
            let lab = LabelControl::new(
                anchor_nil("TOPRIGHT", "TOPLEFT"),
                rect(75.0, implicit_y + i as f64 * 20.0, 0.0, 16.0),
                format!("^7Implicit #{i}:"),
            );
            lab.base.shown.set(shown);
            let (me, w) = (self.me.clone(), Rc::downgrade(&dd));
            dd.tip.set_tooltip_func(move |tooltip, g, (mode, index, _v)| {
                tooltip.clear(false);
                let d = w.upgrade().expect("implicit");
                let m = d
                    .list
                    .borrow()
                    .get(index.wrapping_sub(1))
                    .and_then(|v| v.data.as_ref().and_then(|x| x.downcast_ref::<AffixMod>()).cloned());
                if let (true, Some(m)) = (mode != "OUT", m) {
                    for l in &m.lines {
                        tooltip.add_line(g, 16.0, Some(&format!("^7{l}")));
                    }
                    me.upgrade().expect("itemsTab").add_mod_comparison(tooltip, g, &m, true);
                }
            });
            controls.insert(format!("implicit{i}Label"), lab);
            controls.insert(format!("implicit{i}"), dd.clone());
            drops.push(dd);
        }
        let drops = Rc::new(drops);
        // `buildCorruptLists(modType)` (:3249-3277)
        let rebuild: Rc<dyn Fn(&mut Gfx)> = {
            let (drops, list_of) = (drops.clone(), list_of.clone());
            Rc::new(move |_g: &mut Gfx| {
                let selected: Vec<Option<AffixMod>> = drops
                    .iter()
                    .map(|d| {
                        d.list
                            .borrow()
                            .get(d.sel_index.get().wrapping_sub(1))
                            .and_then(|v| v.data.as_ref().and_then(|x| x.downcast_ref::<AffixMod>()).cloned())
                    })
                    .collect();
                for (i, d) in drops.iter().enumerate() {
                    let mut l = vec![DropItem::table("None")];
                    for m in list_of.borrow().iter() {
                        let taken = selected
                            .iter()
                            .enumerate()
                            .any(|(k, s)| k != i && s.as_ref().is_some_and(|x| x.group == m.group));
                        if !taken {
                            let mut e = DropItem::table(&m.lines.join("/"));
                            e.data = Some(Rc::new(m.clone()));
                            l.push(e);
                        }
                    }
                    let want = selected[i].as_ref().map(|x| x.id.clone());
                    let at = want.and_then(|w| {
                        l.iter().position(|e| {
                            e.data
                                .as_ref()
                                .and_then(|x| x.downcast_ref::<AffixMod>())
                                .is_some_and(|m| m.id == w)
                        })
                    });
                    *d.list.borrow_mut() = l;
                    d.sel_index.set(at.map_or(1, |x| x + 1));
                }
            })
        };
        for d in drops.iter() {
            let r1 = rebuild.clone();
            *d.sel_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, _i: usize, _v: &DropItem| r1(g)));
        }
        rebuild(g);
        let sort = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(355.0, 20.0, 240.0, 18.0),
            mod_sort_list(),
            None,
            None,
            false,
        );
        {
            // `applySort(stat)` (:3323-3348); `implicitList[modType]` keeps its default order next to the sorted one
            let (l2, r1, me, cur2) = (list_of.clone(), rebuild.clone(), self.me.clone(), current.clone());
            let default_order = Rc::new(RefCell::new(list_of.borrow().clone()));
            *sort.sel_func.borrow_mut() = Some(Rc::new(move |g: &mut Gfx, _i: usize, v: &DropItem| {
                let t = me.upgrade().expect("itemsTab");
                let d = t.display_item.borrow().clone().expect("displayItem");
                let slot = d.get_primary_slot();
                let scourge = cur2.borrow().as_str() == "ScourgeUpside";
                if default_order.borrow().iter().map(|m| &m.id).ne(l2.borrow().iter().map(|m| &m.id)) && !v.data.is_some() {
                    let mut by_default = l2.borrow().clone();
                    let order = default_order.borrow();
                    by_default.sort_by_key(|m| order.iter().position(|x| x.id == m.id).unwrap_or(usize::MAX));
                    *l2.borrow_mut() = by_default;
                }
                if let Some(stat) = v.data.as_ref().and_then(|x| x.downcast_ref::<PowerStat>()) {
                    let mut keyed: Vec<(f64, usize, AffixMod)> = l2
                        .borrow()
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(i, m)| {
                            let mut it = copy_of(&d);
                            it.corrupted = true;
                            let lines: Vec<ModLine> = m
                                .lines
                                .iter()
                                .map(|l| {
                                    let l = if scourge { format!("{{scourge}}{l}") } else { l.clone() };
                                    ModLine {
                                        line: if m.mod_tags.is_empty() {
                                            l
                                        } else {
                                            format!("{{tags:{}}}{l}", m.mod_tags.join(","))
                                        },
                                        ..ModLine::default()
                                    }
                                })
                                .collect();
                            if scourge {
                                it.scourge_mod_lines = lines;
                            } else {
                                it.implicit_mod_lines = lines;
                            }
                            it.build_and_parse_raw();
                            (t.power_of(&slot, it, stat), i, m)
                        })
                        .collect();
                    keyed.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
                    *l2.borrow_mut() = keyed.into_iter().map(|x| x.2).collect();
                }
                r1(g);
            }));
        }
        let (cur, drops2, it3, r1, l2, me) = (
            current.clone(),
            drops.clone(),
            item.clone(),
            rebuild.clone(),
            list_of.clone(),
            self.me.clone(),
        );
        let source = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(100.0, 30.0, 150.0, 18.0),
            source_list.iter().map(|s| DropItem::str(s)).collect(),
            Some(Rc::new(move |g: &mut Gfx, _i: usize, v: &DropItem| {
                for d in drops2.iter() {
                    d.set_sel(g, 1, true);
                }
                let value = v.label.clone().unwrap_or_default();
                let empty: HashSet<String> = HashSet::new();
                let of_type = |ty: &str| -> Vec<AffixMod> {
                    it3.affix_mods()
                        .into_iter()
                        .flatten()
                        .filter(|(_, m)| m.ty.as_deref() == Some(ty) && it3.get_mod_spawn_weight(m, &empty, &empty, None) > 0.0)
                        .map(|(_, m)| m.clone())
                        .collect()
                };
                let _ = me.upgrade();
                if value == "Scourge" {
                    *cur.borrow_mut() = "ScourgeUpside".to_string();
                    *l2.borrow_mut() = of_type("ScourgeUpside");
                } else {
                    *cur.borrow_mut() = value.clone();
                    *l2.borrow_mut() = if value == "Glimpse of Chaos" {
                        pob_items::build_raw::item_exclusive_mods()
                            .iter()
                            .filter(|(k, _)| k.starts_with("UniqueSpecialCorruption"))
                            .map(|(_, m)| m.clone())
                            .collect()
                    } else {
                        of_type(&value)
                    };
                }
                r1(g);
            })),
            None,
            false,
        );
        source.base.enabled.set(source_list.len() > 1);
        let (cur, drops3, me) = (current, drops.clone(), self.me.clone());
        let corrupt: Rc<dyn Fn() -> Item> = Rc::new(move || {
            let t = me.upgrade().expect("itemsTab");
            let d = t.display_item.borrow().clone().expect("displayItem");
            let mut it = copy_of(&d);
            it.corrupted = true;
            let scourge = cur.borrow().as_str() == "ScourgeUpside";
            let special = if scourge {
                None
            } else {
                d.implicit_mod_lines
                    .iter()
                    .rfind(|ml| ml.mod_list.iter().flatten().any(|m| &*m.name == "CorruptImplicitCount"))
                    .cloned()
            };
            let mut new_lines = Vec::new();
            for dd in drops3.iter() {
                if dd.sel_index.get() > 1
                    && let Some(m) = dd.list.borrow()[dd.sel_index.get() - 1]
                        .data
                        .as_ref()
                        .and_then(|x| x.downcast_ref::<AffixMod>())
                {
                    for l in &m.lines {
                        let l = if scourge { format!("{{scourge}}{l}") } else { l.clone() };
                        let line = if m.mod_tags.is_empty() {
                            l
                        } else {
                            format!("{{tags:{}}}{l}", m.mod_tags.join(","))
                        };
                        new_lines.push(ModLine {
                            line,
                            ..ModLine::default()
                        });
                    }
                }
            }
            if !new_lines.is_empty() {
                if scourge {
                    it.scourge_mod_lines = new_lines;
                } else {
                    it.implicit_mod_lines.clear();
                    it.implicit_mod_lines.extend(special);
                    it.implicit_mod_lines.extend(new_lines);
                }
            }
            it.build_and_parse_raw();
            it
        });
        controls.insert(
            "sourceLabel".into(),
            LabelControl::new(
                anchor_nil("TOPRIGHT", "TOPLEFT"),
                rect(95.0, 30.0, 0.0, 16.0),
                "^7Source:".to_string(),
            ),
        );
        controls.insert("source".into(), source);
        controls.insert(
            "sortLabel".into(),
            LabelControl::new(
                anchor_nil("TOPRIGHT", "TOPLEFT"),
                rect(350.0, 20.0, 0.0, 16.0),
                "^7Sort by:".to_string(),
            ),
        );
        controls.insert("sort".into(), sort);
        let cant = LabelControl::new(
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(20.0, implicit_y + 20.0, 0.0, 20.0),
            "^7This Items Implicits Cannot Be Changed".to_string(),
        );
        cant.base.shown.set(item.implicits_cannot_be_changed);
        controls.insert("implicitCannotBeChangedLabel".into(), cant);
        let (c1, me) = (corrupt.clone(), self.me.clone());
        let save = ButtonControl::new(
            anchor_nil("BOTTOM", "BOTTOM"),
            rect(-45.0, -4.0, 80.0, 20.0),
            "Corrupt".to_string(),
            move |g| {
                let it = c1();
                me.upgrade().expect("itemsTab").set_display_item(g, Some(it));
                main().close_popup();
            },
        );
        let (c1, me) = (corrupt, self.me.clone());
        save.tip.set_tooltip_func(move |tooltip, g, ()| {
            tooltip.clear(false);
            me.upgrade().expect("itemsTab").add_tooltip(tooltip, g, &Rc::new(c1()), None, false);
        });
        controls.insert("save".into(), save);
        controls.insert(
            "close".into(),
            ButtonControl::new(
                anchor_nil("BOTTOM", "BOTTOM"),
                rect(45.0, -4.0, 80.0, 20.0),
                "Cancel".to_string(),
                |_g| main().close_popup(),
            ),
        );
        main().open_popup(
            g,
            605.0,
            103.0 + 20.0 * max_implicits,
            "Corrupt Item",
            controls,
            None,
            None,
            None,
            None,
        );
    }

    /// `ItemsTab:AddCustomModifierToDisplayItem()` (:3576-3927).
    #[allow(clippy::too_many_lines)]
    pub(crate) fn add_custom_modifier_popup(&self, g: &mut Gfx) {
        let Some(item) = self.display_item.borrow().clone() else { return };
        let ty = item.ty.clone().unwrap_or_default();
        let mut sources: Vec<(&str, &str)> = Vec::new();
        if ty != "Tincture" && ty != "Graft" {
            if ty != "Jewel" {
                sources.push(("Crafting Bench", "MASTER"));
            }
            if ty != "Jewel" && ty != "Flask" {
                sources.push(("Essence", "ESSENCE"));
                sources.push(("Veiled", "VEILED"));
                sources.push(("Beastcraft", "BEASTCRAFT"));
            }
            if matches!(ty.as_str(), "Helmet" | "Body Armour" | "Gloves" | "Boots") {
                sources.push(("Necropolis", "NECROPOLIS"));
            }
            if item.cluster_jewel.is_none() && ty != "Flask" {
                sources.push(("Fossil", "FOSSIL"));
                sources.push(("Delve Drop-restricted", "DELVE"));
                sources.push(("Incursion Drop-restricted", "INCURSION"));
            }
            if !item.crafted {
                sources.push(("Prefix", "PREFIX"));
                sources.push(("Suffix", "SUFFIX"));
            }
        }
        if let Some(allowed) = item.rare_like_unique.and_then(|r| r.supports_custom_modifiers.as_ref()) {
            sources.retain(|(_, id)| allowed.iter().any(|a| a == id));
        }
        let it2 = item.clone();
        let build = move |id: &str| -> Vec<CustomMod> { build_custom_mods(&it2, id) };
        sources.retain(|(_, id)| !build(id).is_empty());
        sources.push(("Custom", "CUSTOM"));
        let source_items: Vec<DropItem> = sources
            .iter()
            .map(|(l, id)| {
                let mut it = DropItem::table(l);
                it.data = Some(Rc::new((*id).to_string()));
                it
            })
            .collect();
        let first_id = sources[0].1.to_string();
        let mod_items = |mods: Vec<CustomMod>| -> Vec<DropItem> {
            mods.into_iter()
                .map(|m| {
                    let mut it = DropItem::table(&m.label);
                    it.data = Some(Rc::new(m));
                    it
                })
                .collect()
        };
        let mod_select = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(100.0, 45.0, 600.0, 18.0),
            mod_items(build(&first_id)),
            None,
            None,
            false,
        );
        let custom = EditControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(100.0, 45.0, 440.0, 18.0),
            EditArgs::default(),
        );
        let cur: Rc<RefCell<String>> = Rc::new(RefCell::new(first_id));
        let (c1, ms) = (cur.clone(), mod_select.clone());
        let source = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(100.0, 20.0, 150.0, 18.0),
            source_items,
            Some(Rc::new(move |g: &mut Gfx, _i: usize, v: &DropItem| {
                let id = v
                    .data
                    .as_ref()
                    .and_then(|d| d.downcast_ref::<String>())
                    .cloned()
                    .unwrap_or_default();
                *ms.list.borrow_mut() = mod_items(build(&id)).into_iter().collect();
                *c1.borrow_mut() = id;
                ms.set_sel(g, 1, true);
            })),
            None,
            false,
        );
        source.base.enabled.set(sources.len() > 1);
        let c1 = cur.clone();
        mod_select.base.shown.set_fn(move |_| c1.borrow().as_str() != "CUSTOM");
        let c1 = cur.clone();
        custom.base.shown.set_fn(move |_| c1.borrow().as_str() == "CUSTOM");
        let (w, me) = (Rc::downgrade(&mod_select), self.me.clone());
        mod_select.tip.set_tooltip_func(move |tooltip, g, (mode, index, _v)| {
            tooltip.clear(false);
            let d = w.upgrade().expect("modSelect");
            let m = d
                .list
                .borrow()
                .get(index.wrapping_sub(1))
                .and_then(|v| v.data.as_ref().and_then(|x| x.downcast_ref::<CustomMod>()).cloned());
            if let (true, Some(m)) = (mode != "OUT", m) {
                for l in &m.m.lines {
                    tooltip.add_line(g, 16.0, Some(&format!("^7{l}")));
                }
                me.upgrade().expect("itemsTab").add_mod_comparison(tooltip, g, &m.m, false);
            }
        });
        let (c1, ms, ce, me) = (cur.clone(), mod_select.clone(), custom.clone(), self.me.clone());
        let add: Rc<dyn Fn() -> Item> = Rc::new(move || {
            let t = me.upgrade().expect("itemsTab");
            let d = t.display_item.borrow().clone().expect("displayItem");
            let mut it = copy_of(&d);
            if c1.borrow().as_str() == "CUSTOM" {
                let buf = ce.buf.borrow().clone();
                if buf.chars().any(|c| !c.is_whitespace()) {
                    let mut ml = ModLine {
                        line: buf,
                        ..ModLine::default()
                    };
                    ml.set_flag_pub("custom");
                    it.explicit_mod_lines.push(ml);
                }
            } else if let Some(m) = ms
                .list
                .borrow()
                .get(ms.sel_index.get().wrapping_sub(1))
                .and_then(|v| v.data.as_ref().and_then(|x| x.downcast_ref::<CustomMod>()).cloned())
            {
                for l in &m.m.lines {
                    let mut ml = ModLine {
                        line: l.clone(),
                        mod_tags: m.m.mod_tags.clone(),
                        ..ModLine::default()
                    };
                    ml.set_flag_pub(m.flag);
                    it.explicit_mod_lines.push(ml);
                }
            }
            it.build_and_parse_raw();
            it
        });
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        controls.insert("sourceLabel".into(), label(20.0, 95.0, "^7Source:"));
        controls.insert("source".into(), source);
        let c1 = cur.clone();
        let sort_label = label(20.0, 350.0, "^7Sort by:");
        sort_label.base.shown.set_fn(move |_| c1.borrow().as_str() != "CUSTOM");
        controls.insert("sortLabel".into(), sort_label);
        let c1 = cur.clone();
        let (ms2, me2) = (mod_select.clone(), self.me.clone());
        let sort = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(355.0, 20.0, 240.0, 18.0),
            mod_sort_list(),
            Some(Rc::new(move |g: &mut Gfx, _i: usize, v: &DropItem| {
                // `applySort` (:3604-3638)
                let t = me2.upgrade().expect("itemsTab");
                let display = t.display_item.borrow().clone().expect("displayItem");
                let slot = display.get_primary_slot();
                let stat = v.data.as_ref().and_then(|x| x.downcast_ref::<PowerStat>()).cloned();
                let mut rows: Vec<(f64, usize, DropItem)> = ms2
                    .list
                    .borrow()
                    .iter()
                    .cloned()
                    .map(|e| {
                        (
                            e.data.as_ref().and_then(|x| x.downcast_ref::<CustomMod>()).map_or(0, |m| m.order),
                            e,
                        )
                    })
                    .map(|(o, e)| {
                        let p = stat.as_ref().map_or(0.0, |s| {
                            let m = e.data.as_ref().and_then(|x| x.downcast_ref::<CustomMod>()).cloned().expect("mod");
                            let mut it = copy_of(&display);
                            for l in &m.m.lines {
                                let mut ml = ModLine {
                                    line: l.clone(),
                                    mod_tags: m.m.mod_tags.clone(),
                                    ..ModLine::default()
                                };
                                ml.set_flag_pub(m.flag);
                                it.explicit_mod_lines.push(ml);
                            }
                            it.build_and_parse_raw();
                            t.power_of(&slot, it, s)
                        });
                        (p, o, e)
                    })
                    .collect();
                rows.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
                *ms2.list.borrow_mut() = rows.into_iter().map(|x| x.2).collect();
                ms2.set_sel(g, 1, true);
            })),
            None,
            false,
        );
        sort.base.shown.set_fn(move |_| c1.borrow().as_str() != "CUSTOM");
        controls.insert("sort".into(), sort);
        let c1 = cur.clone();
        let ml_label = label(45.0, 95.0, "^7Modifier:");
        ml_label.base.shown.set_fn(move |_| {
            let _ = &c1;
            true
        });
        controls.insert("modSelectLabel".into(), ml_label);
        controls.insert("modSelect".into(), mod_select);
        controls.insert("custom".into(), custom);
        let (a1, me) = (add.clone(), self.me.clone());
        let save = ButtonControl::new(None, rect(-45.0, 75.0, 80.0, 20.0), "Add".to_string(), move |g| {
            let it = a1();
            me.upgrade().expect("itemsTab").set_display_item(g, Some(it));
            main().close_popup();
        });
        let (a1, me) = (add, self.me.clone());
        save.tip.set_tooltip_func(move |tooltip, g, ()| {
            tooltip.clear(false);
            me.upgrade().expect("itemsTab").add_tooltip(tooltip, g, &Rc::new(a1()), None, false);
        });
        controls.insert("save".into(), save);
        controls.insert(
            "close".into(),
            ButtonControl::new(None, rect(45.0, 75.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup()),
        );
        let default = (cur.borrow().as_str() == "CUSTOM").then_some("custom");
        main().open_popup(g, 710.0, 105.0, "Add Modifier to Item", controls, Some("save"), default, None, None);
    }
}

/// `enchantItem` / `getSortValue`'s item (:2889-2913).
fn enchant_with(display: &Item, line: &str, slot: usize) -> Item {
    let mut it = copy_of(display);
    let parts: Vec<&str> = line.split('/').filter(|x| !x.is_empty()).collect();
    if let [a, b] = parts.as_slice() {
        it.enchant_mod_lines = vec![crafted_line(a), crafted_line(b)];
    } else {
        if !it.can_have_two_enchants && it.enchant_mod_lines.len() > 1 {
            it.enchant_mod_lines.truncate(1);
        }
        if it.enchant_mod_lines.len() >= slot {
            it.enchant_mod_lines.remove(slot - 1);
        }
        it.enchant_mod_lines
            .insert((slot - 1).min(it.enchant_mod_lines.len()), crafted_line(line));
    }
    it.build_and_parse_raw();
    it
}

/// A `Data/*.lua` table with no `data/` file, read from the checkout (as `stat_describer.rs`): a table's positional strings are its `lines`.
fn lua_data(rel: &str) -> serde_json::Value {
    let root = crate::launch::launch().cfg.script_path.clone();
    let text = std::fs::read_to_string(format!("{root}{rel}")).unwrap_or_else(|e| panic!("{rel}: {e}"));
    let b = text.as_bytes();
    let mut i = text.find("return").map_or(0, |x| x + 6);
    lua_value(b, &mut i)
}

fn lua_skip(b: &[u8], i: &mut usize) {
    loop {
        while *i < b.len() && (b[*i].is_ascii_whitespace() || b[*i] == b',') {
            *i += 1;
        }
        if b[*i..].starts_with(b"--") {
            while *i < b.len() && b[*i] != b'\n' {
                *i += 1;
            }
        } else {
            return;
        }
    }
}

fn lua_string(b: &[u8], i: &mut usize) -> String {
    let q = b[*i];
    *i += 1;
    let mut out = Vec::new();
    while b[*i] != q {
        if b[*i] == b'\\' {
            *i += 1;
        }
        out.push(b[*i]);
        *i += 1;
    }
    *i += 1;
    String::from_utf8_lossy(&out).into_owned()
}

fn lua_value(b: &[u8], i: &mut usize) -> serde_json::Value {
    use serde_json::{Map, Value, json};
    lua_skip(b, i);
    match b[*i] {
        b'{' => {
            *i += 1;
            let mut map = Map::new();
            let mut lines: Vec<Value> = Vec::new();
            loop {
                lua_skip(b, i);
                if b[*i] == b'}' {
                    *i += 1;
                    break;
                }
                let key = if b[*i] == b'[' {
                    *i += 1;
                    lua_skip(b, i);
                    let k = if b[*i] == b'"' {
                        lua_string(b, i)
                    } else {
                        let s = *i;
                        while b[*i] != b']' {
                            *i += 1;
                        }
                        String::from_utf8_lossy(&b[s..*i]).trim().to_string()
                    };
                    lua_skip(b, i);
                    *i += 1; // ]
                    lua_skip(b, i);
                    *i += 1; // =
                    Some(k)
                } else if b[*i].is_ascii_alphabetic() || b[*i] == b'_' {
                    let s = *i;
                    while b[*i].is_ascii_alphanumeric() || b[*i] == b'_' {
                        *i += 1;
                    }
                    let word = String::from_utf8_lossy(&b[s..*i]).into_owned();
                    lua_skip(b, i);
                    if b[*i] == b'=' {
                        *i += 1;
                        Some(word)
                    } else {
                        lines.push(json!(word == "true"));
                        continue;
                    }
                } else {
                    None
                };
                let v = lua_value(b, i);
                match key {
                    Some(k) => {
                        map.insert(k, v);
                    }
                    None => lines.push(v),
                }
            }
            if !lines.is_empty() {
                // a list of numbers (`statOrder`, `weightVal`) stays a list for `AffixMod::from_json`
                if map.is_empty() && lines.iter().all(Value::is_number) {
                    return Value::Array(lines);
                }
                if map.is_empty() && lines.iter().all(Value::is_string) {
                    // `weightKey = { "a", "b" }` is read as a list by `from_json`
                    return Value::Array(lines);
                }
                map.insert("lines".to_string(), Value::Array(lines));
            }
            Value::Object(map)
        }
        b'"' | b'\'' => json!(lua_string(b, i)),
        b't' => {
            *i += 4;
            json!(true)
        }
        b'f' => {
            *i += 5;
            json!(false)
        }
        _ => {
            let s = *i;
            while b[*i] == b'-' || b[*i] == b'.' || b[*i].is_ascii_digit() {
                *i += 1;
            }
            let n: f64 = String::from_utf8_lossy(&b[s..*i]).parse().unwrap_or(0.0);
            json!(n)
        }
    }
}

/// A row of `modList` of the custom modifier popup.
#[derive(Clone)]
struct CustomMod {
    label: String,
    m: AffixMod,
    /// `[listMod.type] = true`.
    flag: &'static str,
    order: usize,
}

/// `buildMods(sourceId)` (:3672-3809).
fn build_custom_mods(item: &Item, id: &str) -> Vec<CustomMod> {
    let empty: HashSet<String> = HashSet::new();
    let affixes = item.affix_mods();
    let row = |m: &AffixMod, label: String, flag: &'static str| CustomMod {
        label,
        m: m.clone(),
        flag,
        order: 0,
    };
    let joined = |m: &AffixMod| m.lines.join("/");
    let ty = |m: &AffixMod| m.ty.clone().unwrap_or_default();
    let by_affix_type = |rows: &mut Vec<(String, usize, CustomMod)>| {
        rows.sort_by(|a, b| {
            let (x, y) = (a.0 == "Prefix", b.0 == "Prefix");
            y.cmp(&x).then(a.1.cmp(&b.1))
        });
    };
    let mut out: Vec<CustomMod> = Vec::new();
    match id {
        "MASTER" => {
            let mods: Vec<serde_json::Value> = pob_data::read_json("master-mods.json").unwrap_or_else(|e| panic!("{e}"));
            let mut exclude: Vec<String> = Vec::new();
            for a in item.prefixes.iter().chain(&item.suffixes) {
                if a.mod_id != "None"
                    && let Some(g) = affixes.and_then(|t| t.get(&a.mod_id)).and_then(|m| m.group.clone())
                {
                    exclude.push(g);
                }
            }
            let tt = item.ty.as_deref().unwrap_or("");
            let mut rows = Vec::new();
            for (i, v) in mods.iter().enumerate() {
                let m = AffixMod::from_json("", v);
                if v.get("types").and_then(|t| t.get(tt)).is_some() && !m.group.as_ref().is_some_and(|g| exclude.contains(g)) {
                    rows.push((ty(&m), i, row(&m, format!("{} ^8({})", joined(&m), ty(&m)), "crafted")));
                }
            }
            by_affix_type(&mut rows);
            out = rows.into_iter().map(|r| r.2).collect();
        }
        "ESSENCE" => {
            let es: serde_json::Value = pob_data::read_json("essences.json").unwrap_or_else(|e| panic!("{e}"));
            let tt = item.ty.as_deref().unwrap_or("");
            let mut rows = Vec::new();
            for (_, e) in es.as_object().into_iter().flatten() {
                let mid = e.get("mods").and_then(|x| x.get(tt)).and_then(serde_json::Value::as_str);
                if let Some(m) = mid.and_then(|x| affixes.and_then(|t| t.get(x))) {
                    let name = e.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
                    let n = |k: &str| e.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
                    rows.push((
                        n("type"),
                        n("tier"),
                        row(m, format!("{name}   ^8[{}] ({})", joined(m), ty(m)), "custom"),
                    ));
                }
            }
            rows.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.total_cmp(&a.1)));
            out = rows.into_iter().map(|r| r.2).collect();
        }
        "PREFIX" | "SUFFIX" => {
            let want = if id == "PREFIX" { "prefix" } else { "suffix" };
            let mut mods: Vec<&AffixMod> = affixes
                .into_iter()
                .flatten()
                .map(|(_, m)| m)
                .filter(|m| {
                    m.ty.as_deref().map(str::to_lowercase).as_deref() == Some(want)
                        && item.get_mod_spawn_weight(m, &empty, &empty, None) > 0.0
                })
                .collect();
            pob_engine::lua::lua_sort(&mut mods, &mut |a, b| {
                for i in 0..a.lines.len().max(b.lines.len()) {
                    if i >= a.lines.len() {
                        return true;
                    } else if i >= b.lines.len() {
                        return false;
                    } else if a.stat_order[i] != b.stat_order[i] {
                        return a.stat_order[i] < b.stat_order[i];
                    }
                }
                a.level > b.level
            });
            out = mods
                .into_iter()
                .map(|m| row(m, format!("{}   ^8[{}]", m.affix, joined(m)), "custom"))
                .collect();
        }
        "VEILED" | "NECROPOLIS" => {
            let file = if id == "VEILED" {
                "veiled-mods.json"
            } else {
                "necropolis-mods.json"
            };
            let j: pob_data::wire::OrderedMap<serde_json::Value> = pob_data::read_json(file).unwrap_or_else(|e| panic!("{e}"));
            let mut rows = Vec::new();
            for (i, (k, v)) in j.iter().enumerate() {
                let m = AffixMod::from_json(k, v);
                let w = if id == "VEILED" {
                    item.get_mod_spawn_weight(&m, &empty, &empty, None)
                } else {
                    item.get_necropolis_mod_spawn_weight(&m)
                };
                if w > 0.0 {
                    rows.push((ty(&m), i, row(&m, format!("{} ({})", joined(&m), ty(&m)), "custom")));
                }
            }
            by_affix_type(&mut rows);
            out = rows.into_iter().map(|r| r.2).collect();
        }
        "FOSSIL" => {
            let mut rows = Vec::new();
            for (i, (_, m)) in affixes.into_iter().flatten().enumerate() {
                if item.check_if_mod_is_delve(m) && item.get_mod_spawn_weight(m, &empty, &empty, None) > 0.0 {
                    rows.push((ty(m), i, row(m, format!("{} ({})", joined(m), ty(m)), "custom")));
                }
            }
            by_affix_type(&mut rows);
            out = rows.into_iter().map(|r| r.2).collect();
        }
        "BEASTCRAFT" => {
            let mut rows = Vec::new();
            let j = lua_data("Data/BeastCraft.lua");
            for (k, v) in j.as_object().into_iter().flatten() {
                let m = AffixMod::from_json(k, v);
                rows.push((ty(&m), k.clone(), row(&m, format!("{} ({})", joined(&m), ty(&m)), "custom")));
            }
            rows.sort_by(|a, b| {
                let (x, y) = (a.0 == "Prefix", b.0 == "Prefix");
                y.cmp(&x).then(a.1.cmp(&b.1))
            });
            out = rows.into_iter().map(|r| r.2).collect();
        }
        "DELVE" | "INCURSION" => {
            // `buildDropRestricted(baseCategories, modDb)` (:3639-3662)
            let (file, db) = if id == "DELVE" {
                ("Data/DelveDropOnly.lua", "item-mods/Delve.json")
            } else {
                ("Data/IncursionDropOnly.lua", "item-mods/Explicit.json")
            };
            let table: pob_data::wire::OrderedMap<serde_json::Value> = pob_data::read_json(db).unwrap_or_else(|e| panic!("{e}"));
            let base = item.base.expect("base");
            let sub = base.sub_type.as_ref().map(|s| format!("{}: {s}", base.ty));
            let mut rows = Vec::new();
            let j = lua_data(file);
            for (k, entry) in j.as_object().into_iter().flatten() {
                let Some(v) = table.get(k) else { continue };
                let hit = entry["categories"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|c| c.as_str() == Some(base.ty.as_str()) || c.as_str() == sub.as_deref());
                if hit {
                    let m = AffixMod::from_json(k, v);
                    rows.push((ty(&m), k.clone(), row(&m, format!("{} ({})", joined(&m), ty(&m)), "custom")));
                }
            }
            rows.sort_by(|a, b| {
                let (x, y) = (a.0 == "Prefix", b.0 == "Prefix");
                y.cmp(&x).then(a.1.cmp(&b.1))
            });
            out = rows.into_iter().map(|r| r.2).collect();
        }
        _ => {}
    }
    for (i, m) in out.iter_mut().enumerate() {
        m.order = i;
    }
    out
}

// ---------------------------------------------------------------------------------------------- NotableDBControl

/// A row: the node id and `node.dn`.
pub type NotableRow = (u32, String);

pub struct NotableDb {
    pub list: Rc<ListControl<NotableRow>>,
}

struct NotableHooks {
    tab: Weak<ItemsTab>,
    /// `self.sortMaxPower`, `node.measuredPower`, whether the sort is by a stat.
    power: Rc<RefCell<(f64, std::collections::HashMap<u32, f64>, bool)>>,
}

impl ListHooks<NotableRow> for NotableHooks {
    /// NotableDBControl.lua:219-234
    fn get_row_value(&self, _l: &ListControl<NotableRow>, _g: &Gfx, column: usize, _index: usize, v: &NotableRow) -> String {
        if column != 1 {
            return String::new();
        }
        let p = self.power.borrow();
        if p.2 {
            let m = p.1.get(&v.0).copied().unwrap_or(0.0);
            if p.0 > 0.0 && m > 0.0 {
                let red = (m / p.0) * (255.0 - 128.0) + 128.0;
                return format!("^x{:X}8080{}", red as u32, v.1);
            }
            return format!("^x808080{}", v.1);
        }
        format!("{}{}", cc("CRAFTED"), v.1)
    }
    fn has_add_value_tooltip(&self) -> bool {
        true
    }
    /// :239-275
    fn add_value_tooltip(&self, _l: &ListControl<NotableRow>, g: &Gfx, tooltip: &mut Tooltip, _index: usize, v: &NotableRow) {
        let t = self.tab.upgrade().expect("itemsTab");
        let d = t.display_item.borrow().clone();
        let key = (v.0, d.as_ref().map(|x| Rc::as_ptr(x) as usize));
        if tooltip.check_for_update(key) {
            let spec = t.read(Engine::spec_rc);
            if let Some(node) = spec.node(v.0) {
                t.add_notable_tooltip(tooltip, g, node);
            }
            tooltip.add_separator(Some(14.0));
            t.append_anoint_tooltip(tooltip, g, Some(v.clone()));
        }
    }
    fn get_drag_value(&self, _l: &ListControl<NotableRow>, _index: usize, v: &NotableRow) -> (String, NotableRow) {
        ("Node".to_string(), v.clone())
    }
}

impl NotableDb {
    /// `NotableDBClass:NotableDBControl(anchor, rect, itemsTab, db, dbType)`.
    #[allow(clippy::too_many_lines)]
    pub fn new(g: &mut Gfx, tab: Weak<ItemsTab>) -> Rc<NotableDb> {
        let power = Rc::new(RefCell::new((0.0, std::collections::HashMap::new(), false)));
        let list = ListControl::new(
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(10.0, 60.0, 360.0, 360.0),
            16.0,
            Some("VERTICAL"),
            false,
            Vec::new(),
            true,
            Box::new(NotableHooks {
                tab: tab.clone(),
                power: power.clone(),
            }),
        );
        let lr: ControlRef = list.clone();
        let flag = Rc::new(Cell::new(true));
        let f1 = flag.clone();
        let mut sort_items = Vec::new();
        for e in pob_data::rules::rules().power_stat_list.as_array().into_iter().flatten() {
            if !e.get("ignoreForItems").and_then(serde_json::Value::as_bool).unwrap_or(false) {
                let label = e.get("label").and_then(serde_json::Value::as_str).unwrap_or("");
                let mut it = DropItem::table(&format!("Sort by {label}"));
                it.data = e
                    .get("stat")
                    .and_then(serde_json::Value::as_str)
                    .map(|s| Rc::new(s.to_string()) as Rc<dyn std::any::Any>);
                sort_items.push(it);
            }
        }
        let sort = DropDownControl::new(
            g,
            anchor("BOTTOMLEFT", &lr, "TOPLEFT"),
            rect(0.0, -22.0, 360.0, 18.0),
            sort_items,
            Some(Rc::new(move |_g: &mut Gfx, _i: usize, _v: &DropItem| f1.set(true))),
            None,
            false,
        );
        let f1 = flag.clone();
        let search = EditControl::new(
            g,
            anchor("BOTTOMLEFT", &lr, "TOPLEFT"),
            rect(0.0, -2.0, 258.0, 18.0),
            EditArgs {
                init: Some(String::new()),
                prompt: Some("Search".to_string()),
                filter: Some("%c".to_string()),
                limit: Some(100),
                change_func: Some(Rc::new(move |_g: &mut Gfx, _b: &str, _: bool| f1.set(true))),
                clearable: true,
                ..EditArgs::default()
            },
        );
        let sr: ControlRef = search.clone();
        let f1 = flag.clone();
        let mode = DropDownControl::new(
            g,
            anchor("LEFT", &sr, "RIGHT"),
            rect(2.0, 0.0, 100.0, 18.0),
            ["Anywhere", "Names", "Modifiers"].iter().map(|s| DropItem::str(s)).collect(),
            Some(Rc::new(move |_g: &mut Gfx, _i: usize, _v: &DropItem| f1.set(true))),
            None,
            false,
        );
        list.host.add("sort", sort.clone());
        list.host.add("search", search.clone());
        list.host.add("searchMode", mode.clone());
        // `Draw` (:193-213): `ListBuilder` in one frame
        let (w, f1, tw) = (Rc::downgrade(&list), flag, tab);
        let builder: Rc<dyn Fn(&Gfx)> = Rc::new(move |_g: &Gfx| {
            if !f1.replace(false) {
                return;
            }
            let t = tw.upgrade().expect("itemsTab");
            let l = w.upgrade().expect("list");
            let buf = search.buf.borrow().to_ascii_lowercase();
            let mut pattern = String::new();
            for c in buf.chars() {
                if "-.+[]$^%?*".contains(c) {
                    pattern.push('%');
                }
                pattern.push(c);
            }
            let m = mode.sel_index.get();
            let spec = t.read(Engine::spec_rc);
            let has_search = pattern.chars().any(|c| !c.is_whitespace());
            let mut art = None;
            t.with_socket_viewer(|v, e| art = Some(v.art(&crate::launch::launch().cfg.script_path, &e.spec_rc().tree)));
            let art = art.expect("art");
            let mut rows: Vec<NotableRow> = spec
                .tree
                .nodes
                .iter()
                .filter(|n| art.nodes.get(&n.id).is_some_and(|x| !x.recipe.is_empty()))
                .filter(|n| {
                    if !has_search {
                        return true;
                    }
                    let hit = |s: &str| super::minion_list::match_or_pattern(&s.to_ascii_lowercase(), &pattern) == Some(true);
                    ((m == 1 || m == 2) && hit(&n.dn)) || ((m == 1 || m == 3) && n.sd.iter().any(|x| hit(x)))
                })
                .map(|n| (n.id, n.dn.clone()))
                .collect();
            let stat_key = sort
                .list
                .borrow()
                .get(sort.sel_index.get() - 1)
                .and_then(|v| v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned());
            let stat = stat_key.and_then(|k| power_stat_list().into_iter().find(|p| p.stat.as_deref() == Some(k.as_str())));
            let mut p = power.borrow_mut();
            p.1.clear();
            p.0 = 0.0;
            p.2 = stat.is_some();
            if let Some(stat) = stat {
                let slot = t
                    .display_item
                    .borrow()
                    .as_ref()
                    .and_then(|d| d.base)
                    .map_or_else(|| "Amulet".to_string(), |b| b.ty.clone());
                let base = t.power_of(&slot, t.anoint_item(None), &stat);
                let mut infinites = Vec::new();
                for (id, dn) in &rows {
                    let v = t.power_of(&slot, t.anoint_item(Some(dn)), &stat) - base;
                    if v == f64::INFINITY {
                        infinites.push(*id);
                    } else {
                        p.0 = f64::max(p.0, v);
                    }
                    p.1.insert(*id, v);
                }
                if !infinites.is_empty() {
                    p.0 *= 2.0;
                    let mx = p.0;
                    for id in infinites {
                        p.1.insert(id, mx);
                    }
                }
                let pw = p.1.clone();
                pob_engine::lua::lua_sort(&mut rows, &mut |a, b| {
                    let (x, y) = (pw[&a.0], pw[&b.0]);
                    if x != y { y < x } else { a.1.as_bytes() < b.1.as_bytes() }
                });
            } else {
                pob_engine::lua::lua_sort(&mut rows, &mut |a, b| a.1.as_bytes() < b.1.as_bytes());
            }
            *l.list.borrow_mut() = rows;
            *l.default_text.borrow_mut() = Some("^7No notables found that match those filters.".to_string());
        });
        builder(&*g);
        *list.before_draw.borrow_mut() = Some(builder);
        Rc::new(NotableDb { list })
    }
}

/// The node lines of NotableDBControl.lua:246-269 through `PassiveTreeView:AddNodeName`.
impl ItemsTab {
    pub(crate) fn add_notable_tooltip(&self, tooltip: &mut Tooltip, g: &Gfx, node: &pob_tree::SpecNode) {
        self.with_socket_viewer(|v, e| {
            std::mem::swap(&mut v.tooltip, tooltip);
            crate::tree_tooltip::add_node_name_pub(v, g, e, node);
            std::mem::swap(&mut v.tooltip, tooltip);
        });
        let sd = &node.tree_node().sd;
        if !sd.is_empty() {
            tooltip.add_line(g, 16.0, Some(""));
            for (i, line) in sd.iter().enumerate() {
                let bad = line != " "
                    && node
                        .mods()
                        .get(i)
                        .and_then(|m| m.as_ref())
                        .is_none_or(|m| m.extra.is_some() || m.list.is_none());
                if bad {
                    let tail = if crate::common::not_supported_mod_tooltips() {
                        " ^8(Not supported in PoB yet)"
                    } else {
                        ""
                    };
                    tooltip.add_line(g, 16.0, Some(&format!("{}{line}{tail}", cc("UNSUPPORTED"))));
                } else {
                    tooltip.add_line(g, 16.0, Some(&format!("{}{line}", cc("MAGIC"))));
                }
            }
        }
        if let Some(r) = &node.tree_node().reminder_text {
            tooltip.add_separator(Some(14.0));
            for line in r {
                tooltip.add_line(g, 14.0, Some(&format!("^xA0A080{line}")));
            }
        }
    }
}
