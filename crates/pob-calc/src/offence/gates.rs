//! What CalcPerform.lua:3798 does before `calcs.offence` can run: `calcs.mirages(env)` (CalcMirages.lua:60-427) is R7.9; R7.3 ports only
//! its DISPATCH -- which branch upstream takes for `env.player.mainSkill` (`calcs.triggers`, :3797, is ported since R7.8: [`crate::triggers`]):
//! * mirages: the five branches of :63-426 are told apart by flags/names; none taken -> `nil` (offence runs); one taken -> R7.9.
//!
//! An unported branch stops the run at :3797/:3798 (perform records it and does not call offence): offence on a state the trigger /
//! mirage code would have changed first is not upstream's offence.

use crate::env::Env;
use crate::session::cache_skill_uuid;

/// What the dispatch found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Nothing more of this module runs; offence follows.
    Pass,
    /// The branch upstream takes is not ported -- `"<File.lua>:<line> <what>"`.
    Unported(String),
}

/// `calcs.mirages(env)` (CalcMirages.lua:60-427): `Pass` when it returns nil without doing anything (no branch, or the General's Cry
/// recursion guard :352-355).
pub(crate) fn mirages_gate(env: &Env) -> Gate {
    let Some(i) = env.skills.as_ref().and_then(|s| s.main_skill) else {
        return Gate::Pass;
    };
    let st = env.skills.as_ref().expect("skills on");
    let sk = &st.active_skill_list[i];
    // :63-65
    let used_by_mirage = sk
        .skill_cfg
        .as_ref()
        .is_some_and(|c| c.skill_cond.get("usedByMirage").copied().unwrap_or(false));
    if used_by_mirage || sk.flag("disable") {
        return Gate::Pass;
    }
    let t = |k: &str| sk.skill_data.get(k).is_some_and(pob_mods::Value::truthy);
    let name = st.skills.effects[sk.active_effect.borrow().granted_effect].name.as_str();
    let branch = if t("triggeredByMirageArcher") {
        Some("67-119 Mirage Archer")
    } else if name == "Reflection" {
        Some("120-164 Reflection (The Saviour)")
    } else if name == "Tawhoa's Chosen" {
        Some("165-283 Tawhoa's Chosen")
    } else if t("triggeredBySacredWisps") {
        Some("284-345 Sacred Wisps")
    } else if t("triggeredByGeneralsCry") {
        // :350-355 the recursion guard returns nil before touching anything
        if env.limited_skills.as_ref().is_some_and(|l| l.contains(&cache_skill_uuid(env, i))) {
            return Gate::Pass;
        }
        Some("346-426 General's Cry")
    } else {
        None
    };
    match branch {
        Some(b) => Gate::Unported(format!("CalcMirages.lua:{b} for {name} (R7.9)")),
        None => Gate::Pass,
    }
}
