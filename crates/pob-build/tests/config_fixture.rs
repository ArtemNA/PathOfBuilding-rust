//! R3.5 gate on the 6 goldens (R3-ARCH §3), fixtures from R3.1 (`crates/pob-calc/tests/fixtures/<build>.{pobts,luajit}.json`,
//! key `config` = the pob-ts probe `test/config/oracle.test.ts` `DUMP` :31-50): the Rust dump in the same shape must equal
//! the pob-ts dump AND the LuaJIT dump exactly -- mod lists as sequences (order = varList order, L10), `input`/`placeholder`
//! key for key, numbers bit for bit. Normalisation (dkjson, R3.1): an absent key, `null` and an empty table are the same.

use std::time::Instant;

use pob_build::{BuildModel, ConfigState, ConfigValue, load_build_xml};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_tree::modtools::format_source_mod;
use serde_json::{Map, Value as Json, json};

fn fixture(name: &str, kind: &str) -> Json {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../pob-calc/tests/fixtures");
    let path = dir.join(format!("{}.{kind}.json", name.replace(' ', "-")));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut j: Json = serde_json::from_str(&text).unwrap();
    j["config"].take()
}

fn model(name: &str) -> BuildModel {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).unwrap();
    load_build_xml(&std::fs::read_to_string(g.xml_path()).unwrap()).unwrap()
}

fn val(v: &ConfigValue) -> Json {
    match v {
        ConfigValue::Num(n) => json!(n),
        ConfigValue::Str(s) => json!(s),
        ConfigValue::Bool(b) => json!(b),
    }
}

fn table(t: &indexmap::IndexMap<String, ConfigValue>) -> Json {
    Json::Object(t.iter().map(|(k, v)| (k.clone(), val(v))).collect::<Map<_, _>>())
}

/// `DUMP` of oracle.test.ts.
fn dump(m: &BuildModel, state: &ConfigState, lists: &pob_build::ConfigModLists) -> Json {
    let fmt = |l: &pob_mods::ModList| Json::Array(l.mods.iter().map(|x| json!(format_source_mod(x))).collect());
    let mut o = Map::new();
    o.insert("mods".into(), fmt(&lists.mod_list));
    o.insert("enemyMods".into(), fmt(&lists.enemy_mod_list));
    o.insert("enemyLevel".into(), json!(state.enemy_level));
    o.insert("input".into(), table(&state.input));
    o.insert("placeholder".into(), table(&state.placeholder));
    o.insert("characterLevel".into(), json!(m.level));
    o.insert("buildPantheonMajorGod".into(), json!(m.pantheon_major_god));
    for (k, var) in [
        ("inputPantheonMajorGod", "pantheonMajorGod"),
        ("inputPantheonMinorGod", "pantheonMinorGod"),
        ("inputBandit", "bandit"),
    ] {
        if let Some(v) = state.input.get(var) {
            o.insert(k.into(), val(v));
        }
    }
    Json::Object(o)
}

fn is_empty(j: Option<&Json>) -> bool {
    match j {
        None | Some(Json::Null) => true,
        Some(Json::Array(a)) => a.is_empty(),
        Some(Json::Object(o)) => o.is_empty(),
        _ => false,
    }
}

/// Exact equality (numbers bit for bit, sequences in order); every difference as a path.
fn diff(path: &str, want: &Json, got: &Json, out: &mut Vec<String>) {
    match (want, got) {
        (Json::Number(a), Json::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            if a.to_bits() != b.to_bits() {
                out.push(format!("{path}: want {a:?}, got {b:?}"));
            }
        }
        (Json::Array(a), Json::Array(b)) => {
            if a.len() != b.len() {
                out.push(format!("{path}: want {} entries, got {}", a.len(), b.len()));
            }
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                diff(&format!("{path}[{i}]"), x, y, out);
            }
        }
        (Json::Object(a), Json::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let (x, y) = (a.get(k), b.get(k));
                if is_empty(x) && is_empty(y) {
                    continue;
                }
                match (x, y) {
                    (Some(x), Some(y)) => diff(&format!("{path}.{k}"), x, y, out),
                    _ => out.push(format!("{path}.{k}: want {x:?}, got {y:?}")),
                }
            }
        }
        _ if is_empty(Some(want)) && is_empty(Some(got)) => {}
        _ => {
            if want != got {
                out.push(format!("{path}: want {want}, got {got}"));
            }
        }
    }
}

/// pob-ts oracle.test.ts `EXPECTED_COUNTS` / `EXPECTED_IGNORED_LEGACY`.
const EXPECTED: [(&str, [usize; 2], &[&str]); 6] = [
    ("Dual Savior", [36, 17], &[]),
    ("Dual Wield Cospris CoC", [44, 21], &["igniteMode"]),
    ("Dual Wield Cospris CoC - Lethal Pride", [44, 21], &["igniteMode"]),
    ("Generals Perforate Zerker", [62, 21], &["enemyAwakeningLevel"]),
    ("Mirage Archer Toxic Rain", [47, 20], &["enemyAwakeningLevel"]),
    ("OccVortex", [39, 22], &["brandAttachedToEnemy"]),
];

#[test]
fn config_dump_equals_pobts_and_luajit_on_six_goldens() {
    assert_eq!(GOLDEN_BUILDS.len(), 6);
    let mut problems = Vec::new();
    for g in GOLDEN_BUILDS.iter() {
        let m = model(g.name);
        let mut state = ConfigState::from_build(&m);
        let lists = state.build_mod_list(m.level).unwrap();
        let got = dump(&m, &state, &lists);
        for kind in ["pobts", "luajit"] {
            let mut d = Vec::new();
            diff(&format!("{} {kind}", g.name), &fixture(g.name, kind), &got, &mut d);
            problems.extend(d);
        }
        let (_, counts, ignored) = EXPECTED.iter().find(|(n, ..)| *n == g.name).unwrap();
        let n = [lists.mod_list.mods.len(), lists.enemy_mod_list.mods.len()];
        println!("{}: [mods, enemyMods] = {n:?}, enemyLevel {}", g.name, state.enemy_level);
        assert_eq!(&n, counts, "{}: mod counts", g.name);
        assert!(
            lists.unimplemented.is_empty(),
            "{}: unimplemented {:?}",
            g.name,
            lists.unimplemented
        );
        assert!(lists.warnings.is_empty(), "{}: warnings {:?}", g.name, lists.warnings);
        assert_eq!(lists.ignored_legacy, *ignored, "{}: ignored legacy vars", g.name);
        // D17: <Build pantheonMajorGod="Arakaali"> is lost because the file has a <Config>.
        assert!(m.has_config_section);
        assert_eq!(state.input.get("pantheonMajorGod"), Some(&ConfigValue::Str("None".into())));
        if g.name == "Mirage Archer Toxic Rain" {
            assert_eq!(m.pantheon_major_god, "Arakaali");
        }
    }
    assert!(problems.is_empty(), "{} mismatches:\n{}", problems.len(), problems.join("\n"));
}

/// The branches the goldens never reach (pob-ts oracle.test.ts `BRANCH_SWEEP`, replayed like its `setConfig` RPC:
/// `input[id] = value; BuildModList()` on one accumulating state). No oracle fixture exists for these in pob-rust, so this
/// only proves every closure runs without a Lua error, unimplemented entry or warning, and spot-checks the tier mods.
#[test]
fn branch_sweep_runs_every_tier() {
    let m = model("Dual Savior");
    let mut state = ConfigState::from_build(&m);
    state.build_mod_list(m.level).unwrap();
    let sweep: [(&str, ConfigValue); 13] = [
        ("enemyIsBoss", ConfigValue::Str("None".into())),
        ("enemyIsBoss", ConfigValue::Str("Boss".into())),
        ("enemyIsBoss", ConfigValue::Str("Uber".into())),
        ("enemyIsBoss", ConfigValue::Str("Pinnacle".into())),
        ("conditionEnemyRareOrUnique", ConfigValue::Bool(true)),
        ("deliriousPercentage", ConfigValue::Str("20Percent".into())),
        ("deliriousPercentage", ConfigValue::Str("100Percent".into())),
        ("enemyPhysicalReduction", ConfigValue::Num(25.0)),
        ("enemyMaxResist", ConfigValue::Bool(true)),
        ("enemyBlockChance", ConfigValue::Num(40.0)),
        ("enemyMultiplierPvpDamage", ConfigValue::Num(150.0)),
        ("presetBossSkills", ConfigValue::Str("Atziri Flameblast".into())),
        ("presetBossSkills", ConfigValue::Str("None".into())),
    ];
    for (id, v) in sweep {
        state.input.insert(id.into(), v.clone());
        let lists = state.build_mod_list(m.level).unwrap_or_else(|e| panic!("{id}={v:?}: {e}"));
        assert!(
            lists.unimplemented.is_empty() && lists.warnings.is_empty(),
            "{id}: {:?} {:?}",
            lists.unimplemented,
            lists.warnings
        );
        let enemy: Vec<String> = lists.enemy_mod_list.mods.iter().map(|x| format_source_mod(x)).collect();
        match (id, &v) {
            ("enemyIsBoss", ConfigValue::Str(t)) if t == "Uber" => {
                assert!(enemy.contains(&"-70|Boss|DamageTaken|MORE|-|-|-".to_owned()), "{enemy:?}");
                assert_eq!(state.enemy_level, 85.0);
            }
            ("enemyIsBoss", ConfigValue::Str(t)) if t == "None" => {
                assert!(!state.placeholder.contains_key("enemyLightningResist"));
            }
            ("presetBossSkills", ConfigValue::Str(t)) if t == "Atziri Flameblast" => {
                // Pinnacle + earlierUber -> uber penetration 10 (data/boss-skills.json).
                assert_eq!(state.placeholder.get("enemyFirePen"), Some(&ConfigValue::Num(10.0)));
                assert_eq!(state.input.get("enemyDamageType"), Some(&ConfigValue::Str("Spell".into())));
                assert!(!state.control_enabled("enemyDamageType"));
            }
            ("presetBossSkills", ConfigValue::Str(t)) if t == "None" => {
                assert_eq!(state.input.get("enemyDamageType"), Some(&ConfigValue::Str("Average".into())));
                assert!(state.control_enabled("enemyDamageType"));
            }
            _ => {}
        }
    }
}

/// `set_placeholder` = EditControl `tostring` -> ConfigTab callback `tonumber(buf)`: "" deletes, numbers round-trip
/// through `%.14g`, non-edit controls only warn.
#[test]
fn set_placeholder_dance() {
    let mut s = ConfigState::new();
    let mut w = Vec::new();
    s.set_placeholder("enemyArmour", &ConfigValue::Num(1.0 / 3.0), &mut w);
    assert_eq!(s.placeholder.get("enemyArmour"), Some(&ConfigValue::Num(0.33333333333333)));
    s.set_placeholder("enemyArmour", &ConfigValue::Str(String::new()), &mut w);
    assert!(!s.placeholder.contains_key("enemyArmour"));
    s.set_placeholder("enemyArmour", &ConfigValue::Str("12".into()), &mut w);
    assert_eq!(s.placeholder.get("enemyArmour"), Some(&ConfigValue::Num(12.0)));
    assert!(w.is_empty());
    s.set_placeholder("enemyIsBoss", &ConfigValue::Num(1.0), &mut w);
    assert_eq!(w.len(), 1);
    // UpdateLevel: input 0 falls through to the placeholder (L4 + `> 0`), capped by MaxEnemyLevel.
    s.input.insert("enemyLevel".into(), ConfigValue::Num(0.0));
    s.placeholder.insert("enemyLevel".into(), ConfigValue::Num(99.0));
    s.update_level(10.0).unwrap();
    assert_eq!(s.enemy_level, 85.0);
    s.placeholder.shift_remove("enemyLevel");
    s.update_level(10.0).unwrap();
    assert_eq!(s.enemy_level, 10.0);
}

/// Legacy `input.customMods` (ConfigTab.lua:1188-1202) and custom blocks: parseMod hits only, source `Custom:<title>`.
#[test]
fn custom_mods_blocks() {
    let mut s = ConfigState::new();
    s.custom_mods_list[0].text = "^7  10% increased Damage  \nnot a mod line\n".into();
    s.custom_mods_list[0].title = "Mine".into();
    let l = s.build_mod_list(90.0).unwrap();
    let custom: Vec<String> = l
        .mod_list
        .mods
        .iter()
        .map(|x| format_source_mod(x))
        .filter(|x| x.contains("|Custom"))
        .collect();
    assert_eq!(custom, ["10|Custom:Mine|Damage|INC|-|-|-"]);
    s.custom_mods_list[0].text.clear();
    s.input.insert("customMods".into(), ConfigValue::Str("10% increased Damage".into()));
    let l = s.build_mod_list(90.0).unwrap();
    assert!(l.mod_list.mods.iter().any(|x| format_source_mod(x) == "10|Custom|Damage|INC|-|-|-"));
}

/// Perf: `build_mod_list` per golden, median of 9 runs on a fresh copy of the loaded state.
#[test]
fn perf_build_mod_list() {
    for g in GOLDEN_BUILDS.iter() {
        let m = model(g.name);
        let base = ConfigState::from_build(&m);
        base.clone().build_mod_list(m.level).unwrap(); // warm the data OnceLocks
        let mut times: Vec<f64> = (0..9)
            .map(|_| {
                let mut s = base.clone();
                let t = Instant::now();
                let l = s.build_mod_list(m.level).unwrap();
                let us = t.elapsed().as_secs_f64() * 1e6;
                std::hint::black_box(l);
                us
            })
            .collect();
        times.sort_by(f64::total_cmp);
        println!(
            "perf build_mod_list {}: median {:.1} us (min {:.1}, max {:.1})",
            g.name, times[4], times[0], times[8]
        );
    }
}
