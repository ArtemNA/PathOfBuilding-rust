//! The switches of the web application (`set_prefetch(false)`, `set_parallel_image_decode(true)`, `set_preload_data(true)`) change who reads
//! and decodes first, never what is shown or calculated; `set_tree_images_async(true)` lets the tree's images come a frame or two later, then the same: a build opened with all three is the build opened without them, its calculation and its
//! tree as drawn once the images are in. Each way runs in a process of its own (the switches are global); this test compares the two.

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::{DrawList, Gfx, RawEvent, TestPlatform};
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::{ModeArg, main};
use std::rc::Rc;

const BUILD: &str = "Generals Perforate Zerker";

fn frame(gfx: &mut Gfx, l: &Rc<Launch>) -> DrawList {
    let mut f = gfx.begin_frame();
    l.on_frame(&mut f);
    f.finish()
}

/// Each command with its texture's size and pixels instead of the texture's id.
fn seen(l: &DrawList) -> String {
    use std::hash::{Hash, Hasher};
    l.commands
        .iter()
        .map(|c| {
            let px = c.texture.pixels().map(|p| {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                p.hash(&mut h);
                h.finish()
            });
            format!(
                "{} {} {:?} {:?} {:?} {:?} {}x{} {px:?}\n",
                c.layer, c.sub_layer, c.pos, c.uv, c.tint, c.clip, c.texture.width, c.texture.height
            )
        })
        .collect()
}

/// One way: opens the build in the tree, waits for its images, and writes what it calculated and drew to `POB_WEB_OUT`.
#[test]
#[ignore = "run by the_web_switches_change_nothing_shown"]
fn open_a_build_one_way() {
    let (Ok(mode), Ok(out)) = (std::env::var("POB_WEB_MODE"), std::env::var("POB_WEB_OUT")) else {
        return;
    };
    let root = std::env::temp_dir().join(format!("pob-ui-web-switches-{}-{mode}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let builds = root.join("Path of Building/Builds");
    std::fs::create_dir_all(&builds).expect("mkdir");
    std::fs::copy(
        format!("{}/spec/TestBuilds/3.13/{BUILD}.xml", scene::pob_root()),
        builds.join(format!("{BUILD}.xml")),
    )
    .expect("copy");
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, Box::new(TestPlatform::new())).expect("fonts");
    gfx.set_dpi_scale_override_percent(100);
    gfx.input(RawEvent::Cursor { x: -100.0, y: -100.0 });
    let l = Launch::new(LaunchConfig {
        user_path: Some(root.to_string_lossy().replace(char::from(92u8), "/")),
        user_folder: "Path of Building".into(),
        script_path: scene::script_path(),
        doc_path: format!("{}/", scene::pob_root()),
        version_number: "?".into(),
        version_branch: Some("?".into()),
        version_platform: Some("?".into()),
        app_version: None,
    });
    if mode.starts_with("web") {
        l.set_prefetch(false);
        l.set_parallel_image_decode(true);
        l.set_preload_data(true);
    }
    if mode == "web-async" {
        l.set_tree_images_async(true);
    }
    l.on_init(&mut gfx);
    main().background.wait();
    frame(&mut gfx, &l);
    let path = format!("{}{BUILD}.xml", main().build_path.borrow());
    main().set_mode("BUILD", vec![ModeArg::Str(path), ModeArg::Str(BUILD.to_string())]);
    // the most a frame of the opening drew: with the tree's images asynchronous it is less, the rest comes later
    let opened = (0..3).map(|_| frame(&mut gfx, &l).commands.len()).max().unwrap_or(0);
    main().build_mode.wait_tree_assets();
    // the frames until three in a row are the same: every image that comes later is in
    let mut last = seen(&frame(&mut gfx, &l));
    let mut same = 0;
    let t = std::time::Instant::now();
    while same < 3 && t.elapsed().as_secs() < 30 {
        std::thread::sleep(std::time::Duration::from_millis(30));
        let now = seen(&frame(&mut gfx, &l));
        same = if now == last { same + 1 } else { 0 };
        last = now;
    }
    let settled = last;
    assert!(l.prompt_msg.borrow().is_none(), "{:?}", l.prompt_msg.borrow());
    let calc = {
        let b = main().build_mode.clone();
        let eng = b.engine.borrow();
        eng.as_ref().expect("engine").output_json().to_string()
    };
    let parallel = pob_gfx::image::PARALLEL_DRAWS.load(std::sync::atomic::Ordering::Relaxed);
    std::fs::write(out, format!("{parallel} {opened}\n{calc}\n{settled}")).expect("write");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_web_switches_change_nothing_shown() {
    let dir = std::env::temp_dir().join(format!("pob-ui-web-switches-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let run = |mode: &str| -> String {
        let out = dir.join(format!("{mode}.txt"));
        let st = std::process::Command::new(std::env::current_exe().expect("exe"))
            .args(["open_a_build_one_way", "--ignored", "--exact", "--test-threads=1"])
            .env("POB_WEB_MODE", mode)
            .env("POB_WEB_OUT", &out)
            .stdout(std::process::Stdio::null())
            .status()
            .expect("run");
        assert!(st.success(), "{mode}");
        std::fs::read_to_string(&out).expect("its output")
    };
    let (desktop, web, web_async) = (run("desktop"), run("web"), run("web-async"));
    // the first line: the draws that waited for their image at the end of a frame, and the most commands a frame of the opening drew
    let head = |s: &str| -> (usize, usize) {
        let mut it = s
            .lines()
            .next()
            .unwrap_or("")
            .split(' ')
            .map(|n| n.parse::<usize>().expect("a number"));
        (it.next().expect("parallel"), it.next().expect("opened"))
    };
    let (d, w, a) = (head(&desktop), head(&web), head(&web_async));
    println!("parallel draws, commands of the opening frame: desktop {d:?}, web {w:?}, web with asynchronous tree images {a:?}");
    assert_eq!(d.0, 0, "the desktop decodes in place");
    assert!(w.0 > 20, "the web way decoded side by side");
    assert_eq!(w.1, d.1, "the parallel decode draws the opening frame whole");
    assert!(a.1 + 100 < d.1, "the asynchronous tree images are not in the opening frame");
    let rest = |s: &str| s.lines().skip(1).collect::<Vec<_>>().join("\n");
    let desktop = rest(&desktop);
    assert!(desktop.lines().count() > 500, "a drawn tree: {} lines", desktop.lines().count());
    for (way, got) in [("web", rest(&web)), ("web-async", rest(&web_async))] {
        if desktop != got {
            let at = desktop.lines().zip(got.lines()).position(|(a, b)| a != b);
            panic!("{way}: the switches changed line {at:?} of {}", desktop.lines().count());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
