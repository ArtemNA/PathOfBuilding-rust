//! `image::DRAW_WHEN_READY` (the web application may turn it on): a tree image not decoded yet draws nothing and does not hold the frame; its
//! decode runs on a thread and a later frame draws it, and from then on the frame is the frame of the decode in place. An image whose pixels fail
//! never draws. Its own test binary: the switch is global.

mod scene;

use pob_gfx::{DrawList, Gfx, ImageHandle, LoadFlag};
use std::sync::atomic::Ordering;

fn images() -> Vec<std::path::PathBuf> {
    let d = std::path::PathBuf::from(format!("{}/src/TreeData", scene::pob_root()));
    vec![
        d.join("Background2.png"),
        d.join("PSGroupBackground1.png"),
        d.join("3_29/frame-3.png"),
        scene::oracle_dir().join("scenes/img/test8.png"),
    ]
}

fn handles() -> Vec<ImageHandle> {
    let mut hs: Vec<ImageHandle> = images()
        .iter()
        .map(|p| {
            let h = ImageHandle::new();
            h.load_lazy(p, &[LoadFlag::Clamp]).expect("load");
            h
        })
        .collect();
    let wrong = ImageHandle::new();
    wrong
        .load_lazy_sized(scene::oracle_dir().join("scenes/img/test8.png"), &[], Some((8, 9)))
        .expect("load");
    hs.push(wrong);
    hs
}

fn seen(l: &DrawList) -> Vec<String> {
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
                "{} {:?} {:?} {:?} {}x{} {px:?}",
                c.layer, c.pos, c.uv, c.tint, c.texture.width, c.texture.height
            )
        })
        .collect()
}

fn frame(gfx: &mut Gfx, hs: &[ImageHandle]) -> DrawList {
    let mut f = gfx.begin_frame();
    for (i, h) in hs.iter().enumerate() {
        let x = 10.0 * i as f64;
        f.draw_image(Some(h), x, 5.0, 40.0, 30.0, None).expect("draw");
        f.draw_image(None, x, 50.0, 4.0, 4.0, None).expect("solid");
        f.draw_image_quad(Some(h), [x, 0.0, x + 8.0, 0.0, x + 8.0, 8.0, x, 8.0], None)
            .expect("quad");
    }
    f.finish()
}

#[test]
fn a_tree_image_draws_once_it_is_ready() {
    let mut gfx = Gfx::new(&scene::fonts_dir(), (1280, 800), 1.0).expect("fonts");
    let in_place = frame(&mut gfx, &handles());

    pob_gfx::image::DRAW_WHEN_READY.store(true, Ordering::Relaxed);
    let hs = handles();
    let first = frame(&mut gfx, &hs);
    // the first frame: the solid squares only, and the decodes have started
    assert_eq!(first.commands.len(), 5, "nothing waits for an image: {:?}", seen(&first));
    assert!(hs.iter().take(4).all(|h| format!("{h:?}").contains("loading")));
    let t = std::time::Instant::now();
    let last = loop {
        let l = frame(&mut gfx, &hs);
        if l.commands.len() == in_place.commands.len() || t.elapsed().as_secs() > 20 {
            break l;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    pob_gfx::image::DRAW_WHEN_READY.store(false, Ordering::Relaxed);
    assert_eq!(seen(&last), seen(&in_place), "once ready, the frame of the decode in place");
    assert_eq!(
        in_place.commands.len(),
        13,
        "4 images twice and 5 squares; the failed one in neither"
    );
}
