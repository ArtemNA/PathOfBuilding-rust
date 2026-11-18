//! Modules/ConfigOptions.lua (run once more after the boot, dump-extra.lua root `configOptions`) -> config-options.json.
//! Reader: crates/pob-data/src/config_options.rs.
//!
//! The file is a sequence of records: `{section, col}` separators, `{var, type, ...}` fields and `{label, ifSkill, ...}`
//! sub-headers (no var/type). Output: `{format: 1, sections: [...], vars: [...]}`, each var's keys in the old schema
//! order; `apply`/`tooltipFunc`/function `tooltip` become `{src, from, to, sha}` (sha256 of the source lines, LF
//! normalized); the free-form gate keys are copied verbatim into `gates` (byte order); a numeric key (an upstream record
//! that wraps `{ifFlag=,ifCond=}` in an extra `{...}`, ConfigTab never reads it) is kept under `malformed`.
//! A key outside STRUCTURAL + GATE_KEYS is dropped and reported (drift).

use std::collections::HashMap;

use crate::ctx::{Ctx, Obj, R, key_text};
use crate::dump::{Func, V};
use crate::js::J;

const FILE: &str = "config-options.json";
const TYPES: &[&str] = &["check", "count", "countAllowZero", "integer", "float", "list", "text"];
/// Gating/annotation keys copied into `gates` (the old allowlist, kept as is: some are read by ConfigTab only).
const GATE_KEYS: &[&str] = &[
    "ifCond", "ifCondTrue", "ifFlag", "ifSkill", "ifSkillList", "ifSkillFlag", "ifSkillData", "ifMult", "ifEnemyMult", "ifOption",
    "ifEnemyCond", "ifMinionCond", "ifMod", "ifNode", "ifTagType", "ifStat", "ifEnemyStat", "implyCond", "implyCondList",
    "implyEnemyCond", "implyMinionCond", "includeTransfigured", "hideIfInvalid", "doNotHighlight", "resizable", "skillName",
    "skillId", "skillType", "level", "summonSkill", "actor", "neg", "effectType", "applyToPlayer", "limitStat", "val", "value",
    "HOVER", "inactiveText", "legacy",
];
const STRUCTURAL: &[&str] = &[
    "var", "type", "label", "list", "defaultState", "defaultIndex", "defaultPlaceholderState", "tooltip", "tooltipFunc", "apply",
    "section", "col",
];

fn is_numeric_key(k: &str) -> bool {
    !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit())
}

struct Sources(HashMap<String, Vec<String>>);

impl Sources {
    /// `{src, from, to, sha}` of a Lua function defined in a real source file.
    fn closure(&mut self, c: &Ctx, v: &V, at: &str) -> R<J> {
        let Some(Func::Lua { src, from, to, .. }) = (match v {
            V::Func(f) => Some(c.d.f(*f)),
            _ => None,
        }) else {
            return Err(format!("{at}: expected a Lua function, got {}", c.show(v)));
        };
        if !self.0.contains_key(&**src) {
            let p = c.pob_root.join("src").join(&**src);
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{at}: {}: {e}", p.display()))?.replace("\r\n", "\n");
            self.0.insert(src.to_string(), text.split('\n').map(str::to_owned).collect());
        }
        let lines = &self.0[&**src];
        if *from < 1 || *to as usize > lines.len() || to < from {
            return Err(format!("{at}: closure range {src}:{from}-{to} outside the file ({} lines)", lines.len()));
        }
        let body = lines[*from as usize - 1..*to as usize].join("\n");
        let mut o = Obj::default();
        o.put("src", J::str(src));
        o.put("from", J::Num(*from as f64));
        o.put("to", J::Num(*to as f64));
        o.put("sha", J::Str(pob_data::sha256::sha256_hex(body.as_bytes())));
        Ok(o.done())
    }
}

fn num_or_str(c: &Ctx, v: &V, at: &str) -> R<J> {
    match v {
        V::Num(n) if n.is_finite() => Ok(J::Num(*n)),
        V::Str(s) => Ok(J::str(s)),
        v => Err(format!("{at}: expected a number or a string, got {}", c.show(v))),
    }
}

fn string(c: &Ctx, v: &V, at: &str) -> R<J> {
    c.string(v, at).map(J::Str)
}

fn number(c: &Ctx, v: &V, at: &str) -> R<J> {
    match v {
        V::Num(n) if n.is_finite() => Ok(J::Num(*n)),
        v => Err(format!("{at}: expected a number, got {}", c.show(v))),
    }
}

fn integer(c: &Ctx, v: &V, at: &str) -> R<J> {
    match v {
        V::Num(n) if n.fract() == 0.0 && n.abs() < 9007199254740992.0 => Ok(J::Num(*n)),
        v => Err(format!("{at}: expected an integer, got {}", c.show(v))),
    }
}

pub fn convert(c: &mut Ctx) -> R<J> {
    let root = c.d.root("configOptions").clone();
    let records = c.seq(&root, "configOptions")?;
    let mut src = Sources(HashMap::new());
    let mut sections = Vec::new();
    let mut vars = Vec::new();
    let mut current = String::new();
    let mut drift = Vec::new();

    for (i, rec) in records.into_iter().enumerate() {
        let at = format!("configOptions[{i}]");
        let t = c.d.t(c.table(rec, &at)?);
        if t.is_sequence() {
            return Err(format!("{at}: expected a record, got a sequence"));
        }
        // Keys in JS order (index keys first, ascending; the rest in byte order, as the dump has them).
        let mut malformed = Vec::new();
        for (k, v) in &t.entries {
            let ks = key_text(k, &at)?;
            if is_numeric_key(&ks) {
                malformed.push((ks, v));
            } else if !STRUCTURAL.contains(&ks.as_str()) && !GATE_KEYS.contains(&ks.as_str()) {
                drift.push(format!("unknown key '{ks}'"));
            }
        }
        let get = |k: &str| t.get(k);

        if let Some(s) = get("section") {
            for (k, _) in &t.entries {
                let ks = key_text(k, &at)?;
                if ks != "section" && ks != "col" && (STRUCTURAL.contains(&ks.as_str()) || GATE_KEYS.contains(&ks.as_str()) || is_numeric_key(&ks)) {
                    drift.push(format!("section record with field key '{ks}' (dropped)"));
                }
            }
            let section = c.string(s, &format!("{at}.section"))?;
            let col = number(c, get("col").ok_or_else(|| format!("{at}.col: missing"))?, &format!("{at}.col"))?;
            current = section.clone();
            let mut o = Obj::default();
            o.put("section", J::Str(section));
            o.put("col", col);
            o.put("index", J::Num(sections.len() as f64));
            sections.push(o.done());
            continue;
        }

        let mut o = Obj::default();
        if let Some(v) = get("var") {
            o.put("var", string(c, v, &format!("{at}.var"))?);
        }
        if let Some(v) = get("type") {
            match v.as_str() {
                Some(s) if TYPES.contains(&s) => o.put("type", J::str(s)),
                _ => return Err(format!("{at}.type: unexpected type {}", c.show(v))),
            }
        }
        if let Some(v) = get("label") {
            o.put("label", string(c, v, &format!("{at}.label"))?);
        }
        o.put("section", J::Str(current.clone()));
        o.put("index", J::Num(vars.len() as f64));
        if let Some(v) = get("list") {
            let la = format!("{at}.list");
            let items = c.seq(v, &la)?;
            if items.is_empty() {
                return Err(format!("{la}: expected a non-empty array"));
            }
            let mut list = Vec::with_capacity(items.len());
            for (j, item) in items.into_iter().enumerate() {
                let ia = format!("{la}[{j}]");
                let it = c.d.t(c.table(item, &ia)?);
                if it.is_sequence() {
                    return Err(format!("{ia}: expected a record, got a sequence"));
                }
                for (k, _) in &it.entries {
                    let ks = key_text(k, &ia)?;
                    if ks != "val" && ks != "label" {
                        drift.push(format!("list entry: unknown key '{ks}'"));
                    }
                }
                let val = num_or_str(c, it.get("val").ok_or_else(|| format!("{ia}.val: missing"))?, &format!("{ia}.val"))?;
                let label = string(c, it.get("label").ok_or_else(|| format!("{ia}.label: missing"))?, &format!("{ia}.label"))?;
                let mut e = Obj::default();
                e.put("val", val);
                e.put("label", label);
                list.push(e.done());
            }
            o.put("list", J::Arr(list));
        }
        if let Some(v) = get("defaultState") {
            let j = match v {
                V::Bool(b) => J::Bool(*b),
                v => num_or_str(c, v, &format!("{at}.defaultState")).map_err(|e| e.replace("a number or a string", "number|string|boolean"))?,
            };
            o.put("defaultState", j);
        }
        if let Some(v) = get("defaultIndex") {
            o.put("defaultIndex", integer(c, v, &format!("{at}.defaultIndex"))?);
        }
        if let Some(v) = get("defaultPlaceholderState") {
            o.put("defaultPlaceholderState", num_or_str(c, v, &format!("{at}.defaultPlaceholderState"))?);
        }
        if let Some(v) = get("tooltip") {
            let j = match v {
                V::Str(s) => J::str(s),
                v => src.closure(c, v, &format!("{at}.tooltip"))?,
            };
            o.put("tooltip", j);
        }
        if let Some(v) = get("tooltipFunc") {
            o.put("tooltipFunc", src.closure(c, v, &format!("{at}.tooltipFunc"))?);
        }
        if let Some(v) = get("apply") {
            o.put("apply", src.closure(c, v, &format!("{at}.apply"))?);
        }
        let mut gates: Vec<(String, J)> = Vec::new();
        for k in GATE_KEYS {
            if let Some(v) = get(k) {
                gates.push((k.to_string(), c.raw(v, &format!("{at}.{k}"))?));
            }
        }
        gates.sort_by(|a, b| a.0.cmp(&b.0));
        o.put("gates", J::Obj(gates));
        if !malformed.is_empty() {
            let mut m = Vec::new();
            for (k, v) in malformed {
                let mut e = Obj::default();
                let j = c.raw(v, &format!("{at}[{k}]"))?;
                e.put("field", J::Str(k));
                e.put("value", j);
                m.push(e.done());
            }
            c.count(FILE, &format!("malformed record (numeric key) at {at}"), m.len());
            o.put("malformed", J::Arr(m));
        }
        vars.push(o.done());
    }
    for d in drift {
        c.drift(FILE, "record", d);
    }
    c.count(FILE, "sections", sections.len());
    c.count(FILE, "vars", vars.len());
    let mut top = Obj::default();
    top.put("format", J::Num(1.0));
    top.put("sections", J::Arr(sections));
    top.put("vars", J::Arr(vars));
    Ok(top.done())
}
