//! The switches of the web application (`set_prefetch(false)`, `set_parallel_image_decode(true)`, `set_preload_data(true)`) change who reads
//! and decodes first, never what is shown or calculated: a build opened with all three is the build opened without them, its calculation and its
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
    if mode == "web" {
        l.set_prefetch(false);
        l.set_parallel_image_decode(true);
        l.set_preload_data(true);
    }
    l.on_init(&mut gfx);
    main().background.wait();
    frame(&mut gfx, &l);
    let path = format!("{}{BUILD}.xml", main().build_path.borrow());
    main().set_mode("BUILD", vec![ModeArg::Str(path), ModeArg::Str(BUILD.to_string())]);
    for _ in 0..3 {
        frame(&mut gfx, &l);
    }
    main().build_mode.wait_tree_assets();
    frame(&mut gfx, &l);
    let settled = frame(&mut gfx, &l);
    assert!(l.prompt_msg.borrow().is_none(), "{:?}", l.prompt_msg.borrow());
    let calc = {
        let b = main().build_mode.clone();
        let eng = b.engine.borrow();
        eng.as_ref().expect("engine").output_json().to_string()
    };
    let parallel = pob_gfx::image::PARALLEL_DRAWS.load(std::sync::atomic::Ordering::Relaxed);
    std::fs::write(out, format!("{parallel}\n{calc}\n{}", seen(&settled))).expect("write");
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
    let (desktop, web) = (run("desktop"), run("web"));
    // the first line: the draws that waited for their image at the end of a frame
    let (d0, w0) = (desktop.lines().next().unwrap_or(""), web.lines().next().unwrap_or(""));
    assert_eq!(d0, "0", "the desktop decodes in place");
    assert!(w0.parse::<usize>().is_ok_and(|n| n > 20), "the web way decoded side by side: {w0}");
    println!("draws that waited for the frame's end: {w0}");
    let rest = |s: &str| s.lines().skip(1).collect::<Vec<_>>().join("\n");
    let (desktop, web) = (rest(&desktop), rest(&web));
    assert!(desktop.lines().count() > 500, "a drawn tree: {} lines", desktop.lines().count());
    if desktop != web {
        let at = desktop.lines().zip(web.lines()).position(|(a, b)| a != b);
        panic!("the web switches changed line {at:?} of {}", desktop.lines().count());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
