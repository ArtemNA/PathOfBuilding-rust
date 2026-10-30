//! The comparison rule of the oracle harness -- a mirror of pob-ts `tools/oracle/compare.ts` (ORACLE.md).
//!
//! Every key of `reference ∪ candidate` is classified exactly once:
//!   ok            round4-equal numbers / strictly equal strings+booleans (O3); `exact` = bit-for-bit
//!   mismatch      both sides have the key, values differ under the rule
//!   unimplemented reference has the key, candidate does not
//!   extra         candidate has a key the reference does not
//! Parity gate: mismatch == 0 && extra == 0; `strict` also demands unimplemented == 0.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::snapshot::{Meta, PartialMeta};
use crate::value::{META_KEY, StatValue, Stats};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatClass {
    Ok,
    Mismatch,
    Unimplemented,
    Extra,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatEntry {
    pub key: String,
    pub cls: StatClass,
    pub expected: Option<StatValue>,
    pub actual: Option<StatValue>,
    /// `Ok` only: bit-for-bit (`f64::to_bits`, so -0 != 0).
    pub exact: bool,
    /// `Mismatch` only: the numbers are within `NEAR_TIE_REL` -- a round4 tie artefact (O8). Still a mismatch for the gate.
    pub near_tie: bool,
}

/// Relative distance below which a round4 mismatch is a tie artefact, not a different value.
pub const NEAR_TIE_REL: f64 = 1e-12;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompareReport {
    /// Keys in the reference (= ok + mismatch + unimplemented).
    pub total: usize,
    pub ok: usize,
    /// Subset of ok.
    pub exact: usize,
    pub mismatch: usize,
    pub unimplemented: usize,
    pub extra: usize,
    /// Every classified key, in byte order.
    pub entries: Vec<StatEntry>,
}

/// PathOfBuilding/src/Modules/Common.lua `round(val, 4)`: `m_floor(val * 10^4 + 0.5) / 10^4`. Non-finite values pass through.
pub fn round4(v: f64) -> f64 {
    if v.is_finite() { (v * 10000.0 + 0.5).floor() / 10000.0 } else { v }
}

/// O3: numbers by round4 on both sides; everything else strictly; non-finite values meet as their canonical strings.
pub fn stats_match(expected: &StatValue, actual: &StatValue) -> bool {
    match (expected.clone().normalized(), actual.clone().normalized()) {
        (StatValue::Num(e), StatValue::Num(a)) => round4(e) == round4(a),
        (e, a) => e == a,
    }
}

/// Bit-for-bit: `to_bits` for numbers (-0 != 0), `==` otherwise.
pub fn stats_exact(expected: &StatValue, actual: &StatValue) -> bool {
    match (expected.clone().normalized(), actual.clone().normalized()) {
        (StatValue::Num(e), StatValue::Num(a)) => e.to_bits() == a.to_bits(),
        (e, a) => e == a,
    }
}

/// Classifies every key of `reference ∪ candidate` (minus `_meta` and `ignore`).
pub fn compare_stats(reference: &Stats, candidate: &Stats, ignore: &[&str]) -> CompareReport {
    let skip = |k: &String| k == META_KEY || ignore.contains(&k.as_str());
    let keys: BTreeSet<&String> = reference.keys().chain(candidate.keys()).filter(|k| !skip(k)).collect();

    let mut r = CompareReport::default();
    for key in keys {
        let expected = reference.get(key);
        let actual = candidate.get(key);
        let mut entry = StatEntry {
            key: key.clone(),
            cls: StatClass::Extra,
            expected: expected.cloned(),
            actual: actual.cloned(),
            exact: false,
            near_tie: false,
        };
        match (expected, actual) {
            (None, _) => r.extra += 1,
            (Some(_), None) => {
                r.total += 1;
                entry.cls = StatClass::Unimplemented;
                r.unimplemented += 1;
            }
            (Some(e), Some(a)) => {
                r.total += 1;
                if stats_match(e, a) {
                    entry.cls = StatClass::Ok;
                    entry.exact = stats_exact(e, a);
                    r.ok += 1;
                    if entry.exact {
                        r.exact += 1;
                    }
                } else {
                    entry.cls = StatClass::Mismatch;
                    if let (Some(ev), Some(av)) = (e.as_num(), a.as_num()) {
                        entry.near_tie = (av - ev).abs() <= ev.abs() * NEAR_TIE_REL;
                    }
                    r.mismatch += 1;
                }
            }
        }
        r.entries.push(entry);
    }
    r
}

pub fn is_parity(report: &CompareReport, strict: bool) -> bool {
    report.mismatch == 0 && report.extra == 0 && (!strict || report.unimplemented == 0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetaDiff {
    pub field: &'static str,
    pub expected: Option<serde_json::Value>,
    pub actual: Option<serde_json::Value>,
}

/// The `_meta` fields in format.ts `META_FIELDS` order (camelCase as on the wire).
pub const META_FIELDS: [&str; 7] = [
    "allocNodes",
    "items",
    "socketGroups",
    "mainSocketGroup",
    "treeVersion",
    "className",
    "ascendName",
];

/// `_meta` field by field, strictly (O7); a field missing on the candidate side is a diff.
pub fn compare_meta(reference: &Meta, candidate: Option<&PartialMeta>) -> Vec<MetaDiff> {
    let to_map = |v: serde_json::Value| match v {
        serde_json::Value::Object(m) => m,
        _ => unreachable!("meta serializes to an object"),
    };
    let expected = to_map(serde_json::to_value(PartialMeta::from(reference)).expect("meta serializes"));
    let actual = to_map(serde_json::to_value(candidate.cloned().unwrap_or_default()).expect("meta serializes"));
    META_FIELDS
        .iter()
        .filter_map(|&field| {
            let (e, a) = (expected.get(field).cloned(), actual.get(field).cloned());
            (e != a).then_some(MetaDiff {
                field,
                expected: e,
                actual: a,
            })
        })
        .collect()
}

pub fn summary_line(report: &CompareReport, strict: bool) -> String {
    format!(
        "keys {} | ok {} (exact {}) | mismatch {} | unimplemented {} | extra {} -> {}",
        report.total,
        report.ok,
        report.exact,
        report.mismatch,
        report.unimplemented,
        report.extra,
        if is_parity(report, strict) { "PARITY" } else { "FAIL" }
    )
}

fn show(v: &Option<StatValue>) -> String {
    v.as_ref().map_or_else(|| "(absent)".to_owned(), ToString::to_string)
}

fn describe_mismatch(e: &StatEntry) -> String {
    let mut s = format!("{}: expected={} actual={}", e.key, show(&e.expected), show(&e.actual));
    if let (Some(ev), Some(av)) = (
        e.expected.as_ref().and_then(StatValue::as_num),
        e.actual.as_ref().and_then(StatValue::as_num),
    ) {
        let delta = av - ev;
        let _ = write!(s, "  round4 {:?} vs {:?}  delta={delta:?}", round4(ev), round4(av));
        if ev != 0.0 {
            let _ = write!(s, " ({:.3e}%)", delta / ev * 100.0);
        }
        if e.near_tie {
            s.push_str("  [round4 tie, O8]");
        }
    }
    s
}

/// Text report: summary, mismatch/extra lists with deltas, compact unimplemented list, `_meta` diffs (compare.ts `renderReport`).
pub fn render_report(report: &CompareReport, title: &str, strict: bool, limit: usize, meta_diffs: &[MetaDiff]) -> String {
    let mut out = format!("== {title} ==\n{}\n", summary_line(report, strict));
    let mut list = |cls: StatClass, name: &str, describe: &dyn Fn(&StatEntry) -> String| {
        let items: Vec<&StatEntry> = report.entries.iter().filter(|e| e.cls == cls).collect();
        if items.is_empty() {
            return;
        }
        let _ = writeln!(out, "{name} ({}):", items.len());
        for e in items.iter().take(limit) {
            let _ = writeln!(out, "  {}", describe(e));
        }
        if items.len() > limit {
            let _ = writeln!(out, "  ... and {} more", items.len() - limit);
        }
    };
    list(StatClass::Mismatch, "mismatch", &describe_mismatch);
    list(StatClass::Extra, "extra", &|e| format!("{}: actual={}", e.key, show(&e.actual)));

    let unimpl: Vec<&str> = report
        .entries
        .iter()
        .filter(|e| e.cls == StatClass::Unimplemented)
        .map(|e| e.key.as_str())
        .collect();
    if !unimpl.is_empty() {
        let _ = writeln!(
            out,
            "unimplemented ({}):\n  {}",
            unimpl.len(),
            unimpl[..unimpl.len().min(limit)].join(", ")
        );
        if unimpl.len() > limit {
            let _ = writeln!(out, "  ... and {} more", unimpl.len() - limit);
        }
    }
    if !meta_diffs.is_empty() {
        let _ = writeln!(out, "_meta ({} diffs):", meta_diffs.len());
        for d in meta_diffs {
            let j = |v: &Option<serde_json::Value>| v.as_ref().map_or_else(|| "undefined".to_owned(), ToString::to_string);
            let _ = writeln!(out, "  {}: expected={} actual={}", d.field, j(&d.expected), j(&d.actual));
        }
    }
    out
}
