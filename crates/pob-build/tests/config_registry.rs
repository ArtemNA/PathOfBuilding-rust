//! The drift guard on `pob_build::config::apply` (pob-ts `test/config/registry.test.ts`): every ported closure carries the
//! sha256 of the upstream `apply` body as `data/config-options.json` recorded it; a re-converted PathOfBuilding where one
//! of the 539 bodies changed turns this red instead of letting the port drift silently (MINEFIELD D1). Since R3.5b the
//! registry is complete: 10 hand-ported (`HAND_PORTED`: `enemyIsBoss`, `presetBossSkills` + the 8 map-affix dropdowns) + 529 generated
//! (`GENERATED`, `tools/config-apply-gen/gen.ts`); the 41 `AffixData[x].apply` of Data/ModMap.lua have their own guard.

use std::collections::HashSet;

use pob_build::config::{APPLY_REGISTRY, GENERATED, HAND_PORTED, MAP_AFFIX_APPLY, map_affix_drift, map_affixes_with_apply, registry_drift};
use pob_data::config_options::config_options;

fn keys_contains(v: &str) -> bool {
    APPLY_REGISTRY.iter().any(|e| e.var == v)
}

#[test]
fn all_539_unique_entries_each_a_var_with_apply() {
    assert_eq!(HAND_PORTED.len(), 10);
    assert_eq!(GENERATED.len(), 529);
    assert_eq!(APPLY_REGISTRY.len(), 539);
    let keys: HashSet<&str> = APPLY_REGISTRY.iter().map(|e| e.var).collect();
    assert_eq!(keys.len(), 539);
    let opts = config_options();
    let without: Vec<&str> = APPLY_REGISTRY
        .iter()
        .map(|e| e.var)
        .filter(|v| opts.var(v).and_then(|d| d.apply.as_ref()).is_none())
        .collect();
    assert!(without.is_empty(), "{without:?}");
    // and the converse: every var with an `apply` has an entry
    let missing: Vec<&str> = opts
        .vars
        .iter()
        .filter(|v| v.apply.is_some())
        .filter_map(|v| v.var.as_deref())
        .filter(|v| !keys_contains(v))
        .collect();
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn sha_from_to_match_json_539_of_539() {
    let drift = registry_drift(&APPLY_REGISTRY);
    assert!(drift.is_empty(), "{drift:#?}");
    println!("registry: {}/539 sha = config-options.json", APPLY_REGISTRY.len() - drift.len());
}

#[test]
fn a_tampered_sha_is_red() {
    let mut entries = APPLY_REGISTRY.to_vec();
    entries[10].sha = "0000000000000000000000000000000000000000000000000000000000000000";
    entries[40].to += 1;
    entries[400].from -= 1;
    let drift = registry_drift(&entries);
    assert_eq!(drift.len(), 3, "{drift:#?}");
    assert!(drift[0].starts_with(entries[10].var));
    assert!(drift[1].starts_with(entries[40].var));
    assert!(drift[2].starts_with(entries[400].var));
}

#[test]
fn registry_is_in_varlist_order() {
    let order: Vec<&str> = config_options()
        .vars
        .iter()
        .filter(|v| v.apply.is_some())
        .filter_map(|v| v.var.as_deref())
        .collect();
    let ours: Vec<&str> = APPLY_REGISTRY.iter().map(|e| e.var).collect();
    assert_eq!(order, ours);
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

/// What stays by hand: the two bodies outside the generator's grammar and the 8 dropdowns of `mapAffixDropDownFunction`.
#[test]
fn hand_ported_are_the_bodies_outside_the_grammar() {
    assert_eq!(HAND_PORTED[0].var, "enemyIsBoss");
    assert_eq!(HAND_PORTED[1].var, "presetBossSkills");
    assert!(
        HAND_PORTED[2..]
            .iter()
            .all(|e| e.var.starts_with("Map") && (e.from, e.to) == (117, 130))
    );
}

/// Every var a set can hold now has its closure: a default set with every `check` var on fires all 322 of them (plus the
/// defaults) and reports nothing as unimplemented.
#[test]
fn nothing_is_unimplemented_any_more() {
    let opts = config_options();
    let mut s = pob_build::ConfigState::new();
    let mut checks = 0;
    for v in opts.vars.iter().filter(|v| v.ty.as_deref() == Some("check") && v.apply.is_some()) {
        s.input.insert(v.var.clone().unwrap(), pob_build::ConfigValue::Bool(true));
        checks += 1;
    }
    assert_eq!(checks, 322);
    let l = s.build_mod_list(90.0).unwrap();
    assert!(l.unimplemented.is_empty(), "{:?}", l.unimplemented);
}

#[test]
fn map_affix_closures_match_closures_json_41_of_41() {
    assert_eq!(MAP_AFFIX_APPLY.len(), 41);
    let drift = map_affix_drift(&MAP_AFFIX_APPLY);
    assert!(drift.is_empty(), "{drift:#?}");
    // every `AffixData` entry with an `apply` is ported, and nothing else
    let mut want = map_affixes_with_apply();
    want.sort();
    let mut ours: Vec<String> = MAP_AFFIX_APPLY.iter().map(|e| e.name.to_owned()).collect();
    ours.sort();
    assert_eq!(want, ours);
    let mut tampered = MAP_AFFIX_APPLY.to_vec();
    tampered[3].sha = "0000000000000000000000000000000000000000000000000000000000000000";
    assert_eq!(map_affix_drift(&tampered).len(), 1);
}
