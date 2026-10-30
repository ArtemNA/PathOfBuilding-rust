//! LuaJIT `tostring` for numbers (`%.14g`, MINEFIELD L1) and `string.format` (C printf semantics, L2).
//!
//! Digits come from Rust's exact float formatting (`{:.*e}` / `{:.*}`), which rounds the exact binary value
//! half-to-even on decimal ties -- the printf rule (`%.0f` of 2.5 = "2", `%.2f` of 0.125 = "0.12").
//! LuaJIT prints every NaN as "nan" (no sign, no `+`), infinities as "inf"/"-inf" (docker P3.6).

use crate::number::trunc_i64;
use crate::tonumber::tonumber;

/// A `string.format` / `tostring` argument: the Lua value kinds PoB passes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arg<'a> {
    Num(f64),
    Str(&'a str),
    Bool(bool),
    Nil,
}

fn non_finite(n: f64) -> Option<&'static str> {
    if n.is_nan() {
        Some("nan")
    } else if n == f64::INFINITY {
        Some("inf")
    } else if n == f64::NEG_INFINITY {
        Some("-inf")
    } else {
        None
    }
}

/// `n` (finite, nonzero) rounded to `p` significant digits: (digits without dot, decimal exponent X with 10^X <= |rounded| < 10^(X+1)).
/// Perf note (R1, 2026-10-31): std's exact formatter costs ~390 ns per %.14g vs LuaJIT ~62 ns; a "shortest digits first"
/// fast path gained only ~20 % on real stats and lost 50 % on 17-digit values, so it was dropped -- the fix is an own digit
/// generator (PLAN, R1-perf), verified by the same LuaJIT corpus.
fn sig_digits(n: f64, p: usize) -> (String, i32) {
    let s = format!("{:.*e}", p - 1, n.abs());
    let (mant, exp) = s.split_once('e').expect("exponential format has an 'e'");
    (mant.replace('.', ""), exp.parse().expect("exponent is an integer"))
}

/// C `printf("%.<precision>g", n)`: `precision` significant digits, exponential when X < -4 or X >= precision,
/// trailing zeros and a bare point removed, exponent with at least two digits; `precision < 1` means 1.
pub fn format_g(n: f64, precision: usize) -> String {
    if let Some(s) = non_finite(n) {
        return s.to_owned();
    }
    let p = precision.max(1);
    let sign = if n.is_sign_negative() { "-" } else { "" };
    if n == 0.0 {
        return format!("{sign}0");
    }
    let (ds, x) = sig_digits(n, p);
    if x < -4 || x >= p as i32 {
        let frac = ds[1..].trim_end_matches('0');
        let dot = if frac.is_empty() { "" } else { "." };
        let esign = if x < 0 { '-' } else { '+' };
        return format!("{sign}{}{dot}{frac}e{esign}{:02}", &ds[..1], x.unsigned_abs());
    }
    if x >= 0 {
        let int_len = x as usize + 1;
        let frac = ds[int_len..].trim_end_matches('0');
        let dot = if frac.is_empty() { "" } else { "." };
        return format!("{sign}{}{dot}{frac}", &ds[..int_len]);
    }
    let zeros = "0".repeat((-x - 1) as usize);
    let frac = format!("{zeros}{ds}");
    let frac = frac.trim_end_matches('0');
    format!("{sign}0.{frac}")
}

/// C `printf("%.<p>f", n)`.
pub fn format_f(n: f64, p: usize) -> String {
    match non_finite(n) {
        Some(s) => s.to_owned(),
        None => format!("{n:.p$}"),
    }
}

/// LuaJIT `tostring(n)` = `%.14g` (L1): 1e15 -> "1e+15", -0 -> "-0", 1/3 -> "0.33333333333333", NaN -> "nan".
pub fn tostring_num(n: f64) -> String {
    format_g(n, 14)
}

/// LuaJIT `tostring` of a scalar.
pub fn tostring(v: Arg<'_>) -> String {
    match v {
        Arg::Num(n) => tostring_num(n),
        Arg::Str(s) => s.to_owned(),
        Arg::Bool(b) => if b { "true" } else { "false" }.to_owned(),
        Arg::Nil => "nil".to_owned(),
    }
}

#[derive(Default)]
struct Flags {
    left: bool,
    plus: bool,
    space: bool,
    zero: bool,
}

fn pad_to(body: &str, sign: &str, f: &Flags, width: usize, allow_zero: bool) -> String {
    let len = sign.len() + body.len();
    if len >= width {
        format!("{sign}{body}")
    } else if f.left {
        format!("{sign}{body}{}", " ".repeat(width - len))
    } else if allow_zero && f.zero {
        format!("{sign}{}{body}", "0".repeat(width - len))
    } else {
        format!("{}{sign}{body}", " ".repeat(width - len))
    }
}

fn sign_for(neg: bool, f: &Flags) -> &'static str {
    if neg {
        "-"
    } else if f.plus {
        "+"
    } else if f.space {
        " "
    } else {
        ""
    }
}

/// Pads an already-signed numeric body: the `0` flag is ignored for inf/nan (`%05.1f` of inf = "  inf");
/// a NaN takes no `+` but does take the space flag (`% .2f` of nan = " nan").
fn pad_number(s: &str, f: &Flags, width: usize) -> String {
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, s),
    };
    let finite = body != "inf" && body != "nan";
    let sign = if body == "nan" {
        if f.space && !f.plus { " " } else { "" }
    } else {
        sign_for(neg, f)
    };
    pad_to(body, sign, f, width, finite)
}

fn arg_number(a: Arg<'_>, conv: char, index: usize) -> Result<f64, String> {
    match a {
        Arg::Num(n) => Ok(n),
        Arg::Str(s) => tonumber(s).ok_or_else(|| bad_arg(index, conv)),
        _ => Err(bad_arg(index, conv)),
    }
}

fn bad_arg(index: usize, conv: char) -> String {
    format!("bad argument #{index} to 'format' (number expected for %{conv})")
}

/// LuaJIT `string.format` for the specifiers PoB uses: `%d %i %s %f %g %x %X %%` with `-+ 0` flags, width, precision.
/// `%d`/`%x` cast through [`trunc_i64`]; `%s` uses [`tostring`]. Unknown specifiers and missing arguments are errors (Lua raises).
pub fn format(fmt: &str, args: &[Arg<'_>]) -> Result<String, String> {
    let b = fmt.as_bytes();
    let mut out = String::with_capacity(fmt.len() + 8);
    let mut next = 0usize;
    let mut i = 0usize;
    let mut lit = 0usize;
    while i < b.len() {
        if b[i] != b'%' {
            i += 1;
            continue;
        }
        out.push_str(&fmt[lit..i]);
        i += 1;
        if b.get(i) == Some(&b'%') {
            out.push('%');
            i += 1;
            lit = i;
            continue;
        }
        let mut f = Flags::default();
        while let Some(&c) = b.get(i) {
            match c {
                b'-' => f.left = true,
                b'+' => f.plus = true,
                b' ' => f.space = true,
                b'0' => f.zero = true,
                _ => break,
            }
            i += 1;
        }
        let mut width = 0usize;
        while let Some(&c) = b.get(i).filter(|c| c.is_ascii_digit()) {
            width = width * 10 + usize::from(c - b'0');
            i += 1;
        }
        let mut prec = None;
        if b.get(i) == Some(&b'.') {
            i += 1;
            let mut p = 0usize;
            while let Some(&c) = b.get(i).filter(|c| c.is_ascii_digit()) {
                p = p * 10 + usize::from(c - b'0');
                i += 1;
            }
            prec = Some(p);
        }
        let conv = b.get(i).map_or('\0', |&c| char::from(c));
        i += 1;
        lit = i;
        let index = next + 1;
        let arg = *args
            .get(next)
            .ok_or_else(|| format!("bad argument #{index} to 'format' (no value)"))?;
        next += 1;
        match conv {
            'd' | 'i' => {
                let v = trunc_i64(arg_number(arg, conv, index)?);
                out.push_str(&pad_to(&v.unsigned_abs().to_string(), sign_for(v < 0, &f), &f, width, true));
            }
            'x' | 'X' => {
                let v = trunc_i64(arg_number(arg, conv, index)?) as u64;
                let hex = if conv == 'x' { format!("{v:x}") } else { format!("{v:X}") };
                out.push_str(&pad_to(&hex, "", &f, width, true));
            }
            'f' => out.push_str(&pad_number(&format_f(arg_number(arg, conv, index)?, prec.unwrap_or(6)), &f, width)),
            'g' => out.push_str(&pad_number(&format_g(arg_number(arg, conv, index)?, prec.unwrap_or(6)), &f, width)),
            's' => {
                let s = tostring(arg);
                // Lua strings are bytes; PoB's formatted strings are ASCII, so a byte cut is a char cut here.
                let s = match prec {
                    Some(mut p) if p < s.len() => {
                        while !s.is_char_boundary(p) {
                            p -= 1;
                        }
                        &s[..p]
                    }
                    _ => s.as_str(),
                };
                out.push_str(&pad_to(s, "", &f, width, false));
            }
            _ => return Err(format!("invalid option '%{conv}' to 'format'")),
        }
    }
    out.push_str(&fmt[lit..]);
    Ok(out)
}
