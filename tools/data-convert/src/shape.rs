//! Declarative record shapes: the old files' zod schemas as data. An object is written in its schema's field order (what the
//! old pipeline's schema pass produced), a field it does not name is drift (reported, dropped), a named field that is
//! missing (when required) or has another type is fatal with its path.

use crate::ctx::{Ctx, R, key_text};
use crate::dump::V;
use crate::fields::Fields;
use crate::js::J;
use crate::mods::normalize_mod;

pub type Custom = fn(&mut Ctx, &V, &str) -> R<J>;

pub enum S {
    Num,
    Str,
    Bool,
    /// The literal `true`.
    True,
    /// A Lua sequence (empty table -> `[]`).
    Arr(&'static S),
    /// A map with any keys (text as LuaJIT `tostring`), values of one shape; an empty table -> `{}`.
    Rec(&'static S),
    /// A fixed-length sequence.
    Tuple(&'static [S]),
    /// A record: (drift group, fields in output order).
    Obj(&'static str, &'static [F]),
    /// A validated top-level mod (mods.rs).
    Mod,
    Custom(Custom),
}

pub struct F {
    pub k: &'static str,
    pub req: bool,
    pub s: S,
}

pub const fn req(k: &'static str, s: S) -> F {
    F { k, req: true, s }
}
pub const fn opt(k: &'static str, s: S) -> F {
    F { k, req: false, s }
}

pub struct Shaper<'f> {
    pub file: &'f str,
}

impl Shaper<'_> {
    pub fn conv(&self, c: &mut Ctx, s: &S, v: &V, at: &str) -> R<J> {
        let bad = |c: &Ctx, what: &str| Err(format!("{at}: expected {what}, got {}", c.show(v)));
        match s {
            S::Num => match v {
                V::Num(n) if n.is_finite() => Ok(J::Num(*n)),
                _ => bad(c, "a finite number"),
            },
            S::Str => match v {
                V::Str(s) => Ok(J::str(s)),
                _ => bad(c, "a string"),
            },
            S::Bool => match v {
                V::Bool(b) => Ok(J::Bool(*b)),
                _ => bad(c, "a boolean"),
            },
            S::True => match v {
                V::Bool(true) => Ok(J::Bool(true)),
                _ => bad(c, "true"),
            },
            S::Arr(e) => {
                let items = c.seq(v, at)?;
                Ok(J::Arr(items.into_iter().enumerate().map(|(i, x)| self.conv(c, e, x, &format!("{at}[{}]", i + 1))).collect::<R<_>>()?))
            }
            S::Rec(e) => {
                let t = c.table(v, at)?;
                let mut o = Vec::new();
                for (k, x) in &c.d.t(t).entries {
                    let k = key_text(k, at)?;
                    let j = self.conv(c, e, x, &format!("{at}.{k}"))?;
                    o.push((k, j));
                }
                Ok(J::Obj(o))
            }
            S::Tuple(es) => {
                let items = c.seq(v, at)?;
                if items.len() != es.len() {
                    return Err(format!("{at}: expected {} elements, got {}", es.len(), items.len()));
                }
                Ok(J::Arr(items.into_iter().zip(es.iter()).enumerate().map(|(i, (x, e))| self.conv(c, e, x, &format!("{at}[{}]", i + 1))).collect::<R<_>>()?))
            }
            S::Obj(kind, fs) => {
                let mut f = Fields::new(c, v, at)?;
                let o = self.fields(c, &mut f, fs, at)?;
                f.finish(c, self.file, kind);
                Ok(J::Obj(o))
            }
            S::Mod => normalize_mod(c, v, at, true),
            S::Custom(g) => g(c, v, at),
        }
    }

    /// The named fields of `fs` taken from `f`, in `fs` order (the caller finishes `f`).
    pub fn fields(&self, c: &mut Ctx, f: &mut Fields, fs: &[F], at: &str) -> R<Vec<(String, J)>> {
        let mut o = Vec::new();
        for fd in fs {
            match f.take(fd.k) {
                Some(x) => o.push((fd.k.to_owned(), self.conv(c, &fd.s, x, &format!("{at}.{}", fd.k))?)),
                None if fd.req => return Err(format!("{at}.{}: missing", fd.k)),
                None => {}
            }
        }
        Ok(o)
    }
}
