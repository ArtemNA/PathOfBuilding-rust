//! `calcs.triggers(env, actor)` -- Modules/CalcTriggers.lua (1 627 lines; `grep -n` on upstream `ed354c2f8`, 2026-10-31): the file-level helpers
//! (`addTriggerIncMoreMods` :24, `slotMatch` :33, `isTriggered` :41, `processAddedCastTime` :45, `packageSkillDataForSimulation` :65,
//! `defaultComparer` :69, `findTriggerSkill` :75, `calcMultiSpellRotationImpact` :91), the handlers ([`handler`]: `helmetFocusHandler` :136,
//! `CWCHandler` :222, `defaultTriggerHandler` :394-1584), the 82 entries of `configTable` ([`config`], :902-1584) and the dispatch
//! (`getUniqueItemTriggerName` :1585, `calcs.triggers` :1602-1627). Called at CalcPerform.lua:3797, after defence and before
//! `calcs.mirages` / `calcs.offence`.
//!
//! Upstream shapes kept:
//! * `findTriggerSkill` builds the candidate source with `calcs.buildActiveSkill` (a nested initEnv + perform, cached by
//!   [`crate::session::CalcSession`]); MAIN reads the cache, CALCULATOR always rebuilds. A cached run that did not reach `calcs.offence`
//!   (its own triggers / mirages branch is unported) has none of the offence values a trigger reads -- that is a boundary
//!   (`Err(note)`), never a nil read as a value.
//! * Breakdown tables (`if breakdown then ... end`) are R8 and not built; the branches whose only effect in them is a Lua error on
//!   nil (`s_format("%.2f", nil)` / arithmetic on nil in MAIN) raise the same error ([`handler`]).
//! * `actor.mainSkill.triggeredBy` is the shared support effect table; the fields `configTable` entries write on it besides
//!   `grantedEffect` (`mainSkill`, `activationFreqInc`/`More`, `attachedBrandCount`, `ignoresTickRate`) are read only by the same
//!   `calcs.triggers` call, so they live in [`TriggeredByExtra`] next to the config.
//! * Minion actors (R7.10b): `calcs.triggers(env, env.minion)` is [`triggers_of`] with `Who::Minion`; skills are addressed by index, the
//!   minion's by [`crate::skills::minion_skill_id`].
//! * Hash order: `configTable` is only ever indexed by key (no `pairs`); the cache is indexed by uuid. No `table.sort` in this file.

mod config;
mod handler;

use pob_data::skills::GrantedEffect;
use pob_lua::number::{max2, round};
use pob_mods::{ModCfg, ModType, Value};

use crate::active_skill::ActiveSkill;
use crate::env::Env;
use crate::offence::Q;
use crate::perform::{PerformState, Who, skill_cfg_of, skill_list_of};
use crate::session::{CachedSkill, CalcMode, CalcSession, cache_skill_uuid, is_limited};
use crate::setup::misc;

/// A `configTable` condition: `function(env, skill) ... end` over `env.player.activeSkillList[skill + 1]`.
pub(crate) type SkillCond = Box<dyn Fn(&Env, usize) -> bool>;

/// `config.comparer(env, uuid, source, triggerRate)` -- reads `GlobalCache.cachedData[env.mode][uuid]` (given), `source ~= nil`, `triggerRate`.
pub(crate) type Comparer = Box<dyn Fn(&CachedSkill, bool, Option<f64>) -> bool>;

/// The fields `configTable` entries write on `actor.mainSkill.triggeredBy` besides `grantedEffect` (see the module doc).
#[derive(Clone, Debug, Default)]
pub(crate) struct TriggeredByExtra {
    /// `triggeredBy.mainSkill` (Arcanist Brand :1330, Intuitive Link :1491).
    pub main_skill: Option<usize>,
    pub activation_freq_inc: Option<f64>,
    pub activation_freq_more: Option<f64>,
    pub attached_brand_count: Value,
    /// `triggeredBy.ignoresTickRate` (:1340).
    pub ignores_tick_rate: bool,
}

/// One entry of a rotation (`packageSkillDataForSimulation` :65-67 / the literal tables of :314, :1176) and the simulation's own fields.
#[derive(Clone, Debug, Default)]
pub(crate) struct SimSkill {
    pub uuid: String,
    pub cd: Option<f64>,
    pub cd_override: Option<f64>,
    pub adds_cast_time: Option<f64>,
    pub icdr: Option<f64>,
    pub added_cooldown: Option<f64>,
    /// `skill.cd` after :102 (the effective cooldown), `next_trig`, `count`.
    cd_eff: f64,
    next_trig: f64,
    count: f64,
}

// --- access to `env.player.activeSkillList` --------------------------------------------------------------------------------------

pub(super) fn skill(env: &Env, i: usize) -> &ActiveSkill {
    env.skills.as_ref().expect("skills on").skill(i)
}

pub(super) fn skill_mut(env: &mut Env, i: usize) -> &mut ActiveSkill {
    env.skills.as_mut().expect("skills on").skill_mut(i)
}

/// `env.player.mainSkill`.
pub(super) fn main_idx(env: &Env) -> usize {
    env.skills
        .as_ref()
        .and_then(|s| s.main_skill)
        .unwrap_or_else(|| panic!("Modules/CalcTriggers.lua: attempt to index field 'mainSkill' (a nil value)"))
}

pub(super) fn skill_count(env: &Env) -> usize {
    env.skills.as_ref().map_or(0, |s| s.active_skill_list.len())
}

/// `skill.activeEffect.grantedEffect`.
pub(super) fn ge_of(env: &Env, i: usize) -> &GrantedEffect {
    let st = env.skills.as_ref().expect("skills on");
    &st.skills.effects[st.skill(i).active_effect.borrow().granted_effect]
}

pub(super) fn name_of(env: &Env, i: usize) -> String {
    ge_of(env, i).name.clone()
}

/// `grantedEffect.fromItem` including CalcSetup's data write (CalcSetup.lua:1633-1634).
pub(super) fn from_item(env: &Env, ge: &GrantedEffect) -> bool {
    ge.from_item || env.skills.as_ref().is_some_and(|s| s.from_item_marks.contains(&ge.id))
}

/// `skill.skillTypes[SkillType[name]]`.
pub(super) fn has_type(env: &Env, i: usize, name: &str) -> bool {
    skill(env, i).skill_types.contains(&crate::offence::skill_type_u32(name))
}

/// `skill.skillData[k]`.
pub(super) fn sd(env: &Env, i: usize, k: &str) -> Value {
    skill(env, i).skill_data.get(k).cloned().unwrap_or(Value::Nil)
}

pub(super) fn sd_truthy(env: &Env, i: usize, k: &str) -> bool {
    skill(env, i).skill_data.get(k).is_some_and(Value::truthy)
}

/// `skill.skillData[k]` where upstream does arithmetic on it (nil/false -> `None`; any other non-number is the Lua error).
pub(super) fn sd_num(env: &Env, i: usize, k: &str, line: u32) -> Option<f64> {
    match skill(env, i).skill_data.get(k) {
        Some(v) if v.truthy() => Some(v.as_num().unwrap_or_else(|| {
            panic!(
                "Modules/CalcTriggers.lua:{line}: attempt to perform arithmetic on field '{k}' (a {} value)",
                v.type_name()
            )
        })),
        _ => None,
    }
}

/// `skill.skillData[k] = v` (nil removes the key).
pub(super) fn set_sd(env: &mut Env, i: usize, k: &str, v: Value) {
    let d = &mut skill_mut(env, i).skill_data;
    if v.is_nil() {
        d.shift_remove(k);
    } else {
        d.insert(k.to_owned(), v);
    }
}

pub(super) fn flag(env: &Env, i: usize, k: &str) -> bool {
    skill(env, i).flag(k)
}

pub(super) fn set_flag(env: &mut Env, i: usize, k: &str) {
    skill_mut(env, i).skill_flags.insert(k.to_owned(), true);
}

/// `skill.skillModList` behind the malformed-mod guard.
pub(super) fn q_of(env: &Env, i: usize) -> Q {
    Q {
        list: skill_list_of(env, i),
        malformed: skill(env, i).malformed_mods.clone(),
    }
}

/// `skill.skillCfg`.
pub(super) fn cfg_of(env: &Env, i: usize) -> ModCfg {
    let st = env.skills.as_ref().expect("skills on");
    skill_cfg_of(st, st.skill(i))
}

/// `skill.skillCfg.flags`.
pub(super) fn cfg_flags(env: &Env, i: usize) -> u32 {
    skill(env, i).skill_cfg.as_ref().map_or_else(
        || panic!("Modules/CalcTriggers.lua: attempt to index field 'skillCfg' (a nil value)"),
        |c| c.flags as u32,
    )
}

/// `env.player.modDB` as a query.
pub(super) fn player_db(env: &Env) -> Q {
    Q {
        list: env.mod_db.clone(),
        malformed: Vec::new(),
    }
}

/// `skill.socketGroup.slot` -- indexing a nil `socketGroup` is the Lua error.
pub(super) fn group_slot(env: &Env, i: usize, line: u32) -> Option<String> {
    let st = env.skills.as_ref().expect("skills on");
    let g = st
        .skill(i)
        .socket_group
        .unwrap_or_else(|| panic!("Modules/CalcTriggers.lua:{line}: attempt to index field 'socketGroup' (a nil value)"));
    st.tab.socket_group_list()[g].slot.clone()
}

/// `skill.activeEffect.srcInstance.<field>`.
pub(super) fn src_instance<R>(env: &Env, i: usize, f: impl FnOnce(&crate::skills_tab::GemInstance) -> R) -> Option<R> {
    let st = env.skills.as_ref().expect("skills on");
    let (g, k) = st.skill(i).active_effect.borrow().src_instance?;
    Some(f(&st.tab.socket_group_list()[g].gem_list[k]))
}

/// `skill.triggeredBy.grantedEffect.levels[skill.triggeredBy.level].cooldown` (`None` without `triggeredBy`; a missing level is the
/// Lua error).
pub(super) fn triggered_by_cd(env: &Env, i: usize, line: u32) -> Option<f64> {
    let st = env.skills.as_ref().expect("skills on");
    let tb = st.skill(i).triggered_by.as_ref()?;
    let tb = tb.borrow();
    let ge = &st.skills.effects[tb.granted_effect];
    let lvl = tb.level.and_then(|l| ge.level(l)).unwrap_or_else(|| {
        panic!(
            "Modules/CalcTriggers.lua:{line}: attempt to index a nil value (levels[triggeredBy.level] of {})",
            ge.name
        )
    });
    lvl.cooldown
}

/// `skill.triggeredBy.grantedEffect` (index).
pub(super) fn triggered_by_ge(env: &Env, i: usize) -> Option<usize> {
    skill(env, i).triggered_by.as_ref().map(|t| t.borrow().granted_effect)
}

pub(super) fn line_err(line: u32, what: &str) -> ! {
    panic!("Modules/CalcTriggers.lua:{line}: {what}")
}

// --- :24-133 the file-level helpers ---------------------------------------------------------------------------------------------

/// :24-31 `addTriggerIncMoreMods(activeSkill, sourceSkill)`.
pub(super) fn add_trigger_inc_more_mods(env: &Env, active: usize, source: usize) {
    let q = q_of(env, active);
    let scfg = cfg_of(env, source);
    for ty in [ModType::Inc, ModType::More] {
        for e in q.tabulate(Some(ty), Some(&scfg), &["TriggeredDamage"]) {
            let m = &e.mod_;
            // `unpack(value.mod)`: the same tag tables
            q.new_mod(
                "Damage",
                Some(ty),
                m.value.clone(),
                m.source.as_deref(),
                m.flags,
                m.keyword_flags,
                m.tags.clone(),
            );
        }
    }
}

/// :33-39 `slotMatch(env, skill)`.
pub(super) fn slot_match(env: &Env, i: usize) -> bool {
    let m = main_idx(env);
    let st = env.skills.as_ref().expect("skills on");
    let main_from_item = from_item(env, ge_of(env, m));
    let src_from_item = |k: usize| src_instance(env, k, |g| g.from_item == Some(true)).unwrap_or(false);
    let fi = main_from_item || from_item(env, ge_of(env, i)) || src_from_item(m) || src_from_item(i);
    let sk = st.skill(i);
    let main = st.skill(m);
    let match1 = fi
        && sk.socket_group.is_some()
        && group_slot(env, i, 36) == {
            let g = main
                .socket_group
                .unwrap_or_else(|| line_err(36, "attempt to index field 'socketGroup' (a nil value)"));
            st.tab.socket_group_list()[g].slot.clone()
        };
    let match2 = !main_from_item && sk.socket_group == main.socket_group;
    match1 || match2
}

/// :41-43 `isTriggered(skill)` (truthiness).
pub(crate) fn is_triggered(env: &Env, i: usize) -> bool {
    let sk = skill(env, i);
    let t = |k: &str| sk.skill_data.get(k).is_some_and(Value::truthy);
    // `grantedEffect.triggered`: no granted effect of the data sets it
    let src_triggered = src_instance(env, i, |g| g.triggered).unwrap_or(false);
    t("triggeredByUnique") || t("triggered") || has_type(env, i, "InbuiltTrigger") || has_type(env, i, "Triggered") || src_triggered
}

/// :45-63 `processAddedCastTime(skill, breakdown)` -> `(addsCastTime, csi)`; sets `skill.skillFlags.addsCastTime` (breakdown: R8).
pub(super) fn process_added_cast_time(env: &mut Env, i: usize) -> Option<(f64, f64)> {
    let q = q_of(env, i);
    let cfg = cfg_of(env, i);
    if !q.flag(Some(&cfg), &["SpellCastTimeAddedToCooldownIfTriggered"]) {
        return None;
    }
    // :47 `skillData.castTimeOverride or grantedEffect.castTime or 1`
    let base = sd_num(env, i, "castTimeOverride", 47).or(ge_of(env, i).cast_time).unwrap_or(1.0);
    let inc = q.inc(Some(&cfg), &["Speed"]);
    let more = q.more(Some(&cfg), &["Speed"]);
    let csi = round((1.0 + inc / 100.0) * more, Some(2.0));
    let adds = base / csi;
    set_flag(env, i, "addsCastTime");
    Some((adds, csi))
}

/// :65-67 `packageSkillDataForSimulation(skill, env)`.
pub(super) fn package(env: &mut Env, i: usize) -> SimSkill {
    let uuid = cache_skill_uuid(env, i);
    let cd = sd_num(env, i, "cooldown", 102);
    let q = q_of(env, i);
    let cfg = cfg_of(env, i);
    let cd_override = q.override_num(Some(&cfg), &["CooldownRecovery"], "Modules/CalcTriggers.lua:102");
    let adds_cast_time = process_added_cast_time(env, i).map(|(a, _)| a);
    let icdr = q.calc_mod(Some(&cfg), &["CooldownRecovery"]);
    let added_cooldown = q.base(Some(&cfg), &["CooldownRecovery"]);
    SimSkill {
        uuid,
        cd,
        cd_override,
        adds_cast_time,
        icdr: Some(icdr),
        added_cooldown: Some(added_cooldown),
        ..SimSkill::default()
    }
}

/// :69-72 `defaultComparer(env, uuid, source, triggerRate)`.
pub(super) fn default_comparer(c: &CachedSkill, source: bool, trigger_rate: Option<f64>) -> bool {
    match c.hit_speed.or(c.speed) {
        Some(s) => !source || s > trigger_rate.unwrap_or(0.0),
        None => false,
    }
}

/// `CalcSession::of(env)` -- the triggers read other skills' runs through it.
pub(super) fn session(env: &Env, line: u32) -> Result<CalcSession, String> {
    CalcSession::of(env).ok_or_else(|| format!("CalcTriggers.lua:{line} calcs.buildActiveSkill outside a CalcSession"))
}

/// `calcs.buildActiveSkill(env, env.mode, skill, uuid, limited)` unless `GlobalCache.cachedData[env.mode][uuid]` exists (MAIN) -- with
/// `always` (`or env.mode == "CALCULATOR"`) a CALCULATOR env always rebuilds.
pub(super) fn ensure_cached(env: &Env, i: usize, uuid: &str, limited: &[String], always: bool, line: u32) -> Result<(), String> {
    let sess = session(env, line)?;
    let mode = CalcMode::of(env);
    let cached = sess.cache().contains(mode, uuid);
    if !cached || (always && mode == CalcMode::Calculator) {
        sess.build_active_skill(env, mode, i, Some(uuid), limited)
            .map_err(|e| format!("CalcTriggers.lua:{line} calcs.buildActiveSkill: {}", e.0))?;
    }
    Ok(())
}

/// Reads `GlobalCache.cachedData[env.mode][uuid]` (`None` = nil). A cached run without `calcs.offence` is a boundary.
pub(super) fn with_cached<R>(env: &Env, uuid: Option<&str>, line: u32, f: impl FnOnce(&CachedSkill) -> R) -> Result<Option<R>, String> {
    let Some(uuid) = uuid else { return Ok(None) };
    let sess = session(env, line)?;
    let cache = sess.cache();
    let Some(c) = cache.get(CalcMode::of(env), uuid) else {
        return Ok(None);
    };
    if !c.run.borrow().offence_ran {
        return Err(format!(
            "CalcTriggers.lua:{line} GlobalCache.cachedData[{}][{uuid}]: the cached run has no calcs.offence (its triggers/mirages branch is unported)",
            CalcMode::of(env).as_str()
        ));
    }
    Ok(Some(f(c)))
}

/// `GlobalCache.cachedData[env.mode][uuid].Env.player.output[k]` (nil -> `None`).
pub(super) fn cached_out(c: &CachedSkill, k: &str) -> Option<f64> {
    c.run.borrow().state.player.output.opt_num(k)
}

/// `GlobalCache.cachedData[env.mode][uuid].Env.player.output[k]` truthy (a number, 0 included, or `true`).
pub(super) fn cached_out_truthy(c: &CachedSkill, k: &str) -> bool {
    match c.run.borrow().state.player.output.get(k) {
        Some(crate::output::OutValue::Bool(b)) => *b,
        Some(_) => true,
        None => false,
    }
}

/// `GlobalCache.cachedData[env.mode][uuid].ActiveSkill.skillData[k]` (= `.Env.player.mainSkill.skillData[k]`).
pub(super) fn cached_sd(c: &CachedSkill, k: &str) -> Value {
    let r = c.run.borrow();
    r.env.skills.as_ref().expect("skills on").active_skill_list[c.active_skill]
        .skill_data
        .get(k)
        .cloned()
        .unwrap_or(Value::Nil)
}

/// What `findTriggerSkill` returns: `source, triggerRate, uuid`.
#[derive(Clone, Debug, Default)]
pub(super) struct Found {
    pub source: Option<usize>,
    pub rate: Option<f64>,
    pub uuid: Option<String>,
}

/// :75-87 `findTriggerSkill(env, skill, source, triggerRate, comparer)`.
pub(super) fn find_trigger_skill(env: &Env, i: usize, cur: &Found, comparer: Option<&Comparer>) -> Result<Found, String> {
    let uuid = cache_skill_uuid(env, i);
    // :79-81
    ensure_cached(env, i, &uuid, &[], true, 80)?;
    let source_set = cur.source.is_some();
    let hit = with_cached(env, Some(&uuid), 83, |c| {
        let pass = match comparer {
            Some(f) => f(c, source_set, cur.rate),
            None => default_comparer(c, source_set, cur.rate),
        };
        (pass, c.hit_speed.or(c.speed))
    })?;
    if let Some((true, rate)) = hit {
        let sk = skill(env, i);
        let used_by_mirage = sk
            .skill_cfg
            .as_ref()
            .is_some_and(|c| c.skill_cond.get("usedByMirage").copied().unwrap_or(false));
        // `skill.skillFlags and not skill.skillFlags.disable` / `skill.skillCfg and not ...` (both tables always exist)
        if !sk.flag("disable") && sk.skill_cfg.is_some() && !used_by_mirage && !has_type(env, i, "OtherThingUsesSkill") {
            return Ok(Found {
                source: Some(i),
                rate,
                uuid: Some(uuid),
            });
        }
    }
    // :86 `return source, triggerRate, source and cacheSkillUUID(source, env)`
    Ok(Found {
        source: cur.source,
        rate: cur.rate,
        uuid: cur.source.map(|s| cache_skill_uuid(env, s)),
    })
}

/// Common.lua:1013 `ceil_b(x, base)` / :1019 `floor_b(x, base)`.
pub(super) fn ceil_b(x: f64, base: f64) -> f64 {
    base * (x / base).ceil()
}

fn floor_b(x: f64, base: f64) -> f64 {
    base * (x / base).floor()
}

/// :91-134 `calcMultiSpellRotationImpact(env, skillRotation, sourceRate, triggerCD, chance, actor)` -> `(mainRate, rates)`; `main_uuid` =
/// `cacheSkillUUID(actor.mainSkill, env)`.
pub(crate) fn calc_multi_spell_rotation_impact(
    main_uuid: &str,
    rotation: &mut [SimSkill],
    source_rate: f64,
    trigger_cd: Option<f64>,
    chance: Option<f64>,
) -> (f64, Vec<(String, f64)>) {
    let tick = misc("ServerTickTime");
    let mut rotation_index = 1usize;
    let mut next_trigger = 0.0;
    let trigger_increment = 1.0 / source_rate;
    let sim_time = trigger_increment * 1000.0;
    let chance = chance.unwrap_or(100.0);
    let n = rotation.len();
    // :101-105
    for s in rotation.iter_mut() {
        let icdr = s.icdr.unwrap_or(1.0);
        let own = s
            .cd_override
            .unwrap_or_else(|| (s.cd.unwrap_or(0.0) + s.added_cooldown.unwrap_or(0.0)) / icdr);
        s.cd_eff = max2(own, (trigger_cd.unwrap_or(0.0) + s.adds_cast_time.unwrap_or(0.0)) / icdr);
        s.next_trig = 0.0;
        s.count = 0.0;
    }
    // :107-120
    while next_trigger < sim_time {
        let mut current = rotation_index;
        loop {
            let s = rotation
                .get_mut(current - 1)
                .unwrap_or_else(|| line_err(110, "attempt to index field '?' (a nil value)"));
            if s.next_trig <= next_trigger {
                s.count += 1.0;
                s.next_trig = ceil_b(floor_b(next_trigger, tick) + s.cd_eff, tick);
                break;
            }
            current = (current % n) + 1;
            if current == rotation_index {
                break;
            }
        }
        rotation_index = (rotation_index % n.max(1)) + 1;
        next_trigger += trigger_increment;
    }
    // :122-131
    let mut main_rate = 0.0;
    let mut rates = Vec::with_capacity(n);
    for s in rotation.iter() {
        let rate = 1.0 / (sim_time / s.count + (trigger_increment / chance * 100.0) - trigger_increment);
        rates.push((s.uuid.clone(), rate));
        if main_uuid == s.uuid {
            main_rate = rate;
        }
    }
    (main_rate, rates)
}

// --- :1585-1627 the dispatch ----------------------------------------------------------------------------------------------------

/// :1585-1600 `getUniqueItemTriggerName(skill)` (a Lua value: `skillData.triggerSource` may be any type).
fn unique_item_trigger_name(env: &Env, i: usize) -> Option<String> {
    let st = env.skills.as_ref().expect("skills on");
    let sk = st.skill(i);
    if let Some(v) = sk.skill_data.get("triggerSource").filter(|v| v.truthy()) {
        return Some(v.as_str().map_or_else(|| format!("{v:?}"), str::to_owned));
    }
    // :1588-1594 `skill.supportList` -- `gemInstance.grantedEffect.fromItem and not support`
    for se in &sk.support_list {
        let ge = &st.skills.effects[se.borrow().granted_effect];
        if from_item(env, ge) && !ge.support {
            return Some(ge.name.clone());
        }
    }
    // :1596-1599 `socketGroup.source:find(".*:.*:(.*),.*")`
    let src = sk.socket_group.and_then(|g| st.tab.socket_group_list()[g].source.clone())?;
    let (_, _, caps) = pob_lua::find(src.as_bytes(), b".*:.*:(.*),.*", None, false).ok().flatten()?;
    let cap = caps.get(0)?.bytes(src.as_bytes())?;
    Some(String::from_utf8_lossy(cap).into_owned())
}

/// :1602-1627 `calcs.triggers(env, env.player)`. `Err(note)` = a boundary of the port (the run stops before offence, as the R7.3 gate did).
pub(crate) fn triggers(env: &mut Env, st: &mut PerformState) -> Result<(), String> {
    let Some(m) = env.skills.as_ref().and_then(|s| s.main_skill) else {
        return Ok(());
    };
    triggers_of(env, st, Who::Player, m)
}

/// :1602-1627 `calcs.triggers(env, actor)` for `actor` = `who` with `actor.mainSkill` = skill `m` (R7.10b: for the minion `st.player` holds
/// the minion's perform state and the player's is in `st.minion`, see [`crate::offence::Off`]).
pub(crate) fn triggers_of(env: &mut Env, st: &mut PerformState, who: Who, m: usize) -> Result<(), String> {
    // :1603
    if flag(env, m, "disable") || is_limited(env, m) {
        return Ok(());
    }
    // :1604-1612
    let skill_name = name_of(env, m);
    let trigger_name = triggered_by_ge(env, m).map(|g| env.skills.as_ref().expect("skills on").skills.effects[g].name.clone());
    let unique_name = if is_triggered(env, m) {
        unique_item_trigger_name(env, m)
    } else {
        None
    };
    let lower = |s: &str| s.to_ascii_lowercase();
    let trigger_lower = trigger_name.as_deref().map(lower);
    // `triggerNameLower:gsub("^awakened ", "")`
    let awakened = trigger_lower
        .as_deref()
        .map(|t| t.strip_prefix("awakened ").unwrap_or(t).to_owned());
    let skill_id = ge_of(env, m).id.clone();
    let candidates = [
        Some(skill_id),
        Some(lower(&skill_name)),
        trigger_lower,
        awakened,
        unique_name.as_deref().map(lower),
    ];
    // :1613-1617 `config = config or key and configTable[key] and configTable[key](env)` -- a nil return falls through (its side
    // effects stay); the entries write `env.player.output`
    let mut found = None;
    for key in candidates.into_iter().flatten() {
        let c = match who {
            Who::Player => config::config_table(env, &mut st.player.output, &key)?,
            _ => {
                let cell = st.minion.clone().expect("env.minion output");
                let mut p = cell.borrow_mut();
                config::config_table(env, &mut p.output, &key)?
            }
        };
        if let Some(c) = c {
            found = Some(c);
            break;
        }
    }
    match found {
        Some(mut c) => {
            // :1618 `config.actor = config.actor or actor`
            let actor = c.actor.unwrap_or(who);
            if actor != who {
                return Err(format!(
                    "CalcTriggers.lua:1618 config.actor = env.minion for calcs.triggers(env, {who:?}) (the handler would write the other actor's output)"
                ));
            }
            // :1619-1621
            if c.trigger_name.is_none() {
                c.trigger_name = Some(trigger_name.unwrap_or(skill_name));
            }
            if c.trigger_chance.is_none() {
                c.trigger_chance = src_instance(env, m, |g| g.trigger_chance).flatten();
            }
            match c.custom_handler {
                // the custom handlers work on `env.player` (:136-392)
                Some(_) if who != Who::Player => Err(format!(
                    "CalcTriggers.lua:1622 a custom trigger handler for calcs.triggers(env, {who:?}) (the handler writes env.player.output)"
                )),
                Some(config::Custom::Cwc) => handler::cwc_handler(env, st),
                Some(config::Custom::Focus) => handler::helmet_focus_handler(env, st),
                None => handler::default_trigger_handler(env, st, c, actor, m),
            }
        }
        None => {
            // :1624
            set_sd(env, m, "triggered", Value::Nil);
            Ok(())
        }
    }
}

/// `env.build.configTab.input[key] == value` (a string input).
pub(super) fn config_input_is(env: &Env, key: &str, value: &str) -> bool {
    matches!(env.config.input.get(key), Some(pob_build::ConfigValue::Str(s)) if s == value)
}

/// `calcLib.canGrantedEffectSupportActiveSkill(grantedEffect, skill)` (CalcTools.lua:86-165, no imbued / granted-skill flags) for
/// `env.player.activeSkillList[skill + 1]`.
pub(super) fn can_support(env: &Env, ge: usize, i: usize) -> bool {
    let st = env.skills.as_ref().expect("skills on");
    let sk = st.skill(i);
    let ge = &st.skills.effects[ge];
    let act = ge_of(env, i);
    let p = env.player.borrow();
    let w1 = p.weapon_data1.get("type");
    let w2 = p.weapon_data2.get("type");
    let view = crate::calc_tools::ActiveSkillView {
        granted_effect: act,
        has_gem_data: sk.active_effect.borrow().gem_data.is_some(),
        src_instance_from_item: src_instance(env, i, |g| g.from_item == Some(true)).unwrap_or(false),
        skill_types: Some(&sk.skill_types),
        minion_skill_types: sk.minion_skill_types.as_ref(),
        summon: None,
        actor_is_player: true,
        weapon1: crate::calc_tools::WeaponView {
            ty: w1.as_str(),
            counts_as_all_1h: p.weapon_data1.get("countsAsAll1H").truthy(),
        },
        weapon2: crate::calc_tools::WeaponView {
            ty: w2.as_str(),
            counts_as_all_1h: p.weapon_data2.get("countsAsAll1H").truthy(),
        },
    };
    crate::calc_tools::can_granted_effect_support_active_skill_ext(ge, &view, false, false, from_item(env, ge), from_item(env, act))
        .unwrap_or_else(|e| panic!("{e}"))
}

/// `skill.triggeredBy and calcLib.canGrantedEffectSupportActiveSkill(skill.triggeredBy.grantedEffect, skill)` (:1507, :1514).
pub(super) fn triggered_by_can_support(env: &Env, i: usize) -> bool {
    triggered_by_ge(env, i).is_some_and(|g| can_support(env, g, i))
}
