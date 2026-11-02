//! U1.1 gate: the pure text functions equal the original SimpleGraphic on the whole SG-oracle corpus
//! `tools/sg-oracle/fixtures/text/*.json` (written by `tools/sg-oracle/lua/text-corpus.lua`), 0 mismatches.
//! Fonts: `<POB_ROOT>/runtime/SimpleGraphic/Fonts` (POB_ROOT defaults to the read-only PoB checkout).

use pob_gfx::text::{FontFamily, FontSet, strip_escapes};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::OnceLock;

const DEFAULT_POB_ROOT: &str = "C:/pob-work/project_15/PathOfBuilding";

fn fonts_dir() -> PathBuf {
    let root = std::env::var_os("POB_ROOT").map_or_else(|| PathBuf::from(DEFAULT_POB_ROOT), PathBuf::from);
    root.join("runtime/SimpleGraphic/Fonts")
}

fn fonts() -> &'static FontSet {
    static F: OnceLock<FontSet> = OnceLock::new();
    F.get_or_init(|| FontSet::load(&fonts_dir()).expect("load fonts"))
}

fn fixture(dir: &str, name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/sg-oracle")
        .join(dir)
        .join(name);
    let s = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn family(name: &str) -> FontFamily {
    FontFamily::from_lua_name(name).unwrap_or_else(|| panic!("unknown font {name}"))
}

/// Collects mismatches, prints the first ones, and asserts the expected total and zero mismatches.
struct Tally {
    what: &'static str,
    total: usize,
    bad: Vec<String>,
}

impl Tally {
    fn new(what: &'static str) -> Self {
        Self {
            what,
            total: 0,
            bad: Vec::new(),
        }
    }
    fn check<T: PartialEq + std::fmt::Debug>(&mut self, got: T, want: T, ctx: impl FnOnce() -> String) {
        self.total += 1;
        if got != want {
            self.bad.push(format!("{}: got {got:?}, want {want:?}", ctx()));
        }
    }
    fn finish(self, expected_total: usize) {
        for b in self.bad.iter().take(30) {
            eprintln!("{} mismatch {b}", self.what);
        }
        eprintln!("{}: {} checked, {} mismatches", self.what, self.total, self.bad.len());
        assert_eq!(self.total, expected_total, "{}: corpus size", self.what);
        assert!(
            self.bad.is_empty(),
            "{}: {} of {} differ from the oracle",
            self.what,
            self.bad.len(),
            self.total
        );
    }
}

fn int(v: &Value) -> i64 {
    v.as_i64().unwrap_or_else(|| panic!("not an integer: {v}"))
}

#[test]
fn glyph_widths_match_oracle() {
    let g = fixture("fixtures/text", "glyphs.json");
    let (from, to) = (int(&g["chars_from"]) as u8, int(&g["chars_to"]) as u8);
    let heights: Vec<f64> = g["heights"].as_array().unwrap().iter().map(|h| h.as_f64().unwrap()).collect();
    let mut t = Tally::new("glyph width");
    for (name, rows) in g["fonts"].as_object().unwrap() {
        let font = fonts().font(family(name));
        for (hi, row) in rows.as_array().unwrap().iter().enumerate() {
            let h = heights[hi];
            for (ci, c) in (from..=to).enumerate() {
                let s = (c as char).to_string();
                t.check(font.string_width(h, &s), int(&row[ci]), || format!("{name} h={h} {s:?}"));
            }
        }
    }
    t.finish(61_180);
}

#[test]
fn string_widths_match_oracle() {
    let w = fixture("fixtures/text", "width.json");
    let heights: Vec<f64> = w["heights"].as_array().unwrap().iter().map(|h| h.as_f64().unwrap()).collect();
    let strings: Vec<&str> = w["strings"].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
    let mut t = Tally::new("string width");
    for (name, rows) in w["fonts"].as_object().unwrap() {
        let f = family(name);
        for (hi, row) in rows.as_array().unwrap().iter().enumerate() {
            for (si, s) in strings.iter().enumerate() {
                let h = heights[hi];
                t.check(fonts().draw_string_width(h, f, s), int(&row[si]), || format!("{name} h={h} {s:?}"));
            }
        }
    }
    t.finish(32_200);
}

#[test]
fn cursor_indices_match_oracle() {
    let c = fixture("fixtures/text", "cursor.json");
    let mut t = Tally::new("cursor index");
    for row in c["rows"].as_array().unwrap() {
        let (name, h, s, y, x0) = (
            row[0].as_str().unwrap(),
            row[1].as_f64().unwrap(),
            row[2].as_str().unwrap(),
            row[3].as_f64().unwrap(),
            int(&row[4]),
        );
        let f = family(name);
        for (j, want) in row[5].as_array().unwrap().iter().enumerate() {
            let x = (x0 + j as i64) as f64;
            let got = fonts().draw_string_cursor_index(h, f, s, x, y) as i64;
            t.check(got, int(want), || format!("{name} h={h} {s:?} x={x} y={y}"));
        }
    }
    t.finish(253_336);
}

#[test]
fn strip_escapes_matches_oracle() {
    let s = fixture("fixtures/text", "strip.json");
    let mut t = Tally::new("strip");
    for case in s["cases"].as_array().unwrap() {
        let (input, want) = (case[0].as_str().unwrap(), case[1].as_str().unwrap());
        t.check(strip_escapes(input), want.to_string(), || format!("{input:?}"));
    }
    t.finish(50);
}

fn bytes(v: &Value) -> Vec<u8> {
    v.as_array().unwrap().iter().map(|b| int(b) as u8).collect()
}

/// U1.1 extra corpus (`tools/sg-oracle/lua/text-extra.lua`): cases `text-corpus.lua` left ambiguous — cursor line
/// height and fractional x/y, scaled pen accumulation, `^X`, non-BMP, control codes, negative/huge heights.
/// Rows whose text is not valid UTF-8 are outside the `&str` API and are only counted (see `invalid_utf8_rows`).
#[test]
fn extra_cases_match_oracle() {
    let c = fixture("fixtures/text-extra", "cases.json");
    let mut t = Tally::new("extra");
    let mut skipped = 0;
    for row in c["rows"].as_array().unwrap() {
        let kind = row[0].as_str().unwrap();
        let raw = bytes(&row[3]);
        let Ok(s) = std::str::from_utf8(&raw) else {
            skipped += 1;
            continue;
        };
        match kind {
            "s" => {
                let want = String::from_utf8(bytes(&row[6])).unwrap();
                t.check(strip_escapes(s), want, || format!("strip {s:?}"));
            }
            "w" => {
                let (f, h) = (family(row[1].as_str().unwrap()), row[2].as_f64().unwrap());
                t.check(fonts().draw_string_width(h, f, s), int(&row[6]), || {
                    format!("width {} h={h} {s:?}", f.lua_name())
                });
            }
            "c" => {
                let (f, h) = (family(row[1].as_str().unwrap()), row[2].as_f64().unwrap());
                let (x, y) = (row[4].as_f64().unwrap(), row[5].as_f64().unwrap());
                let got = fonts().draw_string_cursor_index(h, f, s, x, y) as i64;
                t.check(got, int(&row[6]), || format!("cursor {} h={h} {s:?} x={x} y={y}", f.lua_name()));
            }
            k => panic!("unknown row kind {k}"),
        }
    }
    eprintln!("extra: {skipped} rows with invalid UTF-8 skipped");
    assert_eq!(skipped, 988, "invalid-UTF-8 rows");
    t.finish(19_769);
}

/// Every `.tga` atlas decodes and contains the rectangles its `.tgf` section describes.
#[test]
fn atlases_cover_their_glyphs() {
    let mut n = 0;
    for f in FontFamily::ALL {
        let font = fonts().font(f);
        for fh in font.heights() {
            let img = font.load_atlas(fonts().dir(), fh.height).unwrap_or_else(|e| panic!("{e}"));
            for (code, g) in fh.glyphs.iter().enumerate() {
                assert!(
                    g.x + g.width <= img.width() && g.y + fh.height <= img.height(),
                    "{} h={} glyph {code} {g:?} outside {}x{}",
                    f.lua_name(),
                    fh.height,
                    img.width(),
                    img.height()
                );
            }
            n += 1;
        }
    }
    assert_eq!(n, 7 * 16);
}
