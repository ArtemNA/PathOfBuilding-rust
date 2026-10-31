//! R3.8b: `load_build` = LuaJIT `loadBuildFromXML` on the R3.8a corpus (`tests/fixtures/config-load.json`, 99 full builds,
//! docker PoB, one fresh instance per case). The Rust state is dumped exactly as the corpus' `CFG_BODY` dumps the Lua one
//! (tools/lua-corpus/export-xml-config.ts: numbers `{"num": "%.17g"}`, tables as objects, byte strings one char per byte,
//! `configSets`/`configSetOrderList` as `[id, ...]` pairs sorted by key, mods through `formatSourceMod`), packed with the
//! same `input`/`placeholder` diff against the fixture's `reference`, `normAddr` applied, and compared as JSON, exactly.
//! A Lua error case (`{"prompt": ...}`) must be a `BuildLoadError` whose `launch:ShowErrMsg` prompt is the same text.

use pob_build::{BlockText, BuildLoadError, ConfigState, ConfigValue, LoadedBuild, XmlElement, XmlNode, load_build};
use pob_mods::format::format_source_mod;
use serde_json::{Map, Value as Json, json};

/// `versionNumber` of the PoB checkout the corpus was taken from (`manifest.xml` at pobSha ed354c2f8).
const POB_VERSION: &str = "2.67.2";

fn bstr(s: &str) -> Json {
    Json::String(s.bytes().map(char::from).collect())
}

fn utf8_of_bytes(s: &str) -> String {
    let b: Vec<u8> = s.chars().map(|c| u8::try_from(u32::from(c)).expect("byte string")).collect();
    String::from_utf8(b).expect("the config corpus inputs are UTF-8")
}

fn num(n: f64) -> Json {
    json!({ "num": pob_lua::format_g(n, 17) })
}

fn val(v: &ConfigValue) -> Json {
    match v {
        ConfigValue::Num(n) => num(*n),
        ConfigValue::Str(s) => bstr(s),
        ConfigValue::Bool(b) => json!(b),
    }
}

fn table(t: &indexmap::IndexMap<String, ConfigValue>) -> Json {
    Json::Object(t.iter().map(|(k, v)| (utf8_key(k), val(v))).collect::<Map<_, _>>())
}

fn utf8_key(k: &str) -> String {
    k.bytes().map(char::from).collect()
}

/// `enc` of an xml.lua node table: `attrib`, `elem`, `empty`, children under "1".."n".
fn xml_table(e: &XmlElement) -> Json {
    let mut o = Map::new();
    o.insert(
        "attrib".into(),
        Json::Object(e.attrib.iter().map(|(k, v)| (utf8_key(k), bstr(v))).collect()),
    );
    o.insert("elem".into(), bstr(&e.name));
    if e.empty {
        o.insert("empty".into(), json!(true));
    }
    for (i, c) in e.children.iter().enumerate() {
        let v = match c {
            XmlNode::Text(t) => bstr(t),
            XmlNode::Element(x) => xml_table(x),
        };
        o.insert((i + 1).to_string(), v);
    }
    Json::Object(o)
}

fn custom_mods(state: &ConfigState) -> Json {
    let mut o = Map::new();
    for (i, b) in state.custom_mods_list.iter().enumerate() {
        let text = match &b.text {
            BlockText::Str(s) => bstr(s),
            BlockText::Num(n) => num(*n),
            BlockText::Bool(x) => json!(x),
            BlockText::Element(e) => xml_table(e),
        };
        o.insert(
            (i + 1).to_string(),
            json!({ "enabled": b.enabled, "text": text, "title": bstr(&b.title) }),
        );
    }
    Json::Object(o)
}

/// `CFG_BODY` `dump()`.
fn dump(l: &LoadedBuild) -> Json {
    let m = &l.model;
    let fmt = |list: &pob_mods::ModList| Json::Array(list.mods.iter().map(|x| bstr(&format_source_mod(x))).collect());
    let mut sets: Vec<(f64, Json)> = m
        .config
        .sets
        .iter()
        .map(|s| {
            let st = if s.id == m.config.active_config_set {
                l.config.clone()
            } else {
                ConfigState::from_set(s)
            };
            let title = s.title.as_deref().map_or(Json::Null, bstr);
            let body = json!({ "customModsList": custom_mods(&st), "input": table(&st.input), "placeholder": table(&st.placeholder), "title": title });
            (s.id, body)
        })
        .collect();
    sets.sort_by(|a, b| a.0.total_cmp(&b.0));
    json!({
        "activeConfigSetId": num(m.config.active_config_set),
        "bandit": bstr(&m.bandit),
        "characterLevel": num(m.level),
        "configSetOrderList": m.config.set_order_list.iter().map(|&(i, id)| json!([num(i), num(id)])).collect::<Vec<_>>(),
        "configSets": sets.into_iter().map(|(id, b)| json!([num(id), b])).collect::<Vec<_>>(),
        "enemyLevel": num(l.config.enemy_level),
        "enemyMods": fmt(&l.config_lists.enemy_mod_list),
        "input": table(&l.config.input),
        "mainSocketGroup": num(m.main_socket_group),
        "mods": fmt(&l.config_lists.mod_list),
        "pantheonMajorGod": bstr(&m.pantheon_major_god),
        "pantheonMinorGod": bstr(&m.pantheon_minor_god),
        "placeholder": table(&l.config.placeholder),
    })
}

/// export-xml-config.ts `diff`: key -> new value, `null` = key absent.
fn diff(reference: &Json, cur: &Json) -> Json {
    let (r, c) = (reference.as_object().unwrap(), cur.as_object().unwrap());
    let mut o = Map::new();
    for (k, v) in c {
        if r.get(k) != Some(v) {
            o.insert(k.clone(), v.clone());
        }
    }
    for k in r.keys() {
        if !c.contains_key(k) {
            o.insert(k.clone(), Json::Null);
        }
    }
    Json::Object(o)
}

/// export-xml-config.ts `pack`.
fn pack(mut d: Json, reference: &Json) -> Json {
    let o = d.as_object_mut().unwrap();
    let input = o.shift_remove("input").unwrap();
    let placeholder = o.shift_remove("placeholder").unwrap();
    for s in o["configSets"].as_array_mut().unwrap() {
        let set = s[1].as_object_mut().unwrap();
        let i = set.shift_remove("input").unwrap();
        let p = set.shift_remove("placeholder").unwrap();
        set.insert("inputDiff".into(), diff(&reference["input"], &i));
        set.insert("placeholderDiff".into(), diff(&reference["placeholder"], &p));
    }
    o.insert("inputDiff".into(), diff(&reference["input"], &input));
    o.insert("placeholderDiff".into(), diff(&reference["placeholder"], &placeholder));
    d
}

/// export-xml-config.ts `normAddr`: `(function|table|userdata): 0x<hex>` -> `$1: <addr>` in every string.
fn norm_addr(v: Json) -> Json {
    fn norm(s: &str) -> String {
        let mut out = String::new();
        let mut rest = s;
        'outer: while !rest.is_empty() {
            for kind in ["function: 0x", "table: 0x", "userdata: 0x"] {
                if let Some(tail) = rest.strip_prefix(kind) {
                    let n = tail.bytes().take_while(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')).count();
                    if n > 0 {
                        out.push_str(&kind[..kind.len() - 2]);
                        out.push_str("<addr>");
                        rest = &tail[n..];
                        continue 'outer;
                    }
                }
            }
            let ch = rest.chars().next().unwrap();
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
        out
    }
    match v {
        Json::String(s) => Json::String(norm(&s)),
        Json::Array(a) => Json::Array(a.into_iter().map(norm_addr).collect()),
        Json::Object(o) => Json::Object(o.into_iter().map(|(k, x)| (k, norm_addr(x))).collect()),
        other => other,
    }
}

/// Every difference as a path.
fn paths(path: &str, want: &Json, got: &Json, out: &mut Vec<String>) {
    match (want, got) {
        (Json::Object(a), Json::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => paths(&format!("{path}.{k}"), x, y, out),
                    (x, y) => out.push(format!("{path}.{k}: want {x:?}, got {y:?}")),
                }
            }
        }
        (Json::Array(a), Json::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                paths(&format!("{path}[{i}]"), x, y, out);
            }
        }
        _ if want != got => out.push(format!("{path}: want {want}, got {got}")),
        _ => {}
    }
}

/// Post-load classes: the corpus dumps `build` after the whole `loadBuildFromXML`, calc included; these two fields are
/// changed there, not by the load this crate models. Pinned exactly (case set, want and got); anything else is a failure.
/// - `skill-groups` (R6): CalcSetup.lua:1589 `build.mainSocketGroup = m_min(m_max(#socketGroupList, 1), ...)` -- the base
///   build has no `<Items>`, its two item-granted groups go, 3 -> 2. Rust keeps Build.lua:986's value.
/// - `EstimatePlayerProgress`: Build.lua:898-913 re-estimates `characterLevel` from tree points when
///   `characterLevelAutoMode` -- after the calc, so it lives in `pob_calc::calculate`; the check that this case ends at
///   LuaJIT's level 7 moved to `crates/pob-calc/tests/perform_estimate.rs` (R4.2). `load_build` alone keeps 1.
fn post_load_class(diff: &str) -> Option<&'static str> {
    if diff.ends_with(".mainSocketGroup.num: want \"2\", got \"3\"") {
        return Some("skill-groups");
    }
    (diff == "autolevel-true.characterLevel.num: want \"7\", got \"1\"").then_some("EstimatePlayerProgress")
}

#[test]
fn config_load_corpus_99() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/config-load.json");
    let fixture: Json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let reference = &fixture["reference"];
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 99);
    let (mut loads, mut errors, mut bad) = (0, 0, Vec::new());
    let mut pinned: std::collections::BTreeMap<&str, Vec<String>> = Default::default();
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let want = &c["result"];
        let got = match load_build(&utf8_of_bytes(c["input"].as_str().unwrap())) {
            Ok(l) => {
                assert!(l.unimplemented.is_empty(), "{name}: {:?}", l.unimplemented);
                norm_addr(pack(dump(&l), reference))
            }
            Err(BuildLoadError(msg)) => norm_addr(json!({ "prompt": bstr(&BuildLoadError(msg).prompt(POB_VERSION)) })),
        };
        if want.get("prompt").is_some() {
            errors += 1;
        } else {
            loads += 1;
        }
        let mut d = Vec::new();
        paths(name, want, &got, &mut d);
        for x in d {
            match post_load_class(&x) {
                Some(class) => pinned.entry(class).or_default().push(name.to_owned()),
                None => bad.push(x),
            }
        }
    }
    let count = |k: &str| pinned.get(k).map_or(0, Vec::len);
    println!(
        "config-load: {loads} loads + {errors} errors; pinned skill-groups {} + EstimatePlayerProgress {}; other differences {}",
        count("skill-groups"),
        count("EstimatePlayerProgress"),
        bad.len()
    );
    assert_eq!((loads, errors), (88, 11));
    assert!(bad.is_empty(), "{} differences:\n{}", bad.len(), bad.join("\n"));
    // Every load except `main-socket-bogus` (mainSocketGroup="abc" -> 1 on both sides).
    let msg = &pinned["skill-groups"];
    assert_eq!(msg.len(), 87);
    assert!(!msg.iter().any(|n| n == "main-socket-bogus"));
    assert_eq!(pinned["EstimatePlayerProgress"], ["autolevel-true"]);
    assert_eq!(pinned.len(), 2);
}
