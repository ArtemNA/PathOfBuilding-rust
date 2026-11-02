//! `.tgf` glyph-metric files of SimpleGraphic bitmap fonts (UI-ARCH §3.6).
//!
//! Format (text, CRLF or LF): sections `HEIGHT <h>;`, each followed by exactly 128 lines
//! `GLYPH <x> <y> <width> <spLeft> <spRight>;\t// <code> (<char>)` for codes 0..127
//! (the binary's scanf formats are `HEIGHT %u;` and `GLYPH %u %u %u %d %d;`).

use std::fmt;

/// Number of glyphs per height section (ASCII 0..127).
pub const GLYPH_COUNT: usize = 128;

/// One glyph: its rectangle in the `.tga` atlas (`x`, `y`, `width` x section height) and spacing around it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Glyph {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub sp_left: i32,
    pub sp_right: i32,
}

impl Glyph {
    /// Unscaled pen advance in atlas pixels: `spLeft + width + spRight` (no kerning).
    pub fn advance(&self) -> i64 {
        i64::from(self.width) + i64::from(self.sp_left) + i64::from(self.sp_right)
    }
}

/// One `HEIGHT` section: the metrics of the atlas `<Family>.<height>.tga`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontHeight {
    pub height: u32,
    pub glyphs: Box<[Glyph; GLYPH_COUNT]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TgfError {
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for TgfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tgf line {}: {}", self.line, self.msg)
    }
}

impl std::error::Error for TgfError {}

/// Parses a `.tgf` file. Sections are returned sorted by height (ascending), as the files store them.
pub fn parse_tgf(src: &str) -> Result<Vec<FontHeight>, TgfError> {
    let mut out: Vec<FontHeight> = Vec::new();
    let mut count = 0usize;
    let err = |line: usize, msg: String| TgfError { line, msg };
    for (n, raw) in src.lines().enumerate() {
        let line_no = n + 1;
        let line = raw.trim_end_matches('\r');
        // Comments start at "//" (every GLYPH line carries one).
        let line = line.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some(body) = line.strip_suffix(';') else {
            return Err(err(line_no, format!("missing ';': {line:?}")));
        };
        let mut words = body.split_ascii_whitespace();
        match words.next() {
            Some("HEIGHT") => {
                if let Some(prev) = out.last()
                    && count != GLYPH_COUNT
                {
                    return Err(err(
                        line_no,
                        format!("height {} has {count} glyphs, expected {GLYPH_COUNT}", prev.height),
                    ));
                }
                let h: u32 = parse_num(words.next(), line_no)?;
                if words.next().is_some() {
                    return Err(err(line_no, "extra fields after HEIGHT".into()));
                }
                if out.last().is_some_and(|p| p.height >= h) {
                    return Err(err(line_no, format!("heights not ascending at {h}")));
                }
                out.push(FontHeight {
                    height: h,
                    glyphs: Box::new([Glyph::default(); GLYPH_COUNT]),
                });
                count = 0;
            }
            Some("GLYPH") => {
                let Some(fh) = out.last_mut() else {
                    return Err(err(line_no, "GLYPH before HEIGHT".into()));
                };
                if count >= GLYPH_COUNT {
                    return Err(err(line_no, format!("more than {GLYPH_COUNT} glyphs in height {}", fh.height)));
                }
                let g = Glyph {
                    x: parse_num(words.next(), line_no)?,
                    y: parse_num(words.next(), line_no)?,
                    width: parse_num(words.next(), line_no)?,
                    sp_left: parse_num(words.next(), line_no)?,
                    sp_right: parse_num(words.next(), line_no)?,
                };
                if words.next().is_some() {
                    return Err(err(line_no, "extra fields after GLYPH".into()));
                }
                fh.glyphs[count] = g;
                count += 1;
            }
            _ => return Err(err(line_no, format!("unknown statement {line:?}"))),
        }
    }
    match out.last() {
        None => Err(err(0, "no HEIGHT sections".into())),
        Some(fh) if count != GLYPH_COUNT => Err(err(0, format!("height {} has {count} glyphs, expected {GLYPH_COUNT}", fh.height))),
        Some(_) => Ok(out),
    }
}

fn parse_num<T: std::str::FromStr>(w: Option<&str>, line: usize) -> Result<T, TgfError> {
    let w = w.ok_or_else(|| TgfError {
        line,
        msg: "missing field".into(),
    })?;
    w.parse().map_err(|_| TgfError {
        line,
        msg: format!("bad number {w:?}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_line_endings_and_negative_spacing() {
        let mut s = String::from("HEIGHT 10;\n");
        for c in 0..128 {
            s.push_str(&format!("GLYPH {} 0 5 -1 2;\t// {c}\r\n", c * 8));
        }
        let v = parse_tgf(&s).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(
            v[0].glyphs[127],
            Glyph {
                x: 1016,
                y: 0,
                width: 5,
                sp_left: -1,
                sp_right: 2
            }
        );
        assert_eq!(v[0].glyphs[3].advance(), 6);
    }

    #[test]
    fn rejects_short_section() {
        let e = parse_tgf("HEIGHT 10;\nGLYPH 0 0 1 0 0;\n").unwrap_err();
        assert!(e.msg.contains("1 glyphs"), "{e}");
    }
}
