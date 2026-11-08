//! The search on a golden build: a tried variant leaves no trace, moves keep to the points, and a search result is what a fresh load gives.

use pob_engine::engine::Engine;
use pob_optimizer::{Eval, Goal, Options, Rejected, apply, evaluate, moves, optimize, recalculate, score, violation};

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
    let list = moves(&base, 40, 7);
    assert!(!list.is_empty(), "no move on the golden");
    let start = evaluate(&Engine::load_xml(&xml).expect("load"));
    let mut changed = 0;
    for m in list.iter().take(12) {
        let Ok(s) = apply(&base, *m) else { continue };
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
    let r = optimize(&xml, &goal, opt).expect("search");
    assert!(r.tried > 0);
    assert!(r.best_score >= 1.0, "the best is never worse than the start");
    let again = evaluate(&Engine::load_xml(&r.best_xml).expect("reload"));
    assert_eq!(again, r.best, "the saved build scores what the search said");
    if let Some(last) = r.steps.last() {
        assert_eq!(last.eval, r.best);
    }
}
