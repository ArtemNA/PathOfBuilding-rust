//! Input of the SimpleGraphic host (UI-ARCH §2.5, §3.8): key names, `OnKeyDown(key, doubleClick)` / `OnKeyUp(key)` /
//! `OnChar(ch)`, `IsKeyDown`, `GetCursorPos`.
//!
//! Platform-neutral: the window loop (or a test) feeds [`RawEvent`]s — what the window system delivered — into
//! [`InputState::translate`], which updates the key/cursor state and returns the callbacks the original would fire, in
//! order. Every rule below was measured with the SG oracle (`tools/sg-oracle`, posted input only: `lua/measure.lua` +
//! `inject.ps1`, `lua/input-probe.lua` + `inject-probe.ps1` / `inject-chars.ps1` / `inject-f10.ps1`) and is replayed
//! by `tests/input.rs`:
//!
//! - **Key names** come from the *physical* key (US positions, layout-independent: GLFW maps keys by scancode): letters
//!   lower-case, digits and punctuation as their character, **`=` → `"+"`**, space → `" "`; numpad `0` → `"0"`, numpad
//!   `+`/`-` → `"+"`/`"-"`, numpad Enter → `"RETURN"`; other numpad keys, Caps Lock, Win and Menu have no name (no
//!   callback). Left/right Shift/Ctrl/Alt → `SHIFT`/`CTRL`/`ALT`; `XBUTTON1/2` → `MOUSE4`/`MOUSE5`.
//! - **`OnChar`** after `OnKeyDown`: `"\b"`, `"\r"`, `"\t"`, `"\x1b"` are synthesised from Backspace / Enter / Tab /
//!   Escape (not from numpad Enter); typed text arrives as [`RawEvent::Char`] and is converted like the original:
//!   control codes (< 32, 127) and U+0080..U+009F are dropped, ASCII passes, **U+00A0..U+00AA give the key names
//!   `"F7"`…`"F15"`, `"NUMLOCK"`, `"SCROLLLOCK"`**, every higher code point gives `"?"`; a UTF-16 surrogate pair is one
//!   code point (one `"?"`). SimpleGraphic v2.5.5 (PoB 620ffe6da, `input-probe-chars.json`: 1440 code points; GLFW 3.4's
//!   `WM_CHAR` handling: the high surrogate is held and combined with the next low one, a lone low surrogate is code
//!   point 0, `_glfwInputChar` drops < 32 and 127..159). The v2.5-3b1a346 DLL before it used a Windows-1252 best-fit
//!   table (`ā` → `"a"`, `＋` → `"+"`), gave `"?"` for most of U+0080..U+009F and one `"?"` per surrogate.
//! - **Wheel**: every non-zero vertical message → `OnKeyDown` + `OnKeyUp` of `WHEELUP`/`WHEELDOWN` (no accumulation);
//!   horizontal wheel → nothing.
//! - **Repeats**: an auto-repeated key press fires `OnKeyDown` (and its `OnChar`) again.
//! - **`F10`**: `OnKeyUp` is never fired (4/4 posted releases, `WM_KEYUP` and `WM_SYSKEYUP`).
//! - **`PRINTSCREEN`**: each press *or* release message fires the pair `OnKeyDown` + `OnKeyUp` (GLFW's Print Screen
//!   hack; a real key usually delivers only the release).
//! - A key release of a key that is not down fires nothing; a **mouse button** release always fires `OnKeyUp` (posted
//!   `WM_LBUTTONUP` without a press did).
//! - **`IsKeyDown`** sees the state *after* the event (`CTRL` is true in `OnKeyDown("CTRL")`, false in its `OnKeyUp`);
//!   names: the table below, any single printable ASCII character (letters case-insensitive); `""` and unknown names are
//!   errors with the original's messages.
//! - **`GetCursorPos`** = the OS cursor `/ scale`, rounded to the nearest integer (DPI 150 %: 1680, 680 → 1120, 453;
//!   640, 400 → 427, 267), not clamped to the window.
//! - **Double-click** (`OnKeyDown(button, true)`): the Windows rule — same button again within [`DOUBLE_CLICK_MS`] and
//!   inside a [`DOUBLE_CLICK_RECT`] around the first click's message position; the click after a double-click starts
//!   over — but **only while the OS cursor is over the window**: with the idle cursor outside the client (window-mode
//!   runs) posted clicks never double; with it inside (`measure-raw-desktop`, `input-probe-inside`) they do: 80, 300 and
//!   10 ms apart → `true`, 650 ms or 10 px apart → `false`, triple click → `false, true, false`.

use std::fmt;

/// Named keys of the original, in the order of the binary's table (SG-DLL-FACTS §5).
pub const KEY_NAMES: [&str; 43] = [
    "LEFTBUTTON",
    "MIDDLEBUTTON",
    "RIGHTBUTTON",
    "MOUSE4",
    "MOUSE5",
    "WHEELUP",
    "WHEELDOWN",
    "BACK",
    "TAB",
    "RETURN",
    "ESCAPE",
    "SHIFT",
    "CTRL",
    "ALT",
    "PAUSE",
    "PAGEUP",
    "PAGEDOWN",
    "END",
    "HOME",
    "PRINTSCREEN",
    "INSERT",
    "DELETE",
    "UP",
    "DOWN",
    "LEFT",
    "RIGHT",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "F13",
    "F14",
    "F15",
    "NUMLOCK",
    "SCROLLLOCK",
];

/// `GetDoubleClickTime()` on the measuring machine (inject logs: `dblclick=500`) [H: used for real clicks only].
pub const DOUBLE_CLICK_MS: u64 = 500;
/// `SM_CXDOUBLECLK` x `SM_CYDOUBLECLK` on the measuring machine (`dblrect=4x4`), physical pixels, centred on the first click.
pub const DOUBLE_CLICK_RECT: (f64, f64) = (4.0, 4.0);

/// Printable ASCII 32..=126, sliced into the single-character key names / `OnChar` strings.
const PRINTABLE: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

fn ascii_str(b: u8) -> &'static str {
    debug_assert!((32..127).contains(&b));
    let i = usize::from(b - 32);
    &PRINTABLE[i..=i]
}

/// A key or mouse button as the original names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyId {
    /// Index into [`KEY_NAMES`].
    Named(u8),
    /// A printable ASCII key, letters lower-case.
    Char(u8),
}

impl KeyId {
    const fn named(i: usize) -> Self {
        Self::Named(i as u8)
    }

    pub const LEFTBUTTON: Self = Self::named(0);
    pub const RIGHTBUTTON: Self = Self::named(2);
    pub const WHEELUP: Self = Self::named(5);
    pub const WHEELDOWN: Self = Self::named(6);
    pub const RETURN: Self = Self::named(9);
    pub const PRINTSCREEN: Self = Self::named(19);
    pub const F1: Self = Self::named(26);

    /// The name passed to `OnKeyDown` / `OnKeyUp`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Named(i) => KEY_NAMES[usize::from(i)],
            Self::Char(b) => ascii_str(b),
        }
    }

    /// `F1`…`F15`.
    pub fn function(n: u8) -> Option<Self> {
        (1..=15).contains(&n).then(|| Self::Named(26 + n - 1))
    }

    /// Parses an `IsKeyDown` argument (case-insensitive for letters, like the original: `IsKeyDown("A")` is the `a` key).
    pub fn parse(name: &str) -> Result<Self, KeyNameError> {
        if name.is_empty() {
            return Err(KeyNameError::Empty);
        }
        if let Some(i) = KEY_NAMES.iter().position(|&n| n == name) {
            return Ok(Self::named(i));
        }
        match name.as_bytes() {
            [b @ 32..=126] => Ok(Self::Char(b.to_ascii_lowercase())),
            _ => Err(KeyNameError::Unrecognised),
        }
    }
}

/// `IsKeyDown()` errors (messages as the original's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyNameError {
    Empty,
    Unrecognised,
}

impl fmt::Display for KeyNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "IsKeyDown() argument 1: string is empty"),
            Self::Unrecognised => write!(f, "IsKeyDown(): unrecognised key name"),
        }
    }
}

impl std::error::Error for KeyNameError {}

/// A physical keyboard key, as the platform reports it (US positions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysKey {
    /// Printable key at its US position: `a`…`z` (lower-case), `0`…`9`, `` ` - = [ ] \ ; ' , . / `` and space.
    Char(u8),
    Backspace,
    Tab,
    Enter,
    Escape,
    Shift,
    Ctrl,
    Alt,
    Pause,
    PageUp,
    PageDown,
    End,
    Home,
    PrintScreen,
    Insert,
    Delete,
    Up,
    Down,
    Left,
    Right,
    /// `F1`…`F24`; only `F1`…`F15` have names.
    F(u8),
    NumLock,
    ScrollLock,
    Numpad0,
    NumpadAdd,
    NumpadSubtract,
    NumpadEnter,
    /// Any other key (numpad 1-9 . * /, Caps Lock, Win, Menu…): no name, no callback.
    Other,
}

impl PhysKey {
    /// The original's name of this key, if it has one.
    pub fn key_id(self) -> Option<KeyId> {
        let n = KeyId::named;
        Some(match self {
            Self::Char(b'=') => KeyId::Char(b'+'),
            Self::Char(b @ 32..=126) => KeyId::Char(b.to_ascii_lowercase()),
            Self::Char(_) | Self::Other => return None,
            Self::Backspace => n(7),
            Self::Tab => n(8),
            Self::Enter | Self::NumpadEnter => n(9),
            Self::Escape => n(10),
            Self::Shift => n(11),
            Self::Ctrl => n(12),
            Self::Alt => n(13),
            Self::Pause => n(14),
            Self::PageUp => n(15),
            Self::PageDown => n(16),
            Self::End => n(17),
            Self::Home => n(18),
            Self::PrintScreen => n(19),
            Self::Insert => n(20),
            Self::Delete => n(21),
            Self::Up => n(22),
            Self::Down => n(23),
            Self::Left => n(24),
            Self::Right => n(25),
            Self::F(k) => return KeyId::function(k),
            Self::NumLock => n(41),
            Self::ScrollLock => n(42),
            Self::Numpad0 => KeyId::Char(b'0'),
            Self::NumpadAdd => KeyId::Char(b'+'),
            Self::NumpadSubtract => KeyId::Char(b'-'),
        })
    }

    /// `OnChar` the original synthesises after `OnKeyDown` of this key.
    fn synthesized_char(self) -> Option<&'static str> {
        match self {
            Self::Backspace => Some("\u{8}"),
            Self::Enter => Some("\r"),
            Self::Tab => Some("\t"),
            Self::Escape => Some("\u{1b}"),
            _ => None,
        }
    }
}

/// Mouse buttons with names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
    Mouse4,
    Mouse5,
}

impl Button {
    pub fn key_id(self) -> KeyId {
        KeyId::named(self as usize)
    }
}

/// What the window system delivered (one message).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RawEvent {
    /// Key press (`pressed`, also auto-repeat) or release.
    Key { key: PhysKey, pressed: bool },
    /// A typed character as a UTF-16 code unit or a code point (the platform's `WM_CHAR`).
    Char(u32),
    /// A mouse button message; `pos` = its client position (physical px), `None` = the current cursor.
    Button {
        button: Button,
        pressed: bool,
        pos: Option<(f64, f64)>,
    },
    /// Vertical wheel delta (sign only matters); horizontal scrolling is not an event.
    Wheel(f64),
    /// OS cursor position in physical client pixels.
    Cursor { x: f64, y: f64 },
    /// The OS cursor entered (`true`) or left the window's client area.
    CursorInside(bool),
    /// The window lost the input focus: GLFW releases every held key and button, so the original fires their
    /// `OnKeyUp` [H: from GLFW's `_glfwInputWindowFocus`, not measured].
    FocusLost,
}

/// A callback of the main object, in firing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callback {
    KeyDown(&'static str, bool),
    KeyUp(&'static str),
    Char(&'static str),
}

/// `OnChar` text for a typed code point (rules in the module docs).
pub fn char_text(cp: u32) -> Option<&'static str> {
    let byte = match cp {
        0..=0x1F | 0x7F..=0x9F => return None,
        0x20..=0xFF => cp as u8,
        _ => return Some("?"),
    };
    Some(match byte {
        32..=126 => ascii_str(byte),
        // The original's key table: bytes 0xA0..0xAA name F7..F15, NUMLOCK, SCROLLLOCK.
        0xA0..=0xAA => KEY_NAMES[32 + usize::from(byte - 0xA0)],
        _ => "?",
    })
}

/// Keys/buttons held, the OS cursor and the double-click tracker.
#[derive(Clone, Debug, Default)]
pub struct InputState {
    down: Vec<KeyId>,
    /// Physical client pixels.
    cursor: (f64, f64),
    /// The OS cursor is over the client area.
    inside: bool,
    last_click: Option<(Button, u64, (f64, f64))>,
    /// A `WM_CHAR` high surrogate waiting for its low half (GLFW 3.4 `highSurrogate`).
    high_surrogate: Option<u32>,
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_down(&self, key: KeyId) -> bool {
        self.down.contains(&key)
    }

    /// The OS cursor in physical client pixels.
    pub fn cursor(&self) -> (f64, f64) {
        self.cursor
    }

    fn press(&mut self, k: KeyId) {
        if !self.down.contains(&k) {
            self.down.push(k);
        }
    }

    /// Releases `k`; `false` if it was not down.
    fn release(&mut self, k: KeyId) -> bool {
        let before = self.down.len();
        self.down.retain(|&d| d != k);
        self.down.len() != before
    }

    fn double_click(&mut self, button: Button, now_ms: u64, pos: (f64, f64)) -> bool {
        if !self.inside {
            self.last_click = None;
            return false;
        }
        if let Some((b, t, p)) = self.last_click
            && b == button
            && now_ms.saturating_sub(t) <= DOUBLE_CLICK_MS
            && (pos.0 - p.0).abs() <= DOUBLE_CLICK_RECT.0 / 2.0
            && (pos.1 - p.1).abs() <= DOUBLE_CLICK_RECT.1 / 2.0
        {
            self.last_click = None;
            return true;
        }
        self.last_click = Some((button, now_ms, pos));
        false
    }

    /// Applies one platform event at time `now_ms` (`GetTime`) and returns the callbacks to fire, in order. The state
    /// is already updated when they fire.
    pub fn translate(&mut self, ev: RawEvent, now_ms: u64) -> Vec<Callback> {
        let mut out = Vec::new();
        match ev {
            RawEvent::Key {
                key: PhysKey::PrintScreen, ..
            } => {
                out.push(Callback::KeyDown(KeyId::PRINTSCREEN.name(), false));
                out.push(Callback::KeyUp(KeyId::PRINTSCREEN.name()));
            }
            RawEvent::Key { key, pressed: true } => {
                if let Some(k) = key.key_id() {
                    self.press(k);
                    out.push(Callback::KeyDown(k.name(), false));
                    if let Some(c) = key.synthesized_char() {
                        out.push(Callback::Char(c));
                    }
                }
            }
            RawEvent::Key { key, pressed: false } => {
                if let Some(k) = key.key_id()
                    && self.release(k)
                    && key != PhysKey::F(10)
                {
                    out.push(Callback::KeyUp(k.name()));
                }
            }
            RawEvent::Char(unit @ 0xD800..=0xDBFF) => self.high_surrogate = Some(unit),
            RawEvent::Char(unit) => {
                let cp = match (unit, self.high_surrogate.take()) {
                    (0xDC00..=0xDFFF, Some(hi)) => 0x10000 + ((hi - 0xD800) << 10) + (unit - 0xDC00),
                    // [H: GLFW 3.4 source, not measured] a lone low surrogate is code point 0 (dropped).
                    (0xDC00..=0xDFFF, None) => 0,
                    _ => unit,
                };
                out.extend(char_text(cp).map(Callback::Char));
            }
            RawEvent::Button { button, pressed, pos } => {
                let k = button.key_id();
                if pressed {
                    self.press(k);
                    let dbl = self.double_click(button, now_ms, pos.unwrap_or(self.cursor));
                    out.push(Callback::KeyDown(k.name(), dbl));
                } else {
                    self.release(k);
                    out.push(Callback::KeyUp(k.name()));
                }
            }
            RawEvent::Wheel(dy) => {
                let k = if dy > 0.0 {
                    KeyId::WHEELUP
                } else if dy < 0.0 {
                    KeyId::WHEELDOWN
                } else {
                    return out;
                };
                out.push(Callback::KeyDown(k.name(), false));
                out.push(Callback::KeyUp(k.name()));
            }
            RawEvent::Cursor { x, y } => self.cursor = (x, y),
            RawEvent::CursorInside(inside) => self.inside = inside,
            RawEvent::FocusLost => {
                for k in std::mem::take(&mut self.down) {
                    if k != KeyId::function(10).expect("F10") {
                        out.push(Callback::KeyUp(k.name()));
                    }
                }
            }
        }
        out
    }
}

/// The `RawEvent::Char` units of a typed character: one code point, or two UTF-16 halves above U+FFFF (the original
/// receives `WM_CHAR` per UTF-16 unit; [`InputState::translate`] joins the halves into one code point, as GLFW 3.4).
pub fn char_units(c: char) -> impl Iterator<Item = u32> {
    let mut buf = [0u16; 2];
    let units: Vec<u32> = c.encode_utf16(&mut buf).iter().map(|&u| u32::from(u)).collect();
    units.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_parse() {
        assert_eq!(KeyId::parse("CTRL").map(KeyId::name), Ok("CTRL"));
        assert_eq!(KeyId::parse("A").map(KeyId::name), Ok("a"));
        assert_eq!(KeyId::parse(" ").map(KeyId::name), Ok(" "));
        assert_eq!(KeyId::parse("F15").map(KeyId::name), Ok("F15"));
        assert_eq!(KeyId::parse(""), Err(KeyNameError::Empty));
        assert_eq!(KeyId::parse("F16"), Err(KeyNameError::Unrecognised));
        assert_eq!(KeyId::parse("FOO"), Err(KeyNameError::Unrecognised));
        assert_eq!(KeyId::function(10).map(KeyId::name), Some("F10"));
        assert_eq!(PhysKey::Char(b'=').key_id().map(KeyId::name), Some("+"));
        assert_eq!(PhysKey::NumpadEnter.key_id(), Some(KeyId::RETURN));
    }

    #[test]
    fn char_text_rules() {
        assert_eq!(char_text(u32::from('a')), Some("a"));
        assert_eq!(char_text(0x0D), None);
        assert_eq!(char_text(0x7F), None);
        assert_eq!(char_text(0x80), None);
        assert_eq!(char_text(0x9F), None);
        assert_eq!(char_text(0xA0), Some("F7"));
        assert_eq!(char_text(0xAA), Some("SCROLLLOCK"));
        assert_eq!(char_text(0xE9), Some("?"));
        assert_eq!(char_text(0x101), Some("?"));
        assert_eq!(char_text(0x2007), Some("?"));
        assert_eq!(char_text(0x44F), Some("?"));
        let mut s = InputState::new();
        let typed: Vec<Callback> = char_units('\u{1F600}').flat_map(|u| s.translate(RawEvent::Char(u), 0)).collect();
        assert_eq!(typed, [Callback::Char("?")]);
    }

    #[test]
    fn double_click_window() {
        let mut s = InputState::new();
        let click = |s: &mut InputState, t| {
            let cb = s.translate(
                RawEvent::Button {
                    button: Button::Left,
                    pressed: true,
                    pos: None,
                },
                t,
            );
            s.translate(
                RawEvent::Button {
                    button: Button::Left,
                    pressed: false,
                    pos: None,
                },
                t + 20,
            );
            cb[0]
        };
        assert_eq!(click(&mut s, 0), Callback::KeyDown("LEFTBUTTON", false));
        assert_eq!(
            click(&mut s, 100),
            Callback::KeyDown("LEFTBUTTON", false),
            "cursor outside the window"
        );
        s.translate(RawEvent::CursorInside(true), 900);
        assert_eq!(click(&mut s, 1000), Callback::KeyDown("LEFTBUTTON", false));
        assert_eq!(click(&mut s, 1300), Callback::KeyDown("LEFTBUTTON", true));
        assert_eq!(click(&mut s, 1400), Callback::KeyDown("LEFTBUTTON", false));
        assert_eq!(click(&mut s, 2000), Callback::KeyDown("LEFTBUTTON", false));
        s.translate(RawEvent::Cursor { x: 10.0, y: 0.0 }, 2050);
        assert_eq!(click(&mut s, 2100), Callback::KeyDown("LEFTBUTTON", false));
    }
}
