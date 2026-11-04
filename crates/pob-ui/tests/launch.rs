//! Launch.lua behaviour that the oracle scenes cannot show identically (error texts differ between Lua tracebacks and
//! Rust panics): `PCall` → prompt, the prompt keys (Launch.lua:358-398), `ToastNotification` timing, Settings.xml
//! round trip with a fresh user folder. Runs in a temporary folder.

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::input::PhysKey;
use pob_gfx::{Gfx, RawEvent, TestPlatform};
use pob_ui::launch::{Launch, LaunchConfig, pcall};
use pob_ui::modules::main::main;
use std::rc::Rc;

struct Tmp(std::path::PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn start(tag: &str) -> (Tmp, Gfx, TestPlatform, Rc<Launch>) {
    let dir = std::env::temp_dir().join(format!("pob-ui-u2b-launch-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp");
    let tp = TestPlatform::new();
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, Box::new(tp.clone())).expect("fonts");
    gfx.set_dpi_scale_override_percent(100);
    let root = scene::pob_root();
    let l = Launch::new(LaunchConfig {
        user_path: Some(dir.to_string_lossy().replace('\\', "/")),
        user_folder: "PoB test".into(),
        script_path: scene::script_path(),
        doc_path: format!("{root}/"),
        version_number: "1.2.3".into(),
        version_branch: None,
        version_platform: None,
        app_version: None,
    });
    l.on_init(&mut gfx);
    (Tmp(dir), gfx, tp, l)
}

fn frame(gfx: &mut Gfx, l: &Rc<Launch>) {
    let mut f = gfx.begin_frame();
    l.on_frame(&mut f);
    f.finish();
}

/// The version of the window's "Version:" and of the network's User-Agent comes from `manifest.xml`; without one it is `?` (Launch.lua:24-26).
#[test]
fn manifest_gives_the_version_of_the_user_agent() {
    let (dir, _gfx, _tp, l) = start("manifest");
    assert_eq!(l.net_request("https://x", None, None, true).user_agent, "Path of Building/1.2.3");

    let path = dir.0.join("manifest.xml");
    std::fs::write(
        &path,
        "<?xml version='1.0' encoding='UTF-8'?>\n<PoBVersion>\n\t<Version number=\"2.67.2\" />\n</PoBVersion>\n",
    )
    .expect("manifest");
    let mut cfg = l.cfg.clone();
    cfg.read_manifest(&path.to_string_lossy());
    assert_eq!(cfg.version_number, "2.67.2");
    cfg.read_manifest(&dir.0.join("absent.xml").to_string_lossy());
    assert_eq!(cfg.version_number, "?");
}

/// The application's two lines fit between About and the end of the side panel in the measured width, also with longer versions; `Dev Mode` on the upper row ends before them.
#[test]
fn version_lines_fit_the_side_panel() {
    use pob_ui::modules::main::{VERSION_LINES_X, version_lines};
    let (_dir, gfx, _tp, _l) = start("version-lines");
    // the main anchor is at 4; the side panel's divider starts at 308
    let left = 4.0 + VERSION_LINES_X;
    for (app, up) in [("0.1.0", "2.67.2"), ("10.10.10", "10.100.10")] {
        let (upper, lower) = version_lines(app, up);
        for line in [upper, lower] {
            let w = pob_ui::sg::draw_string_width(&gfx, 16.0, "VAR", &line);
            println!("{line}: {w} px, ends at {}", left + w);
            assert!(left + w <= 308.0, "{line}: {w}");
        }
    }
    let dev = pob_ui::sg::draw_string_width(&gfx, 20.0, "VAR", "Dev Mode");
    println!("Dev Mode: {dev} px, ends at {}", 4.0 + dev);
    assert!(4.0 + dev < left, "Dev Mode: {dev}");
}

#[test]
fn pcall_returns_the_panic_message() {
    assert_eq!(pcall(|| 7), Ok(7));
    assert_eq!(pcall(|| -> i32 { panic!("boom {}", 1) }), Err("boom 1".to_string()));
}

#[test]
fn fresh_user_folder_starts_a_new_build_and_saves_settings() {
    let (tmp, mut gfx, _tp, l) = start("fresh");
    assert!(l.prompt_msg.borrow().is_none());
    frame(&mut gfx, &l);
    // No Settings.xml: main:Init's SetMode("BUILD", false, "Unnamed build") stays (Main.lua:87).
    assert_eq!(main().mode.borrow().as_deref(), Some("BUILD"));
    assert!(tmp.0.join("PoB test/Builds").is_dir());
    l.on_exit(&mut gfx);
    let text = std::fs::read_to_string(tmp.0.join("PoB test/Settings.xml")).expect("Settings.xml");
    assert!(text.contains("<Mode mode=\"BUILD\">"), "{text}");
    assert!(text.contains("<Arg string=\"Unnamed build\"/>"), "{text}");
    assert!(text.contains("tokenExpiry=\"nil\""), "{text}");
    assert!(text.contains("betaTest=\"nil\""), "{text}");
}

#[test]
fn error_prompt_keys() {
    let (_tmp, mut gfx, tp, l) = start("prompt");
    frame(&mut gfx, &l);
    l.show_err_msg("In 'OnFrame': something broke");
    let msg = l.prompt_msg.borrow().clone().expect("prompt");
    assert!(
        msg.starts_with("^1Error:\n\n^0In 'OnFrame': something broke\n^8v1.2.3\n^0Press Enter/Escape"),
        "{msg}"
    );
    // A second error while the prompt is shown is dropped (Launch.lua:382).
    l.show_err_msg("second");
    assert_eq!(l.prompt_msg.borrow().as_deref(), Some(msg.as_str()));
    // Keys go to the prompt, not to main; Ctrl+C copies without colour codes.
    gfx.input(RawEvent::Key {
        key: PhysKey::Ctrl,
        pressed: true,
    });
    l.on_key_down(&mut gfx, "c", false);
    gfx.input(RawEvent::Key {
        key: PhysKey::Ctrl,
        pressed: false,
    });
    let copied = tp.state().clipboard.expect("copied");
    assert!(
        copied.starts_with("Error:\n\nIn 'OnFrame': something broke\nv1.2.3\nPress"),
        "{copied}"
    );
    assert!(main().input_events.borrow().is_empty());
    // F4: back to the build list, popups and pending input dropped.
    main().open_message_popup(&mut gfx, "Title", "text");
    l.on_key_down(&mut gfx, "F4", false);
    assert!(l.prompt_msg.borrow().is_none());
    assert!(main().popups.borrow().is_empty());
    frame(&mut gfx, &l);
    assert_eq!(main().mode.borrow().as_deref(), Some("LIST"));
    // A panic inside main's frame becomes the prompt (PCall), and Return dismisses it.
    main().set_mode("NOPE", Vec::new());
    frame(&mut gfx, &l);
    assert!(
        l.prompt_msg.borrow().is_none(),
        "an unknown mode is ignored like CallMode on a nil table"
    );
    l.show_err_msg("x");
    l.on_key_down(&mut gfx, "RETURN", false);
    assert!(l.prompt_msg.borrow().is_none());
}

#[test]
fn toast_timing() {
    let (_tmp, mut gfx, tp, l) = start("toast");
    frame(&mut gfx, &l);
    let id = main().toasts.add("Title line\nsecond\nthird");
    // calculateHeight: 2 newlines * 16 + 60.
    tp.set_time(1000);
    frame(&mut gfx, &l);
    assert_eq!(main().main_bar_height.get(), 58.0);
    tp.set_time(1125);
    frame(&mut gfx, &l);
    assert_eq!(main().main_bar_height.get(), 58.0 + 46.0);
    tp.set_time(1300);
    frame(&mut gfx, &l);
    assert_eq!(main().main_bar_height.get(), 58.0 + 92.0);
    assert!(main().toasts.exists(&id));
    main().toasts.remove(&gfx, &id, false);
    tp.set_time(1400);
    frame(&mut gfx, &l);
    assert!(!main().toasts.exists(&id));
    assert_eq!(main().main_bar_height.get(), 58.0);
}
