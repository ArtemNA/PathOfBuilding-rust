//! CalcOffence.lua:70-347 -- the file-level helpers of the offence module (line numbers: `grep -n` on upstream `ed354c2f8`, 2026-10-31).
//!
//! `calcDamage` (:70) / `calcAilmentSourceDamage` (:143) read `activeSkill.conversionTable` (built at :1989, R7.4) -- ported here with the
//! table type ([`ConversionTable`]) and first called from R7.4/R7.5. `calcRadiusBreakpoints` (:202) and
//! `setMoltenStrikeTertiaryRadiusBreakdown` (:241) only fill breakdown tables (R8): not ported.

use indexmap::IndexMap;
use pob_data::constants::{constants, monster_table_at};
use pob_data::skill_stat_map::MalformedMod;
use pob_lua::number::{max2, min2, round};
use pob_lua::tostring_num;
use pob_mods::{ModCfg, ModType, StoreRef, Value};

use crate::calc_lib;
use crate::perform::skill_guard;
use crate::setup::{misc, ok};

/// `dmgTypeList` (:35) -- the conversion order.
pub(crate) const DMG_TYPE_LIST: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];

/// `dmgTypeFlags` (:36-46): the order of `damageStatsForTypes` and the bits.
const DMG_TYPE_FLAGS: [(&str, u32); 6] = [
    ("Physical", 0x01),
    ("Lightning", 0x02),
    ("Cold", 0x04),
    ("Fire", 0x08),
    ("Elemental", 0x0E),
    ("Chaos", 0x10),
];

/// `damageStatsForTypes[k]` (:54-65): `{ "Damage", "<Type>Damage"... }` for the bits of `k`, in `dmgTypeFlags.order`.
pub(crate) fn damage_stats_for_types(k: u32) -> Vec<String> {
    let mut names = vec!["Damage".to_owned()];
    for (ty, flag) in DMG_TYPE_FLAGS {
        if k & flag != 0 {
            names.push(format!("{ty}Damage"));
        }
    }
    names
}

/// A skill list query behind the malformed-mod guard (R6.4): a query that reaches a malformed mod's name raises its LuaJIT error.
#[derive(Clone)]
pub(crate) struct Q {
    pub list: StoreRef,
    pub malformed: Vec<MalformedMod>,
}

impl Q {
    pub fn sum(&self, ty: ModType, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
        skill_guard(&self.malformed, names);
        ok(self.list.borrow().sum(ty, cfg, names))
    }
    pub fn base(&self, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
        self.sum(ModType::Base, cfg, names)
    }
    pub fn inc(&self, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
        self.sum(ModType::Inc, cfg, names)
    }
    pub fn more(&self, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
        skill_guard(&self.malformed, names);
        ok(self.list.borrow().more(cfg, names))
    }
    /// `Flag(cfg, ...)` truthy.
    pub fn flag(&self, cfg: Option<&ModCfg>, names: &[&str]) -> bool {
        skill_guard(&self.malformed, names);
        ok(self.list.borrow().flag(cfg, names)) == Some(true)
    }
    pub fn override_(&self, cfg: Option<&ModCfg>, names: &[&str]) -> Option<Value> {
        skill_guard(&self.malformed, names);
        ok(self.list.borrow().override_(cfg, names))
    }
    /// `Override(cfg, name)` used as a number in `x or y` (nil / false -> `None`).
    pub fn override_num(&self, cfg: Option<&ModCfg>, names: &[&str], site: &str) -> Option<f64> {
        match self.override_(cfg, names) {
            Some(v) if v.truthy() => Some(
                v.as_num()
                    .unwrap_or_else(|| panic!("{site}: attempt to perform arithmetic on a {} value", v.type_name())),
            ),
            _ => None,
        }
    }
    pub fn max(&self, cfg: Option<&ModCfg>, names: &[&str]) -> Option<f64> {
        skill_guard(&self.malformed, names);
        ok(self.list.borrow().max(cfg, names))
    }
    pub fn list(&self, cfg: Option<&ModCfg>, names: &[&str]) -> Vec<Value> {
        skill_guard(&self.malformed, names);
        ok(self.list.borrow().list(cfg, names))
    }
    pub fn tabulate(&self, ty: Option<ModType>, cfg: Option<&ModCfg>, names: &[&str]) -> Vec<pob_mods::TabEntry> {
        skill_guard(&self.malformed, names);
        ok(self.list.borrow().tabulate(ty, cfg, names))
    }
    /// `calcLib.mod(skillModList, cfg, ...)`.
    pub fn calc_mod(&self, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
        skill_guard(&self.malformed, names);
        ok(calc_lib::mod_(&self.list.borrow(), cfg, names))
    }
    /// `calcLib.mods(skillModList, cfg, ...)`.
    pub fn calc_mods(&self, cfg: Option<&ModCfg>, names: &[&str]) -> (f64, f64) {
        skill_guard(&self.malformed, names);
        ok(calc_lib::mods(&self.list.borrow(), cfg, names))
    }
    /// `calcLib.val(skillModList, name, cfg)`.
    pub fn calc_val(&self, name: &str, cfg: Option<&ModCfg>) -> f64 {
        skill_guard(&self.malformed, &[name]);
        ok(calc_lib::val(&self.list.borrow(), name, cfg))
    }
    /// `NewMod(name, type, value, source, flags, keywordFlags, tags...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new_mod(
        &self,
        name: &str,
        ty: Option<ModType>,
        value: Value,
        source: Option<&str>,
        flags: u32,
        kw: u32,
        tags: Vec<pob_mods::Tag>,
    ) {
        self.list
            .borrow_mut()
            .new_mod(pob_mods::create_mod(name, ty, value, source, flags, kw, tags));
    }
    /// `NewMod` with a number value and a type.
    #[allow(clippy::too_many_arguments)]
    pub fn nm(&self, name: &str, ty: ModType, value: f64, source: &str, flags: u32, kw: u32, tags: Vec<pob_mods::Tag>) {
        self.new_mod(name, Some(ty), Value::Num(value), Some(source), flags, kw, tags);
    }
    /// `ReplaceMod(name, type, value, source, flags)`.
    pub fn replace_mod(&self, name: &str, ty: ModType, value: f64, source: &str, flags: u32) {
        self.list.borrow_mut().replace_mod(pob_mods::create_mod(
            name,
            Some(ty),
            Value::Num(value),
            Some(source),
            flags,
            0,
            vec![],
        ));
    }
}

/// Common.lua:733 `roundSymmetric(val)` (no `dec`).
pub(crate) fn round_symmetric(val: f64) -> f64 {
    if val >= 0.0 { (val + 0.5).floor() } else { (val - 0.5).ceil() }
}

/// :160-166 `calcAreaMod(incArea, moreArea)`.
pub(crate) fn calc_area_mod(inc_area: f64, more_area: f64) -> f64 {
    let base_area_percent = round_symmetric(inc_area * 100.0);
    let final_area_percent = round_symmetric(max2(0.0, more_area) * 100.0);
    let mut area_percent = base_area_percent * final_area_percent / 100.0;
    area_percent = if area_percent >= 0.0 {
        area_percent.floor()
    } else {
        area_percent.ceil()
    };
    max2(0.0, area_percent) / 100.0
}

/// :173-178 `calcRadius(baseRadius, areaMod, radiusMod)`.
pub(crate) fn calc_radius(base_radius: f64, area_mod: f64, radius_mod: Option<f64>) -> f64 {
    let mut radius_percent = (100.0 * area_mod.sqrt()).floor();
    radius_percent = (radius_percent * radius_mod.unwrap_or(1.0)).floor();
    (base_radius * radius_percent / 100.0).floor()
}

/// :185-190 `calcMoltenStrikeTertiaryRadius(baseRadius, deadzoneRadius, areaMod, speedMod)`.
pub(crate) fn calc_molten_strike_tertiary_radius(base_radius: f64, deadzone_radius: f64, area_mod: f64, speed_mod: f64) -> f64 {
    let max_dist_ignoring_speed = (base_radius * base_radius * area_mod - deadzone_radius * deadzone_radius * (area_mod - 1.0)).sqrt();
    ((max_dist_ignoring_speed - deadzone_radius) * speed_mod + deadzone_radius).floor()
}

/// :296-309 `calcSkillCooldown(skillModList, skillCfg, skillData)` -> `(cooldown, rounded, addedCooldown)`; `addedCooldown` is
/// returned only on the rounded path (nil otherwise).
pub(crate) fn calc_skill_cooldown(q: &Q, cfg: &ModCfg, skill_data: &IndexMap<String, Value>) -> (f64, bool, Option<f64>) {
    let site = "Modules/CalcOffence.lua:299";
    let cooldown_override = q.override_num(Some(cfg), &["CooldownRecovery"], site);
    let added_cooldown = q.base(Some(cfg), &["CooldownRecovery"]);
    let data_cd = sd_num_or(skill_data, "cooldown", 0.0, site);
    let cooldown =
        cooldown_override.unwrap_or_else(|| (data_cd + added_cooldown) / max2(0.0, q.calc_mod(Some(cfg), &["CooldownRecovery"])));
    let gt1 = |k: &str| match skill_data.get(k) {
        Some(Value::Num(v)) => *v > 1.0,
        Some(v) if v.truthy() => panic!("Modules/CalcOffence.lua:302: attempt to compare number with {}", v.type_name()),
        _ => false,
    };
    if gt1("storedUses") || gt1("VaalStoredUses") || q.base(Some(cfg), &["AdditionalCooldownUses"]) > 0.0 {
        (cooldown, false, None)
    } else {
        let tick = misc("ServerTickRate");
        ((cooldown * tick).ceil() / tick, true, Some(added_cooldown))
    }
}

/// :311-320 `calcWarcryCastTime(skillModList, skillCfg, skillData, actor)`; `action_speed_mod` = `calcs.actionSpeedMod(actor)`.
pub(crate) fn calc_warcry_cast_time(q: &Q, cfg: &ModCfg, skill_data: &IndexMap<String, Value>, action_speed_mod: f64) -> f64 {
    let base_speed = 1.0 / q.base(Some(cfg), &["WarcryCastTime"]);
    let mut t = base_speed * q.calc_mod(Some(cfg), &["WarcrySpeed"]) * action_speed_mod;
    t = min2(t, misc("ServerTickRate"));
    t = 1.0 / t;
    if q.flag(Some(cfg), &["InstantWarcry"]) || skill_data.get("triggeredByAutoexertion").is_some_and(Value::truthy) {
        t = 0.0;
    }
    t
}

/// :322-336 `calcSkillDuration(skillModList, skillCfg, skillData, env, enemyDB)`; `mode_effective` = `env.mode_effective`.
pub(crate) fn calc_skill_duration(
    list: &StoreRef,
    malformed: &[MalformedMod],
    cfg: &ModCfg,
    skill_data: &IndexMap<String, Value>,
    mode_effective: bool,
    enemy_db: &StoreRef,
) -> f64 {
    let truthy = |k: &str| skill_data.get(k).is_some_and(Value::truthy);
    let mut names = vec!["Duration", "PrimaryDuration"];
    if truthy("mineDurationAppliesToSkill") {
        names.push("MineDuration");
    }
    skill_guard(malformed, &names);
    let duration_mod = max2(ok(calc_lib::mod_(&list.borrow(), Some(cfg), &names)), 0.0);
    let data_duration = sd_num_or(skill_data, "duration", 0.0, "Modules/CalcOffence.lua:325");
    skill_guard(malformed, &["Duration", "PrimaryDuration"]);
    let duration_base = data_duration + ok(list.borrow().sum(ModType::Base, Some(cfg), &["Duration", "PrimaryDuration"]));
    let mut duration = duration_base * duration_mod;
    let mut debuff_duration_mult = 1.0;
    if mode_effective {
        debuff_duration_mult = 1.0
            / max2(
                misc("BuffExpirationSlowCap"),
                ok(calc_lib::mod_(&enemy_db.borrow(), Some(cfg), &["BuffExpireFaster"])),
            );
    }
    if truthy("debuff") {
        duration *= debuff_duration_mult;
    }
    duration
}

/// :338-342 `calcs.calcTotemLife(env, activeSkill)` -> `(life, lifeMod)`; `level` = `skillData.totemLevel`, `totem_id` =
/// `activeSkill.skillTotemId`.
pub(crate) fn calc_totem_life(q: &Q, cfg: &ModCfg, level: f64, totem_id: Option<f64>) -> (f64, f64) {
    let life_mod = q.calc_mod(Some(cfg), &["TotemLife"]);
    let c = constants();
    let nil = || -> ! { panic!("Modules/CalcOffence.lua:340: attempt to perform arithmetic on a nil value (field '?')") };
    let ally_life = monster_table_at(&c.monster_ally_life_table, level).unwrap_or_else(|| nil());
    let mult = *totem_id
        .and_then(|t| c.totem_life_mult.0.get(&tostring_num(t)))
        .unwrap_or_else(|| nil());
    (round((ally_life * mult).floor() * life_mod, None), life_mod)
}

/// `skillData[k] or fallback` used as a number: a truthy non-number is Lua's arithmetic error.
pub(crate) fn sd_num_or(skill_data: &IndexMap<String, Value>, k: &str, fallback: f64, site: &str) -> f64 {
    match skill_data.get(k) {
        Some(v) if v.truthy() => v
            .as_num()
            .unwrap_or_else(|| panic!("{site}: attempt to perform arithmetic on field '{k}' (a {} value)", v.type_name())),
        _ => fallback,
    }
}

/// `activeSkill.conversionTable` / `cfg.conversionTable` (built by `buildConversionTable`, :1938-1987, R7.4): per source damage type,
/// `conversion[dst]`, `gain[dst]`, the total `[dst]` and `mult`; Chaos has `mult` only.
#[derive(Clone, Debug, Default)]
pub(crate) struct ConvEntry {
    pub conversion: IndexMap<&'static str, f64>,
    pub gain: IndexMap<&'static str, f64>,
    pub total: IndexMap<&'static str, f64>,
    pub mult: f64,
}

pub(crate) type ConversionTable = IndexMap<&'static str, ConvEntry>;

/// :70-141 `calcDamage(activeSkill, output, cfg, breakdown, damageType, typeFlags, convDst)` without the breakdown rows (R8). `base`
/// reads `output[<type>MinBase]` / `output[<type>MaxBase]`; `cfg` is the pass cfg (the conversion table is passed resolved).
pub(crate) fn calc_damage(
    q: &Q,
    conv: &ConversionTable,
    base: &dyn Fn(&str) -> (f64, f64),
    cfg: Option<&ModCfg>,
    damage_type: &'static str,
    type_flags: u32,
    conv_dst: Option<&'static str>,
) -> (f64, f64) {
    let flag = DMG_TYPE_FLAGS.iter().find(|(t, _)| *t == damage_type).map_or(0, |(_, f)| *f);
    let type_flags = type_flags | flag;
    let (mut add_min, mut add_max) = (0.0, 0.0);
    for other in DMG_TYPE_LIST {
        if other == damage_type {
            break;
        }
        let conv_mult = conv
            .get(other)
            .and_then(|e| e.total.get(damage_type))
            .copied()
            .unwrap_or_else(|| panic!("Modules/CalcOffence.lua:82: attempt to compare nil with number"));
        if conv_mult > 0.0 {
            let (min, max) = calc_damage(q, conv, base, cfg, other, type_flags, Some(damage_type));
            add_min += min * conv_mult;
            add_max += max * conv_mult;
        }
    }
    let (base_min, base_max) = base(damage_type);
    if base_min == 0.0 && base_max == 0.0 {
        if conv_dst.is_some() {
            return (add_min, add_max);
        }
        return (round(add_min, None), round(add_max, None));
    }
    let names_owned = damage_stats_for_types(type_flags);
    let names: Vec<&str> = names_owned.iter().map(String::as_str).collect();
    let inc = 1.0 + q.inc(cfg, &names) / 100.0;
    let more = q.more(cfg, &names);
    let generic_more_min = q.more(cfg, &["MinDamage"]);
    let generic_more_max = q.more(cfg, &["MaxDamage"]);
    let (min_n, max_n) = (format!("Min{damage_type}Damage"), format!("Max{damage_type}Damage"));
    let more_min = q.more(cfg, &[&min_n]);
    let more_max = q.more(cfg, &[&max_n]);
    let inc_min = 1.0 + q.inc(cfg, &[&min_n]) / 100.0;
    let inc_max = 1.0 + q.inc(cfg, &[&max_n]) / 100.0;
    let min = ((base_min * inc * more) * generic_more_min + add_min) * more_min * inc_min;
    let max = ((base_max * inc * more) * generic_more_max + add_max) * more_max * inc_max;
    if conv_dst.is_some() {
        return (min, max);
    }
    (round(min, None), round(max, None))
}

/// :143-153 `calcAilmentSourceDamage` without the breakdown rows (R8).
#[allow(dead_code)] // first caller: R7.6 (ailments)
pub(crate) fn calc_ailment_source_damage(
    q: &Q,
    conv: &ConversionTable,
    base: &dyn Fn(&str) -> (f64, f64),
    cfg: Option<&ModCfg>,
    damage_type: &'static str,
    type_flags: u32,
) -> (f64, f64) {
    let (min, max) = calc_damage(q, conv, base, cfg, damage_type, type_flags, None);
    let conv_mult = conv
        .get(damage_type)
        .map(|e| e.mult)
        .unwrap_or_else(|| panic!("Modules/CalcOffence.lua:146: attempt to index a nil value"));
    (min * conv_mult, max * conv_mult)
}
