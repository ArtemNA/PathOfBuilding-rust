//! `Classes/Control.lua` — the control base class — and the class model of the port (UI-ARCH §4.2, decided by the
//! U2a.0 spike):
//!
//! - a control is an `Rc<dyn Control>`; methods take `&self` (Lua's `self`), or `self: Rc<Self>` where Lua does
//!   `return self` (the new `selControl`);
//! - every field that Lua code assigns after construction is a `Cell`/`RefCell`/[`PropCell`] (per-field interior
//!   mutability), borrowed for no longer than one expression, so callbacks and property functions may read and write any
//!   control — including the one whose method is running — without borrow panics (MINEFIELD U13);
//! - Lua's multiple inheritance (`newClass("EditControl", "ControlHost", "Control", "UndoHandler", "TooltipHost")`) is
//!   composition: [`ControlBase`], [`crate::ControlHost`], [`crate::UndoHandler`], [`crate::TooltipHost`] fields plus the
//!   [`Control`] trait whose default methods are the base-class methods;
//! - `Prop<T>` ("value or function", Control.lua:68-79) is [`PropCell`]; a property function gets the host ([`Gfx`]) as
//!   the globals it may call (`DrawStringWidth`, `GetCursorPos`, …) and captures what else it needs (weak references to
//!   controls, like Lua upvalues);
//! - `anchor.other` is a `Weak` (children anchored to the parent under construction use `Rc::new_cyclic`).

use pob_gfx::{Frame, Gfx};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

pub type ControlRef = Rc<dyn Control>;
pub type WeakControl = Weak<dyn Control>;

/// `{ x, y, width, height }` tables (viewPort, regions).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }
}

/// Identity comparison of controls (`control == other` in Lua).
pub fn same(a: &dyn Control, b: &dyn Control) -> bool {
    std::ptr::addr_eq(a as *const dyn Control, b as *const dyn Control)
}

pub fn same_opt(a: Option<&ControlRef>, b: &dyn Control) -> bool {
    a.is_some_and(|a| same(&**a, b))
}

/// `Prop<T>`: a value or a function (Control.lua:68-79).
pub enum Prop<T> {
    Val(T),
    Fn(Rc<dyn Fn(&Gfx) -> T>),
}

impl<T: Clone> Clone for Prop<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Val(v) => Self::Val(v.clone()),
            Self::Fn(f) => Self::Fn(Rc::clone(f)),
        }
    }
}

impl<T> From<T> for Prop<T> {
    fn from(v: T) -> Self {
        Self::Val(v)
    }
}

impl<T> Prop<T> {
    pub fn func(f: impl Fn(&Gfx) -> T + 'static) -> Self {
        Self::Fn(Rc::new(f))
    }
}

/// A `Prop<T>` field. `get` is `GetProperty(name)`: the function is cloned out of the cell before it is called, so it may
/// re-enter the control and even reassign this very field.
pub struct PropCell<T>(RefCell<Prop<T>>);

impl<T: Clone> PropCell<T> {
    pub fn new(p: impl Into<Prop<T>>) -> Self {
        Self(RefCell::new(p.into()))
    }

    pub fn get(&self, g: &Gfx) -> T {
        let p = self.0.borrow().clone();
        match p {
            Prop::Val(v) => v,
            Prop::Fn(f) => f(g),
        }
    }

    /// `self.name = value`.
    pub fn set(&self, p: impl Into<Prop<T>>) {
        *self.0.borrow_mut() = p.into();
    }

    /// `self.name = function() … end`.
    pub fn set_fn(&self, f: impl Fn(&Gfx) -> T + 'static) {
        *self.0.borrow_mut() = Prop::func(f);
    }

    /// `type(self.name) == "function"`.
    pub fn is_fn(&self) -> bool {
        matches!(*self.0.borrow(), Prop::Fn(_))
    }

    /// The raw value when it is not a function (`self.width` read directly, e.g. CheckBoxControl.lua:11).
    pub fn raw(&self) -> Option<T> {
        match &*self.0.borrow() {
            Prop::Val(v) => Some(v.clone()),
            Prop::Fn(_) => None,
        }
    }
}

/// Anchor points (Control.lua:11-21).
pub fn anchor_pos(point: &str) -> Option<(f64, f64)> {
    Some(match point {
        "TOPLEFT" => (0.0, 0.0),
        "TOP" => (0.5, 0.0),
        "TOPRIGHT" => (1.0, 0.0),
        "RIGHT" => (1.0, 0.5),
        "BOTTOMRIGHT" => (1.0, 1.0),
        "BOTTOM" => (0.5, 1.0),
        "BOTTOMLEFT" => (0.0, 1.0),
        "LEFT" => (0.0, 0.5),
        "CENTER" => (0.5, 0.5),
        _ => return None,
    })
}

/// `self.anchor` (Control.lua:89-98).
#[derive(Default)]
pub struct AnchorState {
    pub point: Option<&'static str>,
    pub other: Option<WeakControl>,
    pub other_point: Option<&'static str>,
    pub collapse: bool,
}

/// `{ point, other, otherPoint, collapse }` — the constructor's `anchor` argument.
pub struct Anchor {
    pub point: &'static str,
    pub other: WeakControl,
    pub other_point: &'static str,
    pub collapse: bool,
}

/// `anchor(point, other, otherPoint)`.
pub fn anchor(point: &'static str, other: &ControlRef, other_point: &'static str) -> Option<Anchor> {
    Some(Anchor {
        point,
        other: Rc::downgrade(other),
        other_point,
        collapse: false,
    })
}

/// Same, from a weak reference (a parent under construction in `Rc::new_cyclic`).
pub fn anchor_weak(point: &'static str, other: WeakControl, other_point: &'static str) -> Option<Anchor> {
    Some(Anchor {
        point,
        other,
        other_point,
        collapse: false,
    })
}

/// The constructor's `rect` argument: `{x, y, width, height}`, each a `Prop<number>`.
pub type RectProps = [Prop<f64>; 4];

pub fn rect(x: f64, y: f64, w: f64, h: f64) -> RectProps {
    [x.into(), y.into(), w.into(), h.into()]
}

/// Fields of `Control` (Control.lua:57-69, :89-98, :144-153).
pub struct ControlBase {
    pub x: PropCell<f64>,
    pub y: PropCell<f64>,
    pub width: PropCell<f64>,
    pub height: PropCell<f64>,
    pub shown: PropCell<bool>,
    pub enabled: PropCell<bool>,
    pub anchor: RefCell<AnchorState>,
    pub collapse_x: Cell<Option<f64>>,
    pub collapse_y: Cell<Option<f64>>,
    pub has_focus: Cell<bool>,
    /// `self.tabOrder`: shared list of the tab group (Control.lua:144-152).
    pub tab_order: RefCell<Option<Rc<RefCell<Vec<WeakControl>>>>>,
}

impl ControlBase {
    /// `Control:Control(anchor, rect)` (Control.lua:57-67).
    pub fn new(anchor: Option<Anchor>, rect: RectProps) -> Self {
        let [x, y, width, height] = rect;
        let b = Self {
            x: PropCell::new(x),
            y: PropCell::new(y),
            width: PropCell::new(width),
            height: PropCell::new(height),
            shown: PropCell::new(true),
            enabled: PropCell::new(true),
            anchor: RefCell::new(AnchorState::default()),
            collapse_x: Cell::new(None),
            collapse_y: Cell::new(None),
            has_focus: Cell::new(false),
            tab_order: RefCell::new(None),
        };
        if let Some(a) = anchor {
            b.set_anchor(a.point, Some(a.other), a.other_point, None, a.collapse);
        }
        b
    }

    /// `Control:SetAnchor(point, other, otherPoint, x, y, collapse)` (Control.lua:89-98).
    pub fn set_anchor(
        &self,
        point: &'static str,
        other: Option<WeakControl>,
        other_point: &'static str,
        xy: Option<(f64, f64)>,
        collapse: bool,
    ) {
        *self.anchor.borrow_mut() = AnchorState {
            point: Some(point),
            other,
            other_point: Some(other_point),
            collapse,
        };
        if let Some((x, y)) = xy {
            self.x.set(x);
            self.y.set(y);
        }
    }

    fn anchor_other(&self) -> Option<ControlRef> {
        self.anchor
            .borrow()
            .other
            .as_ref()
            .map(|w| w.upgrade().expect("anchor target dropped"))
    }
}

/// The `Control` class and the optional methods its subclasses may define (`control.Draw`, `control.OnKeyDown`, …).
/// `has_*` answer Lua's `if control.OnKeyDown then`.
pub trait Control: Any {
    fn base(&self) -> &ControlBase;

    // ----- Control.lua methods -----

    /// `Control:GetPos()` (Control.lua:100-127).
    fn get_pos(&self, g: &Gfx) -> (f64, f64) {
        let b = self.base();
        let (point, other_point, collapse) = {
            let a = b.anchor.borrow();
            (a.point, a.other_point, a.collapse)
        };
        let other = b.anchor_other();
        if collapse
            && let Some(o) = &other
            && !o.base().shown.get(g)
        {
            let (x, y) = o.get_pos(g);
            return (x + b.collapse_x.get().unwrap_or(0.0), y + b.collapse_y.get().unwrap_or(0.0));
        }
        let mut x = b.x.get(g);
        let mut y = b.y.get(g);
        if let Some(o) = other {
            let (other_x, other_y) = o.get_pos(g);
            let (mut other_w, mut other_h) = (0.0, 0.0);
            let (mut width, mut height) = (0.0, 0.0);
            let op = other_point.unwrap_or("?");
            let other_pos = anchor_pos(op).unwrap_or_else(|| panic!("invalid anchor position '{op}'"));
            if op != "TOPLEFT" {
                (other_w, other_h) = o.get_size(g);
            }
            let p = point.unwrap_or("?");
            let pos = anchor_pos(p).unwrap_or_else(|| panic!("invalid anchor position '{p}'"));
            if p != "TOPLEFT" {
                (width, height) = self.get_size(g);
            }
            x = (other_x + other_w * other_pos.0 + x - width * pos.0).floor();
            y = (other_y + other_h * other_pos.1 + y - height * pos.1).floor();
        }
        (x, y)
    }

    /// `Control:GetSize()`.
    fn get_size(&self, g: &Gfx) -> (f64, f64) {
        (self.base().width.get(g), self.base().height.get(g))
    }

    /// `Control:IsShown()` (Control.lua:133-135).
    fn is_shown(&self, g: &Gfx) -> bool {
        let b = self.base();
        let collapse = b.anchor.borrow().collapse;
        let other = b.anchor_other();
        (other.is_none() || collapse || other.is_some_and(|o| o.is_shown(g))) && b.shown.get(g)
    }

    fn is_enabled(&self, g: &Gfx) -> bool {
        self.base().enabled.get(g)
    }

    /// `Control:IsMouseInBounds()` (Control.lua:141-146).
    fn is_mouse_in_bounds(&self, g: &Gfx) -> bool {
        let (x, y) = self.get_pos(g);
        let (width, height) = self.get_size(g);
        let (cursor_x, cursor_y) = g.get_cursor_pos();
        cursor_x >= x && cursor_y >= y && cursor_x < x + width && cursor_y < y + height
    }

    /// `Control:SetFocus(focus)` (Control.lua:148-157).
    fn set_focus(&self, g: &mut Gfx, focus: bool) {
        if focus != self.base().has_focus.get() {
            if focus {
                self.on_focus_gained(g);
            } else {
                self.on_focus_lost(g);
            }
            self.base().has_focus.set(focus);
        }
    }

    // ----- optional methods of subclasses -----

    fn on_focus_gained(&self, _g: &mut Gfx) {}
    fn on_focus_lost(&self, _g: &mut Gfx) {}

    fn has_draw(&self) -> bool {
        false
    }
    fn draw(&self, _f: &mut Frame<'_>, _view_port: &Rect, _no_tooltip: bool) {}

    /// `control.IsMouseOver and control:IsMouseOver()`: `None` when the class has no `IsMouseOver`.
    fn is_mouse_over(&self, _g: &Gfx) -> Option<bool> {
        None
    }

    fn has_on_key_down(&self) -> bool {
        false
    }
    fn on_key_down(self: Rc<Self>, _g: &mut Gfx, _key: &str, _double_click: bool) -> Option<ControlRef> {
        None
    }

    fn has_on_key_up(&self) -> bool {
        false
    }
    /// Returns the new `selControl` (or, for a hovered control, "consumed" when `Some`).
    fn on_key_up(self: Rc<Self>, _g: &mut Gfx, _key: &str) -> Option<ControlRef> {
        None
    }

    fn has_on_char(&self) -> bool {
        false
    }
    fn on_char(self: Rc<Self>, _g: &mut Gfx, _key: &str) -> Option<ControlRef> {
        None
    }

    fn has_on_hover_key_up(&self) -> bool {
        false
    }
    fn on_hover_key_up(&self, _g: &mut Gfx, _key: &str) {}

    /// `control.mouseClipRect` (ControlHost.lua:31-35).
    fn mouse_clip_rect(&self, _g: &Gfx) -> Option<[f64; 4]> {
        None
    }

    /// `selControl.selDragActive and selControl.dragTargetList` (ControlHost.lua:17).
    fn sel_drag_holds_focus(&self) -> bool {
        false
    }

    /// `selControl.imbuedSelect and selControl.dropped` (ControlHost.lua:55).
    fn imbued_dropped(&self) -> bool {
        false
    }
}

/// `Control:AddToTabGroup(master)` (Control.lua:159-166).
pub fn add_to_tab_group(this: &ControlRef, master: &ControlRef) {
    let order = master.base().tab_order.borrow().clone();
    let order = match order {
        Some(o) => {
            o.borrow_mut().push(Rc::downgrade(this));
            o
        }
        None => {
            let o = Rc::new(RefCell::new(vec![Rc::downgrade(master), Rc::downgrade(this)]));
            *master.base().tab_order.borrow_mut() = Some(Rc::clone(&o));
            o
        }
    };
    *this.base().tab_order.borrow_mut() = Some(order);
}

/// `Control:TabAdvance(step)` (Control.lua:168-188).
pub fn tab_advance(this: ControlRef, g: &Gfx, step: i64) -> ControlRef {
    let order = this.base().tab_order.borrow().clone();
    if let Some(order) = order {
        let list: Vec<ControlRef> = order
            .borrow()
            .iter()
            .map(|w| w.upgrade().expect("tab group member dropped"))
            .collect();
        if let Some(i) = list.iter().position(|c| same(&**c, &*this)) {
            let n = list.len() as i64;
            let mut index = i as i64 + 1;
            loop {
                index += step;
                if index > n {
                    index = 1;
                } else if index < 1 {
                    index = n;
                }
                let c = &list[(index - 1) as usize];
                if same(&**c, &*this) || (c.has_on_key_down() && c.is_shown(g)) {
                    return Rc::clone(c);
                }
            }
        }
    }
    this
}
