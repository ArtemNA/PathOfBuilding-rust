//! Switching loadouts in any order gives the numbers a fresh load of the same state gives: nothing of the loadout before may stay.
//! The build comes from `POB_LOADOUT_BUILD` (a path to a build XML with loadouts; personal, never in git): without it the test only says so.
//!   POB_LOADOUT_BUILD=<path> cargo test -p pob-ui --release --test loadouts -- --nocapture

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_engine::engine::Engine;
use pob_gfx::{Gfx, RawEvent, TestPlatform};
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::main;
use std::rc::Rc;

const KEYS: [&str; 6] = ["CombinedDPS", "TotalEHP", "Life", "Armour", "FireResist", "PhysicalMaximumHitTaken"];

fn frame(gfx: &mut Gfx, l: &Rc<Launch>) {
    let mut f = gfx.begin_frame();
    l.on_frame(&mut f);
    f.finish();
}

fn stats(e: &Engine) -> Vec<String> {
    KEYS.iter().map(|k| format!("{k}={:?}", e.result().stats.get(*k))).collect()
}

#[test]
fn loadouts_in_any_order_give_a_fresh_load_s_numbers() {
    let Some(path) = std::env::var_os("POB_LOADOUT_BUILD") else {
        eprintln!("SKIPPED: set POB_LOADOUT_BUILD to a build with loadouts");
        return;
    };
    let order: Vec<String> = std::env::var("POB_LOADOUT_ORDER")
        .unwrap_or_else(|_| "69-90,90+,Midgame to Endgame,90+,69-90,Midgame to Endgame,69-90".into())
        .split(',')
        .map(str::to_owned)
        .collect();
    let root = std::env::temp_dir().join(format!("pob-ui-loadouts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let builds = root.join("Path of Building/Builds");
    std::fs::create_dir_all(&builds).expect("mkdir");
    std::fs::copy(&path, builds.join("lo.xml")).expect("copy build");
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
            pob_ui::modules::main::ModeArg::Str(format!("{}lo.xml", main().build_path.borrow())),
            pob_ui::modules::main::ModeArg::Str("lo".into()),
        ],
    );
    for _ in 0..3 {
        frame(&mut gfx, &l);
    }
    let mut failures = Vec::new();
    let mut seen: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for (step, name) in order.iter().enumerate() {
        let b = main().build_mode.clone();
        let drop = b.c().build_loadouts.clone();
        let idx = drop.list.borrow().iter().position(|v| v.label.as_deref() == Some(name.as_str())).unwrap_or_else(|| panic!("no loadout {name}"));
        let item = drop.list.borrow()[idx].clone();
        let f = drop.sel_func.borrow().clone().expect("selFunc");
        f(&mut gfx, idx + 1, &item);
        for _ in 0..3 {
            frame(&mut gfx, &l);
        }
        let (now, fresh) = {
            let eng = b.engine.borrow();
            let e = eng.as_ref().expect("engine");
            let fresh = Engine::load_xml(&e.save_xml().expect("save")).expect("reload");
            (stats(e), stats(&fresh))
        };
        eprintln!("{step:>2} {name:<20} {}", now.join("  "));
        if std::env::var_os("POB_LOADOUT_DIAG").is_some() {
            let mut eng = b.engine.borrow_mut();
            let e = eng.as_mut().expect("engine");
            let m = e.model();
            eprintln!(
                "     model: spec {} items {} skills {} config {}; live env spec nodes {}",
                m.tree.active_spec,
                m.active_item_set,
                m.skills.active_skill_set,
                m.config.active_config_set,
                e.spec_rc().alloc_nodes.len()
            );
            e.refresh().expect("refresh");
            eprintln!("     after a refresh: {}", stats(e).join("  "));
        }
        if now != fresh {
            failures.push(format!("step {step} {name}: switched {now:?}\n    fresh load {fresh:?}"));
        }
        if let Some(first) = seen.get(name)
            && *first != now
        {
            failures.push(format!("step {step} {name}: differs from its first visit\n    first {first:?}\n    now   {now:?}"));
        }
        seen.entry(name.clone()).or_insert(now);
    }
    // each set alone, in mixes no loadout has: tree, items, skills, config, then back down
    let b = main().build_mode.clone();
    let (specs, items, skills, configs) = {
        let eng = b.engine.borrow();
        let e = eng.as_ref().expect("engine");
        let m = e.model();
        (
            (1..=m.tree.specs.len()).collect::<Vec<_>>(),
            m.item_sets.iter().map(|s| s.id).collect::<Vec<_>>(),
            e.skill_sets().into_iter().map(|s| s.0).collect::<Vec<_>>(),
            m.config.sets.iter().map(|s| s.id).collect::<Vec<_>>(),
        )
    };
    let mut moves: Vec<(&str, f64)> = Vec::new();
    for round in 0..2 {
        let rev = |v: Vec<f64>| if round == 0 { v } else { v.into_iter().rev().collect() };
        moves.extend(rev(specs.iter().map(|&i| i as f64).collect()).into_iter().map(|i| ("tree", i)));
        moves.extend(rev(items.clone()).into_iter().map(|i| ("items", i)));
        moves.extend(rev(skills.clone()).into_iter().map(|i| ("skills", i)));
        moves.extend(rev(configs.clone()).into_iter().map(|i| ("config", i)));
    }
    for (kind, id) in moves {
        match kind {
            "tree" => b.tree_tab_made(&mut gfx).set_active_spec(&mut gfx, id as usize),
            "items" => b.items_tab.borrow().clone().expect("itemsTab").set_active_item_set(&mut gfx, id),
            "skills" => b.skills_tab.borrow().clone().expect("skillsTab").set_active_skill_set(&mut gfx, id),
            _ => b.config_tab.borrow().clone().expect("configTab").set_active_config_set(&mut gfx, id),
        }
        for _ in 0..3 {
            frame(&mut gfx, &l);
        }
        let eng = b.engine.borrow();
        let e = eng.as_ref().expect("engine");
        let fresh = Engine::load_xml(&e.save_xml().expect("save")).expect("reload");
        let (now, fresh) = (stats(e), stats(&fresh));
        eprintln!("   {kind:<6} {id:>3} {}", now.join("  "));
        if now != fresh {
            failures.push(format!("{kind} set {id}: switched {now:?}
    fresh load {fresh:?}"));
        }
    }
    failures.extend(pob_ui::modules::build::take_fresh_mismatches());
    pob_ui::modules::main::clear_main();
    let _ = std::fs::remove_dir_all(&root);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
