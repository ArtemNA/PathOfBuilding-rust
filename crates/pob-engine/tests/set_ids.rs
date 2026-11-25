//! 2.69.0 takes the id of a new / copied set as `#self.xSets + 1` (`#t` of a LuaJIT table with holes: a border its layout chooses),
//! `ItemsTab:CreateItemSet(nil)` as the first free id. The sequences below are what the original did on the same build and the same
//! steps: `tools/sg-oracle/scenes/config/u69-sets.json` (config-runner.lua; the `sets` dumps of `fixtures/config/u69-sets.json`, steps
//! 16-72 / 85-134 / 147-183) -- the UI gate `crates/pob-ui/tests/config.rs` compares the whole scene; this one runs the engine alone.
use pob_engine::engine::Engine;

fn golden(name: &str) -> String {
    let p = pob_golden::registry::find_golden(name).expect("golden").xml_path();
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The "Loadouts Test" build of config-runner.lua: three sets of each kind, ids 1-3 (the tree does not matter here).
fn loadouts_test() -> String {
    let x = golden("OccVortex");
    let a = x.find("<Skills ").expect("Skills");
    let a = a + x[a..].find('>').expect(">") + 1;
    let b = x.find("</Skills>").expect("/Skills");
    let x = format!(
        "{}<SkillSet id=\"1\">{}</SkillSet><SkillSet id=\"2\" title=\"Alt\"/><SkillSet id=\"3\" title=\"Third\"/>{}",
        &x[..a],
        &x[a..b],
        &x[b..]
    );
    let x = x.replacen(
        "</Items>",
        "<ItemSet id=\"2\" title=\"Alt\"/><ItemSet id=\"3\" title=\"Third\"/></Items>",
        1,
    );
    let a = x.find("<Config").expect("Config");
    let b = a + x[a..].find("</Config>").expect("/Config") + "</Config>".len();
    format!(
        "{}<Config activeConfigSet=\"1\"><ConfigSet id=\"1\" title=\"Default\"/><ConfigSet id=\"2\" title=\"Alt\"/><ConfigSet id=\"3\" title=\"Third\"/></Config>{}",
        &x[..a],
        &x[b..]
    )
}

fn item_ids(e: &Engine) -> Vec<f64> {
    e.item_sets().iter().map(|s| s.id).collect()
}

#[test]
fn item_set_ids_after_a_hole_equal_the_original() {
    let mut e = Engine::load_xml(&loadouts_test()).expect("load");
    assert_eq!(e.copy_item_set(2.0, " C"), 4.0);
    assert_eq!(e.new_item_set(Some("!"), None), 5.0);
    let undo = e.items_undo_state();
    e.delete_item_set(5.0, 5);
    e.delete_item_set(2.0, 2);
    assert_eq!(item_ids(&e), [1.0, 3.0, 4.0]);
    // `#itemSets` of {1, 3, 4}: 4 (oracle step 40)
    assert_eq!(e.item_sets_len(), 4);
    // `CreateItemSet(nil)`: the first free id
    assert_eq!(e.new_item_set(Some("3"), None), 2.0);
    // `CopyItemSet`: `#t + 1` of {1, 2, 3, 4} (oracle step 52)
    assert_eq!(e.copy_item_set(1.0, "4"), 5.0);
    assert_eq!(item_ids(&e), [1.0, 3.0, 4.0, 2.0, 5.0]);
    // the undo's `self.itemSets = state.itemSets` (a copy in `pairs` order), then the next copy (oracle step 72: 6 of a table of 5)
    let state = e.items_undo_state();
    e.copy_item_set(1.0, "x");
    e.restore_items_undo_state(&state).expect("undo");
    assert_eq!(e.copy_item_set(1.0, "U"), 6.0);
    // an older state: {1..5} with 5 the active one
    e.restore_items_undo_state(&undo).expect("undo");
    assert_eq!(e.item_sets_len(), 5);
}

/// `NewLoadout`'s `NewItemSet(#self.itemsTab.itemSets + 1, name)` with a hole at 2 of {1..5}: 6 (u69-loadouts.json, steps 35-40), where the
/// first free id (2.68's `NewItemSet()`) would be 2.
#[test]
fn item_set_id_over_a_hole_is_a_border() {
    let mut e = Engine::load_xml(&loadouts_test()).expect("load");
    assert_eq!(e.copy_item_set(2.0, "4"), 4.0);
    assert_eq!(e.copy_item_set(2.0, "5"), 5.0);
    e.delete_item_set(2.0, 2);
    assert_eq!(e.item_sets_len(), 5);
    assert_eq!(e.new_item_set_with_id((e.item_sets_len() + 1) as f64, Some("6")), 6.0);
    assert_eq!(e.copy_item_set(1.0, "7"), 7.0);
}

#[test]
fn skill_set_ids_after_a_hole_equal_the_original() {
    let mut e = Engine::load_xml(&loadouts_test()).expect("load");
    assert_eq!(e.copy_skill_set(2.0, " C").expect("copy"), 4.0);
    assert_eq!(e.new_skill_set("!").expect("new"), 5.0);
    e.set_active_skill_set(1.0).expect("active");
    e.delete_skill_set(5.0, 5).expect("delete");
    e.delete_skill_set(2.0, 2).expect("delete");
    assert_eq!(e.skill_sets().iter().map(|s| s.0).collect::<Vec<_>>(), [1.0, 3.0, 4.0]);
    assert_eq!(e.skill_sets_len(), 4);
    // `CreateSkillSet(nil)`: `#t + 1` (oracle step 114), where the items take the first free id
    assert_eq!(e.new_skill_set("3").expect("new"), 5.0);
    let state = e.skills_undo_state();
    assert_eq!(e.copy_skill_set(1.0, "4").expect("copy"), 6.0);
    // the undo's `wipeTable(self.skillSets)` + the state's keys (oracle steps 124-134)
    e.restore_skills_undo_state(&state).expect("undo");
    assert_eq!(e.skill_sets_len(), 5);
    assert_eq!(e.copy_skill_set(1.0, "U").expect("copy"), 6.0);
}

#[test]
fn config_set_ids_after_a_hole_equal_the_original() {
    let mut e = Engine::load_xml(&loadouts_test()).expect("load");
    assert_eq!(e.config_copy_set(2.0, " C"), 4.0);
    assert_eq!(e.config_new_set(None, Some("!")), 5.0);
    e.config_delete_set(5.0, 5);
    e.config_delete_set(2.0, 2);
    assert_eq!(e.config_set_order(), [1.0, 3.0, 4.0]);
    assert_eq!(e.config_sets_len(), 4);
    // oracle steps 176, 183
    assert_eq!(e.config_new_set(None, Some("3")), 5.0);
    assert_eq!(e.config_copy_set(1.0, "4"), 6.0);
}
