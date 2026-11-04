//! Writes `src/` of a package (`src.cache` and `TreeData/`, one file per content) from upstream's `src/`, then checks every file and table of it against upstream; a difference is an error.
//!   cargo run --release -p pob-ui --example gen_src_cache -- <upstream src/> <package src/>

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, root, out] = &args[..] else {
        eprintln!("usage: gen_src_cache <upstream src/> <package src/>");
        std::process::exit(2);
    };
    let root = format!("{}/", root.replace('\\', "/").trim_end_matches('/'));
    let out = Path::new(out);
    let dir = pob_data::ensure_data().expect("data.lock");
    let lock = pob_data::verify_lock(dir, &pob_data::lock_path(dir)).expect("data.lock");
    let fail = |e: String| -> ! {
        eprintln!("gen_src_cache: {e}");
        std::process::exit(1);
    };
    let st = pob_ui::src_cache_gen::generate(&root, out, &lock.manifest_sha256).unwrap_or_else(|e| fail(e));
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    println!(
        "TreeData: {} files, {:.1} MB -> {} files, {:.1} MB ({} copies)",
        st.files_in,
        mb(st.bytes_in),
        st.unique,
        mb(st.unique_bytes),
        st.copies
    );
    println!("src.cache: {} tables, {:.1} MB", st.tables, mb(st.cache_bytes));
    println!(
        "tree.lua: text {:.1} MB, all fields {:.1} MB, what the art reads {:.1} MB",
        mb(st.tree_lua_text),
        mb(st.tree_lua_full),
        mb(st.tree_lua_projected)
    );
    let (files, tables) = pob_ui::src_cache_gen::verify(&root, out, &lock.manifest_sha256).unwrap_or_else(|e| fail(e));
    println!("verified: {files} files byte for byte, {tables} tables bit for bit, no .lua");
}
