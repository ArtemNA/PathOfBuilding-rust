//! R7.3-R7.7 gate (R7-ARCH §3, cards `tasks/R7.3-offence-part-a.md` p. 4, `tasks/R7.4-offence-part-b.md` p. 3, `tasks/R7.6-offence-ailments.md`
//! p. 2, `tasks/R7.7-offence-part-e.md` p. 2): the offence checkpoints OC1 (CalcOffence.lua, before :1057), OC2 (before :1987), OC3 (before
//! :2606), OC4 (before :4156), OC5 (before :5772) and OC6 (the end, :6263) of the main skill against the R7.1 LuaJIT probe
//! (`tests/fixtures/<build>.offence.json`, producer `tools/lua-corpus/export-offence.ts`), 6 goldens, `CalcSession::build_output(MAIN)`,
//! the XML as raw bytes (what the docker probe saw).
//!
//! Compared EXACTLY: `output` (every key; numbers `%.17g`, strings, booleans; from OC3 on also the nested tables `output.MainHand` /
//! `output.OffHand`, key by key), `skillFlags`, the player's `conditions` and `multipliers`, and the counters (`playerModDB`,
//! `enemyModDB`, `skillModList` mod counts). One proven class is pinned by name, the same as CP3/CP4
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

use pob_calc::skill_funcs::checkpoint_hand_outputs;
use pob_calc::{ActorState, CalcMode, CalcSession, CalculateOptions, Checkpoint, Env, OutTable, OutValue, cache_skill_uuid};
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

/// One output value in the probe's encoding.
fn out_json(v: &OutValue) -> Json {
    match v {
        OutValue::Num(x) => n17(*x),
        OutValue::Str(t) => json!(probe_str(t.as_ref())),
        OutValue::Bool(b) => json!(b),
    }
}

/// A table value of `output` in the probe's encoding (`export-offence.ts` DUMP `D`): numbers `%.17g`, the item a class object
/// (`"<class Item>"`). `sourceGem` -- the gem instance TABLE, which the probe dumps member by member (6 levels deep, through `gemData`,
/// `grantedEffect`, ...) -- is written as Rust's reference to it, `"<gem g:i>"` (1-based), and pinned by [`GEM_REF`].
fn table_json(t: &OutTable) -> Json {
    match t {
        OutTable::ReqSource(r) => {
            let mut o = Map::new();
            for (k, v) in [("Str", r.str_), ("Dex", r.dex), ("Int", r.int)] {
                if let Some(v) = v {
                    o.insert(k.to_owned(), n17(v));
                }
            }
            o.insert("source".to_owned(), json!(r.source));
            if r.source_item.is_some() {
                o.insert("sourceItem".to_owned(), json!("<class Item>"));
            }
            if let Some(s) = &r.source_slot {
                o.insert("sourceSlot".to_owned(), json!(probe_str(s)));
            }
            if let Some((g, i)) = r.source_gem {
                o.insert("sourceGem".to_owned(), json!(format!("<gem {}:{}>", g + 1, i + 1)));
            }
            Json::Object(o)
        }
        // written by the buildOutput tail (Calcs.lua:395/:420/:430), after every offence checkpoint
        OutTable::SkillDps(_) | OutTable::Names(_) => json!("<table>"),
        // `output.Minion` -- the minion actor's output, dumped key by key (R7.10)
        OutTable::Minion(m) => Json::Object(m.0.borrow().output.iter().map(|(k, v)| (k.to_string(), out_json(v))).collect()),
    }
}

/// The probe's checkpoint snapshot, from Rust's state (inside the hook: `output.MainHand` / `output.OffHand` come from
/// [`checkpoint_hand_outputs`], set by offence around the OC3 hook).
fn snap(env: &Env, player: &ActorState) -> Json {
    let st = env.skills.as_ref().expect("skills on");
    let i = st.main_skill.expect("mainSkill");
    // R7.9: a mirage's copy is a main skill outside `activeSkillList` (`SkillsState::skill`)
    let sk = st.skill(i);
    let mut output: Map<String, Json> = player.output.iter().map(|(k, v)| (k.to_string(), out_json(v))).collect();
    for (k, t) in player.output.tables() {
        output.insert(k.to_string(), table_json(t));
    }
    if let Some((mh, oh)) = checkpoint_hand_outputs() {
        for (name, t) in [("MainHand", mh), ("OffHand", oh)] {
            let o: Map<String, Json> = t.iter().map(|(k, v)| (k.clone(), out_json(v))).collect();
            output.insert(name.to_owned(), Json::Object(o));
        }
    }
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
        "skillModList": mod_count(&env.skill_mod_list(i)).to_string(),
        "mainSkillName": st.skills.effects[sk.active_effect.borrow().granted_effect].name.clone(),
    })
}

/// The checkpoints Rust reaches (R7.7: all six, OC6 = the end of offence).
const CPS: [&str; 6] = ["OC1", "OC2", "OC3", "OC4", "OC5", "OC6"];

/// A checkpoint hit: the env had `limitedSkills` (a nested run), the mode, the snapshot.
struct Hit {
    nested: bool,
    mode: &'static str,
    snap: Json,
}

/// Proven class `gem-ref` (R7.5): `output.Req<Attr>Item.sourceGem` is the gem instance table (CalcSetup.lua:1847-1853); the probe dumps
/// it member by member, Rust's [`OutTable::ReqSource`] keeps the reference `(group, gem)`. Allowed: the ONLY differing member of such a
/// table is `sourceGem`, LuaJIT has a table there and Rust the reference (every other member -- `Str`/`Dex`/`Int`/`source` -- is
/// compared exactly; `perform_cp.rs` compares the reference itself against the `export-skills.ts` key `<group>:<gem>`).
const GEM_REF: [&str; 4] = ["ReqStrItem", "ReqDexItem", "ReqIntItem", "ReqOmniItem"];

/// The goldens whose top env takes an unported branch at CalcPerform.lua:3798, and the reason Rust records. R7.8 removed the two Cospris
/// entries (Cast On Critical Strike); R7.9 (`calcs.mirages` ported) removed Generals (General's Cry) and Mirage Archer: none is left.
const GATED: [(&str, &str); 0] = [];

/// Every difference between LuaJIT's checkpoint and Rust's, as text lines.
fn diff(lj: &Json, rust: &Json) -> Vec<String> {
    let mut out = Vec::new();
    for (k, v) in lj["output"].as_object().expect("output") {
        if GEM_REF.contains(&k.as_str()) && v.is_object() {
            TABLES.with(|t| {
                let mut t = t.borrow_mut();
                t.0 += 1;
                if rust["output"].get(k) != Some(v) && gem_ref_only(v, &rust["output"][k]) {
                    t.1 += 1;
                }
            });
        }
    }
    for part in ["output", "skillFlags", "playerConditions", "playerMultipliers"] {
        let (w, g) = (lj[part].as_object().expect(part), rust[part].as_object().expect(part));
        let keys: BTreeSet<&String> = w.keys().chain(g.keys()).collect();
        for k in keys {
            let (a, b) = (w.get(k).unwrap_or(&Json::Null), g.get(k).unwrap_or(&Json::Null));
            if a == b {
                continue;
            }
            if part == "output" && GEM_REF.contains(&k.as_str()) && gem_ref_only(a, b) {
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

thread_local! {
    /// `(Req<Attr>Item tables compared, of them equal but for the pinned sourceGem)` -- reported by the gates.
    static TABLES: RefCell<(usize, usize)> = const { RefCell::new((0, 0)) };
}

/// `a` (LuaJIT) and `b` (Rust) differ only in `sourceGem` (LuaJIT: a table, Rust: `"<gem g:i>"`).
fn gem_ref_only(a: &Json, b: &Json) -> bool {
    let (Some(a), Some(b)) = (a.as_object(), b.as_object()) else {
        return false;
    };
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    keys.into_iter().all(|k| {
        let (x, y) = (a.get(k), b.get(k));
        x == y || (k == "sourceGem" && x.is_some_and(Json::is_object) && y.and_then(Json::as_str).is_some_and(|s| s.starts_with("<gem ")))
    })
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
                Checkpoint::Oc3 => "OC3",
                Checkpoint::Oc4 => "OC4",
                Checkpoint::Oc5 => "OC5",
                Checkpoint::Oc6 => "OC6",
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
fn oc1_oc4_equal_luajit_on_six_goldens() {
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
        // R7.9: Dual Savior's first nested run (`calcs.buildActiveSkill` Double Strike) is built by The Saviour's `compareFunc` INSIDE the top
        // perform, which the session hook does not reach -- it is compared on its reproduction ([`repro_nested`], the first nested
        // `buildActiveSkill` of the session log), as `oc1_oc4_equal_luajit_on_all_offence_runs` does.
        let repro: Option<RustRun> = nested
            .then(|| {
                let want_mode = cps["OC1"].get("envMode").and_then(Json::as_str).unwrap_or("MAIN");
                let seen = hits.get("OC1").is_some_and(|v| v.iter().any(|h| h.nested && h.mode == want_mode));
                if seen {
                    return None;
                }
                let (_, _, bas) = rust_runs(g.name);
                let (mode, uuid, flags) = bas.into_iter().find(|(m, _, _)| m.as_str() == want_mode)?;
                println!("{}: first nested run {} {uuid} compared on its reproduction", g.name, mode.as_str());
                repro_nested(g.name, mode, &uuid, &flags)
                    .into_iter()
                    .find(|r| r.limited && r.uuid == uuid)
            })
            .flatten();
        for cp in CPS {
            let w = &cps[cp];
            let want_mode = w.get("envMode").and_then(Json::as_str).unwrap_or("MAIN");
            let repro_hit = repro.as_ref().and_then(|r| r.oc.get(cp)).map(|s| Hit {
                nested: true,
                mode: "MAIN",
                snap: s.clone(),
            });
            let hit = repro_hit.as_ref().or_else(|| {
                hits.get(cp)
                    .and_then(|v| v.iter().find(|h| h.nested == nested && h.mode == want_mode))
            });
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
    let (nt, ng) = TABLES.with(|t| *t.borrow());
    println!("checkpoints compared: {compared}; Req<Attr>Item table values compared {nt}, of them pinned `gem-ref` (sourceGem only) {ng}");
    assert!(all_bad.is_empty(), "OC1-OC6 != LuaJIT ({}):\n{}", all_bad.len(), all_bad.join("\n"));
}

// --- R7.1b: every player `calcs.offence` call of `buildOutput(MAIN)` -------------------------------------------------------------------
// LuaJIT truth: `<build>.offence-runs.json` (headers of every offence call, perform markers) + `-<n>.json` parts (OC snapshots by run `seq`),
// producer `tools/lua-corpus/export-offence.ts runs`. A LuaJIT run is matched to a Rust run by `{mode, uuid, limited, ordinal}` (ordinal =
// the n-th player offence call with that mode/uuid/limitedSkills-or-not, in call order); OC1-OC6 are compared (the Rust hooks).

/// A LuaJIT player offence call: header + OC1-OC6 snapshots.
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
        for k in CPS {
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

/// One Rust player offence call (`Checkpoint::OcEntry`), with the OC1-OC6 snapshots it reached.
struct RustRun {
    mode: &'static str,
    uuid: String,
    limited: bool,
    ordinal: u64,
    oc: BTreeMap<&'static str, Json>,
}

/// The build of a golden as a fresh session.
fn session_of(name: &str) -> CalcSession {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden");
    let model = pob_build::load_build_xml(&std::fs::read_to_string(g.xml_path()).expect("xml")).expect("xml parses");
    CalcSession::new(Rc::new(model), CalculateOptions::default())
}

/// A `calcs.buildActiveSkill` Rust made from INSIDE a perform (`depth >= 1`): `(mode, target uuid, limitedProcessingFlags)`.
type NestedBas = (CalcMode, String, Vec<String>);

fn rust_runs(name: &str) -> (Vec<RustRun>, Vec<String>, Vec<NestedBas>) {
    let sess = session_of(name);
    let mut notes = Vec::new();
    let runs = hooked_runs(&sess, |s| {
        let out = s.build_output(CalcMode::Main).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        notes = out.run.borrow().env.unimplemented.clone();
    });
    let nested = sess
        .log()
        .bas
        .iter()
        .filter(|b| b.depth >= 1)
        .map(|b| (b.mode, b.target_uuid.clone(), b.limited.clone()))
        .collect();
    (runs, notes, nested)
}

/// R7.8: the session hook reaches only the OUTERMOST perform (`CalcSession::perform_inner` takes it for the frame), so a run that
/// `calcs.triggers` builds inside the top perform (`findTriggerSkill` -> `calcs.buildActiveSkill`; Cospris: Cyclone) makes no hook
/// calls. Such a run is REPRODUCED for the comparison: a fresh session of the same build, `initEnv(MAIN)` (the parent env of the nested
/// call is the top MAIN env, `limitedSkills` nil), then the same `buildActiveSkill(env, mode, skill, uuid, flags)` with the hook on --
/// the same inputs as the nested call (a fresh initEnv + perform). Only for a LuaJIT run nested in a perform (`depth >= 2`) that the
/// Rust session log shows was built from inside a perform.
fn repro_nested(name: &str, mode: CalcMode, uuid: &str, limited: &[String]) -> Vec<RustRun> {
    let sess = session_of(name);
    hooked_runs(&sess, |s| {
        let env = s.init_env(CalcMode::Main).unwrap_or_else(|e| panic!("{name}: initEnv {e:?}"));
        let n = env.skills.as_ref().map_or(0, |st| st.active_skill_list.len());
        let i = (0..n)
            .find(|&i| cache_skill_uuid(&env, i) == uuid)
            .unwrap_or_else(|| panic!("{name}: no skill {uuid}"));
        s.build_active_skill(&env, mode, i, Some(uuid), limited)
            .unwrap_or_else(|e| panic!("{name}: buildActiveSkill {e:?}"));
    })
}

/// Runs `f` with a hook on `sess` that records every player offence call it sees.
fn hooked_runs(sess: &CalcSession, f: impl FnOnce(&CalcSession)) -> Vec<RustRun> {
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
                Checkpoint::Oc1 | Checkpoint::Oc2 | Checkpoint::Oc3 | Checkpoint::Oc4 | Checkpoint::Oc5 | Checkpoint::Oc6 => {
                    let k = match cp {
                        Checkpoint::Oc1 => "OC1",
                        Checkpoint::Oc2 => "OC2",
                        Checkpoint::Oc3 => "OC3",
                        Checkpoint::Oc4 => "OC4",
                        Checkpoint::Oc5 => "OC5",
                        _ => "OC6",
                    };
                    v.last_mut().expect("OcEntry before OC").oc.insert(k, snap(env, player));
                }
                _ => {}
            }
        },
    )));
    f(sess);
    sess.set_hook(None);
    Rc::try_unwrap(runs).ok().expect("hook dropped").into_inner()
}

/// Compares every matched run; returns (differences / unexplained gaps, totals
/// `[LuaJIT, Rust, matched, OC1 ok, OC2 ok, OC3 ok, OC4 ok, OC5 ok, OC6 ok, diff lines, not run, of the Rust runs reproduced]`).
fn compare_all() -> (Vec<String>, [usize; 12]) {
    let mut bad = Vec::new();
    let mut unexecuted = Vec::new();
    println!(
        "golden | LuaJIT player runs | Rust runs | matched | OC1 ok | OC2 ok | OC3 ok | OC4 ok | OC5 ok | OC6 ok | differences | Rust did not run"
    );
    let mut tot = [0usize; 12];
    for g in GOLDEN_BUILDS {
        let (lj, main) = lj_runs(g.name);
        let (mut rs, notes, nested_bas) = rust_runs(g.name);
        // R7.8: runs built inside a perform are invisible to the session hook -- reproduced ([`repro_nested`])
        let mut repro = 0;
        for l in &lj {
            let seen = rs
                .iter()
                .any(|r| r.mode == l.mode && r.uuid == l.uuid && r.limited == l.limited && r.ordinal == l.ordinal);
            let bas = nested_bas.iter().find(|(m, u, _)| m.as_str() == l.mode && *u == l.uuid);
            if seen || l.depth < 2 || !l.limited {
                continue;
            }
            let Some((mode, uuid, flags)) = bas else { continue };
            let again = repro_nested(g.name, *mode, uuid, flags);
            if let Some(mut r) = again
                .into_iter()
                .find(|r| r.mode == l.mode && r.uuid == l.uuid && r.limited && r.ordinal == 1)
            {
                println!(
                    "{}: LuaJIT run seq {} ({} {}, depth {}) built inside a perform by Rust (session log) -- compared on its reproduction",
                    g.name, l.seq, l.mode, l.uuid, l.depth
                );
                r.ordinal = l.ordinal;
                rs.push(r);
                repro += 1;
            }
        }
        let (mut matched, mut ndiff, mut nun) = (0, 0, 0);
        let mut oks = [0usize; 6];
        let mut used = BTreeSet::new();
        let gate = GATED.iter().find(|(n, _)| *n == g.name).map(|(_, why)| *why);
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
                if !gate_recorded {
                    bad.push(line.clone());
                }
                unexecuted.push(line);
                nun += 1;
                continue;
            };
            used.insert(i);
            matched += 1;
            for (ci, cp) in CPS.into_iter().enumerate() {
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
                    oks[ci] += 1;
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
            "{} | {} | {} | {matched} | {} | {} | {} | {} | {} | {} | {ndiff} | {nun} (LuaJIT performs without offence: {markers})",
            g.name,
            lj.len(),
            rs.len(),
            oks[0],
            oks[1],
            oks[2],
            oks[3],
            oks[4],
            oks[5]
        );
        for (t, x) in tot.iter_mut().zip([
            lj.len(),
            rs.len(),
            matched,
            oks[0],
            oks[1],
            oks[2],
            oks[3],
            oks[4],
            oks[5],
            ndiff,
            nun,
            repro,
        ]) {
            *t += x;
        }
    }
    println!(
        "TOTAL | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} (Rust runs reproduced: {})",
        tot[0], tot[1], tot[2], tot[3], tot[4], tot[5], tot[6], tot[7], tot[8], tot[9], tot[10], tot[11]
    );
    for u in &unexecuted {
        println!("unexecuted by Rust: {u}");
    }
    let (nt, ng) = TABLES.with(|t| *t.borrow());
    println!("Req<Attr>Item table values compared {nt}, of them pinned `gem-ref` (sourceGem only) {ng}");
    (bad, tot)
}

/// The strict gate: every LuaJIT player offence run is either compared OC1-OC6-equal or explained by a recorded gate. GREEN since R7.10
/// (minion actors): the nested runs of a minion skill (`Summon Ice Golem` x3, `Summon Lightning Golem`) now match `output.Minion` (the
/// minion actor's output, compared key by key) and `ActiveMinionLimit` / `SummonedMinionsPerCast` (CalcOffence.lua:1057-1065).
#[test]
fn oc1_oc4_equal_luajit_on_all_offence_runs() {
    let (bad, _) = compare_all();
    assert!(bad.is_empty(), "offence runs != LuaJIT ({}):\n{}", bad.len(), bad.join("\n"));
}

/// The R7.10 class (closed by R7.10): the keys a minion skill's offence writes from `activeSkill.minion` -- `output.Minion` (a table: the
/// minion actor's output, CalcPerform.lua:1319-1320 `initMinionModDB`) at OC1-OC6, `ActiveMinionLimit` / `SummonedMinionsPerCast`
/// (:1057-1065) from OC2 on.
const MINION_CLASS: [&str; 3] = ["output.Minion:", "output.ActiveMinionLimit:", "output.SummonedMinionsPerCast:"];

/// Pins the measured state so it cannot drift silently. R7.9 (`calcs.mirages`): 82 / 82 / 82, OC1-OC6 82/82 equal, 0 difference lines, 0
/// not run; 4 runs compared on their reproduction (Cyclone of both Cospris goldens, Dual Savior's Double Strike built by The Saviour's
/// `compareFunc` and Generals' Perforate built by General's Cry -- each a `buildActiveSkill` inside the top perform); the 9 runs the mirage
/// gate stopped before R7.9 now run, the mirages' own CALCULATOR performs hooked directly (they run with the caller's hook).
/// R7.10 (minion actors): 82 / 73 / 73, OC1-OC6 73/73 equal, 0 difference lines
/// (the 64 lines of the R7.10 class closed). R7.8 (`calcs.triggers` ported): 82 LuaJIT player offence runs, 73 Rust runs (all
/// matched; 2 of them -- Cyclone of both Cospris goldens, built by `findTriggerSkill` inside the top perform -- compared on their
/// reproduction, [`repro_nested`]), 9 not run (the recorded mirage gates), OC1-OC6 69/73 equal; the 64 difference lines are all the R7.10
/// class ([`MINION_CLASS`]) on the 4 nested minion runs (Generals' Summon Ice Golem joined: its CWDT dispatch is ported). New against
/// R7.7: Cospris x2 top Ice Spear + 5 tail runs each (12), Generals 3 and Mirage 2 tail runs whose dispatch is "cast when damage taken".
/// R7.7: 82 / 56 / 56 / 53 x6 / 48 lines / 26 not run, the 48 lines the same class on 3 runs
/// (OC1 1 + OC2 3 + OC3 3 + OC4 3 + OC5 3 + OC6 3 per run). R7.6 was the same with OC1-OC5
/// (39 lines), R7.5 with OC1-OC4 (30 lines), R7.4 with OC1-OC3 (21 lines);
/// R7.1b was OC1 53, OC2 51, 14 lines: `ExplodeChance` (2 runs of "On Kill Monster Explosion") closed by the port of
/// `skills.EnemyExplode.preDamageFunc` (other.lua:6204-6252).
#[test]
fn offence_runs_state_is_pinned() {
    let (bad, tot) = compare_all();
    assert_eq!(tot, [82, 82, 82, 82, 82, 82, 82, 82, 82, 0, 0, 4], "{}", bad.join("\n"));
    assert_eq!(bad.len(), 0, "unexplained gaps or differences:\n{}", bad.join("\n"));
    let other: Vec<&String> = bad.iter().filter(|l| !MINION_CLASS.iter().any(|c| l.contains(c))).collect();
    assert!(other.is_empty(), "differences outside the R7.10 minion class:\n{other:?}");
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
        "registry {} / closures {}: ported {p}, unported {u}, later (R7.5) {l}",
        reg.len(),
        list.len()
    );
    for n in &names {
        println!("  unported: {n}");
    }
    assert_eq!(list.len(), 134);
    assert_eq!(reg.len(), 134);
    // R7.10b: every closure is ported
    assert_eq!((p, u), (134, 0), "pinned: ported / unported skill functions");
    assert!(bad.is_empty(), "registry drift:\n{}", bad.join("\n"));
}

/// Times of one top-level offence call: the entry instant, entry -> OC1, -> OC2, -> OC3, -> OC4, -> OC5, -> OC6.
type Times = Rc<RefCell<(Option<std::time::Instant>, [Option<f64>; 6])>>;

/// `cargo test --release -p pob-calc --test offence_cp perf -- --ignored --nocapture`: wall time of offence parts A-E of the main skill --
/// offence entry (CalcPerform.lua:3799) -> OC1, -> OC2, -> OC3, -> OC4, -> OC5 and -> OC6 (the whole offence) -- on the first run that reaches OC6 (the top perform; a
/// gated golden: the first nested run), median of 50 after a warm-up, a fresh session per run. The Rust side of
/// `tools/.scratch/perf-offence.json`.
#[test]
#[ignore = "timing; not a check"]
fn perf_offence_parts_a_e() {
    for g in GOLDEN_BUILDS {
        let text = std::fs::read_to_string(g.xml_path()).expect("xml");
        let mut series: [Vec<f64>; 6] = Default::default();
        for run in 0..51 {
            let model = pob_build::load_build_xml(&text).expect("xml parses");
            let sess = CalcSession::new(Rc::new(model), CalculateOptions::default());
            let t: Times = Rc::default();
            let t2 = t.clone();
            sess.set_hook(Some(Box::new(move |cp: Checkpoint, _: &Env, _: &ActorState, _: &ActorState| {
                let mut s = t2.borrow_mut();
                if s.1[5].is_some() {
                    return;
                }
                let ms = s.0.map(|t0| t0.elapsed().as_secs_f64() * 1000.0);
                match cp {
                    Checkpoint::OcEntry => s.0 = Some(std::time::Instant::now()),
                    Checkpoint::Oc1 => s.1[0] = ms,
                    Checkpoint::Oc2 => s.1[1] = ms,
                    Checkpoint::Oc3 => s.1[2] = ms,
                    Checkpoint::Oc4 => s.1[3] = ms,
                    Checkpoint::Oc5 => s.1[4] = ms,
                    Checkpoint::Oc6 => s.1[5] = ms,
                    _ => {}
                }
            })));
            let _ = sess.build_output(CalcMode::Main).expect("buildOutput");
            sess.set_hook(None);
            let s = t.borrow();
            if run > 0 && s.1.iter().all(Option::is_some) {
                for (v, x) in series.iter_mut().zip(s.1) {
                    v.push(x.expect("checked"));
                }
            }
        }
        let med = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        let n = series[0].len();
        let [a, b, c, d, e, f] = &mut series;
        println!(
            "{}: offence entry -> OC1 {:.4} ms, -> OC2 {:.4} ms, -> OC3 {:.4} ms, -> OC4 {:.4} ms, -> OC5 {:.4} ms, -> OC6 {:.4} ms (median of {n})",
            g.name,
            med(a),
            med(b),
            med(c),
            med(d),
            med(e),
            med(f)
        );
    }
}

/// `cargo test --release -p pob-calc --test offence_cp perf_build_output -- --ignored --nocapture`: wall time of
/// `CalcSession::build_output(MAIN)` (a fresh session per run, the XML already loaded; nested performs included), median / min of 30
/// after a warm-up, and the number of performs the session ran -- the Rust side of `tools/.scratch/perf-offence.json` `buildOutput`.
#[test]
#[ignore = "timing; not a check"]
fn perf_build_output() {
    for g in GOLDEN_BUILDS {
        let text = std::fs::read_to_string(g.xml_path()).expect("xml");
        let mut v = Vec::new();
        let mut performs = 0;
        for run in 0..31 {
            let model = Rc::new(pob_build::load_build_xml(&text).expect("xml parses"));
            let sess = CalcSession::new(model, CalculateOptions::default());
            let t0 = std::time::Instant::now();
            let _ = sess.build_output(CalcMode::Main).expect("buildOutput");
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            performs = sess.log().perform.len();
            if run > 0 {
                v.push(ms);
            }
        }
        v.sort_by(f64::total_cmp);
        println!(
            "{}: buildOutput median {:.3} ms, min {:.3} ms (n {}), performs {performs}",
            g.name,
            v[v.len() / 2],
            v[0],
            v.len()
        );
    }
}
