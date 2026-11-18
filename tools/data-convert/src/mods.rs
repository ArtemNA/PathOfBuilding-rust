//! A PoB modifier table `{ name=, type=, value=, flags=, keywordFlags=, source=?, [1]=tag, ... }` -> the one Mod shape all
//! data files share: `{ name, type, value?, flags, keywordFlags, source?, tags, orphanTags?, div?, mult?, malformed? }`.
//! Tags are the contiguous `[1..k]` (what `ipairs` sees); tags behind a hole are `orphanTags`; upstream call-site bugs are
//! kept in `malformed` instead of failing (readers: crates/pob-data/src/wire.rs).

use crate::ctx::{Ctx, R, key_text};
use crate::dump::V;
use crate::js::J;

const MOD_TYPES: &[&str] = &["BASE", "INC", "MORE", "FLAG", "OVERRIDE", "LIST", "MAX", "MIN", "CHANCE", "DUMMY", "Flag"];
const NAMED: &[&str] = &["name", "type", "value", "flags", "keywordFlags", "source", "div", "mult"];

pub fn looks_like_mod(c: &Ctx, v: &V) -> bool {
    let Some(t) = v.as_table() else { return false };
    let t = c.d.t(t);
    matches!(t.get("name"), Some(V::Str(_))) && t.get("flags").is_some() && t.get("keywordFlags").is_some()
}

/// A tag table: keys in key order; in a `validated` mod `type` first (the old files' schema pass reordered those).
fn tag(c: &Ctx, v: &V, at: &str, validated: bool) -> R<Option<J>> {
    let Some(t) = v.as_table() else { return Ok(None) };
    let Some(V::Str(ty)) = c.d.t(t).get("type") else { return Ok(None) };
    let J::Obj(rest) = c.raw(v, at)? else { return Ok(None) };
    if !validated {
        return Ok(Some(J::Obj(rest)));
    }
    let mut o = vec![("type".to_owned(), J::str(ty))];
    o.extend(rest.into_iter().filter(|(k, _)| k != "type"));
    Ok(Some(J::Obj(o)))
}

/// `validated`: a mod the old pipeline's schema re-parsed (a top-level mod: fields in schema order, tags `type` first);
/// otherwise a mod nested in another mod's `value` (fields in construction order, tags as they are).
pub fn normalize_mod(c: &mut Ctx, v: &V, at: &str, validated: bool) -> R<J> {
    if !looks_like_mod(c, v) {
        return Err(format!("{at}: not a mod"));
    }
    let t = c.d.t(v.as_table().unwrap());
    let mut malformed = Vec::new();
    let mut max_index = Vec::new();
    for (k, _) in &t.entries {
        match k {
            V::Num(n) if n.fract() == 0.0 && *n >= 0.0 => max_index.push(*n as i64),
            V::Str(s) if NAMED.contains(&&**s) => {}
            k => {
                // A new mod field upstream: reported, the mod still converts (the port reads only the known ones).
                let k = key_text(k, at)?;
                c.drift("mods", "mod", format!("unknown mod field '{k}'"));
            }
        }
    }
    let ty = match t.get("type") {
        Some(V::Str(s)) if MOD_TYPES.contains(&&**s) => J::str(s),
        None => {
            malformed.push(("type".to_owned(), J::Null));
            J::Null
        }
        Some(v) => return Err(format!("{at}: unknown mod type {}", c.show(v))),
    };
    let mut tags = Vec::new();
    let mut contiguous = 0;
    while let Some(x) = t.index(contiguous + 1) {
        contiguous += 1;
        match tag(c, x, at, validated)? {
            Some(j) => tags.push(j),
            None => malformed.push((format!("tags[{contiguous}]"), c.raw(x, at)?)),
        }
    }
    let mut orphans = Vec::new();
    for i in max_index.into_iter().filter(|&i| i > contiguous) {
        let x = t.index(i).unwrap();
        match tag(c, x, at, validated)? {
            Some(j) => orphans.push(J::Obj(vec![("index".into(), J::Num(i as f64)), ("tag".into(), j)])),
            None => malformed.push((format!("tags[{i}]"), c.raw(x, at)?)),
        }
    }
    let mut num_or_0 = |field: &str| -> R<J> {
        match t.get(field).unwrap() {
            V::Num(n) => Ok(J::Num(*n)),
            v => {
                malformed.push((field.to_owned(), c.raw(v, at)?));
                Ok(J::Num(0.0))
            }
        }
    };
    let flags = num_or_0("flags")?;
    let kw = num_or_0("keywordFlags")?;

    let name = ("name".to_owned(), J::str(t.get("name").unwrap().as_str().unwrap()));
    let value = t.get("value").map(|x| normalize_value(c, x, &format!("{at}.value"))).transpose()?.map(|j| ("value".to_owned(), j));
    let source = t.get("source").and_then(V::as_str).map(|s| ("source".to_owned(), J::str(s)));
    let scale: Vec<(String, J)> =
        ["div", "mult"].iter().filter_map(|k| t.get(k).and_then(V::as_num).map(|n| (k.to_string(), J::Num(n)))).collect();
    let orphans = (!orphans.is_empty()).then(|| ("orphanTags".to_owned(), J::Arr(orphans)));
    let malformed = (!malformed.is_empty()).then(|| {
        let m = malformed.into_iter().map(|(f, v)| J::Obj(vec![("field".into(), J::Str(f)), ("value".into(), v)]));
        ("malformed".to_owned(), J::Arr(m.collect()))
    });
    let tags = ("tags".to_owned(), J::Arr(tags));
    let (flags, kw) = (("flags".to_owned(), flags), ("keywordFlags".to_owned(), kw));
    let mut o = vec![name, ("type".to_owned(), ty)];
    if validated {
        o.extend(value);
        o.extend([flags, kw]);
        o.extend(source);
        o.push(tags);
        o.extend(orphans);
    } else {
        o.extend([flags, kw, tags]);
        o.extend(orphans);
        o.extend(source);
        o.extend(value);
    }
    o.extend(scale);
    o.extend(malformed);
    Ok(J::Obj(o))
}

pub fn normalize_value(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    let Some(t) = v.as_table() else { return c.raw(v, at) };
    let t = c.d.t(t);
    if t.is_sequence() {
        return Ok(J::Arr(t.entries.iter().map(|(_, x)| normalize_value(c, x, at)).collect::<R<_>>()?));
    }
    if looks_like_mod(c, v) {
        return normalize_mod(c, v, at, false);
    }
    let mut o = Vec::new();
    for (k, x) in &t.entries {
        let k = key_text(k, at)?;
        let j = if k == "mod" && looks_like_mod(c, x) { normalize_mod(c, x, at, false)? } else { normalize_value(c, x, at)? };
        o.push((k, j));
    }
    Ok(J::Obj(o))
}

pub fn normalize_mod_list(c: &mut Ctx, v: &V, at: &str) -> R<J> {
    let items = c.seq(v, at)?;
    Ok(J::Arr(items.into_iter().map(|m| normalize_mod(c, m, at, true)).collect::<R<_>>()?))
}
