//! U5 gate: the original `SkillsTab` / `GemSelectControl` / `SkillListControl` / `SkillSetListControl` / `Minion*ListControl` (the whole PoB, run by the SG oracle
//! `tools/sg-oracle/lua/skills-runner.lua`) vs this port, on the public goldens (`tools/sg-oracle/scenes/skills/*.json`) and the local list `local_builds("skills")`.
//! - `skills_states_equal_original` (no GPU): every `dump` step = the oracle's (groups, gems, fields, the list of the gem control, tooltips, the side bar).
//! - `skills_pixels_equal_original` (GPU/WARP, `#[ignore]`): every screenshot = the original's PNG; 0 pixels beyond 2/255 outside the lossy class.
//!   cargo test -p pob-ui --release --test skills -- --include-ignored --nocapture

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

/// OccVortex + three groups (the same edit as skills-runner.lua).
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
        let root = std::env::temp_dir().join(format!("pob-ui-u5-{}-{n}-{name}", std::process::id()));
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
        for b in local_builds("skills") {
            let src = format!("{BUILDS_329}{b}.xml");
            let src = if std::path::Path::new(&src).exists() {
                src
            } else {
                format!("{BUILDS}{b}.xml")
            };
            std::fs::copy(src, builds.join(format!("{b}.xml"))).expect("copy build");
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

/// `<Skills ...>...</Skills>` of a saved file: the attributes are sorted, the order of the groups and gems is the tab's.
fn skills_block(x: &str) -> String {
    let a = x.find("<Skills").expect("<Skills");
    let b = x[a..].find("</Skills>").expect("</Skills>");
    x[a..a + b].replace("\r\n", "\n")
}

fn dump(g: &Gfx, st: &Value, tp: &TestPlatform) -> Value {
    let m = main();
    let b = m.build_mode.clone();
    let mut d = serde_json::Map::new();
    d.insert("popups".into(), json!(m.popups.borrow().len()));
    if let Some(p) = m.popups.borrow().first() {
        d.insert("popupTitle".into(), json!(p.title.get(g)));
        if p.title.get(g) == "Spectre Library" {
            d.insert("library".into(), pob_ui::tabs::minion_list::debug_library().expect("library"));
        }
    }
    d.insert("clipboard".into(), json!(tp.state().clipboard.unwrap_or_else(|| "nil".into())));
    let tab = b.skills_tab.borrow().clone().expect("skillsTab");
    d.insert("skills".into(), tab.debug_dump(g));
    if st["calcs"].as_bool() == Some(true) {
        let t = b.calcs_tab.borrow().clone().expect("calcsTab");
        let mut c = t.debug_dump(g);
        c["sidebar"] = b.breakdown.borrow().clone().expect("breakdown").debug_dump(g);
        c["bsel"] = json!(if b.host.sel().is_some() { "other" } else { "nil" });
        let (ma, ca) = b.engine.borrow().as_ref().expect("engine").debug_alloc_orders();
        c["allocMain"] = json!(ma);
        c["allocCalcs"] = json!(ca);
        c["unsaved"] = json!(b.unsaved.get());
        d.insert("calcs".into(), c);
    }
    let c = b.c();
    let dropdown = |x: &pob_ui::controls::DropDownControl| {
        let list: Vec<String> = x.list.borrow().iter().map(|v| v.label.clone().unwrap_or_default()).collect();
        json!({ "shown": x.is_shown(g), "enabled": x.is_enabled(g), "sel": x.sel_index.get(), "list": list })
    };
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
        json!({
            "viewMode": *b.view_mode.borrow(), "unsaved": b.unsaved.get(), "statBox": lines, "warnings": b.warning_lines.borrow().clone(),
            "points": c.point_display.str_.borrow().clone().unwrap_or("nil".into()),
            "mainSocketGroup": dropdown(&c.main_socket_group), "mainSkill": dropdown(&c.main_skill), "mainSkillPart": dropdown(&c.main_skill_part),
            "mainSkillMinion": dropdown(&c.main_skill_minion), "mainSkillMinionSkill": dropdown(&c.main_skill_minion_skill), "buildLoadouts": dropdown(&c.build_loadouts),
            "minionLibrary": c.main_skill_minion_library.is_shown(g),
        }),
    );
    if let Some(f) = st["file"].as_str() {
        let p = format!("{}{f}", main().build_path.borrow());
        d.insert(
            "saved".into(),
            json!(skills_block(&std::fs::read_to_string(p).expect("saved file"))),
        );
    }
    Value::Object(d)
}

/// The oracle's dump restricted to what `dump()` records.
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
    if !s["library"].is_null() {
        d.insert("library".into(), s["library"].clone());
    }
    d.insert("clipboard".into(), s["clipboard"].clone());
    d.insert("skills".into(), s["skills"].clone());
    if st["calcs"].as_bool() == Some(true) {
        d.insert("calcs".into(), s["calcs"].clone());
    }
    d.insert(
        "build".into(),
        json!({
            "viewMode": b["viewMode"], "unsaved": b["unsaved"], "statBox": lines, "warnings": b["warnings"], "points": b["points"],
            "mainSocketGroup": b["mainSocketGroup"], "mainSkill": b["mainSkill"], "mainSkillPart": b["mainSkillPart"], "mainSkillMinion": b["mainSkillMinion"],
            "mainSkillMinionSkill": b["mainSkillMinionSkill"], "buildLoadouts": b["buildLoadouts"], "minionLibrary": b["minionLibrary"],
        }),
    );
    if let Some(out) = st["fileOut"].as_str() {
        let p = scene::oracle_dir().join(format!("fixtures/skills/{out}"));
        let t = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{name}: {}: {e}", p.display()));
        d.insert("saved".into(), json!(skills_block(&t)));
    }
    Value::Object(d)
}

fn centre(g: &Gfx, c: &dyn Control, dx: f64, dy: f64) -> (f64, f64) {
    let (x, y) = c.get_pos(g);
    let (w, h) = c.get_size(g);
    ((x + w / 2.0).floor() + dx, (y + h / 2.0).floor() + dy)
}

/// `skillsCursor` / `calcsCursor` of skills-runner.lua.
#[allow(clippy::too_many_lines)]
fn cursor_step(gfx: &mut Gfx, st: &Value, cur: &mut (f64, f64)) {
    let m = main();
    let b = m.build_mode.clone();
    let tab = b.skills_tab.borrow().clone();
    let off = |a: &Vec<Value>, i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0);
    // a popup the oracle has and this port does not: the states differ at this step, the comparison says it
    if (st.get("cursorPopup").is_some() || st.get("cursorPopupRow").is_some() || st.get("cursorChild").is_some())
        && m.popups.borrow().is_empty()
    {
        return;
    }
    let row = |g: &Gfx, c: &dyn Control, i: f64| {
        let (x, y) = c.get_pos(g);
        (x + 10.0, y + 2.0 + 16.0 * (i - 1.0) + 8.0)
    };
    if let Some(c) = st["cursorSkill"].as_array() {
        *cur = tab
            .as_ref()
            .expect("tab")
            .debug_control_pos(gfx, c[0].as_str().expect("name"), off(c, 1), off(c, 2));
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
        let (parent, child) = (c[0].as_str().expect("parent"), c[1].as_str().expect("child"));
        let n = match parent {
            "1" => tab
                .as_ref()
                .expect("tab")
                .set_list
                .borrow()
                .upgrade()
                .expect("set list")
                .host
                .get(child),
            _ => pob_ui::tabs::minion_list::debug_child(parent, child),
        }
        .unwrap_or_else(|| panic!("child {parent}/{child}"));
        *cur = centre(gfx, &*n, 0.0, 0.0);
    } else if let Some(c) = st["cursorList"].as_str() {
        let t = tab.as_ref().expect("tab");
        let n = t.group_list.host.get(c).unwrap_or_else(|| panic!("list control {c}"));
        *cur = centre(gfx, &*n, 0.0, 0.0);
    } else if st["cursorLastRow"].as_bool() == Some(true) {
        let t = tab.as_ref().expect("tab");
        let n = t.read(|e| e.socket_groups().len());
        *cur = row(gfx, &*t.group_list, n as f64);
    } else if let Some(i) = st["cursorRow"].as_f64() {
        let t = tab.as_ref().expect("tab");
        *cur = row(gfx, &*t.group_list, i);
    } else if let Some(c) = st["cursorPopupRow"].as_array() {
        let p = m.popups.borrow()[0].clone();
        *cur = row(gfx, &*p.control(c[0].as_str().expect("name")), off(c, 1));
    } else if let Some(c) = st["cursorDrop"].as_array() {
        let t = tab.as_ref().expect("tab");
        let name = c[0].as_str().expect("name");
        let n = t.host.get(name).unwrap_or_else(|| panic!("control {name}"));
        let s = t.gem_select_of(&n).expect("gem select");
        let (x, y) = s.get_pos(gfx);
        let (_, h) = s.get_size(gfx);
        *cur = (
            x + 10.0,
            y + h + 2.0 + (off(c, 1) - 1.0) * (h - 4.0) + 8.0 - s.scroll_bar.offset.get(),
        );
    } else if let Some(c) = st["cursorPick"].as_array() {
        let name = c[1].as_str().expect("name");
        let n: Rc<dyn Control> = if c[0].as_str() == Some("build") {
            b.host.get(name).unwrap_or_else(|| panic!("control {name}"))
        } else {
            tab.as_ref()
                .expect("tab")
                .host
                .get(name)
                .unwrap_or_else(|| panic!("control {name}"))
        };
        let (x, y) = n.get_pos(gfx);
        let (_, h) = n.get_size(gfx);
        *cur = (x + 10.0, y + h + 2.0 + (off(c, 2) - 1.0) * (h - 4.0) + ((h - 4.0) / 2.0).floor());
    } else if let Some(c) = st["cursorControl"].as_array() {
        let t = b.calcs_tab.borrow().clone().expect("calcsTab");
        *cur = t.debug_control_pos(gfx, c[0].as_str().expect("id"), c[1].as_str().expect("name"));
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
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join(format!("scenes/skills/{name}.json"))).expect("scenario"))
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
        if st.get("variantOrder").is_some() {
            continue;
        }
        if st["waitAssets"].as_bool() == Some(true) {
            main().build_mode.wait_tree_assets();
            frame(&mut gfx, &l);
            continue;
        }
        if let Some(o) = st["treeOrder"].as_str() {
            // the overlap order of this oracle process (R27, R31)
            let rec = oracle_record(o).unwrap_or_else(|| panic!("fixtures/skills/{o}.json"));
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
                .set_order_sample(ids, conns);
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
            if let Some(o) = sc["steps"][i + 1]["variantOrder"].as_str() {
                let rec = oracle_record(o).unwrap_or_else(|| panic!("fixtures/skills/{o}.json"));
                let text = with_variants(&std::fs::read_to_string(&path).expect("build"), &rec);
                std::fs::write(&path, text).expect("build");
            }
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
            out.dumps.push(((i + 1) as u64, dump(&gfx, st, &tp), st.clone()));
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

/// R13: `pairs(possibleVariants)` (SkillsTab.lua:474) of the oracle's process, for this one field: the variant it took, as the `variantId` of the `<Gem>`s that have none.
fn with_variants(xml: &str, rec: &Value) -> String {
    let attr = |tag: &str, name: &str| -> Option<String> {
        let key = format!(" {name}=\"");
        let a = tag.find(&key)? + key.len();
        Some(tag[a..a + tag[a..].find('"')?].to_string())
    };
    let sample: std::collections::HashMap<(String, String), String> = rec["gems"]
        .as_array()
        .expect("gems")
        .iter()
        .map(|g| {
            (
                (
                    g["gemId"].as_str().expect("gemId").to_string(),
                    g["skillId"].as_str().expect("skillId").to_string(),
                ),
                g["variantId"].as_str().expect("variantId").to_string(),
            )
        })
        .collect();
    let mut out = String::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<Gem ") {
        let end = at + rest[at..].find('>').expect("tag");
        let tag = &rest[at..end];
        out.push_str(&rest[..at + 4]);
        if attr(tag, "variantId").is_none()
            && let (Some(g), Some(k)) = (attr(tag, "gemId"), attr(tag, "skillId"))
            && let Some(v) = sample.get(&(g, k))
        {
            out.push_str(&format!(" variantId=\"{v}\""));
        }
        rest = &rest[at + 4..];
    }
    out.push_str(rest);
    out
}

/// The real window on both sides (skills-real-seq.ps1): the stages' state and the events the window delivered. Run the sides first:
///   pwsh tools/sg-oracle/run.ps1 -Script skills-runner.lua -ScriptArgs real-input -Inject inject-skills-real.ps1 -Timeout 40
///   pwsh tools/sg-oracle/skills-rust-input.ps1
#[test]
fn skills_real_input_equals_original() {
    let read = |n: &str| -> Option<Value> {
        let t = std::fs::read_to_string(scene::oracle_dir().join(format!("fixtures/skills/{n}.json"))).ok()?;
        Some(serde_json::from_str(t.trim_start_matches('\u{feff}')).expect("json"))
    };
    let (Some(o), Some(r)) = (read("real-input-oracle"), read("real-input-rust")) else {
        eprintln!("SKIPPED skills_real_input_equals_original: no fixtures/skills/real-input-*.json");
        return;
    };
    let mut events = 0;
    for stage in ["start", "tab", "row", "typed", "wheel"] {
        if let Some(d) = first_diff(&norm(&o[stage]["skillsReal"]), &norm(&r[stage]["skillsReal"]), stage) {
            panic!("{d}");
        }
        assert_eq!(o[stage]["events"], r[stage]["events"], "{stage}: events");
        events = o[stage]["events"].as_array().map_or(0, Vec::len);
    }
    assert_eq!(o["row"]["skillsReal"]["display"].as_f64(), Some(4.0), "the row was clicked");
    // the click put the caret before the "0": "1" makes "10", and the limit of 2 refuses "2"
    assert_eq!(o["typed"]["skillsReal"]["gemQuality"].as_f64(), Some(10.0), "the digits arrived");
    assert!(o["wheel"]["skillsReal"]["scrollH"].as_f64() > Some(0.0), "the wheel scrolled");
    eprintln!("5 stages, {events} events");
}

fn oracle_record(name: &str) -> Option<Value> {
    let p = scene::oracle_dir().join(format!("fixtures/skills/{name}.json"));
    Some(serde_json::from_str(&std::fs::read_to_string(p).ok()?).expect("oracle json"))
}

fn scenario_names() -> Vec<String> {
    let mut names: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(scene::oracle_dir().join("scenes/skills/index.json")).expect("index")).expect("json");
    for (i, _) in local_builds("skills").iter().enumerate() {
        let n = format!("local-{}", i + 1);
        if scene::oracle_dir().join(format!("scenes/skills/{n}.json")).exists() && oracle_record(&n).is_some() {
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
    let names = scenario_names();
    names
        .iter()
        .find(|n| oracle_record(n).is_none())
        .map(|n| format!("fixtures/skills/{n}.json is absent (tools/sg-oracle/run.ps1 -Script skills-runner.lua)"))
}

/// One scene against its record: the differences go to `failures`, returns the dumps compared.
fn compare_scene(name: &str, failures: &mut Vec<String>) -> usize {
    let rec = oracle_record(name).expect("record");
    let r = run(name);
    let dumps = rec["dumps"].as_array().expect("dumps");
    assert_eq!(dumps.len(), r.dumps.len(), "{name}: dump count");
    if let Ok(dir) = std::env::var("POB_SKILLS_DUMP_OURS") {
        std::fs::create_dir_all(&dir).expect("mkdir");
        let all: Vec<Value> = r.dumps.iter().map(|(s, d, _)| json!({ "step": s, "state": d })).collect();
        std::fs::write(
            format!("{dir}/{name}.json"),
            serde_json::to_string(&json!({ "dumps": all })).expect("json"),
        )
        .expect("write");
    }
    let mut compared = 0;
    for (o, (step, ours, st)) in dumps.iter().zip(&r.dumps) {
        assert_eq!(o["step"].as_u64(), Some(*step), "{name}: steps");
        let mut state = norm(&oracle_state(o, st, name));
        compared += 1;
        let mut ours = norm(ours);
        for d in [&mut state, &mut ours] {
            if let Some(ss) = d.get_mut("calcs").and_then(|c| c["sections"].as_array_mut()) {
                for s in ss {
                    if s["enabled"] != json!(true) {
                        s["rect"] = Value::Null;
                    }
                }
            }
        }
        if let Some(d) = first_diff(&state, &ours, "") {
            failures.push(format!("{name} step {step}: {d}"));
        }
    }
    compared
}

/// U5-perf: the tooltips of a gem's Quality / Enabled and of a hovered candidate on a build whose calculator reads `GlobalCache` (The Saviour, R47): the cache of `GetMiscCalculator`, not an
/// empty one. `tools/sg-oracle/run.ps1 -Script skills-runner.lua -ScriptArgs u5hover`.
#[test]
fn gem_tooltips_over_the_shared_cache_equal_original() {
    if oracle_record("u5hover").is_none() {
        eprintln!("SKIPPED gem_tooltips_over_the_shared_cache_equal_original: no fixtures/skills/u5hover.json");
        return;
    }
    let mut failures = Vec::new();
    let compared = compare_scene("u5hover", &mut failures);
    eprintln!("dumps compared {compared}, differ {}", failures.len());
    assert!(
        failures.is_empty(),
        "{}",
        failures
            .iter()
            .map(|s| s.chars().take(900).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Choosing the gems whose whole `BuildOutput` reads a detached main skill in the `GlobalCache` (`cached_sd`): a scene of its own, a process of its own (R48).
#[test]
fn real_gem_choice_equals_original() {
    if oracle_record("real-select").is_none() {
        eprintln!("SKIPPED real_gem_choice_equals_original: no fixtures/skills/real-select.json");
        return;
    }
    let mut failures = Vec::new();
    let compared = compare_scene("real-select", &mut failures);
    eprintln!("dumps compared {compared}, differ {}", failures.len());
    assert!(
        failures.is_empty(),
        "{}",
        failures
            .iter()
            .map(|s| s.chars().take(900).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn skills_states_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED skills_states_equal_original: {why}");
        return;
    }
    // POB_SKILLS_ONLY=<scene>: one scene while looking at a difference
    let only = std::env::var("POB_SKILLS_ONLY").ok();
    let mut failures = Vec::new();
    let mut compared = 0;
    for name in scenario_names() {
        if only.as_ref().is_some_and(|o| *o != name) {
            continue;
        }
        compared += compare_scene(&name, &mut failures);
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

/// Rust vs the original, roughly (a busy machine): `perf` scene, tools/sg-oracle/run.ps1 -Script skills-runner.lua -ScriptArgs perf.
#[test]
#[ignore = "numbers for a report"]
fn skills_perf() {
    let Some(rec) = oracle_record("perf") else {
        eprintln!("SKIPPED skills_perf: no fixtures/skills/perf.json");
        return;
    };
    eprintln!("oracle: {}", rec["perf"]);
    eprintln!("rust:   {:?}", run("perf").perf);
}

#[test]
#[ignore = "needs a wgpu adapter"]
fn skills_pixels_equal_original() {
    if let Some(why) = missing_inputs() {
        eprintln!("SKIPPED skills_pixels_equal_original: {why}");
        return;
    }
    let only = std::env::var("POB_SKILLS_ONLY").ok();
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
            let p = scene::oracle_dir().join(format!("fixtures/skills/{shot}.png"));
            let oracle = image::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_rgba8();
            if let Ok(dir) = std::env::var("POB_SKILLS_SAVE_OURS") {
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
