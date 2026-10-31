//! The named helpers of ModParser.lua (and `modLib.createMod`) that closures call by upvalue (R5-ARCH §4: ported by hand,
//! the generated closures of R5.3 call these). Each carries its upstream lines; the registry (`closures.rs`) pins the sha
//! of every body.
//!
//! Values cross these functions as Lua values (`pob_mods::Value`); a Lua runtime error comes back as `ParseError::Lua`
//! with LuaJIT's text where the case is reachable from data, a value the Rust model cannot hold as
//! `ParseError::Unrepresentable`.

use std::cell::RefCell;
use std::rc::Rc;

use pob_data::mod_parser::{FnRef, ModParserData, PVal};
use pob_mods::{LuaError, Mod, ModStore, ModType, Str, Table, Tag, Value, create_mod};

use crate::ParseError;

// --- small Lua semantics ------------------------------------------------------------------------------------------

/// Lua `tonumber(v)` for one value (L11: `tonumber(nil)` is nil, not an error -- the argument is present).
pub fn tonumber(v: &Value) -> Value {
    match v {
        Value::Num(_) => v.clone(),
        Value::Str(s) => pob_lua::tonumber(s).map_or(Value::Nil, Value::Num),
        _ => Value::Nil,
    }
}

/// The string a string operation sees: a string, or a number coerced by `tostring` (LuaJIT `%.14g`).
pub fn lua_string(v: &Value, what: &str) -> Result<String, ParseError> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        Value::Num(n) => Ok(pob_lua::tostring_num(*n)),
        other => Err(ParseError::Lua(LuaError(format!(
            "attempt to index {what} (a {} value)",
            other.type_name()
        )))),
    }
}

/// Lua `a .. b` (strings and numbers only).
pub fn concat(a: &Value, b: &Value) -> Result<Value, ParseError> {
    let s = |v: &Value| match v {
        Value::Str(s) => Ok(s.to_string()),
        Value::Num(n) => Ok(pob_lua::tostring_num(*n)),
        other => Err(ParseError::Lua(LuaError(format!(
            "attempt to concatenate a {} value",
            other.type_name()
        )))),
    };
    Ok(Value::str(&(s(a)? + &s(b)?)))
}

/// `t[k]` on any value as Lua indexes it: tables by key, strings through the string library (no field a mod table uses
/// is a `string.*` function name, so nil), everything else an error.
pub fn index(v: &Value, key: &str) -> Result<Value, ParseError> {
    match v {
        Value::Table(t) => Ok(t.get(key).clone()),
        Value::Mod(m) => Ok(m.field(key)),
        Value::Str(_) => string_lib_field(key),
        other => Err(ParseError::Lua(LuaError(format!("attempt to index a {} value", other.type_name())))),
    }
}

/// `("x").key` -> `string[key]`: a function for the library names (not representable), nil otherwise.
fn string_lib_field(key: &str) -> Result<Value, ParseError> {
    const LIB: [&str; 15] = [
        "byte", "char", "dump", "find", "format", "gmatch", "gsub", "len", "lower", "match", "rep", "reverse", "sub", "upper", "gfind",
    ];
    if LIB.contains(&key) {
        Err(ParseError::Unrepresentable(format!("string.{key} read as a field")))
    } else {
        Ok(Value::Nil)
    }
}

/// `s:gsub(pattern, repl)` with a string replacement; the first return.
pub fn gsub_str(s: &str, pattern: &str, repl: &str) -> Result<String, ParseError> {
    let (out, _) = pob_lua::gsub(s.as_bytes(), pattern.as_bytes(), pob_lua::Repl::Str(repl.as_bytes()), None)
        .map_err(|e| ParseError::Lua(LuaError(e.0)))?;
    String::from_utf8(out).map_err(|_| ParseError::Unrepresentable("gsub produced a non-UTF-8 string".into()))
}

/// `s:gsub(pattern, string.upper)`; the first return.
pub fn gsub_upper(s: &str, pattern: &str) -> Result<String, ParseError> {
    // the function gets the subject and the captures (the whole match when the pattern has none) -- R5.3a fix: it
    // upper-cased the whole subject before
    let mut f = |subject: &[u8], caps: &pob_lua::Captures| caps.get(0).and_then(|c| c.bytes(subject)).map(<[u8]>::to_ascii_uppercase);
    let (out, _) =
        pob_lua::gsub(s.as_bytes(), pattern.as_bytes(), pob_lua::Repl::Fn(&mut f), None).map_err(|e| ParseError::Lua(LuaError(e.0)))?;
    String::from_utf8(out).map_err(|_| ParseError::Unrepresentable("gsub produced a non-UTF-8 string".into()))
}

/// A Lua table literal with string keys; nil fields are absent (as in a Lua constructor).
pub fn tbl<const N: usize>(fields: [(&str, Value); N]) -> Value {
    let mut t = Table::default();
    for (k, v) in fields {
        if !v.is_nil() {
            t.hash.insert(Str::from(k), v);
        }
    }
    Value::table(t)
}

/// A Lua sequence `{ a, b, ... }` of mods.
pub fn mod_list(mods: Vec<Mod>) -> Value {
    Value::table(Table {
        array: mods.into_iter().map(|m| Value::Mod(Rc::new(m))).collect(),
        ..Table::default()
    })
}

// --- ModTools.lua:33-67 modLib.createMod ----------------------------------------------------------------------------

/// `modLib.createMod(modName, modType, modVal, ...)` with the vararg heuristic of ModTools.lua:37-49 (MOD-CORE §1): a string
/// in slot 1 is `source`, a number in slot 2 `flags`, a number in slot 3 `keywordFlags`; everything from `tagStart` on goes
/// into the array part. That array part is what `ipairs(mod)` sees: tags up to the first nil (L9); a value after a hole, or
/// a non-table in a tag slot, is not representable in `Mod` -> `Unrepresentable` (none in the data).
pub fn create_mod_lua(name: &Value, ty: &Value, value: Value, va: &[Value]) -> Result<Mod, ParseError> {
    let mut tag_start = 0;
    let mut source = None;
    let mut flags = 0.0;
    let mut kw = 0.0;
    if let Some(Value::Str(s)) = va.first() {
        source = Some(s.clone());
        tag_start = 1;
    }
    if let Some(Value::Num(n)) = va.get(1) {
        flags = *n;
        tag_start = 2;
    }
    if let Some(Value::Num(n)) = va.get(2) {
        kw = *n;
        tag_start = 3;
    }
    let Value::Str(name) = name else {
        return Err(ParseError::Unrepresentable(format!("createMod: a {} mod name", name.type_name())));
    };
    let ty = match ty {
        Value::Nil => None,
        Value::Str(s) => Some(ModType::parse(s).ok_or_else(|| ParseError::Unrepresentable(format!("mod type '{s}'")))?),
        other => return Err(ParseError::Unrepresentable(format!("createMod: a {} mod type", other.type_name()))),
    };
    let mut tags = Vec::new();
    let rest = va.get(tag_start..).unwrap_or_default();
    let end = rest.iter().position(Value::is_nil).unwrap_or(rest.len());
    for t in &rest[..end] {
        match t {
            Value::Table(t) => tags.push(Tag::from_rc(t.clone())),
            other => {
                return Err(ParseError::Unrepresentable(format!(
                    "createMod: a {} in a tag slot",
                    other.type_name()
                )));
            }
        }
    }
    if rest[end..].iter().any(|v| !v.is_nil()) {
        return Err(ParseError::Unrepresentable("createMod: a tag after a hole (L9 orphan)".into()));
    }
    let mut m = create_mod(name, ty, value, None, flag_u32(flags)?, flag_u32(kw)?, tags);
    m.source = source;
    Ok(m)
}

pub(crate) fn flag_u32(f: f64) -> Result<u32, ParseError> {
    if f >= 0.0 && f.fract() == 0.0 && f < 4_294_967_296.0 {
        Ok(f as u32)
    } else {
        Err(ParseError::Unrepresentable(format!("flags {f}")))
    }
}

/// `mod(name, type, value, ...)` with a string name/type (the common closure call).
pub fn mk(name: &str, ty: &str, value: Value, va: &[Value]) -> Result<Mod, ParseError> {
    create_mod_lua(&Value::str(name), &Value::str(ty), value, va)
}

// --- ModParser.lua:2012-2014 flag ---------------------------------------------------------------------------------

/// `flag(name, ...)` = `mod(name, "FLAG", true, ...)`.
pub fn flag(name: &Value, va: &[Value]) -> Result<Mod, ParseError> {
    create_mod_lua(name, &Value::str("FLAG"), Value::Bool(true), va)
}

// --- ModParser.lua:13-15 firstToUpper -----------------------------------------------------------------------------

/// `(str:gsub("^%l", string.upper))`: an ASCII lowercase first byte goes upper.
pub fn first_to_upper(s: &str) -> String {
    let mut b = s.as_bytes().to_vec();
    if let Some(c) = b.first_mut() {
        c.make_ascii_uppercase();
    }
    String::from_utf8(b).unwrap_or_else(|_| s.to_owned())
}

// --- ModParser.lua:2015-2031 dealNoNonDamageType ------------------------------------------------------------------

const DAMAGE_TYPE_LIST: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];

/// `dealNoNonDamageType(dmgType, forMinion)`: nil when the (first-uppered) type is not a damage type, else a `DealNo<X>`
/// flag for every other type (wrapped in `MinionModifier` when `forMinion` is truthy).
pub fn deal_no_non_damage_type(dmg_type: &Value, for_minion: &Value) -> Result<Value, ParseError> {
    let dmg = first_to_upper(&lua_string(dmg_type, "local 'str'")?);
    if !DAMAGE_TYPE_LIST.contains(&dmg.as_str()) {
        return Ok(Value::Nil);
    }
    let mut mods = Vec::new();
    for d in DAMAGE_TYPE_LIST {
        if d != dmg {
            let deal_no = flag(&Value::str(&format!("DealNo{d}")), &[])?;
            mods.push(if for_minion.truthy() {
                mk("MinionModifier", "LIST", tbl([("mod", Value::Mod(Rc::new(deal_no)))]), &[])?
            } else {
                deal_no
            });
        }
    }
    Ok(mod_list(mods))
}

// --- ModParser.lua:2047-2054 grantedExtraSkill ----------------------------------------------------------------------

/// `grantedExtraSkill(name, level, noSupports)`: zero returns (nil here) when the skill is unknown.
pub fn granted_extra_skill(d: &ModParserData, name: &Value, level: &Value, no_supports: &Value) -> Result<Value, ParseError> {
    let name = gsub_str(&lua_string(name, "local 'name'")?, " skill", "")?;
    let Some(id) = d.gem_id_lookup.get(name.as_str()) else {
        return Ok(Value::Nil);
    };
    let v = tbl([
        ("skillId", Value::Str(id.clone())),
        ("level", tonumber(level)),
        ("noSupports", no_supports.clone()),
    ]);
    Ok(mod_list(vec![mk("ExtraSkill", "LIST", v, &[])?]))
}

// --- ModParser.lua:2055-2072 triggerExtraSkill ----------------------------------------------------------------------

/// `triggerExtraSkill(name, level, options)`.
pub fn trigger_extra_skill(d: &ModParserData, name: &Value, level: &Value, options: &Value) -> Result<Value, ParseError> {
    let opt = |k: &str| -> Result<Value, ParseError> { if options.truthy() { index(options, k) } else { Ok(Value::Nil) } };
    let name = gsub_str(&lua_string(name, "local 'name'")?, " skill", "")?;
    let mut source_skill = opt("sourceSkill")?;
    if source_skill.truthy() {
        source_skill = Value::str(&gsub_str(&lua_string(&source_skill, "field 'sourceSkill'")?, " skill", "")?);
    }
    let skill_id = d.gem_id_lookup.get(name.as_str()).map_or(Value::Nil, |s| Value::Str(s.clone()));
    let mut mods = Vec::new();
    if skill_id.truthy() {
        let v = tbl([
            ("skillId", skill_id.clone()),
            ("level", tonumber(level)),
            ("noSupports", opt("noSupports")?),
            ("triggered", Value::Bool(true)),
            ("source", source_skill),
            ("triggerChance", tonumber(&opt("triggerChance")?)),
        ]);
        mods.push(mk("ExtraSkill", "LIST", v, &[])?);
    }
    let skill_tag = || tbl([("type", Value::str("SkillId")), ("skillId", skill_id.clone())]);
    if opt("ignoreHexproof")?.truthy() {
        let v = tbl([("key", Value::str("ignoreHexproof")), ("value", Value::Bool(true))]);
        mods.push(mk("SkillData", "LIST", v, &[skill_tag()])?);
    }
    if opt("onCrit")?.truthy() {
        let inner = mk(
            "SkillData",
            "LIST",
            tbl([("key", Value::str("triggerOnCrit")), ("value", Value::Bool(true))]),
            &[],
        )?;
        mods.push(mk(
            "ExtraSkillMod",
            "LIST",
            tbl([("mod", Value::Mod(Rc::new(inner)))]),
            &[skill_tag()],
        )?);
    }
    Ok(mod_list(mods))
}

// --- ModParser.lua:2073-2106 extraSupport ---------------------------------------------------------------------------

/// `extraSupport(name, level, slot, appliesToGrantedSkills)`: zero returns (nil) for an unknown skill.
pub fn extra_support(d: &ModParserData, name: &Value, level: &Value, slot: &Value, applies: &Value) -> Result<Value, ParseError> {
    let name = lua_string(name, "local 'name'")?;
    // `gemIdLookup[name:gsub(...)]`: an index expression keeps the first return only
    let lookup = |k: &str| d.gem_id_lookup.get(k).cloned();
    let skill_id = match lookup(&name) {
        Some(s) => Some(s),
        None => match lookup(&gsub_str(&name, "^increased ", "")?) {
            Some(s) => Some(s),
            None => lookup(&gsub_str(&name, " support$", "")?),
        },
    };
    let slot = match slot {
        Value::Str(s) if &**s == "main hand" || &**s == "main hand weapon" => "Weapon 1".to_owned(),
        Value::Str(s) if &**s == "off hand" || &**s == "off hand weapon" => "Weapon 2".to_owned(),
        v if v.truthy() => {
            let up = gsub_upper(&format!(" {}", lua_string(v, "local 'slot'")?), "%W%l")?;
            up.get(1..).unwrap_or_default().to_owned()
        }
        _ => "{SlotName}".to_owned(),
    };
    let slot_tag = tbl([("type", Value::str("SocketedIn")), ("slotName", Value::str(&slot))]);
    let level = tonumber(level);
    let Some(skill_id) = skill_id else { return Ok(Value::Nil) };
    let skills = pob_data::mod_parser::skill_briefs();
    let gems = pob_data::gems::gems();
    let Some(skill) = skills.get(&*skill_id) else {
        return Err(ParseError::Lua(LuaError("attempt to index a nil value".into())));
    };
    let Some(skill_name) = &skill.name else {
        return Err(ParseError::Lua(LuaError(
            "attempt to concatenate field 'name' (a nil value)".into(),
        )));
    };
    let support = |id: Value| {
        tbl([
            ("skillId", id),
            ("level", level.clone()),
            ("appliesToGrantedSkills", applies.clone()),
        ])
    };
    let key = pob_lua::string::lower(&format!("{skill_name} Support"));
    let Some(gem_id) = gems.gem_for_base_name.get(&key) else {
        return Ok(mod_list(vec![mk(
            "ExtraSupport",
            "LIST",
            support(Value::Str(skill_id)),
            &[slot_tag],
        )?]));
    };
    let gem = &gems.gems[gem_id.as_str()];
    let mut mods = vec![mk(
        "ExtraSupport",
        "LIST",
        support(Value::str(&gem.granted_effect_id)),
        std::slice::from_ref(&slot_tag),
    )?];
    if let Some(sec) = &gem.secondary_granted_effect_id
        && let Some(sec_skill) = skills.get(sec)
    {
        // `data.gems[gemId].secondaryGrantedEffect` = data.skills[secondaryGrantedEffectId] (Data.lua)
        if sec_skill.support == Some(true) {
            mods.push(mk("ExtraSupport", "LIST", support(Value::str(sec)), &[slot_tag])?);
        } else {
            mods.push(mk(
                "ExtraSkill",
                "LIST",
                tbl([("skillId", Value::str(sec)), ("level", level.clone())]),
                &[],
            )?);
        }
    }
    Ok(mod_list(mods))
}

// --- ModParser.lua:18-39 getSimpleConv (radius jewels; the visitor is R5.4's caller) --------------------------------

/// A node a radius-jewel visitor looks at (`node.modList`, `node.isTattoo`, `node.type`, `node.conqueredBy`): the tree
/// crate implements it (R5.5).
pub trait JewelNode {
    fn mod_list(&self) -> &[Rc<Mod>];
    fn is_tattoo(&self) -> bool;
    /// `node.type` ("Normal", "Notable", "Keystone", "Socket", "ClassStart", "Mastery", ...).
    fn node_type(&self) -> &str;
    /// `node.conqueredBy` is truthy (a timeless jewel took the node).
    fn conquered_by(&self) -> bool;
    /// `node.modList:Sum("BASE", nil, stat)`.
    fn sum_base(&self, stat: &str) -> Result<f64, LuaError>;
}

/// `getSimpleConv(srcList, dst, type, remove, factor, srcType)` -- the upvalues of the visitor at ModParser.lua:19.
#[derive(Clone, Debug)]
pub struct SimpleConv {
    pub src_list: Vec<Str>,
    pub dst: Str,
    pub ty: Str,
    pub remove: bool,
    pub factor: Option<f64>,
    pub src_type: Option<Str>,
}

pub fn get_simple_conv(src_list: &[&str], dst: &str, ty: &str, remove: bool, factor: Option<f64>, src_type: Option<&str>) -> SimpleConv {
    SimpleConv {
        src_list: src_list.iter().map(|s| Str::from(*s)).collect(),
        dst: Str::from(dst),
        ty: Str::from(ty),
        remove,
        factor,
        src_type: src_type.map(Str::from),
    }
}

impl SimpleConv {
    /// The visitor ModParser.lua:19-38 `function(node, out, data)` (`data` unused). `out` is a ModList (`MergeNewMod`).
    pub fn visit(&self, node: Option<&dyn JewelNode>, out: &mut ModStore) -> Result<(), ParseError> {
        let Some(node) = node else { return Ok(()) };
        let attributes = |s: &str| matches!(s, "Dex" | "Int" | "Str");
        for src in &self.src_list {
            for m in node.mod_list() {
                let ty_s = m.ty.map(ModType::as_str);
                let type_ok = self.src_type.as_deref().is_some_and(|st| ty_s == Some(st)) || ty_s == Some(&*self.ty);
                if m.name == *src && type_ok && !(node.is_tattoo() && attributes(src)) {
                    let num = m.value.as_num().ok_or_else(|| {
                        ParseError::Lua(LuaError(format!(
                            "attempt to perform arithmetic on field 'value' (a {} value)",
                            m.value.type_name()
                        )))
                    })?;
                    let merge = |out: &mut ModStore, name: &str, v: f64| -> Result<(), ParseError> {
                        let mut va = vec![
                            Value::from(m.source.as_ref()),
                            Value::Num(f64::from(m.flags)),
                            Value::Num(f64::from(m.keyword_flags)),
                        ];
                        va.extend(m.tags.iter().map(|t| Value::Table(t.table())));
                        let nm = create_mod_lua(&Value::str(name), &Value::Str(self.ty.clone()), Value::Num(v), &va)?;
                        out.merge_mod(Rc::new(nm), false).map_err(ParseError::Lua)
                    };
                    if self.remove {
                        merge(out, src, -num)?;
                    }
                    match self.factor {
                        Some(f) => merge(out, &self.dst, (num * f).floor())?,
                        None => merge(out, &self.dst, num)?,
                    }
                }
            }
        }
        Ok(())
    }
}

// --- ModParser.lua:6455-6479 getThreshold ---------------------------------------------------------------------------

/// `getThreshold(attrib, name, modType, value, ...)`: `baseMod = mod(name, modType, value, "", ...)` plus the upvalues of
/// the visitor at :6457. `value` is the SAME Lua table as `baseMod.value`, and the visitor writes `value.mod.source`
/// after copying `baseMod` -- so a copy carries the source of the previous visit (bug-compat; the cell is shared).
#[derive(Debug)]
pub struct Threshold {
    /// A string, or a list of strings (`type(attrib) == "table"`).
    pub attrib: Value,
    pub base_mod: Mod,
    pub value: RefCell<Value>,
}

pub fn get_threshold(attrib: Value, name: &str, ty: &str, value: Value, va: &[Value]) -> Result<Threshold, ParseError> {
    let mut args = vec![Value::str("")];
    args.extend_from_slice(va);
    let base_mod = mk(name, ty, value.clone(), &args)?;
    Ok(Threshold {
        attrib,
        base_mod,
        value: RefCell::new(value),
    })
}

impl Threshold {
    /// The upvalues the data carries for a visitor at :6457 (`attrib`, `baseMod`, `value`).
    pub fn from_up(f: &FnRef) -> Result<Threshold, ParseError> {
        let lua = |k: &str| f.up.get(k).and_then(PVal::as_lua).cloned().unwrap_or_default();
        let Value::Mod(base_mod) = lua("baseMod") else {
            return Err(ParseError::Unrepresentable(format!("{}: baseMod upvalue", f.id)));
        };
        Ok(Threshold {
            attrib: lua("attrib"),
            base_mod: (*base_mod).clone(),
            value: RefCell::new(lua("value")),
        })
    }

    /// The visitor ModParser.lua:6457-6478 `function(node, out, data)`; `data` is the per-jewel Lua table.
    pub fn visit(&self, node: bool, out: &mut ModStore, data: &mut Table) -> Result<(), ParseError> {
        let get = |data: &Table, k: &str| data.get(k).as_num().unwrap_or(0.0);
        if node {
            let atts: Vec<Value> = match &self.attrib {
                Value::Table(t) => t.array.clone(),
                a => vec![a.clone()],
            };
            for att in atts {
                let a = lua_string(&att, "local 'att'")?;
                let node_val = out.sum(ModType::Base, None, &[&a]).map_err(ParseError::Lua)?;
                let cur = get(data, &a);
                data.hash.insert(Str::from(a.as_str()), Value::Num(cur + node_val));
                let total = get(data, "total");
                data.hash.insert(Str::from("total"), Value::Num(total + node_val));
            }
        } else if get(data, "total") >= 40.0 {
            let src = data.get("modSource").clone();
            let mut m = self.base_mod.deep_copy();
            m.value = crate::copy_value(&self.value.borrow());
            m.source = match &src {
                Value::Nil => None,
                Value::Str(s) => Some(s.clone()),
                other => return Err(ParseError::Unrepresentable(format!("modSource {}", other.type_name()))),
            };
            let mut v = self.value.borrow_mut();
            if let Value::Table(t) = &mut *v
                && let Value::Mod(inner) = t.get("mod").clone()
            {
                let mut im = (*inner).clone();
                im.source = m.source.clone();
                Rc::make_mut(t).hash.insert(Str::from("mod"), Value::Mod(Rc::new(im)));
            }
            out.add_mod(Rc::new(m));
        }
        Ok(())
    }
}

// --- Lua operators for the generated closures (closures_gen.rs, tools/modparser-gen) --------------------------------
//
// `what` is how LuaJIT names an operand in an error ("local 'num'", "upvalue 'x'", "field 'k'", or "" for a temporary).

fn lua_err(s: String) -> ParseError {
    ParseError::Lua(LuaError(s))
}

fn operand(what: &str, v: &Value) -> String {
    if what.is_empty() {
        format!("a {} value", v.type_name())
    } else {
        format!("{what} (a {} value)", v.type_name())
    }
}

/// Positional argument `i` (nil when absent -- a Lua parameter without an argument).
pub fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

/// `...` of a function with `n` named parameters.
pub fn rest(a: &[Value], n: usize) -> Vec<Value> {
    a.get(n..).map(<[Value]>::to_vec).unwrap_or_default()
}

/// A `return` list as the two values `parseMod` keeps (L11: missing values are nil).
pub fn ret2(v: Vec<Value>) -> (Value, Value) {
    let mut it = v.into_iter();
    (it.next().unwrap_or_default(), it.next().unwrap_or_default())
}

/// An upvalue holding a plain Lua value (nil when this reference does not carry it).
pub fn upv(f: &FnRef, name: &str) -> Result<Value, ParseError> {
    match f.up.get(name) {
        None => Ok(Value::Nil),
        Some(PVal::Lua(v)) => Ok(v.clone()),
        Some(_) => Err(ParseError::Unrepresentable(format!(
            "{}: upvalue '{name}' is not a plain value",
            f.id
        ))),
    }
}

/// An upvalue holding a Lua function (a generated helper such as `explodeFunc`).
pub fn up_fn<'a>(f: &'a FnRef, name: &str) -> Result<&'a FnRef, ParseError> {
    match f.up.get(name) {
        Some(PVal::Fn(g)) => Ok(g),
        _ => Err(ParseError::Unrepresentable(format!("{}: upvalue '{name}' is not a function", f.id))),
    }
}

/// The number an arithmetic operand converts to (L2: numeric strings convert).
fn arith_num(v: &Value) -> Option<f64> {
    match v {
        Value::Num(n) => Some(*n),
        Value::Str(s) => pob_lua::tonumber(s),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

/// `a <op> b` on doubles in Lua order; LuaJIT names the first operand that is not a number.
pub fn arith(op: Arith, a: &Value, b: &Value, wa: &str, wb: &str) -> Result<Value, ParseError> {
    let (Some(x), Some(y)) = (arith_num(a), arith_num(b)) else {
        let (v, w) = if arith_num(a).is_none() { (a, wa) } else { (b, wb) };
        return Err(lua_err(format!("attempt to perform arithmetic on {}", operand(w, v))));
    };
    Ok(Value::Num(match op {
        Arith::Add => x + y,
        Arith::Sub => x - y,
        Arith::Mul => x * y,
        Arith::Div => x / y,
        Arith::Mod => pob_lua::number::modulo(x, y),
        Arith::Pow => x.powf(y),
    }))
}

/// Unary `-v`.
pub fn neg_v(v: &Value, what: &str) -> Result<Value, ParseError> {
    match arith_num(v) {
        Some(n) => Ok(Value::Num(-n)),
        None => Err(lua_err(format!("attempt to perform arithmetic on {}", operand(what, v)))),
    }
}

/// `#v`: a string's byte length, a table's array part.
pub fn len_v(v: &Value, what: &str) -> Result<Value, ParseError> {
    match v {
        Value::Str(s) => Ok(Value::Num(s.len() as f64)),
        Value::Table(t) => Ok(Value::Num(t.array.iter().rposition(|x| !x.is_nil()).map_or(0, |i| i + 1) as f64)),
        Value::Mod(m) => Ok(Value::Num(m.tags.len() as f64)),
        _ => Err(lua_err(format!("attempt to get length of {}", operand(what, v)))),
    }
}

/// `a .. b` (L1: a number is `tostring`ed, `%.14g`).
pub fn concat_v(a: &Value, b: &Value, wa: &str, wb: &str) -> Result<Value, ParseError> {
    let s = |v: &Value| match v {
        Value::Str(s) => Some(s.to_string()),
        Value::Num(n) => Some(pob_lua::tostring_num(*n)),
        _ => None,
    };
    match (s(a), s(b)) {
        (Some(x), Some(y)) => Ok(Value::str(&(x + &y))),
        (None, _) => Err(lua_err(format!("attempt to concatenate {}", operand(wa, a)))),
        (_, None) => Err(lua_err(format!("attempt to concatenate {}", operand(wb, b)))),
    }
}

/// `a == b`.
pub fn lua_eq(a: &Value, b: &Value) -> bool {
    a.raw_eq(b)
}

fn compare(a: &Value, b: &Value, le: bool) -> Result<bool, ParseError> {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => Ok(if le { x <= y } else { x < y }),
        (Value::Str(x), Value::Str(y)) => Ok(if le {
            x.as_bytes() <= y.as_bytes()
        } else {
            x.as_bytes() < y.as_bytes()
        }),
        _ => Err(lua_err(format!("attempt to compare {} with {}", a.type_name(), b.type_name()))),
    }
}

/// `a < b` (numbers, or strings byte-wise -- C locale).
pub fn lt_v(a: &Value, b: &Value) -> Result<bool, ParseError> {
    compare(a, b, false)
}

/// `a <= b`.
pub fn le_v(a: &Value, b: &Value) -> Result<bool, ParseError> {
    compare(a, b, true)
}

/// `o[k]` / `o.k` as Lua indexes it.
pub fn index_v(o: &Value, k: &Value, what: &str) -> Result<Value, ParseError> {
    match o {
        Value::Table(t) => Ok(t.index(k)),
        Value::Mod(_) => Ok(o.index(k)),
        Value::Str(_) => match k {
            Value::Str(key) => string_lib_field(key),
            _ => Ok(Value::Nil),
        },
        _ => Err(lua_err(format!("attempt to index {}", operand(what, o)))),
    }
}

/// `t[k] = v` needs a key: nil and NaN raise (the table itself is not observable -- tools/modparser-gen checks).
pub fn table_key_check(k: &Value) -> Result<(), ParseError> {
    match k {
        Value::Nil => Err(lua_err("table index is nil".into())),
        Value::Num(n) if n.is_nan() => Err(lua_err("table index is NaN".into())),
        _ => Ok(()),
    }
}

/// The receiver of a string method (`s:gsub(...)`): numbers have no methods in LuaJIT.
pub fn self_str(v: &Value, what: &str) -> Result<Str, ParseError> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        _ => Err(lua_err(format!("attempt to index {}", operand(what, v)))),
    }
}

/// A string argument of a `string.*` function (`luaL_checklstring`: numbers convert).
pub fn check_str(v: &Value, n: usize, fname: &str) -> Result<Str, ParseError> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        Value::Num(x) => Ok(Str::from(pob_lua::tostring_num(*x).as_str())),
        _ => Err(lua_err(format!(
            "bad argument #{n} to '{fname}' (string expected, got {})",
            v.type_name()
        ))),
    }
}

/// An integer argument (`luaL_checkint`: numbers and numeric strings, truncated).
fn check_int(v: &Value, n: usize, fname: &str, default: Option<i64>) -> Result<i64, ParseError> {
    match (v, default) {
        (Value::Nil, Some(d)) => Ok(d),
        _ => match arith_num(v) {
            Some(x) => Ok(i64::from(x as i32)),
            None => Err(lua_err(format!(
                "bad argument #{n} to '{fname}' (number expected, got {})",
                v.type_name()
            ))),
        },
    }
}

/// The replacement of a generated `gsub`: `string.upper` or a literal string.
#[derive(Clone, Copy, Debug)]
pub enum GsubRepl {
    Upper,
    Str(&'static str),
}

/// `s:gsub(pattern, repl)`: both returns (the string and the count).
pub fn gsub_v(s: &str, pattern: &str, repl: GsubRepl) -> Result<(Value, Value), ParseError> {
    let (out, n) = match repl {
        GsubRepl::Upper => {
            // `string.upper` gets the first capture (the whole match without captures); the generator only emits
            // patterns without position captures
            let mut f =
                |subject: &[u8], caps: &pob_lua::Captures| caps.get(0).and_then(|c| c.bytes(subject)).map(<[u8]>::to_ascii_uppercase);
            pob_lua::gsub(s.as_bytes(), pattern.as_bytes(), pob_lua::Repl::Fn(&mut f), None)
        }
        GsubRepl::Str(r) => pob_lua::gsub(s.as_bytes(), pattern.as_bytes(), pob_lua::Repl::Str(r.as_bytes()), None),
    }
    .map_err(|e| lua_err(e.0))?;
    let out = String::from_utf8(out).map_err(|_| ParseError::Unrepresentable("gsub produced a non-UTF-8 string".into()))?;
    Ok((Value::str(&out), Value::Num(n as f64)))
}

/// `s:sub(i[, j])` (Lua 5.1 `str_sub`: negative positions count from the end).
pub fn str_sub(s: &str, i: &Value, j: &Value) -> Result<Value, ParseError> {
    let len = s.len() as i64;
    let rel = |p: i64| if p < 0 { len + p + 1 } else { p };
    let start = rel(check_int(i, 1, "sub", None)?).max(1);
    let end = rel(check_int(j, 2, "sub", Some(-1))?).min(len);
    if start > end {
        return Ok(Value::str(""));
    }
    let b = &s.as_bytes()[(start - 1) as usize..end as usize];
    let out = std::str::from_utf8(b).map_err(|_| ParseError::Unrepresentable("string.sub split a UTF-8 sequence".into()))?;
    Ok(Value::str(out))
}

fn check_num(v: &Value, n: usize, fname: &str) -> Result<f64, ParseError> {
    arith_num(v).ok_or_else(|| {
        let got = if v.is_nil() { "no value" } else { v.type_name() };
        lua_err(format!("bad argument #{n} to '{fname}' (number expected, got {got})"))
    })
}

/// `math.floor(x)`.
pub fn math_floor(v: &Value) -> Result<Value, ParseError> {
    Ok(Value::Num(check_num(v, 1, "floor")?.floor()))
}

fn nums(a: &[Value], fname: &str) -> Result<(f64, Vec<f64>), ParseError> {
    let first = check_num(a.first().unwrap_or(&Value::Nil), 1, fname)?;
    let rest = a
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, v)| check_num(v, i + 1, fname))
        .collect::<Result<_, _>>()?;
    Ok((first, rest))
}

/// `math.min(...)` (L7: the SSE fold).
pub fn math_min(a: &[Value]) -> Result<Value, ParseError> {
    let (first, rest) = nums(a, "min")?;
    Ok(Value::Num(pob_lua::number::min(first, &rest)))
}

/// `math.max(...)` (L7).
pub fn math_max(a: &[Value]) -> Result<Value, ParseError> {
    let (first, rest) = nums(a, "max")?;
    Ok(Value::Num(pob_lua::number::max(first, &rest)))
}

/// `bit.bor(...)`.
pub fn bor_v(a: &[Value]) -> Result<Value, ParseError> {
    let (first, rest) = nums(a, "bor")?;
    let init = f64::from(pob_lua::number::tobit(first));
    Ok(Value::Num(rest.iter().fold(init, |x, &y| pob_lua::number::bor(x, y))))
}

/// The three flag tables of Data/Global.lua the closures read as globals.
#[derive(Clone, Copy, Debug)]
pub enum GlobalFlags {
    ModFlag,
    KeywordFlag,
    SkillType,
}

/// `ModFlag.X` / `KeywordFlag.X` / `SkillType.X` (nil for a name the table does not have).
pub fn global_flag(t: GlobalFlags, key: &str) -> Value {
    let g = pob_data::globals::globals();
    let m = match t {
        GlobalFlags::ModFlag => &g.mod_flag,
        GlobalFlags::KeywordFlag => &g.keyword_flag,
        GlobalFlags::SkillType => &g.skill_type,
    };
    m.get(key).map_or(Value::Nil, |n| Value::Num(*n))
}

/// `ModFlag[k]` with a computed key.
pub fn global_flag_v(t: GlobalFlags, k: &Value) -> Value {
    match k {
        Value::Str(s) => global_flag(t, s),
        _ => Value::Nil,
    }
}

fn pval_lua(v: Option<&PVal>, what: &str) -> Result<Value, ParseError> {
    match v {
        None => Ok(Value::Nil),
        Some(PVal::Lua(x)) => Ok(x.clone()),
        Some(_) => Err(ParseError::Unrepresentable(format!("{what}: a closure-holding value"))),
    }
}

/// `gemIdLookup[k]` (a module-local table the closures capture).
pub fn named_gem_id_lookup(d: &ModParserData, k: &Value) -> Result<Value, ParseError> {
    Ok(match k {
        Value::Str(s) => d.gem_id_lookup.get(&**s).map_or(Value::Nil, |x| Value::Str(x.clone())),
        _ => Value::Nil,
    })
}

/// `conquerorList[k]`.
pub fn named_conqueror_list(d: &ModParserData, k: &Value) -> Result<Value, ParseError> {
    match k {
        Value::Str(s) => pval_lua(d.conqueror_list.get(s), "conquerorList"),
        _ => Ok(Value::Nil),
    }
}

/// `dmgTypes[k]`.
pub fn named_dmg_types(d: &ModParserData, k: &Value) -> Result<Value, ParseError> {
    match k {
        Value::Str(s) => pval_lua(d.table("dmgTypes").get(s), "dmgTypes"),
        _ => Ok(Value::Nil),
    }
}

/// A sequence constructor `{ a, b, ... }` (trailing nils are not part of the array).
pub fn seq(mut v: Vec<Value>) -> Value {
    while v.last().is_some_and(Value::is_nil) {
        v.pop();
    }
    Value::table(Table {
        array: v,
        ..Table::default()
    })
}

/// A constructor mixing positional and named fields, filled in source order.
#[derive(Default)]
pub struct TableBuilder(Table);

impl TableBuilder {
    pub fn push(&mut self, v: Value) {
        self.0.array.push(v);
    }

    pub fn extend(&mut self, v: Vec<Value>) {
        self.0.array.extend(v);
    }

    pub fn key(&mut self, k: &str, v: Value) {
        if v.is_nil() {
            self.0.hash.shift_remove(k);
        } else {
            self.0.hash.insert(Str::from(k), v);
        }
    }

    pub fn build(self) -> Value {
        let mut t = self.0;
        while t.array.last().is_some_and(Value::is_nil) {
            t.array.pop();
        }
        Value::table(t)
    }
}

/// `mod(name, type, value, ...)` (ModTools.lua:33 with the vararg heuristic) as a Lua value.
pub fn call_mod(mut a: Vec<Value>) -> Result<Value, ParseError> {
    if a.len() < 3 {
        a.resize(3, Value::Nil);
    }
    let value = std::mem::take(&mut a[2]);
    Ok(Value::Mod(Rc::new(create_mod_lua(&a[0], &a[1], value, &a[3..])?)))
}

/// `flag(name, ...)` (ModParser.lua:2012) as a Lua value.
pub fn call_flag(a: Vec<Value>) -> Result<Value, ParseError> {
    let name = a.first().cloned().unwrap_or_default();
    Ok(Value::Mod(Rc::new(flag(&name, a.get(1..).unwrap_or_default())?)))
}

/// `firstToUpper(str)` (ModParser.lua:13) on a Lua value: `str:gsub` needs a string.
pub fn first_to_upper_v(v: &Value) -> Result<Value, ParseError> {
    Ok(Value::str(&first_to_upper(&self_str(v, "local 'str'")?)))
}
