//! `data.skills` (Data/Skills/*.lua + Modules/Data.lua post-processing) -> skills.json and `data.skillStatMap`
//! (Data/SkillStatMap.lua) -> skill-stat-map.json. Reader: crates/pob-data/src/skills.rs, skill_stat_map.rs.
//!
//! Shared tables (MINEFIELD D13: ExplosiveTrapAltX takes statMap/baseMods/parts/preDamageFunc from ExplosiveTrap) need no
//! special case here: the dump keeps sharing by identity, so each skill simply reads what it points at. `statMap._grantedEffect`
//! (a back-reference Data.lua sets, whichever sharer ran last) is checked to be a skill and dropped. Hook closures become the
//! sorted `hooks` list of the hook fields present.

use crate::ctx::{Ctx, R};
use crate::dump::V;
use crate::fields::Fields;
use crate::js::J;
use crate::mods::{looks_like_mod, normalize_mod};
use crate::shape::{F, S, Shaper, opt, req};

const FILE: &str = "skills.json";
const SSM_FILE: &str = "skill-stat-map.json";

const HOOKS: &[&str] = &["preDamageFunc", "initialFunc", "preSkillTypeFunc", "postCritFunc", "explosiveArrowFunc"];

const STAT_PAIR: S = S::Tuple(&[S::Str, S::Num]);
const PART: S = S::Obj(
    "skill part",
    &[
        req("name", S::Str),
        opt("area", S::Bool),
        opt("projectile", S::Bool),
        opt("melee", S::Bool),
        opt("stages", S::Bool),
        opt("cast", S::Bool),
        opt("spell", S::Bool),
        opt("chaining", S::Bool),
        opt("channelRelease", S::Bool),
        opt("attack", S::Bool),
        opt("hit", S::Bool),
    ],
);

/// Skill fields before `hooks` (schema order), then the ones after it.
const HEAD: &[F] = &[
    req("id", S::Str),
    req("name", S::Str),
    req("modSource", S::Str),
    req("levels", S::Custom(levels)),
    req("stats", S::Arr(&S::Str)),
    req("statMap", S::Custom(skill_stat_map)),
];
const TAIL: &[F] = &[
    opt("color", S::Num),
    opt("statDescriptionScope", S::Str),
    opt("description", S::Str),
    opt("constantStats", S::Arr(&STAT_PAIR)),
    opt("baseFlags", S::Custom(base_flags)),
    opt("skillTypes", S::Custom(skill_types)),
    opt("castTime", S::Num),
    opt("qualityStats", S::Arr(&STAT_PAIR)),
    opt("baseEffectiveness", S::Num),
    opt("hidden", S::Bool),
    opt("notMinionStat", S::Arr(&S::Str)),
    opt("baseTypeName", S::Str),
    opt("incrementalEffectiveness", S::Num),
    opt("baseMods", S::Custom(mod_or_group_list)),
    opt("weaponTypes", S::Rec(&S::Bool)),
    opt("addSkillTypes", S::Arr(&S::Num)),
    opt("excludeSkillTypes", S::Arr(&S::Num)),
    opt("requireSkillTypes", S::Arr(&S::Num)),
    opt("support", S::Bool),
    opt("hasGlobalEffect", S::Bool),
    opt("fromItem", S::Bool),
    opt("parts", S::Arr(&PART)),
    opt("minionList", S::Arr(&S::Str)),
    opt("cannotBeSupported", S::Bool),
    opt("ignoreMinionTypes", S::Bool),
    opt("minionSkillTypes", S::Rec(&S::Bool)),
    opt("flavourText", S::Arr(&S::Str)),
    opt("plusVersionOf", S::Str),
    opt("isTrigger", S::Bool),
    opt("legacy", S::Bool),
    opt("fromTree", S::Bool),
    opt("skillTotemId", S::Num),
    opt("minionUses", S::Rec(&S::Bool)),
    opt("addFlags", S::Rec(&S::Bool)),
    opt("supportGemsOnly", S::Bool),
    opt("minionHasItemSet", S::Bool),
    opt("levelMods", S::Custom(mod_or_group_list)),
    opt("hideFromGemList", S::Bool),
    opt("addMinionList", S::Arr(&S::Str)),
];

/// Level entry fields after `level`/`values`, in schema order.
const LEVEL_FIELDS: &[&str] = &[
    "levelRequirement", "statInterpolation", "cost", "critChance", "damageEffectiveness", "manaMultiplier", "baseMultiplier",
    "cooldown", "storedUses", "attackSpeedMultiplier", "PvPDamageMultiplier", "soulPreventionDuration", "vaalStoredUses",
    "manaReservationPercent", "attackTime", "manaReservationFlat", "lifeReservationPercent", "duration",
];

fn num(c: &Ctx, v: &V, at: &str) -> R<J> {
    match v {
        V::Num(n) if n.is_finite() => Ok(J::Num(*n)),
        v => Err(format!("{at}: expected a finite number, got {}", c.show(v))),
    }
}

fn int_key(k: &V) -> Option<i64> {
    match k {
        V::Num(n) if n.fract() == 0.0 && *n >= 0.0 && *n < 1e15 => Some(*n as i64),
        _ => None,
    }
}

/// The contiguous `[1..k]` of a table (what `ipairs` sees) and the rest; a numeric key past a hole is fatal.
/// (ipairs part, other entries).
type Split<'a> = (Vec<&'a V>, Vec<(&'a V, &'a V)>);

fn split<'a>(c: &Ctx<'a>, v: &V, at: &str) -> R<Split<'a>> {
    let t = c.d.t(c.table(v, at)?);
    let mut list = Vec::new();
    let mut rest = Vec::new();
    for (k, x) in &t.entries {
        match int_key(k) {
            Some(i) if i == list.len() as i64 + 1 => list.push(x),
            Some(i) if i >= 1 => return Err(format!("{at}: gap before index {i}")),
            _ => rest.push((k, x)),
        }
    }
    Ok((list, rest))
}

/// `levels`: always `[{level, values, ...}]` ascending by level, whether the Lua table is 1..n or sparse (MINEFIELD L24).
fn levels(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    let t = c.d.t(c.table(v, at)?);
    let mut out = Vec::new();
    for (k, e) in &t.entries {
        let level = int_key(k).ok_or_else(|| format!("{at}: non-numeric levels key {}", c.show(k)))?;
        out.push((level, level_entry(c, level, e, &format!("{at}[{level}]"))?));
    }
    out.sort_by_key(|(l, _)| *l);
    Ok(J::Arr(out.into_iter().map(|(_, j)| j).collect()))
}

fn level_entry(c: &mut Ctx, level: i64, v: &V, at: &str) -> R<J> {
    let (values, rest) = split(c, v, at)?;
    let mut o = vec![("level".to_owned(), J::Num(level as f64)), ("values".to_owned(), J::Arr(values.into_iter().map(|x| num(c, x, at)).collect::<R<_>>()?))];
    let mut named: Vec<(&str, &V)> = Vec::new();
    for (k, x) in rest {
        match k {
            V::Str(s) if LEVEL_FIELDS.contains(&&**s) => named.push((s, x)),
            k => c.drift(FILE, "skill level", format!("unknown level field '{}'", crate::ctx::key_text(k, at)?)),
        }
    }
    for f in LEVEL_FIELDS {
        let Some((_, x)) = named.iter().find(|(k, _)| k == f) else { continue };
        let fat = format!("{at}.{f}");
        let j = match *f {
            "cost" => cost(c, x, &fat)?,
            "statInterpolation" => J::Arr(c.seq(x, &fat)?.into_iter().map(|y| num(c, y, &fat)).collect::<R<_>>()?),
            _ => num(c, x, &fat)?,
        };
        o.push((f.to_string(), j));
    }
    Ok(J::Obj(o))
}

/// Resource -> amount, sorted by resource; `{}` stays `{}`.
fn cost(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    let mut o = c.str_entries(v, at)?.into_iter().map(|(k, x)| Ok((k.to_owned(), num(c, x, &format!("{at}.{k}"))?))).collect::<R<Vec<_>>>()?;
    o.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(J::Obj(o))
}

/// `{ [SkillType.X] = true, ... }` -> sorted numbers.
fn skill_types(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    let t = c.d.t(c.table(v, at)?);
    let mut n = Vec::new();
    for (k, x) in &t.entries {
        let i = int_key(k).ok_or_else(|| format!("{at}: non-numeric key {}", c.show(k)))?;
        if *x != V::Bool(true) {
            return Err(format!("{at}[{i}]: expected true, got {}", c.show(x)));
        }
        n.push(i);
    }
    n.sort_unstable();
    Ok(J::Arr(n.into_iter().map(|i| J::Num(i as f64)).collect()))
}

/// `{ attack = true, ... }` -> sorted names.
fn base_flags(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    let mut n = Vec::new();
    for (k, x) in c.str_entries(v, at)? {
        if *x != V::Bool(true) {
            return Err(format!("{at}.{k}: expected true, got {}", c.show(x)));
        }
        n.push(k.to_owned());
    }
    n.sort();
    Ok(J::Arr(n.into_iter().map(J::Str).collect()))
}

/// A mod, or a group of mods sharing one scale `{ mod, mod, div = 60 }` -> `{mods, div?, mult?}` (never flattened).
fn mod_or_group(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    if looks_like_mod(c, v) {
        return normalize_mod(c, v, at, true);
    }
    let (list, rest) = split(c, v, at)?;
    let mods = list.into_iter().enumerate().map(|(i, m)| normalize_mod(c, m, &format!("{at}[{}]", i + 1), true)).collect::<R<_>>()?;
    let mut o = vec![("mods".to_owned(), J::Arr(mods))];
    let mut scale = Vec::new();
    for (k, x) in rest {
        match k.as_str() {
            Some(s @ ("div" | "mult")) => scale.push((s, num(c, x, &format!("{at}.{s}"))?)),
            _ => c.drift(FILE, "mod group", format!("unknown mod group field '{}'", crate::ctx::key_text(k, at)?)),
        }
    }
    for f in ["div", "mult"] {
        if let Some((_, j)) = scale.iter().find(|(k, _)| *k == f) {
            o.push((f.to_owned(), j.clone()));
        }
    }
    Ok(J::Obj(o))
}

fn mod_or_group_list(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    let items = c.seq(v, at)?;
    Ok(J::Arr(items.into_iter().enumerate().map(|(i, x)| mod_or_group(c, x, &format!("{at}[{}]", i + 1))).collect::<R<_>>()?))
}

/// A statMap entry: a plain list of mods/groups, or the list plus scaling fields -> `{mods, div?, mult?, base?, value?, skillFlag?}`.
fn stat_map_entry(c: &mut Ctx, v: &V, at: &str, file: &str) -> R<J> {
    let (list, rest) = split(c, v, at)?;
    let mods = list.into_iter().enumerate().map(|(i, m)| mod_or_group(c, m, &format!("{at}[{}]", i + 1))).collect::<R<_>>()?;
    let mut o = vec![("mods".to_owned(), J::Arr(mods))];
    let mut named = Vec::new();
    for (k, x) in rest {
        match k.as_str() {
            Some(s @ ("div" | "mult" | "base" | "value")) => named.push((s, num(c, x, &format!("{at}.{s}"))?)),
            // The old pipeline kept skillFlag only when it is a string.
            Some("skillFlag") => {
                if let V::Str(s) = x {
                    named.push(("skillFlag", J::str(s)));
                }
            }
            _ => c.drift(file, "statMap entry", format!("unknown statMap entry field '{}'", crate::ctx::key_text(k, at)?)),
        }
    }
    for f in ["div", "mult", "base", "value", "skillFlag"] {
        if let Some((_, j)) = named.iter().find(|(k, _)| *k == f) {
            o.push((f.to_owned(), j.clone()));
        }
    }
    Ok(J::Obj(o))
}

fn sorted_stat_map(c: &mut Ctx, v: &V, at: &str, file: &str, skip_back_ref: bool) -> R<J> {
    let mut o = Vec::new();
    for (stat, x) in c.str_entries(v, at)? {
        if skip_back_ref && stat == "_grantedEffect" {
            // Back-reference to (some sharer of) this skill: checked, never written.
            let ok = x.as_table().is_some_and(|t| matches!(c.d.t(t).get("id"), Some(V::Str(_))));
            if !ok {
                return Err(format!("{at}._grantedEffect: expected a skill, got {}", c.show(x)));
            }
            continue;
        }
        o.push((stat.to_owned(), stat_map_entry(c, x, &format!("{at}.{stat}"), file)?));
    }
    o.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(J::Obj(o))
}

fn skill_stat_map(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    sorted_stat_map(c, v, at, FILE, true)
}

fn skill(c: &mut Ctx, sh: &Shaper, id: &str, v: &V) -> R<J> {
    let at = format!("data.skills[{id}]");
    let mut f = Fields::new(c, v, &at)?;
    let mut o = sh.fields(c, &mut f, HEAD, &at)?;
    if !matches!(&o[0].1, J::Str(s) if s == id) {
        return Err(format!("{at}.id is not its key"));
    }
    let mut hooks = Vec::new();
    for h in HOOKS {
        match f.take(h) {
            None => {}
            Some(V::Func(_)) => hooks.push(*h),
            Some(x) => return Err(format!("{at}.{h}: expected a function, got {}", c.show(x))),
        }
    }
    hooks.sort_unstable();
    o.push(("hooks".into(), J::Arr(hooks.into_iter().map(J::str).collect())));
    o.extend(sh.fields(c, &mut f, TAIL, &at)?);
    f.finish(c, FILE, "skill");
    Ok(J::Obj(o))
}

pub fn convert(c: &mut Ctx) -> R<J> {
    let all = c.d.path("data", &["skills"]).ok_or("data.skills missing")?;
    let sh = Shaper { file: FILE };
    let mut out = Vec::new();
    for (id, v) in c.str_entries(all, "data.skills")? {
        out.push((id.to_owned(), skill(c, &sh, id, v)?));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    c.count(FILE, "skills", out.len());
    Ok(J::Obj(out))
}

pub fn convert_stat_map(c: &mut Ctx) -> R<J> {
    let v = c.d.path("data", &["skillStatMap"]).ok_or("data.skillStatMap missing")?;
    let j = sorted_stat_map(c, v, "data.skillStatMap", SSM_FILE, false)?;
    if let J::Obj(o) = &j {
        c.count(SSM_FILE, "stats", o.len());
    }
    Ok(j)
}
