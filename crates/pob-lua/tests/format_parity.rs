//! R1-perf gate for `format.rs` and `lj_strfmt_num.rs`.
//! (1) The port against both real LuaJITs on edge doubles (`tests/lua/export-format-edges.ts` -> `tests/fixtures/luajit-format-edges.json`).
//! (2) The hot path against the port on millions of fixed-seed values (`cargo test --release`; a debug run does 1/50 of them).
//! (3) A control only: the implementation before the port, kept verbatim, is counted against the hot path on the same values. It is not a
//!     reference; `POB_LUA_DIFF_DUMP=<file>` writes where the two differ, so those rows can be put to LuaJIT.
//! (4) `number::pow10` against `powf`.

use pob_lua::lj_strfmt_num::{self as lj, NO_PREC, Spec};
use pob_lua::number;
use pob_lua::{Arg, format, format_f, format_g, tostring_num};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;

// ---- The implementation before R1-perf, verbatim except for names (std's exact formatter + String layout). ----

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

fn sig_digits(n: f64, p: usize) -> (String, i32) {
    let s = format!("{:.*e}", p - 1, n.abs());
    let (mant, exp) = s.split_once('e').expect("exponential format has an 'e'");
    (mant.replace('.', ""), exp.parse().expect("exponent is an integer"))
}

fn ref_g(n: f64, precision: usize) -> String {
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

fn ref_f(n: f64, p: usize) -> String {
    match non_finite(n) {
        Some(s) => s.to_owned(),
        None => format!("{n:.p$}"),
    }
}

// ---- Generators. ----

/// SplitMix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn log_uniform(&mut self, lo: f64, hi: f64) -> f64 {
        (lo.ln() + self.unit() * (hi.ln() - lo.ln())).exp()
    }
    fn signed(&mut self, v: f64) -> f64 {
        if self.next() & 1 == 0 { v } else { -v }
    }
}

fn ulps(v: f64, d: i64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        f64::from_bits(v.to_bits().wrapping_add_signed(d))
    }
}

/// Any bit pattern: NaNs, subnormals, every exponent.
fn random_bits(r: &mut Rng) -> f64 {
    f64::from_bits(r.next())
}

/// Near a power of ten, including just below the point where the rounding carries into the next decade.
fn around_pow10(r: &mut Rng) -> f64 {
    let k = r.below(61) as i32 - 30;
    let base: f64 = format!("1e{k}").parse().unwrap();
    let v = match r.below(3) {
        0 => base,
        1 => base * (1.0 - 0.5 * 10f64.powi(-(r.below(17) as i32 + 1))),
        _ => base * (1 + r.below(9)) as f64,
    };
    let d = r.below(9) as i64 - 4;
    r.signed(ulps(v, d))
}

/// Exact decimal ties: dyadic values at every scale, and integers ending in 5 / 50.
fn halves(r: &mut Rng) -> f64 {
    let v = match r.below(4) {
        0 => {
            // K / 2^j with K odd has its last nonzero decimal, a 5, at j places: a tie for %.<j-1>f.
            let j = r.below(23) as i32;
            let bits = 1 + r.below(53) as u32;
            let k = (r.next() >> (64 - bits)) | 1;
            k as f64 / 2f64.powi(j)
        }
        1 => {
            // K / 2^j with K * 5^j of exactly 15 digits: a tie for %.14g.
            let j = 1 + r.below(21) as u32;
            let pow5 = 5u128.pow(j);
            let (lo, hi) = (10u128.pow(14) / pow5 + 1, 10u128.pow(15) / pow5);
            let k = (lo + u128::from(r.next()) % (hi - lo + 1)) | 1;
            k as f64 / 2f64.powi(j as i32)
        }
        2 => (10u64.pow(13) + r.below(9 * 10u64.pow(13))) as f64 * 10.0 + 5.0,
        _ => (10u64.pow(13) + r.below(9 * 10u64.pow(13))) as f64 * 100.0 + 50.0,
    };
    r.signed(v)
}

/// What PoB formats: integers, short decimals, products of them, over a wide magnitude range.
fn real_like(r: &mut Rng, stats: &[f64]) -> f64 {
    match r.below(6) {
        0 => r.below(10_000_000) as f64,
        1 => r.below(100_000_000) as f64 / 100.0,
        2 => r.below(1_000_000) as f64 / 10.0,
        3 => r.log_uniform(1e-30, 1e25),
        4 => r.log_uniform(1e14, 1.9e19),
        _ if stats.is_empty() => r.log_uniform(0.01, 1e9),
        _ => stats[r.below(stats.len() as u64) as usize] * (1.0 + r.below(200) as f64 / 100.0),
    }
}

/// Numeric stats of the goldens (read at run time, nothing from them is stored here).
fn golden_stats() -> Vec<f64> {
    let goldens = std::env::var("POB_TS_ROOT").unwrap_or_else(|_| "C:/pob-work/pob-ts".into()) + "/tools/oracle/goldens";
    std::fs::read_dir(&goldens)
        .map(|d| {
            d.flatten()
                .flat_map(|e| {
                    let j: Value = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
                    j["stats"]
                        .as_object()
                        .unwrap()
                        .values()
                        .filter_map(Value::as_f64)
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .unwrap_or_default()
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

/// Mismatches of the gate, and the control's count by class.
#[derive(Default)]
struct Outcome {
    bad: Vec<String>,
    control: BTreeMap<String, u64>,
    dump: String,
    dumped: u64,
}

impl Outcome {
    fn gate(&mut self, what: &str, v: f64, new: &str, port: &str) {
        if new != port && self.bad.len() < 20 {
            self.bad
                .push(format!("{what} of {v:e} ({:#x}): port {port:?}, hot path {new:?}", v.to_bits()));
        }
    }
    fn control(&mut self, class: &str, what: &str, v: f64, new: &str, old: &str) {
        if new != old {
            let rescale = v != 0.0 && v.is_finite() && (v.abs() < 2f64.powi(-63) || v.abs() >= 2f64.powi(129));
            let zone = if rescale { "outside [2^-63, 2^129)" } else { "inside" };
            let kind = if what.ends_with('f') { "%f" } else { "%g" };
            *self.control.entry(format!("{class} / {kind} / {zone}")).or_default() += 1;
            if self.dumped < 150_000 {
                self.dumped += 1;
                writeln!(self.dump, "{:016x}|{what}|{old}|{new}", v.to_bits()).unwrap();
            }
        }
    }
}

fn check_value(class: &str, v: f64, r: &mut Rng, o: &mut Outcome) {
    let new = tostring_num(v);
    o.gate("tostring", v, &new, &lj::to_string(&plain(true, 14), v));
    o.control(class, "%.14g", v, &new, &ref_g(v, 14));
    let pg = r.below(21) as usize;
    let what = format!("%.{pg}g");
    let new = format_g(v, pg);
    o.gate(&what, v, &new, &lj::to_string(&plain(true, pg as u32), v));
    o.control(class, &what, v, &new, &ref_g(v, pg));
    // Weighted towards the precisions PoB uses, up past the 22-digit end of the integer path.
    let pf = if r.below(2) == 0 {
        r.below(5) as usize
    } else {
        r.below(26) as usize
    };
    let what = format!("%.{pf}f");
    let new = format_f(v, pf);
    o.gate(&what, v, &new, &lj::to_string(&plain(false, pf as u32), v));
    // The old `%f` of a huge value costs microseconds and is only a control; a sample is enough there.
    if v.abs() < 1e40 || r.below(64) == 0 {
        o.control(class, &what, v, &new, &ref_f(v, pf));
    }
}

fn run_class(name: &str, seed: u64, n: u64, mut gen_value: impl FnMut(&mut Rng) -> f64) {
    let mut r = Rng(seed);
    let mut o = Outcome::default();
    for _ in 0..n {
        let v = gen_value(&mut r);
        check_value(name, v, &mut r, &mut o);
    }
    assert!(
        o.bad.is_empty(),
        "{name}: hot path differs from the port on {n} values; first: {:#?}",
        o.bad
    );
    eprintln!(
        "{name}: {n} values, hot path = port; control (old path differs from new): {:#?}",
        o.control
    );
    if let Ok(path) = std::env::var("POB_LUA_DIFF_DUMP") {
        std::fs::write(format!("{path}.{seed:x}"), o.dump).unwrap();
    }
}

fn scale(n: u64) -> u64 {
    if cfg!(debug_assertions) { n / 50 } else { n }
}

#[test]
fn random_bits_match_port() {
    run_class("random bits", 0x5eed_0001, scale(2_000_000), random_bits);
}

#[test]
fn around_powers_of_ten_match_port() {
    run_class("around 10^k", 0x5eed_0002, scale(2_000_000), around_pow10);
}

#[test]
fn halves_match_port() {
    run_class("halves", 0x5eed_0003, scale(2_000_000), halves);
}

#[test]
fn real_like_values_match_port() {
    let stats = golden_stats();
    run_class("real-like", 0x5eed_0004, scale(2_000_000), |r| real_like(r, &stats));
    // Every golden stat itself, at every precision PoB formats with.
    let mut o = Outcome::default();
    let mut r = Rng(0x5eed_0005);
    for &v in &stats {
        check_value("golden", v, &mut r, &mut o);
        for p in 0..=6 {
            o.gate(&format!("%.{p}f"), v, &format_f(v, p), &lj::to_string(&plain(false, p as u32), v));
        }
    }
    assert!(o.bad.is_empty(), "golden stats: {:#?}", o.bad);
    assert!(o.control.is_empty(), "the old path differs on golden stats: {:#?}", o.control);
}

/// Exact ties and near ties at the tie digits `nd_round` treats differently (lj_strfmt_num.c:271), and just past where `nd_div2k` stops.
#[test]
fn tie_digit_positions_match_port() {
    run_class("ties at 10^k", 0x5eed_0008, scale(500_000), |r| {
        let p = 1 + r.below(15) as u32;
        let head = 10u128.pow(p - 1) + u128::from(r.next()) % (9 * 10u128.pow(p - 1));
        let k = r.below(24) as u32;
        let tie = (head * 10 + 5) * 10u128.pow(k);
        let d = r.below(3) as i64 - 1;
        r.signed(ulps(tie as f64, d))
    });
}

/// The double nearest a decimal tie at its own precision: for few digits it lies far closer to the tie than `nd_div2k` keeps
/// (lj_strfmt_num.c:178), which is what the hot path's near-tie test is for.
#[test]
fn near_ties_at_their_own_precision_match_port() {
    let mut r = Rng(0x5eed_0009);
    for _ in 0..scale(2_000_000) {
        let p = 1 + r.below(17) as u32;
        let head = 10u64.pow(p - 1) + r.next() % (9 * 10u64.pow(p - 1));
        let x = r.below(70) as i32 - 25;
        let base: f64 = format!("{head}5e{}", x - p as i32).parse().unwrap();
        let d = r.below(3) as i64 - 1;
        let v = ulps(base, d);
        let ours = format_g(v, p as usize);
        let port = lj::to_string(&plain(true, p), v);
        assert_eq!(ours, port, "%.{p}g of {v:e} ({:#x})", v.to_bits());
    }
}

#[test]
fn width_and_flags_match_port() {
    let base = Spec {
        g: false,
        left: false,
        plus: false,
        zero: false,
        space: false,
        width: 0,
        prec: 0,
    };
    let specs = [
        (
            "%+.2f",
            Spec {
                plus: true,
                prec: 2,
                ..base
            },
        ),
        (
            "%-12.3f",
            Spec {
                left: true,
                width: 12,
                prec: 3,
                ..base
            },
        ),
        (
            "%012.2f",
            Spec {
                zero: true,
                width: 12,
                prec: 2,
                ..base
            },
        ),
        (
            "% .1f",
            Spec {
                space: true,
                prec: 1,
                ..base
            },
        ),
        ("%5.0f", Spec { width: 5, ..base }),
        ("%f", Spec { prec: NO_PREC, ..base }),
        (
            "%+g",
            Spec {
                g: true,
                plus: true,
                prec: NO_PREC,
                ..base
            },
        ),
        (
            "%g",
            Spec {
                g: true,
                prec: NO_PREC,
                ..base
            },
        ),
        (
            "%08.3g",
            Spec {
                g: true,
                zero: true,
                width: 8,
                prec: 3,
                ..base
            },
        ),
        (
            "%-12g",
            Spec {
                g: true,
                left: true,
                width: 12,
                prec: NO_PREC,
                ..base
            },
        ),
        (
            "%5g",
            Spec {
                g: true,
                width: 5,
                prec: NO_PREC,
                ..base
            },
        ),
        (
            "%14.14g",
            Spec {
                g: true,
                width: 14,
                prec: 14,
                ..base
            },
        ),
    ];
    let mut r = Rng(0x5eed_0006);
    let specials = [
        0.0,
        -0.0,
        f64::NAN,
        -f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        2.5,
        -2.5,
        1e15,
        1e-5,
    ];
    for i in 0..scale(500_000) {
        let v = match specials.get(i as usize) {
            Some(&s) => s,
            None => match r.below(3) {
                0 => around_pow10(&mut r),
                1 => halves(&mut r),
                _ => real_like(&mut r, &[]),
            },
        };
        for (fmt, spec) in &specs {
            let ours = format(fmt, &[Arg::Num(v)]).unwrap();
            assert_eq!(ours, lj::to_string(spec, v), "{fmt} of {v:e} ({:#x})", v.to_bits());
        }
    }
}

#[test]
fn pow10_table_equals_powf() {
    let same = |d: f64| {
        let (t, p) = (number::pow10(d), 10f64.powf(d));
        assert!(
            t.to_bits() == p.to_bits(),
            "10^{d}: table {t:e} ({:#x}), powf {p:e} ({:#x})",
            t.to_bits(),
            p.to_bits()
        );
    };
    for k in -400..=400 {
        same(f64::from(k));
    }
    // Everything `round` / `floor` can be handed that is not a whole number in the table's range.
    for k in -200..=200 {
        same(f64::from(k) / 8.0);
        same(ulps(f64::from(k), 1));
        same(ulps(f64::from(k), -1));
    }
    for d in [-0.0, f64::INFINITY, f64::NEG_INFINITY, 1e300, -1e300, 22.5, 23.0, 2f64.powi(64)] {
        same(d);
    }
    assert!(number::pow10(f64::NAN).is_nan());
    // round / floor against the formulas of Common.lua with `powf`, on the same fixed values.
    let mut r = Rng(0x5eed_0007);
    for _ in 0..scale(1_000_000) {
        let v = real_like(&mut r, &[]);
        let d = r.below(12) as f64;
        let mult = 10f64.powf(d);
        let (want_round, want_floor) = ((v * mult + 0.5).floor() / mult, (v * mult + 0.0001).floor() / mult);
        assert_eq!(number::round(v, Some(d)).to_bits(), want_round.to_bits(), "round({v:e}, {d})");
        assert_eq!(number::floor(v, Some(d)).to_bits(), want_floor.to_bits(), "floor({v:e}, {d})");
    }
}

/// The port and the public API against LuaJIT. A slot is one answer, or the set of answers LuaJIT itself gave after different preceding
/// calls (the producer's measurement): the port must then be one of them. No value is left out.
#[test]
fn edges_match_luajit() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/luajit-format-edges.json");
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let fx: Value = serde_json::from_str(&text).unwrap();
    let names = |key: &str| -> Vec<String> { fx[key].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_owned()).collect() };
    let (fmts, width_fmts) = (names("fmts"), names("widthFmts"));
    let rows = fx["rows"].as_array().unwrap();
    let (mut exact, mut in_set) = (0usize, 0usize);
    let mut by_slot = BTreeMap::<String, usize>::new();
    let mut outside_zone = Vec::new();
    let mut bad = Vec::new();
    let mut check = |slot: &str, hex: &str, v: f64, ours: String, want: &Value| match want {
        Value::String(s) => {
            exact += 1;
            if ours != *s {
                bad.push(format!("{slot} of {hex} ({v:e}): LuaJIT {s:?}, ours {ours:?}"));
            }
        }
        Value::Array(set) => {
            in_set += 1;
            *by_slot.entry(slot.to_owned()).or_default() += 1;
            if !(v.abs() < 2f64.powi(-63) || v.abs() >= 2f64.powi(129)) {
                outside_zone.push(format!("{slot} of {hex}"));
            }
            if !set.iter().any(|s| s.as_str() == Some(ours.as_str())) {
                bad.push(format!("{slot} of {hex} ({v:e}): LuaJIT's set {set:?}, ours {ours:?}"));
            }
        }
        _ => panic!("bad slot {want:?}"),
    };
    for row in rows {
        let row = row.as_array().unwrap();
        let hex = row[0].as_str().unwrap();
        let v = f64::from_bits(u64::from_str_radix(hex, 16).unwrap());
        check("tostring", hex, v, tostring_num(v), &row[1]);
        let all = fmts.iter().chain(if row.len() > 2 + fmts.len() { &width_fmts[..] } else { &[] });
        for (k, fmt) in all.enumerate() {
            check(fmt, hex, v, format(fmt, &[Arg::Num(v)]).unwrap(), &row[2 + k]);
        }
    }
    eprintln!(
        "edges: {} values, {exact} slots with one answer, {in_set} with several (by slot {by_slot:?}), of those outside the rescale zone: {outside_zone:?}",
        rows.len()
    );
    assert!(
        bad.is_empty(),
        "{} of {} differ from LuaJIT; first: {:#?}",
        bad.len(),
        exact + in_set,
        &bad[..bad.len().min(20)]
    );
    assert!(rows.len() >= 2000 && exact >= 25_000, "{} values, {exact} compared", rows.len());
}
