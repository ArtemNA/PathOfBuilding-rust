//! U3a gate: the original `PassiveTreeView` / `TreeTab` (the whole PoB, run by the SG oracle `tools/sg-oracle/lua/tree-runner.lua`) vs this
//! port, on the same builds and scenarios (`tools/sg-oracle/scenes/tree/*.json`).
//! - `tree_orders_equal_original` (no GPU): `pairs(tree.groups)`, `tree.connectors`, `pairs(spec.nodes)` = the oracle's records.
//! - `tree_states_equal_original` (no GPU): every `dump` step = the oracle's (zoom, hover, allocNodes, trace path, the shell).
//! - `tree_pixels_equal_original` (GPU/WARP, `#[ignore]`): every screenshot = the original's PNG over the whole window, the tab's controls
//!   included (U3b). 0 pixels beyond 2/255.
//!   cargo test -p pob-ui --release --test tree -- --include-ignored --nocapture

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::input::PhysKey;
use pob_gfx::{Button, DrawList, Gfx, RawEvent, TestPlatform};
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

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("pob-ui-u3a-{}-{n}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let builds = root.join("Path of Building/Builds");
        std::fs::create_dir_all(&builds).expect("mkdir");
        for b in local_builds("tree") {
            std::fs::copy(format!("{BUILDS}{b}.xml"), builds.join(format!("{b}.xml"))).expect("copy build");
        }
        std::fs::copy(
            format!("{}/spec/TestBuilds/3.13/{GOLDEN}.xml", scene::pob_root()),
            builds.join(format!("{GOLDEN}.xml")),
        )
        .expect("copy golden");
        // the scene `dual-savior`: the overlap of a connector and two nodes by the start of the class (`order-dual-savior`)
        std::fs::copy(
            format!("{}/spec/TestBuilds/3.13/Dual Savior.xml", scene::pob_root()),
            builds.join("Dual Savior.xml"),
        )
        .expect("copy golden");
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

fn tree_dump() -> Value {
    let b = main().build_mode.clone();
    let eng = b.engine.borrow();
    let e = eng.as_ref().expect("engine");
    let spec = e.spec_rc();
    let tv = b.tree_view.borrow();
    let v = tv.as_ref().expect("tree view");
    let mut ids: Vec<u32> = spec.alloc_nodes.keys().copied().collect();
    ids.sort_unstable();
    json!({
        "zoomLevel": v.zoom_level,
        "zoomX": format!("{:.6}", v.zoom_x),
        "zoomY": format!("{:.6}", v.zoom_y),
        "hover": v.hover_node.map_or(json!("nil"), |h| json!(spec.nodes[h].id)),
        "alloc": ids,
        "tracePath": v.trace_path.as_ref().map_or(json!("nil"), |t| json!(t.iter().map(|i| spec.nodes[*i].id).collect::<Vec<_>>())),
        "search": v.search_str,
        "dragging": v.dragging,
        "buildFlag": b.build_flag.get(),
    })
}

fn dump() -> Value {
    let m = main();
    let mut d = Map::new();
    d.insert("mode".into(), json!(m.mode.borrow().clone().unwrap_or("nil".into())));
    d.insert("popups".into(), json!(m.popups.borrow().len()));
    let b = m.build_mode.clone();
    if m.mode.borrow().as_deref() == Some("BUILD") && b.c.borrow().is_some() {
        let c = b.c();
        let mut bd = Map::new();
        bd.insert(
            "points".into(),
            json!(c.point_display.str_.borrow().clone().unwrap_or("nil".into())),
        );
        bd.insert("unsaved".into(), json!(b.unsaved.get()));
        let sel = b.host.sel();
        let sel_name = sel.as_ref().map_or("nil".to_string(), |c| {
            let controls = b.host.controls.borrow();
            controls
                .iter()
                .find(|(_, v)| pob_ui::control::same(&***v, &**c))
                .map_or("other".to_string(), |(k, _)| k.clone())
        });
        bd.insert("sel".into(), json!(sel_name));
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
        bd.insert("statBox".into(), json!(lines));
        d.insert("build".into(), Value::Object(bd));
        d.insert("tree".into(), tree_dump());
        if let Some(v) = b.tree_view.borrow().as_ref() {
            let lines: Vec<String> = v.tooltip.lines.iter().map(|l| l.text.clone().unwrap_or("<sep>".into())).collect();
            d.insert("tooltip".into(), json!(lines));
        }
    }
    Value::Object(d)
}

/// The oracle's dump restricted to what `dump()` records.
fn oracle_state(s: &Value) -> Value {
    let mut d = Map::new();
    d.insert("mode".into(), s["mode"].clone());
    d.insert("popups".into(), s["popups"].clone());
    if let Some(b) = s.get("build") {
        let mut bd = Map::new();
        bd.insert("points".into(), b["points"].clone());
        bd.insert("unsaved".into(), b["unsaved"].clone());
        bd.insert("sel".into(), b["sel"].clone());
        let lines: Vec<Value> = b["statBox"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|l| {
                let parts: Vec<&str> = l.as_str().expect("line").split('|').collect();
                json!(format!("{}|{}", parts[3], parts[4]))
            })
            .collect();
        bd.insert("statBox".into(), Value::Array(lines));
        d.insert("build".into(), Value::Object(bd));
    }
    if let Some(t) = s.get("tree") {
        let mut t = t.clone();
        // the probes of the runner, not view state
        if let Some(o) = t.as_object_mut() {
            for k in ["cursorAt", "mainFocus", "mouseOver", "sel", "bsel", "mainSel", "dragX"] {
                o.remove(k);
            }
        }
        d.insert("tree".into(), t);
    }
    if let Some(t) = s.get("tab") {
        d.insert("tab".into(), t.clone());
    }
    if let Some(t) = s.get("tooltip") {
        // dkjson writes an empty table as an object or an array
        d.insert("tooltip".into(), if t.as_array().is_some() { t.clone() } else { json!([]) });
    }
    Value::Object(d)
}

struct Shot {
    name: String,
    list: DrawList,
    mask: Option<[u32; 4]>,
}

struct Run {
    dumps: Vec<(u64, Value)>,
    shots: Vec<Shot>,
    perf: Vec<(String, f64)>,
    orders: HashMap<String, Value>,
    paths: HashMap<String, Value>,
    unimplemented: Vec<String>,
    /// `powerProgress` steps: (the report list's label, the toast's text).
    progress: Vec<(Option<String>, Option<String>)>,
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

fn order_of() -> Value {
    let b = main().build_mode.clone();
    let tv = b.tree_view.borrow();
    let v = tv.as_ref().expect("tree view");
    v.debug_orders()
}

/// The `treePaths` record of tree-runner.lua: `node.path` of the live spec and `pairs(allocNodes)`.
fn paths_of() -> Value {
    let b = main().build_mode.clone();
    let eng = b.engine.borrow();
    let spec = eng.as_ref().expect("engine").spec_rc();
    let mut paths = Map::new();
    for n in &spec.nodes {
        if let Some(p) = &n.path {
            paths.insert(n.id.to_string(), json!(p.iter().map(|i| spec.nodes[*i].id).collect::<Vec<_>>()));
        }
    }
    json!({ "paths": paths, "allocOrder": spec.alloc_table_pairs() })
}

/// `cursorFind` of tree-runner.lua.
fn cursor_find(kind: &str) -> (f64, f64) {
    let art = main().build_mode.tree_art();
    let b = main().build_mode.clone();
    let eng = b.engine.borrow();
    let spec = eng.as_ref().expect("engine").spec_rc();
    let tv = b.tree_view.borrow();
    let v = tv.as_ref().expect("view");
    let mut best: Option<(u32, f64, f64)> = None;
    let (w, h) = (968.0, 712.0);
    let scale = f64::min(w, h) / art.size * v.zoom;
    let mut take = |id: u32, n: &pob_tree::SpecNode| {
        let t = n.tree_node();
        if let (Some(x), Some(y)) = (t.x, t.y)
            && best.is_none_or(|(i, _, _)| id < i)
        {
            let (x, y) = (
                (x * scale + v.zoom_x + 312.0 + w / 2.0).floor(),
                (y * scale + v.zoom_y + 32.0 + h / 2.0).floor(),
            );
            if kind == "compareOnly" || (x > 320.0 && x < 1270.0 && y > 40.0 && y < 736.0) {
                best = Some((id, x, y));
            }
        }
    };
    if matches!(kind, "freeNotable" | "freeNormal" | "allocNormal") {
        // U10c: the smallest id of a kind of node in the view
        let ty = if kind == "freeNotable" { pob_tree::NodeType::Notable } else { pob_tree::NodeType::Normal };
        for n in &spec.nodes {
            if (kind == "allocNormal") == n.alloc && n.ty() == ty && n.ascendancy_name().is_none() && n.id < 65536 {
                take(n.id, n);
            }
        }
    } else if kind == "clusterSocket" {
        for n in &spec.nodes {
            if n.alloc && n.tree_node().expansion_jewel.is_some() && n.id < 65536 {
                take(n.id, n);
            }
        }
    } else {
        let cmp = v.compare_spec.as_ref().expect("compare spec");
        for c in &cmp.nodes {
            let own = spec.node(c.id);
            if c.alloc && ((kind == "compareOnly" && own.is_none()) || (kind == "compareAlloc" && own.is_some_and(|n| !n.alloc))) {
                take(c.id, c);
            }
        }
    }
    let (_, x, y) = best.unwrap_or_else(|| panic!("cursorFind: none {kind}"));
    (x, y)
}

fn run(name: &str) -> Run {
    let sc: Value =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join(format!("scenes/tree/{name}.json"))).expect("scenario"))
            .expect("json");
    let sandbox = Sandbox::new(name);
    pob_ui::tabs::timeless::set_radius_order_sample(HashMap::new());
    let tp = TestPlatform::new();
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, Box::new(tp.clone())).expect("fonts");
    gfx.set_dpi_scale_override_percent(100);
    gfx.input(RawEvent::Cursor { x: -100.0, y: -100.0 });
    let root = scene::pob_root();
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
    // U10: the fake transport of the leagues; nothing else of these scenes reaches it
    let leagues = sc["steps"].as_array().into_iter().flatten().find_map(|s| s["netLeagues"].as_str());
    if let Some(body) = leagues {
        let t = std::sync::Arc::new(pob_net::ScriptedTransport::default());
        t.add(
            "https://www.pathofexile.com/api/trade/data/leagues",
            Ok(pob_net::Reply {
                code: 200,
                header: String::new(),
                body: body.as_bytes().to_vec(),
            }),
        );
        l.set_transport(t);
    }
    // the requests wait for a `netLeagues` step, as `launch:DownloadPage` of tree-runner.lua: the others never answer
    l.hold_sub_scripts(true);
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
        orders: HashMap::new(),
        paths: HashMap::new(),
        unimplemented: Vec::new(),
        progress: Vec::new(),
    };
    let mut held: HashMap<String, RawEvent> = HashMap::new();
    let oracle_dumps = oracle_record_opt(name).unwrap_or(Value::Null);
    for (i, st) in sc["steps"].as_array().expect("steps").iter().enumerate() {
        if st["waitAssets"].as_bool() == Some(true) {
            main().build_mode.wait_tree_assets();
            frame(&mut gfx, &l);
            continue;
        }
        if st["openNew"].as_bool() == Some(true) {
            main().set_mode(
                "BUILD",
                vec![
                    pob_ui::modules::main::ModeArg::Bool(false),
                    pob_ui::modules::main::ModeArg::Str("Unnamed build".to_string()),
                ],
            );
            frame(&mut gfx, &l);
            continue;
        }
        if st["waitPower"].as_bool() == Some(true) {
            let t0 = std::time::Instant::now();
            frame(&mut gfx, &l);
            pob_ui::tabs::tree::wait_power(&gfx);
            frame(&mut gfx, &l);
            out.perf.push(("power".into(), t0.elapsed().as_secs_f64() * 1e3));
            continue;
        }
        if let Some(p) = st["powerProgress"].as_array() {
            // `powerBuilderProgressCallback(percent)` at a given time; `dismiss`: the user closed the toast first
            let t = main().build_mode.tree_tab.borrow().clone().expect("tab");
            if st["dismiss"].as_bool() == Some(true) {
                let id = t.power_toast.borrow().clone().expect("toast");
                main().toasts.dismiss(&id);
            }
            t.power_builder_progress(p[0].as_u64().expect("time"), p[1].as_i64());
            let toast = t.power_toast.borrow().as_deref().and_then(|id| main().toasts.message(id));
            out.progress.push((t.power_report_list.label.get(&gfx), toast));
            continue;
        }
        if let Some(s) = st["perfPower"].as_str() {
            let t = main().build_mode.tree_tab.borrow().clone().expect("tab");
            let stat = pob_engine::tree_tab::power_stat_list()
                .into_iter()
                .find(|p| p.stat.as_deref() == Some(s));
            t.set_power_calc(&gfx, stat);
            let t0 = std::time::Instant::now();
            frame(&mut gfx, &l);
            pob_ui::tabs::tree::wait_power(&gfx);
            out.perf.push(("powerFull".into(), t0.elapsed().as_secs_f64() * 1e3));
            continue;
        }
        if let Some(s) = st["perfSpec"].as_u64() {
            let t0 = std::time::Instant::now();
            let t = main().build_mode.tree_tab.borrow().clone().expect("tab");
            t.set_active_spec(&mut gfx, s as usize);
            frame(&mut gfx, &l);
            frame(&mut gfx, &l);
            out.perf.push(("spec".into(), t0.elapsed().as_secs_f64() * 1e3));
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
        if let Some(id) = st["perfAlloc"].as_u64() {
            let b = main().build_mode.clone();
            let id = id as u32;
            let t0 = std::time::Instant::now();
            {
                let mut eng = b.engine.borrow_mut();
                let e = eng.as_mut().expect("engine");
                if e.spec_rc().node(id).expect("node").alloc {
                    e.dealloc_node(id).expect("dealloc");
                } else {
                    e.alloc_node(id, None).expect("alloc");
                }
            }
            b.build_flag.set(true);
            frame(&mut gfx, &l);
            out.perf.push(("click".into(), t0.elapsed().as_secs_f64() * 1e3));
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
        // U10c: `launch.devMode` and the ALT key
        if let Some(on) = st["devAlt"].as_bool() {
            pob_ui::launch::launch().dev_mode.set(on);
            gfx.input(RawEvent::Key {
                key: PhysKey::Alt,
                pressed: on,
            });
            continue;
        }
        if let Some(s) = st["search"].as_str()
            && let Some(v) = main().build_mode.tree_view.borrow_mut().as_mut()
        {
            v.search_str = s.to_string();
        }
        if let Some(o) = st["treeOrder"].as_str() {
            out.orders.insert(o.to_string(), order_of());
            // the overlap order of this oracle process (R13: `PassiveTreeView::set_order_sample`)
            let rec = oracle_record(o);
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
            main()
                .build_mode
                .tree_view
                .borrow_mut()
                .as_mut()
                .expect("view")
                .set_order_sample(ids.clone(), conns);
            if let Some(radius) = rec["radiusOrder"].as_object() {
                let of = |l: &Value| {
                    l.as_array()
                        .expect("order")
                        .iter()
                        .map(|v| v.as_f64().expect("id") as u32)
                        .collect()
                };
                pob_ui::tabs::timeless::set_radius_order_sample(radius.iter().map(|(k, l)| (k.parse().expect("id"), of(l))).collect());
            }
            if let Some(names) = rec["clusterNames"].as_array() {
                let names = names.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
                *main()
                    .build_mode
                    .tree_tab
                    .borrow()
                    .as_ref()
                    .expect("tab")
                    .report_order_sample
                    .borrow_mut() = Some((ids, names));
            }
            continue;
        }
        if let Some(o) = st["treePaths"].as_str() {
            out.paths.insert(o.to_string(), paths_of());
            continue;
        }
        if let Some(c) = st["cursor"].as_array() {
            gfx.input(RawEvent::Cursor {
                x: c[0].as_f64().expect("x"),
                y: c[1].as_f64().expect("y"),
            });
        }
        // U10: the centre of a control of the popup
        if let Some(name) = st["cursorPopup"].as_str() {
            let p = main().popups.borrow()[0].clone();
            let c = p.control(name);
            let (x, y) = c.get_pos(&gfx);
            let (w, h) = c.get_size(&gfx);
            gfx.input(RawEvent::Cursor {
                x: (x + w / 2.0).floor(),
                y: (y + h / 2.0).floor(),
            });
        }
        // U10c: `{ control of the popup, 1-based row of its open list }`
        if let Some(c) = st["cursorPopupRow"].as_array() {
            let p = main().popups.borrow()[0].clone();
            let ctl = p.control(c[0].as_str().expect("name"));
            let (x, y) = ctl.get_pos(&gfx);
            let (w, h) = ctl.get_size(&gfx);
            gfx.input(RawEvent::Cursor {
                x: (x + w / 2.0).floor(),
                y: y + h + 2.0 + (c[1].as_f64().expect("row") - 1.0) * (h - 4.0) + 4.0,
            });
        }
        // U10: the reply of the leagues (`netLeagues` of tree-runner.lua)
        if st.get("netLeagues").is_some() {
            l.net.wait_idle();
            l.poll_sub_scripts(&mut gfx);
        }
        if let Some(id) = st["cursorNode"].as_u64() {
            let (x, y) = {
                let art = main().build_mode.tree_art();
                let b = main().build_mode.clone();
                let eng = b.engine.borrow();
                let spec = eng.as_ref().expect("engine").spec_rc();
                let tv = b.tree_view.borrow();
                let v = tv.as_ref().expect("view");
                let n = spec.node(id as u32).expect("node").tree_node();
                // U10c: `viewH`: the heat map's controls take a second line
                let (w, h) = (968.0, st["viewH"].as_f64().unwrap_or(712.0));
                let scale = f64::min(w, h) / art.size * v.zoom;
                (
                    (n.x.expect("x") * scale + v.zoom_x + 312.0 + w / 2.0).floor(),
                    (n.y.expect("y") * scale + v.zoom_y + 32.0 + h / 2.0).floor(),
                )
            };
            gfx.input(RawEvent::Cursor { x, y });
        }
        if let Some(kind) = st["cursorFind"].as_str() {
            let (x, y) = cursor_find(kind);
            gfx.input(RawEvent::Cursor { x, y });
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
        let perf_t0 = std::time::Instant::now();
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
            out.perf.push((label.to_string(), perf_t0.elapsed().as_secs_f64() * 1e3));
        }
        if st["dump"].as_bool() == Some(true) {
            let mut d = dump();
            if st["tab"].as_bool() == Some(true) {
                d["tab"] = pob_ui::tabs::tree::debug_dump(&gfx);
            }
            out.dumps.push(((i + 1) as u64, d));
        }
        if let Some(shot) = st["shot"].as_str() {
            // the popup's `pairs(controls)` of the oracle process (R13)
            if let Some(order) = oracle_popup_order(&oracle_dumps, (i + 1) as u64) {
                pob_ui::tabs::tree::set_popup_order_sample(&order);
            }
            let list = frame(&mut gfx, &l);
            out.shots.push(Shot {
                name: shot.to_string(),
                list,
                mask: st["mask"].as_array().map(|m| {
                    let v = |i: usize| m[i].as_u64().expect("mask") as u32;
                    [v(0), v(1), v(2), v(3)]
                }),
            });
        }
    }
    if let Some(v) = main().build_mode.tree_view.borrow().as_ref() {
        out.unimplemented.clone_from(&v.unimplemented);
    }
    // a panic in a handler is the error prompt (`launch.promptMsg`)
    assert!(l.prompt_msg.borrow().is_none(), "{name}: error: {:?}", l.prompt_msg.borrow());
    pob_ui::modules::main::clear_main();
    out
}

/// `popupOrder` of the oracle's dump of a step.
fn oracle_popup_order(rec: &Value, step: u64) -> Option<Vec<String>> {
    let d = rec["dumps"].as_array()?.iter().find(|d| d["step"].as_u64() == Some(step))?;
    Some(
        d["state"]["popupOrder"]
            .as_array()?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
    )
}

fn oracle_record_opt(name: &str) -> Option<Value> {
    let p = scene::oracle_dir().join(format!("fixtures/tree/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()
}

fn oracle_record(name: &str) -> Value {
    let p = scene::oracle_dir().join(format!("fixtures/tree/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("json")
}

/// U3c: the poeurl buttons of the link popups do not crash. Abyss and `Generate` run since U3d, poeurl since U8c: with no transport (a test's default) the request is refused and counted, no note is left.
/// What they do with an answer: the scene `net-tree` of `tests/import.rs`.
#[test]
fn unported_branches_do_not_crash() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED unported_branches_do_not_crash: {why}");
        return;
    }
    for (step, d) in run("u3c-crash").dumps {
        eprintln!("step {step}: popups {} {}", d["popups"], d["tab"]["popup"]["title"]);
    }
    let notes = pob_ui::modules::main::not_ported_notes();
    eprintln!("notes: {notes:#?}");
    for gone in ["TreeTab.lua:823-836", "TreeTab.lua:765-794"] {
        assert!(!notes.iter().any(|n| n.starts_with(gone)), "{gone}: {notes:?}");
    }
}

/// U3d: the toast and the report list's label at a given progress, TreeTab.lua:291-311.
#[test]
fn power_progress_toast_text() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED power_progress_toast_text: {why}");
        return;
    }
    let got = run("u3d-progress").progress;
    let line = |label: &str, toast: Option<&str>| (Some(label.to_string()), toast.map(str::to_string));
    let want = vec![
        line("Building Power Report... (0%)", Some("Building Power Report... (0%)")),
        // under 100 ms since the last: nothing changes
        line("Building Power Report... (0%)", Some("Building Power Report... (0%)")),
        line("Building Power Report... (42%)", Some("Building Power Report... (42%)")),
        line("Building Power Report...", Some("Building Power Report...")),
        line("Building Power Report... (100%)", Some("Building Power Report... (100%)")),
        // dismissed: the label goes on, the toast stays
        line("Building Power Report... (100%)", Some("Building Power Report... (100%)")),
    ];
    assert_eq!(got, want);
}

/// `(w, h)` of the JPEG / WebP sheets of the trees the scenes use.
fn lossy_sheet_sizes() -> std::collections::HashSet<(u32, u32)> {
    let mut out = std::collections::HashSet::new();
    for v in ["3_13", "3_28", "3_29"] {
        let dir = format!("{}/src/TreeData/{v}", scene::pob_root());
        for e in std::fs::read_dir(dir).expect("TreeData").flatten() {
            let p = e.path();
            if matches!(p.extension().and_then(|x| x.to_str()), Some("jpg" | "jpeg" | "webp"))
                && let Ok(d) = image::image_dimensions(&p)
            {
                out.insert(d);
            }
        }
    }
    out
}

/// Accepted by the user (2026-11-02): a pixel under a JPEG / WebP sheet may differ up to this much -- the decoders differ (JPEG measured 1:1 by
/// `jpeg_decoder_measure`: max Δ 4; WebP not measured separately). Everywhere else the tolerance stays 2/255.
const LOSSY_MAX_DELTA: u8 = 4;

/// The pixels beyond the tolerance: how many are in the lossy class (a lossy sheet among the quads covering them, Δ <= [`LOSSY_MAX_DELTA`]), and the others (`[x, y, Δ]`).
fn classify_bad(
    rgba: &[u8],
    oracle: &image::RgbaImage,
    list: &DrawList,
    lossy: &std::collections::HashSet<(u32, u32)>,
) -> (usize, Vec<[u32; 3]>) {
    let (w, h) = oracle.dimensions();
    let (mut n, mut other) = (0, Vec::new());
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let o = oracle.get_pixel(x, y).0;
            let m = (0..3).map(|k| rgba[i + k].abs_diff(o[k])).max().expect("3");
            if m <= 2 {
                continue;
            }
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let over = list.commands.iter().any(|c| {
                let (x0, x1) = c.pos.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[0]), a.1.max(p[0])));
                let (y0, y1) = c.pos.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[1]), a.1.max(p[1])));
                px >= x0 && px <= x1 && py >= y0 && py <= y1 && lossy.contains(&(c.texture.width, c.texture.height))
            });
            if over && m <= LOSSY_MAX_DELTA {
                n += 1;
            } else {
                other.push([x, y, u32::from(m)]);
            }
        }
    }
    (n, other)
}

/// `scenes/tree/index.json`; `POB_TREE_SCENES=a,b`: a run over those only (a debugging aid: the gate is the run without it).
fn scenario_names() -> Vec<String> {
    if let Some(only) = std::env::var("POB_TREE_SCENES").ok().filter(|s| !s.is_empty()) {
        return only.split(',').map(str::to_owned).collect();
    }
    serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join("scenes/tree/index.json")).expect("index")).expect("json")
}

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

/// What the gates read that is not in git (personal builds and their records, `.gitignore`): the reason to skip, else `None`.
fn missing_inputs() -> Option<String> {
    let fx = scene::oracle_dir().join("fixtures/tree/static.json");
    if !fx.exists() {
        return Some(format!(
            "{} is missing: run tools/sg-oracle/run.ps1 -Script tree-runner.lua on a machine with builds/pc",
            fx.display()
        ));
    }
    let Some(first) = local_builds("tree").into_iter().next() else {
        return Some("builds/local-builds.json is missing (personal builds)".to_owned());
    };
    let b = format!("{BUILDS}{first}.xml");
    if !std::path::Path::new(&b).exists() {
        return Some(format!("{b} is missing (personal builds)"));
    }
    None
}

#[test]
fn tree_states_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED tree_states_equal_original: {why}");
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
        let want = rec["dumps"].as_array().expect("dumps");
        assert_eq!(run.dumps.len(), want.len(), "{name}: number of dumps");
        let mut ndiff = 0;
        for ((s1, got), w) in run.dumps.iter().zip(want) {
            assert_eq!(*s1, w["step"].as_u64().expect("step"));
            if let Some(d) = first_diff(&norm(got), &norm(&oracle_state(&w["state"])), "") {
                ndiff += 1;
                failures.push(format!("{name} step {s1}: {d}"));
            }
        }
        // the orders of the hash tables the view walks
        for (o, ours) in &run.orders {
            let theirs = oracle_record(o);
            std::fs::write(
                scene::out_dir().join(format!("{o}.ours.json")),
                serde_json::to_string(ours).expect("json"),
            )
            .expect("write");
            for key in ["groups", "connectors", "subGraphs", "nodes"] {
                let (mut a, mut b) = (norm(&ours[key]), norm(&theirs[key]));
                // `pairs(tree.nodes)` differs per process of the original (R13): the same set. `subGraphs` too (R31; 24 processes of the golden: 23 + 1 of 2 orders, the
                // insertion follows `pairs(tree.sockets)`). `groups` (number keys only): the order
                if matches!(key, "connectors" | "nodes" | "subGraphs") {
                    for v in [&mut a, &mut b] {
                        if let Some(l) = v.as_array_mut() {
                            l.sort_by_key(|x| x.to_string());
                        }
                    }
                }
                let (a, b) = (&a, &b);
                if norm(a) != norm(b) {
                    let (av, bv) = (a.as_array().map_or(0, Vec::len), b.as_array().map_or(0, Vec::len));
                    let same = a
                        .as_array()
                        .zip(b.as_array())
                        .map_or(0, |(x, y)| x.iter().zip(y).filter(|(p, q)| norm(p) == norm(q)).count());
                    eprintln!("{o}: {key} order differs: {same} of {av}/{bv} at the same place");
                    failures.push(format!("{o}: {key} order: {same} of {av}/{bv} at the same place"));
                }
            }
        }
        for (o, ours) in &run.paths {
            let theirs = oracle_record(o);
            let (mut same, mut other) = (0, Vec::new());
            let empty = Map::new();
            let (a, b) = (
                ours["paths"].as_object().unwrap_or(&empty),
                theirs["paths"].as_object().unwrap_or(&empty),
            );
            for k in a.keys().chain(b.keys().filter(|k| !a.contains_key(*k))) {
                if norm(&a.get(k).cloned().unwrap_or(Value::Null)) == norm(&b.get(k).cloned().unwrap_or(Value::Null)) {
                    same += 1;
                } else {
                    other.push(k.clone());
                }
            }
            eprintln!(
                "{o}: node.path {same} equal, {} differ; pairs(allocNodes) {}",
                other.len(),
                if norm(&ours["allocOrder"]) == norm(&theirs["allocOrder"]) {
                    "equal"
                } else {
                    "differs"
                }
            );
            if !other.is_empty() {
                failures.push(format!(
                    "{o}: node.path differs on {} nodes: {:?}",
                    other.len(),
                    &other[..other.len().min(8)]
                ));
            }
            if norm(&ours["allocOrder"]) != norm(&theirs["allocOrder"]) {
                failures.push(format!("{o}: pairs(allocNodes) order differs"));
            }
        }
        eprintln!(
            "{name}: {} dumps, {ndiff} differ; unimplemented branches hit: {:?}",
            want.len(),
            run.unimplemented
        );
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
    }
    assert!(failures.is_empty(), "{} differ:\n{}", failures.len(), failures.join("\n"));
}

/// Frames the user accepted (2026-11-02), with the pixels allowed outside the lossy class; the text stays gated. `tab-heat-start`: the first frame of the node power walk: under the
/// scene's frozen clock the original finishes it within that frame, this port runs it in the background and the nodes have no colour yet. The next frames (`tab-heat`, `tab-heat-life`) are equal.
const ACCEPTED_FRAMES: &[(&str, usize)] = &[("tab-heat-start", 3616)];

#[test]
#[ignore = "needs a GPU adapter (or POB_GFX_WARP=1); run with --ignored"]
fn tree_pixels_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED tree_pixels_equal_original: {why}");
        return;
    }
    let mut r = pob_gfx::Renderer::new_offscreen().expect("wgpu adapter");
    let mut failures = Vec::new();
    let lossy_sizes = lossy_sheet_sizes();
    let (mut total_lossy, mut total_other) = (0, 0);
    for name in scenario_names() {
        for Shot { name: shot, list, mask } in run(&name).shots {
            let mut rgba = r.render_to_rgba(&list).expect("render");
            // POB_TREE_SAVE_OURS=<dir>;<shot>,<shot>: this port's PNG of those shots
            if let Ok(s) = std::env::var("POB_TREE_SAVE_OURS")
                && let Some((dir, shots)) = s.split_once(';')
                && shots.split(',').any(|x| x == shot)
            {
                std::fs::create_dir_all(dir).expect("mkdir");
                scene::save(
                    &scene::crop(&rgba, scene::SCREEN.0, scene::SCREEN.0, scene::SCREEN.1),
                    &PathBuf::from(dir).join(format!("{shot}.png")),
                );
            }
            let p = scene::oracle_dir().join(format!("fixtures/tree/{shot}.png"));
            let mut oracle = image::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_rgba8();
            assert_eq!(oracle.dimensions(), scene::SCREEN, "{shot}: oracle size");
            let (w, h) = scene::SCREEN;
            // a new scene's rows that follow LuaJIT's unstable sort over a per-process `pairs` (R13/L17): `report` of the state gate compares them as a set
            if let Some([x0, y0, x1, y1]) = mask {
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = ((y * w + x) * 4) as usize;
                        rgba[i..i + 3].copy_from_slice(&[0, 0, 0]);
                        oracle.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                    }
                }
            }
            if let Ok(pts) = std::env::var("POB_TREE_PROBE") {
                // debug: the commands covering a pixel, for the class of a pixel difference ("shot:x:y,...")
                for q in pts.split(',') {
                    let t: Vec<&str> = q.split(':').collect();
                    if t[0] == shot {
                        let (px, py): (f32, f32) = (t[1].parse::<f32>().unwrap() + 0.5, t[2].parse::<f32>().unwrap() + 0.5);
                        for c in &list.commands {
                            let (x0, x1) = c.pos.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[0]), a.1.max(p[0])));
                            let (y0, y1) = c.pos.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[1]), a.1.max(p[1])));
                            if px >= x0 && px <= x1 && py >= y0 && py <= y1 {
                                eprintln!(
                                    "  {shot} {px},{py}: L{}:{} tex {}x{} pos {:?} uv {:?} tint {:?}",
                                    c.layer, c.sub_layer, c.texture.width, c.texture.height, c.pos[0], c.uv, c.tint
                                );
                            }
                        }
                    }
                }
            }
            let mask = scene::text_mask(&list, w, h);
            let (d, heat) = scene::diff(&rgba, w, &oracle, &mask);
            eprintln!(
                "{shot:<28} bad {:>6} ({:.4}%) maxΔ {:>3} | text px {:>6} bad {:>4} maxΔ {:>3} | bbox {:?}",
                d.bad,
                d.frac() * 100.0,
                d.max_delta,
                d.text_pixels,
                d.text_bad,
                d.text_max_delta,
                d.bbox
            );
            if d.bad > 0 {
                let (lossy, other) = classify_bad(&rgba, &oracle, &list, &lossy_sizes);
                eprintln!(
                    "  {shot}: pixels off over a lossy sheet (jpg/webp, the decoders differ: test jpeg_decoder_measure) {lossy}, others {}: {:?}",
                    other.len(),
                    &other[..other.len().min(6)]
                );
                total_lossy += lossy;
                total_other += other.len();
                if other.is_empty() && d.text_bad == 0 {
                    continue;
                }
                if d.text_bad == 0 && ACCEPTED_FRAMES.iter().any(|&(s, max)| s == shot && other.len() <= max) {
                    eprintln!("  {shot}: an accepted exception, {} px outside the lossy class", other.len());
                    continue;
                }
                let out = scene::out_dir();
                scene::save(&heat, &out.join(format!("{shot}.heat.png")));
                scene::save(&scene::crop(&rgba, w, w, h), &out.join(format!("{shot}.ours.png")));
                failures.push(format!("{shot}: {} px off (text {}), bbox {:?}", d.bad, d.text_bad, d.bbox));
            }
        }
    }
    eprintln!("all shots: pixels in the lossy class {total_lossy}, others {total_other}");
    assert!(failures.is_empty(), "the tree differs from the original:\n{}", failures.join("\n"));
}

/// The class of the Δ ≤ 12 pixels of the tree shots: the original's JPEG decoder vs `image`'s, measured 1:1 on the sprite sheets (no scaling, no blending).
#[test]
#[ignore = "a measurement over the checkout's sheets; run with --ignored --nocapture"]
fn jpeg_decoder_measure() {
    let shot = image::open(scene::oracle_dir().join("fixtures/scenes/u3a-jpg.png"))
        .expect("shot")
        .to_rgb8();
    let root = scene::pob_root();
    for (name, x0) in [("skills-disabled-3.jpg", 0u32), ("skills-3.jpg", 500)] {
        let sheet = image::open(format!("{root}/src/TreeData/3_29/{name}")).expect("sheet").to_rgb8();
        let (mut n, mut bad, mut sum, mut max) = (0u64, 0u64, 0u64, 0u8);
        let mut hist = [0u64; 8];
        for y in 0..780 {
            for x in 0..500 {
                let (a, b) = (shot.get_pixel(x0 + x, y).0, sheet.get_pixel(x, y).0);
                let d = (0..3).map(|i| a[i].abs_diff(b[i])).max().expect("3");
                n += 1;
                sum += u64::from(d);
                max = max.max(d);
                bad += u64::from(d > 2);
                hist[usize::from(d.min(7))] += 1;
            }
        }
        eprintln!(
            "{name}: {n} px, Δ>2: {bad}, max Δ {max}, mean Δ {:.3}, hist Δ0..7+ {hist:?}",
            sum as f64 / n as f64
        );
    }
}

/// The bare quads of `scenes/<name>.json` (no tree, no port code) in `pob-gfx` vs the original: `POB_BARE=<scene>`.
#[test]
#[ignore = "a measurement; run with --ignored --nocapture"]
fn bare_quads_measure() {
    let name = std::env::var("POB_BARE").unwrap_or_else(|_| "u3a-bare".into());
    let sc = scene::Scene::load(&name);
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), sc.size, 1.0, Box::new(TestPlatform::new())).expect("fonts");
    let (list, _) = sc.run(&mut gfx);
    let mut r = pob_gfx::Renderer::new_offscreen().expect("wgpu adapter");
    eprintln!("adapter: {:?}", r.adapter_info());
    let rgba = r.render_to_rgba(&list).expect("render");
    let oracle = scene::load_oracle_png(&name);
    let (w, h) = sc.crop();
    let mask = scene::text_mask(&list, w, h);
    let (d, heat) = scene::diff(&rgba, sc.size.0, &oracle, &mask);
    eprintln!(
        "{name}: bad {} ({:.4}%) maxΔ {} bbox {:?}",
        d.bad,
        d.frac() * 100.0,
        d.max_delta,
        d.bbox
    );
    for (zone, x0, y0, x1, y1) in [
        ("headers", 0u32, 0u32, 145u32, 350u32),
        ("sep", 150, 0, 660, 40),
        ("lines", 145, 45, 590, 395),
        ("orbits", 590, 45, 1000, 395),
        ("frac-tiles", 0, 395, 1000, 450),
        ("sheet", 0, 455, 1000, 560),
    ] {
        let (mut n, mut bad, mut max) = (0u64, 0u64, 0u8);
        for y in y0..y1.min(h) {
            for x in x0..x1.min(w) {
                let i = ((y * sc.size.0 + x) * 4) as usize;
                let o = oracle.get_pixel(x, y).0;
                let m = (0..3).map(|k| rgba[i + k].abs_diff(o[k])).max().expect("3");
                n += 1;
                bad += u64::from(m > 2);
                max = max.max(m);
            }
        }
        eprintln!(
            "  {zone:<11} px {n:>7} bad {bad:>6} ({:.4}%) maxΔ {max}",
            bad as f64 * 100.0 / n as f64
        );
    }
    scene::save(&heat, &scene::out_dir().join(format!("{name}.heat.png")));
    scene::save(
        &scene::crop(&rgba, sc.size.0, w, h),
        &scene::out_dir().join(format!("{name}.ours.png")),
    );
}
/// Which sampling the original does: scene `u3a-model` against exact bilinear models computed from the decoded PNG.
#[test]
#[ignore = "a measurement; run with --ignored --nocapture"]
fn sampling_model_measure() {
    let oracle = scene::load_oracle_png("u3a-model");
    let root = scene::pob_root();
    let img = image::open(format!("{root}/src/Assets/masteryheaderallocatedmiddle.png"))
        .expect("png")
        .to_rgba8();
    let (tw, th) = (img.width() as i64, img.height() as i64);
    let tex = |x: i64, y: i64, premul: bool| -> [f64; 4] {
        let p = img.get_pixel(x.rem_euclid(tw) as u32, y.rem_euclid(th) as u32).0;
        let a = f64::from(p[3]) / 255.0;
        let k = if premul { a } else { 1.0 };
        [
            f64::from(p[0]) / 255.0 * k,
            f64::from(p[1]) / 255.0 * k,
            f64::from(p[2]) / 255.0 * k,
            a,
        ]
    };
    let sample = |u: f64, v: f64, premul: bool| -> [f64; 4] {
        let (x, y) = (u * tw as f64 - 0.5, v * th as f64 - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (
            tex(x0, y0, premul),
            tex(x0 + 1, y0, premul),
            tex(x0, y0 + 1, premul),
            tex(x0 + 1, y0 + 1, premul),
        );
        std::array::from_fn(|k| (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy)
    };
    for (label, x0, y0, w, h, bg) in [
        ("1:1 black", 10u32, 10u32, 87u32, 83u32, 0.0),
        ("19x38 black", 110, 10, 19, 38, 0.0),
        ("19x83 black", 140, 10, 19, 83, 0.0),
        ("87x38 black", 170, 10, 87, 38, 0.0),
        ("1:1 white", 310, 10, 87, 83, 1.0),
        ("19x38 white", 410, 10, 19, 38, 1.0),
        ("19x83 white", 440, 10, 19, 83, 1.0),
        ("87x38 white", 470, 10, 87, 38, 1.0),
    ] {
        for premul in [false, true] {
            let (mut bad, mut max, mut n) = (0u32, 0u32, 0u32);
            for y in y0..y0 + h {
                for x in x0..x0 + w {
                    let u = (f64::from(x - x0) + 0.5) / f64::from(w);
                    let v = (f64::from(y - y0) + 0.5) / f64::from(h);
                    let s = sample(u, v, premul);
                    let o = oracle.get_pixel(x, y).0;
                    for k in 0..3 {
                        let col = if premul {
                            s[k] + bg * (1.0 - s[3])
                        } else {
                            s[k] * s[3] + bg * (1.0 - s[3])
                        };
                        let m = ((col * 255.0).round() - f64::from(o[k])).abs() as u32;
                        max = max.max(m);
                        if m > 2 {
                            bad += 1;
                        }
                    }
                    n += 1;
                }
            }
            eprintln!(
                "{label:<12} {} : px {n:>5}, channels off >2: {bad:>5}, maxΔ {max}",
                if premul { "premultiplied" } else { "straight     " }
            );
        }
    }
}
/// Fits models of the original's minification on scene `u3a-model`: a texel offset, a box-filtered mip chain.
#[test]
#[ignore = "a measurement; run with --ignored --nocapture"]
fn sampling_fit_measure() {
    let oracle = scene::load_oracle_png("u3a-model");
    let root = scene::pob_root();
    let img = image::open(format!("{root}/src/Assets/masteryheaderallocatedmiddle.png"))
        .expect("png")
        .to_rgba8();
    // mip chain: straight RGBA, box 2x2 of the level above (dimensions floor(n/2), GL rule; odd sizes take the box of the first pixels)
    let mut levels: Vec<(i64, i64, Vec<[f64; 4]>)> = Vec::new();
    let (tw, th) = (i64::from(img.width()), i64::from(img.height()));
    levels.push((
        tw,
        th,
        img.pixels().map(|p| std::array::from_fn(|k| f64::from(p.0[k]) / 255.0)).collect(),
    ));
    while levels.last().expect("l").0 > 1 || levels.last().expect("l").1 > 1 {
        let (w, h, px) = levels.last().expect("l").clone();
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut out = Vec::with_capacity((nw * nh) as usize);
        for y in 0..nh {
            for x in 0..nw {
                // the centre of a new texel in the old level, bilinear (what a linear blit does)
                let fx = (x as f64 + 0.5) * w as f64 / nw as f64 - 0.5;
                let fy = (y as f64 + 0.5) * h as f64 / nh as f64 - 0.5;
                let (x0, y0) = (fx.floor(), fy.floor());
                let (ax, ay) = (fx - x0, fy - y0);
                let at = |i: i64, j: i64| px[(j.clamp(0, h - 1) * w + i.clamp(0, w - 1)) as usize];
                let (x0, y0) = (x0 as i64, y0 as i64);
                let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
                out.push(std::array::from_fn(|k| {
                    (a[k] * (1.0 - ax) + b[k] * ax) * (1.0 - ay) + (c[k] * (1.0 - ax) + d[k] * ax) * ay
                }));
            }
        }
        levels.push((nw, nh, out));
    }
    let sample = |l: usize, u: f64, v: f64| -> [f64; 4] {
        let (w, h, px) = &levels[l];
        let (x, y) = (u * *w as f64 - 0.5, v * *h as f64 - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let at = |i: i64, j: i64| px[(j.rem_euclid(*h) * w + i.rem_euclid(*w)) as usize];
        let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
        std::array::from_fn(|k| (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy)
    };
    for (label, x0, y0, w, h) in [
        ("19x38", 110u32, 10u32, 19u32, 38u32),
        ("19x83", 140, 10, 19, 83),
        ("87x38", 170, 10, 87, 38),
    ] {
        let err = |f: &dyn Fn(f64, f64) -> [f64; 4]| -> (u32, f64) {
            let (mut max, mut sum) = (0u32, 0.0);
            for y in y0..y0 + h {
                for x in x0..x0 + w {
                    let u = (f64::from(x - x0) + 0.5) / f64::from(w);
                    let v = (f64::from(y - y0) + 0.5) / f64::from(h);
                    let s = f(u, v);
                    let o = oracle.get_pixel(x, y).0;
                    for k in 0..3 {
                        let m = ((s[k] * s[3] * 255.0).round() - f64::from(o[k])).abs();
                        max = max.max(m as u32);
                        sum += m;
                    }
                }
            }
            (max, sum / f64::from(w * h * 3))
        };
        let (mut best, mut best_at) = ((u32::MAX, f64::MAX), (0.0, 0.0));
        for i in -20..=20 {
            for j in -20..=20 {
                let (du, dv) = (f64::from(i) * 0.05 / tw as f64, f64::from(j) * 0.05 / th as f64);
                let e = err(&|u, v| sample(0, u + du, v + dv));
                if e.1 < best.1 {
                    best = e;
                    best_at = (f64::from(i) * 0.05, f64::from(j) * 0.05);
                }
            }
        }
        eprintln!(
            "{label}: L0 exact {:?}; best texel offset {best_at:?}: {best:?}",
            err(&|u, v| sample(0, u, v))
        );
        for l in 1..4 {
            eprintln!(
                "{label}:   mip L{l} {:?}; trilinear L{}..L{l} at .5 {:?}",
                err(&|u, v| sample(l, u, v)),
                l - 1,
                err(&|u, v| {
                    let (a, b) = (sample(l - 1, u, v), sample(l, u, v));
                    std::array::from_fn(|k| (a[k] + b[k]) / 2.0)
                })
            );
        }
    }
}
/// For the pixels of `u3a-model` the exact bilinear misses: the local texel offset that would explain the original's value.
#[test]
#[ignore = "a measurement; run with --ignored --nocapture"]
fn sampling_pixels_measure() {
    let oracle = scene::load_oracle_png("u3a-model");
    let root = scene::pob_root();
    let img = image::open(format!("{root}/src/Assets/masteryheaderallocatedmiddle.png"))
        .expect("png")
        .to_rgba8();
    let (tw, th) = (i64::from(img.width()), i64::from(img.height()));
    let tex = |x: i64, y: i64| -> [f64; 4] {
        let p = img.get_pixel(x.rem_euclid(tw) as u32, y.rem_euclid(th) as u32).0;
        std::array::from_fn(|k| f64::from(p[k]))
    };
    let sample = |x: f64, y: f64| -> [f64; 4] {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (tex(x0, y0), tex(x0 + 1, y0), tex(x0, y0 + 1), tex(x0 + 1, y0 + 1));
        std::array::from_fn(|k| (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy)
    };
    let (x0, y0, w, h) = (170u32, 10u32, 87u32, 38u32);
    let mut shown = 0;
    let mut by_row: std::collections::BTreeMap<u32, (u32, u32)> = Default::default();
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let tx = (f64::from(x - x0) + 0.5) / f64::from(w) * tw as f64 - 0.5;
            let ty = (f64::from(y - y0) + 0.5) / f64::from(h) * th as f64 - 0.5;
            let s = sample(tx, ty);
            let o = oracle.get_pixel(x, y).0;
            let off = |s: [f64; 4]| {
                (0..3)
                    .map(|k| ((s[k] * s[3] / 255.0).round() - f64::from(o[k])).abs() as u32)
                    .max()
                    .expect("3")
            };
            let e0 = off(s);
            let r = by_row.entry(y - y0).or_default();
            r.0 += 1;
            if e0 > 2 {
                r.1 += 1;
                if shown < 24 && e0 > 8 {
                    shown += 1;
                    let (mut best, mut at) = (u32::MAX, (0.0, 0.0));
                    for i in -40..=40 {
                        for j in -40..=40 {
                            let (dx, dy) = (f64::from(i) * 0.025, f64::from(j) * 0.025);
                            let e = off(sample(tx + dx, ty + dy));
                            let better = e < best || (e == best && dx.abs() + dy.abs() < f64::abs(at.0) + f64::abs(at.1));
                            if better {
                                best = e;
                                at = (dx, dy);
                            }
                        }
                    }
                    eprintln!(
                        "px {},{} tex {tx:.3},{ty:.3} frac {:.3},{:.3} Δ{e0} o={:?} s={:?} -> best Δ{best} at {at:?}",
                        x - x0,
                        y - y0,
                        tx - tx.floor(),
                        ty - ty.floor(),
                        &o[..3],
                        s.map(|v| v.round() as i32)
                    );
                }
            }
        }
    }
    eprintln!(
        "bad per row (frac y): {:?}",
        by_row.iter().map(|(r, (_, b))| (*r, *b)).collect::<Vec<_>>()
    );
}
/// Two PNGs of this port for the user (`POB_SHOTS_DIR`, a folder outside git): the static tree at zoom level 1 and a node tooltip.
#[test]
#[ignore = "writes files; POB_SHOTS_DIR=<dir> cargo test -p pob-ui --release --test tree user_screenshots -- --ignored"]
fn user_screenshots() {
    let dir = PathBuf::from(std::env::var("POB_SHOTS_DIR").expect("POB_SHOTS_DIR"));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let run = run("usershots");
    let mut r = pob_gfx::Renderer::new_offscreen().expect("wgpu adapter");
    let (w, h) = scene::SCREEN;
    for Shot { name, list, .. } in run.shots {
        let rgba = r.render_to_rgba(&list).expect("render");
        scene::save(&scene::crop(&rgba, w, w, h), &dir.join(format!("{name}.png")));
    }
    for (step, d) in &run.dumps {
        eprintln!("step {step}: zoomLevel {}, hover {}", d["tree"]["zoomLevel"], d["tree"]["hover"]);
    }
}
