//! Data/Global.lua enums and colours after the full boot (ModFlag, KeywordFlag, SkillType, colorCodes,
//! defaultColorCodes) -> globals.json: five open records in Lua key order.

use crate::ctx::{Ctx, Obj, R, key_text};
use crate::dump::V;
use crate::js::J;

const FILE: &str = "globals.json";

fn record(c: &mut Ctx, k: &str, ints: bool) -> R<J> {
    let at = format!("after.{k}");
    let v = c.d.path("after", &[k]).ok_or_else(|| format!("{at}: missing"))?;
    let t = c.d.t(c.table(v, &at)?);
    let mut o = Vec::new();
    for (key, x) in &t.entries {
        let ks = key_text(key, &at)?;
        let j = match x {
            V::Num(n) if ints && n.fract() == 0.0 => J::Num(*n),
            V::Str(s) if !ints => J::str(s),
            x => return Err(format!("{at}.{ks}: expected {}, got {}", if ints { "an integer" } else { "a string" }, c.show(x))),
        };
        o.push((ks, j));
    }
    c.count(FILE, k, o.len());
    Ok(J::Obj(o))
}

pub fn convert(c: &mut Ctx) -> R<J> {
    let mut o = Obj::default();
    for k in ["ModFlag", "KeywordFlag", "SkillType"] {
        o.put(k, record(c, k, true)?);
    }
    let colors = record(c, "colorCodes", false)?;
    let defaults = record(c, "defaultColorCodes", false)?;
    // The boot must not have changed the colours (a user setting would): the file stands for upstream's defaults.
    if crate::js::stringify(&colors) != crate::js::stringify(&defaults) {
        return Err("after.defaultColorCodes: must equal colorCodes (no runtime modifications)".into());
    }
    o.put("colorCodes", colors);
    o.put("defaultColorCodes", defaults);
    Ok(o.done())
}
