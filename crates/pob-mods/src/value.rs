//! The dynamic Lua value of the mod core (orchestrator decision 1, R2.2): mod values, tag fields and `ModValueTable`s are
//! Lua values in upstream and stay dynamic here. Tables are copy-on-write (`Rc::make_mut`): `copyTable` + a write is the
//! same thing as a COW write, because the core never observes table identity (only `==` on values, and no two distinct
//! tables ever meet in an `==` of the core). Truthiness is Lua's: only `nil` and `false` are false (MINEFIELD L4).

use std::any::Any;
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::types::Mod;

/// An interned-by-`Rc` Lua string (stat names, sources, tag fields).
pub type Str = Rc<str>;

/// A Lua value as the mod core sees it. Functions/userdata never reach the core.
#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Nil,
    Bool(bool),
    Num(f64),
    Str(Str),
    Table(Rc<Table>),
    /// A mod table nested in a value (`{ mod = modLib.createMod(...) }`, ModParser) or returned by a query.
    Mod(Rc<Mod>),
    /// A Lua function value (R5.4: the radius-jewel visitors of ModParser's `jewelFuncList`, `{ func = f, type = ... }`):
    /// DATA only -- the core never calls it; `pob-parser` executes it through its registry by `id`.
    Func(Rc<ClosureRef>),
}

/// A reference to a Lua closure: the upstream definition (`"<src>:<line>"`) plus its upvalues, opaque to the core (the
/// owner -- `pob-parser` -- downcasts `up`). Identity is the `Rc` (Lua compares functions by reference).
pub struct ClosureRef {
    pub id: Str,
    pub up: Rc<dyn Any>,
}

impl fmt::Debug for ClosureRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "function<{}>", self.id)
    }
}

/// LuaJIT `tostring(f)` is `function: 0x<address>`; the address is not reproducible, so every place a function reaches a
/// string prints this placeholder (the R5.1 fixtures normalise LuaJIT's text the same way).
pub const FUNCTION_TOSTRING: &str = "function: <addr>";

/// A Lua table: the array part `1..n` (what `ipairs` and `#` see) plus string keys in insertion order.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub array: Vec<Value>,
    pub hash: IndexMap<Str, Value>,
}

const NIL: &Value = &Value::Nil;

impl Table {
    /// `t.key` for a string key.
    pub fn get(&self, key: &str) -> &Value {
        self.hash.get(key).unwrap_or(NIL)
    }

    /// `t[k]` for any key: strings hit the hash part, integral numbers `1..=#t` the array part, everything else is nil.
    pub fn index(&self, key: &Value) -> Value {
        match key {
            Value::Str(s) => self.get(s).clone(),
            Value::Num(n) => array_slot(*n, self.array.len()).map_or(Value::Nil, |i| self.array[i].clone()),
            _ => Value::Nil,
        }
    }

    /// `t[k] = v` for a string or in-range integral key (the only writes the core makes).
    pub fn set(&mut self, key: &Value, v: Value) {
        match key {
            Value::Str(s) => {
                self.hash.insert(s.clone(), v);
            }
            Value::Num(n) => match array_slot(*n, self.array.len() + 1) {
                Some(i) if i == self.array.len() => self.array.push(v),
                Some(i) => self.array[i] = v,
                None => unimplemented!("Table::set on a non-sequence number key {n} (the core never writes one)"),
            },
            _ => unimplemented!("Table::set with a {} key (the core never writes one)", key.type_name()),
        }
    }

    /// `#t` / the length `ipairs` walks.
    pub fn len(&self) -> usize {
        self.array.len()
    }

    pub fn is_empty(&self) -> bool {
        self.array.is_empty() && self.hash.is_empty()
    }
}

/// 0-based slot of a 1-based integral Lua key `n` within `len`, if any.
fn array_slot(n: f64, len: usize) -> Option<usize> {
    if n >= 1.0 && n.fract() == 0.0 && n <= len as f64 {
        Some(n as usize - 1)
    } else {
        None
    }
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }

    pub fn table(t: Table) -> Value {
        Value::Table(Rc::new(t))
    }

    /// Lua truthiness (L4): `0`, `""` and NaN are true.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    /// Lua `type(v)`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "boolean",
            Value::Num(_) => "number",
            Value::Str(_) => "string",
            Value::Table(_) | Value::Mod(_) => "table",
            Value::Func(_) => "function",
        }
    }

    /// Lua `==` (no metamethods in the core): numbers by value (NaN ~= NaN), strings by content, tables by identity.
    pub fn raw_eq(&self, o: &Value) -> bool {
        match (self, o) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Num(a), Value::Num(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Table(a), Value::Table(b)) => Rc::ptr_eq(a, b),
            (Value::Mod(a), Value::Mod(b)) => Rc::ptr_eq(a, b),
            (Value::Func(a), Value::Func(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    pub fn as_num(&self) -> Option<f64> {
        if let Value::Num(n) = self { Some(*n) } else { None }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let Value::Str(s) = self { Some(s) } else { None }
    }

    /// `v == "literal"`.
    pub fn is_str(&self, s: &str) -> bool {
        self.as_str() == Some(s)
    }

    /// `v.key` on a table value (a mod table exposes its record fields); nil for non-tables. Indexing a non-table is an
    /// error in Lua -- the callers that can reach that check `type_name` first and raise the error with their own site.
    pub fn get(&self, key: &str) -> Value {
        match self {
            Value::Table(t) => t.get(key).clone(),
            Value::Mod(m) => m.field(key),
            _ => Value::Nil,
        }
    }

    /// `v.key = x` on a table value, copy-on-write.
    pub fn set(&mut self, key: &str, x: Value) {
        match self {
            Value::Table(t) => Rc::make_mut(t).set(&Value::str(key), x),
            Value::Mod(m) => Rc::make_mut(m).set_field(key, x),
            _ => unimplemented!("Value::set on a {} (callers check for a table first)", self.type_name()),
        }
    }

    /// `v[k] = x` on a table value, copy-on-write.
    pub fn set_index(&mut self, k: &Value, x: Value) {
        match (self, k) {
            (Value::Table(t), _) => Rc::make_mut(t).set(k, x),
            (Value::Mod(m), Value::Str(s)) => Rc::make_mut(m).set_field(s, x),
            (v, _) => unimplemented!("Value::set_index on a {} (callers check for a table first)", v.type_name()),
        }
    }

    /// `v[k]` on a table value.
    pub fn index(&self, k: &Value) -> Value {
        match self {
            Value::Table(t) => t.index(k),
            Value::Mod(m) => k.as_str().map_or(Value::Nil, |s| m.field(s)),
            _ => Value::Nil,
        }
    }

    pub fn is_table(&self) -> bool {
        matches!(self, Value::Table(_) | Value::Mod(_))
    }

    /// The array part as `ipairs` walks it (a mod table's array part are its tags).
    pub fn ipairs(&self) -> Vec<Value> {
        match self {
            Value::Table(t) => t.array.clone(),
            Value::Mod(m) => m.tags.iter().map(|t| Value::Table(t.table())).collect(),
            _ => Vec::new(),
        }
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Num(n)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<Option<bool>> for Value {
    fn from(b: Option<bool>) -> Self {
        b.map_or(Value::Nil, Value::Bool)
    }
}

impl From<Option<f64>> for Value {
    fn from(n: Option<f64>) -> Self {
        n.map_or(Value::Nil, Value::Num)
    }
}

impl From<Option<&Str>> for Value {
    fn from(s: Option<&Str>) -> Self {
        s.map_or(Value::Nil, |s| Value::Str(s.clone()))
    }
}

// --- errors -------------------------------------------------------------------------------------------------------

/// A Lua runtime error with LuaJIT's message text, prefixed by the upstream `file:line` (L18; the corpus compares texts).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuaError(pub String);

pub type LuaResult<T> = Result<T, LuaError>;

impl fmt::Display for LuaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LuaError {}

/// LuaJIT `ERR_BADOPRT`/`ERR_BADOPRV` (lj_errmsg.h): `attempt to <op> <desc> (a <type> value)` when the operand has a name
/// (`field 'value'`, `local 'var'`, `method 'X'`), `attempt to <op> a <type> value` when it has none.
pub(crate) fn err_op(site: &str, op: &str, desc: Option<&str>, v: &Value) -> LuaError {
    match desc {
        Some(d) => LuaError(format!("{site}: attempt to {op} {d} (a {} value)", v.type_name())),
        None => LuaError(format!("{site}: attempt to {op} a {} value", v.type_name())),
    }
}

/// LuaJIT `ERR_BADCMPT`/`ERR_BADCMPV`: operands in VM order (`a > b` compiles to `b < a`).
pub(crate) fn err_compare(site: &str, a: &Value, b: &Value) -> LuaError {
    let (ta, tb) = (a.type_name(), b.type_name());
    if ta == tb {
        LuaError(format!("{site}: attempt to compare two {ta} values"))
    } else {
        LuaError(format!("{site}: attempt to compare {ta} with {tb}"))
    }
}

/// LuaJIT `ERR_BADARG` with `ERR_ARGTYPE`: `bad argument #n to 'f' (<expected> expected, got <type>)`.
pub(crate) fn err_bad_arg(site: &str, n: u32, fname: &str, expected: &str, got: &Value) -> LuaError {
    // An explicitly passed nil reads "got nil" (fixture: ModStore.lua:747 `pairs(nil)`); "no value" is only a missing arg.
    LuaError(format!(
        "{site}: bad argument #{n} to '{fname}' ({expected} expected, got {})",
        got.type_name()
    ))
}

/// A value in Lua arithmetic: numbers as they are, numeric strings coerced like the VM does, anything else the error.
pub(crate) fn arith(v: &Value, site: &str, desc: Option<&str>) -> LuaResult<f64> {
    match v {
        Value::Num(n) => Ok(*n),
        Value::Str(s) => pob_lua::tonumber(s).ok_or_else(|| err_op(site, "perform arithmetic on", desc, v)),
        _ => Err(err_op(site, "perform arithmetic on", desc, v)),
    }
}

/// A numeric argument of a library function (`math.min`, `bit.band`): `lj_lib_checknum` coerces numeric strings (L6).
pub(crate) fn check_num(v: &Value, site: &str, n: u32, fname: &str) -> LuaResult<f64> {
    match v {
        Value::Num(x) => Ok(*x),
        Value::Str(s) => pob_lua::tonumber(s).ok_or_else(|| err_bad_arg(site, n, fname, "number", v)),
        _ => Err(err_bad_arg(site, n, fname, "number", v)),
    }
}

/// `a .. b` operand: strings as they are, numbers through `tostring` (`%.14g`, L1); anything else the concat error.
pub(crate) fn concat_str(v: &Value, site: &str, desc: Option<&str>) -> LuaResult<Str> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        Value::Num(n) => Ok(Rc::from(pob_lua::tostring_num(*n))),
        _ => Err(err_op(site, "concatenate", desc, v)),
    }
}
