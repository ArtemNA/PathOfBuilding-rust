//! Lua string semantics the controls rely on, on top of `pob_lua::pattern` (byte strings, 1-based positions).
//! Rust strings must stay valid UTF-8: a byte slice that splits a sequence is converted lossily. The Edit filter keeps
//! buffers ASCII by default (EditControl.lua:50, `[^%w%p ]` removed; Paste maps bytes 128-255 to `?`), so this only
//! matters for `main.unicode` filters (`%c`).

use pob_lua::pattern::{self, Capture, Repl};

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// `s:sub(i, j)` with Lua's index rules (negative from the end, clamped).
pub fn sub(s: &str, i: i64, j: i64) -> String {
    let b = s.as_bytes();
    let len = b.len() as i64;
    let i = if i < 0 { (len + i + 1).max(1) } else { i.max(1) };
    let j = if j < 0 { len + j + 1 } else { j.min(len) };
    if i > j {
        return String::new();
    }
    lossy(&b[(i - 1) as usize..j as usize])
}

/// `s:sub(i)`.
pub fn sub_from(s: &str, i: i64) -> String {
    sub(s, i, -1)
}

fn pat_err(p: &str, e: pattern::PatternError) -> ! {
    panic!("malformed pattern '{p}': {e:?}")
}

/// `s:match(p)` → the first capture (or the whole match) as a string; `None` = no match. Position captures are
/// returned as their decimal text.
pub fn lmatch(s: &str, p: &str) -> Option<String> {
    let caps = pattern::match_(s.as_bytes(), p.as_bytes(), None).unwrap_or_else(|e| pat_err(p, e))?;
    Some(match caps.get(0) {
        Some(Capture::Str { start, end }) => lossy(&s.as_bytes()[start..end]),
        Some(Capture::Pos(n)) => n.to_string(),
        None => String::new(),
    })
}

/// `s:match(p) ~= nil`.
pub fn is_match(s: &str, p: &str) -> bool {
    pattern::find(s.as_bytes(), p.as_bytes(), None, false)
        .unwrap_or_else(|e| pat_err(p, e))
        .is_some()
}

/// `s:gsub(p, repl)` with a replacement string.
pub fn gsub(s: &str, p: &str, repl: &str) -> String {
    let (out, _) = pattern::gsub(s.as_bytes(), p.as_bytes(), Repl::Str(repl.as_bytes()), None).unwrap_or_else(|e| pat_err(p, e));
    lossy(&out)
}

/// `s:find(p, init, plain)` → 1-based `(start, end)`.
pub fn find(s: &str, p: &str, init: Option<i64>, plain: bool) -> Option<(usize, usize)> {
    pattern::find(s.as_bytes(), p.as_bytes(), init, plain)
        .unwrap_or_else(|e| pat_err(p, e))
        .map(|(a, b, _)| (a, b))
}

/// `(s .. "\n"):gmatch("()([^\n]*)\n()")` → `(s, line, e)` with 1-based positions (EditControl.lua:334).
pub fn lines_with_pos(s: &str) -> Vec<(usize, String, usize)> {
    let mut out = Vec::new();
    let mut start = 1;
    for line in s.split('\n') {
        let e = start + line.len() + 1;
        out.push((start, line.to_string(), e));
        start = e;
    }
    out
}

/// `utf8.next(s, idx, offset)` of lua-utf8 for `offset` = ±1: the byte position of the next / previous code point,
/// `None` past the ends.
pub fn utf8_next(s: &str, idx: usize, offset: i32) -> Option<usize> {
    let b = s.as_bytes();
    let is_cont = |i: usize| b.get(i - 1).is_some_and(|c| c & 0xC0 == 0x80);
    if offset < 0 {
        let mut i = idx.checked_sub(1)?;
        while i >= 1 && is_cont(i) {
            i -= 1;
        }
        (i >= 1).then_some(i)
    } else {
        let mut i = idx + 1;
        while i <= b.len() && is_cont(i) {
            i += 1;
        }
        (i <= b.len()).then_some(i)
    }
}

/// `string.rep(s, n)` (n ≤ 0 → "").
pub fn rep(s: &str, n: i64) -> String {
    if n <= 0 { String::new() } else { s.repeat(n as usize) }
}
