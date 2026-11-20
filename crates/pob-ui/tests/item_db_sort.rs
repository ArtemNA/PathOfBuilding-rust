//! The unique list of the items tab sorted by a calculated stat (`Sort by Full DPS`, ...) builds in a frame without a panic: the
//! sort runs a calculation per item and slot, and that must not meet a borrow the frame already holds
//! (user report 2026-11-20: `In 'OnFrame': RefCell already borrowed`).
//!   cargo test -p pob-ui --release --test item_db_sort -- --nocapture

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::{Gfx, RawEvent, TestPlatform};
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::main;
use std::rc::Rc;

fn frame(gfx: &mut Gfx, l: &Rc<Launch>) {
    let mut f = gfx.begin_frame();
    l.on_frame(&mut f);
    f.finish();
}

fn open(build: &std::path::Path, tag: &str) -> (Gfx, Rc<Launch>) {
    let root = std::env::temp_dir().join(format!("pob-ui-item-db-sort-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let builds = root.join("Path of Building/Builds");
    std::fs::create_dir_all(&builds).expect("mkdir");
    std::fs::copy(build, builds.join("b.xml")).expect("copy build");
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
    pob_ui::tabs::items::load_item_dbs_step(None);
    for _ in 0..4 {
        frame(&mut gfx, &l);
    }
    main().set_mode(
        "BUILD",
        vec![
            pob_ui::modules::main::ModeArg::Str(format!("{}b.xml", main().build_path.borrow())),
            pob_ui::modules::main::ModeArg::Str("b".into()),
        ],
    );
    for _ in 0..3 {
        frame(&mut gfx, &l);
    }
    (gfx, l)
}

fn sort_uniques_by(gfx: &mut Gfx, l: &Rc<Launch>, slot: usize, mode: &str) -> Result<usize, String> {
    let b = main().build_mode.clone();
    *b.view_mode.borrow_mut() = "ITEMS".into();
    frame(gfx, l);
    let t = b.items_tab.borrow().clone().expect("itemsTab");
    let db = t.unique_db.borrow().clone().expect("uniqueDB");
    db.slot.sel_index.set(slot);
    *db.sort_mode.borrow_mut() = mode.into();
    db.list_build_flag.set(true);
    frame(gfx, l);
    if let Some(msg) = l.prompt_msg.borrow().clone() {
        return Err(msg);
    }
    Ok(db.list.list.borrow().len())
}

/// The builds: `POB_ITEM_DB_SORT_BUILD` (one XML or a folder of them, searched recursively), else the corpus build.
fn builds() -> Vec<std::path::PathBuf> {
    fn walk(p: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        if p.is_dir() {
            let mut es: Vec<_> = std::fs::read_dir(p).expect("read_dir").flatten().map(|e| e.path()).collect();
            es.sort();
            for e in es {
                walk(&e, out);
            }
        } else if p.extension().is_some_and(|x| x == "xml") {
            out.push(p.to_path_buf());
        }
    }
    let mut out = Vec::new();
    match std::env::var_os("POB_ITEM_DB_SORT_BUILD") {
        Some(p) => walk(std::path::Path::new(&p), &mut out),
        None => out.push(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../builds/corpus/Dual Wield Cospris CoC - Lethal Pride.xml"
            )
            .into(),
        ),
    }
    out
}

#[test]
fn uniques_sorted_by_a_calculated_stat_build_without_a_panic() {
    let mut failures = Vec::new();
    for (i, build) in builds().iter().enumerate() {
        let (mut gfx, l) = open(build, &i.to_string());
        // slot filter: "Any slot" (1), "Weapon 1" (2) -- the user's report had "Weapon 1"
        for (slot, mode) in [(2, "FullDPS"), (1, "FullDPS"), (1, "CombinedDPS"), (1, "Life")] {
            match sort_uniques_by(&mut gfx, &l, slot, mode) {
                Ok(n) => eprintln!("{}: slot {slot} {mode}: {n} items", build.display()),
                Err(e) => failures.push(format!("{}: slot {slot} {mode}: {e}", build.display())),
            }
            l.prompt_msg.borrow_mut().take();
        }
        drop(l);
        pob_ui::modules::main::clear_main();
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
