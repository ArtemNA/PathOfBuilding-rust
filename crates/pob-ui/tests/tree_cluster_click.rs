//! A left click on an allocated jewel socket with a cluster jewel in it (PassiveTreeView.lua:391-400): the socket and the cluster's nodes go, and
//! the same frame draws the smaller node list. The frame drew with the order of the old list, an index past its end: "In 'OnFrame': index out of
//! bounds". Upstream's spec test build, the clicks go through the view as the user's do.

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::{Button, Gfx, RawEvent, TestPlatform};
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::{ModeArg, main};
use std::rc::Rc;

const BUILD: &str = "Generals Perforate Zerker";

fn frame(gfx: &mut Gfx, l: &Rc<Launch>) {
    let mut f = gfx.begin_frame();
    l.on_frame(&mut f);
    f.finish();
}

/// The allocated sockets a cluster jewel can go in.
fn cluster_sockets() -> Vec<u32> {
    let b = main().build_mode.clone();
    let eng = b.engine.borrow();
    let spec = eng.as_ref().expect("engine").spec_rc();
    let mut out: Vec<u32> = spec
        .nodes
        .iter()
        .filter(|n| n.alloc && n.tree_node().expansion_jewel.is_some())
        .map(|n| n.id)
        .collect();
    out.sort();
    out
}

/// Moves the view onto the node and gives its centre on the screen (`cursorFind` of tree-runner.lua).
fn screen_of(id: u32) -> (f64, f64) {
    let art = main().build_mode.tree_art();
    let b = main().build_mode.clone();
    let eng = b.engine.borrow();
    let spec = eng.as_ref().expect("engine").spec_rc();
    let mut tv = b.tree_view.borrow_mut();
    let v = tv.as_mut().expect("view");
    // close enough that the view can centre any node (`zoomX` is clamped to 2/3 of the zoom) and a socket is wider than the bars that move the view
    v.zoom_level = 15.0;
    v.zoom = 1.2f64.powf(v.zoom_level);
    let (w, h) = (968.0, 712.0);
    let scale = f64::min(w, h) / art.size * v.zoom;
    let n = spec.node(id).expect("node").tree_node();
    // the node in the middle of the view
    v.zoom_x = -n.x.expect("x") * scale;
    v.zoom_y = -n.y.expect("y") * scale;
    (
        (n.x.expect("x") * scale + v.zoom_x + 312.0 + w / 2.0).floor(),
        (n.y.expect("y") * scale + v.zoom_y + 32.0 + h / 2.0).floor(),
    )
}

fn node_count() -> usize {
    let b = main().build_mode.clone();
    let eng = b.engine.borrow();
    eng.as_ref().expect("engine").spec_rc().nodes.len()
}

#[test]
fn a_click_on_a_cluster_socket_draws_the_new_tree() {
    let root = std::env::temp_dir().join(format!("pob-ui-cluster-click-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let builds = root.join("Path of Building/Builds");
    std::fs::create_dir_all(&builds).expect("mkdir");
    std::fs::copy(
        format!("{}/spec/TestBuilds/3.13/{BUILD}.xml", scene::pob_root()),
        builds.join(format!("{BUILD}.xml")),
    )
    .expect("copy build");
    let tp = TestPlatform::new();
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, Box::new(tp)).expect("fonts");
    gfx.set_dpi_scale_override_percent(100);
    gfx.input(RawEvent::Cursor { x: -100.0, y: -100.0 });
    let pob = scene::pob_root();
    let l = Launch::new(LaunchConfig {
        user_path: Some(root.to_string_lossy().replace('\\', "/")),
        user_folder: "Path of Building".into(),
        script_path: scene::script_path(),
        doc_path: format!("{pob}/"),
        version_number: "?".into(),
        version_branch: Some("?".into()),
        version_platform: Some("?".into()),
        app_version: None,
    });
    l.on_init(&mut gfx);
    assert!(l.prompt_msg.borrow().is_none(), "startup error: {:?}", l.prompt_msg.borrow());
    main().background.wait();
    frame(&mut gfx, &l);
    let path = format!("{}{BUILD}.xml", main().build_path.borrow());
    main().set_mode("BUILD", vec![ModeArg::Str(path), ModeArg::Str(BUILD.to_string())]);
    frame(&mut gfx, &l);
    main().build_mode.wait_tree_assets();
    frame(&mut gfx, &l);

    // the first socket whose click takes a cluster away; a click that does not is undone by the next click on it
    let mut shrank = None;
    for id in cluster_sockets() {
        let before = node_count();
        let (x, y) = screen_of(id);
        gfx.input(RawEvent::Cursor { x, y });
        frame(&mut gfx, &l);
        let hover = main().build_mode.tree_view.borrow().as_ref().and_then(|v| v.hover_node);
        // the socket, or the node of its cluster drawn over it (65760 over 2491): a click on either takes the cluster away
        assert!(hover.is_some(), "socket {id}: the cursor is on a node");
        let press = |pressed| RawEvent::Button {
            button: Button::Left,
            pressed,
            pos: None,
        };
        gfx.input(press(true));
        l.on_key_down(&mut gfx, "LEFTBUTTON", false);
        gfx.input(press(false));
        l.on_key_up(&mut gfx, "LEFTBUTTON");
        frame(&mut gfx, &l);
        assert!(l.prompt_msg.borrow().is_none(), "socket {id}: {:?}", l.prompt_msg.borrow());
        let after = node_count();
        if after < before {
            // the frame after draws the smaller list too
            frame(&mut gfx, &l);
            assert!(l.prompt_msg.borrow().is_none(), "socket {id}: {:?}", l.prompt_msg.borrow());
            shrank = Some((id, before, after));
            break;
        }
    }
    let (id, before, after) = shrank.expect("a click took a cluster away");
    assert!(before - after >= 5, "{before} -> {after}");
    println!("socket {id}: {before} -> {after} nodes, no error");
    let _ = std::fs::remove_dir_all(&root);
}
