//! U1.4 gate: the input the SG oracle received (posted by `tools/sg-oracle/inject*.ps1`) is replayed as platform events
//! through `pob-gfx` (`Gfx::input` + a recording main object), and the callbacks — key/char, `doubleClick`, the
//! `IsKeyDown` snapshot and `GetCursorPos` taken inside each callback — must equal the original's
//! (`fixtures/measure-raw-{window,desktop}.json`, `fixtures/input-probe-{raw,chars,f10}.json`).
//!
//! The scripts below mirror the injectors step by step, as the window system delivered them to GLFW: a posted
//! `WM_KEYDOWN` of a character key is followed by the `WM_CHAR` that `TranslateMessage` made from it; a posted
//! `WM_MOUSEMOVE` does not move the OS cursor (the original's `GetCursorPos` reads the OS cursor, which stayed at the
//! physical 1680, 680), so moves are not events here; GLFW's own artefacts of posted input are applied (a posted Shift
//! press is released at once because the OS key state says "up"; a posted `WM_LBUTTONDBLCLK` is ignored by GLFW).
//!
//! `doubleClick` follows the Windows rule only while the OS cursor is over the window (`src/input/mod.rs`): the
//! window-mode runs had the idle OS cursor outside the client, the desktop run and `input-probe-inside` had it inside.
//!
//! One unexplained difference is asserted, not hidden: in `input-probe-inside.json` the original delivered nothing for
//! step 20 (posted move, button down, move, button up), while the same messages were delivered in the window-mode run and
//! in the shorter probes `input-probe-drag.json` / `input-probe-bisect.json` (cursor inside too).

mod scene;

use pob_gfx::input::{Button, PhysKey, RawEvent};
use pob_gfx::window::{MainObject, dispatch};
use pob_gfx::{Frame, Gfx, TestPlatform};
use serde_json::Value;

/// One step of an injector script: a platform event or the probe's own DPI switch, at a time in ms.
enum Step {
    Ev(RawEvent),
    Dpi(u32),
}

#[derive(Default)]
struct Script {
    t: u64,
    steps: Vec<(u64, Step)>,
    /// `input-probe.lua` steps 2/3 switch the DPI override on the next frame.
    dpi_marks: bool,
}

impl Script {
    fn wait(&mut self, ms: u64) -> &mut Self {
        self.t += ms;
        self
    }
    fn ev(&mut self, e: RawEvent) -> &mut Self {
        self.steps.push((self.t, Step::Ev(e)));
        self
    }
    fn ch(&mut self, cp: u32) -> &mut Self {
        self.ev(RawEvent::Char(cp))
    }
    fn mark(&mut self, n: u32) -> &mut Self {
        self.ch(64 + n);
        if self.dpi_marks && (n == 2 || n == 3) {
            self.steps.push((self.t + 1, Step::Dpi(if n == 2 { 150 } else { 100 })));
        }
        self.wait(150)
    }
    fn down(&mut self, key: PhysKey) -> &mut Self {
        self.ev(RawEvent::Key { key, pressed: true })
    }
    fn up(&mut self, key: PhysKey) -> &mut Self {
        self.ev(RawEvent::Key { key, pressed: false })
    }
    /// Posted key press (+ the `WM_CHAR` it translates to), `hold` ms, release, `after` ms.
    fn key(&mut self, key: PhysKey, ch: Option<u32>, hold: u64, after: u64) -> &mut Self {
        self.down(key);
        if let Some(c) = ch {
            self.ch(c);
        }
        self.wait(hold).up(key).wait(after)
    }
    /// Posted button messages carry their client position in `lParam` (`pos`); GLFW uses it, not the OS cursor.
    fn press_at(&mut self, button: Button, pos: (f64, f64)) -> &mut Self {
        self.ev(RawEvent::Button {
            button,
            pressed: true,
            pos: Some(pos),
        })
    }
    fn release_at(&mut self, button: Button, pos: (f64, f64)) -> &mut Self {
        self.ev(RawEvent::Button {
            button,
            pressed: false,
            pos: Some(pos),
        })
    }
    fn press(&mut self, button: Button) -> &mut Self {
        self.press_at(button, P100)
    }
    fn release(&mut self, button: Button) -> &mut Self {
        self.release_at(button, P100)
    }
    fn click(&mut self, button: Button, hold: u64) -> &mut Self {
        self.click_at(button, hold, P100)
    }
    fn click_at(&mut self, button: Button, hold: u64, pos: (f64, f64)) -> &mut Self {
        self.press_at(button, pos).wait(hold).release_at(button, pos)
    }
    fn wheel(&mut self, d: f64, after: u64) -> &mut Self {
        self.ev(RawEvent::Wheel(d)).wait(after)
    }
}

/// The injectors' click position.
const P100: (f64, f64) = (100.0, 100.0);

fn c(ch: char) -> Option<u32> {
    Some(u32::from(ch))
}

/// `inject.ps1` (U1.0) for `lua/measure.lua`; `inside` = the OS cursor was over the window (desktop-size window).
fn measure_script(inside: bool) -> Script {
    use PhysKey as K;
    let mut s = Script::default();
    s.ev(RawEvent::CursorInside(inside));
    s.wait(2000).mark(1).wait(2000).mark(2).wait(500).mark(3);
    for d in [120.0, -120.0, 240.0, 60.0, 60.0] {
        s.wheel(d, 200);
    }
    s.mark(4).key(K::Char(b'a'), c('a'), 60, 120);
    s.mark(5).down(K::Ctrl).wait(60).key(K::Char(b'z'), c('z'), 60, 120).up(K::Ctrl);
    s.mark(6).key(K::Backspace, Some(0x08), 60, 120);
    s.mark(7).key(K::Enter, Some(0x0D), 60, 120);
    s.mark(8).key(K::Tab, Some(0x09), 60, 120);
    s.mark(9).key(K::Escape, Some(0x1B), 60, 120);
    s.mark(10)
        .key(K::F(1), None, 60, 120)
        .key(K::Delete, None, 60, 120)
        .key(K::Left, None, 60, 120);
    s.mark(11);
    for cp in [0x44F, 0xE9, 0x20AC, 0x1A, 0x08, 0x7F, 0x0D] {
        s.ch(cp).wait(80);
    }
    let l = Button::Left;
    s.mark(12).click(l, 30).wait(80).click(l, 30).wait(700);
    s.mark(13).click(l, 30).wait(300).click(l, 30).wait(700);
    s.mark(14).click(l, 30).wait(650).click(l, 30).wait(700);
    s.mark(15).click(l, 30).wait(80).click(l, 30).wait(80).click(l, 30).wait(700);
    let r = Button::Right;
    s.mark(16)
        .click(r, 30)
        .wait(80)
        .click(r, 30)
        .wait(200)
        .click(Button::Middle, 30)
        .wait(500);
    // down, up, (WM_LBUTTONDBLCLK: ignored by GLFW), up
    s.mark(17).press(l).release(l).wait(80).release(l).wait(700);
    s.mark(18).wait(2000).mark(19).wait(2000).mark(26);
    s
}

/// `inject-probe.ps1` (U1.4) for `lua/input-probe.lua`: tag `raw` (window at 40,40, the idle OS cursor at client
/// 1680, 680 — outside) or `inside` (window at 1080,320, the cursor at client 640, 400).
fn probe_script(inside: bool) -> Script {
    use PhysKey as K;
    let mut s = Script {
        dpi_marks: true,
        ..Script::default()
    };
    let cur = if inside { (640.0, 400.0) } else { (1680.0, 680.0) };
    s.ev(RawEvent::Cursor { x: cur.0, y: cur.1 })
        .ev(RawEvent::CursorInside(inside))
        .wait(500);
    s.mark(1).wait(330).mark(2).wait(1030).mark(3).wait(300);
    let (l, r) = (Button::Left, Button::Right);
    s.mark(4).click(l, 20).wait(110).click(l, 20).wait(700);
    s.mark(5).click(l, 20).wait(10).click(l, 20).wait(700);
    s.mark(6).click(l, 20).wait(110).click_at(l, 20, (110.0, 100.0)).wait(700);
    let p120 = (120.0, 100.0);
    s.mark(7).click_at(r, 20, p120).wait(110).click_at(r, 20, p120).wait(700);
    s.mark(8)
        .click_at(Button::Mouse4, 20, p120)
        .wait(150)
        .click_at(Button::Mouse5, 20, p120)
        .wait(300);
    // A posted Shift press is released by GLFW at once (OS key state "up"); the posted Shift release is then dropped.
    s.mark(9)
        .down(K::Shift)
        .up(K::Shift)
        .wait(60)
        .key(K::Char(b'a'), c('a'), 40, 80)
        .up(K::Shift)
        .wait(150);
    s.mark(10).key(K::Alt, None, 80, 200);
    s.mark(11)
        .key(K::Ctrl, None, 40, 80)
        .down(K::Shift)
        .up(K::Shift)
        .wait(120)
        .key(K::Alt, None, 60, 200);
    s.mark(12);
    for k in [
        K::Pause,
        K::PageUp,
        K::PageDown,
        K::End,
        K::Home,
        K::Insert,
        K::Up,
        K::Down,
        K::Right,
        K::NumLock,
        K::ScrollLock,
        K::PrintScreen,
    ] {
        s.key(k, None, 40, 80);
    }
    s.mark(13);
    for f in 2..=15 {
        s.key(K::F(f), None, 40, 80);
    }
    s.mark(14);
    for b in *b"10-=[];'`\\,./ qm" {
        s.key(K::Char(b), Some(u32::from(b)), 40, 80);
    }
    s.mark(15);
    for (k, ch) in [
        (K::Numpad0, c('0')),
        (K::Other, c('1')),
        (K::Other, c('9')),
        (K::Other, c('*')),
        (K::NumpadAdd, c('+')),
        (K::NumpadSubtract, c('-')),
        (K::Other, c('.')),
        (K::Other, c('/')),
        (K::NumpadEnter, Some(0x0D)),
    ] {
        s.key(k, ch, 40, 80);
    }
    s.mark(16)
        .key(K::Other, None, 40, 80)
        .key(K::Other, None, 40, 80)
        .key(K::Other, None, 40, 80);
    let a = K::Char(b'a');
    s.mark(17)
        .down(a)
        .ch(97)
        .wait(40)
        .down(a)
        .ch(97)
        .wait(40)
        .down(a)
        .ch(97)
        .wait(40)
        .up(a)
        .wait(150);
    s.mark(18).wheel(1.0, 150).wheel(-1.0, 150).wheel(0.0, 150).wait(200);
    s.mark(19);
    for cp in [0xD83D, 0xDE00, 0x7E, 0xA0, 0x80, 0x20] {
        s.ch(cp).wait(60);
    }
    s.wait(150)
        .mark(20)
        .press(l)
        .wait(120)
        .release_at(l, (130.0, 110.0))
        .wait(200)
        .mark(26);
    s
}

/// `inject-chars.ps1` for `lua/input-probe.lua` (tag `chars`): no marks; `~` before every code point.
fn chars_script() -> Script {
    let mut s = Script::default();
    s.ev(RawEvent::Cursor { x: 1680.0, y: 680.0 }).wait(300);
    s.key(PhysKey::Char(b'a'), c('a'), 100, 300);
    let ranges = [(0x80, 0x2FF), (0x370, 0x4FF), (0x2000, 0x20CF), (0x2100, 0x215F), (0xFF00, 0xFF5F)];
    for (a, b) in ranges {
        for cp in a..=b {
            s.ch(0x7E).ch(cp).wait(3);
        }
    }
    s.ch(0x7E);
    s
}

/// `inject-f10.ps1` for `lua/input-probe.lua` (tag `f10`).
fn f10_script() -> Script {
    let mut s = Script {
        dpi_marks: true,
        ..Script::default()
    };
    s.ev(RawEvent::Cursor { x: 1680.0, y: 680.0 }).wait(300);
    for (n, f) in [(1, 10), (2, 10), (3, 9), (4, 10)] {
        s.mark(n).key(PhysKey::F(f), None, 60, 300);
    }
    // A click after the F-keys, then the screenshot mark (`fixtures/input-probe-f10.png`).
    s.mark(5).click(Button::Left, 30).wait(1000);
    s.mark(25).wait(1500).mark(26);
    s
}

/// One observed callback.
#[derive(Clone, Debug, PartialEq)]
struct Obs {
    kind: &'static str,
    key: String,
    dbl: bool,
    keys: Option<Vec<bool>>,
    cursor: Option<(f64, f64)>,
}

/// Records callbacks with the `IsKeyDown` snapshot of `names` and `GetCursorPos`.
struct Recorder {
    names: Vec<String>,
    cursor: bool,
    got: Vec<Obs>,
}

impl Recorder {
    fn obs(&mut self, gfx: &Gfx, kind: &'static str, key: &str, dbl: bool) {
        let keys = self.names.iter().map(|n| gfx.is_key_down(n).expect("valid name")).collect();
        let cursor = self.cursor.then(|| gfx.get_cursor_pos());
        self.got.push(Obs {
            kind,
            key: key.to_string(),
            dbl,
            keys: Some(keys),
            cursor,
        });
    }
}

impl MainObject for Recorder {
    fn on_frame(&mut self, _f: &mut Frame<'_>) {}
    fn on_key_down(&mut self, gfx: &mut Gfx, key: &str, dbl: bool) {
        self.obs(gfx, "down", key, dbl);
    }
    fn on_key_up(&mut self, gfx: &mut Gfx, key: &str) {
        self.obs(gfx, "up", key, false);
    }
    fn on_char(&mut self, gfx: &mut Gfx, ch: &str) {
        self.obs(gfx, "char", ch, false);
    }
}

fn load(file: &str) -> Value {
    let p = scene::oracle_dir().join("fixtures").join(file);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("json")
}

/// The oracle's callbacks. `measure.lua` logs `{ctrl, lb, shift}`; `input-probe.lua` logs `[IsKeyDown(names)…], x, y`.
/// A step mark (`OnChar` "A".."Z", logged as `mark n`) is a character callback too.
fn oracle(v: &Value, chars_mode: bool) -> (Vec<String>, bool, Vec<Obs>) {
    let probe = v.get("names").is_some();
    let names: Vec<String> = if probe {
        v["names"]
            .as_array()
            .expect("names")
            .iter()
            .map(|n| n.as_str().expect("name").to_string())
            .collect()
    } else {
        ["CTRL", "LEFTBUTTON", "SHIFT"].map(String::from).to_vec()
    };
    let state = |s: Option<&Value>| -> (Option<Vec<bool>>, Option<(f64, f64)>) {
        let Some(s) = s else { return (None, None) };
        if probe {
            let a = s.as_array().expect("state");
            let keys = a[0].as_array().expect("keys").iter().map(|k| k.as_i64() == Some(1)).collect();
            (Some(keys), Some((a[1].as_f64().expect("x"), a[2].as_f64().expect("y"))))
        } else {
            let b = |k: &str| s[k].as_bool().expect("bool");
            (Some(vec![b("ctrl"), b("lb"), b("shift")]), None)
        }
    };
    let mut out = Vec::new();
    for e in v["events"].as_array().expect("events") {
        if e[1] == "OnInit" {
            continue;
        }
        let e = e.as_array().expect("event");
        let (kind, key, dbl, st) = match e[1].as_str().expect("kind") {
            "OnKeyDown" => (
                "down",
                e[2].as_str().expect("key").to_string(),
                e[3].as_bool().expect("dbl"),
                e.get(4),
            ),
            "OnKeyUp" => ("up", e[2].as_str().expect("key").to_string(), false, e.get(3)),
            "OnChar" => ("char", e[2].as_str().expect("char").to_string(), false, e.get(4)),
            "mark" if !chars_mode => {
                let n = e[2].as_u64().expect("mark") as u8;
                ("char", char::from(64 + n).to_string(), false, e.get(3))
            }
            _ => continue,
        };
        let (keys, cursor) = state(st);
        out.push(Obs {
            kind,
            key,
            dbl,
            keys,
            cursor,
        });
    }
    (names, probe, out)
}

/// Replays `script` and compares every callback with the oracle file; `missing` = callbacks of `pob-gfx` (by index)
/// that the original did not fire (asserted, see the module docs).
fn replay(file: &str, script: &Script, chars_mode: bool, missing: &[usize]) {
    let v = load(file);
    let (names, cursor, want) = oracle(&v, chars_mode);
    let tp = TestPlatform::new();
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), (1280, 800), 1.0, Box::new(tp.clone())).expect("fonts");
    let mut rec = Recorder {
        names,
        cursor,
        got: Vec::new(),
    };
    for (t, step) in &script.steps {
        tp.set_time(*t);
        match step {
            Step::Ev(e) => dispatch(&mut rec, &mut gfx, *e),
            Step::Dpi(p) => gfx.set_dpi_scale_override_percent(*p),
        }
    }
    let mut got = rec.got;
    if !cursor {
        for o in &mut got {
            o.cursor = None;
        }
    }
    let dropped: Vec<Obs> = missing.iter().rev().map(|&i| got.remove(i)).collect();
    assert_eq!(got.len(), want.len(), "{file}: callback count");
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        // `measure.lua` logs no state with its step marks.
        let w = Obs {
            keys: w.keys.clone().or_else(|| g.keys.clone()),
            ..w.clone()
        };
        assert_eq!(*g, w, "{file} #{i}");
    }
    let dbl = got.iter().filter(|o| o.dbl).count();
    eprintln!(
        "{file}: {} callbacks equal ({dbl} with doubleClick), not fired by the original: {dropped:?}",
        got.len()
    );
}

#[test]
fn measure_window_replays() {
    replay("measure-raw-window.json", &measure_script(false), false, &[]);
}

#[test]
fn measure_desktop_replays() {
    replay("measure-raw-desktop.json", &measure_script(true), false, &[]);
}

#[test]
fn input_probe_replays() {
    replay("input-probe-raw.json", &probe_script(false), false, &[]);
}

#[test]
fn input_probe_inside_replays() {
    // Step 20 (the two callbacks before the final mark, of 188): see the module docs.
    let n = 188;
    replay("input-probe-inside.json", &probe_script(true), false, &[n - 3, n - 2]);
}

#[test]
fn input_probe_chars_replays() {
    replay("input-probe-chars.json", &chars_script(), true, &[]);
}

#[test]
fn input_probe_f10_replays() {
    replay("input-probe-f10.json", &f10_script(), false, &[]);
}

/// `IsKeyDown` argument errors and names accepted by the original (`input-probe.lua` `OnInit` log).
#[test]
fn is_key_down_names_match_original() {
    let v = load("input-probe-raw.json");
    let init = v["events"]
        .as_array()
        .expect("events")
        .iter()
        .find(|e| e[1] == "OnInit")
        .expect("OnInit");
    let gfx = Gfx::with_platform(&scene::fonts_dir(), (1280, 800), 1.0, Box::new(TestPlatform::new())).expect("fonts");
    for case in init[3].as_array().expect("cases") {
        let name = case[0].as_str().expect("name");
        let ok = case[1].as_bool().expect("ok");
        match gfx.is_key_down(name) {
            Ok(down) => {
                assert!(ok, "{name:?} accepted here, error in the original");
                assert!(!down);
            }
            Err(e) => {
                assert!(!ok, "{name:?} rejected here: {e}");
                assert_eq!(e.to_string(), case[2].as_str().expect("msg"), "{name:?}");
            }
        }
    }
}
