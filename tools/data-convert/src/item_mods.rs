//! `data.itemMods` (Modules/Data.lua: the 17 real categories + the derived `Item` merge and `Vestigial` filter)
//! -> item-mods/<Category>.json + item-mods/_index.json. Also the affix-record reader mod_tables.rs shares.
//!
//! A mod table: display lines in the array part `[1..n]` (-> `lines`, a hole is fatal), named fields after it. The file
//! keeps the old schema's field order; a field outside the schema is drift.

use crate::ctx::{Ctx, Obj, R, key_text};
use crate::dump::V;
use crate::js::J;

/// Files in this order; also the wire's serialization order (where a shared table is canonical).
pub const REAL_CATEGORIES: &[&str] = &[
    "Explicit", "ItemExclusive", "Corrupted", "Delve", "Synthesis", "Scourge", "Eldritch", "Mercenary", "Flask", "Tincture",
    "Graft", "Jewel", "JewelAbyss", "JewelCluster", "JewelCharm", "Foulborn", "WatchersEye",
];
/// Data.lua `for _, type in ipairs({ "Explicit", ... })`: the last listed category wins per id.
const ITEM_MERGE_ORDER: &[&str] = &["Explicit", "ItemExclusive", "Corrupted", "Delve", "Synthesis", "Scourge", "Eldritch", "Mercenary"];
const VESTIGIAL_PREFIX: &str = "Divergent";
const VESTIGIAL_RULE: &str = "ItemExclusive id matches /^Divergent/";

/// The value type of one named field.
#[derive(Clone, Copy)]
pub enum K {
    Str,
    Num,
    Strs,
    Nums,
    /// `{ [hash] = { line, ... } }`: key text -> strings (an empty table is `{}`).
    TradeHashes,
    /// `{ [name] = true }`.
    TrueSet,
}

/// (field, type, required)
pub type Spec = &'static [(&'static str, K, bool)];

pub const ITEM_MOD: Spec = &[
    ("type", K::Str, false),
    ("affix", K::Str, true),
    ("group", K::Str, true),
    ("level", K::Num, true),
    ("statOrder", K::Nums, true),
    ("weightKey", K::Strs, true),
    ("weightVal", K::Nums, true),
    ("modTags", K::Strs, true),
    ("tradeHashes", K::TradeHashes, true),
    ("tags", K::Strs, false),
    ("weightMultiplierKey", K::Strs, false),
    ("weightMultiplierVal", K::Nums, false),
];

pub fn strings(c: &Ctx, v: &V, at: &str) -> R<J> {
    Ok(J::Arr(c.seq(v, at)?.into_iter().map(|x| c.string(x, at).map(J::Str)).collect::<R<_>>()?))
}

fn numbers(c: &Ctx, v: &V, at: &str) -> R<J> {
    let one = |x: &V| x.as_num().map(J::Num).ok_or_else(|| format!("{at}: expected a number, got {}", c.show(x)));
    Ok(J::Arr(c.seq(v, at)?.into_iter().map(one).collect::<R<_>>()?))
}

fn field(c: &Ctx, v: &V, k: K, at: &str) -> R<J> {
    Ok(match k {
        K::Str => J::Str(c.string(v, at)?),
        K::Num => J::Num(v.as_num().ok_or_else(|| format!("{at}: expected a number, got {}", c.show(v)))?),
        K::Strs => strings(c, v, at)?,
        K::Nums => numbers(c, v, at)?,
        K::TradeHashes => {
            let t = c.d.t(c.table(v, at)?);
            J::Obj(t.entries.iter().map(|(k, x)| Ok((key_text(k, at)?, strings(c, x, at)?))).collect::<R<_>>()?)
        }
        K::TrueSet => {
            let one = |(k, x): (&str, &V)| match x {
                V::Bool(true) => Ok((k.to_owned(), J::Bool(true))),
                x => Err(format!("{at}.{k}: expected true, got {}", c.show(x))),
            };
            J::Obj(c.str_entries(v, at)?.into_iter().map(one).collect::<R<_>>()?)
        }
    })
}

/// `{ [1..n] = line, name = value, ... }` -> `{ lines, <spec fields in spec order> }`; other named fields are drift.
pub fn affix_record(c: &mut Ctx, v: &V, at: &str, file: &str, kind: &str, spec: Spec) -> R<J> {
    let t = c.d.t(c.table(v, at)?);
    let mut lines = Vec::new();
    let mut named = Vec::new();
    for (k, x) in &t.entries {
        match k {
            V::Num(_) => {
                if *k != V::Num((lines.len() + 1) as f64) {
                    return Err(format!("{at}: line keys are not 1..n (key {} after {} lines)", c.show(k), lines.len()));
                }
                lines.push(J::Str(c.string(x, &format!("{at}[{}]", lines.len() + 1))?));
            }
            V::Str(s) => named.push((&**s, x)),
            k => return Err(format!("{at}: unexpected key {}", c.show(k))),
        }
    }
    let mut o = Obj::default();
    o.put("lines", J::Arr(lines));
    for &(name, k, required) in spec {
        match named.iter().find(|e| e.0 == name) {
            Some(&(_, x)) => o.put(name, field(c, x, k, &format!("{at}.{name}"))?),
            None if required => return Err(format!("{at}.{name}: missing")),
            None => {}
        }
    }
    for (name, x) in named {
        if !spec.iter().any(|s| s.0 == name) {
            let what = format!("unknown field '{name}' ({})", c.show(x).split('(').next().unwrap_or(""));
            c.drift(file, kind, what);
        }
    }
    Ok(o.done())
}

fn item_mods<'a>(c: &Ctx<'a>) -> R<&'a V> {
    c.d.path("data", &["itemMods"]).ok_or_else(|| "data.itemMods missing".to_owned())
}

fn category<'a>(c: &Ctx<'a>, cat: &str) -> R<Vec<(&'a str, &'a V)>> {
    let im = item_mods(c)?;
    let v = c.d.t(c.table(im, "data.itemMods")?).get(cat).ok_or_else(|| format!("data.itemMods.{cat}: missing"))?;
    c.str_entries(v, &format!("data.itemMods.{cat}"))
}

fn convert_category(c: &mut Ctx, cat: &str) -> R<J> {
    let file = format!("item-mods/{cat}.json");
    let mut out = Vec::new();
    for (id, m) in category(c, cat)? {
        out.push((id.to_owned(), affix_record(c, m, &format!("data.itemMods.{cat}.{id}"), &file, "item mod", ITEM_MOD)?));
    }
    c.count(&file, "mods", out.len());
    Ok(J::Obj(out))
}

macro_rules! categories {
    ($($f:ident = $cat:literal),* $(,)?) => {
        $(pub fn $f(c: &mut Ctx) -> R<J> { convert_category(c, $cat) })*
    };
}
categories!(
    explicit = "Explicit", item_exclusive = "ItemExclusive", corrupted = "Corrupted", delve = "Delve",
    synthesis = "Synthesis", scourge = "Scourge", eldritch = "Eldritch", mercenary = "Mercenary", flask = "Flask",
    tincture = "Tincture", graft = "Graft", jewel = "Jewel", jewel_abyss = "JewelAbyss", jewel_cluster = "JewelCluster",
    jewel_charm = "JewelCharm", foulborn = "Foulborn", watchers_eye = "WatchersEye",
);

/// item-mods/_index.json: the merge/filter rules and counts; `Item` and `Vestigial` are checked against the Lua tables.
pub fn index(c: &mut Ctx) -> R<J> {
    const FILE: &str = "item-mods/_index.json";
    let im = item_mods(c)?;
    for (k, _) in c.str_entries(im, "data.itemMods")? {
        if !REAL_CATEGORIES.contains(&k) && k != "Item" && k != "Vestigial" {
            c.drift(FILE, "data.itemMods", format!("unknown category '{k}'"));
        }
    }
    let mut counts = Vec::new();
    for cat in REAL_CATEGORIES {
        counts.push((cat.to_string(), J::Num(category(c, cat)?.len() as f64)));
    }
    counts.sort_by(|a, b| a.0.cmp(&b.0));

    // Data.lua: Item = last-wins merge over ITEM_MERGE_ORDER; the Lua table must hold exactly those tables.
    let mut winner: std::collections::HashMap<&str, &V> = std::collections::HashMap::new();
    for cat in ITEM_MERGE_ORDER {
        for (id, m) in category(c, cat)? {
            winner.insert(id, m);
        }
    }
    let item = category(c, "Item")?;
    if item.len() != winner.len() {
        return Err(format!("data.itemMods.Item: {} entries, the merge gives {}", item.len(), winner.len()));
    }
    for (id, m) in &item {
        if winner.get(id) != Some(m) {
            return Err(format!("data.itemMods.Item.{id}: not the table the {ITEM_MERGE_ORDER:?} merge picks"));
        }
    }
    let excl = category(c, "ItemExclusive")?;
    let vestigial: Vec<_> = excl.iter().filter(|(id, _)| id.starts_with(VESTIGIAL_PREFIX)).collect();
    let wv = category(c, "Vestigial")?;
    if wv.len() != vestigial.len() || wv.iter().zip(&vestigial).any(|(a, b)| a.0 != b.0 || a.1 != b.1) {
        return Err(format!("data.itemMods.Vestigial: not the ItemExclusive entries whose id {VESTIGIAL_RULE}"));
    }

    let strs = |l: &[&str]| J::Arr(l.iter().map(|s| J::str(s)).collect());
    let mut o = Obj::default();
    o.put("categories", strs(REAL_CATEGORIES));
    o.put("itemMergeOrder", strs(ITEM_MERGE_ORDER));
    o.put("vestigialRule", J::str(VESTIGIAL_RULE));
    o.put("counts", J::Obj(counts));
    o.put(
        "derived",
        J::Obj(vec![("Item".into(), J::Num(winner.len() as f64)), ("Vestigial".into(), J::Num(vestigial.len() as f64))]),
    );
    Ok(o.done())
}
