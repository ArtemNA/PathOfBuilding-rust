//! U8a gate of `pob_engine::party_tab` and of the build code, path equivalence (R20, R8-ARCH §1.1): a mutation of `partyTab.actor` + `refresh` == a fresh load of the same buffers, every scalar of
//! `mainOutput` bit for bit; an edit of a buffer alone changes nothing in the calc; a code taken from an engine loads into the same output. The path against the original: `crates/pob-ui/tests/import.rs`.

use pob_calc::StatValue;
use pob_calc::pob_build::{self, PartyImportedBuffs, PartyText};
use pob_engine::engine::Engine;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../pob-calc/tests/fixtures/party-synth/party-attack-full.xml"
);

fn bits(e: &Engine) -> Vec<(String, Option<u64>)> {
    e.result()
        .stats
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                match v {
                    StatValue::Num(n) => Some(n.to_bits()),
                    _ => None,
                },
            )
        })
        .collect()
}

fn fixture() -> String {
    std::fs::read_to_string(FIXTURE).expect("fixture")
}

fn buffers(e: &Engine) -> Vec<(String, String)> {
    e.model()
        .party_imported_buffs
        .iter()
        .map(|b| match &b.text {
            PartyText::Text(t) => (b.name.clone(), t.clone()),
            PartyText::Element => panic!("an element"),
        })
        .collect()
}

/// The fixture with no `<Party>` content.
fn without_party(xml: &str) -> String {
    let a = xml.find("<Party").expect("Party");
    let b = xml.find("</Party>").expect("/Party") + "</Party>".len();
    format!("{}{}", &xml[..a], &xml[b..])
}

#[test]
fn parse_of_every_buffer_equals_a_load() {
    let full = Engine::load_xml(&fixture()).expect("load");
    let list = buffers(&full);
    assert_eq!(list.len(), 7, "the fixture's buffers");
    let mut e = Engine::load_xml(&without_party(&fixture())).expect("load");
    assert_ne!(bits(&e), bits(&full), "the party does something on this build");
    for (name, text) in &list {
        e.party_parse(name, text).expect("parse");
    }
    e.refresh().expect("refresh");
    assert_eq!(bits(&e), bits(&full), "ParseBuffs + the buildFlag frame == a load");
}

#[test]
fn wipe_all_and_rebuild_return_to_the_same_output() {
    let mut e = Engine::load_xml(&fixture()).expect("load");
    let before = bits(&e);
    let list = buffers(&e);
    let none = Engine::load_xml(&without_party(&fixture())).expect("load");
    e.party_wipe_all();
    e.refresh().expect("refresh");
    assert_eq!(bits(&e), bits(&none), "Disable Party Effects == no party");
    for (name, text) in &list {
        e.party_parse(name, text).expect("parse");
    }
    e.refresh().expect("refresh");
    assert_eq!(bits(&e), before, "Rebuild All == the load");
}

#[test]
fn wipe_of_one_buffer_equals_a_load_without_it() {
    let mut e = Engine::load_xml(&fixture()).expect("load");
    e.party_wipe(&["Aura"]);
    e.refresh().expect("refresh");
    let a = fixture().find("<ImportedBuffs name=\"Aura\">").expect("Aura");
    let x = fixture();
    let b = a + x[a..].find("</ImportedBuffs>").expect("end") + "</ImportedBuffs>".len();
    let fresh = Engine::load_xml(&format!("{}{}", &x[..a], &x[b..])).expect("load");
    assert_eq!(bits(&e), bits(&fresh), "wipeTable(actor.Aura)");
}

#[test]
fn an_edit_alone_reaches_the_file_and_not_the_calc() {
    let mut e = Engine::load_xml(&fixture()).expect("load");
    let before = bits(&e);
    let mut state = e.party_state().clone();
    state.buffers.shift_remove("Aura");
    state.destination = "Curse".into();
    state.append = true;
    state.show_advance_tools = true;
    e.party_set_state(state);
    e.refresh().expect("refresh");
    assert_eq!(bits(&e), before, "the buffers are not the actor (PartyTab.lua:518-541)");
    let saved = e.save_xml().expect("save");
    assert!(!saved.contains("<ImportedBuffs name=\"Aura\">"), "Save reads the buffers");
    assert!(saved.contains("destination=\"Curse\"") && saved.contains("append=\"true\"") && saved.contains("ShowAdvanceTools=\"true\""));
    // what SaveDB + load make of the actor survives a materialize
    let actor = e.model().party_imported_buffs.clone();
    e.materialize().expect("materialize");
    assert_eq!(e.model().party_imported_buffs, actor, "materialize keeps the actor");
}

#[test]
fn a_failed_parse_keeps_the_state_and_says_the_line() {
    let mut e = Engine::load_xml(&fixture()).expect("load");
    let before: Vec<PartyImportedBuffs> = e.model().party_imported_buffs.clone();
    let err = e.party_parse("EnemyMods", "1|a|b|BASE").expect_err("a mod without a group");
    assert!(err.contains("PartyTab.lua:744"), "{err}");
    assert_eq!(e.model().party_imported_buffs, before);
}

#[test]
fn a_build_code_round_trip_keeps_the_output() {
    let e = Engine::load_xml(&fixture()).expect("load");
    let code = e.build_code().expect("code");
    let xml = String::from_utf8(pob_build::decode_build_code(&code).expect("Inflate")).expect("utf-8");
    assert_eq!(xml, e.save_xml().expect("save"), "the code holds SaveDB");
    let again = Engine::load_xml(&xml).expect("load");
    assert_eq!(bits(&again), bits(&e), "import of the code == the build");
    assert_eq!(buffers(&again), buffers(&e), "the party came along");
}
