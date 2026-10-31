//! `Classes/PassiveSpec.lua` -- the subset a calculation needs (pob-ts `src/tree/spec.ts`, CALC-ARCH §3): which nodes a
//! build has allocated, and with what mods. Cluster-jewel subgraphs are `crate::cluster` (R5.5a): their nodes are owned
//! spec nodes appended after the Init nodes. Other jewels (timeless/abyss/tattoo/Intuitive Leap), the `path`/`pathDist`
//! machinery that only tree editing uses, and the URL format are P5; each is listed in `unimplemented`.
//!
//! A spec node is upstream's `setmetatable({ linked = {}, power = {} }, treeNode)`: a view onto the tree node that only grows
//! own fields when the mastery branch of `BuildAllDependsAndPaths` writes them. `ReplaceNode(old, treeNode)` assigns the
//! tree's own values back (same tables, same mod objects); since a tree node never changes after it is built, "assigned
//! the tree's value" and "reads through to the tree" are the same thing, so `replace_node` simply drops the own fields --
//! except `reminderText` (`wipeTable` makes an own empty table) and `unknown`/`extra`, which ReplaceNode does not touch.

use std::rc::Rc;

use indexmap::IndexMap;
use pob_build::{NumMap, SpecModel};
use pob_mods::ModList;

use crate::tree::{NodeStats, NodeType, PassiveTree, StatHolder, TreeError, TreeNode, as_id, process_stats};

/// PassiveSpec.lua:1415 -- one shared table upstream.
pub const MASTERY_REMINDER_TEXT: &str = "Tip: Right click to select a different effect";

/// What replaces `pairs(self.allocNodes)` in BuildAllDependsAndPaths (D13); the allocated set must not depend on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AllocOrder {
    /// Ascending node id -- the pob-ts default and the order the fixtures were made with.
    #[default]
    IdAsc,
    IdDesc,
    /// Insertion order of `allocNodes` (= XML order for ImportFromNodeList).
    Insertion,
}

/// `{ used, ascUsed, secondaryAscUsed, sockets }` of `CountAllocNodes` (:846-868).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocNodeCounts {
    pub used: u32,
    pub asc_used: u32,
    pub secondary_asc_used: u32,
    pub sockets: u32,
}

/// What a spec node is a view of: a tree node (`setmetatable({...}, treeNode)`), or a node `BuildSubgraph` made (a plain
/// table upstream, PassiveSpec.lua:2072/2160/2235/2284/2311) -- owned by the spec.
#[derive(Clone)]
pub(crate) enum NodeBase {
    Tree(usize),
    Owned(Rc<TreeNode>),
}

pub struct SpecNode {
    tree: Rc<PassiveTree>,
    base: NodeBase,
    pub id: u32,
    pub alloc: bool,
    /// `node.linked` -- indices into `PassiveSpec::nodes`, in `linkedId` order, links into filtered-out nodes dropped (:65).
    pub linked: Vec<usize>,
    pub depends: Vec<usize>,
    /// Always empty without jewels (P5).
    pub intuitive_leap_likes_affecting: Vec<usize>,
    pub visited: bool,
    pub connected_to_start: bool,
    /// Set by AddMasteryEffectOptionsToNode upstream (display only, P5) -- never true here.
    pub all_mastery_options: bool,
    /// Own `sd`: the `masteryEffects[effect].sd` table (identity is what ReplaceNode compares, :1699).
    own_sd_effect: Option<u64>,
    own_sd: Option<Vec<String>>,
    /// Own `mods`/`modKey`/`modList` written by `ProcessStats` on this node.
    own_stats: Option<NodeStats>,
    own_unknown: Option<bool>,
    own_extra: Option<bool>,
    own_reminder_text: Option<Vec<String>>,
}

impl SpecNode {
    fn new(tree: &Rc<PassiveTree>, tree_idx: usize) -> SpecNode {
        SpecNode::with_base(tree, tree.nodes[tree_idx].id, NodeBase::Tree(tree_idx))
    }

    /// A cluster-subgraph node (`BuildSubgraph`), `linked = {}`.
    pub(crate) fn owned(tree: &Rc<PassiveTree>, node: TreeNode) -> SpecNode {
        SpecNode::with_base(tree, node.id, NodeBase::Owned(Rc::new(node)))
    }

    fn with_base(tree: &Rc<PassiveTree>, id: u32, base: NodeBase) -> SpecNode {
        SpecNode {
            id,
            tree: tree.clone(),
            base,
            alloc: false,
            linked: Vec::new(),
            depends: Vec::new(),
            intuitive_leap_likes_affecting: Vec::new(),
            visited: false,
            connected_to_start: false,
            all_mastery_options: false,
            own_sd_effect: None,
            own_sd: None,
            own_stats: None,
            own_unknown: None,
            own_extra: None,
            own_reminder_text: None,
        }
    }

    /// The tree node this spec node is a view of (`getmetatable(node).__index`); for a cluster-subgraph node, the node
    /// `BuildSubgraph` made.
    pub fn tree_node(&self) -> &TreeNode {
        match &self.base {
            NodeBase::Tree(i) => &self.tree.nodes[*i],
            NodeBase::Owned(n) => n,
        }
    }

    /// Made by `BuildSubgraph` (a cluster-jewel subgraph node, including the expansion sockets it puts back).
    pub fn is_subgraph_node(&self) -> bool {
        matches!(self.base, NodeBase::Owned(_))
    }

    pub fn ty(&self) -> NodeType {
        self.tree_node().ty
    }

    pub fn dn(&self) -> &str {
        &self.tree_node().dn
    }

    pub fn name(&self) -> Option<&str> {
        self.tree_node().name.as_deref()
    }

    pub fn ascendancy_name(&self) -> Option<&str> {
        self.tree_node().ascendancy_name.as_deref()
    }

    pub fn sd(&self) -> &[String] {
        self.own_sd.as_deref().unwrap_or(&self.tree_node().sd)
    }

    fn stats(&self) -> &NodeStats {
        self.own_stats.as_ref().unwrap_or(&self.tree_node().stats)
    }

    pub fn mod_list(&self) -> &ModList {
        &self.stats().mod_list
    }

    pub fn mods(&self) -> &[Option<crate::tree::NodeModLine>] {
        &self.stats().mods
    }

    pub fn mod_key(&self) -> &str {
        &self.stats().mod_key
    }

    /// Never written on a spec node in this subset (only a Keystone gets one, and only masteries are re-processed).
    pub fn keystone_mod(&self) -> Option<&Rc<pob_mods::Mod>> {
        self.stats().keystone_mod.as_ref()
    }

    pub fn unknown(&self) -> bool {
        self.own_unknown.unwrap_or(self.tree_node().stats.unknown)
    }

    pub fn extra(&self) -> bool {
        self.own_extra.unwrap_or(self.tree_node().stats.extra)
    }

    pub fn reminder_text(&self) -> Option<&[String]> {
        match &self.own_reminder_text {
            Some(v) => Some(v),
            None => self.tree_node().reminder_text.as_deref(),
        }
    }

    /// `node.nodesInRadius` (PassiveTree.lua:654-711) through the metatable: the tree socket's/keystone's lists. A node
    /// `BuildSubgraph` made is a plain table without them (PassiveSpec.lua:2160-2170), so `nil` -- as for every other node.
    pub fn nodes_in_radius(&self) -> Option<&crate::radius::NodesInRadius> {
        match &self.base {
            NodeBase::Tree(_) => self.tree.nodes_in_radius(self.id),
            NodeBase::Owned(_) => None,
        }
    }
}

/// What a radius-jewel visitor (`pob_parser::jewel::visit`, ModParser.lua:6151-6593) reads off a spec node.
impl pob_parser::helpers::JewelNode for SpecNode {
    fn mod_list(&self) -> &[Rc<pob_mods::Mod>] {
        &SpecNode::mod_list(self).mods
    }
    /// `node.isTattoo` -- tattoos are not ported (P5): nil.
    fn is_tattoo(&self) -> bool {
        false
    }
    fn node_type(&self) -> &str {
        self.ty().as_str()
    }
    /// `node.conqueredBy` -- written by the timeless-jewel half of BuildAllDependsAndPaths (PassiveSpec.lua:1176-1190),
    /// R5.5c; nil until then.
    fn conquered_by(&self) -> bool {
        false
    }
    fn sum_base(&self, stat: &str) -> Result<f64, pob_mods::LuaError> {
        let st = pob_mods::ModStore::new_list(None);
        st.borrow_mut().add_list(&SpecNode::mod_list(self).mods);
        st.borrow().sum(pob_mods::ModType::Base, None, &[stat])
    }
}

pub struct PassiveSpec {
    pub tree: Rc<PassiveTree>,
    pub tree_version: String,
    /// `self.nodes`: the Init nodes in ascending id, then the cluster-subgraph nodes in the order `BuildSubgraph` made them.
    pub nodes: Vec<SpecNode>,
    pub(crate) index: std::collections::HashMap<u32, usize>,
    /// How many of `nodes` Init made; everything after belongs to a subgraph (`BuildClusterJewelGraphs` drops and remakes them).
    pub(crate) base_len: usize,
    /// `self.subGraphs`: subgraph id -> the subgraph (insertion order).
    pub(crate) sub_graphs: IndexMap<u32, crate::cluster::SubGraph>,
    /// `self.clusterHashFormatVersion` (:97, :105): 1 = legacy cluster hashes, converted by the next graph rebuild.
    pub cluster_hash_format_version: f64,
    pub(crate) legacy_cluster_node_map: Option<IndexMap<u32, u32>>,
    pub(crate) legacy_cluster_node_map_reverse: Option<IndexMap<u32, u32>>,
    pub(crate) ignore_allocating_subgraph: bool,
    pub(crate) check_node_links: bool,
    /// `PostLoad` (:251) has run.
    pub post_loaded: bool,
    /// `self.allocNodes`: id -> index into `nodes`, insertion order (deletes keep the order of the rest).
    pub alloc_nodes: IndexMap<u32, usize>,
    /// `self.allocSubgraphNodes`: hash-list ids the spec has no node for yet -- cluster subgraph ids (>= 65536) and the
    /// expansion sockets; `BuildClusterJewelGraphs` allocates the ones it made and empties the list.
    pub alloc_subgraph_nodes: Vec<f64>,
    pub alloc_extended_nodes: Vec<u32>,
    /// `<Socket nodeId itemId>` (itemId > 0); the jewels themselves are R3.4/P5.
    pub jewels: NumMap<f64>,
    /// mastery node id -> effect id.
    pub mastery_selections: IndexMap<u32, f64>,
    pub cur_class_id: Option<usize>,
    pub cur_class_name: String,
    pub cur_ascend_class_id: Option<usize>,
    pub cur_ascend_class_name: String,
    pub cur_ascend_class_base_name: Option<String>,
    pub cur_secondary_ascend_class_id: Option<f64>,
    pub cur_secondary_ascend_class_name: String,
    pub allocated_notable_count: u32,
    pub allocated_keystone_count: u32,
    pub allocated_mastery_count: u32,
    pub allocated_mastery_type_count: u32,
    pub allocated_mastery_types: IndexMap<String, u32>,
    pub alloc_order: AllocOrder,
    pub unimplemented: Vec<String>,
}

impl PassiveSpec {
    /// `PassiveSpecClass:PassiveSpec(build, treeVersion)` (:27-37): Init, then SelectClass(0).
    pub fn new(tree: Rc<PassiveTree>) -> Result<PassiveSpec, TreeError> {
        let mut s = PassiveSpec {
            tree_version: tree.tree_version.clone(),
            tree,
            nodes: Vec::new(),
            index: std::collections::HashMap::new(),
            base_len: 0,
            sub_graphs: IndexMap::new(),
            cluster_hash_format_version: 2.0,
            legacy_cluster_node_map: None,
            legacy_cluster_node_map_reverse: None,
            ignore_allocating_subgraph: false,
            check_node_links: false,
            post_loaded: false,
            alloc_nodes: IndexMap::new(),
            alloc_subgraph_nodes: Vec::new(),
            alloc_extended_nodes: Vec::new(),
            jewels: NumMap::default(),
            mastery_selections: IndexMap::new(),
            cur_class_id: None,
            cur_class_name: String::new(),
            cur_ascend_class_id: None,
            cur_ascend_class_name: String::new(),
            cur_ascend_class_base_name: None,
            cur_secondary_ascend_class_id: None,
            cur_secondary_ascend_class_name: "None".into(),
            allocated_notable_count: 0,
            allocated_keystone_count: 0,
            allocated_mastery_count: 0,
            allocated_mastery_type_count: 0,
            allocated_mastery_types: IndexMap::new(),
            alloc_order: AllocOrder::IdAsc,
            unimplemented: Vec::new(),
        };
        s.init();
        s.select_class(0.0)?;
        Ok(s)
    }

    /// `self.nodes[id]` as an index.
    pub fn node_idx(&self, id: u32) -> Option<usize> {
        self.index.get(&id).copied()
    }

    /// `self.nodes[id]`.
    pub fn node(&self, id: u32) -> Option<&SpecNode> {
        self.node_idx(id).map(|i| &self.nodes[i])
    }

    /// `pairs(self.allocNodes)` in insertion order.
    pub fn alloc_nodes(&self) -> impl Iterator<Item = &SpecNode> {
        self.alloc_nodes.values().map(|&i| &self.nodes[i])
    }

    /// :39-100 -- the local, modifiable copy of the tree.
    fn init(&mut self) {
        self.unimplemented.extend(
            [
                "PassiveSpec.lua:1111-1409 timeless/abyss/tattoo node replacement (P5)",
                "PassiveSpec.lua:1444 AddMasteryEffectOptionsToNode (unallocated mastery display, P5)",
                "PassiveSpec.lua:1520-1560 Intuitive Leap-like dependency bookkeeping (P5)",
                "PassiveSpec.lua:1591-1695 node paths / pathDist (tree editing only, P5)",
                "PassiveSpec.lua:196 DecodeURL (legacy URL specs, P5)",
            ]
            .map(String::from),
        );
        let tree = self.tree.clone();
        for (ti, tn) in tree.nodes.iter().enumerate() {
            // :52 -- no proxies, no groupless (cluster-jewel template) nodes, no expansion sockets.
            let Some(g) = tn.group else { continue };
            if !tn.is_proxy && !tree.groups[&g].is_proxy && tn.expansion_jewel.as_ref().is_none_or(|e| e.parent.is_none()) {
                self.index.insert(tn.id, self.nodes.len());
                self.nodes.push(SpecNode::new(&tree, ti));
            }
        }
        self.base_len = self.nodes.len();
        for i in 0..self.nodes.len() {
            // :65 `t_insert(node.linked, self.nodes[otherId])` -- inserting nil is a no-op.
            let linked: Vec<usize> = self.nodes[i]
                .tree_node()
                .linked_id
                .iter()
                .filter_map(|id| self.node_idx(*id))
                .collect();
            self.nodes[i].linked = linked;
        }
    }

    pub(crate) fn alloc(&mut self, i: usize) {
        self.nodes[i].alloc = true;
        self.alloc_nodes.insert(self.nodes[i].id, i);
    }

    pub(crate) fn dealloc(&mut self, i: usize) {
        self.nodes[i].alloc = false;
        self.alloc_nodes.shift_remove(&self.nodes[i].id);
    }

    /// :768-777 -- everything but the two kinds of start node.
    fn reset_nodes(&mut self) {
        for i in 0..self.nodes.len() {
            if !self.nodes[i].ty().is_start() {
                self.dealloc(i);
            }
        }
        self.mastery_selections.clear();
    }

    fn cur_class(&self) -> &crate::tree::TreeClass {
        &self.tree.classes[self.cur_class_id.expect("a class is selected")]
    }

    /// :573-596
    pub fn select_class(&mut self, class_id: f64) -> Result<(), TreeError> {
        if self.cur_class_id.is_some() {
            let old = self.cur_class().start_node_id;
            let i = old.and_then(|id| self.node_idx(id)).ok_or_else(|| {
                TreeError(format!(
                    "Classes/PassiveSpec.lua:577: class {} has no start node in the spec",
                    self.cur_class_name
                ))
            })?;
            self.dealloc(i);
        }
        self.reset_ascend_class();
        let cid = as_id(class_id)
            .map(|c| c as usize)
            .filter(|&c| c < self.tree.classes.len())
            .ok_or_else(|| {
                TreeError(format!(
                    "Classes/PassiveSpec.lua:585: no class with id {class_id} in tree {}",
                    self.tree_version
                ))
            })?;
        self.cur_class_id = Some(cid);
        let cls = &self.tree.classes[cid];
        self.cur_class_name = cls.name.clone();
        let start = cls.start_node_id.and_then(|id| self.node_idx(id)).ok_or_else(|| {
            TreeError(format!(
                "Classes/PassiveSpec.lua:589: class {} has no start node in the spec",
                cls.name
            ))
        })?;
        self.alloc(start);
        self.select_ascend_class(0.0)
    }

    /// `self.curClass.classes[id] or self.curClass.classes[0]`.
    fn ascend_class_index(&self, ascend_class_id: f64) -> usize {
        as_id(ascend_class_id)
            .map(|a| a as usize)
            .filter(|&a| a < self.cur_class().classes.len())
            .unwrap_or(0)
    }

    /// :598-608
    fn reset_ascend_class(&mut self) {
        let Some(cur) = self.cur_ascend_class_id else { return };
        if let Some(old) = self.cur_class().classes[cur].start_node_id {
            if let Some(i) = self.node_idx(old) {
                self.nodes[i].alloc = false;
            }
            self.alloc_nodes.shift_remove(&old);
        }
    }

    /// :610-628
    pub fn select_ascend_class(&mut self, ascend_class_id: f64) -> Result<(), TreeError> {
        self.reset_ascend_class();
        let a = self.ascend_class_index(ascend_class_id);
        // `curAscendClassId` keeps the requested id upstream; `classes[id] or classes[0]` makes every lookup land on `a`.
        self.cur_ascend_class_id = Some(a);
        let asc = &self.tree.classes[self.cur_class_id.expect("class")].classes[a];
        self.cur_ascend_class_name = asc.name.clone();
        self.cur_ascend_class_base_name = asc.id.clone();
        if let Some(start) = asc.start_node_id {
            let i = self.node_idx(start).ok_or_else(|| {
                TreeError(format!(
                    "Classes/PassiveSpec.lua:621: ascendancy {} start node missing from the spec",
                    asc.name
                ))
            })?;
            self.alloc(i);
        }
        self.build_all_depends_and_paths();
        Ok(())
    }

    /// :630-663 -- a no-op without `alternate_ascendancies` (every tree <= 3.21, so every 3.13 golden).
    pub fn select_secondary_ascend_class(&mut self, ascend_class_id: f64) {
        if self.tree.json.alternate_ascendancies.is_none() {
            return;
        }
        self.unimplemented
            .push("PassiveSpec.lua:630-663 SelectSecondaryAscendClass (alternate ascendancies, P5)".into());
        self.cur_secondary_ascend_class_id = Some(ascend_class_id);
    }

    /// :256-312 -- the `className`/`hashOverrides`/`treeVersion` arguments are P5 (always nil from `Load` without tattoos).
    pub fn import_from_node_list(
        &mut self,
        class_id: f64,
        ascend_class_id: f64,
        secondary_ascend_class_id: f64,
        hash_list: &[f64],
        mastery_effects: &NumMap<f64>,
    ) -> Result<(), TreeError> {
        self.reset_nodes();
        self.select_class(class_id)?;
        self.select_ascend_class(ascend_class_id)?;
        self.select_secondary_ascend_class(secondary_ascend_class_id);
        // :273-279 -- selections before allocation, so the mastery check below can see them.
        self.mastery_selections.clear();
        for &(mastery, effect) in &mastery_effects.0 {
            // Ignore ggg profile-import codes (>= 65536). A non-id key can match no node (it would stay in the Lua table).
            if effect < 65536.0
                && let Some(m) = as_id(mastery)
            {
                self.mastery_selections.insert(m, effect);
            }
        }
        for &id in hash_list {
            match as_id(id).and_then(|id| self.node_idx(id)) {
                Some(i) => {
                    // :291 -- an unselected mastery is not allocated at all.
                    if self.nodes[i].ty() != NodeType::Mastery || self.mastery_selections.contains_key(&self.nodes[i].id) {
                        self.alloc(i);
                    }
                }
                // Cluster-jewel subgraph ids (>= 65536) and anything else the spec does not know (P5).
                None => self.alloc_subgraph_nodes.push(id),
            }
        }
        for id in self.alloc_extended_nodes.clone() {
            if let Some(i) = self.node_idx(id) {
                self.alloc(i);
            }
        }
        self.build_all_depends_and_paths();
        Ok(())
    }

    /// :1697-1720 `ReplaceNode(old, treeNode)` -- see the module comment: dropping the own fields IS assigning the tree's.
    fn replace_node(&mut self, i: usize) {
        let node = &mut self.nodes[i];
        // `if old.sd == newNode.sd then return 1 end` -- table identity.
        if node.own_sd_effect.is_none() {
            return;
        }
        node.own_sd_effect = None;
        node.own_sd = None;
        node.own_stats = None;
        // `newNode.reminderText or wipeTable(old.reminderText)` -- wipeTable returns an (own) empty table.
        node.own_reminder_text = if node.tree_node().reminder_text.is_some() {
            None
        } else {
            Some(Vec::new())
        };
    }

    /// :820-827
    fn dealloc_single_node(&mut self, i: usize) {
        self.dealloc(i);
        if self.nodes[i].ty() == NodeType::Mastery {
            // AddMasteryEffectOptionsToNode is display only (P5); the selection still goes.
            self.mastery_selections.shift_remove(&self.nodes[i].id);
        }
    }

    fn alloc_nodes_in_order(&self) -> Vec<usize> {
        let mut list: Vec<usize> = self.alloc_nodes.values().copied().collect();
        // Subgraph nodes follow the Init nodes in `nodes`, so the index order is not the id order.
        match self.alloc_order {
            AllocOrder::IdAsc => list.sort_unstable_by_key(|&i| self.nodes[i].id),
            AllocOrder::IdDesc => list.sort_unstable_by_key(|&i| std::cmp::Reverse(self.nodes[i].id)),
            AllocOrder::Insertion => {}
        }
        list
    }

    /// :1092-1600, the R3 subset: reset dependencies, restore nodes from the tree, apply mastery selections and the four
    /// counters, then prune every allocated node with no path to a start node.
    pub fn build_all_depends_and_paths(&mut self) {
        let mut visited: Vec<usize> = Vec::new();

        // :1141-1209 -- one pass over every node (ascending id, then the subgraph nodes, D13).
        for i in 0..self.nodes.len() {
            if self.check_node_links {
                // :1146-1159 -- drop links to nodes a subgraph rebuild removed, point the rest at the canonical node.
                let mut linked = std::mem::take(&mut self.nodes[i].linked);
                for k in (0..linked.len()).rev() {
                    let other = linked[k];
                    match self.nodes.get(other).and_then(|o| self.index.get(&o.id)).copied() {
                        None => {
                            linked.remove(k);
                        }
                        Some(canonical) => linked[k] = canonical,
                    }
                }
                self.nodes[i].linked = linked;
            }
            let n = &mut self.nodes[i];
            n.depends.clear();
            n.intuitive_leap_likes_affecting.clear();
            // `isUnallocatedMastery` is never true without AddMasteryEffectOptionsToNode (P5).
            self.replace_node(i);
            if self.nodes[i].alloc {
                self.nodes[i].depends.push(i); // :1207 "All nodes depend on themselves"
            }
        }
        self.check_node_links = false;

        // :1409-1466 -- mastery effects and the allocation counters.
        self.allocated_mastery_count = 0;
        self.allocated_notable_count = 0;
        self.allocated_keystone_count = 0;
        self.allocated_mastery_types = IndexMap::new();
        self.allocated_mastery_type_count = 0;
        for i in 0..self.nodes.len() {
            let id = self.nodes[i].id;
            let ty = self.nodes[i].ty();
            let selection = self.mastery_selections.get(&id).copied();
            if ty == NodeType::Mastery
                && let Some(sel) = selection
            {
                let effect_key = as_id(sel).map(u64::from).filter(|k| self.tree.mastery_effects.contains_key(k));
                match effect_key {
                    Some(key) if self.alloc_nodes.contains_key(&id) => {
                        self.apply_mastery_effect(i, key);
                        self.allocated_mastery_count += 1;
                        let Some(type_name) = self.nodes[i].name().map(str::to_owned) else {
                            unimplemented!("Classes/PassiveSpec.lua:1433 allocatedMasteryTypes[nil] (a Lua error)");
                        };
                        match self.allocated_mastery_types.get(&type_name).copied() {
                            None => {
                                self.allocated_mastery_types.insert(type_name, 1);
                                self.allocated_mastery_type_count += 1;
                            }
                            Some(prev) => {
                                self.allocated_mastery_types.insert(type_name, prev + 1);
                                if prev == 0 {
                                    self.allocated_mastery_type_count += 1;
                                }
                            }
                        }
                    }
                    _ => {
                        // :1445-1449 -- the tree does not know this effect, or the node is not allocated.
                        self.dealloc(i);
                        self.mastery_selections.shift_remove(&id);
                    }
                }
            } else if ty == NodeType::Mastery && !self.nodes[i].all_mastery_options {
                // AddMasteryEffectOptionsToNode -- display only (P5); see `unimplemented`.
            } else if ty == NodeType::Notable && self.nodes[i].alloc {
                self.allocated_notable_count += 1;
            } else if ty == NodeType::Keystone && self.nodes[i].alloc {
                self.allocated_keystone_count += 1;
            }
        }

        // :1467-1590 -- orphan pruning. `potentialDeps`/`intuitiveLeaps` stay empty without jewels.
        for i in self.alloc_nodes_in_order() {
            // `pairs(self.allocNodes)` skips a key an earlier iteration set to nil; the snapshot does not, hence the check.
            if !self.alloc_nodes.contains_key(&self.nodes[i].id) {
                continue;
            }
            self.nodes[i].visited = true;
            self.nodes[i].connected_to_start = false;
            let mut any_start_found = self.nodes[i].ty().is_start();
            for li in 0..self.nodes[i].linked.len() {
                let other = self.nodes[i].linked[li];
                if !self.nodes[other].alloc || self.nodes[i].depends.contains(&other) {
                    continue;
                }
                if self.nodes[other].ty().is_start() {
                    any_start_found = true;
                    self.nodes[i].connected_to_start = true;
                } else if self.find_start_from_node(other, &mut visited, false) {
                    any_start_found = true;
                    self.nodes[i].connected_to_start = true;
                    for &n in &visited {
                        self.nodes[n].visited = false;
                    }
                    visited.clear();
                } else {
                    // :1505 -- everything walked on the failed search depends on this node.
                    let dep_ids: std::collections::HashSet<u32> = visited
                        .iter()
                        .filter(|&&n| self.nodes[n].intuitive_leap_likes_affecting.is_empty())
                        .map(|&n| self.nodes[n].id)
                        .collect();
                    for &n in &visited {
                        if self.nodes[n].ty() == NodeType::Mastery {
                            // :1513-1533 -- a mastery with another allocated, not-dependent link is not a dependant.
                            let alloc_links = self.nodes[n].linked.iter().filter(|&&l| self.nodes[l].alloc).count();
                            let other_path = alloc_links > 1
                                && self.nodes[n]
                                    .linked
                                    .iter()
                                    .any(|&l| self.nodes[l].alloc && !dep_ids.contains(&self.nodes[l].id));
                            if !other_path {
                                self.nodes[i].depends.push(n);
                            }
                        } else {
                            // No Intuitive Leap-like jewels: `potentialDeps`/`intuitiveLeaps` stay empty (P5).
                            self.nodes[i].depends.push(n);
                        }
                        self.nodes[n].visited = false;
                    }
                    visited.clear();
                }
            }
            self.nodes[i].visited = false;
            if !any_start_found {
                // :1567-1589 -- no jewels, so `prune` is always true.
                for d in self.nodes[i].depends.clone() {
                    self.dealloc_single_node(d);
                }
            }
        }
    }

    /// :1416-1425 for one allocated mastery: `node.sd = effect.sd`, the reminder text, `ProcessStats(node)`.
    fn apply_mastery_effect(&mut self, i: usize, key: u64) {
        let tree = self.tree.clone();
        let effect = &tree.mastery_effects[&key];
        let node = &mut self.nodes[i];
        node.own_sd_effect = Some(key);
        node.all_mastery_options = false;
        node.own_reminder_text = Some(vec![MASTERY_REMINDER_TEXT.to_owned()]);
        // ProcessStats splits `effect.sd` in place; it was fully processed (split) when the tree was built, so this pass
        // leaves it unchanged -- checked below rather than written back into the shared tree.
        let mut sd = effect.sd.clone();
        let mut stats = NodeStats::default();
        let holder = StatHolder {
            id: f64::from(node.id),
            ty: Some(node.ty()),
            dn: node.dn(),
        };
        process_stats(&holder, &mut sd, &mut stats, 1, tree.parse_mode);
        debug_assert_eq!(sd, effect.sd, "a second ProcessStats split a mastery effect again");
        if stats.unknown {
            node.own_unknown = Some(true);
        }
        if stats.extra {
            node.own_extra = Some(true);
        }
        node.own_sd = Some(sd);
        node.own_stats = Some(stats);
    }

    /// :870-899 -- depth-first search for a start node; a Mastery is never walked through.
    fn find_start_from_node(&mut self, i: usize, visited: &mut Vec<usize>, no_ascend: bool) -> bool {
        self.nodes[i].visited = true;
        visited.push(i);
        let node_ascendancy = self.nodes[i].ascendancy_name().is_some();
        let node_is_mastery = self.nodes[i].ty() == NodeType::Mastery;
        for li in 0..self.nodes[i].linked.len() {
            let other = self.nodes[i].linked[li];
            // `local startIndex = nodeAscendancy and #visited + 1` (1-based).
            let start_index = visited.len() + 1;
            if !self.nodes[other].alloc {
                continue;
            }
            let other_type = self.nodes[other].ty();
            let found = other_type.is_start()
                || (!self.nodes[other].visited && !node_is_mastery && self.find_start_from_node(other, visited, no_ascend));
            if !found {
                continue;
            }
            if node_ascendancy && self.nodes[other].ascendancy_name().is_none() {
                // :890-894 -- pathing out of Ascendant: un-visit everything this step added.
                for &n in &visited[start_index - 1..] {
                    self.nodes[n].visited = false;
                }
                visited.truncate(start_index - 1);
            } else if !no_ascend || other_type != NodeType::AscendClassStart {
                return true;
            }
        }
        false
    }

    /// :846-868. `secondaryAscendNameMap` does not exist without alternate ascendancies (P5).
    pub fn count_alloc_nodes(&self) -> AllocNodeCounts {
        let mut c = AllocNodeCounts::default();
        for n in self.alloc_nodes() {
            if n.ty().is_start() {
                continue;
            }
            if n.ascendancy_name().is_some() {
                if !n.tree_node().is_multiple_choice_option {
                    c.asc_used += 1;
                }
            } else {
                c.used += 1;
            }
            if n.ty() == NodeType::Socket {
                c.sockets += 1;
            }
        }
        c
    }

    /// `PassiveSpecClass:Load` (:103-195) from the parsed `<Spec>` -- the "new format" (a `nodes` attribute), the only one the
    /// 3.13 goldens use. A legacy URL-only spec needs DecodeURL (P5) and is refused instead of yielding an empty tree.
    pub fn load(model: &SpecModel) -> Result<PassiveSpec, TreeError> {
        PassiveSpec::load_mode(model, pob_parser::ParseMode::Full)
    }

    /// `load` on the tree whose stats were read in `mode` (R5.4).
    pub fn load_mode(model: &SpecModel, mode: pob_parser::ParseMode) -> Result<PassiveSpec, TreeError> {
        if model.nodes.is_empty() && model.url.is_some() {
            return Err(TreeError(
                "unimplemented: legacy URL spec (Classes/PassiveSpec.lua:191 DecodeURL)".into(),
            ));
        }
        let tree = crate::tree::load_passive_tree_mode(&model.tree_version, mode)?;
        PassiveSpec::load_with_tree(model, tree)
    }

    /// `load` against an already built tree.
    pub fn load_with_tree(model: &SpecModel, tree: Rc<PassiveTree>) -> Result<PassiveSpec, TreeError> {
        let mut spec = PassiveSpec::new(tree)?;
        // :105 `tonumber(xml.attrib.clusterHashFormatVersion) or (xml.attrib.nodes and 1 or 2)` (pob-build).
        spec.cluster_hash_format_version = model.cluster_hash_format_version;
        spec.jewels = model.jewels.clone();
        if !model.overrides.is_empty() {
            spec.unimplemented.push(format!(
                "PassiveSpec.lua:150-176 {} tattoo <Override> entries (P5)",
                model.overrides.len()
            ));
        }
        spec.import_from_node_list(
            model.class_id,
            model.ascend_class_id,
            model.secondary_ascend_class_id,
            &model.nodes,
            &model.mastery_effects,
        )?;
        Ok(spec)
    }
}
