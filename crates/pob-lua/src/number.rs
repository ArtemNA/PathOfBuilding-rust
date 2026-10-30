//! LuaJIT number semantics PoB relies on (MOD-CORE §6, MINEFIELD L5-L7, L20). Doubles only; `-0` and NaN pass through as LuaJIT does.

/// `10 ^ dec` as LuaJIT computes it (`^` -> `pow`).
fn pow10(dec: f64) -> f64 {
    10f64.powf(dec)
}

/// Common.lua:709 `round(val, dec)`: `m_floor(val * 10 ^ dec + 0.5) / 10 ^ dec`, or `m_floor(val + 0.5)` without `dec`.
pub fn round(val: f64, dec: Option<f64>) -> f64 {
    match dec {
        Some(d) => (val * pow10(d) + 0.5).floor() / pow10(d),
        None => (val + 0.5).floor(),
    }
}

/// Common.lua:721 `floor(val, dec)`: `m_floor(val * mult + 0.0001) / mult` with `mult = 10 ^ dec`, or `m_floor(val)`.
pub fn floor(val: f64, dec: Option<f64>) -> f64 {
    match dec {
        Some(d) => {
            let mult = pow10(d);
            (val * mult + 0.0001).floor() / mult
        }
        None => val.floor(),
    }
}

/// `math.modf` (C modf): integer part truncated towards zero; a zero fractional part keeps the sign of `val`
/// (LuaJIT docker P3.6: modf(-3) = -3, -0; modf(-inf) = -inf, -0); NaN -> (nan, nan).
pub fn modf(val: f64) -> (f64, f64) {
    if val.is_nan() {
        return (val, val);
    }
    let zero = if val.is_sign_negative() { -0.0 } else { 0.0 };
    if val.is_infinite() {
        return (val, zero);
    }
    let ip = val.trunc();
    let fp = val - ip;
    (ip, if fp == 0.0 { zero } else { fp })
}

/// Lua 5.1 / LuaJIT `a % b` = `a - floor(a / b) * b` (L5), not fmod. `b == 0` gives NaN by itself.
pub fn modulo(a: f64, b: f64) -> f64 {
    a - (a / b).floor() * b
}

/// `math.min` = left fold of SSE `minsd`: `r = (r < x) ? r : x` -- on NaN or ±0 the RIGHT operand wins (L7).
pub fn min(first: f64, rest: &[f64]) -> f64 {
    rest.iter().fold(first, |r, &x| if r < x { r } else { x })
}

/// `math.max` = left fold of `maxsd`: `r = (r > x) ? r : x`.
pub fn max(first: f64, rest: &[f64]) -> f64 {
    rest.iter().fold(first, |r, &x| if r > x { r } else { x })
}

/// Two-argument `math.min(a, b)`.
pub fn min2(a: f64, b: f64) -> f64 {
    if a < b { a } else { b }
}

/// Two-argument `math.max(a, b)`.
pub fn max2(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}

/// LuaJIT's double -> int64 cast for `%d`/`%x` (x86-64 `cvttsd2si`): NaN and anything outside [-2^63, 2^63) give INT64_MIN.
pub fn trunc_i64(v: f64) -> i64 {
    // `as` saturates in Rust; the hardware "integer indefinite" value is INT64_MIN instead.
    if v.is_nan() || !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&v) {
        i64::MIN
    } else {
        v as i64
    }
}

/// LuaJIT `bit.tobit` normalisation: the double modulo 2^32 as a signed 32-bit integer.
pub fn tobit(v: f64) -> i32 {
    // lj_num2bit: adds 2^52 + 2^51 and reinterprets the low 32 bits -- equivalent to wrapping the truncated value.
    let t = v + 6_755_399_441_055_744.0;
    t.to_bits() as u32 as i32
}

/// `bit.band(a, b)` -- a signed 32-bit result as a double, like LuaJIT.
pub fn band(a: f64, b: f64) -> f64 {
    f64::from(tobit(a) & tobit(b))
}

/// `bit.bor(a, b)`.
pub fn bor(a: f64, b: f64) -> f64 {
    f64::from(tobit(a) | tobit(b))
}

/// `bit.bnot(a)`.
pub fn bnot(a: f64) -> f64 {
    f64::from(!tobit(a))
}

/// `bit.bxor(a, b)`.
pub fn bxor(a: f64, b: f64) -> f64 {
    f64::from(tobit(a) ^ tobit(b))
}

/// `bit.lshift(a, n)`: the shift count is `tobit(n) & 31`.
pub fn lshift(a: f64, n: f64) -> f64 {
    f64::from(((tobit(a) as u32) << (tobit(n) & 31)) as i32)
}

/// `bit.rshift(a, n)`: logical shift of the 32-bit pattern.
pub fn rshift(a: f64, n: f64) -> f64 {
    f64::from(((tobit(a) as u32) >> (tobit(n) & 31)) as i32)
}

/// `bit.arshift(a, n)`: arithmetic shift.
pub fn arshift(a: f64, n: f64) -> f64 {
    f64::from(tobit(a) >> (tobit(n) & 31))
}
