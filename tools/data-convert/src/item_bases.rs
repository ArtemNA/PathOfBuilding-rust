//! `data.itemBases` + `itemBaseLists` + `itemBaseTypeList` (Modules/Data.lua, Data/Bases/*.lua) -> item-bases.json.
//! Reader: crates/pob-data/src/item_bases.rs.

use crate::ctx::{Ctx, Obj, R};
use crate::dump::V;
use crate::fields::Fields;
use crate::js::J;

const FILE: &str = "item-bases.json";

fn strings(c: &Ctx, v: &V, at: &str) -> R<J> {
    Ok(J::Arr(c.seq(v, at)?.into_iter().map(|x| c.string(x, at).map(J::Str)).collect::<R<_>>()?))
}

fn mod_types(c: &Ctx, v: &V, at: &str) -> R<J> {
    Ok(J::Arr(c.seq(v, at)?.into_iter().map(|x| strings(c, x, at)).collect::<R<_>>()?))
}

/// A struct of optional numbers (plus `buff` for flasks), in the file's field order.
fn numbers(c: &mut Ctx, v: &V, at: &str, kind: &str, names: &[&str]) -> R<J> {
    let mut f = Fields::new(c, v, at)?;
    let mut o = Obj::default();
    for &k in names {
        if k == "buff" {
            if let Some(b) = f.take("buff") {
                o.put("buff", strings(c, b, &format!("{at}.buff"))?);
            }
        } else {
            o.opt(k, f.opt_num(c, k)?.map(J::Num));
        }
    }
    f.finish(c, FILE, kind);
    Ok(o.done())
}

pub fn convert(c: &mut Ctx) -> R<J> {
    let d = c.d;
    let get = |k: &str| d.path("data", &[k]).ok_or_else(|| format!("{FILE}: data.{k} missing"));
    let wbases = get("itemBases")?;
    let mut bases = Vec::new();
    for (name, b) in c.str_entries(wbases, "data.itemBases")? {
        let at = format!("data.itemBases[{name}]");
        let mut f = Fields::new(c, b, &at)?;
        let mut o = Obj::default();
        o.put("type", J::Str(f.string(c, "type")?));
        o.opt("subType", f.opt_string(c, "subType")?.map(J::Str));
        o.opt("socketLimit", f.opt_num(c, "socketLimit")?.map(J::Num));
        o.opt("hidden", f.opt_bool(c, "hidden")?.map(J::Bool));
        o.opt("cannotBeAnointed", f.opt_bool(c, "cannotBeAnointed")?.map(J::Bool));
        let tags = c.str_entries(f.req("tags")?, &at)?;
        let tags = tags.into_iter().map(|(k, v)| match v {
            V::Bool(true) => Ok((k.to_owned(), J::Bool(true))),
            v => Err(format!("{at}.tags.{k}: expected true, got {}", c.show(v))),
        });
        o.put("tags", J::Obj(tags.collect::<R<_>>()?));
        if let Some(it) = f.take("influenceTags") {
            let m = c.str_entries(it, &at)?.into_iter().map(|(k, v)| Ok((k.to_owned(), J::Str(c.string(v, &at)?))));
            o.put("influenceTags", J::Obj(m.collect::<R<_>>()?));
        }
        o.opt("implicit", f.opt_string(c, "implicit")?.map(J::Str));
        o.put("implicitModTypes", mod_types(c, f.req("implicitModTypes")?, &at)?);
        o.opt("implicitIds", f.take("implicitIds").map(|v| strings(c, v, &at)).transpose()?);
        o.opt("enchant", f.opt_string(c, "enchant")?.map(J::Str));
        o.opt("enchantModTypes", f.take("enchantModTypes").map(|v| mod_types(c, v, &at)).transpose()?);
        o.opt("enchantIds", f.take("enchantIds").map(|v| strings(c, v, &at)).transpose()?);
        o.opt("flavourText", f.take("flavourText").map(|v| strings(c, v, &at)).transpose()?);
        let req = f.req("req")?;
        o.put("req", numbers(c, req, &format!("{at}.req"), "item base req", &["level", "str", "dex", "int"])?);
        let structs: [(&str, &[&str]); 4] = [
            (
                "armour",
                &[
                    "ArmourBaseMin", "ArmourBaseMax", "EvasionBaseMin", "EvasionBaseMax", "EnergyShieldBaseMin", "EnergyShieldBaseMax",
                    "WardBaseMin", "WardBaseMax", "BlockChance", "MovementPenalty",
                ],
            ),
            ("weapon", &["PhysicalMin", "PhysicalMax", "CritChanceBase", "AttackRateBase", "Range"]),
            ("flask", &["life", "mana", "duration", "chargesUsed", "chargesMax", "buff"]),
            ("tincture", &["manaBurn", "cooldown"]),
        ];
        for (k, names) in structs {
            if let Some(v) = f.take(k) {
                o.put(k, numbers(c, v, &format!("{at}.{k}"), &format!("item base {k}"), names)?);
            }
        }
        f.finish(c, FILE, "item base");
        bases.push((name.to_owned(), o.done()));
    }
    c.count(FILE, "bases", bases.len());

    let base_table = |name: &str| d.t(wbases.as_table().unwrap()).get(name).cloned();
    let mut lists = Vec::new();
    for (ty, arr) in c.str_entries(get("itemBaseLists")?, "data.itemBaseLists")? {
        let at = format!("data.itemBaseLists[{ty}]");
        let mut out = Vec::new();
        for e in c.seq(arr, &at)? {
            let mut f = Fields::new(c, e, &at)?;
            let name = f.string(c, "name")?;
            let label = f.string(c, "label")?;
            if f.take("base").cloned() != base_table(&name) {
                return Err(format!("{at}: entry '{name}'.base is not data.itemBases['{name}']"));
            }
            f.finish(c, FILE, "item base list entry");
            out.push(J::Obj(vec![("label".into(), J::Str(label)), ("name".into(), J::Str(name))]));
        }
        lists.push((ty.to_owned(), J::Arr(out)));
    }
    let type_list = strings(c, get("itemBaseTypeList")?, "data.itemBaseTypeList")?;

    let mut out = Obj::default();
    out.put("bases", J::Obj(bases));
    out.put("lists", J::Obj(lists));
    out.put("typeList", type_list);
    Ok(out.done())
}
