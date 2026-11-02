//! R6-fix smoke: every version of GameVersions.lua `treeVersionList` (`data-patches/tree-versions.json`, taken from LuaJIT) has its
//! `treeVersions[ver].num` and builds a `PassiveTree` (`ParseMode::Full`, each in its own thread so a panic names its version and the
//! others still run). The list must equal `data/manifest.json` `treeVersionList` (the converter's copy).

use pob_tree::{ParseMode, PassiveTree};

#[test]
fn every_tree_version_builds() {
    let list = pob_data::tree::tree_version_list().unwrap();
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(pob_data::data_dir().join("manifest.json")).unwrap()).unwrap();
    let want: Vec<&str> = manifest["treeVersionList"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(list, want, "tree-versions.json vs data/manifest.json treeVersionList");
    let tv = pob_data::tree::tree_versions().unwrap();
    assert_eq!(tv.latest_tree_version, *list.last().unwrap());
    assert_eq!(tv.default_tree_version, list[1]);

    let mut failed = Vec::new();
    let mut built = 0;
    for v in &list {
        let ver = (*v).to_owned();
        let r = std::thread::spawn(move || -> Result<(usize, usize, f64), String> {
            let t = PassiveTree::build_mode(&ver, ParseMode::Full).map_err(|e| e.0)?;
            Ok((t.nodes.len(), t.classes.len(), t.version_num))
        })
        .join();
        match r {
            Ok(Ok((nodes, classes, num))) => {
                assert!(nodes > 1000 && classes >= 7, "{v}: {nodes} nodes, {classes} classes");
                assert_eq!(num, pob_data::tree::tree_version_num(v).unwrap());
                built += 1;
                println!("{v}: num {num}, {nodes} nodes, {classes} classes");
            }
            Ok(Err(e)) => failed.push(format!("{v}: Err {e}")),
            Err(p) => failed.push(format!(
                "{v}: panic {}",
                p.downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .unwrap_or_default()
            )),
        }
    }
    println!("tree versions built: {built}/{}", list.len());
    assert!(
        failed.is_empty(),
        "{} of {} versions fail:\n{}",
        failed.len(),
        list.len(),
        failed.join("\n")
    );
}

#[test]
fn unknown_version_names_it() {
    let e = pob_data::tree::tree_version_num("9_99").unwrap_err();
    assert!(e.0.contains("'9_99'"), "{e}");
}

/// R-tree-legacy: every tree of `treeVersionList` has LuaJIT's shape (`tests/fixtures/tree-shape.json`, `node
/// tools/lua-corpus/export-tree-shape.ts`): node / group counts and the node-id sum once the legacy alternate ascendancies are dropped
/// (PassiveTree.lua:132-176), the sockets (also as `socket_ids`, what ItemsTab makes its slots from), the alternate ascendancies left
/// with their start nodes and `secondaryAscendNameMap` (:208-225).
#[test]
fn tree_shape_matches_luajit() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tree-shape.json");
    let fx: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let trees = fx["trees"].as_array().unwrap();
    assert_eq!(
        trees.len(),
        pob_data::tree::tree_version_list().unwrap().len(),
        "a fixture per version"
    );
    let mut bad = Vec::new();
    for t in trees {
        let ver = t["ver"].as_str().unwrap().to_owned();
        let want = t.clone();
        let r = std::thread::spawn(move || -> Vec<String> {
            let tree = PassiveTree::build_mode(&ver, ParseMode::Full).unwrap();
            let mut bad = Vec::new();
            let mut check = |what: &str, rust: serde_json::Value, lua: &serde_json::Value| {
                if &rust != lua {
                    bad.push(format!("{ver} {what}: rust {rust} luajit {lua}"));
                }
            };
            check("nodes", tree.nodes.len().into(), &want["nodes"]);
            check(
                "nodeIdSum",
                tree.nodes.iter().map(|n| u64::from(n.id)).sum::<u64>().into(),
                &want["nodeIdSum"],
            );
            check("groups", tree.groups.len().into(), &want["groups"]);
            let mut sockets: Vec<u32> = tree.sockets.keys().copied().collect();
            sockets.sort_unstable();
            check("sockets", serde_json::json!(sockets), &want["sockets"]);
            check(
                "socket_ids",
                serde_json::json!(*pob_tree::tree::socket_ids(&ver).unwrap()),
                &want["sockets"],
            );
            let alt = tree.alternate_ascendancies.as_ref().map(|a| {
                let mut v: Vec<String> = a
                    .iter()
                    .map(|(k, c)| {
                        format!(
                            "{k}:{}:{}",
                            c.id.as_deref().unwrap_or("nil"),
                            c.start_node_id.map_or_else(|| "nil".into(), |s| s.to_string())
                        )
                    })
                    .collect();
                v.sort();
                v
            });
            check("alternateAscendancies", serde_json::json!(alt), &want["alternateAscendancies"]);
            let sec = tree.secondary_ascend_name_map.as_ref().map(|m| {
                let mut v: Vec<&String> = m.keys().collect();
                v.sort();
                v
            });
            check("secondaryAscendNames", serde_json::json!(sec), &want["secondaryAscendNames"]);
            let legacy = tree
                .nodes
                .iter()
                .filter(|n| matches!(n.ascendancy_name.as_deref(), Some("Warden" | "Warlock" | "Primalist")))
                .count();
            check("legacyLeft", legacy.into(), &want["legacyLeft"]);
            bad
        })
        .join()
        .unwrap();
        bad.extend(r);
    }
    println!("tree shapes: {} versions, {} differences", trees.len(), bad.len());
    assert!(bad.is_empty(), "tree shape != LuaJIT:\n{}", bad.join("\n"));
}

/// Perf: `PassiveTree` build and `socket_ids` (R-tree-legacy): `cargo test --release -p pob-tree --test tree_versions -- --ignored --nocapture`.
#[test]
#[ignore = "perf"]
fn tree_load_perf() {
    for ver in ["3_29", "3_23", "3_13"] {
        let mut ms: Vec<f64> = (0..10)
            .map(|_| {
                let t = std::time::Instant::now();
                std::hint::black_box(PassiveTree::build_mode(ver, ParseMode::Full).unwrap());
                t.elapsed().as_secs_f64() * 1000.0
            })
            .skip(1)
            .collect();
        ms.sort_by(f64::total_cmp);
        println!(
            "PassiveTree {ver}: median {:.2} ms, min {:.2} (9 runs after one warm-up)",
            ms[4], ms[0]
        );
    }
    let t = std::time::Instant::now();
    let n = pob_tree::tree::socket_ids("3_29").unwrap().len();
    println!(
        "socket_ids 3_29 (cold, tree JSON already parsed): {:.3} ms, {n} sockets",
        t.elapsed().as_secs_f64() * 1000.0
    );
}

/// Perf: the first `socket_ids` of a process parses the latest tree's JSON (what a build of an older tree newly pays once): run alone.
#[test]
#[ignore = "perf"]
fn socket_ids_cold_perf() {
    let t = std::time::Instant::now();
    let n = pob_tree::tree::socket_ids("3_29").unwrap().len();
    println!(
        "socket_ids 3_29 (cold, JSON parse included): {:.1} ms, {n} sockets",
        t.elapsed().as_secs_f64() * 1000.0
    );
}
