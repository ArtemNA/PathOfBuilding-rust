//! Thin byte-level wrappers over `pob_lua::pattern` for the call shapes Item.lua / ItemTools.lua use (L13, L16): every
//! pattern here is upstream's Lua pattern verbatim, never a regex. Captures come back as strings (the patterns only ever
//! capture whole runs bounded by ASCII bytes, so no multi-byte character is split).

use pob_lua::{Captures, Repl};

fn to_string(v: Vec<u8>) -> String {
    String::from_utf8(v).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn caps_of(subject: &[u8], caps: &Captures) -> Vec<String> {
    caps.iter()
        .map(|c| match c.bytes(subject) {
            Some(b) => String::from_utf8_lossy(b).into_owned(),
            None => match c {
                pob_lua::Capture::Pos(n) => n.to_string(),
                pob_lua::Capture::Str { .. } => unreachable!(),
            },
        })
        .collect()
}

/// `s:find(p)` as a boolean (pattern find; a malformed pattern is the Lua error, raised with `site`).
pub fn find(s: &str, p: &str, site: &str) -> bool {
    match pob_lua::find(s.as_bytes(), p.as_bytes(), None, false) {
        Ok(r) => r.is_some(),
        Err(e) => panic!("{site}: {}", e.0),
    }
}

/// `s:find(p, 1, true)` -- 1-based inclusive `(start, end)`.
pub fn find_pattern(s: &str, p: &str) -> Option<(usize, usize)> {
    pob_lua::find(s.as_bytes(), p.as_bytes(), None, false)
        .ok()
        .flatten()
        .map(|c| (c.0.saturating_sub(1), c.1))
}

pub fn find_plain(s: &str, p: &str) -> Option<(usize, usize)> {
    pob_lua::find(s.as_bytes(), p.as_bytes(), None, true)
        .expect("plain find cannot fail")
        .map(|(a, e, _)| (a, e))
}

/// `s:match(p)`: the captures (or the whole match) as strings.
pub fn match_(s: &str, p: &str, site: &str) -> Option<Vec<String>> {
    match pob_lua::match_(s.as_bytes(), p.as_bytes(), None) {
        Ok(r) => r.map(|c| caps_of(s.as_bytes(), &c)),
        Err(e) => panic!("{site}: {}", e.0),
    }
}

/// `s:match(p)` -> the first capture.
pub fn match1(s: &str, p: &str) -> Option<String> {
    match_(s, p, "pattern").map(|mut v| v.swap_remove(0))
}

/// `s:match(p) ~= nil` -- the same test as `s:find(p) ~= nil`, without building the capture strings.
pub fn matches(s: &str, p: &str) -> bool {
    find(s, p, "pattern")
}

/// `for ... in s:gmatch(p)`: each match's captures.
pub fn gmatch(s: &str, p: &str) -> Vec<Vec<String>> {
    pob_lua::gmatch(s.as_bytes(), p.as_bytes())
        .map(|r| caps_of(s.as_bytes(), &r.expect("gmatch pattern")))
        .collect()
}

/// `s:gsub(p, repl, n)` with a replacement STRING (`%1`, `%%` processed like Lua).
pub fn gsub(s: &str, p: &str, repl: &str, n: Option<i64>) -> String {
    let (out, _) = pob_lua::gsub(s.as_bytes(), p.as_bytes(), Repl::Str(repl.as_bytes()), n).expect("gsub pattern");
    to_string(out)
}

/// `s:gsub(p, repl)` -> `(string, count)`.
pub fn gsub_count(s: &str, p: &str, repl: &str) -> (String, usize) {
    let (out, n) = pob_lua::gsub(s.as_bytes(), p.as_bytes(), Repl::Str(repl.as_bytes()), None).expect("gsub pattern");
    (to_string(out), n)
}

/// `s:gsub(p, function(...) ... end, n)`: `f` gets the captures; `None` keeps the matched text (Lua nil/false).
pub fn gsub_fn(s: &str, p: &str, n: Option<i64>, mut f: impl FnMut(&[String]) -> Option<String>) -> String {
    let mut cb = |subject: &[u8], caps: &Captures| -> Option<Vec<u8>> { f(&caps_of(subject, caps)).map(String::into_bytes) };
    let (out, _) = pob_lua::gsub(s.as_bytes(), p.as_bytes(), Repl::Fn(&mut cb), n).expect("gsub pattern");
    to_string(out)
}

/// `string.lower` (ASCII only, L16).
pub fn lower(s: &str) -> String {
    pob_lua::string::lower(s)
}

/// `tonumber(s)` (L3).
pub fn tonumber(s: &str) -> Option<f64> {
    pob_lua::tonumber(s)
}

/// Lua arithmetic coercion of a captured number string (`min + x`): the captures are always numeric.
pub fn arith(s: &str, site: &str) -> f64 {
    tonumber(s).unwrap_or_else(|| panic!("{site}: attempt to perform arithmetic on a string value"))
}

/// `tostring(n)` (L1).
pub fn tostring(n: f64) -> String {
    pob_lua::tostring_num(n)
}

// --- Common.lua rounding (Modules/Common.lua:709-777) ------------------------------------------------------------

fn pow10(dec: f64) -> f64 {
    10f64.powf(dec)
}

/// Common.lua:709 `round(val, dec)`.
pub fn round(val: f64, dec: Option<f64>) -> f64 {
    pob_lua::number::round(val, dec)
}

/// Common.lua:733 `roundSymmetric(val, dec)` -- half away from zero.
pub fn round_symmetric(val: f64, dec: Option<f64>) -> f64 {
    match dec {
        Some(d) => {
            let factor = pow10(d);
            if val >= 0.0 {
                (val * factor + 0.5).floor() / factor
            } else {
                (val * factor - 0.5).ceil() / factor
            }
        }
        None => {
            if val >= 0.0 {
                (val + 0.5).floor()
            } else {
                (val - 0.5).ceil()
            }
        }
    }
}

/// Common.lua:766 `floorSymmetric(val, dec)` -- `select(1, math.modf(...))`.
pub fn floor_symmetric(val: f64, dec: Option<f64>) -> f64 {
    match dec {
        Some(d) => {
            let factor = pow10(d);
            pob_lua::number::modf(val * factor).0 / factor
        }
        None => pob_lua::number::modf(val).0,
    }
}

/// Common.lua:753 `alwaysPositiveRound(val, dec)`.
pub fn always_positive_round(val: f64, dec: Option<f64>) -> f64 {
    match dec {
        Some(d) => {
            let factor = pow10(d);
            floor_symmetric(val * factor + 0.5, None) / factor
        }
        None => floor_symmetric(val + 0.5, None),
    }
}

/// Common.lua:1110 `escapeGGGString`.
pub fn escape_ggg_string(text: &str) -> String {
    let s = gsub(text, "<[^>]+>{([^}]+)}", "%1", None);
    let s = gsub(&s, "%[([^|%]]+)%]", "%1", None);
    gsub(&s, "%[[^|]+|([^|]+)%]", "%1", None)
}
