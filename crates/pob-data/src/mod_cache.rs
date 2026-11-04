//! `Data/ModCache.lua` -- the table ModParser.lua:6993 keeps as `cache`; `parseMod` answers from it before the real parser
//! (Main.lua:136; MINEFIELD W11/D9: on the desktop the cache IS the truth for a line it holds). pob-ts `src/data/modCache.ts`.
//!
//! Mods are `pob_mods::Mod` (R3-ARCH §2.1): `malformed` ones are dropped (0 in the data; L24), `orphanTags` kept beside the
//! mod, never in `tags` (L9). `Mod` holds `Rc`s, so the parsed cache lives per thread (one load per thread that asks).

use std::cell::{Cell, OnceCell};
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

/// A cache entry: built, or still in the binary table.
struct Slot {
    entry: OnceCell<ModCacheEntry>,
    at: usize,
}

/// The binary table the entries are built from, one by one as they are asked for; a program reads few of the 23 201.
struct Lazy {
    table: crate::bincache::Table,
    /// The `mod-cache.json` patches, by line.
    patches: HashMap<&'static str, Vec<&'static crate::patches::Patch>>,
}

/// The entries in file order (byte-sorted lines): `&entries` and `.iter()` walk them, building what is not built yet.
pub struct Entries {
    slots: Vec<Slot>,
    lazy: Option<Lazy>,
    malformed: Cell<usize>,
}

impl Entries {
    fn slot(&self, i: usize) -> &ModCacheEntry {
        let s = &self.slots[i];
        s.entry.get_or_init(|| {
            let lazy = self.lazy.as_ref().expect("an unbuilt entry has its table");
            let mut raw: RawEntry = lazy.table.read_at(s.at).unwrap_or_else(|e| panic!("pob-data: mod-cache: {e}"));
            if let Some(list) = lazy.patches.get(raw.line.as_str()) {
                crate::patches::apply_to_entry(&mut raw, list).unwrap_or_else(|e| panic!("{e}"));
            }
            let (entry, dropped) = build_entry(raw).unwrap_or_else(|e| panic!("pob-data: {e}"));
            self.malformed.set(self.malformed.get() + dropped);
            entry
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = &ModCacheEntry> {
        (0..self.slots.len()).map(|i| self.slot(i))
    }
}

impl<'a> IntoIterator for &'a Entries {
    type Item = &'a ModCacheEntry;
    type IntoIter = Box<dyn Iterator<Item = &'a ModCacheEntry> + 'a>;
    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

/// The cache: the entries plus a lookup index.
pub struct ModCache {
    pub entries: Entries,
    index: HashMap<Str, usize>,
}

impl std::fmt::Debug for ModCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ModCache({} lines)", self.len())
    }
}

impl ModCache {
    /// `cache[line]`.
    pub fn get(&self, line: &str) -> Option<&ModCacheEntry> {
        self.index.get(line).map(|&i| self.entries.slot(i))
    }

    /// Mods dropped for `malformed` (L24) -- 0 in the current data.
    pub fn malformed_dropped(&self) -> usize {
        self.entries.iter().for_each(drop);
        self.entries.malformed.get()
    }

    pub fn len(&self) -> usize {
        self.entries.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.slots.is_empty()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawEntry {
    pub(crate) line: String,
    pub(crate) mods: Option<Vec<Json>>,
    extra: Option<String>,
}

/// One record of the file as an entry, and the mods it dropped for `malformed`.
fn build_entry(r: RawEntry) -> Result<(ModCacheEntry, usize), String> {
    let mut orphan_tags = Vec::new();
    let mut malformed = 0;
    let mods = match r.mods {
        None => None,
        Some(list) => {
            let mut out = Vec::with_capacity(list.len());
            for j in &list {
                match json_to_mod(j).map_err(|e| format!("mod-cache '{}': {e}", r.line))? {
                    ModRecord::Malformed => malformed += 1,
                    ModRecord::Mod(dm) => {
                        orphan_tags.extend(dm.orphan_tags.into_iter().map(|o| (out.len(), o)));
                        out.push(dm.m);
                    }
                }
            }
            Some(out)
        }
    };
    Ok((
        ModCacheEntry {
            line: Str::from(r.line.as_str()),
            mods,
            extra: r.extra.map(|e| Str::from(e.as_str())),
            orphan_tags,
        },
        malformed,
    ))
}

/// The whole JSON file, patched and built.
pub fn from_json() -> ModCache {
    let mut raw: Vec<RawEntry> = crate::read_json_file("mod-cache.json").unwrap_or_else(|e| panic!("{e}"));
    crate::patches::apply_mod_cache(&mut raw).unwrap_or_else(|e| panic!("{e}"));
    let mut slots = Vec::with_capacity(raw.len());
    let mut index = HashMap::with_capacity(raw.len());
    let mut malformed = 0;
    for r in raw {
        let (entry, dropped) = build_entry(r).unwrap_or_else(|e| panic!("pob-data: {e}"));
        malformed += dropped;
        if index.insert(entry.line.clone(), slots.len()).is_some() {
            panic!("pob-data: mod-cache: duplicate line '{}'", entry.line);
        }
        let cell = OnceCell::new();
        let _ = cell.set(entry);
        slots.push(Slot { entry: cell, at: 0 });
    }
    ModCache {
        entries: Entries {
            slots,
            lazy: None,
            malformed: Cell::new(malformed),
        },
        index,
    }
}

/// The index over the binary table; the entries get built when asked for. `None` (the JSON then): no such table, or not the expected shape.
pub fn from_table(table: crate::bincache::Table) -> Option<ModCache> {
    let positions = table.array_positions().ok()??;
    let mut slots = Vec::with_capacity(positions.len());
    let mut index = HashMap::with_capacity(positions.len());
    for at in positions {
        let line = table.object_str(at, "line").ok()??;
        if index.insert(Str::from(line), slots.len()).is_some() {
            panic!("pob-data: mod-cache: duplicate line '{line}'");
        }
        slots.push(Slot {
            entry: OnceCell::new(),
            at,
        });
    }
    let mut patches: HashMap<&'static str, Vec<&'static crate::patches::Patch>> = HashMap::new();
    for p in crate::patches::patches()
        .unwrap_or_else(|e| panic!("{e}"))
        .iter()
        .filter(|p| p.file == "mod-cache.json")
    {
        let line = p.path.first().and_then(Json::as_str).filter(|l| index.contains_key(*l));
        let line = line.unwrap_or_else(|| panic!("pob-data: patch {} {:?}: no such mod-cache line", p.file, p.path));
        patches.entry(line).or_default().push(p);
    }
    Some(ModCache {
        entries: Entries {
            slots,
            lazy: Some(Lazy { table, patches }),
            malformed: Cell::new(0),
        },
        index,
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
            crate::ensure_data().unwrap_or_else(|e| panic!("{e}"));
            let cached = crate::bincache::cache()
                .and_then(|c| c.table("mod-cache.json"))
                .and_then(from_table);
            Rc::new(cached.unwrap_or_else(from_json))
        })
        .clone()
    })
}
