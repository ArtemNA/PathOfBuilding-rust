//! The `BUILD` mode until U2c ports `Modules/Build.lua`: keeps the mode arguments (for Settings.xml) and shows a
//! placeholder screen with a way back to the build list.

use super::main::{Mode, ModeArg, main};
use crate::control::Rect;
use crate::host::{InputEvent, InputEvents, ipairs};
use crate::sg;
use pob_gfx::{Frame, Gfx};
use std::cell::RefCell;

#[derive(Default)]
pub struct BuildStub {
    pub args: RefCell<Vec<ModeArg>>,
}

impl Mode for BuildStub {
    fn init(&self, _g: &mut Gfx, args: &[ModeArg]) {
        *self.args.borrow_mut() = args.to_vec();
    }

    fn get_args(&self) -> Vec<ModeArg> {
        self.args.borrow().clone()
    }

    fn on_frame(&self, f: &mut Frame<'_>, input_events: &mut InputEvents, view_port: &Rect) {
        let back = ipairs(input_events).any(|(_, e)| matches!(e, InputEvent::KeyDown { key, .. } if key == "ESCAPE"));
        if back {
            main().set_mode("LIST", Vec::new());
        }
        let name = match self.args.borrow().get(1) {
            Some(ModeArg::Str(s)) => s.clone(),
            _ => String::new(),
        };
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_string(
            f,
            0.0,
            view_port.height / 2.0 - 30.0,
            Some("CENTER"),
            20.0,
            "VAR",
            &format!("^7Build: ^xFFFF77{name}\n^7The build screen is not ported yet (U2c).\n^8Press Escape to return to the build list."),
        );
    }
}
