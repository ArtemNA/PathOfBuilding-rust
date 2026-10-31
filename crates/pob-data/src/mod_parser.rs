//! `data/mod-parser.json` -- the 19 tables `Modules/ModParser.lua` builds at load time, its closure definitions and two
//! helper tables (pob-ts `tools/convert/extract/mod-parser.ts`, P5-ARCH §2.1-§2.2, R5-ARCH §1.1).
//!
//! * `tables`: pattern (or plain key) -> value. A value is a static Lua value (mods are `pob_mods::Mod`), a closure
//!   `{fn: "<src>:<from>", up: {...}}`, or a table holding closures (`jewelFuncList`: `{func: {fn, up}, type}`).
//!   Inside `up`: `{fn}` (a Lua function upvalue -- the named helpers), `{cfn: true}` (a C function: `bor`, `ipairs`, ...),
//!   `{named: "<table>"}` (one of the 19 tables or an `aux` table).
//! * `fns`: one record per closure DEFINITION (1 263) with the sha256 of its body (the drift contract, R5-ARCH §1.3).
//! * `aux`: `conquerorList`, `gemIdLookup` (module locals closures capture).
//!
//! Order (D13): every table keeps the JSON order -- the converter wrote the keys byte-sorted. Upstream walks these tables
//! with `pairs` (hash order, randomised per process by `LUAJIT_SECURITY_STRHASH`); `scan` (ModParser.lua:6616-6638) only
//! depends on that order when two patterns tie on (index, endIndex, #pattern) -- then the FIRST in this sorted order wins
//! here (pob-parser `scan`; the tie count on the corpus is in the R5.2 report).
//!
//! The wire reshaping of MINEFIELD D3 is undone: an object's `list` field is the Lua array part (`{ "Str", "Dex", tag = ... }`).
//! Mods hold `Rc`s, so the parsed data lives per thread (as `mod_cache`).

use std::cell::OnceCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use indexmap::IndexMap;
use pob_mods::{Str, Table, Value};
use serde::Deserialize;
use serde_json::Value as Json;

use crate::wire::{ModRecord, OrderedMap, OrphanTag, json_num, json_to_mod};

/// The 19 tables, in upstream declaration order (pob-ts `MOD_PARSER_TABLES`).
pub const TABLE_NAMES: [&str; 19] = [
    "formList",
    "modNameList",
    "modFlagList",
    "preFlagList",
    "modTagList",
    "specialModList",
    "jewelFuncList",
    "clusterJewelSkills",
    "unsupportedModList",
    "preSkillNameList",
    "skillNameList",
    "suffixTypes",
    "penTypes",
    "flagTypes",
    "dmgTypes",
    "regenTypes",
    "degenTypes",
    "costTypes",
    "baseCostTypes",
];

/// A reference to a closure definition plus its upvalues (`{fn, up}`).
#[derive(Debug)]
pub struct FnRef {
    /// `"<src>:<from>"`, the key of `fns`.
    pub id: Str,
    pub up: IndexMap<Str, PVal>,
}

/// A value of a ModParser table. Every closure-free subtree is a plain Lua `Value` (`Lua`), converted once at load.
#[derive(Clone, Debug)]
pub enum PVal {
    Lua(Value),
    /// A table some part of which is a closure (only `jewelFuncList` entries and `up` tables).
    Table {
        array: Vec<PVal>,
        hash: IndexMap<Str, PVal>,
    },
    Fn(Rc<FnRef>),
    /// A C function upvalue (`bor`, `ipairs`, `t_insert`, ...).
    CFn,
    /// One of the 19 tables or an `aux` table, captured as an upvalue.
    Named(Str),
}

impl PVal {
    /// The plain Lua value, if this holds no closure.
    pub fn as_lua(&self) -> Option<&Value> {
        if let PVal::Lua(v) = self { Some(v) } else { None }
    }

    pub fn as_fn(&self) -> Option<&Rc<FnRef>> {
        if let PVal::Fn(f) = self { Some(f) } else { None }
    }

    /// `t.key` of a table value (either form).
    pub fn field(&self, key: &str) -> Option<PVal> {
        match self {
            PVal::Lua(Value::Table(t)) => Some(PVal::Lua(t.get(key).clone())),
            PVal::Table { hash, .. } => hash.get(key).cloned(),
            _ => None,
        }
    }
}

/// One table: keys in file order (byte-sorted) and a key index.
#[derive(Debug, Default)]
pub struct PatternTable {
    pub keys: Vec<Str>,
    pub vals: Vec<PVal>,
    /// `(entry, index of the mod in the entry's list, tag)`: tags after a hole in a mod's array part (L9) -- the 2
    /// `^tornado shot fires ...` entries of `specialModList`. Never in `Mod::tags` (ipairs never reaches them).
    pub orphan_tags: Vec<(usize, usize, OrphanTag)>,
    index: HashMap<Str, usize>,
}

impl PatternTable {
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// `t[key]` for a string key.
    pub fn get(&self, key: &str) -> Option<&PVal> {
        self.index.get(key).map(|&i| &self.vals[i])
    }

    pub fn position(&self, key: &str) -> Option<usize> {
        self.index.get(key).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Str, &PVal)> {
        self.keys.iter().zip(&self.vals)
    }
}

/// One closure definition (`fns`).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FnDef {
    pub src: String,
    pub from: u32,
    pub to: u32,
    pub sha: String,
}

#[derive(Debug)]
pub struct ModParserData {
    /// The 19 tables by name (file order: alphabetical).
    pub tables: IndexMap<Str, PatternTable>,
    pub fns: IndexMap<Str, FnDef>,
    /// `conquerorList` (ModParser.lua:41-71): name -> `{ id, type }`.
    pub conqueror_list: PatternTable,
    /// `gemIdLookup` (ModParser.lua:2033-2046): lowercase skill name -> granted effect id.
    pub gem_id_lookup: IndexMap<Str, Str>,
    /// Closure values (`{fn, up}`) anywhere in `tables` -- 1 973 direct references (R5.1b).
    pub fn_refs: usize,
}

impl ModParserData {
    /// One of the 19 tables (panics on an unknown name: a programming error).
    pub fn table(&self, name: &str) -> &PatternTable {
        self.tables.get(name).unwrap_or_else(|| panic!("mod-parser: no table '{name}'"))
    }
}

fn bad(path: &str, j: &Json) -> String {
    let s = j.to_string();
    format!("mod-parser {path}: unexpected {}", &s[..s.len().min(160)])
}

/// The JSON of a mod `normalizeMod` wrote: the createMod fields (+ the L9/L24 lossless markers).
fn looks_like_mod(m: &serde_json::Map<String, Json>) -> bool {
    m.get("name").is_some_and(Json::is_string)
        && m.contains_key("type")
        && m.contains_key("flags")
        && m.contains_key("keywordFlags")
        && m.get("tags").is_some_and(Json::is_array)
        && m.keys().all(|k| {
            matches!(
                k.as_str(),
                "name" | "type" | "value" | "flags" | "keywordFlags" | "source" | "tags" | "orphanTags" | "malformed"
            )
        })
}

fn is_fn_ref(m: &serde_json::Map<String, Json>) -> bool {
    m.get("fn").is_some_and(Json::is_string) && m.keys().all(|k| k == "fn" || k == "up")
}

struct Conv<'a> {
    fn_refs: &'a mut usize,
    /// Orphan tags of the mods of the current entry: `(mod index in the entry's list, tag)`.
    orphans: Vec<(usize, OrphanTag)>,
}

impl Conv<'_> {
    fn val(&mut self, j: &Json, path: &str) -> Result<PVal, String> {
        Ok(match j {
            Json::Null => PVal::Lua(Value::Nil),
            Json::Bool(b) => PVal::Lua(Value::Bool(*b)),
            Json::Number(_) => PVal::Lua(Value::Num(json_num(j).ok_or_else(|| bad(path, j))?)),
            Json::String(s) => PVal::Lua(Value::str(s)),
            Json::Array(a) => {
                let mut items = Vec::with_capacity(a.len());
                for (i, x) in a.iter().enumerate() {
                    let v = self.val(x, &format!("{path}[{}]", i + 1))?;
                    if matches!(v, PVal::Lua(Value::Nil)) {
                        return Err(format!("mod-parser {path}: a null inside an array (a Lua hole)"));
                    }
                    items.push(v);
                }
                PVal::Table {
                    array: items,
                    hash: IndexMap::new(),
                }
                .simplify()
            }
            Json::Object(m) => {
                if let Some(n) = json_num(j) {
                    return Ok(PVal::Lua(Value::Num(n)));
                }
                if is_fn_ref(m) {
                    *self.fn_refs += usize::from(m.contains_key("up"));
                    let id = Str::from(m["fn"].as_str().unwrap_or_default());
                    let mut up = IndexMap::new();
                    if let Some(u) = m.get("up") {
                        let Json::Object(um) = u else {
                            if u.as_array().is_some_and(Vec::is_empty) {
                                return Ok(PVal::Fn(Rc::new(FnRef { id, up })));
                            }
                            return Err(bad(path, u));
                        };
                        for (k, x) in um {
                            up.insert(Str::from(k.as_str()), self.val(x, &format!("{path}.up.{k}"))?);
                        }
                    }
                    return Ok(PVal::Fn(Rc::new(FnRef { id, up })));
                }
                if m.len() == 1 && m.get("cfn") == Some(&Json::Bool(true)) {
                    return Ok(PVal::CFn);
                }
                if m.len() == 1
                    && let Some(Json::String(n)) = m.get("named")
                {
                    return Ok(PVal::Named(Str::from(n.as_str())));
                }
                if looks_like_mod(m) {
                    return match json_to_mod(j).map_err(|e| format!("mod-parser {path}: {e}"))? {
                        ModRecord::Mod(dm) => {
                            // the slot index is fixed up by the caller of a top-level mod list
                            self.orphans.extend(dm.orphan_tags.into_iter().map(|o| (usize::MAX, o)));
                            Ok(PVal::Lua(Value::Mod(Rc::new(dm.m))))
                        }
                        ModRecord::Malformed => Err(format!("mod-parser {path}: a malformed mod (L24)")),
                    };
                }
                let mut array = Vec::new();
                let mut hash = IndexMap::new();
                for (k, x) in m {
                    if k == "list"
                        && let Json::Array(a) = x
                    {
                        // D3: the converter's name for the Lua array part
                        for (i, y) in a.iter().enumerate() {
                            array.push(self.val(y, &format!("{path}.list[{}]", i + 1))?);
                        }
                        continue;
                    }
                    if k.parse::<i64>().is_ok() {
                        return Err(format!("mod-parser {path}: numeric key '{k}' outside a contiguous array part"));
                    }
                    if x.is_null() {
                        continue;
                    }
                    hash.insert(Str::from(k.as_str()), self.val(x, &format!("{path}.{k}"))?);
                }
                PVal::Table { array, hash }.simplify()
            }
        })
    }
}

impl PVal {
    /// A table with no closure inside becomes a plain Lua table.
    fn simplify(self) -> PVal {
        match self {
            PVal::Table { array, hash } => {
                if array.iter().all(|v| matches!(v, PVal::Lua(_))) && hash.values().all(|v| matches!(v, PVal::Lua(_))) {
                    let unwrap = |v: PVal| match v {
                        PVal::Lua(x) => x,
                        _ => unreachable!(),
                    };
                    PVal::Lua(Value::table(Table {
                        array: array.into_iter().map(unwrap).collect(),
                        hash: hash.into_iter().map(|(k, v)| (k, unwrap(v))).collect(),
                    }))
                } else {
                    PVal::Table { array, hash }
                }
            }
            other => other,
        }
    }
}

fn table_of(j: &Json, path: &str, fn_refs: &mut usize) -> Result<PatternTable, String> {
    let entries: Vec<(&String, &Json)> = match j {
        Json::Object(m) => m.iter().collect(),
        Json::Array(a) if a.is_empty() => Vec::new(),
        _ => return Err(bad(path, j)),
    };
    let mut t = PatternTable::default();
    for (k, x) in entries {
        let mut conv = Conv {
            fn_refs,
            orphans: Vec::new(),
        };
        let v = conv.val(x, &format!("{path}.{k}"))?;
        if !conv.orphans.is_empty() {
            // only a top-level list of mods may carry them: find the slot of each orphan's mod by re-reading the JSON
            let Json::Array(a) = x else {
                return Err(format!("mod-parser {path}.{k}: orphan tags outside a top-level mod list"));
            };
            let mut it = conv.orphans.into_iter();
            for (slot, mj) in a.iter().enumerate() {
                let n = mj.get("orphanTags").and_then(Json::as_array).map_or(0, Vec::len);
                for _ in 0..n {
                    let (_, o) = it.next().ok_or_else(|| format!("mod-parser {path}.{k}: orphan bookkeeping"))?;
                    t.orphan_tags.push((t.keys.len(), slot, o));
                }
            }
        }
        let key = Str::from(k.as_str());
        if t.index.insert(key.clone(), t.keys.len()).is_some() {
            return Err(format!("mod-parser {path}: duplicate key '{k}'"));
        }
        t.keys.push(key);
        t.vals.push(v);
    }
    Ok(t)
}

fn build(j: &Json) -> Result<ModParserData, String> {
    let root = j.as_object().ok_or_else(|| bad("root", j))?;
    if root.keys().map(String::as_str).collect::<Vec<_>>() != ["aux", "fns", "tables"] {
        return Err("mod-parser: expected exactly {aux, fns, tables}".to_owned());
    }
    let mut fn_refs = 0;
    let jt = root["tables"].as_object().ok_or_else(|| bad("tables", &root["tables"]))?;
    let mut tables = IndexMap::new();
    for (name, t) in jt {
        if !TABLE_NAMES.contains(&name.as_str()) {
            return Err(format!("mod-parser: unknown table '{name}'"));
        }
        tables.insert(Str::from(name.as_str()), table_of(t, &format!("tables.{name}"), &mut fn_refs)?);
    }
    if tables.len() != TABLE_NAMES.len() {
        return Err(format!("mod-parser: {} tables, expected {}", tables.len(), TABLE_NAMES.len()));
    }
    let fns: OrderedMap<FnDef> = serde_json::from_value(root["fns"].clone()).map_err(|e| format!("mod-parser fns: {e}"))?;
    let fns = fns.0.into_iter().map(|(k, v)| (Str::from(k.as_str()), v)).collect();
    let aux = root["aux"].as_object().ok_or_else(|| bad("aux", &root["aux"]))?;
    let mut dummy = 0;
    let conqueror_list = table_of(aux.get("conquerorList").unwrap_or(&Json::Null), "aux.conquerorList", &mut dummy)?;
    let gem_id_lookup = aux
        .get("gemIdLookup")
        .and_then(Json::as_object)
        .ok_or_else(|| "mod-parser: aux.gemIdLookup missing".to_owned())?
        .iter()
        .map(|(k, v)| {
            v.as_str()
                .map(|s| (Str::from(k.as_str()), Str::from(s)))
                .ok_or_else(|| bad("aux.gemIdLookup", v))
        })
        .collect::<Result<_, _>>()?;
    Ok(ModParserData {
        tables,
        fns,
        conqueror_list,
        gem_id_lookup,
        fn_refs,
    })
}

thread_local! {
    static DATA: OnceCell<Rc<ModParserData>> = const { OnceCell::new() };
}

/// The ModParser tables of this thread (parsed on first use).
pub fn mod_parser() -> Rc<ModParserData> {
    DATA.with(|c| {
        c.get_or_init(|| {
            let j: Json = crate::load("mod-parser.json");
            Rc::new(build(&j).unwrap_or_else(|e| panic!("pob-data: {e}")))
        })
        .clone()
    })
}

/// The two fields of `data.skills[id]` the ModParser helpers read (`extraSupport`, ModParser.lua:2073-2106):
/// `name` and `support` (of a gem's `secondaryGrantedEffect`).
#[derive(Clone, Debug, Deserialize)]
pub struct SkillBrief {
    pub name: Option<String>,
    pub support: Option<bool>,
}

/// `data.skills` reduced to `SkillBrief` (from `data/skills.json`, read once per process).
pub fn skill_briefs() -> &'static OrderedMap<SkillBrief> {
    static S: OnceLock<OrderedMap<SkillBrief>> = OnceLock::new();
    S.get_or_init(|| crate::load("skills.json"))
}
