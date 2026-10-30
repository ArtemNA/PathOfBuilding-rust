//! The candidate output format of `pob-cli calc --json` (= pob-ts `CandidateResult`, candidate.ts:12-17, plus `unimplemented`).
//!
//! A key absent from `stats` is "not computed" (unimplemented in the comparator); NaN/±inf travel as the
//! canonical strings; `-0` is written as `-0.0`, which both serde_json and JS `JSON.parse` read back as -0.

use serde::{Deserialize, Serialize};

use crate::snapshot::PartialMeta;
use crate::value::Stats;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateOutput {
    pub stats: Stats,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<PartialMeta>,
    /// Upstream branches the engine skipped, as `"<File.lua>:<line> <what>"` (RUST-ARCH §5).
    pub unimplemented: Vec<String>,
}

impl CandidateOutput {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("candidate output serializes")
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        let mut out: CandidateOutput = serde_json::from_str(text)?;
        out.stats = std::mem::take(&mut out.stats)
            .into_iter()
            .map(|(k, v)| (k, v.normalized()))
            .collect();
        Ok(out)
    }
}
