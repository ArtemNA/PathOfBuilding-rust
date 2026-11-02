//! Tattoo passives (R-tattoo): `Data/TattooPassives.lua` RAW -- the table PassiveTree.lua:80 `require`s -- as a data patch taken from
//! LuaJIT (`data-patches/tattoo-passives.json`, `tools/lua-corpus/export-tattoo.ts`; pob-ts has no such data file). The processing of
//! PassiveTree.lua:758-786 (type, ProcessStats) is pob-tree's (`tattoo.rs`).
//!
//! `nodes` is keyed by `dn`; the patch holds the keys byte-sorted (the order kept here). Upstream walks it with `pairs` over string keys
//! (PassiveSpec.lua:170, the substitution of a renamed tattoo) -- MINEFIELD R13: no fixed order in LuaJIT itself.

use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value as Json;

use crate::wire::OrderedMap;
use crate::{DataError, data_dir, ensure_data, lock_path, verify_lock};

/// One `stats[key]` entry of a tattoo node (`index`/`statOrder` on the keystone tattoos only, `fmt` absent on 10).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TattooStat {
    pub fmt: Option<String>,
    pub index: Option<f64>,
    pub max: f64,
    pub min: f64,
    pub stat_order: Option<f64>,
}

/// `tattoo.nodes[dn]` -- every field the data has (`deny_unknown_fields`: a new one is a re-take error, not silently dropped).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TattooNode {
    pub id: String,
    pub dn: String,
    /// Absent on most nodes (`nil`); the runegrafts carry `"Runegraft Mastery"`.
    pub name: Option<String>,
    pub sd: Vec<String>,
    /// Not read by the calc (trade/tooltip data).
    pub stats: OrderedMap<TattooStat>,
    pub icon: String,
    #[serde(rename = "activeEffectImage")]
    pub active_effect_image: String,
    #[serde(rename = "isTattoo")]
    pub is_tattoo: bool,
    #[serde(rename = "overrideType")]
    pub override_type: String,
    pub ks: bool,
    pub m: bool,
    #[serde(rename = "not")]
    pub not_: bool,
    #[serde(rename = "reminderText")]
    pub reminder_text: Option<Vec<String>>,
    /// Display / trade only (TreeTab): kept to keep the schema closed.
    pub legacy: Option<bool>,
    #[serde(rename = "MaximumConnected")]
    pub maximum_connected: f64,
    #[serde(rename = "MinimumConnected")]
    pub minimum_connected: f64,
    #[serde(rename = "targetType")]
    pub target_type: String,
    #[serde(rename = "targetValue")]
    pub target_value: String,
}

/// `Data/TattooPassives.lua`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TattooPassives {
    format: u32,
    #[allow(dead_code)]
    producer: Json,
    manifest_sha256: String,
    pob_sha: String,
    /// Display only (`groups[1000000000]`).
    pub groups: Json,
    /// dn -> node, byte-sorted keys.
    pub nodes: OrderedMap<TattooNode>,
}

/// `<repo>/data-patches/tattoo-passives.json`.
pub fn tattoo_patch_path() -> PathBuf {
    data_dir().join("..").join("data-patches").join("tattoo-passives.json")
}

/// Parse the tattoo patch and check it against the lock (`manifestSha256` and `pobSha`).
pub fn parse_tattoo(text: &str, lock_sha: &str, lock_pob: &str) -> Result<TattooPassives, DataError> {
    let t: TattooPassives = serde_json::from_str(text).map_err(|e| DataError(format!("pob-data: tattoo-passives.json: {e}")))?;
    if t.format != 1 {
        return Err(DataError(format!(
            "pob-data: tattoo-passives.json: format {}, expected 1",
            t.format
        )));
    }
    if t.manifest_sha256 != lock_sha || t.pob_sha != lock_pob {
        return Err(DataError(format!(
            "pob-data: tattoo-passives.json was taken from data {} / PoB {}, but data.lock has {lock_sha} / {lock_pob} -- re-take it: \
             node tools/lua-corpus/export-tattoo.ts",
            t.manifest_sha256, t.pob_sha
        )));
    }
    Ok(t)
}

/// The tattoo passives of this process (read once, after the `data.lock` check).
pub fn tattoo_passives() -> Result<&'static TattooPassives, DataError> {
    static T: OnceLock<Result<TattooPassives, DataError>> = OnceLock::new();
    T.get_or_init(|| {
        let dir = ensure_data()?;
        let lock = verify_lock(dir, &lock_path(dir))?;
        let path = tattoo_patch_path();
        let text = std::fs::read_to_string(&path).map_err(|e| DataError(format!("pob-data: {}: {e}", path.display())))?;
        parse_tattoo(&text, &lock.manifest_sha256, &lock.pob_sha)
    })
    .as_ref()
    .map_err(Clone::clone)
}
