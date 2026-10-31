//! R5.5a gate: the spec after `PostLoad` (`BuildClusterJewelGraphs`, `ParseMode::Full`, the build's own items) against
//! the LuaJIT tree dump of the R3.1 fixtures (`tests/fixtures/<build>.luajit.json`, key `tree`; producer
//! `tools/lua-corpus/export-env.ts`). Lives here, not in pob-tree, because the jewels come from pob-items.
//!
//! Exact: the allocated set (no LuaJIT-only ids >= 65536, no LuaJIT-only expansion sockets -- the `cluster ids` /
//! `expansion` classes of `pob-tree/tests/tree_fixture.rs` are 0), every allocated node's type/dn/keystone mod, the
//! counters, `#spec.nodes`. Mods: identical, except the nodes the Lethal Pride conquest rewrites (radius index 3 of
//! socket 61834 -- R5.5c), counted as `tree-jewel` and pinned.

use std::collections::BTreeSet;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_build::{active_spec, load_build_xml};
use pob_calc::ParseMode;
use pob_calc::init_env::{item_options, post_load_spec};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_items::Item;
use pob_tree::modtools::format_source_mod;
use pob_tree::{PassiveSpec, SpecNode};
use serde_json::{Value as Json, json};

fn lj_tree(name: &str) -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.luajit.json", name.replace(' ', "-")));
    let mut j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    j["tree"].take()
}

fn spec_after_post_load(name: &str) -> PassiveSpec {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).unwrap();
    let model = load_build_xml(&std::fs::read_to_string(g.xml_path()).unwrap()).unwrap();
    let mut items = IndexMap::new();
    for im in &model.items {
        let item = Item::parse(&im.raw, &item_options(im, ParseMode::Full));
        if item.base.is_some() {
            items.insert(im.id as u64, Rc::new(item));
        }
    }
    let mut spec = PassiveSpec::load_mode(active_spec(&model), ParseMode::Full).unwrap();
    post_load_spec(&mut spec, &items).unwrap();
    spec
}

/// export-env.ts `tsTree` `alloc` entry.
fn node_dump(n: &SpecNode) -> Json {
    let mods: Vec<String> = n.mod_list().mods.iter().map(|m| format_source_mod(m)).collect();
    let mut o = json!({ "type": n.ty().as_str(), "dn": n.dn(), "mods": mods });
    if let Some(k) = n.keystone_mod() {
        o["ks"] = json!(format_source_mod(k));
    }
    o
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Counts {
    /// Allocated subgraph nodes (ids >= 65536) / expansion sockets put back, all equal to LuaJIT.
    generated: usize,
    sockets: usize,
    identical: usize,
    tree_jewel: usize,
}

#[test]
fn cluster_graphs_equal_luajit_tree() {
    // (generated ids, expansion sockets, mods identical, tree-jewel) -- R5.5a measured.
    let pinned: [(&str, [usize; 4]); 6] = [
        ("Dual Savior", [0, 0, 8, 0]),
        ("Dual Wield Cospris CoC", [16, 6, 120, 0]),
        ("Dual Wield Cospris CoC - Lethal Pride", [16, 6, 103, 17]),
        ("Generals Perforate Zerker", [21, 8, 126, 0]),
        ("Mirage Archer Toxic Rain", [10, 4, 117, 0]),
        ("OccVortex", [0, 0, 132, 0]),
    ];
    for (name, want) in pinned {
        let spec = spec_after_post_load(name);
        let lj = lj_tree(name);
        let lj_alloc = lj["alloc"].as_object().unwrap();
        let lj_ids: BTreeSet<u32> = lj_alloc.keys().map(|k| k.parse().unwrap()).collect();
        let rs_ids: BTreeSet<u32> = spec.alloc_nodes.keys().copied().collect();
        assert_eq!(
            rs_ids.symmetric_difference(&lj_ids).collect::<Vec<_>>(),
            Vec::<&u32>::new(),
            "{name}: allocNodes"
        );
        let radius3: BTreeSet<u32> = if name.contains("Lethal Pride") {
            lj["radius"][2]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as u32)
                .collect()
        } else {
            BTreeSet::new()
        };
        let mut c = Counts::default();
        for &id in &rs_ids {
            let n = spec.node(id).unwrap();
            if id >= 65536 {
                c.generated += 1;
            } else if n.is_subgraph_node() {
                c.sockets += 1;
            }
            let (r, o) = (node_dump(n), &lj_alloc[&id.to_string()]);
            assert_eq!(
                (&r["type"], &r["dn"], &r["ks"]),
                (&o["type"], &o["dn"], &o["ks"]),
                "{name}: node {id} type/dn/ks"
            );
            if r["mods"] == o["mods"] {
                c.identical += 1;
            } else if radius3.contains(&id) && !n.is_subgraph_node() {
                c.tree_jewel += 1;
            } else {
                panic!("{name}: node {id} mods: rust {} vs luajit {}", r["mods"], o["mods"]);
            }
        }
        let counts = json!([
            spec.allocated_notable_count,
            spec.allocated_keystone_count,
            spec.allocated_mastery_count,
            spec.allocated_mastery_type_count
        ]);
        assert_eq!(counts, lj["counts"], "{name}: counters");
        assert_eq!(json!(spec.nodes.len()), lj["nSpec"], "{name}: #spec.nodes");
        let notes: Vec<&String> = spec
            .unimplemented
            .iter()
            .filter(|u| u.contains("D13") || u.contains("ShowErrMsg"))
            .collect();
        eprintln!(
            "{name}: {c:?} (alloc {}, nSpec {}), legacy-conversion notes {notes:?}",
            rs_ids.len(),
            spec.nodes.len()
        );
        // No D13 collision in the rebuilt legacy maps, no "unexpected node counts" message box.
        assert!(notes.is_empty(), "{name}: {notes:?}");
        let got = [c.generated, c.sockets, c.identical, c.tree_jewel];
        assert_eq!((name, got), (name, want), "class counts");
    }
}

/// D13: with the subgraphs in, the allocated set still does not depend on the order `pairs(allocNodes)` is replaced by.
#[test]
fn cluster_alloc_set_independent_of_order() {
    for g in GOLDEN_BUILDS {
        let set = |order: pob_tree::AllocOrder| {
            let model = load_build_xml(&std::fs::read_to_string(g.xml_path()).unwrap()).unwrap();
            let items: IndexMap<u64, Rc<Item>> = model
                .items
                .iter()
                .map(|im| (im.id as u64, Rc::new(Item::parse(&im.raw, &item_options(im, ParseMode::Full)))))
                .filter(|(_, i)| i.base.is_some())
                .collect();
            let mut spec = PassiveSpec::load_mode(active_spec(&model), ParseMode::Full).unwrap();
            spec.alloc_order = order;
            post_load_spec(&mut spec, &items).unwrap();
            let mut v: Vec<u32> = spec.alloc_nodes.keys().copied().collect();
            v.sort_unstable();
            v
        };
        let a = set(pob_tree::AllocOrder::IdAsc);
        assert_eq!(a, set(pob_tree::AllocOrder::IdDesc), "{}", g.name);
        assert_eq!(a, set(pob_tree::AllocOrder::Insertion), "{}", g.name);
    }
}
