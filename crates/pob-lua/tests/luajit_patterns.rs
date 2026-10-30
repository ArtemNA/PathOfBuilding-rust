//! pob-lua Lua patterns and `bit.*` against desktop LuaJIT (`tools/lua-corpus/export-patterns.ts`, R1.5):
//! A -- every ModParser pattern x a sample of mod lines through `find` (all `calls`, not only the hits);
//! B -- the grammar fuzzer (find / find plain / match / gmatch / gsub with string, table and function replacements);
//! C -- `bit.*`. Byte strings are tokens: "s" + printable ASCII, or "x" + hex.

use std::collections::HashMap;

use pob_lua::number;
use pob_lua::{Capture, Captures, PatternError, Repl, find, gmatch, gsub, match_};
use serde_json::{Value, json};

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap()
}

fn unhex(h: &str) -> Vec<u8> {
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
        .collect()
}

/// Token -> bytes.
fn bytes(tok: &Value) -> Vec<u8> {
    let t = tok.as_str().unwrap_or_else(|| panic!("not a byte token: {tok}"));
    match t.split_at(1) {
        ("s", rest) => rest.as_bytes().to_vec(),
        ("x", rest) => unhex(rest),
        _ => panic!("bad byte token {t:?}"),
    }
}

/// Bytes -> token (`tk` of the exporter).
fn tk(b: &[u8]) -> Value {
    if b.iter().all(|&c| (0x20..=0x7e).contains(&c)) {
        Value::String(format!("s{}", std::str::from_utf8(b).unwrap()))
    } else {
        Value::String(format!("x{}", b.iter().map(|c| format!("{c:02x}")).collect::<String>()))
    }
}

fn n(v: usize) -> Value {
    Value::String(format!("n{v}"))
}

/// A capture as a fuzz result token (positions are "n"-numbers).
fn cap_tok(s: &[u8], c: Capture) -> Value {
    match c {
        Capture::Str { start, end } => tk(&s[start..end]),
        Capture::Pos(p) => n(p),
    }
}

fn caps_toks(s: &[u8], caps: &Captures) -> Vec<Value> {
    caps.iter().map(|c| cap_tok(s, c)).collect()
}

fn err(e: &PatternError) -> Value {
    json!(["ERR", tk(e.0.as_bytes())])
}

// --- A ----------------------------------------------------------------------------------------------

struct CorpusA {
    patterns: Vec<(Vec<u8>, bool)>,
    lines: Vec<Vec<u8>>,
    hits: HashMap<(usize, usize), Value>,
    calls: usize,
    hit_count: usize,
}

fn corpus_a() -> CorpusA {
    let fx = fixture("luajit-patterns-modparser.json");
    assert!(fx["errors"].as_array().unwrap().is_empty(), "fixture A has pattern errors");
    let patterns: Vec<(Vec<u8>, bool)> = fx["patterns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (bytes(&r[0]), r[1].as_bool().unwrap_or(false)))
        .collect();
    let lines: Vec<Vec<u8>> = fx["lines"].as_array().unwrap().iter().map(bytes).collect();
    let mut hits = HashMap::new();
    for h in fx["hits"].as_array().unwrap() {
        let h = h.as_array().unwrap();
        let key = (h[0].as_u64().unwrap() as usize, h[1].as_u64().unwrap() as usize);
        assert!(hits.insert(key, Value::Array(h[2..].to_vec())).is_none(), "duplicate hit {key:?}");
    }
    let calls = fx["calls"].as_u64().unwrap() as usize;
    let hit_count = fx["hitCount"].as_u64().unwrap() as usize;
    assert_eq!(calls, patterns.len() * lines.len());
    assert_eq!(hit_count, hits.len());
    CorpusA {
        patterns,
        lines,
        hits,
        calls,
        hit_count,
    }
}

/// Our `find` in the fixture's shape: `[start, end, cap1..cap5]` (positions as JSON numbers, at most 5 captures as `scan`).
fn a_result(line: &[u8], pat: &[u8], plain: bool) -> Option<Value> {
    let (a, b, caps) = find(line, pat, Some(1), plain).unwrap_or_else(|e| panic!("{:?}: {e}", String::from_utf8_lossy(pat)))?;
    let mut v = vec![json!(a), json!(b)];
    for c in caps.iter().take(5) {
        v.push(match c {
            Capture::Str { start, end } => tk(&line[start..end]),
            Capture::Pos(p) => json!(p),
        });
    }
    Some(Value::Array(v))
}

fn run_a(step: usize) -> (usize, usize) {
    let a = corpus_a();
    let (mut calls, mut hits, mut bad) = (0, 0, Vec::new());
    for (pi, (pat, plain)) in a.patterns.iter().enumerate().step_by(step) {
        for (li, line) in a.lines.iter().enumerate() {
            calls += 1;
            let got = a_result(line, pat, *plain);
            let want = a.hits.get(&(pi, li));
            hits += usize::from(want.is_some());
            if got.as_ref() != want && bad.len() < 20 {
                bad.push(format!(
                    "p{pi} {:?} plain={plain} l{li} {:?}: got {got:?} want {want:?}",
                    String::from_utf8_lossy(pat),
                    String::from_utf8_lossy(line)
                ));
            }
        }
    }
    assert!(bad.is_empty(), "A: mismatches (first 20):\n{}", bad.join("\n"));
    if step == 1 {
        assert_eq!((calls, hits), (a.calls, a.hit_count));
    }
    println!("A step {step}: {calls} calls, {hits} hits, 0 mismatches");
    (calls, hits)
}

/// Fast part for the plain `cargo test`: every 16th pattern x all lines.
#[test]
fn a_modparser_sample() {
    let (calls, _) = run_a(16);
    assert!(calls >= 50_000, "{calls}");
}

/// All `calls` of fixture A; slow without optimisations -- run with `--release`.
#[test]
#[cfg_attr(debug_assertions, ignore)]
fn a_modparser_full() {
    let t = std::time::Instant::now();
    run_a(1);
    println!("A full pass: {:?}", t.elapsed());
}

// --- B ----------------------------------------------------------------------------------------------

fn init(v: &Value) -> Option<i64> {
    v.as_i64()
}

fn fuzz_case(input: &[Value]) -> Value {
    let op = input[0].as_str().unwrap();
    let s = bytes(&input[1]);
    let p = bytes(&input[2]);
    let s = s.as_slice();
    match op {
        "find" | "findp" => match find(s, &p, init(&input[3]), op == "findp") {
            Err(e) => err(&e),
            Ok(None) => json!([null]),
            Ok(Some((a, b, caps))) => {
                let mut v = vec![n(a), n(b)];
                v.extend(caps_toks(s, &caps));
                Value::Array(v)
            }
        },
        "match" => match match_(s, &p, init(&input[3])) {
            Err(e) => err(&e),
            Ok(None) => json!([null]),
            Ok(Some(caps)) => Value::Array(caps_toks(s, &caps)),
        },
        "gmatch" => {
            let mut groups = Vec::new();
            for r in gmatch(s, &p).take(50) {
                match r {
                    Err(e) => return err(&e),
                    Ok(caps) => groups.push(Value::Array(caps_toks(s, &caps))),
                }
            }
            Value::Array(groups)
        }
        "gsub_s" => {
            let repl = bytes(&input[3]);
            gsub_result(gsub(s, &p, Repl::Str(&repl), init(&input[4])), None)
        }
        "gsub_t" => {
            // TBL = { a = "X", ["1"] = "Y" }, indexed by the first capture (or the whole match); a number key finds nothing.
            let mut f = |s: &[u8], caps: &Captures| match caps.get(0).and_then(|c| c.bytes(s)) {
                Some(b"a") => Some(b"X".to_vec()),
                Some(b"1") => Some(b"Y".to_vec()),
                _ => None,
            };
            gsub_result(gsub(s, &p, Repl::Fn(&mut f), init(&input[3])), None)
        }
        "gsub_f" => {
            let mode = input[3].as_str().unwrap();
            let mut index_error = false;
            let mut f = |s: &[u8], caps: &Captures| match mode {
                // function(c) return c:upper() end -- indexing a number (position capture) raises
                "upper" => match caps.get(0).unwrap() {
                    Capture::Str { start, end } => Some(s[start..end].to_ascii_uppercase()),
                    Capture::Pos(_) => {
                        index_error = true;
                        None
                    }
                },
                _ => None, // nil / false keep the original text
            };
            let r = gsub(s, &p, Repl::Fn(&mut f), init(&input[4]));
            gsub_result(r, index_error.then_some("attempt to index local 'c' (a number value)"))
        }
        _ => panic!("unknown op {op}"),
    }
}

fn gsub_result(r: Result<(Vec<u8>, usize), PatternError>, lua_error: Option<&str>) -> Value {
    match r {
        Err(e) => err(&e),
        Ok(_) if lua_error.is_some() => json!(["ERR", tk(lua_error.unwrap().as_bytes())]),
        Ok((out, count)) => json!([tk(&out), n(count)]),
    }
}

/// Error text without the `bad argument #N to '...'` prefix (the gate for errors is the fact; texts are reported).
fn err_core(v: &Value) -> Option<String> {
    let a = v.as_array()?;
    if a.len() != 2 || a[0] != "ERR" {
        return None;
    }
    let t = String::from_utf8(bytes(&a[1])).unwrap();
    Some(match t.find("' (") {
        Some(i) if t.starts_with("bad argument #") => t[i + 3..t.len() - 1].to_owned(),
        _ => t,
    })
}

#[test]
fn b_fuzz() {
    let fx = fixture("luajit-patterns-fuzz.json");
    let cases = fx["cases"].as_array().unwrap();
    assert!(cases.len() >= 20_000, "{}", cases.len());
    let (mut errs, mut err_text_same, mut bad) = (0, 0, Vec::new());
    let mut per_op: HashMap<String, usize> = HashMap::new();
    for case in cases {
        let input = case[0].as_array().unwrap();
        let want = &case[1];
        *per_op.entry(input[0].as_str().unwrap().to_owned()).or_default() += 1;
        let got = fuzz_case(input);
        match (err_core(&got), err_core(want)) {
            (Some(g), Some(w)) => {
                errs += 1;
                if got == *want {
                    err_text_same += 1;
                } else if g != w && bad.len() < 30 {
                    bad.push(format!("{input:?}: error text got {got} want {want}"));
                }
            }
            _ => {
                if got != *want && bad.len() < 30 {
                    bad.push(format!("{input:?}: got {got} want {want}"));
                }
            }
        }
    }
    println!(
        "B: {} cases {per_op:?}; errors {errs}, same text {err_text_same}/{errs}",
        cases.len()
    );
    assert!(bad.is_empty(), "B: mismatches (first 30):\n{}", bad.join("\n"));
}

// --- C ----------------------------------------------------------------------------------------------

fn bit_num(tok: &str) -> f64 {
    match tok {
        "nan" => f64::NAN,
        "inf" => f64::INFINITY,
        "-inf" => f64::NEG_INFINITY,
        _ => tok.parse().unwrap_or_else(|_| panic!("bad number {tok:?}")),
    }
}

#[test]
fn c_bit() {
    let fx = fixture("luajit-bit.json");
    let rows = fx["cases"].as_array().unwrap();
    let mut bad = Vec::new();
    for r in rows {
        let r: Vec<&str> = r.as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
        let a = bit_num(r[1]);
        let b = || bit_num(r[2]);
        let v = match r[0] {
            "band" => number::band(a, b()),
            "bor" => number::bor(a, b()),
            "bxor" => number::bxor(a, b()),
            "bnot" => number::bnot(a),
            "tobit" => f64::from(number::tobit(a)),
            "lshift" => number::lshift(a, b()),
            "rshift" => number::rshift(a, b()),
            "arshift" => number::arshift(a, b()),
            op => panic!("unknown op {op}"),
        };
        let got = format!("{v}");
        if got != r[3] && bad.len() < 20 {
            bad.push(format!("{r:?}: got {got}"));
        }
    }
    assert!(bad.is_empty(), "C: mismatches:\n{}", bad.join("\n"));
    assert!(rows.len() >= 1_500);
}

// --- hand-written edges ------------------------------------------------------------------------------

fn m(s: &str, p: &str) -> Option<Vec<String>> {
    match_(s.as_bytes(), p.as_bytes(), None).unwrap().map(|c| {
        c.iter()
            .map(|c| match c {
                Capture::Str { start, end } => s[start..end].to_owned(),
                Capture::Pos(p) => format!("@{p}"),
            })
            .collect()
    })
}

fn e(s: &str, p: &str) -> String {
    match_(s.as_bytes(), p.as_bytes(), None).unwrap_err().0
}

#[test]
fn edges() {
    // empty pattern / empty subject
    assert_eq!(find(b"abc", b"", None, false).unwrap().map(|r| (r.0, r.1)), Some((1, 0)));
    assert_eq!(find(b"", b"", None, false).unwrap().map(|r| (r.0, r.1)), Some((1, 0)));
    assert_eq!(m("", ".*"), Some(vec![String::new()]));
    assert_eq!(m("", "."), None);
    // init beyond the end clamps to #s + 1 (non-LJ_52)
    assert_eq!(find(b"abc", b"", Some(10), false).unwrap().map(|r| (r.0, r.1)), Some((4, 3)));
    // %b
    assert_eq!(m("x(a(b)c)y", "%b()"), Some(vec!["(a(b)c)".into()]));
    assert_eq!(m("x(a(b c", "%b()"), None);
    assert_eq!(e("x", "%b("), "unbalanced pattern");
    // %f at the edges: the subject is bracketed by NULs
    assert_eq!(m("THE (quick) fox", "%f[%a]%a+"), Some(vec!["THE".into()]));
    assert_eq!(m("abc", "%f[%z]()"), Some(vec!["@4".into()]));
    assert_eq!(m("abc", "%f[%l]()"), Some(vec!["@1".into()]));
    assert_eq!(e("abc", "%fx"), "missing '[' after '%f' in pattern");
    // - vs *
    assert_eq!(m("<a><b>", "<(.-)>"), Some(vec!["a".into()]));
    assert_eq!(m("<a><b>", "<(.*)>"), Some(vec!["a><b".into()]));
    // captures: position, back-reference, back-reference to a position capture never matches
    assert_eq!(m("hello", "()ll()"), Some(vec!["@3".into(), "@5".into()]));
    assert_eq!(m("abab", "(ab)%1"), Some(vec!["ab".into()]));
    assert_eq!(m("aa", "()%1"), None);
    // errors
    assert_eq!(e("a", "[a"), "malformed pattern (missing ']')");
    assert_eq!(e("a", "a%"), "malformed pattern (ends with '%')");
    assert_eq!(e("a", "a)"), "invalid pattern capture");
    assert_eq!(e("a", "(a"), "unfinished capture");
    assert_eq!(e("a", "%1"), "invalid capture index");
    assert_eq!(e("a", &"(".repeat(33)), "too many captures");
    // LJ_MAX_XLEVEL = 200 nested match() calls: each matched `a?` nests one level
    let a300 = "a".repeat(300);
    assert_eq!(e(&a300, &"a?".repeat(200)), "pattern too complex");
    assert!(match_(a300.as_bytes(), "a?".repeat(199).as_bytes(), None).is_ok());
    // classes are ASCII (C locale)
    assert_eq!(m("\u{e9}a", "%a+"), Some(vec!["a".into()]));
    // gsub: %x literal (W15), anchors, empty matches, max_n
    let g = |s: &str, p: &str, r: &str, n: Option<i64>| {
        let (o, c) = gsub(s.as_bytes(), p.as_bytes(), Repl::Str(r.as_bytes()), n).unwrap();
        (String::from_utf8(o).unwrap(), c)
    };
    assert_eq!(g("abc", "", "-", None), ("-a-b-c-".into(), 4));
    assert_eq!(g("hello world", "o", "%x%%", Some(1)), ("hellx% world".into(), 1));
    assert_eq!(g("aaa", "^a", "b", None), ("baa".into(), 1));
    assert_eq!(g("abc", "%w", "%0%0", None), ("aabbcc".into(), 3));
    // gmatch: '^' is not an anchor
    assert_eq!(gmatch(b"a^a", b"^a").count(), 1);
}

/// Recursion / capture limits, pinned on the desktop LuaJIT (PoB runtime/lua51.dll via tools/luajit-win, 2026-10-31):
/// `find(("a"):rep(300), ("a?"):rep(n))` -> 1 for n <= 199, "pattern too complex" from 200; `("("):rep(n)..(")"):rep(n)`
/// -> "too many captures" for n in 195..=205; `match(("ab"):rep(150), ("a*b"):rep(n))` -> nil (fails before the limit).
#[test]
fn depth_and_capture_limits_match_desktop() {
    let s = b"a".repeat(300);
    for n in 195..=205 {
        let r = pob_lua::find(&s, &b"a?".repeat(n), None, false);
        if n <= 199 {
            assert_eq!(r.as_ref().map(|o| o.map(|(a, _, _)| a)), Ok(Some(1)), "a? x{n}");
        } else {
            assert_eq!(r.err().map(|e| e.0), Some("pattern too complex".to_owned()), "a? x{n}");
        }
        let caps = [b"(".repeat(n), b")".repeat(n)].concat();
        assert_eq!(
            pob_lua::find(&s, &caps, None, false).err().map(|e| e.0),
            Some("too many captures".to_owned()),
            "() x{n}"
        );
        let ab = b"ab".repeat(150);
        assert_eq!(pob_lua::match_(&ab, &b"a*b".repeat(n), None), Ok(None), "a*b x{n}");
    }
}
