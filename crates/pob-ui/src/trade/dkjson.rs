//! `dkjson.encode` (runtime/lua/dkjson.lua) and `dkjson.decode` over `serde_json::Value`.
//! - a number is `tostring(num)` (`num2str`, :201-203), so the text carries 14 significant digits as the original sends;
//! - `dkjson.encode` of a table has no `keyorder`: it walks `pairs` (R13, of the process). Here the keys are in the map's order; the gates compare decoded values;
//! - an empty Lua table encodes as `[]` (`isarray` with n = 0): a table built here that is empty is `Value::Array(vec![])`, and only a decoded `{}` stays `{}` (dkjson keeps `__jsontype`).

use serde_json::Value;

/// `escapeutf8`'s two-character forms (:112-120).
fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn num(n: f64, out: &mut String) {
    if n.is_nan() || n.is_infinite() {
        // :346-349 "null"
        out.push_str("null");
    } else {
        out.push_str(&pob_lua::format::tostring_num(n));
    }
}

fn write(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => num(n.as_f64().unwrap_or(0.0), out),
        Value::String(s) => quote(s, out),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(x, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                quote(k, out);
                out.push(':');
                write(x, out);
            }
            out.push('}');
        }
    }
}

/// `dkjson.encode(value)`.
pub fn encode(v: &Value) -> String {
    let mut out = String::new();
    write(v, &mut out);
    out
}

/// `dkjson.decode(text)`: `None` is a nil (with an error message beside it). dkjson stops after the first value: what follows it is ignored.
pub fn decode(text: &str) -> Option<Value> {
    let mut d = serde_json::Deserializer::from_str(text).into_iter::<Value>();
    d.next()?.ok()
}

/// A number the way Lua keeps it: from a `f64`, with no integer of its own (`Value::from(f64)` is `null` on NaN: nothing here makes one).
pub fn n(v: f64) -> Value {
    serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_have_the_digits_of_tostring() {
        assert_eq!(encode(&n(0.1 + 0.2)), "0.3");
        assert_eq!(encode(&n(1.0)), "1");
        assert_eq!(encode(&n(123456.7890123456)), "123456.78901235");
        assert_eq!(encode(&n(-2.5e20)), "-2.5e+20");
    }

    #[test]
    fn tables_and_strings() {
        assert_eq!(encode(&json!([])), "[]");
        assert_eq!(
            encode(&json!({"a": [true, null], "b": "x\"\n"})),
            r#"{"a":[true,null],"b":"x\"\n"}"#
        );
    }

    #[test]
    fn decode_stops_after_the_value() {
        assert_eq!(decode("{\"a\":1} tail"), Some(json!({"a":1})));
        assert_eq!(decode("nope"), None);
        assert_eq!(decode(""), None);
    }
}
