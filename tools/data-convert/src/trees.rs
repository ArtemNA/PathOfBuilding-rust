//! Passive trees: TreeData/<ver>/tree.lua (every `treeVersionList` entry) + TreeData/legion/tree-legion.lua ->
//! data/tree/<ver>.json (3_6, 3_12, 3_13 and the latest) or data/tree/_ondemand/<ver>.json, and data/tree/legion.json.
//! Reader: crates/pob-data/src/tree.rs.
//!
//! A tree is exported as is (no ProcessNode), but the old pipeline passed it through a strict zod schema, which decided
//! the bytes: every object's keys come out in SCHEMA order, and a union takes its first matching arm (modern vs legacy
//! node and class shapes). That schema is reproduced below as data (`S`) and is the schema of these files: a key it does
//! not name is dropped and reported (drift); a named key with another type, or a value no union arm accepts, is fatal.
//!
//! Lua -> JSON is the wire rule every old file was built on: a sequence 1..n (or an empty table) is an array, any other
//! table an object, so `z.object`/`z.record` never accept an empty table and `z.array` never accepts a gapped one.

use crate::ctx::{Ctx, R, key_text};
use crate::dump::V;
use crate::js::J;

/// The versions with a committed file under data/tree/ (golden builds); the latest is committed too.
const COMMITTED: &[&str] = &["3_6", "3_12", "3_13"];

pub enum S {
    Num,
    Str,
    Bool,
    Arr(Box<S>),
    /// `z.record(z.string(), _)`: keys in input order (the JS writer puts index keys first).
    Rec(Box<S>),
    /// `z.object(..).strict()`: (key, schema, optional), output in this order. The name groups the drift report.
    Obj(&'static str, Vec<(&'static str, S, bool)>),
    /// `z.union`: the first arm that accepts the value.
    Union(Vec<S>),
    /// Purely graphical subtrees (sprite atlases): `z.record(z.string(), z.unknown())`, copied as is.
    Opaque,
}

use S::*;

fn arr(s: S) -> S {
    Arr(Box::new(s))
}
fn rec(s: S) -> S {
    Rec(Box::new(s))
}
fn req(k: &'static str, s: S) -> (&'static str, S, bool) {
    (k, s, false)
}
fn opt(k: &'static str, s: S) -> (&'static str, S, bool) {
    (k, s, true)
}
fn num_list_or_map() -> S {
    Union(vec![arr(Num), rec(Num)])
}

fn modern_ascendancy() -> S {
    Obj(
        "ascendancy",
        vec![
            req("id", Str),
            req("name", Str),
            opt("flavourText", Str),
            opt("flavourTextColour", Str),
            opt("flavourTextRect", Obj("flavourTextRect", vec![req("x", Num), req("y", Num), req("width", Num), req("height", Num)])),
        ],
    )
}

fn legacy_node() -> S {
    Obj(
        "legacy node",
        vec![
            req("da", Num),
            req("g", Num),
            req("ia", Num),
            req("o", Num),
            req("oidx", Num),
            req("sa", Num),
            req("out", num_list_or_map()),
            opt("in", num_list_or_map()),
            opt("id", Num),
            opt("dn", Str),
            opt("icon", Str),
            opt("ks", Bool),
            opt("m", Bool),
            opt("not", Bool),
            opt("sd", arr(Str)),
            opt("spc", num_list_or_map()),
            opt("passivePointsGranted", Num),
            opt("isJewelSocket", Bool),
            opt("isMultipleChoice", Bool),
            opt("isMultipleChoiceOption", Bool),
            opt("isAscendancyStart", Bool),
            opt("isBlighted", Bool),
            opt("ascendancyName", Str),
            opt("flavourText", arr(Str)),
            opt("reminderText", arr(Str)),
        ],
    )
}

fn modern_node() -> S {
    let strs = || arr(Str);
    Obj(
        "node",
        vec![
            opt("group", Num),
            opt("orbit", Num),
            opt("orbitIndex", Num),
            opt("in", strs()),
            opt("out", strs()),
            opt("skill", Num),
            opt("name", Str),
            opt("icon", Str),
            opt("stats", strs()),
            opt("isNotable", Bool),
            opt("isKeystone", Bool),
            opt("isMastery", Bool),
            opt("isJewelSocket", Bool),
            opt("isMultipleChoice", Bool),
            opt("isMultipleChoiceOption", Bool),
            opt("isAscendancyStart", Bool),
            opt("isBlighted", Bool),
            opt("isBloodline", Bool),
            opt("isProxy", Bool),
            opt("proxyKey", Str),
            opt("ascendancyName", Str),
            opt("classStartIndex", Num),
            opt("grantedStrength", Num),
            opt("grantedDexterity", Num),
            opt("grantedIntelligence", Num),
            opt("grantedPassivePoints", Num),
            opt("reminderText", strs()),
            opt("flavourText", strs()),
            opt("recipe", strs()),
            opt(
                "expansionJewel",
                Obj("expansionJewel", vec![req("index", Num), opt("parent", Str), req("proxy", Str), req("size", Num)]),
            ),
            opt("masteryEffects", arr(Obj("masteryEffect", vec![req("effect", Num), req("stats", strs()), opt("reminderText", strs())]))),
            opt("activeIcon", Str),
            opt("inactiveIcon", Str),
            opt("activeEffectImage", Str),
        ],
    )
}

pub fn tree_schema() -> S {
    let modern_class = Obj(
        "class",
        vec![req("name", Str), req("base_str", Num), req("base_dex", Num), req("base_int", Num), req("ascendancies", arr(modern_ascendancy()))],
    );
    let legacy_class = Obj(
        "legacy class",
        vec![
            req("name", Str),
            req(
                "classes",
                arr(Obj(
                    "legacy ascendancy",
                    vec![req("name", Str), req("displayName", Str), req("flavourText", Str), req("flavourTextColour", Str), req("flavourTextRect", Str)],
                )),
            ),
        ],
    );
    let modern_group = || Obj(
        "group",
        vec![
            req("x", Num),
            req("y", Num),
            req("orbits", arr(Num)),
            req("nodes", arr(Union(vec![Str, Num]))),
            opt("isProxy", Bool),
            opt("background", Obj("background", vec![req("image", Str), opt("isHalfImage", Bool)])),
        ],
    );
    let legacy_group = || Obj(
        "legacy group",
        vec![req("x", Num), req("y", Num), req("n", num_list_or_map()), req("oo", Union(vec![arr(Bool), rec(Bool)]))],
    );
    let group = || Union(vec![modern_group(), legacy_group()]);
    Obj(
        "tree",
        vec![
            opt("tree", Str),
            opt("ruthless", Bool),
            req("classes", Union(vec![arr(modern_class), rec(legacy_class)])),
            opt("alternate_ascendancies", arr(modern_ascendancy())),
            req("groups", Union(vec![rec(group()), arr(group())])),
            req("nodes", Union(vec![rec(Union(vec![modern_node(), legacy_node()])), arr(legacy_node())])),
            opt("root", legacy_node()),
            opt("jewelSlots", arr(Num)),
            req("min_x", Num),
            req("min_y", Num),
            req("max_x", Num),
            req("max_y", Num),
            req(
                "constants",
                Obj(
                    "constants",
                    vec![
                        req("PSSCentreInnerRadius", Num),
                        req(
                            "characterAttributes",
                            Obj("characterAttributes", vec![req("Strength", Num), req("Dexterity", Num), req("Intelligence", Num)]),
                        ),
                        req("classes", rec(Num)),
                        opt("orbitRadii", arr(Num)),
                        opt("skillsPerOrbit", arr(Num)),
                    ],
                ),
            ),
            opt("points", Obj("points", vec![req("ascendancyPoints", Num), req("totalPoints", Num)])),
            opt("assets", Opaque),
            opt("skillSprites", Opaque),
            opt("sprites", Opaque),
            opt("extraImages", arr(Obj("extraImage", vec![req("image", Str), req("x", Num), req("y", Num)]))),
            opt("imageZoomLevels", arr(Num)),
            opt("imageRoot", Str),
            opt(
                "characterData",
                rec(Obj("characterData", vec![req("base_str", Num), req("base_dex", Num), req("base_int", Num)])),
            ),
        ],
    )
}

pub fn legion_schema() -> S {
    let sheet = || arr(Obj("sprite sheet", vec![opt("filename", Str), req("coords", Opaque)]));
    Obj(
        "legion",
        vec![
            req("keystoneActive", sheet()),
            req("keystoneInactive", sheet()),
            req("normalActive", sheet()),
            req("normalInactive", sheet()),
            req("notableActive", sheet()),
            req("notableInactive", sheet()),
            req("treeAssets", sheet()),
        ],
    )
}

/// Wire shape of a Lua value: table -> array (sequence 1..n, empty included) or object.
fn is_array(c: &Ctx, v: &V) -> Option<bool> {
    v.as_table().map(|t| c.d.t(t).is_sequence())
}

/// Does `v` satisfy `s`? `strict`: an object with a key the schema does not name fails (zod `.strict()`); otherwise such
/// keys are tolerated (they become drift). Opaque never contains functions (no wire markers in a tree).
fn accepts(c: &Ctx, v: &V, s: &S, strict: bool) -> bool {
    match s {
        Num => v.as_num().is_some_and(f64::is_finite),
        Str => v.as_str().is_some(),
        Bool => matches!(v, V::Bool(_)),
        Arr(e) => is_array(c, v) == Some(true) && c.d.t(v.as_table().unwrap()).entries.iter().all(|(_, x)| accepts(c, x, e, strict)),
        Rec(e) => {
            is_array(c, v) == Some(false)
                && c.d.t(v.as_table().unwrap()).entries.iter().all(|(k, x)| key_text(k, "").is_ok() && accepts(c, x, e, strict))
        }
        Obj(_, fields) => {
            if is_array(c, v) != Some(false) {
                return false;
            }
            let t = c.d.t(v.as_table().unwrap());
            for (k, s, optional) in fields {
                match t.get(k) {
                    Some(x) if accepts(c, x, s, strict) => {}
                    Some(_) => return false,
                    None if *optional => {}
                    None => return false,
                }
            }
            !strict || t.entries.iter().all(|(k, _)| k.as_str().is_some_and(|k| fields.iter().any(|f| f.0 == k)))
        }
        Union(arms) => arms.iter().any(|a| accepts(c, v, a, strict)),
        Opaque => is_array(c, v) == Some(false) && plain(c, v),
    }
}

/// No functions / userdata anywhere below (the old pipeline's "no wire markers" check).
fn plain(c: &Ctx, v: &V) -> bool {
    match v {
        V::Table(t) => c.d.t(*t).entries.iter().all(|(_, x)| plain(c, x)),
        V::Func(_) | V::Opaque(_) => false,
        _ => true,
    }
}

pub fn convert_value(c: &mut Ctx, file: &str, v: &V, s: &S, at: &str) -> R<J> {
    match s {
        Num if accepts(c, v, s, true) => Ok(J::Num(v.as_num().unwrap())),
        Str if accepts(c, v, s, true) => Ok(J::str(v.as_str().unwrap())),
        Bool if accepts(c, v, s, true) => Ok(J::Bool(matches!(v, V::Bool(true)))),
        Num | Str | Bool => Err(format!("{at}: expected {}, got {}", kind(s), c.show(v))),
        Arr(e) => {
            if is_array(c, v) != Some(true) {
                return Err(format!("{at}: expected an array (Lua sequence 1..n), got {}", c.show(v)));
            }
            let items: Vec<&V> = c.d.t(v.as_table().unwrap()).entries.iter().map(|(_, x)| x).collect();
            let mut out = Vec::with_capacity(items.len());
            for (i, x) in items.into_iter().enumerate() {
                out.push(convert_value(c, file, x, e, &format!("{at}[{i}]"))?);
            }
            Ok(J::Arr(out))
        }
        Rec(e) => {
            if is_array(c, v) != Some(false) {
                return Err(format!("{at}: expected an object (non-sequence table), got {}", c.show(v)));
            }
            let entries: Vec<(&V, &V)> = c.d.t(v.as_table().unwrap()).entries.iter().map(|(k, x)| (k, x)).collect();
            let mut out = Vec::with_capacity(entries.len());
            for (k, x) in entries {
                let k = key_text(k, at)?;
                let j = convert_value(c, file, x, e, &format!("{at}.{k}"))?;
                out.push((k, j));
            }
            Ok(J::Obj(out))
        }
        Obj(name, fields) => {
            if is_array(c, v) != Some(false) {
                return Err(format!("{at}: expected an object (non-sequence table), got {}", c.show(v)));
            }
            let t = c.d.t(v.as_table().unwrap());
            let mut out = Vec::with_capacity(fields.len());
            for (k, fs, optional) in fields {
                match t.get(k) {
                    Some(x) => out.push((k.to_string(), convert_value(c, file, x, fs, &format!("{at}.{k}"))?)),
                    None if *optional => {}
                    None => return Err(format!("{at}.{k}: missing")),
                }
            }
            for (k, x) in &t.entries {
                if k.as_str().is_some_and(|k| fields.iter().any(|f| f.0 == k)) {
                    continue;
                }
                let what = format!("unknown field '{}' ({})", key_text(k, at).unwrap_or_else(|_| format!("{k:?}")), c.show(x).split('(').next().unwrap_or(""));
                c.drift(file, name, what);
            }
            Ok(J::Obj(out))
        }
        Union(arms) => {
            // zod: the first arm that accepts the value as is; with drift tolerated, the first arm that accepts it apart
            // from unknown keys.
            let arm = arms.iter().find(|a| accepts(c, v, a, true)).or_else(|| arms.iter().find(|a| accepts(c, v, a, false)));
            match arm {
                Some(a) => convert_value(c, file, v, a, at),
                None => {
                    let why: Vec<String> =
                        arms.iter().map(|a| convert_value(c, file, v, a, at).err().unwrap_or_else(|| "?".into())).collect();
                    Err(format!("{at}: no shape of the union accepts {}: {}", c.show(v), why.join(" | ")))
                }
            }
        }
        Opaque => {
            if is_array(c, v) != Some(false) {
                return Err(format!("{at}: expected an object (non-sequence table), got {}", c.show(v)));
            }
            c.raw(v, at)
        }
    }
}

fn kind(s: &S) -> &'static str {
    match s {
        Num => "a finite number",
        Str => "a string",
        Bool => "a boolean",
        _ => "a table",
    }
}

/// Output path (relative to data/) of a tree version.
pub fn out_file(ver: &str, latest: &str) -> String {
    if COMMITTED.contains(&ver) || ver == latest { format!("tree/{ver}.json") } else { format!("tree/_ondemand/{ver}.json") }
}

pub struct Versions {
    pub list: Vec<String>,
    pub latest: String,
}

pub fn versions(c: &Ctx) -> R<Versions> {
    let v = c.d.root("versions");
    let t = c.d.t(c.table(v, "versions")?);
    let list = c.seq(t.get("list").ok_or("versions.list missing")?, "versions.list")?;
    let list = list.into_iter().enumerate().map(|(i, x)| c.string(x, &format!("versions.list[{i}]"))).collect::<R<Vec<_>>>()?;
    let latest = c.string(t.get("latest").ok_or("versions.latest missing")?, "versions.latest")?;
    Ok(Versions { list, latest })
}

/// Every tree file: (path relative to data/, value), in manifest order (versions, then legion).
pub fn convert(c: &mut Ctx, vers: &Versions) -> R<Vec<(String, J)>> {
    let tree = tree_schema();
    let mut out = Vec::new();
    let mut nodes = 0;
    for ver in &vers.list {
        let root = format!("tree:{ver}");
        let v = c.d.roots.iter().find(|(n, _)| *n == root).map(|(_, v)| v.clone()).ok_or_else(|| format!("dump: no root '{root}'"))?;
        let file = out_file(ver, &vers.latest);
        let j = convert_value(c, &file, &v, &tree, &root)?;
        if let Some(n) = c.d.t(c.table(&v, &root)?).get("nodes").and_then(V::as_table) {
            nodes += c.d.t(n).entries.len();
        }
        out.push((file, j));
    }
    c.count("tree/*", "nodes, all versions", nodes);
    let v = c.d.root("tree:legion").clone();
    out.push(("tree/legion.json".into(), convert_value(c, "tree/legion.json", &v, &legion_schema(), "tree:legion")?));
    c.count("tree/*", "versions (treeVersionList)", vers.list.len());
    Ok(out)
}
