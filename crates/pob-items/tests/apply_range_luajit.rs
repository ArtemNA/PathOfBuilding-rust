//! `itemLib.applyRange` = LuaJIT on the lines that stress `findScalableLine`: `Unique ID: <hash>` with many runs of digits, real
//! `modScalability` keys as lines, literal `#` and `+#` forms (producer `tests/lua/export-apply-range.ts`).
//! A Lua error upstream raises is a panic here (L18): the fixture's `{error}` cases must panic too.

use std::panic::catch_unwind;

use pob_items::apply_range;
use pob_parser::ParseMode;

#[test]
fn apply_range_scalable_equals_luajit() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/apply-range-scalable.luajit.json");
    let fixture: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("json");
    let cases = fixture["cases"].as_array().expect("cases");
    let (mut same, mut errors) = (0, 0);
    for c in cases {
        let line = c["line"].as_str().expect("line");
        let (range, value_scalar, base) = (
            c["range"].as_f64().expect("range"),
            c["valueScalar"].as_f64(),
            c["baseValueScalar"].as_f64(),
        );
        let got = catch_unwind(|| apply_range(line, range, value_scalar, base, ParseMode::Full));
        match (c["result"].as_str(), got) {
            (Some(want), Ok(got)) => {
                assert_eq!(got, want, "applyRange({line:?}, {range}, {value_scalar:?}, {base:?})");
                same += 1;
            }
            (None, Err(_)) => errors += 1,
            (_, got) => panic!(
                "applyRange({line:?}, {range}, {value_scalar:?}, {base:?}): LuaJIT {}, Rust {}",
                c["result"],
                if got.is_ok() { "an answer" } else { "a panic" }
            ),
        }
    }
    println!("apply_range = LuaJIT on {same} lines, {errors} Lua errors on both sides");
    assert!(same > 1_500, "{same}");
}
