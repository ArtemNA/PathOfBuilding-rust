//! Port of LuaJIT 2.1.1784580905 `src/lj_strfmt_num.c` (commit 2460b3ff93a1c955de3d62cfc825de7d68dc272e; the same file in the docker
//! image and in PoB's desktop `lua51.dll`), kept line for line, quirks included; `:N` below means `lj_strfmt_num.c:N`.
//! Only `%f` and `%g` without `#`: PoB uses no `%a`, `%e`, `%G` or `#`, and `format` rejects them. Differences from the C:
//! - `nd` starts zeroed (:387 leaves it uninitialised). `nd_add_m10e` at :465 can touch a limb below `ndlo`, so on rare `%.14g` values
//!   outside [2^-63, 2^129) LuaJIT's own answer depends on the preceding call; here that memory is always zero.
//! - the text goes to a `Vec<u8>` instead of a raw pointer, so "p += prec" becomes a truncate.
//!
//! The quirks this keeps: an exact tie at 10^9 or above rounds up in `%g` (:271 tests the limb holding the tie digit itself);
//! `%g` with precision <= 14 outside [2^-63, 2^129) works on an inexact `n * 1e±k` (:400); `%f` below ~1e-150 overruns the ring of
//! limbs and prints garbage digits (:210).

#![allow(clippy::manual_div_ceil)] // the C arithmetic stays as written

/// :20 `rescale_e`.
const RESCALE_E: [i32; 32] = [
    -308, -289, -270, -250, -231, -212, -193, -173, -154, -135, -115, -96, -77, -58, -38, 0, 0, 0, 39, 58, 77, 96, 116, 135, 154, 174, 193,
    212, 231, 251, 270, 289,
];

/// :20 `rescale_n`.
const RESCALE_N: [f64; 32] = [
    1e308, 1e289, 1e270, 1e250, 1e231, 1e212, 1e193, 1e173, 1e154, 1e135, 1e115, 1e96, 1e77, 1e58, 1e38, 1e0, 1e0, 1e0, 1e-39, 1e-58,
    1e-77, 1e-96, 1e-116, 1e-135, 1e-154, 1e-174, 1e-193, 1e-212, 1e-231, 1e-251, 1e-270, 1e-289,
];

/// :37 `four_ulp_m_e`: for p in -70..=57, pairs (m, e) with 4 * 2^p <= (u8)m * 10^e, the smallest such value.
#[rustfmt::skip]
const FOUR_ULP_M_E: [i8; 256] = [
    34, -21, 68, -21, 14, -20, 28, -20, 55, -20, 2, -19, 3, -19, 5, -19, 9, -19,
    -82, -18, 35, -18, 7, -17, -117, -17, 28, -17, 56, -17, 112, -16, -33, -16,
    45, -16, 89, -16, -78, -15, 36, -15, 72, -15, -113, -14, 29, -14, 57, -14,
    114, -13, -28, -13, 46, -13, 91, -12, -74, -12, 37, -12, 73, -12, 15, -11, 3,
    -11, 59, -11, 2, -10, 3, -10, 5, -10, 1, -9, -69, -9, 38, -9, 75, -9, 15, -7,
    3, -7, 6, -7, 12, -6, -17, -7, 48, -7, 96, -7, -65, -6, 39, -6, 77, -6, -103,
    -5, 31, -5, 62, -5, 123, -4, -11, -4, 49, -4, 98, -4, -60, -3, 4, -2, 79, -3,
    16, -2, 32, -2, 63, -2, 2, -1, 25, 0, 5, 1, 1, 2, 2, 2, 4, 2, 8, 2, 16, 2,
    32, 2, 64, 2, -128, 2, 26, 2, 52, 2, 103, 3, -51, 3, 41, 4, 82, 4, -92, 4,
    33, 4, 66, 4, -124, 5, 27, 5, 53, 5, 105, 6, 21, 6, 42, 6, 84, 6, 17, 7, 34,
    7, 68, 7, 2, 8, 3, 8, 6, 8, 108, 9, -41, 9, 43, 10, 86, 9, -84, 10, 35, 10,
    69, 10, -118, 11, 28, 11, 55, 12, 11, 13, 22, 13, 44, 13, 88, 13, -80, 13,
    36, 13, 71, 13, -115, 14, 29, 14, 57, 14, 113, 15, -30, 15, 46, 15, 91, 15,
    19, 16, 37, 16, 73, 16, 2, 17, 3, 17, 6, 17,
];

/// :55 `ndigits_dec_threshold`: min(2^32-1, 10^e-1) for e in 0..=10.
const NDIGITS_DEC_THRESHOLD: [u32; 11] = [0, 9, 99, 999, 9999, 99999, 999999, 9999999, 99999999, 999999999, 0xffffffff];

const ND_MUL2K_MAX_SHIFT: u32 = 29;

/// A conversion: the parts of LuaJIT's `SFormat` that `%f` and `%g` read.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    /// `%g` (`STRFMT_T_FP_G`) rather than `%f`.
    pub g: bool,
    pub left: bool,
    pub plus: bool,
    pub zero: bool,
    pub space: bool,
    pub width: u32,
    /// `STRFMT_PREC(sf)`: the written precision, or [`NO_PREC`].
    pub prec: u32,
}

/// `STRFMT_PREC(sf)` without a written precision (lj_strfmt.h: `0 - 1u`).
pub const NO_PREC: u32 = u32::MAX;

/// lj_fls: index of the highest set bit.
fn fls(x: u32) -> u32 {
    31u32.wrapping_sub(x.leading_zeros())
}

/// :63 `ndigits_dec`.
fn ndigits_dec(x: u32) -> u32 {
    let t = ((fls(x | 1) * 77) >> 8) + 1;
    t + u32::from(x > NDIGITS_DEC_THRESHOLD[t as usize])
}

fn wint_r(x: &mut u32, sh: u32, sc: u32) -> u8 {
    let d = x.wrapping_mul(((1 << sh) + sc - 1) / sc) >> sh;
    *x = x.wrapping_sub(d.wrapping_mul(sc));
    (u32::from(b'0').wrapping_add(d)) as u8
}

/// :73 `lj_strfmt_wuint9`.
fn wuint9(mut u: u32) -> [u8; 9] {
    let mut v = u / 10000;
    u -= v * 10000;
    let w = v / 10000;
    v -= w * 10000;
    [
        (u32::from(b'0') + w) as u8,
        wint_r(&mut v, 23, 1000),
        wint_r(&mut v, 12, 100),
        wint_r(&mut v, 10, 10),
        (u32::from(b'0') + v) as u8,
        wint_r(&mut u, 23, 1000),
        wint_r(&mut u, 12, 100),
        wint_r(&mut u, 10, 10),
        (u32::from(b'0') + u) as u8,
    ]
}

/// lj_strfmt_wint; only ever called with non-negative values here.
fn wint(out: &mut Vec<u8>, k: u32) {
    let mut b = [0u8; 10];
    let mut i = b.len();
    let mut k = k;
    loop {
        i -= 1;
        b[i] = b'0' + (k % 10) as u8;
        k /= 10;
        if k == 0 {
            break;
        }
    }
    out.extend_from_slice(&b[i..]);
}

/// The limb pass of `nd_mul2k` (:134, :146).
fn mul_limbs(nd: &mut [u32; 64], ndlo: u32, ndhi: u32, k: u32, mut carry_in: u32) -> u32 {
    for i in ndlo..=ndhi {
        let val = (u64::from(nd[i as usize]) << k) | u64::from(carry_in);
        carry_in = (val / 1_000_000_000) as u32;
        nd[i as usize] = (val as u32).wrapping_sub(carry_in.wrapping_mul(1_000_000_000));
    }
    carry_in
}

/// :124 `nd_mul2k`.
fn nd_mul2k(nd: &mut [u32; 64], mut ndhi: u32, mut k: u32, mut carry_in: u32, sf: &Spec) -> u32 {
    let (mut ndlo, mut start) = (0u32, 1u32);
    if k > ND_MUL2K_MAX_SHIFT * 2 && sf.g {
        start = ndhi.wrapping_sub(sf.prec.wrapping_add(17) / 8);
    }
    while k >= ND_MUL2K_MAX_SHIFT {
        carry_in = mul_limbs(nd, ndlo, ndhi, ND_MUL2K_MAX_SHIFT, carry_in);
        if carry_in != 0 {
            ndhi += 1;
            nd[ndhi as usize] = carry_in;
            carry_in = 0;
            if start == ndlo {
                ndlo += 1;
            }
            start = start.wrapping_add(1);
        }
        k -= ND_MUL2K_MAX_SHIFT;
    }
    if k != 0 {
        carry_in = mul_limbs(nd, ndlo, ndhi, k, carry_in);
        if carry_in != 0 {
            ndhi += 1;
            nd[ndhi as usize] = carry_in;
        }
    }
    ndhi
}

/// The limb pass of `nd_div2k` (:184, :202).
fn div_limbs(nd: &mut [u32; 64], ndlo: u32, ndhi: u32, k: u32, mask: u32, mul: u32) -> u32 {
    let (mut i, mut carry) = (ndhi, 0u32);
    loop {
        let val = nd[i as usize];
        nd[i as usize] = (val >> k).wrapping_add(carry);
        carry = (val & mask).wrapping_mul(mul);
        if i == ndlo {
            return carry;
        }
        i = i.wrapping_sub(1) & 0x3f;
    }
}

/// :157 `nd_div2k`.
fn nd_div2k(nd: &mut [u32; 64], mut ndhi: u32, mut k: u32, sf: &Spec) -> u32 {
    let (mut ndlo, mut stop1, mut stop2) = (0u32, u32::MAX, u32::MAX);
    if ndhi == 0 {
        if nd[0] == 0 {
            return 0;
        }
        let s = nd[0].trailing_zeros();
        if s >= k {
            nd[0] >>= k;
            return 0;
        }
        nd[0] >>= s;
        k -= s;
    }
    if k > 18 && sf.g {
        // :171: %f must not limit precision here or nd_round cannot round to even.
        let floorlog2 = (ndhi * 29).wrapping_add(fls(nd[ndhi as usize])) as i32 - k as i32;
        let floorlog10 = (f64::from(floorlog2) * 0.30102999566398114) as i32;
        stop1 = (62 + (floorlog10 - sf.prec as i32) / 9) as u32;
        stop2 = (61 + ndhi as i32 - (sf.prec as i32) / 8) as u32;
    }
    while k >= 9 {
        let carry = div_limbs(nd, ndlo, ndhi, 9, 0x1ff, 1953125);
        if ndlo != stop1 && ndlo != stop2 {
            if carry != 0 {
                ndlo = ndlo.wrapping_sub(1) & 0x3f;
                nd[ndlo as usize] = carry;
            }
            if nd[ndhi as usize] == 0 {
                ndhi = ndhi.wrapping_sub(1) & 0x3f;
                stop2 = stop2.wrapping_sub(1);
            }
        } else if nd[ndhi as usize] == 0 {
            if ndhi != ndlo {
                ndhi = ndhi.wrapping_sub(1) & 0x3f;
                stop2 = stop2.wrapping_sub(1);
            } else {
                return ndlo;
            }
        }
        k -= 9;
    }
    if k != 0 {
        let carry = div_limbs(nd, ndlo, ndhi, k, (1u32 << k) - 1, 1_000_000_000u32 >> k);
        if carry != 0 {
            ndlo = ndlo.wrapping_sub(1) & 0x3f;
            nd[ndlo as usize] = carry;
        }
    }
    ndlo
}

/// :216 `nd_add_m10e`.
fn nd_add_m10e(nd: &mut [u32; 64], mut ndhi: u32, m: u8, e: i32) -> u32 {
    let (mut i, mut carry);
    if e >= 0 {
        i = e as u32 / 9;
        carry = u32::from(m).wrapping_mul(NDIGITS_DEC_THRESHOLD[(e - i as i32 * 9) as usize] + 1);
    } else {
        let f = (e - 8) / 9;
        i = (64 + f) as u32;
        carry = u32::from(m).wrapping_mul(NDIGITS_DEC_THRESHOLD[(e - f * 9) as usize] + 1);
    }
    loop {
        let mut val = nd[i as usize].wrapping_add(carry);
        if val >= 1_000_000_000 {
            val -= 1_000_000_000;
            nd[i as usize] = val;
            if i == ndhi {
                ndhi = (ndhi + 1) & 0x3f;
                nd[ndhi as usize] = 1;
                break;
            }
            carry = 1;
            i = (i + 1) & 0x3f;
        } else {
            nd[i as usize] = val;
            break;
        }
    }
    ndhi
}

/// :248 `nd_round`: "round to even", except that the loop at :271 starts on the limb holding the tie digit.
fn nd_round(nd: &mut [u32; 64], ndlo: u32, ndhi: u32, e: i32) -> u32 {
    let (mut i, mut d);
    if e >= 0 {
        i = e as u32 / 9;
        d = 8 - e + i as i32 * 9;
    } else {
        let f = (e - 8) / 9;
        i = (64 + f) as u32;
        d = 8 - e + f * 9;
    }
    let buf = wuint9(nd[i as usize]);
    let digit = buf[d as usize];
    if digit < b'5' {
        return ndhi;
    }
    if digit == b'5' {
        let odd = if d != 0 {
            buf[d as usize - 1] & 1
        } else {
            (nd[((i + 1) & 0x3f) as usize] & 1) as u8
        };
        if odd == 0 {
            let mut up = false;
            d += 1;
            while d < 9 {
                if buf[d as usize] != b'0' {
                    up = true;
                    break;
                }
                d += 1;
            }
            while !up && i != ndlo {
                if nd[i as usize] != 0 {
                    up = true;
                }
                i = i.wrapping_sub(1) & 0x3f;
            }
            if !up {
                return ndhi;
            }
        }
    }
    nd_add_m10e(nd, ndhi, 5, e)
}

/// :283 `nd_similar`; `refi` indexes the saved limbs (33, 32, 31).
fn nd_similar(nd: &[u32; 64], mut ndhi: u32, mut refi: usize, hilen: u32, mut prec: u32) -> bool {
    if hilen <= prec {
        if nd[ndhi as usize] != nd[refi] {
            return false;
        }
        prec -= hilen;
        refi -= 1;
        ndhi = ndhi.wrapping_sub(1) & 0x3f;
        if prec >= 9 {
            if nd[ndhi as usize] != nd[refi] {
                return false;
            }
            prec -= 9;
            refi -= 1;
            ndhi = ndhi.wrapping_sub(1) & 0x3f;
        }
    } else {
        prec = prec.wrapping_sub(hilen.wrapping_sub(9));
    }
    let (nd9, ref9) = (wuint9(nd[ndhi as usize]), wuint9(nd[refi]));
    let p = prec as usize;
    nd9[..p] == ref9[..p] && (nd9[p] < b'5') == (ref9[p] < b'5')
}

fn fill(out: &mut Vec<u8>, c: u8, width: u32, len: u32) {
    out.resize(out.len() + width.saturating_sub(len) as usize, c);
}

/// :319 / :391.
fn sign_prefix(sf: &Spec, t: u64) -> Option<u8> {
    if hi(t) & 0x8000_0000 != 0 {
        Some(b'-')
    } else if sf.plus {
        Some(b'+')
    } else if sf.space {
        Some(b' ')
    } else {
        None
    }
}

/// :509 / :577: spaces, sign, zeroes.
fn left_side(out: &mut Vec<u8>, sf: &Spec, prefix: Option<u8>, len: u32) {
    if !sf.left && !sf.zero {
        fill(out, b' ', sf.width, len);
    }
    out.extend(prefix);
    if sf.zero && !sf.left {
        fill(out, b'0', sf.width, len);
    }
}

fn hi(t: u64) -> u32 {
    (t >> 32) as u32
}

fn lo(t: u64) -> u32 {
    t as u32
}

/// :306 `lj_strfmt_wfnum` for `%f` and `%g`: appends the formatted `n`, padding included.
pub fn wfnum(out: &mut Vec<u8>, sf: &Spec, n: f64) {
    let width = sf.width;
    let mut prec = sf.prec;
    let mut t = n.to_bits();
    let len;
    if (hi(t) << 1) >= 0xffe0_0000 {
        // :311 non-finite.
        let is_nan = ((hi(t) & 0x000f_ffff) | lo(t)) != 0;
        let prefix = if is_nan { sf.space.then_some(b' ') } else { sign_prefix(sf, t) };
        len = 3 + u32::from(prefix.is_some());
        if !sf.left {
            fill(out, b' ', width, len);
        }
        out.extend(prefix);
        out.extend_from_slice(if is_nan { b"nan" } else { b"inf" });
    } else {
        // :386 convert n to "nd" format.
        let mut nd = [0u32; 64];
        let mut ndhi = 0u32;
        let mut ndlo;
        let mut e = ((hi(t) >> 20) & 0x7ff) as i32;
        let mut ndebias = 0i32;
        let prefix = sign_prefix(sf, t);
        prec = prec.wrapping_add((((prec as i32) >> 31) & 7) as u32);
        if sf.g {
            prec = prec.wrapping_sub(1);
            prec ^= ((prec as i32) >> 31) as u32;
        }
        let mut rescale = sf.g && prec < 14 && n != 0.0;
        // `Some(len)` once %g has printed in %e style; `None` goes on to the %f-style output.
        let exp_len: Option<u32> = loop {
            if rescale {
                ndebias = RESCALE_E[(e >> 6) as usize];
            }
            let mut load_lo = false;
            if rescale && ndebias != 0 {
                // :402 rescale: `n * 1e±k`, converted 2ulp below (2ulp above is added at :465).
                let mut r = n * RESCALE_N[(e >> 6) as usize];
                if e == 0 {
                    r *= 1e10;
                    ndebias -= 10;
                }
                t = r.to_bits().wrapping_sub(2);
                nd[0] = 0x10_0000 | (hi(t) & 0xf_ffff);
                e = ((hi(t) >> 20) & 0x7ff) as i32 - 1075;
                load_lo = true;
            } else {
                nd[0] = hi(t) & 0xf_ffff;
                if e == 0 {
                    e += 1;
                } else {
                    nd[0] |= 0x10_0000;
                }
                e -= 1043;
                if lo(t) != 0 {
                    e -= 32;
                    load_lo = true;
                }
            }
            if load_lo {
                nd[0] = (nd[0] << 3) | (lo(t) >> 29);
                ndhi = nd_mul2k(&mut nd, ndhi, 29, lo(t) & 0x1fff_ffff, sf);
            }
            if e >= 0 {
                ndhi = nd_mul2k(&mut nd, ndhi, e as u32, 0, sf);
                ndlo = 0;
            } else {
                ndlo = nd_div2k(&mut nd, ndhi, e.unsigned_abs(), sf);
                if ndhi != 0 && nd[ndhi as usize] == 0 {
                    ndhi -= 1;
                }
            }
            if !sf.g {
                break None;
            }
            // :438 %g: assume %e and start by calculating nd's exponent (nde).
            let mut nde = -1i32;
            if ndlo != 0 && nd[ndhi as usize] == 0 {
                ndhi = 64;
                loop {
                    ndhi -= 1;
                    if nd[ndhi as usize] != 0 {
                        break;
                    }
                }
                nde -= 64 * 9;
            }
            let hilen = ndigits_dec(nd[ndhi as usize]);
            nde += (ndhi * 9 + hilen) as i32;
            if ndebias != 0 {
                // :449 did the rescaling error cross a rounding boundary? Compare the digits of -2ulp and +2ulp.
                let eidx = e + 70 + i32::from(lo(t) >= 0xffff_fffe && (!hi(t) << 12) == 0);
                let (m, m_e) = (
                    FOUR_ULP_M_E[eidx as usize * 2] as u8,
                    i32::from(FOUR_ULP_M_E[eidx as usize * 2 + 1]),
                );
                nd[33] = nd[ndhi as usize];
                nd[32] = nd[(ndhi.wrapping_sub(1) & 0x3f) as usize];
                nd[31] = nd[(ndhi.wrapping_sub(2) & 0x3f) as usize];
                nd_add_m10e(&mut nd, ndhi, m, m_e);
                if !nd_similar(&nd, ndhi, 33, hilen, prec + 1) {
                    // :408 rescale_failed.
                    t = n.to_bits();
                    e = ((hi(t) >> 20) & 0x7ff) as i32;
                    ndebias = 0;
                    ndhi = 0;
                    rescale = false;
                    continue;
                }
            }
            if (prec as i32 - nde) < (0x3f & (ndlo as i32).wrapping_neg()) * 9 {
                ndhi = nd_round(&mut nd, ndlo, ndhi, nde - prec as i32 - 1);
                nde += i32::from(hilen != ndigits_dec(nd[ndhi as usize]));
            }
            nde += ndebias;
            if prec as i32 >= nde && nde >= -4 {
                if nde < 0 {
                    ndhi = 0;
                }
                prec = prec.wrapping_sub(nde as u32);
                break None;
            }
            if prec != 0 && width > 5 {
                // :483 decrease precision in order to strip trailing zeroes.
                let maxprec = hilen - 1 + (ndhi.wrapping_sub(ndlo) & 0x3f) * 9;
                if prec >= maxprec {
                    prec = maxprec;
                } else {
                    ndlo = (ndhi as i32 - (prec.wrapping_sub(hilen) as i32 + 9) / 9) as u32 & 0x3f;
                }
                let mut i = prec
                    .wrapping_sub(hilen)
                    .wrapping_sub((ndhi.wrapping_sub(ndlo) & 0x3f) * 9)
                    .wrapping_add(10);
                let mut tail = wuint9(nd[ndlo as usize]);
                while prec != 0 && {
                    i = i.wrapping_sub(1);
                    tail[i as usize] == b'0'
                } {
                    prec -= 1;
                    if i == 0 {
                        if ndlo == ndhi {
                            prec = 0;
                            break;
                        }
                        ndlo = (ndlo + 1) & 0x3f;
                        tail = wuint9(nd[ndlo as usize]);
                        i = 9;
                    }
                }
            }
            let (eprefix, nde) = if nde < 0 { (b'-', nde.unsigned_abs()) } else { (b'+', nde as u32) };
            // :506 emit in %e style.
            let l = 3 + prec + u32::from(prefix.is_some()) + ndigits_dec(nde) + u32::from(nde < 10) + u32::from(prec != 0);
            left_side(out, sf, prefix, l);
            let p0 = out.len();
            out.push(0);
            wint(out, nd[ndhi as usize]);
            out[p0] = out[p0 + 1];
            if prec != 0 {
                out[p0 + 1] = b'.';
                let mut pr = prec as i32 - (out.len() - (p0 + 2)) as i32;
                let mut i = ndhi;
                while pr > 0 && i != ndlo {
                    i = i.wrapping_sub(1) & 0x3f;
                    out.extend_from_slice(&wuint9(nd[i as usize]));
                    pr -= 9;
                }
                strip_zeroes(out, pr);
            } else {
                out.truncate(p0 + 1);
            }
            out.push(b'e');
            out.push(eprefix);
            if nde < 10 {
                out.push(b'0');
            }
            wint(out, nde);
            break Some(l);
        };
        len = match exp_len {
            Some(l) => l,
            None => {
                // :545 %f, or %g in %f style.
                if !sf.g && prec < (0x3f & (ndlo as i32).wrapping_neg()) as u32 * 9 {
                    ndhi = nd_round(&mut nd, ndlo, ndhi, -(prec as i32) - 1);
                }
                if sf.g && prec != 0 && width != 0 {
                    // :552 decrease precision in order to strip trailing zeroes.
                    if ndlo != 0 {
                        let maxprec = (64 - ndlo) * 9;
                        if prec >= maxprec {
                            prec = maxprec;
                        } else {
                            ndlo = 64 - (prec + 8) / 9;
                        }
                        let mut i = prec - (63 - ndlo) * 9;
                        let mut tail = wuint9(nd[ndlo as usize]);
                        while prec != 0 && {
                            i = i.wrapping_sub(1);
                            tail[i as usize] == b'0'
                        } {
                            prec -= 1;
                            if i == 0 {
                                if ndlo == 63 {
                                    prec = 0;
                                    break;
                                }
                                ndlo += 1;
                                tail = wuint9(nd[ndlo as usize]);
                                i = 9;
                            }
                        }
                    } else {
                        prec = 0;
                    }
                }
                let l = (ndhi * 9 + ndigits_dec(nd[ndhi as usize]))
                    .wrapping_add(prec)
                    .wrapping_add(u32::from(prefix.is_some()) + u32::from(prec != 0));
                left_side(out, sf, prefix, l);
                wint(out, nd[ndhi as usize]);
                let mut i = ndhi;
                while i != 0 {
                    i -= 1;
                    out.extend_from_slice(&wuint9(nd[i as usize]));
                }
                if prec != 0 {
                    out.push(b'.');
                    let mut pr = prec as i32;
                    while pr > 0 && i != ndlo {
                        i = i.wrapping_sub(1) & 0x3f;
                        out.extend_from_slice(&wuint9(nd[i as usize]));
                        pr -= 9;
                    }
                    if sf.g {
                        strip_zeroes(out, pr);
                    } else {
                        // :603 %f: emit trailing zeroes.
                        out.resize(out.len() + pr.max(0) as usize, b'0');
                        out.truncate(out.len() - (-pr.min(0)) as usize);
                    }
                }
                l
            }
        };
    }
    if sf.left {
        fill(out, b' ', width, len);
    }
}

/// :529 / :599: drop the digits emitted past the precision (`pr` < 0), then trailing zeroes and a bare point.
fn strip_zeroes(out: &mut Vec<u8>, pr: i32) {
    out.truncate(out.len() - (-pr.min(0)) as usize);
    while out.last() == Some(&b'0') {
        out.pop();
    }
    if out.last() == Some(&b'.') {
        out.pop();
    }
}

/// The port's text, for callers holding a `Spec`.
pub fn to_string(sf: &Spec, n: f64) -> String {
    let mut out = Vec::with_capacity(32);
    wfnum(&mut out, sf, n);
    String::from_utf8_lossy(&out).into_owned()
}
