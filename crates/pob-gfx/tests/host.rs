//! GPU-free tests of the U1.3/U1.4 host API against the SG-oracle probes: `SetDrawColor(str)` acceptance
//! (`fixtures/frame-probe/log.json`), `TakeScreenshot` frame choice, DPI override applied mid-frame, frame-start reset,
//! `DrawStringWidth` of the drift probe (`fixtures/text-probe.json`), the scripted platform (clock, clipboard, URLs),
//! window requests and console. Pixels of the screenshot probes: `tests/scenes.rs` (`frame_probe_equals_original`).

mod scene;

use pob_gfx::text::FontFamily;
use pob_gfx::{Gfx, GfxError, TestPlatform, WindowRequests};
use serde_json::Value;

fn gfx_with(tp: &TestPlatform) -> Gfx {
    Gfx::with_platform(&scene::fonts_dir(), (1280, 800), 1.0, Box::new(tp.clone())).expect("fonts")
}

fn fixture(rel: &str) -> Value {
    let p = scene::oracle_dir().join("fixtures").join(rel);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("json")
}

/// Every `SetDrawColor(str)` of the probe: accepted or "invalid color escape sequence", and the colour after the call
/// (the previous colour 0.25, 0.5, 0.75, 0.5 stays on error).
#[test]
fn set_draw_color_strings_match_original() {
    let log = fixture("frame-probe/log.json");
    let tp = TestPlatform::new();
    let mut g = gfx_with(&tp);
    let mut f = g.begin_frame();
    let mut n = 0;
    for e in log["colors"].as_array().expect("colors") {
        let s = e[0].as_str().expect("string");
        if s.starts_with('(') {
            continue; // numeric form
        }
        f.set_draw_color(0.25, 0.5, 0.75, 0.5);
        let r = f.set_draw_color_escape(s);
        assert_eq!(r.is_ok(), e[1].as_bool().expect("ok"), "{s:?}");
        if let Err(err) = r {
            assert_eq!(err.to_string(), e[2].as_str().expect("msg"), "{s:?}");
        }
        let want: Vec<f32> = e[3]
            .as_array()
            .expect("rgba")
            .iter()
            .map(|v| v.as_f64().expect("num") as f32)
            .collect();
        assert_eq!(f.get_draw_color().to_vec(), want, "{s:?}");
        n += 1;
    }
    assert_eq!(n, 17);
}

/// `TakeScreenshot()` in frame N captures frame N + 1 (the probe's red calling frame came out blue); a call between
/// frames captures the next frame; one call, one shot.
#[test]
fn take_screenshot_captures_the_next_frame() {
    let tp = TestPlatform::new();
    let mut g = gfx_with(&tp);
    let mut f = g.begin_frame();
    f.take_screenshot();
    f.finish();
    assert!(!g.frame_wants_screenshot(), "calling frame");
    g.begin_frame().finish();
    assert!(g.frame_wants_screenshot(), "next frame");
    g.begin_frame().finish();
    assert!(!g.frame_wants_screenshot(), "only once");
    g.take_screenshot(); // e.g. from OnKeyDown (Launch.lua: Ctrl+PRINTSCREEN)
    g.begin_frame().finish();
    assert!(g.frame_wants_screenshot());
}

/// Frame start: layer 0, full-screen viewport, colour kept (probe phase "viewport": all yellow).
#[test]
fn frame_start_resets_viewport_and_layer() {
    let tp = TestPlatform::new();
    let mut g = gfx_with(&tp);
    let mut f = g.begin_frame();
    f.set_viewport(Some([100.0, 100.0, 50.0, 50.0]));
    f.set_draw_layer(Some(3), Some(4)).unwrap();
    f.set_draw_color(0.0, 0.0, 1.0, 0.5);
    f.finish();
    let mut f = g.begin_frame();
    assert_eq!(f.get_draw_layer(), (0, 0));
    assert_eq!(f.get_draw_color(), [0.0, 0.0, 1.0, 0.5]);
    f.draw_image(None, 0.0, 0.0, 1280.0, 800.0, None).unwrap();
    let l = f.finish();
    assert_eq!(l.commands[0].clip, [0.0, 0.0, 1280.0, 800.0]);
}

/// `SetDPIScaleOverridePercent` mid-frame: `GetScreenScale` changes at once and only later draws use it (probe phase
/// "dpi": green 10,10,20,20 before the call at 100 %, red 100,100,100,100 after it at 150 %).
#[test]
fn dpi_override_applies_to_later_draws_of_the_same_frame() {
    let tp = TestPlatform::new();
    let mut g = gfx_with(&tp);
    let mut f = g.begin_frame();
    f.draw_image(None, 10.0, 10.0, 20.0, 20.0, None).unwrap();
    assert_eq!(f.get_screen_scale(), 1.0);
    f.set_dpi_scale_override_percent(150);
    assert_eq!(f.get_screen_scale(), 1.5);
    assert_eq!(f.get_virtual_screen_size(), (853.0, 533.0));
    f.draw_image(None, 100.0, 100.0, 100.0, 100.0, None).unwrap();
    let l = f.finish();
    assert_eq!(l.commands[0].pos[0], [10.0, 10.0]);
    assert_eq!(l.commands[1].pos[0], [150.0, 150.0]);
    assert_eq!(l.commands[1].pos[2], [300.0, 300.0]);
}

/// `DrawStringWidth` stays the integer rule of U1.1 for the strings whose *drawn* pen drifts (`text-probe.lua`).
#[test]
fn draw_string_width_of_drift_probe_matches_original() {
    let tp = TestPlatform::new();
    let g = gfx_with(&tp);
    let rows = fixture("text-probe.json");
    let rows = rows.as_array().expect("rows");
    for r in rows {
        let (h, s, w) = (r[0].as_f64().expect("h"), r[1].as_str().expect("s"), r[2].as_i64().expect("w"));
        assert_eq!(g.draw_string_width(h, FontFamily::Var, s), w, "h {h} {s:?}");
    }
    assert_eq!(rows.len(), 102);
}

#[test]
fn set_blend_mode_alpha_and_errors() {
    let tp = TestPlatform::new();
    let mut g = gfx_with(&tp);
    let mut f = g.begin_frame();
    assert_eq!(f.set_blend_mode("ALPHA"), Ok(()));
    let e = f.set_blend_mode("FOO").unwrap_err();
    assert_eq!(e, GfxError::InvalidBlendMode("FOO".into()));
    // scene u13-blend: "call 12 SetBlendMode: bad argument #1 to '?' (invalid option 'FOO')"
    assert_eq!(e.to_string(), "bad argument #1 to '?' (invalid option 'FOO')");
}

#[test]
fn platform_clock_clipboard_urls() {
    let tp = TestPlatform::new();
    let mut g = gfx_with(&tp);
    tp.set_time(1234);
    assert_eq!(g.get_time(), 1234);
    assert_eq!(g.paste(), None);
    g.copy("build code");
    assert_eq!(g.paste().as_deref(), Some("build code"));
    g.open_url("https://pathofbuilding.community/");
    assert_eq!(tp.state().opened, ["https://pathofbuilding.community/"]);
}

#[test]
fn window_requests_and_console() {
    let tp = TestPlatform::new();
    let mut g = gfx_with(&tp);
    g.set_window_title("Path of Building");
    assert!(!g.exit_requested());
    g.exit(None);
    assert!(g.exit_requested());
    assert_eq!(
        g.take_requests(),
        WindowRequests {
            title: Some("Path of Building".into()),
            exit: Some(None)
        }
    );
    assert_eq!(g.take_requests(), WindowRequests::default());
    g.con_execute("set vid_mode 8");
    g.con_execute("set vid_resizable 3");
    assert_eq!((g.cvar("vid_mode"), g.cvar("vid_resizable")), (Some("8"), Some("3")));
    g.con_printf("Loading main script...");
    assert_eq!(g.console().collect::<Vec<_>>(), ["Loading main script..."]);
    g.con_clear();
    assert_eq!(g.console().count(), 0);
}

/// `TakeScreenshot` file: 24-bit RGB PNG of the whole frame, named `%m%d%y_%H%M%S.png`.
#[test]
fn screenshot_file_is_rgb_png() {
    let dir = std::env::temp_dir().join(format!("pob-gfx-shot-{}", std::process::id()));
    let rgba: Vec<u8> = (0..4 * 6).map(|i| i as u8 * 10).collect();
    let p = pob_gfx::screenshot::save(&rgba, 3, 2, &dir).expect("save");
    let name = p.file_name().and_then(|n| n.to_str()).expect("name").to_string();
    assert_eq!(name.len(), "110226_024705.png".len());
    assert_eq!(&name[6..7], "_");
    let bytes = std::fs::read(&p).expect("read");
    assert_eq!(
        (bytes[24], bytes[25]),
        (8, 2),
        "bit depth 8, colour type 2 (RGB) like the original's shots"
    );
    let img = image::open(&p).expect("png").to_rgb8();
    assert_eq!(img.dimensions(), (3, 2));
    assert_eq!(img.get_pixel(1, 0).0, [40, 50, 60]);
    std::fs::remove_dir_all(&dir).expect("cleanup");
}
