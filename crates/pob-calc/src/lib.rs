//! Calc environment: `init_env` (R3.6, KNOWLEDGE/R3-ARCH.md), `perform` without skills + `calculate` (R4.2, R4-ARCH),
//! later defence/estimations (R4.3/R4.4).
//!
//! `init_env` = `calcs.initEnv(build, "MAIN")` (Modules/CalcSetup.lua:475-1975) without the skill half; the R3 gate
//! (R3-ARCH §3) is `tests/env_fixture.rs` against the R3.1 fixtures in `tests/fixtures/` (LuaJIT + pob-ts dumps).
//! `perform` = CalcPerform.lua:1286-3792 (CALC-ARCH §8 subset); its gate is `tests/perform_fixture.rs` against the pob-ts
//! perform-only fixtures (`tools/lua-corpus/export-perform-only.ts`). `defence` = CalcDefence.lua:511-1671 (R4.3), gated by
//! `tests/defence_fixture.rs` against `tools/lua-corpus/export-defence-only.ts`. `estimations` = CalcDefence.lua:1674-3786
//! (`buildDefenceEstimations`, R4.4b) on top of `pools`; the full output (perform + defence + estimations) is gated by
//! `tests/full_fixture.rs` against the pob-ts full-output fixtures (`<build>.r4-pobts.json`, R4-ARCH gate (a)).

#![forbid(unsafe_code)]

pub mod active_skill;
pub mod allowlist;
pub mod calc_lib;
pub mod calc_tools;
pub mod calculate;
pub mod defence;
pub mod env;
pub mod estimations;
pub mod init_env;
pub(crate) mod offence;
pub mod output;
pub mod perform;
pub mod pools;
pub mod session;
pub mod setup;
pub mod skill_funcs;
pub mod skill_modlist;
pub mod skills;
pub mod skills_tab;
pub(crate) mod triggers;

pub use allowlist::{ALLOWLIST, EXCLUDED, filter_stats, is_allowed};
pub use calculate::{CalcError, CalculateOptions, CalculateResult, Timings, calculate, calculate_file, calculate_with};
pub use defence::{armour_reduction, armour_reduction_f, defence, hit_chance};
pub use estimations::estimations;
pub use output::{FullDpsSkill, OutTable, OutValue, Output, StatValue, output_to_stats};
pub use perform::{
    ActorState, Checkpoint, CheckpointHook, PerformState, PhaseFn, Who, finish_output, noop_phase, perform, perform_skip_ehp, perform_with,
};
pub use session::{
    BuildOutput, CachedSkill, CalcMode, CalcRun, CalcSession, FullDps, FullDpsOutcome, GlobalCache, RunRef, SessionLog, cache_skill_uuid,
};

pub use env::{CalcActor, Env, GrantedSkill, MainSkillStub, RadiusFunc, RadiusJewel, ReqEntry, Slot};
pub use init_env::{InitEnvError, InitEnvOptions, init_env, init_env_seeded, init_env_with, item_options, parse_items};
pub use pob_parser::ParseMode;
pub use skills_tab::{
    GemInstance, SkillSet, SkillsTab, SocketGroup, find_skill_gem, load_skills_tab, process_gem_level, process_socket_group,
};

/// `getStats()._meta` (boot.lua:806-816) of a built env -- pob-ts `metaOf` (`src/calc/output.ts`).
///
/// `alloc_nodes` is `#build.spec.allocNodes`, which on a cluster-jewel build also holds the nodes `BuildClusterJewelGraphs`
/// generates and the expansion sockets it puts back (D16; built in `ParseMode::Full` since R5.5a). `socket_groups` is
/// `#build.skillsTab.socketGroupList` after the skill phase appended the item/tree-granted groups (CalcSetup.lua:1448-1563, R6.3);
/// without the skill phase (`CalculateOptions.skills = false`) it is the XML's own count.
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
        socket_groups: match &env.skills {
            Some(st) => st.tab.socket_group_list().len(),
            None => pob_build::active_skill_groups(&env.build).len(),
        },
        main_socket_group: env.main_socket_group,
        tree_version: env.spec.tree_version.clone(),
        class_name: env.spec.cur_class_name.clone(),
        ascend_name: env.spec.cur_ascend_class_name.clone(),
    }
}
