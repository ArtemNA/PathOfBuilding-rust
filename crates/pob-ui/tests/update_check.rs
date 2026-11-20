//! `Launch::set_update_check(false)` (the web application: a page does not update itself): no "Check for Update" button, no check at the
//! start, Ctrl+U does nothing; the footer of `app_version` stays. On (the desktop, the default) all three ask the network.

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::{Gfx, RawEvent, TestPlatform};
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::main;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;

struct Seen {
    button: bool,
    footer: (String, String),
    /// Network attempts after the start, then after Ctrl+U.
    at_start: usize,
    after_ctrl_u: usize,
}

fn run(update_check: Option<bool>) -> Seen {
    let root = std::env::temp_dir().join(format!("pob-ui-update-check-{}-{update_check:?}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Path of Building/Builds")).expect("mkdir");
    let tp = TestPlatform::new();
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, Box::new(tp)).expect("fonts");
    gfx.input(RawEvent::Cursor { x: -100.0, y: -100.0 });
    let pob = scene::pob_root();
    let l: Rc<Launch> = Launch::new(LaunchConfig {
        user_path: Some(root.to_string_lossy().replace('\\', "/")),
        user_folder: "Path of Building".into(),
        script_path: scene::script_path(),
        doc_path: format!("{pob}/"),
        version_number: "?".into(),
        version_branch: Some("?".into()),
        version_platform: Some("?".into()),
        app_version: Some("9.9.9".into()),
    });
    let net = Arc::new(pob_net::NoNetwork::default());
    l.set_transport(net.clone());
    if let Some(on) = update_check {
        l.set_update_check(on);
    }
    l.on_init(&mut gfx);
    assert!(l.prompt_msg.borrow().is_none(), "startup error: {:?}", l.prompt_msg.borrow());
    l.dev_mode.set(false);
    main().background.wait();
    // what pob-app's main does after the init
    l.check_for_update_at_start();
    let frame = |gfx: &mut Gfx| {
        let mut f = gfx.begin_frame();
        l.on_frame(&mut f);
        f.finish();
    };
    frame(&mut gfx);
    l.net.wait_idle();
    frame(&mut gfx);
    let at_start = net.attempts.load(Ordering::SeqCst);
    let m = main();
    let get = |n: &str| m.host.get(n).unwrap_or_else(|| panic!("control {n}"));
    let button = get("checkUpdate").is_shown(&gfx);
    let label = |n: &str| get(n).debug_value(&gfx).and_then(|v| v.as_str().map(str::to_owned)).expect("label");
    let footer = (label("forkLabel"), label("versionLabel"));
    // Ctrl+U (Launch.lua:166-169)
    gfx.input(RawEvent::Key {
        key: pob_gfx::input::PhysKey::Ctrl,
        pressed: true,
    });
    l.on_key_down(&mut gfx, "u", false);
    gfx.input(RawEvent::Key {
        key: pob_gfx::input::PhysKey::Ctrl,
        pressed: false,
    });
    frame(&mut gfx);
    l.net.wait_idle();
    frame(&mut gfx);
    let after_ctrl_u = net.attempts.load(Ordering::SeqCst);
    let _ = std::fs::remove_dir_all(&root);
    Seen {
        button,
        footer,
        at_start,
        after_ctrl_u,
    }
}

#[test]
fn update_check_off_keeps_the_footer_and_asks_nothing() {
    let on = run(None);
    // the control: the desktop asks at the start and on Ctrl+U, and shows the button
    assert!(on.button, "Check for Update is shown on the desktop");
    assert_eq!(on.at_start, 1, "the check at the start");
    assert_eq!(on.after_ctrl_u, 2, "Ctrl+U checks again");
    assert!(on.footer.0.contains("9.9.9"), "footer {:?}", on.footer);

    let off = run(Some(false));
    assert!(!off.button, "no Check for Update");
    assert_eq!((off.at_start, off.after_ctrl_u), (0, 0), "no request to the network");
    assert_eq!(off.footer, on.footer, "the footer stays");
    assert!(l_prompt_free());
}

fn l_prompt_free() -> bool {
    pob_ui::launch::launch().prompt_msg.borrow().is_none()
}
