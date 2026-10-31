//! R4.2: `EstimatePlayerProgress` (Build.lua:898-913) in `calculate`, against the R3.8a LuaJIT corpus
//! (`crates/pob-build/tests/fixtures/config-load.json`, docker PoB `loadBuildFromXML`, dump after the whole load). The corpus
//! case `autolevel-true` (`characterLevelAutoMode="true"`, `level="1"`) ends with `characterLevel = 7` in LuaJIT; `load_build`
//! alone keeps 1 (it runs no calc), which `config_load.rs` pins and points here. Every other loading case of the corpus
//! has `characterLevelAutoMode` off and must keep its level.
//!
//! Inputs are the corpus' raw bytes ("as the docker fixture", MINEFIELD-RUST R7).

use pob_calc::calculate;
use serde_json::Value as Json;

fn corpus() -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../pob-build/tests/fixtures/config-load.json");
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("json")
}

fn utf8_of_bytes(s: &str) -> String {
    let b: Vec<u8> = s.chars().map(|c| u8::try_from(u32::from(c)).expect("byte string")).collect();
    String::from_utf8(b).expect("the config corpus inputs are UTF-8")
}

fn lua_level(case: &Json) -> f64 {
    let s = case["result"]["characterLevel"]["num"].as_str().expect("characterLevel");
    pob_lua::tonumber(s).expect("number")
}

#[test]
fn autolevel_true_estimates_level_7_like_luajit() {
    let fx = corpus();
    let case = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "autolevel-true")
        .expect("case");
    let r = calculate(&utf8_of_bytes(case["input"].as_str().unwrap())).expect("calculate");
    assert!(r.env.build.character_level_auto_mode);
    assert_eq!(r.env.build.level, 1.0, "the XML level");
    assert_eq!(lua_level(case), 7.0, "LuaJIT corpus");
    assert_eq!(r.character_level, 7.0, "EstimatePlayerProgress");
    // :913 `configTab:BuildModList()` ran with the new level; the corpus' `enemyLevel` is the state after it.
    assert!(r.config_rebuilt.is_some(), "BuildModList re-ran");
    let enemy_level = pob_lua::tonumber(case["result"]["enemyLevel"]["num"].as_str().unwrap()).unwrap();
    assert_eq!(r.env.config.enemy_level, enemy_level);
    // :697 `buildFlag = false` -- no second calc: the stats are those of the XML level.
    assert_eq!(r.env.player.borrow().level, 1.0);
    println!(
        "autolevel-true: level {} -> {} (LuaJIT {}), ExtraPoints {:?}, allocated {}",
        r.env.build.level,
        r.character_level,
        lua_level(case),
        r.perform.player.output.opt_num("ExtraPoints"),
        r.env.spec.count_alloc_nodes().used
    );
}

/// The other loading cases: no auto mode -> the level is the XML's and nothing is rebuilt (where `calculate` gets through:
/// a corpus case may need skills/items the calc does not have yet; those are counted, not hidden).
#[test]
fn level_kept_without_auto_mode() {
    let fx = corpus();
    let (mut checked, mut failed) = (0, Vec::new());
    for case in fx["cases"].as_array().unwrap() {
        if case["result"].get("prompt").is_some() || case["name"] == "autolevel-true" {
            continue;
        }
        let input = utf8_of_bytes(case["input"].as_str().unwrap());
        match std::panic::catch_unwind(|| calculate(&input)) {
            Ok(Ok(r)) => {
                assert!(!r.env.build.character_level_auto_mode, "{}", case["name"]);
                assert_eq!(r.character_level, r.env.build.level, "{}", case["name"]);
                assert_eq!(r.character_level, lua_level(case), "{}: LuaJIT characterLevel", case["name"]);
                assert!(r.config_rebuilt.is_none());
                checked += 1;
            }
            Ok(Err(e)) => failed.push(format!("{}: {e}", case["name"])),
            Err(_) => failed.push(format!("{}: panic", case["name"])),
        }
    }
    println!("level kept: {checked} cases checked, {} not calculable: {failed:?}", failed.len());
    assert!(checked > 0);
}
