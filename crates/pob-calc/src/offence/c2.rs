//! CalcOffence.lua:3346-4154 (R7.5, part C, second half): base hit damage (:3346-3383), hit damage by type with crits, luck, enemy
//! resistances / armour and leech per hit (:3385-3699), damage-type conditions (:3701-3718), leech instances, gain on hit / kill, enemy
//! regeneration, average hit / damage / burst / DPS (:3720-3854), PvP (:3857-3954); after the loop the hand combine (:3957-4043),
//! `DisplayDamage` (CALCS), the minion summon speed and the leech rates (:4100-4153). OC4 (:4156) is taken right after.
//!
//! Hash-order walks (R13/D13): none -- every walk is `ipairs(dmgTypeList)`; the totals are summed in that fixed order.
//! `^` (R2): `rolls` here is never a power; the crit roll / cull / PvP powers are `f64::powf` (UCRT = desktop LuaJIT).
//!
//! Breakdown (R8): the `if breakdown` branches only fill breakdown tables; the queries in their conditions that no earlier code made
//! (`Flag(skillCfg, "CritLucky")`, :3815/:3819) are still made.

use pob_lua::number::{max2, min2};
use pob_mods::{ModCfg, ModType, Str, Value};

use super::c::{LUA, config_is, config_truthy, dflag, dmore, dsum, num_at, or_num};
use super::helpers::{DMG_TYPE_LIST, calc_damage};
use super::{CombineMode, Off};
use crate::output::OutValue;
use crate::setup::{misc, ok};

/// `isElemental` (:31).
fn is_elemental(t: &str) -> bool {
    matches!(t, "Fire" | "Cold" | "Lightning")
}

/// :494-507 `calcResistForType(damageType, cfg)`; `cfg` is nil at :3527/:3538 (the global `dotCfg`: the `local dotCfg` of the ailment
/// sections is declared later) and an ailment / Decay / burning ground cfg in part D (R7.6).
pub(super) fn resist_for_type(off: &Off<'_>, dt: &str, cfg: Option<&ModCfg>) -> f64 {
    let enemy = &off.enemy_db;
    let rname = format!("{dt}Resist");
    let resist = ok(enemy.borrow().override_(cfg, &[&rname])).filter(Value::truthy);
    let floor_max = misc("EnemyMaxResist");
    let max_resist = if dflag(enemy, None, &["DoNotChangeMaxResFromConfig"]) {
        floor_max
    } else {
        let cfg_v = match off.env.config.input.get(&format!("enemy{dt}Resist")) {
            None | Some(pob_build::ConfigValue::Bool(false)) => floor_max,
            Some(pob_build::ConfigValue::Num(x)) => *x,
            Some(o) => panic!("{LUA}:496: bad argument #1 to 'max' (number expected, got {o:?})"),
        };
        min2(max2(cfg_v, floor_max), misc("MaxResistCap"))
    };
    let resist = match resist {
        Some(v) => v
            .as_num()
            .unwrap_or_else(|| panic!("{LUA}:506: bad argument #1 to 'min' (number expected, got {})", v.type_name())),
        None => {
            let flag_name = format!("Enemy{dt}ResistEqualToYours");
            if dflag(&off.mod_db, None, &[&flag_name]) {
                off.st
                    .player
                    .output
                    .opt_num(&rname)
                    .unwrap_or_else(|| panic!("{LUA}:506: bad argument #1 to 'min' (number expected, got nil)"))
            } else if dflag(&off.st.party_members.mod_db, None, &[&flag_name]) {
                // the PartyTab stand-in's output is empty (P5)
                panic!("{LUA}:506: bad argument #1 to 'min' (number expected, got nil)")
            } else {
                let mut names = vec![rname.as_str()];
                if is_elemental(dt) {
                    names.push("ElementalResist");
                }
                dsum(enemy, ModType::Base, cfg, &names) * max2(ok(crate::calc_lib::mod_(&enemy.borrow(), cfg, &names)), 0.0)
            }
        }
    };
    max2(min2(resist, max_resist), misc("ResistFloor"))
}

/// :3346-4154 the rest of the loop body for pass `pi`.
#[allow(clippy::too_many_lines)]
pub(super) fn hit_damage(off: &mut Off<'_>, pi: usize, hit_rate: f64, quantity_multiplier: f64) {
    let q = off.q.clone();
    let eff = off.env.mode_effective;
    let calcs = off.env.mode == "CALCS";

    // :3346-3383 base hit damage
    let (lvl_base_mult, lvl_dmg_eff) = {
        let st = off.env.skills.as_ref().expect("skills on");
        let e = off.sk().active_effect.borrow();
        let ge = &st.skills.effects[e.granted_effect];
        let lvl = e
            .granted_effect_level
            .and_then(|l| ge.level(l))
            .unwrap_or_else(|| panic!("{LUA}:3350: attempt to index field 'grantedEffectLevel' (a nil value)"));
        (lvl.base_multiplier, lvl.damage_effectiveness)
    };
    for dt in DMG_TYPE_LIST {
        let (dmin, dmax) = (format!("{dt}Min"), format!("{dt}Max"));
        let base_multiplier = lvl_base_mult.or_else(|| off.sd_num("baseMultiplier", 3350)).unwrap_or(1.0);
        let damage_effectiveness = lvl_dmg_eff.or_else(|| off.sd_num("damageEffectiveness", 3351)).unwrap_or(1.0);
        let c = Some(off.pcfg(pi));
        let added_min = q.base(c, &[&dmin]) + dsum(&off.enemy_db, ModType::Base, c, &[&format!("Self{dmin}")]);
        let added_max = q.base(c, &[&dmax]) + dsum(&off.enemy_db, ModType::Base, c, &[&format!("Self{dmax}")]);
        let added_mult = q.calc_mod(c, &[&format!("Added{dt}Damage"), "AddedDamage"]);
        let src = |k: &str| off.source_num(pi, k, 3355).unwrap_or(0.0);
        let base_min = (src(&dmin) + src(&format!("{dt}BonusMin"))) * base_multiplier + added_min * damage_effectiveness * added_mult;
        let base_max = (src(&dmax) + src(&format!("{dt}BonusMax"))) * base_multiplier + added_max * damage_effectiveness * added_mult;
        off.pset_num(pi, &format!("{dmin}Base"), base_min);
        off.pset_num(pi, &format!("{dmax}Base"), base_max);
    }

    // :3385-3690 hit damage for each damage type
    let (mut total_hit_min, mut total_hit_max, mut total_hit_avg) = (0.0, 0.0, 0.0);
    // `totalCritMin` / `totalCritMax` feed only the breakdown (:3816-3820)
    let mut total_crit_avg = 0.0;
    let ghost_reaver = q.flag(None, &["GhostReaver"]);
    let mut ghost_reaver_life_leech = 0.0;
    for k in [
        "LifeLeech",
        "LifeLeechInstant",
        "EnergyShieldLeech",
        "EnergyShieldLeechInstant",
        "ManaLeech",
        "ManaLeechInstant",
        "impaleStoredHitAvg",
    ] {
        off.pset_num(pi, k, 0.0);
    }
    for pass in [1, 2] {
        off.set_pass_cond(pi, "CriticalStrike", pass == 1);
        let (mut life_leech_total, mut es_leech_total, mut mana_leech_total) = (0.0, 0.0, 0.0);
        let c = Some(off.pcfg(pi));
        let edb = &off.enemy_db;
        let no_life_leech =
            q.flag(c, &["CannotLeechLife"]) || dflag(edb, None, &["CannotLeechLifeFromSelf"]) || q.flag(c, &["CannotGainLife"]);
        let no_es_leech = q.flag(c, &["CannotLeechEnergyShield"])
            || dflag(edb, None, &["CannotLeechEnergyShieldFromSelf"])
            || q.flag(c, &["CannotGainEnergyShield"]);
        let no_mana_leech =
            q.flag(c, &["CannotLeechMana"]) || dflag(edb, None, &["CannotLeechManaFromSelf"]) || q.flag(c, &["CannotGainMana"]);
        for dt in DMG_TYPE_LIST {
            let (mut hit_min, mut hit_max, mut hit_avg) = (0.0, 0.0, 0.0);
            if off.flag("hit") && off.can_deal.get(dt).copied().unwrap_or(false) {
                let (mn, mx) = {
                    let base = |t: &str| -> (f64, f64) {
                        (
                            num_at(off.pval(pi, &format!("{t}MinBase")), "MinBase", 92),
                            num_at(off.pval(pi, &format!("{t}MaxBase")), "MaxBase", 93),
                        )
                    };
                    calc_damage(&q, off.pass_conversion(pi), &base, Some(off.pcfg(pi)), dt, 0, None)
                };
                hit_min = mn;
                hit_max = mx;
                let conv_mult = off
                    .pass_conversion(pi)
                    .get(dt)
                    .map(|e| e.mult)
                    .unwrap_or_else(|| panic!("{LUA}:3411: attempt to index a nil value"));
                let warcry_max_hit = q.flag(None, &["Condition:WarcryMaxHit"]);
                let warcry = if warcry_max_hit {
                    num_at(off.gval("MaxOffensiveWarcryEffect"), "MaxOffensiveWarcryEffect", 3441)
                } else {
                    num_at(off.gval("OffensiveWarcryEffect"), "OffensiveWarcryEffect", 3443)
                };
                let all_mult = conv_mult
                    * num_at(off.pval(pi, "ScaledDamageEffect"), "ScaledDamageEffect", 3441)
                    * num_at(off.pval(pi, "RuthlessBlowHitEffect"), "RuthlessBlowHitEffect", 3441)
                    * num_at(off.pval(pi, "FistOfWarDamageEffect"), "FistOfWarDamageEffect", 3441)
                    * warcry;
                off.pset_num(pi, "allMult", all_mult);
                let mut all_mult = all_mult;
                if pass == 1 {
                    all_mult *= num_at(off.pval(pi, "CritMultiplier"), "CritMultiplier", 3448);
                }
                hit_min *= all_mult;
                hit_max *= all_mult;
                let sc = Some(&off.cfg);
                let mdb = &off.mod_db;
                let lucky = q.flag(sc, &["LuckyHits"])
                    || (pass == 2 && dt == "Lightning" && q.flag(sc, &["LightningNoCritLucky"]))
                    || (pass == 1 && q.flag(sc, &["CritLucky"]))
                    || (dt == "Lightning" && dflag(mdb, None, &["LightningLuckHits"]))
                    || (dt == "Chaos" && dflag(mdb, None, &["ChaosLuckyHits"]))
                    || (dt == "Fire" && dflag(mdb, None, &["FireLuckyHits"]))
                    || (dt == "Cold" && dflag(mdb, None, &["ColdLuckyHits"]))
                    || (is_elemental(dt) && q.flag(sc, &["ElementalLuckHits"]));
                let mut lucky_chance = if lucky {
                    1.0
                } else {
                    min2(q.base(sc, &["LuckyHitsChance"]), 100.0) / 100.0
                };
                if q.flag(sc, &["UnluckyHits"]) {
                    lucky_chance -= 1.0;
                }
                let mut rolls = lucky_chance;
                if q.flag(sc, &["ExtremeLuck"]) {
                    rolls *= 2.0;
                }
                if q.flag(sc, &["Unexciting"]) {
                    rolls = 0.0;
                }
                off.pset_num(pi, "DamageRolls", rolls);
                if pass == 1 {
                    off.pset_num(pi, "CritDamageRolls", rolls);
                }
                let rolls = rolls.abs() + 2.0;
                let avg_not_lucky = hit_min / rolls + hit_max / rolls;
                let avg_lucky = hit_min / rolls + (rolls - 1.0) * hit_max / rolls;
                let avg_unlucky = (rolls - 1.0) * hit_min / rolls + hit_max / rolls;
                hit_avg = if lucky_chance >= 0.0 {
                    avg_not_lucky * (1.0 - lucky_chance) + avg_lucky * lucky_chance
                } else {
                    avg_not_lucky * (1.0 - lucky_chance.abs()) + avg_unlucky * lucky_chance.abs()
                };
                if (hit_min != 0.0 || hit_max != 0.0) && eff {
                    let eff_mult = enemy_eff_mult(off, &q, pi, pass, dt, hit_avg);
                    hit_min *= eff_mult;
                    hit_max *= eff_mult;
                    hit_avg *= eff_mult;
                    if calcs {
                        off.pset_num(pi, &format!("{dt}EffMult"), eff_mult);
                    }
                }

                // :3613-3639 leech for this damage type
                let c = Some(off.pcfg(pi));
                let edb = &off.enemy_db;
                let (mut life_leech, mut es_leech, mut mana_leech) = (0.0, 0.0, 0.0);
                if off.flag("mine") || off.flag("trap") || off.flag("totem") {
                    life_leech = q.base(c, &["DamageLifeLeechToPlayer"]);
                } else {
                    let ele = is_elemental(dt);
                    let self_life = || dsum(edb, ModType::Base, c, &["SelfDamageLifeLeech"]) / 100.0;
                    if q.flag(None, &["LifeLeechBasedOnChaosDamage"]) {
                        if dt == "Chaos" {
                            life_leech = q.base(
                                c,
                                &[
                                    "DamageLeech",
                                    "DamageLifeLeech",
                                    "PhysicalDamageLifeLeech",
                                    "LightningDamageLifeLeech",
                                    "ColdDamageLifeLeech",
                                    "FireDamageLifeLeech",
                                    "ChaosDamageLifeLeech",
                                    "ElementalDamageLifeLeech",
                                ],
                            ) + self_life();
                        }
                    } else {
                        let n = format!("{dt}DamageLifeLeech");
                        let mut names = vec!["DamageLeech", "DamageLifeLeech", n.as_str()];
                        if ele {
                            names.push("ElementalDamageLifeLeech");
                        }
                        life_leech = q.base(c, &names) + self_life();
                    }
                    let n = format!("{dt}DamageEnergyShieldLeech");
                    let mut names = vec!["DamageEnergyShieldLeech", n.as_str()];
                    if ele {
                        names.push("ElementalDamageEnergyShieldLeech");
                    }
                    es_leech = q.base(c, &names) + dsum(edb, ModType::Base, c, &["SelfDamageEnergyShieldLeech"]) / 100.0;
                    let n = format!("{dt}DamageManaLeech");
                    let mut names = vec!["DamageLeech", "DamageManaLeech", n.as_str()];
                    if ele {
                        names.push("ElementalDamageManaLeech");
                    }
                    mana_leech = q.base(c, &names) + dsum(edb, ModType::Base, c, &["SelfDamageManaLeech"]) / 100.0;
                }
                if life_leech > 0.0 && !no_life_leech {
                    life_leech_total += hit_avg * life_leech / 100.0;
                }
                if mana_leech > 0.0 && !no_mana_leech {
                    mana_leech_total += hit_avg * mana_leech / 100.0;
                }
                if es_leech > 0.0 && !no_es_leech {
                    es_leech_total += hit_avg * es_leech / 100.0;
                }
            }
            if pass == 1 {
                off.pset_num(pi, &format!("{dt}CritAverage"), hit_avg);
                total_crit_avg += hit_avg;
            } else {
                if calcs {
                    off.pset_num(pi, &format!("{dt}Min"), hit_min);
                    off.pset_num(pi, &format!("{dt}Max"), hit_max);
                }
                off.pset_num(pi, &format!("{dt}HitAverage"), hit_avg);
                total_hit_avg += hit_avg;
                total_hit_min += hit_min;
                total_hit_max += hit_max;
            }
        }
        if let Some(v) = off.sd_num("lifeLeechPerUse", 3664)
            && !no_life_leech
        {
            life_leech_total += v;
        }
        if let Some(v) = off.sd_num("manaLeechPerUse", 3667) {
            mana_leech_total += v;
        }
        // :3670-3680 leech caps per instance
        let g = |off: &Off<'_>, k: &str| num_at(off.gval(k), k, 3672);
        life_leech_total = if ghost_reaver {
            min2(life_leech_total, g(off, "MaxEnergyShieldLeechInstance"))
        } else {
            min2(life_leech_total, g(off, "MaxLifeLeechInstance"))
        };
        es_leech_total = min2(es_leech_total, g(off, "MaxEnergyShieldLeechInstance"));
        mana_leech_total = min2(mana_leech_total, g(off, "MaxManaLeechInstance"));
        if ghost_reaver && no_es_leech {
            life_leech_total = 0.0;
        }
        let cc = num_at(off.pval(pi, "CritChance"), "CritChance", 3682);
        let portion = if pass == 1 { cc / 100.0 } else { 1.0 - cc / 100.0 };
        if ghost_reaver {
            ghost_reaver_life_leech += life_leech_total * portion;
        } else {
            let v = num_at(off.pval(pi, "LifeLeech"), "LifeLeech", 3686) + life_leech_total * portion;
            off.pset_num(pi, "LifeLeech", v);
        }
        let v = num_at(off.pval(pi, "EnergyShieldLeech"), "EnergyShieldLeech", 3688) + es_leech_total * portion;
        off.pset_num(pi, "EnergyShieldLeech", v);
        let v = num_at(off.pval(pi, "ManaLeech"), "ManaLeech", 3689) + mana_leech_total * portion;
        off.pset_num(pi, "ManaLeech", v);
    }
    off.pset_num(pi, "TotalMin", total_hit_min);
    off.pset_num(pi, "TotalMax", total_hit_max);

    // :3694-3699 Elemental Equilibrium
    let hit_avg_of = |off: &Off<'_>, dt: &str| num_at(off.pval(pi, &format!("{dt}HitAverage")), "HitAverage", 3694);
    if q.flag(Some(&off.cfg), &["ElementalEquilibrium"])
        && !config_truthy(off, "EEIgnoreHitDamage")
        && (hit_avg_of(off, "Fire") + hit_avg_of(off, "Cold") + hit_avg_of(off, "Lightning") > 0.0)
    {
        let (f, c, l) = (
            hit_avg_of(off, "Fire") > 0.0,
            hit_avg_of(off, "Cold") > 0.0,
            hit_avg_of(off, "Lightning") > 0.0,
        );
        let mut e = off.enemy_db.borrow_mut();
        e.conditions.insert(Str::from("HitByFireDamage"), f);
        e.conditions.insert(Str::from("HitByColdDamage"), c);
        e.conditions.insert(Str::from("HitByLightningDamage"), l);
    }

    // :3701-3718
    let mut highest_type = "Physical";
    for dt in DMG_TYPE_LIST {
        if hit_avg_of(off, dt) > 0.0 {
            q.new_mod(
                &format!("Condition:{dt}HasDamage"),
                Some(ModType::Flag),
                Value::Bool(true),
                Some("Config"),
                0,
                0,
                vec![],
            );
            if hit_avg_of(off, dt) > hit_avg_of(off, highest_type) {
                highest_type = dt;
            }
        }
    }
    if !q.flag(None, &["IsHighestDamageTypeOVERRIDE"]) {
        q.new_mod(
            &format!("Condition:{highest_type}IsHighestDamageType"),
            Some(ModType::Flag),
            Value::Bool(true),
            Some("Config"),
            0,
            0,
            vec![],
        );
    }

    // :3720-3756 leech
    let leech_rate_base = misc("LeechRateBase");
    let instances = |amount: f64, total: f64| -> (f64, f64) {
        if total == 0.0 {
            return (0.0, 0.0);
        }
        let duration = amount / total / leech_rate_base;
        (duration, duration * hit_rate)
    };
    let pnum = |off: &Off<'_>, k: &str, line: u32| num_at(off.pval(pi, k), k, line);
    let instant_prop = |off: &Off<'_>, name: &str| max2(min2(q.base(Some(off.pcfg(pi)), &[name]), 100.0), 0.0) / 100.0;
    let lp = instant_prop(off, "InstantLifeLeech");
    off.pset_num(pi, "LifeLeechInstantProportion", lp);
    if lp > 0.0 {
        let l = pnum(off, "LifeLeech", 3732);
        off.pset_num(pi, "LifeLeechInstant", l * lp);
        off.pset_num(pi, "LifeLeech", l * (1.0 - lp));
    }
    if ghost_reaver && ghost_reaver_life_leech > 0.0 {
        let v = pnum(off, "EnergyShieldLeech", 3736) + ghost_reaver_life_leech;
        off.pset_num(pi, "EnergyShieldLeech", v);
        off.pset_num(pi, "LifeLeech", 0.0);
        off.pset_num(pi, "LifeLeechInstant", 0.0);
    }
    let ep = instant_prop(off, "InstantEnergyShieldLeech");
    off.pset_num(pi, "EnergyShieldLeechInstantProportion", ep);
    if ep > 0.0 {
        let l = pnum(off, "EnergyShieldLeech", 3742);
        off.pset_num(pi, "EnergyShieldLeechInstant", l * ep);
        off.pset_num(pi, "EnergyShieldLeech", l * (1.0 - ep));
    }
    let mp = instant_prop(off, "InstantManaLeech");
    off.pset_num(pi, "ManaLeechInstantProportion", mp);
    if mp > 0.0 {
        let l = pnum(off, "ManaLeech", 3747);
        off.pset_num(pi, "ManaLeechInstant", l * mp);
        off.pset_num(pi, "ManaLeech", l * (1.0 - mp));
    }
    for (pool, gk, line) in [
        ("Life", "Life", 3751),
        ("EnergyShield", "EnergyShield", 3753),
        ("Mana", "Mana", 3755),
    ] {
        let total = num_at(off.gval(gk), gk, line);
        let (d, n) = instances(pnum(off, &format!("{pool}Leech"), line), total);
        off.pset_num(pi, &format!("{pool}LeechDuration"), d);
        off.pset_num(pi, &format!("{pool}LeechInstances"), n);
        let r = pnum(off, &format!("{pool}LeechInstant"), line + 1) * hit_rate;
        off.pset_num(pi, &format!("{pool}LeechInstantRate"), r);
    }

    // :3758-3781 gain on hit / kill
    let mtt = off.flag("mine") || off.flag("trap") || off.flag("totem");
    let c = Some(off.pcfg(pi));
    let edb = &off.enemy_db;
    let (life_on_hit, es_on_hit, mana_on_hit) = if mtt {
        (0.0, 0.0, 0.0)
    } else {
        (
            if !q.flag(c, &["CannotGainLife"]) && !q.flag(c, &["CannotRecoverLifeOutsideLeech"]) {
                q.base(c, &["LifeOnHit"]) + dsum(edb, ModType::Base, c, &["SelfLifeOnHit"])
            } else {
                0.0
            },
            if q.flag(c, &["CannotGainEnergyShield"]) {
                0.0
            } else {
                q.base(c, &["EnergyShieldOnHit"]) + dsum(edb, ModType::Base, c, &["SelfEnergyShieldOnHit"])
            },
            if q.flag(c, &["CannotGainMana"]) {
                0.0
            } else {
                q.base(c, &["ManaOnHit"]) + dsum(edb, ModType::Base, c, &["SelfManaOnHit"])
            },
        )
    };
    off.pset_num(pi, "LifeOnHit", life_on_hit);
    off.pset_num(pi, "EnergyShieldOnHit", es_on_hit);
    off.pset_num(pi, "ManaOnHit", mana_on_hit);
    off.pset_num(pi, "LifeOnHitRate", life_on_hit * hit_rate);
    off.pset_num(pi, "EnergyShieldOnHitRate", es_on_hit * hit_rate);
    off.pset_num(pi, "ManaOnHitRate", mana_on_hit * hit_rate);
    let c = Some(off.pcfg(pi));
    let (life_on_kill, es_on_kill, mana_on_kill) = if mtt {
        (0.0, 0.0, 0.0)
    } else {
        (
            if !q.flag(c, &["CannotGainLife"]) && !q.flag(c, &["CannotRecoverLifeOutsideLeech"]) {
                q.base(c, &["LifeOnKill"]).floor()
            } else {
                0.0
            },
            if q.flag(c, &["CannotGainEnergyShield"]) {
                0.0
            } else {
                q.base(c, &["EnergyShieldOnKill"]).floor()
            },
            if q.flag(c, &["CannotGainMana"]) {
                0.0
            } else {
                q.base(c, &["ManaOnKill"]).floor()
            },
        )
    };
    off.pset_num(pi, "LifeOnKill", life_on_kill);
    off.pset_num(pi, "EnergyShieldOnKill", es_on_kill);
    off.pset_num(pi, "ManaOnKill", mana_on_kill);

    // :3784-3786
    for (k, n) in [
        ("EnemyLifeRegen", "LifeRegen"),
        ("EnemyManaRegen", "ManaRegen"),
        ("EnemyEnergyShieldRegen", "EnergyShieldRegen"),
    ] {
        let v = dsum(&off.enemy_db, ModType::Inc, Some(off.pcfg(pi)), &[n]);
        off.pset_num(pi, k, v);
    }

    // :3789-3801 average damage and DPS
    let cc = pnum(off, "CritChance", 3789);
    let average_hit = total_hit_avg * (1.0 - cc / 100.0) + total_crit_avg * cc / 100.0;
    off.pset_num(pi, "AverageHit", average_hit);
    if off.flag("monsterExplode") {
        off.pset_num(pi, "AverageHitToMonsterLifePercentage", average_hit / off.monster_life * 100.0);
        if off.sd_truthy("hitChanceIsExplodeChance") {
            let v = off.pval(pi, "ExplodeChance").cloned();
            off.pset_val(pi, "HitChance", v);
        }
    }
    let average_damage = average_hit * pnum(off, "HitChance", 3796) / 100.0;
    off.pset_num(pi, "AverageDamage", average_damage);
    let burst_hits = or_num(off.pval(pi, "AverageBurstHits"), 1.0, "AverageBurstHits", 3797);
    off.gset("AverageBurstHits", burst_hits);
    let repeat_penalty = if q.flag(None, &["HasSeals"]) && off.has_type("CanRapidFire") && !q.flag(None, &["NoRepeatBonuses"]) {
        q.calc_mod(Some(&off.cfg), &["SealRepeatPenalty"])
    } else {
        1.0
    };
    off.gset(
        "AverageBurstDamage",
        average_damage + average_damage * (burst_hits - 1.0) * repeat_penalty,
    );
    off.out().set_bool("ShowBurst", burst_hits > 1.0);
    let dps_mult = off
        .sd_num("dpsMultiplier", 3801)
        .unwrap_or_else(|| panic!("{LUA}:3801: attempt to perform arithmetic on field 'dpsMultiplier' (a nil value)"));
    let total_dps = average_damage * off.g_hit_speed(3801) * dps_mult * quantity_multiplier;
    off.pset_num(pi, "TotalDPS", total_dps);
    if off.breakdown && pnum(off, "CritEffect", 3803) != 1.0 {
        // :3815/:3819 the conditions' `Flag(skillCfg, "CritLucky")` (breakdown rows themselves: R8)
        let dr = or_num(off.pval(pi, "DamageRolls"), 0.0, "DamageRolls", 3805);
        let cr = or_num(off.pval(pi, "CritDamageRolls"), 0.0, "CritDamageRolls", 3806);
        if !(dr > 0.0 || cr > 0.0) {
            let _ = q.flag(Some(&off.cfg), &["CritLucky"]);
        }
        if dr < 0.0 {
            let _ = q.flag(Some(&off.cfg), &["CritLucky"]);
        }
    }

    // :3857-3954 PvP
    for f in [
        "isPvP",
        "notAttackPvP",
        "attackPvP",
        "weapon1AttackPvP",
        "weapon2AttackPvP",
        "notAveragePvP",
    ] {
        off.set_flag(f, false);
    }
    if config_truthy(off, "PvpScaling") {
        pvp(off, pi, total_hit_min, total_hit_max);
    }
}

/// :3487-3608 enemy resistances / armour and damage taken: the effective DPS multiplier of one damage type.
#[allow(clippy::too_many_lines)]
fn enemy_eff_mult(off: &mut Off<'_>, q: &super::Q, pi: usize, pass: u8, dt: &'static str, hit_avg: f64) -> f64 {
    let c = Some(off.pcfg(pi));
    let edb = off.enemy_db.clone();
    let mut resist;
    let mut pen = 0.0;
    let dtt = format!("{dt}DamageTaken");
    let mut taken_inc = dsum(&edb, ModType::Inc, c, &["DamageTaken", &dtt]);
    let taken_more = dmore(&edb, c, &["DamageTaken", &dtt]);
    let use_this_resist = |off: &Off<'_>, t: &str| -> f64 {
        let c = Some(off.pcfg(pi));
        let n = format!("Ignore{t}Resistance");
        let mut names = vec![n.as_str()];
        if is_elemental(t) {
            names.push("IgnoreElementalResistances");
        }
        if q.flag(c, &names) || dflag(&off.enemy_db, None, &[&format!("SelfIgnore{t}Resistance")]) {
            return 0.0;
        }
        let n = format!("ChanceToIgnore{t}Resistance");
        let mut names = vec![n.as_str()];
        if is_elemental(t) {
            names.push("ChanceToIgnoreElementalResistances");
        }
        let chance = min2(max2(q.base(c, &names), 0.0), 100.0);
        1.0 - chance / 100.0
    };
    if dt == "Physical" {
        let cc = num_at(off.pval(pi, "CritChance"), "CritChance", 3506);
        let stored = num_at(off.pval(pi, "impaleStoredHitAvg"), "impaleStoredHitAvg", 3506)
            + if pass == 1 {
                hit_avg * (cc / 100.0)
            } else {
                hit_avg * (1.0 - cc / 100.0)
            };
        off.pset_num(pi, "impaleStoredHitAvg", stored);
        let c = Some(off.pcfg(pi));
        let enemy_armour = max2(ok(crate::calc_lib::val(&edb.borrow(), "Armour", None)), 0.0);
        let armour_reduction = crate::pools::armour_reduction_f(enemy_armour, hit_avg * q.more(c, &["CalcArmourAsThoughDealing"]));
        let mut ignore_chance = min2(q.base(c, &["ChanceToIgnoreEnemyPhysicalDamageReduction"]), 100.0);
        if ignore_chance > 0.0 && ignore_chance < 100.0 {
            if config_is(off, "ChanceToIgnoreEnemyPhysicalDamageReductionMode", "MAX") {
                ignore_chance = 100.0;
            } else if config_is(off, "ChanceToIgnoreEnemyPhysicalDamageReductionMode", "MIN") {
                ignore_chance = 0.0;
            }
        }
        if q.flag(c, &["IgnoreEnemyPhysicalDamageReduction"]) || ignore_chance >= 100.0 {
            resist = 0.0;
        } else {
            resist = min2(
                max2(
                    0.0,
                    dsum(&edb, ModType::Base, None, &["PhysicalDamageReduction"])
                        + q.base(c, &["EnemyPhysicalDamageReduction"])
                        + armour_reduction,
                ),
                misc("EnemyPhysicalDamageReductionCap"),
            );
            if resist > 0.0 {
                resist *= 1.0 - (q.base(None, &["PartialIgnoreEnemyPhysicalDamageReduction"]) / 100.0 + ignore_chance / 100.0);
            }
        }
    } else {
        resist = resist_for_type(off, dt, None);
        let c = Some(off.pcfg(pi));
        if ((q.flag(c, &["ChaosDamageUsesLowestResistance"]) || q.flag(c, &["ChaosDamageUsesHighestResistance"])) && dt == "Chaos")
            || (q.flag(c, &["ElementalDamageUsesLowestResistance"]) && is_elemental(dt))
        {
            let mut element_used = dt;
            if is_elemental(dt) {
                taken_inc += dsum(&edb, ModType::Inc, c, &["ElementalDamageTaken"]);
            }
            for ele in DMG_TYPE_LIST {
                if is_elemental(ele) && use_this_resist(off, ele) > 0.0 && dt != ele {
                    let current = resist_for_type(off, ele, None);
                    if q.flag(Some(off.pcfg(pi)), &["ChaosDamageUsesHighestResistance"]) {
                        if resist < current {
                            resist = current;
                            element_used = ele;
                        }
                    } else if resist > current {
                        resist = current;
                        element_used = ele;
                    }
                }
            }
            let c = Some(off.pcfg(pi));
            if is_elemental(element_used) {
                pen = q.base(c, &[&format!("{element_used}Penetration"), "ElementalPenetration"]);
            } else if element_used == "Chaos" {
                pen = q.base(c, &["ChaosPenetration"]);
            }
        } else if is_elemental(dt) {
            pen = q.base(c, &[&format!("{dt}Penetration"), "ElementalPenetration"]);
            taken_inc += dsum(&edb, ModType::Inc, c, &["ElementalDamageTaken"]);
        } else if dt == "Chaos" {
            pen = q.base(c, &["ChaosPenetration"]);
        }
    }
    let c = Some(off.pcfg(pi));
    let invert_ele = max2(min2(q.sum(ModType::Chance, c, &["HitsInvertEleResChance"]), 1.0), 0.0);
    let invert_chaos = max2(min2(q.sum(ModType::Chance, c, &["HitsInvertChaosResChance"]), 1.0), 0.0);
    let invert = if is_elemental(dt) {
        invert_ele
    } else if dt == "Chaos" {
        invert_chaos
    } else {
        0.0
    };
    // :3570 `sourceRes` (breakdown only) -- its flag queries are on `env.modDB` / the party stand-in (no malformed guard): skipped
    if off.flag("projectile") {
        taken_inc += dsum(&edb, ModType::Inc, None, &["ProjectileDamageTaken"]);
    }
    if off.flag("projectile") && off.flag("attack") {
        taken_inc += dsum(&edb, ModType::Inc, None, &["ProjectileAttackDamageTaken"]);
    }
    if off.flag("trap") || off.flag("mine") {
        taken_inc += dsum(&edb, ModType::Inc, None, &["TrapMineDamageTaken"]);
    }
    let mut eff_mult = (1.0 + taken_inc / 100.0) * taken_more;
    let use_res_chance = use_this_resist(off, dt);
    let use_res = use_res_chance > 0.0;
    // `Flag(cfg, isElemental[damageType] and "CannotElePenIgnore" or nil)` -- a nil name matches nothing
    let cannot_ele_pen_ignore = is_elemental(dt) && q.flag(Some(off.pcfg(pi)), &["CannotElePenIgnore"]);
    if cannot_ele_pen_ignore {
        let effective = if invert > 0.0 { resist - 2.0 * invert * resist } else { resist };
        eff_mult *= 1.0 - effective / 100.0;
    } else if use_res {
        let effective = if invert > 0.0 {
            ((resist - pen) * (1.0 - invert) + (-resist - pen) * invert) * use_res_chance
        } else {
            (resist - pen) * use_res_chance
        };
        eff_mult *= 1.0 - effective / 100.0;
    }
    // :3601 the breakdown branch's condition repeats `Flag(cfg, "CannotElePenIgnore")` (same query): nothing new
    eff_mult
}

/// :3867-3954 PvP values.
fn pvp(off: &mut Off<'_>, pi: usize, total_hit_min: f64, total_hit_max: f64) {
    let q = off.q.clone();
    off.set_flag("isPvP", true);
    let attack = off.flag("attack");
    off.set_flag("attackPvP", attack);
    off.set_flag("notAttackPvP", !attack);
    let w1 = off.flag("weapon1Attack");
    off.set_flag("weapon1AttackPvP", w1);
    let w2 = off.flag("weapon2Attack");
    off.set_flag("weapon2AttackPvP", w2);
    let na = off.flag("notAverage");
    off.set_flag("notAveragePvP", na);
    let mut t = match off.env.config.input.get("multiplierPvpTvalueOverride") {
        None | Some(pob_build::ConfigValue::Bool(false)) => None,
        Some(pob_build::ConfigValue::Num(x)) => Some(*x / 1000.0),
        Some(o) => panic!("{LUA}:3876: attempt to perform arithmetic on local 'PvpTvalue' ({o:?})"),
    };
    if t.is_none() {
        let asm = || num_at(off.gval("ActionSpeedMod"), "ActionSpeedMod", 3881);
        let mut v = if let Some(cd) = off.sd_num("cooldown", 3879) {
            cd
        } else if off.flag("mine") {
            or_num(off.pval(pi, "MineLayingTime"), 1.0, "MineLayingTime", 3881) / asm()
        } else if off.flag("trap") {
            or_num(off.pval(pi, "TrapThrowingTime"), 1.0, "TrapThrowingTime", 3883) / asm()
        } else {
            1.0 / (off.g_hit_speed(3885) / asm()) * q.more(Some(off.pcfg(pi)), &["PvpTvalueMultiplier"])
        };
        if v > 2_147_483_647.0 {
            v = 1.0;
        }
        t = Some(v);
    }
    let t = t.expect("set above");
    let mult = q.more(Some(off.pcfg(pi)), &["PvpDamageMultiplier"]);
    let (ne1, ne2, e1, e2) = (
        misc("PvpNonElemental1"),
        misc("PvpNonElemental2"),
        misc("PvpElemental1"),
        misc("PvpElemental2"),
    );
    let pn = |off: &Off<'_>, k: &str| num_at(off.pval(pi, k), k, 3898);
    let pct_non_ele = (pn(off, "PhysicalHitAverage") + pn(off, "ChaosHitAverage")) / (total_hit_min + total_hit_max) * 2.0;
    let pct_ele = 1.0 - pct_non_ele;
    let avg_hit = pn(off, "AverageHit");
    let portion_non_ele = (avg_hit / t / ne2).powf(ne1) * t * ne2 * pct_non_ele;
    let portion_ele = (avg_hit / t / e2).powf(e1) * t * e2 * pct_ele;
    let mut pvp_hit = (portion_non_ele + portion_ele) * mult;
    let mut pvp_dmg = pvp_hit * pn(off, "HitChance") / 100.0;
    let dps_mult = off
        .sd_num("dpsMultiplier", 3904)
        .unwrap_or_else(|| panic!("{LUA}:3904: attempt to perform arithmetic on field 'dpsMultiplier' (a nil value)"));
    let mut pvp_dps = pvp_dmg * off.g_hit_speed(3904) * dps_mult;
    if pvp_hit.is_nan() {
        pvp_hit = 0.0;
    }
    if pvp_dmg.is_nan() {
        pvp_dmg = 0.0;
    }
    if pvp_dps.is_nan() {
        pvp_dps = 0.0;
    }
    off.pset_num(pi, "PvpAverageHit", pvp_hit);
    off.pset_num(pi, "PvpAverageDamage", pvp_dmg);
    off.pset_num(pi, "PvpTotalDPS", pvp_dps);
}

/// :3957-4153 after the pass loop: the hand combine, `DisplayDamage` (CALCS), the minion summon speed, the leech rates.
#[allow(clippy::too_many_lines)]
pub(super) fn after_passes(off: &mut Off<'_>) {
    let q = off.q.clone();
    if off.is_attack {
        use super::b::combine_stat;
        combine_stat(off, "PreEffectiveCritChance", CombineMode::Average, None);
        combine_stat(off, "CritChance", CombineMode::Average, None);
        combine_stat(off, "CritMultiplier", CombineMode::Average, None);
        for stat in [
            "AverageDamage",
            "PvpAverageDamage",
            "TotalDPS",
            "PvpTotalDPS",
            "LifeLeechDuration",
            "LifeLeechInstances",
            "LifeLeechInstant",
            "LifeLeechInstantRate",
            "LifeLeechInstantProportion",
            "EnergyShieldLeechDuration",
            "EnergyShieldLeechInstances",
            "EnergyShieldLeechInstant",
            "EnergyShieldLeechInstantRate",
            "EnergyShieldLeechInstantProportion",
            "ManaLeechDuration",
            "ManaLeechInstances",
            "ManaLeechInstant",
            "ManaLeechInstantRate",
            "ManaLeechInstantProportion",
            "LifeOnHit",
            "LifeOnHitRate",
            "LifeOnKill",
            "EnergyShieldOnHit",
            "EnergyShieldOnHitRate",
            "EnergyShieldOnKill",
            "ManaOnHit",
            "ManaOnHitRate",
            "ManaOnKill",
            "impaleStoredHitAvg",
        ] {
            combine_stat(off, stat, CombineMode::Dps, None);
        }
        // :3991-4042 breakdown only
    }
    if off.env.mode == "CALCS" {
        off.note(format!(
            "{LUA}:4044-4050 output.DisplayDamage (formatNumSep: main.showThousandsSeparators / decimalSeparator, CALCS mode; R8)"
        ));
    }
    // :4096-4098
    if off.sk().minion.is_some() {
        match off.gval("SummonedMinionsPerCast") {
            Some(OutValue::Num(n)) => {
                let n = *n;
                let dps_mult = off.sd_num("dpsMultiplier", 4097).unwrap_or_else(|| {
                    panic!("{LUA}:4097: attempt to perform arithmetic on field 'dpsMultiplier' (a nil value)")
                });
                let v = n * off.g_hit_speed(4097) * dps_mult;
                off.set_sd("summonSpeed", Value::Num(v));
            }
            _ => off.note(format!(
                "{LUA}:4097 skillData.summonSpeed: output.SummonedMinionsPerCast is not computed (:1057-1065, activeSkill.minion.minionData, R7.10)"
            )),
        }
    }

    // :4100-4141 leech rates
    let sc = Some(off.cfg.clone());
    let sc = sc.as_ref();
    let base = misc("LeechRateBase");
    let g = |off: &Off<'_>, k: &str, line: u32| num_at(off.gval(k), k, line);
    let life_ir = g(off, "Life", 4101) * base * q.calc_mod(sc, &["LifeLeechRate"]);
    off.gset("LifeLeechInstanceRate", life_ir);
    off.gset("LifeLeechRate", g(off, "LifeLeechInstances", 4102) * life_ir);
    off.gset("LifeLeechPerHit", life_ir);
    let es_ir = g(off, "EnergyShield", 4104) * base * q.calc_mod(sc, &["EnergyShieldLeechRate"]);
    off.gset("EnergyShieldLeechInstanceRate", es_ir);
    off.gset("EnergyShieldLeechRate", g(off, "EnergyShieldLeechInstances", 4105) * es_ir);
    off.gset("EnergyShieldLeechPerHit", es_ir);
    let mana_ir = g(off, "Mana", 4107) * base * q.calc_mod(sc, &["ManaLeechRate"]);
    off.gset("ManaLeechInstanceRate", mana_ir);
    off.gset("ManaLeechRate", g(off, "ManaLeechInstances", 4108) * mana_ir);
    off.gset("ManaLeechPerHit", mana_ir);
    if q.flag(None, &["ImmortalAmbition"]) {
        let v = g(off, "EnergyShieldLeechRate", 4112) + g(off, "LifeLeechRate", 4112);
        off.gset("EnergyShieldLeechRate", v);
        let v = g(off, "EnergyShieldLeechPerHit", 4113) + g(off, "LifeLeechPerHit", 4113);
        off.gset("EnergyShieldLeechPerHit", v);
        off.gset("LifeLeechRate", 0.0);
        off.gset("LifeLeechPerHit", 0.0);
    }
    if q.flag(None, &["UnaffectedByNonInstantLifeLeech"]) {
        off.gset("LifeLeechRate", 0.0);
        off.gset("LifeLeechPerHit", 0.0);
        off.gset("LifeLeechInstances", 0.0);
    }
    for (pool, line) in [("Life", 4124), ("EnergyShield", 4126), ("Mana", 4128)] {
        let rate = g(off, &format!("{pool}LeechInstantRate"), line)
            + min2(
                g(off, &format!("{pool}LeechRate"), line),
                g(off, &format!("Max{pool}LeechRate"), line),
            ) * g(off, &format!("{pool}RecoveryRateMod"), line);
        off.gset(&format!("{pool}LeechRate"), rate);
        let per_hit = g(off, &format!("{pool}LeechInstant"), line + 1)
            + min2(
                g(off, &format!("{pool}LeechPerHit"), line + 1),
                g(off, &format!("Max{pool}LeechRate"), line + 1),
            ) * g(off, &format!("{pool}LeechDuration"), line + 1)
                * g(off, &format!("{pool}RecoveryRateMod"), line + 1);
        off.gset(&format!("{pool}LeechPerHit"), per_hit);
    }
    let l = g(off, "LifeLeechRate", 4130) > 0.0;
    off.set_flag("leechLife", l);
    let e = g(off, "EnergyShieldLeechRate", 4131) > 0.0;
    off.set_flag("leechES", e);
    let m = g(off, "ManaLeechRate", 4132) > 0.0;
    off.set_flag("leechMana", m);
    if off.sd_truthy("showAverage") {
        for pool in ["Life", "EnergyShield", "Mana"] {
            let v = g(off, &format!("{pool}LeechPerHit"), 4134) + g(off, &format!("{pool}OnHit"), 4134);
            off.gset(&format!("{pool}LeechGainPerHit"), v);
        }
    } else {
        for pool in ["Life", "EnergyShield", "Mana"] {
            let v = g(off, &format!("{pool}LeechRate"), 4138) + g(off, &format!("{pool}OnHitRate"), 4138);
            off.gset(&format!("{pool}LeechGainRate"), v);
        }
    }
    // :4142-4153 breakdown only
}
