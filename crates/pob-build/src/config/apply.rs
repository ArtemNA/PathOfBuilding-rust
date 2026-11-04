//! The `apply` closures of Modules/ConfigOptions.lua that stay by hand (`enemyIsBoss`, `presetBossSkills`: they read `data.*` and
//! `build.configTab.*`, outside the generator's grammar), the 8 map-affix dropdowns (R3.5b, with the 41 `AffixData[x].apply` of
//! Data/ModMap.lua), the runtime the generated closures call (`apply_gen.rs`, `tools/config-apply-gen/gen.ts`), and the
//! merged [`APPLY_REGISTRY`] (all 539, varList order). Truth for all 539: LuaJIT probes `tools/lua-corpus/export-config-apply.ts`
//! (test `config_apply_luajit.rs`). Each entry carries `ConfigOptions.lua:<from>-<to>` and the sha256 of the upstream
//! body as `data/config-options.json` recorded it; `registry_drift` (test `config_registry.rs`) turns a changed body red.
//!
//! Translation rules (mechanical):
//! * `modList:NewMod(name, type, value, "Config", tag1, tag2)` -> `create_mod(.., Some("Config"), 0, 0, [tag1, tag2])`:
//!   ModTools.lua:33-67 decodes the vararg by type -- slot 1 a string = source, slots 2/3 numbers = flags/keywordFlags,
//!   the rest tags (MOD-CORE §1). No call site here passes flags, so both are 0.
//! * Every Lua `{ ... }` literal is a fresh table per call -> a fresh `Tag` per mod (L14, MINEFIELD-RUST R4).
//! * `m_min`/`m_max`/`round`/`tostring` -> `pob_lua`; `x or y` / `if x` -> Lua truthiness (`0` and `""` are true, L4).
//! * `varControls[v]:SetPlaceholder(x, true)` -> `ConfigState::set_placeholder`; `configTab:UpdateLevel()` ->
//!   `ConfigState::update_level`. Arithmetic on a `val` coerces a numeric string and raises otherwise (L3, L18).

use std::rc::Rc;
use std::sync::{LazyLock, OnceLock};

use pob_data::constants::{constants, monster_table_at};
use pob_data::wire::json_num;
use pob_lua::number::{max2, min2, round};
use pob_lua::{tonumber, tostring_num};
use pob_mods::{LuaError, LuaResult, ModList, ModType, Table, Tag, Value, create_mod};
use serde_json::Value as Json;

use super::{ConfigState, ConfigValue, lua_tostring, truthy};
use crate::model::InputValue;

/// The chunk name LuaJIT reports for ConfigOptions.lua (R3.8a: `./Modules/ConfigOptions.lua:2276: ...`).
const CO: &str = "./Modules/ConfigOptions.lua";
const CONFIG: &str = "Config";

/// What an `apply(val, modList, enemyModList, build)` closure reaches: the two lists and `build.configTab`.
pub struct ApplyCtx<'a> {
    pub mod_list: &'a mut ModList,
    pub enemy_mod_list: &'a mut ModList,
    /// `build.configTab` (state + controls).
    pub tab: &'a mut ConfigState,
    /// `build.characterLevel` (read by `UpdateLevel`).
    pub character_level: f64,
    pub warnings: &'a mut Vec<String>,
}

impl ApplyCtx<'_> {
    /// `build.configTab.varControls[var]:SetPlaceholder(v, true)`.
    fn ph(&mut self, var: &str, v: ConfigValue) {
        self.tab.set_placeholder(var, &v, self.warnings);
    }

    fn update_level(&mut self) -> LuaResult<()> {
        self.tab.update_level(self.character_level)
    }
}

pub type ApplyFn = fn(&ConfigValue, &mut ApplyCtx<'_>) -> LuaResult<()>;

/// One ported closure: the var, where its body is upstream, and the sha256 of that body.
#[derive(Clone, Copy)]
pub struct ApplyEntry {
    pub var: &'static str,
    pub from: u32,
    pub to: u32,
    pub sha: &'static str,
    pub apply: ApplyFn,
}

impl std::fmt::Debug for ApplyEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ApplyEntry({} {}:{}-{} {})", self.var, CO, self.from, self.to, self.sha)
    }
}

pub(super) const fn e(var: &'static str, from: u32, to: u32, sha: &'static str, apply: ApplyFn) -> ApplyEntry {
    ApplyEntry { var, from, to, sha, apply }
}

/// Every entry whose `from`/`to`/`sha` differ from `data/config-options.json`, or whose var has no `apply` there.
pub fn registry_drift(entries: &[ApplyEntry]) -> Vec<String> {
    let opts = pob_data::config_options::config_options();
    let mut out = Vec::new();
    for en in entries {
        match opts.var(en.var).and_then(|v| v.apply.as_ref()) {
            None => out.push(format!("{}: no `apply` in config-options.json", en.var)),
            Some(a) => {
                if a.sha != en.sha || a.from != f64::from(en.from) || a.to != f64::from(en.to) {
                    out.push(format!(
                        "{}: registry {}:{}-{} {} != json {}:{}-{} {}",
                        en.var, CO, en.from, en.to, en.sha, a.src, a.from, a.to, a.sha
                    ));
                }
            }
        }
    }
    out
}

// --- helpers ------------------------------------------------------------------------------------------------------------

pub(super) fn s(x: &str) -> Value {
    Value::str(x)
}

pub(super) fn table(pairs: &[(&str, Value)]) -> Table {
    let mut t = Table::default();
    for (k, v) in pairs {
        t.hash.insert(Rc::from(*k), v.clone());
    }
    t
}

fn tag(pairs: &[(&str, Value)]) -> Tag {
    Tag::new(table(pairs))
}

/// `{ type = "Condition", var = var }`.
pub(super) fn cond(var: &str) -> Tag {
    tag(&[("type", s("Condition")), ("var", s(var))])
}

/// `list:NewMod(name, type, value, source, tags...)` (ModStore.lua:107).
fn nm(list: &mut ModList, name: &str, ty: ModType, value: Value, source: &str, tags: Vec<Tag>) {
    list.mods.push(Rc::new(create_mod(name, Some(ty), value, Some(source), 0, 0, tags)));
}

/// `list:NewMod(name, "FLAG", true, source, tags...)`.
fn flag(list: &mut ModList, name: &str, source: &str, tags: Vec<Tag>) {
    nm(list, name, ModType::Flag, Value::Bool(true), source, tags);
}

/// `list:NewMod(name, "BASE", value, source, tags...)`.
fn base(list: &mut ModList, name: &str, value: f64, source: &str, tags: Vec<Tag>) {
    nm(list, name, ModType::Base, Value::Num(value), source, tags);
}

/// `val` in arithmetic: a number, or a string LuaJIT coerces (L3); anything else raises (L18).
fn n(v: Option<&ConfigValue>, site: u32) -> LuaResult<f64> {
    match v {
        Some(InputValue::Num(x)) => Ok(*x),
        Some(InputValue::Str(t)) => tonumber(t).ok_or_else(|| {
            LuaError(format!(
                "{CO}:{site}: attempt to perform arithmetic on local 'val' (a string value)"
            ))
        }),
        Some(InputValue::Bool(_)) => Err(LuaError(format!(
            "{CO}:{site}: attempt to perform arithmetic on local 'val' (a boolean value)"
        ))),
        None => Err(LuaError(format!("{CO}:{site}: attempt to perform arithmetic on a nil value"))),
    }
}

fn is(v: &ConfigValue, x: &str) -> bool {
    matches!(v, InputValue::Str(t) if t == x)
}

fn num(x: f64) -> ConfigValue {
    InputValue::Num(x)
}

fn empty() -> ConfigValue {
    InputValue::Str(String::new())
}

fn misc(k: &str) -> f64 {
    constants().misc[k]
}

fn boss_stat(k: &str) -> f64 {
    pob_data::rules::rules().boss_stats[k]
}

/// `data.monsterXTable[level]` -- nil out of range raises on the arithmetic that follows.
fn table_at(t: &[f64], level: f64, site: u32) -> LuaResult<f64> {
    monster_table_at(t, level).ok_or_else(|| LuaError(format!("{CO}:{site}: attempt to perform arithmetic on a nil value (field '?')")))
}

/// `data.bossSkills` (Data/BossSkills.lua) as converted: `data/boss-skills.json` `skills` (key order = the converter's
/// sorted order, which is the only `pairs` order observable here, D13).
fn boss_skills() -> &'static Json {
    static B: OnceLock<Json> = OnceLock::new();
    B.get_or_init(|| {
        let j: Json = pob_data::read_json("boss-skills.json").unwrap_or_else(|e| panic!("{e}"));
        j["skills"].clone()
    })
}

/// A JSON scalar of the boss data as a Lua value (`nil` for absent/null).
fn jval(j: Option<&Json>) -> Option<ConfigValue> {
    match j? {
        Json::String(t) => Some(InputValue::Str(t.clone())),
        Json::Bool(b) => Some(InputValue::Bool(*b)),
        Json::Null => None,
        other => json_num(other).map(InputValue::Num),
    }
}

fn jtruthy(j: Option<&Json>) -> bool {
    !matches!(j, None | Some(Json::Null) | Some(Json::Bool(false)))
}

// --- the closures -------------------------------------------------------------------------------------------------------

/// ConfigOptions.lua:2086-2222 `enemyIsBoss`: placeholder defaults per tier, written mid-BuildModList (CALC-ARCH §6).
fn enemy_is_boss(v: &ConfigValue, c: &mut ApplyCtx<'_>) -> LuaResult<()> {
    let k = constants();
    // These defaults are here so that the placeholders get reset correctly
    c.ph("enemySpeed", num(700.0));
    c.ph("enemyCritChance", num(5.0));
    c.ph(
        "enemyCritDamage",
        num(k.monster_constants["base_critical_strike_multiplier"] - 100.0),
    );
    let pens = ["enemyLightningPen", "enemyColdPen", "enemyFirePen"];
    let dmgs = ["enemyPhysicalDamage", "enemyLightningDamage", "enemyColdDamage", "enemyFireDamage"];
    let resists = |c: &mut ApplyCtx<'_>, ele: ConfigValue, chaos: ConfigValue| {
        c.ph("enemyLightningResist", ele.clone());
        c.ph("enemyColdResist", ele.clone());
        c.ph("enemyFireResist", ele);
        c.ph("enemyChaosResist", chaos);
    };
    if is(v, "None") {
        resists(c, empty(), empty());
        // `local defaultLevel = 83` ... `if build.configTab.enemyLevel then defaultLevel = build.configTab.enemyLevel end`
        // -- enemyLevel is always a number after UpdateLevel, so the 83 is dead.
        c.ph("enemyLevel", empty());
        c.update_level()?;
        let default_level = c.tab.enemy_level;
        let default_damage = round(table_at(&k.monster_damage_table, default_level, 2105)? * 1.5, None);
        c.ph("enemyPhysicalDamage", num(default_damage));
        c.ph("enemyLightningDamage", empty());
        c.ph("enemyColdDamage", empty());
        c.ph("enemyFireDamage", empty());
        c.ph("enemyChaosDamage", empty());
        c.ph("enemyPhysicalOverwhelm", empty());
        for p in pens {
            c.ph(p, empty());
        }
        c.ph("enemyArmour", num(table_at(&k.monster_armour_table, default_level, 2118)?));
        c.ph("enemyEvasion", num(table_at(&k.monster_evasion_table, default_level, 2119)?));
    } else if is(v, "Boss") {
        flag(c.enemy_mod_list, "Condition:RareOrUnique", CONFIG, vec![cond("Effective")]);
        nm(
            c.enemy_mod_list,
            "AilmentThreshold",
            ModType::More,
            Value::Num(488.0),
            "Boss",
            vec![],
        );
        base(c.mod_list, "WarcryPower", 20.0, "Boss", vec![]);
        base(c.mod_list, "Multiplier:EnemyPower", 20.0, "Boss", vec![]);
        resists(c, num(40.0), num(25.0));
        c.ph("enemyLevel", empty());
        c.update_level()?;
        let default_level = c.tab.enemy_level;
        let default_damage = round(
            table_at(&k.monster_damage_table, default_level, 2139)? * 1.5 * misc("stdBossDPSMult"),
            None,
        );
        for d in dmgs {
            c.ph(d, num(default_damage));
        }
        c.ph("enemyChaosDamage", num(round(default_damage / 2.5, None)));
        c.ph("enemyPhysicalOverwhelm", empty());
        for p in pens {
            c.ph(p, empty());
        }
        c.ph("enemyArmour", num(table_at(&k.monster_armour_table, default_level, 2152)?));
        c.ph("enemyEvasion", num(table_at(&k.monster_evasion_table, default_level, 2153)?));
    } else if is(v, "Pinnacle") || is(v, "Uber") {
        let uber = is(v, "Uber");
        flag(c.enemy_mod_list, "Condition:RareOrUnique", CONFIG, vec![cond("Effective")]);
        flag(c.enemy_mod_list, "Condition:PinnacleBoss", CONFIG, vec![cond("Effective")]);
        if uber {
            nm(c.enemy_mod_list, "DamageTaken", ModType::More, Value::Num(-70.0), "Boss", vec![]);
        }
        nm(
            c.enemy_mod_list,
            "AilmentThreshold",
            ModType::More,
            Value::Num(404.0),
            "Boss",
            vec![],
        );
        base(c.mod_list, "WarcryPower", 20.0, "Boss", vec![]);
        base(c.mod_list, "Multiplier:EnemyPower", 20.0, "Boss", vec![]);
        resists(c, num(50.0), num(30.0));
        let mut default_level = if uber { 85.0 } else { 84.0 };
        c.ph("enemyLevel", num(default_level));
        c.update_level()?;
        default_level = max2(c.tab.enemy_level, default_level);
        let (dps, chaos_div, pen, armour, evasion, site) = if uber {
            ("uberBossDPSMult", 4.0, "uberBossPen", "UberArmourMean", "UberEvasionMean", 2205)
        } else {
            (
                "pinnacleBossDPSMult",
                2.5,
                "pinnacleBossPen",
                "PinnacleArmourMean",
                "PinnacleEvasionMean",
                2172,
            )
        };
        let default_damage = round(table_at(&k.monster_damage_table, default_level, site)? * 1.5 * misc(dps), None);
        for d in dmgs {
            c.ph(d, num(default_damage));
        }
        c.ph("enemyChaosDamage", num(round(default_damage / chaos_div, None)));
        for p in pens {
            c.ph(p, num(misc(pen)));
        }
        let a = table_at(&k.monster_armour_table, default_level, site + 14)?;
        c.ph("enemyArmour", num(round(a * (boss_stat(armour) / 100.0), None)));
        let ev = table_at(&k.monster_evasion_table, default_level, site + 15)?;
        c.ph("enemyEvasion", num(round(ev * (boss_stat(evasion) / 100.0), None)));
    }
    Ok(())
}

/// ConfigOptions.lua:2272-2351 `presetBossSkills`.
fn preset_boss_skills(v: &ConfigValue, c: &mut ApplyCtx<'_>) -> LuaResult<()> {
    if !is(v, "None") {
        let name = lua_tostring(v);
        let Some(boss) = boss_skills().get(&name) else {
            return Err(LuaError(format!("{CO}:2276: attempt to index local 'bossData' (a nil value)")));
        };
        let sel = c.tab.list_value_at("enemyIsBoss", c.tab.sel_index("enemyIsBoss"));
        let mut is_uber = sel.as_ref().is_some_and(|x| is(x, "Uber"));
        if jtruthy(boss.get("earlierUber")) && sel.as_ref().is_some_and(|x| is(x, "Pinnacle")) {
            is_uber = true;
        }
        for d in [
            "enemyPhysicalDamage",
            "enemyLightningDamage",
            "enemyColdDamage",
            "enemyFireDamage",
            "enemyChaosDamage",
        ] {
            c.ph(d, empty());
        }
        // `input.enemyDamageRollRange or varControls[...].placeholder` (L4); the control's placeholder is a string that
        // math.max coerces (L6) -- `n` does the same.
        let rr_src = c.tab.input.get("enemyDamageRollRange").filter(|x| truthy(Some(x))).cloned();
        let rr_src = rr_src.or_else(|| c.tab.control_placeholder("enemyDamageRollRange"));
        let roll_range_mult = min2(max2(n(rr_src.as_ref(), 2284)?, 0.0), 100.0);
        let uber_mult = boss.get("UberDamageMultiplier").and_then(json_num);
        if let Some(Json::Object(mults)) = boss.get("DamageMultipliers") {
            for (damage_type, m) in mults {
                let m0 = m.get(0).and_then(json_num).unwrap_or(f64::NAN);
                let m1 = m.get(1).and_then(json_num).unwrap_or(f64::NAN);
                let tv = table_at(&constants().monster_damage_table, c.tab.enemy_level, 2287)?;
                let var = format!("enemy{damage_type}Damage");
                match uber_mult {
                    Some(u) if is_uber => c.ph(&var, num(round(tv * (m0 + roll_range_mult * m1) * u, None))),
                    _ => c.ph(&var, num(round(tv * (m0 + roll_range_mult * m1), None))),
                }
            }
        }
        for p in ["enemyPhysicalOverwhelm", "enemyLightningPen", "enemyColdPen", "enemyFirePen"] {
            c.ph(p, empty());
        }
        if let Some(Json::Object(pens)) = boss.get("DamagePenetrations") {
            let uber_pens = boss.get("UberDamagePenetrations");
            for (pen_type, pen) in pens {
                let var = format!("enemy{pen_type}");
                let uber_pen = uber_pens.and_then(|u| u.get(pen_type));
                let chosen = if is_uber && jtruthy(uber_pens) && jtruthy(uber_pen) {
                    uber_pen
                } else {
                    Some(pen)
                };
                if let Some(val) = jval(chosen) {
                    c.ph(&var, val);
                }
            }
        }
        if jtruthy(boss.get("DamageType"))
            && let Some(dt) = jval(boss.get("DamageType"))
        {
            c.tab.sel_by_value("enemyDamageType", &dt);
            c.tab.input.insert("enemyDamageType".into(), dt);
        }
        c.tab.set_control_enabled("enemyDamageType", false);
        if is_uber && jtruthy(boss.get("UberSpeed")) {
            if let Some(x) = jval(boss.get("UberSpeed")) {
                c.ph("enemySpeed", x);
            }
        } else if jtruthy(boss.get("speed"))
            && let Some(x) = jval(boss.get("speed"))
        {
            c.ph("enemySpeed", x);
        }
        if jtruthy(boss.get("critChance"))
            && let Some(x) = jval(boss.get("critChance"))
        {
            c.ph("enemyCritChance", x);
        }
        flag(c.mod_list, "BossSkillActive", CONFIG, vec![]);
        // boss specific mods
        if name == "Atziri Flameblast" && is_uber {
            nm(
                c.enemy_mod_list,
                "Damage",
                ModType::Inc,
                Value::Num(60.0),
                "Alluring Abyss Map Mod",
                vec![],
            );
        }
        if let Some(extra) = boss.get("additionalStats").filter(|x| jtruthy(Some(x))) {
            // `isUber and additionalStats.uber or additionalStats.base` (L4).
            let uber = extra.get("uber").filter(|x| jtruthy(Some(x)));
            let stats = if is_uber && uber.is_some() { uber } else { extra.get("base") };
            if let Some(Json::Object(stats)) = stats {
                for (k, x) in stats {
                    match jval(Some(x)) {
                        Some(sv) if lua_tostring(&sv) == "flag" => flag(c.enemy_mod_list, k, "BossSkillAdditionalData", vec![]),
                        Some(InputValue::Num(nv)) => nm(
                            c.enemy_mod_list,
                            k,
                            ModType::Base,
                            Value::Num(nv),
                            "BossSkillAdditionalData",
                            vec![],
                        ),
                        Some(InputValue::Str(sv)) => nm(c.enemy_mod_list, k, ModType::Base, s(&sv), "BossSkillAdditionalData", vec![]),
                        Some(InputValue::Bool(b)) => nm(
                            c.enemy_mod_list,
                            k,
                            ModType::Base,
                            Value::Bool(b),
                            "BossSkillAdditionalData",
                            vec![],
                        ),
                        None => {}
                    }
                }
            }
        }
    } else {
        if !c.tab.control_enabled("enemyDamageType") {
            let avg = InputValue::Str("Average".into());
            c.tab.input.insert("enemyDamageType".into(), avg.clone());
            c.tab.sel_by_value("enemyDamageType", &avg);
        }
        c.tab.set_control_enabled("enemyDamageType", true);
    }
    Ok(())
}

pub static HAND_PORTED: [ApplyEntry; 10] = [
    e(
        "enemyIsBoss",
        2086,
        2222,
        "0c63f14001520552cd4d5379f9f5636ba8e881470124bbf61cba1ddd6ee5d316",
        enemy_is_boss,
    ),
    e(
        "presetBossSkills",
        2272,
        2351,
        "5ee18b51ad867d8b1a16034a80ab73595ed76988455c03766b7301bf005e7203",
        preset_boss_skills,
    ),
    // ---- Map Modifiers (R3.5b): `local function mapAffixDropDownFunction` (:117-130), shared by the 8 dropdowns -------
    e("MapPrefix1", 117, 130, MAP_AFFIX_SHA, map_affix),
    e("MapPrefix2", 117, 130, MAP_AFFIX_SHA, map_affix),
    e("MapPrefix3", 117, 130, MAP_AFFIX_SHA, map_affix),
    e("MapPrefix4", 117, 130, MAP_AFFIX_SHA, map_affix),
    e("MapSuffix1", 117, 130, MAP_AFFIX_SHA, map_affix),
    e("MapSuffix2", 117, 130, MAP_AFFIX_SHA, map_affix),
    e("MapSuffix3", 117, 130, MAP_AFFIX_SHA, map_affix),
    e("MapSuffix4", 117, 130, MAP_AFFIX_SHA, map_affix),
];

/// The whole registry: [`HAND_PORTED`] + the generated entries (`apply_gen.rs`, R3.5b), in varList order -- all 539
/// `apply` closures of config-options.json.
pub static APPLY_REGISTRY: LazyLock<Vec<ApplyEntry>> = LazyLock::new(|| {
    let opts = pob_data::config_options::config_options();
    let order: std::collections::HashMap<&str, usize> = opts
        .vars
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.var.as_deref().map(|n| (n, i)))
        .collect();
    let mut all: Vec<ApplyEntry> = HAND_PORTED.iter().chain(super::apply_gen::GENERATED.iter()).copied().collect();
    all.sort_by_key(|en| order.get(en.var).copied().unwrap_or(usize::MAX));
    all
});

// --- runtime of the generated closures (apply_gen.rs; grammar: tools/config-apply-gen/gen.ts) --------------------------
// A Lua scalar is a `ConfigValue` (number | string | boolean); `nil` never reaches these bodies.

pub(super) type LV = ConfigValue;

pub(super) fn ln(x: f64) -> LV {
    InputValue::Num(x)
}

pub(super) fn ls(x: &str) -> LV {
    InputValue::Str(x.to_owned())
}

/// A boolean scalar (`pub`: the current bodies never need one as a scalar, the grammar does).
pub fn lb(b: bool) -> LV {
    InputValue::Bool(b)
}

fn type_name(v: &LV) -> &'static str {
    match v {
        InputValue::Num(_) => "number",
        InputValue::Str(_) => "string",
        InputValue::Bool(_) => "boolean",
    }
}

/// `type(v)`.
pub(super) fn ltype(v: &LV) -> LV {
    ls(type_name(v))
}

/// Lua `==` (no coercion across types: `"1" == 1` is false).
pub(super) fn leq(a: &LV, b: &LV) -> bool {
    a == b
}

/// A scalar stored as a mod value / table field.
pub(super) fn lval(v: &LV) -> Value {
    match v {
        InputValue::Num(x) => Value::Num(*x),
        InputValue::Str(t) => Value::str(t),
        InputValue::Bool(b) => Value::Bool(*b),
    }
}

/// The number an arithmetic operand coerces to (lj_meta.c `str2num`: numbers, numeric strings).
fn coerce(v: &LV) -> Option<f64> {
    match v {
        InputValue::Num(x) => Some(*x),
        InputValue::Str(t) => tonumber(t),
        InputValue::Bool(_) => None,
    }
}

/// LuaJIT `lj_err_optype`: `attempt to <op> local 'val' (a string value)` / `attempt to <op> a boolean value`.
fn optype(op: &str, v: &LV, name: &str, site: u32) -> LuaError {
    if name.is_empty() {
        LuaError(format!("{CO}:{site}: attempt to {op} a {} value", type_name(v)))
    } else {
        LuaError(format!("{CO}:{site}: attempt to {op} {name} (a {} value)", type_name(v)))
    }
}

/// The arithmetic operators of the generated bodies.
#[derive(Clone, Copy, Debug)]
pub enum Ar {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

/// `a <op> b` (lj_meta_arith: both operands coerced, else the first one that does not coerce is reported).
pub(super) fn arith(op: Ar, a: &LV, b: &LV, site: u32, an: &str, bn: &str) -> LuaResult<LV> {
    match (coerce(a), coerce(b)) {
        (Some(x), Some(y)) => Ok(ln(match op {
            Ar::Add => x + y,
            Ar::Sub => x - y,
            Ar::Mul => x * y,
            Ar::Div => x / y,
            Ar::Mod => pob_lua::number::modulo(x, y),
            Ar::Pow => x.powf(y),
        })),
        (None, _) => Err(optype("perform arithmetic on", a, an, site)),
        (_, None) => Err(optype("perform arithmetic on", b, bn, site)),
    }
}

/// `-a`.
pub(super) fn unm(a: &LV, site: u32, an: &str) -> LuaResult<LV> {
    coerce(a)
        .map(|x| ln(-x))
        .ok_or_else(|| optype("perform arithmetic on", a, an, site))
}

/// `a .. b`: numbers as `tostring` (%.14g), booleans raise.
pub(super) fn concat(a: &LV, b: &LV, site: u32, an: &str, bn: &str) -> LuaResult<LV> {
    let part = |v: &LV, name: &str| match v {
        InputValue::Num(x) => Ok(tostring_num(*x)),
        InputValue::Str(t) => Ok(t.clone()),
        InputValue::Bool(_) => Err(optype("concatenate", v, name, site)),
    };
    Ok(InputValue::Str(part(a, an)? + &part(b, bn)?))
}

/// `a < b` / `a <= b` (lj_meta_comp): numbers, or strings byte-wise; across types `attempt to compare <a> with <b>`.
fn compare(a: &LV, b: &LV, site: u32) -> LuaResult<std::cmp::Ordering> {
    match (a, b) {
        (InputValue::Str(x), InputValue::Str(y)) => Ok(x.as_bytes().cmp(y.as_bytes())),
        _ if type_name(a) == type_name(b) => Err(LuaError(format!("{CO}:{site}: attempt to compare two {} values", type_name(a)))),
        _ => Err(LuaError(format!(
            "{CO}:{site}: attempt to compare {} with {}",
            type_name(a),
            type_name(b)
        ))),
    }
}

/// `a < b` (`a > b` is emitted as `b < a`, the VM order).
pub(super) fn lt(a: &LV, b: &LV, site: u32) -> LuaResult<bool> {
    if let (InputValue::Num(x), InputValue::Num(y)) = (a, b) {
        return Ok(x < y); // NaN: false
    }
    Ok(compare(a, b, site)?.is_lt())
}

/// `a <= b` (`a >= b` is emitted as `b <= a`).
pub(super) fn le(a: &LV, b: &LV, site: u32) -> LuaResult<bool> {
    if let (InputValue::Num(x), InputValue::Num(y)) = (a, b) {
        return Ok(x <= y);
    }
    Ok(compare(a, b, site)?.is_le())
}

/// `math.min`/`math.max` (aliases `m_min`/`m_max`): numeric strings coerced (lj_lib_checknum), else `bad argument`.
/// The function name in that message is what LuaJIT says (`config-apply-strings.json`).
fn checknum(v: &LV, arg: u32, fname: &str, site: u32) -> LuaResult<f64> {
    coerce(v).ok_or_else(|| {
        LuaError(format!(
            "{CO}:{site}: bad argument #{arg} to '{fname}' (number expected, got {})",
            type_name(v)
        ))
    })
}

pub(super) fn lmin(a: &LV, b: &LV, site: u32) -> LuaResult<LV> {
    Ok(ln(min2(checknum(a, 1, "m_min", site)?, checknum(b, 2, "m_min", site)?)))
}

pub(super) fn lmax(a: &LV, b: &LV, site: u32) -> LuaResult<LV> {
    Ok(ln(max2(checknum(a, 1, "m_max", site)?, checknum(b, 2, "m_max", site)?)))
}

/// A table constructor with positional fields (array part `1..n`) and named fields.
pub(super) fn table_arr(array: Vec<Value>, pairs: &[(&str, Value)]) -> Table {
    let mut t = table(pairs);
    t.array = array;
    t
}

/// `list:NewMod(name, type, value, [source,] tags...)` (ModStore.lua:109 -> ModTools.lua:33-67).
pub(super) fn nmod(list: &mut ModList, name: &str, ty: ModType, value: Value, source: Option<&str>, tags: Vec<Tag>) {
    list.mods.push(Rc::new(create_mod(name, Some(ty), value, source, 0, 0, tags)));
}

/// `modLib.createMod(...)` as a value (`{ mod = ... }`).
pub(super) fn cmod(name: &str, ty: ModType, value: Value, source: Option<&str>, tags: Vec<Tag>) -> Value {
    Value::Mod(Rc::new(create_mod(name, Some(ty), value, source, 0, 0, tags)))
}

pub(super) fn skill_type_num(name: &str) -> f64 {
    pob_data::globals::globals().skill_type[name]
}

// --- Map Modifiers: mapAffixDropDownFunction (ConfigOptions.lua:117-130) + Data/ModMap.lua `AffixData[x].apply` --------

const MAP_AFFIX_SHA: &str = "3b2d8cd94190aa522b296b76091fe6fc3ab9078aec72e73042962a70ff6edf25";

/// `data.mapMods` as converted (`data/map-mods.json`).
fn map_mods() -> &'static Json {
    static M: OnceLock<Json> = OnceLock::new();
    M.get_or_init(|| pob_data::read_json("map-mods.json").unwrap_or_else(|e| panic!("{e}")))
}

/// The arguments of an `AffixData[x].apply`: `val` (the tier 1..3 for list/count; unused by the check bodies),
/// `rollRange` (count only: 100), `mapModEffect`, `values`.
pub struct MapArgs<'a> {
    pub tier: f64,
    pub roll_range: f64,
    pub effect: f64,
    pub values: Option<&'a Json>,
}

impl MapArgs<'_> {
    /// `values[val][i][j]...` (1-based); a missing entry is `nil` -> indexing/arithmetic on it raises.
    fn v(&self, path: &[usize]) -> LuaResult<f64> {
        let mut j = self
            .values
            .ok_or_else(|| LuaError("Data/ModMap.lua: attempt to index local 'values' (a nil value)".into()))?;
        if self.tier.fract() != 0.0 || self.tier < 1.0 {
            return Err(LuaError("Data/ModMap.lua: attempt to index a nil value".into()));
        }
        let mut idx = vec![self.tier as usize];
        idx.extend_from_slice(path);
        for i in idx {
            j = i
                .checked_sub(1)
                .and_then(|k| j.get(k))
                .ok_or_else(|| LuaError("Data/ModMap.lua: attempt to index a nil value".into()))?;
        }
        json_num(j).ok_or_else(|| LuaError("Data/ModMap.lua: attempt to perform arithmetic on a nil value".into()))
    }

    /// `(values[val]<p>[1] + (values[val]<p>[2] - values[val]<p>[1]) * rollRange / 100) * mapModEffect`.
    fn roll(&self, path: &[usize]) -> LuaResult<f64> {
        let mut p1 = path.to_vec();
        p1.push(1);
        let mut p2 = path.to_vec();
        p2.push(2);
        let (lo, hi) = (self.v(&p1)?, self.v(&p2)?);
        Ok((lo + (hi - lo) * self.roll_range / 100.0) * self.effect)
    }
}

pub type MapFn = fn(&MapArgs<'_>, &mut ApplyCtx<'_>) -> LuaResult<()>;

/// One hand-ported `data.mapMods.AffixData[name].apply`, with its place in Data/ModMap.lua and the sha256 of the body as
/// `data/closures.json` recorded it (`mapMods.AffixData.<name>.apply`; drift test `config_registry.rs`).
#[derive(Clone, Copy)]
pub struct MapAffixEntry {
    pub name: &'static str,
    pub from: u32,
    pub to: u32,
    pub sha: &'static str,
    pub apply: MapFn,
}

const fn m(name: &'static str, from: u32, to: u32, sha: &'static str, apply: MapFn) -> MapAffixEntry {
    MapAffixEntry {
        name,
        from,
        to,
        sha,
        apply,
    }
}

/// `(enemy)modList:NewMod(name, type, value, "Map mod <affix>", tags...)`.
fn mm(c: &mut ApplyCtx<'_>, enemy: bool, name: &str, ty: ModType, value: f64, affix: &str, tags: Vec<Tag>) {
    let list = if enemy { &mut *c.enemy_mod_list } else { &mut *c.mod_list };
    nmod(list, name, ty, Value::Num(value), Some(&format!("Map mod {affix}")), tags);
}

/// `(enemy)modList:NewMod(name, "FLAG", true, "Map mod <affix>")`.
fn mf(c: &mut ApplyCtx<'_>, enemy: bool, name: &str, affix: &str) {
    let list = if enemy { &mut *c.enemy_mod_list } else { &mut *c.mod_list };
    nmod(
        list,
        name,
        ModType::Flag,
        Value::Bool(true),
        Some(&format!("Map mod {affix}")),
        vec![],
    );
}

fn skill_type_tag(name: &str, neg: bool) -> Tag {
    let mut p = vec![("type", s("SkillType")), ("skillType", Value::Num(skill_type_num(name)))];
    if neg {
        p.push(("neg", Value::Bool(true)));
    }
    tag(&p)
}

const ENEMY: bool = true;
const PLAYER: bool = false;

/// Every `AffixData[x].apply` of Data/ModMap.lua (41), in file order.
#[rustfmt::skip]
pub static MAP_AFFIX_APPLY: [MapAffixEntry; 41] = [
    m("Armoured", 12, 14, "818c41fa87bd12cac6f6fbc1a75aa0669108cf78175096fccb6c67b0d7e61721", |a, c| {
        mm(c, ENEMY, "PhysicalDamageReduction", ModType::Base, a.v(&[])? * a.effect, "Armoured", vec![]); Ok(()) }),
    m("Hexproof", 20, 22, "b2003d96e3ca0ad272d846086b483bb499c75c33bba2846e02286fa44dfcff69", |_, c| {
        mf(c, ENEMY, "Hexproof", "Hexproof"); Ok(()) }),
    m("Hexwarded", 29, 31, "f6d6f8ff58fbdca3b939d90a247b23b4fab890fdc3d5ef1ef8ba052ac1153b1a", |a, c| {
        mm(c, ENEMY, "CurseEffectOnSelf", ModType::More, -a.v(&[])? * a.effect, "Hexwarded", vec![]); Ok(()) }),
    m("Resistant", 38, 41, "4c39240b74d57c4f0a12ca033b84f700c18d767c9e5f43f1109342aec387bf12", |a, c| {
        mm(c, ENEMY, "ElementalResist", ModType::Base, a.v(&[1])? * a.effect, "Resistant", vec![]);
        mm(c, ENEMY, "ChaosResist", ModType::Base, a.v(&[2])? * a.effect, "Resistant", vec![]); Ok(()) }),
    m("Unwavering", 47, 50, "9a5edb6e09eaa1dbc585bc6bd852702cb01275c0fd5b5f15a8abb3ced04ebb57", |a, c| {
        mm(c, ENEMY, "AvoidStun", ModType::Base, 100.0, "Unwavering", vec![]);
        mm(c, ENEMY, "Life", ModType::More, a.roll(&[1])?, "Unwavering", vec![]); Ok(()) }),
    m("Fecund", 56, 58, "2499e4e54cf353ab108a2a7bcc4e9356b1ec075ceaa10b5d5274d4bc7f1dee4f", |a, c| {
        mm(c, ENEMY, "Life", ModType::More, a.roll(&[])?, "Fecund", vec![]); Ok(()) }),
    m("Unstoppable", 64, 67, "d6eeb086a1e7fe3c4d11f6cf1de739dc35f9406ccb5bff86794d9a304a7676ad", |_, c| {
        // MISSING: Monsters cannot be Taunted
        mm(c, ENEMY, "MinimumActionSpeed", ModType::Max, 100.0, "Unstoppable", vec![]); Ok(()) }),
    m("Impervious", 73, 77, "0bdb7b6cf9b33614eee9071eb907c6cd3a74ca85e0aa1cded384fc985aca0c62", |a, c| {
        mm(c, ENEMY, "AvoidPoison", ModType::Base, a.v(&[])? * a.effect, "Impervious", vec![]);
        mm(c, ENEMY, "AvoidImpale", ModType::Base, a.v(&[])? * a.effect, "Impervious", vec![]);
        mm(c, ENEMY, "AvoidBleed", ModType::Base, a.v(&[])? * a.effect, "Impervious", vec![]); Ok(()) }),
    m("Oppressive", 83, 85, "433c9530b060d7ea33de063182a0409dbcd6d0cbef92955cc49aab2fa44506f7", |a, c| {
        mm(c, ENEMY, "SpellSuppressionChance", ModType::Base, a.v(&[])? * a.effect, "Oppressive", vec![]); Ok(()) }),
    m("Buffered", 91, 93, "71fb5806f1c4799d7a688c8358c1d2893d2673939ac6d58084791c25fc95b7ef", |a, c| {
        mm(c, ENEMY, "LifeGainAsEnergyShield", ModType::Base, a.roll(&[])?, "Buffered", vec![]); Ok(()) }),
    m("Titan's", 99, 102, "250a49a257b3d15d688677b4edb40cd63d6cc1e1a393c5953b62cceed2816ef6", |a, c| {
        mm(c, ENEMY, "Life", ModType::More, a.v(&[1])? * a.effect, "Titan's", vec![cond("RareOrUnique")]);
        mm(c, ENEMY, "AreaOfEffect", ModType::Inc, a.v(&[2])? * a.effect, "Titan's", vec![cond("RareOrUnique")]); Ok(()) }),
    m("Savage", 109, 111, "190b6698d0934ea46b9a81e8f7e84e046114a2f37f47b72d459a5bf3a5cc161e", |a, c| {
        mm(c, ENEMY, "Damage", ModType::Inc, a.roll(&[])?, "Savage", vec![]); Ok(()) }),
    m("Burning", 117, 119, "294f42d630dc64fa11726899643c8835cdea0e0e2ac88455195fbeb3551c45df", |a, c| {
        mm(c, ENEMY, "PhysicalDamageGainAsFire", ModType::Base, a.roll(&[])?, "Burning", vec![]); Ok(()) }),
    m("Freezing", 125, 127, "393cc0c7eb5315306ad4b64b2136ccfaac293d3d304df836077cd383cafcc73a", |a, c| {
        mm(c, ENEMY, "PhysicalDamageGainAsCold", ModType::Base, a.roll(&[])?, "Freezing", vec![]); Ok(()) }),
    m("Shocking", 133, 135, "96dc8e5d295dd132cfaec9f7f0d7e41662eff775bf021bf80fb522c69f2c0ab0", |a, c| {
        mm(c, ENEMY, "PhysicalDamageGainAsLightning", ModType::Base, a.roll(&[])?, "Shocking", vec![]); Ok(()) }),
    m("Profane", 141, 144, "02a7c50bb8639489a3675622afa48d0c8cc8b74097e988619a6474efc639a392", |a, c| {
        mm(c, ENEMY, "PhysicalDamageGainAsChaos", ModType::Base, a.roll(&[1])?, "Profane", vec![]);
        mf(c, PLAYER, "Condition:CanBeWithered", "Profane"); Ok(()) }),
    m("Fleet", 150, 153, "a3ad68ab3a97e5bc946578cbf65bf62715f06dd70ae6d2f90dc34de6ddefc7b6", |a, c| {
        // attack and cast is the same so applying it once, movespeed does not matter
        mm(c, ENEMY, "Speed", ModType::Inc, a.roll(&[2])?, "Fleet", vec![]); Ok(()) }),
    m("Conflagrating", 158, 161, "3b142ef4b69f6bfdf15d3bdf52ab79ade50dfbf712af61931409387e674f3250", |_, c| {
        mm(c, ENEMY, "IgniteChance", ModType::Base, 100.0, "Conflagrating", vec![]);
        mf(c, ENEMY, "AllDamageIgnites", "Conflagrating"); Ok(()) }),
    m("Impaling", 167, 169, "b250fe9547c8848044725422d371cae6684e116af30f8d1474797be9d6449a20", |a, c| {
        // `..., "Map mod Impaling", ModFlag.Attack)`: a number in vararg slot 2 = flags (ModTools.lua:42-45).
        let flags = pob_data::globals::globals().mod_flag["Attack"] as u32;
        let md = create_mod("ImpaleChance", Some(ModType::Base), Value::Num(a.v(&[])? * a.effect), Some("Map mod Impaling"), flags, 0, vec![]);
        c.enemy_mod_list.mods.push(Rc::new(md)); Ok(()) }),
    m("Empowered", 175, 177, "adb70f0b03c7bdf728153c0625b823d9d9b7e07b9965099adfa4208bc9741e3a", |a, c| {
        mm(c, ENEMY, "ElementalAilmentChance", ModType::Base, a.v(&[])? * a.effect, "Empowered", vec![]); Ok(()) }),
    m("Overlord's", 183, 186, "6c9f71a497af6b859ec862e80d02c91384e80c390da1042c99ff0b906de1f943", |a, c| {
        mm(c, ENEMY, "Damage", ModType::Inc, a.v(&[1])? * a.effect, "Overlord's", vec![cond("RareOrUnique")]);
        mm(c, ENEMY, "Speed", ModType::Inc, a.v(&[2])? * a.effect, "Overlord's", vec![cond("RareOrUnique")]); Ok(()) }),
    m("Punishing", 193, 194, "db17ed70c654b159acfe392be812363f5ff82659b164962d346e7e7a5fb957e1", |_, _| Ok(())),
    m("Mirrored", 200, 201, "db17ed70c654b159acfe392be812363f5ff82659b164962d346e7e7a5fb957e1", |_, _| Ok(())),
    m("of Balance", 208, 211, "9cf01e0006c0f257d9c72cb48db9e7be5494ce9652daeb54b5157626317d582d", |_, _| Ok(())),
    m("of Congealment", 217, 221, "c604ff72ec4d448ec8a29e4163ff3699b3540b8a82f8ad347f756abb2b349c7e", |_, c| {
        mf(c, ENEMY, "CannotLeechLifeFromSelf", "of Congealment");
        mf(c, ENEMY, "CannotLeechManaFromSelf", "of Congealment");
        mf(c, ENEMY, "CannotLeechEnergyShieldFromSelf", "of Congealment"); Ok(()) }),
    m("of Drought", 228, 230, "0197215368725e64c7f6b763de5e85ff87122baba6255fcad408cd25c7e2ab3e", |a, c| {
        mm(c, PLAYER, "FlaskChargesGained", ModType::Inc, -a.v(&[])? * a.effect, "of Drought", vec![]); Ok(()) }),
    m("of Exposure", 238, 246, "2e295018661bc9e6bd279a4e7499be33584cf6970143bbc83b5d38e32047f6ef", |a, c| {
        if a.v(&[2])? != 0.0 {
            let roll = a.roll(&[])?;
            mm(c, PLAYER, "FireResistMax", ModType::Base, -roll, "of Exposure", vec![]);
            mm(c, PLAYER, "ColdResistMax", ModType::Base, -roll, "of Exposure", vec![]);
            mm(c, PLAYER, "LightningResistMax", ModType::Base, -roll, "of Exposure", vec![]);
            mm(c, PLAYER, "ChaosResistMax", ModType::Base, -roll, "of Exposure", vec![]);
        }
        Ok(()) }),
    m("of Impotence", 253, 255, "3702fdea037a02f9c7d65172696fdb96e0747408dbc1fb1b75475278feeef004", |a, c| {
        mm(c, PLAYER, "AreaOfEffect", ModType::More, -a.v(&[])? * a.effect, "of Impotence", vec![]); Ok(()) }),
    m("of Insulation", 262, 264, "56b5abc314ae5fc025a3cbd2f8e0694f172b4a6205df2e879f794b48c567d95d", |a, c| {
        mm(c, ENEMY, "AvoidElementalAilments", ModType::Base, a.v(&[])? * a.effect, "of Insulation", vec![]); Ok(()) }),
    m("of Miring", 271, 275, "d4562915baf81d3685db4a954441ee8f961052b17ca61617d8112f3639c25232", |a, c| {
        // modList:NewMod("DodgeChanceIsUnlucky", "FLAG", true, "Map mod of Miring") -- OLD MOD
        mm(c, PLAYER, "SpellSuppressionEffect", ModType::Base, -a.v(&[1])? * a.effect, "of Miring", vec![]);
        mm(c, ENEMY, "Accuracy", ModType::Inc, a.v(&[2])? * a.effect, "of Miring", vec![]); Ok(()) }),
    m("of Rust", 282, 285, "8381aacb8d2da2e1f3537007824a18626a333000d29fd0592e9fa801ce6253e6", |a, c| {
        mm(c, PLAYER, "BlockChance", ModType::Inc, -a.v(&[1])? * a.effect, "of Rust", vec![]);
        mm(c, PLAYER, "Armour", ModType::More, -a.v(&[2])? * a.effect, "of Rust", vec![]); Ok(()) }),
    m("of Smothering", 292, 295, "f47c05a2bcd2f4b133df5219028f4ee27876dc6e0167f278b0fccc0e1ee15feb", |a, c| {
        mm(c, PLAYER, "LifeRecoveryRate", ModType::More, -a.v(&[])? * a.effect, "of Smothering", vec![]);
        mm(c, PLAYER, "EnergyShieldRecoveryRate", ModType::More, -a.v(&[])? * a.effect, "of Smothering", vec![]); Ok(()) }),
    m("of Stasis", 301, 305, "315d1c21b867e21ec1ad1b50d9c636ad74bb92203583727f8e0596376ef94d4e", |_, c| {
        mf(c, PLAYER, "NoLifeRegen", "of Stasis");
        mf(c, PLAYER, "NoEnergyShieldRegen", "of Stasis");
        mf(c, PLAYER, "NoManaRegen", "of Stasis"); Ok(()) }),
    m("of Toughness", 311, 313, "6894cf71d5e7e68f642da5f1162d880b65aa815c91c9a0f37f5029769b6809b4", |a, c| {
        // `-(lo + (hi - lo) * rollRange / 100) * mapModEffect`: the minus applies to the parenthesis, before the product.
        let (lo, hi) = (a.v(&[1])?, a.v(&[2])?);
        mm(c, ENEMY, "SelfCritMultiplier", ModType::Inc, -(lo + (hi - lo) * a.roll_range / 100.0) * a.effect, "of Toughness", vec![]);
        Ok(()) }),
    m("of Fatigue", 319, 321, "ef8f4f623031da0d505600bcf238b65e9db244a030c30ff6ad38888730ee3d1c", |a, c| {
        mm(c, PLAYER, "CooldownRecovery", ModType::More, -a.v(&[])? * a.effect, "of Fatigue", vec![]); Ok(()) }),
    m("of Transience", 327, 328, "db17ed70c654b159acfe392be812363f5ff82659b164962d346e7e7a5fb957e1", |_, _| Ok(())),
    m("of Doubt", 334, 336, "6d593d666f8fe51b118df8d6cab42966eb8f5b434c0b9981e1576bf99943f99e", |a, c| {
        mm(c, PLAYER, "AuraEffect", ModType::Inc, -a.v(&[])? * a.effect, "of Doubt",
            vec![skill_type_tag("Aura", false), skill_type_tag("AppliesCurse", true)]);
        Ok(()) }),
    m("of Imprecision", 342, 344, "e3dd033cef23f4731d402b06fe3903c55a12ce6f164facab3d9dd17bf7431c83", |a, c| {
        mm(c, PLAYER, "Accuracy", ModType::More, -a.v(&[])? * a.effect, "of Imprecision", vec![]); Ok(()) }),
    m("of Blinding", 349, 351, "37e6755b9c89f84b7461b4ddbfb74a52bfb2419f7122c049328e97de10875209", |_, _| Ok(())),
    m("of Venom", 356, 358, "473f89de9d1052169a7fb206b69b5c16a94c17a5d120de414c82a2f5942844c7", |_, c| {
        mm(c, ENEMY, "PoisonChance", ModType::Base, 100.0, "of Venom", vec![]); Ok(()) }),
    m("of Deadliness", 364, 367, "492527040dd43a3b391db710b9f1b3bd68a9d0ccfcf921370bd27ad600531adb", |a, c| {
        mm(c, ENEMY, "CritChance", ModType::Inc, a.roll(&[1])?, "of Deadliness", vec![]);
        mm(c, ENEMY, "CritMultiplier", ModType::Base, a.roll(&[2])?, "of Deadliness", vec![]); Ok(()) }),
];

/// Every entry whose `from`/`to`/`sha` differ from `data/closures.json` (`mapMods.AffixData.<name>.apply`), or that is
/// missing there.
pub fn map_affix_drift(entries: &[MapAffixEntry]) -> Vec<String> {
    let j: Json = pob_data::read_json("closures.json").unwrap_or_else(|e| panic!("{e}"));
    let empty = Vec::new();
    let list = j["closures"].as_array().unwrap_or(&empty);
    let mut out = Vec::new();
    for en in entries {
        let path = format!("mapMods.AffixData.{}.apply", en.name);
        match list.iter().find(|x| x["path"].as_str() == Some(path.as_str())) {
            None => out.push(format!("{}: not in closures.json", en.name)),
            Some(x) => {
                let same = x["src"].as_str() == Some("Data/ModMap.lua")
                    && x["from"].as_f64() == Some(f64::from(en.from))
                    && x["to"].as_f64() == Some(f64::from(en.to))
                    && x["sha256"].as_str() == Some(en.sha);
                if !same {
                    out.push(format!(
                        "{}: registry Data/ModMap.lua:{}-{} {} != json {x}",
                        en.name, en.from, en.to, en.sha
                    ));
                }
            }
        }
    }
    out
}

/// The `AffixData` names whose `hasApply` is true in `data/map-mods.json`.
pub fn map_affixes_with_apply() -> Vec<String> {
    match map_mods()["AffixData"].as_object() {
        Some(o) => o
            .iter()
            .filter(|(_, d)| d.get("hasApply") == Some(&Json::Bool(true)))
            .map(|(k, _)| k.clone())
            .collect(),
        None => Vec::new(),
    }
}

/// ConfigOptions.lua:117-130 `mapAffixDropDownFunction(val, modList, enemyModList, build)`.
fn map_affix(v: &ConfigValue, c: &mut ApplyCtx<'_>) -> LuaResult<()> {
    if leq(v, &ls("NONE")) {
        return Ok(());
    }
    // `data.mapMods.AffixData[val] or {}`: the keys are strings, so a number/boolean `val` finds nothing.
    let InputValue::Str(name) = v else { return Ok(()) };
    let Some(affix) = map_mods()["AffixData"].get(name.as_str()) else {
        return Ok(());
    };
    // `if affixData.apply then` -- the converter records it as `hasApply`.
    if affix.get("hasApply") != Some(&Json::Bool(true)) {
        return Ok(());
    }
    let ty = affix.get("type").and_then(Json::as_str);
    let site = match ty {
        Some("check") => 122,
        Some("list") => 124,
        Some("count") => 126,
        _ => return Ok(()),
    };
    // `(1 + (build.configTab.input['multiplierMapModEffect'] or 0) / 100)` (L4: a stored 0 or "" is kept).
    let eff_in = c
        .tab
        .input
        .get("multiplierMapModEffect")
        .filter(|x| truthy(Some(x)))
        .cloned()
        .unwrap_or(ln(0.0));
    let effect = arith(Ar::Add, &ln(1.0), &arith(Ar::Div, &eff_in, &ln(100.0), site, "", "")?, site, "", "")?;
    let InputValue::Num(effect) = effect else {
        unreachable!("arith returns a number")
    };
    // `4 - (build.configTab.varControls['multiplierMapModTier'].selIndex or 1)`
    let tier = 4.0 - c.tab.sel_index("multiplierMapModTier").unwrap_or(1) as f64;
    let args = match site {
        122 => MapArgs {
            tier: f64::NAN,
            roll_range: f64::NAN,
            effect,
            values: None,
        },
        124 => MapArgs {
            tier,
            roll_range: f64::NAN,
            effect,
            values: affix.get("values"),
        },
        _ => MapArgs {
            tier,
            roll_range: 100.0,
            effect,
            values: affix.get("values"),
        },
    };
    let Some(en) = MAP_AFFIX_APPLY.iter().find(|en| en.name == name) else {
        return Err(LuaError(format!("unimplemented: Data/ModMap.lua AffixData['{name}'].apply")));
    };
    (en.apply)(&args, c)
}
