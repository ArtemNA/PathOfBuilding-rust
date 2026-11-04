//! `EditControl.lua`: `ControlHost` + `Control` + `UndoHandler` + `TooltipHost`. Buffer positions are 1-based **byte**
//! indices like the original (`#`, `sub`, `DrawStringCursorIndex`; MINEFIELD U6).

use crate::common::{disable_scroll_control_interaction, unicode};
use crate::control::{Anchor, Control, ControlBase, ControlRef, Prop, Rect, RectProps, anchor_weak, tab_advance};
use crate::controls::basic::ButtonControl;
use crate::controls::scroll_bar::ScrollBarControl;
use crate::host::ControlHost;
use crate::lua::{self, sub, sub_from};
use crate::sg;
use crate::tooltip::TooltipHost;
use crate::undo::{UndoHandler, UndoHost};
use pob_gfx::{Frame, Gfx};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

const PROTECTED_REPLACE: &str = "*";

/// `lastLine(str)` (EditControl.lua:12-23).
fn last_line(s: &str) -> String {
    match s.rfind('\n') {
        Some(i) => s[i + 1..].to_string(),
        None => s.to_string(),
    }
}

/// `newlineCount(str)` (EditControl.lua:25-37).
fn newline_count(s: &str) -> usize {
    s.bytes().filter(|&b| b == b'\n').count()
}

/// An undo state `{ buf, caret }`: a shared table — `self.lastUndoState.caret = …` updates the state on the undo stack.
pub struct EditUndoState {
    pub buf: String,
    pub caret: Cell<usize>,
}

/// `self.inactiveText`: a string or `function(buf)`.
#[derive(Clone)]
pub enum InactiveText {
    Str(String),
    Fn(Rc<dyn Fn(&str) -> String>),
}

/// `changeFunc(buf, placeholder?)`.
pub type ChangeFunc = Rc<dyn Fn(&mut Gfx, &str, bool)>;

pub struct EditControl {
    pub base: ControlBase,
    pub host: ControlHost,
    pub undo: UndoHandler<Rc<EditUndoState>>,
    pub tip: TooltipHost<()>,
    pub buf: RefCell<String>,
    pub caret: Cell<usize>,
    pub sel: Cell<Option<usize>>,
    pub prompt: RefCell<Option<String>>,
    pub filter: RefCell<String>,
    pub filter_pattern: RefCell<String>,
    pub limit: Cell<Option<usize>>,
    pub change_func: RefCell<Option<ChangeFunc>>,
    pub line_height: Cell<Option<f64>>,
    pub default_line_height: Cell<Option<f64>>,
    pub font: Cell<&'static str>,
    pub text_col: RefCell<String>,
    pub inactive_col: RefCell<String>,
    pub disable_col: RefCell<String>,
    pub sel_col: RefCell<String>,
    pub sel_bg_col: RefCell<String>,
    pub blink_start: Cell<f64>,
    pub allow_zoom: Cell<bool>,
    pub is_numeric: Cell<bool>,
    pub number_inc: Cell<Option<f64>>,
    pub protected: Cell<bool>,
    pub placeholder: RefCell<Option<String>>,
    pub inactive_text: RefCell<Option<InactiveText>>,
    pub border_func: RefCell<Option<Rc<dyn Fn() -> (f64, f64, f64)>>>,
    #[allow(clippy::type_complexity)]
    pub enter_func: RefCell<Option<Rc<dyn Fn(&mut Gfx, &str)>>>,
    pub paste_filter: RefCell<Option<Rc<dyn Fn(String) -> String>>>,
    pub drag: Cell<bool>,
    pub last_undo_state: RefCell<Option<Rc<EditUndoState>>>,
    pub scroll_bar_h: Rc<ScrollBarControl>,
    pub scroll_bar_v: Rc<ScrollBarControl>,
    pub button_down: Option<Rc<ButtonControl>>,
    pub button_up: Option<Rc<ButtonControl>>,
    pub button_clear: Option<Rc<ButtonControl>>,
}

/// Constructor arguments after `anchor, rect` (EditControl.lua:43).
#[derive(Default)]
pub struct EditArgs {
    pub init: Option<String>,
    pub prompt: Option<String>,
    pub filter: Option<String>,
    pub limit: Option<usize>,
    pub change_func: Option<ChangeFunc>,
    pub line_height: Option<f64>,
    pub allow_zoom: bool,
    pub clearable: bool,
}

impl EditControl {
    /// `EditControl:EditControl(anchor, rect, init, prompt, filter, limit, changeFunc, lineHeight, allowZoom, clearable)`
    /// (EditControl.lua:43-99).
    pub fn new(g: &Gfx, anchor: Option<Anchor>, rect: RectProps, a: EditArgs) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| {
            let base = ControlBase::new(anchor, rect);
            let host = ControlHost::new();
            let filter = a.filter.unwrap_or_else(|| if unicode() { "%c".into() } else { "^%w%p ".into() });
            let me_dyn: Weak<dyn Control> = me.clone();
            let button_size = {
                let me = me.clone();
                Prop::func(move |g| me.upgrade().expect("edit").get_size(g).1 - 4.0)
            };
            let (mut button_down, mut button_up, mut button_clear) = (None, None, None);
            let is_numeric = filter == "%D" || filter == "^%-%d";
            if is_numeric {
                let m = me.clone();
                let down = ButtonControl::new(
                    anchor_weak("RIGHT", me_dyn.clone(), "RIGHT"),
                    [(-2.0).into(), 0.0.into(), button_size.clone(), button_size.clone()],
                    "-".to_string(),
                    move |g| {
                        let _ = Rc::clone(&m.upgrade().expect("edit")).on_key_up(g, "DOWN");
                    },
                );
                let m = me.clone();
                let down_dyn: Weak<dyn Control> = Rc::downgrade(&down) as Weak<dyn Control>;
                let up = ButtonControl::new(
                    anchor_weak("RIGHT", down_dyn, "LEFT"),
                    [(-1.0).into(), 0.0.into(), button_size.clone(), button_size.clone()],
                    "+".to_string(),
                    move |g| {
                        let _ = Rc::clone(&m.upgrade().expect("edit")).on_key_up(g, "UP");
                    },
                );
                host.add("buttonDown", down.clone());
                host.add("buttonUp", up.clone());
                button_down = Some(down);
                button_up = Some(up);
            } else if a.clearable {
                let m = me.clone();
                let clear = ButtonControl::new(
                    anchor_weak("RIGHT", me_dyn.clone(), "RIGHT"),
                    [(-2.0).into(), 0.0.into(), button_size.clone(), button_size],
                    "x".to_string(),
                    move |g| {
                        m.upgrade().expect("edit").set_text(g, "", true);
                    },
                );
                let m = me.clone();
                clear.base.shown.set_fn(move |g| {
                    let e = m.upgrade().expect("edit");
                    !e.buf.borrow().is_empty() && e.is_mouse_in_bounds(g)
                });
                host.add("buttonClear", clear.clone());
                button_clear = Some(clear);
            }
            let scroll_bar_h = ScrollBarControl::new(
                anchor_weak("BOTTOMLEFT", me_dyn.clone(), "BOTTOMLEFT"),
                [1.0.into(), (-1.0).into(), 0.0.into(), 14.0.into()],
                Some(60.0),
                Some("HORIZONTAL"),
                true,
            );
            let m = me.clone();
            scroll_bar_h.base.width.set_fn(move |g| {
                let e = m.upgrade().expect("edit");
                let (width, _) = e.get_size(g);
                width - if e.scroll_bar_v.base.enabled.get(g) { 16.0 } else { 2.0 }
            });
            host.add("scrollBarH", scroll_bar_h.clone());
            let scroll_bar_v = ScrollBarControl::new(
                anchor_weak("TOPRIGHT", me_dyn, "TOPRIGHT"),
                [(-1.0).into(), 1.0.into(), 14.0.into(), 0.0.into()],
                Some(a.line_height.unwrap_or(0.0) * 3.0),
                Some("VERTICAL"),
                true,
            );
            let m = me.clone();
            scroll_bar_v.base.height.set_fn(move |g| {
                let e = m.upgrade().expect("edit");
                let (_, height) = e.get_size(g);
                height - if e.scroll_bar_h.base.enabled.get(g) { 16.0 } else { 2.0 }
            });
            host.add("scrollBarV", scroll_bar_v.clone());
            if a.line_height.is_none() {
                scroll_bar_h.base.shown.set(false);
                scroll_bar_v.base.shown.set(false);
            }
            let e = Self {
                base,
                host,
                undo: UndoHandler::default(),
                tip: TooltipHost::new(None),
                buf: RefCell::new(String::new()),
                caret: Cell::new(1),
                sel: Cell::new(None),
                prompt: RefCell::new(a.prompt),
                filter_pattern: RefCell::new(format!("[{filter}]")),
                filter: RefCell::new(filter),
                limit: Cell::new(a.limit),
                change_func: RefCell::new(None),
                line_height: Cell::new(a.line_height),
                default_line_height: Cell::new(a.line_height),
                font: Cell::new("VAR"),
                text_col: RefCell::new("^7".into()),
                inactive_col: RefCell::new("^8".into()),
                disable_col: RefCell::new("^9".into()),
                sel_col: RefCell::new("^0".into()),
                sel_bg_col: RefCell::new("^xBBBBBB".into()),
                blink_start: Cell::new(g.get_time() as f64),
                allow_zoom: Cell::new(a.allow_zoom),
                is_numeric: Cell::new(is_numeric),
                number_inc: Cell::new(None),
                protected: Cell::new(false),
                placeholder: RefCell::new(None),
                inactive_text: RefCell::new(None),
                border_func: RefCell::new(None),
                enter_func: RefCell::new(None),
                paste_filter: RefCell::new(None),
                drag: Cell::new(false),
                last_undo_state: RefCell::new(None),
                scroll_bar_h,
                scroll_bar_v,
                button_down,
                button_up,
                button_clear,
            };
            // `self:SetText(init or "")` runs before `changeFunc` is assigned (EditControl.lua:48 vs :53).
            e.set_text_raw(a.init.as_deref().unwrap_or(""));
            e.reset_undo();
            *e.change_func.borrow_mut() = a.change_func;
            e
        })
    }

    fn call_change(&self, g: &mut Gfx, placeholder: bool) {
        let f = self.change_func.borrow().clone();
        if let Some(f) = f {
            let text = if placeholder {
                self.placeholder.borrow().clone().unwrap_or_default()
            } else {
                self.buf.borrow().clone()
            };
            f(g, &text, placeholder);
        }
    }

    fn set_text_raw(&self, text: &str) {
        *self.buf.borrow_mut() = text.to_string();
        self.caret.set(text.len() + 1);
        self.sel.set(None);
    }

    fn buf_len(&self) -> usize {
        self.buf.borrow().len()
    }

    fn line_height_or(&self, g: &Gfx) -> f64 {
        self.line_height.get().unwrap_or_else(|| self.get_size(g).1 - 4.0)
    }

    fn set_last_undo_caret(&self) {
        if let Some(s) = &*self.last_undo_state.borrow() {
            s.caret.set(self.caret.get());
        }
    }

    /// EditControl.lua:101-109
    pub fn set_text(&self, g: &mut Gfx, text: &str, notify: bool) {
        self.set_text_raw(text);
        if notify {
            self.call_change(g, false);
        }
        self.reset_undo();
    }

    /// EditControl.lua:111-116
    pub fn set_placeholder(&self, g: &mut Gfx, text: &str, notify: bool) {
        *self.placeholder.borrow_mut() = Some(text.to_string());
        if notify {
            self.call_change(g, true);
        }
    }

    /// EditControl.lua:118-123 (`self.protected = bool or true`: always true).
    pub fn set_protected(&self, _b: bool) {
        self.protected.set(true);
        self.font.set("FIXED");
    }

    /// EditControl.lua:125-130
    fn mouse_over(&self, g: &Gfx) -> bool {
        if !self.is_shown(g) {
            return false;
        }
        self.is_mouse_in_bounds(g) || self.host.get_mouse_over_control(g).is_some()
    }

    /// EditControl.lua:132-136
    pub fn select_all(&self, g: &Gfx) {
        self.caret.set(self.buf_len() + 1);
        self.sel.set(Some(1));
        self.scroll_caret_into_view(g);
    }

    /// EditControl.lua:138-143
    pub fn get_sel_text(&self) -> String {
        let sel = self.sel.get().expect("GetSelText without selection");
        let (left, right) = (self.caret.get().min(sel), self.caret.get().max(sel));
        sub(&self.buf.borrow(), left as i64, right as i64 - 1)
    }

    /// EditControl.lua:145-165
    pub fn replace_sel(&self, g: &mut Gfx, text: &str) {
        let text = lua::gsub(text, "\r", "");
        if lua::is_match(&text, &self.filter_pattern.borrow()) {
            return;
        }
        let sel = self.sel.get().expect("ReplaceSel without selection");
        let (left, right) = (self.caret.get().min(sel), self.caret.get().max(sel));
        let new_buf = {
            let buf = self.buf.borrow();
            format!("{}{}{}", sub(&buf, 1, left as i64 - 1), text, sub_from(&buf, right as i64))
        };
        if self.limit.get().is_some_and(|l| new_buf.len() > l) {
            return;
        }
        *self.buf.borrow_mut() = new_buf;
        self.caret.set(left + text.len());
        self.sel.set(None);
        self.scroll_caret_into_view(g);
        self.blink_start.set(g.get_time() as f64);
        self.call_change(g, false);
        self.add_undo_state(false);
    }

    /// EditControl.lua:167-187
    pub fn insert(&self, g: &mut Gfx, text: &str) {
        let text = lua::gsub(text, "\r", "");
        let text = lua::gsub(&text, &self.filter_pattern.borrow(), "");
        if text.is_empty() {
            return;
        }
        let new_buf = {
            let buf = self.buf.borrow();
            format!(
                "{}{}{}",
                sub(&buf, 1, self.caret.get() as i64 - 1),
                text,
                sub_from(&buf, self.caret.get() as i64)
            )
        };
        if self.limit.get().is_some_and(|l| new_buf.len() > l) {
            return;
        }
        *self.buf.borrow_mut() = new_buf;
        self.caret.set(self.caret.get() + text.len());
        self.sel.set(None);
        self.scroll_caret_into_view(g);
        self.blink_start.set(g.get_time() as f64);
        self.call_change(g, false);
        self.add_undo_state(false);
    }

    /// EditControl.lua:189-210
    pub fn zoom_text(&self, zoom: &str) {
        let Some(lh) = self.line_height.get() else { return };
        if !self.allow_zoom.get() {
            return;
        }
        let mut text_height = lh;
        match zoom {
            "+" => text_height += 1.0,
            "-" => text_height -= 1.0,
            "0" => text_height = self.default_line_height.get().expect("defaultLineHeight"),
            _ => {}
        }
        self.line_height.set(Some(text_height.clamp(10.0, 100.0)));
    }

    /// EditControl.lua:212-221
    pub fn update_scroll_bars(&self, g: &Gfx) {
        let (width, height) = self.get_size(g);
        let text_height = self.line_height_or(g);
        let font = self.font.get();
        let buf = self.buf.borrow().clone();
        if self.line_height.get().is_some() {
            self.scroll_bar_h
                .set_content_dimension(g, sg::draw_string_width(g, text_height, font, &buf) + 2.0, width - 18.0);
            let h_enabled = self.scroll_bar_h.base.enabled.get(g);
            self.scroll_bar_v.set_content_dimension(
                g,
                newline_count(&format!("{buf}\n")) as f64 * text_height,
                height - if h_enabled { 18.0 } else { 4.0 },
            );
        } else {
            let prompt_w = self
                .prompt
                .borrow()
                .as_ref()
                .map_or(0.0, |p| sg::draw_string_width(g, text_height, font, p) + text_height / 2.0);
            self.scroll_bar_h
                .set_content_dimension(g, sg::draw_string_width(g, text_height, font, &buf) + 2.0, width - 4.0 - prompt_w);
        }
    }

    /// EditControl.lua:223-234
    pub fn scroll_caret_into_view(&self, g: &Gfx) {
        let text_height = self.line_height_or(g);
        let pre = sub(&self.buf.borrow(), 1, self.caret.get() as i64 - 1);
        let caret_x = sg::draw_string_width(g, text_height, self.font.get(), &last_line(&pre));
        self.update_scroll_bars(g);
        self.scroll_bar_h.scroll_into_view(caret_x - text_height, text_height * 2.0);
        if self.line_height.get().is_some() {
            let caret_y = newline_count(&pre) as f64 * text_height;
            self.scroll_bar_v.scroll_into_view(caret_y, text_height);
        }
    }

    /// EditControl.lua:236-244
    pub fn move_caret_vertically(&self, g: &Gfx, offset: f64) {
        let lh = self.line_height.get().expect("lineHeight");
        let pre = sub(&self.buf.borrow(), 1, self.caret.get() as i64 - 1);
        let caret_x = sg::draw_string_width(g, lh, self.font.get(), &last_line(&pre));
        let caret_y = newline_count(&pre) as f64 * lh;
        let idx = sg::draw_string_cursor_index(
            g,
            lh,
            self.font.get(),
            &self.buf.borrow(),
            caret_x + 1.0,
            caret_y + lh / 2.0 + offset,
        );
        self.caret.set(idx);
        self.set_last_undo_caret();
        self.scroll_caret_into_view(g);
        self.blink_start.set(g.get_time() as f64);
    }

    fn blink_on(&self, g: &Gfx) -> bool {
        lua_mod(g.get_time() as f64 - self.blink_start.get(), 1000.0) < 500.0
    }

    fn sel_or_caret_update(&self, g: &Gfx) {
        let shift = sg::is_key_down(g, "SHIFT");
        self.sel.set(if shift {
            Some(self.sel.get().unwrap_or(self.caret.get()))
        } else {
            None
        });
    }
}

/// Lua `a % b` (floored).
fn lua_mod(a: f64, b: f64) -> f64 {
    a - (a / b).floor() * b
}

impl UndoHost for EditControl {
    type State = Rc<EditUndoState>;
    fn undo_handler(&self) -> &UndoHandler<Self::State> {
        &self.undo
    }
    /// EditControl.lua:743-750
    fn create_undo_state(&self) -> Self::State {
        let s = Rc::new(EditUndoState {
            buf: self.buf.borrow().clone(),
            caret: Cell::new(self.caret.get()),
        });
        *self.last_undo_state.borrow_mut() = Some(Rc::clone(&s));
        s
    }
    /// EditControl.lua:752-761 — `changeFunc` needs the host; it is called by [`EditControl::undo_g`].
    fn restore_undo_state(&self, state: Self::State) {
        *self.buf.borrow_mut() = state.buf.clone();
        self.caret.set(state.caret.get());
        self.sel.set(None);
        *self.last_undo_state.borrow_mut() = Some(state);
    }
}

impl EditControl {
    /// `RestoreUndoState` needs `GetSize`/`DrawStringWidth` (ScrollCaretIntoView) and calls `changeFunc`: the generic
    /// mixin has no host access, so `Undo`/`Redo` of the Edit go through these, in the same order as Lua:
    /// buf/caret/sel, `ScrollCaretIntoView`, `changeFunc`, `lastUndoState` (EditControl.lua:752-761), then
    /// `AddUndoState(true)` (UndoHandler.lua:41-55).
    fn restore_and_notify(&self, g: &mut Gfx, state: Rc<EditUndoState>) {
        *self.buf.borrow_mut() = state.buf.clone();
        self.caret.set(state.caret.get());
        self.sel.set(None);
        self.scroll_caret_into_view(g);
        self.call_change(g, false);
        *self.last_undo_state.borrow_mut() = Some(state);
    }

    /// `self:Undo()` (UndoHandler.lua:41-47).
    pub fn undo_g(&self, g: &mut Gfx) {
        let u = &self.undo;
        if u.undo.borrow().len() >= 2 {
            let top = u.undo.borrow_mut().remove(0);
            u.redo.borrow_mut().insert(0, top);
            let s = u.undo.borrow_mut().remove(0);
            self.restore_and_notify(g, s);
            self.add_undo_state(true);
        }
    }

    /// `self:Redo()` (UndoHandler.lua:50-55).
    pub fn redo_g(&self, g: &mut Gfx) {
        let u = &self.undo;
        if !u.redo.borrow().is_empty() {
            let s = u.redo.borrow_mut().remove(0);
            self.restore_and_notify(g, s);
            self.add_undo_state(true);
        }
    }

    fn text_origin(&self, g: &Gfx) -> (f64, f64, f64) {
        let (x, y) = self.get_pos(g);
        let text_height = self.line_height_or(g);
        let mut text_x = x + 2.0;
        if let Some(p) = &*self.prompt.borrow() {
            text_x += sg::draw_string_width(g, text_height, self.font.get(), p) + text_height / 2.0;
        }
        (text_x, y + 2.0, text_height)
    }
}

impl Control for EditControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn debug_value(&self, _g: &Gfx) -> Option<serde_json::Value> {
        Some(serde_json::json!(self.buf.borrow().clone()))
    }

    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        Some(self.mouse_over(g))
    }

    fn has_draw(&self) -> bool {
        true
    }

    /// EditControl.lua:246-434 (the last `textX` update of a line is dead in Lua too).
    #[allow(unused_assignments)]
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let (width, height) = self.get_size(f);
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
        sg::draw_rect(f, x, y, width, height);
        if !enabled {
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
        } else if self.base.has_focus.get() || m_over {
            if self.line_height.get().is_some() {
                sg::set_draw_color(f, 0.1, 0.1, 0.1);
            } else {
                sg::set_draw_color(f, 0.15, 0.15, 0.15);
            }
        } else {
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
        }
        sg::draw_rect(f, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        let mut text_x = x + 2.0;
        let mut text_y = y + 2.0;
        let text_height = self.line_height_or(f);
        let font = self.font.get();
        let prompt = self.prompt.borrow().clone();
        if let Some(p) = &prompt {
            if !enabled {
                let s = format!("{}{}", self.disable_col.borrow(), p);
                sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &s);
            } else {
                let s = format!("{}{}:", self.text_col.borrow(), p);
                sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &s);
            }
            text_x += sg::draw_string_width(f, text_height, font, p) + text_height / 2.0;
        }
        if !enabled {
            return;
        }
        if m_over && !no_tooltip {
            sg::set_draw_layer(f, None, 100);
            self.tip.draw_tooltip(f, x, y, width, height, view_port, ());
            sg::set_draw_layer(f, None, 0);
        }
        self.update_scroll_bars(f);
        let margin_l = text_x - x - 2.0;
        let margin_r = if self.scroll_bar_v.is_shown(f) { 14.0 } else { 0.0 };
        let margin_b = if self.scroll_bar_h.is_shown(f) { 14.0 } else { 0.0 };
        sg::set_viewport(
            f,
            Some([text_x, text_y, width - 4.0 - margin_l - margin_r, height - 4.0 - margin_b]),
        );
        let text_col = self.text_col.borrow().clone();
        let sel_col = self.sel_col.borrow().clone();
        let sel_bg_col = self.sel_bg_col.borrow().clone();
        let off_h = self.scroll_bar_h.offset.get();
        let off_v = self.scroll_bar_v.offset.get();
        if !self.base.has_focus.get() {
            let buf = self.buf.borrow().clone();
            let placeholder = self.placeholder.borrow().clone();
            if buf.is_empty()
                && let Some(ph) = placeholder
            {
                sg::set_draw_color_str(f, &self.disable_col.borrow().clone());
                sg::draw_string(f, -off_h, -off_v, Some("LEFT"), text_height, font, &ph);
            } else {
                sg::set_draw_color_str(f, &self.inactive_col.borrow().clone());
                let inactive = self.inactive_text.borrow().clone();
                if let Some(it) = inactive {
                    let s = match it {
                        InactiveText::Str(s) => s,
                        InactiveText::Fn(func) => func(&buf),
                    };
                    sg::draw_string(f, -off_h, -off_v, Some("LEFT"), text_height, font, &s);
                } else if self.protected.get() {
                    sg::draw_string(
                        f,
                        -off_h,
                        -off_v,
                        Some("LEFT"),
                        text_height,
                        font,
                        &lua::rep(PROTECTED_REPLACE, buf.len() as i64),
                    );
                } else {
                    sg::draw_string(f, -off_h, -off_v, Some("LEFT"), text_height, font, &buf);
                }
            }
            sg::set_viewport(f, None);
            self.draw_children(f, view_port, no_tooltip);
            return;
        }
        if !sg::is_key_down(f, "LEFTBUTTON") {
            self.drag.set(false);
        }
        if self.drag.get() {
            let (cursor_x, cursor_y) = f.get_cursor_pos();
            let idx = sg::draw_string_cursor_index(
                f,
                text_height,
                font,
                &self.buf.borrow(),
                cursor_x - text_x + off_h,
                cursor_y - text_y + off_v,
            );
            self.caret.set(idx);
            self.set_last_undo_caret();
            self.scroll_caret_into_view(f);
        }
        // offsets may have changed in ScrollCaretIntoView
        let off_h = self.scroll_bar_h.offset.get();
        let off_v = self.scroll_bar_v.offset.get();
        text_x = -off_h;
        text_y = -off_v;
        let buf = self.buf.borrow().clone();
        let caret = self.caret.get();
        let blink = self.blink_on(f);
        if self.line_height.get().is_some() {
            let left = caret.min(self.sel.get().unwrap_or(caret));
            let right = caret.max(self.sel.get().unwrap_or(caret));
            let mut caret_xy: Option<(f64, f64)> = None;
            sg::set_draw_color_str(f, &text_col);
            for (s, line, e) in lua::lines_with_pos(&buf) {
                text_x = -off_h;
                if left >= e || right <= s {
                    sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &line);
                }
                if left < e {
                    if left > s {
                        let pre = sub(&line, 1, (left - s) as i64);
                        sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &pre);
                        text_x += sg::draw_string_width(f, text_height, font, &pre);
                    }
                    if left >= s && left == caret {
                        caret_xy = Some((text_x, text_y));
                    }
                }
                if left != right && left < e && right > s {
                    let lo = 1i64.max(left as i64 - s as i64 + 1);
                    let hi = (line.len() as i64).min(right as i64 - s as i64);
                    let mut sel = format!("{}{}", sel_col, f.strip_escapes(&sub(&line, lo, hi)));
                    if right >= e {
                        sel.push_str("  ");
                    }
                    let sel_width = sg::draw_string_width(f, text_height, font, &sel);
                    sg::set_draw_color_str(f, &sel_bg_col);
                    sg::draw_rect(f, text_x, text_y, sel_width, text_height);
                    sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &sel);
                    sg::set_draw_color_str(f, &text_col);
                    text_x += sel_width;
                }
                if right >= s && right < e && right == caret {
                    caret_xy = Some((text_x, text_y));
                }
                if right > s && right < e {
                    let post = sub_from(&line, (right - s + 1) as i64);
                    sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &post);
                    text_x += sg::draw_string_width(f, text_height, font, &post);
                }
                text_y += text_height;
            }
            if let Some((cx, cy)) = caret_xy
                && blink
            {
                sg::set_draw_color_str(f, &text_col);
                sg::draw_rect(f, cx, cy, 1.0, text_height);
            }
        } else if let Some(sel_pos) = self.sel.get()
            && sel_pos != caret
        {
            let left = caret.min(sel_pos);
            let right = caret.max(sel_pos);
            let pre = format!("{}{}", text_col, sub(&buf, 1, left as i64 - 1));
            let sel = format!("{}{}", sel_col, f.strip_escapes(&sub(&buf, left as i64, right as i64 - 1)));
            let post = format!("{}{}", text_col, sub_from(&buf, right as i64));
            let protected = self.protected.get();
            if protected {
                let s = format!(
                    "{}{}",
                    text_col,
                    lua::rep(PROTECTED_REPLACE, pre.len() as i64 - text_col.len() as i64)
                );
                sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &s);
            } else {
                sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &pre);
            }
            text_x += sg::draw_string_width(f, text_height, font, &pre);
            let sel_width = sg::draw_string_width(f, text_height, font, &sel);
            sg::set_draw_color_str(f, &sel_bg_col);
            sg::draw_rect(f, text_x, text_y, sel_width, text_height);
            if protected {
                let s = format!(
                    "{}{}",
                    sel_col,
                    lua::rep(PROTECTED_REPLACE, sel.len() as i64 - sel_col.len() as i64)
                );
                sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &s);
            } else {
                sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &sel);
            }
            if protected && !post.is_empty() {
                let s = format!(
                    "{}{}",
                    text_col,
                    lua::rep(PROTECTED_REPLACE, post.len() as i64 - text_col.len() as i64)
                );
                sg::draw_string(f, text_x + sel_width, text_y, Some("LEFT"), text_height, font, &s);
            } else {
                sg::draw_string(f, text_x + sel_width, text_y, Some("LEFT"), text_height, font, &post);
            }
            if blink {
                let caret_x = if caret > sel_pos { text_x + sel_width } else { text_x };
                sg::set_draw_color_str(f, &text_col);
                sg::draw_rect(f, caret_x, text_y, 1.0, text_height);
            }
        } else {
            let placeholder = self.placeholder.borrow().clone();
            if buf.is_empty()
                && let Some(ph) = placeholder
            {
                sg::set_draw_color_str(f, &self.disable_col.borrow().clone());
                sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &ph);
            }
            let pre = format!("{}{}", text_col, sub(&buf, 1, caret as i64 - 1));
            let post = sub_from(&buf, caret as i64);
            if self.protected.get() {
                let s = format!(
                    "{}{}",
                    text_col,
                    lua::rep(PROTECTED_REPLACE, pre.len() as i64 - text_col.len() as i64)
                );
                sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &s);
            } else {
                sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &pre);
            }
            text_x += sg::draw_string_width(f, text_height, font, &pre);
            if self.protected.get() && !post.is_empty() {
                sg::draw_string(
                    f,
                    text_x,
                    text_y,
                    Some("LEFT"),
                    text_height,
                    font,
                    &lua::rep(PROTECTED_REPLACE, post.len() as i64),
                );
            } else {
                sg::draw_string(f, text_x, text_y, Some("LEFT"), text_height, font, &post);
            }
            if blink {
                sg::set_draw_color_str(f, &text_col);
                sg::draw_rect(f, text_x, text_y, 1.0, text_height);
            }
        }
        sg::set_viewport(f, None);
        self.draw_children(f, view_port, no_tooltip);
    }

    /// EditControl.lua:436-441
    fn on_focus_gained(&self, g: &mut Gfx) {
        self.blink_start.set(g.get_time() as f64);
        if !self.drag.get() && self.host.sel().is_none() && self.line_height.get().is_none() {
            self.select_all(g);
        }
    }

    fn has_on_key_down(&self) -> bool {
        true
    }

    /// EditControl.lua:443-666
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        let m_over_control = self.host.get_mouse_over_control(g);
        if let Some(m) = m_over_control
            && m.has_on_key_down()
        {
            *self.host.sel_control.borrow_mut() = Some(Rc::clone(&m));
            return m.on_key_down(g, key, false).map(|_| self as ControlRef);
        }
        *self.host.sel_control.borrow_mut() = None;
        let shift = sg::is_key_down(g, "SHIFT");
        let ctrl = sg::is_key_down(g, "CTRL");
        let buf_len = self.buf_len();
        match key {
            "LEFTBUTTON" => {
                if !self.mouse_over(g) {
                    return None;
                }
                if double_click {
                    if self.line_height.get().is_some() {
                        let b = |i: i64, j: i64| sub(&self.buf.borrow(), i, j);
                        let c = self.caret.get() as i64;
                        if lua::is_match(&b(c - 1, c), "^%C\n$") {
                            self.caret.set(self.caret.get() - 1);
                        }
                        while {
                            let c = self.caret.get() as i64;
                            lua::is_match(&b(c - 1, c), "[^\n][ \t]")
                        } {
                            self.caret.set(self.caret.get() - 1);
                        }
                        let c = self.caret.get() as i64;
                        let caret_char = b(c, c);
                        if lua::is_match(&caret_char, "%w") {
                            self.sel.set(Some(self.caret.get()));
                            while {
                                let s = self.sel.get().expect("sel") as i64;
                                lua::is_match(&b(s - 1, s - 1), "%w")
                            } {
                                self.sel.set(Some(self.sel.get().expect("sel") - 1));
                            }
                            while {
                                let c = self.caret.get() as i64;
                                lua::is_match(&b(c, c), "%w")
                            } {
                                self.caret.set(self.caret.get() + 1);
                            }
                        } else if lua::is_match(&caret_char, "%S") {
                            self.sel.set(Some(self.caret.get()));
                            while {
                                let s = self.sel.get().expect("sel") as i64;
                                b(s - 1, s - 1) == caret_char
                            } {
                                self.sel.set(Some(self.sel.get().expect("sel") - 1));
                            }
                            while {
                                let c = self.caret.get() as i64;
                                b(c, c) == caret_char
                            } {
                                self.caret.set(self.caret.get() + 1);
                            }
                        }
                    } else {
                        self.sel.set(Some(1));
                        self.caret.set(buf_len + 1);
                    }
                    self.set_last_undo_caret();
                    self.scroll_caret_into_view(g);
                } else if ctrl && lua::is_match(&self.buf.borrow(), "[a-z]*://[^ >,;]*") {
                    let url = self.buf.borrow().clone();
                    g.open_url(&url);
                } else {
                    self.drag.set(true);
                    let (text_x, text_y, text_height) = self.text_origin(g);
                    let (cursor_x, cursor_y) = g.get_cursor_pos();
                    let idx = sg::draw_string_cursor_index(
                        g,
                        text_height,
                        self.font.get(),
                        &self.buf.borrow(),
                        cursor_x - text_x + self.scroll_bar_h.offset.get(),
                        cursor_y - text_y + self.scroll_bar_v.offset.get(),
                    );
                    self.caret.set(idx);
                    self.sel.set(Some(idx));
                    self.set_last_undo_caret();
                    self.scroll_caret_into_view(g);
                    self.blink_start.set(g.get_time() as f64);
                }
            }
            "ESCAPE" => return None,
            "RETURN" => {
                if self.line_height.get().is_some() {
                    self.insert(g, "\n");
                } else {
                    let ef = self.enter_func.borrow().clone();
                    if let Some(ef) = ef {
                        let buf = self.buf.borrow().clone();
                        ef(g, &buf);
                    }
                    return None;
                }
            }
            "a" if ctrl => self.select_all(g),
            "c" | "x" if ctrl && !self.protected.get() => {
                if let Some(sel) = self.sel.get()
                    && sel != self.caret.get()
                {
                    let (left, right) = (self.caret.get().min(sel), self.caret.get().max(sel));
                    let text = sub(&self.buf.borrow(), left as i64, right as i64 - 1);
                    g.copy(&text);
                    if key == "x" {
                        self.replace_sel(g, "");
                    }
                }
            }
            _ if (key == "v" && ctrl) || (key == "RIGHTBUTTON" && self.mouse_over(g)) => {
                if let Some(mut text) = g.paste() {
                    let pf = self.paste_filter.borrow().clone();
                    if let Some(pf) = pf {
                        text = pf(text);
                    }
                    // `text:gsub("[\128-\255]", "?")`: every non-ASCII byte becomes "?"
                    let text: String = text.bytes().map(|b| if b >= 128 { '?' } else { b as char }).collect();
                    if self.sel.get().is_some_and(|s| s != self.caret.get()) {
                        self.replace_sel(g, &text);
                    } else {
                        self.insert(g, &text);
                    }
                }
            }
            "z" if ctrl => self.undo_g(g),
            "y" if ctrl => self.redo_g(g),
            "LEFT" => {
                self.sel_or_caret_update(g);
                if self.caret.get() > 1 {
                    if ctrl {
                        let pre = sub(&self.buf.borrow(), 1, self.caret.get() as i64 - 1);
                        let n = lua::lmatch(&pre, "[%s%p]*$").map_or(0, |m| m.len());
                        self.caret.set(self.caret.get() - n);
                        let pre = sub(&self.buf.borrow(), 1, self.caret.get() as i64 - 1);
                        let n = lua::lmatch(&pre, "%w*$").map_or(0, |m| m.len());
                        self.caret.set(self.caret.get() - n);
                    } else {
                        self.caret
                            .set(lua::utf8_next(&self.buf.borrow(), self.caret.get(), -1).unwrap_or(0));
                    }
                    self.set_last_undo_caret();
                    self.scroll_caret_into_view(g);
                    self.blink_start.set(g.get_time() as f64);
                }
            }
            "RIGHT" => {
                self.sel_or_caret_update(g);
                if self.caret.get() <= buf_len {
                    if ctrl {
                        let post = sub_from(&self.buf.borrow(), self.caret.get() as i64);
                        let n = lua::lmatch(&post, "^%w*").map_or(0, |m| m.len());
                        self.caret.set(self.caret.get() + n);
                        let post = sub_from(&self.buf.borrow(), self.caret.get() as i64);
                        let n = lua::lmatch(&post, "^[%s%p]*").map_or(0, |m| m.len());
                        self.caret.set(self.caret.get() + n);
                    } else {
                        self.caret
                            .set(lua::utf8_next(&self.buf.borrow(), self.caret.get(), 1).unwrap_or(buf_len + 1));
                    }
                    self.set_last_undo_caret();
                    self.scroll_caret_into_view(g);
                    self.blink_start.set(g.get_time() as f64);
                }
            }
            "UP" if self.line_height.get().is_some() => {
                self.sel_or_caret_update(g);
                self.move_caret_vertically(g, -self.line_height.get().expect("lineHeight"));
            }
            "DOWN" if self.line_height.get().is_some() => {
                self.sel_or_caret_update(g);
                self.move_caret_vertically(g, self.line_height.get().expect("lineHeight"));
            }
            "HOME" => {
                self.sel_or_caret_update(g);
                if self.line_height.get().is_some() && !ctrl {
                    let pre = sub(&self.buf.borrow(), 1, self.caret.get() as i64 - 1);
                    self.caret.set(self.caret.get() - last_line(&pre).len());
                } else {
                    self.caret.set(1);
                }
                self.set_last_undo_caret();
                self.scroll_caret_into_view(g);
                self.blink_start.set(g.get_time() as f64);
            }
            "END" => {
                self.sel_or_caret_update(g);
                if self.line_height.get().is_some() && !ctrl {
                    let post = sub_from(&self.buf.borrow(), self.caret.get() as i64);
                    let n = post.find('\n').unwrap_or(post.len());
                    self.caret.set(self.caret.get() + n);
                } else {
                    self.caret.set(buf_len + 1);
                }
                self.set_last_undo_caret();
                self.scroll_caret_into_view(g);
                self.blink_start.set(g.get_time() as f64);
            }
            "PAGEUP" if self.line_height.get().is_some() => {
                self.sel_or_caret_update(g);
                let (_, height) = self.get_size(g);
                self.move_caret_vertically(g, -height + 18.0);
            }
            "PAGEDOWN" if self.line_height.get().is_some() => {
                self.sel_or_caret_update(g);
                let (_, height) = self.get_size(g);
                self.move_caret_vertically(g, height - 18.0);
            }
            "BACK" => {
                if self.sel.get().is_some_and(|s| s != self.caret.get()) {
                    self.replace_sel(g, "");
                } else if self.caret.get() > 1 {
                    let c = self.caret.get() as i64;
                    let b = |i: i64| sub(&self.buf.borrow(), i, i);
                    let mut len: i64 = 1;
                    if sg::is_key_down(g, "CTRL") {
                        while c - len > 1 && lua::is_match(&b(c - len), "%s") && !lua::is_match(&b(c - len - 1), "\n") {
                            len += 1;
                        }
                        if lua::is_match(&b(c - len), "%w") {
                            while c - len > 1 && lua::is_match(&b(c - len - 1), "%w") {
                                len += 1;
                            }
                        }
                    }
                    let new_buf = {
                        let buf = self.buf.borrow();
                        format!("{}{}", sub(&buf, 1, c - 1 - len), sub_from(&buf, c))
                    };
                    *self.buf.borrow_mut() = new_buf;
                    self.caret.set((c - len) as usize);
                    self.sel.set(None);
                    self.scroll_caret_into_view(g);
                    self.blink_start.set(g.get_time() as f64);
                    self.call_change(g, false);
                    self.add_undo_state(false);
                }
            }
            "DELETE" => {
                if self.sel.get().is_some_and(|s| s != self.caret.get()) {
                    self.replace_sel(g, "");
                } else if self.caret.get() <= buf_len {
                    let c = self.caret.get() as i64;
                    let n = buf_len as i64;
                    let b = |i: i64| sub(&self.buf.borrow(), i, i);
                    let mut len: i64 = 1;
                    if sg::is_key_down(g, "CTRL") {
                        while c + len <= n && lua::is_match(&b(c + len - 1), "%s") && !lua::is_match(&b(c + len), "\n") {
                            len += 1;
                        }
                        if lua::is_match(&b(c + len - 1), "%w") {
                            while c + len <= n && lua::is_match(&b(c + len), "%w") {
                                len += 1;
                            }
                        }
                    }
                    let new_buf = {
                        let buf = self.buf.borrow();
                        format!("{}{}", sub(&buf, 1, c - 1), sub_from(&buf, c + len))
                    };
                    *self.buf.borrow_mut() = new_buf;
                    self.sel.set(None);
                    self.blink_start.set(g.get_time() as f64);
                    self.call_change(g, false);
                    self.add_undo_state(false);
                }
            }
            "TAB" => return Some(tab_advance(self, g, if shift { -1 } else { 1 })),
            "+" | "-" | "0" if ctrl && self.allow_zoom.get() => self.zoom_text(key),
            _ => {}
        }
        Some(self)
    }

    fn has_on_key_up(&self) -> bool {
        true
    }

    /// EditControl.lua:668-727
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if let Some(sel) = self.host.sel() {
            if sel.on_key_up(g, key).is_some() {
                return Some(self);
            }
            *self.host.sel_control.borrow_mut() = None;
        }
        let ctrl = sg::is_key_down(g, "CTRL");
        let no_scroll = disable_scroll_control_interaction();
        if key == "LEFTBUTTON" {
            if self.drag.get() {
                self.drag.set(false);
            }
        } else if self.is_numeric.get() {
            let cur = pob_lua::tonumber::tonumber(&self.buf.borrow());
            let inc = self.number_inc.get().unwrap_or(1.0);
            let placeholder = self.placeholder.borrow().clone();
            let tostr = pob_lua::format::tostring_num;
            if (!no_scroll && key == "WHEELUP") || key == "UP" {
                if let Some(cur) = cur {
                    self.set_text(g, &tostr(cur + inc), true);
                } else if let Some(ph) = placeholder {
                    self.set_text(g, &tostr(pob_lua::tonumber::tonumber(&ph).unwrap_or(0.0) + inc), true);
                } else {
                    self.set_text(g, "1", true);
                }
            } else if (!no_scroll && key == "WHEELDOWN") || key == "DOWN" {
                if let Some(cur) = cur
                    && (*self.filter.borrow() != "%D" || cur > 0.0)
                {
                    self.set_text(g, &tostr(cur - inc), true);
                } else if let Some(ph) = placeholder {
                    self.set_text(g, &tostr(pob_lua::tonumber::tonumber(&ph).unwrap_or(0.0) - inc), true);
                } else {
                    self.set_text(g, "0", true);
                }
            }
        } else if ScrollBarControl::is_scroll_up_key(key) {
            if ctrl && self.allow_zoom.get() {
                self.zoom_text("+");
            } else if self.scroll_bar_v.base.enabled.get(g) {
                self.scroll_bar_v.scroll(-1.0);
            } else {
                self.scroll_bar_h.scroll(-1.0);
            }
        } else if ScrollBarControl::is_scroll_down_key(key) {
            if ctrl && self.allow_zoom.get() {
                self.zoom_text("-");
            } else if self.scroll_bar_v.base.enabled.get(g) {
                self.scroll_bar_v.scroll(1.0);
            } else {
                self.scroll_bar_h.scroll(1.0);
            }
        }
        if self.base.has_focus.get() { Some(self) } else { None }
    }

    fn has_on_char(&self) -> bool {
        true
    }

    /// EditControl.lua:729-741
    fn on_char(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if key != "\u{8}" {
            if self.sel.get().is_some_and(|s| s != self.caret.get()) {
                self.replace_sel(g, key);
            } else {
                self.insert(g, key);
            }
        }
        Some(self)
    }
}

impl EditControl {
    /// `self:DrawControls(viewPort, noTooltip and self)`.
    fn draw_children(&self, f: &mut Frame<'_>, view_port: &Rect, no_tooltip: bool) {
        // `noTooltip and self`: when set, children compare against this edit's focus.
        let me: Option<ControlRef> = if no_tooltip { self.self_ref() } else { None };
        self.host.draw_controls(f, view_port, me.as_ref());
    }

    /// A strong reference to this control (for `selControl` comparisons): the scroll bar's anchor holds a weak one.
    fn self_ref(&self) -> Option<ControlRef> {
        self.scroll_bar_h.base.anchor.borrow().other.as_ref().and_then(Weak::upgrade)
    }
}
