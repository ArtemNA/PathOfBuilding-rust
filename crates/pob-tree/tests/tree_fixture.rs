//! R3.3 gate on the 6 goldens (R3-ARCH §3), fixtures from R3.1 (`crates/pob-calc/tests/fixtures/<build>.{pobts,luajit}.json`,
//! key `tree`; producer `tools/lua-corpus/export-env.ts`):
//!   (a) the Rust dump in the shape of the TS half of pob-ts `test/tree/spec.test.ts` (`tsTree`) = the pob-ts dump, exactly;
//!   (b) Rust <-> LuaJIT dump classified exactly as pob-ts classifies it (export-env.ts `selfTreeGlobal`/`selfTree`):
//!       cluster ids (>= 65536) / expansion sockets, node-miss (a ModCache miss on the node), tree-jewel (Lethal Pride, inside
//!       socket 61834's radius), keystoneMap legion keys (LuaJIT-only; R5.5c: each one is in `tree.legion().keystone_map`
//!       with LuaJIT's mods) -- per-build counts pinned, unclassified 0.
//! (a)/(b) run in `ParseMode::CacheOnly` (pob-ts). R5.4 (c): (b) in `ParseMode::Full` -- `node-miss` 0 (every node line
//! answered by the real parser equals LuaJIT), keystoneMap ModCache-miss 0, the rest pinned as in (b).
//! These specs never see the build's items, so `PostLoad` (cluster graphs) does not run and the cluster classes stay as
//! pinned; with the items they are 0 -- `pob-calc/tests/cluster_tree.rs` (R5.5a).

use std::collections::{BTreeMap, BTreeSet};

use pob_build::{active_spec, load_build_xml};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_tree::modtools::format_source_mod;
use pob_tree::{AllocOrder, ParseMode, PassiveSpec, PassiveTree, load_passive_tree_mode};
use serde_json::{Map, Value as Json, json};

fn fixture(name: &str, kind: &str) -> Json {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../pob-calc/tests/fixtures");
    let slug = name.replace(' ', "-");
    let whole = dir.join(format!("{slug}.{kind}.json"));
    let path = if whole.exists() {
        whole
    } else {
        dir.join(format!("{slug}.tree.{kind}.json"))
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut j: Json = serde_json::from_str(&text).unwrap();
    j["tree"].take()
}

fn spec_for(name: &str, order: AllocOrder) -> PassiveSpec {
    spec_for_mode(name, order, ParseMode::CacheOnly)
}

fn spec_for_mode(name: &str, order: AllocOrder, mode: ParseMode) -> PassiveSpec {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).unwrap();
    let m = load_build_xml(&std::fs::read_to_string(g.xml_path()).unwrap()).unwrap();
    let sm = active_spec(&m);
    let mut s = PassiveSpec::new(load_passive_tree_mode(&sm.tree_version, mode).unwrap()).unwrap();
    s.alloc_order = order;
    s.import_from_node_list(
        sm.class_id,
        sm.ascend_class_id,
        sm.secondary_ascend_class_id,
        &sm.nodes,
        &sm.mastery_effects,
    )
    .unwrap();
    s
}

fn fmt_list(mods: &[std::rc::Rc<pob_mods::Mod>]) -> Json {
    Json::Array(mods.iter().map(|m| Json::String(format_source_mod(m))).collect())
}

/// export-env.ts `tsTree` (the TS half of spec.test.ts DUMP).
fn dump(tree: &PassiveTree, spec: &PassiveSpec) -> Json {
    let mut alloc = Map::new();
    for n in spec.alloc_nodes() {
        let mut o = Map::new();
        o.insert("type".into(), json!(n.ty().as_str()));
        o.insert("dn".into(), json!(n.dn()));
        o.insert("mods".into(), fmt_list(&n.mod_list().mods));
        if let Some(k) = n.keystone_mod() {
            o.insert("ks".into(), json!(format_source_mod(k)));
        }
        alloc.insert(n.id.to_string(), Json::Object(o));
    }
    let keystones: Map<String, Json> = tree
        .keystone_map
        .iter()
        .map(|(dn, &i)| (dn.clone(), fmt_list(&tree.nodes[i].stats.mod_list.mods)))
        .collect();
    let notables: Map<String, Json> = tree
        .notable_map
        .iter()
        .map(|(dn, &i)| (dn.clone(), json!(tree.nodes[i].id)))
        .collect();
    let mut node_types: BTreeMap<&str, u32> = BTreeMap::new();
    let mut coords = Map::new();
    for n in &tree.nodes {
        *node_types.entry(n.ty.as_str()).or_default() += 1;
        if let (Some(x), Some(y)) = (n.x, n.y) {
            coords.insert(
                n.id.to_string(),
                json!(format!("{}|{}", pob_lua::format_g(x, 17), pob_lua::format_g(y, 17))),
            );
        }
    }
    json!({
        "alloc": alloc, "keystones": keystones, "notables": notables, "nodeTypes": node_types, "coords": coords,
        "nTree": tree.nodes.len(), "nSpec": spec.nodes.len(),
        "counts": [spec.allocated_notable_count, spec.allocated_keystone_count, spec.allocated_mastery_count, spec.allocated_mastery_type_count],
        "className": spec.cur_class_name, "ascendName": spec.cur_ascend_class_name,
    })
}

/// Exact JSON equality (numbers bit for bit); the first difference as a path.
fn diff(path: &str, want: &Json, got: &Json) -> Option<String> {
    match (want, got) {
        (Json::Number(a), Json::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            (a.to_bits() != b.to_bits()).then(|| format!("{path}: want {a:?}, got {b:?}"))
        }
        (Json::Array(a), Json::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!("{path}: want {} elements, got {} ({want} vs {got})", a.len(), b.len()));
            }
            a.iter()
                .zip(b)
                .enumerate()
                .find_map(|(i, (x, y))| diff(&format!("{path}[{i}]"), x, y))
        }
        (Json::Object(a), Json::Object(b)) => {
            if let Some(k) = a.keys().find(|k| !b.contains_key(*k)) {
                return Some(format!("{path}.{k}: missing"));
            }
            if let Some(k) = b.keys().find(|k| !a.contains_key(*k)) {
                return Some(format!("{path}.{k}: extra = {}", b[k.as_str()]));
            }
            a.iter().find_map(|(k, x)| diff(&format!("{path}.{k}"), x, &b[k]))
        }
        _ => (want != got).then(|| format!("{path}: want {want}, got {got}")),
    }
}

fn uncached(tree: &PassiveTree, id: u32) -> bool {
    let cache = pob_data::mod_cache::mod_cache();
    tree.node(id).is_some_and(|n| n.sd.iter().any(|l| cache.get(l).is_none()))
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Classes {
    cluster_ids: usize,
    expansion_sockets: usize,
    node_miss: usize,
    tree_jewel: usize,
    mods_identical: usize,
}

/// export-env.ts `selfTree`: Rust spec vs the LuaJIT dump. Returns the class counts; unclassified differences panic.
/// `full`: the real parser read the nodes, so a ModCache miss explains nothing (`node-miss` cannot occur).
fn classify(name: &str, tree: &PassiveTree, spec: &PassiveSpec, rust: &Json, lj: &Json, full: bool) -> Classes {
    let mut c = Classes::default();
    let lj_alloc = lj["alloc"].as_object().unwrap();
    let lj_ids: BTreeSet<u32> = lj_alloc.keys().map(|k| k.parse().unwrap()).collect();
    let rs_ids: BTreeSet<u32> = spec.alloc_nodes.keys().copied().collect();
    let only_rs: Vec<_> = rs_ids.difference(&lj_ids).collect();
    assert!(only_rs.is_empty(), "{name}: Rust-only alloc nodes {only_rs:?}");
    let mut unexplained: Vec<u32> = Vec::new();
    for &id in lj_ids.difference(&rs_ids) {
        if id >= 65536 {
            c.cluster_ids += 1;
        } else if tree.node(id).is_some() && spec.node(id).is_none() {
            c.expansion_sockets += 1;
        } else {
            unexplained.push(id);
        }
    }
    assert!(unexplained.is_empty(), "{name}: LuaJIT-only alloc nodes {unexplained:?}");
    let radius: Vec<Vec<u32>> = match &lj["radius"] {
        Json::Array(r) if name.contains("Lethal Pride") => r
            .iter()
            .map(|l| l.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect())
            .collect(),
        _ => Vec::new(),
    };
    let radius_all: BTreeSet<u32> = radius.iter().flatten().copied().collect();
    let mut timeless: Vec<u32> = Vec::new();
    for &id in &rs_ids {
        let (o, r) = (&lj_alloc[&id.to_string()], &rust["alloc"][id.to_string()]);
        assert_eq!(
            (&o["type"], &o["dn"], &o["ks"]),
            (&r["type"], &r["dn"], &r["ks"]),
            "{name}: node {id} type/dn/ks"
        );
        if o["mods"] == r["mods"] {
            c.mods_identical += 1;
        } else if !full && uncached(tree, id) {
            c.node_miss += 1;
        } else if radius_all.contains(&id) {
            c.tree_jewel += 1;
            timeless.push(id);
        } else {
            panic!(
                "{name}: node {id} mods differ, unexplained: rust {} vs luajit {}",
                r["mods"], o["mods"]
            );
        }
    }
    assert_eq!(
        (&rust["className"], &rust["ascendName"]),
        (&lj["className"], &lj["ascendName"]),
        "{name}: class names"
    );
    let counts: Vec<u64> = rust["counts"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
    let lj_counts: Vec<u64> = lj["counts"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
    if c.cluster_ids == 0 && c.expansion_sockets == 0 {
        assert_eq!(counts, lj_counts, "{name}: counters");
        assert_eq!(rust["nSpec"], lj["nSpec"], "{name}: spec.nodes size");
    } else {
        // The oracle counts the subgraph's notables too, and its spec.nodes grew by the subgraph.
        assert!(
            counts.iter().zip(&lj_counts).all(|(a, b)| a <= b),
            "{name}: counters {counts:?} vs {lj_counts:?}"
        );
        assert!(rust["nSpec"].as_u64() < lj["nSpec"].as_u64(), "{name}: spec.nodes not smaller");
    }
    if name.contains("Lethal Pride") {
        let r3: BTreeSet<u32> = radius.get(2).cloned().unwrap_or_default().into_iter().collect();
        assert!(
            timeless.iter().all(|id| r3.contains(id)),
            "{name}: timeless differences outside radius index 3"
        );
    }
    c
}

/// export-env.ts `selfTreeGlobal` (build-independent, once).
fn classify_global(tree: &PassiveTree, rust: &Json, lj: &Json) -> (usize, usize, usize) {
    assert_eq!(rust["nTree"], lj["nTree"]);
    assert_eq!(rust["nodeTypes"], lj["nodeTypes"]);
    assert_eq!(rust["notables"], lj["notables"], "notableMap");
    let (rc, lc) = (rust["coords"].as_object().unwrap(), lj["coords"].as_object().unwrap());
    let coords_same = lc.iter().filter(|(k, v)| rc.get(*k) == Some(v)).count();
    assert_eq!(
        coords_same,
        lc.len(),
        "coords bit-identical (%.17g); a difference would be class platform-libm"
    );
    assert_eq!(rc.len(), lc.len());
    let (mut legion, mut cache_miss, mut same) = (0, 0, 0);
    for (dn, mods) in lj["keystones"].as_object().unwrap() {
        match tree.keystone_map.get(dn) {
            None => {
                // R5.5c: a legacy legion keystone (PassiveTree.lua:737-739) -- in the tree's legion map, with LuaJIT's mods.
                let lk = tree.legion().unwrap().keystone_map.get(dn).copied();
                let ours = lk.map(|i| fmt_list(&tree.legion().unwrap().nodes[i].stats.mod_list.mods));
                assert_eq!(ours.as_ref(), Some(mods), "legion keystone {dn}");
                legion += 1;
            }
            Some(_) if &rust["keystones"][dn] == mods => same += 1,
            Some(&i) if uncached(tree, tree.nodes[i].id) => cache_miss += 1,
            Some(_) => panic!("keystone {dn}: mods differ, unexplained"),
        }
    }
    (legion, cache_miss, same)
}

#[test]
fn tree_dump_equals_pob_ts_and_classifies_against_luajit() {
    // (cluster ids, expansion sockets, node-miss, tree-jewel) per build -- R3.1 report.
    let pinned: [(&str, [usize; 4]); 6] = [
        ("Dual Savior", [0, 0, 1, 0]),
        ("Dual Wield Cospris CoC", [16, 6, 10, 0]),
        ("Dual Wield Cospris CoC - Lethal Pride", [16, 6, 10, 14]),
        ("Generals Perforate Zerker", [21, 8, 17, 0]),
        ("Mirage Archer Toxic Rain", [10, 4, 12, 0]),
        ("OccVortex", [0, 0, 11, 0]),
    ];
    let tree = load_passive_tree_mode("3_13", ParseMode::CacheOnly).unwrap();
    for (i, g) in GOLDEN_BUILDS.iter().enumerate() {
        let spec = spec_for(g.name, AllocOrder::IdAsc);
        let rust = dump(&tree, &spec);
        if let Some(d) = diff("$", &fixture(g.name, "pobts"), &rust) {
            panic!("{}: Rust dump != pob-ts dump: {d}", g.name);
        }
        let lj = fixture(g.name, "luajit");
        if i == 0 {
            let (legion, miss, same) = classify_global(&tree, &rust, &lj);
            eprintln!("keystoneMap: LuaJIT-only (legion) {legion}, common identical {same}, ModCache-miss {miss}, unexplained 0");
            assert_eq!((legion, same + miss), (22, 90));
        }
        let c = classify(g.name, &tree, &spec, &rust, &lj, false);
        eprintln!("{}: {c:?} (alloc rust {})", g.name, spec.alloc_nodes.len());
        let got = [c.cluster_ids, c.expansion_sockets, c.node_miss, c.tree_jewel];
        assert_eq!((g.name, got), pinned[i], "class counts");
    }
}

/// R5.4 gate (c): the LuaJIT classification with the real parser behind `ProcessStats`.
#[test]
fn tree_full_classifies_against_luajit_node_miss_zero() {
    // (cluster ids, expansion sockets, node-miss, tree-jewel) per build
    let pinned: [(&str, [usize; 4]); 6] = [
        ("Dual Savior", [0, 0, 0, 0]),
        ("Dual Wield Cospris CoC", [16, 6, 0, 0]),
        ("Dual Wield Cospris CoC - Lethal Pride", [16, 6, 0, 17]),
        ("Generals Perforate Zerker", [21, 8, 0, 0]),
        ("Mirage Archer Toxic Rain", [10, 4, 0, 0]),
        ("OccVortex", [0, 0, 0, 0]),
    ];
    let tree = load_passive_tree_mode("3_13", ParseMode::Full).unwrap();
    for (i, g) in GOLDEN_BUILDS.iter().enumerate() {
        let spec = spec_for_mode(g.name, AllocOrder::IdAsc, ParseMode::Full);
        let rust = dump(&tree, &spec);
        let lj = fixture(g.name, "luajit");
        if i == 0 {
            let (legion, miss, same) = classify_global(&tree, &rust, &lj);
            eprintln!("Full keystoneMap: LuaJIT-only (legion) {legion}, common identical {same}, ModCache-miss {miss}");
            assert_eq!((legion, same, miss), (22, 90, 0));
        }
        let c = classify(g.name, &tree, &spec, &rust, &lj, true);
        eprintln!("Full {}: {c:?} (alloc rust {})", g.name, spec.alloc_nodes.len());
        let got = [c.cluster_ids, c.expansion_sockets, c.node_miss, c.tree_jewel];
        assert_eq!((g.name, got), pinned[i], "class counts (Full)");
    }
}

/// D13: the allocated set does not depend on the order `pairs(allocNodes)` is replaced by.
#[test]
fn alloc_set_independent_of_order() {
    for g in GOLDEN_BUILDS {
        let set = |o| {
            let mut v: Vec<u32> = spec_for(g.name, o).alloc_nodes.keys().copied().collect();
            v.sort_unstable();
            v
        };
        let a = set(AllocOrder::IdAsc);
        assert_eq!(a, set(AllocOrder::IdDesc), "{}", g.name);
        assert_eq!(a, set(AllocOrder::Insertion), "{}", g.name);
    }
}
