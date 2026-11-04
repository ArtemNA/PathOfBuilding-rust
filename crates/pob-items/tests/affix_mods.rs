//! U11: the typed reading of the mod files says what the JSON tree says, in the same order, over every file `mod_file` reads.

use pob_data::wire::OrderedMap;
use pob_items::craft::AffixMod;

#[test]
fn typed_mods_equal_the_json_trees() {
    let dir = pob_data::data_dir().join("item-mods");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .expect("item-mods")
        .map(|e| e.expect("entry").path())
        .filter(|p| !p.ends_with("_index.json"))
        .collect();
    files.sort();
    files.push(pob_data::data_dir().join("veiled-mods.json"));
    let mut mods = 0;
    for path in &files {
        let text = std::fs::read_to_string(path).expect("read");
        let trees: OrderedMap<serde_json::Value> = serde_json::from_str(&text).expect("json");
        let typed: OrderedMap<AffixMod> = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(trees.keys().eq(typed.keys()), "{}: the order", path.display());
        for (id, tree) in trees.iter() {
            let mut m = typed[id].clone();
            m.id.clone_from(id);
            let want = AffixMod::from_json(id, tree);
            assert!(
                m == want && m.level.to_bits() == want.level.to_bits(),
                "{} {id}:\n  typed {m:?}\n  tree  {want:?}",
                path.display()
            );
            mods += 1;
        }
    }
    println!("{mods} mods of {} files", files.len());
    assert!(mods > 20_000);
}
