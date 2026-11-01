//! `Classes/PassiveSpec.lua` -- the subset a calculation needs (pob-ts `src/tree/spec.ts`, CALC-ARCH §3): which nodes a
//! build has allocated, and with what mods. Cluster-jewel subgraphs are `crate::cluster` (R5.5a): their nodes are owned
//! spec nodes appended after the Init nodes. Other jewels (abyss/tattoo/Intuitive Leap)
//! and the URL format are P5; each is listed in `unimplemented`. Node paths (`pathDist`/`path`, R8.7) are [`PassiveSpec::build_paths`].
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
    /// Ascending node id -- the pob-ts order (R3 fixtures).
    IdAsc,
    IdDesc,
    /// Insertion order of `allocNodes` (= XML order for ImportFromNodeList).
    Insertion,
    /// LuaJIT's own `pairs` order: the mirrored table ([`crate::lj_table`]) -- the default since R8.7 (= LuaJIT on the 6 goldens).
    #[default]
    LuaJit,
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

#[derive(Clone)]
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
    /// `node.allMasteryOptions`: set by AddMasteryEffectOptionsToNode (:1029-1059) on an unallocated mastery, cleared when an effect
    /// is applied (:1422).
    pub all_mastery_options: bool,
    /// `node.pathDist` (:1621-1683): 0 for a root (an allocated node), else the passive points to the nearest allocated node; 1000 =
    /// unreachable.
    pub path_dist: f64,
    /// `node.path` (:1621-1683): the nodes to allocate, this node first, the one next to a root last; empty for a root; `None` = nil.
    pub path: Option<Vec<usize>>,
    /// `node.distanceToClassStart` (:1624-1626, `SetNodeDistanceToClassStart` :925-966) -- jewel sockets only; `None` = nil.
    pub distance_to_class_start: Option<f64>,
    /// `node.conqueredBy` (PassiveSpec.lua:1110-1200): the timeless jewel's `jewelData.conqueredBy` (one shared table).
    pub conquered_by: Option<Rc<crate::timeless::ConqueredBy>>,
    /// `ReplaceNode(node, legionNode)` (:1697): dn/sd/name/mods/modKey/modList/keystoneMod now come from the legion node.
    pub(crate) replaced: Option<Rc<crate::timeless::LegionNode>>,
    /// Own `sd`: the `masteryEffects[effect].sd` table (identity is what ReplaceNode compares, :1699).
    pub(crate) own_sd_effect: Option<u64>,
    /// Own `sd`: a mastery effect's, or the table `NodeAdditionOrReplacementFromString` made (:2469/:2473).
    pub(crate) own_sd: Option<Vec<String>>,
    /// Own `mods`/`modKey`/`modList` written by `ProcessStats` on this node, or by a timeless addition/class-start reconnect.
    pub(crate) own_stats: Option<NodeStats>,
    own_unknown: Option<bool>,
    own_extra: Option<bool>,
    pub(crate) own_reminder_text: Option<Vec<String>>,
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
            path_dist: 1000.0,
            path: None,
            distance_to_class_start: None,
            conquered_by: None,
            replaced: None,
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
        match &self.replaced {
            Some(l) => &l.dn,
            None => &self.tree_node().dn,
        }
    }

    /// `node.name` -- a legion node has none (ReplaceNode :1704 copies the nil).
    pub fn name(&self) -> Option<&str> {
        match &self.replaced {
            Some(_) => None,
            None => self.tree_node().name.as_deref(),
        }
    }

    pub fn ascendancy_name(&self) -> Option<&str> {
        self.tree_node().ascendancy_name.as_deref()
    }

    pub fn sd(&self) -> &[String] {
        match (&self.own_sd, &self.replaced) {
            (Some(sd), _) => sd,
            (None, Some(l)) => &l.sd,
            (None, None) => &self.tree_node().sd,
        }
    }

    pub(crate) fn stats(&self) -> &NodeStats {
        match (&self.own_stats, &self.replaced) {
            (Some(st), _) => st,
            (None, Some(l)) => &l.stats,
            (None, None) => &self.tree_node().stats,
        }
    }

    /// `node.modKey`.
    pub fn mod_key_str(&self) -> &str {
        &self.stats().mod_key
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

    /// The tree's, or a legion keystone's after ReplaceNode (:1713).
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
        match (&self.own_reminder_text, &self.replaced) {
            (Some(v), _) => Some(v),
            (None, Some(_)) => None,
            (None, None) => self.tree_node().reminder_text.as_deref(),
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
    /// `node.conqueredBy` -- written by the timeless-jewel half of BuildAllDependsAndPaths (PassiveSpec.lua:1110-1200).
    fn conquered_by(&self) -> bool {
        self.conquered_by.is_some()
    }
    fn sum_base(&self, stat: &str) -> Result<f64, pob_mods::LuaError> {
        let st = pob_mods::ModStore::new_list(None);
        st.borrow_mut().add_list(&SpecNode::mod_list(self).mods);
        st.borrow().sum(pob_mods::ModType::Base, None, &[stat])
    }
}

#[derive(Clone)]
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
    /// What BuildAllDependsAndPaths reads off `self.build.itemsTab.items[itemId]` for the jewels (`crate::timeless`): set by
    /// the caller once the items are parsed (pob-calc `post_load_spec`); empty = no items (a spec loaded without its build).
    pub(crate) jewel_items: Vec<(f64, Rc<crate::timeless::SpecJewel>)>,
    /// `self.allocNodes` as a LuaJIT table (R8.7): every store upstream makes into it, mirrored -- its `pairs` order is what the
    /// dependency pruning (:1470) and the path roots (:1628) walk ([`AllocOrder::LuaJit`]).
    pub(crate) alloc_table: crate::lj_table::LjTable,
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
            alloc_order: AllocOrder::LuaJit,
            jewel_items: Vec::new(),
            alloc_table: crate::lj_table::LjTable::new(),
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
                "PassiveSpec.lua:1212-1220 tattoo node replacement (hashOverrides, P5); abyss conquests (:1110-1127, :1253-1268) are an explicit error",
                "PassiveSpec.lua:1520-1560 Intuitive Leap-like dependency bookkeeping (P5)",
                "PassiveSpec.lua:1591-1617 Intuitive Leap-like path dependencies (potentialDeps / intuitiveLeaps, P5)",
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
        self.alloc_table.set(f64::from(self.nodes[i].id));
    }

    pub(crate) fn dealloc(&mut self, i: usize) {
        self.nodes[i].alloc = false;
        self.alloc_nodes.shift_remove(&self.nodes[i].id);
        self.alloc_nil(self.nodes[i].id);
    }

    /// `self.allocNodes[id] = nil` on the mirrored table (the key may be absent: LuaJIT then creates it, dead).
    pub(crate) fn alloc_nil(&mut self, id: u32) {
        self.alloc_table.remove(f64::from(id), crate::lj_table::NIL_STORE_CREATES_KEY);
    }

    /// `pairs(self.allocNodes)` of the mirrored table (ids).
    pub fn alloc_table_pairs(&self) -> Vec<u32> {
        self.alloc_table.pairs_keys().into_iter().map(|k| k as u32).collect()
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
            self.alloc_nil(old);
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
        self.build_all_depends_and_paths()
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
        self.build_all_depends_and_paths()
    }

    /// :1697-1720 `ReplaceNode(old, treeNode)` -- see the module comment: dropping the own fields IS assigning the tree's.
    fn replace_node(&mut self, i: usize) {
        let node = &mut self.nodes[i];
        // `if old.sd == newNode.sd then return 1 end` -- table identity: the node still holds the tree's `sd` unless a mastery
        // effect, a legion node or a timeless addition gave it another table.
        if node.own_sd.is_none() && node.replaced.is_none() {
            return;
        }
        node.own_sd_effect = None;
        node.own_sd = None;
        node.own_stats = None;
        node.replaced = None;
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
            self.add_mastery_effect_options_to_node(i);
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
            AllocOrder::LuaJit => {
                list = self
                    .alloc_table
                    .pairs_keys()
                    .into_iter()
                    .map(|k| self.alloc_nodes[&(k as u32)])
                    .collect();
                debug_assert_eq!(
                    list.len(),
                    self.alloc_nodes.len(),
                    "the mirrored allocNodes table holds the allocated set"
                );
            }
        }
        list
    }

    /// :1092-1600, the R3 subset: reset dependencies, restore nodes from the tree, the timeless-jewel conquests (R5.5c,
    /// `crate::timeless`), apply mastery selections and the four counters, then prune every allocated node with no path to a
    /// start node. Errors: what upstream raises in the conquest half (and its unported abyss branch).
    pub fn build_all_depends_and_paths(&mut self) -> Result<(), TreeError> {
        let mut visited: Vec<usize> = Vec::new();
        // :1110-1140 -- the abyss conquests (an error, out of scope) and the radius jewels of `self.jewels`.
        let radius_jewels = self.gather_radius_jewels()?;

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
            // :1162-1166 `isUnallocatedMastery` (no runegrafts: `hashOverrides` is empty) keeps its options.
            let unallocated_mastery = n.all_mastery_options && n.ty() == NodeType::Mastery;
            if !unallocated_mastery {
                self.replace_node(i);
            }
            // :1167 `node.conqueredBy = abyssConquests[id]` (nil -- see gather_radius_jewels), then :1169-1203.
            self.nodes[i].conquered_by = self.radius_conquest(i, &radius_jewels);
            self.nodes[i].intuitive_leap_likes_affecting = self.radius_intuitive_leaps(i, &radius_jewels);
            if self.nodes[i].alloc {
                self.nodes[i].depends.push(i); // :1207 "All nodes depend on themselves"
            }
        }
        self.check_node_links = false;
        // :1210-1407 -- tattoos are P5; the conquered nodes are replaced or get their additions.
        for i in 0..self.nodes.len() {
            if self.nodes[i].conquered_by.is_some() && self.nodes[i].ty() != NodeType::Socket {
                self.conquer_node(i)?;
            }
        }

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
                self.add_mastery_effect_options_to_node(i);
            } else if ty == NodeType::Notable && self.nodes[i].alloc {
                self.allocated_notable_count += 1;
            } else if ty == NodeType::Keystone && self.nodes[i].alloc {
                self.allocated_keystone_count += 1;
            }
        }

        // :1467-1590 -- orphan pruning, with the Intuitive Leap-like bookkeeping: `potentialDeps` (node id -> the allocated nodes it may
        // depend on) and `intuitiveLeaps` (node id -> nodes that may depend on it), both keyed in first-insertion order (walked with
        // `pairs` below: a table of integer keys only ever set, [`crate::lj_table::pairs_order`]).
        let mut potential_deps: IndexMap<u32, Vec<usize>> = IndexMap::new();
        let mut intuitive_leaps: IndexMap<u32, Vec<usize>> = IndexMap::new();
        for i in self.alloc_nodes_in_order() {
            // `pairs(self.allocNodes)` skips a key an earlier iteration set to nil; the snapshot does not, hence the check.
            if !self.alloc_nodes.contains_key(&self.nodes[i].id) {
                continue;
            }
            let node_id = self.nodes[i].id;
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
                            // :1535-1551 -- a node an Intuitive Leap-like jewel affects may depend on this one.
                            if self.nodes[n].intuitive_leap_likes_affecting.is_empty() {
                                self.nodes[i].depends.push(n);
                            } else {
                                potential_deps.entry(self.nodes[n].id).or_default().push(i);
                            }
                            // :1553-1561 -- if `n` holds such a jewel, the nodes in its radius may depend on this one.
                            let affected = self.nodes_in_intuitive_leap_like_radius(n);
                            intuitive_leaps.entry(node_id).or_default().extend(affected);
                        }
                        self.nodes[n].visited = false;
                    }
                    visited.clear();
                }
            }
            self.nodes[i].visited = false;
            if !any_start_found {
                // :1567-1589 -- no start found through ANY node: this node and its dependants are orphans, unless an Intuitive
                // Leap-like jewel holds them.
                for d in self.nodes[i].depends.clone() {
                    match self.intuitive_leap_holder(d) {
                        Some(holder) => intuitive_leaps.entry(holder).or_default().push(d),
                        None => self.dealloc_single_node(d),
                    }
                }
            }
        }

        // :1595-1611 -- the potential dependants whose every jewel socket depends on a node depend on it too.
        let keys: Vec<f64> = potential_deps.keys().map(|&k| f64::from(k)).collect();
        for k in crate::lj_table::pairs_order(&keys) {
            let id = k as u32;
            let potential = self.node_idx(id).expect("a spec node");
            let mut seen: Vec<u32> = Vec::new();
            for &node in &potential_deps[&id] {
                let all_dep = self.nodes[potential]
                    .intuitive_leap_likes_affecting
                    .iter()
                    .all(|p| self.nodes[node].depends.contains(p));
                if all_dep && !seen.contains(&self.nodes[node].id) {
                    self.nodes[node].depends.push(potential);
                    seen.push(self.nodes[node].id);
                }
            }
        }
        // :1613-1630
        let keys: Vec<f64> = intuitive_leaps.keys().map(|&k| f64::from(k)).collect();
        for k in crate::lj_table::pairs_order(&keys) {
            let id = k as u32;
            let node = self.node_idx(id).expect("a spec node");
            let mut seen: Vec<u32> = Vec::new();
            for &dep in &intuitive_leaps[&id] {
                if self.nodes[dep].connected_to_start {
                    continue;
                }
                let all_dep = self.nodes[dep]
                    .intuitive_leap_likes_affecting
                    .iter()
                    .all(|x| self.nodes[node].depends.contains(x));
                if all_dep && !seen.contains(&self.nodes[dep].id) {
                    self.nodes[node].depends.push(dep);
                    seen.push(self.nodes[dep].id);
                }
            }
        }

        let roots = self.alloc_pairs_order();
        self.build_paths(&roots);
        Ok(())
    }

    /// `pairs(self.allocNodes)` as indices into `nodes` -- what the path roots (:1628) are walked in. See [`PassiveSpec::alloc_order`].
    pub fn alloc_pairs_order(&self) -> Vec<usize> {
        self.alloc_nodes_in_order()
    }

    /// :1619-1694 -- reset every node's path, then a multi-source 0-1 BFS from the allocated nodes (`roots`, in `pairs(self.allocNodes)`
    /// order: which of two equally short paths a node gets depends on it), then `SetNodeDistanceToClassStart` for the jewel-socket roots.
    /// `BuildSplitPersonalityPath` (:1694, `self.splitPersonalityPath`) is read by the tree view only (PassiveTreeView.lua:389).
    pub fn build_paths(&mut self, roots: &[usize]) {
        // :1620-1626 `pairs(self.nodes)` -- every node gets its own values, the order cannot matter.
        for n in &mut self.nodes {
            n.path_dist = if n.alloc && n.intuitive_leap_likes_affecting.is_empty() {
                0.0
            } else {
                1000.0
            };
            n.path = None;
            let tn = n.tree_node();
            if tn.is_jewel_socket || tn.expansion_jewel.is_some() {
                n.distance_to_class_start = Some(0.0);
            }
        }
        // :1627-1632
        let root_list: Vec<usize> = roots
            .iter()
            .copied()
            .filter(|&i| self.nodes[i].intuitive_leap_likes_affecting.is_empty() || self.nodes[i].connected_to_start)
            .collect();
        // :1636-1642 -- a Lua array whose start may move below 1 (front insertions land on slots already consumed): `pad` free
        // slots before upstream's index 1.
        let pad = self.nodes.len() + 1;
        let mut queue: Vec<usize> = vec![usize::MAX; pad];
        for &i in &root_list {
            self.nodes[i].path_dist = 0.0;
            self.nodes[i].path = Some(Vec::new());
            queue.push(i);
        }
        let mut queue_start = pad;
        let mut queue_length = queue.len() - 1;
        while queue_start <= queue_length {
            let node = queue[queue_start];
            queue_start += 1;
            let node_dist = self.nodes[node].path_dist;
            let node_path = self.nodes[node].path.clone().unwrap_or_default();
            let node_mastery = self.nodes[node].ty() == NodeType::Mastery;
            for li in 0..self.nodes[node].linked.len() {
                let other = self.nodes[node].linked[li];
                let weight = if self.nodes[other].alloc { 0.0 } else { 1.0 };
                let dist_via_node = node_dist + weight;
                let other_ty = self.nodes[other].ty();
                let node_asc = self.nodes[node].ascendancy_name();
                let other_asc = self.nodes[other].ascendancy_name();
                let can_traverse = !node_mastery
                    && other_ty != NodeType::ClassStart
                    && other_ty != NodeType::AscendClassStart
                    && (node_asc == other_asc || (node_dist == 0.0 && other_asc.is_none()));
                if dist_via_node < self.nodes[other].path_dist && can_traverse {
                    if weight == 0.0 {
                        // Free nodes go to the front so they can shorten paid paths immediately.
                        queue_start -= 1;
                        queue[queue_start] = other;
                    } else {
                        queue_length += 1;
                        if queue.len() <= queue_length {
                            queue.push(other);
                        } else {
                            queue[queue_length] = other;
                        }
                    }
                    self.nodes[other].path_dist = dist_via_node;
                    let mut path = Vec::with_capacity(node_path.len() + 1);
                    path.push(other);
                    path.extend_from_slice(&node_path);
                    self.nodes[other].path = Some(path);
                }
            }
        }
        // :1686-1690
        for &i in &root_list {
            let tn = self.nodes[i].tree_node();
            if tn.is_jewel_socket || tn.expansion_jewel.is_some() {
                self.set_node_distance_to_class_start(i);
            }
        }
    }

    /// :925-966 -- breadth-first over allocated nodes from `root` until the class start is a neighbour.
    fn set_node_distance_to_class_start(&mut self, root: usize) {
        self.nodes[root].distance_to_class_start = Some(0.0);
        if !self.nodes[root].alloc || !self.nodes[root].connected_to_start {
            return;
        }
        let target = self.cur_class().start_node_id;
        let mut dist: std::collections::HashMap<u32, f64> = std::collections::HashMap::new();
        dist.insert(self.nodes[root].id, 0.0);
        let mut queue = vec![root];
        let mut o = 0;
        while o < queue.len() {
            let node = queue[o];
            o += 1;
            let cur_dist = dist[&self.nodes[node].id] + 1.0;
            for li in 0..self.nodes[node].linked.len() {
                let other = self.nodes[node].linked[li];
                if Some(self.nodes[other].id) == target {
                    self.nodes[root].distance_to_class_start = Some(cur_dist - 1.0);
                    return;
                }
                let ot = self.nodes[other].ty();
                if self.nodes[other].alloc
                    && self.nodes[node].ty() != NodeType::Mastery
                    && ot != NodeType::ClassStart
                    && ot != NodeType::AscendClassStart
                    && !dist.contains_key(&self.nodes[other].id)
                {
                    dist.insert(self.nodes[other].id, cur_dist);
                    queue.push(other);
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
            str_id: None,
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

    /// CalcsTab.lua:516-529 `buildMasteryEffectNode(node, effect)`: a plain table `{ id, type, name, sd = <copy of effect.sd> }` run through
    /// `ProcessStats` -- what node power hands initEnv as `override.addNodes` for one effect of the unallocated mastery `i`. Its fields
    /// beyond those four read through to the mastery's tree node here (a mastery has no ascendancy, socket or keystone fields).
    pub fn mastery_effect_node(&self, i: usize, effect_key: u64) -> SpecNode {
        let tree = self.tree.clone();
        let src = &self.nodes[i];
        let effect = &tree.mastery_effects[&effect_key];
        let mut node = SpecNode::with_base(&tree, src.id, src.base.clone());
        let mut sd = effect.sd.clone();
        let mut stats = NodeStats::default();
        let holder = StatHolder {
            id: f64::from(src.id),
            str_id: None,
            ty: Some(src.ty()),
            dn: "",
        };
        process_stats(&holder, &mut sd, &mut stats, 1, tree.parse_mode);
        if stats.unknown {
            node.own_unknown = Some(true);
        }
        if stats.extra {
            node.own_extra = Some(true);
        }
        node.own_sd = Some(sd);
        node.own_stats = Some(stats);
        node.own_reminder_text = Some(Vec::new());
        node
    }

    /// The tree node `tree_idx` itself (`tree.clusterNodeMap` entries, CalcsTab.lua:714-736) as initEnv sees it in `override.addNodes`.
    pub fn tree_node_view(&self, tree_idx: usize) -> SpecNode {
        SpecNode::new(&self.tree, tree_idx)
    }

    /// This spec with `extra` appended to `nodes` -- not in the id index, not linked, not allocated: how initEnv reaches the
    /// `override.addNodes` that are no spec nodes (`env.allocNodes` refers to them by index, as to every node).
    pub fn with_extra_nodes(&self, extra: Vec<SpecNode>) -> PassiveSpec {
        let mut s = self.clone();
        s.nodes.extend(extra);
        s
    }

    /// :1029-1059 `AddMasteryEffectOptionsToNode`: the node gets the stats of ALL its effects (`cacheNode` -- `{ id, sd }` run through
    /// `ProcessStats` after every effect, from line 1 each time, so the last run over every line is what stays), its reminder text goes
    /// back to the tree's, and `allMasteryOptions` is set. What node power reads of it: `modKey` (CalcsTab.lua:567) and the flag.
    fn add_mastery_effect_options_to_node(&mut self, i: usize) {
        let tree = self.tree.clone();
        let node = &mut self.nodes[i];
        let mut sd: Vec<String> = Vec::new();
        for &e in &node.tree_node().mastery_effect_ids {
            let effect = as_id(e).and_then(|k| tree.mastery_effects.get(&u64::from(k))).unwrap_or_else(|| {
                panic!(
                    "Classes/PassiveSpec.lua:1037: attempt to index local 'effect' (a nil value) -- mastery {} effect {e}",
                    node.id
                )
            });
            sd.extend(effect.sd.iter().cloned());
        }
        let mut stats = NodeStats::default();
        // `cacheNode` has no `type` (no keystone mod) and no `dn`.
        let holder = StatHolder {
            id: f64::from(node.id),
            str_id: None,
            ty: None,
            dn: "",
        };
        process_stats(&holder, &mut sd, &mut stats, 1, tree.parse_mode);
        // `for k, v in pairs(cacheNode)`: `unknown`/`extra` exist on it only when set.
        if stats.unknown {
            node.own_unknown = Some(true);
        }
        if stats.extra {
            node.own_extra = Some(true);
        }
        node.own_sd_effect = None;
        node.own_sd = Some(sd);
        node.own_stats = Some(stats);
        // `node.reminderText = nil` -- the tree node's shows through again.
        node.own_reminder_text = None;
        node.all_mastery_options = true;
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
        PassiveSpec::load_with_tree_jewels(model, tree, Vec::new())
    }

    /// [`PassiveSpec::load_mode`] with the socketed jewels' item data known from the start (R8.7): the items tab is loaded before the
    /// tree (Build.lua:655-677), so the BuildAllDependsAndPaths of `ImportFromNodeList` already sees them (conquests, Intuitive Leap).
    pub fn load_mode_jewels(
        model: &SpecModel,
        mode: pob_parser::ParseMode,
        jewels: Vec<(f64, Rc<crate::timeless::SpecJewel>)>,
    ) -> Result<PassiveSpec, TreeError> {
        if model.nodes.is_empty() && model.url.is_some() {
            return Err(TreeError(
                "unimplemented: legacy URL spec (Classes/PassiveSpec.lua:191 DecodeURL)".into(),
            ));
        }
        let tree = crate::tree::load_passive_tree_mode(&model.tree_version, mode)?;
        PassiveSpec::load_with_tree_jewels(model, tree, jewels)
    }

    /// [`PassiveSpec::load_with_tree`] with the jewels' item data.
    pub fn load_with_tree_jewels(
        model: &SpecModel,
        tree: Rc<PassiveTree>,
        jewels: Vec<(f64, Rc<crate::timeless::SpecJewel>)>,
    ) -> Result<PassiveSpec, TreeError> {
        let mut spec = PassiveSpec::new(tree)?;
        spec.jewel_items = jewels;
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
