//! CalcPerform.lua:3497-3622 (the strongest non-damaging ailments on the enemy: Chill/Shock/Scorch/Brittle/Sap, then the chill/shock effect
//! multipliers) and :3627-3665 (exposures with Elemental Equilibrium, consecrated ground) -- R6.8, skills on only.
//!
//! * `pairs(ailments)` (:3565) walks a string-keyed table literal: LuaJIT's order is per process (MINEFIELD-RUST R13). Each ailment reads only
//!   its own mods (`<A>Val`/`<A>Base`/`<A>Override`/`<A>Minimum`/`<A>Max`/`Condition:Already<cond>`), so the order only decides where its new
//!   mods land inside the shared enemy buckets (`DamageTaken` INC: Shock / Ahuana's Bite; `Damage` MORE: Sap / Shaper of Winter) -- a multiset
//!   at CP4, a last-bit summation order later (L10). Rust takes the source order (Chill, Shock, Scorch, Brittle, Sap).
//! * :3566 `enemyDB:Sum(...) > 0 or modDB:Sum(...)`: a number is always truthy in Lua (L4), so the left half of the `and` never stops an
//!   ailment -- only the `not (...)` half does; both sums still run (their errors stay errors).
//! * `breakdown` (:3526/:3542) is set in MAIN (MINEFIELD-RUST R8): the stacking Shock/Scorch branches add a second mod there.
//! * `env.minion`: nothing here reads it.

use pob_lua::number::{max2, min2};
use pob_mods::{Mod, ModType, StoreRef, Tag, Value, create_mod};

use super::{base, flag, inc, nm, num_val, or_override, override_, player_main_skill, skill_guard};
use crate::calc_lib;
use crate::env::Env;
use crate::output::{OutValue, Output};
use crate::setup::{cond, n, new_mod, ok, s, tag};

/// The `ailments` table literal of :3499-3563, in source order: name and `condition`.
const AILMENTS: [(&str, &str); 5] = [
    ("Chill", "Chilled"),
    ("Shock", "Shocked"),
    ("Scorch", "Scorched"),
    ("Brittle", "Brittle"),
    ("Sap", "Sapped"),
];

fn cm(name: &str, ty: ModType, value: f64, source: &str, tags: Vec<Tag>) -> Mod {
    create_mod(name, Some(ty), n(value), Some(source), 0, 0, tags)
}

/// Lua truthiness of `output[key]`.
fn out_truthy(output: &Output, key: &str) -> bool {
    !matches!(output.get(key), None | Some(OutValue::Bool(false)))
}

/// `modDB:Override(nil, max) or modDB:Sum("BASE", nil, max)` -- the stack limit of a stacking Shock/Scorch.
fn stacks_max(mod_db: &StoreRef, name: &str, site: &str) -> f64 {
    or_override(override_(mod_db, &[name]), base(mod_db, &[name]), site)
}

/// `{ type = "Multiplier", var = var, limit = limit }`.
fn mult_limit(var: &str, limit: f64) -> Tag {
    tag(&[("type", s("Multiplier")), ("var", s(var)), ("limit", n(limit))])
}

/// `val.mods(num)` of :3500-3561. May write `output.CurrentShock` / `output.CurrentScorch` (stacking).
fn ailment_mods(env: &Env, ailment: &str, num: f64, output: &mut Output, has_guaranteed_bonechill: bool) -> Vec<Mod> {
    let mod_db = &env.mod_db;
    let enemy_db = &env.enemy_db;
    let breakdown = env.build_breakdown;
    match ailment {
        "Chill" => {
            let mut mods = vec![cm("ActionSpeed", ModType::Inc, -num, "Chill", vec![cond("Chilled")])];
            if flag(mod_db, &["ChillEffectIncDamageTaken"]) {
                mods.push(cm("DamageTaken", ModType::Inc, num, "Ahuana's Bite", vec![cond("Chilled")]));
            } else if flag(mod_db, &["ChillEffectIncColdDamageTaken"]) {
                mods.push(cm("ColdDamageTaken", ModType::Inc, num, "Chilled by Hits", vec![cond("Chilled")]));
            } else if flag(mod_db, &["ChillingAreaIncColdDamageTaken"]) {
                mods.push(cm("ColdDamageTaken", ModType::Inc, num, "Chilling Area", vec![cond("Chilled")]));
            } else if out_truthy(output, "HasBonechill") && (has_guaranteed_bonechill || base(enemy_db, &["ChillVal"]) > 0.0) {
                mods.push(cm("ColdDamageTaken", ModType::Inc, num, "Bonechill", vec![cond("Chilled")]));
            }
            if flag(mod_db, &["ChillEffectLessDamageDealt"]) {
                mods.push(cm("Damage", ModType::More, -num / 2.0, "Shaper of Winter", vec![cond("Chilled")]));
            }
            mods
        }
        "Shock" | "Scorch" => {
            // :3522-3533 / :3537-3548 -- the same shape: Shock on `DamageTaken` INC +num, Scorch on `ElementalResist` BASE -num.
            let (stack_flag, stacks, stacks_max_name, name, ty, value, bd_name, line) = if ailment == "Shock" {
                (
                    "ShockCanStack",
                    "ShockStacks",
                    "ShockStacksMax",
                    "DamageTaken",
                    ModType::Inc,
                    num,
                    "DamageTakenByShock",
                    3525,
                )
            } else {
                (
                    "ScorchCanStack",
                    "ScorchStacks",
                    "ScorchStacksMax",
                    "ElementalResist",
                    ModType::Base,
                    -num,
                    "ElementalResistByScorch",
                    3541,
                )
            };
            let condition = if ailment == "Shock" { "Shocked" } else { "Scorched" };
            let mut mods = Vec::new();
            if flag(mod_db, &[stack_flag]) {
                let site = format!("Modules/CalcPerform.lua:{line}");
                let limit = stacks_max(mod_db, stacks_max_name, &site);
                mods.push(cm(name, ty, value, ailment, vec![cond(condition), mult_limit(stacks, limit)]));
                let have = base(enemy_db, &[&format!("Multiplier:{stacks}")]);
                let limit = stacks_max(mod_db, stacks_max_name, &site);
                output.set_num(&format!("Current{ailment}"), num * min2(have, limit));
                if breakdown {
                    let limit = stacks_max(mod_db, stacks_max_name, &site);
                    let src = format!("{ailment} Stacks");
                    mods.push(cm(bd_name, ty, value, &src, vec![cond(condition), mult_limit(stacks, limit)]));
                }
            } else {
                mods.push(cm(name, ty, value, ailment, vec![cond(condition)]));
            }
            mods
        }
        "Brittle" => vec![cm("SelfCritChance", ModType::Base, num, "Brittle", vec![cond("Brittle")])],
        "Sap" => vec![cm("Damage", ModType::More, -num, "Sap", vec![cond("Sapped")])],
        _ => unreachable!("ailment {ailment}"),
    }
}

/// :3497-3622 -- apply the strongest non-damaging ailments, then update the chill and shock multipliers.
pub(super) fn ailments(env: &mut Env, output: &mut Output, has_guaranteed_bonechill: bool) {
    let mod_db = env.mod_db.clone();
    let enemy_db = env.enemy_db.clone();
    let ms = player_main_skill(env, 3583);
    for (ailment, condition) in AILMENTS {
        let data = pob_data::rules::rules()
            .non_damaging_ailment
            .get(ailment)
            .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:3601: data.nonDamagingAilment.{ailment} is nil"));
        let (base_n, override_n, minimum_n) = (format!("{ailment}Base"), format!("{ailment}Override"), format!("{ailment}Minimum"));
        // :3566 `enemyDB:Sum(...) > 0 or modDB:Sum(...)` -- the right side is a number: always truthy (L4).
        if base(&enemy_db, &[&format!("{ailment}Val")]) <= 0.0 {
            let _ = base(&mod_db, &[&base_n, &override_n, &minimum_n]);
        }
        // :3568 `and not (A or B or C)`
        let blocked = flag(&enemy_db, &[&format!("Condition:Already{condition}")])
            || flag(&enemy_db, &[&format!("{ailment}Immune"), "ElementalAilmentImmune"])
            || base(&enemy_db, &[&format!("Avoid{ailment}"), "AvoidAilments", "AvoidElementalAilments"]) >= 100.0;
        if blocked {
            continue;
        }
        let mut override_v = 0.0;
        let mut minimum = 0.0;
        let rows = ok(mod_db
            .borrow()
            .tabulate(Some(ModType::Base), None, &[&base_n, &override_n, &minimum_n]));
        for row in rows {
            let m = &row.mod_;
            let mut effect = num_val(&m.value, "Modules/CalcPerform.lua:3584");
            let scales_with_source = *m.name == *base_n || *m.name == *minimum_n;
            if *m.name == *override_n {
                new_mod(
                    &enemy_db,
                    &format!("Condition:{condition}"),
                    ModType::Flag,
                    Value::Bool(true),
                    m.source.as_deref(),
                    0,
                    0,
                    vec![],
                );
            }
            if scales_with_source {
                // :3583 the main skill's ailment modifiers when it can inflict the ailment with a hit and `ChecksHighestDamage` is on.
                let effect_n = format!("Enemy{ailment}Effect");
                let use_skill = ailment != "Scorch"
                    && ailment != "Sap"
                    && ailment != "Brittle"
                    && {
                        let cannot = format!("Cannot{ailment}");
                        ms.guard(&[&cannot]);
                        ok(ms.list.borrow().flag(None, &[&cannot])) != Some(true)
                    }
                    && ms.flag("hit")
                    && flag(&mod_db, &["ChecksHighestDamage"]);
                if use_skill {
                    ms.guard(&[&effect_n]);
                    effect *= ok(calc_lib::mod_(&ms.list.borrow(), None, &[&effect_n]));
                } else {
                    effect *= ok(calc_lib::mod_(&mod_db.borrow(), None, &[&effect_n]));
                }
            }
            effect *= ok(calc_lib::mod_(&enemy_db.borrow(), None, &[&format!("Self{ailment}Effect")]));
            if scales_with_source {
                // `NewMod(name, "BASE", effect, mod.source, mod.flags, mod.keywordFlags, unpack(mod))` -- the same tag objects.
                mod_db.borrow_mut().new_mod(create_mod(
                    &override_n,
                    Some(ModType::Base),
                    n(effect),
                    m.source.as_deref(),
                    m.flags,
                    m.keyword_flags,
                    m.tags.clone(),
                ));
                if *m.name == *minimum_n {
                    minimum += effect;
                }
            }
            override_v = max2(max2(override_v, effect), minimum);
        }
        // :3598-3604 the highest maximum over the skills, unless overridden.
        let max_n = format!("{ailment}Max");
        let max_override = override_(&mod_db, &[&max_n]);
        let mut max_ailment = or_override(max_override.clone(), 0.0, "Modules/CalcPerform.lua:3598");
        if !max_override.as_ref().is_some_and(Value::truthy)
            && let Some(st) = &env.skills
        {
            for sk in &st.active_skill_list {
                let list = sk
                    .skill_mod_list
                    .as_ref()
                    .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:3601: activeSkill.baseSkillModList is nil"));
                skill_guard(&sk.malformed_mods, &[&max_n]);
                let skill_max = data.max + ok(list.borrow().sum(ModType::Base, None, &[&max_n]));
                max_ailment = if skill_max > max_ailment { skill_max } else { max_ailment };
            }
        }
        output.set_num(&format!("Maximum{ailment}"), max_ailment);
        let precision = data
            .precision
            .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:3606: attempt to perform arithmetic on field 'precision' (a nil value)"));
        let scale = 10f64.powf(precision);
        let val = base(&enemy_db, &[&format!("{ailment}Val")]);
        let current = (min2(max2(override_v, val), max_ailment) * scale).floor() / scale;
        output.set_num(&format!("Current{ailment}"), current);
        for m in ailment_mods(env, ailment, current, output, has_guaranteed_bonechill) {
            enemy_db.borrow_mut().new_mod(m);
        }
        // :3610 the source slot holds a tag table, so the mod has no source.
        new_mod(
            &enemy_db,
            &format!("Condition:Already{condition}"),
            ModType::Flag,
            Value::Bool(true),
            None,
            0,
            0,
            vec![cond(condition)],
        );
    }

    // :3615-3622 update chill and shock multipliers
    for (cur_key, mult) in [
        ("CurrentChill", "Multiplier:ChillEffect"),
        ("CurrentShock", "Multiplier:ShockEffect"),
    ] {
        let have = base(&enemy_db, &[mult]);
        if let Some(cur) = output.opt_num(cur_key)
            && have < cur
        {
            nm(&enemy_db, mult, ModType::Base, cur - have, "", 0, 0, vec![]);
        }
    }
}

/// `treeVersion:match("(%d+)_(%d+)")` -- the first `<digits>_<digits>`.
fn tree_major_minor(v: &str) -> Option<(f64, f64)> {
    let b = v.as_bytes();
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    for i in 0..b.len() {
        let d1 = digits(i);
        if d1 == 0 || b.get(i + d1) != Some(&b'_') {
            continue;
        }
        let d2 = digits(i + d1 + 1);
        if d2 == 0 {
            continue;
        }
        // tonumber of a digit string: exact for the short versions PoB has.
        let major: f64 = v[i..i + d1].parse().expect("digits");
        let minor: f64 = v[i + d1 + 1..i + d1 + 1 + d2].parse().expect("digits");
        return Some((major, minor));
    }
    None
}

/// :3627-3665 exposures (with Elemental Equilibrium) and consecrated ground on the enemy.
pub(super) fn exposures_consecrated(env: &mut Env) {
    let mod_db = env.mod_db.clone();
    let enemy_db = env.enemy_db.clone();
    let version = tree_major_minor(&env.spec.tree_version);
    for element in ["Fire", "Cold", "Lightning"] {
        // :3631 `tonumber(major) <= 3 and tonumber(minor) <= 15 or ...`
        let (major, minor) = version.unwrap_or_else(|| panic!("Modules/CalcPerform.lua:3631: attempt to compare nil with number"));
        let applies = major <= 3.0 && minor <= 15.0
            || !flag(&mod_db, &["ElementalEquilibrium"])
            || element == "Fire" && !flag(&enemy_db, &["Condition:HitByFireDamage"])
            || element == "Cold" && !flag(&enemy_db, &["Condition:HitByColdDamage"])
            || element == "Lightning" && !flag(&enemy_db, &["Condition:HitByLightningDamage"]);
        if !applies {
            continue;
        }
        let mut min = f64::INFINITY;
        let mut source: Option<pob_mods::Str> = Some(std::rc::Rc::from(""));
        let rows = ok(enemy_db
            .borrow()
            .tabulate(Some(ModType::Base), None, &[&format!("{element}Exposure")]));
        for row in rows {
            let v = num_val(&row.value, "Modules/CalcPerform.lua:3639");
            if v < min {
                min = v;
                source = row.mod_.source.clone();
            }
        }
        if min != f64::INFINITY {
            let rows = ok(mod_db
                .borrow()
                .tabulate(Some(ModType::Base), None, &["ExtraExposure", &format!("Extra{element}Exposure")]));
            for row in rows {
                min += num_val(&row.value, "Modules/CalcPerform.lua:3647");
            }
            let exposure_effect_inc = inc(&mod_db, &["ExposureEffect", &format!("{element}ExposureEffect")]);
            if exposure_effect_inc != 0.0 {
                min *= 1.0 + exposure_effect_inc / 100.0;
            }
            new_mod(
                &enemy_db,
                &format!("Condition:Has{element}Exposure"),
                ModType::Flag,
                Value::Bool(true),
                Some(""),
                0,
                0,
                vec![],
            );
            // `m_min(min, modDB:Override(nil, "ExposureMin"))`: no override is Lua's "no values" (L26) -> `m_min(min)`.
            let value = match override_(&mod_db, &["ExposureMin"]) {
                None => min,
                Some(v) => min2(min, num_val(&v, "Modules/CalcPerform.lua:3655")),
            };
            new_mod(
                &enemy_db,
                &format!("{element}Resist"),
                ModType::Base,
                n(value),
                source.as_deref(),
                0,
                0,
                vec![],
            );
            new_mod(
                &mod_db,
                "Condition:AppliedExposureRecently",
                ModType::Flag,
                Value::Bool(true),
                Some(""),
                0,
                0,
                vec![],
            );
        }
    }

    // :3661-3665 consecrated ground effects on enemies
    if flag(&enemy_db, &["Condition:OnConsecratedGround"]) {
        let effect = 1.0 + inc(&mod_db, &["ConsecratedGroundEffect"]) / 100.0;
        let v = (inc(&enemy_db, &["DamageTakenConsecratedGround"]) * effect).floor();
        nm(&enemy_db, "DamageTaken", ModType::Inc, v, "Consecrated Ground", 0, 0, vec![]);
    }
}
