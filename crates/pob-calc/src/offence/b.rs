//! CalcOffence.lua:1937-2605 (R7.4, part B): the conversion tables
//! (`buildConversionTable`, :1937-1996), the damage passes (`passList`, :1997-2065), `combineStat` (:2066-2173), how often the skill hits
//! (accuracy, enemy block, attack/cast speed, trauma, hit time -- the first `for _, pass in ipairs(passList)`, :2174-2488), the misc DPS
//! multipliers (:2489-2598) and the quantity multiplier (:2599-2605). OC3 (:2606) is taken right after.
//!
//! Hash-order walks (R13/D13) in this section:
//! * `buildConversionTable`: `pairs(skillConv)` / `pairs(globalConv)` scale each key on its own (order-free); the final
//!   `for type in pairs(globalConv)` sums `round(...)` -- whole numbers, exact in any order. The tables are built in `dmgTypeList` order.
//! * nothing else here walks a hash table.
//!
//! Breakdown (R8): every `if breakdown` branch of this section only fills breakdown tables (`breakdown.Accuracy`, `.HitChance`,
//! `.Speed`, `.Time`, `.HitTime`, `.HitSpeed`, `.SustainableTrauma`, the `combineStat` rows) -- not ported; the one query such a
//! branch makes (`calcLib.mod(skillModList, skillCfg, "SkillAttackTime")`, :2459) is still made so a malformed mod raises as upstream.

use indexmap::IndexMap;
use pob_lua::number::{max2, min2, round};
use pob_mods::{ModCfg, ModType, Str, Value};

use super::helpers::{ConvEntry, ConversionTable, DMG_TYPE_LIST, Q};
use super::{CombineMode, HandOut, Off, Pass, PassOut, Source};
use crate::output::OutValue;
use crate::setup::{misc, ok};

const LUA: &str = "Modules/CalcOffence.lua";

/// `isElemental` (:31).
fn is_elemental(t: &str) -> bool {
    matches!(t, "Fire" | "Cold" | "Lightning")
}

/// :1938-1987 `buildConversionTable(cfg)`.
pub(super) fn build_conversion_table(q: &Q, cfg: &ModCfg) -> ConversionTable {
    let c = Some(cfg);
    let mut table = ConversionTable::new();
    for dti in 0..4 {
        let dt = DMG_TYPE_LIST[dti];
        let others = &DMG_TYPE_LIST[dti + 1..];
        let mut global_conv: Vec<f64> = Vec::with_capacity(others.len());
        let mut skill_conv: Vec<f64> = Vec::with_capacity(others.len());
        let mut add: Vec<f64> = Vec::with_capacity(others.len());
        let (mut global_total, mut skill_total) = (0.0, 0.0);
        for &other in others {
            // nil names in the vararg match nothing (ModStore Sum walks `select('#', ...)`)
            let n1 = format!("{dt}DamageConvertTo{other}");
            let n2 = format!("ElementalDamageConvertTo{other}");
            let n3 = format!("NonChaosDamageConvertTo{other}");
            let mut names: Vec<&str> = vec![&n1];
            if is_elemental(dt) {
                names.push(&n2);
            }
            if dt != "Chaos" {
                names.push(&n3);
            }
            let g = max2(q.base(c, &names), 0.0);
            global_conv.push(g);
            global_total += g;
            let s = max2(q.base(c, &[&format!("Skill{dt}DamageConvertTo{other}")]), 0.0);
            skill_conv.push(s);
            skill_total += s;
            let a1 = format!("{dt}DamageGainAs{other}");
            let a2 = format!("ElementalDamageGainAs{other}");
            let a3 = format!("NonChaosDamageGainAs{other}");
            let mut names: Vec<&str> = vec![&a1];
            if is_elemental(dt) {
                names.push(&a2);
            }
            if dt != "Chaos" {
                names.push(&a3);
            }
            add.push(max2(q.base(c, &names), 0.0));
        }
        if skill_total > 100.0 {
            // :1958-1966 each key on its own (order-free)
            let factor = 100.0 / skill_total;
            for v in &mut skill_conv {
                *v *= factor;
            }
            global_conv.fill(0.0);
        } else if global_total + skill_total > 100.0 {
            let factor = (100.0 - skill_total) / global_total;
            for v in &mut global_conv {
                *v *= factor;
            }
        }
        let mut e = ConvEntry::default();
        let mut converted_total = 0.0;
        for (k, &other) in others.iter().enumerate() {
            // :1975 whole percents: the sum below is exact in any `pairs` order
            let conversion = round(global_conv[k] + skill_conv[k], None);
            e.conversion.insert(other, conversion / 100.0);
            e.gain.insert(other, add[k] / 100.0);
            e.total.insert(other, (conversion + add[k]) / 100.0);
            converted_total += conversion;
        }
        e.mult = 1.0 - min2(converted_total / 100.0, 1.0);
        table.insert(dt, e);
    }
    table.insert(
        "Chaos",
        ConvEntry {
            mult: 1.0,
            ..ConvEntry::default()
        },
    );
    table
}

/// :1937-2065 -- the conversion tables and the pass list (`canDeal`, :1932-1935, runs before OC2: [`super::a2`]).
pub(super) fn build_passes(off: &mut Off<'_>) {
    let q = off.q.clone();
    let cfg = off.cfg.clone();
    // :1989-1995
    off.conversion_table = build_conversion_table(&q, &cfg);
    for n in [1u8, 2] {
        if let Some(wc) = off.weapon_cfg(n) {
            off.weapon_conversion[usize::from(n - 1)] = Some(build_conversion_table(&q, &wc));
        }
    }
    // :1998-2065
    if off.is_attack {
        off.hands = Some([HandOut::default(), HandOut::default()]);
        match off.env.keystones_added.get("Precise Technique").copied() {
            Some(b) => off.out().set_bool("PreciseTechnique", b),
            None => off.out().remove("PreciseTechnique"),
        }
        let crit_override = q.override_(Some(&cfg), &["WeaponBaseCritChance"]).filter(Value::truthy);
        for n in [1u8, 2] {
            if !off.flag(if n == 1 { "weapon1Attack" } else { "weapon2Attack" }) {
                continue;
            }
            // `activeSkill.weapon<n>Cfg.skillStats = output.<Hand>` -- the pass cfg carries the hand's numbers (ModStore.lua:355)
            let mut wcfg = off.weapon_cfg(n).unwrap_or_else(|| {
                panic!(
                    "{LUA}:{}: attempt to index field 'weapon{n}Cfg' (a nil value)",
                    if n == 1 { 2008 } else { 2025 }
                )
            });
            wcfg.skill_stats = Some(IndexMap::new());
            let mut source = {
                let p = off.env.player.borrow();
                if n == 1 { p.weapon_data1.clone() } else { p.weapon_data2.clone() }
            };
            if let Some(co) = &crit_override
                && source.get("type").truthy()
                && source.get("type").as_str() != Some("None")
            {
                source.set("CritChance", co.clone());
            }
            if n == 2 {
                if off.sd_truthy("CritChance") {
                    source.set("CritChance", off.sd("CritChance"));
                }
                for ty in ["Physical", "Fire", "Cold"] {
                    let (kmin, kmax) = (format!("setOffHand{ty}Min"), format!("setOffHand{ty}Max"));
                    if off.sd_truthy(&kmin) && off.sd_truthy(&kmax) {
                        source.set(&format!("{ty}Min"), off.sd(&kmin));
                        source.set(&format!("{ty}Max"), off.sd(&kmax));
                    }
                }
                if let Some(t) = off.sd_num("attackTime", 2042) {
                    source.set("AttackRate", Value::Num(1000.0 / t));
                }
            }
            off.passes.push(Pass {
                label: if n == 1 { "Main Hand" } else { "Off Hand" },
                source: Source::Weapon(source),
                cfg: wcfg,
                out: PassOut::Hand(usize::from(n - 1)),
            });
        }
    } else {
        off.passes.push(Pass {
            label: "Skill",
            source: Source::SkillData,
            cfg,
            out: PassOut::Global,
        });
    }
}

/// :2066-2171 `combineStat(stat, mode, ...)` (breakdown rows excluded, R8). `chance_key` is the `...` of `CHANCE`/`CHANCE_AILMENT`.
pub(crate) fn combine_stat(off: &mut Off<'_>, stat: &str, mode: CombineMode, chance_key: Option<&str>) {
    let both = off.flag("bothWeaponAttack");
    let hands = off
        .hands
        .as_ref()
        .unwrap_or_else(|| panic!("{LUA}:2068: attempt to index field 'MainHand' (a nil value)"));
    let mh = hands[0].map.get(stat).cloned();
    let oh = hands[1].map.get(stat).cloned();
    let num = |v: &Option<OutValue>, line: u32| -> f64 {
        match v {
            Some(OutValue::Num(x)) => *x,
            other => panic!("{LUA}:{line}: attempt to perform arithmetic on field '{stat}' ({other:?})"),
        }
    };
    let or0 = |v: &Option<OutValue>, line: u32| -> f64 {
        match v {
            None | Some(OutValue::Bool(false)) => 0.0,
            Some(_) => num(v, line),
        }
    };
    let truthy = |v: &Option<OutValue>| !matches!(v, None | Some(OutValue::Bool(false)));
    // `output.MainHand[stat] or output.OffHand[stat]`
    let either = if truthy(&mh) { mh.clone() } else { oh.clone() };
    let hand_num = |h: usize, k: &str, line: u32| -> f64 {
        match hands[h].map.get(k) {
            Some(OutValue::Num(x)) => *x,
            other => panic!("{LUA}:{line}: attempt to perform arithmetic on field '{k}' ({other:?})"),
        }
    };
    let result: Option<OutValue> = if mode == CombineMode::Or || !both {
        either
    } else {
        match mode {
            CombineMode::Or => unreachable!(),
            CombineMode::Add => Some(OutValue::Num(or0(&mh, 2072) + or0(&oh, 2072))),
            CombineMode::Average => Some(OutValue::Num((or0(&mh, 2074) + or0(&oh, 2074)) / 2.0)),
            CombineMode::HarmonicMean => {
                let (a, b) = (mh.clone(), oh.clone());
                if a == Some(OutValue::Num(0.0)) || b == Some(OutValue::Num(0.0)) {
                    Some(OutValue::Num(0.0))
                } else {
                    Some(OutValue::Num(2.0 / ((1.0 / num(&a, 2079)) + (1.0 / num(&b, 2079)))))
                }
            }
            CombineMode::Chance => {
                if truthy(&mh) && truthy(&oh) {
                    let k = chance_key.unwrap_or_else(|| panic!("{LUA}:2083: attempt to index a nil key"));
                    let mut main_chance = hand_num(0, k, 2083) * hand_num(0, "HitChance", 2083);
                    let mut off_chance = hand_num(1, k, 2084) * hand_num(1, "HitChance", 2084);
                    if off.sd_truthy("doubleHitsWhenDualWielding") {
                        main_chance /= 10000.0;
                        off_chance /= 10000.0;
                        Some(OutValue::Num(num(&mh, 2088) * main_chance + num(&oh, 2088) * off_chance))
                    } else {
                        let main_portion = main_chance / (main_chance + off_chance);
                        let off_portion = off_chance / (main_chance + off_chance);
                        Some(OutValue::Num(num(&mh, 2108) * main_portion + num(&oh, 2108) * off_portion))
                    }
                } else {
                    either
                }
            }
            CombineMode::ChanceAilment => {
                if truthy(&mh) && truthy(&oh) {
                    let k = chance_key.unwrap_or_else(|| panic!("{LUA}:2129: attempt to index a nil key"));
                    // the portions are computed upstream (:2131-2132) but feed only the breakdown
                    let _ = hand_num(0, k, 2129) * hand_num(0, "HitChance", 2129);
                    let _ = hand_num(1, k, 2130) * hand_num(1, "HitChance", 2130);
                    let (a, b) = (num(&mh, 2133), num(&oh, 2133));
                    let max_instance = max2(a, b);
                    let min_instance = min2(a, b);
                    // `stat:gsub("DPS","")` -- a plain pattern
                    let stack_name = format!("{}Stacks", stat.replace("DPS", ""));
                    let g = |k: &str| match off.st.player.output.get(k) {
                        None | Some(OutValue::Bool(false)) => 1.0,
                        Some(OutValue::Num(x)) => *x,
                        other => panic!("{LUA}:2137: attempt to perform arithmetic on field '{k}' ({other:?})"),
                    };
                    let max_instance_stacks = min2(1.0, g(&stack_name) / g(&format!("{stack_name}Max")));
                    Some(OutValue::Num(
                        max_instance * max_instance_stacks + min_instance * (1.0 - max_instance_stacks),
                    ))
                } else {
                    either
                }
            }
            CombineMode::Dps => {
                let mut v = or0(&mh, 2166) + or0(&oh, 2166);
                if !off.sd_truthy("doubleHitsWhenDualWielding") {
                    v /= 2.0;
                }
                Some(OutValue::Num(v))
            }
        }
    };
    let o = off.out();
    match result {
        None => o.remove(stat),
        Some(OutValue::Num(x)) => o.set_num(stat, x),
        Some(OutValue::Bool(b)) => o.set_bool(stat, b),
        Some(OutValue::Str(s)) => o.set_str(stat, &s),
    }
}

/// `activeSkill.activeEffect.grantedEffect.castTime`.
fn cast_time(off: &Off<'_>) -> Option<f64> {
    let st = off.env.skills.as_ref().expect("skills on");
    st.skills.effects[off.sk().active_effect.borrow().granted_effect].cast_time
}

/// `globalOutput[k]` in arithmetic.
fn gnum(off: &Off<'_>, k: &str, line: u32) -> f64 {
    off.out_num(k, line)
}

/// :2174-2488 -- the first walk of the pass list: accuracy, hit chance, enemy block, attack/cast speed, trauma, hit time.
#[allow(clippy::too_many_lines)]
pub(super) fn speed_accuracy(off: &mut Off<'_>) {
    let q = off.q.clone();
    let skill_cfg = off.cfg.clone();
    let sc = Some(&skill_cfg);
    let tick = misc("ServerTickRate");
    let (mut stored_mh_accuracy, mut stored_mh_accuracy_vs_enemy): (Option<f64>, Option<f64>) = (None, None);
    for pi in 0..off.passes.len() {
        let label = off.passes[pi].label;
        // :2183-2187
        if let Some(v) = off.sd_num("averageBurstHits", 2183) {
            off.pset_num(pi, "AverageBurstHits", v);
        } else if let Some(r) = off.pnum(pi, "Repeats")
            && r > 1.0
        {
            off.pset_num(pi, "AverageBurstHits", r);
        }

        // :2189-2229 hit chance
        let (base, base_vs, inc, inc_vs, more, more_vs) = {
            let c = Some(off.pcfg(pi));
            (
                q.base(c, &["Accuracy"]),
                q.base(c, &["Accuracy", "AccuracyVsEnemy"]),
                q.inc(c, &["Accuracy"]),
                q.inc(c, &["Accuracy", "AccuracyVsEnemy"]),
                q.more(c, &["Accuracy"]),
                q.more(c, &["Accuracy", "AccuracyVsEnemy"]),
            )
        };
        off.pset_num(pi, "Accuracy", max2(0.0, (base * (1.0 + inc / 100.0) * more).floor()));
        let mut accuracy_vs_enemy = max2(0.0, (base_vs * (1.0 + inc_vs / 100.0) * more_vs).floor());
        let offhand_is_main = q.flag(None, &["Condition:OffHandAccuracyIsMainHandAccuracy"]);
        if offhand_is_main && label == "Main Hand" {
            stored_mh_accuracy = off.pnum(pi, "Accuracy");
            stored_mh_accuracy_vs_enemy = Some(accuracy_vs_enemy);
        } else if q.flag(None, &["Condition:OffHandAccuracyIsMainHandAccuracy"])
            && label == "Off Hand"
            && let Some(a) = stored_mh_accuracy
        {
            off.pset_num(pi, "Accuracy", a);
            accuracy_vs_enemy = stored_mh_accuracy_vs_enemy.expect("stored with the accuracy");
        }
        let cannot_be_evaded = !off.is_attack
            || q.flag(Some(off.pcfg(pi)), &["CannotBeEvaded"])
            || off.sd_truthy("cannotBeEvaded")
            || (off.env.mode_effective && ok(off.enemy_db.borrow().flag(None, &["CannotEvade"])) == Some(true));
        if cannot_be_evaded {
            off.pset_num(pi, "AccuracyHitChance", 100.0);
        } else {
            let ev = ok(crate::calc_lib::val(&off.enemy_db.borrow(), "Evasion", None));
            let enemy_evasion = max2(round(ev, None), 0.0);
            let m = q.calc_mod(Some(off.pcfg(pi)), &["HitChance"]);
            off.pset_num(
                pi,
                "AccuracyHitChance",
                crate::defence::hit_chance(enemy_evasion, accuracy_vs_enemy) * m,
            );
        }
        // :2243-2247 enemy block chance
        let enemy_block = ok(off.enemy_db.borrow().sum(ModType::Base, Some(off.pcfg(pi)), &["BlockChance"]));
        let reduce = q.base(Some(off.pcfg(pi)), &["reduceEnemyBlock"]);
        off.pset_num(pi, "enemyBlockChance", max2(min2(enemy_block, 100.0) - reduce, 0.0));
        if ok(off.enemy_db.borrow().flag(None, &["CannotBlockAttacks"])) == Some(true) && off.is_attack {
            off.pset_num(pi, "enemyBlockChance", 0.0);
        }
        let ahc = off.pnum_req(pi, "AccuracyHitChance", 2249);
        let ebc = off.pnum_req(pi, "enemyBlockChance", 2249);
        off.pset_num(pi, "HitChance", ahc * (1.0 - ebc / 100.0));
        if ebc > 0.0 && !off.is_attack {
            off.out().set_bool("enemyHasSpellBlock", true);
        }

        // :2268-2270 Precise Technique condition, per pass
        let cond_name = format!("{}AccRatingHigherThanMaxLife", label.replace(' ', ""));
        let acc = match off.pval(pi, "Accuracy") {
            None | Some(OutValue::Bool(false)) => 0.0,
            Some(OutValue::Num(x)) => *x,
            other => panic!("{LUA}:2270: attempt to compare {other:?} with number"),
        };
        let life = off
            .st
            .player
            .output
            .opt_num("Life")
            .unwrap_or_else(|| panic!("{LUA}:2270: attempt to compare number with nil"));
        q.list.borrow_mut().conditions.insert(Str::from(cond_name.as_str()), acc > life);

        // :2272-2471 attack/cast speed
        let ct = cast_time(off);
        if ct == Some(0.0) && !off.sd_truthy("castTimeOverride") && !off.sd_truthy("triggered") {
            off.pset_num(pi, "Time", 0.0);
            off.pset_num(pi, "Speed", 0.0);
        } else if let Some(t) = off.sd_num("timeOverride", 2276) {
            off.pset_num(pi, "Time", t);
            off.pset_num(pi, "Speed", 1.0 / t);
        } else if off.sd_truthy("fixedCastTime") {
            let t = ct.unwrap_or_else(|| panic!("{LUA}:2281: attempt to perform arithmetic on field 'Time' (a nil value)"));
            off.pset_num(pi, "Time", t);
            off.pset_num(pi, "Speed", 1.0 / t);
        } else if off.sd_truthy("triggerTime") && off.sd_truthy("triggered") {
            let trigger_time = off.sd_num("triggerTime", 2287).expect("truthy");
            let c = Some(off.pcfg(pi));
            let linked = q.base(c, &["ActiveSkillsLinkedToTrigger"]);
            let t = if linked > 0.0 {
                trigger_time / (1.0 + q.inc(c, &["CooldownRecovery"]) / 100.0) * linked
            } else {
                trigger_time / (1.0 + q.inc(c, &["CooldownRecovery"]) / 100.0)
            };
            off.pset_num(pi, "Time", t);
            off.pset_num(pi, "TriggerTime", t);
            off.pset_num(pi, "Speed", 1.0 / t);
        } else if off.sd_truthy("triggerRate") && off.sd_truthy("triggered") {
            let rate = off.sd_num("triggerRate", 2295).expect("truthy");
            off.pset_num(pi, "Time", 1.0 / rate);
            off.pset_num(pi, "TriggerTime", 1.0 / rate);
            off.pset_num(pi, "Speed", rate);
            off.set_sd("showAverage", Value::Bool(false));
        } else {
            let base_time = if off.is_attack {
                if let Some(m) = off.sd_num("attackSpeedMultiplier", 2303)
                    && let Some(r) = off.source_num(pi, "AttackRate", 2303)
                {
                    off.source_set(pi, "AttackRate", Value::Num(r * (1.0 + m / 100.0)));
                }
                if off.sd_truthy("castTimeOverridesAttackTime") {
                    let ct = ct.unwrap_or_else(|| panic!("{LUA}:2308: attempt to perform arithmetic on field 'castTime' (a nil value)"));
                    ct / (1.0 + off.source_num(pi, "AttackSpeedInc", 2308).unwrap_or(0.0) / 100.0)
                } else if q.calc_mod(sc, &["SkillAttackTime"]) > 0.0 {
                    let rate = off.source_num(pi, "AttackRate", 2310).unwrap_or(1.0);
                    (1.0 / rate + q.base(Some(off.pcfg(pi)), &["Speed"])) * q.calc_mod(sc, &["SkillAttackTime"])
                } else {
                    let rate = off.source_num(pi, "AttackRate", 2312).unwrap_or(1.0);
                    1.0 / rate + q.base(Some(off.pcfg(pi)), &["Speed"])
                }
            } else {
                // `skillData.castTimeOverride or grantedEffect.castTime or 1`
                off.sd_num("castTimeOverride", 2315).or(ct).unwrap_or(1.0)
            };
            let more = q.more(Some(off.pcfg(pi)), &["Speed"]);
            let repeats = off.st.player.output.opt_num("Repeats").unwrap_or(1.0);
            off.pset_num(pi, "Repeats", repeats);

            // :2320-2405 the max number of trauma stacks you can sustain
            if q.flag(None, &["HasTrauma"]) {
                let c = Some(off.pcfg(pi));
                let effective_attack_rate_cap = tick * repeats;
                let duration = q.base(c, &["TraumaDuration"]) * q.calc_mod(sc, &["Duration"]);
                let trauma_per_attack = 1.0 + min2(q.base(c, &["ExtraTrauma"]), 100.0) / 100.0;
                let inc_per_trauma = q.inc(sc, &["SpeedPerTrauma"]);
                let config_trauma = q.base(sc, &["Multiplier:TraumaStacks"]);
                let inc = q.inc(c, &["Speed"]) - inc_per_trauma * config_trauma;
                let attack_speed_before_inc = 1.0 / base_time * gnum(off, "ActionSpeedMod", 2325) * more;
                let inc_per_trauma_cap =
                    (effective_attack_rate_cap - attack_speed_before_inc * (1.0 + inc / 100.0)) / attack_speed_before_inc * 100.0;
                let hit_chance = off.pnum_req(pi, "HitChance", 2327);
                let trauma_rate_before_inc = trauma_per_attack * (hit_chance / 100.0) * attack_speed_before_inc / repeats;
                let mut trauma =
                    trauma_rate_before_inc * (1.0 + inc / 100.0) / (1.0 / duration - trauma_rate_before_inc * inc_per_trauma / 100.0);
                if trauma < 0.0 || inc_per_trauma * trauma > inc_per_trauma_cap {
                    trauma = trauma_per_attack * (hit_chance / 100.0) * effective_attack_rate_cap / repeats * duration;
                }
                if off.flag("bothWeaponAttack") {
                    trauma /= 2.0;
                }
                q.nm(
                    "Multiplier:SustainableTraumaStacks",
                    ModType::Base,
                    trauma,
                    "Maximum Sustainable Trauma Stacks",
                    0,
                    0,
                    vec![],
                );
            }
            if q.base(sc, &["Multiplier:TraumaStacks"]) == 0.0 {
                let v = q.base(sc, &["Multiplier:SustainableTraumaStacks"]);
                q.nm(
                    "Multiplier:TraumaStacks",
                    ModType::Base,
                    v,
                    "Maximum Sustainable Trauma Stacks",
                    0,
                    0,
                    vec![],
                );
            }
            let inc = q.inc(Some(off.pcfg(pi)), &["Speed"]);

            if off.flag("warcry") {
                let w = off.pnum_req(pi, "WarcryCastTime", 2413);
                off.pset_num(pi, "Speed", 1.0 / w);
            } else {
                let c = Some(off.pcfg(pi));
                let s = 1.0
                    / (base_time / round((1.0 + inc / 100.0) * more, Some(2.0))
                        + q.base(c, &["TotalAttackTime"])
                        + q.base(c, &["TotalCastTime"]));
                off.pset_num(pi, "Speed", s);
            }
            let s = off.pnum_req(pi, "Speed", 2418);
            off.pset_num(pi, "CastRate", s);
            if off.flag("selfCast") {
                let s = off.pnum_req(pi, "Speed", 2421) * gnum(off, "ActionSpeedMod", 2421);
                off.pset_num(pi, "Speed", s);
                off.pset_num(pi, "CastRate", s);
            }
            if off.flag("totem") {
                let totem_action_speed = 1.0 + ok(off.mod_db.borrow().sum(ModType::Inc, None, &["TotemActionSpeed"])) / 100.0;
                off.pset_num(pi, "TotemActionSpeed", totem_action_speed);
                let s = off.pnum_req(pi, "Speed", 2429) * totem_action_speed;
                off.pset_num(pi, "Speed", s);
                off.pset_num(pi, "CastRate", s);
                if off.sd_truthy("totemFireOnce") {
                    let ht = 1.0 / s + gnum(off, "TotemPlacementTime", 2432);
                    off.pset_num(pi, "HitTime", ht);
                    off.pset_num(pi, "HitSpeed", 1.0 / ht);
                }
            }
            if let Some(cd) = off.st.player.output.get("Cooldown").cloned()
                && !matches!(cd, OutValue::Bool(false))
            {
                off.pset(pi, "Cooldown", cd);
                let cd = off.pnum_req(pi, "Cooldown", 2438);
                let s = min2(off.pnum_req(pi, "Speed", 2438), 1.0 / cd * repeats);
                off.pset_num(pi, "Speed", s);
            }
            if off.ptruthy(pi, "Cooldown") && off.flag("selfCast") {
                off.set_flag("notAverage", true);
                off.set_flag("showAverage", false);
                off.set_sd("showAverage", Value::Bool(false));
            }
            if !off.has_type("Channel") {
                let s = min2(off.pnum_req(pi, "Speed", 2446), tick * repeats);
                off.pset_num(pi, "Speed", s);
            }
            let s = off.pnum_req(pi, "Speed", 2448);
            off.pset_num(pi, "Time", if s == 0.0 { 0.0 } else { 1.0 / s });
            if off.breakdown {
                // :2462 the condition of a breakdown-only branch (R8)
                let _ = q.calc_mod(sc, &["SkillAttackTime"]);
            }
        }
        // :2473-2487 hit time
        let triggered_on_death = off.sd_truthy("triggeredOnDeath");
        if off.sd_truthy("hitTimeOverride") && !triggered_on_death {
            let ht = off.sd_num("hitTimeOverride", 2474).expect("truthy");
            off.pset_num(pi, "HitTime", ht);
            off.pset_num(pi, "HitSpeed", 1.0 / ht);
            if off.has_type("Brand") && !q.flag(None, &["UnlimitedBrandDuration"]) {
                let d = off.pnum_req(pi, "Duration", 2478);
                off.pset_num(pi, "BrandTicks", (d * (1.0 / ht)).floor());
            }
        } else if off.sd_truthy("hitTimeMultiplier") && off.ptruthy(pi, "Time") && !triggered_on_death {
            let m = off.sd_num("hitTimeMultiplier", 2481).expect("truthy");
            let ht = off.pnum_req(pi, "Time", 2481) * m;
            off.pset_num(pi, "HitTime", ht);
            let hs = if off.ptruthy(pi, "Cooldown") && off.sd_truthy("triggered") {
                1.0 / max2(ht, off.pnum_req(pi, "Cooldown", 2483))
            } else if off.ptruthy(pi, "Cooldown") {
                1.0 / (ht + off.pnum_req(pi, "Cooldown", 2485))
            } else {
                1.0 / ht
            };
            off.pset_num(pi, "HitSpeed", hs);
        }
    }
}

/// :2489-2605 -- misc DPS multipliers, combined hit chance / speed of the two hands, quantity multiplier (returned: the local
/// `quantityMultiplier` the damage pass reads, :3801).
#[allow(clippy::too_many_lines)]
pub(super) fn misc_dps(off: &mut Off<'_>) -> f64 {
    let q = off.q.clone();
    let skill_cfg = off.cfg.clone();
    let sc = Some(&skill_cfg);
    // :2490
    let mut dps_mult = off.sd_num("dpsMultiplier", 2490).unwrap_or(1.0) * (1.0 + q.inc(sc, &["DPS"]) / 100.0) * q.more(sc, &["DPS"]);
    off.set_sd("dpsMultiplier", Value::Num(dps_mult));
    if off.has_type("Brand") && !off.sd_truthy("countsAttachedBrandsInDamage") {
        let n = off.out_num("AttachedBrandCount", 2492);
        dps_mult *= n;
        off.set_sd("dpsMultiplier", Value::Num(dps_mult));
        let m = off.st.player.output.opt_num("SkillDPSMultiplier").unwrap_or(1.0) * n;
        off.out().set_num("SkillDPSMultiplier", m);
    }
    let final_repeat = matches!(off.env.config.input.get("repeatMode"), Some(pob_build::ConfigValue::Str(s)) if s == "FINAL");
    if final_repeat || q.flag(None, &["OnlyFinalRepeat"]) {
        let r = match off.st.player.output.get("Repeats") {
            None | Some(OutValue::Bool(false)) => 1.0,
            _ => off.out_num("Repeats", 2496),
        };
        dps_mult /= r;
        off.set_sd("dpsMultiplier", Value::Num(dps_mult));
    }
    if q.flag(None, &["TriggeredBySnipe"]) {
        off.set_flag("channelRelease", true);
    }
    // :2504 `Flag(...) and Sum(...)` -- `Flag` answers nil when no mod matched (the key is then absent), false for a false flag
    crate::perform::skill_guard(&q.malformed, &["HasTrauma"]);
    match ok(q.list.borrow().flag(None, &["HasTrauma"])) {
        Some(true) => {
            let v = q.base(sc, &["Multiplier:SustainableTraumaStacks"]);
            off.out().set_num("SustainableTrauma", v);
        }
        Some(false) => off.out().set_bool("SustainableTrauma", false),
        None => off.out().remove("SustainableTrauma"),
    }
    // :2506-2507 Mantra of Flames buff count -- `cfg` here is the global `cfg` (nil): the loop's `local cfg` is out of scope
    {
        let a = q.base(None, &["Multiplier:TraumaStacks"]);
        let mut db = off.mod_db.borrow_mut();
        let v = db.multipliers.get("BuffOnSelf").copied().unwrap_or(0.0) + a;
        db.multipliers.insert(Str::from("BuffOnSelf"), v);
    }
    {
        let a = q.base(None, &["Multiplier:VoltaxicWaitingStages"]);
        let mut db = off.mod_db.borrow_mut();
        let v = db.multipliers.get("BuffOnSelf").copied().unwrap_or(0.0) + a;
        db.multipliers.insert(Str::from("BuffOnSelf"), v);
    }
    if off.is_attack {
        combine_stat(off, "AccuracyHitChance", CombineMode::Average, None);
        combine_stat(off, "HitChance", CombineMode::Average, None);
        combine_stat(off, "Speed", CombineMode::HarmonicMean, None);
        combine_stat(off, "HitSpeed", CombineMode::Or, None);
        combine_stat(off, "HitTime", CombineMode::Or, None);
        let s = off.out_num("Speed", 2515);
        off.out().set_num("Time", if s == 0.0 { 0.0 } else { 1.0 / s });
        if off.out_num("Time", 2521) > 1.0 {
            off.mod_db.borrow_mut().new_mod(pob_mods::create_mod(
                "Condition:OneSecondAttackTime",
                Some(ModType::Flag),
                Value::Bool(true),
                None,
                0,
                0,
                vec![],
            ));
        }
        if q.flag(None, &["UseOffhandAttackSpeed"]) && !off.flag("forceMainHand") {
            let hands = off.hands.as_ref().expect("attack");
            let (s, t) = (hands[1].map.get("Speed").cloned(), hands[1].map.get("Time").cloned());
            off.set_out_value("Speed", s);
            off.set_out_value("Time", t);
        }
        let triggered_on_death = off.sd_truthy("triggeredOnDeath");
        if off.sd_truthy("hitTimeOverride") && !triggered_on_death {
            let ht = off.sd_num("hitTimeOverride", 2556).expect("truthy");
            off.out().set_num("HitTime", ht);
            off.out().set_num("HitSpeed", 1.0 / ht);
        } else if off.sd_truthy("hitTimeMultiplier")
            && !matches!(off.st.player.output.get("Time"), None | Some(OutValue::Bool(false)))
            && !triggered_on_death
        {
            let m = off.sd_num("hitTimeMultiplier", 2559).expect("truthy");
            let ht = off.out_num("Time", 2559) * m;
            off.out().set_num("HitTime", ht);
            let cd = match off.st.player.output.get("Cooldown") {
                None | Some(OutValue::Bool(false)) => None,
                _ => Some(off.out_num("Cooldown", 2560)),
            };
            let hs = match cd {
                Some(cd) if off.sd_truthy("triggered") => 1.0 / max2(ht, cd),
                Some(cd) => 1.0 / (ht + cd),
                None => min2(1.0 / ht, misc("ServerTickRate")),
            };
            off.out().set_num("HitSpeed", hs);
        }
    }
    // :2600-2604 (`activeSkill.skillModList` / `activeSkill.skillCfg` -- the same tables as the locals)
    let quantity = max2(q.base(sc, &["QuantityMultiplier"]), 1.0);
    if quantity > 1.0 {
        off.out().set_num("QuantityMultiplier", quantity);
    }
    quantity
}

impl Off<'_> {
    /// The cfg of pass `pi` (`pass.cfg`).
    pub(crate) fn pcfg(&self, pi: usize) -> &ModCfg {
        &self.passes[pi].cfg
    }

    /// `pass.output[k]`.
    pub(crate) fn pval(&self, pi: usize, k: &str) -> Option<&OutValue> {
        match self.passes[pi].out {
            PassOut::Global => self.st.player.output.get(k),
            PassOut::Hand(h) => self.hands.as_ref().expect("hands")[h].map.get(k),
        }
    }

    /// `pass.output[k]` as a number (nil / non-number -> `None`).
    pub(crate) fn pnum(&self, pi: usize, k: &str) -> Option<f64> {
        match self.pval(pi, k) {
            Some(OutValue::Num(x)) => Some(*x),
            _ => None,
        }
    }

    /// `pass.output[k]` in arithmetic (nil is the Lua error).
    pub(crate) fn pnum_req(&self, pi: usize, k: &str, line: u32) -> f64 {
        match self.pval(pi, k) {
            Some(OutValue::Num(x)) => *x,
            other => panic!("{LUA}:{line}: attempt to perform arithmetic on field '{k}' ({other:?})"),
        }
    }

    /// `pass.output[k]` truthy.
    pub(crate) fn ptruthy(&self, pi: usize, k: &str) -> bool {
        !matches!(self.pval(pi, k), None | Some(OutValue::Bool(false)))
    }

    /// `pass.output[k] = v`; a hand's numbers are mirrored into its cfg's `skillStats` (the same table upstream).
    pub(crate) fn pset(&mut self, pi: usize, k: &str, v: OutValue) {
        match self.passes[pi].out {
            PassOut::Global => match v {
                OutValue::Num(x) => self.out().set_num(k, x),
                OutValue::Bool(b) => self.out().set_bool(k, b),
                OutValue::Str(s) => self.out().set_str(k, &s),
            },
            PassOut::Hand(h) => {
                let key = Str::from(k);
                let stats = self.passes[pi].cfg.skill_stats.get_or_insert_with(IndexMap::new);
                match &v {
                    OutValue::Num(x) => {
                        stats.insert(key.clone(), *x);
                    }
                    _ => {
                        stats.shift_remove(&key);
                    }
                }
                self.hands.as_mut().expect("hands")[h].map.insert(key, v);
            }
        }
    }

    pub(crate) fn pset_num(&mut self, pi: usize, k: &str, v: f64) {
        self.pset(pi, k, OutValue::Num(v));
    }

    /// `pass.source[k]` used as a number (nil / false -> `None`; any other non-number is Lua's arithmetic error).
    pub(crate) fn source_num(&self, pi: usize, k: &str, line: u32) -> Option<f64> {
        let v = match &self.passes[pi].source {
            Source::SkillData => self.sd(k),
            Source::Weapon(d) => d.get(k),
        };
        if !v.truthy() {
            return None;
        }
        Some(v.as_num().unwrap_or_else(|| {
            panic!(
                "{LUA}:{line}: attempt to perform arithmetic on field '{k}' (a {} value)",
                v.type_name()
            )
        }))
    }

    /// `pass.source[k] = v` (the skill pass's source IS `skillData`).
    pub(crate) fn source_set(&mut self, pi: usize, k: &str, v: Value) {
        if let Source::Weapon(d) = &mut self.passes[pi].source {
            d.set(k, v);
        } else {
            self.set_sd(k, v);
        }
    }

    /// `output[k] = v` for a value read off another output table (nil removes the key).
    pub(crate) fn set_out_value(&mut self, k: &str, v: Option<OutValue>) {
        let o = self.out();
        match v {
            None => o.remove(k),
            Some(OutValue::Num(x)) => o.set_num(k, x),
            Some(OutValue::Bool(b)) => o.set_bool(k, b),
            Some(OutValue::Str(s)) => o.set_str(k, &s),
        }
    }

    /// The conversion table a pass's `calcDamage` reads: `(cfg and cfg.conversionTable) or activeSkill.conversionTable`.
    pub(crate) fn pass_conversion(&self, pi: usize) -> &ConversionTable {
        match self.passes[pi].out {
            PassOut::Hand(h) => self.weapon_conversion[h].as_ref().unwrap_or(&self.conversion_table),
            PassOut::Global => &self.conversion_table,
        }
    }
}
