//! Calc environment: `init_env` (R3.6, KNOWLEDGE/R3-ARCH.md), `perform` without skills + `calculate` (R4.2, R4-ARCH),
//! later defence/estimations (R4.3/R4.4).
//!
//! `init_env` = `calcs.initEnv(build, "MAIN")` (Modules/CalcSetup.lua:475-1975) without the skill half; the R3 gate
//! (R3-ARCH §3) is `tests/env_fixture.rs` against the R3.1 fixtures in `tests/fixtures/` (LuaJIT + pob-ts dumps).
//! `perform` = CalcPerform.lua:1286-3792 (CALC-ARCH §8 subset); its gate is `tests/perform_fixture.rs` against the pob-ts
//! perform-only fixtures (`tools/lua-corpus/export-perform-only.ts`). `defence` = CalcDefence.lua:511-1671 (R4.3), gated by
//! `tests/defence_fixture.rs` against `tools/lua-corpus/export-defence-only.ts`.

#![forbid(unsafe_code)]

pub mod calc_lib;
pub mod calculate;
pub mod defence;
pub mod env;
pub mod init_env;
pub mod output;
pub mod perform;
pub mod pools;
pub mod setup;

pub use calculate::{CalcError, CalculateOptions, CalculateResult, Timings, calculate, calculate_file, calculate_with};
pub use defence::{armour_reduction, armour_reduction_f, defence, hit_chance};
pub use output::{OutValue, Output, StatValue, output_to_stats};
pub use perform::{ActorState, PerformState, PhaseFn, Who, finish_output, noop_phase, perform};

pub use env::{CalcActor, Env, GrantedSkill, MainSkillStub, ReqEntry, Slot};
pub use init_env::{InitEnvError, InitEnvOptions, init_env, item_options, parse_items};

/// `getStats()._meta` (boot.lua:806-816) of a built env -- pob-ts `metaOf` (`src/calc/output.ts`).
///
/// Two fields cannot match upstream before P5 and are reported as they are, not faked: `alloc_nodes` is
/// `#build.spec.allocNodes`, which on a cluster-jewel build also holds the nodes `BuildClusterJewelGraphs` generates (D16);
/// `socket_groups` is `#build.skillsTab.socketGroupList`, which upstream also counts the groups the skill phase appends
/// for item/tree-granted skills (CalcSetup.lua:1461-1490).
#[derive(Clone, Debug, PartialEq)]
pub struct EnvMeta {
    pub alloc_nodes: usize,
    pub items: usize,
    pub socket_groups: usize,
    pub main_socket_group: f64,
    pub tree_version: String,
    pub class_name: String,
    pub ascend_name: String,
}

pub fn meta_of(env: &Env) -> EnvMeta {
    EnvMeta {
        alloc_nodes: env.spec.alloc_nodes.len(),
        items: env.items.len(),
        socket_groups: pob_build::active_skill_groups(&env.build).len(),
        main_socket_group: env.main_socket_group,
        tree_version: env.spec.tree_version.clone(),
        class_name: env.spec.cur_class_name.clone(),
        ascend_name: env.spec.cur_ascend_class_name.clone(),
    }
}
