//! Tattoos (R-tattoo): the tattoo passives of the tree (PassiveTree.lua:80 `require("Data.TattooPassives")`, processed at :758-786) and
//! the spec's `hashOverrides` -- loaded from `<Overrides>` (PassiveSpec.lua:161-186), put on the nodes by `ImportFromNodeList` (:300-306)
//! and `BuildAllDependsAndPaths` (:1186, :1236-1244, :1452-1453, :1477, :1484-1490) through `ReplaceNode` (:1726-1746), saved by
//! `PassiveSpec:Save` (:247-265). Line for line; the data is `pob_data::tattoo` (a LuaJIT data patch).
//!
//! `hashOverrides[nodeId]` is `copyTable(tattoo.nodes[dn], true)` with `.id = nodeId` (:181-182): a SHALLOW copy, so it shares `sd`,
//! `mods`, `modList` and `keystoneMod` with the tattoo node -- here one `Rc<TattooNode>` per tattoo, whose identity is the identity of
//! its `sd` table (what ReplaceNode compares, :1728). The copy's own fields (`id`, `sprites`, `effectSprites`) are display only.
//!
//! A spec node is `setmetatable({...}, treeNode)` (:55-58) with `treeNode.__index = treeNode` (PassiveTree.lua:542): a field ReplaceNode
//! sets to a tattoo's `nil` (`name` on all but the runegrafts, `keystoneMod` on all but the keystone tattoos) reads the tree node's again
//! -- [`crate::SpecNode::name`] / [`crate::SpecNode::keystone_mod`].

use std::rc::Rc;

use indexmap::IndexMap;
use pob_data::tattoo::tattoo_passives;

use crate::spec::{MASTERY_REMINDER_TEXT, PassiveSpec, Replaced};
use crate::tree::{NodeStats, NodeType, PassiveTree, StatHolder, TreeError, as_id, process_stats};

/// A processed `tattoo.nodes[dn]` (PassiveTree.lua:759-786: type, sprites, `idMap`, ProcessStats).
#[derive(Debug)]
pub struct TattooNode {
    /// The data's string id (`"acrobatics1136"`): the mods are sourced `"Tree:" .. id` (ProcessStats on the tattoo node, :785).
    pub id: String,
    pub dn: String,
    /// `nil` on all but the runegrafts (`"Runegraft Mastery"`).
    pub name: Option<String>,
    /// After ProcessStats (split in place).
    pub sd: Vec<String>,
    /// `node.type` of the tattoo node itself (:760-769) -- ReplaceNode does not copy it.
    pub ty: NodeType,
    pub stats: NodeStats,
    pub is_tattoo: bool,
    pub override_type: String,
    pub icon: String,
    pub active_effect_image: String,
    pub reminder_text: Option<Vec<String>>,
}

/// `tree.tattoo` (only `nodes`; `groups` is display only, `idMap` -- :780-783 -- is read by CalcBreakdownControl only).
#[derive(Debug)]
pub struct Tattoo {
    /// dn -> node, in the patch's (byte-sorted) key order.
    pub nodes: IndexMap<String, Rc<TattooNode>>,
}

impl PassiveTree {
    /// `self.tattoo` after PassiveTree.lua:758-786 (built on first use; nothing else reads the raw table).
    pub fn tattoo(&self) -> Result<&Tattoo, TreeError> {
        self.tattoo.get_or_init(|| build_tattoo(self)).as_ref().map_err(Clone::clone)
    }
}

/// PassiveTree.lua:758-786. Upstream processes the one `require`d table again for every tree it builds (the same stats, new mod objects);
/// nothing in it depends on the tree, so once per tree here.
fn build_tattoo(tree: &PassiveTree) -> Result<Tattoo, TreeError> {
    let raw = tattoo_passives().map_err(|e| TreeError(e.0))?;
    let mut nodes = IndexMap::with_capacity(raw.nodes.len());
    for (key, n) in raw.nodes.iter() {
        // :760-769 -- determine node type
        let ty = if n.m {
            NodeType::Mastery
        } else if n.ks {
            NodeType::Keystone
        } else if n.not_ {
            NodeType::Notable
        } else {
            NodeType::Normal
        };
        // :785 `self:ProcessStats(node)` -- `node.id` is the data's string id.
        let holder = StatHolder {
            id: 0.0,
            str_id: Some(&n.id),
            ty: Some(ty),
            dn: &n.dn,
        };
        let mut sd = n.sd.clone();
        let mut stats = NodeStats::default();
        process_stats(&holder, &mut sd, &mut stats, 1, tree.parse_mode);
        nodes.insert(
            key.clone(),
            Rc::new(TattooNode {
                id: n.id.clone(),
                dn: n.dn.clone(),
                name: n.name.clone(),
                sd,
                ty,
                stats,
                is_tattoo: n.is_tattoo,
                override_type: n.override_type.clone(),
                icon: n.icon.clone(),
                active_effect_image: n.active_effect_image.clone(),
                reminder_text: n.reminder_text.clone(),
            }),
        );
    }
    Ok(Tattoo { nodes })
}

/// One `<Override>` as `PassiveSpec:Load` reads it (:161-186).
#[derive(Clone, Debug, Default)]
pub struct OverrideInput {
    /// `tonumber(child.attrib.nodeId)` (`None`: the attribute is missing).
    pub node_id: Option<f64>,
    pub dn: Option<String>,
    pub icon: Option<String>,
    pub active_effect_image: Option<String>,
}

impl PassiveSpec {
    /// PassiveSpec.lua:161-186 -- `self.hashOverrides` from the `<Override>` children of `<Overrides>`. A renamed tattoo is looked up by its
    /// art (`activeEffectImage` + `icon`, :169-176): upstream walks `pairs(tattoo.nodes)` (string keys, MINEFIELD R13 -- no fixed order) and
    /// keeps the LAST match; here the patch's key order, and more than one match is recorded in `unimplemented`. Upstream also writes the
    /// match into the shared `tattoo.nodes` under the old name (:172) -- a later spec of the process then finds it directly; the result is
    /// the same node. A tattoo found neither way is dropped (:184 ConPrintf).
    pub fn load_overrides(&mut self, overrides: &[OverrideInput]) -> Result<(), TreeError> {
        if overrides.is_empty() {
            return Ok(());
        }
        let tree = self.tree.clone();
        let tattoo = tree.tattoo()?;
        for o in overrides {
            let Some(node_id) = o.node_id else {
                // :163-165 `launch:ShowErrMsg(...)`, `return true` -- the build does not load.
                return Err(TreeError(
                    "Classes/PassiveSpec.lua:164: Error parsing: 'Override' element missing 'nodeId' attribute".into(),
                ));
            };
            let Some(dn) = o.dn.as_deref() else {
                // `tattoo.nodes[nil]` is nil; the art search can match nothing that would not then fail on `nodes[nil] = data` (:172) or
                // the `..` of :184 -- a Lua error either way.
                return Err(TreeError(
                    "Classes/PassiveSpec.lua:184: attempt to concatenate field 'dn' (a nil value)".into(),
                ));
            };
            let found = match tattoo.nodes.get(dn) {
                Some(t) => Some(t.clone()),
                None => {
                    // :169-176
                    let matches: Vec<&Rc<TattooNode>> = tattoo
                        .nodes
                        .values()
                        .filter(|t| {
                            Some(t.active_effect_image.as_str()) == o.active_effect_image.as_deref()
                                && Some(t.icon.as_str()) == o.icon.as_deref()
                        })
                        .collect();
                    if matches.len() > 1 {
                        self.unimplemented.push(format!(
                            "PassiveSpec.lua:170 tattoo {dn:?} substituted by art: {} candidates, pairs order (R13) picks one -- the last in data order taken",
                            matches.len()
                        ));
                    }
                    matches.last().map(|t| (*t).clone())
                }
            };
            // :179-185
            if let Some(t) = found {
                let Some(id) = as_id(node_id) else {
                    unimplemented!("Classes/PassiveSpec.lua:181 hashOverrides keyed by the non-id number {node_id}");
                };
                self.hash_overrides.insert(id, t);
            }
        }
        Ok(())
    }

    /// PassiveSpec.lua:1726-1746 `ReplaceNode(old, tattooNode)` on spec node `i`.
    pub(crate) fn replace_with_tattoo(&mut self, i: usize, t: &Rc<TattooNode>) {
        let node = &mut self.nodes[i];
        // `if old.sd == newNode.sd then return 1 end` -- the node still holds this tattoo's `sd` table.
        if node.own_sd.is_none() && matches!(&node.replaced, Some(Replaced::Tattoo(r)) if Rc::ptr_eq(r, t)) {
            return;
        }
        node.replaced = Some(Replaced::Tattoo(t.clone()));
        node.own_sd_effect = None;
        node.own_sd = None;
        // `old.modList = new("ModList"); AddList(newNode.modList)` -- the same mod objects (read through `replaced`).
        node.own_stats = None;
        // `newNode.reminderText or wipeTable(old.reminderText)`
        node.own_reminder_text = Some(t.reminder_text.clone().unwrap_or_default());
    }

    /// PassiveSpec.lua:1452-1458 for an allocated mastery with a runegraft: `ReplaceNode(node, hashOverrides[id])` (a no-op after
    /// :1237 -- the same `sd`), the mastery flags and `self.tree:ProcessStats(node)` on the SPEC node: the runegraft's lines again, now
    /// sourced `"Tree:" .. <node id>` (the number) instead of the tattoo's string id; `node.sd` stays the tattoo's table.
    pub(crate) fn apply_tattoo_mastery(&mut self, i: usize, t: &Rc<TattooNode>) {
        self.replace_with_tattoo(i, t);
        let tree = self.tree.clone();
        let node = &mut self.nodes[i];
        node.all_mastery_options = false;
        node.own_reminder_text = Some(vec![MASTERY_REMINDER_TEXT.to_owned()]);
        let mut sd = node.sd().to_vec();
        let before = sd.clone();
        let mut stats = NodeStats::default();
        let holder = StatHolder {
            id: f64::from(node.id),
            str_id: None,
            ty: Some(node.ty()),
            dn: node.dn(),
        };
        process_stats(&holder, &mut sd, &mut stats, 1, tree.parse_mode);
        debug_assert_eq!(sd, before, "a second ProcessStats split a tattoo's lines again");
        if stats.unknown {
            node.own_unknown = Some(true);
        }
        if stats.extra {
            node.own_extra = Some(true);
        }
        // `node.keystoneMod` is not written for a Mastery (ProcessStats :871): it stays what ReplaceNode left.
        stats.keystone_mod = None;
        node.own_stats = Some(stats);
    }
}
