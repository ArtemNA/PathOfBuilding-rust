//! What every converter shares: the dump, the drift report and the plain Lua-value -> JSON mapping.
//!
//! Policy (the point of this tool): a converter names the fields pob-rust reads, in the order the file has them. A field
//! upstream adds is DROPPED and REPORTED (drift), never fatal; a field the port reads that is missing or has another type
//! is fatal. Counts are reported, never pinned.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::dump::{Dump, V};
use crate::js::J;

pub struct Ctx<'a> {
    pub d: &'a Dump,
    pub pob_root: std::path::PathBuf,
    /// (file, where, what) -> count
    pub drift: BTreeMap<(String, String, String), usize>,
    pub counts: Vec<String>,
}

pub type R<T> = Result<T, String>;

impl<'a> Ctx<'a> {
    pub fn new(d: &'a Dump, pob_root: std::path::PathBuf) -> Self {
        Ctx { d, pob_root, drift: BTreeMap::new(), counts: Vec::new() }
    }

    pub fn drift(&mut self, file: &str, at: &str, what: String) {
        *self.drift.entry((file.into(), at.into(), what)).or_default() += 1;
    }

    pub fn count(&mut self, file: &str, what: &str, n: usize) {
        self.counts.push(format!("{file}: {what} = {n}"));
    }

    pub fn report(&self) -> String {
        let mut s = String::new();
        for line in &self.counts {
            let _ = writeln!(s, "count  {line}");
        }
        for ((file, at, what), n) in &self.drift {
            let _ = writeln!(s, "drift  {file}: {at}: {what} (x{n})");
        }
        s
    }

    pub fn table(&self, v: &V, at: &str) -> R<u32> {
        v.as_table().ok_or_else(|| format!("{at}: expected a table, got {}", self.show(v)))
    }

    /// A table's string-keyed entries in dump order (byte order); any other key is fatal.
    pub fn str_entries(&self, v: &V, at: &str) -> R<Vec<(&'a str, &'a V)>> {
        let t = self.d.t(self.table(v, at)?);
        t.entries
            .iter()
            .map(|(k, v)| match k {
                V::Str(s) => Ok((&**s, v)),
                k => Err(format!("{at}: expected string keys, got {}", self.show(k))),
            })
            .collect()
    }

    /// A Lua sequence (keys exactly 1..n; an empty table is one).
    pub fn seq(&self, v: &V, at: &str) -> R<Vec<&'a V>> {
        let t = self.d.t(self.table(v, at)?);
        if !t.is_sequence() {
            return Err(format!("{at}: expected a sequence 1..n"));
        }
        Ok(t.entries.iter().map(|(_, v)| v).collect())
    }

    pub fn string(&self, v: &V, at: &str) -> R<String> {
        v.as_str().map(str::to_owned).ok_or_else(|| format!("{at}: expected a string, got {}", self.show(v)))
    }

    pub fn show(&self, v: &V) -> String {
        match v {
            V::Table(t) => format!("table({} entries)", self.d.t(*t).entries.len()),
            V::Func(_) => "function".into(),
            v => format!("{v:?}"),
        }
    }

    /// The wire mapping every old file was built on: a sequence -> array (empty table -> `[]`), anything else -> object
    /// with keys as text (LuaJIT `tostring` of a number key), sorted numbers, strings, booleans.
    pub fn raw(&self, v: &V, at: &str) -> R<J> {
        Ok(match v {
            V::Nil => J::Null,
            V::Bool(b) => J::Bool(*b),
            V::Num(n) => J::Num(*n),
            V::Str(s) => J::str(s),
            V::Table(t) => {
                let t = self.d.t(*t);
                if t.is_sequence() {
                    J::Arr(t.entries.iter().map(|(_, v)| self.raw(v, at)).collect::<R<_>>()?)
                } else {
                    J::Obj(t.entries.iter().map(|(k, v)| Ok((key_text(k, at)?, self.raw(v, at)?))).collect::<R<_>>()?)
                }
            }
            V::Func(_) | V::Opaque(_) => return Err(format!("{at}: unexpected {}", self.show(v))),
        })
    }

}

/// LuaJIT `tostring` of a table key.
pub fn key_text(k: &V, at: &str) -> R<String> {
    Ok(match k {
        V::Str(s) => s.to_string(),
        V::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
        V::Num(n) => fmt_g14(*n),
        V::Bool(b) => b.to_string(),
        k => return Err(format!("{at}: unsupported key {k:?}")),
    })
}

/// C `%.14g`.
pub fn fmt_g14(x: f64) -> String {
    if !x.is_finite() {
        return if x.is_nan() { "nan".into() } else if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let e = format!("{:.13e}", x);
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if !(-4..14).contains(&exp) {
        let m = mant.trim_end_matches('0').trim_end_matches('.');
        return format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    let s = format!("{:.*}", (13 - exp).max(0) as usize, x);
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

/// Object builder in the file's field order.
#[derive(Default)]
pub struct Obj(pub Vec<(String, J)>);

impl Obj {
    pub fn put(&mut self, k: &str, v: J) {
        self.0.push((k.to_owned(), v));
    }
    pub fn opt(&mut self, k: &str, v: Option<J>) {
        if let Some(v) = v {
            self.put(k, v);
        }
    }
    pub fn done(self) -> J {
        J::Obj(self.0)
    }
}
