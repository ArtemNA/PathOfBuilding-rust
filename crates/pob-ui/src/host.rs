//! `Classes/ControlHost.lua` (host of child controls) and the frame's input events (`main.inputEvents`, Main.lua:495-505).

use crate::common::is_mouse_in_region;
use crate::control::{ControlRef, Rect, same, same_opt};
use indexmap::IndexMap;
use pob_gfx::{Frame, Gfx};
use std::cell::RefCell;
use std::rc::Rc;

/// `{ type = "KeyDown"|"KeyUp"|"Char", key = …, doubleClick = … }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    KeyDown { key: String, double_click: bool },
    KeyUp { key: String },
    Char { key: String },
}

/// `inputEvents`: a Lua array in which consumed events become holes (`inputEvents[id] = nil`, MINEFIELD U2). Later
/// consumers iterate with [`ipairs`] and stop at the first hole.
pub type InputEvents = Vec<Option<InputEvent>>;

/// `ipairs(t)`: indices (0-based) up to the first hole.
pub fn ipairs(events: &InputEvents) -> impl Iterator<Item = (usize, &InputEvent)> {
    events.iter().map_while(Option::as_ref).enumerate()
}

/// `ControlHost` fields (ControlHost.lua:9-12): `controls` keep insertion order (string keys iterate in an
/// unspecified order in LuaJIT, MINEFIELD U3 — insertion order is one of the valid orders).
#[derive(Default)]
pub struct ControlHost {
    pub controls: RefCell<IndexMap<String, ControlRef>>,
    pub sel_control: RefCell<Option<ControlRef>>,
    /// An instance's own `GetMouseOverControl` (Main.lua:1307-1313 assigns one to the options popup).
    pub mouse_over_override: RefCell<Option<Rc<dyn Fn(&Gfx) -> Option<ControlRef>>>>,
}

impl ControlHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// `self.controls[name] = control`.
    pub fn add(&self, name: &str, c: ControlRef) {
        self.controls.borrow_mut().insert(name.to_string(), c);
    }

    /// `self.controls[name] = nil`.
    pub fn remove(&self, name: &str) {
        self.controls.borrow_mut().shift_remove(name);
    }

    /// `(name, control)` of every control, for the gate's dump.
    pub fn controls_named(&self) -> Vec<(String, ControlRef)> {
        self.controls.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub fn get(&self, name: &str) -> Option<ControlRef> {
        self.controls.borrow().get(name).cloned()
    }

    pub fn sel(&self) -> Option<ControlRef> {
        self.sel_control.borrow().clone()
    }

    /// Snapshot for `pairs(self.controls)`: callbacks may add or remove controls during the loop.
    fn snapshot(&self) -> Vec<ControlRef> {
        self.controls.borrow().values().cloned().collect()
    }

    /// `ControlHost:SelectControl(newSelControl)` (ControlHost.lua:14-29).
    pub fn select_control(&self, g: &mut Gfx, new_sel: Option<ControlRef>) {
        let cur = self.sel();
        match (&cur, &new_sel) {
            (Some(a), Some(b)) if same(&**a, &**b) => return,
            (None, None) => return,
            _ => {}
        }
        if let Some(cur) = &cur {
            if cur.sel_drag_holds_focus() {
                return;
            }
            cur.set_focus(g, false);
        }
        *self.sel_control.borrow_mut() = new_sel.clone();
        if let Some(n) = new_sel {
            n.set_focus(g, true);
        }
    }

    /// `ControlHost:GetMouseOverControl()` (ControlHost.lua:31-48).
    pub fn get_mouse_over_control(&self, g: &Gfx) -> Option<ControlRef> {
        let o = self.mouse_over_override.borrow().clone();
        if let Some(o) = o {
            return o(g);
        }
        for control in self.snapshot() {
            if control.is_mouse_over(g) == Some(true) {
                let Some(clip) = control.mouse_clip_rect(g) else {
                    return Some(control);
                };
                let (cursor_x, cursor_y) = g.get_cursor_pos();
                if cursor_x >= clip[0] && cursor_y >= clip[1] && cursor_x < clip[0] + clip[2] && cursor_y < clip[1] + clip[3] {
                    return Some(control);
                }
            }
        }
        None
    }

    /// `ControlHost:ProcessControlsInput(inputEvents, viewPort)` (ControlHost.lua:50-107).
    pub fn process_controls_input(&self, g: &mut Gfx, input_events: &mut InputEvents, view_port: &Rect) {
        let mut id = 0;
        // `for id, event in ipairs(inputEvents)`: a hole made at `id` does not stop this loop (ipairs reads id+1 next).
        while id < input_events.len() {
            let Some(event) = input_events[id].clone() else { break };
            match event {
                InputEvent::KeyDown { key, double_click } => {
                    let prev_sel_control = self.sel();
                    if let Some(sel) = self.sel() {
                        let processed_imbued_control = sel.imbued_dropped() && key.contains("BUTTON");
                        let n = Rc::clone(&sel).on_key_down(g, &key, double_click);
                        self.select_control(g, n);
                        input_events[id] = None;
                        if processed_imbued_control {
                            return;
                        }
                    }
                    if self.sel().is_none() && key.contains("BUTTON") {
                        self.select_control(g, None);
                        if is_mouse_in_region(g, view_port) {
                            let m_over = self.get_mouse_over_control(g);
                            if let Some(m) = m_over
                                && !same_opt(prev_sel_control.as_ref(), &*m)
                                && m.has_on_key_down()
                            {
                                let n = m.on_key_down(g, &key, double_click);
                                self.select_control(g, n);
                                input_events[id] = None;
                            }
                        }
                    }
                }
                InputEvent::KeyUp { key } => {
                    let sel_control = self.sel();
                    if let Some(sel) = &sel_control {
                        if sel.has_on_key_up() {
                            let n = Rc::clone(sel).on_key_up(g, &key);
                            self.select_control(g, n);
                        }
                        input_events[id] = None;
                    }
                    let m_over = self.get_mouse_over_control(g);
                    if let Some(m) = m_over
                        && (sel_control.is_none() || m.has_on_hover_key_up())
                        && is_mouse_in_region(g, view_port)
                    {
                        if sel_control.is_none() && m.has_on_key_up() && Rc::clone(&m).on_key_up(g, &key).is_some() {
                            input_events[id] = None;
                        }
                        if m.has_on_hover_key_up() {
                            m.on_hover_key_up(g, &key);
                        }
                    }
                }
                InputEvent::Char { key } => {
                    if let Some(sel) = self.sel() {
                        if sel.has_on_char() {
                            let n = Rc::clone(&sel).on_char(g, &key);
                            self.select_control(g, n);
                        }
                        input_events[id] = None;
                    }
                }
            }
            id += 1;
        }
    }

    /// `ControlHost:DrawControls(viewPort, selControl)` (ControlHost.lua:109-115).
    pub fn draw_controls(&self, f: &mut Frame<'_>, view_port: &Rect, sel_control: Option<&ControlRef>) {
        for control in self.snapshot() {
            if control.is_shown(f) && control.has_draw() {
                let own = self.sel();
                let no_tooltip = own.as_ref().is_some_and(|s| s.base().has_focus.get() && !same(&**s, &*control))
                    || sel_control.is_some_and(|s| s.base().has_focus.get() && !same(&**s, &*control));
                control.draw(f, view_port, no_tooltip);
            }
        }
    }
}
