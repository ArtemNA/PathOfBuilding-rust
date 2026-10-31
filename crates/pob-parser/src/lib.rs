//! `Modules/ModParser.lua` (R5-ARCH): the inner `parseMod(line, order)` (`parse.rs`, :6640-6992), `scan` (`scan.rs`,
//! :6616-6638), the closure registry (`closures.rs`) and the named helpers (`helpers.rs`), over `data/mod-parser.json`
//! (`pob_data::mod_parser`); and the wrapper `modLib.parseMod` -- ModParser.lua:6994-7021:
//!
//! ```lua
//! parseMod = function(line, isComb)
//!     if not cache[line] then
//!         local modList, extra = parseMod(line, 1)
//!         if modList and extra then modList, extra = parseMod(line, 2) end
//!         cache[line] = { modList, extra }
//!     end
//!     return unpack(copyTable(cache[line]))
//! end
//! ```
//!
//! The cache is the truth for a line it holds (MINEFIELD W11/D9). Two entry points:
//! * `parse_mod_line` -- what the calc uses today: cache only, a miss answers `nil, line` and is counted. Switching it to the
//!   parser is R5.4 (once the closures of R5.3 are in, R5-ARCH §3).
//! * `parse_mod` -- the full wrapper: cache, else the parser (`parse_mod_uncached`), memoised per thread like upstream's
//!   `cache[line] = ...`; a line whose path needs a closure not ported yet is `Err(Unported(id))`.

#![forbid(unsafe_code)]

pub mod closures;
pub mod closures_gen;
pub mod helpers;
pub mod parse;
pub mod scan;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use pob_data::wire::OrphanTag;
use pob_mods::{LuaError, Mod, Str, Table, Value};

pub use closures::ClosureId;
pub use parse::RawParse;

/// Why the parser could not answer a line.
#[derive(Clone, Debug)]
pub enum ParseError {
    /// The path needs this closure definition and it is not ported yet (R5.3/R5.4) -- explicit, counted, never a guess.
    Unported(ClosureId),
    /// A Lua runtime error upstream raises on this path too (text as LuaJIT's where reachable).
    Lua(LuaError),
    /// Upstream builds a value the Rust model cannot hold (a table as a mod type, a tag after a hole, ...).
    Unrepresentable(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Unported(id) => write!(f, "unported closure {id}"),
            ParseError::Lua(e) => write!(f, "{e}"),
            ParseError::Unrepresentable(s) => write!(f, "unrepresentable: {s}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// The two Lua returns of `parseMod` (L11) plus where they came from.
#[derive(Debug)]
pub struct ParsedLine {
    /// `modList` -- a fresh deep copy on every call.
    pub mod_list: Option<Vec<Mod>>,
    /// `extra`. W16: LuaJIT `unpack(copyTable({nil, extra}))` keeps `extra` even when `modList` is nil (Lua 5.4 would
    /// lose it); the cache entries with `mods: null` and an `extra` (2 814) answer it here.
    pub extra: Option<String>,
    /// The line was in the cache.
    pub hit: bool,
}

/// Counters for the step reports: `miss_lines` de-duplicated, in first-seen order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParseStats {
    pub hits: u64,
    pub misses: u64,
    pub miss_lines: Vec<String>,
}

#[derive(Default)]
struct Counters {
    stats: ParseStats,
    seen: HashSet<String>,
}

thread_local! {
    static COUNTERS: RefCell<Counters> = RefCell::new(Counters::default());
}

/// Lua `copyTable` (recursive) of a value: tables and nested mods (`{ mod = createMod(...) }`) become new objects, so
/// nothing a caller writes -- `tag.div` included (MINEFIELD-RUST R4) -- reaches the cache.
pub(crate) fn copy_value(v: &Value) -> Value {
    match v {
        Value::Table(t) => Value::table(Table {
            array: t.array.iter().map(copy_value).collect(),
            hash: t.hash.iter().map(|(k, x)| (k.clone(), copy_value(x))).collect(),
        }),
        Value::Mod(m) => Value::Mod(Rc::new(copy_table_mod(m))),
        other => other.clone(),
    }
}

/// ModParser.lua:7020 `copyTable(cache[line])` for one mod: `Mod::deep_copy` (new tag objects) plus a deep copy of the value.
pub fn copy_table_mod(m: &Mod) -> Mod {
    let mut c = m.deep_copy();
    c.value = copy_value(&m.value);
    c
}

/// `modLib.parseMod(line, isComb)`. `isComb` only drives upstream's `unsupported.txt` dev log, so it changes nothing
/// here; it stays in the signature because every call site passes it (ProcessStats' line-combining loop).
///
/// A miss returns `None, Some(line)` -- what the real parser returns for a line it cannot read
/// (ModParser.lua:6991). `unimplemented`: the parser on a miss (ModParser.lua:6999-7005) -- R5.4 switches this to `parse_mod`.
pub fn parse_mod_line(line: &str, is_comb: bool) -> ParsedLine {
    let _ = is_comb;
    let cache = pob_data::mod_cache::mod_cache();
    let Some(entry) = cache.get(line) else {
        COUNTERS.with(|c| {
            let mut c = c.borrow_mut();
            c.stats.misses += 1;
            if c.seen.insert(line.to_owned()) {
                c.stats.miss_lines.push(line.to_owned());
            }
        });
        return ParsedLine {
            mod_list: None,
            extra: Some(line.to_owned()),
            hit: false,
        };
    };
    COUNTERS.with(|c| c.borrow_mut().stats.hits += 1);
    ParsedLine {
        mod_list: entry.mods.as_ref().map(|mods| mods.iter().map(copy_table_mod).collect()),
        extra: entry.extra.as_deref().map(str::to_owned),
        hit: true,
    }
}

/// This thread's counters since the last reset.
pub fn parse_mod_stats() -> ParseStats {
    COUNTERS.with(|c| c.borrow().stats.clone())
}

pub fn reset_parse_mod_stats() {
    COUNTERS.with(|c| *c.borrow_mut() = Counters::default());
}

// --- the parser behind the wrapper (R5.2) ---------------------------------------------------------------------------

/// The inner `parseMod(line, order)` (ModParser.lua:6640-6992), no cache.
pub fn parse_mod_raw(line: &str, order: u8) -> Result<RawParse, ParseError> {
    let r = parse::parser().parse(line, order);
    PARSER_STATS.with(|s| {
        let mut s = s.borrow_mut();
        s.inner_calls += 1;
        match &r {
            Ok(_) => {}
            Err(ParseError::Unported(id)) => *s.unported.entry(id.clone()).or_default() += 1,
            Err(ParseError::Lua(_)) => s.lua_errors += 1,
            Err(ParseError::Unrepresentable(_)) => s.unrepresentable += 1,
        }
    });
    r
}

/// The wrapper's parse (ModParser.lua:7000-7005) without the cache: `parseMod(line, 1)`, and when it gave both a `modList`
/// and an `extra`, `parseMod(line, 2)`.
pub fn parse_mod_uncached(line: &str) -> Result<RawParse, ParseError> {
    let r = parse_mod_raw(line, 1)?;
    if r.mod_list.is_some() && r.extra.is_some() {
        return parse_mod_raw(line, 2);
    }
    Ok(r)
}

/// `parse_mod`'s answer: like `ParsedLine`, plus the L9 orphan tags of a static `specialModList` entry.
#[derive(Debug)]
pub struct ParsedMod {
    pub mod_list: Option<Vec<Mod>>,
    pub extra: Option<String>,
    pub orphan_tags: Vec<(usize, OrphanTag)>,
    pub hit: bool,
}

thread_local! {
    /// Upstream's `cache[line] = { modList, extra }` for lines the parser answered (ModParser.lua:7006).
    static PARSED: RefCell<HashMap<String, Rc<RawParse>>> = RefCell::new(HashMap::new());
}

/// `modLib.parseMod(line, isComb)` in full (ModParser.lua:6994-7021): the cache, else the parser (memoised), and a fresh
/// copy of the answer either way (`unpack(copyTable(cache[line]))`, MINEFIELD-RUST R4).
pub fn parse_mod(line: &str, is_comb: bool) -> Result<ParsedMod, ParseError> {
    let _ = is_comb;
    let cache = pob_data::mod_cache::mod_cache();
    if let Some(e) = cache.get(line) {
        return Ok(ParsedMod {
            mod_list: e.mods.as_ref().map(|mods| mods.iter().map(copy_table_mod).collect()),
            extra: e.extra.as_deref().map(str::to_owned),
            orphan_tags: e.orphan_tags.clone(),
            hit: true,
        });
    }
    let r = match PARSED.with(|p| p.borrow().get(line).cloned()) {
        Some(r) => r,
        None => {
            let r = Rc::new(parse_mod_uncached(line)?);
            PARSED.with(|p| p.borrow_mut().insert(line.to_owned(), r.clone()));
            r
        }
    };
    Ok(ParsedMod {
        mod_list: r.mod_list.as_ref().map(|mods| mods.iter().map(copy_table_mod).collect()),
        extra: r.extra.clone(),
        orphan_tags: r.orphan_tags.clone(),
        hit: false,
    })
}

/// Counters of the parser (this thread, since the last reset).
#[derive(Clone, Debug, Default)]
pub struct ParserStats {
    pub inner_calls: u64,
    /// Calls stopped by an unported closure, by definition id.
    pub unported: HashMap<Str, u64>,
    pub lua_errors: u64,
    pub unrepresentable: u64,
    /// Lines where more than one `jewelFuncList` pattern yields `cap1` (ModParser.lua:6643: the first in `pairs` order wins).
    pub jewel_multi: Vec<String>,
    /// Lines whose DOUBLED form rewrote a `modNameList` entry (ModParser.lua:6862 -- state that outlives the call).
    pub doubled_writes: Vec<String>,
}

thread_local! {
    static PARSER_STATS: RefCell<ParserStats> = RefCell::new(ParserStats::default());
}

pub fn parser_stats() -> ParserStats {
    PARSER_STATS.with(|s| s.borrow().clone())
}

pub fn reset_parser_stats() {
    PARSER_STATS.with(|s| *s.borrow_mut() = ParserStats::default());
    scan::reset_tie_stats();
}

pub(crate) fn note_jewel_multi(line: &str) {
    PARSER_STATS.with(|s| s.borrow_mut().jewel_multi.push(line.to_owned()));
}

pub(crate) fn note_doubled(line: &str) {
    PARSER_STATS.with(|s| s.borrow_mut().doubled_writes.push(line.to_owned()));
}
