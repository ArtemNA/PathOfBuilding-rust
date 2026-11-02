//! `Modules/ToastNotification.lua`: toast notifications stacked above the bottom bar (the Lua module's locals are this
//! struct's fields; `main` owns the instance like the global `ToastNotification`).

use crate::control::{Control, ControlRef, Rect, WeakControl, anchor_weak, rect};
use crate::controls::ButtonControl;
use crate::host::{InputEvent, InputEvents, ipairs};
use crate::sg;
use pob_gfx::{Frame, Gfx};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

/// ToastNotification.lua:20-21
const SHOW_DURATION: f64 = 250.0;
const HIDE_DURATION: f64 = 75.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastMode {
    Showing,
    Shown,
    Hiding,
}

pub struct Toast {
    pub id: String,
    pub message: RefCell<String>,
    pub mode: Cell<Option<ToastMode>>,
    pub start: Cell<f64>,
    pub height: Cell<f64>,
    pub current_height: Cell<f64>,
    pub dismiss_button: RefCell<Option<Rc<ButtonControl>>>,
}

#[derive(Default)]
pub struct ToastNotification {
    toasts: RefCell<Vec<Rc<Toast>>>,
    dismissed_ids: Rc<RefCell<HashSet<String>>>,
    next_id: Cell<u64>,
    anchor_main: RefCell<Option<WeakControl>>,
    screen_h: Cell<f64>,
}

/// ToastNotification.lua:33-37 (`#message:gsub("[^\n]", "")` = the number of newlines).
fn calculate_height(message: &str) -> f64 {
    message.bytes().filter(|&b| b == b'\n').count() as f64 * 16.0 + 20.0 + 40.0
}

impl ToastNotification {
    /// `ToastNotification:Init(anchor, …)` (ToastNotification.lua:40-45).
    pub fn init(&self, anchor: &ControlRef) {
        *self.anchor_main.borrow_mut() = Some(Rc::downgrade(anchor));
        if self.next_id.get() == 0 {
            self.next_id.set(1);
        }
    }

    /// `ToastNotification:Add(message)` (:55-70).
    pub fn add(&self, message: &str) -> String {
        if self.next_id.get() == 0 {
            self.next_id.set(1);
        }
        let id = format!("toast_{}", self.next_id.get());
        self.next_id.set(self.next_id.get() + 1);
        self.toasts.borrow_mut().push(Rc::new(Toast {
            id: id.clone(),
            message: RefCell::new(message.to_string()),
            mode: Cell::new(None),
            start: Cell::new(0.0),
            height: Cell::new(calculate_height(message)),
            current_height: Cell::new(0.0),
            dismiss_button: RefCell::new(None),
        }));
        id
    }

    /// `ToastNotification:Update(id, message)` (:74-85).
    pub fn update(&self, id: &str, message: &str) -> bool {
        for t in self.toasts.borrow().iter() {
            if t.id == id {
                *t.message.borrow_mut() = message.to_string();
                t.height.set(calculate_height(message));
                return true;
            }
        }
        false
    }

    /// `ToastNotification:Remove(id, immediate)` (:89-104).
    pub fn remove(&self, g: &Gfx, id: &str, immediate: bool) -> bool {
        let pos = self.toasts.borrow().iter().position(|t| t.id == id);
        let Some(i) = pos else { return false };
        if immediate {
            self.toasts.borrow_mut().remove(i);
        } else {
            let t = self.toasts.borrow()[i].clone();
            t.mode.set(Some(ToastMode::Hiding));
            t.start.set(g.get_time() as f64);
        }
        true
    }

    /// `ToastNotification:Exists(id)`.
    pub fn exists(&self, id: &str) -> bool {
        self.toasts.borrow().iter().any(|t| t.id == id)
    }

    /// What the toast's dismiss button leaves (:186).
    pub fn dismiss(&self, id: &str) {
        self.dismissed_ids.borrow_mut().insert(id.to_string());
    }

    /// The text of a toast.
    pub fn message(&self, id: &str) -> Option<String> {
        self.toasts.borrow().iter().find(|t| t.id == id).map(|t| t.message.borrow().clone())
    }

    /// `ToastNotification:Clear(immediate)` (:129-138).
    pub fn clear(&self, g: &Gfx, immediate: bool) {
        if immediate {
            self.toasts.borrow_mut().clear();
        } else {
            for t in self.toasts.borrow().iter() {
                t.mode.set(Some(ToastMode::Hiding));
                t.start.set(g.get_time() as f64);
            }
        }
    }

    /// `ToastNotification:WasDismissed(id)`.
    pub fn was_dismissed(&self, id: &str) -> bool {
        self.dismissed_ids.borrow().contains(id)
    }

    /// `ToastNotification:ClearDismissed(id)`.
    pub fn clear_dismissed(&self, id: Option<&str>) {
        match id {
            Some(id) => {
                self.dismissed_ids.borrow_mut().remove(id);
            }
            None => self.dismissed_ids.borrow_mut().clear(),
        }
    }

    /// `ToastNotification:GetTotalHeight()`.
    pub fn get_total_height(&self) -> f64 {
        self.toasts.borrow().iter().map(|t| t.current_height.get()).sum()
    }

    /// `ToastNotification:UpdateFrame(events, vp, height)`: only `screenH` is kept; events and viewport are passed to
    /// [`Self::render`] (the Lua module stores references to the same tables).
    pub fn update_frame(&self, screen_h: f64) {
        self.screen_h.set(screen_h);
    }

    /// `ToastNotification:Render()` (:154-235) → total toast height.
    pub fn render(&self, f: &mut Frame<'_>, input_events: &InputEvents, view_port: &Rect) -> f64 {
        let mut total = 0.0;
        let mut to_remove = Vec::new();
        let toasts: Vec<Rc<Toast>> = self.toasts.borrow().clone();
        for (i, toast) in toasts.iter().enumerate() {
            if toast.mode.get().is_none() {
                toast.mode.set(Some(ToastMode::Showing));
                toast.start.set(f.get_time() as f64);
                let anchor = self.anchor_main.borrow().clone().expect("ToastNotification:Init");
                let t = Rc::downgrade(toast);
                let dismissed = self.dismissed_ids.clone();
                let b = ButtonControl::new(
                    anchor_weak("BOTTOMLEFT", anchor, "BOTTOMLEFT"),
                    rect(4.0, 0.0, 80.0, 20.0),
                    "Dismiss".to_string(),
                    move |g| {
                        if let Some(t) = t.upgrade() {
                            dismissed.borrow_mut().insert(t.id.clone());
                            t.mode.set(Some(ToastMode::Hiding));
                            t.start.set(g.get_time() as f64);
                        }
                    },
                );
                *toast.dismiss_button.borrow_mut() = Some(b);
            }
            let now = f.get_time() as f64;
            match toast.mode.get() {
                Some(ToastMode::Showing) => {
                    if now >= toast.start.get() + SHOW_DURATION {
                        toast.mode.set(Some(ToastMode::Shown));
                        toast.current_height.set(toast.height.get());
                    } else {
                        toast
                            .current_height
                            .set(toast.height.get() * (now - toast.start.get()) / SHOW_DURATION);
                    }
                }
                Some(ToastMode::Shown) => toast.current_height.set(toast.height.get()),
                Some(ToastMode::Hiding) => {
                    if now >= toast.start.get() + HIDE_DURATION {
                        to_remove.push(i);
                        toast.current_height.set(0.0);
                    } else {
                        toast
                            .current_height
                            .set(toast.height.get() * (1.0 - (now - toast.start.get()) / HIDE_DURATION));
                    }
                }
                None => {}
            }
            total += toast.current_height.get();
        }
        for &i in to_remove.iter().rev() {
            self.toasts.borrow_mut().remove(i);
        }
        let mut y_offset = 58.0;
        let screen_h = self.screen_h.get();
        let toasts: Vec<Rc<Toast>> = self.toasts.borrow().clone();
        for toast in toasts {
            let ch = toast.current_height.get();
            if ch > 0.0 {
                let toast_y = screen_h - y_offset - ch;
                sg::set_draw_color(f, 0.85, 0.85, 0.85);
                sg::draw_rect(f, 0.0, toast_y, 312.0, ch);
                sg::set_draw_color(f, 0.1, 0.1, 0.1);
                sg::draw_rect(f, 0.0, toast_y + 4.0, 308.0, ch - 4.0);
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
                let msg = toast.message.borrow().clone();
                sg::draw_string(
                    f,
                    4.0,
                    toast_y + 8.0,
                    Some("LEFT"),
                    20.0,
                    "VAR",
                    &crate::lua::gsub(&msg, "\n.*", ""),
                );
                sg::draw_string(
                    f,
                    4.0,
                    toast_y + 28.0,
                    Some("LEFT"),
                    16.0,
                    "VAR",
                    &crate::lua::gsub(&msg, "^[^\n]*\n?", ""),
                );
                let button = toast.dismiss_button.borrow().clone();
                if toast.mode.get() == Some(ToastMode::Shown)
                    && let Some(button) = button
                {
                    button.base.y.set(-(y_offset + 4.0));
                    for (_, event) in ipairs(input_events) {
                        if button.is_mouse_over(f) == Some(true) {
                            match event {
                                InputEvent::KeyDown { key, double_click } => {
                                    let _ = Rc::clone(&button).on_key_down(f, key, *double_click);
                                }
                                InputEvent::KeyUp { key } => {
                                    let _ = Rc::clone(&button).on_key_up(f, key);
                                }
                                InputEvent::Char { .. } => {}
                            }
                        }
                    }
                    button.draw(f, view_port, false);
                }
                y_offset += ch;
            }
        }
        total
    }
}
