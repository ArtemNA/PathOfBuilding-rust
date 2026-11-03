//! `ListControl.lua` (abstract list; "derived classes" supply [`ListHooks`]) and `TextListControl.lua`.
//!
//! List values are compared with `==` like Lua (`value == self.selValue`): for table values use a type whose
//! `PartialEq` is identity (e.g. [`ByRef`]). Dragging to other controls (`dragTargetList` → [`DragTarget`],
//! `otherDragSource`, `ReceiveDrag`, `main.showDragText`) and reordering inside a mutable list are ported (U2b).

use crate::control::{Anchor, Control, ControlBase, ControlRef, Prop, PropCell, Rect, RectProps, anchor_weak};
use crate::controls::scroll_bar::ScrollBarControl;
use crate::host::ControlHost;
use crate::sg;
use crate::tooltip::Tooltip;
use pob_gfx::{Frame, Gfx, ImageHandle};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// A list value compared by identity (a Lua table).
#[derive(Debug)]
pub struct ByRef<T>(pub Rc<T>);

impl<T> Clone for ByRef<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T> PartialEq for ByRef<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// A `colList` entry: `width` / `font` / `label` / `sortable` may be values or functions (`GetColumnProperty`).
#[derive(Default)]
pub struct Column {
    pub width: Option<Prop<f64>>,
    pub font: Option<&'static str>,
    pub label: Option<Prop<String>>,
    pub sortable: bool,
    pub offset: Cell<f64>,
    pub width_now: Cell<f64>,
}

fn prop_get<T: Clone>(p: &Option<Prop<T>>, g: &Gfx) -> Option<T> {
    match p {
        None => None,
        Some(Prop::Val(v)) => Some(v.clone()),
        Some(Prop::Fn(f)) => Some(f(g)),
    }
}

/// The methods a `ListControl` subclass may define (ListControl.lua:6-24). Indices are 1-based.
#[allow(unused_variables)]
pub trait ListHooks<V>: 'static {
    /// `:GetRowValue(column, index, value)` — required.
    fn get_row_value(&self, list: &ListControl<V>, g: &Gfx, column: usize, index: usize, value: &V) -> String;
    fn get_row_icon(&self, list: &ListControl<V>, column: usize, index: usize, value: &V) -> Option<ImageHandle> {
        None
    }
    fn has_add_value_tooltip(&self) -> bool {
        false
    }
    fn add_value_tooltip(&self, list: &ListControl<V>, g: &Gfx, tooltip: &mut Tooltip, index: usize, value: &V) {}
    fn on_select(&self, list: &ListControl<V>, g: &mut Gfx, index: usize, value: &V) {}
    /// `:OnSelClick(index, value, doubleClick)`; `Some(false)` releases the focus.
    fn on_sel_click(&self, list: &ListControl<V>, g: &mut Gfx, index: usize, value: &V, double_click: bool) -> Option<bool> {
        None
    }
    fn on_sel_copy(&self, list: &ListControl<V>, g: &mut Gfx, index: usize, value: &V) {}
    fn on_sel_cut(&self, list: &ListControl<V>, g: &mut Gfx, index: usize, value: &V) {}
    fn on_sel_delete(&self, list: &ListControl<V>, g: &mut Gfx, index: usize, value: &V) {}
    fn on_sel_key_down(&self, list: &ListControl<V>, g: &mut Gfx, index: usize, value: &V, key: &str) {}
    fn on_any_key_down(&self, list: &ListControl<V>, g: &mut Gfx, key: &str) {}
    fn on_order_change(&self, list: &ListControl<V>, old_index: usize, new_index: usize) {}
    fn has_can_drag_to_value(&self) -> bool {
        false
    }
    fn can_drag_to_value(&self, list: &ListControl<V>, index: usize, value: &V) -> bool {
        false
    }
    /// `:SetHighlightColor(index, value)` → `true` when it set the colour.
    fn set_highlight_color(&self, list: &ListControl<V>, f: &mut Frame<'_>, index: usize, value: &V) -> bool {
        false
    }
    fn override_select_index(&self, list: &ListControl<V>, g: &Gfx, index: usize) -> bool {
        false
    }
    fn has_re_sort(&self) -> bool {
        false
    }
    fn re_sort(&self, list: &ListControl<V>, column: usize) {}
    /// `:GetDragValue(index, value)` → `(dragType, dragValue)` (needed when `dragTargetList` is set).
    fn get_drag_value(&self, list: &ListControl<V>, index: usize, value: &V) -> (String, V) {
        unimplemented!("ListControl.lua:162: GetDragValue of this list")
    }
    fn on_drag_send(&self, list: &ListControl<V>, g: &mut Gfx, index: usize, value: &V) {}
    /// This list as a drag target (`self` in its own `dragTargetList`): `CanReceiveDrag` / `ReceiveDrag`.
    fn can_receive_drag(&self, list: &ListControl<V>, drag_type: &str, value: &V) -> bool {
        true
    }
    fn receive_drag(&self, list: &ListControl<V>, g: &mut Gfx, drag_type: &str, value: &V) {}
    /// `:OnHoverKeyUp(key)` (ControlHost.lua:93-95).
    fn has_on_hover_key_up(&self) -> bool {
        false
    }
    fn on_hover_key_up(&self, list: &ListControl<V>, g: &mut Gfx, key: &str) {}
}

/// A member of `dragTargetList`: `CanReceiveDrag` (`nil` = accepts everything), `otherDragSource` (here a flag:
/// one list drags at a time), `otherDragTargeting`, `IsMouseOver`, `ReceiveDrag`.
pub trait DragTarget<V> {
    fn can_receive_drag(&self, drag_type: &str, value: &V) -> bool;
    fn set_other_drag_source(&self, on: bool);
    fn other_drag_source(&self) -> bool;
    fn set_other_drag_targeting(&self, _on: bool) {}
    fn is_mouse_over(&self, g: &Gfx) -> bool;
    fn receive_drag(&self, g: &mut Gfx, drag_type: &str, value: &V);
}

/// The list itself as a member of its own `dragTargetList` (BuildListControl.lua:57).
pub struct SelfTarget<V>(pub Weak<ListControl<V>>);

impl<V: Clone + PartialEq + 'static> DragTarget<V> for SelfTarget<V> {
    fn can_receive_drag(&self, drag_type: &str, value: &V) -> bool {
        let l = self.0.upgrade().expect("list");
        l.hooks.can_receive_drag(&l, drag_type, value)
    }
    fn set_other_drag_source(&self, on: bool) {
        self.0.upgrade().expect("list").other_drag_source.set(on);
    }
    fn other_drag_source(&self) -> bool {
        self.0.upgrade().expect("list").other_drag_source.get()
    }
    fn is_mouse_over(&self, g: &Gfx) -> bool {
        self.0.upgrade().expect("list").mouse_over(g)
    }
    fn receive_drag(&self, g: &mut Gfx, drag_type: &str, value: &V) {
        let l = self.0.upgrade().expect("list");
        l.hooks.receive_drag(&l, g, drag_type, value);
    }
}

pub struct ListControl<V> {
    pub base: ControlBase,
    pub host: ControlHost,
    pub hooks: Box<dyn ListHooks<V>>,
    pub row_height: f64,
    /// `"HORIZONTAL"` / `"VERTICAL"` / true → `Some`, false/nil → `None`.
    pub scroll: Option<&'static str>,
    pub scroll_h: bool,
    pub is_mutable: Cell<bool>,
    /// What a subclass does at the head of its `Draw`, before `ListControl.Draw` (NotableDBControl.lua:193-211).
    #[allow(clippy::type_complexity)]
    pub before_draw: RefCell<Option<Rc<dyn Fn(&Gfx)>>>,
    pub list: RefCell<Vec<V>>,
    pub force_tooltip: Cell<bool>,
    pub col_list: RefCell<Vec<Column>>,
    pub col_labels: Cell<bool>,
    pub tooltip: RefCell<Tooltip>,
    pub font: Cell<&'static str>,
    pub label: PropCell<Option<String>>,
    pub label_position_offset: Cell<(f64, f64)>,
    pub show_row_separators: Cell<bool>,
    pub default_text: RefCell<Option<String>>,
    pub scroll_bar_h: Rc<ScrollBarControl>,
    pub scroll_bar_v: Rc<ScrollBarControl>,
    pub sel_index: Cell<Option<usize>>,
    pub sel_value: RefCell<Option<V>>,
    pub sel_cx: Cell<f64>,
    pub sel_cy: Cell<f64>,
    pub sel_dragging: Cell<bool>,
    pub sel_drag_active: Cell<bool>,
    pub sel_drag_index: Cell<Option<usize>>,
    /// Set by another list dragging onto this one (never set while drag targets are unimplemented).
    pub other_drag_source: Cell<bool>,
    pub hover_index: Cell<Option<usize>>,
    pub hover_value: RefCell<Option<V>>,
    pub drag_target_list: RefCell<Option<Vec<Rc<dyn DragTarget<V>>>>>,
    /// `self.dragType, self.dragValue`.
    pub drag: RefCell<Option<(String, V)>>,
    pub drag_target: RefCell<Option<Rc<dyn DragTarget<V>>>>,
}

impl<V: Clone + PartialEq + 'static> ListControl<V> {
    /// `ListControl:ListControl(anchor, rect, rowHeight, scroll, isMutable, list, forceTooltip)` (ListControl.lua:43-88).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        anchor: Option<Anchor>,
        rect: RectProps,
        row_height: f64,
        scroll: Option<&'static str>,
        is_mutable: bool,
        list: Vec<V>,
        force_tooltip: bool,
        hooks: Box<dyn ListHooks<V>>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| {
            let base = ControlBase::new(anchor, rect);
            let host = ControlHost::new();
            let scroll_h = scroll == Some("HORIZONTAL");
            let me_dyn: Weak<dyn Control> = me.clone();
            let sz = if scroll.is_some() { 16.0 } else { 0.0 };
            let sbh = ScrollBarControl::new(
                anchor_weak("BOTTOM", me_dyn.clone(), "BOTTOM"),
                [(-8.0).into(), (-1.0).into(), 0.0.into(), sz.into()],
                Some(row_height * 2.0),
                Some("HORIZONTAL"),
                false,
            );
            let m = me.clone();
            sbh.base.shown.set_fn(move |_| m.upgrade().expect("list").scroll_h);
            let m = me.clone();
            sbh.base.width.set_fn(move |g| m.upgrade().expect("list").get_size(g).0 - 18.0);
            let sbv = ScrollBarControl::new(
                anchor_weak("RIGHT", me_dyn, "RIGHT"),
                [(-1.0).into(), 0.0.into(), sz.into(), 0.0.into()],
                Some(row_height * 2.0),
                Some("VERTICAL"),
                false,
            );
            let m = me.clone();
            sbv.base
                .y
                .set_fn(move |_| if m.upgrade().expect("list").scroll_h { -8.0 } else { 0.0 });
            let m = me.clone();
            sbv.base.height.set_fn(move |g| {
                let l = m.upgrade().expect("list");
                l.get_size(g).1 - 2.0 - if l.scroll_h { 16.0 } else { 0.0 }
            });
            if scroll.is_none() {
                sbh.base.shown.set(false);
                sbv.base.shown.set(false);
            }
            host.add("scrollBarH", sbh.clone());
            host.add("scrollBarV", sbv.clone());
            Self {
                base,
                host,
                hooks,
                row_height,
                scroll,
                scroll_h,
                is_mutable: Cell::new(is_mutable),
                before_draw: RefCell::new(None),
                list: RefCell::new(list),
                force_tooltip: Cell::new(force_tooltip),
                col_list: RefCell::new(vec![Column::default()]),
                col_labels: Cell::new(false),
                tooltip: RefCell::new(Tooltip::new()),
                font: Cell::new("VAR"),
                label: PropCell::new(None),
                label_position_offset: Cell::new((0.0, 0.0)),
                show_row_separators: Cell::new(false),
                default_text: RefCell::new(None),
                scroll_bar_h: sbh,
                scroll_bar_v: sbv,
                sel_index: Cell::new(None),
                sel_value: RefCell::new(None),
                sel_cx: Cell::new(0.0),
                sel_cy: Cell::new(0.0),
                sel_dragging: Cell::new(false),
                sel_drag_active: Cell::new(false),
                sel_drag_index: Cell::new(None),
                other_drag_source: Cell::new(false),
                hover_index: Cell::new(None),
                hover_value: RefCell::new(None),
                drag_target_list: RefCell::new(None),
                drag: RefCell::new(None),
                drag_target: RefCell::new(None),
            }
        })
    }

    fn len(&self) -> usize {
        self.list.borrow().len()
    }

    fn value_at(&self, index: usize) -> Option<V> {
        index.checked_sub(1).and_then(|i| self.list.borrow().get(i).cloned())
    }

    /// `ListControl:SelectIndex(index)` (ListControl.lua:91-112).
    pub fn select_index(&self, g: &mut Gfx, index: Option<usize>) -> bool {
        let value = index.and_then(|i| self.value_at(i));
        *self.sel_value.borrow_mut() = value.clone();
        let (Some(index), Some(value)) = (index, value) else { return false };
        if self.hooks.override_select_index(self, g, index) {
            return false;
        }
        self.sel_index.set(Some(index));
        let (_, height) = self.get_size(g);
        if self.scroll.is_some() {
            self.scroll_bar_v
                .set_content_dimension(g, self.len() as f64 * self.row_height, height - 4.0);
            self.scroll_bar_v
                .scroll_into_view((index as f64 - 2.0) * self.row_height, self.row_height * 3.0);
        }
        self.hooks.on_select(self, g, index, &value);
        true
    }

    /// ListControl.lua:126-134
    pub fn get_row_region(&self, g: &Gfx) -> Rect {
        let (width, height) = self.get_size(g);
        let cl = self.col_labels.get();
        Rect {
            x: 2.0,
            y: if cl { 20.0 } else { 2.0 },
            width: if self.scroll.is_some() { width - 20.0 } else { width },
            height: height - 4.0 - if self.scroll.is_some() && self.scroll_h { 16.0 } else { 0.0 } - if cl { 18.0 } else { 0.0 },
        }
    }

    fn mouse_over(&self, g: &Gfx) -> bool {
        self.is_shown(g) && (self.is_mouse_in_bounds(g) || self.host.get_mouse_over_control(g).is_some())
    }

    /// ListControl.lua:473-483
    pub fn get_hover_index(&self, g: &Gfx) -> Option<usize> {
        let (x, y) = self.get_pos(g);
        let (cursor_x, cursor_y) = g.get_cursor_pos();
        let r = self.get_row_region(g);
        if cursor_x >= x + r.x && cursor_y >= y + r.y && cursor_x < x + r.x + r.width && cursor_y < y + r.y + r.height {
            let index = ((cursor_y - y - r.y + self.scroll_bar_v.offset.get()) / self.row_height).floor() as i64 + 1;
            if index >= 1 && (index as usize) <= self.len() {
                return Some(index as usize);
            }
        }
        None
    }

    /// `self.dragTargetList = { … }`: dragging values onto other controls.
    pub fn set_drag_target_list(&self, targets: Vec<Rc<dyn DragTarget<V>>>) {
        *self.drag_target_list.borrow_mut() = Some(targets);
    }

    fn drag_targets(&self) -> Option<Vec<Rc<dyn DragTarget<V>>>> {
        self.drag_target_list.borrow().clone()
    }

    fn can_drag_to(&self, index: usize, value: &V) -> bool {
        self.hooks.has_can_drag_to_value() && self.hooks.can_drag_to_value(self, index, value)
    }
}

impl<V: Clone + PartialEq + 'static> Control for ListControl<V> {
    fn base(&self) -> &ControlBase {
        &self.base
    }

    /// ListControl.lua:118-124
    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        Some(self.mouse_over(g))
    }

    fn sel_drag_holds_focus(&self) -> bool {
        self.sel_drag_active.get() && self.drag_target_list.borrow().is_some()
    }

    fn has_draw(&self) -> bool {
        true
    }

    /// ListControl.lua:136-338
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, no_tooltip: bool) {
        let before = self.before_draw.borrow().clone();
        if let Some(b) = before {
            b(f);
        }
        let (x, y) = self.get_pos(f);
        let (width, height) = self.get_size(f);
        let row_height = self.row_height;
        let list = self.list.borrow().clone();
        let n = list.len();
        {
            let cols = self.col_list.borrow();
            let mut col_offset = 0.0;
            let last = cols.len();
            for (i, column) in cols.iter().enumerate() {
                column.offset.set(col_offset);
                let w = prop_get(&column.width, f).unwrap_or(if i + 1 == last && self.scroll.is_some() {
                    width - 20.0
                } else {
                    width - col_offset
                });
                column.width_now.set(w);
                col_offset += w;
            }
        }
        let sbv = Rc::clone(&self.scroll_bar_v);
        let row_region = self.get_row_region(f);
        sbv.set_content_dimension(f, n as f64 * row_height, row_region.height);
        let scroll_offset_v = sbv.offset.get();
        let sbh = Rc::clone(&self.scroll_bar_h);
        let (last_off, last_w) = {
            let cols = self.col_list.borrow();
            let c = cols.last().expect("colList");
            (c.offset.get(), c.width_now.get())
        };
        sbh.set_content_dimension(f, last_off + last_w, row_region.width);
        let scroll_offset_h = sbh.offset.get();
        let (cursor_x, cursor_y) = f.get_cursor_pos();
        if self.sel_value.borrow().is_some() && self.sel_dragging.get() && !self.sel_drag_active.get() {
            let (dx, dy) = (cursor_x - self.sel_cx.get(), cursor_y - self.sel_cy.get());
            if dx * dx + dy * dy > 100.0 {
                self.sel_drag_active.set(true);
                if let Some(targets) = self.drag_targets() {
                    let (si, sv) = (
                        self.sel_index.get().expect("selIndex"),
                        self.sel_value.borrow().clone().expect("selValue"),
                    );
                    let (ty, val) = self.hooks.get_drag_value(self, si, &sv);
                    for t in &targets {
                        if t.can_receive_drag(&ty, &val) {
                            t.set_other_drag_source(true);
                        }
                    }
                    *self.drag.borrow_mut() = Some((ty, val));
                }
            }
        }
        self.sel_drag_index.set(None);
        if (self.sel_drag_active.get() || self.other_drag_source.get())
            && self.is_mutable.get()
            && cursor_x >= x + 2.0
            && cursor_y >= y + 2.0
            && cursor_x < x + width - 20.0
            && cursor_y < y + height - 2.0
        {
            let index = ((cursor_y - y - 2.0 + scroll_offset_v) / row_height + 0.5).floor() as i64 + 1;
            let sel = self.sel_index.get().map(|s| s as i64);
            if sel.is_none_or(|s| index < s || index > s + 1) {
                self.sel_drag_index.set(Some(index.min(n as i64 + 1).max(0) as usize));
            }
        }
        if self.sel_drag_active.get()
            && let Some(targets) = self.drag_targets()
        {
            *self.drag_target.borrow_mut() = None;
            for t in targets {
                if self.drag_target.borrow().is_none() && t.other_drag_source() && t.is_mouse_over(f) {
                    t.set_other_drag_targeting(true);
                    *self.drag_target.borrow_mut() = Some(t);
                } else {
                    t.set_other_drag_targeting(false);
                }
            }
        }
        if let Some(label) = self.label.get(f) {
            let (ox, oy) = self.label_position_offset.get();
            sg::draw_string(f, x + ox, y - 20.0 + oy, Some("LEFT"), 16.0, self.font.get(), &label);
        }
        let other_src_no_cdv = self.other_drag_source.get() && !self.hooks.has_can_drag_to_value();
        if other_src_no_cdv {
            sg::set_draw_color(f, 0.2, 0.6, 0.2);
        } else if self.base.has_focus.get() {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
        } else {
            sg::set_draw_color(f, 0.5, 0.5, 0.5);
        }
        sg::draw_rect(f, x, y, width, height);
        if other_src_no_cdv {
            sg::set_draw_color(f, 0.0, 0.05, 0.0);
        } else {
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
        }
        sg::draw_rect(f, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        let me: Option<ControlRef> = if no_tooltip && !self.force_tooltip.get() {
            self.self_ref()
        } else {
            None
        };
        self.host.draw_controls(f, view_port, me.as_ref());
        let scroll = self.scroll.is_some();
        sg::set_viewport(
            f,
            Some([
                x + 2.0,
                y + 2.0,
                if scroll { width - 20.0 } else { width },
                height - 4.0 - if scroll && self.scroll_h { 16.0 } else { 0.0 },
            ]),
        );
        let text_offset_y = if self.show_row_separators.get() { 2.0 } else { 0.0 };
        let text_height = row_height - text_offset_y * 2.0;
        let mut tt: Option<(usize, V, f64, f64, f64)> = None;
        let min_index = (scroll_offset_v / row_height + 1.0).floor() as i64;
        let max_index = ((scroll_offset_v + height) / row_height + 1.0).floor().min(n as f64) as i64;
        let sel_value = self.sel_value.borrow().clone();
        let has_focus = self.base.has_focus.get();
        let cols = self.col_list.borrow();
        let cl = self.col_labels.get();
        let rel_x = cursor_x - (x + 2.0);
        let rel_y = cursor_y - (y + 2.0);
        for (ci, column) in cols.iter().enumerate() {
            let col_index = ci + 1;
            let col_font = column.font.unwrap_or("VAR");
            let clip_width = sg::draw_string_width(f, text_height, col_font, "...");
            let col_offset = column.offset.get() - scroll_offset_h;
            let col_width = column.width_now.get();
            for index in min_index.max(1)..=max_index {
                let index = index as usize;
                let line_y = row_height * (index as f64 - 1.0) - scroll_offset_v + if cl { 18.0 } else { 0.0 };
                let value = &list[index - 1];
                let mut text = self.hooks.get_row_value(self, f, col_index, index, value);
                let icon = self.hooks.get_row_icon(self, col_index, index, value);
                let mut text_width = sg::draw_string_width(f, text_height, col_font, &text);
                if text_width > col_width - 2.0 {
                    let clip_index = sg::draw_string_cursor_index(f, text_height, col_font, &text, col_width - clip_width - 2.0, 0.0);
                    text = format!("{}...", crate::lua::sub(&text, 1, clip_index as i64 - 1));
                    text_width = sg::draw_string_width(f, text_height, col_font, &text);
                }
                if !sbv.dragging.get()
                    && (!self.sel_drag_active.get() || self.can_drag_to(index, value))
                    && rel_x >= col_offset
                    && rel_x < if scroll { width - 20.0 } else { width }
                    && rel_y >= 0.0
                    && rel_y >= line_y
                    && rel_y < height - 2.0 - if scroll && self.scroll_h { 18.0 } else { 0.0 }
                    && rel_y < line_y + row_height
                {
                    tt = Some((
                        index,
                        value.clone(),
                        x + 2.0 + col_offset,
                        line_y + y + 2.0,
                        (text_width + 8.0).max(rel_x - col_offset),
                    ));
                }
                let is_sel = sel_value.as_ref() == Some(value);
                let is_tt = tt.as_ref().is_some_and(|t| &t.1 == value);
                let w = if !scroll { col_width - 4.0 } else { col_width };
                if self.show_row_separators.get() {
                    if has_focus && is_sel {
                        sg::set_draw_color(f, 1.0, 1.0, 1.0);
                    } else if is_tt {
                        sg::set_draw_color(f, 0.8, 0.8, 0.8);
                    } else {
                        sg::set_draw_color(f, 0.5, 0.5, 0.5);
                    }
                    sg::draw_rect(f, col_offset, line_y, w, row_height);
                    if is_sel || is_tt {
                        sg::set_draw_color(f, 0.33, 0.33, 0.33);
                    } else if self.other_drag_source.get() && self.can_drag_to(index, value) {
                        sg::set_draw_color(f, 0.0, 0.2, 0.0);
                    } else if index.is_multiple_of(2) {
                        sg::set_draw_color(f, 0.05, 0.05, 0.05);
                    } else {
                        sg::set_draw_color(f, 0.0, 0.0, 0.0);
                    }
                    sg::draw_rect(f, col_offset, line_y + 1.0, w, row_height - 2.0);
                } else if is_sel || is_tt {
                    if has_focus && is_sel {
                        sg::set_draw_color(f, 1.0, 1.0, 1.0);
                    } else if is_tt {
                        sg::set_draw_color(f, 0.8, 0.8, 0.8);
                    } else {
                        sg::set_draw_color(f, 0.5, 0.5, 0.5);
                    }
                    sg::draw_rect(f, col_offset, line_y, w, row_height);
                    if self.other_drag_source.get() && self.can_drag_to(index, value) {
                        sg::set_draw_color(f, 0.0, 0.2, 0.0);
                    } else {
                        sg::set_draw_color(f, 0.15, 0.15, 0.15);
                    }
                    sg::draw_rect(f, col_offset, line_y + 1.0, w, row_height - 2.0);
                }
                if !self.hooks.set_highlight_color(self, f, index, value) {
                    sg::set_draw_color(f, 1.0, 1.0, 1.0);
                }
                match icon {
                    None => sg::draw_string(f, col_offset, line_y + text_offset_y, Some("LEFT"), text_height, col_font, &text),
                    Some(icon) => {
                        sg::draw_image(f, Some(&icon), col_offset, line_y, 16.0, 16.0, None);
                        sg::draw_string(
                            f,
                            col_offset + 16.0 + 2.0,
                            line_y + text_offset_y,
                            Some("LEFT"),
                            text_height,
                            col_font,
                            &text,
                        );
                    }
                }
            }
            if cl {
                let m_over = rel_x >= col_offset && rel_x <= col_offset + col_width && (0.0..=18.0).contains(&rel_y);
                if m_over && column.sortable {
                    sg::set_draw_color(f, 1.0, 1.0, 1.0);
                    sg::draw_rect(f, col_offset, 1.0, col_width, 18.0);
                    sg::set_draw_color(f, 0.33, 0.33, 0.33);
                    sg::draw_rect(f, col_offset + 1.0, 2.0, col_width - 2.0, 16.0);
                } else {
                    sg::set_draw_color(f, 0.5, 0.5, 0.5);
                    sg::draw_rect(f, col_offset, 1.0, col_width, 18.0);
                    sg::set_draw_color(f, 0.15, 0.15, 0.15);
                    sg::draw_rect(f, col_offset + 1.0, 2.0, col_width - 2.0, 16.0);
                }
                if let Some(label) = prop_get(&column.label, f)
                    && !label.is_empty()
                {
                    sg::set_draw_color(f, 1.0, 1.0, 1.0);
                    sg::draw_string(f, col_offset + col_width / 2.0, 4.0, Some("CENTER_X"), 12.0, "VAR", &label);
                }
            }
        }
        drop(cols);
        if n == 0
            && let Some(dt) = self.default_text.borrow().clone()
        {
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            sg::draw_string(f, 2.0, 2.0, Some("LEFT"), 14.0, self.font.get(), &dt);
        }
        if let Some(sdi) = self.sel_drag_index.get() {
            let line_y = row_height * (sdi as f64 - 1.0) - scroll_offset_v;
            sg::set_draw_color(f, 1.0, 1.0, 1.0);
            sg::draw_rect(f, 0.0, line_y - 1.0, width - 20.0, 3.0);
            sg::set_draw_color(f, 0.0, 0.0, 0.0);
            sg::draw_rect(f, 0.0, line_y, width - 20.0, 1.0);
        }
        sg::set_viewport(f, None);
        if self.sel_drag_active.get() && self.drag_target_list.borrow().is_some() && (!self.is_mutable.get() || !self.mouse_over(f)) {
            let (si, sv) = (
                self.sel_index.get().expect("selIndex"),
                self.sel_value.borrow().clone().expect("selValue"),
            );
            crate::common::set_show_drag_text(self.hooks.get_row_value(self, f, 1, si, &sv));
        }
        self.hover_index.set(tt.as_ref().map(|t| t.0));
        *self.hover_value.borrow_mut() = tt.as_ref().map(|t| t.1.clone());
        if let Some((tt_index, tt_value, tt_x, tt_y, tt_width)) = tt
            && self.hooks.has_add_value_tooltip()
            && (!no_tooltip || self.force_tooltip.get())
        {
            sg::set_draw_layer(f, None, 100);
            let mut tooltip = self.tooltip.borrow_mut();
            self.hooks.add_value_tooltip(self, f, &mut tooltip, tt_index, &tt_value);
            tooltip.draw(f, tt_x, tt_y, Some((tt_width, row_height)), view_port);
            sg::set_draw_layer(f, None, 0);
        }
    }

    fn has_on_key_down(&self) -> bool {
        true
    }

    /// ListControl.lua:340-418
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if let Some(m) = self.host.get_mouse_over_control(g)
            && m.has_on_key_down()
        {
            return m.on_key_down(g, key, false);
        }
        if !self.mouse_over(g) && key.contains("BUTTON") {
            return None;
        }
        let n = self.len();
        if key == "LEFTBUTTON" {
            let mut new_select = None;
            let (x, y) = self.get_pos(g);
            let (cursor_x, cursor_y) = g.get_cursor_pos();
            let r = self.get_row_region(g);
            if cursor_x >= x + r.x && cursor_y >= y + r.y && cursor_x < x + r.x + r.width && cursor_y < y + r.y + r.height {
                let index = ((cursor_y - y - r.y + self.scroll_bar_v.offset.get()) / self.row_height).floor() as i64 + 1;
                if index >= 1 && index as usize <= n {
                    new_select = Some(index as usize);
                }
            } else {
                let scroll_offset_h = self.scroll_bar_h.offset.get();
                let cols: Vec<(f64, f64, bool)> = self
                    .col_list
                    .borrow()
                    .iter()
                    .map(|c| (c.offset.get(), c.width_now.get(), c.sortable))
                    .collect();
                for (ci, (off, w, sortable)) in cols.into_iter().enumerate() {
                    let rel_x = cursor_x - (x + 2.0);
                    let rel_y = cursor_y - (y + 2.0);
                    let adj = rel_x + scroll_offset_h;
                    let m_over = adj >= off && adj <= off + w && (0.0..=18.0).contains(&rel_y);
                    if sortable && m_over && self.hooks.has_re_sort() {
                        self.hooks.re_sort(&self, ci + 1);
                    }
                }
            }
            if self.select_index(g, new_select) {
                if (self.is_mutable.get() || self.drag_target_list.borrow().is_some()) && self.is_shown(g) {
                    self.sel_cx.set(cursor_x);
                    self.sel_cy.set(cursor_y);
                    self.sel_dragging.set(true);
                    self.sel_drag_active.set(false);
                }
                let (si, sv) = (
                    self.sel_index.get().expect("selIndex"),
                    self.sel_value.borrow().clone().expect("selValue"),
                );
                if self.hooks.on_sel_click(&self, g, si, &sv, double_click) == Some(false) {
                    return None;
                }
            }
        } else if n > 0 && !self.sel_drag_active.get() {
            let lua_mod = |a: i64, b: i64| a.rem_euclid(b);
            match key {
                "UP" => {
                    let i = lua_mod(self.sel_index.get().unwrap_or(1) as i64 - 2, n as i64) + 1;
                    self.select_index(g, Some(i as usize));
                }
                "DOWN" => {
                    let i = lua_mod(self.sel_index.get().unwrap_or(n) as i64, n as i64) + 1;
                    self.select_index(g, Some(i as usize));
                }
                "HOME" => {
                    self.select_index(g, Some(1));
                }
                "END" => {
                    self.select_index(g, Some(n));
                }
                _ => {
                    let sv = self.sel_value.borrow().clone();
                    if let Some(sv) = sv {
                        let si = self.sel_index.get().expect("selIndex");
                        if key == "c" && sg::is_key_down(g, "CTRL") {
                            self.hooks.on_sel_copy(&self, g, si, &sv);
                        } else if key == "x" && sg::is_key_down(g, "CTRL") {
                            self.hooks.on_sel_cut(&self, g, si, &sv);
                        } else if key == "BACK" || key == "DELETE" {
                            self.hooks.on_sel_delete(&self, g, si, &sv);
                        } else {
                            self.hooks.on_sel_key_down(&self, g, si, &sv, key);
                        }
                    }
                }
            }
            self.hooks.on_any_key_down(&self, g, key);
        }
        Some(self)
    }

    fn has_on_hover_key_up(&self) -> bool {
        self.hooks.has_on_hover_key_up()
    }

    fn on_hover_key_up(&self, g: &mut Gfx, key: &str) {
        self.hooks.on_hover_key_up(self, g, key);
    }

    fn has_on_key_up(&self) -> bool {
        true
    }

    /// ListControl.lua:420-471
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        let shift_h = self.scroll.is_some() && self.scroll_h && sg::is_key_down(g, "SHIFT");
        if ScrollBarControl::is_scroll_down_key(key) {
            if shift_h {
                self.scroll_bar_h.scroll(1.0)
            } else {
                self.scroll_bar_v.scroll(1.0)
            }
        } else if ScrollBarControl::is_scroll_up_key(key) {
            if shift_h {
                self.scroll_bar_h.scroll(-1.0)
            } else {
                self.scroll_bar_v.scroll(-1.0)
            }
        } else if self.sel_value.borrow().is_some() && key == "LEFTBUTTON" {
            self.sel_dragging.set(false);
            if self.sel_drag_active.get() {
                self.sel_drag_active.set(false);
                let si = self.sel_index.get().expect("selIndex");
                if let Some(mut sdi) = self.sel_drag_index.get()
                    && sdi != si
                {
                    let sv = self.sel_value.borrow().clone().expect("selValue");
                    {
                        let mut l = self.list.borrow_mut();
                        l.remove(si - 1);
                        if sdi > si {
                            sdi -= 1;
                        }
                        self.sel_drag_index.set(Some(sdi));
                        l.insert(sdi - 1, sv);
                    }
                    self.hooks.on_order_change(&self, si, sdi);
                    *self.sel_value.borrow_mut() = None;
                } else {
                    let target = self.drag_target.borrow().clone();
                    if let Some(target) = target {
                        let (ty, val) = self.drag.borrow().clone().expect("dragValue");
                        target.receive_drag(g, &ty, &val);
                        let sv = self.sel_value.borrow().clone();
                        if let Some(sv) = sv {
                            self.hooks.on_drag_send(&self, g, si, &sv);
                        }
                        *self.sel_value.borrow_mut() = None;
                    }
                }
                if let Some(targets) = self.drag_targets() {
                    for t in targets {
                        t.set_other_drag_source(false);
                        t.set_other_drag_targeting(false);
                    }
                }
                *self.drag.borrow_mut() = None;
                *self.drag_target.borrow_mut() = None;
            }
        }
        Some(self)
    }
}

impl<V: Clone + PartialEq + 'static> ListControl<V> {
    /// A strong reference to this list (for `DrawControls(viewPort, self)`): the scroll bar's anchor holds a weak one.
    fn self_ref(&self) -> Option<ControlRef> {
        self.scroll_bar_v.base.anchor.borrow().other.as_ref().and_then(Weak::upgrade)
    }
}

// ================================================================================================= TextListControl

/// A `TextListControl` line: `{ height, font?, x?, align?, underline?, [col] = text }`.
#[derive(Clone, Debug, Default)]
pub struct TextLine {
    pub height: f64,
    pub font: Option<&'static str>,
    pub x: Option<f64>,
    pub align: Option<&'static str>,
    pub underline: Vec<bool>,
    pub cols: Vec<Option<String>>,
}

/// `{ x, align }`.
#[derive(Clone, Copy, Debug)]
pub struct TextColumn {
    pub x: f64,
    pub align: &'static str,
}

pub struct TextListControl {
    pub base: ControlBase,
    pub host: ControlHost,
    pub scroll_bar: Rc<ScrollBarControl>,
    pub columns: RefCell<Vec<TextColumn>>,
    pub list: RefCell<Vec<TextLine>>,
    pub section_heights: RefCell<Option<Vec<f64>>>,
    /// `hoveredLine = { line, x, y, width }`: index into `list` (0-based) + `y`.
    pub hovered_line: Cell<Option<(usize, f64)>>,
    #[allow(clippy::type_complexity)]
    pub on_click: RefCell<Option<Rc<dyn Fn(&mut Gfx, Option<(usize, f64)>)>>>,
}

impl TextListControl {
    /// TextListControl.lua:9-21
    pub fn new(
        anchor: Option<Anchor>,
        rect: RectProps,
        columns: Option<Vec<TextColumn>>,
        list: Vec<TextLine>,
        section_heights: Option<Vec<f64>>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| {
            let base = ControlBase::new(anchor, rect);
            let host = ControlHost::new();
            let me_dyn: Weak<dyn Control> = me.clone();
            let sb = ScrollBarControl::new(
                anchor_weak("RIGHT", me_dyn, "RIGHT"),
                [(-1.0).into(), 0.0.into(), 18.0.into(), 0.0.into()],
                Some(40.0),
                None,
                false,
            );
            let m = me.clone();
            sb.base.height.set_fn(move |g| m.upgrade().expect("textlist").get_size(g).1 - 2.0);
            host.add("scrollBar", sb.clone());
            Self {
                base,
                host,
                scroll_bar: sb,
                columns: RefCell::new(columns.unwrap_or_else(|| vec![TextColumn { x: 0.0, align: "LEFT" }])),
                list: RefCell::new(list),
                section_heights: RefCell::new(section_heights),
                hovered_line: Cell::new(None),
                on_click: RefCell::new(None),
            }
        })
    }
}

impl Control for TextListControl {
    fn base(&self) -> &ControlBase {
        &self.base
    }

    /// TextListControl.lua:23-28
    fn is_mouse_over(&self, g: &Gfx) -> Option<bool> {
        Some(self.is_shown(g) && (self.is_mouse_in_bounds(g) || self.host.get_mouse_over_control(g).is_some()))
    }

    fn has_draw(&self) -> bool {
        true
    }

    /// TextListControl.lua:30-81
    fn draw(&self, f: &mut Frame<'_>, view_port: &Rect, _no_tooltip: bool) {
        let (x, y) = self.get_pos(f);
        let (width, height) = self.get_size(f);
        let sb = Rc::clone(&self.scroll_bar);
        let list = self.list.borrow().clone();
        let content_height: f64 = list.iter().map(|l| l.height).sum();
        sb.set_content_dimension(f, content_height, height - 4.0);
        sg::set_draw_color(f, 0.66, 0.66, 0.66);
        sg::draw_rect(f, x, y, width, height);
        sg::set_draw_color(f, 0.05, 0.05, 0.05);
        sg::draw_rect(f, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        self.host.draw_controls(f, view_port, None);
        sg::set_viewport(f, Some([x + 2.0, y + 2.0, width - 20.0, height - 4.0]));
        let columns = self.columns.borrow().clone();
        for (ci, col) in columns.iter().enumerate() {
            let mut line_y = -sb.offset.get();
            for line in &list {
                if let Some(Some(text)) = line.cols.get(ci) {
                    let mut text_x = line.x.unwrap_or(col.x);
                    let align = line.align.unwrap_or(col.align);
                    sg::draw_string(f, text_x, line_y, Some(align), line.height, line.font.unwrap_or("VAR"), text);
                    if line.underline.get(ci).copied().unwrap_or(false) {
                        let stripped = f.strip_escapes(text);
                        let w = sg::draw_string_width(f, line.height, "VAR", &stripped);
                        if align == "RIGHT_X" {
                            text_x -= w;
                        }
                        sg::set_draw_color(f, 0.5, 0.5, 0.5);
                        sg::draw_rect(f, text_x, line_y + line.height, w, 1.0);
                    }
                }
                line_y += line.height;
            }
        }
        self.hovered_line.set(None);
        let (cursor_x, cursor_y) = f.get_cursor_pos();
        if cursor_x >= x + 2.0 && cursor_x < x + width - 18.0 && cursor_y >= y + 2.0 && cursor_y < y + height - 2.0 {
            let mut row_y = y - sb.offset.get() + 2.0;
            for (i, line) in list.iter().enumerate() {
                if cursor_y >= row_y && cursor_y < row_y + line.height {
                    self.hovered_line.set(Some((i, row_y)));
                    break;
                }
                row_y += line.height;
            }
        }
        sg::set_viewport(f, None);
    }

    fn has_on_key_down(&self) -> bool {
        true
    }

    /// TextListControl.lua:83-94
    fn on_key_down(self: Rc<Self>, g: &mut Gfx, key: &str, _double_click: bool) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        if key == "LEFTBUTTON" {
            let oc = self.on_click.borrow().clone();
            if let Some(oc) = oc {
                oc(g, self.hovered_line.get());
            }
        }
        if let Some(m) = self.host.get_mouse_over_control(g)
            && m.has_on_key_down()
        {
            return m.on_key_down(g, key, false);
        }
        None
    }

    fn has_on_key_up(&self) -> bool {
        true
    }

    /// TextListControl.lua:96-123
    fn on_key_up(self: Rc<Self>, g: &mut Gfx, key: &str) -> Option<ControlRef> {
        if !self.is_shown(g) || !self.is_enabled(g) {
            return None;
        }
        let sb = Rc::clone(&self.scroll_bar);
        let sh = self.section_heights.borrow().clone();
        if sg::is_key_down(g, "SHIFT")
            && let Some(sh) = sh
        {
            let n = sh.len();
            for (i, &h) in sh.iter().enumerate() {
                if h >= sb.offset.get() {
                    if key == "WHEELDOWN" {
                        sb.set_offset(if i + 1 == n { sh[i] } else { sh[i + 1] });
                        return Some(self);
                    } else if key == "WHEELUP" {
                        sb.set_offset(if i == 0 { sh[i] } else { sh[i - 1] });
                        return Some(self);
                    }
                }
            }
            sb.offset.set(sb.offset_max.get().unwrap_or(f64::NAN));
            return Some(self);
        }
        if key == "WHEELDOWN" {
            sb.scroll(1.0);
            return Some(self);
        } else if key == "WHEELUP" {
            sb.scroll(-1.0);
            return Some(self);
        }
        None
    }
}
