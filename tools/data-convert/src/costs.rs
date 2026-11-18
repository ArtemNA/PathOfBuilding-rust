//! `data.costs` (Data/Costs.lua; Modules/Data.lua adds a by-Resource metatable, rebuilt by the reader) -> costs.json:
//! `[{Resource, Stat, ResourceString, Divisor}]` in Lua order.

use std::collections::HashSet;

use crate::ctx::{Ctx, Obj, R};
use crate::fields::Fields;
use crate::js::J;

const FILE: &str = "costs.json";

pub fn convert(c: &mut Ctx) -> R<J> {
    let v = c.d.path("data", &["costs"]).ok_or("data.costs: missing")?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (i, e) in c.seq(v, "data.costs")?.into_iter().enumerate() {
        let at = format!("data.costs[{}]", i + 1);
        let mut f = Fields::new(c, e, &at)?;
        let mut o = Obj::default();
        let resource = f.string(c, "Resource")?;
        if !seen.insert(resource.clone()) {
            return Err(format!("{at}: duplicate Resource '{resource}'"));
        }
        o.put("Resource", J::Str(resource));
        o.put("Stat", J::Str(f.string(c, "Stat")?));
        o.put("ResourceString", J::Str(f.string(c, "ResourceString")?));
        o.put("Divisor", J::Num(f.num(c, "Divisor")?));
        f.finish(c, FILE, "cost");
        out.push(o.done());
    }
    c.count(FILE, "costs", out.len());
    Ok(J::Arr(out))
}
