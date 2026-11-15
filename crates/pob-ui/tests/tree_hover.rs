//! Every node of a build's tree is under the cursor where it is drawn: the Power Report's jump (`Focus`) puts a node in the middle of the
//! tree's view, and the cursor there must hover that node and no other (PassiveTreeView.lua:302-333: no proxy, no node of a proxy group).
//! The build comes from `POB_HOVER_BUILD` (personal, never in git): without it the test only says so.
//!   POB_HOVER_BUILD=<path> cargo test -p pob-ui --release --test tree_hover -- --nocapture

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::{Gfx, RawEvent, TestPlatform};
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::main;
use pob_ui::Control;
use std::rc::Rc;

fn frame(gfx: &mut Gfx, l: &Rc<Launch>) {
    let mut f = gfx.begin_frame();
    l.on_frame(&mut f);
    f.finish();
}

fn open_build() -> Option<(std::path::PathBuf, Gfx, Rc<Launch>)> {
    let Some(path) = std::env::var_os("POB_HOVER_BUILD") else {
        eprintln!("SKIPPED: set POB_HOVER_BUILD to a build");
        return None;
    };
    let root = std::env::temp_dir().join(format!("pob-ui-hover-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let builds = root.join("Path of Building/Builds");
    std::fs::create_dir_all(&builds).expect("mkdir");
    std::fs::copy(&path, builds.join("h.xml")).expect("copy build");
    let tp = TestPlatform::new();
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, Box::new(tp)).expect("fonts");
    gfx.set_dpi_scale_override_percent(100);
    gfx.input(RawEvent::Cursor { x: -100.0, y: -100.0 });
    let l = Launch::new(LaunchConfig {
        user_path: Some(root.to_string_lossy().replace('\\', "/")),
        user_folder: "Path of Building".into(),
        script_path: scene::script_path(),
        doc_path: format!("{}/", scene::pob_root()),
        version_number: "?".into(),
        version_branch: Some("?".into()),
        version_platform: Some("?".into()),
        app_version: None,
    });
    l.on_init(&mut gfx);
    main().background.wait();
    for _ in 0..4 {
        frame(&mut gfx, &l);
    }
    main().set_mode(
        "BUILD",
        vec![
            pob_ui::modules::main::ModeArg::Str(format!("{}h.xml", main().build_path.borrow())),
            pob_ui::modules::main::ModeArg::Str("h".into()),
        ],
    );
    for _ in 0..3 {
        frame(&mut gfx, &l);
    }
    Some((root, gfx, l))
}

fn click(gfx: &mut Gfx, l: &Rc<Launch>, x: f64, y: f64) {
    click_with(gfx, l, x, y, pob_gfx::input::Button::Left);
}

fn click_with(gfx: &mut Gfx, l: &Rc<Launch>, x: f64, y: f64, button: pob_gfx::input::Button) {
    gfx.input(RawEvent::Cursor { x, y });
    frame(gfx, l);
    for pressed in [true, false] {
        for cb in gfx.input(RawEvent::Button { button, pressed, pos: None }) {
            match cb {
                pob_gfx::input::Callback::KeyDown(k, d) => l.on_key_down(gfx, k, d),
                pob_gfx::input::Callback::KeyUp(k) => l.on_key_up(gfx, k),
                pob_gfx::input::Callback::Char(c) => l.on_char(gfx, c),
            }
        }
        frame(gfx, l);
    }
}

/// The Power Report the way a user drives it: a stat, the report, a click on a row with the mouse, the cursor to the middle of the tree where
/// the node now is: its tooltip node is the row's node, and two clicks there (the first takes the focus off the list, ControlHost.lua:52-60)
/// change its allocation. Each change rebuilds the report; the rows are walked over these rebuilds.
#[test]
fn a_power_report_row_leads_to_a_node_that_answers() {
    let Some((root, mut gfx, l)) = open_build() else { return };
    let b = main().build_mode.clone();
    *b.view_mode.borrow_mut() = "TREE".into();
    frame(&mut gfx, &l);
    let tab = b.tree_tab_made(&mut gfx);
    let stat = pob_calc::power::power_stat_list().into_iter().find(|s| s.stat.as_deref() == Some("Life")).expect("Life");
    tab.set_power_calc(&gfx, Some(stat));
    tab.power_report_list.base.shown.set(true);
    let wait = |gfx: &mut Gfx| {
        for _ in 0..3000 {
            frame(gfx, &l);
            if tab.power_job.borrow().is_none() && tab.power.borrow().is_some() && !tab.power_build_flag.get() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the power report never finished");
    };
    let mut bad = Vec::new();
    let (x0, y0, w) = (312.0, 32.0, 968.0);
    let h = 800.0 - 32.0 - 32.0 - 194.0 - 24.0;
    for round in 0..12 {
        wait(&mut gfx);
        let n = tab.power_report_list.list.borrow().len();
        assert!(n > 0, "an empty report");
        let row = (round * 7) % n.min(8) + 1;
        let id = tab.power_report_list.list.borrow()[row - 1].0.id;
        let (lx, ly) = tab.power_report_list.get_pos(&gfx);
        let r = tab.power_report_list.get_row_region(&gfx);
        let rh = tab.power_report_list.row_height;
        let off = tab.power_report_list.scroll_bar_v.offset.get();
        click(&mut gfx, &l, lx + r.x + 40.0, ly + r.y + (row as f64 - 0.5) * rh - off);
        gfx.input(RawEvent::Cursor { x: x0 + w / 2.0, y: y0 + h / 2.0 });
        frame(&mut gfx, &l);
        frame(&mut gfx, &l);
        let (hover, alloc_before) = {
            let tv = b.tree_view.borrow();
            let eng = b.engine.borrow();
            let spec = eng.as_ref().expect("engine").spec_rc();
            let hv = tv.as_ref().expect("view").hover_node.map(|i| spec.nodes[i].id);
            (hv, spec.node(id).is_some_and(|n| n.alloc))
        };
        if hover != Some(id) {
            let tv = b.tree_view.borrow();
            let v = tv.as_ref().expect("view");
            eprintln!("  list at {lx},{ly} region {:?} rh {rh} off {off}; sel {:?}; zoom {} {} {}", (r.x, r.y, r.width, r.height), tab.power_report_list.sel_index.get(), v.zoom_level, v.zoom_x, v.zoom_y);
            drop(tv);
            bad.push(format!("round {round} row {row}: node {id}, hovered {hover:?}"));
            continue;
        }
        let alloc_of = || b.engine.borrow().as_ref().expect("engine").spec_rc().node(id).is_some_and(|n| n.alloc);
        let mut clicks = 0;
        while alloc_of() == alloc_before && clicks < 3 {
            click(&mut gfx, &l, x0 + w / 2.0, y0 + h / 2.0);
            clicks += 1;
        }
        eprintln!("round {round} row {row}: node {id} alloc {alloc_before} -> {} after {clicks} clicks", alloc_of());
        if alloc_of() == alloc_before {
            bad.push(format!("round {round} row {row}: node {id} did not change after {clicks} clicks"));
        }
    }
    pob_ui::modules::main::clear_main();
    let _ = std::fs::remove_dir_all(&root);
    for x in &bad {
        eprintln!("  {x}");
    }
    assert!(bad.is_empty());
}

#[test]
fn every_node_is_hovered_where_it_is_drawn() {
    let Some((root, mut gfx, l)) = open_build() else { return };
    let b = main().build_mode.clone();
    *b.view_mode.borrow_mut() = "TREE".into();
    frame(&mut gfx, &l);
    let tab = b.tree_tab_made(&mut gfx);
    let mut bad = Vec::new();
    let mut checked = 0;
    for drawer in [false, true] {
        tab.power_report_list.base.shown.set(drawer);
        frame(&mut gfx, &l);
        let nodes: Vec<(u32, String, f64, f64)> = {
            let eng = b.engine.borrow();
            let spec = eng.as_ref().expect("engine").spec_rc();
            spec.nodes
                .iter()
                .filter(|n| {
                    let t = n.tree_node();
                    !t.is_proxy
                        && !t.group.is_some_and(|g| spec.tree.groups.get(&g).is_some_and(|g| g.is_proxy))
                        && pob_ui::tree_art::overlay_size(n.ty()).is_some()
                        && t.x.is_some()
                })
                .map(|n| (n.id, n.tree_node().dn.clone(), n.tree_node().x.expect("x"), n.tree_node().y.expect("y")))
                .collect()
        };
        // the tree's view: under the top bar, right of the side panel, above the tab's controls and the drawer (TreeTab.lua:432-446)
        let (x0, y0, w) = (312.0, 32.0, 968.0);
        let convert = if tab.show_convert.get() { 64.0 } else { 32.0 };
        let h = 800.0 - 32.0 - convert - if drawer { 194.0 } else { 0.0 } - 24.0;
        for (id, dn, x, y) in nodes {
            tab.jump_to.set(Some((x, y)));
            frame(&mut gfx, &l);
            // where the node is drawn: `Focus` centres it, the clamp of the zoom offset (PassiveTreeView.lua:275-278) can move it off the centre
            let (sx, sy) = {
                let size = b.tree_art().size;
                let tv = b.tree_view.borrow();
                let v = tv.as_ref().expect("view");
                let scale = f64::min(w, h) / size * v.zoom;
                (x * scale + v.zoom_x + x0 + w / 2.0, y * scale + v.zoom_y + y0 + h / 2.0)
            };
            if !(sx >= x0 + 2.0 && sx < x0 + w - 2.0 && sy >= y0 + 2.0 && sy < y0 + h - 2.0) {
                continue;
            }
            gfx.input(RawEvent::Cursor { x: sx.round(), y: sy.round() });
            frame(&mut gfx, &l);
            let got = {
                let tv = b.tree_view.borrow();
                let v = tv.as_ref().expect("view");
                let eng = b.engine.borrow();
                let spec = eng.as_ref().expect("engine").spec_rc();
                v.hover_node.map(|i| (spec.nodes[i].id, spec.nodes[i].tree_node().dn.clone()))
            };
            checked += 1;
            if got.as_ref().map(|g| g.0) != Some(id) {
                if bad.len() < 3 {
                    let size = b.tree_art().size;
                    let tv = b.tree_view.borrow();
                    let v = tv.as_ref().expect("view");
                    eprintln!("    debug {id}: at ({x},{y}) screen ({sx:.1},{sy:.1}) zoom {} {} ({},{}) art size {size} tree {}", v.zoom_level, v.zoom, v.zoom_x, v.zoom_y, b.engine.borrow().as_ref().expect("e").spec_rc().tree.tree_version);
                }
                bad.push(format!("drawer {drawer}: {id} {dn} -> {got:?}"));
            }
        }
    }
    pob_ui::modules::main::clear_main();
    let _ = std::fs::remove_dir_all(&root);
    eprintln!("{checked} checked, {} wrong", bad.len());
    for b in bad.iter().take(60) {
        eprintln!("  {b}");
    }
    assert!(bad.is_empty());
}

/// A right click on an allocated jewel socket opens the Items tab with the socket's list dropped, when the list has something to choose
/// (PassiveTreeView.lua:521-529, ItemSlotControl.lua:31-33: `#self.items > 1`); with nothing to choose it does nothing, as upstream.
#[test]
fn a_right_click_on_a_socket_opens_its_list() {
    let Some((root, mut gfx, l)) = open_build() else { return };
    let b = main().build_mode.clone();
    let mut bad = Vec::new();
    let sockets: Vec<(u32, f64, f64)> = {
        let eng = b.engine.borrow();
        let spec = eng.as_ref().expect("engine").spec_rc();
        spec.nodes
            .iter()
            .filter(|n| n.alloc && n.ty() == pob_tree::NodeType::Socket && n.id < 65536)
            .filter_map(|n| Some((n.id, n.tree_node().x?, n.tree_node().y?)))
            .collect()
    };
    let (x0, y0, w) = (312.0, 32.0, 968.0);
    let h = 800.0 - 32.0 - 32.0 - 24.0;
    for (id, x, y) in sockets {
        *b.view_mode.borrow_mut() = "TREE".into();
        frame(&mut gfx, &l);
        let tab = b.tree_tab_made(&mut gfx);
        tab.jump_to.set(Some((x, y)));
        frame(&mut gfx, &l);
        click_with(&mut gfx, &l, x0 + w / 2.0, y0 + h / 2.0, pob_gfx::input::Button::Right);
        let items = b.items_tab.borrow().clone().expect("itemsTab");
        let slot = items.sockets.borrow().get(&id).cloned().expect("socket slot");
        let choices = slot.items.borrow().len();
        let mode = b.view_mode.borrow().clone();
        eprintln!("socket {id}: {choices} choices -> {mode}");
        if (choices > 1) != (mode == "ITEMS") {
            bad.push(format!("socket {id}: {choices} choices, view {mode}"));
        }
    }
    pob_ui::modules::main::clear_main();
    let _ = std::fs::remove_dir_all(&root);
    assert!(bad.is_empty(), "{bad:?}");
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn coord(&mut self, max: f64) -> f64 {
        self.below(max as u64) as f64
    }
}

fn send(gfx: &mut Gfx, l: &Rc<Launch>, ev: RawEvent) {
    for cb in gfx.input(ev) {
        match cb {
            pob_gfx::input::Callback::KeyDown(k, d) => l.on_key_down(gfx, k, d),
            pob_gfx::input::Callback::KeyUp(k) => l.on_key_up(gfx, k),
            pob_gfx::input::Callback::Char(c) => l.on_char(gfx, c),
        }
    }
}

/// Random use of the window (clicks anywhere, wheel, keys, drags, tab switches, focus loss) and after each run of it the tree must still
/// answer: with no popup open, the node under the cursor is hovered at once and changes its allocation within 3 clicks (one may only take
/// the focus off a control). A failure prints the actions since the last probe. `POB_FUZZ_SEED`, `POB_FUZZ_ROUNDS`.
#[test]
fn the_tree_answers_after_random_use() {
    use pob_gfx::input::{Button, PhysKey};
    let Some((root, mut gfx, l)) = open_build() else { return };
    let seed: u64 = std::env::var("POB_FUZZ_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    let rounds: u64 = std::env::var("POB_FUZZ_ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(40);
    let mut rng = Rng(seed);
    let mut bad = Vec::new();
    let tabs = ["TREE", "SKILLS", "ITEMS", "CALCS", "CONFIG", "NOTES", "PARTY"];
    let mut probes = 0;
    for round in 0..rounds {
        let mut log = Vec::new();
        for _ in 0..25 {
            let a = rng.below(100);
            let (x, y) = (rng.coord(1280.0), rng.coord(800.0));
            if a < 40 {
                let b = if rng.below(5) == 0 { Button::Right } else { Button::Left };
                log.push(format!("click {b:?} {x},{y}"));
                send(&mut gfx, &l, RawEvent::Cursor { x, y });
                frame(&mut gfx, &l);
                send(&mut gfx, &l, RawEvent::Button { button: b, pressed: true, pos: None });
                frame(&mut gfx, &l);
                send(&mut gfx, &l, RawEvent::Button { button: b, pressed: false, pos: None });
            } else if a < 50 {
                log.push(format!("wheel {x},{y}"));
                send(&mut gfx, &l, RawEvent::Cursor { x, y });
                send(&mut gfx, &l, RawEvent::Wheel(if rng.below(2) == 0 { 1.0 } else { -1.0 }));
            } else if a < 62 {
                let keys = [
                    PhysKey::Escape,
                    PhysKey::Enter,
                    PhysKey::Tab,
                    PhysKey::Up,
                    PhysKey::Down,
                    PhysKey::Backspace,
                    PhysKey::Char(b'a' + rng.below(26) as u8),
                    PhysKey::Char(b'1' + rng.below(7) as u8),
                    PhysKey::PageUp,
                    PhysKey::PageDown,
                ];
                let k = keys[rng.below(keys.len() as u64) as usize];
                let hold = match rng.below(6) {
                    0 => Some(PhysKey::Ctrl),
                    1 => Some(PhysKey::Shift),
                    _ => None,
                };
                log.push(format!("key {hold:?}+{k:?}"));
                if let Some(h) = hold {
                    send(&mut gfx, &l, RawEvent::Key { key: h, pressed: true });
                }
                send(&mut gfx, &l, RawEvent::Key { key: k, pressed: true });
                if let PhysKey::Char(c) = k
                    && hold.is_none()
                {
                    send(&mut gfx, &l, RawEvent::Char(u32::from(c)));
                }
                send(&mut gfx, &l, RawEvent::Key { key: k, pressed: false });
                if let Some(h) = hold {
                    send(&mut gfx, &l, RawEvent::Key { key: h, pressed: false });
                }
            } else if a < 70 {
                let (x2, y2) = (rng.coord(1280.0), rng.coord(800.0));
                log.push(format!("drag {x},{y} -> {x2},{y2}"));
                send(&mut gfx, &l, RawEvent::Cursor { x, y });
                frame(&mut gfx, &l);
                send(&mut gfx, &l, RawEvent::Button { button: Button::Left, pressed: true, pos: None });
                frame(&mut gfx, &l);
                send(&mut gfx, &l, RawEvent::Cursor { x: x2, y: y2 });
                frame(&mut gfx, &l);
                send(&mut gfx, &l, RawEvent::Button { button: Button::Left, pressed: false, pos: None });
            } else if a < 82 {
                let t = tabs[rng.below(tabs.len() as u64) as usize];
                log.push(format!("tab {t}"));
                *main().build_mode.view_mode.borrow_mut() = t.into();
            } else if a < 88 {
                log.push("focus lost".into());
                send(&mut gfx, &l, RawEvent::FocusLost);
            } else {
                log.push("frames".into());
                for _ in 0..5 {
                    frame(&mut gfx, &l);
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            frame(&mut gfx, &l);
        }
        // the probe: no popup, the build open, the tree shown
        if l.prompt_msg.borrow().is_some() {
            log.push(format!("(an error prompt: {:?})", l.prompt_msg.borrow()));
            *l.prompt_msg.borrow_mut() = None;
        }
        for _ in 0..5 {
            if main().popups.borrow().is_empty() {
                break;
            }
            send(&mut gfx, &l, RawEvent::Key { key: PhysKey::Escape, pressed: true });
            send(&mut gfx, &l, RawEvent::Key { key: PhysKey::Escape, pressed: false });
            frame(&mut gfx, &l);
        }
        while !main().popups.borrow().is_empty() {
            main().close_popup();
        }
        if main().mode.borrow().as_deref() != Some("BUILD") {
            log.push("(reopened the build)".into());
            main().set_mode(
                "BUILD",
                vec![
                    pob_ui::modules::main::ModeArg::Str(format!("{}h.xml", main().build_path.borrow())),
                    pob_ui::modules::main::ModeArg::Str("h".into()),
                ],
            );
            for _ in 0..3 {
                frame(&mut gfx, &l);
            }
        }
        let b = main().build_mode.clone();
        *b.view_mode.borrow_mut() = "TREE".into();
        frame(&mut gfx, &l);
        let tab = b.tree_tab_made(&mut gfx);
        // a node that can change: allocated, or one step from the tree
        let cands: Vec<(u32, f64, f64)> = {
            let eng = b.engine.borrow();
            let spec = eng.as_ref().expect("engine").spec_rc();
            spec.nodes
                .iter()
                .filter(|n| {
                    !n.tree_node().is_proxy
                        && n.ascendancy_name().is_none()
                        && matches!(n.ty(), pob_tree::NodeType::Normal | pob_tree::NodeType::Notable)
                        && n.id < 65536
                        && (n.alloc || n.path.as_ref().is_some_and(|p| p.len() == 1))
                })
                .filter_map(|n| Some((n.id, n.tree_node().x?, n.tree_node().y?)))
                .collect()
        };
        if cands.is_empty() {
            continue;
        }
        let (id, x, y) = cands[rng.below(cands.len() as u64) as usize];
        tab.jump_to.set(Some((x, y)));
        frame(&mut gfx, &l);
        let shown = tab.power_report_list.is_shown(&gfx);
        let vp = b.tree_view.borrow().as_ref().expect("view").last_view_port;
        let (x0, y0, w, h) = (vp.x, vp.y, vp.width, vp.height);
        let (sx, sy) = {
            let size = b.tree_art().size;
            let tv = b.tree_view.borrow();
            let v = tv.as_ref().expect("view");
            let scale = f64::min(w, h) / size * v.zoom;
            ((x * scale + v.zoom_x + x0 + w / 2.0).round(), (y * scale + v.zoom_y + y0 + h / 2.0).round())
        };
        if !(sx > x0 + 2.0 && sx < x0 + w - 2.0 && sy > y0 + 2.0 && sy < y0 + h - 2.0) {
            continue;
        }
        probes += 1;
        send(&mut gfx, &l, RawEvent::Cursor { x: sx, y: sy });
        frame(&mut gfx, &l);
        frame(&mut gfx, &l);
        let hovered = {
            let tv = b.tree_view.borrow();
            let eng = b.engine.borrow();
            let spec = eng.as_ref().expect("engine").spec_rc();
            tv.as_ref().expect("view").hover_node.map(|i| spec.nodes[i].id)
        };
        let alloc_of = || b.engine.borrow().as_ref().expect("engine").spec_rc().node(id).is_some_and(|n| n.alloc);
        let before = alloc_of();
        let mut clicks = 0;
        while alloc_of() == before && clicks < 3 {
            click(&mut gfx, &l, sx, sy);
            clicks += 1;
        }
        let ok = hovered == Some(id) && alloc_of() != before;
        eprintln!(
            "round {round}: node {id} hovered {hovered:?} alloc {before} -> {} in {clicks} clicks{}",
            alloc_of(),
            if ok { "" } else { "  <-- FAIL" }
        );
        if !ok && bad.len() < 3 {
            // where the node is really hovered, if anywhere near
            let mut found = None;
            'scan: for r in (0..=120).step_by(4) {
                for (dx, dy) in [(r, 0), (-r, 0), (0, r), (0, -r), (r, r), (-r, -r), (r, -r), (-r, r)] {
                    send(&mut gfx, &l, RawEvent::Cursor { x: sx + f64::from(dx), y: sy + f64::from(dy) });
                    frame(&mut gfx, &l);
                    let hv = {
                        let tv = b.tree_view.borrow();
                        let eng = b.engine.borrow();
                        let spec = eng.as_ref().expect("engine").spec_rc();
                        tv.as_ref().expect("view").hover_node.map(|i| spec.nodes[i].id)
                    };
                    if hv == Some(id) {
                        found = Some((dx, dy));
                        break 'scan;
                    }
                }
            }
            let tv = b.tree_view.borrow();
            let v = tv.as_ref().expect("view");
            eprintln!(
                "    diag: really at offset {found:?}; zoom {} ({}, {}); drawer {shown}; convert {}; screen {:?}; mode {:?}; view {}",
                v.zoom_level,
                v.zoom_x,
                v.zoom_y,
                tab.show_convert.get(),
                pob_ui::common::screen_size(),
                main().mode.borrow(),
                b.view_mode.borrow()
            );
        }
        if !ok {
            bad.push(format!(
                "round {round}: node {id} hovered {hovered:?} alloc {before} -> {} in {clicks} clicks; tree tab focus {}, build focus {}\n      {}",
                alloc_of(),
                tab.host.sel().is_some(),
                b.host.sel().is_some(),
                log.join("\n      ")
            ));
        }
    }
    pob_ui::modules::main::clear_main();
    let _ = std::fs::remove_dir_all(&root);
    for x in &bad {
        eprintln!("{x}");
    }
    eprintln!("{probes} probes, {} failed", bad.len());
    assert!(bad.is_empty(), "{} failed rounds", bad.len());
}

/// How long a frame of the tree takes while the Power Report's walk runs in the background and after it (a measure, not a gate).
#[test]
fn frame_time_while_the_power_report_runs() {
    let Some((root, mut gfx, l)) = open_build() else { return };
    let b = main().build_mode.clone();
    *b.view_mode.borrow_mut() = "TREE".into();
    frame(&mut gfx, &l);
    let tab = b.tree_tab_made(&mut gfx);
    let stat = pob_calc::power::power_stat_list().into_iter().find(|s| s.stat.as_deref() == Some("Life")).expect("Life");
    tab.set_power_calc(&gfx, Some(stat));
    gfx.input(RawEvent::Cursor { x: 800.0, y: 400.0 });
    let mut during = Vec::new();
    let start = std::time::Instant::now();
    loop {
        let t = std::time::Instant::now();
        frame(&mut gfx, &l);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        if tab.power_job.borrow().is_none() && tab.power.borrow().is_some() {
            break;
        }
        during.push(ms);
        assert!(start.elapsed().as_secs() < 300, "the walk never ended");
    }
    let walk = start.elapsed().as_secs_f64();
    let mut after = Vec::new();
    for _ in 0..60 {
        let t = std::time::Instant::now();
        frame(&mut gfx, &l);
        after.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let stats = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        (v.len(), v[v.len() / 2], v[v.len() * 9 / 10], v[v.len() - 1])
    };
    eprintln!("walk {walk:.1} s; frames during (n, median, p90, max ms) {:?}; after {:?}", stats(&mut during), stats(&mut after));
    pob_ui::modules::main::clear_main();
    let _ = std::fs::remove_dir_all(&root);
}
