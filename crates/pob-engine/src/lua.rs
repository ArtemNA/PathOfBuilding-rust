//! The few Lua runtime semantics the display layer evaluates (`BuildDisplayStats.lua` functions, Build.lua's sidebar code): a borrowed
//! Lua value ([`LuaV`]), truthiness, `==`/`<`/`<=` with LuaJIT's error texts, arithmetic coercion, and `table.sort` (LuaJIT `lib_table.c`
//! `auxsort`, the Lua 5.1 quicksort -- not stable, so ties keep the order that algorithm leaves, not the input order).

use std::rc::Rc;

use indexmap::IndexMap;
use pob_calc::{OutTable, OutValue};
use pob_mods::Str;

/// A table value `actor.output` holds: one of [`OutTable`], or `output.MainHand` / `output.OffHand` (kept in the offence state).
#[derive(Clone, Copy, Debug)]
pub enum TableRef<'a> {
    Out(&'a OutTable),
    Hand(&'a IndexMap<Str, OutValue>),
}

/// A Lua value as the display functions see it.
#[derive(Clone, Debug)]
pub enum LuaV<'a> {
    Nil,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Table(TableRef<'a>),
}

pub type LuaRes<'a> = Result<LuaV<'a>, String>;

impl<'a> LuaV<'a> {
    pub fn str(s: &str) -> LuaV<'a> {
        LuaV::Str(Rc::from(s))
    }

    pub fn from_out(v: &OutValue) -> LuaV<'a> {
        match v {
            OutValue::Num(n) => LuaV::Num(*n),
            OutValue::Bool(b) => LuaV::Bool(*b),
            OutValue::Str(s) => LuaV::Str(s.clone()),
        }
    }

    /// `type(v)`.
    pub fn type_name(&self) -> &'static str {
        match self {
            LuaV::Nil => "nil",
            LuaV::Bool(_) => "boolean",
            LuaV::Num(_) => "number",
            LuaV::Str(_) => "string",
            LuaV::Table(_) => "table",
        }
    }

    /// Lua truthiness: everything but `nil` and `false`.
    pub fn truthy(&self) -> bool {
        !matches!(self, LuaV::Nil | LuaV::Bool(false))
    }

    /// `t[k]` for a table value; indexing a non-table is the Lua error.
    pub fn index(&self, k: &str) -> LuaRes<'a> {
        match self {
            LuaV::Table(TableRef::Hand(m)) => Ok(m.get(k).map_or(LuaV::Nil, LuaV::from_out)),
            LuaV::Table(TableRef::Out(t)) => Err(format!("display: indexing output table {t:?} with '{k}' is not modelled")),
            other => Err(format!("attempt to index a {} value", other.type_name())),
        }
    }
}

/// `a == b` (raw equality: numbers by value, strings by content; tables by identity -- never equal here, no display code compares tables).
pub fn eq(a: &LuaV<'_>, b: &LuaV<'_>) -> bool {
    match (a, b) {
        (LuaV::Nil, LuaV::Nil) => true,
        (LuaV::Bool(x), LuaV::Bool(y)) => x == y,
        (LuaV::Num(x), LuaV::Num(y)) => x == y,
        (LuaV::Str(x), LuaV::Str(y)) => x == y,
        _ => false,
    }
}

/// `a ~= b`.
pub fn ne(a: &LuaV<'_>, b: &LuaV<'_>) -> bool {
    !eq(a, b)
}

fn cmp_err(a: &LuaV<'_>, b: &LuaV<'_>) -> String {
    if a.type_name() == b.type_name() {
        format!("attempt to compare two {} values", a.type_name())
    } else {
        format!("attempt to compare {} with {}", a.type_name(), b.type_name())
    }
}

/// `a < b` (numbers, or strings byte-wise as in the C locale); anything else is the Lua error.
pub fn lt(a: &LuaV<'_>, b: &LuaV<'_>) -> Result<bool, String> {
    match (a, b) {
        (LuaV::Num(x), LuaV::Num(y)) => Ok(x < y),
        (LuaV::Str(x), LuaV::Str(y)) => Ok(x.as_bytes() < y.as_bytes()),
        _ => Err(cmp_err(a, b)),
    }
}

/// `a <= b`.
pub fn le(a: &LuaV<'_>, b: &LuaV<'_>) -> Result<bool, String> {
    match (a, b) {
        (LuaV::Num(x), LuaV::Num(y)) => Ok(x <= y),
        (LuaV::Str(x), LuaV::Str(y)) => Ok(x.as_bytes() <= y.as_bytes()),
        _ => Err(cmp_err(a, b)),
    }
}

/// `a > b` (the VM evaluates `b < a`).
pub fn gt(a: &LuaV<'_>, b: &LuaV<'_>) -> Result<bool, String> {
    lt(b, a)
}

/// `a >= b` (the VM evaluates `b <= a`).
pub fn ge(a: &LuaV<'_>, b: &LuaV<'_>) -> Result<bool, String> {
    le(b, a)
}

/// The number an arithmetic operand stands for: a number, or a string `tonumber` accepts (Lua's coercion); else the Lua error.
pub fn arith(a: &LuaV<'_>) -> Result<f64, String> {
    match a {
        LuaV::Num(n) => Ok(*n),
        LuaV::Str(s) => pob_lua::tonumber(s).ok_or_else(|| "attempt to perform arithmetic on a string value".to_owned()),
        other => Err(format!("attempt to perform arithmetic on a {} value", other.type_name())),
    }
}

/// `table.sort(t, comp)` -- LuaJIT `lib_table.c` `auxsort` (Lua 5.1), element moves and comparator calls in the same order, so that
/// equal keys end where LuaJIT leaves them. `lt(a, b)` is `comp(a, b)`. An inconsistent comparator is LuaJIT's
/// "invalid order function for sorting" error.
pub fn lua_sort<T: Clone>(a: &mut [T], lt: &mut dyn FnMut(&T, &T) -> bool) {
    if a.len() > 1 {
        auxsort(a, 1, a.len(), lt);
    }
}

fn auxsort<T: Clone>(a: &mut [T], mut l: usize, mut u: usize, lt: &mut dyn FnMut(&T, &T) -> bool) {
    // 1-based indices as in the C source; `a[k]` is `a[k - 1]`.
    while l < u {
        // sort elements a[l], a[(l+u)/2] and a[u]
        if lt(&a[u - 1], &a[l - 1]) {
            a.swap(l - 1, u - 1);
        }
        if u - l == 1 {
            break;
        }
        let mut i = (l + u) / 2;
        if lt(&a[i - 1], &a[l - 1]) {
            a.swap(i - 1, l - 1);
        } else if lt(&a[u - 1], &a[i - 1]) {
            a.swap(i - 1, u - 1);
        }
        if u - l == 2 {
            break;
        }
        let p = a[i - 1].clone();
        // set2(i, u-1): a[i] = a[u-1], a[u-1] = P
        a.swap(i - 1, u - 2);
        // a[l] <= P == a[u-1] <= a[u], only need to sort from l+1 to u-2
        i = l;
        let mut j = u - 1;
        loop {
            loop {
                i += 1;
                if !lt(&a[i - 1], &p) {
                    break;
                }
                if i >= u {
                    panic!("invalid order function for sorting");
                }
            }
            loop {
                j -= 1;
                if !lt(&p, &a[j - 1]) {
                    break;
                }
                if j <= l {
                    panic!("invalid order function for sorting");
                }
            }
            if j < i {
                break;
            }
            a.swap(i - 1, j - 1);
        }
        // swap pivot (a[u-1]) with a[i]
        a.swap(u - 2, i - 1);
        if i - l < u - i {
            j = l;
            i -= 1;
            l = i + 2;
        } else {
            j = i + 1;
            i = u;
            u = j - 2;
        }
        auxsort(a, j, i, lt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_like_a_sort() {
        let mut v: Vec<i32> = vec![5, 3, 9, 1, 1, 8, 2, 7, 6, 4, 0, 9];
        lua_sort(&mut v, &mut |a, b| a < b);
        assert_eq!(v, vec![0, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 9]);
        let mut w: Vec<i32> = (0..100).map(|k| (k * 37) % 101).collect();
        lua_sort(&mut w, &mut |a, b| a > b);
        assert!(w.windows(2).all(|p| p[0] >= p[1]));
    }

    /// Three equal keys: every median-of-three compare is false, nothing moves.
    #[test]
    fn equal_keys_three() {
        let mut v = vec![(1, 'a'), (1, 'b'), (1, 'c')];
        lua_sort(&mut v, &mut |a, b| a.0 > b.0);
        assert_eq!(v, vec![(1, 'a'), (1, 'b'), (1, 'c')]);
    }
}
