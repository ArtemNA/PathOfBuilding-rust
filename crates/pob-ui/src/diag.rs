//! Ctrl+Shift+F12: the state of the input and of the tree written to a file, for a fault that comes and goes (the port's own, not upstream's).
//! Nothing personal goes in: no build, no path of the user beyond the file's own place, no token.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::time::Instant;

use pob_gfx::Gfx;

use crate::control::{ControlRef, same};
use crate::host::ControlHost;
use crate::launch::launch;
use crate::modules::main::main;

const FRAMES: usize = 240;
const EVENTS: usize = 80;

/// What the last frames and key events were, kept by `Launch`.
#[derive(Default)]
pub struct DiagLog {
    /// When each frame began and how long its `OnFrame` took (ms).
    frames: VecDeque<(Instant, f64)>,
    events: VecDeque<(Instant, String)>,
}

impl DiagLog {
    pub fn frame(&mut self, start: Instant, ms: f64) {
        if self.frames.len() == FRAMES {
            self.frames.pop_front();
        }
        self.frames.push_back((start, ms));
    }

    pub fn event(&mut self, what: String) {
        if self.events.len() == EVENTS {
            self.events.pop_front();
        }
        self.events.push_back((Instant::now(), what));
    }
}

fn name_of(host: &ControlHost, c: Option<ControlRef>) -> String {
    let Some(c) = c else { return "none".into() };
    host.controls_named()
        .into_iter()
        .find(|(_, x)| same(&**x, &*c))
        .map_or_else(|| "(unnamed)".into(), |(n, _)| n)
}

/// The report's text.
pub fn report(g: &Gfx) -> String {
    let l = launch();
    let now = Instant::now();
    let mut s = String::new();
    let _ = writeln!(s, "pob-rust diagnostics, version {}", l.cfg.app_version.as_deref().unwrap_or("?"));
    let (w, h) = crate::common::screen_size();
    let (cx, cy) = g.get_cursor_pos();
    let _ = writeln!(s, "screen {w}x{h}, scale {}, cursor {cx},{cy}", g.get_screen_scale());
    let held: Vec<&str> = ["CTRL", "SHIFT", "ALT", "LEFTBUTTON", "RIGHTBUTTON", "MIDDLEBUTTON"]
        .into_iter()
        .filter(|k| crate::sg::is_key_down(g, k))
        .collect();
    let _ = writeln!(s, "held: {held:?}");
    let _ = writeln!(s, "error prompt: {}", l.prompt_msg.borrow().is_some());
    let m = main();
    let titles: Vec<String> = m.popups.borrow().iter().map(|p| p.title.get(g)).collect();
    let _ = writeln!(s, "popups: {titles:?}");
    let _ = writeln!(s, "mode {:?}", m.mode.borrow());
    let _ = writeln!(
        s,
        "main focus {}, under the cursor {}",
        name_of(&m.host, m.host.sel()),
        name_of(&m.host, m.host.get_mouse_over_control(g))
    );
    let b = m.build_mode.clone();
    let _ = writeln!(s, "view {}", b.view_mode.borrow());
    let _ = writeln!(
        s,
        "build focus {}, under the cursor {}",
        name_of(&b.host, b.host.sel()),
        name_of(&b.host, b.host.get_mouse_over_control(g))
    );
    let _ = writeln!(s, "build flag {}", b.build_flag.get());
    if let Some(t) = b.tree_tab.borrow().as_ref() {
        let _ = writeln!(
            s,
            "tree tab focus {}, under the cursor {}",
            name_of(&t.host, t.host.sel()),
            name_of(&t.host, t.host.get_mouse_over_control(g))
        );
        let _ = writeln!(
            s,
            "power: job running {}, result {}, build flag {}, report shown {}, jump pending {:?}",
            t.power_job.borrow().is_some(),
            t.power.borrow().is_some(),
            t.power_build_flag.get(),
            t.power_report_list.base.shown.raw().unwrap_or(true),
            t.jump_to.get()
        );
    } else {
        let _ = writeln!(s, "tree tab not made yet");
    }
    if let Ok(tv) = b.tree_view.try_borrow() {
        if let Some(v) = tv.as_ref() {
            let vp = v.last_view_port;
            let over = cx >= vp.x && cx < vp.x + vp.width && cy >= vp.y && cy < vp.y + vp.height;
            let _ = writeln!(
                s,
                "tree view: port {vp:?} (cursor over it {over}), zoom {} ({}, {}), hover {:?}, drag {:?}, dragging {}, trace {}, heat map {}, cursor offset {:?}, compare {}, last drawn {} ms ago",
                v.zoom_level,
                v.zoom_x,
                v.zoom_y,
                v.hover_node,
                v.drag,
                v.dragging,
                v.trace_mode,
                v.show_heat_map,
                v.cursor_offset,
                v.compare_spec.is_some(),
                g.get_time().saturating_sub(v.last_draw_time)
            );
        }
    } else {
        let _ = writeln!(s, "tree view: borrowed");
    }
    let log = l.diag.borrow();
    let _ = writeln!(s, "\nlast frames (ms before now: OnFrame ms):");
    for (t, ms) in log.frames.iter().rev().take(60) {
        let _ = writeln!(s, "  -{:.0}: {ms:.1}", now.duration_since(*t).as_secs_f64() * 1e3);
    }
    if let Some((first, _)) = log.frames.front() {
        let n = log.frames.len();
        let span = now.duration_since(*first).as_secs_f64();
        let mut ms: Vec<f64> = log.frames.iter().map(|f| f.1).collect();
        ms.sort_by(f64::total_cmp);
        let _ = writeln!(
            s,
            "{n} frames in {span:.1} s; OnFrame median {:.1} ms, max {:.1} ms",
            ms[n / 2],
            ms[n - 1]
        );
    }
    let _ = writeln!(s, "\nlast key events (ms before now):");
    for (t, e) in log.events.iter().rev() {
        let _ = writeln!(s, "  -{:.0}: {e}", now.duration_since(*t).as_secs_f64() * 1e3);
    }
    s
}

/// Writes the report under the user folder (`Diagnostics/`) and says where in a toast. Returns the file.
pub fn dump(g: &Gfx) -> Option<std::path::PathBuf> {
    let text = report(g);
    let l = launch();
    let dir = std::path::PathBuf::from(format!("{}/{}/Diagnostics", l.cfg.user_path.as_deref()?, l.cfg.user_folder));
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let path = dir.join(format!("diag-{secs}.txt"));
    let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, text));
    match written {
        Ok(()) => {
            main().toasts.add(&format!("Diagnostics written\nDiagnostics/diag-{secs}.txt"));
            Some(path)
        }
        Err(e) => {
            main().toasts.add(&format!("Diagnostics failed\n{e}"));
            None
        }
    }
}
