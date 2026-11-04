//! Writes `data.cache` beside `data/` from the JSON of the locked data (never into `data/`).  cargo run --release -p pob-data --example gen_data_cache

fn main() {
    let dir = pob_data::ensure_data().expect("data.lock");
    let lock = pob_data::verify_lock(dir, &pob_data::lock_path(dir)).expect("data.lock");
    let out = pob_data::bincache::cache_path(dir);
    let n = pob_data::bincache::write_cache(dir, &lock.manifest_sha256, &out).expect("data.cache");
    let size = std::fs::metadata(&out).expect("data.cache").len();
    println!("{}: {n} tables, {:.1} MB", out.display(), size as f64 / 1e6);
}
