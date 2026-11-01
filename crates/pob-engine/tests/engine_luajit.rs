//! R8.8 gate: the `Engine` readers and mutators = LuaJIT running boot.lua's logic, scenario by scenario.
//!
//! Fixtures `tests/fixtures/engine/<slug>.engine.json` (producer `tools/lua-corpus/export-engine.ts`): after `loadBuildFromXML` + recalc --
//! `output` (`engine_outputJSON`: every scalar of `mainOutput`, numbers as `%.17g`), `meta`, `skillList`, `fullDPS`, `configList` (with `eligible`),
//! `statsFor[g]` (a diff against `output`) and `mutations` (boot.lua's mutator paths + recalc, each `{ op, args, after = diff, backEqual, back }`).
//! The Rust engine replays every mutation and must land on `output` + `after`, then back on `output` + `back` -- every key, bit for bit
//! (`%.17g`). `eligible` (the ConfigTab control's `shown` closure) is not ported: counted, not compared.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_calc::StatValue;
use pob_calc::pob_build::InputValue;
use pob_engine::engine::{Engine, GemProp};
use pob_lua::format_g;
use serde_json::Value as Json;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/engine")
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn fixtures() -> Vec<(String, Json)> {
    let mut v: Vec<String> = std::fs::read_dir(dir())
        .expect("fixtures dir")
        .filter_map(|e| {
            e.ok()?
                .file_name()
                .into_string()
                .ok()?
                .strip_suffix(".engine.json")
                .map(str::to_owned)
        })
        .collect();
    v.sort();
    v.into_iter()
        .map(|s| {
            let j: Json = serde_json::from_str(&read(&dir().join(format!("{s}.engine.json")))).expect("json");
            (s, j)
        })
        .collect()
}

fn xml_of(fx: &Json) -> String {
    let name = fx["build"].as_str().expect("build");
    let path = match fx["kind"].as_str() {
        Some("golden") => pob_golden::registry::find_golden(name).expect("golden").xml_path(),
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../pob-calc/tests/fixtures/offence-synth/{name}.xml")),
    };
    read(&path)
}

/// The probe's `N`.
fn n(v: f64) -> Json {
    Json::String(if v.is_nan() { "nan".into() } else { format_g(v, 17) })
}

/// The probe's `S`: a numeric-looking string gets `$str:`.
fn s(v: &str) -> Json {
    let numeric = matches!(v, "nan" | "inf" | "-inf")
        || pob_lua::find(v.as_bytes(), b"^%-?[%d%.]+[eE]?[%+%-]?%d*$", None, false)
            .ok()
            .flatten()
            .is_some();
    Json::String(if numeric { format!("$str:{v}") } else { v.to_owned() })
}

/// `engine_outputJSON` of the engine, in the probe's form.
fn output(e: &Engine) -> BTreeMap<String, Json> {
    e.result()
        .stats
        .iter()
        .map(|(k, v)| {
            let j = match v {
                StatValue::Num(x) => n(*x),
                StatValue::Str(x) => s(x),
                StatValue::Bool(b) => Json::Bool(*b),
            };
            (k.clone(), j)
        })
        .collect()
}

/// `base` with a probe diff applied (`"$nil"` = the key is gone).
fn apply(base: &BTreeMap<String, Json>, diff: &Json) -> BTreeMap<String, Json> {
    let mut m = base.clone();
    if let Some(d) = diff.as_object() {
        for (k, v) in d {
            if v.as_str() == Some("$nil") {
                m.remove(k);
            } else {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    m
}

fn map_diff(got: &BTreeMap<String, Json>, want: &BTreeMap<String, Json>) -> Option<String> {
    let keys: std::collections::BTreeSet<&String> = got.keys().chain(want.keys()).collect();
    let bad: Vec<String> = keys
        .into_iter()
        .filter(|k| got.get(*k) != want.get(*k))
        .map(|k| format!("{k}: rust {:?} luajit {:?}", got.get(k), want.get(k)))
        .collect();
    (!bad.is_empty()).then(|| {
        format!(
            "{} keys differ, e.g. {}",
            bad.len(),
            bad.iter().take(3).cloned().collect::<Vec<_>>().join("; ")
        )
    })
}

fn as_obj(j: &Json) -> BTreeMap<String, Json> {
    j.as_object()
        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

/// One mutation of the fixture applied through the engine API; `true` = forward, `false` = inverse (`before` = the LuaJIT value before).
fn mutate(e: &mut Engine, m: &Json, forward: bool, before: &Json) -> Result<(), String> {
    let a = &m["args"];
    match m["op"].as_str().expect("op") {
        "allocNode" => e.set_node_alloc(a["id"].as_f64().expect("id"), forward),
        "deallocNode" => e.set_node_alloc(a["id"].as_f64().expect("id"), !forward),
        "setConfig" => {
            let id = a["id"].as_str().expect("id");
            let v = if forward { &a["value"] } else { before };
            match v {
                Json::Null => e.clear_config(id),
                Json::Bool(b) => e.set_config(id, Some(InputValue::Bool(*b))),
                Json::Number(x) => e.set_config(id, Some(InputValue::Num(x.as_f64().expect("num")))),
                Json::String(x) => e.set_config(id, Some(InputValue::Str(x.clone()))),
                other => Err(format!("setConfig value {other:?}")),
            }
        }
        "setGem" => {
            let (g, k) = (
                a["group"].as_u64().expect("group") as usize,
                a["gem"].as_u64().expect("gem") as usize,
            );
            let prop = match (a["prop"].as_str().expect("prop"), forward) {
                ("level", true) => GemProp::Level(a["value"].as_f64().expect("v")),
                ("level", false) => GemProp::Level(before.as_f64().expect("before")),
                ("quality", true) => GemProp::Quality(a["value"].as_f64().expect("v")),
                ("quality", false) => GemProp::Quality(before.as_f64().expect("before")),
                ("enabled", f) => GemProp::Enabled(!f),
                (p, _) => return Err(format!("setGem prop {p}")),
            };
            e.set_gem(g, k, prop)
        }
        "addItemText" => {
            let slot = a["slotName"].as_str().expect("slotName");
            if forward {
                e.add_item_text(a["text"].as_str().expect("text"), slot).map(|_| ())
            } else {
                e.equip_item(slot, a["prev"].as_f64().expect("prev"))
            }
        }
        "setMainSocketGroup" => {
            let g = if forward {
                a["group"].as_u64().expect("group") as usize
            } else {
                before.as_f64().expect("before") as usize
            };
            e.set_main_socket_group(g)
        }
        op => Err(format!("unknown op {op}")),
    }
}

/// The value a mutation's inverse restores (what the engine holds before the forward step).
fn before_of(e: &Engine, m: &Json) -> Json {
    let a = &m["args"];
    match m["op"].as_str().expect("op") {
        "setConfig" => e
            .result()
            .env
            .config
            .input
            .get(a["id"].as_str().expect("id"))
            .map_or(Json::Null, |v| match v {
                InputValue::Num(x) => serde_json::json!(x),
                InputValue::Str(x) => Json::String(x.clone()),
                InputValue::Bool(b) => Json::Bool(*b),
            }),
        "setGem" => {
            let st = e.gem_states(a["group"].as_u64().expect("g") as usize);
            let g = &st[a["gem"].as_u64().expect("k") as usize - 1];
            match a["prop"].as_str() {
                Some("level") => serde_json::json!(g.0),
                Some("quality") => serde_json::json!(g.1),
                _ => Json::Null,
            }
        }
        "setMainSocketGroup" => serde_json::json!(e.result().env.main_socket_group),
        _ => Json::Null,
    }
}

#[test]
fn readers_and_mutators_match_luajit() {
    let fx = fixtures();
    if fx.is_empty() {
        panic!("no fixtures in {} -- run tools/lua-corpus/export-engine.ts", dir().display());
    }
    let mut bad: Vec<String> = Vec::new();
    let mut c = BTreeMap::<&str, (usize, usize)>::new();
    let count = |c: &mut BTreeMap<&str, (usize, usize)>, k: &'static str, ok: bool| {
        let x = c.entry(k).or_default();
        x.0 += 1;
        x.1 += usize::from(ok);
        ok
    };
    let mut eligible_not_compared = 0;
    // op -> (runs, equal to LuaJIT, changing the output)
    let mut per_op = BTreeMap::<String, (usize, usize, usize)>::new();
    for (slug, f) in &fx {
        let mut e = Engine::load_xml(&xml_of(f)).unwrap_or_else(|err| panic!("{slug}: {err}"));
        // the probe: loadBuildFromXML, then recalc (`engine_recalc`: no RefreshStatList -- `SkillDPS` in output order)
        e.recalc().unwrap_or_else(|err| panic!("{slug}: {err}"));
        let base_lj = as_obj(&f["output"]);
        let base = output(&e);
        if let Some(d) = map_diff(&base, &base_lj) {
            bad.push(format!("{slug}: output: {d}"));
        }
        count(&mut c, "output keys", base == base_lj);

        // _meta
        let m = &e.result().meta;
        let meta = serde_json::json!({ "allocNodes": m.alloc_nodes, "items": m.items, "socketGroups": m.socket_groups, "mainSocketGroup": n(m.main_socket_group),
            "treeVersion": m.tree_version, "className": m.class_name, "ascendName": m.ascend_name });
        if !count(&mut c, "meta", meta == f["meta"]) {
            bad.push(format!("{slug}: meta rust {meta} luajit {}", f["meta"]));
        }

        // skillList
        let sl = e.skill_list_json();
        let mut sl_probe = Vec::new();
        let tab_groups = e
            .result()
            .env
            .skills
            .as_ref()
            .map(|st| st.tab.socket_group_list().to_vec())
            .unwrap_or_default();
        let gd = pob_data::gems::gems();
        // R13 (SkillsTab.lua:472-477): the probe's canonical name of a gem whose gameId has several variants with its grantedEffectId
        let ambiguous = |gi: &pob_calc::GemInstance| -> bool {
            let (Some(id), Some(sid)) = (gi.gem_data.as_ref(), gi.skill_id.as_ref()) else {
                return false;
            };
            let Some(game) = gd.gems.0.get(id).map(|d| d.game_id.clone()) else {
                return false;
            };
            gd.gems_by_game_id
                .0
                .get(&game)
                .is_some_and(|vs| vs.0.values().filter(|v| gd.gems.0[*v].granted_effect_id == *sid).count() > 1)
        };
        for (gi_idx, g) in sl.as_array().expect("array").iter().enumerate() {
            let gems: Vec<Json> = g["gems"]
                .as_array()
                .expect("gems")
                .iter()
                .enumerate()
                .map(|(k, x)| {
                    let mut o = serde_json::Map::new();
                    for k in ["name", "enabled", "isSupport"] {
                        if !x[k].is_null() {
                            o.insert(k.into(), x[k].clone());
                        }
                    }
                    if tab_groups.get(gi_idx).and_then(|g| g.gem_list.get(k)).is_some_and(ambiguous) {
                        o.insert("name".into(), Json::String("<R13-ambiguous>".into()));
                    }
                    for k in ["level", "quality"] {
                        if let Some(v) = x[k].as_f64() {
                            o.insert(k.into(), n(v));
                        }
                    }
                    Json::Object(o)
                })
                .collect();
            let mut o = g.as_object().expect("obj").clone();
            o.insert("gems".into(), Json::Array(gems));
            sl_probe.push(Json::Object(o));
        }
        if !count(&mut c, "skillList", Json::Array(sl_probe.clone()) == f["skillList"]) {
            let i = sl_probe
                .iter()
                .zip(f["skillList"].as_array().expect("arr"))
                .position(|(a, b)| a != b);
            bad.push(format!(
                "{slug}: skillList differs at {i:?}: rust {:?} luajit {:?}",
                i.map(|i| &sl_probe[i]),
                i.map(|i| &f["skillList"][i])
            ));
        }

        // fullDPS
        let fd: Vec<Json> = e
            .full_dps_json()
            .as_array()
            .expect("arr")
            .iter()
            .map(|x| {
                let mut o = x.as_object().expect("obj").clone();
                for k in ["dps", "count"] {
                    let v = o[k].as_f64().expect("num");
                    o.insert(k.into(), n(v));
                }
                Json::Object(o)
            })
            .collect();
        if !count(&mut c, "fullDPS", Json::Array(fd.clone()) == f["fullDPS"]) {
            bad.push(format!("{slug}: fullDPS rust {fd:?} luajit {}", f["fullDPS"]));
        }

        // configList (without `eligible`)
        let cl = e.config_list_json();
        let lj = f["configList"].as_array().expect("configList");
        let rs = cl.as_array().expect("arr");
        let mut cl_ok = rs.len() == lj.len();
        if !cl_ok {
            bad.push(format!("{slug}: configList {} entries vs {}", rs.len(), lj.len()));
        }
        for (r, l) in rs.iter().zip(lj) {
            eligible_not_compared += 1;
            let mut r = r.as_object().expect("obj").clone();
            if let Some(v) = r.get("value").cloned() {
                r.insert(
                    "value".into(),
                    match v {
                        Json::Number(x) => n(x.as_f64().expect("f")),
                        Json::String(x) => s(&x),
                        o => o,
                    },
                );
            }
            if let Some(Json::Array(opts)) = r.get("options").cloned() {
                let opts: Vec<Json> = opts
                    .into_iter()
                    .map(|o| {
                        let mut o = o.as_object().expect("o").clone();
                        let v = match o["value"].clone() {
                            Json::Number(x) => n(x.as_f64().expect("f")),
                            Json::String(x) => s(&x),
                            other => other,
                        };
                        o.insert("value".into(), v);
                        Json::Object(o)
                    })
                    .collect();
                r.insert("options".into(), Json::Array(opts));
            }
            let mut l = l.as_object().expect("obj").clone();
            l.remove("eligible");
            if Json::Object(r.clone()) != Json::Object(l.clone()) {
                if cl_ok {
                    bad.push(format!("{slug}: configList entry rust {r:?} luajit {l:?}"));
                }
                cl_ok = false;
            }
        }
        count(&mut c, "configList", cl_ok);

        // statsFor
        for (gi, sf) in f["statsFor"].as_array().expect("statsFor").iter().enumerate() {
            let want = apply(&base_lj, &sf["after"]);
            let got: BTreeMap<String, Json> = match e.stats_for(gi + 1) {
                Ok(st) => st
                    .iter()
                    .map(|(k, v)| {
                        let j = match v {
                            StatValue::Num(x) => n(*x),
                            StatValue::Str(x) => s(x),
                            StatValue::Bool(b) => Json::Bool(*b),
                        };
                        (k.clone(), j)
                    })
                    .collect(),
                Err(err) => {
                    bad.push(format!("{slug}: statsFor({}): {err}", gi + 1));
                    continue;
                }
            };
            if !count(&mut c, "statsFor", got == want) {
                bad.push(format!(
                    "{slug}: statsFor({}): {}",
                    gi + 1,
                    map_diff(&got, &want).unwrap_or_default()
                ));
            }
            if output(&e) != base {
                bad.push(format!("{slug}: statsFor({}) did not restore the build", gi + 1));
            }
        }

        // mutations
        for m in f["mutations"].as_array().expect("mutations") {
            let label = format!("{slug}: {} {}", m["op"].as_str().unwrap_or(""), m["args"]);
            if let Some(err) = m.get("error") {
                bad.push(format!("{label}: LuaJIT raised {err}"));
                continue;
            }
            let before = before_of(&e, m);
            if let Err(err) = mutate(&mut e, m, true, &before) {
                bad.push(format!("{label}: {err}"));
                count(&mut c, "mutation", false);
                continue;
            }
            let ok1 = output(&e) == apply(&base_lj, &m["after"]);
            if !ok1 {
                bad.push(format!(
                    "{label}: after: {}",
                    map_diff(&output(&e), &apply(&base_lj, &m["after"])).unwrap_or_default()
                ));
            }
            if let Err(err) = mutate(&mut e, m, false, &before) {
                bad.push(format!("{label}: inverse: {err}"));
                count(&mut c, "mutation", false);
                continue;
            }
            let want_back = apply(&base_lj, &m["back"]);
            let ok2 = output(&e) == want_back;
            if !ok2 {
                bad.push(format!("{label}: back: {}", map_diff(&output(&e), &want_back).unwrap_or_default()));
            }
            count(&mut c, "mutation", ok1 && ok2);
            let x = per_op.entry(m["op"].as_str().unwrap_or("?").to_owned()).or_insert((0, 0, 0));
            x.0 += 1;
            x.1 += usize::from(ok1 && ok2);
            x.2 += usize::from(m["changed"].as_u64().unwrap_or(0) > 0);
            if m["changed"].as_u64().unwrap_or(0) > 0 {
                count(&mut c, "mutation (changing the output)", ok1 && ok2);
            }
        }
    }
    println!(
        "engine = LuaJIT over {} scenarios: (checked, equal) {c:?}; configList `eligible` not compared: {eligible_not_compared}; per mutator (runs, = LuaJIT, changing the output) {per_op:?}",
        fx.len()
    );
    assert!(bad.is_empty(), "{} failures:\n{}", bad.len(), bad.join("\n"));
}

/// `{ slug: { code, xml } }` of the Rust engine right after load: the build code and the `SaveDB` XML it encodes.
fn rust_codes_json() -> String {
    let mut out = serde_json::Map::new();
    for (slug, f) in fixtures() {
        let e = Engine::load_xml(&xml_of(&f)).expect("load");
        out.insert(
            slug,
            serde_json::json!({ "code": e.build_code().expect("code"), "xml": e.save_xml().expect("xml") }),
        );
    }
    serde_json::to_string(&Json::Object(out)).expect("json")
}

/// The probe's report on the Rust build codes (`rust-codes.report.json`): LuaJIT's `common.base64.decode` of every Rust code + zlib inflate
/// is the Rust `SaveDB` XML, and LuaJIT loading that XML gives the output of the original file -- for exactly the codes Rust makes now
/// (the report holds the sha256 of its input).
#[test]
fn rust_build_codes_decode_and_load_in_luajit() {
    let p = dir().join("rust-codes.report.json");
    let rep: Json = serde_json::from_str(&read(&p)).expect("json");
    assert_eq!(
        rep["rustCodesSha256"].as_str(),
        Some(pob_data::sha256::sha256_hex(rust_codes_json().as_bytes()).as_str()),
        "the report is for other Rust codes: rerun write_rust_codes + export-engine.ts"
    );
    let sc = rep["scenarios"].as_object().expect("scenarios");
    let mut bad = Vec::new();
    for (slug, r) in sc {
        if r["luajitDecodeInflateEqualsRustXml"] != Json::Bool(true) || r["luajitLoadOfRustXmlOutputDiffCount"].as_u64() != Some(0) {
            bad.push(format!("{slug}: {r}"));
        }
    }
    println!(
        "Rust build codes decoded by LuaJIT into the Rust XML and loaded to the same output: {}/{}",
        sc.len() - bad.len(),
        sc.len()
    );
    assert_eq!(sc.len(), 26);
    assert!(
        bad.is_empty(),
        "{}",
        bad.join(
            "
"
        )
    );
}

/// Writes `tools/.scratch/rust-engine-codes.json` (`{ slug: { code, xml } }`) for the probe's optional LuaJIT decode of Rust build codes.
#[test]
#[ignore = "writes the input of tools/lua-corpus/export-engine.ts"]
fn write_rust_codes() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/.scratch/rust-engine-codes.json");
    std::fs::write(&p, rust_codes_json()).expect("write");
    println!("wrote {}", p.display());
}
