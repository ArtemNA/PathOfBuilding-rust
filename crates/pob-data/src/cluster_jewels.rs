//! `data.clusterJewels` (Modules/Data.lua:860-900) -- pob-ts `src/data/clusterJewels.ts`. Item.lua reads `jewels[baseName]`;
//! the rest is the tree's business (cluster graphs are out of R3 scope, R3-ARCH §2.6) and stays untyped.

use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value as Json;

use crate::wire::OrderedMap;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClusterJewelSkill {
    pub name: String,
    pub icon: String,
    pub tag: String,
    pub stats: Vec<String>,
    pub enchant: Option<Vec<String>>,
    pub mastery_icon: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClusterJewelInfo {
    pub size: String,
    pub size_index: f64,
    pub min_nodes: f64,
    pub max_nodes: f64,
    /// Upstream spelling (`Indicies`).
    pub total_indicies: f64,
    pub small_indicies: Vec<f64>,
    pub notable_indicies: Vec<f64>,
    pub socket_indicies: Vec<f64>,
    pub skills: OrderedMap<ClusterJewelSkill>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClusterJewels {
    pub jewels: OrderedMap<ClusterJewelInfo>,
    pub keystones: Vec<String>,
    pub notable_sort_order: OrderedMap<f64>,
    pub orbit_offsets: Json,
    pub info_for_notable: Json,
    /// ClusterJewels.lua:1053 (07fa089bf): Putrid Seed proxy node id -> the index (1..4) of the jewel's "1 added passive skill
    /// is X" line that node allocates (PassiveSpec.lua:1929). Keyed by the node id as a string (JSON); see
    /// [`ClusterJewels::blightcore_notable_index`].
    pub blightcore_notable_index: Option<OrderedMap<f64>>,
}

impl ClusterJewels {
    /// `data.clusterJewels.jewels[baseName]`.
    pub fn jewel(&self, base_name: &str) -> Option<&ClusterJewelInfo> {
        self.jewels.get(base_name)
    }

    /// `notableIndex and notableIndex[treeNode.id]` (PassiveSpec.lua:1929-1934, 07fa089bf).
    pub fn blightcore_notable_index(&self, node_id: u32) -> Option<f64> {
        self.blightcore_notable_index.as_ref()?.get(node_id.to_string().as_str()).copied()
    }
}

pub fn cluster_jewels() -> &'static ClusterJewels {
    static C: OnceLock<ClusterJewels> = OnceLock::new();
    C.get_or_init(|| crate::load("cluster-jewels.json"))
}
