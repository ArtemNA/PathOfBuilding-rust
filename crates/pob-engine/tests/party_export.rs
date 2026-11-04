//! R-party-export gate: `<Party><ExportedBuffs>` of `SaveDB` = the original's, on synthetic builds (`tests/fixtures/party-export`, truth: the `SaveDB` of the original PoB, `tools/sg-oracle`,
//! `gen_party_export.py truth`).
//!
//! R13: a buff's `modList` comes from `pairs` over the stats of its level (CalcActiveSkill.lua, `mergeSkillInstanceMods`), so the order of its lines differs between processes of the original
//! (8 processes of the original on 5 builds: 4-8 raw texts, one text with these runs sorted): the lines of one buff compare as a set, sorted here; everything else compares byte for byte.

use std::path::{Path, PathBuf};

use pob_engine::engine::Engine;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/party-export")
}

fn names() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir())
        .expect("fixtures")
        .filter_map(|e| {
            e.ok()?
                .file_name()
                .into_string()
                .ok()?
                .strip_suffix(".party.xml")
                .map(str::to_owned)
        })
        .collect();
    v.sort();
    v
}

/// The `<Party>` element of a composed XML, lines as they are.
fn party_of(xml: &str) -> String {
    let a = xml.find("\t<Party").expect("<Party");
    let e = xml[a..].find("</Party>\n").expect("</Party>") + a + "</Party>\n".len();
    xml[a..e].to_owned()
}

/// Every run of mod lines (`value|source|name|type|flags|keywordFlags|tags`) sorted, in the buff sections (R13, the module doc).
fn canon(party: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut run: Vec<String> = Vec::new();
    let mut in_buffs = false;
    for l in party.lines() {
        if l.contains("<ExportedBuffs") {
            in_buffs = ["\"Aura\"", "\"Curse\"", "\"Warcry Skills\"", "\"Link Skills\""]
                .iter()
                .any(|n| l.contains(n));
        }
        if in_buffs && l.matches('|').count() >= 5 {
            run.push(l.to_owned());
            continue;
        }
        run.sort();
        out.append(&mut run);
        out.push(l.to_owned());
    }
    out
}

fn first_diff(a: &[String], b: &[String]) -> Option<String> {
    let i = a.iter().zip(b).position(|(x, y)| x != y);
    match i {
        Some(i) => Some(format!("line {}: ours {:?}\n  original {:?}", i + 1, a[i], b[i])),
        None if a.len() != b.len() => Some(format!("lines: ours {}, original {}", a.len(), b.len())),
        None => None,
    }
}

#[test]
fn exported_buffs_equal_the_original() {
    let names = names();
    assert_eq!(names.len(), 10, "{names:?}");
    let (mut bytes, mut bad) = (0, Vec::new());
    let (mut load_ms, mut save_ms) = (0.0, 0.0);
    for n in &names {
        let xml = std::fs::read_to_string(dir().join(format!("{n}.xml"))).expect("xml");
        let truth = std::fs::read_to_string(dir().join(format!("{n}.party.xml"))).expect("truth");
        let t0 = std::time::Instant::now();
        let e = Engine::load_xml(&xml).unwrap_or_else(|err| panic!("{n}: {err}"));
        load_ms += t0.elapsed().as_secs_f64() * 1e3;
        let t1 = std::time::Instant::now();
        let saved = e.save_xml().unwrap_or_else(|err| panic!("{n}: {err}"));
        save_ms += t1.elapsed().as_secs_f64() * 1e3;
        let ours = party_of(&saved);
        bytes += usize::from(ours == truth);
        if let Some(d) = first_diff(&canon(&ours), &canon(&truth)) {
            bad.push(format!("{n}: {d}"));
        }
    }
    println!(
        "party export: {} builds, equal {}, byte for byte {bytes}; load {load_ms:.0} ms, SaveDB {save_ms:.1} ms in all",
        names.len(),
        names.len() - bad.len()
    );
    assert!(bad.is_empty(), "{} differ:\n{}", bad.len(), bad.join("\n"));
}

/// The export comes back after a load: `<ExportedBuffs>` is ignored by `Load` (PartyTab.lua:543-557), the flag is not.
#[test]
fn exported_buffs_survive_a_round_trip() {
    for n in names() {
        let xml = std::fs::read_to_string(dir().join(format!("{n}.xml"))).expect("xml");
        let e = Engine::load_xml(&xml).unwrap_or_else(|err| panic!("{n}: {err}"));
        let saved = e.save_xml().expect("save");
        let e2 = Engine::load_xml(&saved).unwrap_or_else(|err| panic!("{n}: reload {err}"));
        assert!(e2.import_export_party(), "{n}: the flag");
        assert_eq!(
            canon(&party_of(&saved)),
            canon(&party_of(&e2.save_xml().expect("save 2"))),
            "{n}: <Party> after a load"
        );
    }
}

/// Off: no `<ExportedBuffs>`, whatever was run.
#[test]
fn no_export_without_the_flag() {
    let n = "px-auras";
    let xml = std::fs::read_to_string(dir().join(format!("{n}.xml")))
        .expect("xml")
        .replace("<Import exportParty=\"true\"/>", "<Import/>");
    let e = Engine::load_xml(&xml).expect("load");
    let saved = e.save_xml().expect("save");
    assert!(!saved.contains("ExportedBuffs"), "{}", party_of(&saved));
    assert!(e.calcs_run().env.buff_exports.is_none());
    assert!(e.result().env.buff_exports.is_none());
}
