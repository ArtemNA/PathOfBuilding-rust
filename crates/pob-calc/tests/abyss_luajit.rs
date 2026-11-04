//! R-abyss-conquest gate: the Abyss conquests of `BuildAllDependsAndPaths` (PassiveSpec.lua:1133-1150, :1277-1293) = LuaJIT, bit for bit.
//! Fixture `tests/fixtures/abyss-synth.json` (producer `tools/sg-oracle/lua/abyss-probe.lua`: the whole original PoB, desktop `lua51.dll`), over a
//! public golden with Abyss jewels in its tree sockets (`tools/sg-oracle/scenes/tree/abyss-synth.json`): every conquered spec node (seed, conqueror,
//! components, type, dn, sd, modKey, modList in list order, keystoneMod) and every scalar of `buildOutput(MAIN)`.

use std::collections::BTreeMap;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_build::{active_spec, load_build_xml};
use pob_calc::init_env::{item_options, post_load_spec};
use pob_calc::{CalculateOptions, ParseMode, StatValue};
use pob_items::Item;
use pob_lua::format::format_g;
use pob_tree::modtools::format_source_mod;
use pob_tree::{ConquerorId, PassiveSpec};
use serde_json::{Value as Json, json};

fn repo_path(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

fn read_json(rel: &str) -> Json {
    let p = repo_path(rel);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

/// `scenarioXml` of abyss-probe.lua.
fn scenario_xml(golden: &str, sc: &Json) -> String {
    let mut x = golden.to_owned();
    if let Some(t) = sc["tree"].as_str() {
        x = x.replacen("treeVersion=\"3_13\"", &format!("treeVersion=\"{t}\""), 1);
    }
    for j in sc["items"].as_array().into_iter().flatten() {
        let open = format!("<Item id=\"{}\"", j["id"]);
        let a = x.find(&open).unwrap_or_else(|| panic!("no {open}"));
        let gt = a + x[a..].find('>').expect(">");
        let e = gt + x[gt..].find("</Item>").expect("</Item>");
        x = format!("{}\n{}\n{}", &x[..=gt], j["text"].as_str().expect("text"), &x[e..]);
    }
    for s in sc["sockets"].as_array().into_iter().flatten() {
        let from = format!("<Socket nodeId=\"{}\" itemId=\"{}\"/>", s["node"], s["from"]);
        assert!(x.contains(&from), "no {from}");
        x = x.replacen(&from, &format!("<Socket nodeId=\"{}\" itemId=\"{}\"/>", s["node"], s["to"]), 1);
    }
    x
}

fn spec_of(xml: &str) -> PassiveSpec {
    let model = load_build_xml(xml).unwrap();
    let mut items = IndexMap::new();
    for im in &model.items {
        let item = Item::parse(&im.raw, &item_options(im, ParseMode::Full));
        if item.base.is_some() {
            items.insert(im.id as u64, Rc::new(item));
        }
    }
    let mut spec = PassiveSpec::load_mode(active_spec(&model), ParseMode::Full).unwrap();
    post_load_spec(&mut spec, &items).unwrap();
    spec
}

/// The spec dump of abyss-probe.lua `dumpScenario`.
fn node_dump(spec: &PassiveSpec) -> Vec<Json> {
    let mut nodes: Vec<_> = spec.nodes.iter().filter(|n| n.conquered_by.is_some()).collect();
    nodes.sort_by_key(|n| n.id);
    nodes
        .iter()
        .map(|n| {
            let c = n.conquered_by.as_ref().expect("filtered");
            let cid = match &c.conqueror_id {
                ConquerorId::Num(v) => format_g(*v, 14),
                ConquerorId::Str(s) => s.clone(),
            };
            let mut cb = json!({ "id": format_g(c.id, 17), "type": c.conqueror_type, "cid": cid });
            if let Some(m) = &c.modification {
                let comps: Vec<Json> = m
                    .iter()
                    .map(|p| {
                        let rolls: Vec<String> = p.rolls.iter().map(|r| format_g(*r, 17)).collect();
                        json!({ "t": p.ty, "id": p.id, "rolls": rolls })
                    })
                    .collect();
                cb["mod"] = json!(comps);
            }
            let mods: Vec<String> = n.mod_list().mods.iter().map(|m| format_source_mod(m)).collect();
            let mut r = json!({
                "id": n.id, "alloc": n.alloc, "type": n.ty().as_str(), "dn": n.dn(), "sd": n.sd(), "mods": mods, "modKey": n.mod_key_str(), "cb": cb,
            });
            if let Some(a) = n.ascendancy_name() {
                r["asc"] = json!(a);
            }
            if let Some(k) = n.keystone_mod() {
                r["ks"] = json!(format_source_mod(k));
            }
            r
        })
        .collect()
}

/// A fixture's node without what the Rust dump does not model.
fn fixture_node(o: &Json) -> Json {
    let mut o = o.clone();
    o.as_object_mut().expect("node").remove("sprite");
    o
}

/// Number bits as `real_builds`: `%.17g` round-trips a double, a text keeps nan and inf.
fn output_diffs(key: &str, want: &Json, got: &BTreeMap<String, StatValue>) -> Vec<String> {
    let mut lua: BTreeMap<String, String> = BTreeMap::new();
    let empty = serde_json::Map::new();
    for (k, v) in want["nums"].as_object().unwrap_or(&empty) {
        lua.insert(k.clone(), v.as_str().expect("num").to_owned());
    }
    for (k, v) in want["strs"].as_object().unwrap_or(&empty) {
        lua.insert(k.clone(), format!("s:{}", v.as_str().expect("str")));
    }
    for (k, v) in want["bools"].as_object().unwrap_or(&empty) {
        lua.insert(k.clone(), format!("b:{}", v.as_bool().expect("bool")));
    }
    let mut out = Vec::new();
    for (k, l) in &lua {
        let r = match got.get(k) {
            Some(StatValue::Num(n)) => {
                // `lj_strfmt` writes -nan and nan by the sign: the class is what a stat means.
                if n.is_nan() && l.ends_with("nan") {
                    continue;
                }
                format_g(*n, 17)
            }
            Some(StatValue::Str(s)) => format!("s:{s}"),
            Some(StatValue::Bool(b)) => format!("b:{b}"),
            None => "<nil>".to_owned(),
        };
        if &r != l {
            out.push(format!("{key} | {k} | rust {r} | luajit {l}"));
        }
    }
    for k in got.keys() {
        if !lua.contains_key(k) {
            out.push(format!("{key} | {k} | rust extra"));
        }
    }
    out
}

#[test]
fn abyss_conquests_equal_luajit() {
    let f = read_json("crates/pob-calc/tests/fixtures/abyss-synth.json");
    let def = read_json("tools/sg-oracle/scenes/tree/abyss-synth.json");
    let golden_path = pob_golden::registry::pob_root().join(f["golden"].as_str().expect("golden"));
    let golden = std::fs::read_to_string(&golden_path).expect("golden").replace("\r\n", "\n");
    let scenarios = def["scenarios"].as_array().expect("scenarios");
    let builds = f["builds"].as_array().expect("builds");
    assert_eq!(scenarios.len(), builds.len());
    let mut diffs = Vec::new();
    let (mut nodes_total, mut keys_total) = (0, 0);
    for (sc, b) in scenarios.iter().zip(builds) {
        let key = b["key"].as_str().expect("key");
        assert_eq!(sc["key"].as_str(), Some(key));
        let xml = scenario_xml(&golden, sc);
        let t0 = std::time::Instant::now();
        let spec = spec_of(&xml);
        let spec_ms = t0.elapsed().as_secs_f64() * 1e3;
        let got = node_dump(&spec);
        // R13: the line of this node is one of what LuaJIT gives over the processes.
        if def["strhash"]["scenario"].as_str() == Some(key) {
            let node = spec.node(def["strhash"]["node"].as_u64().expect("node") as u32).expect("node");
            let line = &node.sd()[0];
            assert!(
                def["strhash"]["firstLine"].get(line).is_some(),
                "{key}: {line:?} is not among {}",
                def["strhash"]["firstLine"]
            );
        }
        let want: Vec<Json> = b["nodes"].as_array().expect("nodes").iter().map(fixture_node).collect();
        if got.len() != want.len() {
            diffs.push(format!("{key}: conquered nodes: rust {} luajit {}", got.len(), want.len()));
        }
        for (r, l) in got.iter().zip(&want) {
            if r != l {
                diffs.push(format!("{key}: node {}:\n  rust   {r}\n  luajit {l}", l["id"]));
            }
        }
        // R13: a node two jewels share goes to the last in `pairs(self.jewels)`, which is of the process; the file's order is one of them, the fixture a process with it.
        if key == "tecrod-and-zorath" {
            let order = b["jewelsPairs"].as_array().expect("jewelsPairs");
            let at = |id: u64| order.iter().position(|k| k.as_u64() == Some(id)).expect("socket");
            assert!(
                at(26725) < at(55190),
                "{key}: the fixture is not of a process with the file's order"
            );
            assert!(def["strhash"]["jewels"]["26725<55190"].as_u64().is_some_and(|n| n > 0));
            assert!(
                spec.unimplemented.iter().any(|u| u.contains("two Abyss jewels")),
                "{key}: a shared node is a recorded boundary"
            );
        }
        nodes_total += want.len();
        assert!(b["calcError"].is_null(), "{key}: {}", b["calcError"]);
        let t1 = std::time::Instant::now();
        let res = pob_calc::calculate_with(
            &xml,
            CalculateOptions {
                parse_mode: ParseMode::Full,
                skills: true,
                ..CalculateOptions::default()
            },
        )
        .unwrap_or_else(|e| panic!("{key}: {e}"));
        let stats: BTreeMap<String, StatValue> = res.stats.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        keys_total += stats.len();
        diffs.extend(output_diffs(key, &b["output"], &stats));
        eprintln!(
            "{key}: {} conquered nodes, {} output keys; spec {spec_ms:.1} ms, whole calculation {:.1} ms",
            want.len(),
            stats.len(),
            t1.elapsed().as_secs_f64() * 1e3
        );
    }
    assert!(
        diffs.is_empty(),
        "{} differ, first:\n{}",
        diffs.len(),
        diffs[..diffs.len().min(8)].join("\n")
    );
    eprintln!(
        "abyss: {} scenarios, {nodes_total} nodes, {keys_total} output keys = LuaJIT",
        builds.len()
    );
    assert!(nodes_total > 0);
}
