//! The unique list of the items tab sorted by a calculated stat (`Sort by Full DPS`, ...) builds in a frame without a panic: the
//! sort runs a calculation per item and slot, and that must not meet a borrow the frame already holds
//! (user report 2026-11-20: `In 'OnFrame': RefCell already borrowed`).
//! And the sort spreads over the frames like upstream's coroutine (ItemDBControl.lua:250-323, 50 ms a frame): a change of the build while the
//! list is sorted by a stat re-measures every item, and in one frame that held the window for seconds (user report 2026-11-20).
//!   cargo test -p pob-ui --release --test item_db_sort -- --nocapture

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::{Gfx, Platform, RawEvent, TestPlatform};
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::main;
use std::rc::Rc;

fn frame(gfx: &mut Gfx, l: &Rc<Launch>) {
    let mut f = gfx.begin_frame();
    l.on_frame(&mut f);
    f.finish();
}

/// `GetTime()` of the running program: the wall clock.
struct WallClock(std::time::Instant);

impl Platform for WallClock {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
    fn copy(&mut self, _text: &str) {}
    fn paste(&mut self) -> Option<String> {
        None
    }
    fn open_url(&mut self, _url: &str) {}
}

fn open(build: &std::path::Path, tag: &str) -> (Gfx, Rc<Launch>) {
    open_with(build, tag, Box::new(TestPlatform::new()))
}

fn open_with(build: &std::path::Path, tag: &str, platform: Box<dyn Platform>) -> (Gfx, Rc<Launch>) {
    let root = std::env::temp_dir().join(format!("pob-ui-item-db-sort-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let builds = root.join("Path of Building/Builds");
    std::fs::create_dir_all(&builds).expect("mkdir");
    std::fs::copy(build, builds.join("b.xml")).expect("copy build");
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, platform).expect("fonts");
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
    while db.is_building() && l.prompt_msg.borrow().is_none() {
        frame(gfx, l);
    }
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

fn names() -> Vec<String> {
    let t = main().build_mode.items_tab.borrow().clone().expect("itemsTab");
    let db = t.unique_db.borrow().clone().expect("uniqueDB");
    let rows = db.list.list.borrow();
    rows.iter()
        .map(|r| match r {
            pob_ui::tabs::items::ItemRow::Obj(i) => i.name.clone(),
            pob_ui::tabs::items::ItemRow::Id(id) => id.to_string(),
        })
        .collect()
}

#[test]
fn a_stat_sort_spreads_over_the_frames() {
    let build = &builds()[0];
    // the order of a frozen clock: the whole list in one frame
    let (mut gfx, l) = open(build, "frozen");
    sort_uniques_by(&mut gfx, &l, 2, "FullDPS").expect("sorted");
    let want = names();
    drop(l);
    pob_ui::modules::main::clear_main();

    let (mut gfx, l) = open_with(build, "wall", Box::new(WallClock(std::time::Instant::now())));
    sort_uniques_by(&mut gfx, &l, 2, "FullDPS").expect("sorted");
    assert_eq!(names(), want, "the same order as in one frame");
    let t = main().build_mode.items_tab.borrow().clone().expect("itemsTab");
    let db = t.unique_db.borrow().clone().expect("uniqueDB");
    // a change of the build: the list is built again over the frames, and the window keeps drawing
    main().build_mode.build_flag.set(true);
    let mut ms = Vec::new();
    let all = std::time::Instant::now();
    loop {
        let t0 = std::time::Instant::now();
        frame(&mut gfx, &l);
        ms.push(t0.elapsed().as_secs_f64() * 1e3);
        if !db.is_building() {
            break;
        }
    }
    let total = all.elapsed().as_secs_f64() * 1e3;
    // the first frame also recalculates the build
    let worst = ms.iter().skip(1).copied().fold(0.0, f64::max);
    eprintln!(
        "rebuild of {} items: {} frames, {total:.0} ms, first {:.0} ms, the worst of the rest {worst:.0} ms",
        want.len(),
        ms.len(),
        ms[0]
    );
    assert!(l.prompt_msg.borrow().is_none(), "{:?}", l.prompt_msg.borrow());
    assert_eq!(names(), want, "the same order after the change");
    assert!(
        ms.len() > 2 && total > 150.0,
        "the gate needs a list that takes longer than a few frames: {total:.0} ms"
    );
    assert!(ms[0] < 150.0, "the first frame holds the window: {:.0} ms", ms[0]);
    assert!(worst < 150.0, "a frame of the sort holds the window: {worst:.0} ms");
}
