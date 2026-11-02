//! The generated closures (`closures_gen.rs`, tools/modparser-gen, R5.3a): the drift contract (every generated definition
//! carries the sha of the Lua body `fns` records), and the partition of `fns` into hand-ported helpers, generated
//! closures and the manual list (`KNOWLEDGE/drafts/R5-manual-closures.md`).

use std::collections::HashSet;

use pob_data::mod_parser::mod_parser;
use pob_parser::closures::{ClosureFn, HELPERS, drift, registry};
use pob_parser::closures_gen::GENERATED;
use pob_parser::closures_manual::MANUAL;

#[test]
fn generated_sha_matches_fns() {
    let d = mod_parser();
    assert_eq!(drift(&GENERATED, &d), Vec::<String>::new());
    let ids: HashSet<&str> = GENERATED.iter().map(|e| e.id).collect();
    assert_eq!(ids.len(), GENERATED.len(), "a definition generated twice");
    assert!(HELPERS.iter().all(|h| !ids.contains(h.id)), "a hand-ported helper was generated");
    assert_eq!(registry().len(), HELPERS.len() + GENERATED.len() + MANUAL.len());
    assert_eq!(registry().len(), d.fns.len(), "every definition of fns is registered (R5.4)");
    let kinds = |f: fn(&ClosureFn) -> bool| GENERATED.iter().filter(|e| f(&e.f)).count();
    println!(
        "generated: {}/{} sha = mod-parser.json fns (special {}, value {}, helper {})",
        GENERATED.len(),
        GENERATED.len(),
        kinds(|f| matches!(f, ClosureFn::Special(_))),
        kinds(|f| matches!(f, ClosureFn::Value(_))),
        kinds(|f| matches!(f, ClosureFn::Helper)),
    );
}

#[test]
fn a_tampered_generated_sha_is_red() {
    let d = mod_parser();
    let mut entries = GENERATED.to_vec();
    entries[0].sha = "0000000000000000000000000000000000000000000000000000000000000000";
    entries[1].to += 1;
    assert_eq!(drift(&entries, &d).len(), 2);
}

/// Every definition of `fns` is exactly one of: hand-ported (HELPERS), generated, or on the manual list.
#[test]
fn fns_are_helpers_generated_or_manual() {
    let d = mod_parser();
    let md = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/modparser-gen/manual-closures.md"))
        .expect("tools/modparser-gen/manual-closures.md (node tools/modparser-gen/gen.ts)");
    let manual: HashSet<&str> = md.lines().filter_map(|l| l.strip_prefix("| `")?.split('`').next()).collect();
    // R5.4: the manual list is exactly the hand-ported `closures_manual::MANUAL`
    assert_eq!(manual, MANUAL.iter().map(|e| e.id).collect::<HashSet<_>>());
    let generated: HashSet<&str> = GENERATED.iter().map(|e| e.id).collect();
    let helpers: HashSet<&str> = HELPERS.iter().map(|e| e.id).collect();
    for id in d.fns.keys() {
        let n = usize::from(manual.contains(&**id)) + usize::from(generated.contains(&**id)) + usize::from(helpers.contains(&**id));
        assert_eq!(n, 1, "{id}: in {n} of helpers/generated/manual");
    }
    assert_eq!(manual.len() + generated.len() + helpers.len(), d.fns.len());
    println!(
        "fns {}: helpers {}, generated {}, manual {}",
        d.fns.len(),
        helpers.len(),
        generated.len(),
        manual.len()
    );
}
