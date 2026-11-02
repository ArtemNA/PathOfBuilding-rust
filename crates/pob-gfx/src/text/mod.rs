//! Pure text functions of the SimpleGraphic host: bitmap fonts, `DrawStringWidth`, `DrawStringCursorIndex`,
//! `StripEscapes`. No GPU here (UI-ARCH §4.1); rendering of glyph quads comes in U1.3.
//!
//! All rules below were deciphered from the exact SG-oracle corpora `tools/sg-oracle/fixtures/text/*.json` (U1.0)
//! and `fixtures/text-extra/cases.json` (U1.1, `lua/text-extra.lua`) of the original `SimpleGraphic v2.5-3b1a346`
//! (C++ sources unavailable) and are gated by `tests/text.rs`:
//!
//! 1. **Effective height** `H` = `floor(height + 0.5)`; values below 1 become 1; an odd `H > 1` becomes `H + 1`
//!    ([`effective_height`]).
//! 2. **Atlas** = the `.tgf` section nearest to `H`, ties to the lower one (h 30 -> 28, 44 -> 40, 46 -> 48); above the
//!    largest (64) the largest. Scale = `H / atlas` (also above 1: h 100 draws atlas 64 at 100/64).
//! 3. **Glyph advance** = `ceil(advance * H / atlas)` per glyph (`advance` = spLeft + width + spRight), summed as integers.
//!    Tab = `ceil(4 * advance(' ') * H / atlas)`. Other ASCII control codes use their own `.tgf` glyphs.
//! 4. **Non-ASCII** code points are drawn as the text `[U+XXXX]` (`{:04X}`) with a smaller atlas, unscaled:
//!    the largest atlas `a < atlas` with `a <= H - 4`, or the main atlas when none exists (h <= 12).
//! 5. **Escapes** `^N`/`^xRRGGBB`/`^XRRGGBB` take no space ([`escape_len`]). `\n` starts a new line; width = widest
//!    line. Text ends at the first NUL ([`until_nul`]).
//! 6. **Cursor index** (1-based byte index): `x`, `y` rounded half up to integers; line = first line whose bottom
//!    `(k + 1) * H >= y` (the last line if `y` is below all); in it, the first glyph with `x <= pen + advance`
//!    (escape bytes skipped); past its end, the index of the line's `\n` (or `len + 1`).
//!
//! The original also accepts invalid UTF-8 (every undecodable byte becomes one `[U+XXXX]`, text-extra corpus);
//! this API takes `&str`, so that case cannot arise here.

mod escape;
mod tgf;

use std::fmt;
use std::path::{Path, PathBuf};

pub use escape::{escape_len, strip_escapes, until_nul};
pub use tgf::{FontHeight, GLYPH_COUNT, Glyph, TgfError, parse_tgf};

/// The seven font families of SimpleGraphic, in the order of the binary's table (SG-DLL-FACTS §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontFamily {
    Fixed,
    Var,
    VarBold,
    FontinSc,
    FontinScItalic,
    Fontin,
    FontinItalic,
}

impl FontFamily {
    pub const ALL: [FontFamily; 7] = [
        Self::Fixed,
        Self::Var,
        Self::VarBold,
        Self::FontinSc,
        Self::FontinScItalic,
        Self::Fontin,
        Self::FontinItalic,
    ];

    /// Lua name as passed to `DrawString*` (`"VAR"`, `"FONTIN SC"`…).
    pub fn lua_name(self) -> &'static str {
        match self {
            Self::Fixed => "FIXED",
            Self::Var => "VAR",
            Self::VarBold => "VAR BOLD",
            Self::FontinSc => "FONTIN SC",
            Self::FontinScItalic => "FONTIN SC ITALIC",
            Self::Fontin => "FONTIN",
            Self::FontinItalic => "FONTIN ITALIC",
        }
    }

    /// Parses a Lua font name (exact, upper-case as the original UI passes them).
    pub fn from_lua_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.lua_name() == name)
    }

    /// File stem under `runtime/SimpleGraphic/Fonts/` (`<stem>.tgf`, `<stem>.<h>.tga`).
    pub fn file_stem(self) -> &'static str {
        match self {
            Self::Fixed => "Bitstream Vera Sans Mono",
            Self::Var => "Liberation Sans",
            Self::VarBold => "Liberation Sans Bold",
            Self::FontinSc => "Fontin SmallCaps",
            Self::FontinScItalic => "Fontin SmallCaps Italic",
            Self::Fontin => "Fontin",
            Self::FontinItalic => "Fontin Italic",
        }
    }
}

#[derive(Debug)]
pub enum FontError {
    Io(PathBuf, std::io::Error),
    Tgf(PathBuf, TgfError),
    Image(PathBuf, image::ImageError),
    NoAtlas(u32),
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(p, e) => write!(f, "{}: {e}", p.display()),
            Self::Tgf(p, e) => write!(f, "{}: {e}", p.display()),
            Self::Image(p, e) => write!(f, "{}: {e}", p.display()),
            Self::NoAtlas(h) => write!(f, "font has no atlas of height {h}"),
        }
    }
}

impl std::error::Error for FontError {}

/// Rule 1: the integer height text is laid out at.
pub fn effective_height(height: f64) -> u32 {
    let r = (height + 0.5).floor();
    // NaN also maps to 1.
    let h = if r.is_nan() || r < 1.0 {
        1
    } else if r >= f64::from(u32::MAX - 1) {
        u32::MAX - 1
    } else {
        r as u32
    };
    if h > 1 && h % 2 == 1 { h + 1 } else { h }
}

/// `ceil(n / d)` for `d > 0`, exact (equals the oracle's float result: `n` and `d` are small integers).
fn ceil_div(n: i64, d: i64) -> i64 {
    if n >= 0 { (n + d - 1) / d } else { -(-n / d) }
}

/// One font family: its `.tgf` sections (one per `.tga` atlas), ascending by height.
#[derive(Clone, Debug)]
pub struct Font {
    family: Option<FontFamily>,
    heights: Vec<FontHeight>,
}

/// The atlases a call lays text out with (rules 1, 2, 4).
#[derive(Clone, Copy, Debug)]
struct Metrics<'a> {
    /// Effective height `H`.
    eff: i64,
    main: &'a FontHeight,
    /// Atlas of the `[U+XXXX]` placeholder (drawn unscaled).
    sub: &'a FontHeight,
}

impl Metrics<'_> {
    /// Rule 3/4: integer advance of one character.
    fn advance(&self, ch: char) -> i64 {
        let atlas = i64::from(self.main.height);
        let scale = |a: i64| if self.eff == atlas { a } else { ceil_div(a * self.eff, atlas) };
        match ch {
            '\t' => scale(4 * self.main.glyphs[usize::from(b' ')].advance()),
            c if c.is_ascii() => scale(self.main.glyphs[c as usize].advance()),
            c => {
                let g = |b: u8| self.sub.glyphs[usize::from(b)].advance();
                let cp = u32::from(c);
                let digits = (32 - cp.leading_zeros()).div_ceil(4).max(4);
                let hex = (0..digits).rev().map(|k| b"0123456789ABCDEF"[((cp >> (4 * k)) & 0xF) as usize]);
                g(b'[') + g(b'U') + g(b'+') + hex.map(g).sum::<i64>() + g(b']')
            }
        }
    }
}

/// The text a non-ASCII code point is drawn as (`[U+{:04X}]`, SG-DLL-FACTS §4).
pub fn placeholder(c: char) -> String {
    format!("[U+{:04X}]", u32::from(c))
}

/// One laid-out item of a line: a character (byte offset, char) — escapes and `\n` are consumed by the walker.
enum Item {
    Glyph(usize, char),
    NewLine(usize),
}

/// Walks `text` byte by byte as SG does: escapes skipped, `\n` reported, everything else one character.
fn items(text: &str) -> impl Iterator<Item = Item> + '_ {
    let text = until_nul(text);
    let b = text.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        loop {
            if i >= b.len() {
                return None;
            }
            let e = escape_len(b, i);
            if e > 0 {
                i += e;
                continue;
            }
            let at = i;
            if b[i] == b'\n' {
                i += 1;
                return Some(Item::NewLine(at));
            }
            let c = if b[i].is_ascii() {
                char::from(b[i])
            } else {
                text[i..].chars().next().expect("index is on a char boundary")
            };
            i += c.len_utf8();
            return Some(Item::Glyph(at, c));
        }
    })
}

impl Font {
    /// Builds a font from parsed `.tgf` sections (must be non-empty and ascending).
    pub fn from_heights(heights: Vec<FontHeight>) -> Self {
        assert!(!heights.is_empty(), "font without heights");
        assert!(heights.windows(2).all(|w| w[0].height < w[1].height), "heights not ascending");
        Self { family: None, heights }
    }

    /// Loads `<dir>/<stem>.tgf`.
    pub fn load(fonts_dir: &Path, family: FontFamily) -> Result<Self, FontError> {
        let path = fonts_dir.join(format!("{}.tgf", family.file_stem()));
        let src = std::fs::read(&path).map_err(|e| FontError::Io(path.clone(), e))?;
        // The files are ASCII; comments may hold any byte, so decode lossily.
        let heights = parse_tgf(&String::from_utf8_lossy(&src)).map_err(|e| FontError::Tgf(path.clone(), e))?;
        Ok(Self {
            family: Some(family),
            ..Self::from_heights(heights)
        })
    }

    pub fn family(&self) -> Option<FontFamily> {
        self.family
    }

    pub fn heights(&self) -> &[FontHeight] {
        &self.heights
    }

    /// Rule 2: index of the atlas used for effective height `eff`.
    pub fn atlas_index(&self, eff: u32) -> usize {
        let mut best = 0;
        for (i, fh) in self.heights.iter().enumerate() {
            if fh.height.abs_diff(eff) < self.heights[best].height.abs_diff(eff) {
                best = i;
            }
        }
        best
    }

    /// Rule 4: index of the atlas of the `[U+XXXX]` placeholder for effective height `eff`.
    pub fn placeholder_atlas_index(&self, eff: u32) -> usize {
        let main = self.atlas_index(eff);
        (0..main).rev().find(|&i| self.heights[i].height + 4 <= eff).unwrap_or(main)
    }

    fn metrics(&self, height: f64) -> Metrics<'_> {
        let eff = effective_height(height);
        Metrics {
            eff: i64::from(eff),
            main: &self.heights[self.atlas_index(eff)],
            sub: &self.heights[self.placeholder_atlas_index(eff)],
        }
    }

    /// Integer advance of one character at `height` (what `DrawStringWidth(height, font, ch)` returns).
    pub fn char_advance(&self, height: f64, ch: char) -> i64 {
        self.metrics(height).advance(ch)
    }

    /// `DrawStringWidth(height, font, text)`: width of the widest line, escapes excluded.
    pub fn string_width(&self, height: f64, text: &str) -> i64 {
        let m = self.metrics(height);
        let (mut max, mut cur) = (0, 0);
        for it in items(text) {
            match it {
                Item::Glyph(_, c) => cur += m.advance(c),
                Item::NewLine(_) => {
                    max = max.max(cur);
                    cur = 0;
                }
            }
        }
        max.max(cur)
    }

    /// `DrawStringCursorIndex(height, font, text, x, y)`: 1-based byte index of the caret position under the point
    /// `(x, y)` relative to the text's top-left corner (rule 6).
    pub fn cursor_index(&self, height: f64, text: &str, x: f64, y: f64) -> usize {
        let m = self.metrics(height);
        let text = until_nul(text);
        // `\n` never occurs inside an escape, so the byte count is the line count - 1.
        let last_line = text.bytes().filter(|&b| b == b'\n').count();
        // The point is rounded half up to whole pixels (text-extra: x 7.25 -> 7, 7.5 -> 8; y likewise).
        let (x, y) = ((x + 0.5).floor(), (y + 0.5).floor());
        let mut target = 0;
        while target < last_line && y > ((target + 1) as i64 * m.eff) as f64 {
            target += 1;
        }
        let (mut line, mut pen) = (0, 0i64);
        for it in items(text) {
            match it {
                Item::NewLine(at) => {
                    if line == target {
                        return at + 1;
                    }
                    line += 1;
                }
                Item::Glyph(at, c) if line == target => {
                    let adv = m.advance(c);
                    if x <= (pen + adv) as f64 {
                        return at + 1;
                    }
                    pen += adv;
                }
                Item::Glyph(..) => {}
            }
        }
        text.len() + 1
    }

    /// Loads the `.tga` atlas of section `height` from `<dir>/<stem>.<height>.tga` as RGBA8.
    pub fn load_atlas(&self, fonts_dir: &Path, height: u32) -> Result<image::RgbaImage, FontError> {
        let family = self.family.expect("load_atlas needs a font loaded with Font::load");
        if !self.heights.iter().any(|fh| fh.height == height) {
            return Err(FontError::NoAtlas(height));
        }
        let path = fonts_dir.join(format!("{}.{height}.tga", family.file_stem()));
        let bytes = std::fs::read(&path).map_err(|e| FontError::Io(path.clone(), e))?;
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Tga).map_err(|e| FontError::Image(path.clone(), e))?;
        Ok(img.to_rgba8())
    }
}

/// All seven families, loaded from one `Fonts` directory.
#[derive(Clone, Debug)]
pub struct FontSet {
    dir: PathBuf,
    fonts: Vec<Font>,
}

impl FontSet {
    /// Loads every `.tgf` from `fonts_dir` (`runtime/SimpleGraphic/Fonts`).
    pub fn load(fonts_dir: &Path) -> Result<Self, FontError> {
        let fonts = FontFamily::ALL
            .into_iter()
            .map(|f| Font::load(fonts_dir, f))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            dir: fonts_dir.to_path_buf(),
            fonts,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn font(&self, family: FontFamily) -> &Font {
        &self.fonts[FontFamily::ALL.iter().position(|&f| f == family).expect("family in ALL")]
    }

    /// `DrawStringWidth(height, font, text)`.
    pub fn draw_string_width(&self, height: f64, font: FontFamily, text: &str) -> i64 {
        self.font(font).string_width(height, text)
    }

    /// `DrawStringCursorIndex(height, font, text, cursorX, cursorY)` (1-based byte index).
    pub fn draw_string_cursor_index(&self, height: f64, font: FontFamily, text: &str, x: f64, y: f64) -> usize {
        self.font(font).cursor_index(height, text, x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_height_rounds_half_up_then_to_even() {
        let cases = [
            (0.0, 1),
            (-3.0, 1),
            (f64::NAN, 1),
            (1.0, 1),
            (1.4, 1),
            (1.5, 2),
            (2.0, 2),
            (3.0, 4),
            (10.5, 12),
        ];
        for (h, e) in cases {
            assert_eq!(effective_height(h), e, "h={h}");
        }
        assert_eq!(effective_height(12.5), 14);
        assert_eq!(effective_height(14.4), 14);
        assert_eq!(effective_height(15.99), 16);
        assert_eq!(effective_height(16.01), 16);
    }

    #[test]
    fn ceil_div_handles_signs() {
        assert_eq!(ceil_div(7, 2), 4);
        assert_eq!(ceil_div(8, 2), 4);
        assert_eq!(ceil_div(-7, 2), -3);
        assert_eq!(ceil_div(0, 5), 0);
    }

    #[test]
    fn family_names_round_trip() {
        for f in FontFamily::ALL {
            assert_eq!(FontFamily::from_lua_name(f.lua_name()), Some(f));
        }
        assert_eq!(FontFamily::from_lua_name("var"), None);
    }
}
