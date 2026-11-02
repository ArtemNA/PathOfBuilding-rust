//! R-tattoo gate: the `<Tree>` of SaveDB (TreeTab:Save -> PassiveSpec:Save, PassiveSpec.lua:197-265 -- `<Overrides>` included) = LuaJIT on
//! the user's builds with tattoos (`builds/pc`, local only). Truth: `<slug>.tree-save.json` of `node tools/lua-corpus/export-real.ts tree-save`
//! (LuaJIT: desktop text mode -> loadBuildFromXML -> `build.treeTab:Save` -> `xml.ComposeXML`), read from `POB_TATTOO_SAVE_DIR`, else
//! `builds/pc-fixtures`. Rust: the same text through `pob_calc::calculate_model` -> [`pob_engine::save::save_tree_of`] -> `compose_xml`.
//!
//! Gated: every `<Overrides>` block (each spec's, in order) byte for byte, and the whole `<Tree>` byte for byte except the lines of two
//! recorded boundaries outside the tattoos, which are listed: `secondaryAscendClassId` of a tree whose only alternate ascendancies are the
//! legacy ones (PassiveTree.lua:132-176 sets `alternate_ascendancies = nil` -> `"nil"`; pob-tree keeps them -> `"0"`, the
//! `PassiveTree.lua:131-172` P5 note) and a `<Socket>` of a jewel in a legacy cluster-subgraph socket (real_builds.rs `CLUSTER_SOCKET`).
//! Without fixtures the test is skipped with a message (other machines).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::CalculateOptions;
use pob_engine::save::save_tree_of;
use serde_json::Value as Json;

fn builds() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../builds")
}

fn fixtures_dir() -> PathBuf {
    std::env::var_os("POB_TATTOO_SAVE_DIR").map_or_else(|| builds().join("pc-fixtures"), PathBuf::from)
}

/// The `<Overrides>...</Overrides>` blocks (or `<Overrides/>`) of a composed `<Tree>`, in order.
fn overrides(xml: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut cur: Option<String> = None;
    for l in xml.lines() {
        let t = l.trim_start();
        if let Some(c) = cur.as_mut() {
            c.push_str(l);
            c.push('\n');
            if t == "</Overrides>" {
                v.extend(cur.take());
            }
        } else if t == "<Overrides/>" {
            v.push(format!("{l}\n"));
        } else if t == "<Overrides>" {
            cur = Some(format!("{l}\n"));
        }
    }
    v
}

/// The lines of a composed XML without its `<Socket>` lines.
fn no_socket<'a>(v: &[&'a str]) -> Vec<&'a str> {
    v.iter().copied().filter(|l| !l.trim_start().starts_with("<Socket ")).collect()
}

/// The recorded non-tattoo boundary a differing `(rust, luajit)` line pair belongs to, if any.
fn boundary(r: &str, l: &str) -> Option<&'static str> {
    let t = (r.trim_start(), l.trim_start());
    if t.0.starts_with("<Spec ") && r.replace("secondaryAscendClassId=\"0\"", "secondaryAscendClassId=\"nil\"") == l {
        return Some("secondaryAscendClassId 0 vs nil (legacy alternate ascendancies, PassiveTree.lua:132-176, pob-tree P5)");
    }
    if t.0.starts_with("<Socket ") && t.1.starts_with("<Socket ") {
        return Some("<Socket> of a legacy cluster-subgraph socket (real_builds.rs CLUSTER_SOCKET)");
    }
    None
}

#[test]
fn tattoo_tree_save_matches_luajit() {
    let Ok(dir) = std::fs::read_dir(fixtures_dir()) else {
        println!("tattoo_save: no {} -- skipped (local only)", fixtures_dir().display());
        return;
    };
    let mut names: Vec<String> = dir
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".tree-save.json"))
        .collect();
    names.sort();
    if names.is_empty() {
        println!(
            "tattoo_save: no *.tree-save.json in {} -- skipped (local only)",
            fixtures_dir().display()
        );
        return;
    }
    let (mut equal, mut with_boundary, mut overrides_n) = (0usize, Vec::new(), 0usize);
    let mut bad = Vec::new();
    for name in &names {
        let fx: Json = serde_json::from_str(&std::fs::read_to_string(fixtures_dir().join(name)).expect("fixture")).expect("json");
        let rel = fx["build"].as_str().expect("build");
        let set = fx["set"].as_str().unwrap_or("pc");
        let Some(lj) = fx["tree"].as_str() else {
            println!("{rel}: LuaJIT did not save ({:?}) -- not gated", fx["loadError"]);
            continue;
        };
        let text = pob_build::read_build_file(builds().join(set).join(rel)).expect("build file");
        if fx["xmlSha256"].as_str() != Some(pob_data::sha256::sha256_hex(text.as_bytes()).as_str()) {
            bad.push(format!("{rel}: stale fixture (the build changed since the probe)"));
            continue;
        }
        let rust = (|| -> Result<String, String> {
            let model = Rc::new(pob_build::load_build_xml(&text).map_err(|e| format!("XML: {e}"))?);
            let (res, _) = pob_calc::calculate::calculate_model(model.clone(), CalculateOptions::default()).map_err(|e| e.0)?;
            Ok(pob_build::compose_xml(&save_tree_of(&model, &res.env)?))
        })();
        let r = match rust {
            Ok(r) => r,
            Err(e) => {
                bad.push(format!("{rel}: rust Err: {e}"));
                continue;
            }
        };
        let (ro, lo) = (overrides(&r), overrides(lj));
        overrides_n += lo.iter().map(|b| b.matches("<Override ").count()).sum::<usize>();
        if ro != lo {
            bad.push(format!("{rel}: <Overrides> differ: rust {ro:?} | luajit {lo:?}"));
            continue;
        }
        if r == lj {
            equal += 1;
            continue;
        }
        let (mut rl, mut ll): (Vec<&str>, Vec<&str>) = (r.lines().collect(), lj.lines().collect());
        let mut why: Vec<&str> = Vec::new();
        if rl.len() != ll.len() {
            // a jewel LuaJIT drops from a legacy cluster-subgraph socket (and Rust keeps): compare without the <Socket> lines
            let (r2, l2) = (no_socket(&rl), no_socket(&ll));
            if r2.len() != l2.len() {
                bad.push(format!("{rel}: <Tree> has {} lines in Rust, {} in LuaJIT", rl.len(), ll.len()));
                continue;
            }
            why.push("<Socket> of a legacy cluster-subgraph socket (real_builds.rs CLUSTER_SOCKET)");
            (rl, ll) = (r2, l2);
        }
        let mut unexplained = None;
        for (i, (a, b)) in rl.iter().zip(&ll).enumerate() {
            if a != b {
                match boundary(a, b) {
                    Some(w) => {
                        if !why.contains(&w) {
                            why.push(w);
                        }
                    }
                    None => {
                        unexplained = Some(format!("{rel}: line {}: rust {a:?} | luajit {b:?}", i + 1));
                        break;
                    }
                }
            }
        }
        match unexplained {
            Some(u) => bad.push(u),
            None => with_boundary.push(format!("{rel}: {}", why.join("; "))),
        }
    }
    println!(
        "--- <Tree> = LuaJIT except recorded non-tattoo boundaries ({}) ---",
        with_boundary.len()
    );
    for w in &with_boundary {
        println!("{w}");
    }
    println!("--- divergent ({}) ---", bad.len());
    for b in &bad {
        println!("{b}");
    }
    println!(
        "tattoo_save: {} builds ({overrides_n} <Override>s); <Overrides> = LuaJIT on {}; whole <Tree> byte for byte {equal}, with recorded boundaries {}; divergent {}",
        names.len(),
        names.len() - bad.len(),
        with_boundary.len(),
        bad.len()
    );
    assert!(bad.is_empty(), "{} builds: Rust <Tree> != LuaJIT", bad.len());
}
