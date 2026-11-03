//! U2b gate: the original `Launch.lua` + `Main.lua` + `BuildList.lua` (run by the SG oracle,
//! `tools/sg-oracle/lua/ui-runner.lua`) vs this port, on the same test build folder and the same input scenarios
//! (`tools/sg-oracle/scenes/ui/*.json`).
//! - `ui_states_equal_original` (no GPU): every `dump` step = the oracle's record `fixtures/ui/<scenario>.json`
//!   (mode, popups, list rows, selection, sub-path, sort, search, the files on disk, Settings.xml after a save).
//! - `ui_pixels_equal_original` (GPU/WARP, `#[ignore]`): every screenshot = the original's PNG over the whole 1280x800
//!   client, 0 pixels beyond 2/255.
//!   cargo test -p pob-ui --release --test ui -- --include-ignored --nocapture
//!
//! The test build folder is a temporary directory (copies of `builds/pc/3.29`), never the user's documents.

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::input::PhysKey;
use pob_gfx::{Button, DrawList, Gfx, RawEvent, TestPlatform};
use pob_ui::ControlRef;
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::{Main, main};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

const BUILDS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../builds/pc/3.29/");

/// A list of personal builds from the local `builds/local-builds.json` (not in git); empty without it.
fn local_builds(key: &str) -> Vec<String> {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../builds/local-builds.json");
    let v: Option<serde_json::Value> = std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok());
    v.and_then(|v| v[key].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()))
        .unwrap_or_default()
}

/// The gates need the personal builds and the oracle's records of them (`fixtures/ui/`), neither is in git.
fn skip(test: &str) -> bool {
    let missing = local_builds("ui_root").is_empty() || !scene::oracle_dir().join("fixtures/ui").is_dir();
    if missing {
        eprintln!("SKIPPED {test}: no personal builds or fixtures/ui");
    }
    missing
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

/// The sandbox of one scenario: `<tmp>/Path of Building/Builds` like ui-runner.lua builds it.
struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        // the tests run in parallel: one's `Drop` must not take the other's folder
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("pob-ui-u2b-{}-{n}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let builds = root.join("Path of Building/Builds");
        std::fs::create_dir_all(builds.join("Merc")).expect("mkdir");
        std::fs::create_dir_all(builds.join("Empty")).expect("mkdir");
        for b in local_builds("ui_root") {
            std::fs::copy(format!("{BUILDS}{b}.xml"), builds.join(format!("{b}.xml"))).expect("copy build");
        }
        for b in local_builds("ui_merc") {
            std::fs::copy(format!("{BUILDS}{b}.xml"), builds.join(format!("Merc/{b}.xml"))).expect("copy build");
        }
        std::fs::copy(
            scene::oracle_dir().join("scenes/ui/Settings.xml"),
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

fn listing(root: &str) -> String {
    let mut out = Vec::new();
    fn walk(root: &str, sub: &str, out: &mut Vec<String>) {
        let mut h = pob_gfx::fs::new_file_search(&format!("{root}{sub}*"), false);
        while let Some(x) = &mut h {
            out.push(format!("{sub}{}", x.get_file_name()));
            if !x.next_file() {
                break;
            }
        }
        let mut h = pob_gfx::fs::new_file_search(&format!("{root}{sub}*"), true);
        let mut dirs = Vec::new();
        while let Some(x) = &mut h {
            dirs.push(x.get_file_name().to_string());
            if !x.next_file() {
                break;
            }
        }
        for d in dirs {
            out.push(format!("{sub}{d}/"));
            walk(root, &format!("{sub}{d}/"), out);
        }
    }
    walk(root, "", &mut out);
    out.sort();
    out.join(",")
}

fn settings(m: &Main) -> Value {
    let path = format!("{}Settings.xml", m.user_path.borrow().clone().unwrap_or_default());
    let text = std::fs::read_to_string(path).expect("Settings.xml");
    let nodes = pob_build::xml::parse_xml_document(&text).expect("xml");
    let mut s = Map::new();
    for n in nodes {
        let pob_build::xml::XmlNode::Element(root) = n else { continue };
        for node in root.child_elements() {
            if node.name == "Mode" {
                let args: Vec<String> = node
                    .child_elements()
                    .map(|a| {
                        if let Some(v) = a.attr("string") {
                            format!("s:{v}")
                        } else if let Some(v) = a.attr("number") {
                            format!("n:{v}")
                        } else {
                            format!("b:{}", a.attr("boolean").unwrap_or("nil"))
                        }
                    })
                    .collect();
                s.insert(
                    "mode".into(),
                    json!(format!("{}({})", node.attr("mode").unwrap_or(""), args.join(","))),
                );
            } else if node.name == "Misc" {
                for (k, v) in &node.attrib {
                    s.insert(k.clone(), json!(v));
                }
            }
        }
    }
    Value::Object(s)
}

fn dump(g: &Gfx, tp: &TestPlatform, settings_too: bool) -> Value {
    let m = main();
    let l = pob_ui::launch::launch();
    let st = tp.state();
    let mut d = Map::new();
    d.insert("mode".into(), json!(m.mode.borrow().clone().unwrap_or("nil".into())));
    d.insert("popups".into(), json!(m.popups.borrow().len()));
    d.insert("prompt".into(), json!(l.prompt_msg.borrow().clone().unwrap_or("nil".into())));
    d.insert("clipboard".into(), json!(st.clipboard.clone().unwrap_or("nil".into())));
    d.insert("opened".into(), json!(st.opened.join(",")));
    d.insert("mainSel".into(), name_of(&m.host.controls.borrow(), m.host.sel()));
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
    let lm = m.list_mode.clone();
    if lm.initialised.get() {
        let bl = lm.bl();
        let list = bl.list.list.borrow().clone();
        let rows: Vec<String> = list
            .iter()
            .enumerate()
            .map(|(i, b)| {
                format!(
                    "{}|{}",
                    bl.list.hooks.get_row_value(&bl.list, g, 1, i + 1, b),
                    bl.list.hooks.get_row_value(&bl.list, g, 2, i + 1, b)
                )
            })
            .collect();
        d.insert("rows".into(), json!(rows));
        d.insert("selIndex".into(), bl.list.sel_index.get().map_or(json!("nil"), |v| json!(v)));
        d.insert("subPath".into(), json!(lm.sub_path.borrow().clone()));
        d.insert("sort".into(), json!(m.s.borrow().build_sort_mode.clone()));
        d.insert(
            "search".into(),
            json!(lm.search_text.borrow().as_ref().expect("search").buf.borrow().clone()),
        );
        d.insert("listSel".into(), name_of(&lm.host.controls.borrow(), lm.host.sel()));
        let folders: Vec<String> = bl.path.folder_list.borrow().iter().map(|f| f.label.clone()).collect();
        d.insert("path".into(), json!(folders.join("/")));
        let bp = m.build_path.borrow().clone();
        let rel = |b: &Option<pob_ui::modules::build_list_helpers::BuildRef>| {
            b.as_ref().map_or("nil".to_string(), |b| b.0.full_file_name[bp.len()..].to_string())
        };
        d.insert("copy".into(), json!(rel(&bl.copy_build.borrow())));
        d.insert("cut".into(), json!(rel(&bl.cut_build.borrow())));
        d.insert("drag".into(), json!(bl.list.sel_drag_active.get()));
    }
    d.insert("files".into(), json!(listing(&m.build_path.borrow())));
    if settings_too {
        d.insert("settings".into(), settings(&m));
    }
    Value::Object(d)
}

pub struct Run {
    pub dumps: Vec<(u64, Value)>,
    pub shots: Vec<(String, DrawList)>,
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

fn pob_root() -> String {
    scene::pob_root()
}

/// Runs `scenes/ui/<name>.json` like `ui-runner.lua`.
fn run(name: &str) -> Run {
    let sc: Value =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join(format!("scenes/ui/{name}.json"))).expect("scenario"))
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
        script_path: format!("{root}/src/"),
        doc_path: format!("{root}/"),
        version_number: "?".into(),
        version_branch: Some("?".into()),
        version_platform: Some("?".into()),
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
    };
    let mut held: HashMap<String, RawEvent> = HashMap::new();
    for (i, st) in sc["steps"].as_array().expect("steps").iter().enumerate() {
        if let Some(n) = st["perf"].as_u64() {
            // Same measure as ui-runner.lua: CPU of launch:OnFrame (UI + command recording), no rendering.
            let t0 = std::time::Instant::now();
            for _ in 0..n {
                frame(&mut gfx, &l);
            }
            let ours = t0.elapsed().as_secs_f64() * 1e3 / n as f64;
            let rec = oracle_record(name);
            eprintln!(
                "{name}: launch:OnFrame {ours:.4} ms/frame (Rust) vs {:.4} ms/frame (original, os.clock)",
                rec["perf"]["msPerFrame"].as_f64().unwrap_or(f64::NAN)
            );
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
            out.dumps
                .push(((i + 1) as u64, dump(&gfx, &tp, st["settings"].as_bool() == Some(true))));
        }
        if let Some(shot) = st["shot"].as_str() {
            out.shots.push((shot.to_string(), frame(&mut gfx, &l)));
        }
    }
    pob_ui::modules::main::clear_main();
    out
}

fn oracle_record(name: &str) -> Value {
    let p = scene::oracle_dir().join(format!("fixtures/ui/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("json")
}

fn scenario_names() -> Vec<String> {
    serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join("scenes/ui/index.json")).expect("index")).expect("json")
}

#[test]
fn ui_states_equal_original() {
    if skip("ui_states_equal_original") {
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
            .map(|d| (d["step"].as_u64().expect("step"), d["state"].clone()))
            .collect();
        assert_eq!(run.dumps.len(), want.len(), "{name}: number of dumps");
        for ((s1, got), (s2, want)) in run.dumps.iter().zip(&want) {
            assert_eq!(s1, s2);
            if norm(got) != norm(want) {
                failures.push(format!("{name} step {s1}:\n  ours   {}\n  oracle {}", norm(got), norm(want)));
            }
        }
        let shots: Vec<&str> = rec["shots"]
            .as_array()
            .expect("shots")
            .iter()
            .map(|s| s.as_str().expect("shot"))
            .collect();
        let ours: Vec<&str> = run.shots.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(ours, shots, "{name}: screenshots");
    }
    assert!(failures.is_empty(), "{} states differ:\n{}", failures.len(), failures.join("\n"));
}

/// The file-function probe of the oracle (MakeDir / RemoveDir / NewFileSearch) = `pob_gfx::fs`.
#[test]
fn fs_probe_equals_original() {
    if skip("fs_probe_equals_original") {
        return;
    }
    let rec = oracle_record(&scenario_names()[0]);
    let p = &rec["probe"];
    let dir = std::env::temp_dir().join(format!("pob-ui-u2b-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let base = format!("{}/", dir.to_string_lossy().replace('\\', "/"));
    let pack = |r: Result<(), String>| match r {
        Ok(()) => "true".to_string(),
        Err(m) => format!("nil|{m}"),
    };
    use pob_gfx::fs::{make_dir, new_file_search, remove_dir};
    let names = |h: Option<pob_gfx::fs::FileSearch>| {
        let mut t = Vec::new();
        let mut h = h;
        while let Some(x) = &mut h {
            t.push(x.get_file_name().to_string());
            if !x.next_file() {
                break;
            }
        }
        t.join(",")
    };
    let mut got = Map::new();
    got.insert("mkNew".into(), json!(pack(make_dir(&base))));
    got.insert("mkExisting".into(), json!(pack(make_dir(&base))));
    got.insert("mkNested".into(), json!(pack(make_dir(&format!("{base}a/b/c")))));
    got.insert("mkA".into(), json!(pack(make_dir(&format!("{base}a")))));
    got.insert("mkNested2".into(), json!(pack(make_dir(&format!("{base}a/b/c")))));
    got.insert("mkB".into(), json!(pack(make_dir(&format!("{base}a/b")))));
    for n in ["x.xml", "B.xml", "c.XML", "a b.xml", "_u.xml", "10.xml", "9.xml", "y.txt"] {
        std::fs::write(format!("{base}a/{n}"), "<Build/>").expect("write");
    }
    let _ = make_dir(&format!("{base}a/Z"));
    let _ = make_dir(&format!("{base}a/e"));
    got.insert("files".into(), json!(names(new_file_search(&format!("{base}a/*.xml"), false))));
    got.insert("dirs".into(), json!(names(new_file_search(&format!("{base}a/*"), true))));
    got.insert("all".into(), json!(names(new_file_search(&format!("{base}a/*"), false))));
    got.insert("none".into(), json!(names(new_file_search(&format!("{base}a/*.nothing"), false))));
    got.insert("rmNonEmpty".into(), json!(pack(remove_dir(&format!("{base}a"), false))));
    got.insert("rmEmpty".into(), json!(pack(remove_dir(&format!("{base}a/Z"), false))));
    got.insert("rmMissing".into(), json!(pack(remove_dir(&format!("{base}nope"), false))));
    got.insert("mkOverFile".into(), json!(pack(make_dir(&format!("{base}a/x.xml")))));
    got.insert("rmRecursive".into(), json!(pack(remove_dir(&format!("{base}a"), true))));
    let _ = std::fs::remove_dir_all(&dir);
    for (k, v) in &got {
        assert_eq!(v, &p[k], "probe {k}");
    }
    // `GetFileModifiedTime` is a FILETIME (100 ns since 1601): the oracle saw `mtime - os.time()` ≈ 1.34e17.
    assert!(p["mtimeMinusNow"].as_f64().expect("mtime") > 1.3e17);
}

#[test]
#[ignore = "needs a GPU adapter (or POB_GFX_WARP=1); run with --ignored"]
fn ui_pixels_equal_original() {
    if skip("ui_pixels_equal_original") {
        return;
    }
    let mut r = pob_gfx::Renderer::new_offscreen().expect("wgpu adapter");
    let mut failures = Vec::new();
    for name in scenario_names() {
        for (shot, list) in run(&name).shots {
            let rgba = r.render_to_rgba(&list).expect("render");
            let p = scene::oracle_dir().join(format!("fixtures/ui/{shot}.png"));
            let oracle = image::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_rgba8();
            assert_eq!(oracle.dimensions(), scene::SCREEN, "{shot}: oracle size");
            let mask = scene::text_mask(&list, scene::SCREEN.0, scene::SCREEN.1);
            let (d, heat) = scene::diff(&rgba, scene::SCREEN.0, &oracle, &mask);
            eprintln!(
                "{shot:<22} bad {:>5} ({:.4}%) maxΔ {:>3} | text px {:>6} bad {:>4} maxΔ {:>3} | bbox {:?}",
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
                scene::save(
                    &scene::crop(&rgba, scene::SCREEN.0, scene::SCREEN.0, scene::SCREEN.1),
                    &out.join(format!("{shot}.ours.png")),
                );
                failures.push(format!("{shot}: {} px off (text {}), bbox {:?}", d.bad, d.text_bad, d.bbox));
            }
        }
    }
    assert!(failures.is_empty(), "UI differs from the original:\n{}", failures.join("\n"));
}
