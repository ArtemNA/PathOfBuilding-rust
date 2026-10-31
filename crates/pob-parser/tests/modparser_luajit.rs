//! The parser against LuaJIT (R5.1 fixtures `tests/fixtures/modparser-*.json`: the INNER `parseMod` of the oracle, via
//! upvalue, on the ModCache lines + golden tree lines + golden item misses). Per line, in fixture order (LuaJIT ran them in
//! that order in one process -- the DOUBLED write of ModParser.lua:6862 is state): `inner1` = `parseMod(line, 1)` and
//! `wrapped` = the wrapper logic without the cache. Equality is bit for bit on the fixture's encoding (numbers `%.17g`,
//! `-0` kept, tables `{a, h}`, L9 orphans as `other`). A line whose path needs an unported closure is counted per closure id
//! (the R5.3 metric); every other line must be equal.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pob_data::wire::OrphanTag;
use pob_mods::{Mod, Value};
use pob_parser::scan::{reset_tie_stats, tie_stats};
use pob_parser::{ParseError, RawParse, parse_mod_raw, parse_mod_uncached, parser_stats, reset_parser_stats};

// --- a minimal JSON reader (the crate has no serde_json dependency) -------------------------------------------------

#[derive(Debug, PartialEq)]
enum J {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(BTreeMap<String, J>),
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn val(&mut self) -> J {
        self.ws();
        match self.s[self.i] {
            b'{' => {
                self.i += 1;
                let mut m = BTreeMap::new();
                loop {
                    self.ws();
                    if self.s[self.i] == b'}' {
                        self.i += 1;
                        return J::Obj(m);
                    }
                    let J::Str(k) = self.val() else { panic!("key") };
                    self.ws();
                    assert_eq!(self.s[self.i], b':');
                    self.i += 1;
                    m.insert(k, self.val());
                    self.ws();
                    if self.s[self.i] == b',' {
                        self.i += 1;
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                loop {
                    self.ws();
                    if self.s[self.i] == b']' {
                        self.i += 1;
                        return J::Arr(a);
                    }
                    a.push(self.val());
                    self.ws();
                    if self.s[self.i] == b',' {
                        self.i += 1;
                    }
                }
            }
            b'"' => {
                self.i += 1;
                let mut out = String::new();
                loop {
                    let c = self.s[self.i];
                    self.i += 1;
                    match c {
                        b'"' => return J::Str(out),
                        b'\\' => {
                            let e = self.s[self.i];
                            self.i += 1;
                            out.push(match e {
                                b'n' => '\n',
                                b't' => '\t',
                                b'r' => '\r',
                                b'b' => '\x08',
                                b'f' => '\x0c',
                                b'u' => {
                                    let h = std::str::from_utf8(&self.s[self.i..self.i + 4]).unwrap();
                                    self.i += 4;
                                    char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap()
                                }
                                other => other as char,
                            });
                        }
                        _ => {
                            // fixture strings are ASCII-escaped; keep any raw UTF-8 as is
                            let start = self.i - 1;
                            let mut end = self.i;
                            while end < self.s.len() && self.s[end] >= 0x80 {
                                end += 1;
                            }
                            out.push_str(std::str::from_utf8(&self.s[start..end]).unwrap());
                            self.i = end;
                        }
                    }
                }
            }
            b't' => {
                self.i += 4;
                J::Bool(true)
            }
            b'f' => {
                self.i += 5;
                J::Bool(false)
            }
            b'n' => {
                self.i += 4;
                J::Null
            }
            _ => {
                let st = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                J::Num(String::from_utf8(self.s[st..self.i].to_vec()).unwrap())
            }
        }
    }
}

fn obj(pairs: Vec<(&str, J)>) -> J {
    J::Obj(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

// --- our answer in the fixture's encoding ---------------------------------------------------------------------------

/// A Rust (UTF-8) string as the fixture's byte string: one char per byte.
fn bstr(s: &str) -> J {
    J::Str(s.bytes().map(char::from).collect())
}

fn fmt17(n: f64) -> String {
    if n.is_nan() {
        "nan".into()
    } else if n.is_infinite() {
        if n > 0.0 { "inf".into() } else { "-inf".into() }
    } else if n == 0.0 {
        if n.is_sign_negative() { "-0".into() } else { "0".into() }
    } else {
        pob_lua::format_g(n, 17)
    }
}

fn num(n: f64) -> J {
    obj(vec![("n", J::Str(fmt17(n)))])
}

/// `intOf`: a flag set as a plain integer.
fn int(n: u32) -> J {
    J::Num(n.to_string())
}

fn enc(v: &Value) -> J {
    match v {
        Value::Nil => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::Num(n) => num(*n),
        Value::Str(s) => bstr(s),
        Value::Table(t) => {
            let mut o = Vec::new();
            if !t.array.is_empty() {
                o.push(("a", J::Arr(t.array.iter().map(enc).collect())));
            }
            if !t.hash.is_empty() {
                o.push(("h", J::Obj(t.hash.iter().map(|(k, x)| (k.to_string(), enc(x))).collect())));
            }
            obj(o)
        }
        Value::Mod(m) => {
            // a nested mod is a plain table to the encoder: hash fields + tags in the array part
            let mut h = vec![
                ("name".to_owned(), bstr(&m.name)),
                ("flags".to_owned(), num(f64::from(m.flags))),
                ("keywordFlags".to_owned(), num(f64::from(m.keyword_flags))),
            ];
            if let Some(t) = m.ty {
                h.push(("type".to_owned(), bstr(t.as_str())));
            }
            if !m.value.is_nil() {
                h.push(("value".to_owned(), enc(&m.value)));
            }
            if let Some(s) = &m.source {
                h.push(("source".to_owned(), bstr(s)));
            }
            let mut o = vec![("h", J::Obj(h.into_iter().collect()))];
            if !m.tags.is_empty() {
                o.push(("a", J::Arr(m.tags.iter().map(|t| enc(&Value::Table(t.table()))).collect())));
            }
            obj(o)
        }
    }
}

fn enc_mod(m: &Mod, orphans: &[&OrphanTag]) -> J {
    let mut o = vec![
        ("name", bstr(&m.name)),
        ("flags", int(m.flags)),
        ("keywordFlags", int(m.keyword_flags)),
        ("tags", J::Arr(m.tags.iter().map(|t| enc(&Value::Table(t.table()))).collect())),
    ];
    if let Some(t) = m.ty {
        o.push(("type", bstr(t.as_str())));
    }
    if !m.value.is_nil() {
        o.push(("value", enc(&m.value)));
    }
    if let Some(s) = &m.source {
        o.push(("source", bstr(s)));
    }
    if !orphans.is_empty() {
        let other = orphans
            .iter()
            .map(|t| (format!("#{}", t.index), enc(&Value::Table(t.tag.table()))))
            .collect();
        o.push(("other", J::Obj(other)));
    }
    obj(o)
}

fn enc_parse(r: &RawParse) -> J {
    let mods = match &r.mod_list {
        None => J::Null,
        Some(l) => J::Arr(
            l.iter()
                .enumerate()
                .map(|(i, m)| {
                    let o: Vec<&OrphanTag> = r.orphan_tags.iter().filter(|(j, _)| *j == i).map(|(_, t)| t).collect();
                    enc_mod(m, &o)
                })
                .collect(),
        ),
    };
    obj(vec![("mods", mods), ("extra", r.extra.as_deref().map_or(J::Null, bstr))])
}

/// The fixture side without `fm` (formatMod text -- not part of the gate).
fn strip(j: &J) -> J {
    match j {
        J::Obj(m) => J::Obj(
            m.iter()
                .filter(|(k, _)| k.as_str() != "fm")
                .map(|(k, v)| {
                    (
                        k.clone(),
                        match v {
                            J::Obj(_) | J::Arr(_) => strip(v),
                            other => clone(other),
                        },
                    )
                })
                .collect(),
        ),
        J::Arr(a) => J::Arr(a.iter().map(strip).collect()),
        other => clone(other),
    }
}

fn clone(j: &J) -> J {
    match j {
        J::Null => J::Null,
        J::Bool(b) => J::Bool(*b),
        J::Num(s) => J::Num(s.clone()),
        J::Str(s) => J::Str(s.clone()),
        J::Arr(a) => J::Arr(a.iter().map(clone).collect()),
        J::Obj(m) => J::Obj(m.iter().map(|(k, v)| (k.clone(), clone(v))).collect()),
    }
}

fn fixtures() -> Vec<PathBuf> {
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    v.retain(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("modparser-") && n.ends_with(".json"))
    });
    v.sort();
    v
}

/// Lines where the DATA differs from upstream, not the parser: `data/mod-parser.json` (pob-ts converter P5.1a,
/// `normalizeMod` -> `JSON.stringify`) writes `math.huge` as `null`, so the static `specialModList` mod has no value
/// (ModParser.lua:2454, :4559 `mod(..., m_huge, ...)`). The pin is exact: the only difference allowed on these lines is
/// LuaJIT's first mod `value = {n: "inf"}` against no value here. `data/mod-cache.json` has the same two nulls.
const DATA_INF_AS_NULL: [&str; 2] = [
    "Projectiles can Chain from any number of additional targets in Close Range",
    "Your Hexes have infinite Duration",
];

/// `want` with the first mod's `value` dropped when it is `{n: "inf"}` (the DATA_INF_AS_NULL pin).
fn without_inf_value(want: &J) -> Option<J> {
    let J::Obj(o) = strip(want) else { return None };
    let mut o = o;
    let Some(J::Arr(mods)) = o.get_mut("mods") else { return None };
    let J::Obj(m) = mods.first_mut()? else { return None };
    (m.remove("value")? == obj(vec![("n", J::Str("inf".into()))])).then_some(J::Obj(o))
}

#[derive(Default)]
struct Side {
    equal: usize,
    data_pinned: usize,
    unported: BTreeMap<String, usize>,
    unported_lines: usize,
    mismatches: Vec<String>,
}

impl Side {
    fn check(&mut self, line: &str, got: Result<RawParse, ParseError>, want: &J) {
        match got {
            Err(ParseError::Unported(id)) => {
                self.unported_lines += 1;
                *self.unported.entry(id.to_string()).or_default() += 1;
            }
            Err(e) => self.mismatches.push(format!("{line:?}: Rust error {e}; LuaJIT {want:?}")),
            Ok(r) => {
                let g = enc_parse(&r);
                if g == strip(want) {
                    self.equal += 1;
                } else if DATA_INF_AS_NULL.contains(&line) && without_inf_value(want).is_some_and(|w| w == g) {
                    self.data_pinned += 1;
                } else {
                    self.mismatches.push(format!("{line:?}:\n   rust {g:?}\n luajit {:?}", strip(want)));
                }
            }
        }
    }
}

#[test]
fn parser_equals_luajit_inner_parse_mod() {
    let files = fixtures();
    if files.is_empty() {
        eprintln!("SKIP: no tests/fixtures/modparser-*.json -- the LuaJIT corpus comes from task R5.1");
        return;
    }
    reset_parser_stats();
    reset_tie_stats();
    let (mut inner, mut wrapped) = (Side::default(), Side::default());
    let mut cache_equal = 0;
    let mut lines = 0;
    let mut sources: BTreeMap<String, usize> = BTreeMap::new();
    for f in &files {
        let text = std::fs::read(f).unwrap();
        let J::Obj(root) = (P { s: &text, i: 0 }).val() else {
            panic!("{f:?}")
        };
        let Some(J::Arr(recs)) = root.get("lines") else {
            panic!("{f:?}: no lines")
        };
        for rec in recs {
            let J::Obj(r) = rec else { panic!() };
            let Some(J::Str(raw)) = r.get("line") else { panic!() };
            let bytes: Vec<u8> = raw.chars().map(|c| u8::try_from(u32::from(c)).expect("byte string")).collect();
            let line = String::from_utf8(bytes).expect("a UTF-8 line");
            lines += 1;
            let in_cache = matches!(r.get("sources"), Some(J::Arr(s)) if s.contains(&J::Str("cache".into())));
            if let Some(J::Arr(s)) = r.get("sources") {
                for x in s {
                    if let J::Str(x) = x {
                        *sources.entry(x.clone()).or_default() += 1;
                    }
                }
            }
            inner.check(&line, parse_mod_raw(&line, 1), &r["inner1"]);
            let before = wrapped.equal;
            wrapped.check(&line, parse_mod_uncached(&line), &r["wrapped"]);
            cache_equal += usize::from(in_cache && wrapped.equal > before);
        }
    }
    for (name, s) in [("inner1", &inner), ("wrapped", &wrapped)] {
        let mut top: Vec<_> = s.unported.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        println!(
            "{name}: {lines} lines {sources:?}: = LuaJIT {}, data-pinned {}, unported {} ({} closure ids), mismatches {}",
            s.equal,
            s.data_pinned,
            s.unported_lines,
            s.unported.len(),
            s.mismatches.len()
        );
        println!("{name} top unported: {:?}", &top[..top.len().min(10)]);
        for m in s.mismatches.iter().take(15) {
            println!("MISMATCH {name} {m}");
        }
    }
    let ts = tie_stats();
    let ps = parser_stats();
    println!(
        "ModCache lines = LuaJIT (wrapped): {cache_equal}; scan ties {} (values differ {}, captures differ {}) of {} scans; jewel multi-match {}; DOUBLED writes {}",
        ts.ties,
        ts.diff_value,
        ts.diff_caps,
        ts.scans,
        ps.jewel_multi.len(),
        ps.doubled_writes.len()
    );
    assert!(
        inner.mismatches.is_empty() && wrapped.mismatches.is_empty(),
        "Rust != LuaJIT (see output)"
    );
    for s in [&inner, &wrapped] {
        assert_eq!(s.equal + s.data_pinned + s.unported_lines, lines);
        assert_eq!(s.data_pinned, DATA_INF_AS_NULL.len());
    }
}
