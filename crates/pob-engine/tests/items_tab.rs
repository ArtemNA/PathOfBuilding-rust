//! U6 gate for the mutators of `pob_engine::items_tab` (path equivalence, as `tests/skills_tab.rs`): on the R8.1 scenarios, for every mutator that applies,
//!  (1) mutation + `refresh` == a fresh load of the engine's own `SaveDB` after it -- every scalar of `mainOutput` bit for bit, and the same `SaveDB`;
//!  (2) the inverse mutation + `refresh` gives the output of the state before it, bit for bit.

use std::path::{Path, PathBuf};

use pob_engine::engine::Engine;
use pob_items::{Item, ItemOptions};
use serde_json::Value as Json;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/r8")
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn scenarios() -> Vec<(String, String)> {
    let mut v: Vec<String> = std::fs::read_dir(dir())
        .expect("fixtures dir")
        .filter_map(|e| {
            e.ok()?
                .file_name()
                .into_string()
                .ok()?
                .strip_suffix(".save.json")
                .map(str::to_owned)
        })
        .collect();
    v.sort();
    v.into_iter()
        .map(|slug| {
            let fx: Json = serde_json::from_str(&read(&dir().join(format!("{slug}.save.json")))).expect("json");
            let name = fx["build"].as_str().expect("build");
            let path = match fx["kind"].as_str() {
                Some("golden") => pob_golden::registry::find_golden(name).expect("golden").xml_path(),
                _ => Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../pob-calc/tests/fixtures/offence-synth/{name}.xml")),
            };
            (slug, read(&path))
        })
        .collect()
}

fn stats(e: &Engine) -> Vec<(String, String)> {
    e.result().stats.iter().map(|(k, v)| (k.clone(), format!("{v:?}"))).collect()
}

fn first_diff(a: &[(String, String)], b: &[(String, String)]) -> Option<String> {
    if a == b {
        return None;
    }
    if a.len() != b.len() {
        return Some(format!("{} vs {} keys", a.len(), b.len()));
    }
    a.iter().zip(b).find(|(x, y)| x != y).map(|(x, y)| format!("{x:?} vs {y:?}"))
}

fn base_engine(xml: &str) -> Engine {
    let e = Engine::load_xml(xml).expect("load");
    Engine::load_xml(&e.save_xml().expect("save")).expect("reload")
}

fn parsed(text: &str) -> Item {
    let mut it = Item::parse(text, &ItemOptions::default());
    assert!(it.base.is_some(), "{text}");
    it.normalise_quality();
    it
}

const RING: &str = "Rarity: RARE\nU6 Loop\nCoral Ring\n+25 to maximum Life\n+10% to Fire Resistance\n";
const AMULET: &str = "Rarity: RARE\nU6 Beads\nJade Amulet\n+12 to Strength\n";
const JEWEL: &str = "Rarity: RARE\nU6 Eye\nCrimson Jewel\n+7 to maximum Life\n";

type Edit = Box<dyn Fn(&mut Engine) -> Result<(), String>>;

fn last_item(e: &Engine) -> f64 {
    *e.item_order().last().expect("an item")
}

fn mutators(e: &Engine) -> Vec<(&'static str, Edit, Option<Edit>)> {
    let weapon2 = e.active_item_set().use_second_weapon_set;
    let first = e.item_order().first().copied();
    let set = e.active_item_set_id();
    let main = e.model().main_socket_group;
    // An empty set removes the item-granted groups (CalcSetup.lua:1470-1520): a flag on such a group does not come back.
    let flagged_source = e
        .socket_groups()
        .iter()
        .any(|g| g.source.is_some() && g.include_in_full_dps == Some(true));
    let equipped_ring = e.slot_sel_item("Ring 1");
    let mut v: Vec<(&'static str, Edit, Option<Edit>)> = vec![
        (
            "add + delete",
            Box::new(|e| e.add_item(&parsed(RING), None).map(|_| ())),
            Some(Box::new(|e| {
                let id = last_item(e);
                e.delete_item(id)
            })),
        ),
        (
            "add + equip + back",
            Box::new(|e| {
                let id = e.add_item(&parsed(AMULET), Some(1))?;
                e.set_slot_sel_item("Amulet", id);
                Ok(())
            }),
            None,
        ),
        (
            "replace an item",
            Box::new(|e| {
                let id = e.add_item(&parsed(RING), None)?;
                let mut it = parsed(AMULET);
                it.id = Some(id);
                e.add_item(&it, None).map(|_| ())
            }),
            None,
        ),
        (
            "ring slot",
            Box::new(|e| {
                let id = e.add_item(&parsed(RING), None)?;
                e.set_slot_sel_item("Ring 1", id);
                Ok(())
            }),
            Some(Box::new(move |e| {
                e.set_slot_sel_item("Ring 1", equipped_ring);
                let id = last_item(e);
                e.delete_item(id)
            })),
        ),
        (
            "weapon set",
            Box::new(move |e| {
                e.set_use_second_weapon_set(!weapon2);
                Ok(())
            }),
            None,
        ),
        (
            "new set + active + back + delete",
            Box::new(|e| {
                let id = e.new_item_set(Some("U6 set"), None);
                e.set_active_item_set(id);
                Ok(())
            }),
            (!flagged_source).then(|| -> Edit {
                Box::new(move |e| {
                    let last = e.item_sets().last().expect("set").id;
                    e.set_active_item_set(set);
                    e.delete_item_set(last);
                    // the empty set's run clamped `build.mainSocketGroup` (CalcSetup.lua:1590), as upstream
                    e.set_main_group(main as usize);
                    Ok(())
                })
            }),
        ),
        (
            "copy set + rename",
            Box::new(move |e| {
                let id = e.copy_item_set(set, "U6 copy");
                e.rename_item_set(id, "U6 renamed");
                e.set_active_item_set(id);
                Ok(())
            }),
            None,
        ),
        (
            "undo state",
            Box::new(|e| {
                let st = e.items_undo_state();
                let id = e.add_item(&parsed(RING), None)?;
                // taking off an item with a granted skill makes a new group on the way back: `includeInFullDPS="nil"` of a new table, not this mutator's
                if e.item_by_id(e.slot_sel_item("Ring 2")).is_none_or(|i| i.granted_skills.is_empty()) {
                    e.set_slot_sel_item("Ring 2", id);
                }
                e.refresh()?;
                e.restore_items_undo_state(&st)
            }),
            None,
        ),
        ("delete all", Box::new(|e| e.delete_all_items()), None),
    ];
    if let Some(id) = first {
        v.push(("delete an item", Box::new(move |e| e.delete_item(id)), None));
        let order = e.item_order();
        if order.len() >= 2 {
            let mut moved = order.clone();
            moved.swap(0, 1);
            let back = order;
            v.push((
                "move item",
                Box::new(move |e| {
                    e.set_item_order(moved.clone());
                    Ok(())
                }),
                Some(Box::new(move |e| {
                    e.set_item_order(back.clone());
                    Ok(())
                })),
            ));
        }
    }
    if let Some(slot) = e.result().env.ordered_slots.iter().find(|s| {
        s.node_id.is_some_and(|n| e.result().env.spec.alloc_nodes.contains_key(&n))
            && e.item_by_id(s.sel_item_id).is_none_or(|i| i.cluster_jewel.is_none())
            && s.node_id.is_some_and(|n| n < 65536)
    }) {
        let node = slot.node_id.expect("socket");
        let old = slot.sel_item_id;
        v.push((
            "socket jewel",
            Box::new(move |e| {
                let id = e.add_item(&parsed(JEWEL), None)?;
                e.set_socket_item(node, id)
            }),
            Some(Box::new(move |e| {
                e.set_socket_item(node, old)?;
                let id = last_item(e);
                e.delete_item(id)
            })),
        ));
    }
    if e.slot_sel_item("Flask 1") != 0.0 {
        let on = e
            .result()
            .env
            .ordered_slots
            .iter()
            .find(|s| s.slot_name == "Flask 1")
            .and_then(|s| s.active)
            .unwrap_or(false);
        v.push((
            "flask active",
            Box::new(move |e| {
                e.set_slot_active("Flask 1", !on);
                Ok(())
            }),
            Some(Box::new(move |e| {
                e.set_slot_active("Flask 1", on);
                Ok(())
            })),
        ));
    }
    v
}

#[test]
fn items_mutators_path_equivalence() {
    let mut checks = 0;
    let mut failures = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for (slug, xml) in scenarios() {
        let base = base_engine(&xml);
        let before = stats(&base);
        let count = mutators(&base).len();
        for i in 0..count {
            let mut e = base_engine(&xml);
            let (name, edit, inverse) = mutators(&e).into_iter().nth(i).expect("mutator");
            names.insert(name);
            edit(&mut e).unwrap_or_else(|er| panic!("{slug}: {name}: {er}"));
            e.refresh().unwrap_or_else(|er| panic!("{slug}: {name}: refresh: {er}"));
            let saved = e.save_xml().expect("save");
            let fresh = Engine::load_xml(&saved).unwrap_or_else(|er| panic!("{slug}: {name}: reload: {er}"));
            checks += 1;
            if let Some(d) = first_diff(&stats(&e), &stats(&fresh)) {
                failures.push(format!("{slug}: {name}: mutated vs fresh: {d}"));
            }
            let again = fresh.save_xml().expect("save");
            // a new set saves `tostring(nil)`, a load reads it as false (ItemsTab.lua:1335, :1438)
            let saved = saved.replace("useSecondWeaponSet=\"nil\"", "useSecondWeaponSet=\"false\"");
            if saved != again {
                let at = saved.bytes().zip(again.bytes()).position(|(a, b)| a != b).unwrap_or(0);
                let (a, b) = (at.saturating_sub(100), at + 100);
                failures.push(format!(
                    "{slug}: {name}: the SaveDB of the fresh load differs: {:?} vs {:?}",
                    saved.get(a..b.min(saved.len())),
                    again.get(a..b.min(again.len()))
                ));
            }
            if let Some(inv) = inverse {
                inv(&mut e).unwrap_or_else(|er| panic!("{slug}: {name}: inverse: {er}"));
                e.refresh().unwrap_or_else(|er| panic!("{slug}: {name}: refresh: {er}"));
                checks += 1;
                if let Some(d) = first_diff(&before, &stats(&e)) {
                    failures.push(format!("{slug}: {name}: inverse: {d}"));
                }
            }
        }
    }
    eprintln!("mutators {names:?}, checks {checks}, failed {}", failures.len());
    assert!(failures.is_empty(), "{}", failures[..failures.len().min(25)].join("\n"));
}
