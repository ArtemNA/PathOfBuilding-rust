//! The drift guard on `pob_build::config::apply` (pob-ts `test/config/registry.test.ts`): every ported closure carries the
//! sha256 of the upstream `apply` body as `data/config-options.json` recorded it; a re-converted PathOfBuilding where one
//! of the 84 bodies changed turns this red instead of letting the port drift silently (MINEFIELD D1).

use std::collections::HashSet;

use pob_build::config::{APPLY_REGISTRY, registry_drift};
use pob_data::config_options::config_options;

#[test]
fn eighty_four_unique_entries_each_a_var_with_apply() {
    assert_eq!(APPLY_REGISTRY.len(), 84);
    let keys: HashSet<&str> = APPLY_REGISTRY.iter().map(|e| e.var).collect();
    assert_eq!(keys.len(), 84);
    let opts = config_options();
    let without: Vec<&str> = APPLY_REGISTRY
        .iter()
        .map(|e| e.var)
        .filter(|v| opts.var(v).and_then(|d| d.apply.as_ref()).is_none())
        .collect();
    assert!(without.is_empty(), "{without:?}");
}

#[test]
fn sha_from_to_match_json_84_of_84() {
    let drift = registry_drift(&APPLY_REGISTRY);
    assert!(drift.is_empty(), "{drift:#?}");
    println!("registry: {}/84 sha = config-options.json", APPLY_REGISTRY.len() - drift.len());
}

#[test]
fn a_tampered_sha_is_red() {
    let mut entries = APPLY_REGISTRY.to_vec();
    entries[10].sha = "0000000000000000000000000000000000000000000000000000000000000000";
    entries[40].to += 1;
    let drift = registry_drift(&entries);
    assert_eq!(drift.len(), 2, "{drift:#?}");
    assert!(drift[0].starts_with(entries[10].var));
    assert!(drift[1].starts_with(entries[40].var));
}

#[test]
fn registry_is_in_varlist_order() {
    let opts = config_options();
    let order: Vec<&str> = opts
        .vars
        .iter()
        .filter_map(|v| v.var.as_deref())
        .filter(|v| keys_contains(v))
        .collect();
    let ours: Vec<&str> = APPLY_REGISTRY.iter().map(|e| e.var).collect();
    assert_eq!(order, ours);
}

fn keys_contains(v: &str) -> bool {
    APPLY_REGISTRY.iter().any(|e| e.var == v)
}

#[test]
fn every_var_a_default_only_set_fires_is_covered() {
    let missing: Vec<&str> = config_options()
        .vars
        .iter()
        .filter(|v| v.apply.is_some() && (v.default_state.is_some() || v.default_index.is_some() || v.default_placeholder_state.is_some()))
        .filter_map(|v| v.var.as_deref())
        .filter(|v| !keys_contains(v))
        .collect();
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn whole_enemy_stats_section_is_covered() {
    let enemy: Vec<&str> = config_options()
        .vars
        .iter()
        .filter(|v| v.section == "Enemy Stats" && v.apply.is_some())
        .filter_map(|v| v.var.as_deref())
        .collect();
    assert_eq!(enemy.len(), 14);
    let missing: Vec<&&str> = enemy.iter().filter(|v| !keys_contains(v)).collect();
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn ported_bodies_add_up_to_537_lua_lines() {
    let lines: u32 = APPLY_REGISTRY.iter().map(|e| e.to - e.from + 1).sum();
    assert_eq!(lines, 537);
}

/// A var whose `apply` would fire but has no entry goes to `unimplemented` (no panic): the first `check` var
/// with an `apply` and no registry entry, set to true on a default set.
#[test]
fn unknown_var_with_value_is_unimplemented() {
    let opts = config_options();
    let var = opts
        .vars
        .iter()
        .find(|v| v.ty.as_deref() == Some("check") && v.apply.is_some() && v.var.as_deref().is_some_and(|n| !keys_contains(n)))
        .and_then(|v| v.var.clone())
        .unwrap();
    let mut s = pob_build::ConfigState::new();
    s.input.insert(var.clone(), pob_build::ConfigValue::Bool(true));
    let l = s.build_mod_list(90.0).unwrap();
    assert_eq!(l.unimplemented.len(), 1, "{:?}", l.unimplemented);
    assert!(l.unimplemented[0].starts_with("Modules/ConfigOptions.lua:") && l.unimplemented[0].ends_with(&format!("apply '{var}'")));
}
