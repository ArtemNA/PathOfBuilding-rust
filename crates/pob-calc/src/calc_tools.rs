//! Modules/CalcTools.lua:42-303 -- the gem helpers of `calcLib` (R6.2): `validateGemLevel`, `doesTypeExpressionMatch`,
//! `canGrantedEffectSupportActiveSkill`, `getGemStatRequirement`, `buildSkillInstanceStats`, `getConvertedModTags`. `gemIsType`,
//! `getGameIdFromGemName` and `isGemIdSame` were ported in R5 to `pob_mods::calc_lib` (ModStore's `SkillType`/gem tags need them) and are
//! re-exported here. Gate: `crates/pob-data/tests/gemdata.rs` against the LuaJIT probe `tools/lua-corpus/export-gemdata.ts`.

use pob_data::skills::{GrantedEffect, SkillTypeSet};
use pob_lua::number::{max2, min2, round};
use pob_mods::{Tag, Value};

pub use pob_mods::calc_lib::{gem_is_type, get_game_id_from_gem_name, is_gem_id_same};

/// A Lua error raised by a ported helper (text as LuaJIT would print it, `file:line` of the upstream source).
pub type LuaError = String;

/// What `validateGemLevel` reads and writes of a gem instance.
pub struct GemLevelTarget<'a> {
    pub granted_effect: &'a GrantedEffect,
    /// `gemInstance.gemData.naturalMaxLevel` (only when the instance has gemData).
    pub natural_max_level: Option<f64>,
}

/// CalcTools.lua:42-58 `calcLib.validateGemLevel(gemInstance)` -- the new `gemInstance.level`. `level` nil reaches `m_max(1, nil)`.
pub fn validate_gem_level(t: &GemLevelTarget<'_>, level: Option<f64>) -> Result<f64, LuaError> {
    let ge = t.granted_effect;
    let has = |l: Option<f64>| l.is_some_and(|l| ge.level(l).is_some());
    let mut level = level;
    if !has(level) {
        // Try limiting to the level range of the skill
        let Some(l) = level else {
            return Err("Modules/CalcTools.lua:46: bad argument #2 to 'max' (number expected, got nil)".into());
        };
        let mut l = max2(1.0, l);
        if ge.levels_len > 0 {
            l = min2(ge.levels_len as f64, l);
        }
        level = Some(l);
    }
    if !has(level)
        && let Some(n) = t.natural_max_level
    {
        level = Some(n);
    }
    if !has(level) {
        // That failed, so just grab any level
        level = Some(ge.levels_next);
    }
    Ok(level.expect("set above"))
}

/// A Lua value of the type-expression stack (`nil`, `false`, `true`).
type LuaBool = Option<bool>;

fn truthy(v: LuaBool) -> bool {
    v == Some(true)
}

/// CalcTools.lua:61-82 `calcLib.doesTypeExpressionMatch(checkTypes, skillTypes, minionTypes)`: postfix over `SkillType` numbers with
/// `OR`/`AND`/`NOT`. `stack[#stack]` of an empty stack is the slot `stack[0]`, which `ipairs` never reads -- kept as `slot0`.
pub fn does_type_expression_match(check: &[f64], skill_types: &SkillTypeSet, minion_types: Option<&SkillTypeSet>) -> bool {
    let st = skill_type_consts();
    let mut stack: Vec<LuaBool> = Vec::new();
    let mut slot0: LuaBool = None;
    // `stack[#stack]` read / write
    fn top(stack: &[LuaBool], slot0: LuaBool) -> LuaBool {
        stack.last().copied().unwrap_or(slot0)
    }
    fn set_top(stack: &mut [LuaBool], slot0: &mut LuaBool, v: LuaBool) {
        match stack.last_mut() {
            Some(t) => *t = v,
            None => *slot0 = v,
        }
    }
    for &t in check {
        if t == st.or {
            let other = stack.pop().flatten();
            let cur = top(&stack, slot0);
            set_top(&mut stack, &mut slot0, if truthy(cur) { cur } else { other });
        } else if t == st.and {
            let other = stack.pop().flatten();
            let cur = top(&stack, slot0);
            set_top(&mut stack, &mut slot0, if truthy(cur) { other } else { cur });
        } else if t == st.not {
            let cur = top(&stack, slot0);
            set_top(&mut stack, &mut slot0, Some(!truthy(cur)));
        } else {
            let k = t as u32;
            let v = f64::from(k) == t && (skill_types.contains(&k) || minion_types.is_some_and(|m| m.contains(&k)));
            stack.push(Some(v));
        }
    }
    // `for _, val in ipairs(stack)`: stops at the first nil
    stack.iter().take_while(|v| v.is_some()).any(|v| truthy(*v))
}

struct SkillTypeConsts {
    or: f64,
    and: f64,
    not: f64,
    skill_granted_by_support: u32,
}

fn skill_type_consts() -> &'static SkillTypeConsts {
    static C: std::sync::OnceLock<SkillTypeConsts> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        let g = &pob_data::globals::globals().skill_type;
        let get = |n: &str| *g.get(n).unwrap_or_else(|| panic!("SkillType.{n} missing"));
        SkillTypeConsts {
            or: get("OR"),
            and: get("AND"),
            not: get("NOT"),
            skill_granted_by_support: get("SkillGrantedBySupport") as u32,
        }
    })
}

/// A weapon of the actor as `canGrantedEffectSupportActiveSkill` reads it (`actor.weaponData1/2`).
#[derive(Clone, Copy, Debug, Default)]
pub struct WeaponView<'a> {
    pub ty: Option<&'a str>,
    pub counts_as_all_1h: bool,
}

/// The summoning skill of a minion skill (`activeSkill.summonSkill`).
#[derive(Clone, Copy, Debug)]
pub struct SummonView<'a> {
    pub granted_effect: &'a GrantedEffect,
    pub skill_types: Option<&'a SkillTypeSet>,
    pub minion_skill_types: Option<&'a SkillTypeSet>,
}

/// What `canGrantedEffectSupportActiveSkill` reads of an active skill.
#[derive(Clone, Copy, Debug)]
pub struct ActiveSkillView<'a> {
    /// `activeSkill.activeEffect.grantedEffect`.
    pub granted_effect: &'a GrantedEffect,
    /// `activeSkill.activeEffect.gemData ~= nil`.
    pub has_gem_data: bool,
    /// `activeSkill.activeEffect.srcInstance and activeSkill.activeEffect.srcInstance.fromItem`.
    pub src_instance_from_item: bool,
    pub skill_types: Option<&'a SkillTypeSet>,
    pub minion_skill_types: Option<&'a SkillTypeSet>,
    pub summon: Option<SummonView<'a>>,
    /// `activeSkill.actor.enemy.player == activeSkill.actor`.
    pub actor_is_player: bool,
    pub weapon1: WeaponView<'a>,
    pub weapon2: WeaponView<'a>,
}

/// CalcTools.lua:86-165 `calcLib.canGrantedEffectSupportActiveSkill(grantedEffect, activeSkill, imbuedSupport, appliesToGrantedSkills)`.
/// `grantedEffect.unsupported` is nil for every effect of the data (pob-data `skills.rs`).
pub fn can_granted_effect_support_active_skill(
    ge: &GrantedEffect,
    active: &ActiveSkillView<'_>,
    imbued_support: bool,
    applies_to_granted_skills: bool,
) -> Result<bool, LuaError> {
    let act = active.granted_effect;
    if act.cannot_be_supported {
        return Ok(false);
    }
    if ge.support_gems_only && !active.has_gem_data {
        return Ok(false);
    }
    if !applies_to_granted_skills
        && ge.from_item
        && ge.support
        && (act.from_item || act.mod_source.as_bytes().starts_with(b"Item") || active.src_instance_from_item)
    {
        return Ok(false);
    }
    // `a and a.x or b` -- a nil `a.x` falls through to `b` (L4)
    let (effective_skill_types, effective_minion_types): (Option<&SkillTypeSet>, Option<&SkillTypeSet>) = if imbued_support {
        let st = active
            .summon
            .and_then(|s| s.granted_effect.skill_types.as_ref())
            .or(act.skill_types.as_ref());
        let mt = active
            .summon
            .and_then(|s| s.granted_effect.minion_skill_types.as_ref())
            .or(act.minion_skill_types.as_ref());
        (st, if ge.ignore_minion_types { None } else { mt })
    } else {
        let st = active.summon.and_then(|s| s.skill_types).or(active.skill_types);
        let mt = active.summon.and_then(|s| s.minion_skill_types).or(active.minion_skill_types);
        (st, if ge.ignore_minion_types { None } else { mt })
    };
    let index_nil =
        |what: &str| format!("Modules/CalcTools.lua (canGrantedEffectSupportActiveSkill): attempt to index a nil value ({what})");
    let Some(exclude) = &ge.exclude_skill_types else {
        return Err(index_nil("excludeSkillTypes"));
    };
    if !exclude.is_empty() {
        let types = effective_skill_types.ok_or_else(|| index_nil("skillTypes"))?;
        if does_type_expression_match(exclude, types, None) {
            return Ok(false);
        }
    }
    if ge.is_trigger && !active.actor_is_player {
        return Ok(false);
    }
    let all_one_hand = active.weapon1.counts_as_all_1h || active.weapon2.counts_as_all_1h;
    if let Some(wanted) = &ge.weapon_types {
        let mut lookup: Vec<&str> = Vec::new();
        if let Some(own) = &act.weapon_types {
            lookup.extend(own.keys().map(String::as_str));
        } else if effective_skill_types
            .ok_or_else(|| index_nil("skillTypes"))?
            .contains(&skill_type_consts().skill_granted_by_support)
        {
            lookup.extend(active.weapon1.ty);
            lookup.extend(active.weapon2.ty);
        }
        if lookup.is_empty() {
            return Ok(false);
        }
        if all_one_hand {
            lookup.extend(["Claw", "Dagger", "One Handed Axe", "One Handed Mace", "One Handed Sword"]);
        }
        if !wanted.keys().any(|w| lookup.contains(&w.as_str())) {
            return Ok(false);
        }
    }
    let Some(require) = &ge.require_skill_types else {
        return Err(index_nil("requireSkillTypes"));
    };
    if require.is_empty() {
        return Ok(true);
    }
    let types = effective_skill_types.ok_or_else(|| index_nil("skillTypes"))?;
    Ok(does_type_expression_match(require, types, effective_minion_types))
}

/// CalcTools.lua:183-194 `calcLib.getGemStatRequirement(level, isSupport, multi)` -- the in-game formula. `^` is the CRT `pow` (R2:
/// desktop UCRT = Rust `powf`).
pub fn get_gem_stat_requirement(level: f64, is_support: bool, multi: f64) -> f64 {
    if multi == 0.0 {
        return 0.0;
    }
    let stat_type = if is_support { 0.5 } else { 0.7 };
    let req = round((20.0 + (level - 3.0) * 3.0) * (multi / 100.0).powf(0.9) * stat_type, None);
    if req < 14.0 { 0.0 } else { req }
}

/// What `buildSkillInstanceStats` reads of a skill instance.
#[derive(Clone, Copy, Debug)]
pub struct SkillInstance {
    pub level: f64,
    pub quality: f64,
    pub actor_level: Option<f64>,
}

/// CalcTools.lua:197-256 `calcLib.buildSkillInstanceStats(skillInstance, grantedEffect)`: stat -> value, in insertion order (a Lua
/// table; callers must not depend on the order).
pub fn build_skill_instance_stats(si: &SkillInstance, ge: &GrantedEffect) -> Result<indexmap::IndexMap<String, f64>, LuaError> {
    let mut stats: indexmap::IndexMap<String, f64> = indexmap::IndexMap::new();
    fn add(stats: &mut indexmap::IndexMap<String, f64>, k: &str, v: f64) {
        let e = stats.entry(k.to_owned()).or_insert(0.0);
        *e += v;
    }
    if si.quality > 0.0
        && let Some(qs) = &ge.quality_stats
    {
        for (s, v) in qs {
            // `math.modf` -> its integral part only
            add(&mut stats, s, (v * si.quality).trunc());
        }
    }
    let level = ge.level(si.level);
    let actor_level = si.actor_level.or_else(|| level.and_then(|l| l.level_requirement)).unwrap_or(1.0);
    let mut available_effectiveness: Option<f64> = None;
    let arith_nil =
        |what: &str| format!("Modules/CalcTools.lua (buildSkillInstanceStats): attempt to perform arithmetic on a nil value ({what})");
    for (i, stat) in ge.stats.iter().enumerate() {
        let index = i + 1;
        let mut stat_value = level.and_then(|l| l.value(index)).unwrap_or(1.0);
        if let Some(interp) = level.and_then(|l| l.stat_interpolation.as_ref()) {
            let kind = interp.get(i).copied();
            if kind == Some(3.0) {
                let eff = *available_effectiveness.get_or_insert_with(|| {
                    let c = &pob_data::constants::constants().game_constants;
                    let base = c["SkillDamageBaseEffectiveness"];
                    let inc = c["SkillDamageIncrementalEffectiveness"];
                    (base + inc * (actor_level - 1.0))
                        * ge.base_effectiveness.unwrap_or(1.0)
                        * (1.0 + ge.incremental_effectiveness.unwrap_or(0.0)).powf(actor_level - 1.0)
                });
                let v = level.and_then(|l| l.value(index)).ok_or_else(|| arith_nil("level[index]"))?;
                stat_value = round(eff * v, None);
            } else if kind == Some(2.0) {
                // levels are stored sorted by key: `orderedLevels` after `table.sort`
                let ordered: Vec<f64> = ge.levels.iter().map(|l| l.level).collect();
                let current = ordered.iter().position(|&l| l == si.level).map(|p| p + 1);
                let lv = |k: usize| ge.level(ordered[k - 1]).expect("listed level");
                if ordered.len() > 1 {
                    let cur = current.ok_or_else(|| arith_nil("currentLevelIndex"))?;
                    let next = min2((cur + 1) as f64, ordered.len() as f64) as usize;
                    let req = |k: usize| lv(k).level_requirement.ok_or_else(|| arith_nil("levelRequirement/stat"));
                    let st = |k: usize| lv(k).value(index).ok_or_else(|| arith_nil("levelRequirement/stat"));
                    let (next_req, prev_req) = (req(next)?, req(next - 1)?);
                    let (next_stat, prev_stat) = (st(next)?, st(next - 1)?);
                    stat_value = round(
                        prev_stat + (next_stat - prev_stat) * (actor_level - prev_req) / (next_req - prev_req),
                        None,
                    );
                } else {
                    let cur = current.ok_or_else(|| {
                        "Modules/CalcTools.lua (buildSkillInstanceStats): attempt to index a nil value (levels[orderedLevels[nil]])"
                            .to_owned()
                    })?;
                    stat_value = round(lv(cur).value(index).ok_or_else(|| arith_nil("stat"))?, None);
                }
            }
        }
        add(&mut stats, stat, stat_value);
    }
    if let Some(cs) = &ge.constant_stats {
        for (s, v) in cs {
            add(&mut stats, s, *v);
        }
    }
    Ok(stats)
}

/// CalcTools.lua:263-279 `calcLib.getConvertedModTags(mod, multiplier, minionMods)`: new tag tables (`copyTable`), ActorCondition on the
/// parent -> Condition for minion mods, `limit * multiplier` for `limitTotal` tags.
pub fn get_converted_mod_tags(tags: &[Tag], multiplier: f64, minion_mods: bool) -> Vec<Tag> {
    tags.iter()
        .map(|t| {
            if minion_mods && t.get("type").is_str("ActorCondition") && t.get("actor").is_str("parent") {
                let mut n = pob_mods::Table::default();
                n.hash.insert("type".into(), Value::str("Condition"));
                let var = t.get("var");
                if !var.is_nil() {
                    n.hash.insert("var".into(), var);
                }
                Tag::new(n)
            } else if t.get("limitTotal").truthy() {
                let mut n = (*t.table()).clone();
                let limit = t
                    .get("limit")
                    .as_num()
                    .expect("CalcTools.lua:272: arithmetic on a non-number `limit`");
                n.hash.insert("limit".into(), Value::Num(limit * multiplier));
                Tag::new(n)
            } else {
                t.deep_copy()
            }
        })
        .collect()
}
