//! ModStore.lua:363-971 `EvalMod` -- the 21 tag branches in upstream order, then the global limits. Tags are walked in
//! array order (`ipairs(mod)`, L9); the first failing tag answers nil; tag types without a branch are ignored (MOD-CORE §3).
//! Tag fields are read with Lua semantics: wrong types die with LuaJIT's message and the upstream line (the corpus compares
//! texts). Every `or` is Lua's (0 and "" are true, L4); every min/max is `pob_lua` `min2/max2` with numeric-string coercion
//! (L6, L7).
//!
//! Bug-compat kept verbatim (MOD-CORE §7.5): `tag.div = GetMultiplier(self, tag.divVar, cfg)` (ModStore.lua:401, :499)
//! WRITES into the shared tag object, and every later reader of that tag sees it (`Tag::set_div`).

use std::rc::Rc;

use pob_lua::number::{band, max2, min2};
use pob_lua::string::{lower, upper};
use pob_lua::{Repl, gsub};

use crate::calc_lib::{gem_is_type, get_game_id_from_gem_name};
use crate::store::{GlobalLimits, ModStore, StoreRef};
use crate::types::{ActorRef, ItemLike, Mod, ModCfg, Tag, TagKind};
use crate::value::{LuaResult, Str, Value, arith, check_num, concat_str, err_bad_arg, err_compare, err_op};

fn site(line: u32) -> String {
    format!("Classes/ModStore.lua:{line}")
}

/// `multiplierName[var]` / `conditionName[var]` (ModStore.lua:21-28): the memo's `__index` concatenates, so a non-string,
/// non-number `var` dies there (`:22` multiplier, `:26` condition) -- MOD-CORE §7.12. Deviation (R2.2, accepted): a number
/// `var` is stringified before `conditions[var]`/`multipliers[var]` (Lua would index the number key, ModStore.lua:293/302)
/// -- unreachable in data, every var is a string.
fn var_name(v: &Value, line: u32) -> LuaResult<Str> {
    concat_str(v, &site(line), Some("local 'var'"))
}

/// `for _, x in pairs(t)` / `ipairs(t)` over a list field: the array part only. Deviation (R2.2, accepted): `pairs` would
/// also walk hash keys -- unreachable in data, every list field (varList, skillNameList, ramp, ...) is a sequence.
fn list<'a>(v: &'a Value, line: u32, fname: &str) -> LuaResult<&'a [Value]> {
    match v {
        Value::Table(t) => Ok(&t.array),
        _ => Err(err_bad_arg(&site(line), 1, fname, "table", v)),
    }
}

/// `s:lower()` on a value: a non-string has no methods (`desc` names the operand as LuaJIT does).
fn lower_of(v: &Value, line: u32, desc: &str) -> LuaResult<String> {
    match v {
        Value::Str(s) => Ok(lower(s)),
        _ => Err(err_op(&site(line), "index", Some(desc), v)),
    }
}

/// `(tag.x or dflt)` in arithmetic.
fn or_num(v: &Value, dflt: f64, line: u32, field: &str) -> LuaResult<f64> {
    if v.truthy() {
        arith(v, &site(line), Some(&format!("field '{field}'")))
    } else {
        Ok(dflt)
    }
}

/// `m_min(a, b)` / `m_max(a, b)` with `a` already a number.
fn min_with(a: f64, b: &Value, line: u32) -> LuaResult<f64> {
    Ok(min2(a, check_num(b, &site(line), 2, "min")?))
}

fn max_with(a: f64, b: &Value, line: u32) -> LuaResult<f64> {
    Ok(max2(a, check_num(b, &site(line), 2, "max")?))
}

/// `a < b` where one side is a number: Lua never coerces in comparisons.
fn lt(a: &Value, b: &Value, line: u32) -> LuaResult<bool> {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => Ok(x < y),
        _ => Err(err_compare(&site(line), a, b)),
    }
}

fn le(a: &Value, b: &Value, line: u32) -> LuaResult<bool> {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => Ok(x <= y),
        _ => Err(err_compare(&site(line), a, b)),
    }
}

/// Where a tag looks: the evaluating store itself, or another actor's modDB.
enum Target {
    Me,
    Store(StoreRef),
}

/// The three write-back paths of a scaling branch: `value.mod.value`, `value.value`, scalar `value` -- each with the line
/// of its `* mult + base`, of its `m_min(.., limitTotal)` and of its `m_max(.., limitNegTotal)`.
struct Lines {
    mul: [u32; 3],
    min: [u32; 3],
    max: [u32; 3],
}

const MULTIPLIER_LINES: Lines = Lines {
    mul: [425, 433, 442],
    min: [427, 435, 444],
    max: [430, 438, 447],
};
const PERSTAT_LINES: Lines = Lines {
    mul: [514, 519, 525],
    min: [516, 521, 527],
    max: [0, 0, 0],
};
const PERCENTSTAT_LINES: Lines = Lines {
    mul: [564, 569, 575],
    min: [566, 571, 577],
    max: [0, 0, 0],
};

/// The scaling arithmetic of Multiplier/PerStat/PercentStat: `x * mult + (tag.base or 0)` (ceil'd for PercentStat), then
/// the deferred `limitTotal` (min) / `limitNegTotal` (max).
struct Scale<'a> {
    mult: f64,
    tag: &'a Tag,
    ceil: bool,
    limit_total: Option<Value>,
    limit_neg_total: Option<Value>,
    lines: &'a Lines,
}

impl Scale<'_> {
    fn apply_x(&self, x: f64, path: usize) -> LuaResult<f64> {
        let line = self.lines.mul[path];
        let mut out = x * self.mult + or_num(self.tag.raw("base"), 0.0, line, "base")?;
        if self.ceil {
            out = out.ceil();
        }
        if let Some(l) = &self.limit_total
            && l.truthy()
        {
            out = min_with(out, l, self.lines.min[path])?;
        }
        if let Some(l) = &self.limit_neg_total
            && l.truthy()
        {
            out = max_with(out, l, self.lines.max[path])?;
        }
        Ok(out)
    }

    /// ModStore.lua:422-448: a table value is copied (`copyTable`, here copy-on-write) and the number goes into
    /// `value.mod.value` when there is a nested mod, `value.value` otherwise; a scalar is replaced outright.
    fn apply(&self, value: Value) -> LuaResult<Value> {
        if value.is_table() {
            let mut value = value;
            let nested = value.get("mod");
            if nested.truthy() {
                let mut sub = nested;
                if !sub.is_table() {
                    return Err(err_op(&site(self.lines.mul[0]), "index", Some("field 'mod'"), &sub));
                }
                let x = arith(&sub.get("value"), &site(self.lines.mul[0]), Some("field 'value'"))?;
                sub.set("value", Value::Num(self.apply_x(x, 0)?));
                value.set("mod", sub);
            } else {
                let x = arith(&value.get("value"), &site(self.lines.mul[1]), Some("field 'value'"))?;
                let new = self.apply_x(x, 1)?;
                value.set("value", Value::Num(new));
            }
            return Ok(value);
        }
        let x = arith(&value, &site(self.lines.mul[2]), Some("local 'value'"))?;
        Ok(Value::Num(self.apply_x(x, 2)?))
    }
}

/// `itemSlot:gsub("%d$", {["1"] = "2", ["2"] = "1"})` -- first result only.
fn swap_slot_digit(slot: &str) -> String {
    let mut repl = |s: &[u8], c: &pob_lua::Captures| -> Option<Vec<u8>> {
        let m = c.get(0).and_then(|c| c.bytes(s)).unwrap_or_default();
        match m {
            b"1" => Some(b"2".to_vec()),
            b"2" => Some(b"1".to_vec()),
            _ => None,
        }
    };
    let (out, _) = gsub(slot.as_bytes(), b"%d$", Repl::Fn(&mut repl), None).expect("constant pattern");
    String::from_utf8(out).expect("ASCII digit swap keeps UTF-8")
}

/// ModStore.lua:693 `tag.itemSlot:lower():gsub("(%l)(%w*)", upper-first):gsub('^%s*(.-)%s*$', '%1')`.
fn normalize_item_slot(raw: &str) -> String {
    let low = lower(raw);
    let mut cap = |s: &[u8], c: &pob_lua::Captures| -> Option<Vec<u8>> {
        let a = c.get(0).and_then(|c| c.bytes(s)).unwrap_or_default();
        let b = c.get(1).and_then(|c| c.bytes(s)).unwrap_or_default();
        let mut out = upper(std::str::from_utf8(a).unwrap_or_default()).into_bytes();
        out.extend_from_slice(b);
        Some(out)
    };
    let (s1, _) = gsub(low.as_bytes(), b"(%l)(%w*)", Repl::Fn(&mut cap), None).expect("constant pattern");
    let (s2, _) = gsub(&s1, b"^%s*(.-)%s*$", Repl::Str(b"%1"), None).expect("constant pattern");
    String::from_utf8(s2).expect("ASCII case mapping keeps UTF-8")
}

/// `item.name:match("Kalandra's Touch")` (no magic characters in the pattern).
fn is_kalandra(item: &Rc<dyn ItemLike>, line: u32) -> LuaResult<bool> {
    match item.name() {
        Some(n) => Ok(n.contains("Kalandra's Touch")),
        None => Err(err_op(&site(line), "index", Some("field 'name'"), &Value::Nil)),
    }
}

type Items = Option<Vec<(Str, Rc<dyn ItemLike>)>>;

/// `pairs(items)` where `items` may be nil (allSlots over an actor without an item list).
fn pairs_items(items: &Items, line: u32) -> LuaResult<&[(Str, Rc<dyn ItemLike>)]> {
    items
        .as_deref()
        .ok_or_else(|| err_bad_arg(&site(line), 1, "pairs", "table", &Value::Nil))
}

impl ModStore {
    fn on<R>(&self, t: &Target, f: impl FnOnce(&ModStore) -> R) -> R {
        match t {
            Target::Me => f(self),
            Target::Store(s) => f(&s.borrow()),
        }
    }

    /// ModStore.lua:48-54 `getActor(self, actorType)`. Deviation (R2.2, accepted): a non-string type answers nil (Lua reads
    /// `self.actor[x]` with that key) -- unreachable in data, every actor field is a string.
    fn get_actor(&self, actor_type: &Value) -> Option<ActorRef> {
        let actor = self.actor.borrow();
        let key = actor_type.as_str()?;
        if key == "player" {
            // `a.player or (a.parent and a.parent.player) or (a.enemy and a.enemy.player)`
            if let Some(p) = actor.actors.get("player") {
                return Some(p.clone());
            }
            for via in ["parent", "enemy"] {
                if let Some(a) = actor.actors.get(via)
                    && let Some(p) = a.borrow().actors.get("player")
                {
                    return Some(p.clone());
                }
            }
            return None;
        }
        actor.actors.get(key).cloned()
    }

    /// `actor.modDB` of a found actor. Upstream assigns a nil modDB and dies at its first use inside GetMultiplier/GetStat
    /// (`attempt to index local 'self'`). Deviation (R2.2, accepted): we die at once with that text and ModStore.lua:303 --
    /// unreachable in data, every actor carries a modDB.
    fn actor_target(actor: &ActorRef) -> LuaResult<Target> {
        match &actor.borrow().mod_db {
            Some(db) => Ok(Target::Store(db.clone())),
            None => Err(err_op(&site(303), "index", Some("local 'self'"), &Value::Nil)),
        }
    }

    /// `GetMultiplier(target, var, cfg)` as EvalMod calls it (noMod = nil).
    fn mult_of(&self, t: &Target, var: &Value, cfg: Option<&ModCfg>) -> LuaResult<Value> {
        let name = var_name(var, 22)?;
        self.on(t, |s| s.get_multiplier(&name, cfg, false))
    }

    /// `GetCondition(target, var, cfg)` as EvalMod calls it.
    fn cond_of(&self, t: &Target, var: &Value, cfg: Option<&ModCfg>) -> LuaResult<bool> {
        let name = var_name(var, 26)?;
        Ok(self.on(t, |s| s.get_condition(&name, cfg, false))? == Some(true))
    }

    /// `GetStat(target, stat, cfg)`: every special case compares the stat NAME, and the tail reads `output[stat]` /
    /// `skillStats[stat]` -- string-keyed maps -- so a non-string stat answers 0. The answer is upstream's, nil included
    /// (ModStore.lua:349); the arithmetic error a nil causes is raised at the call site, as LuaJIT does.
    fn stat_of(&self, t: &Target, stat: &Value, cfg: Option<&ModCfg>) -> LuaResult<Value> {
        match stat.as_str() {
            Some(s) => self.on(t, |st| st.get_stat(s, cfg)),
            None => Ok(Value::Num(0.0)),
        }
    }

    /// ModStore.lua:400-403 / :498-501: `if tag.divVar then tag.div = GetMultiplier(self, tag.divVar, cfg) end`, then
    /// `(tag.div or 1)`. The write goes into the shared tag object (MOD-CORE §7.5 bug-compat, see `Tag`).
    fn divisor(&self, tag: &Tag, cfg: Option<&ModCfg>, line: u32) -> LuaResult<f64> {
        let div_var = tag.raw("divVar");
        if div_var.truthy() {
            tag.set_div(self.mult_of(&Target::Me, div_var, cfg)?);
        }
        or_num(&tag.get("div"), 1.0, line, "div")
    }

    /// ModStore.lua:363 `EvalMod(mod, cfg, globalLimits)`.
    pub fn eval_mod(&self, m: &Mod, cfg: Option<&ModCfg>, global_limits: Option<&mut GlobalLimits>) -> LuaResult<Value> {
        let mut value = m.value.clone();
        for tag in &m.tags {
            let pass = match tag.kind() {
                TagKind::Multiplier => {
                    // ModStore.lua:369-449
                    let mut target = Target::Me;
                    let mut limit_target = Target::Me;
                    if tag.raw("limitActor").truthy() {
                        match self.get_actor(tag.raw("limitActor")) {
                            Some(a) => limit_target = Self::actor_target(&a)?,
                            None => return Ok(Value::Nil),
                        }
                    }
                    if tag.raw("actor").truthy() {
                        match self.get_actor(tag.raw("actor")) {
                            Some(a) => target = Self::actor_target(&a)?,
                            None => return Ok(Value::Nil),
                        }
                    }
                    let base = self.mult_sum(&target, tag, cfg, 394, 395)?;
                    let div = self.divisor(tag, cfg, 403)?;
                    let mut mult = (base / div + 0.0001).floor();
                    if tag.raw("noFloor").truthy() {
                        mult = base / div;
                    }
                    let (mut limit_total, mut limit_neg_total) = (None, None);
                    if tag.raw("limit").truthy() || tag.raw("limitVar").truthy() || tag.raw("limitStat").truthy() {
                        // `tag.limit or tag.limitVar and GetMultiplier(..) or tag.limitStat and GetStat(..)`
                        let limit = if tag.raw("limit").truthy() {
                            tag.raw("limit").clone()
                        } else if tag.raw("limitVar").truthy() {
                            self.mult_of(&limit_target, tag.raw("limitVar"), cfg)?
                        } else {
                            self.stat_of(&limit_target, tag.raw("limitStat"), cfg)?
                        };
                        if tag.raw("limitTotal").truthy() {
                            limit_total = Some(limit);
                        } else if tag.raw("limitNegTotal").truthy() {
                            limit_neg_total = Some(limit);
                        } else {
                            mult = min_with(mult, &limit, 416)?;
                        }
                    }
                    if tag.raw("invert").truthy() && mult != 0.0 {
                        mult = 1.0 / mult;
                    }
                    let s = Scale {
                        mult,
                        tag,
                        ceil: false,
                        limit_total,
                        limit_neg_total,
                        lines: &MULTIPLIER_LINES,
                    };
                    value = s.apply(value)?;
                    true
                }
                TagKind::MultiplierThreshold => {
                    // ModStore.lua:450-480
                    let mut target = Target::Me;
                    let mut threshold_target = Target::Me;
                    let threshold_actor = tag.raw("thresholdActor");
                    if threshold_actor.truthy() {
                        match self.get_actor(threshold_actor) {
                            Some(a) => threshold_target = Self::actor_target(&a)?,
                            None => return Ok(Value::Nil),
                        }
                    }
                    if tag.raw("actor").truthy() {
                        match self.get_actor(tag.raw("actor")) {
                            Some(a) => target = Self::actor_target(&a)?,
                            None => return Ok(Value::Nil),
                        }
                    }
                    let mult = self.mult_sum(&target, tag, cfg, 471, 472)?;
                    let threshold = if tag.raw("threshold").truthy() {
                        tag.raw("threshold").clone()
                    } else {
                        let t = if threshold_actor.truthy() { &threshold_target } else { &target };
                        self.mult_of(t, tag.raw("thresholdVar"), cfg)?
                    };
                    let upper = tag.raw("upper").truthy();
                    let multv = Value::Num(mult);
                    // `(upper and mult > threshold) or (equals and mult ~= threshold) or (not upper and mult < threshold)`
                    let fail = (upper && lt(&threshold, &multv, 478)?)
                        || (tag.raw("equals").truthy() && !multv.raw_eq(&threshold))
                        || (!upper && lt(&multv, &threshold, 478)?);
                    !fail
                }
                TagKind::PerStat => {
                    // ModStore.lua:481-529 -- unlike Multiplier, a missing actor keeps `self`.
                    let target = match self.get_actor(tag.raw("actor")) {
                        Some(a) => Self::actor_target(&a)?,
                        None => Target::Me,
                    };
                    let base = self.stat_sum(&target, tag, cfg, 492, 493)?;
                    let div = self.divisor(tag, cfg, 501)?;
                    let base = arith(&base, &site(501), Some("local 'base'"))?;
                    let mut mult = (base / div + 0.0001).floor();
                    let mut limit_total = None;
                    if tag.raw("limit").truthy() || tag.raw("limitVar").truthy() {
                        let limit = if tag.raw("limit").truthy() {
                            tag.raw("limit").clone()
                        } else {
                            self.mult_of(&Target::Me, tag.raw("limitVar"), cfg)?
                        };
                        if tag.raw("limitTotal").truthy() {
                            limit_total = Some(limit);
                        } else {
                            mult = min_with(mult, &limit, 508)?;
                        }
                    }
                    let s = Scale {
                        mult,
                        tag,
                        ceil: false,
                        limit_total,
                        limit_neg_total: None,
                        lines: &PERSTAT_LINES,
                    };
                    value = s.apply(value)?;
                    true
                }
                TagKind::PercentStat => {
                    // ModStore.lua:530-579
                    let target = match self.get_actor(tag.raw("actor")) {
                        Some(a) => Self::actor_target(&a)?,
                        None => Target::Me,
                    };
                    let base = self.stat_sum(&target, tag, cfg, 541, 542)?;
                    let percent = if tag.raw("percent").truthy() {
                        tag.raw("percent").clone()
                    } else {
                        self.mult_of(&Target::Me, tag.raw("percentVar"), cfg)?
                    };
                    // ModStore.lua:548 `base * (percent and percent / 100 or 1)`
                    let factor = if percent.truthy() {
                        arith(&percent, &site(548), Some("local 'percent'"))? / 100.0
                    } else {
                        1.0
                    };
                    let base = arith(&base, &site(548), Some("local 'base'"))?;
                    let mut mult = base * factor;
                    if tag.raw("floor").truthy() {
                        mult = mult.floor();
                    }
                    let mut limit_total = None;
                    if tag.raw("limit").truthy() || tag.raw("limitVar").truthy() {
                        let limit = if tag.raw("limit").truthy() {
                            tag.raw("limit").clone()
                        } else {
                            self.mult_of(&Target::Me, tag.raw("limitVar"), cfg)?
                        };
                        if tag.raw("limitTotal").truthy() {
                            limit_total = Some(limit);
                        } else {
                            mult = min_with(mult, &limit, 558)?;
                        }
                    }
                    let s = Scale {
                        mult,
                        tag,
                        ceil: true,
                        limit_total,
                        limit_neg_total: None,
                        lines: &PERCENTSTAT_LINES,
                    };
                    value = s.apply(value)?;
                    true
                }
                TagKind::StatThreshold => {
                    // ModStore.lua:580-597
                    let stat = if tag.raw("statList").truthy() {
                        // ModStore.lua:584-586, upstream bug (§7.5): the loop variable shadows the accumulator, so a stat
                        // NAME is added to a number (and the result discarded); the outer `stat` stays 0.
                        for s in list(tag.raw("statList"), 584, "ipairs")? {
                            let g = self.stat_of(&Target::Me, s, cfg)?;
                            let _ = arith(s, &site(585), Some("local 'stat'"))? + arith(&g, &site(585), None)?;
                        }
                        Value::Num(0.0)
                    } else {
                        self.stat_of(&Target::Me, tag.raw("stat"), cfg)?
                    };
                    let mut threshold = if tag.raw("threshold").truthy() {
                        tag.raw("threshold").clone()
                    } else {
                        self.stat_of(&Target::Me, tag.raw("thresholdStat"), cfg)?
                    };
                    if tag.raw("thresholdPercent").truthy() || tag.raw("thresholdPercentVar").truthy() {
                        let pct = if tag.raw("thresholdPercent").truthy() {
                            tag.raw("thresholdPercent").clone()
                        } else {
                            self.mult_of(&Target::Me, tag.raw("thresholdPercentVar"), cfg)?
                        };
                        let t = arith(&threshold, &site(593), Some("local 'threshold'"))?;
                        let f = if pct.truthy() {
                            arith(&pct, &site(593), Some("local 'thresholdPercent'"))? / 100.0
                        } else {
                            1.0
                        };
                        threshold = Value::Num(t * f);
                    }
                    let upper = tag.raw("upper").truthy();
                    let statv = stat;
                    let fail = (upper && lt(&threshold, &statv, 595)?) || (!upper && lt(&statv, &threshold, 595)?);
                    !fail
                }
                TagKind::DistanceRamp => {
                    // ModStore.lua:598-617
                    let Some(dist) = cfg.and_then(|c| c.skill_dist) else {
                        return Ok(Value::Nil);
                    };
                    let distv = Value::Num(dist);
                    let ramp = tag.raw("ramp");
                    let at = |i: usize, line: u32| -> LuaResult<Value> {
                        if !ramp.is_table() {
                            return Err(err_op(&site(line), "index", Some("field 'ramp'"), ramp));
                        }
                        Ok(ramp.index(&Value::Num(i as f64)))
                    };
                    let pt = |p: &Value, j: usize, line: u32| -> LuaResult<Value> {
                        if !p.is_table() {
                            return Err(err_op(&site(line), "index", Some("field '?'"), p));
                        }
                        Ok(p.index(&Value::Num(j as f64)))
                    };
                    let num = |v: &Value, line: u32| arith(v, &site(line), Some("field '?'"));
                    let n = if let Value::Table(t) = ramp { t.len() } else { 0 };
                    let first = at(1, 602)?;
                    if le(&distv, &pt(&first, 1, 602)?, 602)? {
                        let f = num(&pt(&first, 2, 603)?, 603)?;
                        value = Value::Num(arith(&value, &site(603), Some("local 'value'"))? * f);
                    } else {
                        let last = at(n, 604)?;
                        if le(&pt(&last, 1, 604)?, &distv, 604)? {
                            let f = num(&pt(&last, 2, 605)?, 605)?;
                            value = Value::Num(arith(&value, &site(605), Some("local 'value'"))? * f);
                        } else {
                            for i in 1..=n {
                                let dat = at(i, 607)?;
                                let next = at(i + 1, 608)?;
                                if le(&distv, &pt(&next, 1, 609)?, 609)? {
                                    let (d1, d2) = (num(&pt(&dat, 1, 610)?, 610)?, num(&pt(&dat, 2, 610)?, 610)?);
                                    let (n1, n2) = (num(&pt(&next, 1, 610)?, 610)?, num(&pt(&next, 2, 610)?, 610)?);
                                    let v = arith(&value, &site(610), Some("local 'value'"))?;
                                    value = Value::Num(v * (d2 + (n2 - d2) * (dist - d1) / (n1 - d1)));
                                    break;
                                }
                            }
                        }
                    }
                    true
                }
                TagKind::MeleeProximity => {
                    // ModStore.lua:618-630 -- 0-15 full, 16-39 a linear ramp down, 40+ nothing; (15, 16) leaves the value.
                    let Some(dist) = cfg.and_then(|c| c.skill_dist) else {
                        return Ok(Value::Nil);
                    };
                    let ramp = tag.raw("ramp");
                    let top = |line: u32| -> LuaResult<f64> {
                        if !ramp.is_table() {
                            return Err(err_op(&site(line), "index", Some("field 'ramp'"), ramp));
                        }
                        arith(&ramp.index(&Value::Num(1.0)), &site(line), Some("field '?'"))
                    };
                    if dist <= 15.0 {
                        value = Value::Num(arith(&value, &site(624), Some("local 'value'"))? * top(624)?);
                    } else if (16.0..=39.0).contains(&dist) {
                        let v = arith(&value, &site(627), Some("local 'value'"))?;
                        let t = top(627)?;
                        value = Value::Num(v * (t - ((t / 25.0) * (dist - 15.0))));
                    } else if dist >= 40.0 {
                        value = Value::Num(0.0);
                    }
                    true
                }
                TagKind::Limit => {
                    // ModStore.lua:632 `value = m_min(value, tag.limit or self:GetMultiplier(tag.limitVar, cfg))`
                    let limit = if tag.raw("limit").truthy() {
                        tag.raw("limit").clone()
                    } else {
                        self.mult_of(&Target::Me, tag.raw("limitVar"), cfg)?
                    };
                    let v = check_num(&value, &site(632), 1, "min")?;
                    value = Value::Num(min_with(v, &limit, 632)?);
                    true
                }
                TagKind::Condition => self.condition_passes(tag, cfg)?,
                TagKind::ActorCondition => self.actor_condition_passes(tag, cfg)?,
                TagKind::ItemCondition => self.item_condition_passes(tag, cfg)?,
                TagKind::SocketedIn => socketed_in_passes(tag, cfg)?,
                TagKind::SkillName => skill_name_passes(tag, cfg)?,
                TagKind::SkillId => {
                    // ModStore.lua:838-841
                    cfg.and_then(|c| c.skill_granted_effect.as_ref())
                        .is_some_and(|g| Value::Str(g.id.clone()).raw_eq(tag.raw("skillId")))
                }
                TagKind::SkillPart => {
                    // ModStore.lua:842-862
                    let Some(c) = cfg else { return Ok(Value::Nil) };
                    let part = Value::from(c.skill_part);
                    let list_v = tag.raw("skillPartList");
                    let matched = if list_v.truthy() {
                        list(list_v, 848, "ipairs")?.iter().any(|p| p.raw_eq(&part))
                    } else {
                        tag.raw("skillPart").raw_eq(&part)
                    };
                    matched != tag.raw("neg").truthy()
                }
                TagKind::SkillType => {
                    // ModStore.lua:863-880 -- `cfg.skillTypes` is a set with NUMBER keys.
                    let has = |t: &Value| match (t, cfg.and_then(|c| c.skill_types.as_ref())) {
                        (Value::Num(n), Some(set)) => set.contains(n),
                        _ => false,
                    };
                    let list_v = tag.raw("skillTypeList");
                    let matched = if list_v.truthy() {
                        list(list_v, 866, "pairs")?.iter().any(has)
                    } else {
                        has(tag.raw("skillType"))
                    };
                    matched != tag.raw("neg").truthy()
                }
                TagKind::BaseFlag => {
                    // ModStore.lua:881-889 -- the granted effect's own flags win over the cfg-level ones.
                    let flags = cfg.and_then(|c| {
                        c.skill_granted_effect
                            .as_ref()
                            .and_then(|g| g.base_flags.as_ref())
                            .or(c.base_flags.as_ref())
                    });
                    let matched = match (flags, tag.raw("baseFlag").as_str()) {
                        (Some(f), Some(k)) => f.get(k).copied().unwrap_or(false),
                        _ => false,
                    };
                    matched != tag.raw("neg").truthy()
                }
                TagKind::SlotName => {
                    // ModStore.lua:890-910
                    let Some(c) = cfg else { return Ok(Value::Nil) };
                    let slot = Value::from(c.slot_name.as_ref());
                    let list_v = tag.raw("slotNameList");
                    let matched = if list_v.truthy() {
                        list(list_v, 896, "ipairs")?.iter().any(|s| s.raw_eq(&slot))
                    } else {
                        tag.raw("slotName").raw_eq(&slot)
                    };
                    matched != tag.raw("neg").truthy()
                }
                TagKind::ModFlagOr => {
                    // ModStore.lua:911-917 -- `cfg.flags = 0` is TRUE in Lua; band() then answers 0 anyway.
                    let Some(flags) = cfg.and_then(|c| c.flags) else {
                        return Ok(Value::Nil);
                    };
                    band(flags, check_num(tag.raw("modFlags"), &site(915), 2, "band")?) != 0.0
                }
                TagKind::KeywordFlagAnd => {
                    // ModStore.lua:918-924 -- `band(...) ~= tag.keywordFlags` compares against the RAW field.
                    let Some(kw) = cfg.and_then(|c| c.keyword_flags) else {
                        return Ok(Value::Nil);
                    };
                    let wanted = tag.raw("keywordFlags");
                    Value::Num(band(kw, check_num(wanted, &site(922), 2, "band")?)).raw_eq(wanted)
                }
                TagKind::MonsterTag => self.monster_tag_passes(tag)?,
                TagKind::Other => true,
            };
            if !pass {
                return Ok(Value::Nil);
            }
        }
        // ModStore.lua:959-969 -- for EVERY tag carrying both fields, on the value computed above.
        if let Some(gl) = global_limits {
            for tag in &m.tags {
                let (limit, key) = (tag.raw("globalLimit"), tag.raw("globalLimitKey"));
                if !limit.truthy() || !key.truthy() {
                    continue;
                }
                // ModStore.lua:962 `value = value or 0`
                if !value.truthy() {
                    value = Value::Num(0.0);
                }
                let key: Str = match key {
                    Value::Str(s) => s.clone(),
                    other => unimplemented!("ModStore.lua:963 globalLimits keyed by a {} (none in the data)", other.type_name()),
                };
                let used = *gl.entry(key.clone()).or_insert(0.0);
                let v = arith(&value, &site(964), Some("local 'value'"))?;
                // `used + value > tag.globalLimit` compiles to `globalLimit < used + value`.
                if lt(limit, &Value::Num(used + v), 964)? {
                    let l = arith(limit, &site(965), Some("field 'globalLimit'"))?;
                    value = Value::Num(l - used);
                }
                let v = arith(&value, &site(967), Some("local 'value'"))?;
                gl.insert(key, used + v);
            }
        }
        Ok(value)
    }

    /// `Σ GetMultiplier(target, var)` over `tag.varList` (pairs, line `list_line`), or `GetMultiplier(target, tag.var)`.
    fn mult_sum(&self, target: &Target, tag: &Tag, cfg: Option<&ModCfg>, list_line: u32, add_line: u32) -> LuaResult<f64> {
        let var_list = tag.raw("varList");
        if var_list.truthy() {
            let mut base = 0.0;
            for v in list(var_list, list_line, "pairs")? {
                base += arith(&self.mult_of(target, v, cfg)?, &site(add_line), None)?;
            }
            return Ok(base);
        }
        arith(&self.mult_of(target, tag.raw("var"), cfg)?, &site(add_line), None)
    }

    /// `Σ GetStat(target, stat)` over `tag.statList` (ipairs; `base = base + GetStat(..)` at `add_line`), or the raw
    /// `GetStat(target, tag.stat)` -- whose arithmetic (and its nil error) happens at the caller's next line.
    fn stat_sum(&self, target: &Target, tag: &Tag, cfg: Option<&ModCfg>, list_line: u32, add_line: u32) -> LuaResult<Value> {
        let stat_list = tag.raw("statList");
        if stat_list.truthy() {
            let mut base = 0.0;
            for s in list(stat_list, list_line, "ipairs")? {
                base += arith(&self.stat_of(target, s, cfg)?, &site(add_line), None)?;
            }
            return Ok(Value::Num(base));
        }
        self.stat_of(target, tag.raw("stat"), cfg)
    }

    /// ModStore.lua:633-663. Varunastra (`countsAsAll1H`) adds "Added<Cond>" flags to the weapon data: a NEGATED condition
    /// the weapon itself added must not fail the mod, so it is skipped; one the weapon did NOT add returns.
    fn condition_passes(&self, tag: &Tag, cfg: Option<&ModCfg>) -> LuaResult<bool> {
        let all_one_h = {
            let actor = self.actor.borrow();
            let pick = |w: &Option<Rc<crate::value::Table>>| w.as_ref().filter(|t| t.get("countsAsAll1H").truthy()).cloned();
            pick(&actor.weapon_data1).or_else(|| pick(&actor.weapon_data2))
        };
        let neg = tag.raw("neg").truthy();
        let from_cfg = |v: &Value| -> bool {
            match (cfg.and_then(|c| c.skill_cond.as_ref()), v.as_str()) {
                (Some(sc), Some(k)) => sc.get(k).copied().unwrap_or(false),
                _ => false,
            }
        };
        let mut matched = false;
        let var_list = tag.raw("varList");
        if var_list.truthy() {
            for v in list(var_list, 637, "pairs")? {
                if neg && let Some(w) = &all_one_h {
                    let added = w.get(&format!("Added{}", concat_str(v, &site(638), Some("local 'var'"))?)).clone();
                    if !added.is_nil() {
                        if !added.truthy() {
                            return Ok(false);
                        }
                        continue;
                    }
                }
                if self.cond_of(&Target::Me, v, cfg)? || from_cfg(v) {
                    matched = true;
                    break;
                }
            }
        } else {
            let v = tag.raw("var");
            let mut skipped = false;
            if neg && let Some(w) = &all_one_h {
                let added = w.get(&format!("Added{}", concat_str(v, &site(650), Some("field 'var'"))?)).clone();
                if !added.is_nil() {
                    if !added.truthy() {
                        return Ok(false);
                    }
                    skipped = true;
                }
            }
            if !skipped {
                matched = self.cond_of(&Target::Me, v, cfg)? || from_cfg(v);
            }
        }
        Ok(matched != neg)
    }

    /// ModStore.lua:664-690. A missing actor leaves `target` nil, which does NOT fail by itself: the mod can still match on
    /// `cfg.actor` alone.
    fn actor_condition_passes(&self, tag: &Tag, cfg: Option<&ModCfg>) -> LuaResult<bool> {
        let tag_actor = tag.raw("actor");
        let target: Option<Target> = if tag_actor.truthy() {
            // `target = actor and actor.modDB`
            self.get_actor(tag_actor).and_then(|a| a.borrow().mod_db.clone()).map(Target::Store)
        } else {
            Some(Target::Me)
        };
        let var_list = tag.raw("varList");
        let mut matched = false;
        if let Some(t) = &target
            && (tag.raw("var").truthy() || var_list.truthy())
        {
            if var_list.truthy() {
                for v in list(var_list, 673, "pairs")? {
                    if self.cond_of(t, v, cfg)? {
                        matched = true;
                        break;
                    }
                }
            } else {
                matched = self.cond_of(t, tag.raw("var"), cfg)?;
            }
        } else if tag_actor.truthy() && cfg.is_some_and(|c| tag_actor.raw_eq(&Value::from(c.actor.as_ref()))) {
            matched = true;
        }
        Ok(matched != tag.raw("neg").truthy())
    }

    /// ModStore.lua:691-761: the slots the tag looks at (one, a pair, or all), every condition asked about every item, and
    /// every answer must differ from `tag.neg or false`. `pairs(items)` order only permutes `matches`.
    fn item_condition_passes(&self, tag: &Tag, cfg: Option<&ModCfg>) -> LuaResult<bool> {
        let raw_slot = tag.raw("itemSlot");
        let Value::Str(raw_slot) = raw_slot else {
            return Err(err_op(&site(693), "index", Some("field 'itemSlot'"), raw_slot));
        };
        let item_slot = normalize_item_slot(raw_slot);
        let actor = self.actor.borrow();
        let cfg_item = cfg.and_then(|c| c.item.clone());
        let all_slots = tag.raw("allSlots").truthy();
        let items: Items = if all_slots {
            actor
                .item_list
                .as_ref()
                .map(|l| l.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        } else if let Some(list) = &actor.item_list {
            if tag.raw("bothSlots").truthy() {
                let (k1, k2) = (format!("{item_slot} 1"), format!("{item_slot} 2"));
                let mut i1 = list.get(k1.as_str()).cloned();
                let mut i2 = list.get(k2.as_str()).cloned();
                if let Some(i) = &i1
                    && is_kalandra(i, 701)?
                {
                    i1 = i2.clone();
                }
                if let Some(i) = &i2
                    && is_kalandra(i, 702)?
                {
                    i2 = i1.clone();
                }
                match (i1, i2) {
                    (Some(a), Some(b)) => Some(vec![(Str::from(k1), a), (Str::from(k2), b)]),
                    _ => Some(Vec::new()),
                }
            } else {
                let mut item = list.get(item_slot.as_str()).cloned().or_else(|| cfg_item.clone());
                if let Some(i) = &item
                    && i.name().is_some()
                    && is_kalandra(i, 708)?
                {
                    item = list.get(swap_slot_digit(&item_slot).as_str()).cloned();
                }
                match item.or(cfg_item) {
                    Some(i) => Some(vec![(Str::from(item_slot.as_str()), i)]),
                    None => Some(Vec::new()),
                }
            }
        } else {
            Some(Vec::new())
        };
        drop(actor);
        let mut matches: Vec<Value> = Vec::new();
        let search = tag.raw("searchCond");
        if search.truthy() {
            let exclude_self = tag.raw("excludeSelf").truthy();
            for (slot, item) in pairs_items(&items, 715)? {
                let type_ok = !all_slots || (item.item_type() != Some("Jewel") && item.item_type() != Some("Graft"));
                if (type_ok && **slot != *item_slot) || !exclude_self {
                    let s = lower_of(search, 717, "field 'searchCond'")?;
                    matches.push(Value::Bool(item.find_modifier_substring(&s, &lower(slot))));
                }
            }
        }
        let cond = tag.raw("rarityCond");
        if cond.truthy() {
            for (_, i) in pairs_items(&items, 722)? {
                matches.push(Value::Bool(Value::from(i.rarity().map(Str::from).as_ref()).raw_eq(cond)));
            }
        }
        for (field, line, get) in [
            (
                "corruptedCond",
                727,
                (|i: &Rc<dyn ItemLike>| i.corrupted()) as fn(&Rc<dyn ItemLike>) -> Option<bool>,
            ),
            ("shaperCond", 732, |i| i.shaper()),
            ("elderCond", 737, |i| i.elder()),
        ] {
            let cond = tag.raw(field);
            if cond.truthy() {
                for (_, i) in pairs_items(&items, line)? {
                    matches.push(Value::Bool(Value::from(get(i)).raw_eq(cond)));
                }
            }
        }
        let name_cond = tag.raw("nameCond");
        if name_cond.truthy() {
            for (_, i) in pairs_items(&items, 742)? {
                // `item.name and item.name:lower() == tag.nameCond:lower()`
                match i.name() {
                    Some(n) => matches.push(Value::Bool(lower(n) == lower_of(name_cond, 743, "field 'nameCond'")?)),
                    None => matches.push(Value::Nil),
                }
            }
        }
        let has_items = !pairs_items(&items, 747)?.is_empty();
        // `bool == (tag.neg or false)` -- the RAW value, so a truthy non-boolean `neg` matches nothing.
        let neg_v = if tag.raw("neg").truthy() {
            tag.raw("neg").clone()
        } else {
            Value::Bool(false)
        };
        let matched = !matches.iter().any(|b| b.raw_eq(&neg_v));
        Ok(matched && (has_items || tag.raw("neg").truthy()))
    }

    /// ModStore.lua:925-957 -- any monster tag of the minion, compared case-insensitively.
    fn monster_tag_passes(&self, tag: &Tag) -> LuaResult<bool> {
        let actor = self.actor.borrow();
        let Some(Some(monster_tags)) = &actor.minion_data else {
            return Ok(false);
        };
        let list_v = tag.raw("monsterTagList");
        let single = tag.raw("monsterTag");
        // `match` is false, nil or true -- `if match == true then break`.
        let mut matched = Value::Bool(false);
        for t in monster_tags {
            let match_name = lower(t);
            if list_v.truthy() {
                for name in list(list_v, 937, "pairs")? {
                    if lower_of(name, 938, "local 'name'")? == match_name {
                        matched = Value::Bool(true);
                        break;
                    }
                }
            } else {
                // Rewritten every round: with a single tag only the LAST monster tag survives a miss.
                matched = if single.truthy() {
                    Value::Bool(lower_of(single, 944, "field 'monsterTag'")? == match_name)
                } else {
                    single.clone()
                };
            }
            if matches!(matched, Value::Bool(true)) {
                break;
            }
        }
        Ok(matched.truthy() != tag.raw("neg").truthy())
    }
}

/// ModStore.lua:762-803. `match` is a map of independent tests (slot, keyword/colour, sockets): all must hold (none, with
/// `neg`), so `pairs(match)` order cannot change the answer.
fn socketed_in_passes(tag: &Tag, cfg: Option<&ModCfg>) -> LuaResult<bool> {
    let (slot_name, keyword, socket_color, sockets) = (tag.raw("slotName"), tag.raw("keyword"), tag.raw("socketColor"), tag.raw("sockets"));
    let Some(cfg) = cfg else { return Ok(false) };
    if !slot_name.truthy() && !keyword.truthy() && !socket_color.truthy() {
        return Ok(false);
    }
    let mut matches: Vec<bool> = Vec::new();
    if slot_name.truthy() {
        matches.push(slot_name.raw_eq(&Value::from(cfg.slot_name.as_ref())));
    }
    if keyword.truthy() {
        // `(cfg.skillGem and calcLib.gemIsType(cfg.skillGem, tag.keyword)) or false`; a non-string keyword equals none of
        // gemIsType's literals and indexes `gem.tags` with a non-string key -- false.
        matches.push(match (&cfg.skill_gem, keyword.as_str()) {
            (Some(g), Some(k)) => gem_is_type(g, k, false),
            _ => false,
        });
    } else if socket_color.truthy() && !sockets.is_str("all") {
        matches.push(socket_color.raw_eq(&Value::from(cfg.socket_color.as_ref())));
    }
    if sockets.truthy() {
        // `cfg[targetAtrColor] or 0`; an unknown colour reads `cfg[false]` = nil.
        let count = match socket_color.as_str() {
            Some("R") => cfg.strength_gems,
            Some("G") => cfg.dexterity_gems,
            Some("B") => cfg.intelligence_gems,
            _ => None,
        }
        .unwrap_or(0.0);
        if sockets.is_str("all") {
            let total = cfg.intelligence_gems.unwrap_or(0.0) + cfg.dexterity_gems.unwrap_or(0.0) + cfg.strength_gems.unwrap_or(0.0);
            matches.push(total == count && total > 0.0);
        } else if sockets.is_table() && cfg.socket_num.is_some() {
            let n = Value::from(cfg.socket_num);
            matches.push(list(sockets, 767, "ipairs")?.iter().any(|v| v.raw_eq(&n)));
        } else if let Value::Num(s) = sockets {
            matches.push(count < *s);
        } else {
            return Ok(false);
        }
    }
    let neg = tag.raw("neg").truthy();
    Ok(!matches.contains(&neg))
}

/// ModStore.lua:804-837. With `includeTransfigured` the comparison runs on gameIds (Arc / Vaal Arc / Arc of Surging are one
/// skill); an unknown name answers nil while the cfg side falls back to "", and `nil == ""` is false.
fn skill_name_passes(tag: &Tag, cfg: Option<&ModCfg>) -> LuaResult<bool> {
    let summon = tag.raw("summonSkill").truthy();
    let name_list = tag.raw("skillNameList");
    let skill_name = tag.raw("skillName");
    let cfg_name = |c: &ModCfg| {
        if summon {
            c.summon_skill_name.clone()
        } else {
            c.skill_name.clone()
        }
    };
    let mut matched = false;
    if tag.raw("includeTransfigured").truthy() {
        // `tag.summonSkill and (cfg and getGameId(cfg.summonSkillName, true) or "")
        //                   or (cfg and cfg.skillName and getGameId(cfg.skillName, true) or "")`
        let match_id: &str = cfg
            .and_then(cfg_name)
            .and_then(|n| get_game_id_from_gem_name(&Value::Str(n), true))
            .unwrap_or("");
        if name_list.truthy() {
            for name in list(name_list, 809, "pairs")? {
                if name.truthy() && get_game_id_from_gem_name(name, true) == Some(match_id) {
                    matched = true;
                    break;
                }
            }
        } else {
            matched = skill_name.truthy() && get_game_id_from_gem_name(skill_name, true) == Some(match_id);
        }
    } else {
        let match_name = lower(&cfg.and_then(cfg_name).unwrap_or_else(|| Str::from("")));
        if name_list.truthy() {
            for name in list(name_list, 822, "pairs")? {
                if lower_of(name, 823, "local 'name'")? == match_name {
                    matched = true;
                    break;
                }
            }
        } else {
            matched = skill_name.truthy() && lower_of(skill_name, 829, "field 'skillName'")? == match_name;
        }
    }
    Ok(matched != tag.raw("neg").truthy())
}
