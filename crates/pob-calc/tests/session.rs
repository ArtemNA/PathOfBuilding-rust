//! R7.2 gate (R7-ARCH §3, card `tasks/R7.2-calc-session.md` p. 4): `CalcSession` against the R7.1 LuaJIT fixtures
//! (`tests/fixtures/<build>.offence.json`, producer `tools/lua-corpus/export-offence.ts`), 6 goldens, default options.
//!
//!   (a) `cacheSkillUUID` of every active skill = LuaJIT (`mainInfo.activeSkills[].uuid`, order and count), and the top perform's main
//!       skill uuid = LuaJIT's (`logs.performLog`, the `top` entry);
//!   (b) the nested runs: LuaJIT's own logs obey the MAIN-loop law (Calcs.lua:400-404) and the initEnv law; since R7.9 (`calcs.mirages`,
//!       after R7.8's `calcs.triggers`) Rust's `buildActiveSkill` log, perform log, `cacheData` log, initEnv count and cache keys are
//!       LuaJIT's, entry by entry;
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

        // --- Rust. R7.9: every nested run LuaJIT makes Rust makes too (`calcs.triggers` since R7.8, `calcs.mirages` since R7.9) -- the
        // three logs are equal to LuaJIT's entry by entry, and so is the initEnv count.
        let rust_top = log.perform.iter().find(|p| p.top).expect("top perform");
        assert_eq!(rust_top.uuid, lj_top_uuid, "{}: top perform main skill", g.name);
        assert!(!rust_top.skip_ehp && rust_top.depth == 0 && rust_top.mode == CalcMode::Main);
        let mode_of = |v: &Json| match v.as_str().expect("mode") {
            "MAIN" => CalcMode::Main,
            "CALCS" => CalcMode::Calcs,
            "CALCULATOR" => CalcMode::Calculator,
            other => panic!("mode {other}"),
        };
        let lj_bas_rows: Vec<BasEntry> = lj_bas
            .iter()
            .map(|b| BasEntry {
                depth: num(&b["depth"]),
                mode: mode_of(&b["mode"]),
                env_mode: mode_of(&b["envMode"]),
                skill_uuid: s(&b["skillUUID"]),
                target_uuid: s(&b["targetUUID"]),
                limited: arr(&b["limited"]).iter().map(s).collect(),
            })
            .collect();
        assert_eq!(log.bas, lj_bas_rows, "{}: buildActiveSkill log", g.name);
        let rust_perf: Vec<(usize, CalcMode, bool, bool, String)> = log
            .perform
            .iter()
            .map(|p| (p.depth, p.mode, p.skip_ehp, p.top, p.uuid.clone()))
            .collect();
        let lj_perf_rows: Vec<(usize, CalcMode, bool, bool, String)> = lj_perf
            .iter()
            .map(|p| {
                (
                    num(&p["depth"]),
                    mode_of(&p["mode"]),
                    p["skipEHP"] == Json::Bool(true),
                    p["top"] == Json::Bool(true),
                    s(&p["uuid"]),
                )
            })
            .collect();
        assert_eq!(rust_perf, lj_perf_rows, "{}: perform log", g.name);
        let rust_cw: Vec<(CalcMode, bool, String)> = log.cache_writes.iter().map(|c| (c.mode, c.top, c.uuid.clone())).collect();
        let lj_cw_rows: Vec<(CalcMode, bool, String)> = lj_cw
            .iter()
            .map(|c| (mode_of(&c["mode"]), c["top"] == Json::Bool(true), s(&c["uuid"])))
            .collect();
        assert_eq!(rust_cw, lj_cw_rows, "{}: cacheData log", g.name);
        assert_eq!(log.init_env_calls, num(&logs["initEnvCalls"]), "{}: initEnv count", g.name);
        // the cache keys = the uuids LuaJIT wrote, per mode
        for mode in [CalcMode::Main, CalcMode::Calculator] {
            let keys: BTreeSet<String> = sess.cache().uuids(mode).into_iter().collect();
            let lj_keys: BTreeSet<String> = lj_cw_rows.iter().filter(|c| c.0 == mode).map(|c| c.2.clone()).collect();
            assert_eq!(keys, lj_keys, "{}: {} cache keys", g.name, mode.as_str());
        }
        assert!(sess.cache().uuids(CalcMode::Calcs).is_empty());
        assert!(log.con_print.is_empty(), "{}: {:?}", g.name, log.con_print);
        let rust_loop: Vec<String> = log.bas.iter().filter(|b| b.depth == 0).map(|b| b.target_uuid.clone()).collect();
        let sym: Vec<String> = rust_loop.iter().filter(|u| !lj_loop.contains(u)).cloned().collect();

        // (c) calcFullDPS: LuaJIT's is empty on every golden
        assert_eq!(fx["fullDPS"]["combinedDPS"], "0");
        assert_eq!(fx["fullDPS"]["skills"], Json::Object(Default::default()));
        match &out.full_dps {
            FullDpsOutcome::Done(fd) => {
                assert_eq!(fd.combined_dps, 0.0);
                assert_eq!(fd.total_dot_dps, 0.0);
                assert!(fd.skills.is_empty());
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
/// the next call, CALCULATOR always rebuilds; the values are the cached run's offence output (R7.7).
#[test]
fn get_cached_output_value_builds_through_the_cache() {
    let sess = session("OccVortex");
    let env = sess.init_env(CalcMode::Main).unwrap();
    let uuid = cache_skill_uuid(&env, 0);
    let r = get_cached_output_value(&env, 0, &["Speed", "Duration"]).expect("the cached run reached calcs.offence");
    {
        let log = sess.log();
        assert_eq!(log.bas.len(), 1);
        assert_eq!(log.bas[0].limited, vec![uuid.clone()]);
        assert_eq!(log.bas[0].target_uuid, uuid);
    }
    let cached = sess.cache();
    let entry = cached.get(CalcMode::Main, &uuid).expect("cached");
    let run = entry.run.borrow();
    // R7.7: the values are the cached run's offence output
    assert!(run.offence_ran);
    let o = &run.state.player.output;
    assert_eq!(r, vec![o.opt_num("Speed"), o.opt_num("Duration")]);
    assert!(r[0].is_some());
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
/// R7.7: the roll-up runs to the end (both skills reach `calcs.offence`); its numbers are gated against LuaJIT by
/// `offence_synth.rs::fulldps_matches_luajit`.
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
    match &out {
        FullDpsOutcome::Done(fd) => {
            assert!(fd.combined_dps > 0.0, "{fd:?}");
            println!("calcFullDPS: combinedDPS {}, {} entries", fd.combined_dps, fd.skills.len());
        }
        FullDpsOutcome::Unimplemented { processed, note } => panic!("roll-up stopped after {processed}: {note}"),
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

/// `format!("{:?}")` of an env's modDB buckets (names, mods, tags -- `tag.div` cells included).
fn mod_db_dump(env: &pob_calc::Env) -> String {
    match &env.mod_db.borrow().kind {
        pob_mods::StoreKind::Db(db) => format!("{:?}", db.mods),
        pob_mods::StoreKind::List(_) => unreachable!("modDB is a ModDB"),
    }
}

/// R7-perf: the build state a session shares between its initEnvs (`BuildShared`: `build.spec` after `PostLoad`, the config lists,
/// the loaded `build.skillsTab`) gives the env a standalone initEnv gives, and a second MAIN initEnv + perform of the session = the
/// first (output, modDB, unimplemented), on 6 goldens.
#[test]
fn shared_build_state_second_run_equals_first() {
    for g in GOLDEN_BUILDS {
        let m = Rc::new(model(g.name));
        let sess = CalcSession::new(m.clone(), CalculateOptions::default());
        let env1 = sess.init_env(CalcMode::Main).unwrap();
        // the same env as a standalone initEnv (own spec, own config lists, own SkillsTab:Load)
        let alone = pob_calc::init_env_with(
            m.clone(),
            pob_calc::InitEnvOptions {
                parse_mode: pob_calc::ParseMode::Full,
                ..pob_calc::InitEnvOptions::default()
            },
            true,
        )
        .unwrap();
        assert_eq!(
            mod_db_dump(&env1),
            mod_db_dump(&alone),
            "{}: modDB, session vs standalone initEnv",
            g.name
        );
        assert_eq!(env1.unimplemented, alone.unimplemented, "{}", g.name);
        // (Debug: `EffectRef` compares by identity, its Debug prints the effect)
        assert_eq!(
            format!("{:?}", env1.skills.as_ref().unwrap().tab),
            format!("{:?}", alone.skills.as_ref().unwrap().tab),
            "{}: skills tab after the merge",
            g.name
        );
        let db1 = mod_db_dump(&env1);
        let r1 = sess.perform(env1, false);
        let env2 = sess.init_env(CalcMode::Main).unwrap();
        assert!(Rc::ptr_eq(&r1.borrow().env.spec, &env2.spec), "{}: one spec per session", g.name);
        assert_eq!(mod_db_dump(&env2), db1, "{}: modDB, second initEnv vs first", g.name);
        let r2 = sess.perform(env2, false);
        let (a, b) = (r1.borrow(), r2.borrow());
        assert_eq!(
            format!("{:?}", pob_calc::output_to_stats(&a.state.player.output)),
            format!("{:?}", pob_calc::output_to_stats(&b.state.player.output)),
            "{}: player output, second run vs first",
            g.name
        );
        assert_eq!(a.env.unimplemented, b.env.unimplemented, "{}", g.name);
        // the shared tab is the load's, untouched by the merges of both runs
        let shared = sess.shared().unwrap();
        let fresh = pob_calc::skills::SkillsState::load_tab(&m, &b.env.ordered_slots, &b.env.items).unwrap();
        assert_eq!(
            format!("{:?}", shared.skills_tab.as_ref().unwrap()),
            format!("{fresh:?}"),
            "{}: shared skills tab",
            g.name
        );
    }
}
