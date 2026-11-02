//! U4: the mutators of the Calcs tab -- a mutation through the API = a fresh load of what `SaveDB` writes (R8-ARCH §1.1), and the `Calcs` skill select follows them.

use pob_build::InputValue;
use pob_engine::calcs_tab::{BdEnv, BdSection, SlotOrders};
use pob_engine::engine::Engine;

fn golden(name: &str) -> Option<String> {
    let up = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/upstream.json")).ok()?;
    let up: serde_json::Value = serde_json::from_str(&up).ok()?;
    std::fs::read_to_string(format!("{}/spec/TestBuilds/3.13/{name}.xml", up["pobRoot"].as_str()?)).ok()
}

/// The output of both runs after a mutation = the output of a fresh load of the saved state.
fn assert_path_equivalent(e: &mut Engine, what: &str) {
    e.refresh().expect("refresh");
    let xml = e.save_xml().expect("save");
    let fresh = Engine::load_xml(&xml).expect("reload");
    assert_eq!(e.output_json(), fresh.output_json(), "{what}: MAIN");
    let out = |e: &Engine| format!("{:?}", e.calcs_run().state.player.output.get("TotalDPS"));
    assert_eq!(out(e), out(&fresh), "{what}: CALCS");
    assert_eq!(e.calcs_input(), fresh.calcs_input(), "{what}: input");
}

#[test]
fn calcs_mutators_are_path_equivalent() {
    let Some(xml) = golden("Generals Perforate Zerker") else {
        eprintln!("SKIPPED: no upstream checkout");
        return;
    };
    let mut e = Engine::load_xml(&xml).expect("load");
    e.set_calcs_input("misc_buffMode", Some(InputValue::Str("BUFFED".into())));
    assert_path_equivalent(&mut e, "misc_buffMode");
    assert_eq!(e.calcs_input().get("misc_buffMode"), Some(&InputValue::Str("BUFFED".into())));
    let groups = e.skill_select_calcs().expect("select").groups.len();
    assert!(groups > 1);
    e.set_calcs_input("skill_number", Some(InputValue::Num(2.0)));
    assert_path_equivalent(&mut e, "skill_number");
    assert_eq!(e.skill_select_calcs().expect("select").main_group, 2.0);
    e.set_main_active_skill_calcs(1.0).expect("main active skill");
    assert_path_equivalent(&mut e, "mainActiveSkillCalcs");
    e.set_skill_stage_count_calcs(Some(3.0)).expect("stages");
    assert_path_equivalent(&mut e, "skillStageCountCalcs");
    e.set_skill_mine_count_calcs(Some(2.0)).expect("mines");
    assert_path_equivalent(&mut e, "skillMineCountCalcs");
    e.set_skill_part_calcs(1.0).expect("part");
    assert_path_equivalent(&mut e, "skillPartCalcs");
    e.set_skill_minion_skill_calcs(1.0).expect("minion skill");
    assert_path_equivalent(&mut e, "skillMinionSkillCalcs");
    e.set_calcs_input("showMinion", Some(InputValue::Bool(true)));
    assert!(e.calcs_show_minion());
    let state = e.calcs_input();
    e.set_calcs_input("showMinion", Some(InputValue::Bool(false)));
    e.restore_calcs_input(&state);
    assert!(e.calcs_show_minion(), "RestoreUndoState");
}

/// OccVortex with a spectre group and two spectres in the library (the same edit as the sandbox of `crates/pob-ui/tests/calcs.rs`).
#[test]
fn minion_mutator_is_path_equivalent() {
    let Some(xml) = golden("OccVortex") else {
        eprintln!("SKIPPED: no upstream checkout");
        return;
    };
    let build_end = xml.find("<Build ").expect("Build") + xml[xml.find("<Build ").expect("Build")..].find('>').expect(">") + 1;
    let mut x = xml.clone();
    x.insert_str(
        build_end,
        "<Spectre id=\"Metadata/Monsters/Axis/AxisCaster\"/><Spectre id=\"Metadata/Monsters/DemonModular/DemonModularBladeVortex\"/>",
    );
    let skills = x.find("<Skills ").expect("Skills");
    let skills_end = skills + x[skills..].find('>').expect(">") + 1;
    x.insert_str(
        skills_end,
        "<Skill mainActiveSkillCalcs=\"1\" enabled=\"true\" mainActiveSkill=\"1\"><Gem level=\"20\" gemId=\"Metadata/Items/Gems/SkillGemRaiseSpectre\" skillId=\"RaiseSpectre\" qualityId=\"Default\" quality=\"0\" enabled=\"true\" nameSpec=\"Raise Spectre\"/></Skill>",
    );
    let mut e = Engine::load_xml(&x).expect("load");
    e.set_calcs_input("skill_number", Some(InputValue::Num(1.0)));
    assert_path_equivalent(&mut e, "skill_number");
    let list = e
        .skill_select_calcs()
        .expect("select")
        .main
        .expect("main")
        .minion
        .expect("minion")
        .list;
    assert!(list.len() >= 2, "{list:?}");
    e.set_skill_minion_calcs(&list[1]).expect("minion");
    assert_path_equivalent(&mut e, "skillMinionCalcs");
    match (
        &e.skill_select_calcs()
            .expect("select")
            .main
            .expect("main")
            .minion
            .expect("minion")
            .sel,
        &list[1],
    ) {
        (pob_engine::shell::MinionSel::Minion(Some(a)), pob_engine::shell::MinionEntry::Minion { minion_id, .. }) => {
            assert_eq!(a, minion_id)
        }
        other => panic!("{other:?}"),
    }
}

/// CalcSetup.lua:783, :1410: `env.allocNodes` of a run after a tree mutator is a table the `pairs` model can answer for.
#[test]
fn tree_mutator_then_reload_keeps_the_alloc_order_model() {
    // no granted passive here: `env.allocNodes` is the plain copy
    let Some(xml) = golden("Dual Savior") else {
        eprintln!("SKIPPED: no upstream checkout");
        return;
    };
    let mut e = Engine::load_xml(&xml).expect("load");
    let id = e
        .spec_rc()
        .nodes
        .iter()
        .find(|n| !n.alloc && n.dn() == "Life")
        .map(|n| n.id)
        .expect("a free node");
    e.alloc_node(id, None).expect("alloc");
    let live = e.spec_rc().alloc_table_pairs();
    e.refresh().expect("refresh");
    // the live object's history decides, as `copyTable(spec.allocNodes)` of the Lua (the `edit` scene of pob-ui checks it against LuaJIT)
    let mut copy = pob_tree::lj_table::LjTable::new();
    for k in &live {
        copy.set(f64::from(*k));
    }
    let want: Vec<u32> = copy.pairs_keys().into_iter().map(|k| k as u32).collect();
    let run: Vec<u32> = e.debug_alloc_orders().0.into_iter().filter(|i| live.contains(i)).collect();
    let want: Vec<u32> = want.into_iter().filter(|i| run.contains(i)).collect();
    assert_eq!(run, want);
    // a reload of what it saves: another history, the same numbers
    let fresh = Engine::load_xml(&e.save_xml().expect("save")).expect("reload");
    assert_eq!(e.output_json(), fresh.output_json());
    let sorted = |mut v: Vec<u32>| {
        v.sort_unstable();
        v
    };
    assert_eq!(sorted(e.debug_alloc_orders().0), sorted(fresh.debug_alloc_orders().0));
}

#[test]
fn collapsed_is_saved_and_loaded() {
    let Some(xml) = golden("OccVortex") else {
        eprintln!("SKIPPED: no upstream checkout");
        return;
    };
    let mut e = Engine::load_xml(&xml).expect("load");
    e.set_calcs_collapsed("Attributes", "Attributes", true);
    let again = Engine::load_xml(&e.save_xml().expect("save")).expect("reload");
    assert_eq!(again.calcs_collapsed("Attributes", "Attributes"), Some(true));
}

#[test]
fn breakdown_sections_of_a_mod_cell() {
    let Some(xml) = golden("OccVortex") else {
        eprintln!("SKIPPED: no upstream checkout");
        return;
    };
    let e = Engine::load_xml(&xml).expect("load");
    let cell = pob_engine::calcs::sections()
        .iter()
        .find(|s| s.id == "Attributes")
        .and_then(|s| s.sub_sections[0].rows.iter().find(|r| r.label.as_deref() == Some("Strength")))
        .map(|r| r.cells[0].entries.clone())
        .expect("Strength");
    for env in [BdEnv::Calcs, BdEnv::Main] {
        let mut orders = SlotOrders::new();
        let secs = e.breakdown_sections(env, None, &cell, &mut orders).expect("sections");
        assert!(
            secs.iter().any(|s| matches!(s, BdSection::Table { rows, .. } if !rows.is_empty())),
            "{env:?}"
        );
    }
}
