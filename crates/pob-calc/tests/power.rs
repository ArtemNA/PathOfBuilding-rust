//! R8.7 gate (R8-ARCH §2 (г), card `tasks/R8.7-node-power.md`): node power against the LuaJIT fixtures
//! `tests/fixtures/power/<build>.json` (producer `tools/lua-corpus/export-power.ts`), 6 goldens.
//!
//!   (a) the tree paths: `pathDist`, `path` (ids, in order), `depends`, `allMasteryOptions`, `modKey ~= ""`, `distanceToClassStart` of
//!       every spec node = LuaJIT;
//!   (b) `PowerBuilder` for the default stat (`powerStat = nil`, Offence/Defence) and `FullDPS`: `node.power` of every node, the cluster
//!       notables' power, `powerMax`, bit for bit; `BuildPowerReportList` = LuaJIT;
//!   (c) the threaded build = the sequential one; perf (`--nocapture`).

use std::rc::Rc;

use pob_calc::{CalcSession, CalculateOptions};
use pob_golden::registry::GOLDEN_BUILDS;
use serde_json::Value as Json;

fn fixture(name: &str) -> Json {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/power")
        .join(format!("{}.json", name.replace(' ', "-")));
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

/// `%.17g` as the probe prints it (`pob_lua::format`).
fn g17(v: f64) -> String {
    pob_lua::format::format_g(v, 17)
}

fn ids(v: &Json) -> Vec<u64> {
    v.as_array().expect("array").iter().map(|x| x.as_u64().expect("id")).collect()
}

#[test]
fn paths_equal_luajit() {
    let mut report = Vec::new();
    let mut bad = Vec::new();
    for g in GOLDEN_BUILDS {
        let fx = fixture(g.name);
        let sess = session(g.name);
        let shared = sess.shared().expect("BuildShared");
        let spec = &shared.spec;
        let paths = fx["paths"].as_object().expect("paths");
        let mut n = 0;
        let mut diffs = 0;
        for (id, e) in paths {
            let id: u32 = id.parse().expect("id");
            let Some(node) = spec.node(id) else {
                bad.push(format!("{}: node {id} missing in Rust", g.name));
                diffs += 1;
                continue;
            };
            n += 1;
            let mut d = Vec::new();
            if g17(node.path_dist) != e["pathDist"].as_str().unwrap() {
                d.push(format!("pathDist {} vs {}", g17(node.path_dist), e["pathDist"]));
            }
            let ours_path: Option<Vec<u64>> = node.path.as_ref().map(|p| p.iter().map(|&i| u64::from(spec.nodes[i].id)).collect());
            let theirs_path = e.get("path").map(ids);
            if ours_path != theirs_path {
                d.push(format!("path {ours_path:?} vs {theirs_path:?}"));
            }
            let ours_dep: Vec<u64> = node.depends.iter().map(|&i| u64::from(spec.nodes[i].id)).collect();
            if Some(&ours_dep) != e.get("depends").map(ids).as_ref() {
                d.push(format!("depends {ours_dep:?} vs {:?}", e.get("depends")));
            }
            if node.alloc != e["alloc"].as_bool().unwrap() {
                d.push("alloc".into());
            }
            if node.all_mastery_options != e["allMasteryOptions"].as_bool().unwrap() {
                d.push("allMasteryOptions".into());
            }
            if node.mod_key().is_empty() != e["modKeyEmpty"].as_bool().unwrap() {
                d.push(format!("modKeyEmpty {}", node.mod_key().is_empty()));
            }
            let dcs = node.distance_to_class_start.map(g17);
            if dcs.as_deref() != e.get("distanceToClassStart").and_then(Json::as_str) {
                d.push(format!("distanceToClassStart {dcs:?} vs {:?}", e.get("distanceToClassStart")));
            }
            if !d.is_empty() {
                diffs += 1;
                if bad.len() < 40 {
                    bad.push(format!("{} node {id} ({}): {}", g.name, node.dn(), d.join("; ")));
                }
            }
        }
        if spec.nodes.len() != paths.len() {
            bad.push(format!("{}: {} spec nodes vs {} LuaJIT", g.name, spec.nodes.len(), paths.len()));
        }
        report.push(format!("{}: {n} nodes, {diffs} differ", g.name));
    }
    println!("{}", report.join("\n"));
    assert!(bad.is_empty(), "{}\n{}", report.join("\n"), bad.join("\n"));
}

#[test]
fn alloc_pairs_order_equals_luajit() {
    let mut bad = Vec::new();
    for g in GOLDEN_BUILDS {
        let fx = fixture(g.name);
        let sess = session(g.name);
        let shared = sess.shared().expect("BuildShared");
        let ours: Vec<u64> = shared.spec.alloc_table_pairs().into_iter().map(u64::from).collect();
        let theirs = ids(&fx["allocPairsOrder"]);
        println!("{}: pairs(allocNodes) {} keys, equal {}", g.name, ours.len(), ours == theirs);
        if ours != theirs {
            bad.push(format!(
                "{}: {ours:?}
 vs {theirs:?}",
                g.name
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{}",
        bad.join(
            "
"
        )
    );
}

use pob_calc::power::{NodePower, PowerProgress, PowerResult, PowerStat, build_power_threads, find_power_stat};

/// `node.power` as the probe dumps it (`%.17g` strings; a node with an empty table is absent).
fn dump_power(p: &NodePower) -> Option<Json> {
    let mut o = serde_json::Map::new();
    for (k, v) in [
        ("singleStat", p.single_stat),
        ("pathPower", p.path_power),
        ("offence", p.offence),
        ("defence", p.defence),
        ("distance", p.distance),
    ] {
        if let Some(v) = v {
            o.insert(k.into(), Json::String(g17(v)));
        }
    }
    if let Some(m) = &p.mastery_effects {
        let mut mo = serde_json::Map::new();
        for (eid, ep) in m {
            let mut e = serde_json::Map::new();
            for (k, v) in [
                ("singleStat", ep.single_stat),
                ("pathPower", ep.path_power),
                ("offence", ep.offence),
                ("defence", ep.defence),
            ] {
                if let Some(v) = v {
                    e.insert(k.into(), Json::String(g17(v)));
                }
            }
            mo.insert(eid.to_string(), Json::Object(e));
        }
        o.insert("masteryEffects".into(), Json::Object(mo));
    }
    (!o.is_empty()).then_some(Json::Object(o))
}

fn threads() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get()).min(16)
}

/// The power of every node / cluster notable and `powerMax` = LuaJIT, for one stat; returns (nodes compared, mismatches).
fn compare_power(
    name: &str,
    which: &str,
    stat: Option<&PowerStat>,
    res: &PowerResult,
    fx: &Json,
    spec: &pob_tree::PassiveSpec,
    bad: &mut Vec<String>,
) -> (usize, usize) {
    let s = &fx["stats"][which];
    let theirs = s["power"].as_object().expect("power");
    let mut n = 0;
    let mut miss = 0;
    let mut seen = std::collections::BTreeSet::new();
    for (i, node) in spec.nodes.iter().enumerate() {
        let ours = dump_power(&res.nodes[i]);
        let key = node.id.to_string();
        let t = theirs.get(&key);
        n += 1;
        seen.insert(key.clone());
        if ours.as_ref() != t {
            miss += 1;
            if bad.len() < 60 {
                bad.push(format!(
                    "{name} {which} node {key} ({}): ours {} vs LuaJIT {}",
                    node.dn(),
                    ours.map_or("nil".into(), |j| j.to_string()),
                    t.map_or("nil".into(), |j| j.to_string())
                ));
            }
        }
    }
    for k in theirs.keys().filter(|k| !seen.contains(*k)) {
        miss += 1;
        bad.push(format!("{name} {which} node {k}: missing in Rust"));
    }
    // cluster notables
    let tc = s["cluster"].as_object().expect("cluster");
    for (cname, (ti, p)) in &res.cluster {
        let ours = dump_power(p).map(|mut j| {
            j["id"] = Json::from(spec.tree.nodes[*ti].id);
            j
        });
        n += 1;
        if ours.as_ref() != tc.get(cname) {
            miss += 1;
            if bad.len() < 60 {
                bad.push(format!(
                    "{name} {which} cluster {cname}: ours {ours:?} vs LuaJIT {:?}",
                    tc.get(cname)
                ));
            }
        }
    }
    let ours_clusters = res.cluster.values().filter(|(_, p)| dump_power(p).is_some()).count();
    if ours_clusters != tc.len() {
        miss += 1;
        bad.push(format!("{name} {which}: {ours_clusters} cluster entries vs LuaJIT {}", tc.len()));
    }
    let pm = &res.power_max;
    for (k, v) in [
        ("singleStat", pm.single_stat),
        ("offence", pm.offence),
        ("offencePerPoint", pm.offence_per_point),
        ("defence", pm.defence),
        ("defencePerPoint", pm.defence_per_point),
    ] {
        if Some(g17(v).as_str()) != s["powerMax"][k].as_str() {
            miss += 1;
            bad.push(format!("{name} {which} powerMax.{k}: {} vs {}", g17(v), s["powerMax"][k]));
        }
    }
    let _ = stat;
    (n, miss)
}

#[test]
fn power_equals_luajit() {
    let mut report = Vec::new();
    let mut bad = Vec::new();
    let full_dps = find_power_stat("FullDPS").expect("FullDPS");
    for g in GOLDEN_BUILDS {
        let fx = fixture(g.name);
        let model = model(g.name);
        let spec = session(g.name).shared().expect("shared").spec.clone();
        for (which, stat) in [("OffDef", None), ("FullDPS", Some(&full_dps))] {
            let t0 = std::time::Instant::now();
            let res = build_power_threads(&model, CalculateOptions::default(), stat, None, threads(), None)
                .unwrap_or_else(|e| panic!("{}: {e}", g.name))
                .expect("not cancelled");
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let (n, miss) = compare_power(g.name, which, stat, &res, &fx, &spec, &mut bad);
            let granted = granted_of(&model);
            let ds = fx["stats"][which].get("displayStat").map(|d| pob_calc::power::DisplayStatFmt {
                fmt: d["fmt"].as_str().expect("fmt").to_owned(),
                pc: d["pc"].as_bool().unwrap_or(false),
                mod_: d["mod"].as_bool().unwrap_or(false),
                lower_is_better: d["lowerIsBetter"].as_bool().unwrap_or(false),
            });
            let nf = pob_calc::power::NumberFormat {
                show_thousands_separators: fx["main"]["showThousandsSeparators"].as_bool().expect("main"),
                thousands_separator: fx["main"]["thousandsSeparator"].as_str().expect("main").to_owned(),
                decimal_separator: fx["main"]["decimalSeparator"].as_str().expect("main").to_owned(),
            };
            let rep = pob_calc::power::build_power_report_list(
                &spec,
                &granted,
                &res,
                stat.or(pob_calc::power::power_stat_list().first()),
                ds.as_ref(),
                &nf,
            );
            let rep_note = compare_report(g.name, which, &rep, &fx, &mut bad);
            let lj_ms = fx["stats"][which]["ms"].as_f64().unwrap_or(f64::NAN);
            report.push(format!(
                "{} {which}: {n} nodes+clusters, {miss} differ; calls Rust {} / LuaJIT {}; total {}; {ms:.0} ms ({} threads) vs LuaJIT {lj_ms:.0} ms",
                g.name, res.calc_calls, fx["stats"][which]["calcCalls"], res.total, threads()
            ));
            report.push(format!("    {rep_note}"));
        }
    }
    println!("{}", report.join("\n"));
    assert!(bad.is_empty(), "{}\n{}", report.join("\n"), bad.join("\n"));
}

fn report_json(e: &pob_calc::power::ReportEntry) -> Json {
    // keys in byte order, as the fixture writes them
    serde_json::json!({
        "allocated": e.allocated,
        "id": e.id,
        "name": e.name,
        "pathDist": match &e.path_dist {
            pob_calc::power::ReportDist::Num(d) => Json::String(g17(*d)),
            pob_calc::power::ReportDist::Str(s) => Json::String((*s).to_owned()),
        },
        "pathPower": g17(e.path_power),
        "pathPowerStr": e.path_power_str,
        "power": g17(e.power),
        "powerStr": e.power_str,
        "type": e.ty.as_str(),
    })
}

/// `BuildPowerReportList` = LuaJIT: the same entries (all fields) and the same sequence of powers; entries of equal power may stand
/// in another order (upstream: `pairs(spec.nodes)` + an unstable `table.sort`, D13) -- counted, not hidden.
fn compare_report(name: &str, which: &str, ours: &[pob_calc::power::ReportEntry], fx: &Json, bad: &mut Vec<String>) -> String {
    let theirs = fx["stats"][which]["report"].as_array().expect("report");
    let ours: Vec<Json> = ours.iter().map(report_json).collect();
    if ours.len() != theirs.len() {
        bad.push(format!("{name} {which}: report {} entries vs LuaJIT {}", ours.len(), theirs.len()));
        return String::new();
    }
    let powers_equal = ours.iter().zip(theirs).all(|(a, b)| a["power"] == b["power"]);
    if !powers_equal {
        bad.push(format!("{name} {which}: the sequence of report powers differs"));
    }
    let key = |j: &Json| j.to_string();
    let mut a: Vec<String> = ours.iter().map(key).collect();
    let mut b: Vec<String> = theirs.iter().map(key).collect();
    let tie_moved = a.iter().zip(&b).filter(|(x, y)| x != y).count();
    a.sort();
    b.sort();
    if a != b {
        let only: Vec<&String> = a.iter().filter(|x| b.binary_search(x).is_err()).take(5).collect();
        bad.push(format!("{name} {which}: report entries differ, e.g. {only:?}"));
    }
    format!(
        "report {} entries, same entries {}, same power sequence {powers_equal}, {tie_moved} equal-power entries in another order",
        ours.len(),
        a == b
    )
}

/// `mainEnv.grantedPassives` of the MAIN buildOutput.
fn granted_of(model: &pob_build::BuildModel) -> indexmap::IndexMap<u32, bool> {
    let sess = CalcSession::new(Rc::new(model.clone()), CalculateOptions::default());
    let out = sess.build_output(pob_calc::CalcMode::Main).expect("buildOutput");
    out.run.borrow().env.granted_passives.clone()
}

/// (c) The one-session walk in steps (`engine_powerStep`) = the threaded walk, bit for bit; a cancelled walk has no result.
#[test]
fn steps_equal_threads_and_cancel() {
    let full_dps = find_power_stat("FullDPS").expect("FullDPS");
    let name = "Dual Savior";
    let model = model(name);
    for stat in [None, Some(&full_dps)] {
        let threaded = build_power_threads(&model, CalculateOptions::default(), stat, None, threads(), None)
            .expect("threads")
            .expect("not cancelled");
        let sess = CalcSession::new(Rc::new(model.clone()), CalculateOptions::default());
        let main = sess.build_output(pob_calc::CalcMode::Main).expect("buildOutput");
        let granted = main.run.borrow().env.granted_passives.clone();
        let calc = sess.get_misc_calculator().expect("getMiscCalculator");
        let t0 = std::time::Instant::now();
        let mut b = pob_calc::power::PowerBuilder::new(&calc, &granted, stat, None).expect("builder");
        let mut steps = 0;
        while !b.step(97).expect("step") {
            steps += 1;
        }
        let seq = b.result().expect("done");
        println!(
            "{name} {:?}: {} units in {} steps, one session {:.0} ms; equal to {} threads: {}",
            stat.map(|s| s.label.clone()),
            b.progress().1,
            steps + 1,
            t0.elapsed().as_secs_f64() * 1000.0,
            threads(),
            seq == threaded
        );
        assert!(seq == threaded, "{name}: the step-by-step walk differs from the threaded one");
        let mut c = pob_calc::power::PowerBuilder::new(&calc, &granted, stat, None).expect("builder");
        assert!(!c.step(3).expect("step"));
        c.cancel();
        assert!(c.is_done() && c.result().is_none(), "a cancelled walk has no result");
    }
}

/// U3d: the counter of the walk on threads: `nodeIndex` / `total` of the coroutine (CalcsTab.lua:654-657), and the live paths as a parameter.
#[test]
fn walk_counts_nodes_and_takes_live_paths() {
    let p = PowerProgress::default();
    assert_eq!(p.percent(), None, "no plan yet");
    p.set(0, 1500);
    assert_eq!(p.percent(), Some(0));
    p.set(749, 1500);
    assert_eq!(p.percent(), Some(49), "m_floor(749/1500*100)");
    p.set(1500, 1500);
    assert_eq!(p.percent(), Some(100));
    p.set(3, 1500);
    assert_eq!(p.counts(), (1500, 1500), "out-of-order threads do not move it back");

    let model = model("Dual Savior");
    let sess = CalcSession::new(Rc::new(model.clone()), CalculateOptions::default());
    let main = sess.build_output(pob_calc::CalcMode::Main).expect("buildOutput");
    let granted = main.run.borrow().env.granted_passives.clone();
    let calc = sess.get_misc_calculator().expect("getMiscCalculator");
    let spec = sess.shared().expect("shared").spec.clone();
    let plan = pob_calc::power::PowerPlan::new(&spec, &granted, pob_calc::power::PowerMode::of(None), None);
    let after = plan.units_after_calls();
    assert_eq!(after.len(), plan.jobs.len() + 1);
    assert!(after.windows(2).all(|w| w[0] <= w[1]), "the walk only goes forward");
    assert_eq!(*after.last().expect("calls"), plan.units());
    drop(calc);

    let plain = build_power_threads(&model, CalculateOptions::default(), None, None, threads(), None)
        .expect("threads")
        .expect("not cancelled");
    let progress = PowerProgress::default();
    let live = pob_tree::LiveSpecPaths::of(&spec);
    let walked = pob_calc::power::build_power_walk(
        &model,
        CalculateOptions::default(),
        None,
        None,
        pob_calc::power::PowerWalk {
            threads: threads(),
            cancel: None,
            live_paths: Some(&live),
            progress: Some(&progress),
        },
    )
    .expect("walk")
    .expect("not cancelled");
    assert!(walked == plain, "the same paths: the same result");
    let (index, total) = progress.counts();
    println!("nodeIndex {index}, total {total}, percent {:?}", progress.percent());
    assert_eq!((index, total), (plan.units(), walked.total));
}
