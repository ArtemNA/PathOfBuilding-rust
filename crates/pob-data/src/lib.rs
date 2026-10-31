//! The converted PoB data (`data/*.json`, synced from pob-ts by `tools/sync-data.ps1`) as serde models with lazy loaders --
//! the files phase R3 reads (`KNOWLEDGE/R3-ARCH.md` §1). Executable spec: pob-ts `src/data/*.ts`.
//!
//! * Root: `POB_DATA_DIR`, else `<workspace>/data`; the lock file is `data.lock` next to that directory.
//! * First access to any file checks `data.lock` against `data/manifest.json` (sha256 + pobSha + converterVersion,
//!   RUST-ARCH §8); a mismatch is a start-up error (`ensure_data`), and loaders panic with it.
//! * Every loader parses its file once per process (`OnceLock`), except the mod cache: its `pob_mods::Mod`s hold `Rc`s, so
//!   it is cached per thread (`mod_cache`).
//! * Order (D13): JSON objects are read into insertion-ordered maps -- the converter wrote them key-sorted.

#![forbid(unsafe_code)]

pub mod cluster_jewels;
pub mod config_options;
pub mod constants;
pub mod gems;
pub mod globals;
pub mod item_bases;
pub mod item_mods;
pub mod mod_cache;
pub mod mod_parser;
pub mod mod_scalability;
pub mod pantheons;
pub mod patches;
pub mod rules;
pub mod sha256;
pub mod tree;
pub mod wire;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;
use serde::de::DeserializeOwned;

/// A data start-up error (missing file, bad JSON, `data.lock` mismatch).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataError(pub String);

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DataError {}

/// `POB_DATA_DIR`, or `<workspace root>/data`.
pub fn data_dir() -> PathBuf {
    match std::env::var_os("POB_DATA_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data")),
    }
}

/// `data.lock` beside the data directory.
pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join("..").join("data.lock")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockFile {
    pob_sha: String,
    converter_version: String,
    manifest_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestHead {
    pob_sha: String,
    converter_version: String,
}

/// What a passed lock check vouches for.
#[derive(Clone, Debug)]
pub struct LockInfo {
    pub pob_sha: String,
    pub converter_version: String,
    pub manifest_sha256: String,
}

/// RUST-ARCH §8: `lock.manifestSha256` = sha256 of `<dir>/manifest.json`, and the manifest's `pobSha`/`converterVersion`
/// are the locked ones.
pub fn verify_lock(dir: &Path, lock: &Path) -> Result<LockInfo, DataError> {
    let read = |p: &Path| std::fs::read(p).map_err(|e| DataError(format!("pob-data: {}: {e}", p.display())));
    let lock_bytes = read(lock)?;
    let lock_file: LockFile = serde_json::from_slice(&lock_bytes).map_err(|e| DataError(format!("pob-data: {}: {e}", lock.display())))?;
    let manifest_path = dir.join("manifest.json");
    let manifest = read(&manifest_path)?;
    let sha = sha256::sha256_hex(&manifest);
    if sha != lock_file.manifest_sha256 {
        return Err(DataError(format!(
            "pob-data: {} has sha256 {sha}, but {} locks {} -- run tools/sync-data.ps1",
            manifest_path.display(),
            lock.display(),
            lock_file.manifest_sha256
        )));
    }
    let head: ManifestHead =
        serde_json::from_slice(&manifest).map_err(|e| DataError(format!("pob-data: {}: {e}", manifest_path.display())))?;
    if head.pob_sha != lock_file.pob_sha || head.converter_version != lock_file.converter_version {
        return Err(DataError(format!(
            "pob-data: manifest pobSha/converterVersion {}/{} differ from the lock's {}/{}",
            head.pob_sha, head.converter_version, lock_file.pob_sha, lock_file.converter_version
        )));
    }
    Ok(LockInfo {
        pob_sha: head.pob_sha,
        converter_version: head.converter_version,
        manifest_sha256: sha,
    })
}

/// The data directory after the one-time lock check of this process.
pub fn ensure_data() -> Result<&'static Path, DataError> {
    static CHECKED: OnceLock<Result<PathBuf, DataError>> = OnceLock::new();
    CHECKED
        .get_or_init(|| {
            let dir = data_dir();
            verify_lock(&dir, &lock_path(&dir)).map(|_| dir)
        })
        .as_ref()
        .map(PathBuf::as_path)
        .map_err(Clone::clone)
}

/// Read and parse `<data>/<rel>` (after the lock check).
pub fn read_json<T: DeserializeOwned>(rel: &str) -> Result<T, DataError> {
    let dir = ensure_data()?;
    let path = dir.join(rel);
    let text = std::fs::read_to_string(&path).map_err(|e| DataError(format!("pob-data: {}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| DataError(format!("pob-data: {}: {e}", path.display())))
}

/// `read_json` for the `OnceLock` loaders: a data error at start-up is fatal (RUST-ARCH §8).
pub(crate) fn load<T: DeserializeOwned>(rel: &str) -> T {
    read_json(rel).unwrap_or_else(|e| panic!("{e}"))
}

/// Force every R3 file through its loader (the cold-load measurement and a smoke test): the four committed trees, the
/// mod cache of this thread and the rest. Returns the number of files read.
pub fn load_all_r3() -> usize {
    let _ = item_bases::item_bases();
    let _ = constants::constants();
    let _ = rules::rules();
    let _ = globals::globals();
    let _ = pantheons::pantheons();
    let _ = config_options::config_options();
    let _ = mod_scalability::mod_scalability();
    let _ = gems::gems();
    let _ = cluster_jewels::cluster_jewels();
    let _ = mod_cache::mod_cache();
    for v in tree::COMMITTED_TREE_VERSIONS {
        let _ = tree::load_tree(v).unwrap_or_else(|e| panic!("{e}"));
    }
    10 + tree::COMMITTED_TREE_VERSIONS.len()
}
