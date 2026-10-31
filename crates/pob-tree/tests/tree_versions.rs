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
