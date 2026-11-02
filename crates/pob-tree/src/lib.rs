//! The passive tree (`Classes/PassiveTree.lua`) and a build's allocation of it (`Classes/PassiveSpec.lua`) -- the subset
//! `initEnv` needs (CALC-ARCH §3, R3-ARCH §1). Executable spec: pob-ts `src/tree/*`; truth: LuaJIT (fixtures
//! `crates/pob-calc/tests/fixtures/<build>.{luajit,pobts}.json`, key `tree`).
//!
//! Cluster jewels (`BuildClusterJewelGraphs`/`BuildSubgraph`, R5.5a) are in [`cluster`]; the caller runs
//! [`PassiveSpec::post_load`] with the items once they are parsed (pob-calc `init_env`).
//!
//! Jewel radii (`nodesInRadius`, R5.5b) are in [`radius`]; the radius-jewel passes themselves run in pob-calc (CalcSetup).
//!
//! Timeless jewels (legion passives, LUT conquests, R5.5c) are in [`timeless`]; the caller hands the spec the jewels' item data
//! ([`PassiveSpec::set_jewel_items`]) before `PostLoad`.
//!
//! Tattoos (`<Override>`, `hashOverrides`, R-tattoo) are in [`tattoo`]: the tattoo passives of the tree and their ReplaceNode in the spec.
//!
//! Not ported (explicit `unimplemented` lists on `PassiveTree`/`PassiveSpec`): abyss jewels, URL specs,
//! display-only assets.

#![forbid(unsafe_code)]

pub mod cluster;
pub mod lj_table;
pub mod modtools;
pub mod radius;
pub mod spec;
pub mod tattoo;
pub mod timeless;
pub mod tree;

pub use cluster::{ClusterJewel, JewelLookup};
pub use pob_parser::ParseMode;
pub use radius::{NodesInRadius, RadiusInfo, set_jewel_radii};
pub use spec::{AllocNodeCounts, AllocOrder, PassiveSpec, SpecNode};
pub use tattoo::{Tattoo, TattooNode};
pub use timeless::{ConqueredBy, ConquerorId, Legion, LegionNode, SpecJewel};
pub use tree::{
    AscendClass, AscendEntry, MasteryEffect, NodeModLine, NodeStats, NodeType, PassiveTree, TreeClass, TreeError, TreeGroup, TreeNode,
    calc_orbit_angles, load_passive_tree, load_passive_tree_mode,
};
