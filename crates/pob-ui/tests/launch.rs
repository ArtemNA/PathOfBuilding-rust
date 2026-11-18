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
    start_with(tag, |_, _| {})
}

/// `before` sees the temporary user path and the launch before `OnInit`.
fn start_with(tag: &str, before: impl FnOnce(&std::path::Path, &Rc<Launch>)) -> (Tmp, Gfx, TestPlatform, Rc<Launch>) {
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
    before(&dir, &l);
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

/// What `Check for Update` says while the update is looked for and downloaded fits its 140-pixel button (text at 16, Main.lua:240, ButtonControl.lua).
#[test]
fn update_progress_fits_its_button() {
    let (_dir, gfx, _tp, l) = start("update-progress");
    assert_eq!(l.update_progress(), "Checking...");
    for text in ["Checking...", "Downloading...", "Downloading 100%"] {
        let w = pob_ui::sg::draw_string_width(&gfx, 16.0, "VAR", text);
        println!("{text}: {w} px");
        assert!(w <= 132.0, "{text}: {w}");
    }
}

/// A build saved on another tab opens without its tree tab (it is made when first shown); the Items tab's "Manage..." of the passive
/// trees opens the spec list then too (ItemsTab.lua:215-217).
#[test]
fn manage_trees_works_before_the_tree_tab_is_shown() {
    let (_tmp, mut gfx, _tp, l) = start("manage-trees");
    frame(&mut gfx, &l);
    let b = main().build_mode.clone();
    *b.view_mode.borrow_mut() = "ITEMS".into();
    *b.tree_tab.borrow_mut() = None;
    frame(&mut gfx, &l);
    pob_ui::tabs::tree::open_spec_manage_popup(&mut gfx);
    assert_eq!(main().popups.borrow().len(), 1);
    assert!(b.tree_tab.borrow().is_some());
}

/// Ctrl+Shift+F12 writes the state of the input and of the tree under the user folder (the port's own diagnostics); F12 alone does not.
#[test]
fn ctrl_shift_f12_writes_the_diagnostics() {
    let (tmp, mut gfx, _tp, l) = start("diag");
    for _ in 0..3 {
        frame(&mut gfx, &l);
    }
    let dir = tmp.0.join("PoB test/Diagnostics");
    l.on_key_down(&mut gfx, "F12", false);
    assert!(!dir.exists(), "F12 alone wrote");
    gfx.input(RawEvent::Key {
        key: PhysKey::Ctrl,
        pressed: true,
    });
    gfx.input(RawEvent::Key {
        key: PhysKey::Shift,
        pressed: true,
    });
    l.on_key_down(&mut gfx, "F12", false);
    let files: Vec<_> = std::fs::read_dir(&dir).expect("Diagnostics").collect();
    assert_eq!(files.len(), 1);
    let text = std::fs::read_to_string(files[0].as_ref().expect("entry").path()).expect("read");
    println!("{text}");
    for want in [
        "held: [\"CTRL\", \"SHIFT\"]",
        "view TREE",
        "tree view: port",
        "last frames",
        "down F12",
    ] {
        assert!(text.contains(want), "no {want:?} in\n{text}");
    }
}

// ----- the port's own: an error while a build opens from a link offers the update (launch.rs `update_from_err_prompt`) -----

const LINK: &str = "pob://pobbin/abc";
const LINK_URL: &str = "https://pobb.in/pob/abc";

fn pending(kind: pob_ui::update::Install) -> pob_ui::update::Pending {
    pob_ui::update::Pending {
        version: "9.9.9".into(),
        kind,
        file: (kind != pob_ui::update::Install::Manual).then(|| std::path::PathBuf::from("setup.exe")),
        page: "https://example.invalid/releases/v9.9.9".into(),
    }
}

/// The error prompt of a link says what the update is doing, redrawn from the download's progress; without an update, or for an error
/// that is not of a link, it is upstream's prompt unchanged.
#[test]
fn link_error_prompt_tells_of_the_update() {
    use std::sync::atomic::Ordering;
    let (_tmp, mut gfx, _tp, l) = start("link-prompt");
    frame(&mut gfx, &l);
    // an error that is not of a link: upstream's text even with an update ready
    *l.update_pending.borrow_mut() = Some(pending(pob_ui::update::Install::WindowsSetup));
    l.show_err_msg("plain");
    assert_eq!(l.prompt_text(), *l.prompt_msg.borrow());
    l.on_key_down(&mut gfx, "RETURN", false);
    *l.update_pending.borrow_mut() = None;

    *l.opening_link.borrow_mut() = Some(LINK.into());
    l.show_err_msg("In 'OnFrame': broke");
    let msg = l.prompt_msg.borrow().clone().expect("prompt");
    assert!(
        msg.starts_with("^1Error:\n\n^0In 'OnFrame': broke\n^8v1.2.3\n^0Press Enter/Escape"),
        "{msg}"
    );
    assert!(l.opening_link.borrow().is_none(), "the link moved to the prompt");
    // no update: exactly the original's prompt, and U is only a key of it
    assert_eq!(l.prompt_text().as_deref(), Some(msg.as_str()));
    l.on_key_down(&mut gfx, "u", false);
    assert_eq!(l.prompt_msg.borrow().as_deref(), Some(msg.as_str()));
    assert!(!gfx.exit_requested());

    // downloading: the percentage follows the download while the prompt is shown
    let progress = std::sync::Arc::new(pob_net::DownloadProgress::default());
    progress.total.store(200, Ordering::Relaxed);
    progress.done.store(50, Ordering::Relaxed);
    l.set_update_downloading("9.9.9", progress.clone());
    assert_eq!(
        l.prompt_text().expect("text"),
        format!("{msg}\n^2Version 9.9.9 is downloading (25%): it will offer to install when it is done.")
    );
    progress.done.store(100, Ordering::Relaxed);
    frame(&mut gfx, &l);
    assert!(
        l.prompt_text()
            .expect("text")
            .ends_with("downloading (50%): it will offer to install when it is done.")
    );
    l.on_key_down(&mut gfx, "u", false);
    assert!(!gfx.exit_requested(), "nothing to install yet");
    assert!(!l.pending_open_path().expect("path").exists());

    // ready
    *l.update_pending.borrow_mut() = Some(pending(pob_ui::update::Install::WindowsSetup));
    assert_eq!(
        l.prompt_text().expect("text"),
        format!("{msg}\n^2Version 9.9.9 is ready: press U to install it and open this build again.")
    );
    *l.update_pending.borrow_mut() = Some(pending(pob_ui::update::Install::Manual));
    assert!(
        l.prompt_text()
            .expect("text")
            .ends_with("\n^2Version 9.9.9 is out: press U to open its release page.")
    );
}

/// U in the error prompt of a link: the link waits in the user folder and the installer starts (here a recorder, no process), and the
/// program exits; a copy updated by hand gets the release page and nothing waits; an installer that does not start leaves nothing behind.
#[test]
fn u_in_the_link_error_prompt_installs_and_keeps_the_link() {
    use std::cell::RefCell;
    let (_tmp, mut gfx, tp, l) = start("link-install");
    frame(&mut gfx, &l);
    let calls: Rc<RefCell<Vec<String>>> = Rc::default();
    let result: Rc<RefCell<Result<(), String>>> = Rc::new(RefCell::new(Ok(())));
    {
        let (calls, result) = (calls.clone(), result.clone());
        *l.install_hook.borrow_mut() = Some(Box::new(move |p, _exe| {
            calls.borrow_mut().push(p.version.clone());
            result.borrow().clone()
        }));
    }
    let file = l.pending_open_path().expect("path");
    *l.opening_link.borrow_mut() = Some(LINK.into());
    l.show_err_msg("In 'OnFrame': broke");
    *l.update_pending.borrow_mut() = Some(pending(pob_ui::update::Install::WindowsSetup));
    l.on_key_down(&mut gfx, "u", false);
    assert_eq!(std::fs::read_to_string(&file).expect("pending-open.txt"), format!("{LINK}\n"));
    assert_eq!(*calls.borrow(), ["9.9.9"]);
    assert!(gfx.exit_requested());
    let _ = gfx.take_requests();
    std::fs::remove_file(&file).expect("clean");

    // the installer does not start: the file goes, the prompt gives way to the error's toast
    *result.borrow_mut() = Err("no".into());
    l.on_key_down(&mut gfx, "u", false);
    assert_eq!(calls.borrow().len(), 2);
    assert!(!file.exists());
    assert!(!gfx.exit_requested());
    assert!(l.prompt_msg.borrow().is_none());
    assert_eq!(l.update_err_msg.borrow().as_deref(), Some("no"));
    *l.update_err_msg.borrow_mut() = None;

    // by hand: the page, no installer, nothing waits
    *l.opening_link.borrow_mut() = Some(LINK.into());
    l.show_err_msg("In 'OnFrame': broke");
    *l.update_pending.borrow_mut() = Some(pending(pob_ui::update::Install::Manual));
    l.on_key_down(&mut gfx, "u", false);
    assert_eq!(tp.state().opened, ["https://example.invalid/releases/v9.9.9"]);
    assert_eq!(calls.borrow().len(), 2);
    assert!(!file.exists());
    assert!(l.prompt_msg.borrow().is_some());
}

fn write_pending_open(user: &std::path::Path, age: std::time::Duration) -> std::path::PathBuf {
    let dir = user.join("PoB test");
    std::fs::create_dir_all(&dir).expect("user folder");
    let file = dir.join(pob_ui::launch::PENDING_OPEN_FILE);
    std::fs::write(&file, format!("{LINK}\n")).expect("pending-open.txt");
    let f = std::fs::File::options().write(true).open(&file).expect("open");
    f.set_modified(std::time::SystemTime::now() - age).expect("mtime");
    file
}

fn scripted() -> std::sync::Arc<pob_net::ScriptedTransport> {
    let t = std::sync::Arc::new(pob_net::ScriptedTransport::default());
    t.add(
        LINK_URL,
        Ok(pob_net::Reply {
            code: 200,
            body: b"not a build code".to_vec(),
            ..pob_net::Reply::default()
        }),
    );
    t
}

/// The new version starts without a link and finds the one that waited: it opens it as `arg[1]` (the download of the link, a build
/// of it), and the file is gone; the link is forgotten once that build has shown a frame.
#[test]
fn the_link_that_waited_opens_at_start() {
    let net = scripted();
    let mut file = None;
    let (_tmp, mut gfx, _tp, l) = start_with("link-reopen", |dir, l| {
        file = Some(write_pending_open(dir, std::time::Duration::from_secs(60)));
        l.set_transport(net.clone());
    });
    let file = file.expect("file");
    assert!(!file.exists(), "read once");
    assert_eq!(l.opening_link.borrow().as_deref(), Some(LINK));
    for _ in 0..500 {
        frame(&mut gfx, &l);
        if l.opening_link.borrow().is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(net.urls(), [LINK_URL]);
    assert!(l.opening_link.borrow().is_none(), "the build of the link opened");
    assert_eq!(main().mode.borrow().as_deref(), Some("BUILD"));
    assert!(l.prompt_msg.borrow().is_none());
}

/// A link older than a day is removed and not opened; a start with its own link leaves the file alone.
#[test]
fn an_old_link_or_a_start_with_a_link_does_not_open_the_waiting_one() {
    let net = scripted();
    let mut file = None;
    let (_tmp, _gfx, _tp, l) = start_with("link-old", |dir, l| {
        file = Some(write_pending_open(dir, std::time::Duration::from_secs(2 * 24 * 60 * 60)));
        l.set_transport(net.clone());
    });
    assert!(!file.expect("file").exists(), "removed even when not opened");
    assert!(l.opening_link.borrow().is_none());
    assert!(net.urls().is_empty());

    let mut file = None;
    let (_tmp, _gfx, _tp, l) = start_with("link-arg", |dir, l| {
        file = Some(write_pending_open(dir, std::time::Duration::from_secs(60)));
        *l.start_uri.borrow_mut() = Some("pob://pobbin/other".into());
    });
    assert!(file.expect("file").exists());
    assert_eq!(l.opening_link.borrow().as_deref(), Some("pob://pobbin/other"));
}
