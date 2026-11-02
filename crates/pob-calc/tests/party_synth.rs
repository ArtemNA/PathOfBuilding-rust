//! R-party: Rust = LuaJIT on the synthetic scenarios of `tools/lua-corpus/gen-party-synth.ts` (`tests/fixtures/party-synth/*.xml`; truth
//! `<name>.luajit.json`, `node tools/lua-corpus/export-real.ts --set party-synth`): the Party-tab branches no real build reaches (Warcry,
//! otherEffects, extraAura, AuraDebuff, Vaal auras, minions, `partyMembers.output`), Energy Blade and LinkedSupport. Every scalar of the
//! player's output and of `Minion.*` bit for bit, no key missing or extra.

use std::collections::BTreeMap;
use std::path::Path;

use pob_calc::{CalculateOptions, ParseMode, StatValue, calculate_file};
use pob_golden::compare::stats_exact;
use pob_golden::value::StatValue as Gv;
use serde_json::Value as Json;

fn lua_num(s: &str) -> Gv {
    match s {
        "nan" | "-nan" | "inf" | "-inf" => Gv::Str(s.to_owned()),
        _ => Gv::Num(s.parse::<f64>().unwrap_or_else(|e| panic!("bad number {s:?}: {e}"))),
    }
}

fn scalars(dump: &Json) -> BTreeMap<String, Gv> {
    let empty = serde_json::Map::new();
    let obj = |k: &str| dump[k].as_object().unwrap_or(&empty);
    let mut m = BTreeMap::new();
    for (k, v) in obj("nums") {
        m.insert(k.clone(), lua_num(v.as_str().expect("num string")));
    }
    for (k, v) in obj("strs") {
        m.insert(k.clone(), Gv::Str(v.as_str().expect("str").to_owned()));
    }
    for (k, v) in obj("bools") {
        m.insert(k.clone(), Gv::Bool(v.as_bool().expect("bool")));
    }
    m
}

fn to_golden(s: &BTreeMap<String, StatValue>) -> BTreeMap<String, Gv> {
    s.iter()
        .map(|(k, v)| {
            let g = match v {
                StatValue::Num(n) => Gv::Num(*n),
                StatValue::Str(x) => Gv::Str(x.clone()),
                StatValue::Bool(b) => Gv::Bool(*b),
            };
            (k.clone(), g)
        })
        .collect()
}

fn diff(prefix: &str, lua: &BTreeMap<String, Gv>, rust: &BTreeMap<String, Gv>, bad: &mut Vec<String>) -> usize {
    for (k, l) in lua {
        match rust.get(k) {
            Some(r) if stats_exact(l, r) => {}
            Some(r) => bad.push(format!("{prefix}{k}: rust {r:?} luajit {l:?}")),
            None => bad.push(format!("{prefix}{k}: rust nil luajit {l:?}")),
        }
    }
    for (k, r) in rust {
        if !lua.contains_key(k) {
            bad.push(format!("{prefix}{k}: rust {r:?} luajit nil"));
        }
    }
    lua.len()
}

#[test]
fn party_synth_matches_luajit() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/party-synth");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("party-synth")
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok()?.strip_suffix(".luajit.json").map(str::to_owned))
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no fixtures in {}", dir.display());
    let mut failed = 0;
    for name in &names {
        let fx: Json =
            serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.luajit.json"))).expect("fixture")).expect("json");
        assert!(
            fx["loadError"].is_null() && fx["calcError"].is_null(),
            "{name}: LuaJIT failed: {} {}",
            fx["loadError"],
            fx["calcError"]
        );
        let opts = CalculateOptions {
            parse_mode: ParseMode::Full,
            skills: true,
            ..CalculateOptions::default()
        };
        let r = calculate_file(dir.join(format!("{name}.xml")), opts).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut bad = Vec::new();
        let rust: BTreeMap<String, Gv> = to_golden(&r.stats.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
        let n = diff("", &scalars(&fx["output"]), &rust, &mut bad);
        let mut m = 0;
        match (fx["minion"].is_object(), r.perform.minion.as_ref()) {
            (true, Some(min)) => {
                let o = pob_calc::output_to_stats(&min.borrow().output);
                let rm = to_golden(&o.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
                m = diff("Minion.", &scalars(&fx["minion"]), &rm, &mut bad);
            }
            (false, None) => {}
            (l, r) => bad.push(format!("env.minion: luajit {l} rust {}", r.is_some())),
        }
        println!(
            "{name}: {n} player + {m} Minion keys, {} diffs; notes {:?}",
            bad.len(),
            r.unimplemented.len()
        );
        for b in bad.iter().take(40) {
            println!("  {b}");
        }
        failed += usize::from(!bad.is_empty());
    }
    assert_eq!(failed, 0, "{failed}/{} scenarios differ from LuaJIT", names.len());
}
