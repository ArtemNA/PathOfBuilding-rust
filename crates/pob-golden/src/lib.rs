//! Oracle goldens for pob-rust: reads pob-ts snapshots, compares a candidate with the round4 rule, renders the report.
//! Semantics: `reference/pob-ts/ORACLE.md`, `KNOWLEDGE/RUST-ARCH.md` §6.

pub mod candidate;
pub mod compare;
pub mod registry;
pub mod snapshot;
pub mod value;

pub use candidate::CandidateOutput;
pub use compare::{
    CompareReport, MetaDiff, StatClass, StatEntry, compare_meta, compare_stats, is_parity, render_report, round4, summary_line,
};
pub use registry::{GOLDEN_BUILDS, GoldenBuild, find_golden};
pub use snapshot::{Meta, PartialMeta, Snapshot, parse_snapshot};
pub use value::{StatValue, Stats};
