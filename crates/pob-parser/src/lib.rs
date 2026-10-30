//! `modLib.parseMod` -- ModParser.lua:6994-7021, the cached wrapper only (pob-ts `src/mods/parseMod.ts`, R3-ARCH §1):
//!
//! ```lua
//! parseMod = function(line, isComb)
//!     if not cache[line] then <real parser> ; cache[line] = { modList, extra } end
//!     return unpack(copyTable(cache[line]))
//! end
//! ```
//!
//! The cache is the truth for a line it holds (MINEFIELD W11/D9). A line it does not hold answers `nil, line` here and is
//! counted: the real parser is R5 -- until then every miss is a line upstream's parser might have understood
//! (`unimplemented`: ModParser.lua:6994 parseMod real-parser branch).

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use pob_mods::{Mod, Table, Value};

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
fn copy_value(v: &Value) -> Value {
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
/// (ModParser.lua:6991). `unimplemented`: the real parser (ModParser.lua:6994-7019) -- R5.
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
