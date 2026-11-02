//! CalcOffence.lua:1056-1936 -- skill type stats (:1056-1488), skill duration (:1490-1657), uptime (:1659-1690), costs (:1692-1896),
//! Eldritch Battery (:1897), Sacrificial Zeal (:1900-1906), `preDamageFunc` (:1908), corpse/enemy explosions (:1915-1929) and the
//! damage-disabling flags (:1931-1935) -- up to the probe's OC2.

use std::cell::OnceCell;

use indexmap::IndexMap;
use pob_data::constants::{constants, monster_table_at};
use pob_lua::number::{floor, max2, min2, round};
use pob_lua::tostring_num;
use pob_mods::{ModCfg, ModType, Value};

use super::bdh::{calc_radius_breakpoints, molten_strike_tertiary_breakdown};
use super::c::{dmore, dsum};
use super::helpers::{
    DMG_TYPE_LIST, calc_area_mod, calc_molten_strike_tertiary_radius, calc_radius, calc_skill_cooldown, calc_skill_duration,
    calc_totem_life, calc_warcry_cast_time,
};
use super::{Off, Q, run_skill_func};
use crate::breakdown::{BTable, Chain, multi_chain, step};
use crate::setup::{cc, misc, ok};
use crate::sf;

const LUA: &str = "Modules/CalcOffence.lua";

/// `data.costs[resource].Divisor` (Data.lua:718-725: the array with an `__index` by `Resource`).
fn cost_divisor(resource: &str) -> f64 {
    thread_local! {
        static COSTS: OnceCell<Vec<(String, f64)>> = const { OnceCell::new() };
    }
    COSTS.with(|c| {
        let list = c.get_or_init(|| {
            let rows: Vec<pob_data::wire::OrderedMap<pob_data::wire::Scalar>> =
                pob_data::read_json("costs.json").unwrap_or_else(|e| panic!("{e}"));
            rows.iter()
                .map(|r| match (r.get("Resource"), r.get("Divisor")) {
                    (Some(pob_data::wire::Scalar::Str(res)), Some(pob_data::wire::Scalar::Num(d))) => (res.clone(), *d),
                    other => panic!("costs.json row {other:?}"),
                })
                .collect()
        });
        list.iter()
            .find(|(r, _)| r == resource)
            .map(|(_, d)| *d)
            .unwrap_or_else(|| panic!("{LUA}:1721: attempt to index field '?' (data.costs[\"{resource}\"] is nil)"))
    })
}

fn tick() -> f64 {
    misc("ServerTickRate")
}

/// `m_ceil(x * tick) / tick`.
fn to_tick(x: f64) -> f64 {
    (x * tick()).ceil() / tick()
}

/// :1056-1936.
#[allow(clippy::too_many_lines)]
pub(super) fn skill_type_stats(off: &mut Off<'_>) {
    let q = off.q.clone();
    let cfg = off.cfg.clone();
    let c = Some(&cfg);

    // :1057-1065 minion skills -- `activeSkill.minion.minionData` (R7.10)
    if let Some(limit) = off.sk().minion.as_deref().map(|m| m.md("limit")) {
        if limit.truthy() {
            let name = limit
                .as_str()
                .unwrap_or_else(|| panic!("{LUA}:1059: minionData.limit is not a string ({limit:?})"))
                .to_owned();
            let ov = ok(off.env_db.borrow().override_(None, &[&name]));
            let v = match ov {
                Some(v) if v.truthy() => v
                    .as_num()
                    .unwrap_or_else(|| panic!("{LUA}:1059: bad argument #1 to 'floor' (number expected, got {})", v.type_name())),
                _ => q.calc_val(&name, c) * q.more(c, &["ActiveMinionLimit"]),
            };
            off.out().set_num("ActiveMinionLimit", v.floor());
        }
        let per_cast = q.calc_val("MinionPerCastCount", c).floor();
        off.out()
            .set_num("SummonedMinionsPerCast", if per_cast == 0.0 { 1.0 } else { per_cast });
    }
    // :1066-1080 chains
    if off.flag("chaining") {
        if q.flag(c, &["CannotChain"]) || q.flag(c, &["NoAdditionalChains"]) {
            off.out().set_str("ChainMaxString", "Cannot chain");
        } else {
            let mut names = vec!["ChainCountMax"];
            if !off.flag("projectile") {
                names.push("BeamChainCountMax");
            }
            let mut chain_max = q.base(c, &names);
            if q.flag(c, &["AdditionalProjectilesAddChainsInstead"]) {
                let proj = if q.flag(c, &["SingleProjectile"]) {
                    0.0
                } else {
                    ((q.base(c, &["ProjectileCount"]) - 1.0) * q.more(c, &["ProjectileCount"])).floor()
                };
                chain_max += proj;
            }
            chain_max *= q.more(c, &["ChainCountMax"]);
            let chain = min2(chain_max, q.base(c, &["ChainCount"]));
            let o = off.out();
            o.set_num("ChainMax", chain_max);
            o.set_num("ChainMaxString", chain_max);
            o.set_num("Chain", chain);
            o.set_num("ChainRemaining", max2(0.0, chain_max - chain));
        }
    }
    // :1081-1156 projectiles
    if off.flag("projectile") {
        let ramp = |pts: &[(f64, f64)]| -> Value {
            let arr = pts
                .iter()
                .map(|(a, b)| {
                    Value::table(pob_mods::Table {
                        array: vec![Value::Num(*a), Value::Num(*b)],
                        ..pob_mods::Table::default()
                    })
                })
                .collect();
            Value::table(pob_mods::Table {
                array: arr,
                ..pob_mods::Table::default()
            })
        };
        let ap = crate::setup::mod_flag("Attack") | crate::setup::mod_flag("Projectile");
        if q.flag(None, &["PointBlank"]) {
            let t = crate::setup::tag(&[
                ("type", Value::str("DistanceRamp")),
                ("ramp", ramp(&[(10.0, 1.0), (35.0, 0.0), (120.0, -1.0)])),
            ]);
            q.nm("Damage", ModType::More, 30.0, "Point Blank", ap, 0, vec![t]);
        }
        if q.flag(None, &["FarShot"]) {
            let t = crate::setup::tag(&[
                ("type", Value::str("DistanceRamp")),
                ("ramp", ramp(&[(10.0, -0.2), (25.0, 0.0), (70.0, 0.6)])),
            ]);
            q.nm("Damage", ModType::More, 100.0, "Far Shot", ap, 0, vec![t]);
        }
        let single = q.flag(c, &["SingleProjectile"]);
        if q.flag(c, &["NoAdditionalProjectiles"]) || single {
            off.out().set_num("ProjectileCount", 1.0);
        } else {
            let proj_min = q.base(c, &["ProjectileCountMinimum"]);
            let proj_max = q
                .override_num(c, &["ProjectileCountMaximum"], "Modules/CalcOffence.lua:1096")
                .unwrap_or(f64::INFINITY);
            let proj = (q.base(c, &["ProjectileCount"]) * q.more(c, &["ProjectileCount"])).floor();
            off.out().set_num("ProjectileCount", max2(min2(proj, proj_max), proj_min));
        }
        if q.flag(c, &["AdditionalProjectilesAddBouncesInstead"]) {
            let proj_base = if q.flag(c, &["SingleProjectile"]) {
                0.0
            } else {
                q.base(c, &["ProjectileCount"]) + q.base(c, &["BounceCount"]) - 1.0
            };
            let proj_more = q.more(c, &["ProjectileCount"]);
            off.out().set_num("BounceCount", (proj_base * proj_more).floor());
        }
        let enemy_split = |off: &Off<'_>| ok(off.enemy_db.borrow().sum(ModType::Base, c, &["SelfSplitCount"]));
        if q.flag(c, &["CannotSplit"]) || off.has_type("ProjectileNumber") {
            // :1104-1109 under `if breakdown` -- writes output (R8)
            if off.breakdown {
                let split = q.base(c, &["SplitCount"]) + enemy_split(off);
                if split > 0.0 {
                    off.out().set_str("SplitCountString", "Cannot split");
                }
            }
        } else {
            let mut split = q.base(c, &["SplitCount"]) + enemy_split(off);
            if q.flag(c, &["AdditionalProjectilesAddSplitsInstead"]) {
                let added = if q.flag(c, &["SingleProjectile"]) {
                    0.0
                } else {
                    ((q.base(c, &["ProjectileCount"]) - 1.0) * q.more(c, &["ProjectileCount"])).floor()
                };
                split += added;
            }
            if q.flag(c, &["AdditionalChainsAddSplitsInstead"]) {
                split += q.base(c, &["ChainCountMax"]);
            }
            off.out().set_num("SplitCount", split);
            off.out().set_num("SplitCountString", split);
        }
        if q.flag(c, &["CannotFork"]) {
            off.out().set_str("ForkCountString", "Cannot fork");
        } else if q.flag(c, &["ForkOnce"]) {
            off.set_flag("forking", true);
            let cap = if q.flag(c, &["ForkTwice"]) { 2.0 } else { 1.0 };
            let fork_max = min2(q.base(c, &["ForkCountMax"]), cap);
            let forked = min2(fork_max, q.base(c, &["ForkedCount"]));
            let o = off.out();
            o.set_num("ForkCountMax", fork_max);
            o.set_num("ForkedCount", forked);
            o.set_num("ForkCountString", fork_max);
            o.set_num("ForkRemaining", max2(0.0, fork_max - forked));
        } else {
            off.out().set_str("ForkCountString", "0");
        }
        if q.flag(c, &["CannotPierce"]) {
            off.out().set_num("PierceCount", 0.0);
            off.out().set_str("PierceCountString", "Cannot pierce");
        } else {
            let all = q.flag(c, &["PierceAllTargets"]) || ok(off.enemy_db.borrow().flag(None, &["AlwaysPierceSelf"])) == Some(true);
            let pierce = if all {
                off.out().set_num("PierceCount", 100.0);
                off.out().set_str("PierceCountString", "All targets");
                100.0
            } else {
                let p = q.base(c, &["PierceCount"]);
                off.out().set_num("PierceCount", p);
                off.out().set_num("PierceCountString", p);
                p
            };
            if pierce > 0.0 {
                off.set_flag("piercing", true);
            }
            let pierced = min2(pierce, q.base(c, &["PiercedCount"]));
            off.out().set_num("PiercedCount", pierced);
        }
        let v = q.calc_mod(c, &["ProjectileSpeed"]);
        off.out().set_num("ProjectileSpeedMod", v);
        // :1153-1155
        if off.breakdown {
            off.bd_set("ProjectileSpeedMod", bmod(&q.list, c, &["ProjectileSpeed"]));
        }
    }
    // :1157-1183 melee
    if off.flag("melee") {
        for n in [1u8, 2] {
            if off.flag(if n == 1 { "weapon1Attack" } else { "weapon2Attack" }) {
                let range = {
                    let p = off.actor();
                    let d = if n == 1 { &p.weapon_data1 } else { &p.weapon_data2 };
                    d.get("range")
                };
                let wcfg = off.weapon_cfg(n);
                let r = if range.truthy() {
                    let r = range
                        .as_num()
                        .unwrap_or_else(|| panic!("{LUA}:1159: attempt to perform arithmetic on field 'range'"));
                    r + q.base(wcfg.as_ref(), &["MeleeWeaponRange"]) + 10.0 * q.base(wcfg.as_ref(), &["MeleeWeaponRangeMetre"])
                } else {
                    6.0 + q.base(c, &["UnarmedRange"]) + 10.0 * q.base(c, &["UnarmedRangeMetre"])
                };
                if n == 1 {
                    off.st.player.weapon_range1 = Some(r);
                } else {
                    off.st.player.weapon_range2 = Some(r);
                }
            }
        }
        if off.has_type("MeleeSingleTarget") {
            let mut range: f64 = 100.0;
            for n in [1u8, 2] {
                if off.flag(if n == 1 { "weapon1Attack" } else { "weapon2Attack" }) {
                    range = min2(range, weapon_range(off, n, 1167));
                }
            }
            let o = off.out();
            o.set_num("WeaponRange", range + 2.0);
            o.set_num("WeaponRangeMetre", (range + 2.0) / 10.0);
            // :1174-1178
            off.bd_put("WeaponRange", BTable::new().with("radius", range + 2.0));
            let v = 1.0 + q.base(c, &["AdditionalStrikeTarget"]);
            off.out().set_num("StrikeTargets", v);
        }
    }
    // :1184-1186 (3dbca4463 #10301: written before the area calc, so `PerStat ActiveTotemLimit` sees it)
    if off.flag("totem") {
        let limit = q.base(c, &["ActiveTotemLimit", "ActiveBallistaLimit"]);
        off.out().set_num("ActiveTotemLimit", limit);
    }
    // :1187-1189
    if off.flag("area") || off.sd_truthy("radius") || (off.flag("mine") && off.has_type("Aura")) {
        calc_area_of_effect(off, &q, &cfg);
    }
    // :1187-1192
    if off.has_type("Aura") {
        let mut names = vec!["AuraEffect"];
        if !(off.sd_truthy("auraCannotAffectSelf") || off.has_type("AuraAffectsEnemies")) {
            names.push("SkillAuraEffectOnSelf");
        }
        let v = q.calc_mod(c, &names);
        off.out().set_num("AuraEffectMod", v);
        if off.breakdown {
            off.bd_set("AuraEffectMod", bmod(&q.list, c, &names));
        }
    }
    // :1193-1212
    if off.has_type("HasReservation") && !off.has_type("ReservationBecomesCost") {
        for pool in ["Life", "Mana"] {
            let key = format!("{pool}ReservedMod");
            off.out().set_num(&key, 0.0);
            let (pr, pe) = (format!("{pool}Reserved"), format!("{pool}ReservationEfficiency"));
            if q.calc_mod(c, &["SupportManaMultiplier"]) > 0.0 && q.calc_mod(c, &[&pr, "Reserved"]) > 0.0 {
                let v = q.calc_mod(c, &[&pr, "Reserved"]) * floor(q.calc_mod(c, &["SupportManaMultiplier"]), Some(4.0))
                    / max2(0.0, q.calc_mod(c, &[&pe, "ReservationEfficiency"]));
                off.out().set_num(&key, v);
            }
            // :1199-1210
            if off.breakdown {
                let names = [pr.as_str(), "Reserved", "SupportManaMultiplier"];
                let inc = dsum(&q.list, ModType::Inc, c, &names);
                let more = dmore(&q.list, c, &names);
                if inc != 0.0 && more != 1.0 {
                    let t = BTable::lines([
                        sf!("%.2f ^8(increased/reduced)", 1.0 + inc / 100.0),
                        sf!("x %.2f ^8(more/less)", more),
                        sf!("/ %.2f ^8(reservation efficiency)", q.calc_mod(c, &[&pe, "ReservationEfficiency"])),
                        sf!("= %.2f", off.out_num(&key, 1207)),
                    ]);
                    off.bd_put(&key, t);
                }
            }
        }
    }
    // :1213-1218
    if off.has_type("Hex") || off.has_type("Mark") {
        let v = q.calc_mod(c, &["CurseEffect"]);
        off.out().set_num("CurseEffectMod", v);
        if off.breakdown {
            off.bd_set("CurseEffectMod", bmod(&q.list, c, &["CurseEffect"]));
        }
    }
    // :1219-1239
    if off.has_type("Warcry") {
        let full_duration = {
            let sd = &off.sk().skill_data;
            calc_skill_duration(&q.list, &q.malformed, &cfg, sd, off.env.mode_effective, &off.enemy_db)
        };
        let actual_cooldown = match q.override_num(c, &["CooldownRecovery"], "Modules/CalcOffence.lua:1222") {
            Some(v) => v,
            None => {
                let cd = off
                    .sd_num("cooldown", 1222)
                    .unwrap_or_else(|| panic!("{LUA}:1222: attempt to perform arithmetic on field 'cooldown' (a nil value)"));
                (cd + q.base(c, &["CooldownRecovery"])) / q.calc_mod(c, &["CooldownRecovery"])
            }
        };
        let max_hit = ok(off.env_db.borrow().flag(None, &["Condition:WarcryMaxHit"])) == Some(true);
        let uptime = if max_hit { 1.0 } else { min2(full_duration / actual_cooldown, 1.0) };
        let unscaled = q.calc_mod(c, &["WarcryEffect", "BuffEffect"]);
        off.out().set_num("WarcryEffectMod", unscaled * uptime);
        // :1226-1238
        if off.breakdown {
            let mut t = BTable::lines([sf!("%.2f ^8(effect modifiers)", unscaled)]);
            if max_hit || uptime != 1.0 {
                if max_hit {
                    t.push("* 100% uptime (WarcryMaxHit Override)");
                } else if uptime != 1.0 {
                    t.push(sf!("* %.2f%% ^8(uptime)", uptime * 100.0));
                }
                t.push(sf!("= %.1f%%", unscaled * uptime * 100.0));
            }
            off.bd_put("WarcryEffectMod", t);
        }
    }
    // :1240-1251
    if off.has_type("Link") {
        let v = q.calc_mod(c, &["LinkEffect", "BuffEffect"]);
        off.out().set_num("LinkEffectMod", v);
        if off.breakdown {
            off.bd_set("LinkEffectMod", bmod(&q.list, c, &["LinkEffect", "BuffEffect"]));
        }
    }
    if off.has_type("Buff") && off.has_type("Herald") {
        let v = q.calc_mod(c, &["BuffEffect", "BuffEffectOnSelf"]);
        off.out().set_num("HeraldBuffEffectMod", v);
        if off.breakdown {
            let db = off.env_db.clone();
            off.bd_set("HeraldBuffEffectMod", bmod(&db, c, &["BuffEffect", "BuffEffectOnSelf"]));
        }
    }
    // :1252-1256
    let has_cd = off.sd_truthy("trapCooldown") || off.sd_truthy("cooldown");
    if (off.flag("trap") || off.flag("mine")) && !has_cd {
        off.set_flag("notAverage", true);
        off.set_flag("showAverage", false);
        off.set_sd("showAverage", Value::Bool(false));
    }
    // :1257-1332
    if off.flag("trap") {
        trap(off, &q, &cfg);
    } else if off.sd_truthy("cooldown") || q.base(c, &["CooldownRecovery"]) != 0.0 {
        let (cooldown, rounded, added) = calc_skill_cooldown(&q, &cfg, &off.sk().skill_data);
        off.out().set_num("Cooldown", cooldown);
        // :1322-1331
        if off.breakdown {
            // `skillData.cooldown or 0 + addedCooldown` (precedence: `cooldown or (0 + addedCooldown)`)
            let base = off.sd_num("cooldown", 1324).unwrap_or_else(|| {
                0.0 + added.unwrap_or_else(|| panic!("{LUA}:1324: attempt to perform arithmetic on local 'addedCooldown' (a nil value)"))
            });
            let mut t = BTable::lines([
                sf!("%.2fs ^8(base)", base),
                sf!(
                    "/ %.2f ^8(increased/reduced cooldown recovery)",
                    1.0 + q.inc(c, &["CooldownRecovery"]) / 100.0
                ),
            ]);
            if rounded {
                t.push(sf!("rounded up to nearest server tick"));
            }
            t.push(sf!("= %.3fs", cooldown));
            off.bd_put("Cooldown", t);
        }
    }
    // :1333-1344
    if let Some(base_uses) = off.sd_num("storedUses", 1334) {
        let additional = q.base(c, &["AdditionalCooldownUses", "AdditionalUses"]);
        let v = base_uses + additional;
        off.out().set_num("StoredUses", v);
        if off.breakdown {
            let s = |n: f64| if n == 1.0 { "" } else { "s" };
            let mut t = BTable::lines([sf!("%d ^8(skill use%s)", base_uses, s(base_uses))]);
            if additional != 0.0 {
                t.push(sf!("+ %d ^8(additional use%s)", additional, s(additional)));
                t.push(sf!("= %d ^8(total use%s)", v, s(v)));
            }
            off.bd_put("StoredUses", t);
        }
    }
    // :1345-1412
    if off.flag("mine") {
        mine(off, &q, &cfg);
    }
    // :1413-1457
    if off.flag("totem") {
        let base_speed = 1.0
            / if off.flag("ballista") {
                q.base(c, &["BallistaPlacementTime"])
            } else {
                q.base(c, &["TotemPlacementTime"])
            };
        let action = off.out_num("ActionSpeedMod", 1420);
        let speed = base_speed * q.calc_mod(c, &["TotemPlacementSpeed"]) * action;
        // :1436 `env.modDB:Override(nil, "TotemsSummoned") or output.ActiveTotemLimit` (set at :1184-1186)
        let limit = off.out_num("ActiveTotemLimit", 1436);
        let summoned = or_override(&off.env_db, "TotemsSummoned", limit, 1436);
        let o = off.out();
        o.set_num("TotemPlacementSpeed", speed);
        o.set_num("TotemPlacementTime", 1.0 / speed);
        o.set_num("TotemsSummoned", summoned);
        // :1422-1441
        if off.breakdown {
            let mut t = BTable::new();
            multi_chain(
                &mut t,
                &Chain {
                    label: Some("Placement speed:".into()),
                    base: Some(step("%.2f ^8(base placement speed)", base_speed)),
                    steps: vec![
                        step(
                            "%.2f ^8(increased/reduced placement speed)",
                            1.0 + q.inc(c, &["TotemPlacementSpeed"]) / 100.0,
                        ),
                        step("%.2f ^8(more/less placement speed)", q.more(c, &["TotemPlacementSpeed"])),
                        step("%.2f ^8(action speed modifier)", action),
                    ],
                    total: Some(sf!("= %.2f ^8per second", speed)),
                    ..Chain::default()
                },
            );
            off.bd_put("TotemPlacementTime", t);
            let note = if super::c::config_truthy(off, "TotemsSummoned") {
                " ^8(overridden from the Configuration tab)"
            } else {
                " ^8(can be overridden in the Configuration tab)"
            };
            off.bd_put(
                "ActiveTotemLimit",
                BTable::lines([format!("Totems Summoned: {}{note}", tostring_num(summoned))]),
            );
        }
        let level = off
            .sd_num("totemLevel", 340)
            .unwrap_or_else(|| panic!("{LUA}:340: attempt to index field '?' (a nil value)"));
        let (life, life_mod) = calc_totem_life(&q, &cfg, level, off.sk().skill_totem_id);
        let (es, block, armour) = (
            q.base(c, &["TotemEnergyShield"]),
            q.base(c, &["TotemBlockChance"]),
            q.base(c, &["TotemArmour"]),
        );
        let o = off.out();
        o.set_num("TotemLife", life);
        o.set_num("TotemLifeMod", life_mod);
        o.set_num("TotemEnergyShield", es);
        o.set_num("TotemBlockChance", block);
        o.set_num("TotemArmour", armour);
        // :1444-1456
        if off.breakdown {
            off.bd_set("TotemLifeMod", bmod(&q.list, c, &["TotemLife"]));
            let consts = constants();
            let ally = monster_table_at(&consts.monster_ally_life_table, level).expect("checked by calcTotemLife");
            let mult = off
                .sk()
                .skill_totem_id
                .and_then(|t| consts.totem_life_mult.0.get(&tostring_num(t)).copied())
                .expect("checked by calcTotemLife");
            let lv = tostring_num(level);
            off.bd_put(
                "TotemLife",
                BTable::lines([
                    format!("Totem level: {lv}"),
                    format!("{} ^8(base life for a level {lv} monster)", tostring_num(ally)),
                    format!("x {} ^8(life multiplier for this totem type)", tostring_num(mult)),
                    format!("x {} ^8(totem life modifier)", tostring_num(life_mod)),
                    format!("= {}", tostring_num(life)),
                ]),
            );
            off.bd_set("TotemEnergyShield", bmod(&q.list, c, &["TotemEnergyShield"]));
            off.bd_set("TotemBlockChance", bmod(&q.list, c, &["TotemBlockChance"]));
            off.bd_set("TotemArmour", bmod(&q.list, c, &["TotemArmour"]));
        }
    }
    // :1458-1466
    if off.has_type("Brand") {
        let range = misc("BrandAttachmentRangeBase") * q.calc_mod(c, &["BrandAttachmentRange"]);
        let limit = q.base(c, &["ActiveBrandLimit"]);
        let attached = off.sd("attachedBrandCount");
        let o = off.out();
        o.set_num("BrandAttachmentRange", range);
        o.set_num("BrandAttachmentRangeMetre", range / 10.0);
        o.set_num("ActiveBrandLimit", limit);
        off.set_out("AttachedBrandCount", attached);
        off.bd_put("BrandAttachmentRange", BTable::new().with("radius", range));
    }
    // :1468-1470
    if off.flag("warcry") {
        let action = crate::defence::action_speed_mod(&off.mod_db);
        let t = calc_warcry_cast_time(&q, &cfg, &off.sk().skill_data, action);
        off.out().set_num("WarcryCastTime", t);
    }
    // :1472-1488
    if off.flag("corpse") {
        let level = q.base(c, &["CorpseLevel"]);
        let consts = constants();
        let life = monster_table_at(&consts.monster_life_table, level)
            .unwrap_or_else(|| panic!("{LUA}:1474: attempt to perform arithmetic on a nil value (field '?')"));
        let variety = off.sd("corpseMonsterVariety");
        let variety_mult = variety
            .as_str()
            .and_then(|v| consts.monster_variety_life_mult.0.get(v))
            .copied()
            .unwrap_or(1.0);
        let map_mult = consts
            .map_level_life_mult
            .0
            .get(&tostring_num(off.env.enemy_level))
            .copied()
            .unwrap_or(1.0);
        let base_life = life * variety_mult * map_mult;
        let inc = 1.0 + q.inc(c, &["CorpseLife"]) / 100.0;
        let o = off.out();
        o.set_num("CorpseLevel", level);
        o.set_num("BaseCorpseLife", base_life);
        o.set_num("CorpseLifeInc", inc);
        o.set_num("CorpseLife", base_life * inc);
        // :1477-1487
        if off.breakdown {
            let variety_s = match &variety {
                Value::Str(s) => s.to_string(),
                Value::Nil => "nil".to_owned(),
                v => panic!("{LUA}:1480: corpseMonsterVariety is a {}", v.type_name()),
            };
            off.bd_put(
                "CorpseLife",
                BTable::lines([
                    sf!("%d ^8(base life of a level %d monster)", life, level),
                    sf!("x %.2f ^8(%s variety multiplier)", variety_mult, variety_s),
                    sf!(
                        "x %.2f ^8(map level %d monster life multiplier from config)",
                        map_mult,
                        off.env.enemy_level
                    ),
                    sf!(" = %d ^8(base corpse life)", base_life),
                    sf!(""),
                    sf!("x %.2f ^8(corpse maximum life increases)", inc),
                    sf!(" = %d", base_life * inc),
                ]),
            );
        }
    }

    durations(off, &q, &cfg);
    uptime(off, &q, &cfg);
    costs(off, &q, &cfg);

    // :1897-1898 Eldritch Battery
    let unreserved = off.st.player.output.opt_num("ManaUnreserved").unwrap_or(0.0);
    let es = if ok(off.mod_db.borrow().flag(None, &["EnergyShieldProtectsMana"])) == Some(true) {
        off.out_num("EnergyShield", 1898)
    } else {
        0.0
    };
    off.out().set_num("ManaCostPayablePool", unreserved + es);
    // :1902-1906 Sacrificial Zeal
    if q.flag(None, &["Condition:SacrificialZeal"])
        && matches!(off.st.player.output.get("ManaHasCost"), Some(crate::output::OutValue::Bool(true)))
    {
        let v = (off.out_num("ManaCost", 1904) * 0.25).floor();
        let spell = crate::setup::mod_flag("Spell");
        q.nm("PhysicalMin", ModType::Base, v, "Sacrificial Zeal", spell, 0, vec![]);
        q.nm("PhysicalMax", ModType::Base, v, "Sacrificial Zeal", spell, 0, vec![]);
    }
    // :1914-1924 Sacrifice Support (3dbca4463 #10363: the totem's life for totems; was a PercentStat mod in the skill data)
    if off.sd_truthy("sacrificeLifePercent") {
        let life = if off.flag("totem") {
            off.out_num("TotemLife", 1921)
        } else {
            off.out_num("LifeUnreserved", 1921)
        };
        let pct = off.sd_num("sacrificeLifePercent", 1921).expect("truthy");
        let as_chaos = off
            .sd_num("sacrificeLifeAsChaos", 1921)
            .unwrap_or_else(|| panic!("{LUA}:1921: attempt to perform arithmetic on field 'sacrificeLifeAsChaos' (a nil value)"));
        let add = (life * pct / 100.0 * (as_chaos / 100.0)).floor();
        q.nm("ChaosMin", ModType::Base, add, "Sacrifice Support", 0, 0, vec![]);
        q.nm("ChaosMax", ModType::Base, add, "Sacrifice Support", 0, 0, vec![]);
    }

    // :1926
    run_skill_func(off, "preDamageFunc");

    // :1928-1931 `activeSkill.infoMessage` (display only); the `%d` of a nil count is the Lua error
    if off.has_type("Brand") {
        let Some(n) = off.st.player.output.opt_num("AttachedBrandCount") else {
            panic!("{LUA}:1930: bad argument #2 to 'format' (number expected, got nil)");
        };
        let label = if off.sd_truthy("countsAttachedBrandsInDamage") {
            "Average Damage"
        } else {
            "DPS"
        };
        let plural = if n == 1.0 { "" } else { "s" };
        off.sk_mut().info_message = Some(format!("{label} for {} attached Brand{plural}", crate::sf!("%d", n)));
    }

    // :1916-1929 corpse and enemy explosions
    let monster_life = match off.sd_num("corpseLife", 1916) {
        Some(v) => v,
        None => monster_table_at(&constants().monster_life_table, off.env.enemy_level).unwrap_or(100.0),
    };
    off.monster_life = monster_life;
    if off.sd_truthy("explodeCorpse") {
        let dt = match off.sd("corpseExplosionDamageType") {
            Value::Str(s) => s.to_string(),
            Value::Nil | Value::Bool(false) => "Fire".to_owned(),
            o => panic!("{LUA}:1919: attempt to concatenate a {} value", o.type_name()),
        };
        let mult = off
            .sd_num("corpseExplosionLifeMultiplier", 1919)
            .or_else(|| off.sd_num("selfFireExplosionLifeMultiplier", 1919))
            .unwrap_or_else(|| panic!("{LUA}:1919: attempt to perform arithmetic on a nil value"));
        off.set_sd(&format!("{dt}BonusMin"), Value::Num(monster_life * mult));
        off.set_sd(&format!("{dt}BonusMax"), Value::Num(monster_life * mult));
    }
    if off.flag("monsterExplode") {
        for dt in DMG_TYPE_LIST {
            let pct = off.sd_num(&format!("{dt}EffectiveExplodePercentage"), 1925).unwrap_or(0.0);
            let base = pct * monster_life / 100.0;
            off.set_sd(&format!("{dt}Min"), Value::Num(base));
            off.set_sd(&format!("{dt}Max"), Value::Num(base));
        }
    }
    // :1949-1958 `canDeal` (read by the damage passes, R7.5) and, with a breakdown, `damageDisableMods`
    for dt in DMG_TYPE_LIST {
        let names = [format!("DealNo{dt}"), "DealNoDamage".to_owned()];
        let names: [&str; 2] = [&names[0], &names[1]];
        let disabled = q.flag(c, &names);
        off.can_deal.insert(dt, !disabled);
        if off.breakdown && disabled {
            let rows = ok(q.list.borrow().tabulate(Some(ModType::Flag), c, &names));
            off.damage_disable_mods.insert(dt, rows);
        }
    }
}

/// `actor.weaponRange<n>` in arithmetic (nil is the Lua error).
fn weapon_range(off: &Off<'_>, n: u8, line: u32) -> f64 {
    let r = if n == 1 {
        off.st.player.weapon_range1
    } else {
        off.st.player.weapon_range2
    };
    r.unwrap_or_else(|| panic!("{LUA}:{line}: bad argument #2 to 'max' (number expected, got nil)"))
}

/// `env.modDB:Override(nil, name) or fallback`.
fn or_override(db: &pob_mods::StoreRef, name: &str, fallback: f64, line: u32) -> f64 {
    match ok(db.borrow().override_(None, &[name])) {
        Some(v) if v.truthy() => v
            .as_num()
            .unwrap_or_else(|| panic!("{LUA}:{line}: override {name} is a {}", v.type_name())),
        _ => fallback,
    }
}

/// :374-492 `calcAreaOfEffect` (with its breakdown tables, R8.5).
#[allow(clippy::too_many_lines)]
pub(super) fn calc_area_of_effect(off: &mut Off<'_>, q: &Q, cfg: &ModCfg) {
    let c = Some(cfg);
    let (inc_area, more_area) = q.calc_mods(c, &["AreaOfEffect", "AreaOfEffectPrimary"]);
    let area_mod = calc_area_mod(inc_area, more_area);
    off.out().set_num("AreaOfEffectMod", area_mod);
    let radius_mod = q.calc_mod(c, &["AreaOfEffectRadius"]);
    if off.sd_truthy("radiusIsWeaponRange") {
        let mut range: f64 = 0.0;
        if off.flag("weapon1Attack") {
            range = max2(range, weapon_range(off, 1, 381));
        }
        if off.flag("weapon2Attack") {
            range = max2(range, weapon_range(off, 2, 384));
        }
        off.set_sd("radius", Value::Num(range + 2.0));
    }
    let Some(radius) = off.sd_num("radius", 390) else {
        area_mod_breakdown(off, q, cfg, area_mod);
        return;
    };
    off.set_flag("area", true);
    let extra = off.sd_num("radiusExtra", 390).unwrap_or(0.0);
    let base_radius = radius + extra + q.base(c, &["AreaOfEffect"]);
    let r = calc_radius(base_radius, area_mod, Some(radius_mod));
    off.out().set_num("AreaOfEffectRadius", r);
    off.out().set_num("AreaOfEffectRadiusMetres", r / 10.0);
    // :393-396
    if off.breakdown {
        let bps = calc_radius_breakpoints(base_radius, inc_area, more_area, Some(radius_mod));
        let label = sd_label(off, "radiusLabel");
        let t = crate::breakdown::area(base_radius, area_mod, r, bps, label.as_deref(), Some(radius_mod));
        off.bd_put("AreaOfEffectRadius", t);
    }
    // :397-410
    let secondary = off.sd_num("radiusSecondary", 400);
    if let Some(sec) = secondary {
        let (i2, m2) = q.calc_mods(c, &["AreaOfEffect", "AreaOfEffectSecondary"]);
        let am2 = calc_area_mod(i2, m2);
        let r2 = calc_radius(sec + extra, am2, Some(radius_mod));
        let o = off.out();
        o.set_num("AreaOfEffectModSecondary", am2);
        o.set_num("AreaOfEffectRadiusSecondary", r2);
        o.set_num("AreaOfEffectRadiusSecondaryMetres", r2 / 10.0);
        // :403-409
        if off.breakdown {
            let bps = if off.sd_truthy("projectileSpeedAppliesToMSAreaOfEffect") {
                None
            } else {
                calc_radius_breakpoints(sec + extra, i2, m2, Some(radius_mod))
            };
            let label = sd_label(off, "radiusSecondaryLabel");
            let t = crate::breakdown::area(sec + extra, am2, r2, bps, label.as_deref(), Some(radius_mod));
            off.bd_put("AreaOfEffectRadiusSecondary", t);
        }
    }
    // :411-480
    if let Some(ter) = off.sd_num("radiusTertiary", 414) {
        let (i3, m3) = q.calc_mods(c, &["AreaOfEffect", "AreaOfEffectTertiary"]);
        let am3 = calc_area_mod(i3, m3);
        off.out().set_num("AreaOfEffectModTertiary", am3);
        let base3 = ter + extra;
        let r3 = if off.sd_truthy("projectileSpeedAppliesToMSAreaOfEffect") {
            let (is, ms) = q.calc_mods(c, &["ProjectileSpeed"]);
            let speed_mod = round(round(is * ms, Some(10.0)), Some(2.0));
            off.out().set_num("SpeedModTertiary", speed_mod);
            let dead = secondary.unwrap_or_else(|| panic!("{LUA}:418: attempt to perform arithmetic on a nil value"));
            let r3 = calc_molten_strike_tertiary_radius(base3, dead, am3, speed_mod);
            // :419-424
            if off.breakdown {
                let label = sd_label(off, "radiusTertiaryLabel");
                let bd = off.bd().expect("breakdown");
                molten_strike_tertiary_breakdown(bd, dead, base3, label.as_deref(), i3, m3, is, ms);
            }
            r3
        } else if let Some(margin_pct) = off.sd_num("radiusTertiaryBaseMargin", 426) {
            let margin = margin_pct / 100.0;
            let margin_width = margin_pct * 2.0 + 1.0;
            // :429-433 `for deviation = 1 - margin, 1 + margin + 0.01, 0.01` (LuaJIT FORL: accumulated step)
            let mut base_occ: IndexMap<i64, f64> = IndexMap::new();
            let (mut dev, stop) = (1.0 - margin, 1.0 + margin + 0.01);
            while dev <= stop {
                let rd = (base3 * dev).floor() as i64;
                *base_occ.entry(rd).or_insert(0.0) += 1.0;
                dev += 0.01;
            }
            // :435-443 `pairs(baseRadiiOccurrences)` -- integer sums (order-free), but the walk decides the insertion order of
            // `radiiOccurrences`, whose own `pairs` order ExplosiveTrap's float sum follows (MINEFIELD-RUST R11): both are the `lj_tab` order
            let base_keys: Vec<f64> = base_occ.keys().map(|k| *k as f64).collect();
            let mut sum = 0.0;
            let mut occ: IndexMap<i64, f64> = IndexMap::new();
            for adj in pob_tree::lj_table::pairs_order(&base_keys) {
                let count = base_occ[&(adj as i64)];
                let rd = calc_radius(adj, am3, Some(radius_mod));
                sum += rd * count;
                *occ.entry(rd as i64).or_insert(0.0) += count;
            }
            let occ_keys: Vec<f64> = occ.keys().map(|k| *k as f64).collect();
            let occ = pob_tree::lj_table::pairs_order(&occ_keys)
                .into_iter()
                .map(|k| (k as i64, occ[&(k as i64)]))
                .collect();
            off.radius_tertiary_occurrences = Some(occ);
            let r3 = sum / margin_width;
            // :446-471
            if off.breakdown {
                let mut out = BTable::new();
                let (mut inc_bp, mut more_bp, mut red_bp, mut less_bp) = (f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::INFINITY);
                if let Some(l) = sd_label(off, "radiusTertiaryLabel") {
                    out.push(l);
                }
                out.push(sf!(
                    "R ^8(base radius)^7 x %.2f ^8(square root of area of effect modifier)",
                    (100.0 * am3.sqrt()).floor() / 100.0
                ));
                if radius_mod != 1.0 {
                    out.push(sf!("x %.2f ^8(final radius modifier)", radius_mod));
                }
                // `pairs(baseRadiiOccurrences)` then `table.sort(a < b)`: integer keys, ascending
                let mut base_radii: Vec<i64> = base_occ.keys().copied().collect();
                base_radii.sort_unstable();
                for adj in base_radii {
                    let adjf = adj as f64;
                    out.push(sf!(
                        "%.1f%% ^8chance of^7 %.1fm ^8base radius resulting in^7 %.1fm ^8final radius",
                        base_occ[&adj] / margin_width * 100.0,
                        adjf / 10.0,
                        calc_radius(adjf, am3, Some(radius_mod)) / 10.0
                    ));
                    // `m_min(x, nil)` is a Lua error when the radius is 0
                    let (a, b, c3, d) = calc_radius_breakpoints(adjf, i3, m3, Some(radius_mod))
                        .unwrap_or_else(|| panic!("{LUA}:463: bad argument #2 to 'min' (number expected, got nil)"));
                    inc_bp = min2(inc_bp, a);
                    more_bp = min2(more_bp, b);
                    red_bp = min2(red_bp, c3);
                    less_bp = min2(less_bp, d);
                }
                out.push(sf!(
                    "^8Next closest 0.1m breakpoint: %d%% increased AoE / a %d%% more AoE multiplier",
                    inc_bp,
                    more_bp
                ));
                out.push(sf!(
                    "^8Previous closest 0.1m breakpoint: %d%% reduced AoE / a %d%% less AoE multiplier",
                    red_bp,
                    less_bp
                ));
                out.push(sf!("On average, the radius is %.2fm", r3 / 10.0));
                off.bd_put("AreaOfEffectRadiusTertiary", out);
            }
            r3
        } else {
            let r3 = calc_radius(base3, am3, Some(radius_mod));
            // :474-477
            if off.breakdown {
                let bps = calc_radius_breakpoints(base3, i3, m3, Some(radius_mod));
                let label = sd_label(off, "radiusTertiaryLabel");
                let t = crate::breakdown::area(base3, am3, r3, bps, label.as_deref(), Some(radius_mod));
                off.bd_put("AreaOfEffectRadiusTertiary", t);
            }
            r3
        };
        off.out().set_num("AreaOfEffectRadiusTertiary", r3);
        off.out().set_num("AreaOfEffectRadiusTertiaryMetres", r3 / 10.0);
    }
    area_mod_breakdown(off, q, cfg, area_mod);
}

/// :482-491 (the tail of `calcAreaOfEffect`, also reached when `skillData.radius` is nil).
fn area_mod_breakdown(off: &mut Off<'_>, q: &Q, cfg: &ModCfg, area_mod: f64) {
    if !off.breakdown {
        return;
    }
    let c = Some(cfg);
    let mut t = BTable::new();
    if area_mod != 1.0 {
        multi_chain(
            &mut t,
            &Chain {
                steps: vec![
                    step("%.2f ^8(increased/reduced)", 1.0 + q.inc(c, &["AreaOfEffect"]) / 100.0),
                    step("%.2f ^8(more/less)", q.more(c, &["AreaOfEffect"])),
                ],
                total: Some(sf!("= %.2f", area_mod)),
                ..Chain::default()
            },
        );
    }
    off.bd_put("AreaOfEffectMod", t);
}

/// `breakdown.mod(db, cfg, ...)` (CalcBreakdown.lua:68-78) as a breakdown value (nil -> `None`).
fn bmod(db: &pob_mods::StoreRef, cfg: Option<&ModCfg>, names: &[&str]) -> Option<crate::breakdown::BVal> {
    crate::breakdown::mod_(db, cfg, names).map(Into::into)
}

/// `skillData[k]` as a breakdown label (`t_insert(out, label)`: nil inserts nothing).
fn sd_label(off: &Off<'_>, k: &str) -> Option<String> {
    match off.sd(k) {
        Value::Nil => None,
        Value::Str(s) => Some(s.to_string()),
        v => panic!("{LUA}: skillData.{k} is a {} (breakdown label)", v.type_name()),
    }
}

/// :1257-1332 traps.
fn trap(off: &mut Off<'_>, q: &Q, cfg: &ModCfg) {
    let c = Some(cfg);
    let mut base_speed = 1.0 / q.base(c, &["TrapThrowingTime"]);
    let time_mod = q.calc_mod(c, &["SkillTrapThrowingTime"]);
    if time_mod > 0.0 {
        base_speed *= 1.0 / time_mod;
    }
    let action = off.out_num("ActionSpeedMod", 1263);
    let mut speed = base_speed * q.calc_mod(c, &["TrapThrowingSpeed"]) * action;
    let mut count = q.calc_val("TrapThrowCount", c);
    let base_cooldown = off.sd_num("trapCooldown", 1293).or_else(|| off.sd_num("cooldown", 1293));
    if base_cooldown.is_some() {
        count = 1.0;
    }
    let count = or_override(&off.env_db, "TrapThrowCount", count, 1268);
    speed = min2(speed, tick());
    let time = 1.0 / speed;
    let o = off.out();
    o.set_num("TrapThrowCount", count);
    o.set_num("TrapThrowingSpeed", speed);
    o.set_num("TrapThrowingTime", time);
    off.set_sd("timeOverride", Value::Num(time / count));
    // :1272-1291
    if off.breakdown {
        let mut t = BTable::new();
        multi_chain(
            &mut t,
            &Chain {
                label: Some("Throwing rate:".into()),
                base: Some(step("%.2f ^8(base throwing rate)", base_speed)),
                steps: vec![
                    step(
                        "%.2f ^8(increased/reduced throwing speed)",
                        1.0 + q.inc(c, &["TrapThrowingSpeed"]) / 100.0,
                    ),
                    step("%.2f ^8(more/less throwing speed)", q.more(c, &["TrapThrowingSpeed"])),
                    step("%.2f ^8(action speed modifier)", action),
                ],
                total: Some(sf!("= %.2f ^8per second", speed)),
                ..Chain::default()
            },
        );
        off.bd_put("TrapThrowingSpeed", t);
        if time_mod > 0.0 {
            let mut t = BTable::new();
            multi_chain(
                &mut t,
                &Chain {
                    label: Some("Throwing time:".into()),
                    base: Some(step("%.2f ^8(base throwing time)", 1.0 / (speed * time_mod))),
                    steps: vec![step("%.2f ^8(total modifier)", time_mod)],
                    total: Some(sf!("= %.2f ^8seconds per throw", time)),
                    ..Chain::default()
                },
            );
            off.bd_put("TrapThrowingTime", t);
        }
    }
    if base_cooldown.is_some() || q.base(c, &["CooldownRecovery"]) != 0.0 {
        let cd = match base_cooldown {
            // :1299 `skillModList:Override(skillCfg, "CooldownRecovery") or baseCooldown / calcLib.mod(...)` (3dbca4463 #10335)
            Some(b) => to_tick(
                q.override_num(c, &["CooldownRecovery"], "Modules/CalcOffence.lua:1300")
                    .unwrap_or_else(|| b / q.calc_mod(c, &["CooldownRecovery"])),
            ),
            None => calc_skill_cooldown(q, cfg, &off.sk().skill_data).0,
        };
        off.out().set_num("TrapCooldown", cd);
        // :1302-1309
        if off.breakdown {
            let t = BTable::lines([
                sf!("%.2fs ^8(base)", base_cooldown.unwrap_or(4.0)),
                sf!(
                    "/ %.2f ^8(increased/reduced cooldown recovery)",
                    1.0 + q.inc(c, &["CooldownRecovery"]) / 100.0
                ),
                "rounded up to nearest server tick".to_owned(),
                sf!("= %.3fs", cd),
            ]);
            off.bd_put("TrapCooldown", t);
        }
    }
    let (inc, more) = q.calc_mods(c, &["TrapTriggerAreaOfEffect"]);
    let area_mod = calc_area_mod(inc, more);
    let r = calc_radius(misc("TrapTriggerRadiusBase"), area_mod, None);
    off.out().set_num("TrapTriggerRadius", r);
    off.out().set_num("TrapTriggerRadiusMetre", r / 10.0);
    // :1315-1318
    if off.breakdown {
        let base = misc("TrapTriggerRadiusBase");
        let bps = calc_radius_breakpoints(base, inc, more, None);
        off.bd_put("TrapTriggerRadius", crate::breakdown::area(base, area_mod, r, bps, None, None));
    }
}

/// :1345-1412 mines.
fn mine(off: &mut Off<'_>, q: &Q, cfg: &ModCfg) {
    let c = Some(cfg);
    let mut base_speed = 1.0 / q.base(c, &["MineLayingTime"]);
    let time_mod = q.calc_mod(c, &["SkillMineThrowingTime"]);
    if time_mod > 0.0 {
        base_speed *= 1.0 / time_mod;
    }
    let action = off.out_num("ActionSpeedMod", 1351);
    let mut speed = base_speed * q.calc_mod(c, &["MineLayingSpeed"]) * action;
    let mut count = q.calc_val("MineThrowCount", c);
    if off.sd_truthy("trapCooldown") || off.sd_truthy("cooldown") {
        count = 1.0;
    }
    let count = or_override(&off.env_db, "MineThrowCount", count, 1357);
    if count >= 1.0 {
        speed /= 1.0 + (count - 1.0) * 0.1;
    }
    speed = min2(speed, tick());
    let time = 1.0 / speed;
    let o = off.out();
    o.set_num("MineThrowCount", count);
    o.set_num("MineLayingSpeed", speed);
    o.set_num("MineLayingTime", time);
    let t = if off.flag("trap") {
        time / count / off.out_num("TrapThrowCount", 1368)
    } else {
        time / count
    };
    off.set_sd("timeOverride", Value::Num(t));
    // :1373-1394
    if off.breakdown {
        let mut t = BTable::new();
        multi_chain(
            &mut t,
            &Chain {
                label: Some("Throwing rate:".into()),
                base: Some(step("%.2f ^8(base throwing rate)", base_speed)),
                steps: vec![
                    step(
                        "%.2f ^8(increased/reduced throwing speed)",
                        1.0 + q.inc(c, &["MineLayingSpeed"]) / 100.0,
                    ),
                    step("%.2f ^8(more/less throwing speed)", q.more(c, &["MineLayingSpeed"])),
                    step("%.2f ^8(action speed modifier)", action),
                    step("%.2f ^8(additional mine thrown)", 1.0 / (1.0 + (count - 1.0) * 0.1)),
                ],
                total: Some(sf!("= %.2f ^8per second", speed)),
                ..Chain::default()
            },
        );
        off.bd_put("MineLayingTime", t);
        if time_mod > 0.0 {
            let mut t = BTable::new();
            multi_chain(
                &mut t,
                &Chain {
                    label: Some("Throwing time:".into()),
                    base: Some(step("%.2f ^8(base throwing time)", 1.0 / (speed * time_mod))),
                    steps: vec![step("%.2f ^8(total modifier)", time_mod)],
                    total: Some(sf!("= %.2f ^8seconds per throw", time)),
                    ..Chain::default()
                },
            );
            off.bd_put("MineThrowingTime", t);
        }
    }
    let (inc, more) = q.calc_mods(c, &["MineDetonationAreaOfEffect"]);
    let area_mod = calc_area_mod(inc, more);
    let r = calc_radius(misc("MineDetonationRadiusBase"), area_mod, None);
    off.out().set_num("MineDetonationRadius", r);
    off.out().set_num("MineDetonationRadiusMetre", r / 10.0);
    // :1399-1402
    if off.breakdown {
        let base = misc("MineDetonationRadiusBase");
        let bps = calc_radius_breakpoints(base, inc, more, None);
        off.bd_put("MineDetonationRadius", crate::breakdown::area(base, area_mod, r, bps, None, None));
    }
    if off.has_type("Aura") {
        let am = off.out_num("AreaOfEffectMod", 1404);
        let r = calc_radius(misc("MineAuraRadiusBase"), am, None);
        off.out().set_num("MineAuraRadius", r);
        off.out().set_num("MineAuraRadiusMetre", r / 10.0);
        // :1406-1410
        if off.breakdown {
            let (inc, more) = q.calc_mods(c, &["AreaOfEffect"]);
            let base = misc("MineAuraRadiusBase");
            let bps = calc_radius_breakpoints(base, inc, more, None);
            off.bd_put("MineAuraRadius", crate::breakdown::area(base, am, r, bps, None, None));
        }
    }
}

/// :1490-1657 skill duration.
fn durations(off: &mut Off<'_>, q: &Q, cfg: &ModCfg) {
    let c = Some(cfg);
    // :1491-1494
    let mut debuff_mult = 1.0;
    if off.env.mode_effective {
        debuff_mult = 1.0
            / max2(
                misc("BuffExpirationSlowCap"),
                ok(crate::calc_lib::mod_(&off.enemy_db.borrow(), c, &["BuffExpireFaster"])),
            );
    }
    off.debuff_duration_mult = debuff_mult;
    let mine_dur = off.sd_truthy("mineDurationAppliesToSkill");
    let with_mine = |mut v: Vec<&'static str>| {
        if mine_dur {
            v.push("MineDuration");
        }
        v
    };
    // :1496-1524
    let duration_mod = max2(q.calc_mod(c, &with_mine(vec!["Duration", "PrimaryDuration"])), 0.0);
    off.out().set_num("DurationMod", duration_mod);
    // :1498-1503
    if off.breakdown {
        let mut t = crate::breakdown::mod_(&q.list, c, &with_mine(vec!["Duration", "PrimaryDuration"]));
        if let Some(t) = t.as_mut()
            && off.sd_truthy("durationSecondary")
        {
            t.insert(1, "Primary duration:");
        }
        off.bd_set("DurationMod", t.map(Into::into));
    }
    let duration_base = off.sd_num("duration", 1504).unwrap_or(0.0) + q.base(c, &["Duration", "PrimaryDuration"]);
    let permanent = match off.sk().minion.as_ref().map(|m| m.ty.clone()) {
        Some(t) => q.flag(c, &[&format!("{t}PermanentDuration")]),
        None => false,
    };
    if duration_base > 0.0 && !permanent {
        let mut d = duration_base * duration_mod;
        let debuff = off.sd_truthy("debuff");
        if debuff {
            d *= debuff_mult;
        }
        let d = to_tick(d);
        off.out().set_num("Duration", d);
        // :1511-1523
        if off.breakdown && d != duration_base {
            let mut t = BTable::lines([sf!("%.2fs ^8(base)", duration_base)]);
            if duration_mod != 1.0 {
                t.push(sf!("x %.4f ^8(duration modifier)", duration_mod));
            }
            if debuff && debuff_mult != 1.0 {
                t.push(sf!("/ %.3f ^8(debuff expires slower/faster)", 1.0 / debuff_mult));
            }
            t.push(sf!("rounded up to nearest server tick"));
            t.push(sf!("= %.3fs", d));
            off.bd_put("Duration", t);
        }
    }
    // :1525-1578 secondary / tertiary
    for (data_key, mod_name, out_key, debuff_key, bd_mod, label, line) in [
        (
            "durationSecondary",
            "SecondaryDuration",
            "DurationSecondary",
            "debuffSecondary",
            "SecondaryDurationMod",
            "Secondary duration:",
            1525,
        ),
        (
            "durationTertiary",
            "TertiaryDuration",
            "DurationTertiary",
            "debuffTertiary",
            "TertiaryDurationMod",
            "Tertiary duration:",
            1552,
        ),
    ] {
        let base = off.sd_num(data_key, line).unwrap_or(0.0) + q.base(c, &["Duration", mod_name]);
        if base > 0.0 {
            let m = max2(q.calc_mod(c, &with_mine(vec!["Duration", mod_name])), 0.0);
            let mut d = base * m;
            let debuff = off.sd_truthy(debuff_key);
            if debuff {
                d *= debuff_mult;
            }
            let d = to_tick(d);
            off.out().set_num(out_key, d);
            // :1534-1550 / :1561-1577 (the duration modifier row tests the primary `output.DurationMod`)
            if off.breakdown && d != base {
                let mut t = crate::breakdown::mod_(&q.list, c, &with_mine(vec!["Duration", mod_name]));
                if let Some(t) = t.as_mut() {
                    t.insert(1, label);
                }
                off.bd_set(bd_mod, t.map(Into::into));
                let mut t = BTable::lines([sf!("%.2fs ^8(base)", base)]);
                if duration_mod != 1.0 {
                    t.push(sf!("x %.4f ^8(duration modifier)", m));
                }
                if debuff && debuff_mult != 1.0 {
                    t.push(sf!("/ %.3f ^8(debuff expires slower/faster)", 1.0 / debuff_mult));
                }
                t.push(sf!("rounded up to nearest server tick"));
                t.push(sf!("= %.3fs", d));
                off.bd_put(out_key, t);
            }
        }
    }
    // :1579-1608 aura / reserve duration
    for (data_key, out_key, line) in [("auraDuration", "AuraDuration", 1579), ("reserveDuration", "ReserveDuration", 1594)] {
        let base = off.sd_num(data_key, line).unwrap_or(0.0);
        if base > 0.0 {
            let m = max2(q.calc_mod(c, &["Duration"]), 0.0);
            let d = to_tick(base * m);
            off.out().set_num(out_key, d);
            if off.breakdown && d != base {
                let t = BTable::lines([
                    sf!("%.2fs ^8(base)", base),
                    sf!("x %.4f ^8(duration modifier)", m),
                    "rounded up to nearest server tick".to_owned(),
                    sf!("= %.3fs", d),
                ]);
                off.bd_put(out_key, t);
            }
        }
    }
    // :1609-1623
    let base = off.sd_num("soulPreventionDuration", 1609).unwrap_or(0.0);
    if base > 0.0 {
        let mut names = vec!["SoulGainPreventionDuration"];
        if off.sd_truthy("skillEffectAppliesToSoulGainPrevention") {
            names.push("Duration");
        }
        let m = max2(q.calc_mod(c, &with_mine(names)), 0.0);
        let d = max2(((base * m) * tick()).ceil(), 1.0) / tick();
        off.out().set_num("SoulGainPreventionDuration", d);
        if off.breakdown && d != base {
            let t = BTable::lines([
                sf!("%.2fs ^8(base)", base),
                sf!("x %.4f ^8(duration modifier)", m),
                sf!("rounded up to nearest server tick"),
                sf!("= %.3fs", d),
            ]);
            off.bd_put("SoulGainPreventionDuration", t);
        }
    }
    // :1624-1627
    let totem_mod = max2(q.calc_mod(c, &["TotemDuration"]), 0.0);
    let totem_base = q.base(c, &["TotemDuration"]);
    let totem_dur = to_tick(totem_base * totem_mod);
    off.out().set_num("TotemDurationMod", totem_mod);
    off.out().set_num("TotemDuration", totem_dur);
    // :1628-1638
    if off.breakdown {
        off.bd_set("TotemDurationMod", bmod(&q.list, c, &["TotemDuration"]));
        let mut t = BTable::lines([sf!("%.2fs ^8(base)", totem_base)]);
        if totem_mod != 1.0 {
            t.push(sf!("x %.4f ^8(duration modifier)", totem_mod));
        }
        t.push(sf!("rounded up to nearest server tick"));
        t.push(sf!("= %.3fs", totem_dur));
        off.bd_put("TotemDuration", t);
    }
    // :1639-1642
    let impale_mod = max2(q.calc_mod(c, &["ImpaleDuration"]), 0.0);
    let impale_base = cc("impaled_debuff_base_duration_ms") / 1000.0;
    let impale_dur = (impale_base * impale_mod * tick() * debuff_mult).ceil() / tick();
    off.out().set_num("ImpaleDurationMod", impale_mod);
    off.out().set_num("ImpaleDuration", impale_dur);
    // :1643-1656
    if off.breakdown {
        off.bd_set("ImpaleDurationMod", bmod(&q.list, c, &["ImpaleDuration"]));
        let mut t = BTable::lines([sf!("%.2fs ^8(base)", impale_base)]);
        if debuff_mult != 1.0 {
            t.push(sf!("/ %.3f ^8(debuff expires slower/faster)", 1.0 / debuff_mult));
        }
        if impale_mod != 1.0 {
            t.push(sf!("x %.4f ^8(duration modifier)", impale_mod));
        }
        t.push(sf!("rounded up to nearest server tick"));
        t.push(sf!("= %.3fs", impale_dur));
        off.bd_put("ImpaleDuration", t);
    }
}

/// :1659-1690 skill uptime.
fn uptime(off: &mut Off<'_>, q: &Q, cfg: &ModCfg) {
    if off.has_type("Vaal") {
        return;
    }
    let cooldown = off.st.player.output.opt_num("Cooldown").unwrap_or(0.0);
    for dt in [
        "Duration",
        "DurationSecondary",
        "DurationTertiary",
        "AuraDuration",
        "reserveDuration",
    ] {
        let duration = off.st.player.output.opt_num(dt).unwrap_or(0.0);
        if duration != 0.0 && cooldown != 0.0 {
            let no_cdr = q.flag(Some(cfg), &["NoCooldownRecoveryInDuration"]);
            let up = if no_cdr {
                duration / (cooldown + duration)
            } else {
                duration / cooldown
            };
            let v = min2(up, 1.0) * 100.0;
            off.out().set_num(&format!("{dt}Uptime"), v);
            // :1674-1686
            if off.breakdown {
                let first = if q.flag(Some(cfg), &["NoCooldownRecoveryInDuration"]) {
                    sf!("%.2fs / (%.2fs + %.2fs)", duration, cooldown, duration)
                } else {
                    sf!("%.2fs / %.2fs", duration, cooldown)
                };
                off.bd_put(&format!("{dt}Uptime"), BTable::lines([first, sf!("= %d%%", v)]));
            }
        }
    }
}

/// One `costs[resource]` table (:1696-1708).
#[derive(Clone)]
struct Cost {
    name: &'static str,
    ty: &'static str,
    upfront: bool,
    percent: bool,
    unaffected: bool,
    text: &'static str,
    base_cost: f64,
    /// `baseCostRaw` -- only Mana's table has the field.
    base_cost_raw: Option<f64>,
    total_cost: f64,
    base_cost_no_mult: f64,
    final_base_cost: f64,
}

const fn cost(name: &'static str, ty: &'static str, upfront: bool, percent: bool, text: &'static str) -> Cost {
    Cost {
        name,
        ty,
        upfront,
        percent,
        unaffected: false,
        text,
        base_cost: 0.0,
        base_cost_raw: None,
        total_cost: 0.0,
        base_cost_no_mult: 0.0,
        final_base_cost: 0.0,
    }
}

/// :1692-1896 costs (with the `<Resource>Cost` breakdown, R8.5).
#[allow(clippy::too_many_lines)]
fn costs(off: &mut Off<'_>, q: &Q, cfg: &ModCfg) {
    let c = Some(cfg);
    let mut costs = vec![
        Cost {
            base_cost_raw: Some(0.0),
            ..cost("Mana", "Mana", true, false, "mana")
        },
        cost("Life", "Life", true, false, "life"),
        cost("ES", "ES", true, false, "ES"),
        Cost {
            unaffected: true,
            ..cost("Soul", "Soul", true, false, "soul")
        },
        cost("Rage", "Rage", true, false, "rage"),
        cost("ManaPercent", "Mana", true, true, "mana"),
        cost("LifePercent", "Life", true, true, "life"),
        cost("ManaPerMinute", "Mana", false, false, "mana/s"),
        cost("LifePerMinute", "Life", false, false, "life/s"),
        cost("ManaPercentPerMinute", "Mana", false, true, "mana/s"),
        cost("LifePercentPerMinute", "Life", false, true, "life/s"),
        cost("ESPerMinute", "ES", false, false, "ES/s"),
        cost("ESPercentPerMinute", "ES", false, true, "ES/s"),
    ];
    let idx = |costs: &[Cost], name: &str| costs.iter().position(|x| x.name == name).expect("costs entry");
    if q.flag(c, &["HasNoCost"]) {
        // :1711 the whole block is skipped; Eldritch Battery follows
        return;
    }
    let mut mult = floor(q.more(c, &["SupportManaMultiplier"]), Some(4.0));
    let additional_life_cost = q.base(c, &["ManaCostAsLifeCost"]) / 100.0;
    let additional_es_cost = q.base(c, &["ManaCostAsEnergyShieldCost"]) / 100.0;
    let hybrid_life_cost = q.base(c, &["HybridManaAndLifeCost_Life"]) / 100.0;
    // `activeSkill.activeEffect.grantedEffectLevel` (the level table; ProcessSocketGroup may have replaced its `cost` with `{}`)
    let (level_cost, level_flat, level_pct): (Option<IndexMap<String, f64>>, Option<f64>, Option<f64>) = {
        let st = off.env.skills.as_ref().expect("skills on");
        let e = off.sk().active_effect.borrow();
        let ge = &st.skills.effects[e.granted_effect];
        let lk = e
            .granted_effect_level
            .unwrap_or_else(|| panic!("{LUA}:1720: attempt to index field 'grantedEffectLevel' (a nil value)"));
        let lvl = ge
            .level(lk)
            .unwrap_or_else(|| panic!("{LUA}:1720: attempt to index field 'grantedEffectLevel' (a nil value)"));
        let wiped = st.cost_wiped.iter().any(|(id, l)| *id == ge.id && *l == lk);
        let cost_tbl = if wiped {
            Some(IndexMap::new())
        } else {
            lvl.cost.as_ref().map(|m| m.0.clone())
        };
        (cost_tbl, lvl.mana_reservation_flat, lvl.mana_reservation_percent)
    };
    let not_es_instead = !q.flag(c, &["CostESInsteadOfManaOrLife"]);
    let life_instead = q.flag(c, &["CostLifeInsteadOfMana"]);
    for k in 0..costs.len() {
        let resource = costs[k].name;
        let skill_cost = q
            .override_num(c, &[&format!("Base{resource}CostOverride")], "Modules/CalcOffence.lua:1720")
            .or_else(|| level_cost.as_ref().and_then(|m| m.get(resource).copied()));
        let mut base_cost = round(skill_cost.map_or(0.0, |s| s / cost_divisor(resource)), Some(2.0));
        let base_cost_no_mult = q.base(c, &[&format!("{resource}CostNoMult")]);
        let mut divine_correction = 0.0;
        if costs[k].upfront {
            base_cost += q.base(c, &[&format!("{resource}CostBase")]);
            costs[k].total_cost = q.base(c, &[&format!("{resource}Cost"), "Cost"]);
            if resource == "Mana" && off.has_type("ReservationBecomesCost") && !costs[k].percent && not_es_instead && !life_instead {
                let text = costs[k].text;
                let reserved_flat = off.sd_num(&format!("{text}ReservationFlat"), 1728).or(level_flat).unwrap_or(0.0);
                base_cost += reserved_flat;
                let reserved_pct = off.sd_num(&format!("{text}ReservationPercent"), 1730).or(level_pct).unwrap_or(0.0);
                let pool = off.st.player.output.opt_num(resource).unwrap_or(0.0);
                base_cost += (pool * reserved_pct / 100.0).floor();
                divine_correction = round(pool * reserved_pct / 100.0 * mult, None) - (pool * reserved_pct / 100.0 * mult).floor();
            }
        }
        let v = &mut costs[k];
        v.base_cost += base_cost;
        v.base_cost_no_mult += base_cost_no_mult;
        v.final_base_cost = ((v.base_cost * mult).floor() + v.base_cost_no_mult) + divine_correction;
        v.base_cost_raw = v
            .base_cost_raw
            .map(|_| v.base_cost * mult + v.base_cost_no_mult + divine_correction);
        let ty = v.ty;
        let upfront = v.upfront;
        if ty == "Life" {
            let mana_type = resource.replacen("Life", "Mana", 1);
            let mi = idx(&costs, &mana_type);
            if life_instead {
                if upfront {
                    let mana_total = q.base(c, &[&format!("{mana_type}Cost")]);
                    costs[k].total_cost += max2(0.0, mana_total);
                    costs[mi].total_cost -= mana_total;
                }
                let m = costs[mi].clone();
                costs[k].base_cost += m.base_cost;
                costs[k].base_cost_no_mult += m.base_cost_no_mult;
                costs[k].final_base_cost += m.final_base_cost;
                let mm = &mut costs[mi];
                mm.base_cost = 0.0;
                mm.base_cost_raw = Some(0.0);
                mm.final_base_cost = 0.0;
                mm.base_cost_no_mult = 0.0;
            } else if (additional_life_cost > 0.0 || hybrid_life_cost > 0.0) && not_es_instead {
                let m = costs[mi].clone();
                costs[k].base_cost = m.base_cost;
                costs[k].final_base_cost += round(m.final_base_cost * (hybrid_life_cost + additional_life_cost), None);
                if upfront && hybrid_life_cost > 0.0 {
                    let mana_total = q.base(c, &[&format!("{mana_type}Cost")]);
                    costs[k].total_cost += round(max2(0.0, mana_total) * hybrid_life_cost, None);
                }
            }
        } else if ty == "ES" {
            let mana_type = resource.replacen("ES", "Mana", 1);
            let life_type = resource.replacen("ES", "Life", 1);
            let (mi, li) = (idx(&costs, &mana_type), idx(&costs, &life_type));
            if !not_es_instead {
                if upfront {
                    let mana_total = q.base(c, &[&format!("{mana_type}Cost")]);
                    let life_total = q.base(c, &[&format!("{life_type}Cost")]);
                    costs[k].total_cost += max2(0.0, mana_total) + max2(0.0, life_total);
                    costs[mi].total_cost -= mana_total;
                    costs[li].total_cost -= life_total;
                }
                let (m, l) = (costs[mi].clone(), costs[li].clone());
                costs[k].base_cost += m.base_cost + l.base_cost;
                costs[k].base_cost_no_mult += m.base_cost_no_mult + l.base_cost_no_mult;
                costs[k].final_base_cost += m.final_base_cost + l.final_base_cost;
                for j in [mi, li] {
                    let x = &mut costs[j];
                    x.base_cost = 0.0;
                    x.base_cost_raw = x.base_cost_raw.map(|_| 0.0).or(Some(0.0));
                    x.final_base_cost = 0.0;
                    x.base_cost_no_mult = 0.0;
                }
            } else if additional_es_cost > 0.0 {
                let m = costs[mi].clone();
                costs[k].base_cost = m.base_cost;
                costs[k].final_base_cost += round(m.final_base_cost * additional_es_cost, None);
            }
        } else if ty == "Rage" && q.flag(c, &["CostRageInsteadOfSouls"]) {
            let si = idx(&costs, "Soul");
            let s = costs[si].clone();
            costs[k].base_cost = s.base_cost;
            costs[k].base_cost_no_mult += s.base_cost_no_mult;
            costs[k].final_base_cost = s.base_cost;
            mult = 1.0;
            let x = &mut costs[si];
            x.base_cost = 0.0;
            x.base_cost_no_mult = 0.0;
            x.final_base_cost = 0.0;
        }
    }
    // :1801-1895
    for v in &costs {
        let resource = if v.upfront {
            v.name.to_owned()
        } else {
            v.name.replacen("Minute", "Second", 1)
        };
        let has_cost = v.base_cost > 0.0 || v.total_cost > 0.0 || v.base_cost_no_mult > 0.0 || v.final_base_cost > 0.0;
        off.out().set_bool(&format!("{resource}HasCost"), has_cost);
        let cost_name = format!("{resource}Cost");
        let raw_name = format!("{cost_name}Raw");
        let efficiency = q.calc_mod(c, &[&format!("{}CostEfficiency", v.ty), "CostEfficiency"]);
        let type_cost = format!("{}Cost", v.ty);
        // `moreType` / `moreCost` / `inc` as the breakdown below reads them (1 / 1 / 0 unless assigned)
        let (bd_more_type, bd_more_cost, bd_inc);
        let (value, raw) = if !v.unaffected {
            let more_type = q.more(c, &[&type_cost]);
            let more_cost = q.more(c, &["Cost"]);
            let inc = q.inc(c, &[&type_cost, "Cost"]);
            (bd_more_type, bd_more_cost, bd_inc) = (more_type, more_cost, inc);
            let mut raw = v.base_cost_raw.map(|r| {
                max2(
                    0.0,
                    max2(0.0, (1.0 + inc / 100.0) * r * more_type * more_cost / efficiency) + v.total_cost,
                )
            });
            let mut x = v.final_base_cost;
            x = if inc < 0.0 {
                max2(0.0, ((1.0 + inc / 100.0) * x).ceil())
            } else {
                max2(0.0, ((1.0 + inc / 100.0) * x).floor())
            };
            x = if more_type < 1.0 {
                max2(0.0, (more_type * x).ceil())
            } else {
                max2(0.0, (more_type * x).floor())
            };
            x = if more_cost < 1.0 {
                max2(0.0, (more_cost * x).ceil())
            } else {
                max2(0.0, (more_cost * x).floor())
            };
            x = max2(0.0, x / efficiency);
            if v.upfront && !v.percent {
                x = round(x, None);
            }
            x = max2(0.0, x + v.total_cost);
            if v.ty == "Mana" && hybrid_life_cost > 0.0 {
                x = max2(0.0, ((1.0 - hybrid_life_cost) * x).floor());
                raw = raw.map(|r| max2(0.0, (1.0 - hybrid_life_cost) * r));
            }
            (x, raw)
        } else {
            let more_type = q.more(c, &[&type_cost]);
            let inc = q.inc(c, &[&type_cost]);
            (bd_more_type, bd_more_cost, bd_inc) = (more_type, 1.0, inc);
            let mut x = (v.base_cost + v.base_cost_no_mult).floor();
            x = max2(0.0, (1.0 + inc / 100.0) * x);
            x = max2(0.0, more_type * x);
            x = max2(0.0, x / efficiency);
            if v.upfront && !v.percent {
                x = round(x, None);
            }
            x = max2(0.0, x + v.total_cost);
            let raw = v.base_cost_raw.map(|r| {
                max2(
                    0.0,
                    max2(0.0, (1.0 + inc / 100.0) * (r + v.base_cost_no_mult) * more_type / efficiency) + v.total_cost,
                )
            });
            (x, raw)
        };
        // upstream assigns `output[costName]` first (:1813), then `output[costNameRaw]` (:1817); the final value lands in the same key
        off.out().set_num(&cost_name, value);
        match raw {
            Some(r) => off.out().set_num(&raw_name, r),
            None => off.out().remove(&raw_name),
        }
        // :1859-1894
        if off.breakdown && has_cost {
            let pct = if v.percent { "%%" } else { "" };
            let text = v.text;
            let mut t = BTable::lines([crate::breakdown::format(
                &format!("%.2f{pct} ^8(base {text} cost)"),
                &[pob_lua::Arg::Num(v.base_cost)],
            )]);
            if mult != 1.0 {
                t.push(sf!("x %.4f ^8(cost multiplier)", mult));
            }
            if v.base_cost_no_mult != 0.0 {
                t.push(crate::breakdown::format(
                    &format!("+ %d ^8(additional {text} cost)"),
                    &[pob_lua::Arg::Num(v.base_cost_no_mult)],
                ));
            }
            if v.ty == "Life" && (hybrid_life_cost + additional_life_cost) != 0.0 && !q.flag(c, &["CostLifeInsteadOfMana"]) {
                t.push(sf!("x %.2f ^8(mana cost conversion)", hybrid_life_cost + additional_life_cost));
            }
            if v.ty == "ES"
                && additional_es_cost != 0.0
                && !q.flag(c, &["CostLifeInsteadOfMana"])
                && !q.flag(c, &["CostESInsteadOfManaOrLife"])
            {
                t.push(sf!("x %.2f ^8(mana cost conversion)", additional_es_cost));
            }
            if bd_inc != 0.0 {
                t.push(crate::breakdown::format(
                    &format!("x %.2f ^8(increased/reduced {text} cost)"),
                    &[pob_lua::Arg::Num(1.0 + bd_inc / 100.0)],
                ));
            }
            if bd_more_cost != 1.0 {
                t.push(sf!("x %.2f ^8(more/less cost)", bd_more_cost));
            }
            if bd_more_type != 1.0 {
                t.push(crate::breakdown::format(
                    &format!("x %.2f ^8(more/less {text} cost)"),
                    &[pob_lua::Arg::Num(bd_more_type)],
                ));
            }
            if efficiency != 1.0 {
                t.push(crate::breakdown::format(
                    &format!("/ %.2f ^8({text} cost efficiency)"),
                    &[pob_lua::Arg::Num(efficiency)],
                ));
            }
            if v.total_cost != 0.0 {
                t.push(crate::breakdown::format(
                    &format!("%+d ^8(total {text} cost)"),
                    &[pob_lua::Arg::Num(v.total_cost)],
                ));
            }
            if v.ty == "Mana" && hybrid_life_cost > 0.0 {
                t.push(sf!(
                    "x %.2f ^8(%d%% paid for with life)",
                    1.0 - hybrid_life_cost,
                    hybrid_life_cost * 100.0
                ));
            }
            let fmt = format!("= %{}{pct}", if v.upfront { "d" } else { ".2f" });
            t.push(crate::breakdown::format(&fmt, &[pob_lua::Arg::Num(value)]));
            off.bd_put(&cost_name, t);
        }
    }
}
