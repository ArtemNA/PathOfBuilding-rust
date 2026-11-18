//! Data/ModCache.lua (loaded on its own, as it is on disk) -> mod-cache.json: `[{line, mods, extra}]` sorted by line bytes.
//! Reader: crates/pob-data/src/mod_cache.rs.

use crate::ctx::{Ctx, R};
use crate::dump::V;
use crate::js::J;
use crate::mods::normalize_mod_list;

const FILE: &str = "mod-cache.json";

pub fn convert(c: &mut Ctx) -> R<J> {
    let root = c.d.root("modCache").clone();
    let mut out = Vec::new();
    for (line, v) in c.str_entries(&root, "modCache")? {
        let at = format!("modCache[{line}]");
        let t = c.d.t(c.table(v, &at)?);
        let mut mods = J::Null;
        let mut extra = J::Null;
        for (k, x) in &t.entries {
            match k {
                V::Num(n) if *n == 1.0 => mods = normalize_mod_list(c, x, &at)?,
                V::Num(n) if *n == 2.0 => extra = J::Str(c.string(x, &at)?),
                k => return Err(format!("{at}: unexpected key {k:?}")),
            }
        }
        out.push((line, J::Obj(vec![("line".into(), J::str(line)), ("mods".into(), mods), ("extra".into(), extra)])));
    }
    out.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    c.count(FILE, "lines", out.len());
    Ok(J::Arr(out.into_iter().map(|(_, j)| j).collect()))
}
