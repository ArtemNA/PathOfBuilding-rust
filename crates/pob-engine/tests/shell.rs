//! U2c gate for the shell mutators of `pob_engine::shell` (path equivalence, as `tests/engine.rs`): on the 26 R8.1 scenarios, for every
//! mutator that applies to the build,
//!  (1) mutation + `refresh` == a fresh load of the engine's own `SaveDB` (state before the mutation) with the one attribute edited --
//!      every scalar of `mainOutput` bit for bit;
//!  (2) the inverse mutation + `refresh` gives the base output bit for bit;
//! and the readers: `skill_select` / `point_display` / `stat_list` after a fresh load == after `refresh` of the same state.

use std::path::{Path, PathBuf};

use pob_calc::pob_build::{self, XmlElement, XmlNode};
use pob_engine::engine::Engine;
use pob_engine::shell::{MinionEntry, MinionSel};
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

fn root(xml: &str) -> XmlElement {
    match pob_build::parse_xml_document(xml).expect("xml").into_iter().next() {
        Some(XmlNode::Element(e)) => e,
        _ => panic!("no root"),
    }
}

fn child_mut<'a>(e: &'a mut XmlElement, name: &str, nth: usize) -> Option<&'a mut XmlElement> {
    e.children
        .iter_mut()
        .filter_map(|c| match c {
            XmlNode::Element(x) if x.name == name => Some(x),
            _ => None,
        })
        .nth(nth)
}

fn edit_build(xml: &str, f: impl FnOnce(&mut XmlElement)) -> String {
    let mut r = root(xml);
    f(child_mut(&mut r, "Build", 0).expect("<Build>"));
    pob_build::compose_xml(&r)
}

/// `<Skill>` `group` (0-based) of the active skill set.
fn edit_group(xml: &str, group: usize, f: impl FnOnce(&mut XmlElement)) -> String {
    let mut r = root(xml);
    let skills = child_mut(&mut r, "Skills", 0).expect("<Skills>");
    let active = skills.attr("activeSkillSet").unwrap_or("1").to_owned();
    let has_sets = skills.child_elements().any(|c| c.name == "SkillSet");
    let holder = if has_sets {
        skills
            .children
            .iter_mut()
            .find_map(|c| match c {
                XmlNode::Element(x) if x.name == "SkillSet" && x.attr("id") == Some(active.as_str()) => Some(x),
                _ => None,
            })
            .expect("<SkillSet>")
    } else {
        skills
    };
    f(child_mut(holder, "Skill", group).expect("<Skill>"));
    pob_build::compose_xml(&r)
}

fn edit_gem(xml: &str, group: usize, gem: usize, f: impl FnOnce(&mut XmlElement)) -> String {
    edit_group(xml, group, |g| f(child_mut(g, "Gem", gem).expect("<Gem>")))
}

/// `(group, gem)` (0-based) of `displaySkillList[mainActiveSkill].activeEffect.srcInstance` of the main group, and that skill.
fn main_src(e: &Engine) -> Option<(usize, usize)> {
    let st = e.result().env.skills.as_ref()?;
    let g = st.tab.socket_group_list().get((e.main_socket_group() as usize).checked_sub(1)?)?;
    let si = *g
        .display_skill_list
        .as_ref()?
        .get((g.main_active_skill.unwrap_or(1.0) as usize).checked_sub(1)?)?;
    st.skill(si).active_effect.borrow().src_instance
}

struct Gate {
    checks: usize,
    bad: Vec<String>,
    applied: std::collections::BTreeMap<&'static str, (usize, usize)>,
}

impl Gate {
    fn eq(&mut self, what: &str, a: &[(String, String)], b: &[(String, String)]) {
        self.checks += 1;
        if let Some(d) = first_diff(a, b) {
            self.bad.push(format!("{what}: {d}"));
        }
    }

    /// `mutate` + refresh vs the fresh load of `reference`, then `revert` + refresh vs `base`.
    #[allow(clippy::too_many_arguments)]
    fn path(
        &mut self,
        kind: &'static str,
        what: &str,
        e: &mut Engine,
        base: &[(String, String)],
        reference: &str,
        mutate: impl FnOnce(&mut Engine) -> Result<(), String>,
        revert: impl FnOnce(&mut Engine) -> Result<(), String>,
    ) {
        if let Err(err) = mutate(e).and_then(|()| e.refresh()) {
            self.bad.push(format!("{what}: mutate: {err}"));
            return;
        }
        let api = stats(e);
        match Engine::load_xml(reference) {
            Ok(fresh) => {
                self.eq(&format!("{what} (API == fresh XML)"), &api, &stats(&fresh));
                // the readers of the shell see the same state
                self.checks += 1;
                if e.skill_select() != fresh.skill_select() {
                    self.bad.push(format!("{what}: skill_select differs from the fresh load"));
                }
            }
            Err(err) => self.bad.push(format!("{what}: reference load: {err}")),
        }
        match revert(e).and_then(|()| e.refresh()) {
            Ok(()) => self.eq(&format!("{what} (back to base)"), &stats(e), base),
            Err(err) => self.bad.push(format!("{what}: revert: {err}")),
        }
        let x = self.applied.entry(kind).or_default();
        x.0 += 1;
        x.1 += usize::from(api != base);
    }
}

#[test]
fn path_equivalence_of_the_shell_mutators() {
    let mut g = Gate {
        checks: 0,
        bad: Vec::new(),
        applied: Default::default(),
    };
    let list = scenarios();
    for (slug, xml) in &list {
        let mut e = Engine::load_xml(xml).unwrap_or_else(|err| panic!("{slug}: {err}"));
        // the load path = a refresh of the same state (RefreshStatList after BuildOutput); the auto-mode level moves only the next run
        let (sl0, pd0, ss0) = (format!("{:?}", e.stat_list()), e.point_display().clone(), e.skill_select());
        let base = stats(&e);
        if !e.model().character_level_auto_mode && !e.model_has_runtime_groups() {
            let mut r = Engine::load_xml(xml).expect("load");
            r.refresh().expect("refresh");
            g.checks += 1;
            if format!("{:?}", r.stat_list()) != sl0 || r.point_display() != &pd0 || r.skill_select() != ss0 {
                g.bad.push(format!(
                    "{slug}: stat list / point display / skill select after refresh != after load"
                ));
            }
            g.eq(&format!("{slug}: refresh == load"), &stats(&r), &base);
        }
        // the state the UI holds after its first edit: in auto mode the load moved the level (EstimatePlayerProgress), the next run uses
        // it; item-granted socket groups are part of the state (Engine::refresh)
        e.refresh().expect("refresh");
        let base = stats(&e);

        // characterLevel (manual) and back
        let (lvl, auto) = (e.model().level, e.model().character_level_auto_mode);
        let new_lvl = if lvl == 50.0 { 51.0 } else { 50.0 };
        let reference = edit_build(&e.save_xml().expect("save"), |b| {
            b.attrib.insert("level".into(), pob_lua::tostring_num(new_lvl));
            b.attrib.insert("characterLevelAutoMode".into(), "false".into());
        });
        g.path(
            "characterLevel",
            &format!("{slug}: characterLevel {new_lvl}"),
            &mut e,
            &base,
            &reference,
            |e| {
                e.set_character_level(Some(new_lvl));
                Ok(())
            },
            |e| {
                e.set_character_level(Some(lvl));
                e.set_character_level_auto_mode(auto);
                Ok(())
            },
        );

        // mainSocketGroup (UI selFunc, no recalc of its own) and back
        let n = e.socket_group_count();
        let mg = e.main_socket_group();
        if n > 1 {
            let other = if mg == 1.0 { 2.0 } else { 1.0 };
            let reference = edit_build(&e.save_xml().expect("save"), |b| {
                b.attrib.insert("mainSocketGroup".into(), pob_lua::tostring_num(other));
            });
            g.path(
                "mainSocketGroup",
                &format!("{slug}: mainSocketGroup {other}"),
                &mut e,
                &base,
                &reference,
                |e| {
                    e.select_main_socket_group(other);
                    Ok(())
                },
                |e| {
                    e.select_main_socket_group(mg);
                    Ok(())
                },
            );
        }

        // mainActiveSkill of the main group
        let sel = e.skill_select().expect("skill_select");
        let gi = mg as usize - 1;
        if let Some(m) = sel.main.as_ref().filter(|m| m.skills.len() > 1) {
            let cur = m.skill_sel;
            let other = if cur == 1.0 { 2.0 } else { 1.0 };
            let reference = edit_group(&e.save_xml().expect("save"), gi, |s| {
                s.attrib.insert("mainActiveSkill".into(), pob_lua::tostring_num(other));
            });
            g.path(
                "mainActiveSkill",
                &format!("{slug}: mainActiveSkill {other}"),
                &mut e,
                &base,
                &reference,
                |e| e.set_main_active_skill(other),
                |e| e.set_main_active_skill(cur),
            );
        }

        // the srcInstance fields of the main skill
        let sel = e.skill_select().expect("skill_select");
        let Some(m) = sel.main.clone() else { continue };
        let Some((sg, sk)) = main_src(&e) else { continue };
        let gem_attr = |e: &Engine, a: &str| -> Option<String> {
            let x = e.save_xml().expect("save");
            let mut r = root(&x);
            let skills = child_mut(&mut r, "Skills", 0)?;
            let active = skills.attr("activeSkillSet").unwrap_or("1").to_owned();
            let set = skills.children.iter().find_map(|c| match c {
                XmlNode::Element(x) if x.name == "SkillSet" && x.attr("id") == Some(active.as_str()) => Some(x.clone()),
                _ => None,
            })?;
            let grp = set.child_elements().filter(|c| c.name == "Skill").nth(sg)?;
            grp.child_elements().filter(|c| c.name == "Gem").nth(sk)?.attr(a).map(str::to_owned)
        };
        if let Some((parts, cur)) = &m.part {
            for other in (1..=parts.len()).map(|p| p as f64).filter(|p| p != cur).take(2) {
                let prev = gem_attr(&e, "skillPart");
                let reference = edit_gem(&e.save_xml().expect("save"), sg, sk, |gm| {
                    gm.attrib.insert("skillPart".into(), pob_lua::tostring_num(other));
                });
                let back = prev.as_deref().and_then(|s| s.parse::<f64>().ok()).unwrap_or(*cur);
                g.path(
                    "skillPart",
                    &format!("{slug}: skillPart {other}"),
                    &mut e,
                    &base,
                    &reference,
                    |e| e.set_skill_part(other),
                    |e| e.set_skill_part(back),
                );
            }
        }
        if let Some(buf) = &m.stage_count {
            let prev = gem_attr(&e, "skillStageCount").and_then(|s| s.parse::<f64>().ok());
            let other = buf.parse::<f64>().map_or(2.0, |v| if v > 1.0 { v - 1.0 } else { v + 1.0 });
            let reference = edit_gem(&e.save_xml().expect("save"), sg, sk, |gm| {
                gm.attrib.insert("skillStageCount".into(), pob_lua::tostring_num(other));
            });
            g.path(
                "skillStageCount",
                &format!("{slug}: skillStageCount {other}"),
                &mut e,
                &base,
                &reference,
                |e| e.set_skill_stage_count(Some(other)),
                |e| e.set_skill_stage_count(prev),
            );
        }
        if m.mine_count.is_some() {
            let prev = gem_attr(&e, "skillMineCount").and_then(|s| s.parse::<f64>().ok());
            let other = if prev == Some(3.0) { 4.0 } else { 3.0 };
            let reference = edit_gem(&e.save_xml().expect("save"), sg, sk, |gm| {
                gm.attrib.insert("skillMineCount".into(), pob_lua::tostring_num(other));
            });
            g.path(
                "skillMineCount",
                &format!("{slug}: skillMineCount {other}"),
                &mut e,
                &base,
                &reference,
                |e| e.set_skill_mine_count(Some(other)),
                |e| e.set_skill_mine_count(prev),
            );
        }
        if let Some(ms) = &m.minion {
            let pick = ms
                .list
                .iter()
                .find(|x| match (x, &ms.sel) {
                    (MinionEntry::Minion { minion_id, .. }, MinionSel::Minion(cur)) => cur.as_deref() != Some(minion_id.as_str()),
                    (MinionEntry::ItemSet { item_set_id, .. }, MinionSel::ItemSet(cur)) => item_set_id != cur,
                    _ => false,
                })
                .cloned();
            if let Some(v) = pick {
                let (attr, val, prev) = match &v {
                    MinionEntry::Minion { minion_id, .. } => ("skillMinion", minion_id.clone(), gem_attr(&e, "skillMinion")),
                    MinionEntry::ItemSet { item_set_id, .. } => (
                        "skillMinionItemSet",
                        pob_lua::tostring_num(*item_set_id),
                        gem_attr(&e, "skillMinionItemSet"),
                    ),
                    MinionEntry::Text(_) => unreachable!(),
                };
                let reference = edit_gem(&e.save_xml().expect("save"), sg, sk, |gm| {
                    gm.attrib.insert(attr.into(), val.clone());
                });
                let back = match (&v, prev) {
                    (MinionEntry::Minion { .. }, Some(p)) => Some(MinionEntry::Minion {
                        label: String::new(),
                        minion_id: p,
                    }),
                    (MinionEntry::ItemSet { .. }, Some(p)) => Some(MinionEntry::ItemSet {
                        label: String::new(),
                        item_set_id: p.parse().expect("id"),
                    }),
                    _ => None,
                };
                if let Some(back) = back {
                    g.path(
                        "skillMinion",
                        &format!("{slug}: {attr} {val}"),
                        &mut e,
                        &base,
                        &reference,
                        |e| e.set_skill_minion(&v),
                        |e| e.set_skill_minion(&back),
                    );
                }
            }
            if let Some((names, cur)) = &ms.skills
                && names.len() > 1
            {
                let other = if *cur == 1.0 { 2.0 } else { 1.0 };
                let prev = gem_attr(&e, "skillMinionSkill").and_then(|s| s.parse::<f64>().ok()).unwrap_or(*cur);
                let reference = edit_gem(&e.save_xml().expect("save"), sg, sk, |gm| {
                    gm.attrib.insert("skillMinionSkill".into(), pob_lua::tostring_num(other));
                });
                g.path(
                    "skillMinionSkill",
                    &format!("{slug}: skillMinionSkill {other}"),
                    &mut e,
                    &base,
                    &reference,
                    |e| e.set_skill_minion_skill(other),
                    |e| e.set_skill_minion_skill(prev),
                );
            }
        }
    }
    println!(
        "shell path equivalence: {} scenarios, {} checks, {} bad; applied (scenarios, output changed): {:?}",
        list.len(),
        g.checks,
        g.bad.len(),
        g.applied
    );
    assert!(g.bad.is_empty(), "{} failures:\n{}", g.bad.len(), g.bad.join("\n"));
}
