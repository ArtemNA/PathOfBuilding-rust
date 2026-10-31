//! R7.2 gate (R7-ARCH §3, card `tasks/R7.2-calc-session.md` p. 4): `CalcSession` against the R7.1 LuaJIT fixtures
//! (`tests/fixtures/<build>.offence.json`, producer `tools/lua-corpus/export-offence.ts`), 6 goldens, default options.
//!
//!   (a) `cacheSkillUUID` of every active skill = LuaJIT (`mainInfo.activeSkills[].uuid`, order and count), and the top perform's main
//!       skill uuid = LuaJIT's (`logs.performLog`, the `top` entry);
//!   (b) the nested runs: the `buildActiveSkill` calls of the MAIN loop (Calcs.lua:400-404) and their performs, the cache writes and the
//!       initEnv count follow the same laws as LuaJIT's logs. The only nested runs LuaJIT makes that Rust does not are the ones
//!       `calcs.triggers` / `calcs.mirages` make (R7.8/R7.9, skipped at CalcPerform.lua:3797-3801) -- each one is identified in
//!       LuaJIT's log, and the MAIN-loop difference they cause is checked to be exactly that, nothing else;
//!   (c) `calcFullDPS` = LuaJIT's (empty on the goldens), plus its frame on synthetic `includeInFullDPS` groups;
//!   (d) perf of the repeated runs (`--nocapture`).

use std::collections::BTreeSet;
use std::rc::Rc;

use pob_calc::session::{BasEntry, FullDpsOutcome, SessionError, get_cached_output_value};
use pob_calc::{CalcMode, CalcSession, CalculateOptions, cache_skill_uuid};
use pob_golden::registry::GOLDEN_BUILDS;
use serde_json::Value as Json;

fn fixture(name: &str) -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{}.offence.json", name.replace(' ', "-")));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("fixture json")
}

fn model(name: &str) -> pob_build::BuildModel {
    let g = GOLDEN_BUILDS.iter().find(|g| g.name == name).expect("golden");
    let xml = pob_build::read_build_file(g.xml_path()).expect("xml");
    pob_build::load_build_xml(&xml).expect("xml parses")
}

fn session(name: &str) -> CalcSession {
    CalcSession::new(Rc::new(model(name)), CalculateOptions::default())
}

/// A Lua array dumped as `{"1": .., "2": ..}`, in index order.
fn arr(v: &Json) -> Vec<Json> {
    let o = v.as_object().expect("dumped array");
    (1..=o.len()).map(|i| o[&i.to_string()].clone()).collect()
}

fn s(v: &Json) -> String {
    v.as_str().expect("string").to_owned()
}

fn num(v: &Json) -> usize {
    s(v).parse().expect("integer")
}

#[test]
fn cache_skill_uuid_equals_luajit() {
    let mut report = Vec::new();
    for g in GOLDEN_BUILDS {
        let fx = fixture(g.name);
        let sess = session(g.name);
        let env = sess.init_env(CalcMode::Main).unwrap_or_else(|_| panic!("{}: initEnv", g.name));
        let n = env.skills.as_ref().unwrap().active_skill_list.len();
        let ours: Vec<String> = (0..n).map(|i| cache_skill_uuid(&env, i)).collect();
        let theirs: Vec<String> = fx["mainInfo"]["activeSkills"]
            .as_array()
            .expect("activeSkills")
            .iter()
            .map(|e| s(&e["uuid"]))
            .collect();
        assert_eq!(ours, theirs, "{}: cacheSkillUUID of activeSkillList", g.name);
        // includeInFullDPS of every skill's group
        let st = env.skills.as_ref().unwrap();
        for (i, e) in fx["mainInfo"]["activeSkills"].as_array().unwrap().iter().enumerate() {
            let inc = st.active_skill_list[i]
                .socket_group
                .is_some_and(|gi| st.tab.socket_group_list()[gi].include_in_full_dps == Some(true));
            assert_eq!(
                Json::Bool(inc),
                e["includeInFullDPS"],
                "{}: includeInFullDPS of {}",
                g.name,
                ours[i]
            );
        }
        // the main skill at the top perform's entry
        let top = arr(&fx["logs"]["performLog"])
            .into_iter()
            .find(|p| p["top"] == Json::Bool(true))
            .expect("top perform");
        let ms = st.main_skill.expect("main skill");
        assert_eq!(ours[ms], s(&top["uuid"]), "{}: top mainSkill uuid", g.name);
        report.push(format!("{}: uuids {} = LuaJIT, top {}", g.name, ours.len(), ours[ms]));
    }
    println!("{}", report.join("\n"));
}

#[test]
fn nested_runs_follow_luajit() {
    let mut report = Vec::new();
    for g in GOLDEN_BUILDS {
        let fx = fixture(g.name);
        let logs = &fx["logs"];
        let sess = session(g.name);
        let out = sess.build_output(CalcMode::Main).unwrap_or_else(|e| panic!("{}: {e}", g.name));
        let log = sess.log().clone();
        let skills: Vec<String> = fx["mainInfo"]["activeSkills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| s(&e["uuid"]))
            .collect();

        // --- LuaJIT's logs
        let lj_bas = arr(&logs["buildActiveSkillLog"]);
        let lj_perf = arr(&logs["performLog"]);
        let lj_cw = arr(&logs["cacheWriteLog"]);
        let lj_top_uuid = s(&lj_perf.iter().find(|p| p["top"] == Json::Bool(true)).unwrap()["uuid"]);
        let lj_loop: Vec<String> = lj_bas
            .iter()
            .filter(|b| num(&b["depth"]) == 0)
            .map(|b| s(&b["targetUUID"]))
            .collect();
        let lj_nested_bas: BTreeSet<String> = lj_bas
            .iter()
            .filter(|b| num(&b["depth"]) > 0)
            .map(|b| s(&b["targetUUID"]))
            .collect();
        // MAIN entries written before the loop: every cacheData up to and including the top one
        let top_cw = lj_cw.iter().position(|c| c["top"] == Json::Bool(true)).expect("top cacheData");
        let lj_pre: BTreeSet<String> = lj_cw[..=top_cw]
            .iter()
            .filter(|c| c["mode"] == "MAIN")
            .map(|c| s(&c["uuid"]))
            .collect();
        let lj_top_cw_uuid = s(&lj_cw[top_cw]["uuid"]);
        // the loop law on LuaJIT's own log: every skill without a MAIN entry, in activeSkillList order
        let lj_expected: Vec<String> = skills.iter().filter(|u| !lj_pre.contains(*u)).cloned().collect();
        assert_eq!(
            lj_loop, lj_expected,
            "{}: LuaJIT's MAIN loop = activeSkillList minus the pre-loop MAIN entries",
            g.name
        );
        // initEnv law: top + calcFullDPS + one per buildActiveSkill + one per CALCULATOR perform no buildActiveSkill made (mirages)
        let lj_calc_perf_without_bas =
            lj_perf.iter().filter(|p| p["mode"] == "CALCULATOR").count() - lj_bas.iter().filter(|b| b["mode"] == "CALCULATOR").count();
        assert_eq!(
            num(&logs["initEnvCalls"]),
            2 + lj_bas.len() + lj_calc_perf_without_bas,
            "{}: LuaJIT initEnv law",
            g.name
        );

        // --- Rust
        let rust_top = log.perform.iter().find(|p| p.top).expect("top perform");
        assert_eq!(rust_top.uuid, lj_top_uuid, "{}: top perform main skill", g.name);
        assert!(!rust_top.skip_ehp && rust_top.depth == 0 && rust_top.mode == CalcMode::Main);
        let rust_loop: Vec<String> = log.bas.iter().map(|b| b.target_uuid.clone()).collect();
        let rust_expected: Vec<String> = skills.iter().filter(|u| **u != rust_top.uuid).cloned().collect();
        assert_eq!(rust_loop, rust_expected, "{}: Rust MAIN loop", g.name);
        for b in &log.bas {
            assert_eq!(
                b,
                &BasEntry {
                    depth: 0,
                    mode: CalcMode::Main,
                    env_mode: CalcMode::Main,
                    skill_uuid: b.target_uuid.clone(),
                    target_uuid: b.target_uuid.clone(),
                    limited: vec![],
                },
                "{}: buildActiveSkill entry",
                g.name
            );
        }
        // every buildActiveSkill ran one perform(fullEnv, true) of that skill, in order
        let nested: Vec<_> = log.perform.iter().filter(|p| !p.top).collect();
        assert_eq!(nested.len(), log.bas.len(), "{}: one perform per buildActiveSkill", g.name);
        for (p, b) in nested.iter().zip(&log.bas) {
            assert!(p.skip_ehp && p.depth == 0 && p.mode == CalcMode::Main && p.uuid == b.target_uuid);
        }
        // cache writes: the top, then one per nested run, each under its own uuid
        assert_eq!(log.cache_writes.len(), log.perform.len());
        assert!(log.cache_writes[0].top && log.cache_writes[0].uuid == rust_top.uuid);
        let main_keys: BTreeSet<String> = sess.cache().uuids(CalcMode::Main).into_iter().collect();
        assert_eq!(
            main_keys,
            skills.iter().cloned().collect::<BTreeSet<_>>(),
            "{}: MAIN cache = every skill",
            g.name
        );
        assert!(sess.cache().uuids(CalcMode::Calculator).is_empty() && sess.cache().uuids(CalcMode::Calcs).is_empty());
        assert_eq!(log.init_env_calls, 2 + log.bas.len(), "{}: Rust initEnv law", g.name);
        assert!(log.con_print.is_empty(), "{}: {:?}", g.name, log.con_print);

        // --- the difference is exactly the triggers' / mirages' runs
        // LuaJIT pre-loop entries Rust does not have: made by a nested buildActiveSkill (CalcTriggers / CalcMirages)
        let lj_only: BTreeSet<String> = lj_pre.iter().filter(|u| **u != rust_top.uuid).cloned().collect();
        assert!(
            lj_only.is_subset(&lj_nested_bas),
            "{}: {lj_only:?} not from nested buildActiveSkill {lj_nested_bas:?}",
            g.name
        );
        // Rust's top entry LuaJIT does not have: the mirage replaced the top mainSkill before cacheData (CalcMirages Saviour)
        if !lj_pre.contains(&rust_top.uuid) {
            assert_ne!(
                lj_top_cw_uuid, lj_top_uuid,
                "{}: top uuid missing from LuaJIT's cache without a mainSkill swap",
                g.name
            );
        }
        let mut sym: Vec<String> = rust_loop.iter().filter(|u| !lj_loop.contains(u)).map(|u| format!("+{u}")).collect();
        sym.extend(lj_loop.iter().filter(|u| !rust_loop.contains(u)).map(|u| format!("-{u}")));

        // (c) calcFullDPS: LuaJIT's is empty on every golden
        assert_eq!(fx["fullDPS"]["combinedDPS"], "0");
        assert_eq!(fx["fullDPS"]["skills"], Json::Object(Default::default()));
        match &out.full_dps {
            FullDpsOutcome::Done(fd) => {
                assert_eq!(fd.combined_dps, 0.0);
                assert_eq!(fd.total_dot_dps, 0.0);
                assert_eq!(fd.skills, 0);
            }
            other => panic!("{}: calcFullDPS {other:?}", g.name),
        }

        let lj_perf_n = lj_perf.len();
        report.push(format!(
            "{}: buildActiveSkill rust {} / luajit {} (loop {} + nested {}), perform rust {} / luajit {}, initEnv rust {} / luajit {}, \
             MAIN-loop diff {:?} (explained: triggers/mirages {:?})",
            g.name,
            log.bas.len(),
            lj_bas.len(),
            lj_loop.len(),
            lj_bas.len() - lj_loop.len(),
            log.perform.len(),
            lj_perf_n,
            log.init_env_calls,
            num(&logs["initEnvCalls"]),
            sym,
            lj_nested_bas
        ));
    }
    println!("{}", report.join("\n"));
}

/// The MAIN loop is idempotent once every skill is cached: a second `buildOutput` builds nothing (the cache answers).
#[test]
fn second_build_output_reads_the_cache() {
    let sess = session("OccVortex");
    sess.build_output(CalcMode::Main).unwrap();
    let first = sess.log().bas.len();
    sess.build_output(CalcMode::Main).unwrap();
    assert_eq!(sess.log().bas.len(), first);
    sess.wipe_global_cache();
    assert!(sess.cache().uuids(CalcMode::Main).is_empty());
    sess.build_output(CalcMode::Main).unwrap();
    assert_eq!(sess.log().bas.len(), 2 * first);
}

/// `getCachedOutputValue` (CalcPerform.lua:31-43): builds the skill with `limitedProcessingFlags = {uuid}`, MAIN reads the cache on
/// the next call, CALCULATOR always rebuilds; the value itself is offence output -> the declared boundary, never a nil read as a value.
#[test]
fn get_cached_output_value_builds_through_the_cache() {
    let sess = session("OccVortex");
    let env = sess.init_env(CalcMode::Main).unwrap();
    let uuid = cache_skill_uuid(&env, 0);
    let r = get_cached_output_value(&env, 0, &["Speed", "Duration"]);
    assert!(r.unwrap_err().contains("calcs.offence"));
    {
        let log = sess.log();
        assert_eq!(log.bas.len(), 1);
        assert_eq!(log.bas[0].limited, vec![uuid.clone()]);
        assert_eq!(log.bas[0].target_uuid, uuid);
    }
    let cached = sess.cache();
    let entry = cached.get(CalcMode::Main, &uuid).expect("cached");
    let run = entry.run.borrow();
    assert_eq!(run.env.limited_skills, Some(BTreeSet::from([uuid.clone()])));
    assert!(!run.env.build_breakdown);
    assert!(pob_calc::session::is_limited(&run.env, entry.active_skill));
    drop(run);
    drop(cached);
    // MAIN: the second call is a cache hit
    let _ = get_cached_output_value(&env, 0, &["Speed"]);
    assert_eq!(sess.log().bas.len(), 1);
    // CALCULATOR: always rebuilt
    let calc = sess.init_env(CalcMode::Calculator).unwrap();
    let _ = get_cached_output_value(&calc, 0, &["Speed"]);
    let _ = get_cached_output_value(&calc, 0, &["Speed"]);
    assert_eq!(sess.log().bas.len(), 3);
    assert_eq!(sess.cache().uuids(CalcMode::Calculator), vec![uuid]);
}

/// initEnv modes: CALCULATOR needs a MAIN initEnv before it (the shared skills tab), CALCS is not ported.
#[test]
fn init_env_modes() {
    let sess = session("OccVortex");
    assert!(matches!(sess.init_env(CalcMode::Calculator), Err(SessionError::Unimplemented(_))));
    assert!(matches!(sess.init_env(CalcMode::Calcs), Err(SessionError::Unimplemented(_))));
    let main = sess.init_env(CalcMode::Main).unwrap();
    assert!(main.build_breakdown && main.mode == "MAIN");
    let calc = sess.init_env(CalcMode::Calculator).unwrap();
    assert!(!calc.build_breakdown && calc.mode == "CALCULATOR");
    // the same skills, the same uuids
    let n = main.skills.as_ref().unwrap().active_skill_list.len();
    assert_eq!(n, calc.skills.as_ref().unwrap().active_skill_list.len());
    for i in 0..n {
        assert_eq!(cache_skill_uuid(&main, i), cache_skill_uuid(&calc, i));
    }
}

/// The calcFullDPS frame (Calcs.lua:168-345) on synthetic `includeInFullDPS` groups (the goldens have none): one
/// `perform(fullEnv, true)` per enabled skill of an included group, in activeSkillList order, CALCULATOR, one re-init after each;
/// the roll-up stops at the offence boundary.
#[test]
fn calc_full_dps_frame_on_included_groups() {
    let mut m = model("OccVortex");
    let set = m.skills.sets.first_mut().expect("skill set");
    for gi in [0, 4] {
        set.groups[gi].include_in_full_dps = true;
        set.groups[gi].include_in_full_dps_attr = true;
    }
    let sess = CalcSession::new(Rc::new(m), CalculateOptions::default());
    let main = sess.init_env(CalcMode::Main).unwrap();
    let st = main.skills.as_ref().unwrap();
    let expected: Vec<String> = (0..st.active_skill_list.len())
        .filter(|&i| {
            st.active_skill_list[i]
                .socket_group
                .is_some_and(|g| st.tab.socket_group_list()[g].include_in_full_dps == Some(true))
        })
        .map(|i| cache_skill_uuid(&main, i))
        .collect();
    assert!(!expected.is_empty());
    let inits = sess.log().init_env_calls;
    let out = sess.calc_full_dps(CalcMode::Calculator).unwrap();
    match out {
        FullDpsOutcome::Unimplemented { processed, note } => {
            assert_eq!(processed, expected.len());
            assert!(note.starts_with("Calcs.lua:176-336"), "{note}");
        }
        FullDpsOutcome::Done(_) => panic!("roll-up claimed without offence"),
    }
    let log = sess.log();
    let runs: Vec<String> = log.perform.iter().map(|p| p.uuid.clone()).collect();
    assert_eq!(runs, expected);
    assert!(log.perform.iter().all(|p| p.mode == CalcMode::Calculator && p.skip_ehp && !p.top));
    assert_eq!(log.init_env_calls - inits, 1 + expected.len());
    println!("calcFullDPS frame: {} skills performed: {expected:?}", expected.len());
}

/// DoD perf (R7-ARCH §2 (г)): the cost of the repeated runs, median of 7 `buildOutput(MAIN)` per golden (fresh session each).
#[test]
fn perf_repeated_runs() {
    let median = |mut v: Vec<f64>| -> f64 {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let mut rows = Vec::new();
    for g in GOLDEN_BUILDS {
        let m = Rc::new(model(g.name));
        let (mut total, mut top_init, mut top_perf, mut nested_each, mut init_each) = (vec![], vec![], vec![], vec![], vec![]);
        let mut nested_n = 0;
        for i in 0..8 {
            let sess = CalcSession::new(m.clone(), CalculateOptions::default());
            let t = std::time::Instant::now();
            sess.build_output(CalcMode::Main).unwrap();
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            if i == 0 {
                continue; // warm-up
            }
            let log = sess.log();
            total.push(ms);
            top_init.push(log.init_env_ms[0]);
            top_perf.push(log.perform.iter().find(|p| p.top).unwrap().ms);
            nested_each.extend(log.bas_ms.iter().copied());
            init_each.extend(log.init_env_ms.iter().copied());
            nested_n = log.bas.len();
        }
        rows.push(format!(
            "{}: buildOutput {:.2} ms; top initEnv {:.2} + perform {:.2}; {} nested runs, each initEnv+perform {:.2} ms (initEnv alone {:.2})",
            g.name,
            median(total),
            median(top_init),
            median(top_perf),
            nested_n,
            median(nested_each),
            median(init_each)
        ));
    }
    println!("{}", rows.join("\n"));
}
