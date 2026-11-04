//! `Classes/CalcBreakdownControl.lua`: the breakdown popup of the Calcs tab and of the side bar. What goes in it is `pob_engine::calcs_tab`; here are the
//! sizes, the position and the drawing.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use pob_engine::calcs::Entry;
use pob_engine::calcs_tab::{BdEnv, BdHover, BdRow, BdSection, ForceActor, SlotOrders};
use pob_engine::display::format_num_sep;
use pob_gfx::{Frame, Gfx, ImageHandle};

use super::calcs::{CalcsTab, CellKey};
use crate::control::{Control, ControlBase, ControlRef, Rect, WeakControl, anchor_weak, rect};
use crate::controls::scroll_bar::ScrollBarControl;
use crate::host::ControlHost;
use crate::modules::main::main;
use crate::sg;
use crate::tooltip::Tooltip;
use crate::tree_view::PassiveTreeView;

/// `displayData` the popup shows: a cell of the tab or the table `GetSidebarBreakdown` built.
#[derive(Clone)]
pub enum Source {
    Cell(CellKey),
    Sidebar(Rc<SidebarData>),
}

impl PartialEq for Source {
    fn eq(&self, o: &Source) -> bool {
        match (self, o) {
            (Source::Cell(a), Source::Cell(b)) => a == b,
            (Source::Sidebar(a), Source::Sidebar(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// `sidebarBreakdownData`: the cell descriptions + `x`, `y`, `width` (Build.lua:1372-1374).
#[derive(Clone, Debug, PartialEq)]
pub struct SidebarData {
    pub entries: Vec<Entry>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
}

/// One entry of `self.sectionList` with what `SetBreakdownData` measured.
pub struct Sec {
    pub data: BdSection,
    pub width: f64,
    pub height: f64,
    pub offset: f64,
    /// `col.width` by column.
    pub col_width: Vec<Option<f64>>,
}

pub struct CalcBreakdownControl {
    pub base: ControlBase,
    pub host: ControlHost,
    me: Weak<Self>,
    tab: Weak<CalcsTab>,
    pub env: BdEnv,
    tooltip: RefCell<Tooltip>,
    node_viewer: RefCell<Option<PassiveTreeView>>,
    art: RefCell<Option<(ImageHandle, ImageHandle)>>,
    pub border_thickness: f64,
    pub pinned_colour: Cell<[f64; 3]>,
    pub border_colour: Cell<[f64; 3]>,
    pub clear_display_func: RefCell<Option<Rc<dyn Fn(&mut Gfx)>>>,
    pub source: RefCell<Option<Source>>,
    pub pinned: Cell<bool>,
    force_actor: Cell<Option<ForceActor>>,
    pub section_list: RefCell<Vec<Sec>>,
    pub content_width: Cell<f64>,
    pub content_height: Cell<f64>,
    pub scroll_bar: Rc<ScrollBarControl>,
    slot_orders: RefCell<(u64, SlotOrders)>,
}

/// `select(2, string.gsub(s, "%d%d%d%d", ""))`.
fn count_four_digits(s: &str) -> usize {
    let b = s.as_bytes();
    let (mut i, mut n) = (0, 0);
    while i + 4 <= b.len() {
        if b[i..i + 4].iter().all(u8::is_ascii_digit) {
            n += 1;
            i += 4;
        } else {
            i += 1;
        }
    }
    n
}

/// The count of `string.gsub(s, "%a", " ")` / `" to "` / `"%b()"` > 0 test of :609-612.
fn is_numeric_like(s: &str) -> bool {
    !s.bytes().any(|b| b.is_ascii_alphabetic()) || s.contains(" to ") || crate::lua::is_match(s, "%b()")
}

fn width(g: &Gfx, h: f64, text: &str) -> f64 {
    sg::draw_string_width(g, h, "VAR", text)
}

impl CalcBreakdownControl {
    pub fn new(tab: Weak<CalcsTab>, env: BdEnv) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| {
            let w: WeakControl = me.clone();
            let scroll_bar = ScrollBarControl::new(
                anchor_weak("RIGHT", w, "RIGHT"),
                rect(-2.0, 0.0, 18.0, 0.0),
                Some(80.0),
                Some("VERTICAL"),
                true,
            );
            scroll_bar.base.x.set(-2.0);
            let host = ControlHost::new();
            host.add("scrollBar", scroll_bar.clone());
            let base = ControlBase::new(None, rect(0.0, 0.0, 0.0, 0.0));
            base.shown.set(false);
            Self {
                base,
                host,
                me: me.clone(),
                tab,
                env,
                tooltip: RefCell::new(Tooltip::new()),
                node_viewer: RefCell::new(None),
                art: RefCell::new(None),
                border_thickness: 2.0,
                pinned_colour: Cell::new([0.25, 1.0, 0.25]),
                border_colour: Cell::new([0.33, 0.66, 0.33]),
                clear_display_func: RefCell::new(None),
                source: RefCell::new(None),
                pinned: Cell::new(false),
                force_actor: Cell::new(None),
                section_list: RefCell::new(Vec::new()),
                content_width: Cell::new(0.0),
                content_height: Cell::new(0.0),
                scroll_bar,
                slot_orders: RefCell::new((0, SlotOrders::new())),
            }
        })
    }

    fn tab(&self) -> Rc<CalcsTab> {
        self.tab.upgrade().expect("calcsTab")
    }

    fn clear_display(&self, g: &mut Gfx) {
        let f = self.clear_display_func.borrow().clone();
        match f {
            Some(f) => f(g),
            None => self.tab().clear_display_stat(g),
        }
    }

    /// The entries of `displayData` (`ipairs(displayData)`).
    fn entries_of(&self, s: &Source) -> Vec<Entry> {
        match s {
            Source::Cell(k) => self.tab().cell_def(*k).entries.clone(),
            Source::Sidebar(d) => d.entries.clone(),
        }
    }

    /// `sourceData.x`, `.y`, `.width`.
    fn source_rect(&self, s: &Source) -> (f64, f64, f64) {
        match s {
            Source::Cell(k) => self.tab().cell_rect(*k),
            Source::Sidebar(d) => (d.x, d.y, d.width),
        }
    }

    /// :60-144 `SetBreakdownData(displayData, pinned, forceActor)`.
    pub fn set_breakdown_data(&self, g: &mut Gfx, display_data: Option<Source>, pinned: bool, force_actor: Option<ForceActor>) {
        self.pinned.set(pinned);
        if display_data == *self.source.borrow() {
            return;
        }
        *self.source.borrow_mut() = display_data.clone();
        self.force_actor.set(force_actor);
        self.base.shown.set(false);
        let Some(display_data) = display_data else { return };
        let entries = self.entries_of(&display_data);
        let tab = self.tab();
        let sections = {
            let b = tab.build();
            let eng = b.engine.borrow();
            let e = eng.as_ref().expect("engine");
            let mut so = self.slot_orders.borrow_mut();
            let rev = e.output_revision();
            if so.0 != rev {
                *so = (rev, SlotOrders::new());
            }
            e.breakdown_sections(self.env, force_actor, &entries, &mut so.1)
                .unwrap_or_else(|err| panic!("Classes/CalcBreakdownControl.lua: {err}"))
        };
        if sections.is_empty() {
            self.clear_display(g);
            return;
        }
        self.base.shown.set(true);
        let thousands = main().s.borrow().show_thousands_separators;
        let mut content_width: f64 = 0.0;
        let mut offset = 2.0;
        let mut list = Vec::with_capacity(sections.len());
        for data in sections {
            let mut s = Sec {
                width: 0.0,
                height: 0.0,
                offset: 0.0,
                col_width: Vec::new(),
                data,
            };
            match &s.data {
                BdSection::Text { lines, text_size } => {
                    for line in lines {
                        let num = count_four_digits(line);
                        let extra = if thousands && num > 0 { 4.0 * num as f64 } else { 0.0 };
                        s.width = s.width.max(width(g, *text_size, line) + 8.0 + extra);
                    }
                    s.height = lines.len() as f64 * text_size + 4.0;
                }
                BdSection::Radius { .. } => {
                    s.width = 8.0 + 1920.0 / 4.0;
                    s.height = 4.0 + 1080.0 / 4.0;
                }
                BdSection::Table { label, footer, rows, cols } => {
                    s.width = 4.0;
                    for col in cols {
                        let mut w: Option<f64> = None;
                        for row in rows {
                            if let Some(v) = row.cells.get(&col.key) {
                                let num = count_four_digits(v);
                                let extra = if thousands && num > 0 { 4.0 * num as f64 } else { 0.0 };
                                w = Some(
                                    w.unwrap_or(0.0)
                                        .max(width(g, 16.0, &col.label) + 6.0)
                                        .max(width(g, 12.0, v) + 6.0 + extra),
                                );
                            }
                        }
                        if let Some(w) = w {
                            s.width += w;
                        }
                        s.col_width.push(w);
                    }
                    s.height = rows.len() as f64 * 14.0 + 20.0;
                    if let Some(l) = label {
                        content_width = content_width.max(6.0 + width(g, 16.0, &format!("{l}:")));
                        s.height += 16.0;
                    }
                    if let Some(f) = footer {
                        content_width = content_width.max(6.0 + width(g, 12.0, f));
                        s.height += 12.0 * (f.matches('\n').count() + 1) as f64;
                    }
                }
            }
            content_width = content_width.max(s.width);
            s.offset = offset;
            offset += s.height + 8.0;
            list.push(s);
        }
        *self.section_list.borrow_mut() = list;
        self.content_width.set(content_width);
        self.content_height.set(offset - 6.0);
    }

    /// :579-661 `DrawBreakdownTable`.
    fn draw_table(&self, f: &mut Frame<'_>, view_port: &Rect, x: f64, mut y: f64, s: &Sec) {
        let BdSection::Table { label, footer, rows, cols } = &s.data else {
            return;
        };
        let (cursor_x, cursor_y) = f.get_cursor_pos();
        let st = crate::modules::build::BuildMode::display_settings();
        if let Some(l) = label {
            sg::draw_string(f, x + 2.0, y, Some("LEFT"), 16.0, "VAR", &format!("^7{l}:"));
            y += 16.0;
        }
        let mut col_x = x + 4.0;
        let mut xs = vec![0.0; cols.len()];
        for (index, col) in cols.iter().enumerate() {
            if let Some(w) = s.col_width[index] {
                xs[index] = col_x;
                if index > 0 {
                    sg::set_draw_color(f, 0.5, 0.5, 0.5);
                    let h = s.height - if label.is_some() { 16.0 } else { 0.0 } - if footer.is_some() { 12.0 } else { 0.0 };
                    sg::draw_rect(f, col_x - 2.0, y, 1.0, h);
                }
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
                sg::draw_string(f, col_x, y + 2.0, Some("LEFT"), 16.0, "VAR", &col.label);
                col_x += w;
            }
        }
        let mut row_y = y + 20.0;
        for row in rows {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
            sg::draw_rect(f, x + 2.0, row_y - 1.0, s.width - 4.0, 1.0);
            for (ci, col) in cols.iter().enumerate() {
                let Some(cw) = s.col_width[ci] else { continue };
                let Some(v) = row.cells.get(&col.key) else { continue };
                let cx = xs[ci];
                if is_numeric_like(v) && col.right {
                    sg::draw_string(
                        f,
                        cx + cw - 4.0,
                        row_y + 1.0,
                        Some("RIGHT_X"),
                        12.0,
                        "VAR",
                        &format!("^7{}", format_num_sep(v, &st)),
                    );
                } else if is_numeric_like(v) {
                    sg::draw_string(
                        f,
                        cx,
                        row_y + 1.0,
                        Some("LEFT"),
                        12.0,
                        "VAR",
                        &format!("^7{}", format_num_sep(v, &st)),
                    );
                } else {
                    sg::draw_string(f, cx, row_y + 1.0, Some("LEFT"), 12.0, "VAR", &format!("^7{v}"));
                }
                self.draw_cell_hover(f, view_port, row, &col.key, (cx, cw, row_y), (cursor_x, cursor_y));
            }
            row_y += 14.0;
        }
        if let Some(t) = footer {
            sg::draw_string(f, x + 2.0, row_y, Some("LEFT"), 12.0, "VAR", &format!("^7{t}"));
        }
    }

    /// :619-652 the tooltip / node location of a hovered cell.
    fn draw_cell_hover(&self, f: &mut Frame<'_>, view_port: &Rect, row: &BdRow, key: &str, cell: (f64, f64, f64), cursor: (f64, f64)) {
        let Some(hover) = row.hover.get(key) else { return };
        let (cx, cw, row_y) = cell;
        let (cursor_x, cursor_y) = cursor;
        if !(cursor_y >= view_port.y + 2.0
            && cursor_y < view_port.y + view_port.height - 2.0
            && cursor_x >= cx
            && cursor_y >= row_y
            && cursor_x < cx + cw
            && cursor_y < row_y + 14.0)
        {
            return;
        }
        sg::set_draw_layer(f, None, 15);
        sg::set_draw_color(f, 0.0, 1.0, 0.0);
        sg::draw_rect(f, cx - 2.0, row_y - 1.0, cw, 1.0);
        sg::draw_rect(f, cx - 2.0, row_y + 13.0, cw, 1.0);
        match hover {
            BdHover::SourceTotals(source, lines) => {
                let mut t = self.tooltip.borrow_mut();
                t.clear(false);
                t.add_line(f, 16.0, Some(&format!("Total from {source}:")));
                for l in lines {
                    t.add_line(f, 14.0, Some(l));
                }
                t.draw(f, cx, row_y, Some((cw, 12.0)), view_port);
            }
            BdHover::Item(id) => {
                let build = self.tab().build();
                let items_tab = build.items_tab.borrow().clone().expect("itemsTab");
                let item = items_tab.read(|e| match id {
                    Some(i) => e.item_by_id(*i),
                    // :2139 the row keeps the text only; the item of the requirement is one of the build's
                    None => {
                        let text = row.cells.get(key)?;
                        e.item_order().into_iter().filter_map(|i| e.item_by_id(i)).find(|i| {
                            crate::colors::color_code(&i.rarity).is_some_and(|c| format!("{c}{}", i.name) == *text)
                        })
                    }
                });
                let mut t = self.tooltip.borrow_mut();
                t.clear(false);
                if let Some(item) = item {
                    items_tab.add_tooltip(&mut t, f, &item, None, false);
                }
                t.draw(f, cx, row_y, Some((cw, 12.0)), view_port);
            }
            BdHover::Node(Some((nx, ny))) => self.draw_node_viewer(f, view_port, (cx, cw, row_y), (*nx, *ny)),
            BdHover::Node(None) => {}
        }
        sg::set_draw_layer(f, None, 10);
    }

    /// :632-649
    fn draw_node_viewer(&self, f: &mut Frame<'_>, view_port: &Rect, cell: (f64, f64, f64), node: (f64, f64)) {
        let (cx, cw, row_y) = cell;
        let mut viewer_x = cx + cw + 5.0;
        if view_port.x + view_port.width < viewer_x + 304.0 {
            viewer_x = cx - 309.0;
        }
        let viewer_y = row_y.min(view_port.y + view_port.height - 304.0);
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_rect(f, viewer_x, viewer_y, 304.0, 304.0);
        let root = crate::launch::launch().cfg.script_path.clone();
        let tab = self.tab();
        let b = tab.build();
        let mut nv = self.node_viewer.borrow_mut();
        let viewer = nv.get_or_insert_with(|| PassiveTreeView::new(&root));
        viewer.zoom = 5.0;
        let spec = b.engine.borrow().as_ref().expect("engine").spec_rc();
        let scale = viewer.art(&root, &spec.tree).size / 1500.0;
        viewer.zoom_x = -node.0 / scale;
        viewer.zoom_y = -node.1 / scale;
        sg::set_viewport(f, Some([viewer_x + 2.0, viewer_y + 2.0, 300.0, 300.0]));
        let mut none = Vec::new();
        let vp = Rect::new(0.0, 0.0, 300.0, 300.0);
        viewer.draw(f, &root, &b.engine, &b.build_flag, &b.mod_flag, &vp, &mut none);
        sg::set_draw_layer(f, None, 30);
        sg::set_draw_color(f, 1.0, 0.0, 0.0);
        sg::draw_image(f, Some(viewer.highlight_ring()), 135.0, 135.0, 30.0, 30.0, None);
        sg::set_viewport(f, None);
    }

    /// :663-696 `DrawRadiusVisual`.
    fn draw_radius_visual(&self, f: &mut Frame<'_>, r: [f64; 4], radius: f64) {
        let [x, y, w, h] = r;
        let root = crate::launch::launch().cfg.script_path.clone();
        let mut art = self.art.borrow_mut();
        let (guide, overlay) = art.get_or_insert_with(|| {
            let load = |p: &str| {
                let img = f.new_image_handle();
                let _ = img.load(format!("{root}{p}"), &[]);
                img
            };
            (load("Assets/range_guide.png"), load("Assets/game_ui_small.png"))
        });
        sg::set_draw_color(f, 0.75, 0.75, 0.75);
        sg::draw_image(f, Some(guide), x, y, w, h, None);
        sg::set_draw_color_a(f, 0.5, 1.0, 0.5, 0.33);
        render_circle(f, x, y, w, h, 0.0, 0.0, radius);
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
        sg::draw_image(f, Some(overlay), x, y, w, h, None);
    }

    /// :698-761 `Draw` (also called by the overlay pane, CalcSectionControl.lua:429).
    pub fn draw_impl(&self, f: &mut Frame<'_>, view_port: &Rect) {
        let source = self.source.borrow().clone().expect("sourceData");
        let scroll_bar = &self.scroll_bar;
        let (mut w, mut h) = (self.content_width.get(), self.content_height.get());
        let bt = self.border_thickness;
        if h > view_port.height {
            w += scroll_bar.base.width.get(f);
            h = view_port.height;
            scroll_bar.base.height.set(h - bt * 2.0);
            scroll_bar.set_content_dimension(f, self.content_height.get() - bt * 2.0, view_port.height - bt * 2.0);
        } else {
            scroll_bar.set_content_dimension(f, 0.0, 0.0);
        }
        self.base.width.set(w);
        self.base.height.set(h);
        let (sx, sy, sw) = self.source_rect(&source);
        let mut x = sx + sw + 5.0;
        let top = sy.min(view_port.y + view_port.height - h);
        if x + w > view_port.x + view_port.width {
            x = view_port.x.max(sx - 5.0 - w);
        }
        self.base.x.set(x);
        self.base.y.set(top);
        sg::set_draw_layer(f, None, 10);
        sg::set_draw_color_a(f, 0.0, 0.0, 0.0, 0.9);
        sg::draw_rect(f, x + bt, top + bt, w - bt * 2.0, h - bt * 2.0);
        sg::set_draw_layer(f, None, 11);
        let [r, g, b] = if self.pinned.get() {
            self.pinned_colour.get()
        } else {
            self.border_colour.get()
        };
        sg::set_draw_color(f, r, g, b);
        sg::draw_rect(f, x, top, w, bt);
        sg::draw_rect(f, x, top + h - bt, w, bt);
        sg::draw_rect(f, x, top, bt, h);
        sg::draw_rect(f, x + w - bt, top, bt, h);
        sg::set_draw_layer(f, None, 10);
        self.host.draw_controls(f, view_port, None);
        let y = top - scroll_bar.offset.get();
        let st = crate::modules::build::BuildMode::display_settings();
        let list = self.section_list.borrow();
        for s in list.iter() {
            let section_y = y + s.offset;
            match &s.data {
                BdSection::Text { lines, text_size } => {
                    let mut line_y = section_y + 2.0;
                    for line in lines {
                        sg::set_draw_color(f, 1.0, 1.0, 1.0);
                        sg::draw_string(f, x + 4.0, line_y, Some("LEFT"), *text_size, "VAR", &format_num_sep(line, &st));
                        line_y += text_size;
                    }
                }
                BdSection::Table { .. } => self.draw_table(f, view_port, x, section_y, s),
                BdSection::Radius { radius } => {
                    sg::set_draw_color(f, 1.0, 1.0, 1.0);
                    sg::draw_rect(f, x + 2.0, section_y, s.width - 4.0, s.height);
                    self.draw_radius_visual(f, [x + 4.0, section_y + 2.0, s.width - 8.0, s.height - 4.0], *radius);
                }
            }
        }
        sg::set_draw_layer(f, None, 0);
    }

    /// The overlap order of the oracle's process for the node viewer (R27).
    pub fn set_viewer_order_sample(&self, ids: Vec<u32>, connectors: Vec<String>) {
        let root = crate::launch::launch().cfg.script_path.clone();
        let mut nv = self.node_viewer.borrow_mut();
        nv.get_or_insert_with(|| PassiveTreeView::new(&root))
            .set_order_sample(ids, connectors);
    }

    /// `cursorBd` of the gate: inside the first table row of that `source`.
    pub fn debug_cell_pos(&self, source: &str, key: &str) -> (f64, f64) {
        for s in self.section_list.borrow().iter() {
            let BdSection::Table { label, cols, rows, .. } = &s.data else {
                continue;
            };
            let Some(ri) = rows.iter().position(|r| r.cells.get("source").is_some_and(|v| v == source)) else {
                continue;
            };
            let top = self.base.y.raw().expect("y") - self.scroll_bar.offset.get() + s.offset;
            let y = top + if label.is_some() { 16.0 } else { 0.0 } + 20.0 + 14.0 * ri as f64 + 5.0;
            let mut x = self.base.x.raw().expect("x") + 4.0;
            for (i, c) in cols.iter().enumerate() {
                if c.key == key {
                    break;
                }
                x += s.col_width[i].unwrap_or(0.0);
            }
            return (x + 3.0, y);
        }
        panic!("cursorBd: no row of {source}");
    }

    /// `debug`: the state the gate compares.
    /// The lines of the tooltip of a cell.
    pub fn debug_tooltip(&self) -> Vec<String> {
        self.tooltip.borrow().lines.iter().map(|l| l.text.clone().unwrap_or_else(|| "<sep>".to_string())).collect()
    }

    pub fn debug_dump(&self, g: &Gfx) -> serde_json::Value {
        let (x, y) = self.get_pos(g);
        let (w, h) = self.get_size(g);
        let secs: Vec<serde_json::Value> = self
            .section_list
            .borrow()
            .iter()
            .map(|s| match &s.data {
                BdSection::Text { lines, text_size } => serde_json::json!({ "type": "TEXT", "lines": lines, "textSize": text_size, "width": s.width, "height": s.height, "offset": s.offset }),
                BdSection::Radius { radius } => serde_json::json!({ "type": "RADIUS", "radius": radius, "width": s.width, "height": s.height, "offset": s.offset }),
                BdSection::Table { label, footer, rows, cols } => {
                    let c: Vec<_> = cols
                        .iter()
                        .enumerate()
                        .map(|(i, c)| serde_json::json!({ "label": c.label, "key": c.key, "right": c.right, "width": s.col_width[i] }))
                        .collect();
                    let r: Vec<_> = rows
                        .iter()
                        .map(|r| {
                            cols.iter()
                                .map(|c| r.cells.get(&c.key).cloned().map_or(serde_json::Value::Null, serde_json::Value::String))
                                .collect::<Vec<_>>()
                        })
                        .collect();
                    serde_json::json!({ "type": "TABLE", "label": label, "footer": footer, "cols": c, "rows": r, "width": s.width, "height": s.height, "offset": s.offset })
                }
            })
            .collect();
        let shown = self.base.shown.get(g);
        let mut d = serde_json::json!({
            "shown": shown,
            "pinned": self.pinned.get(),
            "scroll": self.scroll_bar.offset.get(),
            "sections": if shown { secs } else { Vec::new() },
        });
        if shown {
            d["rect"] = format!(
                "{},{},{},{}",
                crate::lua::tostring_num(x),
                crate::lua::tostring_num(y),
                crate::lua::tostring_num(w),
                crate::lua::tostring_num(h)
            )
            .into();
            d["contentWidth"] = self.content_width.get().into();
            d["contentHeight"] = self.content_height.get().into();
        }
        d
    }
}

/// Main.lua:1527-1536 `WorldToScreen`.
fn world_to_screen(x: f64, y: f64, width: f64, height: f64) -> (f64, f64) {
    let cos45 = (std::f64::consts::PI / 4.0).cos();
    let cos35 = (std::f64::consts::PI * 0.195).cos();
    let sin35 = (std::f64::consts::PI * 0.195).sin();
    let cx = (x - y) * cos45;
    let cy = -5.33 - (y + x) * cos45 * cos35;
    let cz = 122.0 + (y + x) * cos45 * sin35;
    let sx = width * 0.5 + cx / cz * 1.27 * height;
    let sy = height * 0.5 + cy / cz * 1.27 * height;
    ((sx + 0.5).floor(), (sy + 0.5).floor())
}

/// Main.lua:1539-1560 `RenderCircle`.
#[allow(clippy::too_many_arguments)]
fn render_circle(f: &mut Frame<'_>, x: f64, y: f64, width: f64, height: f64, ox: f64, oy: f64, radius: f64) {
    let mut spans: std::collections::HashMap<i64, (f64, f64)> = std::collections::HashMap::new();
    let (mut min_y, mut max_y) = (height, 0.0f64);
    let mut d = 0.0f64;
    while d <= 360.0 {
        let r = d / 180.0 * std::f64::consts::PI;
        let (px, py) = world_to_screen(ox + r.sin() * radius, oy + r.cos() * radius, width, height);
        if py >= 0.0 && py < height {
            let px = width.min(px.max(0.0));
            min_y = min_y.min(py);
            max_y = max_y.max(py);
            let e = spans.entry(py as i64).or_insert((px, px));
            e.0 = e.0.min(px);
            e.1 = e.1.max(px);
        }
        d += 0.15;
    }
    let mut ly = min_y;
    while ly <= max_y {
        if let Some((a, b)) = spans.get(&(ly as i64)) {
            sg::draw_rect(f, x + a, y + ly, b - a + 1.0, 1.0);
        }
        ly += 1.0;
    }
}

impl Control for CalcBreakdownControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn has_draw(&self) -> bool {
        true
    }
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, _no_tooltip: bool) {
        self.draw_impl(f, view_port);
    }
    /// :42-47
    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        if !self.is_shown(g) {
            return Some(false);
        }
        Some(self.is_mouse_in_bounds(g) || self.host.get_mouse_over_control(g).is_some())
    }
    fn has_on_key_down(&self) -> bool {
        true
    }
    /// :763-785
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, _double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if let Some(m) = self.host.get_mouse_over_control(g)
            && m.has_on_key_down()
        {
            return m.on_key_down(g, key, false);
        }
        let m_over = self.is_mouse_over(g) == Some(true);
        if key.contains("BUTTON") && !m_over {
            self.clear_display(g);
            self.base.shown.set(false);
            return None;
        }
        Some(self.me.upgrade().expect("breakdown"))
    }
    fn has_on_key_up(&self) -> bool {
        true
    }
    /// :787-797
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if ScrollBarControl::is_scroll_down_key(key) {
            self.scroll_bar.scroll(1.0);
        } else if ScrollBarControl::is_scroll_up_key(key) {
            self.scroll_bar.scroll(-1.0);
        }
        Some(self.me.upgrade().expect("breakdown"))
    }
}
