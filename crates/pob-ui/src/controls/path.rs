//! `Classes/PathControl.lua`: a breadcrumb of folder buttons (`Control` + `ControlHost` + `UndoHandler`).

use crate::common::draw_arrow;
use crate::control::{Anchor, Control, ControlBase, ControlRef, Rect, RectProps, WeakControl, anchor_weak, rect};
use crate::controls::ButtonControl;
use crate::host::ControlHost;
use crate::sg;
use crate::undo::UndoHandler;
use pob_gfx::{Frame, Gfx};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// `{ label, path, button }` of `self.folderList`.
#[derive(Clone)]
pub struct PathFolder {
    pub label: String,
    pub path: String,
    pub button: Rc<ButtonControl>,
}

pub type OnChange = Rc<dyn Fn(&mut Gfx, &str)>;

pub struct PathControl {
    pub base: ControlBase,
    pub host: ControlHost,
    pub undo: UndoHandler<String>,
    pub base_path: String,
    pub base_name: String,
    pub sub_path: RefCell<Option<String>>,
    pub folder_list: RefCell<Vec<PathFolder>>,
    pub on_change: RefCell<Option<OnChange>>,
    /// `self.otherDragSource` (a list dragging onto this control).
    pub other_drag_source: Cell<bool>,
    me: Weak<PathControl>,
}

impl PathControl {
    /// `PathControl:PathControl(anchor, rect, basePath, subPath, onChange)` (PathControl.lua:12-22).
    pub fn new(
        g: &mut Gfx,
        anchor: Option<Anchor>,
        rect: RectProps,
        base_path: &str,
        sub_path: Option<&str>,
        on_change: Option<OnChange>,
    ) -> Rc<Self> {
        let p = Rc::new_cyclic(|me: &Weak<Self>| Self {
            base: ControlBase::new(anchor, rect),
            host: ControlHost::new(),
            undo: UndoHandler::default(),
            base_path: base_path.to_string(),
            base_name: crate::lua::lmatch(base_path, "([^/]+)/$").unwrap_or_else(|| "Base".into()),
            sub_path: RefCell::new(None),
            folder_list: RefCell::new(Vec::new()),
            on_change: RefCell::new(None),
            other_drag_source: Cell::new(false),
            me: me.clone(),
        });
        p.set_sub_path(g, sub_path.unwrap_or(""), false);
        p.reset_undo();
        *p.on_change.borrow_mut() = on_change;
        p
    }

    pub fn sub_path(&self) -> String {
        self.sub_path.borrow().clone().unwrap_or_default()
    }

    /// `PathControl:SetSubPath(subPath, noUndo)` (PathControl.lua:24-61).
    pub fn set_sub_path(&self, g: &mut Gfx, sub_path: &str, no_undo: bool) {
        if self.sub_path.borrow().as_deref() == Some(sub_path) {
            return;
        }
        *self.sub_path.borrow_mut() = Some(sub_path.to_string());
        let mut folders = vec![(self.base_name.clone(), String::new())];
        // `subPath:gmatch("([^/]+)()")`
        let b = sub_path.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'/' {
                i += 1;
                continue;
            }
            let start = i;
            while i < b.len() && b[i] != b'/' {
                i += 1;
            }
            // `subPath:sub(1, endIndex)`: `endIndex` is the position after the name, so the slash is in the path
            folders.push((sub_path[start..i].to_string(), sub_path[..(i + 1).min(b.len())].to_string()));
        }
        let me_dyn: WeakControl = self.me.clone();
        let mut x = 2.0;
        let mut list = Vec::new();
        let mut n = 1;
        for (label, path) in folders {
            let name = format!("folder{n}");
            let button: Rc<ButtonControl> = match self.host.get(&name) {
                Some(b) => {
                    let any: Rc<dyn std::any::Any> = b;
                    any.downcast::<ButtonControl>().unwrap_or_else(|_| panic!("{name} is a button"))
                }
                None => {
                    let h = self.base.height.get(g) - 4.0;
                    let b = ButtonControl::new(
                        anchor_weak("LEFT", me_dyn.clone(), "LEFT"),
                        rect(0.0, 0.0, 0.0, h),
                        String::new(),
                        |_| {},
                    );
                    self.host.add(&name, b.clone());
                    b
                }
            };
            button.base.shown.set(true);
            button.base.x.set(x);
            button.label.set(label.clone());
            let w = sg::draw_string_width(g, self.base.height.get(g) - 8.0, "VAR", &label) + 10.0;
            button.base.width.set(w);
            let (me, p) = (self.me.clone(), path.clone());
            *button.on_click.borrow_mut() = Rc::new(move |g: &mut Gfx| {
                if let Some(s) = me.upgrade() {
                    s.set_sub_path(g, &p, false);
                }
            });
            list.push(PathFolder { label, path, button });
            x += w + 12.0;
            n += 1;
        }
        *self.folder_list.borrow_mut() = list;
        while let Some(b) = self.host.get(&format!("folder{n}")) {
            b.base().shown.set(false);
            n += 1;
        }
        let oc = self.on_change.borrow().clone();
        if let Some(oc) = oc {
            oc(g, sub_path);
        }
        if !no_undo {
            self.add_undo_state(false);
        }
    }

    /// UndoHandler.lua:22-26 `ResetUndo`.
    pub fn reset_undo(&self) {
        let mut u = self.undo.undo.borrow_mut();
        u.clear();
        self.undo.redo.borrow_mut().clear();
        u.push(self.sub_path());
    }

    /// UndoHandler.lua:31-38 `AddUndoState`.
    fn add_undo_state(&self, no_clear_redo: bool) {
        {
            let mut u = self.undo.undo.borrow_mut();
            u.insert(0, self.sub_path());
            u.truncate(101);
        }
        self.undo.mod_flag.set(true);
        if !no_clear_redo {
            self.undo.redo.borrow_mut().clear();
        }
    }

    /// UndoHandler.lua:41-47 `Undo` (`RestoreUndoState` = `SetSubPath(state, true)`, PathControl.lua:95-97).
    pub fn undo(&self, g: &mut Gfx) {
        if self.undo.undo.borrow().len() >= 2 {
            let top = self.undo.undo.borrow_mut().remove(0);
            self.undo.redo.borrow_mut().insert(0, top);
            let s = self.undo.undo.borrow_mut().remove(0);
            self.set_sub_path(g, &s, true);
            self.add_undo_state(true);
        }
    }

    /// UndoHandler.lua:50-55 `Redo`.
    pub fn redo(&self, g: &mut Gfx) {
        if !self.undo.redo.borrow().is_empty() {
            let s = self.undo.redo.borrow_mut().remove(0);
            self.set_sub_path(g, &s, true);
            self.add_undo_state(true);
        }
    }

    /// PathControl.lua:63-68
    pub fn mouse_over(&self, g: &Gfx) -> bool {
        self.is_shown(g) && (self.is_mouse_in_bounds(g) || self.host.get_mouse_over_control(g).is_some())
    }
}

impl Control for PathControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }

    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        Some(self.mouse_over(g))
    }

    fn has_draw(&self) -> bool {
        true
    }

    /// PathControl.lua:70-86
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, _no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let (width, height) = self.get_size(f);
        sg::set_draw_color(f, 0.5, 0.5, 0.5);
        sg::draw_rect(f, x, y, width, height);
        sg::set_draw_color(f, 0.0, 0.0, 0.0);
        sg::draw_rect(f, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        self.host.draw_controls(f, view_port, None);
        let list = self.folder_list.borrow().clone();
        let n = list.len();
        for (i, folder) in list.iter().enumerate() {
            let (bx, by) = folder.button.get_pos(f);
            let (bw, bh) = folder.button.get_size(f);
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            draw_arrow(f, bx + bw + 6.0, y + height / 2.0, 8.0, 8.0, "RIGHT");
            if self.other_drag_source.get() && i + 1 < n {
                sg::set_draw_color_a(f, 0.0, 1.0, 0.0, 0.25);
                sg::draw_rect(f, bx, by, bw, bh);
            }
        }
    }

    fn has_on_key_down(&self) -> bool {
        true
    }

    /// PathControl.lua:88-97
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, _double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        let m = self.host.get_mouse_over_control(g)?;
        if m.has_on_key_down() {
            return m.on_key_down(g, key, false);
        }
        None
    }
}
