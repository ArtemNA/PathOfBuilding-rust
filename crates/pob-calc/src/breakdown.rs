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
#[derive(Clone, Debug, Default)]
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
