//! `LjTable::len` (`#t`, `lj_tab_len`) against the desktop LuaJIT of the upstream (fixture of tools/lua-corpus/export-tab-len.ts): tables
//! built from `{}` by `t[k] = true` / `t[k] = nil`, as the item, skill and config set tables of the 2.69.0 set code are (`#sets + 1`).

use pob_tree::lj_table::LjTable;

#[test]
fn the_length_is_luajits() {
    let fx: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/lj-tab-len.json")).unwrap())
            .unwrap();
    let cases = fx["cases"].as_array().unwrap();
    assert!(cases.len() > 1500, "{}", cases.len());
    let mut bad = Vec::new();
    for c in cases {
        let ops: Vec<i64> = c[0].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
        let want = c[1].as_u64().unwrap() as usize;
        let mut t = LjTable::new();
        for &op in &ops {
            if op > 0 {
                t.set(op as f64);
            } else {
                // `t[k] = nil`: an absent key is created and holds nil (lj_meta_tset)
                t.remove((-op) as f64, true);
            }
        }
        if t.len() != want && bad.len() < 20 {
            bad.push(format!("{ops:?}: got {} want {want}", t.len()));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
