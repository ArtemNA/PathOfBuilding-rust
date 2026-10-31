//! R5.5c LUT gate: `pob_data::timeless::read_lut` = LuaJIT `data.readLUT(seed, nodeId, jewelType)` (DataLegionLookUpTableHelper.lua:221)
//! for every jewel type 1-6 x three seeds (SeedMin, SeedMax, the middle; Elegant Hubris x20) x every node of `data.nodeIDList`, bit for
//! bit. Fixture: `crates/pob-tree/tests/fixtures/timeless-lut.json` (producer `tools/lua-corpus/export-timeless.ts`, docker LuaJIT with
//! the LUTs mounted, MINEFIELD-RUST R5). R2: the inflated bytes Rust reads from `data/timeless-luts` = the `.bin` the shim handed LuaJIT.

use pob_data::sha256::sha256_hex;
use pob_data::timeless::{lut_bytes, lut_loaded, node_id_list, read_lut, timeless_jewel_type};
use serde_json::Value as Json;

fn fixture() -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../pob-tree/tests/fixtures/timeless-lut.json");
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

#[test]
fn read_lut_equals_luajit_for_every_type_seed_and_node() {
    let f = fixture();
    let ids: Vec<u32> = f["ids"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect();
    let list = node_id_list();
    let mut want_ids: Vec<u32> = list.nodes.keys().copied().collect();
    want_ids.sort_unstable();
    assert_eq!(ids, want_ids, "nodeIDList ids = LuaJIT's");
    let mut total = 0usize;
    for jewel_type in 1..=6usize {
        let t = &f["types"][jewel_type - 1];
        let (name, mn, mx) = timeless_jewel_type(jewel_type).unwrap();
        assert_eq!(
            (
                t["name"].as_str().unwrap(),
                t["seedMin"].as_f64().unwrap(),
                t["seedMax"].as_f64().unwrap()
            ),
            (name, mn, mx)
        );
        let started = std::time::Instant::now();
        let was_loaded = lut_loaded(jewel_type);
        let mut calls = 0usize;
        let mut nonempty = 0usize;
        let mut mismatches = Vec::new();
        for s in t["seeds"].as_array().unwrap() {
            let seed = s["seed"].as_f64().unwrap();
            let rows: Vec<&str> = s["rows"].as_str().unwrap().split(';').collect();
            assert_eq!(rows.len(), ids.len());
            for (id, want) in ids.iter().zip(rows) {
                let got = read_lut(seed, *id, jewel_type).unwrap_or_else(|e| panic!("{name} seed {seed} node {id}: {e}"));
                let got_s = got.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
                calls += 1;
                if !got.is_empty() {
                    nonempty += 1;
                }
                if got_s != want {
                    mismatches.push(format!("{name} seed {seed} node {id}: rust [{got_s}] luajit [{want}]"));
                }
            }
        }
        eprintln!(
            "type {jewel_type} {name}: {calls} readLUT calls ({nonempty} non-empty) = LuaJIT, {:.1} ms incl. first load (loaded before: {was_loaded}); LuaJIT first {} ms",
            started.elapsed().as_secs_f64() * 1e3,
            t["firstReadMs"].as_str().unwrap()
        );
        assert!(
            mismatches.is_empty(),
            "{} mismatches, first: {:?}",
            mismatches.len(),
            &mismatches[..mismatches.len().min(5)]
        );
        assert_eq!(calls, t["calls"].as_u64().unwrap() as usize);
        total += calls;
    }
    assert_eq!(total, 6 * 3 * ids.len());
}

/// MINEFIELD-RUST R2/R5: Rust inflates `data/timeless-luts/<stem>.zip[.partN]` with flate2; the docker probe read the `.bin` that
/// pob-engine-wasm's `unpackJewelData` (node:zlib) wrote. Same bytes, or the LUT gate compares different data.
#[test]
fn inflated_luts_equal_the_bins_luajit_read() {
    let f = fixture();
    for (jewel_type, stem) in [
        "GloriousVanity",
        "LethalPride",
        "BrutalRestraint",
        "MilitantFaith",
        "ElegantHubris",
        "HeroicTragedy",
    ]
    .iter()
    .enumerate()
    {
        let b = lut_bytes(jewel_type + 1).unwrap();
        let want = &f["bins"][stem];
        assert_eq!(b.len() as u64, want["bytes"].as_u64().unwrap(), "{stem} length");
        assert_eq!(sha256_hex(b), want["sha256"].as_str().unwrap(), "{stem} sha256");
    }
}
