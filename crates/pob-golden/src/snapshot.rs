//! Oracle snapshots as pob-ts writes them (`tools/oracle/format.ts` `snapshotSchema`, ORACLE.md).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::value::{StatValue, Stats};

pub const SNAPSHOT_FORMAT: u32 = 1;
pub const CASE_FORMAT: u32 = 1;

/// One comparison input: a build plus mutations (format.ts `caseSchema`). Mutations are kept opaque until an engine runs them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OracleCase {
    pub format: u32,
    pub id: String,
    pub build: String,
    pub xml_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    pub mutations: Vec<serde_json::Value>,
}

/// `getStats()._meta` (format.ts `metaSchema`). `mainSocketGroup` is absent when the build's is nil (dkjson drops nil).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Meta {
    pub alloc_nodes: u32,
    pub items: u32,
    pub socket_groups: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_socket_group: Option<i64>,
    pub tree_version: String,
    pub class_name: String,
    pub ascend_name: String,
}

/// What a candidate may report of `_meta`: any subset of the fields (candidate.ts `Partial<Meta>`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PartialMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alloc_nodes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket_groups: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_socket_group: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ascend_name: Option<String>,
}

impl From<&Meta> for PartialMeta {
    fn from(m: &Meta) -> Self {
        PartialMeta {
            alloc_nodes: Some(m.alloc_nodes),
            items: Some(m.items),
            socket_groups: Some(m.socket_groups),
            main_socket_group: m.main_socket_group,
            tree_version: Some(m.tree_version.clone()),
            class_name: Some(m.class_name.clone()),
            ascend_name: Some(m.ascend_name.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectedMutation {
    pub index: u32,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Snapshot {
    pub format: u32,
    pub case: OracleCase,
    /// `{kind, pobSha, engine}` -- informational here.
    pub producer: serde_json::Value,
    pub meta: Meta,
    /// Exact doubles (%.17g channel) + strings/booleans: the comparator's reference (O8).
    pub stats: Stats,
    /// The %.14g channel -- informational, never compared against an exact-double candidate (O8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats14: Option<Stats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_type: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neg_zero: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected: Option<Vec<RejectedMutation>>,
}

#[derive(Debug)]
pub struct SnapshotError(pub String);

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SnapshotError {}

/// format.ts `parseSnapshot`: validates, normalizes stat values, restores `-0` for `negZero` keys.
pub fn parse_snapshot(text: &str, source: &str) -> Result<Snapshot, SnapshotError> {
    let mut snap: Snapshot = serde_json::from_str(text).map_err(|e| SnapshotError(format!("{source}: {e}")))?;
    if snap.format != SNAPSHOT_FORMAT || snap.case.format != CASE_FORMAT {
        return Err(SnapshotError(format!(
            "{source}: unsupported format {} / case format {}",
            snap.format, snap.case.format
        )));
    }
    snap.stats = std::mem::take(&mut snap.stats)
        .into_iter()
        .map(|(k, v)| (k, v.normalized()))
        .collect();
    for key in snap.neg_zero.clone().unwrap_or_default() {
        match snap.stats.get_mut(&key) {
            Some(StatValue::Num(v)) if *v == 0.0 => *v = -0.0,
            _ => return Err(SnapshotError(format!("{source}: negZero key '{key}' is not a zero in stats"))),
        }
    }
    Ok(snap)
}
