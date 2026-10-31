//! The inner `parseMod(line, order)` -- ModParser.lua:6640-6992, line by line (comments carry the Lua lines).
//!
//! Values travel as Lua values (`pob_mods::Value`); the line as bytes (`string.sub` positions are bytes, L16). A closure
//! that is not ported yet stops the parse with `ParseError::Unported(id)` -- the result of such a line is unknown, never a
//! guess.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use misc::Misc;
use pob_data::mod_parser::{ModParserData, PVal, PatternTable};
use pob_data::wire::OrphanTag;
use pob_lua::number::bor;
use pob_mods::{LuaError, Mod, ModType, Str, Table, Tag, Value, create_mod};

use crate::closures::{call_special, call_value};
use crate::helpers::{concat, create_mod_lua, flag_u32, index, mk, tbl, tonumber};
use crate::scan::{Hit, ScanTable, scan};
use crate::{ParseError, copy_table_mod};

/// `misc` (ModParser.lua:6882): the merged fields of the tag sources, later writers win.
mod misc {
    use pob_mods::{Str, Value};

    #[derive(Default)]
    pub struct Misc(pub Vec<(Str, Value)>);

    impl Misc {
        pub fn set(&mut self, k: &Str, v: Value) {
            match self.0.iter_mut().find(|(x, _)| x == k) {
                Some(slot) => slot.1 = v,
                None => self.0.push((k.clone(), v)),
            }
        }

        pub fn get(&self, k: &str) -> Value {
            self.0.iter().find(|(x, _)| &**x == k).map_or(Value::Nil, |(_, v)| v.clone())
        }
    }
}

/// What the inner `parseMod` returned: `modList, extra` (+ the L9 orphan tags of a static `specialModList` entry, by mod
/// index -- they sit beside the mods exactly as in `mod_cache`).
#[derive(Debug, Default)]
pub struct RawParse {
    pub mod_list: Option<Vec<Mod>>,
    pub extra: Option<String>,
    pub orphan_tags: Vec<(usize, OrphanTag)>,
}

/// `scan_val`: the value (nil when none), the captures and the matched entry.
type Scanned = (Option<PVal>, Vec<Value>, Option<usize>);

const T_FORM: usize = 0;
const T_NAME: usize = 1;
const T_FLAG: usize = 2;
const T_PRE_FLAG: usize = 3;
const T_TAG: usize = 4;
const T_SPECIAL: usize = 5;
const T_JEWEL: usize = 6;
const T_CLUSTER: usize = 7;
const T_UNSUPPORTED: usize = 8;
const T_PRE_SKILL: usize = 9;
const T_SKILL: usize = 10;
const T_SUFFIX: usize = 11;
const T_PEN: usize = 12;
const T_FLAG_TYPES: usize = 13;
const T_DMG: usize = 14;
const T_REGEN: usize = 15;
const T_DEGEN: usize = 16;
const T_COST: usize = 17;
const T_BASE_COST: usize = 18;

/// The parser of one thread: the data, the prepared scan tables, and the one piece of state upstream's parser mutates.
pub struct Parser {
    pub d: Rc<ModParserData>,
    st: Vec<ScanTable>,
    /// ModParser.lua:6862 `modName[2] = "Multiplier:" .. modNameString .. "Doubled"` writes into the `modNameList` entry
    /// itself (a shared upvalue table): the rewritten entries, by index. Upstream state that outlives the call.
    doubled: RefCell<HashMap<usize, Value>>,
}

thread_local! {
    static PARSER: Rc<Parser> = Rc::new(Parser::new());
}

/// This thread's parser.
pub fn parser() -> Rc<Parser> {
    PARSER.with(Rc::clone)
}

fn lua_err(s: impl Into<String>) -> ParseError {
    ParseError::Lua(LuaError(s.into()))
}

fn utf8(b: Vec<u8>) -> Result<String, ParseError> {
    String::from_utf8(b).map_err(|_| ParseError::Unrepresentable("a line split inside a UTF-8 sequence".into()))
}

/// A returned `modList` value as mods: the sequence `ipairs` sees; anything else in it is not representable.
pub(crate) fn mods_of(v: &Value) -> Result<Option<Vec<Mod>>, ParseError> {
    match v {
        Value::Nil => Ok(None),
        Value::Table(t) => {
            if !t.hash.is_empty() {
                return Err(ParseError::Unrepresentable("a modList with string keys".into()));
            }
            let end = t.array.iter().position(Value::is_nil).unwrap_or(t.array.len());
            if t.array[end..].iter().any(|x| !x.is_nil()) {
                return Err(ParseError::Unrepresentable("a modList with a hole".into()));
            }
            t.array[..end]
                .iter()
                .map(|x| match x {
                    Value::Mod(m) => Ok((**m).clone()),
                    other => Err(ParseError::Unrepresentable(format!("a {} in a modList", other.type_name()))),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some)
        }
        other => Err(ParseError::Unrepresentable(format!("a {} modList", other.type_name()))),
    }
}

fn extra_of(v: &Value) -> Result<Option<String>, ParseError> {
    match v {
        Value::Nil | Value::Bool(false) => Ok(None),
        Value::Str(s) => Ok(Some(s.to_string())),
        other => Err(ParseError::Unrepresentable(format!("a {} extra", other.type_name()))),
    }
}

/// `-v` (LuaJIT: numbers only here -- a numeric string was already converted by `tonumber`).
fn neg(v: &Value) -> Result<Value, ParseError> {
    match v {
        Value::Num(n) => Ok(Value::Num(-n)),
        Value::Str(s) => match pob_lua::tonumber(s) {
            Some(n) => Ok(Value::Num(-n)),
            None => Err(lua_err("attempt to perform arithmetic on local 'modValue' (a string value)")),
        },
        other => Err(lua_err(format!(
            "attempt to perform arithmetic on local 'modValue' (a {} value)",
            other.type_name()
        ))),
    }
}

/// `copyTable(t)` of a tag source (`data.tag`, `entry.tagList[i]`) -> a new tag object.
fn copy_tag(v: &Value) -> Result<Tag, ParseError> {
    match v {
        Value::Table(t) => Ok(Tag::new((**t).clone())),
        other => Err(lua_err(format!(
            "bad argument #1 to 'pairs' (table expected, got {})",
            other.type_name()
        ))),
    }
}

/// `ipairs(t)` of a value (errors like LuaJIT on a non-table).
fn ipairs(v: &Value) -> Result<Vec<Value>, ParseError> {
    match v {
        Value::Table(t) => Ok(t.array.iter().take_while(|x| !x.is_nil()).cloned().collect()),
        other => Err(lua_err(format!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            other.type_name()
        ))),
    }
}

fn str_v(v: &Value) -> Option<Str> {
    if let Value::Str(s) = v { Some(s.clone()) } else { None }
}

/// Lua `%S` somewhere in the bytes (`line:match("%S")`).
fn has_non_space(b: &[u8]) -> bool {
    b.iter().any(|c| !matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
}

impl Parser {
    fn new() -> Parser {
        let d = pob_data::mod_parser::mod_parser();
        let st = pob_data::mod_parser::TABLE_NAMES
            .iter()
            .map(|n| ScanTable::new(d.table(n)))
            .collect();
        Parser {
            d,
            st,
            doubled: RefCell::new(HashMap::new()),
        }
    }

    fn table(&self, t: usize) -> &PatternTable {
        self.d.table(pob_data::mod_parser::TABLE_NAMES[t])
    }

    fn scan(&self, line: &[u8], t: usize, plain: bool) -> Result<Option<Hit>, ParseError> {
        scan(line, self.table(t), &self.st[t], plain, pob_data::mod_parser::TABLE_NAMES[t])
    }

    /// `local v, line, caps = scan(line, list[, plain])`: the value (nil when none) and the new line.
    fn scan_val(&self, line: &mut Vec<u8>, t: usize, plain: bool) -> Result<Scanned, ParseError> {
        match self.scan(line, t, plain)? {
            None => Ok((None, Vec::new(), None)),
            Some(h) => {
                *line = h.rest;
                let v = match (t, self.doubled.borrow().get(&h.entry)) {
                    (T_NAME, Some(w)) => PVal::Lua(w.clone()),
                    _ => self.table(t).vals[h.entry].clone(),
                };
                Ok((Some(v), h.caps, Some(h.entry)))
            }
        }
    }

    /// A static value (tables whose values are never closures).
    fn lua(v: Option<PVal>) -> Result<Value, ParseError> {
        match v {
            None => Ok(Value::Nil),
            Some(PVal::Lua(x)) => Ok(x),
            Some(PVal::Fn(f)) => Err(ParseError::Unported(f.id.clone())),
            Some(_) => Err(ParseError::Unrepresentable(
                "a closure-holding table where a value was expected".into(),
            )),
        }
    }

    /// The inner `parseMod(line, order)`.
    pub fn parse(&self, line_s: &str, order: u8) -> Result<RawParse, ParseError> {
        let d = &*self.d;
        let mut line: Vec<u8> = line_s.as_bytes().to_vec();
        // :6642-6648 jewelFuncList: the first pattern (in `pairs` order) whose find yields cap1
        let lower = line.to_ascii_lowercase();
        let jt = self.table(T_JEWEL);
        let mut jewel_hits = 0;
        let mut first_jewel = None;
        for (i, k) in self.st[T_JEWEL].filtered(&lower) {
            if let Some((_, _, caps)) = pob_lua::find(&lower, k, Some(1), false).map_err(|e| lua_err(e.0))?
                && !caps.is_empty()
            {
                jewel_hits += 1;
                first_jewel.get_or_insert(i);
            }
        }
        if jewel_hits > 1 {
            crate::note_jewel_multi(line_s);
        }
        if let Some(i) = first_jewel {
            return Err(unported_in(&jt.vals[i]));
        }
        let lower_s = std::str::from_utf8(&lower).unwrap_or_default();
        // :6649-6652 an exact jewel key: `{ mod("JewelFunc", "LIST", jewelFunc) }` -- a value holding a function (R5.4)
        if let Some(v) = jt.get(lower_s) {
            return Err(unported_in(v));
        }
        // :6653-6656 clusterJewelSkills[lineLower] (the shared table itself; the wrapper copies)
        if let Some(v) = self.table(T_CLUSTER).get(lower_s) {
            return Ok(RawParse {
                mod_list: mods_of(&Self::lua(Some(v.clone()))?)?,
                ..RawParse::default()
            });
        }
        // :6657-6659
        if self
            .table(T_UNSUPPORTED)
            .get(lower_s)
            .and_then(PVal::as_lua)
            .is_some_and(Value::truthy)
        {
            return Ok(RawParse {
                mod_list: Some(Vec::new()),
                extra: Some(line_s.to_owned()),
                ..RawParse::default()
            });
        }
        // :6660-6667 specialModList: a match covering the whole line
        if let Some(h) = self.scan(&line, T_SPECIAL, false)?
            && h.rest.is_empty()
        {
            let t = self.table(T_SPECIAL);
            return match &t.vals[h.entry] {
                PVal::Fn(f) => {
                    let mut args = vec![h.caps.first().map_or(Value::Nil, tonumber)];
                    args.extend(h.caps.iter().cloned());
                    let (ml, ex) = call_special(d, f, &args)?;
                    Ok(RawParse {
                        mod_list: mods_of(&ml)?,
                        extra: extra_of(&ex)?,
                        ..RawParse::default()
                    })
                }
                PVal::Lua(v) => Ok(RawParse {
                    // `copyTable(specialMod)`
                    mod_list: mods_of(v)?.map(|l| l.iter().map(copy_table_mod).collect()),
                    extra: None,
                    orphan_tags: t
                        .orphan_tags
                        .iter()
                        .filter(|(e, _, _)| *e == h.entry)
                        .map(|(_, slot, o)| (*slot, o.clone()))
                        .collect(),
                }),
                other => Err(unported_in(other)),
            };
        }
        // :6669-6673
        if let Some(c) = pob_lua::match_(&line, b"^Added Small Passive Skills also grant: (.+)$", None).map_err(|e| lua_err(e.0))? {
            let cap = c.get(0).and_then(|x| x.bytes(&line)).map(<[u8]>::to_vec).unwrap_or_default();
            let m = mk("AddToClusterJewelNode", "LIST", Value::str(&utf8(cap)?), &[])?;
            return Ok(RawParse {
                mod_list: Some(vec![m]),
                ..RawParse::default()
            });
        }
        // :6675
        line.push(b' ');
        // :6678-6683 preFlag
        let (pre_flag, pre_flag_cap, _) = self.scan_val(&mut line, T_PRE_FLAG, false)?;
        let pre_flag = match pre_flag {
            Some(PVal::Fn(f)) => call_value(d, &f, &pre_flag_cap)?,
            v => Self::lua(v)?,
        };
        // :6686-6687
        let (skill_tag, _, _) = self.scan_val(&mut line, T_PRE_SKILL, false)?;
        let mut skill_tag = Self::lua(skill_tag)?;
        // :6690-6694
        let (mod_form, mut form_cap, _) = self.scan_val(&mut line, T_FORM, false)?;
        let Some(mod_form) = mod_form else {
            return Ok(RawParse {
                mod_list: None,
                extra: Some(utf8(line)?),
                ..RawParse::default()
            });
        };
        let mod_form = str_v(&Self::lua(Some(mod_form))?).ok_or_else(|| ParseError::Unrepresentable("a non-string form".into()))?;
        // :6697-6714 tags
        let mod_tag = self.tag(&mut line)?;
        let mod_tag2 = if mod_tag.truthy() { self.tag(&mut line)? } else { Value::Nil };
        // :6716-6719
        if order == 2 && !skill_tag.truthy() {
            skill_tag = Self::lua(self.scan_val(&mut line, T_SKILL, false)?.0)?;
        }
        // :6720-6752 name by form
        let mut mod_name_entry = None;
        let mut mod_name = match &*mod_form {
            "PEN" | "BASECOST" | "TOTALCOST" => {
                let t = match &*mod_form {
                    "PEN" => T_PEN,
                    "BASECOST" => T_BASE_COST,
                    _ => T_COST,
                };
                let n = Self::lua(self.scan_val(&mut line, t, true)?.0)?;
                if !n.truthy() {
                    return empty_with(line);
                }
                self.scan_val(&mut line, T_NAME, true)?;
                n
            }
            "FLAG" => {
                let v = Self::lua(self.scan_val(&mut line, T_FLAG_TYPES, false)?.0)?;
                set_cap1(&mut form_cap, v.clone());
                if !v.truthy() {
                    return Ok(RawParse {
                        mod_list: None,
                        extra: Some(utf8(line)?),
                        ..RawParse::default()
                    });
                }
                Self::lua(self.scan_val(&mut line, T_NAME, true)?.0)?
            }
            _ => {
                let (v, _, e) = self.scan_val(&mut line, T_NAME, true)?;
                mod_name_entry = e;
                Self::lua(v)?
            }
        };
        // :6753-6755
        if order == 1 && !skill_tag.truthy() {
            skill_tag = Self::lua(self.scan_val(&mut line, T_SKILL, false)?.0)?;
        }
        // :6758-6759
        let mut mod_flag = Self::lua(self.scan_val(&mut line, T_FLAG, true)?.0)?;
        // :6762-6872 value and type by form
        let cap = |i: usize| form_cap.get(i).cloned().unwrap_or_default();
        let cap1 = cap(0);
        let t1 = tonumber(&cap1);
        let mut mod_value = if t1.truthy() { t1 } else { cap1 };
        let mut value_list: Option<Vec<Value>> = None;
        let mut mod_type: Vec<Str> = vec![Str::from("BASE")];
        let mut type_list = false;
        let mut mod_suffix = Value::Nil;
        let mut mod_extra_tags = Value::Nil;
        let hand_attack = || tbl([("tag", tbl([("type", Value::str("Condition")), ("var", Value::str("{Hand}Attack"))]))]);
        let set_type = |t: &mut Vec<Str>, s: &str| *t = vec![Str::from(s)];
        match &*mod_form {
            "INC" => set_type(&mut mod_type, "INC"),
            "RED" => {
                mod_value = neg(&mod_value)?;
                set_type(&mut mod_type, "INC");
            }
            "MORE" => set_type(&mut mod_type, "MORE"),
            "LESS" => {
                mod_value = neg(&mod_value)?;
                set_type(&mut mod_type, "MORE");
            }
            "BASE" | "GAIN" | "GRANTS_GLOBAL" => mod_suffix = Self::lua(self.scan_val(&mut line, T_SUFFIX, true)?.0)?,
            "LOSE" => {
                mod_value = neg(&mod_value)?;
                mod_suffix = Self::lua(self.scan_val(&mut line, T_SUFFIX, true)?.0)?;
            }
            "GRANTS" => {
                mod_extra_tags = hand_attack();
                mod_suffix = Self::lua(self.scan_val(&mut line, T_SUFFIX, true)?.0)?;
            }
            "REMOVES" => {
                mod_value = neg(&mod_value)?;
                mod_extra_tags = hand_attack();
                mod_suffix = Self::lua(self.scan_val(&mut line, T_SUFFIX, true)?.0)?;
            }
            "CHANCE" => {}
            "REGENPERCENT" | "REGENFLAT" | "DEGENPERCENT" | "DEGENFLAT" => {
                let t = if mod_form.starts_with("REGEN") { T_REGEN } else { T_DEGEN };
                mod_name = self.lookup(t, &cap(1))?;
                if mod_form.ends_with("PERCENT") {
                    mod_suffix = Value::str("Percent");
                }
            }
            "DEGEN" => {
                let dt = self.lookup(T_DMG, &cap(1))?;
                if !dt.truthy() {
                    return empty_with(line);
                }
                mod_name = concat(&dt, &Value::str("Degen"))?;
                mod_suffix = Value::str("");
            }
            "DMG" | "DMGATTACKS" | "DMGSPELLS" | "DMGBOTH" => {
                let dt = self.lookup(T_DMG, &cap(2))?;
                if !dt.truthy() {
                    return empty_with(line);
                }
                value_list = Some(vec![tonumber(&cap(0)), tonumber(&cap(1))]);
                mod_name = Value::table(Table {
                    array: vec![concat(&dt, &Value::str("Min"))?, concat(&dt, &Value::str("Max"))?],
                    ..Table::default()
                });
                let kw = |names: &[&str]| -> Value {
                    let g = pob_data::globals::globals();
                    let f = names.iter().fold(0.0, |a, n| bor(a, g.keyword_flag[*n]));
                    tbl([("keywordFlags", Value::Num(f))])
                };
                if !mod_flag.truthy() {
                    match &*mod_form {
                        "DMGATTACKS" => mod_flag = kw(&["Attack"]),
                        "DMGSPELLS" => mod_flag = kw(&["Spell"]),
                        "DMGBOTH" => mod_flag = kw(&["Attack", "Spell"]),
                        _ => {}
                    }
                }
            }
            "FLAG" => {
                // :6849-6851 `type(modValue) == "table" and modValue.x or y`
                if let Value::Table(t) = &mod_value {
                    let t = t.clone();
                    let n = t.get("name").clone();
                    mod_name = if n.truthy() { n } else { mod_value.clone() };
                    let ty = t.get("type").clone();
                    mod_type = vec![str_v(&ty).ok_or_else(|| ParseError::Unrepresentable("a non-string FLAG type".into()))?];
                    let v = t.get("value").clone();
                    mod_value = if v.truthy() { v } else { Value::Bool(true) };
                } else {
                    mod_name = mod_value.clone();
                    set_type(&mut mod_type, "FLAG");
                    mod_value = Value::Bool(true);
                }
            }
            "OVERRIDE" => set_type(&mut mod_type, "OVERRIDE"),
            "DOUBLED" => {
                // :6856-6871
                let name_string;
                if let Value::Table(t) = &mod_name {
                    name_string = t.array.first().cloned().unwrap_or_default();
                    let v2 = concat(&concat(&Value::str("Multiplier:"), &name_string)?, &Value::str("Doubled"))?;
                    let mut nt = (**t).clone();
                    match nt.array.len() {
                        0 => return Err(ParseError::Unrepresentable("modName[2] = x on an empty array part".into())),
                        1 => nt.array.push(v2),
                        _ => nt.array[1] = v2,
                    }
                    mod_name = Value::table(nt);
                    if let Some(e) = mod_name_entry {
                        self.doubled.borrow_mut().insert(e, mod_name.clone());
                        crate::note_doubled(line_s);
                    }
                } else {
                    name_string = mod_name.clone();
                    if mod_name.truthy() {
                        let v2 = concat(&concat(&Value::str("Multiplier:"), &mod_name)?, &Value::str("Doubled"))?;
                        mod_name = Value::table(Table {
                            array: vec![mod_name.clone(), v2],
                            ..Table::default()
                        });
                    }
                }
                if mod_name.truthy() {
                    mod_type = vec![Str::from("MORE"), Str::from("OVERRIDE")];
                    type_list = true;
                    value_list = Some(vec![Value::Num(100.0), Value::Num(1.0)]);
                    let ns = |suffix: &str| concat(&name_string, &Value::str(suffix));
                    let inner = tbl([(
                        "tag",
                        tbl([
                            ("type", Value::str("Multiplier")),
                            ("var", ns("Doubled")?),
                            ("globalLimit", Value::Num(100.0)),
                            ("globalLimitKey", ns("DoubledLimit")?),
                        ]),
                    )]);
                    let mut t = Table::default();
                    t.array.push(inner);
                    t.hash.insert(Str::from("tag"), Value::Bool(true));
                    mod_extra_tags = Value::table(t);
                }
            }
            _ => {}
        }
        // :6874-6876
        if !mod_name.truthy() {
            return empty_with(line);
        }
        // :6877-6917 combine flags and tags
        let mut flags = 0.0;
        let mut keyword_flags = 0.0;
        let mut tag_list: Vec<Tag> = Vec::new();
        let mut mod_tag_list: Option<Vec<Vec<Tag>>> = None;
        let mut misc = Misc::default();
        for data in [&mod_name, &pre_flag, &mod_flag, &mod_tag, &mod_tag2, &skill_tag, &mod_extra_tags] {
            let Value::Table(t) = data else {
                if matches!(data, Value::Mod(_)) {
                    return Err(ParseError::Unrepresentable("a mod as a tag source".into()));
                }
                continue;
            };
            let num_or0 = |v: &Value| -> Result<f64, ParseError> {
                match v {
                    Value::Nil | Value::Bool(false) => Ok(0.0),
                    Value::Num(n) => Ok(*n),
                    other => Err(lua_err(format!(
                        "bad argument #2 to 'bor' (number expected, got {})",
                        other.type_name()
                    ))),
                }
            };
            flags = bor(flags, num_or0(t.get("flags"))?);
            keyword_flags = bor(keyword_flags, num_or0(t.get("keywordFlags"))?);
            let first = t.array.first().cloned().unwrap_or_default();
            let first_has = |k: &str| -> Result<bool, ParseError> { Ok(first.truthy() && index(&first, k)?.truthy()) };
            if t.get("tag").truthy() {
                if first_has("tag")? {
                    let mut l = Vec::new();
                    for entry in ipairs(data)? {
                        let mut v = Vec::new();
                        let et = index(&entry, "tag")?;
                        if et.truthy() {
                            v.push(copy_tag(&et)?);
                        }
                        l.push(v);
                    }
                    mod_tag_list = Some(l);
                } else {
                    tag_list.push(copy_tag(t.get("tag"))?);
                }
            } else if t.get("tagList").truthy() {
                if first_has("tagList")? {
                    let mut l = Vec::new();
                    for entry in ipairs(data)? {
                        let mut v = Vec::new();
                        for tag in ipairs(&index(&entry, "tagList")?)? {
                            v.push(copy_tag(&tag)?);
                        }
                        l.push(v);
                    }
                    mod_tag_list = Some(l);
                } else {
                    for tag in ipairs(t.get("tagList"))? {
                        tag_list.push(copy_tag(&tag)?);
                    }
                }
            }
            for (k, v) in &t.hash {
                misc.set(k, v.clone());
            }
        }
        // :6919-6930 generate the mod list
        let names = match &mod_name {
            Value::Table(t) => t.array.iter().take_while(|x| !x.is_nil()).cloned().collect(),
            v => vec![v.clone()],
        };
        let suffix = if mod_suffix.truthy() {
            mod_suffix.clone()
        } else {
            let m = misc.get("modSuffix");
            if m.truthy() { m } else { Value::str("") }
        };
        let mut mod_list = Vec::with_capacity(names.len());
        for (i, name) in names.iter().enumerate() {
            let full = concat(name, &suffix)?;
            let ty_s = if type_list {
                mod_type
                    .get(i)
                    .cloned()
                    .ok_or_else(|| ParseError::Unrepresentable("a table as a mod type".into()))?
            } else {
                mod_type[0].clone()
            };
            let ty = ModType::parse(&ty_s).ok_or_else(|| ParseError::Unrepresentable(format!("mod type '{ty_s}'")))?;
            let value = match &value_list {
                Some(l) => match l.get(i) {
                    Some(v) if v.truthy() => v.clone(),
                    _ => return Err(ParseError::Unrepresentable("a table as a mod value (modValue[i] is nil)".into())),
                },
                None => mod_value.clone(),
            };
            let Value::Str(full) = full else {
                unreachable!("concat yields a string")
            };
            let mut m = create_mod(
                &full,
                Some(ty),
                value,
                None,
                flag_u32(flags)?,
                flag_u32(keyword_flags)?,
                tag_list.clone(),
            );
            if let Some(l) = &mod_tag_list
                && let Some(extra) = l.get(i)
            {
                // `t_insert(modList[i], unpack(modTagList[i]))`
                match extra.len() {
                    1 => m.tags.push(extra[0].clone()),
                    0 => return Err(lua_err("wrong number of arguments to 'insert'")),
                    _ => return Err(lua_err("bad argument #2 to 'insert' (number expected, got table)")),
                }
            }
            mod_list.push(m);
        }
        // :6931-6990 special handling
        if !mod_list.is_empty() {
            let wrap = |m: Mod| Value::Mod(Rc::new(m));
            if misc.get("addToAura").truthy() {
                let banner = if misc.get("onlyAddToBanners").truthy() {
                    let st = pob_data::globals::globals().skill_type["Banner"];
                    vec![tbl([("type", Value::str("SkillType")), ("skillType", Value::Num(st))])]
                } else {
                    Vec::new()
                };
                mod_list = mod_list
                    .into_iter()
                    .map(|m| mk("ExtraAuraEffect", "LIST", tbl([("mod", wrap(m))]), &banner))
                    .collect::<Result<_, _>>()?;
            } else if misc.get("newAura").truthy() {
                let only = misc.get("newAuraOnlyAllies");
                mod_list = mod_list
                    .into_iter()
                    .map(|mut m| {
                        let tags: Vec<Value> = m.tags.drain(..).map(|t| Value::Table(t.table())).collect();
                        mk("ExtraAura", "LIST", tbl([("mod", wrap(m)), ("onlyAllies", only.clone())]), &tags)
                    })
                    .collect::<Result<_, _>>()?;
            } else if misc.get("addToMinion").truthy() {
                let mut tags = Vec::new();
                for k in ["playerTag", "addToMinionTag"] {
                    let v = misc.get(k);
                    if v.truthy() {
                        tags.push(v);
                    }
                }
                let ptl = misc.get("playerTagList");
                if ptl.truthy() {
                    tags.extend(ipairs(&ptl)?);
                }
                mod_list = mod_list
                    .into_iter()
                    .map(|m| mk("MinionModifier", "LIST", tbl([("mod", wrap(m))]), &tags))
                    .collect::<Result<_, _>>()?;
            } else if misc.get("addToSkill").truthy() {
                let s = misc.get("addToSkill");
                mod_list = mod_list
                    .into_iter()
                    .map(|m| mk("ExtraSkillMod", "LIST", tbl([("mod", wrap(m))]), std::slice::from_ref(&s)))
                    .collect::<Result<_, _>>()?;
            } else if misc.get("applyToEnemy").truthy() {
                let mut tags = Vec::new();
                let pt = misc.get("playerTag");
                if pt.truthy() {
                    tags.push(pt);
                }
                let ptl = misc.get("playerTagList");
                if ptl.truthy() {
                    tags.extend(ipairs(&ptl)?);
                }
                let actor_enemy = misc.get("actorEnemy").truthy();
                mod_list = mod_list
                    .into_iter()
                    .map(|m| {
                        let new_mod = if !m.tags.is_empty() && actor_enemy {
                            // `newMod = copyTable(effectMod); newMod[1]["actor"] = "enemy"`
                            let mut c = copy_table_mod(&m);
                            let mut t0 = (*c.tags[0].table()).clone();
                            t0.hash.insert(Str::from("actor"), Value::str("enemy"));
                            c.tags[0] = Tag::new(t0);
                            c
                        } else {
                            m
                        };
                        mk("EnemyModifier", "LIST", tbl([("mod", wrap(new_mod))]), &tags)
                    })
                    .collect::<Result<_, _>>()?;
            }
        }
        // :6991 `return modList, line:match("%S") and line`
        let extra = if has_non_space(&line) { Some(utf8(line)?) } else { None };
        Ok(RawParse {
            mod_list: Some(mod_list),
            extra,
            orphan_tags: Vec::new(),
        })
    }

    /// :6697-6705 / :6706-6713 one `modTagList` scan with the closure call.
    fn tag(&self, line: &mut Vec<u8>) -> Result<Value, ParseError> {
        let (v, caps, _) = self.scan_val(line, T_TAG, false)?;
        match v {
            Some(PVal::Fn(f)) => {
                let c1 = caps.first().cloned().unwrap_or_default();
                let digits = match &c1 {
                    Value::Str(s) => pob_lua::match_(s.as_bytes(), b"%d+", None).map_err(|e| lua_err(e.0))?.is_some(),
                    other => return Err(lua_err(format!("attempt to index field '?' (a {} value)", other.type_name()))),
                };
                let mut args = vec![if digits { tonumber(&c1) } else { c1 }];
                args.extend(caps);
                call_value(&self.d, &f, &args)
            }
            v => Self::lua(v),
        }
    }

    /// `list[key]` of a lookup table (`regenTypes[formCap[2]]`, `dmgTypes[...]`): a non-string key reads nil.
    fn lookup(&self, t: usize, key: &Value) -> Result<Value, ParseError> {
        match key {
            Value::Str(k) => Self::lua(self.table(t).get(k).cloned()),
            _ => Ok(Value::Nil),
        }
    }
}

fn set_cap1(caps: &mut Vec<Value>, v: Value) {
    if caps.is_empty() {
        caps.push(v);
    } else {
        caps[0] = v;
    }
}

/// `return { }, line`.
fn empty_with(line: Vec<u8>) -> Result<RawParse, ParseError> {
    Ok(RawParse {
        mod_list: Some(Vec::new()),
        extra: Some(utf8(line)?),
        ..RawParse::default()
    })
}

/// The first closure inside a table value (`jewelFuncList` entries), as `Unported`.
fn unported_in(v: &PVal) -> ParseError {
    fn first(v: &PVal) -> Option<Str> {
        match v {
            PVal::Fn(f) => Some(f.id.clone()),
            PVal::Table { array, hash } => array.iter().chain(hash.values()).find_map(first),
            _ => None,
        }
    }
    match first(v) {
        Some(id) => ParseError::Unported(id),
        None => ParseError::Unrepresentable("a table value without a closure where one was expected".into()),
    }
}

/// `createMod` re-export for the closures of R5.3 (`mod(...)` with Lua values).
pub fn lua_mod(name: &Value, ty: &Value, value: Value, va: &[Value]) -> Result<Mod, ParseError> {
    create_mod_lua(name, ty, value, va)
}
