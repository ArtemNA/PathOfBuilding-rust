//! `Data/ModCache.lua` -- the table ModParser.lua:6993 keeps as `cache`; `parseMod` answers from it before the real parser
//! (Main.lua:136; MINEFIELD W11/D9: on the desktop the cache IS the truth for a line it holds). pob-ts `src/data/modCache.ts`.
//!
//! Mods are `pob_mods::Mod` (R3-ARCH §2.1): `malformed` ones are dropped (0 in the data; L24), `orphanTags` kept beside the
//! mod, never in `tags` (L9). `Mod` holds `Rc`s, so the parsed cache lives per thread (one load per thread that asks).

use std::cell::OnceCell;
use std::collections::HashMap;
use std::rc::Rc;

use pob_mods::{Mod, Str};
use serde::Deserialize;
use serde_json::Value as Json;

use crate::wire::{ModRecord, OrphanTag, json_to_mod};

/// One `c[line] = { modList, extraText }`; `None` is Lua nil in either field.
#[derive(Debug)]
pub struct ModCacheEntry {
    pub line: Str,
    pub mods: Option<Vec<Mod>>,
    pub extra: Option<Str>,
    /// `(index of the mod in mods, orphan tag)` -- recorded, never applied (L9).
    pub orphan_tags: Vec<(usize, OrphanTag)>,
}

/// The cache: entries in file order (byte-sorted lines) plus a lookup index.
#[derive(Debug)]
pub struct ModCache {
    pub entries: Vec<ModCacheEntry>,
    index: HashMap<Str, usize>,
    /// Mods dropped for `malformed` (L24) -- 0 in the current data.
    pub malformed_dropped: usize,
}

impl ModCache {
    /// `cache[line]`.
    pub fn get(&self, line: &str) -> Option<&ModCacheEntry> {
        self.index.get(line).map(|&i| &self.entries[i])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawEntry {
    pub(crate) line: String,
    pub(crate) mods: Option<Vec<Json>>,
    extra: Option<String>,
}

fn build(raw: Vec<RawEntry>) -> Result<ModCache, String> {
    let mut entries = Vec::with_capacity(raw.len());
    let mut index = HashMap::with_capacity(raw.len());
    let mut malformed_dropped = 0;
    for r in raw {
        let mut orphan_tags = Vec::new();
        let mods = match r.mods {
            None => None,
            Some(list) => {
                let mut out = Vec::with_capacity(list.len());
                for j in &list {
                    match json_to_mod(j).map_err(|e| format!("mod-cache '{}': {e}", r.line))? {
                        ModRecord::Malformed => malformed_dropped += 1,
                        ModRecord::Mod(dm) => {
                            orphan_tags.extend(dm.orphan_tags.into_iter().map(|o| (out.len(), o)));
                            out.push(dm.m);
                        }
                    }
                }
                Some(out)
            }
        };
        let line: Str = Str::from(r.line.as_str());
        if index.insert(line.clone(), entries.len()).is_some() {
            return Err(format!("mod-cache: duplicate line '{line}'"));
        }
        entries.push(ModCacheEntry {
            line,
            mods,
            extra: r.extra.map(|e| Str::from(e.as_str())),
            orphan_tags,
        });
    }
    Ok(ModCache {
        entries,
        index,
        malformed_dropped,
    })
}

thread_local! {
    static CACHE: OnceCell<Rc<ModCache>> = const { OnceCell::new() };
}

/// The mod cache of this thread (parsed on first use). Callers must not hand its mods out for mutation -- copy them
/// (`pob-parser` does, ModParser.lua:7020 `copyTable`).
pub fn mod_cache() -> Rc<ModCache> {
    CACHE.with(|c| {
        c.get_or_init(|| {
            let mut raw: Vec<RawEntry> = crate::load("mod-cache.json");
            crate::patches::apply_mod_cache(&mut raw).unwrap_or_else(|e| panic!("{e}"));
            Rc::new(build(raw).unwrap_or_else(|e| panic!("pob-data: {e}")))
        })
        .clone()
    })
}
