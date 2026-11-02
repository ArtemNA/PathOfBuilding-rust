//! Interpreter of SG-oracle scenes (`tools/sg-oracle/scenes/*.json`, format in `lua/scene-runner.lua`) on `pob-gfx`,
//! plus the pixel comparator against the original's screenshots `tools/sg-oracle/fixtures/scenes/*.png`.
//! Shared by `tests/scenes.rs` (gate) and `examples/scene.rs` (PNG dump + perf) via `#[path]`.

#![allow(dead_code)]

use pob_gfx::text::FontFamily;
use pob_gfx::{Align, DrawList, Gfx, ImageHandle};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_POB_ROOT: &str = "C:/pob-work/project_15/PathOfBuilding";
/// The oracle's fixed window (scene-runner.lua: `vid_last 1280,800,…`).
pub const SCREEN: (u32, u32) = (1280, 800);

pub fn fonts_dir() -> PathBuf {
    let root = std::env::var_os("POB_ROOT").map_or_else(|| PathBuf::from(DEFAULT_POB_ROOT), PathBuf::from);
    root.join("runtime/SimpleGraphic/Fonts")
}

pub fn oracle_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/sg-oracle")
}

/// One scene call, decoded from JSON once (so timing a frame measures the API, not JSON matching).
pub enum Call {
    Log,
    Layer(Option<i32>, Option<i32>),
    Viewport(Option<[f64; 4]>),
    Color([f64; 4]),
    ColorEscape(String),
    Image(Option<String>, [f64; 4], Option<[f64; 4]>),
    Quad(Option<String>, [f64; 8], Option<[f64; 8]>),
    Text(f64, f64, Align, f64, FontFamily, String),
}

fn decode(name: &str, call: &[Value]) -> Call {
    let a = &call[1..];
    let num = |v: &Value| v.as_f64().unwrap_or_else(|| panic!("{name}: number expected, got {v}"));
    let opt_i = |v: Option<&Value>| v.and_then(Value::as_f64).map(|x| x as i32);
    let img = |v: &Value| v.as_str().map(str::to_string);
    match call[0].as_str().expect("api name") {
        "@log" => {
            assert_eq!(a[0], "GetDrawColor", "only GetDrawColor is logged");
            Call::Log
        }
        "SetDrawLayer" => Call::Layer(opt_i(a.first()), opt_i(a.get(1))),
        "SetViewport" => Call::Viewport((!a.is_empty()).then(|| [num(&a[0]), num(&a[1]), num(&a[2]), num(&a[3])])),
        "SetDrawColor" => match a[0].as_str() {
            Some(s) => Call::ColorEscape(s.to_string()),
            None => Call::Color([num(&a[0]), num(&a[1]), num(&a[2]), a.get(3).map_or(1.0, num)]),
        },
        "DrawImage" => Call::Image(
            img(&a[0]),
            [num(&a[1]), num(&a[2]), num(&a[3]), num(&a[4])],
            (a.len() >= 9).then(|| [num(&a[5]), num(&a[6]), num(&a[7]), num(&a[8])]),
        ),
        "DrawImageQuad" => Call::Quad(
            img(&a[0]),
            std::array::from_fn(|k| num(&a[1 + k])),
            (a.len() >= 17).then(|| std::array::from_fn(|k| num(&a[9 + k]))),
        ),
        "DrawString" => Call::Text(
            num(&a[0]),
            num(&a[1]),
            Align::from_lua_name(a[2].as_str()).expect("align"),
            num(&a[3]),
            FontFamily::from_lua_name(a[4].as_str().expect("font")).expect("font name"),
            a[5].as_str().expect("text").to_string(),
        ),
        other => panic!("scene {name}: unsupported call {other}"),
    }
}

pub struct Scene {
    pub name: String,
    pub size: (u32, u32),
    pub dpi: u32,
    pub images: HashMap<String, ImageHandle>,
    pub calls: Vec<Call>,
}

impl Scene {
    pub fn load(name: &str) -> Scene {
        let dir = oracle_dir();
        let p = dir.join("scenes").join(format!("{name}.json"));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let size = v["size"].as_array().expect("size");
        let mut images = HashMap::new();
        if let Some(m) = v["images"].as_object() {
            for (k, spec) in m {
                let spec = spec.as_array().expect("image spec");
                let path = dir.join(spec[0].as_str().expect("path"));
                let flags: Vec<&str> = spec[1..].iter().map(|f| f.as_str().expect("flag")).collect();
                let h = ImageHandle::new();
                h.load_lua(&path, &flags).unwrap_or_else(|e| panic!("load {k}: {e}"));
                h.wait();
                images.insert(k.clone(), h);
            }
        }
        Scene {
            name: name.to_string(),
            size: (size[0].as_u64().expect("w") as u32, size[1].as_u64().expect("h") as u32),
            dpi: v["dpi"].as_u64().unwrap_or(100) as u32,
            images,
            calls: v["calls"].as_array().expect("calls").iter().map(|c| decode(name, c.as_array().expect("call"))).collect(),
        }
    }

    /// Scene names in the oracle's order (`scenes/index.json`).
    pub fn names() -> Vec<String> {
        let p = oracle_dir().join("scenes/index.json");
        serde_json::from_str(&std::fs::read_to_string(p).expect("index.json")).expect("index json")
    }

    /// Physical crop of the oracle screenshot: `ceil(size * dpi / 100)` (run.ps1).
    pub fn crop(&self) -> (u32, u32) {
        let k = f64::from(self.dpi) / 100.0;
        ((f64::from(self.size.0) * k).ceil() as u32, (f64::from(self.size.1) * k).ceil() as u32)
    }

    /// One frame of the scene as the runner draws it: reset (`SetDrawLayer(0,0)`, `SetViewport()`,
    /// `SetDrawColor(1,1,1,1)`), then the calls. Returns the sorted draw list and the `@log` results.
    pub fn run(&self, gfx: &mut Gfx) -> (DrawList, Vec<(usize, [f32; 4])>) {
        gfx.set_dpi_scale_override_percent(self.dpi);
        let mut f = gfx.begin_frame();
        f.set_draw_layer(Some(0), Some(0)).expect("layer");
        f.set_viewport(None);
        f.set_draw_color(1.0, 1.0, 1.0, 1.0);
        let mut logs = Vec::new();
        let img = |k: &Option<String>| k.as_ref().map(|k| &self.images[k]);
        for (i, call) in self.calls.iter().enumerate() {
            match call {
                Call::Log => logs.push((i + 1, f.get_draw_color())),
                Call::Layer(l, s) => f.set_draw_layer(*l, *s).expect("SetDrawLayer"),
                Call::Viewport(v) => f.set_viewport(*v),
                Call::Color([r, g, b, a]) => f.set_draw_color(*r, *g, *b, *a),
                Call::ColorEscape(s) => f.set_draw_color_escape(s).expect("SetDrawColor escape"),
                Call::Image(k, [l, t, w, h], tc) => f.draw_image(img(k), *l, *t, *w, *h, *tc).expect("DrawImage"),
                Call::Quad(k, xy, st) => f.draw_image_quad(img(k), *xy, *st).expect("DrawImageQuad"),
                Call::Text(l, t, al, h, font, text) => f.draw_string(*l, *t, *al, *h, *font, text),
            }
        }
        (f.finish(), logs)
    }
}

/// Tolerance per channel (UI-ARCH §5.2: 2/255).
pub const TOLERANCE: u8 = 2;

#[derive(Debug, Default, Clone)]
pub struct Diff {
    pub pixels: usize,
    pub bad: usize,
    pub max_delta: u8,
    /// Pixels inside glyph quads (text) and how many of them are out of tolerance.
    pub text_pixels: usize,
    pub text_bad: usize,
    pub text_max_delta: u8,
    /// Bounding box of out-of-tolerance pixels: x0, y0, x1, y1 (inclusive).
    pub bbox: Option<[u32; 4]>,
}

impl Diff {
    pub fn frac(&self) -> f64 {
        self.bad as f64 / self.pixels.max(1) as f64
    }
}

/// Text mask: every pixel whose centre lies in a glyph quad's bounding box (and inside its clip rectangle).
pub fn text_mask(list: &DrawList, w: u32, h: u32) -> Vec<bool> {
    let mut m = vec![false; (w * h) as usize];
    for c in list.commands.iter().filter(|c| c.glyph) {
        let xs = c.pos.map(|p| p[0]);
        let ys = c.pos.map(|p| p[1]);
        let x0 = xs.iter().copied().fold(f32::INFINITY, f32::min).max(c.clip[0]);
        let x1 = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max).min(c.clip[2]);
        let y0 = ys.iter().copied().fold(f32::INFINITY, f32::min).max(c.clip[1]);
        let y1 = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max).min(c.clip[3]);
        for y in 0..h {
            let cy = y as f32 + 0.5;
            if cy < y0 || cy >= y1 {
                continue;
            }
            for x in 0..w {
                let cx = x as f32 + 0.5;
                if cx >= x0 && cx < x1 {
                    m[(y * w + x) as usize] = true;
                }
            }
        }
    }
    m
}

/// Compares `ours` (RGBA rows of `full_w` width) with the oracle RGB(A) image over the oracle's crop.
pub fn diff(ours: &[u8], full_w: u32, oracle: &image::RgbaImage, mask: &[bool]) -> (Diff, image::RgbaImage) {
    let (w, h) = oracle.dimensions();
    let mut d = Diff::default();
    let mut heat = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let o = oracle.get_pixel(x, y).0;
            let i = ((y * full_w + x) * 4) as usize;
            let delta = (0..3).map(|k| ours[i + k].abs_diff(o[k])).max().unwrap_or(0);
            let text = mask[(y * full_w + x) as usize];
            d.pixels += 1;
            d.max_delta = d.max_delta.max(delta);
            if text {
                d.text_pixels += 1;
                d.text_max_delta = d.text_max_delta.max(delta);
            }
            let g = o[0] / 4 + o[1] / 4 + o[2] / 4;
            if delta > TOLERANCE {
                d.bad += 1;
                if text {
                    d.text_bad += 1;
                }
                d.bbox = Some(match d.bbox {
                    None => [x, y, x, y],
                    Some([a, b, c, e]) => [a.min(x), b.min(y), c.max(x), e.max(y)],
                });
                heat.put_pixel(x, y, image::Rgba([255, 255 - delta, 0, 255]));
            } else {
                heat.put_pixel(x, y, image::Rgba([g, g, g, 255]));
            }
        }
    }
    (d, heat)
}

pub fn load_oracle_png(name: &str) -> image::RgbaImage {
    let p = oracle_dir().join("fixtures/scenes").join(format!("{name}.png"));
    image::open(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())).to_rgba8()
}

/// Our frame cropped to the oracle's size, as an RGBA image.
pub fn crop(ours: &[u8], full_w: u32, w: u32, h: u32) -> image::RgbaImage {
    let mut out = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * full_w + x) * 4) as usize;
            out.put_pixel(x, y, image::Rgba([ours[i], ours[i + 1], ours[i + 2], 255]));
        }
    }
    out
}

pub fn out_dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/sg-scenes");
    std::fs::create_dir_all(&d).expect("mkdir target/sg-scenes");
    d
}

pub fn save(img: &image::RgbaImage, path: &Path) {
    img.save(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
