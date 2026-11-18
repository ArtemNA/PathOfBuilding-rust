//! Item texts and rare-like-unique descriptions (Modules/Data.lua, Data/Uniques/*.lua, Data/Rares.lua) -> uniques.json,
//! rares.json, viscera.json, rare-like-uniques.json. The texts are not parsed here (the Item class does that at runtime).
//!
//! uniques.json needs the post-boot `data` (root `dataAfter`): loading the latest tree appends tree-dependent uniques
//! to `uniques.generated` (MINEFIELD D7). Every other type is expected unchanged by the boot (a change is drift).

use std::collections::HashMap;

use crate::ctx::{Ctx, Obj, R};
use crate::dump::V;
use crate::fields::Fields;
use crate::item_mods::REAL_CATEGORIES;
use crate::js::J;

const RARITY_PREFIX: &str = "Rarity: UNIQUE\n";

fn texts<'a>(c: &Ctx<'a>, v: &'a V, at: &str) -> R<Vec<&'a str>> {
    let one = |(i, x): (usize, &'a V)| match x {
        V::Str(s) if !s.is_empty() => Ok(&**s),
        V::Str(_) => Err(format!("{at}[{}]: empty text", i + 1)),
        x => Err(format!("{at}[{}]: expected a string, got {}", i + 1, c.show(x))),
    };
    c.seq(v, at)?.into_iter().enumerate().map(one).collect()
}

/// First line of an item text without a leading "Rarity: UNIQUE" line.
fn first_line_name(text: &str) -> &str {
    let body = text.strip_prefix(RARITY_PREFIX).unwrap_or(text);
    body.split('\n').next().unwrap_or(body)
}

/// Where the old wire put each mod table first (its `$ref` target): itemMods categories in serialization order, then
/// veiledMods.
/// (mod table id -> (source, key), category table id -> source)
type Canonical = (HashMap<u32, (String, String)>, HashMap<u32, String>);

fn canonical_mods(c: &Ctx) -> R<Canonical> {
    let mut mods = HashMap::new();
    let mut cats = HashMap::new();
    let im = c.d.path("data", &["itemMods"]).ok_or("data.itemMods missing")?;
    let imt = c.d.t(c.table(im, "data.itemMods")?);
    let mut sources: Vec<(String, &V)> = Vec::new();
    for cat in REAL_CATEGORIES.iter().chain(&["Vestigial", "Item"]) {
        if let Some(v) = imt.get(cat) {
            sources.push((format!("itemMods.{cat}"), v));
        }
    }
    sources.push(("veiledMods".into(), c.d.path("data", &["veiledMods"]).ok_or("data.veiledMods missing")?));
    for (source, v) in sources {
        let t = c.table(v, &format!("data.{source}"))?;
        cats.entry(t).or_insert_with(|| source.clone());
        for (k, m) in c.str_entries(v, &format!("data.{source}"))? {
            if let Some(id) = m.as_table() {
                mods.entry(id).or_insert_with(|| (source.clone(), k.to_owned()));
            }
        }
    }
    Ok((mods, cats))
}

pub fn uniques(c: &mut Ctx) -> R<J> {
    const FILE: &str = "uniques.json";
    let d = c.d;
    let before = d.path("data", &["uniques"]).ok_or("data.uniques missing")?;
    let after = d.path("dataAfter", &["uniques"]).ok_or("dataAfter.uniques missing (dump without the post-boot root)")?;
    let before = c.str_entries(before, "data.uniques")?;
    let after = c.str_entries(after, "dataAfter.uniques")?;
    c.count(FILE, "item types", after.len());

    let mut by_type = Vec::new();
    let mut tree_dependent = Vec::new();
    for &(ty, v) in &after {
        let at = format!("dataAfter.uniques.{ty}");
        let list = texts(c, v, &at)?;
        let pre = match before.iter().find(|e| e.0 == ty) {
            Some(&(_, b)) => texts(c, b, &format!("data.uniques.{ty}"))?,
            None => {
                c.drift(FILE, "uniques", format!("type '{ty}' appears only after the boot"));
                Vec::new()
            }
        };
        if ty == "generated" {
            if !list.starts_with(&pre) {
                return Err(format!("{at}: the boot did not keep the {} pre-boot entries as a prefix", pre.len()));
            }
            tree_dependent = list[pre.len()..].iter().map(|t| J::str(first_line_name(t))).collect();
            c.count(FILE, "generated before / after the boot", pre.len());
            c.count(FILE, "tree-dependent generated", tree_dependent.len());
        } else if list != pre {
            c.drift(FILE, "uniques", format!("type '{ty}' changed during the boot (post-boot texts written)"));
        }
        by_type.push((ty.to_owned(), J::Arr(list.into_iter().map(J::str).collect())));
    }
    for &(ty, _) in &before {
        if !after.iter().any(|e| e.0 == ty) {
            return Err(format!("data.uniques.{ty}: gone after the boot"));
        }
    }

    let (mods, _) = canonical_mods(c)?;
    let um = d.path("data", &["uniqueMods"]).ok_or("data.uniqueMods missing")?;
    let mut we_ids = Vec::new();
    for (k, v) in c.str_entries(um, "data.uniqueMods")? {
        if k != "Watcher's Eye" {
            c.drift(FILE, "data.uniqueMods", format!("unknown key '{k}'"));
            continue;
        }
        for (i, e) in c.seq(v, "data.uniqueMods[Watcher's Eye]")?.into_iter().enumerate() {
            let at = format!("data.uniqueMods[Watcher's Eye][{}]", i + 1);
            let mut f = Fields::new(c, e, &at)?;
            let id = f.string(c, "Id")?;
            let m = f.req("mod")?;
            let target = m.as_table().and_then(|t| mods.get(&t));
            if target != Some(&("itemMods.WatchersEye".to_owned(), id.clone())) {
                return Err(format!("{at}.mod: not data.itemMods.WatchersEye['{id}'] (got {target:?})"));
            }
            f.finish(c, FILE, "Watcher's Eye mod");
            we_ids.push(J::Str(id));
        }
    }
    if we_ids.is_empty() && !c.str_entries(um, "data.uniqueMods")?.iter().any(|e| e.0 == "Watcher's Eye") {
        return Err("data.uniqueMods[Watcher's Eye]: missing".into());
    }

    let mut o = Obj::default();
    o.put("byType", J::Obj(by_type));
    o.put("treeDependentGenerated", J::Arr(tree_dependent));
    o.put("watchersEyeModIds", J::Arr(we_ids));
    Ok(o.done())
}

fn text_list(c: &mut Ctx, key: &str, file: &str) -> R<J> {
    let v = c.d.path("data", &[key]).ok_or_else(|| format!("data.{key} missing"))?;
    let list = texts(c, v, &format!("data.{key}"))?;
    c.count(file, "items", list.len());
    Ok(J::Arr(list.into_iter().map(J::str).collect()))
}

pub fn rares(c: &mut Ctx) -> R<J> {
    text_list(c, "rares", "rares.json")
}

/// `J::Null` (no file) on an upstream before Data/Viscera.lua.
pub fn viscera(c: &mut Ctx) -> R<J> {
    if c.d.path("data", &["viscera"]).is_none() {
        return Ok(J::Null);
    }
    text_list(c, "viscera", "viscera.json")
}

const AFFIX_SOURCES: &[&str] = &["itemMods.Explicit", "itemMods.JewelAbyss", "itemMods.JewelCharm", "veiledMods"];

fn only_true(c: &Ctx, v: Option<&V>, at: &str) -> R<Option<J>> {
    match v {
        None => Ok(None),
        Some(V::Bool(true)) => Ok(Some(J::Bool(true))),
        Some(v) => Err(format!("{at}: expected true, got {}", c.show(v))),
    }
}

pub fn rare_like_uniques(c: &mut Ctx) -> R<J> {
    const FILE: &str = "rare-like-uniques.json";
    let d = c.d;
    let (mods, cats) = canonical_mods(c)?;
    let lists = d.path("data", &["itemBaseLists"]).ok_or("data.itemBaseLists missing")?;
    let lists: HashMap<u32, &str> =
        c.str_entries(lists, "data.itemBaseLists")?.into_iter().filter_map(|(k, v)| v.as_table().map(|t| (t, k))).collect();
    let w = d.path("data", &["rareLikeUniques"]).ok_or("data.rareLikeUniques missing")?;
    let mut out = Vec::new();
    for (name, raw) in c.str_entries(w, "data.rareLikeUniques")? {
        let at = format!("data.rareLikeUniques[{name}]");
        let mut f = Fields::new(c, raw, &at)?;

        let aff = f.req("affixes")?;
        let aff_t = c.table(aff, &format!("{at}.affixes"))?;
        let mut affixes: Vec<(String, String)> = Vec::new();
        if let Some(source) = cats.get(&aff_t) {
            // The whole category table (`affixes = data.itemMods.Explicit`).
            if !AFFIX_SOURCES.contains(&source.as_str()) {
                return Err(format!("{at}.affixes: unexpected whole category '{source}'"));
            }
            for (k, _) in c.str_entries(aff, &format!("{at}.affixes"))? {
                affixes.push((source.clone(), k.to_owned()));
            }
        } else {
            for (k, m) in c.str_entries(aff, &format!("{at}.affixes"))? {
                let a = format!("{at}.affixes.{k}");
                let (source, id) = m.as_table().and_then(|t| mods.get(&t)).ok_or_else(|| format!("{a}: not a mod of a known category"))?;
                if id != k || !AFFIX_SOURCES.contains(&source.as_str()) {
                    return Err(format!("{a}: is {source}['{id}'], expected one of {AFFIX_SOURCES:?} under the same id"));
                }
                affixes.push((source.clone(), id.clone()));
            }
        }
        affixes.sort();
        let affixes = affixes.into_iter().map(|(s, id)| J::Obj(vec![("source".into(), J::Str(s)), ("id".into(), J::Str(id))]));

        let valid_bases = match f.take("validBases") {
            None => None,
            Some(vb) => {
                let a = format!("{at}.validBases");
                let t = c.table(vb, &a)?;
                Some(if let Some(list) = lists.get(&t) {
                    J::Obj(vec![("listType".into(), J::str(list))])
                } else {
                    let mut bases = Vec::new();
                    for (i, e) in c.seq(vb, &a)?.into_iter().enumerate() {
                        let a = format!("{a}[{}]", i + 1);
                        let mut ef = Fields::new(c, e, &a)?;
                        let base = ef.req("base")?;
                        c.table(base, &format!("{a}.base"))?;
                        let base = c.raw(base, &format!("{a}.base"))?;
                        ef.finish(c, FILE, "validBases entry");
                        bases.push(J::Obj(vec![("base".into(), base)]));
                    }
                    J::Obj(vec![("bases".into(), J::Arr(bases))])
                })
            }
        };

        let mut o = Obj::default();
        o.put("affixes", J::Arr(affixes.collect()));
        o.opt("validBases", valid_bases);
        o.put("prefixLimit", J::Num(f.num(c, "prefixLimit")?));
        o.put("suffixLimit", J::Num(f.num(c, "suffixLimit")?));
        o.opt("ignoreModType", only_true(c, f.take("ignoreModType"), &format!("{at}.ignoreModType"))?);
        o.opt("allowDuplicateGroups", only_true(c, f.take("allowDuplicateGroups"), &format!("{at}.allowDuplicateGroups"))?);
        if let Some(scm) = f.take("supportsCustomModifiers") {
            let a = format!("{at}.supportsCustomModifiers");
            let one = |(k, v): (&str, &V)| only_true(c, Some(v), &format!("{a}.{k}")).map(|j| (k.to_owned(), j.unwrap()));
            o.put("supportsCustomModifiers", J::Obj(c.str_entries(scm, &a)?.into_iter().map(one).collect::<R<_>>()?));
        }
        f.finish(c, FILE, "rare-like unique");
        out.push((name.to_owned(), o.done()));
    }
    c.count(FILE, "descriptions", out.len());
    Ok(J::Obj(out))
}

