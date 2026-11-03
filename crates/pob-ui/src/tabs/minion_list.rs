//! `Classes/MinionListControl.lua`, `Classes/MinionSearchListControl.lua` and `buildMode:OpenSpectreLibrary` (Build.lua:1522-1551).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_gfx::Gfx;
use pob_lua::format::{Arg, format};

use crate::colors::color_code;
use crate::control::{ControlRef, anchor, rect};
use crate::controls::basic::{ButtonControl, LabelControl};
use crate::controls::drop_down::{DropDownControl, DropItem};
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::list::{DragTarget, ListControl, ListHooks, SelfTarget};
use crate::modules::main::main;
use crate::tooltip::Tooltip;

thread_local! {
    static MINIONS: RefCell<Option<Rc<serde_json::Value>>> = const { RefCell::new(None) };
}

fn minions() -> Rc<serde_json::Value> {
    MINIONS.with_borrow_mut(|m| {
        m.get_or_insert_with(|| Rc::new(pob_data::read_json("minions.json").unwrap_or_else(|e| panic!("minions.json: {e}"))))
            .clone()
    })
}

fn minion_name(id: &str) -> String {
    minions()["minions"][id]["name"].as_str().unwrap_or("").to_string()
}

fn fmt1(spec: &str, v: f64) -> String {
    format(spec, &[Arg::Num(v)]).unwrap_or_else(|e| panic!("{e}"))
}

fn skill_names(id: &str) -> Vec<String> {
    let m = minions();
    let skills = pob_data::skills::skills();
    m["minions"][id]["skillList"]
        .as_array()
        .map(|l| {
            l.iter()
                .filter_map(|s| s.as_str().and_then(|s| skills.get(s)).map(|x| x.name.clone()))
                .collect()
        })
        .unwrap_or_default()
}

struct MinionHooks {
    is_dest_side: bool,
}

impl ListHooks<String> for MinionHooks {
    /// MinionListControl.lua:45-50
    fn get_row_value(&self, _l: &ListControl<String>, _g: &Gfx, column: usize, _i: usize, id: &String) -> String {
        if column == 1 { minion_name(id) } else { String::new() }
    }

    fn has_add_value_tooltip(&self) -> bool {
        true
    }

    /// :52-81
    fn add_value_tooltip(&self, _l: &ListControl<String>, g: &Gfx, tooltip: &mut Tooltip, _i: usize, id: &String) {
        if !tooltip.check_for_update(id.clone()) {
            return;
        }
        let all = minions();
        let m = &all["minions"][id.as_str()];
        let num = |k: &str| m[k].as_f64();
        tooltip.add_line(g, 18.0, Some(&format!("^7{}", m["name"].as_str().unwrap_or(""))));
        tooltip.add_line(g, 14.0, Some(&fmt1("^7Life Multiplier: x%.2f", num("life").unwrap_or(0.0))));
        if let Some(v) = num("energyShield") {
            tooltip.add_line(g, 14.0, Some(&fmt1("^7Energy Shield: %d%% of base Life", v * 100.0)));
        }
        if let Some(v) = num("armour") {
            tooltip.add_line(g, 14.0, Some(&fmt1("^7Armour Multiplier: x%.2f", 1.0 + v)));
        }
        if let Some(v) = num("evasion") {
            tooltip.add_line(g, 14.0, Some(&fmt1("^7Evasion Multiplier: x%.2f", 1.0 + v)));
        }
        let c = |k: &str| color_code(k).unwrap_or_default();
        tooltip.add_line(
            g,
            14.0,
            Some(&format!(
                "^7Resistances: {}{}^7/{}{}^7/{}{}^7/{}{}",
                c("FIRE"),
                fmt1("%d", num("fireResist").unwrap_or(0.0)),
                c("COLD"),
                fmt1("%d", num("coldResist").unwrap_or(0.0)),
                c("LIGHTNING"),
                fmt1("%d", num("lightningResist").unwrap_or(0.0)),
                c("CHAOS"),
                fmt1("%d", num("chaosResist").unwrap_or(0.0)),
            )),
        );
        tooltip.add_line(g, 14.0, Some(&fmt1("^7Base Damage: x%.2f", num("damage").unwrap_or(0.0))));
        tooltip.add_line(
            g,
            14.0,
            Some(&fmt1("^7Base Attack Speed: %.2f", 1.0 / num("attackTime").unwrap_or(0.0))),
        );
        for s in skill_names(id) {
            tooltip.add_line(g, 14.0, Some(&format!("^7Skill: {s}")));
        }
    }

    /// :83-85
    fn get_drag_value(&self, _l: &ListControl<String>, _i: usize, v: &String) -> (String, String) {
        ("MinionId".to_string(), v.clone())
    }

    /// :87-89
    fn can_receive_drag(&self, l: &ListControl<String>, drag_type: &str, value: &String) -> bool {
        drag_type == "MinionId" && !l.list.borrow().contains(value)
    }

    /// :91-93
    fn receive_drag(&self, l: &ListControl<String>, _g: &mut Gfx, _t: &str, value: &String) {
        let at = l.sel_drag_index.get().map_or(l.list.borrow().len(), |i| i - 1);
        l.list.borrow_mut().insert(at, value.clone());
    }

    /// :95-99
    fn on_sel_click(&self, _l: &ListControl<String>, _g: &mut Gfx, _i: usize, _v: &String, double_click: bool) -> Option<bool> {
        if double_click && self.is_dest_side {
            SOURCE.with_borrow(|s| s.upgrade()).expect("source").add_sel();
        }
        None
    }

    /// :101-107
    fn on_sel_delete(&self, l: &ListControl<String>, _g: &mut Gfx, index: usize, _v: &String) {
        if !self.is_dest_side {
            l.list.borrow_mut().remove(index - 1);
            l.sel_index.set(None);
            *l.sel_value.borrow_mut() = None;
        }
    }
}

/// `MinionSearchListControl`: the list with `dest`.
pub struct MinionSource {
    pub list: Rc<ListControl<String>>,
    dest: Rc<ListControl<String>>,
    unfiltered: Vec<String>,
    search_text: RefCell<Option<Rc<EditControl>>>,
    search_mode: RefCell<Option<Rc<DropDownControl>>>,
}

thread_local! {
    static SOURCE: RefCell<Weak<MinionSource>> = const { RefCell::new(Weak::new()) };
    /// The popup's controls, kept for the gate.
    static POPUP: RefCell<Option<(Rc<MinionSource>, Rc<ListControl<String>>)>> = const { RefCell::new(None) };
}

impl MinionSource {
    /// MinionListControl.lua:39-43
    fn add_sel(&self) {
        let v = self.list.sel_value.borrow().clone();
        if let Some(v) = v
            && !self.dest.list.borrow().contains(&v)
        {
            self.dest.list.borrow_mut().push(v);
        }
    }

    /// MinionSearchListControl.lua:37-55
    fn does_entry_match(search: &str, id: &str, mode: usize) -> bool {
        if (mode == 1 || mode == 3) && match_or_pattern(&minion_name(id).to_ascii_lowercase(), search) == Some(true) {
            return true;
        }
        if mode == 2 || mode == 3 {
            for s in skill_names(id) {
                if match_or_pattern(&s.to_ascii_lowercase(), search) == Some(true) {
                    return true;
                }
            }
        }
        false
    }

    /// :57-71
    fn list_filter_changed(&self, g: &mut Gfx, buf: &str, mode: usize) {
        let mut search = String::new();
        for c in buf.to_ascii_lowercase().chars() {
            if "-.+[]$^%?*".contains(c) {
                search.push('%');
            }
            search.push(c);
        }
        if search.bytes().any(|c| !c.is_ascii_whitespace()) {
            let filtered: Vec<String> = self
                .unfiltered
                .iter()
                .filter(|id| Self::does_entry_match(&search, id, mode))
                .cloned()
                .collect();
            *self.list.list.borrow_mut() = filtered;
            self.list.select_index(g, Some(1));
        } else {
            *self.list.list.borrow_mut() = self.unfiltered.clone();
        }
    }
}

/// `string:matchOrPattern(pattern)` (Common.lua:1038-1079); `None`: a pattern error (the `PCall`'s `err`).
fn match_or_pattern(s: &str, pattern: &str) -> Option<bool> {
    fn generate(pattern: &str) -> Option<Vec<String>> {
        // the subgroups: `pattern:gmatch("%b()")`
        let bytes = pattern.as_bytes();
        let mut groups: Vec<(usize, usize, Vec<String>)> = Vec::new();
        for c in pob_lua::gmatch(bytes, b"%b()") {
            let c = c.ok()?;
            let sub = c.get(0).and_then(|x| x.bytes(bytes))?;
            let from = groups.last().map_or(0, |g| g.1.saturating_sub(1));
            let at = pattern[from..].find(std::str::from_utf8(sub).ok()?)? + from;
            let inner = std::str::from_utf8(&sub[1..sub.len() - 1]).ok()?;
            groups.push((at, at + sub.len(), generate(inner)?));
        }
        let first_end = groups.first().map_or(pattern.len(), |g| g.0);
        let mut generated = vec![pattern[..first_end].to_string()];
        for (i, (_, close, pats)) in groups.iter().enumerate() {
            let next = groups.get(i + 1).map_or(pattern.len(), |g| g.0);
            let regular = &pattern[*close..next];
            let mut tmp = Vec::new();
            for sp in &generated {
                for p in pats {
                    tmp.push(format!("{sp}{p}{regular}"));
                }
            }
            generated = tmp;
        }
        let mut out = Vec::new();
        for g in generated {
            for p in g.split('|').filter(|x| !x.is_empty()) {
                if !out.iter().any(|x: &String| x == p) {
                    out.push(p.to_string());
                }
            }
        }
        Some(out)
    }
    for p in generate(pattern)? {
        if pob_lua::match_(s.as_bytes(), p.as_bytes(), None).ok()?.is_some() {
            return Some(true);
        }
    }
    Some(false)
}

/// `buildMode:OpenSpectreLibrary()` (Build.lua:1522-1551).
#[allow(clippy::too_many_lines)]
pub fn open_spectre_library(g: &mut Gfx) {
    let b = main().build_mode.clone();
    let dest_list = b.engine.borrow().as_ref().expect("engine").model().spectres.clone();
    let all = minions();
    let mut source_list: Vec<String> = all["spectreIds"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    source_list.sort_by(|a, b| {
        let (na, nb) = (minion_name(a), minion_name(b));
        if na == nb {
            a.as_bytes().cmp(b.as_bytes())
        } else {
            na.as_bytes().cmp(nb.as_bytes())
        }
    });
    let list = ListControl::new(
        None,
        rect(-139.0, 40.0, 265.0, 250.0),
        16.0,
        Some("VERTICAL"),
        true,
        dest_list,
        false,
        Box::new(MinionHooks { is_dest_side: false }),
    );
    list.label.set(Some("^7Spectres in Build:".to_string()));
    let lr: ControlRef = list.clone();
    let l2 = Rc::downgrade(&list);
    let remove = ButtonControl::new(
        anchor("BOTTOMRIGHT", &lr, "TOPRIGHT"),
        rect(0.0, -2.0, 60.0, 18.0),
        "Remove".to_string(),
        move |g| {
            let l = l2.upgrade().expect("list");
            let v = l.sel_value.borrow().clone();
            if let (Some(i), Some(v)) = (l.sel_index.get(), v) {
                l.hooks.on_sel_delete(&l, g, i, &v);
            }
        },
    );
    let l2 = Rc::downgrade(&list);
    remove
        .base
        .enabled
        .set_fn(move |_| l2.upgrade().expect("list").sel_value.borrow().is_some());
    list.host.add("delete", remove);

    let src = ListControl::new(
        None,
        rect(139.0, 60.0, 265.0, 230.0),
        16.0,
        Some("VERTICAL"),
        false,
        source_list.clone(),
        false,
        Box::new(MinionHooks { is_dest_side: true }),
    );
    src.label.set(Some("^7Available Spectres:".to_string()));
    src.label_position_offset.set((0.0, -20.0));
    let target: Rc<dyn DragTarget<String>> = Rc::new(SelfTarget(Rc::downgrade(&list)));
    src.set_drag_target_list(vec![target]);
    let source = Rc::new(MinionSource {
        list: src.clone(),
        dest: list.clone(),
        unfiltered: source_list,
        search_text: RefCell::new(None),
        search_mode: RefCell::new(None),
    });
    SOURCE.with_borrow_mut(|s| *s = Rc::downgrade(&source));
    POPUP.with_borrow_mut(|p| *p = Some((source.clone(), list.clone())));
    let sr: ControlRef = src.clone();
    let s2 = Rc::downgrade(&source);
    let add = ButtonControl::new(
        anchor("BOTTOMRIGHT", &sr, "TOPRIGHT"),
        rect(0.0, -22.0, 60.0, 18.0),
        "Add".to_string(),
        move |_g| {
            s2.upgrade().expect("source").add_sel();
        },
    );
    let s2 = Rc::downgrade(&source);
    add.base.enabled.set_fn(move |_| {
        let s = s2.upgrade().expect("source");
        let v = s.list.sel_value.borrow().clone();
        v.is_some_and(|v| !s.dest.list.borrow().contains(&v))
    });
    src.host.add("add", add);
    let s2 = Rc::downgrade(&source);
    let search_text = EditControl::new(
        g,
        anchor("BOTTOMLEFT", &sr, "TOPLEFT"),
        rect(0.0, -2.0, 203.0, 18.0),
        EditArgs {
            init: Some(String::new()),
            prompt: Some("Search".into()),
            filter: Some("%c".into()),
            limit: Some(100),
            clearable: true,
            change_func: Some(Rc::new(move |g: &mut Gfx, buf: &str, _| {
                let s = s2.upgrade().expect("source");
                let mode = s.search_mode.borrow().as_ref().expect("mode").sel_index.get();
                s.list_filter_changed(g, buf, mode);
            })),
            ..Default::default()
        },
    );
    let tr: ControlRef = search_text.clone();
    let s2 = Rc::downgrade(&source);
    let search_mode = DropDownControl::new(
        g,
        anchor("LEFT", &tr, "RIGHT"),
        rect(2.0, 0.0, 60.0, 18.0),
        vec![DropItem::str("Names"), DropItem::str("Skills"), DropItem::str("Both")],
        Some(Rc::new(move |g: &mut Gfx, index: usize, _v: &DropItem| {
            let s = s2.upgrade().expect("source");
            let buf = s.search_text.borrow().as_ref().expect("text").buf.borrow().clone();
            s.list_filter_changed(g, &buf, index);
        })),
        None,
        false,
    );
    *source.search_text.borrow_mut() = Some(search_text.clone());
    *source.search_mode.borrow_mut() = Some(search_mode.clone());
    src.host.add("searchText", search_text.clone());
    src.host.add("searchModeDropDown", search_mode);

    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert("list".into(), list.clone());
    controls.insert("source".into(), src);
    let l2 = list.clone();
    controls.insert(
        "save".into(),
        ButtonControl::new(None, rect(-45.0, 330.0, 80.0, 20.0), "Save".to_string(), move |_g| {
            let b = main().build_mode.clone();
            b.engine
                .borrow_mut()
                .as_mut()
                .expect("engine")
                .set_spectre_list(l2.list.borrow().clone());
            b.mod_flag.set(true);
            b.build_flag.set(true);
            main().close_popup();
        }),
    );
    controls.insert(
        "cancel".into(),
        ButtonControl::new(None, rect(45.0, 330.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup()),
    );
    controls.insert(
        "noteLine1".into(),
        LabelControl::new(
            anchor("TOPLEFT", &lr, "BOTTOMLEFT"),
            rect(99.0, 2.0, 0.0, 16.0),
            "^7Spectres in your Library must be assigned to an active".to_string(),
        ),
    );
    controls.insert(
        "noteLine2".into(),
        LabelControl::new(
            anchor("TOPLEFT", &lr, "BOTTOMLEFT"),
            rect(95.0, 18.0, 0.0, 16.0),
            "^7Raise Spectre gem for their buffs and curses to activate".to_string(),
        ),
    );
    let popup = main().open_popup(g, 575.0, 360.0, "Spectre Library", controls, None, None, None, None);
    // `spectrePopup:SelectControl(spectrePopup.controls.source.controls.searchText)`
    popup.host.select_control(g, Some(search_text));
}

/// The gate: the lists of the open library.
pub fn debug_library() -> Option<serde_json::Value> {
    POPUP.with_borrow(|p| {
        p.as_ref().map(|(s, l)| {
            serde_json::json!({
                "dest": l.list.borrow().clone(),
                "source": s.list.list.borrow().len(),
                "sourceFirst": s.list.list.borrow().first().cloned(),
                "sel": s.list.sel_index.get(),
                "search": s.search_text.borrow().as_ref().map(|t| t.buf.borrow().clone()),
            })
        })
    })
}

/// The gate: a child of the open library's lists.
pub fn debug_child(parent: &str, child: &str) -> Option<ControlRef> {
    POPUP.with_borrow(|p| {
        p.as_ref().and_then(|(s, l)| {
            let host = if parent == "source" { &s.list.host } else { &l.host };
            host.get(child)
        })
    })
}
