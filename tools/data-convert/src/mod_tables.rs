//! The other mod tables of Modules/Data.lua -> master-mods.json, veiled-mods.json, necropolis-mods.json, essences.json,
//! enchantments.json, mod-scalability.json. Records are read field by field (item_mods::affix_record); what is left
//! over is drift.

use crate::ctx::{Ctx, Obj, R};
use crate::dump::V;
use crate::fields::Fields;
use crate::item_mods::{K, Spec, affix_record, strings};
use crate::js::J;

const MASTER_MOD: Spec = &[
    ("type", K::Str, true),
    ("affix", K::Str, true),
    ("group", K::Str, true),
    ("level", K::Num, true),
    ("statOrder", K::Nums, true),
    ("modTags", K::Strs, true),
    ("types", K::TrueSet, true),
];

const AFFIX_MOD: Spec = &[
    ("type", K::Str, true),
    ("affix", K::Str, true),
    ("group", K::Str, true),
    ("level", K::Num, true),
    ("statOrder", K::Nums, true),
    ("modTags", K::Strs, true),
    ("weightKey", K::Strs, true),
    ("weightVal", K::Nums, true),
    ("tradeHashes", K::TradeHashes, true),
];

fn data<'a>(c: &Ctx<'a>, k: &str) -> R<&'a V> {
    c.d.path("data", &[k]).ok_or_else(|| format!("data.{k} missing"))
}

pub fn master_mods(c: &mut Ctx) -> R<J> {
    const FILE: &str = "master-mods.json";
    let mut out = Vec::new();
    for (i, m) in c.seq(data(c, "masterMods")?, "data.masterMods")?.into_iter().enumerate() {
        out.push(affix_record(c, m, &format!("data.masterMods[{}]", i + 1), FILE, "master mod", MASTER_MOD)?);
    }
    c.count(FILE, "mods", out.len());
    Ok(J::Arr(out))
}

fn affix_table(c: &mut Ctx, key: &str, file: &str) -> R<J> {
    let mut out = Vec::new();
    for (id, m) in c.str_entries(data(c, key)?, &format!("data.{key}"))? {
        out.push((id.to_owned(), affix_record(c, m, &format!("data.{key}.{id}"), file, "mod", AFFIX_MOD)?));
    }
    c.count(file, "mods", out.len());
    Ok(J::Obj(out))
}

pub fn veiled_mods(c: &mut Ctx) -> R<J> {
    affix_table(c, "veiledMods", "veiled-mods.json")
}

pub fn necropolis_mods(c: &mut Ctx) -> R<J> {
    affix_table(c, "necropolisMods", "necropolis-mods.json")
}

pub fn essences(c: &mut Ctx) -> R<J> {
    const FILE: &str = "essences.json";
    let mut out = Vec::new();
    for (id, e) in c.str_entries(data(c, "essences")?, "data.essences")? {
        let at = format!("data.essences.{id}");
        let mut f = Fields::new(c, e, &at)?;
        let mut o = Obj::default();
        o.put("name", J::Str(f.string(c, "name")?));
        o.put("type", J::Num(f.num(c, "type")?));
        o.put("tier", J::Num(f.num(c, "tier")?));
        let mods = c.str_entries(f.req("mods")?, &format!("{at}.mods"))?;
        let mods = mods.into_iter().map(|(k, v)| Ok((k.to_owned(), J::Str(c.string(v, &format!("{at}.mods.{k}"))?))));
        o.put("mods", J::Obj(mods.collect::<R<_>>()?));
        f.finish(c, FILE, "essence");
        out.push((id.to_owned(), o.done()));
    }
    c.count(FILE, "essences", out.len());
    Ok(J::Obj(out))
}

/// Data.lua loads these 7 from their own files; every other key of `data.enchantments` is derived from them (Flask and
/// one per weapon type) and is not written.
const ENCHANT_REAL: &[&str] = &["Helmet", "Boots", "Gloves", "Belt", "Body Armour", "Weapon", "UtilityFlask"];
/// Derived keys Data.lua adds; anything else is drift.
const ENCHANT_DERIVED_FLASK: &str = "Flask";

/// source label -> lines.
fn enchant_sources(c: &Ctx, v: &V, at: &str) -> R<J> {
    let one = |(k, x): (&str, &V)| Ok((k.to_owned(), strings(c, x, &format!("{at}.{k}"))?));
    Ok(J::Obj(c.str_entries(v, at)?.into_iter().map(one).collect::<R<_>>()?))
}

pub fn enchantments(c: &mut Ctx) -> R<J> {
    const FILE: &str = "enchantments.json";
    let w = data(c, "enchantments")?;
    let t = c.d.t(c.table(w, "data.enchantments")?);
    // Only to tell derived keys from new ones (read without marking `weaponTypeInfo` as converted).
    let root = c.d.t(c.table(c.d.root("data"), "data")?);
    let weapon_types: Vec<String> = match root.get("weaponTypeInfo") {
        Some(wt) => c.str_entries(wt, "data.weaponTypeInfo")?.into_iter().map(|(k, _)| k.to_owned()).collect(),
        None => Vec::new(),
    };
    for (k, _) in c.str_entries(w, "data.enchantments")? {
        if !ENCHANT_REAL.contains(&k) && k != ENCHANT_DERIVED_FLASK && !weapon_types.iter().any(|x| x == k) {
            c.drift(FILE, "data.enchantments", format!("unknown key '{k}'"));
        }
    }
    let mut o = Obj::default();
    for &k in ENCHANT_REAL {
        let at = format!("data.enchantments.{k}");
        let v = t.get(k).ok_or_else(|| format!("{at}: missing"))?;
        let j = if k == "Helmet" {
            // enchant id -> source label -> lines (two levels)
            let one = |(id, x): (&str, &V)| Ok((id.to_owned(), enchant_sources(c, x, &format!("{at}.{id}"))?));
            J::Obj(c.str_entries(v, &at)?.into_iter().map(one).collect::<R<_>>()?)
        } else {
            enchant_sources(c, v, &at)?
        };
        o.put(k, j);
    }
    Ok(o.done())
}

pub fn mod_scalability(c: &mut Ctx) -> R<J> {
    const FILE: &str = "mod-scalability.json";
    let mut out = Vec::new();
    for (line, arr) in c.str_entries(data(c, "modScalability")?, "data.modScalability")? {
        let at = format!("data.modScalability[{line:?}]");
        let mut items = Vec::new();
        for (i, e) in c.seq(arr, &at)?.into_iter().enumerate() {
            let at = format!("{at}[{}]", i + 1);
            let mut f = Fields::new(c, e, &at)?;
            let mut o = Obj::default();
            match f.req("isScalable")? {
                V::Bool(b) => o.put("isScalable", J::Bool(*b)),
                v => return Err(format!("{at}.isScalable: expected a boolean, got {}", c.show(v))),
            }
            o.opt("formats", f.take("formats").map(|v| strings(c, v, &format!("{at}.formats"))).transpose()?);
            f.finish(c, FILE, "scalability entry");
            items.push(o.done());
        }
        out.push((line.to_owned(), J::Arr(items)));
    }
    c.count(FILE, "lines", out.len());
    Ok(J::Obj(out))
}
