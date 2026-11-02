//! Host state ([`Gfx`]), the per-frame drawing API ([`Frame`]) and the frame's command list ([`DrawList`]).
//!
//! Model of the original renderer (UI-ARCH §1.2, §3.2-§3.4, SG-DLL-FACTS §2): every draw call records quads that carry
//! the current `(layer, subLayer)`, a **snapshot of the viewport** (offset + clip rectangle) and the current tint; at the
//! end of the frame the list is stably sorted by `(layer, subLayer)` and drawn in one pass. Geometry is stored in
//! physical pixels (virtual coordinates × screen scale).

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use crate::color::{self, Rgba, WHITE};
use crate::image::{ImageHandle, Texture};
use crate::input::{Callback, InputState, KeyId, KeyNameError, RawEvent};
use crate::platform::{Platform, SystemPlatform};
use crate::text::{self, FontError, FontFamily, FontSet};

/// `DrawString` alignment (`nil` = `Left`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    CenterX,
    RightX,
}

impl Align {
    pub fn from_lua_name(name: Option<&str>) -> Option<Self> {
        Some(match name {
            None | Some("LEFT") => Self::Left,
            Some("CENTER") => Self::Center,
            Some("RIGHT") => Self::Right,
            Some("CENTER_X") => Self::CenterX,
            Some("RIGHT_X") => Self::RightX,
            Some(_) => return None,
        })
    }
}

/// Lua errors raised by the original for bad arguments (SG-DLL-FACTS §6; texts as measured where the oracle saw them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GfxError {
    LayerNeedsSubLayer,
    /// `SetDrawColor(str)` without a valid escape at the start (`"^"`, `" ^1"`, `"^a"`; trailing text after a valid escape
    /// is ignored: `"^1abc"`, `"^xFF80001"` — `fixtures/frame-probe/log.json`).
    InvalidColorEscape(String),
    NoImageLoaded,
}

impl fmt::Display for GfxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LayerNeedsSubLayer => write!(f, "SetDrawLayer(): must provide subLayer if layer is nil"),
            Self::InvalidColorEscape(_) => write!(f, "SetDrawColor() argument 1: invalid color escape sequence"),
            Self::NoImageLoaded => write!(f, "image handle has no image loaded"),
        }
    }
}

impl std::error::Error for GfxError {}

/// One textured quad.
#[derive(Clone, Debug)]
pub struct Command {
    pub layer: i32,
    pub sub_layer: i32,
    pub texture: Rc<Texture>,
    /// Vertices in physical pixels, viewport offset applied; order 1-2-3-4 (triangles 1-2-3, 1-3-4).
    pub pos: [[f32; 2]; 4],
    pub uv: [[f32; 2]; 4],
    pub tint: Rgba,
    /// Viewport clip rectangle in physical pixels: `x0, y0, x1, y1`; a fragment is kept when its pixel centre has
    /// `x0 <= x < x1` and `y0 < y <= y1` (the original's shader test in y-up NDC, SG-DLL-FACTS §2).
    pub clip: [f32; 4],
    /// The quad is a text glyph (used by the oracle diff to measure text pixels separately).
    pub glyph: bool,
}

impl PartialEq for Command {
    fn eq(&self, o: &Self) -> bool {
        self.layer == o.layer
            && self.sub_layer == o.sub_layer
            && self.texture.id() == o.texture.id()
            && self.pos == o.pos
            && self.uv == o.uv
            && self.tint == o.tint
            && self.clip == o.clip
            && self.glyph == o.glyph
    }
}

/// The commands of one frame. `Frame::finish` sorts it by `(layer, subLayer)`, stably (UI-ARCH §11 confirmed).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawList {
    pub commands: Vec<Command>,
    /// Physical size of the frame.
    pub size: (u32, u32),
    /// `SetClearColor` (default opaque black; the Lua UI never sets it).
    pub clear: Rgba,
}

impl DrawList {
    fn sort(&mut self) {
        self.commands.sort_by_key(|c| (c.layer, c.sub_layer));
    }
}

/// Window requests made by Lua-side calls, applied by the window loop after the callback returns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowRequests {
    /// `SetWindowTitle(title)`.
    pub title: Option<String>,
    /// `Exit([msg])`: leave the main loop after the current callback.
    pub exit: Option<Option<String>>,
}

/// Lines kept by `ConPrintf` (the original's console is an in-app overlay; here stderr + this ring).
const CONSOLE_LINES: usize = 1000;

/// Host state that lives between frames: fonts, screen, DPI, the current colour (it is **not** reset per frame,
/// UI-ARCH §11), input state, the platform (clock, clipboard, URLs) and pending window requests.
pub struct Gfx {
    fonts: FontSet,
    atlases: HashMap<(FontFamily, u32), Rc<Texture>>,
    white: Rc<Texture>,
    screen: (u32, u32),
    system_scale: f64,
    dpi_override: u32,
    color: Rgba,
    clear: Rgba,
    layer: i32,
    sub_layer: i32,
    /// Virtual `x, y, w, h`; `None` = whole screen.
    viewport: Option<[f64; 4]>,
    list: DrawList,
    platform: Box<dyn Platform>,
    input: InputState,
    requests: WindowRequests,
    /// `TakeScreenshot()` called since the last frame started: the **next** frame is captured.
    shot_requested: bool,
    /// The current frame is to be captured (set at `begin_frame` from `shot_requested`).
    shot_this_frame: bool,
    screenshot_dir: PathBuf,
    console: VecDeque<String>,
    cvars: HashMap<String, String>,
}

impl Gfx {
    /// `fonts_dir` = `runtime/SimpleGraphic/Fonts`; `screen` = physical frame size. `GetTime()` counts from now.
    pub fn new(fonts_dir: &Path, screen: (u32, u32), system_scale: f64) -> Result<Self, FontError> {
        Self::with_platform(fonts_dir, screen, system_scale, Box::new(SystemPlatform::new(Instant::now())))
    }

    /// Like [`Gfx::new`] with an explicit platform (clock, clipboard, URLs) — e.g. [`crate::platform::TestPlatform`].
    pub fn with_platform(fonts_dir: &Path, screen: (u32, u32), system_scale: f64, platform: Box<dyn Platform>) -> Result<Self, FontError> {
        Ok(Self {
            fonts: FontSet::load(fonts_dir)?,
            atlases: HashMap::new(),
            white: Rc::new(Texture::white()),
            screen,
            system_scale,
            dpi_override: 0,
            color: WHITE,
            clear: [0.0, 0.0, 0.0, 1.0],
            layer: 0,
            sub_layer: 0,
            viewport: None,
            list: DrawList::default(),
            platform,
            input: InputState::new(),
            requests: WindowRequests::default(),
            shot_requested: false,
            shot_this_frame: false,
            screenshot_dir: PathBuf::from("SimpleGraphic/Screenshots"),
            console: VecDeque::new(),
            cvars: HashMap::new(),
        })
    }

    pub fn platform_mut(&mut self) -> &mut dyn Platform {
        &mut *self.platform
    }

    // ----- input (U1.4, `input` module) -----

    /// Feeds one platform event; returns the callbacks to fire (see [`crate::input`]).
    pub fn input(&mut self, ev: RawEvent) -> Vec<Callback> {
        let now = self.get_time();
        self.input.translate(ev, now)
    }

    pub fn input_state(&self) -> &InputState {
        &self.input
    }

    /// `IsKeyDown(keyName)`.
    pub fn is_key_down(&self, name: &str) -> Result<bool, KeyNameError> {
        KeyId::parse(name).map(|k| self.input.is_down(k))
    }

    /// `GetCursorPos()`: the OS cursor in virtual coordinates, `physical / scale` rounded to the nearest integer
    /// (oracle `input-probe`: 640 / 1.5 -> 427, 680 / 1.5 -> 453) [H: halves].
    pub fn get_cursor_pos(&self) -> (f64, f64) {
        let s = self.get_screen_scale();
        let (x, y) = self.input.cursor();
        ((x / s).round(), (y / s).round())
    }

    // ----- platform -----

    /// `Copy(text)`.
    pub fn copy(&mut self, text: &str) {
        self.platform.copy(text);
    }

    /// `Paste()`; `None` = `nil`.
    pub fn paste(&mut self) -> Option<String> {
        self.platform.paste()
    }

    /// `OpenURL(url)`.
    pub fn open_url(&mut self, url: &str) {
        self.platform.open_url(url);
    }

    // ----- window -----

    /// `SetWindowTitle(title)`.
    pub fn set_window_title(&mut self, title: &str) {
        self.requests.title = Some(title.to_string());
    }

    /// `Exit([msg])`: after the current callback the loop calls `OnExit` (not `CanExit`) and ends (oracle traces:
    /// `Exit()` in `OnChar` -> `OnExit` logged next).
    pub fn exit(&mut self, msg: Option<&str>) {
        self.requests.exit = Some(msg.map(str::to_string));
    }

    /// Pending window requests, taken by the window loop.
    pub fn take_requests(&mut self) -> WindowRequests {
        std::mem::take(&mut self.requests)
    }

    pub fn exit_requested(&self) -> bool {
        self.requests.exit.is_some()
    }

    /// `RenderInit(flags…)`: the only call is `RenderInit("DPI_AWARE")` (Launch.lua:69); the window is always
    /// per-monitor DPI aware here, so it is accepted and has no effect.
    pub fn render_init(&mut self, _flags: &[&str]) {}

    /// `ConExecute(cmd)`: only `set <var> <value>` is used (Launch.lua:12-13: `vid_mode 8`, `vid_resizable 3`); the
    /// values are stored and read by the window setup ([`Gfx::cvar`]) [H: their meaning is not measured].
    pub fn con_execute(&mut self, cmd: &str) {
        let mut it = cmd.split_whitespace();
        if let (Some("set"), Some(var), Some(val)) = (it.next(), it.next(), it.next()) {
            self.cvars.insert(var.to_string(), val.to_string());
        }
    }

    pub fn cvar(&self, name: &str) -> Option<&str> {
        self.cvars.get(name).map(String::as_str)
    }

    /// `ConPrintf(fmt, …)` with the text already formatted by the caller (`pob_lua::format`): stderr + a ring buffer.
    pub fn con_printf(&mut self, text: &str) {
        eprintln!("{text}");
        if self.console.len() == CONSOLE_LINES {
            self.console.pop_front();
        }
        self.console.push_back(text.to_string());
    }

    /// `ConClear()`.
    pub fn con_clear(&mut self) {
        self.console.clear();
    }

    pub fn console(&self) -> impl Iterator<Item = &str> {
        self.console.iter().map(String::as_str)
    }

    /// `SetClearColor(r, g, b[, a])` (unused by the Lua UI).
    pub fn set_clear_color(&mut self, r: f64, g: f64, b: f64, a: f64) {
        self.clear = [r as f32, g as f32, b as f32, a as f32];
    }

    // ----- screenshots -----

    /// `TakeScreenshot()`: the **next** frame is written (oracle `frame-probe`), see [`crate::screenshot`].
    pub fn take_screenshot(&mut self) {
        self.shot_requested = true;
    }

    /// Directory `TakeScreenshot` writes to (default `SimpleGraphic/Screenshots`, relative to the working directory).
    pub fn set_screenshot_dir(&mut self, dir: impl Into<PathBuf>) {
        self.screenshot_dir = dir.into();
    }

    pub fn screenshot_dir(&self) -> &Path {
        &self.screenshot_dir
    }

    /// The frame just finished must be written as a screenshot.
    pub fn frame_wants_screenshot(&self) -> bool {
        self.shot_this_frame
    }

    pub fn fonts(&self) -> &FontSet {
        &self.fonts
    }

    pub fn set_screen(&mut self, size: (u32, u32), system_scale: f64) {
        self.screen = size;
        self.system_scale = system_scale;
    }

    /// `GetScreenSize()`: physical pixels.
    pub fn get_screen_size(&self) -> (u32, u32) {
        self.screen
    }

    /// `GetScreenScale()`: the DPI override when set, else the system scale.
    pub fn get_screen_scale(&self) -> f64 {
        if self.dpi_override == 0 { self.system_scale } else { f64::from(self.dpi_override) / 100.0 }
    }

    /// `GetVirtualScreenSize()` (Lua, Common.lua:1118-1126): `floor(w / scale), floor(h / scale)`.
    pub fn get_virtual_screen_size(&self) -> (f64, f64) {
        let s = self.get_screen_scale();
        ((f64::from(self.screen.0) / s).floor(), (f64::from(self.screen.1) / s).floor())
    }

    /// `SetDPIScaleOverridePercent(percent)`: 0 = system DPI.
    pub fn set_dpi_scale_override_percent(&mut self, percent: u32) {
        self.dpi_override = percent;
    }

    pub fn get_dpi_scale_override_percent(&self) -> u32 {
        self.dpi_override
    }

    /// `GetTime()`: integer milliseconds since start (from the platform clock).
    pub fn get_time(&self) -> u64 {
        self.platform.now_ms()
    }

    /// `GetDrawColor()`: the current tint (after escapes of the last `DrawString`).
    pub fn get_draw_color(&self) -> Rgba {
        self.color
    }

    pub fn draw_string_width(&self, height: f64, font: FontFamily, text: &str) -> i64 {
        self.fonts.draw_string_width(height, font, text)
    }

    pub fn draw_string_cursor_index(&self, height: f64, font: FontFamily, text: &str, x: f64, y: f64) -> usize {
        self.fonts.draw_string_cursor_index(height, font, text, x, y)
    }

    pub fn strip_escapes(&self, text: &str) -> String {
        text::strip_escapes(text)
    }

    pub fn new_image_handle(&self) -> ImageHandle {
        ImageHandle::new()
    }

    /// Starts a frame: layer/sub-layer `(0, 0)` (`GetDrawLayer()` = 0 at frame start, UI-ARCH §11) and the full-screen
    /// viewport (oracle `frame-probe`: frames ending with `SetViewport(100, 100, 50, 50)` start unclipped); the colour
    /// carries over. A `TakeScreenshot()` made before this point captures this frame.
    pub fn begin_frame(&mut self) -> Frame<'_> {
        self.layer = 0;
        self.sub_layer = 0;
        self.viewport = None;
        self.shot_this_frame = std::mem::take(&mut self.shot_requested);
        self.list.commands.clear();
        self.list.size = self.screen;
        self.list.clear = self.clear;
        Frame { gfx: self }
    }

    fn atlas(&mut self, font: FontFamily, height: u32) -> Rc<Texture> {
        if let Some(t) = self.atlases.get(&(font, height)) {
            return Rc::clone(t);
        }
        let img = self
            .fonts
            .font(font)
            .load_atlas(self.fonts.dir(), height)
            .unwrap_or_else(|e| panic!("font atlas {font:?} {height}: {e}"));
        let (w, h) = img.dimensions();
        // Atlases wrap like any texture loaded without `CLAMP`: a glyph at the atlas edge (Fontin SmallCaps 28 'd' at
        // x 0) filters against the opposite edge, not against itself (SG-oracle scene `u12-holdout`, scaled text).
        let t = Rc::new(Texture::new(w, h, img.into_raw(), false, false));
        self.atlases.insert((font, height), Rc::clone(&t));
        t
    }
}

/// Drawing is only possible during `OnFrame` ("called outside of OnFrame", SG-DLL-FACTS §6): the draw methods live on
/// this guard. Queries of [`Gfx`] are reachable through `Deref`.
pub struct Frame<'a> {
    gfx: &'a mut Gfx,
}

impl Deref for Frame<'_> {
    type Target = Gfx;
    fn deref(&self) -> &Gfx {
        self.gfx
    }
}

impl DerefMut for Frame<'_> {
    fn deref_mut(&mut self) -> &mut Gfx {
        self.gfx
    }
}

impl Frame<'_> {
    /// `SetDrawLayer(layer|nil[, subLayer])`: a given layer without sub-layer means sub-layer 0.
    pub fn set_draw_layer(&mut self, layer: Option<i32>, sub_layer: Option<i32>) -> Result<(), GfxError> {
        match (layer, sub_layer) {
            (None, None) => return Err(GfxError::LayerNeedsSubLayer),
            (Some(l), s) => {
                self.gfx.layer = l;
                self.gfx.sub_layer = s.unwrap_or(0);
            }
            (None, Some(s)) => self.gfx.sub_layer = s,
        }
        Ok(())
    }

    pub fn get_draw_layer(&self) -> (i32, i32) {
        (self.gfx.layer, self.gfx.sub_layer)
    }

    /// `SetViewport([x, y, w, h])`: offset + clip for the following commands; `None` = whole screen. Not nested.
    pub fn set_viewport(&mut self, rect: Option<[f64; 4]>) {
        self.gfx.viewport = rect;
    }

    /// `SetDrawColor(r, g, b[, a])`.
    pub fn set_draw_color(&mut self, r: f64, g: f64, b: f64, a: f64) {
        self.gfx.color = [r as f32, g as f32, b as f32, a as f32];
    }

    /// `SetDrawColor(escapeStr)`: `^N` / `^xRRGGBB` at the start of the string, alpha 1; on error the colour is kept.
    pub fn set_draw_color_escape(&mut self, s: &str) -> Result<(), GfxError> {
        self.gfx.color = color::parse_escape(s).ok_or_else(|| GfxError::InvalidColorEscape(s.to_string()))?;
        Ok(())
    }

    // No `SetBlendMode`: SimpleGraphic v2.5.5 (PoB 620ffe6da) removed it from the Lua API (scene `u13-blend`: every call is
    // "attempt to call a nil value", the squares are drawn with the default ALPHA blending); the Lua UI never called it.

    /// Viewport offset and clip rectangle in physical pixels. The original works with whole-pixel viewports:
    /// `x`, `y` are rounded (half away from zero), `w`, `h` rounded **up** — 12 fractional viewports of the SG-oracle
    /// scenes `viewport` and `u12-frac` (e.g. 130.25 + 60.25 -> columns 130..=190, 50.5 + 100.5 -> 51..=151,
    /// width 0.4 -> 1 column) [H: negative values and the order rounding / DPI scaling are not measured].
    fn viewport_phys(&self) -> ([f32; 2], [f32; 4]) {
        let s = self.gfx.get_screen_scale();
        match self.gfx.viewport {
            None => ([0.0, 0.0], [0.0, 0.0, self.gfx.screen.0 as f32, self.gfx.screen.1 as f32]),
            Some([x, y, w, h]) => {
                let (x, y, w, h) = ((x * s).round(), (y * s).round(), (w * s).ceil(), (h * s).ceil());
                ([x as f32, y as f32], [x as f32, y as f32, (x + w) as f32, (y + h) as f32])
            }
        }
    }

    /// Records one quad; `pos` in virtual coordinates relative to the viewport.
    fn push(&mut self, texture: Rc<Texture>, pos: [[f64; 2]; 4], uv: [[f64; 2]; 4], glyph: bool) {
        let s = self.gfx.get_screen_scale();
        let (off, clip) = self.viewport_phys();
        let pos = pos.map(|[x, y]| [(x * s) as f32 + off[0], (y * s) as f32 + off[1]]);
        self.push_phys(texture, pos, uv.map(|[u, v]| [u as f32, v as f32]), clip, glyph);
    }

    fn push_phys(&mut self, texture: Rc<Texture>, pos: [[f32; 2]; 4], uv: [[f32; 2]; 4], clip: [f32; 4], glyph: bool) {
        let cmd = Command {
            layer: self.gfx.layer,
            sub_layer: self.gfx.sub_layer,
            texture,
            pos,
            uv,
            tint: self.gfx.color,
            clip,
            glyph,
        };
        self.gfx.list.commands.push(cmd);
    }

    /// The texture to draw for `img` (`None` = solid colour). An unloaded `ASYNC` image draws nothing [H].
    fn image_texture(&self, img: Option<&ImageHandle>) -> Result<Option<Rc<Texture>>, GfxError> {
        match img {
            None => Ok(Some(Rc::clone(&self.gfx.white))),
            Some(h) if !h.has_load() => Err(GfxError::NoImageLoaded),
            Some(h) => Ok(h.texture()),
        }
    }

    /// `DrawImage(img|nil, left, top, width, height[, tcLeft, tcTop, tcRight, tcBottom])`.
    pub fn draw_image(
        &mut self,
        img: Option<&ImageHandle>,
        left: f64,
        top: f64,
        width: f64,
        height: f64,
        tc: Option<[f64; 4]>,
    ) -> Result<(), GfxError> {
        let Some(tex) = self.image_texture(img)? else { return Ok(()) };
        let (r, b) = (left + width, top + height);
        let [tl, tt, tr, tb] = tc.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        self.push(tex, [[left, top], [r, top], [r, b], [left, b]], [[tl, tt], [tr, tt], [tr, tb], [tl, tb]], false);
        Ok(())
    }

    /// `DrawImageQuad(img|nil, x1, y1, …, x4, y4[, s1, t1, …, s4, t4])`.
    pub fn draw_image_quad(
        &mut self,
        img: Option<&ImageHandle>,
        xy: [f64; 8],
        st: Option<[f64; 8]>,
    ) -> Result<(), GfxError> {
        let Some(tex) = self.image_texture(img)? else { return Ok(()) };
        let st = st.unwrap_or([0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]);
        let p = |k: usize| [xy[2 * k], xy[2 * k + 1]];
        let t = |k: usize| [st[2 * k], st[2 * k + 1]];
        self.push(tex, [p(0), p(1), p(2), p(3)], [t(0), t(1), t(2), t(3)], false);
        Ok(())
    }

    /// `DrawString(left, top, align, height, font, text)`: one quad per glyph from the U1.1 atlases.
    ///
    /// Rules (SG oracle, UI-ARCH §11 + U1.1 `text/mod.rs`): each `\n`-separated line is aligned on its own, `CENTER` /
    /// `RIGHT` against the **screen width** (not the viewport); escapes change the tint of the following glyphs and stay
    /// the current colour after the call (alpha reset to 1); advances are the integer `ceil(adv·H/A)` of U1.1. At a DPI
    /// scale the layout happens in physical pixels at height `height·scale` (scenes `dpi150`, `u12-dpi125`).
    pub fn draw_string(&mut self, left: f64, top: f64, align: Align, height: f64, font: FontFamily, text: &str) {
        let s = self.gfx.get_screen_scale();
        let (off, clip) = self.viewport_phys();
        let text = text::until_nul(text);
        let eff = text::effective_height(height * s);
        let (main, sub) = {
            let f = self.gfx.fonts.font(font);
            (f.atlas_index(eff), f.placeholder_atlas_index(eff))
        };
        let (main_h, sub_h) = {
            let h = self.gfx.fonts.font(font).heights();
            (h[main].height, h[sub].height)
        };
        let main_tex = self.gfx.atlas(font, main_h);
        let sub_tex = if sub == main { Rc::clone(&main_tex) } else { self.gfx.atlas(font, sub_h) };
        let screen_w = f64::from(self.gfx.screen.0);
        let (left, top) = (left * s, top * s);
        let Gfx {
            fonts,
            list,
            color,
            layer,
            sub_layer,
            ..
        } = &mut *self.gfx;
        let f = fonts.font(font);
        let ctx = LineCtx {
            font: f,
            main: &f.heights()[main],
            sub: &f.heights()[sub],
            eff,
            main_tex: &main_tex,
            sub_tex: &sub_tex,
            off,
            clip,
            layer: *layer,
            sub_layer: *sub_layer,
        };
        // Lines start on whole pixels (scenes `fractional`, `u12-frac`, `align-viewport`): `LEFT` rounds `left` half away
        // from zero (40.5 -> 41, 60.75 -> 61, -3.5 -> -4, 10.25 -> 10), every other alignment floors its result
        // (200.5 - 30 -> 170, 200 - 31/2 -> 184, 200 - 41/2 -> 179), `top` floors (30.75 -> 30, 10.5 -> 10). Glyphs
        // inside a line keep their fractional scaled offsets (`text-heights`). [H: negative `top`, floor vs truncation.]
        let mut line_top = top.floor();
        for line in text.split('\n') {
            let x = if align == Align::Left {
                left.round()
            } else {
                let w = f.string_width(f64::from(eff), line) as f64;
                match align {
                    Align::Center => ((screen_w - w) / 2.0 + left).floor(),
                    Align::Right => (screen_w - w - left).floor(),
                    Align::CenterX => (left - w / 2.0).floor(),
                    Align::RightX | Align::Left => (left - w).floor(),
                }
            };
            ctx.layout(line, x, line_top, color, &mut list.commands);
            line_top += f64::from(eff);
        }
    }

    /// Ends the frame: the command list stably sorted by `(layer, subLayer)`.
    pub fn finish(self) -> DrawList {
        self.gfx.list.sort();
        std::mem::take(&mut self.gfx.list)
    }
}

/// Everything one `DrawString` line needs; borrows the font while the command list is pushed to.
struct LineCtx<'a> {
    font: &'a text::Font,
    main: &'a text::FontHeight,
    sub: &'a text::FontHeight,
    eff: u32,
    main_tex: &'a Rc<Texture>,
    sub_tex: &'a Rc<Texture>,
    off: [f32; 2],
    clip: [f32; 4],
    layer: i32,
    sub_layer: i32,
}

impl LineCtx<'_> {
    /// One glyph quad: `rect` = x0, y0, x1, y1 in physical pixels (viewport offset added here), `uv` in atlas texels.
    fn quad(&self, out: &mut Vec<Command>, tex: &Rc<Texture>, rect: [f64; 4], uv: [f64; 4], tint: Rgba) {
        let (tw, th) = (f64::from(tex.width), f64::from(tex.height));
        let [x0, y0, x1, y1] = rect.map(|v| v as f32);
        let [ox, oy] = self.off;
        let pos = [[x0 + ox, y0 + oy], [x1 + ox, y0 + oy], [x1 + ox, y1 + oy], [x0 + ox, y1 + oy]];
        let (u0, v0, u1, v1) = ((uv[0] / tw) as f32, (uv[1] / th) as f32, (uv[2] / tw) as f32, (uv[3] / th) as f32);
        out.push(Command {
            layer: self.layer,
            sub_layer: self.sub_layer,
            texture: Rc::clone(tex),
            pos,
            uv: [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
            tint,
            clip: self.clip,
            glyph: true,
        });
    }

    /// Lays out one line (no `\n`) starting at pen `x`, top `y`; escapes update `color` as they are met.
    ///
    /// The pen of a drawn line is **not** the integer advance sum of `DrawStringWidth` (U1.1): the original walks the
    /// line in `f32` — glyph left `gx = pen + spLeft·s`, then `pen = ceil(gx + width·s + spRight·s)` with
    /// `s = H / atlas` — so a scaled line drifts by +1 px wherever the `f32` sum lands just above an integer
    /// (SG-oracle scenes `u13-placeholder` / `u13-h30probe`: VAR h 30, `"hhhhhhhh|"` glyphs at +0,0,0,0,0,+1,+2,+3,+3
    /// against the integer model; this rule fits all 17 probe lines, the integer and the `f64` variants do not).
    /// Unscaled lines (`s = 1`) are exact integers either way.
    fn layout(&self, line: &str, x: f64, y: f64, color: &mut Rgba, out: &mut Vec<Command>) {
        let bytes = line.as_bytes();
        let (fh, sh) = (self.main, self.sub);
        let eff = f64::from(self.eff);
        let (eff_i, atlas_i) = (i64::from(self.eff), i64::from(fh.height));
        let s32 = self.eff as f32 / fh.height as f32;
        let mut pen = x as f32;
        let mut i = 0;
        while i < bytes.len() {
            if let Some((len, c)) = color::escape_color(bytes, i) {
                *color = c;
                i += len;
                continue;
            }
            let b = bytes[i];
            if b == b'\t' {
                // [H] the tab keeps the width rule (not covered by a drawn scene).
                pen += text::scaled_advance(4 * fh.glyphs[usize::from(b' ')].advance(), eff_i, atlas_i) as f32;
                i += 1;
            } else if b.is_ascii() {
                let g = &fh.glyphs[usize::from(b)];
                let gx = pen + g.sp_left as f32 * s32;
                let gx1 = gx + g.width as f32 * s32;
                let rect = [f64::from(gx), y, f64::from(gx1), y + eff];
                let uv = [f64::from(g.x), f64::from(g.y), f64::from(g.x + g.width), f64::from(g.y + fh.height)];
                self.quad(out, self.main_tex, rect, uv, *color);
                pen = (gx1 + g.sp_right as f32 * s32).ceil();
                i += 1;
            } else {
                let ch = line[i..].chars().next().expect("char boundary");
                i += ch.len_utf8();
                // `[U+XXXX]` with the smaller atlas, unscaled, centred vertically (scene `escape-carry`: H 16, atlas 12 ->
                // 2 px down; `u12-frac`: H 12/20/24/32). A placeholder atlas taller than the line is not moved up: offset
                // 0 for H 1 / atlas 10 and H 6 / atlas 10 (scene `u13-placeholder`). An odd difference cannot occur: H > 1
                // and every atlas height are even.
                let a = f64::from(sh.height);
                let py = y + ((eff - a) / 2.0).floor().max(0.0);
                let mut p = f64::from(pen);
                for pb in text::placeholder(ch).bytes() {
                    let g = &sh.glyphs[usize::from(pb)];
                    let gx = p + f64::from(g.sp_left);
                    let rect = [gx, py, gx + f64::from(g.width), py + a];
                    let uv = [f64::from(g.x), f64::from(g.y), f64::from(g.x + g.width), f64::from(g.y + sh.height)];
                    self.quad(out, self.sub_tex, rect, uv, *color);
                    p += g.advance() as f64;
                }
                pen += self.font.char_advance(eff, ch) as f32;
            }
        }
    }
}
