//! U8d gate: the original `TradeQuery` / `CompareBuySimilar` (the whole PoB, run by the SG oracle `tools/sg-oracle/lua/trade-runner.lua`) vs this port, on a public golden, over replies written by hand
//! (`tools/sg-oracle/scenes/trade/*.json`): no site is asked on either side.
//! - `trade_states_equal_original` (no GPU): every `dump` step = the oracle's. A URL with a gzip code and a JSON body are compared decoded: the keys of `dkjson.encode` come in `pairs` order (R13), and the two gzip streams differ.
//! - `trade_pixels_equal_original` (GPU/WARP, `#[ignore]`): every screenshot = the original's PNG; 0 pixels beyond 2/255 outside the lossy class.
//!   cargo test -p pob-ui --release --test trade -- --include-ignored --nocapture

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
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../pob-calc/tests/fixtures/party-synth/party-attack-full.xml"
);

/// What differs by the stream or by `pairs` becomes the same value on both sides.
fn canon(v: &Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => Value::from(f as i64),
            _ => v.clone(),
        },
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), canon(x))).collect()),
        // dkjson writes an empty table as an array
        Value::Array(a) if a.is_empty() => Value::Object(serde_json::Map::new()),
        Value::Array(a) => Value::Array(a.iter().map(canon).collect()),
        Value::String(s) => canon_str(s),
        _ => v.clone(),
    }
}

fn canon_str(s: &str) -> Value {
    // its `state` and `code_challenge` are random on both sides
    if s.contains("/oauth/authorize?") {
        return Value::String("the login page".to_string());
    }
    if s.starts_with('{') {
        if let Ok(j) = serde_json::from_str::<Value>(s) {
            return canon(&j);
        }
        return Value::String(s.to_string());
    }
    if s.starts_with("http") {
        // `url|header|body` of the log
        let mut it = s.splitn(3, '|');
        if let (Some(u), Some(h), Some(b)) = (it.next(), it.next(), it.next()) {
            return json!([canon_str(u), h, canon_str(b)]);
        }
        if s.contains("/trade/search/")
            && let Some((head, code)) = s.rsplit_once('/')
            && let Some(text) = pob_ui::trade::helpers::b64_gzip_decode(code)
            && let Ok(j) = serde_json::from_str::<Value>(&text)
        {
            return json!({"url": head, "query": canon(&j)});
        }
    }
    Value::String(s.to_string())
}

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("pob-ui-u8d-{}-{n}-{name}", std::process::id()));
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

/// The fake transport of a scene (the same list as `trade-runner.lua`): written by hand, no site was asked.
struct NetScript(std::sync::Arc<pob_net::ScriptedTransport>);

impl NetScript {
    fn new(sc: &Value) -> Self {
        let t = std::sync::Arc::new(pob_net::ScriptedTransport::default());
        for r in sc["net"].as_array().into_iter().flatten() {
            let url = r["url"].as_str().expect("url");
            if url == "oauth" {
                let o = &r["oauth"];
                t.set_oauth(pob_net::OauthServerResult {
                    code: o["code"].as_str().map(str::to_owned),
                    err: o["err"].as_str().map(str::to_owned),
                    state: o["state"].as_str().map(str::to_owned),
                    port: o["port"].as_u64().unwrap_or(0) as u16,
                });
                continue;
            }
            let reply = match r["error"].as_str() {
                Some(e) => Err(e.to_owned()),
                None => Ok(pob_net::Reply {
                    code: r["code"].as_u64().expect("code") as u32,
                    header: r["header"].as_str().unwrap_or("").to_owned(),
                    body: r["body"].as_str().unwrap_or("").as_bytes().to_vec(),
                }),
            };
            t.add(url, reply);
        }
        Self(t)
    }

    fn log(&self) -> Vec<String> {
        self.0
            .log
            .lock()
            .expect("log")
            .iter()
            .map(|r| {
                let body = r.body.as_ref().map_or(String::new(), |b| String::from_utf8_lossy(b).into_owned());
                let body = match body.find("code_verifier=") {
                    Some(i) => format!("{}code_verifier=*", &body[..i]),
                    None => body,
                };
                format!("{}|{}|{body}", r.url, r.header.as_deref().unwrap_or(""))
            })
            .collect()
    }
}

fn dump(g: &Gfx, tp: &TestPlatform, net: &NetScript, want_xml: bool) -> Value {
    let m = main();
    let b = m.build_mode.clone();
    let mut d = serde_json::Map::new();
    d.insert("mode".into(), json!(m.mode.borrow().clone().unwrap_or_else(|| "nil".into())));
    d.insert("popups".into(), json!(m.popups.borrow().len()));
    if let Some(p) = m.popups.borrow().first() {
        d.insert("popupTitle".into(), json!(p.title.get(g)));
        let (x, y) = p.get_pos(g);
        let (w, h) = p.get_size(g);
        let n = pob_lua::tostring_num;
        d.insert("popupRect".into(), json!(format!("{},{},{},{}", n(x), n(y), n(w), n(h))));
        d.insert("popup".into(), pob_ui::tabs::import::dump_host(&p.host, g));
    }
    let prompt = pob_ui::launch::launch().prompt_msg.borrow().clone().unwrap_or_else(|| "nil".into());
    d.insert("prompt".into(), json!(prompt));
    let st = tp.state();
    d.insert("clipboard".into(), json!(st.clipboard.unwrap_or_else(|| "nil".to_string())));
    d.insert("opened".into(), json!(st.opened));
    d.insert("netLog".into(), json!(net.log()));
    d.insert("netReal".into(), json!(0));
    if let Some(t) = b.items_tab.borrow().clone() {
        let q = t.trade_query.borrow().clone().expect("tradeQuery");
        d.insert("trade".into(), q.debug_dump(g));
        d.insert("authorized".into(), json!(pob_ui::poe_api::is_authorized()));
        d.insert("unsaved".into(), json!(b.unsaved.get()));
        d.insert("viewMode".into(), json!(*b.view_mode.borrow()));
    }
    if want_xml {
        d.insert("xml".into(), json!(b.save_db_text().expect("SaveDB").replace("\r\n", "\n")));
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
    let popup_ctl = |s: &str| {
        let p = main().popups.borrow().first().cloned().expect("popup");
        p.host.get(s).unwrap_or_else(|| panic!("popup control {s}"))
    };
    if let Some(s) = st["cursorBuild"].as_str() {
        let c = b.host.get(s).unwrap_or_else(|| panic!("control {s}"));
        *cur = center(gfx, &*c);
    } else if let Some(s) = st["cursorCompare"].as_str() {
        let tab = b.compare_tab.borrow().clone().expect("compareTab");
        let c = tab.host.get(s).unwrap_or_else(|| panic!("compare control {s}"));
        *cur = center(gfx, &*c);
    } else if let Some(c) = st["compareItemButton"].as_array() {
        let tab = b.compare_tab.borrow().clone().expect("compareTab");
        let start = 312.0 + tab.items_col_width.get().expect("colWidth") + 10.0 - tab.c().items_h_scroll_bar.offset.get() + 310.0 + 6.0;
        let dx = match c[1].as_str() {
            Some("copy") => 94.0,
            Some("buy") => 30.0,
            _ => 158.0,
        };
        *cur = (
            start + dx,
            236.0 + 4.0 - tab.scroll_y.get() + 20.0 * c[0].as_f64().expect("row") + 10.0,
        );
    } else if let Some(s) = st["cursorItems"].as_str() {
        let t = b.items_tab.borrow().clone().expect("itemsTab");
        let c = t.debug_control(s).unwrap_or_else(|| panic!("control {s}"));
        *cur = center(gfx, &*c);
    } else if let Some(s) = st["cursorPopup"].as_str() {
        *cur = center(gfx, &*popup_ctl(s));
    } else if let Some(c) = st["cursorPopupAt"].as_array() {
        let (x, y) = popup_ctl(c[0].as_str().expect("control")).get_pos(gfx);
        *cur = (x + c[1].as_f64().expect("x"), y + c[2].as_f64().expect("y"));
    } else if let Some(c) = st["cursorPopupDrop"].as_array() {
        let ctl = popup_ctl(c[0].as_str().expect("control"));
        let (x, y) = ctl.get_pos(gfx);
        let (w, h) = ctl.get_size(gfx);
        let row = c[1].as_f64().expect("row");
        *cur = ((x + w / 2.0).floor(), y + h + 2.0 + (row - 1.0) * (h - 4.0) + 4.0);
    } else {
        return;
    }
    gfx.input(RawEvent::Cursor { x: cur.0, y: cur.1 });
}

struct Run {
    dumps: Vec<(u64, Value)>,
    shots: Vec<(String, DrawList)>,
    ms: Vec<(u64, f64)>,
}

fn oracle_record(name: &str) -> Option<Value> {
    let p = scene::oracle_dir().join(format!("fixtures/trade/{name}.json"));
    Some(serde_json::from_str(&std::fs::read_to_string(p).ok()?).expect("oracle json"))
}

/// R13: the orders of the process the oracle's record came from.
fn set_samples(name: &str) {
    let rec = oracle_record(name);
    let list = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect()
    };
    pob_ui::trade::set_realm_order_sample(rec.as_ref().map(|r| list(&r["realmOrder"])).filter(|l| !l.is_empty()));
    pob_ui::trade::set_frame_order_sample(rec.as_ref().map(|r| list(&r["frameOrder"])).filter(|l| !l.is_empty()));
    pob_ui::trade::generator::set_mod_order_sample(rec.as_ref().and_then(|r| {
        r["modOrder"].as_object().map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), list(v)))
                .collect::<HashMap<String, Vec<String>>>()
        })
    }));
}

fn run(name: &str) -> Run {
    set_samples(name);
    let sc: Value =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join(format!("scenes/trade/{name}.json"))).expect("scenario"))
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
    let net = NetScript::new(&sc);
    l.set_transport(net.0.clone());
    l.net.set_inline(true);
    l.hold_sub_scripts(true);
    pob_ui::poe_api::set_os_time(None);
    pob_ui::trade::buy_similar::reset_module_state();
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
        ms: Vec::new(),
    };
    let mut cur = (-100.0, -100.0);
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
            frame(&mut gfx, &l);
            continue;
        }
        if let Some(a) = st["compareImport"].as_array() {
            let (file, label) = (a[0].as_str().expect("file"), a[1].as_str().expect("label"));
            let xml = std::fs::read_to_string(format!("{}{file}.xml", main().build_path.borrow())).expect("build");
            let tab = main().build_mode.compare_tab.borrow().clone().expect("compareTab");
            assert!(tab.import_build(&mut gfx, &xml, label), "import failed");
            frame(&mut gfx, &l);
            continue;
        }
        if let Some(x) = st["compareScrollX"].as_f64() {
            let tab = main().build_mode.compare_tab.borrow().clone().expect("compareTab");
            tab.c().items_h_scroll_bar.set_offset(x);
            frame(&mut gfx, &l);
            continue;
        }
        if !st["apiTokens"].is_null() {
            let t = &st["apiTokens"];
            let m = main();
            let mut a = m.accounts.borrow_mut();
            a.last_token = t["auth"].as_str().map(str::to_owned);
            a.last_refresh_token = t["refresh"].as_str().map(str::to_owned);
            a.token_expiry = t["expiry"].as_f64();
        }
        if let Some(t) = st["osTime"].as_f64() {
            pob_ui::poe_api::set_os_time(Some(t));
        }
        if let Some(a) = st["popupDropSel"].as_array() {
            let p = main().popups.borrow().first().cloned().expect("popup");
            let c: Rc<dyn std::any::Any> = p.host.get(a[0].as_str().expect("name")).expect("control");
            let d = c
                .downcast::<pob_ui::controls::drop_down::DropDownControl>()
                .unwrap_or_else(|_| panic!("a dropdown"));
            let at = d.list.borrow().iter().position(|v| v.label.as_deref() == a[1].as_str());
            if let Some(i) = at {
                d.set_sel(&mut gfx, i as i64 + 1, false);
            }
        }
        if let Some(raw) = st["displayItem"].as_str() {
            let t = main().build_mode.items_tab.borrow().clone().expect("itemsTab");
            t.create_display_item_from_raw(&mut gfx, raw, false);
        }
        if st["netDeliver"].as_bool() == Some(true) {
            // the OAuth server has a thread of its own
            l.net.wait_idle();
            l.poll_sub_scripts(&mut gfx);
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
        out.ms.push(((i + 1) as u64, t0.elapsed().as_secs_f64() * 1e3));
        if st["dump"].as_bool() == Some(true) {
            out.dumps
                .push(((i + 1) as u64, dump(&gfx, &tp, &net, st["xml"].as_bool() == Some(true))));
        }
        if let Some(shot) = st["shot"].as_str() {
            let list = frame(&mut gfx, &l);
            out.shots.push((shot.to_string(), list));
        }
    }
    pob_ui::modules::main::clear_main();
    pob_ui::trade::set_realm_order_sample(None);
    pob_ui::trade::set_frame_order_sample(None);
    pob_ui::trade::generator::set_mod_order_sample(None);
    out
}

fn scenario_names() -> Vec<String> {
    if let Ok(only) = std::env::var("POB_TRADE_SCENES") {
        return only.split(',').map(str::to_owned).collect();
    }
    serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join("scenes/trade/index.json")).expect("index")).expect("json")
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
                let (ta, tb) = (a.to_string(), b.to_string());
                let cut = |s: &str| s[..s.floor_char_boundary(1500)].to_string();
                return Some(format!(
                    "{path}: length oracle {} ours {}: oracle {}, ours {}",
                    x.len(),
                    y.len(),
                    cut(&ta),
                    cut(&tb)
                ));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (v, w))| first_diff(v, w, &format!("{path}[{i}]")))
        }
        (Value::String(x), Value::String(y)) if x != y && path == ".xml" => {
            let (la, lb): (Vec<&str>, Vec<&str>) = (x.lines().collect(), y.lines().collect());
            let keep = |l: &&str| !l.contains("<PlayerStat ") && !l.contains("<MinionStat ") && !l.contains("<FullDPSSkill ");
            let (ka, kb): (Vec<&str>, Vec<&str>) = (la.iter().copied().filter(keep).collect(), lb.iter().copied().filter(keep).collect());
            if ka == kb {
                return None;
            }
            let only_a: Vec<&&str> = ka.iter().filter(|l| !kb.contains(l)).take(8).collect();
            let only_b: Vec<&&str> = kb.iter().filter(|l| !ka.contains(l)).take(8).collect();
            Some(format!(
                "{path}: lines {}/{}: only the oracle {only_a:#?}, only ours {only_b:#?}",
                ka.len(),
                kb.len()
            ))
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

#[test]
fn no_scene_reaches_the_real_network() {
    let me = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/trade.rs")).expect("this file");
    assert!(!me.contains(concat!("Ureq", "Transport")));
    // and no token of a person: the scenes' are `synthetic-*`
    let dir = scene::oracle_dir().join("scenes/trade");
    for e in std::fs::read_dir(dir).expect("scenes").flatten() {
        let text = std::fs::read_to_string(e.path()).expect("scene");
        for needle in ["POESESSID", "Bearer ey"] {
            assert!(!text.contains(needle), "{}: {needle}", e.path().display());
        }
    }
}

/// `missing`: the oracle's records of this scene are absent.
#[test]
fn trade_states_equal_original() {
    let mut failures = Vec::new();
    let mut compared = 0;
    let mut skipped = Vec::new();
    for name in scenario_names() {
        let Some(rec) = oracle_record(&name) else {
            skipped.push(name);
            continue;
        };
        let r = run(&name);
        let dumps = rec["dumps"].as_array().expect("dumps");
        assert_eq!(dumps.len(), r.dumps.len(), "{name}: dump count");
        for (o, (step, ours)) in dumps.iter().zip(&r.dumps) {
            assert_eq!(o["step"].as_u64(), Some(*step), "{name}: steps");
            let s = &o["state"];
            assert!(s.is_object(), "{name} step {step}: {s}");
            assert_eq!(s["netReal"].as_i64(), Some(0), "{name} step {step}: the original reached curl");
            assert_eq!(
                s["writesRefused"].as_str(),
                Some(""),
                "{name} step {step}: a write into the checkout"
            );
            let mut s = canon(s);
            s.as_object_mut().expect("object").remove("writesRefused");
            compared += 1;
            if let Some(d) = first_diff(&s, &canon(ours), "") {
                failures.push(format!("{name} step {step}: {d}"));
            }
        }
    }
    eprintln!(
        "dumps compared {compared}, differ {}, scenes without a record {skipped:?}",
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} differences, the first 60:\n{}",
        failures.len(),
        failures[..failures.len().min(60)].join("\n")
    );
    assert!(compared > 0, "nothing compared");
}

/// The real window on both sides (trade-real-seq.ps1): the stages' state and the events the window delivered. Run the sides first:
///   pwsh tools/sg-oracle/run.ps1 -Script trade-runner.lua -ScriptArgs real-input -Inject inject-trade-real.ps1 -Timeout 40
///   pwsh tools/sg-oracle/trade-rust-input.ps1
#[test]
fn trade_real_input_equals_original() {
    let read = |n: &str| -> Option<Value> {
        let t = std::fs::read_to_string(scene::oracle_dir().join(format!("fixtures/trade/{n}.json"))).ok()?;
        Some(serde_json::from_str(t.trim_start_matches('\u{feff}')).expect("json"))
    };
    let (Some(o), Some(r)) = (read("real-input-oracle"), read("real-input-rust")) else {
        eprintln!("SKIPPED trade_real_input_equals_original: no fixtures/trade/real-input-*.json");
        return;
    };
    let mut events = 0;
    for stage in ["start", "tab", "popup", "typed", "weights", "row", "slider", "back", "closed"] {
        if let Some(d) = first_diff(&canon(&o[stage]["tradeReal"]), &canon(&r[stage]["tradeReal"]), stage) {
            panic!("{d}");
        }
        assert_eq!(o[stage]["events"], r[stage]["events"], "{stage}: events");
        events = o[stage]["events"].as_array().map_or(0, Vec::len);
    }
    assert_eq!(o["popup"]["tradeReal"]["popups"].as_i64(), Some(1), "the popup opened");
    assert_eq!(o["typed"]["tradeReal"]["fetchBuf"].as_str(), Some("32"), "the digit arrived");
    assert_eq!(o["weights"]["tradeReal"]["title"].as_str(), Some("Stat Weight Multipliers"));
    assert_eq!(o["closed"]["tradeReal"]["popups"].as_i64(), Some(0));
    eprintln!("9 stages, {events} events");
}

/// Numbers for the report.
#[test]
#[ignore = "numbers for a report"]
fn trade_perf() {
    for name in ["query-ring", "search"] {
        let r = run(name);
        let mut ms = r.ms.clone();
        ms.sort_by(|a, b| b.1.total_cmp(&a.1));
        eprintln!("rust {name}: the slowest steps {:?}", &ms[..ms.len().min(3)]);
    }
}

/// Two screenshots of this port for the report (`tools/.scratch/u8d-shots/`).
#[test]
#[ignore = "needs a wgpu adapter"]
fn trade_report_shots() {
    let mut r = pob_gfx::Renderer::new_offscreen().expect("wgpu adapter");
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/.scratch/u8d-shots"));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let (w, h) = scene::SCREEN;
    for (scenario, shot, file) in [
        ("search", "search-results", "trader.png"),
        ("buy-rare", "buy-rare", "buy-similar.png"),
    ] {
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
fn trade_pixels_equal_original() {
    let mut r = pob_gfx::Renderer::new_offscreen().expect("wgpu adapter");
    let mut failures = Vec::new();
    let mut shots = 0;
    for name in scenario_names() {
        if oracle_record(&name).is_none() {
            continue;
        }
        for (shot, list) in run(&name).shots {
            shots += 1;
            let rgba = r.render_to_rgba(&list).expect("render");
            let (w, h) = scene::SCREEN;
            let p = scene::oracle_dir().join(format!("fixtures/trade/{shot}.png"));
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
    assert!(failures.is_empty(), "the popups differ from the original:\n{}", failures.join("\n"));
}
