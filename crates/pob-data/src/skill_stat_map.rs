//! `data.skillStatMap` (Data/SkillStatMap.lua via `data/skill-stat-map.json`, 714 stats): the global stat -> mod mappings a granted
//! effect's `statMap` falls back to through `skillStatMapMeta.__index` (Modules/Data.lua:1033-1045, lazy copy + `processMod` -- see
//! [`crate::skills`]). The entries here are the RAW ones (no `source`, no `processMod`): the converter snapshotted the table right after
//! Data.lua, before any lazy copy (MINEFIELD D2).
//!
//! The shapes shared with `skills.json` (`statMap[stat]`, `baseMods`, ...) live here too: a stat-map entry is a list of mods or mod
//! GROUPS (`{ mod(...), mod(...), div = 60 }` -- several mods sharing one scalar, Data.lua:1054-1063/1069-1079) plus the entry's own
//! scalars. Upstream call-site bugs the converter kept lossless (`malformed`, MINEFIELD L24) are [`SkillMod::Malformed`]: what LuaJIT
//! does with them is modelled, not dropped.

use std::cell::OnceCell;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_mods::{Mod, ModType, Str, Table, Tag, Value, create_mod};
use serde_json::Value as Json;

use crate::wire::{ModRecord, json_num, json_to_mod, json_to_value};

/// The `flags` slot of a malformed mod as LuaJIT holds it.
#[derive(Clone, Debug)]
pub enum MalformedFlags {
    Num(u32),
    /// A tag table passed in the flags position (`mod(name, type, value, { type = "Condition", ... })`).
    Table(Table),
}

/// A mod whose Lua table breaks `makeSkillMod`'s contract (MINEFIELD L24) -- kept exactly as LuaJIT holds it:
/// * `flags` is a table (SkillStatMap `cannot_be_stunned_while_leeching`, `SupportBloodsoakedBanner.baseMods`): `formatSourceMod` raises in
///   `formatFlags` (`band(table, n)`), and so does every ModStore query that reaches the mod (`band(cfg.flags, mod.flags)`);
/// * numbers in the tag array (`spell_damage_+%_per_10_int`: `[1] = 2, [2] = 0, [3] = PerStat`): `processMod` raises at its
///   `ipairs(mod)` tag loop (`tag.type` of a number) -- the lazy `statMap` copy of that stat is a Lua error -- and so do
///   `formatSourceMod` (`formatTag`'s `pairs(tag)`) and EvalMod.
///
/// The error texts are LuaJIT's (fixture `gemdata-helpers.json` key `malformed`).
///
/// A mod with only `type = nil` is NOT malformed here: it is a plain [`Mod`] with `ty: None` (inert, as in LuaJIT).
#[derive(Clone, Debug)]
pub struct MalformedMod {
    pub name: Str,
    pub ty: Option<ModType>,
    pub value: Value,
    pub flags: MalformedFlags,
    pub keyword_flags: u32,
    pub source: Option<Str>,
    /// The Lua array part `mod[1..n]` (what `ipairs(mod)` walks): tag tables and the stray numbers.
    pub array: Vec<Value>,
}

impl MalformedMod {
    /// The LuaJIT error `modLib.formatSourceMod(mod)` raises (ModTools.lua: formatValue, then formatModParams -> formatFlags/formatTags).
    pub fn format_error(&self) -> String {
        match (&self.flags, self.array.iter().position(|v| !matches!(v, Value::Table(_)))) {
            (MalformedFlags::Table(_), _) => FORMAT_FLAGS_TABLE_ERROR.to_owned(),
            (MalformedFlags::Num(_), Some(_)) => FORMAT_TAGS_NUMBER_ERROR.to_owned(),
            (MalformedFlags::Num(_), None) => unreachable!("a malformed mod is malformed in flags or tags"),
        }
    }

    /// The LuaJIT error of a ModList query that reaches this mod (`ModList:Sum`/`List` with its name): a table in `flags` fails the
    /// flag test of `SumInternal` (ModList.lua:107 `band(flags, mod.flags)`), a number in the tag array fails EvalMod (ModStore.lua:369).
    /// The calc that adds such a mod to a mod list raises this when it queries the name (R6.4 `mergeSkillInstanceMods` and later).
    pub fn query_error(&self) -> String {
        match &self.flags {
            MalformedFlags::Table(_) => QUERY_FLAGS_TABLE_ERROR.to_owned(),
            MalformedFlags::Num(_) => QUERY_TAGS_NUMBER_ERROR.to_owned(),
        }
    }

    /// Does `processMod`'s `for _, tag in ipairs(mod) do if tag.type == ...` (Data.lua:74-79) raise on this mod?
    pub fn process_mod_error(&self) -> Option<String> {
        self.array
            .iter()
            .any(|v| !matches!(v, Value::Table(_)))
            .then(|| PROCESS_MOD_NUMBER_TAG_ERROR.to_owned())
    }
}

/// LuaJIT's text for `formatSourceMod` of a mod with a table in `flags` (fixture `gemdata-skills-*.json`, R6.2 probe).
pub const FORMAT_FLAGS_TABLE_ERROR: &str = "Modules/ModTools.lua:139: bad argument #1 to 'band' (number expected, got table)";
/// ... and of a mod with a number in its tag array.
pub const FORMAT_TAGS_NUMBER_ERROR: &str = "Modules/ModTools.lua:154: bad argument #1 to 'pairs' (table expected, got number)";
/// `ModList:Sum` on a mod with a table in `flags`.
pub const QUERY_FLAGS_TABLE_ERROR: &str = "Classes/ModList.lua:107: bad argument #2 to 'band' (number expected, got table)";
/// `ModList:Sum`/`List` (EvalMod) on a mod with a number in its tag array.
pub const QUERY_TAGS_NUMBER_ERROR: &str = "Classes/ModStore.lua:369: attempt to index local 'tag' (a number value)";
/// `processMod` (Data.lua:74-79) on a mod with a number in its tag array.
pub const PROCESS_MOD_NUMBER_TAG_ERROR: &str = "Modules/Data.lua:75: attempt to index local 'tag' (a number value)";

/// One mod of a skill mod list.
#[derive(Clone, Debug)]
pub enum SkillMod {
    Mod(Mod),
    Malformed(MalformedMod),
}

impl SkillMod {
    pub fn name(&self) -> &str {
        match self {
            SkillMod::Mod(m) => &m.name,
            SkillMod::Malformed(m) => &m.name,
        }
    }

    /// Lua `copyTable(mod)` (the lazy statMap copy, R4: new tag tables).
    pub fn deep_copy(&self) -> SkillMod {
        match self {
            SkillMod::Mod(m) => SkillMod::Mod(m.deep_copy()),
            SkillMod::Malformed(m) => SkillMod::Malformed(m.clone()),
        }
    }
}

/// One element of a mod list: a mod, or a group of mods sharing one scalar (`{ mod, mod, div = 60 }`).
#[derive(Clone, Debug)]
pub enum ModElem {
    Mod(SkillMod),
    Group {
        mods: Vec<SkillMod>,
        div: Option<f64>,
        mult: Option<f64>,
    },
}

impl ModElem {
    pub fn deep_copy(&self) -> ModElem {
        match self {
            ModElem::Mod(m) => ModElem::Mod(m.deep_copy()),
            ModElem::Group { mods, div, mult } => ModElem::Group {
                mods: mods.iter().map(SkillMod::deep_copy).collect(),
                div: *div,
                mult: *mult,
            },
        }
    }

    /// The mods of the element (`if modOrGroup.name then {modOrGroup} else ipairs(modOrGroup)`).
    pub fn mods_mut(&mut self) -> Vec<&mut SkillMod> {
        match self {
            ModElem::Mod(m) => vec![m],
            ModElem::Group { mods, .. } => mods.iter_mut().collect(),
        }
    }
}

/// `statMap[stat]` / `skillStatMap[stat]`: the mod list plus the entry's scalars.
#[derive(Clone, Debug, Default)]
pub struct StatMapEntry {
    pub mods: Vec<ModElem>,
    pub div: Option<f64>,
    pub mult: Option<f64>,
    pub base: Option<f64>,
    pub value: Option<f64>,
    pub skill_flag: Option<String>,
}

impl StatMapEntry {
    /// Lua `copyTable(map)` (Data.lua:1037).
    pub fn deep_copy(&self) -> StatMapEntry {
        StatMapEntry {
            mods: self.mods.iter().map(ModElem::deep_copy).collect(),
            ..self.clone()
        }
    }
}

fn bad(what: &str, j: &Json) -> String {
    let s = j.to_string();
    format!("{what}: unexpected {}", &s[..s.len().min(200)])
}

fn flags_u32(j: Option<&Json>, what: &str) -> Result<u32, String> {
    let Some(j) = j else { return Ok(0) };
    let f = json_num(j).ok_or_else(|| bad(what, j))?;
    if f >= 0.0 && f.fract() == 0.0 && f < 4_294_967_296.0 {
        Ok(f as u32)
    } else {
        Err(format!("{what}: {f} is not a 32-bit flag set"))
    }
}

/// A converter mod record (`{name, type, value, flags, keywordFlags, source?, tags, malformed?}`) as LuaJIT holds it.
pub fn json_to_skill_mod(j: &Json) -> Result<SkillMod, String> {
    let Json::Object(m) = j else { return Err(bad("mod", j)) };
    for k in m.keys() {
        if !matches!(
            k.as_str(),
            "name" | "type" | "value" | "flags" | "keywordFlags" | "source" | "tags" | "malformed"
        ) {
            return Err(format!("mod: unknown field '{k}'"));
        }
    }
    let Some(Json::Array(malformed)) = m.get("malformed") else {
        return match json_to_mod(j)? {
            ModRecord::Mod(dm) if dm.orphan_tags.is_empty() => Ok(SkillMod::Mod(dm.m)),
            _ => Err(bad("mod with orphanTags", j)),
        };
    };
    let name = m.get("name").and_then(Json::as_str).ok_or_else(|| bad("mod.name", j))?;
    let ty = match m.get("type") {
        None | Some(Json::Null) => None,
        Some(Json::String(s)) => Some(ModType::parse(s).ok_or_else(|| format!("mod '{name}': unknown type '{s}'"))?),
        Some(t) => return Err(bad("mod.type", t)),
    };
    let value = m.get("value").map_or(Ok(Value::Nil), json_to_value)?;
    let source = m.get("source").and_then(Json::as_str).map(Str::from);
    let tags: Vec<Value> = match m.get("tags") {
        Some(Json::Array(a)) => a.iter().map(json_to_value).collect::<Result<_, _>>()?,
        _ => return Err(bad("mod.tags", j)),
    };
    let mut flags = MalformedFlags::Num(0);
    let mut stray: Vec<(usize, Value)> = Vec::new();
    for f in malformed {
        let field = f.get("field").and_then(Json::as_str).ok_or_else(|| bad("malformed.field", f))?;
        let v = f.get("value").unwrap_or(&Json::Null);
        match field {
            "type" => {} // `type = nil`: the mod stays a plain mod (below)
            "flags" => match json_to_value(v)? {
                Value::Table(t) => flags = MalformedFlags::Table(Rc::unwrap_or_clone(t)),
                _ => return Err(bad("malformed flags", v)),
            },
            _ => {
                let i: usize = field
                    .strip_prefix("tags[")
                    .and_then(|r| r.strip_suffix(']'))
                    .and_then(|r| r.parse().ok())
                    .ok_or_else(|| format!("mod '{name}': unknown malformed field '{field}'"))?;
                stray.push((i, json_to_value(v)?));
            }
        }
    }
    if matches!(flags, MalformedFlags::Num(_)) && stray.is_empty() {
        // only `type` was off: Lua holds a normal mod table with `type = nil`
        let tags = tags
            .into_iter()
            .map(|t| match t {
                Value::Table(t) => Ok(Tag::from_rc(t)),
                _ => Err(format!("mod '{name}': a non-table tag")),
            })
            .collect::<Result<_, String>>()?;
        let flags = flags_u32(m.get("flags"), "mod.flags")?;
        let kw = flags_u32(m.get("keywordFlags"), "mod.keywordFlags")?;
        return Ok(SkillMod::Mod(create_mod(name, ty, value, source.as_deref(), flags, kw, tags)));
    }
    if let MalformedFlags::Num(n) = &mut flags {
        *n = flags_u32(m.get("flags"), "mod.flags")?;
    }
    // Rebuild the Lua array part: the stray values at their 1-based slots, the real tags filling the rest in order.
    let n = tags.len() + stray.len();
    let mut array: Vec<Option<Value>> = vec![None; n];
    for (i, v) in stray {
        let slot = array
            .get_mut(i - 1)
            .ok_or_else(|| format!("mod '{name}': stray tag index {i} out of range"))?;
        *slot = Some(v);
    }
    let mut tags = tags.into_iter();
    for slot in array.iter_mut().filter(|s| s.is_none()) {
        *slot = tags.next();
    }
    Ok(SkillMod::Malformed(MalformedMod {
        name: Str::from(name),
        ty,
        value,
        flags,
        keyword_flags: flags_u32(m.get("keywordFlags"), "mod.keywordFlags")?,
        source,
        array: array.into_iter().map(|v| v.expect("filled")).collect(),
    }))
}

fn opt_num(m: &serde_json::Map<String, Json>, k: &str) -> Result<Option<f64>, String> {
    match m.get(k) {
        None => Ok(None),
        Some(j) => json_num(j).map(Some).ok_or_else(|| bad(k, j)),
    }
}

/// A mod list element: a mod record or a group `{mods, div?, mult?}`.
pub fn json_to_elem(j: &Json) -> Result<ModElem, String> {
    let Json::Object(m) = j else {
        return Err(bad("mod list element", j));
    };
    if m.contains_key("name") {
        return json_to_skill_mod(j).map(ModElem::Mod);
    }
    for k in m.keys() {
        if !matches!(k.as_str(), "mods" | "div" | "mult") {
            return Err(format!("mod group: unknown field '{k}'"));
        }
    }
    let Some(Json::Array(mods)) = m.get("mods") else {
        return Err(bad("mod group", j));
    };
    Ok(ModElem::Group {
        mods: mods.iter().map(json_to_skill_mod).collect::<Result<_, _>>()?,
        div: opt_num(m, "div")?,
        mult: opt_num(m, "mult")?,
    })
}

/// A whole mod list (`baseMods`, `levelMods`).
pub fn json_to_elems(j: &Json) -> Result<Vec<ModElem>, String> {
    match j {
        Json::Array(a) => a.iter().map(json_to_elem).collect(),
        _ => Err(bad("mod list", j)),
    }
}

/// A `statMap[stat]` entry `{mods, div?, mult?, base?, value?, skillFlag?}`.
pub fn json_to_entry(j: &Json) -> Result<StatMapEntry, String> {
    let Json::Object(m) = j else { return Err(bad("statMap entry", j)) };
    for k in m.keys() {
        if !matches!(k.as_str(), "mods" | "div" | "mult" | "base" | "value" | "skillFlag") {
            return Err(format!("statMap entry: unknown field '{k}'"));
        }
    }
    Ok(StatMapEntry {
        mods: json_to_elems(m.get("mods").ok_or_else(|| bad("statMap entry without mods", j))?)?,
        div: opt_num(m, "div")?,
        mult: opt_num(m, "mult")?,
        base: opt_num(m, "base")?,
        value: opt_num(m, "value")?,
        skill_flag: match m.get("skillFlag") {
            None => None,
            Some(Json::String(s)) => Some(s.clone()),
            Some(o) => return Err(bad("skillFlag", o)),
        },
    })
}

/// `data.skillStatMap`: stat -> raw entry, in file order.
#[derive(Debug)]
pub struct SkillStatMap {
    pub entries: IndexMap<String, StatMapEntry>,
}

fn build(raw: serde_json::Map<String, Json>) -> Result<SkillStatMap, String> {
    let mut entries = IndexMap::with_capacity(raw.len());
    for (k, v) in raw {
        let e = json_to_entry(&v).map_err(|e| format!("skill-stat-map '{k}': {e}"))?;
        entries.insert(k, e);
    }
    Ok(SkillStatMap { entries })
}

thread_local! {
    static SSM: OnceCell<Rc<SkillStatMap>> = const { OnceCell::new() };
}

/// The skill stat map of this thread (its mods hold `Rc`s; parsed on first use).
pub fn skill_stat_map() -> Rc<SkillStatMap> {
    SSM.with(|c| {
        c.get_or_init(|| {
            let raw: serde_json::Map<String, Json> = crate::load("skill-stat-map.json");
            Rc::new(build(raw).unwrap_or_else(|e| panic!("pob-data: {e}")))
        })
        .clone()
    })
}
