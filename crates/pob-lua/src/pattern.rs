//! Lua patterns with LuaJIT 2.1 semantics (`lib_string.c`, itself a port of Lua 5.1 `lstrlib.c`): `string.find`,
//! `string.match`, `string.gmatch`, `string.gsub`. A direct port of the backtracking matcher -- not a regex translation
//! (MINEFIELD L13). Everything works on bytes (L16); character classes are LuaJIT's ASCII `lj_char_*` tables (C locale).
//! Truth: `tests/fixtures/luajit-patterns-*.json` (desktop LuaJIT).
//!
//! The C code reads `*s` at `src_end` and `*p` at the pattern end as the terminating NUL; `sc`/`pc` reproduce that, so an
//! embedded NUL in a pattern ends it exactly as in C.

use std::fmt;

/// `LUA_MAXCAPTURES`.
pub const MAX_CAPTURES: usize = 32;
/// `LJ_MAX_XLEVEL`: maximum nesting of `match` calls before "pattern too complex".
const MAX_XLEVEL: u32 = 200;
const CAP_UNFINISHED: isize = -1;
const CAP_POSITION: isize = -2;
const L_ESC: u8 = b'%';
/// `lj_str_haspattern`: any of these makes `find` use the matcher instead of a plain substring search.
const SPECIALS: &[u8] = b"^$*+?.([%-";

/// One capture as Lua returns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capture {
    /// A substring: 0-based half-open byte range in the subject.
    Str { start: usize, end: usize },
    /// A position capture `()`: 1-based position, as Lua returns it.
    Pos(usize),
}

impl Capture {
    /// The captured bytes of `subject`, or `None` for a position capture.
    pub fn bytes<'a>(&self, subject: &'a [u8]) -> Option<&'a [u8]> {
        match *self {
            Capture::Str { start, end } => Some(&subject[start..end]),
            Capture::Pos(_) => None,
        }
    }
}

/// The values a match returns (no heap allocation): up to `LUA_MAXCAPTURES` captures.
#[derive(Clone, Copy)]
pub struct Captures {
    len: u8,
    caps: [Capture; MAX_CAPTURES],
}

impl Captures {
    const EMPTY: Captures = Captures {
        len: 0,
        caps: [Capture::Pos(0); MAX_CAPTURES],
    };

    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, i: usize) -> Option<Capture> {
        self.as_slice().get(i).copied()
    }

    pub fn as_slice(&self) -> &[Capture] {
        &self.caps[..usize::from(self.len)]
    }

    pub fn iter(&self) -> impl Iterator<Item = Capture> + '_ {
        self.as_slice().iter().copied()
    }
}

impl fmt::Debug for Captures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

impl PartialEq for Captures {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

/// A pattern error with LuaJIT's message text (`lj_errmsg.h`), e.g. `malformed pattern (missing ']')`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternError(pub String);

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PatternError {}

/// Internal error codes (cheap to carry through the hot path); `LJ_ERR_*` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Err {
    PatB, // STRPATB
    PatC, // STRPATC
    PatE, // STRPATE
    PatM, // STRPATM
    PatU, // STRPATU
    PatX, // STRPATX
    CapI, // STRCAPI
    CapN, // STRCAPN
    CapU, // STRCAPU
}

impl From<Err> for PatternError {
    fn from(e: Err) -> Self {
        PatternError(
            match e {
                Err::PatB => "missing '[' after '%f' in pattern",
                Err::PatC => "invalid pattern capture",
                Err::PatE => "malformed pattern (ends with '%')",
                Err::PatM => "malformed pattern (missing ']')",
                Err::PatU => "unbalanced pattern",
                Err::PatX => "pattern too complex",
                Err::CapI => "invalid capture index",
                Err::CapN => "too many captures",
                Err::CapU => "unfinished capture",
            }
            .to_owned(),
        )
    }
}

type R<T> = Result<T, Err>;

// --- lj_char.c: ASCII character classes ---------------------------------------------------------

fn is_cntrl(c: u8) -> bool {
    c < 0x20 || c == 0x7f
}
fn is_space(c: u8) -> bool {
    c == b' ' || (b'\t'..=b'\r').contains(&c)
}
fn is_graph(c: u8) -> bool {
    (0x21..0x7f).contains(&c)
}
fn is_punct(c: u8) -> bool {
    is_graph(c) && !c.is_ascii_alphanumeric()
}

/// `match_class`: `%a` etc.; an upper-case class letter negates; any other byte matches itself.
fn match_class(c: u8, cl: u8) -> bool {
    let res = match cl.to_ascii_lowercase() {
        b'a' => c.is_ascii_alphabetic(),
        b'c' => is_cntrl(c),
        b'd' => c.is_ascii_digit(),
        b'g' => is_graph(c),
        b'l' => c.is_ascii_lowercase(),
        b'p' => is_punct(c),
        b's' => is_space(c),
        b'u' => c.is_ascii_uppercase(),
        b'w' => c.is_ascii_alphanumeric(),
        b'x' => c.is_ascii_hexdigit(),
        b'z' => c == 0,
        _ => return cl == c,
    };
    if cl.is_ascii_uppercase() { !res } else { res }
}

// --- the matcher ---------------------------------------------------------------------------------

struct MatchState<'a> {
    src: &'a [u8],
    pat: &'a [u8],
    level: usize,
    depth: u32,
    cap_init: [usize; MAX_CAPTURES],
    cap_len: [isize; MAX_CAPTURES],
}

impl<'a> MatchState<'a> {
    fn new(src: &'a [u8], pat: &'a [u8]) -> Self {
        MatchState {
            src,
            pat,
            level: 0,
            depth: 0,
            cap_init: [0; MAX_CAPTURES],
            cap_len: [0; MAX_CAPTURES],
        }
    }

    /// `*p` (NUL past the end).
    #[inline(always)]
    fn pc(&self, p: usize) -> u8 {
        self.pat.get(p).copied().unwrap_or(0)
    }

    /// `*s` (NUL at `src_end`).
    #[inline(always)]
    fn sc(&self, s: usize) -> u8 {
        self.src.get(s).copied().unwrap_or(0)
    }

    /// `classend`: the index just past the single-char class at `p`.
    fn class_end(&self, mut p: usize) -> R<usize> {
        let c = self.pc(p);
        p += 1;
        match c {
            L_ESC => {
                if self.pc(p) == 0 {
                    return Err(Err::PatE);
                }
                Ok(p + 1)
            }
            b'[' => {
                if self.pc(p) == b'^' {
                    p += 1;
                }
                loop {
                    // look for a ']'
                    if self.pc(p) == 0 {
                        return Err(Err::PatM);
                    }
                    let c = self.pc(p);
                    p += 1;
                    if c == L_ESC && self.pc(p) != 0 {
                        p += 1; // skip escapes (e.g. '%]')
                    }
                    if self.pc(p) == b']' {
                        break;
                    }
                }
                Ok(p + 1)
            }
            _ => Ok(p),
        }
    }

    /// `matchbracketclass`: `p` at '[', `ec` at the closing ']'.
    fn match_bracket_class(&self, c: u8, mut p: usize, ec: usize) -> bool {
        let mut sig = true;
        if self.pc(p + 1) == b'^' {
            sig = false;
            p += 1;
        }
        loop {
            p += 1;
            if p >= ec {
                break;
            }
            let pcur = self.pc(p);
            if pcur == L_ESC {
                p += 1;
                if match_class(c, self.pc(p)) {
                    return sig;
                }
            } else if self.pc(p + 1) == b'-' && p + 2 < ec {
                p += 2;
                if pcur <= c && c <= self.pc(p) {
                    return sig;
                }
            } else if pcur == c {
                return sig;
            }
        }
        !sig
    }

    /// `singlematch`.
    #[inline]
    fn single_match(&self, c: u8, p: usize, ep: usize) -> bool {
        match self.pc(p) {
            b'.' => true,
            L_ESC => match_class(c, self.pc(p + 1)),
            b'[' => self.match_bracket_class(c, p, ep - 1),
            pch => pch == c,
        }
    }

    /// `s < src_end && singlematch(*s, p, ep)`.
    #[inline(always)]
    fn single_at(&self, s: usize, p: usize, ep: usize) -> bool {
        match self.src.get(s) {
            Some(&c) => self.single_match(c, p, ep),
            None => false,
        }
    }

    fn match_balance(&self, s: usize, p: usize) -> R<Option<usize>> {
        let b = self.pc(p);
        let e = self.pc(p + 1);
        if b == 0 || e == 0 {
            return Err(Err::PatU);
        }
        if self.sc(s) != b {
            return Ok(None);
        }
        let mut cont = 1;
        let mut s = s + 1;
        while s < self.src.len() {
            let c = self.src[s];
            if c == e {
                cont -= 1;
                if cont == 0 {
                    return Ok(Some(s + 1));
                }
            } else if c == b {
                cont += 1;
            }
            s += 1;
        }
        Ok(None)
    }

    fn max_expand(&mut self, s: usize, p: usize, ep: usize) -> R<Option<usize>> {
        let mut i = 0;
        while self.single_at(s + i, p, ep) {
            i += 1;
        }
        // keep trying to match with the maximum repetitions
        loop {
            if let Some(r) = self.do_match(s + i, ep + 1)? {
                return Ok(Some(r));
            }
            if i == 0 {
                return Ok(None);
            }
            i -= 1;
        }
    }

    fn min_expand(&mut self, mut s: usize, p: usize, ep: usize) -> R<Option<usize>> {
        loop {
            if let Some(r) = self.do_match(s, ep + 1)? {
                return Ok(Some(r));
            }
            if self.single_at(s, p, ep) {
                s += 1; // try with one more repetition
            } else {
                return Ok(None);
            }
        }
    }

    fn start_capture(&mut self, s: usize, p: usize, what: isize) -> R<Option<usize>> {
        let level = self.level;
        if level >= MAX_CAPTURES {
            return Err(Err::CapN);
        }
        self.cap_init[level] = s;
        self.cap_len[level] = what;
        self.level = level + 1;
        let res = self.do_match(s, p)?;
        if res.is_none() {
            self.level -= 1; // undo capture
        }
        Ok(res)
    }

    fn end_capture(&mut self, s: usize, p: usize) -> R<Option<usize>> {
        let l = self.capture_to_close()?;
        self.cap_len[l] = (s - self.cap_init[l]) as isize;
        let res = self.do_match(s, p)?;
        if res.is_none() {
            self.cap_len[l] = CAP_UNFINISHED; // undo capture
        }
        Ok(res)
    }

    fn capture_to_close(&self) -> R<usize> {
        (0..self.level).rev().find(|&l| self.cap_len[l] == CAP_UNFINISHED).ok_or(Err::PatC)
    }

    /// `%1`-`%9` inside a pattern (`l` is the digit byte).
    fn match_capture(&self, s: usize, l: u8) -> R<Option<usize>> {
        let l = usize::from(l).wrapping_sub(usize::from(b'1'));
        if l >= self.level || self.cap_len[l] == CAP_UNFINISHED {
            return Err(Err::CapI);
        }
        // A position capture has len CAP_POSITION, which as size_t never fits: no match.
        let Ok(len) = usize::try_from(self.cap_len[l]) else {
            return Ok(None);
        };
        let init = self.cap_init[l];
        if self.src.len() - s >= len && self.src[init..init + len] == self.src[s..s + len] {
            Ok(Some(s + len))
        } else {
            Ok(None)
        }
    }

    /// `match`: the end of the match of pattern `p..` at subject `s`, or `None`.
    fn do_match(&mut self, mut s: usize, mut p: usize) -> R<Option<usize>> {
        self.depth += 1;
        if self.depth > MAX_XLEVEL {
            return Err(Err::PatX);
        }
        let res = loop {
            match self.pc(p) {
                b'(' => {
                    break if self.pc(p + 1) == b')' {
                        self.start_capture(s, p + 2, CAP_POSITION)?
                    } else {
                        self.start_capture(s, p + 1, CAP_UNFINISHED)?
                    };
                }
                b')' => break self.end_capture(s, p + 1)?,
                L_ESC => match self.pc(p + 1) {
                    b'b' => match self.match_balance(s, p + 2)? {
                        Some(ns) => {
                            s = ns;
                            p += 4;
                            continue;
                        }
                        None => break None,
                    },
                    b'f' => {
                        p += 2;
                        if self.pc(p) != b'[' {
                            return Err(Err::PatB);
                        }
                        let ep = self.class_end(p)?;
                        let previous = if s == 0 { 0 } else { self.src[s - 1] };
                        if self.match_bracket_class(previous, p, ep - 1) || !self.match_bracket_class(self.sc(s), p, ep - 1) {
                            break None;
                        }
                        p = ep;
                        continue;
                    }
                    d if d.is_ascii_digit() => match self.match_capture(s, d)? {
                        Some(ns) => {
                            s = ns;
                            p += 2;
                            continue;
                        }
                        None => break None,
                    },
                    _ => {}
                },
                0 => break Some(s), // end of pattern
                b'$' if self.pc(p + 1) == 0 => break if s == self.src.len() { Some(s) } else { None },
                _ => {}
            }
            // default: a single-char class with an optional quantifier
            let ep = self.class_end(p)?;
            let m = self.single_at(s, p, ep);
            match self.pc(ep) {
                b'?' => {
                    if m && let Some(r) = self.do_match(s + 1, ep + 1)? {
                        break Some(r);
                    }
                    p = ep + 1;
                }
                b'*' => break self.max_expand(s, p, ep)?,
                b'+' => break if m { self.max_expand(s + 1, p, ep)? } else { None },
                b'-' => break self.min_expand(s, p, ep)?,
                _ => {
                    if !m {
                        break None;
                    }
                    s += 1;
                    p = ep;
                }
            }
        };
        self.depth -= 1;
        Ok(res)
    }

    /// `push_onecapture(i, s, e)`.
    fn capture(&self, i: usize, s: usize, e: usize) -> R<Capture> {
        if i >= self.level {
            return if i == 0 {
                Ok(Capture::Str { start: s, end: e })
            } else {
                Err(Err::CapI)
            };
        }
        match self.cap_len[i] {
            CAP_UNFINISHED => Err(Err::CapU),
            CAP_POSITION => Ok(Capture::Pos(self.cap_init[i] + 1)),
            l => Ok(Capture::Str {
                start: self.cap_init[i],
                end: self.cap_init[i] + l as usize,
            }),
        }
    }

    /// `push_captures(s, e)`; `whole = false` is the `s == NULL` form used by `find` (no whole-match fallback).
    fn captures(&self, whole: bool, s: usize, e: usize) -> R<Captures> {
        let n = if self.level == 0 && whole { 1 } else { self.level };
        let mut r = Captures::EMPTY;
        for i in 0..n {
            r.caps[i] = self.capture(i, s, e)?;
        }
        r.len = n as u8;
        Ok(r)
    }

    fn reset(&mut self) {
        self.level = 0;
        self.depth = 0;
    }
}

// --- API -----------------------------------------------------------------------------------------

/// `str_find_aux` start position: negative `init` counts from the end, then clamp to `[0, #s]` (non-LJ_52 build).
fn start_index(len: usize, init: Option<i64>) -> usize {
    let mut start = init.unwrap_or(1);
    if start < 0 {
        start += len as i64;
    } else {
        start -= 1;
    }
    (start.max(0) as usize).min(len)
}

/// `lj_str_find`: plain substring search.
fn find_plain(s: &[u8], p: &[u8]) -> Option<usize> {
    if p.is_empty() {
        return Some(0);
    }
    if p.len() > s.len() {
        return None;
    }
    let (first, rest) = (p[0], &p[1..]);
    let last = s.len() - p.len();
    let mut i = 0;
    while i <= last {
        let off = s[i..=last].iter().position(|&c| c == first)?;
        i += off;
        if &s[i + 1..i + p.len()] == rest {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// The common loop of `string.find` / `string.match` over start positions (with `^` anchoring).
fn find_aux(s: &[u8], p: &[u8], init: Option<i64>, find: bool) -> R<Option<(usize, usize, Captures)>> {
    let st = start_index(s.len(), init);
    let (pat, anchor) = match p.first() {
        Some(b'^') => (&p[1..], true),
        _ => (p, false),
    };
    let mut ms = MatchState::new(s, pat);
    let mut sstr = st;
    loop {
        ms.reset();
        if let Some(e) = ms.do_match(sstr, 0)? {
            let caps = ms.captures(!find, sstr, e)?;
            return Ok(Some((sstr, e, caps)));
        }
        if sstr >= s.len() || anchor {
            return Ok(None);
        }
        sstr += 1;
    }
}

/// `string.find(s, p, init, plain)`: `(start, end, captures)` with 1-based inclusive positions.
pub fn find(s: &[u8], p: &[u8], init: Option<i64>, plain: bool) -> Result<Option<(usize, usize, Captures)>, PatternError> {
    if plain || !p.iter().any(|c| SPECIALS.contains(c)) {
        let st = start_index(s.len(), init);
        return Ok(find_plain(&s[st..], p).map(|q| (st + q + 1, st + q + p.len(), Captures::EMPTY)));
    }
    Ok(find_aux(s, p, init, true)?.map(|(a, e, caps)| (a + 1, e, caps)))
}

/// `string.match(s, p, init)`: the captures, or the whole match when the pattern has none.
pub fn match_(s: &[u8], p: &[u8], init: Option<i64>) -> Result<Option<Captures>, PatternError> {
    Ok(find_aux(s, p, init, false)?.map(|(_, _, caps)| caps))
}

/// `string.gmatch(s, p)` iterator. `^` is not an anchor here (LuaJIT / 5.1); an error ends the iteration.
pub struct GMatch<'a> {
    s: &'a [u8],
    p: &'a [u8],
    pos: usize,
    done: bool,
}

pub fn gmatch<'a>(s: &'a [u8], p: &'a [u8]) -> GMatch<'a> {
    GMatch { s, p, pos: 0, done: false }
}

impl Iterator for GMatch<'_> {
    type Item = Result<Captures, PatternError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut ms = MatchState::new(self.s, self.p);
        let mut src = self.pos;
        while src <= self.s.len() {
            ms.reset();
            match ms.do_match(src, 0) {
                Err(e) => {
                    self.done = true;
                    return Some(Err(e.into()));
                }
                Ok(Some(e)) => {
                    self.pos = if e == src { e + 1 } else { e }; // ensure progress for an empty match
                    let r = ms.captures(true, src, e).map_err(PatternError::from);
                    if r.is_err() {
                        self.done = true;
                    }
                    return Some(r);
                }
                Ok(None) => src += 1,
            }
        }
        self.done = true;
        None
    }
}

/// A function replacement: `(subject, captures) -> Some(replacement)` or `None` (Lua nil/false: keep the match).
pub type ReplFn<'a> = dyn FnMut(&[u8], &Captures) -> Option<Vec<u8>> + 'a;

/// `string.gsub` replacement.
pub enum Repl<'a> {
    /// A replacement string: `%0`-`%9`, `%%`, `%x` (non-digit `x`) -> `x` (W15); a trailing `%` adds the NUL byte C reads there.
    Str(&'a [u8]),
    /// A function (or a table lookup): called with the subject and the captures -- all of them, or the whole match when the
    /// pattern has none (`push_captures`); for a table look up the first one. `None` (Lua nil/false) keeps the matched text.
    Fn(&'a mut ReplFn<'a>),
}

fn push_capture(out: &mut Vec<u8>, s: &[u8], c: Capture) {
    match c {
        Capture::Str { start, end } => out.extend_from_slice(&s[start..end]),
        Capture::Pos(n) => out.extend_from_slice(n.to_string().as_bytes()),
    }
}

/// `add_s`.
fn add_s(ms: &MatchState<'_>, out: &mut Vec<u8>, news: &[u8], s: usize, e: usize) -> R<()> {
    let mut i = 0;
    while i < news.len() {
        let c = news[i];
        if c != L_ESC {
            out.push(c);
        } else {
            i += 1; // skip ESC
            let d = news.get(i).copied().unwrap_or(0);
            if !d.is_ascii_digit() {
                out.push(d);
            } else if d == b'0' {
                out.extend_from_slice(&ms.src[s..e]);
            } else {
                push_capture(out, ms.src, ms.capture(usize::from(d - b'1'), s, e)?);
            }
        }
        i += 1;
    }
    Ok(())
}

/// `string.gsub(s, p, repl, n)`: the new string and the number of substitutions.
pub fn gsub(s: &[u8], p: &[u8], mut repl: Repl<'_>, max_n: Option<i64>) -> Result<(Vec<u8>, usize), PatternError> {
    let max_s = max_n.unwrap_or(s.len() as i64 + 1);
    let (pat, anchor) = match p.first() {
        Some(b'^') => (&p[1..], true),
        _ => (p, false),
    };
    let mut ms = MatchState::new(s, pat);
    let mut out = Vec::with_capacity(s.len());
    let mut src = 0;
    let mut n = 0usize;
    while (n as i64) < max_s {
        ms.reset();
        let e = ms.do_match(src, 0)?;
        if let Some(e) = e {
            n += 1;
            // add_value
            match &mut repl {
                Repl::Str(news) => add_s(&ms, &mut out, news, src, e)?,
                Repl::Fn(f) => {
                    let caps = ms.captures(true, src, e)?;
                    match f(s, &caps) {
                        Some(v) => out.extend_from_slice(&v),
                        None => out.extend_from_slice(&s[src..e]), // keep original text
                    }
                }
            }
        }
        match e {
            Some(e) if e > src => src = e, // non-empty match: skip it
            _ if src < s.len() => {
                out.push(s[src]);
                src += 1;
            }
            _ => break,
        }
        if anchor {
            break;
        }
    }
    out.extend_from_slice(&s[src..]);
    Ok((out, n))
}
