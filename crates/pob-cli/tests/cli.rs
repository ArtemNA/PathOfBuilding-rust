use std::io::Write;
use std::process::{Command, Output, Stdio};

use pob_golden::{CandidateOutput, compare_stats, find_golden, is_parity};

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
fn calc_file_json_is_empty_candidate() {
    let out = Command::new(BIN).arg("calc").arg(dual_savior_xml()).arg("--json").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let parsed = CandidateOutput::from_json(std::str::from_utf8(&out.stdout).unwrap()).unwrap();
    assert!(parsed.stats.is_empty());
    assert_eq!(parsed.unimplemented.len(), 1);
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
    let output = pob_cli::calc("<PathOfBuilding/>").unwrap();
    let report = compare_stats(&golden.stats, &output.stats, &[]);
    assert_eq!(report.unimplemented, 632);
    assert!(is_parity(&report, false));
}
