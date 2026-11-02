//! The `winit` main loop: window + surface, callbacks of a [`MainObject`] (`OnInit`, `OnFrame`, `OnKeyDown`, `OnKeyUp`,
//! `OnChar`, `CanExit`, `OnExit`), frame cadence.
//!
//! Cadence (UI-ARCH §11): the original draws a frame about every 30 ms both idle and under input
//! (`glfwWaitEventsTimeout`), and none while minimised. Here: `ControlFlow::WaitUntil(last + 30 ms)`, input callbacks run
//! as events arrive (between frames), `OnFrame` once per tick; a frame whose draw list equals the previous one is
//! not re-presented (like `r_elideFrames`).
//!
//! Input is minimal in U1.2 (U1.4 does key names, double-click, layouts, `IsKeyDown`): mouse buttons, the wheel
//! (`OnKeyDown` + `OnKeyUp` per message, §11), a few named keys, printable keys as lower-case characters, `OnChar`
//! with non-ASCII as `"?"`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::draw::{DrawList, Frame, Gfx};
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
    /// Host-side request to leave the loop (`Exit()`), polled after each callback.
    fn wants_exit(&self) -> bool {
        false
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

fn key_name(key: &Key) -> Option<String> {
    Some(match key {
        Key::Named(n) => match n {
            NamedKey::Backspace => "BACK",
            NamedKey::Tab => "TAB",
            NamedKey::Enter => "RETURN",
            NamedKey::Escape => "ESCAPE",
            NamedKey::Shift => "SHIFT",
            NamedKey::Control => "CTRL",
            NamedKey::Alt => "ALT",
            NamedKey::Pause => "PAUSE",
            NamedKey::PageUp => "PAGEUP",
            NamedKey::PageDown => "PAGEDOWN",
            NamedKey::End => "END",
            NamedKey::Home => "HOME",
            NamedKey::PrintScreen => "PRINTSCREEN",
            NamedKey::Insert => "INSERT",
            NamedKey::Delete => "DELETE",
            NamedKey::ArrowUp => "UP",
            NamedKey::ArrowDown => "DOWN",
            NamedKey::ArrowLeft => "LEFT",
            NamedKey::ArrowRight => "RIGHT",
            NamedKey::Space => " ",
            NamedKey::F1 => "F1",
            NamedKey::F2 => "F2",
            NamedKey::F3 => "F3",
            NamedKey::F4 => "F4",
            NamedKey::F5 => "F5",
            NamedKey::F6 => "F6",
            NamedKey::F7 => "F7",
            NamedKey::F8 => "F8",
            NamedKey::F9 => "F9",
            NamedKey::F10 => "F10",
            NamedKey::F11 => "F11",
            NamedKey::F12 => "F12",
            _ => return None,
        }
        .to_string(),
        Key::Character(s) => s.to_lowercase(),
        _ => return None,
    })
}

fn button_name(b: MouseButton) -> Option<&'static str> {
    Some(match b {
        MouseButton::Left => "LEFTBUTTON",
        MouseButton::Middle => "MIDDLEBUTTON",
        MouseButton::Right => "RIGHTBUTTON",
        MouseButton::Back => "MOUSE4",
        MouseButton::Forward => "MOUSE5",
        MouseButton::Other(_) => return None,
    })
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
        let mut gfx = Gfx::new(&self.cfg.fonts_dir, (size.width, size.height), window.scale_factor()).map_err(RunError::Font)?;
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
}

impl<M: MainObject> ApplicationHandler for App<M> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.live.is_none()
            && let Err(e) = self.init(el)
        {
            self.error = Some(e);
            el.exit();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(live) = self.live.as_mut() else { return };
        let gfx = &mut live.gfx;
        match event {
            WindowEvent::CloseRequested => {
                if self.main.can_exit(gfx) {
                    self.main.on_exit(gfx);
                    el.exit();
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
                gfx.set_screen(s, scale_factor);
            }
            WindowEvent::RedrawRequested => self.frame(),
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(name) = button_name(button) {
                    match state {
                        ElementState::Pressed => self.main.on_key_down(gfx, name, false),
                        ElementState::Released => self.main.on_key_up(gfx, name),
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(p) => p.y,
                };
                if dy != 0.0 {
                    let name = if dy > 0.0 { "WHEELUP" } else { "WHEELDOWN" };
                    self.main.on_key_down(gfx, name, false);
                    self.main.on_key_up(gfx, name);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let name = key_name(&event.logical_key);
                match event.state {
                    ElementState::Pressed => {
                        if let Some(n) = &name {
                            self.main.on_key_down(gfx, n, false);
                        }
                        if let Some(t) = &event.text {
                            for c in t.chars() {
                                let s = if c.is_ascii() { c.to_string() } else { "?".to_string() };
                                self.main.on_char(gfx, &s);
                            }
                        }
                    }
                    ElementState::Released => {
                        if let Some(n) = &name {
                            self.main.on_key_up(gfx, n);
                        }
                    }
                }
            }
            _ => {}
        }
        if self.main.wants_exit() {
            el.exit();
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_frame {
            if let Some(live) = &self.live {
                live.window.request_redraw();
            }
            self.next_frame = now + FRAME_PERIOD;
        }
        if self.main.wants_exit() {
            el.exit();
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

/// Opens the window and runs the loop until the window closes (and `can_exit` agrees) or `wants_exit()`.
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
