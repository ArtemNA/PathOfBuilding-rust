//! `BuildModel` = pob-ts `loadBuildXml` on the 6 goldens (fixtures from `tools/lua-corpus/export-build-model.ts`): the
//! serialized forms must be equal -- same keys, same strings/booleans, numbers bit for bit. Plus the hand-verified counts of
//! pob-ts `test/build/load.test.ts`.

use pob_build::*;
use pob_golden::registry::GOLDEN_BUILDS;
use serde_json::Value as Json;

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.model.json"))
}

/// Exact equality of two JSON trees; the first difference as a path.
fn diff(path: &str, want: &Json, got: &Json) -> Option<String> {
    match (want, got) {
        (Json::Number(a), Json::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            (a.to_bits() != b.to_bits()).then(|| format!("{path}: want {a:?}, got {b:?}"))
        }
        (Json::Array(a), Json::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!("{path}: want {} elements, got {}", a.len(), b.len()));
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

#[test]
fn model_equals_pob_ts_for_6_goldens() {
    for g in GOLDEN_BUILDS {
        let xml = std::fs::read_to_string(g.xml_path()).unwrap_or_else(|e| panic!("{}: {e}", g.name));
        let model = load_build_xml(&xml).unwrap_or_else(|e| panic!("{}: {e}", g.name));
        let got = serde_json::to_value(&model).unwrap();
        let want: Json = serde_json::from_str(&std::fs::read_to_string(fixture_path(g.name)).unwrap()).unwrap();
        if let Some(d) = diff("$", &want, &got) {
            panic!("{}: {d}", g.name);
        }
    }
}

/// W12: a load leaves nothing behind -- A then B equals B alone.
#[test]
fn no_shared_state_between_loads() {
    let a = std::fs::read_to_string(GOLDEN_BUILDS[0].xml_path()).unwrap();
    let b = std::fs::read_to_string(GOLDEN_BUILDS[1].xml_path()).unwrap();
    let _ = load_build_xml(&a).unwrap();
    let after = serde_json::to_value(load_build_xml(&b).unwrap()).unwrap();
    let alone = serde_json::to_value(load_build_xml(&b).unwrap()).unwrap();
    assert_eq!(after, alone);
}

struct Expected {
    level: f64,
    items: usize,
    groups: usize,
    gems: usize,
    nodes: usize,
    class_id: f64,
    ascend_class_id: f64,
    class_name: &'static str,
    ascend_name: &'static str,
    main_socket_group: f64,
    major: &'static str,
    minor: &'static str,
}

/// pob-ts `test/build/load.test.ts` EXPECTED (hand-verified against the XMLs).
fn expected(name: &str) -> Expected {
    let e = |level, items, groups, gems, nodes, class_id, ascend_class_id, class_name, ascend_name, main_socket_group| Expected {
        level,
        items,
        groups,
        gems,
        nodes,
        class_id,
        ascend_class_id,
        class_name,
        ascend_name,
        main_socket_group,
        major: "None",
        minor: "None",
    };
    match name {
        "Dual Savior" => e(1.0, 3, 4, 5, 8, 1.0, 2.0, "Marauder", "Berserker", 3.0),
        "Dual Wield Cospris CoC" | "Dual Wield Cospris CoC - Lethal Pride" => e(100.0, 28, 9, 26, 130, 2.0, 1.0, "Ranger", "Raider", 5.0),
        "Generals Perforate Zerker" => e(93.0, 29, 10, 27, 126, 1.0, 2.0, "Marauder", "Berserker", 3.0),
        "Mirage Archer Toxic Rain" => Expected {
            major: "Arakaali",
            minor: "Shakari",
            ..e(100.0, 22, 8, 30, 133, 2.0, 2.0, "Ranger", "Deadeye", 1.0)
        },
        "OccVortex" => e(99.0, 18, 6, 23, 132, 3.0, 1.0, "Witch", "Occultist", 5.0),
        _ => panic!("unknown golden {name}"),
    }
}

#[test]
fn counts_match_pob_ts_load_test() {
    let tree = pob_data::tree::load_tree("3_13").unwrap();
    for g in GOLDEN_BUILDS {
        let m = load_build_xml(&std::fs::read_to_string(g.xml_path()).unwrap()).unwrap();
        let want = expected(g.name);
        let spec = active_spec(&m);
        let groups = active_skill_groups(&m);
        assert_eq!(m.level, want.level, "{}", g.name);
        assert_eq!(m.main_socket_group, want.main_socket_group, "{}", g.name);
        assert_eq!(
            (m.pantheon_major_god.as_str(), m.pantheon_minor_god.as_str()),
            (want.major, want.minor),
            "{}",
            g.name
        );
        assert_eq!(
            (spec.class_id, spec.ascend_class_id),
            (want.class_id, want.ascend_class_id),
            "{}",
            g.name
        );
        assert_eq!(spec.nodes.len(), want.nodes, "{}", g.name);
        assert_eq!(m.items.len(), want.items, "{}", g.name);
        assert_eq!(groups.len(), want.groups, "{}", g.name);
        assert_eq!(groups.iter().map(|g| g.gems.len()).sum::<usize>(), want.gems, "{}", g.name);
        assert_eq!(m.unknown_sections, ["Import", "Notes", "TreeView"], "{}", g.name);
        assert!(m.has_config_section, "{}: D17 -- every golden has <Config>", g.name);
        let meta = meta_of(&m, tree);
        assert_eq!(
            (meta.class_name.as_str(), meta.ascend_name.as_str()),
            (want.class_name, want.ascend_name),
            "{}",
            g.name
        );
        if g.name == "Dual Savior" {
            assert_eq!(spec.nodes, [30335.0, 33988.0, 47175.0, 32480.0, 29294.0, 40535.0, 58449.0, 31628.0]);
            assert!(spec.jewels.is_empty());
            assert!(m.items[0].raw.trim_start().starts_with("Rarity: UNIQUE"));
            let set = active_item_set(&m);
            assert_eq!(set.slots["Weapon 1"].item_id, 1.0);
            assert_eq!(set.slots["Weapon 2"].item_id, 3.0);
            assert_eq!(groups[2].source.as_deref(), Some("Item:1:The Saviour, Legion Sword"));
            assert_eq!(groups[2].gems[0].name_spec, "Reflection");
            assert!(!groups[0].gems[1].enabled);
            assert_eq!(active_config(&m).input.get("conditionLeeching"), Some(&InputValue::Bool(true)));
            assert_eq!(m.calcs_input.len(), 2);
        }
    }
}

/// Lua-nuance unit cases on hand-written XML (pob-ts semantics where it is the spec; upstream lines cited).
#[test]
fn lua_nuances() {
    let m = load_build_xml(
        r#"<PathOfBuilding><Build level="0" mainSocketGroup="x" bandit="Alira"/>
        <Skills><Skill enabled="true"><Gem nameSpec="Mj&#246;lner &lt;b&gt;" enabled="false" enableGlobal1="nope" count="0"/></Skill></Skills>
        <Config><Input name="enemyIsBoss" string="uber atziri"/><Input name="presetBossSkills" string="Uber Atziri Flameblast"/>
        <Placeholder name="p" string="s"/><Placeholder name="q" number="3"/><Input name="customMods" string="+1 to Life"/></Config></PathOfBuilding>"#,
    )
    .unwrap();
    assert_eq!(m.level, 0.0, "L4: tonumber(x) or 1 keeps 0");
    assert_eq!(m.main_socket_group, 1.0);
    assert_eq!(m.bandit, "Alira", "kept in the model; D17 loss is the config state's (R3.5)");
    let gem = &active_skill_groups(&m)[0].gems[0];
    assert_eq!(gem.name_spec, "Mjlner ", "&#246; erased (D22), <b> stripped by sanitiseText");
    assert!(!gem.enabled && !gem.enable_global1 && gem.count == 0.0);
    let c = active_config(&m);
    assert_eq!(
        c.input["enemyIsBoss"],
        InputValue::Str("Boss".into()),
        "title-case then Uber Atziri -> Boss"
    );
    assert_eq!(c.input["presetBossSkills"], InputValue::Str("Atziri Flameblast".into()));
    assert_eq!(
        c.input["p"],
        InputValue::Str("s".into()),
        "ConfigTab.lua:876 Placeholder string -> input"
    );
    assert_eq!(c.placeholder["q"], 3.0);
    assert!(!c.input.contains_key("customMods"));
    assert_eq!(c.custom_mods_list[0].text, "+1 to Life", "legacy customMods migration :923-935");
    let empty = load_build_xml("<PathOfBuilding/>").unwrap();
    assert!(!empty.has_config_section);
    assert_eq!(empty.config.sets.len(), 1);
    assert_eq!(empty.tree.specs.len(), 1);
    assert_eq!(empty.item_sets.len(), 1);
}

/// The comparator itself is strict: a 1-ulp number, a missing key, an extra key and a type change are all caught.
#[test]
fn comparator_is_strict() {
    let want: Json = serde_json::from_str(r#"{"a":[1,0.1,"x"],"b":{"c":true}}"#).unwrap();
    assert!(diff("$", &want, &want.clone()).is_none());
    for got in [
        r#"{"a":[1,0.10000000000000002,"x"],"b":{"c":true}}"#,
        r#"{"a":[1,0.1,"x"],"b":{}}"#,
        r#"{"a":[1,0.1,"x"],"b":{"c":true,"d":1}}"#,
        r#"{"a":[1,0.1,"x"],"b":{"c":"true"}}"#,
        r#"{"a":[1,0.1],"b":{"c":true}}"#,
    ] {
        assert!(diff("$", &want, &serde_json::from_str(got).unwrap()).is_some(), "{got}");
    }
}
