//! Every `data-patches/*.json` is taken for the data of `data.lock` (`manifestSha256` and `producer.pobSha`).
//!
//! The loaders check this only when a patch is first read (`patches.rs`, `tattoo.rs`, `tree.rs`), and some are read only by
//! some builds: 0.2.1 shipped `tattoo-passives.json` of the 2.68.0 data, so every build with a tattoo or a Runegraft failed to
//! load, while the tests that ran were green. This test reads them all, whatever a build needs.

use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn json(path: &std::path::Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn every_data_patch_is_of_the_locked_data() {
    let lock = json(&repo().join("data.lock"));
    let want_sha = lock["manifestSha256"].as_str().expect("data.lock manifestSha256");
    let want_pob = lock["pobSha"].as_str().expect("data.lock pobSha");
    let mut files: Vec<PathBuf> = std::fs::read_dir(repo().join("data-patches"))
        .expect("data-patches")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no data-patches/*.json");
    let mut stale = Vec::new();
    for p in &files {
        let v = json(p);
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        let sha = v["manifestSha256"].as_str();
        let pob = v["producer"]["pobSha"].as_str();
        // a patch without the header is not keyed to the data (none such today): list it, so a new file is looked at
        if sha != Some(want_sha) || pob != Some(want_pob) {
            stale.push(format!("{name}: manifestSha256 {sha:?}, pobSha {pob:?}"));
        }
    }
    println!("data-patches: {} files, locked {want_sha} / {want_pob}", files.len());
    assert!(
        stale.is_empty(),
        "data-patches not taken for data.lock ({want_sha} / {want_pob}); re-take them (tools/retake-fixtures.ps1 -DataPatches):\n{}",
        stale.join("\n")
    );
}
