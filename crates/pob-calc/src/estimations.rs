//! `calcs.buildDefenceEstimations(env, actor)` -- Modules/CalcDefence.lua:1674-3870 -- line for line, with the breakdown
//! tables (R8.4, `actor.breakdown`). Executable spec: pob-ts `src/calc/estimations.ts` (P4.4c1 + P4.4c2). Part 1 (:1674-2610): not-hit chance, enemy
//! damage (config / placeholder / enemyDB), the "taken as" shift tables, taken multipliers, the per-type hit, stun, life
//! recoverable / loss prevention, ES bypass, Mind over Matter, guard, aegis, allies, Vaal Arctic Armour, total pools. Part 2
//! (:2612-3786): `numberOfHitsToDie`, number of hits / EHP / survival time, recoup, petrified blood, net recovery, PvP,
//! maximum hit taken (which first rewrites `<T>TotalHitPool`), DoT EHP and degens -- on top of [`crate::pools`].
//! Called from CalcPerform.lua:3793-3795 (`skipEHP` is nil on MAIN).
//!
//! Breakdown (MINEFIELD-RUST R8): MAIN builds it, so every `if breakdown then` branch of :1674-3871 runs upstream. All 42
//! were checked (R4.4b report): :1720, :1823, :1871, :1892, :1903, :1915, :1972, :1988, :2043, :2091, :2186, :2200, :2211,
//! :2240, :2291, :2372, :2403, :2430, :2448, :2492, :2544, :2598, :2885, :2905, :2953, :2967, :3014, :3071, :3101, :3132,
//! :3310, :3374, :3394, :3445, :3493, :3513, :3561, :3630, :3686, :3735, :3755 and the closing :3789-3870 (which also runs
//! `reducePoolsByDamage(nil, takenHit, actor)` and `incomingDamageBreakdown` into locals) -- each writes only `breakdown.*`
//! tables or locals; none writes `output`, a condition, a multiplier or a mod. R8.4 ports them all (`bd`, gated by
//! `tests/breakdown.rs` against LuaJIT); rows built in a `pairs` order (:3326, :3389, :3556, :3618/:3625 -- R13) follow
//! `dmgTypeList` order here and are compared as sets.
//!
//! Upstream quirks kept verbatim (MINEFIELD D21): `:1695` AverageEvadeChance over 4; `:1755` the `enemy<T>enemyOverwhelm`
//! placeholder typo; `:1807` the enemy's physical overwhelm added to every type; `:1958` `AnyTakenReflect` false in both
//! branches; `:2000` `ImpaleAvoidChance` used as a fraction; `:2003` `<T>ResistWhenHit` never written; `:2024` Spell and
//! SpellProjectile both read `...FromSpellProjectiles`; `:2233-2237` "Average" takes the `~= "Melee"` branch; `:2395`
//! `OnlySharedMindOverMatter` never true; `:2424-2446` a shared guard does not set `AnyGuard`; `:2531-2533` the ally life key
//! is removed; `:2691/:2705-2710` `MissingLife/ManaBeforeEnemyHit` are numbers (0 is truthy); `:3181/:3183` `shared or 0 +
//! typed or 0`; `:3176` the dead `or 0`; `-0` from `:2576` (O9) is kept. Dead writes not ported (read by nothing): `:2669`
//! `DamageIn.WardBypass`, `:2851` `DamageIn[T.."EnergyShieldBypass"]`.
//!
//! D13 (float sums in `pairs` hash order), fixed to pob-ts' order: `:166` (pools.rs), `:3389`, `:3556`, `:3625`
//! (`dmgTypeList` order) and `:3618` (ailments Bleed, Ignite, Poison). No golden reaches any of them with more than one addend.
//!
//! `actor.damageShiftTable` / `damageOverTimeShiftTable` are built here (:1846-1922) and kept for the call (the pools read
//! them through [`PoolActor::shift_table`]); nothing after estimations reads them in this slice (offence is P5).
//!
//! Numbers: `m_min/m_max` = LuaJIT `minsd/maxsd` folds (`pob_lua::number`), `round` = Common.lua, `m_floor/m_ceil/m_sqrt/
//! m_abs` = `f64` (exact), `^` = `f64::powf` (UCRT `pow` = desktop LuaJIT, R2). Nil arithmetic panics with the Lua line (L18).

use pob_build::ConfigValue;
use pob_lua::number::{max2, min, min2, round};
use pob_mods::{ModCfg, ModType, StoreRef, Value};

use crate::breakdown::{BTable, Breakdown, Chain, multi_chain, row, row_table, step, step_s};
use crate::calc_lib;
use crate::defence::{action_speed_mod, armour_reduction};
use crate::env::Env;
use crate::output::{OutValue, Output};
use crate::perform::{PerformState, Who};
use crate::pools::{
    ALLY_LIFE_POOL_LIST, Aegis, DamageTable, DmgType, Guard, PoolActor, PoolTable, build_ally_life_pools,
    calc_life_hit_pool_with_loss_prevention, reduce_pools_by_damage, taken_hit_from_damage,
};
use crate::setup::{misc, ok};
use crate::sf;
use pob_lua::tostring_num;

use ModType::{Base, Inc, More};

/// `:28` `dmgTypeList`.
const DMG: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];
/// `:27` `hitSourceList`.
const HIT_SOURCES: [&str; 2] = ["Attack", "Spell"];
/// `:2989` `recoupTypeList`.
const RECOUP_TYPES: [&str; 3] = ["Life", "Mana", "EnergyShield"];

/// `colorCodes[name]` (Data/Global.lua).
fn color_code(name: &str) -> &'static str {
    pob_data::globals::globals()
        .color_codes
        .get(name)
        .map(String::as_str)
        .unwrap_or_else(|| panic!("colorCodes.{name} is nil"))
}

const PHYSICAL: usize = 0;
const FIRE: usize = 3;
const CHAOS: usize = 4;

/// `:24` `isElemental`.
fn is_elemental(t: usize) -> bool {
    matches!(t, 1..=3)
}

fn site(line: u32) -> String {
    format!("Modules/CalcDefence.lua:{line}")
}

// --- readers --------------------------------------------------------------------------------------------------------

fn sum(db: &StoreRef, ty: ModType, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
    ok(db.borrow().sum(ty, cfg, names))
}

fn base(db: &StoreRef, names: &[&str]) -> f64 {
    sum(db, Base, None, names)
}

fn inc(db: &StoreRef, names: &[&str]) -> f64 {
    sum(db, Inc, None, names)
}

fn more(db: &StoreRef, names: &[&str]) -> f64 {
    ok(db.borrow().more(None, names))
}

fn flag(db: &StoreRef, names: &[&str]) -> bool {
    ok(db.borrow().flag(None, names)) == Some(true)
}

fn override_(db: &StoreRef, name: &str) -> Option<Value> {
    ok(db.borrow().override_(None, &[name]))
}

fn max_(db: &StoreRef, name: &str) -> Option<f64> {
    ok(db.borrow().max(None, &[name]))
}

/// `calcLib.mod(db, nil, ...)`.
fn cmod(db: &StoreRef, names: &[&str]) -> f64 {
    ok(calc_lib::mod_(&db.borrow(), None, names))
}

/// `calcLib.val(db, name)`.
fn cval(db: &StoreRef, name: &str) -> f64 {
    ok(calc_lib::val(&db.borrow(), name, None))
}

/// A mod value used as a number.
fn num_val(v: &Value, line: u32) -> f64 {
    v.as_num()
        .unwrap_or_else(|| panic!("{}: attempt to perform arithmetic on a {} value", site(line), v.type_name()))
}

/// `modDB:Override(nil, name) or fallback` used as a number (L26).
fn or_override(v: Option<Value>, fallback: f64, line: u32) -> f64 {
    match v {
        Some(v) if v.truthy() => num_val(&v, line),
        _ => fallback,
    }
}

/// `output[key]` in arithmetic: anything but a number is the Lua error.
fn on(out: &Output, key: &str) -> f64 {
    match out.get(key) {
        Some(OutValue::Num(v)) => *v,
        other => panic!("Modules/CalcDefence.lua: attempt to perform arithmetic on field '{key}' ({other:?})"),
    }
}

fn truthy_val(v: Option<&OutValue>) -> bool {
    !matches!(v, None | Some(OutValue::Bool(false)))
}

fn truthy(out: &Output, key: &str) -> bool {
    truthy_val(out.get(key))
}

/// `(output[key] or 0)` where upstream then does arithmetic on it.
fn or0(out: &Output, key: &str, line: u32) -> f64 {
    match out.get(key) {
        None | Some(OutValue::Bool(false)) => 0.0,
        Some(OutValue::Num(v)) => *v,
        Some(other) => panic!("{}: attempt to perform arithmetic on field '{key}' ({other:?})", site(line)),
    }
}

/// `flag and 0 or output[k1] or output[k2] ...` used as a number.
fn first_num(out: &Output, zero: bool, keys: &[&str], line: u32) -> f64 {
    if zero {
        return 0.0;
    }
    for k in keys {
        match out.get(k) {
            None | Some(OutValue::Bool(false)) => continue,
            Some(OutValue::Num(v)) => return *v,
            Some(other) => panic!("{}: attempt to perform arithmetic on field '{k}' ({other:?})", site(line)),
        }
    }
    panic!(
        "{}: attempt to perform arithmetic on a nil value ({})",
        site(line),
        keys.join(" or ")
    )
}

/// `output[dst] = <value>` (nil removes the key).
fn put(out: &mut Output, key: &str, v: Option<OutValue>) {
    match v {
        None => out.remove(key),
        Some(OutValue::Num(x)) => out.set_num(key, x),
        Some(OutValue::Bool(b)) => out.set_bool(key, b),
        Some(OutValue::Str(s)) => out.set_str(key, &s),
    }
}

/// `output[dst] = output[src]`.
fn copy(out: &mut Output, dst: &str, src: &str) {
    let v = out.get(src).cloned();
    put(out, dst, v);
}

// --- config readers -------------------------------------------------------------------------------------------------

fn cfg_truthy(v: Option<&ConfigValue>) -> bool {
    !matches!(v, None | Some(ConfigValue::Bool(false)))
}

/// Lua `tonumber(v)`: LuaJIT answers nil for a boolean.
fn tonumber(v: Option<&ConfigValue>) -> Option<f64> {
    match v {
        Some(ConfigValue::Num(x)) => Some(*x),
        Some(ConfigValue::Str(s)) => pob_lua::tonumber(s),
        _ => None,
    }
}

/// A config value in arithmetic: numbers, numeric strings (VM coercion); anything else is the error.
fn arith(v: &ConfigValue, line: u32) -> f64 {
    match v {
        ConfigValue::Num(x) => *x,
        ConfigValue::Str(s) => {
            pob_lua::tonumber(s).unwrap_or_else(|| panic!("{}: attempt to perform arithmetic on a string value", site(line)))
        }
        ConfigValue::Bool(_) => panic!("{}: attempt to perform arithmetic on a boolean value", site(line)),
    }
}

/// `a or b or fallback` over config values, used in arithmetic.
fn cfg_or(vals: &[Option<&ConfigValue>], fallback: f64, line: u32) -> f64 {
    for v in vals {
        if cfg_truthy(*v) {
            return arith(v.expect("truthy"), line);
        }
    }
    fallback
}

/// A config value compared with `> 1` / `== 4`: only a number can be compared with a number.
fn cfg_num_cmp(v: &ConfigValue, line: u32) -> f64 {
    match v {
        ConfigValue::Num(x) => *x,
        other => panic!("{}: attempt to compare number with {other:?}", site(line)),
    }
}

/// `damageCategoryConfig == name`.
fn is(dcc: &ConfigValue, name: &str) -> bool {
    matches!(dcc, ConfigValue::Str(s) if s == name)
}

/// `damageCategoryConfig .. "Suffix"` -- a number concatenates through `tostring`, a boolean errors.
fn concat_key(dcc: &ConfigValue, line: u32) -> String {
    match dcc {
        ConfigValue::Str(s) => s.clone(),
        ConfigValue::Num(x) => pob_lua::tostring_num(*x),
        ConfigValue::Bool(_) => panic!(
            "{}: attempt to concatenate a boolean value (local 'damageCategoryConfig')",
            site(line)
        ),
    }
}

/// `(dcc == "Average" and "n " or (dcc == "Untyped" and "n " or " ")) .. dcc` (:2898, :2942).
fn a_hit(dcc: &ConfigValue, line: u32) -> String {
    let art = if is(dcc, "Average") || is(dcc, "Untyped") { "n " } else { " " };
    format!("{art}{}", concat_key(dcc, line))
}

// --- the actor as the pool helpers see it ---------------------------------------------------------------------------

/// `actor.damageShiftTable` / `actor.damageOverTimeShiftTable`: `[from][to]`, every entry set by :1846-1868.
type ShiftTable = [[Option<f64>; 5]; 5];

/// The real actor (`actor.output`, `actor.modDB`, `actor.damageShiftTable`) behind [`PoolActor`].
pub struct EstimationActor<'a> {
    pub output: &'a Output,
    pub mod_db: &'a StoreRef,
    shift: &'a ShiftTable,
}

impl PoolActor for EstimationActor<'_> {
    fn output(&self, key: &str) -> Option<f64> {
        self.output.opt_num(key)
    }

    fn flag(&self, name: &str) -> bool {
        flag(self.mod_db, &[name])
    }

    fn sum_base(&self, name: &str) -> f64 {
        base(self.mod_db, &[name])
    }

    fn sum_more(&self, name: &str) -> f64 {
        sum(self.mod_db, More, None, &[name])
    }

    fn shift_table(&self, damage_type: DmgType) -> Option<[Option<f64>; 5]> {
        Some(self.shift[damage_type as usize])
    }
}

fn view<'a>(out: &'a Output, db: &'a StoreRef, shift: &'a ShiftTable) -> EstimationActor<'a> {
    EstimationActor {
        output: out,
        mod_db: db,
        shift,
    }
}

fn dmg_type(t: usize) -> DmgType {
    DmgType::ALL[t]
}

// --- CalcDefence.lua:2612-2777 `numberOfHitsToDie` ------------------------------------------------------------------

/// `DamageIn`: the hit per damage type plus the bookkeeping upstream keeps in the same table (the function writes
/// `cycles`, `iterations`, `Track*` and `cyclesRan` into it; a sped-up pass hands `iterations` back up, :2747).
#[derive(Clone, Default)]
struct DamageIn {
    dmg: [f64; 5],
    cycles: Option<f64>,
    iterations: Option<f64>,
    track_recoupable: bool,
    track_life_loss_over_time: bool,
    limit_ehp_speedup: bool,
    cycles_ran: bool,
    gain_when_hit: bool,
    life_when_hit: Option<f64>,
    mana_when_hit: Option<f64>,
    energy_shield_when_hit: Option<f64>,
    missing_life_before_enemy_hit: Option<f64>,
    missing_mana_before_enemy_hit: Option<f64>,
}

fn need(v: Option<f64>, line: u32, what: &str) -> f64 {
    v.unwrap_or_else(|| panic!("{}: attempt to perform arithmetic on a nil value ({what})", site(line)))
}

/// Drains the pools with the same hit until life hits 0 and answers how many hits that took (the killing hit's overkill
/// taken off); recurses on a hit `speedUp` times bigger to skip hits (:2723-2749). Infinity = never dies.
fn number_of_hits_to_die(d: &mut DamageIn, out: &mut Output, db: &StoreRef, shift: &ShiftTable) -> f64 {
    let mut num_hits = 0.0;
    // :2615-2616
    let cycles = *d.cycles.get_or_insert(1.0);
    d.iterations.get_or_insert(0.0);

    // :2618-2624
    for t in 0..5 {
        num_hits += d.dmg[t];
    }
    if num_hits == 0.0 {
        return f64::INFINITY;
    }
    // :2626-2631
    let ward_avoid_break_chance = if flag(db, &["Condition:WardNotBreak"]) {
        1.0
    } else {
        min2(base(db, &["WardAvoidBreakChance"]) / 100.0, 1.0)
    };
    if ward_avoid_break_chance == 1.0 && on(out, "Ward") > 0.0 && num_hits < on(out, "Ward") {
        return f64::INFINITY;
    } else {
        num_hits = 0.0;
    }

    // :2633-2637
    let mut ward = or0(out, "Ward", 2633);
    if ward_avoid_break_chance < 1.0 && cycles > 1.0 {
        ward = 0.0;
    }
    // :2638-2647
    let mut aegis = Aegis {
        shared: Some(or0(out, "sharedAegis", 2639)),
        shared_elemental: Some(or0(out, "sharedElementalAegis", 2640)),
        typed: [None; 5],
    };
    let mut guard = Guard {
        shared: Some(or0(out, "sharedGuardAbsorb", 2642)),
        typed: [None; 5],
    };
    for (t, name) in DMG.iter().enumerate() {
        aegis.typed[t] = Some(or0(out, &format!("{name}Aegis"), 2644));
        guard.typed[t] = Some(or0(out, &format!("{name}GuardAbsorb"), 2645));
    }
    let allies = build_ally_life_pools(&view(out, db, shift));

    // :2649-2660 -- `EnergyShield` has no `or 0`: a nil only fails later, inside reducePoolsByDamage (:241).
    let energy_shield = match out.get("EnergyShieldRecoveryCap") {
        None => None,
        Some(OutValue::Num(v)) => Some(*v),
        Some(other) => panic!("{}: EnergyShieldRecoveryCap is {other:?}", site(2654)),
    };
    let mut pool = PoolTable {
        allies_taken_before_you: Some(allies),
        aegis: Some(aegis),
        guard: Some(guard),
        ward: Some(ward),
        ward_active_chance: None,
        energy_shield,
        mana: Some(or0(out, "ManaUnreserved", 2655)),
        life: Some(or0(out, "LifeRecoverable", 2656)),
        life_loss_lost_over_time: Some(or0(out, "LifeLossLostOverTime", 2657)),
        life_below_half_loss_lost_over_time: Some(or0(out, "LifeBelowHalfLossLostOverTime", 2658)),
        damage_taken_that_can_be_recouped: Some([None; 5]),
    };
    // `poolTable.OverkillDamage` -- only a returned table has it.
    let mut overkill: Option<f64> = None;
    let life = |p: &PoolTable| p.life.expect("poolTable.Life is a number");

    // :2662-2668 (`x or false` keeps a true, so on cycles 1 the flags stay as they are)
    if cycles != 1.0 {
        d.track_recoupable = false;
        d.track_life_loss_over_time = false;
    }
    // :2669 `DamageIn.WardBypass = ...` -- written, read by nothing (reducePoolsByDamage sums WardBypass itself): not kept.

    // :2671-2674
    let mut vaal_arctic_armour_hits_left = on(out, "VaalArcticArmourLife");
    if cycles > 1.0 {
        vaal_arctic_armour_hits_left = 0.0;
    }

    // :2676-2751
    let mut iteration_multiplier = 1.0;
    let mut damage_total = 0.0;
    let max_damage = misc("ehpCalcMaxDamage");
    let max_iterations = misc("ehpCalcMaxIterationsToCalc");
    while life(&pool) > 0.0 && d.iterations.expect("set") < max_iterations {
        d.iterations = Some(d.iterations.expect("set") + 1.0);
        let mut damage: DamageTable = [None; 5];
        damage_total = 0.0;
        // :2684 `left > 0 and (<number>) or 1`
        let vaal_arctic_armour_multiplier = if vaal_arctic_armour_hits_left > 0.0 {
            1.0 - on(out, "VaalArcticArmourMitigation") * min2(vaal_arctic_armour_hits_left / iteration_multiplier, 1.0)
        } else {
            1.0
        };
        vaal_arctic_armour_hits_left -= iteration_multiplier;
        for (slot, &dm) in damage.iter_mut().zip(d.dmg.iter()) {
            // :2688 `damage > 0 and <number> or nil` -- nil leaves the type out.
            if dm > 0.0 {
                *slot = Some(dm * iteration_multiplier * vaal_arctic_armour_multiplier);
            }
            damage_total += dm;
        }
        // :2691-2704 gains over the skipped hits (MissingLife/ManaBeforeEnemyHit are Sums: a 0 is truthy, L4)
        if (d.gain_when_hit || d.missing_life_before_enemy_hit.is_some() || d.missing_mana_before_enemy_hit.is_some())
            && (iteration_multiplier > 1.0 || cycles > 1.0)
        {
            let gain_mult = iteration_multiplier * cycles;
            if let Some(m) = d.missing_life_before_enemy_hit {
                // :2694 `gainMult * output.LifeRecoverable or 0` -- the `or 0` binds to the product: dead.
                let l = life(&pool);
                pool.life = Some(min2(
                    l + m * (or0(out, "LifeUnreserved", 2694) - l) * (gain_mult - 1.0) / 100.0,
                    gain_mult * on(out, "LifeRecoverable"),
                ));
            }
            if let Some(m) = d.missing_mana_before_enemy_hit {
                let mana = need(pool.mana, 2697, "poolTable.Mana");
                pool.mana = Some(min2(
                    mana + m * (or0(out, "ManaUnreserved", 2697) - mana) * (gain_mult - 1.0) / 100.0,
                    gain_mult * or0(out, "ManaUnreserved", 2697),
                ));
            }
            if d.gain_when_hit {
                let l = life(&pool);
                pool.life = Some(min2(
                    l + need(d.life_when_hit, 2700, "LifeWhenHit") * (gain_mult - 1.0),
                    gain_mult * or0(out, "LifeRecoverable", 2700),
                ));
                let mana = need(pool.mana, 2701, "poolTable.Mana");
                pool.mana = Some(min2(
                    mana + need(d.mana_when_hit, 2701, "ManaWhenHit") * (gain_mult - 1.0),
                    gain_mult * or0(out, "ManaUnreserved", 2701),
                ));
                let es = need(pool.energy_shield, 2702, "poolTable.EnergyShield");
                pool.energy_shield = Some(min2(
                    es + need(d.energy_shield_when_hit, 2702, "EnergyShieldWhenHit") * (gain_mult - 1.0),
                    gain_mult * on(out, "EnergyShieldRecoveryCap"),
                ));
            }
        }
        // :2705-2710
        if let Some(m) = d.missing_life_before_enemy_hit
            && life(&pool) > 0.0
        {
            let l = life(&pool);
            pool.life = Some(min2(
                l + m * (or0(out, "LifeUnreserved", 2706) - l) / 100.0,
                or0(out, "LifeRecoverable", 2706),
            ));
        }
        if let Some(m) = d.missing_mana_before_enemy_hit {
            let mana = need(pool.mana, 2708, "poolTable.Mana");
            if mana > 0.0 {
                pool.mana = Some(min2(
                    mana + m * (or0(out, "ManaUnreserved", 2709) - mana) / 100.0,
                    or0(out, "ManaUnreserved", 2709),
                ));
            }
        }
        // :2711 a NEW pool table comes back; `Damage` itself is rounded up in place.
        let r = reduce_pools_by_damage(Some(pool), &mut damage, &view(out, db, shift)).unwrap_or_else(|e| panic!("{e}"));
        overkill = Some(r.overkill_damage);
        pool = r.into();

        // :2713-2716
        if life(&pool) > 0.0 && damage_total >= max_damage {
            return f64::INFINITY;
        }
        // :2717-2721
        if d.gain_when_hit && life(&pool) > 0.0 {
            let l = life(&pool);
            pool.life = Some(min2(
                l + need(d.life_when_hit, 2718, "LifeWhenHit"),
                or0(out, "LifeRecoverable", 2718),
            ));
            let mana = need(pool.mana, 2719, "poolTable.Mana");
            pool.mana = Some(min2(
                mana + need(d.mana_when_hit, 2719, "ManaWhenHit"),
                or0(out, "ManaUnreserved", 2719),
            ));
            let es = need(pool.energy_shield, 2720, "poolTable.EnergyShield");
            pool.energy_shield = Some(min2(
                es + need(d.energy_shield_when_hit, 2720, "EnergyShieldWhenHit"),
                on(out, "EnergyShieldRecoveryCap"),
            ));
        }
        iteration_multiplier = 1.0;
        // :2722-2749
        let speed_up = if d.limit_ehp_speedup { 4.0 } else { misc("ehpCalcSpeedUp") };
        let ward_avoid_break_active = ward_avoid_break_chance < 1.0 && pool.ward_active_chance.unwrap_or(0.0) > 0.01;
        if !d.cycles_ran && !ward_avoid_break_active && life(&pool) > 0.0 && d.iterations.expect("set") < max_iterations {
            // :2730 a NEW table for the recursive call's input
            let mut sped = DamageIn::default();
            for t in 0..5 {
                sped.dmg[t] = d.dmg[t] * speed_up;
            }
            sped.limit_ehp_speedup = d.limit_ehp_speedup;
            if d.gain_when_hit {
                sped.gain_when_hit = true;
                sped.life_when_hit = d.life_when_hit;
                sped.mana_when_hit = d.mana_when_hit;
                sped.energy_shield_when_hit = d.energy_shield_when_hit;
            }
            sped.cycles = Some(cycles * speed_up);
            sped.iterations = d.iterations;
            iteration_multiplier = max2((number_of_hits_to_die(&mut sped, out, db, shift) - 1.0) * speed_up - 1.0, 1.0);
            if iteration_multiplier == f64::INFINITY {
                return f64::INFINITY;
            }
            d.iterations = sped.iterations;
            d.cycles_ran = true;
        }
        num_hits += iteration_multiplier;
    }
    // :2752-2756 (independent keys: the `pairs` order shows nowhere)
    if d.track_recoupable {
        let recouped = pool.damage_taken_that_can_be_recouped.unwrap_or([None; 5]);
        for (t, name) in DMG.iter().enumerate() {
            if let Some(v) = recouped[t] {
                let key = format!("{name}RecoupableDamageTaken");
                let cur = on(out, &key);
                out.set_num(&key, cur + v);
            }
        }
    }
    // :2757-2760
    if d.track_life_loss_over_time {
        let v = on(out, "LifeLossLostOverTime") + need(pool.life_loss_lost_over_time, 2758, "LifeLossLostOverTime");
        out.set_num("LifeLossLostOverTime", v);
        let v = on(out, "LifeBelowHalfLossLostOverTime")
            + need(pool.life_below_half_loss_lost_over_time, 2759, "LifeBelowHalfLossLostOverTime");
        out.set_num("LifeBelowHalfLossLostOverTime", v);
    }
    // :2762-2764
    if life(&pool) == 0.0 && cycles == 1.0 {
        num_hits -= need(overkill, 2763, "OverkillDamage") / damage_total;
    }
    // :2765-2772
    damage_total = 0.0;
    for t in 0..5 {
        damage_total += d.dmg[t] * num_hits;
    }
    if life(&pool) >= 0.0 && damage_total >= max_damage {
        return f64::INFINITY;
    }
    // :2773-2776
    if num_hits.is_nan() {
        return 0.0;
    }
    max2(num_hits, 0.0)
}

// --- CalcDefence.lua:3473-3489 / :3715-3731 the degen split -----------------------------------------------------------

/// One type's degen split into (life, mana, energy shield) through Mind over Matter and the ES bypass.
fn split_degen(out: &Output, db: &StoreRef, degen: f64, t: usize) -> (f64, f64, f64) {
    let name = DMG[t];
    let bypass = on(out, &format!("{name}EnergyShieldBypass"));
    let taken_from_mana = on(out, &format!("{name}MindOverMatter")) + on(out, "sharedMindOverMatter");
    let (life_degen, mana_degen, es_degen);
    if on(out, "EnergyShieldRegenRecovery") > 0.0 {
        if flag(db, &["EnergyShieldProtectsMana"]) {
            life_degen = degen * (1.0 - taken_from_mana / 100.0);
            es_degen = degen * (1.0 - bypass / 100.0) * (taken_from_mana / 100.0);
        } else {
            life_degen = degen * (bypass / 100.0) * (1.0 - taken_from_mana / 100.0);
            es_degen = degen * (1.0 - bypass / 100.0);
        }
        mana_degen = degen * (bypass / 100.0) * (taken_from_mana / 100.0);
    } else {
        life_degen = degen * (1.0 - taken_from_mana / 100.0);
        mana_degen = degen * (taken_from_mana / 100.0);
        es_degen = 0.0;
    }
    (life_degen, mana_degen, es_degen)
}

/// `:2351-2363` local `calcMoMEBPool` -- `(pool, maxManaUsable, manaUsed, maxESUsable)`; the last three feed the breakdown.
fn calc_mom_eb_pool(out: &Output, db: &StoreRef, life_pool: f64, mom_effect: f64, es_bypass: f64) -> (f64, f64, f64, f64) {
    let mana = max2(or0(out, "ManaUnreserved", 2352), 0.0);
    // :2353 `MoMEffect < 1 and <number> or m_huge`
    let max_mom_pool = if mom_effect < 1.0 {
        life_pool / (1.0 - mom_effect) - life_pool
    } else {
        f64::INFINITY
    };
    let max_mana_usable = min2(mana, max_mom_pool).floor();
    // :2355-2360 three-argument `m_min`
    let max_es_usable = if flag(db, &["EnergyShieldProtectsMana"]) && es_bypass < 1.0 {
        min(
            on(out, "EnergyShieldRecoveryCap"),
            &[
                max_mom_pool * (1.0 - es_bypass),
                (life_pool + max_mana_usable) / (1.0 - (1.0 - es_bypass) * mom_effect) - (life_pool + max_mana_usable),
            ],
        )
        .floor()
    } else {
        0.0
    };
    let mana_used = min2(max_mom_pool - max_es_usable, max_mana_usable).floor();
    (life_pool + mana_used + max_es_usable, max_mana_usable, mana_used, max_es_usable)
}

/// `:2429-2438` / `:2447-2456` the guard breakdown (`lifeProtected` = limit / rate * (1 - rate)).
fn guard_breakdown(out: &Output, limit_key: &str, rate_key: &str) -> BTable {
    let (limit, rate) = (on(out, limit_key), on(out, rate_key));
    let life_protected = limit / (rate / 100.0) * (1.0 - rate / 100.0);
    BTable::lines([
        sf!("Total life protected:"),
        sf!("%d ^8(guard limit)", limit),
        sf!("/ %.2f ^8(portion taken from guard)", rate / 100.0),
        sf!("x %.2f ^8(portion taken from life and energy shield)", 1.0 - rate / 100.0),
        sf!("= %d", life_protected),
    ])
}

/// `:2372-2385` / `:2403-2416` the Mind over Matter breakdown of one pool.
fn mom_breakdown(out: &Output, db: &StoreRef, mom: f64, r: (f64, f64, f64, f64)) -> BTable {
    let (pool, max_mana, mana, es) = r;
    let life_protected = (mana + es) / mom - (mana + es);
    let shadow = if mana == max_mana {
        String::new()
    } else {
        sf!(" (%d mana shadowed by energy shield)", max_mana - mana)
    };
    let mut t = BTable::lines([
        sf!("Effective life from Mind over Matter:"),
        sf!("%d life (%d life is protected)", on(out, "LifeRecoverable"), life_protected),
        sf!("%d mana%s", mana, shadow),
    ]);
    let protects = flag(db, &["EnergyShieldProtectsMana"]);
    if protects {
        t.push(sf!("%d energy shield", es));
    }
    let es_part = if protects { sf!("+ %d ", es) } else { String::new() };
    t.push(sf!("%d + %d %s= %d", on(out, "LifeRecoverable"), mana, es_part, pool));
    t
}

// --- CalcDefence.lua:1674-3786 `calcs.buildDefenceEstimations` --------------------------------------------------------

/// The phase `calculate` plugs into CalcPerform.lua:3793-3795.
pub fn estimations(env: &mut Env, state: &mut PerformState, who: Who) {
    let (db, enemy_db) = (crate::perform::db_of(env, who), crate::perform::enemy_db_of(env, who));
    let env_mod_db = env.mod_db.clone();
    let is_player = who == Who::Player;
    // the minion's state is a shared cell; `actor.parent` is the player (the enemy for a hostile minion), R7.10
    let minion_rc = (who == Who::Minion).then(|| state.minion.clone().expect("env.minion.output"));
    let hostile = who == Who::Minion && crate::perform::env_minion(env).hostile;
    let mut minion_guard = minion_rc.as_ref().map(|rc| rc.borrow_mut());
    type Parts<'a> = (
        &'a mut Output,
        Option<&'a Output>,
        Option<&'a mut Breakdown>,
        Option<&'a crate::perform::ally::AllyLifeList>,
    );
    let (out, parent, mut bd, ally_life_list): Parts<'_> = match minion_guard.as_mut() {
        Some(g) => {
            let g = &mut **g;
            (
                &mut g.output,
                Some(if hostile { &state.enemy.output } else { &state.player.output }),
                g.breakdown.as_mut(),
                g.ally_life_list.as_ref(),
            )
        }
        None => match who {
            Who::Player => (
                &mut state.player.output,
                None,
                state.player.breakdown.as_mut(),
                state.player.ally_life_list.as_ref(),
            ),
            _ => (
                &mut state.enemy.output,
                None,
                state.enemy.breakdown.as_mut(),
                state.enemy.ally_life_list.as_ref(),
            ),
        },
    };
    let fl = |names: &[&str]| flag(&db, names);
    let input = &env.config.input;
    let placeholder = &env.config.placeholder;
    // :1680 `breakdown` (R8.4: `bd`), :1682 `condList` -- never read.

    // :1684
    let dcc: ConfigValue = match input.get("enemyDamageType") {
        Some(v) if cfg_truthy(Some(v)) => v.clone(),
        _ => ConfigValue::Str("Average".to_owned()),
    };
    let dot = is(&dcc, "DamageOverTime");
    let worst_of = |line: u32| -> f64 {
        match input.get("EHPUnluckyWorstOf") {
            Some(v) if cfg_truthy(Some(v)) => cfg_num_cmp(v, line),
            _ => 1.0,
        }
    };

    // --- :1686-1707 chance to not be hit -------------------------------------------------------------------------
    if !dot {
        let wo = worst_of(1699);
        // :1690/:1692 `specificTypeAvoidance and 0 or AvoidProjectilesChance`
        let projectile_avoid = |out: &Output| first_num(out, truthy(out, "specificTypeAvoidance"), &["AvoidProjectilesChance"], 1690);
        let v = 100.0
            - (1.0 - on(out, "MeleeEvadeChance") / 100.0)
                * (1.0 - on(out, "EffectiveAttackDodgeChance") / 100.0)
                * (1.0 - on(out, "AvoidAllDamageFromHitsChance") / 100.0)
                * 100.0;
        out.set_num("MeleeNotHitChance", v);
        let v = 100.0
            - (1.0 - on(out, "ProjectileEvadeChance") / 100.0)
                * (1.0 - on(out, "EffectiveAttackDodgeChance") / 100.0)
                * (1.0 - on(out, "AvoidAllDamageFromHitsChance") / 100.0)
                * (1.0 - projectile_avoid(out) / 100.0)
                * 100.0;
        out.set_num("ProjectileNotHitChance", v);
        let v =
            100.0 - (1.0 - on(out, "EffectiveSpellDodgeChance") / 100.0) * (1.0 - on(out, "AvoidAllDamageFromHitsChance") / 100.0) * 100.0;
        out.set_num("SpellNotHitChance", v);
        let v = 100.0
            - (1.0 - on(out, "EffectiveSpellDodgeChance") / 100.0)
                * (1.0 - on(out, "AvoidAllDamageFromHitsChance") / 100.0)
                * (1.0 - projectile_avoid(out) / 100.0)
                * 100.0;
        out.set_num("SpellProjectileNotHitChance", v);
        let v = 100.0 - (1.0 - on(out, "AvoidAllDamageFromHitsChance") / 100.0) * 100.0;
        out.set_num("UntypedNotHitChance", v);
        let v = (on(out, "MeleeNotHitChance")
            + on(out, "ProjectileNotHitChance")
            + on(out, "SpellNotHitChance")
            + on(out, "SpellProjectileNotHitChance"))
            / 4.0;
        out.set_num("AverageNotHitChance", v);
        // :1695 two chances over FOUR -- upstream's.
        let v = (on(out, "MeleeEvadeChance") + on(out, "ProjectileEvadeChance")) / 4.0;
        out.set_num("AverageEvadeChance", v);
        // :1696-1697
        let k = concat_key(&dcc, 1696);
        copy(out, "ConfiguredNotHitChance", &format!("{k}NotHitChance"));
        let ev = out.get(&format!("{k}EvadeChance")).cloned();
        if truthy_val(ev.as_ref()) {
            put(out, "ConfiguredEvadeChance", ev);
        } else {
            out.set_num("ConfiguredEvadeChance", 0.0);
        }
        // :1699-1706
        if wo > 1.0 {
            let c = first_num(out, false, &["ConfiguredNotHitChance"], 1700);
            out.set_num("ConfiguredNotHitChance", c / 100.0 * c);
            let c = on(out, "ConfiguredEvadeChance");
            out.set_num("ConfiguredEvadeChance", c / 100.0 * c);
            if wo == 4.0 {
                let c = on(out, "ConfiguredNotHitChance");
                out.set_num("ConfiguredNotHitChance", c / 100.0 * c);
                let c = on(out, "ConfiguredEvadeChance");
                out.set_num("ConfiguredEvadeChance", c / 100.0 * c);
            }
        }
    }

    // --- :1709-1844 enemy damage input and modifications ---------------------------------------------------------
    out.set_num("totalEnemyDamage", 0.0);
    out.set_num("totalEnemyDamageIn", 0.0);
    let elem_names = |t: usize, first: &str| -> Vec<String> {
        let mut v = vec!["Damage".to_owned(), format!("{first}Damage")];
        if is_elemental(t) {
            v.push("ElementalDamage".to_owned());
        }
        v
    };
    if dot {
        for (t, name) in DMG.iter().enumerate() {
            out.set_num(&format!("{name}EnemyPen"), 0.0);
            let names = elem_names(t, name);
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            out.set_num(&format!("{name}EnemyDamageMult"), cmod(&enemy_db, &names));
            out.set_num(&format!("{name}EnemyOverwhelm"), 0.0);
            out.set_num(&format!("{name}EnemyDamage"), 0.0);
        }
    } else {
        // :1720-1734
        if let Some(bd) = bd.as_deref_mut() {
            bd.put(
                "totalEnemyDamage",
                row_table(
                    Some("Total damage from the enemy"),
                    &[
                        ("Type", "type"),
                        ("Value", "value"),
                        ("Mult", "mult"),
                        ("Crit", "crit"),
                        ("Conversion", "convMult"),
                        ("Final", "final"),
                        ("From", "from"),
                    ],
                ),
            );
        }
        // :1735 -- `Override(...)` is not the last vararg here: a miss is an ordinary nil in the `or` chain (L26).
        let enemy_crit_chance = if flag(&enemy_db, &["NeverCrit"]) {
            0.0
        } else if flag(&enemy_db, &["AlwaysCrit"]) {
            100.0
        } else {
            let b = match override_(&db, "enemyCritChance") {
                Some(v) if v.truthy() => num_val(&v, 1735),
                _ => cfg_or(&[input.get("enemyCritChance"), placeholder.get("enemyCritChance")], 0.0, 1735),
            };
            max2(
                min2(
                    b * (1.0 + inc(&db, &["EnemyCritChance"]) / 100.0 + inc(&enemy_db, &["CritChance"]) / 100.0)
                        * (1.0 - on(out, "ConfiguredEvadeChance") / 100.0),
                    100.0,
                ),
                0.0,
            )
        };
        out.set_num("EnemyCritChance", enemy_crit_chance);
        let enemy_crit_damage = max2(
            cfg_or(&[input.get("enemyCritDamage"), placeholder.get("enemyCritDamage")], 0.0, 1737) + base(&enemy_db, &["CritMultiplier"]),
            0.0,
        );
        let v = 1.0 + enemy_crit_chance / 100.0 * (enemy_crit_damage / 100.0) * (1.0 - on(out, "CritExtraDamageReduction") / 100.0);
        out.set_num("EnemyCritEffect", v);
        // :1739 `{keywordFlags = bit.bnot(KeywordFlag.MatchAll)}` -- LuaJIT's signed int32.
        let enemy_cfg = ModCfg {
            keyword_flags: Some(f64::from(!(pob_mods::data::rules().match_all as i32))),
            ..ModCfg::default()
        };
        // `enemyDamageConversion[to][from]`: only Physical is ever a source (:1763), so `[to]` holds the Physical entry.
        let mut enemy_damage_conversion: [Option<f64>; 5] = [None; 5];
        for (t, name) in DMG.iter().enumerate() {
            let names = elem_names(t, name);
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            let enemy_damage_mult = cmod(&enemy_db, &names);
            let mut enemy_damage = tonumber(input.get(&format!("enemy{name}Damage")));
            let mut enemy_pen = tonumber(input.get(&format!("enemy{name}Pen")));
            let mut enemy_overwhelm = tonumber(input.get(&format!("enemy{name}Overwhelm")));
            // :1748
            let source_str = if enemy_damage.is_none() { "Default" } else { "Config" };
            if enemy_damage.is_none() {
                enemy_damage = Some(tonumber(placeholder.get(&format!("enemy{name}Damage"))).unwrap_or(0.0));
            }
            if enemy_pen.is_none() {
                enemy_pen = Some(tonumber(placeholder.get(&format!("enemy{name}Pen"))).unwrap_or(0.0));
            }
            if enemy_overwhelm.is_none() {
                // :1755 "enemy"..T.."enemyOverwhelm" -- upstream's typo; no such placeholder exists.
                enemy_overwhelm = Some(tonumber(placeholder.get(&format!("enemy{name}enemyOverwhelm"))).unwrap_or(0.0));
            }
            let (mut enemy_damage, enemy_pen, mut enemy_overwhelm) =
                (enemy_damage.expect("set"), enemy_pen.expect("set"), enemy_overwhelm.expect("set"));
            // :1759
            enemy_damage += (sum(&enemy_db, Base, Some(&enemy_cfg), &[&format!("{name}Min")])
                + sum(&enemy_db, Base, Some(&enemy_cfg), &[&format!("{name}Max")]))
                / 2.0;

            // :1761-1805 conversion and gain-as (Physical only)
            let mut conversion_total = 0.0;
            if t == PHYSICAL {
                let mut total = 0.0;
                let mut total_skill = 0.0;
                let mut conv = [0.0f64; 5];
                let mut conv_skill = [0.0f64; 5];
                for (to, to_name) in DMG.iter().enumerate() {
                    conv_skill[to] = sum(
                        &enemy_db,
                        Base,
                        Some(&enemy_cfg),
                        &[&format!("{name}DamageSkillConvertTo{to_name}")],
                    );
                    conv[to] = sum(&enemy_db, Base, Some(&enemy_cfg), &[&format!("{name}DamageConvertTo{to_name}")]);
                    total_skill += conv_skill[to];
                    total += conv[to];
                }
                // :1771-1786
                if total_skill > 100.0 {
                    let mult = 100.0 / total_skill;
                    total_skill *= mult;
                    total = 0.0;
                    for to in 0..5 {
                        conv_skill[to] *= mult;
                        conv[to] = 0.0;
                    }
                } else if total + total_skill > 100.0 {
                    let mult = (100.0 - total_skill) / total;
                    total *= mult;
                    for c in conv.iter_mut() {
                        *c *= mult;
                    }
                }
                conversion_total = total + total_skill;
                // :1789-1804
                for (to, to_name) in DMG.iter().enumerate() {
                    let gain_as_percent = sum(&enemy_db, Base, Some(&enemy_cfg), &[&format!("{name}DamageGainAs{to_name}")]) / 100.0;
                    let mut conversion_percent = conv[to] / 100.0;
                    let mut skill_conversion_percent = conv_skill[to] / 100.0;
                    if skill_conversion_percent > 0.0 {
                        if to != CHAOS {
                            // :1795 `data.monsterPhysConversionMultiTable[env.enemyLevel]` (1-based, L8)
                            let phys_multi = pob_data::constants::monster_table_at(
                                &pob_data::constants::constants().monster_phys_conversion_multi_table,
                                env.enemy_level,
                            )
                            .unwrap_or_else(|| panic!("{}: attempt to perform arithmetic on a nil value", site(1795)));
                            skill_conversion_percent *= 1.0 + phys_multi / 100.0;
                        }
                        conversion_percent += skill_conversion_percent;
                    }
                    if gain_as_percent > 0.0 || conversion_percent > 0.0 {
                        enemy_damage_conversion[to] = Some(enemy_damage * gain_as_percent + enemy_damage * conversion_percent);
                    }
                }
            }

            // :1807 the enemy's PHYSICAL overwhelm is added to every damage type's overwhelm (upstream).
            enemy_overwhelm = enemy_overwhelm + base(&enemy_db, &["PhysicalOverwhelm"]) + base(&db, &["EnemyPhysicalOverwhelm"]);

            // :1809-1822
            out.set_num(&format!("{name}EnemyPen"), enemy_pen);
            out.set_num(&format!("{name}EnemyDamageMult"), enemy_damage_mult);
            out.set_num(&format!("{name}EnemyOverwhelm"), enemy_overwhelm);
            let v = on(out, "totalEnemyDamageIn") + enemy_damage;
            out.set_num("totalEnemyDamageIn", v);
            let ed_key = format!("{name}EnemyDamage");
            let v = enemy_damage * (1.0 - conversion_total / 100.0) * enemy_damage_mult * on(out, "EnemyCritEffect");
            out.set_num(&ed_key, v);
            // :1815
            let mut conversion_extra = -enemy_damage * enemy_damage_mult * on(out, "EnemyCritEffect") + on(out, &ed_key);
            if let Some(from_damage) = enemy_damage_conversion[t] {
                // :1816 the one source is Physical; :1817 both elemental names can be present (summed twice, upstream).
                let mut names = vec!["Damage".to_owned(), format!("{name}Damage"), "PhysicalDamage".to_owned()];
                if is_elemental(t) {
                    names.push("ElementalDamage".to_owned());
                }
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                let from_mult = cmod(&enemy_db, &names);
                let v = on(out, &ed_key) + from_damage * from_mult * on(out, "EnemyCritEffect");
                out.set_num(&ed_key, v);
                conversion_extra += from_damage * from_mult * on(out, "EnemyCritEffect");
            }
            let v = on(out, "totalEnemyDamage") + on(out, &ed_key);
            out.set_num("totalEnemyDamage", v);
            // :1823-1842
            if let Some(bd) = bd.as_deref_mut() {
                let mut l = BTable::lines([
                    sf!("from %s: %d", source_str, enemy_damage),
                    sf!("* %.2f (modifiers to enemy damage)", enemy_damage_mult),
                    sf!("* %.3f (enemy crit effect)", on(out, "EnemyCritEffect")),
                ]);
                let abs_extra = if conversion_extra >= 0.0 {
                    conversion_extra
                } else {
                    -conversion_extra
                };
                if conversion_extra != 0.0 {
                    let sign = if conversion_extra > 0.0 { "+" } else { "-" };
                    l.push(sf!("%s %d (enemy damage conversion)", sign, abs_extra));
                }
                l.push(sf!("= %d", on(out, &ed_key)));
                bd.put(ed_key.clone(), l);
                let sign = if conversion_extra > 0.0 {
                    "+"
                } else if conversion_extra < 0.0 {
                    "-"
                } else {
                    ""
                };
                let r = row(&[
                    ("type", sf!("%s", *name)),
                    ("value", sf!("%d", enemy_damage)),
                    ("mult", sf!("%.2f", enemy_damage_mult)),
                    ("crit", sf!("%.2f", on(out, "EnemyCritEffect"))),
                    ("convMult", sf!("%s%d", sign, abs_extra)),
                    ("final", sf!("%d", on(out, &ed_key))),
                    ("from", sf!("%s", source_str)),
                ]);
                bd.table_mut("totalEnemyDamage").field_mut("rowList").push(r);
            }
        }
    }

    // --- :1846-1922 damage taken as ------------------------------------------------------------------------------
    let mut shift: ShiftTable = [[None; 5]; 5];
    let mut dot_shift: ShiftTable = [[None; 5]; 5];
    for (t, name) in DMG.iter().enumerate() {
        let mut dest_total = 0.0;
        let mut dot_destination_total = 0.0;
        for (dest, dest_name) in DMG.iter().enumerate() {
            if dest != t {
                let a = format!("{name}DamageTakenAs{dest_name}");
                let b = format!("ElementalDamageTakenAs{dest_name}");
                let names: Vec<&str> = if is_elemental(t) { vec![&a, &b] } else { vec![&a] };
                let d = base(&db, &names);
                dot_shift[t][dest] = Some(d);
                dot_destination_total += d;
                let a = format!("{name}DamageFromHitsTakenAs{dest_name}");
                let b = format!("ElementalDamageFromHitsTakenAs{dest_name}");
                let names: Vec<&str> = if is_elemental(t) { vec![&a, &b] } else { vec![&a] };
                let h = d + base(&db, &names);
                shift[t][dest] = Some(h);
                dest_total += h;
            }
        }
        dot_shift[t][t] = Some(max2(100.0 - dot_destination_total, 0.0));
        shift[t][t] = Some(max2(100.0 - dest_total, 0.0));
        // :1870 `* shift / 100`, in THIS order.
        let v = on(out, &format!("{name}EnemyDamage")) * shift[t][t].expect("set") / 100.0;
        out.set_num(&format!("{name}TakenDamage"), v);
        // :1871-1884
        if let Some(bd) = bd.as_deref_mut() {
            let mut tb = row_table(Some("Taken"), &[("Type", "type"), ("Value", "value")]);
            tb.field_mut("rowList")
                .push(row(&[("type", sf!("%s", *name)), ("value", sf!("%d", v))]));
            bd.put(format!("{name}TakenDamage"), tb);
        }
    }
    // :1887-1900
    for (t, name) in DMG.iter().enumerate() {
        for (c, c_name) in DMG.iter().enumerate() {
            if t != c {
                let damage = on(out, &format!("{name}EnemyDamage")) * shift[t][c].expect("set") / 100.0;
                let key = format!("{c_name}TakenDamage");
                let v = on(out, &key) + damage;
                out.set_num(&key, v);
                // :1892-1897
                if let Some(bd) = bd.as_deref_mut()
                    && damage > 0.0
                {
                    let r = row(&[("type", sf!("%s", *name)), ("value", sf!("%d", damage))]);
                    bd.table_mut(&key).field_mut("rowList").push(r);
                }
            }
        }
    }
    // :1902-1921
    out.set_num("totalTakenDamage", 0.0);
    if let Some(bd) = bd.as_deref_mut() {
        bd.put(
            "totalTakenDamage",
            row_table(
                Some("Total damage taken from the enemy after taken as"),
                &[("Type", "type"), ("Value", "value")],
            ),
        );
    }
    for name in DMG {
        let v = on(out, "totalTakenDamage") + on(out, &format!("{name}TakenDamage"));
        out.set_num("totalTakenDamage", v);
        // :1915-1920
        if let Some(bd) = bd.as_deref_mut() {
            let r = row(&[
                ("type", sf!("%s", name)),
                ("value", sf!("%d", on(out, &format!("{name}TakenDamage")))),
            ]);
            bd.table_mut("totalTakenDamage").field_mut("rowList").push(r);
        }
    }

    // --- :1924-1984 damage taken multipliers ---------------------------------------------------------------------
    out.set_bool("AnyTakenReflect", false);
    for (t, name) in DMG.iter().enumerate() {
        let dt = format!("{name}DamageTaken");
        let mut base_taken_inc = inc(&db, &["DamageTaken", &dt]);
        let mut base_taken_more = more(&db, &["DamageTaken", &dt]);
        if is_elemental(t) {
            base_taken_inc += inc(&db, &["ElementalDamageTaken"]);
            base_taken_more *= more(&db, &["ElementalDamageTaken"]);
        }
        {
            // :1933-1961 hit
            let wh = format!("{name}DamageTakenWhenHit");
            let mut taken_inc = base_taken_inc + inc(&db, &["DamageTakenWhenHit", &wh]);
            let mut taken_more = base_taken_more * more(&db, &["DamageTakenWhenHit", &wh]);
            if is_elemental(t) {
                taken_inc += inc(&db, &["ElementalDamageTakenWhenHit"]);
                taken_more *= more(&db, &["ElementalDamageTakenWhenHit"]);
            }
            out.set_num(&format!("{name}TakenHitMult"), max2((1.0 + taken_inc / 100.0) * taken_more, 0.0));
            // :1942-1947 `output[hitType.."TakenHitMult"]` is overwritten on every type -- Chaos's value stays.
            for hit_type in HIT_SOURCES {
                let hdt = format!("{hit_type}DamageTaken");
                let base_taken_inc_type = taken_inc + inc(&db, &[&hdt]);
                let base_taken_more_type = taken_more * more(&db, &[&hdt]);
                let v = max2((1.0 + base_taken_inc_type / 100.0) * base_taken_more_type, 0.0);
                out.set_num(&format!("{hit_type}TakenHitMult"), v);
                out.set_num(&format!("{name}{hit_type}TakenHitMult"), v);
            }
            // :1948-1960 reflect
            let rdt = format!("{name}ReflectedDamageTaken");
            taken_inc += inc(&db, &["ReflectedDamageTaken", &rdt]);
            taken_more *= more(&db, &["ReflectedDamageTaken", &rdt]);
            if is_elemental(t) {
                taken_inc += inc(&db, &["ElementalReflectedDamageTaken"]);
                taken_more *= more(&db, &["ElementalReflectedDamageTaken"]);
            }
            out.set_num(&format!("{name}TakenReflect"), max2((1.0 + taken_inc / 100.0) * taken_more, 0.0));
            // :1957-1959 `AnyTakenReflect = false --true` -- false whichever branch runs.
            if out.get(&format!("{name}TakenReflect")) != out.get(&format!("{name}TakenHitMult")) {
                out.set_bool("AnyTakenReflect", false);
            }
        }
        {
            // :1962-1983 damage over time
            let ot = format!("{name}DamageTakenOverTime");
            let mut taken_inc = base_taken_inc + inc(&db, &["DamageTakenOverTime", &ot]);
            let mut taken_more = base_taken_more * more(&db, &["DamageTakenOverTime", &ot]);
            if is_elemental(t) {
                taken_inc += inc(&db, &["ElementalDamageTakenOverTime"]);
                taken_more *= more(&db, &["ElementalDamageTakenOverTime"]);
            }
            // :1969 `Flag and 0 or ResistOverTime or Resist`
            let resist = first_num(
                out,
                fl(&[&format!("SelfIgnore{name}Resistance")]),
                &[&format!("{name}ResistOverTime"), &format!("{name}Resist")],
                1971,
            );
            let reduction = first_num(
                out,
                fl(&[&format!("SelfIgnoreBase{name}DamageReduction")]),
                &[&format!("Base{name}DamageReduction")],
                1971,
            );
            let v = max2(
                (1.0 - resist / 100.0) * (1.0 - reduction / 100.0) * (1.0 + taken_inc / 100.0) * taken_more,
                0.0,
            );
            out.set_num(&format!("{name}TakenDotMult"), v);
            // :1972-1982
            if let Some(bd) = bd.as_deref_mut() {
                let mut tb = BTable::new();
                multi_chain(
                    &mut tb,
                    &Chain {
                        label: Some("DoT Multiplier:".into()),
                        steps: vec![
                            step("%.2f ^8(resistance)", 1.0 - resist / 100.0),
                            step_s("%.2f ^8(%s damage reduction)", 1.0 - reduction / 100.0, &name.to_lowercase()),
                            step("%.2f ^8(increased/reduced damage taken)", 1.0 + taken_inc / 100.0),
                            step("%.2f ^8(more/less damage taken)", taken_more),
                        ],
                        total: Some(sf!("= %.2f", v)),
                        ..Chain::default()
                    },
                );
                bd.put(format!("{name}TakenDotMult"), tb);
            }
        }
    }

    // --- :1986-2151 incoming hit damage multipliers --------------------------------------------------------------
    out.set_num("totalTakenHit", 0.0);
    // :1988-1999
    if let Some(bd) = bd.as_deref_mut() {
        bd.put(
            "totalTakenHit",
            row_table(
                Some("Total damage taken after mitigation"),
                &[("Type", "type"), ("Incoming", "incoming"), ("Mult", "mult"), ("Value", "value")],
            ),
        );
    }
    // :2000 `(Sum or 0)` -- dead `or 0`; `ImpaleAvoidChance` (a percentage) used as a fraction -- upstream.
    let is_attack_category = is(&dcc, "Melee") || is(&dcc, "Projectile") || is(&dcc, "Average");
    let impale_cfg = ModCfg {
        flags: Some(if is_attack_category {
            f64::from(crate::setup::mod_flag("Attack"))
        } else {
            0.0
        }),
        keyword_flags: Some(0.0),
        ..ModCfg::default()
    };
    let impale_avoid = match out.get("ImpaleAvoidChance") {
        None | Some(OutValue::Bool(false)) => 0.0,
        Some(OutValue::Num(v)) => *v,
        Some(other) => panic!("{}: ImpaleAvoidChance is {other:?}", site(2000)),
    };
    let enemy_impale_chance =
        sum(&enemy_db, Base, Some(&impale_cfg), &["ImpaleChance"]) * (if is(&dcc, "Average") { 0.5 } else { 1.0 }) * (1.0 - impale_avoid);
    for (t, name) in DMG.iter().enumerate() {
        // :2003 `<T>ResistWhenHit` is written nowhere.
        let resist = first_num(
            out,
            fl(&[&format!("SelfIgnore{name}Resistance")]),
            &[&format!("{name}ResistWhenHit"), &format!("{name}Resist")],
            2018,
        );
        let reduction = first_num(
            out,
            fl(&[&format!("SelfIgnoreBase{name}DamageReduction")]),
            &[&format!("Base{name}DamageReductionWhenHit"), &format!("Base{name}DamageReduction")],
            2036,
        );
        let enemy_pen = first_num(
            out,
            fl(&[&format!("SelfIgnore{name}Resistance"), &format!("EnemyCannotPen{name}Resistance")]),
            &[&format!("{name}EnemyPen")],
            2018,
        );
        let enemy_overwhelm = first_num(
            out,
            fl(&[&format!("SelfIgnore{name}DamageReduction")]),
            &[&format!("{name}EnemyOverwhelm")],
            2037,
        );
        let damage = on(out, &format!("{name}TakenDamage"));
        let mut impale_damage = if enemy_impale_chance > 0.0 {
            if t == PHYSICAL {
                damage * misc("ImpaleStoredDamageBase")
            } else {
                0.0
            }
        } else {
            0.0
        };
        let mut armour_reduct = 0.0;
        let mut impale_armour_reduct = 0.0;
        // :2011 `not Flag and Sum or 0` -- Sum is a number, a plain conditional.
        let percent_of_armour_applies = min2(
            if !fl(&[&format!("ArmourDoesNotApplyTo{name}DamageTaken")]) {
                base(&db, &[&format!("ArmourAppliesTo{name}DamageTaken")])
            } else {
                0.0
            },
            100.0,
        );
        let mut effective_applied_armour = (on(out, "Armour") * percent_of_armour_applies / 100.0) * (1.0 + on(out, "ArmourDefense"));
        let physical_reduction_based_on_ward = t == PHYSICAL && fl(&["PhysicalReductionBasedOnWard"]);
        if physical_reduction_based_on_ward {
            // :2015 the Override is divided straight away -- a miss is a nil-arithmetic error upstream.
            let o = override_(&db, "PhysicalReductionBasedOnWardPercent")
                .unwrap_or_else(|| panic!("{}: attempt to perform arithmetic on a nil value", site(2015)));
            let multiplier = num_val(&o, 2015) / 100.0;
            effective_applied_armour = on(out, "Ward") * multiplier;
        }
        let res_mult = 1.0 - (resist - enemy_pen) / 100.0;
        // :2020-2027
        let dt = format!("{name}DamageTaken");
        let dtwh = format!("{name}DamageTakenWhenHit");
        let mut taken_flat = base(&db, &["DamageTaken", &dt, "DamageTakenWhenHit", &dtwh]);
        if is(&dcc, "Melee") || is(&dcc, "Projectile") {
            let k = concat_key(&dcc, 2022);
            taken_flat += base(
                &db,
                &[
                    "DamageTakenFromAttacks",
                    &format!("{name}DamageTakenFromAttacks"),
                    &format!("{name}DamageTakenFrom{k}Attacks"),
                ],
            );
        } else if is(&dcc, "Spell") || is(&dcc, "SpellProjectile") {
            taken_flat += base(
                &db,
                &[
                    "DamageTakenFromSpells",
                    &format!("{name}DamageTakenFromSpells"),
                    &format!("{name}DamageTakenFromSpellProjectiles"),
                ],
            );
        } else if is(&dcc, "Average") {
            taken_flat = taken_flat
                + base(&db, &["DamageTakenFromAttacks", &format!("{name}DamageTakenFromAttacks")]) / 2.0
                + base(&db, &[&format!("{name}DamageTakenFromProjectileAttacks")]) / 4.0
                + base(&db, &["DamageTakenFromSpells", &format!("{name}DamageTakenFromSpells")]) / 2.0
                + base(
                    &db,
                    &["DamageTakenFromSpellProjectiles", &format!("{name}DamageTakenFromSpellProjectiles")],
                ) / 4.0;
        }
        // :2028 lower-case `t` -- upstream's key.
        out.set_num(&format!("{name}takenFlat"), taken_flat);
        if percent_of_armour_applies > 0.0 || physical_reduction_based_on_ward {
            armour_reduct = armour_reduction(effective_applied_armour, damage * res_mult);
            armour_reduct = min2(on(out, "DamageReductionMax"), armour_reduct);
            if impale_damage > 0.0 {
                impale_armour_reduct = min2(
                    on(out, "DamageReductionMax"),
                    armour_reduction(effective_applied_armour, impale_damage * res_mult),
                );
            }
        }
        let dr_max = on(out, "DamageReductionMax");
        let total_reduct = min2(dr_max, armour_reduct + reduction);
        let reduct_mult = 1.0 - max2(min2(dr_max, total_reduct - enemy_overwhelm), 0.0) / 100.0;
        out.set_num(&format!("{name}DamageReduction"), 100.0 - reduct_mult * 100.0);
        // :2043-2065
        if let Some(bd) = bd.as_deref_mut() {
            let mut tb = BTable::new();
            if armour_reduct != 0.0 {
                if res_mult != 1.0 {
                    tb.push(sf!(
                        "Enemy Hit Damage After Resistance: %d ^8(total incoming damage)",
                        damage * res_mult
                    ));
                } else {
                    tb.push(sf!("Enemy Hit Damage: %d ^8(total incoming damage)", damage));
                }
                if percent_of_armour_applies != 100.0 && !physical_reduction_based_on_ward {
                    tb.push(sf!("%d%% percent of armour applies", percent_of_armour_applies));
                }
                tb.push(sf!("Reduction from Armour: %d%%", armour_reduct));
            }
            if reduction != 0.0 {
                tb.push(sf!("Base %s Damage Reduction: %d%%", *name, reduction));
            }
            if enemy_overwhelm != 0.0 {
                tb.push(sf!("Enemy Overwhelm %s Damage: %d%%", *name, enemy_overwhelm));
            }
            let parts = [armour_reduct != 0.0, reduction != 0.0, enemy_overwhelm != 0.0]
                .iter()
                .filter(|x| **x)
                .count();
            if parts >= 2 {
                tb.push(sf!("Total %s Damage Reduction: %d%%", *name, 100.0 - reduct_mult * 100.0));
            }
            bd.put(format!("{name}DamageReduction"), tb);
        }
        if impale_damage > 0.0 {
            impale_damage = impale_damage
                * res_mult
                * (1.0 - max2(min2(dr_max, min2(dr_max, impale_armour_reduct + reduction) - enemy_overwhelm), 0.0) / 100.0);
            impale_damage = impale_damage * enemy_impale_chance / 100.0 * 5.0 * on(out, &format!("{name}TakenReflect"));
        }
        // :2066-2076
        let mut taken_mult = on(out, &format!("{name}TakenHitMult"));
        let mut spell_suppress_mult = 1.0;
        let full_suppress = out.opt_num("EffectiveSpellSuppressionChance") == Some(100.0);
        if is(&dcc, "Melee") || is(&dcc, "Projectile") {
            taken_mult = on(out, &format!("{name}AttackTakenHitMult"));
        } else if is(&dcc, "Spell") || is(&dcc, "SpellProjectile") {
            taken_mult = on(out, &format!("{name}SpellTakenHitMult"));
            spell_suppress_mult = if full_suppress {
                1.0 - on(out, "SpellSuppressionEffect") / 100.0
            } else {
                1.0
            };
        } else if is(&dcc, "Average") {
            taken_mult = (on(out, &format!("{name}SpellTakenHitMult")) + on(out, &format!("{name}AttackTakenHitMult"))) / 2.0;
            spell_suppress_mult = if full_suppress {
                1.0 - on(out, "SpellSuppressionEffect") / 100.0 / 2.0
            } else {
                1.0
            };
        }
        // :2077-2090
        out.set_num(&format!("{name}EffectiveAppliedArmour"), effective_applied_armour);
        out.set_num(&format!("{name}ResistTakenHitMulti"), res_mult);
        let after_reduction_multi = taken_mult * spell_suppress_mult;
        out.set_num(&format!("{name}AfterReductionTakenHitMulti"), after_reduction_multi);
        let base_mult = res_mult * reduct_mult;
        out.set_num(&format!("{name}BaseTakenHitMult"), base_mult * after_reduction_multi);
        let taken_mult_reflect = on(out, &format!("{name}TakenReflect"));
        let final_reflect = base_mult * taken_mult_reflect;
        let taken_hit = max2(damage * base_mult + taken_flat, 0.0) * taken_mult * spell_suppress_mult + impale_damage;
        out.set_num(&format!("{name}TakenHit"), taken_hit);
        // :2086 the hit multiplier is REWRITTEN as the effective one.
        let v = if damage > 0.0 {
            on(out, &format!("{name}TakenHit")) / damage
        } else {
            0.0
        };
        out.set_num(&format!("{name}TakenHitMult"), v);
        let v = on(out, "totalTakenHit") + on(out, &format!("{name}TakenHit"));
        out.set_num("totalTakenHit", v);
        if truthy(out, "AnyTakenReflect") {
            out.set_num(&format!("{name}TakenReflectMult"), final_reflect);
        }
        // :2091-2150
        if let Some(bd) = bd.as_deref_mut() {
            let mut tb = BTable::new();
            if resist != 0.0 {
                tb.push(sf!("Resistance: %.2f", 1.0 - resist / 100.0));
            }
            if enemy_pen != 0.0 {
                tb.push(sf!("+ Enemy Pen: %.2f", enemy_pen / 100.0));
            }
            if resist != 0.0 && enemy_pen != 0.0 {
                tb.push(sf!("= %.2f", res_mult));
            }
            if reduction != 0.0 {
                tb.push(sf!("Base %s Damage Taken: %.2f", *name, 1.0 - reduction / 100.0));
            }
            if armour_reduct != 0.0 {
                if res_mult != 1.0 {
                    tb.push(sf!(
                        "Enemy Hit Damage After Resistance: %d ^8(total incoming damage)",
                        damage * res_mult
                    ));
                } else {
                    tb.push(sf!("Enemy Hit Damage: %d ^8(total incoming damage)", damage));
                }
                if percent_of_armour_applies != 100.0 && !physical_reduction_based_on_ward {
                    tb.push(sf!("%d%% percent of armour applies", percent_of_armour_applies));
                }
                tb.push(sf!("Reduction from Armour: %.2f", 1.0 - armour_reduct / 100.0));
            }
            if enemy_overwhelm != 0.0 {
                tb.push(sf!("+ Enemy Overwhelm %s Damage: %.2f", *name, enemy_overwhelm / 100.0));
            }
            if reduction != 0.0 || armour_reduct != 0.0 || enemy_overwhelm != 0.0 {
                tb.push(sf!("= %.2f", reduct_mult));
            }
            if res_mult != 1.0 && reduct_mult != 1.0 {
                tb.push(sf!("%.2f x %.2f = %.3f", res_mult, reduct_mult, base_mult));
            }
            if taken_flat != 0.0 {
                tb.push(sf!("+ Flat: %d", taken_flat));
            }
            if taken_mult != 1.0 {
                tb.push(sf!("x Taken: %.3f", taken_mult));
            }
            if spell_suppress_mult != 1.0 {
                tb.push(sf!("x Spell Suppression: %.3f", spell_suppress_mult));
            }
            if impale_damage != 0.0 {
                tb.push(sf!("+ Impale: %.1f", impale_damage));
            }
            if taken_mult != 1.0 || taken_flat != 0.0 || spell_suppress_mult != 1.0 || impale_damage != 0.0 {
                tb.push(sf!("= %.3f", on(out, &format!("{name}TakenHitMult"))));
            }
            bd.put(format!("{name}TakenHitMult"), tb);
            if truthy(out, "AnyTakenReflect") {
                let mut r = BTable::lines([sf!("Resistance: %.3f", 1.0 - resist / 100.0)]);
                if enemy_pen > 0.0 {
                    r.push(sf!("Enemy Pen: %.2f", enemy_pen));
                }
                r.push(sf!("Taken: %.3f", taken_mult_reflect));
                r.push(sf!("= %.3f", final_reflect));
                bd.put(format!("{name}TakenReflectMult"), r);
            }
        }
    }

    // --- :2153-2256 stun -------------------------------------------------------------------------------------------
    {
        // `stunThresholdSource` (:2156) feeds the breakdown.
        let (mut stun_threshold_base, mut stun_threshold_source) = if fl(&["StunThresholdBasedOnEnergyShieldInsteadOfLife"]) {
            let m = base(&db, &["StunThresholdEnergyShieldPercent"]);
            (
                on(out, "EnergyShield") * m / 100.0,
                format!("{}% of Energy Shield", tostring_num(m)),
            )
        } else if fl(&["StunThresholdBasedOnManaInsteadOfLife"]) {
            let m = base(&db, &["StunThresholdManaPercent"]);
            (on(out, "Mana") * m / 100.0, format!("{}% of Mana", tostring_num(m)))
        } else if fl(&["ChaosInoculation"]) {
            (base(&db, &["Life"]), "Life before Chaos Inoculation".to_owned())
        } else {
            (on(out, "Life"), "Life".to_owned())
        };
        if fl(&["AddESToStunThreshold"]) {
            let es_mult = base(&db, &["ESToStunThresholdPercent"]);
            stun_threshold_base += on(out, "EnergyShield") * es_mult / 100.0;
            stun_threshold_source = format!("{stun_threshold_source} and {}% of Energy Shield", tostring_num(es_mult));
        }
        let stun_threshold_mod = 1.0 + inc(&db, &["StunThreshold"]) / 100.0;
        out.set_num("StunThreshold", stun_threshold_base * stun_threshold_mod);

        let mut not_avoid_chance = if fl(&["StunImmune"]) {
            0.0
        } else {
            100.0 - min2(base(&db, &["AvoidStun"]), 100.0)
        };
        // :2181 `env.modDB` -- the PLAYER's db.
        if on(out, "EnergyShield") > on(out, "totalTakenHit") && !flag(&env_mod_db, &["EnergyShieldProtectsMana"]) {
            not_avoid_chance *= 0.5;
        }
        out.set_num("StunAvoidChance", 100.0 - not_avoid_chance);
        // :2186-2196
        if let Some(bd) = bd.as_deref_mut() {
            let mut tb = BTable::lines([sf!("%d ^8(base from %s)", stun_threshold_base, stun_threshold_source)]);
            if stun_threshold_mod != 1.0 {
                tb.push(sf!("* %.2f ^8(increased threshold)", stun_threshold_mod));
                tb.push(sf!("= %d", on(out, "StunThreshold")));
            }
            bd.put("StunThreshold", tb);
            let custom = color_code("CUSTOM");
            bd.put(
                "StunAvoidChance",
                BTable::lines([
                    format!("{custom}NOTE: Having any energy shield when the hit occurs grants 50% chance to avoid stun."),
                    format!("{custom}POB only applies this modifier when ES > Total incoming damage."),
                ]),
            );
        }
        if on(out, "StunAvoidChance") >= 100.0 {
            out.set_num("StunDuration", 0.0);
            out.set_num("BlockDuration", 0.0);
            // :2200-2203
            if let Some(bd) = bd.as_deref_mut() {
                bd.put("StunDuration", BTable::lines(["Cannot be Stunned".to_owned()]));
                bd.put("BlockDuration", BTable::lines(["Cannot be Stunned".to_owned()]));
            }
        } else {
            let stun_duration = 1.0 + inc(&db, &["StunDuration"]) / 100.0;
            let base_stun_duration = misc("StunBaseDuration");
            let stun_recovery = 1.0 + inc(&db, &["StunRecovery"]) / 100.0;
            let stun_and_block_recovery = 1.0 + inc(&db, &["StunRecovery", "BlockRecovery"]) / 100.0;
            let tick = misc("ServerTickRate");
            out.set_num(
                "StunDuration",
                (base_stun_duration * stun_duration / stun_recovery * tick).ceil() / tick,
            );
            out.set_num(
                "BlockDuration",
                (base_stun_duration * stun_duration / stun_and_block_recovery * tick).ceil() / tick,
            );
            // :2211-2229
            if let Some(bd) = bd.as_deref_mut() {
                let mut s = BTable::lines([sf!("%.2fs ^8(base)", base_stun_duration)]);
                let mut b = BTable::lines([sf!("%.2fs ^8(base)", base_stun_duration)]);
                if stun_duration != 1.0 {
                    s.push(sf!("* %.2f ^8(increased duration)", stun_duration));
                    b.push(sf!("* %.2f ^8(increased duration)", stun_duration));
                }
                if stun_recovery != 1.0 {
                    s.push(sf!("/ %.2f ^8(increased/reduced recovery)", stun_recovery));
                }
                if stun_and_block_recovery != 1.0 {
                    b.push(sf!("/ %.2f ^8(increased/reduced block recovery)", stun_and_block_recovery));
                }
                s.push(sf!("rounded up to nearest server tick"));
                s.push(sf!("= %.2fs", on(out, "StunDuration")));
                b.push(sf!("rounded up to nearest server tick"));
                b.push(sf!("= %.2fs", on(out, "BlockDuration")));
                bd.put("StunDuration", s);
                bd.put("BlockDuration", b);
            }
        }
        out.set_num("InterruptStunAvoidChance", min2(base(&db, &["AvoidInterruptStun"]), 100.0));
        let mut effective_enemy_damage = on(out, "totalTakenHit") + on(out, "PhysicalTakenHit") * 0.25;
        // :2233-2237 verbatim: for "Average" the `elseif ~= "Melee"` fires.
        if !is(&dcc, "Average") {
            effective_enemy_damage = effective_enemy_damage * (1.0 + misc("StunNotMeleeDamageMult") * 3.0) / 4.0;
        } else if !is(&dcc, "Melee") {
            effective_enemy_damage *= misc("StunNotMeleeDamageMult");
        }
        let base_stun_chance = min2(misc("StunBaseMult") * effective_enemy_damage / on(out, "StunThreshold"), 100.0);
        let v = (if base_stun_chance > misc("MinStunChanceNeeded") {
            base_stun_chance
        } else {
            0.0
        }) * not_avoid_chance
            / 100.0;
        out.set_num("SelfStunChance", v);
        // :2240-2255
        if let Some(bd) = bd.as_deref_mut() {
            let mut tb = BTable::lines([
                sf!("%d%% ^8(stun multiplier)", misc("StunBaseMult")),
                sf!("* %.1f ^8(effective enemy stun damage)", effective_enemy_damage),
                sf!("/ %d ^8(stun threshold)", on(out, "StunThreshold")),
            ]);
            if base_stun_chance < misc("MinStunChanceNeeded") {
                tb.push(sf!("= %.2f%%", base_stun_chance));
                tb.push(sf!("Stun Chance has to be more than %d%% to stun.", misc("MinStunChanceNeeded")));
            } else {
                if not_avoid_chance != 100.0 {
                    tb.push(sf!("* %.2f ^8(1 - chance to avoid stun)", not_avoid_chance / 100.0));
                }
                tb.push(sf!("= %.2f%%", on(out, "SelfStunChance")));
            }
            bd.put("SelfStunChance", tb);
        }
    }

    // --- :2258-2272 life recoverable -----------------------------------------------------------------------------
    copy(out, "LifeRecoverable", "LifeUnreserved");
    if cfg_truthy(input.get("conditionLowLife")) {
        let low = match out.get("LowLifePercentage") {
            None | Some(OutValue::Bool(false)) => misc("LowPoolThreshold"),
            Some(OutValue::Num(v)) => *v,
            Some(other) => panic!("{}: LowLifePercentage is {other:?}", site(2261)),
        };
        let v = min2(on(out, "Life") * low / 100.0, on(out, "LifeUnreserved"));
        out.set_num("LifeRecoverable", v);
        if on(out, "LifeRecoverable") < on(out, "LifeUnreserved") {
            out.set_bool("CappingLife", true);
        }
    }
    // :2268-2270 Dissolution of the Flesh
    if fl(&["DamageInsteadReservesLife"]) {
        let v = (on(out, "LifeCancellableReservation") / 100.0) * on(out, "Life");
        out.set_num("LifeRecoverable", v);
    }
    let v = max2(first_num(out, false, &["LifeRecoverable"], 2272), 1.0);
    out.set_num("LifeRecoverable", v);

    // --- :2274-2320 prevented life loss ---------------------------------------------------------------------------
    {
        let recoverable = on(out, "LifeRecoverable");
        let prevented_life_loss = min2(base(&db, &["LifeLossPrevented"]), 100.0);
        out.set_num("preventedLifeLoss", prevented_life_loss);
        let initial_below_half = base(&db, &["LifeLossBelowHalfPrevented"]);
        let v = (1.0 - on(out, "preventedLifeLoss") / 100.0) * initial_below_half;
        out.set_num("preventedLifeLossBelowHalf", v);
        let mut portion_life = 1.0;
        if !cfg_truthy(input.get("conditionLowLife")) {
            // :2284
            portion_life = min2(on(out, "Life") * 0.5 / recoverable, 1.0);
            let v = on(out, "preventedLifeLoss") + on(out, "preventedLifeLossBelowHalf") * portion_life;
            out.set_num("preventedLifeLossTotal", v);
        } else {
            let v = on(out, "preventedLifeLoss") + on(out, "preventedLifeLossBelowHalf");
            out.set_num("preventedLifeLossTotal", v);
        }
        let v = calc_life_hit_pool_with_loss_prevention(recoverable, on(out, "Life"), prevented_life_loss, initial_below_half);
        out.set_num("LifeHitPool", v);
        // :2291-2319
        if let Some(bd) = bd.as_deref_mut() {
            let pll = on(out, "preventedLifeLoss");
            let pbh = on(out, "preventedLifeLossBelowHalf");
            let mut t = BTable::lines([sf!("Total life protected:")]);
            if pll != 0.0 {
                t.push(sf!("%.2f ^8(portion taken over 4 seconds instead)", pll / 100.0));
            }
            if pbh != 0.0 {
                let plus = if pll != 0.0 { "+ " } else { "" };
                if portion_life != 1.0 {
                    if pll != 0.0 {
                        t.push(sf!(""));
                    }
                    t.push(sf!(
                        "%s%.2f ^8(initial portion taken by petrified blood)",
                        plus,
                        initial_below_half / 100.0
                    ));
                    if pll != 0.0 {
                        t.push(sf!("* %.2f ^8(portion not already taken over time)", 1.0 - pll / 100.0));
                    }
                    t.push(sf!("* %.2f ^8(portion of life on low life)", portion_life));
                    t.push(sf!("= %.2f ^8(final portion taken by petrified blood)", pbh * portion_life / 100.0));
                    t.push(sf!(""));
                } else {
                    let what = if pll != 0.0 { "initial portion" } else { "portion" };
                    t.push(sf!(
                        "%s%.2f ^8(%s taken by petrified blood)",
                        plus,
                        initial_below_half / 100.0,
                        what
                    ));
                    if pll != 0.0 {
                        t.push(sf!("* %.2f ^8(portion not already taken over time)", 1.0 - pll / 100.0));
                        t.push(sf!("= %.2f ^8(final portion taken by petrified blood)", pbh / 100.0));
                    }
                }
            }
            t.push(sf!(
                "%.2f ^8(portion taken from life)",
                1.0 - on(out, "preventedLifeLossTotal") / 100.0
            ));
            bd.put("preventedLifeLossTotal", t);
        }
    }

    // --- :2322-2344 energy shield bypass -------------------------------------------------------------------------
    out.set_bool("AnyBypass", false);
    out.set_num("MinimumBypass", 100.0);
    for (t, name) in DMG.iter().enumerate() {
        let key = format!("{name}EnergyShieldBypass");
        if fl(&["UnblockedDamageDoesBypassES"]) {
            out.set_num(&key, 100.0);
            out.set_bool("AnyBypass", true);
        } else {
            // :2330 `Override or Sum or 0` -- the `or 0` is dead.
            out.set_num(&key, or_override(override_(&db, &key), base(&db, &[&key]), 2330));
            if on(out, &key) != 0.0 {
                out.set_bool("AnyBypass", true);
            }
            if t == CHAOS {
                if !fl(&["ChaosNotBypassEnergyShield"]) {
                    let v = on(out, &key) + 100.0;
                    out.set_num(&key, v);
                } else {
                    out.set_bool("AnyBypass", true);
                }
            }
        }
        let v = max2(min2(on(out, &key), 100.0), 0.0);
        out.set_num(&key, v);
        let v = min2(on(out, "MinimumBypass"), on(out, &key));
        out.set_num("MinimumBypass", v);
    }

    // --- :2346-2421 Mind over Matter -----------------------------------------------------------------------------
    out.set_bool("ehpSectionAnySpecificTypes", false);
    out.set_bool("OnlySharedMindOverMatter", false);
    out.set_bool("AnySpecificMindOverMatter", false);
    out.set_num("sharedMindOverMatter", min2(base(&db, &["DamageTakenFromManaBeforeLife"]), 100.0));
    if on(out, "sharedMindOverMatter") > 0.0 {
        let mom = on(out, "sharedMindOverMatter") / 100.0;
        let es_bypass = on(out, "MinimumBypass") / 100.0;
        let r = calc_mom_eb_pool(out, &db, on(out, "LifeRecoverable"), mom, es_bypass);
        out.set_num("sharedManaEffectiveLife", r.0);
        // :2370 a call in an assignment keeps its first value only.
        let v = calc_mom_eb_pool(out, &db, on(out, "LifeHitPool"), mom, es_bypass).0;
        out.set_num("sharedMoMHitPool", v);
        // :2372-2385 (`output.sharedMindOverMatter` is a number: truthy)
        if let Some(bd) = bd.as_deref_mut() {
            bd.put("sharedMindOverMatter", mom_breakdown(out, &db, mom, r));
        }
    } else {
        copy(out, "sharedManaEffectiveLife", "LifeRecoverable");
        copy(out, "sharedMoMHitPool", "LifeHitPool");
    }
    for name in DMG {
        let mom_key = format!("{name}MindOverMatter");
        let v = min2(
            base(&db, &[&format!("{name}DamageTakenFromManaBeforeLife")]),
            100.0 - on(out, "sharedMindOverMatter"),
        );
        out.set_num(&mom_key, v);
        let bypass_key = format!("{name}EnergyShieldBypass");
        if on(out, &mom_key) > 0.0 || (on(out, &bypass_key) > on(out, "MinimumBypass") && on(out, "sharedMindOverMatter") > 0.0) {
            out.set_bool("ehpSectionAnySpecificTypes", true);
            out.set_bool("AnySpecificMindOverMatter", true);
            // :2395 set false again -- never true upstream.
            out.set_bool("OnlySharedMindOverMatter", false);
            let mom = (on(out, &mom_key) + on(out, "sharedMindOverMatter")) / 100.0;
            let es_bypass = on(out, &bypass_key) / 100.0;
            let r = calc_mom_eb_pool(out, &db, on(out, "LifeRecoverable"), mom, es_bypass);
            out.set_num(&format!("{name}ManaEffectiveLife"), r.0);
            let v = calc_mom_eb_pool(out, &db, on(out, "LifeHitPool"), mom, es_bypass).0;
            out.set_num(&format!("{name}MoMHitPool"), v);
            // :2403-2416 (`output[T.."MindOverMatter"]` is a number: truthy)
            if let Some(bd) = bd.as_deref_mut() {
                bd.put(mom_key.clone(), mom_breakdown(out, &db, mom, r));
            }
        } else {
            copy(out, &format!("{name}ManaEffectiveLife"), "sharedManaEffectiveLife");
            copy(out, &format!("{name}MoMHitPool"), "sharedMoMHitPool");
        }
    }

    // --- :2423-2458 guard ----------------------------------------------------------------------------------------
    out.set_bool("AnyGuard", false);
    out.set_num("sharedGuardAbsorbRate", min2(base(&db, &["GuardAbsorbRate"]), 100.0));
    if on(out, "sharedGuardAbsorbRate") > 0.0 {
        out.set_bool("OnlySharedGuard", true);
        out.set_num("sharedGuardAbsorb", cval(&db, "GuardAbsorbLimit"));
        // :2429-2438
        if let Some(bd) = bd.as_deref_mut() {
            bd.put(
                "sharedGuardAbsorb",
                guard_breakdown(out, "sharedGuardAbsorb", "sharedGuardAbsorbRate"),
            );
        }
    }
    for name in DMG {
        let rate_key = format!("{name}GuardAbsorbRate");
        out.set_num(&rate_key, min2(base(&db, &[&rate_key]), 100.0));
        if on(out, &rate_key) > 0.0 {
            out.set_bool("ehpSectionAnySpecificTypes", true);
            out.set_bool("AnyGuard", true);
            out.set_bool("OnlySharedGuard", false);
            out.set_num(&format!("{name}GuardAbsorb"), cval(&db, &format!("{name}GuardAbsorbLimit")));
            // :2447-2456
            if let Some(bd) = bd.as_deref_mut() {
                bd.put(
                    format!("{name}GuardAbsorb"),
                    guard_breakdown(out, &format!("{name}GuardAbsorb"), &rate_key),
                );
            }
        }
    }

    // --- :2460-2483 aegis ----------------------------------------------------------------------------------------
    out.set_bool("AnyAegis", false);
    // `Max()` answers nil unless some value is > 0, hence `or 0`.
    out.set_num("sharedAegis", max_(&db, "AegisValue").unwrap_or(0.0));
    out.set_num("sharedElementalAegis", max_(&db, "ElementalAegisValue").unwrap_or(0.0));
    if on(out, "sharedAegis") > 0.0 {
        out.set_bool("AnyAegis", true);
    }
    if on(out, "sharedElementalAegis") > 0.0 {
        out.set_bool("ehpSectionAnySpecificTypes", true);
        out.set_bool("AnyAegis", true);
    }
    for (t, name) in DMG.iter().enumerate() {
        let aegis_value = max_(&db, &format!("{name}AegisValue")).unwrap_or(0.0);
        if aegis_value > 0.0 {
            out.set_bool("ehpSectionAnySpecificTypes", true);
            out.set_bool("AnyAegis", true);
            out.set_num(&format!("{name}Aegis"), aegis_value);
        } else {
            out.set_num(&format!("{name}Aegis"), 0.0);
        }
        if is_elemental(t) {
            let v = on(out, &format!("{name}Aegis")) + on(out, "sharedElementalAegis");
            out.set_num(&format!("{name}AegisDisplay"), v);
        }
    }

    // --- :2485-2571 damage taken from allies before you ----------------------------------------------------------
    {
        // :2488-2489 Frost Shield (the `lifeProtected` at :2491 is breakdown-only).
        out.set_num("FrostShieldLife", base(&db, &["FrostGlobeHealth"]));
        out.set_num("FrostShieldDamageMitigation", base(&db, &["FrostGlobeDamageMitigation"]));
        // :2491-2500
        if let Some(bd) = bd.as_deref_mut() {
            let (life, mitigation) = (on(out, "FrostShieldLife"), on(out, "FrostShieldDamageMitigation"));
            let life_protected = life / (mitigation / 100.0) * (1.0 - mitigation / 100.0);
            bd.put(
                "FrostShieldLife",
                BTable::lines([
                    sf!("Total life protected:"),
                    sf!("%d ^8(frost shield limit)", life),
                    sf!("/ %.2f ^8(portion taken from frost shield)", mitigation / 100.0),
                    sf!("x %.2f ^8(portion taken from life and energy shield)", 1.0 - mitigation / 100.0),
                    sf!("= %d", life_protected),
                ]),
            );
        }

        // :2502-2517
        for ally in ALLY_LIFE_POOL_LIST.iter() {
            if let Some(redirect) = ally.redirect {
                let mut mitigation = base(&db, &[redirect]);
                if ally.melee_only {
                    mitigation = if is(&dcc, "Melee") {
                        mitigation
                    } else if is(&dcc, "Average") {
                        mitigation / 4.0
                    } else {
                        0.0
                    };
                }
                out.set_num(ally.mitigation, mitigation);
                if mitigation != 0.0 {
                    let mut life = base(&db, &[ally.life]);
                    if life == 0.0
                        && let Some(fallback) = ally.fallback
                    {
                        life = base(&db, &[fallback]);
                    }
                    out.set_num(ally.life, or_override(override_(&db, ally.life), life, 2514));
                }
            }
        }

        // :2519-2534 Companionship (`sharedMinionLifeOverride` is read by the breakdown only).
        let mut shared_minion_life_override = false;
        for ally in ALLY_LIFE_POOL_LIST.iter() {
            if fl(&[&format!("MinionLifeShares{}", ally.life)]) {
                let specific_override = override_(&db, ally.life);
                let total_override = override_(&db, "TotalMinionLife");
                if !total_override.as_ref().is_some_and(Value::truthy)
                    && let Some(so) = specific_override
                {
                    put(out, "TotalMinionLife", Some(OutValue::Num(num_val(&so, 2526))));
                    shared_minion_life_override = true;
                } else if !truthy(out, "TotalMinionLife") || out.opt_num("TotalMinionLife") == Some(0.0) {
                    copy(out, "TotalMinionLife", ally.life);
                }
                let v = on(out, "MinionAllyDamageMitigation") + or0(out, ally.mitigation, 2531);
                out.set_num("MinionAllyDamageMitigation", v);
                out.set_num(ally.mitigation, 0.0);
                // :2533 `output[ally.life] = nil`
                out.remove(ally.life);
            }
        }

        // :2536-2541 Vaal Rejuvenation as the nearest Totem
        let total_totem_life_zero = match out.get("TotalTotemLife") {
            None | Some(OutValue::Bool(false)) => true,
            Some(OutValue::Num(v)) => *v == 0.0,
            _ => false,
        };
        if or0(out, "TotemAllyDamageMitigation", 2538) > 0.0
            && total_totem_life_zero
            && or0(out, "TotalVaalRejuvenationTotemLife", 2538) > 0.0
        {
            let v = on(out, "VaalRejuvenationTotemAllyDamageMitigation") + on(out, "TotemAllyDamageMitigation");
            out.set_num("VaalRejuvenationTotemAllyDamageMitigation", v);
            out.set_num("TotemAllyDamageMitigation", 0.0);
            out.remove("TotalTotemLife");
        }

        // :2544-2559
        if let Some(bd) = bd.as_deref_mut() {
            for ally in ALLY_LIFE_POOL_LIST.iter() {
                let life_list = ally_life_list.and_then(|l| l.get(ally.life));
                let ov = override_(&db, ally.life).filter(Value::truthy);
                let override_truthy =
                    ov.is_some() || (ally.life == "TotalMinionLife" && shared_minion_life_override && truthy(out, "TotalMinionLife"));
                if truthy(out, ally.life) && (life_list.is_some() || override_truthy) {
                    let mut t = BTable::new();
                    if override_truthy {
                        t.push(sf!("%d ^8(from config)", on(out, ally.life)));
                    } else {
                        for entry in life_list.expect("lifeList") {
                            let count = match entry.count {
                                Some(c) if c > 1.0 => format!("{}x ", tostring_num(c)),
                                _ => String::new(),
                            };
                            t.push(sf!("%d ^8(%s%s)", entry.life, count, entry.name.as_str()));
                        }
                    }
                    bd.put(ally.life, t);
                }
            }
        }
        // :2561-2570 allied energy shield. The player has no parent; its party stub's output is empty (P5).
        out.set_num("SoulLinkMitigation", base(&db, &["TakenFromParentESBeforeYou"]));
        if on(out, "SoulLinkMitigation") != 0.0 {
            let p = parent.unwrap_or_else(|| panic!("{}: attempt to index field 'parent' (a nil value)", site(2564)));
            // `actor.parent.output.EnergyShieldRecoveryCap or 0`
            let v = p.opt_num("EnergyShieldRecoveryCap").unwrap_or(0.0);
            out.set_num("AlliedEnergyShield", v);
        } else {
            out.set_num("SoulLinkMitigation", base(&db, &["TakenFromPartyMemberESBeforeYou"]));
            if on(out, "SoulLinkMitigation") != 0.0 {
                if !is_player {
                    panic!("{}: attempt to index field 'partyMembers' (a nil value)", site(2568));
                }
                // `actor.partyMembers.output.EnergyShieldRecoveryCap or 0` -- the stub's output is empty.
                out.set_num("AlliedEnergyShield", 0.0);
            }
        }
    }

    // --- :2573-2577 Vaal Arctic Armour ---------------------------------------------------------------------------
    out.set_num("VaalArcticArmourLife", base(&db, &["VaalArcticArmourMaxHits"]));
    // :2576 `Sum("MORE")` sums the values; without mods `-0 / 100` = -0 (O9).
    out.set_num(
        "VaalArcticArmourMitigation",
        min2(-sum(&db, More, None, &["VaalArcticArmourMitigation"]) / 100.0, 1.0),
    );

    // --- :2579-2610 total pools ----------------------------------------------------------------------------------
    for name in DMG {
        let pool_key = format!("{name}TotalPool");
        let hit_key = format!("{name}TotalHitPool");
        copy(out, &pool_key, &format!("{name}ManaEffectiveLife"));
        copy(out, &hit_key, &format!("{name}MoMHitPool"));
        let bypass = on(out, &format!("{name}EnergyShieldBypass"));
        let mut manatext = "Mana";
        if bypass < 100.0 {
            if fl(&["EnergyShieldProtectsMana"]) {
                // :2586
                manatext = "Mana and non-bypassed Energy Shield";
            } else if bypass > 0.0 {
                let pool_protected = on(out, "EnergyShieldRecoveryCap") / (1.0 - bypass / 100.0) * (bypass / 100.0);
                let p = on(out, &pool_key);
                out.set_num(
                    &pool_key,
                    max2(p - pool_protected, 0.0) + min2(p, pool_protected) / (bypass / 100.0),
                );
                let p = on(out, &hit_key);
                out.set_num(&hit_key, max2(p - pool_protected, 0.0) + min2(p, pool_protected) / (bypass / 100.0));
            } else {
                let v = on(out, &pool_key) + on(out, "EnergyShieldRecoveryCap");
                out.set_num(&pool_key, v);
                let v = on(out, &hit_key) + on(out, "EnergyShieldRecoveryCap");
                out.set_num(&hit_key, v);
            }
        }
        // :2598-2609
        if let Some(bd) = bd.as_deref_mut() {
            let mel = on(out, &format!("{name}ManaEffectiveLife"));
            let mut t = BTable::lines([sf!("Life: %d", on(out, "LifeRecoverable"))]);
            if mel != on(out, "LifeRecoverable") {
                t.push(sf!("%s through MoM: %d", manatext, mel - on(out, "LifeRecoverable")));
            }
            if !fl(&["EnergyShieldProtectsMana"]) && on(out, &format!("{name}EnergyShieldBypass")) < 100.0 {
                t.push(sf!("Non-bypassed Energy Shield: %d", on(out, &pool_key) - mel));
            }
            t.push(sf!("TotalPool: %d", on(out, &pool_key)));
            bd.put(pool_key.clone(), t);
        }
    }

    // ================================================================================================================
    // Part 2: CalcDefence.lua:2779-3786.
    // ================================================================================================================

    // --- :2779-2975 number of hits, effective hit pool, survival time ---------------------------------------------
    if !dot {
        // :2780-2788
        {
            let mut d = DamageIn::default();
            for (t, name) in DMG.iter().enumerate() {
                d.dmg[t] = on(out, &format!("{name}TakenHit"));
            }
            d.limit_ehp_speedup = on(out, "preventedLifeLossTotal") > 0.0;
            let v = number_of_hits_to_die(&mut d, out, &db, &shift);
            out.set_num("NumberOfDamagingHits", v);
        }

        // :2790-2900 the same, mitigated by block, suppression and avoidance
        {
            let mut d = DamageIn::default();
            let mut block_chance = on(out, "EffectiveBlockChance") / 100.0;
            if !is(&dcc, "Melee") && !is(&dcc, "Untyped") {
                let k = concat_key(&dcc, 2794);
                block_chance = first_num(out, false, &[&format!("Effective{k}BlockChance")], 2794) / 100.0;
            }
            if flag(&enemy_db, &["CannotBeBlocked"]) {
                block_chance = 0.0;
            }
            let block_effect = 1.0 - block_chance * on(out, "BlockEffect") / 100.0;
            let mut suppress_chance = 0.0;
            let mut suppression_effect = 1.0;
            let mut extra_avoid_chance = 0.0;
            let mut average_avoid_chance = 0.0;
            let gain_on_block = !cfg_truthy(input.get("DisableEHPGainOnBlock")) && on(out, "NumberOfDamagingHits") > 1.0;
            // :2804-2813
            if gain_on_block {
                d.life_when_hit = Some(on(out, "LifeOnBlock") * block_chance);
                d.mana_when_hit = Some(on(out, "ManaOnBlock") * block_chance);
                d.energy_shield_when_hit = Some(on(out, "EnergyShieldOnBlock") * block_chance);
                if is(&dcc, "Spell") || is(&dcc, "SpellProjectile") {
                    d.energy_shield_when_hit =
                        Some(d.energy_shield_when_hit.expect("set") + on(out, "EnergyShieldOnSpellBlock") * block_chance);
                } else if is(&dcc, "Average") {
                    d.energy_shield_when_hit =
                        Some(d.energy_shield_when_hit.expect("set") + on(out, "EnergyShieldOnSpellBlock") / 2.0 * block_chance);
                }
            }
            // :2814-2829
            if is(&dcc, "Spell") || is(&dcc, "SpellProjectile") || is(&dcc, "Average") {
                suppress_chance = on(out, "EffectiveSpellSuppressionChance") / 100.0;
            }
            if suppress_chance < 1.0 {
                if is(&dcc, "Average") {
                    suppress_chance /= 2.0;
                }
                d.energy_shield_when_hit =
                    Some(d.energy_shield_when_hit.unwrap_or(0.0) + on(out, "EnergyShieldOnSuppress") * suppress_chance);
                d.life_when_hit = Some(d.life_when_hit.unwrap_or(0.0) + on(out, "LifeOnSuppress") * suppress_chance);
                suppression_effect = 1.0 - suppress_chance * on(out, "SpellSuppressionEffect") / 100.0;
            } else {
                let half = if is(&dcc, "Average") { 0.5 } else { 1.0 };
                d.energy_shield_when_hit = Some(d.energy_shield_when_hit.unwrap_or(0.0) + on(out, "EnergyShieldOnSuppress") * half);
                d.life_when_hit = Some(d.life_when_hit.unwrap_or(0.0) + on(out, "LifeOnSuppress") * half);
            }
            // :2830-2835
            if is(&dcc, "Projectile") || is(&dcc, "SpellProjectile") {
                extra_avoid_chance += on(out, "AvoidProjectilesChance");
            } else if is(&dcc, "Average") {
                extra_avoid_chance += on(out, "AvoidProjectilesChance") / 2.0;
            }
            // :2836-2847
            if gain_on_block {
                let ml = base(&db, &["MissingLifeBeforeEnemyHit"]);
                let mm = base(&db, &["MissingManaBeforeEnemyHit"]);
                d.missing_life_before_enemy_hit = Some(ml);
                d.missing_mana_before_enemy_hit = Some(mm);
                if d.life_when_hit.unwrap_or(0.0) != 0.0
                    || d.mana_when_hit.unwrap_or(0.0) != 0.0
                    || d.energy_shield_when_hit != Some(0.0)
                    || ml != 0.0
                    || mm != 0.0
                {
                    d.gain_when_hit = true;
                }
            } else {
                d.life_when_hit = Some(0.0);
                d.mana_when_hit = Some(0.0);
                d.energy_shield_when_hit = Some(0.0);
            }
            for (t, name) in DMG.iter().enumerate() {
                // :2849-2852 Emperor's Vigilance -- `DamageIn[T.."EnergyShieldBypass"]` is read by nothing: not kept.
                let mut avoid_chance = 0.0;
                if truthy(out, "specificTypeAvoidance") {
                    avoid_chance = min2(
                        on(out, &format!("Avoid{name}DamageChance")) + extra_avoid_chance,
                        misc("AvoidChanceCap"),
                    );
                    // :2856-2863
                    let wo = worst_of(2858);
                    if wo > 1.0 {
                        avoid_chance = avoid_chance / 100.0 * avoid_chance;
                        if wo == 4.0 {
                            avoid_chance = avoid_chance / 100.0 * avoid_chance;
                        }
                    }
                    average_avoid_chance += avoid_chance;
                }
                d.dmg[t] = on(out, &format!("{name}TakenHit")) * (block_effect * suppression_effect * (1.0 - avoid_chance / 100.0));
            }
            // :2868-2874
            if on(out, "anyRecoup") > 0.0 {
                d.track_recoupable = true;
                for name in DMG {
                    out.set_num(&format!("{name}RecoupableDamageTaken"), 0.0);
                }
            }
            // :2875-2880
            if on(out, "preventedLifeLossTotal") > 0.0 {
                d.track_life_loss_over_time = true;
                out.set_num("LifeLossLostOverTime", 0.0);
                out.set_num("LifeBelowHalfLossLostOverTime", 0.0);
            }
            // :2881
            d.limit_ehp_speedup = d.track_recoupable || d.track_life_loss_over_time || d.gain_when_hit;
            average_avoid_chance /= 5.0;
            out.set_num(
                "ConfiguredDamageChance",
                100.0 * (block_effect * suppression_effect * (1.0 - average_avoid_chance / 100.0)),
            );
            // :2884 lazy: the call writes the recoup / life-loss outputs.
            if on(out, "ConfiguredDamageChance") != 100.0 || d.track_recoupable || d.track_life_loss_over_time || d.gain_when_hit {
                let v = number_of_hits_to_die(&mut d, out, &db, &shift);
                out.set_num("NumberOfMitigatedDamagingHits", v);
            } else {
                copy(out, "NumberOfMitigatedDamagingHits", "NumberOfDamagingHits");
            }
            // :2885-2899
            if let Some(bd) = bd.as_deref_mut() {
                let mut t = BTable::lines([sf!("%.2f ^8(chance for block to fail)", 1.0 - block_chance)]);
                if truthy(out, "ShowBlockEffect") {
                    t.push(sf!("x %.2f ^8(block effect)", on(out, "BlockEffect") / 100.0));
                }
                if suppression_effect != 1.0 {
                    t.push(sf!("x %.3f ^8(suppression effect)", suppression_effect));
                }
                if average_avoid_chance > 0.0 {
                    t.push(sf!("x %.2f ^8(chance for avoidance to fail)", 1.0 - average_avoid_chance / 100.0));
                }
                t.push(sf!(
                    "= %.1f%% ^8(of damage taken from a%s hit)",
                    on(out, "ConfiguredDamageChance"),
                    a_hit(&dcc, 2898)
                ));
                bd.put("ConfiguredDamageChance", t);
            }
        }

        // :2902-2949
        let v = on(out, "NumberOfMitigatedDamagingHits") / (1.0 - on(out, "ConfiguredNotHitChance") / 100.0);
        out.set_num("TotalNumberOfHits", v);
        if let Some(bd) = bd.as_deref_mut() {
            let mut t = BTable::new();
            if is(&dcc, "Melee") || is(&dcc, "Projectile") {
                let k = concat_key(&dcc, 2908);
                if on(out, &format!("{k}EvadeChance")) > 0.0 {
                    t.push(sf!(
                        "%.2f ^8(chance for evasion to fail)",
                        1.0 - on(out, &format!("{k}EvadeChance")) / 100.0
                    ));
                }
                if on(out, "AttackDodgeChance") > 0.0 {
                    t.push(sf!(
                        "x %.2f ^8(chance for dodge to fail)",
                        1.0 - on(out, "AttackDodgeChance") / 100.0
                    ));
                }
                if is(&dcc, "Projectile") && !truthy(out, "specificTypeAvoidance") && on(out, "AvoidProjectilesChance") > 0.0 {
                    t.push(sf!(
                        "x %.2f ^8(chance for avoidance to fail)",
                        1.0 - on(out, "AvoidProjectilesChance") / 100.0
                    ));
                }
            } else if is(&dcc, "Spell") || is(&dcc, "SpellProjectile") {
                if on(out, "SpellDodgeChance") > 0.0 {
                    t.push(sf!("%.2f ^8(chance for dodge to fail)", 1.0 - on(out, "SpellDodgeChance") / 100.0));
                }
                if is(&dcc, "SpellProjectile") && !truthy(out, "specificTypeAvoidance") && on(out, "AvoidProjectilesChance") > 0.0 {
                    t.push(sf!(
                        "x %.2f ^8(chance for avoidance to fail)",
                        1.0 - on(out, "AvoidProjectilesChance") / 100.0
                    ));
                }
            } else if is(&dcc, "Average") {
                if on(out, "MeleeEvadeChance") > 0.0 || on(out, "ProjectileEvadeChance") > 0.0 {
                    t.push(sf!(
                        "%.2f ^8(chance for evasion to fail, only applies to the attack portion)",
                        1.0 - (on(out, "MeleeEvadeChance") + on(out, "ProjectileEvadeChance")) / 2.0 / 100.0
                    ));
                }
                if on(out, "AttackDodgeChance") > 0.0 || on(out, "SpellDodgeChance") > 0.0 {
                    t.push(sf!(
                        "x%.2f ^8(chance for dodge to fail)",
                        1.0 - (on(out, "AttackDodgeChance") + on(out, "SpellDodgeChance")) / 2.0 / 100.0
                    ));
                }
                if !truthy(out, "specificTypeAvoidance") && on(out, "AvoidProjectilesChance") > 0.0 {
                    t.push(sf!(
                        "x %.2f ^8(chance for avoidance to fail)",
                        1.0 - on(out, "AvoidProjectilesChance") / 2.0 / 100.0
                    ));
                }
            }
            if on(out, "AvoidAllDamageFromHitsChance") > 0.0 {
                t.push(sf!(
                    "x %.2f ^8(chance for damage avoidance to fail)",
                    1.0 - on(out, "AvoidAllDamageFromHitsChance") / 100.0
                ));
            }
            let wo = worst_of(2939);
            if wo > 1.0 {
                t.push(sf!("unlucky worst of %d", wo));
            }
            t.push(sf!(
                "= %d%% ^8(chance to be hit by a%s hit)",
                100.0 - on(out, "ConfiguredNotHitChance"),
                a_hit(&dcc, 2942)
            ));
            bd.put("ConfiguredNotHitChance", t);
            bd.put(
                "TotalNumberOfHits",
                BTable::lines([
                    sf!("%.2f ^8(Number of mitigated hits)", on(out, "NumberOfMitigatedDamagingHits")),
                    sf!("/ %.2f ^8(Chance to even be hit)", 1.0 - on(out, "ConfiguredNotHitChance") / 100.0),
                    sf!("= %.2f ^8(total average number of hits you can take)", on(out, "TotalNumberOfHits")),
                ]),
            );
        }
        // :2951-2959
        let v = on(out, "TotalNumberOfHits") * on(out, "totalEnemyDamageIn");
        out.set_num("TotalEHP", v);
        if let Some(bd) = bd.as_deref_mut() {
            bd.put(
                "TotalEHP",
                BTable::lines([
                    sf!("%.2f ^8(total average number of hits you can take)", on(out, "TotalNumberOfHits")),
                    sf!("x %d ^8(total incoming damage)", on(out, "totalEnemyDamageIn")),
                    sf!("= %d ^8(total damage you can take)", on(out, "TotalEHP")),
                ]),
            );
        }
        // :2961-2974
        {
            let speed = cfg_or(&[input.get("enemySpeed"), placeholder.get("enemySpeed")], 700.0, 2963);
            out.set_num("enemySkillTime", speed / (1.0 + inc(&enemy_db, &["Speed"]) / 100.0));
            let enemy_action_speed = action_speed_mod(&enemy_db);
            let v = on(out, "enemySkillTime") / 1000.0 / enemy_action_speed;
            out.set_num("enemySkillTime", v);
            let v = on(out, "TotalNumberOfHits") * on(out, "enemySkillTime");
            out.set_num("EHPSurvivalTime", v);
            // :2967-2973
            if let Some(bd) = bd.as_deref_mut() {
                bd.put(
                    "EHPSurvivalTime",
                    BTable::lines([
                        sf!("%.2f ^8(total average number of hits you can take)", on(out, "TotalNumberOfHits")),
                        sf!("x %.2f ^8enemy attack/cast time", on(out, "enemySkillTime")),
                        sf!("= %.2f seconds ^8(total time it would take to die)", on(out, "EHPSurvivalTime")),
                    ]),
                );
            }
        }
    }

    // --- :2977-3064 recoup -----------------------------------------------------------------------------------------
    if on(out, "anyRecoup") > 0.0 && !dot {
        let mut total_damage = 0.0;
        let mut total_elemental_damage = 0.0;
        let total_physical_damage_mitigated =
            on(out, "NumberOfMitigatedDamagingHits") * (on(out, "PhysicalTakenDamage") - on(out, "PhysicalTakenHit"));
        let mut extra_pseudo_recoup: [Option<(f64, f64)>; 3] = [None; 3];
        for (t, name) in DMG.iter().enumerate() {
            let v = on(out, &format!("{name}RecoupableDamageTaken"));
            total_damage += v;
            if is_elemental(t) {
                total_elemental_damage += v;
            }
        }
        for (i, recoup_type) in RECOUP_TYPES.iter().enumerate() {
            let recoup_time = if fl(&[&format!("3Second{recoup_type}Recoup")]) || fl(&["3SecondRecoup"]) {
                3.0
            } else {
                4.0
            };
            let total_key = format!("Total{recoup_type}RecoupRecovery");
            out.set_num(&total_key, or0(out, &format!("{recoup_type}Recoup"), 2992) / 100.0 * total_damage);
            // :2993-2995 `Elemental<R>Recoup` is written nowhere upstream.
            let el_key = format!("Elemental{recoup_type}Recoup");
            if or0(out, &el_key, 2993) > 0.0 && total_elemental_damage > 0.0 {
                let v = on(out, &total_key) + on(out, &el_key) / 100.0 * total_elemental_damage;
                out.set_num(&total_key, v);
            }
            for name in DMG {
                let rk = format!("{name}{recoup_type}Recoup");
                let dk = format!("{name}RecoupableDamageTaken");
                if or0(out, &rk, 2997) > 0.0 && on(out, &dk) > 0.0 {
                    let v = on(out, &total_key) + on(out, &rk) / 100.0 * on(out, &dk);
                    out.set_num(&total_key, v);
                }
            }
            let pseudo_key = format!("Total{recoup_type}PseudoRecoup");
            let v = or0(out, &format!("PhysicalDamageMitigated{recoup_type}PseudoRecoup"), 3001) / 100.0 * total_physical_damage_mitigated;
            out.set_num(&pseudo_key, v);
            let dur_key = format!("PhysicalDamageMitigated{recoup_type}PseudoRecoupDuration");
            let pseudo_recoup_duration = match out.get(&dur_key) {
                None | Some(OutValue::Bool(false)) => 4.0,
                Some(OutValue::Num(v)) => *v,
                Some(other) => panic!("{}: {dur_key} is {other:?}", site(3012)),
            };
            // :3003-3010 Pious Path
            if on(out, &pseudo_key) != 0.0 {
                for to in RECOUP_TYPES.iter().skip(i + 1) {
                    let j = RECOUP_TYPES.iter().position(|r| r == to).expect("in list");
                    if fl(&[&format!("{recoup_type}RegenerationRecovers{to}")])
                        && !fl(&[&format!("UnaffectedBy{to}Regen")])
                        && !fl(&[&format!("No{to}Regen")])
                        && !fl(&[&format!("CannotGain{to}")])
                    {
                        extra_pseudo_recoup[j] = Some((
                            on(out, &pseudo_key) * on(out, &format!("{to}RecoveryRateMod"))
                                / on(out, &format!("{recoup_type}RecoveryRateMod")),
                            pseudo_recoup_duration,
                        ));
                    }
                }
            }
            // :3011
            if fl(&[&format!("UnaffectedBy{recoup_type}Regen")]) {
                out.set_num(&pseudo_key, 0.0);
            }
            // :3012-3013
            let extra = extra_pseudo_recoup[i];
            let v = on(out, &total_key) / recoup_time + on(out, &pseudo_key) / pseudo_recoup_duration + extra.map_or(0.0, |(a, b)| a / b);
            out.set_num(&format!("{recoup_type}RecoupRecoveryMax"), v);
            let st = on(out, "EHPSurvivalTime");
            let v = on(out, &total_key) / (st + recoup_time)
                + on(out, &pseudo_key) / (st + pseudo_recoup_duration)
                + extra.map_or(0.0, |(a, b)| a / (st + b));
            out.set_num(&format!("{recoup_type}RecoupRecoveryAvg"), v);
            // :3014-3062
            if let Some(bd) = bd.as_deref_mut() {
                let mut multiple_types = 0;
                let mut mx = BTable::new();
                let plus = |n: i32| if n > 0 { "+" } else { "" };
                if or0(out, &format!("{recoup_type}Recoup"), 3017) > 0.0 {
                    mx.push(sf!("%d ^8(total damage taken during ehp calcs)", total_damage));
                    mx.push(sf!(
                        "* %.2f ^8(percent of damage recouped)",
                        on(out, &format!("{recoup_type}Recoup")) / 100.0
                    ));
                    multiple_types += 1;
                }
                if or0(out, &el_key, 3022) > 0.0 && total_elemental_damage > 0.0 {
                    if multiple_types > 0 {
                        mx.push(sf!(""));
                    }
                    mx.push(sf!(
                        "%s%d ^8(total elemental damage taken during ehp calcs)",
                        plus(multiple_types),
                        total_damage
                    ));
                    mx.push(sf!("* %.2f ^8(percent of damage recouped)", on(out, &el_key) / 100.0));
                    multiple_types += 1;
                }
                for name in DMG {
                    let rk = format!("{name}{recoup_type}Recoup");
                    if or0(out, &rk, 3031) > 0.0 && on(out, &format!("{name}RecoupableDamageTaken")) > 0.0 {
                        if multiple_types > 0 {
                            mx.push(sf!(""));
                        }
                        mx.push(sf!(
                            "%s%d ^8(total %s damage taken during ehp calcs)",
                            plus(multiple_types),
                            total_damage,
                            name
                        ));
                        mx.push(sf!("* %.2f ^8(percent of damage recouped)", on(out, &rk) / 100.0));
                        multiple_types += 1;
                    }
                }
                mx.push(sf!("= %d ^8(total damage recoup amount)", on(out, &total_key)));
                let mut avg = mx.clone();
                mx.push(sf!("/ %.2f ^8(over %d seconds)", recoup_time, recoup_time));
                if on(out, &pseudo_key) > 0.0 {
                    mx.push(sf!("+ %.2f ^8(total damage mitigated pseudo recoup amount)", on(out, &pseudo_key)));
                    mx.push(sf!("/ %.2f ^8(over %d seconds)", pseudo_recoup_duration, pseudo_recoup_duration));
                }
                if let Some((a, b)) = extra {
                    mx.push(sf!("+ %.2f ^8(total damage mitigated pseudo recoup amount)", a));
                    mx.push(sf!("/ %.2f ^8(over %d seconds)", b, b));
                }
                mx.push(sf!("= %.2f per second ^8", on(out, &format!("{recoup_type}RecoupRecoveryMax"))));
                avg.push(sf!(
                    "/ %.2f ^8(total time of the recoup (survival time + %d seconds))",
                    st + recoup_time,
                    recoup_time
                ));
                if on(out, &pseudo_key) > 0.0 {
                    avg.push(sf!("+ %.2f ^8(total damage mitigated pseudo recoup amount)", on(out, &pseudo_key)));
                    avg.push(sf!(
                        "/ %.2f ^8(total time of the recoup (survival time + %d seconds)",
                        st + pseudo_recoup_duration,
                        pseudo_recoup_duration
                    ));
                }
                if let Some((a, b)) = extra {
                    avg.push(sf!("+ %.2f ^8(total damage mitigated pseudo recoup amount)", a));
                    avg.push(sf!("/ %.2f ^8(total time of the recoup (survival time + %d seconds)", st + b, b));
                }
                avg.push(sf!("= %.2f per second ^8", on(out, &format!("{recoup_type}RecoupRecoveryAvg"))));
                bd.put(format!("{recoup_type}RecoupRecoveryMax"), mx);
                bd.put(format!("{recoup_type}RecoupRecoveryAvg"), avg);
            }
        }
    }

    // --- :3066-3093 petrified blood "degen" ----------------------------------------------------------------------
    if on(out, "preventedLifeLossTotal") > 0.0 && truthy(out, "LifeLossLostOverTime") && truthy(out, "LifeBelowHalfLossLostOverTime") {
        let lost = base(&db, &["LifeLossBelowHalfLost"]) / 100.0;
        let v = (on(out, "LifeLossLostOverTime") + on(out, "LifeBelowHalfLossLostOverTime") * lost) / 4.0;
        out.set_num("LifeLossLostMax", v);
        let v = (on(out, "LifeLossLostOverTime") + on(out, "LifeBelowHalfLossLostOverTime") * lost) / (on(out, "EHPSurvivalTime") + 4.0);
        out.set_num("LifeLossLostAvg", v);
        // :3071-3092
        if let Some(bd) = bd.as_deref_mut() {
            let (over, below) = (on(out, "LifeLossLostOverTime"), on(out, "LifeBelowHalfLossLostOverTime"));
            let lines = |t: &mut BTable| {
                if over != 0.0 {
                    t.push(sf!("( %d ^8(total damage prevented by Progenesis)", over));
                }
                if below != 0.0 {
                    let open = if over != 0.0 { "+" } else { "(" };
                    t.push(sf!("%s %d ^8(total damage prevented by petrified blood)", open, below));
                    t.push(sf!("* %.2f ^8(percent of damage taken from petrified blood)", lost));
                }
            };
            let mut mx = BTable::new();
            lines(&mut mx);
            mx.push(sf!(") / %.2f ^8(over 4 seconds)", 4.0));
            mx.push(sf!("= %.2f per second", on(out, "LifeLossLostMax")));
            let mut avg = BTable::new();
            lines(&mut avg);
            avg.push(sf!(
                ") / %.2f ^8(total time of the degen (survival time + 4 seconds))",
                on(out, "EHPSurvivalTime") + 4.0
            ));
            avg.push(sf!("= %.2f per second", on(out, "LifeLossLostAvg")));
            bd.put("LifeLossLostMax", mx);
            bd.put("LifeLossLostAvg", avg);
        }
    }

    // --- :3095-3114 net recovery over time from enemy hits -------------------------------------------------------
    if or0(out, "LifeRecoupRecoveryAvg", 3096) > 0.0 || on(out, "preventedLifeLossTotal") > 0.0 {
        let v = or0(out, "LifeRecoupRecoveryMax", 3097) - or0(out, "LifeLossLostMax", 3097);
        out.set_num("netLifeRecoupAndLossLostOverTimeMax", v);
        let v = or0(out, "LifeRecoupRecoveryAvg", 3098) - or0(out, "LifeLossLostAvg", 3098);
        out.set_num("netLifeRecoupAndLossLostOverTimeAvg", v);
        if or0(out, "LifeRecoupRecoveryAvg", 3099) > 0.0 && on(out, "preventedLifeLossTotal") > 0.0 {
            out.set_bool("showNetRecoup", true);
            // :3101-3112
            if let Some(bd) = bd.as_deref_mut() {
                bd.put(
                    "netLifeRecoupAndLossLostOverTimeMax",
                    BTable::lines([
                        sf!("%.2f ^8(total life recouped per second)", on(out, "LifeRecoupRecoveryMax")),
                        sf!("- %.2f ^8(total life taken over time per second)", on(out, "LifeLossLostMax")),
                        sf!("= %.2f per second", on(out, "netLifeRecoupAndLossLostOverTimeMax")),
                    ]),
                );
                bd.put(
                    "netLifeRecoupAndLossLostOverTimeAvg",
                    BTable::lines([
                        sf!("%.2f ^8(total life recouped per second)", on(out, "LifeRecoupRecoveryAvg")),
                        sf!("- %.2f ^8(total life taken over time per second)", on(out, "LifeLossLostAvg")),
                        sf!("= %.2f per second", on(out, "netLifeRecoupAndLossLostOverTimeAvg")),
                    ]),
                );
            }
        }
    }

    // --- :3116-3162 pvp ------------------------------------------------------------------------------------------
    if cfg_truthy(input.get("PvpScaling")) {
        let pvp_t_value = match out.get("enemySkillTime") {
            None | Some(OutValue::Bool(false)) => 1.0,
            Some(OutValue::Num(v)) => *v,
            Some(other) => panic!("{}: enemySkillTime is {other:?}", site(3128)),
        };
        let pvp_multiplier = cfg_or(&[input.get("enemyMultiplierPvpDamage")], 100.0, 3119) / 100.0;
        let ne1 = misc("PvpNonElemental1");
        let ne2 = misc("PvpNonElemental2");
        let e1 = misc("PvpElemental1");
        let e2 = misc("PvpElemental2");
        let percentage_non_elemental = (on(out, "PhysicalTakenHit") + on(out, "ChaosTakenHit")) / on(out, "totalTakenHit");
        let percentage_elemental = 1.0 - percentage_non_elemental;
        let total = on(out, "totalTakenHit");
        // `^` = C `pow` (UCRT, = desktop LuaJIT; R2).
        let portion_non_elemental = (total / pvp_t_value / ne2).powf(ne1) * pvp_t_value * ne2 * percentage_non_elemental;
        let portion_elemental = (total / pvp_t_value / e2).powf(e1) * pvp_t_value * e2 * percentage_elemental;
        out.set_num("PvPTotalTakenHit", (portion_non_elemental + portion_elemental) * pvp_multiplier);
        // :3132-3161
        if let Some(bd) = bd.as_deref_mut() {
            let mut t = BTable::new();
            let percent_both = percentage_non_elemental > 0.0 && percentage_elemental > 0.0;
            t.push(sf!(
                "Pvp Formula is (D/(T*M))^E*T*%s, where D is the damage, T is the time taken,",
                if percent_both { "M*P" } else { "M" }
            ));
            t.push(sf!(
                " M is the multiplier%s",
                if percent_both {
                    ", E is the exponent and P is the percentage of that type (ele or non ele)"
                } else {
                    " and E is the exponent"
                }
            ));
            let tt = on(out, "totalTakenHit");
            if percent_both {
                t.push(sf!(
                    "(M= %.1f for ele and %.1f for non-ele)(E= %.2f for ele and %.2f for non-ele)",
                    e2,
                    ne2,
                    e1,
                    ne1
                ));
                t.push(sf!(
                    "(%.1f / (%.2f * %.1f)) ^ %.2f * %.2f * %.1f * %.2f = %.1f",
                    tt,
                    pvp_t_value,
                    ne2,
                    ne1,
                    pvp_t_value,
                    ne2,
                    percentage_non_elemental,
                    portion_non_elemental
                ));
                t.push(sf!(
                    "(%.1f / (%.2f * %.1f)) ^ %.2f * %.2f * %.1f * %.2f = %.1f",
                    tt,
                    pvp_t_value,
                    e2,
                    e1,
                    pvp_t_value,
                    e2,
                    percentage_elemental,
                    portion_elemental
                ));
                t.push(sf!(
                    "(portionNonElemental + portionElemental)%s",
                    if pvp_multiplier != 1.0 { " * PvP multiplier" } else { " " }
                ));
                if pvp_multiplier != 1.0 {
                    t.push(sf!("(%.1f + %.1f) * %g", portion_non_elemental, portion_elemental, pvp_multiplier));
                } else {
                    t.push(sf!("%.1f + %.1f", portion_non_elemental, portion_elemental));
                }
            } else if percentage_elemental <= 0.0 {
                t.push(sf!("(M= %.1f for non-ele)(E= %.2f for non-ele)", ne2, ne1));
                t.push(sf!(
                    "(%.1f / (%.2f * %.1f)) ^ %.2f * %.2f * %.1f = %.1f",
                    tt,
                    pvp_t_value,
                    ne2,
                    ne1,
                    pvp_t_value,
                    ne2,
                    portion_non_elemental
                ));
                if pvp_multiplier != 1.0 {
                    t.push(sf!(
                        "%.1f * %g ^8(portionNonElemental * PvP multiplier)",
                        portion_non_elemental,
                        pvp_multiplier
                    ));
                }
            } else if percentage_non_elemental <= 0.0 {
                t.push(sf!("(M= %.1f for ele)(E= %.2f for ele)", e2, e1));
                t.push(sf!(
                    "(%.1f / (%.2f * %.1f)) ^ %.2f * %.2f * %.1f = %.1f",
                    tt,
                    pvp_t_value,
                    e2,
                    e1,
                    pvp_t_value,
                    e2,
                    portion_elemental
                ));
                if pvp_multiplier != 1.0 {
                    t.push(sf!(
                        "%.1f * %g ^8(portionElemental * PvP multiplier)",
                        portion_elemental,
                        pvp_multiplier
                    ));
                }
            }
            t.push(sf!("= %.1f", on(out, "PvPTotalTakenHit")));
            bd.put("PvPTotalTakenHit", t);
        }
    }

    // --- :3164-3369 maximum hit taken ----------------------------------------------------------------------------
    {
        // :3166-3206 <T>TotalHitPool is REWRITTEN here.
        for (t, name) in DMG.iter().enumerate() {
            let hit_pool = format!("{name}TotalHitPool");
            // :3168-3177 ward (`Sum or 0` -- dead)
            let ward_bypass = base(&db, &["WardBypass"]);
            if ward_bypass > 0.0 {
                let pool_protected = on(out, "Ward") / (1.0 - ward_bypass / 100.0) * (ward_bypass / 100.0);
                let mut source_pool = on(out, &hit_pool);
                source_pool = max2(source_pool - pool_protected, 0.0) + min2(source_pool, pool_protected) / (ward_bypass / 100.0);
                out.set_num(&hit_pool, source_pool);
            } else {
                // :3176 `pool + output.Ward or 0` -- the `or 0` is dead.
                let v = on(out, &hit_pool) + on(out, "Ward");
                out.set_num(&hit_pool, v);
            }
            // :3178-3179 aegis -- `isElemental and AegisDisplay or 0` (L4)
            let aegis_display = if is_elemental(t) {
                or0(out, &format!("{name}AegisDisplay"), 3179)
            } else {
                0.0
            };
            let v = on(out, &hit_pool) + max2(max2(on(out, &format!("{name}Aegis")), on(out, "sharedAegis")), aegis_display);
            out.set_num(&hit_pool, v);
            // :3180-3190 `shared or 0 + typed or 0` = `shared or ((0 + typed) or 0)`: the shared rate is always a number.
            let guard_absorb_rate = match out.get("sharedGuardAbsorbRate") {
                Some(OutValue::Num(v)) => *v,
                None | Some(OutValue::Bool(false)) => 0.0 + first_num(out, false, &[&format!("{name}GuardAbsorbRate")], 3181),
                Some(other) => panic!("{}: sharedGuardAbsorbRate is {other:?}", site(3181)),
            };
            if guard_absorb_rate > 0.0 {
                let guard_absorb = match out.get("sharedGuardAbsorb") {
                    Some(OutValue::Num(v)) => *v,
                    None | Some(OutValue::Bool(false)) => 0.0 + first_num(out, false, &[&format!("{name}GuardAbsorb")], 3183),
                    Some(other) => panic!("{}: sharedGuardAbsorb is {other:?}", site(3183)),
                };
                if guard_absorb_rate >= 100.0 {
                    let v = on(out, &hit_pool) + guard_absorb;
                    out.set_num(&hit_pool, v);
                } else {
                    let pool_protected = guard_absorb / (guard_absorb_rate / 100.0) * (1.0 - guard_absorb_rate / 100.0);
                    let p = on(out, &hit_pool);
                    out.set_num(
                        &hit_pool,
                        max2(p - pool_protected, 0.0) + min2(p, pool_protected) / (1.0 - guard_absorb_rate / 100.0),
                    );
                }
            }
            // :3191-3205 undo the ally pool drains in reverse order
            for ally in ALLY_LIFE_POOL_LIST.iter().rev() {
                let (Some(life), Some(mitigation)) = (out.get(ally.life).cloned(), out.get(ally.mitigation).cloned()) else {
                    continue;
                };
                let life = match life {
                    OutValue::Num(v) => v,
                    OutValue::Bool(false) => continue,
                    other => panic!("{}: attempt to compare {other:?} with number", site(3196)),
                };
                if life <= 0.0 || life.is_nan() {
                    continue;
                }
                let mitigation = match mitigation {
                    OutValue::Num(v) => v,
                    OutValue::Bool(false) => continue,
                    other => panic!("{}: attempt to compare {other:?} with number", site(3196)),
                };
                // `mitigation > 0` is false for NaN too.
                if mitigation.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                    continue;
                }
                let mitigation = min2(mitigation, 100.0);
                if mitigation == 100.0 {
                    let v = on(out, &hit_pool) + life;
                    out.set_num(&hit_pool, v);
                } else {
                    let pool_protected = life / (mitigation / 100.0) * (1.0 - mitigation / 100.0);
                    let p = on(out, &hit_pool);
                    out.set_num(
                        &hit_pool,
                        max2(p - pool_protected, 0.0) + min2(p, pool_protected) / (1.0 - mitigation / 100.0),
                    );
                }
            }
        }

        // :3208-3355 the largest hit of each type the pools survive
        for (t, name) in DMG.iter().enumerate() {
            let mut part_min = f64::INFINITY;
            let mut use_conversion_smoothing = false;
            for (c, c_name) in DMG.iter().enumerate() {
                let convert_percent = need(shift[t][c], 3214, "damageShiftTable");
                let taken_flat = on(out, &format!("{c_name}takenFlat"));
                if convert_percent > 0.0 || taken_flat != 0.0 {
                    let hit_taken;
                    let effective_applied_armour = on(out, &format!("{c_name}EffectiveAppliedArmour"));
                    let damage_converted_multi = convert_percent / 100.0;
                    let total_hit_pool = on(out, &format!("{c_name}TotalHitPool"));
                    // :3219 `1 - (-0)` = 1 without Vaal Arctic Armour (O9)
                    let total_taken_multi =
                        on(out, &format!("{c_name}AfterReductionTakenHitMulti")) * (1.0 - on(out, "VaalArcticArmourMitigation"));
                    if damage_converted_multi <= 0.0 {
                        // :3220-3222
                        let taken_without_incoming = max2(taken_flat, 0.0) * total_taken_multi;
                        hit_taken = if taken_without_incoming >= total_hit_pool {
                            0.0
                        } else {
                            f64::INFINITY
                        };
                    } else if effective_applied_armour == 0.0 && convert_percent == 100.0 {
                        // :3223-3226
                        let total_resist_mult = on(out, &format!("{c_name}ResistTakenHitMulti"));
                        let dr_multi = total_resist_mult * (1.0 - on(out, &format!("{c_name}DamageReduction")) / 100.0);
                        hit_taken = max2(total_hit_pool / damage_converted_multi / dr_multi - taken_flat, 0.0) / total_taken_multi;
                    } else {
                        // :3227-3262 the armour quadratic
                        let total_resist_mult = on(out, &format!("{c_name}ResistTakenHitMulti"));
                        let reduction_percent = first_num(
                            out,
                            fl(&[&format!("SelfIgnoreBase{c_name}DamageReduction")]),
                            &[
                                &format!("Base{c_name}DamageReductionWhenHit"),
                                &format!("Base{c_name}DamageReduction"),
                            ],
                            3232,
                        );
                        let flat_dr = reduction_percent / 100.0;
                        let enemy_overwhelm_percent = first_num(
                            out,
                            fl(&[&format!("SelfIgnore{c_name}DamageReduction")]),
                            &[&format!("{c_name}EnemyOverwhelm")],
                            3252,
                        );
                        let resist_x_convert = total_resist_mult * damage_converted_multi;
                        let a = 5.0
                            * (1.0 - flat_dr + enemy_overwhelm_percent / 100.0)
                            * total_taken_multi
                            * resist_x_convert
                            * resist_x_convert;
                        let b = ((enemy_overwhelm_percent / 100.0 - flat_dr) * effective_applied_armour * total_taken_multi
                            - 5.0 * (total_hit_pool - taken_flat * total_taken_multi))
                            * resist_x_convert;
                        let c = -effective_applied_armour * (total_hit_pool - taken_flat * total_taken_multi);
                        // :3256 `a ~= 0 and <number> or m_huge`
                        let raw = if a != 0.0 {
                            (max2(b * b - 4.0 * a * c, 0.0).sqrt() - b) / (2.0 * a)
                        } else {
                            f64::INFINITY
                        };
                        // :3258-3261
                        let no_dr_max_hit = total_hit_pool / damage_converted_multi / total_resist_mult / total_taken_multi
                            * (1.0 - taken_flat * total_taken_multi / total_hit_pool);
                        let max_dr_max_hit = no_dr_max_hit / (1.0 - (on(out, "DamageReductionMax") - enemy_overwhelm_percent) / 100.0);
                        hit_taken = max2(min2(raw, max_dr_max_hit), no_dr_max_hit).floor();
                        use_conversion_smoothing = use_conversion_smoothing || convert_percent != 100.0;
                    }
                    part_min = min2(part_min, hit_taken);
                }
            }

            let enemy_damage_mult = on(out, &format!("{name}EnemyDamageMult"));
            let final_max_hit = if part_min == f64::INFINITY {
                f64::INFINITY
            } else if use_conversion_smoothing {
                // :3273-3302 converge on the hit whose overkill is ~0 (not covered by goldens)
                let mut pass_incoming_damage = part_min;
                let mut previous_overkill: Option<f64> = None;
                let passes = misc("maxHitSmoothingPasses");
                let mut n = 1.0;
                while n <= passes {
                    let actor = view(out, &db, &shift);
                    let (_, mut pass_damages) =
                        taken_hit_from_damage(pass_incoming_damage, dmg_type(t), &actor).unwrap_or_else(|e| panic!("{e}"));
                    let pass_pools = reduce_pools_by_damage(None, &mut pass_damages, &actor).unwrap_or_else(|e| panic!("{e}"));
                    let pass_overkill = pass_pools.overkill_damage - pass_pools.hit_pool_remaining;
                    let mut pass_ratio = 0.0;
                    // :3281 `pairs(passDamages)` (the table as reducePoolsByDamage left it) -- a max, dmgTypeList order.
                    for (p, p_name) in DMG.iter().enumerate() {
                        if pass_damages[p].is_none() {
                            continue;
                        }
                        let part_pool = or0(out, &format!("{p_name}TotalHitPool"), 3283);
                        if part_pool > 0.0 {
                            pass_ratio = max2(pass_ratio, (pass_overkill + part_pool) / part_pool);
                        }
                    }
                    if pass_ratio <= 0.0 {
                        pass_ratio = 1.0;
                    }
                    let mut step_size = 1.0;
                    if n > 1.0
                        && let Some(prev) = previous_overkill
                        && prev != 0.0
                        && !prev.is_nan()
                    {
                        step_size = min2(((pass_overkill - prev) / prev).abs(), 2.0);
                    }
                    // :3294 both values are numbers: plain conditionals.
                    let step_adjust = if step_size > 1.0 {
                        -pass_overkill / step_size
                    } else if n > 1.0 {
                        -pass_overkill * step_size
                    } else {
                        0.0
                    };
                    previous_overkill = Some(pass_overkill);
                    pass_incoming_damage = (pass_incoming_damage + step_adjust) / pass_ratio.sqrt();
                    if pass_overkill < 1.0 && pass_overkill > -1.0 {
                        break;
                    }
                    n += 1.0;
                }
                round(pass_incoming_damage / enemy_damage_mult, None)
            } else {
                round(part_min / enemy_damage_mult, None)
            };
            out.set_num(&format!("{name}MaximumHitTaken"), final_max_hit);
            // :3310-3354
            if let Some(bd) = bd.as_deref_mut() {
                let key = format!("{name}MaximumHitTaken");
                let mut tb = row_table(
                    Some("Maximum hit damage breakdown"),
                    &[
                        ("Type", "type"),
                        ("Pool", "pool"),
                        ("Incoming", "incoming"),
                        ("Multi", "multi"),
                        ("Taken", "taken"),
                    ],
                );
                let actor = view(out, &db, &shift);
                let (full_taken, mut taken_damages) =
                    taken_hit_from_damage(final_max_hit * enemy_damage_mult, dmg_type(t), &actor).unwrap_or_else(|e| panic!("{e}"));
                let full_taken = if full_taken.is_nan() { 0.0 } else { full_taken };
                // :3326 `pairs(takenDamages)` -- a string-keyed hash order (R13): dmgTypeList order here, the gate sorts the rows.
                for (c, c_name) in DMG.iter().enumerate() {
                    let Some(taken_amt) = taken_damages[c] else { continue };
                    let conversion = need(shift[t][c], 3327, "damageShiftTable");
                    let incoming = final_max_hit * enemy_damage_mult * conversion / 100.0;
                    let nan_to_zero = if taken_amt.is_nan() { 0.0 } else { taken_amt };
                    taken_damages[c] = Some(nan_to_zero);
                    tb.field_mut("rowList").push(row(&[
                        ("type", sf!("%d%% as %s", conversion, *c_name)),
                        ("pool", sf!("%d", on(out, &format!("{c_name}TotalHitPool")))),
                        ("incoming", sf!("%.0f", incoming)),
                        ("multi", sf!("x%.3f", nan_to_zero / incoming)),
                        ("taken", sf!("%.0f", nan_to_zero)),
                    ]));
                }
                let full_multi = full_taken / final_max_hit / enemy_damage_mult;
                tb.push("^8Maximum hit is calculated in reverse -");
                tb.push("^8from health pools, via damage reductions, to the max hit:");
                tb.push(sf!("%d ^8(used pool)", full_taken));
                if round(full_multi, Some(2.0)) != 1.0 {
                    tb.push(sf!("/ %.2f ^8(modifiers to damage taken)", full_multi));
                }
                if enemy_damage_mult != 1.0 {
                    tb.push(sf!("/ %.2f ^8(modifiers to enemy damage)", enemy_damage_mult));
                }
                let approx = if use_conversion_smoothing { " (approximate)" } else { "" };
                tb.push(sf!("= %.0f ^8maximum survivable enemy damage%s", final_max_hit, approx));
                tb.push("^8Such a hit would drain the following resources:");
                let pools = reduce_pools_by_damage(None, &mut taken_damages, &actor).unwrap_or_else(|e| panic!("{e}"));
                incoming_damage_breakdown(&mut tb, &pools, out);
                bd.put(key, tb);
            }
        }

        // :3357-3368
        let mut minimum = f64::INFINITY;
        let mut second_minimum = f64::INFINITY;
        for name in DMG {
            let max_hit = on(out, &format!("{name}MaximumHitTaken"));
            if max_hit < minimum {
                second_minimum = minimum;
                minimum = max_hit;
            } else if max_hit < second_minimum {
                second_minimum = max_hit;
            }
        }
        out.set_num("SecondMinimalMaximumHitTaken", second_minimum);
    }

    // --- :3371-3381 effective health pool vs dots ----------------------------------------------------------------
    for name in DMG {
        let v = on(out, &format!("{name}TotalPool")) / on(out, &format!("{name}TakenDotMult"));
        out.set_num(&format!("{name}DotEHP"), v);
        // :3374-3380
        if let Some(bd) = bd.as_deref_mut() {
            bd.put(
                format!("{name}DotEHP"),
                BTable::lines([
                    sf!("Total Pool: %d", on(out, &format!("{name}TotalPool"))),
                    sf!("Dot Damage Taken modifier: %.2f", on(out, &format!("{name}TakenDotMult"))),
                    sf!("Total Effective Dot Pool: %d", v),
                ]),
            );
        }
    }

    // --- :3383-3786 degens ---------------------------------------------------------------------------------------
    out.set_num("TotalBuildDegen", 0.0);
    for (t, name) in DMG.iter().enumerate() {
        let base_val = base(&db, &[&format!("{name}Degen")]);
        if base_val > 0.0 {
            // :3389 `pairs(damageOverTimeShiftTable[T])` -- hash-order float sums (D13); dmgTypeList order here.
            for (c, c_name) in DMG.iter().enumerate() {
                let Some(convert_percent) = dot_shift[t][c] else { continue };
                if convert_percent > 0.0 {
                    let total = base_val * (convert_percent / 100.0) * on(out, &format!("{c_name}TakenDotMult"));
                    let key = format!("{c_name}BuildDegen");
                    let v = or0(out, &key, 3392) + total;
                    out.set_num(&key, v);
                    let v = on(out, "TotalBuildDegen") + total;
                    out.set_num("TotalBuildDegen", v);
                    // :3394-3431
                    if let Some(bd) = bd.as_deref_mut() {
                        let mult = sf!("x %.2f", on(out, &format!("{c_name}TakenDotMult")));
                        bd.table_or("TotalBuildDegen", || {
                            let mut t = row_table(
                                None,
                                &[
                                    ("Base Type", "type"),
                                    ("Final Type", "type2"),
                                    ("Base", "base"),
                                    ("Taken As Percent", "conv"),
                                    ("Multiplier", "mult"),
                                    ("Total", "total"),
                                ],
                            );
                            t.fields.shift_remove("label");
                            t
                        })
                        .field_mut("rowList")
                        .push(row(&[
                            ("type", (*name).to_owned()),
                            ("type2", (*c_name).to_owned()),
                            ("base", sf!("%.1f", base_val)),
                            ("conv", sf!("x %.2f%%", convert_percent)),
                            ("mult", mult.clone()),
                            ("total", sf!("%.1f", total)),
                        ]));
                        bd.table_or(&key, || {
                            row_table(
                                None,
                                &[
                                    ("Base Type", "type"),
                                    ("Base", "base"),
                                    ("Taken As Percent", "conv"),
                                    ("Multiplier", "mult"),
                                    ("Total", "total"),
                                ],
                            )
                        })
                        .field_mut("rowList")
                        .push(row(&[
                            ("type", (*name).to_owned()),
                            ("base", sf!("%.1f", base_val)),
                            ("conv", sf!("x %.2f%%", convert_percent)),
                            ("mult", mult),
                            ("total", sf!("%.1f", total)),
                        ]));
                    }
                }
            }
        }
    }
    if out.opt_num("TotalBuildDegen") == Some(0.0) {
        // :3437 `= nil`
        out.remove("TotalBuildDegen");
    } else {
        // :3439-3512
        copy(out, "NetLifeRegen", "LifeRegenRecovery");
        copy(out, "NetManaRegen", "ManaRegenRecovery");
        copy(out, "NetEnergyShieldRegen", "EnergyShieldRegenRecovery");
        let (mut tl, mut tm, mut te) = (0.0, 0.0, 0.0);
        // :3445-3470
        let net_keys = [
            ("NetLifeRegen", "Total Life Degen"),
            ("NetManaRegen", "Total Mana Degen"),
            ("NetEnergyShieldRegen", "Total Energy Shield Degen"),
        ];
        if let Some(bd) = bd.as_deref_mut() {
            for (k, label) in net_keys {
                bd.put(k, row_table(Some(label), &[("Type", "type"), ("Degen", "degen")]));
            }
        }
        for (t, name) in DMG.iter().enumerate() {
            let key = format!("{name}BuildDegen");
            if truthy(out, &key) {
                let (l, m, e) = split_degen(out, &db, on(out, &key), t);
                tl += l;
                tm += m;
                te += e;
                // :3493-3506
                if let Some(bd) = bd.as_deref_mut() {
                    for ((k, _), d) in net_keys.iter().zip([l, m, e]) {
                        let r = row(&[("type", sf!("%s", *name)), ("degen", sf!("%.2f", d))]);
                        bd.table_mut(k).field_mut("rowList").push(r);
                    }
                }
            }
        }
        let v = on(out, "NetLifeRegen") - tl;
        out.set_num("NetLifeRegen", v);
        let v = on(out, "NetManaRegen") - tm;
        out.set_num("NetManaRegen", v);
        let v = on(out, "NetEnergyShieldRegen") - te;
        out.set_num("NetEnergyShieldRegen", v);
        let v = on(out, "NetLifeRegen") + on(out, "NetManaRegen") + on(out, "NetEnergyShieldRegen");
        out.set_num("TotalNetRegen", v);
        // :3513-3528
        if let Some(bd) = bd.as_deref_mut() {
            let lines = [
                ("NetLifeRegen", "LifeRegenRecovery", "life", tl),
                ("NetManaRegen", "ManaRegenRecovery", "mana", tm),
                ("NetEnergyShieldRegen", "EnergyShieldRegenRecovery", "energy shield", te),
            ];
            for (k, regen, what, total) in lines {
                let t = bd.table_mut(k);
                t.push(sf!("%.1f ^8(total %s regen)", on(out, regen), what));
                t.push(sf!("- %.1f ^8(total %s degen)", total, what));
                t.push(sf!("= %.1f", on(out, k)));
            }
            bd.put(
                "TotalNetRegen",
                BTable::lines([
                    sf!("Net Life Regen: %.1f", on(out, "NetLifeRegen")),
                    sf!("+ Net Mana Regen: %.1f", on(out, "NetManaRegen")),
                    sf!("+ Net Energy Shield Regen: %.1f", on(out, "NetEnergyShieldRegen")),
                    sf!("= Total Net Regen: %.1f", on(out, "TotalNetRegen")),
                ]),
            );
        }
    }
    // :3531-3544 the enemy's ailments on you
    let enemy_crit_ailment_effect =
        1.0 + or0(out, "EnemyCritChance", 3531) / 100.0 * 0.5 * (1.0 - on(out, "CritExtraDamageReduction") / 100.0);
    let enemy_crit_threshold = 10.1;
    let enemy_bleed_chance = 0.0;
    let mut enemy_ignite_chance = 0.0;
    let not_zero = |out: &Output, key: &str| out.get(key) != Some(&OutValue::Num(0.0));
    if not_zero(out, "SelfIgniteEffect") && on(out, "IgniteAvoidChance") < 100.0 && not_zero(out, "SelfIgniteDuration") && !dot {
        enemy_ignite_chance = base(&enemy_db, &["IgniteChance", "ElementalAilmentChance"]);
        // :3538
        let enemy_crit_ailment_chance = if !fl(&["CritsOnYouDontAlwaysApplyElementalAilments"]) {
            if on(out, "EnemyCritChance") > enemy_crit_threshold || enemy_ignite_chance > 0.0 {
                on(out, "EnemyCritChance")
            } else {
                0.0
            }
        } else {
            0.0
        };
        enemy_ignite_chance = (enemy_crit_ailment_chance + (1.0 - enemy_crit_ailment_chance / 100.0) * enemy_ignite_chance)
            * (1.0 - on(out, "IgniteAvoidChance") / 100.0);
    }
    let mut enemy_poison_chance = 0.0;
    if not_zero(out, "SelfPoisonEffect") && on(out, "PoisonAvoidChance") < 100.0 && not_zero(out, "SelfPoisonDuration") && !dot {
        enemy_poison_chance = base(&enemy_db, &["PoisonChance"]) * (1.0 - on(out, "PoisonAvoidChance") / 100.0);
    }
    if dot || (enemy_ignite_chance + enemy_poison_chance + enemy_bleed_chance) > 0.0 {
        out.set_num("TotalDegen", or0(out, "TotalBuildDegen", 3546));
        // `bd` / `source` / `ty` feed the rows of :3561-3602 / :3630-3671.
        let add_degen = |out: &mut Output,
                         mut bd: Option<&mut Breakdown>,
                         shift_row: &[Option<f64>; 5],
                         base_val: f64,
                         line: u32,
                         source: &str,
                         ty: &str| {
            for (c, c_name) in DMG.iter().enumerate() {
                let Some(convert_percent) = shift_row[c] else { continue };
                if convert_percent > 0.0 {
                    let total = base_val * (convert_percent / 100.0) * on(out, &format!("{c_name}TakenDotMult"));
                    let key = format!("{c_name}EnemyDegen");
                    let v = or0(out, &key, line) + total;
                    out.set_num(&key, v);
                    let v = on(out, "TotalDegen") + total;
                    out.set_num("TotalDegen", v);
                    if let Some(bd) = bd.as_deref_mut() {
                        let mult = sf!("x %.2f", on(out, &format!("{c_name}TakenDotMult")));
                        bd.table_or("TotalDegen", || {
                            let mut t = row_table(
                                None,
                                &[
                                    ("Source", "source"),
                                    ("Base Type", "type"),
                                    ("Final Type", "type2"),
                                    ("Base", "base"),
                                    ("Taken As Percent", "conv"),
                                    ("Multiplier", "mult"),
                                    ("Total", "total"),
                                ],
                            );
                            t.fields.shift_remove("label");
                            t
                        })
                        .field_mut("rowList")
                        .push(row(&[
                            ("source", source.to_owned()),
                            ("type", ty.to_owned()),
                            ("type2", (*c_name).to_owned()),
                            ("base", sf!("%.1f", base_val)),
                            ("conv", sf!("x %.2f%%", convert_percent)),
                            ("mult", mult.clone()),
                            ("total", sf!("%.1f", total)),
                        ]));
                        bd.table_or(&key, || {
                            row_table(
                                None,
                                &[
                                    ("Source", "source"),
                                    ("Base Type", "type"),
                                    ("Base", "base"),
                                    ("Taken As Percent", "conv"),
                                    ("Multiplier", "mult"),
                                    ("Total", "total"),
                                ],
                            )
                        })
                        .field_mut("rowList")
                        .push(row(&[
                            ("source", source.to_owned()),
                            ("type", ty.to_owned()),
                            ("base", sf!("%.1f", base_val)),
                            ("conv", sf!("x %.2f%%", convert_percent)),
                            ("mult", mult),
                            ("total", sf!("%.1f", total)),
                        ]));
                    }
                }
            }
        };
        if dot {
            // :3547-3606
            for (t, name) in DMG.iter().enumerate() {
                let cfg_val = tonumber(input.get(&format!("enemy{name}Damage")));
                let source = if cfg_val.is_none() { "Default" } else { "Config" };
                let base_val = cfg_val.unwrap_or_else(|| tonumber(placeholder.get(&format!("enemy{name}Damage"))).unwrap_or(0.0));
                if base_val > 0.0 {
                    // :3556 hash-order sum (D13): dmgTypeList order.
                    add_degen(out, bd.as_deref_mut(), &dot_shift[t], base_val, 3559, source, name);
                }
            }
        } else if (enemy_ignite_chance + enemy_poison_chance + enemy_bleed_chance) > 0.0 {
            // :3607-3675 `ailmentList` walked with `pairs` (:3618): fixed to Bleed, Ignite, Poison (D13).
            let mut ailments: Vec<(&str, usize, Vec<usize>)> = Vec::new();
            if enemy_bleed_chance > 0.0 {
                ailments.push(("Bleed", PHYSICAL, vec![PHYSICAL]));
            }
            if enemy_ignite_chance > 0.0 {
                let sources = if flag(&enemy_db, &["AllDamageIgnites"]) {
                    (0..5).collect()
                } else {
                    vec![FIRE]
                };
                ailments.push(("Ignite", FIRE, sources));
            }
            if enemy_poison_chance > 0.0 {
                ailments.push(("Poison", CHAOS, vec![PHYSICAL, CHAOS]));
            }
            for (source, damage_type, source_types) in ailments {
                let mut base_val = 0.0;
                for s in source_types {
                    base_val += on(out, &format!("{}TakenDamage", DMG[s]));
                }
                base_val = base_val
                    * misc(&format!("{source}PercentBase"))
                    * (enemy_crit_ailment_effect / on(out, "EnemyCritEffect"))
                    * on(out, &format!("Self{source}Effect"))
                    / 100.0;
                if base_val > 0.0 {
                    // :3625 hash-order sum (D13): dmgTypeList order.
                    add_degen(
                        out,
                        bd.as_deref_mut(),
                        &dot_shift[damage_type],
                        base_val,
                        3628,
                        source,
                        DMG[damage_type],
                    );
                }
            }
        }
        if out.opt_num("TotalDegen") == Some(or0(out, "TotalBuildDegen", 3677)) {
            // :3678 `= nil`
            out.remove("TotalDegen");
        } else {
            // :3680-3754
            copy(out, "ComprehensiveNetLifeRegen", "LifeRegenRecovery");
            copy(out, "ComprehensiveNetManaRegen", "ManaRegenRecovery");
            copy(out, "ComprehensiveNetEnergyShieldRegen", "EnergyShieldRegenRecovery");
            let (mut tl, mut tm, mut te) = (0.0, 0.0, 0.0);
            // :3686-3711
            let net_keys = [
                ("ComprehensiveNetLifeRegen", "Total Life Degen"),
                ("ComprehensiveNetManaRegen", "Total Mana Degen"),
                ("ComprehensiveNetEnergyShieldRegen", "Total Energy Shield Degen"),
            ];
            if let Some(bd) = bd.as_deref_mut() {
                for (k, label) in net_keys {
                    bd.put(k, row_table(Some(label), &[("Type", "type"), ("Degen", "degen")]));
                }
            }
            for (t, name) in DMG.iter().enumerate() {
                let type_degen = or0(out, &format!("{name}BuildDegen"), 3713) + or0(out, &format!("{name}EnemyDegen"), 3713);
                if type_degen != 0.0 {
                    let (l, m, e) = split_degen(out, &db, type_degen, t);
                    tl += l;
                    tm += m;
                    te += e;
                    // :3735-3748
                    if let Some(bd) = bd.as_deref_mut() {
                        for ((k, _), d) in net_keys.iter().zip([l, m, e]) {
                            let r = row(&[("type", sf!("%s", *name)), ("degen", sf!("%.2f", d))]);
                            bd.table_mut(k).field_mut("rowList").push(r);
                        }
                    }
                }
            }
            let v = on(out, "ComprehensiveNetLifeRegen") + or0(out, "LifeRecoupRecoveryAvg", 3751) - tl - or0(out, "LifeLossLostAvg", 3751);
            out.set_num("ComprehensiveNetLifeRegen", v);
            let v = on(out, "ComprehensiveNetManaRegen") + or0(out, "ManaRecoupRecoveryAvg", 3752) - tm;
            out.set_num("ComprehensiveNetManaRegen", v);
            let v = on(out, "ComprehensiveNetEnergyShieldRegen") + or0(out, "EnergyShieldRecoupRecoveryAvg", 3753) - te;
            out.set_num("ComprehensiveNetEnergyShieldRegen", v);
            let v =
                on(out, "ComprehensiveNetLifeRegen") + on(out, "ComprehensiveNetManaRegen") + on(out, "ComprehensiveNetEnergyShieldRegen");
            out.set_num("ComprehensiveTotalNetRegen", v);
            // :3755-3782
            if let Some(bd) = bd.as_deref_mut() {
                let t = bd.table_mut("ComprehensiveNetLifeRegen");
                t.push(sf!("%.1f ^8(total life regen)", on(out, "LifeRegenRecovery")));
                if or0(out, "LifeRecoupRecoveryAvg", 3757) != 0.0 {
                    t.push(sf!("+ %.1f ^8(average life recoup)", or0(out, "LifeRecoupRecoveryAvg", 3758)));
                }
                t.push(sf!("- %.1f ^8(total life degen)", tl));
                if or0(out, "LifeLossLostAvg", 3761) != 0.0 {
                    t.push(sf!("- %.1f ^8(average life lost over time)", or0(out, "LifeLossLostAvg", 3762)));
                }
                t.push(sf!("= %.1f", on(out, "ComprehensiveNetLifeRegen")));
                let t = bd.table_mut("ComprehensiveNetManaRegen");
                t.push(sf!("%.1f ^8(total mana regen)", on(out, "ManaRegenRecovery")));
                if or0(out, "ManaRecoupRecoveryAvg", 3766) != 0.0 {
                    t.push(sf!("+ %.1f ^8(average mana recoup)", or0(out, "ManaRecoupRecoveryAvg", 3767)));
                }
                t.push(sf!("- %.1f ^8(total mana degen)", tm));
                t.push(sf!("= %.1f", on(out, "ComprehensiveNetManaRegen")));
                let t = bd.table_mut("ComprehensiveNetEnergyShieldRegen");
                t.push(sf!("%.1f ^8(total energy shield regen)", on(out, "EnergyShieldRegenRecovery")));
                if or0(out, "EnergyShieldRecoupRecoveryAvg", 3772) != 0.0 {
                    t.push(sf!(
                        "+ %.1f ^8(average energy shield recoup)",
                        or0(out, "EnergyShieldRecoupRecoveryAvg", 3773)
                    ));
                }
                t.push(sf!("- %.1f ^8(total energy shield degen)", te));
                t.push(sf!("= %.1f", on(out, "ComprehensiveNetEnergyShieldRegen")));
                bd.put(
                    "ComprehensiveTotalNetRegen",
                    BTable::lines([
                        sf!("Net Life Regen: %.1f", on(out, "ComprehensiveNetLifeRegen")),
                        sf!("+ Net Mana Regen: %.1f", on(out, "ComprehensiveNetManaRegen")),
                        sf!("+ Net Energy Shield Regen: %.1f", on(out, "ComprehensiveNetEnergyShieldRegen")),
                        sf!("= Total Net Regen: %.1f", on(out, "ComprehensiveTotalNetRegen")),
                    ]),
                );
            }
        }
    }

    // :3788-3870 the breakdown of the incoming hit
    if let Some(bd) = bd {
        let mut taken_hit: DamageTable = [None; 5];
        for (t, name) in DMG.iter().enumerate() {
            let th = on(out, &format!("{name}TakenHit"));
            taken_hit[t] = if th > 0.0 { Some(th) } else { None };
            let r = row(&[
                ("type", sf!("%s", *name)),
                ("incoming", sf!("%.1f incoming damage", on(out, &format!("{name}TakenDamage")))),
                ("mult", sf!("x %.3f damage mult", on(out, &format!("{name}TakenHitMult")))),
                ("value", sf!("%d", th.ceil())),
            ]);
            bd.table_mut("totalTakenHit").field_mut("rowList").push(r);
        }
        let actor = view(out, &db, &shift);
        let pools = reduce_pools_by_damage(None, &mut taken_hit, &actor).unwrap_or_else(|e| panic!("{e}"));
        let tt = bd.table_mut("totalTakenHit");
        tt.push("Such a hit would drain the following resources:");
        incoming_damage_breakdown(tt, &pools, out);
        for (t, name) in DMG.iter().enumerate() {
            let lost = &pools.resources_lost[t];
            let mut sum = 0.0;
            let mut tb = BTable::lines([
                sf!("Final %s Damage taken:", *name),
                sf!("%.1f incoming damage", on(out, &format!("{name}TakenDamage"))),
                sf!("x %.3f damage mult", on(out, &format!("{name}TakenHitMult"))),
                sf!("= %.1f", on(out, &format!("{name}TakenHit"))),
            ]);
            tb.push("This part of the hit drains the following resources:");
            let gem = color_code("GEM");
            for (k, ally) in ALLY_LIFE_POOL_LIST.iter().enumerate() {
                if let Some(l) = lost.allies[k] {
                    sum += l;
                    tb.push(sf!(&format!("\t%d {gem}{}", ally.label), l));
                }
            }
            let mut line = |v: Option<f64>, fmt: String, with_type: bool, sum: &mut f64| {
                if let Some(v) = v {
                    *sum += v;
                    tb.push(if with_type { sf!(&fmt, v, *name) } else { sf!(&fmt, v) });
                }
            };
            line(lost.aegis, format!("\t%d {gem}%s Aegis charge"), true, &mut sum);
            line(
                lost.shared_elemental_aegis,
                format!("\t%d {gem}Elemental Aegis charge"),
                false,
                &mut sum,
            );
            line(lost.shared_aegis, format!("\t%d {gem}Shared Aegis charge"), false, &mut sum);
            let scourge = color_code("SCOURGE");
            line(lost.guard, format!("\n\t%d {scourge}%s Guard charge"), true, &mut sum);
            line(lost.shared_guard, format!("\t%d {scourge}Shared Guard charge"), false, &mut sum);
            line(lost.ward, format!("\t%d {}Ward", color_code("WARD")), false, &mut sum);
            line(
                lost.energy_shield,
                format!("\t%d {}Energy Shield", color_code("ES")),
                false,
                &mut sum,
            );
            line(lost.mana, format!("\t%d {}Mana", color_code("MANA")), false, &mut sum);
            let life = color_code("LIFE");
            line(
                lost.life_loss_prevented.filter(|v| *v > 0.0),
                format!("\t%d {life}Life ^7Loss Prevented"),
                false,
                &mut sum,
            );
            line(lost.life.filter(|v| *v > 0.0), format!("\t%d {life}Life"), false, &mut sum);
            line(
                lost.overkill,
                format!("\t%d {}Overkill damage", color_code("NEGATIVE")),
                false,
                &mut sum,
            );
            tb.push(sf!("\t^8(%d total)", sum));
            bd.put(format!("{name}TakenHit"), tb);
        }
    }
}

/// CalcDefence.lua:431-477 `incomingDamageBreakdown(breakdownTable, poolsRemaining, output)`.
fn incoming_damage_breakdown(t: &mut BTable, pools: &crate::pools::PoolsResult, out: &Output) {
    let gem = color_code("GEM");
    for (k, ally) in ALLY_LIFE_POOL_LIST.iter().enumerate() {
        if let Some(pool) = &pools.allies_taken_before_you[k] {
            let life = on(out, ally.life);
            t.push(sf!(
                &format!("\t%d {gem}{} ^7(%d remaining)", ally.label),
                life - pool.remaining,
                pool.remaining
            ));
        }
    }
    for (d, name) in DMG.iter().enumerate() {
        if let Some(a) = pools.resources_lost[d].aegis {
            let rem = need(pools.aegis.typed[d], 442, "Aegis[damageType]");
            t.push(sf!(&format!("\t%d {gem}%s Aegis charge ^7(%d remaining)"), a, *name, rem));
        }
    }
    if let Some(sea) = out.opt_num("sharedElementalAegis") {
        let rem = pools.aegis.shared_elemental;
        if Some(sea) != rem {
            let rem = need(rem, 446, "Aegis.sharedElemental");
            t.push(sf!(&format!("\t%d {gem}Elemental Aegis charge ^7(%d remaining)"), sea - rem, rem));
        }
    }
    if let Some(sa) = out.opt_num("sharedAegis")
        && sa > 0.0
    {
        let rem = need(pools.aegis.shared, 449, "Aegis.shared");
        t.push(sf!(&format!("\t%d {gem}Shared Aegis charge ^7(%d remaining)"), sa - rem, rem));
    }
    let scourge = color_code("SCOURGE");
    for (d, name) in DMG.iter().enumerate() {
        if let Some(g) = pools.resources_lost[d].guard {
            let rem = need(pools.guard.typed[d], 453, "Guard[damageType]");
            t.push(sf!(&format!("\n\t%d {scourge}%s Guard charge ^7(%d remaining)"), g, *name, rem));
        }
    }
    if let Some(sg) = out.opt_num("sharedGuardAbsorb")
        && sg > 0.0
    {
        let rem = need(pools.guard.shared, 457, "Guard.shared");
        t.push(sf!(&format!("\t%d {scourge}Shared Guard charge ^7(%d remaining)"), sg - rem, rem));
    }
    if let Some(w) = out.opt_num("Ward")
        && w > 0.0
    {
        t.push(sf!(&format!("\t%d {}Ward", color_code("WARD")), w));
    }
    if let Some(es) = out.opt_num("EnergyShieldRecoveryCap")
        && es != pools.energy_shield
        && es > 0.0
    {
        t.push(sf!(
            &format!("\t%d {}Energy Shield ^7(%d remaining)", color_code("ES")),
            es - pools.energy_shield,
            pools.energy_shield
        ));
    }
    if let Some(m) = out.opt_num("ManaUnreserved")
        && m != pools.mana
        && m > 0.0
    {
        t.push(sf!(
            &format!("\t%d {}Mana ^7(%d remaining)", color_code("MANA")),
            m - pools.mana,
            pools.mana
        ));
    }
    let life = color_code("LIFE");
    let prevented = pools.life_loss_lost_over_time + pools.life_below_half_loss_lost_over_time;
    if prevented > 0.0 {
        t.push(sf!(&format!("\t%d {life}Life ^7Loss Prevented"), prevented));
    }
    t.push(sf!(
        &format!("\t%d {life}Life ^7(%d remaining)"),
        on(out, "LifeRecoverable") - pools.life,
        pools.life
    ));
    if pools.overkill_damage >= 1.0 {
        t.push(sf!(
            &format!("\t%d {}Overkill damage", color_code("NEGATIVE")),
            pools.overkill_damage
        ));
    }
}
