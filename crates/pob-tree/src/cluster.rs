//! Cluster jewels -- `Classes/PassiveSpec.lua` :1734-2360: `BuildClusterJewelGraphs` (:1806), the legacy (v1) cluster-hash
//! conversion (:1734-1804, :1903-1968), `FindClusterSocket` (:1883) and `BuildSubgraph` (:1970), line for line; `PostLoad`
//! (:251) is the only caller on the load path. `AllocNode`/`DeallocNode` (:813/:838, tree editing) are not ported in this
//! crate, so their two calls do not exist here; `build_cluster_jewel_graphs` is re-entrant (it drops and remakes every
//! subgraph exactly like upstream) for when they are.
//!
//! The jewels come from the items tab, which this crate does not know: the caller hands a lookup `itemId -> what
//! BuildSubgraph reads off the item` ([`ClusterJewel`]). Display-only parts are left out on purpose: connectors
//! (`BuildConnector`), sprites, `searchStrCached`. `self.jewel_data`/`self.extended_hashes` (ImportTab's character import,
//! P5) are never set on the XML load path, so `importedNodes`/`importedGroups` are empty and `addToAllocatedSubgraphNodes`
//! is always false -- see `add_to_allocated_subgraph_nodes`.
//!
//! D13: the `pairs` walks here go in a fixed order (ascending socket id, ascending template index, insertion order); where
//! the order could change a RESULT (two keys colliding in a rebuilt map) the collision is reported in `unimplemented`
//! rather than resolved silently.

use std::collections::BTreeMap;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_build::NumMap;
use pob_data::cluster_jewels::{ClusterJewelInfo, cluster_jewels};
use pob_mods::{ModType, Value, create_mod};

use crate::spec::{PassiveSpec, SpecNode};
use crate::tree::{NodeStats, NodeType, PassiveTree, StatHolder, TreeError, TreeNode, as_id, node_ref_id, process_stats};

const SPEC: &str = "Classes/PassiveSpec.lua";

/// What `BuildSubgraph` reads off a socketed jewel: `jewel.clusterJewel` and the `jewel.jewelData.clusterJewel*` fields
/// (Item.lua `BuildModList`, :2495-2520).
#[derive(Clone, Debug, Default)]
pub struct ClusterJewel {
    /// `jewel.clusterJewel` (`data.clusterJewels.jewels[baseName]`).
    pub cluster_jewel: Option<&'static ClusterJewelInfo>,
    /// `jewelData.clusterJewelValid` is truthy.
    pub valid: bool,
    pub keystone: Option<String>,
    pub notables: Vec<String>,
    pub skill: Option<String>,
    pub socket_count_override: Option<f64>,
    pub socket_count: Option<f64>,
    pub node_count: Option<f64>,
    pub nothingness_count: Option<f64>,
    pub added_mods: Vec<String>,
    pub inc_effect: Option<f64>,
}

/// `GetJewel(itemId)` (:901-910): the item when it exists and has `jewelData`, else nil.
pub type JewelLookup<'a> = &'a dyn Fn(f64) -> Option<Rc<ClusterJewel>>;

/// `self.subGraphs[id]` -- the parts `BuildClusterJewelGraphs` reads back (nodes, parentSocket, entranceNode).
#[derive(Clone)]
pub(crate) struct SubGraph {
    /// Indices into `PassiveSpec::nodes`, `subGraph.nodes` order.
    nodes: Vec<usize>,
    parent_socket: usize,
    entrance: Option<usize>,
}

fn err(line: u32, what: impl std::fmt::Display) -> TreeError {
    TreeError(format!("{SPEC}:{line}: {what}"))
}

/// `group.n` (PassiveTree.lua:502 makes it `group.nodes` on >= 3.10) as ids, `ipairs` order; `tonumber(nodeId)`.
fn group_node_ids(tree: &PassiveTree, g: i64) -> Vec<Option<u32>> {
    let Some(grp) = tree.json.groups.index(g) else {
        return Vec::new();
    };
    if tree.version_num >= 3.1 {
        grp.nodes.iter().flatten().map(node_ref_id).collect()
    } else {
        let mut out = Vec::new();
        if let Some(n) = &grp.n {
            let mut i = 1;
            while let Some(&id) = n.index(i) {
                out.push(as_id(id));
                i += 1;
            }
        }
        out
    }
}

/// `nodeId + i` with a template index (an integral Lua number).
fn offset_id(base: u32, i: i64) -> Result<u32, TreeError> {
    u32::try_from(i64::from(base) + i).map_err(|_| err(2232, format!("node id {base} + {i} out of range")))
}

impl PassiveSpec {
    /// :251 `PostLoad` -- what `Build.lua:680` runs after every tab loaded.
    pub fn post_load(&mut self, get_jewel: JewelLookup<'_>) -> Result<(), TreeError> {
        self.build_cluster_jewel_graphs(get_jewel)?;
        self.post_loaded = true;
        Ok(())
    }

    /// :1734-1740
    fn begin_legacy_cluster_hash_conversion(&mut self) -> bool {
        let needs = self.cluster_hash_format_version < 2.0;
        self.legacy_cluster_node_map = needs.then(IndexMap::new);
        self.legacy_cluster_node_map_reverse = needs.then(IndexMap::new);
        needs
    }

    /// :1744-1752
    fn register_legacy_cluster_node_map(&mut self, legacy: u32, current: u32) {
        let Some(map) = self.legacy_cluster_node_map.as_mut() else {
            return;
        };
        map.insert(legacy, current);
        if let Some(rev) = self.legacy_cluster_node_map_reverse.as_mut() {
            rev.insert(current, legacy);
        }
    }

    /// :1755-1761
    fn get_mapped_cluster_node_id(&self, node_id: f64) -> f64 {
        let mapped = self
            .legacy_cluster_node_map
            .as_ref()
            .and_then(|m| as_id(node_id).and_then(|id| m.get(&id)))
            .copied();
        match mapped {
            Some(m) if self.index.contains_key(&m) => f64::from(m),
            _ => node_id,
        }
    }

    /// :1764-1798
    fn apply_legacy_cluster_node_remap(&mut self) {
        let Some(map) = self.legacy_cluster_node_map.clone() else {
            return;
        };
        let mut converted: Vec<f64> = Vec::new();
        let mut seen: Vec<f64> = Vec::new();
        for id in std::mem::take(&mut self.alloc_subgraph_nodes) {
            let id = self.get_mapped_cluster_node_id(id);
            if !seen.contains(&id) {
                seen.push(id);
                converted.push(id);
            }
        }
        self.alloc_subgraph_nodes = converted;
        // `pairs(self.legacyClusterNodeMap)` -- only the push order into allocSubgraphNodes depends on it.
        for (&legacy, &current) in &map {
            if legacy != current
                && let Some(&li) = self.alloc_nodes.get(&legacy)
                && self.index.contains_key(&current)
            {
                self.nodes[li].alloc = false;
                self.alloc_nodes.shift_remove(&legacy);
                self.alloc_nil(legacy);
                let current = f64::from(current);
                if !seen.contains(&current) {
                    seen.push(current);
                    self.alloc_subgraph_nodes.push(current);
                }
            }
        }
        // :1816-1820 -- `pairs(self.jewels)`: LuaJIT's walk of the table built by `self.jewels[k] = v` in the order the keys were
        // first set (`NumMap` keeps it: Load's <Socket>s, then the socket slots' 0s, pob-calc `populate_socket_slots`). The order decides
        // which value lands when two keys map to one socket (a legacy socket id onto a socket whose slot wrote 0: the jewel is lost).
        let keys: Vec<f64> = self.jewels.0.iter().map(|&(k, _)| k).collect();
        let walk = crate::lj_table::pairs_order(&keys);
        let jewels = self.remap_jewels(&walk);
        // The 0s were set in `pairs(self.slots)` order upstream (string keys, per process): a collision whose outcome changes when they
        // are set in the opposite order is reported, not resolved silently.
        let (set, zeros): (Vec<f64>, Vec<f64>) = keys.iter().partition(|&&k| self.jewels.get(k).is_some_and(|&v| v != 0.0));
        let reversed: Vec<f64> = set.into_iter().chain(zeros.into_iter().rev()).collect();
        let other = self.remap_jewels(&crate::lj_table::pairs_order(&reversed));
        for &(to, item_id) in &jewels.0 {
            if other.get(to) != Some(&item_id) {
                self.unimplemented.push(format!(
                    "{SPEC}:1818 D13: two jewels map to socket {} -- which one wins depends on the per-process pairs(self.slots) order of ItemsTab:PopulateSlots",
                    pob_lua::tostring_num(to)
                ));
            }
        }
        self.jewels = jewels;
    }

    /// :1816-1819 `convertedJewels[self:GetMappedClusterNodeId(nodeId)] = itemId` over `walk` (the `pairs` order of `self.jewels`).
    fn remap_jewels(&self, walk: &[f64]) -> NumMap<f64> {
        let mut jewels: NumMap<f64> = NumMap::default();
        for &node_id in walk {
            let item_id = *self.jewels.get(node_id).expect("a key of self.jewels");
            jewels.set(self.get_mapped_cluster_node_id(node_id), item_id);
        }
        jewels
    }

    /// :1801-1805
    fn end_legacy_cluster_hash_conversion(&mut self) {
        self.cluster_hash_format_version = 2.0;
        self.legacy_cluster_node_map = None;
        self.legacy_cluster_node_map_reverse = None;
    }

    /// :912-921 `GetSocketedJewel(nodeId)` + `GetJewel`.
    fn get_socketed_jewel(&self, node_id: u32, get_jewel: JewelLookup<'_>) -> Option<Rc<ClusterJewel>> {
        let mut item_id = self.jewels.get(f64::from(node_id)).copied();
        if (item_id.is_none() || item_id == Some(0.0))
            && let Some(rev) = &self.legacy_cluster_node_map_reverse
            && let Some(&legacy) = rev.get(&node_id)
        {
            item_id = self.jewels.get(f64::from(legacy)).copied();
        }
        match item_id {
            None => None,
            Some(0.0) => None,
            Some(id) => get_jewel(id),
        }
    }

    /// :1806-1881
    pub fn build_cluster_jewel_graphs(&mut self, get_jewel: JewelLookup<'_>) -> Result<(), TreeError> {
        let needs_legacy = self.begin_legacy_cluster_hash_conversion();

        // Mark that path building should clear out stale references to cluster nodes
        self.check_node_links = true;
        // :1812-1828 -- remove old subgraphs (`pairs(self.subGraphs)`: insertion order; only the order of
        // allocSubgraphNodes depends on it).
        let old = std::mem::take(&mut self.sub_graphs);
        for sg in old.values() {
            for &i in &sg.nodes {
                let id = self.nodes[i].id;
                self.index.remove(&id);
                if self.alloc_nodes.contains_key(&id) {
                    // Reserve the allocation in case the node is regenerated
                    self.alloc_nodes.shift_remove(&id);
                    self.alloc_nil(id);
                    if !self.ignore_allocating_subgraph {
                        self.alloc_subgraph_nodes.push(f64::from(id));
                    }
                }
            }
            let entrance = sg.entrance;
            let pos = entrance
                .and_then(|e| self.nodes[sg.parent_socket].linked.iter().position(|&l| l == e))
                .ok_or_else(|| err(1826, "Entrance for subGraph not linked to parent socket???"))?;
            self.nodes[sg.parent_socket].linked.remove(pos);
        }
        // Every subgraph node sits after the Init nodes; with all subgraphs gone, so are they.
        self.nodes.truncate(self.base_len);
        self.ignore_allocating_subgraph = false;

        // :1832-1845 importedGroups/importedNodes: `self.jewel_data` is only set by ImportTab (P5) -- empty here.

        // :1846-1852 -- `pairs(self.tree.sockets)`: ascending id (each Large socket builds its own subgraph).
        let tree = self.tree.clone();
        for &node_id in tree.sockets.keys() {
            let Some(node) = tree.node(node_id) else { continue };
            let jewel = self.get_socketed_jewel(node_id, get_jewel);
            let large = node.expansion_jewel.as_ref().is_some_and(|e| e.size == Some(2.0));
            if self.alloc_nodes.contains_key(&node_id)
                && large
                && let Some(jewel) = jewel.filter(|j| j.valid)
            {
                // This is a Large Jewel Socket, and it has a cluster jewel in it
                let parent = self.node_idx(node_id).expect("an allocated node is a spec node");
                self.build_subgraph(&jewel, parent, None, get_jewel)?;
            }
        }

        if needs_legacy {
            self.apply_legacy_cluster_node_remap();
        }

        // :1858-1870 (Re-)allocate subgraph nodes
        for id in std::mem::take(&mut self.alloc_subgraph_nodes) {
            let Some((nid, i)) = as_id(id).and_then(|nid| self.node_idx(nid).map(|i| (nid, i))) else {
                continue;
            };
            self.nodes[i].alloc = true;
            if !self.alloc_nodes.contains_key(&nid) {
                self.alloc_nodes.insert(nid, i);
                self.alloc_table.set(f64::from(nid));
                if !self.alloc_extended_nodes.contains(&nid) {
                    self.alloc_extended_nodes.push(nid);
                }
            }
        }

        // Rebuild paths to account for new/removed nodes
        self.build_all_depends_and_paths()?;
        // :1877 `searchStrCached` -- the tree viewer's search cache (UI).
        self.end_legacy_cluster_hash_conversion();
        Ok(())
    }

    /// :1883-1891 `FindClusterSocket(group, index)` -- the tree node id.
    fn find_cluster_socket(&self, group: i64, index: f64) -> Result<Option<u32>, TreeError> {
        for id in group_node_ids(&self.tree, group) {
            let node = id
                .and_then(|id| self.tree.node(id))
                .ok_or_else(|| err(1886, format!("group {group}: a node id that is not in the tree")))?;
            if node.expansion_jewel.as_ref().is_some_and(|e| e.index == Some(index)) {
                return Ok(Some(node.id));
            }
        }
        Ok(None)
    }

    /// :1895-1916 -- the proxy group the legacy parser descended to (a group key).
    fn build_legacy_proxy_group(&self, proxy_group: i64, expansion_size: f64, cluster_size_index: f64) -> Result<i64, TreeError> {
        let mut legacy_group = proxy_group;
        let mut group_size = expansion_size;
        let mut guard = 0;
        while cluster_size_index < group_size && guard < 4 {
            let socket = match self.find_cluster_socket(legacy_group, 1.0)? {
                Some(s) => Some(s),
                None => self.find_cluster_socket(legacy_group, 0.0)?,
            };
            let Some(socket) = socket.and_then(|s| self.tree.node(s)) else {
                break;
            };
            let ej = socket.expansion_jewel.as_ref().expect("FindClusterSocket matched expansionJewel");
            let proxy = ej
                .proxy
                .as_deref()
                .and_then(pob_lua::tonumber)
                .and_then(as_id)
                .and_then(|p| self.tree.node(p));
            let Some(g) = proxy.and_then(|p| p.group) else {
                break;
            };
            legacy_group = g;
            group_size = ej
                .size
                .ok_or_else(|| err(1898, "comparing a number with nil (expansionJewel.size)"))?;
            guard += 1;
        }
        Ok(legacy_group)
    }

    /// :1920-1937 `TranslateClusterOrbitIndex` -- `None` is Lua's nil (an index outside the table).
    fn translate_cluster_orbit_index(&mut self, src: i64, src_n: f64, dest_n: f64) -> Option<i64> {
        let pick = |t: &[i64]| usize::try_from(src).ok().and_then(|i| t.get(i)).copied();
        if src_n == dest_n {
            Some(src)
        } else if src_n == 12.0 && dest_n == 16.0 {
            pick(&[0, 1, 3, 4, 5, 7, 8, 9, 11, 12, 13, 15])
        } else if src_n == 16.0 && dest_n == 12.0 {
            pick(&[0, 1, 1, 2, 3, 4, 4, 5, 6, 7, 7, 8, 9, 10, 10, 11])
        } else if src_n == 6.0 && dest_n == 16.0 {
            pick(&[0, 3, 5, 8, 11, 13])
        } else if src_n == 16.0 && dest_n == 6.0 {
            pick(&[0, 0, 0, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 5, 5, 5])
        } else {
            // :1932 launch:ShowErrMsg (a message box) -- then the "a little krangled" fallback.
            self.unimplemented.push(format!(
                "{SPEC}:1932 ShowErrMsg: unexpected cluster jewel node counts {} -> {}",
                pob_lua::tostring_num(src_n),
                pob_lua::tostring_num(dest_n)
            ));
            Some((src as f64 * dest_n / src_n).floor() as i64)
        }
    }

    /// :1940-1945 -- `node.oidx` of every templated node, into the tree's orbit space.
    fn apply_cluster_orbit_index_adjustment(
        &mut self,
        pending: &mut [TreeNode],
        indicies: &BTreeMap<i64, usize>,
        start_oidx: f64,
        total: f64,
        skills_per_orbit: f64,
    ) -> Result<(), TreeError> {
        for &p in indicies.values() {
            let oidx = pending[p].oidx.expect("templated nodes have an oidx");
            // Lua `%` (floored).
            let a = oidx + start_oidx;
            let rel = a - (a / total).floor() * total;
            let out = self.translate_cluster_orbit_index(rel as i64, total, skills_per_orbit);
            pending[p].oidx = out.map(|o| o as f64);
        }
        Ok(())
    }

    /// :1948-1968
    fn build_legacy_cluster_orbit_mappings(
        &mut self,
        pending: &[TreeNode],
        indicies: &BTreeMap<i64, usize>,
        proxy: &TreeNode,
        total: f64,
        skills_per_orbit: f64,
    ) -> Result<(), TreeError> {
        if self.legacy_cluster_node_map.is_none() {
            return Ok(());
        }
        let nil = |what: &str| err(1955, format!("{what} is nil (a Lua error)"));
        let legacy_spo = proxy
            .o
            .and_then(as_id)
            .and_then(|o| self.tree.skills_per_orbit.get(o as usize))
            .copied()
            .ok_or_else(|| nil("tree.skillsPerOrbit[proxyNode.o + 1]"))?;
        let proxy_oidx = proxy.oidx.ok_or_else(|| nil("proxyNode.oidx"))? as i64;
        let legacy_proxy_rel = self
            .translate_cluster_orbit_index(proxy_oidx, legacy_spo, total)
            .ok_or_else(|| nil("legacyProxyNodeOidxRelativeToClusterIndicies"))?;
        let lua_mod = |a: i64| -> i64 {
            let (a, t) = (a as f64, total);
            (a - (a / t).floor() * t) as i64
        };
        let mut legacy_by_oidx: BTreeMap<i64, u32> = BTreeMap::new();
        let mut current_by_oidx: BTreeMap<i64, u32> = BTreeMap::new();
        let mut collision = false;
        // `pairs(indicies)` -- ascending template index (D13: a collision below would depend on it).
        for (&node_index, &p) in indicies {
            let node = &pending[p];
            let legacy_rel = lua_mod(node_index + legacy_proxy_rel);
            let legacy_oidx = self
                .translate_cluster_orbit_index(legacy_rel, total, legacy_spo)
                .ok_or_else(|| err(1959, "table index is nil (legacyNodeOidx)"))?;
            if legacy_by_oidx.insert(legacy_oidx, node.id).is_some_and(|prev| prev != node.id) {
                collision = true;
            }
            let oidx = node.oidx.ok_or_else(|| nil("node.oidx"))? as i64;
            let current_rel = self
                .translate_cluster_orbit_index(oidx, skills_per_orbit, total)
                .ok_or_else(|| err(1962, "currentNodeOidxRelativeToClusterIndicies is nil"))?;
            let current = self
                .translate_cluster_orbit_index(current_rel, total, legacy_spo)
                .ok_or_else(|| err(1963, "table index is nil (currentNodeOidxInLegacySkillsPerOrbit)"))?;
            if current_by_oidx.insert(current, node.id).is_some_and(|prev| prev != node.id) {
                collision = true;
            }
        }
        if collision {
            self.unimplemented.push(format!(
                "{SPEC}:1957-1964 D13: two cluster nodes share a legacy orbit index -- the mapping depends on pairs(indicies)"
            ));
        }
        for (oidx, &legacy) in &legacy_by_oidx {
            if let Some(&current) = current_by_oidx.get(oidx)
                && legacy != current
            {
                self.register_legacy_cluster_node_map(legacy, current);
            }
        }
        Ok(())
    }

    /// :2027-2056 `addToAllocatedSubgraphNodes(node)`: `matchGroup` walks `importedGroups`, which is empty without
    /// `self.jewel_data` (ImportTab, P5) -- so it is false for every node on the XML load path.
    fn add_to_allocated_subgraph_nodes(&self, _node_id: u32) -> bool {
        false
    }

    /// `PassiveTree:ProcessNode` (PassiveTree.lua:875-906) on a subgraph node: the position in `subGraph.group`, then
    /// `ProcessStats`. Sprites/overlays are display only.
    fn process_subgraph_node(&self, node: &mut TreeNode, gx: f64, gy: f64) -> Result<(), TreeError> {
        let tree = &self.tree;
        let o = node.o.and_then(as_id).map(|o| o as usize);
        let angle = o
            .and_then(|o| tree.orbit_angles_by_orbit.get(o))
            .and_then(|a| node.oidx.and_then(as_id).and_then(|i| a.get(i as usize)))
            .copied()
            .ok_or_else(|| {
                TreeError(format!(
                    "Classes/PassiveTree.lua:899: subgraph node {}: no orbit angle for o={:?} oidx={:?}",
                    node.id, node.o, node.oidx
                ))
            })?;
        let radius = o
            .and_then(|o| tree.orbit_radii.get(o))
            .copied()
            .ok_or_else(|| TreeError(format!("Classes/PassiveTree.lua:900: subgraph node {}: no orbit radius", node.id)))?;
        node.angle = Some(angle);
        // MINEFIELD-RUST R2: `f64::sin/cos` = desktop UCRT.
        node.x = Some(gx + angle.sin() * radius);
        node.y = Some(gy - angle.cos() * radius);
        if node.ty == NodeType::Socket {
            // Upstream processes the socket's own empty `sd`, and BuildAllDependsAndPaths' `ReplaceNode(node,
            // tree.nodes[id])` (:1166) then hands it the tree socket's dn/sd/name/mods/modList: the node was built
            // holding those already (`subgraph_socket`).
            return Ok(());
        }
        let holder = StatHolder {
            id: f64::from(node.id),
            str_id: None,
            ty: Some(node.ty),
            dn: &node.dn,
        };
        let mut sd = std::mem::take(&mut node.sd);
        let mut stats = NodeStats::default();
        process_stats(&holder, &mut sd, &mut stats, 1, tree.parse_mode);
        node.sd = sd;
        node.stats = stats;
        Ok(())
    }

    fn link_nodes(&mut self, a: usize, b: usize) {
        self.nodes[a].linked.push(b);
        self.nodes[b].linked.push(a);
    }

    /// :1970-2360 `BuildSubgraph(jewel, parentSocket, id, upSize, importedNodes, importedGroups)`; `parent_socket` indexes
    /// `self.nodes`. `upSize` only feeds commented-out code, the imported tables are empty (module comment).
    #[allow(clippy::too_many_lines)]
    fn build_subgraph(
        &mut self,
        jewel: &ClusterJewel,
        parent_socket: usize,
        id: Option<u32>,
        get_jewel: JewelLookup<'_>,
    ) -> Result<(), TreeError> {
        let tree = self.tree.clone();
        let expansion = self.nodes[parent_socket]
            .tree_node()
            .expansion_jewel
            .clone()
            .ok_or_else(|| err(1971, "attempt to index a nil value (expansionJewel)"))?;
        let cj = jewel
            .cluster_jewel
            .ok_or_else(|| err(1972, "attempt to index a nil value (jewel.clusterJewel)"))?;

        // :1982-1995 -- the subgraph id.
        let mut id = id.unwrap_or(0x10000);
        let shift = |by: u32| -> Result<u32, TreeError> {
            let idx = expansion
                .index
                .and_then(as_id)
                .ok_or_else(|| err(1991, "bad argument to 'lshift' (expansionJewel.index)"))?;
            Ok(idx << by)
        };
        if expansion.size == Some(2.0) {
            id += shift(6)?;
        } else if expansion.size == Some(1.0) {
            id += shift(9)?;
        }
        let size_index = as_id(cj.size_index).ok_or_else(|| err(1995, "clusterJewel.sizeIndex is not an index"))?;
        let node_id = id + (size_index << 4);
        self.sub_graphs.insert(
            node_id,
            SubGraph {
                nodes: Vec::new(),
                parent_socket,
                entrance: None,
            },
        );

        // :2000-2003 Locate the proxy group
        let proxy = expansion
            .proxy
            .as_deref()
            .and_then(pob_lua::tonumber)
            .and_then(as_id)
            .and_then(|p| tree.node(p))
            .ok_or_else(|| err(2001, "Proxy node not found"))?;
        let proxy_group = proxy
            .group
            .ok_or_else(|| err(2018, "attempt to index a nil value (proxyNode.group)"))?;
        let (gx, gy) = {
            let g = &tree.groups[&proxy_group];
            (g.x, g.y)
        };

        // :2058-2091 Special handling for keystones
        if let Some(name) = &jewel.keystone {
            let &ks = tree
                .cluster_node_map
                .get(name)
                .ok_or_else(|| err(2061, format!("Keystone node not found: {name}")))?;
            let kn = &tree.nodes[ks];
            let mut node = subgraph_node(node_id, NodeType::Keystone, kn.dn.clone(), kn.sd.clone(), kn.icon.clone(), 0.0, 1.0);
            self.process_subgraph_node(&mut node, gx, gy)?;
            let idx = self.push_subgraph_node(node);
            self.sub_graphs[&node_id].nodes.push(idx);
            self.link_nodes(idx, parent_socket);
            self.sub_graphs[&node_id].entrance = Some(idx);
            self.insert_index(idx)?;
            if self.add_to_allocated_subgraph_nodes(node_id) {
                self.alloc_subgraph_nodes.push(f64::from(node_id));
            }
            return Ok(());
        }

        let legacy_proxy_group = if self.legacy_cluster_node_map.is_some() {
            let size = expansion
                .size
                .ok_or_else(|| err(1898, "comparing a number with nil (expansionJewel.size)"))?;
            Some(self.build_legacy_proxy_group(proxy_group, size, cj.size_index)?)
        } else {
            None
        };

        // Initialise orbit flags (`subGraph.group.oo`, display only)
        let node_orbit = cj.size_index + 1.0;

        // :2103-2117 Process list of notables
        let sort_order = &cluster_jewels().notable_sort_order;
        let mut notable_list: Vec<usize> = Vec::new();
        for name in &jewel.notables {
            let Some(&base) = tree.cluster_node_map.get(name) else {
                // Ignore subgraphs when loading old trees where certain notables don't exist
                self.sub_graphs.shift_remove(&node_id);
                return Ok(());
            };
            if !sort_order.contains_key(&tree.nodes[base].dn) {
                return Err(err(2124, format!("Cluster notable has no sort order: {name}")));
            }
            notable_list.push(base);
        }
        // `table.sort` by sortOrder[dn]: two equal keys are the same notable (same dn -> same base node), so the order
        // between them is unobservable; a stable sort is used.
        notable_list.sort_by(|&a, &b| sort_order[&tree.nodes[a].dn].total_cmp(&sort_order[&tree.nodes[b].dn]));

        let skill = jewel.skill.as_ref().and_then(|s| cj.skills.get(s));
        let (skill_name, skill_icon, skill_stats, mastery_icon) = match skill {
            Some(s) => (s.name.clone(), s.icon.clone(), s.stats.clone(), s.mastery_icon.clone()),
            None => (
                "Nothingness".to_owned(),
                "Art/2DArt/SkillIcons/passives/MasteryBlank.png".to_owned(),
                Vec::new(),
                None,
            ),
        };
        // L4: `a or b or 0` -- a 0 is a number, and kept.
        let socket_count = jewel.socket_count_override.or(jewel.socket_count).unwrap_or(0.0);
        let notable_count = notable_list.len() as f64;
        let node_count = jewel
            .node_count
            .unwrap_or(socket_count + notable_count + jewel.nothingness_count.unwrap_or(0.0));
        let small_count = node_count - socket_count - notable_count;

        let mut pending: Vec<TreeNode> = Vec::new();
        let mut indicies: BTreeMap<i64, usize> = BTreeMap::new();

        if let Some(icon) = mastery_icon {
            // :2143-2155 Add mastery node
            pending.push(subgraph_node(
                offset_id(node_id, 12)?,
                NodeType::Mastery,
                "Nothingness".into(),
                Vec::new(),
                icon,
                0.0,
                0.0,
            ));
        }

        // :2159-2186 makeJewel
        let mut make_jewel =
            |spec: &mut PassiveSpec, pending: &mut Vec<TreeNode>, node_index: i64, jewel_index: f64| -> Result<(), TreeError> {
                let socket = spec
                    .find_cluster_socket(proxy_group, jewel_index)?
                    .ok_or_else(|| err(2162, "Socket not found (ran out of sockets nani?)"))?;
                let node = subgraph_socket(&tree, socket, node_orbit, node_index as f64);
                let sid = node.id;
                indicies.insert(node_index, pending.len());
                pending.push(node);
                if let Some(lg) = legacy_proxy_group
                    && spec.legacy_cluster_node_map.is_some()
                    && let Some(legacy) = spec.find_cluster_socket(lg, jewel_index)?
                {
                    spec.register_legacy_cluster_node_map(legacy, sid);
                }
                Ok(())
            };

        // :2188-2198 First pass: sockets
        if cj.size == "Large" && socket_count == 1.0 {
            // Large clusters always have the single jewel at index 6
            make_jewel(self, &mut pending, 6, 1.0)?;
        } else {
            if socket_count > cj.socket_indicies.len() as f64 {
                return Err(err(2193, "Too many sockets!"));
            }
            const GET_JEWELS: [f64; 3] = [0.0, 2.0, 1.0];
            let mut i = 1.0;
            while i <= socket_count {
                let k = i as usize - 1;
                let node_index = cj.socket_indicies[k] as i64;
                // `getJewels[i]` past 3 is nil: FindClusterSocket(group, nil) finds nothing.
                let jewel_index = GET_JEWELS.get(k).copied().unwrap_or(f64::NAN);
                make_jewel(self, &mut pending, node_index, jewel_index)?;
                i += 1.0;
            }
        }
        let indicies_now = |m: &BTreeMap<i64, usize>, k: i64| m.contains_key(&k);

        // :2202-2228 Gather notable indicies
        let mut notable_index_list: Vec<i64> = Vec::new();
        for &ni in &cj.notable_indicies {
            if notable_index_list.len() as f64 == notable_count {
                break;
            }
            let mut ni = ni as i64;
            if cj.size == "Medium" {
                if socket_count == 0.0 && notable_count == 2.0 {
                    // Special rule for two notables in a Medium cluster
                    if ni == 6 {
                        ni = 4;
                    } else if ni == 10 {
                        ni = 8;
                    }
                } else if node_count == 4.0 {
                    // Special rule for notables in a 4-node Medium cluster
                    if ni == 10 {
                        ni = 9;
                    } else if ni == 2 {
                        ni = 3;
                    }
                }
            }
            if !indicies_now(&indicies, ni) {
                notable_index_list.push(ni);
            }
        }
        notable_index_list.sort_unstable();

        // :2231-2253 Create the notables
        for (index, &base) in notable_list.iter().enumerate() {
            let Some(&ni) = notable_index_list.get(index) else {
                // Silently fail to handle cases of jewels with more notables than should be allowed
                break;
            };
            let bn = &tree.nodes[base];
            let node = subgraph_node(
                offset_id(node_id, ni)?,
                NodeType::Notable,
                bn.dn.clone(),
                bn.sd.clone(),
                bn.icon.clone(),
                node_orbit,
                ni as f64,
            );
            indicies.insert(ni, pending.len());
            pending.push(node);
        }

        // :2257-2279 Gather small indicies
        let mut small_index_list: Vec<i64> = Vec::new();
        for &ni in &cj.small_indicies {
            if small_index_list.len() as f64 == small_count {
                break;
            }
            let mut ni = ni as i64;
            if cj.size == "Medium" {
                // Special rules for small nodes in Medium clusters
                if node_count == 5.0 && ni == 4 {
                    ni = 3;
                } else if node_count == 4.0 {
                    if ni == 8 {
                        ni = 9;
                    } else if ni == 4 {
                        ni = 3;
                    }
                }
            }
            if !indicies_now(&indicies, ni) {
                small_index_list.push(ni);
            }
        }

        // :2281-2306 Create the small nodes
        let mut index = 1.0;
        while index <= small_count {
            let Some(&ni) = small_index_list.get(index as usize - 1) else {
                break;
            };
            let mut sd = skill_stats.clone();
            sd.extend(jewel.added_mods.iter().cloned());
            let node = subgraph_node(
                offset_id(node_id, ni)?,
                NodeType::Normal,
                skill_name.clone(),
                sd,
                skill_icon.clone(),
                node_orbit,
                ni as f64,
            );
            indicies.insert(ni, pending.len());
            pending.push(node);
            index += 1.0;
        }

        let entrance_p = *indicies.get(&0).ok_or_else(|| err(2308, "No entrance to subgraph"))?;

        // :2311-2315 Convert from cluster-template index space into the tree's orbit index space.
        let skills_per_orbit = tree
            .skills_per_orbit
            .get(size_index as usize + 1)
            .copied()
            .ok_or_else(|| err(2312, "tree.skillsPerOrbit[sizeIndex + 2] is nil"))?;
        let start_oidx = cluster_jewels()
            .orbit_offsets
            .get(proxy.id.to_string().as_str())
            .and_then(|o| o.get(size_index.to_string().as_str()))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| err(2313, format!("data.clusterJewels.orbitOffsets[{}][{size_index}] is nil", proxy.id)))?;
        self.apply_cluster_orbit_index_adjustment(&mut pending, &indicies, start_oidx, cj.total_indicies, skills_per_orbit)?;
        self.build_legacy_cluster_orbit_mappings(&pending, &indicies, proxy, cj.total_indicies, skills_per_orbit)?;

        // :2318-2325 Perform processing on nodes to calculate positions, parse mods, and other goodies
        for node in &mut pending {
            self.process_subgraph_node(node, gx, gy)?;
            // `node.modList` always exists after ProcessStats; L4: an inc effect of 0 is still truthy.
            if let Some(inc) = jewel.inc_effect
                && node.ty == NodeType::Normal
            {
                node.stats.mod_list.mods.push(Rc::new(create_mod(
                    "PassiveSkillEffect",
                    Some(ModType::Inc),
                    Value::Num(inc),
                    None,
                    0,
                    0,
                    Vec::new(),
                )));
            }
        }
        let first_idx = self.nodes.len();
        let ids: Vec<usize> = (first_idx..first_idx + pending.len()).collect();
        for node in pending {
            self.push_subgraph_node(node);
        }
        self.sub_graphs[&node_id].nodes.clone_from(&ids);
        let entrance = ids[entrance_p];
        self.sub_graphs[&node_id].entrance = Some(entrance);

        // :2327-2345 Generate connectors (the links; the connector art is display only)
        let total = cj.total_indicies as i64;
        let (mut first, mut last): (Option<usize>, Option<usize>) = (None, None);
        for i in 0..total {
            if let Some(&p) = indicies.get(&i) {
                let this = ids[p];
                if first.is_none() {
                    first = Some(this);
                }
                if let Some(l) = last {
                    self.link_nodes(this, l);
                }
                last = Some(this);
            }
        }
        if first != last
            && cj.size != "Small"
            && let (Some(f), Some(l)) = (first, last)
        {
            // Close the loop on non-small clusters
            self.link_nodes(f, l);
        }
        self.link_nodes(entrance, parent_socket);

        // :2347-2357 Add synthetic nodes to the main node list
        for &idx in &ids {
            self.insert_index(idx)?;
            let nid = self.nodes[idx].id;
            if self.add_to_allocated_subgraph_nodes(nid) {
                self.alloc_subgraph_nodes.push(f64::from(nid));
            }
            if self.nodes[idx].ty() == NodeType::Socket {
                // Recurse to smaller jewels
                let jewel = self.get_socketed_jewel(nid, get_jewel);
                // Allocated subgraph nodes are temporarily preserved outside allocNodes while rebuilding.
                let allocated = self.alloc_nodes.contains_key(&nid) || self.alloc_subgraph_nodes.contains(&f64::from(nid));
                if allocated && let Some(jewel) = jewel.filter(|j| j.valid) {
                    self.build_subgraph(&jewel, idx, Some(id), get_jewel)?;
                }
            }
        }
        Ok(())
    }

    fn push_subgraph_node(&mut self, node: TreeNode) -> usize {
        let tree = self.tree.clone();
        self.nodes.push(SpecNode::owned(&tree, node));
        self.nodes.len() - 1
    }

    /// `self.nodes[node.id] = node`. A subgraph node never shadows a live node on the goldens; if one did, the old object
    /// would linger in `nodes` (upstream drops it from `pairs(self.nodes)`), so it is refused rather than mis-modelled.
    fn insert_index(&mut self, idx: usize) -> Result<(), TreeError> {
        let id = self.nodes[idx].id;
        if let Some(prev) = self.index.insert(id, idx)
            && prev != idx
        {
            return Err(TreeError(format!(
                "unimplemented: {SPEC}:2349 subgraph node {id} replaces an existing spec node"
            )));
        }
        Ok(())
    }
}

/// A node table of `BuildSubgraph` (:2072, :2144, :2235, :2284): no `name`, no group of the tree (`subGraph.group`).
fn subgraph_node(id: u32, ty: NodeType, dn: String, sd: Vec<String>, icon: String, o: f64, oidx: f64) -> TreeNode {
    TreeNode {
        id,
        dn,
        name: None,
        sd,
        ty,
        g: None,
        o: Some(o),
        oidx: Some(oidx),
        group: None,
        ascendancy_name: None,
        class_start_index: None,
        is_ascendancy_start: false,
        is_multiple_choice: false,
        is_multiple_choice_option: false,
        is_proxy: false,
        is_blighted: false,
        is_jewel_socket: false,
        is_mastery: false,
        expansion_jewel: None,
        passive_points_granted: 0.0,
        granted_strength: None,
        granted_dexterity: None,
        granted_intelligence: None,
        reminder_text: None,
        mastery_effect_ids: Vec::new(),
        icon,
        linked_id: Vec::new(),
        x: None,
        y: None,
        angle: None,
        stats: NodeStats::default(),
    }
}

/// :2165-2175 `makeJewel`'s node -- `{ type = "Socket", id, dn, sd = {}, icon, expansionJewel, group, o, oidx }` -- as it
/// stands after BuildAllDependsAndPaths' `ReplaceNode(node, tree.nodes[id])` (:1166): dn, sd, name, mods, modKey,
/// modList, keystoneMod, icon and reminderText (`or wipeTable(nil)` = `{}`) are the tree socket's; `unknown`/`extra` are
/// not copied by ReplaceNode and stay unset.
fn subgraph_socket(tree: &PassiveTree, socket: u32, o: f64, oidx: f64) -> TreeNode {
    let t = tree.node(socket).expect("FindClusterSocket returns a tree node");
    let mut node = subgraph_node(t.id, NodeType::Socket, t.dn.clone(), t.sd.clone(), t.icon.clone(), o, oidx);
    node.name = t.name.clone();
    node.expansion_jewel = t.expansion_jewel.clone();
    node.reminder_text = Some(t.reminder_text.clone().unwrap_or_default());
    node.stats = NodeStats {
        unknown: false,
        extra: false,
        ..t.stats.clone()
    };
    node
}
