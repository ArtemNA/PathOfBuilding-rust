//! Where the golden builds and their snapshots live (pob-ts `tools/oracle/paths.ts`). All roots are read-only.

use std::path::PathBuf;

use crate::snapshot::{Snapshot, SnapshotError, parse_snapshot};

pub const DEFAULT_POB_ROOT: &str = "C:/pob-work/project_15/PathOfBuilding";
pub const DEFAULT_ORACLE_ROOT: &str = "C:/pob-work/project_15/pob-engine-wasm";
pub const DEFAULT_POB_TS_ROOT: &str = "C:/pob-work/pob-ts";

fn root(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var).map_or_else(|| PathBuf::from(default), PathBuf::from)
}

/// Upstream PathOfBuilding checkout (`POB_ROOT`).
pub fn pob_root() -> PathBuf {
    root("POB_ROOT", DEFAULT_POB_ROOT)
}

/// project_15 wasm oracle (`POB_ORACLE`, same variable as pob-ts).
pub fn oracle_root() -> PathBuf {
    root("POB_ORACLE", DEFAULT_ORACLE_ROOT)
}

/// pob-ts checkout (`POB_TS_ROOT`).
pub fn pob_ts_root() -> PathBuf {
    root("POB_TS_ROOT", DEFAULT_POB_TS_ROOT)
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
        pob_ts_root().join("tools/oracle/goldens").join(format!("{}.json", self.name))
    }

    pub fn load_snapshot(&self) -> Result<Snapshot, SnapshotError> {
        let path = self.snapshot_path();
        let text = std::fs::read_to_string(&path).map_err(|e| SnapshotError(format!("{}: {e}", path.display())))?;
        parse_snapshot(&text, &path.display().to_string())
    }
}
