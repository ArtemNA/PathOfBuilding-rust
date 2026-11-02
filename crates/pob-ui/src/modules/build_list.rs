//! `Modules/BuildList.lua`: the `LIST` mode (`listMode`, a `ControlHost`): toolbar, search, the build list.
//! The public-builds panel (`getPublicBuilds`, `ExtBuildListControl`) is disabled in the original (`if false then`).

use super::build_list_helpers::{self as helpers, BUILD_SORT_DROP_LIST, BuildRef};
use super::main::{BareControl, Mode, ModeArg, copy_file, file_exists, main, os_rename};
use crate::control::{ControlRef, Rect, anchor, rect};
use crate::controls::build_list::BuildListControl;
use crate::controls::{ButtonControl, DropDownControl, DropItem, EditArgs, EditControl};
use crate::host::{ControlHost, InputEvent, InputEvents, ipairs};
use crate::sg;
use pob_gfx::{Frame, Gfx};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

pub struct ListMode {
    pub host: ControlHost,
    pub initialised: Cell<bool>,
    pub sub_path: RefCell<String>,
    pub build_index: RefCell<Vec<BuildRef>>,
    pub build_list: RefCell<Option<Rc<BuildListControl>>>,
    pub search_text: RefCell<Option<Rc<EditControl>>>,
    pub sort: RefCell<Option<Rc<DropDownControl>>>,
    pub anchor: RefCell<Option<Rc<BareControl>>>,
    me: Weak<ListMode>,
}

impl ListMode {
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            host: ControlHost::new(),
            initialised: Cell::new(false),
            sub_path: RefCell::new(String::new()),
            build_index: RefCell::new(Vec::new()),
            build_list: RefCell::new(None),
            search_text: RefCell::new(None),
            sort: RefCell::new(None),
            anchor: RefCell::new(None),
            me: me.clone(),
        })
    }

    pub fn bl(&self) -> Rc<BuildListControl> {
        self.build_list.borrow().clone().expect("listMode.controls.buildList")
    }

    /// `self:SelectControl(control)`.
    pub fn select_control(&self, g: &mut Gfx, c: Option<ControlRef>) {
        self.host.select_control(g, c);
    }

    fn sel_full_file_name(&self, sel_build_name: Option<&str>) -> Option<String> {
        sel_build_name.map(|n| format!("{}{}{n}.xml", main().build_path.borrow(), self.sub_path.borrow()))
    }

    /// `listMode:GetDestName(subPath, fileName)` (BuildList.lua:153-170).
    pub fn get_dest_name(&self, sub_path: &str, file_name: &str) -> String {
        let bp = main().build_path.borrow().clone();
        let mut i = 2;
        let mut dest = file_name.to_string();
        while file_exists(&format!("{bp}{sub_path}{dest}")) {
            let base = crate::lua::gsub(file_name, "%.xml$", "");
            dest = format!("{base}[{i}].xml");
            i += 1;
        }
        format!("{bp}{sub_path}{dest}")
    }

    /// `listMode:BuildList()` (BuildList.lua:172-175).
    pub fn build_list(&self, g: &mut Gfx) {
        let bp = main().build_path.borrow().clone();
        let idx = helpers::scan_folder(g, &bp, &self.sub_path.borrow().clone());
        *self.build_index.borrow_mut() = idx;
        self.filter_build_list(g);
    }

    /// `listMode:FilterBuildList()` (BuildList.lua:177-183).
    pub fn filter_build_list(&self, g: &mut Gfx) {
        let filter = main().filter_build_list.borrow().clone();
        let list = helpers::filter_list(&self.build_index.borrow(), &self.sub_path.borrow(), filter.as_deref());
        *self.bl().list.list.borrow_mut() = list;
        self.sort_list(g);
    }

    /// `listMode:SortList()` (BuildList.lua:185-189).
    pub fn sort_list(&self, g: &mut Gfx) {
        let bl = self.bl();
        let old = bl.list.sel_value.borrow().as_ref().map(|v| v.0.full_file_name.clone());
        let mode = main().s.borrow().build_sort_mode.clone();
        helpers::sort_list(&mut bl.list.list.borrow_mut(), &mode);
        bl.sel_by_full_file_name(g, old.as_deref());
    }
}

impl Mode for ListMode {
    /// `listMode:Init(selBuildName, subPath)` (BuildList.lua:15-110).
    fn init(&self, g: &mut Gfx, args: &[ModeArg]) {
        let sel_build_name = args.first().and_then(ModeArg::as_str).map(str::to_string);
        let sub_path = args.get(1).and_then(ModeArg::as_str).map(str::to_string);
        if self.initialised.get() {
            if let Some(s) = sub_path {
                *self.sub_path.borrow_mut() = s;
            }
            let bl = self.bl();
            let sp = self.sub_path.borrow().clone();
            bl.path.set_sub_path(g, &sp, false);
            self.build_list(g);
            let f = self.sel_full_file_name(sel_build_name.as_deref());
            bl.sel_by_full_file_name(g, f.as_deref());
            self.select_control(g, Some(bl.list.clone() as ControlRef));
            return;
        }
        let me = self.me.upgrade().expect("listMode");
        let anchor_c = BareControl::new(None, rect(0.0, 4.0, 0.0, 0.0));
        anchor_c.base.x.set_fn(|_| crate::common::screen_size().0 / 2.0);
        *self.anchor.borrow_mut() = Some(anchor_c.clone());
        let anc: ControlRef = anchor_c;
        *self.sub_path.borrow_mut() = sub_path.unwrap_or_default();
        let new = ButtonControl::new(anchor("TOP", &anc, "TOP"), rect(-259.0, 0.0, 60.0, 20.0), "New".to_string(), |_| {
            main().set_mode("BUILD", vec![ModeArg::Bool(false), ModeArg::Str("Unnamed build".into())]);
        });
        let new: ControlRef = new;
        self.host.add("new", new.clone());
        let w = self.me.clone();
        let bl_of = move || w.upgrade().expect("listMode").bl();
        let b = bl_of.clone();
        let new_folder: ControlRef = ButtonControl::new(
            anchor("LEFT", &new, "RIGHT"),
            rect(8.0, 0.0, 90.0, 20.0),
            "New Folder".to_string(),
            move |g| {
                b().new_folder(g);
            },
        );
        self.host.add("newFolder", new_folder.clone());
        let has_sel = {
            let b = bl_of.clone();
            move |_: &Gfx| b().list.sel_value.borrow().is_some()
        };
        let b = bl_of.clone();
        let open = ButtonControl::new(
            anchor("LEFT", &new_folder, "RIGHT"),
            rect(8.0, 0.0, 60.0, 20.0),
            "Open".to_string(),
            move |g| {
                let bl = b();
                let sv = bl.list.sel_value.borrow().clone().expect("selValue");
                bl.load_build(g, &sv);
            },
        );
        open.base.enabled.set_fn(has_sel.clone());
        let open: ControlRef = open;
        self.host.add("open", open.clone());
        let b = bl_of.clone();
        let copy = ButtonControl::new(
            anchor("LEFT", &open, "RIGHT"),
            rect(8.0, 0.0, 60.0, 20.0),
            "Copy".to_string(),
            move |g| {
                let bl = b();
                let sv = bl.list.sel_value.borrow().clone().expect("selValue");
                bl.rename_build(g, &sv, true);
            },
        );
        copy.base.enabled.set_fn(has_sel.clone());
        let copy: ControlRef = copy;
        self.host.add("copy", copy.clone());
        let b = bl_of.clone();
        let rename = ButtonControl::new(
            anchor("LEFT", &copy, "RIGHT"),
            rect(8.0, 0.0, 60.0, 20.0),
            "Rename".to_string(),
            move |g| {
                let bl = b();
                let sv = bl.list.sel_value.borrow().clone().expect("selValue");
                bl.rename_build(g, &sv, false);
            },
        );
        rename.base.enabled.set_fn(has_sel.clone());
        let rename: ControlRef = rename;
        self.host.add("rename", rename.clone());
        let b = bl_of.clone();
        let delete = ButtonControl::new(
            anchor("LEFT", &rename, "RIGHT"),
            rect(8.0, 0.0, 60.0, 20.0),
            "Delete".to_string(),
            move |g| {
                let bl = b();
                let sv = bl.list.sel_value.borrow().clone().expect("selValue");
                bl.delete_build(g, &sv);
            },
        );
        delete.base.enabled.set_fn(has_sel);
        let delete: ControlRef = delete;
        self.host.add("delete", delete.clone());
        let items: Vec<DropItem> = BUILD_SORT_DROP_LIST
            .iter()
            .map(|(label, mode)| {
                let mut it = DropItem::table(label);
                it.data = Some(Rc::new(mode.to_string()) as Rc<dyn Any>);
                it
            })
            .collect();
        let w = self.me.clone();
        let sort = DropDownControl::new(
            g,
            anchor("LEFT", &delete, "RIGHT"),
            rect(8.0, 0.0, 140.0, 20.0),
            items,
            Some(Rc::new(move |g: &mut Gfx, _i, v: &DropItem| {
                let mode = v.data.as_ref().and_then(|d| d.downcast_ref::<String>()).cloned().expect("sortMode");
                main().s.borrow_mut().build_sort_mode = mode;
                w.upgrade().expect("listMode").sort_list(g);
            })),
            None,
            false,
        );
        let cur = main().s.borrow().build_sort_mode.clone();
        super::main::sel_by_value(&sort, &cur);
        *self.sort.borrow_mut() = Some(sort.clone());
        self.host.add("sort", sort);
        let bl = BuildListControl::new(g, anchor("TOP", &anc, "TOP"), rect(0.0, 75.0, 900.0, 0.0), &me);
        bl.list.base.height.set_fn(|_| crate::common::screen_size().1 - 80.0);
        // `if main.showPublicBuilds then` is `if false then` in the original: width 900, offset 0.
        bl.list.base.width.set(900.0);
        bl.list.base.x.set(0.0);
        *self.build_list.borrow_mut() = Some(bl.clone());
        self.host.add("buildList", bl.list.clone());
        let w = self.me.clone();
        let search = EditControl::new(
            g,
            anchor("TOP", &anc, "TOP"),
            rect(0.0, 25.0, 640.0, 20.0),
            EditArgs {
                // `self.filterBuildList` (a listMode field that is never set): the box starts empty.
                init: None,
                prompt: Some("Search".into()),
                filter: Some("%c%(%)".into()),
                limit: Some(100),
                change_func: Some(Rc::new(move |g: &mut Gfx, buf: &str, _| {
                    *main().filter_build_list.borrow_mut() = Some(buf.to_string());
                    w.upgrade().expect("listMode").filter_build_list(g);
                })),
                clearable: true,
                ..Default::default()
            },
        );
        search.set_placeholder(g, "(e.g. class:assassin myfilename)", false);
        search.base.width.set(900.0);
        search.base.x.set(0.0);
        *self.search_text.borrow_mut() = Some(search.clone());
        self.host.add("searchText", search);
        self.build_list(g);
        let f = self.sel_full_file_name(sel_build_name.as_deref());
        bl.sel_by_full_file_name(g, f.as_deref());
        self.select_control(g, Some(bl.list.clone() as ControlRef));
        self.initialised.set(true);
    }

    /// `listMode:GetArgs()` (BuildList.lua:132-134).
    fn get_args(&self) -> Vec<ModeArg> {
        let bl = self.bl();
        let name = bl
            .list
            .sel_value
            .borrow()
            .as_ref()
            .and_then(|v| v.0.build_name.clone())
            .map_or(ModeArg::Bool(false), ModeArg::Str);
        vec![name, ModeArg::Str(self.sub_path.borrow().clone())]
    }

    /// `listMode:OnFrame(inputEvents)` (BuildList.lua:136-151).
    fn on_frame(&self, f: &mut Frame<'_>, input_events: &mut InputEvents, _view_port: &Rect) {
        let events: Vec<InputEvent> = ipairs(input_events).map(|(_, e)| e.clone()).collect();
        for event in events {
            let InputEvent::KeyDown { key, .. } = event else { continue };
            let bl = self.bl();
            if key == "v" && sg::is_key_down(f, "CTRL") {
                let copy = bl.copy_build.borrow().clone();
                let cut = bl.cut_build.borrow().clone();
                let sub = self.sub_path.borrow().clone();
                let m = main();
                let bp = m.build_path.borrow().clone();
                if let Some(build) = copy {
                    if build.0.sub_path != sub {
                        if !helpers::can_move_to_sub_path(&build.0, &sub) {
                            m.open_message_popup(f, "Error", "A folder cannot be copied into itself.");
                        } else if let Some(folder) = &build.0.folder_name {
                            m.copy_folder(f, &build.0.full_file_name, &format!("{bp}{sub}{folder}"));
                        } else {
                            let _ = copy_file(
                                &build.0.full_file_name,
                                &self.get_dest_name(&sub, build.0.file_name.as_deref().unwrap_or("")),
                            );
                        }
                        self.build_list(f);
                    } else {
                        bl.rename_build(f, &build, true);
                    }
                    *bl.copy_build.borrow_mut() = None;
                } else if let Some(build) = cut {
                    if build.0.sub_path != sub {
                        if !helpers::can_move_to_sub_path(&build.0, &sub) {
                            m.open_message_popup(f, "Error", "A folder cannot be moved into itself.");
                        } else if let Some(folder) = &build.0.folder_name {
                            m.move_folder(f, folder, &format!("{bp}{}", build.0.sub_path), &format!("{bp}{sub}"));
                        } else {
                            let _ = os_rename(
                                &build.0.full_file_name,
                                &self.get_dest_name(&sub, build.0.file_name.as_deref().unwrap_or("")),
                            );
                        }
                        self.build_list(f);
                    }
                    *bl.cut_build.borrow_mut() = None;
                }
            } else if key == "n" && sg::is_key_down(f, "CTRL") {
                main().set_mode("BUILD", vec![ModeArg::Bool(false), ModeArg::Str("Unnamed build".into())]);
            } else if key == "MOUSE4" {
                bl.path.undo(f);
            } else if key == "MOUSE5" {
                bl.path.redo(f);
            }
        }
        let vp = main().view_port.get();
        self.host.process_controls_input(f, input_events, &vp);
        main().draw_background(f, &vp);
        self.host.draw_controls(f, &vp, None);
    }
}
