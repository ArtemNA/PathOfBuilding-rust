//! `env.<actor>.breakdown` (R8.4): the table `Modules/CalcBreakdown.lua` returns (CalcPerform.lua:1514-1521 creates one for the player and one
//! for the minion when `env.buildBreakdown`, i.e. MAIN / CALCS) and the generators it carries (`multiChain`, `simple`, `mod`, `slot`).
//!
//! Model: a breakdown entry is a Lua value -- a string, a number, a boolean, a function (a `sourceNameTooltip` closure), an item reference
//! (`slots[i].item = actor.itemList[source]`) or a table with an array part and string keys ([`BTable`]). Lines are `string.format`
//! results (`pob_lua::format`, the [`sf!`] macro) and `..` concatenations (`tostring_num` for numbers), byte for byte as LuaJIT.
//! Upstream never iterates a breakdown table (`pairs(breakdown` has no hit), so key order carries no meaning; it is kept in assignment order.
//! The gate is `tests/breakdown.rs` against the LuaJIT probe `tools/lua-corpus/export-breakdown.ts`.

use indexmap::IndexMap;
use pob_lua::Arg;
use pob_mods::{ModCfg, ModType, StoreRef};

/// One Lua value inside a breakdown table.
#[derive(Clone, Debug, PartialEq)]
pub enum BVal {
    Str(String),
    Num(f64),
    Bool(bool),
    Table(BTable),
    /// An item table (`actor.itemList[slot]`), by its `name`.
    Item(String),
    /// A function value (`row.sourceNameTooltip`, CalcPerform.lua:2139).
    Fn,
    /// A nested breakdown module table: `breakdown.MainHand` / `breakdown.OffHand` = `require(calcs.breakdownModule)(skillModList,
    /// output.MainHand)` (CalcOffence.lua:2006/:2023) -- the pass breakdown of an attack, filled like `actor.breakdown`.
    Module(Box<Breakdown>),
}

impl From<String> for BVal {
    fn from(s: String) -> BVal {
        BVal::Str(s)
    }
}
impl From<&str> for BVal {
    fn from(s: &str) -> BVal {
        BVal::Str(s.to_owned())
    }
}
impl From<f64> for BVal {
    fn from(n: f64) -> BVal {
        BVal::Num(n)
    }
}
impl From<bool> for BVal {
    fn from(b: bool) -> BVal {
        BVal::Bool(b)
    }
}
impl From<BTable> for BVal {
    fn from(t: BTable) -> BVal {
        BVal::Table(t)
    }
}

/// A Lua table: the `1..n` run (`t_insert`) and the other keys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BTable {
    pub list: Vec<BVal>,
    pub fields: IndexMap<String, BVal>,
}

impl BTable {
    pub fn new() -> BTable {
        BTable::default()
    }

    /// `{ "a", "b", ... }`
    pub fn lines<I: IntoIterator<Item = String>>(lines: I) -> BTable {
        BTable {
            list: lines.into_iter().map(BVal::Str).collect(),
            fields: IndexMap::new(),
        }
    }

    /// `t_insert(t, v)`
    pub fn push(&mut self, v: impl Into<BVal>) {
        self.list.push(v.into());
    }

    /// `t[k] = v` (builder form).
    #[must_use]
    pub fn with(mut self, k: &str, v: impl Into<BVal>) -> BTable {
        self.fields.insert(k.to_owned(), v.into());
        self
    }

    /// `t[k] = v` with a value that may be nil (`nil` removes the key).
    pub fn set(&mut self, k: &str, v: Option<BVal>) {
        match v {
            Some(v) => {
                self.fields.insert(k.to_owned(), v);
            }
            None => {
                self.fields.shift_remove(k);
            }
        }
    }

    /// `t[k]` as a table -- a Lua error (index a non-table) when it is not one.
    pub fn field_mut(&mut self, k: &str) -> &mut BTable {
        match self.fields.get_mut(k) {
            Some(BVal::Table(t)) => t,
            _ => panic!("Modules/CalcBreakdown: attempt to index field '{k}' (a nil value)"),
        }
    }

    /// `#t`
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// `t_insert(t, pos, v)` (1-based `pos`).
    pub fn insert(&mut self, pos: usize, v: impl Into<BVal>) {
        self.list.insert(pos - 1, v.into());
    }

    /// `t_remove(t, pos)` (1-based `pos`; nothing when out of range).
    pub fn remove(&mut self, pos: usize) {
        if pos >= 1 && pos <= self.list.len() {
            self.list.remove(pos - 1);
        }
    }

    /// `t[i] = s` for `1 <= i <= #t + 1`.
    pub fn set_at(&mut self, i: usize, v: impl Into<BVal>) {
        if i == self.list.len() + 1 {
            self.list.push(v.into());
        } else {
            self.list[i - 1] = v.into();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

/// `{ label = l, key = k }` x n -- a `colList`.
pub fn col_list(cols: &[(&str, &str)]) -> BTable {
    BTable {
        list: cols
            .iter()
            .map(|(label, key)| BVal::Table(BTable::new().with("label", *label).with("key", *key)))
            .collect(),
        fields: IndexMap::new(),
    }
}

/// `{ label = label, rowList = { }, colList = cols }` (`label` nil when `None`).
pub fn row_table(label: Option<&str>, cols: &[(&str, &str)]) -> BTable {
    let mut t = BTable::new();
    if let Some(l) = label {
        t.fields.insert("label".into(), l.into());
    }
    t.fields.insert("rowList".into(), BTable::new().into());
    t.fields.insert("colList".into(), col_list(cols).into());
    t
}

/// A `rowList` entry: string fields in the given order.
pub fn row(fields: &[(&str, String)]) -> BTable {
    let mut t = BTable::new();
    for (k, v) in fields {
        t.fields.insert((*k).to_owned(), BVal::Str(v.clone()));
    }
    t
}

/// `actor.breakdown` -- the entries the calc modules assign (`breakdown[key] = ...`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Breakdown {
    entries: IndexMap<String, BVal>,
}

impl Breakdown {
    pub fn new() -> Breakdown {
        Breakdown::default()
    }

    /// `breakdown[k] = v`; `None` (nil) removes the key.
    pub fn set(&mut self, k: impl Into<String>, v: Option<BVal>) {
        let k = k.into();
        match v {
            Some(v) => {
                self.entries.insert(k, v);
            }
            None => {
                self.entries.shift_remove(&k);
            }
        }
    }

    /// `breakdown[k] = t`
    pub fn put(&mut self, k: impl Into<String>, t: BTable) {
        self.entries.insert(k.into(), BVal::Table(t));
    }

    pub fn get(&self, k: &str) -> Option<&BVal> {
        self.entries.get(k)
    }

    pub fn get_mut(&mut self, k: &str) -> Option<&mut BVal> {
        self.entries.get_mut(k)
    }

    /// `breakdown[k]` is a table (`breakdown[k] and ...`).
    pub fn has_table(&self, k: &str) -> bool {
        matches!(self.entries.get(k), Some(BVal::Table(_)))
    }

    /// `breakdown[k]` as a nested breakdown module (`breakdown.MainHand`, CalcOffence.lua:2006/:2023).
    pub fn module_mut(&mut self, k: &str) -> &mut Breakdown {
        match self.entries.get_mut(k) {
            Some(BVal::Module(b)) => b,
            _ => panic!("Modules/CalcOffence: attempt to index field '{k}' (a nil value)"),
        }
    }

    /// `breakdown[k]` as a table (a Lua "attempt to index a nil value" when absent).
    pub fn table_mut(&mut self, k: &str) -> &mut BTable {
        match self.entries.get_mut(k) {
            Some(BVal::Table(t)) => t,
            _ => panic!("Modules/CalcDefence: attempt to index field '{k}' (a nil value)"),
        }
    }

    /// `breakdown[k] = breakdown[k] or t`, then `breakdown[k]`.
    pub fn table_or(&mut self, k: &str, t: impl FnOnce() -> BTable) -> &mut BTable {
        if !matches!(self.entries.get(k), Some(BVal::Table(_))) {
            self.entries.insert(k.to_owned(), BVal::Table(t()));
        }
        self.table_mut(k)
    }

    pub fn entries(&self) -> impl Iterator<Item = (&String, &BVal)> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Build.lua:1894-1902: the entry is a non-empty breakdown (`#t > 0`, `radius`, or a non-empty `rowList` / `reservations` /
    /// `damageTypes` / `slots` / `modList`). `None` = `breakdown[key]` is nil.
    pub fn is_populated(v: &BVal) -> bool {
        let t = match v {
            BVal::Table(t) => t,
            // `#s > 0` on a string; a breakdown entry is never a number / boolean / function here
            BVal::Str(s) => return !s.is_empty(),
            _ => return false,
        };
        let nonempty = |k: &str| matches!(t.fields.get(k), Some(BVal::Table(x)) if !x.is_empty());
        !t.is_empty()
            || t.fields.get("radius").is_some_and(|r| !matches!(r, BVal::Bool(false)))
            || nonempty("rowList")
            || nonempty("reservations")
            || nonempty("damageTypes")
            || nonempty("slots")
            || nonempty("modList")
    }
}

// --- string.format ---------------------------------------------------------------------------------------------------

/// A value [`sf!`] passes to `string.format`.
pub trait ToArg {
    fn arg(&self) -> Arg<'_>;
}
impl ToArg for f64 {
    fn arg(&self) -> Arg<'_> {
        Arg::Num(*self)
    }
}
impl ToArg for &str {
    fn arg(&self) -> Arg<'_> {
        Arg::Str(self)
    }
}
impl ToArg for String {
    fn arg(&self) -> Arg<'_> {
        Arg::Str(self)
    }
}
impl ToArg for bool {
    fn arg(&self) -> Arg<'_> {
        Arg::Bool(*self)
    }
}

/// `string.format`; a format error is a Lua error (raised).
pub fn format(fmt: &str, args: &[Arg<'_>]) -> String {
    pob_lua::format(fmt, args).unwrap_or_else(|e| panic!("breakdown s_format({fmt:?}): {e}"))
}

/// `s_format(fmt, ...)`
#[macro_export]
macro_rules! sf {
    ($fmt:expr $(, $a:expr)* $(,)?) => {
        $crate::breakdown::format($fmt, &[$($crate::breakdown::ToArg::arg(&$a)),*])
    };
}

/// A `string.format` argument kept in a chain (`{ fmt, value, extra... }`).
#[derive(Clone, Debug)]
pub enum OwnedArg {
    Num(f64),
    Str(String),
}

impl OwnedArg {
    fn as_arg(&self) -> Arg<'_> {
        match self {
            OwnedArg::Num(n) => Arg::Num(*n),
            OwnedArg::Str(s) => Arg::Str(s),
        }
    }
}

/// One `{ fmt, value, ... }` of a `multiChain` chain.
#[derive(Clone, Debug)]
pub struct Step {
    pub fmt: &'static str,
    pub value: f64,
    pub extra: Vec<OwnedArg>,
}

/// `{ fmt, value }`
pub fn step(fmt: &'static str, value: f64) -> Step {
    Step {
        fmt,
        value,
        extra: Vec::new(),
    }
}

/// `{ fmt, value, s }`
pub fn step_s(fmt: &'static str, value: f64, s: &str) -> Step {
    Step {
        fmt,
        value,
        extra: vec![OwnedArg::Str(s.to_owned())],
    }
}

/// `{ fmt, value, n }`
pub fn step_n(fmt: &'static str, value: f64, n: f64) -> Step {
    Step {
        fmt,
        value,
        extra: vec![OwnedArg::Num(n)],
    }
}

/// The chain table `breakdown.multiChain` reads.
#[derive(Clone, Debug, Default)]
pub struct Chain {
    pub label: Option<String>,
    pub base: Option<Step>,
    pub steps: Vec<Step>,
    pub total: Option<String>,
    pub no_total: bool,
}

fn step_line(s: &Step) -> String {
    let mut args = vec![Arg::Num(s.value)];
    args.extend(s.extra.iter().map(OwnedArg::as_arg));
    format(s.fmt, &args)
}

// --- CalcBreakdown.lua -----------------------------------------------------------------------------------------------

/// CalcBreakdown.lua:17-41 `breakdown.multiChain(out, chain)` -- returns `lines`.
pub fn multi_chain(out: &mut BTable, chain: &Chain) -> usize {
    let base = chain.base.as_ref().map(|b| b.value);
    let mut multiplier = 1.0;
    let mut lines = 0usize;
    if let Some(l) = &chain.label {
        out.push(l.clone());
    }
    if let Some(b) = &chain.base {
        out.push(step_line(b));
    }
    if base != Some(0.0) {
        for m in &chain.steps {
            if m.value != 1.0 {
                multiplier *= m.value;
                out.push(format!("x {}", step_line(m)));
                lines += 1;
            }
        }
    }
    if let Some(t) = &chain.total {
        out.push(t.clone());
    } else if (!chain.no_total && (lines > 0 && base.is_some())) || (lines > 1 && base.is_none()) {
        out.push(sf!("= %.2f", multiplier * base.unwrap_or(1.0)));
    }
    lines
}

fn sum(db: &StoreRef, ty: ModType, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
    crate::setup::ok(db.borrow().sum(ty, cfg, names))
}

fn more(db: &StoreRef, cfg: Option<&ModCfg>, names: &[&str]) -> f64 {
    crate::setup::ok(db.borrow().more(cfg, names))
}

/// CalcBreakdown.lua:43-66 `breakdown.simple(extraBase, cfg, total, ...)` -- `None` = nil. `base` sums only the FIRST name (`(...)`).
pub fn simple(db: &StoreRef, extra_base: Option<f64>, cfg: Option<&ModCfg>, total: f64, names: &[&str]) -> Option<BTable> {
    let extra_base = extra_base.unwrap_or(0.0);
    let base = sum(db, ModType::Base, cfg, &names[..1]);
    if base + extra_base == 0.0 {
        return None;
    }
    let inc = sum(db, ModType::Inc, cfg, names);
    let more = more(db, cfg, names);
    if !(inc != 0.0 || more != 1.0 || (base != 0.0 && extra_base != 0.0)) {
        return None;
    }
    let mut out = BTable::new();
    if base != 0.0 && extra_base != 0.0 {
        out.push(sf!("(%g + %g) ^8(base)", extra_base, base));
    } else {
        out.push(sf!("%g ^8(base)", base + extra_base));
    }
    if inc != 0.0 {
        out.push(sf!("x %.2f ^8(increased/reduced)", 1.0 + inc / 100.0));
    }
    if more != 1.0 {
        out.push(sf!("x %.2f ^8(more/less)", more));
    }
    out.push(sf!("= %g", total));
    Some(out)
}

/// CalcBreakdown.lua:68-78 `breakdown.mod(modList, cfg, ...)` -- `None` = nil.
pub fn mod_(db: &StoreRef, cfg: Option<&ModCfg>, names: &[&str]) -> Option<BTable> {
    let inc = sum(db, ModType::Inc, cfg, names);
    let more = more(db, cfg, names);
    if inc != 0.0 || more != 1.0 {
        Some(BTable::lines([
            sf!("%.2f ^8(increased/reduced)", 1.0 + inc / 100.0),
            sf!("x %.2f ^8(more/less)", more),
            sf!("= %.2f", (1.0 + inc / 100.0) * more),
        ]))
    } else {
        None
    }
}

/// `(cond) and s or false` -- a slot field.
fn str_or_false(cond: bool, s: impl FnOnce() -> String) -> BVal {
    if cond { BVal::Str(s()) } else { BVal::Bool(false) }
}

/// One `slots` entry (CalcBreakdown.lua:84-92 / the inline copies in CalcDefence.lua:886-893, :909-915, :953-960, :979-986): `inc`/`more`
/// are `false` when neutral (`(x) and s_format(...)`), `item` nil when the slot holds no item.
#[allow(clippy::too_many_arguments)]
pub fn slot_entry(
    base: f64,
    inc: Option<f64>,
    more: f64,
    total: String,
    source: &str,
    source_name: Option<&str>,
    item: Option<String>,
) -> BTable {
    let mut t = BTable::new().with("base", base);
    if let Some(inc) = inc {
        t.fields
            .insert("inc".into(), str_or_false(inc != 0.0, || sf!(" x %.2f", 1.0 + inc / 100.0)));
    }
    t.fields.insert("more".into(), str_or_false(more != 1.0, || sf!(" x %.2f", more)));
    t.fields.insert("total".into(), total.into());
    t.fields.insert("source".into(), source.into());
    if let Some(n) = source_name {
        t.fields.insert("sourceName".into(), n.into());
    }
    if let Some(i) = item {
        t.fields.insert("item".into(), BVal::Item(i));
    }
    t
}

/// CalcBreakdown.lua:80-93 `breakdown.slot(source, sourceName, cfg, base, total, ...)`: appends to `breakdown[<first name>].slots`.
#[allow(clippy::too_many_arguments)]
pub fn slot(
    bd: &mut Breakdown,
    db: &StoreRef,
    item: Option<String>,
    source: &str,
    source_name: Option<&str>,
    cfg: Option<&ModCfg>,
    base: f64,
    total: Option<f64>,
    names: &[&str],
) {
    let inc = sum(db, ModType::Inc, cfg, names);
    let more = more(db, cfg, names);
    let total = sf!("%.2f", total.unwrap_or(base * (1.0 + inc / 100.0) * more));
    let e = slot_entry(base, Some(inc), more, total, source, source_name, item);
    bd.table_mut(names[0]).field_mut("slots").push(e);
}

/// CalcBreakdown.lua:95-112 `breakdown.area(base, areaMod, total, incBreakpoint, moreBreakpoint, redBreakpoint, lessBreakpoint, label,
/// radiusMod)`. `bps` = the four breakpoints when all are non-nil; `label` nil is not inserted (`t_insert(out, nil)`).
pub fn area(
    base: f64,
    area_mod: f64,
    total: f64,
    bps: Option<(f64, f64, f64, f64)>,
    label: Option<&str>,
    radius_mod: Option<f64>,
) -> BTable {
    let mut out = BTable::new();
    if let Some(l) = label {
        out.push(l);
    }
    if base != total {
        out.push(sf!("%.1fm ^8(base radius)", base / 10.0));
        out.push(sf!(
            "x %.2f ^8(square root of area of effect modifier)",
            (100.0 * area_mod.sqrt()).floor() / 100.0
        ));
        if let Some(r) = radius_mod
            && r != 1.0
        {
            out.push(sf!("x %.2f ^8(final radius modifier)", r));
        }
        out.push(sf!("= %.1fm", total / 10.0));
    }
    if let Some((inc, more, red, less)) = bps {
        out.push(sf!(
            "^8Next 0.1m breakpoint: %d%% increased AoE / a %d%% more AoE multiplier",
            inc,
            more
        ));
        out.push(sf!(
            "^8Previous 0.1m breakpoint: %d%% reduced AoE / a %d%% less AoE multiplier",
            red,
            less
        ));
    }
    out.fields.insert("radius".into(), BVal::Num(total));
    out
}

/// CalcBreakdown.lua:114-160 `breakdown.effMult(damageType, resist, pen, taken, mult, takenMore, sourceRes, useRes, invertChance,
/// effectiveResist)`.
#[allow(clippy::too_many_arguments)]
pub fn eff_mult(
    damage_type: &str,
    resist: f64,
    pen: f64,
    taken: f64,
    mult: f64,
    taken_more: f64,
    source_res: Option<&str>,
    use_res: bool,
    invert_chance: Option<f64>,
    effective_resist: Option<f64>,
) -> BTable {
    let mut out = BTable::new();
    let resist_form = if damage_type == "Physical" {
        "physical damage reduction"
    } else {
        "resistance"
    };
    let effective_resist = effective_resist.unwrap_or(resist - pen);
    if let Some(sr) = source_res
        && sr != damage_type
    {
        out.push(sf!("Enemy %s: %d%% ^8(%s)", resist_form, resist, sr));
    } else if resist != 0.0 {
        out.push(sf!("Enemy %s: %d%%", resist_form, resist));
    }
    if let Some(ic) = invert_chance.filter(|ic| *ic != 0.0 && use_res) {
        out.push("Effective resistance:");
        out.push(sf!("%g%% ^8(non-inverted hit after penetration)", resist - pen));
        out.push(sf!("%g%% ^8(inverted hit after penetration)", -resist - pen));
        out.push(sf!(
            "= %g%% ^8(weighted average from %.0f%% inversion chance)",
            effective_resist,
            ic * 100.0
        ));
    } else if pen != 0.0 || !use_res {
        out.push("Effective resistance:");
        out.push(sf!("%d%% ^8(resistance)", resist));
        if pen < 0.0 {
            out.push(sf!("+ %d%% ^8(penetration)", -pen));
        } else if pen > 0.0 {
            out.push(sf!("- %d%% ^8(penetration)", pen));
        }
        if use_res {
            out.push(sf!("= %d%%", resist - pen));
        } else {
            out.push(sf!("x %d%% ^8(resistance ignored)", 0.0));
            out.push(sf!("= %d%%", 0.0));
        }
    }
    if use_res {
        multi_chain(
            &mut out,
            &Chain {
                label: Some("Effective DPS modifier:".into()),
                steps: vec![
                    step_s("%.2f ^8(%s)", 1.0 - effective_resist / 100.0, resist_form),
                    step("%.2f ^8(increased/reduced damage taken)", 1.0 + taken / 100.0),
                    step("%.2f ^8(more/less damage taken)", taken_more),
                ],
                total: Some(sf!("= %.3f", mult)),
                ..Chain::default()
            },
        );
    } else {
        out.push("Effective DPS modifier:");
        out.push(sf!("= %.3f ^8(increased/reduced damage taken)", mult));
    }
    out
}

/// CalcBreakdown.lua:162-173 `breakdown.dot(out, baseVal, inc, more, mult, rate, aura, effMult, total)` (`mult` nil -> 0; a nil
/// `rate` / `aura` / `effMult` is a chain step multiChain skips, `mult[2]` nil).
#[allow(clippy::too_many_arguments)]
pub fn dot(
    out: &mut BTable,
    base_val: f64,
    inc: f64,
    more: f64,
    mult: Option<f64>,
    rate: Option<f64>,
    aura: Option<f64>,
    eff_mult: Option<f64>,
    total: f64,
) {
    let mut steps = vec![
        step("%.2f ^8(increased/reduced)", 1.0 + inc / 100.0),
        step("%.2f ^8(more/less)", more),
        step("%.2f ^8(multiplier)", 1.0 + mult.unwrap_or(0.0) / 100.0),
    ];
    if let Some(r) = rate {
        steps.push(step("%.2f ^8(rate modifier)", r));
    }
    if let Some(a) = aura {
        steps.push(step("%.3f ^8(aura effect modifier)", a));
    }
    if let Some(e) = eff_mult {
        steps.push(step("%.3f ^8(effective DPS modifier)", e));
    }
    multi_chain(
        out,
        &Chain {
            base: Some(step("%.1f ^8(base damage per second)", base_val)),
            steps,
            total: Some(sf!("= %.1f ^8per second", total)),
            ..Chain::default()
        },
    );
}

/// CalcBreakdown.lua:175-196 `breakdown.critDot(dotMulti, critMulti, dotChance, critChance)`.
pub fn crit_dot(dot_multi: f64, crit_multi: f64, dot_chance: f64, crit_chance: f64) -> BTable {
    let combined = (dot_multi * dot_chance) + (crit_multi * crit_chance);
    let mut out = BTable::new();
    if dot_chance > 0.0 {
        out.push("Contribution from Non-crits:");
        out.push(sf!("%.2f ^8(dot multiplier for non-crits)", dot_multi));
        out.push(sf!("x %.4f ^8(portion of instances created by non-crits)", dot_chance));
        out.push(sf!("= %.2f", dot_multi * dot_chance));
    }
    if crit_chance > 0.0 {
        out.push("Contribution from Crits:");
        out.push(sf!("%.2f ^8(dot multiplier for crits)", crit_multi));
        out.push(sf!("x %.4f ^8(portion of instances created by crits)", crit_chance));
        out.push(sf!("= %.2f", crit_multi * crit_chance));
    }
    if (dot_chance > 0.0 && crit_chance > 0.0) && (dot_multi != crit_multi) {
        out.push("Effective DoT Multiplier:");
        out.push(sf!("%.2f + %.2f", dot_multi * dot_chance, crit_multi * crit_chance));
        out.push(sf!("= %.2f", combined));
    }
    out
}

/// CalcBreakdown.lua:198-252 `breakdown.leech(instant, instantRate, instances, pool, rate, max, dur, instantLeechProportion, hitRate)`:
/// `show_average` = `actor.mainSkill.skillData.showAverage`, `rate_mod` = `calcLib.mod(modDB, skillCfg, rate)` (the module's `modDB`;
/// `skillCfg` there is the global, nil).
#[allow(clippy::too_many_arguments)]
pub fn leech(
    show_average: bool,
    rate_mod: f64,
    instant: f64,
    instant_rate: f64,
    instances: f64,
    pool: f64,
    max: f64,
    dur: f64,
    instant_leech_proportion: f64,
    hit_rate: f64,
) -> BTable {
    let base = crate::setup::misc("LeechRateBase");
    let mut out = BTable::new();
    let instant_line = |out: &mut BTable| {
        if instant_leech_proportion != 1.0 {
            out.push(sf!(
                "Instant Leech: %.1f ^8(%d%% x %.1f)",
                instant,
                instant_leech_proportion * 100.0,
                dur * pool * base / (1.0 - instant_leech_proportion)
            ));
        } else {
            out.push(sf!("Instant Leech: %.1f", instant));
        }
    };
    if show_average {
        if instant > 0.0 {
            instant_line(&mut out);
        }
        if instances > 0.0 {
            out.push("Total leeched per instance:");
            out.push(sf!("%d ^8(size of leech destination pool)", pool));
            out.push(sf!("x %.2f ^8(base leech rate is %d%% per second)", base, 100.0 * base));
            if rate_mod != 1.0 {
                out.push(sf!("x %.2f ^8(leech rate modifier)", rate_mod));
            }
            out.push(sf!("x %.2fs ^8(instance duration)", dur));
            out.push(sf!("= %.1f", pool * base * rate_mod * dur));
        }
    } else {
        if instant_rate > 0.0 {
            instant_line(&mut out);
            out.push(sf!(
                "Instant Leech per second: %.1f ^8(%.1f x %.2f)",
                instant_rate,
                instant,
                hit_rate
            ));
        }
        if instances > 0.0 {
            out.push("Rate per instance:");
            out.push(sf!("%d ^8(size of leech destination pool)", pool));
            out.push(sf!("x %.2f ^8(base leech rate is %d%% per second)", base, 100.0 * base));
            if rate_mod != 1.0 {
                out.push(sf!("x %.2f ^8(leech rate modifier)", rate_mod));
            }
            out.push(sf!("= %.1f ^8per second", pool * base * rate_mod));
            out.push("Maximum leech rate against one target:");
            out.push(sf!("%.1f", pool * base * rate_mod));
            out.push(sf!("x %.2f ^8(average instances)", instances));
            let total = pool * base * rate_mod * instances;
            out.push(sf!("= %.1f ^8per second", total));
            if total <= max {
                out.push(sf!("Time to reach max: %.1fs", dur));
            }
            out.push(sf!("Leech rate cap: %.1f", max));
            if total > max {
                out.push(sf!("Time to reach cap: %.1fs", dur / total * max));
            }
        }
    }
    out
}

/// `colorCodes[name]` (Data/Global.lua).
pub fn color(name: &str) -> &'static str {
    pob_data::globals::globals()
        .color_codes
        .get(name)
        .map(String::as_str)
        .unwrap_or_else(|| panic!("colorCodes.{name} is nil"))
}
