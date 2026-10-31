//! The parser on the ModCache corpus (23 207 lines), bypassing the cache: the wrapper logic (ModParser.lua:7000-7005) over
//! the inner `parseMod`. Without LuaJIT fixtures the reference is the cache itself -- informational (D9: the cache may be
//! stale against the current parser); `modparser_luajit.rs` is the gate.

use std::collections::HashMap;
use std::time::Instant;

use pob_parser::scan::{mod_eq, tie_stats};
use pob_parser::{ParseError, parse_mod_uncached, parser_stats, reset_parser_stats};

#[derive(Default, Debug)]
struct Tally {
    same: usize,
    differ: usize,
    unported: usize,
    lua: usize,
    unrepresentable: usize,
    by_id: HashMap<String, usize>,
    differ_examples: Vec<String>,
    other_examples: Vec<String>,
}

#[test]
fn modcache_corpus_against_the_cache() {
    let cache = pob_data::mod_cache::mod_cache();
    reset_parser_stats();
    let t0 = Instant::now();
    let mut t = Tally::default();
    for e in &cache.entries {
        match parse_mod_uncached(&e.line) {
            Ok(r) => {
                let same = r.extra.as_deref() == e.extra.as_deref()
                    && match (&r.mod_list, &e.mods) {
                        (None, None) => true,
                        (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| mod_eq(x, y)),
                        _ => false,
                    }
                    && r.orphan_tags.len() == e.orphan_tags.len();
                if same {
                    t.same += 1;
                } else {
                    t.differ += 1;
                    if t.differ_examples.len() < 40 {
                        t.differ_examples.push(format!(
                            "{:?}\n   got {:?} / {:?}\n  want {:?} / {:?}",
                            e.line, r.mod_list, r.extra, e.mods, e.extra
                        ));
                    }
                }
            }
            Err(ParseError::Unported(id)) => {
                t.unported += 1;
                *t.by_id.entry(id.to_string()).or_default() += 1;
            }
            Err(ParseError::Lua(err)) => {
                t.lua += 1;
                if t.other_examples.len() < 20 {
                    t.other_examples.push(format!("{:?}: lua {err}", e.line));
                }
            }
            Err(ParseError::Unrepresentable(s)) => {
                t.unrepresentable += 1;
                if t.other_examples.len() < 20 {
                    t.other_examples.push(format!("{:?}: unrepresentable {s}", e.line));
                }
            }
        }
    }
    let el = t0.elapsed();
    let mut ids: Vec<_> = t.by_id.iter().collect();
    ids.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!(
        "corpus {}: = cache {}, != cache {}, unported {} ({} ids), lua errors {}, unrepresentable {}; {:.1} us/line",
        cache.len(),
        t.same,
        t.differ,
        t.unported,
        ids.len(),
        t.lua,
        t.unrepresentable,
        el.as_secs_f64() * 1e6 / cache.len() as f64
    );
    println!("top unported: {:?}", &ids[..ids.len().min(15)]);
    for x in &t.differ_examples {
        println!("DIFF {x}");
    }
    for x in &t.other_examples {
        println!("ERR {x}");
    }
    let ts = tie_stats();
    println!(
        "ties: scans {} ties {} diff_value {} diff_caps {}",
        ts.scans, ts.ties, ts.diff_value, ts.diff_caps
    );
    for x in &ts.examples {
        println!("TIE {x}");
    }
    let ps = parser_stats();
    println!("jewel multi {:?}; doubled writes {:?}", ps.jewel_multi, ps.doubled_writes);
    assert_eq!(t.same + t.differ + t.unported + t.lua + t.unrepresentable, cache.len());
}

/// L25: a parsed `Condition:X` / `Multiplier:X` mod gated by a tag on the same variable recurses forever in EvalMod.
/// Counted on the corpus lines the parser answers (informational: the parser is bug-compatible either way).
#[test]
fn self_referencing_condition_mods_on_the_corpus() {
    let cache = pob_data::mod_cache::mod_cache();
    let mut hits = Vec::new();
    for e in &cache.entries {
        let Ok(r) = parse_mod_uncached(&e.line) else { continue };
        for m in r.mod_list.iter().flatten() {
            for (prefix, ty) in [("Condition:", "Condition"), ("Multiplier:", "Multiplier")] {
                let Some(var) = m.name.strip_prefix(prefix) else { continue };
                let same = m.tags.iter().any(|t| {
                    t.get("type").as_str() == Some(ty)
                        && (t.get("var").as_str() == Some(var)
                            || matches!(t.get("varList"), pob_mods::Value::Table(l) if l.array.iter().any(|x| x.as_str() == Some(var))))
                });
                if same {
                    hits.push(e.line.to_string());
                }
            }
        }
    }
    println!("L25 self-referencing Condition/Multiplier mods: {} {:?}", hits.len(), hits);
}

/// Parsing the 23 207 ModCache lines past the cache (wrapper logic, `parse_mod_uncached`): us/line, median of 5 runs.
/// `cargo test --release -p pob-parser --test corpus -- --ignored --nocapture`
#[test]
#[ignore]
fn perf_modcache_past_the_cache() {
    let cache = pob_data::mod_cache::mod_cache();
    let lines: Vec<&str> = cache.entries.iter().map(|e| &*e.line).collect();
    let run = || {
        let t = Instant::now();
        let mut ok = 0;
        for l in &lines {
            ok += usize::from(parse_mod_uncached(l).is_ok());
        }
        (t.elapsed().as_secs_f64() * 1e6 / lines.len() as f64, ok)
    };
    let _ = run();
    let mut us: Vec<f64> = (0..5).map(|_| run().0).collect();
    us.sort_by(f64::total_cmp);
    println!(
        "parse_mod_uncached over {} ModCache lines: median {:.2} us/line (runs {:?})",
        lines.len(),
        us[2],
        us
    );
    // the wrapper with the cache (every line a hit): lookup + `copyTable`
    let mut cached: Vec<f64> = (0..5)
        .map(|_| {
            let t = Instant::now();
            for l in &lines {
                let _ = pob_parser::parse_mod(l, false);
            }
            t.elapsed().as_secs_f64() * 1e6 / lines.len() as f64
        })
        .collect();
    cached.sort_by(f64::total_cmp);
    println!("parse_mod (cache hits) median {:.2} us/line", cached[2]);
}
