//! `Classes/BuildSetListControl.lua` and `Classes/BuildSetService.lua` (2.69.0): the list of the "Manage Loadouts" popup over
//! `buildMode.loadoutsList`, its New / Rename / Copy / Delete / "New/Copy Custom" buttons and their popups. A row's value is the spec's
//! 1-based index in `specList` at the time of the `SyncLoadouts` that made the list (see `loadouts.rs`).
use super::build::BuildMode;
use super::main::main;
use crate::control::{ControlRef, anchor, rect};
use crate::controls::list::{ListControl, ListHooks};
use crate::controls::{ButtonControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl};
use indexmap::IndexMap;
use pob_engine::tree_tab::SpecSummary;
use pob_gfx::Gfx;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

pub type BuildSetList = ListControl<usize>;

/// The control's own state (`self` of BuildSetListControl.lua).
pub struct BuildSetListState {
    build: Weak<BuildMode>,
    list: RefCell<Weak<BuildSetList>>,
    /// `self.list` is the table `buildMode.loadoutsList` was when the control took it (ctor, `ResetList`).
    table: RefCell<Rc<RefCell<Vec<usize>>>>,
    /// `spec:CountAllocNodes()` and the class names of the rows, by (output revision, spec count, active spec, a counter of the edits).
    summaries: RefCell<Option<((u64, usize, usize, u64), Vec<SpecSummary>)>>,
    edits: Cell<u64>,
}

struct Hooks(Rc<BuildSetListState>);

impl BuildSetListState {
    fn build(&self) -> Rc<BuildMode> {
        self.build.upgrade().expect("buildMode")
    }

    fn summaries(&self) -> Vec<SpecSummary> {
        let b = self.build();
        let eng = b.engine.borrow();
        let e = eng.as_ref().expect("engine");
        let key = (e.output_revision(), e.spec_count(), e.active_spec(), self.edits.get());
        let mut s = self.summaries.borrow_mut();
        if s.as_ref().is_none_or(|(k, _)| *k != key) {
            *s = Some((key, e.spec_summaries().unwrap_or_else(|er| panic!("{er}"))));
        }
        s.as_ref().expect("summaries").1.clone()
    }

    fn sel_value(&self) -> Option<usize> {
        self.list.borrow().upgrade().and_then(|l| *l.sel_value.borrow())
    }

    /// `BuildSetListClass:ResetList()` (:214-216).
    fn reset_list(&self) {
        self.edits.set(self.edits.get() + 1);
        let t = self.build().loadouts_list.borrow().clone();
        if let Some(l) = self.list.borrow().upgrade() {
            *l.list.borrow_mut() = t.borrow().clone();
        }
        *self.table.borrow_mut() = t;
    }
}

impl ListHooks<usize> for Hooks {
    /// BuildSetListControl.lua:172-183
    fn get_row_value(&self, _l: &BuildSetList, _g: &Gfx, column: usize, index: usize, spec: &usize) -> String {
        if column != 1 {
            return String::new();
        }
        let b = self.0.build();
        let label = {
            let eng = b.engine.borrow();
            eng.as_ref().expect("engine").spec_list_labels().get(spec - 1).cloned()
        };
        let label = label.unwrap_or_else(|| panic!("Classes/BuildSetListControl.lua:174: attempt to index local 'spec' (a nil value)"));
        let summaries = self.0.summaries();
        let s = &summaries[spec - 1];
        let class = if s.ascend_class_name != "None" {
            &s.ascend_class_name
        } else {
            &s.class_name
        };
        let current = if b.active_loadout.get() == Some(index as f64) {
            "  ^9(Current)"
        } else {
            ""
        };
        format!("{label} ({class}, {} points){current}", s.used)
    }

    /// :185-189
    fn on_sel_click(&self, _l: &BuildSetList, g: &mut Gfx, index: usize, _spec: &usize, double_click: bool) -> Option<bool> {
        let b = self.0.build();
        if double_click && b.active_loadout.get() != Some(index as f64) {
            b.c().build_loadouts.set_sel(g, (index + 1) as i64, false);
        }
        None
    }

    /// :203-207
    fn on_sel_key_down(&self, _l: &BuildSetList, g: &mut Gfx, _index: usize, spec: &usize, key: &str) {
        if key == "F2" {
            let title = spec_title(&self.0.build(), *spec);
            rename_loadout(g, &self.0, title);
        }
    }

    /// :209-212. The drag moved the row in `self.list` -- `buildMode.loadoutsList` itself while the control has that table. `OnOrderChange`
    /// gets no gfx here: the build runs it at the start of the frame, after the popup's input (Main.lua: `ProcessInput`, then the mode's `OnFrame`).
    fn on_order_change(&self, l: &BuildSetList, old_index: usize, new_index: usize) {
        let b = self.0.build();
        if Rc::ptr_eq(&self.0.table.borrow(), &b.loadouts_list.borrow()) {
            *b.loadouts_list.borrow().borrow_mut() = l.list.borrow().clone();
        }
        let state = Rc::downgrade(&self.0);
        *b.pending_loadout_order.borrow_mut() = Some(Box::new(move |g: &mut Gfx| {
            let Some(state) = state.upgrade() else { return };
            // BuildSetService:ReorderLoadout (BuildSetService.lua:37-39)
            state.build().reorder_loadout(g, old_index, new_index);
            state.reset_list();
        }));
    }
}

/// `spec.title` of `specList[spec]`.
fn spec_title(b: &BuildMode, spec: usize) -> Option<String> {
    let eng = b.engine.borrow();
    eng.as_ref()
        .expect("engine")
        .model()
        .tree
        .specs
        .get(spec - 1)
        .unwrap_or_else(|| panic!("Classes/BuildSetListControl.lua: attempt to index a nil value (spec {spec})"))
        .title
        .clone()
}

/// `BuildSetService:SpecNameLookup()` (BuildSetService.lua:41-47).
fn spec_name_lookup(b: &BuildMode) -> HashSet<String> {
    b.engine.borrow().as_ref().expect("engine").spec_list_labels().into_iter().collect()
}

/// `BuildSetListClass:BasicLoadoutPopup(options)` (:218-242): Save runs `saveCallback`, `ResetList`, `main:ClosePopup()`.
fn basic_loadout_popup(
    g: &mut Gfx,
    state: &Rc<BuildSetListState>,
    title: &str,
    default_name: Option<String>,
    save: Rc<dyn Fn(&mut Gfx, String)>,
) {
    let lookup = spec_name_lookup(&state.build());
    let st = Rc::downgrade(state);
    crate::set_name_popup::open_name_popup(
        g,
        110.0,
        title,
        "^7Enter name for this loadout:",
        default_name.as_deref().unwrap_or("Default"),
        Rc::new(move |buf: &str| !lookup.contains(buf) && crate::set_name_popup::has_non_space(buf)),
        Rc::new(move |g: &mut Gfx, name: String| {
            save(g, name);
            if let Some(s) = st.upgrade() {
                s.reset_list();
            }
        }),
    );
}

/// `BuildSetListClass:RenameLoadout(loadoutName)` (:60-68) + `BuildSetService:RenameLoadout` (BuildSetService.lua:21-24).
fn rename_loadout(g: &mut Gfx, state: &Rc<BuildSetListState>, name: Option<String>) {
    let b = state.build.clone();
    let old = name.clone().unwrap_or_else(|| "Default".to_string());
    basic_loadout_popup(
        g,
        state,
        "Rename Loadout",
        name,
        Rc::new(move |g: &mut Gfx, new_name: String| {
            let b = b.upgrade().expect("buildMode");
            b.rename_loadout(&old, &new_name);
            b.sync_loadouts_pub(g);
        }),
    );
}

/// `BuildSetListClass:CopyLoadout(loadoutName)` (:70-78).
fn copy_loadout(g: &mut Gfx, state: &Rc<BuildSetListState>, name: Option<String>) {
    let b = state.build.clone();
    let from = name.clone().unwrap_or_else(|| "Default".to_string());
    basic_loadout_popup(
        g,
        state,
        "Copy Loadout",
        name,
        Rc::new(move |g: &mut Gfx, new_name: String| {
            b.upgrade().expect("buildMode").copy_loadout(g, &from, &new_name);
        }),
    );
}

/// `BuildSetListClass:NewLoadout()` (:162-170).
fn new_loadout(g: &mut Gfx, state: &Rc<BuildSetListState>) {
    let b = state.build.clone();
    basic_loadout_popup(
        g,
        state,
        "New Loadout",
        Some("New Loadout".to_string()),
        Rc::new(move |g: &mut Gfx, name: String| b.upgrade().expect("buildMode").new_loadout(g, &name)),
    );
}

/// `BuildSetListClass:DeleteLoadout(index, spec)` (:191-201) + `BuildSetService:DeleteLoadout` (BuildSetService.lua:26-31).
fn delete_loadout(g: &mut Gfx, state: &Rc<BuildSetListState>, index: usize, spec: usize) {
    let Some(l) = state.list.borrow().upgrade() else { return };
    if l.list.borrow().len() <= 1 {
        return;
    }
    let b = state.build();
    let title = spec_title(&b, spec).unwrap_or_else(|| "Default".to_string());
    let st = Rc::downgrade(state);
    main().open_confirm_popup(
        g,
        "Delete Loadout",
        &format!("Are you sure you want to delete '{title}'?"),
        "Delete",
        Rc::new(move |g: &mut Gfx| {
            let Some(st) = st.upgrade() else { return };
            let b = st.build();
            let list = st.list.borrow().upgrade().map(|l| l.list.borrow().clone()).unwrap_or_default();
            let active = b.active_loadout.get();
            let next_index = if active == Some(index as f64) {
                if index > 1 { index - 1 } else { index + 1 }
            } else {
                active.map_or(1, |a| a as usize)
            };
            let next = list
                .get(next_index.wrapping_sub(1))
                .copied()
                .unwrap_or_else(|| panic!("Classes/BuildSetService.lua:30: attempt to index local 'nextLoadout' (a nil value)"));
            let next_title = spec_title(&b, next).unwrap_or_else(|| "Default".to_string());
            b.delete_loadout(g, &title, &next_title);
            if let Some(l) = st.list.borrow().upgrade() {
                l.sel_index.set(None);
                *l.sel_value.borrow_mut() = None;
            }
            st.reset_list();
        }),
        None,
    );
}

/// `getList(setList, orderList, activeId)` of `CustomLoadout` (:81-92): "^7New" and the titles; the 1-based row of `activeId` (1: none).
fn set_drop_list(sets: &[(f64, Option<String>)], active: Option<f64>) -> (Vec<String>, usize) {
    let mut list = vec!["^7New".to_string()];
    let mut active_index = 1;
    for (i, (id, title)) in sets.iter().enumerate() {
        list.push(title.clone().unwrap_or_else(|| "Default".to_string()));
        if Some(*id) == active {
            active_index = i + 2;
        }
    }
    (list, active_index)
}

/// `BuildSetListClass:CustomLoadout(build)` (:80-160): `build` is the selected loadout, or all ids 0.
fn custom_loadout(g: &mut Gfx, state: &Rc<BuildSetListState>, build: Option<super::loadouts::Loadout>) {
    let b = state.build();
    let build = build.unwrap_or_else(|| panic!("Classes/BuildSetListControl.lua:95: attempt to index local 'build' (a nil value)"));
    let spec_id = build
        .spec_id
        .unwrap_or_else(|| panic!("Classes/BuildSetListControl.lua:95: attempt to compare nil with number"));
    let lookup = spec_name_lookup(&b);
    let (specs, items, skills, configs) = {
        let eng = b.engine.borrow();
        let e = eng.as_ref().expect("engine");
        let items: Vec<(f64, Option<String>)> = e.item_sets().iter().map(|s| (s.id, s.title.clone())).collect();
        let configs: Vec<(f64, Option<String>)> = e.config_set_order().into_iter().map(|id| (id, e.config_set_title(id))).collect();
        (e.model().tree.specs.clone(), items, e.skill_sets(), configs)
    };
    // :95 `build.specId > 0 and specList[build.specId].title or "New Loadout"` (a nil title is "New Loadout" too)
    let build_name = if spec_id > 0 {
        specs
            .get(spec_id - 1)
            .unwrap_or_else(|| panic!("Classes/BuildSetListControl.lua:95: attempt to index a nil value (field '?')"))
            .title
            .clone()
    } else {
        None
    }
    .unwrap_or_else(|| "New Loadout".to_string());
    let save_cell: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
    let sc = save_cell.clone();
    let edit = EditControl::new(
        g,
        None,
        rect(0.0, 40.0, 350.0, 20.0),
        EditArgs {
            init: Some(build_name),
            limit: Some(100),
            change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _: bool| {
                if let Some(s) = sc.borrow().as_ref() {
                    s.base
                        .enabled
                        .set(!lookup.contains(buf) && crate::set_name_popup::has_non_space(buf));
                }
            })),
            ..EditArgs::default()
        },
    );
    let drop = |g: &mut Gfx, y: f64, list: Vec<String>, sel: usize| {
        let d = DropDownControl::new(
            g,
            None,
            rect(0.0, y, 350.0, 20.0),
            list.iter().map(|s| DropItem::str(s)).collect(),
            Some(Rc::new(|_g: &mut Gfx, _i, _v: &DropItem| {})),
            None,
            false,
        );
        d.set_sel(g, sel as i64, false);
        d
    };
    let label = |d: &Rc<DropDownControl>, text: &str| {
        let r: ControlRef = d.clone();
        LabelControl::new(anchor("BOTTOMLEFT", &r, "TOPLEFT"), rect(4.0, -4.0, 0.0, 16.0), text.to_string())
    };
    let mut tree_list: Vec<String> = b.engine.borrow().as_ref().expect("engine").spec_list_labels();
    tree_list.insert(0, "^7New".to_string());
    let tree_drop = drop(g, 90.0, tree_list, spec_id + 1);
    let (skill_list, skill_sel) = set_drop_list(&skills, build.skill_set_id);
    let skill_drop = drop(g, 140.0, skill_list, skill_sel);
    let (item_list, item_sel) = set_drop_list(&items, build.item_set_id);
    let item_drop = drop(g, 190.0, item_list, item_sel);
    let (config_list, config_sel) = set_drop_list(&configs, build.config_set_id);
    let config_drop = drop(g, 240.0, config_list, config_sel);
    let (st, e2, td, id, sd, cd) = (
        Rc::downgrade(state),
        edit.clone(),
        tree_drop.clone(),
        item_drop.clone(),
        skill_drop.clone(),
        config_drop.clone(),
    );
    let save = ButtonControl::new(None, rect(-45.0, 270.0, 80.0, 20.0), "Save".to_string(), move |g| {
        let Some(st) = st.upgrade() else { return };
        let b = st.build();
        let (tree_index, item_index, skill_index, config_index) =
            (td.sel_index.get(), id.sel_index.get(), sd.sel_index.get(), cd.sel_index.get());
        let (item_order, skill_order, config_order) = {
            let eng = b.engine.borrow();
            let e = eng.as_ref().expect("engine");
            let items: Vec<f64> = e.item_sets().iter().map(|s| s.id).collect();
            let skills: Vec<f64> = e.skill_sets().into_iter().map(|s| s.0).collect();
            (items, skills, e.config_set_order())
        };
        // :143-149
        let new_spec_id = if tree_index == 1 { -1 } else { (tree_index - 1) as i64 };
        let pick = |index: usize, order: &[f64]| {
            if index == 1 {
                -1.0
            } else {
                order.get(index - 2).copied().unwrap_or(0.0)
            }
        };
        let name = e2.buf.borrow().clone();
        // BuildSetService:CustomLoadout (BuildSetService.lua:33-35)
        b.custom_loadout(
            g,
            new_spec_id,
            pick(item_index, &item_order),
            pick(skill_index, &skill_order),
            pick(config_index, &config_order),
            &name,
        );
        st.reset_list();
        main().close_popup();
    });
    save.base.enabled.set(false);
    *save_cell.borrow_mut() = Some(save.clone());
    let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
    controls.insert(
        "label".into(),
        LabelControl::new(None, rect(0.0, 20.0, 0.0, 16.0), "^7Enter name for this loadout:".to_string()),
    );
    controls.insert("edit".into(), edit);
    controls.insert("treeLabel".into(), label(&tree_drop, "^7Copy from Tree:"));
    controls.insert("skillLabel".into(), label(&skill_drop, "^7Copy from Skill Set:"));
    controls.insert("itemLabel".into(), label(&item_drop, "^7Copy from Item Set:"));
    controls.insert("configLabel".into(), label(&config_drop, "^7Copy from Config Set:"));
    controls.insert("save".into(), save);
    controls.insert(
        "cancel".into(),
        ButtonControl::new(None, rect(45.0, 270.0, 80.0, 20.0), "Cancel".to_string(), |_g| main().close_popup()),
    );
    // the drop-downs last: an open one is drawn over the controls below it
    controls.insert("configDropDown".into(), config_drop);
    controls.insert("itemDropDown".into(), item_drop);
    controls.insert("skillDropDown".into(), skill_drop);
    controls.insert("treeDropDown".into(), tree_drop);
    main().open_popup(
        g,
        370.0,
        300.0,
        "Create Custom Loadout",
        controls,
        Some("save"),
        Some("edit"),
        Some("cancel"),
        None,
    );
}

/// `BuildSetListClass:BuildSetListControl(anchor, rect, buildMode)` (:12-58), the list of `OpenBuildSetManagePopup` (Build.lua:2811).
pub fn new_build_set_list(build: Weak<BuildMode>) -> Rc<BuildSetList> {
    let b = build.upgrade().expect("buildMode");
    let table = b.loadouts_list.borrow().clone();
    let state = Rc::new(BuildSetListState {
        build: build.clone(),
        list: RefCell::new(Weak::new()),
        table: RefCell::new(table.clone()),
        summaries: RefCell::new(None),
        edits: Cell::new(0),
    });
    let values = table.borrow().clone();
    let list = ListControl::new(
        None,
        rect(0.0, 50.0, 380.0, 200.0),
        16.0,
        Some("VERTICAL"),
        true,
        values,
        false,
        Box::new(Hooks(state.clone())),
    );
    *state.list.borrow_mut() = Rc::downgrade(&list);
    let lr: ControlRef = list.clone();
    let button = |a, r, label: &str, f: Box<dyn Fn(&mut Gfx, &Rc<BuildSetListState>)>| {
        let st = state.clone();
        ButtonControl::new(a, r, label.to_string(), move |g| f(g, &st))
    };
    let new_btn = button(
        anchor("BOTTOMLEFT", &lr, "TOP"),
        rect(-190.0, -4.0, 60.0, 18.0),
        "New",
        Box::new(new_loadout),
    );
    let new_r: ControlRef = new_btn.clone();
    let rename = button(
        anchor("LEFT", &new_r, "RIGHT"),
        rect(5.0, 0.0, 60.0, 18.0),
        "Rename",
        Box::new(|g, st| {
            let spec = st.sel_value().expect("selValue");
            rename_loadout(g, st, spec_title(&st.build(), spec));
        }),
    );
    let rename_r: ControlRef = rename.clone();
    let copy = button(
        anchor("LEFT", &rename_r, "RIGHT"),
        rect(5.0, 0.0, 60.0, 18.0),
        "Copy",
        Box::new(|g, st| {
            let spec = st.sel_value().expect("selValue");
            copy_loadout(g, st, spec_title(&st.build(), spec));
        }),
    );
    let copy_r: ControlRef = copy.clone();
    let delete = button(
        anchor("LEFT", &copy_r, "RIGHT"),
        rect(5.0, 0.0, 60.0, 18.0),
        "Delete",
        Box::new(|g, st| {
            let l = st.list.borrow().upgrade().expect("list");
            let (index, spec) = (l.sel_index.get(), *l.sel_value.borrow());
            delete_loadout(
                g,
                st,
                index.expect("selIndex"),
                spec.unwrap_or_else(|| panic!("Classes/BuildSetListControl.lua:193: attempt to index local 'spec' (a nil value)")),
            );
        }),
    );
    let delete_r: ControlRef = delete.clone();
    let custom = button(
        anchor("LEFT", &delete_r, "RIGHT"),
        rect(5.0, 0.0, 120.0, 18.0),
        "New/Copy Custom",
        Box::new(|g, st| match st.sel_value() {
            None => custom_loadout(
                g,
                st,
                Some(super::loadouts::Loadout {
                    spec_id: Some(0),
                    item_set_id: Some(0.0),
                    skill_set_id: Some(0.0),
                    config_set_id: Some(0.0),
                }),
            ),
            Some(spec) => {
                let name = spec_title(&st.build(), spec).unwrap_or_else(|| "Default".to_string());
                let l = st.build().get_loadout_by_name(&name);
                custom_loadout(g, st, l);
            }
        }),
    );
    let sel = |st: &Rc<BuildSetListState>| {
        let st = Rc::downgrade(st);
        move |_g: &Gfx| st.upgrade().is_some_and(|s| s.sel_value().is_some())
    };
    rename.base.enabled.set_fn(sel(&state));
    copy.base.enabled.set_fn(sel(&state));
    let st = Rc::downgrade(&state);
    delete.base.enabled.set_fn(move |_g: &Gfx| {
        st.upgrade()
            .is_some_and(|s| s.sel_value().is_some() && s.list.borrow().upgrade().is_some_and(|l| l.list.borrow().len() > 1))
    });
    list.host.add("new", new_btn);
    list.host.add("rename", rename);
    list.host.add("copy", copy);
    list.host.add("delete", delete);
    list.host.add("custom", custom);
    list
}
