//! The pure checks of pob-ts `test/tree/{tree,spec}.test.ts` (no oracle): node types, maps, coordinates, links, the
//! ConnectedTo flags, parse coverage, Init, the Dual Savior allocation, orphan pruning and the URL refusal.

use pob_build::{NumMap, SpecModel, active_spec, load_build_xml};
use pob_golden::registry::find_golden;
use pob_tree::modtools::format_source_mod;
use pob_tree::{NodeType, ParseMode, PassiveSpec, PassiveTree, calc_orbit_angles, load_passive_tree_mode};

const DUAL_SAVIOR_ALLOC: [u32; 8] = [29294, 30335, 31628, 32480, 33988, 40535, 47175, 58449];

fn cached(line: &str) -> bool {
    pob_data::mod_cache::mod_cache().get(line).is_some()
}

#[test]
fn tree_3_13_types_classes_maps() {
    let t = load_passive_tree_mode("3_13", ParseMode::CacheOnly).unwrap();
    assert_eq!(t.nodes.len(), 2706);
    assert!(t.node(0).is_none());
    let count = |ty| t.nodes.iter().filter(|n| n.ty == ty).count();
    let want = [
        (NodeType::ClassStart, 7),
        (NodeType::AscendClassStart, 19),
        (NodeType::Keystone, 45),
        (NodeType::Mastery, 189),
        (NodeType::Normal, 1561),
        (NodeType::Notable, 828),
        (NodeType::Socket, 57),
    ];
    for (ty, n) in want {
        assert_eq!(count(ty), n, "{ty:?}");
    }
    assert_eq!(t.classes.len(), 7);
    assert_eq!(t.classes[0].name, "Scion");
    assert_eq!(t.classes[6].name, "Shadow");
    assert_eq!(t.classes[1].start_node_id, Some(47175));
    assert_eq!(t.class_name_map["Marauder"], 1);
    assert_eq!(t.classes[1].classes[0].name, "None");
    assert_eq!(t.classes[1].classes[2].name, "Berserker");
    let b = t.ascend_name_map["Berserker"];
    assert_eq!((b.class_id, b.ascend_class_id), (1, 2));
    assert_eq!(t.classes[1].classes[2].start_node_id, Some(29294));
    assert_eq!(t.node(29294).unwrap().ty, NodeType::AscendClassStart);
    assert_eq!(t.keystone_map.len(), 90);
    assert_eq!(t.keystone_map.values().collect::<std::collections::HashSet<_>>().len(), 45);
    assert_eq!(t.keystone_map["Resolute Technique"], t.keystone_map["resolute technique"]);
    assert_eq!(t.notable_map.len(), 685);
    assert_eq!(t.nodes[t.notable_map["twin terrors"]].id, 6);
    // :583-588 duplicate rule: the node WITH a group wins.
    assert_eq!(t.nodes[t.notable_map["no witnesses"]].id, 24067);
    assert_eq!(t.nodes[t.notable_map["intensity"]].id, 37403);
    assert_eq!(t.sockets.len(), 57);
    assert_eq!(t.mastery_effects.len(), 0);
}

#[test]
fn tree_3_13_node_6_stats_coords_links_flags() {
    let t = load_passive_tree_mode("3_13", ParseMode::CacheOnly).unwrap();
    let n = t.node(6).unwrap();
    assert_eq!(n.dn, "Twin Terrors");
    assert_eq!(n.ty, NodeType::Notable);
    assert!(cached(&n.sd[0]) && !cached(&n.sd[1]));
    let mods: Vec<String> = n.stats.mod_list.mods.iter().map(|m| format_source_mod(m)).collect();
    assert_eq!(mods, ["12|Tree:6|Accuracy|INC|-|-|type=Condition/var=DualWielding"]);
    assert!(n.stats.unknown);
    assert_eq!(n.stats.mod_key, "[12 = Accuracy|INC|-|-|type=Condition/var=DualWielding]");
    // x/y from the oracle, %.17g -- bit for bit.
    assert_eq!(pob_lua::format_g(n.x.unwrap(), 17), "3615.3099999999999");
    assert_eq!(pob_lua::format_g(n.y.unwrap(), 17), "4767.0038845869212");
    assert_eq!(n.angle, Some(calc_orbit_angles(12.0)[11]));
    assert_eq!(n.linked_id, [25209]);
    assert_eq!(t.node(47175).unwrap().linked_id, [31628, 50904, 17765, 24704, 29294]);
    let flagged: Vec<String> = t
        .node(31628)
        .unwrap()
        .stats
        .mod_list
        .mods
        .iter()
        .filter(|m| &*m.name == "Condition:ConnectedToMarauderStart")
        .map(|m| format_source_mod(m))
        .collect();
    assert_eq!(flagged, ["true|Tree:31628|Condition:ConnectedToMarauderStart|FLAG|-|-|-"]);
    assert!(t.node(29294).unwrap().stats.mod_list.mods.is_empty());
    let k = t.node(t.nodes[t.keystone_map["resolute technique"]].id).unwrap();
    assert_eq!(
        format_source_mod(k.stats.keystone_mod.as_ref().unwrap()),
        format!("Resolute Technique|Tree{}|Keystone|LIST|-|-|-", k.id)
    );
}

#[test]
fn orbit_angles() {
    assert_eq!(calc_orbit_angles(1.0).len(), 2);
    assert_eq!(calc_orbit_angles(12.0).len(), 13);
    assert_eq!(calc_orbit_angles(16.0).len(), 16);
    assert_eq!(calc_orbit_angles(40.0).len(), 40);
    assert_eq!(calc_orbit_angles(12.0)[6], std::f64::consts::PI);
}

#[test]
fn unknown_version_and_every_committed_tree() {
    let e = load_passive_tree_mode("3_99", ParseMode::CacheOnly).err().unwrap();
    assert!(e.0.contains("version '3_99'"), "{e}"); // W4: names the version
    for v in pob_data::tree::COMMITTED_TREE_VERSIONS {
        let t = load_passive_tree_mode(v, ParseMode::CacheOnly).unwrap();
        assert!(t.nodes.len() > 1000, "{v}");
        assert_eq!(t.classes.len(), 7, "{v}");
        assert_eq!(t.classes[1].name, "Marauder", "{v}");
        assert!(t.classes[1].start_node_id.is_some(), "{v}");
    }
    let legacy = load_passive_tree_mode("3_6", ParseMode::CacheOnly).unwrap();
    assert_eq!(legacy.version_num, 3.06);
    assert_eq!(legacy.node(47175).unwrap().ty, NodeType::ClassStart);
    assert_eq!(legacy.classes[1].start_node_id, Some(47175));
    assert_eq!(legacy.node(44683).unwrap().class_start_index, Some(6.0));
}

/// pob-ts tree.test.ts pins: 3_13 is built in a fresh thread, so the parse counters are its own.
#[test]
fn parse_coverage_3_13() {
    std::thread::spawn(|| {
        pob_parser::reset_parse_mod_stats();
        let t = PassiveTree::build_mode("3_13", ParseMode::CacheOnly).unwrap();
        let s = pob_parser::parse_mod_stats();
        let mut lines: Vec<&str> = t.nodes.iter().flat_map(|n| n.sd.iter().map(String::as_str)).collect();
        lines.sort_unstable();
        lines.dedup();
        assert_eq!(lines.len(), 2437);
        assert_eq!(lines.iter().filter(|l| !cached(l)).count(), 810);
        assert_eq!((s.hits, s.misses), (3930, 2041));
        assert_eq!(t.nodes.iter().filter(|n| n.sd.iter().any(|l| !cached(l))).count(), 709);
        assert_eq!(t.nodes.iter().filter(|n| n.stats.unknown).count(), 748);
        assert_eq!(t.nodes.iter().filter(|n| n.stats.extra).count(), 32);
    })
    .join()
    .unwrap();
}

/// R5.4: the same tree in `ParseMode::Full` -- every cache miss goes to the real parser, none fails.
#[test]
fn parse_coverage_3_13_full() {
    std::thread::spawn(|| {
        pob_parser::reset_parse_mod_stats();
        let t = PassiveTree::build_mode("3_13", ParseMode::Full).unwrap();
        let s = pob_parser::parse_mod_stats();
        let unknown = t.nodes.iter().filter(|n| n.stats.unknown).count();
        let extra = t.nodes.iter().filter(|n| n.stats.extra).count();
        println!(
            "Full: hits {} parsed {} misses {} errors {:?}; unknown {unknown} extra {extra}",
            s.hits, s.parsed, s.misses, s.error_lines
        );
        assert_eq!((s.hits, s.parsed, s.misses), (3930, 1184, 0));
        assert!(s.error_lines.is_empty());
        assert_eq!((unknown, extra), (UNKNOWN_FULL, EXTRA_FULL));
    })
    .join()
    .unwrap();
}

const UNKNOWN_FULL: usize = 134;
const EXTRA_FULL: usize = 73;

fn model(nodes: &[u32]) -> SpecModel {
    SpecModel {
        title: None,
        tree_version: "3_13".into(),
        cluster_hash_format_version: 2.0,
        class_id: 1.0,
        ascend_class_id: 2.0,
        secondary_ascend_class_id: Some(0.0),
        nodes: nodes.iter().map(|&n| f64::from(n)).collect(),
        mastery_effects: NumMap::default(),
        jewels: NumMap::default(),
        url: None,
        overrides: Vec::new(),
    }
}

fn sorted_alloc(s: &PassiveSpec) -> Vec<u32> {
    let mut v: Vec<u32> = s.alloc_nodes.keys().copied().collect();
    v.sort_unstable();
    v
}

#[test]
fn spec_init() {
    let t = load_passive_tree_mode("3_13", ParseMode::CacheOnly).unwrap();
    let s = PassiveSpec::new(t.clone()).unwrap();
    assert_eq!(s.nodes.len(), 2237);
    assert!(s.node(33).is_none()); // cluster-jewel notable, no group
    assert!(s.node(22994).is_none()); // group 543 is a proxy
    assert!(s.node(64583).is_none()); // socket with expansionJewel.parent
    assert!(s.node(6).is_some());
    assert_eq!(s.cur_class_name, "Scion");
    assert_eq!(s.cur_ascend_class_name, "None");
    assert_eq!(
        s.alloc_nodes.keys().copied().collect::<Vec<_>>(),
        [t.classes[0].start_node_id.unwrap()]
    );
    let start = s.node(47175).unwrap();
    assert_eq!(
        start.linked.iter().map(|&i| s.nodes[i].id).collect::<Vec<_>>(),
        [31628, 50904, 17765, 24704, 29294]
    );
}

#[test]
fn spec_dual_savior() {
    let g = find_golden("Dual Savior").unwrap();
    let m = load_build_xml(&std::fs::read_to_string(g.xml_path()).unwrap()).unwrap();
    let sm = active_spec(&m);
    let mut nodes: Vec<f64> = sm.nodes.clone();
    nodes.sort_by(f64::total_cmp);
    assert_eq!(nodes, DUAL_SAVIOR_ALLOC.map(f64::from));
    let s = PassiveSpec::load_mode(sm, ParseMode::CacheOnly).unwrap();
    assert_eq!(sorted_alloc(&s), DUAL_SAVIOR_ALLOC);
    assert_eq!(
        (s.cur_class_name.as_str(), s.cur_ascend_class_name.as_str()),
        ("Marauder", "Berserker")
    );
    let counts = (
        s.allocated_notable_count,
        s.allocated_keystone_count,
        s.allocated_mastery_count,
        s.allocated_mastery_type_count,
    );
    assert_eq!(counts, (1, 0, 0, 0));
    let c = s.count_alloc_nodes();
    assert_eq!((c.used, c.asc_used, c.secondary_asc_used, c.sockets), (6, 0, 0, 0));
    assert!(s.alloc_subgraph_nodes.is_empty());
    let n = s.node(58449).unwrap();
    assert!(std::ptr::eq(n.mod_list(), &n.tree_node().stats.mod_list)); // reads through until written
}

#[test]
fn spec_orphan_pruning() {
    let mut list = DUAL_SAVIOR_ALLOC.to_vec();
    list.extend([50904, 4184]);
    let s = PassiveSpec::load_mode(&model(&list), ParseMode::CacheOnly).unwrap();
    assert!(s.node(4184).is_some());
    assert!(!s.node(4184).unwrap().alloc);
    let mut want = DUAL_SAVIOR_ALLOC.to_vec();
    want.push(50904);
    want.sort_unstable();
    assert_eq!(sorted_alloc(&s), want);
}

#[test]
fn spec_refuses_url_only() {
    let mut m = model(&[]);
    m.url = Some("AAAABAMAAA==".into());
    let e = PassiveSpec::load_mode(&m, ParseMode::CacheOnly).err().unwrap();
    assert!(e.0.contains("unimplemented: legacy URL spec"), "{e}");
}
