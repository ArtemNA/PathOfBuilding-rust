//! CalcOffence.lua:5771-6263 (R7.7, part E): the skill damage over time (`dotCfg` :5771-5804, `preDotFunc` :5806, the DoT per damage
//! type :5808-5859, stacking / ground DoTs :5861-5914), the per-second cost of upfront costs (:5916-5957), the self hit damage
//! (:5959-6105, with `calcs.applyDmgTakenConversion` CalcDefence.lua:84-142) and the combined DPS (:6107-6262). OC6 is taken at the end.
//!
//! Hash-order walks (R13/L10):
//! * `for _, sourceFunc in pairs(nameToHandler)` (:6077) -- seven STRING keys, LuaJIT's order differs between processes. Each handler only
//!   reads; the one shared write is the sum `output.SelfHitDamage = (output.SelfHitDamage or 0) + dmgTaken`: with at most two handlers
//!   returning a value the sum is order-free (`0 + a + b` = `0 + b + a`); with three or more it is a float sum in an unknown order --
//!   Rust walks the constructor order and records the case (MINEFIELD-RUST R13: a fixed order, the class is named).
//! * `for selfHitSkill, displayName in ipairs({["FRDamageTaken"] = "Forbidden Rite"})` (:6091) -- `ipairs` over a table with no array
//!   part: the loop body never runs (upstream as written).
//! * `for pool, costResource in pairs(...)` is the buildOutput tail ([`crate::session`]).
//!
//! `activeSkill.mirage` (:6213) is set by `calcs.mirages` (CalcMirages.lua:76/:302, [`crate::mirages`], R7.9) before offence runs: the
//! mirage block (:6212-6248) adds the mirage run's DPS to the combined DPS.
//!
//! Breakdown (R8): every `if breakdown` branch of this part only fills breakdown tables; the queries their conditions make
//! (`Flag(nil, "Enemy<T>ResistEqualToYours")` :5835, `Sum("BASE", skillCfg, "Multiplier:SustainableTraumaStacks")` :6050) are still made.
//! `activeSkill.dotCfg` / `activeSkill["dot<T>Cfg"]` (:5803/:5812) are read nowhere else upstream (`grep`): they live as locals.

use pob_lua::number::{max2, min2, round};
use pob_mods::{ModCfg, ModType, Value};

use super::c::{LUA, dflag, dmore, dsum, num_at, or_num};
use super::c2::resist_for_type;
use super::helpers::{DMG_TYPE_LIST, Q};
use super::{Off, run_skill_func};
use crate::output::OutValue;
use crate::setup::{keyword_flag, misc, mod_flag};

/// `isElemental` (:31).
fn is_elemental(t: &str) -> bool {
    matches!(t, "Fire" | "Cold" | "Lightning")
}

/// `output[k] or 0` (`false` is falsy too).
fn g0(off: &Off<'_>, k: &str, line: u32) -> f64 {
    or_num(off.gval(k), 0.0, k, line)
}

/// `output[k]` in arithmetic.
fn gn(off: &Off<'_>, k: &str, line: u32) -> f64 {
    num_at(off.gval(k), k, line)
}

/// `output[k]` truthy.
fn gt(off: &Off<'_>, k: &str) -> bool {
    !matches!(off.gval(k), None | Some(OutValue::Bool(false)))
}

/// `output[k] = v` for a value read off `output` (nil removes the key).
fn gset_val(off: &mut Off<'_>, k: &str, v: Option<OutValue>) {
    off.set_out_value(k, v);
}

/// `x < y` for an output value (nil / non-number -> the Lua compare error).
fn lt(v: Option<&OutValue>, y: f64, k: &str, line: u32) -> bool {
    match v {
        Some(OutValue::Num(x)) => *x < y,
        other => panic!("{LUA}:{line}: attempt to compare {other:?} ({k}) with number"),
    }
}

/// :5771-6263 -- everything after the ailments.
pub(super) fn dot_cost_combined(off: &mut Off<'_>, quantity_multiplier: f64) {
    let q = off.q.clone();
    skill_dot(off, &q, quantity_multiplier);
    per_second_costs(off, &q);
    self_hit(off, &q);
    combined_dps(off, quantity_multiplier);
}

/// :5771-5914 the skill DoT.
#[allow(clippy::too_many_lines)]
fn skill_dot(off: &mut Off<'_>, q: &Q, quantity_multiplier: f64) {
    let sc = off.cfg.clone();
    let site_bor = || -> ! { panic!("{LUA}:5778: bad argument #2 to 'bor' (number expected, got nil)") };
    let sflags = sc.flags.unwrap_or_else(|| site_bor()) as u32;
    let skw = sc
        .keyword_flags
        .unwrap_or_else(|| panic!("{LUA}:5779: bad argument #1 to 'band' (number expected, got nil)")) as u32;
    // :5772-5795
    let mut flags = mod_flag("Dot") | sflags;
    let kw = skw & !keyword_flag("Hit");
    for (f, k) in [
        ("Area", "dotIsArea"),
        ("Projectile", "dotIsProjectile"),
        ("Spell", "dotIsSpell"),
        ("Attack", "dotIsAttack"),
        ("Hit", "dotIsHit"),
    ] {
        if flags & mod_flag(f) == mod_flag(f) && !off.sd_truthy(k) {
            flags &= !mod_flag(f);
        }
    }
    let dot_cfg = ModCfg {
        skill_name: sc.skill_name.clone(),
        skill_part: sc.skill_part,
        skill_types: sc.skill_types.clone(),
        summon_skill_name: sc.summon_skill_name.clone(),
        slot_name: sc.slot_name.clone(),
        flags: Some(f64::from(flags)),
        keyword_flags: Some(f64::from(kw)),
        ..ModCfg::default()
    };
    // :5798-5801
    let mut dot_taken_cfg = dot_cfg.clone();
    if off.sd_truthy("dotIsSpell") {
        dot_taken_cfg.flags = Some(f64::from(flags & !mod_flag("Spell")));
    }
    // :5804
    off.gset("TotalDotInstance", 0.0);
    // :5806
    run_skill_func(off, "preDotFunc");
    let edb = off.enemy_db.clone();
    let cap = misc("DotDpsCap");
    // :5809-5859
    for dt in DMG_TYPE_LIST {
        let dot_type_cfg = ModCfg {
            keyword_flags: Some(f64::from(kw | keyword_flag(&format!("{dt}Dot")))),
            ..dot_cfg.clone()
        };
        let dot_key = format!("{dt}Dot");
        let base_val = if off.can_deal.get(dt).copied().unwrap_or(false) {
            off.sd_num(&dot_key, 5815).unwrap_or(0.0)
        } else {
            0.0
        };
        if !(base_val > 0.0 || g0(off, &dot_key, 5819) > 0.0) {
            continue;
        }
        off.set_flag("dot", true);
        let mut eff_mult = 1.0;
        if off.env.mode_effective {
            let (a, b) = (format!("{dt}DamageTaken"), format!("{dt}DamageTakenOverTime"));
            let names = ["DamageTaken", "DamageTakenOverTime", a.as_str(), b.as_str()];
            let mut taken_inc = dsum(&edb, ModType::Inc, Some(&dot_taken_cfg), &names);
            if is_elemental(dt) {
                taken_inc += dsum(&edb, ModType::Inc, Some(&dot_taken_cfg), &["ElementalDamageTaken"]);
            }
            let mut taken_more = dmore(&edb, Some(&dot_taken_cfg), &names);
            if is_elemental(dt) {
                taken_more *= dmore(&edb, Some(&dot_taken_cfg), &["ElementalDamageTaken"]);
            }
            let resist = if dt == "Physical" {
                max2(
                    0.0,
                    min2(
                        dsum(&edb, ModType::Base, None, &["PhysicalDamageReduction"]),
                        misc("EnemyPhysicalDamageReductionCap"),
                    ),
                )
            } else {
                resist_for_type(off, dt, Some(&dot_type_cfg))
            };
            eff_mult = (1.0 - resist / 100.0) * (1.0 + taken_inc / 100.0) * taken_more;
            off.gset(&format!("{dt}DotEffMult"), eff_mult);
            if off.breakdown && eff_mult != 1.0 {
                // :5835 `sourceRes` (breakdown text only; the queries are made)
                let f = format!("Enemy{dt}ResistEqualToYours");
                if !dflag(&off.mod_db, None, &[&f]) {
                    let _ = dflag(&off.st.party_members.mod_db, None, &[&f]);
                }
            }
        }
        let el = is_elemental(dt);
        let dmg = format!("{dt}Damage");
        let mut names = vec!["Damage", dmg.as_str()];
        if el {
            names.push("ElementalDamage");
        }
        let mut inc = q.inc(Some(&dot_type_cfg), &names);
        if q.flag(None, &["dotIsHeraldOfAsh"]) {
            inc = max2(inc - q.inc(Some(&sc), &names), 0.0);
        }
        let more = q.more(Some(&dot_type_cfg), &names);
        let site = format!("{LUA}:5845");
        let mult = q.override_num(Some(&dot_type_cfg), &["DotMultiplier"], &site).unwrap_or_else(|| {
            q.base(Some(&dot_type_cfg), &["DotMultiplier"]) + q.base(Some(&dot_type_cfg), &[&format!("{dt}DotMultiplier")])
        });
        let aura = if off.has_type("Aura") && !off.has_type("RemoteMined") && !off.has_type("Banner") {
            q.calc_mod(Some(&dot_type_cfg), &["AuraEffect"])
        } else {
            1.0
        };
        let total = base_val * (1.0 + inc / 100.0) * more * (1.0 + mult / 100.0) * aura * eff_mult;
        let tdi = gn(off, "TotalDotInstance", 5850);
        let cur = off.gval(&dot_key).cloned();
        let unset = match &cur {
            None | Some(OutValue::Bool(false)) => true,
            Some(OutValue::Num(x)) => *x == 0.0,
            _ => false,
        };
        if unset {
            off.gset(&dot_key, total);
            off.gset("TotalDotInstance", min2(tdi + total, cap));
        } else {
            let prev = or_num(cur.as_ref(), 0.0, &dot_key, 5852);
            off.gset("TotalDotInstance", min2(tdi + total + prev, cap));
        }
    }
    // :5861-5914
    if q.flag(None, &["DotCanStack"]) {
        off.set_flag("DotCanStack", true);
        let key = if kw & keyword_flag("Mine") != 0 {
            "MineLayingSpeed"
        } else if kw & keyword_flag("Trap") != 0 {
            "TrapThrowingSpeed"
        } else {
            "Speed"
        };
        let speed = gn(off, key, 5871);
        let dps_mult = off
            .sd_num("dpsMultiplier", 5871)
            .unwrap_or_else(|| panic!("{LUA}:5871: attempt to perform arithmetic on field 'dpsMultiplier' (a nil value)"));
        let v = min2(
            gn(off, "TotalDotInstance", 5871) * speed * gn(off, "Duration", 5871) * dps_mult * quantity_multiplier,
            cap,
        );
        off.gset("TotalDot", v);
        off.gset("TotalDotCalcSection", v);
    } else if q.flag(None, &["dotIsBurningGround"]) {
        ground(off, "BurningGroundDPS", Some("BurningGroundFromIgnite"), 5890);
    } else if q.flag(None, &["dotIsCausticGround"]) {
        ground(off, "CausticGroundDPS", Some("CausticGroundFromPoison"), 5897);
    } else if q.flag(None, &["dotIsCorruptingBlood"]) {
        ground(off, "CorruptingBloodDPS", None, 5904);
    } else {
        if q.flag(None, &["DotCanStackAsTotems"]) && off.flag("totem") {
            off.set_flag("DotCanStack", true);
        }
        // `activeSkill.skillTypes[SkillType.Brand] and not skillData.countsAttachedBrandsInDamage and output.AttachedBrandCount or 1`
        let abc = if off.has_type("Brand") && !off.sd_truthy("countsAttachedBrandsInDamage") && gt(off, "AttachedBrandCount") {
            gn(off, "AttachedBrandCount", 5912)
        } else {
            1.0
        };
        let tdi = gn(off, "TotalDotInstance", 5912);
        let v = if abc > 1.0 { min2(tdi * abc, cap) } else { tdi };
        off.gset("TotalDot", v);
        off.gset("TotalDotCalcSection", v);
    }
}

/// :5888-5906 a ground / corrupting blood DoT: `TotalDot = 0`, the instance goes to `<key>` when larger.
fn ground(off: &mut Off<'_>, key: &str, from_flag: Option<&str>, line: u32) {
    off.gset("TotalDot", 0.0);
    let tdi = gn(off, "TotalDotInstance", line - 1);
    off.gset("TotalDotCalcSection", tdi);
    let cur = off.gval(key).cloned();
    let replace = match &cur {
        None | Some(OutValue::Bool(false)) => true,
        other => lt(other.as_ref(), tdi, key, line),
    };
    if replace {
        let prev = or_num(cur.as_ref(), 0.0, key, line + 1);
        off.gset(key, max2(prev, tdi));
        if let Some(f) = from_flag {
            off.out().set_bool(f, false);
        }
    }
}

/// `costs.order` and `costs[resource].upfront` (:1693-1708).
const COSTS_ORDER: [(&str, bool); 13] = [
    ("Mana", true),
    ("Life", true),
    ("ES", true),
    ("Soul", true),
    ("Rage", true),
    ("ManaPercent", true),
    ("LifePercent", true),
    ("ManaPerMinute", false),
    ("LifePerMinute", false),
    ("ManaPercentPerMinute", false),
    ("LifePercentPerMinute", false),
    ("ESPerMinute", false),
    ("ESPercentPerMinute", false),
];

/// :5917-5957 the per-second cost of the upfront costs (breakdown rows excluded).
fn per_second_costs(off: &mut Off<'_>, q: &Q) {
    let sc = off.cfg.clone();
    for (resource, upfront) in COSTS_ORDER {
        // :5919
        let eb = off.mod_db.borrow().flag(None, &["EnergyShieldProtectsMana"]);
        let eb = crate::setup::ok(eb) == Some(true);
        // :5920
        if !upfront || !gt(off, &format!("{resource}HasCost")) {
            continue;
        }
        let cost_key = format!("{resource}Cost");
        if gn(off, &cost_key, 5920) <= 0.0 {
            continue;
        }
        if gt(off, &format!("{resource}PerSecondHasCost")) {
            // `not (EB and Sum(...))`: the Sum is a number (truthy), so this is `not EB`; the Sum is made when EB holds
            if !eb {
                continue;
            }
            let _ = q.base(Some(&sc), &["ManaCostAsEnergyShieldCost"]);
        }
        let speed_ok = match off.gval("Speed") {
            Some(OutValue::Num(s)) => *s > 0.0,
            other => panic!("{LUA}:5920: attempt to compare {other:?} (Speed) with number"),
        };
        if !(speed_ok || gt(off, "Cooldown")) {
            continue;
        }
        let used = if eb && resource == "Mana" { "ES" } else { resource };
        let repeats = or_num(off.gval("Repeats"), 1.0, "Repeats", 5927);
        let use_speed = if off.flag("trap") || off.flag("mine") {
            // :5931-5933
            let pre = if gt(off, "TrapThrowingSpeed") {
                off.gval("TrapThrowingSpeed").cloned()
            } else {
                off.gval("MineLayingSpeed").cloned()
            };
            let cd = if gt(off, "TrapCooldown") {
                off.gval("TrapCooldown").cloned()
            } else {
                off.gval("Cooldown").cloned()
            };
            match cd {
                Some(OutValue::Num(c)) if c > 0.0 => 1.0 / c,
                Some(OutValue::Num(_)) | None | Some(OutValue::Bool(false)) => num_at(pre.as_ref(), "preSpeed", 5948),
                other => panic!("{LUA}:5933: attempt to compare {other:?} (cooldown) with number"),
            }
        } else if off.flag("totem") {
            // :5936
            let v = if cooldown_positive(off, 5936) {
                let tps = gn(off, "TotemPlacementSpeed", 5936);
                if tps > 0.0 { tps } else { 1.0 / gn(off, "Cooldown", 5936) }
            } else {
                gn(off, "TotemPlacementSpeed", 5936)
            };
            v / repeats
        } else if q.flag(None, &["HasSeals"]) && q.flag(None, &["UseMaxUnleash"]) && off.sd_truthy("hitTimeOverride") {
            // :5939-5940 `env.player.mainSkill` is this skill (the player's offence)
            off.sd_num("hitTimeOverride", 5940).expect("truthy") / repeats
        } else {
            // :5943
            let v = if cooldown_positive(off, 5943) {
                let s = gn(off, "Speed", 5943);
                if s > 0.0 { s } else { 1.0 / gn(off, "Cooldown", 5943) }
            } else {
                gn(off, "Speed", 5943)
            };
            v / repeats
        };
        // :5947-5948
        off.out().set_bool(&format!("{used}PerSecondHasCost"), true);
        let ps_key = format!("{used}PerSecondCost");
        let v = g0(off, &ps_key, 5948) + gn(off, &cost_key, 5948) * use_speed;
        off.gset(&ps_key, v);
    }
}

/// `output.Cooldown and output.Cooldown > 0`.
fn cooldown_positive(off: &Off<'_>, line: u32) -> bool {
    match off.gval("Cooldown") {
        None | Some(OutValue::Bool(false)) => false,
        Some(OutValue::Num(c)) => *c > 0.0,
        other => panic!("{LUA}:{line}: attempt to compare {other:?} (Cooldown) with number"),
    }
}

/// `string.gsub(" "..s, "%W%l", string.upper):sub(2)` (ASCII, LuaJIT's `%W`/`%l` classes).
fn title_words(s: &str) -> String {
    let mut b: Vec<u8> = Vec::with_capacity(s.len() + 1);
    b.push(b' ');
    b.extend_from_slice(s.as_bytes());
    let mut i = 0;
    while i + 1 < b.len() {
        if !b[i].is_ascii_alphanumeric() && b[i + 1].is_ascii_lowercase() {
            b[i + 1] = b[i + 1].to_ascii_uppercase();
            i += 2;
        } else {
            i += 1;
        }
    }
    String::from_utf8_lossy(&b[1..]).into_owned()
}

/// `calcs.applyDmgTakenConversion(activeSkill, output, breakdown, sourceType, baseDmg)` (CalcDefence.lua:84-142) -> the total damage
/// taken (the breakdown lines are R8).
pub(super) fn apply_dmg_taken_conversion(off: &Off<'_>, q: &Q, source: &str, base_dmg: f64) -> f64 {
    const CD: &str = "Modules/CalcDefence.lua";
    let el = is_elemental(source);
    // :88-92
    let mut shift = [0.0; 5];
    let mut total_taken_as = 0.0;
    for (i, dt) in DMG_TYPE_LIST.iter().enumerate() {
        let a = format!("{source}DamageTakenAs{dt}");
        let b = format!("{source}DamageFromHitsTakenAs{dt}");
        let c = format!("ElementalDamageTakenAs{dt}");
        let d = format!("ElementalDamageFromHitsTakenAs{dt}");
        let mut names = vec![a.as_str(), b.as_str()];
        if el {
            names.push(c.as_str());
            names.push(d.as_str());
        }
        shift[i] = q.base(None, &names);
        total_taken_as += shift[i];
    }
    let mut total = 0.0;
    // :93-140
    for (i, dt) in DMG_TYPE_LIST.iter().enumerate() {
        let taken_as = if *dt == source {
            max2(1.0 - total_taken_as / 100.0, 0.0)
        } else {
            shift[i] / 100.0
        };
        if taken_as == 0.0 {
            continue;
        }
        let damage = base_dmg * taken_as;
        let (a, b) = (format!("{dt}DamageTaken"), format!("{dt}DamageTakenWhenHit"));
        let names = ["DamageTaken", a.as_str(), "DamageTakenWhenHit", b.as_str()];
        let mut inc = q.inc(None, &names);
        let mut more = q.more(None, &["DamageTaken", a.as_str(), "DamageTakenWhenHit", b.as_str()]);
        if matches!(*dt, "Lightning" | "Cold" | "Fire") {
            inc += q.inc(None, &["ElementalDamageTaken", "ElementalDamageTakenWhenHit"]);
            more *= q.more(None, &["ElementalDamageTaken", "ElementalDamageTakenWhenHit"]);
        }
        let taken_mods = max2((1.0 + inc / 100.0) * more, 0.0);
        // `Flag(...) and 0 or output[a] or output[b]`
        let first_or = |k1: &str, k2: &str, line: u32| -> f64 {
            match off.gval(k1) {
                None | Some(OutValue::Bool(false)) => num_at(off.gval(k2), k2, line),
                v => num_at(v, k1, line),
            }
        };
        let reduction = if q.flag(None, &[&format!("SelfIgnoreBase{dt}DamageReduction")]) {
            0.0
        } else {
            first_or(&format!("Base{dt}DamageReductionWhenHit"), &format!("Base{dt}DamageReduction"), 124)
        };
        let resist = if q.flag(None, &[&format!("SelfIgnore{dt}Resistance")]) {
            0.0
        } else {
            first_or(&format!("{dt}ResistWhenHit"), &format!("{dt}Resist"), 106)
        };
        let mut armour_reduct = 0.0;
        let res_mult = 1.0 - resist / 100.0;
        let pct = min2(
            if q.flag(None, &[&format!("ArmourDoesNotApplyTo{dt}DamageTaken")]) {
                0.0
            } else {
                q.base(None, &[&format!("ArmourAppliesTo{dt}DamageTaken")])
            },
            100.0,
        );
        let ward_based = *dt == "Physical" && q.flag(None, &["PhysicalReductionBasedOnWard"]);
        let dr_max = num_at(off.gval("DamageReductionMax"), "DamageReductionMax", 122);
        if pct > 0.0 || ward_based {
            let mut eff_armour =
                (num_at(off.gval("Armour"), "Armour", 113) * pct / 100.0) * (1.0 + num_at(off.gval("ArmourDefense"), "ArmourDefense", 113));
            if ward_based {
                let m = q
                    .override_num(None, &["PhysicalReductionBasedOnWardPercent"], &format!("{CD}:115"))
                    .unwrap_or_else(|| panic!("{CD}:115: attempt to perform arithmetic on a nil value"))
                    / 100.0;
                eff_armour = num_at(off.gval("Ward"), "Ward", 116) * m;
            }
            let eff_damage = damage * res_mult;
            armour_reduct = round(
                if eff_armour != 0.0 && damage * res_mult != 0.0 {
                    eff_armour / (eff_armour + eff_damage * 5.0) * 100.0
                } else {
                    0.0
                },
                None,
            );
            armour_reduct = min2(dr_max, armour_reduct);
        }
        let reduct_mult = (1.0 - max2(min2(dr_max, armour_reduct + reduction), 0.0) / 100.0) * taken_mods;
        let combined = res_mult * reduct_mult;
        total += damage * combined;
    }
    total
}

/// `for _, value in ipairs(skillModList:List(nil, name))` -> `(baseDamage or dmgMult, damageType)` of each mod value.
fn self_damage_mods(q: &Q, name: &str, field: &str) -> Vec<(Value, String)> {
    q.list(None, &[name])
        .into_iter()
        .map(|v| {
            let ty = v.get("damageType");
            let ty = ty
                .as_str()
                .unwrap_or_else(|| panic!("{LUA}:5967: attempt to concatenate field 'damageType' ({})", ty.type_name()))
                .to_owned();
            (v.get(field), title_words(&ty))
        })
        .collect()
}

fn val_num(v: &Value, what: &str, line: u32) -> f64 {
    v.as_num().unwrap_or_else(|| {
        panic!(
            "{LUA}:{line}: attempt to perform arithmetic on field '{what}' (a {} value)",
            v.type_name()
        )
    })
}

/// :5960-6105 the self hit damage (the breakdown lines are R8).
#[allow(clippy::too_many_lines)]
fn self_hit(off: &mut Off<'_>, q: &Q) {
    let name = {
        let st = off.env.skills.as_ref().expect("skills on");
        st.skills.effects[off.sk().active_effect.borrow().granted_effect].name.clone()
    };
    let mut taken: Vec<f64> = Vec::new();
    // "Heartbound Loop" (:5963-5977)
    if name == "Summon Skeletons" {
        let mut acc: Option<(f64, String)> = None;
        for (v, ty) in self_damage_mods(q, "HeartboundLoopSelfDamage", "baseDamage") {
            let prev = acc.as_ref().map_or(0.0, |a| a.0);
            acc = Some((prev + val_num(&v, "baseDamage", 5966), ty));
        }
        if let Some((val, ty)) = acc {
            let t = apply_dmg_taken_conversion(off, q, &ty, val);
            taken.push(t * gn(off, "SummonedMinionsPerCast", 5974));
        }
    }
    // "Storm Secret" (:5978-5993)
    if name == "Herald of Thunder" {
        let mut acc: Option<(f64, String)> = None;
        for (v, ty) in self_damage_mods(q, "StormSecretSelfDamage", "baseDamage") {
            let prev = acc.as_ref().map_or(0.0, |a| a.0);
            acc = Some((prev + val_num(&v, "baseDamage", 5982), ty));
        }
        if let Some((val, ty)) = acc {
            taken.push(apply_dmg_taken_conversion(off, q, &ty, val));
        }
    }
    // "Eye of Innocence" (:5994-6008)
    {
        let first = self_damage_mods(q, "EyeOfInnocenceSelfDamage", "baseDamage").into_iter().next();
        if off.flag("ignite")
            && let Some((v, ty)) = first
            && v.truthy()
        {
            let val = val_num(&v, "baseDamage", 6003);
            taken.push(apply_dmg_taken_conversion(off, q, &ty, val));
        }
    }
    // "Echoes of Creation" (:6009-6028)
    {
        let first = self_damage_mods(q, "EchoesOfCreationSelfDamage", "dmgMult").into_iter().next();
        let mut avg = gn(off, "GlobalWarcryUptimeRatio", 6015) / 100.0;
        if q.flag(None, &["Condition:WarcryMaxHit"]) {
            avg = dsum(&off.mod_db, ModType::Base, None, &["Multiplier:ExertingWarcryCount"]);
        }
        if let Some((m, ty)) = first
            && m.truthy()
        {
            let m = val_num(&m, "dmgMult", 6020);
            let life = g0(off, "Life", 6020);
            taken.push(apply_dmg_taken_conversion(off, q, &ty, life * m / 100.0 * avg));
        }
    }
    // "Scold's Bridle" (:6029-6043)
    {
        let first = self_damage_mods(q, "ScoldsBridleSelfDamage", "dmgMult").into_iter().next();
        if gt(off, "ManaHasCost")
            && let Some((m, ty)) = first
            && m.truthy()
        {
            let m = val_num(&m, "dmgMult", 6036);
            let cost = g0(off, "ManaCost", 6036);
            taken.push(apply_dmg_taken_conversion(off, q, &ty, cost * m / 100.0));
        }
    }
    // "Trauma" (:6044-6056)
    {
        let stacks = max2(q.base(None, &["Multiplier:TraumaStacks"]), 1.0);
        let per = q.base(None, &["TraumaSelfDamageTakenLife"]);
        let base = Q {
            list: off.sk().skill_mod_list.clone().expect("baseSkillModList"),
            malformed: q.malformed.clone(),
        };
        if base.flag(None, &["HasTrauma"]) {
            let val = per * stacks;
            let t = apply_dmg_taken_conversion(off, q, "Physical", val);
            if off.breakdown {
                let _ = q.base(Some(&off.cfg), &["Multiplier:SustainableTraumaStacks"]);
            }
            taken.push(t);
        }
    }
    // "Enmity's Embrace" (:6057-6071)
    {
        let mut acc: Option<(f64, String)> = None;
        for (v, ty) in self_damage_mods(q, "EnmitysEmbraceSelfDamage", "baseDamage") {
            let prev = acc.as_ref().map_or(0.0, |a| a.0);
            acc = Some((prev + val_num(&v, "baseDamage", 6061), ty));
        }
        if let Some((val, ty)) = acc {
            taken.push(apply_dmg_taken_conversion(off, q, &ty, val));
        }
    }
    // :6077-6088 `pairs(nameToHandler)` (see the module doc)
    if taken.len() >= 3 {
        off.note(format!(
            "CalcOffence.lua:6077 pairs(nameToHandler): {} self hit sources summed in a hash order (R13, Rust: constructor order)",
            taken.len()
        ));
    }
    for t in taken {
        let v = g0(off, "SelfHitDamage", 6080) + t;
        off.gset("SelfHitDamage", v);
    }
    // :6091-6101 `ipairs` over a hash-only table: no iteration.
}

/// :6107-6262 the combined DPS (with the mirage block, see the module doc).
#[allow(clippy::too_many_lines)]
fn combined_dps(off: &mut Off<'_>, quantity_multiplier: f64) {
    let cap = misc("DotDpsCap");
    let show_average = off.sd_truthy("showAverage");
    // :6107-6109
    let base_key = if show_average { "AverageDamage" } else { "TotalDPS" };
    let base_v = off.gval(base_key).cloned();
    gset_val(off, "CombinedDPS", base_v.clone());
    gset_val(off, "CombinedAvg", base_v.clone());
    let base = || num_at(base_v.as_ref(), base_key, 6118);
    if off.flag("dot") {
        let v = num_at(base_v.as_ref(), base_key, 6111) + g0(off, "TotalDot", 6111);
        off.gset("WithDotDPS", v);
    }
    if quantity_multiplier > 1.0 && gt(off, "TotalPoisonDPS") {
        let v = min2(gn(off, "TotalPoisonDPS", 6114) * quantity_multiplier, cap);
        off.gset("TotalPoisonDPS", v);
    }
    if show_average {
        let v = gn(off, "CombinedAvg", 6117) + g0(off, "TotalPoisonAverageDamage", 6117);
        off.gset("CombinedAvg", v);
        let v = base() + g0(off, "TotalPoisonAverageDamage", 6118);
        off.gset("WithPoisonDPS", v);
    } else {
        let v = base() + g0(off, "TotalPoisonDPS", 6120);
        off.gset("WithPoisonDPS", v);
    }
    // :6122-6137
    if off.flag("ignite") {
        if off.flag("igniteCanStack") {
            if show_average {
                let v = gn(off, "CombinedDPS", 6125) + gn(off, "IgniteDamage", 6125);
                off.gset("CombinedAvg", v);
            } else {
                let v = base() + gn(off, "TotalIgniteDPS", 6127);
                off.gset("WithIgniteDPS", v);
            }
        } else if show_average {
            let v = base() + gn(off, "IgniteDamage", 6130);
            off.gset("WithIgniteDPS", v);
            let v = gn(off, "CombinedAvg", 6131) + gn(off, "IgniteDamage", 6131);
            off.gset("CombinedAvg", v);
        } else {
            let v = base() + gn(off, "IgniteDPS", 6133);
            off.gset("WithIgniteDPS", v);
        }
    } else {
        gset_val(off, "WithIgniteDPS", base_v.clone());
    }
    // :6138-6140
    if off.flag("monsterExplode") {
        let v = gn(off, "CombinedAvg", 6139) / off.monster_life * 100.0;
        off.gset("CombinedAvgToMonsterLife", v);
    }
    // :6141-6150
    if off.flag("bleed") {
        if show_average {
            let v = base() + gn(off, "BleedDamage", 6143);
            off.gset("WithBleedDPS", v);
            let v = gn(off, "CombinedAvg", 6144) + gn(off, "BleedDamage", 6144);
            off.gset("CombinedAvg", v);
        } else {
            let v = base() + gn(off, "BleedDPS", 6146);
            off.gset("WithBleedDPS", v);
        }
    } else {
        gset_val(off, "WithBleedDPS", base_v.clone());
    }
    // :6151-6209
    if off.flag("impale") {
        let dps_mult = off
            .sd_num("dpsMultiplier", 6158)
            .unwrap_or_else(|| panic!("{LUA}:6158: attempt to perform arithmetic on field 'dpsMultiplier' (a nil value)"));
        let mut impale;
        if off.flag("attack") && off.sd_truthy("doubleHitsWhenDualWielding") && off.flag("bothWeaponAttack") {
            let hands = off
                .hands
                .as_ref()
                .unwrap_or_else(|| panic!("{LUA}:6156: attempt to index field 'MainHand' (a nil value)"));
            let hand = |h: usize| -> f64 {
                let m = &hands[h].map;
                let mod_ = or_num(m.get("ImpaleModifier"), 1.0, "ImpaleModifier", 6156);
                num_at(m.get("impaleStoredHitAvg"), "impaleStoredHitAvg", 6156)
                    * (mod_ - 1.0)
                    * num_at(m.get("HitChance"), "HitChance", 6156)
                    / 100.0
                    * dps_mult
            };
            impale = hand(0) + hand(1);
        } else {
            let m = g_or1(off, "ImpaleModifier", 6160);
            impale = gn(off, "impaleStoredHitAvg", 6160) * (m - 1.0) * gn(off, "HitChance", 6160) / 100.0 * dps_mult;
        }
        off.gset("ImpaleDPS", impale);
        if gn(off, "ImpaleDuration", 6162) <= 0.0 {
            impale = 0.0;
            off.gset("ImpaleDPS", 0.0);
        }
        if show_average {
            let v = gn(off, "AverageDamage", 6166) + impale;
            off.gset("WithImpaleDPS", v);
            let v = gn(off, "CombinedAvg", 6167) + impale;
            off.gset("CombinedAvg", v);
        } else {
            off.set_flag("notAverage", true);
            let hs = if gt(off, "HitSpeed") {
                gn(off, "HitSpeed", 6170)
            } else {
                gn(off, "Speed", 6170)
            };
            impale *= hs;
            off.gset("ImpaleDPS", impale);
            let v = gn(off, "TotalDPS", 6171) + impale;
            off.gset("WithImpaleDPS", v);
        }
        if quantity_multiplier > 1.0 {
            impale *= quantity_multiplier;
            off.gset("ImpaleDPS", impale);
        }
        let v = gn(off, "CombinedDPS", 6176) + impale;
        off.gset("CombinedDPS", v);
    }
    // :6212-6248 `activeSkill.mirage` (set by `calcs.mirages` before offence: Mirage Archer / Sacred Wisps, R7.9)
    let mut best_cull = 1.0;
    let mirage = off.sk().mirage.clone();
    if let Some(m) = mirage.as_ref().filter(|m| m.output.is_some())
        && let Some(m_total) = m.out("TotalDPS")
    {
        let count = m.count.unwrap_or(1.0);
        off.gset("MirageDPS", m_total * count);
        let v = gn(off, "CombinedDPS", 6216) + m_total * count;
        off.gset("CombinedDPS", v);
        gset_val(off, "MirageBurningGroundDPS", m.out_val("BurningGroundDPS").cloned());
        gset_val(off, "MirageCausticGroundDPS", m.out_val("CausticGroundDPS").cloned());
        if let Some(x) = m.out("IgniteDPS")
            && x > g0(off, "IgniteDPS", 6220)
        {
            let v = gn(off, "MirageDPS", 6221) + x;
            off.gset("MirageDPS", v);
            off.gset("IgniteDPS", 0.0);
        }
        if let Some(x) = m.out("BleedDPS")
            && x > g0(off, "BleedDPS", 6224)
        {
            let v = gn(off, "MirageDPS", 6225) + x;
            off.gset("MirageDPS", v);
            off.gset("BleedDPS", 0.0);
        }
        if let Some(x) = m.out("PoisonDPS") {
            let v = gn(off, "MirageDPS", 6230) + x * count;
            off.gset("MirageDPS", v);
            let v = gn(off, "CombinedDPS", 6231) + x * count;
            off.gset("CombinedDPS", v);
        }
        if let Some(x) = m.out("ImpaleDPS") {
            let v = gn(off, "MirageDPS", 6234) + x * count;
            off.gset("MirageDPS", v);
            let v = gn(off, "CombinedDPS", 6235) + x * count;
            off.gset("CombinedDPS", v);
        }
        if let Some(x) = m.out("DecayDPS") {
            let v = gn(off, "MirageDPS", 6238) + x;
            off.gset("MirageDPS", v);
            let v = gn(off, "CombinedDPS", 6239) + x;
            off.gset("CombinedDPS", v);
        }
        let dot_can_stack = off.flag("DotCanStack");
        if let Some(x) = m.out("TotalDot")
            && (dot_can_stack || !gt(off, "TotalDot") || off.gval("TotalDot") == Some(&OutValue::Num(0.0)))
        {
            let k = if dot_can_stack { count } else { 1.0 };
            let v = gn(off, "MirageDPS", 6242) + x * k;
            off.gset("MirageDPS", v);
            let v = gn(off, "CombinedDPS", 6243) + x * k;
            off.gset("CombinedDPS", v);
        }
        let cull = m
            .out("CullMultiplier")
            .unwrap_or_else(|| panic!("{LUA}:6245: attempt to compare nil with number (mirage.output.CullMultiplier)"));
        if cull > 1.0 {
            best_cull = cull;
        }
    }
    // :6249-6255
    let total_dot_dps = g0(off, "TotalDot", 6249)
        + g0(off, "TotalPoisonDPS", 6249)
        + max2(g0(off, "CausticGroundDPS", 6249), g0(off, "MirageCausticGroundDPS", 6249))
        + (if gt(off, "TotalIgniteDPS") {
            gn(off, "TotalIgniteDPS", 6249)
        } else {
            g0(off, "IgniteDPS", 6249)
        })
        + max2(g0(off, "BurningGroundDPS", 6249), g0(off, "MirageBurningGroundDPS", 6249))
        + g0(off, "BleedDPS", 6249)
        + g0(off, "CorruptingBloodDPS", 6249)
        + g0(off, "DecayDPS", 6249);
    let capped = min2(total_dot_dps, cap);
    off.gset("TotalDotDPS", capped);
    if capped != total_dot_dps {
        off.out().set_bool("showTotalDotDPS", true);
    }
    if !show_average {
        let v = gn(off, "CombinedDPS", 6255) + capped;
        off.gset("CombinedDPS", v);
    }
    // :6258-6262
    best_cull = max2(best_cull, gn(off, "CullMultiplier", 6258));
    let combined = gn(off, "CombinedDPS", 6259);
    off.gset("CullingDPS", combined * (best_cull - 1.0));
    let rdm = gn(off, "ReservationDpsMultiplier", 6260);
    off.gset("ReservationDPS", combined * (rdm - 1.0));
    off.gset("CombinedDPS", combined * best_cull * rdm);
}

/// `output[k] or 1`.
fn g_or1(off: &Off<'_>, k: &str, line: u32) -> f64 {
    or_num(off.gval(k), 1.0, k, line)
}
