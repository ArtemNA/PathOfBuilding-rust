//! Modules/CalcActiveSkill.lua:229-876 `calcs.buildActiveSkillModList(env, activeSkill)` with its helpers `mergeLevelMod` (:22),
//! `mergeSkillInstanceMods` (:53) and `getWeaponFlags` (:195), called for every skill of `env.player.activeSkillList` by CalcSetup.lua:1941-1945
//! (MAIN; the `else` branch :1946-1969 is the non-MAIN re-run). R6.4.
//!
//! * The skill mod list is a `ModList` whose parent is `actor.modDB` (= `env.modDB` for the player); `baseSkillModList` is the same table.
//! * `mergeLevelMod`'s cache (:21) only shares the merged mod tables between calls -- nothing mutates them afterwards, so the copies are
//!   made per call (identity is not observable here).
//! * Malformed data mods ([`SkillMod::Malformed`], MINEFIELD L24) are held beside the list ([`ActiveSkill::malformed_mods`]): every query of
//!   the list that reaches their name raises their `query_error()`, as LuaJIT's `ModList` does when its name test passes.
//! * Hash-order walks (MINEFIELD-RUST R13): `pairs(stats)` (:62) and `pairs(part)` (:258) -- the stat order only decides the mod order of
//!   the list (the gate compares multisets), the part keys are distinct. `pairs(activeSkill.effectList)` (:297, :519) is a sequence.
//! * Minions (:668-803): the minion table ([`crate::minion::Minion`]: type, level, life table, weapon data, item set) and, for a minion
//!   skill (`createMinionSkills`, R7.10), the minion actor's side (`actor.minionData`: the damage fixup, `actorLevel`, `summonSkill`).

use std::rc::Rc;

use indexmap::IndexMap;
use pob_data::gems::{GemEntry, gems};
use pob_data::skill_stat_map::{MalformedMod, ModElem, SkillMod};
use pob_data::skills::{GrantedEffect, Skills};
use pob_items::{DataTable, Item};
use pob_lua::number::{max2, min2};
use pob_mods::{Mod, ModCfg, ModType, StoreKind, StoreRef, Str, Tag, Value, compare_mod_params, create_mod};

use crate::active_skill::{ActiveSkill, Buff, EffectRef, SkillCfg};
use crate::calc_tools::{GemLevelTarget, LuaError, SkillInstance, build_skill_instance_stats, validate_gem_level};
use crate::minion::{LifeTable, Minion};
use crate::skills::SkillsState;
use crate::skills_tab::{GemInstance, SkillsTab};

const CAS: &str = "Modules/CalcActiveSkill.lua";

/// What `buildActiveSkillModList` reads of the env. The skill's actor (`activeSkill.actor`: the list's parent `actor.modDB` :492, its items
/// and weapons) is [`SkillActor`], apart: for a player skill of this env it is `env.player`, for a mirage's copy (`calcs.copyActiveSkill`,
/// CalcActiveSkill.lua:181-183, R7.9) the player of ANOTHER env, while `mod_db` here stays the new env's.
pub struct ModListInputs<'a> {
    /// `env.modDB` (read for `SkillData`, :661; the player's skill list has it as parent through [`SkillActor::mod_db`]).
    pub mod_db: &'a StoreRef,
    /// `env.enemy.modDB`.
    pub enemy_db: &'a StoreRef,
    /// `activeSkill.actor.itemList`.
    pub item_list: &'a IndexMap<String, Rc<Item>>,
    pub weapon_data1: &'a DataTable,
    pub weapon_data2: &'a DataTable,
    pub mode_buffs: bool,
    pub mode_combat: bool,
    pub mode_effective: bool,
    /// `env.configInput.meleeDistance or env.configPlaceholder.meleeDistance`.
    pub melee_distance: Value,
    /// `env.configInput.projectileDistance or env.configPlaceholder.projectileDistance`.
    pub projectile_distance: Value,
    /// `env.build.spectreList`.
    pub spectre_list: &'a [String],
    /// `env.build.characterLevel`.
    pub character_level: f64,
    pub enemy_level: f64,
    /// `env.build.itemsTab.items` (a minion's item set, :735-786).
    pub items: &'a IndexMap<u64, Rc<Item>>,
    /// `env.build.itemsTab.itemSets` in `itemSetOrderList` order.
    pub item_sets: &'a [pob_build::ItemSetModel],
    /// `env.theIronMass ~= nil`.
    pub the_iron_mass: bool,
    /// `env.mode == "CALCS"` (R8.6): the `*Calcs` gem fields (CalcActiveSkill.lua:250/:623/:649/:694/:736).
    pub calcs: bool,
}

/// `activeSkill.actor` as `buildActiveSkillModList` reads it: the player (`env.player`) or a minion (`createMinionSkills`, R7.10).
pub(crate) struct SkillActor<'a> {
    /// `actor.modDB` -- the parent of the skill list.
    pub mod_db: &'a StoreRef,
    pub item_list: &'a IndexMap<String, Rc<Item>>,
    pub weapon_data1: &'a DataTable,
    pub weapon_data2: &'a DataTable,
    /// `actor.minionData` and `actor.level` (nil for the player).
    pub minion: Option<(&'a Value, f64)>,
    /// `activeSkill.summonSkill.activeEffect.grantedEffect.name`.
    pub summon_name: Option<String>,
    /// `env.mode == "CALCS" and activeSkill == env.player.mainSkill` (CalcActiveSkill.lua:250/:694/:736, R8.6).
    pub calcs_main: bool,
}

fn skill_type(name: &str) -> u32 {
    *pob_data::globals::globals()
        .skill_type
        .get(name)
        .unwrap_or_else(|| panic!("SkillType.{name} missing")) as u32
}

fn mod_flag(name: &str) -> u32 {
    *pob_data::globals::globals()
        .mod_flag
        .get(name)
        .unwrap_or_else(|| panic!("ModFlag.{name} missing")) as u32
}

fn kw_flag(name: &str) -> u32 {
    *pob_data::globals::globals()
        .keyword_flag
        .get(name)
        .unwrap_or_else(|| panic!("KeywordFlag.{name} missing")) as u32
}

fn gem(id: &str) -> &'static GemEntry {
    &gems().gems[id]
}

/// `data.minions` (Data/Minions.lua via `data/minions.json`) as a Lua table -- R6 only asks whether a type exists and its `hostile`.
pub(crate) fn minion_data(id: &str) -> Value {
    thread_local! {
        static M: std::cell::OnceCell<Value> = const { std::cell::OnceCell::new() };
    }
    M.with(|c| {
        c.get_or_init(|| {
            let j = pob_data::read_json("minions.json").unwrap_or_else(|e| panic!("pob-data: minions.json: {e}"));
            let v = pob_data::wire::json_to_value(&j).unwrap_or_else(|e| panic!("pob-data: minions.json: {e}"));
            v.get("minions")
        })
        .get(id)
    })
}

/// `data.minions[id] ~= nil`.
pub fn is_minion(id: &str) -> bool {
    !minion_data(id).is_nil()
}

/// Lua `a or b`.
fn or(a: Value, b: Value) -> Value {
    if a.truthy() { a } else { b }
}

/// `str:gsub("^%l", string.upper)`.
fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) if f.is_ascii_lowercase() => f.to_ascii_uppercase().to_string() + c.as_str(),
        _ => s.to_owned(),
    }
}

/// `str:gsub("%s+", "")` (Lua `%s`: space, \t, \n, \v, \f, \r).
fn strip_space(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
        .collect()
}

/// A query of the skill list that reaches a malformed mod's name raises its LuaJIT error.
fn guard(malformed: &[MalformedMod], names: &[&str]) -> Result<(), LuaError> {
    match malformed.iter().find(|m| names.contains(&&*m.name)) {
        Some(m) => Err(m.query_error()),
        None => Ok(()),
    }
}

fn lua<T>(r: pob_mods::LuaResult<T>) -> Result<T, LuaError> {
    r.map_err(|e| e.0)
}

/// `mergeLevelMod`'s value write (:33-44) onto a shallow copy of the mod's value.
pub(crate) fn level_value(old: &Value, value: f64) -> Value {
    match old {
        Value::Table(_) | Value::Mod(_) => {
            let mut t = old.clone();
            let inner = t.get("mod");
            if inner.truthy() {
                let mut im = inner;
                im.set("value", Value::Num(value));
                t.set("mod", im);
            } else {
                t.set("value", Value::Num(value));
            }
            t
        }
        _ => Value::Num(value),
    }
}

/// CalcActiveSkill.lua:22-50 `mergeLevelMod(modList, mod, value)` (value is never nil at the call sites).
fn merge_level_mod(ml: &StoreRef, malformed: &mut Vec<MalformedMod>, m: &SkillMod, value: f64) {
    match m {
        SkillMod::Mod(md) => {
            let mut n = md.clone();
            n.value = level_value(&md.value, value);
            ml.borrow_mut().add_mod(Rc::new(n));
        }
        SkillMod::Malformed(mm) => {
            let mut n = mm.clone();
            n.value = level_value(&mm.value, value);
            malformed.push(n);
        }
    }
}

/// `calcLib.validateGemLevel(effect)` on a shared effect table.
fn validate_effect_level(sk: &Skills, e: &EffectRef) -> Result<f64, LuaError> {
    let (ge_idx, level, gem_data) = {
        let b = e.borrow();
        (b.granted_effect, b.level, b.gem_data.clone())
    };
    let l = validate_gem_level(
        &GemLevelTarget {
            granted_effect: &sk.effects[ge_idx],
            natural_max_level: gem_data.as_deref().map(|g| gem(g).natural_max_level),
        },
        level,
    )?;
    e.borrow_mut().level = Some(l);
    Ok(l)
}

/// CalcActiveSkill.lua:62-72 -- every `statMap` mod of every stat, with its computed value (the shared walk of `mergeSkillInstanceMods`).
pub(crate) fn for_each_stat_mod(
    sk: &Skills,
    ge: &GrantedEffect,
    stats: &IndexMap<String, f64>,
    mut f: impl FnMut(&SkillMod, f64),
) -> Result<(), LuaError> {
    for (stat, &stat_value) in stats {
        let Some(map) = sk.stat_map(ge, stat)? else { continue };
        for elem in &map.mods {
            match elem {
                ModElem::Mod(m) => {
                    let v = map
                        .value
                        .unwrap_or(stat_value * map.mult.unwrap_or(1.0) / map.div.unwrap_or(1.0) + map.base.unwrap_or(0.0));
                    f(m, v);
                }
                ModElem::Group { mods, div, mult } => {
                    // a group carries no `value`/`base`
                    let v = stat_value * mult.unwrap_or(1.0) / div.unwrap_or(1.0) + 0.0;
                    for m in mods {
                        f(m, v);
                    }
                }
            }
        }
    }
    Ok(())
}

/// CalcActiveSkill.lua:53-79 `calcs.mergeSkillInstanceMods(env, modList, skillEffect, extraStats)`.
fn merge_skill_instance_mods(
    sk: &Skills,
    ml: &StoreRef,
    malformed: &mut Vec<MalformedMod>,
    effect: &EffectRef,
    extra_stats: Option<&[Value]>,
) -> Result<(), LuaError> {
    let level = validate_effect_level(sk, effect)?;
    let (ge_idx, quality, actor_level) = {
        let b = effect.borrow();
        (b.granted_effect, b.quality, b.actor_level)
    };
    let ge = &sk.effects[ge_idx];
    let quality = quality.ok_or_else(|| "Modules/CalcTools.lua:194: attempt to compare nil with number".to_owned())?;
    let mut stats = build_skill_instance_stats(
        &SkillInstance {
            level,
            quality,
            actor_level,
        },
        ge,
    )?;
    if let Some(extra) = extra_stats
        && !extra.is_empty()
    {
        for stat in extra {
            let key = stat.get("key");
            let key = key
                .as_str()
                .unwrap_or_else(|| unimplemented!("{CAS}:59 ExtraSkillStat with a non-string key {key:?}"));
            let v = stat
                .get("value")
                .as_num()
                .ok_or_else(|| format!("{CAS}:59: attempt to perform arithmetic on field 'value'"))?;
            let e = stats.entry(key.to_owned()).or_insert(0.0);
            *e += v;
        }
    }
    for_each_stat_mod(sk, ge, &stats, |m, v| merge_level_mod(ml, malformed, m, v))?;
    // `modList:AddList(grantedEffect.baseMods)` -- the data's own mod tables
    if let Some(base) = &ge.base_mods {
        for elem in base {
            match elem {
                ModElem::Mod(SkillMod::Mod(m)) => ml.borrow_mut().add_mod(Rc::new(m.clone())),
                ModElem::Mod(SkillMod::Malformed(m)) => malformed.push(m.clone()),
                ModElem::Group { .. } => unimplemented!("{CAS}:78 AddList of a mod group in {}.baseMods (none in the data)", ge.id),
            }
        }
    }
    Ok(())
}

/// CalcActiveSkill.lua:195-226 `getWeaponFlags(env, weaponData, weaponTypes)` -> (flags, info present).
fn get_weapon_flags(
    wd: &DataTable,
    weapon_types: &[&pob_data::wire::OrderedMap<bool>],
) -> (Option<u32>, Option<&'static pob_data::rules::WeaponTypeInfo>) {
    let ty = wd.get("type");
    let Some(info) = ty.as_str().and_then(|t| pob_data::rules::rules().weapon_type_info.get(t)) else {
        return (None, None);
    };
    let t = ty.as_str().expect("checked");
    let all1h = wd.get("countsAsAll1H").truthy();
    for types in weapon_types {
        let has = |k: &str| types.get(k).copied().unwrap_or(false);
        if !has(t)
            && (!all1h || !(has("Claw") || has("Dagger") || has("One Handed Axe") || has("One Handed Mace") || has("One Handed Sword")))
        {
            return (None, Some(info));
        }
    }
    let mut flags = info
        .flag
        .as_deref()
        .map(mod_flag)
        .unwrap_or_else(|| panic!("{CAS}:208 ModFlag[nil] for weapon type {t}"));
    if all1h {
        flags = mod_flag("Axe") | mod_flag("Claw") | mod_flag("Dagger") | mod_flag("Mace") | mod_flag("Sword");
    }
    if t != "None" {
        flags |= mod_flag("Weapon");
        flags |= if info.one_hand == Some(true) {
            mod_flag("Weapon1H")
        } else {
            mod_flag("Weapon2H")
        };
        flags |= if info.melee == Some(true) {
            mod_flag("WeaponMelee")
        } else {
            mod_flag("WeaponRanged")
        };
    }
    (Some(flags), Some(info))
}

fn gem_mut(tab: &mut SkillsTab, (g, k): (usize, usize)) -> &mut GemInstance {
    &mut tab.socket_group_list_mut()[g].gem_list[k]
}

/// `activeEffect.gemData and activeEffect.gemData.secondaryGrantedEffect`.
fn has_secondary(sk: &Skills, gem_data: Option<&str>) -> bool {
    gem_data.is_some_and(|g| {
        gem(g)
            .secondary_granted_effect_id
            .as_deref()
            .is_some_and(|id| sk.index.contains_key(id))
    })
}

/// The flags a skill part sets (`true`) or clears (`false`) -- `pairs(part)` over its boolean fields.
fn part_flags(p: &pob_data::skills::SkillPart) -> [(&'static str, Option<bool>); 10] {
    [
        ("area", p.area),
        ("projectile", p.projectile),
        ("melee", p.melee),
        ("stages", p.stages),
        ("cast", p.cast),
        ("spell", p.spell),
        ("chaining", p.chaining),
        ("channelRelease", p.channel_release),
        ("attack", p.attack),
        ("hit", p.hit),
    ]
}

/// `skillFlags[k] = v` with Lua nil-deletion.
fn set_flag(flags: &mut IndexMap<String, bool>, k: &str, v: Option<bool>) {
    match v {
        Some(b) => {
            flags.insert(k.to_owned(), b);
        }
        None => {
            flags.shift_remove(k);
        }
    }
}

/// `skillData[k] = v` with Lua nil-deletion.
fn set_data(data: &mut IndexMap<String, Value>, k: &str, v: Value) {
    if v.is_nil() {
        data.shift_remove(k);
    } else {
        data.insert(k.to_owned(), v);
    }
}

fn num(v: Option<f64>) -> Value {
    v.map_or(Value::Nil, Value::Num)
}

/// `modList:NewMod(name, type, value, source)`.
fn add_mod(ml: &StoreRef, name: &str, ty: ModType, value: Value, source: &str) {
    ml.borrow_mut()
        .new_mod(create_mod(name, Some(ty), value, Some(source), 0, 0, Vec::new()));
}

/// `NewMod(name, type, value, source, tag?)` with the tag in the flags position -- `createMod` sniffs types, so a table there is a tag; a
/// nil source shifts the tag past a nil (`{ ..., nil, tag }`), out of `ipairs`' reach.
fn new_mod_tagged(name: &str, value: Value, source: Option<&str>, tag: Option<&Tag>) -> Mod {
    let tags = match (source, tag) {
        (Some(_), Some(t)) => vec![t.clone()],
        _ => Vec::new(),
    };
    create_mod(name, Some(ModType::Base), value, source, 0, 0, tags)
}

/// CalcSetup.lua:1941-1945: `buildActiveSkillModList` for every skill of `env.player.activeSkillList` (a sequence, in order). Returns the
/// upstream branches skipped (`file:line`).
pub fn build_all(st: &mut SkillsState, inp: &ModListInputs<'_>) -> Result<Vec<String>, LuaError> {
    let mut unimplemented = Vec::new();
    for i in 0..st.active_skill_list.len() {
        build_active_skill_mod_list(st, i, inp, &mut unimplemented)?;
    }
    Ok(unimplemented)
}

/// CalcActiveSkill.lua:229-876 `calcs.buildActiveSkillModList(env, activeSkill)` for `env.player.activeSkillList[idx]` (actor = the player,
/// `summonSkill` nil, mode MAIN).
#[allow(clippy::too_many_lines)]
pub fn build_active_skill_mod_list(
    st: &mut SkillsState,
    idx: usize,
    inp: &ModListInputs<'_>,
    unimplemented: &mut Vec<String>,
) -> Result<(), LuaError> {
    let sk = st.skills.clone();
    let is_main = st.main_skill == Some(idx);
    let SkillsState {
        tab,
        active_skill_list,
        aux_skill_list,
        ..
    } = st;
    let skill: &mut ActiveSkill = &mut active_skill_list[idx];
    let actor = SkillActor {
        mod_db: inp.mod_db,
        item_list: inp.item_list,
        weapon_data1: inp.weapon_data1,
        weapon_data2: inp.weapon_data2,
        minion: None,
        summon_name: None,
        calcs_main: inp.calcs && is_main,
    };
    // :871-874 auxiliary skill list
    if build_skill(&sk, tab, skill, inp, &actor, unimplemented)? {
        aux_skill_list.push(idx);
    }
    Ok(())
}

/// CalcActiveSkill.lua:229-876 `calcs.buildActiveSkillModList(env, activeSkill)` for any actor (mode MAIN); `Ok(true)` when the skill goes
/// into `env.auxSkillList` (:871-874).
#[allow(clippy::too_many_lines)]
pub(crate) fn build_skill(
    sk: &Rc<Skills>,
    tab: &mut SkillsTab,
    skill: &mut ActiveSkill,
    inp: &ModListInputs<'_>,
    actor: &SkillActor<'_>,
    _unimplemented: &mut Vec<String>,
) -> Result<bool, LuaError> {
    let ae = skill.active_effect.clone();
    let (act_idx, gem_data, src) = {
        let e = ae.borrow();
        (e.granted_effect, e.gem_data.clone(), e.src_instance)
    };
    let act: &GrantedEffect = &sk.effects[act_idx];
    let secondary = has_secondary(sk, gem_data.as_deref());
    let src_nil = |line: u32| format!("{CAS}:{line}: attempt to index field 'srcInstance' (a nil value)");
    let has_type = |skill: &ActiveSkill, n: &str| skill.skill_types.contains(&skill_type(n));
    let summon = skill.summon_skill.is_some();

    // :236-245 mode flags
    if inp.mode_buffs {
        skill.skill_flags.insert("buffs".into(), true);
    }
    if inp.mode_combat {
        skill.skill_flags.insert("combat".into(), true);
    }
    if inp.mode_effective {
        skill.skill_flags.insert("effective".into(), true);
    }

    // :247-270 multipart skills (`env.mode == "CALCS" and activeSkill == env.player.mainSkill`: the `skillPartCalcs` field)
    match act.parts.as_ref() {
        Some(parts) if parts.len() > 1 => {
            let s = src.ok_or_else(|| src_nil(if actor.calcs_main { 251 } else { 254 }))?;
            let gi = gem_mut(tab, s);
            let part_no = if actor.calcs_main {
                let k = min2(parts.len() as f64, gi.skill_part_calcs.unwrap_or(1.0));
                gi.skill_part_calcs = Some(k);
                k
            } else {
                let k = min2(parts.len() as f64, gi.skill_part.unwrap_or(1.0));
                gi.skill_part = Some(k);
                k
            };
            skill.skill_part = Some(part_no);
            let part = (part_no >= 1.0 && part_no.fract() == 0.0)
                .then(|| parts.get(part_no as usize - 1))
                .flatten()
                .ok_or_else(|| format!("{CAS}:258: bad argument #1 to 'pairs' (table expected, got nil)"))?;
            for (k, v) in part_flags(part) {
                match v {
                    Some(true) => set_flag(&mut skill.skill_flags, k, Some(true)),
                    Some(false) => set_flag(&mut skill.skill_flags, k, None),
                    None => {}
                }
            }
            skill.skill_part_name = Some(part.name.clone());
            skill.skill_flags.insert("multiPart".into(), true);
        }
        _ => {
            if let Some(s) = src
                && !secondary
            {
                let gi = gem_mut(tab, s);
                gi.skill_part = None;
                gi.skill_part_calcs = None;
            }
        }
    }

    // :272-282 shield / staff requirements
    let item_type = |slot: &str| actor.item_list.get(slot).and_then(|i| i.ty.clone());
    if (has_type(skill, "RequiresShield") || skill.flag("shieldAttack")) && !summon && item_type("Weapon 2").as_deref() != Some("Shield") {
        skill.skill_flags.insert("disable".into(), true);
        skill.disable_reason = Some("This skill requires a Shield".into());
    }
    if has_type(skill, "RequiresStaff") && !summon && item_type("Weapon 1").as_deref() != Some("Staff") {
        skill.skill_flags.insert("disable".into(), true);
        skill.disable_reason = Some("This skill requires a Staff".into());
    }

    // :284-348 weapon flags
    if skill.flag("shieldAttack") {
        skill.skill_flags.insert("weapon2Attack".into(), true);
        skill.weapon2_flags = Some(0);
    } else {
        if skill.flag("forceSourceWeapon")
            && let Some(g) = skill.socket_group
            && tab.socket_group_list()[g].source_item.is_some()
        {
            let source_slot = tab.socket_group_list()[g].slot.clone().unwrap_or_default();
            skill
                .skill_flags
                .insert("forceMainHand".into(), source_slot.starts_with("Weapon 1"));
            skill.skill_flags.insert("forceOffHand".into(), source_slot.starts_with("Weapon 2"));
        }
        // `{ activeGrantedEffect.weaponTypes }` -- a nil first entry leaves the table empty (`#` = 0), so the supports' land at [1]
        let mut weapon_types: Vec<&pob_data::wire::OrderedMap<bool>> = act.weapon_types.iter().collect();
        for e in &skill.effect_list {
            let ge = &sk.effects[e.borrow().granted_effect];
            if ge.support
                && let Some(w) = &ge.weapon_types
            {
                weapon_types.push(w);
            }
        }
        let (mut w1_flags, mut w1_info) = (None, None);
        if !skill.flag("forceOffHand") {
            (w1_flags, w1_info) = get_weapon_flags(actor.weapon_data1, &weapon_types);
        }
        if w1_flags.is_none() && summon {
            // :306-309 minion skills seem to ignore weapon types
            let none = pob_data::rules::rules()
                .weapon_type_info
                .get("None")
                .unwrap_or_else(|| panic!("{CAS}:308: data.weaponTypeInfo.None is nil"));
            w1_flags = Some(mod_flag(none.flag.as_deref().unwrap_or_else(|| panic!("{CAS}:308: ModFlag[nil]"))));
            w1_info = Some(none);
        }
        if let Some(fl) = w1_flags {
            if skill.flag("attack") || skill.flag("dotFromAttack") {
                skill.weapon1_flags = Some(fl);
                skill.skill_flags.insert("weapon1Attack".into(), true);
                let melee = w1_info.expect("info with flags").melee == Some(true);
                if melee && skill.flag("melee") {
                    skill.skill_flags.shift_remove("projectile");
                } else if !melee && skill.flag("projectile") {
                    skill.skill_flags.shift_remove("melee");
                }
            }
        } else if (has_type(skill, "DualWieldOnly") || skill.flag("forceMainHand") || w1_info.is_some()) && !summon {
            skill.skill_flags.insert("disable".into(), true);
            skill.disable_reason = Some("Main Hand weapon is not usable with this skill".into());
        }
        if !skill.flag("forceMainHand") {
            let (w2_flags, w2_info) = get_weapon_flags(actor.weapon_data2, &weapon_types);
            if let Some(fl) = w2_flags {
                let (t1, t2) = (actor.weapon_data1.get("type"), actor.weapon_data2.get("type"));
                if has_type(skill, "DualWieldRequiresDifferentTypes")
                    && t1.raw_eq(&t2)
                    && !(actor.weapon_data2.get("countsAsAll1H").truthy() || actor.weapon_data1.get("countsAsAll1H").truthy())
                {
                    skill.skill_flags.insert("disable".into(), true);
                    skill
                        .disable_reason
                        .get_or_insert_with(|| "Weapon Types Need to be Different".into());
                } else if skill.flag("attack") || skill.flag("dotFromAttack") {
                    skill.weapon2_flags = Some(fl);
                    skill.skill_flags.insert("weapon2Attack".into(), true);
                }
            } else if (has_type(skill, "DualWieldOnly") || w2_info.is_some()) && !summon {
                skill.skill_flags.insert("disable".into(), true);
                skill
                    .disable_reason
                    .get_or_insert_with(|| "Off Hand weapon is not usable with this skill".into());
            } else if skill.flag("disable") {
                skill.disable_reason = Some("No usable weapon equipped".into());
            }
        }
        if skill.flag("attack") {
            // `weapon1Attack and weapon2Attack`
            let v = match skill.skill_flags.get("weapon1Attack").copied() {
                Some(true) => skill.skill_flags.get("weapon2Attack").copied(),
                other => other,
            };
            set_flag(&mut skill.skill_flags, "bothWeaponAttack", v);
        }
    }

    // :350-356 stat-map flagged skill flags (activeEffect.level is not validated yet)
    {
        let (level, quality, actor_level) = {
            let e = ae.borrow();
            (e.level, e.quality, e.actor_level)
        };
        let quality = quality.ok_or_else(|| "Modules/CalcTools.lua:194: attempt to compare nil with number".to_owned())?;
        let stats = build_skill_instance_stats(
            &SkillInstance {
                level: level.unwrap_or(f64::NAN),
                quality,
                actor_level,
            },
            act,
        )?;
        for (stat, &v) in &stats {
            let map = sk.stat_map(act, stat)?;
            if v != 0.0
                && let Some(map) = map
                && let Some(f) = &map.skill_flag
            {
                skill.skill_flags.insert(f.clone(), true);
            }
        }
    }

    // :357-378 skill mod flags
    let mut skill_mod_flags = 0u32;
    if skill.flag("hit") {
        skill_mod_flags |= mod_flag("Hit");
    }
    if skill.flag("attack") {
        skill_mod_flags |= mod_flag("Attack");
    } else {
        skill_mod_flags |= mod_flag("Cast");
        if skill.flag("spell") {
            skill_mod_flags |= mod_flag("Spell");
        }
    }
    if skill.flag("melee") {
        skill_mod_flags |= mod_flag("Melee");
    } else if skill.flag("projectile") {
        skill_mod_flags |= mod_flag("Projectile");
        skill.skill_flags.insert("chaining".into(), true);
    }
    if skill.flag("area") {
        skill_mod_flags |= mod_flag("Area");
    }

    // :380-438 skill keyword flags
    let mut kw = 0u32;
    if skill.flag("hit") {
        kw |= kw_flag("Hit");
    }
    for (t, k) in [
        ("Aura", "Aura"),
        ("AppliesCurse", "Curse"),
        ("Warcry", "Warcry"),
        ("Movement", "Movement"),
        ("Vaal", "Vaal"),
        ("Lightning", "Lightning"),
        ("Cold", "Cold"),
        ("Fire", "Fire"),
        ("Chaos", "Chaos"),
        ("Physical", "Physical"),
    ] {
        if has_type(skill, t) {
            kw |= kw_flag(k);
        }
    }
    if skill.flag("weapon1Attack") && skill.weapon1_flags.expect("weapon1Attack sets weapon1Flags") & mod_flag("Bow") != 0 {
        kw |= kw_flag("Bow");
    }
    if skill.flag("brand") {
        kw |= kw_flag("Brand");
    }
    if skill.flag("arrow") {
        kw |= kw_flag("Arrow");
    }
    if skill.flag("totem") {
        kw |= kw_flag("Totem");
    } else if skill.flag("trap") {
        kw |= kw_flag("Trap");
    } else if skill.flag("mine") {
        kw |= kw_flag("Mine");
    } else if !has_type(skill, "Triggered") {
        skill.skill_flags.insert("selfCast".into(), true);
    }
    if has_type(skill, "Attack") {
        kw |= kw_flag("Attack");
    }
    if has_type(skill, "Spell") && !skill.flag("cast") {
        kw |= kw_flag("Spell");
    }

    // :440-453 totem ID
    if skill.flag("totem") {
        skill.skill_totem_id = Some(act.skill_totem_id.unwrap_or(match act.color {
            Some(2.0) => 2.0,
            Some(3.0) => 3.0,
            _ => 1.0,
        }));
    }

    // :455-460 distance
    let effective_range = if skill.flag("melee") {
        inp.melee_distance.clone()
    } else {
        inp.projectile_distance.clone()
    };

    // :462-489 the cfg structures
    let gem_cfg = ae.borrow().gem_cfg.clone();
    let skill_dist = if inp.mode_effective {
        match effective_range {
            Value::Num(x) => Some(x),
            Value::Nil => None,
            other => unimplemented!("{CAS}:474 skillDist = a {} config value", other.type_name()),
        }
    } else {
        None
    };
    let skill_cfg = SkillCfg {
        flags: f64::from(skill_mod_flags | skill.weapon1_flags.or(skill.weapon2_flags).unwrap_or(0)),
        keyword_flags: f64::from(kw),
        skill_name: act.name.strip_prefix("Vaal ").unwrap_or(&act.name).to_owned(),
        summon_skill_name: actor.summon_name.clone(),
        skill_gem: gem_data.clone(),
        skill_granted_effect: act_idx,
        skill_part: skill.skill_part,
        skill_cond: IndexMap::new(),
        skill_cond_inherits: false,
        skill_dist,
        slot_name: skill
            .slot_name
            .clone()
            .or_else(|| gem_cfg.as_ref().and_then(|c| c.group.slot_name.clone())),
        socket_color: gem_cfg.as_ref().and_then(|c| c.socket_color.clone()),
        socket_num: gem_cfg.as_ref().map(|c| c.socket_num),
    };
    if skill.flag("weapon1Attack") {
        let mut c = skill_cfg.clone();
        c.skill_cond = IndexMap::from([("MainHandAttack".to_owned(), true)]);
        c.skill_cond_inherits = true;
        c.flags = f64::from(skill_mod_flags | skill.weapon1_flags.expect("weapon1Attack"));
        skill.weapon1_cfg = Some(c);
    }
    if skill.flag("weapon2Attack") {
        let mut c = skill_cfg.clone();
        c.skill_cond = IndexMap::from([("OffHandAttack".to_owned(), true)]);
        c.skill_cond_inherits = true;
        c.flags = f64::from(skill_mod_flags | skill.weapon2_flags.expect("weapon2Attack"));
        skill.weapon2_cfg = Some(c);
    }
    let cfg: ModCfg = skill_cfg.mod_cfg(sk, &skill.skill_types, None);
    skill.skill_cfg = Some(skill_cfg);

    // :491-501 the skill mod list (parent: `actor.modDB`) and a minion actor's damage fixup
    let ml = pob_mods::ModStore::new_list(Some(actor.mod_db.clone()));
    skill.skill_mod_list = Some(ml.clone());
    if let Some((md, _)) = actor.minion
        && let Some(fix) = md.get("damageFixup").as_num()
    {
        ml.borrow_mut().new_mod(create_mod(
            "Damage",
            Some(ModType::More),
            Value::Num(-100.0 * fix),
            Some("Damage Fixup"),
            mod_flag("Attack"),
            0,
            Vec::new(),
        ));
        ml.borrow_mut().new_mod(create_mod(
            "Speed",
            Some(ModType::More),
            Value::Num(100.0 * fix),
            Some("Damage Fixup"),
            mod_flag("Attack"),
            0,
            Vec::new(),
        ));
    }
    let mut malformed: Vec<MalformedMod> = Vec::new();

    // :503-508 DisableSkill
    let curse_application_skill = skill.socket_group.is_some_and(|g| tab.socket_group_list()[g].source_item.is_some())
        && skill.flag("curse")
        && src.is_some_and(|s| {
            let gi = &tab.socket_group_list()[s.0].gem_list[s.1];
            gi.no_supports == Some(true) && gi.triggered
        });
    guard(&malformed, &["DisableSkill"])?;
    if lua(ml.borrow().flag(Some(&cfg), &["DisableSkill"]))?.unwrap_or(false) {
        let enable = lua(ml.borrow().flag(Some(&cfg), &["EnableSkill"]))?.unwrap_or(false)
            || (curse_application_skill && lua(ml.borrow().flag(None, &["ForceEnableCurseApplication"]))?.unwrap_or(false));
        if !enable {
            skill.skill_flags.insert("disable".into(), true);
            skill.disable_reason = Some("Skills of this type are disabled".into());
        }
    }

    // :510-516 a disabled skill stops here
    if skill.flag("disable") {
        skill.skill_flags.clear();
        skill.skill_flags.insert("disable".into(), true);
        let l = validate_effect_level(sk, &ae)?;
        ae.borrow_mut().granted_effect_level = act.level(l).map(|_| l);
        skill.malformed_mods = malformed;
        return Ok(false);
    }

    // :518-549 support gem modifiers
    for se in skill.effect_list.clone() {
        let ge = &sk.effects[se.borrow().granted_effect];
        if !ge.support {
            continue;
        }
        merge_skill_instance_mods(sk, &ml, &mut malformed, &se, None)?;
        let lv = se.borrow().level.expect("validated");
        let level = ge
            .level(lv)
            .ok_or_else(|| format!("{CAS}:523: attempt to index local 'level' (a nil value)"))?;
        if let Some(m) = level.mana_multiplier {
            add_mod(&ml, "SupportManaMultiplier", ModType::More, Value::Num(m), &ge.mod_source);
        }
        if let Some(p) = level.mana_reservation_percent {
            skill.skill_data.insert("manaReservationPercent".into(), Value::Num(p));
        }
        // multiple triggers / the trigger reference
        let matched = ge.add_skill_types.is_some() && !skill.flag("disable");
        if matched && ge.is_trigger {
            if skill.triggered_by.is_some() {
                skill.skill_flags.insert("disable".into(), true);
                skill.disable_reason = Some("This skill is supported by more than one trigger".into());
            } else {
                skill.triggered_by = Some(se.clone());
            }
        }
        if let Some(m) = level.pvp_damage_multiplier {
            add_mod(&ml, "PvpDamageMultiplier", ModType::More, Value::Num(m), &ge.mod_source);
        }
        if let Some(u) = level.stored_uses {
            skill.skill_data.insert("storedUses".into(), Value::Num(u));
        }
        if let Some(u) = level.vaal_stored_uses {
            // `skillData.storedUses or 0 + level.vaalStoredUses`
            let v = or(skill.skill_data.get("storedUses").cloned().unwrap_or_default(), Value::Num(0.0 + u));
            set_data(&mut skill.skill_data, "storedUses", v);
        }
    }

    // :551-558 gem level / quality / socket quality
    let src_gem: Option<GemInstance> = src.map(|s| tab.socket_group_list()[s.0].gem_list[s.1].clone());
    let (ae_level, ae_quality) = {
        let e = ae.borrow();
        (e.level, e.quality)
    };
    let gem_level = match src_gem.as_ref().and_then(|g| g.level) {
        Some(l) => Value::Num(l),
        None => num(ae_level),
    };
    let gem_quality = match src_gem.as_ref().and_then(|g| g.quality) {
        Some(q) => Value::Num(q),
        None => num(ae_quality),
    };
    add_mod(&ml, "GemLevel", ModType::Base, gem_level, "Max Level");
    add_mod(&ml, "GemQuality", ModType::Base, gem_quality, "Max Quality");
    // `srcInstance and srcInstance.matchesSocket or activeEffect.matchesSocket` (the effect never has one)
    if src_gem.as_ref().is_some_and(|g| g.matches_socket == Some(true)) {
        add_mod(
            &ml,
            "GemSocketQuality",
            ModType::Base,
            Value::Num(crate::setup::misc("MatchingSocketQualityBonus")),
            "Socket Quality",
        );
    }
    // :559-569 SupportedGemProperty
    guard(&malformed, &["SupportedGemProperty"])?;
    let props = lua(ml.borrow().tabulate(Some(ModType::List), Some(&cfg), &["SupportedGemProperty"]))?;
    for p in props {
        let value = &p.value;
        let is_support_gem = gem_data
            .as_deref()
            .is_none_or(|g| gem(g).tags.get("support").copied().unwrap_or(false));
        if value.get("keyword").is_str("grants_active_skill") && !is_support_gem {
            let key = value.get("key");
            let key = key
                .as_str()
                .unwrap_or_else(|| unimplemented!("{CAS}:566 SupportedGemProperty with a non-string key"))
                .to_owned();
            let v = value
                .get("value")
                .as_num()
                .ok_or_else(|| format!("{CAS}:566: attempt to perform arithmetic on field 'value'"))?;
            {
                let mut e = ae.borrow_mut();
                if key == "quality" {
                    e.support_quality = Some(e.support_quality.unwrap_or(0.0) + v);
                }
                let arith = || format!("{CAS}:566: attempt to perform arithmetic on field '{key}' (a nil value)");
                let slot = match key.as_str() {
                    "level" => &mut e.level,
                    "quality" => &mut e.quality,
                    "req" => &mut e.req,
                    "globalQuality" => &mut e.global_quality,
                    "itemQuality" => &mut e.item_quality,
                    "supportQuality" => &mut e.support_quality,
                    "socketQuality" => &mut e.socket_quality,
                    other => {
                        let x = e.other.get_mut(other).ok_or_else(arith)?;
                        *x += v;
                        drop(e);
                        let first = p.mod_.tags.first();
                        ml.borrow_mut().new_mod(new_mod_tagged(
                            &format!("GemSupport{}", upper_first(&key)),
                            Value::Num(v),
                            p.mod_.source.as_deref(),
                            first,
                        ));
                        continue;
                    }
                };
                *slot = Some(slot.ok_or_else(arith)? + v);
            }
            ml.borrow_mut().new_mod(new_mod_tagged(
                &format!("GemSupport{}", upper_first(&key)),
                Value::Num(v),
                p.mod_.source.as_deref(),
                p.mod_.tags.first(),
            ));
        }
    }
    // :571-574 gem item properties
    let info = ae.borrow().gem_property_info.clone().unwrap_or_default();
    for gp in &info {
        let key = gp.value.get("key");
        let key = key
            .as_str()
            .unwrap_or_else(|| unimplemented!("{CAS}:573 GemProperty with a non-string key"));
        ml.borrow_mut().new_mod(new_mod_tagged(
            &format!("GemItem{}", upper_first(key)),
            gp.value.get("value"),
            gp.mod_.source.as_deref(),
            gp.mod_.tags.first(),
        ));
    }

    // :576-579 active gem modifiers (`actorLevel = actor.minionData and actor.level`)
    ae.borrow_mut().actor_level = actor.minion.map(|(_, l)| l);
    guard(&malformed, &["ExtraSkillStat"])?;
    let extra = lua(ml.borrow().list(Some(&cfg), &["ExtraSkillStat"]))?;
    merge_skill_instance_mods(sk, &ml, &mut malformed, &ae, Some(&extra))?;
    let ae_level = ae.borrow().level.expect("validated");
    let level = act.level(ae_level);
    ae.borrow_mut().granted_effect_level = level.map(|_| ae_level);

    // :581-607 extra modifiers from the granted effect level (no level of the data has `damageMultiplier`)
    let level = level.ok_or_else(|| format!("{CAS}:583: attempt to index local 'level' (a nil value)"))?;
    set_data(&mut skill.skill_data, "CritChance", num(level.crit_chance));
    if let Some(t) = level.attack_time {
        skill.skill_data.insert("attackTime".into(), Value::Num(t));
    }
    if let Some(m) = level.attack_speed_multiplier {
        skill.skill_data.insert("attackSpeedMultiplier".into(), Value::Num(m));
    }
    if let Some(c) = level.cooldown {
        skill.skill_data.insert("cooldown".into(), Value::Num(c));
    }
    if let Some(u) = level.stored_uses {
        skill.skill_data.insert("storedUses".into(), Value::Num(u));
    }
    if let Some(u) = level.vaal_stored_uses {
        let v = or(skill.skill_data.get("storedUses").cloned().unwrap_or_default(), Value::Num(0.0 + u));
        set_data(&mut skill.skill_data, "storedUses", v);
    }
    if let Some(d) = level.soul_prevention_duration {
        skill.skill_data.insert("soulPreventionDuration".into(), Value::Num(d));
    }
    if let Some(m) = level.pvp_damage_multiplier {
        add_mod(&ml, "PvpDamageMultiplier", ModType::More, Value::Num(m), &act.mod_source);
    }

    // :609-614 extra modifiers from other sources
    guard(&malformed, &["ExtraSkillMod"])?;
    let extra_mods = lua(ml.borrow().list(Some(&cfg), &["ExtraSkillMod"]))?;
    let mut extra_list = Vec::new();
    for v in extra_mods {
        let Value::Mod(m) = v.get("mod") else {
            unimplemented!("{CAS}:612 ExtraSkillMod whose value.mod is not a mod")
        };
        ml.borrow_mut().add_mod(m.clone());
        extra_list.push(m);
    }
    skill.extra_skill_mod_list = Some(extra_list);

    // :616-619 totem level
    if skill.flag("totem") {
        set_data(&mut skill.skill_data, "totemLevel", num(level.level_requirement));
    }

    // :621-631 active mine count: `(env.mode == "CALCS" and srcInstance.skillMineCountCalcs) or (env.mode ~= "CALCS" and
    // srcInstance.skillMineCount)` -- a false-y first half falls to the second, which is `false` in CALCS (no count)
    if skill.flag("mine") {
        let s = src.ok_or_else(|| src_nil(623))?;
        let gi = &tab.socket_group_list()[s.0].gem_list[s.1];
        let count = if inp.calcs {
            gi.skill_mine_count_calcs
        } else {
            gi.skill_mine_count
        };
        skill.active_mine_count = count;
        if let Some(c) = count
            && c > 0.0
        {
            add_mod(&ml, "Multiplier:ActiveMineCount", ModType::Base, Value::Num(c), "Base");
            let mut e = inp.enemy_db.borrow_mut();
            let prev = e.multipliers.get("ActiveMineCount").copied().unwrap_or(0.0);
            e.multipliers.insert(Str::from("ActiveMineCount"), max2(c, prev));
        }
    } else if let Some(s) = src
        && !secondary
    {
        let gi = gem_mut(tab, s);
        gi.skill_mine_count_calcs = None;
        gi.skill_mine_count = None;
    }

    // :634-658 stages
    let no_potential_stage = !act.parts.as_ref().is_some_and(|ps| ps.iter().any(|p| p.stages == Some(true)));
    let base = strip_space(&act.name);
    let max_name = format!("Multiplier:{base}MaxStages");
    guard(&malformed, &[&max_name])?;
    let limit = lua(ml.borrow().sum(ModType::Base, Some(&cfg), &[&max_name]))?;
    set_data(
        &mut skill.skill_data,
        "stagesMax",
        if limit > 0.0 { Value::Num(limit) } else { Value::Nil },
    );
    if limit > 0.0 {
        skill.skill_flags.insert("multiStage".into(), true);
        let s = src.ok_or_else(|| src_nil(649))?;
        let min_name = format!("Multiplier:{base}MinimumStage");
        guard(&malformed, &[&min_name])?;
        let min_stage = lua(ml.borrow().sum(ModType::Base, Some(&cfg), &[&min_name]))?;
        let gi = &tab.socket_group_list()[s.0].gem_list[s.1];
        let first = if inp.calcs {
            gi.skill_stage_count_calcs
        } else {
            gi.skill_stage_count
        }
        .or_else(|| skill.skill_data.get("stagesMax").and_then(Value::as_num))
        .unwrap_or(1.0);
        let count = max2(first, 1.0 + min_stage);
        skill.active_stage_count = Some(count);
        if count > 0.0 {
            add_mod(
                &ml,
                &format!("Multiplier:{base}Stage"),
                ModType::Base,
                Value::Num(min2(limit, count)),
                "Base",
            );
            let count = count - 1.0;
            skill.active_stage_count = Some(count);
            add_mod(
                &ml,
                &format!("Multiplier:{base}StageAfterFirst"),
                ModType::Base,
                Value::Num(min2(limit - 1.0, count)),
                "Base",
            );
        }
    } else if no_potential_stage
        && let Some(s) = src
        && !secondary
    {
        let gi = gem_mut(tab, s);
        gi.skill_stage_count_calcs = None;
        gi.skill_stage_count = None;
    }

    // :660-666 skill data (env.modDB, then the skill list -- which reads through to env.modDB again)
    for v in lua(inp.mod_db.borrow().list(Some(&cfg), &["SkillData"]))? {
        let key = v.get("key");
        let key = key
            .as_str()
            .unwrap_or_else(|| unimplemented!("{CAS}:662 SkillData with a non-string key"));
        set_data(&mut skill.skill_data, key, v.get("value"));
    }
    guard(&malformed, &["SkillData"])?;
    for v in lua(ml.borrow().list(Some(&cfg), &["SkillData"]))? {
        let key = v.get("key");
        let key = key
            .as_str()
            .unwrap_or_else(|| unimplemented!("{CAS}:665 SkillData with a non-string key"));
        set_data(&mut skill.skill_data, key, v.get("value"));
    }

    // :668-802 minion
    let mut is_spectre = false;
    let mut minion_list: Vec<String> = match &act.minion_list {
        Some(l) if !l.is_empty() => l.clone(),
        Some(_) => {
            is_spectre = true;
            inp.spectre_list.to_vec()
        }
        None => Vec::new(),
    };
    let mut minion_support_level: IndexMap<String, Value> = IndexMap::new();
    for e in &skill.effect_list {
        let (g, l) = {
            let b = e.borrow();
            (b.granted_effect, b.level)
        };
        let ge = &sk.effects[g];
        if ge.support
            && let Some(add) = &ge.add_minion_list
        {
            for t in add {
                if !minion_list.contains(t) {
                    let req = l
                        .and_then(|l| ge.level(l))
                        .ok_or_else(|| format!("{CAS}:685: attempt to index a nil value (levels[skillEffect.level])"))?
                        .level_requirement;
                    minion_support_level.insert(t.clone(), num(req));
                    minion_list.push(t.clone());
                }
            }
        }
    }
    if !minion_list.is_empty() && actor.minion.is_none() {
        // `not activeSkill.actor.minionData` -- the player
        let s = src.ok_or_else(|| src_nil(if actor.calcs_main { 695 } else { 699 }))?;
        let gi = gem_mut(tab, s);
        let cur = if actor.calcs_main {
            &gi.skill_minion_calcs
        } else {
            &gi.skill_minion
        };
        let index = cur.as_ref().and_then(|m| minion_list.iter().position(|x| x == m)).unwrap_or(0);
        let minion_type = minion_list[index].clone();
        if actor.calcs_main {
            gi.skill_minion_calcs = Some(minion_type.clone());
        } else {
            gi.skill_minion = Some(minion_type.clone());
        }
        skill.skill_flags.insert("haveMinion".into(), true);
        let data = minion_data(&minion_type);
        // `minion.minionData and minion.minionData.hostile or false`
        let hostile = data.get("hostile").truthy();
        let sd = |k: &str| skill.skill_data.get(k).cloned().unwrap_or_default();
        let level_req = || num(level.level_requirement);
        let lvl = if sd("minionLevelIsEnemyLevel").truthy() {
            Value::Num(inp.enemy_level)
        } else if sd("minionLevelIsPlayerLevel").truthy() {
            let cap = sd("minionLevelIsPlayerLevel")
                .as_num()
                .ok_or_else(|| format!("{CAS}:718: bad argument #2 to 'min' (number expected)"))?;
            Value::Num(min2(inp.character_level, cap))
        } else {
            or(
                minion_support_level.get(&minion_type).cloned().unwrap_or_default(),
                or(sd("minionLevel"), level_req()),
            )
        };
        let lvl = lvl
            .as_num()
            .ok_or_else(|| format!("{CAS}:721: bad argument #1 to 'max' (number expected)"))?;
        let lvl = min2(max2(lvl, 1.0), 100.0);
        if data.is_nil() {
            return Err(format!("{CAS}:724: attempt to index field 'minionData' (a nil value)"));
        }
        let md_num = |k: &str, line: u32| -> Result<f64, LuaError> {
            data.get(k)
                .as_num()
                .ok_or_else(|| format!("{CAS}:{line}: attempt to perform arithmetic on field '{k}' (a nil value)"))
        };
        // :724-728 life table
        let life_table = if hostile {
            LifeTable::Monster
        } else if data.get("lifeScaling").is_str("AltLife1") {
            LifeTable::Monster2
        } else if data.get("lifeScaling").is_str("AltLife2") {
            LifeTable::Monster3
        } else if is_spectre {
            LifeTable::Monster
        } else {
            LifeTable::Ally
        };
        // :729-734 base damage
        let attack_time = md_num("attackTime", 729)?;
        let c = pob_data::constants::constants();
        let damage_table = if is_spectre || hostile {
            &c.monster_damage_table
        } else {
            &c.monster_ally_damage_table
        };
        let mut damage = pob_data::constants::monster_table_at(damage_table, lvl)
            .ok_or_else(|| format!("{CAS}:731: attempt to perform arithmetic on a nil value (damageTable[{lvl}])"))?
            * md_num("damage", 731)?;
        if !data.get("baseDamageIgnoresAttackSpeed").truthy() {
            damage *= attack_time;
        }
        // :735-750 item set (MAIN: `srcInstance.skillMinionItemSet`)
        let mut item_set: Option<usize> = None;
        if act.minion_has_item_set {
            let gi = gem_mut(tab, s);
            let cur = if actor.calcs_main {
                gi.skill_minion_item_set_calcs
            } else {
                gi.skill_minion_item_set
            };
            let found = cur.and_then(|id| inp.item_sets.iter().position(|x| x.id == id));
            item_set = match found {
                Some(k) => Some(k),
                None => {
                    // `srcInstance.skillMinionItemSet[Calcs] = itemSetOrderList[1]`; `itemSets[nil]` is nil
                    let first = inp.item_sets.first().map(|x| x.id);
                    if actor.calcs_main {
                        gi.skill_minion_item_set_calcs = first;
                    } else {
                        gi.skill_minion_item_set = first;
                    }
                    if inp.item_sets.is_empty() { None } else { Some(0) }
                }
            };
        } else if !secondary {
            let gi = gem_mut(tab, s);
            gi.skill_minion_item_set_calcs = None;
            gi.skill_minion_item_set = None;
        }
        // :751-765 weapon 1
        let player_w1 = inp.weapon_data1;
        let mut w1 = if (sd("minionUseBowAndQuiver").truthy() && player_w1.get("type").is_str("Bow"))
            || sd("minionUseMainHandWeapon").truthy()
            || (inp.the_iron_mass && minion_type == "RaisedSkeleton")
        {
            player_w1.clone()
        } else {
            let spread = md_num("damageSpread", 760)?;
            let mut d = DataTable::default();
            d.set("type", or(data.get("weaponType1"), Value::str("None")));
            d.set("AttackRate", Value::Num(1.0 / attack_time));
            d.set("CritChance", Value::Num(5.0));
            d.set("PhysicalMin", Value::Num(pob_lua::number::round(damage * (1.0 - spread), None)));
            d.set("PhysicalMax", Value::Num(pob_lua::number::round(damage * (1.0 + spread), None)));
            d.set("range", data.get("attackRange"));
            d
        };
        // :766-787 the slots the minion uses
        let mut w2 = DataTable::default();
        let uses: Option<Vec<String>> = act
            .minion_uses
            .as_ref()
            .map(|u| u.0.iter().filter(|(_, v)| **v).map(|(k, _)| k.clone()).collect());
        if let Some(u) = &uses {
            let set_weapon = |slot: &str, n: f64| -> Result<Option<Option<DataTable>>, LuaError> {
                let Some(k) = item_set else { return Ok(None) };
                let set = &inp.item_sets[k];
                let name = if set.use_second_weapon_set {
                    format!("{slot} Swap")
                } else {
                    slot.to_owned()
                };
                let sel = set.slots.get(&name).map_or(0.0, |x| x.item_id);
                let item = (sel >= 0.0 && sel.fract() == 0.0).then(|| inp.items.get(&(sel as u64))).flatten();
                Ok(Some(
                    item.and_then(|it| it.weapon_data.as_ref())
                        .and_then(|wd| wd.get(&pob_items::NumKey(n)).cloned()),
                ))
            };
            if u.iter().any(|x| x == "Weapon 1") {
                match set_weapon("Weapon 1", 1.0)? {
                    Some(Some(d)) => w1 = d,
                    Some(None) => {}
                    None => w1 = player_w1.clone(),
                }
            }
            if u.iter().any(|x| x == "Weapon 2") {
                match set_weapon("Weapon 2", 2.0)? {
                    Some(Some(d)) => w2 = d,
                    Some(None) => {}
                    None => w2 = inp.weapon_data2.clone(),
                }
            }
        }
        // :788-793 NonSpectreMinionsUseParentMainHandAttackTime
        if !is_spectre {
            guard(&malformed, &["NonSpectreMinionsUseParentMainHandAttackTime"])?;
            if lua(ml.borrow().flag(Some(&cfg), &["NonSpectreMinionsUseParentMainHandAttackTime"]))? == Some(true)
                && let Some(rate) = player_w1.get("AttackRate").as_num()
                && rate > 0.0
            {
                w1.set("AttackRate", Value::Num(rate));
            }
        }
        skill.minion = Some(Box::new(Minion {
            ty: minion_type.clone(),
            data,
            hostile,
            level: lvl,
            item_list: IndexMap::new(),
            uses,
            life_table,
            weapon_data1: w1,
            weapon_data2: w2,
            item_set,
            actor: None,
            active_skill_list: Vec::new(),
            main_skill: None,
        }));
    } else if let Some(s) = src
        && !secondary
    {
        let gi = gem_mut(tab, s);
        gi.skill_minion_calcs = None;
        gi.skill_minion = None;
        gi.skill_minion_item_set_calcs = None;
        gi.skill_minion_item_set = None;
        gi.skill_minion_skill = None;
        gi.skill_minion_skill_calcs = None;
    }
    skill.minion_list = Some(minion_list);

    // :804-869 separate the global effect modifiers into buffs
    if malformed.iter().any(|m| m.process_mod_error().is_some()) {
        // `for _, tag in ipairs(skillModList[i])` reaches a number
        return Err(format!("{CAS}:809: attempt to index local 'tag' (a number value)"));
    }
    let mut i = 0;
    loop {
        let m = match &ml.borrow().kind {
            StoreKind::List(l) => l.mods.get(i).cloned(),
            StoreKind::Db(_) => unreachable!("a ModList"),
        };
        let Some(m) = m else { break };
        let Some(tag) = m.tags.iter().find(|t| t.ty().is_str("GlobalEffect")) else {
            i += 1;
            continue;
        };
        let effect_type = tag.get("effectType");
        let effect_name = or(tag.get("effectName"), Value::str(&act.name));
        let mod_cond = tag.get("modCond");
        let remove = |ml: &StoreRef, i: usize| {
            if let StoreKind::List(l) = &mut ml.borrow_mut().kind {
                l.mods.remove(i);
            }
        };
        if mod_cond.truthy() {
            let var = mod_cond
                .as_str()
                .unwrap_or_else(|| unimplemented!("{CAS}:816 GlobalEffect modCond that is not a string"));
            if !lua(ml.borrow().get_condition(var, Some(&cfg), false))?.unwrap_or(false) {
                remove(&ml, i);
                continue;
            }
        }
        if !effect_type.truthy() {
            i += 1;
            continue;
        }
        let pos = skill
            .buff_list
            .iter()
            .position(|b| b.ty.raw_eq(&effect_type) && b.name.raw_eq(&effect_name));
        let pos = match pos {
            Some(p) => p,
            None => {
                let mut buff = Buff {
                    ty: effect_type.clone(),
                    name: effect_name.clone(),
                    allow_totem_buff: tag.get("allowTotemBuff"),
                    cond: tag.get("effectCond"),
                    enemy_cond: tag.get("effectEnemyCond"),
                    stack_var: tag.get("effectStackVar"),
                    stack_limit: tag.get("effectStackLimit"),
                    stack_limit_var: tag.get("effectStackLimitVar"),
                    apply_not_player: tag.get("applyNotPlayer"),
                    apply_minions: tag.get("applyMinions"),
                    ..Buff::default()
                };
                if m.source.as_deref() == Some(act.mod_source.as_str()) {
                    // inherit the buff configuration from the active skill
                    let sd = |k: &str| skill.skill_data.get(k).cloned().unwrap_or_default();
                    buff.active_skill_buff = Some(true);
                    buff.apply_not_player = or(buff.apply_not_player, sd("buffNotPlayer"));
                    buff.apply_minions = or(buff.apply_minions, sd("buffMinions"));
                    buff.apply_allies = sd("buffAllies");
                    buff.allow_totem_buff = sd("allowTotemBuff");
                }
                skill.buff_list.push(buff);
                skill.buff_list.len() - 1
            }
        };
        let list = &mut skill.buff_list[pos].mod_list;
        let mut matched = false;
        for slot in list.iter_mut() {
            let dest = &*slot;
            if compare_mod_params(&m, dest) && matches!(dest.ty, Some(ModType::Base | ModType::Inc)) {
                let mut n = dest.deep_copy();
                n.value = match (&dest.value, &m.value) {
                    (Value::Num(a), Value::Num(b)) => Value::Num(a + b),
                    (a, b) => {
                        return Err(format!(
                            "{CAS}:856: attempt to perform arithmetic on a {} value ({})",
                            if matches!(a, Value::Num(_)) { b.type_name() } else { a.type_name() },
                            m.name
                        ));
                    }
                };
                *slot = Rc::new(n);
                matched = true;
                break;
            }
        }
        if !matched {
            list.push(m.clone());
        }
        remove(&ml, i);
    }

    skill.malformed_mods = malformed;
    Ok(!skill.buff_list.is_empty())
}
