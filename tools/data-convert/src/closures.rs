//! Inventory of every Lua function reachable from `data` (MINEFIELD D1) -> closures.json:
//! `{ closures: [{path, src, from, to, sha256}], uniqueSources, byHook }`.
//!
//! The paths are those of the old wire snapshot: one walk over `data` in the wire's category order with one `seen` set, so
//! a shared table is walked only at its first ("canonical") place and every later occurrence is a `$ref` (not walked).
//! Category order (`KEY_ORDER`) and the table-key (`$tk`) order by the key's canonical path decide which place that is.
//! `sha256` hashes the function's source lines from..to of the checkout (LF-normalized), so a changed body shows up.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use crate::ctx::{Ctx, Obj, R, key_text};
use crate::dump::{Func, V};
use crate::js::J;

const FILE: &str = "closures.json";

/// Root categories walked first, then the rest in key order (the wire's `KEY_ORDER[""]`).
const ROOT_ORDER: &[&str] = &[
    "skills", "gems", "itemBases", "itemMods", "veiledMods", "masterMods", "enchantments", "essences", "uniques", "minions",
    "bosses", "bossSkills", "clusterJewels", "mapMods", "costs", "pantheons", "rares",
];
/// Keys of `data.itemMods` walked first (the merged `Item` last: its entries are the shared ones).
const ITEM_MODS_ORDER: &[&str] = &[
    "Explicit", "ItemExclusive", "Corrupted", "Delve", "Synthesis", "Scourge", "Eldritch", "Mercenary", "Flask", "Tincture",
    "Graft", "Jewel", "JewelAbyss", "JewelCluster", "JewelCharm", "Foulborn", "WatchersEye", "Vestigial", "Item",
];

thread_local! {
    static SOURCES: RefCell<HashMap<PathBuf, std::rc::Rc<Vec<String>>>> = RefCell::new(HashMap::new());
}

/// Source lines `from..=to` of `<pobRoot>/src/<src>`, joined with LF (CRLF normalized).
pub fn source(pob_root: &Path, src: &str, from: u32, to: u32) -> R<String> {
    let p = pob_root.join("src").join(src);
    let lines = SOURCES.with(|m| -> R<_> {
        if let Some(l) = m.borrow().get(&p) {
            return Ok(l.clone());
        }
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let l = std::rc::Rc::new(text.replace("\r\n", "\n").split('\n').map(str::to_owned).collect::<Vec<_>>());
        m.borrow_mut().insert(p.clone(), l.clone());
        Ok(l)
    })?;
    if from < 1 || to as usize > lines.len() || to < from {
        return Err(format!("closure range {src}:{from}-{to} outside file ({} lines)", lines.len()));
    }
    Ok(lines[from as usize - 1..to as usize].join("\n"))
}

/// JavaScript string order (UTF-16 code units).
pub fn js_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// The wire serializer's JSON text of a string (`[%c"\\]` escaped).
fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => o.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => o.push(ch),
        }
    }
    o.push('"');
    o
}

struct Rec {
    path: String,
    src: String,
    from: u32,
    to: u32,
}

struct Walk<'c, 'a> {
    c: &'c Ctx<'a>,
    /// table -> its canonical wire path text without the closing bracket, e.g. `["skills","Arc"`.
    seen: HashMap<u32, String>,
    out: Vec<Rec>,
}

impl Walk<'_, '_> {
    fn value(&mut self, v: &V, prefix: String, dot: String, order: Option<&[&str]>) -> R<()> {
        match v {
            V::Table(t) => self.table(*t, prefix, dot, order),
            V::Func(f) => {
                if let Func::Lua { src, from, to, .. } = self.c.d.f(*f) {
                    self.out.push(Rec { path: dot, src: src.to_string(), from: *from, to: *to });
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn table(&mut self, id: u32, prefix: String, dot: String, order: Option<&[&str]>) -> R<()> {
        if self.seen.contains_key(&id) {
            return Ok(()); // a `$ref`: walked at its canonical place
        }
        self.seen.insert(id, prefix.clone());
        let t = self.c.d.t(id);
        if t.is_sequence() {
            for (i, (_, x)) in t.entries.iter().enumerate() {
                self.value(x, format!("{prefix},{i}"), format!("{dot}[{i}]"), None)?;
            }
            return Ok(());
        }
        let mut plain: Vec<(&V, &V)> = Vec::new();
        let mut tkeys: Vec<(&V, &V)> = Vec::new();
        for (k, x) in &t.entries {
            match k {
                V::Table(_) => tkeys.push((k, x)),
                V::Num(_) | V::Str(_) | V::Bool(_) => plain.push((k, x)),
                k => return Err(format!("{dot}: unsupported key type {}", self.c.show(k))),
            }
        }
        if let Some(order) = order {
            let rank = |k: &V| k.as_str().and_then(|s| order.iter().position(|o| *o == s));
            let (mut head, tail): (Vec<_>, Vec<_>) = plain.into_iter().partition(|(k, _)| rank(k).is_some());
            head.sort_by_key(|(k, _)| rank(k));
            head.extend(tail);
            plain = head;
        }
        for (k, x) in plain {
            let ks = key_text(k, &dot)?;
            self.value(x, format!("{prefix},{}", json_str(&ks)), format!("{dot}.{ks}"), None)?;
        }
        if !tkeys.is_empty() {
            let mut e = tkeys
                .into_iter()
                .map(|(k, x)| {
                    let p = self.seen.get(&k.as_table().unwrap()).cloned();
                    p.map(|p| (p, x)).ok_or_else(|| format!("{dot}: table-typed key that was not walked earlier (D4)"))
                })
                .collect::<R<Vec<_>>>()?;
            e.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            for (i, (_, x)) in e.into_iter().enumerate() {
                self.value(x, format!("{prefix},{{\"$tk\":{i}}}"), format!("{dot}.$tk[{i}]"), None)?;
            }
        }
        Ok(())
    }
}

pub fn convert(c: &mut Ctx) -> R<J> {
    let data = c.d.root("data");
    let root = c.d.t(c.table(data, "data")?);
    let mut keys: Vec<&str> = Vec::new();
    for (k, _) in &root.entries {
        keys.push(k.as_str().ok_or_else(|| format!("data: non-string root key {}", c.show(k)))?);
    }
    let rank = |k: &str| ROOT_ORDER.iter().position(|o| *o == k);
    let (mut head, tail): (Vec<&str>, Vec<&str>) = keys.into_iter().partition(|k| rank(k).is_some());
    head.sort_by_key(|k| rank(k));
    head.extend(tail);

    let mut w = Walk { c, seen: HashMap::new(), out: Vec::new() };
    for k in head {
        let v = root.get(k).unwrap();
        let order = (k == "itemMods").then_some(ITEM_MODS_ORDER);
        w.value(v, format!("[{}", json_str(k)), k.to_owned(), order)?;
    }
    let mut recs = w.out;
    recs.sort_by(|a, b| js_cmp(&a.path, &b.path));

    let mut unique = BTreeSet::new();
    let mut hooks: HashMap<String, usize> = HashMap::new();
    let mut list = Vec::with_capacity(recs.len());
    for r in &recs {
        let body = source(&c.pob_root, &r.src, r.from, r.to)?;
        unique.insert((r.src.clone(), r.from, r.to));
        // `path.replace(/\[\d+\]$/, '').split('.').pop()`
        let p = r.path.as_str();
        let p = match p.rfind('[') {
            Some(i) if p.ends_with(']') && i + 2 < p.len() && p[i + 1..p.len() - 1].bytes().all(|b| b.is_ascii_digit()) => &p[..i],
            _ => p,
        };
        let hook = p.rsplit('.').next().unwrap_or(p);
        *hooks.entry(hook.to_owned()).or_default() += 1;
        let mut o = Obj::default();
        o.put("path", J::str(&r.path));
        o.put("src", J::str(&r.src));
        o.put("from", J::Num(r.from as f64));
        o.put("to", J::Num(r.to as f64));
        o.put("sha256", J::Str(sha256_hex(body.as_bytes())));
        list.push(o.done());
    }
    let mut by_hook: Vec<(String, usize)> = hooks.into_iter().collect();
    by_hook.sort_by(|a, b| js_cmp(&a.0, &b.0));
    c.count(FILE, "closures", list.len());
    c.count(FILE, "uniqueSources", unique.len());

    let mut o = Obj::default();
    o.put("closures", J::Arr(list));
    o.put("uniqueSources", J::Num(unique.len() as f64));
    o.put("byHook", J::Obj(by_hook.into_iter().map(|(k, n)| (k, J::Num(n as f64))).collect()));
    Ok(o.done())
}

/// SHA-256 (FIPS 180-4), lowercase hex.
pub fn sha256_hex(msg: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut m = msg.to_vec();
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&((msg.len() as u64) * 8).to_be_bytes());
    for block in m.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn sha256() {
        assert_eq!(super::sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(super::sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
