//! The hand-written rule tables of Modules/Data.lua (weapon classes, ailments, curse priority, jewel radii, timeless
//! jewels, boss stats, the power-stat list, ...) -> rules.json. Structs keep the file's field order (an unknown field is
//! drift); records and lists pass through in Lua key order. Code is never kept: `powerStatList[i].transform` closures
//! become a named op, `powerStatList.GetFromOutput` is dropped.

use crate::closures::{js_cmp, source};
use crate::ctx::{Ctx, Obj, R, key_text};
use crate::dump::{Func, V};
use crate::fields::Fields;
use crate::js::J;

const FILE: &str = "rules.json";

#[derive(Clone, Copy)]
enum K {
    Bool,
    Num,
    Str,
    StrArr,
}

fn leaf(c: &Ctx, v: &V, k: K, at: &str) -> R<J> {
    Ok(match (k, v) {
        (K::Bool, V::Bool(b)) => J::Bool(*b),
        (K::Num, V::Num(n)) if n.is_finite() => J::Num(*n),
        (K::Str, V::Str(s)) => J::str(s),
        (K::StrArr, _) => strings(c, v, at)?,
        (k, v) => {
            let want = match k {
                K::Bool => "a boolean",
                K::Num => "a finite number",
                _ => "a string",
            };
            return Err(format!("{at}: expected {want}, got {}", c.show(v)));
        }
    })
}

fn strings(c: &Ctx, v: &V, at: &str) -> R<J> {
    let items = c.seq(v, at)?;
    Ok(J::Arr(items.into_iter().enumerate().map(|(i, x)| leaf(c, x, K::Str, &format!("{at}[{}]", i + 1))).collect::<R<_>>()?))
}

fn numbers(c: &Ctx, v: &V, at: &str) -> R<J> {
    let items = c.seq(v, at)?;
    Ok(J::Arr(items.into_iter().enumerate().map(|(i, x)| leaf(c, x, K::Num, &format!("{at}[{}]", i + 1))).collect::<R<_>>()?))
}

/// A struct: `(field, kind, required)` in the file's order; anything else is drift.
fn strukt(c: &mut Ctx, v: &V, at: &str, kind: &str, fields: &[(&str, K, bool)]) -> R<Obj> {
    let mut f = Fields::new(c, v, at)?;
    let mut o = Obj::default();
    for &(k, ty, required) in fields {
        let x = if required { Some(f.req(k)?) } else { f.take(k) };
        if let Some(x) = x {
            o.put(k, leaf(c, x, ty, &format!("{at}.{k}"))?);
        }
    }
    f.finish(c, FILE, kind);
    Ok(o)
}

/// `(key text, value)` of a table in Lua key order.
fn entries<'a>(c: &Ctx<'a>, v: &V, at: &str) -> R<Vec<(String, &'a V)>> {
    let t = c.d.t(c.table(v, at)?);
    t.entries.iter().map(|(k, x)| Ok((key_text(k, at)?, x))).collect()
}

fn get<'a>(c: &Ctx<'a>, k: &str) -> R<&'a V> {
    c.d.path("data", &[k]).ok_or_else(|| format!("data.{k}: missing"))
}

/// An open record whose values all convert with `f`.
fn record(c: &mut Ctx, k: &str, mut f: impl FnMut(&mut Ctx, &V, &str) -> R<J>) -> R<J> {
    let at = format!("data.{k}");
    let v = get(c, k)?;
    let mut o = Vec::new();
    for (key, x) in entries(c, v, &at)? {
        let j = f(c, x, &format!("{at}.{key}"))?;
        o.push((key, j));
    }
    c.count(FILE, k, o.len());
    Ok(J::Obj(o))
}

fn list(c: &mut Ctx, k: &str, f: fn(&Ctx, &V, &str) -> R<J>) -> R<J> {
    let j = f(c, get(c, k)?, &format!("data.{k}"))?;
    if let J::Arr(a) = &j {
        c.count(FILE, k, a.len());
    }
    Ok(j)
}

/// `{ [name] = true }` -> sorted names.
fn true_set(c: &mut Ctx, k: &str) -> R<J> {
    let at = format!("data.{k}");
    let mut names = Vec::new();
    for (name, x) in entries(c, get(c, k)?, &at)? {
        if *x != V::Bool(true) {
            return Err(format!("{at}.{name}: expected true, got {}", c.show(x)));
        }
        names.push(name);
    }
    names.sort_by(|a, b| js_cmp(a, b));
    c.count(FILE, k, names.len());
    Ok(J::Arr(names.into_iter().map(J::Str).collect()))
}

/// tag -> slot -> patterns (itemTagSpecial, itemTagSpecialExclusionPattern); an empty slot map is `{}`.
fn tag_slot_patterns(c: &mut Ctx, k: &str) -> R<J> {
    let at = format!("data.{k}");
    let mut o = Vec::new();
    for (tag, x) in entries(c, get(c, k)?, &at)? {
        let at = format!("{at}.{tag}");
        let slots = entries(c, x, &at)?.into_iter().map(|(slot, p)| Ok((slot.clone(), strings(c, p, &format!("{at}.{slot}"))?)));
        o.push((tag, J::Obj(slots.collect::<R<_>>()?)));
    }
    Ok(J::Obj(o))
}

fn unarmed(c: &mut Ctx) -> R<J> {
    const FIELDS: &[(&str, K, bool)] =
        &[("type", K::Str, true), ("AttackRate", K::Num, true), ("CritChance", K::Num, true), ("PhysicalMin", K::Num, true), ("PhysicalMax", K::Num, true)];
    let v = get(c, "unarmedWeaponData")?;
    let t = c.d.t(c.table(v, "data.unarmedWeaponData")?);
    let mut out = Vec::new();
    for (k, x) in &t.entries {
        let at = format!("data.unarmedWeaponData[{}]", key_text(k, "data.unarmedWeaponData")?);
        let class = match k {
            V::Num(n) if n.fract() == 0.0 && *n >= 0.0 => *n,
            k => return Err(format!("{at}: expected a class id key, got {}", c.show(k))),
        };
        let mut o = Obj::default();
        o.put("classId", J::Num(class));
        o.0.extend(strukt(c, x, &at, "unarmedWeaponData entry", FIELDS)?.0);
        out.push(o.done());
    }
    c.count(FILE, "unarmedWeaponData", out.len());
    Ok(J::Arr(out))
}

/// jewelRadii[version][i]: `{inner, outer, col, label}`; `innerSquared`/`outerSquared` are what setJewelRadiiGlobally
/// derives in place for the selected version (D7): checked, then dropped.
fn jewel_radii(c: &mut Ctx) -> R<J> {
    let mut o = Vec::new();
    for (ver, arr) in entries(c, get(c, "jewelRadii")?, "data.jewelRadii")? {
        let mut out = Vec::new();
        for (i, e) in c.seq(arr, &format!("data.jewelRadii.{ver}"))?.into_iter().enumerate() {
            let at = format!("data.jewelRadii.{ver}[{}]", i + 1);
            let mut f = Fields::new(c, e, &at)?;
            let mut r = Obj::default();
            let inner = f.num(c, "inner")?;
            let outer = f.num(c, "outer")?;
            r.put("inner", J::Num(inner));
            r.put("outer", J::Num(outer));
            r.put("col", J::Str(f.string(c, "col")?));
            r.put("label", J::Str(f.string(c, "label")?));
            for (k, want) in [("innerSquared", inner * inner), ("outerSquared", outer * outer)] {
                if let Some(x) = f.take(k)
                    && x.as_num() != Some(want)
                {
                    return Err(format!("{at}.{k}: derived field (D7) is {}, expected {want}", c.show(x)));
                }
            }
            f.finish(c, FILE, "jewelRadii entry");
            out.push(r.done());
        }
        o.push((ver, J::Arr(out)));
    }
    c.count(FILE, "jewelRadii versions", o.len());
    Ok(J::Obj(o))
}

/// D1: a `transform` closure -> the named op its body performs.
fn transform(c: &Ctx, v: &V, at: &str) -> R<J> {
    let func = if let V::Func(f) = v { Some(c.d.f(*f)) } else { None };
    let Some(Func::Lua { src, from, to, .. }) = func else {
        return Err(format!("{at}: expected a Lua function, got {}", c.show(v)));
    };
    let body = source(&c.pob_root, src, *from, *to)?;
    if body.contains("return -value") {
        return Ok(J::str("negate"));
    }
    if body.contains("gsub(\"^The \"") {
        return Ok(J::str("stripThePrefix"));
    }
    Err(format!("{at}: unrecognized transform closure body: {}", body.chars().take(200).collect::<String>()))
}

fn power_stats(c: &mut Ctx) -> R<J> {
    const FIELDS: &[(&str, K, bool)] = &[
        ("stat", K::Str, false),
        ("label", K::Str, true),
        ("combinedOffDef", K::Bool, false),
        ("ignoreForItems", K::Bool, false),
        ("ignoreForNodes", K::Bool, false),
        ("reverseSort", K::Bool, false),
        ("itemField", K::Str, false),
    ];
    let v = get(c, "powerStatList")?;
    let t = c.d.t(c.table(v, "data.powerStatList")?);
    let mut out = Vec::new();
    for (k, x) in &t.entries {
        match k {
            V::Num(n) if *n == (out.len() + 1) as f64 => {}
            // Code (reads a stat out of a calc output): not data.
            V::Str(s) if &**s == "GetFromOutput" && matches!(x, V::Func(_)) => continue,
            k => {
                c.drift(FILE, "powerStatList", format!("unknown key {}", key_text(k, "data.powerStatList")?));
                continue;
            }
        }
        let at = format!("data.powerStatList[{}]", out.len() + 1);
        let mut f = Fields::new(c, x, &at)?;
        let mut o = Obj::default();
        for &(k, ty, required) in FIELDS {
            let y = if required { Some(f.req(k)?) } else { f.take(k) };
            if let Some(y) = y {
                o.put(k, leaf(c, y, ty, &format!("{at}.{k}"))?);
            }
        }
        if let Some(y) = f.take("transform") {
            o.put("transform", transform(c, y, &format!("{at}.transform"))?);
        }
        f.finish(c, FILE, "powerStatList entry");
        out.push(o.done());
    }
    c.count(FILE, "powerStatList", out.len());
    Ok(J::Arr(out))
}

pub fn convert(c: &mut Ctx) -> R<J> {
    let mut o = Obj::default();
    o.put(
        "weaponTypeInfo",
        record(c, "weaponTypeInfo", |c, v, at| {
            let f = &[("oneHand", K::Bool, true), ("melee", K::Bool, true), ("flag", K::Str, true), ("label", K::Str, false)];
            Ok(strukt(c, v, at, "weaponTypeInfo entry", f)?.done())
        })?,
    );
    o.put("unarmedWeaponData", unarmed(c)?);
    o.put(
        "nonDamagingAilment",
        record(c, "nonDamagingAilment", |c, v, at| {
            let f = &[
                ("associatedType", K::Str, true),
                ("alt", K::Bool, true),
                ("default", K::Num, false),
                ("min", K::Num, true),
                ("max", K::Num, true),
                ("precision", K::Num, true),
                ("duration", K::Num, false),
            ];
            Ok(strukt(c, v, at, "nonDamagingAilment entry", f)?.done())
        })?,
    );
    for k in ["ailmentTypeList", "elementalAilmentTypeList", "nonDamagingAilmentTypeList", "nonElementalAilmentTypeList"] {
        o.put(k, list(c, k, strings)?);
    }
    o.put(
        "highPrecisionMods",
        record(c, "highPrecisionMods", |c, v, at| {
            Ok(strukt(c, v, at, "highPrecisionMods entry", &[("BASE", K::Num, false), ("MORE", K::Num, false)])?.done())
        })?,
    );
    o.put("defaultHighPrecision", leaf(c, get(c, "defaultHighPrecision")?, K::Num, "data.defaultHighPrecision")?);
    o.put("cursePriority", record(c, "cursePriority", |c, v, at| leaf(c, v, K::Num, at))?);
    o.put("keystones", list(c, "keystones", strings)?);
    o.put("jewelRadii", jewel_radii(c)?);
    o.put("skillColorMap", list(c, "skillColorMap", strings)?);
    let mut sources = Vec::new();
    for (i, e) in c.seq(get(c, "enchantmentSource")?, "data.enchantmentSource")?.into_iter().enumerate() {
        let at = format!("data.enchantmentSource[{}]", i + 1);
        sources.push(strukt(c, e, &at, "enchantmentSource entry", &[("name", K::Str, true), ("label", K::Str, true)])?.done());
    }
    o.put("enchantmentSource", J::Arr(sources));
    o.put("itemTagSpecial", tag_slot_patterns(c, "itemTagSpecial")?);
    o.put("itemTagSpecialExclusionPattern", tag_slot_patterns(c, "itemTagSpecialExclusionPattern")?);
    o.put("timelessJewelTypes", list(c, "timelessJewelTypes", strings)?);
    o.put("timelessJewelSeedMin", list(c, "timelessJewelSeedMin", numbers)?);
    o.put("timelessJewelSeedMax", list(c, "timelessJewelSeedMax", numbers)?);
    let mut trade = Vec::new();
    for (i, e) in c.seq(get(c, "timelessJewelTradeIDs")?, "data.timelessJewelTradeIDs")?.into_iter().enumerate() {
        let at = format!("data.timelessJewelTradeIDs[{}]", i + 1);
        trade.push(strukt(c, e, &at, "timelessJewelTradeIDs entry", &[("keystone", K::StrArr, false), ("devotion", K::StrArr, false)])?.done());
    }
    o.put("timelessJewelTradeIDs", J::Arr(trade));
    o.put("timelessJewelAdditions", leaf(c, get(c, "timelessJewelAdditions")?, K::Num, "data.timelessJewelAdditions")?);
    for k in ["casterTagCrucibleUniques", "minionTagCrucibleUniques", "vestigialUniqueBaseTypes"] {
        o.put(k, true_set(c, k)?);
    }
    o.put("vestigialModMappings", record(c, "vestigialModMappings", |c, v, at| leaf(c, v, K::Str, at))?);
    let boss = &[
        ("PinnacleArmourMean", K::Num, true),
        ("PinnacleEvasionMean", K::Num, true),
        ("UberArmourMean", K::Num, true),
        ("UberEvasionMean", K::Num, true),
    ];
    o.put("bossStats", strukt(c, get(c, "bossStats")?, "data.bossStats", "bossStats", boss)?.done());
    o.put("enemyIsBossTooltip", leaf(c, get(c, "enemyIsBossTooltip")?, K::Str, "data.enemyIsBossTooltip")?);
    o.put("powerStatList", power_stats(c)?);
    Ok(o.done())
}
