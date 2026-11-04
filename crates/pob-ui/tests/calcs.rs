//! U4 gate: the original `CalcsTab` / `CalcSectionControl` / `CalcBreakdownControl` (the whole PoB, run by the SG oracle
//! `tools/sg-oracle/lua/calcs-runner.lua`) vs this port, on the public goldens (`tools/sg-oracle/scenes/calcs/*.json`).
//! - `calcs_states_equal_original` (no GPU): every `dump` step = the oracle's (sections, skill controls, popups with all their rows).
//! - `calcs_pixels_equal_original` (GPU/WARP, `#[ignore]`): every screenshot = the original's PNG; 0 pixels beyond 2/255 outside the lossy class.
//!   cargo test -p pob-ui --release --test calcs -- --include-ignored --nocapture

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::{Button, DrawList, Gfx, RawEvent, TestPlatform};
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::main;
use pob_ui::tabs::calc_breakdown::CalcBreakdownControl;
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

const BUILDS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../builds/pc/3.29/");
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

/// A public golden has no minions, mines or stages: OccVortex + three groups (the same edit as calcs-runner.lua).
fn spectre_test() -> String {
    let mut x = std::fs::read_to_string(format!("{}/spec/TestBuilds/3.13/OccVortex.xml", scene::pob_root())).expect("golden");
    let after = |x: &str, tag: &str| {
        let i = x.find(tag).expect("tag");
        i + x[i..].find('>').expect(">") + 1
    };
    let at = after(&x, "<Build ");
    x.insert_str(
        at,
        r#"<Spectre id="Metadata/Monsters/Axis/AxisCaster"/><Spectre id="Metadata/Monsters/DemonModular/DemonModularBladeVortex"/>"#,
    );
    let gem = |id: &str, skill: &str, name: &str| {
        format!(
            r#"<Gem level="20" gemId="Metadata/Items/Gems/{id}" skillId="{skill}" qualityId="Default" quality="0" enabled="true" nameSpec="{name}"/>"#
        )
    };
    let group = |g: String| format!(r#"<Skill mainActiveSkillCalcs="1" enabled="true" mainActiveSkill="1">{g}</Skill>"#);
    let at = after(&x, "<Skills ");
    let groups = [
        group(gem("SkillGemRaiseSpectre", "RaiseSpectre", "Raise Spectre")),
        group(
            gem("SkillGemFireball", "Fireball", "Fireball")
                + &gem("SkillGemSupportBlastchainMine", "SupportBlastchainMine", "Blastchain Mine"),
        ),
        group(gem("SkillGemIncinerate", "Incinerate", "Incinerate")),
    ]
    .concat();
    x.insert_str(at, &groups);
    x
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
        let root = std::env::temp_dir().join(format!("pob-ui-u4-{}-{n}-{name}", std::process::id()));
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
        for b in local_builds("calcs") {
            std::fs::copy(format!("{BUILDS}{b}.xml"), builds.join(format!("{b}.xml"))).expect("copy build");
        }
        std::fs::write(builds.join("Spectre Test.xml"), spectre_test()).expect("write");
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

fn dump(g: &Gfx) -> Value {
    let b = main().build_mode.clone();
    let tab = b.calcs_tab.borrow().clone().expect("calcsTab");
    let mut d = tab.debug_dump(g);
    let side = b.breakdown.borrow().clone().expect("breakdown");
    d["sidebar"] = side.debug_dump(g);
    d["bsel"] = json!(match b.host.sel() {
        Some(c) if pob_ui::control::same(&*c, &*side) => "breakdown",
        Some(_) => "other",
        None => "nil",
    });
    d["unsaved"] = json!(b.unsaved.get());
    let (a, c) = b.engine.borrow().as_ref().expect("engine").debug_alloc_orders();
    d["allocMain"] = json!(a);
    d["allocCalcs"] = json!(c);
    json!({ "calcs": d, "viewMode": *b.view_mode.borrow() })
}

/// `calcsCursor` of calcs-runner.lua.
fn cursor_step(gfx: &mut Gfx, st: &Value, cur: &mut (f64, f64)) {
    let b = main().build_mode.clone();
    let tab = b.calcs_tab.borrow().clone().expect("calcsTab");
    if let Some(c) = st["cursorCell"].as_array() {
        *cur = tab.debug_cell_pos(
            c[0].as_str().expect("id"),
            c[1].as_str().expect("label"),
            c[2].as_u64().unwrap_or(1) as usize,
        );
    } else if let Some(c) = st["cursorControl"].as_array() {
        *cur = tab.debug_control_pos(gfx, c[0].as_str().expect("id"), c[1].as_str().expect("name"));
    } else if let Some(c) = st["cursorOverlay"].as_array() {
        let s = tab
            .sections
            .iter()
            .find(|s| s.debug_id() == c[0].as_str().expect("id"))
            .expect("section");
        *cur = (
            s.overlay_x.get() + c[1].as_f64().expect("dx"),
            s.overlay_y.get() + c[2].as_f64().expect("dy"),
        );
    } else if let Some(c) = st["cursorRel"].as_array() {
        cur.0 += c[0].as_f64().expect("dx");
        cur.1 += c[1].as_f64().expect("dy");
    } else if let Some(s) = st["cursorStat"].as_str() {
        *cur = b.debug_stat_pos(gfx, s);
    } else if let Some(c) = st["cursorBd"].as_array() {
        let bd: Rc<CalcBreakdownControl> = if c[0].as_str() == Some("sidebar") {
            b.breakdown.borrow().clone().expect("breakdown")
        } else {
            tab.breakdown.clone()
        };
        *cur = bd.debug_cell_pos(c[1].as_str().expect("source"), c[2].as_str().expect("key"));
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
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join(format!("scenes/calcs/{name}.json"))).expect("scenario"))
            .expect("json");
    let sandbox = Sandbox::new(name);
    pob_engine::calcs_tab::set_mod_row_order_sample(None);
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
        perf: Vec::new(),
    };
    let mut cur = (-100.0, -100.0);
    for (i, st) in sc["steps"].as_array().expect("steps").iter().enumerate() {
        if st["waitAssets"].as_bool() == Some(true) {
            main().build_mode.wait_tree_assets();
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
        if let Some(o) = st["treeOrder"].as_str() {
            // the overlap order of this oracle process (R27), for the node viewer
            let rec: Value = serde_json::from_str(
                &std::fs::read_to_string(scene::oracle_dir().join(format!("fixtures/calcs/{o}.json"))).expect("order"),
            )
            .expect("json");
            let ids = rec["nodes"]
                .as_array()
                .expect("nodes")
                .iter()
                .map(|v| v.as_f64().expect("id") as u32)
                .collect();
            let conns = rec["connectors"]
                .as_array()
                .expect("connectors")
                .iter()
                .map(|v| v.as_str().expect("c").to_string())
                .collect();
            let tab = main().build_mode.calcs_tab.borrow().clone().expect("calcsTab");
            tab.breakdown.set_viewer_order_sample(ids, conns);
            continue;
        }
        if let Some(o) = st["buffOrder"].as_str() {
            // R13: the mods of `env.buffs[name]` of this oracle process
            let rec: Value = serde_json::from_str(
                &std::fs::read_to_string(scene::oracle_dir().join(format!("fixtures/calcs/{o}.json"))).expect("order"),
            )
            .expect("json");
            let keys = rec["keys"]
                .as_array()
                .expect("keys")
                .iter()
                .map(|v| v.as_str().expect("key").to_string())
                .collect();
            pob_engine::calcs_tab::set_mod_row_order_sample(Some(keys));
            continue;
        }
        if let Some(id) = st["perfAlloc"].as_u64() {
            let b = main().build_mode.clone();
            let id = id as u32;
            {
                let mut eng = b.engine.borrow_mut();
                let e = eng.as_mut().expect("engine");
                if e.spec_rc().node(id).expect("node").alloc {
                    e.dealloc_node(id).expect("dealloc");
                } else {
                    e.alloc_node(id, None).expect("alloc");
                }
            }
            // `spec:AddUndoState()` of the step: `spec.modFlag`
            b.mod_flag.set(true);
            b.build_flag.set(true);
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
        if let Some(n) = st["perfRecalc"].as_u64() {
            let b = main().build_mode.clone();
            let t0 = std::time::Instant::now();
            for _ in 0..n {
                b.engine.borrow_mut().as_mut().expect("engine").refresh().expect("refresh");
            }
            out.perf.push(("recalc".into(), t0.elapsed().as_secs_f64() * 1e3 / n as f64));
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
            out.dumps.push(((i + 1) as u64, dump(&gfx)));
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
    let p = scene::oracle_dir().join(format!("fixtures/calcs/{name}.json"));
    Some(serde_json::from_str(&std::fs::read_to_string(p).ok()?).expect("oracle json"))
}

fn scenario_names() -> Vec<String> {
    let mut names: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join("scenes/calcs/index.json")).expect("index")).expect("json");
    // personal builds: compared when the local list, the scenes and the fixtures are there
    for (i, _) in local_builds("calcs").iter().enumerate() {
        let n = format!("local-{}", i + 1);
        if scene::oracle_dir().join(format!("scenes/calcs/{n}.json")).exists() && oracle_record(&n).is_some() {
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
                return Some(format!("{path}: length oracle {} ours {}", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (v, w))| first_diff(v, w, &format!("{path}[{i}]")))
        }
        _ if a == b => None,
        _ => Some(format!("{path}: oracle {a}, ours {b}")),
    }
}

fn missing_inputs() -> Option<String> {
    let names = scenario_names();
    names
        .iter()
        .find(|n| oracle_record(n).is_none())
        .map(|n| format!("fixtures/calcs/{n}.json is absent (tools/sg-oracle/run.ps1 -Script calcs-runner.lua)"))
}

#[test]
fn calcs_states_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED calcs_states_equal_original: {why}");
        return;
    }
    let mut failures = Vec::new();
    let mut compared = 0;
    for name in scenario_names() {
        let rec = oracle_record(&name).expect("record");
        let r = run(&name);
        let dumps = rec["dumps"].as_array().expect("dumps");
        assert_eq!(dumps.len(), r.dumps.len(), "{name}: dump count");
        // POB_CALCS_DUMP_OURS=<dir>: this port's dumps, for a look at a difference
        if let Ok(dir) = std::env::var("POB_CALCS_DUMP_OURS") {
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
            let mut state = json!({ "calcs": o["state"]["calcs"], "viewMode": o["state"]["build"]["viewMode"] });
            state = norm(&state);
            compared += 1;
            let mut ours = norm(ours);
            // A section that is off keeps `section.y = section.y - offset` of every frame (CalcsTab.lua:314) and is neither drawn nor hit: its position counts frames.
            for d in [&mut state, &mut ours] {
                for s in d["calcs"]["sections"].as_array_mut().expect("sections") {
                    if s["enabled"] != json!(true) {
                        s["rect"] = Value::Null;
                    }
                }
            }
            if let Some(d) = first_diff(&state, &ours, "") {
                failures.push(format!("{name} step {step}: {d}"));
            }
        }
    }
    eprintln!("dumps compared {compared}, differ {}", failures.len());
    assert!(
        failures.is_empty(),
        "{} differences, the first 20:\n{}",
        failures.len(),
        failures[..failures.len().min(20)].join("\n")
    );
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

/// Rust vs the original, roughly (a busy machine): `perf` scene, tools/sg-oracle/run.ps1 -Script calcs-runner.lua -ScriptArgs perf.
#[test]
#[ignore = "numbers for a report"]
fn calcs_perf() {
    let Some(rec) = oracle_record("perf") else {
        eprintln!("SKIPPED calcs_perf: no fixtures/calcs/perf.json");
        return;
    };
    eprintln!("oracle: {}", rec["perf"]);
    eprintln!("rust:   {:?}", run("perf").perf);
}

#[test]
#[ignore = "needs a wgpu adapter"]
fn calcs_pixels_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED calcs_pixels_equal_original: {why}");
        return;
    }
    let mut r = pob_gfx::Renderer::new_offscreen().expect("wgpu adapter");
    let lossy_sizes = lossy_sheet_sizes();
    let mut failures = Vec::new();
    let (mut shots, mut lossy_total) = (0, 0);
    for name in scenario_names() {
        for (shot, list) in run(&name).shots {
            shots += 1;
            let rgba = r.render_to_rgba(&list).expect("render");
            let (w, h) = scene::SCREEN;
            let p = scene::oracle_dir().join(format!("fixtures/calcs/{shot}.png"));
            let oracle = image::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_rgba8();
            if let Ok(dir) = std::env::var("POB_CALCS_SAVE_OURS") {
                std::fs::create_dir_all(&dir).expect("mkdir");
                scene::save(&scene::crop(&rgba, w, w, h), &PathBuf::from(&dir).join(format!("{shot}.png")));
            }
            let mask = scene::text_mask(&list, w, h);
            let (d, heat) = scene::diff(&rgba, w, &oracle, &mask);
            eprintln!(
                "{shot:<24} bad {:>6} maxΔ {:>3} | text bad {:>4} maxΔ {:>3} | bbox {:?}",
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
