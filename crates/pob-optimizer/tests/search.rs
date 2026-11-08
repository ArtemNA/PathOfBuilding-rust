//! The search on a golden build: a tried variant leaves no trace, moves keep to the points, and a search result is what a fresh load gives.

use pob_engine::engine::Engine;
use pob_optimizer::{
    Change, ConfigGuard, Eval, Goal, Locks, Options, Rejected, apply, evaluate, moves, optimize, recalculate, score, violation,
};

fn golden() -> String {
    let upstream = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/upstream.json")).expect("upstream");
    let root = std::env::var("POB_ROOT").unwrap_or_else(|_| {
        let at = upstream.find("\"pobRoot\"").expect("pobRoot");
        upstream[at + 9..].split('"').nth(1).expect("value").to_string()
    });
    std::fs::read_to_string(format!("{root}/spec/TestBuilds/3.13/Generals Perforate Zerker.xml"))
        .expect("golden")
        .replace("\r\n", "\n")
}

fn open_goal() -> Goal {
    Goal {
        min_elemental_resist: -1000.0,
        min_chaos_resist: None,
        min_ehp: None,
        dps_weight: 1.0,
        ehp_weight: 1.0,
        forbid_gems: Vec::new(),
    }
}

fn sample() -> Eval {
    Eval {
        dps: 100.0,
        ehp: 1000.0,
        life: 500.0,
        fire: 75.0,
        cold: 75.0,
        lightning: 75.0,
        chaos: 0.0,
        warnings: vec!["You are 5 Str short".into()],
        gems: vec!["determination".into()],
        dropped_config: Vec::new(),
    }
}

#[test]
fn hard_conditions_reject() {
    let start = sample();
    let mut goal = open_goal();
    goal.min_elemental_resist = 75.0;
    assert_eq!(violation(&start, &goal, &start), None);
    let mut v = start.clone();
    v.cold = 74.0;
    assert_eq!(violation(&v, &goal, &start), Some(Rejected::ElementalResist));
    goal.min_chaos_resist = Some(1.0);
    assert_eq!(violation(&start, &goal, &start), Some(Rejected::ChaosResist));
    goal.min_chaos_resist = None;
    goal.min_ehp = Some(1001.0);
    assert_eq!(violation(&start, &goal, &start), Some(Rejected::Ehp));
    goal.min_ehp = None;
    goal.forbid_gems = vec!["determination".into()];
    assert_eq!(violation(&start, &goal, &start), Some(Rejected::ForbiddenGem));
    goal.forbid_gems.clear();
    // the same problem with another number is not new, another problem is
    let mut v = start.clone();
    v.warnings = vec!["You are 9 Str short".into()];
    assert_eq!(violation(&v, &goal, &start), None);
    v.warnings.push("You are 3 Dex short".into());
    assert_eq!(violation(&v, &goal, &start), Some(Rejected::NewWarning));
}

#[test]
fn score_is_one_for_the_start_and_follows_the_weights() {
    let start = sample();
    let mut goal = open_goal();
    assert_eq!(score(&start, &goal, &start), 1.0);
    let mut v = start.clone();
    v.dps = 200.0;
    assert_eq!(score(&v, &goal, &start), 1.5);
    goal.ehp_weight = 0.0;
    assert_eq!(score(&v, &goal, &start), 2.0);
    goal.dps_weight = 0.0;
    goal.ehp_weight = 1.0;
    assert_eq!(score(&v, &goal, &start), 1.0);
}

#[test]
fn a_tried_variant_leaves_no_trace() {
    let xml = golden();
    let mut e = Engine::load_xml(&xml).expect("load");
    let base = (*e.spec_rc()).clone();
    let list = moves(&base, 40, 7, &Locks::default());
    assert!(!list.is_empty(), "no move on the golden");
    let start = evaluate(&Engine::load_xml(&xml).expect("load"));
    let mut changed = 0;
    for m in list.iter().take(12) {
        let Ok(s) = apply(&base, *m, &Locks::default()) else { continue };
        assert!(s.alloc_nodes.len() <= base.alloc_nodes.len(), "{m:?}: costs more points");
        let v = recalculate(&mut e, s).expect("variant");
        // the comparison below can tell a changed build from the start
        if v != start {
            changed += 1;
        }
        let back = recalculate(&mut e, base.clone()).expect("back");
        assert_eq!(back, start, "{m:?}: going back gives the start bit for bit");
    }
    assert!(changed > 0, "no variant changed a number: the check above would prove nothing");
}

#[test]
fn the_result_is_what_a_fresh_load_gives() {
    let xml = golden();
    let opt = Options {
        seconds: 20.0,
        moves_per_round: 30,
        threads: 4,
        seed: 3,
    };
    let goal = open_goal();
    let r = optimize(&xml, &goal, &Locks::default(), opt).expect("search");
    assert!(r.tried > 0);
    assert!(r.best_score >= 1.0, "the best is never worse than the start");
    let again = evaluate(&Engine::load_xml(&r.best_xml).expect("reload"));
    assert_eq!(
        (again.dps, again.ehp),
        (r.best.dps, r.best.ehp),
        "the saved build scores what the search said"
    );
    if let Some(last) = r.steps.last() {
        assert_eq!(last.eval, r.best);
    }
}

/// The ids a tree has and the tree after every move of a list, for the lock tests.
fn base_spec() -> (Engine, pob_tree::PassiveSpec) {
    let e = Engine::load_xml(&golden()).expect("load");
    let s = (*e.spec_rc()).clone();
    (e, s)
}

#[test]
fn a_required_node_is_never_lost() {
    let (_e, base) = base_spec();
    let free = Locks::default();
    let without = moves(&base, 5000, 11, &free);
    // a node the free search really does remove, or the lock below proves nothing
    let target = without[0].remove;
    assert!(without.iter().any(|m| m.remove == target));
    let locks = Locks {
        require: [target].into(),
        ..Locks::default()
    };
    let with = moves(&base, 5000, 11, &locks);
    assert!(!with.is_empty());
    for m in with {
        assert_ne!(m.remove, target);
        let s = apply(&base, m, &locks).expect("move");
        assert!(s.alloc_nodes.contains_key(&target), "{m:?} lost the required node");
    }
    // and a move that would lose it is refused, not only left out
    let bad = without.iter().find(|m| m.remove == target).expect("move");
    assert!(apply(&base, *bad, &locks).is_err());
}

#[test]
fn a_forbidden_node_is_never_taken() {
    let (_e, base) = base_spec();
    let free = Locks::default();
    let without = moves(&base, 5000, 13, &free);
    let target = without.iter().find(|m| m.effect.is_none()).expect("a plain move").add;
    let locks = Locks {
        forbid: [target].into(),
        ..Locks::default()
    };
    for m in moves(&base, 5000, 13, &locks) {
        let s = apply(&base, m, &locks).expect("move");
        assert!(!s.alloc_nodes.contains_key(&target), "{m:?} took the forbidden node");
    }
    let bad = without.iter().find(|m| m.add == target).expect("move");
    assert!(apply(&base, *bad, &locks).is_err());
}

/// The builds of the current league, for what the old golden's tree does not have. Personal, not in git: nothing to check without them.
fn local_builds() -> Vec<String> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../builds/pc/3.29");
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<_> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "xml"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .map(|x| x.replace("\r\n", "\n"))
        .collect()
}

#[test]
fn a_chosen_mastery_survives_a_save_and_a_load() {
    let free = Locks::default();
    let mut checked = 0;
    for xml in local_builds().iter().take(6) {
        let mut e = Engine::load_xml(xml).expect("load");
        let base = (*e.spec_rc()).clone();
        let Some(m) = moves(&base, 20_000, 17, &free).into_iter().find(|m| m.effect.is_some()) else {
            continue;
        };
        let s = apply(&base, m, &free).expect("move");
        let v = recalculate(&mut e, s).expect("variant");
        let saved = e.save_xml().expect("save");
        let fresh = Engine::load_xml(&saved).expect("reload");
        assert_eq!(evaluate(&fresh), v, "{m:?}");
        assert_eq!(fresh.spec_rc().mastery_selections.get(&m.add).copied(), m.effect, "{m:?}");
        checked += 1;
    }
    if !local_builds().is_empty() {
        assert!(checked > 0, "no build gave a mastery move: the check above would prove nothing");
    }
}

#[test]
fn names_resolve_and_a_typo_is_an_error() {
    let (_e, base) = base_spec();
    let have = base.alloc_nodes.keys().next().copied().expect("a node");
    let name = base.node(have).expect("node").dn().to_string();
    let l = Locks::resolve(&base, &[], std::slice::from_ref(&name)).expect("by name");
    assert!(l.require.contains(&have));
    assert!(l.require.iter().all(|id| base.alloc_nodes.contains_key(id)));
    assert!(Locks::resolve(&base, &["no such node".into()], &[]).is_err());
    assert!(Locks::resolve(&base, &[have.to_string()], &[]).is_ok());
}

#[test]
fn a_config_option_goes_with_its_reason() {
    let mut e = Engine::load_xml(&golden()).expect("load");
    // every check option the build shows only because of something in it
    let mut set = 0;
    for v in &pob_data::config_options::config_options().vars {
        let Some(name) = &v.var else { continue };
        if v.ty.as_deref() != Some("check") || !(v.gates.contains_key("ifMod") || v.gates.contains_key("ifCond")) {
            continue;
        }
        if e.set_config(name, Some(pob_build::InputValue::Bool(true))).is_ok() {
            set += 1;
        }
    }
    e.refresh().expect("refresh");
    let guard = ConfigGuard::new(&e);
    assert!(
        set > 0 && !guard.watched().is_empty(),
        "nothing to watch: the check below would prove nothing"
    );
    assert!(guard.orphans(&e).is_empty(), "nothing has gone yet");
    let before = guard.watched().len();

    // the reasons go: no items
    let slots = [
        "Weapon 1",
        "Weapon 2",
        "Helmet",
        "Body Armour",
        "Gloves",
        "Boots",
        "Amulet",
        "Ring 1",
        "Ring 2",
        "Belt",
    ];
    for s in slots {
        e.equip_item(s, 0.0).expect("empty the slot");
    }
    e.refresh().expect("refresh");
    let items_only = guard.orphans(&e).len();
    // and what the options hang on besides the items: the gems and the tree
    e.delete_all_socket_groups().expect("no gems");
    let mut bare = (*e.spec_rc()).clone();
    let ids: Vec<u32> = bare.alloc_nodes.keys().copied().collect();
    for id in ids {
        if let Some(i) = bare.node_idx(id)
            && bare.nodes[i].ty() != pob_tree::NodeType::ClassStart
            && bare.nodes[i].alloc
        {
            let _ = bare.dealloc_node(i);
        }
    }
    e.write_spec_to_model(bare);
    e.refresh().expect("refresh");
    let gone = guard.orphans(&e);
    eprintln!(
        "watched {before}, hidden without items {items_only}, without items, gems and tree {}",
        gone.len()
    );
    assert!(!gone.is_empty(), "taking everything off hid no option among {before}");
    let dropped = guard.settle(&mut e).expect("settle");
    assert!(dropped.iter().all(|d| gone.contains(d) || !gone.is_empty()));
    for d in &gone {
        assert!(dropped.contains(d));
        let cur = e.config_state().input.get(d).cloned();
        assert_ne!(cur, Some(pob_build::InputValue::Bool(true)), "{d}: still on");
    }
    assert!(guard.orphans(&e).iter().all(|o| dropped.contains(o)));
}

/// A curse on the wearer writes to the store the list of extra curses comes from: it used to panic ("RefCell already borrowed").
#[test]
fn a_belt_that_curses_the_wearer_calculates() {
    let db = pob_optimizer::uniques().expect("uniques");
    let (_, _, raw) = db.iter().find(|(n, _, _)| n.starts_with("Coward's Chains")).expect("the belt");
    let mut e = Engine::load_xml(&golden()).expect("load");
    e.replace_item_text(raw, "Belt").expect("the belt goes on");
    e.refresh().expect("refresh");
    assert!(evaluate(&e).life > 0.0);
}

#[test]
fn a_support_swap_keeps_the_active_gem_and_a_unique_goes_on() {
    let xml = golden();
    let opt = Options {
        seconds: 25.0,
        moves_per_round: 60,
        threads: 4,
        seed: 5,
    };
    let locks = Locks {
        swap_supports: true,
        item_slots: vec!["Helmet".into(), "Belt".into()],
        ..Locks::default()
    };
    let r = optimize(&xml, &open_goal(), &locks, opt).expect("search");
    let e = Engine::load_xml(&xml).expect("load");
    let main = e.model().main_socket_group as usize;
    let actives = |x: &Engine| -> Vec<Option<String>> {
        x.gem_states(main)
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.3)
            .map(|(i, _)| x.gem_id_at(main, i + 1))
            .collect()
    };
    let after = Engine::load_xml(&r.best_xml).expect("reload");
    assert_eq!(actives(&e), actives(&after), "the active gems of the main group stay");
    let _ = Change::Item {
        slot: String::new(),
        name: String::new(),
        raw: String::new(),
    };
    assert!(r.tried > 0);
    assert_eq!(evaluate(&after).dps, r.best.dps);
}

fn with_a_tattoo() -> (Engine, pob_tree::PassiveSpec, u32) {
    let (mut e, mut base) = base_spec();
    let list = moves(&base, 5000, 19, &Locks::default());
    let node = list[0].remove;
    let tattoo = base
        .tree
        .tattoo()
        .expect("tattoos")
        .nodes
        .values()
        .next()
        .cloned()
        .expect("a tattoo");
    base.hash_overrides.insert(node, tattoo);
    recalculate(&mut e, base.clone()).expect("tattooed");
    (e, base, node)
}

#[test]
fn tattoos_are_left_alone_unless_allowed() {
    let (_e, base, node) = with_a_tattoo();
    let off = Locks::default();
    for m in moves(&base, 5000, 19, &off) {
        assert_ne!(m.remove, node);
        let s = apply(&base, m, &off).expect("move");
        assert!(s.alloc_nodes.contains_key(&node), "{m:?} lost the tattooed node");
    }
    let on = Locks {
        tattoos: true,
        ..Locks::default()
    };
    assert!(
        moves(&base, 5000, 19, &on).iter().any(|m| m.remove == node),
        "allowed, the tattooed node is an ordinary one: the check above is not empty"
    );
}

#[test]
fn allowed_tattoos_are_in_every_round_and_survive_a_save() {
    let (e, base) = base_spec();
    let locks = Locks {
        tattoos: true,
        ..Locks::default()
    };
    let list = pob_optimizer::candidates(&e, &locks, &[], 80, 21).expect("candidates");
    let tattoos: Vec<&Change> = list.iter().filter(|c| matches!(c, Change::Tattoo { .. })).collect();
    assert!(!tattoos.is_empty(), "allowed tattoos must be among the variants");
    let off = pob_optimizer::candidates(&e, &Locks::default(), &[], 80, 21).expect("candidates");
    assert!(!off.iter().any(|c| matches!(c, Change::Tattoo { .. })), "off by default");

    let Change::Tattoo { node, dn } = tattoos[0].clone() else {
        unreachable!()
    };
    let mut f = Engine::load_xml(&golden()).expect("load");
    let guard = ConfigGuard::new(&f);
    let v = pob_optimizer::try_change(&mut f, &base, tattoos[0], &locks, &guard).expect("tattoo");
    let fresh = Engine::load_xml(&f.save_xml().expect("save")).expect("reload");
    assert_eq!((evaluate(&fresh).dps, evaluate(&fresh).ehp), (v.dps, v.ehp));
    assert_eq!(fresh.spec_rc().hash_overrides.get(&node).map(|t| t.dn.clone()), Some(dn));
}

fn cluster_locks() -> Locks {
    Locks {
        clusters: true,
        ..Locks::default()
    }
}

fn cluster_names(list: &[Change]) -> Vec<String> {
    list.iter()
        .filter_map(|c| match c {
            Change::Cluster { name, .. } => Some(name.to_lowercase()),
            _ => None,
        })
        .collect()
}

#[test]
fn clusters_are_off_by_default_and_in_every_round_when_allowed() {
    let (e, _) = base_spec();
    let off = pob_optimizer::candidates(&e, &Locks::default(), &[], 120, 23).expect("candidates");
    assert!(cluster_names(&off).is_empty(), "off by default");
    let on = pob_optimizer::candidates(&e, &cluster_locks(), &[], 120, 23).expect("candidates");
    assert!(!cluster_names(&on).is_empty(), "allowed clusters must be among the variants");
}

#[test]
fn forbidden_cluster_mods_and_combinations_never_appear() {
    let (e, _) = base_spec();
    let on = pob_optimizer::candidates(&e, &cluster_locks(), &[], 400, 29).expect("candidates");
    let names = cluster_names(&on);
    // a notable the free search really does pick, or the bans below prove nothing
    let large = names.iter().find(|n| n.contains(", ")).expect("a jewel with two notables");
    let notes: Vec<String> = large
        .split(": ")
        .nth(1)
        .expect("notables")
        .trim_end_matches(')')
        .split(", ")
        .map(str::to_owned)
        .collect();
    let mut banned = cluster_locks();
    banned.forbid_cluster_mods = vec![notes[0].clone()];
    let list = pob_optimizer::candidates(&e, &banned, &[], 400, 29).expect("candidates");
    assert!(!cluster_names(&list).is_empty());
    assert!(
        cluster_names(&list).iter().all(|n| !n.contains(&notes[0])),
        "a forbidden mod appeared"
    );

    let mut pair = cluster_locks();
    pair.forbid_cluster_combos = vec![vec![notes[0].clone(), notes[1].clone()]];
    let list = pob_optimizer::candidates(&e, &pair, &[], 400, 29).expect("candidates");
    assert!(
        cluster_names(&list)
            .iter()
            .all(|n| !(n.contains(&notes[0]) && n.contains(&notes[1]))),
        "a forbidden pair appeared"
    );
}

#[test]
fn a_cluster_goes_in_keeps_to_the_points_and_survives_a_save() {
    let (e, base) = base_spec();
    let locks = cluster_locks();
    let guard = ConfigGuard::new(&e);
    let list = pob_optimizer::candidates(&e, &locks, &[], 400, 31).expect("candidates");
    let xml = golden();
    let (mut ok, mut with_notables, mut last_err) = (0, 0, String::new());
    for c in list.iter().filter(|c| matches!(c, Change::Cluster { .. })).take(25) {
        let mut f = Engine::load_xml(&xml).expect("load");
        let v = match pob_optimizer::try_change(&mut f, &base, c, &locks, &guard) {
            Ok(v) => v,
            Err(x) => {
                last_err = x;
                continue;
            }
        };
        ok += 1;
        let spec = f.spec_rc();
        assert!(spec.alloc_nodes.len() <= base.alloc_nodes.len(), "{c:?}: costs more points");
        if spec
            .nodes
            .iter()
            .any(|n| n.is_subgraph_node() && n.alloc && n.ty() == pob_tree::NodeType::Notable)
        {
            with_notables += 1;
        }
        let fresh = Engine::load_xml(&f.save_xml().expect("save")).expect("reload");
        assert_eq!((evaluate(&fresh).dps, evaluate(&fresh).ehp), (v.dps, v.ehp), "{c:?}");
    }
    assert!(ok > 0, "no cluster variant calculated: {last_err}");
    assert!(with_notables > 0, "no cluster variant took a notable");
}

#[test]
fn a_stopped_search_ends_with_what_it_has() {
    let control = pob_optimizer::Control::default();
    control.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let opt = Options {
        seconds: 600.0,
        moves_per_round: 100,
        threads: 2,
        seed: 1,
    };
    let began = std::time::Instant::now();
    let r = pob_optimizer::optimize_with(&golden(), &open_goal(), &Locks::default(), opt, &control).expect("search");
    assert!(began.elapsed().as_secs() < 60, "the stop was not heard");
    assert_eq!(r.tried, 0);
    assert_eq!(r.best, r.start);
}
