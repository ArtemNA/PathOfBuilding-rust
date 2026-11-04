//! U8b: the readers of the Compare tab leave the engine as it was, and their results follow from the state.

use pob_calc::env::CalcOverride;
use pob_calc::power::find_power_stat;
use pob_engine::compare_tab::{model_groups_mut, tabulate_entry};
use pob_engine::engine::Engine;

fn golden(name: &str) -> Option<String> {
    let up = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/upstream.json")).ok()?;
    let up: serde_json::Value = serde_json::from_str(&up).ok()?;
    std::fs::read_to_string(format!("{}/spec/TestBuilds/3.13/{name}.xml", up["pobRoot"].as_str()?)).ok()
}

#[test]
fn readers_do_not_touch_the_engine() {
    let Some(xml) = golden("OccVortex") else {
        eprintln!("SKIPPED: no upstream checkout");
        return;
    };
    let e = Engine::load_xml(&xml).expect("load");
    let before = (e.output_json(), e.save_xml().expect("save"));
    let stat = find_power_stat("TotalDPS").expect("stat");
    // an empty edit and an empty override change nothing
    assert_eq!(e.compare_edited_impact(|_| {}, &stat).expect("edited"), 0.0);
    assert_eq!(e.compare_override_impact(CalcOverride::default(), &stat).expect("override"), 0.0);
    assert!(e.compare_fresh_lines(CalcOverride::default(), "h").expect("fresh").is_empty());
    let groups = e.compare_group_models().expect("groups");
    assert_eq!(groups.len(), e.socket_groups().len());
    // a second copy of the main group adds its skills to nothing the main skill reads through an edit of a clone
    let _ = e.compare_edited_impact(
        |m| {
            let g = model_groups_mut(m).expect("groups");
            let first = g[0].clone();
            g.push(first);
        },
        &stat,
    );
    assert_eq!(before, (e.output_json(), e.save_xml().expect("save")));
}

#[test]
fn config_impact_comes_back() {
    let Some(xml) = golden("OccVortex") else {
        eprintln!("SKIPPED: no upstream checkout");
        return;
    };
    let e = Engine::load_xml(&xml).expect("load");
    let stat = find_power_stat("TotalDPS").expect("stat");
    let before = e.output_json();
    let on = e
        .compare_config_impact("useFrenzyCharges", Some(pob_calc::pob_build::InputValue::Bool(true)), &stat)
        .expect("impact");
    assert!(on > 0.0, "Frenzy Charges raise the hit DPS of this build: {on}");
    assert_eq!(before, e.output_json());
    assert!(e.config_state().input.get("useFrenzyCharges").is_none());
}

#[test]
fn stage_default_of_an_entry_and_the_tabulation() {
    let Some(xml) = golden("Dual Savior") else {
        eprintln!("SKIPPED: no upstream checkout");
        return;
    };
    let e = Engine::load_xml(&xml).expect("load");
    let a = e.compare_skill_select(false).expect("select");
    let b = e.skill_select().expect("select");
    assert_eq!(a.groups, b.groups);
    assert_eq!(a.main_group, b.main_group);
    // the rows of the mod cells
    let run = e.calcs_run();
    let actor = pob_engine::calcs::player_actor(&run.env, &run.state.player).expect("actor");
    let tabulated = pob_engine::calcs::sections()
        .iter()
        .flat_map(|s| &s.sub_sections)
        .flat_map(|s| &s.rows)
        .flat_map(|r| &r.cells)
        .flat_map(|c| &c.entries)
        .filter(|en| !en.mod_name.is_empty())
        .any(|en| !tabulate_entry(en, &actor).is_empty());
    assert!(tabulated);
    assert_eq!(e.compare_breakdown_lines("NoSuchBreakdown"), None);
}
