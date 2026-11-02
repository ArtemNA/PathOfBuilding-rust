//! `Classes/NotesTab.lua`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use pob_gfx::{Frame, Gfx};
use serde_json::json;

use crate::colors::color_code;
use crate::control::{Control, ControlRef, Rect, anchor, rect};
use crate::controls::basic::{ButtonControl, LabelControl};
use crate::controls::edit::{EditArgs, EditControl};
use crate::host::{ControlHost, InputEvent, InputEvents, ipairs};
use crate::modules::build::BuildMode;
use crate::modules::main::{BareControl, main};
use crate::sg;

const NOTES_DESC: &str = "^7You can use Ctrl +/- (or Ctrl+Scroll) to zoom in and out and Ctrl+0 to reset.
This field also supports different colors.  Using the caret symbol (^) followed by a Hex code or a number (0-9) will set the color.
Below are some common color codes PoB uses:\t";

fn gsub(s: &str, pat: &str, repl: &str) -> String {
    let out = pob_lua::gsub(s.as_bytes(), pat.as_bytes(), pob_lua::Repl::Str(repl.as_bytes()), None)
        .expect("NotesTab pattern")
        .0;
    String::from_utf8_lossy(&out).into_owned()
}

/// `:gsub("%^x(%x%x%x%x%x%x)","^_x%1"):gsub("%^(%d)","^_%1")`.
fn show_codes(s: &str) -> String {
    gsub(&gsub(s, "%^x(%x%x%x%x%x%x)", "^_x%1"), "%^(%d)", "^_%1")
}

pub struct NotesTab {
    pub anchor: Rc<BareControl>,
    pub host: ControlHost,
    build: Weak<BuildMode>,
    pub edit: Rc<EditControl>,
    pub toggle_color_codes: Rc<ButtonControl>,
    last_content: RefCell<String>,
    pub show_color_codes: Cell<bool>,
    pub mod_flag: Cell<bool>,
    /// What the engine has: `Save` reads the buffer.
    synced: RefCell<String>,
}

impl NotesTab {
    /// :12-51, then `Load` (:75-82).
    pub fn new(g: &mut Gfx, build: Weak<BuildMode>) -> Rc<Self> {
        let tab = Rc::new_cyclic(|me: &Weak<Self>| {
            let host = ControlHost::new();
            let a = BareControl::new(None, rect(0.0, 0.0, 0.0, 0.0));
            let a_ref: ControlRef = a.clone();
            let notes_desc = LabelControl::new(
                anchor("TOPLEFT", &a_ref, "TOPLEFT"),
                rect(8.0, 8.0, 150.0, 16.0),
                NOTES_DESC.to_string(),
            );
            let desc_ref: ControlRef = notes_desc.clone();
            let button = |other: &ControlRef, x: f64, y: f64, label: String, color: String| {
                let m = me.clone();
                ButtonControl::new(anchor("TOPLEFT", other, "TOPLEFT"), rect(x, y, 100.0, 18.0), label, move |g| {
                    m.upgrade().expect("notesTab").set_color(g, &color)
                })
            };
            let code = |c: &str| color_code(c).unwrap_or_else(|| panic!("colorCodes.{c}"));
            let coded = |other: &ControlRef, x: f64, y: f64, name: &str| {
                let c = code(name);
                button(other, x, y, format!("{c}{name}"), c)
            };
            let normal = coded(&desc_ref, 0.0, 48.0, "NORMAL");
            let normal_r: ControlRef = normal.clone();
            let magic = coded(&normal_r, 120.0, 0.0, "MAGIC");
            let magic_r: ControlRef = magic.clone();
            let rare = coded(&magic_r, 120.0, 0.0, "RARE");
            let rare_r: ControlRef = rare.clone();
            let unique = coded(&rare_r, 120.0, 0.0, "UNIQUE");
            let fire = coded(&normal_r, 0.0, 18.0, "FIRE");
            let fire_r: ControlRef = fire.clone();
            let cold = coded(&fire_r, 120.0, 0.0, "COLD");
            let cold_r: ControlRef = cold.clone();
            let lightning = coded(&cold_r, 120.0, 0.0, "LIGHTNING");
            let lightning_r: ControlRef = lightning.clone();
            let chaos = coded(&lightning_r, 120.0, 0.0, "CHAOS");
            let strength = coded(&fire_r, 0.0, 18.0, "STRENGTH");
            let strength_r: ControlRef = strength.clone();
            let dexterity = coded(&strength_r, 120.0, 0.0, "DEXTERITY");
            let dexterity_r: ControlRef = dexterity.clone();
            let intelligence = coded(&dexterity_r, 120.0, 0.0, "INTELLIGENCE");
            let intelligence_r: ControlRef = intelligence.clone();
            let default = button(&intelligence_r, 120.0, 0.0, "^7DEFAULT".to_string(), "^7".to_string());
            let edit = EditControl::new(
                g,
                anchor("TOPLEFT", &fire_r, "TOPLEFT"),
                rect(0.0, 48.0, 0.0, 0.0),
                EditArgs {
                    init: Some(String::new()),
                    filter: Some("^%C\t\n".into()),
                    line_height: Some(16.0),
                    allow_zoom: true,
                    ..EditArgs::default()
                },
            );
            let ar = a.clone();
            edit.base.width.set_fn(move |g| ar.base.width.get(g) - 16.0);
            let ar = a.clone();
            edit.base.height.set_fn(move |g| ar.base.height.get(g) - 128.0);
            let m = me.clone();
            let toggle_color_codes = ButtonControl::new(
                anchor("TOPRIGHT", &a_ref, "TOPRIGHT"),
                rect(-10.0, 70.0, 160.0, 20.0),
                "Show Color Codes".to_string(),
                move |_g| {
                    let t = m.upgrade().expect("notesTab");
                    t.set_show_color_codes(!t.show_color_codes.get());
                },
            );
            host.add("notesDesc", notes_desc);
            host.add("normal", normal);
            host.add("magic", magic);
            host.add("rare", rare);
            host.add("unique", unique);
            host.add("fire", fire);
            host.add("cold", cold);
            host.add("lightning", lightning);
            host.add("chaos", chaos);
            host.add("strength", strength);
            host.add("dexterity", dexterity);
            host.add("intelligence", intelligence);
            host.add("default", default);
            host.add("edit", edit.clone());
            host.add("toggleColorCodes", toggle_color_codes.clone());
            Self {
                anchor: a,
                host,
                build,
                edit,
                toggle_color_codes,
                last_content: RefCell::new(String::new()),
                show_color_codes: Cell::new(false),
                mod_flag: Cell::new(false),
                synced: RefCell::new(String::new()),
            }
        });
        tab.host.select_control(g, Some(tab.edit.clone()));
        let text = tab.engine_notes();
        // NotesTab.lua:75-82 `SetText(node)` for a text node
        if !text.is_empty() {
            tab.edit.set_text(g, &text, false);
        }
        *tab.last_content.borrow_mut() = tab.edit.buf.borrow().clone();
        *tab.synced.borrow_mut() = text;
        tab
    }

    fn engine_notes(&self) -> String {
        let b = self.build.upgrade().expect("buildMode");
        let e = b.engine.borrow();
        e.as_ref().expect("engine").notes().to_string()
    }

    /// :53-62
    pub fn set_show_color_codes(&self, setting: bool) {
        self.show_color_codes.set(setting);
        let buf = self.edit.buf.borrow().clone();
        if setting {
            self.toggle_color_codes.label.set("Hide Color Codes".to_string());
            *self.edit.buf.borrow_mut() = show_codes(&buf);
        } else {
            self.toggle_color_codes.label.set("Show Color Codes".to_string());
            *self.edit.buf.borrow_mut() = gsub(&gsub(&buf, "%^_x(%x%x%x%x%x%x)", "^x%1"), "%^_(%d)", "^%1");
        }
    }

    /// :64-73
    fn set_color(&self, g: &mut Gfx, color: &str) {
        let show = self.show_color_codes.get();
        let text = if show { show_codes(color) } else { color.to_string() };
        let e = &self.edit;
        if e.sel.get().is_none() || e.sel.get() == Some(e.caret.get()) {
            e.insert(g, &text);
        } else {
            let sel = e.get_sel_text();
            let pat: &[u8] = if show { b"^.*(%^_x%x%x%x%x%x%x)" } else { b"^.*(%^x%x%x%x%x%x%x)" };
            let last_color = pob_lua::match_(sel.as_bytes(), pat, None)
                .expect("NotesTab pattern")
                .and_then(|c| {
                    c.get(0)
                        .and_then(|x| x.bytes(sel.as_bytes()).map(|b| String::from_utf8_lossy(b).into_owned()))
                })
                .unwrap_or_else(|| "^7".to_string());
            let stripped = gsub(&sel, if show { "%^_x%x%x%x%x%x%x" } else { "%^x%x%x%x%x%x%x" }, "");
            e.replace_sel(g, &format!("{text}{stripped}{last_color}"));
        }
    }

    /// :84-87: `Save` turns the color codes back on; what it writes is the buffer ([`Self::sync`]).
    pub fn before_save(&self) {
        self.set_show_color_codes(false);
        self.sync();
    }

    /// :89-92
    pub fn reset_mod_flags(&self) {
        *self.last_content.borrow_mut() = self.edit.buf.borrow().clone();
        self.mod_flag.set(false);
    }

    fn sync(&self) {
        let buf = self.edit.buf.borrow();
        if *self.synced.borrow() != *buf {
            let b = self.build.upgrade().expect("buildMode");
            b.engine.borrow_mut().as_mut().expect("engine").set_notes(&buf);
            self.synced.borrow_mut().clone_from(&buf);
        }
    }

    /// :94-116
    pub fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, input_events: &mut InputEvents) {
        let a = &self.anchor.base;
        a.x.set(view_port.x);
        a.y.set(view_port.y);
        a.width.set(view_port.width);
        a.height.set(view_port.height);
        let downs: Vec<String> = ipairs(input_events)
            .filter_map(|(_, e)| match e {
                InputEvent::KeyDown { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();
        for key in downs {
            if key == "z" && sg::is_key_down(f, "CTRL") {
                self.edit.undo_g(f);
            } else if key == "y" && sg::is_key_down(f, "CTRL") {
                self.edit.redo_g(f);
            }
        }
        self.host.process_controls_input(f, input_events, view_port);
        main().draw_background(f, view_port);
        self.host.draw_controls(f, view_port, None);
        self.mod_flag.set(*self.last_content.borrow() != *self.edit.buf.borrow());
        self.sync();
    }

    /// The state the gate compares (the same fields as `config-runner.lua`).
    pub fn debug_dump(&self, g: &Gfx) -> serde_json::Value {
        let e = &self.edit;
        let (x, y) = e.get_pos(g);
        let (w, h) = e.get_size(g);
        let sel = match self.host.sel() {
            Some(c) if crate::control::same(&*c, &**e) => "edit",
            Some(_) => "other",
            None => "nil",
        };
        json!({
            "buf": e.buf.borrow().clone(),
            "caret": e.caret.get(),
            "sel": e.sel.get().unwrap_or(0),
            "lineHeight": e.line_height.get().unwrap_or(0.0),
            "show": self.show_color_codes.get(),
            "toggleLabel": self.toggle_color_codes.label.get(g),
            "modFlag": self.mod_flag.get(),
            "rect": format!("{x},{y},{w},{h}"),
            "focus": sel,
        })
    }
}
