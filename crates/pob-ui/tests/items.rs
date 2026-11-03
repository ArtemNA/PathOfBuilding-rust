//! U6 gate: the original `ItemsTab` / `ItemSlotControl` / `ItemListControl` / `ItemDBControl` (the whole PoB, run by the SG oracle
//! `tools/sg-oracle/lua/items-runner.lua`) vs this port, on the public goldens (`tools/sg-oracle/scenes/items/*.json`) and the local list `local_builds("items")`.
//! - `items_states_equal_original` (no GPU): every `dump` step = the oracle's.
//! - `items_pixels_equal_original` (GPU/WARP, `#[ignore]`): every screenshot = the original's PNG; 0 pixels beyond 2/255 outside the lossy class.
//!   cargo test -p pob-ui --release --test items -- --include-ignored --nocapture

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::{Button, DrawList, Gfx, RawEvent, TestPlatform};
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
        let root = std::env::temp_dir().join(format!("pob-ui-u6-{}-{n}-{name}", std::process::id()));
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
        // the same edit as items-runner.lua
        let w = r#"<TradeSearchWeights><Stat stat="FullDPS" label="old label" weightMult="1.5"/><Stat stat="NoSuchStat" label="gone" weightMult="2"/><Stat stat="Life" label="zero" weightMult="0"/></TradeSearchWeights><TradeSearchWeights><Stat stat="TotalEHP" weightMult="0.125"/><Stat stat="Armour" label="not a number" weightMult="x"/></TradeSearchWeights></Items>"#;
        let x = std::fs::read_to_string(format!("{}/spec/TestBuilds/3.13/OccVortex.xml", scene::pob_root())).expect("golden");
        std::fs::write(builds.join("Trade Weights Test.xml"), x.replacen("</Items>", w, 1)).expect("weights build");
        for b in local_builds("items") {
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

/// `<Items ...>...</Items>` of a saved file.
fn items_block(x: &str) -> String {
    let a = x.find("<Items").expect("<Items");
    let b = x[a..].find("</Items>").unwrap_or(x.len() - a);
    let items = x[a..a + b].replace("\r\n", "\n");
    let s = x.find("<Skills").expect("<Skills");
    let e = x[s..].find("</Skills>").unwrap_or(x.len() - s);
    // the groups' attributes: a `<Gem>`'s `variantId` is of the process (R48)
    let groups: Vec<&str> = x[s..s + e].lines().map(str::trim).filter(|l| l.starts_with("<Skill ")).collect();
    format!("{items}\n{}", groups.join("\n"))
}

fn dump(g: &Gfx, st: &Value, tp: &TestPlatform) -> Value {
    let m = main();
    let b = m.build_mode.clone();
    let mut d = serde_json::Map::new();
    d.insert("popups".into(), json!(m.popups.borrow().len()));
    if let Some(p) = m.popups.borrow().first() {
        d.insert("popupTitle".into(), json!(p.title.get(g)));
    }
    d.insert("clipboard".into(), json!(tp.state().clipboard.unwrap_or_else(|| "nil".into())));
    let tab = b.items_tab.borrow().clone().expect("itemsTab");
    d.insert("items".into(), tab.debug_dump(g));
    if st["treeTip"].as_bool() == Some(true) {
        let tv = b.tree_view.borrow();
        let lines: Vec<String> = tv
            .as_ref()
            .expect("view")
            .tooltip
            .lines
            .iter()
            .map(|l| l.text.clone().unwrap_or_else(|| "<sep>".to_string()))
            .collect();
        d.insert("treeTip".into(), json!(lines));
    }
    let c = b.c();
    let lines: Vec<String> = c
        .stat_box
        .list
        .borrow()
        .iter()
        .map(|l| {
            let col = |i: usize| l.cols.get(i).cloned().flatten().unwrap_or("nil".into());
            format!("{}|{}", col(0), col(1))
        })
        .collect();
    d.insert(
        "build".into(),
        json!({ "viewMode": *b.view_mode.borrow(), "unsaved": b.unsaved.get(), "statBox": lines, "warnings": b.warning_lines.borrow().clone(),
            "loadouts": c.build_loadouts.list.borrow().iter().map(|v| v.label.clone().unwrap_or_default()).collect::<Vec<_>>(), "loadoutSel": c.build_loadouts.sel_index.get() }),
    );
    if let Some(f) = st["file"].as_str() {
        let p = format!("{}{f}", main().build_path.borrow());
        d.insert("saved".into(), json!(items_block(&std::fs::read_to_string(p).expect("saved file"))));
    }
    Value::Object(d)
}

fn oracle_state(o: &Value, st: &Value, name: &str) -> Value {
    let s = &o["state"];
    let b = &s["build"];
    let lines: Vec<Value> = b["statBox"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| {
            let parts: Vec<&str> = l.as_str().expect("line").split('|').collect();
            json!(format!("{}|{}", parts[3], parts[4]))
        })
        .collect();
    let mut d = serde_json::Map::new();
    d.insert("popups".into(), s["popups"].clone());
    if !s["popupTitle"].is_null() {
        d.insert("popupTitle".into(), s["popupTitle"].clone());
    }
    d.insert("clipboard".into(), s["clipboard"].clone());
    d.insert("items".into(), s["items"].clone());
    if st["treeTip"].as_bool() == Some(true) {
        d.insert("treeTip".into(), s["tooltip"].clone());
    }
    d.insert(
        "build".into(),
        json!({ "viewMode": b["viewMode"], "unsaved": b["unsaved"], "statBox": lines, "warnings": b["warnings"], "loadouts": b["buildLoadouts"]["list"], "loadoutSel": b["buildLoadouts"]["sel"] }),
    );
    if let Some(out) = st["fileOut"].as_str() {
        let p = scene::oracle_dir().join(format!("fixtures/items/{out}"));
        let t = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{name}: {}: {e}", p.display()));
        d.insert("saved".into(), json!(items_block(&t)));
    }
    Value::Object(d)
}

fn centre(g: &Gfx, c: &dyn Control, dx: f64, dy: f64) -> (f64, f64) {
    let (x, y) = c.get_pos(g);
    let (w, h) = c.get_size(g);
    ((x + w / 2.0).floor() + dx, (y + h / 2.0).floor() + dy)
}

/// `itemsCursor` of items-runner.lua.
fn cursor_step(gfx: &mut Gfx, st: &Value, cur: &mut (f64, f64)) {
    let m = main();
    let b = m.build_mode.clone();
    let tab = b.items_tab.borrow().clone();
    let off = |a: &Vec<Value>, i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0);
    let ctl = |name: &str| {
        tab.as_ref()
            .expect("tab")
            .debug_control(name)
            .unwrap_or_else(|| panic!("control {name}"))
    };
    if (st.get("cursorPopup").is_some()
        || st.get("cursorPopupRow").is_some()
        || st.get("cursorPopupChild").is_some()
        || st.get("cursorPopupPick").is_some())
        && m.popups.borrow().is_empty()
    {
        return;
    }
    if let Some(c) = st["cursorItem"].as_array() {
        *cur = centre(gfx, &*ctl(c[0].as_str().expect("name")), off(c, 1), off(c, 2));
    } else if let Some(c) = st["cursorBuild"].as_array() {
        let n = b
            .host
            .get(c[0].as_str().expect("name"))
            .unwrap_or_else(|| panic!("build control {}", c[0]));
        *cur = centre(gfx, &*n, off(c, 1), off(c, 2));
    } else if let Some(c) = st["cursorPopup"].as_array() {
        let p = m.popups.borrow()[0].clone();
        *cur = centre(gfx, &*p.control(c[0].as_str().expect("name")), off(c, 1), off(c, 2));
    } else if let Some(c) = st["cursorChild"].as_array() {
        let n = tab
            .as_ref()
            .expect("tab")
            .debug_child(c[0].as_str().expect("parent"), c[1].as_str().expect("child"))
            .unwrap_or_else(|| panic!("child {c:?}"));
        *cur = centre(gfx, &*n, off(c, 2), off(c, 3));
    } else if let Some(c) = st["cursorSkillCtl"].as_str() {
        let t = b.skills_tab.borrow().clone().expect("skillsTab");
        *cur = t.debug_control_pos(gfx, c, 0.0, 0.0);
    } else if let Some(id) = st["cursorNode"].as_u64() {
        let art = b.tree_art();
        let eng = b.engine.borrow();
        let spec = eng.as_ref().expect("engine").spec_rc();
        let tv = b.tree_view.borrow();
        let v = tv.as_ref().expect("view");
        let n = spec.node(id as u32).expect("node").tree_node();
        let (w, h) = (968.0, 712.0);
        let scale = f64::min(w, h) / art.size * v.zoom;
        *cur = (
            (n.x.expect("x") * scale + v.zoom_x + 312.0 + w / 2.0).floor(),
            (n.y.expect("y") * scale + v.zoom_y + 32.0 + h / 2.0).floor(),
        );
    } else if let Some(i) = st["cursorSkillRow"].as_f64() {
        let t = b.skills_tab.borrow().clone().expect("skillsTab");
        let n = t.read(|e| e.socket_groups().len()) as f64;
        let row = if i > 0.0 { i } else { n };
        let (x, y) = t.group_list.get_pos(gfx);
        *cur = (x + 10.0, y + 2.0 + 16.0 * (row - 1.0) + 8.0);
    } else if let Some(c) = st["cursorItemRow"].as_array() {
        let (x, y) = ctl(c[0].as_str().expect("name")).get_pos(gfx);
        *cur = (x + 10.0, y + 2.0 + 16.0 * (off(c, 1) - 1.0) + 8.0);
    } else if let Some(c) = st["cursorPopupRow"].as_array() {
        let p = m.popups.borrow()[0].clone();
        let (x, y) = p.control(c[0].as_str().expect("name")).get_pos(gfx);
        *cur = (x + 10.0, y + 2.0 + 16.0 * (off(c, 1) - 1.0) + 8.0);
    } else if let Some(c) = st["cursorPopupPick"].as_array() {
        let p = m.popups.borrow()[0].clone();
        let n = p.control(c[0].as_str().expect("name"));
        let (x, y) = n.get_pos(gfx);
        let (_, h) = n.get_size(gfx);
        *cur = (x + 10.0, y + h + 2.0 + (off(c, 1) - 1.0) * (h - 4.0) + ((h - 4.0) / 2.0).floor());
    } else if let Some(c) = st["cursorBuildPick"].as_array() {
        let n = b
            .host
            .get(c[0].as_str().expect("name"))
            .unwrap_or_else(|| panic!("build control {}", c[0]));
        let (x, y) = n.get_pos(gfx);
        let (_, h) = n.get_size(gfx);
        *cur = (x + 10.0, y + h + 2.0 + (off(c, 1) - 1.0) * (h - 4.0) + ((h - 4.0) / 2.0).floor());
    } else if let Some(c) = st["cursorPick"].as_array() {
        let n = ctl(c[0].as_str().expect("name"));
        let (x, y) = n.get_pos(gfx);
        let (_, h) = n.get_size(gfx);
        *cur = (x + 10.0, y + h + 2.0 + (off(c, 1) - 1.0) * (h - 4.0) + ((h - 4.0) / 2.0).floor());
    } else {
        return;
    }
    gfx.input(RawEvent::Cursor { x: cur.0, y: cur.1 });
}

struct Run {
    dumps: Vec<(u64, Value, Value)>,
    shots: Vec<(String, DrawList)>,
    perf: Vec<(String, f64)>,
}

fn run(name: &str) -> Run {
    let sc: Value =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join(format!("scenes/items/{name}.json"))).expect("scenario"))
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
        if let Some(o) = st["treeOrder"].as_str() {
            // the overlap order of this oracle process (R27, R31)
            let rec = oracle_record(o).unwrap_or_else(|| panic!("fixtures/items/{o}.json"));
            let ids: Vec<u32> = rec["nodes"]
                .as_array()
                .expect("nodes")
                .iter()
                .map(|v| v.as_f64().expect("id") as u32)
                .collect();
            let conns: Vec<String> = rec["connectors"]
                .as_array()
                .expect("connectors")
                .iter()
                .map(|v| v.as_str().expect("c").to_string())
                .collect();
            let subs: Vec<u32> = rec["subGraphs"]
                .as_array()
                .expect("subGraphs")
                .iter()
                .map(|v| v.as_f64().expect("id") as u32)
                .collect();
            let build = main().build_mode.clone();
            let mut view = build.tree_view.borrow_mut();
            let view = view.as_mut().expect("view");
            view.set_order_sample(ids, conns);
            view.set_sub_graph_order_sample(subs);
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
            Some("SHIFT") => gfx.input(RawEvent::Key {
                key: pob_gfx::input::PhysKey::Shift,
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
            out.dumps.push(((i + 1) as u64, dump(&gfx, st, &tp), st.clone()));
        }
        if let Some(shot) = st["shot"].as_str() {
            // R34: a label without a colour code takes the colour of the control drawn before it, in the `pairs` order of this oracle process
            if let Some(order) = oracle_popup_order(name, (i + 1) as u64) {
                pob_ui::tabs::tree::set_popup_order_sample(&order);
            }
            let list = frame(&mut gfx, &l);
            out.shots.push((shot.to_string(), list));
        }
    }
    assert!(l.prompt_msg.borrow().is_none(), "{name}: error: {:?}", l.prompt_msg.borrow());
    pob_ui::modules::main::clear_main();
    out
}

/// The real window on both sides (items-real-seq.ps1): the stages' state and the events the window delivered. Run the sides first:
///   pwsh tools/sg-oracle/run.ps1 -Script items-runner.lua -ScriptArgs real-input -Inject inject-items-real.ps1 -Timeout 40
///   pwsh tools/sg-oracle/items-rust-input.ps1
#[test]
fn items_real_input_equals_original() {
    let read = |n: &str| -> Option<Value> {
        let t = std::fs::read_to_string(scene::oracle_dir().join(format!("fixtures/items/{n}.json"))).ok()?;
        Some(serde_json::from_str(t.trim_start_matches('\u{feff}')).expect("json"))
    };
    let (Some(o), Some(r)) = (read("real-input-oracle"), read("real-input-rust")) else {
        eprintln!("SKIPPED items_real_input_equals_original: no fixtures/items/real-input-*.json");
        return;
    };
    let mut events = 0;
    for stage in ["start", "tab", "row", "popup", "craft", "typed", "wheel"] {
        if let Some(d) = first_diff(&norm(&o[stage]["itemsReal"]), &norm(&r[stage]["itemsReal"]), stage) {
            panic!("{d}");
        }
        assert_eq!(o[stage]["events"], r[stage]["events"], "{stage}: events");
        events = o[stage]["events"].as_array().map_or(0, Vec::len);
    }
    assert_eq!(o["row"]["itemsReal"]["listSel"].as_f64(), Some(2.0), "the row was clicked");
    assert_ne!(o["craft"]["itemsReal"]["displayRaw"].as_str(), Some("nil"), "the editor opened");
    assert_eq!(o["typed"]["itemsReal"]["search"].as_str(), Some("12"), "the digits arrived");
    assert!(
        o["typed"]["itemsReal"]["dbN"].as_f64() < o["craft"]["itemsReal"]["dbN"].as_f64(),
        "the search filtered"
    );
    eprintln!(
        "7 stages, {events} events, the wheel scrolled to {}",
        o["wheel"]["itemsReal"]["scrollV"]
    );
}

fn oracle_record(name: &str) -> Option<Value> {
    let p = scene::oracle_dir().join(format!("fixtures/items/{name}.json"));
    Some(serde_json::from_str(&std::fs::read_to_string(p).ok()?).expect("oracle json"))
}

/// `popupOrder` of the oracle's dump of a step.
fn oracle_popup_order(scene: &str, step: u64) -> Option<Vec<String>> {
    let rec = oracle_record(scene)?;
    let d = rec["dumps"].as_array()?.iter().find(|d| d["step"].as_u64() == Some(step))?;
    Some(
        d["state"]["popupOrder"]
            .as_array()?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
    )
}

fn scenario_names() -> Vec<String> {
    let mut names: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join("scenes/items/index.json")).expect("index")).expect("json");
    for (i, _) in local_builds("items").iter().enumerate() {
        let n = format!("local-{}", i + 1);
        if scene::oracle_dir().join(format!("scenes/items/{n}.json")).exists() && oracle_record(&n).is_some() {
            names.push(n);
        } else {
            eprintln!("SKIPPED scene {n}: no scene or fixture");
        }
    }
    names
}

fn first_diff(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        // an empty Lua table encodes as `[]`
        (Value::Array(x), Value::Object(y)) | (Value::Object(y), Value::Array(x)) if x.is_empty() && y.is_empty() => None,
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
        (Value::Array(x), Value::Array(y)) => x
            .iter()
            .zip(y)
            .enumerate()
            .find_map(|(i, (v, w))| first_diff(v, w, &format!("{path}[{i}]")))
            .or_else(|| (x.len() != y.len()).then(|| format!("{path}: length oracle {} ours {}", x.len(), y.len()))),
        _ if a == b => None,
        (Value::String(x), Value::String(y)) if x.len() > 300 => {
            let at = x.bytes().zip(y.bytes()).position(|(p, q)| p != q).unwrap_or(x.len().min(y.len()));
            let from = at.saturating_sub(120);
            Some(format!(
                "{path}: at byte {at}: oracle {:?}, ours {:?}",
                x.get(from..(at + 120).min(x.len())),
                y.get(from..(at + 120).min(y.len()))
            ))
        }
        _ => Some(format!("{path}: oracle {a}, ours {b}")),
    }
}

fn missing_inputs() -> Option<String> {
    scenario_names()
        .iter()
        .find(|n| oracle_record(n).is_none())
        .map(|n| format!("fixtures/items/{n}.json is absent (tools/sg-oracle/run.ps1 -Script items-runner.lua)"))
}

#[test]
fn items_states_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED items_states_equal_original: {why}");
        return;
    }
    // POB_ITEMS_ONLY=<scene>: one scene while looking at a difference
    let only = std::env::var("POB_ITEMS_ONLY").ok();
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
        if let Ok(dir) = std::env::var("POB_ITEMS_DUMP_OURS") {
            std::fs::create_dir_all(&dir).expect("mkdir");
            let all: Vec<Value> = r.dumps.iter().map(|(s, d, _)| json!({ "step": s, "state": d })).collect();
            std::fs::write(
                format!("{dir}/{name}.json"),
                serde_json::to_string(&json!({ "dumps": all })).expect("json"),
            )
            .expect("write");
        }
        for (o, (step, ours, st)) in dumps.iter().zip(&r.dumps) {
            assert_eq!(o["step"].as_u64(), Some(*step), "{name}: steps");
            let state = norm(&oracle_state(o, st, &name));
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
    assert!(
        failures.is_empty(),
        "{} differences, the first 25:\n{}",
        failures.len(),
        failures[..failures.len().min(25)]
            .iter()
            .map(|s| s.chars().take(420).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Rust vs the original, roughly (a busy machine): `perf` scene.
#[test]
#[ignore = "numbers for a report"]
fn items_perf() {
    let Some(rec) = oracle_record("perf") else {
        eprintln!("SKIPPED items_perf: no fixtures/items/perf.json");
        return;
    };
    eprintln!("oracle: {}", rec["perf"]);
    eprintln!("rust:   {:?}", run("perf").perf);
}

/// What the start of the program pays for the databases of `onFrameFuncs["LoadItems"]`: the frames until they are ready, frame by frame.
#[test]
#[ignore = "numbers for a report"]
fn item_db_startup_perf() {
    let sandbox = Sandbox::new("startup");
    let tp = TestPlatform::new();
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, Box::new(tp.clone())).expect("fonts");
    gfx.set_dpi_scale_override_percent(100);
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
    let t0 = std::time::Instant::now();
    l.on_init(&mut gfx);
    eprintln!("on_init {:.1} ms", t0.elapsed().as_secs_f64() * 1e3);
    main().background.wait();
    let mut frames = Vec::new();
    while !main().item_dbs_ready() {
        let t = std::time::Instant::now();
        frame(&mut gfx, &l);
        frames.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let sum: f64 = frames.iter().sum();
    let max = frames.iter().copied().fold(0.0, f64::max);
    eprintln!(
        "frames {}, total {sum:.1} ms, longest {max:.1} ms, first three {:?}, uniques {}, rares {}",
        frames.len(),
        &frames[..frames.len().min(3)],
        pob_ui::tabs::items::unique_db_items().len(),
        pob_ui::tabs::items::rare_db_items().len()
    );
    pob_ui::modules::main::clear_main();
}

#[test]
#[ignore = "needs a wgpu adapter"]
fn items_pixels_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED items_pixels_equal_original: {why}");
        return;
    }
    let only = std::env::var("POB_ITEMS_ONLY").ok();
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
            let p = scene::oracle_dir().join(format!("fixtures/items/{shot}.png"));
            let oracle = image::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_rgba8();
            if let Ok(dir) = std::env::var("POB_ITEMS_SAVE_OURS") {
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
            // The original draws this frame in more than one way between processes (MINEFIELD R54): any frame it was seen to draw counts, unmasked.
            let variant = (2..)
                .map_while(|n| image::open(scene::oracle_dir().join(format!("fixtures/items/{shot}~{n}.png"))).ok())
                .any(|v| {
                    let v = v.to_rgba8();
                    let (d, _) = scene::diff(&rgba, w, &v, &mask);
                    d.bad == 0 || (d.text_bad == 0 && classify_bad(&rgba, &v, &list, &lossy_sizes).1 == 0)
                });
            if variant {
                eprintln!("{shot}: equals a variant of the original");
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
