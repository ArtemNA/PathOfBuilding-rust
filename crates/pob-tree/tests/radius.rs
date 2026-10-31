//! R5.5b: `nodesInRadius` (PassiveTree.lua:654-711) of every socket and keystone of the tree against LuaJIT
//! (`tests/fixtures/radius-tree-<ver>.json`, producer `tools/lua-corpus/export-radius.ts`): the id SETS of each radius, exactly,
//! and the radius table (`data.jewelRadius` as `main:LoadTree` set it for the tree's version, MINEFIELD D7).

use std::collections::BTreeSet;

use pob_tree::{ParseMode, load_passive_tree_mode, set_jewel_radii};
use serde_json::Value as Json;

fn fixture(name: &str) -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

fn ids(j: &Json) -> BTreeSet<u32> {
    j.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()
}

#[test]
fn set_jewel_radii_picks_the_table_by_version() {
    // Data.lua:585 `tonumber(major) <= 3 and tonumber(minor) <= 15`
    for (v, n, max) in [
        ("3_6", 8, 2000.0),
        ("3_13", 8, 2000.0),
        ("3_15", 8, 2000.0),
        ("3_16", 10, 2880.0),
        ("3_29", 10, 2880.0),
    ] {
        let (t, m) = set_jewel_radii(v);
        assert_eq!((t.len(), m), (n, max), "{v}");
        assert!(
            t.iter()
                .all(|r| r.outer_squared == r.outer * r.outer && r.inner_squared == r.inner * r.inner)
        );
    }
}

#[test]
fn nodes_in_radius_equals_luajit() {
    let lj = fixture("radius-tree-3_13.json");
    let tree = load_passive_tree_mode("3_13", ParseMode::CacheOnly).unwrap();
    let table: Vec<(f64, f64, String)> = tree.jewel_radius.iter().map(|r| (r.inner, r.outer, r.label.clone())).collect();
    let lj_table: Vec<(f64, f64, String)> = lj["jewelRadius"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["inner"].as_f64().unwrap(),
                r["outer"].as_f64().unwrap(),
                r["label"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(table, lj_table, "data.jewelRadius of the 3_13 tree");

    let check = |what: &str, entries: &Json| -> (usize, usize) {
        let (mut nodes, mut lists) = (0, 0);
        for e in entries.as_array().unwrap() {
            let Some(id) = e["id"].as_u64() else { continue }; // legion keystones (string ids) come later, P5
            let id = id as u32;
            let ours = tree.nodes_in_radius(id).unwrap_or_else(|| panic!("{what} {id}: no nodesInRadius"));
            let theirs = e["radius"].as_array().unwrap();
            assert_eq!(ours.len(), theirs.len(), "{what} {id}: #nodesInRadius");
            for (r, (a, b)) in ours.iter().zip(theirs).enumerate() {
                let a: BTreeSet<u32> = a.iter().copied().collect();
                assert_eq!(a, ids(b), "{what} {id}: radius {}", r + 1);
                nodes += a.len();
                lists += 1;
            }
        }
        (lists, nodes)
    };
    let (sl, sn) = check("socket", &lj["sockets"]);
    let (kl, kn) = check("keystone", &lj["keystones"]);
    assert_eq!(lj["sockets"].as_array().unwrap().len(), tree.sockets.len(), "#sockets");
    println!(
        "3_13: sockets {} x radii = {sl} lists, {sn} ids; keystones {kl} lists, {kn} ids -- all = LuaJIT",
        tree.sockets.len()
    );
}
