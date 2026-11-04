//! `Classes/BuildListControl.lua` and `Classes/FolderListControl.lua`: `ListControl` subclasses ([`ListHooks`]) of the
//! build list (with a [`PathControl`] above it and drag-and-drop into folders) and of the folder picker.

use crate::control::{Anchor, ControlRef, Prop, RectProps, anchor_weak, rect};
use crate::controls::list::{Column, DragTarget, ListControl, ListHooks, SelfTarget};
use crate::controls::path::PathControl;
use crate::controls::{ButtonControl, ByRef, EditArgs, EditControl, LabelControl};
use crate::lua;
use crate::modules::build_list::ListMode;
use crate::modules::build_list_helpers::{BuildRef, can_move_to_sub_path};
use crate::modules::main::{ModeArg, copy_file, file_exists, main, os_remove, os_rename};
use indexmap::IndexMap;
use pob_gfx::Gfx;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// What the Lua `BuildListControl` instance holds besides the list (`listMode`, `copyBuild`, `cutBuild`,
/// `controls.path`).
pub struct BuildListControl {
    pub list: Rc<ListControl<BuildRef>>,
    pub path: Rc<PathControl>,
    pub list_mode: Weak<ListMode>,
    pub copy_build: RefCell<Option<BuildRef>>,
    pub cut_build: RefCell<Option<BuildRef>>,
}

struct BuildListHooks(Weak<BuildListControl>);

impl BuildListHooks {
    fn ctl(&self) -> Rc<BuildListControl> {
        self.0.upgrade().expect("build list")
    }
}

fn s_eq(a: &Option<String>, b: &Option<String>) -> bool {
    a == b
}

/// `BuildListClass:GetRowValue` (BuildListControl.lua:191-224) over the host's `subPath` and `cutBuild`: shared with the popup of `CompareTab`.
pub(crate) fn build_row_value(build: &BuildRef, base_sub: Option<&str>, cut: Option<&BuildRef>, column: usize) -> String {
    let b = &build.0;
    if column == 1 {
        let mut prefix = String::new();
        if let Some(base_sub) = base_sub
            && b.sub_path != base_sub
        {
            if b.sub_path.as_bytes().starts_with(base_sub.as_bytes()) {
                prefix = b.sub_path[base_sub.len()..].to_string();
            } else {
                prefix = b.sub_path.clone();
            }
        }
        let label = match &b.folder_name {
            Some(f) => format!(">> {prefix}{f}"),
            None => format!("{prefix}{}", b.build_name.as_deref().unwrap_or("?")),
        };
        if let Some(c) = cut
            && s_eq(&c.0.build_name, &b.build_name)
            && s_eq(&c.0.folder_name, &b.folder_name)
            && c.0.sub_path == b.sub_path
        {
            return format!("^xC0B0B0{label}");
        }
        label
    } else if column == 2 {
        if b.build_name.is_some() {
            let color = b
                .class_name
                .as_ref()
                .and_then(|c| crate::colors::color_code(&c.to_ascii_uppercase()))
                .unwrap_or_else(|| "^7".into());
            let name = match &b.ascend_class_name {
                Some(a) if a != "None" => a.clone(),
                _ => b.class_name.clone().unwrap_or_else(|| "?".into()),
            };
            format!("{color}Level {} {name}", b.level.unwrap_or(1.0) as i64)
        } else {
            String::new()
        }
    } else {
        String::new()
    }
}

impl ListHooks<BuildRef> for BuildListHooks {
    /// BuildListControl.lua:223-252
    fn get_row_value(&self, _list: &ListControl<BuildRef>, _g: &Gfx, column: usize, _index: usize, build: &BuildRef) -> String {
        let ctl = self.ctl();
        let base_sub = ctl.list_mode.upgrade().map(|lm| lm.sub_path.borrow().clone());
        let cut = ctl.cut_build.borrow().clone();
        build_row_value(build, base_sub.as_deref(), cut.as_ref(), column)
    }

    /// BuildListControl.lua:254-256
    fn get_drag_value(&self, _list: &ListControl<BuildRef>, _index: usize, build: &BuildRef) -> (String, BuildRef) {
        ("Build".into(), build.clone())
    }

    /// BuildListControl.lua:258-260
    fn can_receive_drag(&self, _list: &ListControl<BuildRef>, drag_type: &str, _value: &BuildRef) -> bool {
        drag_type == "Build"
    }

    /// BuildListControl.lua:262-282
    fn receive_drag(&self, list: &ListControl<BuildRef>, g: &mut Gfx, drag_type: &str, build: &BuildRef) {
        if drag_type != "Build" {
            return;
        }
        let ctl = self.ctl();
        let hover = list.hover_value.borrow().clone();
        if let Some(h) = hover
            && let Some(folder) = &h.0.folder_name
        {
            let target = format!("{}{folder}/", h.0.sub_path);
            ctl.move_into(g, build, &target);
        }
    }

    fn has_can_drag_to_value(&self) -> bool {
        true
    }

    /// BuildListControl.lua:284-286 (`source` is this list: the only drag source of the build list).
    fn can_drag_to_value(&self, list: &ListControl<BuildRef>, _index: usize, build: &BuildRef) -> bool {
        let Some(folder) = &build.0.folder_name else { return false };
        let sel = list.sel_value.borrow().clone();
        match sel {
            Some(s) => s != *build && can_move_to_sub_path(&s.0, &format!("{}{folder}/", build.0.sub_path)),
            None => false,
        }
    }

    /// BuildListControl.lua:288-294
    fn on_sel_click(&self, list: &ListControl<BuildRef>, g: &mut Gfx, _index: usize, build: &BuildRef, double_click: bool) -> Option<bool> {
        if double_click {
            self.ctl().load_build(g, build);
            list.sel_dragging.set(false);
            list.sel_drag_active.set(false);
        }
        None
    }

    fn on_sel_copy(&self, _list: &ListControl<BuildRef>, _g: &mut Gfx, _index: usize, build: &BuildRef) {
        let ctl = self.ctl();
        *ctl.copy_build.borrow_mut() = Some(build.clone());
        *ctl.cut_build.borrow_mut() = None;
    }

    fn on_sel_cut(&self, _list: &ListControl<BuildRef>, _g: &mut Gfx, _index: usize, build: &BuildRef) {
        let ctl = self.ctl();
        *ctl.copy_build.borrow_mut() = None;
        *ctl.cut_build.borrow_mut() = Some(build.clone());
    }

    fn on_sel_delete(&self, _list: &ListControl<BuildRef>, g: &mut Gfx, _index: usize, build: &BuildRef) {
        self.ctl().delete_build(g, build);
    }

    /// BuildListControl.lua:314-320
    fn on_sel_key_down(&self, _list: &ListControl<BuildRef>, g: &mut Gfx, _index: usize, build: &BuildRef, key: &str) {
        if key == "RETURN" {
            self.ctl().load_build(g, build);
        } else if key == "F2" {
            self.ctl().rename_build(g, build, false);
        }
    }
}

/// `self.controls.path` as a drag target (BuildListControl.lua:33-55).
struct PathTarget(Weak<BuildListControl>);

impl DragTarget<BuildRef> for PathTarget {
    fn can_receive_drag(&self, drag_type: &str, _value: &BuildRef) -> bool {
        let ctl = self.0.upgrade().expect("build list");
        drag_type == "Build" && ctl.path.folder_list.borrow().len() > 1
    }
    fn set_other_drag_source(&self, on: bool) {
        self.0.upgrade().expect("build list").path.other_drag_source.set(on);
    }
    fn other_drag_source(&self) -> bool {
        self.0.upgrade().expect("build list").path.other_drag_source.get()
    }
    fn is_mouse_over(&self, g: &Gfx) -> bool {
        self.0.upgrade().expect("build list").path.mouse_over(g)
    }
    fn receive_drag(&self, g: &mut Gfx, drag_type: &str, build: &BuildRef) {
        if drag_type != "Build" {
            return;
        }
        let ctl = self.0.upgrade().expect("build list");
        let folders = ctl.path.folder_list.borrow().clone();
        for folder in folders {
            if folder.button.is_mouse_over_now(g) {
                ctl.move_into(g, build, &folder.path);
                break;
            }
        }
    }
}

impl BuildListControl {
    /// `BuildListControl:BuildListControl(anchor, rect, listMode)` (BuildListControl.lua:16-62).
    pub fn new(g: &mut Gfx, anchor: Option<Anchor>, r: RectProps, list_mode: &Rc<ListMode>) -> Rc<Self> {
        let width0 = match &r[2] {
            Prop::Val(v) => *v,
            Prop::Fn(f) => f(g),
        };
        let lm_weak = Rc::downgrade(list_mode);
        Rc::new_cyclic(|me: &Weak<Self>| {
            let list = ListControl::new(
                anchor,
                r,
                20.0,
                Some("VERTICAL"),
                false,
                Vec::new(),
                false,
                Box::new(BuildListHooks(me.clone())),
            );
            let lw = Rc::downgrade(&list);
            list.col_list.replace(vec![
                Column {
                    width: Some(Prop::func(move |g| lw.upgrade().expect("list").base.width.get(g) - 172.0)),
                    ..Default::default()
                },
                Column::default(),
            ]);
            list.show_row_separators.set(true);
            let list_dyn: crate::control::WeakControl = Rc::downgrade(&list) as crate::control::WeakControl;
            let sub = list_mode.sub_path.borrow().clone();
            let lw = Rc::downgrade(&list);
            let lmw = lm_weak.clone();
            let path = PathControl::new(
                g,
                anchor_weak("BOTTOM", list_dyn, "TOP"),
                rect(0.0, -2.0, width0, 24.0),
                &main().build_path.borrow(),
                Some(&sub),
                Some(Rc::new(move |g: &mut Gfx, sub_path: &str| {
                    let lm = lmw.upgrade().expect("listMode");
                    *lm.sub_path.borrow_mut() = sub_path.to_string();
                    lm.build_list(g);
                    let l = lw.upgrade().expect("list");
                    l.sel_index.set(None);
                    *l.sel_value.borrow_mut() = None;
                    l.sel_dragging.set(false);
                    l.sel_drag_active.set(false);
                    l.other_drag_source.set(false);
                })),
            );
            list.host.add("path", path.clone());
            let targets: Vec<Rc<dyn DragTarget<BuildRef>>> =
                vec![Rc::new(PathTarget(me.clone())), Rc::new(SelfTarget(Rc::downgrade(&list)))];
            list.set_drag_target_list(targets);
            let lw = Rc::downgrade(&list);
            path.base.width.set_fn(move |g| lw.upgrade().expect("list").base.width.get(g));
            Self {
                list,
                path,
                list_mode: lm_weak,
                copy_build: RefCell::new(None),
                cut_build: RefCell::new(None),
            }
        })
    }

    fn lm(&self) -> Rc<ListMode> {
        self.list_mode.upgrade().expect("listMode")
    }

    /// Moving a build or folder into `target` (the bodies of the two `ReceiveDrag`s, BuildListControl.lua:37-50, :266-279).
    fn move_into(&self, g: &mut Gfx, build: &BuildRef, target: &str) {
        if !can_move_to_sub_path(&build.0, target) {
            return;
        }
        let m = main();
        let bp = m.build_path.borrow().clone();
        if let Some(folder) = &build.0.folder_name {
            m.move_folder(g, folder, &format!("{bp}{}", build.0.sub_path), &format!("{bp}{target}"));
        } else {
            let dest = self.lm().get_dest_name(target, build.0.file_name.as_deref().unwrap_or(""));
            if let Err(msg) = os_rename(&build.0.full_file_name, &dest) {
                m.open_message_popup(
                    g,
                    "Error",
                    &format!("Couldn't move '{}' to '{dest}': {msg}", build.0.full_file_name),
                );
                return;
            }
        }
        self.lm().build_list(g);
    }

    /// `BuildListControl:SelByFullFileName(fullFileName)` (BuildListControl.lua:64-75).
    pub fn sel_by_full_file_name(&self, g: &mut Gfx, full_file_name: Option<&str>) {
        if let Some(name) = full_file_name {
            let idx = self.list.list.borrow().iter().position(|b| b.0.full_file_name == name);
            if let Some(i) = idx {
                self.list.select_index(g, Some(i + 1));
                return;
            }
        }
        self.list.sel_index.set(None);
        *self.list.sel_value.borrow_mut() = None;
    }

    /// `BuildListControl:LoadBuild(build)` (BuildListControl.lua:77-83).
    pub fn load_build(&self, g: &mut Gfx, build: &BuildRef) {
        match &build.0.folder_name {
            Some(folder) => self.path.set_sub_path(g, &format!("{}{folder}/", build.0.sub_path), false),
            None => main().set_mode(
                "BUILD",
                vec![
                    ModeArg::Str(build.0.full_file_name.clone()),
                    ModeArg::Str(build.0.build_name.clone().unwrap_or_default()),
                ],
            ),
        }
    }

    /// `BuildListControl:NewFolder()` (BuildListControl.lua:85-92).
    pub fn new_folder(self: &Rc<Self>, g: &mut Gfx) {
        let m = main();
        let path = format!("{}{}", m.build_path.borrow(), self.lm().sub_path.borrow());
        let me = Rc::downgrade(self);
        m.open_new_folder_popup(
            g,
            &path,
            Some(Rc::new(move |g: &mut Gfx, name: Option<&str>| {
                let s = me.upgrade().expect("build list");
                if name.is_some() {
                    s.lm().build_list(g);
                }
                s.lm().select_control(g, Some(s.list.clone() as ControlRef));
            })),
        );
    }

    /// `BuildListControl:RenameBuild(build, copyOnName)` (BuildListControl.lua:94-167).
    pub fn rename_build(self: &Rc<Self>, g: &mut Gfx, build: &BuildRef, copy_on_name: bool) {
        let mut controls: IndexMap<String, ControlRef> = IndexMap::new();
        let what = if build.0.folder_name.is_some() { "folder:" } else { "build:" };
        controls.insert(
            "label".into(),
            LabelControl::new(None, rect(0.0, 20.0, 0.0, 16.0), format!("^7Enter the new name for this {what}")),
        );
        let save_cell: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
        let (sc, b) = (save_cell.clone(), build.clone());
        let edit = EditControl::new(
            g,
            None,
            rect(0.0, 40.0, 350.0, 20.0),
            EditArgs {
                init: Some(
                    build
                        .0
                        .folder_name
                        .clone()
                        .or_else(|| build.0.build_name.clone())
                        .unwrap_or_default(),
                ),
                filter: Some("\\/:%*%?\"<>|%c".into()),
                limit: Some(100),
                change_func: Some(Rc::new(move |_g, buf: &str, _| {
                    let save = sc.borrow().clone().expect("save");
                    save.base.enabled.set(false);
                    if b.0.folder_name.is_some() {
                        if lua::is_match(buf, "%S") {
                            save.base.enabled.set(true);
                        }
                    } else if lua::is_match(buf, "%S") {
                        let bn = b.0.build_name.clone().unwrap_or_default();
                        if !buf.eq_ignore_ascii_case(&bn) {
                            let new_file = format!("{}{}{buf}.xml", main().build_path.borrow(), b.0.sub_path);
                            if !file_exists(&new_file) {
                                save.base.enabled.set(true);
                            }
                        } else if buf != bn {
                            save.base.enabled.set(true);
                        }
                    }
                })),
                ..Default::default()
            },
        );
        controls.insert("edit".into(), edit.clone());
        let (me, b) = (Rc::downgrade(self), build.clone());
        let save = ButtonControl::new(None, rect(-45.0, 70.0, 80.0, 20.0), "Save".to_string(), move |g| {
            let s = me.upgrade().expect("build list");
            let m = main();
            let bp = m.build_path.borrow().clone();
            let new_name = edit.buf.borrow().clone();
            if b.0.folder_name.is_some() {
                let dst = format!("{bp}{}{new_name}", b.0.sub_path);
                if copy_on_name {
                    m.copy_folder(g, &b.0.full_file_name, &dst);
                } else if let Err(msg) = os_rename(&b.0.full_file_name, &dst) {
                    m.open_message_popup(
                        g,
                        "Error",
                        &format!("Couldn't rename '{}' to '{new_name}': {msg}", b.0.full_file_name),
                    );
                    return;
                }
                s.lm().build_list(g);
            } else {
                let new_file_name = format!("{new_name}.xml");
                let dst = format!("{bp}{}{new_file_name}", b.0.sub_path);
                if copy_on_name {
                    if let Err(msg) = copy_file(&b.0.full_file_name, &dst) {
                        m.open_message_popup(g, "Error", &format!("Couldn't copy build: {msg}"));
                        return;
                    }
                } else if let Err(msg) = os_rename(&b.0.full_file_name, &dst) {
                    m.open_message_popup(
                        g,
                        "Error",
                        &format!("Couldn't rename '{}' to '{new_file_name}': {msg}", b.0.full_file_name),
                    );
                    return;
                }
                s.lm().build_list(g);
                s.sel_by_full_file_name(g, Some(&dst));
            }
            m.close_popup();
            s.lm().select_control(g, Some(s.list.clone() as ControlRef));
        });
        save.base.enabled.set(false);
        *save_cell.borrow_mut() = Some(save.clone());
        controls.insert("save".into(), save);
        let me = Rc::downgrade(self);
        controls.insert(
            "cancel".into(),
            ButtonControl::new(None, rect(45.0, 70.0, 80.0, 20.0), "Cancel".to_string(), move |g| {
                main().close_popup();
                let s = me.upgrade().expect("build list");
                s.lm().select_control(g, Some(s.list.clone() as ControlRef));
            }),
        );
        let title = format!(
            "{}{}",
            if copy_on_name { "Copy " } else { "Rename " },
            if build.0.folder_name.is_some() { "Folder" } else { "Build" }
        );
        main().open_popup(g, 370.0, 100.0, &title, controls, Some("save"), Some("edit"), None, None);
    }

    /// `BuildListControl:DeleteBuild(build)` (BuildListControl.lua:169-209).
    pub fn delete_build(self: &Rc<Self>, g: &mut Gfx, build: &BuildRef) {
        let m = main();
        let me = Rc::downgrade(self);
        let after = move |g: &mut Gfx| {
            let s = me.upgrade().expect("build list");
            s.lm().build_list(g);
            s.list.sel_index.set(None);
            *s.list.sel_value.borrow_mut() = None;
        };
        if let Some(folder) = &build.0.folder_name {
            let full = build.0.full_file_name.clone();
            if pob_gfx::fs::new_file_search(&format!("{full}/*"), false).is_some()
                || pob_gfx::fs::new_file_search(&format!("{full}/*"), true).is_some()
            {
                m.open_confirm_popup(
                    g,
                    "Confirm Folder Delete",
                    &format!("The folder is not empty.\nAre you sure you want to delete folder:\n{folder}\nThis cannot be undone."),
                    "Delete",
                    Rc::new(move |g: &mut Gfx| {
                        let _ = pob_gfx::fs::remove_dir(&full, true);
                        after(g);
                    }),
                    None,
                );
            } else {
                if let Err(msg) = pob_gfx::fs::remove_dir(&full, false) {
                    m.open_message_popup(g, "Error", &format!("Couldn't delete '{full}': {msg}"));
                    return;
                }
                after(g);
            }
        } else {
            let full = build.0.full_file_name.clone();
            m.open_confirm_popup(
                g,
                "Confirm Delete",
                &format!(
                    "Are you sure you want to delete build:\n{}\nThis cannot be undone.",
                    build.0.build_name.as_deref().unwrap_or("")
                ),
                "Delete",
                Rc::new(move |g: &mut Gfx| {
                    let _ = os_remove(&full);
                    after(g);
                }),
                None,
            );
        }
    }
}

// ================================================================================================ FolderListControl

/// `{ name, fullFileName, modified }` of the folder list.
#[derive(Clone, Debug)]
pub struct FolderEntry {
    pub name: String,
    pub full_file_name: String,
    pub modified: f64,
}

pub type FolderRef = ByRef<FolderEntry>;

/// `FolderListControl` (FolderListControl.lua): the folders under `main.buildPath..subPath`.
pub struct FolderListControl {
    pub list: Rc<ListControl<FolderRef>>,
    pub path: Rc<PathControl>,
    pub sub_path: RefCell<String>,
    pub sort_mode: RefCell<String>,
    pub on_change: Option<Rc<dyn Fn(&mut Gfx, &str)>>,
}

struct FolderListHooks(Weak<FolderListControl>);

impl ListHooks<FolderRef> for FolderListHooks {
    /// FolderListControl.lua:66-70
    fn get_row_value(&self, _list: &ListControl<FolderRef>, _g: &Gfx, column: usize, _index: usize, folder: &FolderRef) -> String {
        if column == 1 { folder.0.name.clone() } else { String::new() }
    }

    /// FolderListControl.lua:72-76
    fn on_sel_click(
        &self,
        _list: &ListControl<FolderRef>,
        g: &mut Gfx,
        _index: usize,
        folder: &FolderRef,
        double_click: bool,
    ) -> Option<bool> {
        if double_click {
            self.0.upgrade().expect("folder list").open_folder(g, &folder.0.name);
        }
        None
    }

    /// FolderListControl.lua:78-90
    fn on_sel_delete(&self, list: &ListControl<FolderRef>, g: &mut Gfx, _index: usize, folder: &FolderRef) {
        let full = &folder.0.full_file_name;
        if pob_gfx::fs::new_file_search(&format!("{full}/*"), false).is_some()
            || pob_gfx::fs::new_file_search(&format!("{full}/*"), true).is_some()
        {
            main().open_message_popup(g, "Delete Folder", "The folder is not empty.");
        } else {
            if let Err(msg) = pob_gfx::fs::remove_dir(full, false) {
                main().open_message_popup(g, "Error", &format!("Couldn't delete '{full}': {msg}"));
                return;
            }
            self.0.upgrade().expect("folder list").build_list(g);
            list.sel_index.set(None);
            *list.sel_value.borrow_mut() = None;
        }
    }
}

impl FolderListControl {
    /// `FolderListControl:FolderListControl(anchor, rect, subPath, onChange)` (FolderListControl.lua:12-28).
    pub fn new(
        g: &mut Gfx,
        anchor: Option<Anchor>,
        r: RectProps,
        sub_path: Option<&str>,
        on_change: Option<Rc<dyn Fn(&mut Gfx, &str)>>,
    ) -> Rc<Self> {
        let width0 = match &r[2] {
            Prop::Val(v) => *v,
            Prop::Fn(f) => f(g),
        };
        let fl = Rc::new_cyclic(|me: &Weak<Self>| {
            let list = ListControl::new(
                anchor,
                r,
                16.0,
                Some("VERTICAL"),
                false,
                Vec::new(),
                false,
                Box::new(FolderListHooks(me.clone())),
            );
            let list_dyn: crate::control::WeakControl = Rc::downgrade(&list) as crate::control::WeakControl;
            let mw = me.clone();
            let path = PathControl::new(
                g,
                anchor_weak("BOTTOM", list_dyn, "TOP"),
                rect(0.0, -2.0, width0, 24.0),
                &main().build_path.borrow(),
                Some(sub_path.unwrap_or("")),
                Some(Rc::new(move |g: &mut Gfx, new_sub: &str| {
                    let s = mw.upgrade().expect("folder list");
                    *s.sub_path.borrow_mut() = new_sub.to_string();
                    s.build_list(g);
                    s.list.sel_index.set(None);
                    *s.list.sel_value.borrow_mut() = None;
                    if let Some(oc) = &s.on_change {
                        oc(g, new_sub);
                    }
                })),
            );
            list.host.add("path", path.clone());
            Self {
                list,
                path,
                sub_path: RefCell::new(sub_path.unwrap_or("").to_string()),
                sort_mode: RefCell::new("NAME".into()),
                on_change,
            }
        });
        fl.build_list(g);
        fl
    }

    /// FolderListControl.lua:30-46
    pub fn sort_list(&self) {
        let edited = *self.sort_mode.borrow() == "EDITED";
        let less = move |a: &FolderEntry, b: &FolderEntry| {
            if edited && a.modified != b.modified {
                return a.modified > b.modified;
            }
            crate::modules::build_list_helpers::natural_sort_compare(&a.name, &b.name)
        };
        self.list.list.borrow_mut().sort_by(|a, b| {
            if less(&a.0, &b.0) {
                std::cmp::Ordering::Less
            } else if less(&b.0, &a.0) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
    }

    /// FolderListControl.lua:48-64 (`UpdateScrollbar` / `Redraw` / `Close` do not exist on these classes).
    pub fn build_list(&self, _g: &mut Gfx) {
        let bp = main().build_path.borrow().clone();
        let mut out = Vec::new();
        let mut handle = pob_gfx::fs::new_file_search(&format!("{bp}{}*", self.sub_path.borrow()), true);
        while let Some(h) = &mut handle {
            let name = h.get_file_name().to_string();
            out.push(ByRef(Rc::new(FolderEntry {
                full_file_name: format!("{bp}{}{name}", self.sub_path.borrow()),
                name,
                modified: h.get_file_modified_time(),
            })));
            if !h.next_file() {
                break;
            }
        }
        *self.list.list.borrow_mut() = out;
        self.sort_list();
    }

    /// FolderListControl.lua:62-64
    pub fn open_folder(&self, g: &mut Gfx, folder_name: &str) {
        let sub = format!("{}{folder_name}/", self.sub_path.borrow());
        self.path.set_sub_path(g, &sub, false);
    }
}

/// `button:IsMouseOver()` for a button held by type.
trait MouseOverNow {
    fn is_mouse_over_now(&self, g: &Gfx) -> bool;
}

impl MouseOverNow for ButtonControl {
    fn is_mouse_over_now(&self, g: &Gfx) -> bool {
        crate::control::Control::is_mouse_over(self, g) == Some(true)
    }
}
