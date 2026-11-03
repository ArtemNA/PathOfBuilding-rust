//! U8a gate: the original `ImportTab` / `PartyTab` (the whole PoB, run by the SG oracle `tools/sg-oracle/lua/import-runner.lua`) vs this port, on the public goldens
//! (`tools/sg-oracle/scenes/import/*.json`).
//! - `import_states_equal_original` (no GPU): every `dump` step = the oracle's; a saved build = the oracle's file. A generated code is compared by its XML (the two zlib streams differ).
//! - `import_pixels_equal_original` (GPU/WARP, `#[ignore]`): every screenshot = the original's PNG; 0 pixels beyond 2/255 outside the lossy class.
//!   cargo test -p pob-ui --release --test import -- --include-ignored --nocapture

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::{Button, DrawList, Gfx, RawEvent, TestPlatform};
use pob_ui::control::Control;
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::{ModeArg, main};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

const GOLDENS: [&str; 5] = [
    "Generals Perforate Zerker",
    "Dual Savior",
    "Dual Wield Cospris CoC",
    "Mirage Archer Toxic Rain",
    "OccVortex",
];

const BUILDS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../builds/pc/3.29/");
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../pob-calc/tests/fixtures/party-synth/party-attack-full.xml"
);

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
        // dkjson writes an empty table as an array
        Value::Array(a) if a.is_empty() => Value::Object(serde_json::Map::new()),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        _ => v.clone(),
    }
}

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("pob-ui-u8a-{}-{n}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let builds = root.join("Path of Building/Builds");
        std::fs::create_dir_all(&builds).expect("mkdir");
        for b in GOLDENS {
            std::fs::copy(
                format!("{}/spec/TestBuilds/3.13/{b}.xml", scene::pob_root()),
                builds.join(format!("{b}.xml")),
            )
            .expect("copy golden");
        }
        std::fs::copy(FIXTURE, builds.join("party-attack-full.xml")).expect("copy fixture");
        for b in local_builds("import") {
            let src = format!("{BUILDS}{b}.xml");
            let src = if std::path::Path::new(&src).exists() {
                PathBuf::from(src)
            } else {
                scene::oracle_dir().join(format!("fixtures/import/local-builds/{b}.xml"))
            };
            std::fs::copy(&src, builds.join(format!("{b}.xml"))).expect("copy build");
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

fn xml_of(code: &str) -> String {
    match pob_build::decode_build_code(code) {
        Some(x) => String::from_utf8_lossy(&x).replace("\r\n", "\n"),
        None => code.to_string(),
    }
}

fn dump(g: &Gfx, tp: &TestPlatform) -> Value {
    let m = main();
    let b = m.build_mode.clone();
    let mut d = serde_json::Map::new();
    d.insert("mode".into(), json!(m.mode.borrow().clone().unwrap_or_else(|| "nil".into())));
    d.insert("popups".into(), json!(m.popups.borrow().len()));
    if let Some(p) = m.popups.borrow().first() {
        d.insert("popupTitle".into(), json!(p.title.get(g)));
    }
    let prompt = pob_ui::launch::launch().prompt_msg.borrow().clone().unwrap_or_else(|| "nil".into());
    d.insert("prompt".into(), json!(prompt));
    d.insert(
        "clipboard".into(),
        json!(tp.state().clipboard.map_or_else(|| "nil".to_string(), |c| xml_of(&c))),
    );
    if let Some(import) = b.import_tab.borrow().clone() {
        let party = b.party_tab.borrow().clone().expect("partyTab");
        let c = b.c();
        let lines: Vec<String> = c
            .stat_box
            .list
            .borrow()
            .iter()
            .map(|l| {
                let u = if l.underline.is_empty() {
                    "-".to_string()
                } else {
                    l.underline.iter().map(|b| if *b { "1" } else { "0" }).collect()
                };
                let col = |i: usize| l.cols.get(i).cloned().flatten().unwrap_or("nil".into());
                let n = |x: f64| pob_lua::tostring_num(x);
                format!(
                    "{}|{}|{}|{}|{}|{}",
                    n(l.height),
                    l.align.unwrap_or("-"),
                    l.x.map_or("-".into(), n),
                    col(0),
                    col(1),
                    u
                )
            })
            .collect();
        let sel_import = import
            .host
            .sel()
            .is_some_and(|s| pob_ui::control::same(&*s, &*import.import_code_in));
        d.insert(
            "build".into(),
            json!({
                "buildName": b.build_name.borrow().clone().unwrap_or_else(|| "nil".into()),
                "dbFile": b.db_file_name.borrow().path().is_some(),
                "unsaved": b.unsaved.get(),
                "viewMode": *b.view_mode.borrow(),
                "characterLevel": b.engine.borrow().as_ref().expect("engine").model().level,
                "statBox": lines,
                "importSel": sel_import,
            }),
        );
        d.insert("import".into(), import.debug_dump(g));
        d.insert("party".into(), party.debug_dump(g));
    }
    Value::Object(d)
}

fn center(g: &Gfx, c: &dyn Control) -> (f64, f64) {
    let (x, y) = c.get_pos(g);
    let (w, h) = c.get_size(g);
    ((x + w / 2.0).floor(), (y + h / 2.0).floor())
}

fn cursor_step(gfx: &mut Gfx, st: &Value, cur: &mut (f64, f64)) {
    let b = main().build_mode.clone();
    let import = b.import_tab.borrow().clone();
    let party = b.party_tab.borrow().clone();
    let tab_control = |tab: &str, name: &str| -> pob_ui::control::ControlRef {
        if tab == "import" {
            import.as_ref().expect("importTab").host.get(name)
        } else {
            party.as_ref().expect("partyTab").host.get(name)
        }
        .unwrap_or_else(|| panic!("control {name}"))
    };
    if let Some(s) = st["cursorBuild"].as_str() {
        let c = b.host.get(s).unwrap_or_else(|| panic!("control {s}"));
        *cur = center(gfx, &*c);
    } else if let Some(s) = st["cursorImport"].as_str() {
        *cur = center(gfx, &*tab_control("import", s));
    } else if let Some(s) = st["cursorParty"].as_str() {
        *cur = center(gfx, &*tab_control("party", s));
    } else if let Some(s) = st["cursorPopup"].as_str() {
        let p = main().popups.borrow().first().cloned().expect("popup");
        let c = p.host.get(s).unwrap_or_else(|| panic!("popup control {s}"));
        *cur = center(gfx, &*c);
    } else if let Some(c) = st["cursorDrop"].as_array() {
        let ctl = tab_control(c[0].as_str().expect("tab"), c[1].as_str().expect("control"));
        let (x, y) = ctl.get_pos(gfx);
        let (w, h) = ctl.get_size(gfx);
        let row = c[2].as_f64().expect("row");
        *cur = ((x + w / 2.0).floor(), y + h + 2.0 + (row - 1.0) * (h - 4.0) + 4.0);
    } else {
        return;
    }
    gfx.input(RawEvent::Cursor { x: cur.0, y: cur.1 });
}

struct Run {
    dumps: Vec<(u64, Value)>,
    shots: Vec<(String, DrawList)>,
    perf: Vec<(String, f64)>,
    files: HashMap<String, String>,
}

/// R13: `pairs(enemyModList)` (PartyTab.lua:752) is of the process: the group names of the longest label the oracle's record holds.
fn enemy_mods_order(name: &str) -> Option<Vec<String>> {
    let rec = oracle_record(name)?;
    let mut best: Vec<String> = Vec::new();
    for d in rec["dumps"].as_array().into_iter().flatten() {
        let Some(label) = d["state"]["party"]["controls"]["simpleEnemyMods"]["value"].as_str() else {
            continue;
        };
        let names: Vec<String> = label
            .lines()
            .filter_map(|l| l.split_once(':').map(|(n, _)| n.to_string()))
            .collect();
        if names.len() > best.len() {
            best = names;
        }
    }
    Some(best)
}

fn run(name: &str) -> Run {
    pob_ui::tabs::party::set_enemy_mods_order_sample(enemy_mods_order(name));
    let sc: Value =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join(format!("scenes/import/{name}.json"))).expect("scenario"))
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
        files: HashMap::new(),
    };
    let mut cur = (-100.0, -100.0);
    let mut perf_xml: Option<String> = None;
    for (i, st) in sc["steps"].as_array().expect("steps").iter().enumerate() {
        if st["waitAssets"].as_bool() == Some(true) {
            main().build_mode.wait_tree_assets();
            frame(&mut gfx, &l);
            continue;
        }
        if st["openNew"].as_bool() == Some(true) {
            main().set_mode("BUILD", vec![ModeArg::Bool(false), ModeArg::Str("Unnamed build".into())]);
            frame(&mut gfx, &l);
            continue;
        }
        if let Some(b) = st["open"].as_str() {
            let path = format!("{}{b}.xml", main().build_path.borrow());
            main().set_mode("BUILD", vec![ModeArg::Str(path), ModeArg::Str(b.to_string())]);
            let t0 = std::time::Instant::now();
            frame(&mut gfx, &l);
            out.perf.push((format!("open {b}"), t0.elapsed().as_secs_f64() * 1e3));
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
        if let Some(n) = st["perfGenerate"].as_u64() {
            let b = main().build_mode.clone();
            let t0 = std::time::Instant::now();
            for _ in 0..n {
                let xml = b.save_db_text().expect("save");
                std::hint::black_box(pob_build::encode_build_code(xml.as_bytes()));
            }
            out.perf.push(("generate".into(), t0.elapsed().as_secs_f64() * 1e3 / n as f64));
            continue;
        }
        if st["perfImport"].as_bool() == Some(true) {
            use pob_ui::modules::main::Mode;
            let b = main().build_mode.clone();
            // `Init` makes a new tab with an empty field: the text is the one of the first time
            if perf_xml.is_none() {
                let code = b
                    .import_tab
                    .borrow()
                    .as_ref()
                    .expect("importTab")
                    .import_code_in
                    .buf
                    .borrow()
                    .clone();
                perf_xml = Some(String::from_utf8_lossy(&pob_build::decode_build_code(&code).expect("code")).into_owned());
            }
            let xml = perf_xml.clone().expect("xml");
            let t0 = std::time::Instant::now();
            b.shutdown(&mut gfx);
            b.init(
                &mut gfx,
                &[
                    ModeArg::Bool(false),
                    ModeArg::Str("Imported build".into()),
                    ModeArg::Str(xml),
                    ModeArg::Bool(false),
                ],
            );
            *b.view_mode.borrow_mut() = "IMPORT".into();
            frame(&mut gfx, &l);
            out.perf.push(("import".into(), t0.elapsed().as_secs_f64() * 1e3));
            continue;
        }
        if !st["token"].is_null() {
            main().accounts.borrow_mut().last_token = st["token"].as_str().map(str::to_owned);
        }
        if let Some(d) = st["devMode"].as_bool() {
            l.dev_mode.set(d);
        }
        if let Some(m) = st["importMode"].as_str() {
            main().build_mode.import_tab.borrow().as_ref().expect("importTab").debug_set_mode(m);
        }
        if let Some(t) = st["setGenerated"].as_str() {
            let tab = main().build_mode.import_tab.borrow().clone().expect("importTab");
            tab.debug_set_generated(&mut gfx, t);
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
        let hold = |gfx: &mut Gfx, k: &Value, pressed: bool| match k.as_str() {
            Some("LEFTBUTTON") => gfx.input(RawEvent::Button {
                button: Button::Left,
                pressed,
                pos: None,
            }),
            Some("CTRL") => gfx.input(RawEvent::Key {
                key: pob_gfx::input::PhysKey::Ctrl,
                pressed,
            }),
            other => panic!("hold of {other:?}"),
        };
        for k in st["hold"].as_array().into_iter().flatten() {
            hold(&mut gfx, k, true);
        }
        for k in st["release"].as_array().into_iter().flatten() {
            hold(&mut gfx, k, false);
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
            out.dumps.push(((i + 1) as u64, dump(&gfx, &tp)));
        }
        if let (Some(file), Some(file_out)) = (st["file"].as_str(), st["fileOut"].as_str()) {
            let p = format!("{}{file}", main().build_path.borrow());
            out.files.insert(
                file_out.to_string(),
                std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}")),
            );
        }
        if let Some(shot) = st["shot"].as_str() {
            let list = frame(&mut gfx, &l);
            out.shots.push((shot.to_string(), list));
        }
    }
    pob_ui::modules::main::clear_main();
    out
}

fn oracle_record(name: &str) -> Option<Value> {
    let p = scene::oracle_dir().join(format!("fixtures/import/{name}.json"));
    Some(serde_json::from_str(&std::fs::read_to_string(p).ok()?).expect("oracle json"))
}

fn scenario_names() -> Vec<String> {
    let mut names: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join("scenes/import/index.json")).expect("index")).expect("json");
    // POB_IMPORT_SCENES=a,b: a look at these scenes alone
    if let Ok(only) = std::env::var("POB_IMPORT_SCENES") {
        return only.split(',').map(str::to_owned).collect();
    }
    // personal builds: compared when the local list, the scenes and the fixtures are there
    for (i, _) in local_builds("import").iter().enumerate() {
        let n = format!("local-{}", i + 1);
        if scene::oracle_dir().join(format!("scenes/import/{n}.json")).exists() && oracle_record(&n).is_some() {
            names.push(n);
        } else {
            eprintln!("SKIPPED scene {n}: no scene or fixture");
        }
    }
    names
}

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
                    None if v.is_null() => {}
                    None => return Some(format!("{p}: oracle {v}, ours missing")),
                }
            }
            for (k, w) in y {
                if !x.contains_key(k) && !w.is_null() {
                    return Some(format!("{path}.{k}: oracle missing, ours {w}"));
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{path}: length oracle {} ours {}: oracle {a}, ours {b}", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (v, w))| first_diff(v, w, &format!("{path}[{i}]")))
        }
        (Value::String(x), Value::String(y)) if x != y => {
            let at = x.bytes().zip(y.bytes()).position(|(p, q)| p != q).unwrap_or(x.len().min(y.len()));
            let cut = |s: &str| s[s.floor_char_boundary(at.saturating_sub(60))..s.ceil_char_boundary((at + 60).min(s.len()))].to_string();
            Some(format!(
                "{path}: strings differ at byte {at} of {}/{}: oracle {:?}, ours {:?}",
                x.len(),
                y.len(),
                cut(x),
                cut(y)
            ))
        }
        _ if a == b => None,
        _ => Some(format!("{path}: oracle {a}, ours {b}")),
    }
}

/// The field of a generated code: its text differs with the zlib stream, `generated` holds the XML.
fn without_code(mut v: Value) -> Value {
    if let Some(o) = v["import"]["controls"]["generateCodeOut"].as_object_mut() {
        o.remove("value");
    }
    v
}

fn missing_inputs() -> Option<String> {
    scenario_names()
        .iter()
        .find(|n| oracle_record(n).is_none())
        .map(|n| format!("fixtures/import/{n}.json is absent (tools/sg-oracle/run.ps1 -Script import-runner.lua)"))
}

#[test]
fn import_states_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED import_states_equal_original: {why}");
        return;
    }
    let mut failures = Vec::new();
    let mut compared = 0;
    for name in scenario_names() {
        let rec = oracle_record(&name).expect("record");
        let r = run(&name);
        let dumps = rec["dumps"].as_array().expect("dumps");
        assert_eq!(dumps.len(), r.dumps.len(), "{name}: dump count");
        for (o, (step, ours)) in dumps.iter().zip(&r.dumps) {
            assert_eq!(o["step"].as_u64(), Some(*step), "{name}: steps");
            let mut s = o["state"].clone();
            assert!(s.is_object(), "{name} step {step}: {s}");
            if let Some(m) = s.as_object_mut() {
                m.remove("writesRefused");
                m.remove("file");
            }
            compared += 1;
            if let Some(d) = first_diff(&without_code(norm(&s)), &without_code(norm(ours)), "") {
                failures.push(format!("{name} step {step}: {d}"));
            }
        }
        for (file, text) in &r.files {
            let p = scene::oracle_dir().join(format!("fixtures/import/{file}"));
            let oracle = std::fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
                .replace("\r\n", "\n");
            if oracle != *text {
                let at = oracle.lines().zip(text.lines()).position(|(a, b)| a != b);
                let line = at.map(|i| (oracle.lines().nth(i).map(str::to_owned), text.lines().nth(i).map(str::to_owned)));
                failures.push(format!(
                    "{name}: saved {file} differs at line {at:?}, {} bytes vs {}: {line:?}",
                    oracle.len(),
                    text.len()
                ));
            }
        }
    }
    eprintln!("dumps compared {compared}, differ {}", failures.len());
    assert!(
        failures.is_empty(),
        "{} differences, the first 300:\n{}",
        failures.len(),
        failures[..failures.len().min(300)].join("\n")
    );
    assert!(compared > 0, "nothing compared");
}

/// The real window on both sides (import-real-seq.ps1): the stages' state and the events the window delivered. Run the sides first:
///   pwsh tools/sg-oracle/run.ps1 -Script import-runner.lua -ScriptArgs real-input -Inject inject-import-real.ps1 -Timeout 40
///   pwsh tools/sg-oracle/import-rust-input.ps1
#[test]
fn import_real_input_equals_original() {
    let read = |n: &str| -> Option<Value> {
        let t = std::fs::read_to_string(scene::oracle_dir().join(format!("fixtures/import/{n}.json"))).ok()?;
        Some(serde_json::from_str(t.trim_start_matches('\u{feff}')).expect("json"))
    };
    let (Some(o), Some(r)) = (read("real-input-oracle"), read("real-input-rust")) else {
        eprintln!("SKIPPED import_real_input_equals_original: no fixtures/import/real-input-*.json");
        return;
    };
    let mut events = 0;
    for stage in ["start", "tab", "generate", "typed", "party", "advanced", "auras"] {
        if let Some(d) = first_diff(&norm(&o[stage]["importReal"]), &norm(&r[stage]["importReal"]), stage) {
            panic!("{d}");
        }
        assert_eq!(o[stage]["events"], r[stage]["events"], "{stage}: events");
        events = o[stage]["events"].as_array().map_or(0, Vec::len);
    }
    assert_eq!(o["typed"]["importReal"]["accountBuf"].as_str(), Some("12"), "the digits arrived");
    assert_eq!(o["generate"]["importReal"]["hasCode"].as_bool(), Some(true), "the code came");
    assert_eq!(o["auras"]["importReal"]["aurasBuf"].as_str(), Some("3"), "the buffer took the key");
    eprintln!("7 stages, {events} events");
}

/// Rust vs the original, roughly (a busy machine): `perf` scene, tools/sg-oracle/run.ps1 -Script import-runner.lua -ScriptArgs perf.
#[test]
#[ignore = "numbers for a report"]
fn import_perf() {
    let Some(rec) = oracle_record("perf") else {
        eprintln!("SKIPPED import_perf: no fixtures/import/perf.json");
        return;
    };
    eprintln!("oracle: {}", rec["perf"]);
    eprintln!("rust:   {:?}", run("perf").perf);
}

/// Two screenshots of this port for the report (`tools/.scratch/u8a-shots/`): the Import and Party tabs on a public golden.
#[test]
#[ignore = "needs a wgpu adapter"]
fn import_report_shots() {
    let mut r = pob_gfx::Renderer::new_offscreen().expect("wgpu adapter");
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/.scratch/u8a-shots"));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let (w, h) = scene::SCREEN;
    for (scenario, shot, file) in [("code", "code-pasted", "import.png"), ("party", "party-imported", "party.png")] {
        for (s, list) in run(scenario).shots {
            if s == shot {
                let rgba = r.render_to_rgba(&list).expect("render");
                scene::save(&scene::crop(&rgba, w, w, h), &dir.join(file));
            }
        }
    }
}

#[test]
#[ignore = "needs a wgpu adapter"]
fn import_pixels_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED import_pixels_equal_original: {why}");
        return;
    }
    let mut r = pob_gfx::Renderer::new_offscreen().expect("wgpu adapter");
    let mut failures = Vec::new();
    let mut shots = 0;
    for name in scenario_names() {
        for (shot, list) in run(&name).shots {
            shots += 1;
            let rgba = r.render_to_rgba(&list).expect("render");
            let (w, h) = scene::SCREEN;
            let p = scene::oracle_dir().join(format!("fixtures/import/{shot}.png"));
            let oracle = image::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_rgba8();
            let mask = scene::text_mask(&list, w, h);
            let (d, heat) = scene::diff(&rgba, w, &oracle, &mask);
            eprintln!(
                "{shot:<28} bad {:>6} maxΔ {:>3} | text bad {:>4} maxΔ {:>3} | bbox {:?}",
                d.bad, d.max_delta, d.text_bad, d.text_max_delta, d.bbox
            );
            if d.bad == 0 {
                continue;
            }
            let out = scene::out_dir();
            scene::save(&heat, &out.join(format!("{shot}.heat.png")));
            scene::save(&scene::crop(&rgba, w, w, h), &out.join(format!("{shot}.ours.png")));
            failures.push(format!("{shot}: {} px off (text {}), bbox {:?}", d.bad, d.text_bad, d.bbox));
        }
    }
    eprintln!("shots {shots}");
    assert!(shots > 0, "nothing compared");
    assert!(failures.is_empty(), "the tabs differ from the original:\n{}", failures.join("\n"));
}
