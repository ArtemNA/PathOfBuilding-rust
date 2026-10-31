//! R6.5 gate (R6-ARCH §3 (б)): the perform checkpoint CP1 -- the state at the first `doActorLifeMana(env.player)` (the end of
//! `doActorAttribsConditions`, CalcPerform.lua:546 called from :1944: after the skill loop :1334-1512 and the flasks :1513-1942) -- against the
//! LuaJIT probe of R6.1 (`tests/fixtures/<build>.skills.json`, `checkpoints.CP1`, producer `tools/lua-corpus/export-skills.ts`), 6 goldens,
//! `ParseMode::Full`, skills on, the XML as raw bytes (what the docker probe saw).
//!
//! Compared EXACTLY for the player and the enemy: the modDB as a multiset of `modLib.formatSourceMod` strings (the order of a ModDB
//! bucket comes from `pairs` over string-keyed tables in LuaJIT -- per process, MINEFIELD-RUST R13), the mod count, `conditions`,
//! `multipliers` (`%.17g`) and `output` (`%.17g` / string / boolean). No tolerance, no skipped key, no pin.
//!
//! The checkpoints are taken through `perform_with`'s hook (`Checkpoint`), not a copy of perform. R6.6: CP2 -- after the first
//! `doActorLifeManaReservation(env.player)` (:2092; attributes/pools :1943-1966 and the per-skill reservation loop :1978-2090) -- is gated
//! the same way. R6.7: CP3 -- the entry of `calcs.defenceForConditionals` (:3484; heralds/auras :2164, buffs/debuffs/curses/guards
//! :2193-3377, Choir/extra auras/keystones/Dancing Dervish :3394-3482) -- too, with ONE proven class pinned by name ([`TABLE_OUTPUT`]).
//! R6.8: CP4 -- the entry of `calcs.defence` (:3792; misc buffs :3489, non-damaging ailments and chill/shock multipliers :3497-3622, the
//! enemy's charges/misc :3625-3626, exposures/consecrated ground :3627-3665, ally life pools :3667-3790) -- the same way, the same pin.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use pob_calc::{ActorState, Checkpoint, Env, InitEnvOptions, OutValue, ParseMode, init_env_with, perform_with};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_lua::format_g;
use pob_mods::format::format_source_mod;
use pob_mods::{StoreKind, StoreRef};
use serde_json::{Map, Value as Json, json};

fn fixture(name: &str) -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.skills.json", name.replace(' ', "-")));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("fixture json")
}

fn n17(x: f64) -> Json {
    Json::String(format_g(x, 17))
}

/// The probe's `snapActor`: `modDB` (a sorted multiset), `modCount`, `conditions`, `multipliers`, `output`.
fn snap(db: &StoreRef, st: &ActorState) -> Json {
    let s = db.borrow();
    let StoreKind::Db(d) = &s.kind else { panic!("not a ModDB") };
    let mut mods: Vec<String> = d.mods.values().flat_map(|b| b.iter().map(|m| format_source_mod(m))).collect();
    mods.sort();
    let conditions: Map<String, Json> = s.conditions.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    let multipliers: Map<String, Json> = s.multipliers.iter().map(|(k, v)| (k.to_string(), n17(*v))).collect();
    let output: Map<String, Json> = st
        .output
        .iter()
        .map(|(k, v)| {
            let j = match v {
                OutValue::Num(x) => n17(*x),
                OutValue::Str(t) => json!(t.to_string()),
                OutValue::Bool(b) => json!(b),
            };
            (k.to_string(), j)
        })
        .collect();
    json!({ "modDB": mods, "modCount": n17(s_count(&mods)), "conditions": conditions, "multipliers": multipliers, "output": output })
}

fn s_count(m: &[String]) -> f64 {
    m.len() as f64
}

fn run(name: &str) -> BTreeMap<&'static str, Json> {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden");
    let model = pob_build::load_build_xml(&std::fs::read_to_string(g.xml_path()).expect("xml")).expect("xml parses");
    let mut env: Env = init_env_with(
        Rc::new(model),
        InitEnvOptions {
            parse_mode: ParseMode::Full,
            ..InitEnvOptions::default()
        },
        true,
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut cps: BTreeMap<&'static str, Json> = BTreeMap::new();
    let mut hook = |cp: Checkpoint, env: &Env, player: &ActorState, enemy: &ActorState| {
        let key = match cp {
            Checkpoint::Cp1 => "CP1",
            Checkpoint::Cp2 => "CP2",
            Checkpoint::Cp3 => "CP3",
            Checkpoint::Cp4 => "CP4",
        };
        let pdb = env.player.borrow().mod_db.clone();
        let edb = env.enemy.borrow().mod_db.clone();
        cps.entry(key)
            .or_insert_with(|| json!({ "player": snap(&pdb, player), "enemy": snap(&edb, enemy) }));
    };
    let _ = perform_with(&mut env, Some(pob_calc::defence), Some(pob_calc::estimations), Some(&mut hook));
    cps
}

/// Every difference between LuaJIT's snapshot and Rust's, as text lines.
fn diff_actor(who: &str, lj: &Json, rust: &Json) -> Vec<String> {
    let mut out = Vec::new();
    // modDB: multiset difference
    let mut want: BTreeMap<String, i64> = BTreeMap::new();
    for m in lj["modDB"].as_array().expect("modDB") {
        *want.entry(m.as_str().unwrap().to_owned()).or_default() += 1;
    }
    for m in rust["modDB"].as_array().expect("modDB") {
        *want.entry(m.as_str().unwrap().to_owned()).or_default() -= 1;
    }
    for (m, c) in &want {
        if *c > 0 {
            out.push(format!("{who}.modDB: only LuaJIT x{c}: {m}"));
        } else if *c < 0 {
            out.push(format!("{who}.modDB: only Rust x{}: {m}", -c));
        }
    }
    if lj["modCount"] != rust["modCount"] {
        out.push(format!("{who}.modCount: lj {} rust {}", lj["modCount"], rust["modCount"]));
    }
    for part in ["conditions", "multipliers", "output"] {
        let (w, g) = (lj[part].as_object().expect(part), rust[part].as_object().expect(part));
        let keys: BTreeSet<&String> = w.keys().chain(g.keys()).collect();
        for k in keys {
            let (a, b) = (w.get(k).unwrap_or(&Json::Null), g.get(k).unwrap_or(&Json::Null));
            if a != b {
                out.push(format!("{who}.{part}.{k}: lj {a} rust {b}"));
            }
        }
    }
    out
}

/// Proven class `output-table-value` (R6-ARCH §6, R6.6): CalcPerform.lua:2156 `output["Req"..attr.."Item"] = out.source` stores the
/// requirement-source TABLE in `output`; Rust's `Output` holds numbers/strings/booleans only (the key never reaches `getStats`). Pinned: a
/// difference on such a key is allowed only when LuaJIT has a table and Rust nothing; any other difference on it fails.
const TABLE_OUTPUT: [&str; 4] = ["ReqStrItem", "ReqDexItem", "ReqIntItem", "ReqOmniItem"];

/// Removes the pinned `output-table-value` differences of a checkpoint (`player.output.Req<Attr>Item: lj {table} rust null`); returns
/// how many.
fn strip_table_output(d: &mut Vec<String>) -> usize {
    let before = d.len();
    d.retain(|x| {
        !TABLE_OUTPUT
            .iter()
            .any(|k| x.starts_with(&format!("player.output.{k}: lj {{")) && x.ends_with(" rust null"))
    });
    before - d.len()
}

#[test]
fn cp1_cp2_cp3_cp4_equal_luajit_on_six_goldens() {
    let mut all_bad = Vec::new();
    for g in GOLDEN_BUILDS {
        let lj = fixture(g.name);
        let cps = run(g.name);
        for cp in ["CP1", "CP2", "CP3", "CP4"] {
            let (Some(w), Some(r)) = (lj["checkpoints"].get(cp), cps.get(cp)) else {
                panic!(
                    "{}: checkpoint {cp} missing (lj {}, rust {})",
                    g.name,
                    lj["checkpoints"].get(cp).is_some(),
                    cps.contains_key(cp)
                );
            };
            let mut d = diff_actor("player", &w["player"], &r["player"]);
            d.extend(diff_actor("enemy", &w["enemy"], &r["enemy"]));
            let pinned = strip_table_output(&mut d);
            println!(
                "{}: {cp} player {} mods / {} conditions / {} multipliers / {} output, enemy {} mods / {} conditions / {} multipliers -- differences {}, pinned output-table-value {pinned}",
                g.name,
                r["player"]["modCount"].as_str().unwrap(),
                r["player"]["conditions"].as_object().unwrap().len(),
                r["player"]["multipliers"].as_object().unwrap().len(),
                r["player"]["output"].as_object().unwrap().len(),
                r["enemy"]["modCount"].as_str().unwrap(),
                r["enemy"]["conditions"].as_object().unwrap().len(),
                r["enemy"]["multipliers"].as_object().unwrap().len(),
                d.len()
            );
            all_bad.extend(d.into_iter().map(|x| format!("{}: {cp} {x}", g.name)));
        }
    }
    assert!(
        all_bad.is_empty(),
        "CP1/CP2/CP3/CP4 != LuaJIT ({}):\n{}",
        all_bad.len(),
        all_bad.join("\n")
    );
}

/// `cargo test --release -p pob-calc --test perform_cp perf -- --ignored --nocapture`: wall time of `perform` from its entry to CP1, to CP2, to
/// CP3 and to CP4 (median of 50 runs after a warm-up, a fresh `initEnv` per run) -- the Rust side of `tools/.scratch/perf-skills.json` (LuaJIT
/// perform -> CP4).
#[test]
#[ignore = "timing; not a check"]
fn perf_perform_to_checkpoints() {
    for g in GOLDEN_BUILDS {
        let text = std::fs::read_to_string(g.xml_path()).expect("xml");
        let (mut cp1, mut cp2, mut cp3, mut cp4) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for run in 0..51 {
            let model = pob_build::load_build_xml(&text).expect("xml parses");
            let opts = InitEnvOptions {
                parse_mode: ParseMode::Full,
                ..InitEnvOptions::default()
            };
            let mut env = init_env_with(Rc::new(model), opts, true).expect("initEnv");
            let t0 = std::time::Instant::now();
            let (mut t1, mut t2, mut t3, mut t4) = (None, None, None, None);
            let mut hook = |cp: Checkpoint, _: &Env, _: &ActorState, _: &ActorState| match cp {
                Checkpoint::Cp1 => t1 = Some(t0.elapsed()),
                Checkpoint::Cp2 => t2 = Some(t0.elapsed()),
                Checkpoint::Cp3 => t3 = Some(t0.elapsed()),
                Checkpoint::Cp4 => t4 = Some(t0.elapsed()),
            };
            let _ = perform_with(&mut env, Some(pob_calc::defence), Some(pob_calc::estimations), Some(&mut hook));
            if run > 0 {
                cp1.push(t1.unwrap().as_secs_f64() * 1000.0);
                cp2.push(t2.unwrap().as_secs_f64() * 1000.0);
                cp3.push(t3.unwrap().as_secs_f64() * 1000.0);
                cp4.push(t4.unwrap().as_secs_f64() * 1000.0);
            }
        }
        let med = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        println!(
            "{}: perform -> CP1 {:.3} ms, perform -> CP2 {:.3} ms, perform -> CP3 {:.3} ms, perform -> CP4 {:.3} ms (median of {})",
            g.name,
            med(&mut cp1),
            med(&mut cp2),
            med(&mut cp3),
            med(&mut cp4),
            cp1.len()
        );
    }
}
