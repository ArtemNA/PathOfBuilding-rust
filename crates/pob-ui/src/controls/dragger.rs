//! `Classes/DraggerControl.lua` (a button that reports drags) and `Classes/ResizableEditControl.lua` (an `EditControl`
//! with a dragger in its bottom-right corner).

use crate::control::{Anchor, Control, ControlBase, ControlRef, PropCell, Rect, RectProps, anchor_weak, same};
use crate::controls::edit::{EditArgs, EditControl};
use crate::sg;
use crate::tooltip::TooltipHost;
use pob_gfx::{Frame, Gfx, ImageHandle};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// `{ X = …, Y = … }` passed to the dragger callbacks.
pub type DragFunc = Rc<dyn Fn(&mut Gfx, f64, f64)>;

// =================================================================================================== DraggerControl

/// `DraggerControl` (DraggerControl.lua): `Control` + `TooltipHost`.
pub struct DraggerControl {
    pub base: ControlBase,
    pub tip: TooltipHost<()>,
    pub label: PropCell<String>,
    pub on_key_down: RefCell<Option<DragFunc>>,
    pub on_key_up: RefCell<Option<DragFunc>>,
    pub on_right_click: RefCell<Option<DragFunc>>,
    pub on_hover: RefCell<Option<Rc<dyn Fn(&Gfx)>>>,
    pub force_tooltip: Cell<bool>,
    pub cursor_x: Cell<f64>,
    pub cursor_y: Cell<f64>,
    pub clicked: Cell<bool>,
    pub dragging: Cell<bool>,
    pub image: RefCell<Option<ImageHandle>>,
    /// `GetProperty("locked")` (`nil` = false).
    pub locked: PropCell<bool>,
    pub over_size_text: Cell<Option<f64>>,
}

impl DraggerControl {
    /// `DraggerControl:DraggerControl(anchor, rect, label, onKeyDown, onKeyUp, onRightClick, onHover, forceTooltip)`
    /// (DraggerControl.lua:9-21).
    pub fn new(
        anchor: Option<Anchor>,
        rect: RectProps,
        label: impl Into<crate::control::Prop<String>>,
        on_key_down: Option<DragFunc>,
        on_key_up: Option<DragFunc>,
        on_right_click: Option<DragFunc>,
    ) -> Rc<Self> {
        Rc::new(Self {
            base: ControlBase::new(anchor, rect),
            tip: TooltipHost::new(None),
            label: PropCell::new(label),
            on_key_down: RefCell::new(on_key_down),
            on_key_up: RefCell::new(on_key_up),
            on_right_click: RefCell::new(on_right_click),
            on_hover: RefCell::new(None),
            force_tooltip: Cell::new(false),
            cursor_x: Cell::new(0.0),
            cursor_y: Cell::new(0.0),
            clicked: Cell::new(false),
            dragging: Cell::new(false),
            image: RefCell::new(None),
            locked: PropCell::new(false),
            over_size_text: Cell::new(None),
        })
    }

    /// `DraggerControl:SetImage(path)` (DraggerControl.lua:23-30).
    pub fn set_image(&self, path: Option<&str>) {
        *self.image.borrow_mut() = path.map(|p| {
            let h = ImageHandle::new();
            h.load_lua(p, &[]).unwrap_or_else(|e| panic!("{e}"));
            h
        });
    }

    /// DraggerControl.lua:32-37
    fn mouse_over(&self, g: &Gfx) -> bool {
        self.is_shown(g) && self.is_mouse_in_bounds(g)
    }

    fn call(f: &RefCell<Option<DragFunc>>, g: &mut Gfx, x: f64, y: f64) {
        let f = f.borrow().clone();
        if let Some(f) = f {
            f(g, x, y);
        }
    }
}

impl Control for DraggerControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }

    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        Some(self.mouse_over(g))
    }

    fn has_draw(&self) -> bool {
        true
    }

    /// DraggerControl.lua:39-109
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let (width, height) = self.get_size(f);
        let enabled = self.is_enabled(f);
        let m_over = self.mouse_over(f);
        let locked = self.locked.get(f);
        if !enabled {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else if m_over || locked {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        sg::draw_rect(f, x, y, width, height);
        if !enabled {
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
        } else if self.clicked.get() && m_over {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        } else if m_over || locked {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else {
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
        }
        sg::draw_rect(f, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        let image = self.image.borrow().clone();
        if let Some(image) = image {
            if enabled {
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
            } else {
                sg::set_draw_color(f, 0.33, 0.33, 0.33);
            }
            sg::draw_image(f, Some(&image), x + 2.0, y + 2.0, width - 4.0, height - 4.0, None);
            if self.clicked.get() && m_over {
                sg::set_draw_color_a(f, 1.0, 1.0, 1.0, 0.5);
                sg::draw_rect(f, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
            }
        }
        if !enabled {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else if m_over || locked || self.dragging.get() {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        let label = self.label.get(f);
        let (w, h) = (width, height);
        match label.as_str() {
            "+" => {
                sg::draw_rect(f, x + w * 0.2, y + h * 0.45, w * 0.6, h * 0.1);
                sg::draw_rect(f, x + w * 0.45, y + h * 0.2, w * 0.1, h * 0.6);
            }
            "-" => sg::draw_rect(f, x + w * 0.2, y + h * 0.45, w * 0.6, h * 0.1),
            "x" => {
                #[rustfmt::skip]
                sg::draw_quad(f, [x + w * 0.2, y + h * 0.3, x + w * 0.3, y + h * 0.2, x + w * 0.8, y + h * 0.7, x + w * 0.7, y + h * 0.8]);
                #[rustfmt::skip]
                sg::draw_quad(f, [x + w * 0.7, y + h * 0.2, x + w * 0.8, y + h * 0.3, x + w * 0.3, y + h * 0.8, x + w * 0.2, y + h * 0.7]);
            }
            "//" => {
                #[rustfmt::skip]
                sg::draw_quad(f, [x + w * 0.75, y + h * 0.15, x + w * 0.85, y + h * 0.25, x + w * 0.25, y + h * 0.85, x + w * 0.15, y + h * 0.75]);
                #[rustfmt::skip]
                sg::draw_quad(f, [x + w * 0.75, y + h * 0.5, x + w * 0.85, y + h * 0.6, x + w * 0.6, y + h * 0.85, x + w * 0.5, y + h * 0.75]);
            }
            _ => {
                let over_size = self.over_size_text.get().unwrap_or(0.0);
                sg::draw_string(
                    f,
                    x + w / 2.0,
                    y + 2.0 - over_size,
                    Some("CENTER_X"),
                    h - 4.0 + over_size * 2.0,
                    "VAR",
                    &label,
                );
            }
        }
        if m_over {
            if !no_tooltip || self.force_tooltip.get() {
                sg::set_draw_layer(f, None, 100);
                self.tip.draw_tooltip(f, x, y, width, height, view_port, ());
                sg::set_draw_layer(f, None, 0);
            }
            let on_hover = self.on_hover.borrow().clone();
            if let Some(h) = on_hover {
                h(f);
            }
        }
    }

    fn has_on_key_down(&self) -> bool {
        true
    }

    /// DraggerControl.lua:111-127
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, _double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) || self.locked.get(g) {
            return None;
        }
        if key == "LEFTBUTTON" {
            let (cursor_x, cursor_y) = g.get_cursor_pos();
            self.clicked.set(true);
            self.dragging.set(true);
            Self::call(&self.on_key_down, g, cursor_x, cursor_y);
            self.cursor_x.set(cursor_x);
            self.cursor_y.set(cursor_y);
        }
        Some(self)
    }

    fn has_on_key_up(&self) -> bool {
        true
    }

    /// DraggerControl.lua:128-145
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) || self.locked.get(g) {
            return None;
        }
        let (cursor_x, cursor_y) = g.get_cursor_pos();
        if key == "LEFTBUTTON" {
            self.clicked.set(false);
            self.dragging.set(false);
            Self::call(&self.on_key_up, g, self.cursor_x.get() - cursor_x, self.cursor_y.get() - cursor_y);
        }
        if key == "RIGHTBUTTON" && self.is_mouse_in_bounds(g) {
            Self::call(&self.on_right_click, g, cursor_x, cursor_y);
        }
        Some(self)
    }
}

// ============================================================================================= ResizableEditControl

/// `ResizableEditControl` (ResizableEditControl.lua): an `EditControl` subclass. The edit is held inside; this wrapper
/// is what the parent hosts and what becomes `selControl` (the edit's `return self` is mapped back to the wrapper).
pub struct ResizableEditControl {
    pub edit: Rc<EditControl>,
    pub min_width: f64,
    pub max_width: f64,
    pub min_height: f64,
    pub max_height: f64,
    pub dragger_height: Rc<DraggerControl>,
}

impl ResizableEditControl {
    /// `ResizableEditControl:ResizableEditControl(anchor, rect, init, prompt, filter, limit, changeFunc, lineHeight,
    /// allowZoom, clearable)` (ResizableEditControl.lua:12-32): `rect = {x, y, width, height, minWidth, minHeight,
    /// maxWidth, maxHeight}` (numbers; missing limits = the size).
    pub fn new(g: &Gfx, anchor: Option<Anchor>, rect: [Option<f64>; 8], a: EditArgs) -> Rc<Self> {
        let n = |i: usize| rect[i].unwrap_or(0.0);
        let edit = EditControl::new(g, anchor, crate::control::rect(n(0), n(1), n(2), n(3)), a);
        let (width, height) = (rect[2], rect[3]);
        let min_height = rect[5].or(height).unwrap_or(0.0);
        let max_height = rect[7].or(height).unwrap_or(0.0);
        let min_width = rect[4].or(width).unwrap_or(0.0);
        let max_width = rect[6].or(width).unwrap_or(0.0);
        Rc::new_cyclic(|me: &std::rc::Weak<Self>| {
            let edit_dyn: crate::control::WeakControl = Rc::downgrade(&edit) as crate::control::WeakControl;
            let m = me.clone();
            let dragger = DraggerControl::new(
                anchor_weak("BOTTOMRIGHT", edit_dyn, "BOTTOMRIGHT"),
                crate::control::rect(7.0, 7.0, 14.0, 14.0),
                "//".to_string(),
                None,
                None,
                Some(Rc::new(move |g: &mut Gfx, _x, _y| {
                    let s = m.upgrade().expect("resizable edit");
                    let b = &s.edit.base;
                    if b.height.get(g) != s.min_height || b.width.get(g) != s.min_width {
                        s.set_width(Some(s.min_width));
                        s.set_height(Some(s.min_height));
                    } else {
                        s.set_width(Some(s.max_width));
                        s.set_height(Some(s.max_height));
                    }
                })),
            );
            edit.host.add("draggerHeight", dragger.clone());
            edit.protected.set(false);
            Self {
                edit,
                min_width,
                max_width,
                min_height,
                max_height,
                dragger_height: dragger,
            }
        })
    }

    /// ResizableEditControl.lua:39-46
    fn set_bounded_drag(&self, g: &Gfx) {
        if self.dragger_height.dragging.get() {
            let (cursor_x, cursor_y) = g.get_cursor_pos();
            let (x, y) = self.get_pos(g);
            self.set_height(Some(cursor_y - y));
            self.set_width(Some(cursor_x - x));
        }
    }

    /// ResizableEditControl.lua:48-50 (`m_max(m_min(width or 0, maxWidth), minWidth)`).
    pub fn set_width(&self, width: Option<f64>) {
        self.edit
            .base
            .width
            .set(width.unwrap_or(0.0).min(self.max_width).max(self.min_width));
    }

    /// ResizableEditControl.lua:51-53
    pub fn set_height(&self, height: Option<f64>) {
        self.edit
            .base
            .height
            .set(height.unwrap_or(0.0).min(self.max_height).max(self.min_height));
    }

    /// The edit returned itself as the new `selControl`: the hosted control is this wrapper.
    fn map_sel(self: &Rc<Self>, r: Option<ControlRef>) -> Option<ControlRef> {
        match r {
            Some(c) if same(&*c, &*self.edit) => Some(Rc::clone(self) as ControlRef),
            other => other,
        }
    }
}

impl Control for ResizableEditControl {
    fn base(&self) -> &ControlBase {
        &self.edit.base
    }
    fn get_pos(&self, g: &Gfx) -> (f64, f64) {
        self.edit.get_pos(g)
    }
    fn is_shown(&self, g: &Gfx) -> bool {
        self.edit.is_shown(g)
    }
    fn on_focus_gained(&self, g: &mut Gfx) {
        self.edit.on_focus_gained(g);
    }
    fn on_focus_lost(&self, g: &mut Gfx) {
        self.edit.on_focus_lost(g);
    }
    fn set_focus(&self, g: &mut Gfx, focus: bool) {
        self.edit.set_focus(g, focus);
    }
    fn has_draw(&self) -> bool {
        true
    }
    /// ResizableEditControl.lua:34-37
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, no_tooltip: bool) {
        self.set_bounded_drag(f);
        self.edit.draw(f, view_port, no_tooltip);
    }
    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        self.edit.is_mouse_over(g)
    }
    fn has_on_key_down(&self) -> bool {
        true
    }
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, double_click: bool) -> Option<ControlRef> {
        let r = Rc::clone(&self.edit).on_key_down(g, key, double_click);
        self.map_sel(r)
    }
    fn has_on_key_up(&self) -> bool {
        true
    }
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        let r = Rc::clone(&self.edit).on_key_up(g, key);
        self.map_sel(r)
    }
    fn has_on_char(&self) -> bool {
        true
    }
    fn on_char(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        let r = Rc::clone(&self.edit).on_char(g, key);
        self.map_sel(r)
    }
}
