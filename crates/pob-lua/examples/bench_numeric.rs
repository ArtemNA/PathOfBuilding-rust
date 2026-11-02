//! Micro-bench of the R1.1 operations on the same inputs as tools/lua-corpus/perf-numeric.ts (LuaJIT / pob-ts side):
//! tostring (%.14g), format("%.2f"), tonumber, round(a, 2), a ^ 0.9, math.sin. ns/op, median [min-max] of 7 runs >= 200 ms each.
//! `cargo run --release -p pob-lua --example bench_numeric`

use std::hint::black_box;
use std::time::Instant;

use pob_lua::{Arg, format, number, tonumber, tostring_num};

fn num(tok: &str) -> f64 {
    match tok {
        "nan" | "-nan" => f64::NAN,
        "inf" => f64::INFINITY,
        "-inf" => f64::NEG_INFINITY,
        _ => tok.parse().unwrap(),
    }
}

fn bench(name: &str, n: usize, mut f: impl FnMut()) {
    let mut reps = 1usize;
    loop {
        let t = Instant::now();
        for _ in 0..reps {
            f();
        }
        if t.elapsed().as_millis() >= 200 {
            break;
        }
        reps *= 2;
    }
    let mut ns: Vec<f64> = (0..7)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..reps {
                f();
            }
            t.elapsed().as_nanos() as f64 / (reps * n) as f64
        })
        .collect();
    ns.sort_by(f64::total_cmp);
    println!("{name:<22} n={n:<5} {:>8.1} ns/op  [{:.1}-{:.1}]", ns[3], ns[0], ns[6]);
}

fn main() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/luajit-numeric.json");
    let fx: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let all: Vec<f64> = fx["tostring"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| num(r[0].as_str().unwrap()))
        .collect();
    let finite: Vec<f64> = all.iter().copied().filter(|v| v.is_finite() && v.abs() < 1e15).collect();
    let strs: Vec<String> = fx["tonumber"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let h = r[0].as_str().unwrap().trim_start_matches('x');
            let b: Vec<u8> = (0..h.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
                .collect();
            String::from_utf8(b).unwrap()
        })
        .collect();

    bench("tostring (%.14g)", all.len(), || {
        for &v in &all {
            black_box(tostring_num(black_box(v)));
        }
    });
    // Realistic inputs: every numeric stat of the six goldens (the corpus above is mostly random 17-digit doubles, the worst case).
    let goldens = std::env::var("POB_TS_ROOT").unwrap_or_else(|_| "C:/pob-work/pob-ts".into()) + "/tools/oracle/goldens";
    let stats: Vec<f64> = std::fs::read_dir(&goldens)
        .map(|d| {
            d.flatten()
                .flat_map(|e| {
                    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
                    j["stats"]
                        .as_object()
                        .unwrap()
                        .values()
                        .filter_map(serde_json::Value::as_f64)
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .unwrap_or_default();
    if !stats.is_empty() {
        bench("tostring (golden stats)", stats.len(), || {
            for &v in &stats {
                black_box(tostring_num(black_box(v)));
            }
        });
    }
    bench("format(\"%.2f\")", finite.len(), || {
        for &v in &finite {
            black_box(format("%.2f", &[Arg::Num(black_box(v))]).unwrap());
        }
    });
    bench("tonumber", strs.len(), || {
        for s in &strs {
            black_box(tonumber(black_box(s)));
        }
    });
    bench("round(a, 2)", finite.len(), || {
        for &v in &finite {
            black_box(number::round(black_box(v), Some(2.0)));
        }
    });
    // The line above folds `10 ^ 2` at compile time; a caller in another crate, or a variable `dec`, does not.
    bench("round(a, dec)", finite.len(), || {
        for &v in &finite {
            black_box(number::round(black_box(v), Some(black_box(2.0))));
        }
    });
    bench("a ^ 0.9", finite.len(), || {
        for &v in &finite {
            black_box(black_box(v.abs()).powf(0.9));
        }
    });
    bench("math.sin", finite.len(), || {
        for &v in &finite {
            black_box(black_box(v).sin());
        }
    });
}
