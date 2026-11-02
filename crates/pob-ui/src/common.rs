//! Globals of `Modules/Common.lua` and the drawing helpers of `Modules/Main.lua` that controls call (`main:DrawArrow`,
//! `main:DrawCheckMark`, `main:WrapString`), plus the `main` settings controls read (`main.showFlavourText`,
//! `main.notSupportedModTooltips`). `main` is a global table in Lua; here it is a thread-local [`MainFlags`] until
//! `Modules/Main.lua` is ported (U2b).

use crate::control::Rect;
use crate::sg;
use pob_gfx::{Frame, Gfx};
use std::cell::Cell;

/// `isMouseInRegion(region)` (Common.lua:55-58).
pub fn is_mouse_in_region(g: &Gfx, region: &Rect) -> bool {
    let (cursor_x, cursor_y) = g.get_cursor_pos();
    cursor_x >= region.x && cursor_x < region.x + region.width && cursor_y >= region.y && cursor_y < region.y + region.height
}

/// The `main` fields controls read. Defaults = `main:Init` (Main.lua:119, :122).
pub struct MainFlags {
    pub show_flavour_text: Cell<bool>,
    pub not_supported_mod_tooltips: Cell<bool>,
    /// `main.invertSliderScrollDirection` (`nil` until Settings.xml sets it, Main.lua:654).
    pub invert_slider_scroll_direction: Cell<bool>,
    /// `main.disableScrollControlInteraction` (`nil` by default).
    pub disable_scroll_control_interaction: Cell<bool>,
    /// `main.unicode` (`nil` by default; EditControl.lua:50 filter `%c` instead of `^%w%p `).
    pub unicode: Cell<bool>,
}

pub fn disable_scroll_control_interaction() -> bool {
    MAIN.with(|m| m.disable_scroll_control_interaction.get())
}

pub fn unicode() -> bool {
    MAIN.with(|m| m.unicode.get())
}

thread_local! {
    pub static MAIN: MainFlags = const {
        MainFlags {
            show_flavour_text: Cell::new(true),
            not_supported_mod_tooltips: Cell::new(true),
            invert_slider_scroll_direction: Cell::new(false),
            disable_scroll_control_interaction: Cell::new(false),
            unicode: Cell::new(false),
        }
    };
}

pub fn invert_slider_scroll_direction() -> bool {
    MAIN.with(|m| m.invert_slider_scroll_direction.get())
}

pub fn show_flavour_text() -> bool {
    MAIN.with(|m| m.show_flavour_text.get())
}

pub fn not_supported_mod_tooltips() -> bool {
    MAIN.with(|m| m.not_supported_mod_tooltips.get())
}

/// `main:DrawArrow(x, y, width, height, dir)` (Main.lua:1497-1513).
pub fn draw_arrow(f: &mut Frame<'_>, x: f64, y: f64, width: f64, height: f64, dir: &str) {
    let x1 = x - width / 2.0;
    let x2 = x + width / 2.0;
    let x_mid = (x1 + x2) / 2.0;
    let y1 = y - height / 2.0;
    let y2 = y + height / 2.0;
    let y_mid = (y1 + y2) / 2.0;
    match dir {
        "UP" => sg::draw_quad(f, [x_mid, y1, x_mid, y1, x2, y2, x1, y2]),
        "RIGHT" => sg::draw_quad(f, [x1, y1, x2, y_mid, x2, y_mid, x1, y2]),
        "DOWN" => sg::draw_quad(f, [x1, y1, x2, y1, x_mid, y2, x_mid, y2]),
        "LEFT" => sg::draw_quad(f, [x1, y_mid, x2, y1, x2, y2, x1, y_mid]),
        _ => {}
    }
}

/// `main:DrawCheckMark(x, y, size)` (Main.lua:1515-1521).
pub fn draw_check_mark(f: &mut Frame<'_>, x: f64, y: f64, size: f64) {
    let size = size / 0.8;
    let x = x - size / 2.0;
    let y = y - size / 2.0;
    sg::draw_quad(
        f,
        [
            x + size * 0.15,
            y + size * 0.50,
            x + size * 0.30,
            y + size * 0.45,
            x + size * 0.50,
            y + size * 0.80,
            x + size * 0.40,
            y + size * 0.90,
        ],
    );
    sg::draw_quad(
        f,
        [
            x + size * 0.40,
            y + size * 0.90,
            x + size * 0.35,
            y + size * 0.75,
            x + size * 0.80,
            y + size * 0.10,
            x + size * 0.90,
            y + size * 0.20,
        ],
    );
}

/// Lua `%s`: space, `\t`, `\n`, `\v`, `\f`, `\r`.
fn is_lua_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `str:find("%s+", init)` on bytes: 1-based inclusive `(s, e)`.
fn find_spaces(s: &[u8], init: usize) -> Option<(usize, usize)> {
    let start = init.max(1) - 1;
    let i = (start..s.len()).find(|&i| is_lua_space(s[i]))?;
    let mut e = i;
    while e + 1 < s.len() && is_lua_space(s[e + 1]) {
        e += 1;
    }
    Some((i + 1, e + 1))
}

/// Lua `str:sub(i, j)` for 1-based `i <= j` (both within `1..=len` or `j < i` → empty).
fn sub(s: &str, i: usize, j: usize) -> &str {
    let b = s.as_bytes();
    let j = j.min(b.len());
    if i == 0 || i > j {
        return "";
    }
    // Byte slicing like Lua; a split UTF-8 sequence is replaced rather than panicking.
    std::str::from_utf8(&b[i - 1..j]).unwrap_or("")
}

/// `main:WrapString(str, height, width)` (Main.lua:1797-1820), verbatim: the overflowing word stays on the line that
/// overflowed (`lastBreak` is updated before the check).
pub fn wrap_string(g: &Gfx, s: &str, height: f64, width: f64) -> Vec<String> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
    let mut line_start = 1;
    let mut last_space = 1;
    loop {
        let (st, e) = find_spaces(bytes, last_space).unwrap_or((len + 1, len + 1));
        if st > len {
            out.push(sub(s, line_start, len).to_string());
            break;
        }
        let last_break = st - 1;
        last_space = e + 1;
        if sg::draw_string_width(g, height, "VAR", sub(s, line_start, st - 1)) > width {
            out.push(sub(s, line_start, last_break).to_string());
            line_start = last_space;
        }
    }
    out
}
