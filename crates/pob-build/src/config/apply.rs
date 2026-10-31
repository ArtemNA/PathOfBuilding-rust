//! The `apply` closures of Modules/ConfigOptions.lua, ported one by one (R3-ARCH §2.5; executable spec pob-ts
//! `src/config/apply.ts`, the same 84 entries), the 8 map-affix dropdowns (R3.5b, with the 41 `AffixData[x].apply` of
//! Data/ModMap.lua), the runtime the generated closures call (`apply_gen.rs`, `tools/config-apply-gen/gen.ts`), and the
//! merged [`APPLY_REGISTRY`] (all 539, varList order). Truth for all 539: LuaJIT probe `tools/lua-corpus/export-config-apply.ts`
//! (test `config_apply_luajit.rs`).
//!
//! The R3.5 84 entries = every `var` that (a) appears in a `<Config>` of the six goldens, or (b) has a `defaultState`/`defaultIndex`/
//! `defaultPlaceholderState` (so `NewConfigSet` fires it with no XML), or (c) lives in the `Enemy Stats` section -- and has
//! an `apply` upstream: 537 Lua lines. Each entry carries `ConfigOptions.lua:<from>-<to>` and the sha256 of the upstream
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

fn skill_name(name: &str, transfigured: bool) -> Tag {
    if transfigured {
        tag(&[
            ("type", s("SkillName")),
            ("skillName", s(name)),
            ("includeTransfigured", Value::Bool(true)),
        ])
    } else {
        tag(&[("type", s("SkillName")), ("skillName", s(name))])
    }
}

/// `{ type = "SkillName", skillName = name, [includeTransfigured = true,] summonSkill = true }`.
fn summon_skill(name: &str, transfigured: bool) -> Tag {
    let mut p = vec![("type", s("SkillName")), ("skillName", s(name))];
    if transfigured {
        p.push(("includeTransfigured", Value::Bool(true)));
    }
    p.push(("summonSkill", Value::Bool(true)));
    tag(&p)
}

/// `{ type = "SkillType", skillType = SkillType[name] }`.
fn skill_type(name: &str) -> Tag {
    let n = pob_data::globals::globals().skill_type[name];
    tag(&[("type", s("SkillType")), ("skillType", Value::Num(n))])
}

/// `{ key = key, value = value }` (a `SkillData` LIST value).
fn skill_data(key: &str, value: Value) -> Value {
    Value::table(table(&[("key", s(key)), ("value", value)]))
}

/// `{ mod = modLib.createMod(...) }` (a `MinionModifier` LIST value).
fn minion_mod(name: &str, tags: Vec<Tag>) -> Value {
    let m = create_mod(name, Some(ModType::Flag), Value::Bool(true), Some(CONFIG), 0, 0, tags);
    Value::table(table(&[("mod", Value::Mod(Rc::new(m)))]))
}

/// `modList:NewMod("MinionModifier", "LIST", { mod = ... })` -- no vararg at all, so `source` is nil (ModTools.lua:33-67).
fn minion(list: &mut ModList, value: Value) {
    list.mods.push(Rc::new(create_mod(
        "MinionModifier",
        Some(ModType::List),
        value,
        None,
        0,
        0,
        vec![],
    )));
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

/// ConfigOptions.lua:437-445 / :2073-2079: `val = m_min(val, cap)` and four or six debuff mods with `val.." <what>"` sources.
fn touch_stacks(list: &mut ModList, val: f64, rows: &[(&str, f64, &str)], cond_var: &str) {
    for (name, mult, what) in rows {
        let debuff = tag(&[("type", s("GlobalEffect")), ("effectType", s("Debuff"))]);
        let src = format!("{} {what}", tostring_num(val));
        nm(list, name, ModType::Inc, Value::Num(val * mult), &src, vec![debuff, cond(cond_var)]);
    }
}

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

/// ConfigOptions.lua:2223-2244 `deliriousPercentage`.
fn delirious(v: &ConfigValue, c: &mut ApplyCtx<'_>) -> LuaResult<()> {
    for (key, more, inc, src) in [
        ("20Percent", -16.0, 6.0, "20% Delirious"),
        ("40Percent", -32.0, 12.0, "40% Delirious"),
        ("60Percent", -48.0, 18.0, "60% Delirious"),
        ("80Percent", -64.0, 24.0, "80% Delirious"),
        ("100Percent", -80.0, 30.0, "100% Delirious"),
    ] {
        if is(v, key) {
            nm(c.enemy_mod_list, "DamageTaken", ModType::More, Value::Num(more), src, vec![]);
            nm(c.enemy_mod_list, "Damage", ModType::Inc, Value::Num(inc), src, vec![]);
        }
    }
    Ok(())
}

/// The hand-ported entries, in ConfigOptions.lua order: the 84 of R3.5, then the 8 `mapAffixDropDownFunction` vars (R3.5b).
#[rustfmt::skip]
pub static HAND_PORTED: [ApplyEntry; 92] = [
    // ---- General ---------------------------------------------------------------------------------------------------
    e("conditionMoving", 171, 173, "90b615ed5b44c6d5018b6977206565bb5ff66ced0ecbacdfa5f19d278c256150", |_, c| {
        flag(c.mod_list, "Condition:Moving", CONFIG, vec![]); Ok(()) }),
    e("conditionFullLife", 174, 176, "b696f1a1ba42ff2a9fea93fef66d5c3041c3a130ae5c0bcbbab82341d17da32f", |_, c| {
        flag(c.mod_list, "Condition:FullLife", CONFIG, vec![]); Ok(()) }),
    e("resourceGainMode", 219, 225, "ebfe6d41ba268031d92a7ddfe184a4f95d96440ec93bca16d2ab8b065fd0761f", |v, c| {
        if is(v, "AVERAGE") {
            flag(c.mod_list, "Condition:AverageResourceGain", CONFIG, vec![]);
        } else if is(v, "MAX") {
            flag(c.mod_list, "Condition:MaxResourceGain", CONFIG, vec![]);
        }
        Ok(()) }),

    // ---- Skill Options ---------------------------------------------------------------------------------------------
    e("aspectOfLunarisLunarProtection", 262, 264, "c71ded45dbd7d267b4dfad044ae445f3f756e120275e2bc58583f8dd3a681283", |v, c| {
        base(c.mod_list, "Multiplier:LunarProtection", n(Some(v), 263)?, CONFIG, vec![]); Ok(()) }),
    e("aspectOfLunarisLunarResistance", 265, 267, "e490f49b1a6d873c8d1f634510de4dcb2b01c9edb7fdd4c7e328dd2a41ea452a", |v, c| {
        base(c.mod_list, "Multiplier:LunarResistance", n(Some(v), 266)?, CONFIG, vec![]); Ok(()) }),
    e("aspectOfSolarisSunscaldStacks", 269, 271, "25da6fd513a11103dcadf1c2d7304a35be2e23bc82ba32cfce509a5c069ba788", |v, c| {
        base(c.mod_list, "Multiplier:SunscaldStack", n(Some(v), 270)?, CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("bloodsoakedBannerStages", 321, 325, "1b04493ac5df93e664e0a1ea987649b2abbe4034c46712b1b98f0e12ee717a24", |v, c| {
        // 10 is the maximum amount of Corrupting Blood Stages ... 9 here with val -1 so it defaults to 1 stage.
        base(c.mod_list, "Multiplier:BloodsoakedBannerStageAfterFirst", min2(n(Some(v), 323)? - 1.0, 9.0), CONFIG, vec![cond("Effective")]);
        Ok(()) }),
    e("targetBrandedEnemy", 337, 339, "c0ba9d8c392689a2244a214dd0616233d68ede9d1283c3260e7bcf3fb407bc8b", |_, c| {
        flag(c.mod_list, "Condition:TargetingBrandedEnemy", CONFIG, vec![]); Ok(()) }),
    e("ConcPathBypassCD", 372, 374, "9f424363834101a705599479c98d6ad68440f65cee23375b2751166b3e57b53a", |_, c| {
        nm(c.mod_list, "CooldownRecovery", ModType::Override, Value::Num(0.0), CONFIG, vec![skill_name("Consecrated Path of Endurance", false)]);
        Ok(()) }),
    e("conditionCorruptingCryStages", 376, 380, "f74154527d5c755cef296cc2f96a270c6bad5374573b5eb0f822c577230c38d5", |v, c| {
        base(c.mod_list, "Multiplier:CorruptingCryStageAfterFirst", min2(n(Some(v), 378)? - 1.0, 9.0), CONFIG, vec![cond("Effective")]);
        Ok(()) }),
    e("channellingCycloneCheck", 386, 388, "51351a86df56397d2355f536202e8ab5a966a5ba9f3ec2f7ca217e0d819567bc", |_, c| {
        flag(c.mod_list, "Condition:ChannellingCyclone", CONFIG, vec![]); Ok(()) }),
    e("touchedDebuffsCount", 437, 445, "837f2a37582f374819bead60f604b4a91f8023ab65e688c2c321f8063c2b764a", |v, c| {
        let val = min2(n(Some(v), 438)?, 10.0);
        touch_stacks(c.mod_list, val, &[
            ("DamageTaken", 6.0, "Eroding Touch Stacks"),
            ("ActionSpeed", -6.0, "Paralysing Touch Stacks"),
            ("FlaskChargesGained", -9.0, "Diluting Touch Stacks"),
            ("FlaskEffect", -9.0, "Diluting Touch Stacks"),
            ("LifeRecoveryRate", -9.0, "Wasting Touch Stacks"),
            ("EnergyShieldRecoveryRate", -9.0, "Wasting Touch Stacks"),
        ], "AffectedByGloriousMadness");
        Ok(()) }),
    e("FlickerStrikeBypassCD", 458, 460, "1a454a754c973832d61e78989a198f26d2a2654d06542f17fbbf63bb5821f522", |_, c| {
        nm(c.mod_list, "CooldownRecovery", ModType::Override, Value::Num(0.0), CONFIG, vec![skill_name("Flicker Strike", true)]);
        Ok(()) }),
    e("infusedChannellingInfusion", 506, 508, "e4f9fef89cd8332634bd32a419ae672e05d6080843df228a6522a4e69c938cff", |_, c| {
        flag(c.mod_list, "Condition:InfusionActive", CONFIG, vec![]); Ok(()) }),
    e("plagueBearerState", 558, 564, "078a89f70fe84eadd25120e1e91b43df8c8fa85641f8b99dbe92d679a9009558", |v, c| {
        if is(v, "INC") {
            flag(c.mod_list, "Condition:PlagueBearerIncubating", CONFIG, vec![]);
        } else if is(v, "INF") {
            flag(c.mod_list, "Condition:PlagueBearerInfecting", CONFIG, vec![]);
        }
        Ok(()) }),
    e("perforateSpikeOverlap", 566, 568, "e5013c5a44076fc8617f383e2e7e55a27eced01e4284fa1a77d06edb4524bd11", |v, c| {
        base(c.mod_list, "Multiplier:PerforateSpikeOverlap", n(Some(v), 567)?, CONFIG, vec![skill_name("Perforate", true)]); Ok(()) }),
    e("prideEffect", 578, 582, "8e3682139e2ca7e7f4d6bc08af198c7a31b2822121f3ba24d184d1b906a9a9c3", |v, c| {
        if is(v, "MAX") {
            flag(c.mod_list, "Condition:PrideMaxEffect", CONFIG, vec![]);
        }
        Ok(()) }),
    e("raiseSpectreEnableBuffs", 588, 590, "4ee916ab444e253ac5e8edaa2f66a54031f4d758f42f91d487facd88f24d9e91", |_, c| {
        nm(c.mod_list, "SkillData", ModType::List, skill_data("enable", Value::Bool(true)), CONFIG,
            vec![skill_type("Buff"), summon_skill("Raise Spectre", true)]);
        Ok(()) }),
    e("raiseSpectreEnableCurses", 591, 594, "8914e2a3ab770c3723ffa7dfd65a01ff1d98de2d44eca56bac076f9cbb9d7990", |_, c| {
        nm(c.mod_list, "SkillData", ModType::List, skill_data("enable", Value::Bool(true)), CONFIG,
            vec![skill_type("Hex"), summon_skill("Raise Spectre", true)]);
        nm(c.mod_list, "SkillData", ModType::List, skill_data("enable", Value::Bool(true)), CONFIG,
            vec![skill_type("Mark"), summon_skill("Raise Spectre", true)]);
        Ok(()) }),
    e("sigilOfPowerStages", 631, 633, "e6e9b9bb75afdb3079805f63f34cedb91b3ccd91c583da83d28407119bbe880d", |v, c| {
        base(c.mod_list, "Multiplier:SigilOfPowerStage", n(Some(v), 632)?, CONFIG, vec![]); Ok(()) }),
    e("configSpectralTigerCount", 644, 647, "0b700acbc0cf5e862c220c98b0f93634f0108664d1cdd4600d6291134084a285", |v, c| {
        base(c.mod_list, "Multiplier:SpectralTigerConfig", n(Some(v), 645)?, CONFIG, vec![]);
        base(c.mod_list, "Multiplier:SpectralTigerCount", 1.0, CONFIG,
            vec![tag(&[("type", s("Multiplier")), ("var", s("SpectralTigerConfig")), ("limitStat", s("ActiveTigerLimit"))])]);
        Ok(()) }),
    e("bloodSandStance", 654, 658, "e7b96b1718aa2798b232ac6e27399d62e7354ec42aa187290ac890af94c1943a", |v, c| {
        if is(v, "SAND") {
            flag(c.mod_list, "Condition:SandStance", CONFIG, vec![]);
        }
        Ok(()) }),
    e("changedStance", 659, 661, "77371b4fe44d407d58279fc5bd1ff5ee392a7c665d8438cd57f0d3db9876b4bf", |_, c| {
        flag(c.mod_list, "Condition:ChangedStanceRecently", CONFIG, vec![]); Ok(()) }),
    e("summonElementalRelicEnableAngerAura", 678, 680, "e3351f04588cbd58023676f2b0d54a140d1b249301494aa742d420a5dcdbdb40", |_, c| {
        nm(c.mod_list, "SkillData", ModType::List, skill_data("enable", Value::Bool(true)), CONFIG,
            vec![tag(&[("type", s("SkillId")), ("skillId", s("Anger"))]), summon_skill("Summon Elemental Relic", false)]);
        Ok(()) }),
    e("summonElementalRelicEnableHatredAura", 681, 683, "0804fae3461983a27d752d06a595bb3513b9f025b70f0ff9224ddaec2f9ef9de", |_, c| {
        nm(c.mod_list, "SkillData", ModType::List, skill_data("enable", Value::Bool(true)), CONFIG,
            vec![tag(&[("type", s("SkillId")), ("skillId", s("Hatred"))]), summon_skill("Summon Elemental Relic", false)]);
        Ok(()) }),
    e("summonElementalRelicEnableWrathAura", 684, 686, "8d9152516f704c5becf7fcf1a01d7844e4667ef3bda4a98a9c7ead2ffc88093e", |_, c| {
        nm(c.mod_list, "SkillData", ModType::List, skill_data("enable", Value::Bool(true)), CONFIG,
            vec![tag(&[("type", s("SkillId")), ("skillId", s("Wrath"))]), summon_skill("Summon Elemental Relic", false)]);
        Ok(()) }),
    e("toxicRainPodOverlap", 711, 713, "17a8c089fc6222df5b7b65c71e6cce8e8276fc2b530e92b68dc81223bd0e9472", |v, c| {
        nm(c.mod_list, "SkillData", ModType::List, skill_data("podOverlapMultiplier", Value::Num(n(Some(v), 712)?)), CONFIG,
            vec![skill_name("Toxic Rain", true)]);
        Ok(()) }),
    e("VigilantStrikeBypassCD", 731, 733, "b41bae9974ecbcd6e354303310bdfa7c33938750c4a4e6e6dd99c8a814924f1d", |_, c| {
        nm(c.mod_list, "CooldownRecovery", ModType::Override, Value::Num(0.0), CONFIG, vec![skill_name("Vigilant Strike", false)]);
        Ok(()) }),
    e("multiplierWarcryPower", 743, 745, "bd60e9aa086a60eb31f4c24fe261720b2e53c3b250adcfc115dfbf57b8081903", |v, c| {
        nm(c.mod_list, "WarcryPower", ModType::Override, Value::Num(n(Some(v), 744)?), CONFIG, vec![]); Ok(()) }),
    e("enemySizePreset", 787, 801, "b6fbaec2d2ed71f698282dee1ae4b8013116ecdafa8793078809aad034e7adf5", |v, c| {
        // `varControls['enemyRadius']:SetPlaceholder(k, false)` (:789/792/795/798): notify = false skips `changeFunc`
        // (EditControl.lua:111-116) -- the config set's placeholder table is NOT touched; only the mod is state.
        for (size, r) in [("Small", 2.0), ("Medium", 3.0), ("Large", 5.0), ("Huge", 11.0)] {
            if is(v, size) {
                base(c.mod_list, "EnemyRadius", r, CONFIG, vec![]);
                break;
            }
        }
        Ok(()) }),

    // ---- When In Combat --------------------------------------------------------------------------------------------
    e("usePowerCharges", 900, 902, "02ee3e2ba74c8f51f9e0c3c7cb1ed25f751961e206cb7d2caad73a1a1bc8d237", |_, c| {
        flag(c.mod_list, "UsePowerCharges", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("useFrenzyCharges", 906, 908, "5b1e679956d73ed048f299e34e8cfe5f00deed94014034c56132a92defafada8", |_, c| {
        flag(c.mod_list, "UseFrenzyCharges", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("useEnduranceCharges", 912, 914, "a9f56e0cabc817ccc99a7820fdd36be5184cb0ebd720bddde25cf9eabe639fd9", |_, c| {
        flag(c.mod_list, "UseEnduranceCharges", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("multiplierGaleForce", 936, 938, "076bf660f1a305856ebfeca77dcf3f61b1d9b3ff273f504380d57fe0c0f89fa5", |v, c| {
        base(c.mod_list, "Multiplier:GaleForce", n(Some(v), 937)?, CONFIG,
            vec![tag(&[("type", s("IgnoreCond"))]), cond("Combat"), cond("CanGainGaleForce")]);
        Ok(()) }),
    e("repeatMode", 956, 964, "7e50ce2711bee1c4611bb82535fd92f6d3e33ab0df365d0aca8ade1986e0ff8b", |v, c| {
        if is(v, "AVERAGE") {
            flag(c.mod_list, "Condition:averageRepeat", CONFIG, vec![]);
            minion(c.mod_list, minion_mod("Condition:averageRepeat", vec![]));
        } else if is(v, "FINAL") || is(v, "FINAL_DPS") {
            flag(c.mod_list, "Condition:alwaysFinalRepeat", CONFIG, vec![]);
            minion(c.mod_list, minion_mod("Condition:alwaysFinalRepeat", vec![]));
        }
        Ok(()) }),
    e("buffOnslaught", 1010, 1012, "96a9b38ea6e8afcca7b11defc81282d3a5ec2ee5b2e38c1b21db999adde6872a", |_, c| {
        flag(c.mod_list, "Condition:Onslaught", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("buffFortification", 1039, 1041, "d6f3eb7dd239e120b0d40a437ff37f680ef21d4f2394bbcbd32bba578ede220c", |_, c| {
        flag(c.mod_list, "Condition:Fortified", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("buffTailwind", 1045, 1047, "fc23bab8ced0add1d86ee5544d8d80ed0ca775062e6effbedca949aca478e7eb", |_, c| {
        flag(c.mod_list, "Condition:Tailwind", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("buffElusive", 1060, 1063, "e1c5fc727c40ca796f313f8759867fdc17054825386446f12dd25e22df8ca8c2", |_, c| {
        flag(c.mod_list, "Condition:Elusive", CONFIG, vec![cond("Combat"), cond("CanBeElusive")]);
        flag(c.mod_list, "Elusive", CONFIG, vec![cond("Combat"), cond("CanBeElusive")]);
        Ok(()) }),
    e("multiplierRage", 1073, 1075, "35f9f8405316a80be53638301b36c00cd08b0f2063fd8b2b52eb48bc7c4c5421", |v, c| {
        base(c.mod_list, "Multiplier:RageStack", n(Some(v), 1074)?, CONFIG,
            vec![tag(&[("type", s("IgnoreCond"))]), cond("Combat"), cond("CanGainRage")]);
        Ok(()) }),
    e("conditionLeeching", 1079, 1081, "1463f8cd35792fa8e744080c6eecaa426a20589c03396a9aecff0c33e7dd8b60", |_, c| {
        flag(c.mod_list, "Condition:Leeching", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("conditionOnConsecratedGround", 1148, 1151, "2c541522af253ec1f4635c122ee1f13708ad75bc19831c163e3d4c1edc74246c", |_, c| {
        flag(c.mod_list, "Condition:OnConsecratedGround", CONFIG, vec![cond("Combat")]);
        minion(c.mod_list, minion_mod("Condition:OnConsecratedGround", vec![cond("Combat")]));
        Ok(()) }),
    e("multiplierWitheredStackCountSelf", 1231, 1233, "50d46c00b2fa06d5746b37751e2911cc692461d83bce46ced754a01eca8a7ff7", |v, c| {
        base(c.mod_list, "Multiplier:WitheredStack", n(Some(v), 1232)?, CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("conditionCritRecently", 1251, 1254, "92ce5062a47e4219589d8d7fd5bae192a4d73c1e7fb433ed9987913762e1ad52", |_, c| {
        flag(c.mod_list, "Condition:CritRecently", CONFIG, vec![cond("Combat")]);
        flag(c.mod_list, "Condition:SkillCritRecently", CONFIG, vec![cond("Combat")]);
        Ok(()) }),
    e("conditionKilledRecently", 1281, 1283, "726e47245af407bf257c9cfd0c3c54eede41870554d4b370b61e2158751b9294", |_, c| {
        flag(c.mod_list, "Condition:KilledRecently", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("conditionBeenHitRecently", 1375, 1377, "6e20ba4606958e57521f574cd31e9dde09832fe463ae50f08259e455b5d9b943", |_, c| {
        flag(c.mod_list, "Condition:BeenHitRecently", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("conditionBlockedRecently", 1421, 1423, "f5f9edc6769491db3c569f0f76a43abc695ba818f140dac4a8d3efccc49ee28a", |_, c| {
        flag(c.mod_list, "Condition:BlockedRecently", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("conditionChampionIntimidate", 1486, 1488, "6749ab540fcdd971cf4e4e978dd26b2fb6a7e8e1b72d3f801534b3805aa57e5c", |_, c| {
        flag(c.enemy_mod_list, "Condition:ChampionIntimidate", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("conditionSacrificeMinion", 1566, 1568, "125826b42b9f09d61e63986745d32dfe826d4d53727c06841159757bc038675c", |_, c| {
        flag(c.mod_list, "Condition:SacrificeMinionOnAttack", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("conditionUsedWarcryRecently", 1569, 1573, "287d9043f2266b3d02cdcded98d76cbe5ae67b605e8b0f483ca75afde9db81c8", |_, c| {
        flag(c.mod_list, "Condition:UsedWarcryRecently", CONFIG, vec![cond("Combat")]);
        flag(c.mod_list, "Condition:UsedWarcryInPast8Seconds", CONFIG, vec![cond("Combat")]);
        flag(c.mod_list, "Condition:UsedSkillRecently", CONFIG, vec![cond("Combat")]);
        Ok(()) }),
    e("conditionUsedWarcryInPast8Seconds", 1574, 1576, "962b281dfcfd2281eeeec4d0d40b35d8cfbaacda658fb5ff58b2a7ce1b8c9e30", |_, c| {
        flag(c.mod_list, "Condition:UsedWarcryInPast8Seconds", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("conditionConsumedCorpseRecently", 1610, 1612, "1d3cad5bec6276e63949d99d85b9bf5e40ae18d092c670e4a3fffb1c2e343a46", |_, c| {
        flag(c.mod_list, "Condition:ConsumedCorpseRecently", CONFIG, vec![cond("Combat")]); Ok(()) }),
    e("multiplierWarcryUsedRecently", 1623, 1628, "6dc6dd5e314889f3c8ba19ddd4c1076b6cd32fdc66b6d2a20944bbc3234ded16", |v, c| {
        base(c.mod_list, "Multiplier:WarcryUsedRecently", min2(n(Some(v), 1624)?, 100.0), CONFIG, vec![cond("Combat")]);
        flag(c.mod_list, "Condition:UsedWarcryRecently", CONFIG, vec![cond("Combat")]);
        flag(c.mod_list, "Condition:UsedWarcryInPast8Seconds", CONFIG, vec![cond("Combat")]);
        flag(c.mod_list, "Condition:UsedSkillRecently", CONFIG, vec![cond("Combat")]);
        Ok(()) }),
    e("GamblesprintMovementSpeed", 1684, 1686, "d09693450351982f7a4740708619099386ae03e8063ead776be81cbd645937b9", |v, c| {
        nm(c.mod_list, "MovementSpeed", ModType::Inc, Value::Num(n(Some(v), 1685)?), CONFIG, vec![cond("Combat"), cond("HaveGamblesprint")]);
        Ok(()) }),

    // ---- For Effective DPS -----------------------------------------------------------------------------------------
    e("conditionEnemyBleeding", 1812, 1814, "09434b734b11c7236347cae4f88e694a0e281a0a95cc1a81ef2fed7339384abc", |_, c| {
        flag(c.enemy_mod_list, "Condition:Bleeding", CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("multiplierWitheredStackCount", 1839, 1841, "ef7b69a03cabccb062db23c136e3fee65d3f6d42f413e9dc5c91729c71664562", |v, c| {
        base(c.enemy_mod_list, "Multiplier:WitheredStack", n(Some(v), 1840)?, CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("conditionEnemyMaimed", 1851, 1853, "bf1fdd1424d4a257f686680a8b81bf5b2cc9fdb74df62c604605f132fb442e9d", |_, c| {
        flag(c.enemy_mod_list, "Condition:Maimed", CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("conditionEnemyBlinded", 1860, 1862, "b1559c028b389ae3be0c062f47d474fb67d25b24e9a66855dc16bc4485b880a9", |_, c| {
        flag(c.enemy_mod_list, "Condition:Blinded", CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("conditionEnemyTaunted", 1866, 1868, "c3df3c49dc51d1af972d067df58abaa6c26cee68a107d0d5c1e49c30dcddf6da", |_, c| {
        flag(c.enemy_mod_list, "Condition:Taunted", CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("conditionEnemyIgnited", 1878, 1880, "f0ad3bd3d82144bd77049dfb7e4aa006537381b092165becbbb30e8c8b66fc4d", |_, c| {
        flag(c.enemy_mod_list, "Condition:Ignited", CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("ScorchStacks", 1892, 1894, "e2568e7b20393d0dd8dbf46e9bef82ac15382dd667fd974c041696bd0b02c1eb", |v, c| {
        base(c.enemy_mod_list, "Multiplier:ScorchStacks", n(Some(v), 1893)?, CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("conditionEnemyChilled", 1899, 1902, "ee590f9bc502168f0ac261cf0afedffb90e4c630b2484e9a175d4a1f8a0c0d89", |_, c| {
        flag(c.enemy_mod_list, "Condition:Chilled", CONFIG, vec![cond("Effective")]);
        flag(c.enemy_mod_list, "Condition:ChilledConfig", CONFIG, vec![cond("Effective")]);
        Ok(()) }),
    e("ShockStacks", 1951, 1953, "fe1e720ead53e6a40ffb9adfb3561e9c4adb6c6afd2b97018da0dc82754689ee", |v, c| {
        base(c.enemy_mod_list, "Multiplier:ShockStacks", n(Some(v), 1952)?, CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("conditionEnemyIntimidated", 1982, 1984, "e5259654d8fd7e9bd1d1c73607893eead8c078a0a442f6584707acda19fc0708", |_, c| {
        flag(c.enemy_mod_list, "Condition:Intimidated", CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("multiplierEnemyHallowingFlame", 1991, 1993, "17acc07165771490984bb4d6da5323bf224b5c627440f991fa3c5948c4a72d91", |v, c| {
        base(c.enemy_mod_list, "Multiplier:HallowingFlame", n(Some(v), 1992)?, CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("multiplierHallowingFlameStacksRemovedByAlly", 1994, 1996, "73c61da47e3ebf5b614fcc76212c4d476ac0aa3ef29eca05e16d11cf9c8d009f", |v, c| {
        base(c.mod_list, "Multiplier:HallowingFlameStacksRemovedByAlly", n(Some(v), 1995)?, CONFIG, vec![]); Ok(()) }),
    e("conditionEnemyUnnerved", 2003, 2005, "8107201b77e08a42a676a4b9852a9efd91b0c9544746ff8ff39cb8132d73759f", |_, c| {
        flag(c.enemy_mod_list, "Condition:Unnerved", CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("conditionEnemyOnConsecratedGround", 2012, 2014, "f347d42daea2c77680231fb0227164313de6d61e76716ef1491d0636bdf3993c", |_, c| {
        flag(c.enemy_mod_list, "Condition:OnConsecratedGround", CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("enemyConditionHitByLightningDamage", 2054, 2056, "2615840c7ec67f6c2c33ae3c510f0beef25a4c871e3ba58c0566e39b62585b7a", |_, c| {
        flag(c.enemy_mod_list, "Condition:HitByLightningDamage", CONFIG, vec![]); Ok(()) }),
    e("maniaDebuffsCount", 2073, 2079, "d3f3d3dec2eddc2beb592c290f36d41cc3a34851405c9bdd24580fce825c5d8c", |v, c| {
        let val = min2(n(Some(v), 2074)?, 15.0);
        touch_stacks(c.enemy_mod_list, val, &[
            ("DamageTaken", 4.0, "Mania Stacks"),
            ("ActionSpeed", -2.0, "Mania Stacks"),
            ("LifeRecoveryRate", -10.0, "Mania Stacks"),
            ("EnergyShieldRecoveryRate", -10.0, "Mania Stacks"),
        ], "AfflictedByMania");
        Ok(()) }),

    // ---- Enemy Stats -----------------------------------------------------------------------------------------------
    e("conditionEnemyRareOrUnique", 2083, 2085, "bc6a496666e85b5019b67db8ae1554dc60b70311ddb715ad1a2f007e1d8d9a5e", |_, c| {
        flag(c.enemy_mod_list, "Condition:RareOrUnique", CONFIG, vec![cond("Effective")]); Ok(()) }),
    e("enemyIsBoss", 2086, 2222, "0c63f14001520552cd4d5379f9f5636ba8e881470124bbf61cba1ddd6ee5d316", enemy_is_boss),
    e("deliriousPercentage", 2223, 2244, "86434cd5153d9879a5ab0aaee11ef43e5dfe92a0003325043a08b459f2bf0192", delirious),
    e("enemyPhysicalReduction", 2245, 2247, "634a29e9261ef68cb826fc5250d607b2e29f5be2e2b69739e4ba5c8ad927ecee", |v, c| {
        base(c.enemy_mod_list, "PhysicalDamageReduction", n(Some(v), 2246)?, "EnemyConfig", vec![]); Ok(()) }),
    e("enemyLightningResist", 2248, 2250, "4dc0ff09bcdba38aeeab082649222c2ebcf86fe0c608136d2690bdecdeb2e924", |v, c| {
        base(c.enemy_mod_list, "LightningResist", n(Some(v), 2249)?, "EnemyConfig", vec![]); Ok(()) }),
    e("enemyColdResist", 2251, 2253, "b6ba75e088d68118f9285d656bf17bb4d4e5ce0c0a148996f073436248e29f6d", |v, c| {
        base(c.enemy_mod_list, "ColdResist", n(Some(v), 2252)?, "EnemyConfig", vec![]); Ok(()) }),
    e("enemyFireResist", 2254, 2256, "36cd29b3768a0c1e9d8b6195ddc889e801c891d8a68cb7830c5279d696f3e576", |v, c| {
        base(c.enemy_mod_list, "FireResist", n(Some(v), 2255)?, "EnemyConfig", vec![]); Ok(()) }),
    e("enemyChaosResist", 2257, 2259, "cd6772cf0cdabef4ca44bb22f9714e7843bc0912b7a138da00e3845ae66269d8", |v, c| {
        base(c.enemy_mod_list, "ChaosResist", n(Some(v), 2258)?, "EnemyConfig", vec![]); Ok(()) }),
    e("enemyMaxResist", 2260, 2262, "63673d557f60c61107cd5bd0b068845771e99bb12f9f41decf388313914e1b49", |_, c| {
        flag(c.enemy_mod_list, "DoNotChangeMaxResFromConfig", "EnemyConfig", vec![]); Ok(()) }),
    e("enemyBlockChance", 2263, 2265, "6a3b71977c72ac469a9c053d35977aa0bed66065cb9c306d83d0a7dec6e260bd", |v, c| {
        base(c.enemy_mod_list, "BlockChance", n(Some(v), 2264)?, CONFIG, vec![]); Ok(()) }),
    e("enemyEvasion", 2266, 2268, "f77251e205681bd29d074a0779dce259ce2f5d2a778d9fd71fdc3ad56dcc78fa", |v, c| {
        base(c.enemy_mod_list, "Evasion", n(Some(v), 2267)?, CONFIG, vec![]); Ok(()) }),
    e("enemyArmour", 2269, 2271, "e15192b77f669f4e98625a9d907d70bbf4eacb00093960263179aed865ae6bd0", |v, c| {
        base(c.enemy_mod_list, "Armour", n(Some(v), 2270)?, CONFIG, vec![]); Ok(()) }),
    e("presetBossSkills", 2272, 2351, "5ee18b51ad867d8b1a16034a80ab73595ed76988455c03766b7301bf005e7203", preset_boss_skills),
    e("enemyMultiplierPvpDamage", 2363, 2365, "4c62b2235a2d837991bbf68505d967403b7d3af66d5dbd35f438678d1b8a764d", |v, c| {
        base(c.enemy_mod_list, "MultiplierPvpDamage", n(Some(v), 2364)?, CONFIG, vec![]); Ok(()) }),

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
/// The function name in that message is not verified against LuaJIT (no probe case reaches it).
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
