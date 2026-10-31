//! `data-patches/mod-numbers.json` (R5.2b, MINEFIELD-RUST R9): the loaders of `mod-cache.json`/`mod-parser.json` apply the
//! LuaJIT numbers the converter lost (`math.huge` -> `null`, `-0` -> `0`).

use pob_data::mod_cache::mod_cache;
use pob_data::mod_parser::{PVal, mod_parser};
use pob_data::patches::{Patch, apply_at, parse_patches, patch_path, patches};
use pob_mods::Value;
use serde_json::{Value as Json, json};

fn count(file: &str, value: &str) -> usize {
    patches().unwrap().iter().filter(|p| p.file == file && p.value == value).count()
}

fn first_value(mods: &[pob_mods::Mod]) -> f64 {
    match &mods[0].value {
        Value::Num(n) => *n,
        v => panic!("not a number: {v:?}"),
    }
}

#[test]
fn patch_counts_from_luajit() {
    // 2 inf in both files, 14 `-0` ("0% reduced ..." lines) in the cache; nothing else (no nan / -inf anywhere).
    assert_eq!(patches().unwrap().len(), 18);
    assert_eq!(count("mod-cache.json", "inf"), 2);
    assert_eq!(count("mod-cache.json", "-0"), 14);
    assert_eq!(count("mod-parser.json", "inf"), 2);
}

#[test]
fn mod_cache_carries_inf_and_negative_zero() {
    let c = mod_cache();
    for line in [
        "Your Hexes have infinite Duration",
        "Projectiles can Chain from any number of additional targets in Close Range",
    ] {
        let mods = c.get(line).unwrap().mods.as_ref().unwrap();
        assert_eq!(first_value(mods), f64::INFINITY, "{line}");
    }
    let mut neg_zero = 0;
    for p in patches().unwrap().iter().filter(|p| p.file == "mod-cache.json" && p.value == "-0") {
        let line = p.path[0].as_str().unwrap();
        let v = first_value(c.get(line).unwrap().mods.as_ref().unwrap());
        assert!(v == 0.0 && v.is_sign_negative(), "{line}: {v:?}");
        neg_zero += 1;
    }
    assert_eq!(neg_zero, 14);
    // every other 0 value of the cache stays +0 (the patch touches only what LuaJIT has as -0)
    let zeros_negative: usize = c
        .entries
        .iter()
        .flat_map(|e| e.mods.iter().flatten())
        .filter(|m| matches!(&m.value, Value::Num(n) if *n == 0.0 && n.is_sign_negative()))
        .count();
    assert_eq!(zeros_negative, 14);
}

#[test]
fn mod_parser_carries_inf() {
    let d = mod_parser();
    let t = d.table("specialModList");
    for key in [
        "^your hexes have infinite duration$",
        "^projectiles can chain from any number of additional targets in close range$",
    ] {
        let PVal::Lua(Value::Table(list)) = t.get(key).unwrap() else {
            panic!("{key}")
        };
        let Value::Mod(m) = &list.array[0] else { panic!("{key}") };
        assert!(matches!(m.value, Value::Num(n) if n == f64::INFINITY), "{key}: {:?}", m.value);
    }
}

#[test]
fn stale_patch_is_an_error() {
    let text = std::fs::read_to_string(patch_path()).unwrap();
    let good = patches().unwrap().len();
    let lock = text
        .split("\"manifestSha256\":\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(parse_patches(&text, &lock).unwrap().len(), good);
    let e = parse_patches(&text, &"0".repeat(64)).unwrap_err();
    assert!(e.0.contains("export-data-patch") && e.0.contains(&lock), "{e}");
}

#[test]
fn drifted_target_is_an_error() {
    let p = |value: &str| Patch {
        file: "x.json".into(),
        path: vec![json!("a"), json!(0)],
        value: value.into(),
    };
    let mut ok = json!({"a": [null]});
    apply_at(&mut ok, &[json!("a"), json!(0)], &p("inf")).unwrap();
    assert_eq!(ok, json!({"a": [{"$num": "inf"}]}));
    let mut neg = json!({"a": [0]});
    apply_at(&mut neg, &[json!("a"), json!(0)], &p("-0")).unwrap();
    assert!(neg["a"][0].as_f64().unwrap().is_sign_negative());
    // the data already holds a real number / the path does not exist: not what the converter wrote
    for (mut j, value) in [
        (json!({"a": [5]}), "inf"),
        (json!({"a": [1]}), "-0"),
        (json!({"a": []}), "inf"),
        (json!({"b": 1}), "inf"),
    ] {
        assert!(apply_at(&mut j, &[json!("a"), json!(0)], &p(value)).is_err(), "{j} {value}");
    }
    let _: Json = Json::Null;
}
