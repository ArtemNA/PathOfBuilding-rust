//! CalcOffence.lua:2606-3345 (R7.5, part C, first half): the head of the damage pass loop -- exerted attacks / warcry uptimes and
//! pacts (:2615-2974), Ruthless and Fist of War (:2976-3033), Explosive Arrow (:3035-3039), crit chance / multiplier / effect
//! (:3041-3275), triple / double damage (:3277-3298), culling and reservation DPS (:3300-3313), `postCritFunc` (:3315) and the
//! Cryogenesis added damage redirection (:3317-3344). The rest of the loop body is [`super::c2`].
//!
//! `output` inside the loop is `pass.output` (a hand table or `output` itself), `globalOutput` the top-level `output`; `cfg` the pass cfg
//! (`skillCfg` for the "Skill" pass -- the same table), `skillCfg` = [`Off::cfg`].
//!
//! Hash-order walks (R13/D13): none -- the warcry list, `dmgTypeList` and `actor.activeSkillList` are arrays.
//!
//! Breakdown (R8.5): the `if breakdown` / `if globalBreakdown` branches of this half are ported where they stand; the queries their
//! conditions make (`Flag(skillCfg, "Unexciting")`, `Every3UseCrit`, ...) are still made where a malformed mod could raise.

use pob_lua::number::{floor, max2, min2, round};
use pob_mods::{ModCfg, ModType, StoreRef, Str, Value};

use super::helpers::{Q, calc_skill_cooldown, calc_skill_duration, calc_warcry_cast_time};
use super::{Off, PassOut, run_skill_func};
use crate::breakdown::BTable;
use crate::output::OutValue;
use crate::perform::{skill_cfg_of, skill_list_of};
use crate::setup::{misc, mod_flag, ok, tag};
use crate::sf;

pub(super) const LUA: &str = "Modules/CalcOffence.lua";

/// `db:Flag(cfg, ...)` truthy.
pub(super) fn dflag(db: &StoreRef, cfg: Option<&ModCfg>, names: &[&str]) -> bool {
    ok(db.borrow().flag(cfg, names)) == Some(true)
}

/// `db:Sum(type, cfg, ...)`.
pub(super) fn dsum(db: &StoreRef, ty: ModType, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
    ok(db.borrow().sum(ty, cfg, names))
}

/// `db:More(cfg, ...)`.
pub(super) fn dmore(db: &StoreRef, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
    ok(db.borrow().more(cfg, names))
}

/// A Lua value used in arithmetic (nil / non-number -> the Lua error at `line`).
pub(super) fn num_at(v: Option<&OutValue>, k: &str, line: u32) -> f64 {
    match v {
        Some(OutValue::Num(x)) => *x,
        other => panic!("{LUA}:{line}: attempt to perform arithmetic on field '{k}' ({other:?})"),
    }
}

/// `x or d` for an output value used as a number.
pub(super) fn or_num(v: Option<&OutValue>, d: f64, k: &str, line: u32) -> f64 {
    match v {
        None | Some(OutValue::Bool(false)) => d,
        other => num_at(other, k, line),
    }
}

/// `env.configInput[key]`.
pub(super) fn config(off: &Off<'_>, key: &str) -> Option<pob_build::ConfigValue> {
    off.env.config.input.get(key).cloned()
}

/// `env.configInput[key] == s`.
pub(super) fn config_is(off: &Off<'_>, key: &str, s: &str) -> bool {
    matches!(off.env.config.input.get(key), Some(pob_build::ConfigValue::Str(v)) if v == s)
}

/// `env.configInput[key]` truthy.
pub(super) fn config_truthy(off: &Off<'_>, key: &str) -> bool {
    !matches!(off.env.config.input.get(key), None | Some(pob_build::ConfigValue::Bool(false)))
}

impl Off<'_> {
    /// `globalOutput[k]`.
    pub(super) fn gval(&self, k: &str) -> Option<&OutValue> {
        self.st.player.output.get(k)
    }

    /// `globalOutput[k] or d` as a number.
    pub(super) fn g_or(&self, k: &str, d: f64, line: u32) -> f64 {
        or_num(self.gval(k), d, k, line)
    }

    /// `globalOutput[k]` truthy.
    pub(super) fn gtruthy(&self, k: &str) -> bool {
        !matches!(self.gval(k), None | Some(OutValue::Bool(false)))
    }

    /// `globalOutput.HitSpeed or globalOutput.Speed`.
    pub(super) fn g_hit_speed(&self, line: u32) -> f64 {
        match self.gval("HitSpeed") {
            None | Some(OutValue::Bool(false)) => num_at(self.gval("Speed"), "Speed", line),
            v => num_at(v, "HitSpeed", line),
        }
    }

    pub(super) fn gset(&mut self, k: &str, v: f64) {
        self.out().set_num(k, v);
    }

    /// `pass.output[k] = v` for any Lua value read off an output table (nil removes the key).
    pub(super) fn pset_val(&mut self, pi: usize, k: &str, v: Option<OutValue>) {
        match v {
            Some(v) => self.pset(pi, k, v),
            None => match self.passes[pi].out {
                PassOut::Global => self.out().remove(k),
                PassOut::Hand(h) => {
                    if let Some(s) = self.passes[pi].cfg.skill_stats.as_mut() {
                        s.shift_remove(k);
                    }
                    self.hands.as_mut().expect("hands")[h].map.shift_remove(k);
                }
            },
        }
    }

    /// `cfg.skillCond[key] = v` on the pass cfg -- the same table as `activeSkill.skillCfg` ("Skill" pass) or
    /// `activeSkill.weapon<n>Cfg` (a hand pass), so the write is kept on the active skill too.
    pub(super) fn set_pass_cond(&mut self, pi: usize, key: &str, v: bool) {
        self.passes[pi].cfg.skill_cond.get_or_insert_default().insert(Str::from(key), v);
        let out = self.passes[pi].out;
        if out == PassOut::Global {
            self.cfg.skill_cond.get_or_insert_default().insert(Str::from(key), v);
        }
        let sk = self.sk_mut();
        let c = match out {
            PassOut::Global => sk.skill_cfg.as_mut(),
            PassOut::Hand(0) => sk.weapon1_cfg.as_mut(),
            PassOut::Hand(_) => sk.weapon2_cfg.as_mut(),
        };
        c.expect("pass cfg").skill_cond.insert(key.to_owned(), v);
    }
}

/// `value` of `for _, value in ipairs(actor.activeSkillList)` (`j` = the position): its skill list query, `skillCfg` and `skillData`.
fn other_skill(off: &Off<'_>, j: usize) -> (Q, ModCfg, indexmap::IndexMap<String, Value>) {
    let st = off.env.skills.as_ref().expect("skills on");
    let id = off.actor_skill(j);
    let sk = st.skill(id);
    (
        Q {
            list: skill_list_of(off.env, id),
            malformed: sk.malformed_mods.clone(),
        },
        skill_cfg_of(st, sk),
        sk.skill_data.clone(),
    )
}

/// `value.activeEffect.grantedEffect.name` and `value.skillFlags.disable` of every active skill.
fn skill_names(off: &Off<'_>) -> Vec<(String, bool)> {
    let st = off.env.skills.as_ref().expect("skills on");
    (0..off.actor_skill_count())
        .map(|j| {
            let sk = st.skill(off.actor_skill(j));
            (
                st.skills.effects[sk.active_effect.borrow().granted_effect].name.clone(),
                sk.flag("disable"),
            )
        })
        .collect()
}

/// `value.skillData.storedUses + value.skillModList:Sum("BASE", value.skillCfg, "AdditionalCooldownUses")`.
fn stored_uses(q: &Q, cfg: &ModCfg, sd: &indexmap::IndexMap<String, Value>, line: u32) -> f64 {
    let s = match sd.get("storedUses") {
        Some(Value::Num(x)) => *x,
        other => panic!("{LUA}:{line}: attempt to perform arithmetic on field 'storedUses' ({other:?})"),
    };
    s + q.base(Some(cfg), &["AdditionalCooldownUses"])
}

/// One warcry of the exert loop (:2631-2828): `<Prefix>CryDuration` / `CryCooldown` / `CryCastTime`.
fn warcry_times(off: &mut Off<'_>, j: usize, prefix: &str) {
    let (vq, vcfg, vsd) = other_skill(off, j);
    let d = calc_skill_duration(&vq.list, &vq.malformed, &vcfg, &vsd, off.env.mode_effective, &off.enemy_db);
    off.gset(&format!("{prefix}CryDuration"), d);
    let (cd, _, _) = calc_skill_cooldown(&vq, &vcfg, &vsd);
    off.gset(&format!("{prefix}CryCooldown"), cd);
    let action = crate::defence::action_speed_mod(&off.mod_db);
    let ct = calc_warcry_cast_time(&vq, &vcfg, &vsd, action);
    off.gset(&format!("{prefix}CryCastTime"), ct);
}

/// The uptime of warcry `j` (`<Prefix>` keys): `<count_key>` exerts, writes `<up_key>` and adds it to `GlobalWarcryUptimeRatio`.
#[allow(clippy::too_many_arguments)]
fn warcry_uptime(off: &mut Off<'_>, j: usize, prefix: &str, count_key: &str, count_mod: &str, up_key: &str, line: u32) {
    let count = dsum(&off.env_db, ModType::Base, None, &[count_mod]);
    off.gset(count_key, count);
    let speed = num_at(off.gval("Speed"), "Speed", line);
    let (cd_key, ct_key) = (format!("{prefix}CryCooldown"), format!("{prefix}CryCastTime"));
    let cd = num_at(off.gval(&cd_key), &cd_key, line);
    let ct = num_at(off.gval(&ct_key), &ct_key, line);
    let base_uptime_ratio = min2((count / speed) / (cd + ct), 1.0) * 100.0;
    let (vq, vcfg, vsd) = other_skill(off, j);
    let stored = stored_uses(&vq, &vcfg, &vsd, line + 1);
    let up = min2(100.0, base_uptime_ratio * stored);
    off.gset(up_key, up);
    let g = num_at(off.gval("GlobalWarcryUptimeRatio"), "GlobalWarcryUptimeRatio", line + 3) + up;
    off.gset("GlobalWarcryUptimeRatio", g);
    // :2640-2652 (and the same block of each warcry): `globalBreakdown.<Prefix>UpTimeRatio`
    if off.breakdown {
        let mut t = BTable::lines([sf!("(%d ^8(number of exerts)", count), sf!("/ %.2f) ^8(attacks per second)", speed)]);
        if ct > 0.0 {
            t.push(sf!("/ (%.2f ^8(warcry cooldown)", cd));
            t.push(sf!("+ %.2f) ^8(warcry casttime)", ct));
        } else {
            t.push(sf!("/ %.2f ^8(average warcry cooldown)", cd));
        }
        t.push(sf!("* %d ^8(stored uses)", stored));
        t.push(sf!("= %d%%", up));
        off.bd_put(up_key, t);
    }
}

/// :2626-2974 (`env.mode_buffs`): warcry uptimes, exerted attacks, pacts.
#[allow(clippy::too_many_lines)]
fn exerts_and_pacts(off: &mut Off<'_>, pi: usize, exerted_double_damage: f64, exerting_warcry_count: f64) {
    let q = off.q.clone();
    let skill_cfg = off.cfg.clone();
    let types_ok = ![
        "NeverExertable",
        "Triggered",
        "Channel",
        "OtherThingUsesSkill",
        "Retaliation",
        "SummonsTotem",
    ]
    .iter()
    .any(|t| off.has_type(t));
    if types_ok {
        let names = skill_names(off);
        for (j, (name, disable)) in names.iter().enumerate() {
            let disable = *disable;
            if name == "Ancestral Cry" && off.has_type("MeleeSingleTarget") && !off.gtruthy("AncestralCryCalculated") && !disable {
                warcry_times(off, j, "Ancestral");
                warcry_uptime(
                    off,
                    j,
                    "Ancestral",
                    "AncestralExertsCount",
                    "NumAncestralExerts",
                    "AncestralUpTimeRatio",
                    2636,
                );
                off.out().set_bool("AncestralCryCalculated", true);
            } else if name == "Infernal Cry" && !off.gtruthy("InfernalCryCalculated") && !disable {
                warcry_times(off, j, "Infernal");
                if off.has_type("Melee") {
                    warcry_uptime(
                        off,
                        j,
                        "Infernal",
                        "InfernalExertsCount",
                        "NumInfernalExerts",
                        "InfernalUpTimeRatio",
                        2660,
                    );
                }
                off.out().set_bool("InfernalCryCalculated", true);
            } else if name == "Intimidating Cry" && off.has_type("Melee") && !off.gtruthy("IntimidatingCryCalculated") && !disable {
                off.out().set_bool("CreateWarcryOffensiveCalcSection", true);
                warcry_times(off, j, "Intimidating");
                warcry_uptime(
                    off,
                    j,
                    "Intimidating",
                    "IntimidatingExertsCount",
                    "NumIntimidatingExerts",
                    "IntimidatingUpTimeRatio",
                    2685,
                );
                let enemy_dd = if off.env.mode_effective {
                    dsum(&off.enemy_db, ModType::Base, Some(off.pcfg(pi)), &["SelfDoubleDamageChance"])
                } else {
                    0.0
                };
                let dd_chance = min2(
                    q.base(Some(off.pcfg(pi)), &["DoubleDamageChance"]) + enemy_dd + exerted_double_damage,
                    100.0,
                );
                let avg = 2.0 * (1.0 - dd_chance / 100.0);
                off.gset("IntimidatingAvgDmg", avg);
                if off.breakdown {
                    let t = BTable::lines([
                        "Average Intimidating Cry Damage:".to_owned(),
                        sf!(
                            "%.2f%% ^8(base double damage increase to hit 100%%)",
                            (1.0 - dd_chance / 100.0) * 100.0
                        ),
                        sf!("x %d ^8(double damage multiplier)", 2.0),
                        sf!("= %.2f", avg),
                    ]);
                    off.bd_put("IntimidatingAvgDmg", t);
                }
                let up = off.g_or("IntimidatingUpTimeRatio", 0.0, 2712);
                off.gset("IntimidatingHitEffect", 1.0 + avg * up / 100.0);
                if off.breakdown {
                    let t = BTable::lines([
                        sf!("1 + (%.2f ^8(average exerted damage)", avg),
                        sf!("x %.2f) ^8(uptime %%)", up / 100.0),
                        sf!("= %.2f", 1.0 + avg * up / 100.0),
                    ]);
                    off.bd_put("IntimidatingHitEffect", t);
                }
                off.gset("IntimidatingMaxHitEffect", 1.0 + avg);
                let t = off.g_or("TheoreticalOffensiveWarcryEffect", 0.0, 2722) * (1.0 + avg * up / 100.0);
                off.gset("TheoreticalOffensiveWarcryEffect", t);
                let t = off.g_or("TheoreticalMaxOffensiveWarcryEffect", 0.0, 2723) * (1.0 + avg);
                off.gset("TheoreticalMaxOffensiveWarcryEffect", t);
                off.out().set_bool("IntimidatingCryCalculated", true);
            } else if name == "Rallying Cry" && off.has_type("Melee") && !off.gtruthy("RallyingCryCalculated") && !disable {
                off.out().set_bool("CreateWarcryOffensiveCalcSection", true);
                warcry_times(off, j, "Rallying");
                warcry_uptime(
                    off,
                    j,
                    "Rallying",
                    "RallyingExertsCount",
                    "NumRallyingExerts",
                    "RallyingUpTimeRatio",
                    2731,
                );
                let allies = min2(
                    dsum(&off.env_db, ModType::Base, Some(off.pcfg(pi)), &["Multiplier:NearbyAlly"]),
                    5.0,
                );
                let per_ally = dsum(&off.env_db, ModType::Base, None, &["RallyingExertMoreDamagePerAlly"]) / 100.0;
                let avg = allies * per_ally;
                off.gset("RallyingAvgDmg", avg);
                if off.breakdown {
                    let t = BTable::lines([
                        "Average Rallying Cry Damage:".to_owned(),
                        sf!("%.2f ^8(average damage multiplier per ally)", per_ally),
                        sf!("x %d ^8(number of nearby allies (max=5))", allies),
                        sf!("= %.2f", avg),
                    ]);
                    off.bd_put("RallyingAvgDmg", t);
                }
                let up = off.g_or("RallyingUpTimeRatio", 0.0, 2757);
                let hit = 1.0 + avg * up / 100.0;
                off.gset("RallyingHitEffect", hit);
                if off.breakdown {
                    let t = BTable::lines([
                        sf!("1 + (%.2f ^8(average exerted damage)", avg),
                        sf!("x %.2f) ^8(uptime %%)", up / 100.0),
                        sf!("= %.2f", hit),
                    ]);
                    off.bd_put("RallyingHitEffect", t);
                }
                off.gset("RallyingMaxHitEffect", 1.0 + avg);
                for (k, m) in [
                    ("OffensiveWarcryEffect", hit),
                    ("MaxOffensiveWarcryEffect", 1.0 + avg),
                    ("TheoreticalOffensiveWarcryEffect", hit),
                    ("TheoreticalMaxOffensiveWarcryEffect", 1.0 + avg),
                ] {
                    let v = off.g_or(k, 0.0, 2766) * m;
                    off.gset(k, v);
                }
                off.out().set_bool("RallyingCryCalculated", true);
            } else if name == "Seismic Cry" && off.has_type("Slam") && !off.gtruthy("SeismicCryCalculated") && !disable {
                off.out().set_bool("CreateWarcryOffensiveCalcSection", true);
                warcry_times(off, j, "Seismic");
                warcry_uptime(
                    off,
                    j,
                    "Seismic",
                    "SeismicExertsCount",
                    "NumSeismicExerts",
                    "SeismicUpTimeRatio",
                    2778,
                );
                if q.flag(None, &["Condition:WarcryMaxHit"]) {
                    let more_aoe = dsum(&off.env_db, ModType::Base, None, &["SeismicMoreAoE"]);
                    q.nm("AreaOfEffect", ModType::More, more_aoe, "Max Seismic Exert AoE", 0, 0, vec![]);
                } else {
                    let more_aoe = dsum(&off.env_db, ModType::Base, None, &["SeismicMoreAoE"]);
                    let up = off.g_or("SeismicUpTimeRatio", 0.0, 2786);
                    q.nm(
                        "AreaOfEffect",
                        ModType::More,
                        (more_aoe / 100.0 * up).floor(),
                        "Avg Seismic Exert AoE",
                        0,
                        0,
                        vec![],
                    );
                }
                super::a2::calc_area_of_effect(off, &q, &skill_cfg);
                off.out().set_bool("SeismicCryCalculated", true);
            } else if name == "Battlemage's Cry" && !off.gtruthy("BattleMageCryCalculated") && !disable {
                warcry_times(off, j, "BattleMage");
                if off.has_type("Melee") {
                    warcry_uptime(
                        off,
                        j,
                        "BattleMage",
                        "BattleCryExertsCount",
                        "NumBattlemageExerts",
                        "BattlemageUpTimeRatio",
                        2809,
                    );
                }
                off.out().set_bool("BattleMageCryCalculated", true);
            }
        }

        // :2831-2838
        if q.flag(None, &["Condition:WarcryMaxHit"]) {
            let v = off.gval("MaxOffensiveWarcryEffect").cloned();
            off.set_out_value("AilmentWarcryEffect", v);
            off.set_sd("showAverage", Value::Bool(true));
            off.set_flag("showAverage", true);
            off.set_flag("notAverage", false);
        } else {
            let v = off.gval("OffensiveWarcryEffect").cloned();
            off.set_out_value("AilmentWarcryEffect", v);
        }
        // :2843-2846
        for cry in [
            "AncestralUpTimeRatio",
            "InfernalUpTimeRatio",
            "IntimidatingUpTimeRatio",
            "RallyingUpTimeRatio",
            "SeismicUpTimeRatio",
            "BattlemageUpTimeRatio",
        ] {
            let a = off.g_or("ExertedAttackUptimeRatio", 0.0, 2845);
            let b = off.g_or(cry, 0.0, 2845);
            off.gset("ExertedAttackUptimeRatio", max2(a, b));
        }
        // :2847-2856
        if off.breakdown {
            let mut t = BTable::lines([sf!("Maximum of:")]);
            for cry in [
                "AncestralUpTimeRatio",
                "InfernalUpTimeRatio",
                "IntimidatingUpTimeRatio",
                "RallyingUpTimeRatio",
                "SeismicUpTimeRatio",
                "BattlemageUpTimeRatio",
            ] {
                if off.gtruthy(cry) {
                    let name = cry.split_once("Up").map_or(cry, |(n, _)| n);
                    t.push(sf!("%d%% ^8(%s Cry Uptime)", off.g_or(cry, 0.0, 2852), name));
                }
            }
            t.push(sf!("= %d%%", off.g_or("ExertedAttackUptimeRatio", 0.0, 2855)));
            off.bd_put("ExertedAttackUptimeRatio", t);
        }
        // :2857-2888
        let uptime = num_at(off.gval("ExertedAttackUptimeRatio"), "ExertedAttackUptimeRatio", 2857);
        if uptime > 0.0 && !off.gtruthy("ExertedAttackUptimeRatioCalculated") {
            let c = Some(off.pcfg(pi));
            let inc_exerted = q.inc(c, &["ExertIncrease"]);
            let more_exerted = q.sum(ModType::More, c, &["ExertIncrease"]);
            let more_exerted_damage = q.sum(ModType::More, c, &["ExertAttackIncrease"]);
            let overexertion = q.sum(ModType::More, c, &["OverexertionExertAverageIncrease"]);
            let echoes = q.sum(ModType::More, c, &["EchoesExertAverageIncrease"]);
            let attack = mod_flag("Attack");
            if q.flag(None, &["Condition:WarcryMaxHit"]) {
                q.nm("Damage", ModType::Inc, inc_exerted, "Exerted Attacks", 0, 0, vec![]);
                q.nm("Damage", ModType::More, more_exerted, "Exerted Attacks", 0, 0, vec![]);
                q.nm(
                    "Damage",
                    ModType::More,
                    more_exerted_damage,
                    "Exerted Attack Damage",
                    attack,
                    0,
                    vec![],
                );
                q.nm(
                    "Damage",
                    ModType::More,
                    overexertion * exerting_warcry_count,
                    "Max Autoexertion Support",
                    0,
                    0,
                    vec![],
                );
                q.nm(
                    "Damage",
                    ModType::More,
                    echoes * exerting_warcry_count,
                    "Max Echoes of Creation",
                    0,
                    0,
                    vec![],
                );
            } else {
                let global_up = num_at(off.gval("GlobalWarcryUptimeRatio"), "GlobalWarcryUptimeRatio", 2873);
                q.nm(
                    "Damage",
                    ModType::Inc,
                    inc_exerted * uptime / 100.0,
                    "Uptime Scaled Exerted Attacks",
                    0,
                    0,
                    vec![],
                );
                q.nm(
                    "Damage",
                    ModType::More,
                    more_exerted * uptime / 100.0,
                    "Uptime Scaled Exerted Attacks",
                    0,
                    0,
                    vec![],
                );
                q.nm(
                    "Damage",
                    ModType::More,
                    more_exerted_damage * uptime / 100.0,
                    "Uptime Scaled Exerted Attack Damage",
                    attack,
                    0,
                    vec![],
                );
                q.nm(
                    "Damage",
                    ModType::More,
                    overexertion * global_up / 100.0,
                    "Uptime Scaled Autoexertion Support",
                    0,
                    0,
                    vec![],
                );
                q.nm(
                    "Damage",
                    ModType::More,
                    echoes * global_up / 100.0,
                    "Uptime Scaled Echoes of Creation",
                    0,
                    0,
                    vec![],
                );
            }
            let sc = Some(&skill_cfg);
            let avg = q.calc_mod(sc, &["ExertIncrease"]);
            let avg = avg
                * q.calc_mod(
                    sc,
                    &[
                        "ExertAttackIncrease",
                        "OverexertionExertAverageIncrease",
                        "EchoesExertAverageIncrease",
                    ],
                );
            off.gset("ExertedAttackAvgDmg", avg);
            off.gset("ExertedAttackHitEffect", avg * uptime / 100.0);
            off.gset("ExertedAttackMaxHitEffect", avg);
            if off.breakdown {
                let t = BTable::lines([
                    sf!("(%.2f ^8(average exerted damage)", avg),
                    sf!("x %.2f) ^8(uptime %%)", uptime / 100.0),
                    sf!("= %.2f", avg * uptime / 100.0),
                ]);
                off.bd_put("ExertedAttackHitEffect", t);
            }
            off.out().set_bool("ExertedAttackUptimeRatioCalculated", true);
        }
    }

    // :2892-2973 pacts
    if off.has_type("Spell") && !off.has_type("Brand") {
        let names = skill_names(off);
        for (j, (pact_name, disable)) in names.iter().enumerate() {
            let Some(key) = pact_name.strip_prefix("Pact of ").filter(|k| !k.is_empty()) else {
                continue;
            };
            let key = if key == "K'Tash" { "Ktash" } else { key };
            let is_vaal = off.has_type("Vaal");
            let applies = (key == "Beidat"
                && !is_vaal
                && !off.has_type("Channel")
                && (off.flag("projectile") || off.has_type("Cascadable") || off.flag("chaining") && !off.flag("projectile")))
                || (key == "Ghorr" && !is_vaal && off.has_type("DamageOverTime"))
                || (key == "Ktash" && is_vaal && (off.has_type("Damage") || off.has_type("DamageOverTime")))
                || (key == "Lycia" && !is_vaal && off.has_type("Channel") && off.flag("selfCast"));
            let calculated = format!("PactOf{key}Calculated");
            if !(applies && !off.gtruthy(&calculated) && !*disable) {
                continue;
            }
            let (vq, vcfg, vsd) = other_skill(off, j);
            let vc = Some(&vcfg);
            let (cooldown, _, _) = calc_skill_cooldown(&vq, &vcfg, &vsd);
            let cast_time = {
                let st = off.env.skills.as_ref().expect("skills on");
                let sk = st.skill(off.actor_skill(j));
                st.skills.effects[sk.active_effect.borrow().granted_effect]
                    .cast_time
                    .unwrap_or_else(|| panic!("{LUA}:2907: attempt to perform arithmetic on field 'castTime' (a nil value)"))
            };
            let cast_rate = 1.0 / cast_time * vq.calc_mod(vc, &["Speed"]) * crate::defence::action_speed_mod(&off.mod_db);
            let cast_time = 1.0 / min2(cast_rate, misc("ServerTickRate"));
            let count = vq.base(vc, &[&format!("{key}EmpoweredSpells")]);
            let stored = stored_uses(&vq, &vcfg, &vsd, 2910);
            // :2938-2940 repeated spells keep the empowerment of the initial cast (3dbca4463 #10348)
            let speed = num_at(off.gval("Speed"), "Speed", 2939);
            let repeats = num_at(off.gval("Repeats"), "Repeats", 2939);
            let empowered_use_rate = speed / repeats;
            let mut uptime = if empowered_use_rate == 0.0 {
                100.0
            } else {
                min2((count / empowered_use_rate) / (cooldown + cast_time), 1.0) * 100.0
            };
            uptime = min2(100.0, uptime * stored);
            let effect = if q.flag(None, &["Condition:PactMaxHit"]) { 100.0 } else { uptime };
            let effect_mult = effect / 100.0;
            off.out().set_bool("CreatePactOffensiveCalcSection", true);
            off.gset(&format!("PactOf{key}Cooldown"), cooldown);
            off.gset(&format!("PactOf{key}CastTime"), cast_time);
            off.gset(&format!("{key}EmpoweredCount"), count);
            off.gset(&format!("{key}UpTimeRatio"), uptime);
            if off.breakdown {
                let t = BTable::lines([
                    sf!("(%.2f ^8(number of Empowered)", count),
                    sf!("/ %.2f) ^8(non-repeated casts per second)", empowered_use_rate),
                    sf!("/ (%.2f ^8(pact cooldown)", cooldown),
                    sf!("+ %.2f) ^8(pact cast time)", cast_time),
                    sf!("* %.2f ^8(stored uses)", stored),
                    sf!("= %.2f%%", uptime),
                ]);
                off.bd_put(&format!("{key}UpTimeRatio"), t);
            }
            if let Some(d) = vq.list(vc, &[&format!("{key}PactDamage")]).first() {
                let Value::Mod(m) = d.get("mod") else {
                    panic!("{LUA}:2934: attempt to index local 'mod' (a nil value)");
                };
                let v = m
                    .value
                    .as_num()
                    .unwrap_or_else(|| panic!("{LUA}:2935: attempt to perform arithmetic on field 'value'"));
                q.new_mod(
                    &m.name,
                    m.ty,
                    Value::Num(v * effect_mult),
                    Some(&format!("Uptime Scaled {pact_name}")),
                    m.flags,
                    m.keyword_flags,
                    m.tags.clone(),
                );
            }
            if key == "Beidat" {
                let sc = Some(&skill_cfg);
                if off.flag("projectile") && !q.flag(sc, &["NoAdditionalProjectiles"]) && !q.flag(sc, &["SingleProjectile"]) {
                    let v = vq.base(vc, &["BeidatAdditionalProjectiles"]) * effect_mult;
                    off.gset("BeidatAdditionalProjectiles", v);
                    let p = num_at(off.gval("ProjectileCount"), "ProjectileCount", 2942) + v;
                    off.gset("ProjectileCount", p);
                }
                if off.flag("chaining") && !off.flag("projectile") && !q.flag(sc, &["CannotChain"]) && !q.flag(sc, &["NoAdditionalChains"])
                {
                    // :2974 `* skillModList:More(skillCfg, "ChainCountMax")` (3dbca4463)
                    let v = vq.base(vc, &["BeidatAdditionalBeamChains"]) * effect_mult * q.more(sc, &["ChainCountMax"]);
                    off.gset("BeidatAdditionalBeamChains", v);
                    let cm = num_at(off.gval("ChainMax"), "ChainMax", 2946) + v;
                    off.gset("ChainMax", cm);
                    off.gset("ChainMaxString", cm);
                    let ch = num_at(off.gval("Chain"), "Chain", 2948);
                    off.gset("ChainRemaining", max2(0.0, cm - ch));
                }
                if off.has_type("Cascadable") {
                    let v = vq.base(vc, &["BeidatAdditionalCascades"]) * effect_mult;
                    off.gset("BeidatAdditionalCascades", v);
                }
            } else if key == "Ktash" {
                let v = vq.base(vc, &["KtashPactSoulRefundChance"]) * effect_mult;
                off.gset("KtashSoulRefundChance", v);
                if off.gtruthy("SoulGainPreventionDuration") {
                    let duration = num_at(off.gval("SoulGainPreventionDuration"), "SoulGainPreventionDuration", 2958);
                    let duration_mod = 1.0 + vq.base(vc, &["KtashPactSoulGainPrevention"]) * effect_mult / 100.0;
                    let tick = misc("ServerTickRate");
                    off.gset(
                        "SoulGainPreventionDuration",
                        max2((duration * duration_mod * tick).ceil(), 1.0) / tick,
                    );
                    if off.breakdown {
                        let t = BTable::lines([
                            sf!("%.3fs ^8(before Pact of K'Tash)", duration),
                            sf!("x %.4f ^8(average Pact modifier)", duration_mod),
                            "rounded up to nearest server tick".to_owned(),
                            sf!("= %.3fs", max2((duration * duration_mod * tick).ceil(), 1.0) / tick),
                        ]);
                        off.bd_put("SoulGainPreventionDuration", t);
                    }
                }
            }
            off.out().set_bool(&calculated, true);
        }
    }
}

/// :2976-3033 Ruthless and Fist of War.
fn ruthless_fist_of_war(off: &mut Off<'_>, pi: usize) {
    let q = off.q.clone();
    off.pset_num(pi, "RuthlessBlowHitEffect", 1.0);
    off.pset_num(pi, "RuthlessBlowAilmentEffect", 1.0);
    off.pset_num(pi, "FistOfWarDamageEffect", 1.0);
    if !off.env.mode_combat {
        return;
    }
    // `env.configInput.ruthlessSupportMode or "AVERAGE"`
    let ruthless_effect = match config(off, "ruthlessSupportMode") {
        None | Some(pob_build::ConfigValue::Bool(false)) => Some("AVERAGE".to_owned()),
        Some(pob_build::ConfigValue::Str(s)) => Some(s),
        Some(_) => None,
    };
    let max_count = q.base(Some(off.pcfg(pi)), &["RuthlessBlowMaxCount"]);
    off.pset_num(pi, "RuthlessBlowMaxCount", max_count);
    let used_by_mirage = off
        .cfg
        .skill_cond
        .as_ref()
        .and_then(|c| c.get("usedByMirage"))
        .copied()
        .unwrap_or(false);
    if max_count > 0.0 && (!used_by_mirage || off.sd_num("mirageUses", 2983).unwrap_or(0.0) > max_count) {
        match ruthless_effect.as_deref() {
            Some("AVERAGE") => off.pset_num(pi, "RuthlessBlowChance", round(100.0 / max_count, None)),
            Some("MAX") => {
                off.pset_num(pi, "RuthlessBlowChance", 100.0);
                let d = off
                    .sd_num("dpsMultiplier", 2988)
                    .unwrap_or_else(|| panic!("{LUA}:2988: attempt to perform arithmetic on field 'dpsMultiplier' (a nil value)"));
                off.set_sd("dpsMultiplier", Value::Num(d / max_count));
            }
            _ => {}
        }
    } else {
        off.pset_num(pi, "RuthlessBlowChance", 0.0);
    }
    let hit_mult = 1.0 + q.base(Some(off.pcfg(pi)), &["RuthlessBlowHitMultiplier"]) / 100.0;
    off.pset_num(pi, "RuthlessBlowHitMultiplier", hit_mult);
    let ail_mult = 1.0 + q.base(Some(off.pcfg(pi)), &["RuthlessBlowAilmentMultiplier"]) / 100.0;
    off.pset_num(pi, "RuthlessBlowAilmentMultiplier", ail_mult);
    let chance = num_at(off.pval(pi, "RuthlessBlowChance"), "RuthlessBlowChance", 2995);
    off.pset_num(pi, "RuthlessBlowHitEffect", 1.0 - chance / 100.0 + chance / 100.0 * hit_mult);
    off.pset_num(pi, "RuthlessBlowAilmentEffect", 1.0 - chance / 100.0 + chance / 100.0 * ail_mult);

    let cooldown = q.base(Some(off.pcfg(pi)), &["FistOfWarCooldown"]);
    off.gset("FistOfWarCooldown", cooldown);
    if cooldown != 0.0 && off.has_type("Slam") && !off.has_type("Vaal") && !off.has_type("OtherThingUsesSkill") {
        let mult = q.base(None, &["FistOfWarDamageMultiplier"]) / 100.0;
        off.gset("FistOfWarDamageMultiplier", mult);
        let speed = num_at(off.gval("Speed"), "Speed", 3002);
        let up = min2((1.0 / speed) / cooldown, 1.0) * 100.0;
        off.gset("FistOfWarUptimeRatio", up);
        if off.breakdown {
            let t = BTable::lines([
                sf!("min( (1 / %.2f) ^8(second per attack)", speed),
                sf!("/ %.2f, 1) ^8(fist of war cooldown)", cooldown),
                sf!("= %d%%", up),
            ]);
            off.bd_put("FistOfWarUptimeRatio", t);
        }
        off.gset("AvgFistOfWarDamage", mult);
        let avg_effect = 1.0 + mult * (up / 100.0);
        off.gset("AvgFistOfWarDamageEffect", avg_effect);
        if off.breakdown {
            let t = BTable::lines([
                sf!("1 + (%.2f ^8(fist of war damage multiplier)", mult),
                sf!("x %.2f) ^8(fist of war uptime ratio)", up / 100.0),
                sf!("= %.2f", avg_effect),
            ]);
            off.bd_put("AvgFistOfWarDamageEffect", t);
        }
        off.gset("MaxFistOfWarDamageEffect", 1.0 + mult);
        if q.flag(None, &["Condition:WarcryMaxHit"]) {
            off.pset_num(pi, "FistOfWarDamageEffect", 1.0 + mult);
            let inc_aoe = q.base(None, &["FistOfWarIncAoE"]);
            q.nm("AreaOfEffect", ModType::Inc, inc_aoe, "Max Fist of War Boosted AoE", 0, 0, vec![]);
        } else {
            off.pset_num(pi, "FistOfWarDamageEffect", avg_effect);
            let inc_aoe = q.base(None, &["FistOfWarIncAoE"]);
            q.nm(
                "AreaOfEffect",
                ModType::Inc,
                (inc_aoe / 100.0 * up).floor(),
                "Avg Fist Of War Boosted AoE",
                0,
                0,
                vec![],
            );
        }
        let skill_cfg = off.cfg.clone();
        super::a2::calc_area_of_effect(off, &q, &skill_cfg);
        let t = off.g_or("TheoreticalOffensiveWarcryEffect", 0.0, 3028) * avg_effect;
        off.gset("TheoreticalOffensiveWarcryEffect", t);
        let t = off.g_or("TheoreticalMaxOffensiveWarcryEffect", 0.0, 3029) * (1.0 + mult);
        off.gset("TheoreticalMaxOffensiveWarcryEffect", t);
    } else {
        off.pset_num(pi, "FistOfWarDamageEffect", 1.0);
    }
}

/// :3041-3275 crit chance, crit multiplier, crit effect.
#[allow(clippy::too_many_lines)]
fn crits(off: &mut Off<'_>, pi: usize) {
    let q = off.q.clone();
    let eff = off.env.mode_effective;
    let set_zero = |off: &mut Off<'_>| {
        for (k, v) in [
            ("PreEffectiveCritChance", 0.0),
            ("CritChance", 0.0),
            ("CritMultiplier", 0.0),
            ("BonusCritDotMultiplier", 0.0),
            ("CritEffect", 1.0),
        ] {
            off.pset_num(pi, k, v);
        }
    };
    if q.flag(Some(off.pcfg(pi)), &["NeverCrit"]) {
        set_zero(off);
    } else if q.flag(Some(off.pcfg(pi)), &["SpellSkillsCannotDealCriticalStrikesExceptOnFinalRepeat"]) {
        if or_num(off.pval(pi, "Repeats"), 1.0, "Repeats", 3049) == 1.0 {
            set_zero(off);
        } else if q.flag(Some(off.pcfg(pi)), &["SpellSkillsAlwaysDealCriticalStrikesOnFinalRepeat"]) {
            if config_is(off, "repeatMode", "None") {
                off.pset_num(pi, "PreEffectiveCritChance", 0.0);
                off.pset_num(pi, "CritChance", 0.0);
            } else if config_is(off, "repeatMode", "AVERAGE") {
                let r = num_at(off.pval(pi, "Repeats"), "Repeats", 3060);
                off.pset_num(pi, "PreEffectiveCritChance", 100.0 / r);
                off.pset_num(pi, "CritChance", 100.0 / r);
                // :3062-3068
                if off.breakdown {
                    let t = BTable::lines([
                        sf!("100%%"),
                        sf!("/ %d ^8(number of repeats)", r),
                        sf!("= %.2f%% average critical strike chance", 100.0 / r),
                    ]);
                    off.pbd_put(pi, "CritChance", t);
                }
            } else {
                off.pset_num(pi, "PreEffectiveCritChance", 100.0);
                off.pset_num(pi, "CritChance", 100.0);
            }
        }
    } else {
        let mut crit_override = q.override_(Some(off.pcfg(pi)), &["CritChance"]).filter(Value::truthy);
        if q.flag(Some(off.pcfg(pi)), &["MainHandCritIsEqualToParent"]) {
            // :3079 `actor.parent.output.MainHand and actor.parent.output.MainHand.CritChance or actor.parent.weaponData1.CritChance`
            if off.who != crate::perform::Who::Minion {
                panic!("{LUA}:3079: attempt to index field 'parent' (a nil value)");
            }
            let from_hand = off.player_state().hands.as_ref().map(|h| {
                h[0].map.get("CritChance").map(|v| match v {
                    OutValue::Num(x) => Value::Num(*x),
                    OutValue::Bool(b) => Value::Bool(*b),
                    OutValue::Str(s) => Value::Str(s.clone()),
                })
            });
            crit_override = from_hand
                .flatten()
                .filter(Value::truthy)
                .or_else(|| Some(off.env.player.borrow().weapon_data1.get("CritChance")).filter(Value::truthy));
        } else if q.flag(Some(off.pcfg(pi)), &["MainHandCritIsEqualToPartyMember"]) {
            // :3081 `actor.partyMembers` -- nil on a minion
            if off.who == crate::perform::Who::Minion {
                panic!("{LUA}:3081: attempt to index field 'partyMembers' (a nil value)");
            }
            // the PartyTab stand-in has an empty output and no weaponData1 (P5): `... or 0`
            crit_override = Some(Value::Num(0.0));
        }
        // `critOverride or source.CritChance or 0`
        let source_crit = match &off.passes[pi].source {
            super::Source::SkillData => off.sd("CritChance"),
            super::Source::Weapon(d) => d.get("CritChance"),
        };
        let mut base_crit: Value = match &crit_override {
            Some(v) => v.clone(),
            None if source_crit.truthy() => source_crit,
            None => Value::Num(0.0),
        };
        let base_crit_from_main_hand = q.flag(Some(off.pcfg(pi)), &["BaseCritFromMainHand"]);
        let base_crit_from_parent = q.flag(Some(off.pcfg(pi)), &["AttackCritIsEqualToParentMainHand"]);
        if base_crit_from_main_hand {
            base_crit = off.actor().weapon_data1.get("CritChance");
        } else if base_crit_from_parent {
            // :3090 `actor.parent.weaponData1 and actor.parent.weaponData1.CritChance or baseCrit`
            if off.who != crate::perform::Who::Minion {
                panic!("{LUA}:3090: attempt to index field 'parent' (a nil value)");
            }
            let c = off.env.player.borrow().weapon_data1.get("CritChance");
            if c.truthy() {
                base_crit = c;
            }
        }
        if crit_override.as_ref().and_then(Value::as_num) == Some(100.0) {
            off.pset_num(pi, "PreEffectiveCritChance", 100.0);
            off.pset_num(pi, "PreBifurcateCritChance", 100.0);
            off.pset_num(pi, "CritChance", 100.0);
        } else {
            let (mut base, mut inc, mut more) = (0.0, 0.0, 1.0);
            if crit_override.is_none() {
                let c = Some(off.pcfg(pi));
                base = q.base(c, &["CritChance"])
                    + if eff {
                        dsum(&off.enemy_db, ModType::Base, None, &["SelfCritChance"])
                    } else {
                        0.0
                    };
                inc = q.inc(c, &["CritChance"])
                    + if eff {
                        dsum(&off.enemy_db, ModType::Inc, None, &["SelfCritChance"])
                    } else {
                        0.0
                    };
                more = q.more(c, &["CritChance"])
                    * if eff {
                        dmore(&off.enemy_db, None, &["SelfCritChance"])
                    } else {
                        1.0
                    };
            }
            let base_crit_n = base_crit
                .as_num()
                .unwrap_or_else(|| panic!("{LUA}:3107: attempt to perform arithmetic on local 'baseCrit' ({base_crit:?})"));
            let mut cc = round((base_crit_n + base) * (1.0 + inc / 100.0) * more * 100.0, None) / 100.0;
            let cap = match q.override_num(None, &["CritChanceCap"], "Modules/CalcOffence.lua:3109") {
                Some(v) => v,
                None => q.base(Some(off.pcfg(pi)), &["CritChanceCap"]),
            };
            cc = min2(cc, cap);
            if base_crit_n + base > 0.0 {
                cc = max2(cc, 0.0);
            }
            let pre_cap = round((base_crit_n + base) * (1.0 + inc / 100.0) * more * 100.0, None) / 100.0;
            off.pset_num(pi, "PreEffectiveCritChance", cc);
            let pre_effective = cc;
            let pre_lucky = cc;
            let mut crit_rolls = 0.0;
            if eff && q.flag(Some(off.pcfg(pi)), &["CritChanceLucky"]) {
                crit_rolls += 1.0;
            }
            if q.flag(Some(&off.cfg), &["ExtremeLuck"]) {
                crit_rolls *= 2.0;
            }
            if crit_rolls != 0.0 {
                if dflag(&off.mod_db, None, &["Unexciting"]) {
                    let p = cc / 100.0;
                    cc = (3.0 * p.powf(2.0) - 2.0 * p.powf(3.0)) * 100.0;
                } else {
                    cc = (1.0 - (1.0 - cc / 100.0).powf(crit_rolls + 1.0)) * 100.0;
                }
            }
            off.pset_num(pi, "PreBifurcateCritChance", cc);
            let pre_bifurcate = cc;
            if eff && q.flag(Some(off.pcfg(pi)), &["BifurcateCrit"]) {
                cc = (1.0 - (1.0 - cc / 100.0).powf(2.0)) * 100.0;
            }
            let mut pre_hit_check = cc;
            let pre_skill_use = cc;
            let mut ahc = 100.0;
            if eff {
                if q.flag(Some(&off.cfg), &["Every3UseCrit"]) {
                    cc = (2.0 * cc + 100.0) / 3.0;
                }
                if q.flag(Some(&off.cfg), &["Every5UseCrit"]) {
                    cc = (4.0 * cc + 100.0) / 5.0;
                }
                pre_hit_check = cc;
                ahc = num_at(off.pval(pi, "AccuracyHitChance"), "AccuracyHitChance", 3146);
                cc = cc * ahc / 100.0;
            }
            off.pset_num(pi, "CritChance", cc);
            // :3148-3200
            if off.breakdown && cc != base_crit_n {
                let mut t = BTable::new();
                let from = if base_crit_from_main_hand {
                    " from main weapon"
                } else if base_crit_from_parent {
                    " from parent main weapon"
                } else {
                    ""
                };
                if base != 0.0 {
                    t.push(sf!("(%g + %g) ^8(base%s)", base_crit_n, base, from));
                } else {
                    t.push(sf!("%g ^8(base%s)", base_crit_n + base, from));
                }
                if inc != 0.0 {
                    t.push(format!("{} ^8(increased/reduced)", sf!("x %.2f", 1.0 + inc / 100.0)));
                }
                if more != 1.0 {
                    t.push(format!("{} ^8(more/less)", sf!("x %.2f", more)));
                }
                t.push(sf!("= %.2f%% ^8(crit chance)", pre_effective));
                if pre_cap > 100.0 {
                    let over_cap = pre_cap - 100.0;
                    t.push(sf!(
                        "Crit is overcapped by %.2f%% (%d%% increased Critical Strike Chance)",
                        over_cap,
                        over_cap / more / (base_crit_n + base) * 100.0
                    ));
                }
                let pre = t.clone();
                if eff {
                    if crit_rolls != 0.0 {
                        if q.flag(Some(&off.cfg), &["Unexciting"]) {
                            t.push("Crit Chance is Unexciting:");
                            t.push(sf!("(3 x %.4f^ 2) - (2 x %.4f^ 3)", pre_lucky / 100.0, pre_lucky / 100.0));
                        } else {
                            t.push("Crit Chance is Lucky:");
                            t.push(sf!("1 - (1 - %.4f)^ %d", pre_lucky / 100.0, crit_rolls + 1.0));
                        }
                        t.push(sf!("= %.2f%%", pre_bifurcate));
                    }
                    if q.flag(Some(off.pcfg(pi)), &["BifurcateCrit"]) {
                        t.push("Critical Strike Bifurcates:");
                        t.push(sf!("1 - (1 - %.4f) x (1 - %.4f)", pre_bifurcate / 100.0, pre_bifurcate / 100.0));
                        t.push(sf!("= %.2f%%", pre_skill_use));
                    }
                    let e3 = q.flag(Some(&off.cfg), &["Every3UseCrit"]);
                    if e3 {
                        t.push(sf!(
                            "+ %.2f%% ^8(crit every 3rd use)",
                            (2.0 * pre_skill_use + 100.0) / 3.0 - pre_skill_use
                        ));
                    }
                    let e5 = q.flag(Some(&off.cfg), &["Every5UseCrit"]);
                    if e5 {
                        t.push(sf!(
                            "+ %.2f%% ^8(crit every 5th use)",
                            (4.0 * pre_skill_use + 100.0) / 5.0 - pre_skill_use
                        ));
                    }
                    if e3 || q.flag(Some(&off.cfg), &["Every5UseCrit"]) {
                        t.push(sf!("= %.2f%%", pre_hit_check));
                    }
                }
                if eff && ahc < 100.0 {
                    t.push("Crit confirmation roll:");
                    t.push(sf!("%.2f%%", pre_hit_check));
                    t.push(sf!("x %.2f ^8(chance to hit)", ahc / 100.0));
                    t.push(sf!("= %.2f%%", cc));
                }
                if let Some(bd) = off.pbd(pi) {
                    bd.put("CritChance", t);
                    bd.put("PreEffectiveCritChance", pre);
                }
            }
        }
    }

    // :3203-3272
    if !matches!(off.pval(pi, "CritEffect"), None | Some(OutValue::Bool(false))) {
        // CritEffect already set
    } else {
        if q.flag(Some(off.pcfg(pi)), &["NoCritMultiplier"]) {
            off.pset_num(pi, "CritMultiplier", 1.0);
        } else {
            let mut extra = q.base(Some(off.pcfg(pi)), &["CritMultiplier"]) / 100.0;
            if let Some(m) = q.override_num(Some(&off.cfg), &["CritMultiplier"], "Modules/CalcOffence.lua:3210") {
                extra = (m - 100.0) / 100.0;
            }
            if eff {
                let enemy_inc = 1.0 + dsum(&off.enemy_db, ModType::Inc, None, &["SelfCritMultiplier"]) / 100.0;
                extra += dsum(&off.enemy_db, ModType::Base, None, &["SelfCritMultiplier"]) / 100.0;
                extra = round(extra * enemy_inc, Some(2.0));
                // :3216-3222
                if off.breakdown && enemy_inc != 1.0 {
                    let t = BTable::lines([
                        sf!(
                            "%d%% ^8(additional extra damage)",
                            (dsum(&off.enemy_db, ModType::Base, None, &["SelfCritMultiplier"])
                                + q.base(Some(off.pcfg(pi)), &["CritMultiplier"]))
                                / 100.0
                        ),
                        sf!("x %.2f ^8(increased/reduced extra crit damage taken by enemy)", enemy_inc),
                        sf!("= %d%% ^8(extra crit damage)", extra * 100.0),
                    ]);
                    off.pbd_put(pi, "CritMultiplier", t);
                }
            }
            let crit_override = q.override_(Some(off.pcfg(pi)), &["CritChance"]).filter(Value::truthy);
            if eff
                && q.flag(Some(off.pcfg(pi)), &["BifurcateCrit"])
                && !matches!(off.pval(pi, "PreBifurcateCritChance"), None | Some(OutValue::Bool(false)))
                && crit_override.as_ref().and_then(Value::as_num) != Some(100.0)
            {
                let pct = num_at(off.pval(pi, "PreBifurcateCritChance"), "PreBifurcateCritChance", 3229);
                let mut bifurcate_multi_chance = pct.powf(2.0) / 100.0;
                let effective = num_at(off.pval(pi, "CritChance"), "CritChance", 3240);
                let mut use_chance = 1.0;
                if q.flag(Some(&off.cfg), &["Every3UseCrit"]) {
                    use_chance = use_chance * 2.0 / 3.0;
                }
                if q.flag(Some(&off.cfg), &["Every5UseCrit"]) {
                    use_chance = use_chance * 4.0 / 5.0;
                }
                bifurcate_multi_chance *= use_chance;
                let conditional = if effective > 0.0 { bifurcate_multi_chance / effective } else { 0.0 };
                off.pset_num(pi, "CritBifurcates", 1.0 + conditional);
                // :3241-3251
                if off.breakdown {
                    let mut t = BTable::lines([sf!("%.2f%% ^8(pre-bifurcate crit chance)", pct), sf!("x %.2f%%", pct)]);
                    if use_chance < 1.0 {
                        t.push(sf!("x %.2f%% ^8(uses that can bifurcate)", use_chance * 100.0));
                    }
                    t.push(sf!("= %.2f%% ^8(chance both crit rolls succeed)", bifurcate_multi_chance));
                    t.push(sf!("/ %.2f%% ^8(effective crit chance)", effective));
                    t.push(sf!("= %.2f ^8(crit Bifurcates effect)", 1.0 + conditional));
                    off.pbd_put(pi, "CritBifurcates", t);
                }
                let bonus = conditional * extra;
                extra += bonus;
                q.nm(
                    "CritMultiplier",
                    ModType::More,
                    floor(conditional * 100.0, Some(2.0)),
                    "Bifurcated Crit Damage Bonus",
                    mod_flag("Hit"),
                    0,
                    vec![],
                );
            }
            off.pset_num(pi, "CritMultiplier", 1.0 + max2(0.0, extra));
        }
        let pct = num_at(off.pval(pi, "CritChance"), "CritChance", 3262) / 100.0;
        let mult = num_at(off.pval(pi, "CritMultiplier"), "CritMultiplier", 3263);
        let effect = 1.0 - pct + pct * mult;
        off.pset_num(pi, "CritEffect", effect);
        // :3266-3272
        if off.breakdown && effect != 1.0 {
            let t = BTable::lines([
                sf!("(1 - %.4f) ^8(portion of damage from non-crits)", pct),
                sf!("+ [ (%.4f x %g) ^8(portion of damage from crits)", pct, mult),
                sf!("= %.3f", effect),
            ]);
            off.pbd_put(pi, "CritEffect", t);
        }
        let c = Some(off.pcfg(pi));
        let bonus = (q.base(c, &["CritMultiplier"]) - 50.0) * q.base(c, &["CritMultiplierAppliesToDegen"]) / 10000.0;
        off.pset_num(pi, "BonusCritDotMultiplier", bonus);
    }
    if off.pval(pi, "CritChance") != Some(&OutValue::Num(0.0)) {
        q.list.borrow_mut().conditions.insert(Str::from("CritInPast8Sec"), true);
    }
}

/// :3277-3313 triple / double damage, the hit rate, culling and reservation DPS; returns `hitRate`.
fn double_triple_cull(off: &mut Off<'_>, pi: usize) -> f64 {
    let q = off.q.clone();
    let eff = off.env.mode_effective;
    off.pset_num(pi, "ScaledDamageEffect", 1.0);
    let cc = num_at(off.pval(pi, "CritChance"), "CritChance", 3281);
    let c = Some(off.pcfg(pi));
    let triple_on_crit = min2(q.base(c, &["TripleDamageChanceOnCrit"]), 100.0);
    // `Sum(...) or 0 + (...)`: `or` binds loosest and a Sum is never nil -- the right-hand side is never evaluated (L4)
    let triple = min2(q.base(c, &["TripleDamageChance"]), 100.0);
    let triple_effect = 2.0 * triple / 100.0;
    let double_on_crit = min2(q.base(c, &["DoubleDamageChanceOnCrit"]), 100.0);
    let enemy_dd = if eff {
        dsum(&off.enemy_db, ModType::Base, c, &["SelfDoubleDamageChance"])
    } else {
        0.0
    };
    let mut double = min2(q.base(c, &["DoubleDamageChance"]) + enemy_dd + (double_on_crit * cc / 100.0), 100.0);
    off.pset_num(pi, "TripleDamageChanceOnCrit", triple_on_crit);
    off.pset_num(pi, "TripleDamageChance", triple);
    off.pset_num(pi, "TripleDamageEffect", triple_effect);
    off.pset_num(pi, "DoubleDamageChanceOnCrit", double_on_crit);
    if off.gtruthy("IntimidatingUpTimeRatio") && q.flag(None, &["Condition:WarcryMaxHit"]) {
        double = 100.0;
    } else if off.gtruthy("IntimidatingUpTimeRatio") {
        double = min2(
            double + num_at(off.gval("IntimidatingUpTimeRatio"), "IntimidatingUpTimeRatio", 3290),
            100.0,
        );
    }
    if triple > 0.0 {
        double = max2(double - triple * double / 100.0, 0.0);
    }
    off.pset_num(pi, "DoubleDamageChance", double);
    let double_effect = double / 100.0;
    off.pset_num(pi, "DoubleDamageEffect", double_effect);
    off.pset_num(pi, "ScaledDamageEffect", 1.0 * (1.0 + double_effect + triple_effect));

    let hit_chance = num_at(off.pval(pi, "HitChance"), "HitChance", 3300);
    let dps_mult = off
        .sd_num("dpsMultiplier", 3300)
        .unwrap_or_else(|| panic!("{LUA}:3300: attempt to perform arithmetic on field 'dpsMultiplier' (a nil value)"));
    let hit_rate = hit_chance / 100.0 * off.g_hit_speed(3300) * dps_mult;

    let c = Some(off.pcfg(pi));
    let mut critical_cull = q.max(c, &["CriticalCullPercent"]).unwrap_or(0.0);
    if critical_cull > 0.0 {
        critical_cull = min2(critical_cull, critical_cull * (1.0 - (1.0 - cc / 100.0).powf(hit_rate)));
    }
    let regular_cull = q.max(c, &["CullPercent"]).unwrap_or(0.0);
    let max_cull = max2(critical_cull, regular_cull);
    off.gset("CullPercent", max_cull);
    off.gset("CullMultiplier", 100.0 / (100.0 - max_cull));
    let lrp = dsum(&off.enemy_db, ModType::Base, None, &["LifeReservationPercent"]);
    off.gset("ReservationDpsMultiplier", 100.0 / (100.0 - lrp));
    hit_rate
}

/// :3317-3344 Cryogenesis: every added damage mod (but Elemental Hit's) becomes the redirect type.
fn added_damage_redirect(off: &mut Off<'_>, pi: usize) {
    let q = off.q.clone();
    let redirect = if q.flag(Some(off.pcfg(pi)), &["AllAddedDamageAsLightning"]) {
        "Lightning"
    } else if q.flag(Some(off.pcfg(pi)), &["AllAddedDamageAsCold"]) {
        "Cold"
    } else {
        return;
    };
    for dt in super::helpers::DMG_TYPE_LIST {
        if dt == redirect {
            continue;
        }
        for bound in ["Min", "Max"] {
            let old = format!("{dt}{bound}");
            let new = format!("{redirect}{bound}");
            for e in q.tabulate(Some(ModType::Base), Some(off.pcfg(pi)), &[&old]) {
                let m = &e.mod_;
                let src = m
                    .source
                    .as_deref()
                    .unwrap_or_else(|| panic!("{LUA}:3331: attempt to index field 'source' (a nil value)"));
                if src.contains("ElementalHit") {
                    continue;
                }
                let mut tags = vec![tag(&[("type", Value::str("Cryogenesis Added Damage"))])];
                tags.extend(m.tags.iter().cloned());
                q.list.borrow_mut().convert_mod(
                    &old,
                    pob_mods::create_mod(
                        &new,
                        Some(ModType::Base),
                        m.value.clone(),
                        Some(src),
                        m.flags,
                        m.keyword_flags,
                        tags,
                    ),
                );
            }
        }
    }
}

/// :2606-3955 the damage pass loop.
pub(super) fn damage_passes(off: &mut Off<'_>, quantity_multiplier: f64) {
    for pi in 0..off.passes.len() {
        off.cur_pass = pi;
        // :2616-2624
        let exerted_double_damage = dsum(&off.env_db, ModType::Base, Some(off.pcfg(pi)), &["ExertDoubleDamageChance"]);
        let exerting_warcry_count = dsum(&off.env_db, ModType::Base, None, &["Multiplier:ExertingWarcryCount"]);
        for (k, v) in [
            ("OffensiveWarcryEffect", 1.0),
            ("MaxOffensiveWarcryEffect", 1.0),
            ("TheoreticalOffensiveWarcryEffect", 1.0),
            ("TheoreticalMaxOffensiveWarcryEffect", 1.0),
            ("RallyingHitEffect", 1.0),
            ("AilmentWarcryEffect", 1.0),
            ("GlobalWarcryUptimeRatio", 0.0),
        ] {
            off.gset(k, v);
        }
        if off.env.mode_buffs {
            exerts_and_pacts(off, pi, exerted_double_damage, exerting_warcry_count);
        }
        ruthless_fist_of_war(off, pi);
        // :3037-3039
        let name = {
            let st = off.env.skills.as_ref().expect("skills on");
            st.skills.effects[off.sk().active_effect.borrow().granted_effect].name.clone()
        };
        if name == "Explosive Arrow" {
            let id = {
                let st = off.env.skills.as_ref().expect("skills on");
                st.skills.effects[off.sk().active_effect.borrow().granted_effect].id.clone()
            };
            crate::skill_funcs::call(off, &id, "explosiveArrowFunc");
        }
        crits(off, pi);
        let hit_rate = double_triple_cull(off, pi);
        run_skill_func(off, "postCritFunc");
        added_damage_redirect(off, pi);
        super::c2::hit_damage(off, pi, hit_rate, quantity_multiplier);
    }
}
