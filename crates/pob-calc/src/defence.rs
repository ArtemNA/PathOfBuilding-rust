//! `calcs.defence(env, actor)` -- Modules/CalcDefence.lua:651-1671 -- with `calcs.resistances` (:511-649), the leaf helpers
//! `calcs.hitChance` (:57-64) / `calcs.armourReduction` (:75-78) and `calcs.actionSpeedMod` (CalcPerform.lua:1141-1151).
//! Executable spec: pob-ts `src/calc/defence.ts` (P4.4b); line numbers from `grep -n` on upstream `ed354c2f8`, 2026-10-31
//! (CALC-ARCH §8 Erratum: the doc's numbers after :1000 are shifted -- these are re-taken).
//!
//! Semantics that are easy to get wrong (pob-ts header, MINEFIELD L4/L26):
//!  * `A and B or C` falls through to `C` when `B` is nil/false; `Override` answers zero values on a miss (L26).
//!  * `Flag()` answers `true` or nothing, so `output.X = modDB:Flag(...)` leaves the key ABSENT, never `false`.
//!  * every `NewMod` here lands in `modDB` during the calculation and is read by the next `Sum` (Zealot's Oath feeds the
//!    next pass of the regen loop) -- nothing is reordered (L10).
//!
//! Breakdown (MINEFIELD-RUST R8): MAIN builds it (`env.buildBreakdown`, `env.player.breakdown` at CalcPerform.lua:1515-1518),
//! so all 35 `if breakdown then` branches of :511-1671 run upstream. Each was checked (R4.3 report): every one writes only
//! `breakdown.*` tables (directly, or via `breakdown.slot`/`multiChain`/`mod`, CalcBreakdown.lua:17-93, which read modDB and
//! `t_insert` into breakdown tables) -- none writes `output`, a condition or a mod, so none is ported.
//!
//! Numbers: `round`/`floor(x, dec)` = Common.lua (`pob_lua::number`), `m_min/m_max` = LuaJIT `minsd/maxsd` folds (L7),
//! `m_modf` = `pob_lua::number::modf`, `m_floor` = `f64::floor`, `^` = `f64::powf` (UCRT `pow`, = desktop LuaJIT, R2/O11).
//! Where upstream would do arithmetic on a nil, this panics with the Lua `file:line` (L18).
//!
//! `env.minion` is nil in this slice (no skill phase, CALC-ARCH §8), so the three minion branches (:665-676, :699) are dead;
//! they are written out as such. Branches no golden reaches (MINEFIELD D21 list) are ported and marked "not covered by goldens".

use std::rc::Rc;

use pob_build::ConfigValue;
use pob_data::rules::rules;
use pob_lua::number::{floor, max2, min2, modf, round};
use pob_mods::{Mod, ModCfg, ModType, StoreRef, Table, Tag, Value, create_mod};

use crate::calc_lib;
use crate::env::Env;
use crate::output::{OutValue, Output};
use crate::perform::{PerformState, Who};
use crate::setup::{misc, n, ok, s, tag};

pub use crate::pools::armour_reduction_f;

use ModType::{Base, Flag, Inc, Max, More, Override};

// --- CalcDefence.lua:24-30 module-local tables ----------------------------------------------------------------------

/// `:24` `isElemental`.
fn is_elemental(x: &str) -> bool {
    matches!(x, "Fire" | "Cold" | "Lightning")
}

/// `:28` `dmgTypeList`; `:30` `resistTypeList`.
const DMG_TYPE_LIST: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];
const RESIST_TYPE_LIST: [&str; 4] = ["Fire", "Cold", "Lightning", "Chaos"];

/// `:863` `pairs({"Helmet", ...})` over an array literal -- the array part, in order.
const DEFENCE_SLOTS: [&str; 6] = ["Helmet", "Gloves", "Boots", "Body Armour", "Weapon 2", "Weapon 3"];
/// `:755` the four block types.
const BLOCK_TYPES: [&str; 4] = [
    "BlockChance",
    "ProjectileBlockChance",
    "SpellBlockChance",
    "SpellProjectileBlockChance",
];
/// `:1255` the regen resources, in this order (the chain-breaker / Pious Path loops only look forward).
const REGEN_RESOURCES: [&str; 4] = ["Mana", "Life", "Energy Shield", "Rage"];
/// `:1406` `recoupTypeList`.
const RECOUP_TYPES: [&str; 3] = ["Life", "Mana", "EnergyShield"];

fn site(line: u32) -> String {
    format!("Modules/CalcDefence.lua:{line}")
}

// --- small readers (Lua errors stay errors, L18) --------------------------------------------------------------------

fn sum(db: &StoreRef, ty: ModType, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
    ok(db.borrow().sum(ty, cfg, names))
}

fn base(db: &StoreRef, names: &[&str]) -> f64 {
    sum(db, Base, None, names)
}

fn inc(db: &StoreRef, names: &[&str]) -> f64 {
    sum(db, Inc, None, names)
}

fn more_cfg(db: &StoreRef, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
    ok(db.borrow().more(cfg, names))
}

fn more(db: &StoreRef, names: &[&str]) -> f64 {
    more_cfg(db, None, names)
}

/// `calcLib.mod(db, cfg, ...)`.
fn cmod_cfg(db: &StoreRef, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
    ok(calc_lib::mod_(&db.borrow(), cfg, names))
}

fn cmod(db: &StoreRef, names: &[&str]) -> f64 {
    cmod_cfg(db, None, names)
}

/// `calcLib.val(db, name)`.
fn cval(db: &StoreRef, name: &str) -> f64 {
    ok(calc_lib::val(&db.borrow(), name, None))
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

/// A Lua value used in arithmetic: anything but a number is the error.
fn num_val(v: &Value, line: u32) -> f64 {
    v.as_num()
        .unwrap_or_else(|| panic!("{}: attempt to perform arithmetic on a {} value", site(line), v.type_name()))
}

/// `modDB:Override(nil, name) or fallback` used as a number (`Override`'s zero values adjust to nil inside `or`, L26).
fn or_override(v: Option<Value>, fallback: f64, line: u32) -> f64 {
    match v {
        Some(v) if v.truthy() => num_val(&v, line),
        _ => fallback,
    }
}

/// `name, isElemental[elem] and extra` as a Sum vararg: a nil name is simply a miss.
fn with_elem<'a>(name: &'a str, elem: &str, extra: &'a str) -> Vec<&'a str> {
    if is_elemental(elem) { vec![name, extra] } else { vec![name] }
}

#[allow(clippy::too_many_arguments)]
fn new_mod(db: &StoreRef, name: &str, ty: ModType, value: Value, source: Option<&str>, flags: u32, kw: u32, tags: Vec<Tag>) {
    db.borrow_mut().new_mod(create_mod(name, Some(ty), value, source, flags, kw, tags));
}

fn nm(db: &StoreRef, name: &str, ty: ModType, value: f64, source: Option<&str>) {
    new_mod(db, name, ty, n(value), source, 0, 0, vec![]);
}

/// `output[key] or fallback` as a number.
fn out_or(out: &Output, key: &str, fallback: f64, line: u32) -> f64 {
    match out.get(key) {
        None | Some(OutValue::Bool(false)) => fallback,
        Some(OutValue::Num(v)) => *v,
        Some(other) => panic!("{}: attempt to perform arithmetic on field '{key}' ({other:?})", site(line)),
    }
}

/// `output[key] = <config value>` -- nil removes the key.
fn set_config_value(out: &mut Output, key: &str, v: Option<&ConfigValue>) {
    match v {
        None => out.remove(key),
        Some(ConfigValue::Num(x)) => out.set_num(key, *x),
        Some(ConfigValue::Bool(b)) => out.set_bool(key, *b),
        Some(ConfigValue::Str(x)) => out.set_str(key, x),
    }
}

fn config_truthy(v: Option<&ConfigValue>) -> bool {
    !matches!(v, None | Some(ConfigValue::Bool(false)))
}

/// `actor.parent.output` -- the player has no parent: upstream indexes nil (only reached when the flag is set).
fn parent_output(line: u32) -> ! {
    panic!("{}: attempt to index field 'parent' (a nil value)", site(line))
}

/// `output[key] = actor.parent.output[key]` (nil removes the key).
fn from_parent(out: &mut Output, parent: Option<&Output>, key: &str, line: u32) {
    let p = parent.unwrap_or_else(|| parent_output(line));
    match p.get(key).cloned() {
        Some(crate::output::OutValue::Num(v)) => out.set_num(key, v),
        Some(crate::output::OutValue::Bool(b)) => out.set_bool(key, b),
        Some(crate::output::OutValue::Str(s)) => out.set_str(key, &s),
        None => out.remove(key),
    }
}

/// `actor.partyMembers.output[key]` -- PartyTab is P5; the stub's output table is empty (as perform's `party_output`).
fn party_output(_key: &str) -> Option<f64> {
    None
}

/// `output[key] = actor.partyMembers.output[key]`.
fn set_from_party(out: &mut Output, key: &str) {
    match party_output(key) {
        Some(v) => out.set_num(key, v),
        None => out.remove(key),
    }
}

/// `:769-772` / `:1163-1165` `if env.configInput.EHPUnluckyWorstOf and env.configInput.EHPUnluckyWorstOf ~= 1 then rolls =
/// -EHPUnluckyWorstOf / 2` -- `Some(rolls)` when the branch runs (not covered by goldens: the config is unset on all six).
fn ehp_unlucky_rolls(env: &Env, line: u32) -> Option<f64> {
    match env.config.input.get("EHPUnluckyWorstOf") {
        None | Some(ConfigValue::Bool(false)) => None,
        Some(ConfigValue::Num(v)) if *v == 1.0 => None,
        Some(ConfigValue::Num(v)) => Some(-*v / 2.0),
        // a string is `~= 1` and `-s` coerces it (Lua string arithmetic)
        Some(ConfigValue::Str(x)) => {
            let v = pob_lua::tonumber(x).unwrap_or_else(|| panic!("{}: attempt to perform arithmetic on a string value", site(line)));
            Some(-v / 2.0)
        }
        Some(ConfigValue::Bool(true)) => panic!("{}: attempt to perform arithmetic on a boolean value", site(line)),
    }
}

/// `p` after `rolls` lucky (> 0) / unlucky (< 0) rolls, or the Unexciting median -- `:773-783` and `:1166-1175`, where
/// `cur` is the current `output.Effective…` value (not covered by goldens: rolls are 0 on all six).
fn apply_rolls(db: &StoreRef, rolls: f64, cur: f64) -> f64 {
    let p = cur / 100.0;
    if flag(db, &["Unexciting"]) {
        // Unexciting rolls three times and keeps the median result -> 3p^2 - 2p^3
        (3.0 * p.powf(2.0) - 2.0 * p.powf(3.0)) * 100.0
    } else if rolls > 0.0 {
        (1.0 - (1.0 - p).powf(rolls + 1.0)) * 100.0
    } else {
        p.powf(rolls.abs()) * cur
    }
}

// --- CalcDefence.lua:57-78 hit chance and armour reduction ----------------------------------------------------------

/// `:57-64` `calcs.hitChance(evasion, accuracy)`. `hitChance(0, 0)` = `round(nan)` folded by `minsd/maxsd` = 100 (L7).
pub fn hit_chance(evasion: f64, accuracy: f64) -> f64 {
    if accuracy < 0.0 {
        return 5.0;
    }
    let raw_chance = accuracy / (accuracy + (evasion / 5.0).powf(0.9)) * 125.0;
    max2(min2(round(raw_chance, None), 100.0), 5.0)
}

/// `:75-78` `calcs.armourReduction` -- `round(calcs.armourReductionF(armour, raw))` (`armourReductionF` is `pools`').
pub fn armour_reduction(armour: f64, raw: f64) -> f64 {
    round(armour_reduction_f(armour, raw), None)
}

// --- CalcPerform.lua:1141-1151 `calcs.actionSpeedMod` ----------------------------------------------------------------

pub fn action_speed_mod(db: &StoreRef) -> f64 {
    let minimum_action_speed = max_(db, "MinimumActionSpeed").unwrap_or(0.0);
    let maximum_action_speed_reduction = max_(db, "MaximumActionSpeedReduction");
    let mut m = 1.0 + (max2(-misc("TemporalChainsEffectCap"), inc(db, &["TemporalChainsActionSpeed"])) + inc(db, &["ActionSpeed"])) / 100.0;
    m = max2(minimum_action_speed / 100.0, m);
    if let Some(r) = maximum_action_speed_reduction {
        m = min2((100.0 - r) / 100.0, m);
    }
    m
}

// --- CalcTools.lua:258-273 `calcLib.getConvertedModTags` (minionMods = nil) -- perform.rs has the same private helper ---

fn get_converted_mod_tags(m: &Mod, multiplier: f64) -> Vec<Tag> {
    m.tags
        .iter()
        .map(|t| {
            if t.get("limitTotal").truthy() {
                let mut tbl: Table = (*t.table()).clone();
                let limit = num_val(tbl.get("limit"), 266);
                tbl.hash.insert(Rc::from("limit"), n(limit * multiplier));
                Tag::new(tbl)
            } else {
                t.deep_copy()
            }
        })
        .collect()
}

// --- CalcDefence.lua:511-649 `calcs.resistances` ---------------------------------------------------------------------

pub fn resistances(db: &StoreRef, out: &mut Output) {
    // :526
    out.set_num("PhysicalResist", 0.0);

    // :529-546 maximum-resistance conversion (`maxRes` at most once per source; 0 is truthy, L4)
    for res_from in RESIST_TYPE_LIST {
        let mut max_res: Option<f64> = None;
        for res_to in RESIST_TYPE_LIST {
            let conversion_rate = base(db, &[&format!("{res_from}MaxResConvertTo{res_to}")]) / 100.0;
            if conversion_rate != 0.0 {
                if max_res.is_none() {
                    let mut m = 0.0;
                    let entries = ok(db.borrow().tabulate(Some(Base), None, &[&format!("{res_from}ResistMax")]));
                    for e in entries {
                        if e.mod_.source.as_deref() != Some("Base") {
                            m += num_val(&e.value, 538);
                        }
                    }
                    max_res = Some(m);
                }
                let m = max_res.unwrap_or(0.0);
                if m != 0.0 {
                    nm(
                        db,
                        &format!("{res_to}ResistMax"),
                        Base,
                        m * conversion_rate,
                        Some(&format!("{res_from} To {res_to} Max Resistance Conversion")),
                    );
                }
            }
        }
    }

    // :549-573 resistance conversion
    for res_from in RESIST_TYPE_LIST {
        let mut res: Option<f64> = None;
        for res_to in RESIST_TYPE_LIST {
            let conversion_rate = base(db, &[&format!("{res_from}ResConvertTo{res_to}")]) / 100.0;
            if conversion_rate != 0.0 {
                let from_name = format!("{res_from}Resist");
                let to_name = format!("{res_to}Resist");
                if res.is_none() {
                    let mut r = 0.0;
                    for e in ok(db.borrow().tabulate(Some(Base), None, &[&from_name])) {
                        if e.mod_.source.as_deref() != Some("Base") {
                            r += num_val(&e.value, 558);
                        }
                    }
                    res = Some(r);
                }
                let r = res.unwrap_or(0.0);
                if r != 0.0 {
                    nm(
                        db,
                        &to_name,
                        Base,
                        r * conversion_rate,
                        Some(&format!("{res_from} To {res_to} Resistance Conversion")),
                    );
                }
                for e in ok(db.borrow().tabulate(Some(Inc), None, &[&from_name])) {
                    nm(
                        db,
                        &to_name,
                        Inc,
                        num_val(&e.value, 566) * conversion_rate,
                        e.mod_.source.as_deref(),
                    );
                }
                for e in ok(db.borrow().tabulate(Some(More), None, &[&from_name])) {
                    nm(
                        db,
                        &to_name,
                        More,
                        num_val(&e.value, 569) * conversion_rate,
                        e.mod_.source.as_deref(),
                    );
                }
            }
        }
    }

    // :575-591 Melding of the Flesh (not covered by goldens)
    if flag(db, &["ElementalResistMaxIsHighestResistMax"]) {
        let mut highest_resist_max = 0.0;
        let mut highest_resist_max_type = "";
        for elem in RESIST_TYPE_LIST {
            let name = format!("{elem}ResistMax");
            let ov = override_(db, &name);
            let resist_max = match ov {
                Some(v) if v.truthy() => num_val(&v, 580),
                _ => min2(misc("MaxResistCap"), base(db, &with_elem(&name, elem, "ElementalResistMax"))),
            };
            if resist_max > highest_resist_max && is_elemental(elem) {
                highest_resist_max = resist_max;
                highest_resist_max_type = elem;
            }
        }
        for elem in RESIST_TYPE_LIST {
            if is_elemental(elem) {
                nm(
                    db,
                    &format!("{elem}ResistMax"),
                    Override,
                    highest_resist_max,
                    Some(&format!("{highest_resist_max_type} Melding of the Flesh")),
                );
            }
        }
    }

    // :593-649 per element
    for elem in RESIST_TYPE_LIST {
        let resist = format!("{elem}Resist");
        let resist_max = format!("{elem}ResistMax");
        let totem_resist = format!("Totem{elem}Resist");
        let totem_resist_max = format!("Totem{elem}ResistMax");
        // :595-599
        let mut min = misc("ResistFloor");
        let mut max = match override_(db, &resist_max) {
            Some(v) if v.truthy() => num_val(&v, 596),
            _ => min2(misc("MaxResistCap"), base(db, &with_elem(&resist_max, elem, "ElementalResistMax"))),
        };
        let mut total = match override_(db, &resist) {
            Some(v) if v.truthy() => Some(num_val(&v, 597)),
            _ => None,
        };
        let mut totem_max = match override_(db, &totem_resist_max) {
            Some(v) if v.truthy() => num_val(&v, 598),
            _ => min2(
                misc("MaxResistCap"),
                base(db, &with_elem(&totem_resist_max, elem, "TotemElementalResistMax")),
            ),
        };
        let mut totem_total = match override_(db, &totem_resist) {
            Some(v) if v.truthy() => Some(num_val(&v, 599)),
            _ => None,
        };
        // :600-607
        let mut dot_total: Option<f64> = None;
        if total.is_none() {
            let names = with_elem(&resist, elem, "ElementalResist");
            let b = base(db, &names);
            let i = max2(cmod(db, &names), 0.0);
            total = Some(b * i);
            let dot_cfg = ModCfg {
                flags: Some(f64::from(crate::setup::mod_flag("Dot"))),
                keyword_flags: Some(0.0),
                ..ModCfg::default()
            };
            let dot_base = sum(db, Base, Some(&dot_cfg), &names);
            dot_total = Some(dot_base * i);
        }
        // :608-611
        if totem_total.is_none() {
            let names = with_elem(&totem_resist, elem, "TotemElementalResist");
            let b = base(db, &names);
            totem_total = Some(b * max2(cmod(db, &names), 0.0));
        }
        let total_v = total.expect("set above");
        let totem_total_v = totem_total.expect("set above");

        // :613-619 fractional resistances are truncated (`-0` survives, O9)
        let total_t = modf(total_v).0;
        let dot_total_t = match dot_total {
            Some(d) => modf(d).0,
            None => total_t,
        };
        let totem_total_t = modf(totem_total_v).0;
        min = modf(min).0;
        max = modf(max).0;
        totem_max = modf(totem_max).0;

        // :621-623
        let fin = max2(min2(total_t, max), min);
        let dot_final = max2(min2(dot_total_t, max), min);
        let totem_final = max2(min2(totem_total_t, totem_max), min);

        // :626-635
        out.set_num(&resist, fin);
        out.set_num(&format!("{elem}ResistTotal"), total_t);
        out.set_num(&format!("{elem}ResistOverCap"), max2(0.0, total_t - max));
        out.set_num(&format!("{elem}ResistOver75"), max2(0.0, fin - 75.0));
        out.set_num(&format!("Missing{elem}Resist"), max2(0.0, max - fin));
        out.set_num(&format!("{elem}ResistOverTime"), dot_final);
        out.set_num(&totem_resist, totem_final);
        out.set_num(&format!("Totem{elem}ResistTotal"), totem_total_t);
        out.set_num(&format!("Totem{elem}ResistOverCap"), max2(0.0, totem_total_t - totem_max));
        out.set_num(&format!("MissingTotem{elem}Resist"), max2(0.0, totem_max - totem_final));
        // :635-648 breakdown only.
    }
}

// --- CalcDefence.lua:651-1671 `calcs.defence` ------------------------------------------------------------------------

/// `calcs.defence(env, actor)` as a perform phase ([`crate::PhaseFn`]).
#[allow(clippy::too_many_lines)]
pub fn defence(env: &mut Env, state: &mut PerformState, who: Who) {
    let (db, enemy_db, item_list) = {
        let a = crate::perform::actor_of(env, who).borrow();
        (a.mod_db.clone(), crate::perform::enemy_db_of(env, who), a.item_list.clone())
    };
    // the minion's state is a shared cell (`output.Minion`); `actor.parent` is the player (the enemy for a hostile minion), R7.10
    let minion_rc = (who == Who::Minion).then(|| state.minion.clone().expect("env.minion.output"));
    let hostile = who == Who::Minion && crate::perform::env_minion(env).hostile;
    let mut minion_guard = minion_rc.as_ref().map(|rc| rc.borrow_mut());
    let (actor_state, parent): (&mut crate::perform::ActorState, Option<&Output>) = match minion_guard.as_mut() {
        Some(g) => (&mut **g, Some(if hostile { &state.enemy.output } else { &state.player.output })),
        None => match who {
            Who::Player => (&mut state.player, None),
            _ => (&mut state.enemy, None),
        },
    };
    let fl = |names: &[&str]| flag(&db, names);
    let efl = |names: &[&str]| flag(&enemy_db, names);
    // :660 `local condList = modDB.conditions` -- never read.

    // --- :662-676 action speed, resistances, minion branches ------------------------------------------------------
    actor_state.output.set_num("ActionSpeedMod", action_speed_mod(&db));
    resistances(&db, &mut actor_state.output);
    // :666-671 `ResistanceAddedToMinions` -- the actor's final resistances into `env.minion.modDB`
    if let Some(m) = env.minion.clone() {
        let added = base(&db, &["ResistanceAddedToMinions"]);
        if added > 0.0 {
            let mdb = m.borrow().mod_db.clone();
            for elem in ["Fire", "Cold", "Lightning", "Chaos"] {
                let fin = actor_state.output.num(&format!("{elem}Resist"));
                mdb.borrow_mut().new_mod(pob_mods::create_mod(
                    &format!("{elem}Resist"),
                    Some(ModType::Base),
                    pob_mods::Value::Num((fin * added / 100.0).floor()),
                    Some("Player"),
                    0,
                    0,
                    Vec::new(),
                ));
            }
        }
    }
    // :673-676 Formless Inferno: `if actor == env.minion`
    if who == Who::Minion {
        crate::perform::do_actor_life_mana(&db, &mut actor_state.output);
        crate::perform::do_actor_life_mana_reservation(&db, actor_state, false);
    }
    let out = &mut actor_state.output;

    // --- :678-790 block -----------------------------------------------------------------------------------------------
    out.set_num("BlockChanceMax", min2(base(&db, &["BlockChanceMax"]), misc("BlockChanceCap")));
    if fl(&["MaximumBlockAttackChanceIsEqualToParent"]) {
        from_parent(out, parent, "BlockChanceMax", 681);
    } else if fl(&["MaximumBlockAttackChanceIsEqualToPartyMember"]) {
        set_from_party(out, "BlockChanceMax"); // not covered by goldens
    }
    out.set_num("BlockChanceOverCap", 0.0);
    out.set_num("SpellBlockChanceOverCap", 0.0);
    let mut base_block_chance = 0.0;
    // :688-694
    if let Some(ad) = item_list.get("Weapon 2").and_then(|i| i.armour_data.as_ref()) {
        base_block_chance += num_val(&ad.get("BlockChance"), 689);
    }
    if let Some(ad) = item_list.get("Weapon 3").and_then(|i| i.armour_data.as_ref()) {
        base_block_chance += num_val(&ad.get("BlockChance"), 693);
    }
    out.set_num("ShieldBlockChance", base_block_chance);
    // :696 `not keystonesAdded["Necromantic Aegis"] and Override(...) or baseBlockChance`
    if !env.keystones_added.get("Necromantic Aegis").copied().unwrap_or(false) {
        base_block_chance = or_override(override_(&db, "ReplaceShieldBlock"), base_block_chance, 696);
    }
    // :699 `actor == env.minion and env.keystonesAdded["Necromantic Aegis"] and env.player.modDB:Override(nil, "ReplaceShieldBlock") or baseBlockChance`
    if who == Who::Minion && env.keystones_added.get("Necromantic Aegis").copied().unwrap_or(false) {
        base_block_chance = or_override(override_(&env.mod_db, "ReplaceShieldBlock"), base_block_chance, 699);
    }

    if fl(&["BlockAttackChanceIsEqualToParent"]) {
        let p = parent.unwrap_or_else(|| parent_output(702));
        let v = p
            .opt_num("BlockChance")
            .unwrap_or_else(|| panic!("{}: bad argument #1 to 'min' (number expected, got nil)", site(702)));
        let m = out.num("BlockChanceMax");
        out.set_num("BlockChance", min2(v, m));
    } else if fl(&["BlockAttackChanceIsEqualToPartyMember"]) {
        // :704 `m_min(nil, ...)` with the empty party output -- not covered by goldens
        let v = party_output("BlockChance").unwrap_or_else(|| panic!("{}: bad argument #1 to 'min' (number expected, got nil)", site(704)));
        let m = out.num("BlockChanceMax");
        out.set_num("BlockChance", min2(v, m));
    } else if fl(&["MaxBlockIfNotBlockedRecently"]) {
        let m = out.num("BlockChanceMax");
        out.set_num("BlockChance", m);
    } else {
        let i = inc(&db, &["BlockChance"]);
        let m = more(&db, &["BlockChance"]);
        let total_block_chance = round((base_block_chance + base(&db, &["BlockChance"])) * (1.0 + i / 100.0) * m, None);
        let cap = out.num("BlockChanceMax");
        out.set_num("BlockChance", min2(total_block_chance, cap));
        out.set_num("BlockChanceOverCap", max2(0.0, total_block_chance - cap));
    }

    // :715
    let v = min2(
        out.num("BlockChance") + base(&db, &["ProjectileBlockChance"]) * cmod(&db, &["BlockChance"]),
        out.num("BlockChanceMax"),
    );
    out.set_num("ProjectileBlockChance", v);
    if fl(&["SpellBlockChanceMaxIsBlockChanceMax"]) {
        let m = out.num("BlockChanceMax");
        out.set_num("SpellBlockChanceMax", m);
    } else {
        out.set_num(
            "SpellBlockChanceMax",
            min2(base(&db, &["SpellBlockChanceMax"]), misc("BlockChanceCap")),
        );
    }
    if fl(&["MaxSpellBlockIfNotBlockedRecently"]) {
        let m = out.num("SpellBlockChanceMax");
        out.set_num("SpellBlockChance", m);
        out.set_num("SpellProjectileBlockChance", m);
    } else if fl(&["SpellBlockChanceIsBlockChance"]) {
        let (b, p, o) = (
            out.num("BlockChance"),
            out.num("ProjectileBlockChance"),
            out.num("BlockChanceOverCap"),
        );
        out.set_num("SpellBlockChance", b);
        out.set_num("SpellProjectileBlockChance", p);
        out.set_num("SpellBlockChanceOverCap", o);
    } else {
        // :729-734 D21: the INC/MORE read is `BlockChance`, not `SpellBlockChance` (upstream, verbatim).
        let i = inc(&db, &["BlockChance"]);
        let m = more(&db, &["BlockChance"]);
        let total_spell_block_chance = round(base(&db, &["SpellBlockChance"]) * (1.0 + i / 100.0) * m, None);
        let cap = out.num("SpellBlockChanceMax");
        out.set_num("SpellBlockChance", min2(total_spell_block_chance, cap));
        out.set_num("SpellBlockChanceOverCap", max2(0.0, total_spell_block_chance - cap));
        let v = max2(
            min2(
                out.num("SpellBlockChance") + base(&db, &["ProjectileSpellBlockChance"]) * cmod(&db, &["SpellBlockChance"]),
                cap,
            ),
            0.0,
        );
        out.set_num("SpellProjectileBlockChance", v);
    }
    // :736-746 breakdown only.
    if fl(&["CannotBlockAttacks"]) {
        out.set_num("BlockChance", 0.0);
        out.set_num("ProjectileBlockChance", 0.0);
    }
    if fl(&["CannotBlockSpells"]) {
        out.set_num("SpellBlockChance", 0.0);
        out.set_num("SpellProjectileBlockChance", 0.0);
    }
    // :755-784
    for block_type in BLOCK_TYPES {
        let eff = format!("Effective{block_type}");
        let b = out.num(block_type);
        // `env.mode_effective and m_max(...) or output[blockType]` -- m_max answers a number (truthy)
        let v = if env.mode_effective {
            max2(b - base(&enemy_db, &["reduceEnemyBlock"]), 0.0)
        } else {
            b
        };
        out.set_num(&eff, v);
        let mut block_rolls = 0.0;
        if env.mode_effective {
            if fl(&[&format!("{block_type}IsLucky")]) {
                block_rolls += 1.0; // not covered by goldens
            }
            if fl(&[&format!("{block_type}IsUnlucky")]) {
                block_rolls -= 1.0; // not covered by goldens
            }
            if fl(&["ExtremeLuck"]) {
                block_rolls *= 2.0; // not covered by goldens
            }
        }
        if let Some(r) = ehp_unlucky_rolls(env, 771) {
            block_rolls = r;
        }
        if block_rolls != 0.0 {
            let cur = out.num(&eff);
            out.set_num(&eff, apply_rolls(&db, block_rolls, cur));
        }
    }
    let avg = (out.num("EffectiveBlockChance")
        + out.num("EffectiveProjectileBlockChance")
        + out.num("EffectiveSpellBlockChance")
        + out.num("EffectiveSpellProjectileBlockChance"))
        / 4.0;
    out.set_num("EffectiveAverageBlockChance", avg);
    out.set_num("BlockEffect", 100.0 - base(&db, &["BlockEffect"]));
    if out.num("BlockEffect") != 0.0 {
        out.set_bool("ShowBlockEffect", true);
        let v = 100.0 - out.num("BlockEffect");
        out.set_num("DamageTakenOnBlock", v);
    }

    // --- :792-800 Armour and Energy Shield Mastery (not covered by goldens) ------------------------------------------
    if fl(&["ArmourAppliesToEnergyShieldRecharge"]) {
        let multiplier = max_(&db, "ImprovedArmourAppliesToEnergyShieldRecharge").unwrap_or(100.0) / 100.0;
        let entries = ok(db.borrow().tabulate(Some(Inc), None, &["Armour", "ArmourAndEvasion", "Defences"]));
        for e in entries {
            let m = &e.mod_;
            let modifiers = get_converted_mod_tags(m, multiplier);
            // :798 the RAW `mod.value` is scaled, not the evaluated one
            new_mod(
                &db,
                "EnergyShieldRecharge",
                Inc,
                n((num_val(&m.value, 798) * multiplier).floor()),
                m.source.as_deref(),
                m.flags,
                m.keyword_flags,
                modifiers,
            );
        }
    }

    // --- :802-843 six "defence increased by" flags: the FIRST Tabulate entry only ------------------------------------
    for (flag_name, target, from) in [
        ("ArmourIncreasedByUncappedFireRes", "Armour", "FireResistTotal"),
        ("ArmourIncreasedByOvercappedFireRes", "Armour", "FireResistOverCap"),
        ("EvasionRatingIncreasedByUncappedColdRes", "Evasion", "ColdResistTotal"),
        ("EvasionRatingIncreasedByOvercappedColdRes", "Evasion", "ColdResistOverCap"),
        (
            "EnergyShieldIncreasedByChanceToBlockSpellDamage",
            "EnergyShield",
            "SpellBlockChance",
        ),
        ("EnergyShieldIncreasedByChaosResistance", "EnergyShield", "ChaosResist"),
    ] {
        if fl(&[flag_name]) {
            let entries = ok(db.borrow().tabulate(Some(Flag), None, &[flag_name]));
            if let Some(e) = entries.first() {
                nm(&db, target, Inc, out.num(from), e.mod_.source.as_deref());
            }
        }
    }

    // --- :845-1135 primary defences ----------------------------------------------------------------------------------
    {
        let iron_reflexes = fl(&["IronReflexes"]);
        let mut ward = 0.0;
        let mut energy_shield = 0.0;
        let mut armour = 0.0;
        let mut evasion = 0.0;
        // :851-856 breakdown tables only.
        let mut gear_ward = 0.0;
        let mut gear_energy_shield = 0.0;
        let mut gear_armour = 0.0;
        let mut gear_evasion = 0.0;
        // :862 `wipeTable(tempTable1)`: a cfg holding only `slotName`
        let mut slot_cfg = ModCfg::default();
        for slot in DEFENCE_SLOTS {
            let Some(armour_data) = item_list.get(slot).and_then(|i| i.armour_data.as_ref()) else {
                continue;
            };
            slot_cfg.slot_name = Some(Rc::from(slot));
            let cfg = Some(&slot_cfg);
            // `not <flag> and armourData.X or 0`
            let field = |no: bool, key: &str, line: u32| -> f64 {
                if no {
                    return 0.0;
                }
                let v = armour_data.get(key);
                if v.truthy() { num_val(&v, line) } else { 0.0 }
            };
            let energy_shield_base = field(fl(&[&format!("GainNoEnergyShieldFrom{slot}")]), "EnergyShield", 867);
            let mut armour_base = field(fl(&[&format!("GainNoArmourFrom{slot}")]), "Armour", 868);
            // :869 Iron Reflexes: "no armour from X" also kills X's evasion
            let no_evasion = fl(&[&format!("GainNoEvasionFrom{slot}")]) || (fl(&[&format!("GainNoArmourFrom{slot}")]) && iron_reflexes);
            let mut evasion_base = field(no_evasion, "Evasion", 869);
            let mut ward_base = field(fl(&[&format!("GainNoWardFrom{slot}")]), "Ward", 870);
            // :871-878 (not covered by goldens)
            if slot == "Body Armour" && fl(&["ConvertBodyArmourArmourEvasionToWard"]) {
                let conversion = min2(base(&db, &["BodyArmourArmourEvasionToWardPercent"]) / 100.0, 1.0);
                let converted_armour = armour_base * conversion;
                let converted_evasion = evasion_base * conversion;
                armour_base -= converted_armour;
                evasion_base -= converted_evasion;
                ward_base += converted_evasion + converted_armour;
            }
            if ward_base > 0.0 {
                if fl(&["EnergyShieldToWard"]) {
                    // not covered by goldens
                    let i = sum(&db, Inc, cfg, &["Ward", "Defences", "EnergyShield"]);
                    let m = more_cfg(&db, cfg, &["Ward", "Defences"]);
                    ward += ward_base * (1.0 + i / 100.0) * m;
                    gear_ward += ward_base;
                } else {
                    ward += ward_base * cmod_cfg(&db, cfg, &["Ward", "Defences"]);
                    gear_ward += ward_base;
                }
            }
            if energy_shield_base > 0.0 {
                if fl(&["EnergyShieldToWard"]) {
                    // not covered by goldens
                    let m = more_cfg(&db, cfg, &["EnergyShield", "Defences"]);
                    energy_shield += energy_shield_base * m;
                    gear_energy_shield += energy_shield_base;
                } else if !fl(&["ConvertArmourESToLife"]) {
                    // D21: with ConvertArmourESToLife the slot's ES goes nowhere (not covered by goldens)
                    energy_shield += energy_shield_base * cmod_cfg(&db, cfg, &["EnergyShield", "Defences", &format!("{slot}ESAndArmour")]);
                    gear_energy_shield += energy_shield_base;
                }
            }
            if armour_base > 0.0 {
                armour += armour_base * cmod_cfg(&db, cfg, &["Armour", "ArmourAndEvasion", "Defences", &format!("{slot}ESAndArmour")]);
                gear_armour += armour_base;
            }
            if evasion_base > 0.0 {
                gear_evasion += evasion_base;
                if iron_reflexes {
                    // not covered by goldens
                    armour += evasion_base * cmod_cfg(&db, cfg, &["Armour", "Evasion", "ArmourAndEvasion", "Defences"]);
                } else {
                    evasion += evasion_base * cmod_cfg(&db, cfg, &["Evasion", "ArmourAndEvasion", "Defences"]);
                }
            }
        }
        // :945-968 global ward
        let ward_base = base(&db, &["Ward"]);
        if ward_base > 0.0 {
            if fl(&["EnergyShieldToWard"]) {
                let i = inc(&db, &["Ward", "Defences", "EnergyShield"]);
                let m = more(&db, &["Ward", "Defences"]);
                ward += ward_base * (1.0 + i / 100.0) * m;
            } else {
                ward += ward_base * cmod(&db, &["Ward", "Defences"]);
            }
        }
        // :969-988 global energy shield
        let energy_shield_base = base(&db, &["EnergyShield"]);
        if energy_shield_base > 0.0 {
            if fl(&["EnergyShieldToWard"]) {
                energy_shield += energy_shield_base * more(&db, &["EnergyShield", "Defences"]);
            } else {
                energy_shield += energy_shield_base * cmod(&db, &["EnergyShield", "Defences"]);
            }
        }
        // :989-995 global armour
        let armour_base = base(&db, &["Armour", "ArmourAndEvasion"]);
        if armour_base > 0.0 {
            armour += armour_base * cmod(&db, &["Armour", "ArmourAndEvasion", "Defences"]);
        }
        // :996-1009 global evasion
        let evasion_base = base(&db, &["Evasion", "ArmourAndEvasion"]);
        if evasion_base > 0.0 {
            if iron_reflexes {
                armour += evasion_base * cmod(&db, &["Armour", "Evasion", "ArmourAndEvasion", "Defences"]);
            } else {
                evasion += evasion_base * cmod(&db, &["Evasion", "ArmourAndEvasion", "Defences"]);
            }
        }
        // :1010-1018 mana -> armour (the "2 *" is upstream's)
        let conv_mana_to_armour = base(&db, &["ManaConvertToArmour"]);
        if conv_mana_to_armour > 0.0 {
            let armour_base = 2.0 * base(&db, &["Mana"]) * conv_mana_to_armour / 100.0;
            let total = armour_base * cmod(&db, &["Mana", "Armour", "ArmourAndEvasion", "Defences"]);
            armour += total;
        }
        // :1019-1026 mana -> energy shield
        let conv_mana_to_es = base(&db, &["ManaGainAsEnergyShield"]);
        if conv_mana_to_es > 0.0 {
            let energy_shield_base = base(&db, &["Mana"]) * conv_mana_to_es / 100.0;
            energy_shield += energy_shield_base * cmod(&db, &["Mana", "EnergyShield", "Defences"]);
        }
        // :1027-1040 life -> armour (Chaos Inoculation pins it at 1; not covered by goldens)
        let conv_life_to_armour = base(&db, &["LifeGainAsArmour"]);
        if conv_life_to_armour > 0.0 {
            let armour_base = base(&db, &["Life"]) * conv_life_to_armour / 100.0;
            let total = if fl(&["ChaosInoculation"]) {
                1.0
            } else {
                armour_base * cmod(&db, &["Life", "Armour", "ArmourAndEvasion", "Defences"])
            };
            armour += total;
        }
        // :1041-1054 life -> energy shield
        let conv_life_to_es = base(&db, &["LifeConvertToEnergyShield", "LifeGainAsEnergyShield"]);
        if conv_life_to_es > 0.0 {
            let energy_shield_base = base(&db, &["Life"]) * conv_life_to_es / 100.0;
            let total = if fl(&["ChaosInoculation"]) {
                1.0
            } else {
                energy_shield_base * cmod(&db, &["Life", "EnergyShield", "Defences"])
            };
            energy_shield += total;
        }
        // :1055-1063 evasion -> armour (gear evasion counts too)
        let conv_evasion_to_armour = base(&db, &["EvasionGainAsArmour"]);
        if conv_evasion_to_armour > 0.0 {
            let armour_base = (base(&db, &["Evasion", "ArmourAndEvasion"]) + gear_evasion) * conv_evasion_to_armour / 100.0;
            let total = armour_base * cmod(&db, &["Evasion", "Armour", "ArmourAndEvasion", "Defences"]);
            armour += total;
        }
        // :1064-1074
        let es = or_override(override_(&db, "EnergyShield"), max2(round(energy_shield, None), 0.0), 1064);
        out.set_num("EnergyShield", es);
        out.set_num("Armour", max2(round(armour, None), 0.0));
        out.set_num("Evasion", max2(round(evasion, None), 0.0));
        out.set_num("MeleeEvasion", max2(round(evasion * cmod(&db, &["MeleeEvasion"]), None), 0.0));
        out.set_num(
            "ProjectileEvasion",
            max2(round(evasion * cmod(&db, &["ProjectileEvasion"]), None), 0.0),
        );
        let (a, e) = (out.num("Armour"), out.num("Evasion"));
        out.set_num("LowestOfArmourAndEvasion", min2(a, e));
        out.set_num("Ward", max2(ward.floor(), 0.0));
        out.set_num("Gear:Ward", gear_ward);
        out.set_num("Gear:EnergyShield", gear_energy_shield);
        out.set_num("Gear:Armour", gear_armour);
        out.set_num("Gear:Evasion", gear_evasion);

        // :1075 `A and Armour < ES or B and Evasion < ES or configInput.conditionLowEnergyShield`: true, or the config value
        let es = out.num("EnergyShield");
        let low_es = env.config.input.get("conditionLowEnergyShield");
        let capping = (fl(&["ArmourESRecoveryCap"]) && a < es) || (fl(&["EvasionESRecoveryCap"]) && e < es);
        if capping {
            out.set_bool("CappingES", true);
        } else {
            set_config_value(out, "CappingES", low_es);
        }

        if capping || config_truthy(low_es) {
            // :1078 the tightest active cap, else the ES pool (numbers are truthy)
            let cap = if fl(&["ArmourESRecoveryCap"]) && fl(&["EvasionESRecoveryCap"]) {
                min2(a, e)
            } else if fl(&["ArmourESRecoveryCap"]) {
                a
            } else if fl(&["EvasionESRecoveryCap"]) {
                e
            } else {
                es
            };
            out.set_num("EnergyShieldRecoveryCap", cap);
            // :1079 (not covered by goldens: the config is unset)
            if config_truthy(low_es) {
                out.set_num("EnergyShieldRecoveryCap", min2(es * misc("LowPoolThreshold"), cap));
            }
        } else {
            out.set_num("EnergyShieldRecoveryCap", es);
        }

        // :1083-1134 evade chance
        if fl(&["CannotEvade"]) || efl(&["CannotBeEvaded"]) {
            out.set_num("EvadeChance", 0.0);
            out.set_num("MeleeEvadeChance", 0.0);
            out.set_num("ProjectileEvadeChance", 0.0);
        } else if fl(&["AlwaysEvade"]) {
            out.set_num("EvadeChance", 100.0);
            out.set_num("MeleeEvadeChance", 100.0);
            out.set_num("ProjectileEvadeChance", 100.0);
        } else {
            let enemy_accuracy = round(cval(&enemy_db, "Accuracy"), None);
            let evade_chance = base(&db, &["EvadeChance"]);
            // :1095 the enemy's own chance-to-hit multiplier (shadows `calcs.hitChance` upstream)
            let hit_chance_mod = cmod(&enemy_db, &["HitChance"]);
            let mut evade_stat = out.num("Evasion");
            let mut melee_evade_stat = out.num("MeleeEvasion");
            let mut projectile_evade_stat = out.num("ProjectileEvasion");
            if fl(&["EvadeChanceBasedOnWard"]) {
                // :1100 the Override is divided straight away -- a miss is a nil arithmetic error (not covered by goldens)
                let ov = override_(&db, "EvadeChanceBasedOnWardPercent").unwrap_or(Value::Nil);
                let multiplier = num_val(&ov, 1100) / 100.0;
                evade_stat = out.num("Ward") * multiplier;
                melee_evade_stat = evade_stat;
                projectile_evade_stat = evade_stat;
            }
            out.set_num(
                "EvadeChance",
                100.0 - (hit_chance(evade_stat, enemy_accuracy) - evade_chance) * hit_chance_mod,
            );
            let cap = misc("EvadeChanceCap");
            let melee = max2(
                0.0,
                min2(
                    cap,
                    (100.0 - (hit_chance(melee_evade_stat, enemy_accuracy) - evade_chance) * hit_chance_mod)
                        * cmod(&db, &["EvadeChance", "MeleeEvadeChance"]),
                ),
            );
            out.set_num("MeleeEvadeChance", melee);
            let projectile = max2(
                0.0,
                min2(
                    cap,
                    (100.0 - (hit_chance(projectile_evade_stat, enemy_accuracy) - evade_chance) * hit_chance_mod)
                        * cmod(&db, &["EvadeChance", "ProjectileEvadeChance"]),
                ),
            );
            out.set_num("ProjectileEvadeChance", projectile);
            // :1109-1114
            if melee != projectile {
                out.set_bool("splitEvade", true);
            } else {
                out.set_num("EvadeChance", melee);
                out.set_bool("noSplitEvade", true);
            }
            // :1115-1133 breakdown only.
        }
    }

    // --- :1137-1178 spell suppression ---------------------------------------------------------------------------------
    let spell_suppression_chance = base(&db, &["SpellSuppressionChance"]);
    let total_spell_suppression_chance = or_override(override_(&db, "SpellSuppressionChance"), spell_suppression_chance, 1138);
    // :1142-1144 Acrobatics converts the UNCAPPED base (not covered by goldens)
    if fl(&["ConvertSpellSuppressionToSpellDodge"]) {
        nm(&db, "SpellDodgeChance", Base, spell_suppression_chance / 2.0, Some("Acrobatics"));
    }
    let cap = misc("SuppressionChanceCap");
    out.set_num("SpellSuppressionChance", min2(total_spell_suppression_chance, cap));
    out.set_num(
        "SpellSuppressionEffect",
        max2(misc("SuppressionEffect") + base(&db, &["SpellSuppressionEffect"]), 0.0),
    );
    let v = if efl(&["CannotBeSuppressed"]) {
        0.0
    } else {
        out.num("SpellSuppressionChance")
    };
    out.set_num("EffectiveSpellSuppressionChance", v);
    let mut suppress_rolls = 0.0;
    if env.mode_effective {
        if fl(&["SpellSuppressionChanceIsLucky"]) {
            suppress_rolls += 1.0; // not covered by goldens
        }
        if fl(&["SpellSuppressionChanceIsUnlucky"]) {
            suppress_rolls -= 1.0; // not covered by goldens
        }
        if fl(&["ExtremeLuck"]) {
            suppress_rolls *= 2.0; // not covered by goldens
        }
    }
    if let Some(r) = ehp_unlucky_rolls(env, 1164) {
        suppress_rolls = r;
    }
    if suppress_rolls != 0.0 {
        let cur = out.num("EffectiveSpellSuppressionChance");
        out.set_num("EffectiveSpellSuppressionChance", apply_rolls(&db, suppress_rolls, cur));
    }
    out.set_num("SpellSuppressionChanceOverCap", max2(0.0, total_spell_suppression_chance - cap));

    // --- :1180-1209 dodge ----------------------------------------------------------------------------------------------
    let total_attack_dodge_chance = base(&db, &["AttackDodgeChance"]);
    let total_spell_dodge_chance = base(&db, &["SpellDodgeChance"]);
    let attack_dodge_chance_max = misc("DodgeChanceCap");
    let spell_dodge_chance_max = or_override(override_(&db, "SpellDodgeChanceMax"), base(&db, &["SpellDodgeChanceMax"]), 1184);
    // :1185 `Sum(...) or 0` -- Sum always answers a number: the `or 0` is dead (D21)
    let enemy_reduce_dodge_chance = base(&enemy_db, &["reduceEnemyDodge"]);
    out.set_num("AttackDodgeChance", min2(total_attack_dodge_chance, attack_dodge_chance_max));
    let v = if efl(&["CannotBeDodged"]) {
        0.0
    } else {
        min2(
            max2(total_attack_dodge_chance - enemy_reduce_dodge_chance, 0.0),
            attack_dodge_chance_max,
        )
    };
    out.set_num("EffectiveAttackDodgeChance", v);
    out.set_num("SpellDodgeChance", min2(total_spell_dodge_chance, spell_dodge_chance_max));
    let v = if efl(&["CannotBeDodged"]) {
        0.0
    } else {
        min2(
            max2(total_spell_dodge_chance - enemy_reduce_dodge_chance, 0.0),
            spell_dodge_chance_max,
        )
    };
    out.set_num("EffectiveSpellDodgeChance", v);
    if env.mode_effective && fl(&["DodgeChanceIsUnlucky"]) {
        // not covered by goldens
        let a = out.num("EffectiveAttackDodgeChance");
        out.set_num("EffectiveAttackDodgeChance", a / 100.0 * a);
        let sp = out.num("EffectiveSpellDodgeChance");
        out.set_num("EffectiveSpellDodgeChance", sp / 100.0 * sp);
    }
    out.set_num(
        "AttackDodgeChanceOverCap",
        max2(0.0, total_attack_dodge_chance - attack_dodge_chance_max),
    );
    out.set_num(
        "SpellDodgeChanceOverCap",
        max2(0.0, total_spell_dodge_chance - spell_dodge_chance_max),
    );
    // :1198-1209 breakdown only.

    // --- :1211-1217 recovery-rate modifiers ----------------------------------------------------------------------------
    out.set_num("LifeRecoveryRateMod", 1.0);
    if !fl(&["CannotRecoverLifeOutsideLeech"]) {
        out.set_num("LifeRecoveryRateMod", cmod(&db, &["LifeRecoveryRate"]));
    }
    out.set_num("ManaRecoveryRateMod", cmod(&db, &["ManaRecoveryRate"]));
    out.set_num("EnergyShieldRecoveryRateMod", cmod(&db, &["EnergyShieldRecoveryRate"]));

    // --- :1219-1252 leech caps -----------------------------------------------------------------------------------------
    let life = out.num("Life");
    out.set_num("MaxLifeLeechInstance", life * cval(&db, "MaxLifeLeechInstance") / 100.0);
    out.set_num("MaxLifeLeechRatePercent", cval(&db, "MaxLifeLeechRate"));
    if fl(&["MaximumLifeLeechIsEqualToParent"]) {
        from_parent(out, parent, "MaxLifeLeechRatePercent", 1223);
    } else if fl(&["MaximumLifeLeechIsEqualToPartyMember"]) {
        set_from_party(out, "MaxLifeLeechRatePercent"); // not covered by goldens
    }
    let v = out.num("Life") * out.num("MaxLifeLeechRatePercent") / 100.0;
    out.set_num("MaxLifeLeechRate", v);
    // :1228-1234 breakdown only.
    let es = out.num("EnergyShield");
    out.set_num(
        "MaxEnergyShieldLeechInstance",
        es * cval(&db, "MaxEnergyShieldLeechInstance") / 100.0,
    );
    out.set_num("MaxEnergyShieldLeechRate", es * cval(&db, "MaxEnergyShieldLeechRate") / 100.0);
    // :1237-1243 breakdown only.
    let mana = out.num("Mana");
    out.set_num("MaxManaLeechInstance", mana * cval(&db, "MaxManaLeechInstance") / 100.0);
    out.set_num("MaxManaLeechRate", mana * cval(&db, "MaxManaLeechRate") / 100.0);
    // :1246-1252 breakdown only.

    // --- :1254-1340 regeneration, degeneration, recovery ----------------------------------------------------------------
    for (i, resource_name) in REGEN_RESOURCES.iter().enumerate() {
        let resource = resource_name.replace(' ', "");
        let r = resource.as_str();
        // :1258 `output[resource] or 0`
        let pool = out_or(out, r, 0.0, 1258);
        let mut i_ = inc(&db, &[&format!("{r}Regen")]);
        let m = more(&db, &[&format!("{r}Regen")]);
        let mut regen_rate = 0.0;
        // :1264 `output[resource.."RecoveryRateMod"] or 1` -- Rage has none
        let recovery_rate_mod = out_or(out, &format!("{r}RecoveryRateMod"), 1.0, 1264);
        if fl(&[&format!("No{r}Regen")]) || fl(&[&format!("CannotGain{r}")]) {
            out.set_num(&format!("{r}Regen"), 0.0);
        } else if r == "Life" && fl(&["ZealotsOath"]) {
            // :1267-1276 Zealot's Oath (not covered by goldens): ES comes later in this loop and picks the mods up
            out.set_num("LifeRegen", 0.0);
            let life_base = base(&db, &["LifeRegen"]);
            if life_base > 0.0 {
                nm(&db, "EnergyShieldRegen", Base, life_base, Some("Zealot's Oath"));
            }
            let life_percent = base(&db, &["LifeRegenPercent"]);
            if life_percent > 0.0 {
                nm(&db, "EnergyShieldRegenPercent", Base, life_percent, Some("Zealot's Oath"));
            }
        } else {
            // :1278-1285 legacy Chain Breaker (not covered by goldens)
            if i_ != 0.0 {
                for other_name in &REGEN_RESOURCES[i + 1..] {
                    let other = other_name.replace(' ', "");
                    if fl(&[&format!("{r}RegenTo{other}Regen")]) {
                        nm(
                            &db,
                            &format!("{other}Regen"),
                            Inc,
                            i_,
                            Some(&format!("{resource_name} instead applies to {other_name}")),
                        );
                        i_ = 0.0;
                    }
                }
            }
            // :1286-1292 (not covered by goldens)
            if r == "Life" && base(&db, &["LifeRegenAppliesToEnergyShield"]) > 0.0 {
                let conversion = min2(base(&db, &["LifeRegenAppliesToEnergyShield"]), 100.0) / 100.0;
                let life_base = base(&db, &["LifeRegen"]);
                let life_percent = base(&db, &["LifeRegenPercent"]);
                nm(
                    &db,
                    "EnergyShieldRegen",
                    Base,
                    floor(life_base * conversion, Some(2.0)),
                    Some("Life Regen to ES Regen"),
                );
                nm(
                    &db,
                    "EnergyShieldRegenPercent",
                    Base,
                    floor(life_percent * conversion, Some(2.0)),
                    Some("Life Regen to ES Regen"),
                );
            }
            let base_regen = base(&db, &[&format!("{r}Regen")]) + pool * base(&db, &[&format!("{r}RegenPercent")]) / 100.0;
            let regen = base_regen * (1.0 + i_ / 100.0) * m;
            // :1295-1301 Pious Path (not covered by goldens)
            if regen != 0.0 {
                for other_name in &REGEN_RESOURCES[i + 1..] {
                    let other = other_name.replace(' ', "");
                    if fl(&[&format!("{r}RegenerationRecovers{other}")]) {
                        nm(
                            &db,
                            &format!("{other}Recovery"),
                            Base,
                            regen,
                            Some(&format!("{resource_name} Regeneration Recovers {other_name}")),
                        );
                    }
                }
            }
            regen_rate = round(regen * recovery_rate_mod, Some(1.0));
            out.set_num(&format!("{r}Regen"), regen_rate);
        }
        out.set_num(&format!("{r}RegenInc"), i_);
        let mut base_degen = base(&db, &[&format!("{r}Degen")]) + (pool * base(&db, &[&format!("{r}DegenPercent")]) / 100.0);
        let tincture_degen_percent = base(&db, &[&format!("{r}DegenPercentTincture")]);
        // :1308 tincture minimum 1 degen per stack
        base_degen += max2(pool * tincture_degen_percent / 100.0, tincture_degen_percent);
        let degen_rate = if base_degen > 0.0 {
            base_degen * cmod(&db, &[&format!("{r}Degen")])
        } else {
            0.0
        };
        out.set_num(&format!("{r}Degen"), degen_rate);
        let recovery_rate = base(&db, &[&format!("{r}Recovery")]) * recovery_rate_mod;
        out.set_num(&format!("{r}Recovery"), recovery_rate);
        let regen_part = if fl(&[&format!("UnaffectedBy{r}Regen")]) { 0.0 } else { regen_rate };
        let regen_recovery = regen_part - degen_rate + recovery_rate;
        out.set_num(&format!("{r}RegenRecovery"), regen_recovery);
        if regen_recovery > 0.0 {
            new_mod(
                &db,
                &format!("Condition:CanGain{r}"),
                Flag,
                Value::Bool(true),
                Some(&format!("{resource_name}Regen")),
                0,
                0,
                vec![],
            );
        }
        let pct = if pool > 0.0 {
            round(regen_recovery / pool * 100.0, Some(1.0))
        } else {
            0.0
        };
        out.set_num(&format!("{r}RegenPercent"), pct);
        // :1318-1338 breakdown only.
    }

    // --- :1342-1401 energy shield recharge ------------------------------------------------------------------------------
    // :1343 `Flag and not Flag`: nil (absent) when the first Flag misses
    let applies_to_life = if fl(&["EnergyShieldRechargeAppliesToLife"]) {
        let v = !fl(&["CannotRecoverLifeOutsideLeech"]);
        out.set_bool("EnergyShieldRechargeAppliesToLife", v);
        v
    } else {
        out.remove("EnergyShieldRechargeAppliesToLife");
        false
    };
    let applies_to_es = !(fl(&["NoEnergyShieldRecharge"]) || fl(&["CannotGainEnergyShield"]) || applies_to_life);
    out.set_bool("EnergyShieldRechargeAppliesToEnergyShield", applies_to_es);
    if applies_to_life || applies_to_es {
        let i = inc(&db, &["EnergyShieldRecharge"]);
        let m = more(&db, &["EnergyShieldRecharge"]);
        let b = or_override(override_(&db, "EnergyShieldRecharge"), misc("EnergyShieldRechargeBase"), 1349);
        if applies_to_life {
            // not covered by goldens
            let recharge = out.num("Life") * b * (1.0 + i / 100.0) * m;
            let v = round(recharge * out.num("LifeRecoveryRateMod"), None);
            out.set_num("LifeRecharge", v);
        } else {
            let recharge = out.num("EnergyShield") * b * (1.0 + i / 100.0) * m;
            let v = round(recharge * out.num("EnergyShieldRecoveryRateMod"), None);
            out.set_num("EnergyShieldRecharge", v);
        }
        out.set_num(
            "EnergyShieldRechargeDelay",
            misc("EnergyShieldRechargeDelay") / (1.0 + inc(&db, &["EnergyShieldRechargeFaster"]) / 100.0),
        );
        // :1353-1398 breakdown only.
    } else {
        out.set_num("EnergyShieldRecharge", 0.0);
    }

    // --- :1403-1520 recoup ----------------------------------------------------------------------------------------------
    {
        out.set_num("anyRecoup", 0.0);
        let recoup_inc = 1.0 + inc(&db, &["RecoupRecoveryAmount"]) / 100.0;
        let recoup_more = more(&db, &["RecoupRecoveryAmount"]);
        let first_flag_source = |add_flag: &str, line: u32| -> Option<pob_mods::Str> {
            let entries = ok(db.borrow().tabulate(Some(Flag), None, &[add_flag]));
            let e = entries
                .first()
                .unwrap_or_else(|| panic!("{}: attempt to index a nil value", site(line)));
            e.mod_.source.clone()
        };
        // :1409-1421 "<X> recoup also applies to <Y>" (not covered by goldens)
        for recoup_type in RECOUP_TYPES {
            for recoup_type2 in RECOUP_TYPES {
                if recoup_type != recoup_type2 {
                    let add_flag = format!("Add{recoup_type}RecoupTo{recoup_type2}Recoup");
                    if fl(&[&add_flag]) {
                        let base_recoup = base(&db, &[&format!("{recoup_type}Recoup")]);
                        let src = first_flag_source(&add_flag, 1416);
                        nm(&db, &format!("{recoup_type2}Recoup"), Base, base_recoup, src.as_deref());
                    }
                }
            }
        }
        // :1422-1448
        for recoup_type in RECOUP_TYPES {
            if recoup_type == "Life" && fl(&["EnergyShieldRecoupInsteadOfLife"]) {
                // not covered by goldens
                out.set_num("LifeRecoup", 0.0);
                let life_recoup = base(&db, &["LifeRecoup"]);
                nm(&db, "EnergyShieldRecoup", Base, life_recoup, Some("Life Recoup Conversion"));
            } else {
                let base_recoup = base(&db, &[&format!("{recoup_type}Recoup")]);
                let v = base_recoup * out.num(&format!("{recoup_type}RecoveryRateMod")) * recoup_inc * recoup_more;
                out.set_num(&format!("{recoup_type}Recoup"), v);
                let any = out.num("anyRecoup") + v;
                out.set_num("anyRecoup", any);
                // :1431-1446 breakdown only.
            }
        }
        // :1449-1454 Absorption Charges (not covered by goldens)
        if fl(&["UsePowerCharges"]) && fl(&["PowerChargesConvertToAbsorptionCharges"]) {
            let per = base(&db, &["PerAbsorptionElementalEnergyShieldRecoup"]);
            for elem in ["Cold", "Fire", "Lightning"] {
                new_mod(
                    &db,
                    &format!("{elem}EnergyShieldRecoup"),
                    Base,
                    n(per),
                    Some("Absorption Charges"),
                    0,
                    0,
                    vec![tag(&[("type", s("Multiplier")), ("var", s("AbsorptionCharge"))])],
                );
            }
        }
        // :1456-1490 per damage type
        for recoup_type in RECOUP_TYPES {
            for damage_type in DMG_TYPE_LIST {
                let recoup_name = format!("{damage_type}{recoup_type}Recoup");
                let recoup = base(&db, &[&recoup_name]);
                if recoup_type == "Life" && fl(&["EnergyShieldRecoupInsteadOfLife"]) {
                    // not covered by goldens
                    out.set_num(&recoup_name, 0.0);
                    nm(
                        &db,
                        &format!("{damage_type}EnergyShieldRecoup"),
                        Base,
                        recoup,
                        Some("Life Recoup Conversion"),
                    );
                } else {
                    let v = recoup * out.num(&format!("{recoup_type}RecoveryRateMod")) * recoup_inc * recoup_more;
                    out.set_num(&recoup_name, v);
                    let any = out.num("anyRecoup") + v;
                    out.set_num("anyRecoup", any);
                    // :1466-1477 breakdown only.
                }
                for recoup_type2 in RECOUP_TYPES {
                    if recoup_type != recoup_type2 {
                        let add_flag = format!("Add{recoup_type}RecoupTo{recoup_type2}Recoup");
                        if fl(&[&add_flag]) {
                            // not covered by goldens
                            let src = first_flag_source(&add_flag, 1484);
                            db.borrow_mut().replace_mod(create_mod(
                                &format!("{damage_type}{recoup_type2}Recoup"),
                                Some(Base),
                                n(recoup),
                                src.as_deref(),
                                0,
                                0,
                                vec![],
                            ));
                        }
                    }
                }
            }
        }
        // :1491-1519 pseudo recoup (not covered by goldens)
        for resource in RECOUP_TYPES {
            if !fl(&[&format!("No{resource}Regen")]) && !fl(&[&format!("CannotGain{resource}")]) {
                let key = format!("PhysicalDamageMitigated{resource}PseudoRecoup");
                let pseudo = base(&db, &[&key]);
                if pseudo > 0.0 {
                    let dur_key = format!("{key}Duration");
                    out.set_num(&dur_key, base(&db, &[&dur_key]));
                    if out.num(&dur_key) == 0.0 {
                        out.set_num(&dur_key, 4.0);
                    }
                    let i = inc(&db, &[&format!("{resource}Regen")]);
                    let m = more(&db, &[&format!("{resource}Regen")]);
                    let v = pseudo * (1.0 + i / 100.0) * m * out.num(&format!("{resource}RecoveryRateMod"));
                    out.set_num(&key, v);
                    let any = out.num("anyRecoup") + v;
                    out.set_num("anyRecoup", any);
                    // :1504-1515 breakdown only.
                }
            }
        }
    }

    // --- :1522-1532 ward recharge ---------------------------------------------------------------------------------------
    out.set_num(
        "WardRechargeDelay",
        misc("WardRechargeDelay") / (1.0 + inc(&db, &["WardRechargeFaster"]) / 100.0),
    );
    // :1524-1531 breakdown only.

    // --- :1534-1540 damage reduction ------------------------------------------------------------------------------------
    out.set_num(
        "DamageReductionMax",
        or_override(override_(&db, "DamageReductionMax"), misc("DamageReductionCap"), 1535),
    );
    // :1536 a source-less mod
    nm(&db, "ArmourAppliesToPhysicalDamageTaken", Base, 100.0, None);
    for damage_type in DMG_TYPE_LIST {
        let dr = format!("{damage_type}DamageReduction");
        let dr_max = out.num("DamageReductionMax");
        let v = match override_(&db, &dr) {
            Some(v) if v.truthy() => num_val(&v, 1538),
            _ => min2(
                max2(0.0, base(&db, &with_elem(&dr, damage_type, "ElementalDamageReduction"))),
                dr_max,
            ),
        };
        out.set_num(&format!("Base{dr}"), v);
        let v = match override_(&db, &dr) {
            Some(v) if v.truthy() => num_val(&v, 1539),
            _ => min2(
                max2(0.0, out.num(&format!("Base{dr}")) + base(&db, &[&format!("{dr}WhenHit")])),
                dr_max,
            ),
        };
        out.set_num(&format!("Base{dr}WhenHit"), v);
    }

    // --- :1542-1559 movement speed, blind -------------------------------------------------------------------------------
    let msm = match override_(&db, "MovementSpeed") {
        Some(v) if v.truthy() => num_val(&v, 1543),
        // `Flag and partyMembers.output.MovementSpeedMod or calcLib.mod(...)` -- the empty party output answers nil
        _ => match fl(&["MovementSpeedEqualHighestLinkedPlayers"])
            .then(|| party_output("MovementSpeedMod"))
            .flatten()
        {
            Some(v) => v,
            None => cmod(&db, &["MovementSpeed"]),
        },
    };
    out.set_num("MovementSpeedMod", msm);
    if fl(&["MovementSpeedCannotBeBelowBase"]) {
        let v = max2(out.num("MovementSpeedMod"), 1.0);
        out.set_num("MovementSpeedMod", v);
    }
    let v = out.num("MovementSpeedMod") * out.num("ActionSpeedMod");
    out.set_num("EffectiveMovementSpeedMod", v);
    // :1548-1555 breakdown only.
    if efl(&["Blind"]) {
        out.set_num("BlindEffectMod", cmod(&enemy_db, &["BlindEffect", "BuffEffectOnSelf"]) * 100.0);
    }

    // --- :1561-1573 recovery on block -----------------------------------------------------------------------------------
    out.set_num("LifeOnBlock", 0.0);
    out.set_num("LifeOnSuppress", 0.0);
    if !fl(&["CannotRecoverLifeOutsideLeech"]) {
        out.set_num("LifeOnBlock", base(&db, &["LifeOnBlock"]));
        out.set_num("LifeOnSuppress", base(&db, &["LifeOnSuppress"]));
    }
    out.set_num("ManaOnBlock", base(&db, &["ManaOnBlock"]));
    out.set_num("EnergyShieldOnBlock", base(&db, &["EnergyShieldOnBlock"]));
    out.set_num("EnergyShieldOnSpellBlock", base(&db, &["EnergyShieldOnSpellBlock"]));
    out.set_num("EnergyShieldOnSuppress", base(&db, &["EnergyShieldOnSuppress"]));

    // --- :1575-1593 avoidances / immunities -----------------------------------------------------------------------------
    out.set_bool("specificTypeAvoidance", false);
    let avoid_cap = misc("AvoidChanceCap");
    for damage_type in DMG_TYPE_LIST {
        let key = format!("Avoid{damage_type}DamageChance");
        let v = min2(base(&db, &[&key]), avoid_cap);
        out.set_num(&key, v);
        if v > 0.0 {
            out.set_bool("specificTypeAvoidance", true);
        }
    }
    out.set_num("AvoidProjectilesChance", min2(base(&db, &["AvoidProjectilesChance"]), avoid_cap));
    out.set_num(
        "AvoidAllDamageFromHitsChance",
        min2(base(&db, &["AvoidAllDamageFromHitsChance"]), avoid_cap),
    );
    let v = if fl(&["BlindImmune"]) {
        100.0
    } else {
        min2(base(&db, &["AvoidBlind"]), 100.0)
    };
    out.set_num("BlindAvoidChance", v);
    let v = if fl(&["ImpaleImmune"]) {
        100.0
    } else {
        min2(base(&db, &["AvoidImpale"]), 100.0)
    };
    out.set_num("ImpaleAvoidChance", v);
    // :1590-1593 `Flag()` answers true or nothing -- absent without the mod
    for (key, name) in [
        ("CorruptedBloodImmunity", "CorruptedBloodImmune"),
        ("MaimImmunity", "MaimImmune"),
        ("HinderImmunity", "HinderImmune"),
        ("KnockbackImmunity", "KnockbackImmune"),
    ] {
        if fl(&[name]) {
            out.set_bool(key, true);
        } else {
            out.remove(key);
        }
    }

    // :1595-1599 Ancestral Vision (not covered by goldens); `Sum(...) or 0` -- dead `or` (D21)
    if fl(&["SpellSuppressionAppliesToAilmentAvoidance"]) {
        let pct = base(&db, &["SpellSuppressionAppliesToAilmentAvoidancePercent"]) / 100.0;
        nm(
            &db,
            "AvoidElementalAilments",
            Base,
            (pct * spell_suppression_chance).floor(),
            Some("Ancestral Vision"),
        );
    }
    // :1600-1607 Foulborn Ancestral Vision (not covered by goldens)
    if fl(&["SpellSuppressionAppliesToChanceToDefendWithArmour"]) {
        let percent = max_(&db, "SpellSuppressionAppliesToChanceToDefendWithArmourPercent").unwrap_or(0.0);
        let percent_armour = max_(&db, "SpellSuppressionAppliesToChanceToDefendWithArmourPercentArmour").unwrap_or(0.0);
        let cond = |var: &str, neg: bool| {
            let mut p = vec![("type", s("Condition")), ("var", s(var))];
            if neg {
                p.push(("neg", Value::Bool(true)));
            }
            tag(&p)
        };
        new_mod(
            &db,
            "ArmourDefense",
            Max,
            n(percent_armour - 100.0),
            Some("Chance to Defend from Spell Suppression: Max Calc"),
            0,
            0,
            vec![cond("ArmourMax", false)],
        );
        new_mod(
            &db,
            "ArmourDefense",
            Max,
            n(min2(percent * spell_suppression_chance / 100.0, 1.0) * (percent_armour - 100.0)),
            Some("Chance to Defend from Spell Suppression: Average Calc"),
            0,
            0,
            vec![cond("ArmourAvg", false)],
        );
        // :1606 D21: `modSource` is an undeclared global -- always nil, so always the literal
        new_mod(
            &db,
            "ArmourDefense",
            Max,
            n(min2((percent * spell_suppression_chance / 100.0).floor(), 1.0) * (percent_armour - 100.0)),
            Some("Chance to Defend from Spell Suppression: Min Calc"),
            0,
            0,
            vec![cond("ArmourMax", true), cond("ArmourAvg", true)],
        );
    }
    let armour_defense = max_(&db, "ArmourDefense").unwrap_or(0.0) / 100.0;
    out.set_num("ArmourDefense", armour_defense);
    // :1609 `... or nil`: absent, not 0
    if armour_defense > 0.0 {
        out.set_num("RawArmourDefense", (1.0 + armour_defense) * 100.0);
    } else {
        out.remove("RawArmourDefense");
    }

    // :1611-1617 "only used for breakdown purposes" -- but the mod is created unconditionally (not covered by goldens)
    if fl(&["ShockAvoidAppliesToElementalAilments"]) {
        let b = base(&db, &["AvoidShock"]);
        if b != 0.0 {
            nm(&db, "AvoidShockAppliesToElementalAilments", Base, b, Some("Stormshroud"));
        }
    }

    // :1619-1625 ailment avoidance
    let rules = rules();
    for ailment in &rules.non_elemental_ailment_type_list {
        let v = if fl(&[&format!("{ailment}Immune")]) {
            100.0
        } else {
            min2(base(&db, &[&format!("Avoid{ailment}"), "AvoidAilments"]), 100.0).floor()
        };
        out.set_num(&format!("{ailment}AvoidChance"), v);
    }
    for ailment in &rules.elemental_ailment_type_list {
        let shock_avoid_applies_to_all = fl(&["ShockAvoidAppliesToElementalAilments"]) && ailment != "Shock";
        let v = if fl(&[&format!("{ailment}Immune"), "ElementalAilmentImmune"]) {
            100.0
        } else {
            let extra = if shock_avoid_applies_to_all {
                base(&db, &["AvoidShock"])
            } else {
                0.0
            };
            min2(
                base(&db, &[&format!("Avoid{ailment}"), "AvoidAilments", "AvoidElementalAilments"]) + extra,
                100.0,
            )
            .floor()
        };
        out.set_num(&format!("{ailment}AvoidChance"), v);
    }

    // --- :1627-1643 curses, light radius, effect-on-self, debuff expiration ---------------------------------------------
    let v = if fl(&["CurseImmune"]) {
        100.0
    } else {
        min2(base(&db, &["AvoidCurse"]), 100.0)
    };
    out.set_num("CurseAvoidChance", v);
    let v = if fl(&["SilenceImmune"]) {
        100.0
    } else {
        out.num("CurseAvoidChance")
    };
    out.set_num("SilenceAvoidChance", v);
    out.set_num("CritExtraDamageReduction", min2(base(&db, &["ReduceCritExtraDamage"]), 100.0));
    out.set_num("LightRadiusMod", cmod(&db, &["LightRadius"]));
    out.set_num("LightRadiusInc", max2(inc(&db, &["LightRadius"]), 0.0));
    // :1632-1634 breakdown only.
    out.set_num(
        "CurseEffectOnSelf",
        max2(more(&db, &["CurseEffectOnSelf"]) * (100.0 + inc(&db, &["CurseEffectOnSelf"])), 0.0),
    );
    out.set_num(
        "ExposureEffectOnSelf",
        more(&db, &["ExposureEffectOnSelf"]) * (100.0 + inc(&db, &["ExposureEffectOnSelf"])),
    );
    out.set_num(
        "WitherEffectOnSelf",
        more(&db, &["WitherEffectOnSelf"]) * (100.0 + inc(&db, &["WitherEffectOnSelf"])),
    );
    // :1640-1643
    let rate = base(&db, &["SelfDebuffExpirationRate"]);
    out.set_num("DebuffExpirationRate", rate);
    let modifier = 10000.0 / (100.0 + rate);
    out.set_num("DebuffExpirationModifier", modifier);
    out.set_bool("showDebuffExpirationModifier", modifier != 100.0);
    out.set_num(
        "SelfBlindDuration",
        more(&db, &["SelfBlindDuration"]) * (100.0 + inc(&db, &["SelfBlindDuration"])) * modifier / 100.0,
    );

    // :1645-1655 Firesong, "only used for breakdown purposes" -- mods still created (not covered by goldens)
    if fl(&["IgniteDurationAppliesToElementalAilments"]) {
        let i = inc(&db, &["SelfIgniteDuration"]);
        let m = more(&db, &["SelfIgniteDuration"]);
        if i != 0.0 {
            nm(&db, "SelfIgniteDurationToElementalAilments", Inc, i, Some("Firesong"));
        }
        if m != 1.0 {
            nm(&db, "SelfIgniteDurationToElementalAilments", More, m, Some("Firesong"));
        }
    }

    // :1657-1667 ailment duration on self (the two loops multiply in opposite orders; IEEE `*` commutes)
    for ailment in &rules.non_elemental_ailment_type_list {
        let d = format!("Self{ailment}Duration");
        let m = more(&db, &[&d, "SelfAilmentDuration"]);
        let i = (100.0 + inc(&db, &[&d, "SelfAilmentDuration"])) * 100.0;
        let v = i * m / (100.0 + out.num("DebuffExpirationRate") + base(&db, &[&format!("Self{ailment}DebuffExpirationRate")]));
        out.set_num(&d, v);
    }
    for ailment in &rules.elemental_ailment_type_list {
        let ignite_applies_to_all = fl(&["IgniteDurationAppliesToElementalAilments"]) && ailment != "Ignite";
        let d = format!("Self{ailment}Duration");
        let m = more(&db, &[&d, "SelfAilmentDuration", "SelfElementalAilmentDuration"])
            * if ignite_applies_to_all {
                more(&db, &["SelfIgniteDuration"])
            } else {
                1.0
            };
        let i = (100.0
            + inc(&db, &[&d, "SelfAilmentDuration", "SelfElementalAilmentDuration"])
            + if ignite_applies_to_all {
                inc(&db, &["SelfIgniteDuration"])
            } else {
                0.0
            })
            * 100.0;
        let v = m * i / (100.0 + out.num("DebuffExpirationRate") + base(&db, &[&format!("Self{ailment}DebuffExpirationRate")]));
        out.set_num(&d, v);
    }
    // :1668-1670 D21: `"Condition:"..ailment.."edSelf"` verbatim -- BleededSelf, IgniteedSelf, SapedSelf, ...
    for ailment in &rules.ailment_type_list {
        let self_mod = cmod(&db, &[&format!("Self{ailment}Effect")]);
        let enemy_name = format!("Enemy{ailment}Effect");
        let other = if fl(&[&format!("Condition:{ailment}edSelf")]) {
            cmod(&db, &[&enemy_name])
        } else {
            cmod(&enemy_db, &[&enemy_name])
        };
        out.set_num(&format!("Self{ailment}Effect"), self_mod * other * 100.0);
    }
}
