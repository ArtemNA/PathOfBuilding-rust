//! R8.8 gate (path equivalence, pob-ts LESSONS 4): for every mutator of [`Engine`] on the 26 R8.1 scenarios (6 goldens + 20 R7-synth),
//!  (1) "mutation through the API == a fresh load of the XML with the same state": the reference XML is the engine's own `SaveDB` of the
//!      state before the mutation (what upstream would save -- it holds the socket groups CalcSetup added, which upstream's `setGem` path
//!      sees, see `Engine::set_gem`) with the one attribute/element edited (`xml.lua` DOM -> `ComposeXML`), loaded into a new engine --
//!      every scalar of `mainOutput` bit for bit;
//!  (2) "back to the base == the base": the inverse mutation gives the base output bit for bit;
//!  (3) no hidden state: load A then load B in one thread == B alone in a fresh thread.
//! Plus the build code: `decode(encode(SaveDB)) == SaveDB` (base64.lua port + zlib).

use std::path::{Path, PathBuf};
use std::time::Instant;

use pob_calc::pob_build::{self, XmlElement, XmlNode};
use pob_engine::engine::{Engine, GemProp};
use serde_json::Value as Json;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/r8")
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// `(slug, XML text)` of the 26 R8.1 scenarios.
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
        let ka: Vec<&String> = a.iter().map(|x| &x.0).collect();
        let only: Vec<&String> = b.iter().map(|x| &x.0).filter(|k| !ka.contains(k)).take(3).collect();
        return Some(format!("{} vs {} keys (e.g. {only:?})", a.len(), b.len()));
    }
    a.iter().zip(b).find(|(x, y)| x != y).map(|(x, y)| format!("{x:?} vs {y:?}"))
}

// --- the reference XML: the original file, one thing edited ----------------------------------------------------------------------

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

fn compose(r: &XmlElement) -> String {
    pob_build::compose_xml(r)
}

/// The active `<Spec>` (TreeTab: `m_min(activeSpec, #specList)`).
fn edit_spec(xml: &str, f: impl FnOnce(&mut XmlElement)) -> String {
    let mut r = root(xml);
    let tree = child_mut(&mut r, "Tree", 0).expect("<Tree>");
    let n = tree.child_elements().filter(|c| c.name == "Spec").count();
    let active = tree
        .attr("activeSpec")
        .and_then(|a| a.parse::<usize>().ok())
        .unwrap_or(1)
        .min(n)
        .max(1);
    f(child_mut(tree, "Spec", active - 1).expect("<Spec>"));
    compose(&r)
}

/// The active `<ConfigSet>`.
fn edit_config(xml: &str, f: impl FnOnce(&mut XmlElement)) -> String {
    let mut r = root(xml);
    let cfg = child_mut(&mut r, "Config", 0).expect("<Config>");
    let active = cfg.attr("activeConfigSet").unwrap_or("1").to_owned();
    // a legacy `<Config>` holds its `<Input>`s directly (ConfigTab.lua:839-860)
    let has_sets = cfg.child_elements().any(|c| c.name == "ConfigSet");
    let set = if has_sets {
        cfg.children
            .iter_mut()
            .find_map(|c| match c {
                XmlNode::Element(x) if x.name == "ConfigSet" && x.attr("id") == Some(active.as_str()) => Some(x),
                _ => None,
            })
            .expect("<ConfigSet>")
    } else {
        cfg
    };
    f(set);
    compose(&r)
}

/// `<Gem>` `gem` (0-based) of socket group `group` (0-based) of the active skill set.
fn edit_gem(xml: &str, group: usize, gem: usize, f: impl FnOnce(&mut XmlElement)) -> String {
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
    let g = child_mut(holder, "Skill", group).expect("<Skill>");
    f(child_mut(g, "Gem", gem).expect("<Gem>"));
    compose(&r)
}

fn set_input(set: &mut XmlElement, name: &str, kind: &str, value: &str) {
    set.children
        .retain(|c| !matches!(c, XmlNode::Element(x) if x.name == "Input" && x.attr("name") == Some(name)));
    let mut e = XmlElement {
        name: "Input".into(),
        attrib: Default::default(),
        children: Vec::new(),
        empty: true,
    };
    e.attrib.insert("name".into(), name.into());
    e.attrib.insert(kind.into(), value.into());
    set.children.insert(0, XmlNode::Element(e));
}

// --- choosing what to mutate -------------------------------------------------------------------------------------------------------

/// An unallocated node one step from the tree (`#node.path == 1`), no mastery / start / cluster socket, lowest id.
fn alloc_candidates(e: &Engine) -> Vec<u32> {
    let spec = &e.result().env.spec;
    let mut ids: Vec<u32> = spec
        .nodes
        .iter()
        .filter(|n| {
            !n.alloc
                && n.id < 65536
                && n.path.as_ref().is_some_and(|p| p.len() == 1)
                && n.ty() != pob_tree::NodeType::Mastery
                && !n.ty().is_start()
                && n.tree_node().expansion_jewel.is_none()
                && n.ascendancy_name().is_none()
        })
        .map(|n| n.id)
        .collect();
    ids.sort_unstable();
    ids
}

/// An allocated leaf (`#node.depends == 1`), no mastery / start / socket / ascendancy, lowest id.
fn dealloc_candidates(e: &Engine) -> Vec<u32> {
    let spec = &e.result().env.spec;
    let mut ids: Vec<u32> = spec
        .alloc_nodes()
        .filter(|n| {
            n.depends.len() == 1
                && n.id < 65536
                && n.ty() != pob_tree::NodeType::Mastery
                && !n.ty().is_start()
                && !n.tree_node().is_jewel_socket
                && n.tree_node().expansion_jewel.is_none()
                && n.ascendancy_name().is_none()
        })
        .map(|n| n.id)
        .collect();
    ids.sort_unstable();
    ids
}

fn nodes_with(spec: &mut XmlElement, add: Option<u32>, remove: Option<u32>) {
    let mut ids: Vec<String> = spec
        .attr("nodes")
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if let Some(r) = remove {
        ids.retain(|x| *x != r.to_string());
    }
    if let Some(a) = add {
        ids.push(a.to_string());
    }
    spec.attrib.insert("nodes".into(), ids.join(","));
}

struct Gate {
    checks: usize,
    bad: Vec<String>,
    t_recalc: Vec<f64>,
}

impl Gate {
    fn eq(&mut self, what: &str, a: &[(String, String)], b: &[(String, String)]) {
        self.checks += 1;
        if let Some(d) = first_diff(a, b) {
            self.bad.push(format!("{what}: {d}"));
        }
    }

    /// API mutation `mutate` vs the fresh load of `reference`, then `revert` vs `base`.
    fn path(
        &mut self,
        what: &str,
        e: &mut Engine,
        base: &[(String, String)],
        reference: &str,
        mutate: impl FnOnce(&mut Engine) -> Result<(), String>,
        revert: impl FnOnce(&mut Engine) -> Result<(), String>,
    ) -> bool {
        let t = Instant::now();
        if let Err(err) = mutate(e) {
            self.bad.push(format!("{what}: mutate: {err}"));
            return false;
        }
        self.t_recalc.push(t.elapsed().as_secs_f64() * 1e3);
        let api = stats(e);
        match Engine::load_xml(reference) {
            Ok(fresh) => self.eq(&format!("{what} (API == fresh XML)"), &api, &stats(&fresh)),
            Err(err) => self.bad.push(format!("{what}: reference load: {err}")),
        }
        match revert(e) {
            Ok(()) => self.eq(&format!("{what} (back to base)"), &stats(e), base),
            Err(err) => self.bad.push(format!("{what}: revert: {err}")),
        }
        api != base
    }
}

#[test]
fn path_equivalence_of_every_mutator() {
    let mut g = Gate {
        checks: 0,
        bad: Vec::new(),
        t_recalc: Vec::new(),
    };
    let mut per_mutator = std::collections::BTreeMap::<&str, (usize, usize)>::new();
    let list = scenarios();
    for (slug, xml) in &list {
        let mut e = Engine::load_xml(xml).unwrap_or_else(|err| panic!("{slug}: {err}"));
        let base = stats(&e);
        // the DOM round trip of the reference path changes nothing by itself
        g.eq(
            &format!("{slug}: ComposeXML(ParseXML(file))"),
            &base,
            &stats(&Engine::load_xml(&compose(&root(xml))).expect("load")),
        );

        // build code: decode(encode(SaveDB)) == SaveDB
        let saved = e.save_xml().expect("save");
        let code = e.build_code().expect("code");
        g.checks += 1;
        if pob_build::decode_build_code(&code).as_deref() != Some(saved.as_bytes()) {
            g.bad.push(format!("{slug}: build code does not decode to the SaveDB XML"));
        }

        // allocNode / deallocNode: the lowest-id candidates until one changes the output (a no-op one still checks both paths)
        let eff = |_: &mut Gate, k: &'static str, changed: bool, per: &mut std::collections::BTreeMap<&str, (usize, usize)>| {
            let x = per.entry(k).or_default();
            x.0 += 1;
            x.1 += usize::from(changed);
            changed
        };
        for id in alloc_candidates(&e).into_iter().take(12) {
            let reference = edit_spec(&e.save_xml().expect("save"), |s| nodes_with(s, Some(id), None));
            let c = g.path(
                &format!("{slug}: allocNode({id})"),
                &mut e,
                &base,
                &reference,
                |e| e.set_node_alloc(f64::from(id), true),
                |e| e.set_node_alloc(f64::from(id), false),
            );
            if eff(&mut g, "allocNode", c, &mut per_mutator) {
                break;
            }
        }
        for id in dealloc_candidates(&e).into_iter().take(12) {
            let reference = edit_spec(&e.save_xml().expect("save"), |s| nodes_with(s, None, Some(id)));
            let c = g.path(
                &format!("{slug}: deallocNode({id})"),
                &mut e,
                &base,
                &reference,
                |e| e.set_node_alloc(f64::from(id), false),
                |e| e.set_node_alloc(f64::from(id), true),
            );
            if eff(&mut g, "deallocNode", c, &mut per_mutator) {
                break;
            }
        }

        // setConfig: check vars, then count vars, until one of each changes the output
        for (kind_name, vars) in [
            (
                "setConfig check",
                &[
                    "conditionKilledRecently",
                    "buffOnslaught",
                    "conditionFullLife",
                    "usePowerCharges",
                    "conditionEnemyBlinded",
                ][..],
            ),
            (
                "setConfig count",
                &[
                    "enemyLevel",
                    "enemyPhysicalReduction",
                    "enemyArmour",
                    "enemyEvasion",
                    "enemyChaosResist",
                ][..],
            ),
        ] {
            for &var in vars {
                if !pob_engine::api::config_var_known(var) {
                    continue;
                }
                let before = e.result().env.config.input.get(var).cloned();
                let (kind, on, val) = match (kind_name, &before) {
                    ("setConfig check", Some(pob_build::InputValue::Bool(true))) => {
                        ("boolean", "false".to_owned(), pob_build::InputValue::Bool(false))
                    }
                    ("setConfig check", _) => ("boolean", "true".to_owned(), pob_build::InputValue::Bool(true)),
                    (_, Some(pob_build::InputValue::Num(n))) if *n == 70.0 => ("number", "60".to_owned(), pob_build::InputValue::Num(60.0)),
                    _ => ("number", "70".to_owned(), pob_build::InputValue::Num(70.0)),
                };
                let reference = edit_config(&e.save_xml().expect("save"), |s| set_input(s, var, kind, &on));
                let c = g.path(
                    &format!("{slug}: setConfig({var}={on})"),
                    &mut e,
                    &base,
                    &reference,
                    |e| e.set_config(var, Some(val)),
                    |e| match before {
                        Some(b) => e.set_config(var, Some(b)),
                        None => e.clear_config(var),
                    },
                );
                if eff(&mut g, kind_name, c, &mut per_mutator) {
                    break;
                }
            }
        }

        // setGem on the main socket group: level, quality, enabled (a support gem if any)
        let main = e.result().env.main_socket_group as usize;
        let groups_in_xml = pob_build::active_skill_groups(e.model()).len();
        if main >= 1 && main <= groups_in_xml {
            let gems = &e.gem_states(main);
            if let Some((k, level, quality)) = gems.iter().enumerate().find_map(|(k, g)| Some((k, g.0?, g.1?))) {
                let nl = if level > 1.0 { level - 1.0 } else { level + 1.0 };
                let reference = edit_gem(&e.save_xml().expect("save"), main - 1, k, |x| {
                    x.attrib.insert("level".into(), pob_lua::tostring_num(nl));
                });
                let c = g.path(
                    &format!("{slug}: setGem({main},{},level={nl})", k + 1),
                    &mut e,
                    &base,
                    &reference,
                    |e| e.set_gem(main, k + 1, GemProp::Level(nl)),
                    |e| e.set_gem(main, k + 1, GemProp::Level(level)),
                );
                eff(&mut g, "setGem level", c, &mut per_mutator);
                let nq = if quality > 0.0 { 0.0 } else { 20.0 };
                let reference = edit_gem(&e.save_xml().expect("save"), main - 1, k, |x| {
                    x.attrib.insert("quality".into(), pob_lua::tostring_num(nq));
                });
                let c = g.path(
                    &format!("{slug}: setGem({main},{},quality={nq})", k + 1),
                    &mut e,
                    &base,
                    &reference,
                    |e| e.set_gem(main, k + 1, GemProp::Quality(nq)),
                    |e| e.set_gem(main, k + 1, GemProp::Quality(quality)),
                );
                eff(&mut g, "setGem quality", c, &mut per_mutator);
            }
            if let Some(k) = gems.iter().position(|g| g.2 && g.3) {
                let reference = edit_gem(&e.save_xml().expect("save"), main - 1, k, |x| {
                    x.attrib.insert("enabled".into(), "false".into());
                    x.attrib.insert("enableGlobal1".into(), "true".into());
                    x.attrib.insert("enableGlobal2".into(), "true".into());
                });
                let c = g.path(
                    &format!("{slug}: setGem({main},{},enabled=false)", k + 1),
                    &mut e,
                    &base,
                    &reference,
                    |e| e.set_gem(main, k + 1, GemProp::Enabled(false)),
                    |e| e.set_gem(main, k + 1, GemProp::Enabled(true)),
                );
                eff(&mut g, "setGem enabled", c, &mut per_mutator);
            }
        }

        // addItemText: a rare ring into "Ring 1" (reference: the saved XML with the `<Item>` text and the slot), inverse: the previous ring
        {
            const RING: &str = "Rarity: RARE
Engine Test Loop
Iron Ring
Implicits: 1
Adds 1 to 4 Physical Damage to Attacks
+30 to maximum Life
+20% to Fire Resistance";
            let prev = e
                .result()
                .env
                .ordered_slots
                .iter()
                .find(|s| s.slot_name == "Ring 1")
                .map_or(0.0, |s| s.sel_item_id);
            let mut id = 1.0;
            while e.result().env.item(id).is_some() {
                id += 1.0;
            }
            let mut r = root(&e.save_xml().expect("save"));
            let items = child_mut(&mut r, "Items", 0).expect("<Items>");
            let active = items.attr("activeItemSet").unwrap_or("1").to_owned();
            let mut it = XmlElement {
                name: "Item".into(),
                attrib: Default::default(),
                children: vec![XmlNode::Text(RING.into())],
                empty: false,
            };
            it.attrib.insert("id".into(), pob_lua::tostring_num(id));
            items.children.insert(0, XmlNode::Element(it));
            for c in &mut items.children {
                if let XmlNode::Element(set) = c
                    && set.name == "ItemSet"
                    && set.attr("id") == Some(active.as_str())
                {
                    for sl in &mut set.children {
                        if let XmlNode::Element(x) = sl
                            && x.attr("name") == Some("Ring 1")
                        {
                            x.attrib.insert("itemId".into(), pob_lua::tostring_num(id));
                        }
                    }
                }
            }
            let reference = compose(&r);
            let c = g.path(
                &format!("{slug}: addItemText(Ring 1)"),
                &mut e,
                &base,
                &reference,
                |e| e.add_item_text(RING, "Ring 1").map(|_| ()),
                |e| e.equip_item("Ring 1", prev),
            );
            eff(&mut g, "addItemText", c, &mut per_mutator);
        }

        // setMainSocketGroup
        let n = e.socket_group_count().min(groups_in_xml);
        if n >= 2 {
            let other = if main == 1 { 2 } else { 1 };
            let mut r = root(&e.save_xml().expect("save"));
            child_mut(&mut r, "Build", 0)
                .expect("<Build>")
                .attrib
                .insert("mainSocketGroup".into(), other.to_string());
            child_mut(&mut r, "Build", 0)
                .expect("<Build>")
                .attrib
                .shift_remove("mainSkillIndex");
            let reference = compose(&r);
            let c = g.path(
                &format!("{slug}: setMainSocketGroup({other})"),
                &mut e,
                &base,
                &reference,
                |e| e.set_main_socket_group(other),
                |e| e.set_main_socket_group(main),
            );
            eff(&mut g, "setMainSocketGroup", c, &mut per_mutator);
        }
    }

    // no hidden state: A then B in one thread == B alone in a fresh thread
    for w in list.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let mut ea = Engine::load_xml(&a.1).expect("A");
        ea.set_config("conditionKilledRecently", Some(pob_build::InputValue::Bool(true)))
            .expect("A mutation");
        let eb = Engine::load_xml(&b.1).expect("B");
        let after_a = stats(&eb);
        let xb = b.1.clone();
        let fresh = std::thread::spawn(move || stats(&Engine::load_xml(&xb).expect("B fresh")))
            .join()
            .expect("thread");
        g.eq(&format!("load {} -> load {} == fresh {}", a.0, b.0, b.0), &after_a, &fresh);
        per_mutator.entry("load A -> load B").or_default().0 += 1;
    }

    g.t_recalc.sort_by(f64::total_cmp);
    let med = g.t_recalc.get(g.t_recalc.len() / 2).copied().unwrap_or(0.0);
    println!(
        "path equivalence: {} checks over {} scenarios; per mutator (runs, of them changing the output) {per_mutator:?}; recalc after a mutation: median {med:.1} ms (n={})",
        g.checks,
        list.len(),
        g.t_recalc.len()
    );
    assert!(g.bad.is_empty(), "{} failures:\n{}", g.bad.len(), g.bad.join("\n"));
}
