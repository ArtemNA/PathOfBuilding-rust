//! U2c gate: the original `Modules/Build.lua` (the whole PoB, run by the SG oracle `tools/sg-oracle/lua/build-runner.lua`) vs this
//! port's BUILD shell, on the same test build folder (5 builds of `builds/pc/3.29` + the golden "Generals Perforate Zerker") and the
//! same scenarios (`tools/sg-oracle/scenes/build/*.json`).
//! - `build_states_equal_original` (no GPU): every `dump` step = the oracle's record `fixtures/build/<scenario>.json` (top bar,
//!   class / loadout drop-downs, main-skill controls, the stat box lines, warnings, control rectangles, popups, saved files).
//! - `build_pixels_equal_original` (GPU/WARP, `#[ignore]`): every screenshot = the original's PNG over the shell — the top bar
//!   (`y < 32`) and the side bar (`x < 312`); the tab area (TREE … COMPARE: U3–U8) is masked out. 0 pixels beyond 2/255.
//!   cargo test -p pob-ui --release --test build -- --include-ignored --nocapture
//!
//! The test build folder is a temporary directory, never the user's documents.

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::input::PhysKey;
use pob_gfx::{Button, DrawList, Gfx, RawEvent, TestPlatform};
use pob_ui::ControlRef;
use pob_ui::controls::{DropDownControl, DropItem};
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::main;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

const BUILDS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../builds/pc/3.29/");
/// A list of personal builds from the local `builds/local-builds.json` (not in git); empty without it.
fn local_builds(key: &str) -> Vec<String> {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../builds/local-builds.json");
    let v: Option<serde_json::Value> = std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok());
    v.and_then(|v| {
        v[key]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
    })
    .unwrap_or_default()
}
const GOLDEN: &str = "Generals Perforate Zerker";

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

fn pob_root() -> String {
    scene::pob_root()
}

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        // unique per call: the state and the pixel tests run the same scenario in parallel threads
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("pob-ui-u2c-{}-{n}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let builds = root.join("Path of Building/Builds");
        std::fs::create_dir_all(&builds).expect("mkdir");
        for b in local_builds("build") {
            std::fs::copy(format!("{BUILDS}{b}.xml"), builds.join(format!("{b}.xml"))).expect("copy build");
        }
        std::fs::copy(
            format!("{}/spec/TestBuilds/3.13/{GOLDEN}.xml", pob_root()),
            builds.join(format!("{GOLDEN}.xml")),
        )
        .expect("copy golden");
        std::fs::copy(
            scene::oracle_dir().join("scenes/build/Settings.xml"),
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

fn name_of(controls: &indexmap::IndexMap<String, ControlRef>, c: Option<ControlRef>) -> Value {
    let Some(c) = c else { return json!("nil") };
    for (k, v) in controls {
        if pob_ui::control::same(&**v, &*c) {
            return json!(k);
        }
    }
    json!("other")
}

fn label_of(v: &DropItem) -> String {
    v.label.clone().unwrap_or_else(|| "nil".into())
}

fn drop_dump(g: &Gfx, d: &DropDownControl) -> Value {
    use pob_ui::Control;
    json!({
        "shown": d.is_shown(g),
        "enabled": d.is_enabled(g),
        "sel": d.sel_index.get(),
        "list": d.list.borrow().iter().map(label_of).collect::<Vec<_>>(),
    })
}

/// Lua `%g` of the integral rectangles the controls have.
fn g_fmt(v: f64) -> String {
    pob_lua::tostring_num(v)
}

fn build_dump(g: &Gfx) -> Value {
    use pob_ui::Control;
    let b = main().build_mode.clone();
    let c = b.c();
    let eng = b.engine.borrow();
    let e = eng.as_ref().expect("engine");
    let mut d = Map::new();
    d.insert("buildName".into(), json!(b.build_name.borrow().clone().unwrap_or("nil".into())));
    d.insert(
        "dbFileSubPath".into(),
        json!(b.db_file_sub_path.borrow().clone().unwrap_or("nil".into())),
    );
    d.insert("unsaved".into(), json!(b.unsaved.get()));
    d.insert("viewMode".into(), json!(b.view_mode.borrow().clone()));
    d.insert("characterLevel".into(), json!(e.model().level));
    d.insert("levelBuf".into(), json!(c.character_level.buf.borrow().clone()));
    d.insert("levelMode".into(), json!(c.level_scaling_button.label.get(g)));
    d.insert(
        "points".into(),
        json!(c.point_display.str_.borrow().clone().unwrap_or("nil".into())),
    );
    d.insert("pointsReq".into(), json!(c.point_display.req.borrow().clone()));
    d.insert("mainSocketGroup".into(), json!(e.main_socket_group()));
    drop(eng);
    for (k, dd) in [
        ("classDrop", &c.class_drop),
        ("ascendDrop", &c.ascend_drop),
        ("secondaryAscendDrop", &c.secondary_ascend_drop),
        ("buildLoadouts", &c.build_loadouts),
        ("mainSocketGroup", &c.main_socket_group),
        ("mainSkill", &c.main_skill),
        ("mainSkillPart", &c.main_skill_part),
        ("mainSkillMinion", &c.main_skill_minion),
        ("mainSkillMinionSkill", &c.main_skill_minion_skill),
    ] {
        d.insert(k.into(), drop_dump(g, dd));
    }
    d.insert(
        "stageCount".into(),
        json!(if c.main_skill_stage_count.is_shown(g) {
            c.main_skill_stage_count.buf.borrow().clone()
        } else {
            "hidden".into()
        }),
    );
    d.insert(
        "mineCount".into(),
        json!(if c.main_skill_mine_count.is_shown(g) {
            c.main_skill_mine_count.buf.borrow().clone()
        } else {
            "hidden".into()
        }),
    );
    d.insert("minionLibrary".into(), json!(c.main_skill_minion_library.is_shown(g)));
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
            format!(
                "{}|{}|{}|{}|{}|{}",
                g_fmt(l.height),
                l.align.unwrap_or("-"),
                l.x.map_or("-".into(), g_fmt),
                col(0),
                col(1),
                u
            )
        })
        .collect();
    d.insert("statBox".into(), json!(lines));
    d.insert("warnings".into(), json!(b.warning_lines.borrow().clone()));
    d.insert("sel".into(), name_of(&b.host.controls.borrow(), b.host.sel()));
    let mut rects = Map::new();
    let ctl = |k: &str| -> ControlRef { b.host.get(k).unwrap_or_else(|| panic!("control {k}")) };
    for k in [
        "back",
        "save",
        "saveAs",
        "buildName",
        "pointDisplay",
        "levelScalingButton",
        "characterLevel",
        "classDrop",
        "ascendDrop",
        "secondaryAscendDrop",
        "buildLoadouts",
        "mainSocketGroup",
        "mainSkill",
        "mainSkillPart",
        "mainSkillStageCount",
        "mainSkillMinion",
        "statBox",
        "warnings",
    ] {
        let c = ctl(k);
        let (x, y) = c.get_pos(g);
        let (w, h) = c.get_size(g);
        rects.insert(k.into(), json!(format!("{},{},{},{}", g_fmt(x), g_fmt(y), g_fmt(w), g_fmt(h))));
    }
    d.insert("rects".into(), Value::Object(rects));
    Value::Object(d)
}

fn dump(g: &Gfx, tp: &TestPlatform, st: &Value, out_files: &mut HashMap<String, String>) -> Value {
    let m = main();
    let l = pob_ui::launch::launch();
    let mut d = Map::new();
    d.insert("mode".into(), json!(m.mode.borrow().clone().unwrap_or("nil".into())));
    d.insert("popups".into(), json!(m.popups.borrow().len()));
    d.insert("prompt".into(), json!(l.prompt_msg.borrow().clone().unwrap_or("nil".into())));
    d.insert("clipboard".into(), json!(tp.state().clipboard.clone().unwrap_or("nil".into())));
    let top = m.popups.borrow().first().cloned();
    if let Some(p) = top {
        d.insert("popupTitle".into(), json!(p.title.get(g)));
        d.insert("popupSel".into(), name_of(&p.host.controls.borrow(), p.host.sel()));
        let edit = p.host.get("edit").map(|e| {
            let any: Rc<dyn std::any::Any> = e;
            any.downcast::<pob_ui::controls::EditControl>().expect("edit").buf.borrow().clone()
        });
        d.insert("popupEdit".into(), json!(edit.unwrap_or("nil".into())));
    }
    if m.mode.borrow().as_deref() == Some("BUILD") && m.build_mode.c.borrow().is_some() {
        d.insert("build".into(), build_dump(g));
    }
    if let Some(file) = st["file"].as_str() {
        let p = format!("{}{file}", m.build_path.borrow());
        match std::fs::read_to_string(&p) {
            Ok(t) => {
                d.insert("file".into(), json!("saved"));
                out_files.insert(st["fileOut"].as_str().expect("fileOut").to_string(), t);
            }
            Err(_) => {
                d.insert("file".into(), json!("missing"));
            }
        }
    }
    Value::Object(d)
}

struct Run {
    dumps: Vec<(u64, Value)>,
    /// `(name, frame, the top popup's rectangle)`.
    shots: Vec<(String, DrawList, Option<[f64; 4]>)>,
    files: HashMap<String, String>,
    perf: Vec<(String, f64)>,
}

fn held_event(name: &str) -> RawEvent {
    let key = match name {
        "CTRL" => PhysKey::Ctrl,
        "SHIFT" => PhysKey::Shift,
        "ALT" => PhysKey::Alt,
        "LEFTBUTTON" => {
            return RawEvent::Button {
                button: Button::Left,
                pressed: true,
                pos: None,
            };
        }
        other => panic!("hold of {other} not supported"),
    };
    RawEvent::Key { key, pressed: true }
}

fn release(ev: RawEvent) -> RawEvent {
    match ev {
        RawEvent::Key { key, .. } => RawEvent::Key { key, pressed: false },
        RawEvent::Button { button, pos, .. } => RawEvent::Button {
            button,
            pressed: false,
            pos,
        },
        e => e,
    }
}

fn frame(gfx: &mut Gfx, l: &Rc<Launch>) -> DrawList {
    let mut f = gfx.begin_frame();
    l.on_frame(&mut f);
    f.finish()
}

/// Runs `scenes/build/<name>.json` like `build-runner.lua`.
fn run(name: &str) -> Run {
    let sc: Value =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join(format!("scenes/build/{name}.json"))).expect("scenario"))
            .expect("json");
    let sandbox = Sandbox::new(name);
    let tp = TestPlatform::new();
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, Box::new(tp.clone())).expect("fonts");
    gfx.set_dpi_scale_override_percent(100);
    gfx.input(RawEvent::Cursor { x: -100.0, y: -100.0 });
    let root = pob_root();
    let l = Launch::new(LaunchConfig {
        user_path: Some(sandbox.0.to_string_lossy().replace('\\', "/")),
        user_folder: "Path of Building".into(),
        script_path: scene::script_path(),
        doc_path: format!("{root}/"),
        version_number: "?".into(),
        version_branch: Some("?".into()),
        version_platform: Some("?".into()),
        app_version: None,
    });
    l.on_init(&mut gfx);
    assert!(l.prompt_msg.borrow().is_none(), "startup error: {:?}", l.prompt_msg.borrow());
    main().background.wait();
    // the runners wait for `main.uniqueDB.loading` and `main.rareDB.loading` too
    pob_ui::tabs::items::load_item_dbs_step(None);
    for _ in 0..4 {
        frame(&mut gfx, &l);
    }
    let mut out = Run {
        dumps: Vec::new(),
        shots: Vec::new(),
        files: HashMap::new(),
        perf: Vec::new(),
    };
    let mut held: HashMap<String, RawEvent> = HashMap::new();
    for (i, st) in sc["steps"].as_array().expect("steps").iter().enumerate() {
        if let Some(n) = st["perf"].as_u64() {
            let t0 = std::time::Instant::now();
            for _ in 0..n {
                frame(&mut gfx, &l);
            }
            out.perf.push(("frame".into(), t0.elapsed().as_secs_f64() * 1e3 / n as f64));
            continue;
        }
        if let Some(n) = st["perfRecalc"].as_u64() {
            let b = main().build_mode.clone();
            let t0 = std::time::Instant::now();
            for _ in 0..n {
                b.engine.borrow_mut().as_mut().expect("engine").refresh().expect("refresh");
            }
            out.perf.push(("recalc".into(), t0.elapsed().as_secs_f64() * 1e3 / n as f64));
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
        if let Some(c) = st["cursor"].as_array() {
            gfx.input(RawEvent::Cursor {
                x: c[0].as_f64().expect("x"),
                y: c[1].as_f64().expect("y"),
            });
        }
        if let Some(t) = st["time"].as_u64() {
            tp.set_time(t);
        }
        for k in st["hold"].as_array().into_iter().flatten() {
            let ev = held_event(k.as_str().expect("key"));
            gfx.input(ev);
            held.insert(k.as_str().expect("key").to_string(), ev);
        }
        for k in st["release"].as_array().into_iter().flatten() {
            if let Some(ev) = held.remove(k.as_str().expect("key")) {
                gfx.input(release(ev));
            }
        }
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
        if st["dump"].as_bool() == Some(true) {
            out.dumps.push(((i + 1) as u64, dump(&gfx, &tp, st, &mut out.files)));
        }
        if let Some(shot) = st["shot"].as_str() {
            let list = frame(&mut gfx, &l);
            let popup = main().popups.borrow().first().map(|p| {
                use pob_ui::Control;
                let (x, y) = p.get_pos(&gfx);
                let (w, h) = p.get_size(&gfx);
                [x, y, w, h]
            });
            out.shots.push((shot.to_string(), list, popup));
        }
    }
    pob_ui::modules::main::clear_main();
    out
}

fn oracle_record(name: &str) -> Value {
    let p = scene::oracle_dir().join(format!("fixtures/build/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("json")
}

fn scenario_names() -> Vec<String> {
    serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join("scenes/build/index.json")).expect("index")).expect("json")
}

/// The first differing key path of two JSON values.
fn first_diff(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                let (va, vb) = (x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null));
                if let Some(d) = first_diff(va, vb, &format!("{path}.{k}")) {
                    return Some(d);
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) => {
            for i in 0..x.len().max(y.len()) {
                let (va, vb) = (x.get(i).unwrap_or(&Value::Null), y.get(i).unwrap_or(&Value::Null));
                if let Some(d) = first_diff(va, vb, &format!("{path}[{i}]")) {
                    return Some(d);
                }
            }
            None
        }
        _ if a == b => None,
        _ => Some(format!("{path}: ours {a} / oracle {b}")),
    }
}

/// What the gates read that is not in git: the saved XML records of the oracle (`fixtures/build/*.xml`, `.gitignore`) and the personal builds.
/// The reason to skip, else `None`.
fn missing_inputs() -> Option<String> {
    let Some(first) = local_builds("build").into_iter().next() else {
        return Some("builds/local-builds.json is missing (personal builds)".to_owned());
    };
    let b = format!("{BUILDS}{first}.xml");
    if !std::path::Path::new(&b).exists() {
        return Some(format!("{b} is missing (personal builds)"));
    }
    let dir = scene::oracle_dir().join("fixtures/build");
    let any_xml = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| e.path().extension().is_some_and(|x| x == "xml"));
    if !any_xml {
        return Some(format!(
            "no {}/*.xml (ignored by git; in a worktree copy them, or run tools/sg-oracle/run.ps1 -Script build-runner.lua)",
            dir.display()
        ));
    }
    None
}

#[test]
fn build_states_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED build_states_equal_original: {why}");
        return;
    }
    let mut failures = Vec::new();
    for name in scenario_names() {
        let rec = oracle_record(&name);
        assert_eq!(
            rec["errors"].as_array().map(Vec::len),
            Some(0),
            "{name}: oracle errors {}",
            rec["errors"]
        );
        let run = run(&name);
        let want: Vec<(u64, Value)> = rec["dumps"]
            .as_array()
            .expect("dumps")
            .iter()
            .map(|d| {
                let mut s = d["state"].clone();
                // the checkout writes the harness refused (the `.bin` cache of the Timeless Jewel data) -- the oracle's, not a UI state
                if let Some(o) = s.as_object_mut() {
                    o.remove("writesRefused");
                }
                (d["step"].as_u64().expect("step"), s)
            })
            .collect();
        assert_eq!(run.dumps.len(), want.len(), "{name}: number of dumps");
        let mut ndiff = 0;
        for ((s1, got), (s2, want)) in run.dumps.iter().zip(&want) {
            assert_eq!(s1, s2);
            if let Some(d) = first_diff(&norm(got), &norm(want), "") {
                ndiff += 1;
                failures.push(format!("{name} step {s1}: {d}"));
                let out = scene::out_dir().join(format!("build-{name}-{s1}.ours.json"));
                std::fs::write(&out, serde_json::to_string_pretty(got).expect("json")).expect("write");
            }
        }
        // the oracle's perf (os.clock, LuaJIT) next to ours
        let lua: Vec<(String, f64)> = rec["perf"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| {
                let what = p["what"].as_str().unwrap_or("");
                let label = if what == "open" {
                    format!("open {}", p["build"].as_str().unwrap_or(""))
                } else {
                    what.to_string()
                };
                (label, p["ms"].as_f64().unwrap_or(f64::NAN))
            })
            .collect();
        for ((w1, ours), (w2, theirs)) in run.perf.iter().zip(&lua) {
            eprintln!("{name}: {w1:<34} Rust {ours:>9.2} ms | original {theirs:>9.2} ms ({w2})");
        }
        eprintln!("{name}: {} dumps, {ndiff} differ", want.len());
        // Save: the file the original wrote vs ours, through a reload (SaveDB's attribute / section order is `pairs` order in LuaJIT,
        // R13; the R8.8 gate already holds SaveDB = LuaJIT canonically): the same output and the same re-saved XML.
        for (out_name, ours) in &run.files {
            let p = scene::oracle_dir().join(format!("fixtures/build/{out_name}"));
            let theirs = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            let (a, b) = (
                pob_engine::engine::Engine::load_xml(ours).expect("our saved file"),
                pob_engine::engine::Engine::load_xml(&theirs).expect("the original's saved file"),
            );
            let st = |e: &pob_engine::engine::Engine| format!("{:?}", e.result().stats);
            // the order of `<Input>` / `<Placeholder>` is `pairs` order in the original (ConfigTab.lua:970/:983, R13): lines as a multiset
            let canon = |e: &pob_engine::engine::Engine| {
                let x = e.save_xml().expect("save");
                let mut l: Vec<String> = x.lines().map(str::to_owned).collect();
                l.sort();
                l
            };
            if st(&a) != st(&b) || canon(&a) != canon(&b) {
                failures.push(format!(
                    "{name}: saved {out_name} differs from the original's (after a reload): output {}, SaveDB {}",
                    if st(&a) == st(&b) { "=" } else { "!=" },
                    if canon(&a) == canon(&b) { "=" } else { "!=" }
                ));
                let out = scene::out_dir();
                std::fs::write(out.join(format!("{out_name}.ours.resaved.xml")), a.save_xml().expect("save")).expect("write");
                std::fs::write(out.join(format!("{out_name}.oracle.resaved.xml")), b.save_xml().expect("save")).expect("write");
                std::fs::write(out.join(format!("{out_name}.ours.xml")), ours).expect("write");
            } else {
                eprintln!("{name}: saved {out_name} = the original's (reload: output bit for bit, SaveDB canonically)");
            }
        }
        let shots: Vec<&str> = rec["shots"]
            .as_array()
            .expect("shots")
            .iter()
            .map(|s| s.as_str().expect("shot"))
            .collect();
        let ours: Vec<&str> = run.shots.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(ours, shots, "{name}: screenshots");
    }
    assert!(failures.is_empty(), "{} states differ:\n{}", failures.len(), failures.join("\n"));
}

/// "New" in the build list (`main:SetMode("BUILD", false, "Unnamed build")`): an empty build opens (not oracle-gated: a smoke test).
#[test]
fn new_build_opens() {
    if local_builds("build").is_empty() {
        eprintln!("SKIPPED new_build_opens: no personal builds (its sandbox copies them)");
        return;
    }
    let sandbox = Sandbox::new("new");
    let tp = TestPlatform::new();
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, Box::new(tp.clone())).expect("fonts");
    let root = pob_root();
    let l = Launch::new(LaunchConfig {
        user_path: Some(sandbox.0.to_string_lossy().replace('\\', "/")),
        user_folder: "Path of Building".into(),
        script_path: scene::script_path(),
        doc_path: format!("{root}/"),
        version_number: "?".into(),
        version_branch: Some("?".into()),
        version_platform: Some("?".into()),
        app_version: None,
    });
    l.on_init(&mut gfx);
    main().set_mode(
        "BUILD",
        vec![
            pob_ui::modules::main::ModeArg::Bool(false),
            pob_ui::modules::main::ModeArg::Str("Unnamed build".into()),
        ],
    );
    for _ in 0..3 {
        frame(&mut gfx, &l);
    }
    assert!(l.prompt_msg.borrow().is_none(), "error: {:?}", l.prompt_msg.borrow());
    assert_eq!(main().mode.borrow().as_deref(), Some("BUILD"));
    let d = build_dump(&gfx);
    assert_eq!(d["mainSocketGroup"]["list"], json!(["<No skills added yet>"]));
    assert_eq!(d["unsaved"], json!(false));
    pob_ui::modules::main::clear_main();
}

/// The compared region of a BUILD screenshot: everything but the tab area (`x >= 312 && y >= 32`, Build.lua:1293-1299), plus the
/// open popup (it is drawn over the tab area, Main.lua:437-444).
fn shell_pixel(x: u32, y: u32, popup: Option<[f64; 4]>) -> bool {
    let in_popup = popup.is_some_and(|[px, py, pw, ph]| {
        let (fx, fy) = (f64::from(x), f64::from(y));
        fx >= px && fx < px + pw && fy >= py && fy < py + ph
    });
    x < 312 || y < 32 || in_popup
}

#[test]
#[ignore = "needs a GPU adapter (or POB_GFX_WARP=1); run with --ignored"]
fn build_pixels_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED build_pixels_equal_original: {why}");
        return;
    }
    let mut r = pob_gfx::Renderer::new_offscreen().expect("wgpu adapter");
    let mut failures = Vec::new();
    for name in scenario_names() {
        for (shot, list, popup) in run(&name).shots {
            let mut rgba = r.render_to_rgba(&list).expect("render");
            let p = scene::oracle_dir().join(format!("fixtures/build/{shot}.png"));
            let mut oracle = image::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_rgba8();
            assert_eq!(oracle.dimensions(), scene::SCREEN, "{shot}: oracle size");
            let (w, h) = scene::SCREEN;
            for y in 0..h {
                for x in 0..w {
                    if !shell_pixel(x, y, popup) {
                        let i = ((y * w + x) * 4) as usize;
                        rgba[i..i + 3].copy_from_slice(&[0, 0, 0]);
                        oracle.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                    }
                }
            }
            let mask = scene::text_mask(&list, w, h);
            let (d, heat) = scene::diff(&rgba, w, &oracle, &mask);
            eprintln!(
                "{shot:<28} bad {:>5} ({:.4}%) maxΔ {:>3} | text px {:>6} bad {:>4} maxΔ {:>3} | bbox {:?}",
                d.bad,
                d.frac() * 100.0,
                d.max_delta,
                d.text_pixels,
                d.text_bad,
                d.text_max_delta,
                d.bbox
            );
            if d.bad > 0 {
                let out = scene::out_dir();
                scene::save(&heat, &out.join(format!("{shot}.heat.png")));
                scene::save(&scene::crop(&rgba, w, w, h), &out.join(format!("{shot}.ours.png")));
                failures.push(format!("{shot}: {} px off (text {}), bbox {:?}", d.bad, d.text_bad, d.bbox));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "the BUILD shell differs from the original:\n{}",
        failures.join("\n")
    );
}
