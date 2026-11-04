//! `CompareTabClass:OpenImportFolderPopup` (Classes/CompareTab.lua:1534-1645): a `BuildListControl` over a small `listHost` instead of `listMode`, with folders navigable and nothing renamed, deleted or dragged.
//! The Rust list control is bound to `ListMode` (rename, delete and drag move files), so this host has its own `ListHooks`; both read the same row text ([`build_row_value`]).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_gfx::Gfx;

use crate::control::{Control, ControlRef, anchor_nil, anchor_weak, rect};
use crate::controls::build_list::build_row_value;
use crate::controls::list::{Column, DragTarget, ListControl, ListHooks, SelfTarget};
use crate::controls::path::PathControl;
use crate::controls::scroll_bar::ScrollBarControl;
use crate::controls::{ButtonControl, DropDownControl, DropItem, EditArgs, EditControl};
use crate::modules::build_list_helpers::{self as helpers, BUILD_SORT_DROP_LIST, BuildRef, can_move_to_sub_path};
use crate::modules::main::{main, sel_by_value};

use super::CompareTab;

/// `listHost` (:1540-1565) and the popup's locals `searchText`, `sortMode`.
pub struct FolderHost {
    tab: Weak<CompareTab>,
    sub_path: RefCell<String>,
    build_index: RefCell<Vec<BuildRef>>,
    search_text: RefCell<String>,
    sort_mode: RefCell<String>,
    // The controls own the host through their closures: the popup closes, all of it goes.
    list: RefCell<Weak<ListControl<BuildRef>>>,
    path: RefCell<Weak<PathControl>>,
    search: RefCell<Weak<EditControl>>,
    sort: RefCell<Weak<DropDownControl>>,
    open: RefCell<Weak<ButtonControl>>,
}

impl FolderHost {
    fn path(&self) -> Rc<PathControl> {
        self.path.borrow().upgrade().expect("path")
    }

    fn list(&self) -> Rc<ListControl<BuildRef>> {
        self.list.borrow().upgrade().expect("buildList")
    }

    /// `listHost:BuildList()` (:1545-1548).
    fn build_list(&self, g: &mut Gfx) {
        let bp = main().build_path.borrow().clone();
        let sub = self.sub_path.borrow().clone();
        *self.build_index.borrow_mut() = helpers::scan_folder(g, &bp, &sub);
        self.filter_build_list(g);
    }

    /// `listHost:FilterBuildList()` (:1549-1555).
    fn filter_build_list(&self, g: &mut Gfx) {
        let list = helpers::filter_list(
            &self.build_index.borrow(),
            &self.sub_path.borrow(),
            Some(self.search_text.borrow().as_str()),
        );
        *self.list().list.borrow_mut() = list;
        self.sort_list(g);
    }

    /// `listHost:SortList()` (:1556-1562) with `BuildListClass:SelByFullFileName` (BuildListControl.lua:65-76).
    fn sort_list(&self, g: &mut Gfx) {
        let l = self.list();
        let selected = l.sel_value.borrow().as_ref().map(|v| v.0.full_file_name.clone());
        helpers::sort_list(&mut l.list.borrow_mut(), &self.sort_mode.borrow());
        if let Some(name) = selected {
            let idx = l.list.borrow().iter().position(|b| b.0.full_file_name == name);
            if let Some(i) = idx {
                l.select_index(g, Some(i + 1));
                return;
            }
        }
        l.sel_index.set(None);
        *l.sel_value.borrow_mut() = None;
    }

    /// `importBuildEntry` (:1568-1585).
    fn import_entry(&self, g: &mut Gfx, build: &BuildRef) {
        let m = main();
        let Some(text) = helpers::read_text_mode(&build.0.full_file_name, None) else {
            m.open_message_popup(g, "Import Error", &format!("Couldn't open '{}'.", build.0.full_file_name));
            return;
        };
        let Some(xml) = text.filter(|t| !t.is_empty()) else {
            m.open_message_popup(g, "Import Error", "Build file is empty or unreadable.");
            return;
        };
        let tab = self.tab.upgrade().expect("compareTab");
        if tab.import_build(g, &xml, build.0.build_name.as_deref().unwrap_or_default()) {
            m.close_popup();
        } else {
            m.open_message_popup(g, "Import Error", "Failed to import build for comparison.");
        }
    }

    /// `controls.buildList:LoadBuild(build)` (:1607-1613).
    fn load_build(&self, g: &mut Gfx, build: &BuildRef) {
        match &build.0.folder_name {
            Some(folder) => {
                let path = self.path();
                path.set_sub_path(g, &format!("{}{folder}/", build.0.sub_path), false);
            }
            None => self.import_entry(g, build),
        }
    }

    /// The gate's state (the runner's `folderDump`).
    fn debug_dump(&self, g: &Gfx) -> serde_json::Value {
        let l = self.list();
        let rows: Vec<String> = l
            .list
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, v)| {
                format!(
                    "{}|{}",
                    l.hooks.get_row_value(&l, g, 1, i + 1, v),
                    l.hooks.get_row_value(&l, g, 2, i + 1, v)
                )
            })
            .collect();
        let path = self.path();
        let labels: Vec<String> = path.folder_list.borrow().iter().map(|f| f.label.clone()).collect();
        serde_json::json!({
            "rows": rows,
            "sel": l.sel_index.get().unwrap_or(0),
            "subPath": self.sub_path.borrow().clone(),
            "path": labels,
            "search": self.search.borrow().upgrade().expect("search").buf.borrow().clone(),
            "sort": self.sort.borrow().upgrade().expect("sort").sel_index.get(),
            "sortMode": self.sort_mode.borrow().clone(),
            "openEnabled": self.open.borrow().upgrade().expect("open").is_enabled(g),
            "scrollV": l.scroll_bar_v.offset.get(),
        })
    }
}

struct FolderHooks(Rc<FolderHost>);

impl FolderHooks {
    fn host(&self) -> &Rc<FolderHost> {
        &self.0
    }
}

impl ListHooks<BuildRef> for FolderHooks {
    fn get_row_value(&self, _list: &ListControl<BuildRef>, _g: &Gfx, column: usize, _index: usize, build: &BuildRef) -> String {
        // `self.cutBuild` stays nil: `OnSelCut` is empty (:1626)
        build_row_value(build, Some(self.host().sub_path.borrow().as_str()), None, column)
    }

    /// BuildListControl.lua:226-228
    fn get_drag_value(&self, _list: &ListControl<BuildRef>, _index: usize, build: &BuildRef) -> (String, BuildRef) {
        ("Build".into(), build.clone())
    }

    /// :1624
    fn can_receive_drag(&self, _list: &ListControl<BuildRef>, _drag_type: &str, _value: &BuildRef) -> bool {
        false
    }

    fn has_can_drag_to_value(&self) -> bool {
        true
    }

    /// BuildListControl.lua:255-257
    fn can_drag_to_value(&self, list: &ListControl<BuildRef>, _index: usize, build: &BuildRef) -> bool {
        let Some(folder) = &build.0.folder_name else { return false };
        match list.sel_value.borrow().clone() {
            Some(s) => s != *build && can_move_to_sub_path(&s.0, &format!("{}{folder}/", build.0.sub_path)),
            None => false,
        }
    }

    /// BuildListControl.lua:259-265 over the popup's `LoadBuild`.
    fn on_sel_click(&self, list: &ListControl<BuildRef>, g: &mut Gfx, _index: usize, build: &BuildRef, double_click: bool) -> Option<bool> {
        if double_click {
            self.host().load_build(g, build);
            list.sel_dragging.set(false);
            list.sel_drag_active.set(false);
        }
        None
    }

    /// :1614-1618
    fn on_sel_key_down(&self, _list: &ListControl<BuildRef>, g: &mut Gfx, _index: usize, build: &BuildRef, key: &str) {
        if key == "RETURN" {
            self.host().load_build(g, build);
        }
    }

    fn has_on_hover_key_up(&self) -> bool {
        true
    }

    /// :1619-1623
    fn on_hover_key_up(&self, _list: &ListControl<BuildRef>, g: &mut Gfx, key: &str) {
        if ScrollBarControl::is_scroll_down_key(key) || ScrollBarControl::is_scroll_up_key(key) {
            self.host().list().on_key_up(g, key);
        }
    }
}

/// `controls.buildList.controls.path` as a drag target: `CanReceiveDrag` is false (:1628).
struct NoDropPath(Weak<FolderHost>);

impl DragTarget<BuildRef> for NoDropPath {
    fn can_receive_drag(&self, _drag_type: &str, _value: &BuildRef) -> bool {
        false
    }
    fn set_other_drag_source(&self, on: bool) {
        self.0.upgrade().expect("listHost").path().other_drag_source.set(on);
    }
    fn other_drag_source(&self) -> bool {
        self.0.upgrade().expect("listHost").path().other_drag_source.get()
    }
    fn is_mouse_over(&self, g: &Gfx) -> bool {
        self.0.upgrade().expect("listHost").path().mouse_over(g)
    }
    fn receive_drag(&self, _g: &mut Gfx, _drag_type: &str, _value: &BuildRef) {}
}

impl CompareTab {
    /// `OpenImportFolderPopup` (:1534-1645).
    pub fn open_import_folder_popup(&self, g: &mut Gfx) {
        let host = Rc::new(FolderHost {
            tab: self.me.clone(),
            sub_path: RefCell::new(String::new()),
            build_index: RefCell::new(Vec::new()),
            search_text: RefCell::new(String::new()),
            sort_mode: RefCell::new(main().s.borrow().build_sort_mode.clone()),
            list: RefCell::new(Weak::new()),
            path: RefCell::new(Weak::new()),
            search: RefCell::new(Weak::new()),
            sort: RefCell::new(Weak::new()),
            open: RefCell::new(Weak::new()),
        });
        *self.folder_host.borrow_mut() = Rc::downgrade(&host);
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        // :1588-1592
        let h = host.clone();
        let search = EditControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(15.0, 25.0, 450.0, 20.0),
            EditArgs {
                init: Some(String::new()),
                prompt: Some("Search".into()),
                filter: Some("%c%(%)".into()),
                limit: Some(100),
                change_func: Some(Rc::new(move |g: &mut Gfx, buf: &str, _| {
                    *h.search_text.borrow_mut() = buf.to_string();
                    h.filter_build_list(g);
                })),
                clearable: true,
                ..EditArgs::default()
            },
        );
        search.set_placeholder(g, "(e.g. class:assassin myfilename)", false);
        *host.search.borrow_mut() = Rc::downgrade(&search);
        // :1593-1598
        let items: Vec<DropItem> = BUILD_SORT_DROP_LIST
            .iter()
            .map(|(label, mode)| {
                let mut it = DropItem::table(label);
                it.data = Some(Rc::new(mode.to_string()) as Rc<dyn std::any::Any>);
                it
            })
            .collect();
        let h = host.clone();
        let sort = DropDownControl::new(
            g,
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(475.0, 25.0, 210.0, 20.0),
            items,
            Some(Rc::new(move |g: &mut Gfx, _i, v: &DropItem| {
                let mode = v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned().expect("sortMode");
                main().s.borrow_mut().build_sort_mode = mode.clone();
                *h.sort_mode.borrow_mut() = mode;
                h.sort_list(g);
            })),
            None,
            false,
        );
        let cur = host.sort_mode.borrow().clone();
        sel_by_value(&sort, &cur);
        *host.sort.borrow_mut() = Rc::downgrade(&sort);
        // :1601-1603, BuildListControl.lua:17-32
        let list = ListControl::new(
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(15.0, 75.0, 0.0, 0.0),
            20.0,
            Some("VERTICAL"),
            false,
            Vec::new(),
            false,
            Box::new(FolderHooks(host.clone())),
        );
        list.base.width.set_fn(|_| 670.0);
        list.base.height.set_fn(|_| 355.0);
        list.col_list.replace(vec![
            Column {
                width: Some(crate::control::Prop::func(|_| 670.0 - 172.0)),
                ..Default::default()
            },
            Column::default(),
        ]);
        list.show_row_separators.set(true);
        *host.list.borrow_mut() = Rc::downgrade(&list);
        let list_dyn: crate::control::WeakControl = Rc::downgrade(&list) as crate::control::WeakControl;
        let (h, lw) = (Rc::downgrade(&host), Rc::downgrade(&list));
        let path = PathControl::new(
            g,
            anchor_weak("BOTTOM", list_dyn, "TOP"),
            rect(0.0, -2.0, 0.0, 24.0),
            &main().build_path.borrow(),
            Some(""),
            Some(Rc::new(move |g: &mut Gfx, sub_path: &str| {
                let host = h.upgrade().expect("listHost");
                *host.sub_path.borrow_mut() = sub_path.to_string();
                host.build_list(g);
                let l = lw.upgrade().expect("list");
                l.sel_index.set(None);
                *l.sel_value.borrow_mut() = None;
                l.sel_dragging.set(false);
                l.sel_drag_active.set(false);
                l.other_drag_source.set(false);
            })),
        );
        path.base.width.set_fn(|_| 670.0);
        *host.path.borrow_mut() = Rc::downgrade(&path);
        list.host.add("path", path);
        let targets: Vec<Rc<dyn DragTarget<BuildRef>>> =
            vec![Rc::new(NoDropPath(Rc::downgrade(&host))), Rc::new(SelfTarget(Rc::downgrade(&list)))];
        list.set_drag_target_list(targets);
        // :1631
        host.build_list(g);
        // :1633-1639
        let h = host.clone();
        let open = ButtonControl::new(
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(255.0, 465.0, 80.0, 20.0),
            "Open".to_string(),
            move |g| {
                let sel = h.list().sel_value.borrow().clone();
                if let Some(sel) = sel {
                    h.load_build(g, &sel);
                }
            },
        );
        let h = host.clone();
        open.base.enabled.set_fn(move |_| h.list().sel_value.borrow().is_some());
        *host.open.borrow_mut() = Rc::downgrade(&open);
        // :1640-1642
        let close = ButtonControl::new(
            anchor_nil("TOPLEFT", "TOPLEFT"),
            rect(365.0, 465.0, 80.0, 20.0),
            "Close".to_string(),
            |_| main().close_popup(),
        );
        controls.insert("searchText".into(), search);
        controls.insert("sort".into(), sort);
        controls.insert("buildList".into(), list);
        controls.insert("open".into(), open);
        controls.insert("close".into(), close);
        main().open_popup(
            g,
            700.0,
            500.0,
            "Import from Folder",
            controls,
            Some("open"),
            Some("searchText"),
            Some("close"),
            None,
        );
    }

    /// The centre of a button of the popup's path (1-based), for the gate's cursor.
    pub fn debug_folder_path_button(&self, g: &Gfx, index: usize) -> (f64, f64) {
        let host = self.folder_host.borrow().upgrade().expect("folder popup");
        let path = host.path();
        let button = path.folder_list.borrow()[index - 1].button.clone();
        let (x, y) = button.get_pos(g);
        let (w, h) = button.get_size(g);
        ((x + w / 2.0).floor(), (y + h / 2.0).floor())
    }

    /// The state of the open folder popup for the gate; `None` when it is closed.
    pub fn debug_folder(&self, g: &Gfx) -> Option<serde_json::Value> {
        self.folder_host.borrow().upgrade().map(|h| h.debug_dump(g))
    }
}
