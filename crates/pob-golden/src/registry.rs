//! Where the golden builds and their snapshots live (pob-ts `tools/oracle/paths.ts`). All roots are read-only.
//!
//! The upstream roots come from the ONE config `tools/upstream.json` (R-UP); env vars `POB_ROOT` / `POB_ORACLE` /
//! `POB_TS_ROOT` / `POB_GOLDENS_DIR` override it.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::snapshot::{Snapshot, SnapshotError, parse_snapshot};

/// Repo root of pob-rust (this crate is `crates/pob-golden`).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `tools/upstream.json` -- which upstream PoB checkout (and sha) pob-rust targets.
pub fn upstream_config() -> &'static serde_json::Value {
    static CFG: OnceLock<serde_json::Value> = OnceLock::new();
    CFG.get_or_init(|| {
        let path = repo_root().join("tools/upstream.json");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    })
}

fn root(var: &str, key: &str) -> PathBuf {
    if let Some(v) = std::env::var_os(var) {
        return PathBuf::from(v);
    }
    let p = upstream_config()[key]
        .as_str()
        .unwrap_or_else(|| panic!("tools/upstream.json: missing \"{key}\""));
    let p = PathBuf::from(p);
    if p.is_absolute() { p } else { repo_root().join(p) }
}

/// Upstream PathOfBuilding checkout (`POB_ROOT`, else `pobRoot`).
pub fn pob_root() -> PathBuf {
    root("POB_ROOT", "pobRoot")
}

/// project_15 wasm oracle (`POB_ORACLE`, same variable as pob-ts; else `oracleRoot`).
pub fn oracle_root() -> PathBuf {
    root("POB_ORACLE", "oracleRoot")
}

/// pob-ts checkout (`POB_TS_ROOT`, else `pobTsRoot`).
pub fn pob_ts_root() -> PathBuf {
    root("POB_TS_ROOT", "pobTsRoot")
}

/// Oracle golden snapshots of the target upstream (`POB_GOLDENS_DIR`, else `goldensDir`; R-UP moved them out of
/// pob-ts, whose `tools/oracle/goldens` stay at the sha pob-ts was built for).
pub fn goldens_dir() -> PathBuf {
    root("POB_GOLDENS_DIR", "goldensDir")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoldenSource {
    /// PathOfBuilding/spec/TestBuilds/3.13
    Upstream,
    /// pob-engine-wasm/test/fixtures
    OracleFixture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoldenBuild {
    pub name: &'static str,
    pub source: GoldenSource,
}

/// The golden set in name order (paths.ts `GOLDEN_BUILDS`).
pub const GOLDEN_BUILDS: [GoldenBuild; 6] = [
    GoldenBuild {
        name: "Dual Savior",
        source: GoldenSource::Upstream,
    },
    GoldenBuild {
        name: "Dual Wield Cospris CoC",
        source: GoldenSource::Upstream,
    },
    GoldenBuild {
        name: "Dual Wield Cospris CoC - Lethal Pride",
        source: GoldenSource::OracleFixture,
    },
    GoldenBuild {
        name: "Generals Perforate Zerker",
        source: GoldenSource::Upstream,
    },
    GoldenBuild {
        name: "Mirage Archer Toxic Rain",
        source: GoldenSource::Upstream,
    },
    GoldenBuild {
        name: "OccVortex",
        source: GoldenSource::Upstream,
    },
];

pub fn find_golden(name: &str) -> Option<GoldenBuild> {
    GOLDEN_BUILDS.iter().copied().find(|b| b.name == name)
}

impl GoldenBuild {
    pub fn xml_path(&self) -> PathBuf {
        match self.source {
            GoldenSource::Upstream => pob_root().join("spec/TestBuilds/3.13").join(format!("{}.xml", self.name)),
            GoldenSource::OracleFixture => oracle_root().join("test/fixtures").join(format!("{}.xml", self.name)),
        }
    }

    pub fn snapshot_path(&self) -> PathBuf {
        goldens_dir().join(format!("{}.json", self.name))
    }

    pub fn load_snapshot(&self) -> Result<Snapshot, SnapshotError> {
        let path = self.snapshot_path();
        let text = std::fs::read_to_string(&path).map_err(|e| SnapshotError(format!("{}: {e}", path.display())))?;
        parse_snapshot(&text, &path.display().to_string())
    }
}
