//! R3.8b: `parse_xml_bytes` = `runtime/lua/xml.lua` `ParseXML` on the R3.8a LuaJIT corpus (`tests/fixtures/xml-lua.json`,
//! 244 inputs, desktop LuaJIT, docker cross-checked). Fixture strings are byte strings: one JSON char per byte (<= U+00FF).
//! Shape (tools/lua-corpus/export-xml-config.ts `XML_BODY`): a tree is `{"children": [...]}`, an element
//! `{"attrib": [[k, v], ...] sorted by key, "children": [...], "elem": name, "empty": true?}`, a string is a text node;
//! a `nil, err` return is `["ERR", msg]`, a raised error `["THROW", msg]`. Comparison: exact JSON equality.

use pob_build::xml::{RawXmlNode, parse_xml_bytes};
use serde_json::{Map, Value as Json, json};

fn bytes_of(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or_else(|_| panic!("fixture char {c:?} is not a byte")))
        .collect()
}

fn str_of(b: &[u8]) -> Json {
    Json::String(b.iter().map(|&c| char::from(c)).collect())
}

fn node(n: &RawXmlNode) -> Json {
    match n {
        RawXmlNode::Text(t) => str_of(t),
        RawXmlNode::Element(e) => {
            let mut attrib: Vec<(&Vec<u8>, &Vec<u8>)> = e.attrib.iter().collect();
            attrib.sort_by(|a, b| a.0.cmp(b.0));
            let mut o = Map::new();
            o.insert(
                "attrib".into(),
                Json::Array(attrib.into_iter().map(|(k, v)| json!([str_of(k), str_of(v)])).collect()),
            );
            o.insert("children".into(), Json::Array(e.children.iter().map(node).collect()));
            o.insert("elem".into(), str_of(&e.name));
            if e.empty {
                o.insert("empty".into(), json!(true));
            }
            Json::Object(o)
        }
    }
}

#[test]
fn xml_lua_corpus_244() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xml-lua.json");
    let fixture: Json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 244);
    let mut bad = Vec::new();
    for (i, c) in cases.iter().enumerate() {
        let input = c["input"].as_str().unwrap();
        let got = match parse_xml_bytes(&bytes_of(input)) {
            Ok(top) => json!({ "children": top.iter().map(node).collect::<Vec<_>>() }),
            Err(e) => json!([if e.thrown { "THROW" } else { "ERR" }, e.message]),
        };
        if got != c["result"] {
            bad.push(format!("#{i} {input:?}\n  want {}\n  got  {got}", c["result"]));
        }
    }
    println!("xml-lua: {}/{} equal", cases.len() - bad.len(), cases.len());
    assert!(bad.is_empty(), "{} of {} differ:\n{}", bad.len(), cases.len(), bad.join("\n"));
}
