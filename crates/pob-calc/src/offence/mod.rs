//! `calcs.offence(env, actor, activeSkill)` -- Modules/CalcOffence.lua:348-6263 (one function) and its file-level helpers :70-347
//! (R7-ARCH §1 p. 2: ported section by section; line numbers `grep -n` on upstream `ed354c2f8`, 2026-10-31).
//!
//! R7.3 (part A): :348-1936 -- the setup (skill data, stat conversions, limits, Battlemage/Spellblade, repeats, seals, random phys,
//! momentum, the skill functions `initialFunc`/`preSkillTypeFunc`), the skill type stats (chains, projectiles, melee range, area, aura,
//! reservation, curse/warcry/link/herald effect, trap/mine/totem/brand, corpse), durations, uptime, costs, Eldritch Battery, Sacrificial
//! Zeal, `preDamageFunc`, explosions and the damage-disabling flags. Everything after :1936 (conversion table, passes, damage, crits,
//! ailments, DoT, combined DPS -- R7.4-R7.7) is recorded as `unimplemented` by [`offence`].
//!
//! The checkpoints of the R7.1 probe (`tools/lua-corpus/export-offence.ts`, line hook): OC1 = before :1057, OC2 = before :1987 (the
//! `FNEW` of `buildConversionTable` is attributed to its `end` line, so nothing between :1937 and :1986 runs before it) -- the hook
//! [`Checkpoint::Oc1`]/[`Checkpoint::Oc2`].
//!
//! Upstream shapes kept:
//! * `actor.breakdown` exists iff `env.buildBreakdown` (CalcPerform.lua:1514-1519). Branches under `if breakdown` that write anything
//!   other than breakdown tables are ported (MINEFIELD-RUST R8: :1104-1109 `SplitCountString`, the skill functions'
//!   `NormalHitsPerCast`); pure breakdown tables are R8.
//! * `activeSkill.skillCfg` is read through a [`ModCfg`] snapshot that is rebuilt after the only `skillCond` writes of part A (:518-525).
//! * Hash-order walks (R13/D13): `pairs` over string keys in part A are `pairs({ ["Weapon 1"] = ..., ["Weapon 2"] = ... })` (Energy
//!   Blade, not ported) and `baseRadiiOccurrences` (integer keys, an integer sum: order-free).

mod a1;
mod a2;
mod gates;
pub(crate) mod helpers;

use indexmap::IndexMap;
use pob_mods::{ModCfg, StoreRef, Value};

use crate::active_skill::ActiveSkill;
use crate::env::Env;
use crate::output::Output;
use crate::perform::{ActorState, Checkpoint, PerformState, skill_cfg_of, skill_list_of};
pub(crate) use gates::{Gate, mirages_gate, triggers_gate};
pub(crate) use helpers::{Q, calc_skill_duration, calc_totem_life};

/// The checkpoint hook as offence receives it.
pub(crate) type Hook<'h, 'a> = Option<&'h mut (dyn FnMut(Checkpoint, &Env, &ActorState, &ActorState) + 'a)>;

/// The state of one `calcs.offence(env, env.player, env.player.mainSkill)` call.
pub(crate) struct Off<'e> {
    pub env: &'e mut Env,
    pub st: &'e mut PerformState,
    /// `env.player.mainSkill` (index into `activeSkillList`).
    pub i: usize,
    /// `activeSkill.skillModList`.
    pub q: Q,
    /// `activeSkill.skillCfg`.
    pub cfg: ModCfg,
    /// `actor.modDB` / `actor.enemy.modDB`.
    pub mod_db: StoreRef,
    pub enemy_db: StoreRef,
    /// `actor.breakdown` is set.
    pub breakdown: bool,
    /// `output.AreaOfEffectRadiusTertiaryOccurrences` (a table value in upstream's output: radius -> count, :445), insertion order.
    pub radius_tertiary_occurrences: Option<IndexMap<i64, f64>>,
}

impl Off<'_> {
    pub fn sk(&self) -> &ActiveSkill {
        &self.env.skills.as_ref().expect("skills on").active_skill_list[self.i]
    }

    pub fn sk_mut(&mut self) -> &mut ActiveSkill {
        &mut self.env.skills.as_mut().expect("skills on").active_skill_list[self.i]
    }

    pub fn out(&mut self) -> &mut Output {
        &mut self.st.player.output
    }

    /// `skillData[k]`.
    pub fn sd(&self, k: &str) -> Value {
        self.sk().skill_data.get(k).cloned().unwrap_or(Value::Nil)
    }

    pub fn sd_truthy(&self, k: &str) -> bool {
        self.sk().skill_data.get(k).is_some_and(Value::truthy)
    }

    /// `skillData[k]` where upstream does arithmetic on it (nil/false -> `None`; any other non-number is the Lua error).
    pub fn sd_num(&self, k: &str, line: u32) -> Option<f64> {
        match self.sk().skill_data.get(k) {
            Some(v) if v.truthy() => Some(v.as_num().unwrap_or_else(|| {
                panic!(
                    "Modules/CalcOffence.lua:{line}: attempt to perform arithmetic on field '{k}' (a {} value)",
                    v.type_name()
                )
            })),
            _ => None,
        }
    }

    /// `skillData[k] = v` (nil removes the key).
    pub fn set_sd(&mut self, k: &str, v: Value) {
        let sd = &mut self.sk_mut().skill_data;
        if v.is_nil() {
            sd.shift_remove(k);
        } else {
            sd.insert(k.to_owned(), v);
        }
    }

    /// `skillFlags[k]` truthy.
    pub fn flag(&self, k: &str) -> bool {
        self.sk().flag(k)
    }

    /// `skillFlags[k] = v`.
    pub fn set_flag(&mut self, k: &str, v: bool) {
        self.sk_mut().skill_flags.insert(k.to_owned(), v);
    }

    /// `activeSkill.skillTypes[SkillType[name]]`.
    pub fn has_type(&self, name: &str) -> bool {
        self.sk().skill_types.contains(&skill_type_u32(name))
    }

    /// `activeSkill.weapon1Cfg` / `weapon2Cfg` as a ModStore cfg (`skillCond` reads through to the skill cfg's).
    pub fn weapon_cfg(&self, n: u8) -> Option<ModCfg> {
        let st = self.env.skills.as_ref().expect("skills on");
        let sk = &st.active_skill_list[self.i];
        let w = if n == 1 { sk.weapon1_cfg.as_ref() } else { sk.weapon2_cfg.as_ref() }?;
        let parent = sk.skill_cfg.as_ref().map(|c| &c.skill_cond);
        Some(w.mod_cfg(&st.skills, &sk.skill_types, parent))
    }

    /// Rebuild [`Off::cfg`] after a `skillCfg` write.
    pub fn refresh_cfg(&mut self) {
        let st = self.env.skills.as_ref().expect("skills on");
        self.cfg = skill_cfg_of(st, &st.active_skill_list[self.i]);
    }

    /// `output[k]` as a Lua value (nil when absent).
    pub fn out_val(&self, k: &str) -> Value {
        match self.st.player.output.get(k) {
            Some(crate::output::OutValue::Num(v)) => Value::Num(*v),
            Some(crate::output::OutValue::Bool(b)) => Value::Bool(*b),
            Some(crate::output::OutValue::Str(s)) => Value::Str(s.clone()),
            None => Value::Nil,
        }
    }

    /// `output[k]` used in arithmetic (nil is the Lua error).
    pub fn out_num(&self, k: &str, line: u32) -> f64 {
        self.st.player.output.opt_num(k).unwrap_or_else(|| {
            panic!(
                "Modules/CalcOffence.lua:{line}: attempt to perform arithmetic on field '{k}' (a {:?} value)",
                self.st.player.output.get(k)
            )
        })
    }

    /// `output[k] = v` for a Lua value (nil removes the key).
    pub fn set_out(&mut self, k: &str, v: Value) {
        let o = self.out();
        match v {
            Value::Nil => o.remove(k),
            Value::Num(x) => o.set_num(k, x),
            Value::Bool(b) => o.set_bool(k, b),
            Value::Str(s) => o.set_str(k, &s),
            other => panic!("output.{k} = a {} value (not kept, R8)", other.type_name()),
        }
    }

    pub fn note(&mut self, what: String) {
        if !self.env.unimplemented.contains(&what) {
            self.env.unimplemented.push(what);
        }
    }
}

/// `SkillType[name]`.
pub(crate) fn skill_type_u32(name: &str) -> u32 {
    *pob_data::globals::globals()
        .skill_type
        .get(name)
        .unwrap_or_else(|| panic!("SkillType.{name} is nil")) as u32
}

/// `calcs.offence(env, env.player, env.player.mainSkill)` (CalcPerform.lua:3799) -- part A (:348-1936, R7.3); the rest is recorded as
/// unimplemented. `hook` sees [`Checkpoint::Oc1`] / [`Checkpoint::Oc2`].
pub(crate) fn offence(env: &mut Env, st: &mut PerformState, mut hook: Hook<'_, '_>) {
    let i = env
        .skills
        .as_ref()
        .and_then(|s| s.main_skill)
        .unwrap_or_else(|| panic!("Modules/CalcOffence.lua:358: attempt to index local 'activeSkill' (a nil value)"));
    let (q, cfg) = {
        let s = env.skills.as_ref().expect("skills on");
        let sk = &s.active_skill_list[i];
        (
            Q {
                list: skill_list_of(env, i),
                malformed: sk.malformed_mods.clone(),
            },
            skill_cfg_of(s, sk),
        )
    };
    let mod_db = env.mod_db.clone();
    let enemy_db = env.enemy_db.clone();
    let breakdown = env.build_breakdown;
    let mut off = Off {
        env,
        st,
        i,
        q,
        cfg,
        mod_db,
        enemy_db,
        breakdown,
        radius_tertiary_occurrences: None,
    };
    if let Some(h) = hook.as_mut() {
        h(Checkpoint::OcEntry, off.env, &off.st.player, &off.st.enemy);
    }
    // :348-1055
    if !a1::setup(&mut off) {
        return;
    }
    if let Some(h) = hook.as_mut() {
        h(Checkpoint::Oc1, off.env, &off.st.player, &off.st.enemy);
    }
    // :1056-1936
    a2::skill_type_stats(&mut off);
    if let Some(h) = hook.as_mut() {
        h(Checkpoint::Oc2, off.env, &off.st.player, &off.st.enemy);
    }
    off.note("CalcOffence.lua:1937-6263 conversion table, damage passes, speed/accuracy, damage, crits, leech, ailments, DoT, combined DPS (R7.4-R7.7)".into());
}

/// `runSkillFunc(name)` (:509-514): `activeSkill.activeEffect.grantedEffect[name](activeSkill, output, breakdown)`.
pub(crate) fn run_skill_func(off: &mut Off<'_>, name: &str) {
    let (id, has) = {
        let st = off.env.skills.as_ref().expect("skills on");
        let ge = &st.skills.effects[off.sk().active_effect.borrow().granted_effect];
        (ge.id.clone(), ge.hooks.iter().any(|h| h == name))
    };
    if !has {
        return;
    }
    crate::skill_funcs::call(off, &id, name);
}
