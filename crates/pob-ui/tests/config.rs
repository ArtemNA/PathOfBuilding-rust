//! U7 gate: the original `ConfigTab` / `ConfigSetListControl` / `ConfigModBrowser` / `NotesTab` (the whole PoB, run by the SG oracle `tools/sg-oracle/lua/config-runner.lua`) vs this
//! port, on the public goldens (`tools/sg-oracle/scenes/config/*.json`).
//! - `config_states_equal_original` (no GPU): every `dump` step = the oracle's; a saved build = the oracle's file.
//! - `config_pixels_equal_original` (GPU/WARP, `#[ignore]`): every screenshot = the original's PNG; 0 pixels beyond 2/255 outside the lossy class.
//!   cargo test -p pob-ui --release --test config -- --include-ignored --nocapture

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::{Button, DrawList, Gfx, RawEvent, TestPlatform};
use pob_ui::control::Control;
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::main;
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

/// A public golden with minions: OccVortex + three groups (the same edit as config-runner.lua).
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

/// U10c: the minion as the main skill and custom mods for the `if*` keys no golden reaches (the same edit as config-runner.lua).
fn dev_test() -> String {
    let x = spectre_test();
    let at = x.find("mainSocketGroup=\"").expect("mainSocketGroup") + "mainSocketGroup=\"".len();
    let end = at + x[at..].find('"').expect("quote");
    let x = format!("{}1{}", &x[..at], &x[end..]);
    let at = x.find("<Config").expect("Config");
    let end = at + x[at..].find('>').expect(">") + 1;
    let i = r#"<CustomModifierBlock title="T0">1% increased Damage per Siphoning Charge</CustomModifierBlock><CustomModifierBlock title="T1">1% increased Damage per Summoned Totem</CustomModifierBlock><CustomModifierBlock title="T2">10% increased Damage against enemies affected by 2 or more poisons</CustomModifierBlock><CustomModifierBlock title="T3">Minions deal 10% increased Damage while on Full Life</CustomModifierBlock>"#;
    format!("{}{i}{}", &x[..end], &x[end..])
}

/// A loadout over a second tree and a second config set (the same edit as config-runner.lua).
fn loadout_test() -> String {
    let x = std::fs::read_to_string(format!("{}/spec/TestBuilds/3.13/OccVortex.xml", scene::pob_root())).expect("golden");
    let a = x.find("<Spec ").expect("Spec");
    let b = a + x[a..].find("</Spec>").expect("/Spec") + "</Spec>".len();
    let spec = &x[a..b];
    let two = format!("{spec}\n{}", spec.replacen("<Spec ", "<Spec title=\"Alt\" ", 1));
    let x = format!("{}{two}{}", &x[..a], &x[b..]);
    let a = x.find("<Config").expect("Config");
    let b = a + x[a..].find("</Config>").expect("/Config") + "</Config>".len();
    format!(
        "{}<Config activeConfigSet=\"1\"><ConfigSet id=\"1\" title=\"Default\"/><ConfigSet id=\"2\" title=\"Alt\"><Input name=\"buffOnslaught\" boolean=\"true\"/></ConfigSet></Config>{}",
        &x[..a],
        &x[b..]
    )
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
        let root = std::env::temp_dir().join(format!("pob-ui-u7-{}-{n}-{name}", std::process::id()));
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
        // U10: a skill for the only option of type `float` (the same edit as config-runner.lua)
        let x = std::fs::read_to_string(format!("{}/spec/TestBuilds/3.13/OccVortex.xml", scene::pob_root())).expect("golden");
        let at = x.find("<Skills ").expect("<Skills");
        let end = at + x[at..].find('>').expect(">") + 1;
        let g = r#"<Skill mainActiveSkillCalcs="1" enabled="true" mainActiveSkill="1"><Gem level="20" gemId="Metadata/Items/Gems/SkillGemIntuitiveLink" skillId="IntuitiveLink" qualityId="Default" quality="0" enabled="true" nameSpec="Intuitive Link"/></Skill>"#;
        std::fs::write(builds.join("Float Test.xml"), format!("{}{g}{}", &x[..end], &x[end..])).expect("float build");
        for b in local_builds("config") {
            // a local copy next to the fixtures when builds/pc has no such build
            let src = format!("{BUILDS}{b}.xml");
            let src = if std::path::Path::new(&src).exists() {
                PathBuf::from(src)
            } else {
                scene::oracle_dir().join(format!("fixtures/config/local-builds/{b}.xml"))
            };
            std::fs::copy(&src, builds.join(format!("{b}.xml"))).expect("copy build");
        }
        std::fs::write(builds.join("Spectre Test.xml"), spectre_test()).expect("write");
        std::fs::write(builds.join("Loadout Test.xml"), loadout_test()).expect("write");
        std::fs::write(builds.join("Dev Test.xml"), dev_test()).expect("write");
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
    let config = b.config_tab.borrow().clone().expect("configTab");
    let notes = b.notes_tab.borrow().clone().expect("notesTab");
    json!({
        "config": config.debug_dump(g),
        "notes": notes.debug_dump(g),
        "popup": pob_ui::tabs::config::debug_popup(g),
        "statBox": lines,
        "loadouts": {
            "sel": c.build_loadouts.sel_index.get(),
            "list": c.build_loadouts.list.borrow().iter().map(|i| i.label.clone().unwrap_or_default()).collect::<Vec<_>>(),
        },
        "activeSpec": b.engine.borrow().as_ref().expect("engine").model().tree.active_spec,
        "viewMode": *b.view_mode.borrow(),
        "unsaved": b.unsaved.get(),
    })
}

fn center(g: &Gfx, c: &dyn Control) -> (f64, f64) {
    let (x, y) = c.get_pos(g);
    let (w, h) = c.get_size(g);
    ((x + w / 2.0).floor(), (y + h / 2.0).floor())
}

/// `calcsCursor` of config-runner.lua.
fn cursor_step(gfx: &mut Gfx, st: &Value, cur: &mut (f64, f64)) {
    let b = main().build_mode.clone();
    let Some(tab) = b.config_tab.borrow().clone() else { return };
    let notes = b.notes_tab.borrow().clone().expect("notesTab");
    if st["scrollTop"].as_bool() == Some(true) {
        tab.scroll_bar.set_offset(0.0);
    }
    if let Some(v) = st["scrollVar"].as_str() {
        tab.debug_scroll_to_var(gfx, v);
    }
    if let Some(i) = st["scrollBlock"].as_u64() {
        tab.debug_scroll_to_block(gfx, i as usize);
    }
    if let Some(v) = st["cursorVar"].as_str() {
        *cur = tab.debug_var_pos(gfx, v);
    } else if let Some(c) = st["cursorDrop"].as_array() {
        *cur = tab.debug_drop_row_pos(gfx, c[0].as_str().expect("var"), c[1].as_u64().expect("row") as usize);
    } else if let Some(c) = st["cursorBlock"].as_array() {
        *cur = tab.debug_block_pos(gfx, c[0].as_u64().expect("block") as usize, c[1].as_str().expect("part"));
    } else if let Some(s) = st["cursorSection"].as_str() {
        *cur = tab.debug_section_toggle_pos(gfx, s);
    } else if let Some(s) = st["cursorBuild"].as_str() {
        let c = b.host.get(s).unwrap_or_else(|| panic!("control {s}"));
        *cur = center(gfx, &*c);
    } else if let Some(s) = st["cursorTab"].as_str() {
        let c = tab.host.get(s).unwrap_or_else(|| panic!("control {s}"));
        *cur = center(gfx, &*c);
    } else if let Some(s) = st["cursorNotes"].as_str() {
        let c = notes.host.get(s).unwrap_or_else(|| panic!("control {s}"));
        *cur = center(gfx, &*c);
    } else if let Some(s) = st["cursorPopup"].as_str() {
        let p = main().popups.borrow().first().cloned().expect("popup");
        let c = p.host.get(s).unwrap_or_else(|| panic!("popup control {s}"));
        *cur = center(gfx, &*c);
    } else if let Some(c) = st["cursorList"].as_array() {
        *cur = pob_ui::tabs::config::debug_list_button_pos(gfx, c[0].as_str().expect("list"), c[1].as_str().expect("button"));
    } else if let Some(c) = st["cursorRow"].as_array() {
        *cur = pob_ui::tabs::config::debug_list_row_pos(gfx, c[0].as_str().expect("list"), c[1].as_u64().expect("row") as usize);
    } else if let Some(c) = st["cursorRel"].as_array() {
        cur.0 += c[0].as_f64().expect("dx");
        cur.1 += c[1].as_f64().expect("dy");
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
    rows: HashMap<String, Vec<String>>,
    dev_texts: Vec<(String, Value, std::collections::BTreeMap<String, usize>)>,
}

/// The lines of a tooltip text in a fixed order: the mods of different names come in the order of `pairs(modDB.mods)`, one of the process (R13).
fn sorted_lines(v: &Value) -> Value {
    let Some(m) = v.as_object() else { return v.clone() };
    Value::Object(
        m.iter()
            .map(|(k, t)| {
                let mut lines: Vec<&str> = t.as_str().unwrap_or_default().split('\n').collect();
                lines.sort_unstable();
                (k.clone(), json!(lines))
            })
            .collect(),
    )
}

fn run(name: &str) -> Run {
    let sc: Value =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join(format!("scenes/config/{name}.json"))).expect("scenario"))
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
        perf: Vec::new(),
        files: HashMap::new(),
        rows: HashMap::new(),
        dev_texts: Vec::new(),
    };
    // R13: `pairs(value.sources)` of the oracle's process, per step
    let mut source_order: HashMap<u64, Vec<String>> = HashMap::new();
    if let Some(rec) = oracle_record(name) {
        for d in rec["dumps"].as_array().into_iter().flatten() {
            if let Some(raw) = d["state"]["popup"]["tipRaw"].as_array() {
                let order = raw.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect();
                source_order.insert(d["step"].as_u64().expect("step"), order);
            }
        }
    }
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
        if let Some(n) = st["perf"].as_u64() {
            let t0 = std::time::Instant::now();
            for _ in 0..n {
                frame(&mut gfx, &l);
            }
            out.perf.push(("frame".into(), t0.elapsed().as_secs_f64() * 1e3 / n as f64));
            continue;
        }
        // U10c: `launch.devMode` and the ALT key
        if let Some(on) = st["devAlt"].as_bool() {
            pob_ui::launch::launch().dev_mode.set(on);
            gfx.input(RawEvent::Key {
                key: pob_gfx::input::PhysKey::Alt,
                pressed: on,
            });
            continue;
        }
        // U10c: `tooltipText` of every var
        if let Some(n) = st["devTexts"].as_str() {
            let tab = main().build_mode.config_tab.borrow().clone().expect("configTab");
            let (texts, branches) = tab.debug_tooltip_texts();
            out.dev_texts.push((n.to_string(), texts, branches));
            continue;
        }
        if let Some(c) = st["cursor"].as_array() {
            cur = (c[0].as_f64().expect("x"), c[1].as_f64().expect("y"));
            gfx.input(RawEvent::Cursor { x: cur.0, y: cur.1 });
        }
        cursor_step(&mut gfx, st, &mut cur);
        pob_ui::tabs::config_mod_browser::set_source_order_sample(source_order.get(&((i + 1) as u64)).cloned());
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
        if let (Some(file), Some(file_out)) = (st["file"].as_str(), st["fileOut"].as_str()) {
            let p = format!("{}{file}", main().build_path.borrow());
            out.files.insert(
                file_out.to_string(),
                std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}")),
            );
        }
        if let Some(n) = st["rowsOut"].as_str() {
            let p = main().popups.borrow().first().cloned().expect("popup");
            let rows = pob_ui::tabs::config_mod_browser::debug_all_rows(&gfx, p.host.get("listControl").expect("listControl"));
            out.rows.insert(n.to_string(), rows);
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
    let p = scene::oracle_dir().join(format!("fixtures/config/{name}.json"));
    Some(serde_json::from_str(&std::fs::read_to_string(p).ok()?).expect("oracle json"))
}

fn scenario_names() -> Vec<String> {
    let mut names: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join("scenes/config/index.json")).expect("index")).expect("json");
    // POB_CONFIG_SCENES=a,b: a look at these scenes alone
    if let Ok(only) = std::env::var("POB_CONFIG_SCENES") {
        return only.split(',').map(str::to_owned).collect();
    }
    // personal builds: compared when the local list, the scenes and the fixtures are there
    for (i, _) in local_builds("config").iter().enumerate() {
        let n = format!("local-{}", i + 1);
        if scene::oracle_dir().join(format!("scenes/config/{n}.json")).exists() && oracle_record(&n).is_some() {
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
        _ if a == b => None,
        _ => Some(format!("{path}: oracle {a}, ours {b}")),
    }
}

fn missing_inputs() -> Option<String> {
    let names = scenario_names();
    names
        .iter()
        .find(|n| oracle_record(n).is_none())
        .map(|n| format!("fixtures/config/{n}.json is absent (tools/sg-oracle/run.ps1 -Script config-runner.lua)"))
}

#[test]
fn config_states_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED config_states_equal_original: {why}");
        return;
    }
    let mut failures = Vec::new();
    let mut compared = 0;
    let mut dev_branches: std::collections::BTreeMap<String, std::collections::BTreeMap<String, usize>> = Default::default();
    for name in scenario_names() {
        let rec = oracle_record(&name).expect("record");
        let r = run(&name);
        let dumps = rec["dumps"].as_array().expect("dumps");
        assert_eq!(dumps.len(), r.dumps.len(), "{name}: dump count");
        for (o, (step, ours)) in dumps.iter().zip(&r.dumps) {
            assert_eq!(o["step"].as_u64(), Some(*step), "{name}: steps");
            let mut s = o["state"].clone();
            assert!(s.is_object(), "{name} step {step}: {s}");
            if let Some(p) = s["popup"].as_object_mut() {
                p.remove("tipRaw");
            }
            let s = &s;
            let state = norm(&json!({
                "config": s["config"],
                "notes": s["notes"],
                "popup": s["popup"],
                "statBox": s["build"]["statBox"],
                "loadouts": { "sel": s["build"]["buildLoadouts"]["sel"], "list": s["build"]["buildLoadouts"]["list"] },
                "activeSpec": s["activeSpec"],
                "viewMode": s["build"]["viewMode"],
                "unsaved": s["build"]["unsaved"],
            }));
            compared += 1;
            if let Some(d) = first_diff(&state, &norm(ours), "") {
                failures.push(format!("{name} step {step}: {d}"));
            }
        }
        for (n, rows) in &r.rows {
            let p = scene::oracle_dir().join(format!("fixtures/config/{n}.json"));
            let oracle: Vec<String> =
                serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("rows");
            if oracle != *rows {
                let at = oracle.iter().zip(rows).position(|(a, b)| a != b);
                let row = at.map(|i| (oracle[i].clone(), rows[i].clone()));
                failures.push(format!(
                    "{name}: {n}: {} rows vs {}, the first difference at {at:?}: {row:?}",
                    oracle.len(),
                    rows.len()
                ));
            }
        }
        for (n, texts, branches) in &r.dev_texts {
            let p = scene::oracle_dir().join(format!("fixtures/config/{n}.json"));
            let oracle: Value =
                serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("texts");
            eprintln!(
                "{n}: {} vars with a text, the if* keys that gave one: {branches:?}",
                oracle.as_object().map_or(0, serde_json::Map::len)
            );
            *dev_branches.entry(n.clone()).or_default() = branches.clone();
            if let Some(d) = first_diff(&sorted_lines(&oracle), &sorted_lines(texts), n) {
                failures.push(format!("{name}: {d}"));
            }
        }
        for (file, text) in &r.files {
            let p = scene::oracle_dir().join(format!("fixtures/config/{file}"));
            // Lua writes in text mode (CRLF)
            let oracle = std::fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
                .replace("\r\n", "\n");
            if oracle != *text {
                let at = oracle.lines().zip(text.lines()).position(|(a, b)| a != b);
                failures.push(format!(
                    "{name}: saved {file} differs at line {at:?}, {} bytes vs {}",
                    oracle.len(),
                    text.len()
                ));
                if let Ok(dir) = std::env::var("POB_CONFIG_DUMP_OURS") {
                    std::fs::create_dir_all(&dir).expect("mkdir");
                    std::fs::write(format!("{dir}/{file}"), text).expect("write");
                }
            }
        }
    }
    eprintln!("dumps compared {compared}, differ {}", failures.len());
    // U10c: every `if*` key with a dev tooltip and data gave a text somewhere, or the comparison above proves nothing about it
    if !dev_branches.is_empty() {
        for gate in [
            "Cond",
            "MinionCond",
            "EnemyCond",
            "CondTrue",
            "Mult",
            "EnemyMult",
            "Stat",
            "TagType",
            "Mod",
        ] {
            let n: usize = dev_branches.values().filter_map(|b| b.get(gate)).sum();
            eprintln!("dev tooltip of {gate}: {n} vars");
            assert!(
                n > 0 || std::env::var("POB_CONFIG_SCENES").is_ok(),
                "no scene shows the dev tooltip of {gate}"
            );
        }
    }
    assert!(
        failures.is_empty(),
        "{} differences, the first 300:\n{}",
        failures.len(),
        failures[..failures.len().min(300)].join("\n")
    );
}

/// The real window on both sides (config-real-seq.ps1): the stages' state and the events the window delivered. Run the sides first:
///   pwsh tools/sg-oracle/run.ps1 -Script config-runner.lua -ScriptArgs real-input -Inject inject-config-real.ps1 -Timeout 40
///   pwsh tools/sg-oracle/config-rust-input.ps1
#[test]
fn config_real_input_equals_original() {
    let read = |n: &str| -> Option<Value> {
        let t = std::fs::read_to_string(scene::oracle_dir().join(format!("fixtures/config/{n}.json"))).ok()?;
        Some(serde_json::from_str(t.trim_start_matches('\u{feff}')).expect("json"))
    };
    let (Some(o), Some(r)) = (read("real-input-oracle"), read("real-input-rust")) else {
        eprintln!("SKIPPED config_real_input_equals_original: no fixtures/config/real-input-*.json");
        return;
    };
    let mut events = 0;
    for stage in ["start", "tab", "check", "typed", "wheel"] {
        if let Some(d) = first_diff(&norm(&o[stage]["configReal"]), &norm(&r[stage]["configReal"]), stage) {
            panic!("{d}");
        }
        assert_eq!(o[stage]["events"], r[stage]["events"], "{stage}: events");
        events = o[stage]["events"].as_array().map_or(0, Vec::len);
    }
    assert!(o["wheel"]["configReal"]["scroll"].as_f64() > Some(0.0), "the wheel scrolled");
    assert!(o["typed"]["configReal"]["inputEdit"].as_f64() == Some(25.0), "the digits arrived");
    eprintln!("5 stages, {events} events");
}

/// Rust vs the original, roughly (a busy machine): `perf` scene, tools/sg-oracle/run.ps1 -Script config-runner.lua -ScriptArgs perf.
#[test]
#[ignore = "numbers for a report"]
fn config_perf() {
    let Some(rec) = oracle_record("perf") else {
        eprintln!("SKIPPED config_perf: no fixtures/config/perf.json");
        return;
    };
    eprintln!("oracle: {}", rec["perf"]);
    eprintln!("rust:   {:?}", run("perf").perf);
}

/// Two screenshots of this port for the report (`tools/.scratch/u7-shots/`): the Configuration and Notes tabs on a public golden.
#[test]
#[ignore = "needs a wgpu adapter"]
fn config_report_shots() {
    let mut r = pob_gfx::Renderer::new_offscreen().expect("wgpu adapter");
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/.scratch/u7-shots"));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let (w, h) = scene::SCREEN;
    for (scenario, shot, file) in [
        ("options", "options-open", "configuration.png"),
        ("notes", "notes-fire", "notes.png"),
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
fn config_pixels_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED config_pixels_equal_original: {why}");
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
            let p = scene::oracle_dir().join(format!("fixtures/config/{shot}.png"));
            let oracle = image::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_rgba8();
            if let Ok(dir) = std::env::var("POB_CONFIG_SAVE_OURS") {
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
            let out = scene::out_dir();
            scene::save(&heat, &out.join(format!("{shot}.heat.png")));
            scene::save(&scene::crop(&rgba, w, w, h), &out.join(format!("{shot}.ours.png")));
            failures.push(format!("{shot}: {} px off (text {}), bbox {:?}", d.bad, d.text_bad, d.bbox));
        }
    }
    eprintln!("shots {shots}");
    assert!(failures.is_empty(), "the tab differs from the original:\n{}", failures.join("\n"));
}
