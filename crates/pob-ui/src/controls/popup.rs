//! `Classes/PopupDialog.lua`: a modal dialog box (`ControlHost` + `Control`) with a configurable set of controls.
//! `main.screenW/H` come from [`crate::common::screen_size`]; `main:ClosePopup()` from [`crate::modules::main`].

use crate::common::screen_size;
use crate::control::{Control, ControlBase, ControlRef, PropCell, Rect, WeakControl};
use crate::controls::scroll_bar::ScrollBarControl;
use crate::host::{ControlHost, InputEvent, InputEvents, ipairs};
use crate::sg;
use indexmap::IndexMap;
use pob_gfx::{Frame, Gfx};
use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

/// `scrollBarFunc` / `resizeFunc`: called from `Draw` before the controls are drawn.
pub type PopupFunc = Rc<dyn Fn(&Gfx)>;

/// The popup's own `DrawControls(viewPort)` replacement (Main.lua:1293-1304 assigns one to the options popup).
pub type DrawControlsFn = Rc<dyn Fn(&PopupDialog, &mut Frame<'_>, &Rect)>;

pub struct PopupDialog {
    pub base: ControlBase,
    pub host: ControlHost,
    pub title: PropCell<String>,
    pub enter_control: Option<String>,
    pub escape_control: Option<String>,
    pub scroll_bar_func: Option<PopupFunc>,
    pub resize_func: Option<PopupFunc>,
    pub draw_controls_override: RefCell<Option<DrawControlsFn>>,
}

impl PopupDialog {
    /// `PopupDialog:PopupDialog(width, height, title, controls, enterControl, defaultControl, escapeControl,
    /// scrollBarFunc, resizeFunc)` (PopupDialog.lua:11-41). `controls` iterate in insertion order (Lua `pairs`: array
    /// part first, then the hash part — callers insert numbered labels first, MINEFIELD U3).
    pub fn new(
        g: &mut Gfx,
        width: f64,
        height: f64,
        title: &str,
        controls: IndexMap<String, ControlRef>,
        enter_control: Option<&str>,
        default_control: Option<&str>,
        escape_control: Option<&str>,
        scroll_bar_func: Option<PopupFunc>,
        resize_func: Option<PopupFunc>,
    ) -> Rc<Self> {
        let popup = Rc::new_cyclic(|me: &Weak<Self>| {
            let base = ControlBase::new(None, [0.0.into(), 0.0.into(), width.into(), height.into()]);
            base.x.set_fn(move |_| ((screen_size().0 - width) / 2.0).floor());
            base.y.set_fn(move |_| ((screen_size().1 - height) / 2.0).floor());
            let me_dyn: WeakControl = me.clone();
            for control in controls.values() {
                let b = control.base();
                let (has_point, has_other) = {
                    let a = b.anchor.borrow();
                    (a.point.is_some(), a.other.is_some())
                };
                if !has_point {
                    b.set_anchor("TOP", Some(me_dyn.clone()), "TOP", None, false);
                } else if !has_other {
                    b.anchor.borrow_mut().other = Some(me_dyn.clone());
                }
                // `type(control.anchor.other) ~= "table"` (an anchor given by control name): not used by the ported callers.
            }
            let host = ControlHost::new();
            *host.controls.borrow_mut() = controls;
            Self {
                base,
                host,
                title: PropCell::new(title.to_string()),
                enter_control: enter_control.map(str::to_string),
                escape_control: escape_control.map(str::to_string),
                scroll_bar_func,
                resize_func,
                draw_controls_override: RefCell::new(None),
            }
        });
        if let Some(d) = default_control {
            let c = popup.host.get(d);
            popup.host.select_control(g, c);
        }
        popup
    }

    /// `self.controls[name]`.
    pub fn control_names(&self) -> Vec<String> {
        self.host.controls_named().into_iter().map(|x| x.0).collect()
    }

    pub fn control(&self, name: &str) -> ControlRef {
        self.host.get(name).unwrap_or_else(|| panic!("popup has no control '{name}'"))
    }

    /// `self:DrawControls(viewPort)` — the override if one was assigned.
    pub fn draw_controls(&self, f: &mut Frame<'_>, view_port: &Rect) {
        let o = self.draw_controls_override.borrow().clone();
        match o {
            Some(o) => o(self, f, view_port),
            None => self.host.draw_controls(f, view_port, None),
        }
    }

    /// `self.controls.scrollBar` as a scroll bar.
    fn scroll_bar(&self) -> Rc<ScrollBarControl> {
        let c: Rc<dyn Any> = self.control("scrollBar");
        c.downcast::<ScrollBarControl>()
            .unwrap_or_else(|_| panic!("popup control 'scrollBar' is not a ScrollBarControl"))
    }

    /// `PopupDialog:Draw(viewPort)` (PopupDialog.lua:43-67).
    pub fn draw_popup(&self, f: &mut Frame<'_>, view_port: &Rect) {
        let (x, y) = self.get_pos(f);
        let (width, height) = self.get_size(f);
        sg::set_draw_color(f, 0.8, 0.8, 0.8);
        sg::draw_rect(f, x, y, width, height);
        sg::set_draw_color(f, 0.1, 0.1, 0.1);
        sg::draw_rect(f, x + 2.0, y + 2.0, width - 4.0, height - 4.0);
        let title = self.title.get(f);
        let title_width = sg::draw_string_width(f, 16.0, "VAR", &title);
        let title_x = x + ((width - title_width - 8.0) / 2.0).floor();
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_rect(f, title_x, y - 10.0, title_width + 8.0, 24.0);
        sg::set_draw_color(f, 0.0, 0.0, 0.0);
        sg::draw_rect(f, title_x + 2.0, y - 8.0, title_width + 4.0, 20.0);
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_string(f, title_x + 4.0, y - 7.0, Some("LEFT"), 16.0, "VAR", &title);
        if let Some(func) = &self.scroll_bar_func {
            func(f);
        }
        if let Some(func) = &self.resize_func {
            func(f);
        }
        self.draw_controls(f, view_port);
    }

    /// `PopupDialog:ProcessInput(inputEvents, viewPort)` (PopupDialog.lua:69-93).
    pub fn process_input(&self, g: &mut Gfx, input_events: &mut InputEvents, view_port: &Rect) {
        self.host.process_controls_input(g, input_events, view_port);
        let events: Vec<InputEvent> = ipairs(input_events).map(|(_, e)| e.clone()).collect();
        for event in events {
            match &event {
                InputEvent::KeyDown { key, .. } => {
                    if key == "ESCAPE" {
                        match &self.escape_control {
                            Some(name) => self.control(name).click(g),
                            None => crate::modules::main::main().close_popup(),
                        }
                        return;
                    } else if key == "RETURN"
                        && let Some(name) = &self.enter_control
                    {
                        self.control(name).click(g);
                        return;
                    }
                }
                InputEvent::KeyUp { key } if self.scroll_bar_func.is_some() => {
                    let sb = self.scroll_bar();
                    if ScrollBarControl::is_scroll_down_key(key) {
                        sb.scroll(1.0);
                    } else if ScrollBarControl::is_scroll_up_key(key) {
                        sb.scroll(-1.0);
                    }
                }
                _ => {}
            }
        }
    }
}

impl Control for PopupDialog {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn has_draw(&self) -> bool {
        true
    }
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, _no_tooltip: bool) {
        self.draw_popup(f, view_port);
    }
}
