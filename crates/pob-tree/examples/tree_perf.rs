//! R3.3 perf: 3_13 tree build cold (first in the process: tree JSON + mod cache load + build) and warm (data loaded, a
//! fresh `PassiveTree::build`), and `PassiveSpec::load` per golden (tree cached), ms. Run several processes for the cold median:
//! `cargo run --release -p pob-tree --example tree_perf`.

use std::time::Instant;

use pob_build::{active_spec, load_build_xml};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_tree::{ParseMode, PassiveSpec, PassiveTree, load_passive_tree_mode};

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let runs = 7;
    let t = Instant::now();
    let _ = load_passive_tree_mode("3_13", ParseMode::CacheOnly).unwrap();
    println!("cold 3_13 (data load + build): {:.2} ms", ms(t));
    let warm: Vec<f64> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            let _ = PassiveTree::build_mode("3_13", ParseMode::CacheOnly).unwrap();
            ms(t)
        })
        .collect();
    println!("warm 3_13 build: median {:.2} ms of {runs} {warm:.2?}", median(warm.clone()));
    for g in GOLDEN_BUILDS {
        let m = load_build_xml(&std::fs::read_to_string(g.xml_path()).unwrap()).unwrap();
        let sm = active_spec(&m);
        let v: Vec<f64> = (0..runs)
            .map(|_| {
                let t = Instant::now();
                let _ = PassiveSpec::load_mode(sm, ParseMode::CacheOnly).unwrap();
                ms(t)
            })
            .collect();
        println!("Spec::load {}: median {:.3} ms of {runs}", g.name, median(v));
    }
}
