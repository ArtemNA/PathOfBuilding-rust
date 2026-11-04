//! `LabelControl.lua`, `ButtonControl.lua`, `CheckBoxControl.lua`, `RectangleOutlineControl.lua`, `SectionControl.lua`.

use crate::common::draw_check_mark;
use crate::control::{Anchor, Control, ControlBase, ControlRef, PropCell, Rect, RectProps};
use crate::sg;
use crate::tooltip::TooltipHost;
use pob_gfx::{Frame, Gfx, ImageHandle};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

pub type Callback = Rc<dyn Fn(&mut Gfx)>;

// ===================================================================================================== LabelControl

/// `LabelControl` (LabelControl.lua).
pub struct LabelControl {
    pub base: ControlBase,
    pub label: PropCell<String>,
}

impl LabelControl {
    /// `LabelControl:LabelControl(anchor, rect, label)` (LabelControl.lua:11-18).
    pub fn new(anchor: Option<Anchor>, rect: RectProps, label: impl Into<crate::control::Prop<String>>) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| {
            let base = ControlBase::new(anchor, rect);
            let me = me.clone();
            base.width.set_fn(move |g| {
                let s = me.upgrade().expect("label");
                sg::draw_string_width(g, s.base.height.get(g), "VAR", &s.label.get(g))
            });
            Self {
                base,
                label: PropCell::new(label),
            }
        })
    }
}

impl Control for LabelControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn debug_value(&self, g: &Gfx) -> Option<serde_json::Value> {
        Some(serde_json::json!(self.label.get(g)))
    }
    fn has_draw(&self) -> bool {
        true
    }
    /// LabelControl.lua:20-23
    fn draw(&self, f: &mut Frame<'_>, _vp: &Rect, _no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let h = self.base.height.get(f);
        let label = self.label.get(f);
        sg::draw_string(f, x, y, Some("LEFT"), h, "VAR", &label);
    }
}

// ==================================================================================================== ButtonControl

/// `ButtonControl` (ButtonControl.lua): `Control` + `TooltipHost`.
pub struct ButtonControl {
    pub base: ControlBase,
    pub tip: TooltipHost<()>,
    pub label: PropCell<String>,
    pub on_click: RefCell<Callback>,
    pub on_hover: RefCell<Option<Rc<dyn Fn(&Gfx)>>>,
    /// An `onHover` that draws (TradeQuery.lua:1129-1140).
    pub on_hover_frame: RefCell<Option<Rc<dyn Fn(&mut Frame<'_>)>>>,
    pub force_tooltip: Cell<bool>,
    pub clicked: Cell<bool>,
    pub image: RefCell<Option<ImageHandle>>,
    /// `GetProperty("locked")` (`nil` = false).
    pub locked: PropCell<bool>,
    pub enter_func: RefCell<Option<Callback>>,
    pub over_size_text: Cell<Option<f64>>,
}

impl ButtonControl {
    /// `ButtonControl:ButtonControl(anchor, rect, label, onClick, onHover, forceTooltip)` (ButtonControl.lua:9-17).
    pub fn new(
        anchor: Option<Anchor>,
        rect: RectProps,
        label: impl Into<crate::control::Prop<String>>,
        on_click: impl Fn(&mut Gfx) + 'static,
    ) -> Rc<Self> {
        Rc::new(Self {
            base: ControlBase::new(anchor, rect),
            tip: TooltipHost::new(None),
            label: PropCell::new(label),
            on_click: RefCell::new(Rc::new(on_click)),
            on_hover: RefCell::new(None),
            on_hover_frame: RefCell::new(None),
            force_tooltip: Cell::new(false),
            clicked: Cell::new(false),
            image: RefCell::new(None),
            locked: PropCell::new(false),
            enter_func: RefCell::new(None),
            over_size_text: Cell::new(None),
        })
    }

    fn call_on_click(&self, g: &mut Gfx) {
        let f = self.on_click.borrow().clone();
        f(g);
    }

    /// `ButtonControl:Click()` (ButtonControl.lua:19-23).
    pub fn click(&self, g: &mut Gfx) {
        if self.is_shown(g) && self.is_enabled(g) {
            self.call_on_click(g);
        }
    }

    /// `ButtonControl:SetImage(path)` (ButtonControl.lua:25-32): a failed load is a Lua error in the original.
    pub fn set_image(&self, path: Option<&str>) {
        *self.image.borrow_mut() = path.map(|p| {
            let h = ImageHandle::new();
            h.load_lua(p, &[]).unwrap_or_else(|e| panic!("{e}"));
            h
        });
    }

    fn mouse_over(&self, g: &Gfx) -> bool {
        self.is_shown(g) && self.is_mouse_in_bounds(g)
    }
}

impl Control for ButtonControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn debug_value(&self, g: &Gfx) -> Option<serde_json::Value> {
        Some(serde_json::json!(self.label.get(g)))
    }

    fn click(&self, g: &mut Gfx) {
        ButtonControl::click(self, g);
    }

    /// ButtonControl.lua:34-39
    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        Some(self.mouse_over(g))
    }

    fn has_draw(&self) -> bool {
        true
    }

    /// ButtonControl.lua:41-104
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
        if enabled {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        }
        let label = self.label.get(f);
        match label.as_str() {
            "+" => {
                sg::draw_rect(f, x + width * 0.2, y + height * 0.45, width * 0.6, height * 0.1);
                sg::draw_rect(f, x + width * 0.45, y + height * 0.2, width * 0.1, height * 0.6);
            }
            "-" => sg::draw_rect(f, x + width * 0.2, y + height * 0.45, width * 0.6, height * 0.1),
            "x" => {
                sg::draw_quad(
                    f,
                    [
                        x + width * 0.2,
                        y + height * 0.3,
                        x + width * 0.3,
                        y + height * 0.2,
                        x + width * 0.8,
                        y + height * 0.7,
                        x + width * 0.7,
                        y + height * 0.8,
                    ],
                );
                sg::draw_quad(
                    f,
                    [
                        x + width * 0.7,
                        y + height * 0.2,
                        x + width * 0.8,
                        y + height * 0.3,
                        x + width * 0.3,
                        y + height * 0.8,
                        x + width * 0.2,
                        y + height * 0.7,
                    ],
                );
            }
            _ => {
                let over_size = self.over_size_text.get().unwrap_or(0.0);
                sg::draw_string(
                    f,
                    x + width / 2.0,
                    y + 2.0 - over_size,
                    Some("CENTER_X"),
                    height - 4.0 + over_size * 2.0,
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
            let on_hover = self.on_hover_frame.borrow().clone();
            if let Some(h) = on_hover {
                h(f);
            }
        }
    }

    fn has_on_key_down(&self) -> bool {
        true
    }

    /// ButtonControl.lua:106-116
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, _double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if key == "LEFTBUTTON" {
            self.clicked.set(true);
        } else {
            let enter = self.enter_func.borrow().clone();
            if let Some(e) = enter {
                e(g);
            }
        }
        Some(self)
    }

    fn has_on_key_up(&self) -> bool {
        true
    }

    /// ButtonControl.lua:118-129 (`return self.onClick()`: the callbacks of the port return nothing → `nil`).
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if key == "LEFTBUTTON" && self.clicked.get() {
            self.clicked.set(false);
            if self.mouse_over(g) {
                self.call_on_click(g);
                return None;
            }
        }
        self.clicked.set(false);
        None
    }
}

// ================================================================================================== CheckBoxControl

/// `CheckBoxControl` (CheckBoxControl.lua): `Control` + `TooltipHost`; `tooltipFunc` gets `self.state`.
pub struct CheckBoxControl {
    pub base: ControlBase,
    pub tip: TooltipHost<bool>,
    pub label: PropCell<Option<String>>,
    pub label_width: Cell<f64>,
    pub label_right: Cell<bool>,
    #[allow(clippy::type_complexity)]
    pub change_func: RefCell<Option<Rc<dyn Fn(&mut Gfx, bool)>>>,
    pub border_func: RefCell<Option<Rc<dyn Fn() -> (f64, f64, f64)>>>,
    /// `box.RealDraw = box.Draw; function box:Draw(...)`: what a caller draws first, at the control's position.
    #[allow(clippy::type_complexity)]
    pub before_draw: RefCell<Option<Rc<dyn Fn(&mut Frame<'_>, f64, f64)>>>,
    pub state: Cell<bool>,
    pub clicked: Cell<bool>,
}

impl CheckBoxControl {
    /// `CheckBoxControl:CheckBoxControl(anchor, rect, label, changeFunc, tooltipText, initialState)`
    /// (CheckBoxControl.lua:9-19): `rect = {x, y, size}`; the height is set to the width.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        g: &Gfx,
        anchor: Option<Anchor>,
        rect: [f64; 3],
        label: Option<&str>,
        change_func: Option<Rc<dyn Fn(&mut Gfx, bool)>>,
        tooltip_text: Option<&str>,
        initial_state: bool,
    ) -> Rc<Self> {
        let size = rect[2];
        let base = ControlBase::new(anchor, [rect[0].into(), rect[1].into(), size.into(), size.into()]);
        Rc::new(Self {
            base,
            tip: TooltipHost::new(tooltip_text.map(str::to_string)),
            label: PropCell::new(label.map(str::to_string)),
            label_width: Cell::new(sg::draw_string_width(g, size - 4.0, "VAR", label.unwrap_or("")) + 5.0),
            label_right: Cell::new(false),
            change_func: RefCell::new(change_func),
            border_func: RefCell::new(None),
            before_draw: RefCell::new(None),
            state: Cell::new(initial_state),
            clicked: Cell::new(false),
        })
    }

    /// CheckBoxControl.lua:21-36
    fn mouse_over(&self, g: &Gfx) -> bool {
        if !self.is_shown(g) {
            return false;
        }
        let (mut x, y) = self.get_pos(g);
        let (mut width, height) = self.get_size(g);
        let (cursor_x, cursor_y) = g.get_cursor_pos();
        if self.label.get(g).is_some() {
            if !self.label_right.get() {
                x -= self.label_width.get();
            }
            width += self.label_width.get();
        }
        cursor_x >= x && cursor_y >= y && cursor_x < x + width && cursor_y < y + height
    }
}

impl Control for CheckBoxControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn debug_value(&self, _g: &Gfx) -> Option<serde_json::Value> {
        Some(serde_json::json!(self.state.get()))
    }

    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        Some(self.mouse_over(g))
    }

    fn has_draw(&self) -> bool {
        true
    }

    /// CheckBoxControl.lua:38-88
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let before = self.before_draw.borrow().clone();
        if let Some(b) = before {
            b(f, x, y);
        }
        let size = self.base.width.get(f);
        let enabled = self.is_enabled(f);
        let m_over = self.mouse_over(f);
        let border = self.border_func.borrow().clone();
        if !enabled {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else if m_over {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else if let Some(bf) = border {
            let (r, g, b) = bf();
            sg::set_draw_color(f, r, g, b);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        sg::draw_rect(f, x, y, size, size);
        if !enabled {
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
        } else if self.clicked.get() && m_over {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        } else if m_over {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else {
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
        }
        sg::draw_rect(f, x + 1.0, y + 1.0, size - 2.0, size - 2.0);
        if self.state.get() {
            if !enabled {
                sg::set_draw_color(f, 0.33, 0.33, 0.33);
            } else if m_over {
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
            } else {
                sg::set_draw_color(f, 0.75, 0.75, 0.75);
            }
            draw_check_mark(f, x + size / 2.0, y + size / 2.0, size * 0.8);
        }
        if enabled {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        }
        let label = self.label.get(f);
        if let Some(label) = &label {
            if self.label_right.get() {
                let w = self.base.width.get(f);
                sg::draw_string(f, x + w + 5.0, y + 2.0, None, size - 4.0, "VAR", label);
            } else {
                sg::draw_string(f, x - 5.0, y + 2.0, Some("RIGHT_X"), size - 4.0, "VAR", label);
            }
        }
        if m_over && !no_tooltip {
            sg::set_draw_layer(f, None, 100);
            self.tip.draw_tooltip(f, x, y, size, size, view_port, self.state.get());
            sg::set_draw_layer(f, None, 0);
        }
    }

    fn has_on_key_down(&self) -> bool {
        true
    }

    /// CheckBoxControl.lua:90-98
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, _double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if key == "LEFTBUTTON" {
            self.clicked.set(true);
        }
        Some(self)
    }

    fn has_on_key_up(&self) -> bool {
        true
    }

    /// CheckBoxControl.lua:100-116
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if key == "LEFTBUTTON" && self.mouse_over(g) {
            self.state.set(!self.state.get());
            let cf = self.change_func.borrow().clone();
            if let Some(cf) = cf {
                cf(g, self.state.get());
            }
        }
        self.clicked.set(false);
        None
    }
}

// ========================================================================================= RectangleOutlineControl

/// `RectangleOutlineControl` (RectangleOutlineControl.lua).
pub struct RectangleOutlineControl {
    pub base: ControlBase,
    pub stroke: Cell<f64>,
    pub colors: Cell<[f64; 3]>,
}

impl RectangleOutlineControl {
    /// RectangleOutlineControl.lua:9-14
    pub fn new(anchor: Option<Anchor>, rect: RectProps, colors: Option<[f64; 3]>, stroke: Option<f64>) -> Rc<Self> {
        Rc::new(Self {
            base: ControlBase::new(anchor, rect),
            stroke: Cell::new(stroke.unwrap_or(1.0)),
            colors: Cell::new(colors.unwrap_or([1.0, 1.0, 1.0])),
        })
    }
}

impl Control for RectangleOutlineControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn has_draw(&self) -> bool {
        true
    }
    /// RectangleOutlineControl.lua:16-23 (`self.width`/`self.height` read raw: numbers).
    fn draw(&self, f: &mut Frame<'_>, _vp: &Rect, _no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let [r, g, b] = self.colors.get();
        let s = self.stroke.get();
        let w = self.base.width.raw().expect("RectangleOutlineControl width must be a number");
        let h = self.base.height.raw().expect("RectangleOutlineControl height must be a number");
        sg::set_draw_color(f, r, g, b);
        sg::draw_rect(f, x, y, w + s, s);
        sg::draw_rect(f, x, y + h, w + s, s);
        sg::draw_rect(f, x, y, s, h + s);
        sg::draw_rect(f, x + w, y, s, h + s);
    }
}

// =================================================================================================== SectionControl

/// `SectionControl` (SectionControl.lua).
pub struct SectionControl {
    pub base: ControlBase,
    pub label: PropCell<String>,
}

impl SectionControl {
    pub fn new(anchor: Option<Anchor>, rect: RectProps, label: impl Into<crate::control::Prop<String>>) -> Rc<Self> {
        Rc::new(Self {
            base: ControlBase::new(anchor, rect),
            label: PropCell::new(label),
        })
    }
}

impl Control for SectionControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn debug_value(&self, g: &Gfx) -> Option<serde_json::Value> {
        Some(serde_json::json!(self.label.get(g)))
    }
    fn has_draw(&self) -> bool {
        true
    }
    /// SectionControl.lua:15-33
    fn draw(&self, f: &mut Frame<'_>, _vp: &Rect, _no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let (width, height) = self.get_size(f);
        sg::set_draw_layer(f, None, -10);
        sg::set_draw_color(f, 0.66, 0.66, 0.66);
        sg::draw_rect(f, x, y, width, height);
        sg::set_draw_color(f, 0.1, 0.1, 0.1);
        sg::draw_rect(f, x + 2.0, y + 2.0, width - 4.0, height - 4.0);
        sg::set_draw_layer(f, None, 0);
        let label = self.label.get(f);
        let label_width = sg::draw_string_width(f, 14.0, "VAR", &label);
        sg::set_draw_color(f, 0.66, 0.66, 0.66);
        sg::draw_rect(f, x + 6.0, y - 8.0, label_width + 6.0, 18.0);
        sg::set_draw_color(f, 0.0, 0.0, 0.0);
        sg::draw_rect(f, x + 7.0, y - 7.0, label_width + 4.0, 16.0);
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_string(f, x + 9.0, y - 6.0, Some("LEFT"), 14.0, "VAR", &label);
    }
}
