//! Textures and `NewImageHandle()` / `handle:Load(file, flags…)` / `handle:ImageSize()` (UI-ARCH §2.3, §3.5).
//!
//! A [`Texture`] is CPU-side RGBA8 pixels plus sampling flags, shared by `Rc`; the renderer uploads it to the GPU the
//! first time a draw command references it (keyed by [`Texture::id`]) and drops the GPU copy once the last `Rc` is gone.
//! Filtering is linear and addressing repeats unless `CLAMP` (UI-ARCH §11: "фільтрація зображень лінійна").

use std::cell::{Ref, RefCell};
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// `Load` flags (SG-DLL-FACTS §6: `CLAMP`, `MIPMAP`, `NEAREST`; `ASYNC` is passed by PassiveTree.lua:263).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadFlag {
    Clamp,
    /// Accepted; mipmaps are not generated (the only Lua use, PassiveTree.lua:263, is commented out).
    Mipmap,
    /// Accepted, no effect (measured, see `ImageHandle::load`).
    Nearest,
    Async,
}

impl LoadFlag {
    pub fn from_lua_name(name: &str) -> Option<Self> {
        match name {
            "CLAMP" => Some(Self::Clamp),
            "MIPMAP" => Some(Self::Mipmap),
            "NEAREST" => Some(Self::Nearest),
            "ASYNC" => Some(Self::Async),
            _ => None,
        }
    }
}

/// RGBA8 pixels (straight alpha) + sampling mode. Immutable once created.
pub struct Texture {
    id: u64,
    pub width: u32,
    pub height: u32,
    pub clamp: bool,
    pub nearest: bool,
    /// The window's renderer lets them go after the upload ([`Renderer::release_uploaded_pixels`](crate::render::Renderer::release_uploaded_pixels)); an offscreen one keeps
    /// them, so that any later renderer can upload the texture again.
    pixels: RefCell<Option<Box<[u8]>>>,
}

impl fmt::Debug for Texture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Texture#{}({}x{})", self.id, self.width, self.height)
    }
}

impl Texture {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>, clamp: bool, nearest: bool) -> Self {
        assert_eq!(rgba.len(), width as usize * height as usize * 4, "rgba size");
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            clamp,
            nearest,
            pixels: RefCell::new(Some(rgba.into_boxed_slice())),
        }
    }

    /// The 1x1 opaque white texture used for `DrawImage(nil, …)` (`@white`, SG-DLL-FACTS §2).
    pub fn white() -> Self {
        Self::new(1, 1, vec![255; 4], false, false)
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// `None` once released.
    pub fn pixels(&self) -> Option<Ref<'_, [u8]>> {
        Ref::filter_map(self.pixels.borrow(), |p| p.as_deref()).ok()
    }

    pub(crate) fn release_pixels(&self) {
        *self.pixels.borrow_mut() = None;
    }
}

#[derive(Debug)]
pub enum ImageError {
    UnknownFlag(String),
    Io(PathBuf, std::io::Error),
    Decode(PathBuf, String),
    /// The thread of a prefetch died.
    Lost,
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFlag(s) => write!(f, "unrecognised flag '{s}'"),
            Self::Io(p, e) => write!(f, "{}: {e}", p.display()),
            Self::Decode(p, e) => write!(f, "{}: {e}", p.display()),
            Self::Lost => f.write_str("the decoding thread is gone"),
        }
    }
}

impl std::error::Error for ImageError {}

type Decoded = Result<(u32, u32, Vec<u8>), ImageError>;

fn decode(path: &Path) -> Decoded {
    let bytes = std::fs::read(path).map_err(|e| ImageError::Io(path.to_path_buf(), e))?;
    let img = image::load_from_memory(&bytes).map_err(|e| ImageError::Decode(path.to_path_buf(), e.to_string()))?;
    let rgba = img.to_rgba8();
    Ok((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// The image's size from its header, nothing decoded.
fn read_size(path: &Path) -> Result<(u32, u32), ImageError> {
    image::ImageReader::open(path)
        .map_err(|e| ImageError::Io(path.to_path_buf(), e))?
        .with_guessed_format()
        .map_err(|e| ImageError::Io(path.to_path_buf(), e))?
        .into_dimensions()
        .map_err(|e| ImageError::Decode(path.to_path_buf(), e.to_string()))
}

fn finished(d: Decoded, clamp: bool, nearest: bool) -> State {
    match d {
        Ok((w, h, px)) => State::Loaded(Rc::new(Texture::new(w, h, px, clamp, nearest))),
        Err(_) => State::Failed,
    }
}

enum State {
    Empty,
    /// [`ImageHandle::load_lazy`], not asked for yet. `size`: known from the file's header (a synchronous `Load`), `None` for an `ASYNC` one.
    Lazy {
        path: PathBuf,
        size: Option<(u32, u32)>,
        clamp: bool,
    },
    /// A synchronous lazy load whose pixels a thread is decoding ([`ImageHandle::prefetch`]).
    Prefetching {
        rx: mpsc::Receiver<Decoded>,
        size: (u32, u32),
        clamp: bool,
    },
    Loading {
        rx: mpsc::Receiver<Decoded>,
        clamp: bool,
        nearest: bool,
    },
    Loaded(Rc<Texture>),
    /// A failed load leaves the handle without an image (`ImageSize` 0,0) [H: not measured].
    Failed,
}

/// `NewImageHandle()`. Cloning shares the handle (like a Lua reference).
#[derive(Clone)]
pub struct ImageHandle(Rc<RefCell<State>>);

impl Default for ImageHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ImageHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match &*self.0.borrow() {
            State::Empty => "empty".to_string(),
            State::Lazy { .. } => "lazy".to_string(),
            State::Loading { .. } | State::Prefetching { .. } => "loading".to_string(),
            State::Loaded(t) => format!("{t:?}"),
            State::Failed => "failed".to_string(),
        };
        write!(f, "ImageHandle({s})")
    }
}

impl ImageHandle {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(State::Empty)))
    }

    /// `handle:Load(fileName, flags…)`. Without `ASYNC` the image is decoded now and a decode error is returned; with
    /// `ASYNC` it is decoded on a background thread and `image_size()` stays `(0, 0)` until it arrives (UI-ARCH U7).
    pub fn load(&self, path: impl AsRef<Path>, flags: &[LoadFlag]) -> Result<(), ImageError> {
        let path = path.as_ref().to_path_buf();
        let clamp = flags.contains(&LoadFlag::Clamp);
        // `NEAREST` is accepted but changes nothing in the original (SG-oracle scene `u12-image`: a `NEAREST` image is
        // pixel-identical to the linear one); the Lua UI never passes it.
        let nearest = false;
        if flags.contains(&LoadFlag::Async) {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                // The receiver may be gone (handle reloaded/dropped): nothing to do then.
                let _ = tx.send(decode(&path));
            });
            *self.0.borrow_mut() = State::Loading { rx, clamp, nearest };
            return Ok(());
        }
        match decode(&path) {
            Ok((w, h, px)) => {
                *self.0.borrow_mut() = State::Loaded(Rc::new(Texture::new(w, h, px, clamp, nearest)));
                Ok(())
            }
            Err(e) => {
                *self.0.borrow_mut() = State::Failed;
                Err(e)
            }
        }
    }

    /// `load` that keeps the pixels out of memory until something draws the image: the same image and the same `ImageSize` at every point the
    /// caller can see. Without `ASYNC` the size comes from the file's header now, and the first draw decodes (waiting for [`Self::prefetch`] if there was one).
    /// With `ASYNC` the size stays `(0, 0)` until a background decode has finished, and the first question starts that decode.
    pub fn load_lazy(&self, path: impl AsRef<Path>, flags: &[LoadFlag]) -> Result<(), ImageError> {
        let path = path.as_ref().to_path_buf();
        let clamp = flags.contains(&LoadFlag::Clamp);
        let size = if flags.contains(&LoadFlag::Async) {
            None
        } else {
            match read_size(&path) {
                Ok(s) => Some(s),
                Err(e) => {
                    *self.0.borrow_mut() = State::Failed;
                    return Err(e);
                }
            }
        };
        *self.0.borrow_mut() = State::Lazy { path, size, clamp };
        Ok(())
    }

    /// Starts decoding a synchronous lazy image on a thread: the frame that draws it waits for less.
    pub fn prefetch(&self) {
        let mut st = self.0.borrow_mut();
        let State::Lazy {
            path,
            size: Some(size),
            clamp,
        } = &*st
        else {
            return;
        };
        let (path, size, clamp) = (path.clone(), *size, *clamp);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(decode(&path));
        });
        *st = State::Prefetching { rx, size, clamp };
    }

    /// `load` with Lua flag names (`"CLAMP"`, `"ASYNC"`…).
    pub fn load_lua(&self, path: impl AsRef<Path>, flags: &[&str]) -> Result<(), ImageError> {
        let flags = flags
            .iter()
            .map(|s| LoadFlag::from_lua_name(s).ok_or_else(|| ImageError::UnknownFlag((*s).to_string())))
            .collect::<Result<Vec<_>, _>>()?;
        self.load(path, &flags)
    }

    /// An `ASYNC` lazy image goes to a thread; a synchronous one gets its pixels now.
    fn start_lazy(&self) {
        let mut st = self.0.borrow_mut();
        let next = match &*st {
            State::Lazy { path, size: None, clamp } => {
                let (path, clamp) = (path.clone(), *clamp);
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(decode(&path));
                });
                State::Loading { rx, clamp, nearest: false }
            }
            State::Lazy {
                path,
                size: Some(_),
                clamp,
            } => finished(decode(path), *clamp, false),
            State::Prefetching { rx, clamp, .. } => finished(rx.recv().unwrap_or(Err(ImageError::Lost)), *clamp, false),
            _ => return,
        };
        *st = next;
    }

    /// Picks up a finished background decode.
    fn poll(&self) {
        self.start_lazy();
        let mut st = self.0.borrow_mut();
        let State::Loading { rx, clamp, nearest } = &*st else { return };
        let next = match rx.try_recv() {
            Ok(Ok((w, h, px))) => State::Loaded(Rc::new(Texture::new(w, h, px, *clamp, *nearest))),
            Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => State::Failed,
            Err(mpsc::TryRecvError::Empty) => return,
        };
        *st = next;
    }

    /// Blocks until a pending `ASYNC` load finishes (tests, tools).
    pub fn wait(&self) {
        self.start_lazy();
        let mut st = self.0.borrow_mut();
        if let State::Loading { rx, clamp, nearest } = &*st {
            *st = match rx.recv() {
                Ok(Ok((w, h, px))) => State::Loaded(Rc::new(Texture::new(w, h, px, *clamp, *nearest))),
                _ => State::Failed,
            };
        }
    }

    /// `handle:ImageSize()`: `(0, 0)` until loaded.
    pub fn image_size(&self) -> (u32, u32) {
        if let State::Lazy { size: Some(s), .. } | State::Prefetching { size: s, .. } = &*self.0.borrow() {
            return *s;
        }
        self.texture().map_or((0, 0), |t| (t.width, t.height))
    }

    /// An image is loaded (not merely requested).
    pub fn is_valid(&self) -> bool {
        self.texture().is_some()
    }

    pub fn is_loading(&self) -> bool {
        self.poll();
        matches!(&*self.0.borrow(), State::Loading { .. })
    }

    /// `Load` was called (successfully or still pending): drawing is allowed.
    pub(crate) fn has_load(&self) -> bool {
        !matches!(&*self.0.borrow(), State::Empty)
    }

    pub fn texture(&self) -> Option<Rc<Texture>> {
        self.poll();
        match &*self.0.borrow() {
            State::Loaded(t) => Some(Rc::clone(t)),
            _ => None,
        }
    }
}
