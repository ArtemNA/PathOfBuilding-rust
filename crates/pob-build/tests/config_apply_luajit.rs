//! R3.5b gate: all 539 `apply` closures of ConfigOptions.lua against LuaJIT, closure by closure.
//!
//! Fixture `fixtures/config-apply.json` = `tools/lua-corpus/export-config-apply.ts` (docker LuaJIT, PoB HeadlessWrapper,
//! `newBuild()`): for every var with an `apply` and representative values by type, `apply(val, modList, enemyModList, build)`
//! on fresh lists in the same start state of the config tab, then `formatSourceMod` of both lists (call order; sorted for
//! the bodies that iterate `pairs` over string keys, R13), the Lua error, and the side effects on the tab. Here: the same
//! start state rebuilt from the fixture, the registry entry called, the same dump -- compared exactly (strings byte for
//! byte, numbers bit for bit, mod order included).

use pob_build::config::{APPLY_REGISTRY, ApplyCtx};
use pob_build::{ConfigState, ConfigValue};
use pob_data::config_options::config_options;
use pob_mods::ModList;
use pob_mods::format::format_source_mod;
use serde_json::Value as Json;

#[derive(Clone, Debug, PartialEq)]
enum V {
    Null,
    Num(u64),
    Str(String),
    Bool(bool),
}

fn v_json(j: &Json) -> V {
    match j {
        Json::Null => V::Null,
        Json::Bool(b) => V::Bool(*b),
        Json::String(s) => V::Str(s.clone()),
        Json::Object(o) => {
            let s = o["num"].as_str().expect("num");
            let f: f64 = match s {
                "inf" => f64::INFINITY,
                "-inf" => f64::NEG_INFINITY,
                "nan" | "-nan" => f64::NAN,
                _ => s.parse().unwrap_or_else(|e| panic!("{s}: {e}")),
            };
            V::Num(f.to_bits())
        }
        other => panic!("unexpected {other}"),
    }
}

fn v_cfg(v: Option<&ConfigValue>) -> V {
    match v {
        None => V::Null,
        Some(ConfigValue::Num(n)) => V::Num(n.to_bits()),
        Some(ConfigValue::Str(s)) => V::Str(s.clone()),
        Some(ConfigValue::Bool(b)) => V::Bool(*b),
    }
}

fn cfg_of(j: &Json) -> ConfigValue {
    match v_json(j) {
        V::Num(b) => ConfigValue::Num(f64::from_bits(b)),
        V::Str(s) => ConfigValue::Str(s),
        V::Bool(b) => ConfigValue::Bool(b),
        V::Null => panic!("nil config value"),
    }
}

fn pairs(j: &Json) -> Vec<(String, Json)> {
    j.as_array()
        .unwrap()
        .iter()
        .map(|p| (p[0].as_str().unwrap().to_owned(), p[1].clone()))
        .collect()
}

/// The 1-based index of `val` in the dropdown list of `var`.
fn list_index(var: &str, val: &ConfigValue) -> usize {
    let list = config_options()
        .var(var)
        .and_then(|d| d.list.as_ref())
        .unwrap_or_else(|| panic!("{var}: no list"));
    let pos = list.iter().position(|e| match (&e.val, val) {
        (pob_data::wire::Scalar::Num(a), ConfigValue::Num(b)) => a == b,
        (pob_data::wire::Scalar::Str(a), ConfigValue::Str(b)) => a == b,
        (pob_data::wire::Scalar::Bool(a), ConfigValue::Bool(b)) => a == b,
        _ => false,
    });
    pos.unwrap_or_else(|| panic!("{var}: {val:?} not in list")) + 1
}

/// The tab right after `newBuild()` as the probe snapshotted it.
fn start_state(s: &Json) -> ConfigState {
    let mut st = ConfigState::new();
    st.input = pairs(&s["input"]).into_iter().map(|(k, v)| (k, cfg_of(&v))).collect();
    st.placeholder = pairs(&s["placeholder"]).into_iter().map(|(k, v)| (k, cfg_of(&v))).collect();
    let V::Num(lvl) = v_json(&s["enemyLevel"]) else {
        panic!("enemyLevel")
    };
    st.enemy_level = f64::from_bits(lvl);
    for (var, sel) in pairs(&s["selIndex"]) {
        let V::Num(b) = v_json(&sel) else { panic!("selIndex") };
        st.controls.sel_index.insert(var, f64::from_bits(b) as usize);
    }
    for var in s["disabled"].as_array().unwrap() {
        st.controls.enabled.insert(var.as_str().unwrap().to_owned(), false);
    }
    st
}

/// `tdiff`: every key whose value changed, in byte order of the key, with the new value (`Null` = removed).
fn tdiff(before: &indexmap::IndexMap<String, ConfigValue>, after: &indexmap::IndexMap<String, ConfigValue>) -> Vec<(String, V)> {
    let mut keys: Vec<&String> = before.keys().chain(after.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| before.get(*k) != after.get(*k))
        .map(|k| (k.clone(), v_cfg(after.get(k))))
        .collect()
}

fn ctldiff(before: &ConfigState, after: &ConfigState) -> Vec<(String, String, V)> {
    let mut keys: Vec<&String> = before
        .controls
        .sel_index
        .keys()
        .chain(after.controls.sel_index.keys())
        .chain(before.controls.enabled.keys())
        .chain(after.controls.enabled.keys())
        .collect();
    keys.sort();
    keys.dedup();
    let mut out = Vec::new();
    for k in keys {
        let (a, b) = (before.controls.sel_index.get(k), after.controls.sel_index.get(k));
        if a != b {
            out.push(("sel".into(), k.clone(), b.map_or(V::Null, |x| V::Num((*x as f64).to_bits()))));
        }
        if before.control_enabled(k) != after.control_enabled(k) {
            out.push(("en".into(), k.clone(), V::Bool(after.control_enabled(k))));
        }
    }
    out
}

#[test]
fn all_539_apply_closures_match_luajit() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/config-apply.json");
    let fx: Json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let start = start_state(&fx["start"]);
    let V::Num(cl) = v_json(&fx["start"]["characterLevel"]) else {
        panic!("characterLevel")
    };
    let character_level = f64::from_bits(cl);

    let cases = fx["cases"].as_array().unwrap();
    let mut vars = std::collections::HashSet::new();
    let mut bad: Vec<String> = Vec::new();
    let mut with_side = 0;
    let mut with_mods = 0;
    for (i, c) in cases.iter().enumerate() {
        let var = c["var"].as_str().unwrap();
        vars.insert(var.to_owned());
        let entry = APPLY_REGISTRY
            .iter()
            .find(|e| e.var == var)
            .unwrap_or_else(|| panic!("{var}: no registry entry"));
        let mut st = start.clone();
        if let Some(ctx) = c.get("ctx") {
            for (k, v) in pairs(&ctx["input"]) {
                st.input.insert(k, cfg_of(&v));
            }
            for (k, v) in pairs(&ctx["sel"]) {
                let idx = list_index(&k, &cfg_of(&v));
                st.controls.sel_index.insert(k, idx);
            }
        }
        let before = st.clone();
        let val = cfg_of(&c["val"]);
        let (mut ml, mut eml, mut warnings) = (ModList::default(), ModList::default(), Vec::new());
        let res = {
            let mut ctx = ApplyCtx {
                mod_list: &mut ml,
                enemy_mod_list: &mut eml,
                tab: &mut st,
                character_level,
                warnings: &mut warnings,
            };
            (entry.apply)(&val, &mut ctx)
        };
        let label = format!(
            "#{i} {var} = {:?}{}",
            c["val"],
            c.get("ctx").map_or(String::new(), |x| format!(" ctx {x}"))
        );
        let mut diffs = Vec::new();
        let want_err = c.get("error").and_then(Json::as_str);
        let got_err = res.as_ref().err().map(|e| e.0.as_str());
        if want_err != got_err {
            diffs.push(format!("error: want {want_err:?}, got {got_err:?}"));
        }
        let sorted = c["sorted"].as_bool().unwrap();
        for (key, list) in [("mods", &ml), ("enemyMods", &eml)] {
            let want: Vec<&str> = c[key].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
            let mut got: Vec<String> = list.mods.iter().map(|m| format_source_mod(m)).collect();
            if sorted {
                got.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            }
            if want != got {
                diffs.push(format!("{key}:\n      want {want:?}\n      got  {got:?}"));
            }
        }
        if !(ml.mods.is_empty() && eml.mods.is_empty()) {
            with_mods += 1;
        }
        let want_in: Vec<(String, V)> = pairs(&c["input"]).into_iter().map(|(k, v)| (k, v_json(&v))).collect();
        let want_ph: Vec<(String, V)> = pairs(&c["placeholder"]).into_iter().map(|(k, v)| (k, v_json(&v))).collect();
        let got_in = tdiff(&before.input, &st.input);
        let got_ph = tdiff(&before.placeholder, &st.placeholder);
        if want_in != got_in {
            diffs.push(format!("input: want {want_in:?}, got {got_in:?}"));
        }
        if want_ph != got_ph {
            diffs.push(format!("placeholder: want {want_ph:?}, got {got_ph:?}"));
        }
        let want_lvl = v_json(&c["enemyLevel"]);
        let got_lvl = if st.enemy_level.to_bits() != before.enemy_level.to_bits() {
            V::Num(st.enemy_level.to_bits())
        } else {
            V::Null
        };
        if want_lvl != got_lvl {
            diffs.push(format!("enemyLevel: want {want_lvl:?}, got {got_lvl:?}"));
        }
        let want_ctl: Vec<(String, String, V)> = c["controls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| (x[0].as_str().unwrap().to_owned(), x[1].as_str().unwrap().to_owned(), v_json(&x[2])))
            .collect();
        let got_ctl = ctldiff(&before, &st);
        if want_ctl != got_ctl {
            diffs.push(format!("controls: want {want_ctl:?}, got {got_ctl:?}"));
        }
        if !(want_in.is_empty() && want_ph.is_empty() && want_lvl == V::Null && want_ctl.is_empty()) {
            with_side += 1;
        }
        if !diffs.is_empty() {
            bad.push(format!("{label}\n    {}", diffs.join("\n    ")));
        }
    }
    println!(
        "config-apply: {} cases / {} vars ({}), {} cases with mods, {} cases with side effects; Rust = LuaJIT on {}/{}",
        cases.len(),
        vars.len(),
        fx["luajit"].as_str().unwrap_or("?"),
        with_mods,
        with_side,
        cases.len() - bad.len(),
        cases.len()
    );
    assert_eq!(vars.len(), 539, "the fixture covers every var with an apply");
    for b in bad.iter().take(40) {
        println!("{b}");
    }
    assert!(bad.is_empty(), "{} of {} cases differ from LuaJIT", bad.len(), cases.len());
}
