//! LuaJIT `tonumber(s)` (lj_strscan, MINEFIELD L3): trims " \t\n\v\f\r", takes an optional sign, then
//! decimal (`5`, `5.`, `.5`, exponent), hex (`0x`, optional fraction and `p` exponent; a plain hex integer wraps
//! modulo 2^64), binary (`0b101`) or the words inf/infinity/nan in any case. Anything else is `None`.
//! Docker P3.6: "inf" -> inf, "0b101" -> 5, "1e999" -> inf, "0xffffffffffffffff" -> 1.8446744073709552e19.

const LUA_SPACE: &[u8] = b" \t\n\x0b\x0c\r";

pub fn tonumber(s: &str) -> Option<f64> {
    let t = trim(s.as_bytes());
    if t.is_empty() {
        return None;
    }
    let (neg, body) = match t[0] {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    let v = if let Some(w) = word(body) {
        w
    } else if body.len() > 1 && body[0] == b'0' && matches!(body[1], b'x' | b'X') {
        hex(&body[2..])?
    } else if body.len() > 1 && body[0] == b'0' && matches!(body[1], b'b' | b'B') {
        binary(&body[2..])?
    } else {
        decimal(body)?
    };
    Some(if neg { -v } else { v })
}

fn trim(b: &[u8]) -> &[u8] {
    let start = b.iter().position(|c| !LUA_SPACE.contains(c)).unwrap_or(b.len());
    let end = b.iter().rposition(|c| !LUA_SPACE.contains(c)).map_or(start, |i| i + 1);
    &b[start..end]
}

fn word(b: &[u8]) -> Option<f64> {
    if b.eq_ignore_ascii_case(b"inf") || b.eq_ignore_ascii_case(b"infinity") {
        Some(f64::INFINITY)
    } else if b.eq_ignore_ascii_case(b"nan") {
        Some(f64::NAN)
    } else {
        None
    }
}

/// `[01]+`, wrapping modulo 2^64, then converted to a double (round to nearest).
fn binary(b: &[u8]) -> Option<f64> {
    if b.is_empty() || !b.iter().all(|c| matches!(c, b'0' | b'1')) {
        return None;
    }
    let v = b.iter().fold(0u64, |acc, c| acc.wrapping_shl(1) | u64::from(c - b'0'));
    Some(v as f64)
}

/// Decimal grammar `(digits [. digits*] | . digits) [(e|E) [+-] digits]`, validated here, then converted by Rust's
/// correctly rounded parser (lj_strscan rounds correctly too; overflow -> inf, underflow -> 0).
fn decimal(b: &[u8]) -> Option<f64> {
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let int_len = digits(0);
    let mut i = int_len;
    let mut frac_len = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        frac_len = digits(i);
        i += frac_len;
    }
    if int_len + frac_len == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_len = digits(i);
        if exp_len == 0 {
            return None;
        }
        i += exp_len;
    }
    if i != b.len() {
        return None;
    }
    std::str::from_utf8(b).ok()?.parse().ok()
}

fn hex_val(c: u8) -> Option<u32> {
    char::from(c).to_digit(16)
}

/// Hex after `0x`: `H* [. H*] [p [+-] D+]` with at least one hex digit. A plain integer wraps modulo 2^64;
/// with a fraction or exponent the exact value `mant * 2^e` is rounded once to the nearest double.
fn hex(b: &[u8]) -> Option<f64> {
    let mut i = 0;
    let mut mant: u128 = 0;
    let mut wrapped: u64 = 0;
    let mut sticky = false;
    let mut e2: i64 = 0;
    let mut n_digits = 0;
    let push = |d: u32, frac: bool, mant: &mut u128, e2: &mut i64, sticky: &mut bool| {
        if *mant >> 120 == 0 {
            *mant = (*mant << 4) | u128::from(d);
            if frac {
                *e2 -= 4;
            }
        } else {
            // Mantissa already has > 120 bits: this digit only matters as a sticky bit (or as a scale for integer digits).
            *sticky |= d != 0;
            if !frac {
                *e2 += 4;
            }
        }
    };
    while let Some(d) = b.get(i).and_then(|&c| hex_val(c)) {
        push(d, false, &mut mant, &mut e2, &mut sticky);
        wrapped = (wrapped << 4) | u64::from(d);
        n_digits += 1;
        i += 1;
    }
    let mut has_frac = false;
    if b.get(i) == Some(&b'.') {
        has_frac = true;
        i += 1;
        while let Some(d) = b.get(i).and_then(|&c| hex_val(c)) {
            push(d, true, &mut mant, &mut e2, &mut sticky);
            n_digits += 1;
            i += 1;
        }
    }
    if n_digits == 0 {
        return None;
    }
    let mut has_exp = false;
    if matches!(b.get(i), Some(b'p' | b'P')) {
        has_exp = true;
        i += 1;
        let neg = match b.get(i) {
            Some(b'-') => {
                i += 1;
                true
            }
            Some(b'+') => {
                i += 1;
                false
            }
            _ => false,
        };
        let start = i;
        let mut e: i64 = 0;
        while let Some(&c) = b.get(i).filter(|c| c.is_ascii_digit()) {
            e = (e * 10 + i64::from(c - b'0')).min(1 << 40);
            i += 1;
        }
        if i == start {
            return None;
        }
        e2 += if neg { -e } else { e };
    }
    if i != b.len() {
        return None;
    }
    if !has_frac && !has_exp {
        return Some(wrapped as f64);
    }
    Some(scale_round(mant, sticky, e2))
}

/// Nearest double (ties to even) to `(mant + sticky·ε) * 2^e2`.
fn scale_round(mant: u128, sticky: bool, e2: i64) -> f64 {
    if mant == 0 {
        return 0.0;
    }
    let nb = 128 - i64::from(mant.leading_zeros());
    let top = nb - 1 + e2; // exponent of the leading bit
    if top > 1023 {
        return f64::INFINITY;
    }
    // Bits we may keep: 53 for normals, fewer for subnormals (the lowest representable bit is 2^-1074).
    let keep = if top >= -1022 { 53 } else { 53 - (-1022 - top) };
    if keep <= 0 {
        // Below half the smallest subnormal rounds to 0; exactly half (keep == 0, a single 1 bit, no sticky) ties to even = 0.
        let above_half = keep == 0 && (mant.count_ones() > 1 || sticky);
        return if above_half { f64::from_bits(1) } else { 0.0 };
    }
    // `sticky` only exists when the mantissa overflowed 120 bits, so `drop > 0` whenever it is set.
    let drop = (nb - keep).max(0);
    let mut m = mant >> drop;
    if drop > 0 {
        let half = 1u128 << (drop - 1);
        let rest = mant & ((1u128 << drop) - 1);
        if rest > half || (rest == half && (sticky || m & 1 == 1)) {
            m += 1;
        }
    }
    let mut exp = e2 + drop;
    if m >> 53 != 0 {
        m >>= 1;
        exp += 1;
    }
    // m < 2^53 is exact as a double, and m * 2^exp is representable (precision was cut to fit), so every
    // power-of-two step below is exact: intermediates only have more magnitude than the result.
    let mut v = m as f64;
    while exp != 0 {
        let s = exp.clamp(-1000, 1000);
        v *= 2f64.powi(s as i32);
        exp -= s;
    }
    v
}
