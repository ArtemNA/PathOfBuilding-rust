//! Lua-pattern micro-bench on the R1.5 D workloads (`tools/lua-corpus/export-patterns.ts`, LuaJIT side):
//! (1) full pass of fixture A (`patterns x lines` `find`), (2) `scan`-like loop over `specialModList` (ModParser.lua:6616-6638),
//! (3) `gsub("(%d+)", "%1")` over the lines of A. Same protocol as the Lua chunk: double reps until one batch >= 300 ms,
//! then 5 timed batches; ns/op median [min-max].
//! `cargo run --release -p pob-lua --example bench_patterns`

use std::hint::black_box;
use std::time::Instant;

use pob_lua::{Repl, find, gsub};

fn unhex(h: &str) -> Vec<u8> {
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
        .collect()
}

fn bytes(tok: &serde_json::Value) -> Vec<u8> {
    let t = tok.as_str().unwrap();
    match t.split_at(1) {
        ("s", rest) => rest.as_bytes().to_vec(),
        ("x", rest) => unhex(rest),
        _ => panic!("bad token {t:?}"),
    }
}

fn measure(name: &str, ops: usize, mut f: impl FnMut() -> usize) {
    let mut reps = 1usize;
    loop {
        let t = Instant::now();
        for _ in 0..reps {
            black_box(f());
        }
        if t.elapsed().as_millis() >= 300 {
            break;
        }
        reps *= 2;
    }
    let mut ns: Vec<f64> = (0..5)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..reps {
                black_box(f());
            }
            t.elapsed().as_nanos() as f64 / (reps * ops) as f64
        })
        .collect();
    ns.sort_by(f64::total_cmp);
    let ms = |x: f64| x * ops as f64 / 1e6;
    println!(
        "{name:<26} ops/rep={ops:<8} reps={reps:<5} {:>8.1} ns/op [{:.1}-{:.1}]  {:>9.3} ms/rep [{:.3}-{:.3}]",
        ns[2],
        ns[0],
        ns[4],
        ms(ns[2]),
        ms(ns[0]),
        ms(ns[4])
    );
}

fn main() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/luajit-patterns-modparser.json");
    let fx: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let pats: Vec<(Vec<u8>, bool, bool)> = fx["patterns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let special = r[1].is_null() && r[2].as_str().unwrap().split(',').any(|t| t == "specialModList");
            (bytes(&r[0]), r[1].as_bool().unwrap_or(false), special)
        })
        .collect();
    let lines: Vec<Vec<u8>> = fx["lines"].as_array().unwrap().iter().map(bytes).collect();
    let spec: Vec<&[u8]> = pats.iter().filter(|p| p.2).map(|p| p.0.as_slice()).collect();
    println!("patterns {}, lines {}, specialModList {}", pats.len(), lines.len(), spec.len());

    measure("full_pass_find", pats.len() * lines.len(), || {
        let mut n = 0;
        for (pat, plain, _) in &pats {
            for line in &lines {
                if find(line, pat, Some(1), *plain).unwrap().is_some() {
                    n += 1;
                }
            }
        }
        n
    });

    measure("scan_like_specialModList", lines.len() * spec.len(), || {
        let mut n = 0;
        for line in &lines {
            let line_lower = line.to_ascii_lowercase();
            let mut best: Option<(usize, usize, &[u8], pob_lua::Captures)> = None;
            for &pattern in &spec {
                if let Some((index, end_index, caps)) = find(&line_lower, pattern, Some(1), false).unwrap() {
                    let better = match &best {
                        None => true,
                        Some((bi, be, bp, _)) => {
                            index < *bi || (index == *bi && (end_index > *be || (end_index == *be && pattern.len() > bp.len())))
                        }
                    };
                    if better {
                        best = Some((index, end_index, pattern, caps));
                    }
                }
            }
            if best.is_some() {
                n += 1;
            }
        }
        n
    });

    measure("gsub_digits", lines.len(), || {
        let mut n = 0;
        for line in &lines {
            n += gsub(line, b"(%d+)", Repl::Str(b"%1"), None).unwrap().0.len();
        }
        n
    });
}
