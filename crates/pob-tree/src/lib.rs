//! The passive tree (`Classes/PassiveTree.lua`) and a build's allocation of it (`Classes/PassiveSpec.lua`) -- the subset
//! `initEnv` needs (CALC-ARCH §3, R3-ARCH §1). Executable spec: pob-ts `src/tree/*`; truth: LuaJIT (fixtures
//! `crates/pob-calc/tests/fixtures/<build>.{luajit,pobts}.json`, key `tree`).
//!
//! Cluster jewels (`BuildClusterJewelGraphs`/`BuildSubgraph`, R5.5a) are in [`cluster`]; the caller runs
//! [`PassiveSpec::post_load`] with the items once they are parsed (pob-calc `init_env`).
//!
//! Jewel radii (`nodesInRadius`, R5.5b) are in [`radius`]; the radius-jewel passes themselves run in pob-calc (CalcSetup).
//!
//! Not ported (explicit `unimplemented` lists on `PassiveTree`/`PassiveSpec`): timeless/abyss jewels, tattoos,
//! paths/`pathDist`, URL specs, legion/tattoo passives, alternate ascendancies, display-only assets.

#![forbid(unsafe_code)]

pub mod cluster;
pub mod modtools;
pub mod radius;
pub mod spec;
pub mod tree;

pub use cluster::{ClusterJewel, JewelLookup};
pub use pob_parser::ParseMode;
pub use radius::{NodesInRadius, RadiusInfo, set_jewel_radii};
pub use spec::{AllocNodeCounts, AllocOrder, PassiveSpec, SpecNode};
pub use tree::{
    AscendClass, AscendEntry, MasteryEffect, NodeModLine, NodeStats, NodeType, PassiveTree, TreeClass, TreeError, TreeGroup, TreeNode,
    calc_orbit_angles, load_passive_tree, load_passive_tree_mode,
};
