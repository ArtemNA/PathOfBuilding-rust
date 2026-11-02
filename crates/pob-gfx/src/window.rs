//! The `winit` main loop: window + surface, callbacks of a [`MainObject`] (`OnInit`, `OnFrame`, `OnKeyDown`, `OnKeyUp`,
//! `OnChar`, `CanExit`, `OnExit`), frame cadence, window requests (`SetWindowTitle`, `Exit`) and `TakeScreenshot`.
//!
//! Cadence (UI-ARCH §11): the original draws a frame about every 30 ms both idle and under input
//! (`glfwWaitEventsTimeout`), and none while minimised. Here: `ControlFlow::WaitUntil(last + 30 ms)`, input callbacks run
//! as events arrive (between frames), `OnFrame` once per tick; a frame whose draw list equals the previous one is
//! not re-presented (like `r_elideFrames`).
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
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::draw::{DrawList, Frame, Gfx};
use crate::input::{self, Button, Callback, PhysKey, RawEvent};
use crate::platform::SystemPlatform;
use crate::render::{RenderError, Renderer};

/// Frame period of the original (§11: ~30 ms, ~33 fps).
pub const FRAME_PERIOD: Duration = Duration::from_millis(30);

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

struct Live {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    gfx: Gfx,
    last: Option<DrawList>,
}

struct App<M: MainObject> {
    main: M,
    cfg: WindowConfig,
    start: Instant,
    live: Option<Live>,
    next_frame: Instant,
    stats: RunStats,
    error: Option<RunError>,
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

/// `winit` window event -> platform events, in delivery order (a key press with text = the key, then its characters).
fn raw_events(event: &WindowEvent) -> Vec<RawEvent> {
    match event {
        WindowEvent::KeyboardInput { event, .. } => {
            let key = match event.physical_key {
                PhysicalKey::Code(c) => phys_key(c),
                PhysicalKey::Unidentified(_) => PhysKey::Other,
            };
            let pressed = event.state == ElementState::Pressed;
            let mut v = vec![RawEvent::Key { key, pressed }];
            if pressed && let Some(t) = &event.text {
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
            .with_inner_size(PhysicalSize::new(self.cfg.size.0, self.cfg.size.1));
        let window = Arc::new(el.create_window(attrs).map_err(|e| RunError::EventLoop(e.to_string()))?);
        self.stats.window_ready = self.start.elapsed();
        let instance = Renderer::new_instance();
        let surface = instance
            .create_surface(Arc::clone(&window))
            .map_err(|e| RunError::Render(RenderError::Device(e.to_string())))?;
        let adapter = Renderer::request_adapter(&instance, Some(&surface)).map_err(RunError::Render)?;
        let caps = surface.get_capabilities(&adapter);
        // Non-sRGB target: the original blends in gamma space (GL ES via ANGLE without sRGB framebuffers).
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let renderer = Renderer::with_adapter(&adapter, format).map_err(RunError::Render)?;
        self.stats.gpu_ready = self.start.elapsed();
        let size = window.inner_size();
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
        surface.configure(renderer.device(), &config);
        // `GetTime()` counts from `run()` (the original: from process start).
        let platform = Box::new(SystemPlatform::new(self.start));
        let mut gfx =
            Gfx::with_platform(&self.cfg.fonts_dir, (size.width, size.height), window.scale_factor(), platform).map_err(RunError::Font)?;
        self.stats.fonts_ready = self.start.elapsed();
        self.main.on_init(&mut gfx);
        self.live = Some(Live {
            window,
            surface,
            config,
            renderer,
            gfx,
            last: None,
        });
        Ok(())
    }

    fn frame(&mut self) {
        let Some(live) = self.live.as_mut() else { return };
        if live.window.is_minimized() == Some(true) || live.config.width == 0 || live.config.height == 0 {
            return;
        }
        let mut frame = live.gfx.begin_frame();
        self.main.on_frame(&mut frame);
        let list = frame.finish();
        self.stats.frames += 1;
        if live.gfx.frame_wants_screenshot() {
            // Written even when the frame is elided (identical to the previous one).
            match live.renderer.render_to_rgba(&list) {
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
                live.surface.configure(live.renderer.device(), &live.config);
                return;
            }
        };
        let view = tex.texture.create_view(&wgpu::TextureViewDescriptor::default());
        live.renderer.render(&list, &view);
        live.window.pre_present_notify();
        live.renderer.present(tex);
        self.stats.presented += 1;
        if self.stats.first_frame.is_none() {
            self.stats.first_frame = Some(self.start.elapsed());
        }
        live.last = Some(list);
    }

    /// Applies `SetWindowTitle` / `Exit` made by the last callback.
    fn apply_requests(&mut self, el: &ActiveEventLoop) {
        let Some(live) = self.live.as_mut() else { return };
        let req = live.gfx.take_requests();
        if let Some(t) = req.title {
            live.window.set_title(&t);
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
                live.config.width = size.width;
                live.config.height = size.height;
                if size.width > 0 && size.height > 0 {
                    live.surface.configure(live.renderer.device(), &live.config);
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
            WindowEvent::RedrawRequested => self.frame(),
            _ => {
                for ev in raw_events(&event) {
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
        if now >= self.next_frame {
            if let Some(live) = &self.live {
                live.window.request_redraw();
            }
            self.next_frame = now + FRAME_PERIOD;
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

/// Opens the window and runs the loop until the window closes (and `can_exit` agrees) or `Exit()` is called.
pub fn run<M: MainObject>(main: M, cfg: WindowConfig) -> Result<(M, RunStats), RunError> {
    let start = Instant::now();
    let el = EventLoop::new().map_err(|e| RunError::EventLoop(e.to_string()))?;
    let mut app = App {
        main,
        cfg,
        start,
        live: None,
        next_frame: start,
        stats: RunStats::default(),
        error: None,
    };
    el.run_app(&mut app).map_err(|e| RunError::EventLoop(e.to_string()))?;
    if let Some(e) = app.error {
        return Err(e);
    }
    Ok((app.main, app.stats))
}
