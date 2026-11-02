//! Loader counts against pob-ts (`test/convert/*.test.ts`, `KNOWLEDGE/DATA-INVENTORY.md`) and the `data.lock` check.

use std::path::PathBuf;

use pob_data::wire::LuaTable;
use pob_data::*;

#[test]
fn lock_matches_the_synced_data() {
    let dir = data_dir();
    let info = verify_lock(&dir, &lock_path(&dir)).expect("data.lock matches data/manifest.json");
    // the upstream of tools/upstream.json (R-UP)
    let cfg: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/upstream.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(info.pob_sha, cfg["pobSha"].as_str().unwrap());
    assert!(ensure_data().is_ok());
}

/// A tampered lock (or manifest) is a start-up error naming both sides.
#[test]
fn lock_mismatch_is_an_error() {
    let tmp = std::env::temp_dir().join(format!("pob-data-lock-{}", std::process::id()));
    let data = tmp.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::copy(data_dir().join("manifest.json"), data.join("manifest.json")).unwrap();
    let real: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(lock_path(&data_dir())).unwrap()).unwrap();
    let write_lock = |sha: &str| {
        let mut l = real.clone();
        l["manifestSha256"] = serde_json::Value::String(sha.to_owned());
        std::fs::write(tmp.join("data.lock"), serde_json::to_string(&l).unwrap()).unwrap();
    };
    let lock: PathBuf = lock_path(&data);
    write_lock(real["manifestSha256"].as_str().unwrap());
    assert!(verify_lock(&data, &lock).is_ok(), "an untouched copy passes");
    write_lock("0000000000000000000000000000000000000000000000000000000000000000");
    let e = verify_lock(&data, &lock).unwrap_err();
    assert!(e.0.contains("sync-data"), "{e}");
    // the manifest changed under an intact lock
    write_lock(real["manifestSha256"].as_str().unwrap());
    let mut m = std::fs::read(data.join("manifest.json")).unwrap();
    m.push(b'\n');
    std::fs::write(data.join("manifest.json"), m).unwrap();
    assert!(verify_lock(&data, &lock).is_err());
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn mod_cache_counts() {
    let c = mod_cache::mod_cache();
    let (mut both, mut mods_only, mut extra_only, mut empty, mut mods_total) = (0, 0, 0, 0, 0);
    for e in &c.entries {
        match (&e.mods, &e.extra) {
            (Some(m), Some(_)) => {
                both += 1;
                mods_total += m.len();
            }
            (Some(m), None) => {
                mods_only += 1;
                mods_total += m.len();
            }
            (None, Some(_)) => extra_only += 1,
            (None, None) => empty += 1,
        }
    }
    // 3dbca4463 (R-UP): 23 207 -> 23 201 entries (grep -c '^c["' Data/ModCache.lua; the categories counted from data/mod-cache.json by node)
    assert_eq!(c.len(), 23_201);
    assert_eq!((both, mods_only, extra_only, empty), (1096, 19_299, 2785, 21));
    assert_eq!(mods_total, 23_334);
    assert_eq!(c.malformed_dropped, 0);
    let orphan_records = c.entries.iter().filter(|e| !e.orphan_tags.is_empty()).count();
    assert_eq!(orphan_records, 3);
    let e = c
        .get("Hits have 15% chance to ignore Enemy Physical Damage Reduction while you have Sacrificial Zeal")
        .expect("orphan record");
    let m = &e.mods.as_ref().unwrap()[0];
    assert!(m.tags.is_empty(), "the orphan is not in tags (L9)");
    assert_eq!(e.orphan_tags[0].0, 0);
    assert_eq!(e.orphan_tags[0].1.index, 2);
    assert_eq!(e.orphan_tags[0].1.tag.get("var").as_str(), Some("SacrificialZeal"));
    // " Wolf Alpha Talisman" left Data/ModCache.lua in 3dbca4463; another `{nil, extra}` record (ModCache.lua:4)
    let w = c.get("(10-15)% increased Energy Shield Recharge Rate").unwrap();
    assert!(w.mods.is_none());
    assert_eq!(w.extra.as_deref(), Some("(10-15)% increased Energy Shield Recharge Rate "));
}

#[test]
fn item_bases_counts() {
    let b = item_bases::item_bases();
    assert_eq!(b.bases.len(), 1106);
    assert_eq!(b.lists.len(), 70);
    assert_eq!(b.type_list.len(), 70);
    assert_eq!(b.lists.values().map(Vec::len).sum::<usize>(), 1098);
    assert_eq!(b.bases.values().filter(|x| x.hidden == Some(true)).count(), 8);
    assert_eq!(
        b.base("Paua Amulet").unwrap().implicit.as_deref(),
        Some("(20-30)% increased Mana Regeneration Rate")
    );
    assert_eq!(b.base("Rusted Sword").unwrap().socket_limit, Some(3.0));
    assert!(b.base("constructor").is_none());
}

#[test]
fn config_options_counts() {
    let c = config_options::config_options();
    let typed = c.vars.iter().filter(|v| v.ty.is_some()).count();
    assert_eq!((typed, c.vars.len() - typed, c.vars.len()), (586, 93, 679));
    let count = |f: fn(&config_options::ConfigVar) -> bool| c.vars.iter().filter(|v| f(v)).count();
    assert_eq!(count(|v| v.apply.is_some()), 539);
    assert_eq!(count(|v| v.list.is_some()), 41);
    assert_eq!(count(|v| v.default_index.is_some()), 13);
    assert_eq!(count(|v| v.default_state.is_some()), 15);
    assert_eq!(count(|v| v.default_placeholder_state.is_some()), 17);
    assert_eq!(count(|v| v.tooltip_func.is_some()), 12);
    assert_eq!(count(|v| v.malformed.is_some()), 2);
    let boss = c.var("enemyIsBoss").unwrap();
    assert_eq!(boss.default_index, Some(3.0));
    assert_eq!(boss.list.as_ref().unwrap()[2].val, wire::Scalar::Str("Pinnacle".into()));
    assert_eq!(
        c.var("conditionLeeching").unwrap().gates.get("ifCond").and_then(|v| v.as_str()),
        Some("Leeching")
    );
}

#[test]
fn gems_counts() {
    let g = gems::gems();
    assert_eq!(g.gems.len(), 871);
    assert_eq!(g.gems.keys().filter(|k| k.ends_with("AltX")).count(), 190);
    assert_eq!(g.gems.keys().filter(|k| k.ends_with("AltY")).count(), 72);
    assert_eq!(g.gems.values().filter(|x| x.vaal_gem == Some(true)).count(), 101);
    assert_eq!(g.gem_for_skill.len(), 822);
    assert_eq!(g.gem_for_base_name.len(), 872);
    assert_eq!(g.gems_by_game_id.len(), 608);
    assert_eq!(g.gem_granted_effect_id_for_vaal_gem_id.len(), 101);
    assert_eq!(g.gem_vaal_gem_id_for_base_gem_id.len(), 101);
    let fb = &g.gems["Metadata/Items/Gems/SkillGemFireball"];
    assert_eq!((fb.name.as_str(), fb.req_int, fb.natural_max_level), ("Fireball", 100.0, 20.0));
}

#[test]
fn constants_counts() {
    let c = constants::constants();
    assert_eq!(
        (
            c.misc.len(),
            c.character_constants.len(),
            c.monster_constants.len(),
            c.game_constants.len()
        ),
        (60, 89, 57, 42)
    );
    for t in [
        &c.monster_accuracy_table,
        &c.monster_ailment_threshold_table,
        &c.monster_ally_damage_table,
        &c.monster_ally_life_table,
        &c.monster_armour_table,
        &c.monster_damage_table,
        &c.monster_evasion_table,
        &c.monster_life_table,
        &c.monster_phys_conversion_multi_table,
        &c.gold_respec_prices,
    ] {
        assert_eq!(t.len(), 100);
    }
    assert_eq!(c.monster_experience_level_map.len(), 87);
    assert_eq!((c.map_level_boss_ailment_mult.len(), c.map_level_boss_life_mult.len()), (90, 90));
    assert_eq!(
        (
            c.map_level_life_mult.len(),
            c.monster_variety_life_mult.len(),
            c.totem_life_mult.len()
        ),
        (25, 252, 20)
    );
    assert_eq!(constants::monster_table_at(&c.monster_accuracy_table, 83.0), Some(518.0));
    assert_eq!(constants::monster_table_at(&c.monster_life_table, 100.0), Some(41_817.0));
    assert_eq!(constants::monster_table_at(&c.monster_life_table, 101.0), None);
    assert_eq!(c.misc["ResistFloor"], -200.0);
    assert_eq!(c.misc["DotDpsCap"], 35_791_394.0);
}

#[test]
fn rules_globals_pantheons_counts() {
    let r = rules::rules();
    assert_eq!(
        (r.weapon_type_info.len(), r.unarmed_weapon_data.len(), r.non_damaging_ailment.len()),
        (15, 7, 6)
    );
    assert_eq!(
        (
            r.ailment_type_list.len(),
            r.elemental_ailment_type_list.len(),
            r.non_damaging_ailment_type_list.len(),
            r.non_elemental_ailment_type_list.len()
        ),
        (9, 7, 6, 2)
    );
    assert_eq!(
        (
            r.high_precision_mods.len(),
            r.curse_priority.len(),
            r.keystones.len(),
            r.jewel_radii.len()
        ),
        (39, 26, 58, 2)
    );
    assert_eq!((r.item_tag_special.len(), r.item_tag_special_exclusion_pattern.len()), (3, 3));
    assert_eq!(
        (r.vestigial_mod_mappings.len(), r.boss_stats.len(), r.timeless_jewel_additions),
        (472, 4, 337.0)
    );
    assert_eq!(r.power_stat_list.as_array().map(Vec::len), Some(85));
    assert_eq!(r.high_precision_mods["SupportManaMultiplier"]["MORE"], 4.0);
    assert_eq!(r.unarmed_weapon_data_for(0.0).unwrap().ty, "None");
    let g = globals::globals();
    assert_eq!(
        (g.mod_flag.len(), g.keyword_flag.len(), g.skill_type.len(), g.color_codes.len()),
        (27, 30, 135, 68)
    );
    assert_eq!(g.mod_flag["WeaponMask"], f64::from(0x2fff_0000u32));
    assert_eq!(g.skill_type["Spell"], 2.0);
    let p = pantheons::pantheons();
    assert_eq!(p.len(), 12);
    assert_eq!(p["Arakaali"].souls[0].name, "Soul of Arakaali");
}

#[test]
fn cluster_jewels_and_scalability_counts() {
    let c = cluster_jewels::cluster_jewels();
    assert_eq!((c.jewels.len(), c.keystones.len(), c.notable_sort_order.len()), (3, 8, 300));
    assert_eq!(c.orbit_offsets.as_object().map(|o| o.len()), Some(42));
    assert_eq!(c.info_for_notable.as_object().map(|o| o.len()), Some(287));
    let small = c.jewel("Small Cluster Jewel").unwrap();
    assert_eq!((small.min_nodes, small.max_nodes, small.total_indicies), (2.0, 3.0, 6.0));
    assert_eq!(mod_scalability::mod_scalability().len(), 15_202);
}

#[test]
fn trees_by_version() {
    // (version, nodes incl. a modern "root", groups is an array, classes is an array)
    for (v, nodes, groups_array, classes_array) in [
        ("3_6", 1862, true, false),
        ("3_12", 2699, true, true),
        ("3_13", 2707, true, true),
        ("3_29", 3397, false, true),
    ] {
        let t = tree::load_tree(v).unwrap();
        assert_eq!(t.nodes.len(), nodes, "{v} nodes");
        assert_eq!(t.groups.is_array(), groups_array, "{v} groups shape (D16)");
        assert_eq!(t.classes.is_array(), classes_array, "{v} classes shape (D16)");
        assert!(std::ptr::eq(t, tree::load_tree(v).unwrap()), "parsed once");
    }
    let t13 = tree::load_tree("3_13").unwrap();
    assert!(t13.nodes["root"].skill.is_none(), "modern root has no skill");
    let n = t13.nodes.values().find(|n| n.skill == Some(31_472.0)).unwrap();
    assert_eq!((n.name.as_deref(), n.granted_intelligence), (Some("Intelligence"), Some(10.0)));
    assert_eq!(t13.class(1).unwrap().name, "Marauder"); // classes 0-based (D16)
    assert_eq!(t13.class(0).unwrap().name, "Scion");
    let t6 = tree::load_tree("3_6").unwrap();
    assert!(t6.root.is_some(), "legacy root beside nodes");
    assert_eq!(t6.class(1).unwrap().name, "Marauder");
    assert!(matches!(
        t6.nodes.values().find(|n| n.id == Some(6.0)).unwrap().in_,
        Some(LuaTable::Array(_) | LuaTable::Object(_))
    ));
    let t29 = tree::load_tree("3_29").unwrap();
    assert!(t29.groups.index(1).is_some(), "object groups keyed by Lua key");
    assert_eq!(tree::tree_version_num("3_6").unwrap(), 3.06);
    assert!(tree::tree_version_num("9_99").is_err());
    let e = tree::load_tree("9_99").unwrap_err();
    assert!(e.0.contains("9_99"), "{e}");
    // an on-demand version loads from data/tree/_ondemand/
    assert!(tree::load_tree("3_17").unwrap().groups.is_array());
}

#[test]
fn load_all_r3_files() {
    assert_eq!(load_all_r3(), 14);
}
