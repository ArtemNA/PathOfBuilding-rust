//! Lua-shaped wrappers over the `pob-gfx` host so that ported code reads like the original: `DrawImage(nil, …)` →
//! `draw_rect(f, …)`, `SetDrawColor(r, g, b)` → `set_draw_color(f, r, g, b)`, font/align by their Lua names. Lua raises an
//! error on a bad argument; the port panics with the same message (UI-ARCH §4.5: Launch.lua shows it as a prompt).

use pob_gfx::text::FontFamily;
use pob_gfx::{Align, Frame, Gfx, ImageHandle};

pub fn font(name: &str) -> FontFamily {
    FontFamily::from_lua_name(name).unwrap_or_else(|| panic!("invalid font name '{name}'"))
}

pub fn align(name: Option<&str>) -> Align {
    Align::from_lua_name(name).unwrap_or_else(|| panic!("invalid alignment '{name:?}'"))
}

/// `SetDrawColor(r, g, b)` (alpha 1).
pub fn set_draw_color(f: &mut Frame<'_>, r: f64, g: f64, b: f64) {
    f.set_draw_color(r, g, b, 1.0);
}

/// `SetDrawColor(r, g, b, a)`.
pub fn set_draw_color_a(f: &mut Frame<'_>, r: f64, g: f64, b: f64, a: f64) {
    f.set_draw_color(r, g, b, a);
}

/// `SetDrawColor(escapeString)`.
pub fn set_draw_color_str(f: &mut Frame<'_>, s: &str) {
    f.set_draw_color_escape(s).unwrap_or_else(|e| panic!("{e}"));
}

/// `SetDrawLayer(layer|nil, subLayer)`.
pub fn set_draw_layer(f: &mut Frame<'_>, layer: Option<i32>, sub_layer: i32) {
    f.set_draw_layer(layer, Some(sub_layer)).unwrap_or_else(|e| panic!("{e}"));
}

/// `SetViewport(x, y, w, h)` / `SetViewport()`.
pub fn set_viewport(f: &mut Frame<'_>, rect: Option<[f64; 4]>) {
    f.set_viewport(rect);
}

/// `DrawImage(nil, left, top, width, height)`.
pub fn draw_rect(f: &mut Frame<'_>, left: f64, top: f64, width: f64, height: f64) {
    f.draw_image(None, left, top, width, height, None).unwrap_or_else(|e| panic!("{e}"));
}

/// `DrawImage(img, left, top, width, height[, tc…])`.
pub fn draw_image(f: &mut Frame<'_>, img: Option<&ImageHandle>, left: f64, top: f64, width: f64, height: f64, tc: Option<[f64; 4]>) {
    f.draw_image(img, left, top, width, height, tc).unwrap_or_else(|e| panic!("{e}"));
}

/// `DrawImageQuad(nil, x1, y1, …, x4, y4)`.
pub fn draw_quad(f: &mut Frame<'_>, xy: [f64; 8]) {
    f.draw_image_quad(None, xy, None).unwrap_or_else(|e| panic!("{e}"));
}

/// `DrawString(left, top, align, height, font, text)`.
pub fn draw_string(f: &mut Frame<'_>, left: f64, top: f64, align_name: Option<&str>, height: f64, font_name: &str, text: &str) {
    f.draw_string(left, top, align(align_name), height, font(font_name), text);
}

/// `DrawStringWidth(height, font, text)` (an integer in the original; Lua numbers are doubles).
pub fn draw_string_width(g: &Gfx, height: f64, font_name: &str, text: &str) -> f64 {
    g.draw_string_width(height, font(font_name), text) as f64
}

/// `DrawStringCursorIndex(height, font, text, x, y)`: 1-based byte index.
pub fn draw_string_cursor_index(g: &Gfx, height: f64, font_name: &str, text: &str, x: f64, y: f64) -> usize {
    g.draw_string_cursor_index(height, font(font_name), text, x, y)
}

/// `IsKeyDown(name)`.
pub fn is_key_down(g: &Gfx, name: &str) -> bool {
    g.is_key_down(name).unwrap_or_else(|e| panic!("{e}"))
}
