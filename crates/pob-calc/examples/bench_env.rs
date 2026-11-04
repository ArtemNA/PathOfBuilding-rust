//! R3.6 perf: "XML -> env" for the six goldens (R3-ARCH §3), release build.
//!
//!   cargo run --release -p pob-calc --example bench_env            -- warm: per build, median/min/max of RUNS runs of
//!                                                                     (a) XML -> env (XML parse + items + spec + init_env)
//!                                                                     (b) init_env alone (spec and items prepared untimed)
//!   cargo run --release -p pob-calc --example bench_env -- cold NAME  -- one XML -> env in a fresh process, data loading
//!                                                                     (serde of data/*.json) included; repeat per process
//!
//! Timings: `std::time::Instant`, milliseconds. The XML file is read before the clock starts in both modes.

use std::rc::Rc;
use std::time::Instant;

use indexmap::IndexMap;
use pob_build::{active_spec, load_build_xml};
use pob_calc::{InitEnvOptions, init_env, item_options};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_items::Item;
use pob_tree::PassiveSpec;

const RUNS: usize = 9;

fn xml_to_env(xml: &str) -> usize {
    let model = Rc::new(load_build_xml(xml).expect("xml"));
    let opts = InitEnvOptions {
        parse_mode: pob_calc::ParseMode::CacheOnly,
        ..InitEnvOptions::default()
    };
    let env = init_env(model, opts).expect("init_env");
    env.alloc_nodes.len()
}

fn stats(mut v: Vec<f64>) -> (f64, f64, f64) {
    v.sort_by(f64::total_cmp);
    (v[v.len() / 2], v[0], v[v.len() - 1])
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("cold") {
        let name = args.get(1).expect("cold NAME");
        let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden name");
        let xml = std::fs::read_to_string(g.xml_path()).expect("xml");
        let t = Instant::now();
        let n = xml_to_env(&xml);
        println!("{:.3}", t.elapsed().as_secs_f64() * 1e3);
        assert!(n > 0);
        return;
    }
    // Warm-up: load every data file the six builds need.
    for g in GOLDEN_BUILDS {
        xml_to_env(&std::fs::read_to_string(g.xml_path()).expect("xml"));
    }
    println!("build | XML->env warm median (min-max) ms | init_env only median (min-max) ms | XML parse + model | spec | items");
    for g in GOLDEN_BUILDS {
        let xml = std::fs::read_to_string(g.xml_path()).expect("xml");
        let mut full = Vec::new();
        let mut init = Vec::new();
        let (mut parse, mut specs, mut parsed) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..RUNS {
            let t = Instant::now();
            xml_to_env(&xml);
            full.push(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            let model = Rc::new(load_build_xml(&xml).expect("xml"));
            parse.push(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            let spec = PassiveSpec::load_mode(active_spec(&model), pob_calc::ParseMode::CacheOnly).expect("spec");
            specs.push(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            let mut items = IndexMap::new();
            for im in &model.items {
                let item = Item::parse(&im.raw, &item_options(im, pob_calc::ParseMode::CacheOnly));
                if item.base.is_some() {
                    items.insert(im.id as u64, Rc::new(item));
                }
            }
            parsed.push(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            let env = init_env(
                model,
                InitEnvOptions {
                    spec: Some(spec),
                    items: Some(items),
                    parse_mode: pob_calc::ParseMode::CacheOnly,
                },
            )
            .expect("init_env");
            init.push(t.elapsed().as_secs_f64() * 1e3);
            drop(env);
        }
        let (fm, fa, fb) = stats(full);
        let (im, ia, ib) = stats(init);
        println!(
            "{} | {fm:.3} ({fa:.3}-{fb:.3}) | {im:.3} ({ia:.3}-{ib:.3}) | {:.3} | {:.3} | {:.3}",
            g.name,
            stats(parse).0,
            stats(specs).0,
            stats(parsed).0
        );
    }
}
