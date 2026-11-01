//! `Modules/BuildDisplayStats.lua` (293 lines) verbatim: `displayStats` (174 entries, separators included) and `minionDisplayStats`
//! (28), with every `condFunc` / `warnFunc` / `suffix` / `suffixCondFunc` / `overCapStatCondFunc` as a Rust function that evaluates the
//! same Lua expression (same short-circuits, same `nil`/`false` results, the Lua error where Lua would raise one).
//!
//! A Lua table constructor with a repeated key keeps the last one: `flag = "poison", flag = "notAverage"` (:96) is
//! `flag = "notAverage"`; the same for :99-102 (`impale` dropped).

use pob_calc::OutTable;

use crate::lua::{LuaRes, LuaV, TableRef, eq, ge, gt, le, lt, ne};
use crate::view::ActorView;

/// `colorCodes` (Data/Global.lua:7-77), the entries the display reads.
pub mod color {
    pub const NORMAL: &str = "^xC8C8C8";
    pub const RARE: &str = "^xFFFF77";
    pub const CURRENCY: &str = "^xAA9E82";
    pub const CUSTOM: &str = "^x5CF0BB";
    pub const SOURCE: &str = "^x88FFFF";
    pub const WARNING: &str = "^xFF9922";
    pub const FIRE: &str = "^xB97123";
    pub const COLD: &str = "^x3F6DB3";
    pub const LIGHTNING: &str = "^xADAA47";
    pub const CHAOS: &str = "^xD02090";
    pub const POSITIVE: &str = "^x33FF77";
    pub const NEGATIVE: &str = "^xDD0022";
    pub const MARAUDER: &str = "^xE05030";
    pub const RANGER: &str = "^x70FF70";
    pub const WITCH: &str = "^x7070FF";
    // :65-77 aliases
    pub const STRENGTH: &str = MARAUDER;
    pub const DEXTERITY: &str = RANGER;
    pub const INTELLIGENCE: &str = WITCH;
    pub const LIFE: &str = MARAUDER;
    pub const MANA: &str = WITCH;
    pub const ES: &str = SOURCE;
    pub const WARD: &str = RARE;
    pub const ARMOUR: &str = NORMAL;
    pub const EVASION: &str = POSITIVE;
    pub const RAGE: &str = WARNING;
    pub const PHYS: &str = NORMAL;
}

use color::*;

/// A display function: `function(v, o)` -- the stat value and the actor's `output`.
pub type StatFn = for<'a> fn(&LuaV<'a>, &ActorView<'a>) -> LuaRes<'a>;

/// `suffix`: a string, or `function(value, output)`.
#[derive(Clone, Copy)]
pub enum Suffix {
    Str(&'static str),
    Fn(StatFn),
}

/// One `DisplayStat` (BuildDisplayStats.lua:20-47). `bool` fields are `true` or absent (nil) in the Lua table.
#[derive(Clone, Copy)]
pub struct DisplayStat {
    pub stat: Option<&'static str>,
    pub child_stat: Option<&'static str>,
    pub label: Option<&'static str>,
    pub fmt: Option<&'static str>,
    pub breakdown: Option<&'static str>,
    pub mod_names: Option<&'static [&'static str]>,
    pub ignored_sections: Option<&'static [&'static str]>,
    pub color: Option<&'static str>,
    pub flag: Option<&'static str>,
    pub not_flag: Option<&'static str>,
    pub cond_func: Option<StatFn>,
    pub warn_func: Option<StatFn>,
    pub warn_color: bool,
    pub suffix: Option<Suffix>,
    pub suffix_cond_func: Option<StatFn>,
    pub over_cap_stat: Option<&'static str>,
    pub over_cap_stat_cond_func: Option<StatFn>,
    pub compact_value: bool,
    pub comp_percent: bool,
    pub lower_is_better: bool,
    pub pc: bool,
    pub mod_: bool,
    pub hide_stat: bool,
    pub chaos_inoc: bool,
    pub pool: Option<&'static str>,
    pub label_stat: Option<&'static str>,
    pub val: Option<&'static str>,
}

/// `{ }` -- a separator.
const SEP: DisplayStat = DisplayStat {
    stat: None,
    child_stat: None,
    label: None,
    fmt: None,
    breakdown: None,
    mod_names: None,
    ignored_sections: None,
    color: None,
    flag: None,
    not_flag: None,
    cond_func: None,
    warn_func: None,
    warn_color: false,
    suffix: None,
    suffix_cond_func: None,
    over_cap_stat: None,
    over_cap_stat_cond_func: None,
    compact_value: false,
    comp_percent: false,
    lower_is_better: false,
    pc: false,
    mod_: false,
    hide_stat: false,
    chaos_inoc: false,
    pool: None,
    label_stat: None,
    val: None,
};

/// `{ stat = stat, label = label, fmt = fmt }`; the other fields are set by the builder methods below (one per Lua field).
const fn s(stat: &'static str, label: &'static str, fmt: &'static str) -> DisplayStat {
    DisplayStat {
        stat: Some(stat),
        label: Some(label),
        fmt: Some(fmt),
        ..SEP
    }
}

/// `{ stat, label, fmt = ".1f", compactValue = true, compPercent = true }` -- the common damage row.
const fn dmg(stat: &'static str, label: &'static str) -> DisplayStat {
    s(stat, label, ".1f").cv().cp()
}

/// `{ stat, label, fmt, color, pool, compPercent = true, lowerIsBetter = true, condFunc }` -- a cost row (:118-131).
const fn cost(stat: &'static str, label: &'static str, fmt: &'static str, color: &'static str, cond: StatFn) -> DisplayStat {
    s(stat, label, fmt).color(color).cp().lower().cond(cond)
}

impl DisplayStat {
    const fn child(mut self, c: &'static str) -> Self {
        self.child_stat = Some(c);
        self
    }
    const fn bd(mut self, k: &'static str) -> Self {
        self.breakdown = Some(k);
        self
    }
    const fn mods(mut self, m: &'static [&'static str]) -> Self {
        self.mod_names = Some(m);
        self
    }
    const fn ignored(mut self, m: &'static [&'static str]) -> Self {
        self.ignored_sections = Some(m);
        self
    }
    const fn color(mut self, c: &'static str) -> Self {
        self.color = Some(c);
        self
    }
    const fn flag(mut self, f: &'static str) -> Self {
        self.flag = Some(f);
        self
    }
    const fn not_flag(mut self, f: &'static str) -> Self {
        self.not_flag = Some(f);
        self
    }
    const fn cond(mut self, f: StatFn) -> Self {
        self.cond_func = Some(f);
        self
    }
    const fn warn(mut self, f: StatFn) -> Self {
        self.warn_func = Some(f);
        self
    }
    const fn warn_color(mut self) -> Self {
        self.warn_color = true;
        self
    }
    const fn suffix(mut self, sfx: Suffix) -> Self {
        self.suffix = Some(sfx);
        self
    }
    const fn suffix_cond(mut self, f: StatFn) -> Self {
        self.suffix_cond_func = Some(f);
        self
    }
    const fn over_cap(mut self, k: &'static str) -> Self {
        self.over_cap_stat = Some(k);
        self
    }
    const fn over_cap_cond(mut self, f: StatFn) -> Self {
        self.over_cap_stat_cond_func = Some(f);
        self
    }
    const fn cv(mut self) -> Self {
        self.compact_value = true;
        self
    }
    const fn cp(mut self) -> Self {
        self.comp_percent = true;
        self
    }
    const fn lower(mut self) -> Self {
        self.lower_is_better = true;
        self
    }
    const fn pc(mut self) -> Self {
        self.pc = true;
        self
    }
    const fn mod_(mut self) -> Self {
        self.mod_ = true;
        self
    }
    const fn hide(mut self) -> Self {
        self.hide_stat = true;
        self
    }
    const fn pool(mut self, p: &'static str) -> Self {
        self.pool = Some(p);
        self
    }
}

const DEFENSE_IGNORED_SECTIONS: &[&str] = &["Base from Gear", "Inc. from Tree"];

// --- helpers for the Lua expressions ----------------------------------------------------------------------------------------------

fn b<'a>(x: bool) -> LuaV<'a> {
    LuaV::Bool(x)
}

fn n<'a>(x: f64) -> LuaV<'a> {
    LuaV::Num(x)
}

/// `(x or 0)`.
fn or0(x: LuaV<'_>) -> LuaV<'_> {
    if x.truthy() { x } else { LuaV::Num(0.0) }
}

/// `(o[k] or 0) == 0`.
fn zero(o: &ActorView<'_>, k: &str) -> bool {
    eq(&or0(o.get(k)), &n(0.0))
}

/// `data.misc.DotDpsCap`.
fn dot_cap<'a>() -> LuaV<'a> {
    let c = pob_data::constants::constants();
    LuaV::Num(*c.misc.get("DotDpsCap").expect("data.misc.DotDpsCap"))
}

/// `v >= data.misc.DotDpsCap and msg`.
fn cap_warn<'a>(v: &LuaV<'a>, msg: &str) -> LuaRes<'a> {
    Ok(if ge(v, &dot_cap())? { LuaV::str(msg) } else { b(false) })
}

/// `v ~= o.TotalDPS and (o[k1] or 0) == 0 and ...` -- the "Total DPS inc. X" rows.
fn ne_total_and_zero<'a>(v: &LuaV<'a>, o: &ActorView<'a>, keys: &[&str]) -> LuaRes<'a> {
    Ok(b(ne(v, &o.get("TotalDPS")) && keys.iter().all(|k| zero(o, k))))
}

/// `v > 0 and (o.TriggerTime or 0) == 0`.
fn speed_cond<'a>(v: &LuaV<'a>, o: &ActorView<'a>) -> LuaRes<'a> {
    Ok(b(gt(v, &n(0.0))? && zero(o, "TriggerTime")))
}

/// `(o.TriggerTime or 0) ~= 0`.
fn trigger_cond<'a>(_v: &LuaV<'a>, o: &ActorView<'a>) -> LuaRes<'a> {
    Ok(b(!zero(o, "TriggerTime")))
}

/// `not o.TriggerTime`.
fn not_trigger<'a>(_v: &LuaV<'a>, o: &ActorView<'a>) -> LuaRes<'a> {
    Ok(b(!o.get("TriggerTime").truthy()))
}

/// `v > 0`.
fn positive<'a>(v: &LuaV<'a>, _o: &ActorView<'a>) -> LuaRes<'a> {
    Ok(b(gt(v, &n(0.0))?))
}

/// `function() return true end`.
fn always<'a>(_v: &LuaV<'a>, _o: &ActorView<'a>) -> LuaRes<'a> {
    Ok(b(true))
}

/// `(o[k] or 0) > 0` -- the stat's own value re-read (`CullingDPS`, `ReservationDPS`).
fn own_positive<'a>(o: &ActorView<'a>, k: &str) -> LuaRes<'a> {
    Ok(b(gt(&or0(o.get(k)), &n(0.0))?))
}

/// `o.<a> <op> 0 and o.<b> ~= 0` -- the Regen/Recovery pairs (`le` = Regen, else Recovery).
fn regen_pair<'a>(o: &ActorView<'a>, recovery: &str, regen: &str, regen_row: bool) -> LuaRes<'a> {
    let r = o.get(recovery);
    let first = if regen_row { le(&r, &n(0.0))? } else { gt(&r, &n(0.0))? };
    Ok(b(first && ne(&o.get(regen), &n(0.0))))
}

/// `"You do not meet the <Attr> requirement of " .. (o.Req<A>Item.source == "Item" and o.Req<A>Item.sourceItem.name or
/// o.Req<A>Item.source == "Gem" and o.Req<A>Item.sourceGem.nameSpec)` (:134/:136/:138/:140).
fn req_warn<'a>(o: &ActorView<'a>, attr: &str, attr_name: &str) -> LuaRes<'a> {
    let key = format!("Req{attr}Item");
    let e = match o.get(&key) {
        LuaV::Table(TableRef::Out(OutTable::ReqSource(e))) => e,
        LuaV::Nil => return Err(format!("attempt to index field '{key}' (a nil value)")),
        other => return Err(format!("display: output.{key} is a {} value", other.type_name())),
    };
    let who: LuaV<'a> = if e.source == "Item" {
        let item = e
            .source_item
            .as_ref()
            .ok_or_else(|| "attempt to index field 'sourceItem' (a nil value)".to_owned())?;
        LuaV::str(&item.name)
    } else if e.source == "Gem" {
        let (g, k) = e
            .source_gem
            .ok_or_else(|| "attempt to index field 'sourceGem' (a nil value)".to_owned())?;
        match o.gem_name_spec(g, k) {
            Some(s) => LuaV::str(&s),
            None => LuaV::Nil,
        }
    } else {
        b(false)
    };
    match who {
        LuaV::Str(s) => Ok(LuaV::str(&format!("You do not meet the {attr_name} requirement of {s}"))),
        LuaV::Num(x) => Ok(LuaV::str(&format!(
            "You do not meet the {attr_name} requirement of {}",
            pob_lua::tostring_num(x)
        ))),
        other => Err(format!("attempt to concatenate a {} value", other.type_name())),
    }
}

/// :12-14 `maxHitSuffix(value, output)`: `value == math.huge and "Immune" or output.GuardSkillActive and "Guard" or nil`.
fn max_hit_suffix<'a>(v: &LuaV<'a>, o: &ActorView<'a>) -> LuaRes<'a> {
    if eq(v, &n(f64::INFINITY)) {
        return Ok(LuaV::str("Immune"));
    }
    Ok(if o.get("GuardSkillActive").truthy() {
        LuaV::str("Guard")
    } else {
        LuaV::Nil
    })
}

/// `o.LightningMaximumHitTaken ~= o.ColdMaximumHitTaken or o.LightningMaximumHitTaken ~= o.FireMaximumHitTaken` (:148-150).
fn ele_split<'a>(_v: &LuaV<'a>, o: &ActorView<'a>) -> LuaRes<'a> {
    let l = o.get("LightningMaximumHitTaken");
    Ok(b(ne(&l, &o.get("ColdMaximumHitTaken")) || ne(&l, &o.get("FireMaximumHitTaken"))))
}

/// `v > 0 and o.MeleeEvadeChance ~= o.ProjectileEvadeChance` (:199/:200).
fn evade_split<'a>(v: &LuaV<'a>, o: &ActorView<'a>) -> LuaRes<'a> {
    Ok(b(gt(v, &n(0.0))? && ne(&o.get("MeleeEvadeChance"), &o.get("ProjectileEvadeChance"))))
}

/// :153/:154 `warnFunc = function(v,o) return v < o.Life and "You do not have enough Accuracy for Precise Technique" end`.
fn accuracy_warn<'a>(v: &LuaV<'a>, o: &ActorView<'a>) -> LuaRes<'a> {
    Ok(if lt(v, &o.get("Life"))? {
        LuaV::str("You do not have enough Accuracy for Precise Technique")
    } else {
        b(false)
    })
}

// --- displayStats (BuildDisplayStats.lua:50-248), one entry per Lua line --------------------------------------------------------

/// `displayStats`.
pub static DISPLAY_STATS: &[DisplayStat] = &[
    /* :51 */ s("ActiveMinionLimit", "Active Minion Limit", "d"),
    /* :52 */ dmg("AverageHit", "Average Hit"),
    /* :53 */ dmg("PvpAverageHit", "PvP Average Hit").flag("isPvP"),
    /* :54 */ dmg("AverageDamage", "Average Damage").flag("attack"),
    /* :55 */
    dmg("AverageDamage", "Average Damage")
        .flag("monsterExplode")
        .cond(|_v, o| Ok(b(ne(&o.get("HitChance"), &n(100.0))))),
    // :56 `o.AverageBurstHits and o.AverageBurstHits > 1 and v > 0`
    dmg("AverageBurstDamage", "Average Burst Damage").cond(|v, o| {
        let h = o.get("AverageBurstHits");
        if !h.truthy() {
            return Ok(h);
        }
        if !gt(&h, &n(1.0))? {
            return Ok(b(false));
        }
        Ok(b(gt(v, &n(0.0))?))
    }),
    /* :57 */ dmg("PvpAverageDamage", "PvP Average Damage").flag("attackPvP"),
    /* :58 */
    s("Speed", "Attack Rate", ".2f")
        .cp()
        .flag("attack")
        .cond(speed_cond)
        .bd("Speed")
        .mods(&["Speed"])
        .ignored(&["Inc. Cast Speed", "More Cast Speed"]),
    /* :59 */
    s("KineticFusilladeMaxEffectiveAPS", "Max Eff. KF Attack Rate", ".2f")
        .cp()
        .flag("attack")
        .cond(positive),
    /* :60 */ s("Speed", "Cast Rate", ".2f").cp().flag("spell").cond(speed_cond).mods(&["Speed"]),
    /* :61 */
    s("Speed", "Effective Trigger Rate", ".2f")
        .cp()
        .not_flag("skipEffectiveRate")
        .cond(trigger_cond),
    /* :62 */ s("WarcryCastTime", "Cast Time", ".2fs").cp().lower().flag("warcry"),
    /* :63 */ s("HitSpeed", "Hit Rate", ".2f").cp().cond(not_trigger),
    /* :64 */
    s("HitTime", "Channel Time", ".2fs")
        .cp()
        .flag("channelRelease")
        .lower()
        .cond(not_trigger),
    /* :65 */ s("ChannelTimeToTrigger", "Channel Time", ".2fs").cp().lower(),
    /* :66 */ s("TrapThrowingTime", "Trap Throwing Time", ".2fs").cp().lower(),
    /* :67 */ s("TrapCooldown", "Trap Cooldown", ".3fs").lower(),
    /* :68 */ s("MineLayingTime", "Mine Throwing Time", ".2fs").cp().lower(),
    /* :69 */ s("TrapThrowCount", "Avg. Traps per Throw", ".2f"),
    /* :70 */ s("MineThrowCount", "Avg. Mines per Throw", ".2f"),
    /* :71 */ s("TotemPlacementTime", "Totem Placement Time", ".2fs")
        .cp()
        .lower()
        .cond(not_trigger),
    /* :72 */ s("PreEffectiveCritChance", "Crit Chance", ".2f%%")
        .flag("hit")
        .bd("PreEffectiveCritChance"),
    /* :73 */
    s("CritChance", "Effective Crit Chance", ".2f%%")
        .flag("hit")
        .cond(|v, o| Ok(b(ne(v, &o.get("PreEffectiveCritChance")))))
        .bd("CritChance"),
    /* :74 */
    s("CritMultiplier", "Crit Multiplier", "d%%")
        .pc()
        .cond(|_v, o| Ok(b(gt(&or0(o.get("CritChance")), &n(0.0))?)))
        .bd("CritMultiplier"),
    /* :75 */ s("HitChance", "Hit Chance", ".0f%%").flag("attack").bd("HitChance"),
    /* :76 */ s("HitChance", "Hit Chance", ".0f%%").cond(|_v, o| Ok(o.get("enemyHasSpellBlock"))),
    /* :77 */ dmg("TotalDPS", "Hit DPS").flag("notAverage"),
    /* :78 */ dmg("PvpTotalDPS", "PvP Hit DPS").flag("notAveragePvP"),
    /* :79 */ dmg("TotalDPS", "Hit DPS").flag("showAverage").cond(trigger_cond),
    /* :80 */ dmg("TotalDot", "DoT DPS"),
    /* :81 */
    dmg("WithDotDPS", "Total DPS inc. DoT")
        .flag("notAverage")
        .cond(|v, o| ne_total_and_zero(v, o, &["PoisonDPS", "IgniteDPS", "ImpaleDPS", "BleedDPS"])),
    /* :82 */ dmg("BleedDPS", "Bleed DPS").warn(|v, _o| cap_warn(v, "Bleed DPS exceeds in game limit")),
    /* :83 */
    dmg("CorruptingBloodDPS", "Corrupting Blood DPS").warn(|v, _o| cap_warn(v, "Corrupting Blood DPS exceeds in game limit")),
    /* :84 */ dmg("BleedDamage", "Total Damage per Bleed").flag("showAverage"),
    /* :85 */
    dmg("WithBleedDPS", "Total DPS inc. Bleed")
        .flag("notAverage")
        .cond(|v, o| ne_total_and_zero(v, o, &["TotalDot", "PoisonDPS", "ImpaleDPS", "IgniteDPS"])),
    /* :86 */ dmg("IgniteDPS", "Ignite DPS").warn(|v, _o| cap_warn(v, "Ignite DPS exceeds in game limit")),
    /* :87 */ dmg("IgniteDamage", "Total Damage per Ignite").flag("showAverage"),
    /* :88 */ dmg("BurningGroundDPS", "Burning Ground DPS").warn(|v, _o| cap_warn(v, "Burning Ground DPS exceeds in game limit")),
    /* :89 */
    dmg("MirageBurningGroundDPS", "Mirage Burning Ground DPS")
        .cond(|v, o| Ok(b(ne(v, &o.get("BurningGroundDPS")))))
        .warn(|v, _o| cap_warn(v, "Mirage Burning Ground DPS exceeds in game limit")),
    /* :90 */
    dmg("WithIgniteDPS", "Total DPS inc. Ignite")
        .flag("notAverage")
        .cond(|v, o| ne_total_and_zero(v, o, &["TotalDot", "PoisonDPS", "ImpaleDPS", "BleedDPS"])),
    /* :91 */ dmg("WithIgniteAverageDamage", "Average Dmg. inc. Ignite"),
    /* :92 */ dmg("PoisonDPS", "Single Poison DPS").warn(|v, _o| cap_warn(v, "Poison DPS exceeds in game limit")),
    /* :93 */ dmg("CausticGroundDPS", "Caustic Ground DPS").warn(|v, _o| cap_warn(v, "Caustic Ground DPS exceeds in game limit")),
    /* :94 */
    dmg("MirageCausticGroundDPS", "Mirage Caustic Ground DPS")
        .cond(|v, o| Ok(b(ne(v, &o.get("CausticGroundDPS")))))
        .warn(|v, _o| cap_warn(v, "Mirage Caustic Ground DPS exceeds in game limit")),
    /* :95 */ dmg("PoisonDamage", "Total Damage per Poison"),
    // :96 `flag = "poison", flag = "notAverage"` -> the last one
    dmg("WithPoisonDPS", "Total DPS inc. Poison")
        .flag("notAverage")
        .cond(|v, o| ne_total_and_zero(v, o, &["TotalDot", "IgniteDPS", "ImpaleDPS", "BleedDPS"])),
    /* :97 */ dmg("DecayDPS", "Decay DPS"),
    // :98
    dmg("TotalDotDPS", "Total DoT DPS")
        .cond(|v, o| {
            let show = o.get("showTotalDotDPS");
            if show.truthy() {
                return Ok(show);
            }
            let ign = {
                let t = o.get("TotalIgniteDPS");
                if t.truthy() { t } else { o.get("IgniteDPS") }
            };
            Ok(b(ne(v, &o.get("TotalDot"))
                && ne(v, &o.get("TotalPoisonDPS"))
                && ne(v, &o.get("CausticGroundDPS"))
                && ne(v, &ign)
                && ne(v, &o.get("BurningGroundDPS"))
                && ne(v, &o.get("BleedDPS"))
                && ne(v, &o.get("CorruptingBloodDPS"))
                && ne(v, &o.get("MirageCausticGroundDPS"))
                && ne(v, &o.get("MirageBurningGroundDPS"))))
        })
        .warn(|v, _o| cap_warn(v, "DoT DPS exceeds in game limit")),
    // :99-102 `flag = "impale", flag = "<x>"` -> the last one
    /* :99 */ dmg("ImpaleDPS", "Impale Damage").flag("showAverage"),
    /* :100 */
    dmg("WithImpaleDPS", "Damage inc. Impale")
        .flag("showAverage")
        .cond(|v, o| ne_total_and_zero(v, o, &["TotalDot", "IgniteDPS", "PoisonDPS", "BleedDPS"])),
    /* :101 */ dmg("ImpaleDPS", "Impale DPS").flag("notAverage"),
    /* :102 */
    dmg("WithImpaleDPS", "Total DPS inc. Impale")
        .flag("notAverage")
        .cond(|v, o| ne_total_and_zero(v, o, &["TotalDot", "IgniteDPS", "PoisonDPS", "BleedDPS"])),
    /* :103 */ dmg("MirageDPS", "Total Mirage DPS").flag("mirageArcher").cond(positive),
    /* :104 */ dmg("MirageDPS", "Total Wisp DPS").flag("wisp").cond(positive),
    /* :105 */ dmg("CullingDPS", "Culling DPS").cond(|_v, o| own_positive(o, "CullingDPS")),
    /* :106 */ dmg("ReservationDPS", "Reservation DPS").cond(|_v, o| own_positive(o, "ReservationDPS")),
    // :107 `v ~= ((o.TotalDPS or 0) + (o.TotalDot or 0)) and v ~= o.WithImpaleDPS and ( o.showTotalDotDPS or ( v ~= o.WithPoisonDPS and
    // v ~= o.WithIgniteDPS and v ~= o.WithBleedDPS ) )`
    dmg("CombinedDPS", "Combined DPS").flag("notAverage").cond(|v, o| {
        let sum = crate::lua::arith(&or0(o.get("TotalDPS")))? + crate::lua::arith(&or0(o.get("TotalDot")))?;
        if !(ne(v, &n(sum)) && ne(v, &o.get("WithImpaleDPS"))) {
            return Ok(b(false));
        }
        let show = o.get("showTotalDotDPS");
        if show.truthy() {
            return Ok(show);
        }
        Ok(b(ne(v, &o.get("WithPoisonDPS"))
            && ne(v, &o.get("WithIgniteDPS"))
            && ne(v, &o.get("WithBleedDPS"))))
    }),
    // :108 `(v ~= o.AverageDamage and (o.TotalDot or 0) == 0) and (v ~= o.WithPoisonDPS or v ~= o.WithIgniteDPS or v ~= o.WithBleedDPS)`
    dmg("CombinedAvg", "Combined Total Damage").flag("showAverage").cond(|v, o| {
        Ok(b((ne(v, &o.get("AverageDamage")) && zero(o, "TotalDot"))
            && (ne(v, &o.get("WithPoisonDPS"))
                || ne(v, &o.get("WithIgniteDPS"))
                || ne(v, &o.get("WithBleedDPS")))))
    }),
    /* :109 */ s("ExplodeChance", "Total Explode Chance", ".0f%%"),
    /* :110 */ s("CombinedAvgToMonsterLife", "Enemy Life Equivalent", ".1f%%"),
    /* :111 */ s("Cooldown", "Skill Cooldown", ".3fs").not_flag("skipEffectiveRate").lower(),
    /* :112 */ s("SealCooldown", "Seal Gain Frequency", ".2fs").lower(),
    /* :113 */ s("SealMax", "Max Number of Seals", "d"),
    /* :114 */ s("TimeMaxSeals", "Time to Gain Max Seals", ".2fs").lower(),
    /* :115 */ s("AreaOfEffectRadiusMetres", "AoE Radius", ".1fm")
        .bd("AreaOfEffectRadius")
        .mods(&["AreaOfEffect"]),
    /* :116 */ s("BrandAttachmentRangeMetre", "Attachment Range", ".1fm").flag("brand"),
    /* :117 */ s("BrandTicks", "Activations per Brand", "d").flag("brand"),
    /* :118 */ cost("ManaCost", "Mana Cost", "d", MANA, |_v, o| Ok(o.get("ManaHasCost"))).pool("ManaUnreserved"),
    /* :119 */
    cost("ManaPercentCost", "Mana Cost", "d%%", MANA, |_v, o| Ok(o.get("ManaPercentHasCost"))).pool("ManaUnreservedPercent"),
    /* :120 */
    cost("ManaPerSecondCost", "Mana Cost per second", ".2f", MANA, |_v, o| {
        Ok(o.get("ManaPerSecondHasCost"))
    })
    .pool("ManaUnreserved"),
    /* :121 */
    cost("ManaPercentPerSecondCost", "Mana Cost per second", ".2f%%", MANA, |_v, o| {
        Ok(o.get("ManaPercentPerSecondHasCost"))
    })
    .pool("ManaUnreservedPercent"),
    /* :122 */ cost("LifeCost", "Life Cost", "d", LIFE, |_v, o| Ok(o.get("LifeHasCost"))).pool("LifeUnreserved"),
    /* :123 */
    cost("LifePercentCost", "Life Cost", "d%%", LIFE, |_v, o| Ok(o.get("LifePercentHasCost"))).pool("LifeUnreservedPercent"),
    /* :124 */
    cost("LifePerSecondCost", "Life Cost per second", ".2f", LIFE, |_v, o| {
        Ok(o.get("LifePerSecondHasCost"))
    })
    .pool("LifeUnreserved"),
    /* :125 */
    cost("LifePercentPerSecondCost", "Life Cost per second", ".2f%%", LIFE, |_v, o| {
        Ok(o.get("LifePercentPerSecondHasCost"))
    })
    .pool("LifeUnreservedPercent"),
    /* :126 */ cost("ESCost", "Energy Shield Cost", "d", ES, |_v, o| Ok(o.get("ESHasCost"))).pool("EnergyShield"),
    /* :127 */
    cost("ESPerSecondCost", "ES Cost per second", ".2f", ES, |_v, o| {
        Ok(o.get("ESPerSecondHasCost"))
    })
    .pool("EnergyShield"),
    /* :128 (no pool) */
    cost("ESPercentPerSecondCost", "ES Cost per second", ".2f%%", ES, |_v, o| {
        Ok(o.get("ESPercentPerSecondHasCost"))
    }),
    /* :129 */ cost("RageCost", "Rage Cost", "d", RAGE, |_v, o| Ok(o.get("RageHasCost"))).pool("Rage"),
    /* :130 */
    cost("RagePerSecondCost", "Rage Cost per second", ".2f", RAGE, |_v, o| {
        Ok(o.get("RagePerSecondHasCost"))
    })
    .pool("Rage"),
    /* :131 */ cost("SoulCost", "Soul Cost", "d", RAGE, |_v, o| Ok(o.get("SoulHasCost"))).pool("Soul"),
    /* :132 */ SEP,
    /* :133 */ s("Str", "Strength", "d").color(STRENGTH).bd("Str"),
    /* :134 */
    s("ReqStr", "Strength Required", "d")
        .color(STRENGTH)
        .lower()
        .cond(|v, o| Ok(b(gt(v, &o.get("Str"))?)))
        .warn(|_v, o| req_warn(o, "Str", "Strength")),
    /* :135 */ s("Dex", "Dexterity", "d").color(DEXTERITY).bd("Dex"),
    /* :136 */
    s("ReqDex", "Dexterity Required", "d")
        .color(DEXTERITY)
        .lower()
        .cond(|v, o| Ok(b(gt(v, &o.get("Dex"))?)))
        .warn(|_v, o| req_warn(o, "Dex", "Dexterity")),
    /* :137 */ s("Int", "Intelligence", "d").color(INTELLIGENCE).bd("Int"),
    /* :138 */
    s("ReqInt", "Intelligence Required", "d")
        .color(INTELLIGENCE)
        .lower()
        .cond(|v, o| Ok(b(gt(v, &o.get("Int"))?)))
        .warn(|_v, o| req_warn(o, "Int", "Intelligence")),
    /* :139 */ s("Omni", "Omniscience", "d").color(RARE),
    /* :140 */
    s("ReqOmni", "Omniscience Required", "d")
        .color(RARE)
        .lower()
        .cond(|v, o| Ok(b(gt(v, &or0(o.get("Omni")))?)))
        .warn(|_v, o| req_warn(o, "Omni", "Omniscience")),
    /* :141 */ SEP,
    /* :142 */ s("Devotion", "Devotion", "d").color(RARE),
    /* :143 */ SEP,
    /* :144 */
    s("TotalEHP", "Effective Hit Pool", ".0f")
        .suffix(Suffix::Str("Guard"))
        .cv()
        .cp()
        .suffix_cond(|_v, o| Ok(o.get("GuardSkillActive"))),
    /* :145 */ s("PvPTotalTakenHit", "PvP Hit Taken", ".1f").flag("isPvP").cv().lower(),
    /* :146 */
    s("PhysicalMaximumHitTaken", "Phys Max Hit", ".0f")
        .cv()
        .color(PHYS)
        .cp()
        .suffix(Suffix::Fn(max_hit_suffix)),
    /* :147 */
    s("LightningMaximumHitTaken", "Elemental Max Hit", ".0f")
        .cv()
        .color(LIGHTNING)
        .cp()
        .suffix(Suffix::Fn(max_hit_suffix))
        .cond(|_v, o| {
            let l = o.get("LightningMaximumHitTaken");
            Ok(b(eq(&l, &o.get("ColdMaximumHitTaken")) && eq(&l, &o.get("FireMaximumHitTaken"))))
        }),
    /* :148 */
    s("FireMaximumHitTaken", "Fire Max Hit", ".0f")
        .cv()
        .color(FIRE)
        .cp()
        .suffix(Suffix::Fn(max_hit_suffix))
        .cond(ele_split),
    /* :149 */
    s("ColdMaximumHitTaken", "Cold Max Hit", ".0f")
        .cv()
        .color(COLD)
        .cp()
        .suffix(Suffix::Fn(max_hit_suffix))
        .cond(ele_split),
    /* :150 */
    s("LightningMaximumHitTaken", "Lightning Max Hit", ".0f")
        .cv()
        .color(LIGHTNING)
        .cp()
        .suffix(Suffix::Fn(max_hit_suffix))
        .cond(ele_split),
    /* :151 */
    s("ChaosMaximumHitTaken", "Chaos Max Hit", ".0f")
        .cv()
        .color(CHAOS)
        .cp()
        .suffix(Suffix::Fn(max_hit_suffix)),
    /* :152 */ SEP,
    /* :153 */
    s("MainHand", "MH Accuracy", "d")
        .child("Accuracy")
        .cond(|_v, o| Ok(o.get("PreciseTechnique")))
        .warn(accuracy_warn)
        .warn_color(),
    /* :154 */
    s("OffHand", "OH Accuracy", "d")
        .child("Accuracy")
        .cond(|_v, o| Ok(o.get("PreciseTechnique")))
        .warn(accuracy_warn)
        .warn_color(),
    /* :155 */
    s("Life", "Total Life", "d")
        .cv()
        .color(LIFE)
        .cp()
        .mods(&["Life"])
        .ignored(&["Base from Gear", "Inc. from Tree"]),
    /* :156 */
    s("Spec:LifeInc", "%Inc Life from Tree", "d%%")
        .color(LIFE)
        .cond(|v, o| Ok(b(gt(v, &n(0.0))? && gt(&o.get("Life"), &n(1.0))?))),
    /* :157 */
    s("LifeUnreserved", "Unreserved Life", "d")
        .color(LIFE)
        .cond(|v, o| Ok(b(lt(v, &o.get("Life"))?)))
        .cp()
        .warn(|v, _o| {
            Ok(if le(v, &n(0.0))? {
                LuaV::str("Your unreserved Life is below 1")
            } else {
                b(false)
            })
        }),
    /* :158 */
    s("LifeRecoverable", "Life Recoverable", "d")
        .color(LIFE)
        .cond(|v, o| Ok(b(lt(v, &o.get("LifeUnreserved"))?))),
    /* :159 */
    s("LifeUnreservedPercent", "Unreserved Life", "d%%")
        .color(LIFE)
        .cond(|v, _o| Ok(b(lt(v, &n(100.0))?))),
    /* :160 */
    s("LifeRegenRecovery", "Life Regen", ".1f")
        .color(LIFE)
        .cond(|_v, o| regen_pair(o, "LifeRecovery", "LifeRegenRecovery", true))
        .bd("LifeRegenRecovery"),
    /* :161 */
    s("LifeRegenRecovery", "Life Recovery", ".1f")
        .color(LIFE)
        .cond(|_v, o| regen_pair(o, "LifeRecovery", "LifeRegenRecovery", false))
        .bd("LifeRegenRecovery"),
    /* :162 */ s("LifeRecharge", "Life Recharge", ".1f").color(LIFE).cond(positive),
    /* :163 */ s("LifeLeechGainRate", "Life Leech/On Hit Rate", ".1f")
        .color(LIFE)
        .cp()
        .bd("LifeLeech"),
    /* :164 */ s("LifeLeechGainPerHit", "Life Leech/Gain per Hit", ".1f").color(LIFE).cp(),
    /* :165 */ SEP,
    /* :166 */
    s("Mana", "Total Mana", "d")
        .cv()
        .color(MANA)
        .cp()
        .mods(&["Mana"])
        .ignored(&["Base from Gear", "Inc. from Tree"]),
    /* :167 */ s("Spec:ManaInc", "%Inc Mana from Tree", "d%%").color(MANA),
    /* :168 */
    s("ManaUnreserved", "Unreserved Mana", "d")
        .color(MANA)
        .cond(|v, o| Ok(b(lt(v, &o.get("Mana"))?)))
        .cp()
        .warn(|v, _o| {
            Ok(if lt(v, &n(0.0))? {
                LuaV::str("Your unreserved Mana is negative")
            } else {
                b(false)
            })
        }),
    /* :169 */
    s("ManaUnreservedPercent", "Unreserved Mana", "d%%")
        .color(MANA)
        .cond(|v, _o| Ok(b(lt(v, &n(100.0))?)))
        .bd("ManaReserved"),
    /* :170 */
    s("ManaRegenRecovery", "Mana Regen", ".1f")
        .color(MANA)
        .cond(|_v, o| regen_pair(o, "ManaRecovery", "ManaRegenRecovery", true)),
    /* :171 */
    s("ManaRegenRecovery", "Mana Recovery", ".1f")
        .color(MANA)
        .cond(|_v, o| regen_pair(o, "ManaRecovery", "ManaRegenRecovery", false)),
    /* :172 */ s("ManaLeechGainRate", "Mana Leech/On Hit Rate", ".1f").color(MANA).cp(),
    /* :173 */ s("ManaLeechGainPerHit", "Mana Leech/Gain per Hit", ".1f").color(MANA).cp(),
    /* :174 */ SEP,
    /* :177 */
    s("EnergyShield", "Energy Shield", "d")
        .cv()
        .color(ES)
        .cp()
        .mods(&["EnergyShield", "Defences"])
        .ignored(DEFENSE_IGNORED_SECTIONS),
    /* :178 */ s("EnergyShieldRecoveryCap", "Recoverable ES", "d")
        .color(ES)
        .cond(|_v, o| Ok(o.get("CappingES"))),
    /* :179 */ s("Spec:EnergyShieldInc", "%Inc ES from Tree", "d%%").color(ES),
    /* :180 */
    s("EnergyShieldRegenRecovery", "ES Regen", ".1f")
        .color(ES)
        .cond(|_v, o| regen_pair(o, "EnergyShieldRecovery", "EnergyShieldRegenRecovery", true)),
    /* :181 */
    s("EnergyShieldRegenRecovery", "ES Recovery", ".1f")
        .color(ES)
        .cond(|_v, o| regen_pair(o, "EnergyShieldRecovery", "EnergyShieldRegenRecovery", false)),
    /* :182 */ s("EnergyShieldLeechGainRate", "ES Leech/On Hit Rate", ".1f").color(ES).cp(),
    /* :183 */ s("EnergyShieldLeechGainPerHit", "ES Leech/Gain per Hit", ".1f").color(ES).cp(),
    /* :184 */ SEP,
    /* :185 */
    s("Ward", "Ward", "d")
        .cv()
        .color(WARD)
        .cp()
        .mods(&["Ward"])
        .ignored(DEFENSE_IGNORED_SECTIONS),
    /* :186 */ SEP,
    /* :187 */ s("Rage", "Rage", "d").color(RAGE).cp(),
    /* :188 */ s("RageRegenRecovery", "Rage Regen", ".1f").color(RAGE).cp(),
    /* :189 */ SEP,
    /* :190 */ s("TotalBuildDegen", "Total Degen", ".1f").lower(),
    /* :191 */ s("TotalNetRegen", "Total Net Recovery", "+.1f"),
    /* :192 */ s("NetLifeRegen", "Net Life Recovery", "+.1f").color(LIFE),
    /* :193 */ s("NetManaRegen", "Net Mana Recovery", "+.1f").color(MANA),
    /* :194 */ s("NetEnergyShieldRegen", "Net ES Recovery", "+.1f").color(ES),
    /* :195 */ SEP,
    /* :196 */
    s("Evasion", "Evasion rating", "d")
        .color(EVASION)
        .cp()
        .mods(&["Evasion", "ArmourAndEvasion", "Defences"])
        .ignored(DEFENSE_IGNORED_SECTIONS),
    /* :197 */ s("Spec:EvasionInc", "%Inc Evasion from Tree", "d%%").color(EVASION),
    /* :198 */
    s("MeleeEvadeChance", "Evade Chance", "d%%")
        .color(EVASION)
        .cond(|v, o| Ok(b(gt(v, &n(0.0))? && eq(&o.get("MeleeEvadeChance"), &o.get("ProjectileEvadeChance"))))),
    /* :199 */ s("MeleeEvadeChance", "Melee Evade Chance", "d%%").color(EVASION).cond(evade_split),
    /* :200 */ s("ProjectileEvadeChance", "Projectile Evade Chance", "d%%")
        .color(EVASION)
        .cond(evade_split),
    /* :201 */ SEP,
    /* :202 */
    s("Armour", "Armour", "d")
        .cp()
        .mods(&["Armour", "ArmourAndEvasion", "Defences", "ArmourDefense"])
        .ignored(DEFENSE_IGNORED_SECTIONS),
    /* :203 */ s("Spec:ArmourInc", "%Inc Armour from Tree", "d%%"),
    /* :204 */ s("PhysicalDamageReduction", "Phys. Damage Reduction", "d%%").cond(always),
    /* :205 */ SEP,
    /* :206 */ s("EffectiveBlockChance", "Block Chance", ".3f%%")
        .over_cap("BlockChanceOverCap")
        .bd("BlockChance"),
    /* :207 */
    s("EffectiveSpellBlockChance", "Spell Block Chance", ".3f%%")
        .over_cap("SpellBlockChanceOverCap")
        .bd("SpellBlockChance"),
    /* :208 */ s("AttackDodgeChance", "Attack Dodge Chance", "d%%").over_cap("AttackDodgeChanceOverCap"),
    /* :209 */ s("SpellDodgeChance", "Spell Dodge Chance", "d%%").over_cap("SpellDodgeChanceOverCap"),
    /* :210 */
    s("EffectiveSpellSuppressionChance", "Spell Suppression Chance", ".2f%%")
        .over_cap("SpellSuppressionChanceOverCap")
        .bd("")
        .mods(&["SpellSuppressionChance"]),
    /* :211 */ SEP,
    /* :212 */
    s("FireResist", "Fire Resistance", "d%%")
        .color(FIRE)
        .cond(always)
        .over_cap("FireResistOverCap"),
    /* :213 */ s("FireResistOverCap", "Fire Res. Over Max", "d%%").hide(),
    /* :214 */
    s("ColdResist", "Cold Resistance", "d%%")
        .color(COLD)
        .cond(always)
        .over_cap("ColdResistOverCap"),
    /* :215 */ s("ColdResistOverCap", "Cold Res. Over Max", "d%%").hide(),
    /* :216 */
    s("LightningResist", "Lightning Resistance", "d%%")
        .color(LIGHTNING)
        .cond(always)
        .over_cap("LightningResistOverCap"),
    /* :217 */ s("LightningResistOverCap", "Lightning Res. Over Max", "d%%").hide(),
    /* :218 */
    s("ChaosResist", "Chaos Resistance", "d%%")
        .color(CHAOS)
        .over_cap("ChaosResistOverCap")
        .over_cap_cond(|_v, o| Ok(b(!o.get("ChaosInoculation").truthy())))
        .suffix(Suffix::Str("Immune"))
        .suffix_cond(|_v, o| Ok(o.get("ChaosInoculation"))),
    /* :219 */ s("ChaosResistOverCap", "Chaos Res. Over Max", "d%%").hide(),
    /* :220 */ SEP,
    /* :221 */ s("EffectiveMovementSpeedMod", "Movement Speed Modifier", "+d%%")
        .mod_()
        .cond(always),
    // :222-242 commented out upstream
    /* :243 */ SEP,
    /* :244 */ dmg("FullDPS", "Full DPS").color(CURRENCY),
    /* :245 */
    dmg("FullDotDPS", "Full Dot DPS")
        .color(CURRENCY)
        .cond(|v, _o| Ok(b(ge(v, &dot_cap())?)))
        .warn(|_v, _o| Ok(LuaV::str("Full Dot DPS exceeds in game limit"))),
    /* :246 */ SEP,
    /* :247 (no fmt) */
    DisplayStat {
        stat: Some("SkillDPS"),
        label: Some("Skill DPS"),
        ..SEP
    }
    .cv()
    .cond(always),
];

// --- minionDisplayStats (BuildDisplayStats.lua:250-279) -------------------------------------------------------------------------

/// `minionDisplayStats`.
pub static MINION_DISPLAY_STATS: &[DisplayStat] = &[
    /* :251 */ dmg("AverageDamage", "Average Damage"),
    /* :252 */ s("Speed", "Attack/Cast Rate", ".2f").cp().cond(speed_cond),
    /* :253 */ s("HitSpeed", "Hit Rate", ".2f"),
    /* :254 */
    s("Speed", "Effective Trigger Rate", ".2f")
        .cp()
        .not_flag("skipEffectiveRate")
        .cond(trigger_cond),
    /* :255 */ dmg("TotalDPS", "Hit DPS"),
    /* :256 */ dmg("TotalDot", "DoT DPS"),
    /* :257 */
    dmg("WithDotDPS", "Total DPS inc. DoT").cond(|v, o| ne_total_and_zero(v, o, &["PoisonDPS", "IgniteDPS", "ImpaleDPS", "BleedDPS"])),
    /* :258 */ dmg("BleedDPS", "Bleed DPS").warn(|v, _o| cap_warn(v, "Minion Bleed DPS exceeds in game limit")),
    /* :259 */
    dmg("WithBleedDPS", "Total DPS inc. Bleed").cond(|v, o| ne_total_and_zero(v, o, &["TotalDot", "PoisonDPS", "ImpaleDPS", "IgniteDPS"])),
    /* :260 */ dmg("IgniteDPS", "Ignite DPS").warn(|v, _o| cap_warn(v, "Minion Ignite DPS exceeds in game limit")),
    /* :261 */
    dmg("WithIgniteDPS", "Total DPS inc. Ignite").cond(|v, o| ne_total_and_zero(v, o, &["TotalDot", "PoisonDPS", "ImpaleDPS", "BleedDPS"])),
    /* :262 */ dmg("PoisonDPS", "Poison DPS").warn(|v, _o| cap_warn(v, "Minion Poison dps exceeds in game limit")),
    /* :263 */ dmg("PoisonDamage", "Total Damage per Poison"),
    /* :264 */
    dmg("WithPoisonDPS", "Total DPS inc. Poison").cond(|v, o| ne_total_and_zero(v, o, &["TotalDot", "IgniteDPS", "ImpaleDPS", "BleedDPS"])),
    /* :265 */ dmg("DecayDPS", "Decay DPS"),
    /* :266 */
    dmg("TotalDotDPS", "Total DoT DPS")
        .cond(|v, o| {
            let ign = {
                let t = o.get("TotalIgniteDPS");
                if t.truthy() { t } else { o.get("IgniteDPS") }
            };
            Ok(b(ne(v, &o.get("TotalDot"))
                && ne(v, &o.get("ImpaleDPS"))
                && ne(v, &o.get("TotalPoisonDPS"))
                && ne(v, &ign)
                && ne(v, &o.get("BleedDPS"))))
        })
        .warn(|v, _o| cap_warn(v, "Minion DoT DPS exceeds in game limit")),
    /* :267 */ dmg("ImpaleDPS", "Impale DPS").flag("impale"),
    /* :268 */
    dmg("WithImpaleDPS", "Total DPS inc. Impale")
        .flag("impale")
        .cond(|v, o| ne_total_and_zero(v, o, &["TotalDot", "IgniteDPS", "PoisonDPS", "BleedDPS"])),
    /* :269 */ dmg("CullingDPS", "Culling DPS").cond(|_v, o| own_positive(o, "CullingDPS")),
    /* :270 */ dmg("ReservationDPS", "Reservation DPS").cond(|_v, o| own_positive(o, "ReservationDPS")),
    /* :271 */
    dmg("CombinedDPS", "Combined DPS").cond(|v, o| {
        let sum = crate::lua::arith(&or0(o.get("TotalDPS")))? + crate::lua::arith(&or0(o.get("TotalDot")))?;
        Ok(b(ne(v, &n(sum))
            && ne(v, &o.get("WithImpaleDPS"))
            && ne(v, &o.get("WithPoisonDPS"))
            && ne(v, &o.get("WithIgniteDPS"))
            && ne(v, &o.get("WithBleedDPS"))))
    }),
    /* :272 */ s("Cooldown", "Skill Cooldown", ".3fs").not_flag("skipEffectiveRate").lower(),
    /* :273 */
    s("Life", "Total Life", ".1f")
        .cv()
        .color(LIFE)
        .cp()
        .mods(&["Life"])
        .ignored(&["Base from Gear", "Inc. from Tree"]),
    /* :274 */ s("LifeRegenRecovery", "Life Recovery", ".1f").color(LIFE),
    /* :275 */ s("LifeLeechGainRate", "Life Leech/On Hit Rate", ".1f").color(LIFE).cp(),
    /* :276 */
    s("EnergyShield", "Energy Shield", "d")
        .cv()
        .color(ES)
        .cp()
        .mods(&["EnergyShield", "Defences"])
        .ignored(DEFENSE_IGNORED_SECTIONS),
    /* :277 */ s("EnergyShieldRegenRecovery", "ES Recovery", ".1f").color(ES),
    /* :278 */ s("EnergyShieldLeechGainRate", "ES Leech/On Hit Rate", ".1f").color(ES).cp(),
];

/// `extraSaveStats` (:282-291).
pub static EXTRA_SAVE_STATS: &[&str] = &[
    "PowerCharges",
    "PowerChargesMax",
    "FrenzyCharges",
    "FrenzyChargesMax",
    "EnduranceCharges",
    "EnduranceChargesMax",
    "ActiveTotemLimit",
    "ActiveMinionLimit",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_sizes() {
        assert_eq!(DISPLAY_STATS.len(), 174);
        assert_eq!(MINION_DISPLAY_STATS.len(), 28);
    }
}
