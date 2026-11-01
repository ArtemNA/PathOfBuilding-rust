//! R7-synth: the offence branches the six goldens never execute (FullDPS groups, a minion as the main skill, per-skill functions of
//! `Data/Skills/*.lua`, triggers, mirages) against desktop-path LuaJIT, scenario by scenario.
//!
//! Corpus `tests/fixtures/offence-synth/<scenario>.{xml,luajit.json}` (producer `tools/lua-corpus/export-offence-synth.ts`): the XML is one
//! of the golden builds with `<Skills>` replaced (+ optional gear / config inputs / one `<CustomModifierBlock>`); the JSON holds the full MAIN
//! `output` as LuaJIT leaves it after `wipeGlobalCache` + `calcs.buildOutput(build, "MAIN")` (`output.{nums,strs,bools}` = every scalar as `%.17g`,
//! `output.tables` = structural dumps), `fullDPS` (the `calcFullDPS` result), `minion` (the minion actor's output), `mainSkill`, and `covers`
//! (the lines of CalcOffence/CalcTriggers/CalcMirages/Calcs.lua and the `Data/Skills` functions the scenario really executes).
//!
//! `offence_synth_matches_luajit` (R7.10b: 0 divergences on all 186 scenarios): `calculate_file(<scenario>.xml, Full, skills: true)`; EVERY scalar key of the LuaJIT
//! output is compared with Rust's `stats` (round4 = "ok"; bit for bit = "exact"); a key LuaJIT has and Rust lacks is "missing", the reverse "extra".
//! Prints "scenario -> compared / ok / exact / mismatch / missing / extra", per-category totals and every divergence. Table-valued keys
//! (`SkillDPS`, `Minion`, ...) are counted, not compared (Rust's `stats` are the oracle's scalar-only flatten).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use pob_calc::{CalculateOptions, ParseMode, StatValue, calculate_file};
use pob_golden::compare::{stats_exact, stats_match};
use pob_golden::value::StatValue as Gv;
use pob_lua::format_g;
use serde_json::Value as Json;

const MIN_SCENARIOS: usize = 40;
const MIN_SKILL_FNS: usize = 60;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/offence-synth")
}

/// A LuaJIT number as the oracle wire carries it (`engine_outputJSON`): NaN and the infinities become the strings `tostring` renders.
fn lua_num(s: &str) -> Gv {
    match s {
        "nan" | "-nan" | "inf" | "-inf" => Gv::Str(s.to_owned()),
        _ => Gv::Num(s.parse::<f64>().unwrap_or_else(|e| panic!("bad number {s:?}: {e}"))),
    }
}

fn to_golden(v: &StatValue) -> Gv {
    match v {
        StatValue::Num(n) => Gv::Num(*n),
        StatValue::Str(s) => Gv::Str(s.clone()),
        StatValue::Bool(b) => Gv::Bool(*b),
    }
}

fn show(v: Option<&Gv>) -> String {
    match v {
        None => "nil".to_owned(),
        Some(Gv::Num(n)) => format_g(*n, 17),
        Some(Gv::Str(s)) => format!("{s:?}"),
        Some(Gv::Bool(b)) => b.to_string(),
    }
}

/// The scalar part of a LuaJIT output dump (`nums`/`strs`/`bools`); an empty Lua table is `[]` in dkjson, i.e. no keys of that type.
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

fn scenario_names() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir())
        .unwrap_or_else(|e| panic!("{}: {e}", dir().display()))
        .filter_map(|e| {
            e.ok()?
                .file_name()
                .into_string()
                .ok()?
                .strip_suffix(".luajit.json")
                .map(str::to_owned)
        })
        .collect();
    v.sort();
    v
}

fn fixture(name: &str) -> Json {
    let text = std::fs::read_to_string(dir().join(format!("{name}.luajit.json"))).expect("fixture");
    serde_json::from_str(&text).expect("fixture json")
}

#[derive(Default)]
struct Row {
    scenario: String,
    category: String,
    compared: usize,
    ok: usize,
    exact: usize,
    missing: usize,
    extra: usize,
    tables_lua: usize,
    minion_keys: usize,
    /// R7-synth-2: Rust's `unimplemented` notes (a boundary reached although the output matched).
    unimplemented: Vec<String>,
    bad: Vec<String>,
}

fn run_scenario(name: &str) -> Row {
    let fx = fixture(name);
    assert_eq!(fx["scenario"].as_str(), Some(name), "{name}: scenario field");
    let lua = scalars(&fx["output"]);
    let covers: Vec<&str> = fx["covers"].as_array().expect("covers").iter().filter_map(Json::as_str).collect();
    let cover_text = covers.iter().take(3).copied().collect::<Vec<_>>().join(" | ");
    let mut row = Row {
        scenario: name.to_owned(),
        category: fx["category"].as_str().unwrap_or("?").to_owned(),
        tables_lua: fx["output"]["tables"].as_object().map_or(0, |o| o.len()),
        minion_keys: fx["minion"].as_object().map_or(0, |_| scalars(&fx["minion"]).len()),
        ..Row::default()
    };
    let opts = CalculateOptions {
        parse_mode: ParseMode::Full,
        skills: true,
        ..CalculateOptions::default()
    };
    // a panic or an `Err` inside Rust is a divergence of its own (reported, the other scenarios still run)
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        calculate_file(dir().join(format!("{name}.xml")), opts)
    }));
    let res = match outcome {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            row.bad
                .push(format!("{name} | <whole scenario> | rust Err: {e} | covers: {cover_text}"));
            return row;
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default();
            row.bad
                .push(format!("{name} | <whole scenario> | rust PANIC: {msg} | covers: {cover_text}"));
            return row;
        }
    };
    row.unimplemented.clone_from(&res.unimplemented);
    let rust: BTreeMap<&str, Gv> = res.stats.iter().map(|(k, v)| (k.as_str(), to_golden(v))).collect();
    for (k, l) in &lua {
        row.compared += 1;
        match rust.get(k.as_str()) {
            Some(r) => {
                let (ok, exact) = (stats_match(l, r), stats_exact(l, r));
                row.ok += usize::from(ok);
                row.exact += usize::from(exact);
                if !ok || !exact {
                    row.bad.push(format!(
                        "{name} | {k} | rust {} | luajit {} | {} | covers: {cover_text}",
                        show(Some(r)),
                        show(Some(l)),
                        if ok { "round4-ok, NOT exact" } else { "MISMATCH" },
                    ));
                }
            }
            None => {
                row.missing += 1;
                row.bad.push(format!(
                    "{name} | {k} | rust nil | luajit {} | MISSING | covers: {cover_text}",
                    show(Some(l))
                ));
            }
        }
    }
    for (k, r) in &rust {
        if !lua.contains_key(*k) {
            row.extra += 1;
            row.bad.push(format!(
                "{name} | {k} | rust {} | luajit nil | EXTRA | covers: {cover_text}",
                show(Some(r))
            ));
        }
    }
    row
}

/// Fixture sanity (not ignored): the corpus is as large and as varied as the R7-synth card demands.
#[test]
fn offence_synth_corpus_is_wellformed() {
    let names = scenario_names();
    assert!(names.len() >= MIN_SCENARIOS, "{} scenarios < {MIN_SCENARIOS}", names.len());
    let mut cats: BTreeMap<String, usize> = BTreeMap::new();
    let mut fns = std::collections::BTreeSet::new();
    for n in &names {
        let fx = fixture(n);
        assert_eq!(fx["scenario"].as_str(), Some(n.as_str()));
        assert!(dir().join(format!("{n}.xml")).is_file(), "{n}: xml");
        assert!(!fx["covers"].as_array().expect("covers").is_empty(), "{n}: empty covers");
        assert!(fx["output"]["nums"].is_object(), "{n}: output.nums");
        assert!(fx["fullDPS"].is_object(), "{n}: fullDPS");
        *cats.entry(fx["category"].as_str().expect("category").to_owned()).or_default() += 1;
        for f in fx["fnCalled"].as_array().expect("fnCalled") {
            fns.insert(f.as_str().expect("closure path").to_owned());
        }
    }
    println!(
        "{} scenarios by category: {cats:?}; {} distinct Data/Skills functions called",
        names.len(),
        fns.len()
    );
    assert!(fns.len() >= MIN_SKILL_FNS, "{} skill functions < {MIN_SKILL_FNS}", fns.len());
}

#[test]
fn offence_synth_matches_luajit() {
    let names = scenario_names();
    assert!(names.len() >= MIN_SCENARIOS, "{} scenarios < {MIN_SCENARIOS}", names.len());
    let rows: Vec<Row> = names.iter().map(|n| run_scenario(n)).collect();
    let mut t = String::new();
    let _ = writeln!(
        t,
        "{:<40} {:<9} {:>8} {:>6} {:>6} {:>9} {:>8} {:>6} {:>7}",
        "scenario", "category", "compared", "ok", "exact", "mismatch", "missing", "extra", "tables"
    );
    let mut by_cat: BTreeMap<&str, [usize; 6]> = BTreeMap::new();
    for r in &rows {
        let mismatch = r.compared - r.ok - r.missing;
        let _ = writeln!(
            t,
            "{:<40} {:<9} {:>8} {:>6} {:>6} {:>9} {:>8} {:>6} {:>7}",
            r.scenario, r.category, r.compared, r.ok, r.exact, mismatch, r.missing, r.extra, r.tables_lua
        );
        let c = by_cat.entry(r.category.as_str()).or_default();
        for (i, v) in [r.compared, r.ok, r.exact, r.missing, r.extra, 1].into_iter().enumerate() {
            c[i] += v;
        }
    }
    println!("{t}");
    println!("--- per category: scenarios / compared / ok / exact / missing / extra ---");
    for (c, v) in &by_cat {
        println!(
            "{c:<10} {:>4} scen {:>7} compared {:>7} ok {:>7} exact {:>7} missing {:>5} extra",
            v[5], v[0], v[1], v[2], v[3], v[4]
        );
    }
    // R7-synth-2: a scenario pinned at a recorded boundary ([`SYNTH2_BOUNDARY`]) is gated by `synth2_scenarios_match_luajit` (exact count)
    let at_boundary = |r: &Row| SYNTH2_BOUNDARY.iter().any(|(s, _, _)| *s == r.scenario);
    let bad_total: usize = rows.iter().filter(|r| !at_boundary(r)).map(|r| r.bad.len()).sum();
    let bounded: usize = rows.iter().filter(|r| at_boundary(r)).map(|r| r.bad.len()).sum();
    println!("--- every divergence (scenario | key | rust | luajit | class | covers) ---");
    for r in rows.iter().filter(|r| !at_boundary(r)) {
        for b in &r.bad {
            println!("{b}");
        }
    }
    println!(
        "--- {bad_total} divergent (scenario, key) pairs (+ {bounded} in the {} scenarios at a recorded boundary); Lua minion scalar keys in the corpus: {} ---",
        SYNTH2_BOUNDARY.len(),
        rows.iter().map(|r| r.minion_keys).sum::<usize>()
    );
    assert_eq!(
        bad_total, 0,
        "{bad_total} (scenario, key) pairs differ from LuaJIT, see the report above"
    );
}

// --- R7.7: the `calcFullDPS` gate on the 15 `fulldps` scenarios -----------------------------------------------------------------------------

/// The `fullDPS` table in the probe's encoding (`%.17g` numbers; `skills` = `{"1": {...}, ...}`; nil fields absent).
fn full_dps_json(fd: &pob_calc::FullDps) -> Json {
    let n = |x: f64| Json::String(format_g(x, 17));
    let mut o = serde_json::Map::new();
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
        o.insert(k.to_owned(), n(v));
    }
    if let Some(c) = fd.culling_dps {
        o.insert("cullingDPS".to_owned(), n(c));
    }
    let mut skills = serde_json::Map::new();
    for (i, s) in fd.skills.iter().enumerate() {
        let mut e = serde_json::Map::new();
        e.insert("name".to_owned(), Json::String(s.name.clone()));
        e.insert("dps".to_owned(), n(s.dps));
        e.insert("count".to_owned(), n(s.count));
        for (k, v) in [("trigger", &s.trigger), ("skillPart", &s.skill_part), ("source", &s.source)] {
            if let Some(v) = v {
                e.insert(k.to_owned(), Json::String(v.clone()));
            }
        }
        skills.insert((i + 1).to_string(), Json::Object(e));
    }
    o.insert("skills".to_owned(), Json::Object(skills));
    Json::Object(o)
}

/// Every difference between two JSON objects (one level of nesting for `skills`), as `key: lj .. rust ..` lines.
fn json_diff(prefix: &str, lj: &Json, rust: &Json, out: &mut Vec<String>) {
    let (Some(a), Some(b)) = (lj.as_object(), rust.as_object()) else {
        if lj != rust {
            out.push(format!("{prefix}: lj {lj} rust {rust}"));
        }
        return;
    };
    let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for k in keys {
        let (x, y) = (a.get(k).unwrap_or(&Json::Null), b.get(k).unwrap_or(&Json::Null));
        if x.is_object() || y.is_object() {
            json_diff(&format!("{prefix}.{k}"), x, y, out);
        } else if x != y {
            out.push(format!("{prefix}.{k}: lj {x} rust {y}"));
        }
    }
}

/// The scenarios whose `calcFullDPS` Rust cannot finish yet, and the boundary it must report (measured R7.7: the roll-up stops at the
/// first included skill whose run needs an unported triggers / mirages branch, or at a minion skill -- R7.8/R7.9/R7.10). R7.9 (`calcs.mirages`
/// ported): the mirage boundaries are gone. R7.10b (the minion's triggers / offence and the roll-up :178-212 ported): none left --
/// `fd-all-cos`, `fd-all-gen`, `fd-all-mir`, `fd-occ-minions` (golems, zombies, skeletons) = LuaJIT.
const FULLDPS_BOUNDARY: [(&str, &str); 0] = [];

/// A scenario whose roll-up runs to the end but differs from LuaJIT, with the measured cause.
/// `fd-cos-coc` (R7.8, `calcs.triggers` ported: every number = LuaJIT) -- `skills[2].trigger` = `activeSkill.infoTrigger` (Calcs.lua:254), which
/// `calcs.triggers` now writes (`ActiveSkill::info_trigger` = "Cast On Critical Strike"); `CalcSession::calc_full_dps` (session.rs, outside
/// the R7.8 card files) still takes `trigger` only from the Saviour note -- a one-line read of `info_trigger` there closes it.
/// (`fd-gen-melee`, the R7.7 `specEnv` / `cachedPlayerDB` class, = LuaJIT since R7.2b.)
const FULLDPS_KNOWN: [(&str, &[&str]); 0] = [];

/// The class every R7-synth scenario shares that is not the trigger code's: `output.GemHasLevel` / `GemLevel` / `GemHasQuality` /
/// `GemQuality` are written by CalcPerform.lua:3935-3975 (the perform tail after offence, "3803-3990 minion output, buff export, party
/// export (P5)") -- MISSING in Rust on all 186 scenarios until the R7 tail ported CalcPerform.lua:3935-3986; empty since.
const PERFORM_TAIL_GEM: [&str; 0] = [];

/// R7.8 gate (card p. 2): the 21 `trigger` scenarios of R7-synth (CoC, CoMK, CWC, CWDT, Cospri's, Mjolner, Arcanist Brand, Spellslinger,
/// Hextouch, cast on death / stun / block, spell on (bow) attack, traps / mines) against LuaJIT, EVERY scalar key exactly (bit for bit);
/// the only allowed divergence is [`PERFORM_TAIL_GEM`] missing.
#[test]
fn trigger_scenarios_match_luajit() {
    let mut equal = Vec::new();
    let mut bad = Vec::new();
    let mut compared = 0;
    for name in scenario_names() {
        if fixture(&name)["category"].as_str() != Some("trigger") {
            continue;
        }
        let r = run_scenario(&name);
        compared += r.compared;
        let other: Vec<&String> = r
            .bad
            .iter()
            .filter(|b| {
                let mut f = b.split(" | ");
                let key = f.nth(1).unwrap_or("");
                !(PERFORM_TAIL_GEM.contains(&key) && b.contains("| MISSING |"))
            })
            .collect();
        println!(
            "{name}: compared {} ok {} exact {} missing {} extra {} -- outside the perform-tail class: {}",
            r.compared,
            r.ok,
            r.exact,
            r.missing,
            r.extra,
            other.len()
        );
        if other.is_empty() && r.missing == PERFORM_TAIL_GEM.len() {
            equal.push(name);
        } else {
            bad.extend(other.into_iter().cloned());
            if r.missing != PERFORM_TAIL_GEM.len() {
                bad.push(format!("{name}: {} missing", r.missing));
            }
        }
    }
    println!(
        "trigger: {} scenarios = LuaJIT (but {PERFORM_TAIL_GEM:?}); {compared} keys compared",
        equal.len()
    );
    assert!(bad.is_empty(), "trigger scenarios != LuaJIT ({}):\n{}", bad.len(), bad.join("\n"));
    assert_eq!(equal.len(), 21, "pinned: all 21 trigger scenarios");
}

/// R7.7 gate (card p. 2): `calcs.calcFullDPS` (Calcs.lua:142-356) and the buildOutput tail (`SkillDPS` / `FullDPS` / `FullDotDPS`,
/// :395-397) against LuaJIT on the 15 `fulldps` R7-synth scenarios: the whole `fullDPS` table (every number `%.17g`, every `skills`
/// entry with `trigger` / `skillPart` / `source`) and the three output keys, exactly. A scenario Rust cannot finish must report its
/// boundary ([`FULLDPS_BOUNDARY`]) and emit none of the three keys.
#[test]
fn fulldps_matches_luajit() {
    let mut done = Vec::new();
    let mut bounded = Vec::new();
    let mut known = Vec::new();
    let mut bad = Vec::new();
    for name in scenario_names() {
        let fx = fixture(&name);
        if fx["category"].as_str() != Some("fulldps") {
            continue;
        }
        let xml = pob_build::read_build_file(dir().join(format!("{name}.xml"))).expect("xml");
        let model = pob_build::load_build_xml(&xml).expect("xml parses");
        let opts = CalculateOptions {
            parse_mode: ParseMode::Full,
            skills: true,
            ..CalculateOptions::default()
        };
        let sess = pob_calc::CalcSession::new(std::rc::Rc::new(model), opts);
        let out = sess.build_output(pob_calc::CalcMode::Main).expect("buildOutput");
        let r = out.run.borrow();
        let o = &r.state.player.output;
        let lua = scalars(&fx["output"]);
        match &out.full_dps {
            pob_calc::FullDpsOutcome::Done(fd) => {
                let mut d = Vec::new();
                json_diff("fullDPS", &fx["fullDPS"], &full_dps_json(fd), &mut d);
                for k in ["FullDPS", "FullDotDPS"] {
                    let rv = o.opt_num(k).map(Gv::Num);
                    if lua.get(k) != rv.as_ref() {
                        d.push(format!("output.{k}: lj {} rust {}", show(lua.get(k)), show(rv.as_ref())));
                    }
                }
                let sd = match o.table("SkillDPS") {
                    Some(pob_calc::OutTable::SkillDps(v)) => full_dps_json(&pob_calc::FullDps {
                        skills: v.clone(),
                        ..pob_calc::FullDps::default()
                    })["skills"]
                        .clone(),
                    other => Json::String(format!("{other:?}")),
                };
                json_diff("output.SkillDPS", &fx["output"]["tables"]["SkillDPS"], &sd, &mut d);
                println!("{name}: Done, {} skills entries, differences {}", fd.skills.len(), d.len());
                for x in &d {
                    println!("  {name}: {x}");
                }
                if let Some((_, keys)) = FULLDPS_KNOWN.iter().find(|(n, _)| *n == name) {
                    let got: Vec<&str> = d.iter().map(|x| x.split(':').next().unwrap_or("")).collect();
                    if got != *keys {
                        bad.push(format!("{name}: known class changed: {got:?}"));
                    }
                    known.push(name.clone());
                    continue;
                }
                if d.is_empty() {
                    done.push(name.clone());
                }
                bad.extend(d.into_iter().map(|x| format!("{name}: {x}")));
            }
            pob_calc::FullDpsOutcome::Unimplemented { processed, note } => {
                println!("{name}: boundary after {processed} skills: {note}");
                let want = FULLDPS_BOUNDARY.iter().find(|(n, _)| *n == name).map(|(_, w)| *w);
                match want {
                    Some(w) if note.contains(w) => bounded.push(name.clone()),
                    _ => bad.push(format!("{name}: unexpected boundary ({want:?}): {note}")),
                }
                for k in ["FullDPS", "FullDotDPS", "SkillDPS"] {
                    if o.get(k).is_some() || o.table(k).is_some() {
                        bad.push(format!("{name}: {k} written although the roll-up stopped"));
                    }
                }
            }
        }
    }
    println!(
        "fulldps: {} = LuaJIT {done:?}; {} at a recorded boundary {bounded:?}; {} known class {known:?}",
        done.len(),
        bounded.len(),
        known.len()
    );
    assert!(bad.is_empty(), "calcFullDPS != LuaJIT ({}):\n{}", bad.len(), bad.join("\n"));
    assert_eq!(
        (done.len(), bounded.len(), known.len()),
        (15, 0, 0),
        "pinned split of the 15 fulldps scenarios"
    );
}

/// R7.9 gate (card p. 2): the 7 `mirage` scenarios of R7-synth (Mirage Archer x2, Sacred Wisps, The Saviour, Tawhoa's Chosen, General's Cry
/// x2) against LuaJIT, EVERY scalar key exactly (bit for bit); the only allowed divergence is [`PERFORM_TAIL_GEM`] missing.
#[test]
fn mirage_scenarios_match_luajit() {
    let mut equal = Vec::new();
    let mut bad = Vec::new();
    let mut compared = 0;
    for name in scenario_names() {
        if fixture(&name)["category"].as_str() != Some("mirage") {
            continue;
        }
        let r = run_scenario(&name);
        compared += r.compared;
        let other: Vec<&String> = r
            .bad
            .iter()
            .filter(|b| {
                let key = b.split(" | ").nth(1).unwrap_or("");
                !(PERFORM_TAIL_GEM.contains(&key) && b.contains("| MISSING |"))
            })
            .collect();
        println!(
            "{name}: compared {} ok {} exact {} missing {} extra {} -- outside the perform-tail class: {}",
            r.compared,
            r.ok,
            r.exact,
            r.missing,
            r.extra,
            other.len()
        );
        if other.is_empty() && r.missing == PERFORM_TAIL_GEM.len() {
            equal.push(name);
        } else {
            bad.extend(other.into_iter().cloned());
            if r.missing != PERFORM_TAIL_GEM.len() {
                bad.push(format!("{name}: {} missing", r.missing));
            }
        }
    }
    println!(
        "mirage: {} scenarios = LuaJIT (but {PERFORM_TAIL_GEM:?}); {compared} keys compared",
        equal.len()
    );
    assert!(bad.is_empty(), "mirage scenarios != LuaJIT ({}):\n{}", bad.len(), bad.join("\n"));
    assert_eq!(equal.len(), 7, "pinned: all 7 mirage scenarios");
}

// --- R7.10: the `minion` scenarios (a minion skill as the main skill) -------------------------------------------------------------------

/// The player's output of a `minion` scenario and the minion actor's output (`Minion.*`, written by perform / defence / estimations and,
/// since R7.10b, by `calcs.triggers(env, env.minion)` / `calcs.offence(env, env.minion, env.minion.mainSkill)`, CalcPerform.lua:3807-3808)
/// against LuaJIT: `(row, keys, equal, missing, other divergences)`.
fn minion_scenario(name: &str) -> (Row, usize, usize, usize, Vec<String>) {
    let row = run_scenario(name);
    let fx = fixture(name);
    let xml = pob_build::read_build_file(dir().join(format!("{name}.xml"))).expect("xml");
    let model = pob_build::load_build_xml(&xml).expect("xml parses");
    let opts = CalculateOptions {
        parse_mode: ParseMode::Full,
        skills: true,
        ..CalculateOptions::default()
    };
    let sess = pob_calc::CalcSession::new(std::rc::Rc::new(model), opts);
    let out = sess.build_output(pob_calc::CalcMode::Main).expect("buildOutput");
    let r = out.run.borrow();
    let lua = scalars(&fx["minion"]);
    let mut bad = Vec::new();
    let (mut equal, mut missing) = (0, 0);
    let Some(ms) = &r.state.minion else {
        bad.push(format!("{name} | Minion | rust has no env.minion"));
        return (row, lua.len(), 0, 0, bad);
    };
    let ms = ms.borrow();
    let rust: BTreeMap<String, Gv> = pob_calc::output_to_stats(&ms.output)
        .iter()
        .map(|(k, v)| (k.clone(), to_golden(v)))
        .collect();
    for (k, l) in &lua {
        match rust.get(k) {
            Some(v) if stats_exact(l, v) => equal += 1,
            Some(v) => bad.push(format!(
                "{name} | Minion.{k} | rust {} | luajit {} | MISMATCH",
                show(Some(v)),
                show(Some(l))
            )),
            None => missing += 1,
        }
    }
    for (k, v) in &rust {
        if !lua.contains_key(k) {
            bad.push(format!("{name} | Minion.{k} | rust {} | luajit nil | EXTRA", show(Some(v))));
        }
    }
    (row, lua.len(), equal, missing, bad)
}

/// R7.10 / R7.10b gate: the 23 `minion` scenarios -- the player's output (every scalar key, exactly; the only allowed divergence is
/// [`PERFORM_TAIL_GEM`] missing) and the minion actor's output `Minion.*` (every key = LuaJIT bit for bit, no missing and no extra key).
#[test]
fn minion_scenarios_match_luajit() {
    let mut bad = Vec::new();
    let (mut n, mut player_equal, mut minion_equal_all) = (0, 0, 0);
    let (mut tot_keys, mut tot_equal, mut tot_missing) = (0, 0, 0);
    for name in scenario_names() {
        if fixture(&name)["category"].as_str() != Some("minion") {
            continue;
        }
        n += 1;
        let (r, keys, equal, missing, mbad) = minion_scenario(&name);
        let other: Vec<&String> = r
            .bad
            .iter()
            .filter(|b| {
                let key = b.split(" | ").nth(1).unwrap_or("");
                !(PERFORM_TAIL_GEM.contains(&key) && b.contains("| MISSING |"))
            })
            .collect();
        println!(
            "{name}: player compared {} ok {} exact {} missing {} extra {} (outside the perform-tail class {}); Minion keys {keys} equal {equal} missing {missing} other {}",
            r.compared,
            r.ok,
            r.exact,
            r.missing,
            r.extra,
            other.len(),
            mbad.len()
        );
        if other.is_empty() {
            player_equal += 1;
        }
        if mbad.is_empty() && missing == 0 {
            minion_equal_all += 1;
        }
        tot_keys += keys;
        tot_equal += equal;
        tot_missing += missing;
        bad.extend(other.into_iter().cloned());
        bad.extend(mbad);
    }
    println!(
        "minion: {n} scenarios; player output = LuaJIT on {player_equal}; Minion.* complete on {minion_equal_all}; Minion keys {tot_keys}: equal {tot_equal}, missing {tot_missing}"
    );
    for b in &bad {
        println!("{b}");
    }
    assert!(bad.is_empty(), "minion scenarios != LuaJIT ({}):\n{}", bad.len(), bad.join("\n"));
    // R7.10b: the player's output = LuaJIT on all 23; `Minion.*` complete and = LuaJIT bit for bit on all 23 (14 215 keys; before R7.10b
    // the 3 322 keys of the minion's triggers / offence were missing, CalcPerform.lua:3807-3808).
    assert_eq!(
        (n, player_equal, minion_equal_all, tot_keys, tot_equal, tot_missing),
        (23, 23, 23, 14215, 14215, 0),
        "pinned state of the 23 minion scenarios"
    );
}

// --- R7-synth-2: the branches R7 / R8.5b left without a LuaJIT check ------------------------------------------------------------------

/// `unimplemented` notes a synth2 scenario may carry beyond its base golden's, with the measured reason: `(scenario, note prefix, why)`.
const SYNTH2_KNOWN_NOTES: [(&str, &str, &str); 1] = [(
    "s2-enemy-explode-avg-2types",
    "Data/Skills/other.lua:6230 skills.EnemyExplode.preDamageFunc: pairs(typeAmountChances)",
    "R13 (lj-strhash): the fixture holds the LuaJIT run of Rust's insertion order; the other order changes no key (r13.otherOrder)",
)];

/// A scenario with a minion (`env.minion` set) carries this wording of the golden's own P5 note CalcPerform.lua:3803-3990 (party / buff
/// export) instead -- not a gap of the scenario.
const MINION_PATH_P5_NOTE: &str = "CalcPerform.lua:3811-3990 buff export, party export (P5)";

/// The synth2 scenarios that stop at a recorded boundary of the port (not = LuaJIT; pinned so the state cannot drift silently):
/// `(scenario, every own note's prefix, divergent (scenario, key) pairs)`. Empty since R7-synth-2 ported `copyActiveSkill` of a skill with
/// a minion (CalcActiveSkill.lua:185-190; before: Tawhoa's Chosen + Dominating Blow 131 divergent pairs, The Saviour + Dominating Blow
/// = LuaJIT but with the boundary note of its cached run).
const SYNTH2_BOUNDARY: [(&str, &[&str], usize); 0] = [];

/// The CalcPerform.lua:1488-1489 note (The Saviour's Reflection, perform/skills_loop.rs) names the skill's list index, which an extra
/// socket group shifts (`s2-saviour-dominating-blow-dlink`: skill 3/4 vs the golden's 2/3) -- compared without it.
fn note_key(u: &str) -> String {
    const P: &str = "CalcPerform.lua:1488-1489 activeSkill.infoMessage/infoTrigger of skill ";
    match u.strip_prefix(P) {
        Some(rest) => format!("{P}#{}", rest.trim_start_matches(|c: char| c.is_ascii_digit())),
        None => u.to_owned(),
    }
}

/// R7-synth-2 gate (tasks/R7-synth-2.md): the `synth2` scenarios -- skill functions FN_SKIP kept out (AvengingFlame, ExplosiveTrap / AltX,
/// FrozenSweep / AltX, MinionInstability, EnemyExplode "Average Of All Sources"), the `configTable` entries of CalcTriggers.lua no scenario
/// executed, minion skills under mirages (General's Cry, Tawhoa's Chosen, The Saviour with Dominating Blow), Destructive Link on a minion,
/// KineticFusillade's sequential projectiles -- against LuaJIT: EVERY scalar key of the player's output and of `Minion.*` bit for bit, no
/// missing / extra key, and no `unimplemented` note in Rust beyond the base golden's (a boundary reached on the way is a gap even if the
/// output happened to match) except [`SYNTH2_KNOWN_NOTES`]; [`SYNTH2_BOUNDARY`] scenarios are pinned at their boundary. R13 scenarios
/// (`r13` in the fixture): the output is the LuaJIT run that took Rust's order (MINEFIELD-RUST R13); `r13.otherOrder` lists what the other
/// order changes (printed).
#[test]
fn synth2_scenarios_match_luajit() {
    let (mut n, mut equal, mut keys, mut minion_keys) = (0, 0, 0, 0);
    let mut bad = Vec::new();
    let mut bounded = Vec::new();
    let mut baselines: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in scenario_names() {
        let fx = fixture(&name);
        if fx["category"].as_str() != Some("synth2") {
            continue;
        }
        n += 1;
        let (r, mk, mbad) = if fx["minion"].is_object() {
            let (r, mk, _equal, missing, mut mbad) = minion_scenario(&name);
            if missing > 0 {
                mbad.push(format!("{name} | Minion | {missing} keys missing in Rust"));
            }
            (r, mk, mbad)
        } else {
            (run_scenario(&name), 0, Vec::new())
        };
        let r13 = fx
            .get("r13")
            .filter(|v| v.is_object())
            .map(|v| format!("; R13 order {} other {}", v["order"], v["otherOrder"]));
        // the notes the base golden build itself carries (the P5 / UI-only gaps of every build) are not this scenario's
        let base = fx["build"].as_str().expect("build");
        let base_notes = baselines
            .entry(base.to_owned())
            .or_insert_with(|| {
                let g = pob_golden::registry::find_golden(base).expect("golden");
                let opts = CalculateOptions {
                    parse_mode: ParseMode::Full,
                    skills: true,
                    ..CalculateOptions::default()
                };
                calculate_file(g.xml_path(), opts).expect("golden").unimplemented
            })
            .clone();
        let own: Vec<String> = r
            .unimplemented
            .iter()
            .filter(|u| {
                !base_notes.iter().any(|b| note_key(b) == note_key(u))
                    && !(fx["minion"].is_object() && u.as_str() == MINION_PATH_P5_NOTE)
                    && !SYNTH2_KNOWN_NOTES.iter().any(|(s, p, _)| *s == name && u.starts_with(p))
            })
            .cloned()
            .collect();
        let divergent = r.bad.len() + mbad.len();
        println!(
            "{name}: player compared {} exact {} missing {} extra {}; Minion keys {mk}; own unimplemented notes {}{}",
            r.compared,
            r.exact,
            r.missing,
            r.extra,
            own.len(),
            r13.unwrap_or_default()
        );
        keys += r.compared;
        minion_keys += mk;
        if let Some((_, prefixes, count)) = SYNTH2_BOUNDARY.iter().find(|(s, _, _)| *s == name) {
            let notes_ok = own.len() == prefixes.len() && prefixes.iter().all(|p| own.iter().any(|u| u.starts_with(p)));
            println!("  {name}: at a recorded boundary -- {divergent} divergent pairs, notes {own:?}");
            if notes_ok && divergent == *count {
                bounded.push(name.clone());
            } else {
                bad.push(format!(
                    "{name}: boundary state changed: {divergent} divergent pairs (pinned {count}), notes {own:?}"
                ));
            }
            continue;
        }
        if divergent == 0 && own.is_empty() {
            equal += 1;
        }
        bad.extend(r.bad);
        bad.extend(mbad);
        bad.extend(own.into_iter().map(|u| format!("{name} | unimplemented | {u}")));
    }
    println!(
        "synth2: {n} scenarios, {equal} = LuaJIT, {} at a recorded boundary {bounded:?}; {keys} player keys + {minion_keys} Minion keys compared; {} divergences",
        bounded.len(),
        bad.len()
    );
    assert!(bad.is_empty(), "synth2 scenarios != LuaJIT ({}):\n{}", bad.len(), bad.join("\n"));
    assert_eq!((n, equal, bounded.len()), (48, 48, 0), "pinned split of the 48 synth2 scenarios");
}

/// Every hook `data/skills.json` lists is either a closure of `data/closures.json` or a [`pob_calc::skill_funcs::HOOK_ALIASES`] entry whose
/// target is one (act_dex.lua:7325 -- `skills.ExplosiveTrapAltX.preDamageFunc` is ExplosiveTrap's function; R7-synth-2 found the panic).
#[test]
fn hook_aliases_cover_every_hook() {
    let read = |f: &str| -> Json {
        serde_json::from_str(&std::fs::read_to_string(pob_data::data_dir().join(f)).expect("data file")).expect("json")
    };
    let closures: std::collections::BTreeSet<String> = read("closures.json")["closures"]
        .as_array()
        .expect("closures")
        .iter()
        .filter_map(|c| c["path"].as_str().map(str::to_owned))
        .collect();
    let skills = read("skills.json");
    let skills = skills.get("skills").unwrap_or(&skills).as_object().expect("skills").clone();
    let aliases = pob_calc::skill_funcs::HOOK_ALIASES;
    let mut via_alias = Vec::new();
    for (id, s) in &skills {
        for h in s["hooks"].as_array().into_iter().flatten().filter_map(Json::as_str) {
            let path = format!("skills.{id}.{h}");
            if closures.contains(&path) {
                continue;
            }
            let to = aliases.iter().find(|(a, _)| *a == path).map(|(_, t)| *t);
            assert!(
                to.is_some_and(|t| closures.contains(t)),
                "{path}: neither a closure nor an alias of one"
            );
            via_alias.push(path);
        }
    }
    let want: Vec<String> = aliases.iter().map(|(a, _)| (*a).to_owned()).collect();
    assert_eq!(via_alias, want, "every alias is used");
}
