//! R8.2 gates on the R8.1 LuaJIT fixtures (`crates/pob-engine/tests/fixtures/r8/<slug>.{save.json,save.xml}`, 26 scenarios):
//! build codes (ImportTab.lua :505/:632) and `xml.lua` `ComposeXML`.
//!
//! The fixture `code` is LuaJIT `common.base64` over Node `zlib.deflateSync(save.xml)` with `+/` -> `-_`
//! (`save.json.base64.luajitEncodeEqualsNode`), so: (1) Rust decodes the LuaJIT code into `save.xml` byte for byte;
//! (2) Rust's base64 of those same zlib bytes is the LuaJIT code byte for byte; (3) `decode(encode(x)) == x`.

use std::path::{Path, PathBuf};

use pob_build::code::{base64_decode, base64_encode, inflate};
use pob_build::{XmlElement, XmlNode, compose_xml, decode_build_code, encode_build_code, parse_xml_document};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../pob-engine/tests/fixtures/r8")
}

/// `(slug, save.json, save.xml)` for every scenario, sorted by slug.
fn scenarios() -> Vec<(String, serde_json::Value, String)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(fixture_dir()).expect("r8 fixture dir") {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let Some(slug) = name.strip_suffix(".save.json") else { continue };
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("save.json")).expect("json");
        let xml = std::fs::read_to_string(fixture_dir().join(format!("{slug}.save.xml"))).expect("save.xml");
        out.push((slug.to_string(), json, xml));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(out.len(), 26, "R8.1 has 26 scenarios");
    out
}

#[test]
fn build_codes_match_luajit() {
    let (mut decoded, mut b64, mut round, mut zlib_same) = (0, 0, 0, 0);
    let dump = std::env::var_os("POB_R82_DUMP_DIR").map(PathBuf::from);
    for (slug, json, xml) in scenarios() {
        let code = json["code"].as_str().expect("code");
        assert_eq!(json["codeLen"].as_u64(), Some(code.len() as u64), "{slug}");
        // (1) LuaJIT code -> save.xml
        let back = decode_build_code(code).unwrap_or_else(|| panic!("{slug}: Inflate failed"));
        assert!(back == xml.as_bytes(), "{slug}: decode(code) != save.xml");
        decoded += 1;
        // (2) base64 part = LuaJIT common.base64 (on the same zlib bytes)
        let z = base64_decode(code.replace('-', "+").replace('_', "/").as_bytes());
        assert_eq!(json["zlibLen"].as_u64(), Some(z.len() as u64), "{slug}");
        assert_eq!(base64_encode(&z).replace('+', "-").replace('/', "_"), code, "{slug}: base64");
        assert_eq!(inflate(&z).as_deref(), Some(xml.as_bytes()), "{slug}");
        b64 += 1;
        // (3) Rust code round trip; zlib byte equality with Node is informational only (R8-ARCH §1 p. 3)
        let rust_code = encode_build_code(xml.as_bytes());
        assert!(
            decode_build_code(&rust_code).as_deref() == Some(xml.as_bytes()),
            "{slug}: decode(encode(x)) != x"
        );
        round += 1;
        if rust_code == code {
            zlib_same += 1;
        }
        if let Some(dir) = &dump {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(format!("{slug}.code")), &rust_code).unwrap();
        }
    }
    eprintln!(
        "codes: decode(LuaJIT) {decoded}/26, base64 = LuaJIT {b64}/26, round trip {round}/26, Rust code byte-equal to LuaJIT+Node {zlib_same}/26"
    );
}

fn el(name: &str, attrib: &[(&str, &str)], children: Vec<XmlNode>) -> XmlNode {
    XmlNode::Element(XmlElement {
        name: name.into(),
        attrib: attrib.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        children,
        empty: false,
    })
}

#[test]
fn compose_xml_is_xml_lua() {
    // xml.lua:112-171: tabs per level, attribs encoded, `node[1]` absent -> `/>`, an empty-string text child still opens the
    // element (a Lua "" is truthy) and is written as a tab-indented empty line.
    let XmlNode::Element(root) = el(
        "PathOfBuilding",
        &[],
        vec![
            el("Build", &[("a", "x<y>&'\""), ("b", "")], vec![]),
            el(
                "Config",
                &[],
                vec![el(
                    "CustomModifierBlock",
                    &[("title", "Default")],
                    vec![XmlNode::Text(String::new())],
                )],
            ),
            el("Notes", &[], vec![XmlNode::Text("line 1\nA & B".into())]),
        ],
    ) else {
        unreachable!()
    };
    let want = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<PathOfBuilding>\n\t<Build a=\"x&lt;y&gt;&amp;&apos;&quot;\" b=\"\"/>\n\t<Config>\n\t\t<CustomModifierBlock title=\"Default\">\n\t\t\t\n\t\t</CustomModifierBlock>\n\t</Config>\n\t<Notes>\n\t\tline 1\nA &amp; B\n\t</Notes>\n</PathOfBuilding>\n";
    assert_eq!(compose_xml(&root), want);
}

/// `ParseXML` -> `ComposeXML` on the 26 LuaJIT-composed fixtures. `ParseXML` drops whitespace-only text runs (xml.lua:44/:58),
/// so an element whose only child was `""` comes back as `<x/>`; everything else must be byte-identical, and the tree must
/// survive a second pass unchanged.
#[test]
fn compose_xml_round_trips_fixtures() {
    let (mut identical, mut only_empty_text) = (0, 0);
    for (slug, _, xml) in scenarios() {
        let doc = parse_xml_document(&xml).expect("parse");
        let [XmlNode::Element(root)] = doc.as_slice() else {
            panic!("{slug}: one root")
        };
        let out = compose_xml(root);
        let again = parse_xml_document(&out).expect("reparse");
        assert_eq!(again, doc, "{slug}: tree changed through ComposeXML");
        if out == xml {
            identical += 1;
            continue;
        }
        // Differences may only be `<x ...>\n<tabs>\n<tabs></x>` (an empty-string text child) collapsed to `<x .../>`.
        let mut expected = String::new();
        let lines: Vec<&str> = xml.split_inclusive('\n').collect();
        let mut i = 0;
        while i < lines.len() {
            let l = lines[i];
            if i + 2 < lines.len() && l.ends_with(">\n") && !l.trim_start().starts_with("</") && lines[i + 1].trim_matches('\t') == "\n" {
                let name = l.trim_start_matches('\t')[1..].split([' ', '>']).next().unwrap();
                if lines[i + 2].trim_start_matches('\t') == format!("</{name}>\n") {
                    expected.push_str(&l[..l.len() - 2]);
                    expected.push_str("/>\n");
                    i += 3;
                    continue;
                }
            }
            expected.push_str(l);
            i += 1;
        }
        assert_eq!(out, expected, "{slug}: ComposeXML(ParseXML(x)) differs beyond empty text runs");
        only_empty_text += 1;
    }
    eprintln!("ComposeXML(ParseXML(x)): byte-identical {identical}/26, identical up to dropped empty text {only_empty_text}/26");
}

/// Perf vs `tools/.scratch/perf-r8.json` (LuaJIT `common.base64` encode/decode, Node zlib): `cargo test --release -p pob-build
/// --test save_codes perf_codes -- --ignored --nocapture`. Median of 50 runs after one warm-up, ms.
#[test]
#[ignore]
fn perf_codes() {
    fn median(f: &mut dyn FnMut()) -> f64 {
        f();
        let mut t: Vec<f64> = (0..50)
            .map(|_| {
                let t0 = std::time::Instant::now();
                f();
                t0.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        t.sort_by(f64::total_cmp);
        t[t.len() / 2]
    }
    for (slug, json, xml) in scenarios() {
        if json["kind"] != "golden" {
            continue;
        }
        let code = json["code"].as_str().unwrap().to_string();
        let z = base64_decode(code.replace('-', "+").replace('_', "/").as_bytes());
        let zr = pob_build::code::deflate(xml.as_bytes());
        let enc = median(&mut || drop(std::hint::black_box(base64_encode(&z))));
        let dec = median(&mut || drop(std::hint::black_box(base64_decode(code.as_bytes()))));
        let defl = median(&mut || drop(std::hint::black_box(pob_build::code::deflate(xml.as_bytes()))));
        let infl = median(&mut || drop(std::hint::black_box(inflate(&z))));
        let full = median(&mut || drop(std::hint::black_box(encode_build_code(xml.as_bytes()))));
        eprintln!(
            "{slug}: xml {} B, zlib Rust {} B / Node {} B; base64 enc {enc:.4} ms, dec {dec:.4} ms; deflate {defl:.4} ms, inflate {infl:.4} ms; encode_build_code {full:.4} ms",
            xml.len(),
            zr.len(),
            z.len()
        );
    }
}
