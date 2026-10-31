use std::io::Write;
use std::process::{Command, Output, Stdio};

use pob_golden::compare::compare_meta;
use pob_golden::{CandidateOutput, GOLDEN_BUILDS, compare_stats, find_golden, is_parity};

const BIN: &str = env!("CARGO_BIN_EXE_pob-cli");

fn dual_savior_xml() -> std::path::PathBuf {
    find_golden("Dual Savior").unwrap().xml_path()
}

fn run_stdin(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn calc_file_json_has_meta_and_no_stats() {
    let out = Command::new(BIN).arg("calc").arg(dual_savior_xml()).arg("--json").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let parsed = CandidateOutput::from_json(std::str::from_utf8(&out.stdout).unwrap()).unwrap();
    assert!(parsed.stats.is_empty());
    assert!(parsed.meta.is_some());
    assert!(parsed.unimplemented.iter().any(|u| u == pob_cli::ENGINE_UNIMPLEMENTED));
    assert!(parsed.unimplemented.iter().any(|u| u.starts_with("CalcSetup.lua:1453-1946 skills")));
}

#[test]
fn calc_stdin_matches_file_byte_for_byte() {
    let path = dual_savior_xml();
    let from_file = Command::new(BIN).arg("calc").arg(&path).arg("--json").output().unwrap();
    let from_stdin = run_stdin(&["calc", "-", "--json"], &std::fs::read(&path).unwrap());
    assert_eq!(from_stdin.status.code(), Some(0));
    assert_eq!(from_file.stdout, from_stdin.stdout);
}

#[test]
fn missing_file_exits_2() {
    let out = Command::new(BIN).args(["calc", "definitely-not-here.xml"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(!out.stderr.is_empty());
}

#[test]
fn empty_stdin_exits_2() {
    let out = run_stdin(&["calc", "-", "--json"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("empty XML"));
}

#[test]
fn usage_and_unknown_args() {
    assert_eq!(Command::new(BIN).output().unwrap().status.code(), Some(2));
    assert_eq!(Command::new(BIN).arg("--help").output().unwrap().status.code(), Some(0));
    assert_eq!(Command::new(BIN).arg("bogus").output().unwrap().status.code(), Some(2));
    assert_eq!(
        Command::new(BIN).args(["calc", "-", "--bogus"]).output().unwrap().status.code(),
        Some(2)
    );
}

#[test]
fn dual_savior_all_unimplemented_but_non_strict_parity() {
    let golden = find_golden("Dual Savior").unwrap().load_snapshot().unwrap();
    let output = pob_cli::calc(&pob_build::read_build_file(dual_savior_xml()).unwrap()).unwrap();
    let report = compare_stats(&golden.stats, &output.stats, &[]);
    assert_eq!(report.unimplemented, 632);
    assert!(is_parity(&report, false));
}

/// R3-ARCH §3 (c): `meta` against `_meta` of the six goldens (`compare_meta`, field by field, O7).
///
/// NOT 0 diffs: two fields cannot match before P5 without faking (pob-ts `metaOf` reports the same numbers):
///  * `allocNodes` = `#build.spec.allocNodes`, which upstream also holds the nodes `BuildClusterJewelGraphs` generates and
///    the expansion sockets it re-adds (D16) -- 4 cluster builds;
///  * `socketGroups` = `#socketGroupList`, which upstream also counts the groups the skill phase appends for item/tree
///    granted skills (CalcSetup.lua:1461-1490) -- 2 builds.
///
/// The diffs are pinned one by one (field, golden value, ours), so any other drift fails.
#[test]
fn meta_against_goldens() {
    type Diffs = &'static [(&'static str, i64, i64)];
    let pinned: [(&str, Diffs); 6] = [
        ("Dual Savior", &[]),
        ("Dual Wield Cospris CoC", &[("allocNodes", 120, 98)]),
        ("Dual Wield Cospris CoC - Lethal Pride", &[("allocNodes", 120, 98)]),
        ("Generals Perforate Zerker", &[("allocNodes", 126, 97)]),
        ("Mirage Archer Toxic Rain", &[("allocNodes", 117, 103), ("socketGroups", 9, 8)]),
        ("OccVortex", &[("socketGroups", 7, 6)]),
    ];
    let mut zero = 0;
    for (g, (name, want)) in GOLDEN_BUILDS.iter().zip(pinned) {
        assert_eq!(g.name, name);
        let snap = g.load_snapshot().unwrap();
        let out = pob_cli::calc(&pob_build::read_build_file(g.xml_path()).unwrap()).unwrap();
        let diffs = compare_meta(&snap.meta, out.meta.as_ref());
        let got: Vec<(&str, i64, i64)> = diffs
            .iter()
            .map(|d| {
                let num = |v: &Option<serde_json::Value>| v.as_ref().and_then(serde_json::Value::as_i64).unwrap_or(-1);
                (d.field, num(&d.expected), num(&d.actual))
            })
            .collect();
        println!("{name}: {got:?}");
        assert_eq!(got, want, "{name}: _meta diffs");
        zero += usize::from(diffs.is_empty());
    }
    assert_eq!(zero, 1, "builds with 0 _meta diffs");
}
