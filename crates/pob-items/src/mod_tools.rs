//! Modules/ModTools.lua:135-245 -- `formatFlags`, `formatValue`, `formatModParams`, `formatMod`, `formatSourceMod`,
//! `setSource` (pob-ts `src/mods/format.ts`). `formatTag(s)` is `pob_mods::format_tags` (R2, corpus-checked).
//!
//! TODO(R3.7): перенести в pob-mods -- these belong to the mod core next to `format_tags`; they live here only because R3.4
//! must not change `pob-mods`.

use pob_mods::{Mod, Table, Value, format_tags};

/// ModTools.lua:136-149 `modLib.formatFlags(flags, src)`: every name whose bits are all set (`band(flags, val) == val`,
/// LuaJIT's signed 32-bit `band`), sorted, joined by ","; none -> "-".
pub fn format_flags(flags: u32, src: &indexmap::IndexMap<String, f64>) -> String {
    let mut names: Vec<&str> = src
        .iter()
        .filter(|(_, val)| pob_lua::number::band(f64::from(flags), **val) == **val)
        .map(|(n, _)| n.as_str())
        .collect();
    names.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    if names.is_empty() { "-".to_owned() } else { names.join(",") }
}

fn tostring_value(v: &Value) -> String {
    match v {
        Value::Nil => "nil".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) => pob_lua::tostring_num(*n),
        Value::Str(s) => s.to_string(),
        Value::Table(_) | Value::Mod(_) => unreachable!("tables are formatted as tables"),
    }
}

fn format_table(t: &Table) -> String {
    // `pairs(value)` + `t_sort(paramNames)`, `type` first. Number keys (the array part) and string keys together make
    // `t_sort` compare a number with a string -- a Lua error.
    let has_named = t.hash.keys().any(|k| &**k != "type");
    if !t.array.is_empty() && has_named {
        panic!("Modules/ModTools.lua:204: attempt to compare number with string");
    }
    let mut parts: Vec<String> = Vec::new();
    if t.hash.contains_key("type") {
        parts.push(format!("type={}", format_value(t.get("type"))));
    }
    for (i, v) in t.array.iter().enumerate() {
        parts.push(format!("{}={}", i + 1, format_value(v)));
    }
    let mut names: Vec<&str> = t.hash.keys().map(|k| &**k).filter(|k| *k != "type").collect();
    names.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    for name in names {
        let v = t.get(name);
        if name == "mod" {
            match v {
                Value::Mod(m) => parts.push(format!("mod=[{}]", format_mod(m))),
                other => panic!("Modules/ModTools.lua:218: formatMod of a {} (attempt to index)", other.type_name()),
            }
        } else {
            parts.push(format!("{name}={}", format_value(v)));
        }
    }
    format!("{{{}}}", parts.join("/"))
}

/// ModTools.lua:194-225 `modLib.formatValue`.
pub fn format_value(v: &Value) -> String {
    match v {
        Value::Table(t) => format_table(t),
        Value::Mod(_) => unimplemented!("Modules/ModTools.lua:194 formatValue of a bare mod table (no such value in the data)"),
        other => tostring_value(other),
    }
}

/// ModTools.lua:227-229 `modLib.formatModParams`: a nil type prints "nil" (`%s` of nil).
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

/// ModTools.lua:231-233 `modLib.formatMod`.
pub fn format_mod(m: &Mod) -> String {
    format!("{} = {}", format_value(&m.value), format_mod_params(m))
}

/// ModTools.lua:235-237 `modLib.formatSourceMod`: a missing source prints "nil".
pub fn format_source_mod(m: &Mod) -> String {
    format!(
        "{}|{}|{}",
        format_value(&m.value),
        m.source.as_deref().unwrap_or("nil"),
        format_mod_params(m)
    )
}

/// ModTools.lua:239-245 `modLib.setSource`: the mod and, for `{ mod = ... }` values, the nested mod.
pub fn set_source(m: &mut Mod, source: &str) {
    m.source = Some(source.into());
    if let Value::Table(t) = &mut m.value
        && let Value::Mod(_) = t.get("mod")
    {
        let t = std::rc::Rc::make_mut(t);
        if let Some(Value::Mod(inner)) = t.hash.get_mut("mod") {
            std::rc::Rc::make_mut(inner).source = Some(source.into());
        }
    }
}
