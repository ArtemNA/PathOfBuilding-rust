//! Stat values: the scalar domain of `getStats()` (pob-ts `tools/oracle/format.ts`).
//!
//! A snapshot never carries NaN/Infinity as numbers: they are the canonical strings
//! `"nan"`, `"inf"`, `"-inf"` (MINEFIELD O2); the C spelling `"-nan"` folds into `"nan"`.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub const NAN: &str = "nan";
pub const INF: &str = "inf";
pub const NEG_INF: &str = "-inf";

/// `getStats()` bookkeeping key -- not a stat (MINEFIELD O7).
pub const META_KEY: &str = "_meta";

#[derive(Debug, Clone, PartialEq)]
pub enum StatValue {
    Num(f64),
    Str(String),
    Bool(bool),
}

/// Stats keyed in byte order (Rust `String` ordering = pob-ts `byteOrder`).
pub type Stats = BTreeMap<String, StatValue>;

impl StatValue {
    /// format.ts `normalizeStatValue`: non-finite numbers -> canonical strings, `"-nan"` -> `"nan"`.
    pub fn normalized(self) -> StatValue {
        match self {
            StatValue::Num(v) if v.is_nan() => StatValue::Str(NAN.to_owned()),
            StatValue::Num(v) if v == f64::INFINITY => StatValue::Str(INF.to_owned()),
            StatValue::Num(v) if v == f64::NEG_INFINITY => StatValue::Str(NEG_INF.to_owned()),
            StatValue::Str(s) if s == "-nan" => StatValue::Str(NAN.to_owned()),
            other => other,
        }
    }

    pub fn as_num(&self) -> Option<f64> {
        match self {
            StatValue::Num(v) => Some(*v),
            _ => None,
        }
    }
}

impl From<f64> for StatValue {
    fn from(v: f64) -> Self {
        StatValue::Num(v).normalized()
    }
}

impl From<bool> for StatValue {
    fn from(v: bool) -> Self {
        StatValue::Bool(v)
    }
}

impl From<&str> for StatValue {
    fn from(v: &str) -> Self {
        StatValue::Str(v.to_owned()).normalized()
    }
}

/// Drops `_meta` and normalizes every value (format.ts `normalizeStats`); `-0` is preserved.
pub fn normalize_stats(raw: impl IntoIterator<Item = (String, StatValue)>) -> Stats {
    raw.into_iter()
        .filter(|(k, _)| k != META_KEY)
        .map(|(k, v)| (k, v.normalized()))
        .collect()
}

impl fmt::Display for StatValue {
    /// Human-readable, for reports only (not byte-identical with JS `String(number)`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatValue::Num(v) if *v == 0.0 && v.is_sign_negative() => f.write_str("-0"),
            StatValue::Num(v) => write!(f, "{v:?}"),
            StatValue::Str(s) => write!(f, "{s:?}"),
            StatValue::Bool(b) => write!(f, "{b}"),
        }
    }
}

impl Serialize for StatValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.clone().normalized() {
            StatValue::Num(v) => s.serialize_f64(v),
            StatValue::Str(v) => s.serialize_str(&v),
            StatValue::Bool(v) => s.serialize_bool(v),
        }
    }
}

// A hand-written visitor instead of `#[serde(untagged)]`: untagged buffers values
// through serde's `Content`, and we want the f64 straight from serde_json's
// float_roundtrip parser (MINEFIELD-RUST R1).
impl<'de> Deserialize<'de> for StatValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = StatValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a number, string or boolean")
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<StatValue, E> {
                Ok(StatValue::Num(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<StatValue, E> {
                Ok(StatValue::Num(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<StatValue, E> {
                Ok(StatValue::Num(v as f64))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<StatValue, E> {
                Ok(StatValue::Bool(v))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<StatValue, E> {
                Ok(StatValue::Str(v.to_owned()))
            }
        }
        d.deserialize_any(V)
    }
}
