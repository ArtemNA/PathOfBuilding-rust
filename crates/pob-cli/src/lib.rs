//! pob-cli library: the `calc` entry that produces the oracle candidate output.
//!
//! R3.6: XML -> `BuildModel` -> `init_env` (CalcSetup.lua without the skill half) -> `meta`. `stats` stay empty until
//! perform/defence (R4); `unimplemented` lists every upstream branch the env skipped plus the output stage itself.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use pob_calc::{InitEnvOptions, init_env, meta_of};
use pob_golden::{CandidateOutput, PartialMeta};

/// Upstream stage reported as skipped while perform/defence are not ported (RUST-ARCH §5).
pub const ENGINE_UNIMPLEMENTED: &str = "Modules/Calcs.lua:384 calcs.buildOutput: perform/defence not ported yet (R4), stats empty";

/// Computes the build output: `meta` from a fresh env, no `stats` yet. A Lua error upstream would raise (L18) is a panic
/// in the port; it is caught here and reported as an error (exit 2), never as a half-filled output.
pub fn calc(xml: &str) -> Result<CandidateOutput, String> {
    if xml.is_empty() {
        return Err("empty XML".into());
    }
    let model = pob_build::load_build_xml(xml).map_err(|e| format!("XML: {e:?}"))?;
    let env = catch_unwind(AssertUnwindSafe(|| init_env(Rc::new(model), InitEnvOptions::default())))
        .map_err(|p| {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".into());
            format!("initEnv: {msg}")
        })?
        .map_err(|e| format!("initEnv: {e}"))?;
    let m = meta_of(&env);
    let as_u32 = |n: usize| u32::try_from(n).expect("count fits u32");
    let meta = PartialMeta {
        alloc_nodes: Some(as_u32(m.alloc_nodes)),
        items: Some(as_u32(m.items)),
        socket_groups: Some(as_u32(m.socket_groups)),
        // A clamped `min(max(#groups, 1), mainSocketGroup)`: an integer whenever the XML's is.
        main_socket_group: (m.main_socket_group.fract() == 0.0).then_some(m.main_socket_group as i64),
        tree_version: Some(m.tree_version),
        class_name: Some(m.class_name),
        ascend_name: Some(m.ascend_name),
    };
    let mut unimplemented = env.spec.unimplemented.clone();
    unimplemented.extend(env.unimplemented.iter().cloned());
    unimplemented.push(ENGINE_UNIMPLEMENTED.into());
    Ok(CandidateOutput {
        stats: Default::default(),
        meta: Some(meta),
        unimplemented,
    })
}
