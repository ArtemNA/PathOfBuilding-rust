//! A record read field by field: what is taken is the schema, what is left over is drift (reported, dropped).

use crate::ctx::{Ctx, R};
use crate::dump::V;

pub struct Fields<'a> {
    at: String,
    entries: Vec<(&'a str, &'a V, bool)>,
}

impl<'a> Fields<'a> {
    pub fn new(c: &Ctx<'a>, v: &V, at: &str) -> R<Self> {
        Ok(Fields { at: at.to_owned(), entries: c.str_entries(v, at)?.into_iter().map(|(k, v)| (k, v, false)).collect() })
    }
    pub fn take(&mut self, k: &str) -> Option<&'a V> {
        let e = self.entries.iter_mut().find(|e| e.0 == k)?;
        e.2 = true;
        Some(e.1)
    }
    pub fn req(&mut self, k: &str) -> R<&'a V> {
        self.take(k).ok_or_else(|| format!("{}.{k}: missing", self.at))
    }
    pub fn string(&mut self, c: &Ctx, k: &str) -> R<String> {
        let v = self.req(k)?;
        c.string(v, &format!("{}.{k}", self.at))
    }
    pub fn opt_string(&mut self, c: &Ctx, k: &str) -> R<Option<String>> {
        self.take(k).map(|v| c.string(v, &format!("{}.{k}", self.at))).transpose()
    }
    pub fn num(&mut self, c: &Ctx, k: &str) -> R<f64> {
        let v = self.req(k)?;
        v.as_num().ok_or_else(|| format!("{}.{k}: expected a number, got {}", self.at, c.show(v)))
    }
    pub fn opt_num(&mut self, c: &Ctx, k: &str) -> R<Option<f64>> {
        match self.take(k) {
            None => Ok(None),
            Some(v) => v.as_num().map(Some).ok_or_else(|| format!("{}.{k}: expected a number, got {}", self.at, c.show(v))),
        }
    }
    pub fn opt_bool(&mut self, c: &Ctx, k: &str) -> R<Option<bool>> {
        match self.take(k) {
            None => Ok(None),
            Some(V::Bool(b)) => Ok(Some(*b)),
            Some(v) => Err(format!("{}.{k}: expected a boolean, got {}", self.at, c.show(v))),
        }
    }
    /// Report every field nobody took (`kind` groups them in the report, e.g. "item base").
    pub fn finish(self, c: &mut Ctx, file: &str, kind: &str) {
        for (k, v, used) in self.entries {
            if !used {
                let what = format!("unknown field '{k}' ({})", c.show(v).split('(').next().unwrap_or(""));
                c.drift(file, kind, what);
            }
        }
    }
}
