//! U8b gate: the original `CompareTab` / `CompareEntry` (the whole PoB, run by the SG oracle `tools/sg-oracle/lua/compare-runner.lua`) vs this port, on the public goldens
//! (`tools/sg-oracle/scenes/compare/*.json`) and the local list `local_builds("compare")`.
//! - `compare_states_equal_original` (no GPU): every `dump` step = the oracle's: the selectors, the numbers of the entries, every `DrawString` of the tab's `Draw`.
//! - `compare_pixels_equal_original` (GPU/WARP, `#[ignore]`): every screenshot = the original's PNG; 0 pixels beyond 2/255 outside the lossy class.
//!   cargo test -p pob-ui --release --test compare -- --include-ignored --nocapture

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::{DrawList, Gfx, RawEvent, TestPlatform};
use pob_ui::control::Control;
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::main;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::rc::Rc;

const GOLDENS: [&str; 5] = [
    "Generals Perforate Zerker",
    "Dual Savior",
    "Dual Wield Cospris CoC",
    "Mirage Archer Toxic Rain",
    "OccVortex",
];

const BUILDS_329: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../builds/pc/3.29/");
const BUILDS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../builds/pc/");

/// A list of personal builds from the local `builds/local-builds.json` (not in git); empty without it.
fn local_builds(key: &str) -> Vec<String> {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../builds/local-builds.json");
    let v: Option<Value> = std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok());
    v.and_then(|v| {
        v[key]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
    })
    .unwrap_or_default()
}

fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => Value::from(f as i64),
            _ => v.clone(),
        },
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        _ => v.clone(),
    }
}

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("pob-ui-u8b-{}-{n}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let builds = root.join("Path of Building/Builds");
        std::fs::create_dir_all(&builds).expect("mkdir");
        let pob = scene::pob_root();
        for b in GOLDENS {
            std::fs::copy(format!("{pob}/spec/TestBuilds/3.13/{b}.xml"), builds.join(format!("{b}.xml"))).expect("copy golden");
        }
        // the same edits as compare-runner.lua
        let gem = |id: &str, skill: &str, name: &str| {
            format!(
                r#"<Gem level="20" gemId="Metadata/Items/Gems/{id}" skillId="{skill}" qualityId="Default" quality="0" enabled="true" nameSpec="{name}"/>"#
            )
        };
        let group = |g: String| format!(r#"<Skill mainActiveSkillCalcs="1" enabled="true" mainActiveSkill="1">{g}</Skill>"#);
        let mut x = std::fs::read_to_string(format!("{pob}/spec/TestBuilds/3.13/OccVortex.xml")).expect("golden");
        let insert_after_tag = |x: &mut String, tag: &str, what: &str| {
            let a = x.find(tag).expect("tag");
            let e = a + x[a..].find('>').expect(">") + 1;
            x.insert_str(e, what);
        };
        insert_after_tag(&mut x, "<Build ", r#"<Spectre id="Metadata/Monsters/Axis/AxisCaster"/>"#);
        let groups = group(gem("SkillGemRaiseSpectre", "RaiseSpectre", "Raise Spectre"))
            + &group(
                gem("SkillGemFireball", "Fireball", "Fireball")
                    + &gem("SkillGemSupportBlastchainMine", "SupportBlastchainMine", "Blastchain Mine"),
            )
            + &group(gem("SkillGemIncinerate", "Incinerate", "Incinerate"));
        insert_after_tag(&mut x, "<Skills ", &groups);
        std::fs::write(builds.join("Spectre Test.xml"), x).expect("spectre build");
        let y = std::fs::read_to_string(format!("{pob}/spec/TestBuilds/3.13/Dual Savior.xml")).expect("golden");
        std::fs::write(
            builds.join("Sets Test.xml"),
            y.replacen("</Items>", r#"<ItemSet id="2" title="Empty set"/></Items>"#, 1),
        )
        .expect("sets build");
        for b in local_builds("compare") {
            let src = format!("{BUILDS_329}{b}.xml");
            let src = if std::path::Path::new(&src).exists() {
                src
            } else {
                format!("{BUILDS}{b}.xml")
            };
            std::fs::copy(src, builds.join(format!("{b}.xml"))).expect("copy build");
        }
        std::fs::copy(
            scene::oracle_dir().join("scenes/tree/Settings.xml"),
            root.join("Path of Building/Settings.xml"),
        )
        .expect("settings");
        Self(root)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn frame(gfx: &mut Gfx, l: &Rc<Launch>) -> DrawList {
    let mut f = gfx.begin_frame();
    l.on_frame(&mut f);
    f.finish()
}

fn dump(g: &Gfx, st: &Value) -> Value {
    let m = main();
    let b = m.build_mode.clone();
    let mut d = serde_json::Map::new();
    d.insert("popups".into(), json!(m.popups.borrow().len()));
    if let Some(p) = m.popups.borrow().first() {
        d.insert("popupTitle".into(), json!(p.title.get(g)));
    }
    let tab = b.compare_tab.borrow().clone().expect("compareTab");
    let mut c = tab.debug_dump(g);
    let mut log = pob_ui::sg::text_log_take();
    log.sort();
    c["text"] = json!(log);
    d.insert("compare".into(), c);
    let cc = b.c();
    let lines: Vec<String> = cc
        .stat_box
        .list
        .borrow()
        .iter()
        .map(|l| {
            let col = |i: usize| l.cols.get(i).cloned().flatten().unwrap_or("nil".into());
            format!("{}|{}", col(0), col(1))
        })
        .collect();
    d.insert("build".into(), json!({ "viewMode": *b.view_mode.borrow(), "statBox": lines }));
    if let Some(f) = st["saved"].as_str() {
        let p = format!("{}{f}.xml", m.build_path.borrow());
        d.insert(
            "savedHasCompare".into(),
            json!(std::fs::read_to_string(p).expect("saved").contains("<Compare")),
        );
    }
    Value::Object(d)
}

fn oracle_state(o: &Value) -> Value {
    let s = &o["state"];
    assert!(s.is_object(), "oracle dump: {s}");
    let mut d = serde_json::Map::new();
    d.insert("popups".into(), s["popups"].clone());
    if !s["popupTitle"].is_null() {
        d.insert("popupTitle".into(), s["popupTitle"].clone());
    }
    d.insert("compare".into(), s["compare"].clone());
    d.insert("build".into(), s["build"].clone());
    if !s["savedHasCompare"].is_null() {
        d.insert("savedHasCompare".into(), s["savedHasCompare"].clone());
    }
    Value::Object(d)
}

fn centre(g: &Gfx, c: &dyn Control, dx: f64, dy: f64) -> (f64, f64) {
    let (x, y) = c.get_pos(g);
    let (w, h) = c.get_size(g);
    ((x + w / 2.0).floor() + dx, (y + h / 2.0).floor() + dy)
}

/// The cursor steps of compare-runner.lua.
fn cursor_step(gfx: &mut Gfx, st: &Value, cur: &mut (f64, f64)) {
    let m = main();
    let b = m.build_mode.clone();
    let off = |a: &Vec<Value>, i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0);
    let ctl = |name: &str| {
        b.compare_tab
            .borrow()
            .as_ref()
            .expect("tab")
            .debug_control(name)
            .unwrap_or_else(|| panic!("control {name}"))
    };
    if let Some(c) = st["cursorCompare"].as_array() {
        *cur = centre(gfx, &*ctl(c[0].as_str().expect("name")), off(c, 1), off(c, 2));
    } else if let Some(c) = st["cursorCompareRow"].as_array() {
        let n = ctl(c[0].as_str().expect("name"));
        let (x, y) = n.get_pos(gfx);
        let (_, h) = n.get_size(gfx);
        *cur = (x + 10.0, y + h + 2.0 + (off(c, 1) - 1.0) * (h - 4.0) + ((h - 4.0) / 2.0).floor());
    } else if let Some(c) = st["compareItemButton"].as_array() {
        let tab = b.compare_tab.borrow().clone().expect("tab");
        let start = 312.0 + tab.items_col_width.get().expect("colWidth") + 10.0 - tab.c().items_h_scroll_bar.offset.get() + 310.0 + 6.0;
        let dx = match c[1].as_str() {
            Some("copy") => 94.0,
            Some("buy") => 30.0,
            _ => 158.0,
        };
        *cur = (start + dx, 236.0 + 4.0 - tab.scroll_y.get() + 20.0 * off(c, 0) + 10.0);
    } else if let Some(c) = st["cursorBuild"].as_array() {
        let n = b
            .host
            .get(c[0].as_str().expect("name"))
            .unwrap_or_else(|| panic!("build control {}", c[0]));
        *cur = centre(gfx, &*n, off(c, 1), off(c, 2));
    } else if let Some(c) = st["cursorPopup"].as_array() {
        let popups = m.popups.borrow();
        let p = popups.first().expect("popup");
        let n = p.host.get(c[0].as_str().expect("name")).expect("popup control");
        *cur = centre(gfx, &*n, off(c, 1), off(c, 2));
    } else {
        return;
    }
    gfx.input(RawEvent::Cursor { x: cur.0, y: cur.1 });
}

struct Run {
    dumps: Vec<(u64, Value)>,
    shots: Vec<(String, DrawList)>,
    perf: Vec<(String, f64)>,
}

fn run(name: &str) -> Run {
    let sc: Value =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join(format!("scenes/compare/{name}.json"))).expect("scenario"))
            .expect("json");
    let sandbox = Sandbox::new(name);
    let tp = TestPlatform::new();
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, Box::new(tp.clone())).expect("fonts");
    gfx.set_dpi_scale_override_percent(100);
    gfx.input(RawEvent::Cursor { x: -100.0, y: -100.0 });
    let root = scene::pob_root();
    let l = Launch::new(LaunchConfig {
        user_path: Some(sandbox.0.to_string_lossy().replace('\\', "/")),
        user_folder: "Path of Building".into(),
        script_path: format!("{root}/src/"),
        doc_path: format!("{root}/"),
        version_number: "?".into(),
        version_branch: Some("?".into()),
        version_platform: Some("?".into()),
    });
    l.on_init(&mut gfx);
    assert!(l.prompt_msg.borrow().is_none(), "startup error: {:?}", l.prompt_msg.borrow());
    main().background.wait();
    pob_ui::tabs::items::load_item_dbs_step(None);
    for _ in 0..4 {
        frame(&mut gfx, &l);
    }
    let mut out = Run {
        dumps: Vec::new(),
        shots: Vec::new(),
        perf: Vec::new(),
    };
    let mut cur = (-100.0, -100.0);
    for (i, st) in sc["steps"].as_array().expect("steps").iter().enumerate() {
        if st.get("cacheProbe").is_some() {
            continue;
        }
        if st["waitAssets"].as_bool() == Some(true) {
            main().build_mode.ensure_tree_view();
            main().build_mode.wait_tree_assets();
            if let Some(tab) = main().build_mode.compare_tab.borrow().as_ref() {
                for e in tab.entries.borrow().iter() {
                    e.b.ensure_tree_view();
                    e.b.wait_tree_assets();
                }
            }
            frame(&mut gfx, &l);
            continue;
        }
        if let Some(o) = st["treeOrder"].as_str() {
            // the overlap order of this oracle process (R27, R31)
            let rec = oracle_record(o).unwrap_or_else(|| panic!("fixtures/compare/{o}.json"));
            let tab = main().build_mode.compare_tab.borrow().clone().expect("compareTab");
            let entry = tab.active_compare().expect("entry");
            for (b, k) in [(main().build_mode.clone(), "primary"), (entry.b.clone(), "entry")] {
                b.ensure_tree_view();
                let r = &rec[k];
                let ids: Vec<u32> = r["nodes"]
                    .as_array()
                    .expect("nodes")
                    .iter()
                    .map(|v| v.as_f64().expect("id") as u32)
                    .collect();
                let conns: Vec<String> = r["connectors"]
                    .as_array()
                    .expect("connectors")
                    .iter()
                    .map(|v| v.as_str().expect("c").to_string())
                    .collect();
                let subs: Vec<u32> = r["subGraphs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| v.as_f64().expect("id") as u32)
                    .collect();
                let mut view = b.tree_view.borrow_mut();
                let view = view.as_mut().expect("view");
                view.set_order_sample(ids, conns);
                view.set_sub_graph_order_sample(subs);
            }
            continue;
        }
        if st["closeBuild"].as_bool() == Some(true) {
            let b = main().build_mode.clone();
            b.mod_flag.set(false);
            b.unsaved.set(false);
            main().set_mode("LIST", Vec::new());
            frame(&mut gfx, &l);
            continue;
        }
        if let Some(b) = st["open"].as_str() {
            let path = format!("{}{b}.xml", main().build_path.borrow());
            main().set_mode(
                "BUILD",
                vec![
                    pob_ui::modules::main::ModeArg::Str(path),
                    pob_ui::modules::main::ModeArg::Str(b.to_string()),
                ],
            );
            let t0 = std::time::Instant::now();
            frame(&mut gfx, &l);
            out.perf.push((format!("open {b}"), t0.elapsed().as_secs_f64() * 1e3));
            continue;
        }
        if let Some(a) = st["compareImport"].as_array() {
            let (file, label) = (a[0].as_str().expect("file"), a[1].as_str().expect("label"));
            let xml = std::fs::read_to_string(format!("{}{file}.xml", main().build_path.borrow())).expect("build");
            let tab = main().build_mode.compare_tab.borrow().clone().expect("compareTab");
            let t0 = std::time::Instant::now();
            assert!(tab.import_build(&mut gfx, &xml, label), "import failed");
            out.perf.push((format!("import {file}"), t0.elapsed().as_secs_f64() * 1e3));
            frame(&mut gfx, &l);
            continue;
        }
        if let Some(a) = st["compareSel"].as_array() {
            let tab = main().build_mode.compare_tab.borrow().clone().expect("compareTab");
            tab.debug_select(&mut gfx, a[0].as_str().expect("name"), a[1].as_i64().expect("index"));
            frame(&mut gfx, &l);
            continue;
        }
        if let Some(x) = st["compareScrollX"].as_f64() {
            let tab = main().build_mode.compare_tab.borrow().clone().expect("compareTab");
            tab.c().items_h_scroll_bar.set_offset(x);
            frame(&mut gfx, &l);
            continue;
        }
        if st["save"].as_bool() == Some(true) {
            main().build_mode.save_db_file(&mut gfx);
            frame(&mut gfx, &l);
            continue;
        }
        if let Some(n) = st["perf"].as_u64() {
            let t0 = std::time::Instant::now();
            for _ in 0..n {
                frame(&mut gfx, &l);
            }
            out.perf.push(("frame".into(), t0.elapsed().as_secs_f64() * 1e3 / n as f64));
            continue;
        }
        if let Some(c) = st["cursor"].as_array() {
            cur = (c[0].as_f64().expect("x"), c[1].as_f64().expect("y"));
            gfx.input(RawEvent::Cursor { x: cur.0, y: cur.1 });
        }
        cursor_step(&mut gfx, st, &mut cur);
        if let Some(t) = st["time"].as_u64() {
            tp.set_time(t);
        }
        if let Some(c) = st["clipboard"].as_str() {
            gfx.copy(c);
        }
        if st["dump"].as_bool() == Some(true) {
            pob_ui::sg::text_log_arm();
        }
        let t0 = std::time::Instant::now();
        for e in st["events"].as_array().into_iter().flatten() {
            let key = e[1].as_str().expect("key");
            match e[0].as_str().expect("type") {
                "KeyDown" => l.on_key_down(&mut gfx, key, e.get(2).and_then(Value::as_bool).unwrap_or(false)),
                "KeyUp" => l.on_key_up(&mut gfx, key),
                "Char" => l.on_char(&mut gfx, key),
                other => panic!("event {other}"),
            }
        }
        frame(&mut gfx, &l);
        if let Some(label) = st["perfClick"].as_str() {
            out.perf.push((label.to_string(), t0.elapsed().as_secs_f64() * 1e3));
        }
        if st["dump"].as_bool() == Some(true) {
            out.dumps.push(((i + 1) as u64, dump(&gfx, st)));
        }
        if let Some(shot) = st["shot"].as_str() {
            let list = frame(&mut gfx, &l);
            out.shots.push((shot.to_string(), list));
        }
    }
    assert!(l.prompt_msg.borrow().is_none(), "{name}: error: {:?}", l.prompt_msg.borrow());
    pob_ui::modules::main::clear_main();
    out
}

fn oracle_record(name: &str) -> Option<Value> {
    let p = scene::oracle_dir().join(format!("fixtures/compare/{name}.json"));
    Some(serde_json::from_str(&std::fs::read_to_string(p).ok()?).expect("oracle json"))
}

fn scenario_names() -> Vec<String> {
    let mut names: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join("scenes/compare/index.json")).expect("index"))
            .expect("json");
    // `probe-*`: numbers of the oracle for the report
    names.retain(|n| !n.starts_with("probe-"));
    for i in 0..local_builds("compare").len() {
        let n = format!("local-{}", i + 1);
        if scene::oracle_dir().join(format!("scenes/compare/{n}.json")).exists() && oracle_record(&n).is_some() {
            names.push(n);
        } else {
            eprintln!("SKIPPED scene {n}: no scene or fixture");
        }
    }
    names
}

/// The first difference; of two text logs (sorted `DrawString`s) the first line on each side that the other does not have.
fn first_diff(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, v) in x {
                let p = format!("{path}.{k}");
                match y.get(k) {
                    Some(w) => {
                        if let Some(d) = first_diff(v, w, &p) {
                            return Some(d);
                        }
                    }
                    None => return Some(format!("{p}: only in the oracle: {v}")),
                }
            }
            y.keys()
                .find(|k| !x.contains_key(*k))
                .map(|k| format!("{path}.{k}: only in ours: {}", y[k]))
        }
        (Value::Array(x), Value::Array(y)) => {
            if path.ends_with(".text") {
                let only_o: Vec<&Value> = x.iter().filter(|v| !y.contains(v)).collect();
                let only_r: Vec<&Value> = y.iter().filter(|v| !x.contains(v)).collect();
                if only_o.is_empty() && only_r.is_empty() && x.len() == y.len() {
                    return None;
                }
                return Some(format!(
                    "{path}: {} lines vs {}; oracle only {}: {:?}; ours only {}: {:?}",
                    x.len(),
                    y.len(),
                    only_o.len(),
                    &only_o[..only_o.len().min(6)],
                    only_r.len(),
                    &only_r[..only_r.len().min(6)]
                ));
            }
            if x.len() != y.len() {
                return Some(format!("{path}: length {} vs {}: {a} vs {b}", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (v, w))| first_diff(v, w, &format!("{path}[{i}]")))
        }
        _ => (a != b).then(|| format!("{path}: oracle {a} ours {b}")),
    }
}

fn missing_inputs() -> Option<String> {
    let idx = scene::oracle_dir().join("fixtures/compare/summary.json");
    (!idx.exists()).then(|| format!("no {}: run compare-runner.lua", idx.display()))
}

#[test]
fn compare_states_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED compare_states_equal_original: {why}");
        return;
    }
    // POB_COMPARE_ONLY=<scene>: one scene while looking at a difference
    let only = std::env::var("POB_COMPARE_ONLY").ok();
    let mut failures = Vec::new();
    let mut compared = 0;
    for name in scenario_names() {
        if only.as_ref().is_some_and(|o| *o != name) {
            continue;
        }
        let rec = oracle_record(&name).expect("record");
        let r = run(&name);
        let dumps = rec["dumps"].as_array().expect("dumps");
        assert_eq!(dumps.len(), r.dumps.len(), "{name}: dump count");
        if let Ok(dir) = std::env::var("POB_COMPARE_DUMP_OURS") {
            std::fs::create_dir_all(&dir).expect("mkdir");
            let all: Vec<Value> = r.dumps.iter().map(|(s, d)| json!({ "step": s, "state": d })).collect();
            std::fs::write(
                format!("{dir}/{name}.json"),
                serde_json::to_string(&json!({ "dumps": all })).expect("json"),
            )
            .expect("write");
        }
        for (o, (step, ours)) in dumps.iter().zip(&r.dumps) {
            assert_eq!(o["step"].as_u64(), Some(*step), "{name}: steps");
            let state = norm(&oracle_state(o));
            compared += 1;
            if let Some(d) = first_diff(&state, &norm(ours), "") {
                failures.push(format!("{name} step {step}: {d}"));
            }
        }
    }
    eprintln!("dumps compared {compared}, differ {}", failures.len());
    let notes = pob_ui::modules::main::not_ported_notes();
    if !notes.is_empty() {
        eprintln!("unported branches pressed: {notes:?}");
    }
    assert!(compared > 0, "nothing compared");
    assert!(
        failures.is_empty(),
        "{} differences, the first 25:\n{}",
        failures.len(),
        failures[..failures.len().min(8)]
            .iter()
            .map(|s| s.chars().take(520).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
#[ignore = "numbers for a report"]
fn compare_perf() {
    let r = run("probe-perf");
    for (w, ms) in r.perf {
        eprintln!("{w}: {ms:.2} ms");
    }
}

#[test]
#[ignore = "needs a wgpu adapter"]
fn compare_pixels_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED compare_pixels_equal_original: {why}");
        return;
    }
    let only = std::env::var("POB_COMPARE_ONLY").ok();
    let mut r = pob_gfx::Renderer::new_offscreen().expect("wgpu adapter");
    let lossy_sizes = lossy_sheet_sizes();
    let mut failures = Vec::new();
    let (mut shots, mut lossy_total) = (0, 0);
    for name in scenario_names() {
        if only.as_ref().is_some_and(|o| *o != name) {
            continue;
        }
        for (shot, list) in run(&name).shots {
            shots += 1;
            let rgba = r.render_to_rgba(&list).expect("render");
            let (w, h) = scene::SCREEN;
            let p = scene::oracle_dir().join(format!("fixtures/compare/{shot}.png"));
            let oracle = image::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_rgba8();
            if let Ok(dir) = std::env::var("POB_COMPARE_SAVE_OURS") {
                std::fs::create_dir_all(&dir).expect("mkdir");
                scene::save(&scene::crop(&rgba, w, w, h), &PathBuf::from(&dir).join(format!("{shot}.png")));
            }
            let mask = scene::text_mask(&list, w, h);
            let (d, heat) = scene::diff(&rgba, w, &oracle, &mask);
            eprintln!(
                "{shot:<28} bad {:>6} maxΔ {:>3} | text bad {:>4} maxΔ {:>3} | bbox {:?}",
                d.bad, d.max_delta, d.text_bad, d.text_max_delta, d.bbox
            );
            if d.bad == 0 {
                continue;
            }
            let (lossy, other) = classify_bad(&rgba, &oracle, &list, &lossy_sizes);
            lossy_total += lossy;
            if other == 0 && d.text_bad == 0 {
                continue;
            }
            let out = scene::out_dir();
            scene::save(&heat, &out.join(format!("{shot}.heat.png")));
            scene::save(&scene::crop(&rgba, w, w, h), &out.join(format!("{shot}.ours.png")));
            failures.push(format!(
                "{shot}: {} px off ({other} outside the lossy class, text {}), bbox {:?}",
                d.bad, d.text_bad, d.bbox
            ));
        }
    }
    eprintln!("shots {shots}, pixels in the lossy class {lossy_total}");
    assert!(failures.is_empty(), "the tab differs from the original:\n{}", failures.join("\n"));
}

/// The sizes of the lossy sheets (jpg/webp, tests/tree.rs).
fn lossy_sheet_sizes() -> std::collections::HashSet<(u32, u32)> {
    let root = PathBuf::from(scene::pob_root()).join("src/TreeData");
    let mut out = std::collections::HashSet::new();
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "jpg" || x == "webp")
                && let Ok(s) = image::image_dimensions(&p)
            {
                out.insert(s);
            }
        }
    }
    out
}

const LOSSY_MAX_DELTA: u8 = 4;

/// tests/tree.rs `classify_bad`: a pixel off by at most 4 under a jpg/webp sheet is the class of the decoders.
fn classify_bad(
    rgba: &[u8],
    oracle: &image::RgbaImage,
    list: &DrawList,
    lossy_sizes: &std::collections::HashSet<(u32, u32)>,
) -> (usize, usize) {
    let (w, h) = scene::SCREEN;
    let mut under = vec![false; (w * h) as usize];
    for c in &list.commands {
        if lossy_sizes.contains(&(c.texture.width, c.texture.height)) {
            let (x0, x1) = c.pos.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[0]), a.1.max(p[0])));
            let (y0, y1) = c.pos.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[1]), a.1.max(p[1])));
            for y in (y0.max(0.0) as u32)..(y1.max(0.0).ceil() as u32).min(h) {
                for x in (x0.max(0.0) as u32)..(x1.max(0.0).ceil() as u32).min(w) {
                    under[(y * w + x) as usize] = true;
                }
            }
        }
    }
    let (mut lossy, mut other) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let o = oracle.get_pixel(x, y).0;
            let d = (0..3).map(|k| rgba[i + k].abs_diff(o[k])).max().unwrap_or(0);
            if d > 2 {
                if under[(y * w + x) as usize] && d <= LOSSY_MAX_DELTA {
                    lossy += 1;
                } else {
                    other += 1;
                }
            }
        }
    }
    (lossy, other)
}
