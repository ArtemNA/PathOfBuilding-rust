//! Path equivalence of the character import (R8-ARCH §1.1, MINEFIELD R20): what the mutators leave = a fresh load of what `SaveDB` writes. The characters are the synthetic ones of the oracle's scenes
//! (`tools/sg-oracle/scenes/import/char-*.json`, written by hand, `gen_net.py`). What the original does with them is compared in `pob-ui/tests/import.rs`.

use pob_engine::engine::Engine;
use serde_json::Value;

fn scene_character(name: &str) -> Option<Value> {
    let p = format!("{}/../../tools/sg-oracle/scenes/import/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let sc: Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
    let text = sc["steps"]
        .as_array()?
        .iter()
        .find_map(|s| s["clipboard"].as_str().filter(|c| c.starts_with('{')))?;
    let v: Value = serde_json::from_str(text).ok()?;
    Some(v["character"].clone())
}

fn empty_build() -> Engine {
    let version = pob_data::tree::tree_versions().expect("GameVersions").latest_tree_version.clone();
    Engine::load_xml(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<PathOfBuilding>\n\t<Build level=\"1\" targetVersion=\"3_0\" characterLevelAutoMode=\"true\"/>\n\t<Tree activeSpec=\"1\">\n\t\t<Spec treeVersion=\"{version}\" classId=\"0\" ascendClassId=\"0\" secondaryAscendClassId=\"nil\" nodes=\"\"/>\n\t</Tree>\n</PathOfBuilding>"
    ))
    .expect("an empty build")
}

fn import(e: &mut Engine, c: &Value) {
    let carry = e.import_items_begin(true, true).expect("begin");
    e.import_items_body(c, false, None, carry).expect("items");
    let extra = e.import_extra_points();
    e.import_jewels(c, true, None).expect("jewels");
    let out = e.import_passive_tree(c, extra).expect("tree");
    e.import_set_level(out.level);
    e.set_character_level_auto_mode(false);
    e.refresh().expect("the frame's run");
}

/// What an import wrote: the tree, the sockets, the items, the groups (a load of a new build's XML fills attributes of the sets that a new build leaves nil: `title`, `useSecondWeaponSet`).
fn lines_without_stats(xml: &str) -> Vec<String> {
    let from = xml.find("<Items ").unwrap_or(0);
    let to = xml.find("<ItemSet ").unwrap_or(xml.len());
    let mut out: Vec<String> = xml[from..to].lines().map(str::to_owned).collect();
    out.extend(
        xml.lines()
            .filter(|l| {
                ["<Spec ", "<Socket ", "<Skill ", "<Gem ", "<Override "]
                    .iter()
                    .any(|k| l.contains(k))
            })
            .map(str::to_owned),
    );
    out.retain(|l| !l.contains("<Items "));
    // nil and its default are one value to the Lua that reads them, a load writes the default
    for l in &mut out {
        for (from, to) in [
            (" count=\"nil\"", " count=\"1\""),
            (" enableGlobal2=\"nil\"", " enableGlobal2=\"false\""),
            (" includeInFullDPS=\"nil\"", " includeInFullDPS=\"false\""),
            (" mainActiveSkill=\"nil\"", " mainActiveSkill=\"1\""),
            (" mainActiveSkillCalcs=\"nil\"", " mainActiveSkillCalcs=\"1\""),
        ] {
            *l = l.replace(from, to);
        }
    }
    out
}

#[test]
fn an_import_equals_a_load_of_what_it_saves() {
    let mut compared = 0;
    for name in ["char-new", "char-ruthless", "char-level-low"] {
        let Some(c) = scene_character(name) else {
            eprintln!("SKIPPED {name}: no scene");
            continue;
        };
        let mut e = empty_build();
        import(&mut e, &c);
        let saved = e.save_xml().expect("save");
        let mut fresh = Engine::load_xml(&saved).expect("load");
        fresh.refresh().expect("run");
        let (x, y) = (lines_without_stats(&fresh.save_xml().expect("save")), lines_without_stats(&saved));
        let diff: Vec<(&String, &String)> = x.iter().zip(&y).filter(|(p, q)| p != q).take(6).collect();
        assert!(x == y, "{name}: the build, lines {}/{}: {diff:#?}", x.len(), y.len());
        let stats = |e: &Engine| e.stat_list().clone();
        let (a, b) = (stats(&e), stats(&fresh));
        assert_eq!(format!("{a:?}"), format!("{b:?}"), "{name}: the stat list");
        compared += 1;
    }
    assert!(compared > 0, "nothing compared");
}

#[test]
fn replacing_by_id_keeps_the_items_and_the_groups() {
    let Some(c) = scene_character("char-new") else {
        eprintln!("SKIPPED: no scene");
        return;
    };
    let mut e = empty_build();
    import(&mut e, &c);
    let (items, groups) = (e.model().items.len(), e.socket_groups().len());
    assert!(items > 10 && groups > 2, "{items} items, {groups} groups");
    import(&mut e, &c);
    assert_eq!(e.model().items.len(), items, "the same ids replace");
    assert_eq!(e.socket_groups().len(), groups);
}

#[test]
fn a_missing_table_is_the_lua_error_and_leaves_the_engine_alive() {
    let mut e = empty_build();
    let err = e
        .import_jewels(&serde_json::json!({ "passives": {} }), true, None)
        .expect_err("copyTable(nil)");
    assert!(err.contains("copyTable"), "{err}");
    let err = e
        .import_passive_tree(&serde_json::json!({ "class": "Witch", "passives": { "hashes": [] } }), 0.0)
        .expect_err("charData.league:match");
    assert!(err.contains("league"), "{err}");
    e.refresh().expect("still runs");
}

/// `spec.jewel_data` + `hashes_ex`: the nodes of the graph that GGG's hashes name are allocated (PassiveSpec.lua:2070-2093); the scene's character names two of the ring.
#[test]
fn the_imported_cluster_hashes_allocate_nodes_of_the_graph() {
    let Some(c) = scene_character("char-new") else {
        eprintln!("SKIPPED: no scene");
        return;
    };
    let mut e = empty_build();
    import(&mut e, &c);
    let spec = e.spec_rc();
    let graphs = spec.sub_graph_views();
    assert_eq!(graphs.len(), 1);
    let on: Vec<u32> = graphs[0]
        .links
        .iter()
        .map(|l| &spec.nodes[l.0])
        .filter(|n| n.alloc)
        .map(|n| n.id)
        .collect();
    assert!(on.len() >= 2, "{on:?}");
    assert!(on.iter().all(|id| *id >= 65536), "{on:?}");
}
