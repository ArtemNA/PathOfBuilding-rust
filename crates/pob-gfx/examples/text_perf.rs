//! ns per call of the text functions, on the same inputs as the SG measurement in
//! `tools/sg-oracle/lua/text-extra.lua` (`fixtures/text-extra/perf.json`, 200 000 calls each from Lua).
//!   cargo run --release -p pob-gfx --example text_perf [-- <fonts dir>]
#[path = "../tests/scene/mod.rs"]
mod scene;

use pob_gfx::text::{FontFamily, FontSet, strip_escapes};
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let dir = std::env::args().nth(1).map_or_else(scene::fonts_dir, PathBuf::from);
    let t0 = Instant::now();
    let fonts = FontSet::load(&dir).expect("fonts");
    println!("load 7 .tgf: {:.2} ms", t0.elapsed().as_secs_f64() * 1e3);
    let s1 = "+12.5% increased Damage";
    let s2 = "^7The quick brown fox ^xFF8000jumps over the lazy dog";
    const N: u32 = 2_000_000;
    let var = FontFamily::Var;
    let bench = |name: &str, f: &dyn Fn() -> i64| {
        for _ in 0..N / 10 {
            black_box(f());
        }
        let t = Instant::now();
        for _ in 0..N {
            black_box(f());
        }
        println!("{name:14} {:7.1} ns/call", t.elapsed().as_secs_f64() * 1e9 / f64::from(N));
    };
    bench("width_short", &|| fonts.draw_string_width(black_box(16.0), var, black_box(s1)));
    bench("width_long", &|| fonts.draw_string_width(black_box(16.0), var, black_box(s2)));
    bench("width_scaled", &|| fonts.draw_string_width(black_box(30.0), var, black_box(s1)));
    bench("cursor", &|| {
        fonts.draw_string_cursor_index(black_box(16.0), var, black_box(s1), 80.0, 5.0) as i64
    });
    bench("strip", &|| strip_escapes(black_box(s2)).len() as i64);
}
