//! R7.2b gate: `specEnv` / `cachedPlayerDB` / `accelerate` of `calcs.initEnv` (CalcSetup.lua:475-482, :590-693, :306-374, :1945-1968) as
//! `calcs.buildOutput` hands them to `calcs.calcFullDPS` (Calcs.lua:386-392, :143, :337-344).
//!
//! fd-gen-melee (R7-synth) is the scenario where the order own/parent decides: MAIN's own modDB holds `OVERRIDE 50@Config` before
//! perform's `OVERRIDE 999999@infinite power` (Redblade Banner, CalcPerform.lua:1344-1347), so MAIN's `WarcryPower` is 50; a full-DPS
//! env reads the config mods through its parent (`cachedPlayerDB`) and perform's own override wins -- the bleed / poison numbers
//! below follow.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::{CalcMode, CalcSession, CalculateOptions, FullDpsOutcome, OutTable, ParseMode};
use pob_lua::format_g;
use serde_json::Value as Json;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/offence-synth")
}

fn session(name: &str) -> (CalcSession, Json) {
    let fx: Json =
        serde_json::from_str(&std::fs::read_to_string(dir().join(format!("{name}.luajit.json"))).expect("fixture")).expect("fixture json");
    let xml = pob_build::read_build_file(dir().join(format!("{name}.xml"))).expect("xml");
    let model = pob_build::load_build_xml(&xml).expect("xml parses");
    let opts = CalculateOptions {
        parse_mode: ParseMode::Full,
        skills: true,
        ..CalculateOptions::default()
    };
    (CalcSession::new(Rc::new(model), opts), fx)
}

/// `%.17g`, as the oracle wire carries numbers.
fn g(x: f64) -> String {
    format_g(x, 17)
}

fn lj_str<'a>(fx: &'a Json, path: &[&str]) -> &'a str {
    let mut v = fx;
    for p in path {
        v = &v[*p];
    }
    v.as_str().unwrap_or_else(|| panic!("fixture has no {}", path.join(".")))
}

/// The 13 keys R7.7 pinned (`FULLDPS_KNOWN`, offence_synth.rs) -- `fullDPS` / `FullDPS` / `bleedDPS` / `poisonDPS` and what they feed --
/// equal LuaJIT bit for bit, and so does every other number and string of `fullDPS` and of `output.SkillDPS`.
#[test]
fn fd_gen_melee_full_dps_equals_luajit() {
    let (sess, fx) = session("fd-gen-melee");
    let out = sess.build_output(CalcMode::Main).expect("buildOutput");
    let FullDpsOutcome::Done(fd) = &out.full_dps else {
        panic!("fd-gen-melee: calcFullDPS stopped: {:?}", out.full_dps)
    };
    let r = out.run.borrow();
    let o = &r.state.player.output;
    let mut bad = Vec::new();
    let mut eq = |what: String, lj: &str, rust: String| {
        if lj != rust {
            bad.push(format!("{what}: lj {lj} rust {rust}"));
        }
    };
    for (k, v) in [
        ("TotalDotDPS", fd.total_dot_dps),
        ("TotalPoisonDPS", fd.total_poison_dps),
        ("bleedDPS", fd.bleed_dps),
        ("burningGroundDPS", fd.burning_ground_dps),
        ("causticGroundDPS", fd.caustic_ground_dps),
        ("combinedDPS", fd.combined_dps),
        ("corruptingBloodDPS", fd.corrupting_blood_dps),
        ("cullingMulti", fd.culling_multi),
        ("decayDPS", fd.decay_dps),
        ("dotDPS", fd.dot_dps),
        ("igniteDPS", fd.ignite_dps),
        ("impaleDPS", fd.impale_dps),
    ] {
        eq(format!("fullDPS.{k}"), lj_str(&fx, &["fullDPS", k]), g(v));
    }
    eq(
        "fullDPS.cullingDPS".into(),
        lj_str(&fx, &["fullDPS", "cullingDPS"]),
        fd.culling_dps.map_or_else(|| "nil".into(), g),
    );
    let lj_skills = fx["fullDPS"]["skills"].as_object().expect("skills");
    assert_eq!(lj_skills.len(), fd.skills.len(), "fullDPS.skills count");
    let Some(OutTable::SkillDps(sd)) = o.table("SkillDPS") else {
        panic!("output.SkillDPS missing")
    };
    assert_eq!(sd, &fd.skills, "output.SkillDPS = fullDPS.skills (Calcs.lua:395)");
    for (i, s) in fd.skills.iter().enumerate() {
        let k = (i + 1).to_string();
        eq(
            format!("fullDPS.skills.{k}.name"),
            lj_str(&fx, &["fullDPS", "skills", &k, "name"]),
            s.name.clone(),
        );
        eq(
            format!("fullDPS.skills.{k}.dps"),
            lj_str(&fx, &["fullDPS", "skills", &k, "dps"]),
            g(s.dps),
        );
        eq(
            format!("fullDPS.skills.{k}.count"),
            lj_str(&fx, &["fullDPS", "skills", &k, "count"]),
            g(s.count),
        );
        for (f, v) in [("trigger", &s.trigger), ("skillPart", &s.skill_part), ("source", &s.source)] {
            let lj = fx["fullDPS"]["skills"][&k][f].as_str().unwrap_or("nil");
            eq(format!("fullDPS.skills.{k}.{f}"), lj, v.clone().unwrap_or_else(|| "nil".into()));
        }
    }
    // Calcs.lua:396-397
    eq(
        "output.FullDPS".into(),
        lj_str(&fx, &["output", "nums", "FullDPS"]),
        g(o.opt_num("FullDPS").expect("FullDPS")),
    );
    eq(
        "output.FullDotDPS".into(),
        lj_str(&fx, &["output", "nums", "FullDotDPS"]),
        g(o.opt_num("FullDotDPS").expect("FullDotDPS")),
    );
    // the numbers R7.7 had wrong (Rust 17129.108990596105 / 12992.892629076245 / 6212224.3555320418), spelled out
    assert_eq!(g(fd.bleed_dps), "17107.24155839816");
    assert_eq!(g(fd.total_poison_dps), "12976.464923869378");
    assert_eq!(g(fd.combined_dps), "6212181.8053793702");
    assert!(
        bad.is_empty(),
        "fd-gen-melee calcFullDPS != LuaJIT ({}):\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// The shape of the model: MAIN's modDB is parentless and owns the base / config mods; the full-DPS env's modDB has `cachedPlayerDB`
/// (a `specCopy` of MAIN's own mods, Common.lua:524-543) as its parent and does not hold them itself; every CALCULATOR cache entry
/// `calcFullDPS` wrote holds the one env it re-initialises in place (`Env = env`, Common.lua:939; Calcs.lua:337-344), and each
/// re-init counts as an initEnv.
#[test]
fn full_dps_env_reads_main_config_through_parent() {
    let (sess, _) = session("fd-gen-melee");
    let out = sess.build_output(CalcMode::Main).expect("buildOutput");
    let FullDpsOutcome::Done(fd) = &out.full_dps else {
        panic!("calcFullDPS stopped")
    };
    let main = out.run.borrow();
    assert!(main.env.mod_db.borrow().parent.is_none(), "MAIN initEnv has no specEnv");
    let own_count = |db: &pob_mods::StoreRef, name: &str| match &db.borrow().kind {
        pob_mods::StoreKind::Db(d) => d.mods.get(name).map_or(0, Vec::len),
        pob_mods::StoreKind::List(_) => unreachable!(),
    };
    // CalcSetup.lua:590-650: the base mods live in MAIN's own DB
    assert!(own_count(&main.env.mod_db, "TotemFireResist") > 0);

    let cache = sess.cache();
    let uuids = cache.uuids(CalcMode::Calculator);
    assert!(!uuids.is_empty());
    let first = cache.get(CalcMode::Calculator, &uuids[0]).expect("entry").run.clone();
    for u in &uuids {
        let e = cache.get(CalcMode::Calculator, u).expect("entry");
        let r = e.run.borrow();
        if r.env.limited_skills.is_none() {
            // not a buildActiveSkill env: it is calcFullDPS's
            assert!(Rc::ptr_eq(&e.run, &first), "{u}: calcFullDPS caches one env");
        }
    }
    let full = first.borrow();
    let parent = full
        .env
        .mod_db
        .borrow()
        .parent
        .clone()
        .expect("full-DPS modDB has cachedPlayerDB as parent");
    assert!(!Rc::ptr_eq(&parent, &main.env.mod_db), "specCopy, not MAIN's DB itself");
    assert_eq!(
        own_count(&full.env.mod_db, "TotemFireResist"),
        0,
        "base mods are not added again (:590)"
    );
    assert!(own_count(&parent, "TotemFireResist") > 0);
    assert!(full.env.enemy_db.borrow().parent.is_some(), "cachedEnemyDB (:690)");
    drop(full);
    drop(main);
    drop(cache);

    // initEnvs: MAIN, calcFullDPS's first, one accelerated re-init per performed skill (:337-344), plus the buildActiveSkill ones.
    let log = sess.log();
    let full_dps_skills = fd
        .skills
        .iter()
        .filter(|s| s.source.is_none() && !s.name.starts_with("Full "))
        .count();
    assert!(full_dps_skills >= 2);
    assert_eq!(log.init_env_calls, 2 + full_dps_skills + log.bas.len(), "initEnv count");
}

/// `getMiscCalculator`'s base pass (Calcs.lua:89-98): a CALCULATOR initEnv hands its own DBs to calcFullDPS (:93). Its cached DBs hold
/// the same base / config mods as MAIN's (initEnv differs between the two modes only in display state), so FullDPS is the LuaJIT
/// buildOutput value of fd-gen-melee.
#[test]
fn misc_calculator_full_dps_equals_build_output() {
    let (sess, fx) = session("fd-gen-melee");
    sess.build_output(CalcMode::Main).expect("buildOutput");
    let out = sess.misc_calculator_base().expect("getMiscCalculator");
    let r = out.run.borrow();
    assert_eq!(r.env.mode, "CALCULATOR");
    let o = &r.state.player.output;
    assert_eq!(
        g(o.opt_num("FullDPS").expect("FullDPS")),
        lj_str(&fx, &["output", "nums", "FullDPS"])
    );
    assert_eq!(
        g(o.opt_num("FullDotDPS").expect("FullDotDPS")),
        lj_str(&fx, &["output", "nums", "FullDotDPS"])
    );
}

/// R7.2b perf (card p. 3): `buildOutput(MAIN)` (fresh session each, so `BuildShared` included) and a second `calcFullDPS(CALCULATOR)` on
/// the same session, median of 15 after a warm-up, on the eight full-DPS scenarios that finish. Uses only API that predates R7.2b, so the
/// same file measures the tree before it.
/// `cargo test -p pob-calc --release --test spec_env -- --ignored --nocapture perf`
#[test]
#[ignore = "timing run; release build"]
fn perf_full_dps_spec_env() {
    let median = |mut v: Vec<f64>| -> f64 {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let mut rows = Vec::new();
    for name in [
        "fd-gen-melee",
        "fd-all-occ",
        "fd-mir-bow-ballista",
        "fd-occ-dot",
        "fd-occ-group-count",
        "fd-occ-poison-ignite",
        "fd-occ-spells",
        "fd-occ-trap-mine-totem",
    ] {
        let (mut bo, mut fdps) = (Vec::new(), Vec::new());
        let mut inits = 0;
        for i in 0..16 {
            let (sess, _) = session(name);
            let t = std::time::Instant::now();
            sess.build_output(CalcMode::Main).expect("buildOutput");
            let a = t.elapsed().as_secs_f64() * 1000.0;
            let t = std::time::Instant::now();
            sess.calc_full_dps(CalcMode::Calculator).expect("calcFullDPS");
            let b = t.elapsed().as_secs_f64() * 1000.0;
            if i == 0 {
                continue;
            }
            bo.push(a);
            fdps.push(b);
            inits = sess.log().init_env_calls;
        }
        rows.push(format!(
            "{name}: buildOutput {:.3} ms; calcFullDPS {:.3} ms; initEnv calls {inits}",
            median(bo),
            median(fdps)
        ));
    }
    println!("{}", rows.join("\n"));
}
