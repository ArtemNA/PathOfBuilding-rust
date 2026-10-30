//! Prints `<build>\t<key>\t<f64 bits hex>` for every numeric stat of the six goldens, as parsed by pob-golden.
//! Diffed against `tools/golden-bits.mjs` (JS JSON.parse) to prove the float parser is exact (MINEFIELD-RUST R1).

fn main() {
    for b in pob_golden::GOLDEN_BUILDS {
        let snap = b.load_snapshot().unwrap_or_else(|e| panic!("{e}"));
        for (k, v) in &snap.stats {
            if let Some(x) = v.as_num() {
                println!("{}\t{k}\t{:016x}", b.name, x.to_bits());
            }
        }
    }
}
