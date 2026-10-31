//! R5.5b gate against LuaJIT (`crates/pob-tree/tests/fixtures/radius-<build>.json`, producer `tools/lua-corpus/export-radius.ts`):
//! for every golden in `ParseMode::Full` (the build's own items, `PostLoad` cluster graphs) --
//!   * every jewel socket of the spec: `spec.nodes[socket].nodesInRadius` (each radius, as id SETS) -- `nil` on the sockets
//!     `BuildSubgraph` made, exactly as upstream;
//!   * `env.radiusJewelList` of a fresh `initEnv` (CalcSetup.lua:871-935): socket, `type`, item, node set, in list order; and the
//!     `env.extraRadiusNodeList` set;
//!   * the x/y (`%.17g`) of every node `BuildSubgraph` generated (R5.5a computed them with `f64::sin/cos`, MINEFIELD-RUST R2).
//!     A coordinate difference would be class `platform-libm` only with desktop-LuaJIT proof; measured: none.

use std::collections::BTreeSet;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_build::{active_spec, load_build_xml};
use pob_calc::{InitEnvOptions, ParseMode, init_env, item_options};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_items::Item;
use pob_lua::format_g;
use pob_tree::PassiveSpec;
use serde_json::Value as Json;

fn fixture(name: &str) -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pob-tree/tests/fixtures")
        .join(format!("radius-{}.json", name.replace(' ', "-")));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

fn ids(j: &Json) -> BTreeSet<u32> {
    j.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()
}

fn g17(x: Option<f64>) -> Json {
    x.map_or(Json::Null, |v| Json::String(format_g(v, 17)))
}

#[test]
fn radius_jewels_equal_luajit() {
    let mut rows = Vec::new();
    for g in GOLDEN_BUILDS {
        let name = g.name;
        let lj = fixture(name);
        let model = Rc::new(load_build_xml(&std::fs::read_to_string(g.xml_path()).unwrap()).unwrap());
        let mut items = IndexMap::new();
        for im in &model.items {
            let item = Item::parse(&im.raw, &item_options(im, ParseMode::Full));
            if item.base.is_some() {
                items.insert(im.id as u64, Rc::new(item));
            }
        }
        let spec = PassiveSpec::load_mode(active_spec(&model), ParseMode::Full).unwrap();
        let env = init_env(
            model.clone(),
            InitEnvOptions {
                spec: Some(spec),
                items: Some(items),
                parse_mode: ParseMode::Full,
            },
        )
        .unwrap();
        let spec = &env.spec;
        assert_eq!(lj["treeVersion"].as_str(), Some(spec.tree_version.as_str()), "{name}: tree version");

        // (1) nodesInRadius of every jewel socket
        let (mut lists, mut sockets) = (0, 0);
        for s in lj["sockets"].as_array().unwrap() {
            let id = s["node"].as_u64().unwrap() as u32;
            let node = spec.node(id);
            assert_eq!(node.is_some(), s["inSpec"].as_bool().unwrap(), "{name}: socket {id} in spec");
            let Some(node) = node else { continue };
            assert_eq!(
                node.is_subgraph_node(),
                s["generated"].as_bool().unwrap(),
                "{name}: socket {id} generated"
            );
            match (node.nodes_in_radius(), s["radius"].as_array()) {
                (None, None) => {}
                (Some(ours), Some(theirs)) => {
                    assert_eq!(ours.len(), theirs.len(), "{name}: socket {id} #nodesInRadius");
                    for (r, (a, b)) in ours.iter().zip(theirs).enumerate() {
                        assert_eq!(
                            a.iter().copied().collect::<BTreeSet<_>>(),
                            ids(b),
                            "{name}: socket {id} radius {}",
                            r + 1
                        );
                        lists += 1;
                    }
                }
                (a, b) => panic!(
                    "{name}: socket {id}: nodesInRadius rust {:?} vs luajit {:?}",
                    a.is_some(),
                    b.is_some()
                ),
            }
            sockets += 1;
        }

        // (2) env.radiusJewelList / env.extraRadiusNodeList
        let rads = lj["radiusJewelList"].as_array().unwrap();
        assert_eq!(env.radius_jewel_list.len(), rads.len(), "{name}: #radiusJewelList");
        for (i, (ours, theirs)) in env.radius_jewel_list.iter().zip(rads).enumerate() {
            assert_eq!(
                u64::from(ours.node_id),
                theirs["nodeId"].as_u64().unwrap(),
                "{name}: rad {i} nodeId"
            );
            assert_eq!(ours.ty.as_str(), theirs["type"].as_str(), "{name}: rad {i} type");
            assert_eq!(Some(ours.item.name.as_str()), theirs["item"].as_str(), "{name}: rad {i} item");
            assert_eq!(
                ours.nodes.iter().copied().collect::<BTreeSet<_>>(),
                ids(&theirs["nodes"]),
                "{name}: rad {i} nodes"
            );
        }
        let extra: BTreeSet<u32> = env.extra_radius_node_list.keys().copied().collect();
        assert_eq!(extra, ids(&lj["extraRadiusNodeList"]), "{name}: extraRadiusNodeList");
        let notes: Vec<&String> = env.unimplemented.iter().filter(|u| u.contains("radius jewel")).collect();
        assert!(notes.is_empty(), "{name}: {notes:?}");

        // (3) coordinates of the generated nodes
        let generated = lj["generated"].as_array().unwrap();
        let ours_gen: Vec<u32> = spec.nodes.iter().filter(|n| n.is_subgraph_node()).map(|n| n.id).collect();
        assert_eq!(
            ours_gen.iter().copied().collect::<BTreeSet<_>>(),
            generated.iter().map(|x| x["id"].as_u64().unwrap() as u32).collect(),
            "{name}: generated ids"
        );
        let mut coord_diff = Vec::new();
        for x in generated {
            let id = x["id"].as_u64().unwrap() as u32;
            let t = spec.node(id).unwrap().tree_node();
            let ours = [g17(t.x), g17(t.y), g17(t.angle)];
            let theirs = [x["x"].clone(), x["y"].clone(), x["angle"].clone()];
            if ours != theirs {
                coord_diff.push(format!("{id}: rust {ours:?} luajit {theirs:?}"));
            }
        }
        assert!(
            coord_diff.is_empty(),
            "{name}: generated coordinates differ:\n{}",
            coord_diff.join("\n")
        );
        rows.push(format!(
            "{name}: sockets {sockets} ({lists} radius lists) = LuaJIT; radiusJewelList {}, extra {}; generated coords {}/{} bit-identical",
            rads.len(),
            extra.len(),
            generated.len(),
            generated.len()
        ));
    }
    println!("{}", rows.join("\n"));
}
