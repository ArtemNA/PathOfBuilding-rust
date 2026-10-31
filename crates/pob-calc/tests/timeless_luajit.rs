//! R5.5c gate: the conquest half of `BuildAllDependsAndPaths` (PassiveSpec.lua:1110-1406) = LuaJIT on the Lethal Pride golden and
//! on the same XML with the jewel replaced by a synthetic Glorious Vanity / Brutal Restraint / Militant Faith / Elegant Hubris /
//! Heroic Tragedy. Fixture `crates/pob-tree/tests/fixtures/timeless-builds.json` (producer `tools/lua-corpus/export-timeless.ts`, docker
//! LuaJIT with the LUTs mounted): every spec node in `nodesInRadius[jewelRadiusIndex]` of socket 61834 after the load --
//! `conqueredBy` (seed, conqueror type/id, the item's own table), alloc, type, dn, sd, modKey, modList (formatSourceMod, in order),
//! keystoneMod. Rust: the build's items (`ParseMode::Full`), `PassiveSpec::load_mode` + `post_load_spec` (as `init_env`).

use std::rc::Rc;

use indexmap::IndexMap;
use pob_build::{active_spec, load_build_xml};
use pob_calc::ParseMode;
use pob_calc::init_env::{item_options, post_load_spec};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_items::Item;
use pob_lua::format::format_g;
use pob_tree::modtools::format_source_mod;
use pob_tree::{ConquerorId, PassiveSpec};
use serde_json::{Value as Json, json};

fn fixture() -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../pob-tree/tests/fixtures/timeless-builds.json");
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

/// export-timeless.ts `withJewel`: item 3's text from `<Item id="3">` to the next `</Item>` replaced.
fn with_jewel(xml: &str, text: &str) -> String {
    let open = "<Item id=\"3\">";
    let a = xml.find(open).expect("item 3");
    let b = a + xml[a..].find("</Item>").expect("item 3 end");
    format!("{}\n{text}\n{}", &xml[..a + open.len()], &xml[b..])
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

#[test]
fn conquests_equal_luajit() {
    let f = fixture();
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == f["build"].as_str().unwrap()).unwrap();
    let xml = std::fs::read_to_string(g.xml_path()).unwrap();
    let mut summary = Vec::new();
    for b in f["builds"].as_array().unwrap() {
        let key = b["key"].as_str().unwrap();
        let x = match b["jewel"].as_str() {
            Some(text) => with_jewel(&xml, text),
            None => xml.clone(),
        };
        let spec = spec_of(&x);
        let mut diffs = Vec::new();
        let (mut conquered, mut replaced, mut with_additions) = (0, 0, 0);
        for o in b["nodes"].as_array().unwrap() {
            let id = o["id"].as_u64().unwrap() as u32;
            let n = spec.node(id).unwrap_or_else(|| panic!("{key}: node {id} not in the spec"));
            let cb = n.conquered_by.as_ref().map(|c| {
                let cid = match &c.conqueror_id {
                    ConquerorId::Num(v) => format_g(*v, 14),
                    ConquerorId::Str(s) => s.clone(),
                };
                json!({ "id": format_g(c.id, 17), "type": c.conqueror_type, "cid": cid, "same": true })
            });
            let mods: Vec<String> = n.mod_list().mods.iter().map(|m| format_source_mod(m)).collect();
            let mut r = json!({
                "id": id, "alloc": n.alloc, "type": n.ty().as_str(), "dn": n.dn(), "sd": n.sd(), "mods": mods,
                "modKey": n.mod_key_str(),
            });
            if let Some(k) = n.keystone_mod() {
                r["ks"] = json!(format_source_mod(k));
            }
            if let Some(cb) = cb {
                r["cb"] = cb;
                conquered += 1;
            }
            if n.dn() != n.tree_node().dn {
                replaced += 1;
            }
            if n.sd().len() > n.tree_node().sd.len() {
                with_additions += 1;
            }
            if &r != o {
                diffs.push(format!("{key}: node {id}:\n  rust   {r}\n  luajit {o}"));
            }
        }
        assert!(
            diffs.is_empty(),
            "{} nodes differ, first:\n{}",
            diffs.len(),
            diffs[..diffs.len().min(3)].join("\n")
        );
        let nodes = b["nodes"].as_array().unwrap().len();
        eprintln!("{key}: {nodes} nodes in radius = LuaJIT (conquered {conquered}, dn replaced {replaced}, sd grown {with_additions})");
        summary.push((key.to_owned(), nodes, conquered));
    }
    let want: Vec<(String, usize, usize)> = [
        "lethal-pride",
        "glorious-vanity",
        "brutal-restraint",
        "militant-faith",
        "elegant-hubris",
        "heroic-tragedy",
    ]
    .iter()
    .map(|k| ((*k).to_owned(), 96, 96))
    .collect();
    assert_eq!(summary, want);
}
