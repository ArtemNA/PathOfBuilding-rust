//! The data the Spectre Library reads (5606cc64f: Build.lua:1690-2023, MinionListControl.lua:52-174, GemSelectControl.lua:134).
//! Counts measured on the upstream Lua with grep, not by this converter.

use pob_data::world_areas::world_areas;
use serde_json::Value;

#[test]
fn world_areas_counts() {
    let w = world_areas();
    // `grep -c '^worldAreas\[' src/Data/WorldAreas.lua` at 5606cc64f: 1090; `grep -c '	description = ' ...`: 138; `bossVarieties`: 690
    assert_eq!(w.len(), 1090);
    assert_eq!(w.values().filter(|a| a.description.is_some()).count(), 138);
    assert_eq!(w.values().filter(|a| a.boss_varieties.is_some()).count(), 690);
    let a = &w["1_1_1"];
    assert_eq!((a.name.as_str(), a.level, a.is_map), ("The Twilight Strand (Act 1)", 1.0, false));
    assert_eq!(a.monster_varieties, ["Drowned", "Sand Spitter"]);
    assert_eq!(a.boss_varieties.as_deref(), Some(&["Hillock".to_string()][..]));
}

#[test]
fn spectre_fields_counts() {
    let m: Value = pob_data::read_json("minions.json").expect("minions.json");
    let ms = m["minions"].as_object().expect("minions");
    let spectres: Vec<&Value> = m["spectreIds"]
        .as_array()
        .expect("spectreIds")
        .iter()
        .map(|id| &ms[id.as_str().expect("id")])
        .collect();
    let count = |f: &dyn Fn(&Value) -> bool| spectres.iter().filter(|s| f(s)).count();
    // `grep -c 'recommendedSpectre = true' src/Data/Spectres.lua`: 51 (Minions.lua: 0); `itemisedCorpse = true`: 112
    assert_eq!(count(&|s| s["extraFlags"]["recommendedSpectre"] == Value::Bool(true)), 51);
    assert_eq!(count(&|s| s["itemisedCorpse"] == Value::Bool(true)), 112);
    // every spectre has the fields the library reads unguarded (`minion.baseMovementSpeed / 10`, `#minion.spawnLocation`)
    assert_eq!(
        count(&|s| s["baseMovementSpeed"].is_number() && s["spawnLocation"].is_array()),
        spectres.len()
    );
    let corpse_thief = &ms["Metadata/Monsters/ParasiticSquid/ParasiticSquidAbyssSpectre"];
    assert_eq!(corpse_thief["monsterCategory"], "Beast");
    assert_eq!(corpse_thief["baseMovementSpeed"], 26.0);
    assert_eq!(corpse_thief["spawnLocation"], serde_json::json!(["Ziggurat (Map)"]));
}

#[test]
fn imbued_only_skills() {
    let s = pob_data::skills::skills();
    let ids: Vec<&str> = s.effects.iter().filter(|e| e.imbued_only).map(|e| e.id.as_str()).collect();
    // `grep -c 'imbuedOnly = true' src/Data/Skills/*.lua`: 12, all in other.lua
    assert_eq!(ids.len(), 12, "{ids:?}");
    assert!(ids.contains(&"SupportConstriction"));
}
