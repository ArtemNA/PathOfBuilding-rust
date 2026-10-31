//! How a mod is matched against a query (MOD-CORE §2.4, §4) and how two mods are compared for MergeMod (§5).

use pob_lua::tostring_num;

use crate::data;
use crate::types::{Mod, ModType, Tag};
use crate::value::{LuaResult, Table, Value, err_op};

/// Data/Global.lua:189-215 `MatchKeywordFlags`. The default is "any of the mod's flags"; a mod carrying MatchAll needs all
/// of them. Upstream's two-level cache is an optimisation only. Bits are compared as u32: every flag of the data (max
/// 0x40000000) fits in 31 bits, so `band`'s signed result and the u32 one compare the same.
pub fn match_keyword_flags(keyword_flags: u32, mod_keyword_flags: u32) -> bool {
    let match_all = data::rules().match_all;
    let mask = !match_all; // MatchAllMask = bnot(KeywordFlag.MatchAll)
    let mod_masked = mod_keyword_flags & mask;
    let keyword_masked = keyword_flags & mask;
    if mod_keyword_flags & match_all != 0 {
        keyword_masked & mod_masked == mod_masked
    } else {
        mod_masked == 0 || keyword_masked & mod_masked != 0
    }
}

/// What a query asks a mod for: `None` type = Tabulate without a type (`mod.type == modType or not modType`, ModDB.lua:286).
pub(crate) struct Query<'a> {
    pub ty: Option<ModType>,
    pub flags: u32,
    pub keyword_flags: u32,
    pub source: Option<&'a str>,
}

/// The `and`-chain of every `*Internal`, in upstream order -- type, flags, keywordFlags, source -- with the same short
/// circuit: a mod failing the type or flags never reaches the source test, so it cannot throw. `guard` is ModDB.SumInternal's
/// `mod.source and ...` (ModDB.lua:144, the only guarded one); unguarded, a mod without source under a source filter dies
/// with LuaJIT's `attempt to index field 'source' (a nil value)` at `site` (MOD-CORE §7.10).
pub(crate) fn mod_matches(m: &Mod, q: &Query<'_>, guard: bool, site: &str) -> LuaResult<bool> {
    if let Some(t) = q.ty
        && m.ty != Some(t)
    {
        return Ok(false);
    }
    if q.flags & m.flags != m.flags || !match_keyword_flags(q.keyword_flags, m.keyword_flags) {
        return Ok(false);
    }
    // `not source`: "" is TRUE in Lua, so an empty source filter is a real (never matching) filter.
    let Some(source) = q.source else { return Ok(true) };
    match &m.source {
        Some(s) => Ok(pob_lua::string::first_non_colon(s) == Some(source)),
        None if guard => Ok(false),
        None => Err(err_op(site, "index", Some("field 'source'"), &Value::Nil)),
    }
}

/// `data.highPrecisionMods[mod.name] and data.highPrecisionMods[mod.name][mod.type]` (t[nil] reads nil).
pub(crate) fn mod_high_precision(m: &Mod) -> Option<f64> {
    data::high_precision(&m.name, m.ty?.as_str())
}

/// `tostring(val)` of a scalar tag field.
fn tostring_scalar(v: &Value) -> String {
    match v {
        Value::Nil => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) => tostring_num(*n),
        Value::Str(s) => s.to_string(),
        Value::Func(_) => crate::value::FUNCTION_TOSTRING.to_owned(),
        Value::Table(_) | Value::Mod(_) => unreachable!("tables are formatted by format_table"),
    }
}

/// `pairs(tag)` + `t_sort(paramNames)` with `type` first (ModTools.lua:152-163): a Lua array part reports number keys
/// 1..n (sorted numerically), a map its string keys in byte order (MOD-CORE §7.15).
fn format_table(t: &Table) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !t.array.is_empty() {
        if !t.hash.is_empty() {
            unimplemented!("ModTools.lua:160 t_sort over mixed number/string keys (a Lua error; no tag in the data has one)");
        }
        for (i, v) in t.array.iter().enumerate() {
            parts.push(format!("{}={}", i + 1, format_val(v)));
        }
        return parts.join("/");
    }
    let mut names: Vec<&str> = t.hash.keys().map(|k| &**k).filter(|k| *k != "type").collect();
    names.sort_unstable();
    if t.hash.contains_key("type") {
        names.insert(0, "type");
    }
    for name in names {
        parts.push(format!("{name}={}", format_val(t.get(name))));
    }
    parts.join("/")
}

/// ModTools.lua:168-181: a table value -- a list of tables via formatTags, a list of scalars via `table.concat(val, ",")`,
/// a map recursively -- in braces; a scalar through `tostring` (numbers `%.14g`, L1).
fn format_val(v: &Value) -> String {
    let t = match v {
        Value::Table(t) => t,
        Value::Mod(_) => unimplemented!("formatTag of a mod-valued tag field (none in the data)"),
        _ => return tostring_scalar(v),
    };
    let inner = match t.array.first() {
        None | Some(Value::Nil) | Some(Value::Bool(false)) => format_table(t),
        Some(Value::Table(_)) => format_table_list(&t.array),
        Some(_) => t.array.iter().map(tostring_scalar).collect::<Vec<_>>().join(","),
    };
    format!("{{{inner}}}")
}

fn format_table_list(list: &[Value]) -> String {
    if list.is_empty() {
        return "-".into();
    }
    list.iter()
        .map(|v| match v {
            Value::Table(t) => format_table(t),
            other => unimplemented!("formatTags over a non-table element ({})", other.type_name()),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// ModTools.lua:151-187 `modLib.formatTag`.
pub fn format_tag(tag: &Tag) -> String {
    format_table(&tag.table())
}

/// ModTools.lua:188-194 `modLib.formatTags`: joined by ","; an empty list is "-".
pub fn format_tags(tags: &[Tag]) -> String {
    if tags.is_empty() {
        return "-".into();
    }
    tags.iter().map(format_tag).collect::<Vec<_>>().join(",")
}

/// ModTools.lua:121-134 `modLib.compareModParams`: name/type/flags/keywordFlags and the tags pairwise -- `source` is NOT
/// compared, so MergeMod keeps the OLD mod's source (§2.2).
pub fn compare_mod_params(a: &Mod, b: &Mod) -> bool {
    if a.name != b.name || a.ty != b.ty || a.flags != b.flags || a.keyword_flags != b.keyword_flags || a.tags.len() != b.tags.len() {
        return false;
    }
    a.tags
        .iter()
        .zip(&b.tags)
        .all(|(x, y)| x.ty().raw_eq(y.ty()) && format_tag(x) == format_tag(y))
}
