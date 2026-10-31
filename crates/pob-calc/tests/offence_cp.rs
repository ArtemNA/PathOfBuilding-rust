//! R7.3 gate (R7-ARCH §3, card `tasks/R7.3-offence-part-a.md` p. 4): the offence checkpoints OC1 (CalcOffence.lua, before :1057) and OC2
//! (before :1987) of the main skill against the R7.1 LuaJIT probe (`tests/fixtures/<build>.offence.json`, producer
//! `tools/lua-corpus/export-offence.ts`), 6 goldens, `CalcSession::build_output(MAIN)`, the XML as raw bytes (what the docker probe saw).
//!
//! Compared EXACTLY: `output` (every key; numbers `%.17g`, strings, booleans), `skillFlags`, the player's `conditions` and `multipliers`,
//! and the counters (`playerModDB`, `enemyModDB`, `skillModList` mod counts). One proven class is pinned by name, the same as CP3/CP4
//! (`perform_cp.rs`): `output-table-value` -- `Req<Attr>Item` holds a TABLE in LuaJIT's output, Rust's `Output` keeps scalars only.
//!
//! Which run: `checkpoints` = the top perform (Rust: the hit whose env has no `limitedSkills`); `checkpointsFirstNested` (Dual Savior:
//! `calcs.mirages` skips the top offence) = the first nested run (`calcs.buildActiveSkill`, Rust: the first hit with `limitedSkills`).
//! Where CalcPerform.lua:3797-3798 takes a branch of `calcs.triggers` / `calcs.mirages` that is not ported (R7.8/R7.9), Rust does not run
//! offence for that env and records why; such a golden has no Rust checkpoint -- the test requires the recorded reason to be exactly the
//! expected one ([`GATED`]) and reports it, it never compares a checkpoint Rust did not make.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use pob_calc::{ActorState, CalcMode, CalcSession, CalculateOptions, Checkpoint, Env, OutValue};
use pob_golden::registry::GOLDEN_BUILDS;
use pob_lua::format_g;
use pob_mods::{StoreKind, StoreRef};
use serde_json::{Map, Value as Json, json};

fn fixture(name: &str) -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.offence.json", name.replace(' ', "-")));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("fixture json")
}

fn n17(x: f64) -> Json {
    Json::String(format_g(x, 17))
}

fn mod_count(db: &StoreRef) -> usize {
    match &db.borrow().kind {
        StoreKind::Db(d) => d.mods.values().map(Vec::len).sum(),
        StoreKind::List(l) => l.mods.len(),
    }
}

/// The probe's checkpoint snapshot, from Rust's state.
fn snap(env: &Env, player: &ActorState) -> Json {
    let st = env.skills.as_ref().expect("skills on");
    let i = st.main_skill.expect("mainSkill");
    let sk = &st.active_skill_list[i];
    let output: Map<String, Json> = player
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
    let flags: Map<String, Json> = sk.skill_flags.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    let pdb = env.player.borrow().mod_db.clone();
    let edb = env.enemy.borrow().mod_db.clone();
    let conditions: Map<String, Json> = pdb.borrow().conditions.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    let multipliers: Map<String, Json> = pdb.borrow().multipliers.iter().map(|(k, v)| (k.to_string(), n17(*v))).collect();
    json!({
        "output": output,
        "skillFlags": flags,
        "playerConditions": conditions,
        "playerMultipliers": multipliers,
        "playerModDB": mod_count(&pdb).to_string(),
        "enemyModDB": mod_count(&edb).to_string(),
        "skillModList": mod_count(&env.skill_mod_lists[i]).to_string(),
        "mainSkillName": st.skills.effects[sk.active_effect.borrow().granted_effect].name.clone(),
    })
}

/// A checkpoint hit: the env had `limitedSkills` (a nested run), the mode, the snapshot.
struct Hit {
    nested: bool,
    mode: &'static str,
    snap: Json,
}

/// Proven class `output-table-value` (perform_cp.rs, R6.6).
const TABLE_OUTPUT: [&str; 4] = ["ReqStrItem", "ReqDexItem", "ReqIntItem", "ReqOmniItem"];

/// The goldens whose top env takes an unported branch at CalcPerform.lua:3797-3798, and the reason Rust records (measured: the R7.1
/// fixtures' OC1 has `SkillTriggerRate`/`TriggerRateCap` (Cospris: Cast On Critical Strike), `skillFlags.mirageArcher` (Toxic Rain) and the
/// General's Cry mods on the skill list (Perforate: `skillModList` 10, `HasNoCost`)).
const GATED: [(&str, &str); 4] = [
    (
        "Dual Wield Cospris CoC - Lethal Pride",
        "CalcTriggers.lua:1156 configTable[\"cast on critical strike\"] trigger handler for Ice Spear (R7.8)",
    ),
    (
        "Dual Wield Cospris CoC",
        "CalcTriggers.lua:1156 configTable[\"cast on critical strike\"] trigger handler for Ice Spear (R7.8)",
    ),
    (
        "Generals Perforate Zerker",
        "CalcMirages.lua:346-426 General's Cry for Perforate (R7.9)",
    ),
    (
        "Mirage Archer Toxic Rain",
        "CalcMirages.lua:67-119 Mirage Archer for Toxic Rain (R7.9)",
    ),
];

/// Every difference between LuaJIT's checkpoint and Rust's, as text lines.
fn diff(lj: &Json, rust: &Json) -> Vec<String> {
    let mut out = Vec::new();
    for part in ["output", "skillFlags", "playerConditions", "playerMultipliers"] {
        let (w, g) = (lj[part].as_object().expect(part), rust[part].as_object().expect(part));
        let keys: BTreeSet<&String> = w.keys().chain(g.keys()).collect();
        for k in keys {
            let (a, b) = (w.get(k).unwrap_or(&Json::Null), g.get(k).unwrap_or(&Json::Null));
            if a == b {
                continue;
            }
            if part == "output" && TABLE_OUTPUT.contains(&k.as_str()) && a.is_object() && b.is_null() {
                continue;
            }
            out.push(format!("{part}.{k}: lj {a} rust {b}"));
        }
    }
    for k in ["playerModDB", "enemyModDB", "skillModList", "mainSkillName"] {
        if lj[k] != rust[k] {
            out.push(format!("{k}: lj {} rust {}", lj[k], rust[k]));
        }
    }
    out
}

fn run(name: &str) -> (BTreeMap<&'static str, Vec<Hit>>, Vec<String>) {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden");
    let model = pob_build::load_build_xml(&std::fs::read_to_string(g.xml_path()).expect("xml")).expect("xml parses");
    let sess = CalcSession::new(Rc::new(model), CalculateOptions::default());
    let hits: Rc<RefCell<BTreeMap<&'static str, Vec<Hit>>>> = Rc::default();
    let h2 = hits.clone();
    sess.set_hook(Some(Box::new(
        move |cp: Checkpoint, env: &Env, player: &ActorState, _: &ActorState| {
            let key = match cp {
                Checkpoint::Oc1 => "OC1",
                Checkpoint::Oc2 => "OC2",
                _ => return,
            };
            h2.borrow_mut().entry(key).or_default().push(Hit {
                nested: env.limited_skills.is_some(),
                mode: env.mode,
                snap: snap(env, player),
            });
        },
    )));
    let out = sess.build_output(CalcMode::Main).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    sess.set_hook(None);
    let notes = out.run.borrow().env.unimplemented.clone();
    let hits = Rc::try_unwrap(hits).ok().expect("hook dropped").into_inner();
    (hits, notes)
}

#[test]
fn oc1_oc2_equal_luajit_on_six_goldens() {
    let mut all_bad = Vec::new();
    let mut compared = 0;
    for g in GOLDEN_BUILDS {
        let lj = fixture(g.name);
        let (hits, notes) = run(g.name);
        let (cps, nested) = match lj.get("checkpointsFirstNested") {
            Some(c) => (c, true),
            None => (&lj["checkpoints"], false),
        };
        if let Some((_, why)) = GATED.iter().find(|(n, _)| *n == g.name) {
            let recorded = notes.iter().any(|u| u == why);
            let top_hit = hits.values().flatten().any(|h| !h.nested);
            println!(
                "{}: top env gated at CalcPerform.lua:3797-3798 -- {why} (recorded {recorded}, top offence ran {top_hit})",
                g.name
            );
            if !recorded || top_hit {
                all_bad.push(format!(
                    "{}: expected the gate `{why}` (recorded {recorded}), top offence ran {top_hit}",
                    g.name
                ));
            }
            continue;
        }
        for cp in ["OC1", "OC2"] {
            let w = &cps[cp];
            let want_mode = w.get("envMode").and_then(Json::as_str).unwrap_or("MAIN");
            let hit = hits
                .get(cp)
                .and_then(|v| v.iter().find(|h| h.nested == nested && h.mode == want_mode));
            let Some(hit) = hit else {
                all_bad.push(format!("{}: {cp} no Rust hit (nested {nested}, mode {want_mode})", g.name));
                continue;
            };
            let d = diff(w, &hit.snap);
            compared += 1;
            println!(
                "{}: {cp} ({}) output {} keys, skillFlags {}, conditions {}, multipliers {}, playerModDB {}, enemyModDB {}, skillModList {} -- differences {}",
                g.name,
                if nested { "first nested" } else { "top" },
                hit.snap["output"].as_object().unwrap().len(),
                hit.snap["skillFlags"].as_object().unwrap().len(),
                hit.snap["playerConditions"].as_object().unwrap().len(),
                hit.snap["playerMultipliers"].as_object().unwrap().len(),
                hit.snap["playerModDB"],
                hit.snap["enemyModDB"],
                hit.snap["skillModList"],
                d.len()
            );
            all_bad.extend(d.into_iter().map(|x| format!("{}: {cp} {x}", g.name)));
        }
    }
    println!("checkpoints compared: {compared}");
    assert!(all_bad.is_empty(), "OC1/OC2 != LuaJIT ({}):\n{}", all_bad.len(), all_bad.join("\n"));
}

/// The skill-function registry = `data/closures.json` (every `Data/Skills/` closure, `src`/`from`/`to`/sha256), 134/134.
#[test]
fn skill_func_registry_matches_closures_json() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/closures.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).expect("closures.json")).expect("json");
    let list: Vec<&Json> = j["closures"]
        .as_array()
        .expect("closures")
        .iter()
        .filter(|x| x["src"].as_str().is_some_and(|s| s.starts_with("Data/Skills/")))
        .collect();
    let reg = pob_calc::skill_funcs::skill_func_entries();
    let mut bad = Vec::new();
    for x in &list {
        let path = x["path"].as_str().unwrap();
        match reg.iter().find(|e| e.0 == path) {
            None => bad.push(format!("{path}: not in the registry")),
            Some(e) => {
                let same = x["src"].as_str() == Some(&format!("Data/Skills/{}", e.1))
                    && x["from"].as_u64() == Some(u64::from(e.2))
                    && x["to"].as_u64() == Some(u64::from(e.3))
                    && x["sha256"].as_str() == Some(e.4);
                if !same {
                    bad.push(format!("{path}: registry {}:{}-{} {} != {x}", e.1, e.2, e.3, e.4));
                }
            }
        }
    }
    for e in &reg {
        if !list.iter().any(|x| x["path"].as_str() == Some(e.0)) {
            bad.push(format!("{}: not in closures.json", e.0));
        }
    }
    let (p, u, l, names) = pob_calc::skill_funcs::skill_func_summary();
    println!(
        "registry {} / closures {}: ported {p}, unported {u}, later (R7.4/R7.5) {l}",
        reg.len(),
        list.len()
    );
    for n in &names {
        println!("  unported: {n}");
    }
    assert_eq!(list.len(), 134);
    assert_eq!(reg.len(), 134);
    assert!(bad.is_empty(), "registry drift:\n{}", bad.join("\n"));
}

/// Times of one top-level offence call: entry -> OC1, entry -> OC2.
type Times = Rc<RefCell<(Option<std::time::Instant>, Option<f64>, Option<f64>)>>;

/// `cargo test --release -p pob-calc --test offence_cp perf -- --ignored --nocapture`: wall time of offence part A of the main skill --
/// offence entry (CalcPerform.lua:3799) -> OC1 and -> OC2 -- on the first run that reaches it (the top perform; Dual Savior: the first
/// nested run), median of 50 after a warm-up, a fresh session per run. The Rust side of `tools/.scratch/perf-offence.json`.
#[test]
#[ignore = "timing; not a check"]
fn perf_offence_part_a() {
    for g in GOLDEN_BUILDS {
        let text = std::fs::read_to_string(g.xml_path()).expect("xml");
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for run in 0..51 {
            let model = pob_build::load_build_xml(&text).expect("xml parses");
            let sess = CalcSession::new(Rc::new(model), CalculateOptions::default());
            let t: Times = Rc::default();
            let t2 = t.clone();
            sess.set_hook(Some(Box::new(move |cp: Checkpoint, _: &Env, _: &ActorState, _: &ActorState| {
                let mut s = t2.borrow_mut();
                if s.2.is_some() {
                    return;
                }
                match cp {
                    Checkpoint::OcEntry => s.0 = Some(std::time::Instant::now()),
                    Checkpoint::Oc1 => s.1 = s.0.map(|t0| t0.elapsed().as_secs_f64() * 1000.0),
                    Checkpoint::Oc2 => s.2 = s.0.map(|t0| t0.elapsed().as_secs_f64() * 1000.0),
                    _ => {}
                }
            })));
            let _ = sess.build_output(CalcMode::Main).expect("buildOutput");
            sess.set_hook(None);
            let s = t.borrow();
            if run > 0
                && let (Some(x), Some(y)) = (s.1, s.2)
            {
                a.push(x);
                b.push(y);
            }
        }
        let med = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        println!(
            "{}: offence entry -> OC1 {:.4} ms, entry -> OC2 {:.4} ms (median of {})",
            g.name,
            med(&mut a),
            med(&mut b),
            a.len()
        );
    }
}
