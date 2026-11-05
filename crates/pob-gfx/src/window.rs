//! The `winit` main loop: window + surface, callbacks of a [`MainObject`] (`OnInit`, `OnFrame`, `OnKeyDown`, `OnKeyUp`,
//! `OnChar`, `CanExit`, `OnExit`), frame cadence, window requests (`SetWindowTitle`, `Exit`) and `TakeScreenshot`.
//!
//! Cadence: the original has no frame timer of its own, `OnFrame` follows the display (120 fps on a 119 Hz monitor, measured by
//! `tools/sg-oracle/lua/frame-rate-probe.lua`, with or without input), and none while minimised. Here: a frame that changed the picture is
//! followed by the next one at once, and presenting paces them to the display. A frame whose draw list equals the previous one is not
//! re-presented (like `r_elideFrames`), so nothing paces it: the next comes after [`IDLE_FRAME_PERIOD`], or at the next input.
//! Input callbacks run as events arrive (between frames). [`MIN_FRAME_PERIOD`] holds when presenting does not wait (a hidden window).
//!
//! Input: `winit` events become platform-neutral [`RawEvent`]s (physical key positions, typed text as UTF-16 units, wheel
//! messages, the OS cursor) and go through [`dispatch`] — the same path the oracle replay tests use (`tests/input.rs`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Icon, Window, WindowId};

use crate::draw::{DrawList, Frame, Gfx};
use crate::input::{self, Button, Callback, PhysKey, RawEvent};
use crate::platform::SystemPlatform;
use crate::render::{RenderError, Renderer, RendererCore};

/// The period of frames while the picture does not change and no input comes.
pub const IDLE_FRAME_PERIOD: Duration = Duration::from_millis(30);
/// The shortest period of frames, whatever else asks for one (250 fps).
pub const MIN_FRAME_PERIOD: Duration = Duration::from_millis(4);

/// The Lua "main object" (`SetMainObject`, Launch.lua:17): the host calls these between/at frames.
pub trait MainObject {
    fn on_init(&mut self, _gfx: &mut Gfx) {}
    fn on_frame(&mut self, frame: &mut Frame<'_>);
    fn on_key_down(&mut self, _gfx: &mut Gfx, _key: &str, _double_click: bool) {}
    fn on_key_up(&mut self, _gfx: &mut Gfx, _key: &str) {}
    fn on_char(&mut self, _gfx: &mut Gfx, _ch: &str) {}
    /// `false` blocks closing the window (Main.lua:365).
    fn can_exit(&mut self, _gfx: &mut Gfx) -> bool {
        true
    }
    fn on_exit(&mut self, _gfx: &mut Gfx) {}
}

/// Feeds one platform event to `gfx` and fires the resulting callbacks on `main`, in order.
pub fn dispatch<M: MainObject + ?Sized>(main: &mut M, gfx: &mut Gfx, ev: RawEvent) {
    for cb in gfx.input(ev) {
        match cb {
            Callback::KeyDown(k, dbl) => main.on_key_down(gfx, k, dbl),
            Callback::KeyUp(k) => main.on_key_up(gfx, k),
            Callback::Char(c) => main.on_char(gfx, c),
        }
    }
}

pub struct WindowConfig {
    pub title: String,
    /// Initial client size in physical pixels.
    pub size: (u32, u32),
    /// `runtime/SimpleGraphic/Fonts`.
    pub fonts_dir: PathBuf,
}

/// What the loop measured.
#[derive(Clone, Copy, Debug, Default)]
pub struct RunStats {
    /// From `run()` to the first presented frame (cold start of the window).
    pub first_frame: Option<Duration>,
    /// From `run()` to: window created, GPU device ready, fonts parsed (cold-start breakdown).
    pub window_ready: Duration,
    pub gpu_ready: Duration,
    pub fonts_ready: Duration,
    pub frames: u64,
    pub presented: u64,
    pub screenshots: u64,
}

#[derive(Debug)]
pub enum RunError {
    EventLoop(String),
    Render(RenderError),
    Font(crate::text::FontError),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EventLoop(e) => write!(f, "event loop: {e}"),
            Self::Render(e) => write!(f, "{e}"),
            Self::Font(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RunError {}

/// What the GPU thread hands over: the slow part of creating a renderer (adapter, device, pipelines).
struct GpuStart {
    adapter: wgpu::Adapter,
    core: RendererCore,
    ready: Duration,
}

/// The renderer and the surface's configuration, once the GPU thread is done and something is to be drawn.
struct Gpu {
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

struct Live {
    window: Arc<Window>,
    surface: Arc<wgpu::Surface<'static>>,
    /// The GPU thread until the first frame has something to present: the data of the first `OnFrame` loads meanwhile.
    gpu_start: Option<std::thread::JoinHandle<Result<GpuStart, RenderError>>>,
    gpu: Option<Gpu>,
    gfx: Gfx,
    last: Option<DrawList>,
}

impl Live {
    /// Waits for the GPU thread and configures the surface, on this thread (creating the swap chain may talk to the window).
    fn ensure_gpu(&mut self, stats: &mut RunStats) -> Result<(), RunError> {
        let Some(handle) = self.gpu_start.take() else { return Ok(()) };
        let start = handle
            .join()
            .map_err(|_| RunError::Render(RenderError::Device("the GPU thread panicked".into())))?
            .map_err(RunError::Render)?;
        stats.gpu_ready = start.ready;
        let caps = self.surface.get_capabilities(&start.adapter);
        // Non-sRGB target: the original blends in gamma space (GL ES via ANGLE without sRGB framebuffers).
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let mut renderer = Renderer::from_core(start.core, format);
        renderer.release_uploaded_pixels();
        let size = self.window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::default(),
        };
        self.surface.configure(renderer.device(), &config);
        self.gpu = Some(Gpu { config, renderer });
        Ok(())
    }
}

struct App<M: MainObject> {
    main: M,
    cfg: WindowConfig,
    icon: Option<Icon>,
    start: Instant,
    live: Option<Live>,
    next_frame: Instant,
    /// When the last frame was asked for.
    last_frame: Instant,
    stats: RunStats,
    error: Option<RunError>,
    modifiers: ModifiersState,
}

/// Physical key -> the original's key (US positions; GLFW maps keys by scancode, so names ignore the layout).
fn phys_key(code: KeyCode) -> PhysKey {
    use KeyCode as K;
    let ch = |c: u8| PhysKey::Char(c);
    match code {
        K::KeyA => ch(b'a'),
        K::KeyB => ch(b'b'),
        K::KeyC => ch(b'c'),
        K::KeyD => ch(b'd'),
        K::KeyE => ch(b'e'),
        K::KeyF => ch(b'f'),
        K::KeyG => ch(b'g'),
        K::KeyH => ch(b'h'),
        K::KeyI => ch(b'i'),
        K::KeyJ => ch(b'j'),
        K::KeyK => ch(b'k'),
        K::KeyL => ch(b'l'),
        K::KeyM => ch(b'm'),
        K::KeyN => ch(b'n'),
        K::KeyO => ch(b'o'),
        K::KeyP => ch(b'p'),
        K::KeyQ => ch(b'q'),
        K::KeyR => ch(b'r'),
        K::KeyS => ch(b's'),
        K::KeyT => ch(b't'),
        K::KeyU => ch(b'u'),
        K::KeyV => ch(b'v'),
        K::KeyW => ch(b'w'),
        K::KeyX => ch(b'x'),
        K::KeyY => ch(b'y'),
        K::KeyZ => ch(b'z'),
        K::Digit0 => ch(b'0'),
        K::Digit1 => ch(b'1'),
        K::Digit2 => ch(b'2'),
        K::Digit3 => ch(b'3'),
        K::Digit4 => ch(b'4'),
        K::Digit5 => ch(b'5'),
        K::Digit6 => ch(b'6'),
        K::Digit7 => ch(b'7'),
        K::Digit8 => ch(b'8'),
        K::Digit9 => ch(b'9'),
        K::Minus => ch(b'-'),
        K::Equal => ch(b'='),
        K::BracketLeft => ch(b'['),
        K::BracketRight => ch(b']'),
        K::Backslash => ch(b'\\'),
        K::Semicolon => ch(b';'),
        K::Quote => ch(b'\''),
        K::Backquote => ch(b'`'),
        K::Comma => ch(b','),
        K::Period => ch(b'.'),
        K::Slash => ch(b'/'),
        K::Space => ch(b' '),
        K::Backspace => PhysKey::Backspace,
        K::Tab => PhysKey::Tab,
        K::Enter => PhysKey::Enter,
        K::Escape => PhysKey::Escape,
        K::ShiftLeft | K::ShiftRight => PhysKey::Shift,
        K::ControlLeft | K::ControlRight => PhysKey::Ctrl,
        // The UI asks for `CTRL` only; on a Mac its shortcuts are the ones of Cmd, and Ctrl keeps working too.
        #[cfg(target_os = "macos")]
        K::SuperLeft | K::SuperRight => PhysKey::Ctrl,
        K::AltLeft | K::AltRight => PhysKey::Alt,
        K::Pause => PhysKey::Pause,
        K::PageUp => PhysKey::PageUp,
        K::PageDown => PhysKey::PageDown,
        K::End => PhysKey::End,
        K::Home => PhysKey::Home,
        K::PrintScreen => PhysKey::PrintScreen,
        K::Insert => PhysKey::Insert,
        K::Delete => PhysKey::Delete,
        K::ArrowUp => PhysKey::Up,
        K::ArrowDown => PhysKey::Down,
        K::ArrowLeft => PhysKey::Left,
        K::ArrowRight => PhysKey::Right,
        K::F1 => PhysKey::F(1),
        K::F2 => PhysKey::F(2),
        K::F3 => PhysKey::F(3),
        K::F4 => PhysKey::F(4),
        K::F5 => PhysKey::F(5),
        K::F6 => PhysKey::F(6),
        K::F7 => PhysKey::F(7),
        K::F8 => PhysKey::F(8),
        K::F9 => PhysKey::F(9),
        K::F10 => PhysKey::F(10),
        K::F11 => PhysKey::F(11),
        K::F12 => PhysKey::F(12),
        K::F13 => PhysKey::F(13),
        K::F14 => PhysKey::F(14),
        K::F15 => PhysKey::F(15),
        K::NumLock => PhysKey::NumLock,
        K::ScrollLock => PhysKey::ScrollLock,
        K::Numpad0 => PhysKey::Numpad0,
        K::NumpadAdd => PhysKey::NumpadAdd,
        K::NumpadSubtract => PhysKey::NumpadSubtract,
        K::NumpadEnter => PhysKey::NumpadEnter,
        _ => PhysKey::Other,
    }
}

fn button(b: MouseButton) -> Option<Button> {
    Some(match b {
        MouseButton::Left => Button::Left,
        MouseButton::Middle => Button::Middle,
        MouseButton::Right => Button::Right,
        MouseButton::Back => Button::Mouse4,
        MouseButton::Forward => Button::Mouse5,
        MouseButton::Other(_) => return None,
    })
}

/// A key held with Ctrl types nothing: `KeyEvent::text` ignores Ctrl and would give the `v` of Ctrl+V after the paste, GLFW gives no character there. Ctrl+Alt is AltGr on Windows and does type.
fn types_text(modifiers: ModifiersState) -> bool {
    // Cmd is `CTRL` on a Mac (`phys_key`), and Cmd+Alt is no AltGr there.
    #[cfg(target_os = "macos")]
    if modifiers.super_key() {
        return false;
    }
    !modifiers.control_key() || modifiers.alt_key()
}

/// `winit` window event -> platform events, in delivery order (a key press with text = the key, then its characters).
fn raw_events(event: &WindowEvent, modifiers: ModifiersState) -> Vec<RawEvent> {
    match event {
        WindowEvent::KeyboardInput { event, .. } => {
            let key = match event.physical_key {
                PhysicalKey::Code(c) => phys_key(c),
                PhysicalKey::Unidentified(_) => PhysKey::Other,
            };
            let pressed = event.state == ElementState::Pressed;
            let mut v = vec![RawEvent::Key { key, pressed }];
            if pressed
                && types_text(modifiers)
                && let Some(t) = &event.text
            {
                v.extend(t.chars().flat_map(input::char_units).map(RawEvent::Char));
            }
            v
        }
        WindowEvent::MouseInput { state, button: b, .. } => button(*b)
            .map(|button| RawEvent::Button {
                button,
                pressed: *state == ElementState::Pressed,
                pos: None,
            })
            .into_iter()
            .collect(),
        WindowEvent::MouseWheel { delta, .. } => {
            let dy = match delta {
                MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
                MouseScrollDelta::PixelDelta(p) => p.y,
            };
            vec![RawEvent::Wheel(dy)]
        }
        WindowEvent::CursorMoved { position, .. } => vec![RawEvent::Cursor {
            x: position.x,
            y: position.y,
        }],
        WindowEvent::CursorEntered { .. } => vec![RawEvent::CursorInside(true)],
        WindowEvent::CursorLeft { .. } => vec![RawEvent::CursorInside(false)],
        WindowEvent::Focused(false) => vec![RawEvent::FocusLost],
        _ => Vec::new(),
    }
}

impl<M: MainObject> App<M> {
    fn init(&mut self, el: &ActiveEventLoop) -> Result<(), RunError> {
        let attrs = Window::default_attributes()
            .with_title(self.cfg.title.clone())
            .with_window_icon(self.icon.clone())
            .with_inner_size(PhysicalSize::new(self.cfg.size.0, self.cfg.size.1));
        #[cfg(windows)]
        let attrs = winit::platform::windows::WindowAttributesExtWindows::with_taskbar_icon(attrs, self.icon.clone());
        let window = Arc::new(el.create_window(attrs).map_err(|e| RunError::EventLoop(e.to_string()))?);
        self.stats.window_ready = self.start.elapsed();
        crate::trace::mark("window");
        let instance = Renderer::new_instance();
        let surface = Arc::new(
            instance
                .create_surface(Arc::clone(&window))
                .map_err(|e| RunError::Render(RenderError::Device(e.to_string())))?,
        );
        let size = window.inner_size();
        let (thread_surface, start) = (Arc::clone(&surface), self.start);
        let gpu_start = std::thread::Builder::new()
            .name("gpu start".into())
            .spawn(move || {
                let adapter = Renderer::request_adapter(&instance, Some(&thread_surface))?;
                let core = RendererCore::new(&adapter)?;
                crate::trace::mark(&format!("gpu ({})", adapter.get_info().name));
                Ok(GpuStart {
                    adapter,
                    core,
                    ready: start.elapsed(),
                })
            })
            .map_err(|e| RunError::Render(RenderError::Device(e.to_string())))?;
        // `GetTime()` counts from `run()` (the original: from process start).
        let platform = Box::new(SystemPlatform::new(self.start));
        let mut gfx =
            Gfx::with_platform(&self.cfg.fonts_dir, (size.width, size.height), window.scale_factor(), platform).map_err(RunError::Font)?;
        self.stats.fonts_ready = self.start.elapsed();
        crate::trace::mark("fonts");
        self.main.on_init(&mut gfx);
        crate::trace::mark("on_init");
        self.live = Some(Live {
            window,
            surface,
            gpu_start: Some(gpu_start),
            gpu: None,
            gfx,
            last: None,
        });
        Ok(())
    }

    fn frame(&mut self) {
        let Some(live) = self.live.as_mut() else { return };
        let size = live.window.inner_size();
        if live.window.is_minimized() == Some(true) || size.width == 0 || size.height == 0 {
            return;
        }
        let mut frame = live.gfx.begin_frame();
        self.main.on_frame(&mut frame);
        let list = frame.finish();
        self.stats.frames += 1;
        if self.stats.frames == 1 {
            crate::trace::mark("first OnFrame");
        }
        if let Err(e) = live.ensure_gpu(&mut self.stats) {
            self.error = Some(e);
            return;
        }
        let gpu = live.gpu.as_mut().expect("ensure_gpu");
        if live.gfx.frame_wants_screenshot() {
            // Written even when the frame is elided (identical to the previous one).
            match gpu.renderer.render_to_rgba(&list) {
                Ok(rgba) => match crate::screenshot::save(&rgba, list.size.0, list.size.1, live.gfx.screenshot_dir()) {
                    Ok(p) => {
                        self.stats.screenshots += 1;
                        live.gfx.con_printf(&format!("Wrote screenshot to {}", p.display()));
                    }
                    Err(e) => live.gfx.con_printf(&format!("Screenshot failed: {e}")),
                },
                Err(e) => live.gfx.con_printf(&format!("Screenshot failed: {e}")),
            }
        }
        if live.last.as_ref() == Some(&list) {
            return;
        }
        let tex = match live.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => {
                live.surface.configure(gpu.renderer.device(), &gpu.config);
                return;
            }
        };
        let view = tex.texture.create_view(&wgpu::TextureViewDescriptor::default());
        gpu.renderer.render(&list, &view);
        live.window.pre_present_notify();
        gpu.renderer.present(tex);
        self.stats.presented += 1;
        if self.stats.first_frame.is_none() {
            self.stats.first_frame = Some(self.start.elapsed());
            crate::trace::mark("first present");
        }
        live.last = Some(list);
        self.next_frame = Instant::now();
    }

    /// Applies `SetWindowTitle` / `Exit` made by the last callback.
    fn apply_requests(&mut self, el: &ActiveEventLoop) {
        let Some(live) = self.live.as_mut() else { return };
        let req = live.gfx.take_requests();
        if let Some(t) = req.title {
            live.window.set_title(&t);
        }
        if req.foreground {
            live.window.focus_window();
        }
        if req.exit.is_some() {
            self.main.on_exit(&mut live.gfx);
            el.exit();
        }
    }
}

impl<M: MainObject> ApplicationHandler for App<M> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.live.is_none() {
            if let Err(e) = self.init(el) {
                self.error = Some(e);
                el.exit();
                return;
            }
            self.apply_requests(el);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(live) = self.live.as_mut() else { return };
        let gfx = &mut live.gfx;
        match &event {
            WindowEvent::CloseRequested => {
                if self.main.can_exit(gfx) {
                    self.main.on_exit(gfx);
                    el.exit();
                    return;
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = live.gpu.as_mut() {
                    gpu.config.width = size.width;
                    gpu.config.height = size.height;
                    if size.width > 0 && size.height > 0 {
                        live.surface.configure(gpu.renderer.device(), &gpu.config);
                    }
                }
                let scale = live.window.scale_factor();
                gfx.set_screen((size.width, size.height), scale);
                live.last = None;
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let s = gfx.get_screen_size();
                gfx.set_screen(s, *scale_factor);
                live.last = None;
            }
            WindowEvent::RedrawRequested => {
                self.frame();
                if self.error.is_some() {
                    el.exit();
                    return;
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            _ => {
                for ev in raw_events(&event, self.modifiers) {
                    self.next_frame = Instant::now();
                    dispatch(&mut self.main, gfx, ev);
                    if gfx.exit_requested() {
                        break;
                    }
                }
            }
        }
        self.apply_requests(el);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let earliest = self.last_frame + MIN_FRAME_PERIOD;
        if now >= self.next_frame && now >= earliest {
            if let Some(live) = &self.live {
                live.window.request_redraw();
            }
            self.last_frame = now;
            self.next_frame = now + IDLE_FRAME_PERIOD;
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.next_frame.max(self.last_frame + MIN_FRAME_PERIOD)));
    }
}

/// Opens the window and runs the loop until the window closes (and `can_exit` agrees) or `Exit()` is called.
pub fn run<M: MainObject>(main: M, cfg: WindowConfig) -> Result<(M, RunStats), RunError> {
    run_with_icon(main, cfg, None)
}

/// A PNG as a window icon; an icon that does not decode leaves the default one.
fn decode_icon(png: &[u8]) -> Option<Icon> {
    let rgba = ::image::load_from_memory_with_format(png, ::image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (w, h) = rgba.dimensions();
    Icon::from_rgba(rgba.into_raw(), w, h).ok()
}

/// [`run`] with the window's icon, a PNG.
pub fn run_with_icon<M: MainObject>(main: M, cfg: WindowConfig, icon_png: Option<&[u8]>) -> Result<(M, RunStats), RunError> {
    let start = Instant::now();
    crate::trace::mark("run");
    let el = EventLoop::new().map_err(|e| RunError::EventLoop(e.to_string()))?;
    let mut app = App {
        main,
        cfg,
        icon: icon_png.and_then(decode_icon),
        start,
        live: None,
        next_frame: start,
        last_frame: start,
        stats: RunStats::default(),
        error: None,
        modifiers: ModifiersState::empty(),
    };
    el.run_app(&mut app).map_err(|e| RunError::EventLoop(e.to_string()))?;
    if let Some(e) = app.error {
        return Err(e);
    }
    Ok((app.main, app.stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_types_nothing_but_altgr_does() {
        assert!(types_text(ModifiersState::empty()));
        assert!(types_text(ModifiersState::SHIFT));
        assert!(!types_text(ModifiersState::CONTROL));
        assert!(!types_text(ModifiersState::CONTROL | ModifiersState::SHIFT));
        assert!(types_text(ModifiersState::CONTROL | ModifiersState::ALT));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cmd_is_ctrl_on_a_mac() {
        assert!(matches!(phys_key(KeyCode::SuperLeft), PhysKey::Ctrl));
        assert!(matches!(phys_key(KeyCode::SuperRight), PhysKey::Ctrl));
        assert!(matches!(phys_key(KeyCode::ControlLeft), PhysKey::Ctrl));
        assert!(!types_text(ModifiersState::SUPER));
        assert!(!types_text(ModifiersState::SUPER | ModifiersState::ALT));
        assert!(types_text(ModifiersState::ALT));
    }
}
