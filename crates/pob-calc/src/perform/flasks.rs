//! CalcPerform.lua:1524-1942 -- ConvertArmourESToLife (:1524), damage -> aura effect (Breach rings, :1539), minion life -> player
//! (Rigwald's Hunt, :1565), Mageblood (:1577), the flask upvalues and breakdown (:1602-1634), `mergeFlasks` (:1636-1854, called at :1937
//! and :3121), the tincture upvalues and breakdown (:1855-1860) and `mergeTinctures` (:1862-1934, called at :2095) -- R6.5, skills on only.
//!
//! * `mergeBuff` (:45-67) is ported here (its only callers before the buff phase are the flasks and tinctures).
//! * The closures capture the upvalues of :1602-1611 / :1855-1857 ONCE: the :3121 call reuses the values computed at :1602
//!   ([`FlaskCtx`]), and `mergeTinctures` decrements its `tinctureLimit` upvalue ([`TinctureCtx`]).
//! * `env.minion` (R7.10): `flasksApplyToMinion` / `quickSilverAppliesToAllies` / `nonUniqueFlasksApplyToMinion` (:1607-1609) and the
//!   minion half of `mergeFlasks` (:1829-1853) are ported.
//! * Hash-order walks (MINEFIELD-RUST R13/D13): `pairs(flasks)` / `pairs(tinctures)` (:1798, :1909) are keyed by item TABLES (address
//!   order); Rust walks the slot order (`env.flasks`). The order decides (a) which of two flasks with the same `mergeBuff` key and an EQUAL
//!   value keeps its mod (the first; only the source can differ), and (b) which tinctures fit under `TinctureLimit` -- neither happens on the
//!   goldens. `pairs(appliedFlaskBuffs)` / `pairs(tinctureBuffs)` / the condition tables are keyed by strings: their order only decides the
//!   insertion order into modDB (a multiset at the checkpoints).

use std::rc::Rc;

use indexmap::IndexMap;
use pob_items::Item;
use pob_lua::number::{max2, min2};
use pob_mods::{Mod, ModCfg, ModStore, ModType, StoreRef, Value, create_mod};

use super::{base, cfg_actor, flag, inc, max_, more, nm, num_val, set_cond, sum};
use crate::calc_tools::get_converted_mod_tags;
use crate::env::Env;
use crate::output::Output;
use crate::setup::{keyword_flag, list_mods, mod_flag, n};

/// `str:gsub("%s+", "")` (Lua `%s`: space, \t, \n, \v, \f, \r).
fn strip_space(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
        .collect()
}

/// `item.baseName` -- indexed unconditionally upstream.
fn base_name(item: &Item, line: u32) -> &str {
    item.base_name
        .as_deref()
        .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:{line}: attempt to concatenate field 'baseName' (a nil value)"))
}

/// :1524-1575
pub(super) fn conversions(env: &mut Env) {
    let mod_db = env.mod_db.clone();
    // :1524-1537 ConvertArmourESToLife -- `pairs` over an array: in order.
    if flag(&mod_db, &["ConvertArmourESToLife"]) {
        let p = env.player.borrow();
        for slot in ["Helmet", "Gloves", "Boots", "Body Armour", "Weapon 2", "Weapon 3"] {
            if let Some(ad) = p.item_list.get(slot).and_then(|i| i.armour_data.as_ref()) {
                let es_base = ad.num("EnergyShield").unwrap_or(0.0);
                if es_base > 0.0 {
                    nm(
                        &mod_db,
                        "Life",
                        ModType::Base,
                        es_base,
                        &format!("{slot} ES to Life Conversion"),
                        0,
                        0,
                        vec![],
                    );
                }
            }
        }
    }
    // :1539-1563 damage to aura effect conversion from Breach rings
    for element in ["Lightning", "Fire", "Cold", "Chaos", "Physical"] {
        if !flag(&mod_db, &[&format!("{element}DamageAppliesTo{element}AuraEffect")]) {
            continue;
        }
        // `(Sum(...) or 100) / 100` -- Sum is always a number: the `or 100` is dead.
        let multiplier = base(&mod_db, &[&format!("Improved{element}DamageAppliesTo{element}AuraEffect")]) / 100.0;
        let limit = max_(&mod_db, None, &[&format!("{element}DamageAppliesTo{element}AuraEffectLimit")]);
        let mut total_converted = 0.0;
        // `Tabulate("INC", { }, ...)` -- an empty cfg table, not nil.
        let entries = super::ok(
            mod_db
                .borrow()
                .tabulate(Some(ModType::Inc), Some(&ModCfg::default()), &[&format!("{element}Damage")]),
        );
        for e in entries {
            let m = &e.mod_;
            let modifiers = get_converted_mod_tags(&m.tags, multiplier, true);
            let mut converted = (num_val(&m.value, "Modules/CalcPerform.lua:1549") * multiplier).floor();
            if let Some(limit) = limit
                && converted > 0.0
            {
                let remaining = limit - total_converted;
                if remaining <= 0.0 {
                    converted = 0.0;
                } else {
                    converted = min2(converted, remaining);
                    total_converted += converted;
                }
            }
            if converted != 0.0 {
                let created = create_mod(
                    "AuraEffect",
                    m.ty,
                    n(converted),
                    m.source.as_deref(),
                    m.flags,
                    keyword_flag(element),
                    modifiers,
                );
                mod_db.borrow_mut().new_mod(created);
            }
        }
    }
    // :1565-1575 minion life conversion from Rigwald's Hunt
    if flag(&mod_db, &["MinionLifeAppliesToPlayer"]) {
        let multiplier = max_(&mod_db, None, &["ImprovedMinionLifeAppliesToPlayer"]).unwrap_or(100.0) / 100.0;
        for value in super::list(&mod_db, &["MinionModifier"]) {
            let Value::Mod(m) = value.get("mod") else {
                panic!("Modules/CalcPerform.lua:1569: attempt to index field 'mod' (a non-mod value)")
            };
            if &*m.name == "Life" && m.ty == Some(ModType::Inc) {
                let modifiers = get_converted_mod_tags(&m.tags, multiplier, true);
                let v = (num_val(&m.value, "Modules/CalcPerform.lua:1572") * multiplier).floor();
                let created = create_mod(
                    "Life",
                    Some(ModType::Inc),
                    n(v),
                    m.source.as_deref(),
                    m.flags,
                    m.keyword_flags,
                    modifiers,
                );
                mod_db.borrow_mut().new_mod(created);
            }
        }
    }
}

/// `env.flasks[item] = true` -- a set keyed by the item table.
fn add_flask(env: &mut Env, item: &Rc<Item>) {
    if !env.flasks.iter().any(|f| Rc::ptr_eq(f, item)) {
        env.flasks.push(item.clone());
    }
}

/// :1577-1600 Mageblood: the first / last N magic utility flasks of `orderedSlots` become active.
pub(super) fn mageblood(env: &mut Env) {
    let mod_db = env.mod_db.clone();
    let max_left = base(&mod_db, &["LeftActiveMagicUtilityFlasks"]);
    let max_right = base(&mod_db, &["RightActiveMagicUtilityFlasks"]);
    if max_left > 0.0 || max_right > 0.0 {
        let mut magic: Vec<Rc<Item>> = Vec::new();
        for slot in &env.ordered_slots {
            if let Some(item) = env.item(slot.sel_item_id)
                && item.ty.as_deref() == Some("Flask")
                && item.rarity == "MAGIC"
            {
                let bn = base_name(item, 1583);
                if !(bn.contains("Life Flask") || bn.contains("Mana Flask") || bn.contains("Hybrid Flask")) {
                    magic.push(item.clone());
                }
            }
        }
        let len = magic.len() as f64;
        if max_left > 0.0 {
            // `for i = 1, m_min(maxLeft, #magic)` (a fractional limit truncates the loop)
            let mut i = 1.0;
            while i <= min2(max_left, len) {
                add_flask(env, &magic[i as usize - 1]);
                i += 1.0;
            }
        }
        if max_right > 0.0 {
            // `for i = #magic, m_max(#magic - maxRight + 1, 1), -1`
            let stop = max2(len - max_right + 1.0, 1.0);
            let mut i = len;
            while i >= stop {
                add_flask(env, &magic[i as usize - 1]);
                i -= 1.0;
            }
        }
    }
}

/// The upvalues `mergeFlasks` / `calcFlaskRecovery` / `calcFlaskMods` capture (:1602-1611).
pub(super) struct FlaskCtx {
    effect_inc: f64,
    effect_inc_magic: f64,
    effect_inc_magic_no_adjacent: f64,
    effect_inc_non_player: f64,
    effect_inc_magic_non_player: f64,
    /// `env.minion and ...` (:1607-1609).
    flasks_apply_to_minion: bool,
    quick_silver_applies_to_allies: bool,
    pub non_unique_flasks_apply_to_minion: bool,
    flask_total_rate_inc: f64,
    flask_dur_inc: f64,
}

/// :1607-1609 `env.minion and modDB:Flag(env.player.mainSkill.skillCfg, name)` (`player`) / `env.minion and env.minion.modDB:Flag(nil, name)`.
fn minion_flag(env: &Env, player: bool, name: &str) -> bool {
    let Some(m) = &env.minion else { return false };
    if player {
        let cfg = super::player_main_skill(env, 1607).cfg;
        super::flag_cfg(&env.mod_db, Some(&cfg), &[name])
    } else {
        flag(&m.borrow().mod_db, &[name])
    }
}

/// :1602-1634 the flask upvalues and the flask breakdown (MAIN builds the breakdown: it writes output keys, MINEFIELD-RUST R8).
pub(super) fn flask_ctx(env: &Env, output: &mut Output, breakdown: bool) -> FlaskCtx {
    let db = &env.mod_db;
    let player = cfg_actor("player");
    let cx = FlaskCtx {
        effect_inc: sum(db, ModType::Inc, Some(&player), &["FlaskEffect"]),
        effect_inc_magic: sum(db, ModType::Inc, Some(&player), &["MagicUtilityFlaskEffect"]),
        effect_inc_magic_no_adjacent: sum(db, ModType::Inc, Some(&player), &["MagicFlaskNoAdjacentEffect"]),
        effect_inc_non_player: inc(db, &["FlaskEffect"]),
        effect_inc_magic_non_player: inc(db, &["MagicUtilityFlaskEffect"]),
        flasks_apply_to_minion: minion_flag(env, true, "FlasksApplyToMinion"),
        quick_silver_applies_to_allies: minion_flag(env, true, "QuickSilverAppliesToAllies"),
        non_unique_flasks_apply_to_minion: minion_flag(env, false, "ParentNonUniqueFlasksAppliedToYou"),
        flask_total_rate_inc: inc(db, &["FlaskRecoveryRate"]),
        flask_dur_inc: inc(db, &["FlaskDuration"]),
    };
    if breakdown {
        let charges_generated = base(db, &["FlaskChargesGenerated"]);
        // `pairs(env.flasks)` counting the `true` values
        let used_flasks = env.flasks.len() as f64;
        let per_flask = base(db, &["FlaskChargesGeneratedPerEmptyFlask"]) * (5.0 - used_flasks);
        let total = charges_generated + per_flask;
        let utility = base(db, &["UtilityFlaskChargesGenerated"]);
        let life = base(db, &["LifeFlaskChargesGenerated"]);
        let mana = base(db, &["ManaFlaskChargesGenerated"]);
        output.set_num("FlaskEffect", cx.effect_inc);
        output.set_num("FlaskChargeGen", total);
        output.set_num("LifeFlaskChargeGen", total + life);
        output.set_num("ManaFlaskChargeGen", total + mana);
        output.set_num("UtilityFlaskChargeGen", total + utility);
        output.set_num("FlaskChargeOnCritChance", min2(100.0, base(db, &["FlaskChargeOnCritChance"])));
    }
    cx
}

fn flask_num(item: &Item, k: &str, line: u32) -> f64 {
    item.flask_data
        .as_ref()
        .and_then(|d| d.num(k))
        .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:{line}: attempt to perform arithmetic on field '{k}' (a nil value)"))
}

fn flask_opt(item: &Item, k: &str) -> Value {
    item.flask_data.as_ref().map(|d| d.get(k)).unwrap_or_default()
}

fn flask_base(item: &Item) -> &'static pob_data::item_bases::ItemBaseFlask {
    item.base
        .and_then(|b| b.flask.as_ref())
        .unwrap_or_else(|| panic!("Modules/CalcPerform.lua: attempt to index field 'flask' (a nil value)"))
}

/// :1637-1649 `getFlaskInstantRecovery`.
fn instant_recovery(db: &StoreRef, item: &Item) -> f64 {
    let mut perc = flask_opt(item, "instantPerc").as_num().unwrap_or(0.0);
    if flag(db, &["Condition:LowLife"]) {
        perc += flask_opt(item, "instantLowLifePerc").as_num().unwrap_or(0.0);
    }
    let fb = flask_base(item);
    if fb.life.is_some() {
        perc += base(db, &["LifeFlaskInstantRecovery"]);
    }
    if fb.mana.is_some() {
        perc += base(db, &["ManaFlaskInstantRecovery"]);
    }
    min2(100.0, perc)
}

/// :1651-1689 `calcFlaskRecovery(type, item)`.
fn flask_recovery(db: &StoreRef, cx: &FlaskCtx, ty: &str, item: &Item) -> Vec<Rc<Mod>> {
    let mut out = Vec::new();
    let l = ty.to_ascii_lowercase();
    if !flask_opt(item, &format!("{l}EffectNotRemoved")).truthy() && !flag(db, &[&format!("{ty}FlaskEffectNotRemoved")]) {
        return out;
    }
    let name = item.name.as_str();
    let base_amount = flask_num(item, &format!("{l}Base"), 1662);
    let dur = flask_num(item, "duration", 1663);
    let inst = instant_recovery(db, item);
    let rec_inc = inc(db, &[&format!("Flask{ty}Recovery")]);
    let rec_more = more(db, None, &[&format!("Flask{ty}Recovery")]);
    let rate_inc = inc(db, &[&format!("Flask{ty}RecoveryRate")]);
    let mut total = base_amount * (1.0 - inst / 100.0) * (1.0 + rec_inc / 100.0) * rec_more * (1.0 + cx.flask_dur_inc / 100.0);
    let flask_dur = dur * (1.0 + cx.flask_dur_inc / 100.0) / (1.0 + cx.flask_total_rate_inc / 100.0) / (1.0 + rate_inc / 100.0);
    let low_life_more = more(db, None, &["FlaskLifeRecoveryLowLife"]);
    if low_life_more > 1.0 {
        total *= if l == "life" {
            (low_life_more - 1.0) / (1.0 + cx.effect_inc / 100.0) + 1.0
        } else {
            1.0
        };
    }
    let rate = total / flask_dur;
    out.push(Rc::new(create_mod(
        &format!("{ty}Recovery"),
        Some(ModType::Base),
        n(rate),
        Some(name),
        0,
        0,
        vec![],
    )));
    if flag(db, &[&format!("{ty}FlaskAppliesToEnergyShield")]) {
        out.push(Rc::new(create_mod(
            "EnergyShieldRecovery",
            Some(ModType::Base),
            n(rate),
            Some(name),
            0,
            0,
            vec![],
        )));
    }
    if flag(db, &[&format!("{ty}FlaskAppliesToLife")]) {
        out.push(Rc::new(create_mod(
            "LifeRecovery",
            Some(ModType::Base),
            n(rate),
            Some(name),
            0,
            0,
            vec![],
        )));
    }
    out
}

type Buffs = IndexMap<String, Vec<Rc<Mod>>>;

/// :45-67 `mergeBuff(src, destTable, destKey)` (the algorithm lives in [`super::buffs::merge_mods`]).
fn merge_buff(src: &[Rc<Mod>], dest_table: &mut Buffs, key: &str) {
    super::buffs::merge_mods(src, dest_table.entry(key.to_owned()).or_default());
}

/// `srcList = new("ModList"):ModList(); srcList:ScaleAddList(list, scale)`.
fn scaled(list: &[Rc<Mod>], scale: f64) -> Vec<Rc<Mod>> {
    let src = ModStore::new_list(None);
    super::ok(src.borrow_mut().scale_add_list(list, scale, false));
    list_mods(&src)
}

/// `item.buffModList` / `item.modList` as the Lua arrays.
fn buff_mods(item: &Item) -> Vec<Rc<Mod>> {
    item.buff_mod_list
        .as_ref()
        .map(|l| l.iter().map(|m| Rc::new(m.clone())).collect())
        .unwrap_or_else(|| panic!("Modules/CalcPerform.lua: item.buffModList is nil"))
}

fn item_mods(item: &Item) -> Vec<Rc<Mod>> {
    item.mod_list
        .as_ref()
        .map(|l| l.iter().map(|m| Rc::new(m.m.clone())).collect())
        .unwrap_or_else(|| panic!("Modules/CalcPerform.lua: item.modList is nil"))
}

/// The `mergeBuff` key of an item's `modList` (:1747-1755 / :1880-1888).
fn mod_list_key(item: &Item, mods: &[Rc<Mod>]) -> String {
    if item.rarity == "UNIQUE" || item.rarity == "RELIC" {
        item.title
            .clone()
            .unwrap_or_else(|| panic!("Modules/CalcPerform.lua:1750: item.title is nil (a nil table key)"))
    } else {
        let mut key = String::new();
        for m in mods {
            key.push_str(&pob_mods::format::format_mod_params(m));
            key.push('&');
        }
        key
    }
}

/// `env.flaskSlotMap[item]` -- the map is keyed by the slot here: the last flask slot holding this item (a later assignment wins).
fn flask_slot_num(env: &Env, item: &Rc<Item>) -> Option<f64> {
    let mut found = None;
    for (slot_name, num) in &env.flask_slot_map {
        if let Some(slot) = env.ordered_slots.iter().find(|s| &s.slot_name == slot_name)
            && env.item(slot.sel_item_id).is_some_and(|i| Rc::ptr_eq(i, item))
        {
            found = Some(*num);
        }
    }
    found
}

#[derive(Default)]
struct FlaskTables {
    buffs: Buffs,
    conditions: IndexMap<String, bool>,
    conditions_non_utility: IndexMap<String, bool>,
    per_base: IndexMap<String, Buffs>,
    non_player: Buffs,
    per_base_non_player: IndexMap<String, Buffs>,
    non_utility: Buffs,
}

/// :1700-1771 `calcFlaskMods(item, baseName, buffModList, modList, onlyMinion)`.
#[allow(clippy::too_many_arguments)]
fn calc_flask_mods(
    env: &Env,
    cx: &FlaskCtx,
    t: &mut FlaskTables,
    only_recovery: bool,
    check_non_recovery: bool,
    item: &Rc<Item>,
    base_key: &str,
    buff_mod_list: &[Rc<Mod>],
    mod_list: &[Rc<Mod>],
    only_minion: bool,
) {
    let item_base_name = base_name(item, 1700).to_owned();
    let item_effect_inc = flask_num(item, "effectInc", 1701);
    let mut eff = cx.effect_inc + item_effect_inc;
    let mut eff_non_player = cx.effect_inc_non_player + item_effect_inc;
    let fb = flask_base(item);
    let life_or_mana = fb.life.is_some() || fb.mana.is_some();
    if item.rarity == "MAGIC" && !life_or_mana {
        eff += cx.effect_inc_magic;
        eff_non_player += cx.effect_inc_magic_non_player;
    }
    // Essence of Desolation belt mod: bonus for magic flasks with no flask in an adjacent slot (1-5)
    if item.rarity == "MAGIC"
        && cx.effect_inc_magic_no_adjacent != 0.0
        && let Some(num) = flask_slot_num(env, item)
    {
        let occ = |k: f64| env.flask_slot_occupied.get(&pob_lua::tostring_num(k)).copied().unwrap_or(false);
        if !(occ(num - 1.0) || occ(num + 1.0)) {
            eff += cx.effect_inc_magic_no_adjacent;
        }
    }
    let effect_mod = 1.0 + eff / 100.0;
    let effect_mod_non_player = 1.0 + eff_non_player / 100.0;
    let is_utility = !life_or_mana;
    let to_minion = (!only_recovery || check_non_recovery)
        && (cx.flasks_apply_to_minion
            || cx.quick_silver_applies_to_allies
            || (cx.non_unique_flasks_apply_to_minion && item.rarity != "UNIQUE" && item.rarity != "RELIC"));

    if !buff_mod_list.is_empty() {
        if !only_minion {
            let src = scaled(buff_mod_list, effect_mod);
            merge_buff(&src, &mut t.buffs, base_key);
            merge_buff(&src, t.per_base.entry(item_base_name.clone()).or_default(), base_key);
            if !is_utility {
                merge_buff(&src, &mut t.non_utility, base_key);
            }
        }
        if to_minion {
            let src = scaled(buff_mod_list, effect_mod_non_player);
            merge_buff(&src, &mut t.non_player, base_key);
            merge_buff(&src, t.per_base_non_player.entry(item_base_name.clone()).or_default(), base_key);
        }
    }
    if !mod_list.is_empty() {
        let src = scaled(mod_list, effect_mod);
        let key = mod_list_key(item, mod_list);
        if !only_recovery {
            merge_buff(&src, &mut t.buffs, &key);
            merge_buff(&src, t.per_base.entry(item_base_name.clone()).or_default(), &key);
            if !is_utility {
                merge_buff(&src, &mut t.non_utility, &key);
            }
        }
        if to_minion {
            let src = scaled(mod_list, effect_mod_non_player);
            merge_buff(&src, &mut t.non_player, &key);
            merge_buff(&src, t.per_base_non_player.entry(item_base_name).or_default(), &key);
        }
    }
}

/// :1691-1854 `mergeFlasks(flasks, onlyRecovery, checkNonRecoveryFlasksForMinions)` over `env.flasks`.
pub(super) fn merge_flasks(env: &Env, cx: &FlaskCtx, only_recovery: bool, check_non_recovery: bool) {
    let db = env.mod_db.clone();
    let mut t = FlaskTables::default();
    for item in &env.flasks {
        let bn = base_name(item, 1799).to_owned();
        t.per_base.entry(bn.clone()).or_default();
        t.per_base_non_player.entry(bn.clone()).or_default();
        let fb = flask_base(item);
        if instant_recovery(&db, item) < 100.0 {
            let using = format!("Using{}", strip_space(&bn));
            t.conditions.insert("UsingFlask".into(), true);
            t.conditions.insert(using.clone(), true);
            if fb.life.is_some() || fb.mana.is_some() {
                t.conditions_non_utility.insert("UsingFlask".into(), true);
                t.conditions_non_utility.insert(using, true);
            }
            if fb.life.is_some() && !flag(&db, &["CannotRecoverLifeOutsideLeech"]) {
                t.conditions.insert("UsingLifeFlask".into(), true);
                t.conditions_non_utility.insert("UsingLifeFlask".into(), true);
            }
            if fb.mana.is_some() {
                t.conditions.insert("UsingManaFlask".into(), true);
                t.conditions_non_utility.insert("UsingManaFlask".into(), true);
            }
        }
        if bn == "Iron Flask" {
            let on_ward_break = base(&db, &["IronFlaskChargesGeneratedOnWardBreak"]);
            if on_ward_break > 0.0 {
                let gain_mod = flask_num(item, "gainMod", 1819) * (1.0 + inc(&db, &["FlaskChargesGained"]) / 100.0);
                let charges_used = flask_num(item, "chargesUsed", 1820) * (1.0 + inc(&db, &["FlaskChargesUsed"]) / 100.0);
                if on_ward_break * gain_mod > charges_used {
                    t.conditions.insert("UnbrokenWard".into(), true);
                }
            }
        }
        if only_recovery {
            if fb.life.is_some() && !flag(&db, &["CannotRecoverLifeOutsideLeech"]) {
                let rec = flask_recovery(&db, cx, "Life", item);
                calc_flask_mods(
                    env,
                    cx,
                    &mut t,
                    only_recovery,
                    check_non_recovery,
                    item,
                    "LifeFlask",
                    &rec,
                    &[],
                    false,
                );
            }
            if fb.mana.is_some() {
                let rec = flask_recovery(&db, cx, "Mana", item);
                calc_flask_mods(
                    env,
                    cx,
                    &mut t,
                    only_recovery,
                    check_non_recovery,
                    item,
                    "ManaFlask",
                    &rec,
                    &[],
                    false,
                );
            }
            if check_non_recovery {
                let (b, m) = (buff_mods(item), item_mods(item));
                calc_flask_mods(env, cx, &mut t, only_recovery, check_non_recovery, item, &bn, &b, &m, true);
            }
        } else {
            let (b, m) = (buff_mods(item), item_mods(item));
            calc_flask_mods(env, cx, &mut t, only_recovery, check_non_recovery, item, &bn, &b, &m, false);
        }
    }
    if !flag(&db, &["FlasksDoNotApplyToPlayer"]) {
        let utility_disabled = flag(&db, &["UtilityFlasksDoNotApplyToPlayer"]);
        let (conds, buffs) = if utility_disabled {
            (&t.conditions_non_utility, &t.non_utility)
        } else {
            (&t.conditions, &t.buffs)
        };
        for (c, status) in conds {
            set_cond(&db, c, *status);
        }
        for list in buffs.values() {
            db.borrow_mut().add_list(list);
        }
    }
    // :1829-1853 the minion half (R7.10)
    if let Some(m) = &env.minion {
        let mdb = m.borrow().mod_db.clone();
        if cx.flasks_apply_to_minion || cx.non_unique_flasks_apply_to_minion {
            for (c, status) in &t.conditions {
                set_cond(&mdb, c, *status);
            }
            for list in t.non_player.values() {
                mdb.borrow_mut().add_list(list);
            }
        } else if cx.quick_silver_applies_to_allies
            && let Some(per) = t.per_base_non_player.get("Quicksilver Flask")
        {
            // `minionModDB.conditions[k] = flaskConditions[k]` -- a nil assignment removes the key
            for k in ["UsingQuicksilverFlask", "UsingFlask"] {
                match t.conditions.get(k) {
                    Some(v) => set_cond(&mdb, k, *v),
                    None => {
                        mdb.borrow_mut().conditions.shift_remove(k);
                    }
                }
            }
            for list in per.values() {
                mdb.borrow_mut().add_list(list);
            }
        }
    }
}

/// The upvalues `mergeTinctures` captures (:1855-1857) -- `tinctureLimit` is decremented by the call.
pub(super) struct TinctureCtx {
    effect_inc: f64,
    effect_inc_magic: f64,
    tincture_limit: f64,
}

/// :1855-1860 the tincture upvalues and breakdown.
pub(super) fn tincture_ctx(env: &Env, output: &mut Output, breakdown: bool) -> TinctureCtx {
    let db = &env.mod_db;
    let player = cfg_actor("player");
    let cx = TinctureCtx {
        effect_inc: sum(db, ModType::Inc, Some(&player), &["TinctureEffect"]),
        effect_inc_magic: sum(db, ModType::Inc, Some(&player), &["MagicTinctureEffect"]),
        tincture_limit: base(db, &["TinctureLimit"]),
    };
    if breakdown {
        output.set_num("TinctureEffect", cx.effect_inc);
        output.set_num("TinctureLimit", cx.tincture_limit);
    }
    cx
}

/// :1862-1934 `mergeTinctures(env.tinctures)`.
pub(super) fn merge_tinctures(env: &Env, cx: &mut TinctureCtx, output: &Output) {
    let db = env.mod_db.clone();
    let not_burn = min2(base(&db, &["TincturesNotInflictManaBurn"]), 100.0);
    let can_gain = flag(&db, &["Condition:WeepingWoundsInsteadOfManaBurn"])
        || (not_burn < 100.0 && output.opt_num("ManaUnreserved").unwrap_or(0.0) > 0.0);
    if !can_gain {
        return;
    }
    let mut buffs: Buffs = IndexMap::new();
    let mut conditions: IndexMap<String, bool> = IndexMap::new();
    let mut per_base: IndexMap<String, Buffs> = IndexMap::new();
    for item in &env.tinctures {
        if cx.tincture_limit <= 0.0 {
            break;
        }
        cx.tincture_limit -= 1.0;
        let bn = base_name(item, 1914).to_owned();
        per_base.entry(bn.clone()).or_default();
        conditions.insert("UsingTincture".into(), true);
        conditions.insert(format!("Using{}", strip_space(&bn)), true);
        // :1867-1898 `calcTinctureMods(item, item.baseName, item.buffModList, item.modList)`
        let item_inc =
            item.tincture_data.as_ref().and_then(|d| d.num("effectInc")).unwrap_or_else(|| {
                panic!("Modules/CalcPerform.lua:1868: attempt to perform arithmetic on field 'effectInc' (a nil value)")
            });
        let mut eff = cx.effect_inc + item_inc;
        if item.rarity == "MAGIC" {
            eff += cx.effect_inc_magic;
        }
        // rounded to 2 decimal places before applying it
        let effect_mod = ((1.0 + eff / 100.0) * (1.0 + item.quality.unwrap_or(0.0) / 100.0) * 100.0).floor() / 100.0;
        let buff_mod_list = buff_mods(item);
        if !buff_mod_list.is_empty() {
            let src = scaled(&buff_mod_list, effect_mod);
            merge_buff(&src, &mut buffs, &bn);
            merge_buff(&src, per_base.entry(bn.clone()).or_default(), &bn);
        }
        let mod_list = item_mods(item);
        if !mod_list.is_empty() {
            let src = scaled(&mod_list, effect_mod);
            let key = mod_list_key(item, &mod_list);
            merge_buff(&src, &mut buffs, &key);
            merge_buff(&src, per_base.entry(bn.clone()).or_default(), &key);
        }
    }
    for (c, status) in &conditions {
        set_cond(&db, c, *status);
    }
    for list in buffs.values() {
        db.borrow_mut().add_list(list);
    }
    if flag(&db, &["TinctureRangedWeapons"]) {
        let melee = mod_flag("WeaponMelee");
        let ranged = mod_flag("WeaponRanged");
        for list in buffs.values() {
            for buff in list {
                if buff.flags & melee == melee {
                    // `copyTable(buff, true)` -- a shallow copy (the tags stay shared)
                    let mut m = (**buff).clone();
                    m.flags = (m.flags & !melee) | ranged;
                    db.borrow_mut().add_list(&[Rc::new(m)]);
                }
            }
        }
    }
}
