//! Modules/ModTools.lua:136-245 -- the textual rendering of a mod (`formatFlags`, `formatValue`, `formatModParams`,
//! `formatMod`, `formatSourceMod`) and `setSource`. `formatMod` builds `node.modKey` in `ProcessStats`; `formatSourceMod`
//! is the string every oracle probe dumps; `formatTag(s)` is [`crate::format_tags`] (R2, corpus-checked).
//! The one implementation (R3.8b): pob-tree, pob-items and pob-build re-export or call this module.
//! Numbers go through `pob_lua::tostring_num` (`%.14g`, MOD-CORE §5, MINEFIELD O10).

use std::rc::Rc;
use std::sync::OnceLock;

use indexmap::IndexMap;
use pob_lua::number::band;
use pob_lua::tostring_num;

use crate::matching::format_tags;
use crate::types::Mod;
use crate::value::{Table, Value};

/// `ModFlag` and `KeywordFlag` (Data/Global.lua via `data/globals.json`) -- the `src` tables `formatModParams` passes.
struct FlagTables {
    mod_flag: IndexMap<String, f64>,
    keyword_flag: IndexMap<String, f64>,
}

fn flag_tables() -> &'static FlagTables {
    static T: OnceLock<FlagTables> = OnceLock::new();
    T.get_or_init(|| {
        let path = crate::data::data_dir().join("globals.json");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("pob-mods data: {}: {e}", path.display()));
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("pob-mods data: {}: {e}", path.display()));
        let table = |key: &str| -> IndexMap<String, f64> {
            json[key]
                .as_object()
                .unwrap_or_else(|| panic!("pob-mods data: globals.json {key} is not a table"))
                .iter()
                .map(|(k, v)| {
                    let n = v.as_f64().unwrap_or_else(|| panic!("pob-mods data: {key}.{k} is not a number"));
                    (k.clone(), n)
                })
                .collect()
        };
        FlagTables {
            mod_flag: table("ModFlag"),
            keyword_flag: table("KeywordFlag"),
        }
    })
}

/// ModTools.lua:136-149 `modLib.formatFlags(flags, src)`: every name of `src` whose bits are all set in `flags`
/// (`band(flags, val) == val`, LuaJIT's signed 32-bit `band`, compared against the table value as Lua does), byte-sorted,
/// joined by ","; none prints "-".
pub fn format_flags(flags: u32, src: &IndexMap<String, f64>) -> String {
    let mut names: Vec<&str> = src
        .iter()
        .filter(|(_, v)| band(f64::from(flags), **v) == **v)
        .map(|(k, _)| k.as_str())
        .collect();
    names.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    if names.is_empty() { "-".to_owned() } else { names.join(",") }
}

/// LuaJIT `tostring` of a scalar value.
fn tostring_scalar(v: &Value) -> String {
    match v {
        Value::Nil => "nil".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) => tostring_num(*n),
        Value::Str(s) => s.to_string(),
        Value::Func(_) => crate::value::FUNCTION_TOSTRING.to_owned(),
        Value::Table(_) | Value::Mod(_) => unreachable!("tables are formatted by format_value"),
    }
}

/// `pairs(value)` + `t_sort(paramNames)` + `type` first (:196-225). `pairs` skips nil slots of the array part; number keys
/// and string keys together make `t_sort` compare a number with a string -- a Lua error.
fn format_table(t: &Table) -> String {
    let mut named: Vec<&str> = t.hash.keys().map(|k| &**k).filter(|k| *k != "type").collect();
    named.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    let numbered = t.array.iter().any(|x| !x.is_nil());
    if numbered && !named.is_empty() {
        panic!("Modules/ModTools.lua:209: attempt to compare number with string");
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

/// ModTools.lua:196-225 `modLib.formatValue`: a scalar through `tostring`; a table as `{type=.../k=v/...}`, `mod`
/// rendered as `mod=[formatMod]`.
pub fn format_value(value: &Value) -> String {
    match value {
        Value::Table(t) => format_table(t),
        Value::Mod(_) => unimplemented!("Modules/ModTools.lua:197 formatValue of a bare mod table (none in the data)"),
        other => tostring_scalar(other),
    }
}

/// ModTools.lua:227-229 `modLib.formatModParams`; a nil `type` prints "nil" (L24).
pub fn format_mod_params(m: &Mod) -> String {
    let f = flag_tables();
    format!(
        "{}|{}|{}|{}|{}",
        m.name,
        m.ty.map_or("nil", |t| t.as_str()),
        format_flags(m.flags, &f.mod_flag),
        format_flags(m.keyword_flags, &f.keyword_flag),
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

/// ModTools.lua:239-245 `modLib.setSource`: in place, on `value.mod` too (`type(mod.value) == "table" and
/// mod.value.mod`). Callers own the mod (a `parseMod` copy), so the copy-on-write below never actually copies.
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
