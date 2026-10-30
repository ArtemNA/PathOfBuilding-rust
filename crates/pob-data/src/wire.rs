//! The shapes the converter writes (`reference/pob-ts/WIRE-FORMAT.md`, MINEFIELD D3/D8/L9/L24) and their readers:
//! ordered maps, the Lua-table-as-array-or-object of D3, `{"$num": ...}` numbers, and mods (`pob_mods::Mod`).
//!
//! No `#[serde(untagged)]`/`flatten` anywhere: both buffer through serde's `Content`, and MINEFIELD-RUST R1 keeps every
//! number on the direct `serde_json` (`float_roundtrip`) path -- the readers below are visitors.

use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_mods::{Mod, ModType, Str, Table, Tag, Value, create_mod};
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value as Json;

/// A JSON object in file order (the converter writes keys sorted, D13; `pairs`-sensitive readers rely on it).
#[derive(Clone, Debug)]
pub struct OrderedMap<V>(pub IndexMap<String, V>);

impl<V> Default for OrderedMap<V> {
    fn default() -> Self {
        OrderedMap(IndexMap::new())
    }
}

impl<V> std::ops::Deref for OrderedMap<V> {
    type Target = IndexMap<String, V>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for OrderedMap<V> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V_<V>(PhantomData<V>);
        impl<'de, V: Deserialize<'de>> Visitor<'de> for V_<V> {
            type Value = OrderedMap<V>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON object (an empty Lua table may come as [])")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
                let mut out = IndexMap::with_capacity(m.size_hint().unwrap_or(0));
                while let Some((k, v)) = m.next_entry::<String, V>()? {
                    out.insert(k, v);
                }
                Ok(OrderedMap(out))
            }
            /// WIRE-FORMAT: an empty Lua table is written `[]` whatever it means.
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Self::Value, A::Error> {
                if s.next_element::<de::IgnoredAny>()?.is_some() {
                    return Err(de::Error::custom("expected an object, got a non-empty array"));
                }
                Ok(OrderedMap::default())
            }
        }
        d.deserialize_any(V_(PhantomData))
    }
}

/// A Lua table the serializer wrote as an array (keys exactly `1..n`) or an object (anything else, keys `tostring`ed) --
/// MINEFIELD D3; which one depends on the data version (D16: tree `groups`/`classes`).
#[derive(Clone, Debug)]
pub enum LuaTable<T> {
    Array(Vec<T>),
    Object(IndexMap<String, T>),
}

impl<T> Default for LuaTable<T> {
    fn default() -> Self {
        LuaTable::Array(Vec::new())
    }
}

impl<T> LuaTable<T> {
    /// `t[key]` for an integral Lua key: array index `key - 1`, or the object key `tostring(key)`.
    pub fn index(&self, key: i64) -> Option<&T> {
        match self {
            LuaTable::Array(v) => usize::try_from(key - 1).ok().and_then(|i| v.get(i)),
            LuaTable::Object(m) => m.get(key.to_string().as_str()),
        }
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        match self {
            LuaTable::Array(v) => v.len(),
            LuaTable::Object(m) => m.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_array(&self) -> bool {
        matches!(self, LuaTable::Array(_))
    }

    /// Every entry with its Lua key, ascending (a deterministic `pairs`, D13). Object keys that are not integers sort
    /// after the integers by byte order and carry `None`.
    pub fn entries(&self) -> Vec<(Option<i64>, &T)> {
        match self {
            LuaTable::Array(v) => v.iter().enumerate().map(|(i, x)| (Some(i as i64 + 1), x)).collect(),
            LuaTable::Object(m) => {
                let mut out: Vec<(Option<i64>, &str, &T)> = m.iter().map(|(k, v)| (k.parse::<i64>().ok(), k.as_str(), v)).collect();
                out.sort_by(|a, b| match (a.0, b.0) {
                    (Some(x), Some(y)) => x.cmp(&y),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => a.1.as_bytes().cmp(b.1.as_bytes()),
                });
                out.into_iter().map(|(k, _, v)| (k, v)).collect()
            }
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for LuaTable<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V_<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V_<T> {
            type Value = LuaTable<T>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a Lua table (array or object)")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Self::Value, A::Error> {
                let mut out = Vec::with_capacity(s.size_hint().unwrap_or(0));
                while let Some(x) = s.next_element()? {
                    out.push(x);
                }
                Ok(LuaTable::Array(out))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
                let mut out = IndexMap::with_capacity(m.size_hint().unwrap_or(0));
                while let Some((k, v)) = m.next_entry::<String, T>()? {
                    out.insert(k, v);
                }
                Ok(LuaTable::Object(out))
            }
        }
        d.deserialize_any(V_(PhantomData))
    }
}

/// A number field that may carry the wire marker `{"$num": "nan"|"inf"|"-inf"}` (WIRE-FORMAT, D8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Num(pub f64);

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V_;
        impl<'de> Visitor<'de> for V_ {
            type Value = Num;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number or {\"$num\": ...}")
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Num, E> {
                Ok(Num(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Num, E> {
                Ok(Num(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Num, E> {
                Ok(Num(v as f64))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Num, A::Error> {
                let Some((k, v)) = m.next_entry::<String, String>()? else {
                    return Err(de::Error::custom("empty object where a number was expected"));
                };
                if k != "$num" || m.next_key::<String>()?.is_some() {
                    return Err(de::Error::custom(
                        "an object other than {\"$num\": ...} where a number was expected",
                    ));
                }
                marker_num(&v)
                    .map(Num)
                    .ok_or_else(|| de::Error::custom(format!("unknown $num marker '{v}'")))
            }
        }
        d.deserialize_any(V_)
    }
}

fn marker_num(s: &str) -> Option<f64> {
    match s {
        "nan" => Some(f64::NAN),
        "inf" => Some(f64::INFINITY),
        "-inf" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

/// A JSON number, or a `$num` marker object, as `f64`.
pub fn json_num(j: &Json) -> Option<f64> {
    match j {
        Json::Number(n) => n.as_f64(),
        Json::Object(m) if m.len() == 1 => m.get("$num").and_then(Json::as_str).and_then(marker_num),
        _ => None,
    }
}

/// A scalar the data mixes (config `defaultState`, list `val`, ...): Lua number, string or boolean.
#[derive(Clone, Debug, PartialEq)]
pub enum Scalar {
    Num(f64),
    Str(String),
    Bool(bool),
}

impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V_;
        impl<'de> Visitor<'de> for V_ {
            type Value = Scalar;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a number, string or boolean")
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Scalar, E> {
                Ok(Scalar::Num(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Scalar, E> {
                Ok(Scalar::Num(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Scalar, E> {
                Ok(Scalar::Num(v as f64))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Scalar, E> {
                Ok(Scalar::Str(v.to_owned()))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Scalar, E> {
                Ok(Scalar::Bool(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, m: A) -> Result<Scalar, A::Error> {
                Num::deserialize(de::value::MapAccessDeserializer::new(m)).map(|n| Scalar::Num(n.0))
            }
        }
        d.deserialize_any(V_)
    }
}

// --- mods (pob-ts `Mod` JSON: name/type/value/flags/keywordFlags/source/tags + orphanTags/malformed) --------------------

/// A tag the converter moved out of `tags` because it sat after a hole in the Lua array part (L9): `ipairs` never reaches
/// it, so it is kept for the record and never applied.
#[derive(Clone, Debug)]
pub struct OrphanTag {
    /// The Lua array index it sat at (1-based, > `#tags + 1`).
    pub index: usize,
    pub tag: Tag,
}

/// A mod as the data carries it: the `pob_mods::Mod` plus the orphan tags kept beside it (R3-ARCH §2.1).
#[derive(Clone, Debug)]
pub struct DataMod {
    pub m: Mod,
    pub orphan_tags: Vec<OrphanTag>,
}

/// What reading one mod record gave.
pub enum ModRecord {
    Mod(DataMod),
    /// L24: an upstream call-site bug kept lossless by the converter (`malformed`); the loader drops it (inert, like P3).
    Malformed,
}

fn bad(what: &str, j: &Json) -> String {
    format!("{what}: unexpected {j}")
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

/// The JSON object of a mod nested in a value (`{ mod = modLib.createMod(...) }`): exactly the createMod fields.
fn is_mod_object(m: &serde_json::Map<String, Json>) -> bool {
    m.contains_key("name")
        && m.contains_key("type")
        && m.contains_key("flags")
        && m.contains_key("keywordFlags")
        && m.get("tags").is_some_and(Json::is_array)
        && m.keys()
            .all(|k| matches!(k.as_str(), "name" | "type" | "value" | "flags" | "keywordFlags" | "source" | "tags"))
}

/// A wire value as the Lua value it was: arrays are sequences, objects string-keyed tables, a nested createMod record a
/// `Value::Mod`, `$num` markers numbers. `null` (Lua nil) is only legal as a whole value (`createMod(name, type, nil)`).
pub fn json_to_value(j: &Json) -> Result<Value, String> {
    Ok(match j {
        Json::Null => Value::Nil,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(_) => Value::Num(json_num(j).ok_or_else(|| bad("number", j))?),
        Json::String(s) => Value::str(s),
        Json::Array(a) => Value::table(Table {
            array: a
                .iter()
                .map(|x| {
                    if x.is_null() {
                        Err("a null inside an array (a Lua hole)".to_owned())
                    } else {
                        json_to_value(x)
                    }
                })
                .collect::<Result<_, _>>()?,
            ..Table::default()
        }),
        Json::Object(m) => {
            if let Some(n) = json_num(j) {
                return Ok(Value::Num(n));
            }
            if is_mod_object(m) {
                return match json_to_mod(j)? {
                    ModRecord::Mod(dm) if dm.orphan_tags.is_empty() => Ok(Value::Mod(Rc::new(dm.m))),
                    _ => Err(bad("nested mod with orphanTags/malformed", j)),
                };
            }
            let mut t = Table::default();
            for (k, v) in m {
                // D3: an object key that is an integer was a Lua number key; the mod core's tables hold string keys only.
                if k.parse::<i64>().is_ok() {
                    return Err(format!(
                        "numeric key '{k}' in a mod value table (not representable in pob_mods::Table)"
                    ));
                }
                if v.is_null() {
                    continue;
                }
                t.hash.insert(Str::from(k.as_str()), json_to_value(v)?);
            }
            Value::table(t)
        }
    })
}

fn json_to_table(j: &Json, what: &str) -> Result<Table, String> {
    match json_to_value(j)? {
        Value::Table(t) => Ok(Rc::unwrap_or_clone(t)),
        _ => Err(bad(what, j)),
    }
}

/// One mod record -> `pob_mods::Mod` (`modLib.createMod(name, type, value, source, flags, keywordFlags, unpack(tags))`).
/// `type: null` is the L24 inert `type = nil`; an unknown type string is a data error.
pub fn json_to_mod(j: &Json) -> Result<ModRecord, String> {
    let Json::Object(m) = j else { return Err(bad("mod", j)) };
    if m.contains_key("malformed") {
        return Ok(ModRecord::Malformed);
    }
    let name = m.get("name").and_then(Json::as_str).ok_or_else(|| bad("mod.name", j))?;
    let ty = match m.get("type") {
        None | Some(Json::Null) => None,
        Some(Json::String(s)) => Some(ModType::parse(s).ok_or_else(|| format!("mod '{name}': unknown type '{s}'"))?),
        Some(t) => return Err(bad("mod.type", t)),
    };
    let value = match m.get("value") {
        None => Value::Nil,
        Some(v) => json_to_value(v)?,
    };
    let source = match m.get("source") {
        None | Some(Json::Null) => None,
        Some(Json::String(s)) => Some(s.as_str()),
        Some(s) => return Err(bad("mod.source", s)),
    };
    let tags = match m.get("tags") {
        None => Vec::new(),
        Some(Json::Array(a)) => a.iter().map(|t| json_to_table(t, "tag").map(Tag::new)).collect::<Result<_, _>>()?,
        Some(t) => return Err(bad("mod.tags", t)),
    };
    let orphan_tags = match m.get("orphanTags") {
        None => Vec::new(),
        Some(Json::Array(a)) => a
            .iter()
            .map(|o| {
                let index = o.get("index").and_then(Json::as_u64).ok_or_else(|| bad("orphanTag.index", o))?;
                let tag = json_to_table(o.get("tag").unwrap_or(&Json::Null), "orphanTag.tag")?;
                Ok(OrphanTag {
                    index: index as usize,
                    tag: Tag::new(tag),
                })
            })
            .collect::<Result<_, String>>()?,
        Some(t) => return Err(bad("mod.orphanTags", t)),
    };
    let flags = flags_u32(m.get("flags"), "mod.flags")?;
    let keyword_flags = flags_u32(m.get("keywordFlags"), "mod.keywordFlags")?;
    Ok(ModRecord::Mod(DataMod {
        m: create_mod(name, ty, value, source, flags, keyword_flags, tags),
        orphan_tags,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn num_markers_and_scalars() {
        let n: Num = serde_json::from_str(r#"{"$num":"-inf"}"#).unwrap();
        assert_eq!(n.0, f64::NEG_INFINITY);
        let n: Num = serde_json::from_str(r#"{"$num":"nan"}"#).unwrap();
        assert!(n.0.is_nan());
        let n: Num = serde_json::from_str("0.10000000000000001").unwrap();
        assert_eq!(n.0.to_bits(), 0.1f64.to_bits());
        assert!(serde_json::from_str::<Num>(r#"{"$num":"zero"}"#).is_err());
        let s: Vec<Scalar> = serde_json::from_str(r#"[1, "x", true, {"$num":"inf"}]"#).unwrap();
        assert_eq!(
            s,
            [
                Scalar::Num(1.0),
                Scalar::Str("x".into()),
                Scalar::Bool(true),
                Scalar::Num(f64::INFINITY)
            ]
        );
    }

    #[test]
    fn lua_table_both_shapes() {
        let a: LuaTable<i32> = serde_json::from_str("[10, 20]").unwrap();
        assert_eq!(a.index(2), Some(&20));
        assert_eq!(a.index(0), None);
        let o: LuaTable<i32> = serde_json::from_str(r#"{"10": 1, "2": 2, "x": 3}"#).unwrap();
        assert_eq!(o.index(2), Some(&2));
        let keys: Vec<Option<i64>> = o.entries().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, [Some(2), Some(10), None]);
        let e: OrderedMap<i32> = serde_json::from_str("[]").unwrap();
        assert!(e.is_empty());
    }

    #[test]
    fn mod_with_nested_mod_value() {
        let j: Json = serde_json::from_str(
            r#"{"name":"ExtraAura","type":"LIST","value":{"mod":{"name":"Life","type":"INC","value":5,"flags":0,"keywordFlags":0,"tags":[{"type":"Condition","var":"X"}]}},"flags":0,"keywordFlags":4,"tags":[]}"#,
        )
        .unwrap();
        let ModRecord::Mod(dm) = json_to_mod(&j).unwrap() else { panic!() };
        assert_eq!(dm.m.keyword_flags, 4);
        let Value::Table(t) = &dm.m.value else { panic!() };
        let Value::Mod(inner) = t.get("mod") else {
            panic!("nested createMod -> Value::Mod")
        };
        assert_eq!(inner.ty, Some(ModType::Inc));
        assert_eq!(inner.tags.len(), 1);
        let malformed: Json =
            serde_json::from_str(r#"{"name":"X","type":null,"value":0,"flags":0,"keywordFlags":0,"tags":[],"malformed":[]}"#).unwrap();
        assert!(matches!(json_to_mod(&malformed).unwrap(), ModRecord::Malformed));
    }
}
