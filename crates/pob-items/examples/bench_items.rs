//! R3.4 perf: `Item::parse` (ParseRaw + ModRange + BuildModList, as ItemsTab:Load does) for all 128 items of the 6 goldens
//! (`cargo run -p pob-items --release --example bench_items`). The data files and the ModCache are loaded by a discarded
//! warm-up pass; each run loops for >= 200 ms; 7 runs, median [min - max] in microseconds per item.

#[path = "../tests/support/dump.rs"]
mod dump;

use std::hint::black_box;
use std::time::{Duration, Instant};

use pob_golden::registry::GOLDEN_BUILDS;
use pob_items::Item;

const RUNS: usize = 7;
const MIN: Duration = Duration::from_millis(200);

fn main() {
    let models: Vec<_> = GOLDEN_BUILDS
        .iter()
        .map(|g| pob_build::load_build_xml(&std::fs::read_to_string(g.xml_path()).expect("xml")).expect("model"))
        .collect();
    let items: Vec<_> = models
        .iter()
        .flat_map(|m| m.items.iter())
        .map(|im| (im.raw.clone(), dump::options_of(im, pob_parser::ParseMode::CacheOnly)))
        .collect();
    let pass = || {
        let mut kept = 0usize;
        for (raw, opts) in &items {
            let item = Item::parse(raw, opts);
            kept += usize::from(item.base.is_some());
            black_box(item);
        }
        kept
    };
    let kept = pass(); // warm-up: data + ModCache
    let mut per_item = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let t = Instant::now();
        let mut passes = 0u32;
        while t.elapsed() < MIN {
            black_box(pass());
            passes += 1;
        }
        per_item.push(t.elapsed().as_secs_f64() * 1e6 / f64::from(passes) / items.len() as f64);
    }
    per_item.sort_by(f64::total_cmp);
    println!(
        "Item::parse (ParseRaw + BuildModList), {} items ({} with a base), 6 goldens: {:.2} us/item [{:.2} - {:.2}], {RUNS} runs",
        items.len(),
        kept,
        per_item[RUNS / 2],
        per_item[0],
        per_item[RUNS - 1]
    );
}
