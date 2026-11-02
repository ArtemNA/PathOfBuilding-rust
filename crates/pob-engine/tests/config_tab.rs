//! U7 gate of the tab's mutators (`pob_engine::config_tab`), path equivalence (R8-ARCH §1.1): a write through the tab's way (`input[var] = v`, `BuildModList`, `buildFlag` -> `refresh`) ==
//! the same through `set_config` == a fresh load of what `SaveDB` writes, on every option type, on the set operations and on the custom modifiers; every scalar of `mainOutput` bit for bit.
//! The path against the original itself (stat box, placeholders, saved file): `crates/pob-ui/tests/config.rs`.

use pob_calc::StatValue;
use pob_calc::pob_build::{BlockText, CustomModsBlockModel, InputValue};
use pob_engine::config_tab::ConfigGate;
use pob_engine::engine::Engine;

fn golden(name: &str) -> String {
    let p = pob_golden::registry::find_golden(name).expect("golden").xml_path();
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

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

/// The tab's callback: the write, `BuildModList`, then the `buildFlag` frame.
fn tab_write(e: &mut Engine, var: &str, v: Option<InputValue>) {
    e.config_write_input(var, v);
    e.config_build_mod_list().expect("BuildModList");
    e.refresh().expect("refresh");
}

fn assert_reload_same(e: &Engine, what: &str) {
    let xml = e.save_xml().expect("save");
    let fresh = Engine::load_xml(&xml).expect("load");
    assert_eq!(bits(e), bits(&fresh), "{what}: the saved file's output");
    // the rest of SaveDB is no fixed point of load + save (MINEFIELD-RUST R18)
    let config = |x: &str| {
        let a = x.find("<Config").expect("Config");
        let b = x.find("</Config>").expect("/Config");
        x[a..b].to_string()
    };
    assert_eq!(config(&xml), config(&fresh.save_xml().expect("save")), "{what}: the saved set");
}

#[test]
fn every_option_type_equals_set_config_and_reload() {
    for build in ["OccVortex", "Dual Savior"] {
        let xml = golden(build);
        for (var, v) in [
            ("buffOnslaught", InputValue::Bool(true)),
            ("multiplierMapModEffect", InputValue::Num(25.0)),
            ("enemyPhysicalReduction", InputValue::Num(-5.0)),
            ("enemyFireResist", InputValue::Num(0.0)),
            ("enemyFireResist", InputValue::Num(40.0)),
            ("resistancePenalty", InputValue::Num(0.0)),
            ("enemyIsBoss", InputValue::Str("Pinnacle".into())),
            ("bandit", InputValue::Str("Alira".into())),
        ] {
            let what = format!("{build}: {var}={v:?}");
            let mut a = Engine::load_xml(&xml).expect("load");
            tab_write(&mut a, var, Some(v.clone()));
            let mut b = Engine::load_xml(&xml).expect("load");
            b.set_config(var, Some(v)).expect("set_config");
            b.refresh().expect("refresh");
            assert_eq!(bits(&a), bits(&b), "{what}: the tab's way vs set_config");
            assert_reload_same(&a, &what);
        }
        // a cleared number field falls back to the placeholder (ConfigTab.lua:309, :1147)
        let mut a = Engine::load_xml(&xml).expect("load");
        tab_write(&mut a, "enemyFireResist", Some(InputValue::Num(40.0)));
        tab_write(&mut a, "enemyFireResist", None);
        let mut b = Engine::load_xml(&xml).expect("load");
        b.clear_config("enemyFireResist").expect("clear");
        b.refresh().expect("refresh");
        assert_eq!(bits(&a), bits(&b), "{build}: a cleared field");
        assert_reload_same(&a, "a cleared field");
    }
}

/// The Lua's tables keep what `BuildModList` wrote: `presetBossSkills` leaves `input.enemyDamageType`, its "None" resets it (ConfigOptions.lua:2305, :2344-2349).
#[test]
fn boss_preset_is_path_dependent_as_in_the_lua() {
    let mut e = Engine::load_xml(&golden("OccVortex")).expect("load");
    tab_write(&mut e, "enemyDamageType", Some(InputValue::Str("Melee".into())));
    tab_write(&mut e, "presetBossSkills", Some(InputValue::Str("Atziri Flameblast".into())));
    assert_eq!(
        e.config_state().input.get("enemyDamageType"),
        Some(&InputValue::Str("Spell".into()))
    );
    assert!(!e.config_control_enabled("enemyDamageType"));
    assert_reload_same(&e, "preset on");
    tab_write(&mut e, "presetBossSkills", Some(InputValue::Str("None".into())));
    assert_eq!(
        e.config_state().input.get("enemyDamageType"),
        Some(&InputValue::Str("Average".into()))
    );
    assert!(e.config_control_enabled("enemyDamageType"));
    assert_reload_same(&e, "preset off");
}

#[test]
fn custom_mods_equal_reload() {
    let mut e = Engine::load_xml(&golden("OccVortex")).expect("load");
    let life = bits(&e);
    e.config_write_custom_mods(vec![
        CustomModsBlockModel {
            title: "A".into(),
            enabled: true,
            text: BlockText::Str("+100 to maximum Life\nnot a mod".into()),
        },
        CustomModsBlockModel {
            title: "B".into(),
            enabled: false,
            text: BlockText::Str("+1000 to maximum Life".into()),
        },
    ]);
    e.config_build_mod_list().expect("BuildModList");
    e.refresh().expect("refresh");
    assert_ne!(life, bits(&e), "the block applies");
    assert_reload_same(&e, "custom mods");
}

#[test]
fn set_operations_equal_reload() {
    let xml = golden("OccVortex");
    let mut e = Engine::load_xml(&xml).expect("load");
    let base = bits(&e);
    let first = e.config_active_set();
    // New: defaults, in the list only on Save
    let mut new = e.config_new_set();
    new.title = Some("Second".into());
    let id = new.id;
    assert!(!e.config_set_exists(id));
    e.config_insert_set(new);
    assert_eq!(e.config_set_order(), [first, id]);
    e.config_activate(id);
    e.config_build_mod_list().expect("BuildModList");
    e.refresh().expect("refresh");
    tab_write(&mut e, "buffOnslaught", Some(InputValue::Bool(true)));
    let second = bits(&e);
    assert_ne!(base, second);
    assert_reload_same(&e, "the second set");
    // back: the first set's output, bit for bit; the second keeps its input
    e.config_activate(first);
    e.config_build_mod_list().expect("BuildModList");
    e.refresh().expect("refresh");
    assert_eq!(base, bits(&e), "back to the first set");
    assert_reload_same(&e, "back");
    // copy, rename, delete
    let mut copy = e.config_copy_set(id);
    let copy_id = copy.id;
    copy.title = Some("Copy".into());
    e.config_insert_set(copy);
    e.config_activate(copy_id);
    e.config_build_mod_list().expect("BuildModList");
    e.refresh().expect("refresh");
    assert_eq!(second, bits(&e), "a copy of the second set");
    e.config_rename_set(copy_id, "Renamed");
    assert_eq!(e.config_set_title(copy_id).as_deref(), Some("Renamed"));
    assert_reload_same(&e, "the copy");
    e.config_write_order(&[first, id]);
    e.config_remove_set(copy_id);
    e.config_activate(first);
    e.config_build_mod_list().expect("BuildModList");
    e.refresh().expect("refresh");
    assert_eq!(base, bits(&e), "after the delete");
    assert_reload_same(&e, "after the delete");
}

#[test]
fn undo_state_restores_the_set() {
    let mut e = Engine::load_xml(&golden("OccVortex")).expect("load");
    let base = bits(&e);
    let state = e.config_undo_state();
    tab_write(&mut e, "bandit", Some(InputValue::Str("Oak".into())));
    assert_ne!(base, bits(&e));
    e.config_restore_undo(state);
    e.config_build_mod_list().expect("BuildModList");
    e.refresh().expect("refresh");
    assert_eq!(base, bits(&e));
}

/// The gates answer from the run: a recalculation gives the new answers (Calcs.lua:467-609).
#[test]
fn gates_follow_the_run() {
    let mut e = Engine::load_xml(&golden("OccVortex")).expect("load");
    assert!(e.config_gate(ConfigGate::Skill, "Vortex"));
    assert!(!e.config_gate(ConfigGate::Skill, "Fireball"));
    assert!(!e.config_gate(ConfigGate::Flag, "BossSkillActive"));
    tab_write(&mut e, "presetBossSkills", Some(InputValue::Str("Atziri Flameblast".into())));
    assert!(e.config_gate(ConfigGate::Flag, "BossSkillActive"));
}

/// Notes and the view mode go to the saved file.
#[test]
fn notes_and_view_mode_are_saved() {
    let mut e = Engine::load_xml(&golden("OccVortex")).expect("load");
    e.set_notes("^xE05030red");
    e.set_view_mode("NOTES");
    let fresh = Engine::load_xml(&e.save_xml().expect("save")).expect("load");
    assert_eq!(fresh.notes(), "^xE05030red");
    assert_eq!(fresh.model().view_mode.as_deref(), Some("NOTES"));
}
