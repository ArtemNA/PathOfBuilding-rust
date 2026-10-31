//! The passive tree (`Classes/PassiveTree.lua`) and a build's allocation of it (`Classes/PassiveSpec.lua`) -- the subset
//! `initEnv` needs (CALC-ARCH §3, R3-ARCH §1). Executable spec: pob-ts `src/tree/*`; truth: LuaJIT (fixtures
//! `crates/pob-calc/tests/fixtures/<build>.{luajit,pobts}.json`, key `tree`).
//!
//! Not ported (explicit `unimplemented` lists on `PassiveTree`/`PassiveSpec`): cluster jewels (`BuildClusterJewelGraphs`,
//! ids >= 65536 are kept in `alloc_subgraph_nodes`), radius/timeless/abyss jewels, tattoos, paths/`pathDist`, URL specs,
//! legion/tattoo passives, alternate ascendancies, display-only assets.

#![forbid(unsafe_code)]

pub mod modtools;
pub mod spec;
pub mod tree;

pub use spec::{AllocNodeCounts, AllocOrder, PassiveSpec, SpecNode};
pub use tree::{
    AscendClass, AscendEntry, MasteryEffect, NodeModLine, NodeStats, NodeType, PassiveTree, TreeClass, TreeError, TreeGroup, TreeNode,
    calc_orbit_angles, load_passive_tree,
};
