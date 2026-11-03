//! `DropDownControl.lua`: `Control` + `ControlHost` + `TooltipHost` + `SearchHost`.

use crate::common::{disable_scroll_control_interaction, draw_arrow};
use crate::control::{Anchor, Control, ControlBase, ControlRef, Rect, RectProps, anchor_weak};
use crate::controls::scroll_bar::ScrollBarControl;
use crate::host::ControlHost;
use crate::search::SearchHost;
use crate::sg;
use crate::tooltip::TooltipHost;
use pob_gfx::{Frame, Gfx};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// A list entry: a string, or a table with `label` / `detail` / `searchFilter` (+ caller data).
#[derive(Clone, Default)]
pub struct DropItem {
    pub is_table: bool,
    pub label: Option<String>,
    pub detail: Option<String>,
    pub search_filter: Option<String>,
    pub data: Option<Rc<dyn Any>>,
}

impl DropItem {
    pub fn str(s: &str) -> Self {
        Self {
            label: Some(s.to_string()),
            ..Self::default()
        }
    }

    pub fn table(label: &str) -> Self {
        Self {
            is_table: true,
            label: Some(label.to_string()),
            ..Self::default()
        }
    }

    fn label_or_err(&self) -> &str {
        self.label
            .as_deref()
            .expect("bad argument to DrawString (string expected, got nil)")
    }
}

/// `tooltipFunc(tooltip, mode, index, value)`: mode `"BODY"` / `"OUT"` / `"HOVER"`.
pub type DropTooltipArgs = (&'static str, usize, Option<DropItem>);

pub struct DropDownControl {
    pub base: ControlBase,
    pub host: ControlHost,
    pub tip: TooltipHost<DropTooltipArgs>,
    pub search: SearchHost,
    pub scroll_bar: Rc<ScrollBarControl>,
    pub list: RefCell<Vec<DropItem>>,
    pub sel_index: Cell<usize>,
    #[allow(clippy::type_complexity)]
    pub sel_func: RefCell<Option<Rc<dyn Fn(&mut Gfx, usize, &DropItem)>>>,
    pub drop_height: Cell<f64>,
    pub dropped: Cell<bool>,
    pub drop_up: Cell<bool>,
    pub dropped_width: Cell<f64>,
    pub max_dropped_width: Cell<f64>,
    pub enable_dropped_width: Cell<bool>,
    pub enable_change_box_width: Cell<bool>,
    pub hover_sel_drop: Cell<Option<i64>>,
    pub hover_sel: Cell<Option<i64>>,
    pub border_func: RefCell<Option<Rc<dyn Fn() -> (f64, f64, f64)>>>,
    pub other_drag_source: Cell<bool>,
    pub imbued_select: Cell<bool>,
    /// What a subclass draws after `DropDownControl.Draw` (TimelessJewelSocketControl.lua:25-58).
    #[allow(clippy::type_complexity)]
    pub post_draw: RefCell<Option<Rc<dyn Fn(&mut Frame<'_>, &Rect, bool)>>>,
}

impl DropDownControl {
    /// `DropDownControl:DropDownControl(anchor, rect, list, selFunc, tooltipText, ignoreSearchOrder)`
    /// (DropDownControl.lua:14-58).
    #[allow(clippy::type_complexity)]
    pub fn new(
        g: &Gfx,
        anchor: Option<Anchor>,
        rect: RectProps,
        list: Vec<DropItem>,
        sel_func: Option<Rc<dyn Fn(&mut Gfx, usize, &DropItem)>>,
        tooltip_text: Option<&str>,
        ignore_search_order: bool,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| {
            let base = ControlBase::new(anchor, rect);
            let host = ControlHost::new();
            let m = me.clone();
            let values: Rc<dyn Fn() -> Vec<String>> = Rc::new(move || {
                let d = m.upgrade().expect("dropdown");
                // valueAccessor: searchFilter, then label (tables), else the string; StripEscapes'd
                d.list
                    .borrow()
                    .iter()
                    .map(|v| {
                        let s = if v.is_table {
                            v.search_filter.as_deref().or(v.label.as_deref())
                        } else {
                            v.label.as_deref()
                        };
                        pob_gfx::text::strip_escapes(s.expect("StripEscapes of a table"))
                    })
                    .collect()
            });
            let search = SearchHost::new(Some(values), ignore_search_order);
            let height = base.height.raw().expect("DropDownControl height must be a number");
            let width = base.width.raw().expect("DropDownControl width must be a number");
            let me_dyn: Weak<dyn Control> = me.clone();
            let scroll_bar = ScrollBarControl::new(
                anchor_weak("TOPRIGHT", me_dyn, "TOPRIGHT"),
                [(-1.0).into(), 0.0.into(), 18.0.into(), 0.0.into()],
                Some((height - 4.0) * 4.0),
                None,
                false,
            );
            let m = me.clone();
            scroll_bar
                .base
                .height
                .set_fn(move |_| m.upgrade().expect("dropdown").drop_height.get() + 2.0);
            let m = me.clone();
            scroll_bar.base.shown.set_fn(move |g| {
                let d = m.upgrade().expect("dropdown");
                d.dropped.get() && d.scroll_bar.base.enabled.get(g)
            });
            host.add("scrollBar", scroll_bar.clone());
            let d = Self {
                base,
                host,
                tip: TooltipHost::new(tooltip_text.map(str::to_string)),
                search,
                scroll_bar,
                list: RefCell::new(Vec::new()),
                sel_index: Cell::new(1),
                sel_func: RefCell::new(None),
                drop_height: Cell::new(0.0),
                dropped: Cell::new(false),
                drop_up: Cell::new(false),
                dropped_width: Cell::new(width),
                max_dropped_width: Cell::new(width.max(300.0)),
                enable_dropped_width: Cell::new(false),
                enable_change_box_width: Cell::new(false),
                hover_sel_drop: Cell::new(None),
                hover_sel: Cell::new(None),
                border_func: RefCell::new(None),
                other_drag_source: Cell::new(false),
                imbued_select: Cell::new(false),
                post_draw: RefCell::new(None),
            };
            d.set_list(g, Some(list));
            d.sel_index.set(1);
            *d.sel_func.borrow_mut() = sel_func;
            // `droppedWidth = self.width` / `maxDroppedWidth` are re-set after SetList (DropDownControl.lua:49-51)
            d.dropped_width.set(width);
            d.max_dropped_width.set(width.max(300.0));
            d.enable_dropped_width.set(false);
            d.enable_change_box_width.set(false);
            d
        })
    }

    fn height_raw(&self) -> f64 {
        self.base.height.raw().expect("DropDownControl height must be a number")
    }

    /// DropDownControl.lua:61-79
    pub fn drop_index_to_list_index(&self, drop_index: Option<i64>) -> Option<i64> {
        if !self.search.is_search_active() {
            return drop_index;
        }
        let mut drop_index = drop_index?;
        if drop_index <= 0 || drop_index > self.get_drop_count() as i64 {
            return None;
        }
        for (i, info) in self.search.search_infos.borrow().iter().enumerate() {
            if info.matches {
                drop_index -= 1;
                if drop_index <= 0 {
                    return Some(i as i64 + 1);
                }
            }
        }
        None
    }

    /// DropDownControl.lua:82-103
    pub fn list_index_to_drop_index(&self, list_index: i64, default: Option<i64>) -> Option<i64> {
        if !self.search.is_search_active() {
            return Some(list_index);
        }
        if list_index <= 0 || list_index > self.list.borrow().len() as i64 {
            return None;
        }
        let mut drop_index = 0;
        for (i, info) in self.search.search_infos.borrow().iter().enumerate() {
            if info.matches {
                drop_index += 1;
                if list_index == i as i64 + 1 {
                    return Some(drop_index);
                }
            }
        }
        default
    }

    /// DropDownControl.lua:105-111
    pub fn get_drop_count(&self) -> usize {
        if self.search.is_search_active() {
            self.search.get_match_count()
        } else {
            self.list.borrow().len()
        }
    }

    /// DropDownControl.lua:113-133
    fn draw_search_highlights(
        &self,
        f: &mut Frame<'_>,
        label: &str,
        info: Option<&crate::search::SearchInfo>,
        x: f64,
        y: f64,
        height: f64,
    ) {
        let Some(info) = info.filter(|i| i.matches) else { return };
        let mut start_x;
        let mut end_x = 0.0;
        let mut last = 0usize;
        sg::set_draw_color_a(f, 1.0, 1.0, 0.0, 0.2);
        let stripped = f.strip_escapes(label);
        for r in &info.ranges {
            if r.from as i64 - last as i64 - 1 > 0 {
                start_x =
                    sg::draw_string_width(f, height, "VAR", &crate::lua::sub(&stripped, last as i64 + 1, r.from as i64 - 1)) + x + end_x;
            } else {
                start_x = end_x;
            }
            end_x = sg::draw_string_width(f, height, "VAR", &crate::lua::sub(&stripped, r.from as i64, r.to as i64)) + x + start_x;
            last = r.to;
            sg::draw_rect(f, start_x, y, end_x - start_x, height);
        }
        sg::set_draw_color(f, 1.0, 1.0, 1.0);
    }

    /// DropDownControl.lua:136-150 (string entries compare with `label`, tables by `key` — here `"label"` only).
    pub fn sel_by_label(&self, value: &str) {
        let idx = self.list.borrow().iter().position(|v| v.label.as_deref() == Some(value));
        if let Some(i) = idx {
            self.sel_index.set(i + 1);
        }
    }

    /// DropDownControl.lua:156-158
    pub fn get_sel_value(&self) -> Option<DropItem> {
        self.list.borrow().get(self.sel_index.get().wrapping_sub(1)).cloned()
    }

    /// DropDownControl.lua:160-169
    pub fn set_sel(&self, g: &mut Gfx, new_sel: i64, no_call_sel_func: bool) {
        let new_sel = 1.max((self.get_drop_count() as i64).min(new_sel));
        let Some(new_sel) = self.drop_index_to_list_index(Some(new_sel)) else {
            return;
        };
        if new_sel as usize != self.sel_index.get() {
            self.sel_index.set(new_sel as usize);
            let sf = self.sel_func.borrow().clone();
            if !no_call_sel_func && let Some(sf) = sf {
                let item = self.list.borrow().get(new_sel as usize - 1).cloned().unwrap_or_default();
                sf(g, new_sel as usize, &item);
            }
        }
    }

    /// DropDownControl.lua:171-176
    pub fn scroll_sel_into_view(&self, g: &Gfx) {
        let (_, height) = self.get_size(g);
        let sb = &self.scroll_bar;
        sb.set_content_dimension(g, (height - 4.0) * self.get_drop_count() as f64, self.drop_height.get());
        let di = self
            .list_index_to_drop_index(self.sel_index.get() as i64, Some(1))
            .expect("ListIndexToDropIndex");
        sb.scroll_into_view((di - 2) as f64 * (height - 4.0), 3.0 * (height - 4.0));
    }

    /// `DropDownControl:IsMouseOver()` → `mOver, mOverComp` (DropDownControl.lua:178-210).
    pub fn mouse_over(&self, g: &Gfx) -> (bool, Option<&'static str>) {
        if !self.is_shown(g) {
            return (false, None);
        }
        if self.host.get_mouse_over_control(g).is_some() {
            return (true, None);
        }
        let (x, y) = self.get_pos(g);
        let (mut width, height) = self.get_size(g);
        let (cursor_x, cursor_y) = g.get_cursor_pos();
        let drop_extra = if self.dropped.get() { self.drop_height.get() + 2.0 } else { 0.0 };
        let m_over = if self.dropped.get() {
            width = width.max(self.dropped_width.get());
            if self.drop_up.get() {
                cursor_x >= x && cursor_y >= y - drop_extra && cursor_x < x + width && cursor_y < y + height
            } else {
                cursor_x >= x && cursor_y >= y && cursor_x < x + width && cursor_y < y + height + drop_extra
            }
        } else {
            cursor_x >= x && cursor_y >= y && cursor_x < x + width && cursor_y < y + height
        };
        let comp = m_over.then_some(if cursor_y >= y && cursor_y < y + height { "BODY" } else { "DROP" });
        (m_over, comp)
    }

    /// DropDownControl.lua:528-535
    pub fn set_list(&self, g: &Gfx, list: Option<Vec<DropItem>>) {
        if let Some(l) = list {
            *self.list.borrow_mut() = l;
            self.check_dropped_width(g, self.enable_dropped_width.get());
        }
    }

    /// DropDownControl.lua:524-526
    pub fn get_hover_index(&self) -> i64 {
        self.hover_sel.get().unwrap_or(self.sel_index.get() as i64)
    }

    /// DropDownControl.lua:537-573
    pub fn check_dropped_width(&self, g: &Gfx, enable: bool) {
        self.enable_dropped_width.set(enable);
        let width = self.base.width.raw().expect("DropDownControl width must be a number");
        if enable {
            let mut scroll_width = 0.0;
            if self.dropped.get() && self.scroll_bar.base.enabled.get(g) {
                scroll_width = self.scroll_bar.base.width.raw().expect("scrollBar width");
            }
            let line_height = self.height_raw() - 4.0;
            let mut d_width = width;
            for line in self.list.borrow().iter() {
                let s = if line.is_table {
                    line.label.clone().unwrap_or_default()
                } else {
                    line.label.clone().expect("string")
                };
                d_width = d_width.max(sg::draw_string_width(g, line_height, "VAR", &s) + 10.0);
            }
            self.dropped_width.set((d_width + scroll_width).min(self.max_dropped_width.get()));
            if self.enable_change_box_width.get() {
                let line = self
                    .list
                    .borrow()
                    .get(self.sel_index.get().wrapping_sub(1))
                    .and_then(|l| l.label.clone());
                let box_width = sg::draw_string_width(g, line_height, "VAR", line.as_deref().unwrap_or("")) + 20.0;
                #[allow(clippy::manual_clamp)] // m_max(m_min(w, 390), 190), NaN behaviour of the original kept
                self.base.width.set(box_width.min(390.0).max(190.0));
            }
            let w = self.base.width.raw().expect("width");
            self.scroll_bar.base.x.set(self.dropped_width.get() - w - 1.0);
        } else {
            self.dropped_width.set(width);
            self.scroll_bar.base.x.set(-1.0);
        }
    }
}

impl Control for DropDownControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn debug_value(&self, _g: &Gfx) -> Option<serde_json::Value> {
        Some(serde_json::json!(self.sel_index.get()))
    }

    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        Some(self.mouse_over(g).0)
    }

    fn imbued_dropped(&self) -> bool {
        self.imbued_select.get() && self.dropped.get()
    }

    fn has_draw(&self) -> bool {
        true
    }

    /// DropDownControl.lua:212-425
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let (width, height) = self.get_size(f);
        let enabled = self.is_enabled(f);
        let sb = Rc::clone(&self.scroll_bar);
        let line_height = height - 4.0;
        let list_len = self.list.borrow().len();
        self.drop_height.set(line_height * (list_len.min(20) as f64));
        sb.base.y.set(height + 1.0);
        if y + height + self.drop_height.get() + 4.0 <= view_port.y + view_port.height {
            self.drop_up.set(false);
        } else {
            let lines_above = ((y - view_port.y - 4.0) / line_height).floor();
            let lines_below = ((view_port.y + view_port.height - y - height - 4.0) / line_height).floor();
            if lines_above > lines_below {
                self.drop_up.set(true);
                if y - view_port.y < self.drop_height.get() + 4.0 {
                    self.drop_height.set(line_height * lines_above);
                }
                sb.base.y.set(-self.drop_height.get() - 3.0);
            } else {
                self.drop_up.set(false);
                self.drop_height.set(line_height * lines_below);
            }
        }
        if self.search.is_search_active() && !self.dropped.get() {
            self.search.reset_search();
        }
        self.drop_height.set(
            self.drop_height
                .get()
                .min(self.get_drop_count() as f64 * line_height)
                .max(line_height),
        );
        let (m_over, m_over_comp) = self.mouse_over(f);
        let drop_extra = self.drop_height.get() + 4.0;
        sb.set_content_dimension(f, line_height * self.get_drop_count() as f64, self.drop_height.get());
        let drop_y = if self.drop_up.get() { y - drop_extra } else { y + height };
        let dropped = self.dropped.get();
        let border = self.border_func.borrow().clone();
        if !enabled {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else if m_over || dropped {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else if let Some(bf) = border {
            let (r, g, b) = bf();
            sg::set_draw_color(f, r, g, b);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        sg::draw_rect(f, x, y, width, height);
        if dropped {
            sg::set_draw_layer(f, None, 5);
            sg::draw_rect(f, x, drop_y, self.dropped_width.get(), drop_extra);
            sg::set_draw_layer(f, None, 0);
        }
        if !enabled || dropped {
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
        } else if m_over {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else {
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
        }
        sg::draw_rect(f, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        if !enabled {
            sg::set_draw_color(f, 0.33, 0.33, 0.33);
        } else if m_over || dropped {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        draw_arrow(f, x + width - height / 2.0, y + height / 2.0, height / 2.0, height / 2.0, "DOWN");
        if dropped {
            sg::set_draw_layer(f, None, 5);
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
            sg::draw_rect(f, x + 1.0, drop_y + 1.0, self.dropped_width.get() - 2.0, drop_extra - 2.0);
            sg::set_draw_layer(f, None, 0);
        }
        if self.other_drag_source.get() {
            sg::set_draw_color_a(f, 0.0, 1.0, 0.0, 0.25);
            sg::draw_rect(f, x, y, width, height);
        }
        if enabled {
            if (m_over || dropped) && m_over_comp != Some("DROP") && !no_tooltip {
                sg::set_draw_layer(f, None, 100);
                let sel = self.sel_index.get();
                let item = self.list.borrow().get(sel.wrapping_sub(1)).cloned();
                self.tip.draw_tooltip(
                    f,
                    x,
                    y - if dropped && self.drop_up.get() { drop_extra } else { 0.0 },
                    width,
                    height + if dropped { drop_extra } else { 0.0 },
                    view_port,
                    (if m_over { "BODY" } else { "OUT" }, sel, item),
                );
                sg::set_draw_layer(f, None, 0);
            }
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.66, 0.66, 0.66);
        }
        let (sel_label, sel_detail) = if self.search.is_search_active() {
            (Some(format!("Search: {}", self.search.get_search_term_pretty())), None)
        } else {
            match self.list.borrow().get(self.sel_index.get().wrapping_sub(1)) {
                Some(item) => (item.label.clone(), if item.is_table { item.detail.clone() } else { None }),
                None => (None, None),
            }
        };
        sg::set_viewport(f, Some([x + 2.0, y + 2.0, width - height, line_height]));
        sg::draw_string(f, 0.0, 0.0, Some("LEFT"), line_height, "VAR", sel_label.as_deref().unwrap_or(""));
        if let Some(detail) = &sel_detail {
            let dx = sg::draw_string_width(f, line_height, "VAR", detail);
            if !enabled || dropped {
                sg::set_draw_color(f, 0.0, 0.0, 0.0);
            } else if m_over {
                sg::set_draw_color(f, 0.33, 0.33, 0.33);
            } else {
                sg::set_draw_color(f, 0.0, 0.0, 0.0);
            }
            sg::draw_rect(f, width - dx - 4.0 - 22.0, 0.0, width - 4.0, line_height);
            if enabled {
                sg::set_draw_color(f, 1.0, 1.0, 1.0);
            } else {
                sg::set_draw_color(f, 0.66, 0.66, 0.66);
            }
            sg::draw_string(f, width - dx - 22.0, 0.0, Some("LEFT"), line_height, "VAR", detail);
        }
        sg::set_viewport(f, None);
        if dropped {
            sg::set_draw_layer(f, None, 5);
            self.host.draw_controls(f, view_port, None);
            let width = self.dropped_width.get();
            let (_, cursor_y) = f.get_cursor_pos();
            let sb_over = sb.is_mouse_over(f) == Some(true);
            let hover_drop = (m_over && !sb_over).then(|| ((cursor_y - drop_y + sb.offset.get()) / line_height).floor() as i64 + 1);
            self.hover_sel_drop.set(hover_drop);
            let mut hover = self.drop_index_to_list_index(hover_drop);
            if let Some(h) = hover
                && (h < 1 || h as usize > self.list.borrow().len())
            {
                hover = None;
            }
            self.hover_sel.set(hover);
            if let Some(h) = hover
                && !no_tooltip
            {
                sg::set_draw_layer(f, None, 100);
                let item = self.list.borrow().get(h as usize - 1).cloned();
                let hd = hover_drop.expect("hoverSelDrop");
                self.tip.draw_tooltip(
                    f,
                    x,
                    drop_y + 2.0 + (hd - 1) as f64 * line_height - sb.offset.get(),
                    width,
                    line_height,
                    view_port,
                    ("HOVER", h as usize, item),
                );
                sg::set_draw_layer(f, None, 5);
            }
            let sb_enabled = sb.base.enabled.get(f);
            sg::set_viewport(
                f,
                Some([
                    x + 2.0,
                    drop_y + 2.0,
                    if sb_enabled { width - 22.0 } else { width - 4.0 },
                    self.drop_height.get(),
                ]),
            );
            let mut drop_index = 0;
            let list = self.list.borrow().clone();
            let infos = self.search.search_infos.borrow().clone();
            let active = self.search.is_search_active();
            for (i, item) in list.iter().enumerate() {
                let index = i as i64 + 1;
                let info = infos.get(i);
                if !active || info.is_some_and(|i| i.matches) {
                    drop_index += 1;
                    let y = (drop_index - 1) as f64 * line_height - sb.offset.get();
                    if Some(index) == hover {
                        sg::set_draw_color(f, 0.33, 0.33, 0.33);
                        sg::draw_rect(f, 0.0, y, width - 4.0, line_height);
                    }
                    let hot = Some(index) == hover || index as usize == self.sel_index.get();
                    if hot {
                        sg::set_draw_color(f, 1.0, 1.0, 1.0);
                    } else {
                        sg::set_draw_color(f, 0.66, 0.66, 0.66);
                    }
                    let label = item.label_or_err();
                    sg::draw_string(f, 0.0, y, Some("LEFT"), line_height, "VAR", label);
                    if item.is_table
                        && let Some(detail) = &item.detail
                    {
                        let dx = sg::draw_string_width(f, line_height, "VAR", detail);
                        if Some(index) == hover {
                            sg::set_draw_color(f, 0.33, 0.33, 0.33);
                        } else {
                            sg::set_draw_color(f, 0.0, 0.0, 0.0);
                        }
                        sg::draw_rect(f, width - dx - 8.0 - 22.0, y, width - 4.0, line_height);
                        if hot {
                            sg::set_draw_color(f, 1.0, 1.0, 1.0);
                        } else {
                            sg::set_draw_color(f, 0.66, 0.66, 0.66);
                        }
                        sg::draw_string(f, width - dx - 4.0 - 22.0, y, Some("LEFT"), line_height, "VAR", detail);
                    }
                    self.draw_search_highlights(f, label, info, 0.0, y, line_height);
                }
            }
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            if active && self.search.get_match_count() == 0 {
                sg::draw_string(f, 0.0, 0.0, Some("LEFT"), line_height, "VAR", "<No matches>");
            }
            sg::set_viewport(f, None);
            sg::set_draw_layer(f, None, 0);
        }
        let post = self.post_draw.borrow().clone();
        if let Some(post) = post {
            post(f, view_port, no_tooltip);
        }
    }

    fn has_on_char(&self) -> bool {
        true
    }

    /// DropDownControl.lua:427-432
    fn on_char(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) || !self.dropped.get() {
            return None;
        }
        self.search.on_search_char(key);
        Some(self)
    }

    fn has_on_key_down(&self) -> bool {
        true
    }

    /// DropDownControl.lua:434-464
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, _double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if self.dropped.get() && self.search.on_search_key_down(key) {
            return Some(self);
        }
        let m_over_control = self.host.get_mouse_over_control(g);
        if let Some(m) = m_over_control
            && m.has_on_key_down()
        {
            *self.host.sel_control.borrow_mut() = Some(Rc::clone(&m));
            return m.on_key_down(g, key, false).map(|_| self as ControlRef);
        }
        *self.host.sel_control.borrow_mut() = None;
        if key == "LEFTBUTTON" || key == "RIGHTBUTTON" {
            let (m_over, comp) = self.mouse_over(g);
            if !m_over || (self.dropped.get() && comp == Some("BODY")) {
                self.dropped.set(false);
                return Some(self);
            }
            if !self.dropped.get() {
                self.dropped.set(true);
                self.scroll_sel_into_view(g);
            }
        } else if key == "ESCAPE" {
            self.dropped.set(false);
        }
        if self.dropped.get() { Some(self) } else { None }
    }

    fn has_on_key_up(&self) -> bool {
        true
    }

    /// DropDownControl.lua:466-522
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if let Some(sel) = self.host.sel() {
            if sel.on_key_up(g, key).is_none() {
                *self.host.sel_control.borrow_mut() = None;
            }
            return Some(self);
        }
        let sel_drop = |s: &Self| {
            s.list_index_to_drop_index(s.sel_index.get() as i64, Some(0))
                .expect("ListIndexToDropIndex")
        };
        if key == "LEFTBUTTON" || key == "RIGHTBUTTON" {
            let (m_over, comp) = self.mouse_over(g);
            if !m_over {
                self.dropped.set(false);
            } else if comp == Some("DROP") {
                let (_, y) = self.get_pos(g);
                let (_, height) = self.get_size(g);
                let (_, cursor_y) = g.get_cursor_pos();
                let drop_extra = self.drop_height.get() + 4.0;
                let drop_y = if self.drop_up.get() { y - drop_extra } else { y + height };
                let idx = ((cursor_y - drop_y + self.scroll_bar.offset.get()) / (height - 4.0)).floor() as i64 + 1;
                self.set_sel(g, idx, false);
                self.dropped.set(false);
            }
        } else if ScrollBarControl::is_scroll_down_key(key) {
            if self.dropped.get() && self.scroll_bar.base.enabled.get(g) {
                self.scroll_bar.scroll(1.0);
            } else {
                if disable_scroll_control_interaction() && key == "WHEELDOWN" {
                    return None;
                }
                let d = sel_drop(&self);
                self.set_sel(g, d + 1, false);
            }
            return Some(self);
        } else if key == "DOWN" {
            let d = sel_drop(&self);
            self.set_sel(g, d + 1, false);
            self.scroll_sel_into_view(g);
            return Some(self);
        } else if ScrollBarControl::is_scroll_up_key(key) {
            if self.dropped.get() && self.scroll_bar.base.enabled.get(g) {
                self.scroll_bar.scroll(-1.0);
            } else {
                if disable_scroll_control_interaction() && key == "WHEELUP" {
                    return None;
                }
                let d = sel_drop(&self);
                self.set_sel(g, d - 1, false);
            }
            return Some(self);
        } else if key == "UP" {
            let d = sel_drop(&self);
            self.set_sel(g, d - 1, false);
            self.scroll_sel_into_view(g);
            return Some(self);
        }
        if self.dropped.get() { Some(self) } else { None }
    }
}
