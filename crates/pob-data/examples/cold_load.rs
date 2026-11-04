//! Cold load of every R3 data file in a fresh process: one line per file and the total, in ms. From `data.cache` if there is one (POB_DATA_NO_CACHE=1: the JSON).
//! Run several times for a median:  cargo run --release -p pob-data --example cold_load

use std::time::Instant;

fn main() {
    let total = Instant::now();
    let mut lines = Vec::new();
    let mut time = |name: &str, f: &dyn Fn()| {
        let t = Instant::now();
        f();
        lines.push(format!("{name:<22} {:>8.1} ms", t.elapsed().as_secs_f64() * 1e3));
    };
    time("lock check", &|| {
        pob_data::ensure_data().expect("data.lock");
    });
    time("item-bases", &|| {
        pob_data::item_bases::item_bases();
    });
    time("constants", &|| {
        pob_data::constants::constants();
    });
    time("rules", &|| {
        pob_data::rules::rules();
    });
    time("globals", &|| {
        pob_data::globals::globals();
    });
    time("pantheons", &|| {
        pob_data::pantheons::pantheons();
    });
    time("config-options", &|| {
        pob_data::config_options::config_options();
    });
    time("mod-scalability", &|| {
        pob_data::mod_scalability::mod_scalability();
    });
    time("gems", &|| {
        pob_data::gems::gems();
    });
    time("cluster-jewels", &|| {
        pob_data::cluster_jewels::cluster_jewels();
    });
    time("mod-cache", &|| {
        pob_data::mod_cache::mod_cache();
    });
    for v in pob_data::tree::COMMITTED_TREE_VERSIONS {
        time(&format!("tree/{v}"), &|| {
            pob_data::tree::load_tree(v).expect("tree");
        });
    }
    for l in &lines {
        println!("{l}");
    }
    println!("{:<22} {:>8.1} ms", "TOTAL", total.elapsed().as_secs_f64() * 1e3);
}
