//! Rust twins of the oracle galleries (`tools/sg-oracle/lua/controls-gallery.lua`) and the scenario runner of
//! `controls-runner.lua`: same controls, same scripted cursor/time/keys, same frame (`ProcessControlsInput` then
//! `DrawControls`), same dumps.

#![allow(dead_code)]

#[path = "../../../pob-gfx/tests/scene/mod.rs"]
pub mod scene;

use pob_gfx::input::PhysKey;
use pob_gfx::{Button, DrawList, Frame, Gfx, RawEvent, TestPlatform};
use pob_ui::controls::*;
use pob_ui::{ControlHost, ControlRef, InputEvent, InputEvents, Rect, anchor, rect};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

pub type CustomFrame = Box<dyn Fn(&mut Frame<'_>, &mut InputEvents, &Rect)>;

pub struct Env {
    pub host: Rc<ControlHost>,
    pub dump: Box<dyn Fn(&mut Gfx) -> Value>,
    /// Replaces `host:ProcessControlsInput` + `host:DrawControls` (the popup gallery's host table).
    pub custom: Option<CustomFrame>,
}

fn sel_name(host: &ControlHost, named: &[(&str, &ControlRef)]) -> Value {
    let Some(sel) = host.sel() else { return json!("nil") };
    for (n, c) in named {
        if pob_ui::control::same(&*sel, &***c) {
            return json!(n);
        }
    }
    json!("other")
}

/// `G.basic` (controls-gallery.lua).
pub fn basic(g: &Gfx) -> Env {
    let host = Rc::new(ControlHost::new());
    let clicks = Rc::new(Cell::new(0));
    let checked = Rc::new(Cell::new(0));
    let last_check = Rc::new(RefCell::new("none".to_string()));
    let add = |n: &str, c: ControlRef| {
        host.add(n, c.clone());
        c
    };
    let section: ControlRef = SectionControl::new(None, rect(20.0, 30.0, 400.0, 250.0), "^7Section label".to_string());
    add("section", section);
    let title: ControlRef = add(
        "title",
        LabelControl::new(None, rect(40.0, 50.0, 0.0, 16.0), "^7Label 16 ^1red ^xFFD700gold".to_string()),
    );
    let cl = clicks.clone();
    let small: ControlRef = add(
        "small",
        LabelControl::new(
            anchor("TOPLEFT", &title, "BOTTOMLEFT"),
            rect(0.0, 4.0, 0.0, 14.0),
            pob_ui::Prop::func(move |_| format!("^8Clicks: {}", cl.get())),
        ),
    );
    let cl = clicks.clone();
    let button = ButtonControl::new(
        anchor("TOPLEFT", &small, "BOTTOMLEFT"),
        rect(0.0, 10.0, 100.0, 20.0),
        "Click me".to_string(),
        move |_| cl.set(cl.get() + 1),
    );
    button.tip.tooltip_text.set(Some("Button tooltip\nSecond line ^2green".to_string()));
    let button: ControlRef = add("button", button);
    let disabled = ButtonControl::new(
        anchor("LEFT", &button, "RIGHT"),
        rect(8.0, 0.0, 90.0, 20.0),
        "Disabled".to_string(),
        |_| {},
    );
    disabled.base.enabled.set(false);
    let disabled: ControlRef = add("disabled", disabled);
    let locked = ButtonControl::new(
        anchor("LEFT", &disabled, "RIGHT"),
        rect(8.0, 0.0, 70.0, 20.0),
        "Locked".to_string(),
        |_| {},
    );
    let cl = clicks.clone();
    locked.locked.set_fn(move |_| cl.get() == 0);
    add("locked", locked);
    let plus: ControlRef = add(
        "plus",
        ButtonControl::new(
            anchor("TOPLEFT", &button, "BOTTOMLEFT"),
            rect(0.0, 10.0, 20.0, 20.0),
            "+".to_string(),
            |_| {},
        ),
    );
    let minus: ControlRef = add(
        "minus",
        ButtonControl::new(anchor("LEFT", &plus, "RIGHT"), rect(4.0, 0.0, 20.0, 20.0), "-".to_string(), |_| {}),
    );
    add(
        "cross",
        ButtonControl::new(anchor("LEFT", &minus, "RIGHT"), rect(4.0, 0.0, 20.0, 20.0), "x".to_string(), |_| {}),
    );
    let (ck, lc) = (checked.clone(), last_check.clone());
    let check = CheckBoxControl::new(
        g,
        anchor("TOPLEFT", &plus, "BOTTOMLEFT"),
        [120.0, 12.0, 20.0],
        Some("^7Check me:"),
        Some(Rc::new(move |_g: &mut Gfx, state: bool| {
            ck.set(ck.get() + 1);
            *lc.borrow_mut() = state.to_string();
        })),
        Some("CheckBox tooltip"),
        false,
    );
    let check_rc = check.clone();
    let check: ControlRef = add("check", check);
    add(
        "checkOn",
        CheckBoxControl::new(
            g,
            anchor("LEFT", &check, "RIGHT"),
            [140.0, 0.0, 18.0],
            Some("On:"),
            None,
            None,
            true,
        ),
    );
    let check_right = CheckBoxControl::new(
        g,
        anchor("TOPLEFT", &check, "BOTTOMLEFT"),
        [0.0, 8.0, 16.0],
        Some("right label"),
        None,
        None,
        true,
    );
    check_right.label_right.set(true);
    let check_right: ControlRef = add("checkRight", check_right);
    let check_off = CheckBoxControl::new(
        g,
        anchor("LEFT", &check_right, "RIGHT"),
        [160.0, 0.0, 20.0],
        Some("Disabled:"),
        None,
        None,
        true,
    );
    check_off.base.enabled.set(false);
    add("checkOff", check_off);
    add(
        "outline",
        RectangleOutlineControl::new(None, rect(450.0, 40.0, 120.0, 60.0), Some([0.2, 0.8, 1.0]), Some(2.0)),
    );
    add(
        "outline2",
        RectangleOutlineControl::new(None, rect(450.5, 120.25, 80.0, 30.0), None, None),
    );
    let h = host.clone();
    Env {
        host,
        custom: None,
        dump: Box::new(move |_g| {
            json!({
                "clicks": clicks.get(), "checked": checked.get(), "lastCheck": *last_check.borrow(), "check": check_rc.state.get(),
                "sel": sel_name(&h, &[("button", &button), ("check", &check)]),
            })
        }),
    }
}

/// `G.scroll` (controls-gallery.lua).
pub fn scroll(g: &Gfx) -> Env {
    let host = Rc::new(ControlHost::new());
    let changes = Rc::new(Cell::new(0));
    let v = ScrollBarControl::new(None, rect(40.0, 40.0, 16.0, 200.0), Some(20.0), None, false);
    v.set_content_dimension(g, 1000.0, 200.0);
    let h = ScrollBarControl::new(None, rect(80.0, 40.0, 300.0, 16.0), Some(30.0), Some("HORIZONTAL"), false);
    h.set_content_dimension(g, 450.0, 300.0);
    let off = ScrollBarControl::new(None, rect(80.0, 70.0, 200.0, 12.0), None, Some("HORIZONTAL"), false);
    off.set_content_dimension(g, 100.0, 200.0);
    let auto = ScrollBarControl::new(None, rect(400.0, 40.0, 16.0, 100.0), Some(10.0), None, true);
    let ch = changes.clone();
    let slider = SliderControl::new(
        None,
        rect(80.0, 100.0, 200.0, 18.0),
        Some(Rc::new(move |_val| ch.set(ch.get() + 1))),
        None,
    );
    slider.tip.set_tooltip_func(|tooltip, g, val| {
        tooltip.clear(false);
        tooltip.add_line(g, 14.0, Some(&format!("^7Value: {val:.3}")));
    });
    let div = SliderControl::new(None, rect(80.0, 130.0, 200.0, 20.0), None, None);
    div.div_count.set(Some(4.0));
    div.set_val(0.5);
    host.add("v", v.clone());
    host.add("h", h.clone());
    host.add("off", off);
    host.add("auto", auto);
    host.add("slider", slider.clone());
    host.add("div", div.clone());
    let hst = host.clone();
    let (vr, hr, sr): (ControlRef, ControlRef, ControlRef) = (v.clone(), h.clone(), slider.clone());
    Env {
        host,
        custom: None,
        dump: Box::new(move |_g| {
            json!({
                "v": v.offset.get(), "h": h.offset.get(), "slider": format!("{:.6}", slider.val.get()), "div": format!("{:.6}", div.val.get()),
                "changes": changes.get(), "dragging": v.dragging.get(), "sliderDragging": slider.dragging.get(),
                "sel": sel_name(&hst, &[("v", &vr), ("h", &hr), ("slider", &sr)]),
            })
        }),
    }
}

/// `G.edit` (controls-gallery.lua).
pub fn edit(g: &Gfx) -> Env {
    let host = Rc::new(ControlHost::new());
    let changes = Rc::new(Cell::new(0));
    let last = Rc::new(RefCell::new(String::new()));
    let enter = Rc::new(RefCell::new(String::new()));
    let (ch, la) = (changes.clone(), last.clone());
    let plain = EditControl::new(
        g,
        None,
        rect(40.0, 40.0, 200.0, 20.0),
        EditArgs {
            init: Some("Hello world".into()),
            limit: Some(50),
            change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _ph: bool| {
                ch.set(ch.get() + 1);
                *la.borrow_mut() = buf.to_string();
            })),
            ..EditArgs::default()
        },
    );
    let en = enter.clone();
    *plain.enter_func.borrow_mut() = Some(Rc::new(move |_g: &mut Gfx, buf: &str| *en.borrow_mut() = buf.to_string()));
    let prompt = EditControl::new(
        g,
        None,
        rect(40.0, 70.0, 260.0, 20.0),
        EditArgs {
            init: Some(String::new()),
            prompt: Some("Search".into()),
            ..EditArgs::default()
        },
    );
    *prompt.placeholder.borrow_mut() = Some("type here".into()); // SetPlaceholder(text) without notify
    let num = EditControl::new(
        g,
        None,
        rect(40.0, 100.0, 100.0, 20.0),
        EditArgs {
            init: Some("5".into()),
            filter: Some("%D".into()),
            limit: Some(4),
            ..EditArgs::default()
        },
    );
    let multi = EditControl::new(
        g,
        None,
        rect(40.0, 130.0, 300.0, 60.0),
        EditArgs {
            init: Some("line one\nline two is longer than the box is wide\n\tthird\nfour".into()),
            filter: Some("^%w%p%s".into()),
            line_height: Some(16.0),
            ..EditArgs::default()
        },
    );
    let protect = EditControl::new(
        g,
        None,
        rect(40.0, 240.0, 150.0, 20.0),
        EditArgs {
            init: Some("secret".into()),
            ..EditArgs::default()
        },
    );
    protect.set_protected(true);
    let clear = EditControl::new(
        g,
        None,
        rect(200.0, 240.0, 150.0, 20.0),
        EditArgs {
            init: Some("clear me".into()),
            clearable: true,
            ..EditArgs::default()
        },
    );
    let disabled = EditControl::new(
        g,
        None,
        rect(360.0, 240.0, 120.0, 20.0),
        EditArgs {
            init: Some("disabled".into()),
            prompt: Some("P".into()),
            ..EditArgs::default()
        },
    );
    disabled.base.enabled.set(false);
    let named: Vec<(&'static str, ControlRef)> = vec![
        ("plain", plain.clone()),
        ("prompt", prompt.clone()),
        ("num", num.clone()),
        ("multi", multi.clone()),
        ("protect", protect),
        ("clear", clear.clone()),
        ("disabled", disabled),
    ];
    for (n, c) in &named {
        host.add(n, c.clone());
    }
    let hst = host.clone();
    let e = |c: &EditControl| {
        let sel = c.sel.get().map_or(json!("nil"), |s| json!(s));
        json!({ "buf": *c.buf.borrow(), "caret": c.caret.get(), "sel": sel, "focus": c.base.has_focus.get() })
    };
    Env {
        host,
        custom: None,
        dump: Box::new(move |g| {
            let refs: Vec<(&str, &ControlRef)> = named.iter().map(|(n, c)| (*n, c)).collect();
            json!({
                "plain": e(&plain), "prompt": e(&prompt), "num": e(&num), "multi": e(&multi), "clear": e(&clear),
                "multiScroll": [multi.scroll_bar_h.offset.get(), multi.scroll_bar_v.offset.get()],
                "changes": changes.get(), "last": *last.borrow(), "enter": *enter.borrow(),
                "clip": g.paste().unwrap_or_else(|| "nil".into()), "sel": sel_name(&hst, &refs),
            })
        }),
    }
}

/// `G.drop` (controls-gallery.lua).
pub fn drop(g: &Gfx) -> Env {
    let host = Rc::new(ControlHost::new());
    let sels = Rc::new(Cell::new(0));
    let last_sel = Rc::new(RefCell::new("none".to_string()));
    let mut list: Vec<DropItem> = (1..=30).map(|i| DropItem::str(&format!("Item {i}"))).collect();
    list[4] = DropItem::str("^xFF8080Coloured five");
    let (s, l) = (sels.clone(), last_sel.clone());
    let dd = DropDownControl::new(
        g,
        None,
        rect(40.0, 40.0, 200.0, 20.0),
        list,
        Some(Rc::new(move |_g: &mut Gfx, index: usize, value: &DropItem| {
            s.set(s.get() + 1);
            *l.borrow_mut() = format!("{index}:{}", value.label.as_deref().unwrap_or(""));
        })),
        Some("Dropdown tooltip"),
        false,
    );
    let mut items = vec![DropItem::table("Alpha"), DropItem::table("Beta"), DropItem::table("Gamma beta")];
    for (it, d) in items.iter_mut().zip(["1st", "2nd", "3rd"]) {
        it.detail = Some(d.to_string());
    }
    items[2].search_filter = Some("gamma delta".into());
    let (s, l) = (sels.clone(), last_sel.clone());
    let tbl = DropDownControl::new(
        g,
        None,
        rect(300.0, 40.0, 220.0, 20.0),
        items,
        Some(Rc::new(move |_g: &mut Gfx, index: usize, value: &DropItem| {
            s.set(s.get() + 1);
            *l.borrow_mut() = format!("{index}:{}", value.label.as_deref().unwrap_or(""));
        })),
        None,
        false,
    );
    let up = DropDownControl::new(
        g,
        None,
        rect(40.0, 760.0, 150.0, 20.0),
        ["one", "two", "three", "four", "five"].map(DropItem::str).to_vec(),
        None,
        None,
        false,
    );
    let wide = DropDownControl::new(
        g,
        None,
        rect(600.0, 40.0, 100.0, 20.0),
        ["short", "a much longer entry than the box"].map(DropItem::str).to_vec(),
        None,
        None,
        false,
    );
    wide.check_dropped_width(g, true);
    let off = DropDownControl::new(
        g,
        None,
        rect(40.0, 100.0, 150.0, 20.0),
        vec![DropItem::str("disabled")],
        None,
        None,
        false,
    );
    off.base.enabled.set(false);
    let named: Vec<(&'static str, ControlRef)> = vec![
        ("dd", dd.clone()),
        ("tbl", tbl.clone()),
        ("up", up.clone()),
        ("wide", wide.clone()),
        ("off", off),
    ];
    for (n, c) in &named {
        host.add(n, c.clone());
    }
    let hst = host.clone();
    let d = |c: &DropDownControl| {
        json!({
            "sel": c.sel_index.get(), "dropped": c.dropped.get(), "term": *c.search.search_term.borrow(), "count": c.get_drop_count(),
            "scroll": c.scroll_bar.offset.get(), "hover": c.hover_sel.get().map_or(json!("nil"), |h| json!(h)), "up": c.drop_up.get(),
            "dw": c.dropped_width.get(),
        })
    };
    Env {
        host,
        custom: None,
        dump: Box::new(move |_g| {
            let refs: Vec<(&str, &ControlRef)> = named.iter().map(|(n, c)| (*n, c)).collect();
            json!({ "dd": d(&dd), "tbl": d(&tbl), "up": d(&up), "wide": d(&wide), "sels": sels.get(), "lastSel": *last_sel.borrow(), "sel": sel_name(&hst, &refs) })
        }),
    }
}

pub struct Item {
    pub name: String,
    pub n: usize,
}

#[derive(Default)]
struct ListStats {
    selects: Cell<u32>,
    clicks: Cell<u32>,
    order: Cell<u32>,
    deleted: Cell<u32>,
    text_clicks: Cell<u32>,
}

struct MainListHooks(Rc<ListStats>);

impl ListHooks<ByRef<Item>> for MainListHooks {
    fn get_row_value(&self, _l: &ListControl<ByRef<Item>>, _g: &Gfx, column: usize, _index: usize, value: &ByRef<Item>) -> String {
        if column == 1 {
            value.0.name.clone()
        } else {
            format!("#{}", value.0.n)
        }
    }
    fn has_add_value_tooltip(&self) -> bool {
        true
    }
    fn add_value_tooltip(&self, _l: &ListControl<ByRef<Item>>, g: &Gfx, tooltip: &mut pob_ui::Tooltip, _index: usize, value: &ByRef<Item>) {
        tooltip.clear(false);
        tooltip.add_line(g, 14.0, Some(&format!("^7Tooltip for {}", value.0.name)));
    }
    fn on_select(&self, _l: &ListControl<ByRef<Item>>, _g: &mut Gfx, _i: usize, _v: &ByRef<Item>) {
        self.0.selects.set(self.0.selects.get() + 1);
    }
    fn on_sel_click(&self, _l: &ListControl<ByRef<Item>>, _g: &mut Gfx, _i: usize, _v: &ByRef<Item>, _d: bool) -> Option<bool> {
        self.0.clicks.set(self.0.clicks.get() + 1);
        None
    }
    fn on_order_change(&self, _l: &ListControl<ByRef<Item>>, _a: usize, _b: usize) {
        self.0.order.set(self.0.order.get() + 1);
    }
    fn on_sel_delete(&self, l: &ListControl<ByRef<Item>>, _g: &mut Gfx, index: usize, _v: &ByRef<Item>) {
        l.list.borrow_mut().remove(index - 1);
        l.sel_index.set(None);
        *l.sel_value.borrow_mut() = None;
        self.0.deleted.set(self.0.deleted.get() + 1);
    }
}

struct StrHooks;

impl ListHooks<String> for StrHooks {
    fn get_row_value(&self, _l: &ListControl<String>, _g: &Gfx, _c: usize, _i: usize, value: &String) -> String {
        value.clone()
    }
}

/// `G.list` (controls-gallery.lua).
pub fn list(_g: &Gfx) -> Env {
    let host = Rc::new(ControlHost::new());
    let st = Rc::new(ListStats::default());
    let mut items: Vec<ByRef<Item>> = (1..=25)
        .map(|n| {
            ByRef(Rc::new(Item {
                name: format!("Entry number {n}"),
                n,
            }))
        })
        .collect();
    items[2] = ByRef(Rc::new(Item {
        name: "A rather long entry name that will be clipped by the column width".into(),
        n: 3,
    }));
    let l = ListControl::new(
        None,
        rect(40.0, 40.0, 260.0, 200.0),
        18.0,
        Some("VERTICAL"),
        true,
        items,
        false,
        Box::new(MainListHooks(st.clone())),
    );
    *l.col_list.borrow_mut() = vec![
        Column {
            width: Some(180.0.into()),
            label: Some("Name".to_string().into()),
            sortable: true,
            ..Column::default()
        },
        Column {
            label: Some("N".to_string().into()),
            ..Column::default()
        },
    ];
    l.col_labels.set(true);
    l.label.set(Some("^7List label".to_string()));
    let sep = ListControl::new(
        None,
        rect(340.0, 40.0, 200.0, 120.0),
        20.0,
        None,
        false,
        ["one", "two", "three"].map(String::from).to_vec(),
        false,
        Box::new(StrHooks),
    );
    sep.show_row_separators.set(true);
    let empty = ListControl::new(
        None,
        rect(340.0, 180.0, 200.0, 60.0),
        16.0,
        Some("VERTICAL"),
        false,
        Vec::new(),
        false,
        Box::new(StrHooks),
    );
    *empty.default_text.borrow_mut() = Some("^8No entries".into());
    let lines: Vec<TextLine> = (1..=12)
        .map(|i| TextLine {
            height: 16.0,
            cols: vec![Some(format!("^7Line {i}")), Some(format!("^x80FF80{}", i * 10))],
            underline: if i == 2 { vec![true, true] } else { Vec::new() },
            font: (i == 4).then_some("FIXED"),
            align: (i == 5).then_some("CENTER_X"),
            x: (i == 5).then_some(100.0),
        })
        .collect();
    let text = TextListControl::new(
        None,
        rect(580.0, 40.0, 220.0, 150.0),
        Some(vec![
            TextColumn { x: 0.0, align: "LEFT" },
            TextColumn {
                x: 196.0,
                align: "RIGHT_X",
            },
        ]),
        lines,
        Some(vec![0.0, 64.0]),
    );
    let s2 = st.clone();
    *text.on_click.borrow_mut() = Some(Rc::new(move |_g, _line| s2.text_clicks.set(s2.text_clicks.get() + 1)));
    let named: Vec<(&'static str, ControlRef)> = vec![("list", l.clone()), ("sep", sep.clone()), ("empty", empty), ("text", text.clone())];
    for (n, c) in &named {
        host.add(n, c.clone());
    }
    let hst = host.clone();
    let opt = |o: Option<usize>| o.map_or(json!("nil"), |v| json!(v));
    Env {
        host,
        custom: None,
        dump: Box::new(move |_g| {
            let refs: Vec<(&str, &ControlRef)> = named.iter().map(|(n, c)| (*n, c)).collect();
            let order: Vec<String> = l.list.borrow().iter().map(|v| v.0.n.to_string()).collect();
            json!({
                "sel": sel_name(&hst, &refs), "selIndex": opt(l.sel_index.get()), "hasSelValue": l.sel_value.borrow().is_some(),
                "order": order.join(","), "hover": opt(l.hover_index.get()), "scroll": l.scroll_bar_v.offset.get(),
                "drag": l.sel_drag_active.get(), "sepSel": opt(sep.sel_index.get()), "textScroll": text.scroll_bar.offset.get(),
                "textHover": text.hovered_line.get().map_or(json!("nil"), |(_, y)| json!(y)),
                "selects": st.selects.get(), "clicks": st.clicks.get(), "orderChanges": st.order.get(), "deleted": st.deleted.get(),
                "textClicks": st.text_clicks.get(),
            })
        }),
    }
}

pub fn build(name: &str, g: &mut Gfx) -> Env {
    match name {
        "popup" => popup(g),
        "dragger" => dragger(g),
        "basic" => basic(g),
        "scroll" => scroll(g),
        "edit" => edit(g),
        "drop" => drop(g),
        "list" => list(g),
        other => panic!("no gallery builder {other}"),
    }
}

/// Result of a scenario: dumps per step and the draw list of each screenshot frame.
pub struct Run {
    pub dumps: Vec<(u64, Value)>,
    pub shots: Vec<(String, DrawList)>,
}

fn phys_key(name: &str) -> RawEvent {
    let key = match name {
        "CTRL" => PhysKey::Ctrl,
        "SHIFT" => PhysKey::Shift,
        "ALT" => PhysKey::Alt,
        "LEFTBUTTON" => {
            return RawEvent::Button {
                button: Button::Left,
                pressed: true,
                pos: None,
            };
        }
        other => panic!("hold of {other} not supported"),
    };
    RawEvent::Key { key, pressed: true }
}

fn release(ev: RawEvent) -> RawEvent {
    match ev {
        RawEvent::Key { key, .. } => RawEvent::Key { key, pressed: false },
        RawEvent::Button { button, pos, .. } => RawEvent::Button {
            button,
            pressed: false,
            pos,
        },
        e => e,
    }
}

fn frame(gfx: &mut Gfx, env: &Env, events: Vec<Option<InputEvent>>) -> DrawList {
    let vp = Rect::new(0.0, 0.0, 1280.0, 800.0);
    let mut events = events;
    let mut f = gfx.begin_frame();
    pob_ui::sg::set_draw_layer(&mut f, Some(0), 0);
    f.set_viewport(None);
    f.set_draw_color(1.0, 1.0, 1.0, 1.0);
    if let Some(custom) = &env.custom {
        custom(&mut f, &mut events, &vp);
    } else {
        env.host.process_controls_input(&mut f, &mut events, &vp);
        env.host.draw_controls(&mut f, &vp, None);
    }
    f.finish()
}

/// Runs `scenes/controls/<name>.json` like `controls-runner.lua`.
pub fn run(name: &str) -> Run {
    let p = scene::oracle_dir().join(format!("scenes/controls/{name}.json"));
    let sc: Value = serde_json::from_str(&std::fs::read_to_string(&p).expect("scenario")).expect("json");
    let tp = TestPlatform::new();
    let mut gfx = Gfx::with_platform(&scene::fonts_dir(), scene::SCREEN, 1.0, Box::new(tp.clone())).expect("fonts");
    gfx.set_dpi_scale_override_percent(100);
    gfx.input(RawEvent::Cursor { x: -100.0, y: -100.0 });
    let env = build(sc["build"].as_str().expect("build"), &mut gfx);
    let mut out = Run {
        dumps: Vec::new(),
        shots: Vec::new(),
    };
    let mut held: HashMap<String, RawEvent> = HashMap::new();
    for _ in 0..2 {
        frame(&mut gfx, &env, Vec::new());
    }
    for (i, st) in sc["steps"].as_array().expect("steps").iter().enumerate() {
        if let Some(c) = st["cursor"].as_array() {
            gfx.input(RawEvent::Cursor {
                x: c[0].as_f64().expect("x"),
                y: c[1].as_f64().expect("y"),
            });
        }
        if let Some(t) = st["time"].as_u64() {
            tp.set_time(t);
        }
        for k in st["hold"].as_array().into_iter().flatten() {
            let ev = phys_key(k.as_str().expect("key"));
            gfx.input(ev);
            held.insert(k.as_str().expect("key").to_string(), ev);
        }
        for k in st["release"].as_array().into_iter().flatten() {
            if let Some(ev) = held.remove(k.as_str().expect("key")) {
                gfx.input(release(ev));
            }
        }
        let events = st["events"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                let key = e[1].as_str().expect("key").to_string();
                Some(match e[0].as_str().expect("type") {
                    "KeyDown" => InputEvent::KeyDown {
                        key,
                        double_click: e.get(2).and_then(Value::as_bool).unwrap_or(false),
                    },
                    "KeyUp" => InputEvent::KeyUp { key },
                    "Char" => InputEvent::Char { key },
                    other => panic!("event {other}"),
                })
            })
            .collect();
        frame(&mut gfx, &env, events);
        if st["dump"].as_bool() == Some(true) {
            out.dumps.push(((i + 1) as u64, (env.dump)(&mut gfx)));
        }
        if let Some(shot) = st["shot"].as_str() {
            let list = frame(&mut gfx, &env, Vec::new());
            out.shots.push((shot.to_string(), list));
        }
    }
    out
}

/// The oracle's record `fixtures/controls/<name>.json`.
pub fn oracle_record(name: &str) -> Value {
    let p = scene::oracle_dir().join(format!("fixtures/controls/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).expect("json")
}

pub fn scenario_names() -> Vec<String> {
    let p = scene::oracle_dir().join("scenes/controls/index.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("index")).expect("json")
}

/// `G.popup` (controls-gallery.lua).
pub fn popup(g: &mut Gfx) -> Env {
    pob_ui::common::set_screen_size(1280.0, 800.0);
    let saved = Rc::new(Cell::new(0));
    let cancelled = Rc::new(Cell::new(0));
    let changes = Rc::new(Cell::new(0));
    let save_cell: Rc<RefCell<Option<Rc<ButtonControl>>>> = Rc::new(RefCell::new(None));
    let mut controls: indexmap::IndexMap<String, ControlRef> = indexmap::IndexMap::new();
    controls.insert(
        "1".into(),
        LabelControl::new(None, rect(0.0, 20.0, 0.0, 16.0), "^7First line of the message".to_string()),
    );
    controls.insert(
        "2".into(),
        LabelControl::new(None, rect(0.0, 36.0, 0.0, 16.0), "^8second ^1line".to_string()),
    );
    let (ch, sc) = (changes.clone(), save_cell.clone());
    let edit = EditControl::new(
        g,
        None,
        rect(0.0, 60.0, 350.0, 20.0),
        EditArgs {
            init: Some("start".into()),
            filter: Some(r#"\/:%*%?"<>|%c"#.into()),
            limit: Some(100),
            change_func: Some(Rc::new(move |_g: &mut Gfx, buf: &str, _| {
                ch.set(ch.get() + 1);
                let save = sc.borrow().clone().expect("save");
                save.base.enabled.set(pob_ui::lua::is_match(buf, "%S"));
            })),
            ..Default::default()
        },
    );
    controls.insert("edit".into(), edit.clone());
    let s = saved.clone();
    let save = ButtonControl::new(None, rect(-45.0, 90.0, 80.0, 20.0), "Save".to_string(), move |_| s.set(s.get() + 1));
    *save_cell.borrow_mut() = Some(save.clone());
    controls.insert("save".into(), save.clone());
    let c = cancelled.clone();
    controls.insert(
        "cancel".into(),
        ButtonControl::new(None, rect(45.0, 90.0, 80.0, 20.0), "Cancel".to_string(), move |_| {
            c.set(c.get() + 1)
        }),
    );
    let off = ButtonControl::new(
        pob_ui::control::anchor_nil("TOPLEFT", "TOPLEFT"),
        rect(10.0, 90.0, 50.0, 20.0),
        "Off".to_string(),
        |_| {},
    );
    off.base.enabled.set(false);
    controls.insert("off".into(), off);
    let named: Vec<(String, ControlRef)> = controls.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let popup = PopupDialog::new(
        g,
        370.0,
        120.0,
        "Popup ^1Title",
        controls,
        Some("save"),
        Some("edit"),
        Some("cancel"),
        None,
        None,
    );
    let p = popup.clone();
    Env {
        host: Rc::new(ControlHost::new()),
        custom: Some(Box::new(move |f, events, vp| {
            p.process_input(f, events, vp);
            pob_ui::sg::set_draw_layer(f, Some(10), 0);
            pob_ui::sg::set_draw_color_a(f, 0.0, 0.0, 0.0, 0.5);
            let (w, h) = pob_ui::common::screen_size();
            pob_ui::sg::draw_rect(f, 0.0, 0.0, w, h);
            p.draw_popup(f, vp);
            pob_ui::sg::set_draw_layer(f, Some(0), 0);
        })),
        dump: Box::new(move |g| {
            let refs: Vec<(&str, &ControlRef)> = named.iter().map(|(n, c)| (n.as_str(), c)).collect();
            let (x, y) = pob_ui::Control::get_pos(&*popup, g);
            json!({
                "saved": saved.get(), "cancelled": cancelled.get(), "changes": changes.get(), "buf": *edit.buf.borrow(),
                "sel": sel_name(&popup.host, &refs), "x": x, "y": y, "saveEnabled": save.base.enabled.get(g),
            })
        }),
    }
}

/// `G.dragger` (controls-gallery.lua).
pub fn dragger(g: &mut Gfx) -> Env {
    let host = Rc::new(ControlHost::new());
    let down = Rc::new(Cell::new((-1.0, -1.0)));
    let up = Rc::new(Cell::new((0.0, 0.0)));
    let right = Rc::new(Cell::new(0));
    let (d, u, r) = (down.clone(), up.clone(), right.clone());
    let drag = DraggerControl::new(
        None,
        rect(40.0, 40.0, 20.0, 20.0),
        "+".to_string(),
        Some(Rc::new(move |_g: &mut Gfx, x, y| d.set((x, y)))),
        Some(Rc::new(move |_g: &mut Gfx, x, y| u.set((x, y)))),
        Some(Rc::new(move |_g: &mut Gfx, _x, _y| r.set(r.get() + 1))),
    );
    drag.tip.tooltip_text.set(Some("Dragger tooltip".to_string()));
    let drag_ref: ControlRef = drag.clone();
    let mk = |prev: &ControlRef, w: f64, label: &str| -> Rc<DraggerControl> {
        DraggerControl::new(
            anchor("LEFT", prev, "RIGHT"),
            rect(4.0, 0.0, w, 20.0),
            label.to_string(),
            None,
            None,
            None,
        )
    };
    let minus: ControlRef = mk(&drag_ref, 20.0, "-");
    let cross: ControlRef = mk(&minus, 20.0, "x");
    let slash: ControlRef = mk(&cross, 20.0, "//");
    let text: ControlRef = mk(&slash, 60.0, "Drag");
    let locked = mk(&text, 60.0, "Lock");
    locked.locked.set(true);
    let locked: ControlRef = locked;
    let off = mk(&locked, 60.0, "Off");
    off.base.enabled.set(false);
    let off: ControlRef = off;
    let redit = ResizableEditControl::new(
        g,
        None,
        [
            Some(100.0),
            Some(100.0),
            Some(200.0),
            Some(40.0),
            Some(100.0),
            Some(30.0),
            Some(400.0),
            Some(200.0),
        ],
        EditArgs {
            init: Some("line one\nline two".into()),
            line_height: Some(16.0),
            ..Default::default()
        },
    );
    let redit_ref: ControlRef = redit.clone();
    let named: Vec<(&'static str, ControlRef)> = vec![
        ("drag", drag_ref),
        ("minus", minus),
        ("cross", cross),
        ("slash", slash),
        ("text", text),
        ("locked", locked),
        ("off", off),
        ("redit", redit_ref),
    ];
    for (n, c) in &named {
        host.add(n, c.clone());
    }
    let h = host.clone();
    Env {
        host,
        custom: None,
        dump: Box::new(move |g| {
            let refs: Vec<(&str, &ControlRef)> = named.iter().map(|(n, c)| (*n, c)).collect();
            let ((dx, dy), (ux, uy)) = (down.get(), up.get());
            json!({
                "downX": dx, "downY": dy, "upDX": ux, "upDY": uy, "right": right.get(),
                "dragging": drag.dragging.get(), "clicked": drag.clicked.get(), "sel": sel_name(&h, &refs),
                "w": redit.edit.base.width.get(g), "h": redit.edit.base.height.get(g), "rdrag": redit.dragger_height.dragging.get(),
            })
        }),
    }
}
