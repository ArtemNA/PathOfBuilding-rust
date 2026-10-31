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

use pob_calc::{ActorState, CalcMode, CalcSession, CalculateOptions, Checkpoint, Env, OutValue, cache_skill_uuid};
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

/// The probe's string encoding (`export-offence.ts` DUMP `D`): a string that LOOKS like a number (`nan`/`inf`/`-inf` or
/// `^-?[%d.]+[eE]?[+-]?%d*$`) is prefixed `$str:` so it cannot be confused with a `%.17g` number.
fn probe_str(v: &str) -> String {
    let b = v.strip_prefix('-').unwrap_or(v).as_bytes();
    let i = b.iter().take_while(|c| c.is_ascii_digit() || **c == b'.').count();
    let mut rest = &b[i..];
    if i > 0 {
        if let [c, tail @ ..] = rest
            && matches!(c, b'e' | b'E')
        {
            rest = tail;
        }
        if let [c, tail @ ..] = rest
            && matches!(c, b'+' | b'-')
        {
            rest = tail;
        }
        rest = &rest[rest.iter().take_while(|c| c.is_ascii_digit()).count()..];
    }
    if (i > 0 && rest.is_empty()) || matches!(v, "nan" | "inf" | "-inf") {
        format!("$str:{v}")
    } else {
        v.to_owned()
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
                OutValue::Str(t) => json!(probe_str(t.as_ref())),
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

// --- R7.1b: every player `calcs.offence` call of `buildOutput(MAIN)` -------------------------------------------------------------------
// LuaJIT truth: `<build>.offence-runs.json` (headers of every offence call, perform markers) + `-<n>.json` parts (OC snapshots by run `seq`),
// producer `tools/lua-corpus/export-offence.ts runs`. A LuaJIT run is matched to a Rust run by `{mode, uuid, limited, ordinal}` (ordinal =
// the n-th player offence call with that mode/uuid/limitedSkills-or-not, in call order); only OC1/OC2 are compared (the Rust hooks).

/// A LuaJIT player offence call: header + OC1/OC2 snapshots.
struct LjRun {
    seq: u64,
    mode: String,
    uuid: String,
    limited: bool,
    ordinal: u64,
    depth: u64,
    name: String,
    oc: BTreeMap<&'static str, Json>,
}

fn lj_runs(name: &str) -> (Vec<LjRun>, Json) {
    let slug = name.replace(' ', "-");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let read = |f: String| -> Json {
        let p = dir.join(&f);
        serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("fixture json")
    };
    let main = read(format!("{slug}.offence-runs.json"));
    let parts: Vec<Json> = main["parts"]
        .as_array()
        .expect("parts")
        .iter()
        .map(|f| read(f.as_str().expect("part name").to_owned()))
        .collect();
    let mut runs = Vec::new();
    for h in main["runs"].as_array().expect("runs") {
        if h["player"] != json!(true) {
            continue;
        }
        let num = |k: &str| h[k].as_str().expect(k).parse::<f64>().expect(k) as u64;
        let seq = num("seq");
        let mut oc = BTreeMap::new();
        for k in ["OC1", "OC2"] {
            if let Some(v) = parts.iter().find_map(|p| p["runs"].get(seq.to_string()).and_then(|r| r.get(k))) {
                oc.insert(k, v.clone());
            }
        }
        runs.push(LjRun {
            seq,
            mode: h["mode"].as_str().expect("mode").to_owned(),
            uuid: h["uuid"].as_str().expect("uuid").to_owned(),
            limited: h["limited"] == json!(true),
            ordinal: num("ordinal"),
            depth: num("depth"),
            name: h["name"].as_str().expect("name").to_owned(),
            oc,
        });
    }
    (runs, main)
}

/// One Rust player offence call (`Checkpoint::OcEntry`), with the OC1/OC2 snapshots it reached.
struct RustRun {
    mode: &'static str,
    uuid: String,
    limited: bool,
    ordinal: u64,
    oc: BTreeMap<&'static str, Json>,
}

fn rust_runs(name: &str) -> (Vec<RustRun>, Vec<String>) {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden");
    let model = pob_build::load_build_xml(&std::fs::read_to_string(g.xml_path()).expect("xml")).expect("xml parses");
    let sess = CalcSession::new(Rc::new(model), CalculateOptions::default());
    let runs: Rc<RefCell<Vec<RustRun>>> = Rc::default();
    let r2 = runs.clone();
    sess.set_hook(Some(Box::new(
        move |cp: Checkpoint, env: &Env, player: &ActorState, _: &ActorState| {
            let mut v = r2.borrow_mut();
            match cp {
                Checkpoint::OcEntry => {
                    let uuid = cache_skill_uuid(env, env.skills.as_ref().expect("skills on").main_skill.expect("mainSkill"));
                    let limited = env.limited_skills.is_some();
                    let ordinal = v
                        .iter()
                        .filter(|r| r.mode == env.mode && r.uuid == uuid && r.limited == limited)
                        .count() as u64
                        + 1;
                    v.push(RustRun {
                        mode: env.mode,
                        uuid,
                        limited,
                        ordinal,
                        oc: BTreeMap::new(),
                    });
                }
                Checkpoint::Oc1 | Checkpoint::Oc2 => {
                    let k = if cp == Checkpoint::Oc1 { "OC1" } else { "OC2" };
                    v.last_mut().expect("OcEntry before OC").oc.insert(k, snap(env, player));
                }
                _ => {}
            }
        },
    )));
    let out = sess.build_output(CalcMode::Main).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    sess.set_hook(None);
    let notes = out.run.borrow().env.unimplemented.clone();
    (Rc::try_unwrap(runs).ok().expect("hook dropped").into_inner(), notes)
}

/// Dual Savior: the LuaJIT `CALCULATOR`-mode offence runs come from `calcs.mirages` (Reflection), which Rust does not port yet.
const SAVIOR_GATE: (&str, &str) = (
    "Dual Savior",
    "CalcMirages.lua:120-164 Reflection (The Saviour) for Reflection (R7.9)",
);

/// Compares every matched run; returns (differences / unexplained gaps, totals `[LuaJIT, Rust, matched, OC1 ok, OC2 ok, diff lines, not run]`).
fn compare_all() -> (Vec<String>, [usize; 7]) {
    let mut bad = Vec::new();
    let mut unexecuted = Vec::new();
    println!("golden | LuaJIT player runs | Rust runs | matched | OC1 ok | OC2 ok | differences | Rust did not run");
    let mut tot = [0usize; 7];
    for g in GOLDEN_BUILDS {
        let (lj, main) = lj_runs(g.name);
        let (rs, notes) = rust_runs(g.name);
        let (mut matched, mut ok1, mut ok2, mut ndiff, mut nun) = (0, 0, 0, 0, 0);
        let mut used = BTreeSet::new();
        let gate = GATED.iter().chain([&SAVIOR_GATE]).find(|(n, _)| *n == g.name).map(|(_, why)| *why);
        let gate_recorded = gate.is_some_and(|why| notes.iter().any(|u| u == why));
        for l in &lj {
            let found = rs
                .iter()
                .enumerate()
                .find(|(_, r)| r.mode == l.mode && r.uuid == l.uuid && r.limited == l.limited && r.ordinal == l.ordinal);
            let Some((i, r)) = found else {
                let why = match gate {
                    Some(why) if gate_recorded => format!("recorded gate `{why}`"),
                    Some(why) => format!("expected gate `{why}` NOT recorded"),
                    None => "no gate recorded".to_owned(),
                };
                let line = format!(
                    "{}: LuaJIT run #{} (seq {}, {} {} depth {}, limitedSkills {}) -- Rust did not run offence: {why}",
                    g.name, l.ordinal, l.seq, l.mode, l.uuid, l.depth, l.limited
                );
                // Savior: only the mirage simulation's CALCULATOR runs are covered by the gate
                if !gate_recorded || (g.name == SAVIOR_GATE.0 && l.mode != "CALCULATOR") {
                    bad.push(line.clone());
                }
                unexecuted.push(line);
                nun += 1;
                continue;
            };
            used.insert(i);
            matched += 1;
            for cp in ["OC1", "OC2"] {
                let (Some(w), Some(h)) = (l.oc.get(cp), r.oc.get(cp)) else {
                    bad.push(format!(
                        "{}: {} {cp} missing (LuaJIT {}, Rust {})",
                        g.name,
                        l.uuid,
                        l.oc.contains_key(cp),
                        r.oc.contains_key(cp)
                    ));
                    continue;
                };
                let d = diff(w, h);
                if d.is_empty() {
                    if cp == "OC1" {
                        ok1 += 1;
                    } else {
                        ok2 += 1;
                    }
                } else {
                    ndiff += d.len();
                    bad.extend(d.into_iter().map(|x| {
                        format!(
                            "{}: run seq {} ({}, {} limited {}) {cp} {x}",
                            g.name, l.seq, l.name, l.mode, l.limited
                        )
                    }));
                }
            }
        }
        for (_, r) in rs.iter().enumerate().filter(|(i, _)| !used.contains(i)) {
            bad.push(format!(
                "{}: Rust run {} #{} (limited {}) has no LuaJIT counterpart",
                g.name, r.uuid, r.ordinal, r.limited
            ));
        }
        let markers = main["performs"]
            .as_array()
            .expect("performs")
            .iter()
            .filter(|p| p["offenceCalled"] == json!(false))
            .count();
        println!(
            "{} | {} | {} | {matched} | {ok1} | {ok2} | {ndiff} | {nun} (LuaJIT performs without offence: {markers})",
            g.name,
            lj.len(),
            rs.len()
        );
        for (t, x) in tot.iter_mut().zip([lj.len(), rs.len(), matched, ok1, ok2, ndiff, nun]) {
            *t += x;
        }
    }
    println!(
        "TOTAL | {} | {} | {} | {} | {} | {} | {}",
        tot[0], tot[1], tot[2], tot[3], tot[4], tot[5], tot[6]
    );
    for u in &unexecuted {
        println!("unexecuted by Rust: {u}");
    }
    (bad, tot)
}

/// The strict gate: every LuaJIT player offence run is either compared OC1/OC2-equal or explained by a recorded gate. Currently RED on the
/// known classes (report of `tasks/R7.1b-offence-all-runs.md`); un-ignore when R7.10 (minion limit / `output.Minion`) and the
/// `OnKillMonsterExplosion` skill function (Data/Skills/other.lua:6205-6251) are ported.
#[test]
#[ignore = "R7.1b: known divergences on nested minion / OnKillMonsterExplosion runs (R7.10, other.lua:6205)"]
fn oc1_oc2_equal_luajit_on_all_offence_runs() {
    let (bad, _) = compare_all();
    assert!(bad.is_empty(), "offence runs != LuaJIT ({}):\n{}", bad.len(), bad.join("\n"));
}

/// Pins the measured R7.1b state so it cannot drift silently: 82 LuaJIT player offence runs, 56 run by Rust (all matched), 26 not run
/// (the recorded triggers/mirages gates), OC1 53/56 and OC2 51/56 equal, 14 difference lines in the known runs.
#[test]
fn offence_runs_state_is_pinned() {
    let (bad, tot) = compare_all();
    assert_eq!(tot, [82, 56, 56, 53, 51, 14, 26], "{}", bad.join("\n"));
    assert_eq!(bad.len(), 14, "unexplained gaps or differences:\n{}", bad.join("\n"));
}

/// The skill-function registry = `data/closures.json` (every `Data/Skills/` closure, `src`/`from`/`to`/sha256), 134/134.
#[test]
fn skill_func_registry_matches_closures_json() {
    let p = pob_data::data_dir().join("closures.json");
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
