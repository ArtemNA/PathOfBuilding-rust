//! `Launch::set_sign_in(false)` (a host without an OAuth sign-in, the web application): the Import tab has no "Import From Your Account"
//! section, and "Import By Account Name" takes its place. On (the desktop, the default) the layout is upstream's: the sg-oracle scenes of
//! `tests/import.rs` hold it to the pixel.

#[path = "../../pob-gfx/tests/scene/mod.rs"]
mod scene;

use pob_gfx::{Gfx, RawEvent, TestPlatform};
use pob_ui::launch::{Launch, LaunchConfig};
use pob_ui::modules::main::{ModeArg, main};
use std::rc::Rc;

const OAUTH: [&str; 6] = [
    "sectionOauthCharImport",
    "charImportStatusLabel",
    "authenticateButton",
    "oauthErrorLabel",
    "accountRealm",
    "charImportTree",
];

struct Layout {
    shown: Vec<(&'static str, bool)>,
    oauth_pos: (f64, f64),
    oauth_bottom: f64,
    site_pos: (f64, f64),
    site_shown: bool,
}

fn layout(sign_in: Option<bool>) -> Layout {
    let root = std::env::temp_dir().join(format!("pob-ui-sign-in-{}-{sign_in:?}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Path of Building/Builds")).expect("mkdir");
    let tp = TestPlatform::new();
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, Box::new(tp)).expect("fonts");
    gfx.set_dpi_scale_override_percent(100);
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
        app_version: None,
    });
    if let Some(on) = sign_in {
        l.set_sign_in(on);
    }
    l.on_init(&mut gfx);
    assert!(l.prompt_msg.borrow().is_none(), "startup error: {:?}", l.prompt_msg.borrow());
    main().background.wait();
    main().set_mode("BUILD", vec![ModeArg::Bool(false), ModeArg::Str("Unnamed build".into())]);
    for _ in 0..3 {
        let mut f = gfx.begin_frame();
        l.on_frame(&mut f);
        f.finish();
    }
    *main().build_mode.view_mode.borrow_mut() = "IMPORT".into();
    for _ in 0..2 {
        let mut f = gfx.begin_frame();
        l.on_frame(&mut f);
        f.finish();
    }
    assert!(l.prompt_msg.borrow().is_none(), "error: {:?}", l.prompt_msg.borrow());
    let tab = main().build_mode.import_tab.borrow().clone().expect("importTab");
    let get = |n: &str| tab.host.get(n).unwrap_or_else(|| panic!("control {n}"));
    let oauth = get("sectionOauthCharImport");
    let site = get("sectionCharSiteImport");
    let out = Layout {
        shown: OAUTH.iter().map(|n| (*n, get(n).is_shown(&gfx))).collect(),
        oauth_pos: oauth.get_pos(&gfx),
        oauth_bottom: oauth.get_pos(&gfx).1 + oauth.get_size(&gfx).1,
        site_pos: site.get_pos(&gfx),
        site_shown: site.is_shown(&gfx),
    };
    let _ = std::fs::remove_dir_all(&root);
    out
}

#[test]
fn without_sign_in_the_account_section_is_gone() {
    let off = layout(Some(false));
    for (n, shown) in &off.shown {
        assert!(!shown, "{n} is shown without a sign-in");
    }
    assert!(off.site_shown, "Import By Account Name is shown");
    assert_eq!(
        off.site_pos, off.oauth_pos,
        "Import By Account Name takes the place of the sign-in section"
    );
}

#[test]
fn with_sign_in_the_layout_is_upstreams() {
    for l in [layout(None), layout(Some(true))] {
        let shown: Vec<_> = l.shown.iter().filter(|(_, s)| *s).map(|(n, _)| *n).collect();
        // not authorized: the section, its status, the button and the error label (ImportTab.lua:24-197); the character controls wait
        assert_eq!(
            shown,
            [
                "sectionOauthCharImport",
                "charImportStatusLabel",
                "authenticateButton",
                "oauthErrorLabel"
            ],
            "shown with a sign-in"
        );
        assert!(l.site_shown);
        // ImportTab.lua:493: `{"TOPLEFT", sectionOauthCharImport, "BOTTOMLEFT"}, {0, 18, ...}`
        assert_eq!(l.site_pos, (l.oauth_pos.0, l.oauth_bottom + 18.0));
    }
}
