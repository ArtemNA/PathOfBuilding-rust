//! R-stat-describer: the lines `setupFallbackWeights` asks of `data.describeStats` (TreeTab.lua:2006) are computed by the port of
//! `Modules/StatDescriber.lua`. The fixture is what the original answered (`tools/sg-oracle/lua/tree-runner.lua`, step `legionLines`) at the
//! commit it names: it is compared only at that commit, and nothing at run time depends on it.

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use std::collections::BTreeMap;

use pob_data::timeless::{LegionStat, legion_passives};
use pob_data::wire::OrderedMap;
use pob_data::{data_dir, lock_path, verify_lock};
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::tabs::timeless::legion_stat_line;
use serde_json::Value as Json;

type Lines = BTreeMap<String, BTreeMap<String, String>>;

#[test]
fn legion_lines_equal_the_original() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legion_stat_lines.json");
    let fx: Json = serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture")).expect("fixture json");
    let dir = data_dir();
    let lock = verify_lock(&dir, &lock_path(&dir)).expect("data.lock");
    if fx["pobSha"].as_str() != Some(lock.pob_sha.as_str()) {
        println!(
            "the fixture is of PoB {}, data.lock says {}: re-take it to compare",
            fx["pobSha"], lock.pob_sha
        );
        return;
    }
    let tmp = std::env::temp_dir().join(format!("pob-legion-lines-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).expect("tmp");
    let root = scene::pob_root();
    let _launch = Launch::new(LaunchConfig {
        user_path: Some(tmp.to_string_lossy().replace('\\', "/")),
        user_folder: "PoB test".into(),
        script_path: scene::script_path(),
        doc_path: format!("{root}/"),
        version_number: "1.2.3".into(),
        version_branch: None,
        version_platform: None,
        app_version: None,
    });
    let leg = legion_passives().expect("legion");
    let mut got = Lines::new();
    let mut add = |id: &str, sorted: &[String], stats: &OrderedMap<LegionStat>| {
        if sorted.len() > 1 {
            let mut keys: Vec<&String> = stats.keys().collect();
            let mut sorted_keys: Vec<&String> = sorted.iter().collect();
            keys.sort();
            sorted_keys.sort();
            assert_eq!(keys, sorted_keys, "{id}: the keys of `stats` are not its `sortedStats`");
            // A stat with no line is nil in Lua, and the fixture's JSON has no such key (kalguur_notable_24)
            let by_stat = sorted
                .iter()
                .filter_map(|k| legion_stat_line(sorted, k).map(|line| (k.clone(), line)))
                .collect();
            got.insert(id.to_owned(), by_stat);
        }
    };
    for n in &leg.nodes {
        add(&n.id, &n.sorted_stats, &n.stats);
    }
    for a in &leg.additions {
        add(&a.id, &a.sorted_stats, &a.stats);
    }
    let want: Lines = serde_json::from_value(fx["lines"].clone()).expect("lines");
    let lines: usize = want.values().map(BTreeMap::len).sum();
    let mut bad = Vec::new();
    for (id, by_stat) in &want {
        for (k, line) in by_stat {
            let g = got.get(id).and_then(|m| m.get(k));
            if g != Some(line) {
                bad.push(format!("{id} {k}: want {line:?}, got {g:?}"));
            }
        }
    }
    let got_lines: usize = got.values().map(BTreeMap::len).sum();
    println!(
        "legion lines: {} passives, {lines} lines in the fixture, {got_lines} computed, {} differ",
        want.len(),
        bad.len()
    );
    std::fs::remove_dir_all(&tmp).ok();
    assert_eq!(got, want, "{}", bad.join("\n"));
}
