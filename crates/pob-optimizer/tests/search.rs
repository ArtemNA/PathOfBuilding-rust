//! The search on a golden build: a tried variant leaves no trace, moves keep to the points, and a search result is what a fresh load gives.

use pob_engine::engine::Engine;
use pob_optimizer::{Eval, Goal, Locks, Options, Rejected, apply, evaluate, moves, optimize, recalculate, score, violation};

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
    assert_eq!(again, r.best, "the saved build scores what the search said");
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
