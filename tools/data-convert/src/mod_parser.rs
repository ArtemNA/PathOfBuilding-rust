//! The tables Modules/ModParser.lua builds at load time -> mod-parser.json. Reader: crates/pob-data/src/mod_parser.rs.
//!
//! Finding them does not depend on the shape of the code around them: every Lua function reachable from the post-boot
//! `modLib` through upvalue chains is visited, and each table captured as an upvalue is indexed by its upvalue name (the
//! `local` name in ModParser.lua). A wanted name that is missing or bound to two different tables is fatal; everything
//! else is reported, so a new table shows up in the report rather than silently.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::ctx::{Ctx, R, key_text};
use crate::dump::{Func, V};
use crate::js::J;
use crate::mods::{looks_like_mod, normalize_mod};

const FILE: &str = "mod-parser.json";

/// The tables pob-rust's parser port reads, by their ModParser.lua local name.
pub const TABLES: &[&str] = &[
    "formList", "modNameList", "modFlagList", "preFlagList", "modTagList", "specialModList", "jewelFuncList", "clusterJewelSkills",
    "unsupportedModList", "preSkillNameList", "skillNameList", "suffixTypes", "penTypes", "flagTypes", "dmgTypes", "regenTypes",
    "degenTypes", "costTypes", "baseCostTypes",
];
/// Helper tables many closures capture: written once under `aux` and referenced as `{named}`.
const AUX: &[&str] = &["conquerorList", "gemIdLookup"];

struct St {
    named: HashMap<u32, &'static str>,
    aux: BTreeMap<&'static str, u32>,
    fns: BTreeMap<String, J>,
    lines: HashMap<String, Vec<String>>,
}

/// Every table-valued upvalue reachable from `root` through functions: name -> distinct tables.
fn upvalue_tables(c: &Ctx, root: &V) -> R<BTreeMap<String, Vec<u32>>> {
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    for (_, v) in &c.d.t(c.table(root, "modLib")?).entries {
        if let V::Func(f) = v {
            queue.push_back(*f);
        }
    }
    let mut out: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    while let Some(f) = queue.pop_front() {
        if !seen.insert(f) {
            continue;
        }
        if let Func::Lua { upvalues, .. } = c.d.f(f) {
            for (name, v) in upvalues {
                match v {
                    V::Func(g) => queue.push_back(*g),
                    V::Table(t) => {
                        let e = out.entry(name.to_string()).or_default();
                        if !e.contains(t) {
                            e.push(*t);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(out)
}

fn fn_key(c: &Ctx, st: &mut St, f: u32) -> R<Option<String>> {
    let Func::Lua { src, from, to, .. } = c.d.f(f) else { return Ok(None) };
    // Only functions defined in a real source file get a body hash; anything else counts as native.
    if src.len() >= 200 || src.contains('\n') || !src.ends_with(".lua") {
        return Ok(None);
    }
    let key = format!("{src}:{from}");
    if !st.fns.contains_key(&key) {
        if !st.lines.contains_key(&**src) {
            let p = c.pob_root.join("src").join(&**src);
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?.replace("\r\n", "\n");
            st.lines.insert(src.to_string(), text.split('\n').map(str::to_owned).collect());
        }
        let lines = &st.lines[&**src];
        if *from < 1 || *to as usize > lines.len() || to < from {
            return Err(format!("{key}: range outside the file"));
        }
        let body = lines[*from as usize - 1..*to as usize].join("\n");
        let sha = pob_data::sha256::sha256_hex(body.as_bytes());
        let entry = J::Obj(vec![
            ("src".into(), J::str(src)),
            ("from".into(), J::Num(*from as f64)),
            ("to".into(), J::Num(*to as f64)),
            ("sha".into(), J::Str(sha)),
        ]);
        st.fns.insert(key.clone(), entry);
    }
    Ok(Some(key))
}

/// A function value inside one of the tables: `{fn, up}` with every upvalue (named tables as `{named}`).
fn closure(c: &mut Ctx, st: &mut St, f: u32, at: &str) -> R<J> {
    let key = fn_key(c, st, f)?.ok_or_else(|| format!("{at}: a table-value function is native"))?;
    let Func::Lua { upvalues, .. } = c.d.f(f) else { unreachable!() };
    let mut up: Vec<(String, J)> = Vec::new();
    for (name, v) in upvalues {
        if &**name == "_ENV" || *v == V::Nil {
            continue;
        }
        let j = match v {
            V::Table(t) if st.named.contains_key(t) => J::Obj(vec![("named".into(), J::str(st.named[t]))]),
            V::Table(t) if AUX.contains(&&**name) => {
                let n = AUX.iter().find(|a| **a == &**name).unwrap();
                if let Some(prev) = st.aux.insert(n, *t)
                    && prev != *t
                {
                    return Err(format!("{at}: two different tables behind upvalue '{name}'"));
                }
                J::Obj(vec![("named".into(), J::str(n))])
            }
            v => value(c, st, v, false, &format!("{at}.up.{name}"))?,
        };
        up.push((name.to_string(), j));
    }
    up.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(J::Obj(vec![("fn".into(), J::Str(key)), ("up".into(), J::Obj(up))]))
}

/// `wrapped`: inside the tables themselves (functions become `{fn, up}`); otherwise inside an upvalue (`{fn}` / `{cfn}`).
fn value(c: &mut Ctx, st: &mut St, v: &V, wrapped: bool, at: &str) -> R<J> {
    match v {
        V::Num(n) if !n.is_finite() => Err(format!("{at}: non-finite number")),
        V::Func(f) if wrapped => closure(c, st, *f, at),
        V::Func(f) => Ok(match fn_key(c, st, *f)? {
            Some(k) => J::Obj(vec![("fn".into(), J::Str(k))]),
            None => J::Obj(vec![("cfn".into(), J::Bool(true))]),
        }),
        V::Table(t) => {
            let t = c.d.t(*t);
            if t.is_sequence() {
                let items: Vec<&V> = t.entries.iter().map(|(_, x)| x).collect();
                if !items.is_empty() && items.iter().all(|x| looks_like_mod(c, x)) {
                    return Ok(J::Arr(items.into_iter().map(|x| normalize_mod(c, x, at, true)).collect::<R<_>>()?));
                }
                return Ok(J::Arr(items.into_iter().map(|x| value(c, st, x, wrapped, at)).collect::<R<_>>()?));
            }
            if looks_like_mod(c, v) {
                return normalize_mod(c, v, at, true);
            }
            // A table with an array part 1..n AND named fields: the array part becomes `list`.
            let mut entries: Vec<(String, J)> = Vec::new();
            let numeric: Vec<i64> = t.entries.iter().filter_map(|(k, _)| integral_key(k)).collect();
            let contiguous = !numeric.is_empty() && numeric.iter().enumerate().all(|(i, n)| *n == i as i64 + 1);
            if contiguous {
                let list = t.entries.iter().filter(|(k, _)| integral_key(k).is_some()).map(|(_, x)| value(c, st, x, wrapped, at));
                entries.push(("list".into(), J::Arr(list.collect::<R<_>>()?)));
            }
            for (k, x) in &t.entries {
                if contiguous && integral_key(k).is_some() {
                    continue;
                }
                entries.push((key_text(k, at)?, value(c, st, x, wrapped, at)?));
            }
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(J::Obj(entries))
        }
        v => c.raw(v, at),
    }
}

fn integral_key(k: &V) -> Option<i64> {
    match k {
        V::Num(n) if n.fract() == 0.0 && *n >= 0.0 => Some(*n as i64),
        _ => None,
    }
}

fn pattern_table(c: &mut Ctx, st: &mut St, t: u32, at: &str) -> R<J> {
    let mut out = Vec::new();
    for (k, v) in &c.d.t(t).entries {
        let k = key_text(k, at)?;
        let j = value(c, st, v, true, &format!("{at}[{k}]"))?;
        out.push((k, j));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(J::Obj(out))
}

pub fn convert(c: &mut Ctx) -> R<J> {
    let modlib = c.d.path("after", &["modLib"]).ok_or("after.modLib missing")?.clone();
    let found = upvalue_tables(c, &modlib)?;
    let mut st = St { named: HashMap::new(), aux: BTreeMap::new(), fns: BTreeMap::new(), lines: HashMap::new() };
    let mut ids = Vec::new();
    for name in TABLES {
        match found.get(*name).map(Vec::as_slice) {
            Some([t]) => {
                st.named.insert(*t, name);
                ids.push((*name, *t));
            }
            Some(many) => return Err(format!("{FILE}: upvalue '{name}' is bound to {} different tables", many.len())),
            None => return Err(format!("{FILE}: no function reachable from modLib captures a table named '{name}'")),
        }
    }
    let reachable = found.keys().filter(|k| !TABLES.contains(&k.as_str())).cloned().collect::<Vec<_>>();
    c.count(FILE, "other table upvalues reachable from modLib (not read)", reachable.len());
    for (name, t) in &ids {
        c.count(FILE, &format!("tables.{name}"), c.d.t(*t).entries.len());
    }

    let mut tables = Vec::new();
    for (name, t) in ids {
        tables.push((name.to_owned(), pattern_table(c, &mut st, t, &format!("tables.{name}"))?));
    }
    tables.sort_by(|a, b| a.0.cmp(&b.0));
    let mut aux = Vec::new();
    for name in AUX {
        let t = *st.aux.get(name).ok_or_else(|| format!("{FILE}: no closure captures aux table '{name}'"))?;
        aux.push((name.to_string(), pattern_table(c, &mut st, t, &format!("aux.{name}"))?));
    }
    let fns = std::mem::take(&mut st.fns).into_iter().collect::<Vec<_>>();
    c.count(FILE, "fns", fns.len());
    Ok(J::Obj(vec![("aux".into(), J::Obj(aux)), ("fns".into(), J::Obj(fns)), ("tables".into(), J::Obj(tables))]))
}
