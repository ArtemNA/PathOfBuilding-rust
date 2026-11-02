//! GPU-free tests of the U1.2 draw-list semantics (layers, viewport snapshots, colour carry, alignment snapping,
//! images). Pixel equality with the original is gated by `tests/scenes.rs`.

mod scene;

use pob_gfx::text::FontFamily;
use pob_gfx::{Align, DrawList, Gfx, GfxError, ImageHandle, LoadFlag};

fn gfx() -> Gfx {
    Gfx::new(&scene::fonts_dir(), (1280, 800), 1.0).expect("fonts")
}

fn test_png() -> std::path::PathBuf {
    scene::oracle_dir().join("scenes/img/test8.png")
}

/// Left x of each command's quad.
fn lefts(l: &DrawList) -> Vec<f32> {
    l.commands.iter().map(|c| c.pos[0][0]).collect()
}

#[test]
fn layers_sort_stably_at_frame_end() {
    let mut g = gfx();
    let mut f = g.begin_frame();
    let rect = |f: &mut pob_gfx::Frame<'_>, x: f64| f.draw_image(None, x, 0.0, 1.0, 1.0, None).expect("draw");
    f.set_draw_layer(Some(5), None).unwrap();
    rect(&mut f, 1.0);
    f.set_draw_layer(Some(0), None).unwrap();
    rect(&mut f, 2.0);
    f.set_draw_layer(None, Some(10)).unwrap();
    rect(&mut f, 3.0);
    f.set_draw_layer(Some(0), Some(-5)).unwrap();
    rect(&mut f, 4.0);
    f.set_draw_layer(Some(0), None).unwrap();
    rect(&mut f, 5.0);
    f.set_draw_layer(Some(-1), None).unwrap();
    rect(&mut f, 6.0);
    assert_eq!(f.get_draw_layer(), (-1, 0));
    assert_eq!(f.set_draw_layer(None, None), Err(GfxError::LayerNeedsSubLayer));
    let l = f.finish();
    assert_eq!(lefts(&l), [6.0, 4.0, 2.0, 5.0, 3.0, 1.0]);
}

#[test]
fn viewport_is_snapshotted_offset_and_rounded() {
    let mut g = gfx();
    let mut f = g.begin_frame();
    f.set_viewport(Some([10.5, 20.25, 30.25, 40.0]));
    f.draw_image(None, 1.0, 2.0, 3.0, 4.0, None).unwrap();
    f.set_viewport(None);
    f.draw_image(None, 1.0, 2.0, 3.0, 4.0, None).unwrap();
    let l = f.finish();
    // x 10.5 -> 11, y 20.25 -> 20, w 30.25 -> 31 (up), h 40.
    assert_eq!(l.commands[0].pos[0], [12.0, 22.0]);
    assert_eq!(l.commands[0].clip, [11.0, 20.0, 42.0, 60.0]);
    assert_eq!(l.commands[1].pos[0], [1.0, 2.0]);
    assert_eq!(l.commands[1].clip, [0.0, 0.0, 1280.0, 800.0]);
}

#[test]
fn escapes_carry_colour_with_alpha_one_and_colour_survives_frames() {
    let mut g = gfx();
    {
        let mut f = g.begin_frame();
        f.set_draw_color(1.0, 1.0, 1.0, 0.5);
        f.draw_string(0.0, 0.0, Align::Left, 16.0, FontFamily::Var, "a ^2b");
        let l = f.finish();
        assert_eq!(l.commands[0].tint, [1.0, 1.0, 1.0, 0.5]);
        assert_eq!(l.commands.last().unwrap().tint, [0.0, 1.0, 0.0, 1.0]);
    }
    assert_eq!(g.get_draw_color(), [0.0, 1.0, 0.0, 1.0]);
    let mut f = g.begin_frame();
    assert_eq!(f.get_draw_color(), [0.0, 1.0, 0.0, 1.0], "colour is not reset per frame");
    assert_eq!(f.get_draw_layer(), (0, 0));
    // Escape after the last glyph still applies.
    f.draw_string(0.0, 0.0, Align::Left, 16.0, FontFamily::Var, "x^4");
    assert_eq!(f.get_draw_color(), [1.0, 1.0, 0.0, 1.0]);
    assert!(f.set_draw_color_escape("bad").is_err());
    f.set_draw_color_escape("^xFF8000").unwrap();
    assert_eq!(f.get_draw_color(), [1.0, 128.0 / 255.0, 0.0, 1.0]);
}

#[test]
fn line_origins_snap_like_the_original() {
    let mut g = gfx();
    let w = g.draw_string_width(16.0, FontFamily::Var, "odd i");
    assert_eq!(w, 31);
    let first_x = |g: &mut Gfx, left: f64, top: f64, align: Align, text: &str| {
        let mut f = g.begin_frame();
        f.draw_string(left, top, align, 16.0, FontFamily::Var, text);
        let l = f.finish();
        let c = &l.commands[0];
        // The first glyph of these strings has spLeft 1 at VAR 16.
        (c.pos[0][0] - 1.0, c.pos[0][1])
    };
    assert_eq!(first_x(&mut g, 40.5, 10.5, Align::Left, "o"), (41.0, 10.0));
    assert_eq!(first_x(&mut g, -3.5, 30.75, Align::Left, "o"), (-4.0, 30.0));
    assert_eq!(first_x(&mut g, 200.0, 0.0, Align::CenterX, "odd i"), (184.0, 0.0));
    assert_eq!(first_x(&mut g, 231.5, 0.0, Align::RightX, "odd i"), (200.0, 0.0));
    // CENTER/RIGHT measure against the screen width (1280), not the viewport.
    assert_eq!(first_x(&mut g, 0.0, 0.0, Align::Center, "odd i"), (624.0, 0.0));
    assert_eq!(first_x(&mut g, 10.0, 0.0, Align::Right, "odd i"), (1239.0, 0.0));
}

#[test]
fn dpi_override_scales_geometry_and_virtual_size() {
    let mut g = gfx();
    g.set_dpi_scale_override_percent(150);
    assert_eq!(g.get_screen_scale(), 1.5);
    assert_eq!(g.get_virtual_screen_size(), (853.0, 533.0));
    let mut f = g.begin_frame();
    f.draw_image(None, 10.0, 10.0, 100.0, 20.0, None).unwrap();
    let l = f.finish();
    assert_eq!(l.commands[0].pos[2], [165.0, 45.0]);
}

#[test]
fn image_handles_sync_async_and_errors() {
    let mut g = gfx();
    let h = ImageHandle::new();
    assert_eq!(h.image_size(), (0, 0));
    {
        let mut f = g.begin_frame();
        assert_eq!(f.draw_image(Some(&h), 0.0, 0.0, 1.0, 1.0, None), Err(GfxError::NoImageLoaded));
    }
    h.load(test_png(), &[]).unwrap();
    assert_eq!(h.image_size(), (8, 8));
    assert!(h.is_valid());

    let a = ImageHandle::new();
    a.load(test_png(), &[LoadFlag::Async, LoadFlag::Clamp]).unwrap();
    // Until the background decode is picked up: size 0, drawing records nothing.
    {
        let mut f = g.begin_frame();
        let before = a.image_size();
        f.draw_image(Some(&a), 0.0, 0.0, 1.0, 1.0, None).unwrap();
        let l = f.finish();
        if before == (0, 0) {
            assert!(l.commands.is_empty());
        }
    }
    a.wait();
    assert_eq!(a.image_size(), (8, 8));
    assert!(a.texture().unwrap().clamp);
    assert!(!a.is_loading());

    let bad = ImageHandle::new();
    assert!(bad.load("no/such/file.png", &[]).is_err());
    assert_eq!(bad.image_size(), (0, 0));
    assert!(bad.load_lua(test_png(), &["WRONG"]).is_err());
}
