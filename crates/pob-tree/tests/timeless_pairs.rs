//! R5.5c D13: `pob_tree::lj_table::pairs_order` (the LuaJIT 2.1 table model behind `pairs(additions)`, PassiveSpec.lua:1314) = the
//! order LuaJIT walks 4 000 integer-keyed tables filled like `additions` (1-8 keys from 0..40 / 0..130 / 0..400, repeats update in
//! place). Fixture `tests/fixtures/timeless-pairs.json` (producer `tools/lua-corpus/export-timeless.ts`, docker LuaJIT).

use pob_tree::lj_table::pairs_order;
use serde_json::Value as Json;

fn nums(s: &str) -> Vec<f64> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.split(',').map(|x| x.parse().unwrap()).collect()
    }
}

#[test]
fn pairs_order_equals_luajit() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/timeless-pairs.json");
    let f: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let cases = f["cases"].as_array().unwrap();
    let (mut same, mut ascending, mut bad) = (0, 0, Vec::new());
    for c in cases {
        let (seq, want) = c.as_str().unwrap().split_once('>').unwrap();
        let (seq, want) = (nums(seq), nums(want));
        let got = pairs_order(&seq);
        let mut sorted = want.clone();
        sorted.sort_by(f64::total_cmp);
        if sorted == want {
            ascending += 1;
        }
        if got == want {
            same += 1;
        } else {
            bad.push(format!("{seq:?}: rust {got:?} luajit {want:?}"));
        }
    }
    eprintln!("{same}/{} pairs orders = LuaJIT ({ascending} of them ascending)", cases.len());
    assert!(bad.is_empty(), "{} differ, first: {:?}", bad.len(), &bad[..bad.len().min(5)]);
    assert_eq!(cases.len(), 4000);
}
