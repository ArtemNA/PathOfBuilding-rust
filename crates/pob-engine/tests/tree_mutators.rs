//! U3a: path equivalence of the tree view's mutators (R8-ARCH §1.1): `Engine::alloc_node` / `dealloc_node` + `refresh` (the `buildFlag`
//! branch of Build.lua:1258-1271) ≡ a fresh load of the XML with the same state, all output keys bit for bit; deallocating what was
//! allocated returns to the base bit for bit. On the golden + the 3.29 builds of `builds/pc` (skipped when absent: personal, not in git).

use std::path::PathBuf;

use pob_engine::engine::Engine;
use pob_tree::NodeType;

fn inputs() -> Vec<(String, String)> {
    let upstream: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/upstream.json")).expect("upstream"),
    )
    .expect("json");
    let root = upstream["pobRoot"].as_str().expect("pobRoot");
    let mut out = Vec::new();
    let golden = format!("{root}/spec/TestBuilds/3.13/Generals Perforate Zerker.xml");
    out.push(("golden".to_string(), std::fs::read_to_string(golden).expect("golden")));
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../builds/pc/3.29"));
    for name in ["build-a", "build-f", "build-g", "build-e"] {
        if let Ok(x) = std::fs::read_to_string(dir.join(format!("{name}.xml"))) {
            out.push((name.to_string(), x));
        }
    }
    out
}

/// The `buildFlag` cycle (MINEFIELD R20): an auto-level move recalculates once more.
fn settle(e: &mut Engine) {
    let before = e.model().level;
    e.refresh().expect("refresh");
    if e.model().character_level_auto_mode && e.model().level != before {
        e.refresh().expect("refresh");
    }
}

fn keys(e: &Engine) -> String {
    format!("{:?}", e.result().stats)
}

fn alloc_ids(e: &Engine) -> Vec<u32> {
    let spec = e.spec_rc();
    let mut v: Vec<u32> = spec.alloc_nodes.keys().copied().collect();
    v.sort_unstable();
    v
}

#[test]
fn path_equivalence_of_the_tree_mutators() {
    let (mut checks, mut skipped_cluster) = (0, 0);
    for (name, xml) in inputs() {
        let mut base = Engine::load_xml(&xml).expect("load");
        settle(&mut base);
        let base_xml = base.save_xml().expect("save");
        let base_stats = keys(&Engine::load_xml(&base_xml).expect("reload"));
        // every 200th reachable node: path lengths 1 and up, ascendancy and tree nodes
        let spec = base.spec_rc();
        let picks: Vec<u32> = spec
            .nodes
            .iter()
            .filter(|n| !n.alloc && n.path.is_some() && n.ty() != NodeType::Mastery && !n.is_subgraph_node())
            .map(|n| n.id)
            .step_by(200)
            .take(8)
            .collect();
        assert!(!picks.is_empty(), "{name}: no reachable node");
        for id in picks {
            let mut e = Engine::load_xml(&base_xml).expect("load");
            let before = alloc_ids(&e);
            match e.alloc_node(id, None) {
                Ok(()) => {}
                Err(err) if err.contains("unimplemented") => {
                    skipped_cluster += 1;
                    continue;
                }
                Err(err) => panic!("{name} {id}: {err}"),
            }
            // the live spec changes at once (the frame goes on with it), the output with the next frame's refresh
            assert!(e.spec_rc().node(id).expect("node").alloc, "{name} {id}: allocated");
            settle(&mut e);
            let saved = e.save_xml().expect("save");
            let fresh = Engine::load_xml(&saved).expect("fresh");
            assert_eq!(keys(&e), keys(&fresh), "{name} {id}: API = a fresh load");
            assert_eq!(alloc_ids(&e), alloc_ids(&fresh), "{name} {id}: allocNodes");
            checks += 1;

            // back: the nodes the allocation added, leaf first
            let added: Vec<u32> = alloc_ids(&e).into_iter().filter(|i| !before.contains(i)).collect();
            e.dealloc_node(id).expect("dealloc");
            settle(&mut e);
            let still: Vec<u32> = alloc_ids(&e).into_iter().filter(|i| added.contains(i)).collect();
            for rest in still {
                if e.spec_rc().node(rest).expect("node").alloc {
                    e.dealloc_node(rest).expect("dealloc");
                }
            }
            settle(&mut e);
            let fresh = Engine::load_xml(&e.save_xml().expect("save")).expect("fresh");
            assert_eq!(keys(&fresh), keys(&e), "{name} {id}: API = a fresh load after dealloc");
            if alloc_ids(&e) == before {
                assert_eq!(keys(&fresh), base_stats, "{name} {id}: round trip");
            }
            checks += 1;
        }
    }
    eprintln!("tree mutators: {checks} path-equivalence checks, {skipped_cluster} allocations over a cluster socket (unimplemented)");
    assert!(checks > 0);
}

/// A trace path (`tracePath`, PassiveTreeView.lua:514): `AllocNode(node, altPath)` allocates the given nodes, not `node.path`.
#[test]
fn alloc_with_an_alternate_path() {
    let (_, xml) = inputs().remove(0);
    let mut e = Engine::load_xml(&xml).expect("load");
    let spec = e.spec_rc();
    let n = spec
        .nodes
        .iter()
        .find(|n| !n.alloc && n.path.as_ref().is_some_and(|p| p.len() >= 2) && n.ty() == NodeType::Normal)
        .expect("a node");
    let alt: Vec<u32> = n.path.as_ref().expect("path").iter().map(|i| spec.nodes[*i].id).collect();
    let id = n.id;
    e.alloc_node(id, Some(&alt)).expect("alloc");
    for a in &alt {
        assert!(e.spec_rc().node(*a).expect("node").alloc);
    }
    settle(&mut e);
    let fresh = Engine::load_xml(&e.save_xml().expect("save")).expect("fresh");
    assert_eq!(keys(&e), keys(&fresh));
}
