//! U3b: path equivalence of the tree tab's mutators (`src/tree_tab.rs`): the mutator + `refresh` ≡ a fresh load of what `SaveDB` writes (all output keys, the
//! allocation, the class, the masteries and tattoos). On the public golden only.

use pob_engine::engine::Engine;

fn golden() -> String {
    let upstream: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/upstream.json")).expect("upstream"),
    )
    .expect("json");
    let root = upstream["pobRoot"].as_str().expect("pobRoot");
    std::fs::read_to_string(format!("{root}/spec/TestBuilds/3.13/Generals Perforate Zerker.xml")).expect("golden")
}

fn settle(e: &mut Engine) {
    let before = e.model().level;
    e.refresh().expect("refresh");
    if e.model().character_level_auto_mode && e.model().level != before {
        e.refresh().expect("refresh");
    }
}

fn state(e: &Engine) -> String {
    let s = e.spec_rc();
    let mut ids: Vec<u32> = s.alloc_nodes.keys().copied().collect();
    ids.sort_unstable();
    let mut m: Vec<(u32, u64)> = s.mastery_selections.iter().map(|(k, v)| (*k, *v as u64)).collect();
    m.sort_unstable();
    let mut o: Vec<(u32, String)> = s.hash_overrides.iter().map(|(k, t)| (*k, t.dn.clone())).collect();
    o.sort();
    format!(
        "{:?} | {ids:?} | {m:?} | {o:?} | {:?} {:?} {:?} {} | {} {}",
        e.result().stats,
        s.cur_class_id,
        s.cur_ascend_class_id,
        s.cur_secondary_ascend_class_id,
        s.tree_version,
        e.active_spec(),
        e.spec_list_labels().join(",")
    )
}

/// `f`, the `buildFlag` frame, then: state == a fresh load of the save.
fn check(e: &mut Engine, what: &str, f: impl FnOnce(&mut Engine)) {
    f(e);
    settle(e);
    let fresh = Engine::load_xml(&e.save_xml().expect("save")).expect("fresh");
    assert_eq!(state(e), state(&fresh), "{what}");
}

#[test]
fn path_equivalence_of_the_tab_mutators() {
    let mut e = Engine::load_xml(&golden()).expect("load");
    settle(&mut e);
    let latest = pob_data::tree::tree_versions().expect("versions").latest_tree_version.clone();

    check(&mut e, "convert (copy)", |e| {
        e.convert_to_version(&latest, false, false).expect("convert");
    });
    assert_eq!(e.spec_count(), 2);
    assert_eq!(e.active_spec(), 2);
    check(&mut e, "set_active_spec 1", |e| e.set_active_spec(1).expect("set"));
    check(&mut e, "set_active_spec 2", |e| e.set_active_spec(2).expect("set"));

    // the mastery: a selection, the allocation of its path
    check(&mut e, "save_mastery", |e| {
        let spec = e.spec_rc();
        let n = spec
            .nodes
            .iter()
            .find(|n| !n.alloc && n.ty() == pob_tree::NodeType::Mastery && n.path.is_some() && !n.tree_node().mastery_effect_ids.is_empty())
            .expect("a mastery");
        let id = n.id;
        let effect = e.mastery_choices(id)[0].id;
        e.save_mastery(id, effect, None).expect("mastery");
        assert!(e.spec_rc().node(id).expect("node").alloc);
    });
    // a different effect on the allocated mastery
    check(&mut e, "save_mastery again", |e| {
        let (id, cur) = {
            let spec = e.spec_rc();
            let (k, v) = spec.mastery_selections.iter().next().expect("selected");
            (*k, *v)
        };
        let other = e.mastery_choices(id).iter().find(|c| c.id != cur).expect("another effect").id;
        e.save_mastery(id, other, None).expect("mastery");
    });

    // tattoos
    let keystone = {
        let spec = e.spec_rc();
        spec.nodes
            .iter()
            .find(|n| n.ty() == pob_tree::NodeType::Keystone && e.node_can_tattoo(n.id))
            .expect("a keystone")
            .id
    };
    let choices = e.tattoo_choices(keystone, false).expect("choices");
    assert!(!choices.is_empty());
    check(&mut e, "add_tattoo", |e| e.add_tattoo(keystone, &choices[0].key).expect("tattoo"));
    let (count, _) = e.tattoo_count();
    assert_eq!(count, 1);
    check(&mut e, "remove_tattoo", |e| e.remove_tattoo(keystone).expect("remove"));
    check(&mut e, "add_tattoo 2", |e| e.add_tattoo(keystone, &choices[0].key).expect("tattoo"));
    check(&mut e, "remove_all_tattoos", |e| e.remove_all_tattoos().expect("remove all"));

    // undo state
    let st = e.create_undo_state();
    check(&mut e, "ascendancy", |e| e.select_ascend_class(1.0).expect("ascend"));
    check(&mut e, "restore", |e| e.restore_undo_state(&st).expect("restore"));

    // the class: connected or not, ConnectToClass
    let other = (1..e.spec_rc().tree.classes.len())
        .find(|c| Some(*c) != e.spec_rc().cur_class_id)
        .expect("a class");
    let connected = e.is_class_connected(other);
    assert!(!connected);
    check(&mut e, "connect_to_class", |e| {
        assert!(e.connect_to_class(other).expect("connect"));
        assert!(e.is_class_connected(other));
    });
    check(&mut e, "select_class", |e| e.select_class(other as f64).expect("class"));

    // the spec list
    let copy = e.copy_spec_model(1).expect("copy");
    check(&mut e, "push_spec", |e| e.push_spec(copy));
    check(&mut e, "rename", |e| e.rename_spec(3, "Named"));
    check(&mut e, "move", |e| e.move_spec(3, 1));
    assert_eq!(e.spec_list_labels()[0], "[3.13] Named");
    check(&mut e, "delete", |e| {
        e.delete_spec(1).expect("delete");
    });
    let new_spec = e.new_spec_model();
    check(&mut e, "new", |e| e.push_spec(new_spec));
    check(&mut e, "convert all", |e| e.convert_all_to_version(&latest).expect("all"));

    // reset: the live spec keeps what `ResetNodes` leaves; the numbers are the fresh load's
    e.reset_tree().expect("reset");
    settle(&mut e);
    let fresh = Engine::load_xml(&e.save_xml().expect("save")).expect("fresh");
    assert_eq!(format!("{:?}", e.result().stats), format!("{:?}", fresh.result().stats), "reset");
}

/// `AllocNode` / `DeallocNode` over a cluster socket (PassiveSpec.lua:836-838, :861-862) on a golden with cluster jewels, if a socket qualifies.
#[test]
fn cluster_socket_rebuilds_the_graphs() {
    let mut e = Engine::load_xml(&golden()).expect("load");
    settle(&mut e);
    let spec = e.spec_rc();
    let socket = spec
        .nodes
        .iter()
        .find(|n| n.alloc && n.tree_node().expansion_jewel.is_some() && !n.is_subgraph_node())
        .map(|n| n.id);
    drop(spec);
    let Some(socket) = socket else {
        eprintln!("no allocated cluster socket in the golden");
        return;
    };
    let before = e.spec_rc().nodes.len();
    e.dealloc_node(socket).expect("dealloc");
    let after = e.spec_rc().nodes.len();
    eprintln!("cluster socket {socket}: {before} -> {after} spec nodes");
    settle(&mut e);
    let fresh = Engine::load_xml(&e.save_xml().expect("save")).expect("fresh");
    assert_eq!(state(&e), state(&fresh), "dealloc over the socket");
    e.alloc_node(socket, None).expect("alloc");
    settle(&mut e);
    let fresh = Engine::load_xml(&e.save_xml().expect("save")).expect("fresh");
    assert_eq!(state(&e), state(&fresh), "alloc over the socket");
    eprintln!("cluster socket back: {} spec nodes", e.spec_rc().nodes.len());
}

/// `DecodeURL` / `EncodeURL` round trip.
#[test]
fn tree_url_round_trip() {
    let mut e = Engine::load_xml(&golden()).expect("load");
    let latest = pob_data::tree::tree_versions().expect("versions").latest_tree_version.clone();
    e.convert_to_version(&latest, true, false).expect("convert");
    settle(&mut e);
    let url = e
        .encode_tree_url("https://www.pathofexile.com/passive-skill-tree/3.29.0/")
        .expect("encode");
    let m = Engine::decode_tree_url(&url, &latest, "Decoded").expect("decode").expect("valid");
    let s = e.spec_rc();
    assert_eq!(m.class_id as usize, s.cur_class_id.expect("class"));
    assert_eq!(m.ascend_class_id as usize, s.cur_ascend_class_id.expect("ascend"));
    // `AllocateDecodedNodes`: a new spec has no subgraph nodes yet (PassiveSpec.lua:343-347)
    let mut want: Vec<u32> = s
        .alloc_nodes()
        .filter(|n| !n.ty().is_start() && !n.is_subgraph_node() && n.id < 65536)
        .map(|n| n.id)
        .collect();
    want.sort_unstable();
    let mut got: Vec<u32> = m.nodes.iter().map(|n| *n as u32).collect();
    got.sort_unstable();
    assert_eq!(got, want);
    assert!(Engine::decode_tree_url("https://x/AAAA", &latest, "t").expect("decode").is_err());
    assert!(
        Engine::decode_tree_url("https://x/AAAABwEAAAAA", &latest, "t")
            .expect("decode")
            .is_err()
    );
}

/// `DecodePoePlannerURL` (PassiveSpec.lua:395-495): little endian; the version table decides; the screen gate is `tab-cover` of tests/tree.rs.
#[test]
fn poe_planner_url_decodes() {
    let u16le = |v: u16| v.to_le_bytes();
    let mut b: Vec<u8> = vec![0; 6];
    b.extend(u16le(51));
    b.extend([1, 2, 0]);
    b.extend(u16le(2));
    b.extend(u16le(476));
    b.extend(u16le(31961));
    b.extend(u16le(0));
    b.extend(u16le(0));
    b.extend(u16le(1));
    b.extend(u16le(292));
    b.extend(u16le(31822));
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut text = String::new();
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..=c.len() {
            text.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    let url = format!("https://poeplanner.com/{text}");
    let versions = |v: u32| (v == 51).then(|| "3_29".to_string());
    assert_eq!(Engine::poe_planner_version(&url, &versions).as_deref(), Ok("3_29"));
    assert!(
        Engine::poe_planner_version("https://poeplanner.com/AAAA", &versions)
            .expect_err("short")
            .contains("unrecognised")
    );
    let m = Engine::decode_poe_planner_url(&url, "3_29", "P").expect("decode");
    assert_eq!((m.class_id, m.ascend_class_id), (1.0, 2.0));
    assert_eq!(m.nodes.iter().copied().filter(|n| [476.0, 31961.0, 292.0].contains(n)).count(), 3);
    assert_eq!(m.mastery_effects.0, vec![(292.0, 31822.0)]);
    // a cut link reads past the end: Lua's arithmetic error
    let cut = format!("https://poeplanner.com/{}", &text[..24]);
    assert!(Engine::decode_poe_planner_url(&cut, "3_29", "P").is_err());
}
