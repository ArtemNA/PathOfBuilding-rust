//! `ScrollBarControl.lua` and `SliderControl.lua`.

use crate::common::{draw_arrow, invert_slider_scroll_direction};
use crate::control::{Anchor, Control, ControlBase, ControlRef, PropCell, Rect, RectProps};
use crate::sg;
use crate::tooltip::TooltipHost;
use pob_gfx::{Frame, Gfx};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

// ================================================================================================= ScrollBarControl

/// `ScrollBarControl` (ScrollBarControl.lua). `self.enabled` is assigned booleans by the class itself.
pub struct ScrollBarControl {
    pub base: ControlBase,
    pub step: Cell<f64>,
    /// `"VERTICAL"` / `"HORIZONTAL"`.
    pub dir: &'static str,
    pub offset: Cell<f64>,
    pub con_dim: Cell<Option<f64>>,
    pub view_dim: Cell<Option<f64>>,
    pub offset_max: Cell<Option<f64>>,
    pub knob_dim: Cell<Option<f64>>,
    pub knob_travel: Cell<Option<f64>>,
    pub dragging: Cell<bool>,
    pub drag_cx: Cell<f64>,
    pub drag_cy: Cell<f64>,
    pub drag_knob_pos: Cell<f64>,
    pub hold_comp: Cell<Option<&'static str>>,
    pub hold_time: Cell<f64>,
    pub hold_base: Cell<f64>,
    pub hold_repeating: Cell<bool>,
    pub hold_pause_time: Cell<Option<f64>>,
    /// `GetProperty("locked")`.
    pub locked: PropCell<bool>,
}

impl ScrollBarControl {
    /// `ScrollBarControl:ScrollBarControl(anchor, rect, step, dir, autoHide)` (ScrollBarControl.lua:14-26).
    pub fn new(anchor: Option<Anchor>, rect: RectProps, step: Option<f64>, dir: Option<&'static str>, auto_hide: bool) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| {
            let base = ControlBase::new(anchor, rect);
            let step = step.unwrap_or_else(|| base.width.raw().expect("ScrollBarControl width must be a number") * 2.0);
            base.enabled.set(false);
            if auto_hide {
                let me = me.clone();
                base.shown.set_fn(move |g| me.upgrade().expect("scrollbar").base.enabled.get(g));
            }
            Self {
                base,
                step: Cell::new(step),
                dir: dir.unwrap_or("VERTICAL"),
                offset: Cell::new(0.0),
                con_dim: Cell::new(None),
                view_dim: Cell::new(None),
                offset_max: Cell::new(None),
                knob_dim: Cell::new(None),
                knob_travel: Cell::new(None),
                dragging: Cell::new(false),
                drag_cx: Cell::new(0.0),
                drag_cy: Cell::new(0.0),
                drag_knob_pos: Cell::new(0.0),
                hold_comp: Cell::new(None),
                hold_time: Cell::new(0.0),
                hold_base: Cell::new(0.0),
                hold_repeating: Cell::new(false),
                hold_pause_time: Cell::new(None),
                locked: PropCell::new(false),
            }
        })
    }

    /// ScrollBarControl.lua:28-48
    pub fn set_content_dimension(&self, g: &Gfx, con_dim: f64, view_dim: f64) {
        self.con_dim.set(Some(con_dim));
        self.view_dim.set(Some(view_dim));
        if con_dim <= view_dim {
            self.base.enabled.set(false);
            self.offset_max.set(Some(0.0));
            self.offset.set(0.0);
        } else {
            let (width, height) = self.get_size(g);
            self.base.enabled.set(true);
            let (knob_dim, knob_travel) = if self.dir == "HORIZONTAL" {
                let k = height.max((width - height * 2.0) * view_dim / con_dim);
                (k, (width - height * 2.0) - k)
            } else {
                let k = width.max((height - width * 2.0) * view_dim / con_dim);
                (k, (height - width * 2.0) - k)
            };
            self.knob_dim.set(Some(knob_dim));
            self.knob_travel.set(Some(knob_travel));
            self.offset_max.set(Some(con_dim - view_dim));
            self.offset.set(self.offset.get().min(con_dim - view_dim));
        }
    }

    /// ScrollBarControl.lua:50-52
    pub fn set_offset(&self, offset: f64) {
        self.offset.set(0f64.max(self.offset_max.get().unwrap_or(0.0).min(offset)).floor());
    }

    /// ScrollBarControl.lua:54-56
    pub fn scroll(&self, mult: f64) {
        self.set_offset(self.offset.get() + self.step.get() * mult);
    }

    /// ScrollBarControl.lua:58-64
    pub fn scroll_into_view(&self, min_dim: f64, size: f64) {
        let view_dim = self.view_dim.get().expect("ScrollIntoView before SetContentDimension");
        if self.offset.get() > min_dim {
            self.set_offset(min_dim);
        } else if self.offset.get() + view_dim < min_dim + size {
            self.set_offset(min_dim + size - view_dim);
        }
    }

    fn knob_travel(&self) -> f64 {
        self.knob_travel
            .get()
            .expect("attempt to perform arithmetic on field 'knobTravel' (a nil value)")
    }

    /// ScrollBarControl.lua:66-68
    pub fn set_offset_from_knob_pos(&self, knob_pos: f64) {
        self.set_offset(self.offset_max.get().unwrap_or(f64::NAN) * (knob_pos / self.knob_travel()));
    }

    /// ScrollBarControl.lua:70-72
    pub fn get_knob_pos_for_offset(&self) -> f64 {
        self.knob_travel() * (self.offset.get() / self.offset_max.get().unwrap_or(f64::NAN))
    }

    /// `ScrollBarControl:IsMouseOver()` → `mOver, mOverComp` (ScrollBarControl.lua:74-111).
    pub fn mouse_over(&self, g: &Gfx) -> (bool, Option<&'static str>) {
        if !self.is_shown(g) {
            return (false, None);
        }
        let (x, y) = self.get_pos(g);
        let (width, height) = self.get_size(g);
        let (cursor_x, cursor_y) = g.get_cursor_pos();
        let m_over = cursor_x >= x && cursor_y >= y && cursor_x < x + width && cursor_y < y + height;
        let mut comp = None;
        if m_over && self.base.enabled.get(g) {
            let (rel_dim, short_dim, long_dim) = if self.dir == "HORIZONTAL" {
                (cursor_x - x, height, width)
            } else {
                (cursor_y - y, width, height)
            };
            comp = Some(if rel_dim < short_dim {
                "UP"
            } else if rel_dim >= long_dim - short_dim {
                "DOWN"
            } else {
                let knob_pos = self.get_knob_pos_for_offset();
                let knob_dim = self.knob_dim.get().unwrap_or(f64::NAN);
                if rel_dim < short_dim + knob_pos {
                    "SLIDEUP"
                } else if rel_dim >= short_dim + knob_pos + knob_dim {
                    "SLIDEDOWN"
                } else {
                    "KNOB"
                }
            });
        }
        (m_over, comp)
    }

    /// ScrollBarControl.lua:322-327
    pub fn is_scroll_down_key(key: &str) -> bool {
        matches!(key, "WHEELDOWN" | "PAGEDOWN")
    }
    pub fn is_scroll_up_key(key: &str) -> bool {
        matches!(key, "WHEELUP" | "PAGEUP")
    }
}

impl Control for ScrollBarControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }

    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        Some(self.mouse_over(g).0)
    }

    fn has_draw(&self) -> bool {
        true
    }

    /// ScrollBarControl.lua:113-257
    fn draw(&self, f: &mut Frame<'_>, _vp: &Rect, _no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let (width, height) = self.get_size(f);
        let enabled = self.is_enabled(f);
        let (_m_over, m_over_comp) = self.mouse_over(f);
        let dir = self.dir;
        if !sg::is_key_down(f, "LEFTBUTTON") {
            self.dragging.set(false);
        }
        if self.dragging.get() {
            let (cursor_x, cursor_y) = f.get_cursor_pos();
            if self.dir == "HORIZONTAL" {
                self.set_offset_from_knob_pos((cursor_x - self.drag_cx.get()) + self.drag_knob_pos.get());
            } else {
                self.set_offset_from_knob_pos((cursor_y - self.drag_cy.get()) + self.drag_knob_pos.get());
            }
        } else if let Some(hold_comp) = self.hold_comp.get() {
            if m_over_comp == Some(hold_comp) {
                let now = f.get_time() as f64;
                if !self.hold_repeating.get() && now - self.hold_time.get() > 500.0 {
                    self.hold_repeating.set(true);
                    self.hold_time.set(now - 1.0);
                }
                if self.hold_repeating.get() {
                    if let Some(p) = self.hold_pause_time.take() {
                        self.hold_time.set(self.hold_time.get() + (now - p));
                    }
                    let time = now - self.hold_time.get();
                    if hold_comp == "UP" {
                        self.set_offset(self.hold_base.get() - (time / 50.0).ceil() * self.step.get());
                    } else if hold_comp == "DOWN" {
                        self.set_offset(self.hold_base.get() + (time / 50.0).ceil() * self.step.get());
                    }
                }
            } else if self.hold_repeating.get() && self.hold_pause_time.get().is_none() {
                self.hold_pause_time.set(Some(f.get_time() as f64));
            }
        }
        let horiz = dir == "HORIZONTAL";
        // up/left button
        if !enabled {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else if m_over_comp == Some("UP") {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        if horiz {
            sg::draw_rect(f, x, y, height, height);
        } else {
            sg::draw_rect(f, x, y, width, width);
        }
        if enabled && m_over_comp == Some("UP") {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else {
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
        }
        if horiz {
            sg::draw_rect(f, x + 1.0, y + 1.0, height - 2.0, height - 2.0);
        } else {
            sg::draw_rect(f, x + 1.0, y + 1.0, width - 2.0, width - 2.0);
        }
        if !enabled {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else if m_over_comp == Some("UP") {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        if horiz {
            draw_arrow(f, x + height / 2.0, y + height / 2.0, height / 2.0, height / 2.0, "LEFT");
        } else {
            draw_arrow(f, x + width / 2.0, y + width / 2.0, width / 2.0, width / 2.0, "UP");
        }
        // down/right button
        if !enabled {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else if m_over_comp == Some("DOWN") {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        if horiz {
            sg::draw_rect(f, x + width - height, y, height, height);
        } else {
            sg::draw_rect(f, x, y + height - width, width, width);
        }
        if enabled && m_over_comp == Some("DOWN") {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else {
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
        }
        if horiz {
            sg::draw_rect(f, x + width - height + 1.0, y + 1.0, height - 2.0, height - 2.0);
        } else {
            sg::draw_rect(f, x + 1.0, y + height - width + 1.0, width - 2.0, width - 2.0);
        }
        if !enabled {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else if m_over_comp == Some("DOWN") {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        if horiz {
            draw_arrow(f, x + width - height / 2.0, y + height / 2.0, height / 2.0, height / 2.0, "RIGHT");
        } else {
            draw_arrow(f, x + width / 2.0, y + height - width / 2.0, width / 2.0, width / 2.0, "DOWN");
        }
        // slide background
        if !enabled {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else if self.dragging.get() || matches!(m_over_comp, Some("KNOB" | "SLIDEUP" | "SLIDEDOWN")) {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        if horiz {
            sg::draw_rect(f, x + height, y, width - height * 2.0, height);
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
            sg::draw_rect(f, x + height, y + 1.0, width - height * 2.0, height - 2.0);
        } else {
            sg::draw_rect(f, x, y + width, width, height - width * 2.0);
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
            sg::draw_rect(f, x + 1.0, y + width, width - 2.0, height - width * 2.0);
        }
        // knob
        if enabled {
            if self.dragging.get() || m_over_comp == Some("KNOB") {
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
            } else {
                sg::set_draw_color(f, 0.5, 0.5, 0.5);
            }
            let knob_pos = self.get_knob_pos_for_offset();
            let knob_dim = self.knob_dim.get().expect("knobDim");
            if horiz {
                sg::draw_rect(f, x + height + knob_pos + 1.0, y + 2.0, knob_dim - 2.0, height - 4.0);
            } else {
                sg::draw_rect(f, x + 2.0, y + width + knob_pos + 1.0, width - 4.0, knob_dim - 2.0);
            }
        }
    }

    fn has_on_key_down(&self) -> bool {
        true
    }

    /// ScrollBarControl.lua:259-293
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, _double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) || self.locked.get(g) {
            return None;
        }
        if key == "LEFTBUTTON" {
            let (m_over, comp) = self.mouse_over(g);
            if !m_over {
                return None;
            }
            match comp {
                Some("KNOB") => {
                    if !self.dragging.get() {
                        self.dragging.set(true);
                        let (cursor_x, cursor_y) = g.get_cursor_pos();
                        self.drag_cx.set(cursor_x);
                        self.drag_cy.set(cursor_y);
                        self.drag_knob_pos.set(self.get_knob_pos_for_offset());
                    }
                }
                Some(c @ ("UP" | "DOWN")) => {
                    self.scroll(if c == "UP" { -1.0 } else { 1.0 });
                    self.hold_comp.set(Some(c));
                    self.hold_time.set(g.get_time() as f64);
                    self.hold_base.set(self.offset.get());
                }
                Some("SLIDEUP") => self.set_offset_from_knob_pos(self.get_knob_pos_for_offset() - self.knob_dim.get().expect("knobDim")),
                Some("SLIDEDOWN") => self.set_offset_from_knob_pos(self.get_knob_pos_for_offset() + self.knob_dim.get().expect("knobDim")),
                _ => {}
            }
        }
        Some(self)
    }

    fn has_on_key_up(&self) -> bool {
        true
    }

    /// ScrollBarControl.lua:295-319
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) || self.locked.get(g) {
            return None;
        }
        if key == "LEFTBUTTON" {
            if self.dragging.get() {
                self.dragging.set(false);
                let (cursor_x, cursor_y) = g.get_cursor_pos();
                if self.dir == "HORIZONTAL" {
                    self.set_offset_from_knob_pos((cursor_x - self.drag_cx.get()) + self.drag_knob_pos.get());
                } else {
                    self.set_offset_from_knob_pos((cursor_y - self.drag_cy.get()) + self.drag_knob_pos.get());
                }
            } else if self.hold_comp.get().is_some() {
                self.hold_comp.set(None);
                self.hold_repeating.set(false);
                self.hold_pause_time.set(None);
            }
        } else if Self::is_scroll_down_key(key) {
            self.scroll(1.0);
        } else if Self::is_scroll_up_key(key) {
            self.scroll(-1.0);
        }
        None
    }
}

// ==================================================================================================== SliderControl

/// `SliderControl` (SliderControl.lua): `Control` + `TooltipHost`; `tooltipFunc` gets the value under the cursor.
pub struct SliderControl {
    pub base: ControlBase,
    pub tip: TooltipHost<f64>,
    pub knob_size: Cell<f64>,
    pub val: Cell<f64>,
    pub change_func: RefCell<Option<Rc<dyn Fn(&mut Gfx, f64)>>>,
    /// `{ SHIFT, CTRL, DEFAULT }`.
    pub scroll_wheel_speed: Cell<[f64; 3]>,
    pub div_count: Cell<Option<f64>>,
    pub dragging: Cell<bool>,
    pub drag_cx: Cell<f64>,
    pub drag_knob_x: Cell<f64>,
}

impl SliderControl {
    /// `SliderControl:SliderControl(anchor, rect, changeFunc, scrollWheelSpeedTbl)` (SliderControl.lua:13-21).
    pub fn new(
        anchor: Option<Anchor>,
        rect: RectProps,
        change_func: Option<Rc<dyn Fn(&mut Gfx, f64)>>,
        scroll_wheel_speed: Option<[f64; 3]>,
    ) -> Rc<Self> {
        let base = ControlBase::new(anchor, rect);
        let knob = base.height.raw().expect("SliderControl height must be a number") - 2.0;
        Rc::new(Self {
            base,
            tip: TooltipHost::new(None),
            knob_size: Cell::new(knob),
            val: Cell::new(0.0),
            change_func: RefCell::new(change_func),
            scroll_wheel_speed: Cell::new(scroll_wheel_speed.unwrap_or([0.25, 0.01, 0.05])),
            div_count: Cell::new(None),
            dragging: Cell::new(false),
            drag_cx: Cell::new(0.0),
            drag_knob_x: Cell::new(0.0),
        })
    }

    /// SliderControl.lua:23-42
    pub fn mouse_over(&self, g: &Gfx) -> (bool, Option<&'static str>) {
        if !self.is_shown(g) {
            return (false, None);
        }
        let (x, y) = self.get_pos(g);
        let (width, height) = self.get_size(g);
        let (cursor_x, cursor_y) = g.get_cursor_pos();
        let m_over = cursor_x >= x && cursor_y >= y && cursor_x < x + width && cursor_y < y + height;
        let mut comp = None;
        if m_over {
            let rel_x = cursor_x - x - 2.0;
            let knob_x = self.get_knob_x_for_val(g);
            comp = Some(if rel_x >= knob_x && rel_x < knob_x + self.knob_size.get() {
                "KNOB"
            } else {
                "SLIDE"
            });
        }
        (m_over, comp)
    }

    /// SliderControl.lua:44-47
    pub fn get_knob_travel(&self, g: &Gfx) -> f64 {
        self.get_size(g).0 - self.knob_size.get() - 2.0
    }

    /// SliderControl.lua:49-57
    pub fn set_val(&self, g: &mut Gfx, new_val: f64) {
        let new_val = 0f64.max(1f64.min(new_val));
        if new_val != self.val.get() {
            self.val.set(new_val);
            let cf = self.change_func.borrow().clone();
            if let Some(cf) = cf {
                cf(g, new_val);
            }
        }
    }

    /// SliderControl.lua:59-67
    pub fn get_div_val(&self, val: Option<f64>) -> (f64, f64) {
        let val = val.unwrap_or(self.val.get());
        match self.div_count.get() {
            Some(n) if n > 1.0 => {
                let div_index = (val * n).ceil().max(1.0);
                (div_index, val * n - div_index + 1.0)
            }
            _ => (1.0, val),
        }
    }

    /// SliderControl.lua:69-71
    pub fn set_val_from_knob_x(&self, g: &mut Gfx, knob_x: f64) {
        let travel = self.get_knob_travel(g);
        self.set_val(g, knob_x / travel);
    }

    /// SliderControl.lua:73-76
    pub fn get_knob_x_for_val(&self, g: &Gfx) -> f64 {
        self.get_knob_travel(g) * self.val.get()
    }
}

impl Control for SliderControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }

    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        Some(self.mouse_over(g).0)
    }

    fn has_draw(&self) -> bool {
        true
    }

    /// SliderControl.lua:78-133
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, _no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let (width, height) = self.get_size(f);
        let enabled = self.is_enabled(f);
        let knob_travel = self.get_knob_travel(f);
        if self.dragging.get() && !sg::is_key_down(f, "LEFTBUTTON") {
            self.dragging.set(false);
        }
        if self.dragging.get() {
            let (cursor_x, _) = f.get_cursor_pos();
            self.set_val_from_knob_x(f, (cursor_x - self.drag_cx.get()) + self.drag_knob_x.get());
        }
        let (m_over, m_over_comp) = self.mouse_over(f);
        if !enabled {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else if self.dragging.get() || m_over {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        sg::draw_rect(f, x, y, width, height);
        sg::set_draw_color(f, 0.0, 0.0, 0.0);
        sg::draw_rect(f, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        let knob_size = self.knob_size.get();
        if enabled {
            if let Some(div_count) = self.div_count.get() {
                sg::set_draw_color(f, 0.33, 0.33, 0.33);
                // `for d = 0, knobTravel + 0.5, knobTravel / self.divCount` (LuaJIT: the variable accumulates the step)
                let (limit, step) = (knob_travel + 0.5, knob_travel / div_count);
                let mut d = 0.0;
                while if step > 0.0 { d <= limit } else { d >= limit } {
                    sg::draw_rect(f, x + knob_size / 2.0 + d, y + 1.0, 2.0, height - 2.0);
                    d += step;
                }
            }
            if self.dragging.get() || m_over_comp == Some("KNOB") {
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
            } else {
                sg::set_draw_color(f, 0.5, 0.5, 0.5);
            }
            let knob_x = self.get_knob_x_for_val(f);
            if self.div_count.get().is_some() {
                let arrow_height = knob_size / 2.0;
                draw_arrow(
                    f,
                    x + 1.0 + knob_x + knob_size / 2.0,
                    y + height / 2.0 - arrow_height / 2.0,
                    knob_size,
                    arrow_height,
                    "UP",
                );
                draw_arrow(
                    f,
                    x + 1.0 + knob_x + knob_size / 2.0,
                    y + height / 2.0 + arrow_height / 2.0,
                    knob_size,
                    arrow_height,
                    "DOWN",
                );
            } else {
                sg::draw_rect(f, x + 2.0 + knob_x, y + 2.0, knob_size - 2.0, knob_size - 2.0);
            }
        }
        if enabled && (m_over || self.dragging.get()) {
            sg::set_draw_layer(f, None, 100);
            if self.dragging.get() {
                self.tip.draw_tooltip(f, x, y, width, height, view_port, self.val.get());
            } else {
                let (cursor_x, _) = f.get_cursor_pos();
                let val = (cursor_x - x - 1.0 - knob_size / 2.0) / knob_travel;
                self.tip.draw_tooltip(f, x, y, width, height, view_port, 0f64.max(1f64.min(val)));
            }
            sg::set_draw_layer(f, None, 0);
        }
    }

    fn has_on_key_down(&self) -> bool {
        true
    }

    /// SliderControl.lua:135-156
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, _double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if key == "LEFTBUTTON" {
            let (m_over, comp) = self.mouse_over(g);
            if !m_over {
                return None;
            }
            if !self.dragging.get() {
                self.dragging.set(true);
                let (cursor_x, _) = g.get_cursor_pos();
                self.drag_cx.set(cursor_x);
                if comp == Some("SLIDE") {
                    let (x, _) = self.get_pos(g);
                    self.set_val_from_knob_x(g, cursor_x - x - 1.0 - self.knob_size.get() / 2.0);
                }
                self.drag_knob_x.set(self.get_knob_x_for_val(g));
            }
        }
        Some(self)
    }

    fn has_on_key_up(&self) -> bool {
        true
    }

    /// SliderControl.lua:158-185
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        let invert = invert_slider_scroll_direction();
        let [shift, ctrl, default] = self.scroll_wheel_speed.get();
        let speed = |g: &Gfx| {
            if sg::is_key_down(g, "SHIFT") {
                shift
            } else if sg::is_key_down(g, "CTRL") {
                ctrl
            } else {
                default
            }
        };
        if key == "LEFTBUTTON" {
            if self.dragging.get() {
                self.dragging.set(false);
                let (cursor_x, _) = g.get_cursor_pos();
                self.set_val_from_knob_x(g, (cursor_x - self.drag_cx.get()) + self.drag_knob_x.get());
            }
        } else if (!invert && key == "WHEELDOWN") || (invert && key == "WHEELUP") || key == "DOWN" || key == "LEFT" {
            self.set_val(g, self.val.get() - speed(g));
        } else if (!invert && key == "WHEELUP") || (invert && key == "WHEELDOWN") || key == "UP" || key == "RIGHT" {
            self.set_val(g, self.val.get() + speed(g));
        }
        None
    }
}
