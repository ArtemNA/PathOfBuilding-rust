//! CalcOffence.lua:4155-5648 (R7.6, part D, first half): the ailment pass loop -- the skill flags of the ailments (:4155-4162), per pass
//! the Perfect Agony mods (:4173-4193), the chances to inflict the secondary effects (:4195-4303), `calcAilmentDamage` (:4333-4410),
//! bleed (:4412-4683), poison (:4685-4988) and ignite (:4990-5330); the rest of the loop body (non-damaging ailments, knockback, stun,
//! impale) and everything after the loop is [`super::d2`].
//!
//! `output` inside the loop is `pass.output`, `globalOutput` the top-level `output`, `cfg` the pass cfg, `skillCfg` = [`Off::cfg`].
//! The ailment cfgs (`activeSkill.bleedCfg` / `poisonCfg` / `igniteCfg` and their `OH` twins) are read nowhere else upstream (`grep`):
//! they live only as the local `dotCfg` here ([`dot_cfg`]); their `skillCond` is `setmetatable({ CriticalStrike = true }, { __index =
//! function(_, k) return skillCfg.skillCond[k] or cfg.skillCond[k] end })` -- a snapshot of that `or` (no other `skillCond` key is
//! written while a `dotCfg` is in use) plus the own `CriticalStrike`. Every ailment cfg lacks `conversionTable`, so
//! `calcAilmentSourceDamage` reads `activeSkill.conversionTable` (also for a hand pass).
//!
//! Hash-order walks (R13/D13): none here -- `ailmentTypeList`, `elementalAilmentTypeList` and `dmgTypeList` are arrays.
//! `^` (R2): `m_pow` of the crit chance (:4570, :5183) -- `f64::powf` (UCRT = desktop LuaJIT).
//!
//! Breakdown (R8): the `if breakdown` branches at :4602-4610, :4899-4907 and :5296-5303 rewrite `output.<Ailment>DotMulti` -- ported; the
//! rest only fills breakdown tables (their only query that no other code makes, `Flag(nil, "Condition:NonPoisonedOnly")` :4760, is
//! still made).

use pob_lua::number::{max2, min2};
use pob_mods::{ModCfg, ModType, Str, Value};

use super::Off;
use super::c::{LUA, config, dflag, dmore, dsum, num_at, or_num};
use super::c2::resist_for_type;
use super::helpers::{ConversionTable, Q, calc_ailment_source_damage};
use crate::breakdown::{BTable, Chain, multi_chain, step};
use crate::calc_lib;
use crate::setup::{keyword_flag, misc, mod_flag, ok, tag};
use crate::sf;
use pob_lua::tostring_num;

/// `dmgTypeFlags.flags.Chaos` / `.Fire` (:36-46).
const FLAG_CHAOS: u32 = 0x10;
const FLAG_FIRE: u32 = 0x08;

/// `calcLib.mod(db, cfg, ...)` on a non-skill store (enemyDB).
pub(super) fn dmod(db: &pob_mods::StoreRef, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
    ok(calc_lib::mod_(&db.borrow(), cfg, names))
}

/// The `cfg.flags` / `cfg.keywordFlags` of a cfg as the 32-bit integers `bit.*` sees.
fn flags_of(c: &ModCfg) -> (u32, u32) {
    let f = c
        .flags
        .unwrap_or_else(|| panic!("{LUA}:4420: bad argument #1 to 'band' (number expected, got nil)"));
    let k = c
        .keyword_flags
        .unwrap_or_else(|| panic!("{LUA}:4421: bad argument #1 to 'band' (number expected, got nil)"));
    (f as u32, k as u32)
}

/// The ailment `dotCfg` of pass `pi` (:4414-4426, :4687-4699, :4992-5004) with `checkWeapon1HFlags` (:4308-4310) applied. `kind` /
/// `dot` = the ailment's and its DoT's keyword flags (`Bleed`/`PhysicalDot`, `Poison`/`ChaosDot`, `Ignite`/`FireDot`).
fn dot_cfg(off: &Off<'_>, pi: usize, kind: &str, dot: &str) -> ModCfg {
    let sc = &off.cfg;
    let pc = off.pcfg(pi);
    let (cflags, ckw) = flags_of(pc);
    let mut flags = mod_flag("Dot") | mod_flag("Ailment") | (cflags & mod_flag("WeaponMask"));
    if cflags & mod_flag("Melee") != 0 {
        flags |= mod_flag("MeleeHit");
    }
    let kw = (ckw & !keyword_flag("Hit")) | keyword_flag(kind) | keyword_flag("Ailment") | keyword_flag(dot);
    // `skillCfg.skillCond[key] or cfg.skillCond[key]` for every key either has, then the own `CriticalStrike = true`
    let mut cond: indexmap::IndexMap<Str, bool> = indexmap::IndexMap::new();
    for src in [sc.skill_cond.as_ref(), pc.skill_cond.as_ref()].into_iter().flatten() {
        for (k, v) in src {
            let e = cond.entry(k.clone()).or_insert(false);
            *e = *e || *v;
        }
    }
    cond.insert(Str::from("CriticalStrike"), true);
    // checkWeapon1HFlags
    flags |= cflags & mod_flag("Weapon1H");
    ModCfg {
        flags: Some(f64::from(flags)),
        keyword_flags: Some(f64::from(kw)),
        skill_name: sc.skill_name.clone(),
        skill_part: sc.skill_part,
        skill_types: sc.skill_types.clone(),
        summon_skill_name: sc.summon_skill_name.clone(),
        slot_name: sc.slot_name.clone(),
        skill_cond: Some(cond),
        skill_dist: sc.skill_dist,
        ..ModCfg::default()
    }
}

/// `dotCfg.skillCond["CriticalStrike"] = v`.
fn set_crit(c: &mut ModCfg, v: bool) {
    c.skill_cond.get_or_insert_default().insert(Str::from("CriticalStrike"), v);
}

/// `1 + (Override(dotCfg, "DotMultiplier") or Sum("BASE", dotCfg, "DotMultiplier") + Sum("BASE", dotCfg, <type>)) / 100`.
fn dot_multi(q: &Q, c: &ModCfg, type_multi: &str, line: u32) -> f64 {
    let site = format!("{LUA}:{line}");
    let m = q
        .override_num(Some(c), &["DotMultiplier"], &site)
        .unwrap_or_else(|| q.base(Some(c), &["DotMultiplier"]) + q.base(Some(c), &[type_multi]));
    1.0 + m / 100.0
}

/// The DoT multiplier of the crit sub pass (:4520-4526 and the poison / ignite twins): with `AilmentsAreNeverFromCrit` the query runs
/// with `CriticalStrike` false and the cfg is reset to true afterwards.
fn crit_dot_multi(off: &Off<'_>, q: &Q, c: &mut ModCfg, type_multi: &str, line: u32) -> f64 {
    if dflag(&off.mod_db, None, &["AilmentsAreNeverFromCrit"]) {
        set_crit(c, false);
        let v = dot_multi(q, c, type_multi, line);
        set_crit(c, true);
        v
    } else {
        dot_multi(q, c, type_multi, line)
    }
}

/// `calcAilmentSourceDamage(activeSkill, output, dotCfg, breakdown, damageType, typeFlags)` of pass `pi` (`activeSkill.conversionTable`);
/// `bd_key` = the `pass.breakdown` entry passed as `breakdown` (nil when `None` or without a breakdown).
#[allow(clippy::too_many_arguments)]
fn source_damage(off: &mut Off<'_>, q: &Q, c: &ModCfg, pi: usize, dt: &'static str, type_flags: u32, bd_key: Option<&str>) -> (f64, f64) {
    let mut bd_t = bd_key.and_then(|k| super::bdh::take_pbd_table(off, pi, k));
    let r = {
        let conv: &ConversionTable = &off.conversion_table;
        let base = |t: &str| -> (f64, f64) {
            (
                num_at(off.pval(pi, &format!("{t}MinBase")), "MinBase", 91),
                num_at(off.pval(pi, &format!("{t}MaxBase")), "MaxBase", 92),
            )
        };
        calc_ailment_source_damage(q, conv, &base, Some(c), bd_t.as_mut(), dt, type_flags)
    };
    if let (Some(k), Some(t)) = (bd_key, bd_t) {
        off.pbd_put(pi, k, t);
    }
    r
}

/// :4333-4410 `calcAilmentDamage(type, sourceCritChance, sourceHitDmg, sourceCritDmg, hideFromBreakdown)` for pass `pi`: writes
/// `output[type.."Chance"]` (and the `<type>Chance` / `<type>DPS` breakdown rows), returns the base value.
#[allow(clippy::too_many_arguments)]
pub(super) fn calc_ailment_damage(off: &mut Off<'_>, q: &Q, pi: usize, ty: &str, source_crit: f64, hit: f64, crit: f64, hide: bool) -> f64 {
    let on_hit = off.pnum_req(pi, &format!("{ty}ChanceOnHit"), 4336);
    let on_crit = off.pnum_req(pi, &format!("{ty}ChanceOnCrit"), 4337);
    let from_hit = on_hit * (1.0 - source_crit / 100.0);
    let from_crit = on_crit * source_crit / 100.0;
    let chance = from_hit + from_crit;
    off.pset_num(pi, &format!("{ty}Chance"), chance);
    let base_from_hit = hit * from_hit / (from_hit + from_crit);
    let base_from_crit = crit * from_crit / (from_hit + from_crit);
    let base_val = base_from_hit + base_from_crit;
    let source_mult = q.more(Some(off.pcfg(pi)), &[&format!("{ty}AsThoughDealing")]);
    let label = off.passes[pi].label;
    // :4344-4362
    if off.breakdown && chance != 0.0 && !hide {
        let is_attack = off.is_attack;
        let mut lines = Vec::new();
        lines.push(sf!("Chance on Non-crit: %d%%", on_hit));
        lines.push(sf!("Chance on Crit: %d%%", on_crit));
        if on_hit != on_crit {
            lines.push("Combined chance:".to_owned());
            lines.push(sf!("%d x (1 - %.4f) ^8(chance from non-crits)", on_hit, source_crit / 100.0));
            lines.push(sf!("+ %d x %.4f ^8(chance from crits)", on_crit, source_crit / 100.0));
            let per_hit = on_hit * (1.0 - source_crit / 100.0) + on_crit * source_crit / 100.0;
            lines.push(sf!("= %.2f", per_hit));
        }
        let bd = off.pbd(pi).expect("breakdown");
        let t = bd.table_or(&format!("{ty}Chance"), BTable::new);
        if !t.is_empty() {
            t.push("");
        }
        if is_attack {
            t.push(format!("{label}:"));
        }
        for l in lines {
            t.push(l);
        }
    }
    // :4363-4408
    if off.breakdown && base_val > 0.0 && !hide {
        let is_attack = off.is_attack;
        let cc = off.pnum_req(pi, "CritChance", 4373);
        let mut lines: Vec<String> = Vec::new();
        if hit == crit || cc == 0.0 {
            lines.push("Total damage:".to_owned());
            lines.push(sf!("%.1f ^8(source damage)", hit));
            if source_mult > 1.0 {
                lines.push(sf!("x %.2f ^8(inflicting as though dealing more damage)", source_mult));
                lines.push(sf!("= %.1f", base_val * source_mult));
            }
        } else {
            if base_from_hit > 0.0 {
                lines.push("Damage from Non-crits:".to_owned());
                lines.push(sf!("%.1f ^8(source damage from non-crits)", hit));
                lines.push(sf!(
                    "x %.3f ^8(portion of instances created by non-crits)",
                    from_hit / (from_hit + from_crit)
                ));
                if source_mult == 1.0 || base_from_crit != 0.0 {
                    lines.push(sf!("= %.1f", base_from_hit));
                }
            }
            if base_from_crit > 0.0 {
                lines.push("Damage from Crits:".to_owned());
                lines.push(sf!("%.1f ^8(source damage from crits)", crit));
                lines.push(sf!(
                    "x %.3f ^8(portion of instances created by crits)",
                    from_crit / (from_hit + from_crit)
                ));
                if source_mult == 1.0 || base_from_hit != 0.0 {
                    lines.push(sf!("= %.1f", base_from_crit));
                }
            }
            if base_from_hit > 0.0 && base_from_crit > 0.0 {
                lines.push("Total damage:".to_owned());
                lines.push(sf!("%.1f + %.1f", base_from_hit, base_from_crit));
                if source_mult == 1.0 {
                    lines.push(sf!("= %.1f", base_val));
                }
            }
            if source_mult > 1.0 {
                lines.push(sf!("x %.2f ^8(inflicting as though dealing more damage)", source_mult));
                lines.push(sf!("= %.1f", base_val * source_mult));
            }
        }
        let bd = off.pbd(pi).expect("breakdown");
        let t = bd.table_or(&format!("{ty}DPS"), BTable::new);
        if !t.is_empty() {
            t.push("");
        }
        if is_attack {
            t.push(format!("{label}:"));
        }
        for l in lines {
            t.push(l);
        }
    }
    base_val
}

/// `output.<k>` of pass `pi` as a number, nil -> the Lua error.
fn pn(off: &Off<'_>, pi: usize, k: &str, line: u32) -> f64 {
    off.pnum_req(pi, k, line)
}

/// `effMult` of a DoT against the enemy's `dt` resistance (:4851-4854, :5192-5195, :5202-5205, :5225-5228): `extra` = the additional
/// "taken" name (`ElementalDamageTaken` for fire).
fn dot_eff_mult(off: &Off<'_>, c: &ModCfg, dt: &str, extra: Option<&str>) -> f64 {
    let resist = resist_for_type(off, dt, Some(c));
    let (a, b) = (format!("{dt}DamageTaken"), format!("{dt}DamageTakenOverTime"));
    let mut names = vec!["DamageTaken", "DamageTakenOverTime", a.as_str(), b.as_str()];
    if let Some(e) = extra {
        names.push(e);
    }
    let taken_inc = dsum(&off.enemy_db, ModType::Inc, Some(c), &names);
    let taken_more = dmore(&off.enemy_db, Some(c), &names);
    (1.0 - resist / 100.0) * (1.0 + taken_inc / 100.0) * taken_more
}

/// :4155-5648 the ailment pass loop (the second half of its body: [`super::d2::pass_tail`]).
pub(super) fn ailment_passes(off: &mut Off<'_>, quantity_multiplier: f64) {
    let q = off.q.clone();
    let rules = pob_data::rules::rules();
    // :4157-4162
    for a in &rules.ailment_type_list {
        off.set_flag(&a.to_lowercase(), false);
    }
    let cfg = off.cfg.clone();
    let stack = q.flag_opt(Some(&cfg), &["IgniteCanStack"]);
    set_flag_opt(off, "igniteCanStack", stack);
    let to_chaos = q.flag_opt(Some(&cfg), &["IgniteToChaos"]);
    set_flag_opt(off, "igniteToChaos", to_chaos);
    off.set_flag("impale", false);
    for pi in 0..off.passes.len() {
        perfect_agony(off, &q, pi);
        chances(off, &q, pi);
        if off.can_deal.get("Physical").copied().unwrap_or(false)
            && pn(off, pi, "BleedChanceOnHit", 4413) + pn(off, pi, "BleedChanceOnCrit", 4413) > 0.0
        {
            bleed(off, &q, pi);
        }
        if off.can_deal.get("Chaos").copied().unwrap_or(false)
            && pn(off, pi, "PoisonChanceOnHit", 4686) + pn(off, pi, "PoisonChanceOnCrit", 4686) + pn(off, pi, "ChaosPoisonChance", 4686)
                > 0.0
        {
            poison(off, &q, pi, quantity_multiplier);
        }
        if off.can_deal.get("Fire").copied().unwrap_or(false)
            && pn(off, pi, "IgniteChanceOnHit", 4991) + pn(off, pi, "IgniteChanceOnCrit", 4991) > 0.0
        {
            ignite(off, &q, pi);
        }
        super::d2::pass_tail(off, &q, pi);
    }
}

/// `skillFlags[k] = <Flag result>` (nil removes the key).
fn set_flag_opt(off: &mut Off<'_>, k: &str, v: Option<bool>) {
    let f = &mut off.sk_mut().skill_flags;
    match v {
        Some(b) => {
            f.insert(k.to_owned(), b);
        }
        None => {
            f.shift_remove(k);
        }
    }
}

/// :4173-4193 Perfect Agony.
fn perfect_agony(off: &mut Off<'_>, q: &Q, pi: usize) {
    let crit_cond = || tag(&[("type", Value::str("Condition")), ("var", Value::str("CriticalStrike"))]);
    let hand_condition = match off.passes[pi].label {
        "Off Hand" => Some(tag(&[("type", Value::str("Condition")), ("var", Value::str("OffHandAttack"))])),
        "Main Hand" => Some(tag(&[("type", Value::str("Condition")), ("var", Value::str("MainHandAttack"))])),
        _ => None,
    };
    let c = off.pcfg(pi).clone();
    let ailment = mod_flag("Ailment");
    if q.base(None, &["CritMultiplierAppliesToDegen"]) > 0.0 {
        for e in q.tabulate(Some(ModType::Base), Some(&c), &["CritMultiplier"]) {
            let m = &e.mod_;
            if m.source.as_deref() == Some("Base") {
                continue;
            }
            let v = m.value.as_num().unwrap_or_else(|| {
                panic!(
                    "{LUA}:4180: bad argument #1 to 'floor' (number expected, got {})",
                    m.value.type_name()
                )
            });
            let mut tags = vec![crit_cond()];
            match &hand_condition {
                Some(h) => {
                    tags.push(h.clone());
                    tags.extend(m.tags.iter().cloned());
                }
                None if !m.tags.is_empty() => {
                    // `NewMod(..., cond, nil, unpack(mod))`: the hole in the tag array -- what ModStore's tag walks see after it is
                    // LuaJIT's table border, not modelled
                    off.note(format!(
                        "{LUA}:4180 Perfect Agony (legacy) mod with a nil hand condition before {} tag(s) of {}",
                        m.tags.len(),
                        m.name
                    ));
                }
                None => {}
            }
            q.new_mod(
                "DotMultiplier",
                Some(ModType::Base),
                Value::Num((v / 2.0).floor()),
                m.source.as_deref(),
                ailment,
                0,
                tags,
            );
        }
    }
    if q.flag(None, &["DotMultiplierIsCritMultiplier"]) {
        let site = format!("{LUA}:4187");
        let multi_override = q.override_num(Some(&c), &["CritMultiplier"], &site).map(|m| m - 100.0);
        let v = multi_override.unwrap_or_else(|| q.base(Some(&c), &["CritMultiplier"]))
            + dsum(&off.enemy_db, ModType::Base, Some(&c), &["SelfCritMultiplier"]);
        let mut tags = vec![crit_cond()];
        if let Some(h) = hand_condition {
            tags.push(h);
        }
        q.new_mod(
            "DotMultiplier",
            Some(ModType::Override),
            Value::Num(v),
            Some("Perfect Agony"),
            ailment,
            0,
            tags,
        );
    }
}

/// :4195-4303 the chances to inflict the secondary effects.
#[allow(clippy::too_many_lines)]
fn chances(off: &mut Off<'_>, q: &Q, pi: usize) {
    let edb = off.enemy_db.clone();
    let eff = off.env.mode_effective;
    let (attack, hit) = (off.flag("attack"), off.flag("hit"));
    // :4196-4217 on crit
    off.set_pass_cond(pi, "CriticalStrike", true);
    let c = off.pcfg(pi).clone();
    let c = Some(&c);
    let bleed_on_crit = if !attack || q.flag(c, &["CannotBleed"]) {
        0.0
    } else {
        min2(
            100.0,
            q.base(c, &["BleedChance"]) + dsum(&edb, ModType::Base, None, &["SelfBleedChance"]),
        )
    };
    off.pset_num(pi, "BleedChanceOnCrit", bleed_on_crit);
    let poison_on_crit = if !hit || q.flag(c, &["CannotPoison"]) {
        0.0
    } else {
        min2(
            100.0,
            q.base(c, &["PoisonChance"]) + dsum(&edb, ModType::Base, None, &["SelfPoisonChance"]),
        )
    };
    off.pset_num(pi, "PoisonChanceOnCrit", poison_on_crit);
    let impale_on_crit = if !hit {
        0.0
    } else if eff {
        min2(100.0, q.base(c, &["ImpaleChance"]))
    } else {
        0.0
    };
    off.pset_num(pi, "ImpaleChanceOnCrit", impale_on_crit);
    let kb_on_crit = if !hit || q.flag(c, &["CannotKnockback"]) {
        0.0
    } else {
        q.base(c, &["EnemyKnockbackChance"])
    };
    off.pset_num(pi, "KnockbackChanceOnCrit", kb_on_crit);
    // :4217-4229 on hit
    off.set_pass_cond(pi, "CriticalStrike", false);
    let c = off.pcfg(pi).clone();
    let c = Some(&c);
    let bleed_on_hit = if !attack || q.flag(c, &["CannotBleed"]) {
        0.0
    } else {
        min2(
            100.0,
            q.base(c, &["BleedChance"]) + dsum(&edb, ModType::Base, None, &["SelfBleedChance"]),
        )
    };
    off.pset_num(pi, "BleedChanceOnHit", bleed_on_hit);
    if !hit || q.flag(c, &["CannotPoison"]) {
        off.pset_num(pi, "PoisonChanceOnHit", 0.0);
        off.pset_num(pi, "ChaosPoisonChance", 0.0);
    } else {
        let v = min2(
            100.0,
            q.base(c, &["PoisonChance"]) + dsum(&edb, ModType::Base, None, &["SelfPoisonChance"]),
        );
        off.pset_num(pi, "PoisonChanceOnHit", v);
        off.pset_num(pi, "ChaosPoisonChance", min2(100.0, q.base(c, &["ChaosPoisonChance"])));
    }
    // :4231-4262 the elemental ailments
    let rules = pob_data::rules::rules();
    let ignite_can_scorch = dflag(&off.env_db, None, &["IgniteCanScorch"]);
    for ailment in &rules.elemental_ailment_type_list {
        let a = ailment.as_str();
        let mut chance = q.base(c, &[&format!("Enemy{a}Chance")]) + dsum(&edb, ModType::Base, None, &[&format!("Self{a}Chance")]);
        // `Flag(...) and 100 or 0 + Sum(...)` = `Flag(...) and 100 or (0 + Sum(...))`
        let immune_or_avoid = if dflag(&edb, None, &[&format!("{a}Immune"), "ElementalAilmentImmune"]) {
            100.0
        } else {
            0.0 + dsum(
                &edb,
                ModType::Base,
                None,
                &[&format!("Avoid{a}"), "AvoidAilments", "AvoidElementalAilments"],
            )
        };
        let avoid = 1.0 - min2(immune_or_avoid, 100.0) / 100.0;
        if a == "Chill" {
            chance = 100.0;
        }
        chance *= avoid;
        if a == "Ignite" && ignite_can_scorch {
            off.pset_num(pi, "ScorchChance", min2(100.0, chance));
            q.nm("EnemyScorchChance", ModType::Base, chance, "Ignite Chance", 0, 0, vec![]);
        }
        let (on_hit_k, on_crit_k) = (format!("{a}ChanceOnHit"), format!("{a}ChanceOnCrit"));
        if hit && !(q.flag(c, &[&format!("Cannot{a}")]) || avoid == 0.0) {
            let on_hit = min2(100.0, chance);
            off.pset_num(pi, &on_hit_k, on_hit);
            let alt = rules.non_damaging_ailment.0.get(a).is_some_and(|d| d.alt == Some(true));
            if q.flag(c, &[&format!("CritsDontAlways{a}")]) || (alt && !q.flag(c, &["CritAlwaysAltAilments"])) {
                off.pset_num(pi, &on_crit_k, on_hit);
            } else {
                off.pset_num(pi, &on_crit_k, 100.0);
            }
        } else {
            off.pset_num(pi, &on_hit_k, 0.0);
            off.pset_num(pi, &on_crit_k, 0.0);
        }
        if a == "Scorch" && ignite_can_scorch && avoid != 0.0 {
            off.pset_num(pi, "ScorchChanceOnCrit", 100.0);
        }
        let on_crit_part = if q.flag(c, &["NeverCrit"]) {
            0.0
        } else {
            pn(off, pi, &on_crit_k, 4259)
        };
        if pn(off, pi, &on_hit_k, 4259) + on_crit_part > 0.0 {
            off.set_flag(&format!("inflict{a}"), true);
        }
    }
    // :4263-4277
    let kb_on_hit = if !hit || q.flag(c, &["CannotKnockback"]) {
        0.0
    } else {
        q.base(c, &["EnemyKnockbackChance"])
    };
    off.pset_num(pi, "KnockbackChanceOnHit", kb_on_hit);
    let impale = if eff { min2(100.0, q.base(c, &["ImpaleChance"])) } else { 0.0 };
    off.pset_num(pi, "ImpaleChance", impale);
    for (k, f) in [
        ("FireExposureChance", "applyFireExposure"),
        ("ColdExposureChance", "applyColdExposure"),
        ("LightningExposureChance", "applyLightningExposure"),
    ] {
        if q.base(c, &[k]) > 0.0 {
            off.set_flag(f, true);
        }
    }
    // :4278-4287
    if eff {
        for ailment in &rules.ailment_type_list {
            let a = ailment.as_str();
            let mult = if dflag(&edb, None, &[&format!("{a}Immune")]) {
                0.0
            } else {
                1.0 - dsum(&edb, ModType::Base, None, &[&format!("Avoid{a}")]) / 100.0
            };
            for k in [format!("{a}ChanceOnHit"), format!("{a}ChanceOnCrit")] {
                let v = pn(off, pi, &k, 4281) * mult;
                off.pset_num(pi, &k, v);
            }
            if a == "Poison" {
                let v = pn(off, pi, "ChaosPoisonChance", 4284) * mult;
                off.pset_num(pi, "ChaosPoisonChance", v);
            }
        }
    }
    // :4289-4303
    if ailment_mode(off) == "CRIT" || dflag(&off.mod_db, None, &["AilmentsOnlyFromCrit"]) {
        for ailment in &rules.ailment_type_list {
            off.pset_num(pi, &format!("{ailment}ChanceOnHit"), 0.0);
        }
    }
    if dflag(&off.mod_db, None, &["AilmentsAreNeverFromCrit"]) {
        for ailment in &rules.ailment_type_list {
            let v = if dflag(&off.mod_db, None, &["AilmentsOnlyFromCrit"]) {
                0.0
            } else {
                pn(off, pi, &format!("{ailment}ChanceOnHit"), 4301)
            };
            off.pset_num(pi, &format!("{ailment}ChanceOnCrit"), v);
        }
    }
}

/// `env.configInput.ailmentMode or "AVERAGE"` (:4289) compared with a string.
pub(super) fn ailment_mode(off: &Off<'_>) -> String {
    match config(off, "ailmentMode") {
        None | Some(pob_build::ConfigValue::Bool(false)) => "AVERAGE".to_owned(),
        Some(pob_build::ConfigValue::Str(s)) => s,
        // a number / true compares unequal to every string
        Some(_) => String::new(),
    }
}

/// `globalOutput[k]` in arithmetic.
fn gn(off: &Off<'_>, k: &str, line: u32) -> f64 {
    num_at(off.gval(k), k, line)
}

/// `output.HitTime or output.Time` of pass `pi`.
fn hit_time(off: &Off<'_>, pi: usize, line: u32) -> f64 {
    match off.pval(pi, "HitTime") {
        None | Some(crate::output::OutValue::Bool(false)) => pn(off, pi, "Time", line),
        v => num_at(v, "HitTime", line),
    }
}

/// `skillData.dpsMultiplier` in arithmetic.
fn dps_multiplier(off: &Off<'_>, line: u32) -> f64 {
    off.sd_num("dpsMultiplier", line)
        .unwrap_or_else(|| panic!("{LUA}:{line}: attempt to perform arithmetic on field 'dpsMultiplier' (a nil value)"))
}

/// :4412-4683 bleed (with its breakdown rows, R8.5).
#[allow(clippy::too_many_lines)]
fn bleed(off: &mut Off<'_>, q: &Q, pi: usize) {
    let edb = off.enemy_db.clone();
    let mut dc = dot_cfg(off, pi, "Bleed", "PhysicalDot");
    let c = off.pcfg(pi).clone();
    // :4428-4430
    off.pbd_put(pi, "BleedPhysical", BTable::new().with("damageTypes", BTable::new()));
    // :4433-4441
    let config_stacks = dsum(&edb, ModType::Base, None, &["Multiplier:BleedStacks"]);
    let site = format!("{LUA}:4434");
    let max_stacks = q
        .override_num(Some(&c), &["BleedStacksMax"], &site)
        .unwrap_or_else(|| q.base(Some(&c), &["BleedStacksMax"]));
    let site = format!("{LUA}:4435");
    let override_stack_potential = if q.override_(None, &["BleedStackPotentialOverride"]).is_some_and(|v| v.truthy()) {
        Some(q.override_num(None, &["BleedStackPotentialOverride"], &site).expect("truthy") / max_stacks)
    } else {
        None
    };
    off.gset("BleedStacksMax", max_stacks);
    let site = format!("{LUA}:4437");
    let duration_base = q.override_num(Some(&dc), &["BleedDurationBase"], &site).unwrap_or_else(|| {
        if off.sd_truthy("bleedDurationIsSkillDuration") && off.sd_truthy("duration") {
            off.sd_num("duration", 4437).expect("truthy")
        } else {
            misc("BleedDurationBase")
        }
    });
    let mut dur_names = vec!["EnemyBleedDuration", "EnemyAilmentDuration", "DamagingAilmentDuration"];
    if off.sd_truthy("bleedIsSkillEffect") {
        dur_names.push("Duration");
    }
    let mut duration_mod = q.calc_mod(Some(&dc), &dur_names) * dmod(&edb, None, &["SelfBleedDuration", "SelfAilmentDuration"])
        / dmod(&edb, Some(&dc), &["BleedExpireRate"]);
    duration_mod = max2(duration_mod, 0.0);
    let rate_mod = q.calc_mod(Some(&c), &["BleedFaster"]) + dsum(&edb, ModType::Inc, None, &["SelfBleedFaster"]) / 100.0;
    let bleed_duration = duration_base * duration_mod / rate_mod * off.debuff_duration_mult;
    off.gset("BleedDuration", bleed_duration);
    // :4444-4464
    let crit_chance = pn(off, pi, "CritChance", 4444);
    let bleed_chance = pn(off, pi, "BleedChanceOnHit", 4444) / 100.0 * (1.0 - crit_chance / 100.0)
        + pn(off, pi, "BleedChanceOnCrit", 4444) / 100.0 * crit_chance / 100.0;
    let mut bleed_stacks = pn(off, pi, "HitChance", 4446) / 100.0 * bleed_chance * dps_multiplier(off, 4446);
    let hit_speed = off.g_hit_speed(4447);
    if hit_speed > 0.0 {
        bleed_stacks = bleed_stacks * bleed_duration * off.g_hit_speed(4449);
    }
    let site = format!("{LUA}:4451");
    let active_totems = match ok(off.env_db.borrow().override_(None, &["TotemsSummoned"])) {
        Some(v) if v.truthy() => v
            .as_num()
            .unwrap_or_else(|| panic!("{site}: attempt to perform arithmetic on a {} value", v.type_name())),
        _ => q.base(Some(&off.cfg.clone()), &["ActiveTotemLimit", "ActiveBallistaLimit"]),
    };
    if off.flag("totem") {
        bleed_stacks *= active_totems;
    }
    if config_stacks > 0.0 {
        bleed_stacks = config_stacks;
    }
    if bleed_stacks < 1.0 && override_stack_potential.unwrap_or(0.0) <= 1.0 {
        q.new_mod(
            "Condition:SingleBleed",
            Some(ModType::Flag),
            Value::Bool(true),
            Some("bleed"),
            0,
            0,
            vec![],
        );
    }
    let stack_potential = override_stack_potential.unwrap_or(bleed_stacks / max_stacks);
    off.gset("BleedStackPotential", stack_potential);
    // :4466-4485
    if off.breakdown {
        let mut t = BTable::lines(["Ratio of bleed stacks to maximum active bleeds:".to_owned()]);
        if let Some(o) = override_stack_potential {
            t.push(sf!("= %g ^8(stack potential override)", o));
        } else {
            t.push(sf!("%.2f ^8(chance to hit)", pn(off, pi, "HitChance", 4471) / 100.0));
            t.push(sf!("* %.2f ^8(chance to bleed)", bleed_chance));
            if off.g_hit_speed(4473) > 0.0 {
                t.push(sf!(
                    "* (%.2f / %.2f) ^8(Bleed duration / Attack Time)",
                    bleed_duration,
                    hit_time(off, pi, 4474)
                ));
            }
            let dm = dps_multiplier(off, 4476);
            if dm != 1.0 {
                t.push(sf!("* %g ^8(DPS multiplier for this skill)", dm));
            }
            if off.flag("totem") {
                t.push(sf!("* %d ^8(active number of Totems)", active_totems));
            }
            t.push(sf!("/ %d ^8(max number of stacks)", max_stacks));
            t.push(sf!("= %.2f", stack_potential));
        }
        off.bd_put("BleedStackPotential", t);
    }
    // :4488-4496
    let roll = if stack_potential > 1.0 {
        (bleed_stacks - (max_stacks - 1.0) / 2.0) / (bleed_stacks + 1.0) * 100.0
    } else {
        50.0
    };
    off.gset("BleedRollAverage", roll);
    // :4497-4511
    if off.breakdown {
        let custom = crate::breakdown::color("CUSTOM");
        let mut t = BTable::lines([
            format!("{custom}This is the average roll of a Bleed affecting the enemy if you are constantly attacking"),
            crate::breakdown::format(
                &format!(
                    "{custom}If attacking constantly, your average strongest bleed currently achieves ^7%.2f%%{custom} of its max damage"
                ),
                &[pob_lua::Arg::Num(roll)],
            ),
            String::new(),
            "Average Bleed Roll:".to_owned(),
        ]);
        if bleed_stacks >= max_stacks {
            t.push(sf!("%.2f - (%.2f - 1)/2", bleed_stacks, max_stacks));
            t.push(sf!("/ (%.2f + 1)", bleed_stacks));
            t.push(sf!("= %.2f%%", roll));
        } else {
            t.push(sf!("50%% (averaging <= %d bleeds)", max_stacks));
        }
        off.bd_put("BleedRollAverage", t);
    }
    // :4513-4536
    let (mut avg_crit, mut max_crit, mut avg_hit, mut min_hit, mut max_hit) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for sub_pass in 1..=2 {
        set_crit(&mut dc, sub_pass != 1);
        let (min, max) = source_damage(off, q, &dc, pi, "Physical", 0, (sub_pass == 1).then_some("BleedPhysical"));
        off.pset_num(pi, "BleedPhysicalMin", min);
        off.pset_num(pi, "BleedPhysicalMax", max);
        if sub_pass == 2 {
            let m = crit_dot_multi(off, q, &mut dc, "PhysicalDotMultiplier", 4522);
            off.pset_num(pi, "CritBleedDotMulti", m);
            let min_c = min * m;
            max_crit = max * m;
            avg_crit = min_c + (max_crit - min_c) * roll / 100.0;
        } else {
            let m = dot_multi(q, &dc, "PhysicalDotMultiplier", 4531);
            off.pset_num(pi, "BleedDotMulti", m);
            min_hit = min * m;
            max_hit = max * m;
            avg_hit = min_hit + (max_hit - min_hit) * roll / 100.0;
        }
    }
    // :4538-4566
    if off.breakdown {
        off.pbd_put(
            pi,
            "BleedDPS",
            BTable::lines([format!(
                "{}NOTE: Calculation uses new Weighted Avg formula for critical hits",
                crate::breakdown::color("CUSTOM")
            )]),
        );
        let (pmin, pmax) = (pn(off, pi, "BleedPhysicalMin", 4547), pn(off, pi, "BleedPhysicalMax", 4547));
        let derivation = sf!("%.0f + (%.0f - %.0f) * %.2f%%", pmin, pmax, pmin, roll);
        let note = "^8min source phys + (max source phys - min source phys) * average bleed roll".to_owned();
        let t = if avg_hit == avg_crit || pn(off, pi, "CritChance", 4544) == 0.0 {
            BTable::lines([
                "Dmg Derivation:".to_owned(),
                derivation,
                note,
                sf!("* %.2f ^8(Bleed DoT Multi)", pn(off, pi, "BleedDotMulti", 4549)),
                sf!("= %.2f", avg_hit),
            ])
        } else {
            BTable::lines([
                "Non-Crit Dmg Derivation:".to_owned(),
                derivation.clone(),
                note.clone(),
                sf!("* %.2f ^8(Bleed DoT Multi for Non-Crit)", pn(off, pi, "BleedDotMulti", 4557)),
                sf!("= %.0f", avg_hit),
                String::new(),
                "Crit Dmg Derivation:".to_owned(),
                derivation,
                note,
                sf!("* %.2f ^8(Bleed DoT Multi for Crit)", pn(off, pi, "CritBleedDotMulti", 4563)),
                sf!("= %.2f", avg_crit),
            ])
        };
        off.bd_put("BleedDPS", t);
    }
    // :4568-4575
    let base_percent = if off.sd_truthy("bleedBasePercent") {
        off.sd_num("bleedBasePercent", 4568).expect("truthy")
    } else {
        misc("BleedPercentBase")
    };
    let crit_chance = pn(off, pi, "CritChance", 4570);
    let ailment_crit_chance = 100.0 * (1.0 - (1.0 - crit_chance / 100.0).powf(max2(stack_potential, 1.0)));
    let ruthless = pn(off, pi, "RuthlessBlowAilmentEffect", 4573);
    let fist = pn(off, pi, "FistOfWarDamageEffect", 4573);
    let warcry = gn(off, "AilmentWarcryEffect", 4573);
    let base_min = calc_ailment_damage(off, q, pi, "Bleed", ailment_crit_chance, min_hit, 0.0, true) * base_percent / 100.0;
    let base_max =
        calc_ailment_damage(off, q, pi, "Bleed", 100.0, max_hit, max_crit, true) * base_percent / 100.0 * ruthless * fist * warcry;
    let average = calc_ailment_damage(off, q, pi, "Bleed", ailment_crit_chance, avg_hit, avg_crit, false);
    let base_bleed_dps = average * base_percent / 100.0 * ruthless * fist * warcry;
    if base_bleed_dps > 0.0 {
        off.set_flag("bleed", true);
        off.set_flag("duration", true);
        let mut eff_mult = 1.0;
        if off.env.mode_effective {
            let resist = min2(
                max2(0.0, dsum(&edb, ModType::Base, None, &["PhysicalDamageReduction"])),
                misc("EnemyPhysicalDamageReductionCap"),
            );
            let names = [
                "DamageTaken",
                "DamageTakenOverTime",
                "PhysicalDamageTaken",
                "PhysicalDamageTakenOverTime",
            ];
            let taken_inc = dsum(&edb, ModType::Inc, Some(&dc), &names);
            let taken_more = dmore(&edb, Some(&dc), &names);
            eff_mult = (1.0 - resist / 100.0) * (1.0 + taken_inc / 100.0) * taken_more;
            off.gset("BleedEffMult", eff_mult);
            // :4586-4588
            if off.breakdown && eff_mult != 1.0 {
                let t = crate::breakdown::eff_mult("Physical", resist, 0.0, taken_inc, eff_mult, taken_more, None, true, None, None);
                off.bd_put("BleedEffMult", t);
            }
        }
        let effect_mod = q.calc_mod(Some(&dc), &["AilmentEffect"]);
        let active = min2(bleed_stacks, max_stacks);
        let base_dps = base_bleed_dps * effect_mod * rate_mod * active * eff_mult;
        off.pset_num(pi, "BaseBleedDPS", base_dps);
        let cap = misc("DotDpsCap");
        let dps = min2(base_dps, cap);
        off.pset_num(pi, "BleedDPS", dps);
        off.gset("BleedStacks", bleed_stacks);
        let base = pn(off, pi, "BaseBleedDPS", 4601);
        off.gset("BleedDamage", base * gn(off, "BleedDuration", 4601));
        // :4602-4682 (the branch rewrites output.BleedDotMulti)
        if off.breakdown {
            rewrite_dot_multi(off, pi, "Bleed", ailment_crit_chance, 4603);
            let mut lines = vec![sf!("x %.2f ^8(bleed deals %d%% per second)", base_percent / 100.0, base_percent)];
            if effect_mod != 1.0 {
                lines.push(sf!("x %.2f ^8(ailment effect modifier)", effect_mod));
            }
            if ruthless != 1.0 {
                lines.push(sf!("x %.2f ^8(ruthless blow effect modifier)", ruthless));
            }
            if fist != 1.0 {
                lines.push(sf!("x %.2f ^8(fist of war effect modifier)", fist));
            }
            if warcry > 1.0 {
                lines.push(sf!("x %.2f ^8(combined ailment warcry effect modifier)", warcry));
            }
            lines.push(sf!("= %.1f", base_bleed_dps));
            lines.push(String::new());
            lines.push("Total Bleed DPS:".to_owned());
            ailment_dps_lines(
                &mut lines,
                "Bleed",
                AilmentDps {
                    base: base_bleed_dps,
                    effect_mod,
                    rate_mod,
                    rate_label: "damage rate modifier",
                    stacks: active,
                    stacks_label: "avg bleed stacks",
                    eff_mult,
                    uncapped: base_dps,
                    capped: dps,
                    min_uncapped: base_min * effect_mod * rate_mod * active * eff_mult,
                    max_uncapped: base_max * effect_mod * rate_mod * active * eff_mult,
                    cap,
                },
            );
            if let Some(bd) = off.pbd(pi) {
                let t = bd.table_or("BleedDPS", BTable::new);
                for l in lines {
                    t.push(l);
                }
            }
            if bleed_duration != duration_base {
                let mut t = BTable::lines([sf!("%.2fs ^8(base duration)", duration_base)]);
                if duration_mod != 1.0 {
                    t.push(sf!("x %.2f ^8(duration modifier)", duration_mod));
                }
                if rate_mod != 1.0 {
                    t.push(sf!("/ %.2f ^8(damage rate modifier)", rate_mod));
                }
                if off.debuff_duration_mult != 1.0 {
                    t.push(sf!("/ %.2f ^8(debuff expires slower/faster)", 1.0 / off.debuff_duration_mult));
                }
                t.push(sf!("= %.2fs", bleed_duration));
                off.bd_put("BleedDuration", t);
            }
        }
    }
}

/// The inputs of the shared "Total <Ailment> DPS" / "DPS Range" rows (bleed :4628-4664, poison, ignite twins).
struct AilmentDps {
    base: f64,
    effect_mod: f64,
    rate_mod: f64,
    rate_label: &'static str,
    stacks: f64,
    stacks_label: &'static str,
    eff_mult: f64,
    uncapped: f64,
    capped: f64,
    min_uncapped: f64,
    max_uncapped: f64,
    cap: f64,
}

/// :4628-4664 (bleed; the poison / ignite blocks have the same rows with their names).
fn ailment_dps_lines(lines: &mut Vec<String>, name: &str, a: AilmentDps) {
    let neg = crate::breakdown::color("NEGATIVE");
    let f = |fmt: String, args: &[f64]| {
        let args: Vec<pob_lua::Arg<'_>> = args.iter().map(|x| pob_lua::Arg::Num(*x)).collect();
        crate::breakdown::format(&fmt, &args)
    };
    if a.base != a.uncapped {
        lines.push(sf!("%.1f ^8(base damage per second)", a.base));
    }
    if a.effect_mod != 1.0 {
        lines.push(sf!("x %.2f ^8(ailment effect modifier)", a.effect_mod));
    }
    if a.rate_mod != 1.0 {
        lines.push(f(format!("x %.2f ^8({})", a.rate_label), &[a.rate_mod]));
    }
    if a.stacks != 1.0 {
        lines.push(f(format!("x %.2f ^8({})", a.stacks_label), &[a.stacks]));
    }
    if a.eff_mult != 1.0 {
        lines.push(sf!("x %.3f ^8(effective DPS modifier from enemy debuffs)", a.eff_mult));
    }
    if a.capped != a.uncapped {
        lines.push(f(format!("= %.1f ^8(Uncapped raw {name} DPS)"), &[a.uncapped]));
        lines.push(f(
            format!("^8(Raw {name} DPS is {neg}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)"),
            &[a.uncapped - a.capped, (a.uncapped - a.capped) / a.capped * 100.0],
        ));
        lines.push(f(format!("= %d ^8(Capped {name} DPS)"), &[a.capped]));
    } else {
        lines.push(sf!("= %.1f ^8per second", a.capped));
    }
    let min_capped = min2(a.min_uncapped, a.cap);
    let max_capped = min2(a.max_uncapped, a.cap);
    lines.push(f(format!("%.2f%% of Maximum {name} DPS"), &[a.capped / max_capped * 100.0]));
    lines.push(String::new());
    lines.push("DPS Range:".to_owned());
    if max_capped == a.max_uncapped && min_capped == a.min_uncapped {
        lines.push(f(format!("%.0f to %.0f ^8({name} DPS Range)"), &[a.min_uncapped, a.max_uncapped]));
    } else {
        lines.push(f(
            format!("%.0f to %.0f ^8(Uncapped {name} DPS Range)"),
            &[a.min_uncapped, a.max_uncapped],
        ));
    }
    if min_capped != a.min_uncapped {
        lines.push(f(
            format!("^8(Raw Min {name} DPS is {neg}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)"),
            &[a.min_uncapped - min_capped, (a.min_uncapped - min_capped) / min_capped * 100.0],
        ));
    }
    if max_capped != a.max_uncapped {
        lines.push(f(
            format!("^8(Raw Max {name} DPS is {neg}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)"),
            &[a.max_uncapped - max_capped, (a.max_uncapped - max_capped) / max_capped * 100.0],
        ));
    }
    if max_capped != a.max_uncapped || min_capped != a.min_uncapped {
        lines.push(f(format!("%.0f to %.0f ^8(Capped {name} DPS Range)"), &[min_capped, max_capped]));
    }
}

/// :4603-4609 / :4900-4906 / :5296-5302: `output.<A>DotMulti` becomes the hit / crit weighted multiplier when the crit one differs.
/// `crit_weight` = the crit chance the crit share uses (`ailmentCritChance` for bleed / ignite, `globalOutput.CritChance` for poison).
fn rewrite_dot_multi(off: &mut Off<'_>, pi: usize, a: &str, crit_weight: f64, line: u32) {
    let (crit_k, k) = (format!("Crit{a}DotMulti"), format!("{a}DotMulti"));
    if !off.ptruthy(pi, &crit_k) {
        return;
    }
    let crit_m = pn(off, pi, &crit_k, line);
    let m = num_at(off.pval(pi, &k), &k, line);
    if crit_m == m {
        return;
    }
    let g_crit = gn(off, "CritChance", line + 1);
    let from_hit = pn(off, pi, &format!("{a}ChanceOnHit"), line + 1) / 100.0 * (1.0 - g_crit / 100.0);
    let from_crit = pn(off, pi, &format!("{a}ChanceOnCrit"), line + 2) / 100.0 * crit_weight / 100.0;
    let total_from_hit = from_hit / (from_hit + from_crit);
    let total_from_crit = from_crit / (from_hit + from_crit);
    let t = crate::breakdown::crit_dot(m, crit_m, total_from_hit, total_from_crit);
    off.pbd_put(pi, &k, t);
    off.pset_num(pi, &k, (m * total_from_hit) + (crit_m * total_from_crit));
}

/// :4685-4988 poison (with its breakdown rows, R8.5).
#[allow(clippy::too_many_lines)]
fn poison(off: &mut Off<'_>, q: &Q, pi: usize, quantity_multiplier: f64) {
    let edb = off.enemy_db.clone();
    let mut dc = dot_cfg(off, pi, "Poison", "ChaosDot");
    let c = off.pcfg(pi).clone();
    // :4701-4707
    if off.breakdown {
        for k in ["PoisonPhysical", "PoisonLightning", "PoisonCold", "PoisonFire", "PoisonChaos"] {
            off.pbd_put(pi, k, BTable::new().with("damageTypes", BTable::new()));
        }
    }
    // :4708-4712
    let rate_mod = q.calc_mod(Some(&c), &["PoisonFaster"]) + dsum(&edb, ModType::Inc, None, &["SelfPoisonFaster"]) / 100.0;
    let site = format!("{LUA}:4709");
    let duration_base = q.override_num(Some(&dc), &["PoisonDurationBase"], &site).unwrap_or_else(|| {
        if off.sd_truthy("poisonDurationIsSkillDuration") && off.sd_truthy("duration") {
            off.sd_num("duration", 4709).expect("truthy")
        } else {
            misc("PoisonDurationBase")
        }
    });
    let mut dur_names = vec!["EnemyPoisonDuration", "EnemyAilmentDuration", "DamagingAilmentDuration"];
    if off.sd_truthy("poisonIsSkillEffect") {
        dur_names.push("Duration");
    }
    let mut duration_mod = q.calc_mod(Some(&dc), &dur_names) * dmod(&edb, None, &["SelfPoisonDuration", "SelfAilmentDuration"]);
    duration_mod = max2(duration_mod, 0.0);
    let poison_duration = duration_base * duration_mod / rate_mod * off.debuff_duration_mult;
    off.gset("PoisonDuration", poison_duration);
    // :4714-4717
    let chaos_hit_avg = or_num(off.pval(pi, "ChaosHitAverage"), 0.0, "ChaosHitAverage", 4714);
    let chaos_poison_chance = if chaos_hit_avg > 0.0 {
        pn(off, pi, "ChaosPoisonChance", 4714)
    } else {
        0.0
    };
    let on_hit = min2(100.0, pn(off, pi, "PoisonChanceOnHit", 4715) + chaos_poison_chance);
    let on_crit = min2(100.0, pn(off, pi, "PoisonChanceOnCrit", 4716) + chaos_poison_chance);
    let crit_chance = pn(off, pi, "CritChance", 4717);
    let poison_chance = on_hit / 100.0 * (1.0 - crit_chance / 100.0) + on_crit / 100.0 * crit_chance / 100.0;
    // :4720-4723
    let mut additional = 1.0;
    if !q.flag(None, &["CannotMultiplePoison"]) {
        additional =
            1.0 + min2(q.base(Some(&c), &["AdditionalPoisonChance"]) / 100.0, 1.0) + (q.base(Some(&c), &["AdditionalPoisonStacks"]));
    }
    // :4726-4746
    let stack_limit = q.min(Some(&c), &["PoisonStackLimit"]);
    let stack_multiplier = if off.sd_truthy("stackMultiplier") {
        off.sd_num("stackMultiplier", 4727).expect("truthy")
    } else {
        1.0
    };
    let mut stacks = pn(off, pi, "HitChance", 4727) / 100.0
        * poison_chance
        * additional
        * dps_multiplier(off, 4727)
        * stack_multiplier
        * quantity_multiplier;
    let mut uncapped_stacks: Option<f64> = None;
    if off.g_hit_speed(4729) > 0.0 {
        stacks = stacks * poison_duration * off.g_hit_speed(4731);
        if stack_limit.truthy() {
            let limit = stack_limit
                .as_num()
                .unwrap_or_else(|| panic!("{LUA}:4734: attempt to compare number with {}", stack_limit.type_name()));
            if limit > 0.0 && stacks > limit {
                let num_poisoning_hits = (limit / additional).ceil();
                let max_poison_stacks = num_poisoning_hits * additional;
                uncapped_stacks = Some(max2(stacks, max_poison_stacks));
                stacks = min2(stacks, max_poison_stacks);
            }
        }
    }
    let multiplier_on_enemy_zero = match config(off, "multiplierPoisonOnEnemy") {
        None | Some(pob_build::ConfigValue::Bool(false)) => true,
        Some(pob_build::ConfigValue::Num(x)) => x == 0.0,
        Some(_) => false,
    };
    if stacks < additional && multiplier_on_enemy_zero {
        q.new_mod(
            "Condition:NonPoisonedOnly",
            Some(ModType::Flag),
            Value::Bool(true),
            Some("Calculation"),
            0,
            0,
            vec![],
        );
    }
    // :4747-4772
    if off.breakdown {
        let mut t = BTable::new();
        multi_chain(
            &mut t,
            &Chain {
                base: Some(step("%.2fs ^8(poison duration)", poison_duration)),
                steps: vec![
                    step("%.2f ^8(poison chance)", poison_chance),
                    step("%.2f ^8(hit chance)", pn(off, pi, "HitChance", 4753) / 100.0),
                    step("%.2f ^8(avg. # of poisons inflicted)", additional),
                    step("%.2f ^8(hits per second)", off.g_hit_speed(4755)),
                    step(
                        "%g ^8(dps multiplier for this skill)",
                        off.sd_num("dpsMultiplier", 4756).unwrap_or(1.0),
                    ),
                    step("%g ^8(stack multiplier for this skill)", stack_multiplier),
                    step("%g ^8(quantity multiplier for this skill)", quantity_multiplier),
                ],
                total: Some(sf!("= %.2f", stacks)),
                ..Chain::default()
            },
        );
        if q.flag(None, &["Condition:NonPoisonedOnly"]) {
            t.push("Assuming 'non-Poisoned' Enemy");
            if stacks < additional {
                t.push("^8(time between hits is longer than poison duration)");
            } else {
                t.push(format!("^8(affected by poison stack limit of: {})", tostring_num(additional)));
            }
        } else if let Some(limit) = stack_limit.as_num().filter(|l| stack_limit.truthy() && stacks >= *l) {
            t.push(format!("^8(affected by poison stack limit of: {})", tostring_num(limit)));
            if let Some(u) = uncapped_stacks {
                t.push(format!("^8(uncapped poison stacks: {})", sf!("%.2f", u)));
            }
        }
        off.bd_put("PoisonStacks", t);
    }
    // :4774-4837
    let (mut hit_dmg, mut crit_dmg, mut max_crit, mut min_hit, mut max_hit) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for sub_pass in 1..=2 {
        set_crit(&mut dc, sub_pass != 1);
        let (mut total_min, mut total_max) = (0.0, 0.0);
        {
            let (min, max) = source_damage(off, q, &dc, pi, "Chaos", 0, (sub_pass == 1).then_some("PoisonChaos"));
            off.pset_num(pi, "PoisonChaosMin", min);
            off.pset_num(pi, "PoisonChaosMax", max);
            total_min += min;
            total_max += max;
        }
        let mut non_chaos_mult = 1.0;
        if pn(off, pi, "ChaosPoisonChance", 4786) > 0.0 && pn(off, pi, "PoisonChaosMax", 4786) > 0.0 {
            let k = if sub_pass == 2 { "PoisonChanceOnCrit" } else { "PoisonChanceOnHit" };
            let cur = pn(off, pi, k, 4789);
            let chaos_chance = min2(100.0, cur + pn(off, pi, "ChaosPoisonChance", 4789));
            non_chaos_mult = cur / chaos_chance;
            off.pset_num(pi, k, chaos_chance);
        }
        for (dt, flag, key) in [
            ("Lightning", "LightningCanPoison", "PoisonLightning"),
            ("Cold", "ColdCanPoison", "PoisonCold"),
            ("Fire", "FireCanPoison", "PoisonFire"),
            ("Physical", "", "PoisonPhysical"),
        ] {
            let can = off.can_deal.get(dt).copied().unwrap_or(false) && (flag.is_empty() || q.flag(Some(&c), &[flag]));
            if can {
                let (min, max) = source_damage(off, q, &dc, pi, dt, FLAG_CHAOS, (sub_pass == 1).then_some(key));
                off.pset_num(pi, &format!("{key}Min"), min);
                off.pset_num(pi, &format!("{key}Max"), max);
                total_min += min * non_chaos_mult;
                total_max += max * non_chaos_mult;
            }
        }
        if sub_pass == 2 {
            let m = crit_dot_multi(off, q, &mut dc, "ChaosDotMultiplier", 4824);
            off.pset_num(pi, "CritPoisonDotMulti", m);
            crit_dmg = (total_min + total_max) / 2.0 * m;
            max_crit = total_max * m;
        } else {
            let m = dot_multi(q, &dc, "ChaosDotMultiplier", 4832);
            off.pset_num(pi, "PoisonDotMulti", m);
            hit_dmg = (total_min + total_max) / 2.0 * m;
            min_hit = total_min * m;
            max_hit = total_max * m;
        }
    }
    // :4843-4846
    let percent = misc("PoisonPercentBase");
    let crit_chance = pn(off, pi, "CritChance", 4843);
    let ruthless = pn(off, pi, "RuthlessBlowAilmentEffect", 4844);
    let fist = pn(off, pi, "FistOfWarDamageEffect", 4844);
    let warcry = gn(off, "AilmentWarcryEffect", 4844);
    // :4838-4842
    if off.breakdown {
        let mode = if ailment_mode(off) == "CRIT" {
            "Crits Only"
        } else {
            "Average Damage"
        };
        off.bd_put(
            "PoisonDPS",
            BTable::lines([sf!("Ailment mode: %s ^8(can be changed in the Configuration tab)", mode)]),
        );
    }
    let base_min = calc_ailment_damage(off, q, pi, "Poison", crit_chance, min_hit, 0.0, true) * percent;
    let base_max = calc_ailment_damage(off, q, pi, "Poison", 100.0, max_hit, max_crit, true) * percent * ruthless * fist * warcry;
    let base_val = calc_ailment_damage(off, q, pi, "Poison", crit_chance, hit_dmg, crit_dmg, false) * percent * ruthless * fist * warcry;
    if base_val > 0.0 {
        off.set_flag("poison", true);
        off.set_flag("duration", true);
        let mut eff_mult = 1.0;
        if off.env.mode_effective {
            eff_mult = dot_eff_mult(off, &dc, "Chaos", None);
            off.gset("PoisonEffMult", eff_mult);
            // :4855-4859
            if off.breakdown && eff_mult != 1.0 {
                let t = dot_eff_mult_breakdown(off, &dc, "Chaos", None, eff_mult);
                off.bd_put("PoisonEffMult", t);
            }
        }
        if q.flag(None, &["Condition:NonPoisonedOnly"]) {
            stacks = min2(additional, stacks);
        }
        off.gset("PoisonStacks", stacks);
        let effect_mod = q.calc_mod(Some(&dc), &["AilmentEffect"]);
        let cap = misc("DotDpsCap");
        let single_uncapped = min2(base_val * effect_mod * rate_mod * eff_mult, cap);
        let single_capped = min2(single_uncapped, cap);
        off.pset_num(pi, "PoisonDPS", single_capped);
        off.pset_num(pi, "PoisonDamage", single_capped * gn(off, "PoisonDuration", 4869));
        let ground_mult = max2(
            q.max(None, &["PoisonDpsAsCausticGround"]).unwrap_or(0.0),
            ok(edb.borrow().max(None, &["PoisonDpsAsCausticGround"])).unwrap_or(0.0),
        );
        if ground_mult > 0.0 {
            let uncapped = base_val * effect_mod * rate_mod * eff_mult * ground_mult / 100.0;
            let ground = min2(uncapped, cap);
            off.pset_num(pi, "CausticGroundDPS", ground);
            off.out().set_bool("CausticGroundFromPoison", true);
            // :4877-4884
            if off.breakdown {
                let t = BTable::lines([
                    sf!("%.1f ^8(single poison damage per second)", base_val * effect_mod * rate_mod),
                    sf!("* %.1f%% ^8(percent as Caustic ground)", ground_mult),
                    sf!("* %.3f ^8(effect mult)", eff_mult),
                    sf!("= %.1f ^8per second", ground),
                ]);
                off.pbd_put(pi, "CausticGroundDPS", t);
            }
        }
        let (mut total_uncapped, mut total_capped) = (0.0, 0.0);
        if off.sd_truthy("showAverage") {
            let v = off.pval(pi, "PoisonDamage").cloned();
            off.pset_val(pi, "TotalPoisonAverageDamage", v);
            let v = off.pval(pi, "PoisonDPS").cloned();
            off.pset_val(pi, "TotalPoisonDPS", v);
        } else {
            total_uncapped = single_capped * stacks;
            total_capped = min2(total_uncapped, cap);
            off.pset_num(pi, "TotalPoisonDPS", total_capped);
        }
        // :4897-4985 (the branch rewrites output.PoisonDotMulti)
        if off.breakdown {
            let g_crit = gn(off, "CritChance", 4902);
            rewrite_dot_multi(off, pi, "Poison", g_crit, 4900);
            let mut lines = Vec::new();
            if ruthless != 1.0 {
                lines.push(sf!("x %.2f ^8(ruthless blow effect modifier)", ruthless));
            }
            lines.push("x 0.30 ^8(poison deals 30% per second)".to_owned());
            lines.push(sf!("= %.1f", base_val, 1.0));
            lines.push(String::new());
            lines.push("Poison DPS:".to_owned());
            ailment_dps_lines(
                &mut lines,
                "Poison",
                AilmentDps {
                    base: base_val,
                    effect_mod,
                    rate_mod,
                    rate_label: "damage rate modifier",
                    stacks: 1.0,
                    stacks_label: "",
                    eff_mult,
                    uncapped: single_uncapped,
                    capped: single_capped,
                    min_uncapped: base_min * effect_mod * rate_mod * eff_mult,
                    max_uncapped: base_max * effect_mod * rate_mod * eff_mult,
                    cap,
                },
            );
            if let Some(bd) = off.pbd(pi) {
                let t = bd.table_or("PoisonDPS", BTable::new);
                for l in lines {
                    t.push(l);
                }
            }
            if !off.sd_truthy("showAverage") && stacks != 1.0 {
                let neg = crate::breakdown::color("NEGATIVE");
                let mut t = BTable::lines([
                    sf!("%.1f ^8single poison damage per second", single_capped),
                    sf!("x %.2f ^8(poison stacks)", stacks),
                ]);
                if total_capped != total_uncapped {
                    t.push(sf!("= %.1f ^8(Uncapped raw Poison DPS)", total_uncapped));
                    t.push(crate::breakdown::format(
                        &format!("^8(Raw Poison DPS is {neg}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)"),
                        &[
                            pob_lua::Arg::Num(total_uncapped - total_capped),
                            pob_lua::Arg::Num((total_uncapped - total_capped) / total_capped * 100.0),
                        ],
                    ));
                    t.push(sf!("= %d ^8(Capped Poison DPS)", total_capped));
                } else {
                    t.push(sf!("= %.1f ^8per second", total_capped));
                }
                off.pbd_put(pi, "TotalPoisonDPS", t);
            }
            if poison_duration != 2.0 {
                let mut t = BTable::lines([sf!("%.2fs ^8(base duration)", duration_base)]);
                if duration_mod != 1.0 {
                    t.push(sf!("x %.2f ^8(duration modifier)", duration_mod));
                }
                if rate_mod != 1.0 {
                    t.push(sf!("/ %.2f ^8(damage rate modifier)", rate_mod));
                }
                if off.debuff_duration_mult != 1.0 {
                    t.push(sf!("/ %.2f ^8(debuff expires slower/faster)", 1.0 / off.debuff_duration_mult));
                }
                t.push(sf!("= %.2fs", poison_duration));
                off.bd_put("PoisonDuration", t);
            }
            let mut t = BTable::new();
            if off.is_attack {
                t.push(format!("{}:", off.passes[pi].label));
            }
            t.push(sf!("%.1f ^8(damage per second)", single_capped));
            t.push(sf!("x %.2fs ^8(poison duration)", poison_duration));
            t.push(sf!("= %.1f ^8damage per poison stack", pn(off, pi, "PoisonDamage", 4983)));
            off.pbd_put(pi, "PoisonDamage", t);
        }
    }
}

/// The `breakdown.effMult(dt, resist, 0, takenInc, effMult, takenMore, sourceRes, true)` of a DoT (:4857, :5199, :5209): the same queries
/// as [`dot_eff_mult`], `sourceRes` from `env.modDB` / the party stand-in.
fn dot_eff_mult_breakdown(off: &Off<'_>, c: &ModCfg, dt: &str, extra: Option<&str>, eff_mult: f64) -> BTable {
    let resist = resist_for_type(off, dt, Some(c));
    let (a, b) = (format!("{dt}DamageTaken"), format!("{dt}DamageTakenOverTime"));
    let mut names = vec!["DamageTaken", "DamageTakenOverTime", a.as_str(), b.as_str()];
    if let Some(e) = extra {
        names.push(e);
    }
    let taken_inc = dsum(&off.enemy_db, ModType::Inc, Some(c), &names);
    let taken_more = dmore(&off.enemy_db, Some(c), &names);
    let flag_name = format!("Enemy{dt}ResistEqualToYours");
    let source_res = if dflag(&off.env_db, None, &[&flag_name]) {
        format!("Your {dt} Resistance")
    } else if dflag(&off.st.party_members.mod_db, None, &[&flag_name]) {
        format!("Party Member {dt} Resistance")
    } else {
        dt.to_owned()
    };
    crate::breakdown::eff_mult(
        dt,
        resist,
        0.0,
        taken_inc,
        eff_mult,
        taken_more,
        Some(&source_res),
        true,
        None,
        None,
    )
}

/// :4990-5330 ignite (with its breakdown rows, R8.5).
#[allow(clippy::too_many_lines)]
fn ignite(off: &mut Off<'_>, q: &Q, pi: usize) {
    let edb = off.enemy_db.clone();
    let mut dc = dot_cfg(off, pi, "Ignite", "FireDot");
    let c = off.pcfg(pi).clone();
    // :5006-5012
    if off.breakdown {
        for k in ["IgnitePhysical", "IgniteLightning", "IgniteCold", "IgniteFire", "IgniteChaos"] {
            off.pbd_put(pi, k, BTable::new().with("damageTypes", BTable::new()));
        }
    }
    // :5014
    let crit_chance = pn(off, pi, "CritChance", 5014);
    let per_hit = pn(off, pi, "IgniteChanceOnHit", 5014) * (1.0 - crit_chance / 100.0)
        + pn(off, pi, "IgniteChanceOnCrit", 5014) * crit_chance / 100.0;
    off.gset("IgniteChancePerHit", per_hit);
    // :5017-5022
    let mut max_stacks = 1.0;
    if off.flag("igniteCanStack") {
        let site = format!("{LUA}:5019");
        max_stacks = q
            .override_num(Some(&c), &["IgniteStacks"], &site)
            .unwrap_or_else(|| max_stacks + q.base(Some(&c), &["IgniteStacks"]));
    }
    let site = format!("{LUA}:5021");
    let override_stack_potential = if q.override_(None, &["IgniteStackPotentialOverride"]).is_some_and(|v| v.truthy()) {
        Some(q.override_num(None, &["IgniteStackPotentialOverride"], &site).expect("truthy") / max_stacks)
    } else {
        None
    };
    off.gset("IgniteStacksMax", max_stacks);
    // :5024-5028
    let rate_mod = (q.calc_mod(Some(&c), &["IgniteBurnFaster"]) + dsum(&edb, ModType::Inc, None, &["SelfIgniteBurnFaster"]) / 100.0)
        / q.calc_mod(Some(&c), &["IgniteBurnSlower"]);
    let site = format!("{LUA}:5025");
    let duration_base = q
        .override_num(Some(&dc), &["IgniteDurationBase"], &site)
        .unwrap_or_else(|| misc("IgniteDurationBase"));
    let mut duration_mod = max2(
        q.calc_mod(
            Some(&dc),
            &[
                "EnemyIgniteDuration",
                "EnemyAilmentDuration",
                "EnemyElementalAilmentDuration",
                "DamagingAilmentDuration",
            ],
        ) * dmod(
            &edb,
            None,
            &["SelfIgniteDuration", "SelfAilmentDuration", "SelfElementalAilmentDuration"],
        ),
        0.0,
    );
    duration_mod = max2(duration_mod, 0.0);
    let ignite_duration = duration_base * duration_mod / rate_mod * off.debuff_duration_mult;
    off.gset("IgniteDuration", ignite_duration);
    // :5030-5042
    let crit_chance = pn(off, pi, "CritChance", 5030);
    let ignite_chance = pn(off, pi, "IgniteChanceOnHit", 5030) / 100.0 * (1.0 - crit_chance / 100.0)
        + pn(off, pi, "IgniteChanceOnCrit", 5030) / 100.0 * crit_chance / 100.0;
    let mut stacks = pn(off, pi, "HitChance", 5032) / 100.0 * ignite_chance * dps_multiplier(off, 5032);
    if !off.sd_truthy("triggeredOnDeath") {
        if off.ptruthy(pi, "Cooldown") {
            let cd = pn(off, pi, "Cooldown", 5035);
            stacks = stacks * ignite_duration / max2(cd, hit_time(off, pi, 5035));
        } else if off.g_hit_speed(5036) > 0.0 {
            stacks = stacks * ignite_duration * off.g_hit_speed(5038);
        }
    }
    let stack_potential = override_stack_potential.unwrap_or(stacks / max_stacks);
    off.gset("IgniteStackPotential", stack_potential);
    // :5043-5072
    if off.breakdown {
        let mut t = BTable::lines([
            format!("{}NOTE: Calculation uses a Weighted Avg formula", crate::breakdown::color("CUSTOM")),
            String::new(),
        ]);
        let site = format!("{LUA}:5050");
        if let Some(o) = q.override_num(None, &["IgniteStackPotentialOverride"], &site) {
            let osp = override_stack_potential.expect("set with the override");
            if max_stacks != 1.0 {
                t.push(sf!("= %d / %d ^8(stack potential override / max ignite stacks)", o, max_stacks));
                t.push(sf!("= %g ^8(stack potential)", osp));
            } else {
                t.push(sf!("= %g ^8(stack potential override)", osp));
            }
        } else {
            t.push(sf!("%.2f ^8(chance to hit)", pn(off, pi, "HitChance", 5056) / 100.0));
            t.push(sf!("* %.2f ^8(chance to ignite)", ignite_chance));
            if off.sd_truthy("triggeredOnDeath") {
                t.push(sf!("* 1 ^8Cast on Death override"));
            } else if off.ptruthy(pi, "Cooldown") {
                t.push(sf!(
                    "* (%.2f / max(%.2f, %.2f) ^8(Ignite Duration / max(Cooldown, Cast Time))",
                    ignite_duration,
                    pn(off, pi, "Cooldown", 5061),
                    hit_time(off, pi, 5061)
                ));
            } else if off.g_hit_speed(5062) > 0.0 {
                let t_hit = match off.gval("HitTime") {
                    None | Some(crate::output::OutValue::Bool(false)) => pn(off, pi, "Time", 5063),
                    v => num_at(v, "HitTime", 5063),
                };
                t.push(sf!("* (%.2f / %.2f) ^8(Ignite Duration / Attack Time)", ignite_duration, t_hit));
            }
            let dm = dps_multiplier(off, 5065);
            if dm != 1.0 {
                t.push(sf!("* %g ^8(DPS multiplier for this skill)", dm));
            }
            t.push(sf!("/ %d ^8(max number of stacks)", max_stacks));
            t.push(sf!("= %.2f", stack_potential));
        }
        off.bd_put("IgniteStackPotential", t);
    }
    // :5074-5082
    let roll = if stack_potential > 1.0 {
        (stacks - (max_stacks - 1.0) / 2.0) / (stacks + 1.0) * 100.0
    } else {
        50.0
    };
    off.gset("IgniteRollAverage", roll);
    // :5083-5097
    if off.breakdown {
        let custom = crate::breakdown::color("CUSTOM");
        let mut t = BTable::lines([
            format!("{custom}This is the average roll of an ignite affecting the enemy if you are constantly attacking"),
            crate::breakdown::format(
                &format!(
                    "{custom}If attacking constantly, your average strongest Ignite currently achieves ^7%.2f%%{custom} of its max damage"
                ),
                &[pob_lua::Arg::Num(roll)],
            ),
            String::new(),
            "Average Ignite Roll:".to_owned(),
        ]);
        if stacks >= max_stacks {
            t.push(sf!("%.2f - (%.2f - 1)/2", stacks, max_stacks));
            t.push(sf!("/ (%.2f + 1)", stacks));
            t.push(sf!("= %.2f%%", roll));
        } else {
            t.push(sf!("50%% (averaging <= %d ignites)", max_stacks));
        }
        off.bd_put("IgniteRollAverage", t);
    }
    // :5099-5156
    let (mut hit_dmg, mut crit_dmg, mut max_crit, mut min_hit, mut max_hit) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for sub_pass in 1..=2 {
        set_crit(&mut dc, sub_pass != 1);
        let (mut total_min, mut total_max) = (0.0, 0.0);
        for (dt, flag, cannot, key, tf) in [
            ("Physical", "PhysicalCanIgnite", false, "IgnitePhysical", FLAG_FIRE),
            ("Lightning", "LightningCanIgnite", false, "IgniteLightning", FLAG_FIRE),
            ("Cold", "ColdCanIgnite", false, "IgniteCold", FLAG_FIRE),
            ("Fire", "FireCannotIgnite", true, "IgniteFire", 0),
            ("Chaos", "ChaosCanIgnite", false, "IgniteChaos", FLAG_FIRE),
        ] {
            let can = off.can_deal.get(dt).copied().unwrap_or(false) && (q.flag(Some(&c), &[flag]) != cannot);
            if can {
                let (min, max) = source_damage(off, q, &dc, pi, dt, tf, (sub_pass == 1).then_some(key));
                off.pset_num(pi, &format!("{key}Min"), min);
                off.pset_num(pi, &format!("{key}Max"), max);
                total_min += min;
                total_max += max;
            }
        }
        if sub_pass == 2 {
            let m = crit_dot_multi(off, q, &mut dc, "FireDotMultiplier", 5141);
            off.pset_num(pi, "CritIgniteDotMulti", m);
            crit_dmg = (total_min + (total_max - total_min) * roll / 100.0) * m;
            max_crit = total_max * m;
        } else {
            let m = dot_multi(q, &dc, "FireDotMultiplier", 5149);
            off.pset_num(pi, "IgniteDotMulti", m);
            hit_dmg = (total_min + (total_max - total_min) * roll / 100.0) * m;
            min_hit = total_min * m;
            max_hit = total_max * m;
        }
        off.pset_num(pi, "IgniteTotalMin", total_min);
        off.pset_num(pi, "IgniteTotalMax", total_max);
    }
    // :5157-5181
    if off.breakdown {
        let (tmin, tmax) = (pn(off, pi, "IgniteTotalMin", 5161), pn(off, pi, "IgniteTotalMax", 5161));
        let derivation = sf!("%.0f + (%.0f - %.0f) * %.2f%%", tmin, tmax, tmin, roll);
        let note = "^8min combined sources + (max combined sources - min combined sources) * average ignite roll".to_owned();
        let t = if hit_dmg == crit_dmg || pn(off, pi, "CritChance", 5158) == 0.0 {
            BTable::lines([
                "Dmg Derivation:".to_owned(),
                derivation,
                note,
                sf!("* %.2f ^8(Ignite DoT Multi)", pn(off, pi, "IgniteDotMulti", 5163)),
                sf!("= %.2f", hit_dmg),
            ])
        } else {
            BTable::lines([
                "Non-Crit Dmg Derivation:".to_owned(),
                derivation.clone(),
                note.clone(),
                sf!("* %.2f ^8(Ignite DoT Multi for Non-Crit)", pn(off, pi, "IgniteDotMulti", 5171)),
                sf!("= %.2f", hit_dmg),
                String::new(),
                "Crit Dmg Derivation:".to_owned(),
                derivation,
                note,
                sf!("* %.2f ^8(Ignite DoT Multi for Crit)", pn(off, pi, "CritIgniteDotMulti", 5177)),
                sf!("= %.2f", crit_dmg),
            ])
        };
        off.bd_put("IgniteDPS", t);
    }
    // :5183-5186
    let crit_chance = pn(off, pi, "CritChance", 5183);
    let ailment_crit_chance = 100.0 * (1.0 - (1.0 - crit_chance / 100.0).powf(max2(1.0, stacks)));
    let percent = misc("IgnitePercentBase");
    let ruthless = pn(off, pi, "RuthlessBlowAilmentEffect", 5185);
    let fist = pn(off, pi, "FistOfWarDamageEffect", 5185);
    let warcry = gn(off, "AilmentWarcryEffect", 5185);
    let base_min = calc_ailment_damage(off, q, pi, "Ignite", ailment_crit_chance, min_hit, 0.0, true) * percent;
    let base_max = calc_ailment_damage(off, q, pi, "Ignite", 100.0, max_hit, max_crit, true) * percent * ruthless * fist * warcry;
    let base_val =
        calc_ailment_damage(off, q, pi, "Ignite", ailment_crit_chance, hit_dmg, crit_dmg, false) * percent * ruthless * fist * warcry;
    if base_val > 0.0 {
        off.set_flag("ignite", true);
        let mut eff_mult = 1.0;
        if off.env.mode_effective {
            let (dt, extra) = if q.flag(Some(&c), &["IgniteToChaos"]) {
                ("Chaos", None)
            } else {
                ("Fire", Some("ElementalDamageTaken"))
            };
            eff_mult = dot_eff_mult(off, &dc, dt, extra);
            off.gset("IgniteEffMult", eff_mult);
            // :5197-5200 / :5207-5210
            if off.breakdown && eff_mult != 1.0 {
                let t = dot_eff_mult_breakdown(off, &dc, dt, extra, eff_mult);
                off.bd_put("IgniteEffMult", t);
            }
        }
        let effect_mod = q.calc_mod(Some(&dc), &["AilmentEffect"]);
        let active = min2(stacks, max_stacks);
        let cap = misc("DotDpsCap");
        let uncapped = base_val * effect_mod * rate_mod * active * eff_mult;
        let capped = min2(uncapped, cap);
        off.pset_num(pi, "IgniteDPS", capped);
        let ground_mult = max2(
            q.max(None, &["IgniteDpsAsBurningGround"]).unwrap_or(0.0),
            ok(edb.borrow().max(None, &["IgniteDpsAsBurningGround"])).unwrap_or(0.0),
        );
        if ground_mult > 0.0 {
            let fire_eff_mult = dot_eff_mult(off, &dc, "Fire", Some("ElementalDamageTaken"));
            let uncapped = base_val * effect_mod * rate_mod * fire_eff_mult * ground_mult / 100.0;
            off.gset("BurningGroundDPS", min2(uncapped, cap));
            off.out().set_bool("BurningGroundFromIgnite", true);
            // :5233-5240
            if off.breakdown {
                let t = BTable::lines([
                    sf!("%.1f ^8(ignite damage per second)", base_val * effect_mod * rate_mod),
                    sf!("* %.1f%% ^8(percent as burning ground)", ground_mult),
                    sf!("* %.3f ^8(effective DPS modifier)", fire_eff_mult),
                    sf!("= %.1f ^8per second", min2(uncapped, cap)),
                ]);
                off.bd_put("BurningGroundDPS", t);
            }
        }
        let dps = pn(off, pi, "IgniteDPS", 5242);
        off.gset("IgniteDamage", dps * gn(off, "IgniteDuration", 5242));
        if off.flag("igniteCanStack") {
            let dur = gn(off, "IgniteDuration", 5244);
            off.pset_num(pi, "IgniteDamage", dps * dur);
            off.pset_num(pi, "IgniteStacksMax", max_stacks);
            off.pset_num(pi, "TotalIgniteDPS", dps);
        }
        // :5249-5329 (the branch rewrites output.IgniteDotMulti)
        if off.breakdown {
            let mut lines = Vec::new();
            if ruthless != 1.0 {
                lines.push(sf!("x %.2f ^8(ruthless blow effect modifier)", ruthless));
            }
            lines.push("x 0.9 ^8(ignite deals 90% per second)".to_owned());
            lines.push(sf!("= %.1f", base_val, 1.0));
            lines.push(String::new());
            lines.push("Average Ignite DPS:".to_owned());
            ailment_dps_lines(
                &mut lines,
                "Ignite",
                AilmentDps {
                    base: base_val,
                    effect_mod,
                    rate_mod,
                    rate_label: "burn rate modifier",
                    stacks: active,
                    stacks_label: "avg ignite stacks",
                    eff_mult,
                    uncapped,
                    capped,
                    min_uncapped: base_min * effect_mod * rate_mod * active * eff_mult,
                    max_uncapped: base_max * effect_mod * rate_mod * active * eff_mult,
                    cap,
                },
            );
            if let Some(bd) = off.pbd(pi) {
                let t = bd.table_or("IgniteDPS", BTable::new);
                for l in lines {
                    t.push(l);
                }
            }
            rewrite_dot_multi(off, pi, "Ignite", ailment_crit_chance, 5296);
            if off.flag("igniteCanStack") {
                let mut t = BTable::new();
                if off.is_attack {
                    t.push(format!("{}:", off.passes[pi].label));
                }
                t.push(sf!("%.1f ^8(damage per second)", dps));
                t.push(sf!("x %.2fs ^8(ignite duration)", ignite_duration));
                t.push(sf!("= %.1f ^8damage per ignite stack", pn(off, pi, "IgniteDamage", 5311)));
                off.pbd_put(pi, "IgniteDamage", t);
            }
            if ignite_duration != misc("IgniteDurationBase") {
                let mut t = BTable::lines([sf!("%.2fs ^8(base duration)", duration_base)]);
                if duration_mod != 1.0 {
                    t.push(sf!("x %.2f ^8(duration modifier)", duration_mod));
                }
                if rate_mod != 1.0 {
                    t.push(sf!("/ %.2f ^8(burn rate modifier)", rate_mod));
                }
                if off.debuff_duration_mult != 1.0 {
                    t.push(sf!("/ %.2f ^8(debuff expires slower/faster)", 1.0 / off.debuff_duration_mult));
                }
                t.push(sf!("= %.2fs", ignite_duration));
                off.bd_put("IgniteDuration", t);
            }
        }
    }
}
