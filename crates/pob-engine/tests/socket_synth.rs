//! R-tree-legacy gate: a socket slot holding an item it does not accept, or one that does not exist (ItemSlotControl.lua:105-106 ->
//! `SetSelItemId(0)` :72-79, on TreeTab:Load's SetActiveSpec and TreeTab:PostLoad; pob-calc `populate_socket_slots`), and a build without any
//! item (TreeTab.lua:573): the jewels left in the spec, `allocNodes`, the `<Tree>` of SaveDB byte for byte and the MAIN output bit for bit = LuaJIT.
//! Truth: `fixtures/socket-synth/<scenario>.luajit.json` (`node tools/lua-corpus/export-socket-synth.ts`, two passes).

use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use pob_calc::{CalculateOptions, StatValue};
use pob_engine::save::save_tree_of;
use serde_json::Value as Json;

fn lines<'a>(fx: &'a Json, key: &str) -> Vec<&'a str> {
    fx[key]
        .as_array()
        .map_or_else(Vec::new, |a| a.iter().map(|v| v.as_str().unwrap()).collect())
}

/// `key=value` lines -> map.
fn map<'a>(fx: &'a Json, key: &str) -> BTreeMap<&'a str, &'a str> {
    lines(fx, key).into_iter().map(|l| l.split_once('=').expect("key=value")).collect()
}

#[test]
fn invalid_socket_items_match_luajit() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/socket-synth");
    let mut bad = Vec::new();
    let mut scenarios = 0;
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let name = e.file_name().into_string().unwrap();
        let Some(stem) = name.strip_suffix(".luajit.json") else { continue };
        scenarios += 1;
        let fx: Json = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
        let text = pob_build::read_build_file(dir.join(format!("{stem}.xml"))).unwrap();
        let model = Rc::new(pob_build::load_build_xml(&text).unwrap());
        let opts = CalculateOptions {
            parse_mode: pob_calc::ParseMode::Full,
            skills: true,
            ..CalculateOptions::default()
        };
        let (res, _) = pob_calc::calculate::calculate_model(model.clone(), opts).unwrap();

        let mut jewels: Vec<String> = res
            .env
            .spec
            .jewels
            .0
            .iter()
            .filter(|(_, v)| *v != 0.0)
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        jewels.sort();
        if jewels != lines(&fx, "jewels") {
            bad.push(format!("{stem}: jewels rust {jewels:?} | luajit {:?}", lines(&fx, "jewels")));
        }
        if Some(res.env.spec.alloc_nodes.len() as u64) != fx["allocNodes"].as_u64() {
            bad.push(format!(
                "{stem}: allocNodes rust {} | luajit {}",
                res.env.spec.alloc_nodes.len(),
                fx["allocNodes"]
            ));
        }
        let tree = pob_build::compose_xml(&save_tree_of(&model, &res.env).unwrap());
        if Some(tree.as_str()) != fx["tree"].as_str() {
            bad.push(format!("{stem}: <Tree> differs"));
        }

        let (nums, strs, bools) = (map(&fx, "nums"), map(&fx, "strs"), map(&fx, "bools"));
        for (k, v) in &res.stats {
            let same = match v {
                StatValue::Num(n) => nums.get(k.as_str()).is_some_and(|l| l.parse::<f64>().is_ok_and(|l| l == *n)),
                // `output_to_stats` writes a non-finite number as its text (real_builds.rs `lua_num`)
                StatValue::Str(s) => strs.get(k.as_str()) == Some(&s.as_str()) || nums.get(k.as_str()) == Some(&s.as_str()),
                StatValue::Bool(b) => bools.get(k.as_str()).is_some_and(|l| *l == b.to_string()),
            };
            if !same {
                bad.push(format!("{stem}: {k} rust {v:?}"));
            }
        }
        let n_lua = nums.len() + strs.len() + bools.len();
        if res.stats.len() != n_lua {
            bad.push(format!("{stem}: {} keys in Rust, {n_lua} in LuaJIT", res.stats.len()));
        }
        println!("{stem}: {} keys compared", res.stats.len());
    }
    assert_eq!(scenarios, 4, "the scenarios of export-socket-synth.ts");
    assert!(bad.is_empty(), "{} differences:\n{}", bad.len(), bad.join("\n"));
}

/// Perf: load + first calculation of the builds with a legacy alternate ascendancy, median of 9 after a warm-up (the trees cached, as desktop):
/// `cargo test --release -p pob-engine --test socket_synth -- --ignored --nocapture` (`builds/pc`, local only).
#[test]
#[ignore = "perf"]
fn legacy_load_perf() {
    let pc = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../builds/pc");
    for name in ["build-k", "build-l", "build-m", "build-n"] {
        let Ok(text) = pob_build::read_build_file(pc.join(format!("{name}.xml"))) else {
            println!("{name}: absent");
            continue;
        };
        let mut ms: Vec<f64> = (0..10)
            .map(|_| {
                let t = std::time::Instant::now();
                let model = Rc::new(pob_build::load_build_xml(&text).unwrap());
                let opts = CalculateOptions {
                    parse_mode: pob_calc::ParseMode::Full,
                    skills: true,
                    ..CalculateOptions::default()
                };
                std::hint::black_box(pob_calc::calculate::calculate_model(model, opts).unwrap());
                t.elapsed().as_secs_f64() * 1000.0
            })
            .skip(1)
            .collect();
        ms.sort_by(f64::total_cmp);
        println!("{name}: median {:.1} ms, min {:.1}", ms[4], ms[0]);
    }
}
