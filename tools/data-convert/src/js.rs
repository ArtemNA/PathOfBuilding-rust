//! The text form of today's data files: `JSON.stringify(value, null, 1) + "\n"` -- byte for byte, so a new converter can be
//! proven against the old one. Two JavaScript rules leak into the bytes and are reproduced here:
//! * object key order: array-index keys ("0", "17", ...) first in ascending numeric order, then the rest in insertion order;
//! * numbers: ECMAScript `Number::toString` (shortest round-trip digits; `1e+21`, `1e-7`, `0.000001`), non-finite -> `null`.

use std::fmt::Write;

#[derive(Clone, Debug)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    /// Insertion order; the writer applies the JS key order.
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn str(s: &str) -> J {
        J::Str(s.to_owned())
    }
}

/// `JSON.stringify(v, null, 1) + "\n"`.
pub fn stringify(v: &J) -> String {
    let mut out = String::with_capacity(1 << 20);
    write(&mut out, v, 0);
    out.push('\n');
    out
}

fn array_index(k: &str) -> Option<u32> {
    // A canonical uint32 below 2^32 - 1.
    if k.is_empty() || k.len() > 10 || !k.bytes().all(|b| b.is_ascii_digit()) || (k.len() > 1 && k.starts_with('0')) {
        return None;
    }
    let n: u64 = k.parse().ok()?;
    (n < u32::MAX as u64).then_some(n as u32)
}

fn indent(out: &mut String, n: usize) {
    out.push('\n');
    for _ in 0..n {
        out.push(' ');
    }
}

fn write(out: &mut String, v: &J, depth: usize) {
    match v {
        J::Null => out.push_str("null"),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Num(n) => number(out, *n),
        J::Str(s) => string(out, s),
        J::Arr(a) => {
            if a.is_empty() {
                return out.push_str("[]");
            }
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                indent(out, depth + 1);
                write(out, x, depth + 1);
            }
            indent(out, depth);
            out.push(']');
        }
        J::Obj(o) => {
            if o.is_empty() {
                return out.push_str("{}");
            }
            let mut order: Vec<usize> = (0..o.len()).collect();
            if o.iter().any(|(k, _)| array_index(k).is_some()) {
                let mut idx: Vec<(u32, usize)> = o.iter().enumerate().filter_map(|(i, (k, _))| array_index(k).map(|n| (n, i))).collect();
                idx.sort_unstable();
                order = idx.iter().map(|&(_, i)| i).chain((0..o.len()).filter(|&i| array_index(&o[i].0).is_none())).collect();
            }
            out.push('{');
            for (n, &i) in order.iter().enumerate() {
                if n > 0 {
                    out.push(',');
                }
                indent(out, depth + 1);
                string(out, &o[i].0);
                out.push_str(": ");
                write(out, &o[i].1, depth + 1);
            }
            indent(out, depth);
            out.push('}');
        }
    }
}

fn string(out: &mut String, s: &str) {
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
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// ECMAScript `Number::toString(10)`.
pub fn number(out: &mut String, x: f64) {
    if !x.is_finite() {
        return out.push_str("null");
    }
    if x == 0.0 {
        return out.push('0');
    }
    if x < 0.0 {
        out.push('-');
    }
    // Rust's `{:e}` is the shortest round-trip digit string: "d.ddde<exp>".
    let e = format!("{:e}", x.abs());
    let (mant, exp) = e.split_once('e').unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap() + 1; // value = 0.digits * 10^n
    if k <= n && n <= 21 {
        out.push_str(&digits);
        for _ in 0..n - k {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in 0..-n {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if n >= 1 { '+' } else { '-' });
        let _ = write!(out, "{}", (n - 1).abs());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(x: f64) -> String {
        let mut o = String::new();
        number(&mut o, x);
        o
    }
    #[test]
    fn js_numbers() {
        assert_eq!(s(5.0), "5");
        assert_eq!(s(-0.0), "0");
        assert_eq!(s(0.1), "0.1");
        assert_eq!(s(1.25), "1.25");
        assert_eq!(s(1e21), "1e+21");
        assert_eq!(s(123456789012345680000.0), "123456789012345680000");
        assert_eq!(s(1e-7), "1e-7");
        assert_eq!(s(0.000001), "0.000001");
        assert_eq!(s(1.5e-10), "1.5e-10");
        assert_eq!(s(f64::NAN), "null");
    }
    #[test]
    fn js_key_order() {
        let v = J::Obj(vec![("b".into(), J::Null), ("10".into(), J::Null), ("a".into(), J::Null), ("2".into(), J::Null), ("01".into(), J::Null)]);
        assert_eq!(stringify(&v), "{\n \"2\": null,\n \"10\": null,\n \"b\": null,\n \"a\": null,\n \"01\": null\n}\n");
    }
}
