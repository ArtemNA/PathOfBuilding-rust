//! U5 gate for the mutators of `pob_engine::skills_tab` (path equivalence, as `tests/shell.rs`): on the R8.1 scenarios, for every mutator that applies,
//!  (1) mutation + `refresh` == a fresh load of the engine's own `SaveDB` after it -- every scalar of `mainOutput` bit for bit, and the same `SaveDB`;
//!  (2) the inverse mutation + `refresh` gives the output of the state before it, bit for bit.

use std::path::{Path, PathBuf};

use pob_engine::engine::Engine;
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

/// The engine over the saved state of the XML: what `SaveDB` keeps of the run's groups is in the model from the start.
fn base_engine(xml: &str) -> Engine {
    let e = Engine::load_xml(xml).expect("load");
    Engine::load_xml(&e.save_xml().expect("save")).expect("reload")
}

type Edit = Box<dyn Fn(&mut Engine) -> Result<(), String>>;

/// `name`, what it needs (`group with a gem`), the mutation, the inverse (when there is one).
fn mutators(e: &Engine) -> Vec<(&'static str, Edit, Option<Edit>)> {
    let groups = e.socket_groups();
    let Some(g) = groups
        .iter()
        .position(|g| g.source.is_none() && !g.gem_list.is_empty())
        .map(|i| i + 1)
    else {
        return Vec::new();
    };
    let group = &groups[g - 1];
    let n = groups.len();
    let gems = group.gem_list.len();
    let level = group.gem_list[0].level;
    let quality = group.gem_list[0].quality;
    let enabled = group.gem_list[0].enabled;
    let count = group.gem_list[0].count;
    let label = group.label.clone().unwrap_or_default();
    let slot = group.slot.clone();
    let main = e.model().main_socket_group;
    let group_enabled = group.enabled;
    let main_active = groups
        .get((e.main_socket_group() as usize).wrapping_sub(1))
        .and_then(|x| x.main_active_skill)
        .unwrap_or(1.0);
    let full_dps = group.include_in_full_dps == Some(true);
    let first_gem = group.gem_list[0].gem_id.clone();
    let mut v: Vec<(&'static str, Edit, Option<Edit>)> = vec![
        (
            "new group + delete",
            Box::new(|e| e.new_socket_group().map(|_| ())),
            Some(Box::new(move |e| {
                let last = e
                    .socket_groups()
                    .iter()
                    .rposition(|g| g.gem_list.is_empty() && g.source.is_none())
                    .expect("the new group")
                    + 1;
                e.delete_socket_group(last)
            })),
        ),
        (
            "label",
            Box::new(move |e| e.set_group_label(g, "U5 label")),
            Some(Box::new(move |e| e.set_group_label(g, &label))),
        ),
        (
            "slot",
            Box::new(move |e| e.set_group_slot(g, Some("Gloves")).map(|_| ())),
            Some(Box::new(move |e| e.set_group_slot(g, slot.as_deref()).map(|_| ()))),
        ),
        (
            "group enabled",
            Box::new(move |e| e.set_group_enabled(g, !group_enabled)),
            Some(Box::new(move |e| e.set_group_enabled(g, group_enabled))),
        ),
        (
            "full dps",
            Box::new(move |e| e.set_group_full_dps(g, !full_dps)),
            Some(Box::new(move |e| e.set_group_full_dps(g, full_dps))),
        ),
        ("group count", Box::new(move |e| e.set_group_count(g, Some(3.0))), None),
        (
            "gem level",
            Box::new(move |e| e.set_gem_level(g, 1, Some(5.0))),
            Some(Box::new(move |e| e.set_gem_level(g, 1, level))),
        ),
        (
            "gem quality",
            Box::new(move |e| e.set_gem_quality(g, 1, Some(7.0))),
            Some(Box::new(move |e| e.set_gem_quality(g, 1, quality))),
        ),
        (
            "gem enabled",
            Box::new(move |e| e.set_gem_enabled(g, 1, !enabled).map(|_| ())),
            Some(Box::new(move |e| {
                e.set_gem_enabled(g, 1, enabled)?;
                // a run without the gem clamped `mainActiveSkill` of the main group (CalcSetup.lua:1882-1890), as upstream
                e.set_main_active_skill(main_active)
            })),
        ),
        (
            "gem count",
            Box::new(move |e| e.set_gem_count(g, 1, Some(2.0)).map(|_| ())),
            Some(Box::new(move |e| e.set_gem_count(g, 1, count).map(|_| ()))),
        ),
        (
            "add gem + delete",
            Box::new(move |e| {
                e.add_empty_gem(g)?;
                e.set_gem_id(g, gems + 1, Some("Metadata/Items/Gems/SkillGemFireball")).map(|_| ())
            }),
            Some(Box::new(move |e| e.delete_gem(g, gems + 1))),
        ),
        ("delete gem", Box::new(move |e| e.delete_gem(g, 1)), None),
        (
            "default level + quality",
            Box::new(|e| {
                e.set_default_gem_level("levelOne")?;
                e.set_default_gem_quality(Some(5.0))
            }),
            None,
        ),
        (
            "gem options",
            Box::new(|e| {
                e.set_sort_gems_by_dps(false);
                e.set_sort_gems_by_dps_field("TotalDPS");
                e.set_show_support_gem_types("NORMAL");
                e.set_show_legacy_gems(true);
                Ok(())
            }),
            None,
        ),
        (
            "skill set new + active + back + delete",
            Box::new(|e| {
                let id = e.new_skill_set("U5 set")?;
                e.set_active_skill_set(id).map(|_| ())
            }),
            Some(Box::new(move |e| {
                let first = e.skill_sets()[0].0;
                let last = e.skill_sets().last().expect("set").0;
                e.set_active_skill_set(first)?;
                // the empty set's run clamped `build.mainSocketGroup` (CalcSetup.lua:1590), as upstream
                e.set_main_group(main as usize);
                e.delete_skill_set(last)
            })),
        ),
        (
            "skill set copy + rename",
            Box::new(|e| {
                let first = e.skill_sets()[0].0;
                let id = e.copy_skill_set(first, "U5 copy")?;
                e.rename_skill_set(id, "U5 renamed")?;
                e.set_active_skill_set(id).map(|_| ())
            }),
            None,
        ),
        (
            "undo state",
            Box::new(move |e| {
                let st = e.skills_undo_state();
                e.set_gem_quality(g, 1, Some(9.0))?;
                e.refresh()?;
                e.restore_skills_undo_state(&st)
            }),
            None,
        ),
        (
            "spectres",
            Box::new(|e| {
                e.set_spectre_list(vec!["Metadata/Monsters/Axis/AxisCaster".to_owned()]);
                Ok(())
            }),
            None,
        ),
        ("delete all", Box::new(|e| e.delete_all_socket_groups()), None),
        (
            "paste",
            Box::new(|e| {
                e.paste_socket_group("Label: pasted\r\nFireball 20/0  1\r\nArc 3/5 DISABLED 2\r\n")
                    .map(|_| ())
            }),
            None,
        ),
    ];
    if n >= 2 {
        v.push((
            "move group",
            Box::new(move |e| {
                let to = if g == n { 1 } else { n };
                e.move_socket_group(g, to)
            }),
            Some(Box::new(move |e| {
                let to = if g == n { 1 } else { n };
                e.move_socket_group(to, g)
            })),
        ));
        let other = if g == 1 { 2 } else { 1 };
        v.push((
            "main group",
            Box::new(move |e| {
                e.set_main_group(other);
                Ok(())
            }),
            Some(Box::new(move |e| {
                e.set_main_group(main as usize);
                Ok(())
            })),
        ));
        v.push(("delete group", Box::new(move |e| e.delete_socket_group(g)), None));
    }
    if first_gem.is_some() {
        v.push((
            "gem id",
            Box::new(move |e| e.set_gem_id(g, 1, Some("Metadata/Items/Gems/SkillGemArc")).map(|_| ())),
            None,
        ));
    }
    v
}

#[test]
fn skills_mutators_path_equivalence() {
    let (mut checks, mut failures) = (0, Vec::new());
    for (slug, xml) in scenarios() {
        let base = base_engine(&xml);
        let base_stats = stats(&base);
        let base_xml = base.save_xml().expect("save");
        let names = mutators(&base).len();
        for k in 0..names {
            let mut e = Engine::load_xml(&base_xml).expect("load");
            let list = mutators(&e);
            let (name, edit, inverse) = &list[k];
            edit(&mut e).unwrap_or_else(|err| panic!("{slug}: {name}: {err}"));
            e.refresh().unwrap_or_else(|err| panic!("{slug}: {name}: refresh: {err}"));
            let saved = e.save_xml().expect("save");
            let fresh = Engine::load_xml(&saved).unwrap_or_else(|err| panic!("{slug}: {name}: reload: {err}"));
            checks += 1;
            if let Some(d) = first_diff(&stats(&e), &stats(&fresh)) {
                failures.push(format!("{slug}: {name}: path: {d}"));
            }
            let again = fresh.save_xml().expect("save");
            checks += 1;
            // a new group's `includeInFullDPS` is nil: `tostring(nil)` is saved, `"nil" == "true"` reads back as false (SkillsTab.lua:448, :591)
            // and so do the group CalcSetup makes (`mainActiveSkill` nil, :596-597, `tonumber("nil") or 1`, :453-454)
            // A group CalcSetup made is not compared: its gem has neither `gemId` nor `count` until a load gives them (SkillsTab.lua:487-501), `mainOutput` above is.
            let nil_to_false = |x: &str| {
                let mut out = String::new();
                let mut rest = x;
                while let Some(a) = rest.find("<Skill ") {
                    let b = rest[a..]
                        .find("</Skill>")
                        .map_or_else(|| rest[a..].find("/>").map_or(rest.len(), |e| a + e + 2), |e| a + e + 8);
                    out.push_str(&rest[..a]);
                    let block = &rest[a..b.min(rest.len())];
                    if !block.split('>').next().unwrap_or("").contains(" source=") {
                        out.push_str(block);
                    }
                    rest = &rest[b.min(rest.len())..];
                }
                out.push_str(rest);
                out.replace("includeInFullDPS=\"nil\"", "includeInFullDPS=\"false\"")
                    .replace("mainActiveSkill=\"nil\"", "mainActiveSkill=\"1\"")
                    .replace("mainActiveSkillCalcs=\"nil\"", "mainActiveSkillCalcs=\"1\"")
            };
            if nil_to_false(&saved) != nil_to_false(&again) {
                let (a, b) = (nil_to_false(&saved), nil_to_false(&again));
                let at = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(0);
                let from = at.saturating_sub(60);
                failures.push(format!(
                    "{slug}: {name}: SaveDB of the fresh load differs at {at}: {:?} vs {:?}",
                    a.get(from..at + 60),
                    b.get(from..at + 60)
                ));
            }
            if let Some(inv) = inverse {
                inv(&mut e).unwrap_or_else(|err| panic!("{slug}: {name}: inverse: {err}"));
                e.refresh().unwrap_or_else(|err| panic!("{slug}: {name}: refresh: {err}"));
                checks += 1;
                if let Some(d) = first_diff(&stats(&e), &base_stats) {
                    failures.push(format!("{slug}: {name}: round trip: {d}"));
                }
            }
        }
    }
    eprintln!("checks {checks}, failures {}", failures.len());
    assert!(
        failures.is_empty(),
        "{} failures, the first 25:\n{}",
        failures.len(),
        failures[..failures.len().min(25)].join("\n")
    );
}

/// Between the mutation and the next run the tab shows the edit and the old `displayLabel` (upstream's tables), `SaveDB` writes the edit.
#[test]
fn skills_live_state_before_the_run() {
    let (_, xml) = scenarios().into_iter().next().expect("scenario");
    let mut e = base_engine(&xml);
    let i = e.new_socket_group().expect("new");
    assert!(e.socket_groups()[i - 1].display_label.is_none());
    e.set_group_label(i, "kept").expect("label");
    assert!(e.save_xml().expect("save").contains("kept"));
    e.refresh().expect("refresh");
    assert_eq!(e.socket_groups()[i - 1].label.as_deref(), Some("kept"));
    assert!(e.socket_groups()[i - 1].display_label.is_some());
}
