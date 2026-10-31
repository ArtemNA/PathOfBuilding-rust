//! ModTools.lua:135-245 -- the textual rendering of a mod (`formatFlags`, `formatValue`, `formatModParams`, `formatMod`,
//! `formatSourceMod`) and `setSource` (pob-ts `src/mods/format.ts`). `formatMod` builds `node.modKey` in `ProcessStats`;
//! `formatSourceMod` is the string every oracle probe dumps. `formatTag`/`formatTags` already live in `pob-mods`.
//!
//! TODO(R3.7): move this module into `pob-mods` (it is here only because R3.3 may not write `pob-mods`). Numbers go through
//! `pob_lua::tostring_num` (`%.14g`, MOD-CORE §5, MINEFIELD O10).

use std::rc::Rc;

use pob_data::wire::OrderedMap;
use pob_lua::number::band;
use pob_lua::tostring_num;
use pob_mods::{Mod, Value, format_tags};

/// ModTools.lua:135-148 `modLib.formatFlags`: every name of `src` whose bits are all set in `flags`, byte-sorted, joined by
/// ","; none prints "-". `band` is LuaJIT's signed 32-bit one, compared against the table value as Lua does.
pub fn format_flags(flags: u32, src: &OrderedMap<f64>) -> String {
    let mut names: Vec<&str> = src
        .iter()
        .filter(|(_, v)| band(f64::from(flags), **v) == **v)
        .map(|(k, _)| k.as_str())
        .collect();
    names.sort_unstable();
    if names.is_empty() { "-".to_owned() } else { names.join(",") }
}

/// LuaJIT `tostring` of a scalar value.
fn tostring_scalar(v: &Value) -> String {
    match v {
        Value::Nil => "nil".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) => tostring_num(*n),
        Value::Str(s) => s.to_string(),
        Value::Table(_) | Value::Mod(_) => unreachable!("tables are formatted by format_value"),
    }
}

/// ModTools.lua:194-225 `modLib.formatValue`: a scalar through `tostring`; a table as `{type=.../k=v/...}` -- `type` first,
/// the other keys sorted (`pairs` + `t_sort`; an array part reports number keys 1..n), `mod` rendered as `mod=[formatMod]`.
pub fn format_value(value: &Value) -> String {
    let t = match value {
        Value::Table(t) => t,
        Value::Mod(_) => unimplemented!("Modules/ModTools.lua:195 formatValue of a bare mod table (none in the data)"),
        other => return tostring_scalar(other),
    };
    let named: Vec<&str> = {
        let mut v: Vec<&str> = t.hash.keys().map(|k| &**k).filter(|k| *k != "type").collect();
        v.sort_unstable();
        v
    };
    let numbered = t.array.iter().any(|x| !x.is_nil());
    if numbered && !named.is_empty() {
        // `t_sort` over number and string keys together.
        panic!("Modules/ModTools.lua:206: attempt to compare number with string");
    }
    let mut parts: Vec<String> = Vec::new();
    if t.hash.contains_key("type") {
        parts.push(format!("type={}", format_value(t.get("type"))));
    }
    for (i, v) in t.array.iter().enumerate() {
        if !v.is_nil() {
            parts.push(format!("{}={}", i + 1, format_value(v)));
        }
    }
    for name in named {
        let v = t.get(name);
        if name == "mod" {
            match v {
                Value::Mod(m) => parts.push(format!("mod=[{}]", format_mod(m))),
                _ => unimplemented!("Modules/ModTools.lua:218 formatMod of a non-mod `value.mod` (none in the data)"),
            }
        } else {
            parts.push(format!("{name}={}", format_value(v)));
        }
    }
    format!("{{{}}}", parts.join("/"))
}

/// ModTools.lua:227-229 `modLib.formatModParams`; a nil `type` prints "nil" (L24).
pub fn format_mod_params(m: &Mod) -> String {
    let g = pob_data::globals::globals();
    format!(
        "{}|{}|{}|{}|{}",
        m.name,
        m.ty.map_or("nil", |t| t.as_str()),
        format_flags(m.flags, &g.mod_flag),
        format_flags(m.keyword_flags, &g.keyword_flag),
        format_tags(&m.tags)
    )
}

/// ModTools.lua:231-233 `modLib.formatMod` -- one `[...]` of `node.modKey`.
pub fn format_mod(m: &Mod) -> String {
    format!("{} = {}", format_value(&m.value), format_mod_params(m))
}

/// ModTools.lua:235-237 `modLib.formatSourceMod`; a missing source prints "nil".
pub fn format_source_mod(m: &Mod) -> String {
    format!(
        "{}|{}|{}",
        format_value(&m.value),
        m.source.as_deref().unwrap_or("nil"),
        format_mod_params(m)
    )
}

/// ModTools.lua:239-245 `modLib.setSource`: in place, on `value.mod` too. Callers own the mod (a `parseMod` copy), so the
/// copy-on-write below never actually copies.
pub fn set_source(m: &mut Mod, source: &str) {
    let src: Rc<str> = Rc::from(source);
    m.source = Some(src.clone());
    if let Value::Table(t) = &mut m.value
        && t.get("mod").truthy()
    {
        let t = Rc::make_mut(t);
        match t.hash.get_mut("mod") {
            Some(Value::Mod(inner)) => Rc::make_mut(inner).source = Some(src),
            Some(v @ Value::Table(_)) => v.set("source", Value::Str(src)),
            _ => unimplemented!("Modules/ModTools.lua:242 `value.mod.source = ...` on a non-table `value.mod`"),
        }
    }
}
