//! LuaJIT `tostring` for numbers (`%.14g`, MINEFIELD L1) and `string.format` (C printf semantics, L2).
//!
//! The truth is LuaJIT's `lj_strfmt_num.c`, ported line for line in [`crate::lj_strfmt_num`]. It is not plain printf: an exact tie at 10^9
//! or above rounds up in `%g`, and `%g` with precision <= 14 outside [2^-63, 2^129) works on an inexact product.
//! The hot path here gives the port's answer by exact 128-bit integer arithmetic ([`round_scaled`]) and lays the text out in a stack
//! buffer; anywhere it cannot show that it agrees, it answers `None` / `false` and the port does the work. `tests/format_parity.rs`
//! compares the two on millions of values, and the port against both LuaJITs.
//! Perf (R1-perf): most of the pre-port cost was String shuffling around std's digits, not the digits themselves.

use crate::lj_strfmt_num::{self as lj, NO_PREC, Spec};
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

/// 10^k for the `u64` digit values: k <= 19.
const POW10_U64: [u64; 20] = {
    let mut t = [1u64; 20];
    let mut i = 1;
    while i < 20 {
        t[i] = t[i - 1] * 10;
        i += 1;
    }
    t
};

/// Largest decimal scale of [`round_scaled`]: a 53-bit mantissa times 10^22 (< 2^74) stays below 2^127.
const MAX_SCALE: i32 = 22;

const POW10_U128: [u128; MAX_SCALE as usize + 1] = {
    let mut t = [1u128; MAX_SCALE as usize + 1];
    let mut i = 1;
    while i <= MAX_SCALE as usize {
        t[i] = t[i - 1] * 10;
        i += 1;
    }
    t
};

/// Largest `%g` precision whose digits fit the `u64` path; wider ones go to the port.
const MAX_FAST_G: usize = 17;

/// See [`round_scaled`].
const NEAR_TIE_BITS: u32 = 24;

const DIGIT_PAIRS: &[u8; 200] = b"0001020304050607080910111213141516171819\
2021222324252627282930313233343536373839\
4041424344454647484950515253545556575859\
6061626364656667686970717273747576777879\
8081828384858687888990919293949596979899";

/// A stack buffer for one formatted number; the callers below never exceed it (see their length notes).
struct NumBuf {
    b: [u8; 32],
    len: usize,
}

impl NumBuf {
    fn new() -> Self {
        NumBuf { b: [0; 32], len: 0 }
    }
    fn push(&mut self, c: u8) {
        self.b[self.len] = c;
        self.len += 1;
    }
    fn extend(&mut self, s: &[u8]) {
        self.b[self.len..self.len + s.len()].copy_from_slice(s);
        self.len += s.len();
    }
    fn as_str(&self) -> &str {
        // Only ASCII is ever written; `unsafe` is forbidden in this workspace, so this stays a checked conversion.
        std::str::from_utf8(&self.b[..self.len]).expect("number text is ASCII")
    }
}

/// Fills `out` with the decimal digits of `d`, right-aligned and zero-padded; `d` must fit.
fn write_digits(out: &mut [u8], mut d: u64) {
    let mut i = out.len();
    while d >= 100 {
        let pair = (d % 100) as usize * 2;
        d /= 100;
        i -= 2;
        out[i..i + 2].copy_from_slice(&DIGIT_PAIRS[pair..pair + 2]);
    }
    if d >= 10 {
        i -= 2;
        out[i..i + 2].copy_from_slice(&DIGIT_PAIRS[d as usize * 2..d as usize * 2 + 2]);
    } else {
        i -= 1;
        out[i] = b'0' + d as u8;
    }
    out[..i].fill(b'0');
}

/// `abs` (finite, >= 0) as `m * 2^e` exactly.
fn decompose(abs: f64) -> (u64, i32) {
    let bits = abs.to_bits();
    let biased = (bits >> 52) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    if biased == 0 {
        (frac, -1074)
    } else {
        (frac | (1u64 << 52), biased - 1075)
    }
}

/// `m * 2^e * 10^s` (m != 0) rounded to an integer the way `nd_round` (lj_strfmt_num.c:248) does, in exact integer arithmetic; `None` when
/// it does not fit or, for `%g` (`g`), when `nd_div2k` might have cut off what decides the rounding.
fn round_scaled(m: u64, e: i32, s: i32, g: bool) -> Option<u128> {
    if s >= 0 {
        if s > MAX_SCALE {
            return None;
        }
        let v = u128::from(m) * POW10_U128[s as usize];
        if e >= 0 {
            return (e as u32 <= v.leading_zeros()).then(|| v << e);
        }
        let sh = e.unsigned_abs();
        if sh >= 128 {
            return None;
        }
        let q = v >> sh;
        let rem = v & ((1u128 << sh) - 1);
        let half = 1u128 << (sh - 1);
        // lj_strfmt_num.c:170: for %g the division keeps only some 9 digits past the last printed one, so a value just above a tie (a "5"
        // followed by that many zeros) can read as an exact tie. Leave the 2^-24 of a unit above the tie to the port. %f keeps every digit.
        if g && rem > half && sh > NEAR_TIE_BITS && (rem - half) >> (sh - NEAR_TIE_BITS) == 0 {
            return None;
        }
        return Some(q + u128::from(rem > half || (rem == half && q & 1 == 1)));
    }
    let k = s.unsigned_abs() as usize;
    if k >= POW10_U64.len() {
        return None;
    }
    let (num, den) = if e >= 0 {
        if e as u32 > m.leading_zeros() {
            return None;
        }
        (m << e, POW10_U64[k])
    } else {
        let sh = e.unsigned_abs();
        if sh > POW10_U64[k].leading_zeros() {
            return None;
        }
        (m, POW10_U64[k] << sh)
    };
    let (q, r) = (num / den, num % den);
    let rest = den - r;
    // lj_strfmt_num.c:271: the tie digit is at 10^(k-1); from 10^9 up it sits above limb `ndlo`, and an exact tie always rounds up.
    Some(u128::from(q + u64::from(r > rest || (r == rest && (q & 1 == 1 || k >= 10)))))
}

/// floor(log10(2^be)) for the binary exponents the integer path can reach (be in -63..=128, tested below): a lower bound of the
/// decimal exponent of any value in [2^be, 2^(be+1)), and at most one below it.
fn decimal_exponent_estimate(be: i32) -> i32 {
    (be * 78913) >> 18
}

/// `abs` (finite, nonzero) rounded to `p` <= [`MAX_FAST_G`] significant digits by integer arithmetic: (the `p` digits as a number,
/// decimal exponent X with 10^X <= rounded < 10^(X+1)); `None` outside the range the 128-bit product covers and outside the exact
/// conversion of LuaJIT (lj_strfmt_num.c:402: 2^-63 <= abs < 2^129; beyond it `%g` works on a rescaled, inexact value).
fn sig_digits_exact(abs: f64, p: usize) -> Option<(u64, i32)> {
    let (m, e) = decompose(abs);
    let be = e + 63 - m.leading_zeros() as i32;
    if !(-63..=128).contains(&be) {
        return None;
    }
    let limit = u128::from(POW10_U64[p]);
    let mut x = decimal_exponent_estimate(be);
    let mut d = round_scaled(m, e, p as i32 - 1 - x, true)?;
    if d >= limit {
        // The estimate was one low, or the rounding carried to 10^p; either way the next decade gives the digits.
        x += 1;
        d = round_scaled(m, e, p as i32 - 1 - x, true)?;
        if d >= limit {
            d /= 10;
            x += 1;
        }
    }
    debug_assert!(d >= limit / 10 && d < limit);
    Some((d as u64, x))
}

/// `%.<precision>g` of `n` by integer arithmetic into `out`; `false` (nothing usable written) when the port has to answer.
/// Longest text: "-d.ddddddddddddddddde-308" = 24 bytes.
fn write_g(out: &mut NumBuf, n: f64, precision: usize) -> bool {
    let p = precision.max(1);
    if !n.is_finite() || p > MAX_FAST_G {
        return false;
    }
    if n == 0.0 {
        out.extend(if n.is_sign_negative() { b"-0" } else { b"0" });
        return true;
    }
    let Some((d, x)) = sig_digits_exact(n.abs(), p) else {
        return false;
    };
    if n.is_sign_negative() {
        out.push(b'-');
    }
    let mut ds = [0u8; MAX_FAST_G];
    write_digits(&mut ds[..p], d);
    let ds = &ds[..p];
    let mut len = p;
    while len > 1 && ds[len - 1] == b'0' {
        len -= 1;
    }
    if x < -4 || x >= p as i32 {
        out.push(ds[0]);
        if len > 1 {
            out.push(b'.');
            out.extend(&ds[1..len]);
        }
        out.push(b'e');
        out.push(if x < 0 { b'-' } else { b'+' });
        let ax = x.unsigned_abs() as usize;
        if ax >= 100 {
            out.push(b'0' + (ax / 100) as u8);
        }
        out.extend(&DIGIT_PAIRS[ax % 100 * 2..ax % 100 * 2 + 2]);
    } else if x >= 0 {
        let int_len = x as usize + 1;
        out.extend(&ds[..int_len]);
        if len > int_len {
            out.push(b'.');
            out.extend(&ds[int_len..len]);
        }
    } else {
        out.extend(b"0.");
        for _ in 0..(-x - 1) {
            out.push(b'0');
        }
        out.extend(&ds[..len]);
    }
    true
}

/// `%.<p>f` by integer arithmetic into `out`; `false` (nothing usable written) when the port has to answer.
/// Longest text: a sign, 23 digits at `p` = 22 and the point.
fn write_f(out: &mut NumBuf, n: f64, p: usize) -> bool {
    if !n.is_finite() || p > MAX_SCALE as usize {
        return false;
    }
    let (m, e) = decompose(n.abs());
    let d = if m == 0 {
        0
    } else {
        match round_scaled(m, e, p as i32, false).map(u64::try_from) {
            Some(Ok(d)) => d,
            _ => return false,
        }
    };
    let mut digits = [0u8; 23];
    let total = d.checked_ilog10().map_or(0, |l| l as usize + 1).max(p + 1);
    write_digits(&mut digits[..total], d);
    if n.is_sign_negative() {
        out.push(b'-');
    }
    let int_len = total - p;
    out.extend(&digits[..int_len]);
    if p > 0 {
        out.push(b'.');
        out.extend(&digits[int_len..total]);
    }
    true
}

const fn plain(g: bool, prec: u32) -> Spec {
    Spec {
        g,
        left: false,
        plus: false,
        zero: false,
        space: false,
        width: 0,
        prec,
    }
}

/// `string.format("%.<precision>g", n)`.
pub fn format_g(n: f64, precision: usize) -> String {
    let mut b = NumBuf::new();
    if write_g(&mut b, n, precision) {
        return b.as_str().to_owned();
    }
    lj::to_string(&plain(true, precision as u32), n)
}

/// `string.format("%.<p>f", n)`.
pub fn format_f(n: f64, p: usize) -> String {
    let mut b = NumBuf::new();
    if write_f(&mut b, n, p) {
        return b.as_str().to_owned();
    }
    lj::to_string(&plain(false, p as u32), n)
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

fn pad_to(out: &mut String, body: &str, sign: &str, f: &Flags, width: usize, allow_zero: bool) {
    let fill = width.saturating_sub(sign.len() + body.len());
    if f.left {
        out.push_str(sign);
        out.push_str(body);
        out.extend(std::iter::repeat_n(' ', fill));
    } else if allow_zero && f.zero {
        out.push_str(sign);
        out.extend(std::iter::repeat_n('0', fill));
        out.push_str(body);
    } else {
        out.extend(std::iter::repeat_n(' ', fill));
        out.push_str(sign);
        out.push_str(body);
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

/// Pads the text of a finite number with its own `-`; the same layout as `lj_strfmt_wfnum` (lj_strfmt_num.c:577).
fn pad_number(out: &mut String, s: &str, f: &Flags, width: usize) {
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, s),
    };
    pad_to(out, body, sign_for(neg, f), f, width, true);
}

/// `%f` / `%g`: the hot path where it applies, the port otherwise.
fn format_float(out: &mut String, n: f64, g: bool, prec: Option<usize>, f: &Flags, width: usize) {
    let mut b = NumBuf::new();
    let p = prec.unwrap_or(6);
    // %g with a width stays with the port: after a carry lj_strfmt_num.c:485 reads a stale digit count, and the padding is computed from
    // the result (`%14.14g` of 9.999999999999999e17 is an unpadded "1e+18").
    if if g {
        width == 0 && write_g(&mut b, n, p)
    } else {
        write_f(&mut b, n, p)
    } {
        pad_number(out, b.as_str(), f, width);
        return;
    }
    let spec = Spec {
        g,
        left: f.left,
        plus: f.plus,
        zero: f.zero,
        space: f.space,
        width: width as u32,
        prec: prec.map_or(NO_PREC, |p| p as u32),
    };
    let mut text = Vec::with_capacity(32);
    lj::wfnum(&mut text, &spec, n);
    out.push_str(&String::from_utf8_lossy(&text));
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
    let mut out = String::with_capacity(fmt.len() + 16);
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
                pad_to(&mut out, &v.unsigned_abs().to_string(), sign_for(v < 0, &f), &f, width, true);
            }
            'x' | 'X' => {
                let v = trunc_i64(arg_number(arg, conv, index)?) as u64;
                let hex = if conv == 'x' { format!("{v:x}") } else { format!("{v:X}") };
                pad_to(&mut out, &hex, "", &f, width, true);
            }
            'f' | 'g' => {
                let n = arg_number(arg, conv, index)?;
                format_float(&mut out, n, conv == 'g', prec, &f, width);
            }
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
                pad_to(&mut out, s, "", &f, width, false);
            }
            _ => return Err(format!("invalid option '%{conv}' to 'format'")),
        }
    }
    out.push_str(&fmt[lit..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_exponent_estimate_is_the_floor_of_be_log10_2() {
        // round_scaled refuses anything further out: s <= 22 needs X >= -22, and k <= 19 needs X <= 35 at p = 17.
        for be in -200..=200 {
            let exact = (f64::from(be) * 2f64.log10()).floor() as i32;
            assert_eq!(decimal_exponent_estimate(be), exact, "be = {be}");
        }
    }

    /// `tests/format_parity.rs` would pass vacuously if everything fell through to std, so pin where the integer paths apply.
    #[test]
    fn integer_paths_cover_the_ranges_pob_formats() {
        for v in [1e-8, 1e-6, 0.1, 1.0, 1.5, 255.0, 1e13, 99999999999999.5, 1e14, 1e15, 1.8e19] {
            assert!(sig_digits_exact(v, 14).is_some(), "{v:e}");
        }
        for v in [1e-30, 5e-324, 1e20, 1e300] {
            assert!(sig_digits_exact(v, 14).is_none(), "{v:e}");
        }
        for (v, p) in [(0.0, 2), (1.5, 0), (1e-20, 2), (1e15, 2), (1.8e17, 2), (1.0, 19)] {
            assert!(write_f(&mut NumBuf::new(), v, p), "{v:e} at {p}");
        }
        for (v, p) in [(1e-30, 2), (1.9e17, 2), (1e300, 2), (1.5, 23)] {
            assert!(!write_f(&mut NumBuf::new(), v, p), "{v:e} at {p}");
        }
    }
}
