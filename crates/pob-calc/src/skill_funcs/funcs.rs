//! Hand ports of the skill functions of `Data/Skills/*.lua` (`initialFunc`, `preSkillTypeFunc`, `preDamageFunc`) -- one Rust function
//! per distinct body (several skills share a body: the registry maps each `<src>:<from>` to one of these). The `breakdown` rows are built
//! as upstream (R8.5b, gate `tests/breakdown.rs` on the fn-synth scenarios).

use pob_lua::number::{max2, min2};
use pob_mods::{ModType, Table, Value};

use crate::breakdown::BTable;
use crate::offence::Off;
use crate::setup::{misc, mod_flag, tag};
use crate::sf;

/// `activeSkill.skillPart == n`.
fn part(off: &Off<'_>, n: f64) -> bool {
    off.sk().skill_part == Some(n)
}

/// `skillData[k]` in arithmetic (nil is the Lua error at `src`).
fn sdn(off: &Off<'_>, k: &str, src: &str) -> f64 {
    off.sd_num(k, 0)
        .unwrap_or_else(|| panic!("{src}: attempt to perform arithmetic on field '{k}' (a nil value)"))
}

/// `skillData[k] or d`.
fn sd_or(off: &Off<'_>, k: &str, d: f64) -> f64 {
    off.sd_num(k, 0).unwrap_or(d)
}

/// `output[k]` in arithmetic.
fn on(off: &Off<'_>, k: &str) -> f64 {
    off.out_num(k, 0)
}

fn set(off: &mut Off<'_>, k: &str, v: f64) {
    off.set_sd(k, Value::Num(v));
}

/// `skillModList:Sum(type, skillCfg, ...)`.
fn sum(off: &Off<'_>, ty: ModType, names: &[&str]) -> f64 {
    off.q.sum(ty, Some(&off.cfg), names)
}

fn more(off: &Off<'_>, names: &[&str]) -> f64 {
    off.q.more(Some(&off.cfg), names)
}

/// `{ type = "DistanceRamp", ramp = {{a,b},{c,d}} }`.
fn distance_ramp(pts: [(f64, f64); 2]) -> pob_mods::Tag {
    let arr = pts
        .iter()
        .map(|(a, b)| {
            Value::table(Table {
                array: vec![Value::Num(*a), Value::Num(*b)],
                ..Table::default()
            })
        })
        .collect();
    tag(&[
        ("type", Value::str("DistanceRamp")),
        (
            "ramp",
            Value::table(Table {
                array: arr,
                ..Table::default()
            }),
        ),
    ])
}

/// `(skillData.dpsMultiplier or 1) * m`.
fn mul_dps(off: &mut Off<'_>, m: f64) {
    let v = sd_or(off, "dpsMultiplier", 1.0) * m;
    set(off, "dpsMultiplier", v);
}

/// `skillData.dpsMultiplier = ... ; output.SkillDPSMultiplier = (output.SkillDPSMultiplier or 1) * m` (the `~= 1` guard is the caller's).
fn mul_dps_out(off: &mut Off<'_>, m: f64) {
    mul_dps(off, m);
    let o = off.st.player.output.opt_num("SkillDPSMultiplier").unwrap_or(1.0) * m;
    off.out().set_num("SkillDPSMultiplier", o);
}

// --- shared bodies ----------------------------------------------------------------------------------------------------------

/// act_int.lua:715 (ArmageddonBrand, BrandSupport, PenanceBrandAltX/Y, StormBrand/AltX): brand activation frequency.
pub(super) fn brand_activation(off: &mut Off<'_>) {
    let v = sdn(off, "repeatFrequency", "act_int.lua:716")
        / (1.0 + sum(off, ModType::Inc, &["Speed", "BrandActivationFrequency"]) / 100.0)
        / more(off, &["BrandActivationFrequency"]);
    set(off, "hitTimeOverride", v);
}

/// spectre.lua:7277 AzmeriHydraBarrage.
pub(super) fn dps_eq_projectile_count(off: &mut Off<'_>) {
    let v = on(off, "ProjectileCount");
    set(off, "dpsMultiplier", v);
}

/// act_dex.lua:1026 (Barrage/AltX, BlastRain, VaalLightningArrow).
pub(super) fn part2_dps_eq_projectile_count(off: &mut Off<'_>) {
    if part(off, 2.0) {
        dps_eq_projectile_count(off);
    }
}

/// act_int.lua:1086 BallLightning (the `NormalHitsPerCast` output is written under `if breakdown`, R8).
pub(super) fn ball_lightning(off: &mut Off<'_>) {
    let mut dps = 0.0;
    if part(off, 1.0) {
        dps = 1.0;
    } else if part(off, 2.0) {
        let secs = sdn(off, "strikeInterval", "act_int.lua:1116");
        let duration = sdn(off, "duration", "act_int.lua:1122");
        let max_strikes = (duration / secs).floor();
        let base_speed = sdn(off, "projectileSpeed", "act_int.lua:1125");
        let (inc, more_) = off.q.calc_mods(Some(&off.cfg), &["ProjectileSpeed"]);
        let ball_dist_per_sec = base_speed * (inc * more_);
        let per_strike = ball_dist_per_sec * secs;
        let enemy_radius = off
            .q
            .override_num(Some(&off.cfg), &["EnemyRadius"], "act_int.lua:1132")
            .unwrap_or_else(|| sum(off, ModType::Base, &["EnemyRadius"]));
        let base_strike_radius = on(off, "AreaOfEffectRadius");
        let strike_radius = base_strike_radius + enemy_radius;
        let cast_dist = if let Some(d) = off.cfg.skill_dist {
            d
        } else if off.flag("triggered") {
            (base_strike_radius / 2.0).floor()
        } else {
            base_strike_radius
        };
        let first = max2(1.0, ((cast_dist - strike_radius) / per_strike).ceil());
        let last = (min2(misc("ProjectileDistanceCap"), cast_dist + strike_radius) / per_strike).floor();
        let num = max2(0.0, min2(max_strikes, last + 1.0 - first));
        let last = first + num - 1.0;
        dps = num;
        // :1145-1156
        if off.breakdown {
            let hits = BTable::lines([
                sf!("^8Balls travel at^7 %.2f^8 units/sec.", ball_dist_per_sec),
                sf!(
                    "^8Lightning bolts strike all nearby enemies every^7 %.2f^8 seconds (^7%.2f^8 strikes/sec).",
                    secs,
                    1.0 / secs
                ),
                sf!("^8Balls travel^7 %.2f^8 units between each bolt strike.", per_strike),
                sf!("^8Assumes balls are cast^7 %d^8 units from the enemy.", cast_dist),
                sf!("^8Balls can strike enemies up to^7 %d^8 units away from themselves.", strike_radius),
                sf!(
                    "^8The first strike is at^7 %.2f^8 seconds after it is cast, when the ball is^7 %d^8 units from the cast point.",
                    first * secs,
                    first * per_strike
                ),
                sf!(
                    "^8The last strike is at^7 %.2f^8 seconds after it is cast, when the ball is^7 %d^8 units from the cast point.",
                    last * secs,
                    last * per_strike
                ),
            ]);
            off.out().set_num("NormalHitsPerCast", num);
            off.bd_put("NormalHitsPerCast", hits);
        }
    }
    if dps != 1.0 {
        mul_dps_out(off, dps);
    }
}

/// The `NormalHitsPerCast` breakdown of act_int.lua:1267-1272 / :1387-1392.
fn ball_strikes_breakdown(off: &mut Off<'_>, secs: f64, duration: f64, n: f64) {
    if off.breakdown {
        off.bd_put(
            "NormalHitsPerCast",
            BTable::lines([
                sf!(
                    "^8Lightning bolts strike all nearby enemies every^7 %.2f^8 seconds (^7%.2f^8 strikes/sec).",
                    secs,
                    1.0 / secs
                ),
                sf!("^8Balls lasts for ^7%d^8 seconds for a total of ^7%d^8 strikes.", duration, n),
            ]),
        );
    }
}

/// act_int.lua:1252 BallLightningAltX.
pub(super) fn ball_lightning_alt_x(off: &mut Off<'_>) {
    if part(off, 2.0) || part(off, 3.0) {
        let secs = sdn(off, "strikeInterval", "act_int.lua:1258");
        let duration = sdn(off, "duration", "act_int.lua:1259");
        let n = (duration / secs).floor();
        if n != 1.0 {
            mul_dps_out(off, n);
        }
        off.out().set_num("NormalHitsPerCast", n);
        ball_strikes_breakdown(off, secs, duration, n);
    }
}

/// act_int.lua:1375 BallLightningAltY.
pub(super) fn ball_lightning_alt_y(off: &mut Off<'_>) {
    let secs = sdn(off, "strikeInterval", "act_int.lua:1379");
    let duration = sdn(off, "duration", "act_int.lua:1380");
    let n = (duration / secs).floor();
    mul_dps(off, n);
    off.out().set_num("NormalHitsPerCast", n);
    let o = off.st.player.output.opt_num("SkillDPSMultiplier").unwrap_or(1.0) * n;
    off.out().set_num("SkillDPSMultiplier", o);
    ball_strikes_breakdown(off, secs, duration, n);
}

/// act_int.lua:1470 Bane / BaneAltX (preSkillTypeFunc).
pub(super) fn bane(off: &mut Off<'_>) {
    let limit = off.st.player.output.opt_num("EnemyCurseLimit");
    let mut count = 0.0;
    let st = off.env.skills.as_ref().expect("skills on");
    let me = st.skill(off.i).socket_group;
    // `ipairs(activeSkill.actor.activeSkillList)`
    for j in (0..off.actor_skill_count()).map(|j| off.actor_skill(j)) {
        let sk = st.skill(j);
        if sk.socket_group == me {
            let list = crate::perform::skill_list_of(off.env, j);
            // `skill.skillModList:GetCondition("AppliedByBane")` -- no cfg
            crate::perform::skill_guard(&sk.malformed_mods, &["Condition:AppliedByBane"]);
            if crate::setup::ok(list.borrow().get_condition("AppliedByBane", None, false)) == Some(true) {
                count += 1.0;
                if Some(count) == limit {
                    break;
                }
            }
        }
    }
    off.q.nm("Multiplier:CurseApplied", ModType::Base, count, "Base", 0, 0, vec![]);
}

/// act_dex.lua:1374 BladeBlast / AltX.
pub(super) fn blade_blast(off: &mut Off<'_>) {
    let m = sdn(off, "dpsBaseMultiplier", "act_dex.lua:1375");
    mul_dps(off, m);
    if part(off, 2.0) {
        set(off, "hitTimeOverride", 1.0);
    }
}

/// act_dex.lua:1906 BladeFlurry / AltX.
pub(super) fn blade_flurry(off: &mut Off<'_>) {
    if part(off, 2.0) && sdn(off, "numStages", "act_dex.lua:1907") > 0.0 {
        let n = sdn(off, "numStages", "act_dex.lua:1908");
        let mut multi = 0.0;
        let mut i = 1.0;
        while i <= n {
            multi += 0.8 + 0.2 * i;
            i += 1.0;
        }
        multi /= 0.8 + 0.2 * n;
        set(off, "dpsMultiplier", multi / n + 1.0);
    }
}

/// act_dex.lua:2209 BladeVortex.
pub(super) fn blade_vortex(off: &mut Off<'_>) {
    let v = sdn(off, "hitFrequency", "act_dex.lua:2210")
        / (1.0 + sum(off, ModType::Base, &["Multiplier:BladeVortexBlade"]) * sdn(off, "hitFrequencyPerBlade", "act_dex.lua:2210"));
    set(off, "hitTimeOverride", v);
}

/// act_dex.lua:2787 BladefallAltZ.
pub(super) fn bladefall_alt_z(off: &mut Off<'_>) {
    let v = sdn(off, "hitFrequency", "act_dex.lua:2788") / (1.0 + sdn(off, "incVolleyFrequency", "act_dex.lua:2788") / 100.0);
    set(off, "hitTimeOverride", v);
}

/// act_int.lua:1823 BlazingSalvo.
pub(super) fn blazing_salvo(off: &mut Off<'_>) {
    if part(off, 2.0) {
        let pc = on(off, "ProjectileCount");
        mul_dps(off, pc);
    }
}

/// other.lua:669 BloodSacramentUnique (initialFunc).
pub(super) fn blood_sacrament(off: &mut Off<'_>) {
    if on(off, "LifeReservedPercent") >= 100.0 {
        return;
    }
    let pct = sd_or(off, "LifeReservedPercent", 3.0);
    let base = sd_or(off, "LifeReservedBase", f64::INFINITY);
    off.q.nm(
        "Multiplier:ChannelledLifeReservedPercentPerStage",
        ModType::Base,
        pct,
        "Blood Sacrament",
        0,
        0,
        vec![],
    );
    off.q.nm(
        "Multiplier:ChannelledLifeReservedPerStage",
        ModType::Base,
        base,
        "Blood Sacrament",
        0,
        0,
        vec![],
    );
}

/// act_int.lua:2298 Bodyswap / AltX.
pub(super) fn bodyswap(off: &mut Off<'_>) {
    if part(off, 1.0) {
        let pool = if off.flag("totem") { on(off, "TotemLife") } else { on(off, "Life") };
        let v = pool * sdn(off, "selfFireExplosionLifeMultiplier", "act_int.lua:2302");
        set(off, "FireBonusMin", v);
        set(off, "FireBonusMax", v);
    }
}

fn charged_dash_with(off: &mut Off<'_>, src: &str) {
    if part(off, 3.0) {
        let v = sum(off, ModType::Inc, &["chargedDashFinalDamageModifier"]);
        let t = tag(&[("type", Value::str("Release Damage")), ("skillPart", Value::Num(3.0))]);
        off.q.nm("Damage", ModType::More, v, src, mod_flag("Attack"), 0, vec![t]);
    }
}

/// act_dex.lua:3939 ChargedDash.
pub(super) fn charged_dash(off: &mut Off<'_>) {
    charged_dash_with(off, "Skill:ChargedDash");
}

/// act_dex.lua:4064 ChargedDashAltX.
pub(super) fn charged_dash_alt_x(off: &mut Off<'_>) {
    charged_dash_with(off, "Skill:ChargedDashAltX");
}

fn cremation_rate(off: &mut Off<'_>) {
    let v = sdn(off, "cremationFireRate", "act_dex.lua:4357") / (1.0 + sd_or(off, "cremationFireRateIncrease", 0.0));
    set(off, "hitTimeOverride", v);
}

/// act_dex.lua:4355 Cremation / AltX.
pub(super) fn cremation_part1(off: &mut Off<'_>) {
    if part(off, 1.0) {
        cremation_rate(off);
    }
}

/// act_dex.lua:4592 CremationAltY.
pub(super) fn cremation(off: &mut Off<'_>) {
    cremation_rate(off);
}

fn cyclone_with(off: &mut Off<'_>, src: &str) {
    let w = |off: &Off<'_>, n: u8, k: &str| -> Value {
        let p = off.actor();
        if n == 1 { p.weapon_data1.get(k) } else { p.weapon_data2.get(k) }
    };
    let num = |v: Value, site: &str| {
        v.as_num()
            .unwrap_or_else(|| panic!("{site}: attempt to perform arithmetic on field 'rangeBonus'"))
    };
    let range = if off.flag("weapon1Attack") && w(off, 1, "range").truthy() {
        let c1 = off.weapon_cfg(1);
        let b1 = off.q.base(c1.as_ref(), &["MeleeWeaponRange"])
            + 10.0 * off.q.base(c1.as_ref(), &["MeleeWeaponRangeMetre"])
            + num(w(off, 1, "rangeBonus"), "act_dex.lua:4706");
        if off.flag("weapon2Attack") && w(off, 2, "range").truthy() {
            let c2 = off.weapon_cfg(2);
            (b1 + off.q.base(c2.as_ref(), &["MeleeWeaponRange"])
                + 10.0 * off.q.base(c2.as_ref(), &["MeleeWeaponRangeMetre"])
                + num(w(off, 2, "rangeBonus"), "act_dex.lua:4708"))
                / 2.0
        } else {
            b1
        }
    } else {
        sum(off, ModType::Base, &["UnarmedRange"]) + 10.0 * sum(off, ModType::Base, &["UnarmedRangeMetre"])
    };
    off.q.nm("Multiplier:AdditionalMeleeRange", ModType::Base, range, src, 0, 0, vec![]);
}

/// act_dex.lua:4703 Cyclone / VaalCyclone (initialFunc).
pub(super) fn cyclone(off: &mut Off<'_>) {
    cyclone_with(off, "Skill:Cyclone");
}

/// act_dex.lua:4809 CycloneAltX (initialFunc).
pub(super) fn cyclone_alt_x(off: &mut Off<'_>) {
    cyclone_with(off, "Skill:CycloneAltX");
}

/// act_int.lua:3889 DarkPact.
pub(super) fn dark_pact(off: &mut Off<'_>) {
    let life = if part(off, 1.0) {
        if off.flag("totem") { on(off, "TotemLife") } else { on(off, "Life") }
    } else {
        sd_or(off, "skeletonLife", 0.0)
    };
    let add = life * sdn(off, "lifeDealtAsChaos", "act_int.lua:3901") / 100.0;
    let (mn, mx) = (sdn(off, "ChaosMin", "act_int.lua:3902"), sdn(off, "ChaosMax", "act_int.lua:3903"));
    set(off, "ChaosMin", mn + add);
    set(off, "ChaosMax", mx + add);
}

/// act_int.lua:4006 DarkPactAltX.
pub(super) fn dark_pact_alt_x(off: &mut Off<'_>) {
    let life = if off.flag("totem") { on(off, "TotemLife") } else { on(off, "Life") };
    let add = life * sdn(off, "percentageLifeSacrificed", "act_int.lua:4013") / 100.0
        * (sdn(off, "percentageSacrificedDealtAsChaos", "act_int.lua:4013") / 100.0);
    let (mn, mx) = (sdn(off, "ChaosMin", "act_int.lua:4015"), sdn(off, "ChaosMax", "act_int.lua:4016"));
    let a = if part(off, 2.0) {
        add.floor()
    } else {
        let casts = 7.0 / (1.0 + min2(100.0, sd_or(off, "additionalRuinChance", 0.0)) / 100.0);
        (add / casts).floor()
    };
    set(off, "ChaosMin", mn + a);
    set(off, "ChaosMax", mx + a);
}

/// other.lua:1282 DeathWish.
pub(super) fn death_wish(off: &mut Off<'_>) {
    if part(off, 2.0) {
        let v = on(off, "Life") * sdn(off, "selfFireExplosionLifeMultiplier", "other.lua:1285");
        set(off, "FireBonusMin", v);
        set(off, "FireBonusMax", v);
    }
}

fn divine_ire_with(off: &mut Off<'_>, var: &str) {
    let v = sum(off, ModType::Base, &[var]);
    set(off, "hitTimeMultiplier", v);
}

/// act_int.lua:4602 DivineIre.
pub(super) fn divine_ire(off: &mut Off<'_>) {
    if part(off, 2.0) {
        divine_ire_with(off, "Multiplier:DivineIreStage");
    }
}

/// act_int.lua:4712 DivineIreAltX.
pub(super) fn divine_ire_alt_x(off: &mut Off<'_>) {
    if part(off, 2.0) {
        divine_ire_with(off, "Multiplier:DivineIreofHolyLightningStage");
    }
}

/// act_int.lua:4823 DivineIreAltY.
pub(super) fn divine_ire_alt_y(off: &mut Off<'_>) {
    divine_ire_with(off, "Multiplier:DivineIreofDisintegrationStage");
}

/// `math.floor(skillData.duration * output.DurationMod * 10)`.
fn duration_100ms(off: &Off<'_>, site: &str) -> f64 {
    (sdn(off, "duration", site) * on(off, "DurationMod") * 10.0).floor()
}

/// act_str.lua:3363 Earthquake.
pub(super) fn earthquake(off: &mut Off<'_>) {
    let d = duration_100ms(off, "act_str.lua:3364");
    let v = sum(off, ModType::Inc, &["EarthquakeDurationIncDamage"]) * d;
    off.q.nm("Damage", ModType::Inc, v, "Skill:Earthquake", 0, 0, vec![]);
}

fn eye_of_winter_with(off: &mut Off<'_>, src: &str) {
    let v = sum(off, ModType::Base, &["EyeOfWinterRamp"]);
    let t = distance_ramp([(0.0, 0.0), (60.0 * on(off, "ProjectileSpeedMod"), 1.0)]);
    off.q.nm("Damage", ModType::More, v, src, 0, 0, vec![t]);
}

pub(super) fn eye_of_winter(off: &mut Off<'_>) {
    eye_of_winter_with(off, "Skill:EyeOfWinter");
}
pub(super) fn eye_of_winter_alt_x(off: &mut Off<'_>) {
    eye_of_winter_with(off, "Skill:EyeOfWinterAltX");
}
pub(super) fn eye_of_winter_alt_y(off: &mut Off<'_>) {
    eye_of_winter_with(off, "Skill:EyeOfWinterAltY");
}

fn flameblast_with(off: &mut Off<'_>, var: &str) {
    let v = max2(
        sum(off, ModType::Base, &[var]) - sum(off, ModType::Base, &["Multiplier:FlameblastMinimumStage"]),
        1.0,
    );
    set(off, "hitTimeMultiplier", v);
}

pub(super) fn flameblast(off: &mut Off<'_>) {
    flameblast_with(off, "Multiplier:FlameblastStage");
}
pub(super) fn flameblast_alt_x(off: &mut Off<'_>) {
    flameblast_with(off, "Multiplier:FlameblastofCelerityStage");
}
pub(super) fn flameblast_alt_y(off: &mut Off<'_>) {
    flameblast_with(off, "Multiplier:FlameblastofContractionStage");
}

/// `output.Duration / output.TrapCooldown` -> `output.AverageActiveTraps`.
fn average_active_traps(off: &mut Off<'_>) -> (f64, f64, f64) {
    let (duration, cooldown) = (on(off, "Duration"), on(off, "TrapCooldown"));
    let v = duration / cooldown;
    off.out().set_num("AverageActiveTraps", v);
    (v, duration, cooldown)
}

/// act_dex.lua:7710-7716 / :7820-7826 `breakdown.AverageActiveTraps`.
fn flamethrower_traps_breakdown(off: &mut Off<'_>, avg: f64, duration: f64, cooldown: f64) {
    if off.breakdown {
        off.bd_put(
            "AverageActiveTraps",
            BTable::lines([
                "Average active traps, not considering stored cooldown uses:".to_owned(),
                sf!("%.2f^8 (skill duration)", duration),
                sf!("/ %.2f^8 (cooldown)", cooldown),
                sf!("= %.2f traps", avg),
            ]),
        );
    }
}

/// act_dex.lua:7691 FlamethrowerTrap.
pub(super) fn flamethrower_trap(off: &mut Off<'_>) {
    let (avg, duration, cooldown) = average_active_traps(off);
    let t = if part(off, 2.0) || part(off, 4.0) { 0.3 } else { 0.1 };
    set(off, "hitTimeOverride", t);
    if part(off, 3.0) || part(off, 4.0) {
        mul_dps(off, avg);
    }
    flamethrower_traps_breakdown(off, avg, duration, cooldown);
}

/// act_dex.lua:7809 FlamethrowerTrapAltX.
pub(super) fn flamethrower_trap_alt_x(off: &mut Off<'_>) {
    let (avg, duration, cooldown) = average_active_traps(off);
    set(off, "hitTimeOverride", 0.1);
    mul_dps(off, avg);
    flamethrower_traps_breakdown(off, avg, duration, cooldown);
}

/// act_int.lua:7686 FreezingPulse.
pub(super) fn freezing_pulse(off: &mut Off<'_>) {
    let psm = on(off, "ProjectileSpeedMod");
    off.q.nm(
        "Damage",
        ModType::More,
        -50.0,
        "Skill:FreezingPulse",
        0,
        0,
        vec![distance_ramp([(0.0, 0.0), (60.0 * psm, 1.0)])],
    );
    off.q.nm(
        "EnemyFreezeChance",
        ModType::Base,
        25.0,
        "Skill:FreezingPulse",
        0,
        0,
        vec![distance_ramp([(0.0, 1.0), (15.0 * psm, 0.0)])],
    );
}

/// act_int.lua:7933 FrostBombAltY.
pub(super) fn frost_bomb_alt_y(off: &mut Off<'_>) {
    let d = duration_100ms(off, "act_int.lua:7934");
    off.q.nm(
        "Multiplier:100msFrostBombDuration",
        ModType::Base,
        d,
        "Skill:FrostBombAltY",
        0,
        0,
        vec![],
    );
}

/// act_int.lua:8543 GalvanicField / AltX.
pub(super) fn hit_time_repeat_frequency(off: &mut Off<'_>) {
    let v = sdn(off, "repeatFrequency", "act_int.lua:8544");
    set(off, "hitTimeOverride", v);
}

/// act_str.lua:5743 HeraldOfAsh.
pub(super) fn herald_of_ash(off: &mut Off<'_>) {
    let v = sd_or(off, "hoaOverkill", 0.0)
        * (1.0 + sdn(off, "hoaMoreBurn", "act_str.lua:5744") / 100.0)
        * sdn(off, "hoaOverkillPercent", "act_str.lua:5744");
    set(off, "FireDot", v);
}

/// `repeatFrequency / (1 + INC(var) / 100)`.
fn repeat_over_inc(off: &mut Off<'_>, var: &str, site: &str) {
    let v = sdn(off, "repeatFrequency", site) / (1.0 + sum(off, ModType::Inc, &[var]) / 100.0);
    set(off, "hitTimeOverride", v);
}

pub(super) fn herald_of_the_breach(off: &mut Off<'_>) {
    repeat_over_inc(off, "PulseFrequencyPerPressure", "other.lua:2259");
}
pub(super) fn herald_of_thunder(off: &mut Off<'_>) {
    repeat_over_inc(off, "HeraldStormFrequency", "act_int.lua:9384");
}
pub(super) fn hydrosphere(off: &mut Off<'_>) {
    if part(off, 1.0) || part(off, 2.0) || part(off, 3.0) {
        repeat_over_inc(off, "HydroSphereFrequency", "act_int.lua:8974");
    }
}
pub(super) fn static_strike(off: &mut Off<'_>) {
    if part(off, 2.0) {
        repeat_over_inc(off, "StaticStrikeFrequency", "act_str.lua:10728");
    }
}
pub(super) fn void_sphere(off: &mut Off<'_>) {
    repeat_over_inc(off, "VoidSphereFrequency", "act_int.lua:20268");
}

/// act_int.lua:9805 IceSpear / AltX.
pub(super) fn ice_spear(off: &mut Off<'_>) {
    if part(off, 3.0) || part(off, 4.0) {
        let pc = on(off, "ProjectileCount");
        mul_dps(off, pc);
    }
}

fn incinerate_with(off: &mut Off<'_>, var: &str) {
    if part(off, 2.0) {
        let v = max2(
            sum(off, ModType::Base, &[var]) - sum(off, ModType::Base, &["Multiplier:IncinerateMinimumStage"]) - 0.4175,
            0.5825,
        );
        set(off, "hitTimeMultiplier", v);
    }
}

pub(super) fn incinerate(off: &mut Off<'_>) {
    incinerate_with(off, "Multiplier:IncinerateStage");
}
pub(super) fn incinerate_alt_x(off: &mut Off<'_>) {
    incinerate_with(off, "Multiplier:IncinerateofExpanseStage");
}
pub(super) fn incinerate_alt_y(off: &mut Off<'_>) {
    incinerate_with(off, "Multiplier:IncinerateofVentingStage");
}

fn infernal_blow_with(off: &mut Off<'_>, src: &str) {
    let effect = sum(off, ModType::Base, &["DebuffEffect"]);
    if part(off, 2.0) || part(off, 3.0) {
        let t = tag(&[
            ("type", Value::str("Multiplier")),
            ("var", Value::str("DebuffStack")),
            ("base", Value::Num(-100.0 + effect)),
        ]);
        off.q.nm("Damage", ModType::More, effect, src, 0, 0, vec![t]);
    }
    if part(off, 3.0) {
        set(off, "dpsMultiplier", 1.0 / 6.0);
    }
}

pub(super) fn infernal_blow(off: &mut Off<'_>) {
    infernal_blow_with(off, "Skill:InfernalBlow");
}
pub(super) fn infernal_blow_alt_x(off: &mut Off<'_>) {
    infernal_blow_with(off, "Skill:InfernalBlowAltX");
}

fn kinetic_fusillade_with(off: &mut Off<'_>, src: &str) {
    if part(off, 1.0) {
        let pc = on(off, "ProjectileCount");
        set(off, "dpsMultiplier", pc);
        let per = sd_or(off, "damagePerProjectile", 0.0);
        if per != 0.0 && pc > 1.0 {
            let avg = per * (pc - 1.0) / 2.0;
            off.q.nm("Damage", ModType::More, avg, src, 0, 0, vec![]);
            off.out().set_num("KineticFusilladeAvgMoreMult", avg);
            // act_int.lua:11029-11040 (:11247-11258 AltX)
            if off.breakdown {
                let mut t = BTable::new();
                t.push(sf!("^8Each projectile deals^7 %d%%^8 more damage per previous projectile", per));
                t.push(sf!("^8With^7 %d^8 projectiles, damage progression is:^7", pc));
                let mut i = 1.0;
                while i <= pc {
                    t.push(sf!("  ^8Projectile %d:^7 %d%%^8 more damage", i, per * (i - 1.0)));
                    i += 1.0;
                }
                t.push(sf!("^8Average more multiplier:^7 %.1f%%", avg));
                off.bd_put("KineticFusilladeSequentialBreakdown", t);
                let max = off
                    .q
                    .override_num(Some(&off.cfg), &["ProjectileCountMaximum"], "act_int.lua:11039")
                    .unwrap_or_else(|| panic!("act_int.lua:11039: bad argument #2 to 'format' (number expected, got nil)"));
                let line = sf!("^8Maximum number of Kinetic Fusilade projectiles:^7 %d", max);
                if let Some(b) = off.bd() {
                    b.table_or("ProjectileCount", BTable::new).push(line);
                }
            }
        }
    }
}

pub(super) fn kinetic_fusillade(off: &mut Off<'_>) {
    kinetic_fusillade_with(off, "Skill:KineticFusillade");
}
pub(super) fn kinetic_fusillade_alt_x(off: &mut Off<'_>) {
    kinetic_fusillade_with(off, "Skill:KineticFusilladeAltX");
}

fn lancing_steel_with(off: &mut Off<'_>, src: &str) {
    if part(off, 2.0) {
        let pc = on(off, "ProjectileCount");
        let pct = (pc - 1.0) / pc;
        let mult = (more(off, &["LancingSteelSubsequentDamage"]) - 1.0) * 100.0 * pct;
        set(off, "dpsMultiplier", pc);
        off.q.nm("Damage", ModType::More, mult, src, 0, 0, vec![]);
    }
}

pub(super) fn lancing_steel(off: &mut Off<'_>) {
    lancing_steel_with(off, "Skill:LancingSteel");
}
pub(super) fn lancing_steel_alt_x(off: &mut Off<'_>) {
    lancing_steel_with(off, "Skill:LancingSteelAltX");
}

/// other.lua:2596 LightningBolt, Vortex / AltX: `hitTimeOverride = output.Cooldown` (nil stays nil).
pub(super) fn hit_time_cooldown(off: &mut Off<'_>) {
    let v = off.out_val("Cooldown");
    off.set_sd("hitTimeOverride", v);
}

/// act_int.lua:12642 LightningTendrilsAltX.
pub(super) fn lightning_tendrils_alt_x(off: &mut Off<'_>) {
    let interval = sdn(off, "pulseInterval", "act_int.lua:12644");
    if part(off, 2.0) {
        let t = tag(&[("type", Value::str("SkillPart")), ("skillPart", Value::Num(2.0))]);
        off.q
            .nm("DPS", ModType::More, -(1.0 / interval) * 100.0, "Normal pulse", 0, 0, vec![t]);
    } else if part(off, 3.0) {
        let t = tag(&[("type", Value::str("SkillPart")), ("skillPart", Value::Num(3.0))]);
        off.q.nm(
            "DPS",
            ModType::More,
            -(interval - 1.0) / interval * 100.0,
            "Stronger pulse",
            0,
            0,
            vec![t],
        );
    }
}

/// act_int.lua:13527 Manabond.
pub(super) fn manabond(off: &mut Off<'_>) {
    let missing = sd_or(off, "ManabondMissingUnreservedManaPercentage", 100.0);
    let gain = sdn(off, "ManabondMissingManaGainPercent", "act_int.lua:13529");
    let unreserved = off.st.player.output.opt_num("ManaUnreserved").unwrap_or(0.0);
    let v = ((gain / 100.0) * (missing / 100.0) * unreserved).floor();
    let src = "Manabond gain % missing unreserved mana as base lightning damage";
    off.q.nm("LightningMin", ModType::Base, v, src, 0, 0, vec![]);
    off.q.nm("LightningMax", ModType::Base, v, src, 0, 0, vec![]);
}

fn molten_shell_with(off: &mut Off<'_>, key: &str) {
    let add = sd_or(off, key, 0.0) * sdn(off, "moltenShellReflect", "act_str.lua:7600") / 100.0;
    set(off, "FireMin", add);
    set(off, "FireMax", add);
}

pub(super) fn molten_shell(off: &mut Off<'_>) {
    molten_shell_with(off, "MoltenShellDamageMitigated");
}
pub(super) fn vaal_molten_shell(off: &mut Off<'_>) {
    molten_shell_with(off, "VaalMoltenShellDamageMitigated");
}

/// The overlap chance of MoltenStrike / AltX (:7842-7851); `None` on the melee part.
fn molten_overlap(off: &mut Off<'_>) -> Option<f64> {
    if part(off, 1.0) {
        return None;
    }
    let enemy_radius = off
        .q
        .override_num(Some(&off.cfg), &["EnemyRadius"], "act_str.lua:7843")
        .unwrap_or_else(|| sum(off, ModType::Base, &["EnemyRadius"]));
    let ball = on(off, "AreaOfEffectRadius");
    let inner = on(off, "AreaOfEffectRadiusSecondary");
    let outer = on(off, "AreaOfEffectRadiusTertiary");
    let overlap = min2(1.0, (enemy_radius + ball - inner) / (outer - inner));
    off.out().set_num("OverlapChance", overlap * 100.0);
    // :7856-7864 (:8035-8043 AltX)
    if off.breakdown {
        let pct = on(off, "OverlapChance");
        off.bd_put(
            "OverlapChance",
            BTable::lines([
                "Chance for individual balls to land on the enemy:".to_owned(),
                "^8= (area where a ball can land on enemy) / (total area)".to_owned(),
                "^8= (enemy radius + ball radius - min travel) / (max travel - min travel)".to_owned(),
                sf!(
                    "^8= (^7%d^8 + ^7%d^8 - ^7%d) / (^7%d^8 - ^7%d)",
                    enemy_radius,
                    ball,
                    inner,
                    outer,
                    inner
                ),
                sf!("^8=^7 %.2f^8%%", pct),
            ]),
        );
    }
    Some(overlap)
}

/// The `SkillDPSMultiplier` breakdown `number of projectiles * overlap chance` (act_str.lua:7870-7875, :8053-8058).
fn molten_dps_breakdown(off: &mut Off<'_>, n: f64, overlap: f64, dps: f64) {
    if off.breakdown {
        off.bd_put(
            "SkillDPSMultiplier",
            BTable::lines([
                "DPS multiplier".to_owned(),
                "^8= number of projectiles * overlap chance".to_owned(),
                sf!("^8=^7 %d^8 *^7 %.3f^8", n, overlap),
                sf!("^8=^7 %.3f", dps),
            ]),
        );
    }
}

/// act_str.lua:7832 MoltenStrike.
pub(super) fn molten_strike(off: &mut Off<'_>) {
    let Some(overlap) = molten_overlap(off) else { return };
    let n = on(off, "ProjectileCount");
    let dps = if part(off, 3.0) { overlap * n } else { 1.0 };
    if part(off, 3.0) {
        molten_dps_breakdown(off, n, overlap, dps);
    }
    if dps != 1.0 {
        mul_dps_out(off, dps);
    }
}

/// act_str.lua:8011 MoltenStrikeAltX.
pub(super) fn molten_strike_alt_x(off: &mut Off<'_>) {
    let Some(overlap) = molten_overlap(off) else { return };
    let n = on(off, "ProjectileCount");
    let mut dps = 1.0;
    if part(off, 3.0) || part(off, 5.0) || part(off, 6.0) {
        dps = overlap * n;
        if !part(off, 6.0) {
            molten_dps_breakdown(off, n, overlap, dps);
        } else {
            let fifth = 1.0 + sdn(off, "FifthStrikeDamage", "act_str.lua:8060") / 100.0;
            let fifth_projectiles = sdn(off, "FifthStrikeProjectiles", "act_str.lua:8061");
            let fifth_overall = fifth * overlap * (n + fifth_projectiles);
            dps = 0.8 * dps + 0.2 * fifth_overall;
            // :8066-8076
            if off.breakdown {
                off.bd_put(
                    "SkillDPSMultiplier",
                    BTable::lines([
                        "Weighted average DPS multiplier for balls".to_owned(),
                        "^8= (0.8 * balls dps) + (0.2 * 5th attack balls dps)".to_owned(),
                        sf!(
                            "^8= (0.8 * normal ball hit * overlap chance * number of projectiles) + (0.2 * ball hit * 5th attack multiplier * overlap chance * (number of projectiles + %d))",
                            fifth_projectiles
                        ),
                        sf!(
                            "^8= ball hit * overlap chance * (0.8 * number of projectiles + 0.2 * 5th attack multiplier * (number of projectiles + %d))",
                            fifth_projectiles
                        ),
                        sf!(
                            "^8= ball hit * ^7%.3f ^8* (0.8 * ^7%d ^8+ 0.2 * ^7%.1f ^8* ^7%d^8)",
                            overlap,
                            n,
                            fifth,
                            n + fifth_projectiles
                        ),
                        sf!("^8= ball hit * ^7 %.3f", dps),
                    ]),
                );
            }
        }
    }
    if dps != 1.0 {
        mul_dps_out(off, dps);
    }
}

/// act_int.lua:13621 OrbOfStorms.
pub(super) fn orb_of_storms(off: &mut Off<'_>) {
    let v = sdn(off, "hitFrequency", "act_int.lua:13622") / off.q.calc_mod(Some(&off.cfg), &["Speed"]);
    set(off, "hitTimeOverride", v);
}

/// act_int.lua:13809 PenanceBrand.
pub(super) fn penance_brand(off: &mut Off<'_>) {
    let v = sdn(off, "repeatFrequency", "act_int.lua:13810") * sum(off, ModType::Base, &["Multiplier:PenanceBrandMaxEnergy"])
        / (1.0 + sum(off, ModType::Inc, &["Speed", "BrandActivationFrequency"]) / 100.0)
        / more(off, &["BrandActivationFrequency"]);
    set(off, "hitTimeOverride", v);
}

/// act_str.lua:8304 Perforate / AltX / AltY.
pub(super) fn perforate(off: &mut Off<'_>) {
    let v = max2(sd_or(off, "dpsMultiplier", 1.0), 1.0);
    set(off, "dpsMultiplier", v);
}

/// act_dex.lua:17423 PoisonousConcoction / AltX.
pub(super) fn poisonous_concoction(off: &mut Off<'_>) {
    let multiplier = sum(off, ModType::Base, &["ChaosPerLifeFlaskPercent"]);
    let added = off.st.player.output.opt_num("LifeFlaskRecovery").unwrap_or(0.0) * (multiplier / 100.0);
    off.q
        .nm("ChaosMin", ModType::Base, added, "Life Flask charges consumed", 0, 0, vec![]);
    off.q
        .nm("ChaosMax", ModType::Base, added, "Life Flask charges consumed", 0, 0, vec![]);
}

/// act_str.lua:9082 RageVortex (preSkillTypeFunc).
pub(super) fn rage_vortex(off: &mut Off<'_>) {
    if part(off, 2.0) {
        let max_rage = sum(off, ModType::Base, &["MaximumRage"]);
        let pct = sdn(off, "MaxRageVortexSacrificePercentage", "act_str.lua:9085") / 100.0;
        let config_override = sum(off, ModType::Base, &["Multiplier:RageSacrificedStacks"]);
        let max_sacrificed = (pct * max_rage).floor();
        let stacks = min2(if config_override > 0.0 { config_override } else { max_sacrificed }, max_sacrificed);
        off.q
            .nm("Multiplier:RageSacrificed", ModType::Base, stacks, "Skill:RageVortex", 0, 0, vec![]);
    }
}

/// act_int.lua:15428 RighteousFire.
pub(super) fn righteous_fire(off: &mut Off<'_>) {
    if off.flag("totem") && on(off, "TotemLife") > 1.0 {
        let v = on(off, "TotemLife") * sdn(off, "RFLifeMultiplier", "act_int.lua:15430")
            + on(off, "TotemEnergyShield") * sdn(off, "RFESMultiplier", "act_int.lua:15430");
        set(off, "FireDot", v);
    } else if on(off, "LifeUnreserved") > 1.0 {
        let v = on(off, "Life") * sdn(off, "RFLifeMultiplier", "act_int.lua:15432")
            + on(off, "EnergyShield") * sdn(off, "RFESMultiplier", "act_int.lua:15432");
        set(off, "FireDot", v);
    }
}

/// act_int.lua:15533 RighteousFireAltX.
pub(super) fn righteous_fire_alt_x(off: &mut Off<'_>) {
    if on(off, "LifeUnreserved") > 1.0 {
        let v = on(off, "Mana") * sdn(off, "RFManaMultiplier", "act_int.lua:15535");
        set(off, "FireDot", v);
    }
}

/// act_int.lua:15625 VaalRighteousFire.
pub(super) fn vaal_righteous_fire(off: &mut Off<'_>) {
    let pool = if off.flag("totem") {
        on(off, "TotemLife")
    } else {
        on(off, "Life") + on(off, "EnergyShield")
    };
    let v = pool * sdn(off, "percentSacrificed", "act_int.lua:15627") * sdn(off, "RFMultiplier", "act_int.lua:15627");
    set(off, "FireDot", v);
}

/// act_dex.lua:12990 ScourgeArrow -- `cfg` is an undefined global (nil).
pub(super) fn scourge_arrow(off: &mut Off<'_>) {
    let v = max2(off.q.base(None, &["Multiplier:ScourgeArrowStage"]) - 0.5, 0.5);
    set(off, "hitTimeMultiplier", v);
}

/// act_dex.lua:13779 ShrapnelBallista / AltX.
pub(super) fn shrapnel_ballista(off: &mut Off<'_>) {
    if !off.q.flag(None, &["SequentialProjectiles"]) {
        let pc = on(off, "ProjectileCount");
        let overlap = off
            .sd_num("ShrapnelBallistaProjectileOverlap", 0)
            .unwrap_or(if off.has_type("Rain") { pc } else { 1.0 });
        mul_dps(off, min2(overlap, pc));
    }
    let split = off.st.player.output.opt_num("SplitCount").unwrap_or(0.0);
    if split > 0.0 {
        let t = tag(&[("type", Value::str("Condition")), ("var", Value::str("ReturningProjectile"))]);
        off.q.nm("DPS", ModType::More, split * 100.0, "Split Return", 0, 0, vec![t]);
    }
}

/// act_int.lua:16721 Spark / AltX / AltY.
pub(super) fn spark(off: &mut Off<'_>) {
    if part(off, 2.0) {
        let n = 1.0 + (on(off, "Duration") / 0.66).floor();
        mul_dps(off, n);
        let v = sdn(off, "dpsMultiplier", "act_int.lua:16725");
        off.out().set_num("SkillDPSMultiplier", v);
    }
}

/// act_int.lua:18036 StormBurst.
pub(super) fn storm_burst(off: &mut Off<'_>) {
    if part(off, 2.0) {
        let duration = sdn(off, "duration", "act_int.lua:18038") * on(off, "DurationMod");
        let jump = sdn(off, "repeatFrequency", "act_int.lua:18040") * 10.0;
        mul_dps(off, 1.0 + (duration * 10.0 / jump).floor());
    } else if part(off, 3.0) {
        storm_burst_remaining(off, "act_int.lua:18044");
    }
}

fn storm_burst_remaining(off: &mut Off<'_>, site: &str) {
    let jumps = ((sdn(off, "duration", site) * on(off, "DurationMod") + 0.001) / sdn(off, "repeatFrequency", site)).floor();
    let v = max2(sdn(off, "remainingDurationDamage", site) * jumps - 100.0, 0.0);
    off.q.nm("Damage", ModType::More, v, "Skill:StormBurst", 0, 0, vec![]);
}

/// act_int.lua:18151 StormBurstAltX.
pub(super) fn storm_burst_alt_x(off: &mut Off<'_>) {
    if part(off, 2.0) {
        storm_burst_remaining(off, "act_int.lua:18153");
    }
}

fn storm_rain_rate(off: &mut Off<'_>) {
    let v = sdn(off, "hitFrequency", "act_dex.lua:11749") / (1.0 + sum(off, ModType::Inc, &["StormRainBeamFrequency"]) / 100.0);
    set(off, "hitTimeOverride", v);
}

pub(super) fn storm_rain(off: &mut Off<'_>) {
    if part(off, 2.0) {
        storm_rain_rate(off);
        let v = sd_or(off, "beamOverlapMultiplier", 1.0);
        set(off, "dpsMultiplier", v);
    }
}
pub(super) fn storm_rain_alt_x(off: &mut Off<'_>) {
    if part(off, 2.0) {
        storm_rain_rate(off);
        let v = min2(
            sd_or(off, "activeArrowMultiplier", 1.0),
            sum(off, ModType::Base, &["StormRainAllowedStormArrows"]),
        );
        set(off, "dpsMultiplier", v);
    }
}
pub(super) fn storm_rain_alt_y(off: &mut Off<'_>) {
    if part(off, 2.0) {
        storm_rain_rate(off);
        let v = sum(off, ModType::Base, &["StormRainAllowedStormArrows"]);
        set(off, "dpsMultiplier", v);
    }
}

/// act_dex.lua:18034 Tornado.
pub(super) fn tornado(off: &mut Off<'_>) {
    let v = off.sd("damageInterval");
    off.set_sd("hitTimeOverride", v);
}

/// act_dex.lua:18127 TornadoAltY.
pub(super) fn tornado_alt_y(off: &mut Off<'_>) {
    tornado(off);
    let v = sum(off, ModType::Base, &["Multiplier:TornadoofElementalTurbulenceStage"]);
    mul_dps(off, v);
}

/// act_dex.lua:16051 TornadoShot.
pub(super) fn tornado_shot(off: &mut Off<'_>) {
    if part(off, 2.0) && off.st.player.output.opt_num("ReturnChance").unwrap_or(0.0) == 0.0 {
        let split = off.st.player.output.opt_num("SplitCount").unwrap_or(0.0);
        let mut avg = on(off, "ProjectileCount") + split;
        let q = &off.q;
        if q.flag(None, &["SequentialProjectiles"])
            && !q.flag(None, &["OneShotProj"])
            && !q.flag(None, &["NoAdditionalProjectiles"])
            && !q.flag(None, &["SingleProjectile"])
            && !q.flag(None, &["TriggeredBySnipe"])
        {
            avg = 1.0 + split;
        }
        let chance = min2(
            off.sd_num("tornadoShotSecondaryHitChance", 0)
                .unwrap_or_else(|| 20.0 * sum(off, ModType::Base, &["tornadoShotSecondaryProjectiles"]))
                / 100.0,
            1.0,
        );
        mul_dps(off, 1.0 + chance * avg);
    }
}

/// act_dex.lua:14935 ToxicRain / AltX / AltY.
pub(super) fn toxic_rain(off: &mut Off<'_>) {
    let v = min2(sd_or(off, "podOverlapMultiplier", 1.0), on(off, "ProjectileCount"));
    set(off, "dpsMultiplier", v);
}

/// act_dex.lua:2428 VaalBladeVortex.
pub(super) fn vaal_blade_vortex(off: &mut Off<'_>) {
    let v = sdn(off, "hitFrequency", "act_dex.lua:2429")
        / (1.0 + sdn(off, "VaalBladeVortexBlade", "act_dex.lua:2429") * sd_or(off, "hitFrequencyPerBlade", 0.0));
    set(off, "hitTimeOverride", v);
}

/// act_int.lua:20445 VoltaxicBurst.
pub(super) fn voltaxic_burst(off: &mut Off<'_>) {
    let d = duration_100ms(off, "act_int.lua:20446");
    let v = sum(off, ModType::Inc, &["VoltaxicDurationIncDamage"]) * d;
    off.q.nm("Damage", ModType::Inc, v, "Skill:VoltaxicBurst", 0, 0, vec![]);
}

fn wave_of_conviction_with(off: &mut Off<'_>, src: &str) {
    if let Some(d) = off.sd_num("duration", 0) {
        let tick = misc("ServerTickRate");
        let duration = ((d * tick).ceil() / tick * on(off, "DurationMod") * 10.0).floor();
        let v = sum(off, ModType::Inc, &["WaveOfConvictionDurationDotMulti"]) * duration / 100.0;
        let t = tag(&[("type", Value::str("Multiplier")), ("var", Value::str("WoCDurationExpired"))]);
        off.q.nm("DotMultiplier", ModType::Base, v, src, 0, 0, vec![t]);
    }
}

pub(super) fn wave_of_conviction(off: &mut Off<'_>) {
    wave_of_conviction_with(off, "Skill:WaveOfConviction");
}
pub(super) fn wave_of_conviction_alt_y(off: &mut Off<'_>) {
    wave_of_conviction_with(off, "Skill:WaveOfConvictionAltY");
}

/// act_int.lua:20983 WinterOrb.
pub(super) fn winter_orb(off: &mut Off<'_>) {
    let rate = 1.0 + sum(off, ModType::Inc, &["HitRate", "Speed"]) / 100.0;
    let mult = more(off, &["HitRate"]);
    let v = sdn(off, "repeatFrequency", "act_int.lua:20986") / rate / mult;
    set(off, "hitTimeOverride", v);
}

/// act_int.lua:21107 WintertideBrand.
pub(super) fn wintertide_brand(off: &mut Off<'_>) {
    let p1 = part(off, 1.0);
    off.set_sd("countsAttachedBrandsInDamage", Value::Bool(p1));
    brand_activation(off);
    if !p1 {
        return;
    }
    let site = "act_int.lua:21112";
    let hit_time = sdn(off, "hitTimeOverride", site);
    let max_stages_cap = sum(off, ModType::Base, &["Multiplier:WintertideBrandMaxStages"]);
    let debuff_mult = 1.0
        / max2(
            misc("BuffExpirationSlowCap"),
            crate::setup::ok(crate::calc_lib::mod_(&off.enemy_db.borrow(), Some(&off.cfg), &["BuffExpireFaster"])),
        );
    // `calcSkillDuration(..., {})`: `env.mode_effective` of the empty table is nil
    let duration = crate::offence::calc_skill_duration(&off.q.list, &off.q.malformed, &off.cfg, &off.sk().skill_data, false, &off.enemy_db)
        * debuff_mult;
    let max_stages = min2((duration / hit_time).floor(), max_stages_cap);
    let time_to_max = max_stages * hit_time;
    let time_at_max = max2(duration - time_to_max, 0.0);
    let per_stage = sum(off, ModType::Base, &["Multiplier:WintertideBrandDamagePerStage"]);
    let avg_stages = (max_stages * (max_stages - 1.0) / 2.0 * hit_time + max_stages * time_at_max) / duration;
    let avg_mult = avg_stages * per_stage;
    let end_base = sd_or(off, "durationTertiary", 0.0) + sum(off, ModType::Base, &["Duration", "TertiaryDuration"]);
    let end_mod = max2(off.q.calc_mod(Some(&off.cfg), &["Duration", "TertiaryDuration"]), 0.0);
    let tick = misc("ServerTickRate");
    let end_duration = (end_base * end_mod * debuff_mult * tick).ceil() / tick;
    let end_uptime = min2(end_duration / duration, 1.0);
    let attached = sdn(off, "attachedBrandCount", site);
    let end_mult = max_stages * per_stage;
    let dps = attached * (100.0 + avg_mult) + end_uptime * (100.0 + end_mult) - 100.0;
    off.q
        .nm("Damage", ModType::More, dps, "Wintertide Brand Average Multiplier", 0, 0, vec![]);
}

/// `explodeMod.<k>` of an `ExplodeMod` LIST value (`{ type, chance, amount }`, ModParser.lua:2116).
fn explode_field(v: &Value, k: &str) -> Value {
    match v {
        Value::Table(t) => t.get(k).clone(),
        other => panic!(
            "Data/Skills/other.lua:6209: attempt to index local 'explodeMod' (a {} value)",
            other.type_name()
        ),
    }
}

fn explode_num(v: &Value, k: &str, line: u32) -> f64 {
    let x = explode_field(v, k);
    x.as_num().unwrap_or_else(|| {
        panic!(
            "Data/Skills/other.lua:{line}: attempt to perform arithmetic on field '{k}' (a {} value)",
            x.type_name()
        )
    })
}

fn explode_type(v: &Value, line: u32) -> String {
    match explode_field(v, "type") {
        Value::Str(s) => s.to_string(),
        other => panic!(
            "Data/Skills/other.lua:{line}: attempt to concatenate field 'type' (a {} value)",
            other.type_name()
        ),
    }
}

/// `activeSkill.activeEffect.srcInstance.explodeSource.modSource or "Tree:"..explodeSource.id` (other.lua:6209).
fn explode_source_name(off: &Off<'_>) -> String {
    let st = off.env.skills.as_ref().expect("skills on");
    let (g, k) = off
        .sk()
        .active_effect
        .borrow()
        .src_instance
        .unwrap_or_else(|| panic!("Data/Skills/other.lua:6209: attempt to index field 'srcInstance' (a nil value)"));
    let src = st.tab.socket_group_list()[g].gem_list[k]
        .explode_source
        .clone()
        .unwrap_or_else(|| panic!("Data/Skills/other.lua:6209: attempt to index field 'explodeSource' (a nil value)"));
    match src {
        crate::skills_tab::ExplodeSource::Node(id) => format!("Tree:{id}"),
        crate::skills_tab::ExplodeSource::Item(it) => match (&it.0.mod_source, it.0.id) {
            (Some(s), _) => s.clone(),
            (None, Some(id)) => format!("Tree:{}", pob_lua::tostring_num(id)),
            (None, None) => panic!("Data/Skills/other.lua:6209: attempt to concatenate field 'id' (a nil value)"),
        },
    }
}

/// `skills.EnemyExplode.preDamageFunc` (other.lua:6204-6252, "On Kill Monster Explosion").
///
/// Part 3 ("Average Of All Sources") walks `pairs(typeAmountChances)` (string keys, R13) and `pairs(amountChance)` (number keys, R11);
/// the products/sums there depend on that order. The number keys are walked in their LuaJIT order (the `lj_tab` model, R11, R7.10b);
/// the string keys in insertion order (the `List` order -- the fixed order of MINEFIELD-RUST R13), and with more than one type the result
/// may differ from a LuaJIT process: that is recorded (the set of LuaJIT results is not measured -- no corpus scenario has part 3).
pub(super) fn enemy_explode(off: &mut Off<'_>) {
    off.out().set_num("ExplodeChance", 0.0);
    let part = off.sk().skill_part;
    if part != Some(3.0) {
        let all = off.q.tabulate(Some(ModType::List), Some(&off.cfg), &["ExplodeMod"]);
        for e in &all {
            let active_effect_source = explode_source_name(off);
            if e.mod_.source.as_deref() == Some(active_effect_source.as_str()) {
                let em = &e.value;
                let ty = explode_type(em, 6212);
                let amount = explode_num(em, "amount", 6213);
                if ty == "RandomElement" {
                    for t in ["Fire", "Cold", "Lightning"] {
                        set(off, &format!("{t}EffectiveExplodePercentage"), amount / 3.0);
                    }
                } else {
                    set(off, &format!("{ty}EffectiveExplodePercentage"), amount);
                }
                let chance = if part == Some(2.0) { 1.0 } else { explode_num(em, "chance", 6219) };
                off.out().set_num("ExplodeChance", chance);
            }
        }
    } else {
        // type -> (amount -> summed chance), insertion order
        let list = off.q.list(Some(&off.cfg), &["ExplodeMod"]);
        let mut by_type: Vec<(String, Vec<(f64, f64)>)> = Vec::new();
        for em in &list {
            let ty = explode_type(em, 6226);
            let amount = explode_num(em, "amount", 6227);
            let chance = explode_num(em, "chance", 6227);
            let i = match by_type.iter().position(|(t, _)| *t == ty) {
                Some(i) => i,
                None => {
                    by_type.push((ty, Vec::new()));
                    by_type.len() - 1
                }
            };
            let ac = &mut by_type[i].1;
            match ac.iter_mut().find(|(a, _)| *a == amount) {
                Some(x) => x.1 += chance,
                None => ac.push((amount, chance)),
            }
        }
        if by_type.len() > 1 {
            off.note(
                "Data/Skills/other.lua:6230 skills.EnemyExplode.preDamageFunc: pairs(typeAmountChances) over more than one ExplodeMod type (string keys, R13: the LuaJIT order varies by process; insertion order used)"
                    .into(),
            );
        }
        for (ty, ac) in &by_type {
            let mut phys_explode_chance = 0.0;
            // :6232 `pairs(amountChance)` -- number keys, the `lj_tab` order (R11)
            let keys: Vec<f64> = ac.iter().map(|(a, _)| *a).collect();
            let ordered: Vec<(f64, f64)> = pob_tree::lj_table::pairs_order(&keys)
                .into_iter()
                .map(|k| *ac.iter().find(|(a, _)| *a == k).expect("a key of the table"))
                .collect();
            for &(amount, chance) in &ordered {
                let amount_x_chance = amount * chance;
                if ty == "RandomElement" {
                    for t in ["Fire", "Cold", "Lightning"] {
                        let k = format!("{t}EffectiveExplodePercentage");
                        let v = sd_or(off, &k, 0.0) + amount_x_chance / 3.0;
                        set(off, &k, v);
                    }
                } else {
                    let k = format!("{ty}EffectiveExplodePercentage");
                    let v = sd_or(off, &k, 0.0) + amount_x_chance;
                    set(off, &k, v);
                }
                if ty == "Physical" {
                    phys_explode_chance = 1.0 - ((1.0 - phys_explode_chance) * (1.0 - chance));
                }
                let ec = 1.0 - ((1.0 - on(off, "ExplodeChance")) * (1.0 - chance));
                off.out().set_num("ExplodeChance", ec);
            }
            if ty == "Physical" && phys_explode_chance != 0.0 {
                off.q.new_mod(
                    "CalcArmourAsThoughDealing",
                    Some(ModType::More),
                    Value::Num(100.0 / min2(phys_explode_chance, 1.0) - 100.0),
                    None,
                    0,
                    0,
                    vec![],
                );
            }
        }
    }
    let ec = min2(on(off, "ExplodeChance") * 100.0, 100.0);
    off.out().set_num("ExplodeChance", ec);
}

// --- R7.5: `explosiveArrowFunc` (CalcOffence.lua:3038) and `postCritFunc` (:3315) ----------------------------------------------------

/// act_dex.lua:6699-6778 ExplosiveArrow.explosiveArrowFunc(activeSkill, output, globalOutput, globalBreakdown, env): `output` is the
/// damage pass's (`off.cur_pass`); the breakdown rows are R8.
pub(super) fn explosive_arrow(off: &mut Off<'_>) {
    const SRC: &str = "Data/Skills/act_dex.lua";
    let pi = off.cur_pass;
    let skill_part = off.sk().skill_part;
    if skill_part != Some(1.0) && skill_part != Some(2.0) {
        return;
    }
    let q = off.q.clone();
    let cfg = off.cfg.clone();
    let c = Some(&cfg);
    let duration = crate::offence::calc_skill_duration(
        &q.list,
        &q.malformed,
        &cfg,
        &off.sk().skill_data,
        off.env.mode_effective,
        &off.enemy_db,
    );
    let fuse_limit = q.base(c, &["ExplosiveArrowMaxFuseCount"]);
    let active_totems = if off.flag("totem") {
        let o = crate::setup::ok(off.env_db.borrow().override_(None, &["TotemsSummoned"])).filter(Value::truthy);
        Some(match o {
            Some(v) => v
                .as_num()
                .unwrap_or_else(|| panic!("{SRC}:6729: attempt to perform arithmetic on local 'activeTotems'")),
            None => q.base(c, &["ActiveTotemLimit", "ActiveBallistaLimit"]),
        })
    } else {
        None
    };
    let mut barrage = None;
    if q.flag(None, &["SequentialProjectiles"])
        && !q.flag(None, &["OneShotProj"])
        && !q.flag(None, &["NoAdditionalProjectiles"])
        && !q.flag(None, &["TriggeredBySnipe"])
    {
        let b = q.base(c, &["ProjectileCount"]);
        barrage = Some(b);
        let d = sdn(off, "dpsMultiplier", "act_dex.lua:6721") / b;
        set(off, "dpsMultiplier", d);
    }
    let hit_chance = match off.pval(pi, "HitChance") {
        Some(crate::output::OutValue::Num(x)) => *x,
        o => panic!("{SRC}:6724: attempt to perform arithmetic on field 'HitChance' ({o:?})"),
    };
    let mut rate = (hit_chance / 100.0) * on(off, "Speed") * sdn(off, "dpsMultiplier", "act_dex.lua:6724") * barrage.unwrap_or(1.0);
    if let Some(t) = active_totems {
        rate *= t;
    }
    if part(off, 2.0) {
        let maximum = min2((rate * duration).floor() + 1.0, fuse_limit);
        off.q
            .nm("Multiplier:ExplosiveArrowStage", ModType::Base, maximum, "Base", 0, 0, vec![]);
        off.q.nm(
            "Multiplier:ExplosiveArrowStageAfterFirst",
            ModType::Base,
            maximum - 1.0,
            "Base",
            0,
            0,
            vec![],
        );
        off.out().set_num("MaxExplosiveArrowFuseCalculated", maximum);
    } else {
        off.out().remove("MaxExplosiveArrowFuseCalculated");
    }
    let time_to_max = fuse_limit / rate;
    let hit_time = if part(off, 2.0) || (part(off, 1.0) && off.sk().active_stage_count.unwrap_or(0.0) + 1.0 >= fuse_limit) {
        min2(duration, time_to_max)
    } else {
        duration
    };
    off.out().set_num("HitTime", hit_time);
    off.out().set_num("HitSpeed", 1.0 / hit_time);
}

/// act_int.lua:11046-11123 KineticFusillade(AltX).postCritFunc (`output` = the top-level output).
pub(super) fn kinetic_fusillade_post_crit(off: &mut Off<'_>) {
    const SRC: &str = "act_int.lua:11053";
    let base_delay = off.sd_num("delayPerProjectile", 0);
    let projectile_count = if part(off, 1.0) { on(off, "ProjectileCount") } else { 1.0 };
    let hover_delay = sdn(off, "duration", "act_int.lua:11064");
    let duration_mod = on(off, "DurationMod");
    let base_time = base_delay
        .unwrap_or_else(|| panic!("{SRC}: attempt to perform arithmetic on local 'baseDelayBetweenProjectiles' (a nil value)"))
        * (projectile_count - 1.0);
    let effective_delay = (hover_delay + base_time) * duration_mod;
    let tick_time = misc("ServerTickTime");
    let rounded = (effective_delay / tick_time).ceil() * tick_time;
    let max_aps = 1.0 / rounded;
    let current = off.st.player.output.opt_num("Speed");
    off.out().set_num("KineticFusilladeMaxEffectiveAPS", max_aps);
    // :11076-11113
    if off.breakdown {
        let base_delay = base_delay.unwrap_or(0.0); // nil already raised above
        let max_pred = 1.0 / effective_delay;
        let mut t = BTable::new();
        t.push(sf!("^8Base hover delay:^7 %.1fs", hover_delay));
        t.push(sf!("^8Base delay between projectiles:^7 %.2fs", base_delay));
        t.push(sf!(
            "^8Base time for^7 %d added ^8projectiles:^7 %.2fs x %d = %.1fs",
            projectile_count - 1.0,
            base_delay,
            projectile_count - 1.0,
            base_time
        ));
        t.push(sf!("^8Duration modifier:^7 %.4f", duration_mod));
        t.push("");
        t.push(sf!("^6Lockstep"));
        let eff_line = sf!(
            "^8Effective delay:^7 (%.1f + %.2f) x %.4f = %.4fs",
            hover_delay,
            base_time,
            duration_mod,
            effective_delay
        );
        t.push(eff_line.clone());
        t.push(sf!("^8Rounded up to nearest server tick^7 = %.4fs", rounded));
        t.push(sf!("^8Max effective attack rate:^7 1 / %.4f = %.2f", rounded, max_aps));
        if let Some(cur) = current {
            let current_inc = sum(off, ModType::Inc, &["Speed"]);
            let needed = ((max_aps / cur) * (1.0 + current_inc / 100.0) - 1.0) * 100.0;
            let additional = needed - current_inc;
            let needed_pred = ((max_pred / cur) * (1.0 + current_inc / 100.0) - 1.0) * 100.0;
            let additional_pred = needed_pred - current_inc;
            let verdict = |t: &mut BTable, max: f64, add: f64| {
                if cur > max {
                    t.push(sf!("^1Current attack rate (%.2f) exceeds max effective rate!", cur));
                    t.push(sf!("^1Reduce attack speed by at least^7 %d%% ^1", (-add).ceil()));
                    t.push(sf!("^1DPS is reduced by %.1f%%", (1.0 - max / cur) * 100.0));
                } else {
                    t.push(sf!("^2Current attack rate (%.2f) is within effective limits", cur));
                    t.push(sf!("^2You can add up to^7 %d%% ^2increased attack speed", add.floor()));
                }
            };
            verdict(&mut t, max_aps, additional);
            t.push("");
            t.push(sf!("^6Predictive"));
            t.push(eff_line);
            t.push(sf!("^8Max effective attack rate:^7 1 / %.4f = %.2f", effective_delay, max_pred));
            verdict(&mut t, max_pred, additional_pred);
        }
        off.bd_put("KineticFusilladeMaxEffectiveAPS", t);
    }
    if part(off, 1.0)
        && let Some(cur) = current
        && cur > max_aps
    {
        let ratio = max_aps / cur;
        let original = off.sd_num("dpsMultiplier", 0).unwrap_or_else(|| on(off, "ProjectileCount"));
        set(off, "dpsMultiplier", original * ratio);
    }
}

/// act_int.lua:12651-12662 LightningTendrilsAltX.postCritFunc.
pub(super) fn lightning_tendrils_alt_x_post_crit(off: &mut Off<'_>) {
    if part(off, 1.0) {
        let interval = sdn(off, "pulseInterval", "act_int.lua:12658");
        let pulse_damage = sdn(off, "pulseDamage", "act_int.lua:12655") / 100.0;
        let crit_chance = on(off, "PreEffectiveCritChance") / 100.0;
        let effective = on(off, "CritChance") / 100.0;
        let crit_multi = on(off, "CritMultiplier");
        let average_more = 100.0
            * (((interval - 1.0) * (1.0 + crit_chance * (crit_multi - 1.0)) + (1.0 + pulse_damage) * crit_multi)
                / (interval * ((1.0 - effective) + crit_multi * effective))
                - 1.0);
        let kw = crate::setup::keyword_flag("Hit") | crate::setup::keyword_flag("Ailment");
        let t = tag(&[("type", Value::str("SkillPart")), ("skillPart", Value::Num(1.0))]);
        off.q
            .nm("Damage", ModType::More, average_more, "Average Pulse Damage", 0, kw, vec![t]);
    }
}

/// The trap wave variants sharing the body of act_int.lua:11667 (`LightningSpireTrap`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Waves {
    /// act_int.lua:11667 LightningSpireTrap.
    Spire,
    /// act_int.lua:11912 LightningSpireTrapAltX (fixed pulse rate, active traps from the throw time).
    SpireAltX,
    /// act_int.lua:12150 LightningSpireTrapAltY (fixed pulse rate).
    SpireAltY,
    /// act_dex.lua:13381 SeismicTrap (AoE-scaled radius, `SeismicPulseFrequency`).
    Seismic,
}

/// `hitChance(enemyRadius, areaDamageRadius, areaSpreadRadius)` of the wave traps; `output.OverlapChance` and its breakdown
/// (act_int.lua:11713-11719 and the copies in the other wave traps).
fn overlap_chance(off: &mut Off<'_>, src: &str) -> f64 {
    let enemy_radius = off
        .q
        .override_num(Some(&off.cfg), &["EnemyRadius"], src)
        .unwrap_or_else(|| sum(off, ModType::Base, &["EnemyRadius"]));
    let wave_radius = on(off, "AreaOfEffectRadiusSecondary");
    let full_radius = on(off, "AreaOfEffectRadius");
    let damaging = wave_radius + enemy_radius - 1.0;
    let overlap = min2(damaging * damaging / (full_radius * full_radius), 1.0);
    off.out().set_num("OverlapChance", overlap * 100.0);
    if off.breakdown {
        off.bd_put(
            "OverlapChance",
            BTable::lines([
                "Chance for individual wave to land within range to damage enemy:".to_owned(),
                "^8= (area where wave can spawn to damage enemy) / (total area)".to_owned(),
                "^8= (^7secondary radius^8 + ^7enemy radius^8 - 1) ^ 2 / ^7radius^8 ^ 2".to_owned(),
                sf!("^8= (^7%d^8 +^7 %d^8 - 1) ^ 2 /^7 %d^8 ^ 2", wave_radius, enemy_radius, full_radius),
                sf!("^8=^7 %.3f^8%%", overlap * 100.0),
            ]),
        );
    }
    overlap
}

/// The `SkillDPSMultiplier` breakdown of the wave traps' skill parts 2-6 (act_int.lua:11742-11782 and copies; other parts: none).
fn trap_waves_dps_breakdown(off: &mut Off<'_>, avg_traps: f64, max_waves: f64, overlap: f64, dps: f64) {
    if !off.breakdown {
        return;
    }
    let mut t = BTable::lines(["DPS multiplier".to_owned()]);
    if part(off, 2.0) {
        t.push("^8= ^7maximum waves^8 * ^7overlap chance^8");
        t.push(sf!("^8=^7 %d^8 *^7 %.2f^8", max_waves, overlap));
        t.push(sf!("^8=^7 %.3f", dps));
    } else if part(off, 3.0) {
        t.push(sf!("^8=^7 %d (maximum waves)", dps));
    } else if part(off, 4.0) {
        t.push(sf!("^8=^7 %.2f (average active traps)", dps));
    } else if part(off, 5.0) {
        t.push("^8= ^7average active traps^8 * ^7maximum waves^8 * ^7overlap chance^8");
        t.push(sf!("^8=^7 %.2f^8 *^7 %d^8 *^7 %.2f", avg_traps, max_waves, overlap));
        t.push(sf!("^8=^7 %.3f", dps));
    } else if part(off, 6.0) {
        t.push("^8= ^7average active traps^8 * ^7maximum waves");
        t.push(sf!("^8=^7 %.2f^8 *^7 %d", avg_traps, max_waves));
        t.push(sf!("^8=^7 %.3f", dps));
    } else {
        return;
    }
    off.bd_put("SkillDPSMultiplier", t);
}

/// The wave traps' body: pulse rate -> `skillData.hitTimeOverride`, pulses per trap, average active traps, the overlap chance and the
/// skill part's DPS multiplier, with their breakdown rows.
fn trap_waves(off: &mut Off<'_>, v: Waves, src: &str) {
    // :11676 (not Seismic) `output.AreaOfEffectRadius = skillData.radius`
    if v != Waves::Seismic {
        let r = off.sd("radius");
        off.set_out("AreaOfEffectRadius", r);
        // :11677-11679
        if off.breakdown {
            off.bd_put(
                "AreaOfEffectRadius",
                BTable::lines(["Targeting area of this skill is not affected by Area of Effect modifiers.".to_owned()]),
            );
        }
    }
    let nil = |k: &str| -> ! { panic!("{src}: attempt to perform arithmetic on local '{k}' (a nil value)") };
    let base_interval = off.sd_num("repeatInterval", 0);
    // `(incFrequency, moreFrequency)` of the variants with a frequency (Spire, Seismic)
    let mut frequency = None;
    let wave_pulse_rate = match v {
        Waves::Spire | Waves::Seismic => {
            let names: &[&str] = if v == Waves::Seismic {
                &["TrapThrowingSpeed", "SeismicPulseFrequency"]
            } else {
                &["TrapThrowingSpeed"]
            };
            let inc = 1.0 + sum(off, ModType::Inc, names) / 100.0;
            let more_ = more(off, names);
            frequency = Some((inc, more_));
            inc * more_ / base_interval.unwrap_or_else(|| nil("baseInterval"))
        }
        Waves::SpireAltX | Waves::SpireAltY => 1.0 / base_interval.unwrap_or_else(|| nil("baseInterval")),
    };
    set(off, "hitTimeOverride", 1.0 / wave_pulse_rate);
    if matches!(v, Waves::Spire | Waves::Seismic) {
        off.out().set_num("WavePulseRate", wave_pulse_rate);
    }
    let inc_duration = 1.0 + sum(off, ModType::Inc, &["Duration"]) / 100.0;
    let more_duration = more(off, &["Duration"]);
    let base_duration = sdn(off, "duration", src);
    let duration = base_duration * inc_duration * more_duration;
    let pulses = (duration * wave_pulse_rate).floor();
    off.out().set_num("PulsesPerTrap", pulses);
    let effective_duration = pulses / wave_pulse_rate;
    // the divisor: the throw time (AltX) or `output.TrapCooldown`
    let (average_active_traps, divisor) = if v == Waves::SpireAltX {
        let action_speed_mod = 1.0 + sum(off, ModType::Inc, &["ActionSpeed"]) / 100.0;
        let base_speed = 1.0 / sum(off, ModType::Base, &["TrapThrowingTime"]);
        let throw_speed = base_speed * off.q.calc_mod(Some(&off.cfg), &["TrapThrowingSpeed"]) * action_speed_mod;
        let throw_speed = min2(throw_speed, misc("ServerTickRate"));
        let throw_time = 1.0 / throw_speed;
        (effective_duration / throw_time, throw_time)
    } else {
        let cooldown = on(off, "TrapCooldown");
        (effective_duration / cooldown, cooldown)
    };
    off.out().set_num("AverageActiveTraps", average_active_traps);
    let overlap = overlap_chance(off, src);
    // :11720-11740 (AltX/AltY: no pulse rate table, duration-only breakpoints, AltX: the throw time)
    if off.breakdown {
        let base_interval = base_interval.unwrap_or_else(|| nil("baseInterval"));
        if let Some((inc_frequency, more_frequency)) = frequency {
            off.bd_put(
                "WavePulseRate",
                BTable::lines([
                    "Pulse rate:".to_owned(),
                    sf!("%.2f ^8(base pulse rate)", 1.0 / base_interval),
                    sf!("* %.2f ^8(increased/reduced pulse frequency)", inc_frequency),
                    sf!("* %.2f ^8(more/less pulse frequency)", more_frequency),
                    sf!("= %.2f^8/s", wave_pulse_rate),
                ]),
            );
        }
        let next_duration = (100.0 * ((pulses + 1.0) / (wave_pulse_rate * base_duration * more_duration) - inc_duration)).ceil();
        // `-math.ceil(100 * (...) - 1)`
        let prev_duration = -(100.0 * (pulses / (wave_pulse_rate * base_duration * more_duration) - inc_duration) - 1.0).ceil();
        let mut t = BTable::lines([
            "Pulses per trap:".to_owned(),
            sf!("%.3f ^8(unrounded skill duration)", duration),
            sf!("* %.2f ^8(pulse rate)", wave_pulse_rate),
            sf!("= %.2f ^8pulses", duration * wave_pulse_rate),
            "^8rounded down".to_owned(),
            sf!("= %d ^8pulses", pulses),
        ]);
        if let Some((inc_frequency, more_frequency)) = frequency {
            t.push(sf!(
                "^8Next breakpoint: %d%% increased Trap Throwing Speed / %d%% increased Duration",
                (100.0 * ((pulses + 1.0) * base_interval / (duration * more_frequency) - inc_frequency)).ceil(),
                next_duration
            ));
            t.push(sf!(
                "^8Previous breakpoint: %d%% reduced Trap Throwing Speed / %d%% reduced Duration",
                -(100.0 * (pulses * base_interval / (duration * more_frequency) - inc_frequency) - 1.0).ceil(),
                prev_duration
            ));
        } else {
            t.push(sf!("^8Next breakpoint: %d%% increased Duration", next_duration));
            t.push(sf!("^8Previous breakpoint: %d%% reduced Duration", prev_duration));
        }
        off.bd_put("PulsesPerTrap", t);
        let (head, tail) = if v == Waves::SpireAltX {
            ("Average active traps:", sf!("/ %.2f ^8(throw time)", divisor))
        } else {
            (
                "Average active traps, not considering stored cooldown uses:",
                sf!("/ %.2f ^8(cooldown)", divisor),
            )
        };
        off.bd_put(
            "AverageActiveTraps",
            BTable::lines([
                head.to_owned(),
                sf!(
                    "%.2f^8 /^7 %.2f^8 (pulses / pulse rate = effective skill duration)",
                    pulses,
                    wave_pulse_rate
                ),
                tail,
                sf!("= %.2f traps", average_active_traps),
            ]),
        );
    }
    let max_waves = sum(off, ModType::Base, &["MaximumWaves"]);
    let mut dps_multiplier = 1.0;
    if part(off, 2.0) {
        dps_multiplier = max_waves * overlap;
    } else if part(off, 3.0) {
        dps_multiplier = max_waves;
    } else if part(off, 4.0) {
        dps_multiplier = average_active_traps;
    } else if part(off, 5.0) {
        dps_multiplier = average_active_traps * max_waves * overlap;
    } else if part(off, 6.0) {
        dps_multiplier = average_active_traps * max_waves;
    }
    trap_waves_dps_breakdown(off, average_active_traps, max_waves, overlap, dps_multiplier);
    if dps_multiplier != 1.0 {
        mul_dps_out(off, dps_multiplier);
    }
}

/// act_int.lua:11667 LightningSpireTrap.
pub(super) fn lightning_spire_trap(off: &mut Off<'_>) {
    trap_waves(off, Waves::Spire, "act_int.lua:11667");
}

/// act_int.lua:11912 LightningSpireTrapAltX.
pub(super) fn lightning_spire_trap_alt_x(off: &mut Off<'_>) {
    trap_waves(off, Waves::SpireAltX, "act_int.lua:11912");
}

/// act_int.lua:12150 LightningSpireTrapAltY.
pub(super) fn lightning_spire_trap_alt_y(off: &mut Off<'_>) {
    trap_waves(off, Waves::SpireAltY, "act_int.lua:12150");
}

/// act_dex.lua:13381 SeismicTrap.
pub(super) fn seismic_trap(off: &mut Off<'_>) {
    trap_waves(off, Waves::Seismic, "act_dex.lua:13381");
}

/// act_dex.lua:13622 SeismicTrapAltX (the overlap chance and skill parts 2/3 only).
pub(super) fn seismic_trap_alt_x(off: &mut Off<'_>) {
    let overlap = overlap_chance(off, "act_dex.lua:13622");
    let max_waves = sum(off, ModType::Base, &["MaximumWaves"]);
    let mut dps_multiplier = 1.0;
    if part(off, 2.0) {
        dps_multiplier = max_waves * overlap;
    } else if part(off, 3.0) {
        dps_multiplier = max_waves;
    }
    // :13649-13662 (parts 2 / 3 only)
    if part(off, 2.0) || part(off, 3.0) {
        trap_waves_dps_breakdown(off, 0.0, max_waves, overlap, dps_multiplier);
    }
    if dps_multiplier != 1.0 {
        mul_dps_out(off, dps_multiplier);
    }
}

/// minion.lua:756 GAZombieCorpseGroundImpact / :1756 MinionInstability (R7.10b): `skillData.hitTimeOverride = 1 /
/// (activeSkill.summonSkill.skillData.summonSpeed or 1)` -- `summonSkill` is the player skill that has the minion.
pub(super) fn summon_speed_hit_time(off: &mut Off<'_>) {
    let owner = off
        .sk()
        .summon_skill
        .unwrap_or_else(|| panic!("Data/Skills/minion.lua:757: attempt to index field 'summonSkill' (a nil value)"));
    let speed = {
        let st = off.env.skills.as_ref().expect("skills on");
        match st.skill(owner).skill_data.get("summonSpeed") {
            None | Some(Value::Nil | Value::Bool(false)) => 1.0,
            Some(v) => v.as_num().unwrap_or_else(|| {
                panic!(
                    "Data/Skills/minion.lua:757: attempt to perform arithmetic on a {} value",
                    v.type_name()
                )
            }),
        }
    };
    set(off, "hitTimeOverride", 1.0 / speed);
}

/// act_int.lua:7390 ForbiddenRite / :7549 ForbiddenRiteAltX (preDamageFunc, R7.10b): the chaos damage added from life / energy shield and
/// `output.FRDamageTaken` (the self damage per cast) and its breakdown; `with_life` = the base gem (the AltX has no life part).
fn forbidden_rite_with(off: &mut Off<'_>, with_life: bool, src: &str) {
    use pob_lua::number::{floor, round};
    let names = ["DamageTaken", "ChaosDamageTaken", "DamageTakenWhenHit", "ChaosDamageTakenWhenHit"];
    let base_taken_flat = off.q.sum(ModType::Base, None, &names);
    let base_taken_inc = off.q.sum(ModType::Inc, None, &names);
    let base_taken_more = off.q.more(None, &names);
    let chaos_damage_taken = max2((1.0 + base_taken_inc / 100.0) * base_taken_more, 0.0);
    let chaos_flat = floor(round(base_taken_flat * chaos_damage_taken, None), Some(0.0));
    let (life_k, es_k, res_k) = if off.flag("totem") {
        ("TotemLife", "TotemEnergyShield", "TotemChaosResist")
    } else {
        ("Life", "EnergyShield", "ChaosResist")
    };
    let num = |off: &Off<'_>, k: &str| {
        off.st
            .player
            .output
            .opt_num(k)
            .unwrap_or_else(|| panic!("{src}: attempt to perform arithmetic on a nil value (output.{k})"))
    };
    // `life` is read (and used) only by the base gem
    let life = if with_life { num(off, life_k) } else { 0.0 };
    let energy_shield = num(off, es_k);
    let chaos_resistance = num(off, res_k);
    let es_add = energy_shield * sdn(off, "energyShieldDealtAsChaos", src);
    let add = if with_life {
        life * sdn(off, "lifeDealtAsChaos", src) + es_add
    } else {
        es_add
    };
    let self_life = if with_life {
        floor(
            round(life * sdn(off, "SelfDamageTakenLife", src), None) * (100.0 - chaos_resistance) / 100.0 * chaos_damage_taken,
            None,
        )
    } else {
        0.0
    };
    let self_es = floor(
        round(energy_shield * sdn(off, "SelfDamageTakenES", src), None) * (100.0 - chaos_resistance) / 100.0 * chaos_damage_taken,
        None,
    );
    let min = sdn(off, "ChaosMin", src) + add;
    set(off, "ChaosMin", min);
    let max = sdn(off, "ChaosMax", src) + add;
    set(off, "ChaosMax", max);
    if part(off, 2.0) {
        let pc = on(off, "ProjectileCount");
        mul_dps(off, pc + 1.0);
    }
    let taken = if with_life {
        self_life + self_es + chaos_flat
    } else {
        self_es + chaos_flat
    };
    off.out().set_num("FRDamageTaken", taken);
    // :7416-7432 / :7572-7587
    if off.breakdown {
        let sd_life = if with_life { sdn(off, "SelfDamageTakenLife", src) } else { 0.0 };
        let sd_es = sdn(off, "SelfDamageTakenES", src);
        let mut t = BTable::new();
        t.push(sf!("Damage Taken per cast from Forbidden Rite: %d", taken));
        if with_life {
            t.push(sf!(
                "^8=^7 %d^8 (Life) *^7 %d%%^8 (of Life taken as Chaos Damage)",
                life,
                sd_life * 100.0
            ));
        }
        if energy_shield != 0.0 {
            t.push(sf!(
                "^8+^7 %d^8 (ES) *^7 %d%%^8 (of ES taken as Chaos Damage)",
                energy_shield,
                sd_es * 100.0
            ));
        }
        let chaos = if with_life {
            life * sd_life + energy_shield * sd_es
        } else {
            energy_shield * sd_es
        };
        t.push(sf!(
            "^8=^7 %d^8 (Chaos Damage) *^7 %d%%^8 (Chaos Resistance)",
            chaos,
            chaos_resistance
        ));
        if chaos_flat != 0.0 {
            t.push(sf!("^8 -^7 %d^8 (Flat Damage reduction)", -base_taken_flat));
        }
        if chaos_damage_taken != 1.0 {
            t.push(sf!("^8 *^7 %.2f^8 (Damage taken Multiplier)", chaos_damage_taken));
        }
        off.bd_put("FRDamageTaken", t);
    }
}

/// act_int.lua:7390 ForbiddenRite.
pub(super) fn forbidden_rite(off: &mut Off<'_>) {
    forbidden_rite_with(off, true, "Data/Skills/act_int.lua:7390");
}

/// act_int.lua:7549 ForbiddenRiteAltX.
pub(super) fn forbidden_rite_alt_x(off: &mut Off<'_>) {
    forbidden_rite_with(off, false, "Data/Skills/act_int.lua:7549");
}

/// sup_str.lua:2540 AvengingFlame (preDamageFunc, R7.10b): `TotemLife` of the trigger source's cached MAIN / CALCS run (`GlobalCache.cachedData
/// ["MAIN"][uuid] or ["CALCS"][uuid]`, whatever the env's mode) as added fire damage. A cached run that stopped at a boundary of the port
/// (no `calcs.offence`, so no `TotemLife`) is a boundary here too -- never a nil read as 0.
pub(super) fn avenging_flame(off: &mut Off<'_>) {
    const SRC: &str = "Data/Skills/sup_str.lua:2540-2548";
    let uuid = off.sd("triggerSourceUUID");
    let totem_life = match uuid.as_str() {
        None if uuid.truthy() => panic!("{SRC}: GlobalCache.cachedData index by a {} uuid", uuid.type_name()),
        None => 0.0,
        Some(u) => {
            let sess = crate::session::CalcSession::of(off.env).unwrap_or_else(|| panic!("{SRC}: GlobalCache outside a CalcSession"));
            let cache = sess.cache();
            let c = cache
                .get(crate::session::CalcMode::Main, u)
                .or_else(|| cache.get(crate::session::CalcMode::Calcs, u));
            match c {
                None => 0.0,
                Some(c) if !c.offence_ran => {
                    let what = format!("{SRC} skills.AvengingFlame.preDamageFunc: the cached run of {u} has no calcs.offence (TotemLife)");
                    drop(cache);
                    off.note(what);
                    return;
                }
                Some(c) => c.run.borrow().state.player.output.opt_num("TotemLife").unwrap_or(0.0),
            }
        }
    };
    let add = totem_life * sdn(off, "lifeDealtAsFire", SRC) / 100.0;
    let max = sd_or(off, "FireMax", 0.0) + add;
    set(off, "FireMax", max);
    let min = sd_or(off, "FireMin", 0.0) + add;
    set(off, "FireMin", min);
}

/// act_str.lua:4679 FrozenSweep / :4797 FrozenSweepAltX (preDamageFunc, R7.10b): the cooldown and statue count of the parent Frozen Legion
/// (`parent` = its name) in the same slot as `activeSkill.actor.mainSkill`.
fn frozen_sweep_with(off: &mut Off<'_>, parent: &str, src: &str) {
    off.set_sd("showAverage", Value::Bool(false));
    off.set_flag("showAverage", false);
    off.set_flag("notAverage", true);
    // `activeSkill.actor.mainSkill`: the player's main skill, or the minion's (this skill) for a minion actor
    let actor_main = if off.who == crate::perform::Who::Player {
        off.player_main()
    } else {
        off.i
    };
    let parent_skill = {
        let st = off.env.skills.as_ref().expect("skills on");
        let slot_of = |i: usize| {
            let g = st
                .skill(i)
                .socket_group
                .unwrap_or_else(|| panic!("{src}: attempt to index field 'socketGroup' (a nil value)"));
            st.tab.socket_group_list()[g].slot.clone()
        };
        let mut found = None;
        for j in (0..off.actor_skill_count()).map(|j| off.actor_skill(j)) {
            let sk = st.skill(j);
            if st.skills.effects[sk.active_effect.borrow().granted_effect].name == parent && slot_of(actor_main) == slot_of(off.i) {
                found = Some(j);
                break;
            }
        }
        found.unwrap_or_else(|| panic!("{src}: attempt to index local 'parentSkill' (a nil value)"))
    };
    let (pq, pcfg, psd) = {
        let st = off.env.skills.as_ref().expect("skills on");
        let sk = st.skill(parent_skill);
        (
            crate::offence::Q {
                list: crate::perform::skill_list_of(off.env, parent_skill),
                malformed: sk.malformed_mods.clone(),
            },
            crate::perform::skill_cfg_of(st, sk),
            sk.skill_data.clone(),
        )
    };
    let pc = Some(&pcfg);
    if pq.flag(pc, &["DisableSkill"]) && !pq.flag(pc, &["EnableSkill"]) {
        return;
    }
    let cd_val = psd.get("cooldown").cloned().unwrap_or(Value::Nil);
    off.set_sd("cooldown", cd_val.clone());
    let cooldown = match pq.override_(pc, &["CooldownRecovery"]).filter(Value::truthy) {
        Some(v) => v
            .as_num()
            .unwrap_or_else(|| panic!("{src}: attempt to perform arithmetic on a {} value", v.type_name())),
        None => {
            let cd = cd_val.as_num().unwrap_or_else(|| {
                panic!(
                    "{src}: attempt to perform arithmetic on field 'cooldown' (a {} value)",
                    cd_val.type_name()
                )
            });
            (cd + pq.base(pc, &["CooldownRecovery"])) / max2(0.0, pq.calc_mod(pc, &["CooldownRecovery"]))
        }
    };
    let tick = misc("ServerTickRate");
    let out_cd = (cooldown * tick).ceil() / tick;
    off.out().set_num("Cooldown", out_cd);
    set(off, "hitTimeOverride", out_cd);
    let stored = match psd.get("storedUses") {
        Some(Value::Num(x)) => *x,
        other => panic!("{src}: attempt to perform arithmetic on field 'storedUses' ({other:?})"),
    };
    let max_statues = stored + pq.base(pc, &["AdditionalCooldownUses"]);
    let waited = if part(off, 1.0) {
        Value::Num(1.0)
    } else if part(off, 2.0) {
        Value::Num(max_statues)
    } else {
        Value::Nil
    };
    off.set_sd("averageBurstHits", waited);
}

/// act_str.lua:4679 FrozenSweep.
pub(super) fn frozen_sweep(off: &mut Off<'_>) {
    frozen_sweep_with(off, "Frozen Legion", "Data/Skills/act_str.lua:4679");
}

/// act_str.lua:4797 FrozenSweepAltX.
pub(super) fn frozen_sweep_alt_x(off: &mut Off<'_>) {
    frozen_sweep_with(off, "Frozen Legion of Rallying", "Data/Skills/act_str.lua:4797");
}

/// act_dex.lua:7156 ExplosiveTrap (and its statMap's `_grantedEffect`, the same body; R7.10b): the overlap chance of the small explosions --
/// a float sum over `pairs(output.AreaOfEffectRadiusTertiaryOccurrences)`, walked in the LuaJIT `pairs` order the offence state keeps
/// ([`crate::offence::Off::radius_tertiary_occurrences`], MINEFIELD-RUST R11). Breakdown rows: R8.5b (no LuaJIT scenario -- the skill is
/// not reachable in the R7-synth corpus, export-offence-synth.ts `FN_SKIP`).
pub(super) fn explosive_trap(off: &mut Off<'_>) {
    const SRC: &str = "Data/Skills/act_dex.lua:7156-7221";
    let enemy_radius = match off.q.override_(Some(&off.cfg), &["EnemyRadius"]).filter(Value::truthy) {
        Some(v) => v.as_num().unwrap_or_else(|| {
            panic!(
                "{SRC}: attempt to perform arithmetic on local 'enemyRadius' (a {} value)",
                v.type_name()
            )
        }),
        None => sum(off, ModType::Base, &["EnemyRadius"]),
    };
    let full_radius = off.st.player.output.opt_num("AreaOfEffectRadiusSecondary");
    let margin_width = sdn(off, "radiusTertiaryBaseMargin", SRC) * 2.0 + 1.0;
    let occ = off
        .radius_tertiary_occurrences
        .clone()
        .unwrap_or_else(|| panic!("{SRC}: bad argument #1 to 'pairs' (table expected, got nil)"));
    let mut overlap_chance = 0.0;
    for (&small_radius, &occurrence_count) in &occ {
        let spread =
            full_radius.unwrap_or_else(|| panic!("{SRC}: attempt to perform arithmetic on local 'areaSpreadRadius' (a nil value)"));
        // `hitChance(enemyRadius, areaDamageRadius, areaSpreadRadius)`
        let damaging = small_radius as f64 + enemy_radius - 1.0;
        let hit = min2(damaging * damaging / (spread * spread), 1.0);
        overlap_chance += hit * occurrence_count / margin_width;
    }
    off.out().set_num("OverlapChance", overlap_chance * 100.0);
    let small = sum(off, ModType::Base, &["SmallExplosions"]);
    off.out().set_num("SmallExplosionsPerTrap", small);
    // :7179-7198 -- the radii in `table.sort` order (numbers ascending), so the rows do not depend on the `pairs` order
    if off.breakdown {
        let mut t = BTable::lines([
            "Chance for individual small explosion to land within range to damage enemy:".to_owned(),
            "^8= (area where a small explosion can spawn to damage enemy) / (total area)".to_owned(),
            "^8= (^7tertiary radius^8 + ^7enemy radius^8 - 1) ^ 2 / ^7secondary radius^8 ^ 2".to_owned(),
            "^8Result is the weighted sum of overlap chances for each possible tertiary radius".to_owned(),
        ]);
        let mut radii: Vec<(i64, f64)> = occ.iter().map(|(r, c)| (*r, *c)).collect();
        radii.sort_by_key(|x| x.0);
        let n = radii.len();
        for (i, &(small_radius, count)) in radii.iter().enumerate() {
            let spread = full_radius.unwrap_or_else(|| panic!("{SRC}: bad argument #4 to 'format' (number expected, got nil)"));
            t.push(sf!(
                "^8(^7%d^8 +^7 %d^8 - 1) ^ 2 /^7 %d^8 ^ 2 *^7 %d/%d%s",
                small_radius as f64,
                enemy_radius,
                spread,
                count,
                margin_width,
                if i + 1 == n { "" } else { " ^8+" }
            ));
        }
        t.push(sf!("^8=^7 %.3f^8%%", overlap_chance * 100.0));
        off.bd_put("OverlapChance", t);
    }
    let dps_multiplier = if part(off, 2.0) {
        1.0 + small * overlap_chance
    } else if part(off, 3.0) {
        1.0 + small
    } else {
        1.0
    };
    // :7203-7215
    if off.breakdown && (part(off, 2.0) || part(off, 3.0)) {
        let mut t = BTable::lines(["DPS multiplier".to_owned()]);
        if part(off, 2.0) {
            t.push("^8= 1 + ^7small explosions^8 * ^7overlap chance^8");
            t.push(sf!("^8= 1 +^7 %d^8 *^7 %.2f^8", small, overlap_chance));
            t.push(sf!("^8=^7 %.3f", dps_multiplier));
        } else {
            t.push(sf!("^8= 1 +^7 %d (small explosions)", dps_multiplier));
        }
        off.bd_put("SkillDPSMultiplier", t);
    }
    if dps_multiplier != 1.0 {
        mul_dps_out(off, dps_multiplier);
    }
}
