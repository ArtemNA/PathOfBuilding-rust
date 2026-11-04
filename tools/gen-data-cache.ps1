# Writes data.cache beside data/ from the locked JSON (crates/pob-data/src/bincache.rs): for a package and for a machine that wants the fast start. Without it the program reads the JSON;
# after tools/sync-data.ps1 the old file no longer matches data.lock and is ignored until this runs again. It writes nothing into data/.
$ErrorActionPreference = 'Stop'
cargo run --release -p pob-data --example gen_data_cache
