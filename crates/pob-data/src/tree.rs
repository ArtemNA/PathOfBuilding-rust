//! `TreeData/<ver>/tree.lua` as the converter snapshotted it -- the raw table `PassiveTreeClass:PassiveTree` copies onto
//! itself (D12: nothing transformed). pob-ts `src/data/tree.ts`. Shapes depend on the version (D16): `groups` is an array up
//! to 3_17 and an object from 3_18; `classes` is an array (index = classId) from 3.10 and an object `"0".."6"` before;
//! modern trees keep `nodes["root"]` (without `skill`), legacy ones a top-level `root`. Access goes through `LuaTable`.
//!
//! Only the fields the calc reads are typed; UI-only keys (sprites, assets, flavour) are skipped on load.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};
use serde_json::Value as Json;

use crate::DataError;
use crate::wire::{LuaTable, OrderedMap};

/// A node reference in `in`/`out`/`group.nodes`: a number in legacy trees, a string id in modern ones.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeRef {
    Num(f64),
    Str(String),
}

impl<'de> Deserialize<'de> for NodeRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V_;
        impl<'de> Visitor<'de> for V_ {
            type Value = NodeRef;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a node id (number or string)")
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<NodeRef, E> {
                Ok(NodeRef::Num(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<NodeRef, E> {
                Ok(NodeRef::Num(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<NodeRef, E> {
                Ok(NodeRef::Num(v as f64))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<NodeRef, E> {
                Ok(NodeRef::Str(v.to_owned()))
            }
        }
        d.deserialize_any(V_)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct TreeAscendancy {
    /// Modern trees only; legacy ascendancies have `name`/`displayName`.
    pub id: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TreeClass {
    pub name: String,
    pub base_str: Option<f64>,
    pub base_dex: Option<f64>,
    pub base_int: Option<f64>,
    /// Modern (>= 3.10); PassiveTree.lua:179 migrates it onto `classes`.
    pub ascendancies: Option<Vec<TreeAscendancy>>,
    /// Legacy (< 3.10): already the migrated name.
    pub classes: Option<LuaTable<TreeAscendancy>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeGroup {
    pub x: f64,
    pub y: f64,
    /// Modern; legacy uses `n`/`oo`.
    pub orbits: Option<Vec<f64>>,
    pub nodes: Option<Vec<NodeRef>>,
    pub n: Option<LuaTable<f64>>,
    pub oo: Option<LuaTable<bool>>,
    pub is_proxy: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TreeExpansionJewel {
    pub index: Option<f64>,
    pub parent: Option<String>,
    pub proxy: Option<String>,
    pub size: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeMasteryEffect {
    pub effect: f64,
    pub stats: Vec<String>,
    pub reminder_text: Option<Vec<String>>,
}

/// A node as the JSON holds it -- modern field names and the legacy abbreviations side by side.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    // modern (>= 3.10)
    pub skill: Option<f64>,
    pub name: Option<String>,
    pub stats: Option<Vec<String>>,
    pub group: Option<f64>,
    pub orbit: Option<f64>,
    pub orbit_index: Option<f64>,
    pub granted_passive_points: Option<f64>,
    // legacy (< 3.10)
    pub id: Option<f64>,
    pub dn: Option<String>,
    pub sd: Option<Vec<String>>,
    pub g: Option<f64>,
    pub o: Option<f64>,
    pub oidx: Option<f64>,
    pub ks: Option<bool>,
    pub m: Option<bool>,
    #[serde(rename = "not")]
    pub not_: Option<bool>,
    pub spc: Option<LuaTable<f64>>,
    pub passive_points_granted: Option<f64>,
    pub sa: Option<f64>,
    pub da: Option<f64>,
    pub ia: Option<f64>,
    // common
    #[serde(rename = "in")]
    pub in_: Option<LuaTable<NodeRef>>,
    pub out: Option<LuaTable<NodeRef>>,
    pub icon: Option<String>,
    pub is_notable: Option<bool>,
    pub is_keystone: Option<bool>,
    pub is_mastery: Option<bool>,
    pub is_jewel_socket: Option<bool>,
    pub is_multiple_choice: Option<bool>,
    pub is_multiple_choice_option: Option<bool>,
    pub is_ascendancy_start: Option<bool>,
    pub is_blighted: Option<bool>,
    pub is_bloodline: Option<bool>,
    pub is_proxy: Option<bool>,
    pub ascendancy_name: Option<String>,
    pub class_start_index: Option<f64>,
    pub granted_strength: Option<f64>,
    pub granted_dexterity: Option<f64>,
    pub granted_intelligence: Option<f64>,
    pub reminder_text: Option<Vec<String>>,
    pub mastery_effects: Option<Vec<TreeMasteryEffect>>,
    pub active_effect_image: Option<String>,
    pub expansion_jewel: Option<TreeExpansionJewel>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeConstants {
    pub orbit_radii: Option<Vec<f64>>,
    pub skills_per_orbit: Option<Vec<f64>>,
    #[serde(rename = "PSSCentreInnerRadius")]
    pub pss_centre_inner_radius: Option<f64>,
    pub classes: Option<OrderedMap<f64>>,
    pub character_attributes: Option<OrderedMap<f64>>,
}

#[derive(Debug, Deserialize)]
pub struct TreeJson {
    pub tree: Option<String>,
    pub classes: LuaTable<TreeClass>,
    pub groups: LuaTable<TreeGroup>,
    /// Keyed by node id (string). Modern trees include `"root"`. 2_6 writes `nodes` as a Lua array: it is keyed by the Lua key
    /// `"1".."n"` (what `pairs` sees); the node id is the node's own `id` field (PassiveTree.lua:543 `nodeMap[node.id]`).
    #[serde(deserialize_with = "nodes_table")]
    pub nodes: OrderedMap<TreeNode>,
    /// Legacy trees keep `root` beside `nodes`.
    pub root: Option<TreeNode>,
    pub constants: TreeConstants,
    #[serde(rename = "jewelSlots")]
    pub jewel_slots: Option<Vec<f64>>,
    pub min_x: Option<f64>,
    pub min_y: Option<f64>,
    pub max_x: Option<f64>,
    pub max_y: Option<f64>,
    pub alternate_ascendancies: Option<Json>,
    pub points: Option<Json>,
    /// 3.06 trees only: the class base attributes live here, keyed by classId (`"0".."6"`), not on `classes[i]`
    /// (CalcSetup.lua:588 `tree.characterData and tree.characterData[classId] or tree.classes[classId]`, MINEFIELD D19.5).
    #[serde(rename = "characterData")]
    pub character_data: Option<OrderedMap<CharacterData>>,
}

/// `tree.nodes`: a JSON object, or a JSON array (2_6) keyed by its 1-based Lua key.
fn nodes_table<'de, D: Deserializer<'de>>(d: D) -> Result<OrderedMap<TreeNode>, D::Error> {
    Ok(match LuaTable::<TreeNode>::deserialize(d)? {
        LuaTable::Object(m) => OrderedMap(m),
        LuaTable::Array(v) => OrderedMap(v.into_iter().enumerate().map(|(i, n)| ((i + 1).to_string(), n)).collect()),
    })
}

/// One `tree.characterData[classId]` entry of a 3.06 tree.
#[derive(Clone, Debug, Deserialize)]
pub struct CharacterData {
    pub base_str: Option<f64>,
    pub base_dex: Option<f64>,
    pub base_int: Option<f64>,
}

impl TreeJson {
    /// `tree.classes[classId]` -- array index for modern trees (0-based classId, D16), object key for legacy.
    pub fn class(&self, class_id: i64) -> Option<&TreeClass> {
        match &self.classes {
            LuaTable::Array(v) => usize::try_from(class_id).ok().and_then(|i| v.get(i)),
            LuaTable::Object(m) => m.get(class_id.to_string().as_str()),
        }
    }

    /// `pairs(tree.alternate_ascendancies)` as `(Lua key, id, name)` in ascending key order: a JSON array is a Lua sequence (key =
    /// index + 1), an object has number keys. Empty when the tree has none (PassiveTree.lua:133).
    pub fn alternate_ascendancy_list(&self) -> Result<Vec<(i64, Option<String>, String)>, DataError> {
        let entry = |k: i64, v: &Json| {
            let id = v.get("id").and_then(Json::as_str).map(str::to_owned);
            let name = v.get("name").and_then(Json::as_str).unwrap_or_default().to_owned();
            (k, id, name)
        };
        let mut out = Vec::new();
        match self.alternate_ascendancies.as_ref() {
            None | Some(Json::Null) => {}
            Some(Json::Array(a)) => out.extend(a.iter().enumerate().map(|(i, v)| entry(i as i64 + 1, v))),
            Some(Json::Object(o)) => {
                for (k, v) in o {
                    let k: i64 = k
                        .parse()
                        .map_err(|_| DataError(format!("pob-data: alternate_ascendancies key '{k}' is not a number")))?;
                    out.push(entry(k, v));
                }
                out.sort_by_key(|e| e.0);
            }
            Some(v) => return Err(DataError(format!("pob-data: alternate_ascendancies is {v}"))),
        }
        Ok(out)
    }
}

/// The trees committed under `data/tree/` (golden versions + latest); the other 37 live in `data/tree/_ondemand/`.
pub const COMMITTED_TREE_VERSIONS: [&str; 4] = ["3_6", "3_12", "3_13", "3_29"];

/// One `treeVersions[ver]` entry of GameVersions.lua (`url` is UI only and not exported).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeVersion {
    pub ver: String,
    pub display: String,
    pub num: f64,
}

/// `data-patches/tree-versions.json` (`tools/lua-corpus/export-tree-versions.ts`): GameVersions.lua taken from LuaJIT -- the converter
/// snapshots the tree data but not this table.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TreeVersions {
    format: u32,
    #[allow(dead_code)]
    producer: Json,
    manifest_sha256: String,
    pob_sha: String,
    pub latest_tree_version: String,
    pub default_tree_version: String,
    /// In `treeVersionList` order.
    pub tree_versions: Vec<TreeVersion>,
}

/// `<repo>/data-patches/tree-versions.json`.
pub fn tree_versions_patch_path() -> std::path::PathBuf {
    crate::data_dir().join("..").join("data-patches").join("tree-versions.json")
}

/// Parse the tree-versions patch and check it against the lock (`manifestSha256` and `pobSha`).
pub fn parse_tree_versions(text: &str, lock_sha: &str, lock_pob: &str) -> Result<TreeVersions, DataError> {
    let t: TreeVersions = serde_json::from_str(text).map_err(|e| DataError(format!("pob-data: tree-versions.json: {e}")))?;
    if t.format != 1 {
        return Err(DataError(format!("pob-data: tree-versions.json: format {}, expected 1", t.format)));
    }
    if t.manifest_sha256 != lock_sha || t.pob_sha != lock_pob {
        return Err(DataError(format!(
            "pob-data: tree-versions.json was taken from data {} / PoB {}, but data.lock has {lock_sha} / {lock_pob} -- re-take it:              node tools/lua-corpus/export-tree-versions.ts",
            t.manifest_sha256, t.pob_sha
        )));
    }
    Ok(t)
}

/// GameVersions.lua of this process (read once, after the `data.lock` check).
pub fn tree_versions() -> Result<&'static TreeVersions, DataError> {
    static T: OnceLock<Result<TreeVersions, DataError>> = OnceLock::new();
    T.get_or_init(|| {
        let dir = crate::ensure_data()?;
        let lock = crate::verify_lock(dir, &crate::lock_path(dir))?;
        let path = tree_versions_patch_path();
        let text = std::fs::read_to_string(&path).map_err(|e| DataError(format!("pob-data: {}: {e}", path.display())))?;
        parse_tree_versions(&text, &lock.manifest_sha256, &lock.pob_sha)
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// `treeVersionList` (GameVersions.lua:11).
pub fn tree_version_list() -> Result<Vec<&'static str>, DataError> {
    Ok(tree_versions()?.tree_versions.iter().map(|v| v.ver.as_str()).collect())
}

/// `treeVersions[ver].num` (GameVersions.lua) -- the number decides the migration branch (`>= 3.10`), so it is data, not a parse
/// of the name ("3_6" is 3.06). An unknown version is an error naming it (W4).
pub fn tree_version_num(version: &str) -> Result<f64, DataError> {
    let t = tree_versions()?;
    t.tree_versions.iter().find(|v| v.ver == version).map(|v| v.num).ok_or_else(|| {
        DataError(format!(
            "unknown tree version '{version}' (GameVersions.lua treeVersions has {} versions, {} .. {})",
            t.tree_versions.len(),
            t.tree_versions.first().map_or("", |v| v.ver.as_str()),
            t.latest_tree_version
        ))
    })
}

/// `data/tree/<ver>.json`, else `data/tree/_ondemand/<ver>.json` for the other versions of the manifest, parsed once per
/// version. W4: a missing tree is an error naming the version, never a nil cascade.
pub fn load_tree(version: &str) -> Result<&'static TreeJson, DataError> {
    static CACHE: OnceLock<Mutex<HashMap<String, &'static TreeJson>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(t) = cache.lock().expect("tree cache").get(version) {
        return Ok(t);
    }
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(DataError(format!("invalid tree version '{version}'")));
    }
    let rel = if COMMITTED_TREE_VERSIONS.contains(&version) {
        format!("tree/{version}.json")
    } else {
        format!("tree/_ondemand/{version}.json")
    };
    let tree: TreeJson = crate::read_json(&rel).map_err(|e| DataError(format!("passive tree data for version '{version}': {e}")))?;
    let leaked: &'static TreeJson = Box::leak(Box::new(tree));
    // Two threads may race to parse the same version; the first insert wins, the loser's copy stays leaked (bounded by
    // the number of versions).
    Ok(*cache.lock().expect("tree cache").entry(version.to_owned()).or_insert(leaked))
}
