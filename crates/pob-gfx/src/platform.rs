//! What the host takes from the operating system besides the window: the clock (`GetTime`), the clipboard
//! (`Copy` / `Paste`) and the browser (`OpenURL`). [`SystemPlatform`] is the real one; [`TestPlatform`] replays a
//! scripted clock and records clipboard/URL traffic (UI-ARCH §5.1 (в): "детерміновані GetTime/…").

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

pub trait Platform {
    /// `GetTime()`: integer milliseconds since the host started.
    fn now_ms(&self) -> u64;
    /// `Copy(text)`. Failures are silent, like the original's.
    fn copy(&mut self, text: &str);
    /// `Paste()`: `None` when the clipboard holds no text (SkillsTab.lua:709 handles `nil`).
    fn paste(&mut self) -> Option<String>;
    /// `OpenURL(url)`: hand the URL to the system's default handler.
    fn open_url(&mut self, url: &str);
}

/// The real platform: `Instant` clock, `arboard` clipboard, `explorer`/`open`/`xdg-open` for URLs.
pub struct SystemPlatform {
    start: Instant,
    clipboard: Option<arboard::Clipboard>,
}

impl SystemPlatform {
    /// `start` = the moment `GetTime()` counts from (the original counts from process start: `GetTime()` in `OnInit`
    /// was 573..853 ms in the oracle runs).
    pub fn new(start: Instant) -> Self {
        Self { start, clipboard: None }
    }

    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        self.clipboard.as_mut()
    }
}

impl Platform for SystemPlatform {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn copy(&mut self, text: &str) {
        if let Some(c) = self.clipboard() {
            let _ = c.set_text(text);
        }
    }

    fn paste(&mut self) -> Option<String> {
        self.clipboard()?.get_text().ok()
    }

    fn open_url(&mut self, url: &str) {
        // Like the original's `ShellExecute(open)`: the shell picks the handler. No new crates (UI-ARCH §10 п. 8).
        // Not `explorer`: it splits a long URL with several `%20` into separate links (the OAuth `scope`).
        let mut command = if cfg!(windows) {
            let mut c = std::process::Command::new("rundll32");
            c.arg("url.dll,FileProtocolHandler");
            c
        } else if cfg!(target_os = "macos") {
            std::process::Command::new("open")
        } else {
            std::process::Command::new("xdg-open")
        };
        let _ = command.arg(url).spawn();
    }
}

/// Scripted platform for tests: the clock is set by the test; clipboard and opened URLs are kept in memory. Clones
/// share the state, so a test keeps one clone and hands the other to [`crate::Gfx::with_platform`].
#[derive(Clone, Default)]
pub struct TestPlatform(Rc<RefCell<TestState>>);

#[derive(Clone, Debug, Default)]
pub struct TestState {
    pub time_ms: u64,
    pub clipboard: Option<String>,
    pub opened: Vec<String>,
}

impl TestPlatform {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_time(&self, ms: u64) {
        self.0.borrow_mut().time_ms = ms;
    }

    pub fn state(&self) -> TestState {
        self.0.borrow().clone()
    }
}

impl Platform for TestPlatform {
    fn now_ms(&self) -> u64 {
        self.0.borrow().time_ms
    }

    fn copy(&mut self, text: &str) {
        self.0.borrow_mut().clipboard = Some(text.to_string());
    }

    fn paste(&mut self) -> Option<String> {
        self.0.borrow().clipboard.clone()
    }

    fn open_url(&mut self, url: &str) {
        self.0.borrow_mut().opened.push(url.to_string());
    }
}
