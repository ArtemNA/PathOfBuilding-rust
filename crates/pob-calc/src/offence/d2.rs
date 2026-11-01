//! CalcOffence.lua:5332-5770 (R7.6, part D, second half): the tail of the ailment pass loop -- the non-damaging ailments (:5332-5513:
//! the enemy ailment threshold, Chilling Area :5377-5397, freeze :5398-5414, the cycle :5415-5513), knockback (:5515-5524), the enemy
//! stun (:5526-5594) and impale (:5596-5647); after the loop the combine of the secondary effect stats (:5650-5696), Decay
//! (:5698-5741) and the burning ground (:5743-5769). OC5 (:5772) is taken right after.
//!
//! Hash-order walk (R13): `for ailment, val in pairs(ailments)` (:5416) over five STRING keys -- LuaJIT's order differs between
//! processes. Each iteration writes only its own ailment's keys (`<A>Chance`, `<A>Duration`, `<A>EffectMod`, `skillFlags[a]`), the
//! `ailments` table is local to the pass and its `effList` feeds only the breakdown, and the one shared write --
//! `cfg.skillCond["CriticalStrike"]` -- is the same value in every iteration (it depends on `ailmentMode` and a modDB flag only): the
//! result is order-free, Rust walks the constructor order (Chill, Shock, Scorch, Brittle, Sap). Only the ORDER of the queries differs
//! (visible solely through a malformed-mod error, R6.4).
//!
//! Breakdown (R8.5): the `if breakdown` / `if globalBreakdown` branches of this half are ported where they stand (the non-damaging
//! ailment effect table :5439-5493 mutates the local `effList` only).

use pob_data::constants::{constants, monster_table_at};
use pob_lua::number::{max2, min2, round};
use pob_lua::tostring_num;
use pob_mods::{ModCfg, ModType};

use super::b::combine_stat;
use super::c::{LUA, config_is, dmore, dsum};
use super::c2::resist_for_type;
use super::d::{ailment_mode, calc_ailment_damage, dmod};
use super::helpers::{DMG_TYPE_LIST, Q};
use super::{CombineMode, Off};
use crate::breakdown::{BTable, Chain, multi_chain, step};
use crate::setup::{keyword_flag, misc, mod_flag, ok};
use crate::sf;

/// `output[k]` of pass `pi` in arithmetic.
fn pn(off: &Off<'_>, pi: usize, k: &str, line: u32) -> f64 {
    off.pnum_req(pi, k, line)
}

/// `data.nonDamagingAilment[name]`.
fn ailment_data(name: &str) -> &'static pob_data::rules::NonDamagingAilment {
    pob_data::rules::rules()
        .non_damaging_ailment
        .0
        .get(name)
        .unwrap_or_else(|| panic!("data.nonDamagingAilment.{name} is nil"))
}

/// :4315-4330 `calcAverageSourceDamage(ailment)` for pass `pi`.
fn average_source_damage(off: &Off<'_>, q: &Q, pi: usize, ailment: &str) -> (f64, f64) {
    let assoc = ailment_data(ailment).associated_type.as_deref();
    let (mut hit, mut crit) = (0.0, 0.0);
    for ty in DMG_TYPE_LIST {
        if !off.can_deal.get(ty).copied().unwrap_or(false) {
            continue;
        }
        let c = Some(off.pcfg(pi));
        let applies = if Some(ty) == assoc {
            !q.flag(c, &[&format!("{ty}Cannot{ailment}")])
        } else {
            q.flag(c, &[&format!("{ty}Can{ailment}")])
        };
        if applies {
            hit += pn(off, pi, &format!("{ty}HitAverage"), 4325);
            crit += pn(off, pi, &format!("{ty}CritAverage"), 4326);
        }
    }
    (hit, crit)
}

/// :5332-5647 the tail of the ailment pass loop body for pass `pi`.
#[allow(clippy::too_many_lines)]
pub(super) fn pass_tail(off: &mut Off<'_>, q: &Q, pi: usize) {
    let edb = off.enemy_db.clone();
    let mode = ailment_mode(off);
    // :5333-5342 the enemy ailment threshold (feeds only the breakdown; its Lua errors are kept)
    let enemy_threshold = {
        let is_boss = !config_is(off, "enemyIsBoss", "None");
        let lvl = off.env.enemy_level;
        let k = constants();
        let threshold = monster_table_at(&k.monster_ailment_threshold_table, lvl);
        let mut map_life = Some(1.0);
        let mut map_ailment = Some(1.0);
        if lvl >= 66.0 {
            let boss = if is_boss {
                monster_table_at(&k.map_level_boss_life_mult, lvl)
            } else {
                None
            };
            map_life = boss.or_else(|| k.map_level_life_mult.0.get(&tostring_num(lvl)).copied());
            // `isBoss and data.mapLevelBossAilmentMult[lvl] or enemyMapAilmentMult`
            map_ailment = if is_boss {
                monster_table_at(&k.map_level_boss_ailment_mult, lvl).or(Some(1.0))
            } else {
                Some(1.0)
            };
        }
        let type_mult = if is_boss { 7.68 } else { 1.0 };
        let nil = || -> ! { panic!("{LUA}:5342: attempt to perform arithmetic on a nil value") };
        threshold.unwrap_or_else(|| nil())
            * type_mult
            * map_life.unwrap_or_else(|| nil())
            * dmore(&edb, None, &["Life"])
            * map_ailment.unwrap_or_else(|| nil())
            * dmore(&edb, None, &["AilmentThreshold"])
    };
    let c = off.pcfg(pi).clone();
    // :5378-5397 Chilling Area
    if off.has_type("ChillingArea") || off.has_type("NonHitChill") {
        off.set_flag("chill", true);
        let inc_chill = q.inc(Some(&c), &["EnemyChillEffect"]);
        let more_chill = q.more(Some(&c), &["EnemyChillEffect"]);
        let effect_mod = (1.0 + inc_chill / 100.0) * more_chill * dmod(&edb, None, &["SelfChillEffect"]);
        off.pset_num(pi, "ChillEffectMod", effect_mod);
        let names = ["EnemyChillDuration", "EnemyAilmentDuration", "EnemyElementalAilmentDuration"];
        let self_names = ["SelfChillDuration", "SelfAilmentDuration", "SelfElementalAilmentDuration"];
        let inc_dur = q.inc(Some(&c), &names) + dsum(&edb, ModType::Inc, None, &self_names);
        let more_dur = q.more(Some(&c), &names) * dmore(&edb, None, &self_names);
        off.pset_num(pi, "ChillDurationMod", (1.0 + inc_dur / 100.0) * more_dur);
        let chill = ailment_data("Chill");
        let site = format!("{LUA}:5386");
        let max = q.override_num(None, &["ChillMax"], &site).unwrap_or(chill.max);
        let default = chill
            .default
            .unwrap_or_else(|| panic!("{site}: attempt to perform arithmetic on field 'default' (a nil value)"));
        let source_effect = min2(max, (default * effect_mod).floor());
        off.pset_num(pi, "ChillSourceEffect", source_effect);
        // :5387-5396 (`base` is a string: `chain.base[2]` is nil, no base row)
        if off.breakdown {
            let mut t = BTable::new();
            multi_chain(
                &mut t,
                &Chain {
                    label: Some(sf!("Effect of Chill: ^8(capped at %d%%)", max)),
                    steps: vec![
                        step("%.2f ^8(increased/reduced effect of chill)", 1.0 + inc_chill / 100.0),
                        step("%.2f ^8(more/less effect of chill)", more_chill),
                    ],
                    total: Some(sf!("= %.0f%%", source_effect)),
                    ..Chain::default()
                },
            );
            off.pbd_put(pi, "DotChill", t);
        }
    }
    // :5399-5414 freeze
    let mode_text = if mode == "CRIT" { "Crits Only" } else { "Average Damage" };
    if pn(off, pi, "FreezeChanceOnHit", 5399) + pn(off, pi, "FreezeChanceOnCrit", 5399) > 0.0 {
        // :5400-5404
        if off.breakdown {
            let t = BTable::lines([sf!("Ailment mode: %s ^8(can be changed in the Configuration tab)", mode_text)]);
            off.bd_put("FreezeDurationMod", t);
        }
        let crit_chance = pn(off, pi, "CritChance", 5405);
        let (h, cr) = average_source_damage(off, q, pi, "Freeze");
        let base_val = calc_ailment_damage(off, q, pi, "Freeze", crit_chance, h, cr, false) * q.more(Some(&c), &["FreezeAsThoughDealing"]);
        if base_val > 0.0 {
            off.set_flag("freeze", true);
            let v = 1.0
                + q.inc(
                    Some(&c),
                    &["EnemyFreezeDuration", "EnemyAilmentDuration", "EnemyElementalAilmentDuration"],
                ) / 100.0
                + dsum(
                    &edb,
                    ModType::Inc,
                    None,
                    &[
                        "SelfFreezeDuration",
                        "SelfElementalAilmentDuration",
                        "SelfAilmentDuration",
                        "HoarfrostFreezeDuration",
                    ],
                ) / 100.0;
            off.pset_num(pi, "FreezeDurationMod", v);
            // :5409-5412
            if let Some(bd) = off.pbd(pi) {
                let t = bd.table_mut("FreezeDPS");
                t.push(sf!(
                    "For freeze to apply for the minimum of 0.3 seconds, target must have no more than %.0f Ailment Threshold.",
                    base_val * 20.0 * v
                ));
                t.push(sf!(
                    "^8(Ailment Threshold is about equal to Life except on bosses where it is about half of their life)"
                ));
            }
        }
    }
    // :5416-5513 the non-damaging ailments (order-free, see the module doc)
    for a in ["Chill", "Shock", "Scorch", "Brittle", "Sap"] {
        let total = pn(off, pi, &format!("{a}ChanceOnHit"), 5417) + pn(off, pi, &format!("{a}ChanceOnCrit"), 5417);
        // `(onHit + onCrit) > 0` (NaN fails it)
        if total.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            continue;
        }
        // :5418-5422
        if off.breakdown {
            let t = BTable::lines([sf!("Ailment mode: %s ^8(can be changed in the Configuration tab)", mode_text)]);
            off.bd_put(&format!("{a}EffectMod"), t);
        }
        let crit_cond = mode == "CRIT" && !super::c::dflag(&off.mod_db, None, &["AilmentsAreNeverFromCrit"]);
        off.set_pass_cond(pi, "CriticalStrike", crit_cond);
        let crit_chance = pn(off, pi, "CritChance", 5430);
        let (h, cr) = average_source_damage(off, q, pi, a);
        let base = calc_ailment_damage(off, q, pi, a, crit_chance, h, cr, false);
        let c = off.pcfg(pi).clone();
        let damage = base * q.more(Some(&c), &[&format!("{a}AsThoughDealing")]);
        if damage > 0.0 {
            off.set_flag(&a.to_lowercase(), true);
            let own = format!("Enemy{a}Duration");
            let names = [own.as_str(), "EnemyElementalAilmentDuration", "EnemyAilmentDuration"];
            let own_self = format!("Self{a}Duration");
            let self_names = [own_self.as_str(), "SelfElementalAilmentDuration", "SelfAilmentDuration"];
            let inc_dur = q.inc(Some(&c), &names) + dsum(&edb, ModType::Inc, None, &self_names);
            let more_dur = q.more(Some(&c), &names) * dmore(&edb, None, &self_names);
            let base_dur = ailment_data(a)
                .duration
                .unwrap_or_else(|| panic!("{LUA}:5436: attempt to perform arithmetic on field 'duration' (a nil value)"));
            let dur = base_dur * (1.0 + inc_dur / 100.0) * more_dur * off.debuff_duration_mult;
            off.pset_num(pi, &format!("{a}Duration"), dur);
            let effect = q.calc_mod(Some(&c), &[&format!("Enemy{a}Effect")]) * dmod(&edb, None, &[&format!("Self{a}Effect")]);
            off.pset_num(pi, &format!("{a}EffectMod"), effect);
            // :5439-5493
            if off.breakdown {
                ailment_effect_table(off, pi, a, damage, effect, enemy_threshold);
            }
            // :5494-5510
            if off.breakdown && dur != base_dur {
                let mut t = BTable::new();
                if off.is_attack {
                    t.push(format!("{}:", off.passes[pi].label));
                }
                t.push(sf!("%.2fs ^8(base duration)", base_dur));
                if inc_dur != 0.0 {
                    t.push(sf!("x %.2f ^8(increased/reduced duration)", 1.0 + inc_dur / 100.0));
                }
                if more_dur != 1.0 {
                    t.push(sf!("x %.2f ^8(more/less duration)", more_dur));
                }
                if off.debuff_duration_mult != 1.0 {
                    t.push(sf!("/ %.2f ^8(debuff expires slower/faster)", 1.0 / off.debuff_duration_mult));
                }
                t.push(sf!("= %.2fs", dur));
                off.pbd_put(pi, &format!("{a}Duration"), t);
            }
        }
    }
    let c = off.pcfg(pi).clone();
    let c = Some(&c);
    // :5516-5524 knockback
    let crit_chance = pn(off, pi, "CritChance", 5516);
    let kb = min2(
        100.0,
        pn(off, pi, "KnockbackChanceOnHit", 5516) * (1.0 - crit_chance / 100.0)
            + pn(off, pi, "KnockbackChanceOnCrit", 5516) * crit_chance / 100.0
            + dsum(&edb, ModType::Base, None, &["SelfKnockbackChance"]),
    );
    off.pset_num(pi, "KnockbackChance", kb);
    if kb > 0.0 {
        let d = round(4.0 * q.calc_mod(c, &["EnemyKnockbackDistance"]), None);
        off.pset_num(pi, "KnockbackDistance", d);
        // :5519-5523
        off.pbd_put(pi, "KnockbackDistance", BTable::new().with("radius", d));
    }
    // :5527-5563 enemy stun
    let red = -q.inc(c, &["EnemyStunThreshold"]);
    let thr = if red > 75.0 {
        1.0 - (75.0 + (red - 75.0) * 25.0 / (red - 50.0)) / 100.0
    } else {
        1.0 - red / 100.0
    };
    off.pset_num(pi, "EnemyStunThresholdMod", thr);
    let base = if off.sd_truthy("baseStunDuration") {
        off.sd_num("baseStunDuration", 5533).expect("truthy")
    } else {
        0.35
    };
    let inc_dur = q.inc(c, &["EnemyStunDuration"]);
    let inc_dur_crit = q.inc(c, &["EnemyStunDurationOnCrit"]);
    let more_dur = q.more(c, &["EnemyStunDuration"]);
    let chance_to_double = min2(
        q.base(c, &["DoubleEnemyStunDurationChance"]) + dsum(&edb, ModType::Base, c, &["SelfDoubleStunDurationChance"]),
        100.0,
    );
    let inc_recov = dsum(&edb, ModType::Inc, None, &["StunRecovery"]);
    // `minimumStunDuration` / `maximumStunDuration` feed only the breakdown
    let minimum = base * more_dur / (1.0 + inc_recov / 100.0);
    let mut stun = minimum;
    let crit_chance = pn(off, pi, "CritChance", 5542);
    if inc_dur_crit != 0.0 && crit_chance != 0.0 {
        if crit_chance == 100.0 {
            stun = minimum * (1.0 + (inc_dur + inc_dur_crit) / 100.0);
        } else {
            stun *= 1.0 + (inc_dur + inc_dur_crit * crit_chance / 100.0) / 100.0;
        }
    } else {
        stun = minimum * (1.0 + inc_dur / 100.0);
    }
    if chance_to_double != 0.0 {
        stun *= 1.0 + chance_to_double / 100.0;
    }
    off.pset_num(pi, "EnemyStunDuration", stun);
    // :5562-5593 (the minimum / maximum of :5539-5560)
    if off.breakdown && stun != base {
        let (mut min_d, mut max_d) = (minimum, minimum);
        if inc_dur_crit != 0.0 && crit_chance != 0.0 {
            if crit_chance == 100.0 {
                min_d *= 1.0 + (inc_dur + inc_dur_crit) / 100.0;
                max_d = min_d;
            } else {
                min_d *= 1.0 + inc_dur / 100.0;
                max_d *= 1.0 + (inc_dur + inc_dur_crit) / 100.0;
            }
        } else {
            min_d *= 1.0 + inc_dur / 100.0;
            max_d = min_d;
        }
        if chance_to_double != 0.0 {
            if chance_to_double == 100.0 {
                min_d *= 2.0;
            }
            max_d *= 2.0;
        }
        let mut t = BTable::lines([sf!("%.2fs ^8(base duration)", base)]);
        if inc_dur != 0.0 || (inc_dur_crit != 0.0 && crit_chance != 0.0) {
            t.push(sf!(
                "x %.2f ^8(increased/reduced stun duration)",
                1.0 + (inc_dur + inc_dur_crit * crit_chance / 100.0) / 100.0
            ));
        }
        if more_dur != 1.0 {
            t.push(sf!("x %.2f ^8(more/less stun duration)", more_dur));
        }
        if chance_to_double != 0.0 {
            t.push(sf!("x %.2f ^8(chance to double stun duration)", 1.0 + chance_to_double / 100.0));
        }
        if inc_recov != 0.0 {
            t.push(sf!("/ %.2f ^8(increased/reduced enemy stun recovery)", 1.0 + inc_recov / 100.0));
        }
        t.push(sf!("= %.2fs", stun));
        if min_d != max_d {
            t.push(sf!("(minimum: %.2fs, maximum: %.2fs)", min_d, max_d));
        }
        let enemy_action_speed = crate::defence::action_speed_mod(&edb);
        if enemy_action_speed != 1.0 {
            t.push(sf!("/ %.2f ^8(enemy action speed)", enemy_action_speed));
            t.push(sf!(
                "= %.2fs (note that for effects that care about duration this is ignored)",
                stun / enemy_action_speed
            ));
            if min_d != max_d {
                t.push(sf!(
                    "(minimum: %.2fs, maximum: %.2fs)",
                    min_d / enemy_action_speed,
                    max_d / enemy_action_speed
                ));
            }
        }
        off.pbd_put(pi, "EnemyStunDuration", t);
    }
    // :5597-5647 impale
    if off.can_deal.get("Physical").copied().unwrap_or(false)
        && pn(off, pi, "ImpaleChance", 5597) + pn(off, pi, "ImpaleChanceOnCrit", 5597) > 0.0
    {
        off.set_flag("impale", true);
        let crit = pn(off, pi, "CritChance", 5599) / 100.0;
        let impale_chance = min2(pn(off, pi, "ImpaleChance", 5600) / 100.0, 1.0) * (1.0 - crit)
            + min2(pn(off, pi, "ImpaleChanceOnCrit", 5600) / 100.0, 1.0) * crit;
        let max_stacks = q.base(c, &["ImpaleStacksMax"]) * (1.0 + q.base(c, &["ImpaleAdditionalDurationChance"]) / 100.0);
        let config_stacks = dsum(&edb, ModType::Base, c, &["Multiplier:ImpaleStacks"]);
        let stacks = min2(max_stacks, config_stacks);
        let base_stored = misc("ImpaleStoredDamageBase");
        let inc_on_bleed = q.inc(c, &["ImpaleEffectOnBleed"]) * min2(q.base(c, &["BleedChance"]) / 100.0, 1.0);
        let inc = (q.inc(c, &["ImpaleEffect"]) + inc_on_bleed) / 100.0;
        let more = round(q.more(c, &["ImpaleEffect"]), Some(2.0));
        let modifier = (1.0 + inc) * more;
        let stored = base_stored * modifier;
        let hit_damage_mod = stored * stacks;
        let enemy_armour = max2(ok(crate::calc_lib::val(&edb.borrow(), "Armour", None)), 0.0);
        let armour_reduction = crate::pools::armour_reduction_f(enemy_armour, hit_damage_mod * pn(off, pi, "impaleStoredHitAvg", 5614));
        let mut resist = min2(
            max2(
                0.0,
                dsum(&edb, ModType::Base, None, &["PhysicalDamageReduction"])
                    + q.base(c, &["EnemyImpalePhysicalDamageReduction"])
                    + armour_reduction,
            ),
            misc("EnemyPhysicalDamageReductionCap"),
        );
        if q.flag(c, &["IgnoreEnemyImpalePhysicalDamageReduction"]) {
            resist = 0.0;
        }
        let taken_cfg = ModCfg {
            flags: Some(f64::from(mod_flag("Hit"))),
            ..ModCfg::default()
        };
        let names = ["DamageTaken", "PhysicalDamageTaken", "ReflectedDamageTaken"];
        let taken = (1.0 + dsum(&edb, ModType::Inc, Some(&taken_cfg), &names) / 100.0) * dmore(&edb, Some(&taken_cfg), &names);
        let dmg_modifier = hit_damage_mod * (1.0 - resist / 100.0) * impale_chance * taken;
        off.gset("ImpaleStacksMax", max_stacks);
        off.gset("ImpaleStacks", stacks);
        off.pset_num(pi, "ImpaleStoredDamage", stored * 100.0);
        off.pset_num(pi, "ImpaleModifier", 1.0 + dmg_modifier);
        // :5629-5645
        if off.breakdown {
            let t = BTable::lines([
                "10% ^8(base value)".to_owned(),
                sf!("x %.2f ^8(increased effectiveness)", modifier),
                sf!("= %.1f%%", stored * 100.0),
            ]);
            off.pbd_put(pi, "ImpaleStoredDamage", t);
            let mut t = BTable::lines([
                sf!("%d ^8(number of stacks, can be overridden in the Configuration tab)", stacks),
                sf!("x %.3f ^8(stored damage)", stored),
                sf!("x %.2f ^8(impale chance)", impale_chance),
                sf!("x %.2f ^8(impale enemy physical damage reduction)", 1.0 - resist / 100.0),
            ]);
            if taken != 1.0 {
                t.push(sf!("x %.2f ^8(impale enemy damage taken)", taken));
            }
            t.push(sf!("= %.3f ^8(impale damage multiplier)", dmg_modifier));
            off.pbd_put(pi, "ImpaleModifier", t);
        }
    }
}

/// :5344-5376 the `ailments` table's `effList`, `effect` and `thresh` of a non-damaging ailment.
fn ailment_curve(a: &str) -> (Vec<f64>, f64, bool) {
    // (effList, the effect constant, ramping is decided by the caller for Chill)
    match a {
        "Chill" => (vec![10.0, 20.0], 50.0, false),
        "Shock" => (vec![10.0, 20.0, 40.0], 50.0, true),
        "Scorch" => (vec![5.0, 10.0, 20.0], 50.0, true),
        "Brittle" => (vec![2.0, 4.0], 10.0, true),
        "Sap" => (vec![5.0, 10.0], 100.0 / 3.0, false),
        _ => unreachable!("non-damaging ailment {a}"),
    }
}

/// :5439-5493 the `<A>DPS` effect table (`label`, `footer`, `rowList`, `colList`) of a non-damaging ailment.
fn ailment_effect_table(off: &mut Off<'_>, pi: usize, a: &str, damage: f64, effect_mod: f64, enemy_threshold: f64) {
    let data = ailment_data(a);
    let (mut eff_list, k, ramping) = ailment_curve(a);
    // `ramping = output.HasBonechill or false` for Chill (:5348)
    let ramping = if a == "Chill" {
        !matches!(off.pval(pi, "HasBonechill"), None | Some(crate::output::OutValue::Bool(false)))
    } else {
        ramping
    };
    let effect = |damage: f64, effect_mod: f64| k * (damage / enemy_threshold).powf(0.4) * effect_mod;
    let thresh = |damage: f64, value: f64, effect_mod: f64| damage * (k * effect_mod / value).powf(2.5);
    let gnum_or = |off: &Off<'_>, key: &str| match off.gval(key) {
        None | Some(crate::output::OutValue::Bool(false)) => None,
        Some(crate::output::OutValue::Num(x)) => Some(*x),
        Some(o) => panic!("{LUA}:5440: attempt to compare {o:?} with number"),
    };
    let maximum = gnum_or(off, &format!("Maximum{a}")).unwrap_or(data.max);
    let current = max2(min2(gnum_or(off, &format!("Current{a}")).unwrap_or(0.0), maximum), 0.0);
    let desired = max2(
        min2(dsum(&off.enemy_db, ModType::Base, None, &[&format!("Desired{a}Val")]), maximum),
        0.0,
    );
    if data.min != 0.0 {
        eff_list.push(data.min);
    }
    if enemy_threshold > 0.0 {
        eff_list.push(effect(damage, effect_mod));
    }
    if !eff_list.contains(&maximum) {
        eff_list.push(maximum);
    }
    if current > 0.0 && !eff_list.contains(&current) {
        eff_list.push(current);
    }
    if desired > 0.0 && !eff_list.contains(&desired) && current == 0.0 {
        eff_list.push(desired);
    }
    let label = format!(
        "Resulting ailment effect{}",
        if current > 0.0 && ramping {
            sf!(" ^8(with a ^7%s%% ^8%s on the enemy)^7", tostring_num(current), a)
        } else {
            String::new()
        }
    );
    let footer = sf!(
        "^8(ailment threshold is about equal to life, except on bosses that have specific ailment thresholds)\n(the above table shows that when the enemy has X ailment threshold, you ^8%s for Y)",
        a.to_lowercase()
    );
    // `table.sort(val.effList)` -- numbers, `<`
    // (numbers with `<`: any sort gives the same sequence of values)
    eff_list.sort_by(|x, y| x.partial_cmp(y).unwrap_or_else(|| panic!("{LUA}:5463: attempt to compare NaN")));
    let precision = data
        .precision
        .unwrap_or_else(|| panic!("{LUA}:5469: attempt to perform arithmetic on local 'precision' (a nil value)"));
    let boss = match off.env.config.input.get("enemyIsBoss") {
        Some(pob_build::ConfigValue::Str(s)) => s.to_string(),
        None => "nil".to_owned(),
        Some(o) => panic!("{LUA}:5472: enemyIsBoss is {o:?}"),
    };
    let mut rows = BTable::new();
    for value in eff_list {
        let th = thresh(damage, value, effect_mod);
        let dec_check = value / value.floor();
        let p10 = 10f64.powf(precision);
        let value = (value * p10).floor() / p10;
        let value_format = format!("%.{}f%%", tostring_num(precision));
        let mut thresh_string = sf!("%d", th);
        if (th + 0.5).floor() == (enemy_threshold + 0.5).floor() {
            thresh_string.push_str(&sf!(" ^8(%s)", boss.as_str()));
        }
        let mut labels: Vec<&str> = Vec::new();
        if dec_check == 1.0 && value != 0.0 {
            if value == current {
                labels.push("current");
            }
            if value == desired {
                labels.push("desired");
            }
            if value == maximum {
                labels.push("maximum");
            }
            if value == data.min {
                labels.push("minimum");
            }
        }
        let mut eff = crate::breakdown::format(&value_format, &[pob_lua::Arg::Num(value)]);
        if !labels.is_empty() {
            eff.push_str(&format!(" ^8({})", labels.join(", ")));
        }
        rows.push(crate::breakdown::row(&[("effect", eff), ("thresh", thresh_string)]));
    }
    let bd = off.pbd(pi).expect("breakdown");
    let t = bd.table_mut(&format!("{a}DPS"));
    t.fields.insert("label".into(), label.into());
    t.fields.insert("footer".into(), footer.into());
    t.fields.insert("rowList".into(), rows.into());
    t.fields.insert(
        "colList".into(),
        crate::breakdown::col_list(&[("Ailment Threshold", "thresh"), (&format!("{a} Effect"), "effect")]).into(),
    );
}

/// :5650-5769 after the ailment pass loop.
pub(super) fn after_ailments(off: &mut Off<'_>) {
    let q = off.q.clone();
    // :5651-5696
    if off.is_attack {
        use CombineMode::{Average, Chance, ChanceAilment, Dps};
        let show_average = off.sd_truthy("showAverage");
        let mut list: Vec<(&str, CombineMode, Option<&str>)> = vec![
            ("BleedChance", Average, None),
            ("BleedDPS", ChanceAilment, Some("BleedChance")),
            ("PoisonChance", Average, None),
            ("PoisonDPS", Chance, Some("PoisonChance")),
            ("CausticGroundDPS", Chance, Some("PoisonChance")),
            ("TotalPoisonDPS", Dps, None),
            ("PoisonDamage", Chance, Some("PoisonChance")),
        ];
        list.push(if show_average {
            ("TotalPoisonAverageDamage", Dps, None)
        } else {
            ("TotalPoisonStacks", Dps, None)
        });
        list.push(("IgniteChance", Average, None));
        list.push(("IgniteDPS", ChanceAilment, Some("IgniteChance")));
        if off.flag("igniteCanStack") {
            list.push(("IgniteDamage", Chance, Some("IgniteChance")));
            if show_average {
                list.push(("TotalIgniteAverageDamage", Dps, None));
            }
            list.push(("IgniteStacksMax", Dps, None));
            list.push(("TotalIgniteDPS", Dps, None));
        }
        for k in [
            "ChillEffectMod",
            "ChillDuration",
            "ShockChance",
            "ShockDuration",
            "ShockEffectMod",
            "FreezeChance",
            "FreezeDurationMod",
            "ScorchChance",
            "ScorchEffectMod",
            "ScorchDuration",
            "BrittleChance",
            "BrittleEffectMod",
            "BrittleDuration",
            "SapChance",
            "SapEffectMod",
            "SapDuration",
            "ImpaleChance",
            "ImpaleStoredDamage",
        ] {
            list.push((k, Average, None));
        }
        list.push(("ImpaleModifier", Chance, Some("ImpaleChance")));
        for (k, m, ch) in list {
            combine_stat(off, k, m, ch);
        }
    }
    let edb = off.enemy_db.clone();
    // :5698-5741 Decay
    if off.flag("hit") && off.sd_truthy("decay") && off.can_deal.get("Chaos").copied().unwrap_or(false) {
        off.set_flag("decay", true);
        let sc = &off.cfg;
        let skw = sc
            .keyword_flags
            .unwrap_or_else(|| panic!("{LUA}:5708: bad argument #1 to 'band' (number expected, got nil)")) as u32;
        let dc = ModCfg {
            flags: Some(f64::from(mod_flag("Dot"))),
            keyword_flags: Some(f64::from((skw & !keyword_flag("Hit")) | keyword_flag("ChaosDot"))),
            skill_name: sc.skill_name.clone(),
            skill_part: sc.skill_part,
            skill_types: sc.skill_types.clone(),
            summon_skill_name: sc.summon_skill_name.clone(),
            slot_name: sc.slot_name.clone(),
            ..ModCfg::default()
        };
        // :5701 `activeSkill.decayCfg = { ... }` (kept for the Calcs tab, R8.6)
        off.keep_cfg("decayCfg".into(), dc.clone(), None);
        let mut eff_mult = 1.0;
        if off.env.mode_effective {
            let resist = resist_for_type(off, "Chaos", Some(&dc));
            let names = ["DamageTaken", "DamageTakenOverTime", "ChaosDamageTaken", "ChaosDamageTakenOverTime"];
            let taken_inc = dsum(&edb, ModType::Inc, None, &names);
            let taken_more = dmore(&edb, None, &names);
            eff_mult = (1.0 - resist / 100.0) * (1.0 + taken_inc / 100.0) * taken_more;
            off.out().set_num("DecayEffMult", eff_mult);
            // :5718-5721
            if off.breakdown && eff_mult != 1.0 {
                let source_res = if super::c::dflag(&off.env_db, None, &["EnemyChaosResistEqualToYours"]) {
                    "Your Chaos Resistance"
                } else if super::c::dflag(&off.st.party_members.mod_db, None, &["EnemyChaosResistEqualToYours"]) {
                    "Party Member Chaos Resistance"
                } else {
                    "Chaos"
                };
                let t = crate::breakdown::eff_mult(
                    "Chaos",
                    resist,
                    0.0,
                    taken_inc,
                    eff_mult,
                    taken_more,
                    Some(source_res),
                    true,
                    None,
                    None,
                );
                off.bd_put("DecayEffMult", t);
            }
        }
        let inc = q.inc(Some(&dc), &["Damage", "ChaosDamage"]);
        let more = q.more(Some(&dc), &["Damage", "ChaosDamage"]);
        let site = format!("{LUA}:5725");
        let mult = q
            .override_num(Some(&dc), &["DotMultiplier"], &site)
            .unwrap_or_else(|| q.base(Some(&dc), &["DotMultiplier"]) + q.base(Some(&dc), &["ChaosDotMultiplier"]));
        let decay = off.sd_num("decay", 5726).expect("truthy");
        off.out()
            .set_num("DecayDPS", decay * (1.0 + inc / 100.0) * more * (1.0 + mult / 100.0) * eff_mult);
        let d = 8.0 * off.debuff_duration_mult;
        off.out().set_num("DecayDuration", d);
        // :5728-5740
        if off.breakdown {
            let total = off.out_num("DecayDPS", 5730);
            let mut t = BTable::new();
            crate::breakdown::dot(&mut t, decay, inc, more, Some(mult), None, None, Some(eff_mult), total);
            off.bd_put("DecayDPS", t);
            if d != 8.0 {
                let mut t = BTable::lines([sf!("%.2fs ^8(base duration)", 8.0)]);
                if off.debuff_duration_mult != 1.0 {
                    t.push(sf!("/ %.2f ^8(debuff expires slower/faster)", 1.0 / off.debuff_duration_mult));
                }
                t.push(sf!("= %.2fs", d));
                off.bd_put("DecayDuration", t);
            }
        }
    }
    // :5743-5769 burning ground
    let mdb = off.mod_db.clone();
    let base_drops = dsum(&mdb, ModType::Base, None, &["DropsBurningGround"]);
    if base_drops > 0.0 && off.can_deal.get("Fire").copied().unwrap_or(false) {
        let dot = ModCfg {
            flags: Some(f64::from(mod_flag("Dot"))),
            keyword_flags: Some(0.0),
            ..ModCfg::default()
        };
        let dot_taken = dot.clone();
        let dot_type = ModCfg {
            keyword_flags: Some(f64::from(keyword_flag("FireDot"))),
            ..dot.clone()
        };
        let mut eff_mult = 1.0;
        if off.env.mode_effective {
            let resist = resist_for_type(off, "Fire", Some(&dot_type));
            let names = [
                "DamageTaken",
                "DamageTakenOverTime",
                "FireDamageTaken",
                "FireDamageTakenOverTime",
                "ElementalDamageTaken",
            ];
            let taken_inc = dsum(&edb, ModType::Inc, Some(&dot_taken), &names);
            let taken_more = dmore(&edb, Some(&dot_taken), &names);
            eff_mult = (1.0 - resist / 100.0) * (1.0 + taken_inc / 100.0) * taken_more;
        }
        let names = ["Damage", "FireDamage", "ElementalDamage"];
        let inc = dsum(&mdb, ModType::Inc, Some(&dot_type), &names);
        let more = dmore(&mdb, Some(&dot_type), &names);
        let mult = match ok(mdb.borrow().override_(Some(&dot_type), &["DotMultiplier"])) {
            Some(v) if v.truthy() => v
                .as_num()
                .unwrap_or_else(|| panic!("{LUA}:5763: attempt to perform arithmetic on a {} value", v.type_name())),
            _ => {
                dsum(&mdb, ModType::Base, Some(&dot_type), &["DotMultiplier"])
                    + dsum(&mdb, ModType::Base, Some(&dot_type), &["FireDotMultiplier"])
            }
        };
        let total = base_drops * (1.0 + inc / 100.0) * more * (1.0 + mult / 100.0) * eff_mult;
        let cur = off.gval("BurningGroundDPS").cloned();
        let replace = match cur {
            None | Some(crate::output::OutValue::Bool(false)) => true,
            Some(crate::output::OutValue::Num(x)) => x < total,
            Some(o) => panic!("{LUA}:5764: attempt to compare {o:?} with number"),
        };
        if replace {
            off.out().set_num("BurningGroundDPS", total);
            off.out().set_bool("BurningGroundFromIgnite", false);
        }
    }
}
