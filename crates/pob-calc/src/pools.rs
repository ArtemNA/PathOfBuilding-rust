//! The pool helpers of Modules/CalcDefence.lua (R4.4a): `allyLifePoolList` (:33-43), `buildAllyLifePools` (:45-55),
//! `calcs.armourReductionF` (:67-72), `calcs.takenHitFromDamage` (:150-177), `calcLifeHitPoolWithLossPrevention` (:179-183)
//! and `calcs.reducePoolsByDamage` (:190-424) -- line for line, Lua line numbers in the comments. Executable spec: pob-ts
//! `src/calc/pools.ts`. `resourcesLostToTypeDamage` (:246, :276-395, :422) is breakdown-only ([`ResourcesLost`], R8.4).
//!
//! The actor is abstracted by [`PoolActor`] (the handful of `output`/`modDB` reads these helpers do); `CalcActor` implements
//! it in R4.4b, the LuaJIT test (`tests/pools_luajit.rs`) implements it from the fixture's `actorReads`.
//!
//! Tables: upstream only ever keys a damage table (and the per-type aegis/guard/recoup/pool tables) by damage type, so each
//! is a `[Option<f64>; 5]` indexed by [`DmgType`] (`None` = key absent). Every `pairs` walk over such a table is done in
//! `dmgTypeList` order (D13); where the order could reach a float it is named below.
//!
//! In place: `reducePoolsByDamage` rewrites the damage table (each positive value rounded up, the rest removed, :196-200)
//! and drains the ally/aegis/guard/recoup tables of the pool table, handing them back in the result. Here the pool table
//! is taken by value and its tables come back moved into [`PoolsResult`] -- the same values a Lua caller would see.
//!
//! Numbers: `m_min/m_max` = LuaJIT `minsd/maxsd` folds (L7/O9, `pob_lua::number::{min2,max2,min}`), `round` = Common.lua
//! `round`, `m_floor/m_ceil` = `f64::floor/ceil` (both keep `-0`), `m_huge` = infinity. Where upstream would do arithmetic
//! on (or compare) a nil, a [`PoolError`] with the Lua `file:line` is returned (L18).

use std::fmt;

use pob_lua::number::{max2, min, min2, round};

/// `:28` `dmgTypeList`, ordered by the conversion sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DmgType {
    Physical = 0,
    Lightning = 1,
    Cold = 2,
    Fire = 3,
    Chaos = 4,
}

impl DmgType {
    /// `dmgTypeList` in order.
    pub const ALL: [DmgType; 5] = [DmgType::Physical, DmgType::Lightning, DmgType::Cold, DmgType::Fire, DmgType::Chaos];

    pub fn name(self) -> &'static str {
        DMG_TYPE_LIST[self as usize]
    }

    pub fn from_name(name: &str) -> Option<DmgType> {
        DMG_TYPE_LIST.iter().position(|&n| n == name).map(|i| DmgType::ALL[i])
    }

    /// `:24` `isElemental`.
    pub fn is_elemental(self) -> bool {
        matches!(self, DmgType::Fire | DmgType::Cold | DmgType::Lightning)
    }
}

/// `:28` `dmgTypeList` as names.
pub const DMG_TYPE_LIST: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];

/// `<prefix><Type><suffix>` for the five types, in `dmgTypeList` order -- the output keys upstream builds by concatenation.
macro_rules! per_type {
    ($prefix:literal, $suffix:literal) => {
        [
            concat!($prefix, "Physical", $suffix),
            concat!($prefix, "Lightning", $suffix),
            concat!($prefix, "Cold", $suffix),
            concat!($prefix, "Fire", $suffix),
            concat!($prefix, "Chaos", $suffix),
        ]
    };
}

const RESIST_TAKEN_HIT_MULTI: [&str; 5] = per_type!("", "ResistTakenHitMulti");
const EFFECTIVE_APPLIED_ARMOUR: [&str; 5] = per_type!("", "EffectiveAppliedArmour");
const SELF_IGNORE_BASE_DR: [&str; 5] = per_type!("SelfIgnoreBase", "DamageReduction");
const BASE_DR_WHEN_HIT: [&str; 5] = per_type!("Base", "DamageReductionWhenHit");
const BASE_DR: [&str; 5] = per_type!("Base", "DamageReduction");
const SELF_IGNORE_DR: [&str; 5] = per_type!("SelfIgnore", "DamageReduction");
const ENEMY_OVERWHELM: [&str; 5] = per_type!("", "EnemyOverwhelm");
const TAKEN_FLAT: [&str; 5] = per_type!("", "takenFlat");
const AFTER_REDUCTION_TAKEN_HIT_MULTI: [&str; 5] = per_type!("", "AfterReductionTakenHitMulti");
const AEGIS: [&str; 5] = per_type!("", "Aegis");
const GUARD_ABSORB: [&str; 5] = per_type!("", "GuardAbsorb");
const GUARD_ABSORB_RATE: [&str; 5] = per_type!("", "GuardAbsorbRate");
const MIND_OVER_MATTER: [&str; 5] = per_type!("", "MindOverMatter");
const ENERGY_SHIELD_BYPASS: [&str; 5] = per_type!("", "EnergyShieldBypass");

/// One entry of `:33-43` `allyLifePoolList`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllyLifePool {
    pub key: &'static str,
    pub life: &'static str,
    pub mitigation: &'static str,
    pub redirect: Option<&'static str>,
    pub fallback: Option<&'static str>,
    pub melee_only: bool,
    pub label: &'static str,
}

const fn ally(
    key: &'static str,
    life: &'static str,
    mitigation: &'static str,
    redirect: Option<&'static str>,
    fallback: Option<&'static str>,
    melee_only: bool,
    label: &'static str,
) -> AllyLifePool {
    AllyLifePool {
        key,
        life,
        mitigation,
        redirect,
        fallback,
        melee_only,
        label,
    }
}

/// `:33-43` -- damage is shifted to external allies before Frost Shield; also the order in which the pools are drained.
pub const ALLY_LIFE_POOL_LIST: [AllyLifePool; 9] = [
    ally(
        "minion",
        "TotalMinionLife",
        "MinionAllyDamageMitigation",
        Some("takenFromMinionBeforeYou"),
        Some("Multiplier:MinionLife"),
        false,
        "Minion Life",
    ),
    ally(
        "radianceSentinel",
        "TotalRadianceSentinelLife",
        "RadianceSentinelAllyDamageMitigation",
        Some("takenFromRadianceSentinelBeforeYou"),
        None,
        false,
        "Total Sentinel of Radiance Life",
    ),
    ally(
        "spectres",
        "TotalSpectreLife",
        "SpectreAllyDamageMitigation",
        Some("takenFromSpectresBeforeYou"),
        None,
        false,
        "Total Spectre Life",
    ),
    ally(
        "totems",
        "TotalTotemLife",
        "TotemAllyDamageMitigation",
        Some("takenFromTotemsBeforeYou"),
        None,
        false,
        "Nearest Totem Life",
    ),
    ally(
        "vaalRejuvenationTotems",
        "TotalVaalRejuvenationTotemLife",
        "VaalRejuvenationTotemAllyDamageMitigation",
        Some("takenFromVaalRejuvenationTotemsBeforeYou"),
        None,
        false,
        "Total Vaal Rejuvenation Totem Life",
    ),
    ally(
        "voidSpawn",
        "TotalVoidSpawnLife",
        "VoidSpawnAllyDamageMitigation",
        Some("takenFromVoidSpawnBeforeYou"),
        None,
        false,
        "Total Void Spawn Life",
    ),
    ally(
        "stoneGolem",
        "TotalStoneGolemLife",
        "StoneGolemAllyDamageMitigation",
        Some("takenFromStoneGolemBeforeYou"),
        None,
        true,
        "Total Stone Golem Life",
    ),
    ally(
        "soulLink",
        "AlliedEnergyShield",
        "SoulLinkMitigation",
        None,
        None,
        false,
        "Total Allied Energy shield",
    ),
    ally(
        "frostShield",
        "FrostShieldLife",
        "FrostShieldDamageMitigation",
        None,
        None,
        false,
        "Frost Shield Life",
    ),
];

/// What the pool helpers read off an actor. `output` answers `None` for nil (and for any non-number, which upstream could
/// not do arithmetic on either); `flag` is `modDB:Flag(nil, name)`; `sum_*` is `modDB:Sum(kind, nil, name)`;
/// `shift_table(t)` is `actor.damageShiftTable[t]` as a row in `dmgTypeList` order (`None` row = nil table).
pub trait PoolActor {
    fn output(&self, key: &str) -> Option<f64>;
    fn flag(&self, name: &str) -> bool;
    fn sum_base(&self, name: &str) -> f64;
    fn sum_more(&self, name: &str) -> f64;
    fn shift_table(&self, damage_type: DmgType) -> Option<[Option<f64>; 5]>;
}

/// A Lua runtime error upstream would raise (arithmetic on / comparison with a nil), with its `file:line`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolError {
    pub line: u32,
    pub message: String,
}

impl fmt::Display for PoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Modules/CalcDefence.lua:{}: {}", self.line, self.message)
    }
}

impl std::error::Error for PoolError {}

/// A value upstream does arithmetic on or compares: nil is an error at `line`.
fn need(v: Option<f64>, line: u32, what: &str) -> Result<f64, PoolError> {
    v.ok_or_else(|| PoolError {
        line,
        message: format!("attempt to use a nil value ({what}) in arithmetic/comparison"),
    })
}

/// Damage per type, `None` = key absent.
pub type DamageTable = [Option<f64>; 5];

/// One ally pool (`:51`). `damage_type` is tested at :271 but written nowhere upstream, so it is always `None` there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AllyPool {
    pub remaining: f64,
    pub percent: f64,
    pub damage_type: Option<DmgType>,
}

/// `alliesTakenBeforeYou`, indexed like [`ALLY_LIFE_POOL_LIST`] (`None` = no pool for that ally).
pub type AllyPools = [Option<AllyPool>; 9];

/// `Aegis` table: `shared`, `sharedElemental` and one entry per damage type (`None` = key absent).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Aegis {
    pub shared: Option<f64>,
    pub shared_elemental: Option<f64>,
    pub typed: [Option<f64>; 5],
}

/// `Guard` table: `shared` and one entry per damage type (`None` = key absent).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Guard {
    pub shared: Option<f64>,
    pub typed: [Option<f64>; 5],
}

/// `poolTable` -- every field optional; a missing one falls back to the actor's output (:202-236).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PoolTable {
    pub allies_taken_before_you: Option<AllyPools>,
    pub aegis: Option<Aegis>,
    pub guard: Option<Guard>,
    pub ward: Option<f64>,
    pub ward_active_chance: Option<f64>,
    pub energy_shield: Option<f64>,
    pub mana: Option<f64>,
    pub life: Option<f64>,
    pub life_loss_lost_over_time: Option<f64>,
    pub life_below_half_loss_lost_over_time: Option<f64>,
    pub damage_taken_that_can_be_recouped: Option<[Option<f64>; 5]>,
}

/// `resourcesLostToTypeDamage[damageType]` (:247): what each pool lost to that type of the hit (`nil` below 1) -- read by the breakdown
/// only (`incomingDamageBreakdown`, CalcDefence.lua:3805-3869; R8.4).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourcesLost {
    /// `[ally.key]`, by `ALLY_LIFE_POOL_LIST` index.
    pub allies: [Option<f64>; 9],
    pub aegis: Option<f64>,
    pub shared_elemental_aegis: Option<f64>,
    pub shared_aegis: Option<f64>,
    pub guard: Option<f64>,
    pub shared_guard: Option<f64>,
    pub ward: Option<f64>,
    pub energy_shield: Option<f64>,
    pub mana: Option<f64>,
    pub life_loss_prevented: Option<f64>,
    pub life: Option<f64>,
    pub overkill: Option<f64>,
}

/// `x >= 1 and x or nil`
fn ge1(x: f64) -> Option<f64> {
    if x >= 1.0 { Some(x) } else { None }
}

/// What `reducePoolsByDamage` returns (:408-423).
#[derive(Clone, Debug, PartialEq)]
pub struct PoolsResult {
    /// `resourcesLostToTypeDamage` (breakdown only), by `dmgTypeList` index.
    pub resources_lost: [ResourcesLost; 5],
    pub allies_taken_before_you: AllyPools,
    pub aegis: Aegis,
    pub guard: Guard,
    pub ward: f64,
    pub ward_active_chance: f64,
    pub energy_shield: f64,
    pub mana: f64,
    pub life: f64,
    pub damage_taken_that_can_be_recouped: [Option<f64>; 5],
    pub life_loss_lost_over_time: f64,
    pub life_below_half_loss_lost_over_time: f64,
    pub overkill_damage: f64,
    pub hit_pool_remaining: f64,
}

impl From<PoolsResult> for PoolTable {
    /// The result is itself a complete `poolTable` for the next hit (`numberOfHitsToDie`).
    fn from(r: PoolsResult) -> PoolTable {
        PoolTable {
            allies_taken_before_you: Some(r.allies_taken_before_you),
            aegis: Some(r.aegis),
            guard: Some(r.guard),
            ward: Some(r.ward),
            ward_active_chance: Some(r.ward_active_chance),
            energy_shield: Some(r.energy_shield),
            mana: Some(r.mana),
            life: Some(r.life),
            life_loss_lost_over_time: Some(r.life_loss_lost_over_time),
            life_below_half_loss_lost_over_time: Some(r.life_below_half_loss_lost_over_time),
            damage_taken_that_can_be_recouped: Some(r.damage_taken_that_can_be_recouped),
        }
    }
}

// --- CalcDefence.lua:45-55 `buildAllyLifePools` ---------------------------------------------------------------------

/// Every ally that takes part of a hit before you, with its life and share.
pub fn build_ally_life_pools<A: PoolActor + ?Sized>(actor: &A) -> AllyPools {
    let mut pools: AllyPools = [None; 9];
    for (slot, ally) in pools.iter_mut().zip(ALLY_LIFE_POOL_LIST.iter()) {
        let life = actor.output(ally.life);
        let mitigation = actor.output(ally.mitigation);
        // :50 `life and life > 0 and mitigation and mitigation > 0`
        if let (Some(life), Some(mitigation)) = (life, mitigation)
            && life > 0.0
            && mitigation > 0.0
        {
            *slot = Some(AllyPool {
                remaining: life,
                percent: min2(mitigation, 100.0) / 100.0,
                damage_type: None,
            });
        }
    }
    pools
}

// --- CalcDefence.lua:67-72 `calcs.armourReductionF` -----------------------------------------------------------------

/// Damage reduction from armour, float. `0/0` is dodged by the explicit zero case.
pub fn armour_reduction_f(armour: f64, raw: f64) -> f64 {
    if armour == 0.0 && raw == 0.0 {
        return 0.0;
    }
    armour / (armour + raw * 5.0) * 100.0
}

// --- CalcDefence.lua:150-177 `calcs.takenHitFromDamage` -------------------------------------------------------------

/// The damage an incoming hit of `raw_damage` (after the enemy damage multiplier) actually takes off, per "taken as" type.
/// Returns the two Lua values: the sum and the per-type table.
pub fn taken_hit_from_damage<A: PoolActor + ?Sized>(
    raw_damage: f64,
    damage_type: DmgType,
    actor: &A,
) -> Result<(f64, DamageTable), PoolError> {
    // :154-163 resistance x (armour + flat DR - overwhelm) for the part taken as `t`
    let mitigation_multiplier_for_type = |damage: f64, t: usize| -> Result<f64, PoolError> {
        let total_resist_mult = need(actor.output(RESIST_TAKEN_HIT_MULTI[t]), 157, RESIST_TAKEN_HIT_MULTI[t])?;
        // :157 -> :71 a nil armour fails inside `armourReductionF` (`nil == 0` is false, then the division)
        let effective_applied_armour = need(actor.output(EFFECTIVE_APPLIED_ARMOUR[t]), 71, EFFECTIVE_APPLIED_ARMOUR[t])?;
        let armour_dr_percent = armour_reduction_f(effective_applied_armour, damage * total_resist_mult);
        // :158 `Flag and 0 or WhenHit or Base`
        let flat_dr_percent = if actor.flag(SELF_IGNORE_BASE_DR[t]) {
            Some(0.0)
        } else {
            actor.output(BASE_DR_WHEN_HIT[t]).or_else(|| actor.output(BASE_DR[t]))
        };
        let flat_dr_percent = need(flat_dr_percent, 159, BASE_DR[t])?;
        let total_dr_percent = min2(
            need(actor.output("DamageReductionMax"), 159, "DamageReductionMax")?,
            armour_dr_percent + flat_dr_percent,
        );
        // :160
        let enemy_overwhelm_percent = if actor.flag(SELF_IGNORE_DR[t]) {
            0.0
        } else {
            need(actor.output(ENEMY_OVERWHELM[t]), 161, ENEMY_OVERWHELM[t])?
        };
        let dr_max = need(actor.output("DamageReductionMax"), 161, "DamageReductionMax")?;
        let total_dr_multi = 1.0 - max2(min2(dr_max, total_dr_percent - enemy_overwhelm_percent), 0.0) / 100.0;
        Ok(total_resist_mult * total_dr_multi)
    };
    let mut received_damage_sum = 0.0;
    let mut damages: DamageTable = [None; 5];
    // :166 `pairs(actor.damageShiftTable[damageType])` adds floats to `receivedDamageSum` in HASH order (D13/L10); the
    // port walks `dmgTypeList` order, as pob-ts. On every golden each shift row is the identity (100 on the diagonal) and
    // `<T>takenFlat` is 0, so exactly one addend reaches the sum and no order can show.
    let row = actor.shift_table(damage_type).ok_or_else(|| PoolError {
        line: 166,
        message: "bad argument #1 to 'for iterator' (table expected, got nil)".to_owned(),
    })?;
    for (t, convert_percent) in row.iter().enumerate() {
        let Some(convert_percent) = *convert_percent else { continue };
        let taken_flat = actor.output(TAKEN_FLAT[t]);
        // :168 `nil ~= 0` is true (a nil `takenFlat` then fails at the `+` on :171)
        if convert_percent > 0.0 || taken_flat.is_none_or(|v| v != 0.0) {
            let converted_damage = raw_damage * convert_percent / 100.0;
            // :170 `-Sum("MORE")` without mods is -0, so `1 - vaalArctic` is exactly 1 (O9).
            let vaal_arctic = min2(-actor.sum_more("VaalArcticArmourMitigation") / 100.0, 1.0);
            let mitigation = mitigation_multiplier_for_type(converted_damage, t)?;
            let taken_flat = need(taken_flat, 171, TAKEN_FLAT[t])?;
            let after = need(
                actor.output(AFTER_REDUCTION_TAKEN_HIT_MULTI[t]),
                171,
                AFTER_REDUCTION_TAKEN_HIT_MULTI[t],
            )?;
            let reduced_damage = round(max2(converted_damage * mitigation + taken_flat, 0.0) * after, None) * (1.0 - vaal_arctic);
            received_damage_sum += reduced_damage;
            // :173 `(...) and reducedDamage or nil` -- a number is truthy (0 included); nil leaves the key out.
            if reduced_damage > 0.0 || convert_percent > 0.0 {
                damages[t] = Some(reduced_damage);
            }
        }
    }
    Ok((received_damage_sum, damages))
}

// --- CalcDefence.lua:179-183 `calcLifeHitPoolWithLossPrevention` ----------------------------------------------------

/// The life pool a single hit has to chew through when part of the loss is prevented, the below-half part prevented twice.
pub fn calc_life_hit_pool_with_loss_prevention(
    life: f64,
    max_life: f64,
    life_loss_prevented: f64,
    life_loss_below_half_prevented: f64,
) -> f64 {
    let half_life = max_life * 0.5;
    let above_low = max2(life - half_life, 0.0);
    above_low / (1.0 - life_loss_prevented / 100.0)
        + min2(life, half_life) / (1.0 - life_loss_below_half_prevented / 100.0) / (1.0 - life_loss_prevented / 100.0)
}

/// `calcLifeHitPoolWithLossPrevention(life, output.Life, output.preventedLifeLoss, ...)` -- the nils fail inside, :180/:182.
fn life_hit_pool<A: PoolActor + ?Sized>(actor: &A, life: f64, life_loss_below_half_prevented: f64) -> Result<f64, PoolError> {
    let max_life = need(actor.output("Life"), 180, "Life")?;
    let prevented = need(actor.output("preventedLifeLoss"), 182, "preventedLifeLoss")?;
    Ok(calc_life_hit_pool_with_loss_prevention(
        life,
        max_life,
        prevented,
        life_loss_below_half_prevented,
    ))
}

// --- CalcDefence.lua:190-424 `calcs.reducePoolsByDamage` ------------------------------------------------------------

/// Drains the pools with one hit, in upstream's order: allies, aegis (typed, shared elemental, shared), guard (typed,
/// shared), ward, energy shield, Mind over Matter (mana, ES protecting mana), prevented life loss, life; the rest is
/// overkill. `pool_table` `None` = `nil` (all pools from output). `damage_table` is rewritten in place (:196-200).
pub fn reduce_pools_by_damage<A: PoolActor + ?Sized>(
    pool_table: Option<PoolTable>,
    damage_table: &mut DamageTable,
    actor: &A,
) -> Result<PoolsResult, PoolError> {
    // :194
    let pool_tbl = pool_table.unwrap_or_default();

    // :196-200 -- IN PLACE: each positive damage rounded up, the rest (NaN included) removed. The total is a sum of
    // integers, so the `pairs` order can show only past 2^53.
    let mut damage_total = 0.0;
    for slot in damage_table.iter_mut() {
        let Some(damage) = *slot else { continue };
        *slot = if damage > 0.0 { Some(damage.ceil()) } else { None };
        damage_total += slot.unwrap_or(0.0);
    }
    let present: [bool; 5] = damage_table.map(|d| d.is_some());

    // :202-205
    let mut allies = match pool_tbl.allies_taken_before_you {
        Some(a) => a,
        None => build_ally_life_pools(actor),
    };
    // :207
    let mut recouped = pool_tbl.damage_taken_that_can_be_recouped.unwrap_or([None; 5]);
    // :208-217 -- the per-type aegis only for the types in this hit (`output.x or 0`)
    let mut aegis = match pool_tbl.aegis {
        Some(a) => a,
        None => {
            let mut a = Aegis {
                shared: Some(actor.output("sharedAegis").unwrap_or(0.0)),
                shared_elemental: Some(actor.output("sharedElementalAegis").unwrap_or(0.0)),
                typed: [None; 5],
            };
            for t in (0..5).filter(|&t| present[t]) {
                a.typed[t] = Some(actor.output(AEGIS[t]).unwrap_or(0.0));
            }
            a
        }
    };
    // :218-224
    let mut guard = match pool_tbl.guard {
        Some(g) => g,
        None => {
            let mut g = Guard {
                shared: Some(actor.output("sharedGuardAbsorb").unwrap_or(0.0)),
                typed: [None; 5],
            };
            for t in (0..5).filter(|&t| present[t]) {
                g.typed[t] = Some(actor.output(GUARD_ABSORB[t]).unwrap_or(0.0));
            }
            g
        }
    };

    // :226-229 -- `x or y or 0` over numbers; a pool's 0 is truthy and stays (L4).
    let mut ward = pool_tbl.ward.or_else(|| actor.output("Ward")).unwrap_or(0.0);
    // :227 `(ward > 0 and 1 or 0)` -- read BEFORE the clamp at :239.
    let ward_active_chance = pool_tbl.ward_active_chance.unwrap_or(if ward > 0.0 { 1.0 } else { 0.0 });
    let ward_avoid_break_chance = if actor.flag("Condition:WardNotBreak") {
        1.0
    } else {
        min2(actor.sum_base("WardAvoidBreakChance") / 100.0, 1.0)
    };
    let ward_bypass_below = actor.sum_base("WardBypassBelowPercent") / 100.0;

    // :231 no `or 0` on the energy shield: a nil fails the comparison at :241.
    let mut energy_shield = need(
        pool_tbl.energy_shield.or_else(|| actor.output("EnergyShieldRecoveryCap")),
        241,
        "energyShield",
    )?;
    let mut mana = pool_tbl.mana.or_else(|| actor.output("ManaUnreserved")).unwrap_or(0.0);
    let mut life = pool_tbl.life.or_else(|| actor.output("LifeRecoverable")).unwrap_or(0.0);
    let life_loss_below_half_prevented = actor.sum_base("LifeLossBelowHalfPrevented");
    let mut life_loss_lost_over_time = pool_tbl.life_loss_lost_over_time.unwrap_or(0.0);
    let mut life_below_half_loss_lost_over_time = pool_tbl.life_below_half_loss_lost_over_time.unwrap_or(0.0);
    let mut overkill_damage = 0.0;

    // :239-243 `x >= 0 and x or 0` -- a plain clamp (NaN -> 0).
    ward = if ward >= 0.0 { ward } else { 0.0 };
    let ward_before_hit = ward;
    energy_shield = if energy_shield >= 0.0 { energy_shield } else { 0.0 };
    mana = if mana >= 0.0 { mana } else { 0.0 };
    life = if life >= 0.0 { life } else { 0.0 };

    // :247
    let mut lost: [ResourcesLost; 5] = Default::default();
    // :245-264 the MoM (+ES protecting mana, +bypass) pools, per damage type in the hit, independently.
    let life_hit_pool_initial = life_hit_pool(actor, life, life_loss_below_half_prevented)?;
    let mut mom_pools_remaining = [0.0f64; 5];
    let mut es_pools_remaining = [0.0f64; 5];
    for t in (0..5).filter(|&t| present[t]) {
        let mom_effect = min2(
            need(actor.output("sharedMindOverMatter"), 251, "sharedMindOverMatter")?
                + need(actor.output(MIND_OVER_MATTER[t]), 251, MIND_OVER_MATTER[t])?,
            100.0,
        ) / 100.0;
        // MoMEffect == 1 -> `x / 0 - x` = inf (or NaN for a 0 pool), exactly as in Lua.
        let max_mom = life_hit_pool_initial / (1.0 - mom_effect) - life_hit_pool_initial;
        let mom_pool = if mom_effect < 1.0 { min2(max_mom, mana) } else { mana };
        let life_plus_mom_hit_pool = life_hit_pool_initial + mom_pool;
        // :255 `output[...] / 100 or 0` -- the quotient is a number, the `or 0` is dead (a nil errors first).
        let es_bypass = need(actor.output(ENERGY_SHIELD_BYPASS[t]), 255, ENERGY_SHIELD_BYPASS[t])? / 100.0;
        let es_pool = if es_bypass > 0.0 {
            min2(life_plus_mom_hit_pool / es_bypass - life_plus_mom_hit_pool, energy_shield)
        } else {
            energy_shield
        };
        if actor.flag("EnergyShieldProtectsMana") && es_bypass < 1.0 {
            // :258 a three-argument min (O9)
            es_pools_remaining[t] = min(
                max_mom * (1.0 - es_bypass),
                &[
                    energy_shield,
                    life_plus_mom_hit_pool / (1.0 - (1.0 - es_bypass) * mom_effect) - life_plus_mom_hit_pool,
                ],
            )
            .floor();
            mom_pools_remaining[t] = min2(max_mom - es_pools_remaining[t], mom_pool).floor();
        } else {
            es_pools_remaining[t] = es_pool.floor();
            mom_pools_remaining[t] = mom_pool.floor();
        }
    }

    // :266-397 the hit, type by type in dmgTypeList order
    for t in 0..5 {
        let Some(mut damage_remainder) = damage_table[t] else { continue };
        let damage_type = DmgType::ALL[t];
        // :269-279 allies take their share first
        for (k, ally) in allies.iter_mut().enumerate() {
            let Some(ally) = ally else { continue };
            if ally.damage_type.is_none_or(|d| d == damage_type) && ally.remaining > 0.0 {
                let temp_damage = min2(damage_remainder * ally.percent, ally.remaining);
                ally.remaining = (ally.remaining - temp_damage).floor();
                damage_remainder -= temp_damage;
                lost[t].allies[k] = ge1(temp_damage);
            }
        }
        // :280-281 what allies took before you does not count as you taking damage
        recouped[t] = Some(recouped[t].unwrap_or(0.0) + damage_remainder);
        // :282-299 aegis: typed, shared elemental (elements only), shared
        let typed_aegis = need(aegis.typed[t], 282, "aegis[damageType]")?;
        if typed_aegis > 0.0 {
            let temp_damage = min2(damage_remainder, typed_aegis);
            aegis.typed[t] = Some(typed_aegis - temp_damage);
            damage_remainder -= temp_damage;
            lost[t].aegis = ge1(temp_damage);
        }
        if damage_type.is_elemental() {
            let shared_elemental = need(aegis.shared_elemental, 288, "aegis.sharedElemental")?;
            if shared_elemental > 0.0 {
                let temp_damage = min2(damage_remainder, shared_elemental);
                aegis.shared_elemental = Some(shared_elemental - temp_damage);
                damage_remainder -= temp_damage;
                lost[t].shared_elemental_aegis = ge1(temp_damage);
            }
        }
        let shared_aegis = need(aegis.shared, 294, "aegis.shared")?;
        if shared_aegis > 0.0 {
            let temp_damage = min2(damage_remainder, shared_aegis);
            aegis.shared = Some(shared_aegis - temp_damage);
            damage_remainder -= temp_damage;
            lost[t].shared_aegis = ge1(temp_damage);
        }
        // :300-311 guard: typed, then shared
        let typed_guard = need(guard.typed[t], 300, "guard[damageType]")?;
        if typed_guard > 0.0 {
            let rate = need(actor.output(GUARD_ABSORB_RATE[t]), 301, GUARD_ABSORB_RATE[t])?;
            let temp_damage = min2(damage_remainder * rate / 100.0, typed_guard);
            guard.typed[t] = Some((typed_guard - temp_damage).floor());
            damage_remainder -= temp_damage;
            lost[t].guard = ge1(temp_damage);
        }
        let shared_guard = need(guard.shared, 306, "guard.shared")?;
        if shared_guard > 0.0 {
            let rate = need(actor.output("sharedGuardAbsorbRate"), 307, "sharedGuardAbsorbRate")?;
            let temp_damage = min2(damage_remainder * rate / 100.0, shared_guard);
            guard.shared = Some((shared_guard - temp_damage).floor());
            damage_remainder -= temp_damage;
            lost[t].shared_guard = ge1(temp_damage);
        }
        // :312-318 ward (`(Sum or 0)` -- Sum is always a number, the `or 0` is dead)
        if ward > 0.0 && (ward_bypass_below == 0.0 || damage_total >= ward_before_hit * ward_bypass_below) {
            let mut temp_damage = min2(damage_remainder * (1.0 - actor.sum_base("WardBypass") / 100.0), ward);
            ward -= temp_damage;
            temp_damage *= ward_active_chance;
            damage_remainder -= temp_damage;
            lost[t].ward = ge1(temp_damage);
        }
        // :319-321 -- `es_pool` is this type's pool as it stood BEFORE the energy shield takes its part below.
        let mom_effect = min2(
            need(actor.output("sharedMindOverMatter"), 319, "sharedMindOverMatter")?
                + need(actor.output(MIND_OVER_MATTER[t]), 319, MIND_OVER_MATTER[t])?,
            100.0,
        ) / 100.0;
        let es_bypass = need(actor.output(ENERGY_SHIELD_BYPASS[t]), 320, ENERGY_SHIELD_BYPASS[t])? / 100.0;
        let es_pool = es_pools_remaining[t];
        // :322-330 energy shield; every type's pool shrinks with it (independently per key)
        if energy_shield > 0.0 && !actor.flag("EnergyShieldProtectsMana") && es_bypass < 1.0 {
            let temp_damage = min2(damage_remainder * (1.0 - es_bypass), es_pool);
            for t2 in (0..5).filter(|&t2| present[t2]) {
                es_pools_remaining[t2] = max2(es_pools_remaining[t2] - temp_damage, 0.0);
            }
            energy_shield -= temp_damage;
            damage_remainder -= temp_damage;
            lost[t].energy_shield = ge1(temp_damage);
        }
        // :331-350 Mind over Matter (with energy shield protecting the mana first)
        if mom_effect > 0.0 {
            let mut mom_damage = (damage_remainder * mom_effect).ceil();
            if actor.flag("EnergyShieldProtectsMana") && energy_shield > 0.0 && es_bypass < 1.0 {
                let temp_damage = min2(mom_damage * (1.0 - es_bypass), es_pool).ceil();
                for t2 in (0..5).filter(|&t2| present[t2]) {
                    es_pools_remaining[t2] = max2(es_pools_remaining[t2] - temp_damage, 0.0).floor();
                }
                energy_shield -= temp_damage;
                damage_remainder -= temp_damage;
                mom_damage -= temp_damage;
                lost[t].energy_shield = ge1(temp_damage);
            }
            let temp_damage = min2(mom_damage, mom_pools_remaining[t]).ceil();
            for t2 in (0..5).filter(|&t2| present[t2]) {
                mom_pools_remaining[t2] = max2(mom_pools_remaining[t2] - temp_damage, 0.0).floor();
            }
            mana -= temp_damage;
            damage_remainder -= temp_damage;
            lost[t].mana = ge1(temp_damage);
        }
        // :351-387 prevented life loss (Progenesis, Petrified Blood)
        if need(actor.output("preventedLifeLossTotal"), 351, "preventedLifeLossTotal")? > 0.0 {
            let half_life = need(actor.output("Life"), 352, "Life")? * 0.5;
            let life_over_half_life = max2(life - half_life, 0.0);
            let prevent_percent = need(actor.output("preventedLifeLoss"), 354, "preventedLifeLoss")? / 100.0;
            let pool_above_low = life_over_half_life / (1.0 - prevent_percent);
            let prevent_below_half_percent = life_loss_below_half_prevented / 100.0;
            let damage_that_life_can_still_take = pool_above_low
                + max2(min2(life, half_life), 0.0)
                    / (1.0 - prevent_below_half_percent)
                    / (1.0 - need(actor.output("preventedLifeLoss"), 357, "preventedLifeLoss")? / 100.0);
            if damage_that_life_can_still_take < damage_remainder {
                overkill_damage = overkill_damage + damage_remainder - damage_that_life_can_still_take;
                damage_remainder = damage_that_life_can_still_take;
            }
            // :362 `~= 0` -- a nil is "not 0" too.
            if actor.output("preventedLifeLossBelowHalf").is_none_or(|v| v != 0.0) {
                let damage_to_split = min2(damage_remainder, pool_above_low);
                let lost_life = damage_to_split * (1.0 - prevent_percent);
                let prevented_loss = damage_to_split * prevent_percent;
                damage_remainder -= damage_to_split;
                life_loss_lost_over_time += prevented_loss;
                life -= lost_life;
                lost[t].life = Some(lost_life);
                let mut prevented = prevented_loss;
                if life <= half_life {
                    let unspecifically_low_life_prevented_damage = damage_remainder * prevent_percent;
                    life_loss_lost_over_time += unspecifically_low_life_prevented_damage;
                    damage_remainder -= unspecifically_low_life_prevented_damage;
                    let specifically_low_life_prevented_damage = damage_remainder * prevent_below_half_percent;
                    life_below_half_loss_lost_over_time += specifically_low_life_prevented_damage;
                    damage_remainder -= specifically_low_life_prevented_damage;
                    prevented = prevented + unspecifically_low_life_prevented_damage + specifically_low_life_prevented_damage;
                }
                lost[t].life_loss_prevented = ge1(prevented);
            } else {
                let temp_damage = damage_remainder * need(actor.output("preventedLifeLoss"), 382, "preventedLifeLoss")? / 100.0;
                life_loss_lost_over_time += temp_damage;
                damage_remainder -= temp_damage;
                lost[t].life_loss_prevented = ge1(temp_damage);
            }
        }
        // :388-393 life
        if life > 0.0 {
            let temp_damage = min2(damage_remainder, life);
            life -= temp_damage;
            damage_remainder -= temp_damage;
            lost[t].life = Some(lost[t].life.unwrap_or(0.0) + if temp_damage > 0.0 { temp_damage } else { 0.0 });
        }
        // :394-395
        overkill_damage += damage_remainder;
        lost[t].overkill = ge1(damage_remainder);
    }

    // :399-404 the smallest pool left over the types in the hit. `m_min` depends on the order only for NaN/±0 operands
    // (floors of pools); walked in dmgTypeList order (D13).
    let mut mom_pool_remaining = f64::INFINITY;
    let mut es_pool_remaining = f64::INFINITY;
    for t in (0..5).filter(|&t| present[t]) {
        mom_pool_remaining = min2(mom_pool_remaining, mom_pools_remaining[t]);
        es_pool_remaining = min2(es_pool_remaining, es_pools_remaining[t]);
    }
    // :405-406 `life >= 1 and calc(...) or 0` -- the pool is a number, so a plain conditional.
    let life_part = if life >= 1.0 {
        life_hit_pool(actor, life, life_loss_below_half_prevented)?
    } else {
        0.0
    };
    let hit_pool_remaining = life_part
        + (if mom_pool_remaining != f64::INFINITY {
            mom_pool_remaining
        } else {
            0.0
        })
        + (if es_pool_remaining != f64::INFINITY {
            es_pool_remaining
        } else {
            0.0
        });

    // :408-423
    Ok(PoolsResult {
        resources_lost: lost,
        allies_taken_before_you: allies,
        aegis,
        guard,
        // :412 `ward < before and (chance > 0 and before or 0) or ward`: the inner value is a NUMBER (0 included), so a
        // broken ward reports `before` or 0 -- never falls through to `ward` (L4).
        ward: (if ward < ward_before_hit {
            if ward_avoid_break_chance > 0.0 { ward_before_hit } else { 0.0 }
        } else {
            ward
        })
        .floor(),
        // :413 the product is a number (0 is truthy) -- a plain conditional.
        ward_active_chance: if ward < ward_before_hit {
            ward_active_chance * ward_avoid_break_chance
        } else {
            ward_active_chance
        },
        energy_shield: energy_shield.floor(),
        mana: mana.floor(),
        life: life.floor(),
        damage_taken_that_can_be_recouped: recouped,
        life_loss_lost_over_time: life_loss_lost_over_time.ceil(),
        life_below_half_loss_lost_over_time: life_below_half_loss_lost_over_time.ceil(),
        overkill_damage: overkill_damage.ceil(),
        hit_pool_remaining: hit_pool_remaining.floor(),
    })
}
