//! `scan(line, patternList, plain)` -- ModParser.lua:6616-6638, verbatim:
//!
//! ```lua
//! local lineLower = line:lower()
//! for pattern, patternVal in pairs(patternList) do
//!     local index, endIndex, cap1, cap2, cap3, cap4, cap5 = lineLower:find(pattern, 1, plain)
//!     if index and (not bestIndex or index < bestIndex or (index == bestIndex and (endIndex > bestEndIndex
//!         or (endIndex == bestEndIndex and #pattern > #bestPattern)))) then ... best = this
//! end
//! return bestVal, line:sub(1, bestStart - 1) .. line:sub(bestEnd + 1, -1), { cap1, .., cap5 }   -- or nil, line
//! ```
//!
//! Order (D13/D15): `pairs` is a hash walk; here the walk is the data's byte-sorted key order. The rule only depends on
//! the order for an exact tie (same index, endIndex and pattern length): the FIRST in sorted order wins (strict `>`).
//! Every tie is counted (`tie_stats`) -- with the values (or captures) of the tied patterns compared -- so the report can say
//! whether the walk order can change a result.
//!
//! Speed: `find` runs on every pattern of the table (specialModList 3 112). A pattern is skipped without calling `find` when
//! the lowered line lacks a literal the pattern cannot match without (`Filter`): exact, because such a `find` returns nil.

use std::cell::RefCell;
use std::collections::HashMap;

use pob_data::mod_parser::{FnRef, PVal, PatternTable};
use pob_lua::{Capture, Captures};
use pob_mods::{LuaError, Mod, Value};

use crate::ParseError;

/// What a pattern needs to possibly match `line:lower()` from `init = 1`.
#[derive(Debug, Default)]
struct Filter {
    /// An anchored pattern's literal start (`^foo`): the line must start with it.
    prefix: Box<[u8]>,
    /// The longest run of required literal bytes: the line must contain it.
    contains: Box<[u8]>,
}

#[derive(Debug)]
struct Pat {
    p: Box<[u8]>,
    filter: Filter,
}

/// A table's patterns prepared for `scan`. Each pattern with a required literal of >= 2 bytes is filed under one byte pair
/// of it (`buckets`); a scan visits only the buckets of the pairs the line has, plus `always`, in table order -- the
/// skipped patterns cannot match, so the walk (and the tie rule) is the full walk's.
#[derive(Debug)]
pub struct ScanTable {
    pats: Vec<Pat>,
    buckets: HashMap<u16, Vec<u32>>,
    always: Vec<u32>,
}

/// Rough commonness of a lowercase byte in English mod text (higher = more common); picks a selective pair.
fn commonness(b: u8) -> u32 {
    const ORDER: &[u8] = b" etaoinsrhldcumfpgwybvkxjqz";
    ORDER.iter().position(|&c| c == b).map_or(0, |p| (ORDER.len() - p) as u32)
}

impl ScanTable {
    pub fn new(t: &PatternTable) -> ScanTable {
        let mut st = ScanTable {
            pats: Vec::with_capacity(t.len()),
            buckets: HashMap::new(),
            always: Vec::new(),
        };
        for (i, k) in t.keys.iter().enumerate() {
            let filter = filter_of(k.as_bytes());
            let pair = filter
                .contains
                .windows(2)
                .min_by_key(|w| commonness(w[0]) + commonness(w[1]))
                .map(|w| u16::from_be_bytes([w[0], w[1]]));
            let i = u32::try_from(i).expect("table size");
            match pair {
                Some(p) => st.buckets.entry(p).or_default().push(i),
                None => st.always.push(i),
            }
            st.pats.push(Pat {
                p: k.as_bytes().into(),
                filter,
            });
        }
        st
    }

    /// The patterns that can match `lower` (from init 1), ascending, with their pattern bytes.
    pub(crate) fn filtered<'a>(&'a self, lower: &'a [u8]) -> impl Iterator<Item = (usize, &'a [u8])> + 'a {
        self.candidates(lower).into_iter().filter_map(move |i| {
            let pat = &self.pats[i as usize];
            (lower.starts_with(&pat.filter.prefix) && contains(lower, &pat.filter.contains)).then_some((i as usize, &*pat.p))
        })
    }

    /// The patterns worth a `find` on `lower`, ascending.
    fn candidates(&self, lower: &[u8]) -> Vec<u32> {
        let mut out = self.always.clone();
        let mut seen: Vec<u16> = Vec::with_capacity(lower.len());
        for w in lower.windows(2) {
            let c = u16::from_be_bytes([w[0], w[1]]);
            if seen.contains(&c) {
                continue;
            }
            seen.push(c);
            if let Some(b) = self.buckets.get(&c) {
                out.extend_from_slice(b);
            }
        }
        out.sort_unstable();
        out
    }
}

/// End of a `[set]` starting at `p[i] == '['` (lstrlib `classEnd`), or None when unfinished.
fn class_end(p: &[u8], mut i: usize) -> Option<usize> {
    i += 1;
    if p.get(i) == Some(&b'^') {
        i += 1;
    }
    loop {
        let c = *p.get(i)?;
        i += 1;
        if c == b'%' {
            i += 1;
        }
        if p.get(i)? == &b']' {
            return Some(i + 1);
        }
    }
}

/// The literal requirements of a Lua pattern (conservative: anything unusual gives no filter).
fn filter_of(p: &[u8]) -> Filter {
    let (body, anchored) = match p.first() {
        Some(b'^') => (&p[1..], true),
        _ => (p, false),
    };
    let mut best: Vec<u8> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut prefix: Option<Vec<u8>> = None;
    let mut at_start = anchored;
    let end_run = |cur: &mut Vec<u8>, best: &mut Vec<u8>, prefix: &mut Option<Vec<u8>>, at_start: &mut bool| {
        if *at_start {
            *prefix = Some(cur.clone());
            *at_start = false;
        }
        if cur.len() > best.len() {
            *best = cur.clone();
        }
        cur.clear();
    };
    let mut i = 0;
    while i < body.len() {
        let c = body[i];
        if c == b'(' || c == b')' {
            i += 1;
            continue;
        }
        if c == b'$' && i + 1 == body.len() {
            break;
        }
        let (lit, next) = match c {
            b'%' => match body.get(i + 1) {
                None => return Filter::default(),
                Some(b'b' | b'f') => return Filter::default(),
                Some(d) if d.is_ascii_alphanumeric() => (None, i + 2),
                Some(&d) => (Some(d), i + 2),
            },
            b'[' => match class_end(body, i) {
                Some(e) => (None, e),
                None => return Filter::default(),
            },
            b'.' => (None, i + 1),
            _ => (Some(c), i + 1),
        };
        match (body.get(next), lit) {
            (Some(b'*' | b'-' | b'?'), _) => {
                end_run(&mut cur, &mut best, &mut prefix, &mut at_start);
                i = next + 1;
            }
            (Some(b'+'), Some(l)) => {
                cur.push(l);
                end_run(&mut cur, &mut best, &mut prefix, &mut at_start);
                cur.push(l);
                i = next + 1;
            }
            (Some(b'+'), None) => {
                end_run(&mut cur, &mut best, &mut prefix, &mut at_start);
                i = next + 1;
            }
            (_, Some(l)) => {
                cur.push(l);
                i = next;
            }
            (_, None) => {
                end_run(&mut cur, &mut best, &mut prefix, &mut at_start);
                i = next;
            }
        }
    }
    end_run(&mut cur, &mut best, &mut prefix, &mut at_start);
    Filter {
        prefix: prefix.unwrap_or_default().into(),
        contains: best.into(),
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    match needle.len() {
        0 => true,
        n if n > hay.len() => false,
        _ => {
            let first = needle[0];
            (0..=hay.len() - needle.len()).any(|i| hay[i] == first && &hay[i..i + needle.len()] == needle)
        }
    }
}

/// What `scan` found: the entry index, the line without the match, and `{ cap1, .., cap5 }` (from `lineLower`).
#[derive(Debug)]
pub struct Hit {
    pub entry: usize,
    pub rest: Vec<u8>,
    pub caps: Vec<Value>,
}

/// Counters of exact ties (D13): `ties` = scans whose winner tied with another matching pattern; `diff_value` = ... where a
/// tied pattern's value differs from the winner's; `diff_caps` = ... where only the captures differ.
#[derive(Clone, Debug, Default)]
pub struct TieStats {
    pub scans: u64,
    pub ties: u64,
    pub diff_value: u64,
    pub diff_caps: u64,
    pub examples: Vec<String>,
}

thread_local! {
    static TIES: RefCell<TieStats> = RefCell::new(TieStats::default());
}

pub fn tie_stats() -> TieStats {
    TIES.with(|t| t.borrow().clone())
}

pub fn reset_tie_stats() {
    TIES.with(|t| *t.borrow_mut() = TieStats::default());
}

fn caps_values(lower: &[u8], caps: &Captures) -> Result<Vec<Value>, ParseError> {
    caps.iter()
        .take(5)
        .map(|c| match c {
            Capture::Str { start, end } => std::str::from_utf8(&lower[start..end])
                .map(Value::str)
                .map_err(|_| ParseError::Unrepresentable("a capture splitting a UTF-8 sequence".into())),
            Capture::Pos(p) => Ok(Value::Num(p as f64)),
        })
        .collect()
}

/// `scan(line, patternList, plain)`. `name` is the table's name (tie reports only).
pub fn scan(line: &[u8], t: &PatternTable, st: &ScanTable, plain: bool, name: &str) -> Result<Option<Hit>, ParseError> {
    let lower = line.to_ascii_lowercase();
    let mut best: Option<(usize, usize, usize, Captures)> = None; // (entry, index, endIndex, caps)
    let mut ties: Vec<(usize, Captures)> = Vec::new();
    for i in st.candidates(&lower) {
        let i = i as usize;
        let pat = &st.pats[i];
        if !lower.starts_with(&pat.filter.prefix) || !contains(&lower, &pat.filter.contains) {
            continue;
        }
        let Some((index, end, caps)) = pob_lua::find(&lower, &pat.p, Some(1), plain).map_err(|e| ParseError::Lua(LuaError(e.0)))? else {
            continue;
        };
        let better = match &best {
            None => true,
            Some((bi, bidx, bend, _)) => {
                index < *bidx || (index == *bidx && (end > *bend || (end == *bend && pat.p.len() > st.pats[*bi].p.len())))
            }
        };
        if better {
            best = Some((i, index, end, caps));
            ties.clear();
        } else if let Some((bi, bidx, bend, _)) = &best
            && index == *bidx
            && end == *bend
            && pat.p.len() == st.pats[*bi].p.len()
        {
            ties.push((i, caps));
        }
    }
    let Some((entry, index, end, caps)) = best else { return Ok(None) };
    let cap_vals = caps_values(&lower, &caps)?;
    TIES.with(|s| {
        let mut s = s.borrow_mut();
        s.scans += 1;
        if ties.is_empty() {
            return;
        }
        s.ties += 1;
        let mut dv = false;
        let mut dc = false;
        for (j, c) in &ties {
            if !pval_eq(&t.vals[*j], &t.vals[entry]) {
                dv = true;
            } else if caps_values(&lower, c).ok().is_none_or(|cv| !values_eq(&cv, &cap_vals)) {
                dc = true;
            }
        }
        if dv || dc {
            if dv {
                s.diff_value += 1;
            } else {
                s.diff_caps += 1;
            }
            if s.examples.len() < 20 {
                let others: Vec<&str> = ties.iter().map(|(j, _)| &*t.keys[*j]).collect();
                s.examples.push(format!(
                    "{name}: '{}' -> '{}' ties {:?}{}",
                    String::from_utf8_lossy(line),
                    t.keys[entry],
                    others,
                    if dv { " (values differ)" } else { " (captures differ)" }
                ));
            }
        }
    });
    let mut rest = Vec::with_capacity(line.len());
    rest.extend_from_slice(&line[..index - 1]);
    rest.extend_from_slice(&line[end..]);
    Ok(Some(Hit {
        entry,
        rest,
        caps: cap_vals,
    }))
}

// --- structural equality (tie reports, tests) ----------------------------------------------------------------------

fn values_eq(a: &[Value], b: &[Value]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| value_eq(x, y))
}

/// Structural equality of Lua values (numbers by bits, tables by content, mods field by field).
pub fn value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Num(x), Value::Num(y)) => x.to_bits() == y.to_bits(),
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Table(x), Value::Table(y)) => {
            values_eq(&x.array, &y.array)
                && x.hash.len() == y.hash.len()
                && x.hash.iter().all(|(k, v)| y.hash.get(k).is_some_and(|w| value_eq(v, w)))
        }
        (Value::Mod(x), Value::Mod(y)) => mod_eq(x, y),
        _ => false,
    }
}

pub fn mod_eq(a: &Mod, b: &Mod) -> bool {
    a.name == b.name
        && a.ty == b.ty
        && value_eq(&a.value, &b.value)
        && a.flags == b.flags
        && a.keyword_flags == b.keyword_flags
        && a.source == b.source
        && a.tags.len() == b.tags.len()
        && a.tags
            .iter()
            .zip(&b.tags)
            .all(|(x, y)| value_eq(&Value::Table(x.table()), &Value::Table(y.table())))
}

fn fn_eq(a: &FnRef, b: &FnRef) -> bool {
    a.id == b.id && a.up.len() == b.up.len() && a.up.iter().all(|(k, v)| b.up.get(k).is_some_and(|w| pval_eq(v, w)))
}

pub fn pval_eq(a: &PVal, b: &PVal) -> bool {
    match (a, b) {
        (PVal::Lua(x), PVal::Lua(y)) => value_eq(x, y),
        (PVal::Fn(x), PVal::Fn(y)) => fn_eq(x, y),
        (PVal::CFn, PVal::CFn) => true,
        (PVal::Named(x), PVal::Named(y)) => x == y,
        (PVal::Table { array: a1, hash: h1 }, PVal::Table { array: a2, hash: h2 }) => {
            a1.len() == a2.len()
                && a1.iter().zip(a2).all(|(x, y)| pval_eq(x, y))
                && h1.len() == h2.len()
                && h1.iter().all(|(k, v)| h2.get(k).is_some_and(|w| pval_eq(v, w)))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(p: &str) -> (String, String) {
        let x = filter_of(p.as_bytes());
        (
            String::from_utf8(x.prefix.into()).unwrap(),
            String::from_utf8(x.contains.into()).unwrap(),
        )
    }

    #[test]
    fn literal_requirements() {
        assert_eq!(f("^zealot's oath$"), ("zealot's oath".into(), "zealot's oath".into()));
        assert_eq!(
            f("(%d+)%% increased bear trap throwing speed"),
            ("".into(), "% increased bear trap throwing speed".into())
        );
        assert_eq!(f("^you have (%d+) additional"), ("you have ".into(), " additional".into()));
        assert_eq!(f("ab?c"), ("".into(), "a".into()));
        assert_eq!(f("xa+b"), ("".into(), "xa".into()));
        assert_eq!(f("^[%+%-]?(%d+) to"), ("".into(), " to".into()));
        assert_eq!(f("per (%d+)%% of"), ("".into(), "per ".into()));
        assert_eq!(f("(%d+)%% of"), ("".into(), "% of".into()));
        assert_eq!(f("a%bxy"), ("".into(), "".into()));
        assert_eq!(f("^%-(%d+)"), ("-".into(), "-".into()));
        assert_eq!(f("a$b"), ("".into(), "a$b".into()));
    }
}
