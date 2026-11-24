//! `image::PARALLEL_DECODE` (the web application): the images a frame draws for the first time decode side by side and the frame waits for them
//! once, at `Frame::finish`. The frame is the frame of the decode in place: the same commands in the same order, the same pixels; an image whose
//! pixels fail draws nothing in both. Its own test binary: the switch is global.

mod scene;

use pob_gfx::{DrawList, Gfx, ImageHandle, LoadFlag};
use std::sync::atomic::Ordering;

fn test_png() -> std::path::PathBuf {
    scene::oracle_dir().join("scenes/img/test8.png")
}

/// Sheets of the tree, of other sizes.
fn sheets() -> Vec<std::path::PathBuf> {
    let d = std::path::PathBuf::from(format!("{}/src/TreeData", scene::pob_root()));
    ["Background2.png", "PSGroupBackground1.png", "3_29/frame-3.png"]
        .iter()
        .map(|f| d.join(f))
        .collect()
}

/// What the frame draws: each command with its texture's size and pixels instead of the texture's id.
fn seen(l: &DrawList) -> Vec<String> {
    l.commands
        .iter()
        .map(|c| {
            let px = c.texture.pixels().map(|p| {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                p.hash(&mut h);
                h.finish()
            });
            format!(
                "{} {} {:?} {:?} {:?} {:?} {}x{} {px:?}",
                c.layer, c.sub_layer, c.pos, c.uv, c.tint, c.clip, c.texture.width, c.texture.height
            )
        })
        .collect()
}

/// One frame: every image twice (the second draw finds it decoding or decoded), one with a wrong size (fails), text and solid colour between.
fn frame(gfx: &mut Gfx, parallel: bool) -> (DrawList, Vec<String>) {
    pob_gfx::image::PARALLEL_DECODE.store(parallel, Ordering::Relaxed);
    let mut paths = sheets();
    paths.push(test_png());
    let handles: Vec<ImageHandle> = paths
        .iter()
        .map(|p| {
            let h = ImageHandle::new();
            h.load_lazy(p, &[LoadFlag::Clamp]).expect("load");
            h
        })
        .collect();
    let wrong = ImageHandle::new();
    wrong.load_lazy_sized(test_png(), &[], Some((8, 9))).expect("load");
    let mut f = gfx.begin_frame();
    let mut states = Vec::new();
    for (i, h) in handles.iter().chain(std::iter::once(&wrong)).enumerate() {
        let x = 10.0 * i as f64;
        f.set_draw_layer(Some(i as i32 % 2), None).expect("layer");
        f.draw_image(Some(h), x, 5.0, 40.0, 30.0, Some([0.1, 0.2, 0.9, 0.8])).expect("draw");
        f.draw_image(None, x, 50.0, 4.0, 4.0, None).expect("solid");
        f.draw_image_quad(Some(h), [x, 0.0, x + 8.0, 0.0, x + 8.0, 8.0, x, 8.0], None)
            .expect("quad");
        states.push(format!("{h:?}"));
    }
    let list = f.finish();
    pob_gfx::image::PARALLEL_DECODE.store(false, Ordering::Relaxed);
    (list, states)
}

#[test]
fn the_parallel_frame_is_the_frame_in_place() {
    let mut gfx = Gfx::new(&scene::fonts_dir(), (1280, 800), 1.0).expect("fonts");
    let (in_place, during) = frame(&mut gfx, false);
    let (parallel, during_parallel) = frame(&mut gfx, true);
    assert_eq!(seen(&parallel), seen(&in_place));
    // 4 images x 2 draws + 5 solid squares; the failed image is in neither
    assert_eq!(in_place.commands.len(), 13);
    // in place each image was decoded where it was drawn; side by side none was by the end of its draws
    assert!(during.iter().take(4).all(|s| !s.contains("loading")), "{during:?}");
    assert!(during_parallel.iter().take(4).all(|s| s.contains("loading")), "{during_parallel:?}");
}
