//! `data.misc` + `data.*Constants` + the level-indexed monster/map tables (Data/Misc.lua, Modules/Data.lua:236-326)
//! -> constants.json. Every value is a plain number. `misc` is a struct (the calc formulas' magic numbers: fields in the
//! file's order, an unknown one is drift); the others are open records or arrays, passed through in Lua key order.

use crate::ctx::{Ctx, Obj, R, key_text};
use crate::dump::V;
use crate::fields::Fields;
use crate::js::J;

const FILE: &str = "constants.json";

/// data.misc fields in the file's order.
const MISC: &[&str] = &[
    "AccuracyPerDexBase", "AvoidChanceCap", "BleedDurationBase", "BleedPercentBase", "BlockChanceCap",
    "BrandAttachmentRangeBase", "BuffExpirationSlowCap", "DamageReductionCap", "DodgeChanceCap", "DotDpsCap",
    "EnemyMaxResist", "EnemyPhysicalDamageReductionCap", "EnergyShieldRechargeBase", "EnergyShieldRechargeDelay",
    "EvadeChanceCap", "FortifyBaseDuration", "IgniteDurationBase", "IgnitePercentBase", "ImpaleStoredDamageBase",
    "LeechRateBase", "LowPoolThreshold", "ManaRegenBase", "MatchingSocketQualityBonus", "MaxEnemyLevel", "MaxResistCap",
    "MinStunChanceNeeded", "MineAuraRadiusBase", "MineDetonationRadiusBase", "PlayerMovementSpeed", "PoisonDurationBase",
    "PoisonPercentBase", "ProjectileDistanceCap", "PvpElemental1", "PvpElemental2", "PvpNonElemental1",
    "PvpNonElemental2", "ResistFloor", "ServerTickRate", "ServerTickTime", "StunBaseDuration", "StunBaseMult",
    "StunNotMeleeDamageMult", "SuppressionChanceCap", "SuppressionEffect", "TemporalChainsEffectCap", "Transfiguration",
    "TrapTriggerRadiusBase", "WardRechargeDelay", "ehpCalcMaxDamage", "ehpCalcMaxIterationsToCalc", "ehpCalcSpeedUp",
    "experiencePenaltyMultiplier", "maxExperiencePenaltyFreeAreaLevel", "maxHitSmoothingPasses", "maxStatIncrease",
    "pinnacleBossDPSMult", "pinnacleBossPen", "stdBossDPSMult", "uberBossDPSMult", "uberBossPen",
];

/// Open records of numbers (string or number keys), in Lua key order.
const RECORDS: &[&str] = &["characterConstants", "monsterConstants", "gameConstants"];
/// Level tables: Lua sequences 1..n (level 1 = index 0).
const ARRAYS: &[&str] = &[
    "monsterAccuracyTable", "monsterAilmentThresholdTable", "monsterAllyDamageTable", "monsterAllyLifeTable",
    "monsterArmourTable", "monsterDamageTable", "monsterEvasionTable", "monsterLifeTable", "monsterLifeTable2",
    "monsterLifeTable3", "monsterPhysConversionMultiTable", "monsterExperienceLevelMap", "mapLevelBossAilmentMult",
    "mapLevelBossLifeMult",
];
/// Keyed by something other than 1..n (map level from 66, variety name, a gap at 14): records.
const SPARSE: &[&str] = &["mapLevelLifeMult", "monsterVarietyLifeMult", "totemLifeMult"];

fn num(c: &Ctx, v: &V, at: &str) -> R<J> {
    match v {
        V::Num(n) if n.is_finite() => Ok(J::Num(*n)),
        v => Err(format!("{at}: expected a finite number, got {}", c.show(v))),
    }
}

fn record(c: &mut Ctx, k: &str) -> R<J> {
    let at = format!("data.{k}");
    let v = c.d.path("data", &[k]).ok_or_else(|| format!("{at}: missing"))?;
    let t = c.d.t(c.table(v, &at)?);
    let mut o = Vec::new();
    for (key, x) in &t.entries {
        let ks = key_text(key, &at)?;
        o.push((ks.clone(), num(c, x, &format!("{at}.{ks}"))?));
    }
    c.count(FILE, k, o.len());
    Ok(J::Obj(o))
}

fn array(c: &mut Ctx, k: &str) -> R<J> {
    let at = format!("data.{k}");
    let v = c.d.path("data", &[k]).ok_or_else(|| format!("{at}: missing"))?;
    let items = c.seq(v, &at)?;
    let a = items.iter().enumerate().map(|(i, x)| num(c, x, &format!("{at}[{}]", i + 1))).collect::<R<Vec<_>>>()?;
    c.count(FILE, k, a.len());
    Ok(J::Arr(a))
}

pub fn convert(c: &mut Ctx) -> R<J> {
    let mut o = Obj::default();
    let misc = c.d.path("data", &["misc"]).ok_or("data.misc: missing")?;
    let mut f = Fields::new(c, misc, "data.misc")?;
    let mut m = Obj::default();
    for &k in MISC {
        let v = f.req(k)?;
        m.put(k, num(c, v, &format!("data.misc.{k}"))?);
    }
    f.finish(c, FILE, "misc");
    o.put("misc", m.done());
    for &k in RECORDS {
        o.put(k, record(c, k)?);
    }
    for &k in ARRAYS {
        o.put(k, array(c, k)?);
    }
    for &k in SPARSE {
        o.put(k, record(c, k)?);
    }
    o.put("goldRespecPrices", array(c, "goldRespecPrices")?);
    Ok(o.done())
}
