//! Data patches (`data-patches/*.json`, MINEFIELD-RUST R9): the pob-ts converter wrote `math.huge`/`nan` as JSON `null` and lost the
//! sign of `-0` in `mod-cache.json`/`mod-parser.json`. `tools/lua-corpus/export-data-patch.ts` takes the exact values from LuaJIT and
//! writes only the places where the data differs; the loaders of those two files apply them right after parsing.
//!
//! Format: `{format: 1, producer, manifestSha256, patches: [{file, path, value}]}`; `path` = object keys (strings) and 0-based array
//! indices (integers) from the file root (`mod-cache.json`: `[<line>, "mods", i, ...]`), `value` = `"inf" | "-inf" | "nan" | "-0"`.
//! `manifestSha256` must equal `data.lock`'s: a re-synced `data/` needs a re-taken patch (an error naming both sides). A patch
//! whose target is not what the converter writes (`null` for inf/nan, `0` for `-0`) is an error as well -- the data drifted.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Map, Number, Value as Json};

use crate::mod_cache::RawEntry;
use crate::{DataError, data_dir, lock_path, verify_lock};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PatchFile {
    format: u32,
    #[allow(dead_code)]
    producer: Json,
    manifest_sha256: String,
    patches: Vec<Patch>,
}

/// One patched number.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    /// `"mod-cache.json"` or `"mod-parser.json"`.
    pub file: String,
    pub path: Vec<Json>,
    /// `"inf" | "-inf" | "nan" | "-0"`.
    pub value: String,
}

/// `<repo>/data-patches/mod-numbers.json` (beside the data directory).
pub fn patch_path() -> PathBuf {
    data_dir().join("..").join("data-patches").join("mod-numbers.json")
}

/// Parse a patch file and check its `manifestSha256` against `lock_sha`.
pub fn parse_patches(text: &str, lock_sha: &str) -> Result<Vec<Patch>, DataError> {
    let f: PatchFile = serde_json::from_str(text).map_err(|e| DataError(format!("pob-data: mod-numbers.json: {e}")))?;
    if f.format != 1 {
        return Err(DataError(format!("pob-data: mod-numbers.json: format {}, expected 1", f.format)));
    }
    if f.manifest_sha256 != lock_sha {
        return Err(DataError(format!(
            "pob-data: mod-numbers.json was taken from data with manifest sha256 {}, but data.lock has {lock_sha} -- \
             re-take it: node tools/lua-corpus/export-data-patch.ts",
            f.manifest_sha256
        )));
    }
    Ok(f.patches)
}

/// All patches of this process (read once, after the `data.lock` check).
pub fn patches() -> Result<&'static [Patch], DataError> {
    static P: OnceLock<Result<Vec<Patch>, DataError>> = OnceLock::new();
    P.get_or_init(|| {
        let dir = crate::ensure_data()?;
        let lock = verify_lock(dir, &lock_path(dir))?;
        let path = patch_path();
        let text = std::fs::read_to_string(&path).map_err(|e| DataError(format!("pob-data: {}: {e}", path.display())))?;
        parse_patches(&text, &lock.manifest_sha256)
    })
    .as_ref()
    .map(Vec::as_slice)
    .map_err(Clone::clone)
}

fn num_marker(s: &str) -> Option<Json> {
    match s {
        "inf" | "-inf" | "nan" => {
            let mut o = Map::new();
            o.insert("$num".to_owned(), Json::String(s.to_owned()));
            Some(Json::Object(o))
        }
        "-0" => Number::from_f64(-0.0).map(Json::Number),
        _ => None,
    }
}

fn kind(j: &Json) -> &'static str {
    match j {
        Json::Null => "null",
        Json::Bool(_) => "a bool",
        Json::Number(_) => "a number",
        Json::String(_) => "a string",
        Json::Array(_) => "an array",
        Json::Object(_) => "an object",
    }
}

/// Set the number at `path` (relative to `root`) to `p.value`, checking what the converter left there.
pub fn apply_at(root: &mut Json, path: &[Json], p: &Patch) -> Result<(), DataError> {
    let err = |m: String| DataError(format!("pob-data: patch {} {:?}: {m}", p.file, p.path));
    let mut cur = root;
    for k in path {
        cur = match (k, cur) {
            (Json::String(s), Json::Object(o)) => o.get_mut(s.as_str()).ok_or_else(|| err(format!("no key {s:?}")))?,
            (Json::Number(n), Json::Array(a)) => {
                let i = n.as_u64().ok_or_else(|| err("bad index".to_owned()))? as usize;
                a.get_mut(i).ok_or_else(|| err(format!("no index {i}")))?
            }
            (k, c) => return Err(err(format!("path element {k} does not fit {}", kind(c)))),
        };
    }
    let new = num_marker(&p.value).ok_or_else(|| err(format!("unknown value {:?}", p.value)))?;
    let expected = if p.value == "-0" {
        cur.as_f64().is_some_and(|f| f == 0.0 && f.is_sign_positive())
    } else {
        cur.is_null()
    };
    if !expected {
        return Err(err(format!("the data holds {cur}, not what the converter writes for {}", p.value)));
    }
    *cur = new;
    Ok(())
}

/// Apply the `mod-parser.json` patches to the parsed file. Returns how many were applied.
pub(crate) fn apply_mod_parser(root: &mut Json) -> Result<usize, DataError> {
    let mut n = 0;
    for p in patches()?.iter().filter(|p| p.file == "mod-parser.json") {
        apply_at(root, &p.path, p)?;
        n += 1;
    }
    Ok(n)
}

/// Apply the `mod-cache.json` patches (`path[0]` = the entry's line, `path[1]` = `"mods"`). Returns how many were applied.
pub(crate) fn apply_mod_cache(raw: &mut [RawEntry]) -> Result<usize, DataError> {
    let mine: Vec<&Patch> = patches()?.iter().filter(|p| p.file == "mod-cache.json").collect();
    if mine.is_empty() {
        return Ok(0);
    }
    let index: HashMap<String, usize> = raw.iter().enumerate().map(|(i, e)| (e.line.clone(), i)).collect();
    for p in &mine {
        let bad = |m: &str| DataError(format!("pob-data: patch {} {:?}: {m}", p.file, p.path));
        let i = p
            .path
            .first()
            .and_then(Json::as_str)
            .and_then(|l| index.get(l))
            .copied()
            .ok_or_else(|| bad("no such mod-cache line"))?;
        if p.path.get(1).and_then(Json::as_str) != Some("mods") {
            return Err(bad("expected [line, \"mods\", ...]"));
        }
        let mods = raw[i].mods.take().ok_or_else(|| bad("the entry has no mods"))?;
        let mut arr = Json::Array(mods);
        let r = apply_at(&mut arr, &p.path[2..], p);
        let Json::Array(mods) = arr else { unreachable!() };
        raw[i].mods = Some(mods);
        r?;
    }
    Ok(mine.len())
}

// --- mod-parser-upvalues.json (R5.5b0) -------------------------------------------------------------------------------------

/// One closure whose upvalues the converter did not write: the `{fn}` node at `path` of `mod-parser.json` gets `up`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UpPatch {
    /// `"<src>:<from>"` -- must equal the node's `fn`.
    #[serde(rename = "fn")]
    pub fn_id: String,
    pub path: Vec<Json>,
    /// The upvalues in the converter's shape (`{name: value}`; functions `{fn, up}`, C functions `{cfn: true}`).
    pub up: Json,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpFile {
    format: u32,
    #[allow(dead_code)]
    producer: Json,
    manifest_sha256: String,
    patches: Vec<UpPatch>,
    /// LuaJIT's results of the patched visitors on a synthetic scenario (the Rust tests; not read by the loaders).
    goldens: Json,
}

/// `<repo>/data-patches/mod-parser-upvalues.json`.
pub fn upvalue_patch_path() -> PathBuf {
    data_dir().join("..").join("data-patches").join("mod-parser-upvalues.json")
}

/// Parse the upvalue patch file and check its `manifestSha256` against `lock_sha`.
fn parse_up_file(text: &str, lock_sha: &str) -> Result<UpFile, DataError> {
    let f: UpFile = serde_json::from_str(text).map_err(|e| DataError(format!("pob-data: mod-parser-upvalues.json: {e}")))?;
    if f.format != 1 {
        return Err(DataError(format!(
            "pob-data: mod-parser-upvalues.json: format {}, expected 1",
            f.format
        )));
    }
    if f.manifest_sha256 != lock_sha {
        return Err(DataError(format!(
            "pob-data: mod-parser-upvalues.json was taken from data with manifest sha256 {}, but data.lock has {lock_sha} -- \
             re-take it: node tools/lua-corpus/export-jewel-upvalues.ts",
            f.manifest_sha256
        )));
    }
    Ok(f)
}

fn up_file() -> Result<&'static UpFile, DataError> {
    static P: OnceLock<Result<UpFile, DataError>> = OnceLock::new();
    P.get_or_init(|| {
        let dir = crate::ensure_data()?;
        let lock = verify_lock(dir, &lock_path(dir))?;
        let path = upvalue_patch_path();
        let text = std::fs::read_to_string(&path).map_err(|e| DataError(format!("pob-data: {}: {e}", path.display())))?;
        parse_up_file(&text, &lock.manifest_sha256)
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// The upvalue patches of this process.
pub fn upvalue_patches() -> Result<&'static [UpPatch], DataError> {
    up_file().map(|f| f.patches.as_slice())
}

/// The JSON value type of [`upvalue_goldens`] (so a test without a `serde_json` dependency can name it).
pub type Golden = Json;

/// `{scenario, entries}`: the LuaJIT results of the patched visitors (see `tools/lua-corpus/export-jewel-upvalues.ts`).
pub fn upvalue_goldens() -> Result<&'static Json, DataError> {
    up_file().map(|f| &f.goldens)
}

/// Give the `{fn}` node at `p.path` its upvalues. The node must be exactly what the converter writes for a nested closure.
pub fn apply_upvalue_patch(root: &mut Json, p: &UpPatch) -> Result<(), DataError> {
    let err = |m: String| DataError(format!("pob-data: upvalue patch {:?}: {m}", p.path));
    let mut cur = root;
    for k in &p.path {
        cur = match (k, cur) {
            (Json::String(s), Json::Object(o)) => o.get_mut(s.as_str()).ok_or_else(|| err(format!("no key {s:?}")))?,
            (Json::Number(n), Json::Array(a)) => {
                let i = n.as_u64().ok_or_else(|| err("bad index".to_owned()))? as usize;
                a.get_mut(i).ok_or_else(|| err(format!("no index {i}")))?
            }
            (k, c) => return Err(err(format!("path element {k} does not fit {}", kind(c)))),
        };
    }
    let Json::Object(node) = cur else {
        return Err(err(format!("the target is {}, not a closure", kind(cur))));
    };
    if node.get("fn").and_then(Json::as_str) != Some(p.fn_id.as_str()) {
        return Err(err(format!(
            "the target is {}, expected fn {}",
            Json::Object(node.clone()),
            p.fn_id
        )));
    }
    let empty = match node.get("up") {
        None => true,
        Some(Json::Array(a)) => a.is_empty(),
        Some(Json::Object(o)) => o.is_empty(),
        Some(_) => false,
    };
    if !empty {
        return Err(err("the closure already carries its upvalues -- the data drifted".to_owned()));
    }
    node.insert("up".to_owned(), p.up.clone());
    Ok(())
}

/// Apply the upvalue patches to the parsed `mod-parser.json`. Returns how many were applied.
pub(crate) fn apply_mod_parser_upvalues(root: &mut Json) -> Result<usize, DataError> {
    let ps = upvalue_patches()?;
    for p in ps {
        apply_upvalue_patch(root, p)?;
    }
    Ok(ps.len())
}

// --- skill-levels-shape.json (R6.2) ---------------------------------------------------------------------------------------------

/// effect id -> (`#levels`, `next(levels)`).
pub type LevelsShapes = HashMap<String, (usize, f64)>;

/// `#grantedEffect.levels` and `next(grantedEffect.levels)` of one effect, as LuaJIT has them.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LevelsShape {
    pub len: usize,
    pub next: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ShapeFile {
    format: u32,
    #[allow(dead_code)]
    producer: Json,
    manifest_sha256: String,
    shapes: crate::wire::OrderedMap<LevelsShape>,
}

/// `<repo>/data-patches/skill-levels-shape.json`.
pub fn skill_levels_shape_path() -> PathBuf {
    data_dir().join("..").join("data-patches").join("skill-levels-shape.json")
}

/// Parse the levels-shape file and check its `manifestSha256` against `lock_sha`. The converter wrote `levels` as a sorted list, which
/// loses the Lua table's shape: `#levels` is the border LuaJIT finds (0 for a table built only with `[20] = ...`) and `next(levels)` the
/// first key of its traversal (array part first, then the hash nodes) -- both read by validateGemLevel/`naturalMaxLevel`/ProcessGemLevel.
/// Integer keys hash deterministically (MINEFIELD-RUST R11), so the patch is a plain table taken from LuaJIT
/// (`tools/lua-corpus/export-gemdata.ts`, checked equal over 20 processes).
pub fn parse_levels_shape(text: &str, lock_sha: &str) -> Result<HashMap<String, (usize, f64)>, DataError> {
    let f: ShapeFile = serde_json::from_str(text).map_err(|e| DataError(format!("pob-data: skill-levels-shape.json: {e}")))?;
    if f.format != 1 {
        return Err(DataError(format!(
            "pob-data: skill-levels-shape.json: format {}, expected 1",
            f.format
        )));
    }
    if f.manifest_sha256 != lock_sha {
        return Err(DataError(format!(
            "pob-data: skill-levels-shape.json was taken from data with manifest sha256 {}, but data.lock has {lock_sha} -- \
             re-take it: node tools/lua-corpus/export-gemdata.ts",
            f.manifest_sha256
        )));
    }
    Ok(f.shapes.0.into_iter().map(|(k, s)| (k, (s.len, s.next))).collect())
}

/// The levels shapes of this process (effect id -> (`#levels`, `next(levels)`)).
pub fn skill_levels_shape() -> Result<&'static HashMap<String, (usize, f64)>, DataError> {
    static P: OnceLock<Result<LevelsShapes, DataError>> = OnceLock::new();
    P.get_or_init(|| {
        let dir = crate::ensure_data()?;
        let lock = verify_lock(dir, &lock_path(dir))?;
        let path = skill_levels_shape_path();
        let text = std::fs::read_to_string(&path).map_err(|e| DataError(format!("pob-data: {}: {e}", path.display())))?;
        parse_levels_shape(&text, &lock.manifest_sha256)
    })
    .as_ref()
    .map_err(Clone::clone)
}
