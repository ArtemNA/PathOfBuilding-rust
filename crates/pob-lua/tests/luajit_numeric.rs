//! pob-lua against desktop LuaJIT: the corpora of pob-ts test/lua/corpus.ts with answers captured from the docker image
//! (`tools/lua-corpus/export-numeric.ts` -> `tests/fixtures/luajit-numeric.json`). Every value is compared bit for bit
//! (NaN == NaN regardless of sign).

use pob_lua::number;
use pob_lua::{Arg, format, tonumber, tostring_num};
use serde_json::Value;

fn fixture() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/luajit-numeric.json");
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap()
}

fn rows<'a>(fx: &'a Value, key: &str) -> Vec<Vec<&'a str>> {
    fx[key]
        .as_array()
        .unwrap_or_else(|| panic!("fixture has no '{key}'"))
        .iter()
        .map(|r| r.as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect())
        .collect()
}

/// Input token (pob-ts `encodeNum`): nan / -nan / inf / -inf / -0.0 / %.17g.
fn num(tok: &str) -> f64 {
    match tok {
        "nan" => f64::NAN,
        "-nan" => -f64::NAN,
        "inf" => f64::INFINITY,
        "-inf" => f64::NEG_INFINITY,
        _ => tok.parse().unwrap_or_else(|_| panic!("bad number token {tok:?}")),
    }
}

/// Output token of the Lua `enc()`: a type prefix (`f`; `i` never occurs on LuaJIT) + an input-style token.
fn out(tok: &str) -> f64 {
    let body = tok.strip_prefix('f').or_else(|| tok.strip_prefix('i')).unwrap_or(tok);
    if body == "0" { 0.0 } else { num(body) }
}

fn same(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

fn show(v: f64) -> String {
    if v == 0.0 && v.is_sign_negative() {
        "-0.0".into()
    } else {
        format!("{v:e}")
    }
}

struct Tally {
    total: usize,
    bad: Vec<String>,
}

impl Tally {
    fn new() -> Self {
        Tally { total: 0, bad: Vec::new() }
    }
    fn check(&mut self, ok: bool, what: impl FnOnce() -> String) {
        self.total += 1;
        if !ok {
            self.bad.push(what());
        }
    }
    fn assert(self, name: &str) -> usize {
        assert!(
            self.bad.is_empty(),
            "{name}: {} of {} differ from LuaJIT; first: {:#?}",
            self.bad.len(),
            self.total,
            &self.bad[..self.bad.len().min(15)]
        );
        self.total
    }
}

#[test]
fn tostring_is_luajit_14g() {
    let fx = fixture();
    let mut t = Tally::new();
    for r in rows(&fx, "tostring") {
        let ours = tostring_num(num(r[0]));
        t.check(ours == r[1], || format!("{} -> want {:?} got {ours:?}", r[0], r[1]));
    }
    let n = t.assert("tostring");
    assert!(n >= 3200, "{n}");
}

#[test]
fn format_is_luajit_printf() {
    let fx = fixture();
    let mut t = Tally::new();
    for r in rows(&fx, "format") {
        let (fmt, tok, want) = (r[0], r[1], r[2]);
        let (kind, rest) = tok.split_at(1);
        let arg = match kind {
            "n" => Arg::Num(num(rest)),
            "s" => Arg::Str(rest),
            "b" => Arg::Bool(rest == "1"),
            "z" => Arg::Nil,
            _ => panic!("bad arg token {tok:?}"),
        };
        let ours = format(fmt, &[arg]);
        t.check(ours.as_deref() == Ok(want), || {
            format!("{fmt} <- {tok}: want {want:?} got {ours:?}")
        });
    }
    let n = t.assert("format");
    assert!(n >= 600, "{n}");
}

fn unhex(h: &str) -> String {
    let h = h.strip_prefix('x').unwrap_or(h);
    let bytes: Vec<u8> = (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
        .collect();
    String::from_utf8(bytes).expect("tonumber corpus is ASCII")
}

#[test]
fn tonumber_is_lj_strscan() {
    let fx = fixture();
    let mut t = Tally::new();
    for r in rows(&fx, "tonumber") {
        let s = unhex(r[0]);
        let ours = tonumber(&s);
        let ok = match (r[1], ours) {
            ("nil", None) => true,
            ("nil", Some(_)) | (_, None) => false,
            (w, Some(v)) => same(out(w), v),
        };
        t.check(ok, || format!("{s:?}: want {} got {ours:?}", r[1]));
    }
    let n = t.assert("tonumber");
    assert!(n >= 340, "{n}");
}

#[test]
fn math_helpers_match_luajit() {
    let fx = fixture();
    let mut t = Tally::new();
    for r in rows(&fx, "math") {
        let (a, b) = (num(r[0]), num(r[1]));
        let dec = (r[2] != "nil").then(|| r[2].parse::<f64>().unwrap());
        let (ip, fp) = number::modf(a);
        let ours = [
            ("min", number::min2(a, b)),
            ("max", number::max2(a, b)),
            ("mod", number::modulo(a, b)),
            ("modf.ip", ip),
            ("modf.fp", fp),
            ("round", number::round(a, dec)),
            ("floor", number::floor(a, dec)),
        ];
        for (k, (name, v)) in ours.into_iter().enumerate() {
            let want = out(r[3 + k]);
            t.check(same(want, v), || {
                format!("{name}(a={} b={} dec={}): want {} got {}", r[0], r[1], r[2], r[3 + k], show(v))
            });
        }
    }
    let n = t.assert("math");
    assert_eq!(n, 560 * 7);
}

fn libm(name: &str, a: f64, b: f64) -> f64 {
    match name {
        "^" | "pow" => a.powf(b),
        "sqrt" => a.sqrt(),
        "log" => a.ln(),
        "sin" => a.sin(),
        "cos" => a.cos(),
        "tan" => a.tan(),
        "ceil" => a.ceil(),
        "floor" => a.floor(),
        "abs" => a.abs(),
        _ => panic!("unknown libm fn {name:?}"),
    }
}

/// libm is CRT-dependent (MINEFIELD-RUST R2): the truth is the Windows desktop LuaJIT (PoB runtime/lua51.dll, UCRT),
/// captured by tools/lua-corpus/export-libm-desktop.ts. The docker image is Alpine/musl -- its answers differ for
/// sin/cos/tan/pow in the last bit; those rows are counted as a known platform class, never used as the gate.
#[test]
fn libm_matches_desktop_luajit() {
    let fx = fixture();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/desktop-libm.json");
    let desk: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let docker = rows(&fx, "libm");
    let desktop = rows(&desk, "libm");
    assert_eq!(docker.len(), desktop.len());
    let mut t = Tally::new();
    let mut musl_differs: std::collections::BTreeMap<&str, usize> = Default::default();
    for (d, w) in docker.iter().zip(&desktop) {
        assert_eq!((d[0], d[1], d[2]), (w[0], w[1], w[2]), "fixtures out of step");
        let (name, a) = (w[0], num(w[1]));
        let b = if w[2].is_empty() { 0.0 } else { num(w[2]) };
        let ours = libm(name, a, b);
        t.check(same(num(w[3]), ours), || {
            format!("{name}({}, {}): desktop {} ours {}", w[1], w[2], w[3], show(ours))
        });
        // Docker interpreter vs JIT must agree with each other (it is the same libm either way).
        assert!(same(out(d[3]), out(d[4])), "docker interp/jit differ: {d:?}");
        if !same(out(d[3]), ours) {
            *musl_differs.entry(name).or_default() += 1;
        }
    }
    eprintln!("libm: docker (musl) differs from desktop/ours in {musl_differs:?}");
    let n = t.assert("libm");
    assert_eq!(n, 7720);
}
