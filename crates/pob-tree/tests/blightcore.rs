//! Putrid Seed (07fa089bf, upstream #10466): the data the port reads and the `pairs(self.nodes)` order of
//! PassiveTree.lua:680-700. Pure checks on `data/` (no data-patches): the full graph (`BuildBlightcoreSubgraph`) needs a
//! LuaJIT capture with a seed build.

use std::collections::BTreeMap;

use pob_data::tree::TreeJson;
use pob_tree::tree::tree_nodes_pairs_order;

fn tree_3_30() -> TreeJson {
    let path = pob_data::data_dir().join("tree").join("3_30.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

/// ClusterJewels.lua:1053-1078 `blightcoreNotableIndex` reaches the loader (`deny_unknown_fields` would refuse it otherwise).
#[test]
fn blightcore_notable_index_loads() {
    let cj = pob_data::cluster_jewels::cluster_jewels();
    let map = cj.blightcore_notable_index.as_ref().expect("blightcoreNotableIndex");
    assert_eq!(map.len(), 24);
    // ClusterJewels.lua:1054-1057, :1077
    assert_eq!(cj.blightcore_notable_index(12922), Some(1.0));
    assert_eq!(cj.blightcore_notable_index(40214), Some(2.0));
    assert_eq!(cj.blightcore_notable_index(39736), Some(3.0));
    assert_eq!(cj.blightcore_notable_index(30872), Some(4.0));
    assert_eq!(cj.blightcore_notable_index(2062), None);
}

/// `node.proxyKey` survives the tree loader: 6 layouts of 12 proxy nodes on 3_30.
#[test]
fn proxy_key_loads() {
    let t = tree_3_30();
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for n in t.nodes.values() {
        if let Some(k) = n.proxy_key.as_deref().filter(|k| k.starts_with("Blightcore")) {
            assert_eq!(n.is_proxy, Some(true), "{k}");
            *count.entry(k).or_default() += 1;
        }
    }
    assert_eq!(count.len(), 6);
    assert!(count.values().all(|&c| c == 12), "{count:?}");
}

/// The order `blightcoreNodes[proxyKey]` gets (PassiveTree.lua:682-687): LuaJIT's `pairs` of the tree.lua constructor without
/// the "root" collision. Pinned from a separate Python port of `lj_tab_newkey` run over the keys of
/// `TreeData/3_30/tree.lua` in file order (not from this crate); none of them is ascending id.
#[test]
fn blightcore_layout_order_3_30() {
    let t = tree_3_30();
    let order = tree_nodes_pairs_order(&t).unwrap();
    assert_eq!(order.len(), t.nodes.len() - 1, "every node but \"root\"");
    let layout = |key: &str| -> Vec<u32> {
        order
            .iter()
            .copied()
            .filter(|id| t.nodes[id.to_string().as_str()].proxy_key.as_deref() == Some(key))
            .collect()
    };
    let want: [(&str, [u32; 12]); 6] = [
        (
            "BlightcoreBottomLeft",
            [5428, 27212, 25220, 17101, 5375, 20723, 2062, 14289, 52611, 45087, 12572, 29007],
        ),
        (
            "BlightcoreRight",
            [2326, 48156, 17936, 31962, 7233, 36235, 64658, 64773, 30372, 5947, 55337, 13120],
        ),
        (
            "BlightcoreTopRight",
            [59983, 4802, 61095, 10198, 39342, 18705, 58887, 51283, 48147, 19818, 23095, 18097],
        ),
        (
            "BlightcoreTopLeft",
            [13560, 46696, 27715, 24089, 42461, 5792, 57893, 57131, 63437, 48042, 49749, 21894],
        ),
        (
            "BlightcoreBottomRight",
            [52114, 18229, 11849, 64935, 60919, 44094, 58805, 15941, 12406, 62507, 22863, 55954],
        ),
        (
            "BlightcoreLeft",
            [19352, 40214, 18424, 44752, 30872, 12922, 60681, 39736, 34897, 43606, 59330, 18075],
        ),
    ];
    for (key, ids) in want {
        assert_eq!(layout(key), ids, "{key}");
    }
}
